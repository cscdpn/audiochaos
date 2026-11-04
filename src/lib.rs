//! audiochaos: a modular synthesizer, as a library shared by the standalone
//! program (`src/main.rs`) and the DAW plugin (`plugin/`).
//!
//! From the bottom up:
//!
//! - [`engine`]: the real-time core. A [`engine::graph::Graph`] of modules
//!   and cables, computed block by block on the audio thread, and the
//!   [`engine::control`] pair that lets the UI edit it without locks.
//! - [`modules`]: the signal-processing bricks, one file each, and the
//!   registry that names them.
//! - [`midi`]: MIDI messages turned into atomics that `midi` modules read.
//! - [`patch`] and [`presets`]: patches as JSON, and building graphs from
//!   them.
//! - Front ends: [`gui`] (the node editor, also drawn by the plugin),
//!   [`tui`], and [`session`], which wires sound cards, engine and MIDI
//!   together for the standalone program.
//!
//! Cargo features follow the same layers: the core always builds; `editor`
//! adds the node editor; `app`, on by default, adds what only the standalone
//! program needs (sound cards, MIDI ports, its window, the terminal UI). See
//! ARCHITECTURE.md for the threads and the rules the audio thread lives by.

pub mod engine;
#[cfg(feature = "editor")]
pub mod gui;
pub mod midi;
pub mod modules;
pub mod patch;
pub mod presets;
#[cfg(feature = "app")]
pub mod session;
#[cfg(feature = "app")]
pub mod tui;
