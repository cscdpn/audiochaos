//! The tempo and position of the DAW hosting the plugin, for `clock`
//! modules to follow. The plugin writes them for every frame; the audio
//! thread reads them, so atomics, like MIDI.

use std::sync::atomic::{AtomicU64, Ordering};

/// Both NaN when unknown: in the standalone program, or when the host does
/// not say.
pub struct Transport {
    /// Beats per minute.
    tempo: AtomicU64,
    /// Beats since the start of the song, while the host plays.
    beats: AtomicU64,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            tempo: AtomicU64::new(f64::NAN.to_bits()),
            beats: AtomicU64::new(f64::NAN.to_bits()),
        }
    }
}

impl Transport {
    pub fn set(&self, tempo: Option<f64>, beats: Option<f64>) {
        store(&self.tempo, tempo);
        store(&self.beats, beats);
    }

    pub fn tempo(&self) -> Option<f64> {
        load(&self.tempo)
    }

    pub fn beats(&self) -> Option<f64> {
        load(&self.beats)
    }
}

fn store(atomic: &AtomicU64, value: Option<f64>) {
    atomic.store(value.unwrap_or(f64::NAN).to_bits(), Ordering::Relaxed);
}

fn load(atomic: &AtomicU64) -> Option<f64> {
    Some(f64::from_bits(atomic.load(Ordering::Relaxed))).filter(|v| !v.is_nan())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_until_set() {
        let transport = Transport::default();
        assert_eq!((transport.tempo(), transport.beats()), (None, None));
        transport.set(Some(97.5), Some(12.25));
        assert_eq!(
            (transport.tempo(), transport.beats()),
            (Some(97.5), Some(12.25))
        );
        transport.set(Some(97.5), None);
        assert_eq!(transport.beats(), None);
    }
}
