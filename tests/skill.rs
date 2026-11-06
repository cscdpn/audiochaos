//! The skill that teaches a language model to write songs, in
//! `skills/audiochaos-song`: its module reference is generated from the
//! modules themselves, and its example has to play.

use std::fmt::Write;
use std::path::Path;

use audiochaos::engine::score::SCALES;
use audiochaos::modules::{self, Context, master};
use audiochaos::patch::Patch;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/skills/audiochaos-song");

/// Modules that need a controller, a sound card input or sound files,
/// which a generated song cannot count on.
const LEFT_OUT: [&str; 5] = ["input", "midi", "poly", "sampler", "looper"];

fn reference() -> String {
    let mut text = String::from(
        "# Module reference\n\
         \n\
         Generated from the code by `tests/skill.rs`: do not edit by hand.\n\
         \n\
         Every module type a song can use, with its exact port and knob names.\n\
         Each knob also has a CV input named after it plus ` cv`: `freq` has\n\
         `freq cv`. Several cables into one input add up.\n",
    );
    for info in modules::infos().filter(|info| !LEFT_OUT.contains(&info.name)) {
        writeln!(text, "\n## {}\n\n{}\n", info.name, info.description).unwrap();
        let inputs: Vec<String> = info.inputs.iter().map(|i| format!("`{i}`")).collect();
        let inputs = if inputs.is_empty() {
            "none".to_string()
        } else {
            inputs.join(", ")
        };
        writeln!(text, "- inputs: {inputs}").unwrap();
        let outputs: Vec<String> = info
            .outputs
            .iter()
            .zip(info.output_details)
            .map(|(o, detail)| format!("`{o}` ({detail})"))
            .collect();
        writeln!(text, "- outputs: {}", outputs.join(", ")).unwrap();
        if !info.params.is_empty() {
            writeln!(text, "- knobs:").unwrap();
        }
        for p in info.params {
            let unit = if p.unit.is_empty() {
                String::new()
            } else {
                format!(" {}", p.unit)
            };
            writeln!(
                text,
                "  - `{}`: {}..{}{unit}, default {}. {}",
                p.name, p.min, p.max, p.default, p.help
            )
            .unwrap();
        }
    }
    text
}

#[test]
fn the_module_reference_is_up_to_date() {
    let path = Path::new(DIR).join("reference.md");
    let expected = reference();
    if std::env::var_os("UPDATE_SKILL").is_some() {
        std::fs::write(&path, &expected).unwrap();
    }
    // Git checks text files out with CRLF line endings on Windows.
    let actual = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        actual == expected,
        "{} is out of date: run `UPDATE_SKILL=1 cargo test --test skill`",
        path.display()
    );
}

#[test]
fn the_scales_listed_are_the_ones_there_are() {
    let skill = std::fs::read_to_string(Path::new(DIR).join("SKILL.md")).unwrap();
    // Wrapped over several lines there.
    let skill = skill.split_whitespace().collect::<Vec<_>>().join(" ");
    let names: Vec<&str> = SCALES.iter().map(|(name, _)| *name).collect();
    assert!(
        skill.contains(&format!("one of these scales: {}.", names.join(", "))),
        "SKILL.md should list: {}",
        names.join(", ")
    );
}

#[test]
fn the_example_loads_and_plays() {
    const SR: f32 = 48_000.0;
    let json = std::fs::read_to_string(Path::new(DIR).join("example.json")).unwrap();
    let patch = Patch::from_json(&json).unwrap();
    let (mut graph, out) = patch.build(&Context::new(SR)).unwrap();
    let audio = graph.render(out, master::OUT_LEFT, 10 * SR as usize);
    let rms = (audio.iter().map(|s| s * s).sum::<f32>() / audio.len() as f32).sqrt();
    assert!(rms > 0.02, "rms {rms}");
}
