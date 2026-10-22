use crate::{Error, Provider, WEB_UA, valid_id};
use octave_core::TrackId;
pub use octave_core::lyrics::{LyricCue, Lyrics};
use serde_json::{Value, json};

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
        // This Provider has no cookie jar or account headers. Mobile context is
        // local to this request; concurrent web requests keep their own context.
        let mobile_version = "7.21.50";
        let mobile = self.post(
            "https://music.youtube.com/youtubei/v1/browse?prettyPrint=false",
            json!({"context":{"client":{"clientName":"ANDROID_MUSIC","clientVersion":mobile_version,"hl":"en"}},"browseId":browse}),
            "21",
            mobile_version,
            WEB_UA,
        );
        lyrics_or_fallback(mobile, id, || {
            self.post(
                "https://music.youtube.com/youtubei/v1/browse?prettyPrint=false",
                json!({"context":context,"browseId":browse}),
                "67",
                version,
                WEB_UA,
            )
        })
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
fn lyrics_or_fallback(
    mobile: Result<Value, Error>,
    id: &TrackId,
    fallback: impl FnOnce() -> Result<Value, Error>,
) -> Result<Option<Lyrics>, Error> {
    if let Ok(page) = mobile
        && let Some(lyrics) = parse(&page, id)
    {
        return Ok(Some(lyrics));
    }
    // A failed/unsupported timed request must not prevent the web fallback.
    fallback().map(|page| parse(&page, id))
}

fn parse(value: &Value, id: &TrackId) -> Option<Lyrics> {
    if let Some(lines) = value.get("timedLyricsData").and_then(Value::as_array)
        && let Some(lyrics) = parse_timed(lines, value, id)
    {
        return Some(lyrics);
    }
    if let Some(shelf) = value.get("musicDescriptionShelfRenderer") {
        let text = runs(&shelf["description"]);
        if !text.is_empty() && text.len() <= 65536 {
            return Some(Lyrics {
                track_id: id.clone(),
                text,
                source: runs(&shelf["footer"]),
                cues: Vec::new(),
            });
        }
    }
    match value {
        Value::Object(map) => map.values().find_map(|value| parse(value, id)),
        Value::Array(items) => items.iter().find_map(|value| parse(value, id)),
        _ => None,
    }
}

