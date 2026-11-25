//! Graphical front end: a node editor with one block per module, wires for
//! cables, sliders for knobs, and patch import and export. The standalone
//! program shows it in a window of its own, the plugin in the host's.

#[cfg(feature = "app")]
mod app;
mod editor;
#[cfg(feature = "app")]
mod keyboard;
#[cfg(feature = "app")]
mod settings;
mod theme;
mod toolbar;

#[cfg(feature = "app")]
pub use app::run;
pub use editor::Editor;
pub use theme::Theme;
