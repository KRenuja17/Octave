//! Public player assets only. Account cookies and complete media URLs never
//! cross the worker boundary. Evaluation has no host APIs and Windows job limits.
use crate::{Error, Provider, transport};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

const PLAYER_LIMIT: usize = 6 * 1024 * 1024;
const VALUE_LIMIT: usize = 8192;
#[cfg(windows)]
const WORKER_DEADLINE: Duration = Duration::from_secs(20);

pub(crate) struct Player {
    source: String,
    pub timestamp: u32,
    loaded: Instant,
}

fn check(cancelled: &impl Fn() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

pub(crate) fn player(
    provider: &Provider,
    cancelled: impl Fn() -> bool,
) -> Result<Arc<Player>, Error> {
    static CACHE: OnceLock<Mutex<Option<Arc<Player>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let started = Instant::now();
    let mut guard = loop {
        check(&cancelled)?;
        if let Ok(guard) = cache.try_lock() {
            break guard;
        }
        if started.elapsed() >= Duration::from_secs(25) {
            return Err(Error::Busy);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if let Some(player) = guard
        .as_ref()
        .filter(|p| p.loaded.elapsed() < Duration::from_secs(3600))
    {
        return Ok(player.clone());
    }
    let bytes = public_asset(
        provider,
        "https://www.youtube.com/iframe_api",
        64 * 1024,
        &cancelled,
    )?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| Error::InvalidResponse)?
        .replace("\\/", "/");
    let hash = player_hash(&text)?;
    let source = String::from_utf8(public_asset(
        provider,
        &format!("https://www.youtube.com/s/player/{hash}/player_ias.vflset/en_US/base.js"),
        PLAYER_LIMIT,
        &cancelled,
    )?)
    .map_err(|_| Error::InvalidResponse)?;
    let timestamp = timestamp(&source)?;
    let player = Arc::new(Player {
        source,
        timestamp,
        loaded: Instant::now(),
    });
    *guard = Some(player.clone());
    Ok(player)
}

fn public_asset(
    provider: &Provider,
    url: &str,
    limit: usize,
    cancelled: impl Fn() -> bool,
) -> Result<Vec<u8>, Error> {
    // Both URLs are constructed here, not supplied by an account response. The
    // shared client has no cookie jar; redirects and session headers are absent.
    let request = provider
        .transport
        .client
        .get(url)
        .build()
        .map_err(crate::network)?;
    provider.transport.bytes(
        request,
        transport::Expected::Bytes(limit),
        transport::Lane::Media,
        transport::REQUEST_TIMEOUT,
        cancelled,
    )
}

fn player_hash(text: &str) -> Result<&str, Error> {
    let (_, suffix) = text
        .split_once("/s/player/")
        .ok_or(Error::InvalidResponse)?;
    let (hash, _) = suffix.split_once('/').ok_or(Error::InvalidResponse)?;
    if hash.len() != 8 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::InvalidResponse);
    }
    Ok(hash)
}

fn timestamp(text: &str) -> Result<u32, Error> {
    let mut result = None;
    for suffix in text.split("signatureTimestamp").skip(1) {
        let Some(digits) = suffix.trim_start().strip_prefix(':').map(str::trim_start) else {
            continue;
        };
        let digits: String = digits
            .chars()
            .take_while(char::is_ascii_digit)
            .take(7)
            .collect();
        let Ok(value) = digits.parse::<u32>() else {
            continue;
        };
        if !(10_000..=100_000).contains(&value) {
            return Err(Error::InvalidResponse);
        }
        if result.is_some_and(|prior| prior != value) {
            return Err(Error::InvalidResponse);
        }
        result = Some(value);
    }
    result.ok_or(Error::InvalidResponse)
}

