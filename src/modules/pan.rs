use std::f32::consts::FRAC_PI_4;

use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};

pub const IN: usize = 0;

pub const OUT_LEFT: usize = 0;
pub const OUT_RIGHT: usize = 1;

pub const PAN: usize = 0;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "pan",
    description: "Places a sound between the left and right speakers: its outputs go into the master's `left` and `right`. An LFO on `pan cv` moves it around.",
    inputs: &["in"],
    outputs: &["left", "right"],
    output_details: &["to the master's left", "to the master's right"],
    params: &[ParamInfo {
        name: "pan",
        min: -1.0,
        max: 1.0,
        default: 0.0,
        scale: Scale::Linear,
        unit: "",
        help: "-1 all left, 0 centre, 1 all right.",
    }],
};

/// Constant-power panning: left² + right² stays 1, so a sound keeps its
/// loudness as it moves, and sits 3 dB down on each side in the centre, as
/// in a DAW's mixer.
pub struct Pan;

impl Module for Pan {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        for (i, s) in inputs[IN].iter().enumerate() {
            let angle = (params[PAN][i] + 1.0) * FRAC_PI_4;
            outputs[OUT_LEFT][i] = s * angle.cos();
            outputs[OUT_RIGHT][i] = s * angle.sin();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::run;

    fn sides(pan: f32) -> (f32, f32) {
        let left = run(&mut Pan, &[1.0], &[pan], OUT_LEFT, 1)[0];
        let right = run(&mut Pan, &[1.0], &[pan], OUT_RIGHT, 1)[0];
        (left, right)
    }

    #[test]
    fn the_ends_send_everything_to_one_side() {
        let (left, right) = sides(-1.0);
        assert!((left - 1.0).abs() < 1e-6 && right.abs() < 1e-6);
        let (left, right) = sides(1.0);
        assert!(left.abs() < 1e-6 && (right - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_centre_is_even_and_3_db_down() {
        let (left, right) = sides(0.0);
        assert!((left - right).abs() < 1e-6);
        assert!((left - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    }

    #[test]
    fn the_power_stays_the_same_as_it_moves() {
        for pan in [-0.8, -0.3, 0.2, 0.6] {
            let (left, right) = sides(pan);
            assert!((left * left + right * right - 1.0).abs() < 1e-5, "{pan}");
        }
    }
}
