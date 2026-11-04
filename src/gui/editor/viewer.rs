//! The bridge between egui-snarl and the controller: draws each block, and
//! turns the user's edits into controller calls.

use std::collections::HashMap;

use egui::emath::TSTransform;
use egui::{Color32, Pos2, Rect, Ui, Vec2};
use egui_snarl::ui::{PinInfo, SnarlPin, SnarlViewer};
use egui_snarl::{InPin, InPinId, NodeId as BlockId, OutPin, OutPinId, Snarl};

use crate::engine::control::{Controller, SampleState};
use crate::engine::graph::{Cable, NodeId};
use crate::engine::sample::Sample;
use crate::modules::clock::Tap;
use crate::modules::{self, Context, clock, master, sampler};
use crate::patch;

use super::layout::BLOCK_GAP;
use super::snarl_id;
use super::widgets::{AUDIO_PIN, CV_PIN, SLIDER_WIDTH, knob, led};

/// Draws blocks for egui-snarl and turns edits into controller calls.
pub(super) struct Viewer<'a> {
    pub(super) controller: &'a mut Controller,
    pub(super) context: &'a Context,
    pub(super) status: &'a mut String,
    pub(super) sizes: &'a mut HashMap<BlockId, Vec2>,
    pub(super) show_origin: &'a mut bool,
    pub(super) taps: &'a mut HashMap<NodeId, Tap>,
    pub(super) to_global: &'a mut TSTransform,
    pub(super) to_duplicate: &'a mut Option<Vec<BlockId>>,
    /// Where the graph is drawn, on screen.
    pub(super) view: Rect,
}

impl Viewer<'_> {
    fn cable(snarl: &Snarl<NodeId>, from: OutPinId, to: InPinId) -> Cable {
        Cable {
            from: snarl[from.node],
            from_port: from.output,
            to: snarl[to.node],
            to_port: to.input,
        }
    }

    /// Shows the sound's file; clicking it picks another.
    fn show_sample_button(&mut self, id: NodeId, sample: Option<&SampleState>, ui: &mut Ui) {
        let label = sample.map_or("Load sample…", |s| Sample::file_name(&s.path));
        let problem = sample.and_then(|s| s.problem.as_deref());
        let text = match problem {
            Some(_) => egui::RichText::new(label).color(Color32::RED),
            None => egui::RichText::new(label),
        };
        let mut button = ui.button(text);
        if let Some(problem) = problem {
            button = button.on_hover_text(problem);
        }
        if button.clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("WAV sound", &["wav"])
                .pick_file()
        {
            self.load_sample(id, &path.to_string_lossy());
        }
    }

    /// Sets the clock's tempo from the rhythm of clicks, unless its `tap`
    /// input does, which overrides the knob.
    fn show_tap_button(&mut self, id: NodeId, ui: &mut Ui) {
        let tap_plugged = self
            .controller
            .cables()
            .iter()
            .any(|c| c.to == id && c.to_port == clock::IN_TAP);
        let button = ui
            .add_enabled(!tap_plugged, egui::Button::new("Tap"))
            .on_disabled_hover_text("The tap input sets the tempo");
        if button.clicked() {
            let now = ui.input(|i| i.time);
            if let Some(bpm) = self.taps.entry(id).or_default().tap(now)
                && let Err(e) = self.controller.set_param(id, clock::TEMPO, bpm)
            {
                *self.status = format!("tempo not changed: {e}");
            }
        }
    }

    fn unplug(&mut self, from: OutPinId, to: InPinId, snarl: &mut Snarl<NodeId>) {
        match self.controller.disconnect(Self::cable(snarl, from, to)) {
            Ok(_) => {
                snarl.disconnect(from, to);
            }
            Err(e) => *self.status = format!("cannot disconnect: {e}"),
        }
    }

    /// Adds a module and its block at `pos`.
    pub(super) fn add(&mut self, kind: &str, pos: Pos2, snarl: &mut Snarl<NodeId>) {
        match patch::add_module(self.controller, self.context, kind) {
            Ok(id) => {
                snarl.insert_node(pos, id);
                *self.status = format!("added {}", self.controller.node(id).name);
            }
            Err(e) => *self.status = format!("cannot add {kind}: {e}"),
        }
    }

    /// Gives a sampler the sound in a file. A file that cannot be read changes
    /// nothing.
    pub(super) fn load_sample(&mut self, id: NodeId, path: &str) {
        let name = Sample::file_name(path);
        *self.status = match Sample::load(path) {
            Ok(sample) => match self.controller.set_sample(id, Box::new(sample)) {
                Ok(()) => format!("loaded {name}"),
                Err(e) => format!("cannot load {name}: {e}"),
            },
            Err(e) => format!("cannot load {name}: {e}"),
        };
    }

    /// Removes a module, its block and its wires.
    pub(super) fn remove(&mut self, block: BlockId, snarl: &mut Snarl<NodeId>) {
        let id = snarl[block];
        let name = self.controller.node(id).name.clone();
        match self.controller.remove(id) {
            Ok(()) => {
                snarl.remove_node(block);
                *self.status = format!("removed {name}");
            }
            Err(e) => *self.status = format!("cannot remove {name}: {e}"),
        }
    }
}