pub(crate) fn decode(
    player: &Player,
    n: Option<&str>,
    signature: Option<&str>,
    cancelled: impl Fn() -> bool,
) -> Result<Value, Error> {
    check(&cancelled)?;
    for value in [n, signature].into_iter().flatten() {
        if value.is_empty() || value.len() > VALUE_LIMIT {
            return Err(Error::InvalidResponse);
        }
    }
    if n.is_none() && signature.is_none() {
        return Ok(json!({}));
    }
    // One bounded helper process at a time, including transition preparation.
    static EVALUATING: Mutex<()> = Mutex::new(());
    let started = Instant::now();
    let _guard = loop {
        check(&cancelled)?;
        if let Ok(guard) = EVALUATING.try_lock() {
            break guard;
        }
        if started.elapsed() > Duration::from_secs(25) {
            return Err(Error::Busy);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let input = serde_json::to_vec(&json!({"player":player.source,"n":n,"signature":signature}))
        .map_err(|_| Error::SignatureWorker)?;
    let output = worker(&input, cancelled)?;
    let value: Value = serde_json::from_slice(&output).map_err(|_| Error::SignatureWorker)?;
    let object = value.as_object().ok_or(Error::SignatureWorker)?;
    if object.len() != usize::from(n.is_some()) + usize::from(signature.is_some()) {
        return Err(Error::SignatureWorker);
    }
    for (key, expected) in [("n", n.is_some()), ("signature", signature.is_some())] {
        if expected
            && !object
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(valid_output)
        {
            return Err(Error::SignatureWorker);
        }
    }
    Ok(value)
}

fn valid_output(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= VALUE_LIMIT
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.=+/".contains(&b))
}

pub(crate) fn resolve_response(
    id: &octave_core::TrackId,
    mut response: Value,
    decode: impl FnOnce(Option<&str>, Option<&str>) -> Result<Value, Error>,
) -> Result<crate::ResolvedStream, Error> {
    if response
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str)
        != Some("OK")
    {
        return crate::parse_stream(id, &response);
    }
    if response
        .pointer("/videoDetails/videoId")
        .and_then(Value::as_str)
        != Some(&id.0)
    {
        return Err(Error::InvalidResponse);
    }
    let formats = response
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)
        .ok_or(Error::UnsupportedStream)?;
    let mut audio = formats
        .iter()
        .filter(|audio| {
            audio["mimeType"].as_str().is_some_and(|mime| {
                mime.split(';').next().map(str::trim) == Some("audio/mp4")
                    && mime
                        .split(';')
                        .skip(1)
                        .any(|p| p.trim() == "codecs=\"mp4a.40.2\"")
            }) && audio.get("drmFamilies").is_none()
                && audio.get("drmTrackType").is_none()
                && (audio["url"].is_string()
                    || audio["signatureCipher"].is_string()
                    || audio["cipher"].is_string())
        })
        .max_by_key(|audio| crate::reported_bitrate(audio).unwrap_or(0))
        .ok_or(Error::UnsupportedStream)?
        .clone();
    let (mut url, signature, parameter) = if let Some(url) = audio["url"].as_str() {
        (
            reqwest::Url::parse(url).map_err(|_| Error::InvalidResponse)?,
            None,
            None,
        )
    } else {
        let cipher = audio["signatureCipher"]
            .as_str()
            .or_else(|| audio["cipher"].as_str())
            .ok_or(Error::UnsupportedStream)?;
        cipher_url(cipher)?
    };
    crate::validate_media_url(&url)?;
    let n = unique_query(&url, "n")?;
    if signature.is_some() || n.is_some() {
        let values = decode(n.as_deref(), signature.as_deref())?;
        let mut pairs: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(key, _)| key != "n")
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        if n.is_some() {
            pairs.push((
                "n".into(),
                values["n"]
                    .as_str()
                    .filter(|v| valid_output(v))
                    .ok_or(Error::SignatureWorker)?
                    .into(),
            ));
        }
        if let Some(parameter) = parameter {
            if pairs.iter().any(|(key, _)| *key == parameter) {
                return Err(Error::InvalidResponse);
            }
            pairs.push((
                parameter,
                values["signature"]
                    .as_str()
                    .filter(|v| valid_output(v))
                    .ok_or(Error::SignatureWorker)?
                    .into(),
            ));
        }
        url.set_query(None);
        url.query_pairs_mut().extend_pairs(pairs);
    }
    crate::validate_media_url(&url)?;
    audio["url"] = url.to_string().into();
    response["streamingData"]["adaptiveFormats"] = json!([audio]);
    crate::parse_stream(id, &response)
}

fn unique_query(url: &reqwest::Url, key: &str) -> Result<Option<String>, Error> {
    let mut values = url.query_pairs().filter(|(name, _)| name == key);
    let first = values.next().map(|(_, value)| value.into_owned());
    if values.next().is_some() {
        return Err(Error::InvalidResponse);
    }
    Ok(first)
}

fn cipher_url(cipher: &str) -> Result<(reqwest::Url, Option<String>, Option<String>), Error> {
    if cipher.len() > 32768 {
        return Err(Error::InvalidResponse);
    }
    let parsed = reqwest::Url::parse(&format!("https://fixture.invalid/?{cipher}"))
        .map_err(|_| Error::InvalidResponse)?;
    if parsed
        .query_pairs()
        .any(|(key, _)| !["url", "s", "sp"].contains(&key.as_ref()))
    {
        return Err(Error::InvalidResponse);
    }
    let raw = unique_query(&parsed, "url")?.ok_or(Error::InvalidResponse)?;
    let url = reqwest::Url::parse(&raw).map_err(|_| Error::InvalidResponse)?;
    crate::validate_media_url(&url)?;
    let signature = unique_query(&parsed, "s")?
        .filter(|s| valid_output(s))
        .ok_or(Error::InvalidResponse)?;
    let parameter = unique_query(&parsed, "sp")?.unwrap_or_else(|| "signature".into());
    if !["sig", "signature"].contains(&parameter.as_str()) {
        return Err(Error::InvalidResponse);
    }
    Ok((url, Some(signature), Some(parameter)))
}

