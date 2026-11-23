use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};

pub const CHANNELS: usize = 4;

pub const OUT: usize = 0;

/// Parameter index of an input's mute switch, `channel` counted from 0.
pub const fn mute(channel: usize) -> usize {
    CHANNELS + channel
}

const fn level(name: &'static str) -> ParamInfo {
    ParamInfo {
        name,
        min: 0.0,
        max: 1.0,
        default: 1.0,
        scale: Scale::Linear,
        unit: "",
        help: "How much of this input goes into the mix.",
    }
}

const fn mute_switch(name: &'static str) -> ParamInfo {
    ParamInfo {
        name,
        min: 0.0,
        max: 1.0,
        default: 0.0,
        scale: Scale::Switch,
        unit: "",
        help: "On, this input is left out of the mix, whatever its level.",
    }
}

pub static INFO: ModuleInfo = ModuleInfo {
    name: "mix",
    description: "Four inputs summed, each at its own level and with its own mute: voices into one sound, or a crossfade with CV.",
    inputs: &["1", "2", "3", "4"],
    outputs: &["out"],
    output_details: &["sum of inputs × levels"],
    params: &[
        level("level 1"),
        level("level 2"),
        level("level 3"),
        level("level 4"),
        mute_switch("mute 1"),
        mute_switch("mute 2"),
        mute_switch("mute 3"),
        mute_switch("mute 4"),
    ],
};

/// A mixer. Several cables into one input already add up, but without a
/// level each; and summed voices get loud, which the master's limiter would
/// turn into distortion.
pub struct Mixer;

impl Module for Mixer {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        let out = &mut outputs[OUT];
        out.fill(0.0);
        let (levels, mutes) = params.split_at(CHANNELS);
        for ((input, level), mute) in inputs.iter().zip(levels).zip(mutes) {
            for (((o, x), l), m) in out.iter_mut().zip(input).zip(level).zip(mute) {
                // The switch glides from 0 to 1, so muting fades.
                *o += x * l * (1.0 - m);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::run;

    #[test]
    fn sums_each_input_at_its_level() {
        let out = run(
            &mut Mixer,
            &[1.0, 2.0, 4.0, 8.0],
            &[1.0, 0.5, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0],
            OUT,
            1,
        );
        assert!(out.iter().all(|&s| s == 3.0));
    }

    #[test]
    fn a_muted_input_is_left_out() {
        let out = run(
            &mut Mixer,
            &[1.0, 2.0, 4.0, 8.0],
            &[1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0],
            OUT,
            1,
        );
        assert!(out.iter().all(|&s| s == 5.0));
    }
}
