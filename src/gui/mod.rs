//! Graphical front end: a node editor with one block per module, wires for
//! cables, sliders for knobs, and patch import and export.

mod editor;

use std::error::Error;
use std::fs;
use std::path::Path;
use std::time::Duration;

use eframe::egui;

use crate::engine::audio::Playback;
use crate::engine::control::Controller;
use crate::engine::record::{self, Recorder};
use crate::modules::Context;
use crate::patch::Patch;
use crate::presets;
use editor::Editor;

/// How often the window refreshes without input, to keep the audio error
/// count and the recording time current.
const REFRESH: Duration = Duration::from_millis(250);

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
    let mut editor = Editor::new(controller, context, &patch.positions);
    editor.status = status;
    let app = GuiApp {
        editor,
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
    /// Owned so the sound plays exactly as long as the window is open.
    playback: Playback,
    recorder: Recorder,
}

impl eframe::App for GuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(REFRESH);
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
        ui.separator();
        if ui.button("Import…").clicked() {
            self.import();
        }
        if ui.button("Export…").clicked() {
            self.export();
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
        let Some(path) = patch_dialog().pick_file() else {
            return;
        };
        let label = display_name(&path);
        match fs::read_to_string(&path) {
            Ok(json) => match Patch::from_json(&json) {
                Ok(patch) => self.load(&patch, &label),
                Err(e) => self.editor.status = format!("cannot load {label}: {e}"),
            },
            Err(e) => self.editor.status = format!("cannot read {label}: {e}"),
        }
    }

    fn export(&mut self) {
        let Some(path) = patch_dialog().set_file_name("patch.json").save_file() else {
            return;
        };
        let label = display_name(&path);
        self.editor.status = match fs::write(&path, self.editor.patch().to_json() + "\n") {
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
