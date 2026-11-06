//! The only part of the engine that touches the sound card.
//!
//! Cards are reached one of two ways. Shared, the default: through the
//! system's mixer, alongside other programs, with the buffer sizes it allows;
//! ALSA's mixing devices only take 1024 frames, about 45 ms from input to
//! output. Or low latency: as directly as the system allows. On Linux, that
//! is the card's hardware device, which no other program can use meanwhile,
//! in the card's own sample format, with a real-time audio thread. On
//! Windows, an ASIO driver, in builds with the `asio` feature.

use std::error::Error;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, Device, FromSample, Host, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
    SupportedStreamConfig,
};

use super::control::{Engine, Frame};
use super::input::{self, Callbacks, Capture, LiveInput};
use super::record::Tap;

/// Why low latency is not available in this build, if it is not.
pub const LOW_LATENCY_UNAVAILABLE: Option<&str> =
    if cfg!(any(target_os = "linux", all(windows, feature = "asio"))) {
        None
    } else if cfg!(windows) {
        Some("needs a build with the asio feature, see the README")
    } else {
        Some("not available on this system")
    };

/// The callback size in low latency on Linux when none is asked for: a
/// hardware device left to choose takes a buffer far larger than the
/// mixer's. 256 frames, 5.3 ms at 48 kHz, played without a glitch on a
/// laptop's built-in card where 128 did not; USB interfaces often take less.
/// ASIO drivers have a size of their own, set in their control panel.
const LOW_LATENCY_BUFFER: u32 = 256;

/// The sample formats the engine converts its samples to: floats, which
/// desktop mixers offer, and the integers that cards' own devices take.
const FORMATS: [SampleFormat; 3] = [SampleFormat::F32, SampleFormat::I32, SampleFormat::I16];

/// A sound card to pick from, as the UIs list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceEntry {
    /// What `output` and `input` take to open it.
    pub id: String,
    pub name: String,
}

/// A device and how to open it.
pub struct Endpoint {
    pub device: Device,
    pub config: StreamConfig,
    pub format: SampleFormat,
}

/// The output devices worth offering, as they are now: cards come and go.
pub fn outputs(low_latency: bool) -> Vec<DeviceEntry> {
    host(low_latency)
        .and_then(|host| Ok(host.output_devices()?))
        .map(listed)
        .unwrap_or_default()
}

/// The input devices worth offering, as they are now.
pub fn inputs(low_latency: bool) -> Vec<DeviceEntry> {
    host(low_latency)
        .and_then(|host| Ok(host.input_devices()?))
        .map(listed)
        .unwrap_or_default()
}

