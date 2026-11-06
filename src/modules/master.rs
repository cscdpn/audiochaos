use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};

/// Both speakers.
pub const IN: usize = 0;
pub const IN_LEFT: usize = 1;
pub const IN_RIGHT: usize = 2;

pub const OUT_LEFT: usize = 0;
pub const OUT_RIGHT: usize = 1;

pub const VOLUME: usize = 0;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "master",
    description: "The output: volume and a soft limiter, the only thing the speakers hear. `in` plays on both speakers, `left` and `right` on one each, from a `pan`.",
    inputs: &["in", "left", "right"],
    outputs: &["left", "right"],
    output_details: &["to the left speaker", "to the right speaker"],
    params: &[ParamInfo {
        name: "volume",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        scale: Scale::Linear,
        unit: "",
        help: "Output level, before the soft limiter.",
    }],
};

/// Last module before the speakers. Feedback patches can blow up to huge or
/// non-finite values, so this one guarantees a bounded, finite signal: `tanh`
/// soft-clips to (-1, 1) and anything non-finite is muted.
pub struct Master;

fn limit(sample: f32, volume: f32) -> f32 {
    if sample.is_finite() {
        (sample * volume).tanh()
    } else {
        0.0
    }
}

impl Module for Master {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        for (i, s) in inputs[IN].iter().enumerate() {
            let volume = params[VOLUME][i];
            outputs[OUT_LEFT][i] = limit(s + inputs[IN_LEFT][i], volume);
            outputs[OUT_RIGHT][i] = limit(s + inputs[IN_RIGHT][i], volume);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::run;

    #[test]
    fn quiet_signals_pass_almost_unchanged() {
        let out = run(&mut Master, &[0.01, 0.0, 0.0], &[1.0], OUT_LEFT, 10);
        assert!(out.iter().all(|s| (s - 0.01).abs() < 1e-5));
    }

    #[test]
    fn in_plays_on_both_sides_and_left_and_right_on_their_own() {
        let side = |inputs: &[f32], port| run(&mut Master, inputs, &[1.0], port, 1)[0];
        let both = [0.01, 0.0, 0.0];
        assert_eq!(side(&both, OUT_LEFT), side(&both, OUT_RIGHT));
        assert!(side(&[0.0, 0.01, 0.0], OUT_RIGHT) == 0.0);
        assert!(side(&[0.0, 0.0, 0.01], OUT_LEFT) == 0.0);
        assert!((side(&[0.01, 0.0, 0.02], OUT_RIGHT) - 0.03).abs() < 1e-5);
    }

    #[test]
    fn loud_signals_are_bounded() {
        for port in [OUT_LEFT, OUT_RIGHT] {
            let out = run(&mut Master, &[1e6, 1e6, 1e6], &[1.0], port, 10);
            assert!(out.iter().all(|&s| s <= 1.0));
        }
    }

    #[test]
    fn non_finite_input_is_muted() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for port in [OUT_LEFT, OUT_RIGHT] {
                let out = run(&mut Master, &[bad, 0.0, 0.0], &[1.0], port, 10);
                assert!(out.iter().all(|&s| s == 0.0), "{bad} was not muted");
            }
        }
    }
}
