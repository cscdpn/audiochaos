//! The fixed set of modules the UI patches together, and the built-in presets
//! it can start from.

use crate::engine::graph::{Graph, NodeId};
use crate::modules::{
    atten::Atten, crusher::Crusher, delay::Delay, distortion::Distortion, filter::Filter, lfo::Lfo,
    master::Master, noise::Noise, oscillator::Oscillator, sample_hold::SampleHold,
    sequencer::Sequencer, vca::Vca,
};
use crate::patch::{Patch, PatchError};

/// Built-in presets, in the same JSON format as saved patches. The first one
/// is the default.
const PRESETS: [(&str, &str); 2] = [
    ("chaos", include_str!("../presets/chaos.json")),
    ("bass", include_str!("../presets/bass.json")),
];

pub fn preset_names() -> impl Iterator<Item = &'static str> {
    PRESETS.iter().map(|(name, _)| *name)
}

pub fn default_preset() -> Patch {
    preset(PRESETS[0].0).expect("the default preset exists")
}

pub fn preset(name: &str) -> Option<Patch> {
    let (_, json) = PRESETS.iter().find(|(n, _)| *n == name)?;
    Some(Patch::from_json(json).expect("built-in presets are valid"))
}

/// Builds the rack, sets it up with `patch`, and returns it with the node that
/// feeds the speakers.
///
/// Modules are ordered from sources to the master: a cable going backwards in
/// this order is a feedback cable and costs one block of delay.
pub fn build(sample_rate: f32, patch: &Patch) -> Result<(Graph, NodeId), PatchError> {
    let sr = sample_rate;
    let mut g = Graph::new(sr);
    g.add("seq", Box::new(Sequencer::new(sr)));
    g.add("lfo", Box::new(Lfo::new(sr)));
    g.add("noise", Box::new(Noise::new(0x5eed)));
    g.add("s&h", Box::new(SampleHold::new()));
    g.add("atten", Box::new(Atten));
    g.add("osc 1", Box::new(Oscillator::new(sr)));
    g.add("osc 2", Box::new(Oscillator::new(sr)));
    g.add("filter", Box::new(Filter::new(sr)));
    g.add("dist", Box::new(Distortion));
    g.add("crush", Box::new(Crusher::new()));
    g.add("delay", Box::new(Delay::new(sr)));
    g.add("vca", Box::new(Vca));
    let master = g.add("master", Box::new(Master));
    patch.apply_to_graph(&mut g)?;
    Ok((g, master))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::master;
    use crate::modules::test_util::{rising_zero_crossings, rms};

    const SR: f32 = 48_000.0;

    fn render(preset_name: &str, seconds: usize) -> Vec<f32> {
        let (mut g, out) = build(SR, &preset(preset_name).unwrap()).unwrap();
        g.render(out, master::OUT, seconds * SR as usize)
    }

    /// 100 ms windows.
    fn windows(audio: &[f32]) -> impl Iterator<Item = &[f32]> {
        audio.chunks_exact(SR as usize / 10)
    }

    fn spread(values: &[f32]) -> f32 {
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32;
        variance.sqrt() / mean
    }

    #[test]
    fn every_preset_loads_and_round_trips() {
        for name in preset_names() {
            let patch = preset(name).unwrap();
            let (g, _) = build(SR, &patch).unwrap();
            let mut saved = Patch::from_graph(&g);
            saved.description = patch.description.clone();
            assert_eq!(saved, patch, "{name}");
        }
        assert_eq!(preset("nope"), None);
        assert_eq!(default_preset(), preset("chaos").unwrap());
    }

    #[test]
    fn bass_makes_sound() {
        assert!(rms(&render("bass", 1)) > 0.05);
    }

    #[test]
    fn chaos_is_loud_and_bounded() {
        let audio = render("chaos", 10);
        assert!(audio.iter().all(|s| s.abs() <= 1.0));
        assert!(rms(&audio) > 0.2, "rms {}", rms(&audio));
    }

    #[test]
    fn chaos_keeps_changing() {
        let audio = render("chaos", 10);
        // Loudness and brightness (zero crossings) both move from one 100 ms
        // window to the next, and the patch never falls silent.
        let loudness: Vec<f32> = windows(&audio).map(rms).collect();
        let brightness: Vec<f32> = windows(&audio)
            .map(|w| rising_zero_crossings(w) as f32)
            .collect();
        assert!(loudness.iter().all(|&l| l > 0.02), "a window went silent");
        assert!(
            spread(&loudness) > 0.1,
            "loudness spread {}",
            spread(&loudness)
        );
        assert!(
            spread(&brightness) > 0.1,
            "brightness spread {}",
            spread(&brightness)
        );
    }
}
