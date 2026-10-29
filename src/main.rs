use std::error::Error;
use std::process::ExitCode;
use std::{env, fs, io};

use audiochaos::engine::audio;
use audiochaos::engine::control::Engine;
use audiochaos::patch::Patch;
use audiochaos::tui::{self, App};
use audiochaos::{gui, rack};

const USAGE: &str = "usage: audiochaos [--tui] [preset | patch.json]

Starts the graphical editor, or the terminal UI with --tui, on the given
built-in preset or patch file (default: the first preset).";

/// Room for a burst of key presses, or a whole patch being loaded, even if
/// the audio thread stalls briefly.
const COMMAND_QUEUE_CAPACITY: usize = 1024;

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

#[derive(Debug)]
struct Args {
    tui: bool,
    patch: Option<String>,
}

/// `None` when help was asked for.
fn parse_args(args: impl Iterator<Item = String>) -> Result<Option<Args>, String> {
    let mut parsed = Args {
        tui: false,
        patch: None,
    };
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--tui" => parsed.tui = true,
            _ if arg.starts_with('-') => return Err(format!("unknown option {arg}\n{USAGE}")),
            _ if parsed.patch.is_some() => return Err(format!("too many arguments\n{USAGE}")),
            _ => parsed.patch = Some(arg),
        }
    }
    Ok(Some(parsed))
}

fn run() -> Result<(), Box<dyn Error>> {
    let Some(args) = parse_args(env::args().skip(1))? else {
        println!("{USAGE}");
        return Ok(());
    };
    let patch = match &args.patch {
        None => rack::default_preset(),
        Some(arg) => load_patch(arg)?,
    };
    let (device, config) = audio::default_output()?;
    let (graph, output) = rack::build(config.sample_rate as f32, &patch)?;
    let (engine, controller) = Engine::new(graph, output, COMMAND_QUEUE_CAPACITY);
    let playback = audio::play(&device, config, engine)?;
    if args.tui {
        tui::run(App::new(controller), &playback)?;
    } else {
        gui::run(controller, &patch, playback)?;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<Args>, String> {
        parse_args(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn defaults_to_the_gui_and_the_default_preset() {
        let args = parse(&[]).unwrap().unwrap();
        assert!(!args.tui);
        assert_eq!(args.patch, None);
    }

    #[test]
    fn takes_a_flag_and_a_patch_in_any_order() {
        let args = parse(&["bass", "--tui"]).unwrap().unwrap();
        assert!(args.tui);
        assert_eq!(args.patch.as_deref(), Some("bass"));
    }

    #[test]
    fn help_and_mistakes() {
        assert!(parse(&["--help"]).unwrap().is_none());
        assert!(
            parse(&["--nope"])
                .unwrap_err()
                .starts_with("unknown option")
        );
        assert!(parse(&["a", "b"]).unwrap_err().starts_with("too many"));
    }
}
