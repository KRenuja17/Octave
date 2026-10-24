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
    let args: Vec<_> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--list-outputs") {
        for (index, device) in octave_player::output_devices()
            .map_err(|e| e.to_string())?
            .iter()
            .enumerate()
        {
            println!("output_index={index} name={}", device.name());
        }
        return Ok(());
    }
    let id = std::env::args()
        .nth(1)
        .ok_or("Usage: decode VIDEO_ID [--output]")?;
    let start = Instant::now();
    let provider = Provider::new().map_err(|e| e.to_string())?;
    let stream = provider
        .resolve(&TrackId(id.clone()))
        .map_err(|e| format!("resolve_error={e:?}: {e}"))?;
    if args.iter().any(|arg| arg == "--inspect") {
        // Resolver metadata only: no decoder, PCM, player or output device.
        println!(
            "stream_inspection=passed duration_seconds={} content_bytes={} bitrate_bps={}",
            stream.duration.as_secs(),
            stream.content_length,
            stream.bitrate_bps.unwrap_or(0)
        );
        return Ok(());
    }
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
    if args.iter().any(|arg| arg == "--full-decode") {
        let mut samples = 0_u64;
        let deadline = Instant::now() + Duration::from_secs(900);
        for sample in decoder.by_ref() {
            if !sample.is_finite() {
                return Err("Non-finite PCM".into());
            }
            samples += 1;
            if samples.is_multiple_of(u64::from(rate) * u64::from(channels) * 60) {
                println!(
                    "decoded_seconds={}",
                    samples / u64::from(rate) / u64::from(channels)
                );
                if Instant::now() > deadline {
                    return Err("Full decode exceeded 15-minute workload limit".into());
                }
            }
        }
        if let Some(error) = *fault.lock().unwrap() {
            return Err(error.to_string());
        }
        let seconds = samples / u64::from(rate) / u64::from(channels);
        if seconds + 2 < duration.as_secs() {
            return Err("Early decoder EOF".into());
        }
        println!(
            "full_decode=passed decoded_seconds={seconds} elapsed_ms={}",
            start.elapsed().as_millis()
        );
        return Ok(());
    }
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
    if args
        .iter()
        .any(|arg| arg == "--output" || arg == "--muted-output")
    {
        let device = if let Some(index) = args.windows(2).find(|pair| pair[0] == "--output-index") {
            let index: usize = index[1].parse().map_err(|_| "Invalid output index")?;
            Some(
                octave_player::output_devices()
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .nth(index)
                    .ok_or("Output index is unavailable")?,
            )
        } else {
            None
        };
        output_probe(
            id,
            device,
            args.iter().any(|arg| arg == "--muted-output"),
            args.iter().any(|arg| arg == "--eq-test"),
        )?;
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
            return Err(error.to_string());
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
fn output_probe(
    id: String,
    device: Option<octave_player::OutputDevice>,
    muted: bool,
    eq_test: bool,
) -> Result<(), String> {
    use octave_player::{PlaybackState, Player};
    let player = Player::new();
    player.set_output_device(device);
    if eq_test {
        player.set_equalizer(octave_player::Equalizer {
            gains_db: [6.0, -3.0, 2.0],
        });
    }
    if muted {
        player.set_volume(0.0);
    }
    player.play(TrackId(id.clone()));
    wait_for(&player, |s| {
        matches!(s.state, PlaybackState::Playing(_)) && s.position >= Duration::from_secs(1)
    })?;
    for seconds in [60, 5, 120, 10, 5] {
        player.seek(Duration::from_secs(seconds));
    }
    wait_for(&player, |s| {
        matches!(s.state, PlaybackState::Playing(_))
            && s.position >= Duration::from_secs(6)
            && s.position < Duration::from_secs(10)
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
    let position = player.snapshot().position;
    player.set_output_device(None);
    if !matches!(player.snapshot().state, PlaybackState::Idle) {
        return Err("Output selection did not stop playback".into());
    }
    player.play_at(TrackId(id), position);
    wait_for(&player, |s| {
        matches!(s.state, PlaybackState::Playing(_))
            && s.position > position + Duration::from_secs(1)
    })?;
    player.stop();
    if !matches!(player.snapshot().state, PlaybackState::Idle) {
        return Err("Stop failed".into());
    }
    println!("output_pause_seek_resume_switch_stop=passed");
    Ok(())
}
