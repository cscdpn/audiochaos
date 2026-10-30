use std::sync::Arc;

use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::Smoothed;
use crate::midi::{KNOBS, PADS, State};
use crate::modules::oscillator;

pub const OUT_PITCH: usize = 0;
pub const OUT_GATE: usize = 1;
pub const OUT_VELOCITY: usize = 2;
pub const OUT_FIRST_PAD: usize = 3;
pub const OUT_FIRST_KNOB: usize = OUT_FIRST_PAD + PADS;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "midi",
    inputs: &[],
    outputs: &[
        "pitch", "gate", "velocity", "pad 1", "pad 2", "pad 3", "pad 4", "pad 5", "pad 6", "pad 7",
        "pad 8", "knob 1", "knob 2", "knob 3", "knob 4", "knob 5", "knob 6", "knob 7", "knob 8",
    ],
    params: &[],
};

/// The pitch bend range, as on most synths.
const BEND_SEMITONES: f32 = 2.0;

/// A MIDI controller as voltages: the keyboard's pitch, gate and velocity,
/// a gate per pad, and the knobs (0..1).
///
/// `pitch` is tuned for an oscillator's `freq cv`: patched there, the `freq`
/// knob sets the pitch of middle C and every key is a semitone.
pub struct Midi {
    state: Arc<State>,
    /// MIDI values come in steps of 1/127; gliding over them avoids audible
    /// staircases when a knob sweeps a cutoff.
    knobs: [Smoothed; KNOBS],
    bend: Smoothed,
}

impl Midi {
    pub fn new(state: Arc<State>, sample_rate: f32) -> Self {
        Self {
            knobs: std::array::from_fn(|k| Smoothed::new(state.knob(k), sample_rate)),
            bend: Smoothed::new(state.bend(), sample_rate),
            state,
        }
    }
}

/// One octave in the oscillator's `freq` knob space, which spans
/// log2(max / min) octaves.
fn octave() -> f32 {
    let freq = &oscillator::INFO.params[oscillator::FREQ];
    1.0 / (freq.max / freq.min).log2()
}

impl Module for Midi {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, _: &[Buffer], _: &[Buffer], outputs: &mut [Buffer]) {
        let (keyboard, rest) = outputs.split_at_mut(OUT_FIRST_PAD);
        let (pads, knobs) = rest.split_at_mut(PADS);

        let semitones = f32::from(self.state.note()) - 60.0;
        self.bend.set(self.state.bend());
        for s in keyboard[OUT_PITCH].iter_mut() {
            *s = (semitones + self.bend.tick() * BEND_SEMITONES) / 12.0 * octave();
        }
        let gate = if self.state.gate() { 1.0 } else { 0.0 };
        keyboard[OUT_GATE].fill(gate);
        keyboard[OUT_VELOCITY].fill(self.state.velocity());

        for (p, out) in pads.iter_mut().enumerate() {
            out.fill(if self.state.pad(p) { 1.0 } else { 0.0 });
        }
        for (k, (out, smoothed)) in knobs.iter_mut().zip(&mut self.knobs).enumerate() {
            smoothed.set(self.state.knob(k));
            for s in out.iter_mut() {
                *s = smoothed.tick();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::graph::{Cable, Graph};
    use crate::midi::Parser;
    use crate::modules::oscillator::Oscillator;
    use crate::modules::test_util::{rising_zero_crossings, run};

    const SR: f32 = 48_000.0;

    fn played(messages: &[[u8; 3]]) -> Arc<State> {
        let state = Arc::new(State::default());
        let mut parser = Parser::default();
        for m in messages {
            parser.handle(m, &state);
        }
        state
    }

    /// One second of the oscillator, its `freq` at middle C, tracking `pitch`.
    fn oscillator_frequency(state: Arc<State>) -> usize {
        let mut g = Graph::new(SR);
        let midi = g.add("midi", Box::new(Midi::new(state, SR)));
        let osc = g.add("osc", Box::new(Oscillator::new(SR)));
        g.set_param(osc, oscillator::FREQ, 261.63);
        g.connect(Cable {
            from: midi,
            from_port: OUT_PITCH,
            to: osc,
            to_port: oscillator::INFO.cv_port(oscillator::FREQ),
        })
        .unwrap();
        // Past the knob's glide.
        g.render(osc, oscillator::OUT_SINE, 4800);
        rising_zero_crossings(&g.render(osc, oscillator::OUT_SINE, SR as usize))
    }

    #[test]
    fn pitch_plays_the_oscillator_in_tune() {
        assert!(oscillator_frequency(played(&[])).abs_diff(262) <= 2);
        // An octave up, then a fifth up (392 Hz).
        assert!(oscillator_frequency(played(&[[0x90, 72, 100]])).abs_diff(523) <= 2);
        assert!(oscillator_frequency(played(&[[0x90, 67, 100]])).abs_diff(392) <= 2);
        // Full bend up: two semitones (D, 294 Hz).
        assert!(oscillator_frequency(played(&[[0xe0, 0x7f, 0x7f]])).abs_diff(294) <= 2);
    }

    #[test]
    fn gate_velocity_and_pads_follow_the_controller() {
        let state = played(&[[0x90, 64, 127], [0x99, 38, 100]]);
        let mut midi = Midi::new(state, SR);
        assert_eq!(run(&mut midi, &[], &[], OUT_GATE, 1)[0], 1.0);
        assert_eq!(run(&mut midi, &[], &[], OUT_VELOCITY, 1)[0], 1.0);
        assert_eq!(run(&mut midi, &[], &[], OUT_FIRST_PAD + 2, 1)[0], 1.0);
        assert_eq!(run(&mut midi, &[], &[], OUT_FIRST_PAD, 1)[0], 0.0);
    }

    #[test]
    fn knobs_glide_to_their_value() {
        let state = played(&[[0xb0, 3, 127]]);
        let mut midi = Midi::new(Arc::clone(&state), SR);
        let out = run(&mut midi, &[], &[], OUT_FIRST_KNOB + 2, 100);
        assert!((out[out.len() - 1] - 1.0).abs() < 1e-3);
        let mut parser = Parser::default();
        parser.handle(&[0xb0, 3, 0], &state);
        let out = run(&mut midi, &[], &[], OUT_FIRST_KNOB + 2, 1);
        assert!(out[0] > 0.5, "no jump to the new value");
    }
}
