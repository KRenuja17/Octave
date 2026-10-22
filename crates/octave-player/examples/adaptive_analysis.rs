//! Silent, local-only analysis probe. No arguments benchmarks generated PCM.
//! Two file arguments analyze the outgoing 60s tail and incoming 60s intro.
use octave_player::adaptive::{Analysis, Analyzer, MAX_WINDOW_SECONDS, propose};
use rodio::{Decoder, Source};
use std::{
    fs::File,
    io::BufReader,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

fn generated(bpm: u32) -> Result<(Analysis, usize), String> {
    let cancel = AtomicBool::new(false);
    let mut analyzer = Analyzer::new(48000, 2, Duration::ZERO).map_err(|e| format!("{e:?}"))?;
    for block in 0..MAX_WINDOW_SECONDS as usize * 10 {
        let pcm: Vec<_> = (block * 4800..(block + 1) * 4800)
            .flat_map(|frame| {
                let phase = frame % (48000 * 60 / bpm as usize);
                let sample = if phase < 1440 {
                    (phase as f32 * 440.0 / 48000.0 * std::f32::consts::TAU).sin() * 0.7
                } else {
                    0.0
                };
                [sample, sample]
            })
            .collect();
        analyzer.feed(&pcm, &cancel).map_err(|e| format!("{e:?}"))?;
    }
    let bytes = analyzer.retained_bytes();
    Ok((
        analyzer.finish(&cancel).map_err(|e| format!("{e:?}"))?,
        bytes,
    ))
}
fn local(path: &str, outro: bool) -> Result<Analysis, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let bytes = file.metadata().map_err(|e| e.to_string())?.len();
    let mut decoder = Decoder::builder()
        .with_data(BufReader::new(file))
        .with_byte_len(bytes)
        .with_seekable(true)
        .build()
        .map_err(|e| e.to_string())?;
    let rate = decoder.sample_rate().get();
    let channels = usize::from(decoder.channels().get());
    let start = if outro {
        decoder
            .total_duration()
            .ok_or("Outgoing file needs a known duration")?
            .saturating_sub(Duration::from_secs(u64::from(MAX_WINDOW_SECONDS)))
    } else {
        Duration::ZERO
    };
    if !start.is_zero() {
        decoder.try_seek(start).map_err(|e| e.to_string())?;
    }
    let mut analyzer = Analyzer::new(rate, channels, start).map_err(|e| format!("{e:?}"))?;
    let cancel = AtomicBool::new(false);
    let block_frames = rate as usize / 10;
    let mut total_frames = 0;
    let limit = rate as usize * MAX_WINDOW_SECONDS as usize;
    while total_frames < limit {
        let pcm: Vec<_> = decoder
            .by_ref()
            .take(block_frames.min(limit - total_frames) * channels)
            .collect();
        if pcm.is_empty() {
            break;
        }
        total_frames += analyzer.feed(&pcm, &cancel).map_err(|e| format!("{e:?}"))?;
    }
    analyzer.finish(&cancel).map_err(|e| format!("{e:?}"))
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => {
            let mut milliseconds = Vec::new();
            for _ in 0..9 {
                let start = Instant::now();
                let (outgoing, bytes) = generated(120)?;
                let (incoming, _) = generated(120)?;
                milliseconds.push(start.elapsed().as_secs_f64() * 1000.0);
                println!(
                    "generated_pair_seconds=120 retained_envelope_bytes_per_window={bytes} outgoing={outgoing:?} decision={:?}",
                    propose(&outgoing, &incoming)
                );
            }
            milliseconds.sort_by(f64::total_cmp);
            println!(
                "iterations=9 stereo_rate=48000 analysis_plus_generation_median_ms={:.3} p95_ms={:.3}",
                milliseconds[4], milliseconds[8]
            );
        }
        [outgoing, incoming] => {
            let start = Instant::now();
            let a = local(outgoing, true)?;
            let b = local(incoming, false)?;
            println!(
                "outgoing={a:?}\nincoming={b:?}\ndecision={:?}\nanalysis_plus_decode_ms={}",
                propose(&a, &b),
                start.elapsed().as_millis()
            );
        }
        _ => {
            return Err(
                "Usage: adaptive_analysis [OUTGOING_LOCAL_FILE INCOMING_LOCAL_FILE]".into(),
            );
        }
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("analysis_failed: {error}");
        std::process::exit(1);
    }
}