impl SnarlViewer<NodeId> for Viewer<'_> {
    fn title(&mut self, node: &NodeId) -> String {
        self.controller.node(*node).name.clone()
    }

    /// The title, then the light of modules that have one.
    fn show_header(
        &mut self,
        block: BlockId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeId>,
    ) {
        let id = snarl[block];
        let description = self.controller.node(id).info.description;
        // Not selectable, else the title takes the clicks and drags that
        // select and move the block.
        ui.add(egui::Label::new(self.title(&id)).selectable(false))
            .on_hover_text(description);
        if let Some(brightness) = self.controller.led(id) {
            led(brightness, ui);
        }
    }

    fn inputs(&mut self, node: &NodeId) -> usize {
        self.controller.node(*node).info.num_input_ports()
    }

    fn outputs(&mut self, node: &NodeId) -> usize {
        self.controller.node(*node).info.outputs.len()
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut Ui,
        snarl: &mut Snarl<NodeId>,
    ) -> impl SnarlPin + 'static {
        let info = self.controller.node(snarl[pin.id.node]).info;
        ui.label(info.input_port_name(pin.id.input));
        let colour = if pin.id.input < info.inputs.len() {
            AUDIO_PIN
        } else {
            CV_PIN
        };
        PinInfo::circle().with_fill(colour)
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut Ui,
        snarl: &mut Snarl<NodeId>,
    ) -> impl SnarlPin + 'static {
        let info = self.controller.node(snarl[pin.id.node]).info;
        // Laid out from the pin leftwards.
        ui.label(info.outputs[pin.id.output]);
        ui.weak(info.output_details[pin.id.output]);
        PinInfo::circle().with_fill(AUDIO_PIN)
    }

    fn final_node_rect(
        &mut self,
        block: BlockId,
        rect: Rect,
        _ui: &mut Ui,
        _snarl: &mut Snarl<NodeId>,
    ) {
        self.sizes.insert(block, rect.size());
    }

    /// Shows the top left corner of the graph, where `arrange` starts, at the
    /// current zoom.
    fn current_transform(&mut self, to_global: &mut TSTransform, _snarl: &mut Snarl<NodeId>) {
        if std::mem::take(self.show_origin) {
            to_global.translation = self.view.min.to_vec2() + Vec2::splat(BLOCK_GAP);
        }
        *self.to_global = *to_global;
    }

    fn has_body(&mut self, node: &NodeId) -> bool {
        !self.controller.node(*node).info.params.is_empty()
    }

    fn show_body(
        &mut self,
        block: BlockId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeId>,
    ) {
        let node = self.controller.node(snarl[block]);
        let (id, info, mut values) = (node.id, node.info, node.params.clone());
        let sample = node.sample.clone();
        ui.spacing_mut().slider_width = SLIDER_WIDTH;
        ui.vertical(|ui| {
            for (p, (param, value)) in info.params.iter().zip(&mut values).enumerate() {
                let modulated = self.controller.modulated(id, p);
                if knob(param, value, modulated, ui)
                    && let Err(e) = self.controller.set_param(id, p, *value)
                {
                    *self.status = format!("knob not changed: {e}");
                }
            }
            if info.name == sampler::INFO.name {
                self.show_sample_button(id, sample.as_ref(), ui);
            }
            if info.name == clock::INFO.name {
                self.show_tap_button(id, ui);
            }
        });
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<NodeId>) {
        match self.controller.connect(Self::cable(snarl, from.id, to.id)) {
            Ok(()) => {
                snarl.connect(from.id, to.id);
            }
            Err(e) => *self.status = format!("cannot connect: {e}"),
        }
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<NodeId>) {
        self.unplug(from.id, to.id, snarl);
    }

    fn drop_outputs(&mut self, pin: &OutPin, snarl: &mut Snarl<NodeId>) {
        for &to in &pin.remotes {
            self.unplug(pin.id, to, snarl);
        }
    }

    fn drop_inputs(&mut self, pin: &InPin, snarl: &mut Snarl<NodeId>) {
        for &from in &pin.remotes {
            self.unplug(from, pin.id, snarl);
        }
    }

    /// Right-click on the canvas: the palette.
    fn has_graph_menu(&mut self, _pos: Pos2, _snarl: &mut Snarl<NodeId>) -> bool {
        true
    }

    fn show_graph_menu(&mut self, pos: Pos2, ui: &mut Ui, snarl: &mut Snarl<NodeId>) {
        ui.label("Add module");
        // The master is the one output; a patch has exactly one.
        for info in modules::infos().filter(|i| i.name != master::INFO.name) {
            if ui
                .button(info.name)
                .on_hover_text(info.description)
                .clicked()
            {
                self.add(info.name, pos, snarl);
                ui.close();
            }
        }
    }

    /// Right-click on a block's title.
    fn has_node_menu(&mut self, _node: &NodeId) -> bool {
        true
    }

    fn show_node_menu(
        &mut self,
        block: BlockId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeId>,
    ) {
        let removable = snarl[block] != self.controller.output();
        let button = ui
            .add_enabled(removable, egui::Button::new("Duplicate"))
            .on_disabled_hover_text("A patch has one master");
        if button.clicked() {
            // The whole selection if this block is in it.
            let selection = egui_snarl::ui::get_selected_nodes(snarl_id(), ui.ctx());
            let blocks = if selection.contains(&block) {
                selection
            } else {
                vec![block]
            };
            *self.to_duplicate = Some(blocks);
            ui.close();
        }
        let button = ui
            .add_enabled(removable, egui::Button::new("Remove"))
            .on_disabled_hover_text("The master feeds the speakers");
        if button.clicked() {
            self.remove(block, snarl);
            ui.close();
        }
    }
}
