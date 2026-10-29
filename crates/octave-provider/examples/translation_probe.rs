//! Opt-in service check using production transport and synthetic text only.
//! No account, saved profile, API key, lyrics or audio device is accessed.
use octave_core::{
    TrackId,
    lyrics::{LyricCue, Lyrics},
};
use octave_provider::translation::Translator;

fn main() {
    if let Err(error) = run() {
        // Shared provider Display messages describe YouTube/playback. Keep this
        // service diagnostic neutral while preserving its redacted error class.
        if let Some(error) = error.downcast_ref::<octave_provider::Error>() {
            eprintln!("translation_probe_failed: {error:?}");
        } else {
            eprintln!("translation_probe_failed: {error}");
        }
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let endpoint = args.next().ok_or("Supply endpoint [--synthetic]")?;
    let synthetic = match args.next().as_deref() {
        None => false,
        Some("--synthetic") => true,
        _ => return Err("Expected --synthetic or no additional argument".into()),
    };
    if args.next().is_some() {
        return Err("Unexpected argument".into());
    }
    let translator = Translator::new(&endpoint, String::new())?;
    let languages = translator.languages()?;
    println!("metadata_available=true language_count={}", languages.len());
    if !synthetic {
        return Ok(());
    }
    if !languages.iter().any(|language| language.code == "es") {
        return Err("Synthetic check requires Spanish support".into());
    }
    let text = ["The morning sky is blue.", "", "The morning sky is blue."];
    let original = Lyrics {
        track_id: TrackId("synthetic-translation-check".into()),
        text: text.join("\n"),
        source: "Octave synthetic integration check".into(),
        cues: text
            .iter()
            .enumerate()
            .map(|(id, text)| LyricCue {
                id,
                text: (*text).into(),
                start_ms: id as u64 * 1000,
                end_ms: (id as u64 + 1) * 1000,
            })
            .collect(),
    };
    let result = translator.translate(&original, "es", || false)?;
    if result.original != original
        || result.target != "es"
        || result.lines.len() != 3
        || !result.lines[1].is_empty()
        || result.lines[0].trim().is_empty()
        || result.lines[2].trim().is_empty()
        || result.lines[0] == text[0]
        || result.lines[2] == text[2]
    {
        return Err("Synthetic translation acceptance failed".into());
    }
    println!(
        "synthetic_translation=true cue_identity_preserved=true empty_line_preserved=true repeated_occurrences_preserved=true"
    );
    println!("language_quality_reviewed=false live_lyrics_ui_tested=false");
    Ok(())
}
