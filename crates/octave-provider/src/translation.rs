//! Explicit LibreTranslate-compatible requests; no implicit public endpoint.
use crate::{Error, network};
use octave_core::lyrics::Lyrics;
use reqwest::{Url, blocking::Client};
use serde_json::{Value, json};
use std::{
    io::Read,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub struct Translator {
    endpoint: Url,
    client: Client,
    key: Zeroizing<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranslatedLyrics {
    pub original: Lyrics,
    pub target: String,
    pub lines: Vec<String>,
}
pub fn valid_endpoint(endpoint: &str) -> bool {
    endpoint_url(endpoint).is_some()
}
fn endpoint_url(endpoint: &str) -> Option<Url> {
    let mut url = Url::parse(endpoint.trim()).ok()?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let local = matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return None;
    }
    if !url.path().trim_end_matches('/').ends_with("/translate") {
        let path = format!("{}/translate", url.path().trim_end_matches('/'));
        url.set_path(&path);
    }
    Some(url)
}
impl Translator {
    pub fn new(endpoint: &str, key: String) -> Result<Self, Error> {
        let key = Zeroizing::new(key);
        if key.len() > 512 {
            return Err(Error::InvalidResponse);
        }
        Ok(Self {
            endpoint: endpoint_url(endpoint).ok_or(Error::InvalidResponse)?,
            key,
            client: Client::builder()
                .timeout(Duration::from_secs(8))
                .connect_timeout(Duration::from_secs(4))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("Octave/0.2 lyrics translation")
                .build()
                .map_err(network)?,
        })
    }
    pub fn translate(
        &self,
        original: &Lyrics,
        target: &str,
        cancelled: impl Fn() -> bool,
    ) -> Result<TranslatedLyrics, Error> {
        if !(2..=12).contains(&target.len())
            || !target.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
        {
            return Err(Error::InvalidResponse);
        }
        let mut lines: Vec<String> = if original.cues.is_empty() {
            original.text.lines().map(str::to_owned).collect()
        } else {
            original.cues.iter().map(|cue| cue.text.clone()).collect()
        };
        if lines.len() > 4096 || original.text.len() > 65536 {
            return Err(Error::InvalidResponse);
        }
        let indices: Vec<_> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.chars().any(|c| c.is_alphanumeric()))
            .map(|(i, _)| i)
            .collect();
        let mut batches: Vec<Vec<usize>> = Vec::new();
        let mut batch = Vec::new();
        let mut count = 0;
        for index in indices {
            let chars = lines[index].chars().count();
            if chars > 4000 {
                return Err(Error::InvalidResponse);
            }
            if !batch.is_empty() && (batch.len() == 24 || count + chars > 4000) {
                batches.push(std::mem::take(&mut batch));
                count = 0;
            }
            batch.push(index);
            count += chars;
        }
        if !batch.is_empty() {
            batches.push(batch);
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        for batch in &batches {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            if batch
                .iter()
                .any(|&index| lines[index].chars().count() > 4000)
            {
                return Err(Error::InvalidResponse);
            }
            let input: Vec<_> = batch.iter().map(|&index| lines[index].as_str()).collect();
            let mut body = json!({"q":input,"source":"auto","target":target,"format":"text"});
            if !self.key.is_empty() {
                body["api_key"] = json!(self.key.as_str());
            }
            let response = self
                .client
                .post(self.endpoint.clone())
                .json(&body)
                .send()
                .map_err(network)?;
            if !response.status().is_success() {
                return Err(Error::Http(response.status().as_u16()));
            }
            let mut bytes = Vec::new();
            response
                .take(256 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| Error::Network)?;
            if bytes.len() > 256 * 1024 {
                return Err(Error::InvalidResponse);
            }
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|_| Error::InvalidResponse)?;
            let translated = parse_lines(&value, batch.len())?;
            if cancelled() {
                return Err(Error::Cancelled);
            }
            for (&index, line) in batch.iter().zip(translated) {
                lines[index] = line;
            }
        }
        if lines.iter().map(String::len).sum::<usize>() > 131072 {
            return Err(Error::InvalidResponse);
        }
        Ok(TranslatedLyrics {
            original: original.clone(),
            target: target.into(),
            lines,
        })
    }
}
fn parse_lines(value: &Value, count: usize) -> Result<Vec<String>, Error> {
    let lines = value["translatedText"]
        .as_array()
        .ok_or(Error::InvalidResponse)?;
    if lines.len() != count {
        return Err(Error::InvalidResponse);
    }
    lines
        .iter()
        .map(|line| {
            line.as_str()
                .filter(|s| !s.trim().is_empty() && s.len() <= 16384)
                .map(str::to_owned)
                .ok_or(Error::InvalidResponse)
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_http_translation_keeps_cue_identity_and_instrumental_lines() {
        use std::io::{BufRead, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = std::io::BufReader::new(socket.try_clone().unwrap());
            let mut size = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                assert!(
                    !line.to_ascii_lowercase().starts_with("cookie:")
                        && !line.to_ascii_lowercase().starts_with("authorization:")
                );
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    size = value.trim().parse::<usize>().unwrap();
                }
            }
            let mut bytes = vec![0; size];
            reader.read_exact(&mut bytes).unwrap();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["q"], json!(["Original alpha", "Original beta"]));
            assert_eq!(body["api_key"], "fixture-key");
            let payload = r#"{"translatedText":["Translated alpha","Translated beta"]}"#;
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len()).unwrap();
        });
        let original = Lyrics {
            track_id: octave_core::TrackId("fixture".into()),
            text: "Original alpha\n♪\nOriginal beta".into(),
            source: "Original fixture".into(),
            cues: vec![
                octave_core::lyrics::LyricCue {
                    id: 0,
                    text: "Original alpha".into(),
                    start_ms: 1000,
                    end_ms: 2000,
                },
                octave_core::lyrics::LyricCue {
                    id: 1,
                    text: "♪".into(),
                    start_ms: 2000,
                    end_ms: 3000,
                },
                octave_core::lyrics::LyricCue {
                    id: 2,
                    text: "Original beta".into(),
                    start_ms: 3000,
                    end_ms: 4000,
                },
            ],
        };
        let translator = Translator::new(&endpoint, "fixture-key".into()).unwrap();
        let result = translator.translate(&original, "es", || false).unwrap();
        worker.join().unwrap();
        assert_eq!(result.original, original);
        assert_eq!(
            result.lines,
            vec!["Translated alpha", "♪", "Translated beta"]
        );
        assert_eq!(
            translator.translate(&original, "es", || true),
            Err(Error::Cancelled)
        );
    }
    #[test]
    fn endpoints_are_explicit_and_alignment_is_never_guessed() {
        assert!(valid_endpoint("https://translate.example.com"));
        assert!(valid_endpoint("http://127.0.0.1:5000"));
        for url in [
            "http://translate.example.com",
            "https://key@example.com",
            "https://example.com?api_key=secret",
            "file:///tmp/translate",
            "",
        ] {
            assert!(!valid_endpoint(url));
        }
        assert_eq!(
            parse_lines(&json!({"translatedText":["Uno","Dos"]}), 2).unwrap(),
            vec!["Uno", "Dos"]
        );
        for value in [
            json!({"translatedText":"Merged lines"}),
            json!({"translatedText":["Only one"]}),
            json!({"translatedText":["Uno",null]}),
        ] {
            assert_eq!(parse_lines(&value, 2), Err(Error::InvalidResponse));
        }
    }
}
