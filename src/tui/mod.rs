//! Terminal front end: one screen to turn knobs, one to patch cables.

mod app;
mod view;

use std::io;
use std::path::Path;
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};

pub use app::App;

use crate::engine::audio::Playback;
use crate::engine::record::Recorder;

/// How often the screen refreshes without input, to keep the audio error
/// count current.
const REFRESH: Duration = Duration::from_millis(100);

/// Runs until the user quits. The terminal is restored even on error or panic.
/// Recordings go to the current directory.
pub fn run(app: App, playback: &Playback, recorder: Recorder) -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, app, playback, recorder);
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    mut app: App,
    playback: &Playback,
    mut recorder: Recorder,
) -> io::Result<()> {
    while !app.quit {
        terminal.draw(|frame| view::draw(frame, &app, playback.errors(), recorder.elapsed()))?;
        if event::poll(REFRESH)?
            && let Event::Key(key) = event::read()?
        {
            if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('r') {
                app.status = recorder.toggle(Path::new("."));
            } else {
                app.handle_key(key);
            }
        }
    }
    Ok(())
}
