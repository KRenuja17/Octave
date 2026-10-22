//! Optional unsigned discovery; failures never change the listening session.
use crate::{Error, Provider, WEB_UA, collect_tracks, valid_id};
use octave_core::{Track, TrackId};
use serde_json::{Value, json};
impl Provider {
    pub fn related(&self, id: &TrackId) -> Result<Vec<Track>, Error> {
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
        let Some(browse) = related_id(&next) else {
            return Ok(Vec::new());
        };
        let page = self.post(
            "https://music.youtube.com/youtubei/v1/browse?prettyPrint=false",
            json!({"context":context,"browseId":browse}),
            "67",
            version,
            WEB_UA,
        )?;
        let mut tracks = Vec::new();
        collect_tracks(&page, &mut tracks);
        tracks.retain(|track| track.id != *id);
        tracks.truncate(100);
        Ok(tracks)
    }
}
fn related_id(value: &Value) -> Option<&str> {
    if let Some(id) = value
        .get("browseId")
        .and_then(Value::as_str)
        .filter(|s| s.starts_with("MPTR"))
    {
        return Some(id);
    }
    match value {
        Value::Object(map) => map.values().find_map(related_id),
        Value::Array(array) => array.iter().find_map(related_id),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Opt-in unsigned discovery request; no audio or account access"]
    fn live_unsigned_related() {
        let tracks = Provider::new()
            .unwrap()
            .related(&TrackId("hpSrLjc5SMs".into()))
            .unwrap();
        println!("unsigned_related_songs={}", tracks.len());
        assert!(!tracks.is_empty());
    }

    #[test]
    fn only_related_endpoints_are_selected() {
        assert_eq!(
            related_id(&json!({"tabs":[{"browseId":"MPLYt_lyrics"},{"browseId":"MPTRt_related"}]})),
            Some("MPTRt_related")
        );
        assert_eq!(related_id(&json!({"browseId":"MPLYt_lyrics"})), None);
    }
}
