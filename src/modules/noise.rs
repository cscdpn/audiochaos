use crate::engine::module::{Buffer, Module, ModuleInfo};

pub const OUT: usize = 0;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "noise",
    inputs: &[],
    outputs: &["white"],
    output_details: &["±1"],
    params: &[],
};

/// White noise from a xorshift32 generator: not cryptographic, but tiny,
/// allocation-free and plenty random for the ear.
pub struct Noise {
    state: u32,
}

impl Noise {
    pub fn new(seed: u32) -> Self {
        // xorshift gets stuck at zero.
        Self { state: seed.max(1) }
    }

    fn next_sample(&mut self) -> f32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

impl Module for Noise {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, _: &[Buffer], _: &[Buffer], outputs: &mut [Buffer]) {
        for out in &mut outputs[OUT] {
            *out = self.next_sample();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::{rms, run};

    #[test]
    fn spans_the_unit_range_and_averages_to_zero() {
        let out = run(&mut Noise::new(42), &[], &[], OUT, 1000);
        assert!(out.iter().all(|s| (-1.0..=1.0).contains(s)));
        let mean = out.iter().sum::<f32>() / out.len() as f32;
        assert!(mean.abs() < 0.02);
        // Uniform noise on -1..1 has an RMS of 1/sqrt(3).
        assert!((rms(&out) - 0.577).abs() < 0.02);
    }

    #[test]
    fn zero_seed_does_not_get_stuck() {
        let out = run(&mut Noise::new(0), &[], &[], OUT, 1);
        assert!(out.windows(2).any(|w| w[0] != w[1]));
    }
}
