use crate::engine::module::{Buffer, Module};
use crate::engine::param::Smoothed;

pub const IN: usize = 0;

pub const OUT: usize = 0;

pub const VOLUME: usize = 0;

/// Last module before the speakers. Feedback patches can blow up to huge or
/// non-finite values, so this one guarantees a bounded, finite signal: `tanh`
/// soft-clips to (-1, 1) and anything non-finite is muted.
pub struct Master {
    volume: Smoothed,
}

impl Master {
    pub fn new(sample_rate: f32, volume: f32) -> Self {
        Self {
            volume: Smoothed::new(volume, sample_rate),
        }
    }
}

impl Module for Master {
    fn num_inputs(&self) -> usize {
        1
    }

    fn num_outputs(&self) -> usize {
        1
    }

    fn process(&mut self, inputs: &[Buffer], outputs: &mut [Buffer]) {
        for (out, s) in outputs[OUT].iter_mut().zip(inputs[IN]) {
            let volume = self.volume.tick();
            *out = if s.is_finite() {
                (s * volume).tanh()
            } else {
                0.0
            };
        }
    }

    fn set_param(&mut self, param: usize, value: f32) {
        if param == VOLUME {
            self.volume.set(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::run;

    const SR: f32 = 48_000.0;

    #[test]
    fn quiet_signals_pass_almost_unchanged() {
        let mut master = Master::new(SR, 1.0);
        let out = run(&mut master, &[0.01], OUT, 10);
        assert!(out.iter().all(|s| (s - 0.01).abs() < 1e-5));
    }

    #[test]
    fn loud_signals_are_bounded() {
        let mut master = Master::new(SR, 1.0);
        let out = run(&mut master, &[1e6], OUT, 10);
        assert!(out.iter().all(|&s| s <= 1.0));
    }

    #[test]
    fn non_finite_input_is_muted() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut master = Master::new(SR, 1.0);
            let out = run(&mut master, &[bad], OUT, 10);
            assert!(out.iter().all(|&s| s == 0.0), "{bad} was not muted");
        }
    }
}
