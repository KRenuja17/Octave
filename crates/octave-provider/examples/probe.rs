//! Opt-in live probe. Prints only counts and classified errors, never URLs or raw bodies.
use octave_core::TrackId;
use octave_provider::{Provider, range::RangeReader};
use std::{
    io::Read,
    sync::{Arc, atomic::AtomicU64},
};
fn main() {
    if let Err(error) = run() {
        eprintln!("probe_failed: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let provider = Provider::new()?;
    let argument = std::env::args().nth(1);
    if argument.as_deref() == Some("--search") {
        let query = std::env::args().nth(2).ok_or("Supply a search query")?;
        let tracks = provider.search(&query)?;
        println!("search_results={}", tracks.len());
        for track in tracks.iter().take(5) {
            match provider.resolve(&track.id) {
                Ok(stream) => println!(
                    "video_id={} duration_seconds={} resolved=true",
                    track.id.0,
                    stream.duration.as_secs()
                ),
                Err(error) => println!("video_id={} error={error}", track.id.0),
            }
        }
        return Ok(());
    }
    let id = if let Some(id) = argument {
        TrackId(id)
    } else {
        let tracks = provider.search("Beethoven Moonlight Sonata")?;
        println!("search_results={}", tracks.len());
        tracks.first().ok_or("No search results")?.id.clone()
    };
    let stream = provider.resolve(&id)?;
    println!(
        "resolved_bytes={} duration_seconds={}",
        stream.content_length,
        stream.duration.as_secs()
    );
    let mut reader = RangeReader::new(provider, stream, Arc::new(AtomicU64::new(1)), 1);
    let mut bytes = [0; 4096];
    reader.read_exact(&mut bytes)?;
    println!("range_read_bytes={}", bytes.len());
    Ok(())
}
