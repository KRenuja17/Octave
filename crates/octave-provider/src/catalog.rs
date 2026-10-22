//! Normalized unsigned catalog reads; never reuse account credentials.
use crate::{
    Error, Provider, WEB_UA,
    account::{Page, parse_page},
    valid_id,
};
use octave_core::{Collection, Track, TrackId};
use serde_json::{Value, json};
use std::collections::HashMap;
#[derive(Clone, Debug, Default)]
pub struct TrackDetails {
    pub album: String,
    pub duration: String,
}
#[derive(Clone, Debug, Default)]
pub struct CatalogPage {
    pub title: String,
    pub description: String,
    pub artwork: Option<String>,
    pub page: Page,
    pub details: HashMap<TrackId, TrackDetails>,
    pub covers: HashMap<String, String>,
}
impl Provider {
    pub fn catalog(&self, route: &str, continuation: Option<&str>) -> Result<CatalogPage, Error> {
        let version = "1.20260707.12.00";
        let mut body = json!({"context":{"client":{"clientName":"WEB_REMIX","clientVersion":version,"hl":"en"}}});
        let endpoint = if let Some(query) = route.strip_prefix("search:") {
            if query.trim().is_empty() || query.len() > 512 {
                return Err(Error::InvalidResponse);
            }
            body["query"] = json!(query);
            "search"
        } else {
            let (id, params) = route
                .split_once('~')
                .map(|(a, b)| (a, Some(b)))
                .unwrap_or((route, None));
            if !(id.starts_with("FEmusic_")
                || id.starts_with("VL")
                || id.starts_with("MPRE")
                || id.starts_with("UC"))
                || id.len() > 256
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            {
                return Err(Error::InvalidResponse);
            }
            body["browseId"] = json!(id);
            if let Some(params) = params {
                if params.len() > 4096 {
                    return Err(Error::InvalidResponse);
                }
                body["params"] = json!(params);
            }
            "browse"
        };
        if let Some(token) = continuation {
            if token.len() > 16384 {
                return Err(Error::InvalidResponse);
            }
            body["continuation"] = json!(token);
        }
        let value = self.post(
            &format!("https://music.youtube.com/youtubei/v1/{endpoint}?prettyPrint=false"),
            body,
            "67",
            version,
            WEB_UA,
        )?;
        normalize(&value, route)
    }
    /// A service-selected batch, never a claim of whole-playlist true random.
    pub fn service_order(&self, id: &str, radio: bool) -> Result<Vec<Track>, Error> {
        let version = "1.20260707.12.00";
        let mut body = json!({"context":{"client":{"clientName":"WEB_REMIX","clientVersion":version,"hl":"en"}},"enablePersistentPlaylistPanel":true,"isAudioOnly":true,"tunerSettingValue":"AUTOMIX_SETTING_NORMAL"});
        if radio {
            if !valid_id(id) {
                return Err(Error::InvalidTrack);
            }
            body["videoId"] = json!(id);
            body["playlistId"] = json!(format!("RDAMVM{id}"));
            body["params"] = json!("wAEB");
        } else {
            body["playlistId"] =
                json!(crate::playlist::playlist_id(id).ok_or(Error::InvalidResponse)?);
            body["params"] = json!("wAEB8gECKAE%3D");
        }
        let value = self.post(
            "https://music.youtube.com/youtubei/v1/next?prettyPrint=false",
            body,
            "67",
            version,
            WEB_UA,
        )?;
        let mut tracks = Vec::new();
        collect_watch(&value, &mut tracks);
        tracks.truncate(100);
        if tracks.is_empty() {
            Err(Error::Unavailable)
        } else {
            Ok(tracks)
        }
    }
    pub fn suggestions(&self, query: &str) -> Result<Vec<String>, Error> {
        if query.trim().is_empty() || query.len() > 512 {
            return Ok(Vec::new());
        }
        let version = "1.20260707.12.00";
        let value=self.post("https://music.youtube.com/youtubei/v1/music/get_search_suggestions?prettyPrint=false",json!({"context":{"client":{"clientName":"WEB_REMIX","clientVersion":version,"hl":"en"}},"input":query}),"67",version,WEB_UA)?;
        let mut out = Vec::new();
        suggestions(&value, &mut out);
        out.truncate(10);
        Ok(out)
    }
}
fn text(value: &Value) -> String {
    value["simpleText"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            value["runs"]
                .as_array()
                .map(|runs| runs.iter().filter_map(|run| run["text"].as_str()).collect())
                .unwrap_or_default()
        })
}
fn image(value: &Value) -> Option<String> {
    if let Some(array) = value.get("thumbnails").and_then(Value::as_array) {
        return array
            .last()?
            .get("url")?
            .as_str()
            .filter(|s| s.len() <= 2048)
            .map(str::to_owned);
    }
    match value {
        Value::Object(map) => map.values().find_map(image),
        Value::Array(items) => items.iter().find_map(image),
        _ => None,
    }
}
fn normalize(value: &Value, route: &str) -> Result<CatalogPage, Error> {
    let page = parse_page(value).map_err(|_| Error::InvalidResponse)?;
    let title = match route {
        "FEmusic_home" => "Home",
        "FEmusic_explore" => "Explore",
        "FEmusic_moods_and_genres" => "Moods & genres",
        "FEmusic_charts" => "Charts",
        _ => "Collection",
    }
    .to_owned();
    let mut result = CatalogPage {
        page,
        title,
        ..Default::default()
    };
    walk(value, &mut result);
    result.page.tracks.truncate(2000);
    result.page.collections.truncate(500);
    Ok(result)
}
fn walk(value: &Value, page: &mut CatalogPage) {
    for key in [
        "musicResponsiveHeaderRenderer",
        "musicDetailHeaderRenderer",
        "musicImmersiveHeaderRenderer",
        "musicVisualHeaderRenderer",
    ] {
        if let Some(header) = value.get(key) {
            let title = text(&header["title"]);
            if !title.is_empty() {
                page.title = title;
            }
            let description = text(&header["description"]);
            page.description = description.chars().take(1000).collect();
            page.artwork = image(header);
        }
    }
    for key in [
        "musicTwoRowItemRenderer",
        "musicResponsiveListItemRenderer",
        "musicNavigationButtonRenderer",
    ] {
        if let Some(row) = value.get(key) {
            let endpoint = &row["navigationEndpoint"]["browseEndpoint"];
            if let Some(id) = endpoint["browseId"].as_str().filter(|id| {
                id.starts_with("UC")
                    || id.starts_with("VL")
                    || id.starts_with("MPRE")
                    || id.starts_with("FEmusic_")
            }) {
                let title = text(&row["title"]);
                let title = if title.is_empty() {
                    text(&row["buttonText"])
                } else {
                    title
                };
                let title = if title.is_empty() {
                    text(
                        &row["flexColumns"][0]["musicResponsiveListItemFlexColumnRenderer"]["text"],
                    )
                } else {
                    title
                };
                let target =
                    if let Some(params) = endpoint["params"].as_str().filter(|s| s.len() <= 4096) {
                        format!("{id}~{params}")
                    } else {
                        id.into()
                    };
                if !title.is_empty() && !page.page.collections.iter().any(|c| c.id == target) {
                    page.page.collections.push(Collection {
                        id: target.clone(),
                        title,
                    });
                }
                if let Some(url) = image(row) {
                    page.covers.insert(target, url);
                }
            }
            if let Some(id) = row
                .pointer("/playlistItemData/videoId")
                .and_then(Value::as_str)
            {
                let album = text(
                    &row["flexColumns"][2]["musicResponsiveListItemFlexColumnRenderer"]["text"],
                );
                let duration = text(
                    &row["fixedColumns"][0]["musicResponsiveListItemFixedColumnRenderer"]["text"],
                );
                page.details
                    .insert(TrackId(id.into()), TrackDetails { album, duration });
            }
            if key == "musicTwoRowItemRenderer"
                && let Some(id) = row
                    .pointer("/navigationEndpoint/watchEndpoint/videoId")
                    .and_then(Value::as_str)
                    .filter(|id| valid_id(id))
            {
                let title = text(&row["title"]);
                if !title.is_empty() && !page.page.tracks.iter().any(|t| t.id.0 == id) {
                    page.page.tracks.push(Track {
                        id: TrackId(id.into()),
                        title,
                        artists: vec![text(&row["subtitle"])],
                    });
                }
            }
        }
    }
    match value {
        Value::Object(map) => map.values().for_each(|child| walk(child, page)),
        Value::Array(items) => items.iter().for_each(|child| walk(child, page)),
        _ => (),
    }
}
fn collect_watch(value: &Value, out: &mut Vec<Track>) {
    if let Some(row) = value.get("playlistPanelVideoRenderer")
        && let Some(id) = row["videoId"].as_str().filter(|id| valid_id(id))
    {
        let title = text(&row["title"]);
        if !title.is_empty() {
            out.push(Track {
                id: TrackId(id.into()),
                title,
                artists: vec![text(&row["longBylineText"])],
            });
        }
        return;
    }
    match value {
        Value::Object(map) => map.values().for_each(|child| collect_watch(child, out)),
        Value::Array(items) => items.iter().for_each(|child| collect_watch(child, out)),
        _ => (),
    }
}
fn suggestions(value: &Value, out: &mut Vec<String>) {
    if let Some(row) = value.get("searchSuggestionRenderer") {
        let title = text(&row["suggestion"]);
        if !title.is_empty() && title.len() <= 512 && !out.contains(&title) {
            out.push(title);
        }
    }
    match value {
        Value::Object(map) => map.values().for_each(|child| suggestions(child, out)),
        Value::Array(items) => items.iter().for_each(|child| suggestions(child, out)),
        _ => (),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_catalog_keeps_genre_parameters_and_song_metadata() {
        let page=normalize(&json!({"contents":[{"musicNavigationButtonRenderer":{"buttonText":{"runs":[{"text":"Original genre"}]},"navigationEndpoint":{"browseEndpoint":{"browseId":"FEmusic_moods_and_genres_category","params":"fixture"}}}},{"musicTwoRowItemRenderer":{"title":{"runs":[{"text":"Original song"}]},"navigationEndpoint":{"watchEndpoint":{"videoId":"abcdefghijk"}}}}]}),"FEmusic_home").unwrap();
        assert!(
            page.page
                .collections
                .iter()
                .any(|c| c.id.ends_with("~fixture"))
        );
        assert_eq!(page.page.tracks.len(), 1);
    }
    #[test]
    #[ignore = "Opt-in unsigned catalog reads; no audio or account"]
    fn live_unsigned_catalog_and_service_order() {
        let provider = Provider::new().unwrap();
        for id in ["FEmusic_home", "FEmusic_explore"] {
            let page = provider.catalog(id, None).unwrap();
            println!(
                "catalog={id} songs={} collections={}",
                page.page.tracks.len(),
                page.page.collections.len()
            );
            assert!(!page.page.tracks.is_empty() || !page.page.collections.is_empty());
        }
        let mix = provider.service_order("hpSrLjc5SMs", true).unwrap();
        println!("service_radio_batch={}", mix.len());
        assert!(!mix.is_empty());
        assert!(!provider.suggestions("oasis").unwrap().is_empty());
    }
}
