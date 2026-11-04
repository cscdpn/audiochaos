//! The only part of the engine that touches the sound card.

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Device, Host, SampleFormat, Stream, StreamConfig};

use super::control::Engine;
use super::input::{self, Callbacks, LiveInput};
use super::record::Tap;

/// A sound card to pick from, as the UIs list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceEntry {
    /// What `output` and `input` take to open it.
    pub id: String,
    pub name: String,
}

/// The output devices worth offering, as they are now: cards come and go.
pub fn outputs() -> Vec<DeviceEntry> {
    cpal::default_host()
        .output_devices()
        .map(listed)
        .unwrap_or_default()
}

/// The input devices worth offering, as they are now.
pub fn inputs() -> Vec<DeviceEntry> {
    cpal::default_host()
        .input_devices()
        .map(listed)
        .unwrap_or_default()
}

fn listed(devices: impl Iterator<Item = Device>) -> Vec<DeviceEntry> {
    devices
        .filter_map(|device| {
            let id = device.id().ok()?.to_string();
            worth_listing(&id).then(|| DeviceEntry {
                name: name(&device),
                id,
            })
        })
        .collect()
}

/// ALSA lists each card a dozen times over: as `hw:` (exclusive), `surround`
/// layouts, `hdmi:` outputs... Its own default device is the one to use.
/// Other systems list each card once.
fn worth_listing(id: &str) -> bool {
    !id.starts_with("alsa:") || is_card_default(id)
}

/// The name of a device, for people.
pub fn name(device: &Device) -> String {
    device
        .description()
        .map_or_else(|_| "unnamed device".to_string(), |d| d.name().to_string())
}

/// An output device and its preferred configuration: the one with this `id`,
/// or else the system's default. The sample rate must be known before
/// building the graph, hence the two-step setup.
///
/// ALSA's default is card 0, and a USB MIDI controller plugged in at boot can
/// take that index although it has no audio output. So when the default does
/// not open, the first sound card that does takes over.
///
/// `buffer` asks for callbacks of that many frames, if the device takes
/// them; the system's default otherwise.
pub fn output(
    id: Option<&str>,
    buffer: Option<u32>,
) -> Result<(Device, StreamConfig), Box<dyn Error>> {
    let host = cpal::default_host();
    let (device, config) = match id {
        Some(id) => {
            let device = by_id(&host, id)?;
            let config = output_config(&device)?;
            (device, config)
        }
        None => {
            let device = host
                .default_output_device()
                .ok_or("no audio output device")?;
            with_fallback(device, host.output_devices()?, output_config)?
        }
    };
    let config = with_buffer(config, buffer, |c: StreamConfig| {
        device
            .build_output_stream(c, |_: &mut [f32], _| {}, |_| {}, None)
            .is_ok()
    });
    Ok((device, config))
}

/// Looked up in the list rather than with cpal's `device_by_id`, which
/// misses ALSA's card defaults: it looks for `default:CARD=X,DEV=0`.
fn by_id(host: &Host, id: &str) -> Result<Device, Box<dyn Error>> {
    host.devices()?
        .find(|d| d.id().is_ok_and(|d| d.to_string() == id))
        .ok_or_else(|| format!("no sound card {id}: unplugged?").into())
}

fn output_config(device: &Device) -> Result<StreamConfig, Box<dyn Error>> {
    let supported = device.default_output_config()?;
    // Every desktop backend (PipeWire, PulseAudio, CoreAudio, WASAPI) offers
    // f32 by default; supporting other formats is not worth the code yet.
    if supported.sample_format() != SampleFormat::F32 {
        return Err(format!("unsupported sample format {}", supported.sample_format()).into());
    }
    Ok(supported.config())
}

/// An input device, the one with this `id` or else the system's default, at
/// the output's sample rate: the engine has a single rate, and resampling the
/// input is not worth its latency and code. The default falls back on the
/// first sound card that opens, like the output, and asks for the same
/// `buffer`.
pub fn input(
    id: Option<&str>,
    sample_rate: u32,
    buffer: Option<u32>,
) -> Result<(Device, StreamConfig), Box<dyn Error>> {
    let host = cpal::default_host();
    let config = |d: &Device| input_config(d, sample_rate);
    let (device, config) = match id {
        Some(id) => {
            let device = by_id(&host, id)?;
            let config = config(&device)?;
            (device, config)
        }
        None => {
            let device = host.default_input_device().ok_or("no audio input device")?;
            with_fallback(device, host.input_devices()?, config)?
        }
    };
    let config = with_buffer(config, buffer, |c: StreamConfig| {
        device
            .build_input_stream(c, |_: &[f32], _| {}, |_| {}, None)
            .is_ok()
    });
    Ok((device, config))
}

fn input_config(device: &Device, sample_rate: u32) -> Result<StreamConfig, Box<dyn Error>> {
    let default = device.default_input_config()?;
    let supported =
        if default.sample_format() == SampleFormat::F32 && default.sample_rate() == sample_rate {
            default
        } else {
            device
                .supported_input_configs()?
                .filter(|c| c.sample_format() == SampleFormat::F32)
                .find_map(|c| c.try_with_sample_rate(sample_rate))
                .ok_or(format!("the audio input cannot run at {sample_rate} Hz"))?
        };
    Ok(supported.config())
}

/// `config` with callbacks of `buffer` frames if the device `accepts` them.
/// Devices do not always tell beforehand: ALSA's mixing devices claim to take
/// any size, then only open with 1024. So a stream is opened to find out.
fn with_buffer(
    config: StreamConfig,
    buffer: Option<u32>,
    accepts: impl Fn(StreamConfig) -> bool,
) -> StreamConfig {
    let Some(frames) = buffer else {
        return config;
    };
    let fixed = StreamConfig {
        buffer_size: BufferSize::Fixed(frames),
        ..config
    };
    if accepts(fixed) { fixed } else { config }
}

