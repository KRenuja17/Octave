//! Opt-in muted live transition probe; repeats one accessible track without an account.
use octave_core::TrackId;
use octave_player::{PlaybackState, Player};
use std::{
    thread,
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let id = TrackId(
        std::env::args()
            .nth(1)
            .ok_or("Usage: transitions VIDEO_ID")?,
    );
    let duration = octave_provider::Provider::new()?.resolve(&id)?.duration;
    if duration < Duration::from_secs(30) {
        return Err("Probe needs a track longer than 30 seconds".into());
    }
    let modes = match std::env::args().nth(2) {
        Some(value) => vec![value.parse::<u8>()?.min(5)],
        None => vec![0, 3],
    };
    for fade in modes {
        let player = Player::new();
        player.set_volume(0.0);
        player.set_crossfade(fade);
        player.play_at(id.clone(), duration - Duration::from_secs(20));
        let start = Instant::now();
        loop {
            let snapshot = player.snapshot();
            if let PlaybackState::Failed(_, error) = snapshot.state {
                return Err(error.to_string().into());
            }
            if snapshot.sequence == 1 && snapshot.position >= Duration::from_millis(100) {
                let quality = snapshot
                    .quality
                    .ok_or("Incoming stream quality was missing")?;
                println!("incoming_quality={quality}");
                break;
            }
            if matches!(snapshot.state, PlaybackState::Ended(_)) {
                return Err(format!(
                    "Next track was not ready for transition: {:?}",
                    player.preparation_failure()
                )
                .into());
            }
            if start.elapsed() > Duration::from_secs(50) {
                return Err("Transition timed out".into());
            }
            player.prepare_next(Some(id.clone()));
            thread::sleep(Duration::from_millis(20));
        }
        println!(
            "incoming_started fade_seconds={fade} elapsed_ms={}",
            start.elapsed().as_millis()
        );
        player.toggle_pause();
        thread::sleep(Duration::from_millis(100));
        let paused = player.snapshot();
        if !matches!(paused.state, PlaybackState::Paused(_)) {
            return Err("Transition pause failed".into());
        }
        thread::sleep(Duration::from_millis(150));
        if player.snapshot().position.saturating_sub(paused.position) > Duration::from_millis(50) {
            return Err("Paused transition kept advancing".into());
        }
        player.seek(Duration::from_secs(10));
        let deadline = Instant::now() + Duration::from_secs(20);
        while !matches!(player.snapshot().state, PlaybackState::Paused(_)) {
            if let PlaybackState::Failed(_, failure) = player.snapshot().state {
                return Err(format!("Paused seek failed: {failure}").into());
            }
            if Instant::now() > deadline {
                return Err("Paused seek timed out".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
        if player.snapshot().sequence != 0 {
            return Err("Seek retained obsolete transition".into());
        }
        player.toggle_pause();
        thread::sleep(Duration::from_millis(200));
        player.stop();
        if player.snapshot().state != PlaybackState::Idle {
            return Err("Stop failed".into());
        }
        println!(
            "transition_passed fade_seconds={fade} elapsed_ms={} paused_incoming_ms={}",
            start.elapsed().as_millis(),
            paused.position.as_millis()
        );
    }
    Ok(())
}
