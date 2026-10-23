//! Real loopback range I/O and decoding, with an in-memory paced output only.
use super::*;
use octave_core::diagnostics::Sensitive;
use octave_provider::ResolvedStream;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    time::Instant,
};

struct Server {
    stop: Arc<AtomicBool>,
    delays: Arc<Mutex<Vec<Duration>>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.worker.take().unwrap().join();
    }
}

fn delayed_reader(shared: &Shared) -> (RangeReader, Server) {
    const DATA_BYTES: u32 = 48000 * 2 * 2 * 6;
    let mut wav = Vec::with_capacity(DATA_BYTES as usize + 44);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(DATA_BYTES + 36).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&48000_u32.to_le_bytes());
    wav.extend_from_slice(&192000_u32.to_le_bytes());
    wav.extend_from_slice(&4_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&DATA_BYTES.to_le_bytes());
    for _ in 0..DATA_BYTES / 2 {
        wav.extend_from_slice(&8192_i16.to_le_bytes());
    }
    let length = wav.len() as u64;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let done = stop.clone();
    let delays = Arc::new(Mutex::new(Vec::new()));
    let measured = delays.clone();
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !done.load(Ordering::Acquire) && Instant::now() < deadline {
            let (mut socket, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Err(error) => panic!("Loopback fixture accept failed: {error}"),
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut input = BufReader::new(socket.try_clone().unwrap());
            let mut range = None;
            loop {
                let mut line = String::new();
                if input.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                let lower = line.to_ascii_lowercase();
                if let Some(value) = lower.strip_prefix("range: bytes=") {
                    let (start, end) = value.trim().split_once('-').unwrap();
                    range = Some((
                        start.parse::<usize>().unwrap(),
                        end.parse::<usize>().unwrap(),
                    ));
                }
            }
            let (start, end) = range.expect("Expected a bounded byte-range request");
            assert!(end >= start && end < wav.len() && end - start < 256 * 1024);
            let began = Instant::now();
            if start > 0 {
                thread::sleep(Duration::from_millis(500));
            }
            let headers = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{length}\r\nConnection: close\r\n\r\n",
                end - start + 1
            );
            if socket.write_all(headers.as_bytes()).is_ok() {
                let _ = socket.write_all(&wav[start..=end]);
            }
            if start > 0 {
                measured.lock().unwrap().push(began.elapsed());
            }
        }
    });
    let reader = RangeReader::new(
        Provider::new().unwrap(),
        ResolvedStream {
            bitrate_bps: Some(1536000),
            track_id: TrackId("fixture0001".into()),
            url: Sensitive::new(format!("http://{address}/audio")),
            content_length: length,
            duration: Duration::from_secs(6),
        },
        shared.generation.clone(),
        0,
    );
    (
        reader,
        Server {
            stop,
            delays,
            worker: Some(worker),
        },
    )
}

