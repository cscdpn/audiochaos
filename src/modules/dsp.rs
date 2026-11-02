//! Building blocks shared by modules.

use std::f32::consts::{PI, TAU};

/// A delay line with a fractional, interpolated read position, so modulating
/// the delay bends the pitch like tape.
pub struct DelayLine {
    buffer: Vec<f32>,
    write: usize,
}

impl DelayLine {
    /// Allocates the line, so call it before the audio thread starts.
    pub fn new(max_samples: usize) -> Self {
        Self {
            buffer: vec![0.0; max_samples + 2],
            write: 0,
        }
    }

    /// What was pushed `delay` samples ago. Read before pushing the current
    /// sample. Delays outside what the line holds are clamped.
    pub fn read(&self, delay: f32) -> f32 {
        let len = self.buffer.len();
        let delay = delay.clamp(1.0, (len - 2) as f32);
        let position = (self.write as f32 - delay).rem_euclid(len as f32);
        let i = position as usize % len;
        let frac = position.fract();
        let next = (i + 1) % len;
        self.buffer[i] * (1.0 - frac) + self.buffer[next] * frac
    }

    pub fn push(&mut self, sample: f32) {
        self.buffer[self.write] = sample;
        self.write = (self.write + 1) % self.buffer.len();
    }
}

/// A first-order all-pass filter: leaves every frequency's level alone and
/// shifts its phase, by 90° at the frequency its coefficient was made for.
/// Mixed back with the dry signal, the shifted frequencies cancel into notches.
#[derive(Clone, Copy, Default)]
pub struct Allpass {
    state: f32,
}

impl Allpass {
    pub fn process(&mut self, x: f32, coefficient: f32) -> f32 {
        let y = coefficient * x + self.state;
        self.state = x - coefficient * y;
        y
    }
}

/// The coefficient of an [`Allpass`] centered on `freq`. Frequencies are kept
/// below Nyquist, where the filter would stop being stable.
pub fn allpass_coefficient(freq: f32, sample_rate: f32) -> f32 {
    let t = (PI * freq.clamp(1.0, 0.45 * sample_rate) / sample_rate).tan();
    (t - 1.0) / (t + 1.0)
}

/// Follows a clock signal, like the clock input of a hardware delay: its
/// tempo is the time between the last two rising edges, kept when the clock
/// stops.
#[derive(Default)]
pub struct ClockIn {
    last: f32,
    /// Samples since the last rising edge, once there has been one.
    since: Option<u32>,
    period: Option<u32>,
}

impl ClockIn {
    /// Reads one sample of the clock. Returns whether it is a rising edge.
    pub fn tick(&mut self, x: f32) -> bool {
        // Like `s&h`'s trigger: an LFO's square clocks as well as a gate.
        let rising = self.last <= 0.0 && x > 0.0;
        self.last = x;
        self.since = self.since.map(|s| s.saturating_add(1));
        if rising {
            if self.since.is_some() {
                self.period = self.since;
            }
            self.since = Some(0);
        }
        rising
    }

    /// Samples between the last two rising edges, once two have come.
    pub fn period(&self) -> Option<u32> {
        self.period
    }
}

/// A light that flashes at the start of each cycle and fades out, for
/// modules that repeat: still visible when the screen happens to be drawn
/// late in the cycle. `phase` goes 0..1 over a cycle.
pub fn flash(phase: f32) -> f32 {
    (1.0 - phase).powi(3)
}

/// A light that follows a sine LFO at `phase` (0..1), from off to full.
pub fn glow(phase: f32) -> f32 {
    0.5 + 0.5 * (TAU * phase).sin()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    #[test]
    fn a_delay_line_interpolates_between_samples() {
        let mut line = DelayLine::new(10);
        line.push(1.0);
        line.push(3.0);
        assert_eq!(line.read(1.0), 3.0);
        assert_eq!(line.read(2.0), 1.0);
        assert_eq!(line.read(1.5), 2.0);
    }

    #[test]
    fn a_delay_line_clamps_delays_it_cannot_hold() {
        let mut line = DelayLine::new(4);
        for s in [1.0, 2.0, 3.0, 4.0, 5.0] {
            line.push(s);
        }
        assert_eq!(line.read(100.0), line.read(4.0));
        assert_eq!(line.read(0.0), 5.0);
    }

    /// Level and phase of the filter at `freq`, from its response to a sine.
    fn response(coefficient: f32, freq: f32) -> (f32, f32) {
        let mut allpass = Allpass::default();
        let w = std::f32::consts::TAU * freq / SR;
        let (settle, count) = (48_000, 48_000);
        let (mut re, mut im) = (0.0, 0.0);
        for n in 0..settle + count {
            let out = allpass.process((w * n as f32).sin(), coefficient);
            if n >= settle {
                re += out * (w * n as f32).sin();
                im += out * (w * n as f32).cos();
            }
        }
        let scale = 2.0 / count as f32;
        let (re, im) = (re * scale, im * scale);
        ((re * re + im * im).sqrt(), im.atan2(re).to_degrees())
    }

    #[test]
    fn an_allpass_shifts_a_quarter_turn_at_its_frequency_and_keeps_every_level() {
        let coefficient = allpass_coefficient(1000.0, SR);
        let (level, phase) = response(coefficient, 1000.0);
        assert!((level - 1.0).abs() < 0.01, "{level}");
        assert!((phase + 90.0).abs() < 2.0, "{phase}");
        for freq in [100.0, 5000.0] {
            let (level, _) = response(coefficient, freq);
            assert!((level - 1.0).abs() < 0.01, "{level} at {freq} Hz");
        }
    }

    fn clock(edges_every: usize, samples: usize) -> ClockIn {
        let mut clock = ClockIn::default();
        for n in 0..samples {
            clock.tick(if n % edges_every < edges_every / 2 {
                1.0
            } else {
                0.0
            });
        }
        clock
    }

    #[test]
    fn a_clock_needs_two_edges_for_a_tempo() {
        assert_eq!(clock(100, 50).period(), None);
        assert_eq!(clock(100, 101).period(), Some(100));
    }

    #[test]
    fn a_clock_keeps_its_last_tempo_when_it_stops() {
        let mut clock = clock(100, 1000);
        for _ in 0..10_000 {
            clock.tick(0.0);
        }
        assert_eq!(clock.period(), Some(100));
        clock.tick(1.0);
        assert_eq!(clock.period(), Some(100 + 10_000));
    }

    #[test]
    fn a_clock_edge_is_a_rise_above_zero() {
        let mut clock = ClockIn::default();
        let edges: Vec<bool> = [-1.0, 1.0, 1.0, 0.0, 0.5, -1.0, 0.0]
            .iter()
            .map(|&x| clock.tick(x))
            .collect();
        assert_eq!(edges, [false, true, false, false, true, false, false]);
    }

    #[test]
    fn lights_stay_between_off_and_full() {
        for phase in [0.0, 0.25, 0.5, 0.75, 0.999] {
            assert!((0.0..=1.0).contains(&flash(phase)), "{phase}");
            assert!((0.0..=1.0).contains(&glow(phase)), "{phase}");
        }
        assert_eq!(flash(0.0), 1.0);
        assert_eq!(glow(0.25), 1.0);
    }

    #[test]
    fn the_coefficient_stays_stable_at_any_frequency() {
        for freq in [0.0, 1.0, SR / 2.0, SR, 1e9] {
            assert!(allpass_coefficient(freq, SR).abs() < 1.0, "{freq}");
        }
    }
}
