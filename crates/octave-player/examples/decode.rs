//! Opt-in live AAC/seek probe. No audio is emitted unless --output is supplied.
use octave_core::TrackId;
use octave_provider::{Provider, range::RangeReader};
use rodio::{Decoder, Source};
use std::{
    sync::{Arc, atomic::AtomicU64},
    time::{Duration, Instant},
};
fn main() {
    if let Err(error) = run() {
        eprintln!("probe_failed: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    let id = std::env::args()
        .nth(1)
        .ok_or("Usage: decode VIDEO_ID [--output]")?;
    let start = Instant::now();
    let provider = Provider::new().map_err(|e| e.to_string())?;
    let stream = provider
        .resolve(&TrackId(id.clone()))
        .map_err(|e| e.to_string())?;
    let length = stream.content_length;
    let duration = stream.duration;
    let reader = RangeReader::new(provider, stream, Arc::new(AtomicU64::new(1)), 1);
    let fault = reader.fault();
    let mut decoder = Decoder::builder()
        .with_data(reader)
        .with_byte_len(length)
        .with_seekable(true)
        .with_hint("m4a")
        .build()
        .map_err(|e| {
            format!(
                "Decoder initialization failed: {e}; transport={:?}",
                *fault.lock().unwrap()
            )
        })?;
    let rate = decoder.sample_rate().get();
    let channels = decoder.channels().get();
    println!(
        "decoder_ready_ms={} rate={} channels={} duration_seconds={}",
        start.elapsed().as_millis(),
        rate,
        channels,
        duration.as_secs()
    );
    for seconds in [0, 60, 5, 1200, 5] {
        if seconds >= duration.as_secs() {
            continue;
        }
        decoder
            .try_seek(Duration::from_secs(seconds))
            .map_err(|_| "Seek failed")?;
        let requested = rate as usize * channels as usize;
        let samples: Vec<f32> = decoder.by_ref().take(requested).collect();
        if let Some(error) = *fault.lock().unwrap() {
            return Err(error.to_string());
        }
        if samples.len() != requested || samples.iter().any(|s| !s.is_finite()) {
            return Err("Invalid or incomplete decoded PCM".into());
        }
        println!("seek_seconds={seconds} decoded_samples={}", samples.len());
    }
    if std::env::args().any(|arg| arg == "--output") {
        output_probe(id)?;
    }
    Ok(())
}
fn wait_for(
    player: &octave_player::Player,
    predicate: impl Fn(&octave_player::Snapshot) -> bool,
) -> Result<(), String> {
    let start = Instant::now();
    loop {
        let snapshot = player.snapshot();
        if let octave_player::PlaybackState::Failed(_, error) = &snapshot.state {
            return Err(error.clone());
        }
        if predicate(&snapshot) {
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(45) {
            return Err("Output probe timed out".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
fn output_probe(id: String) -> Result<(), String> {
    use octave_player::{PlaybackState, Player};
    let player = Player::new();
    player.play(TrackId(id));
    wait_for(&player, |s| {
        matches!(s.state, PlaybackState::Playing(_)) && s.position >= Duration::from_secs(1)
    })?;
    player.toggle_pause();
    wait_for(&player, |s| matches!(s.state, PlaybackState::Paused(_)))?;
    let before = player.snapshot().position;
    std::thread::sleep(Duration::from_millis(200));
    if player.snapshot().position > before + Duration::from_millis(50) {
        return Err("Pause did not hold position".into());
    }
    player.seek(Duration::from_secs(60));
    wait_for(&player, |s| {
        matches!(s.state, PlaybackState::Paused(_)) && s.position >= Duration::from_secs(60)
    })?;
    player.toggle_pause();
    wait_for(&player, |s| {
        matches!(s.state, PlaybackState::Playing(_)) && s.position >= Duration::from_secs(61)
    })?;
    player.stop();
    if !matches!(player.snapshot().state, PlaybackState::Idle) {
        return Err("Stop failed".into());
    }
    println!("output_pause_seek_resume_stop=passed");
    Ok(())
}
