use super::*;
use std::collections::VecDeque;
use transition::Prepared;

struct Clock {
    quality: octave_core::AudioQuality,
    id: TrackId,
    sequence: u64,
    consumed: Arc<AtomicU64>,
    offset: Duration,
    duration: Duration,
    rate: rodio::SampleRate,
    channels: rodio::ChannelCount,
}
impl Clock {
    fn new(audio: &Prepared, sequence: u64) -> Self {
        Self {
            quality: audio.quality,
            id: audio.id.clone(),
            sequence,
            consumed: Arc::new(AtomicU64::new(0)),
            offset: audio.offset,
            duration: audio.duration,
            rate: audio.rate,
            channels: audio.channels,
        }
    }
    fn snapshot(&self, paused: bool, ended: bool) -> Snapshot {
        Snapshot {
            quality: Some(self.quality),
            sequence: self.sequence,
            state: if ended {
                PlaybackState::Ended(self.id.clone())
            } else if paused {
                PlaybackState::Paused(self.id.clone())
            } else {
                PlaybackState::Playing(self.id.clone())
            },
            position: (self.offset
                + Duration::from_secs_f64(
                    self.consumed.load(Ordering::Relaxed) as f64
                        / f64::from(self.rate.get())
                        / f64::from(self.channels.get()),
                ))
            .min(self.duration),
            duration: self.duration,
        }
    }
    fn append(
        &self,
        output: &rodio::Player,
        shared: &Shared,
        request: &Request,
        samples: Vec<f32>,
    ) {
        if !samples.is_empty() && current(shared, request) {
            output.append(Gated {
                inner: SamplesBuffer::new(self.channels, self.rate, samples),
                generation: shared.generation.clone(),
                expected: request.generation,
                consumed: self.consumed.clone(),
            });
        }
    }
}
pub(crate) fn play(shared: &Shared, request: &Request) -> Result<(), Failure> {
    let audio = Prepared::open(shared, request)?;
    if !current(shared, request) {
        return Ok(());
    }
    let failed = Arc::new(AtomicBool::new(false));
    let callback_failed = failed.clone();
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
    run(shared, request, audio, &output, &failed)
}