struct PlaybackWorker {
    shared: Arc<Shared>,
    worker: Option<thread::JoinHandle<Result<(), Failure>>>,
}
impl Drop for PlaybackWorker {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Release);
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
        if let Some(output) = self.shared.output.lock().unwrap().as_ref() {
            output.stop();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[test]
fn delayed_range_refills_do_not_starve_the_consumed_clock() {
    let shared = Arc::new(Shared::default());
    let (reader, server) = delayed_reader(&shared);
    let fault = reader.fault();
    let length = reader.length();
    let decoder = Decoder::builder()
        .with_data(reader)
        .with_byte_len(length)
        .with_seekable(true)
        .with_hint("wav")
        .build()
        .unwrap();
    let audio = Prepared {
        quality: octave_core::AudioQuality {
            bitrate_bps: Some(1536000),
            sample_rate: 48000,
            channels: 2,
        },
        id: TrackId("fixture0001".into()),
        decoder: Box::new(decoder),
        fault,
        duration: Duration::from_secs(6),
        offset: Duration::ZERO,
        channels: rodio::nz!(2),
        rate: rodio::nz!(48000),
        eq: equalizer::Processor::new(Equalizer::default(), 48000, 2),
        head: VecDeque::new(),
        decoded: 0,
    };
    let request = Request {
        equalizer: Equalizer::default(),
        id: audio.id.clone(),
        offset: Duration::ZERO,
        generation: 0,
        device: None,
    };
    let (output, mut source) = rodio::Player::new();
    let output = Arc::new(output);
    *shared.output.lock().unwrap() = Some(output.clone());
    let active = shared.clone();
    let mut worker = PlaybackWorker {
        shared: shared.clone(),
        worker: Some(thread::spawn(move || {
            run(&active, &request, audio, &output, &AtomicBool::new(false))
        })),
    };
    let player = Player {
        shared: shared.clone(),
    };
    let started = Instant::now();
    let mut tick = started;
    let mut starved = 0;
    let mut peak_queued = 0;
    let mut had_audio = false;
    while !worker.worker.as_ref().unwrap().is_finished() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "Refill fixture timed out"
        );
        let before = player.snapshot();
        assert!(source.by_ref().take(960).all(f32::is_finite));
        let after = player.snapshot();
        if let Some(timeline) = shared.timeline.lock().unwrap().as_ref() {
            peak_queued = peak_queued.max(timeline.current.queued_samples());
        }
        if had_audio
            && before.position < Duration::from_millis(5980)
            && before.position == after.position
        {
            starved += 1;
        }
        had_audio |= !after.position.is_zero();
        tick += Duration::from_millis(10);
        thread::sleep(tick.saturating_duration_since(Instant::now()));
    }
    worker.worker.take().unwrap().join().unwrap().unwrap();
    let final_snapshot = player.snapshot();
    assert_eq!(final_snapshot.position, Duration::from_secs(6));
    assert!(matches!(final_snapshot.state, PlaybackState::Ended(_)));
    let delays = server.delays.lock().unwrap();
    assert!(
        delays.len() >= 3,
        "Must exercise multiple actual range refills"
    );
    println!(
        "range_refill_fixture delayed_ranges={} minimum_refill_ms={} starved_10ms_callbacks={starved} peak_queued_samples={peak_queued} elapsed_ms={}",
        delays.len(),
        delays.iter().min().unwrap().as_millis(),
        started.elapsed().as_millis()
    );
    assert_eq!(
        starved, 0,
        "500ms refills must not exhaust bounded queued audio"
    );
    assert!(peak_queued <= output_budget(48000, 2));
}

#[test]
fn output_budget_covers_normal_refills_and_bounds_extreme_pcm() {
    assert_eq!(output_budget(44100, 2), 176400);
    assert_eq!(output_budget(48000, 2), 192000);
    for rate in [8000, 44100, 48000, 192000] {
        for channels in [1, 2, 6, 8] {
            let budget = output_budget(rate, channels);
            assert!(budget <= MAX_OUTPUT_SAMPLES as u64);
            assert!(budget.is_multiple_of(u64::from(channels)));
            assert!(budget <= u64::from(rate) * u64::from(channels) * 2);
        }
    }
}

#[test]
fn stop_cancels_the_larger_output_queue_without_consuming_stale_pcm() {
    let shared = Arc::new(Shared::default());
    let request = Request {
        id: TrackId("fixture0001".into()),
        offset: Duration::ZERO,
        generation: 0,
        equalizer: Equalizer::default(),
        device: None,
    };
    let mut audio = super::tests::fixture("fixture0001", 0.5, 48000, 2);
    audio.duration = Duration::from_secs(10);
    audio.decoder = Box::new(rodio::source::Zero::new_samples(
        audio.channels,
        audio.rate,
        48000 * 2 * 10,
    ));
    let (output, mut source) = rodio::Player::new();
    let output = Arc::new(output);
    *shared.output.lock().unwrap() = Some(output.clone());
    let active = shared.clone();
    let mut worker = PlaybackWorker {
        shared: shared.clone(),
        worker: Some(thread::spawn(move || {
            run(&active, &request, audio, &output, &AtomicBool::new(false))
        })),
    };
    let player = Player {
        shared: shared.clone(),
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    let clock = loop {
        assert!(Instant::now() < deadline, "Queue did not prebuffer");
        if let Some(timeline) = shared.timeline.lock().unwrap().as_ref()
            && timeline.current.queued_samples() >= output_budget(48000, 2)
        {
            break timeline.current.clone();
        }
        thread::sleep(Duration::from_millis(2));
    };
    player.stop();
    assert!(source.by_ref().take(9600).all(|sample| sample == 0.0));
    worker.worker.take().unwrap().join().unwrap().unwrap();
    assert_eq!(clock.consumed.load(Ordering::Acquire), 0);
    assert_eq!(player.snapshot().state, PlaybackState::Idle);
}
