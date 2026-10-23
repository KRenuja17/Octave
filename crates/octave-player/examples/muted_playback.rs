//! Explicit default-device probe: 30 seconds, or one real-time hour on repeat.
//! Always zero player volume; no account, system setting changes or retries.
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

/// Sum only observed consumed-clock increments. At a handoff the unobserved
/// outgoing tail is deliberately omitted; never substitute advertised duration.
#[derive(Default)]
struct Consumed {
    previous: Option<(u64, Duration)>,
    total: Duration,
}
impl Consumed {
    fn observe(&mut self, sequence: u64, position: Duration, repeat: bool) -> Result<bool, String> {
        let (delta, handoff) = match self.previous {
            None if sequence == 0 => (Duration::ZERO, false),
            Some((old, before)) if sequence == old => (
                position
                    .checked_sub(before)
                    .ok_or("Consumed clock regressed")?,
                false,
            ),
            Some((old, _)) if repeat && sequence.checked_sub(old) == Some(1) => (position, true),
            _ => return Err("Unexpected track handoff or skipped sequence".into()),
        };
        self.total += delta;
        self.previous = Some((sequence, position));
        Ok(handoff)
    }
}

fn run() -> Result<(), String> {
    const USAGE: &str = "Usage: muted_playback VIDEO_ID --muted-default-output [--one-hour]\nOpens the default audio device at player volume zero. The default window is 30 seconds; --one-hour repeats the exact recording gaplessly for 3600 wall-clock seconds. No sign-in or automatic retries.";
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!("{USAGE}");
        return Ok(());
    }
    if !(args.len() == 2 || (args.len() == 3 && args[2] == "--one-hour"))
        || args[1] != "--muted-default-output"
        || args[0].len() != 11
        || !args[0]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(USAGE.into());
    }
    let repeat = args.len() == 3;
    let seconds = if repeat { 3600 } else { 30 };
    let window = Duration::from_secs(seconds);
    let tolerance = Duration::from_secs(if repeat { 2 } else { 1 });
    let id = TrackId(args[0].clone());
    let player = Player::new();
    // Set zero before submitting any play request. The production output path
    // applies this value before it queues the first PCM, with no system change.
    player.set_volume(0.0);
    let requested = Instant::now();
    player.play(id.clone());
    let mut started: Option<Instant> = None;
    let mut consumed = Consumed::default();
    let mut quality = None;
    let mut quality_changes = 0;
    let mut buffering = false;
    let mut buffer_events = 0;
    let mut buffer_time = Duration::ZERO;
    let mut polled = Instant::now();
    let interval = if repeat { 60 } else { 10 };
    let mut milestone = interval;
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
        if let Some(error) = player.preparation_failure() {
            return Err(format!("Next-track preparation failed: {error}"));
        }
        match &snapshot.state {
            PlaybackState::Failed(_, error) => return Err(error.to_string()),
            PlaybackState::Loading(current) if started.is_none() && current == &id => (),
            PlaybackState::Playing(current) if current == &id => {
                if !snapshot.position.is_zero() {
                    let handoff = consumed.observe(snapshot.sequence, snapshot.position, repeat)?;
                    if started.is_none() {
                        quality = snapshot.quality;
                        if quality.is_none() {
                            return Err("Stream quality is missing".into());
                        }
                        println!(
                            "playback_started startup_ms={} quality={} requested_seconds={seconds} repeat={repeat} output=default player_volume=0 sign_in=false automatic_retries=false",
                            requested.elapsed().as_millis(),
                            quality.unwrap()
                        );
                    }
                    let elapsed = started.get_or_insert(now).elapsed();
                    if handoff {
                        if snapshot.quality.is_none() {
                            return Err("Incoming stream quality is missing".into());
                        }
                        quality_changes += u64::from(snapshot.quality != quality);
                        quality = snapshot.quality;
                        println!(
                            "muted_probe_handoff sequence={} quality={} observed_consumed_seconds={:.3}",
                            snapshot.sequence,
                            quality.unwrap(),
                            consumed.total.as_secs_f64()
                        );
                    } else if snapshot.quality != quality {
                        return Err("Stream quality changed unexpectedly".into());
                    }
                    if repeat {
                        // The public queue owner normally supplies repeat semantics.
                        // Reuse its production preparation and same-device gapless join.
                        player.prepare_next(Some(id.clone()));
                    }
                    if elapsed.as_secs() >= milestone {
                        println!(
                            "muted_probe_progress wall_seconds={:.3} consumed_delta_seconds={:.3} transitions={} buffer_events={buffer_events} buffering_ms={}",
                            elapsed.as_secs_f64(),
                            consumed.total.as_secs_f64(),
                            snapshot.sequence,
                            buffer_time.as_millis()
                        );
                        milestone += interval;
                    }
                    if elapsed >= window {
                        // Stop before checking/reporting so both a pass and an
                        // assertion failure promptly release queued audio.
                        player.stop();
                        if player.snapshot().state != PlaybackState::Idle {
                            return Err("Stop did not become Idle".into());
                        }
                        if !(window - tolerance..=window + tolerance).contains(&consumed.total) {
                            return Err(
                                "Observed consumption exceeds the allowed wall-clock deviation"
                                    .into(),
                            );
                        }
                        if repeat && snapshot.sequence == 0 {
                            return Err("The repeated workload did not include a handoff".into());
                        }
                        println!(
                            "muted_default_output_probe=passed wall_seconds={:.3} consumed_delta_seconds={:.3} buffer_events={buffer_events} buffering_ms={} transitions={} quality_changes={quality_changes} requested_seconds={seconds} stopped_idle=true output=default player_volume=0",
                            elapsed.as_secs_f64(),
                            consumed.total.as_secs_f64(),
                            buffer_time.as_millis(),
                            snapshot.sequence
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoffs_count_only_observed_audio_and_preserve_same_recording_occurrences() {
        let mut clock = Consumed::default();
        assert!(!clock.observe(0, Duration::from_millis(20), true).unwrap());
        assert!(!clock.observe(0, Duration::from_secs(10), true).unwrap());
        assert!(clock.observe(1, Duration::from_millis(40), true).unwrap());
        assert_eq!(clock.total, Duration::from_millis(10020));
        assert!(!clock.observe(1, Duration::from_secs(2), true).unwrap());
        assert_eq!(clock.total, Duration::from_millis(11980));
    }

    #[test]
    fn regressed_skipped_or_unapproved_handoffs_fail_without_advancing_evidence() {
        for (sequence, position, repeat) in [
            (0, Duration::from_secs(1), true),
            (2, Duration::from_secs(1), true),
            (1, Duration::from_secs(1), false),
        ] {
            let mut clock = Consumed::default();
            clock.observe(0, Duration::from_secs(2), true).unwrap();
            clock.observe(0, Duration::from_secs(3), true).unwrap();
            assert!(clock.observe(sequence, position, repeat).is_err());
            assert_eq!(clock.total, Duration::from_secs(1));
            assert_eq!(clock.previous, Some((0, Duration::from_secs(3))));
        }
        assert!(
            Consumed::default()
                .observe(1, Duration::ZERO, true)
                .is_err()
        );
    }
}
