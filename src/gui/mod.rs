//! Graphical front end: a node editor with one block per module, wires for
//! cables, sliders for knobs, and patch import and export.

mod editor;
mod keyboard;

use std::error::Error;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui;

use crate::engine::audio::Playback;
use crate::engine::control::Controller;
use crate::engine::record::{self, Recorder};
use crate::modules::Context;
use crate::patch::Patch;
use crate::presets;
use editor::Editor;
use keyboard::Keyboard;

/// How often the window refreshes without input, to keep the audio error
/// count and the recording time current.
const REFRESH: Duration = Duration::from_millis(250);
/// How often it refreshes while lights blink or ticks move: fast enough to
/// follow a beat.
const LIVE_REFRESH: Duration = Duration::from_millis(33);

/// Runs until the window is closed. `patch` is the one already playing; only
/// its block positions are read. `status` is the first message shown.
/// Recordings go to the current directory.
pub fn run(
    controller: Controller,
    context: Context,
    patch: &Patch,
    playback: Playback,
    recorder: Recorder,
    status: String,
) -> Result<(), Box<dyn Error>> {
    let keyboard = Keyboard::new(Arc::clone(&context.midi));
    let mut editor = Editor::new(controller, context, &patch.positions);
    editor.status = status;
    let app = GuiApp {
        editor,
        keyboard,
        playback,
        recorder,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("audiochaos")
            .with_inner_size([1400.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native("audiochaos", options, Box::new(|_| Ok(Box::new(app))))?;
    Ok(())
}

struct GuiApp {
    editor: Editor,
    keyboard: Keyboard,
    /// Owned so the sound plays exactly as long as the window is open.
    playback: Playback,
    recorder: Recorder,
}

impl eframe::App for GuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let refresh = if self.editor.moves() {
            LIVE_REFRESH
        } else {
            REFRESH
        };
        ui.ctx().request_repaint_after(refresh);
        let playing = !ui.ctx().egui_wants_keyboard_input();
        ui.ctx().input(|input| {
            for event in &input.events {
                self.keyboard.handle(event, playing);
            }
        });
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("Patch", |ui| self.patch_menu(ui));
                ui.separator();
                self.record_button(ui);
                ui.separator();
                ui.label(&self.editor.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(format!("audio errors: {}", self.playback.errors()));
                });
            });
        });
        egui::CentralPanel::default().show(ui, |ui| self.editor.show_graph(ui));
    }
}

impl GuiApp {
    fn patch_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Presets", |ui| {
            for name in presets::names() {
                if ui.button(name).clicked() {
                    let patch = presets::get(name).expect("listed presets exist");
                    self.load(&patch, name);
                }
            }
        });
        if ui.button("Arrange blocks").clicked() {
            self.editor.arrange();
        }
        ui.separator();
        if ui.button("Import…").clicked() {
            self.import();
        }
        if ui.button("Export…").clicked() {
            self.export();
        }
        ui.separator();
        let selection = self.editor.selection(ui.ctx());
        let group = self.editor.group(&selection);
        let selected = group.is_some();
        let hint = "Select blocks with Shift+click, or Shift+drag around them";
        let duplicate = egui::Button::new("Duplicate selection").shortcut_text("Ctrl+D");
        if ui
            .add_enabled(selected, duplicate)
            .on_disabled_hover_text(hint)
            .clicked()
        {
            self.editor.duplicate(&selection);
        }
        if ui
            .add_enabled(selected, egui::Button::new("Save selection…"))
            .on_disabled_hover_text(hint)
            .clicked()
            && let Some(group) = group
        {
            self.save(&group, "blocks.json");
        }
        if ui
            .button("Insert blocks…")
            .on_hover_text("Adds the blocks of a saved selection, or of a patch but its master")
            .clicked()
            && let Some((patch, _)) = self.read_patch()
        {
            self.editor.paste(&patch);
        }
    }

    fn record_button(&mut self, ui: &mut egui::Ui) {
        let button = match self.recorder.elapsed() {
            Some(elapsed) => egui::Button::new(
                egui::RichText::new(format!("■ Stop  {}", record::clock(elapsed)))
                    .color(egui::Color32::RED),
            ),
            None => egui::Button::new("● Rec"),
        };
        if ui.add(button).clicked() {
            self.editor.status = self.recorder.toggle(Path::new("."));
        }
    }

    fn load(&mut self, patch: &Patch, label: &str) {
        self.editor.status = match self.editor.load(patch) {
            Ok(problems) if problems.is_empty() => format!("loaded {label}"),
            Ok(problems) => format!("loaded {label}; {}", problems.join("; ")),
            Err(e) => format!("cannot load {label}: {e}"),
        };
    }

    fn import(&mut self) {
        if let Some((patch, label)) = self.read_patch() {
            self.load(&patch, &label);
        }
    }

    /// A patch file picked by the user, and its name; `None`, and what went
    /// wrong in the status, if it does not read.
    fn read_patch(&mut self) -> Option<(Patch, String)> {
        let path = patch_dialog().pick_file()?;
        let label = display_name(&path);
        let patch = match fs::read_to_string(&path) {
            Ok(json) => Patch::from_json(&json).map_err(|e| format!("cannot load {label}: {e}")),
            Err(e) => Err(format!("cannot read {label}: {e}")),
        };
        match patch {
            Ok(patch) => Some((patch, label)),
            Err(e) => {
                self.editor.status = e;
                None
            }
        }
    }

    fn export(&mut self) {
        self.save(&self.editor.patch(), "patch.json");
    }

    fn save(&mut self, patch: &Patch, default_name: &str) {
        let Some(path) = patch_dialog().set_file_name(default_name).save_file() else {
            return;
        };
        let label = display_name(&path);
        self.editor.status = match fs::write(&path, patch.to_json() + "\n") {
            Ok(()) => format!("saved {label}"),
            Err(e) => format!("cannot save {label}: {e}"),
        };
    }
}

fn patch_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("audiochaos patch", &["json"])
}

fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}
