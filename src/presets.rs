//! The built-in presets, compiled into the binary.

use crate::patch::Patch;

/// In the same JSON format as saved patches. The first one is the default.
const PRESETS: [(&str, &str); 3] = [
    ("chaos", include_str!("../presets/chaos.json")),
    ("bass", include_str!("../presets/bass.json")),
    ("groovygroove", include_str!("../presets/groovygroove.json")),
];

pub fn names() -> impl Iterator<Item = &'static str> {
    PRESETS.iter().map(|(name, _)| *name)
}

pub fn default() -> Patch {
    get(PRESETS[0].0).expect("the default preset exists")
}

pub fn get(name: &str) -> Option<Patch> {
    let (_, json) = PRESETS.iter().find(|(n, _)| *n == name)?;
    Some(Patch::from_json(json).expect("built-in presets are valid"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::master;
    use crate::modules::test_util::{rising_zero_crossings, rms};

    const SR: f32 = 48_000.0;

    fn render(preset_name: &str, seconds: usize) -> Vec<f32> {
        let (mut g, out) = get(preset_name).unwrap().build(SR).unwrap();
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
        for name in names() {
            let patch = get(name).unwrap();
            let (g, _) = patch.build(SR).unwrap();
            let mut saved = Patch::from_graph(&g);
            saved.description = patch.description.clone();
            saved.positions = patch.positions.clone();
            assert_eq!(saved, patch, "{name}");
        }
        assert_eq!(get("nope"), None);
        assert_eq!(default(), get("chaos").unwrap());
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
