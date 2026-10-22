//! Independent research prototype. Never changes production playback.
//!
//! Feed only the desired intro/outro window from an existing decoder, not an
//! entire retained track. This estimates periodic onsets, not vocals, phrases,
//! musical downbeats or a reliable beat grid for arbitrary recordings.
use octave_core::TrackId;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub const MAX_WINDOW_SECONDS: u32 = 60;
const BINS_PER_SECOND: usize = 100;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidFormat,
    InvalidPcm,
    Cancelled,
    InvalidMapping,
}
#[derive(Clone, Copy, Debug)]
pub struct BeatEstimate {
    pub bpm: f32,
    pub confidence: f32,
    /// Offset within the analyzed source window, not a musical downbeat claim.
    pub first_onset: Duration,
}
#[derive(Clone, Copy, Debug)]
pub struct Analysis {
    pub source_start: Duration,
    pub duration: Duration,
    pub rms: f32,
    pub beat: Option<BeatEstimate>,
}
pub struct Analyzer {
    rate: u32,
    channels: usize,
    frames: usize,
    bin_frames: usize,
    bin_power: f64,
    total_power: f64,
    envelope: Vec<f32>,
    source_start: Duration,
    error: Option<Error>,
}
impl Analyzer {
    pub fn new(rate: u32, channels: usize, source_start: Duration) -> Result<Self, Error> {
        if !(8000..=192000).contains(&rate)
            || !(1..=8).contains(&channels)
            || source_start > Duration::from_secs(24 * 60 * 60)
        {
            return Err(Error::InvalidFormat);
        }
        Ok(Self {
            rate,
            channels,
            frames: 0,
            bin_frames: 0,
            bin_power: 0.0,
            total_power: 0.0,
            envelope: Vec::with_capacity(6000),
            source_start,
            error: None,
        })
    }
    /// Returns frames consumed. Stops at 60 seconds, retaining at most 6,000
    /// energy values (24 KiB), regardless of channel count or sample rate.
    /// Caller owns scheduling; cancellation is checked at least every 10ms of PCM.
    pub fn feed(&mut self, interleaved: &[f32], cancel: &AtomicBool) -> Result<usize, Error> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if !interleaved.len().is_multiple_of(self.channels) {
            self.error = Some(Error::InvalidPcm);
            return Err(Error::InvalidPcm);
        }
        let remaining = self.rate as usize * MAX_WINDOW_SECONDS as usize - self.frames;
        let mut consumed = 0;
        for frame in interleaved.chunks_exact(self.channels).take(remaining) {
            if self.bin_frames == 0 && cancel.load(Ordering::Relaxed) {
                self.error = Some(Error::Cancelled);
                return Err(Error::Cancelled);
            }
            if frame.iter().any(|s| !s.is_finite() || s.abs() > 8.0) {
                self.error = Some(Error::InvalidPcm);
                return Err(Error::InvalidPcm);
            }
            let power =
                frame.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / self.channels as f64;
            self.bin_power += power;
            self.total_power += power;
            self.bin_frames += 1;
            self.frames += 1;
            consumed += 1;
            // Rational boundaries also handle rates such as 44,100Hz exactly.
            if self.frames * BINS_PER_SECOND / self.rate as usize > self.envelope.len() {
                self.envelope
                    .push((self.bin_power / self.bin_frames as f64).sqrt() as f32);
                self.bin_frames = 0;
                self.bin_power = 0.0;
            }
        }
        Ok(consumed)
    }
    pub fn retained_bytes(&self) -> usize {
        self.envelope.capacity() * size_of::<f32>()
    }
    pub fn finish(self, cancel: &AtomicBool) -> Result<Analysis, Error> {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        if let Some(error) = self.error {
            return Err(error);
        }
        let rms = (self.total_power / self.frames.max(1) as f64).sqrt() as f32;
        let beat = estimate(&self.envelope, rms, cancel)?;
        Ok(Analysis {
            source_start: self.source_start,
            duration: Duration::from_secs_f64(self.frames as f64 / f64::from(self.rate)),
            rms,
            beat,
        })
    }
}
fn estimate(
    envelope: &[f32],
    rms: f32,
    cancel: &AtomicBool,
) -> Result<Option<BeatEstimate>, Error> {
    if envelope.len() < 800 || rms < 0.001 {
        return Ok(None);
    }
    let onset: Vec<f32> = envelope
        .iter()
        .enumerate()
        .map(|(i, e)| (e - i.checked_sub(1).map_or(0.0, |i| envelope[i])).max(0.0))
        .collect();
    let mean = onset.iter().sum::<f32>() / onset.len() as f32;
    let variance = onset.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / onset.len() as f32;
    if variance < 0.000001 {
        return Ok(None);
    }
    let threshold = mean + 2.0 * variance.sqrt();
    let peaks: Vec<_> = onset
        .iter()
        .enumerate()
        .filter(|(_, v)| **v > threshold)
        .map(|(i, _)| i)
        .collect();
    if peaks.len() < 8 {
        return Ok(None);
    }
    let mut best = (0.0_f32, 0_usize);
    // 70–180 BPM at 10ms resolution. This deliberately leaves half/double-tempo
    // ambiguity unresolved; candidate output is for controlled comparison only.
    for lag in 33..=86 {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let (mut cross, mut left, mut right) = (0.0_f64, 0.0_f64, 0.0_f64);
        for i in lag..onset.len() {
            let a = f64::from(onset[i] - mean);
            let b = f64::from(onset[i - lag] - mean);
            cross += a * b;
            left += a * a;
            right += b * b;
        }
        let score = (cross / (left * right).sqrt().max(f64::EPSILON)) as f32;
        if score > best.0 + 0.005 {
            best = (score, lag);
        }
    }
    if best.0 < 0.8 {
        return Ok(None);
    }
    Ok(Some(BeatEstimate {
        bpm: 6000.0 / best.1 as f32,
        confidence: best.0.clamp(0.0, 1.0),
        first_onset: Duration::from_millis(peaks[0] as u64 * 10),
    }))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallback {
    LowConfidence,
    IncompatibleTempo,
    IncompatibleEnergy,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Decision {
    Natural(Fallback),
    /// A proposal only. No resampler, overlap, bass handoff or vocal avoidance
    /// is enabled by this value; production playback does not consume it.
    Candidate {
        incoming_rate_ratio: f32,
        confidence: f32,
        max_overlap: Duration,
    },
}
pub fn propose(outgoing: &Analysis, incoming: &Analysis) -> Decision {
    let (Some(a), Some(b)) = (outgoing.beat, incoming.beat) else {
        return Decision::Natural(Fallback::LowConfidence);
    };
    if !a.bpm.is_finite()
        || !b.bpm.is_finite()
        || !a.confidence.is_finite()
        || !b.confidence.is_finite()
        || a.confidence.min(b.confidence) < 0.8
        || a.confidence.max(b.confidence) > 1.0
        || !(69.0..=183.0).contains(&a.bpm)
        || !(69.0..=183.0).contains(&b.bpm)
        || outgoing.duration < Duration::from_secs(8)
        || incoming.duration < Duration::from_secs(8)
    {
        return Decision::Natural(Fallback::LowConfidence);
    }
    let ratio = a.bpm / b.bpm;
    if !(0.97..=1.03).contains(&ratio) {
        return Decision::Natural(Fallback::IncompatibleTempo);
    }
    if !outgoing.rms.is_finite()
        || !incoming.rms.is_finite()
        || outgoing.rms.min(incoming.rms) < 0.001
        || outgoing.rms.max(incoming.rms) / outgoing.rms.min(incoming.rms) > 4.0
    {
        return Decision::Natural(Fallback::IncompatibleEnergy);
    }
    Decision::Candidate {
        incoming_rate_ratio: ratio,
        confidence: a.confidence.min(b.confidence),
        max_overlap: Duration::from_secs(12)
            .min(outgoing.duration / 2)
            .min(incoming.duration / 2),
    }
}

/// An explicit source-time contract for later skip/loop/stretch experiments.
/// Each occurrence has a separate segment; output must remain contiguous.
#[derive(Clone, Debug)]
pub struct Segment {
    pub output_start: Duration,
    pub output_duration: Duration,
    pub source_start: Duration,
    pub source_duration: Duration,
}
#[derive(Clone, Debug)]
pub struct SourceMap {
    pub track: TrackId,
    segments: Vec<Segment>,
}
impl SourceMap {
    pub fn new(track: TrackId, segments: Vec<Segment>) -> Result<Self, Error> {
        if segments.is_empty() || segments.len() > 64 {
            return Err(Error::InvalidMapping);
        }
        let mut end = Duration::ZERO;
        for segment in &segments {
            if segment.output_start != end
                || segment.output_duration.is_zero()
                || segment.source_duration.is_zero()
                || segment.source_duration > Duration::from_secs(120)
                || segment.source_start > Duration::from_secs(24 * 60 * 60)
                || !(0.5..=2.0).contains(
                    &(segment.source_duration.as_secs_f64()
                        / segment.output_duration.as_secs_f64()),
                )
            {
                return Err(Error::InvalidMapping);
            }
            end = end
                .checked_add(segment.output_duration)
                .ok_or(Error::InvalidMapping)?;
            if end > Duration::from_secs(120) {
                return Err(Error::InvalidMapping);
            }
        }
        Ok(Self { track, segments })
    }
    /// Half-open segments map the exact handoff to the new occurrence. There is
    /// no source value beyond the final output frame, including its end point.
    pub fn source_at(&self, output: Duration) -> Option<Duration> {
        let s = self
            .segments
            .iter()
            .find(|s| output >= s.output_start && output < s.output_start + s.output_duration)?;
        let offset = (output - s.output_start).as_nanos() * s.source_duration.as_nanos()
            / s.output_duration.as_nanos();
        Some(s.source_start + Duration::from_nanos(offset as u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn pulses(bpm: f32, seconds: usize) -> Analysis {
        let cancel = AtomicBool::new(false);
        let mut analyzer = Analyzer::new(8000, 1, Duration::ZERO).unwrap();
        for second in 0..seconds {
            let pcm: Vec<_> = (second * 8000..(second + 1) * 8000)
                .map(|i| {
                    if (i as f32 / 8000.0) % (60.0 / bpm) < 0.03 {
                        0.7
                    } else {
                        0.0
                    }
                })
                .collect();
            analyzer.feed(&pcm, &cancel).unwrap();
        }
        assert!(analyzer.retained_bytes() <= 24_000);
        analyzer.finish(&cancel).unwrap()
    }
    #[test]
    fn periodic_windows_propose_bounded_candidate_and_reject_incompatible_pairs() {
        let a = pulses(120.0, 30);
        let beat = a.beat.unwrap();
        assert!((beat.bpm - 120.0).abs() < 1.0 && beat.confidence > 0.95);
        assert!(
            matches!(propose(&a,&a), Decision::Candidate {max_overlap,..} if max_overlap==Duration::from_secs(12))
        );
        assert_eq!(
            propose(&a, &pulses(90.0, 30)),
            Decision::Natural(Fallback::IncompatibleTempo)
        );
        let quiet = Analysis { rms: 0.001, ..a };
        assert_eq!(
            propose(&a, &quiet),
            Decision::Natural(Fallback::IncompatibleEnergy)
        );
        let invalid = Analysis {
            beat: Some(BeatEstimate {
                bpm: f32::NAN,
                ..beat
            }),
            ..a
        };
        assert_eq!(
            propose(&a, &invalid),
            Decision::Natural(Fallback::LowConfidence)
        );
    }
    #[test]
    fn silence_noise_short_malformed_cancelled_and_oversized_windows_are_bounded() {
        let cancel = AtomicBool::new(false);
        let mut analyzer = Analyzer::new(8000, 1, Duration::ZERO).unwrap();
        assert_eq!(
            analyzer.feed(&vec![0.0; 8000 * 61], &cancel).unwrap(),
            8000 * 60
        );
        assert_eq!(analyzer.feed(&[0.0], &cancel).unwrap(), 0);
        assert_eq!(analyzer.retained_bytes(), 24_000);
        assert!(analyzer.finish(&cancel).unwrap().beat.is_none());
        assert!(pulses(120.0, 4).beat.is_none());
        let mut noise = Analyzer::new(8000, 1, Duration::ZERO).unwrap();
        let mut seed = 17_u32;
        for _ in 0..20 {
            let samples: Vec<_> = (0..8000)
                .map(|_| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    (seed as f32 / u32::MAX as f32 - 0.5) * 0.5
                })
                .collect();
            noise.feed(&samples, &cancel).unwrap();
        }
        assert!(noise.finish(&cancel).unwrap().beat.is_none());
        let mut bad = Analyzer::new(44100, 2, Duration::ZERO).unwrap();
        assert_eq!(bad.feed(&[0.0], &cancel), Err(Error::InvalidPcm));
        assert!(matches!(bad.finish(&cancel), Err(Error::InvalidPcm)));
        let mut bad = Analyzer::new(44100, 1, Duration::ZERO).unwrap();
        assert_eq!(bad.feed(&[f32::NAN], &cancel), Err(Error::InvalidPcm));
        let mut stopped = Analyzer::new(48000, 2, Duration::ZERO).unwrap();
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(stopped.feed(&[0.0, 0.0], &cancel), Err(Error::Cancelled));
        assert!(matches!(stopped.finish(&cancel), Err(Error::Cancelled)));
        assert!(Analyzer::new(0, 1, Duration::ZERO).is_err());
    }
    #[test]
    fn source_mapping_handles_skips_repeated_occurrences_and_stretch_boundaries() {
        let seconds = Duration::from_secs;
        let map = SourceMap::new(
            TrackId("fixture".into()),
            vec![
                Segment {
                    output_start: seconds(0),
                    output_duration: seconds(4),
                    source_start: seconds(2),
                    source_duration: seconds(4),
                },
                Segment {
                    output_start: seconds(4),
                    output_duration: seconds(4),
                    source_start: seconds(2),
                    source_duration: seconds(4),
                },
                Segment {
                    output_start: seconds(8),
                    output_duration: seconds(4),
                    source_start: seconds(6),
                    source_duration: seconds(2),
                },
            ],
        )
        .unwrap();
        assert_eq!(map.source_at(seconds(0)), Some(seconds(2)));
        assert_eq!(map.source_at(seconds(3)), Some(seconds(5)));
        assert_eq!(map.source_at(seconds(4)), Some(seconds(2)));
        assert_eq!(map.source_at(seconds(10)), Some(seconds(7)));
        assert_eq!(map.source_at(seconds(12)), None);
        let mut invalid = map.segments.clone();
        invalid[1].output_start = seconds(5);
        assert!(SourceMap::new(map.track, invalid).is_err());
    }
}
