use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};

pub const IN: usize = 0;

pub const OUT: usize = 0;

pub const VOLUME: usize = 0;

pub static INFO: ModuleInfo = ModuleInfo {
    inputs: &["in"],
    outputs: &["out"],
    params: &[ParamInfo {
        name: "volume",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        scale: Scale::Linear,
        unit: "",
    }],
};

/// Last module before the speakers. Feedback patches can blow up to huge or
/// non-finite values, so this one guarantees a bounded, finite signal: `tanh`
/// soft-clips to (-1, 1) and anything non-finite is muted.
pub struct Master;

impl Module for Master {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        for (i, s) in inputs[IN].iter().enumerate() {
            outputs[OUT][i] = if s.is_finite() {
                (s * params[VOLUME][i]).tanh()
            } else {
                0.0
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::run;

    #[test]
    fn quiet_signals_pass_almost_unchanged() {
        let out = run(&mut Master, &[0.01], &[1.0], OUT, 10);
        assert!(out.iter().all(|s| (s - 0.01).abs() < 1e-5));
    }

    #[test]
    fn loud_signals_are_bounded() {
        let out = run(&mut Master, &[1e6], &[1.0], OUT, 10);
        assert!(out.iter().all(|&s| s <= 1.0));
    }

    #[test]
    fn non_finite_input_is_muted() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let out = run(&mut Master, &[bad], &[1.0], OUT, 10);
            assert!(out.iter().all(|&s| s == 0.0), "{bad} was not muted");
        }
    }
}
