use std::f32::consts::TAU;

use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};

/// Audio-rate linear FM, scaled by the `fm depth` knob (in Hz per unit) and
/// added to the frequency. Negative totals run the phase backwards
/// (through-zero FM), which is where the metallic, unstable sounds live.
pub const IN_FM: usize = 0;

pub const OUT_SINE: usize = 0;
pub const OUT_SAW: usize = 1;
pub const OUT_SQUARE: usize = 2;

pub const FREQ: usize = 0;
pub const FM_DEPTH: usize = 1;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "osc",
    inputs: &["fm"],
    outputs: &["sine", "saw", "square"],
    params: &[
        ParamInfo {
            name: "freq",
            min: 1.0,
            max: 10_000.0,
            default: 110.0,
            scale: Scale::Exponential,
            unit: "Hz",
        },
        ParamInfo {
            name: "fm depth",
            min: 1.0,
            max: 10_000.0,
            default: 100.0,
            scale: Scale::Exponential,
            unit: "Hz",
        },
    ],
};

/// The saw and square are naive (not band-limited) on purpose: the aliasing
/// is part of the harsh sound this synth is after.
pub struct Oscillator {
    sample_rate: f32,
    phase: f32,
}

impl Oscillator {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            phase: 0.0,
        }
    }
}

impl Module for Oscillator {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        let [sine, saw, square] = outputs else {
            unreachable!("the graph allocates one buffer per output")
        };
        for i in 0..inputs[IN_FM].len() {
            let p = self.phase;
            sine[i] = (p * TAU).sin();
            saw[i] = 2.0 * p - 1.0;
            square[i] = if p < 0.5 { 1.0 } else { -1.0 };
            let freq = params[FREQ][i] + inputs[IN_FM][i] * params[FM_DEPTH][i];
            self.phase = (p + freq / self.sample_rate).rem_euclid(1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::module::BLOCK;
    use crate::modules::test_util::{rising_zero_crossings, run};

    const SR: f32 = 48_000.0;
    /// One second of audio.
    const BLOCKS: usize = 48_000 / BLOCK;

    #[test]
    fn sine_runs_at_the_set_frequency() {
        let out = run(
            &mut Oscillator::new(SR),
            &[0.0],
            &[440.0, 1.0],
            OUT_SINE,
            BLOCKS,
        );
        assert!(rising_zero_crossings(&out).abs_diff(440) <= 1);
    }

    #[test]
    fn fm_input_is_scaled_by_the_depth() {
        let out = run(
            &mut Oscillator::new(SR),
            &[0.5],
            &[440.0, 220.0],
            OUT_SINE,
            BLOCKS,
        );
        assert!(rising_zero_crossings(&out).abs_diff(550) <= 1);
    }

    #[test]
    fn negative_frequency_still_oscillates() {
        let out = run(
            &mut Oscillator::new(SR),
            &[-1.0],
            &[100.0, 540.0],
            OUT_SINE,
            BLOCKS,
        );
        assert!(rising_zero_crossings(&out).abs_diff(440) <= 1);
    }

    #[test]
    fn all_waveforms_stay_within_unit_range() {
        for port in [OUT_SINE, OUT_SAW, OUT_SQUARE] {
            let out = run(
                &mut Oscillator::new(SR),
                &[1.0],
                &[1234.0, 5000.0],
                port,
                100,
            );
            assert!(out.iter().all(|s| (-1.0..=1.0).contains(s)));
        }
    }

    #[test]
    fn square_only_takes_two_values() {
        let out = run(
            &mut Oscillator::new(SR),
            &[0.0],
            &[300.0, 1.0],
            OUT_SQUARE,
            100,
        );
        assert!(out.iter().all(|&s| s == 1.0 || s == -1.0));
    }
}
