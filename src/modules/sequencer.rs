use crate::engine::module::{BLOCK, Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};
use crate::modules::dsp::ClockIn;

pub const IN_CLOCK: usize = 0;

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
        help: "This step's CV, ±1.",
    }
}

pub static INFO: ModuleInfo = ModuleInfo {
    name: "seq",
    description: "Eight-step CV sequencer, on its own clock or stepped by `clock`.",
    inputs: &["clock"],
    outputs: &["cv", "gate"],
    output_details: &["step, ±1", "0 or 1"],
    params: &[
        ParamInfo {
            name: "rate",
            min: 0.1,
            max: 50.0,
            default: 4.0,
            scale: Scale::Exponential,
            unit: "st/s",
            help: "Steps per second. Ignored while `clock` has a cable.",
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
///
/// With a cable in `clock`, each clock tick moves to the next step instead,
/// starting from the first, and `gate` follows the clock: `rate` is ignored.
pub struct Sequencer {
    sample_rate: f32,
    /// Position in steps, 0..STEPS.
    position: f32,
    clock: ClockIn,
    clocked: bool,
    gate: f32,
}

impl Sequencer {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            position: 0.0,
            clock: ClockIn::default(),
            clocked: false,
            gate: 0.0,
        }
    }
}

impl Module for Sequencer {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        let [cv, gate] = outputs else {
            unreachable!("the graph allocates one buffer per output")
        };
        for i in 0..cv.len() {
            let clock = inputs[IN_CLOCK][i];
            if self.clock.tick(clock) && self.clocked {
                self.position = (self.position.floor() + 1.0) % STEPS as f32;
            }
            let current = self.position as usize;
            cv[i] = params[step(current)][i];
            let high = if self.clocked {
                clock > 0.0
            } else {
                self.position.fract() < 0.5
            };
            gate[i] = if high { 1.0 } else { 0.0 };
            if !self.clocked {
                self.position = (self.position + params[RATE][i] / self.sample_rate) % STEPS as f32;
            }
        }
        self.gate = gate[BLOCK - 1];
    }

    fn set_plugged(&mut self, _input: usize, plugged: bool) {
        self.clocked = plugged;
        if plugged {
            // Just before the first step, so the first tick plays it.
            self.position = (STEPS - 1) as f32;
        }
    }

    fn led(&self) -> Option<f32> {
        Some(self.gate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::{run, run_signal};

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
        let out = run(
            &mut Sequencer::new(SR),
            &[0.0],
            &params(),
            OUT_CV,
            2 * STEPS,
        );
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
        let out = run(&mut Sequencer::new(SR), &[0.0], &params(), OUT_GATE, 3);
        for block in 0..3 {
            let start = block * BLOCK;
            assert_eq!(out[start + 1], 1.0);
            assert_eq!(out[start + BLOCK - 2], 0.0);
        }
    }

    #[test]
    fn a_plugged_clock_steps_from_the_first_step_on_each_tick() {
        let mut seq = Sequencer::new(SR);
        seq.set_plugged(IN_CLOCK, true);
        // A tick every other round of two blocks; the rate knob is ignored.
        let mut steps = Vec::new();
        for b in 0..2 * STEPS + 2 {
            let clock = [if b % 2 == 0 { 1.0 } else { 0.0 }; BLOCK];
            let cv = run_signal(&mut seq, &[clock], &params(), OUT_CV, 1);
            let gate = run_signal(&mut seq, &[clock], &params(), OUT_GATE, 1);
            assert_eq!(gate[0], clock[0], "the gate follows the clock");
            steps.push((cv[0] * 10.0).round() as usize);
        }
        let expected: Vec<usize> = (0..steps.len()).map(|b| (b / 2) % STEPS).collect();
        assert_eq!(steps, expected);
    }

    #[test]
    fn the_light_is_the_gate() {
        let mut seq = Sequencer::new(SR);
        run(&mut seq, &[0.0], &params(), OUT_GATE, 1);
        assert_eq!(seq.led(), Some(0.0));
        seq.set_plugged(IN_CLOCK, true);
        run(&mut seq, &[1.0], &params(), OUT_GATE, 1);
        assert_eq!(seq.led(), Some(1.0));
    }
}