/// ASIO drivers, in low latency on Windows; else the system's usual way.
fn host(low_latency: bool) -> Result<Host, Box<dyn Error>> {
    #[cfg(all(windows, feature = "asio"))]
    if low_latency {
        return Ok(cpal::host_from_id(cpal::HostId::Asio)?);
    }
    #[cfg(not(all(windows, feature = "asio")))]
    let _ = low_latency;
    Ok(cpal::default_host())
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
/// layouts, `hdmi:` outputs... Its own default device is the one to list,
/// whatever the mode: `by_id` swaps in the hardware device in low latency.
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
/// `buffer` asks for callbacks of that many frames, if the device takes
/// them; the system's default otherwise.
pub fn output(
    id: Option<&str>,
    buffer: Option<u32>,
    low_latency: bool,
) -> Result<Endpoint, Box<dyn Error>> {
    let host = host(low_latency)?;
    let (device, supported) = match id {
        Some(id) => {
            let device = by_id(&host, id, low_latency)?;
            let config = output_config(&device)?;
            (device, config)
        }
        None => first_that_opens(
            defaults(
                host.default_output_device(),
                host.output_devices()?,
                low_latency,
            ),
            output_config,
        )?,
    };
    let format = supported.sample_format();
    let buffer =
        buffer.or((low_latency && cfg!(target_os = "linux")).then_some(LOW_LATENCY_BUFFER));
    let config = with_buffer(supported.config(), buffer, |c| {
        device
            .build_output_stream_raw(c, format, |_, _| {}, |_| {}, None)
            .is_ok()
    });
    Ok(Endpoint {
        device,
        config,
        format,
    })
}

/// Looked up in the list rather than with cpal's `device_by_id`, which
/// misses ALSA's card defaults: it looks for `default:CARD=X,DEV=0`. In low
/// latency, a card's default device stands for its hardware device.
fn by_id(host: &Host, id: &str, low_latency: bool) -> Result<Device, Box<dyn Error>> {
    let wanted = low_latency
        .then(|| hardware_id(id, card_index))
        .flatten()
        .unwrap_or_else(|| id.to_string());
    host.devices()?
        .find(|d| d.id().is_ok_and(|d| d.to_string() == wanted))
        .ok_or_else(|| format!("no sound card {wanted}: unplugged?").into())
}

/// The hardware device of the card whose default device is `id`, if it is
/// one. ALSA lists hardware devices by card index, not name, and
/// `card_index` gives it.
fn hardware_id(id: &str, card_index: impl Fn(&str) -> Option<u32>) -> Option<String> {
    let card = id.strip_prefix("alsa:default:CARD=")?;
    Some(format!("alsa:hw:CARD={},DEV=0", card_index(card)?))
}

/// The index ALSA gave the card with this name: `/proc/asound/PCH` links to
/// `card1`.
fn card_index(name: &str) -> Option<u32> {
    let link = std::fs::read_link(Path::new("/proc/asound").join(name)).ok()?;
    link.to_str()?.strip_prefix("card")?.parse().ok()
}

/// The devices to try, in order, when none was chosen: the system's default,
/// then each sound card. ALSA's default is card 0, and a USB MIDI controller
/// plugged in at boot can take that index although it has no audio output.
/// In low latency on Linux, the default goes through the sound server, so
/// only the cards' hardware devices are tried.
fn defaults(
    default: Option<Device>,
    devices: impl Iterator<Item = Device>,
    low_latency: bool,
) -> impl Iterator<Item = Device> {
    let direct = low_latency && cfg!(target_os = "linux");
    let card = move |id: &str| {
        if direct {
            is_card_hardware(id)
        } else {
            is_card_default(id)
        }
    };
    default
        .filter(|_| !direct)
        .into_iter()
        .chain(devices.filter(move |d| d.id().is_ok_and(|id| card(&id.to_string()))))
}

/// The first of `devices` that `config` works for, or else the first error.
fn first_that_opens(
    devices: impl Iterator<Item = Device>,
    config: impl Fn(&Device) -> Result<SupportedStreamConfig, Box<dyn Error>>,
) -> Result<(Device, SupportedStreamConfig), Box<dyn Error>> {
    let mut first_error = None;
    for device in devices {
        match config(&device) {
            Ok(c) => return Ok((device, c)),
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    Err(first_error.unwrap_or_else(|| "no sound card found".into()))
}

fn output_config(device: &Device) -> Result<SupportedStreamConfig, Box<dyn Error>> {
    let supported = device.default_output_config()?;
    if !FORMATS.contains(&supported.sample_format()) {
        return Err(format!("unsupported sample format {}", supported.sample_format()).into());
    }
    Ok(supported)
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
    low_latency: bool,
) -> Result<Endpoint, Box<dyn Error>> {
    let host = host(low_latency)?;
    let config = |d: &Device| input_config(d, sample_rate);
    let (device, supported) = match id {
        Some(id) => {
            let device = by_id(&host, id, low_latency)?;
            let config = config(&device)?;
            (device, config)
        }
        None => first_that_opens(
            defaults(
                host.default_input_device(),
                host.input_devices()?,
                low_latency,
            ),
            config,
        )?,
    };
    let format = supported.sample_format();
    let config = with_buffer(supported.config(), buffer, |c| {
        device
            .build_input_stream_raw(c, format, |_, _| {}, |_| {}, None)
            .is_ok()
    });
    Ok(Endpoint {
        device,
        config,
        format,
    })
}

fn input_config(
    device: &Device,
    sample_rate: u32,
) -> Result<SupportedStreamConfig, Box<dyn Error>> {
    let default = device.default_input_config()?;
    if FORMATS.contains(&default.sample_format()) && default.sample_rate() == sample_rate {
        return Ok(default);
    }
    FORMATS
        .iter()
        .find_map(|&format| {
            device
                .supported_input_configs()
                .ok()?
                .filter(|c| c.sample_format() == format)
                .find_map(|c| c.try_with_sample_rate(sample_rate))
        })
        .ok_or_else(|| format!("the audio input cannot run at {sample_rate} Hz").into())
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

/// A card's own default device, which mixes with other programs, as opposed
/// to `hw:` (exclusive), `hdmi:` (likely no speakers) or `null` (no sound).
fn is_card_default(id: &str) -> bool {
    id.starts_with("alsa:default:CARD=")
}

/// A card's first hardware device: its main outputs and inputs, where the
/// others are often HDMI.
fn is_card_hardware(id: &str) -> bool {
    id.starts_with("alsa:hw:CARD=") && id.ends_with(",DEV=0")
}

/// Where the engine's sound comes in from: a device and the block that
/// `audio in` modules read.
pub struct Input {
    pub endpoint: Endpoint,
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

fn unsupported(format: SampleFormat) -> Box<dyn Error> {
    format!("unsupported sample format {format}").into()
}

/// Plays the engine's output, also handing every sample to `tap` for live
/// recording, and feeds it `input` when there is one. An input that does not
/// open leaves the engine without, and plays on.
pub fn play(
    output: &Endpoint,
    mut engine: Engine,
    tap: Tap,
    input: Option<Input>,
) -> Result<Playback, Box<dyn Error>> {
    let errors = Arc::new(AtomicUsize::new(0));
    let callbacks = Arc::new(Callbacks::default());
    let (input_stream, input_problem) = match input {
        None => (None, None),
        Some(input) => match capture(input, &mut engine, &callbacks, &errors) {
            Ok(stream) => (Some(stream), None),
            Err(e) => (None, Some(e.to_string())),
        },
    };
    let (callbacks, error_count) = (callbacks, Arc::clone(&errors));
    let stream = match output.format {
        SampleFormat::F32 => output_stream::<f32>(output, engine, tap, callbacks, error_count),
        SampleFormat::I32 => output_stream::<i32>(output, engine, tap, callbacks, error_count),
        SampleFormat::I16 => output_stream::<i16>(output, engine, tap, callbacks, error_count),
        other => return Err(unsupported(other)),
    }?;
    stream.play()?;
    let playback = Playback {
        _output: stream,
        _input: input_stream,
        input_problem,
        errors,
    };
    Ok(playback)
}

fn output_stream<T: SizedSample + FromSample<f32>>(
    output: &Endpoint,
    mut engine: Engine,
    mut tap: Tap,
    callbacks: Arc<Callbacks>,
    errors: Arc<AtomicUsize>,
) -> Result<Stream, cpal::Error> {
    let channels = output.config.channels as usize;
    output.device.build_output_stream(
        output.config,
        move |data: &mut [T], _| {
            callbacks.output_ran(data.len() / channels);
            for frame in data.chunks_mut(channels) {
                let played = engine.next_frame();
                tap.push(played);
                write_frame(frame, played);
            }
        },
        move |_| {
            errors.fetch_add(1, Ordering::Relaxed);
        },
        None,
    )
}

/// Left and right on the first two channels, further channels silent, and
/// both mixed down on a mono device.
fn write_frame<T: Sample + FromSample<f32>>(out: &mut [T], [left, right]: Frame) {
    match out {
        [mono] => *mono = T::from_sample((left + right) / 2.0),
        [l, r, rest @ ..] => {
            *l = T::from_sample(left);
            *r = T::from_sample(right);
            rest.fill(T::EQUILIBRIUM);
        }
        [] => {}
    }
}

fn capture(
    input: Input,
    engine: &mut Engine,
    callbacks: &Arc<Callbacks>,
    errors: &Arc<AtomicUsize>,
) -> Result<Stream, Box<dyn Error>> {
    let (capture, feed) = input::queue(input.live, Arc::clone(callbacks), Arc::clone(errors));
    let endpoint = &input.endpoint;
    let errors = Arc::clone(errors);
    let stream = match endpoint.format {
        SampleFormat::F32 => input_stream::<f32>(endpoint, capture, errors),
        SampleFormat::I32 => input_stream::<i32>(endpoint, capture, errors),
        SampleFormat::I16 => input_stream::<i16>(endpoint, capture, errors),
        other => return Err(unsupported(other)),
    }?;
    stream.play()?;
    engine.set_input(feed);
    Ok(stream)
}

fn input_stream<T: SizedSample>(
    input: &Endpoint,
    mut capture: Capture,
    errors: Arc<AtomicUsize>,
) -> Result<Stream, cpal::Error>
where
    f32: FromSample<T>,
{
    let channels = input.config.channels as usize;
    input.device.build_input_stream(
        input.config,
        move |data: &[T], _| capture.push_with(data, channels, f32::from_sample),
        move |_| {
            errors.fetch_add(1, Ordering::Relaxed);
        },
        None,
    )
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
    fn frames_go_to_the_first_two_channels_or_mix_down_on_one() {
        let mut stereo = [0.0_f32; 2];
        write_frame(&mut stereo, [0.5, -0.5]);
        assert_eq!(stereo, [0.5, -0.5]);
        let mut surround = [1.0_f32; 4];
        write_frame(&mut surround, [0.5, -0.5]);
        assert_eq!(surround, [0.5, -0.5, 0.0, 0.0]);
        let mut mono = [0_i16; 1];
        write_frame(&mut mono, [0.5, 0.25]);
        assert_eq!(mono, [i16::from_sample(0.375_f32)]);
    }

    #[test]
    fn alsa_lists_each_card_once_other_systems_everything() {
        assert!(worth_listing("alsa:default:CARD=U192k"));
        assert!(!worth_listing("alsa:hw:CARD=1,DEV=0"));
        assert!(!worth_listing("alsa:null"));
        assert!(worth_listing("wasapi:{0.0.0.00000000}.{1234}"));
        assert!(worth_listing("asio:UMC ASIO Driver"));
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

    #[test]
    fn low_latency_opens_a_cards_first_hardware_device() {
        let index = |name: &str| (name == "U192k").then_some(2);
        assert_eq!(
            hardware_id("alsa:default:CARD=U192k", index).as_deref(),
            Some("alsa:hw:CARD=2,DEV=0")
        );
        assert_eq!(hardware_id("alsa:default:CARD=Gone", index), None);
        assert_eq!(hardware_id("asio:UMC ASIO Driver", index), None);

        assert!(is_card_hardware("alsa:hw:CARD=2,DEV=0"));
        assert!(!is_card_hardware("alsa:hw:CARD=1,DEV=3"), "HDMI");
        assert!(!is_card_hardware("alsa:plughw:CARD=2,DEV=0"));
    }

    #[test]
    fn alsa_cards_are_found_by_name() {
        // Only where ALSA runs, and with a card.
        let Some(card) = std::fs::read_dir("/proc/asound")
            .into_iter()
            .flatten()
            .flatten()
            .find(|e| e.file_type().is_ok_and(|t| t.is_symlink()))
        else {
            return;
        };
        let name = card.file_name().into_string().unwrap();
        let target = std::fs::read_link(card.path()).unwrap();
        let index = card_index(&name).unwrap();
        assert_eq!(target.to_str(), Some(format!("card{index}").as_str()));
        assert_eq!(card_index("no such card"), None);
    }
}
