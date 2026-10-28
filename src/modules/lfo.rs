use std::f32::consts::TAU;

use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};

pub const OUT_SINE: usize = 0;
pub const OUT_TRIANGLE: usize = 1;
pub const OUT_SAW: usize = 2;
pub const OUT_SQUARE: usize = 3;

pub const RATE: usize = 0;

pub static INFO: ModuleInfo = ModuleInfo {
    inputs: &[],
    outputs: &["sine", "triangle", "saw", "square"],
    params: &[ParamInfo {
        name: "rate",
        min: 0.01,
        max: 50.0,
        default: 1.0,
        scale: Scale::Exponential,
        unit: "Hz",
    }],
};

/// Slow bipolar (-1..1) modulation source, meant for CV inputs.
pub struct Lfo {
    sample_rate: f32,
    phase: f32,
}

impl Lfo {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            phase: 0.0,
        }
    }
}

impl Module for Lfo {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, _: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        let [sine, triangle, saw, square] = outputs else {
            unreachable!("the graph allocates one buffer per output")
        };
        for (i, rate) in params[RATE].iter().enumerate() {
            let p = self.phase;
            sine[i] = (p * TAU).sin();
            triangle[i] = 1.0 - 4.0 * (p - 0.5).abs();
            saw[i] = 2.0 * p - 1.0;
            square[i] = if p < 0.5 { 1.0 } else { -1.0 };
            self.phase = (p + rate / self.sample_rate).fract();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::module::BLOCK;
    use crate::modules::test_util::{rising_zero_crossings, run};

    const SR: f32 = 48_000.0;

    #[test]
    fn runs_at_the_set_rate() {
        // Ten seconds at 2 Hz.
        let out = run(
            &mut Lfo::new(SR),
            &[],
            &[2.0],
            OUT_SINE,
            10 * 48_000 / BLOCK,
        );
        assert!(rising_zero_crossings(&out).abs_diff(20) <= 1);
    }

    #[test]
    fn all_shapes_stay_within_unit_range() {
        for port in [OUT_SINE, OUT_TRIANGLE, OUT_SAW, OUT_SQUARE] {
            let out = run(&mut Lfo::new(SR), &[], &[37.0], port, 1000);
            assert!(out.iter().all(|s| (-1.0..=1.0).contains(s)));
        }
    }
}
