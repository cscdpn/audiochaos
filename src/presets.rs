//! The built-in presets, compiled into the binary.

use crate::patch::Patch;

/// In the same JSON format as saved patches. The first one is the default.
const PRESETS: [(&str, &str); 6] = [
    ("chaos", include_str!("../presets/chaos.json")),
    ("bass", include_str!("../presets/bass.json")),
    ("groovygroove", include_str!("../presets/groovygroove.json")),
    ("keys", include_str!("../presets/keys.json")),
    ("chords", include_str!("../presets/chords.json")),
    ("looper", include_str!("../presets/looper.json")),
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
    use std::f32::consts::TAU;
    use std::sync::Arc;

    use crate::modules::Context;
    use crate::modules::master;
    use crate::modules::test_util::{rising_zero_crossings, rms};

    const SR: f32 = 48_000.0;

    fn render(preset_name: &str, seconds: usize) -> Vec<f32> {
        let (mut g, out) = get(preset_name).unwrap().build(&Context::new(SR)).unwrap();
        g.render(out, master::OUT, seconds * SR as usize)
    }

    /// 100 ms windows.
    fn windows(audio: &[f32]) -> impl Iterator<Item = &[f32]> {
        audio
            .as_chunks::<{ SR as usize / 10 }>()
            .0
            .iter()
            .map(|w| w.as_slice())
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
            let (g, _) = patch.build(&Context::new(SR)).unwrap();
            let mut saved = Patch::from_graph(&g);
            saved.description = patch.description.clone();
            saved.positions = patch.positions.clone();
            assert_eq!(saved, patch, "{name}");
        }
        assert_eq!(get("nope"), None);
        assert_eq!(default(), get("chaos").unwrap());
    }

    #[test]
    fn keys_play_from_the_controller_and_rest_silent() {
        use crate::midi::Parser;
        let context = Context::new(SR);
        let (mut g, out) = get("keys").unwrap().build(&context).unwrap();
        assert!(rms(&g.render(out, master::OUT, SR as usize)) < 1e-3);
        let mut parser = Parser::default();
        parser.handle(&[0x90, 60, 100], &context.midi);
        let played = rms(&g.render(out, master::OUT, SR as usize));
        assert!(played > 0.05, "keys {played}");
        parser.handle(&[0x80, 60, 0], &context.midi);
        parser.handle(&[0x99, 36, 100], &context.midi);
        assert!(
            rms(&g.render(out, master::OUT, SR as usize)) > 0.05,
            "pad 1"
        );
    }

    #[test]
    fn chords_play_every_note_and_fade_out() {
        use crate::midi::Parser;
        let context = Context::new(SR);
        let (mut g, out) = get("chords").unwrap().build(&context).unwrap();
        assert!(rms(&g.render(out, master::OUT, SR as usize)) < 1e-3);
        let mut parser = Parser::default();
        parser.handle(&[0x90, 60, 100], &context.midi);
        let one = rms(&g.render(out, master::OUT, SR as usize));
        for note in [64, 67] {
            parser.handle(&[0x90, note, 100], &context.midi);
        }
        let three = rms(&g.render(out, master::OUT, SR as usize));
        assert!(one > 0.02, "one note {one}");
        assert!(
            three > 1.3 * one,
            "a chord {three} is no louder than a note {one}"
        );
        // One voice per note, the fourth silent.
        let voice = |g: &mut crate::engine::graph::Graph, v: usize| {
            let vca = g
                .nodes()
                .find(|&n| g.name(n) == format!("vca {v}"))
                .unwrap();
            rms(&g.render(vca, crate::modules::vca::OUT, 4800))
        };
        for v in 1..=3 {
            assert!(voice(&mut g, v) > 0.1, "voice {v}");
        }
        assert!(voice(&mut g, 4) < 1e-6, "voice 4");
        for note in [60, 64, 67] {
            parser.handle(&[0x80, note, 0], &context.midi);
        }
        // The release lasts 0.8 s.
        g.render(out, master::OUT, SR as usize);
        assert!(rms(&g.render(out, master::OUT, SR as usize / 10)) < 1e-3);
    }

    #[test]
    fn looper_loops_what_came_in_while_a_key_was_held() {
        use crate::engine::input::{Callbacks, queue};
        use crate::engine::module::BLOCK;
        use crate::midi::Parser;
        let context = Context::new(SR);
        let (mut g, out) = get("looper").unwrap().build(&context).unwrap();
        let (mut capture, mut feed) = queue(
            Arc::clone(&context.input),
            Arc::new(Callbacks::default()),
            Arc::default(),
        );
        // A 440 Hz tone on input 1, block by block as a sound card would.
        let mut t = 0;
        let mut play = |g: &mut crate::engine::graph::Graph, tone: bool, blocks: usize| {
            let mut audio = Vec::new();
            for _ in 0..blocks {
                let frames: Vec<f32> = (t..t + BLOCK)
                    .flat_map(|i| {
                        let x = (TAU * 440.0 * i as f32 / SR).sin();
                        [if tone { 0.5 * x } else { 0.0 }, 0.0]
                    })
                    .collect();
                t += BLOCK;
                capture.push(&frames, 2);
                feed.next_block();
                audio.extend(g.render(out, master::OUT, BLOCK));
            }
            audio
        };
        let second = SR as usize / BLOCK;
        assert!(rms(&play(&mut g, true, second)) < 1e-3, "not recording yet");
        let mut parser = Parser::default();
        parser.handle(&[0x90, 60, 100], &context.midi);
        play(&mut g, true, second / 2);
        parser.handle(&[0x80, 60, 0], &context.midi);
        // The tone has stopped; the loop goes on.
        let looped = play(&mut g, false, 2 * second);
        assert!(rms(&looped) > 0.05, "{}", rms(&looped));
        let pitch = rising_zero_crossings(&looped[..SR as usize]);
        assert!(pitch.abs_diff(440) < 15, "{pitch} Hz");
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
