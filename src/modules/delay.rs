use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};
use crate::modules::dsp::DelayLine;

pub const IN: usize = 0;

pub const OUT: usize = 0;

pub const TIME: usize = 0;
pub const FEEDBACK: usize = 1;
pub const MIX: usize = 2;

const MAX_SECONDS: f32 = 2.0;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "delay",
    inputs: &["in"],
    outputs: &["out"],
    output_details: &["input + echoes"],
    params: &[
        ParamInfo {
            name: "time",
            min: 0.001,
            max: MAX_SECONDS,
            default: 0.25,
            scale: Scale::Exponential,
            unit: "s",
        },
        ParamInfo {
            name: "feedback",
            min: 0.0,
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

/// Echoes, from a line whose modulated time bends the pitch like tape. The
/// feedback path goes through `tanh`: above 1 the echoes grow into a screaming
/// self-oscillation instead of exploding to infinity.
pub struct Delay {
    sample_rate: f32,
    line: DelayLine,
}

impl Delay {
    /// Allocates the delay line, so call it before the audio thread starts.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            line: DelayLine::new((MAX_SECONDS * sample_rate) as usize),
        }
    }
}

impl Module for Delay {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        for (i, x) in inputs[IN].iter().enumerate() {
            let wet = self.line.read(params[TIME][i] * self.sample_rate);
            self.line.push(x + (params[FEEDBACK][i] * wet).tanh());
            let mix = params[MIX][i];
            outputs[OUT][i] = (1.0 - mix) * x + mix * wet;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::module::BLOCK;
    use crate::modules::noise::Noise;
    use crate::modules::test_util::{process_signal, run};

    const SR: f32 = 48_000.0;

    #[test]
    fn an_impulse_comes_back_after_the_delay_time() {
        let mut impulse = vec![0.0; 4800];
        impulse[0] = 1.0;
        // 100 samples, fully wet, no feedback.
        let out = process_signal(&mut Delay::new(SR), &impulse, &[100.0 / SR, 0.0, 1.0], OUT);
        let peak = out.iter().position(|&s| s > 0.5).unwrap();
        assert_eq!(peak, 100);
        assert!(out[200..].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn feedback_repeats_the_echo() {
        let mut impulse = vec![0.0; 4800];
        impulse[0] = 0.5;
        let out = process_signal(&mut Delay::new(SR), &impulse, &[100.0 / SR, 0.5, 1.0], OUT);
        assert!(out[200] > 0.1);
        assert!(out[300] > 0.0 && out[300] < out[200]);
    }

    #[test]
    fn runaway_feedback_stays_bounded() {
        let noise = run(&mut Noise::new(3), &[], &[], 0, 10 * 48_000 / BLOCK);
        let out = process_signal(&mut Delay::new(SR), &noise, &[0.01, 1.5, 1.0], OUT);
        // Each echo is tanh'd, so a sample can hold at most one input plus one.
        assert!(out.iter().all(|s| s.abs() <= 2.0));
    }
}
