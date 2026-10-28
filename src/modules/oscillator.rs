use std::f32::consts::TAU;

use crate::engine::module::{Buffer, Module};
use crate::engine::param::Smoothed;

/// Linear frequency modulation, in Hz, added to the base frequency.
/// Negative totals run the phase backwards (through-zero FM).
pub const IN_FM: usize = 0;

pub const OUT_SINE: usize = 0;
pub const OUT_SAW: usize = 1;
pub const OUT_SQUARE: usize = 2;

/// Base frequency in Hz.
pub const FREQ: usize = 0;

/// The saw and square are naive (not band-limited) on purpose: the aliasing
/// is part of the harsh sound this synth is after.
pub struct Oscillator {
    sample_rate: f32,
    phase: f32,
    freq: Smoothed,
}

impl Oscillator {
    pub fn new(sample_rate: f32, freq: f32) -> Self {
        Self {
            sample_rate,
            phase: 0.0,
            freq: Smoothed::new(freq, sample_rate),
        }
    }
}

impl Module for Oscillator {
    fn num_inputs(&self) -> usize {
        1
    }

    fn num_outputs(&self) -> usize {
        3
    }

    fn process(&mut self, inputs: &[Buffer], outputs: &mut [Buffer]) {
        let [sine, saw, square] = outputs else {
            unreachable!("the graph allocates exactly num_outputs() buffers")
        };
        for (i, fm) in inputs[IN_FM].iter().enumerate() {
            let p = self.phase;
            sine[i] = (p * TAU).sin();
            saw[i] = 2.0 * p - 1.0;
            square[i] = if p < 0.5 { 1.0 } else { -1.0 };
            let freq = self.freq.tick() + fm;
            self.phase = (p + freq / self.sample_rate).rem_euclid(1.0);
        }
    }

    fn set_param(&mut self, param: usize, value: f32) {
        if param == FREQ {
            self.freq.set(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::module::BLOCK;
    use crate::modules::test_util::run;

    const SR: f32 = 48_000.0;
    /// One second of audio.
    const BLOCKS: usize = 48_000 / BLOCK;

    fn rising_zero_crossings(signal: &[f32]) -> usize {
        signal
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count()
    }

    #[test]
    fn sine_runs_at_the_set_frequency() {
        let mut osc = Oscillator::new(SR, 440.0);
        let out = run(&mut osc, &[0.0], OUT_SINE, BLOCKS);
        assert!(rising_zero_crossings(&out).abs_diff(440) <= 1);
    }

    #[test]
    fn fm_input_shifts_the_frequency() {
        let mut osc = Oscillator::new(SR, 440.0);
        let out = run(&mut osc, &[110.0], OUT_SINE, BLOCKS);
        assert!(rising_zero_crossings(&out).abs_diff(550) <= 1);
    }

    #[test]
    fn negative_frequency_still_oscillates() {
        let mut osc = Oscillator::new(SR, -440.0);
        let out = run(&mut osc, &[0.0], OUT_SINE, BLOCKS);
        assert!(rising_zero_crossings(&out).abs_diff(440) <= 1);
    }

    #[test]
    fn all_waveforms_stay_within_unit_range() {
        for port in [OUT_SINE, OUT_SAW, OUT_SQUARE] {
            let mut osc = Oscillator::new(SR, 1234.0);
            let out = run(&mut osc, &[5000.0], port, 100);
            assert!(out.iter().all(|s| (-1.0..=1.0).contains(s)));
        }
    }

    #[test]
    fn square_only_takes_two_values() {
        let mut osc = Oscillator::new(SR, 300.0);
        let out = run(&mut osc, &[0.0], OUT_SQUARE, 100);
        assert!(out.iter().all(|&s| s == 1.0 || s == -1.0));
    }

    #[test]
    fn set_param_changes_the_frequency() {
        let mut osc = Oscillator::new(SR, 440.0);
        osc.set_param(FREQ, 880.0);
        run(&mut osc, &[0.0], OUT_SINE, BLOCKS / 10); // let the glide settle
        let out = run(&mut osc, &[0.0], OUT_SINE, BLOCKS);
        assert!(rising_zero_crossings(&out).abs_diff(880) <= 1);
    }
}
