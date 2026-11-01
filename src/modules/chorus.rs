use std::f32::consts::TAU;

use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};
use crate::modules::dsp::DelayLine;

pub const IN: usize = 0;

pub const OUT: usize = 0;

pub const RATE: usize = 0;
pub const DEPTH: usize = 1;
pub const MIX: usize = 2;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "chorus",
    inputs: &["in"],
    outputs: &["out"],
    output_details: &["input + voices"],
    params: &[
        ParamInfo {
            name: "rate",
            min: 0.05,
            max: 10.0,
            default: 0.8,
            scale: Scale::Exponential,
            unit: "Hz",
        },
        ParamInfo {
            name: "depth",
            min: 0.0,
            max: 1.0,
            default: 0.5,
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

const VOICES: usize = 3;
/// Long enough to hear separate players, short enough not to hear echoes.
const BASE_SECONDS: f32 = 0.02;
const SWING_SECONDS: f32 = 0.008;

/// Copies of the input, each with its delay swaying at a different moment, so
/// they drift slightly in and out of tune like several players together.
pub struct Chorus {
    sample_rate: f32,
    line: DelayLine,
    phase: f32,
}

impl Chorus {
    /// Allocates the delay line, so call it before the audio thread starts.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            line: DelayLine::new(((BASE_SECONDS + SWING_SECONDS) * sample_rate) as usize + 1),
            phase: 0.0,
        }
    }
}

impl Module for Chorus {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        for (i, x) in inputs[IN].iter().enumerate() {
            let swing = SWING_SECONDS * params[DEPTH][i];
            let mut wet = 0.0;
            for voice in 0..VOICES {
                let turn = self.phase + voice as f32 / VOICES as f32;
                let seconds = BASE_SECONDS + swing * (TAU * turn).sin();
                wet += self.line.read(seconds * self.sample_rate);
            }
            // Drifting apart, the voices add up as powers, not as amplitudes.
            wet /= (VOICES as f32).sqrt();
            self.line.push(*x);
            self.phase = (self.phase + params[RATE][i] / self.sample_rate).fract();
            let mix = params[MIX][i];
            outputs[OUT][i] = (1.0 - mix) * x + mix * wet;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::{process_signal, sine, wobbles};

    const SR: f32 = 48_000.0;

    fn impulse_response(depth: f32) -> Vec<f32> {
        let mut impulse = vec![0.0; 4800];
        impulse[0] = 1.0;
        process_signal(&mut Chorus::new(SR), &impulse, &[0.05, depth, 1.0], OUT)
    }

    #[test]
    fn without_depth_the_voices_stay_together() {
        let out = impulse_response(0.0);
        let peak = out.iter().position(|&s| s > 0.5).unwrap();
        assert_eq!(peak, (BASE_SECONDS * SR) as usize);
    }

    #[test]
    fn depth_spreads_the_voices_apart() {
        let out = impulse_response(1.0);
        let heard: Vec<_> = (0..out.len()).filter(|&i| out[i] > 0.05).collect();
        let span = heard.last().unwrap() - heard.first().unwrap();
        let expected = SWING_SECONDS * SR * (TAU / VOICES as f32).sin() * 2.0;
        assert!((span as f32 - expected).abs() < 10.0, "{span}, {expected}");
        assert!(out.iter().all(|&s| s < 0.6), "voices arrive one at a time");
    }

    #[test]
    fn mix_zero_is_the_dry_input() {
        let dry = sine(440.0, SR, 4800);
        let out = process_signal(&mut Chorus::new(SR), &dry, &[1.0, 1.0, 0.0], OUT);
        assert_eq!(out, dry[..out.len()]);
    }

    #[test]
    fn rate_sets_how_fast_the_sound_moves() {
        // Low, so the level moves once per voice per cycle instead
        // of beating faster than `wobbles` can see.
        let count = |rate| wobbles(&mut Chorus::new(SR), 100.0, &[rate, 0.5, 0.5], OUT);
        let (slow, fast) = (count(1.0), count(2.0));
        assert!(slow >= 5, "{slow}");
        assert!(fast.abs_diff(2 * slow) <= 3, "{slow}, {fast}");
    }
}
