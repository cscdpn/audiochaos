//! MIDI input: keyboards, pads and knobs, turned into a shared state that
//! `midi` modules read.
//!
//! MIDI messages arrive on midir's threads, or from the DAW hosting the
//! plugin, at any time. They update atomics that the audio thread reads once
//! per block, so neither side ever waits for the other.

#[cfg(feature = "app")]
mod ports;

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, Ordering};

#[cfg(feature = "app")]
pub use ports::{Inputs, connect, describe, monitor};

pub const PADS: usize = 8;
pub const KNOBS: usize = 8;
/// Notes played at once by `poly` modules: enough for most chords, and a
/// patch of one chain per voice stays manageable.
pub const VOICES: usize = 4;
/// Notes on channel 10 (9 counting from 0) come from pads: the General MIDI
/// drum channel, the usual default of pad controllers.
const PAD_CHANNEL: u8 = 9;
/// Notes below this come from pads too, whatever their channel. The MPK mini
/// mk2 sends everything on channel 1, with its keys from note 48 up (one
/// octave down reaches 36) and its pad bank B on notes 32 to 39. Its bank A
/// (44 to 51) sends the very same messages as keys, so it plays as keys.
const LOWEST_KEY: u8 = 40;
const FIRST_PAD_NOTE: u8 = 36;
/// Knobs send these controller numbers, 1 to 8: the MPK mini's defaults.
const FIRST_KNOB_CC: u8 = 1;
const MIDDLE_C: u8 = 60;
const BEND_CENTRE: u16 = 8192;

/// One note: the key, whether it is held, and how hard it was hit.
pub struct Voice {
    note: AtomicU8,
    gate: AtomicBool,
    velocity: AtomicU8,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            note: AtomicU8::new(MIDDLE_C),
            gate: AtomicBool::new(false),
            velocity: AtomicU8::new(0),
        }
    }
}

impl Voice {
    /// The note playing, or the last one played.
    pub fn note(&self) -> u8 {
        self.note.load(Ordering::Relaxed)
    }

    /// Whether its key is held.
    pub fn gate(&self) -> bool {
        self.gate.load(Ordering::Relaxed)
    }

    /// 0 to 1.
    pub fn velocity(&self) -> f32 {
        f32::from(self.velocity.load(Ordering::Relaxed)) / 127.0
    }

    fn play(&self, note: u8, velocity: u8) {
        self.note.store(note, Ordering::Relaxed);
        self.velocity.store(velocity, Ordering::Relaxed);
        self.gate.store(true, Ordering::Relaxed);
    }
}

/// The latest state of every connected controller.
pub struct State {
    /// The keyboard played one note at a time, for `midi` modules.
    mono: Voice,
    /// The keyboard played up to `VOICES` notes at once, for `poly` modules.
    voices: [Voice; VOICES],
    /// 14 bits, centred on 8192.
    bend: AtomicU16,
    pads: [AtomicBool; PADS],
    knobs: [AtomicU8; KNOBS],
}

impl Default for State {
    fn default() -> Self {
        Self {
            mono: Voice::default(),
            voices: Default::default(),
            bend: AtomicU16::new(BEND_CENTRE),
            pads: Default::default(),
            knobs: Default::default(),
        }
    }
}

impl State {
    /// The keyboard note playing, or the last one played.
    pub fn note(&self) -> u8 {
        self.mono.note()
    }

    /// Whether a key is held.
    pub fn gate(&self) -> bool {
        self.mono.gate()
    }

    /// 0 to 1.
    pub fn velocity(&self) -> f32 {
        self.mono.velocity()
    }

    pub fn voice(&self, voice: usize) -> &Voice {
        &self.voices[voice]
    }

    /// -1 to 1.
    pub fn bend(&self) -> f32 {
        let bend = f32::from(self.bend.load(Ordering::Relaxed)) - f32::from(BEND_CENTRE);
        (bend / f32::from(BEND_CENTRE)).max(-1.0)
    }

    pub fn pad(&self, pad: usize) -> bool {
        self.pads[pad].load(Ordering::Relaxed)
    }

    /// 0 to 1.
    pub fn knob(&self, knob: usize) -> f32 {
        f32::from(self.knobs[knob].load(Ordering::Relaxed)) / 127.0
    }
}

/// Decodes one controller's messages into the shared state.
#[derive(Default)]
pub struct Parser {
    /// Keys held down, oldest first: releasing the newest falls back to the
    /// one before, like a monophonic synth.
    held: Vec<u8>,
    /// When each voice last started or ended a note, in messages handled,
    /// to free the voice released longest ago and steal the oldest note.
    voice_ages: [u64; VOICES],
    messages: u64,
}

