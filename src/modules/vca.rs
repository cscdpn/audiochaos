use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};

pub const IN_SIGNAL: usize = 0;
/// Added to the gain, unclamped: an LFO here gives tremolo, an envelope or a
/// gate shapes notes, and an audio-rate signal gives ring modulation.
pub const IN_MOD: usize = 1;

pub const OUT: usize = 0;

pub const GAIN: usize = 0;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "vca",
    inputs: &["in", "mod"],
    outputs: &["out"],
    output_details: &["input × gain"],
    params: &[ParamInfo {
        name: "gain",
        min: 0.0,
        max: 2.0,
        default: 1.0,
        scale: Scale::Linear,
        unit: "",
    }],
};

/// Voltage-controlled amplifier: `out = in * (gain + mod)`.
pub struct Vca;

impl Module for Vca {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        for i in 0..outputs[OUT].len() {
            outputs[OUT][i] = inputs[IN_SIGNAL][i] * (params[GAIN][i] + inputs[IN_MOD][i]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::run;

    #[test]
    fn gain_scales_the_signal() {
        assert!(
            run(&mut Vca, &[0.8, 0.0], &[0.5], OUT, 10)
                .iter()
                .all(|&s| s == 0.4)
        );
    }

    #[test]
    fn mod_adds_to_the_gain() {
        assert!(
            run(&mut Vca, &[2.0, 0.25], &[0.5], OUT, 10)
                .iter()
                .all(|&s| s == 1.5)
        );
    }

    #[test]
    fn zero_gain_and_no_mod_is_silent() {
        assert!(
            run(&mut Vca, &[1.0, 0.0], &[0.0], OUT, 10)
                .iter()
                .all(|&s| s == 0.0)
        );
    }
}