#[cfg(not(windows))]
fn worker(_: &[u8], _: impl Fn() -> bool) -> Result<Vec<u8>, Error> {
    // Fail closed until an equivalent native resource boundary is qualified.
    Err(Error::SignatureWorker)
}

#[cfg(windows)]
fn worker(input: &[u8], cancelled: impl Fn() -> bool) -> Result<Vec<u8>, Error> {
    use std::{
        fs::File,
        io::{Read, Write},
    };
    use win_custody::{Job, JobLimits, Launch, WaitOutcome, anonymous_pipe, null_device};
    let map = |_| Error::SignatureWorker;
    let current = std::env::current_exe().map_err(map)?;
    let mut directory = current.parent().ok_or(Error::SignatureWorker)?;
    // Cargo's attended test executable lives in target/{profile}/deps.
    if directory.file_name().is_some_and(|name| name == "deps") {
        directory = directory.parent().ok_or(Error::SignatureWorker)?;
    }
    let executable = directory.join("octave-signature.exe");
    if !executable.is_file() {
        return Err(Error::SignatureWorker);
    }
    let limits = JobLimits::new()
        .kill_on_close(true)
        .active_processes(1)
        .process_memory(512 * 1024 * 1024);
    let job = Job::create(&limits).map_err(map)?;
    if job.limits().map_err(map)? != limits {
        return Err(Error::SignatureWorker);
    }
    let (input_read, input_write) = anonymous_pipe().map_err(map)?;
    let (output_read, output_write) = anonymous_pipe().map_err(map)?;
    let null = null_device().map_err(map)?;
    let child = Launch::new(&executable)
        .arg("--stdio-v1")
        .env_clear()
        .env(
            "SystemRoot",
            std::env::var_os("SystemRoot").unwrap_or_default(),
        )
        .current_dir(directory)
        .create_no_window(true)
        .stdin(&input_read)
        .stdout(&output_write)
        .stderr(&null)
        .spawn_in_job(&job)
        .map_err(map)?;
    drop(input_read);
    drop(output_write);
    let input = input.to_owned();
    let writer = std::thread::spawn(move || File::from(input_write).write_all(&input));
    let reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        File::from(output_read)
            .take((2 * VALUE_LIMIT + 1024 + 1) as u64)
            .read_to_end(&mut output)
            .map(|_| output)
    });
    let started = Instant::now();
    let status = loop {
        if cancelled() {
            break Err(Error::Cancelled);
        }
        if started.elapsed() > WORKER_DEADLINE {
            break Err(Error::SignatureWorker);
        }
        match child.wait(Some(Duration::from_millis(20))) {
            Ok(WaitOutcome::Signaled) => {
                break child.exit_code().map_err(map).and_then(|code| {
                    if code == Some(0) {
                        Ok(())
                    } else {
                        Err(Error::SignatureWorker)
                    }
                });
            }
            Ok(_) => (),
            Err(_) => break Err(Error::SignatureWorker),
        }
    };
    // Terminate before joining pipe threads on every path, including cancellation,
    // overlong output, parser panic and engine allocation failures.
    let _ = job.terminate(70);
    drop(job);
    let written = writer.join().map_err(|_| Error::SignatureWorker)?;
    let output = reader
        .join()
        .map_err(|_| Error::SignatureWorker)?
        .map_err(map)?;
    status?;
    written.map_err(map)?;
    if output.len() > 2 * VALUE_LIMIT + 1024 {
        return Err(Error::SignatureWorker);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_asset_identity_and_timestamp_are_bounded() {
        assert_eq!(
            player_hash("https://www.youtube.com/s/player/11a45ffb/widget.js").unwrap(),
            "11a45ffb"
        );
        assert!(player_hash("/s/player/../../evil").is_err());
        assert_eq!(
            timestamp("{signatureTimestamp:21343, x:{signatureTimestamp:21343}}").unwrap(),
            21343
        );
        assert!(timestamp("signatureTimestamp:21343, signatureTimestamp:21344").is_err());
        assert!(timestamp("signatureTimestamp:99999999999999").is_err());
        assert!(!valid_output("value\nextra"));
        assert!(!valid_output("space value"));
    }
    #[test]
    fn highest_offered_aac_is_deciphered_without_account_data() {
        let id = octave_core::TrackId("abcdefghijk".into());
        let mut query = reqwest::Url::parse("https://fixture.invalid").unwrap();
        query
            .query_pairs_mut()
            .append_pair(
                "url",
                "https://r1.googlevideo.com/videoplayback?n=synthetic-n&keep=value",
            )
            .append_pair("s", "synthetic-signature")
            .append_pair("sp", "sig");
        let cipher = query.query().unwrap();
        let base = json!({"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","contentLength":"1024","approxDurationMs":"5000"});
        let mut low = base.clone();
        low["url"] = "https://r1.googlevideo.com/videoplayback".into();
        low["bitrate"] = 128000.into();
        let mut high = base;
        high["signatureCipher"] = cipher.into();
        high["bitrate"] = 256000.into();
        let response = json!({"playabilityStatus":{"status":"OK"},"videoDetails":{"videoId":id.0},"streamingData":{"adaptiveFormats":[low,high]}});
        let stream = resolve_response(&id, response.clone(), |n, s| {
            assert_eq!(n, Some("synthetic-n"));
            assert_eq!(s, Some("synthetic-signature"));
            Ok(json!({"n":"decoded-n","signature":"decoded-sig"}))
        })
        .unwrap();
        assert_eq!(stream.bitrate_bps, Some(256000));
        let url = reqwest::Url::parse(stream.url.expose()).unwrap();
        assert_eq!(
            unique_query(&url, "n").unwrap().as_deref(),
            Some("decoded-n")
        );
        assert_eq!(
            unique_query(&url, "sig").unwrap().as_deref(),
            Some("decoded-sig")
        );
        assert_eq!(
            unique_query(&url, "keep").unwrap().as_deref(),
            Some("value")
        );
        assert!(!format!("{stream:?}").contains("decoded"));
        let mut mismatch = response.clone();
        mismatch["videoDetails"]["videoId"] = "wrongwrongw".into();
        assert!(matches!(
            resolve_response(&id, mismatch, |_, _| panic!(
                "must not decode mismatched track"
            )),
            Err(Error::InvalidResponse)
        ));
        let mut drm = response;
        drm["streamingData"]["adaptiveFormats"][1]["drmFamilies"] = json!(["WIDEVINE"]);
        assert_eq!(
            resolve_response(&id, drm, |_, _| panic!("must not decode DRM"))
                .unwrap()
                .bitrate_bps,
            Some(128000)
        );
    }
    #[test]
    fn cipher_and_cdn_validation_reject_ambiguity_and_foreign_targets() {
        for cipher in [
            "url=https%3A%2F%2Fr1.googlevideo.com%2Fvideoplayback&s=x&s=y",
            "url=https%3A%2F%2Fattacker.invalid%2Fvideoplayback&s=x",
            "url=https%3A%2F%2Fr1.googlevideo.com%2Fvideoplayback&s=x&sp=n",
        ] {
            assert!(cipher_url(cipher).is_err());
        }
        for url in [
            "http://r1.googlevideo.com/videoplayback",
            "https://r1.googlevideo.com:8443/videoplayback",
            "https://r1.googlevideo.com/other",
            "https://r1.googlevideo.com/videoplayback#extra",
            "https://r1.googlevideo.com.attacker.invalid/videoplayback",
        ] {
            assert!(crate::validate_media_url(&reqwest::Url::parse(url).unwrap()).is_err());
        }
    }
    #[test]
    #[ignore = "Native Windows helper qualification with a public player asset only; build octave-signature and set OCTAVE_PUBLIC_PLAYER"]
    fn native_public_player_worker() {
        let path = std::env::var_os("OCTAVE_PUBLIC_PLAYER").expect("Public asset path required");
        let source = std::fs::read_to_string(path).unwrap();
        assert!(source.len() <= PLAYER_LIMIT);
        let player = Player {
            timestamp: timestamp(&source).unwrap(),
            source,
            loaded: Instant::now(),
        };
        let output = decode(
            &player,
            Some("synthetic-throttle"),
            Some("synthetic-signature"),
            || false,
        )
        .unwrap();
        assert!(output["n"].as_str().is_some_and(valid_output));
        assert!(output["signature"].as_str().is_some_and(valid_output));
        let started = Instant::now();
        assert!(matches!(
            decode(&player, Some("synthetic"), None, || started.elapsed()
                > Duration::from_millis(50)),
            Err(Error::Cancelled)
        ));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
