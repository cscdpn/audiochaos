use std::error::Error;
use std::process::ExitCode;
use std::{env, fs, io};

use audiochaos::engine::audio;
use audiochaos::engine::control::Engine;
use audiochaos::patch::Patch;
use audiochaos::rack;
use audiochaos::tui::{self, App};

/// Room for a burst of key presses, or a whole patch being loaded, even if
/// the audio thread stalls briefly.
const COMMAND_QUEUE_CAPACITY: usize = 1024;

/// Usage: `audiochaos [preset | patch.json]`, starting on the default preset
/// when no argument is given.
fn main() -> ExitCode {
    // Returning the error from `main` would print it with `Debug`, quotes and
    // escapes included.
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let patch = match env::args().nth(1) {
        None => rack::default_preset(),
        Some(arg) => load_patch(&arg)?,
    };
    let (device, config) = audio::default_output()?;
    let (graph, output) = rack::build(config.sample_rate as f32, &patch)?;
    let (engine, controller) = Engine::new(graph, output, COMMAND_QUEUE_CAPACITY);
    let playback = audio::play(&device, config, engine)?;
    tui::run(App::new(controller), &playback)?;
    Ok(())
}

/// A built-in preset name, or else a path to a patch file.
fn load_patch(arg: &str) -> Result<Patch, Box<dyn Error>> {
    if let Some(patch) = rack::preset(arg) {
        return Ok(patch);
    }
    match fs::read_to_string(arg) {
        Ok(json) => Ok(Patch::from_json(&json)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let names: Vec<_> = rack::preset_names().collect();
            Err(format!(
                "no preset or file named {arg:?} (presets: {})",
                names.join(", ")
            )
            .into())
        }
        Err(e) => Err(format!("cannot read {arg:?}: {e}").into()),
    }
}
