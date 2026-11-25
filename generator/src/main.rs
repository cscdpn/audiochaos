//! `audiochaos-gen`: prints the song a text or a file gives.

use std::ffi::OsString;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: audiochaos-gen TEXT
       audiochaos-gen --file PATH
       COMMAND | audiochaos-gen

Prints the audiochaos patch generated from TEXT, taken as is, from the
bytes of any file at PATH (- reads standard input), or from what another
command pipes in. The same input always gives the same patch, for a given
version of the generator.

    audiochaos-gen \"hello world\" > hello.json
    head -c 64 /dev/urandom | audiochaos-gen > noise.json
    audiochaos hello.json";

#[derive(Debug, PartialEq)]
enum Input {
    Text(String),
    File(PathBuf),
    Stdin,
}

/// The input the arguments name, `None` for help. Without arguments, what
/// another command pipes in, but not what someone would have to type.
fn parse(args: &[OsString], piped: bool) -> Result<Option<Input>, String> {
    let words: Vec<Option<&str>> = args.iter().map(|a| a.to_str()).collect();
    match &words[..] {
        [] if piped => Ok(Some(Input::Stdin)),
        [Some("-h" | "--help")] => Ok(None),
        [Some("--file"), Some("-")] => Ok(Some(Input::Stdin)),
        [Some("--file"), _] => Ok(Some(Input::File(PathBuf::from(&args[1])))),
        [Some("--file")] => Err(format!("--file needs a path\n{USAGE}")),
        [Some(text)] => Ok(Some(Input::Text(text.to_string()))),
        [None] => Err(format!(
            "the text is not valid Unicode: pass it with --file\n{USAGE}"
        )),
        _ => Err(USAGE.to_string()),
    }
}

fn read(input: Input) -> Result<Vec<u8>, String> {
    match input {
        Input::Text(text) => Ok(text.into_bytes()),
        Input::File(path) => {
            std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))
        }
        Input::Stdin => {
            let mut bytes = Vec::new();
            io::stdin()
                .read_to_end(&mut bytes)
                .map_err(|e| format!("cannot read standard input: {e}"))?;
            Ok(bytes)
        }
    }
}

/// Writes the patch. A reader that stops early, as `head`, is no error:
/// the rest was not wanted.
fn write(json: &str, out: &mut impl Write) -> Result<(), String> {
    match writeln!(out, "{json}").and_then(|()| out.flush()) {
        Err(e) if e.kind() != io::ErrorKind::BrokenPipe => {
            Err(format!("cannot write the patch: {e}"))
        }
        _ => Ok(()),
    }
}

fn run() -> Result<(), String> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let Some(input) = parse(&args, !io::stdin().is_terminal())? else {
        println!("{USAGE}");
        return Ok(());
    };
    let patch = audiochaos_gen::generate(&read(input)?);
    write(&patch.to_json(), &mut io::stdout().lock())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(args: &[&str]) -> Result<Option<Input>, String> {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        parse(&args, false)
    }

    #[test]
    fn one_argument_is_the_text_as_is() {
        assert_eq!(
            parse_str(&["  Hello, world  "]),
            Ok(Some(Input::Text("  Hello, world  ".into())))
        );
        assert_eq!(parse_str(&[""]), Ok(Some(Input::Text(String::new()))));
    }

    #[test]
    fn file_takes_a_path_or_standard_input() {
        assert_eq!(
            parse_str(&["--file", "song.bin"]),
            Ok(Some(Input::File("song.bin".into())))
        );
        assert_eq!(parse_str(&["--file", "-"]), Ok(Some(Input::Stdin)));
    }

    #[test]
    fn without_arguments_a_pipe_is_the_input() {
        assert_eq!(parse(&[], true), Ok(Some(Input::Stdin)));
        // From a terminal, the user would have to type the input.
        assert!(parse(&[], false).is_err());
        // Arguments win over the pipe.
        let args = [OsString::from("hello")];
        assert_eq!(parse(&args, true), Ok(Some(Input::Text("hello".into()))));
    }

    /// A pipe whose reader has gone, as after `| head -c 10`.
    struct Closed;

    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_reader_stopping_early_is_no_error() {
        assert_eq!(write("{}", &mut Closed), Ok(()));
        let mut out = Vec::new();
        assert_eq!(write("{}", &mut out), Ok(()));
        assert_eq!(out, b"{}\n");
    }

    #[test]
    fn help_and_mistakes() {
        assert_eq!(parse_str(&["--help"]), Ok(None));
        assert!(parse_str(&[]).is_err());
        assert!(parse_str(&["--file"]).is_err());
        assert!(parse_str(&["two", "words"]).is_err());
    }

    #[test]
    fn a_text_is_read_as_its_utf8_bytes() {
        assert_eq!(read(Input::Text("é".into())), Ok(vec![0xc3, 0xa9]));
    }
}
