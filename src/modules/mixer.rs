use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};

pub const CHANNELS: usize = 4;

pub const OUT: usize = 0;

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

pub static INFO: ModuleInfo = ModuleInfo {
    name: "mix",
    description: "Four inputs summed, each at its own level: voices into one sound, or a crossfade with CV.",
    inputs: &["1", "2", "3", "4"],
    outputs: &["out"],
    output_details: &["sum of inputs × levels"],
    params: &[
        level("level 1"),
        level("level 2"),
        level("level 3"),
        level("level 4"),
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
        for (input, level) in inputs.iter().zip(params) {
            for ((o, x), l) in out.iter_mut().zip(input).zip(level) {
                *o += x * l;
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
            &[1.0, 0.5, 0.25, 0.0],
            OUT,
            1,
        );
        assert!(out.iter().all(|&s| s == 3.0));
    }
}
