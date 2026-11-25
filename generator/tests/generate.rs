//! Every input gives a valid patch that plays, and always the same one.

use std::path::Path;

use audiochaos::engine::module::BLOCK;
use audiochaos::modules::{self, Context};
use audiochaos::patch::Patch;
use audiochaos_gen::generate;

const SR: f32 = 48_000.0;

/// Texts and raw bytes, short and long, empty included.
fn inputs() -> impl Iterator<Item = Vec<u8>> {
    let texts = (0..150).map(|i| format!("song number {i}").into_bytes());
    let bytes = (0..150u32).map(|i| i.to_le_bytes().repeat(i as usize % 7));
    [Vec::new(), "é ✓ \u{0}".as_bytes().to_vec()]
        .into_iter()
        .chain(texts)
        .chain(bytes)
}

#[test]
fn the_same_input_gives_the_same_patch() {
    for input in inputs().take(20) {
        assert_eq!(generate(&input), generate(&input));
    }
}

#[test]
fn inputs_one_byte_apart_give_different_songs() {
    assert_ne!(generate(b"hello"), generate(b"hellp"));
    assert_ne!(generate(b""), generate(b"\0"));
}

#[test]
fn every_patch_loads_with_its_knobs_in_range() {
    let context = Context::new(SR);
    for input in inputs() {
        let patch = generate(&input);
        let reloaded = Patch::from_json(&patch.to_json()).unwrap();
        assert_eq!(reloaded, patch, "{input:?} does not survive JSON");
        if let Err(e) = patch.build(&context) {
            panic!("{input:?} does not load: {e}\n{}", patch.to_json());
        }
        for (module, knobs) in &patch.knobs {
            let kind = &patch
                .modules
                .iter()
                .find(|m| &m.name == module)
                .unwrap()
                .kind;
            let info = modules::infos().find(|info| info.name == kind).unwrap();
            for (knob, &value) in knobs {
                let param = info.params.iter().find(|p| p.name == knob).unwrap();
                assert!(
                    (param.min..=param.max).contains(&value),
                    "{input:?}: {module} {knob} at {value}"
                );
            }
        }
    }
}

/// Plays the first section until it makes sound: something always plays
/// from the start.
#[test]
fn every_song_makes_sound_from_its_first_section_and_stays_finite() {
    let context = Context::new(SR);
    for input in inputs().step_by(10) {
        let patch = generate(&input);
        let (mut graph, out) = patch.build(&context).unwrap();
        let song = patch.modules.iter().find(|m| m.kind == "song").unwrap();
        let bars = song.sections.as_ref().unwrap()[0].bars as f32;
        let tempo = patch.knobs["clock"]["tempo"];
        let samples = (bars * 240.0 / tempo * SR) as usize;
        let mut heard = false;
        for _ in 0..samples / BLOCK {
            graph.process();
            let audio = graph.outputs(out);
            let [left, right, ..] = audio else {
                unreachable!("the master has two outputs")
            };
            assert!(
                left.iter().chain(right).all(|s| s.is_finite()),
                "{input:?} went non-finite"
            );
            if left.iter().any(|s| s.abs() > 1e-3) {
                heard = true;
                break;
            }
        }
        assert!(heard, "{input:?} is silent in its first section");
    }
}

/// A change to the generator that changes the songs inputs give must bump
/// its `VERSION`, then refresh this file.
#[test]
fn hello_world_still_gives_the_same_song() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hello-world.json");
    let expected = generate(b"hello world").to_json() + "\n";
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &expected).unwrap();
    }
    // Git checks text files out with CRLF line endings on Windows.
    let actual = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        actual == expected,
        "{} changed: bump audiochaos_gen::VERSION, then run \
         `UPDATE_GOLDEN=1 cargo test -p audiochaos-gen`",
        path.display()
    );
}
