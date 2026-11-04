//! The computer keyboard as a one-octave piano, to play `midi` blocks without
//! a MIDI controller.

use std::sync::Arc;

use egui::{Event, Key};

use crate::midi::{Parser, State};

/// The two bottom letter rows, from C to B: white keys on the lower row, black
/// keys on the row above, as on trackers. Read by position, so the piano lies
/// under the same fingers on QWERTY and AZERTY (W X C V B N , there).
const KEYS: [Key; 12] = [
    Key::Z,
    Key::S,
    Key::X,
    Key::D,
    Key::C,
    Key::V,
    Key::G,
    Key::B,
    Key::H,
    Key::N,
    Key::J,
    Key::M,
];
const MIDDLE_C: u8 = 60;
/// Computer keys cannot tell how hard they are hit.
const VELOCITY: u8 = 100;
const NOTE_ON: u8 = 0x90;
const NOTE_OFF: u8 = 0x80;

/// Plays into the same state as the MIDI controllers, as one more controller.
pub struct Keyboard {
    state: Arc<State>,
    parser: Parser,
    /// Which of `KEYS` this keyboard pressed, so it only releases its own notes
    /// and never cuts a note held on a MIDI controller.
    held: [bool; KEYS.len()],
}

impl Keyboard {
    pub fn new(state: Arc<State>) -> Self {
        Self {
            state,
            parser: Parser::default(),
            held: [false; KEYS.len()],
        }
    }

    /// Plays what `event` asks for. `playing` is false while typing in a text
    /// field: keys still release their notes, but start none.
    pub fn handle(&mut self, event: &Event, playing: bool) {
        match *event {
            Event::Key {
                key,
                physical_key,
                pressed,
                repeat,
                modifiers,
            } => {
                let Some(k) = KEYS.iter().position(|&k| k == physical_key.unwrap_or(key)) else {
                    return;
                };
                // Shortcuts such as Ctrl+Z are not notes.
                let shortcut = modifiers.ctrl || modifiers.alt || modifiers.command;
                if pressed && !repeat && playing && !shortcut && !self.held[k] {
                    self.send(k, NOTE_ON, true);
                } else if !pressed && self.held[k] {
                    self.send(k, NOTE_OFF, false);
                }
            }
            // Releases that happen in another window never reach this one.
            Event::WindowFocused(false) => {
                for k in 0..KEYS.len() {
                    if self.held[k] {
                        self.send(k, NOTE_OFF, false);
                    }
                }
            }
            _ => {}
        }
    }

    fn send(&mut self, key: usize, status: u8, held: bool) {
        let note = MIDDLE_C + key as u8;
        self.parser.handle(&[status, note, VELOCITY], &self.state);
        self.held[key] = held;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Modifiers;

    fn key(physical_key: Key, pressed: bool) -> Event {
        Event::Key {
            // What an AZERTY layout reports for some of these positions.
            key: Key::Q,
            physical_key: Some(physical_key),
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    fn keyboard() -> (Keyboard, Arc<State>) {
        let state = Arc::new(State::default());
        (Keyboard::new(Arc::clone(&state)), state)
    }

    #[test]
    fn the_bottom_rows_play_an_octave_from_middle_c() {
        let (mut keyboard, state) = keyboard();
        for (k, note) in [(Key::Z, 60), (Key::S, 61), (Key::B, 67), (Key::M, 71)] {
            keyboard.handle(&key(k, true), true);
            assert!(state.gate());
            assert_eq!(state.note(), note, "{k:?}");
            keyboard.handle(&key(k, false), true);
            assert!(!state.gate(), "{k:?}");
        }
    }

    #[test]
    fn keys_are_read_by_position_not_by_letter() {
        let (mut keyboard, state) = keyboard();
        let mut event = key(Key::Z, true);
        if let Event::Key { key, .. } = &mut event {
            // AZERTY's W, in QWERTY's Z place.
            *key = Key::W;
        }
        keyboard.handle(&event, true);
        assert_eq!(state.note(), 60);
        assert!(state.gate());
    }

    #[test]
    fn other_keys_and_shortcuts_play_nothing() {
        let (mut keyboard, state) = keyboard();
        keyboard.handle(&key(Key::A, true), true);
        assert!(!state.gate());
        let mut undo = key(Key::Z, true);
        if let Event::Key { modifiers, .. } = &mut undo {
            *modifiers = Modifiers::COMMAND;
        }
        keyboard.handle(&undo, true);
        assert!(!state.gate());
    }

    #[test]
    fn typing_in_a_field_starts_no_note_but_releases_held_ones() {
        let (mut keyboard, state) = keyboard();
        keyboard.handle(&key(Key::Z, true), false);
        assert!(!state.gate());
        keyboard.handle(&key(Key::Z, true), true);
        keyboard.handle(&key(Key::Z, false), false);
        assert!(!state.gate());
    }

    #[test]
    fn releasing_a_key_never_pressed_leaves_other_controllers_playing() {
        let (mut keyboard, state) = keyboard();
        Parser::default().handle(&[NOTE_ON, 48, 100], &state);
        keyboard.handle(&key(Key::Z, false), true);
        assert!(state.gate());
    }

    #[test]
    fn losing_focus_releases_every_key() {
        let (mut keyboard, state) = keyboard();
        keyboard.handle(&key(Key::Z, true), true);
        keyboard.handle(&key(Key::B, true), true);
        keyboard.handle(&Event::WindowFocused(false), true);
        assert!(!state.gate());
    }

    #[test]
    fn the_newest_key_wins_and_releasing_it_falls_back() {
        let (mut keyboard, state) = keyboard();
        keyboard.handle(&key(Key::Z, true), true);
        keyboard.handle(&key(Key::B, true), true);
        assert_eq!(state.note(), 67);
        keyboard.handle(&key(Key::B, false), true);
        assert_eq!(state.note(), 60);
        assert!(state.gate());
    }
}