impl Parser {
    pub fn handle(&mut self, message: &[u8], state: &State) {
        let &[status, data1, data2] = message else {
            return;
        };
        self.messages += 1;
        let channel = status & 0x0f;
        match (status & 0xf0, data2) {
            (0x90, velocity) if velocity > 0 => self.note_on(channel, data1, velocity, state),
            (0x80, _) | (0x90, _) => self.note_off(channel, data1, state),
            (0xb0, value) => {
                let knob = data1.wrapping_sub(FIRST_KNOB_CC) as usize;
                if knob < KNOBS {
                    state.knobs[knob].store(value, Ordering::Relaxed);
                }
            }
            (0xe0, msb) => {
                let bend = u16::from(msb) << 7 | u16::from(data1);
                state.bend.store(bend, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    fn note_on(&mut self, channel: u8, note: u8, velocity: u8, state: &State) {
        if is_pad(channel, note) {
            state.pads[pad(note)].store(true, Ordering::Relaxed);
            return;
        }
        self.held.retain(|&n| n != note);
        self.held.push(note);
        state.mono.play(note, velocity);
        let voice = self.free_voice(note, state);
        state.voices[voice].play(note, velocity);
        self.voice_ages[voice] = self.messages;
    }

    /// The voice for a new note: the one already on it if any, else the one
    /// released longest ago (its tail has faded most), else the oldest note.
    /// Voices held from other controllers count as busy, so two controllers
    /// share the voices, though each only knows the ages of its own.
    fn free_voice(&self, note: u8, state: &State) -> usize {
        let voices = &state.voices;
        let oldest = |busy: bool| {
            (0..VOICES)
                .filter(|&v| voices[v].gate() == busy)
                .min_by_key(|&v| self.voice_ages[v])
        };
        (0..VOICES)
            .find(|&v| voices[v].gate() && voices[v].note() == note)
            .or_else(|| oldest(false))
            .or_else(|| oldest(true))
            .expect("there are voices")
    }

    fn note_off(&mut self, channel: u8, note: u8, state: &State) {
        if is_pad(channel, note) {
            state.pads[pad(note)].store(false, Ordering::Relaxed);
            return;
        }
        self.held.retain(|&n| n != note);
        match self.held.last() {
            Some(&previous) => state.mono.note.store(previous, Ordering::Relaxed),
            None => state.mono.gate.store(false, Ordering::Relaxed),
        }
        for (voice, age) in state.voices.iter().zip(&mut self.voice_ages) {
            if voice.gate() && voice.note() == note {
                voice.gate.store(false, Ordering::Relaxed);
                *age = self.messages;
            }
        }
    }
}

fn is_pad(channel: u8, note: u8) -> bool {
    channel == PAD_CHANNEL || note < LOWEST_KEY
}

/// Pads usually start at note 36 (C1); each further bank of eight lands on
/// the same eight pads. Wrapping keeps lower notes in step, as 256 is a
/// multiple of 8.
fn pad(note: u8) -> usize {
    usize::from(note.wrapping_sub(FIRST_PAD_NOTE)) % PADS
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: u8 = 0x90;
    const OFF: u8 = 0x80;

    fn play(messages: &[[u8; 3]]) -> State {
        let state = State::default();
        let mut parser = Parser::default();
        for m in messages {
            parser.handle(m, &state);
        }
        state
    }

    #[test]
    fn starts_silent_on_middle_c() {
        let state = State::default();
        assert_eq!(state.note(), 60);
        assert!(!state.gate());
        assert_eq!(state.bend(), 0.0);
        assert_eq!(state.knob(0), 0.0);
    }

    #[test]
    fn keys_set_note_velocity_and_gate() {
        let state = play(&[[ON, 64, 127]]);
        assert_eq!(state.note(), 64);
        assert_eq!(state.velocity(), 1.0);
        assert!(state.gate());
        let state = play(&[[ON, 64, 127], [OFF, 64, 0]]);
        assert!(!state.gate());
        assert_eq!(state.note(), 64, "the note stays for release tails");
    }

    #[test]
    fn note_on_with_zero_velocity_is_a_release() {
        assert!(!play(&[[ON, 64, 100], [ON, 64, 0]]).gate());
    }

    #[test]
    fn releasing_the_last_key_falls_back_to_the_one_still_held() {
        let state = play(&[[ON, 60, 100], [ON, 67, 100], [OFF, 67, 0]]);
        assert!(state.gate());
        assert_eq!(state.note(), 60);
        let state = play(&[[ON, 60, 100], [ON, 67, 100], [OFF, 60, 0]]);
        assert_eq!(state.note(), 67);
    }

    /// The note of each voice still held, in voice order.
    fn chord(state: &State) -> Vec<Option<u8>> {
        (0..VOICES)
            .map(|v| state.voice(v).gate().then(|| state.voice(v).note()))
            .collect()
    }

    #[test]
    fn a_chord_takes_one_voice_per_note() {
        let state = play(&[[ON, 60, 100], [ON, 64, 90], [ON, 67, 80]]);
        assert_eq!(chord(&state), [Some(60), Some(64), Some(67), None]);
        assert_eq!(state.voice(1).velocity(), 90.0 / 127.0);
        // The mono outputs still play the last note.
        assert_eq!(state.note(), 67);
    }

    #[test]
    fn releasing_a_note_frees_its_voice_and_keeps_its_pitch() {
        let state = play(&[[ON, 60, 100], [ON, 64, 100], [OFF, 60, 0]]);
        assert_eq!(chord(&state), [None, Some(64), None, None]);
        assert_eq!(state.voice(0).note(), 60, "for its release tail");
    }

    #[test]
    fn a_new_note_takes_the_voice_released_longest_ago() {
        let state = play(&[
            [ON, 60, 100],
            [ON, 62, 100],
            [ON, 64, 100],
            [OFF, 62, 0],
            [OFF, 60, 0],
            [ON, 70, 100],
        ]);
        // Voice 3 never played, so it was released "longest ago".
        assert_eq!(chord(&state), [None, None, Some(64), Some(70)]);
        let state = play(&[
            [ON, 60, 100],
            [ON, 62, 100],
            [ON, 64, 100],
            [ON, 65, 100],
            [OFF, 62, 0],
            [OFF, 60, 0],
            [ON, 70, 100],
        ]);
        assert_eq!(chord(&state), [None, Some(70), Some(64), Some(65)]);
    }

    #[test]
    fn a_fifth_note_steals_the_oldest() {
        let state = play(&[
            [ON, 60, 100],
            [ON, 62, 100],
            [ON, 64, 100],
            [ON, 65, 100],
            [ON, 67, 100],
        ]);
        assert_eq!(chord(&state), [Some(67), Some(62), Some(64), Some(65)]);
    }

    #[test]
    fn controllers_share_the_voices() {
        let state = play(&[[ON, 60, 100]]);
        let mut other = Parser::default();
        other.handle(&[ON, 64, 100], &state);
        assert_eq!(chord(&state), [Some(60), Some(64), None, None]);
    }

    #[test]
    fn pads_play_no_voice() {
        let state = play(&[[ON | PAD_CHANNEL, 36, 100]]);
        assert_eq!(chord(&state), [None; VOICES]);
    }

    #[test]
    fn pads_on_channel_10_have_their_own_gates() {
        let pad_on = ON | PAD_CHANNEL;
        let state = play(&[[pad_on, 36, 100], [pad_on, 43, 100]]);
        assert!(state.pad(0));
        assert!(state.pad(7));
        assert!(!state.pad(1));
        assert!(!state.gate(), "pads do not play the keyboard");
        let state = play(&[[pad_on, 37, 100], [OFF | PAD_CHANNEL, 37, 0]]);
        assert!(!state.pad(1));
        // The second bank of eight lands on the same pads.
        assert!(play(&[[pad_on, 44, 100]]).pad(0));
    }

    /// As recorded from a real MPK mini mk2.
    #[test]
    fn mpk_mini_mk2_bank_b_pads_are_pads_on_channel_1() {
        for (note, pad) in [(36, 0), (39, 3), (32, 4), (35, 7)] {
            let state = play(&[[ON, note, 75]]);
            assert!(state.pad(pad), "note {note}");
            assert!(!state.gate(), "note {note} played the keyboard");
            assert!(!play(&[[ON, note, 75], [OFF, note, 0]]).pad(pad));
        }
    }

    #[test]
    fn mpk_mini_mk2_keys_and_bank_a_play_the_keyboard() {
        for note in [48, 72, 44, 51] {
            let state = play(&[[ON, note, 70]]);
            assert!(state.gate(), "note {note}");
            assert_eq!(state.note(), note);
        }
    }

    #[test]
    fn controllers_1_to_8_are_the_knobs() {
        let state = play(&[
            [0xb0, 1, 127],
            [0xb0, 8, 64],
            [0xb0, 9, 127],
            [0xb0, 0, 127],
        ]);
        assert_eq!(state.knob(0), 1.0);
        assert!((state.knob(7) - 0.504).abs() < 0.01);
        assert_eq!((1..7).map(|k| state.knob(k)).sum::<f32>(), 0.0);
    }

    #[test]
    fn pitch_bend_spans_minus_one_to_one() {
        assert_eq!(play(&[[0xe0, 0, 0]]).bend(), -1.0);
        assert!((play(&[[0xe0, 0x7f, 0x7f]]).bend() - 1.0).abs() < 0.001);
        assert_eq!(play(&[[0xe0, 0, 0x40]]).bend(), 0.0);
    }

    #[test]
    fn other_messages_are_ignored() {
        let state = play(&[[0xa0, 60, 100], [0xd0, 10, 0]]);
        assert!(!state.gate());
        Parser::default().handle(&[0xf8], &state);
    }
}
