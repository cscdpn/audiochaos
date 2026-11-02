use std::sync::Arc;

use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::Smoothed;
use crate::midi::{State, VOICES};
use crate::modules::midi::pitch_cv;

/// Outputs come by voice: pitch, gate and velocity of voice 1, then voice 2…
pub const OUTPUTS_PER_VOICE: usize = 3;

pub const fn out_pitch(voice: usize) -> usize {
    OUTPUTS_PER_VOICE * voice
}

pub const fn out_gate(voice: usize) -> usize {
    OUTPUTS_PER_VOICE * voice + 1
}

pub const fn out_velocity(voice: usize) -> usize {
    OUTPUTS_PER_VOICE * voice + 2
}

pub static INFO: ModuleInfo = ModuleInfo {
    name: "poly",
    description: "A MIDI keyboard, or the computer keyboard, playing up to four notes at once: one pitch, gate and velocity per voice.",
    inputs: &[],
    outputs: &[
        "pitch 1",
        "gate 1",
        "velocity 1",
        "pitch 2",
        "gate 2",
        "velocity 2",
        "pitch 3",
        "gate 3",
        "velocity 3",
        "pitch 4",
        "gate 4",
        "velocity 4",
    ],
    output_details: &[
        "for freq cv",
        "key held, 0 or 1",
        "0..1",
        "for freq cv",
        "key held, 0 or 1",
        "0..1",
        "for freq cv",
        "key held, 0 or 1",
        "0..1",
        "for freq cv",
        "key held, 0 or 1",
        "0..1",
    ],
    params: &[],
};

/// The keyboard as `VOICES` voices, for chords: patch one oscillator,
/// envelope and VCA per voice. Each new note takes a free voice, the one
/// released longest ago, or else the oldest note's. Pitches are tuned like
/// `midi`'s, and the pitch bend moves every voice.
pub struct Poly {
    state: Arc<State>,
    bend: Smoothed,
}

impl Poly {
    pub fn new(state: Arc<State>, sample_rate: f32) -> Self {
        Self {
            bend: Smoothed::new(state.bend(), sample_rate),
            state,
        }
    }
}

impl Module for Poly {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, _: &[Buffer], _: &[Buffer], outputs: &mut [Buffer]) {
        self.bend.set(self.state.bend());
        let notes: [u8; VOICES] = std::array::from_fn(|v| self.state.voice(v).note());
        for i in 0..outputs[0].len() {
            let bend = self.bend.tick();
            for (v, &note) in notes.iter().enumerate() {
                outputs[out_pitch(v)][i] = pitch_cv(note, bend);
            }
        }
        for v in 0..VOICES {
            let voice = self.state.voice(v);
            outputs[out_gate(v)].fill(if voice.gate() { 1.0 } else { 0.0 });
            outputs[out_velocity(v)].fill(voice.velocity());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi::Parser;
    use crate::modules::midi::{self, Midi};
    use crate::modules::test_util::run;

    const SR: f32 = 48_000.0;

    fn played(messages: &[[u8; 3]]) -> Arc<State> {
        let state = Arc::new(State::default());
        let mut parser = Parser::default();
        for m in messages {
            parser.handle(m, &state);
        }
        state
    }

    #[test]
    fn every_voice_has_its_three_outputs() {
        assert_eq!(INFO.outputs.len(), VOICES * OUTPUTS_PER_VOICE);
        for v in 0..VOICES {
            let n = v + 1;
            assert_eq!(INFO.outputs[out_pitch(v)], format!("pitch {n}"));
            assert_eq!(INFO.outputs[out_gate(v)], format!("gate {n}"));
            assert_eq!(INFO.outputs[out_velocity(v)], format!("velocity {n}"));
        }
    }

    #[test]
    fn a_chord_comes_out_one_note_per_voice() {
        let state = played(&[[0x90, 60, 127], [0x90, 64, 127], [0x90, 67, 127]]);
        let mut poly = Poly::new(Arc::clone(&state), SR);
        let out = |poly: &mut Poly, port| run(poly, &[], &[], port, 1)[0];
        // Each voice's pitch is the one `midi` would give for its note.
        for (v, note) in [(0, 60), (1, 64), (2, 67)] {
            let mono = played(&[[0x90, note, 127]]);
            let expected = run(&mut Midi::new(mono, SR), &[], &[], midi::OUT_PITCH, 1)[0];
            assert_eq!(out(&mut poly, out_pitch(v)), expected, "voice {v}");
            assert_eq!(out(&mut poly, out_gate(v)), 1.0);
            assert_eq!(out(&mut poly, out_velocity(v)), 1.0);
        }
        assert_eq!(out(&mut poly, out_gate(3)), 0.0);
        assert!(out(&mut poly, out_pitch(1)) > out(&mut poly, out_pitch(0)));
    }
}
