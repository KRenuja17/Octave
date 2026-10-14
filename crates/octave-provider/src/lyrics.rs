use crate::{Error, Provider, WEB_UA, valid_id};
use octave_core::TrackId;
use serde_json::{Value, json};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lyrics {
    pub text: String,
    pub source: String,
}
impl Provider {
    pub fn lyrics(&self, id: &TrackId) -> Result<Option<Lyrics>, Error> {
        if !valid_id(&id.0) {
            return Err(Error::InvalidTrack);
        }
        let version = "1.20260707.12.00";
        let context =
            json!({"client":{"clientName":"WEB_REMIX","clientVersion":version,"hl":"en"}});
        let next = self.post(
            "https://music.youtube.com/youtubei/v1/next?prettyPrint=false",
            json!({"context":context,"videoId":id.0}),
            "67",
            version,
            WEB_UA,
        )?;
        let Some(browse) = find_id(&next) else {
            return Ok(None);
        };
        let page = self.post(
            "https://music.youtube.com/youtubei/v1/browse?prettyPrint=false",
            json!({"context":context,"browseId":browse}),
            "67",
            version,
            WEB_UA,
        )?;
        Ok(parse(&page))
    }
}
fn find_id(value: &Value) -> Option<&str> {
    if let Some(id) = value
        .get("browseId")
        .and_then(Value::as_str)
        .filter(|id| id.starts_with("MPLYt"))
    {
        return Some(id);
    }
    match value {
        Value::Object(map) => map.values().find_map(find_id),
        Value::Array(items) => items.iter().find_map(find_id),
        _ => None,
    }
}
fn runs(value: &Value) -> String {
    value
        .get("runs")
        .and_then(Value::as_array)
        .map(|runs| runs.iter().filter_map(|r| r["text"].as_str()).collect())
        .unwrap_or_default()
}
fn parse(value: &Value) -> Option<Lyrics> {
    if let Some(shelf) = value.get("musicDescriptionShelfRenderer") {
        let text = runs(&shelf["description"]);
        if !text.is_empty() && text.len() <= 65536 {
            return Some(Lyrics {
                text,
                source: runs(&shelf["footer"]),
            });
        }
    }
    match value {
        Value::Object(map) => map.values().find_map(parse),
        Value::Array(items) => items.iter().find_map(parse),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_lyrics_keep_source_and_missing_is_normal() {
        assert_eq!(parse(&json!({})), None);
        let lyrics = parse(&json!({"musicDescriptionShelfRenderer":{"description":{"runs":[{"text":"Original test line\n"},{"text":"Another test line"}]},"footer":{"runs":[{"text":"Test source"}]}}})).unwrap();
        assert_eq!(lyrics.text, "Original test line\nAnother test line");
        assert_eq!(lyrics.source, "Test source");
        assert_eq!(
            find_id(&json!({"nested":{"browseId":"MPLYt_test"}})),
            Some("MPLYt_test")
        );
    }
}
