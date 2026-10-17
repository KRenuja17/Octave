//! One playback worker, bounded PCM queue, and generation-based cancellation.
mod devices;
mod equalizer;
pub use devices::{OutputDevice, output_devices};
pub use equalizer::Equalizer;
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
                Error::Network | Error::Timeout | Error::Http(403 | 408 | 429 | 500..=599)
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
        Self { shared }
    }
    pub fn play(&self, id: TrackId) {
        self.request(id, Duration::ZERO, false, Duration::ZERO);
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
        if let Some(output) = self.shared.output.lock().unwrap().as_ref() {
            output.stop();
        }
        self.shared.paused.store(paused, Ordering::Release);
        *self.shared.snapshot.lock().unwrap() = Snapshot {
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
            if let Err(message) = play_request(&shared, &request) {
                let mut snapshot = shared.snapshot.lock().unwrap();
                if current(&shared, &request) {
                    snapshot.state = PlaybackState::Failed(request.id.clone(), message);
                }
            }
            *shared.output.lock().unwrap() = None;
        } else {
            thread::sleep(Duration::from_millis(15));
        }
    }
}
fn play_request(shared: &Shared, request: &Request) -> Result<(), Failure> {
    let provider = Provider::new().map_err(Failure::Provider)?;
    let stream = provider
        .resolve_cancellable(&request.id, || !current(shared, request))
        .map_err(Failure::Provider)?;
    if !current(shared, request) {
        return Ok(());
    }
    let duration = stream.duration;
    let length = stream.content_length;
    let reader = RangeReader::new(
        provider,
        stream,
        shared.generation.clone(),
        request.generation,
    );
    let fault = reader.fault();
    let mut decoder = Decoder::builder()
        .with_data(reader)
        .with_byte_len(length)
        .with_seekable(true)
        .with_hint("m4a")
        .build()
        .map_err(|_| transport_error(&fault, "Audio decoder could not open this stream."))?;
    let offset = request
        .offset
        .min(duration.saturating_sub(Duration::from_millis(100)));
    if !offset.is_zero() {
        decoder
            .try_seek(offset)
            .map_err(|_| transport_error(&fault, "Seeking this stream failed."))?;
    }
    if !current(shared, request) {
        return Ok(());
    }
    let device_failed = Arc::new(AtomicBool::new(false));
    let callback_failed = device_failed.clone();
    let builder = match &request.device {
        Some(selected) => rodio::DeviceSinkBuilder::from_device(selected.device.clone()),
        None => rodio::DeviceSinkBuilder::from_default_device(),
    };
    let mut device = builder
        .map_err(|_| Failure::Audio("No usable audio output device is available."))?
        .with_error_callback(move |_| {
            callback_failed.store(true, Ordering::Release);
        })
        .open_sink_or_fallback()
        .map_err(|_| Failure::Audio("No usable audio output device is available."))?;
    device.log_on_drop(false);
    let output = Arc::new(rodio::Player::connect_new(device.mixer()));
    {
        let mut active = shared.output.lock().unwrap();
        output.set_volume(f32::from_bits(shared.volume.load(Ordering::Acquire)));
        *active = Some(output.clone());
    }
    let channels = decoder.channels();
    let rate = decoder.sample_rate();
    if channels.get() > 8 || rate.get() > 192000 {
        return Err(Failure::Audio(
            "Unsupported audio channel count or sample rate.",
        ));
    }
    let count = (rate.get() as usize / 10) * channels.get() as usize;
    let mut equalizer =
        equalizer::Processor::new(request.equalizer, rate.get(), channels.get() as usize);
    let consumed = Arc::new(AtomicU64::new(0));
    let mut ended = false;
    while current(shared, request) {
        if device_failed.load(Ordering::Acquire) {
            return Err(Failure::Audio(
                "The audio output device disconnected or failed. Select an output and retry.",
            ));
        }
        let paused = shared.paused.load(Ordering::Acquire);
        if paused {
            output.pause();
        } else {
            output.play();
        }
        let position = offset
            + Duration::from_secs_f64(
                consumed.load(Ordering::Relaxed) as f64
                    / f64::from(rate.get())
                    / f64::from(channels.get()),
            );
        // The decoder iterator hides codec errors, so reject an early EOF after
        // all queued PCM has drained rather than reporting a successful finish.
        if ended && output.empty() && position + Duration::from_secs(2) < duration {
            return Err(Failure::Audio(
                "Audio ended before the advertised duration. Retry playback.",
            ));
        }
        let state = if ended && output.empty() {
            PlaybackState::Ended(request.id.clone())
        } else if paused {
            PlaybackState::Paused(request.id.clone())
        } else {
            PlaybackState::Playing(request.id.clone())
        };
        publish(
            shared,
            request,
            Snapshot {
                state,
                position: position.min(duration),
                duration,
            },
        );
        if ended && output.empty() {
            break;
        }
        if paused || output.len() >= 3 || ended {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        let mut samples: Vec<f32> = decoder.by_ref().take(count).collect();
        equalizer.process(&mut samples);
        if let Some(error) = *fault.lock().unwrap() {
            return Err(Failure::Provider(error));
        }
        if samples.is_empty() {
            ended = true;
        } else if current(shared, request) {
            output.append(Gated {
                inner: SamplesBuffer::new(channels, rate, samples),
                generation: shared.generation.clone(),
                expected: request.generation,
                consumed: consumed.clone(),
            });
        }
    }
    output.stop();
    drop(device);
    Ok(())
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
