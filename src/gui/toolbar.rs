//! What both the standalone program and the plugin show above the editor:
//! the Patch menu and the Stop button.

use std::fs;
use std::path::Path;

use crate::patch::Patch;
use crate::presets;

use super::Editor;

impl Editor {
    /// The Patch menu: presets, layout, files, and groups of blocks.
    pub fn patch_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Presets", |ui| {
            for name in presets::names() {
                if ui.button(name).clicked() {
                    let patch = presets::get(name).expect("listed presets exist");
                    self.load_labelled(&patch, name);
                }
            }
        });
        if ui.button("Arrange blocks").clicked() {
            self.arrange();
        }
        ui.separator();
        if ui.button("Import…").clicked() {
            self.import();
        }
        if ui.button("Export…").clicked() {
            self.export();
        }
        ui.separator();
        let selection = self.selection(ui.ctx());
        let group = self.group(&selection);
        let selected = group.is_some();
        let hint = "Select blocks with Shift+click, or Shift+drag around them";
        let duplicate = egui::Button::new("Duplicate selection").shortcut_text("Ctrl+D");
        if ui
            .add_enabled(selected, duplicate)
            .on_disabled_hover_text(hint)
            .clicked()
        {
            self.duplicate(&selection);
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
            self.paste(&patch);
        }
    }

    /// Stops or starts the whole synth.
    pub fn run_button(&mut self, ui: &mut egui::Ui) {
        let controller = &self.controller;
        let (label, tip) = if controller.running() {
            ("⏸ Stop", "Silences the synth and holds everything still")
        } else {
            ("▶ Start", "Plays again from where it stopped")
        };
        if ui.button(label).on_hover_text(tip).clicked() {
            controller.set_running(!controller.running());
        }
    }

    fn load_labelled(&mut self, patch: &Patch, label: &str) {
        self.status = match self.load(patch) {
            Ok(problems) if problems.is_empty() => format!("loaded {label}"),
            Ok(problems) => format!("loaded {label}; {}", problems.join("; ")),
            Err(e) => format!("cannot load {label}: {e}"),
        };
    }

    fn import(&mut self) {
        if let Some((patch, label)) = self.read_patch() {
            self.load_labelled(&patch, &label);
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
                self.status = e;
                None
            }
        }
    }

    fn export(&mut self) {
        self.save(&self.patch(), "patch.json");
    }

    fn save(&mut self, patch: &Patch, default_name: &str) {
        let Some(path) = patch_dialog().set_file_name(default_name).save_file() else {
            return;
        };
        let label = display_name(&path);
        self.status = match fs::write(&path, patch.to_json() + "\n") {
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
