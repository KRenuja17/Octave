//! Experimental unsigned YouTube Music access. No cookies or account state are read.
pub mod account;
pub mod lyrics;
pub mod playlist;
pub mod range;
pub mod related;
pub mod translation;
use octave_core::{Track, TrackId, diagnostics::Sensitive};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::{fmt, io::Read, time::Duration};
const WEB_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";
const PLAYER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const JSON_LIMIT: u64 = 4 * 1024 * 1024;
/// Errors omit response bodies, credentials and signed media URLs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Network,
    Timeout,
    Http(u16),
    InvalidResponse,
    SignInRequired,
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
   Self::Timeout => f.write_str("Network request timed out. Retry playback."),
   Self::Http(403) => f.write_str("YouTube rejected the stream (403). Retry to resolve a fresh URL."),
   Self::Http(code) => write!(f, "YouTube returned HTTP {code}."),
   Self::InvalidResponse => f.write_str("YouTube returned an unexpected response."),
   Self::SignInRequired => f.write_str("YouTube requires sign-in for this request. Account playback is not available yet."),
   Self::Unavailable => f.write_str("This track is unavailable to this unsigned client."),
   Self::UnsupportedStream => f.write_str("No direct AAC stream is available. This prototype cannot resolve ciphered streams."),
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
    client: Client,
}
impl Provider {
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
        let response = self
            .client
            .get(format!(
                "https://i.ytimg.com/vi/{}/{}.jpg",
                id.0,
                if large { "maxresdefault" } else { "hqdefault" }
            ))
            .timeout(Duration::from_secs(5))
            .send()
            .map_err(network)?;
        if !response.status().is_success() {
            return Err(Error::Http(response.status().as_u16()));
        }
        let mut bytes = Vec::new();
        response
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Network)?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(Error::InvalidResponse);
        }
        Ok(bytes)
    }
    pub fn new() -> Result<Self, Error> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(WEB_UA)
            .build()
            .map_err(network)?;
        Ok(Self { client })
    }
    fn post(
        &self,
        url: &str,
        body: Value,
        client_id: &str,
        version: &str,
        ua: &str,
    ) -> Result<Value, Error> {
        let mut request = self
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
        let response = request.send().map_err(network)?;
        if !response.status().is_success() {
            return Err(Error::Http(response.status().as_u16()));
        }
        let mut bytes = Vec::new();
        response
            .take(JSON_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Network)?;
        if bytes.len() as u64 > JSON_LIMIT {
            return Err(Error::InvalidResponse);
        }
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
    /// Cancellation is checked between requests; each request has a 12s timeout.
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
        let visitor = self.post("https://www.youtube.com/youtubei/v1/visitor_id?prettyPrint=false",
   json!({"context":{"client":{"clientName":"WEB","clientVersion":"2.20260707.00.00","hl":"en"}}}), "1", "2.20260707.00.00", WEB_UA)?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let visitor = visitor
            .pointer("/responseContext/visitorData")
            .and_then(Value::as_str)
            .ok_or(Error::InvalidResponse)?;
        let value = self.post("https://www.youtube.com/youtubei/v1/player?prettyPrint=false",
   json!({"context":{"client":{"clientName":"VISIONOS","clientVersion":"1.02","deviceMake":"Apple","deviceModel":"RealityDevice17,1","osName":"visionOS","osVersion":"26.5.23O471","hl":"en","visitorData":visitor}},"videoId":id.0}), "101", "1.02", PLAYER_UA)?;
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
        Some("LOGIN_REQUIRED") => return Err(Error::SignInRequired),
        Some(_) => return Err(Error::Unavailable),
        None => return Err(Error::InvalidResponse),
    }
    let formats = value
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)
        .ok_or(Error::UnsupportedStream)?;
    let audio = formats
        .iter()
        .find(|v| v["itag"] == 140 && v["url"].is_string())
        .ok_or(Error::UnsupportedStream)?;
    let mime = audio["mimeType"].as_str().ok_or(Error::UnsupportedStream)?;
    if mime.split(';').next().map(str::trim) != Some("audio/mp4")
        || !mime
            .split(';')
            .skip(1)
            .any(|part| part.trim() == "codecs=\"mp4a.40.2\"")
    {
        return Err(Error::UnsupportedStream);
    }
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
        bitrate_bps: audio["averageBitrate"]
            .as_u64()
            .or_else(|| audio["bitrate"].as_u64())
            .filter(|n| (1000..=1_000_000).contains(n))
            .map(|n| n as u32),
        track_id: id.clone(),
        url: Sensitive::new(url.to_owned()),
        content_length: number("contentLength")?,
        duration: Duration::from_millis(number("approxDurationMs")?),
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
