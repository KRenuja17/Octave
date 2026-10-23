//! Experimental unsigned YouTube Music access. No cookies or account state are read.
pub mod account;
pub mod catalog;
#[cfg(test)]
mod compatibility_tests;
pub mod lyrics;
pub mod playlist;
pub mod range;
pub mod related;
pub mod translation;
mod transport;
use octave_core::{Track, TrackId, diagnostics::Sensitive};
use serde_json::{Value, json};
use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
const WEB_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";
const PLAYER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const JSON_LIMIT: u64 = 4 * 1024 * 1024;
/// Errors omit response bodies, credentials and signed media URLs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Network,
    Busy,
    Timeout,
    Http(u16),
    InvalidResponse,
    SignInRequired,
    BotCheckRequired,
    Unavailable,
    UnsupportedStream,
    InvalidTrack,
    Cancelled,
    InvalidRange,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
   Self::Network => f.write_str("Network request failed. Check your connection and retry."),
   Self::Busy => f.write_str("Network workers are busy. Retry shortly."),
   Self::Timeout => f.write_str("Network request timed out. Retry playback."),
   Self::Http(403) => f.write_str("YouTube rejected the stream (403). Retry to resolve a fresh URL."),
   Self::Http(code) => write!(f, "YouTube returned HTTP {code}."),
   Self::InvalidResponse => f.write_str("YouTube returned an unexpected response."),
   Self::SignInRequired => f.write_str("YouTube requires sign-in for this request. Account playback is not available yet."),
   Self::BotCheckRequired => f.write_str("YouTube blocked this unsigned request with a bot-verification check. Try another recording or retry later."),
   Self::Unavailable => f.write_str("This track is unavailable to this unsigned client."),
   Self::UnsupportedStream => f.write_str("No supported direct AAC stream was offered. Ciphered streams and manifest-only playback are not supported."),
   Self::InvalidTrack => f.write_str("Enter an 11-character YouTube video ID."),
   Self::Cancelled => f.write_str("Playback cancelled."),
   Self::InvalidRange => f.write_str("The audio server did not return the requested byte range."),
  }
    }
}
impl std::error::Error for Error {}
fn network(error: reqwest::Error) -> Error {
    if error.is_timeout() {
        Error::Timeout
    } else {
        Error::Network
    }
}
#[derive(Debug)]
pub struct ResolvedStream {
    pub bitrate_bps: Option<u32>,
    pub track_id: TrackId,
    pub url: Sensitive<String>,
    pub content_length: u64,
    pub duration: Duration,
}
#[derive(Clone)]
pub struct Provider {
    transport: Arc<transport::Transport>,
    cancelled: Option<Arc<AtomicBool>>,
}
impl Provider {
    /// A per-operation token. It never changes another provider/operation.
    pub fn with_cancellation(mut self, cancelled: Arc<AtomicBool>) -> Self {
        self.cancelled = Some(cancelled);
        self
    }
    fn is_cancelled(&self) -> bool {
        self.cancelled
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    }
    pub fn public_artwork(&self, url: &str) -> Result<Vec<u8>, Error> {
        let parsed = reqwest::Url::parse(url).map_err(|_| Error::InvalidResponse)?;
        if parsed.scheme() != "https"
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || url.len() > 2048
            || !parsed.host_str().is_some_and(|host| {
                host.ends_with(".googleusercontent.com") || host.ends_with(".ytimg.com")
            })
        {
            return Err(Error::InvalidResponse);
        }
        let request = self.transport.client.get(parsed).build().map_err(network)?;
        self.transport.bytes(
            request,
            transport::Expected::Bytes(2 * 1024 * 1024),
            transport::Lane::Metadata,
            Duration::from_secs(5),
            || self.is_cancelled(),
        )
    }
    /// Public video artwork only; never sends account credentials.
    pub fn thumbnail(&self, id: &TrackId) -> Result<Vec<u8>, Error> {
        self.thumbnail_tier(id, false)
    }
    /// A bounded larger artwork tier for Focus. Missing high-res art falls back.
    pub fn thumbnail_large(&self, id: &TrackId) -> Result<Vec<u8>, Error> {
        match self.thumbnail_tier(id, true) {
            Err(Error::Http(404)) => self.thumbnail(id),
            result => result,
        }
    }
    fn thumbnail_tier(&self, id: &TrackId, large: bool) -> Result<Vec<u8>, Error> {
        if id.0.len() != 11
            || !id
                .0
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err(Error::InvalidTrack);
        }
        let request = self
            .transport
            .client
            .get(format!(
                "https://i.ytimg.com/vi/{}/{}.jpg",
                id.0,
                if large { "maxresdefault" } else { "hqdefault" }
            ))
            .build()
            .map_err(network)?;
        self.transport.bytes(
            request,
            transport::Expected::Bytes(2 * 1024 * 1024),
            transport::Lane::Metadata,
            Duration::from_secs(5),
            || self.is_cancelled(),
        )
    }
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            transport: transport::shared()?,
            cancelled: None,
        })
    }
    fn post(
        &self,
        url: &str,
        body: Value,
        client_id: &str,
        version: &str,
        ua: &str,
    ) -> Result<Value, Error> {
        self.json_reply(
            self.post_request(url, body, client_id, version, ua)?,
            transport::Lane::Metadata,
            || false,
        )
    }
    fn post_request(
        &self,
        url: &str,
        body: Value,
        client_id: &str,
        version: &str,
        ua: &str,
    ) -> Result<reqwest::Request, Error> {
        let mut request = self
            .transport
            .client
            .post(url)
            .header("Origin", "https://music.youtube.com")
            .header("X-YouTube-Client-Name", client_id)
            .header("X-YouTube-Client-Version", version)
            .header("User-Agent", ua)
            .json(&body);
        if let Some(visitor) = body
            .pointer("/context/client/visitorData")
            .and_then(Value::as_str)
        {
            request = request.header("X-Goog-Visitor-Id", visitor);
        }
        request.build().map_err(network)
    }
    fn json_reply(
        &self,
        request: reqwest::Request,
        lane: transport::Lane,
        cancelled: impl Fn() -> bool,
    ) -> Result<Value, Error> {
        let bytes = self.transport.bytes(
            request,
            transport::Expected::Bytes(JSON_LIMIT as usize),
            lane,
            transport::REQUEST_TIMEOUT,
            || self.is_cancelled() || cancelled(),
        )?;
        serde_json::from_slice(&bytes).map_err(|_| Error::InvalidResponse)
    }
    pub fn search(&self, query: &str) -> Result<Vec<Track>, Error> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        if query.len() > 512 {
            return Err(Error::InvalidResponse);
        }
        let version = "1.20260707.12.00";
        let value = self.post("https://music.youtube.com/youtubei/v1/search?prettyPrint=false",
   json!({"context":{"client":{"clientName":"WEB_REMIX","clientVersion":version,"hl":"en"}},"query":query}), "67", version, WEB_UA)?;
        let mut tracks = Vec::new();
        collect_tracks(&value, &mut tracks);
        tracks.truncate(40);
        Ok(tracks)
    }
    pub fn resolve(&self, id: &TrackId) -> Result<ResolvedStream, Error> {
        self.resolve_cancellable(id, || false)
    }
    /// Cancellation aborts pending HTTP within a 20ms polling interval. Each
    /// visitor/player transaction has a 12s absolute deadline including its body.
    pub fn resolve_cancellable(
        &self,
        id: &TrackId,
        cancelled: impl Fn() -> bool,
    ) -> Result<ResolvedStream, Error> {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if !valid_id(&id.0) {
            return Err(Error::InvalidTrack);
        }
        let visitor = self.json_reply(self.post_request("https://www.youtube.com/youtubei/v1/visitor_id?prettyPrint=false",
   json!({"context":{"client":{"clientName":"WEB","clientVersion":"2.20260707.00.00","hl":"en"}}}), "1", "2.20260707.00.00", WEB_UA)?, transport::Lane::Media, &cancelled)?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let visitor = visitor
            .pointer("/responseContext/visitorData")
            .and_then(Value::as_str)
            .ok_or(Error::InvalidResponse)?;
        let value = self.json_reply(self.post_request("https://www.youtube.com/youtubei/v1/player?prettyPrint=false",
   json!({"context":{"client":{"clientName":"VISIONOS","clientVersion":"1.02","deviceMake":"Apple","deviceModel":"RealityDevice17,1","osName":"visionOS","osVersion":"26.5.23O471","hl":"en","visitorData":visitor}},"videoId":id.0}), "101", "1.02", PLAYER_UA)?, transport::Lane::Media, &cancelled)?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        parse_stream(id, &value)
    }
}
fn valid_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn parse_stream(id: &TrackId, value: &Value) -> Result<ResolvedStream, Error> {
    match value
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str)
    {
        Some("OK") => (),
        Some("LOGIN_REQUIRED") => {
            let reason = value
                .pointer("/playabilityStatus/reason")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase();
            return Err(
                if reason.contains("not a bot") || reason.contains("automated") {
                    Error::BotCheckRequired
                } else {
                    Error::SignInRequired
                },
            );
        }
        Some(_) => return Err(Error::Unavailable),
        None => return Err(Error::InvalidResponse),
    }
    let formats = value
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)
        .ok_or(Error::UnsupportedStream)?;
    let audio = formats
        .iter()
        .filter(|v| {
            v["url"].is_string()
                && v["mimeType"].as_str().is_some_and(|mime| {
                    mime.split(';').next().map(str::trim) == Some("audio/mp4")
                        && mime
                            .split(';')
                            .skip(1)
                            .any(|part| part.trim() == "codecs=\"mp4a.40.2\"")
                })
        })
        .max_by_key(|v| reported_bitrate(v).unwrap_or(0))
        .ok_or(Error::UnsupportedStream)?;
    let url = audio["url"].as_str().ok_or(Error::UnsupportedStream)?;
    let parsed = reqwest::Url::parse(url).map_err(|_| Error::InvalidResponse)?;
    if parsed.scheme() != "https"
        || !parsed
            .host_str()
            .is_some_and(|h| h.ends_with(".googlevideo.com"))
    {
        return Err(Error::InvalidResponse);
    }
    let number = |key: &str| {
        audio[key]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|n| *n > 0)
            .ok_or(Error::InvalidResponse)
    };
    Ok(ResolvedStream {
        bitrate_bps: reported_bitrate(audio),
        track_id: id.clone(),
        url: Sensitive::new(url.to_owned()),
        content_length: number("contentLength")?,
        duration: Duration::from_millis(number("approxDurationMs")?),
    })
}
fn reported_bitrate(audio: &Value) -> Option<u32> {
    ["averageBitrate", "bitrate"].into_iter().find_map(|key| {
        audio[key]
            .as_u64()
            .filter(|n| (1000..=1_000_000).contains(n))
            .map(|n| n as u32)
    })
}
fn collect_tracks(value: &Value, out: &mut Vec<Track>) {
    if out.len() >= 2000 {
        return;
    }
    // Search often puts the best match in a card rather than a list row.
    if let Some(card) = value.get("musicCardShelfRenderer")
        && let Some(runs) = card.pointer("/title/runs").and_then(Value::as_array)
        && let Some(id) = runs.iter().find_map(|r| {
            r.pointer("/navigationEndpoint/watchEndpoint/videoId")
                .and_then(Value::as_str)
        })
        && valid_id(id)
        && !out.iter().any(|t| t.id.0 == id)
    {
        let title: String = runs.iter().filter_map(|r| r["text"].as_str()).collect();
        if !title.is_empty() {
            out.push(Track {
                id: TrackId(id.to_owned()),
                title,
                artists: Vec::new(),
            });
        }
    }
    if let Some(row) = value.get("musicResponsiveListItemRenderer")
        && let Some(columns) = row["flexColumns"].as_array()
    {
        let runs = columns
            .first()
            .and_then(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs"))
            .and_then(Value::as_array);
        if let Some(runs) = runs {
            let id = row
                .pointer("/playlistItemData/videoId")
                .and_then(Value::as_str)
                .or_else(|| {
                    runs.iter().find_map(|r| {
                        r.pointer("/navigationEndpoint/watchEndpoint/videoId")
                            .and_then(Value::as_str)
                    })
                });
            if let Some(id) = id.filter(|id| valid_id(id))
                && !out.iter().any(|t| t.id.0 == id)
            {
                let title: String = runs.iter().filter_map(|r| r["text"].as_str()).collect();
                let artists = columns
                    .get(1)
                    .and_then(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs"))
                    .and_then(Value::as_array)
                    .map(|runs| {
                        runs.iter()
                            .filter(|r| {
                                r.pointer("/navigationEndpoint/browseEndpoint/browseId")
                                    .and_then(Value::as_str)
                                    .is_some_and(|id| id.starts_with("UC"))
                            })
                            .filter_map(|r| r["text"].as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                if !title.is_empty() {
                    out.push(Track {
                        id: TrackId(id.to_owned()),
                        title,
                        artists,
                    });
                }
            }
        }
    }
    match value {
        Value::Object(map) => {
            for child in map.values() {
                collect_tracks(child, out);
            }
        }
        Value::Array(array) => {
            for child in array {
                collect_tracks(child, out);
            }
        }
        _ => (),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn operation_cancellation_aborts_metadata_without_canceling_another_operation() {
        use std::{
            io::{BufRead, Read, Write},
            net::TcpListener,
            sync::mpsc,
            thread,
        };
        let provider = Provider {
            transport: Arc::new(transport::Transport::new().unwrap()),
            cancelled: None,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/fixture", listener.local_addr().unwrap());
        let (ready, wait) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = std::io::BufReader::new(socket.try_clone().unwrap());
            loop {
                let mut line = String::new();
                request.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n{")
                .unwrap();
            ready.send(()).unwrap();
            assert_eq!(socket.read(&mut [0; 1]).unwrap(), 0);
        });
        let flag = Arc::new(AtomicBool::new(false));
        let cancel = flag.clone();
        let trigger = thread::spawn(move || {
            wait.recv_timeout(Duration::from_secs(2)).unwrap();
            cancel.store(true, Ordering::Release);
        });
        let scoped = provider.clone().with_cancellation(flag);
        let started = std::time::Instant::now();
        assert_eq!(
            scoped.json_reply(
                scoped.transport.client.get(url).build().unwrap(),
                transport::Lane::Metadata,
                || false
            ),
            Err(Error::Cancelled)
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        trigger.join().unwrap();
        server.join().unwrap();
        assert!(!provider.is_cancelled());
        assert!(scoped.is_cancelled());
    }
    #[test]
    fn highest_direct_supported_aac_is_selected_from_offered_formats() {
        let id = TrackId("abcdefghijk".into());
        let base = json!({"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","url":"https://r1.googlevideo.com/videoplayback","contentLength":"1024","approxDurationMs":"5000","bitrate":128000});
        let mut high = base.clone();
        high["bitrate"] = json!(256000);
        let mut unsupported = base.clone();
        unsupported["mimeType"] = json!("audio/webm; codecs=\"opus\"");
        unsupported["bitrate"] = json!(512000);
        let mut cipher = high.clone();
        cipher.as_object_mut().unwrap().remove("url");
        cipher["signatureCipher"] = json!("secret");
        cipher["bitrate"] = json!(320000);
        let value = json!({"playabilityStatus":{"status":"OK"},"streamingData":{"adaptiveFormats":[base, high, unsupported, cipher]}});
        assert_eq!(parse_stream(&id, &value).unwrap().bitrate_bps, Some(256000));
    }
    #[test]
    fn selected_quality_uses_reported_values_and_rejects_unexpected_codecs() {
        let id = TrackId("abcdefghijk".into());
        let mut value = json!({"playabilityStatus":{"status":"OK"},"streamingData":{"adaptiveFormats":[{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","url":"https://r1.googlevideo.com/videoplayback?token=secret","contentLength":"1024","approxDurationMs":"5000","averageBitrate":129450,"bitrate":140000}]}});
        assert_eq!(parse_stream(&id, &value).unwrap().bitrate_bps, Some(129450));
        let audio = &mut value["streamingData"]["adaptiveFormats"][0];
        audio.as_object_mut().unwrap().remove("averageBitrate");
        assert_eq!(parse_stream(&id, &value).unwrap().bitrate_bps, Some(140000));
        value["streamingData"]["adaptiveFormats"][0]["bitrate"] = json!(u64::MAX);
        assert_eq!(parse_stream(&id, &value).unwrap().bitrate_bps, None);
        value["streamingData"]["adaptiveFormats"][0]["mimeType"] =
            json!("audio/webm; codecs=\"opus\"");
        assert_eq!(
            parse_stream(&id, &value).unwrap_err(),
            Error::UnsupportedStream
        );
        assert!(!format!("{:?}", parse_stream(&id, &value)).contains("secret"));
    }
    #[test]
    fn search_includes_top_result_cards() {
        let card = json!({"musicCardShelfRenderer":{"title":{"runs":[{"text":"Best match","navigationEndpoint":{"watchEndpoint":{"videoId":"abcdefghijk"}}}]}}});
        let mut out = Vec::new();
        collect_tracks(&json!([card.clone(), card]), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "Best match");
    }
    #[test]
    fn restricted_and_ciphered_streams_are_explicit_errors() {
        let id = TrackId("abcdefghijk".into());
        assert_eq!(
            parse_stream(
                &id,
                &json!({"playabilityStatus":{"status":"LOGIN_REQUIRED","reason":"secret"}})
            )
            .unwrap_err(),
            Error::SignInRequired
        );
        assert_eq!(parse_stream(&id, &json!({"playabilityStatus":{"status":"OK"},"streamingData":{"adaptiveFormats":[{"itag":140,"signatureCipher":"secret"}]}})).unwrap_err(), Error::UnsupportedStream);
    }
    #[test]
    fn bot_verification_is_distinct_from_account_required_and_remains_redacted() {
        let id = TrackId("abcdefghijk".into());
        for reason in [
            "Sign in to confirm you're not a bot. secret",
            "SIGN IN TO CONFIRM YOU ARE NOT A BOT. secret",
            "Automated queries detected. secret",
        ] {
            let error = parse_stream(
                &id,
                &json!({"playabilityStatus":{"status":"LOGIN_REQUIRED","reason":reason}}),
            )
            .unwrap_err();
            assert_eq!(error, Error::BotCheckRequired);
            assert!(error.to_string().contains("bot-verification"));
            assert!(!format!("{error:?} {error}").contains("secret"));
        }
        for reason in ["Sign in to confirm your age", "Private video", "secret", ""] {
            assert_eq!(
                parse_stream(
                    &id,
                    &json!({"playabilityStatus":{"status":"LOGIN_REQUIRED","reason":reason}})
                )
                .unwrap_err(),
                Error::SignInRequired
            );
        }
        assert_eq!(
            parse_stream(
                &id,
                &json!({"playabilityStatus":{"status":"LOGIN_REQUIRED"}})
            )
            .unwrap_err(),
            Error::SignInRequired
        );
        assert_eq!(
            parse_stream(
                &id,
                &json!({"playabilityStatus":{"status":"UNPLAYABLE","reason":"not a bot"}})
            )
            .unwrap_err(),
            Error::Unavailable
        );
    }
    #[test]
    fn stream_debug_redacts_url_and_rejects_foreign_hosts() {
        let id = TrackId("abcdefghijk".into());
        let mut value = json!({"playabilityStatus":{"status":"OK"},"streamingData":{"adaptiveFormats":[{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","url":"https://r1.googlevideo.com/videoplayback?token=secret","contentLength":"1024","approxDurationMs":"5000"}]}});
        assert!(!format!("{:?}", parse_stream(&id, &value).unwrap()).contains("secret"));
        value["streamingData"]["adaptiveFormats"][0]["url"] =
            json!("https://googlevideo.com.attacker.test/audio");
        assert_eq!(
            parse_stream(&id, &value).unwrap_err(),
            Error::InvalidResponse
        );
    }
    #[test]
    fn search_deduplicates_tracks_and_ignores_browse_only_rows() {
        let row = json!({"musicResponsiveListItemRenderer":{"playlistItemData":{"videoId":"abcdefghijk"},"flexColumns":[{"musicResponsiveListItemFlexColumnRenderer":{"text":{"runs":[{"text":"Example"}]}}}]}});
        let mut out = Vec::new();
        collect_tracks(
            &json!([row.clone(), row, {"musicResponsiveListItemRenderer":{"flexColumns":[]}}]),
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "Example");
    }
}
