use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};

pub const IN: usize = 0;

pub const OUT: usize = 0;

pub const BITS: usize = 0;
pub const DOWNSAMPLE: usize = 1;

pub static INFO: ModuleInfo = ModuleInfo {
    inputs: &["in"],
    outputs: &["out"],
    params: &[
        ParamInfo {
            name: "bits",
            min: 1.0,
            max: 16.0,
            default: 16.0,
            scale: Scale::Linear,
            unit: "bit",
        },
        ParamInfo {
            name: "downsample",
            min: 1.0,
            max: 64.0,
            default: 1.0,
            scale: Scale::Exponential,
            unit: "x",
        },
    ],
};

/// Bitcrusher: quantizes the amplitude to `bits` and holds each sample for
/// `downsample` samples. Both are continuous, so sweeping them is smooth-ish.
#[derive(Default)]
pub struct Crusher {
    held: f32,
    /// Samples since the last grab.
    counter: f32,
}

impl Crusher {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Module for Crusher {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        for (i, x) in inputs[IN].iter().enumerate() {
            if self.counter <= 0.0 {
                let step = 2.0 / params[BITS][i].exp2();
                self.held = (x / step).round() * step;
                self.counter += params[DOWNSAMPLE][i];
            }
            self.counter -= 1.0;
            outputs[OUT][i] = self.held;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::{process_signal, sine};

    const SR: f32 = 48_000.0;

    fn distinct_values(signal: &[f32]) -> usize {
        // `+ 0.0` turns -0.0 into 0.0, which would otherwise count twice.
        let mut values: Vec<_> = signal.iter().map(|s| (s + 0.0).to_bits()).collect();
        values.sort();
        values.dedup();
        values.len()
    }

    #[test]
    fn few_bits_means_few_levels() {
        let input = sine(100.0, SR, 4800);
        let out = process_signal(&mut Crusher::new(), &input, &[2.0, 1.0], OUT);
        assert!(distinct_values(&out) <= 5);
    }

    #[test]
    fn sixteen_bits_is_nearly_transparent() {
        let input = sine(100.0, SR, 4800);
        let out = process_signal(&mut Crusher::new(), &input, &[16.0, 1.0], OUT);
        assert!(out.iter().zip(&input).all(|(o, i)| (o - i).abs() < 1e-4));
    }

    #[test]
    fn downsampling_holds_each_value() {
        let input = sine(100.0, SR, 4800);
        let out = process_signal(&mut Crusher::new(), &input, &[16.0, 8.0], OUT);
        for chunk in out.chunks_exact(8) {
            assert!(chunk.iter().all(|&s| s == chunk[0]));
        }
    }
}
