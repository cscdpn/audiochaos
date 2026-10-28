//! Terminal front end: one screen to turn knobs, one to patch cables.

mod app;
mod view;

use std::io;
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event};

pub use app::App;

use crate::engine::audio::Playback;

/// How often the screen refreshes without input, to keep the audio error
/// count current.
const REFRESH: Duration = Duration::from_millis(100);

/// Runs until the user quits. The terminal is restored even on error or panic.
pub fn run(app: App, playback: &Playback) -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, app, playback);
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal, mut app: App, playback: &Playback) -> io::Result<()> {
    while !app.quit {
        terminal.draw(|frame| view::draw(frame, &app, playback.errors()))?;
        if event::poll(REFRESH)?
            && let Event::Key(key) = event::read()?
        {
            app.handle_key(key);
        }
    }
    Ok(())
}
