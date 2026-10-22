//! Bounded next-track preparation and frame-aligned linear crossfade.
use super::*;
use std::collections::VecDeque;

pub(crate) const MAX_FADE: u8 = 12;
/// Per-segment PCM ceiling: 8 MiB, plus one 100ms decode block.
pub(crate) const MAX_TRANSITION_SAMPLES: usize = 2 * 1024 * 1024;
pub(crate) fn tail_budget(rate: u32, channels: u16, seconds: u8) -> usize {
    (rate as usize * channels as usize * usize::from(seconds.min(MAX_FADE)))
        .min(MAX_TRANSITION_SAMPLES)
        / channels as usize
        * channels as usize
}

#[derive(Clone, PartialEq)]
pub(crate) struct Plan {
    pub generation: u64,
    pub sequence: u64,
    pub next: TrackId,
    pub seconds: u8,
}
#[derive(Default)]
pub(crate) struct Preparation {
    pub failure: Option<Failure>,
    pub committed: Option<Plan>,
    pub plan: Option<Plan>,
    pub ready: Option<Prepared>,
    revision: u64,
}
impl Preparation {
    pub fn set(&mut self, plan: Option<Plan>) {
        if self.plan != plan {
            self.committed = None;
            self.failure = None;
            self.plan = plan;
            self.ready = None;
            self.revision = self.revision.wrapping_add(1);
        }
    }
}
pub(crate) struct Prepared {
    pub quality: octave_core::AudioQuality,
    pub id: TrackId,
    pub decoder: Box<dyn Source<Item = f32> + Send>,
    pub fault: Arc<Mutex<Option<octave_provider::Error>>>,
    pub duration: Duration,
    pub offset: Duration,
    pub channels: rodio::ChannelCount,
    pub rate: rodio::SampleRate,
    pub eq: equalizer::Processor,
    pub head: VecDeque<f32>,
    pub decoded: usize,
}
impl Prepared {
    pub fn open(shared: &Shared, request: &Request) -> Result<Self, Failure> {
        let provider = Provider::new().map_err(Failure::Provider)?;
        let stream = provider
            .resolve_cancellable(&request.id, || !current(shared, request))
            .map_err(Failure::Provider)?;
        let duration = stream.duration;
        let bitrate_bps = stream.bitrate_bps;
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
        let channels = decoder.channels();
        let rate = decoder.sample_rate();
        if channels.get() > 8 || rate.get() > 192000 {
            return Err(Failure::Audio(
                "Unsupported audio channel count or sample rate.",
            ));
        }
        Ok(Self {
            quality: octave_core::AudioQuality {
                bitrate_bps,
                sample_rate: rate.get(),
                channels: channels.get(),
            },
            id: request.id.clone(),
            decoder: Box::new(decoder),
            fault,
            duration,
            offset,
            channels,
            rate,
            eq: equalizer::Processor::new(request.equalizer, rate.get(), channels.get() as usize),
            head: VecDeque::new(),
            decoded: 0,
        })
    }
    pub fn block_size(&self) -> usize {
        self.rate.get() as usize / 10 * self.channels.get() as usize
    }
    pub fn read(&mut self, count: usize) -> Result<Vec<f32>, Failure> {
        let mut samples: Vec<_> = self.decoder.by_ref().take(count).collect();
        if let Some(error) = *self.fault.lock().unwrap() {
            return Err(Failure::Provider(error));
        }
        if !samples.len().is_multiple_of(self.channels.get() as usize) {
            return Err(Failure::Audio("Audio ended inside a channel frame."));
        }
        self.decoded += samples.len();
        self.eq.process(&mut samples);
        Ok(samples)
    }
    pub fn complete(&self) -> bool {
        self.offset
            + Duration::from_secs_f64(
                self.decoded as f64 / f64::from(self.rate.get()) / f64::from(self.channels.get()),
            )
            + Duration::from_secs(2)
            >= self.duration
    }
}
pub(crate) fn work(shared: Arc<Shared>) {
    let mut attempted = None;
    while !shared.closed.load(Ordering::Acquire) {
        let job = {
            let preparation = shared.preparation.lock().unwrap();
            if attempted == Some(preparation.revision) {
                None
            } else {
                attempted = Some(preparation.revision);
                preparation.plan.clone().map(|p| (preparation.revision, p))
            }
        };
        if let Some((revision, plan)) = job {
            let request = Request {
                id: plan.next,
                offset: Duration::ZERO,
                generation: plan.generation,
                equalizer: *shared.equalizer.lock().unwrap(),
                device: None,
            };
            let prepared = Prepared::open(&shared, &request).and_then(|mut p| {
                let count = tail_budget(p.rate.get(), p.channels.get(), MAX_FADE) + p.block_size();
                p.head = p.read(count)?.into();
                if p.head.is_empty() {
                    return Err(Failure::Audio("Next track has no audio."));
                }
                if p.head.len() < count && !p.complete() {
                    return Err(Failure::Audio(
                        "Next track ended before the advertised duration.",
                    ));
                }
                Ok(p)
            });
            let mut preparation = shared.preparation.lock().unwrap();
            if preparation.revision == revision && current(&shared, &request) {
                match prepared {
                    Ok(audio) => preparation.ready = Some(audio),
                    Err(error) => preparation.failure = Some(error),
                }
            }
        } else {
            thread::sleep(Duration::from_millis(15));
        }
    }
}
/// Complementary gains avoid boosting identical correlated signals. Whole frames only.
pub(crate) fn overlap(
    tail: &mut Vec<f32>,
    head: &mut VecDeque<f32>,
    channels: usize,
    max_samples: usize,
) -> Vec<f32> {
    let count = tail.len().min(head.len()).min(max_samples) / channels * channels;
    let start = tail.len() - count;
    let mut mixed = tail.split_off(start);
    let frames = count / channels;
    for (frame, samples) in mixed.chunks_exact_mut(channels).enumerate() {
        let incoming = (frame + 1) as f32 / frames as f32;
        for sample in samples {
            *sample = *sample * (1.0 - incoming) + head.pop_front().unwrap() * incoming;
        }
    }
    mixed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn twelve_second_overlap_preserves_unmixed_audio_and_complementary_gains() {
        let mut tail = vec![0.5; 14 * 48000 * 2];
        let mut head = VecDeque::from(vec![-0.5; 14 * 48000 * 2]);
        let mixed = overlap(&mut tail, &mut head, 2, tail_budget(48000, 2, 12));
        assert_eq!(mixed.len(), 12 * 48000 * 2);
        assert_eq!(tail.len(), 2 * 48000 * 2);
        assert_eq!(head.len(), 2 * 48000 * 2);
        assert!(tail.iter().all(|sample| *sample == 0.5));
        assert!(head.iter().all(|sample| *sample == -0.5));
        assert!(
            mixed
                .iter()
                .all(|sample| sample.is_finite() && sample.abs() <= 0.5)
        );
        assert!(
            mixed
                .as_chunks::<2>()
                .0
                .iter()
                .all(|frame| frame[0] == frame[1])
        );
    }
    #[test]
    fn twelve_second_buffers_are_frame_aligned_and_memory_bounded() {
        assert_eq!(tail_budget(48000, 2, 12), 1_152_000);
        for rate in [8000, 44100, 48000, 192000] {
            for channels in [1, 2, 6, 8] {
                let samples = tail_budget(rate, channels, 12);
                assert!(samples <= MAX_TRANSITION_SAMPLES);
                assert_eq!(samples % usize::from(channels), 0);
            }
        }
    }
    #[test]
    fn overlap_is_frame_aligned_bounded_and_preserves_unmixed_samples() {
        let mut tail = vec![0.5; 12];
        let mut head = VecDeque::from(vec![-0.5; 10]);
        let mixed = overlap(&mut tail, &mut head, 2, 8);
        assert_eq!(tail, vec![0.5; 4]);
        assert_eq!(mixed, vec![0.25, 0.25, 0.0, 0.0, -0.25, -0.25, -0.5, -0.5]);
        assert_eq!(head, vec![-0.5; 2]);
        let mut identical = vec![1.0; 10];
        let mut next = VecDeque::from(vec![1.0; 4]);
        assert_eq!(overlap(&mut identical, &mut next, 2, 100), vec![1.0; 4]);
        assert_eq!(identical.len(), 6);
        assert!(next.is_empty());
    }
    #[test]
    fn zero_overlap_is_exact_concatenation() {
        let mut tail = vec![0.1, 0.2];
        let mut head = VecDeque::from(vec![0.3, 0.4]);
        assert!(overlap(&mut tail, &mut head, 2, 0).is_empty());
        assert_eq!(tail, vec![0.1, 0.2]);
        assert_eq!(head, vec![0.3, 0.4]);
    }
}
