use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};

/// Channel indices: the same index names a channel's input, output and gain.
pub const A: usize = 0;
pub const B: usize = 1;
pub const C: usize = 2;

pub const CHANNELS: usize = 3;

const fn gain(name: &'static str) -> ParamInfo {
    ParamInfo {
        name,
        min: -1.0,
        max: 1.0,
        default: 1.0,
        scale: Scale::Linear,
        unit: "",
    }
}

pub static INFO: ModuleInfo = ModuleInfo {
    name: "atten",
    inputs: &["a", "b", "c"],
    outputs: &["a", "b", "c"],
    params: &[gain("gain a"), gain("gain b"), gain("gain c")],
};

/// Three attenuverters: each output is its input times a gain from -1 to 1.
/// A ±1 CV sweeps a knob's whole range, so this is how a modulation is made
/// subtle (small gain) or inverted (negative gain).
pub struct Atten;

impl Module for Atten {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        for channel in 0..CHANNELS {
            let signal = inputs[channel].iter().zip(params[channel]);
            for (out, (s, g)) in outputs[channel].iter_mut().zip(signal) {
                *out = s * g;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::run;

    #[test]
    fn each_channel_scales_its_own_input() {
        let inputs = [1.0, -0.5, 0.8];
        let gains = [0.25, 1.0, -0.5];
        for (channel, expected) in [(A, 0.25), (B, -0.5), (C, -0.4)] {
            let out = run(&mut Atten, &inputs, &gains, channel, 2);
            assert!(out.iter().all(|&s| s == expected), "channel {channel}");
        }
    }
}
