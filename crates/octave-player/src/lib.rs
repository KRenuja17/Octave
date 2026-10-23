//! Playback and next-track preparation workers, bounded PCM, and generation cancellation.
pub mod adaptive;
mod devices;
mod equalizer;
mod playback;
mod transition;
pub use devices::{OutputDevice, output_devices};
pub use equalizer::Equalizer;
pub const MAX_CROSSFADE_SECONDS: u8 = transition::MAX_FADE;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionMode {
    Gapless,
    Standard { seconds: u8 },
}
#[derive(Clone, Debug, PartialEq)]
pub enum TransitionStatus {
    Idle,
    Preparing,
    Ready,
    PreparationFailed,
    Mixing {
        outgoing: TrackId,
        incoming: TrackId,
        elapsed: Duration,
        duration: Duration,
    },
    NaturalTransition,
}
#[derive(Clone)]
struct Mix {
    generation: u64,
    sequence: u64,
    outgoing: TrackId,
    incoming: TrackId,
    duration: Duration,
}
use octave_core::TrackId;
use octave_provider::{Provider, range::RangeReader};
use rodio::{Decoder, Source, buffer::SamplesBuffer};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PlaybackState {
    #[default]
    Idle,
    Loading(TrackId),
    Playing(TrackId),
    Paused(TrackId),
    Ended(TrackId),
    Failed(TrackId, Failure),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Provider(octave_provider::Error),
    Audio(&'static str),
}
impl Failure {
    pub fn retryable(&self) -> bool {
        use octave_provider::Error;
        matches!(
            self,
            Self::Provider(
                Error::Network
                    | Error::Busy
                    | Error::Timeout
                    | Error::Http(403 | 408 | 429 | 500..=599)
            )
        )
    }
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provider(e) => e.fmt(f),
            Self::Audio(message) => f.write_str(message),
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub quality: Option<octave_core::AudioQuality>,
    /// Increments when prepared audio for another queue entry starts being consumed.
    pub sequence: u64,
    pub state: PlaybackState,
    pub position: Duration,
    pub duration: Duration,
}
#[derive(Clone)]
struct Request {
    equalizer: Equalizer,
    id: TrackId,
    offset: Duration,
    generation: u64,
    device: Option<OutputDevice>,
}
#[derive(Default)]
struct Shared {
    mix: Mutex<Option<Mix>>,
    preparation: Mutex<transition::Preparation>,
    crossfade: AtomicU32,
    equalizer: Mutex<Equalizer>,
    generation: Arc<AtomicU64>,
    pending: Mutex<Option<Request>>,
    paused: AtomicBool,
    closed: AtomicBool,
    snapshot: Mutex<Snapshot>,
    output: Mutex<Option<Arc<rodio::Player>>>,
    volume: AtomicU32,
    device: Mutex<Option<OutputDevice>>,
}
/// Dropping this handle cancels work. Networking has a bounded timeout and never blocks the UI.
pub struct Player {
    shared: Arc<Shared>,
}
impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}
impl Player {
    pub fn new() -> Self {
        let shared = Arc::new(Shared::default());
        shared.volume.store(0.7_f32.to_bits(), Ordering::Release);
        let worker = shared.clone();
        thread::spawn(move || work(worker));
        let worker = shared.clone();
        thread::spawn(move || transition::work(worker));
        Self { shared }
    }
    pub fn play(&self, id: TrackId) {
        self.request(id, Duration::ZERO, false, Duration::ZERO);
    }
    /// Zero selects gapless; 1..=12 selects a bounded linear crossfade.
    pub fn set_crossfade(&self, seconds: u8) {
        self.set_transition_mode(if seconds == 0 {
            TransitionMode::Gapless
        } else {
            TransitionMode::Standard { seconds }
        });
    }
    pub fn set_transition_mode(&self, mode: TransitionMode) {
        let seconds = match mode {
            TransitionMode::Gapless => 0,
            TransitionMode::Standard { seconds } => seconds.clamp(1, transition::MAX_FADE),
        };
        self.stop();
        self.shared.crossfade.store(
            u32::from(seconds.min(transition::MAX_FADE)),
            Ordering::Release,
        );
    }
    /// Queue owner supplies the next entry, including repeat semantics. No autoplay from Idle.
    pub fn prepare_next(&self, next: Option<TrackId>) {
        let snapshot = self.snapshot();
        let active = matches!(
            snapshot.state,
            PlaybackState::Loading(_) | PlaybackState::Playing(_) | PlaybackState::Paused(_)
        );
        let plan = next.filter(|_| active).map(|next| transition::Plan {
            generation: self.shared.generation.load(Ordering::Acquire),
            sequence: snapshot.sequence,
            next,
            seconds: self.shared.crossfade.load(Ordering::Acquire) as u8,
        });
        let mut preparation = self.shared.preparation.lock().unwrap();
        let retract = preparation.committed.as_ref().is_some_and(|committed| {
            committed.sequence == snapshot.sequence && Some(committed) != plan.as_ref()
        });
        if retract {
            drop(preparation);
            if let PlaybackState::Playing(id) | PlaybackState::Paused(id) = snapshot.state {
                self.request(
                    id,
                    snapshot.position,
                    self.shared.paused.load(Ordering::Acquire),
                    snapshot.duration,
                );
            }
        } else {
            preparation.set(plan);
        }
    }
    /// A preparation failure never interrupts current audio; queue advance retries normally.
    pub fn preparation_failure(&self) -> Option<Failure> {
        self.shared.preparation.lock().unwrap().failure.clone()
    }
    pub fn transition_status(&self) -> TransitionStatus {
        let snapshot = self.snapshot();
        if !matches!(
            snapshot.state,
            PlaybackState::Playing(_) | PlaybackState::Paused(_) | PlaybackState::Loading(_)
        ) {
            return TransitionStatus::Idle;
        }
        let mix = self.shared.mix.lock().unwrap().clone();
        if let Some(mix) = mix.filter(|mix| {
            mix.generation == self.shared.generation.load(Ordering::Acquire)
                && mix.sequence == snapshot.sequence
        }) {
            if !mix.duration.is_zero() && snapshot.position < mix.duration {
                return TransitionStatus::Mixing {
                    outgoing: mix.outgoing,
                    incoming: mix.incoming,
                    elapsed: snapshot.position,
                    duration: mix.duration,
                };
            }
            if mix.duration.is_zero() && snapshot.position < Duration::from_secs(1) {
                return TransitionStatus::NaturalTransition;
            }
        }
        let preparation = self.shared.preparation.lock().unwrap();
        if preparation.failure.is_some() {
            TransitionStatus::PreparationFailed
        } else if preparation.ready.is_some() || preparation.committed.is_some() {
            TransitionStatus::Ready
        } else if preparation.plan.is_some() {
            TransitionStatus::Preparing
        } else {
            TransitionStatus::Idle
        }
    }
    /// Apply on the next explicit play; cancel old PCM to avoid abrupt filter changes.
    pub fn set_equalizer(&self, settings: Equalizer) {
        self.stop();
        *self.shared.equalizer.lock().unwrap() = settings.sanitized();
    }
    /// Stops current/pending playback. Selection is used by the next explicit play.
    /// None follows the system default at the time the next stream is opened.
    pub fn set_output_device(&self, device: Option<OutputDevice>) {
        self.stop();
        *self.shared.device.lock().unwrap() = device;
    }
    /// Explicit resume only; constructing a player never starts audio.
    pub fn play_at(&self, id: TrackId, offset: Duration) {
        self.request(id, offset, false, Duration::ZERO);
    }
    pub fn set_volume(&self, volume: f32) {
        let volume = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            0.7
        };
        let output = self.shared.output.lock().unwrap();
        self.shared
            .volume
            .store(volume.to_bits(), Ordering::Release);
        if let Some(output) = output.as_ref() {
            output.set_volume(volume);
        }
    }
    pub fn seek(&self, offset: Duration) {
        let snapshot = self.snapshot();
        let id = match snapshot.state {
            PlaybackState::Playing(id) | PlaybackState::Paused(id) | PlaybackState::Ended(id) => id,
            PlaybackState::Loading(id) if !snapshot.duration.is_zero() => id,
            _ => return,
        };
        self.request(
            id,
            offset.min(snapshot.duration.saturating_sub(Duration::from_millis(100))),
            self.shared.paused.load(Ordering::Acquire),
            snapshot.duration,
        );
    }
    fn request(&self, id: TrackId, offset: Duration, paused: bool, duration: Duration) {
        let mut pending = self.shared.pending.lock().unwrap();
        let generation = self.shared.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.shared.preparation.lock().unwrap().set(None);
        *self.shared.mix.lock().unwrap() = None;
        if let Some(output) = self.shared.output.lock().unwrap().as_ref() {
            output.stop();
        }
        self.shared.paused.store(paused, Ordering::Release);
        *self.shared.snapshot.lock().unwrap() = Snapshot {
            quality: None,
            sequence: 0,
            state: PlaybackState::Loading(id.clone()),
            position: offset,
            duration,
        };
        *pending = Some(Request {
            equalizer: *self.shared.equalizer.lock().unwrap(),
            id,
            offset,
            generation,
            device: self.shared.device.lock().unwrap().clone(),
        });
    }
    pub fn toggle_pause(&self) {
        let paused = !self.shared.paused.fetch_xor(true, Ordering::AcqRel);
        if let Some(output) = self.shared.output.lock().unwrap().as_ref() {
            if paused {
                output.pause();
            } else {
                output.play();
            }
        }
    }
    pub fn stop(&self) {
        let mut pending = self.shared.pending.lock().unwrap();
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
        self.shared.preparation.lock().unwrap().set(None);
        *pending = None;
        if let Some(output) = self.shared.output.lock().unwrap().as_ref() {
            output.stop();
        }
        *self.shared.snapshot.lock().unwrap() = Snapshot::default();
    }
    pub fn snapshot(&self) -> Snapshot {
        self.shared.snapshot.lock().unwrap().clone()
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
        self.shared.closed.store(true, Ordering::Release);
    }
}
fn current(shared: &Shared, request: &Request) -> bool {
    shared.generation.load(Ordering::Acquire) == request.generation
        && !shared.closed.load(Ordering::Acquire)
}
fn publish(shared: &Shared, request: &Request, snapshot: Snapshot) {
    let mut state = shared.snapshot.lock().unwrap();
    if current(shared, request) {
        *state = snapshot;
    }
}
fn work(shared: Arc<Shared>) {
    while !shared.closed.load(Ordering::Acquire) {
        let request = shared.pending.lock().unwrap().take();
        if let Some(request) = request {
            if let Err(message) = playback::play(&shared, &request) {
                let mut snapshot = shared.snapshot.lock().unwrap();
                if current(&shared, &request) {
                    let id = match &snapshot.state {
                        PlaybackState::Playing(id) | PlaybackState::Paused(id) => id.clone(),
                        _ => request.id.clone(),
                    };
                    snapshot.state = PlaybackState::Failed(id, message);
                }
            }
            *shared.output.lock().unwrap() = None;
        } else {
            thread::sleep(Duration::from_millis(15));
        }
    }
}
fn transport_error(
    fault: &Arc<Mutex<Option<octave_provider::Error>>>,
    fallback: &'static str,
) -> Failure {
    fault
        .lock()
        .unwrap()
        .map(Failure::Provider)
        .unwrap_or(Failure::Audio(fallback))
}
/// The audio callback consumes PCM only; a stale generation becomes silent immediately.
struct Gated {
    inner: SamplesBuffer,
    generation: Arc<AtomicU64>,
    expected: u64,
    consumed: Arc<AtomicU64>,
}
impl Iterator for Gated {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.generation.load(Ordering::Acquire) != self.expected {
            return None;
        }
        let sample = self.inner.next()?;
        self.consumed.fetch_add(1, Ordering::Relaxed);
        Some(sample)
    }
}
impl Source for Gated {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_edits_retract_committed_audio_and_stop_cancels_preparation() {
        let player = Player {
            shared: Arc::new(Shared::default()),
        };
        let id = TrackId("abcdefghijk".into());
        player.play(id.clone());
        *player.shared.snapshot.lock().unwrap() = Snapshot {
            state: PlaybackState::Paused(id),
            position: Duration::from_secs(10),
            ..Default::default()
        };
        player.shared.paused.store(true, Ordering::Release);
        player.prepare_next(Some(TrackId("next".into())));
        {
            let mut p = player.shared.preparation.lock().unwrap();
            p.committed = p.plan.clone();
        }
        let old = player.shared.generation.load(Ordering::Acquire);
        player.prepare_next(Some(TrackId("replacement".into())));
        assert!(player.shared.generation.load(Ordering::Acquire) > old);
        assert!(player.shared.paused.load(Ordering::Acquire));
        assert_eq!(
            player
                .shared
                .pending
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .offset,
            Duration::from_secs(10)
        );
        player.prepare_next(Some(TrackId("replacement".into())));
        player.stop();
        assert!(player.shared.preparation.lock().unwrap().plan.is_none());
        player.prepare_next(Some(TrackId("not autoplay".into())));
        assert!(player.shared.preparation.lock().unwrap().plan.is_none());
    }
    #[test]
    fn recovery_only_retries_transient_provider_failures() {
        use octave_provider::Error;
        for error in [
            Error::Timeout,
            Error::Network,
            Error::Http(403),
            Error::Http(503),
        ] {
            assert!(Failure::Provider(error).retryable());
        }
        for error in [
            Error::SignInRequired,
            Error::UnsupportedStream,
            Error::InvalidTrack,
            Error::Http(404),
        ] {
            assert!(!Failure::Provider(error).retryable());
        }
        assert!(!Failure::Audio("Device disconnected").retryable());
    }
    #[test]
    fn changing_output_cancels_pending_work_without_autoplay() {
        let player = Player {
            shared: Arc::new(Shared::default()),
        };
        player.play_at(TrackId("abcdefghijk".into()), Duration::from_secs(42));
        let old = player.shared.pending.lock().unwrap().clone().unwrap();
        player.set_output_device(None);
        assert!(player.shared.pending.lock().unwrap().is_none());
        assert!(!current(&player.shared, &old));
        publish(
            &player.shared,
            &old,
            Snapshot {
                state: PlaybackState::Playing(old.id.clone()),
                ..Default::default()
            },
        );
        assert_eq!(player.snapshot().state, PlaybackState::Idle);
        player.play_at(old.id, Duration::from_secs(42));
        let next = player.shared.pending.lock().unwrap().clone().unwrap();
        assert!(next.device.is_none());
        assert_eq!(next.offset, Duration::from_secs(42));
    }
    #[test]
    fn resume_offset_and_volume_are_explicit_and_bounded() {
        let player = Player {
            shared: Arc::new(Shared::default()),
        };
        assert_eq!(player.snapshot().state, PlaybackState::Idle);
        player.set_volume(2.0);
        assert_eq!(
            f32::from_bits(player.shared.volume.load(Ordering::Acquire)),
            1.0
        );
        player.set_volume(f32::NAN);
        assert_eq!(
            f32::from_bits(player.shared.volume.load(Ordering::Acquire)),
            0.7
        );
        player.play_at(TrackId("abcdefghijk".into()), Duration::from_secs(42));
        assert_eq!(
            player
                .shared
                .pending
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .offset,
            Duration::from_secs(42)
        );
    }
    #[test]
    fn rapid_requests_keep_latest_seek_and_reject_old_results() {
        let player = Player {
            shared: Arc::new(Shared::default()),
        };
        let id = TrackId("abcdefghijk".into());
        player.play(id.clone());
        let old = player.shared.pending.lock().unwrap().clone().unwrap();
        *player.shared.snapshot.lock().unwrap() = Snapshot {
            sequence: 0,
            quality: None,
            state: PlaybackState::Playing(id),
            duration: Duration::from_secs(200),
            position: Duration::ZERO,
        };
        player.seek(Duration::from_secs(60));
        player.seek(Duration::from_secs(5));
        assert_eq!(
            player
                .shared
                .pending
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .offset,
            Duration::from_secs(5)
        );
        publish(
            &player.shared,
            &old,
            Snapshot {
                state: PlaybackState::Failed(old.id.clone(), Failure::Audio("old error")),
                ..Default::default()
            },
        );
        assert!(matches!(player.snapshot().state, PlaybackState::Loading(_)));
        player.stop();
        assert!(player.shared.pending.lock().unwrap().is_none());
        publish(
            &player.shared,
            &old,
            Snapshot {
                state: PlaybackState::Playing(old.id.clone()),
                ..Default::default()
            },
        );
        assert_eq!(player.snapshot().state, PlaybackState::Idle);
    }
    #[test]
    fn mix_status_uses_current_consumed_clock_and_is_cancelled_by_seek() {
        let player = Player {
            shared: Arc::new(Shared::default()),
        };
        let id = TrackId("incoming".into());
        *player.shared.mix.lock().unwrap() = Some(Mix {
            generation: 0,
            sequence: 1,
            outgoing: TrackId("outgoing".into()),
            incoming: id.clone(),
            duration: Duration::from_secs(4),
        });
        *player.shared.snapshot.lock().unwrap() = Snapshot {
            state: PlaybackState::Paused(id),
            sequence: 1,
            position: Duration::from_secs(1),
            duration: Duration::from_secs(30),
            ..Default::default()
        };
        let status = player.transition_status();
        assert!(
            matches!(status,TransitionStatus::Mixing{elapsed,..} if elapsed==Duration::from_secs(1))
        );
        assert_eq!(player.transition_status(), status); // pause cannot animate a second clock
        player.seek(Duration::from_secs(10));
        assert!(!matches!(
            player.transition_status(),
            TransitionStatus::Mixing { .. }
        ));
        player.stop();
        assert_eq!(player.transition_status(), TransitionStatus::Idle);
    }
    #[test]
    fn stale_audio_stops_without_consuming_more_samples() {
        let generation = Arc::new(AtomicU64::new(1));
        let consumed = Arc::new(AtomicU64::new(0));
        let mut source = Gated {
            inner: SamplesBuffer::new(rodio::nz!(1), rodio::nz!(8000), vec![0.5; 8]),
            generation: generation.clone(),
            expected: 1,
            consumed: consumed.clone(),
        };
        assert_eq!(source.next(), Some(0.5));
        generation.store(2, Ordering::Release);
        assert_eq!(source.next(), None);
        assert_eq!(consumed.load(Ordering::Relaxed), 1);
    }
}