/// `device` if `config` works for it, else the first card's own default
/// device it works for.
fn with_fallback(
    device: Device,
    mut devices: impl Iterator<Item = Device>,
    config: impl Fn(&Device) -> Result<StreamConfig, Box<dyn Error>>,
) -> Result<(Device, StreamConfig), Box<dyn Error>> {
    match config(&device) {
        Ok(c) => Ok((device, c)),
        Err(e) => devices
            .find_map(|d| {
                let card = d.id().is_ok_and(|id| is_card_default(&id.to_string()));
                card.then(|| config(&d).ok().map(|c| (d, c))).flatten()
            })
            .ok_or(e),
    }
}

/// A card's own default device, which mixes with other programs, as opposed
/// to `hw:` (exclusive), `hdmi:` (likely no speakers) or `null` (no sound).
fn is_card_default(id: &str) -> bool {
    id.starts_with("alsa:default:CARD=")
}

/// Where the engine's sound comes in from: a device and the block that
/// `audio in` modules read.
pub struct Input {
    pub device: Device,
    pub config: StreamConfig,
    pub live: Arc<LiveInput>,
}

/// Running audio streams. Sound stops when it is dropped.
pub struct Playback {
    _output: Stream,
    _input: Option<Stream>,
    input_problem: Option<String>,
    errors: Arc<AtomicUsize>,
}

impl Playback {
    /// Stream errors so far, mostly buffer underruns, and input glitches.
    /// Counted rather than printed, because printing would corrupt the TUI.
    pub fn errors(&self) -> usize {
        self.errors.load(Ordering::Relaxed)
    }

    /// Why the input asked for did not open, if it did not.
    pub fn input_problem(&self) -> Option<&str> {
        self.input_problem.as_deref()
    }
}

/// Plays the engine's output, also handing every sample to `tap` for live
/// recording, and feeds it `input` when there is one. An input that does not
/// open leaves the engine without, and plays on.
pub fn play(
    device: &Device,
    config: StreamConfig,
    mut engine: Engine,
    mut tap: Tap,
    input: Option<Input>,
) -> Result<Playback, Box<dyn Error>> {
    let channels = config.channels as usize;
    let errors = Arc::new(AtomicUsize::new(0));
    let callbacks = Arc::new(Callbacks::default());
    let (input_stream, input_problem) = match input {
        None => (None, None),
        Some(input) => match capture(input, &mut engine, &callbacks, &errors) {
            Ok(stream) => (Some(stream), None),
            Err(e) => (None, Some(e.to_string())),
        },
    };
    let error_count = Arc::clone(&errors);
    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _| {
            callbacks.output_ran(data.len() / channels);
            // The engine is mono: the same sample goes to every channel.
            for frame in data.chunks_mut(channels) {
                let sample = engine.next_sample();
                tap.push(sample);
                frame.fill(sample);
            }
        },
        move |_| {
            error_count.fetch_add(1, Ordering::Relaxed);
        },
        None,
    )?;
    stream.play()?;
    let playback = Playback {
        _output: stream,
        _input: input_stream,
        input_problem,
        errors,
    };
    Ok(playback)
}

fn capture(
    input: Input,
    engine: &mut Engine,
    callbacks: &Arc<Callbacks>,
    errors: &Arc<AtomicUsize>,
) -> Result<Stream, Box<dyn Error>> {
    let (mut capture, feed) = input::queue(input.live, Arc::clone(callbacks), Arc::clone(errors));
    let channels = input.config.channels as usize;
    let error_count = Arc::clone(errors);
    let stream = input.device.build_input_stream(
        input.config,
        move |data: &[f32], _| capture.push(data, channels),
        move |_| {
            error_count.fetch_add(1, Ordering::Relaxed);
        },
        None,
    )?;
    stream.play()?;
    engine.set_input(feed);
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buffer_size_the_device_refuses_falls_back_on_its_default() {
        let config = StreamConfig {
            channels: 2,
            sample_rate: 48_000,
            buffer_size: BufferSize::Default,
        };
        let only_1024 = |c: StreamConfig| c.buffer_size == BufferSize::Fixed(1024);
        let size = |buffer| with_buffer(config, buffer, only_1024).buffer_size;
        assert_eq!(size(None), BufferSize::Default);
        assert_eq!(size(Some(1024)), BufferSize::Fixed(1024));
        assert_eq!(size(Some(128)), BufferSize::Default);
    }

    #[test]
    fn alsa_lists_each_card_once_other_systems_everything() {
        assert!(worth_listing("alsa:default:CARD=U192k"));
        assert!(!worth_listing("alsa:hw:CARD=1,DEV=0"));
        assert!(!worth_listing("alsa:null"));
        assert!(worth_listing("wasapi:{0.0.0.00000000}.{1234}"));
        assert!(worth_listing("coreaudio:BuiltInSpeakerDevice"));
    }

    #[test]
    fn only_a_card_default_replaces_the_system_default() {
        assert!(is_card_default("alsa:default:CARD=PCH"));
        for id in [
            "alsa:null",
            "alsa:default",
            "alsa:sysdefault:CARD=PCH",
            "alsa:hdmi:CARD=PCH,DEV=0",
            "alsa:plughw:CARD=1,DEV=0",
        ] {
            assert!(!is_card_default(id), "{id}");
        }
    }
}
