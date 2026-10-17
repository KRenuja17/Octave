//! Three peaking bands, Q=0.707, with automatic headroom.
//! Independently implemented from the RBJ equations documented at
//! https://www.w3.org/TR/audio-eq-cookbook/ (no upstream source code copied).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Equalizer {
    pub gains_db: [f32; 3],
}
impl Equalizer {
    pub fn sanitized(self) -> Self {
        Self {
            gains_db: self.gains_db.map(|v| {
                if v.is_finite() {
                    v.clamp(-12.0, 12.0)
                } else {
                    0.0
                }
            }),
        }
    }
    pub fn headroom_db(self) -> f32 {
        -self
            .sanitized()
            .gains_db
            .into_iter()
            .map(|v| v.max(0.0))
            .sum::<f32>()
    }
}
#[derive(Clone, Copy)]
struct Coefficients {
    b: [f64; 3],
    a: [f64; 2],
}
impl Coefficients {
    fn peak(rate: u32, frequency: f64, gain: f32) -> Self {
        let w = std::f64::consts::TAU * frequency.min(f64::from(rate) * 0.45) / f64::from(rate);
        let a = 10_f64.powf(f64::from(gain) / 40.0);
        let alpha = w.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let denominator = 1.0 + alpha / a;
        Self {
            b: [
                (1.0 + alpha * a) / denominator,
                -2.0 * w.cos() / denominator,
                (1.0 - alpha * a) / denominator,
            ],
            a: [
                -2.0 * w.cos() / denominator,
                (1.0 - alpha / a) / denominator,
            ],
        }
    }
}
pub(crate) struct Processor {
    filters: [Coefficients; 3],
    state: Vec<[[f64; 2]; 3]>,
    channel: usize,
    gain: f64,
    bypass: bool,
}
impl Processor {
    pub fn new(settings: Equalizer, rate: u32, channels: usize) -> Self {
        assert!(rate > 0 && (1..=8).contains(&channels));
        let settings = settings.sanitized();
        Self {
            filters: std::array::from_fn(|i| {
                Coefficients::peak(rate, [100.0, 1000.0, 10000.0][i], settings.gains_db[i])
            }),
            state: vec![[[0.0; 2]; 3]; channels],
            channel: 0,
            gain: 10_f64.powf(f64::from(settings.headroom_db()) / 20.0),
            bypass: settings.gains_db == [0.0; 3],
        }
    }
    pub fn process(&mut self, samples: &mut [f32]) {
        if self.bypass {
            return;
        }
        for sample in samples {
            let mut value = if sample.is_finite() {
                f64::from(*sample) * self.gain
            } else {
                0.0
            };
            for (filter, state) in self.filters.iter().zip(self.state[self.channel].iter_mut()) {
                let output = filter.b[0] * value + state[0];
                state[0] = filter.b[1] * value - filter.a[0] * output + state[1];
                state[1] = filter.b[2] * value - filter.a[1] * output;
                value = output;
            }
            *sample = value.clamp(-1.0, 1.0) as f32;
            self.channel = (self.channel + 1) % self.state.len();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flat_is_bit_exact_and_blocks_do_not_change_filter_history() {
        let signal: Vec<f32> = (0..20000)
            .map(|n| ((n as f32) * 0.11).sin() * 0.2)
            .collect();
        let mut flat = signal.clone();
        Processor::new(Equalizer::default(), 48000, 2).process(&mut flat);
        assert_eq!(flat, signal);
        let eq = Equalizer {
            gains_db: [6.0, -4.0, 3.0],
        };
        let mut whole = signal.clone();
        Processor::new(eq, 48000, 2).process(&mut whole);
        let mut pieces = signal;
        let mut processor = Processor::new(eq, 48000, 2);
        for chunk in pieces.chunks_mut(137) {
            processor.process(chunk);
        }
        assert_eq!(pieces, whole);
    }
    #[test]
    fn bands_have_expected_response_and_channels_are_independent() {
        for (band, frequency) in [100.0, 1000.0, 10000.0].into_iter().enumerate() {
            for db in [-12.0, 6.0, 12.0] {
                let mut gains = [0.0; 3];
                gains[band] = db;
                let eq = Equalizer { gains_db: gains };
                let mut signal: Vec<f32> = (0..48000)
                    .flat_map(|n| {
                        [
                            ((std::f64::consts::TAU * frequency * f64::from(n) / 48000.0).sin()
                                * 0.1) as f32,
                            0.0,
                        ]
                    })
                    .collect();
                Processor::new(eq, 48000, 2).process(&mut signal);
                let rms = (signal[48000..]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|frame| {
                        assert_eq!(frame[1], 0.0);
                        f64::from(frame[0]).powi(2)
                    })
                    .sum::<f64>()
                    / 24000.0)
                    .sqrt();
                let measured = 20.0 * (rms / (0.1 / std::f64::consts::SQRT_2)).log10();
                assert!(
                    (measured - f64::from(db + eq.headroom_db())).abs() < 0.05,
                    "band={band} measured={measured}"
                );
            }
        }
    }
    #[test]
    fn extremes_remain_finite_and_bounded_across_supported_rates() {
        assert_eq!(
            Equalizer {
                gains_db: [f32::NAN, 99.0, -99.0]
            }
            .sanitized()
            .gains_db,
            [0.0, 12.0, -12.0]
        );
        for rate in [8000, 44100, 48000, 192000] {
            for gains_db in [[12.0; 3], [-12.0; 3], [12.0, -12.0, 12.0]] {
                let mut signal = vec![1.0; rate as usize * 8];
                signal[0] = f32::NAN;
                Processor::new(Equalizer { gains_db }, rate, 8).process(&mut signal);
                assert!(signal.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
            }
        }
    }
}