fn milliseconds(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

fn parse_timed(lines: &[Value], parent: &Value, track_id: &TrackId) -> Option<Lyrics> {
    if lines.is_empty() || lines.len() > 4096 {
        return None;
    }
    let mut lyrics = Lyrics {
        track_id: track_id.clone(),
        text: String::new(),
        source: parent["sourceMessage"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        cues: Vec::with_capacity(lines.len()),
    };
    let mut valid_timing = true;
    let mut previous_end = 0;
    for (id, line) in lines.iter().enumerate() {
        let text = line["lyricLine"].as_str()?;
        if id > 0 {
            lyrics.text.push('\n');
        }
        lyrics.text.push_str(text);
        if lyrics.text.len() > 65536 {
            return None;
        }
        let range = &line["cueRange"];
        match (
            milliseconds(&range["startTimeMilliseconds"]),
            milliseconds(&range["endTimeMilliseconds"]),
        ) {
            (Some(start), Some(end))
                if start >= previous_end && end > start && end <= 86_400_000 =>
            {
                lyrics.cues.push(LyricCue {
                    id,
                    text: text.to_owned(),
                    start_ms: start,
                    end_ms: end,
                });
                previous_end = end;
            }
            _ => {
                valid_timing = false;
            }
        }
    }
    if lyrics.text.trim().is_empty() {
        return None;
    }
    // Reject the entire timing map if any cue is invalid. Keep the original
    // text, including gaps/blank lines, rather than dropping or retiming lines.
    if !valid_timing {
        lyrics.cues.clear();
    }
    Some(lyrics)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn parse(value: &Value) -> Option<Lyrics> {
        super::parse(value, &TrackId("aaaaaaaaaaa".into()))
    }
    fn timed(lines: Value) -> Value {
        // Actual mobile renderer nesting, with independently authored fixture text.
        json!({"contents":{"elementRenderer":{"newElement":{"type":{"componentType":{"model":{
            "timedLyricsModel":{"lyricsData":{"timedLyricsData":lines,"sourceMessage":"Original fixture attribution"}}
        }}}}}}})
    }
    fn line(text: &str, start: Value, end: Value) -> Value {
        json!({"lyricLine":text,"cueRange":{"startTimeMilliseconds":start,"endTimeMilliseconds":end}})
    }
    #[test]
    fn mobile_errors_and_missing_data_fall_back_without_losing_identity() {
        let id = TrackId("bbbbbbbbbbb".into());
        for mobile in [Err(Error::Timeout), Err(Error::Http(403)), Ok(json!({}))] {
            let lyrics = lyrics_or_fallback(mobile.clone(), &id, || {
                Ok(timed(json!([
                    {"lyricLine":"Plain fallback"}
                ])))
            })
            .unwrap()
            .unwrap();
            assert_eq!(lyrics.track_id, id);
            assert_eq!(lyrics.text, "Plain fallback");
            assert!(lyrics.cues.is_empty());
            assert_eq!(
                lyrics_or_fallback(mobile, &id, || Err(Error::Network)),
                Err(Error::Network)
            );
        }
        assert_eq!(
            lyrics_or_fallback(Ok(json!({})), &id, || Ok(json!({}))),
            Ok(None)
        );
        let result = lyrics_or_fallback(
            Ok(timed(json!([line("Timed", json!(100), json!(500))]))),
            &id,
            || panic!("Valid mobile response should not fetch again"),
        );
        assert_eq!(result.unwrap().unwrap().cues.len(), 1);
    }
    #[test]
    fn timed_lines_keep_gaps_unicode_attribution_and_occurrence_ids() {
        let page = timed(json!([
            line("Original refrain", json!("1000"), json!(2000)),
            line("", json!(3000), json!(4000)),
            line("Original refrain", json!(5000), json!(6000)),
            line("සිංහල 日本語", json!(7000), json!(8000))
        ]));
        let lyrics = parse(&page).unwrap();
        assert_eq!(lyrics.cues.len(), 4);
        assert_eq!(lyrics.cues[2].id, 2);
        assert_eq!(lyrics.cues[1].start_ms, 3000);
        assert_eq!(
            lyrics.text,
            "Original refrain\n\nOriginal refrain\nසිංහල 日本語"
        );
        assert_eq!(lyrics.source, "Original fixture attribution");
    }
    #[test]
    fn invalid_or_partial_timing_preserves_all_text_as_plain() {
        for (start, end) in [
            (json!(null), json!(3000)),
            (json!(-1), json!(3000)),
            (json!(2000), json!(2000)),
            (json!(2500), json!(2000)),
            (json!(1500), json!(3000)),
            (json!("oops"), json!(3000)),
            (json!(2000.5), json!(3000)),
            (json!(2000), json!(86_400_001)),
        ] {
            let lyrics = parse(&timed(json!([
                line("First", json!(1000), json!(2000)),
                line("Second", start, end)
            ])))
            .unwrap();
            assert!(lyrics.cues.is_empty());
            assert_eq!(lyrics.text, "First\nSecond");
        }
        let lyrics = parse(&timed(json!([{"lyricLine":"Without timing"}]))).unwrap();
        assert!(lyrics.cues.is_empty());
    }
    #[test]
    fn bounded_malformed_and_empty_documents_are_not_lyrics() {
        for lines in [
            json!([]),
            json!([{"lyricLine":42}]),
            json!([{"lyricLine":"  "}]),
            json!([{"lyricLine":"x".repeat(65537)}]),
            json!(vec![json!({"lyricLine":"x"}); 4097]),
        ] {
            assert_eq!(parse(&timed(lines)), None);
        }
    }
    #[test]
    #[ignore = "Opt-in unsigned network requests; no audio or account access"]
    fn live_unsigned_timed_lyrics() {
        let provider = Provider::new().unwrap();
        let mut timed_count = 0;
        // Exact recording IDs matter: the original five music-video examples
        // serve valid text without cueRange. Include upstream public test tracks
        // whose unsigned responses have real timings; retain the plain examples.
        for raw in [
            "hpSrLjc5SMs",
            "6Zt4P1scOwE",
            "dQw4w9WgXcQ",
            "kJQP7kiw5Fk",
            "hT_nvWreIhg",
            "fJ9rUzIMcZQ",
            "JGwWNGJdvx8",
        ] {
            let lyrics = provider
                .lyrics(&TrackId(raw.into()))
                .unwrap()
                .expect("Lyrics available");
            assert_eq!(lyrics.track_id.0, raw);
            println!(
                "track={raw} cues={} characters={} attribution={} first_ms={:?} last_ms={:?}",
                lyrics.cues.len(),
                lyrics.text.chars().count(),
                !lyrics.source.is_empty(),
                lyrics.cues.first().map(|cue| cue.start_ms),
                lyrics.cues.last().map(|cue| cue.end_ms)
            );
            timed_count += usize::from(!lyrics.cues.is_empty());
            assert!(!lyrics.source.is_empty());
        }
        assert!(
            timed_count >= 2,
            "Need actual service timings for multiple tracks; got {timed_count}"
        );
    }
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
