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
pub const RUN: usize = 1 + STEPS;

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
    description: "Eight-step CV sequencer, on its own clock or stepped by `clock`. `run` starts and stops it: patch another seq's `cv` into `run cv` to sequence sequencers.",
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
        ParamInfo {
            name: "run",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            scale: Scale::Linear,
            unit: "",
            help: "Plays past halfway, stops below. Starting again starts from the first step. With the knob at 0, a gate or another seq's `cv` in `run cv` starts and stops it.",
        },
    ],
};

/// Eight-step CV sequencer with its own clock.
///
/// With a cable in `clock`, each clock tick moves to the next step instead,
/// starting from the first, and `gate` follows the clock: `rate` is ignored.
///
/// While `run` is low it holds its step with the gate closed, and when `run`
/// rises it starts over from the first step.
pub struct Sequencer {
    sample_rate: f32,
    /// Position in steps, 0..STEPS.
    position: f32,
    clock: ClockIn,
    clocked: bool,
    running: bool,
    gate: f32,
}

impl Sequencer {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            position: 0.0,
            clock: ClockIn::default(),
            clocked: false,
            // So that a sequencer running from the start does not start over
            // on its first sample, which would skip its first clock tick.
            running: true,
            gate: 0.0,
        }
    }

    /// Back to the first step. A clocked sequencer plays it at once if the
    /// clock is high, since the tick that went with it may have come a block
    /// earlier, when another module started it a block late; else it plays
    /// it on the next tick.
    fn start_over(&mut self, clock: f32) {
        self.position = if self.clocked && clock <= 0.0 {
            (STEPS - 1) as f32
        } else {
            0.0
        };
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
            let ticked = self.clock.tick(clock);
            let running = params[RUN][i] > 0.5;
            if running && !self.running {
                self.start_over(clock);
            } else if running && ticked && self.clocked {
                self.position = (self.position.floor() + 1.0) % STEPS as f32;
            }
            self.running = running;
            let current = self.position as usize;
            cv[i] = params[step(current)][i];
            let high = if self.clocked {
                clock > 0.0
            } else {
                self.position.fract() < 0.5
            };
            gate[i] = if high && running { 1.0 } else { 0.0 };
            if running && !self.clocked {
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
        p.push(1.0);
        p
    }

    fn stopped() -> Vec<f32> {
        let mut p = params();
        p[RUN] = 0.0;
        p
    }

    /// The step played at the start of each of `blocks` blocks.
    fn steps(seq: &mut Sequencer, clock: f32, params: &[f32], blocks: usize) -> Vec<usize> {
        (0..blocks)
            .map(|_| {
                let cv = run_signal(seq, &[[clock; BLOCK]], params, OUT_CV, 1);
                (cv[1] * 10.0).round() as usize
            })
            .collect()
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
    fn stopped_it_holds_its_step_with_the_gate_closed() {
        let mut seq = Sequencer::new(SR);
        assert_eq!(steps(&mut seq, 0.0, &params(), 3), [0, 1, 2]);
        assert_eq!(steps(&mut seq, 0.0, &stopped(), 3), [3, 3, 3]);
        let gate = run(&mut seq, &[0.0], &stopped(), OUT_GATE, 2);
        assert!(gate.iter().all(|&g| g == 0.0));
        assert_eq!(seq.led(), Some(0.0));
    }

    #[test]
    fn started_again_it_starts_from_the_first_step() {
        let mut seq = Sequencer::new(SR);
        steps(&mut seq, 0.0, &params(), 3);
        steps(&mut seq, 0.0, &stopped(), 1);
        assert_eq!(steps(&mut seq, 0.0, &params(), 3), [0, 1, 2]);
    }

    #[test]
    fn a_seq_that_starts_running_starts_on_its_first_step() {
        let mut seq = Sequencer::new(SR);
        seq.set_plugged(IN_CLOCK, true);
        // Waiting for the first tick, then stepping on each.
        let ticks = [0.0, 1.0, 0.0, 1.0];
        let played: Vec<usize> = ticks
            .iter()
            .map(|&clock| steps(&mut seq, clock, &params(), 1)[0])
            .collect();
        assert_eq!(played, [STEPS - 1, 0, 0, 1]);
    }

    #[test]
    fn a_clocked_seq_started_on_a_tick_plays_its_first_step_on_that_tick() {
        let mut seq = Sequencer::new(SR);
        seq.set_plugged(IN_CLOCK, true);
        for clock in [1.0, 0.0, 1.0, 0.0] {
            steps(&mut seq, clock, &params(), 1);
        }
        steps(&mut seq, 0.0, &stopped(), 1);
        // Started with the tick, or a block after it while the clock is high.
        let mut late = Sequencer::new(SR);
        late.set_plugged(IN_CLOCK, true);
        steps(&mut late, 0.0, &stopped(), 1);
        steps(&mut late, 1.0, &stopped(), 1);
        for seq in [&mut seq, &mut late] {
            let played: Vec<usize> = [1.0, 1.0, 0.0, 1.0, 0.0]
                .iter()
                .map(|&clock| steps(seq, clock, &params(), 1)[0])
                .collect();
            assert_eq!(played, [0, 0, 0, 1, 1]);
        }
    }

    #[test]
    fn a_clocked_seq_started_between_ticks_waits_for_the_next() {
        let mut seq = Sequencer::new(SR);
        seq.set_plugged(IN_CLOCK, true);
        for clock in [1.0, 0.0, 1.0, 0.0] {
            steps(&mut seq, clock, &params(), 1);
        }
        steps(&mut seq, 0.0, &stopped(), 1);
        let gate = run_signal(&mut seq, &[[0.0; BLOCK]], &params(), OUT_GATE, 1);
        assert!(gate.iter().all(|&g| g == 0.0));
        assert_eq!(steps(&mut seq, 1.0, &params(), 1), [0]);
    }

    #[test]
    fn one_seq_starts_and_stops_another() {
        use crate::engine::graph::{Cable, Graph};

        // The rate knob tops out at 50 steps per second: one step per block
        // at this rate.
        let sr = 50.0 * BLOCK as f32;
        let mut g = Graph::new(sr);
        let song = g.add("song", Box::new(Sequencer::new(sr)));
        let riff = g.add("riff", Box::new(Sequencer::new(sr)));
        // The song plays one step every four blocks: on, off, on, off...
        g.init_param(song, RATE, 50.0 / 4.0);
        for n in 0..STEPS {
            g.init_param(song, step(n), if n % 2 == 0 { 1.0 } else { 0.0 });
        }
        g.init_param(riff, RATE, 50.0);
        for n in 0..STEPS {
            g.init_param(riff, step(n), n as f32 / 10.0);
        }
        g.init_param(riff, RUN, 0.0);
        g.connect(Cable {
            from: song,
            from_port: OUT_CV,
            to: riff,
            to_port: INFO.cv_port(RUN),
        })
        .unwrap();
        let played: Vec<usize> = (0..12)
            .map(|_| {
                g.process();
                (g.output(riff, OUT_CV)[1] * 10.0).round() as usize
            })
            .collect();
        assert_eq!(played, [0, 1, 2, 3, 4, 4, 4, 4, 0, 1, 2, 3]);
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
