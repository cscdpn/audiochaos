use crate::engine::module::{Buffer, Module, ModuleInfo};

pub const IN: usize = 0;
pub const IN_TRIGGER: usize = 1;

pub const OUT: usize = 0;

pub static INFO: ModuleInfo = ModuleInfo {
    inputs: &["in", "trigger"],
    outputs: &["out"],
    params: &[],
};

/// Grabs the input each time the trigger crosses zero upwards, and holds it.
/// Noise in, LFO square on the trigger: classic random stepped voltages.
#[derive(Default)]
pub struct SampleHold {
    held: f32,
    last_trigger: f32,
}

impl SampleHold {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Module for SampleHold {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], _: &[Buffer], outputs: &mut [Buffer]) {
        let signal = inputs[IN].iter().zip(inputs[IN_TRIGGER]);
        for (out, (s, trigger)) in outputs[OUT].iter_mut().zip(signal) {
            if self.last_trigger <= 0.0 && trigger > 0.0 {
                self.held = *s;
            }
            self.last_trigger = trigger;
            *out = self.held;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::module::BLOCK;
    use crate::modules::test_util::run_signal;

    #[test]
    fn holds_the_input_from_the_last_rising_edge() {
        let ramp: Buffer = std::array::from_fn(|i| i as f32);
        let trigger: Buffer = std::array::from_fn(|i| if i == 10 || i == 40 { 1.0 } else { -1.0 });
        let out = run_signal(&mut SampleHold::new(), &[ramp, trigger], &[], OUT, 1);
        assert!(out[..10].iter().all(|&s| s == 0.0));
        assert!(out[10..40].iter().all(|&s| s == 10.0));
        assert!(out[40..BLOCK].iter().all(|&s| s == 40.0));
    }
}
