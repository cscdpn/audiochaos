//! The only part of the engine that touches the sound card.

use std::error::Error;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};

use super::control::Engine;

/// The default output device and its preferred configuration. The sample rate
/// must be known before building the graph, hence the two-step setup.
pub fn default_output() -> Result<(Device, StreamConfig), Box<dyn Error>> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no audio output device")?;
    let supported = device.default_output_config()?;
    // Every desktop backend (PipeWire, PulseAudio, CoreAudio, WASAPI) offers
    // f32 by default; supporting other formats is not worth the code yet.
    if supported.sample_format() != SampleFormat::F32 {
        return Err(format!("unsupported sample format {}", supported.sample_format()).into());
    }
    Ok((device, supported.config()))
}

/// Starts playing. Sound stops when the returned stream is dropped.
pub fn play(
    device: &Device,
    config: StreamConfig,
    mut engine: Engine,
) -> Result<Stream, Box<dyn Error>> {
    let channels = config.channels as usize;
    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _| {
            // The engine is mono: the same sample goes to every channel.
            for frame in data.chunks_mut(channels) {
                frame.fill(engine.next_sample());
            }
        },
        |err| eprintln!("audio stream error: {err}"),
        None,
    )?;
    stream.play()?;
    Ok(stream)
}
