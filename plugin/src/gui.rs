//! The node editor in the host's window: the standalone program's editor,
//! with the Patch menu and the Stop button, but no sound card, MIDI or
//! recording controls, which are the host's business.

use std::sync::{Arc, Mutex};

use audiochaos::gui::Editor;
use egui::Vec2;
use nice_plug::editor::dpi::LogicalSize;
use nice_plug::prelude::ResizeHint;
use nice_plug_egui::resizable_window::ResizableWindow;
use nice_plug_egui::{
    EguiEditor, EguiEditorState, EguiNiceSettings, Frame, NiceEguiApp, RepaintNotifier,
    create_egui_editor,
};

use crate::lock;

const INITIAL_SIZE: LogicalSize<f32> = LogicalSize::new(1200.0, 800.0);
const MIN_SIZE: LogicalSize<f32> = LogicalSize::new(600.0, 400.0);

/// The window's size, kept across openings.
pub fn state() -> Arc<EguiEditorState> {
    EguiEditorState::from_size(INITIAL_SIZE, 1.0)
}

pub fn editor(state: Arc<EguiEditorState>, editor: Arc<Mutex<Editor>>) -> Option<EguiEditor<Gui>> {
    let resize = ResizeHint::resizable().with_min_logical_size(MIN_SIZE);
    create_egui_editor(
        state,
        RepaintNotifier::new(),
        EguiNiceSettings::new().with_resize_hint(resize),
        Gui { editor },
    )
}

pub struct Gui {
    editor: Arc<Mutex<Editor>>,
}

impl NiceEguiApp for Gui {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        let mut editor = lock(&self.editor);
        ui.ctx().request_repaint_after(editor.refresh());
        ResizableWindow::new("audiochaos")
            .min_size(Vec2::new(MIN_SIZE.width, MIN_SIZE.height))
            .show(ui, |ui| {
                egui::Panel::top("menu").show(ui, |ui| {
                    egui::MenuBar::new().ui(ui, |ui| {
                        ui.menu_button("Patch", |ui| editor.patch_menu(ui));
                        ui.separator();
                        editor.run_button(ui);
                        ui.separator();
                        ui.label(&editor.status);
                    });
                });
                egui::CentralPanel::default().show(ui, |ui| editor.show_graph(ui));
            });
    }
}
