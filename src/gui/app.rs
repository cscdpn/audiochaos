//! The standalone program's window: the editor, plus the sound card, MIDI
//! and recording controls that a DAW provides to the plugin.

use std::error::Error;
use std::path::Path;
use std::sync::Arc;

use eframe::egui;

use crate::engine::audio::{self, DeviceEntry, Playback};
use crate::engine::record::{self, Recorder};
use crate::midi;
use crate::patch::Patch;
use crate::session::{self, Devices, Session};

use super::Editor;
use super::keyboard::Keyboard;

/// Runs until the window is closed. `session` plays `patch` on `devices`;
/// only the patch's block positions are read. `status` is the first message
/// shown. Recordings go to the current directory.
pub fn run(
    session: Session,
    devices: Devices,
    midi: Option<midi::Inputs>,
    patch: &Patch,
    status: String,
) -> Result<(), Box<dyn Error>> {
    let midi_state = Arc::clone(&session.context.midi);
    let keyboard = Keyboard::new(Arc::clone(&midi_state));
    let mut editor = Editor::new(session.controller, session.context, &patch.positions);
    editor.status = status;
    let app = GuiApp {
        editor,
        keyboard,
        playback: session.playback,
        recorder: session.recorder,
        devices,
        device_lists: None,
        midi_state,
        midi_inputs: midi,
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
    /// The sound cards playing.
    devices: Devices,
    /// The outputs and inputs, listed when the Audio menu opens.
    device_lists: Option<(Vec<DeviceEntry>, Vec<DeviceEntry>)>,
    /// What the MIDI controllers play, shared by every session.
    midi_state: Arc<midi::State>,
    /// Owned so the controllers play as long as the window is open.
    midi_inputs: Option<midi::Inputs>,
}

impl eframe::App for GuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(self.editor.refresh());
        let playing = !ui.ctx().egui_wants_keyboard_input();
        ui.ctx().input(|input| {
            for event in &input.events {
                self.keyboard.handle(event, playing);
            }
        });
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("Patch", |ui| self.editor.patch_menu(ui));
                let audio = ui.menu_button("Audio", |ui| self.audio_menu(ui));
                if audio.inner.is_none() {
                    // Closed: list the cards again next time, in case one
                    // was plugged in or out.
                    self.device_lists = None;
                }
                ui.separator();
                self.editor.run_button(ui);
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
    fn audio_menu(&mut self, ui: &mut egui::Ui) {
        let (outputs, inputs) = self
            .device_lists
            .get_or_insert_with(|| (audio::outputs(), audio::inputs()))
            .clone();
        ui.weak("Output");
        if let Some(output) = device_choice(ui, &self.devices.output, &outputs) {
            self.switch(Devices {
                output,
                ..self.devices.clone()
            });
        }
        ui.separator();
        ui.weak("Input");
        if let Some(input) = device_choice(ui, &self.devices.input, &inputs) {
            self.switch(Devices {
                input,
                ..self.devices.clone()
            });
        }
        ui.separator();
        if ui
            .button("Reconnect MIDI controllers")
            .on_hover_text("Picks up controllers plugged in since")
            .clicked()
        {
            // Closed first: some systems open a port only once.
            self.midi_inputs = None;
            let (inputs, status) = session::connect_midi(&self.midi_state);
            self.midi_inputs = inputs;
            self.editor.status = status;
        }
    }

    /// Plays the patch on other sound cards. The modules start afresh, as
    /// the new cards may run at another rate: sequencers start over and
    /// loopers forget their recordings. The running recording is saved.
    /// Nothing changes if the cards do not open.
    fn switch(&mut self, devices: Devices) {
        let patch = self.editor.patch();
        match session::start(&patch, &devices, &self.midi_state) {
            Ok(session) => {
                // The old streams stop as they are dropped here.
                self.playback = session.playback;
                self.recorder = session.recorder;
                self.editor
                    .replace_engine(session.controller, session.context);
                self.devices = devices;
                let mut status = vec![format!("playing on {}", session.output)];
                status.extend(session.problems);
                self.editor.status = status.join("; ");
            }
            Err(e) => self.editor.status = format!("cannot switch sound cards: {e}"),
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
}

/// Radio buttons for the system's default and each of `devices`, `current`
/// selected. Returns the one clicked, if another.
fn device_choice(
    ui: &mut egui::Ui,
    current: &Option<String>,
    devices: &[DeviceEntry],
) -> Option<Option<String>> {
    let mut choice = None;
    if ui.radio(current.is_none(), "System default").clicked() && current.is_some() {
        choice = Some(None);
    }
    for device in devices {
        let selected = current.as_deref() == Some(device.id.as_str());
        if ui.radio(selected, &device.name).clicked() && !selected {
            choice = Some(Some(device.id.clone()));
        }
    }
    if devices.is_empty() {
        ui.weak("no sound card found");
    }
    choice
}
