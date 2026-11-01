//! Building blocks shared by the effects.

use std::f32::consts::PI;

/// A delay line with a fractional, interpolated read position, so modulating
/// the delay bends the pitch like tape.
pub struct DelayLine {
    buffer: Vec<f32>,
    write: usize,
}

impl DelayLine {
    /// Allocates the line, so call it before the audio thread starts.
    pub fn new(max_samples: usize) -> Self {
        Self {
            buffer: vec![0.0; max_samples + 2],
            write: 0,
        }
    }

    /// What was pushed `delay` samples ago. Read before pushing the current
    /// sample. Delays outside what the line holds are clamped.
    pub fn read(&self, delay: f32) -> f32 {
        let len = self.buffer.len();
        let delay = delay.clamp(1.0, (len - 2) as f32);
        let position = (self.write as f32 - delay).rem_euclid(len as f32);
        let i = position as usize % len;
        let frac = position.fract();
        let next = (i + 1) % len;
        self.buffer[i] * (1.0 - frac) + self.buffer[next] * frac
    }

    pub fn push(&mut self, sample: f32) {
        self.buffer[self.write] = sample;
        self.write = (self.write + 1) % self.buffer.len();
    }
}

/// A first-order all-pass filter: leaves every frequency's level alone and
/// shifts its phase, by 90° at the frequency its coefficient was made for.
/// Mixed back with the dry signal, the shifted frequencies cancel into notches.
#[derive(Clone, Copy, Default)]
pub struct Allpass {
    state: f32,
}

impl Allpass {
    pub fn process(&mut self, x: f32, coefficient: f32) -> f32 {
        let y = coefficient * x + self.state;
        self.state = x - coefficient * y;
        y
    }
}

/// The coefficient of an [`Allpass`] centered on `freq`. Frequencies are kept
/// below Nyquist, where the filter would stop being stable.
pub fn allpass_coefficient(freq: f32, sample_rate: f32) -> f32 {
    let t = (PI * freq.clamp(1.0, 0.45 * sample_rate) / sample_rate).tan();
    (t - 1.0) / (t + 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    #[test]
    fn a_delay_line_interpolates_between_samples() {
        let mut line = DelayLine::new(10);
        line.push(1.0);
        line.push(3.0);
        assert_eq!(line.read(1.0), 3.0);
        assert_eq!(line.read(2.0), 1.0);
        assert_eq!(line.read(1.5), 2.0);
    }

    #[test]
    fn a_delay_line_clamps_delays_it_cannot_hold() {
        let mut line = DelayLine::new(4);
        for s in [1.0, 2.0, 3.0, 4.0, 5.0] {
            line.push(s);
        }
        assert_eq!(line.read(100.0), line.read(4.0));
        assert_eq!(line.read(0.0), 5.0);
    }

    /// Level and phase of the filter at `freq`, from its response to a sine.
    fn response(coefficient: f32, freq: f32) -> (f32, f32) {
        let mut allpass = Allpass::default();
        let w = std::f32::consts::TAU * freq / SR;
        let (settle, count) = (48_000, 48_000);
        let (mut re, mut im) = (0.0, 0.0);
        for n in 0..settle + count {
            let out = allpass.process((w * n as f32).sin(), coefficient);
            if n >= settle {
                re += out * (w * n as f32).sin();
                im += out * (w * n as f32).cos();
            }
        }
        let scale = 2.0 / count as f32;
        let (re, im) = (re * scale, im * scale);
        ((re * re + im * im).sqrt(), im.atan2(re).to_degrees())
    }

    #[test]
    fn an_allpass_shifts_a_quarter_turn_at_its_frequency_and_keeps_every_level() {
        let coefficient = allpass_coefficient(1000.0, SR);
        let (level, phase) = response(coefficient, 1000.0);
        assert!((level - 1.0).abs() < 0.01, "{level}");
        assert!((phase + 90.0).abs() < 2.0, "{phase}");
        for freq in [100.0, 5000.0] {
            let (level, _) = response(coefficient, freq);
            assert!((level - 1.0).abs() < 0.01, "{level} at {freq} Hz");
        }
    }

    #[test]
    fn the_coefficient_stays_stable_at_any_frequency() {
        for freq in [0.0, 1.0, SR / 2.0, SR, 1e9] {
            assert!(allpass_coefficient(freq, SR).abs() < 1.0, "{freq}");
        }
    }
}
