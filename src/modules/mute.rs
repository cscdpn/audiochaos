use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};

/// Channel indices: the same index names a channel's input and output.
pub const A: usize = 0;
pub const B: usize = 1;

pub const MUTE: usize = 0;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "mute",
    description: "A kill switch on any cable: two channels, for a pan's left and right, silenced together by one switch, or by a gate in `mute cv`.",
    inputs: &["a", "b"],
    outputs: &["a", "b"],
    output_details: &["a, or silence", "b, or silence"],
    params: &[ParamInfo {
        name: "mute",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        scale: Scale::Switch,
        unit: "",
        help: "On, both outputs fall silent.",
    }],
};

/// Passes its inputs through until muted. The switch glides like any knob,
/// so muting fades over a few milliseconds instead of clicking.
pub struct Mute;

impl Module for Mute {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        for (input, output) in inputs.iter().zip(outputs) {
            for ((o, x), m) in output.iter_mut().zip(input).zip(&params[MUTE]) {
                *o = x * (1.0 - m);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::run;

    #[test]
    fn passes_both_channels_until_muted() {
        for (channel, value) in [(A, 0.5), (B, -0.25)] {
            let open = run(&mut Mute, &[0.5, -0.25], &[0.0], channel, 1);
            assert!(open.iter().all(|&s| s == value));
            let muted = run(&mut Mute, &[0.5, -0.25], &[1.0], channel, 1);
            assert!(muted.iter().all(|&s| s == 0.0));
        }
    }
}
