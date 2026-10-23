//! Explicit 30-second default-device probe. The player volume is always zero.
//! No account, device selection, system volume change, preparation or retries.
use octave_core::TrackId;
use octave_player::{PlaybackState, Player};
use std::{
    thread,
    time::{Duration, Instant},
};

fn main() {
    if let Err(error) = run() {
        eprintln!("muted_default_output_probe=failed reason={error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    const USAGE: &str = "Usage: muted_playback VIDEO_ID --muted-default-output\nOpens the default audio device for a 30-second playback window at player volume zero. No sign-in or automatic retries.";
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!("{USAGE}");
        return Ok(());
    }
    if args.len() != 2
        || args[1] != "--muted-default-output"
        || args[0].len() != 11
        || !args[0]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(USAGE.into());
    }
    let id = TrackId(args[0].clone());
    let player = Player::new();
    // Set zero before submitting any play request. The production output path
    // applies this value before it queues the first PCM, with no system change.
    player.set_volume(0.0);
    let requested = Instant::now();
    player.play(id.clone());
    let mut started: Option<Instant> = None;
    let mut first = Duration::ZERO;
    let mut previous = Duration::ZERO;
    let mut quality = None;
    let mut buffering = false;
    let mut buffer_events = 0;
    let mut buffer_time = Duration::ZERO;
    let mut polled = Instant::now();
    let mut milestone = 10;
    loop {
        let snapshot = player.snapshot();
        let now = Instant::now();
        if buffering {
            buffer_time += now.duration_since(polled);
        }
        polled = now;
        if snapshot.buffering && !buffering {
            buffer_events += 1;
        }
        buffering = snapshot.buffering;
        match &snapshot.state {
            PlaybackState::Failed(_, error) => return Err(error.to_string()),
            PlaybackState::Loading(current) if started.is_none() && current == &id => (),
            PlaybackState::Playing(current) if current == &id => {
                if snapshot.sequence != 0 {
                    return Err("Unexpected track handoff".into());
                }
                if snapshot.position < previous {
                    return Err("Consumed clock regressed".into());
                }
                previous = snapshot.position;
                if !snapshot.position.is_zero() {
                    if started.is_none() {
                        first = snapshot.position;
                        quality = snapshot.quality;
                        if quality.is_none() {
                            return Err("Stream quality is missing".into());
                        }
                        println!(
                            "playback_started startup_ms={} quality={} output=default player_volume=0 sign_in=false automatic_retries=false",
                            requested.elapsed().as_millis(),
                            quality.unwrap()
                        );
                    }
                    let elapsed = started.get_or_insert(now).elapsed();
                    if snapshot.quality != quality {
                        return Err("Stream quality changed unexpectedly".into());
                    }
                    if elapsed.as_secs() >= milestone {
                        println!(
                            "muted_probe_progress wall_seconds={:.3} consumed_delta_seconds={:.3}",
                            elapsed.as_secs_f64(),
                            (snapshot.position - first).as_secs_f64()
                        );
                        milestone += 10;
                    }
                    if elapsed >= Duration::from_secs(30) {
                        let consumed = snapshot.position - first;
                        // Stop before checking/reporting so both a pass and an
                        // assertion failure promptly release queued audio.
                        player.stop();
                        if player.snapshot().state != PlaybackState::Idle {
                            return Err("Stop did not become Idle".into());
                        }
                        if !(Duration::from_secs(29)..=Duration::from_secs(31)).contains(&consumed)
                        {
                            return Err(
                                "Consumed clock differs from wall time by more than one second"
                                    .into(),
                            );
                        }
                        println!(
                            "muted_default_output_probe=passed wall_seconds={:.3} consumed_delta_seconds={:.3} buffer_events={buffer_events} buffering_ms={} stopped_idle=true output=default player_volume=0",
                            elapsed.as_secs_f64(),
                            consumed.as_secs_f64(),
                            buffer_time.as_millis()
                        );
                        return Ok(());
                    }
                }
            }
            _ => return Err("Playback entered an unexpected state".into()),
        }
        if started.is_none() && requested.elapsed() >= Duration::from_secs(15) {
            return Err("No consumed audio within the 15-second startup budget".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}
