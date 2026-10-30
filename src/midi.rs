//! MIDI input: keyboards, pads and knobs, turned into a shared state that
//! `midi` modules read.
//!
//! MIDI messages arrive on midir's threads, at any time. They update atomics
//! that the audio thread reads once per block, so neither side ever waits for
//! the other.

use std::error::Error;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, Ordering};

use midir::{MidiInput, MidiInputConnection, MidiInputPort};

pub const PADS: usize = 8;
pub const KNOBS: usize = 8;
/// Notes on channel 10 (9 counting from 0) come from pads: the General MIDI
/// drum channel, the usual default of pad controllers.
const PAD_CHANNEL: u8 = 9;
const FIRST_PAD_NOTE: u8 = 36;
/// Knobs send these controller numbers, 1 to 8: the MPK mini's defaults.
const FIRST_KNOB_CC: u8 = 1;
const MIDDLE_C: u8 = 60;
const BEND_CENTRE: u16 = 8192;

/// The latest state of every connected controller.
pub struct State {
    note: AtomicU8,
    gate: AtomicBool,
    velocity: AtomicU8,
    /// 14 bits, centred on 8192.
    bend: AtomicU16,
    pads: [AtomicBool; PADS],
    knobs: [AtomicU8; KNOBS],
}

impl Default for State {
    fn default() -> Self {
        Self {
            note: AtomicU8::new(MIDDLE_C),
            gate: AtomicBool::new(false),
            velocity: AtomicU8::new(0),
            bend: AtomicU16::new(BEND_CENTRE),
            pads: Default::default(),
            knobs: Default::default(),
        }
    }
}

impl State {
    /// The keyboard note playing, or the last one played.
    pub fn note(&self) -> u8 {
        self.note.load(Ordering::Relaxed)
    }

    /// Whether a key is held.
    pub fn gate(&self) -> bool {
        self.gate.load(Ordering::Relaxed)
    }

    /// 0 to 1.
    pub fn velocity(&self) -> f32 {
        f32::from(self.velocity.load(Ordering::Relaxed)) / 127.0
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
}

impl Parser {
    pub fn handle(&mut self, message: &[u8], state: &State) {
        let &[status, data1, data2] = message else {
            return;
        };
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
        if channel == PAD_CHANNEL {
            state.pads[pad(note)].store(true, Ordering::Relaxed);
            return;
        }
        self.held.retain(|&n| n != note);
        self.held.push(note);
        state.note.store(note, Ordering::Relaxed);
        state.velocity.store(velocity, Ordering::Relaxed);
        state.gate.store(true, Ordering::Relaxed);
    }

    fn note_off(&mut self, channel: u8, note: u8, state: &State) {
        if channel == PAD_CHANNEL {
            state.pads[pad(note)].store(false, Ordering::Relaxed);
            return;
        }
        self.held.retain(|&n| n != note);
        match self.held.last() {
            Some(&previous) => state.note.store(previous, Ordering::Relaxed),
            None => state.gate.store(false, Ordering::Relaxed),
        }
    }
}

/// Pads usually start at note 36 (C1); each further bank of eight lands on
/// the same eight pads. Wrapping keeps lower notes in step, as 256 is a
/// multiple of 8.
fn pad(note: u8) -> usize {
    usize::from(note.wrapping_sub(FIRST_PAD_NOTE)) % PADS
}

/// Open MIDI connections. Input stops when this is dropped.
pub struct Inputs {
    _connections: Vec<MidiInputConnection<()>>,
    pub names: Vec<String>,
}

/// Listens to every MIDI input port, so any plugged-in controller plays.
/// Controllers plugged in later are not picked up.
pub fn connect(state: &Arc<State>) -> Result<Inputs, Box<dyn Error>> {
    let mut connections = Vec::new();
    let mut names = Vec::new();
    for (port, name) in ports()? {
        let state = Arc::clone(state);
        let mut parser = Parser::default();
        let input = MidiInput::new("audiochaos")?;
        let connection = input
            .connect(
                &port,
                "audiochaos",
                move |_, message, _| parser.handle(message, &state),
                (),
            )
            .map_err(|e| format!("cannot open {name}: {e}"))?;
        connections.push(connection);
        names.push(name);
    }
    Ok(Inputs {
        _connections: connections,
        names,
    })
}

/// Prints every message from every MIDI input until Enter is pressed, to
/// check what a controller sends.
pub fn monitor() -> Result<(), Box<dyn Error>> {
    let mut connections = Vec::new();
    for (port, name) in ports()? {
        println!("listening to {name}");
        let input = MidiInput::new("audiochaos monitor")?;
        let label = name.clone();
        let connection = input
            .connect(
                &port,
                "audiochaos monitor",
                move |_, message, _| println!("{label}: {}", describe(message)),
                (),
            )
            .map_err(|e| format!("cannot open {name}: {e}"))?;
        connections.push(connection);
    }
    if connections.is_empty() {
        return Err("no MIDI controller found".into());
    }
    println!("play something; press Enter to stop");
    io::stdin().read_line(&mut String::new())?;
    Ok(())
}

/// Every input port but software loopbacks, which only echo other programs.
fn ports() -> Result<Vec<(MidiInputPort, String)>, Box<dyn Error>> {
    let input = MidiInput::new("audiochaos")?;
    let mut ports = Vec::new();
    for port in input.ports() {
        let name = input.port_name(&port)?;
        if !name.contains("Through") {
            ports.push((port, name));
        }
    }
    Ok(ports)
}

/// One message in words, channels counted from 1 as on the hardware.
pub fn describe(message: &[u8]) -> String {
    let &[status, data1, data2] = message else {
        return format!("{message:02x?}");
    };
    let channel = (status & 0x0f) + 1;
    match (status & 0xf0, data2) {
        (0x90, velocity) if velocity > 0 => {
            format!("channel {channel} note {data1} on, velocity {velocity}")
        }
        (0x80, _) | (0x90, _) => format!("channel {channel} note {data1} off"),
        (0xb0, value) => format!("channel {channel} controller {data1} = {value}"),
        (0xe0, msb) => format!(
            "channel {channel} pitch bend {}",
            (u16::from(msb) << 7 | u16::from(data1)) as i32 - i32::from(BEND_CENTRE)
        ),
        _ => format!("{message:02x?}"),
    }
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

    #[test]
    fn describes_messages_for_the_monitor() {
        assert_eq!(
            describe(&[0x90, 60, 100]),
            "channel 1 note 60 on, velocity 100"
        );
        assert_eq!(describe(&[0x89, 36, 0]), "channel 10 note 36 off");
        assert_eq!(describe(&[0xb0, 3, 42]), "channel 1 controller 3 = 42");
        assert_eq!(describe(&[0xe0, 0, 0x40]), "channel 1 pitch bend 0");
        assert_eq!(describe(&[0xf8]), "[f8]");
    }
}
