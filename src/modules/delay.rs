use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale, Smoothed};
use crate::modules::dsp::{ClockIn, DelayLine, flash};

pub const IN: usize = 0;
pub const IN_CLOCK: usize = 1;

pub const OUT: usize = 0;

pub const TIME: usize = 0;
pub const FEEDBACK: usize = 1;
pub const MIX: usize = 2;

const MAX_SECONDS: f32 = 2.0;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "delay",
    description: "Echoes, up to 2 s. Feedback above 1 screams instead of exploding.",
    inputs: &["in", "clock"],
    outputs: &["out"],
    output_details: &["input + echoes"],
    params: &[
        ParamInfo {
            name: "time",
            min: 0.001,
            max: MAX_SECONDS,
            default: 0.25,
            scale: Scale::Exponential,
            unit: "s",
            help: "Time between echoes. Ignored while `clock` has a cable.",
        },
        ParamInfo {
            name: "feedback",
            min: 0.0,
            max: 1.5,
            default: 0.5,
            scale: Scale::Linear,
            unit: "",
            help: "How much of each echo comes back: above 1 it self-oscillates.",
        },
        ParamInfo {
            name: "mix",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            scale: Scale::Linear,
            unit: "",
            help: "0 dry, 1 only the echoes.",
        },
    ],
};

/// Echoes, from a line whose modulated time bends the pitch like tape. The
/// feedback path goes through `tanh`: above 1 the echoes grow into a screaming
/// self-oscillation instead of exploding to infinity.
///
/// With a cable in `clock`, the time between two clock ticks replaces `time`
/// and its CV, like a hardware delay's clock input: a `clock` block keeps the
/// echoes in tempo, and a pad tapped twice sets the time by hand.
pub struct Delay {
    sample_rate: f32,
    line: DelayLine,
    clock: ClockIn,
    clocked: bool,
    /// The clock's time, in seconds, gliding so that a change of tempo bends
    /// the echoes instead of clicking. None until the clock has a tempo.
    clock_time: Option<Smoothed>,
    /// Through the current echo, 0..1, for the light.
    echo: f32,
}

impl Delay {
    /// Allocates the delay line, so call it before the audio thread starts.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            line: DelayLine::new((MAX_SECONDS * sample_rate) as usize),
            clock: ClockIn::default(),
            clocked: false,
            clock_time: None,
            echo: 0.0,
        }
    }

    /// The delay time for this sample, in seconds.
    fn time(&mut self, clock: f32, knob: f32) -> f32 {
        let tick = self.clock.tick(clock);
        if !self.clocked {
            return knob;
        }
        if let Some(period) = self.clock.period() {
            let seconds = period as f32 / self.sample_rate;
            match &mut self.clock_time {
                Some(time) => time.set(seconds),
                None => self.clock_time = Some(Smoothed::new(seconds, self.sample_rate)),
            }
        }
        if tick {
            // The light blinks with the clock.
            self.echo = 0.0;
        }
        self.clock_time.as_mut().map_or(knob, Smoothed::tick)
    }
}

impl Module for Delay {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        for (i, x) in inputs[IN].iter().enumerate() {
            let seconds = self.time(inputs[IN_CLOCK][i], params[TIME][i]);
            let seconds = seconds.min(MAX_SECONDS);
            let wet = self.line.read(seconds * self.sample_rate);
            self.line.push(x + (params[FEEDBACK][i] * wet).tanh());
            let mix = params[MIX][i];
            outputs[OUT][i] = (1.0 - mix) * x + mix * wet;
            self.echo = (self.echo + 1.0 / (seconds * self.sample_rate)).fract();
        }
    }

    fn set_plugged(&mut self, input: usize, plugged: bool) {
        if input == IN_CLOCK {
            self.clocked = plugged;
            // A clock plugged in later has its own tempo to learn.
            self.clock = ClockIn::default();
            self.clock_time = None;
        }
    }

    fn led(&self) -> Option<f32> {
        Some(flash(self.echo))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::module::BLOCK;
    use crate::modules::noise::Noise;
    use crate::modules::test_util::{process_signal, run, run_signal};

    const SR: f32 = 48_000.0;

    #[test]
    fn an_impulse_comes_back_after_the_delay_time() {
        let mut impulse = vec![0.0; 4800];
        impulse[0] = 1.0;
        // 100 samples, fully wet, no feedback.
        let out = process_signal(&mut Delay::new(SR), &impulse, &[100.0 / SR, 0.0, 1.0], OUT);
        let peak = out.iter().position(|&s| s > 0.5).unwrap();
        assert_eq!(peak, 100);
        assert!(out[200..].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn feedback_repeats_the_echo() {
        let mut impulse = vec![0.0; 4800];
        impulse[0] = 0.5;
        let out = process_signal(&mut Delay::new(SR), &impulse, &[100.0 / SR, 0.5, 1.0], OUT);
        assert!(out[200] > 0.1);
        assert!(out[300] > 0.0 && out[300] < out[200]);
    }

    #[test]
    fn runaway_feedback_stays_bounded() {
        let noise = run(&mut Noise::new(3), &[], &[], 0, 10 * 48_000 / BLOCK);
        let out = process_signal(&mut Delay::new(SR), &noise, &[0.01, 1.5, 1.0], OUT);
        // Each echo is tanh'd, so a sample can hold at most one input plus one.
        assert!(out.iter().all(|s| s.abs() <= 2.0));
    }

    /// An impulse every `period` samples into `in`, with a gate rising every
    /// `clock` samples into `clock`; returns the output.
    fn clocked(delay: &mut Delay, period: usize, clock: usize, time: f32) -> Vec<f32> {
        let blocks = 4 * 48_000 / BLOCK;
        let mut out = Vec::new();
        for b in 0..blocks {
            let at = |i: usize| b * BLOCK + i;
            let signal: Buffer = std::array::from_fn(|i| (at(i) % period == 0) as u8 as f32);
            let gate: Buffer = std::array::from_fn(|i| (at(i) % clock < clock / 2) as u8 as f32);
            out.extend(run_signal(
                delay,
                &[signal, gate],
                &[time, 0.0, 1.0],
                OUT,
                1,
            ));
        }
        out
    }

    #[test]
    fn a_plugged_clock_sets_the_time_and_the_knob_no_longer_does() {
        let mut delay = Delay::new(SR);
        delay.set_plugged(IN_CLOCK, true);
        // Impulses every second; a clock every 0.3 s; the knob says 0.1 s.
        let out = clocked(&mut delay, 48_000, 14_400, 0.1);
        let echo = out[48_000..].iter().position(|&s| s > 0.5).unwrap();
        assert_eq!(echo, 14_400);
    }

    #[test]
    fn an_unplugged_clock_input_is_ignored() {
        let mut delay = Delay::new(SR);
        let out = clocked(&mut delay, 48_000, 14_400, 0.1);
        let echo = out[48_000..].iter().position(|&s| s > 0.5).unwrap();
        assert_eq!(echo, 4800);
    }

    #[test]
    fn the_light_flashes_once_per_echo() {
        let mut delay = Delay::new(SR);
        let mut flashes = 0_usize;
        let mut last = 0.0;
        // Two seconds at 0.25 s, the light read once per block.
        for _ in 0..2 * 48_000 / BLOCK {
            let silence = [[0.0; BLOCK]; 2];
            run_signal(&mut delay, &silence, &[0.25, 0.0, 1.0], OUT, 1);
            let light = delay.led().unwrap();
            if light > last + 0.5 {
                flashes += 1;
            }
            last = light;
        }
        assert!(flashes.abs_diff(8) <= 1, "{flashes}");
    }
}
