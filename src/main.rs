use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;
use std::{env, fs, io};

use audiochaos::engine::control::Engine;
use audiochaos::engine::{audio, record};
use audiochaos::midi;
use audiochaos::modules::Context;
use audiochaos::patch::Patch;
use audiochaos::tui::{self, App};
use audiochaos::{gui, presets};

const USAGE: &str = "usage: audiochaos [--tui] [preset | patch.json]
       audiochaos --render out.wav [--seconds N] [preset | patch.json]
       audiochaos --midi-monitor

Starts the graphical editor, or the terminal UI with --tui, on the given
built-in preset or patch file (default: the first preset).

--render writes N seconds (default 30) of the patch to a WAV file instead,
without opening the sound card.

--midi-monitor prints what the connected MIDI controllers send.";

const DEFAULT_RENDER_SECONDS: f32 = 30.0;
/// The sound card's rate is unknown, and irrelevant, when rendering offline.
const RENDER_SAMPLE_RATE: f32 = 48_000.0;

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
    render: Option<PathBuf>,
    seconds: Option<f32>,
    midi_monitor: bool,
}

/// `None` when help was asked for.
fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Option<Args>, String> {
    let mut parsed = Args {
        tui: false,
        patch: None,
        render: None,
        seconds: None,
        midi_monitor: false,
    };
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--tui" => parsed.tui = true,
            "--midi-monitor" => parsed.midi_monitor = true,
            "--render" => parsed.render = Some(value()?.into()),
            "--seconds" => {
                let seconds = value()?;
                match seconds.parse::<f32>() {
                    Ok(s) if s > 0.0 && s.is_finite() => parsed.seconds = Some(s),
                    _ => return Err(format!("invalid duration {seconds:?}\n{USAGE}")),
                }
            }
            _ if arg.starts_with('-') => return Err(format!("unknown option {arg}\n{USAGE}")),
            _ if parsed.patch.is_some() => return Err(format!("too many arguments\n{USAGE}")),
            _ => parsed.patch = Some(arg),
        }
    }
    if parsed.render.is_some() && parsed.tui {
        return Err(format!("--render and --tui do not go together\n{USAGE}"));
    }
    let others = parsed.tui || parsed.patch.is_some() || parsed.render.is_some();
    if parsed.midi_monitor && others {
        return Err(format!("--midi-monitor goes alone\n{USAGE}"));
    }
    if parsed.seconds.is_some() && parsed.render.is_none() {
        return Err(format!("--seconds only goes with --render\n{USAGE}"));
    }
    Ok(Some(parsed))
}

fn run() -> Result<(), Box<dyn Error>> {
    let Some(args) = parse_args(env::args().skip(1))? else {
        println!("{USAGE}");
        return Ok(());
    };
    if args.midi_monitor {
        return midi::monitor();
    }
    let patch = match &args.patch {
        None => presets::default(),
        Some(arg) => load_patch(arg)?,
    };
    if let Some(path) = &args.render {
        let seconds = args.seconds.unwrap_or(DEFAULT_RENDER_SECONDS);
        let (mut graph, output) = patch.build(&Context::new(RENDER_SAMPLE_RATE))?;
        record::render(&mut graph, output, seconds, path)
            .map_err(|e| format!("cannot render to {}: {e}", path.display()))?;
        println!("wrote {seconds} s to {}", path.display());
        return Ok(());
    }
    let (device, config) = audio::default_output()?;
    let context = Context::new(config.sample_rate as f32);
    // A missing MIDI setup is no reason not to play.
    let (_midi_inputs, status) = match midi::connect(&context.midi) {
        Ok(inputs) if inputs.names.is_empty() => (Some(inputs), "no MIDI controller".to_string()),
        Ok(inputs) => {
            let status = format!("MIDI: {}", inputs.names.join(", "));
            (Some(inputs), status)
        }
        Err(e) => (None, format!("no MIDI: {e}")),
    };
    let (graph, output) = patch.build(&context)?;
    let (engine, controller) = Engine::new(graph, output, COMMAND_QUEUE_CAPACITY);
    let (tap, recorder) = record::tap(config.sample_rate);
    let playback = audio::play(&device, config, engine, tap)?;
    if args.tui {
        let mut app = App::new(controller);
        app.status = status;
        tui::run(app, &playback, recorder)?;
    } else {
        gui::run(controller, context, &patch, playback, recorder, status)?;
    }
    Ok(())
}

/// A built-in preset name, or else a path to a patch file.
fn load_patch(arg: &str) -> Result<Patch, Box<dyn Error>> {
    if let Some(patch) = presets::get(arg) {
        return Ok(patch);
    }
    match fs::read_to_string(arg) {
        Ok(json) => Ok(Patch::from_json(&json)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let names: Vec<_> = presets::names().collect();
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
        assert_eq!(args.render, None);
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

    #[test]
    fn render_takes_a_path_and_an_optional_duration() {
        let args = parse(&["--render", "out.wav", "bass"]).unwrap().unwrap();
        assert_eq!(args.render, Some(PathBuf::from("out.wav")));
        assert_eq!(args.seconds, None);
        assert_eq!(args.patch.as_deref(), Some("bass"));

        let args = parse(&["--seconds", "2.5", "--render", "x.wav"])
            .unwrap()
            .unwrap();
        assert_eq!(args.seconds, Some(2.5));
    }

    #[test]
    fn render_mistakes() {
        let error = |args: &[&str]| parse(args).unwrap_err();
        assert!(error(&["--render"]).starts_with("--render needs a value"));
        assert!(error(&["--render", "x.wav", "--seconds", "-1"]).starts_with("invalid duration"));
        assert!(error(&["--render", "x.wav", "--seconds", "abc"]).starts_with("invalid duration"));
        assert!(error(&["--seconds", "3"]).starts_with("--seconds only goes with --render"));
        assert!(error(&["--render", "x.wav", "--tui"]).contains("do not go together"));
    }

    #[test]
    fn midi_monitor_goes_alone() {
        assert!(parse(&["--midi-monitor"]).unwrap().unwrap().midi_monitor);
        let error = parse(&["--midi-monitor", "bass"]).unwrap_err();
        assert!(error.starts_with("--midi-monitor goes alone"));
    }
}
