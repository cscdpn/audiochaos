use std::error::Error;

use audiochaos::engine::audio;
use audiochaos::engine::control::Engine;
use audiochaos::rack;
use audiochaos::tui::{self, App};

/// Room for a burst of key presses even if the audio thread stalls briefly.
const COMMAND_QUEUE_CAPACITY: usize = 1024;

fn main() -> Result<(), Box<dyn Error>> {
    let (device, config) = audio::default_output()?;
    let (graph, output) = rack::build(config.sample_rate as f32)?;
    let (engine, controller) = Engine::new(graph, output, COMMAND_QUEUE_CAPACITY);
    let playback = audio::play(&device, config, engine)?;
    tui::run(App::new(controller), &playback)?;
    Ok(())
}