fn run(
    shared: &Shared,
    request: &Request,
    mut audio: Prepared,
    output: &rodio::Player,
    failed: &AtomicBool,
) -> Result<(), Failure> {
    // Stop queued PCM on errors as well as cancellation/normal completion.
    struct StopOnDrop<'a>(&'a rodio::Player);
    impl Drop for StopOnDrop<'_> {
        fn drop(&mut self) {
            self.0.stop();
        }
    }
    let _stop = StopOnDrop(output);
    let fade = shared
        .crossfade
        .load(Ordering::Acquire)
        .min(u32::from(transition::MAX_FADE)) as usize;
    let mut clock = Clock::new(&audio, 0);
    let mut previous: Option<Clock> = None;
    let mut tail = VecDeque::new();
    let mut ended = false;
    while current(shared, request) {
        if failed.load(Ordering::Acquire) {
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
        if clock.consumed.load(Ordering::Relaxed) > 0 {
            previous = None;
        }
        let visible = previous.as_ref().unwrap_or(&clock);
        publish(
            shared,
            request,
            visible.snapshot(paused, ended && output.empty()),
        );
        if ended && output.empty() {
            break;
        }
        // Let the incoming clock become visible before doing more fallible I/O.
        if paused || output.len() >= 3 || ended || previous.is_some() {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        let count = audio.block_size();
        let keep = transition::tail_budget(audio.rate.get(), audio.channels.get(), fade as u8);
        if tail.len() > keep {
            let count = count.min(tail.len() - keep);
            clock.append(output, shared, request, tail.drain(..count).collect());
            continue;
        }
        let samples = audio.read(count)?;
        if !samples.is_empty() {
            tail.extend(samples);
            continue;
        }
        if !audio.complete() {
            return Err(Failure::Audio(
                "Audio ended before the advertised duration. Retry playback.",
            ));
        }
        let mut preparation = shared.preparation.lock().unwrap();
        let eligible = preparation.plan.as_ref().is_some_and(|plan| {
            plan.generation == request.generation && plan.sequence == clock.sequence
        });
        let next = if eligible {
            preparation.ready.take()
        } else {
            None
        };
        let mut remaining: Vec<_> = tail.drain(..).collect();
        if let Some(mut next) = next {
            let compatible = next.rate == audio.rate && next.channels == audio.channels;
            let mixed = transition::overlap(
                &mut remaining,
                &mut next.head,
                audio.channels.get() as usize,
                if compatible {
                    // Leave at least half of short tracks for ordinary playback/another transition.
                    let next_half = (next.duration.as_secs_f64() * f64::from(next.rate.get()) / 2.0)
                        as usize
                        * next.channels.get() as usize;
                    keep.min(audio.decoded / 2).min(next_half)
                } else {
                    0
                },
            );
            clock.append(output, shared, request, remaining);
            let next_clock = Clock::new(&next, clock.sequence + 1);
            *shared.mix.lock().unwrap() = Some(Mix {
                generation: request.generation,
                sequence: next_clock.sequence,
                outgoing: clock.id.clone(),
                incoming: next_clock.id.clone(),
                duration: Duration::from_secs_f64(
                    mixed.len() as f64
                        / f64::from(audio.rate.get())
                        / f64::from(audio.channels.get()),
                ),
            });
            next_clock.append(output, shared, request, mixed);
            let count = next.block_size().min(next.head.len());
            next_clock.append(output, shared, request, next.head.drain(..count).collect());
            tail = std::mem::take(&mut next.head);
            preparation.committed = preparation.plan.clone();
            previous = Some(clock);
            clock = next_clock;
            audio = next;
        } else {
            clock.append(output, shared, request, remaining);
            ended = true;
        }
    }
    output.stop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Account-free accelerated one-hour pipeline soak; run optimized, takes several minutes"]
    fn one_hour_pipeline_soak() {
        let shared = Arc::new(Shared::default());
        shared.crossfade.store(5, Ordering::Release);
        let request = Request {
            id: TrackId("synthetic".into()),
            offset: Duration::ZERO,
            generation: 0,
            equalizer: Equalizer {
                gains_db: [6.0, -3.0, 2.0],
            },
            device: None,
        };
        let mut audio = fixture("synthetic", 0.0, 48000, 2);
        audio.decoder = Box::new(rodio::source::Zero::new_samples(
            audio.channels,
            audio.rate,
            48000 * 2 * 3600,
        ));
        audio.duration = Duration::from_secs(3600);
        audio.eq = equalizer::Processor::new(request.equalizer, 48000, 2);
        let (output, mut source) = rodio::Player::new();
        let output = Arc::new(output);
        let worker_shared = shared.clone();
        let worker_output = output.clone();
        let worker = thread::spawn(move || {
            run(
                &worker_shared,
                &request,
                audio,
                &worker_output,
                &AtomicBool::new(false),
            )
        });
        let started = std::time::Instant::now();
        let mut milestone = 600;
        while !worker.is_finished() {
            assert!(
                started.elapsed() < Duration::from_secs(600),
                "Pipeline soak timed out"
            );
            if !output.empty() {
                assert!(source.by_ref().take(9600).all(|sample| sample == 0.0));
            }
            let position = shared.snapshot.lock().unwrap().position.as_secs();
            if position >= milestone {
                println!(
                    "pipeline_seconds={position} elapsed_ms={}",
                    started.elapsed().as_millis()
                );
                milestone += 600;
            }
            thread::sleep(Duration::from_millis(1));
        }
        worker.join().unwrap().unwrap();
        let snapshot = shared.snapshot.lock().unwrap();
        assert_eq!(snapshot.position, Duration::from_secs(3600));
        assert_eq!(
            snapshot.state,
            PlaybackState::Ended(TrackId("synthetic".into()))
        );
        println!(
            "pipeline_hour=passed samples=345600000 eq=true retained_tail_seconds=5 elapsed_ms={}",
            started.elapsed().as_millis()
        );
    }
    #[test]
    fn transport_device_and_early_eof_failures_stop_queued_audio() {
        for scenario in 0..3 {
            let shared = Shared::default();
            let request = Request {
                id: TrackId("first".into()),
                offset: Duration::ZERO,
                generation: 0,
                equalizer: Equalizer::default(),
                device: None,
            };
            let mut audio = fixture("first", 0.5, 8000, 2);
            let failed = AtomicBool::new(scenario == 1);
            let expected = match scenario {
                0 => {
                    *audio.fault.lock().unwrap() = Some(octave_provider::Error::Http(403));
                    Failure::Provider(octave_provider::Error::Http(403))
                }
                1 => Failure::Audio(
                    "The audio output device disconnected or failed. Select an output and retry.",
                ),
                _ => {
                    audio.decoder = Box::new(SamplesBuffer::new(
                        audio.channels,
                        audio.rate,
                        Vec::<f32>::new(),
                    ));
                    audio.duration = Duration::from_secs(10);
                    Failure::Audio("Audio ended before the advertised duration. Retry playback.")
                }
            };
            let (output, mut source) = rodio::Player::new();
            output.append(SamplesBuffer::new(
                rodio::nz!(2),
                rodio::nz!(8000),
                vec![0.9; 800],
            ));
            assert_eq!(
                run(&shared, &request, audio, &output, &failed),
                Err(expected)
            );
            assert!(source.by_ref().take(800).all(|sample| sample == 0.0));
            assert!(!matches!(
                shared.snapshot.lock().unwrap().state,
                PlaybackState::Ended(_)
            ));
        }
    }
    fn fixture(id: &str, value: f32, rate: u32, channels: u16) -> Prepared {
        let rate = rodio::SampleRate::new(rate).unwrap();
        let channels = rodio::ChannelCount::new(channels).unwrap();
        Prepared {
            quality: octave_core::AudioQuality {
                bitrate_bps: Some(128000),
                sample_rate: rate.get(),
                channels: channels.get(),
            },
            id: TrackId(id.into()),
            decoder: Box::new(SamplesBuffer::new(
                channels,
                rate,
                vec![value; rate.get() as usize * channels.get() as usize],
            )),
            fault: Arc::new(Mutex::new(None)),
            duration: Duration::from_secs(1),
            offset: Duration::ZERO,
            rate,
            channels,
            eq: equalizer::Processor::new(
                Equalizer::default(),
                rate.get(),
                channels.get() as usize,
            ),
            head: VecDeque::new(),
            decoded: 0,
        }
    }
    fn render(fade: u32, ready: bool, next_rate: u32) -> (Vec<f32>, Snapshot) {
        let shared = Arc::new(Shared::default());
        shared.crossfade.store(fade, Ordering::Release);
        let request = Request {
            id: TrackId("first".into()),
            offset: Duration::ZERO,
            generation: 0,
            equalizer: Equalizer::default(),
            device: None,
        };
        if ready {
            let mut next = fixture("second", -0.5, next_rate, 2);
            next.head = next.read(next_rate as usize * 2).unwrap().into();
            let mut preparation = shared.preparation.lock().unwrap();
            preparation.set(Some(transition::Plan {
                generation: 0,
                sequence: 0,
                next: next.id.clone(),
                seconds: fade as u8,
            }));
            preparation.ready = Some(next);
        }
        let (output, mut source) = rodio::Player::new();
        let output = Arc::new(output);
        let worker_shared = shared.clone();
        let worker_output = output.clone();
        let worker = thread::spawn(move || {
            run(
                &worker_shared,
                &request,
                fixture("first", 0.5, 8000, 2),
                &worker_output,
                &AtomicBool::new(false),
            )
        });
        let mut samples = Vec::new();
        let player = Player {
            shared: shared.clone(),
        };
        let mut mix_seen = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !worker.is_finished() {
            assert!(std::time::Instant::now() < deadline, "render timed out");
            if let TransitionStatus::Mixing {
                elapsed, duration, ..
            } = player.transition_status()
            {
                assert!(elapsed < duration);
                mix_seen = true;
            }
            // Consume 10 ms per turn; give the producer time to refill its bounded queue.
            if !output.empty() {
                samples.extend(source.by_ref().take(160));
            }
            thread::sleep(Duration::from_millis(2));
        }
        worker.join().unwrap().unwrap();
        if fade > 0 && ready && next_rate == 8000 {
            assert!(
                mix_seen,
                "Mixing must be observable only while mixed samples are consumed"
            );
        }
        let snapshot = shared.snapshot.lock().unwrap().clone();
        (samples, snapshot)
    }
    #[test]
    fn prepared_join_and_fade_preserve_samples_through_the_real_output_queue() {
        for fade in [0, 1, 5, 12] {
            let (samples, snapshot) = render(fade, true, 8000);
            let expected = if fade == 0 { 32000 } else { 24000 };
            // Queue keep-alive can append final silence after the last song; never inside the join.
            assert!(samples.len() >= expected);
            if fade == 0 {
                assert!(samples[..16000].iter().all(|s| *s == 0.5));
                assert!(samples[16000..32000].iter().all(|s| *s == -0.5));
            } else {
                assert!(samples[..8000].iter().all(|s| *s == 0.5));
                for (frame, pair) in samples[8000..16000].as_chunks::<2>().0.iter().enumerate() {
                    let expected = 0.5 - (frame + 1) as f32 / 4000.0;
                    assert!((pair[0] - expected).abs() < 0.000001);
                    assert_eq!(pair[0], pair[1]);
                }
                assert!(samples[16000..24000].iter().all(|s| *s == -0.5));
            }
            assert!(samples[expected..].iter().all(|s| *s == 0.0));
            assert_eq!(snapshot.sequence, 1);
            assert_eq!(
                snapshot.state,
                PlaybackState::Ended(TrackId("second".into()))
            );
            assert_eq!(snapshot.position, Duration::from_secs(1));
        }
    }
    #[test]
    fn unprepared_next_retains_the_full_tail_and_format_change_skips_fade() {
        let (samples, snapshot) = render(5, false, 8000);
        assert!(samples[..16000].iter().all(|s| *s == 0.5));
        assert!(samples[16000..].iter().all(|s| *s == 0.0));
        assert_eq!(snapshot.sequence, 0);
        let (samples, snapshot) = render(5, true, 16000);
        assert!(samples[..16000].iter().all(|s| *s == 0.5));
        assert!(samples[16000..48000].iter().all(|s| *s == -0.5));
        assert_eq!(snapshot.sequence, 1);
        assert_eq!(snapshot.quality.unwrap().sample_rate, 16000);
    }
}
