//! The only part of the engine that touches the sound card.

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};

use super::control::Engine;
use super::record::Tap;

/// The default output device and its preferred configuration. The sample rate
/// must be known before building the graph, hence the two-step setup.
///
/// ALSA's default is card 0, and a USB MIDI controller plugged in at boot can
/// take that index although it has no audio output. So when the default does
/// not open, the first sound card that does takes over.
pub fn default_output() -> Result<(Device, StreamConfig), Box<dyn Error>> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("no audio output device")?;
    match f32_config(&device) {
        Ok(config) => Ok((device, config)),
        Err(e) => host
            .output_devices()?
            .filter(|d| d.id().is_ok_and(|id| is_card_default(&id.to_string())))
            .find_map(|d| f32_config(&d).ok().map(|config| (d, config)))
            .ok_or(e),
    }
}

fn f32_config(device: &Device) -> Result<StreamConfig, Box<dyn Error>> {
    let supported = device.default_output_config()?;
    // Every desktop backend (PipeWire, PulseAudio, CoreAudio, WASAPI) offers
    // f32 by default; supporting other formats is not worth the code yet.
    if supported.sample_format() != SampleFormat::F32 {
        return Err(format!("unsupported sample format {}", supported.sample_format()).into());
    }
    Ok(supported.config())
}

/// A card's own default device, which mixes with other programs, as opposed
/// to `hw:` (exclusive), `hdmi:` (likely no speakers) or `null` (no sound).
fn is_card_default(id: &str) -> bool {
    id.starts_with("alsa:default:CARD=")
}

/// A running audio stream. Sound stops when it is dropped.
pub struct Playback {
    _stream: Stream,
    errors: Arc<AtomicUsize>,
}

impl Playback {
    /// Stream errors so far, mostly buffer underruns. Counted rather than
    /// printed, because printing would corrupt the TUI.
    pub fn errors(&self) -> usize {
        self.errors.load(Ordering::Relaxed)
    }
}

/// Plays the engine's output, also handing every sample to `tap` for live
/// recording.
pub fn play(
    device: &Device,
    config: StreamConfig,
    mut engine: Engine,
    mut tap: Tap,
) -> Result<Playback, Box<dyn Error>> {
    let channels = config.channels as usize;
    let errors = Arc::new(AtomicUsize::new(0));
    let error_count = Arc::clone(&errors);
    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _| {
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
    Ok(Playback {
        _stream: stream,
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
