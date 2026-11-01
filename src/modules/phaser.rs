use std::f32::consts::TAU;

use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};
use crate::modules::dsp::{Allpass, allpass_coefficient};

pub const IN: usize = 0;

pub const OUT: usize = 0;

pub const RATE: usize = 0;
pub const DEPTH: usize = 1;
pub const FREQ: usize = 2;
pub const FEEDBACK: usize = 3;
pub const MIX: usize = 4;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "phaser",
    inputs: &["in"],
    outputs: &["out"],
    output_details: &["input + sweep"],
    params: &[
        ParamInfo {
            name: "rate",
            min: 0.01,
            max: 20.0,
            default: 0.5,
            scale: Scale::Exponential,
            unit: "Hz",
        },
        ParamInfo {
            name: "depth",
            min: 0.0,
            max: 1.0,
            default: 0.7,
            scale: Scale::Linear,
            unit: "",
        },
        ParamInfo {
            name: "freq",
            min: 100.0,
            max: 5000.0,
            default: 800.0,
            scale: Scale::Exponential,
            unit: "Hz",
        },
        ParamInfo {
            name: "feedback",
            min: -1.5,
            max: 1.5,
            default: 0.4,
            scale: Scale::Linear,
            unit: "",
        },
        ParamInfo {
            name: "mix",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            scale: Scale::Linear,
            unit: "",
        },
    ],
};

/// Four stages make two notches, the classic pedal sound.
const STAGES: usize = 4;
/// How far full depth sweeps the stages, either way.
const SWEEP_OCTAVES: f32 = 2.0;

/// All-pass stages swept together: mixed with the dry signal, their phase
/// shift carves notches that move up and down, smoother than a flanger's comb
/// because they are few and unevenly spaced.
pub struct Phaser {
    sample_rate: f32,
    stages: [Allpass; STAGES],
    last: f32,
    phase: f32,
}

impl Phaser {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            stages: [Allpass::default(); STAGES],
            last: 0.0,
            phase: 0.0,
        }
    }
}

impl Module for Phaser {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        for (i, x) in inputs[IN].iter().enumerate() {
            let octaves = SWEEP_OCTAVES * params[DEPTH][i] * (TAU * self.phase).sin();
            let freq = params[FREQ][i] * octaves.exp2();
            let coefficient = allpass_coefficient(freq, self.sample_rate);
            // Through `tanh`, so feedback above 1 sings instead of exploding.
            let mut wet = x + (params[FEEDBACK][i] * self.last).tanh();
            for stage in &mut self.stages {
                wet = stage.process(wet, coefficient);
            }
            self.last = wet;
            self.phase = (self.phase + params[RATE][i] / self.sample_rate).fract();
            let mix = params[MIX][i];
            outputs[OUT][i] = (1.0 - mix) * x + mix * wet;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::{gain_at, process_signal, sine, wobbles};

    const SR: f32 = 48_000.0;

    /// Rate, depth, freq, feedback, mix: still, on 1 kHz, without feedback.
    fn still(feedback: f32, mix: f32) -> [f32; 5] {
        [1.0, 0.0, 1000.0, feedback, mix]
    }

    #[test]
    fn notches_fall_where_the_stages_add_up_to_half_a_turn() {
        // Each of the four stages shifts by 45° at tan(π/8) of its frequency,
        // and by 135° at the inverse.
        let tan = (std::f32::consts::PI / 8.0).tan();
        let gain = |freq| gain_at(&mut Phaser::new(SR), freq, &still(0.0, 0.5), OUT);
        assert!(gain(1000.0 * tan) < 0.05, "{}", gain(1000.0 * tan));
        assert!(gain(1000.0 / tan) < 0.05, "{}", gain(1000.0 / tan));
        assert!(gain(1000.0) > 0.95, "{}", gain(1000.0));
    }

    #[test]
    fn the_wet_signal_alone_keeps_its_level() {
        for freq in [100.0, 1000.0, 8000.0] {
            let gain = gain_at(&mut Phaser::new(SR), freq, &still(0.0, 1.0), OUT);
            assert!((gain - 1.0).abs() < 0.02, "{gain} at {freq} Hz");
        }
    }

    #[test]
    fn feedback_raises_the_peak_between_the_notches() {
        let gain = gain_at(&mut Phaser::new(SR), 1000.0, &still(0.5, 1.0), OUT);
        assert!(gain > 1.5, "{gain}");
    }

    #[test]
    fn mix_zero_is_the_dry_input() {
        let dry = sine(440.0, SR, 4800);
        let params = [1.0, 1.0, 1000.0, 1.0, 0.0];
        let out = process_signal(&mut Phaser::new(SR), &dry, &params, OUT);
        assert_eq!(out, dry[..out.len()]);
    }

    #[test]
    fn rate_sets_how_fast_the_sound_moves() {
        let count = |rate| {
            wobbles(
                &mut Phaser::new(SR),
                1000.0,
                &[rate, 0.5, 1000.0, 0.0, 0.5],
                OUT,
            )
        };
        let (slow, fast) = (count(1.0), count(2.0));
        assert!(slow >= 5, "{slow}");
        assert!(fast.abs_diff(2 * slow) <= 3, "{slow}, {fast}");
    }
}
