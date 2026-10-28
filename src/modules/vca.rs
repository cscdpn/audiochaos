use crate::engine::module::{Buffer, Module};
use crate::engine::param::Smoothed;

pub const IN_SIGNAL: usize = 0;
/// Added to the gain knob, so an LFO here gives tremolo and an audio-rate
/// signal gives amplitude modulation.
pub const IN_CV: usize = 1;

pub const OUT: usize = 0;

pub const GAIN: usize = 0;

/// Voltage-controlled amplifier: `out = signal * (gain + cv)`.
pub struct Vca {
    gain: Smoothed,
}

impl Vca {
    pub fn new(sample_rate: f32, gain: f32) -> Self {
        Self {
            gain: Smoothed::new(gain, sample_rate),
        }
    }
}

impl Module for Vca {
    fn num_inputs(&self) -> usize {
        2
    }

    fn num_outputs(&self) -> usize {
        1
    }

    fn process(&mut self, inputs: &[Buffer], outputs: &mut [Buffer]) {
        let signal = inputs[IN_SIGNAL].iter().zip(inputs[IN_CV]);
        for (out, (s, cv)) in outputs[OUT].iter_mut().zip(signal) {
            *out = s * (self.gain.tick() + cv);
        }
    }

    fn set_param(&mut self, param: usize, value: f32) {
        if param == GAIN {
            self.gain.set(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::run;

    const SR: f32 = 48_000.0;

    #[test]
    fn gain_scales_the_signal() {
        let mut vca = Vca::new(SR, 0.5);
        assert!(
            run(&mut vca, &[0.8, 0.0], OUT, 10)
                .iter()
                .all(|&s| s == 0.4)
        );
    }

    #[test]
    fn cv_adds_to_the_gain() {
        let mut vca = Vca::new(SR, 0.5);
        assert!(
            run(&mut vca, &[2.0, 0.25], OUT, 10)
                .iter()
                .all(|&s| s == 1.5)
        );
    }

    #[test]
    fn zero_gain_and_no_cv_is_silent() {
        let mut vca = Vca::new(SR, 0.0);
        assert!(
            run(&mut vca, &[1.0, 0.0], OUT, 10)
                .iter()
                .all(|&s| s == 0.0)
        );
    }
}
