use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};

pub const IN: usize = 0;

pub const OUT: usize = 0;

pub const DRIVE: usize = 0;
pub const FOLD: usize = 1;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "dist",
    inputs: &["in"],
    outputs: &["out"],
    params: &[
        ParamInfo {
            name: "drive",
            min: 1.0,
            max: 100.0,
            default: 4.0,
            scale: Scale::Exponential,
            unit: "x",
        },
        ParamInfo {
            name: "fold",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            scale: Scale::Linear,
            unit: "",
        },
    ],
};

/// Amplifies by `drive`, then blends two shapers: `tanh` clipping (fold at 0),
/// which squares everything off, and a sine wavefolder (fold at 1), which
/// keeps folding the wave back on itself and gets wilder as drive rises.
pub struct Distortion;

impl Module for Distortion {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        for (i, x) in inputs[IN].iter().enumerate() {
            let driven = x * params[DRIVE][i];
            let fold = params[FOLD][i];
            outputs[OUT][i] = (1.0 - fold) * driven.tanh() + fold * driven.sin();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::{process_signal, rising_zero_crossings, sine};

    const SR: f32 = 48_000.0;

    #[test]
    fn output_is_bounded_whatever_the_drive() {
        let input = sine(100.0, SR, 4800);
        for fold in [0.0, 0.5, 1.0] {
            let out = process_signal(&mut Distortion, &input, &[100.0, fold], OUT);
            assert!(out.iter().all(|s| (-1.0..=1.0).contains(s)));
        }
    }

    #[test]
    fn clipping_flattens_the_peaks() {
        let input = sine(100.0, SR, 4800);
        let out = process_signal(&mut Distortion, &input, &[100.0, 0.0], OUT);
        let near_peak = out.iter().filter(|s| s.abs() > 0.99).count();
        assert!(near_peak > out.len() / 2);
    }

    #[test]
    fn folding_adds_zero_crossings() {
        let input = sine(100.0, SR, 48_000);
        let out = process_signal(&mut Distortion, &input, &[20.0, 1.0], OUT);
        assert!(rising_zero_crossings(&out) > 10 * rising_zero_crossings(&input));
    }
}
