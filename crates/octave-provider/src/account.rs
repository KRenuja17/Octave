//! Explicit, memory-only browser sessions. Credentials never reach playback URLs.
use crate::{JSON_LIMIT, Provider, WEB_UA, collect_tracks};
use octave_core::{Track, TrackId};
use reqwest::header::HeaderValue;
use serde_json::{Value, json};
use std::{
    fmt,
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;
const ORIGIN: &str = "https://music.youtube.com";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidSession,
    Expired,
    Network,
    Unexpected,
    Rejected,
    Cancelled,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
        Self::InvalidSession => "Paste a valid YouTube Music Cookie header value containing SAPISID or __Secure-3PAPISID.",
        Self::Expired => "The account session was rejected or expired. Disconnect and connect a fresh browser session.",
        Self::Network => "Account request failed. Check your connection and retry.",
        Self::Unexpected => "The account response format was not recognized. No changes were applied locally.",
        Self::Rejected => "YouTube did not confirm that change. Your local like state has been left unchanged.",
        Self::Cancelled => "Account request cancelled.",
    })
    }
}
struct Credentials {
    cookie: Zeroizing<String>,
    sapisid: Zeroizing<String>,
    index: u8,
    revoked: AtomicBool,
}
#[derive(Clone)]
pub struct Account {
    credentials: Arc<Credentials>,
}
impl fmt::Debug for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Account([REDACTED])")
    }
}
pub use octave_core::Collection;
#[derive(Clone, Debug, Default)]
pub struct Page {
    pub tracks: Vec<Track>,
    pub collections: Vec<Collection>,
    pub continuation: Option<String>,
}
impl Account {
    pub fn new(cookie: String, index: u8) -> Result<Self, Error> {
        let cookie = Zeroizing::new(cookie);
        if cookie.len() > 16384
            || cookie.contains(['\r', '\n'])
            || HeaderValue::from_str(&cookie).is_err()
        {
            return Err(Error::InvalidSession);
        }
        let sapisid = cookie
            .split(';')
            .filter_map(|p| p.trim().split_once('='))
            .find(|(name, value)| {
                matches!(*name, "SAPISID" | "__Secure-3PAPISID") && !value.is_empty()
            })
            .map(|(_, value)| Zeroizing::new(value.to_owned()))
            .ok_or(Error::InvalidSession)?;
        Ok(Self {
            credentials: Arc::new(Credentials {
                cookie,
                sapisid,
                index,
                revoked: AtomicBool::new(false),
            }),
        })
    }
    pub fn disconnect(&self) {
        self.credentials.revoked.store(true, Ordering::Release);
    }
    fn active(&self) -> Result<(), Error> {
        if self.credentials.revoked.load(Ordering::Acquire) {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
    fn request(&self, endpoint: &str, mut body: Value) -> Result<Value, Error> {
        self.active()?;
        let version = "1.20260707.12.00";
        body["context"] = json!({"client":{"clientName":"WEB_REMIX","clientVersion":version,"hl":"en"},"user":{}});
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::InvalidSession)?
            .as_secs();
        let input = Zeroizing::new(format!(
            "{timestamp} {} {ORIGIN}",
            self.credentials.sapisid.as_str()
        ));
        let digest =
            ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, input.as_bytes());
        let hash: String = digest.as_ref().iter().map(|b| format!("{b:02x}")).collect();
        let mut authorization = HeaderValue::from_str(&format!("SAPISIDHASH {timestamp}_{hash}"))
            .map_err(|_| Error::InvalidSession)?;
        authorization.set_sensitive(true);
        let mut cookie =
            HeaderValue::from_str(&self.credentials.cookie).map_err(|_| Error::InvalidSession)?;
        cookie.set_sensitive(true);
        let provider = Provider::new().map_err(|_| Error::Network)?;
        self.active()?;
        let response = provider
            .client
            .post(format!("{ORIGIN}/youtubei/v1/{endpoint}?prettyPrint=false"))
            .header("Origin", ORIGIN)
            .header("X-Origin", ORIGIN)
            .header("User-Agent", WEB_UA)
            .header("X-YouTube-Client-Name", "67")
            .header("X-YouTube-Client-Version", version)
            .header("X-Goog-AuthUser", self.credentials.index.to_string())
            .header("Authorization", authorization)
            .header("Cookie", cookie)
            .json(&body)
            .send()
            .map_err(|_| Error::Network)?;
        if matches!(response.status().as_u16(), 401 | 403) {
            return Err(Error::Expired);
        }
        if !response.status().is_success() {
            return Err(Error::Network);
        }
        let mut bytes = Vec::new();
        response
            .take(JSON_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Network)?;
        self.active()?;
        if bytes.len() as u64 > JSON_LIMIT {
            return Err(Error::Unexpected);
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| Error::Unexpected)?;
        if value
            .pointer("/responseContext/mainAppWebResponseContext/loggedOut")
            .and_then(Value::as_bool)
            == Some(true)
        {
            return Err(Error::Expired);
        }
        if value.get("error").is_some() {
            return Err(Error::Rejected);
        }
        Ok(value)
    }
    pub fn browse(&self, id: &str, continuation: Option<&str>) -> Result<Page, Error> {
        if id.len() > 256
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
        {
            return Err(Error::Unexpected);
        }
        let body = if let Some(token) = continuation {
            if token.len() > 16384 {
                return Err(Error::Unexpected);
            }
            json!({"continuation":token})
        } else {
            json!({"browseId":id})
        };
        let value = self.request("browse", body)?;
        parse_page(&value)
    }
    pub fn rate(&self, id: &TrackId, liked: bool) -> Result<(), Error> {
        if !crate::valid_id(&id.0) {
            return Err(Error::Unexpected);
        }
        let value = self.request(
            if liked {
                "like/like"
            } else {
                "like/removelike"
            },
            json!({"target":{"videoId":id.0}}),
        )?;
        if value.get("status").and_then(Value::as_str) == Some("STATUS_SUCCEEDED") {
            Ok(())
        } else {
            Err(Error::Rejected)
        }
    }
}
fn text(value: &Value) -> String {
    value
        .get("simpleText")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            value
                .get("runs")
                .and_then(Value::as_array)
                .map(|runs| runs.iter().filter_map(|r| r["text"].as_str()).collect())
                .unwrap_or_default()
        })
}
pub(crate) fn parse_page(value: &Value) -> Result<Page, Error> {
    if ![
        "contents",
        "continuationContents",
        "onResponseReceivedActions",
        "onResponseReceivedEndpoints",
    ]
    .iter()
    .any(|k| value.get(k).is_some())
    {
        return Err(Error::Unexpected);
    }
    let mut page = Page::default();
    collect_tracks(value, &mut page.tracks);
    walk(value, &mut page);
    Ok(page)
}
fn walk(value: &Value, page: &mut Page) {
    if let Some(row) = value.get("musicTwoRowItemRenderer") {
        let id = row
            .pointer("/navigationEndpoint/browseEndpoint/browseId")
            .and_then(Value::as_str);
        let title = text(&row["title"]);
        if let Some(id) = id.filter(|id| id.starts_with("VL") || id.starts_with("MPRE"))
            && !title.is_empty()
            && !page.collections.iter().any(|c| c.id == id)
        {
            page.collections.push(Collection {
                id: id.into(),
                title,
            });
        }
    }
    if page.continuation.is_none() {
        page.continuation = value
            .pointer("/nextContinuationData/continuation")
            .or_else(|| value.pointer("/continuationCommand/token"))
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
    match value {
        Value::Object(map) => {
            for child in map.values() {
                walk(child, page);
            }
        }
        Value::Array(items) => {
            for child in items {
                walk(child, page);
            }
        }
        _ => (),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sessions_redact_validate_and_revoke_shared_handles() {
        assert!(Account::new("SAPISID=secret\r\nInjected: yes".into(), 0).is_err());
        assert!(Account::new("other=secret".into(), 0).is_err());
        let account = Account::new("SAPISID=secret; SID=other".into(), 0).unwrap();
        assert!(!format!("{account:?}").contains("secret"));
        let clone = account.clone();
        account.disconnect();
        assert_eq!(clone.active(), Err(Error::Cancelled));
    }
    #[test]
    fn collection_pages_keep_cursor_and_ignore_unrecognized_responses() {
        let row = json!({"musicTwoRowItemRenderer":{"title":{"runs":[{"text":"An album"}]},"navigationEndpoint":{"browseEndpoint":{"browseId":"MPRE_example"}}}});
        let page = parse_page(&json!({"contents":[row.clone(),row,{"nextContinuationData":{"continuation":"next-page"}}]})).unwrap();
        assert_eq!(page.collections.len(), 1);
        assert_eq!(page.continuation.as_deref(), Some("next-page"));
        assert!(parse_page(&json!({"responseContext":{}})).is_err());
    }
}
