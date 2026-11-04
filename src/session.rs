//! Everything that runs while the synth plays: the sound card's streams, the
//! engine they drive, and the controller the UIs talk to. Switching sound
//! cards builds it all again from the patch, since the new card may run at
//! another rate, which every module is built for.

use std::error::Error;
use std::sync::Arc;

use cpal::traits::DeviceTrait;
use cpal::{BufferSize, StreamConfig};

use crate::engine::audio::{self, Playback};
use crate::engine::control::{Controller, Engine};
use crate::engine::input::LiveInput;
use crate::engine::record::{self, Recorder};
use crate::midi;
use crate::modules::Context;
use crate::patch::{self, Patch};

/// Room for a burst of key presses, or a whole patch being loaded, even if
/// the audio thread stalls briefly.
const COMMAND_QUEUE_CAPACITY: usize = 1024;

/// Which sound cards to use, by `audio::DeviceEntry` id; `None` for the
/// system's default.
#[derive(Clone, Debug, Default)]
pub struct Devices {
    pub output: Option<String>,
    pub input: Option<String>,
    /// Callback size to ask the cards for, in frames.
    pub buffer: Option<u32>,
    /// Whether to reach the cards as directly as the system allows: see
    /// `audio`.
    pub low_latency: bool,
}

pub struct Session {
    pub controller: Controller,
    pub context: Context,
    /// Owned so the sound plays exactly as long as the session lives.
    pub playback: Playback,
    pub recorder: Recorder,
    /// The output's name and buffer, for people.
    pub output: String,
    /// What did not work out, though the synth plays: missing sound files,
    /// a buffer size or an input the card refused.
    pub problems: Vec<String>,
}

/// Plays `patch` on `devices`, with MIDI from `midi`, which outlives sessions
/// so controllers stay connected.
pub fn start(
    patch: &Patch,
    devices: &Devices,
    midi: &Arc<midi::State>,
) -> Result<Session, Box<dyn Error>> {
    let card = audio::output(
        devices.output.as_deref(),
        devices.buffer,
        devices.low_latency,
    )?;
    let config = card.config;
    let context = Context {
        sample_rate: config.sample_rate as f32,
        midi: Arc::clone(midi),
        input: Arc::new(LiveInput::default()),
        transport: Arc::default(),
    };
    let (graph, output) = patch.build(&context)?;
    let mut problems = patch::sample_problems(&graph);
    let (engine, controller) = Engine::new(graph, output, COMMAND_QUEUE_CAPACITY);
    let (tap, recorder) = record::tap(config.sample_rate);
    if let Some(frames) = devices.buffer
        && config.buffer_size == BufferSize::Default
    {
        problems.push(format!(
            "the sound card refused a buffer of {frames} frames"
        ));
    }
    // In low latency, the card plays alone, so its own input is the obvious
    // one: a second card would drift from it, and ASIO opens one at a time.
    let input_id = devices.input.clone().or_else(|| {
        devices
            .low_latency
            .then(|| card.device.id().ok().map(|id| id.to_string()))
            .flatten()
    });
    let input = match audio::input(
        input_id.as_deref(),
        config.sample_rate,
        config_buffer(&config).or(devices.buffer),
        devices.low_latency,
    ) {
        Ok(endpoint) => Some(audio::Input {
            endpoint,
            live: Arc::clone(&context.input),
        }),
        Err(e) => {
            problems.push(format!("no audio input: {e}"));
            None
        }
    };
    let playback = audio::play(&card, engine, tap, input)?;
    if let Some(e) = playback.input_problem() {
        problems.push(format!("no audio input: {e}"));
    }
    Ok(Session {
        controller,
        context,
        playback,
        recorder,
        output: describe(&audio::name(&card.device), &config, devices.low_latency),
        problems,
    })
}

fn config_buffer(config: &StreamConfig) -> Option<u32> {
    match config.buffer_size {
        BufferSize::Fixed(frames) => Some(frames),
        BufferSize::Default => None,
    }
}

/// `name`, and how long a callback lasts when it is known: the delay from a
/// key press to the sound is a couple of them.
fn describe(name: &str, config: &StreamConfig, low_latency: bool) -> String {
    let mode = if low_latency { ", low latency" } else { "" };
    match config_buffer(config) {
        Some(frames) => {
            let ms = 1000.0 * frames as f32 / config.sample_rate as f32;
            format!("{name}{mode}, {frames} frames ({ms:.1} ms)")
        }
        None => format!("{name}{mode}"),
    }
}

/// Listens to every MIDI controller plugged in, and says which, or why not.
/// A missing MIDI setup is no reason not to play.
pub fn connect_midi(state: &Arc<midi::State>) -> (Option<midi::Inputs>, String) {
    match midi::connect(state) {
        Ok(inputs) if inputs.names.is_empty() => (Some(inputs), "no MIDI controller".to_string()),
        Ok(inputs) => {
            let status = format!("MIDI: {}", inputs.names.join(", "));
            (Some(inputs), status)
        }
        Err(e) => (None, format!("no MIDI: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_output_says_how_long_a_buffer_lasts_when_known() {
        let config = |buffer_size| StreamConfig {
            channels: 2,
            sample_rate: 48_000,
            buffer_size,
        };
        assert_eq!(
            describe("UMC202HD", &config(BufferSize::Fixed(256)), true),
            "UMC202HD, low latency, 256 frames (5.3 ms)"
        );
        assert_eq!(describe("PCH", &config(BufferSize::Default), false), "PCH");
    }
}
