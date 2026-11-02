use std::f32::consts::TAU;

use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};
use crate::modules::dsp::{ClockIn, glow};

pub const IN_CLOCK: usize = 0;

pub const OUT_SINE: usize = 0;
pub const OUT_TRIANGLE: usize = 1;
pub const OUT_SAW: usize = 2;
pub const OUT_SQUARE: usize = 3;

pub const RATE: usize = 0;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "lfo",
    description: "Slow sine, triangle, saw and square, to move other knobs. Follows `clock` when it has a cable.",
    inputs: &["clock"],
    outputs: &["sine", "triangle", "saw", "square"],
    output_details: &["±1", "±1", "±1", "±1"],
    params: &[ParamInfo {
        name: "rate",
        min: 0.01,
        max: 50.0,
        default: 1.0,
        scale: Scale::Exponential,
        unit: "Hz",
        help: "Cycles per second. Ignored while `clock` has a cable.",
    }],
};

/// Slow bipolar (-1..1) modulation source, meant for CV inputs.
///
/// With a cable in `clock`, it follows the clock instead of `rate`: one cycle
/// per clock period, restarting on each tick, so it stays in time.
pub struct Lfo {
    sample_rate: f32,
    phase: f32,
    clock: ClockIn,
    clocked: bool,
}

impl Lfo {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            phase: 0.0,
            clock: ClockIn::default(),
            clocked: false,
        }
    }
}

impl Module for Lfo {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        let [sine, triangle, saw, square] = outputs else {
            unreachable!("the graph allocates one buffer per output")
        };
        for (i, &knob) in params[RATE].iter().enumerate() {
            let tick = self.clock.tick(inputs[IN_CLOCK][i]);
            let mut rate = knob;
            if self.clocked {
                if tick {
                    self.phase = 0.0;
                }
                if let Some(period) = self.clock.period() {
                    rate = self.sample_rate / period as f32;
                }
            }
            let p = self.phase;
            sine[i] = (p * TAU).sin();
            triangle[i] = 1.0 - 4.0 * (p - 0.5).abs();
            saw[i] = 2.0 * p - 1.0;
            square[i] = if p < 0.5 { 1.0 } else { -1.0 };
            self.phase = (p + rate / self.sample_rate).fract();
        }
    }

    fn set_plugged(&mut self, _input: usize, plugged: bool) {
        self.clocked = plugged;
        // A clock plugged in later has its own tempo to learn.
        self.clock = ClockIn::default();
    }

    fn led(&self) -> Option<f32> {
        Some(glow(self.phase))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::module::BLOCK;
    use crate::modules::test_util::{rising_zero_crossings, run, run_signal};

    const SR: f32 = 48_000.0;

    #[test]
    fn runs_at_the_set_rate() {
        // Ten seconds at 2 Hz.
        let out = run(
            &mut Lfo::new(SR),
            &[0.0],
            &[2.0],
            OUT_SINE,
            10 * 48_000 / BLOCK,
        );
        assert!(rising_zero_crossings(&out).abs_diff(20) <= 1);
    }

    #[test]
    fn all_shapes_stay_within_unit_range() {
        for port in [OUT_SINE, OUT_TRIANGLE, OUT_SAW, OUT_SQUARE] {
            let out = run(&mut Lfo::new(SR), &[0.0], &[37.0], port, 1000);
            assert!(out.iter().all(|s| (-1.0..=1.0).contains(s)));
        }
    }

    /// A gate that rises every `period` samples, for `blocks` blocks.
    fn clock(period: usize, blocks: usize) -> Vec<Buffer> {
        (0..blocks)
            .map(|b| std::array::from_fn(|i| ((b * BLOCK + i) % period < period / 2) as u8 as f32))
            .collect()
    }

    /// Runs a module on a changing input, one buffer per block.
    fn run_clocked(lfo: &mut Lfo, clock: &[Buffer], rate: f32) -> Vec<f32> {
        clock
            .iter()
            .flat_map(|block| run_signal(lfo, &[*block], &[rate], OUT_SAW, 1))
            .collect()
    }

    #[test]
    fn a_plugged_clock_sets_the_rate_and_the_phase() {
        let mut lfo = Lfo::new(SR);
        lfo.set_plugged(IN_CLOCK, true);
        // Ten seconds of a 4 Hz clock, the knob saying 1 Hz.
        let period = SR as usize / 4;
        let out = run_clocked(&mut lfo, &clock(period, 10 * 48_000 / BLOCK), 1.0);
        let resets: Vec<usize> = (1..out.len()).filter(|&n| out[n] < out[n - 1]).collect();
        assert!(resets.len().abs_diff(40) <= 1, "{}", resets.len());
        // Each cycle starts on a clock tick.
        assert!(resets.iter().all(|&n| n % period == 0), "{resets:?}");
    }

    #[test]
    fn an_unplugged_clock_input_leaves_the_knob_in_charge() {
        let mut lfo = Lfo::new(SR);
        lfo.set_plugged(IN_CLOCK, true);
        run_clocked(&mut lfo, &clock(1000, 100), 1.0);
        lfo.set_plugged(IN_CLOCK, false);
        let out = run(&mut lfo, &[0.0], &[2.0], OUT_SINE, 10 * 48_000 / BLOCK);
        assert!(rising_zero_crossings(&out).abs_diff(20) <= 1);
    }

    #[test]
    fn the_light_glows_at_the_rate_it_runs_at() {
        let mut lfo = Lfo::new(SR);
        let mut lights = Vec::new();
        // One second at 3 Hz, the light read once per block.
        for _ in 0..48_000 / BLOCK {
            run(&mut lfo, &[0.0], &[3.0], OUT_SINE, 1);
            lights.push(lfo.led().unwrap());
        }
        let peaks = lights
            .windows(3)
            .filter(|w| w[1] > w[0] && w[1] >= w[2])
            .count();
        assert_eq!(peaks, 3);
    }
}
