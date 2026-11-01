use std::f32::consts::TAU;

use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};
use crate::modules::dsp::DelayLine;

pub const IN: usize = 0;

pub const OUT: usize = 0;

pub const RATE: usize = 0;
pub const DEPTH: usize = 1;
pub const TIME: usize = 2;
pub const FEEDBACK: usize = 3;
pub const MIX: usize = 4;

const MAX_SECONDS: f32 = 0.02;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "flanger",
    inputs: &["in"],
    outputs: &["out"],
    output_details: &["input + sweep"],
    params: &[
        ParamInfo {
            name: "rate",
            min: 0.01,
            max: 20.0,
            default: 0.3,
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
            name: "time",
            min: 0.0002,
            max: MAX_SECONDS,
            default: 0.003,
            scale: Scale::Exponential,
            unit: "s",
        },
        ParamInfo {
            name: "feedback",
            min: -1.5,
            max: 1.5,
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

/// A delay so short that it is heard as a timbre: mixed with the dry signal,
/// it carves a comb of notches, and sweeping the delay sweeps the comb.
///
/// It is not a patch of `delay` and `lfo` because its feedback lives inside
/// the milliseconds a cable's block of latency would already use up. Negative
/// feedback sounds hollow; above 1 it self-oscillates through `tanh`, like the
/// delay.
pub struct Flanger {
    sample_rate: f32,
    line: DelayLine,
    phase: f32,
}

impl Flanger {
    /// Allocates the delay line, so call it before the audio thread starts.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            // The sweep reaches twice the `time` knob.
            line: DelayLine::new((2.0 * MAX_SECONDS * sample_rate) as usize + 1),
            phase: 0.0,
        }
    }
}

impl Module for Flanger {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        for (i, x) in inputs[IN].iter().enumerate() {
            let sweep = 1.0 + params[DEPTH][i] * (TAU * self.phase).sin();
            let wet = self.line.read(params[TIME][i] * sweep * self.sample_rate);
            self.line.push(x + (params[FEEDBACK][i] * wet).tanh());
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
    const ONE_MS: f32 = 0.001;

    #[test]
    fn the_comb_cancels_where_the_delay_is_half_a_period() {
        let gain = |freq| {
            gain_at(
                &mut Flanger::new(SR),
                freq,
                &[1.0, 0.0, ONE_MS, 0.0, 0.5],
                OUT,
            )
        };
        assert!(gain(500.0) < 0.05, "{}", gain(500.0));
        assert!(gain(1500.0) < 0.05, "{}", gain(1500.0));
        assert!(gain(1000.0) > 0.95, "{}", gain(1000.0));
    }

    #[test]
    fn positive_feedback_deepens_the_peaks_and_negative_moves_them() {
        let gain = |feedback, freq| {
            let params = [1.0, 0.0, ONE_MS, feedback, 1.0];
            gain_at(&mut Flanger::new(SR), freq, &params, OUT)
        };
        assert!(gain(0.5, 1000.0) > 1.5, "{}", gain(0.5, 1000.0));
        assert!(gain(-0.5, 1000.0) < 0.8, "{}", gain(-0.5, 1000.0));
        assert!(gain(-0.5, 500.0) > 1.5, "{}", gain(-0.5, 500.0));
    }

    #[test]
    fn mix_zero_is_the_dry_input() {
        let dry = sine(440.0, SR, 4800);
        let out = process_signal(
            &mut Flanger::new(SR),
            &dry,
            &[1.0, 1.0, ONE_MS, 1.0, 0.0],
            OUT,
        );
        assert_eq!(out, dry[..out.len()]);
    }

    #[test]
    fn rate_sets_how_fast_the_sound_moves() {
        let count = |rate| {
            wobbles(
                &mut Flanger::new(SR),
                1000.0,
                &[rate, 0.5, ONE_MS, 0.0, 0.5],
                OUT,
            )
        };
        let (slow, fast) = (count(1.0), count(2.0));
        assert!(slow >= 5, "{slow}");
        assert!(fast.abs_diff(2 * slow) <= 3, "{slow}, {fast}");
    }

    #[test]
    fn the_longest_sweep_fits_in_the_line() {
        let mut impulse = vec![0.0; 4800];
        impulse[0] = 1.0;
        let params = [0.01, 1.0, MAX_SECONDS, 0.0, 1.0];
        // A quarter of the way through a very slow cycle is close to the top.
        let mut flanger = Flanger::new(SR);
        flanger.phase = 0.25;
        let out = process_signal(&mut flanger, &impulse, &params, OUT);
        let peak = out.iter().position(|&s| s > 0.4).unwrap();
        assert!(
            peak.abs_diff((2.0 * MAX_SECONDS * SR) as usize) <= 2,
            "{peak}"
        );
    }
}
