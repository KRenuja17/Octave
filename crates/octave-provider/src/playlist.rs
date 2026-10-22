//! Complete, finite public playlists. Partial results never masquerade as a queue.
use crate::{Error, Provider, WEB_UA, collect_tracks};
use octave_core::Track;
use serde_json::{Value, json};
pub const MAX_PUBLIC_PLAYLIST: usize = 10_000;
#[derive(Default)]
pub struct PlaylistPage {
    pub tracks: Vec<Track>,
    pub skipped: usize,
    pub continuation: Option<String>,
}
pub struct CompletePlaylist {
    pub tracks: Vec<Track>,
    pub skipped: usize,
    pub pages: usize,
}
pub fn playlist_id(input: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(input).ok();
    let raw = if let Some(url) = parsed {
        if !matches!(
            url.host_str(),
            Some("music.youtube.com" | "www.youtube.com" | "youtube.com")
        ) {
            return None;
        }
        url.query_pairs()
            .find(|(key, _)| key == "list")?
            .1
            .into_owned()
    } else {
        input
            .trim()
            .strip_prefix("VL")
            .unwrap_or(input.trim())
            .to_owned()
    };
    ((raw.starts_with("PL") || raw.starts_with("OLAK"))
        && (8..=128).contains(&raw.len())
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')))
    .then_some(raw)
}
impl Provider {
    pub fn playlist_page(
        &self,
        id: &str,
        continuation: Option<&str>,
    ) -> Result<PlaylistPage, Error> {
        let id = playlist_id(id).ok_or(Error::InvalidResponse)?;
        let version = "1.20260707.12.00";
        let mut body = json!({"context":{"client":{"clientName":"WEB_REMIX","clientVersion":version,"hl":"en"}}});
        if let Some(token) = continuation {
            if token.len() > 16384 {
                return Err(Error::InvalidResponse);
            }
            body["continuation"] = json!(token);
        } else {
            body["browseId"] = json!(format!("VL{id}"));
        }
        let value = self.post(
            "https://music.youtube.com/youtubei/v1/browse?prettyPrint=false",
            body,
            "67",
            version,
            WEB_UA,
        )?;
        parse_page(&value)
    }
    pub fn complete_playlist(
        &self,
        id: &str,
        cancelled: impl Fn() -> bool,
        progress: impl FnMut(usize, usize),
    ) -> Result<CompletePlaylist, Error> {
        collect_pages(|token| self.playlist_page(id, token), cancelled, progress)
    }
}
fn token(value: &Value) -> Option<String> {
    if let Some(token) = value
        .pointer("/nextContinuationData/continuation")
        .or_else(|| value.pointer("/continuationCommand/token"))
        .and_then(Value::as_str)
    {
        return Some(token.into());
    }
    match value {
        Value::Object(map) => map.values().find_map(token),
        Value::Array(items) => items.iter().find_map(token),
        _ => None,
    }
}
fn shelf(value: &Value) -> Option<&Value> {
    for key in [
        "musicPlaylistShelfRenderer",
        "musicPlaylistShelfContinuation",
        "appendContinuationItemsAction",
    ] {
        if let Some(root) = value.get(key) {
            return Some(root);
        }
    }
    match value {
        Value::Object(map) => map.values().find_map(shelf),
        Value::Array(items) => items.iter().find_map(shelf),
        _ => None,
    }
}
fn rows(value: &Value, page: &mut PlaylistPage) {
    if value.get("musicResponsiveListItemRenderer").is_some() {
        let mut track = Vec::new();
        collect_tracks(value, &mut track);
        if track.is_empty() {
            page.skipped += 1;
        } else {
            page.tracks.extend(track);
        } // retain duplicates between rows
        return;
    }
    match value {
        Value::Object(map) => map.values().for_each(|child| rows(child, page)),
        Value::Array(items) => items.iter().for_each(|child| rows(child, page)),
        _ => (),
    }
}
fn parse_page(value: &Value) -> Result<PlaylistPage, Error> {
    let root = shelf(value).ok_or(Error::InvalidResponse)?;
    let mut page = PlaylistPage {
        continuation: token(root),
        ..Default::default()
    };
    rows(root, &mut page);
    Ok(page)
}
fn collect_pages(
    mut fetch: impl FnMut(Option<&str>) -> Result<PlaylistPage, Error>,
    cancelled: impl Fn() -> bool,
    mut progress: impl FnMut(usize, usize),
) -> Result<CompletePlaylist, Error> {
    let mut result = CompletePlaylist {
        tracks: Vec::new(),
        skipped: 0,
        pages: 0,
    };
    let mut next = None;
    let mut seen = std::collections::HashSet::new();
    loop {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let page = fetch(next.as_deref())?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        result.tracks.extend(page.tracks);
        result.skipped += page.skipped;
        result.pages += 1;
        if result.tracks.len() + result.skipped > MAX_PUBLIC_PLAYLIST || result.pages > 200 {
            return Err(Error::InvalidResponse);
        }
        progress(result.tracks.len(), result.skipped);
        match page.continuation {
            Some(token)
                if !token.is_empty() && token.len() <= 16384 && seen.insert(token.clone()) =>
            {
                next = Some(token)
            }
            Some(_) => return Err(Error::InvalidResponse),
            None => return Ok(result),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Opt-in public playlist pagination; no sign-in or audio"]
    fn live_public_playlist_continuation() {
        let provider = Provider::new().unwrap();
        let id = "PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI";
        let first = provider.playlist_page(id, None).unwrap();
        let token = first
            .continuation
            .as_deref()
            .expect("Public reference playlist has another page");
        let second = provider.playlist_page(id, Some(token)).unwrap();
        println!(
            "public_playlist_first={} second={} unavailable={}",
            first.tracks.len(),
            second.tracks.len(),
            first.skipped + second.skipped
        );
        assert!(!first.tracks.is_empty() && !second.tracks.is_empty());
        assert_ne!(first.continuation, second.continuation);
        let all = provider.complete_playlist(id, || false, |_, _| {}).unwrap();
        println!(
            "complete_public_playlist={} pages={}",
            all.tracks.len(),
            all.pages
        );
        assert!(all.tracks.len() >= first.tracks.len() + second.tracks.len());
    }
    fn track() -> Track {
        Track {
            id: octave_core::TrackId("abcdefghijk".into()),
            title: "Original fixture".into(),
            artists: Vec::new(),
        }
    }
    #[test]
    fn complete_paging_keeps_1753_occurrences_and_rejects_loops_partial_and_cancelled_results() {
        let mut pages = 0;
        let all = collect_pages(
            |_| {
                pages += 1;
                Ok(PlaylistPage {
                    tracks: vec![track(); if pages == 18 { 53 } else { 100 }],
                    continuation: (pages < 18).then(|| format!("page{pages}")),
                    skipped: 0,
                })
            },
            || false,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(all.tracks.len(), 1753);
        assert_eq!(all.pages, 18);
        assert!(
            collect_pages(
                |_| Ok(PlaylistPage {
                    continuation: Some("repeated".into()),
                    ..Default::default()
                }),
                || false,
                |_, _| {}
            )
            .is_err()
        );
        assert!(matches!(
            collect_pages(|_| panic!("Cancelled before request"), || true, |_, _| {}),
            Err(Error::Cancelled)
        ));
        let mut count = 0;
        assert!(
            collect_pages(
                |_| {
                    count += 1;
                    if count == 1 {
                        Ok(PlaylistPage {
                            tracks: vec![track()],
                            continuation: Some("next".into()),
                            skipped: 0,
                        })
                    } else {
                        Err(Error::Network)
                    }
                },
                || false,
                |_, _| {}
            )
            .is_err()
        );
    }
    #[test]
    fn only_finite_playlist_ids_are_allowed_and_rows_preserve_duplicates() {
        assert_eq!(
            playlist_id("https://music.youtube.com/playlist?list=PL_fixture"),
            Some("PL_fixture".into())
        );
        assert!(playlist_id("RD_infinite_mix").is_none());
        assert!(playlist_id("https://example.com/?list=PL_fixture").is_none());
        let row = json!({"musicResponsiveListItemRenderer":{"playlistItemData":{"videoId":"abcdefghijk"},"flexColumns":[{"musicResponsiveListItemFlexColumnRenderer":{"text":{"runs":[{"text":"Original fixture"}]}}}]}});
        let page =
            parse_page(&json!({"musicPlaylistShelfRenderer":{"contents":[row.clone(),row]}}))
                .unwrap();
        assert_eq!(page.tracks.len(), 2);
    }
}
