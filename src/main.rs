use std::error::Error;
use std::io::{self, BufRead, Write};

use audiochaos::engine::audio;
use audiochaos::engine::control::Engine;
use audiochaos::engine::graph::{Cable, Graph};
use audiochaos::modules::master::{self, Master};
use audiochaos::modules::oscillator::{self, Oscillator};

/// Temporary command-line front end: plays a sine and reads new frequencies
/// from stdin. It will be replaced by the TUI.
fn main() -> Result<(), Box<dyn Error>> {
    let (device, config) = audio::default_output()?;
    let sample_rate = config.sample_rate as f32;

    let mut graph = Graph::new();
    let osc = graph.add(Box::new(Oscillator::new(sample_rate, 220.0)));
    let out = graph.add(Box::new(Master::new(sample_rate, 0.5)));
    graph.connect(Cable {
        from: osc,
        from_port: oscillator::OUT_SINE,
        to: out,
        to_port: master::IN,
    })?;

    let (engine, mut controller) = Engine::new(graph, out, 64);
    let _stream = audio::play(&device, config, engine)?;

    println!("Playing a 220 Hz sine. Type a frequency in Hz and press Enter, or q to quit.");
    for line in io::stdin().lock().lines() {
        let line = line?;
        match line.trim() {
            "q" => break,
            text => match text.parse::<f32>() {
                Ok(freq) => {
                    if controller.set_param(osc, oscillator::FREQ, freq).is_err() {
                        eprintln!("audio thread is not keeping up, command dropped");
                    }
                }
                Err(_) => eprintln!("not a number: {text}"),
            },
        }
        print!("> ");
        io::stdout().flush()?;
    }
    Ok(())
}
