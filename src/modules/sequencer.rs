use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};

/// Step value, -1..1: patch it into a CV input, typically `freq cv`.
pub const OUT_CV: usize = 0;
/// 1 during the first half of each step, 0 otherwise.
pub const OUT_GATE: usize = 1;

pub const RATE: usize = 0;
pub const STEPS: usize = 8;

/// Parameter index of a step's value.
pub const fn step(n: usize) -> usize {
    1 + n
}

const fn step_param(name: &'static str) -> ParamInfo {
    ParamInfo {
        name,
        min: -1.0,
        max: 1.0,
        default: 0.0,
        scale: Scale::Linear,
        unit: "",
    }
}

pub static INFO: ModuleInfo = ModuleInfo {
    name: "seq",
    inputs: &[],
    outputs: &["cv", "gate"],
    params: &[
        ParamInfo {
            name: "rate",
            min: 0.1,
            max: 50.0,
            default: 4.0,
            scale: Scale::Exponential,
            unit: "st/s",
        },
        step_param("step 1"),
        step_param("step 2"),
        step_param("step 3"),
        step_param("step 4"),
        step_param("step 5"),
        step_param("step 6"),
        step_param("step 7"),
        step_param("step 8"),
    ],
};

/// Eight-step CV sequencer with its own clock.
pub struct Sequencer {
    sample_rate: f32,
    /// Position in steps, 0..STEPS.
    position: f32,
}

impl Sequencer {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            position: 0.0,
        }
    }
}

impl Module for Sequencer {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, _: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        let [cv, gate] = outputs else {
            unreachable!("the graph allocates one buffer per output")
        };
        for i in 0..cv.len() {
            let current = self.position as usize;
            cv[i] = params[step(current)][i];
            gate[i] = if self.position.fract() < 0.5 {
                1.0
            } else {
                0.0
            };
            self.position = (self.position + params[RATE][i] / self.sample_rate) % STEPS as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::module::BLOCK;
    use crate::modules::test_util::run;

    /// One step per block, so the output is easy to check.
    const SR: f32 = 48_000.0;
    const RATE_ONE_STEP_PER_BLOCK: f32 = SR / BLOCK as f32;

    fn params() -> Vec<f32> {
        let mut p = vec![RATE_ONE_STEP_PER_BLOCK];
        p.extend((0..STEPS).map(|n| n as f32 / 10.0));
        p
    }

    #[test]
    fn plays_each_step_in_order_then_loops() {
        let out = run(&mut Sequencer::new(SR), &[], &params(), OUT_CV, 2 * STEPS);
        for block in 0..2 * STEPS {
            let expected = (block % STEPS) as f32 / 10.0;
            assert!(
                (out[block * BLOCK + 1] - expected).abs() < 1e-6,
                "block {block}"
            );
        }
    }

    #[test]
    fn gate_is_high_for_the_first_half_of_each_step() {
        let out = run(&mut Sequencer::new(SR), &[], &params(), OUT_GATE, 3);
        for block in 0..3 {
            let start = block * BLOCK;
            assert_eq!(out[start + 1], 1.0);
            assert_eq!(out[start + BLOCK - 2], 0.0);
        }
    }
}
