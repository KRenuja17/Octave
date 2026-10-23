//! Opt-in network/AAC workload. The rodio queue is consumed in memory; this
//! module never opens an output device or starts the public playback worker.
use super::*;
use std::time::Instant;

struct Workers {
    shared: Arc<Shared>,
    threads: Vec<thread::JoinHandle<()>>,
}
impl Drop for Workers {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Release);
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
        if let Some(output) = self.shared.output.lock().unwrap().as_ref() {
            output.stop();
        }
        for worker in self.threads.drain(..) {
            let _ = worker.join();
        }
    }
}

#[test]
#[ignore = "Live unsigned AAC through the in-memory queue; no device; run optimized"]
fn live_aac_network_pipeline() {
    let seconds = std::env::var("OCTAVE_NETWORK_SOAK_SECONDS")
        .unwrap_or_else(|_| "3600".into())
        .parse::<u64>()
        .expect("OCTAVE_NETWORK_SOAK_SECONDS must be an integer");
    assert!((10..=3600).contains(&seconds));
    let started = Instant::now();
    let shared = Arc::new(Shared::default());
    let player = Player {
        shared: shared.clone(),
    };
    shared.crossfade.store(12, Ordering::Release);
    let equalizer = Equalizer {
        gains_db: [6.0, -3.0, 2.0],
    };
    *shared.equalizer.lock().unwrap() = equalizer;
    let id = TrackId("dQw4w9WgXcQ".into());
    let request = Request {
        id: id.clone(),
        offset: Duration::ZERO,
        generation: 0,
        equalizer,
        device: None,
    };
    // Prepared::open only resolves/range-fetches/decodes. Do not call play(),
    // Player::new(), DeviceSinkBuilder, or connect this queue to a mixer/device.
    let audio = Prepared::open(&shared, &request).expect("Initial unsigned AAC open failed");
    assert!(audio.duration >= Duration::from_secs(30));
    let rate = audio.rate;
    let channels = audio.channels;
    let block = audio.block_size();
    let target = seconds * u64::from(rate.get()) * u64::from(channels.get());
    let (output, mut source) = rodio::Player::new();
    let output = Arc::new(output);
    *shared.output.lock().unwrap() = Some(output.clone());
    let mut workers = Workers {
        shared: shared.clone(),
        threads: Vec::new(),
    };
    let active = shared.clone();
    workers
        .threads
        .push(thread::spawn(move || transition::work(active)));
    let active = shared.clone();
    let (finished, result) = std::sync::mpsc::channel();
    workers.threads.push(thread::spawn(move || {
        let outcome = run(&active, &request, audio, &output, &AtomicBool::new(false));
        if let Err(error) = &outcome {
            publish_failure(&active, &request, error.clone());
        }
        let _ = finished.send(outcome);
    }));
    // Retain counters only, never decoded PCM or media URLs. Capturing each
    // timeline before consumption counts outgoing and incoming samples exactly.
    let mut clocks: Vec<Clock> = Vec::new();
    let mut consumed = 0;
    let mut previous = (0, Duration::ZERO);
    let mut milestone = 600;
    let mut buffer_events = 0;
    let mut buffering = false;
    while consumed < target {
        assert!(
            started.elapsed() < Duration::from_secs(660),
            "Network pipeline timed out"
        );
        match result.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => (),
            outcome => panic!("Playback worker ended before the target: {outcome:?}"),
        }
        if let Some(error) = player.preparation_failure() {
            panic!("Next-track preparation failed: {error}");
        }
        let snapshot = player.snapshot();
        assert!(!matches!(
            snapshot.state,
            PlaybackState::Ended(_) | PlaybackState::Failed(_, _)
        ));
        assert!(snapshot.sequence >= previous.0 && snapshot.sequence <= previous.0 + 1);
        if snapshot.sequence == previous.0 {
            assert!(snapshot.position >= previous.1, "Consumed clock regressed");
        }
        previous = (snapshot.sequence, snapshot.position);
        if snapshot.buffering && !buffering {
            buffer_events += 1;
        }
        buffering = snapshot.buffering;
        if let Some(timeline) = shared.timeline.lock().unwrap().as_ref()
            && clocks
                .last()
                .is_none_or(|clock| clock.sequence != timeline.current.sequence)
        {
            assert_eq!(timeline.current.sequence, clocks.len() as u64);
            assert_eq!(timeline.current.rate, rate);
            assert_eq!(timeline.current.channels, channels);
            assert!(clocks.len() < 256, "Counter retention bound");
            clocks.push(timeline.current.clone());
        }
        player.prepare_next(Some(id.clone()));
        assert!(
            source.by_ref().take(block).all(f32::is_finite),
            "Non-finite PCM"
        );
        consumed = clocks
            .iter()
            .map(|clock| clock.consumed.load(Ordering::Acquire))
            .sum();
        let elapsed_audio = consumed / u64::from(rate.get()) / u64::from(channels.get());
        if elapsed_audio >= milestone {
            println!(
                "network_pipeline_seconds={elapsed_audio} transitions={} elapsed_ms={}",
                clocks.len().saturating_sub(1),
                started.elapsed().as_millis()
            );
            milestone += 600;
        }
        // At most 100ms of samples per 10ms: accelerated, with ordinary
        // production queue backpressure and real network/decode work.
        thread::sleep(Duration::from_millis(10));
    }
    player.stop();
    assert_eq!(player.snapshot().state, PlaybackState::Idle);
    drop(workers); // cancel and join both workers, including in-flight ranges
    let outcome = result.recv().expect("Worker result missing");
    assert!(matches!(
        outcome,
        Ok(()) | Err(Failure::Provider(octave_provider::Error::Cancelled))
    ));
    println!(
        "network_pipeline=passed requested_audio_seconds={seconds} consumed_samples={consumed} transitions={} buffer_events={buffer_events} elapsed_ms={} eq=true retained_tail_seconds=12 audio_device_opened=false",
        clocks.len().saturating_sub(1),
        started.elapsed().as_millis()
    );
}
