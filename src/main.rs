use std::env;
use std::error::Error;
use std::process::ExitCode;

use audiochaos::engine::audio;
use audiochaos::engine::control::Engine;
use audiochaos::rack::{self, Preset};
use audiochaos::tui::{self, App};

/// Room for a burst of key presses even if the audio thread stalls briefly.
const COMMAND_QUEUE_CAPACITY: usize = 1024;

/// Usage: `audiochaos [preset]`, with `chaos` as the default.
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
    let preset = match env::args().nth(1) {
        None => Preset::Chaos,
        Some(name) => Preset::from_name(&name).ok_or_else(|| {
            let names: Vec<_> = Preset::ALL.iter().map(|p| p.name()).collect();
            format!(
                "unknown preset {name:?}, expected one of: {}",
                names.join(", ")
            )
        })?,
    };
    let (device, config) = audio::default_output()?;
    let (graph, output) = rack::build(config.sample_rate as f32, preset)?;
    let (engine, controller) = Engine::new(graph, output, COMMAND_QUEUE_CAPACITY);
    let playback = audio::play(&device, config, engine)?;
    tui::run(App::new(controller), &playback)?;
    Ok(())
}
