use std::collections::{BTreeMap, HashSet};
use std::error::Error;

use eframe::egui::{self, Color32, Pos2, Ui};
use egui_snarl::ui::{PinInfo, SnarlPin, SnarlStyle, SnarlViewer, SnarlWidget};
use egui_snarl::{InPin, InPinId, NodeId as BlockId, OutPin, OutPinId, Snarl};

use crate::engine::control::Controller;
use crate::engine::graph::{Cable, NodeId};
use crate::engine::param::Scale;
use crate::modules::{self, master};
use crate::patch::{self, Patch};

const AUDIO_PIN: Color32 = Color32::from_rgb(0xe0, 0x6c, 0x3c);
const CV_PIN: Color32 = Color32::from_rgb(0x4c, 0x9b, 0xd6);
const SLIDER_WIDTH: f32 = 150.0;

/// Default layout: blocks stacked in columns, in processing order (sources
/// first, master last in the presets), starting a new column past this
/// height.
const COLUMN_HEIGHT: f32 = 900.0;
const COLUMN_WIDTH: f32 = 330.0;
const ROW_HEIGHT: f32 = 24.0;
const BLOCK_GAP: f32 = 50.0;

/// The node editor: one block per module, wires for cables.
///
/// The controller is the source of truth. The snarl (egui-snarl's graph) is
/// a view of it plus block positions; a block or wire is added to or removed
/// from the snarl only once the controller has accepted the change.
pub struct Editor {
    pub(super) controller: Controller,
    /// Each block holds the id of its module.
    snarl: Snarl<NodeId>,
    pub(super) status: String,
}

impl Editor {
    pub fn new(controller: Controller, positions: &BTreeMap<String, [f32; 2]>) -> Self {
        let mut editor = Self {
            controller,
            snarl: Snarl::new(),
            status: String::new(),
        };
        editor.rebuild(positions);
        editor
    }

    /// Recreates every block and wire from the controller. Modules missing
    /// from `positions` get the default layout.
    fn rebuild(&mut self, positions: &BTreeMap<String, [f32; 2]>) {
        let layout = default_layout(&self.controller);
        self.snarl = Snarl::new();
        for (node, default) in self.controller.nodes().iter().zip(layout) {
            let [x, y] = positions.get(&node.name).copied().unwrap_or(default);
            self.snarl.insert_node(egui::pos2(x, y), node.id);
        }
        for &cable in self.controller.cables() {
            let (from, to) = wire(&self.snarl, cable);
            self.snarl.connect(from, to);
        }
    }

    /// egui-snarl removes wires on its own, without asking the viewer, when
    /// the user yanks them off a pin with Ctrl+drag and drops them on nothing.
    /// Cables whose wire is gone are disconnected, so what plays is always
    /// what is drawn.
    fn disconnect_cables_without_wires(&mut self) {
        let drawn: HashSet<_> = self.snarl.wires().collect();
        for cable in self.controller.cables().to_vec() {
            let (from, to) = wire(&self.snarl, cable);
            if drawn.contains(&(from, to)) {
                continue;
            }
            if let Err(e) = self.controller.disconnect(cable) {
                self.status = format!("cannot disconnect: {e}");
                self.snarl.connect(from, to);
            }
        }
    }

    /// Replaces the playing patch, block positions included.
    pub fn load(&mut self, patch: &Patch) -> Result<(), Box<dyn Error>> {
        patch.load_into(&mut self.controller)?;
        self.rebuild(&patch.positions);
        Ok(())
    }

    /// The playing patch, block positions included.
    pub fn patch(&self) -> Patch {
        let mut patch = Patch::from_controller(&self.controller);
        patch.positions = self
            .snarl
            .nodes_pos()
            .map(|(pos, &id)| (self.controller.node(id).name.clone(), [pos.x, pos.y]))
            .collect();
        patch
    }

    pub fn show_graph(&mut self, ui: &mut Ui) {
        let mut viewer = Viewer {
            controller: &mut self.controller,
            status: &mut self.status,
        };
        SnarlWidget::new()
            .id(egui::Id::new("patch"))
            .style(SnarlStyle::new())
            .show(&mut self.snarl, &mut viewer, ui);
        self.disconnect_cables_without_wires();
    }
}

fn block(snarl: &Snarl<NodeId>, id: NodeId) -> BlockId {
    snarl
        .node_ids()
        .find(|&(_, &node)| node == id)
        .map(|(block, _)| block)
        .expect("every module has a block")
}

/// The pins a cable is drawn between.
fn wire(snarl: &Snarl<NodeId>, cable: Cable) -> (OutPinId, InPinId) {
    let from = OutPinId {
        node: block(snarl, cable.from),
        output: cable.from_port,
    };
    let to = InPinId {
        node: block(snarl, cable.to),
        input: cable.to_port,
    };
    (from, to)
}

/// Columns of blocks, sized from each block's pin count, in the controller's
/// module order.
fn default_layout(controller: &Controller) -> Vec<[f32; 2]> {
    let (mut x, mut y) = (0.0, 0.0);
    controller
        .nodes()
        .iter()
        .map(|node| {
            let rows = node.info.num_input_ports().max(node.info.outputs.len());
            let height = ROW_HEIGHT * (rows + 1) as f32;
            if y > 0.0 && y + height > COLUMN_HEIGHT {
                (x, y) = (x + COLUMN_WIDTH, 0.0);
            }
            let position = [x, y];
            y += height + BLOCK_GAP;
            position
        })
        .collect()
}

/// Draws blocks for egui-snarl and turns edits into controller calls.
struct Viewer<'a> {
    controller: &'a mut Controller,
    status: &'a mut String,
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

    fn unplug(&mut self, from: OutPinId, to: InPinId, snarl: &mut Snarl<NodeId>) {
        match self.controller.disconnect(Self::cable(snarl, from, to)) {
            Ok(_) => {
                snarl.disconnect(from, to);
            }
            Err(e) => *self.status = format!("cannot disconnect: {e}"),
        }
    }

    /// Adds a module and its block at `pos`.
    fn add(&mut self, kind: &str, pos: Pos2, snarl: &mut Snarl<NodeId>) {
        match patch::add_module(self.controller, kind) {
            Ok(id) => {
                snarl.insert_node(pos, id);
                *self.status = format!("added {}", self.controller.node(id).name);
            }
            Err(e) => *self.status = format!("cannot add {kind}: {e}"),
        }
    }

    /// Removes a module, its block and its wires.
    fn remove(&mut self, block: BlockId, snarl: &mut Snarl<NodeId>) {
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
        ui.label(info.outputs[pin.id.output]);
        PinInfo::circle().with_fill(AUDIO_PIN)
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
        ui.spacing_mut().slider_width = SLIDER_WIDTH;
        ui.vertical(|ui| {
            for (p, (param, value)) in info.params.iter().zip(&mut values).enumerate() {
                let slider = egui::Slider::new(value, param.min..=param.max)
                    .text(param.name)
                    .logarithmic(param.scale == Scale::Exponential)
                    .custom_formatter(|v, _| param.format(v as f32));
                if ui.add(slider).changed()
                    && let Err(e) = self.controller.set_param(id, p, *value)
                {
                    *self.status = format!("knob not changed: {e}");
                }
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
        for info in modules::KINDS
            .iter()
            .filter(|i| i.name != master::INFO.name)
        {
            if ui.button(info.name).clicked() {
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
            .add_enabled(removable, egui::Button::new("Remove"))
            .on_disabled_hover_text("The master feeds the speakers");
        if button.clicked() {
            self.remove(block, snarl);
            ui.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::control::Engine;
    use crate::modules::{filter, master, oscillator};
    use crate::presets;

    fn editor(preset: &str) -> Editor {
        let patch = presets::get(preset).unwrap();
        let (graph, out) = patch.build(48_000.0).unwrap();
        // Large enough for every command these tests send without an engine
        // draining the queue.
        let (_, controller) = Engine::new(graph, out, 1024);
        Editor::new(controller, &patch.positions)
    }

    fn module(editor: &Editor, name: &str) -> NodeId {
        let nodes = editor.controller.nodes();
        nodes.iter().find(|n| n.name == name).unwrap().id
    }

    fn block_of(editor: &Editor, name: &str) -> BlockId {
        block(&editor.snarl, module(editor, name))
    }

    fn wires(editor: &Editor) -> Vec<(OutPinId, InPinId)> {
        let mut wires: Vec<_> = editor.snarl.wires().collect();
        wires.sort();
        wires
    }

    /// The wires the controller's cables should be drawn as.
    fn expected_wires(editor: &Editor) -> Vec<(OutPinId, InPinId)> {
        let mut wires: Vec<_> = editor
            .controller
            .cables()
            .iter()
            .map(|&c| wire(&editor.snarl, c))
            .collect();
        wires.sort();
        wires
    }

    /// Runs `f` with a viewer, as egui-snarl would when the user edits wires.
    fn with_viewer(editor: &mut Editor, f: impl FnOnce(&mut Viewer, &mut Snarl<NodeId>)) {
        let mut viewer = Viewer {
            controller: &mut editor.controller,
            status: &mut editor.status,
        };
        f(&mut viewer, &mut editor.snarl);
    }

    /// osc 1 saw into the filter's cutoff CV, as pins.
    fn osc_to_cutoff(editor: &Editor) -> (OutPinId, InPinId) {
        (
            OutPinId {
                node: block_of(editor, "osc 1"),
                output: oscillator::OUT_SAW,
            },
            InPinId {
                node: block_of(editor, "filter"),
                input: filter::INFO.cv_port(filter::CUTOFF),
            },
        )
    }

    #[test]
    fn draws_one_block_per_module_and_one_wire_per_cable() {
        let editor = editor("chaos");
        assert_eq!(
            editor.snarl.nodes().count(),
            editor.controller.nodes().len()
        );
        assert_eq!(wires(&editor), expected_wires(&editor));
        assert!(!wires(&editor).is_empty());
    }

    #[test]
    fn dragging_a_wire_connects_the_cable() {
        let mut editor = editor("bass");
        let (from, to) = osc_to_cutoff(&editor);
        with_viewer(&mut editor, |viewer, snarl| {
            viewer.connect(&snarl.out_pin(from), &snarl.in_pin(to), snarl);
        });
        assert!(editor.snarl.wires().any(|w| w == (from, to)));
        assert_eq!(wires(&editor), expected_wires(&editor));
    }

    #[test]
    fn a_refused_cable_draws_no_wire_and_says_why() {
        let mut editor = editor("bass");
        let out = block_of(&editor, "master");
        let (from, to) = (
            OutPinId {
                node: out,
                output: master::OUT,
            },
            InPinId {
                node: out,
                input: master::IN,
            },
        );
        with_viewer(&mut editor, |viewer, snarl| {
            viewer.connect(&snarl.out_pin(from), &snarl.in_pin(to), snarl);
            // The same cable twice is refused by the controller.
            viewer.connect(&snarl.out_pin(from), &snarl.in_pin(to), snarl);
        });
        assert_eq!(editor.status, "cannot connect: already connected");
        assert_eq!(wires(&editor), expected_wires(&editor));
    }

    #[test]
    fn removing_wires_disconnects_the_cables() {
        let mut editor = editor("chaos");
        let fm = InPinId {
            node: block_of(&editor, "osc 1"),
            input: oscillator::IN_FM,
        };
        let fm_wires = |e: &Editor| e.snarl.wires().filter(|(_, to)| *to == fm).count();
        // The chaos preset feeds osc 2 and the delay into osc 1's FM.
        assert_eq!(fm_wires(&editor), 2);
        with_viewer(&mut editor, |viewer, snarl| {
            viewer.drop_inputs(&snarl.in_pin(fm), snarl);
        });
        assert_eq!(fm_wires(&editor), 0);
        assert_eq!(wires(&editor), expected_wires(&editor));
    }

    #[test]
    fn wires_yanked_off_by_the_widget_disconnect_their_cables() {
        let mut editor = editor("chaos");
        let fm = InPinId {
            node: block_of(&editor, "osc 1"),
            input: oscillator::IN_FM,
        };
        let before = editor.controller.cables().len();
        // What egui-snarl does on Ctrl+drag from a pin, bypassing the viewer.
        editor.snarl.drop_inputs(fm);
        editor.disconnect_cables_without_wires();
        assert_eq!(editor.controller.cables().len(), before - 2);
        assert_eq!(wires(&editor), expected_wires(&editor));
    }

    #[test]
    fn loading_a_patch_redraws_wires_and_positions() {
        let mut editor = editor("chaos");
        let mut bass = presets::get("bass").unwrap();
        bass.positions.insert("osc 1".to_string(), [123.0, 456.0]);
        editor.load(&bass).unwrap();
        assert_eq!(wires(&editor), expected_wires(&editor));
        assert_eq!(editor.controller.cables().len(), bass.cables.len());
        let osc = block_of(&editor, "osc 1");
        let pos = editor.snarl.get_node_info(osc).unwrap().pos;
        assert_eq!([pos.x, pos.y], [123.0, 456.0]);
    }

    #[test]
    fn saved_patch_round_trips_with_positions() {
        let mut editor = editor("chaos");
        let saved = editor.patch();
        assert_eq!(saved.positions.len(), editor.controller.nodes().len());
        editor
            .load(&Patch::from_json(&saved.to_json()).unwrap())
            .unwrap();
        assert_eq!(editor.patch(), saved);
    }

    #[test]
    fn the_palette_adds_a_module_and_its_block_where_clicked() {
        let mut editor = editor("bass");
        let before = editor.controller.nodes().len();
        with_viewer(&mut editor, |viewer, snarl| {
            viewer.add("filter", egui::pos2(10.0, 20.0), snarl);
        });
        assert_eq!(editor.status, "added filter 2");
        assert_eq!(editor.controller.nodes().len(), before + 1);
        let pos = editor
            .snarl
            .get_node_info(block_of(&editor, "filter 2"))
            .unwrap()
            .pos;
        assert_eq!([pos.x, pos.y], [10.0, 20.0]);
        assert_eq!(editor.patch().positions["filter 2"], [10.0, 20.0]);
    }

    #[test]
    fn removing_a_block_removes_its_module_and_wires() {
        let mut editor = editor("chaos");
        let osc = block_of(&editor, "osc 1");
        with_viewer(&mut editor, |viewer, snarl| viewer.remove(osc, snarl));
        assert_eq!(editor.status, "removed osc 1");
        assert!(editor.controller.nodes().iter().all(|n| n.name != "osc 1"));
        assert_eq!(
            editor.snarl.nodes().count(),
            editor.controller.nodes().len()
        );
        assert_eq!(wires(&editor), expected_wires(&editor));
        // Still consistent on the next frame.
        editor.disconnect_cables_without_wires();
        assert_eq!(wires(&editor), expected_wires(&editor));
    }

    #[test]
    fn the_master_stays() {
        let mut editor = editor("bass");
        let out = block_of(&editor, "master");
        with_viewer(&mut editor, |viewer, snarl| viewer.remove(out, snarl));
        assert_eq!(
            editor.status,
            "cannot remove master: the output module cannot be removed"
        );
        assert!(editor.snarl.get_node_info(out).is_some());
    }

    #[test]
    fn loading_a_patch_with_other_modules_redraws_every_block() {
        let mut editor = editor("chaos");
        let small = Patch::from_json(
            r#"{ "version": 2, "modules": [
                { "name": "lfo", "type": "lfo" },
                { "name": "master", "type": "master" }
            ], "cables": [
                { "from": "lfo", "output": "sine", "to": "master", "input": "in" }
            ] }"#,
        )
        .unwrap();
        editor.load(&small).unwrap();
        assert_eq!(editor.snarl.nodes().count(), 2);
        assert_eq!(wires(&editor), expected_wires(&editor));
        assert_eq!(wires(&editor).len(), 1);
    }

    #[test]
    fn default_layout_keeps_columns_within_their_height() {
        let editor = editor("bass");
        let layout = default_layout(&editor.controller);
        assert!(layout.iter().all(|[_, y]| *y < COLUMN_HEIGHT));
        assert!(layout.windows(2).all(|w| w[1][0] >= w[0][0]));
        assert!(layout.last().unwrap()[0] > 0.0, "everything in one column");
    }

    #[test]
    fn renders_without_a_window() {
        let mut editor = editor("chaos");
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| editor.show_graph(ui));
            // No renderer consumes the font textures here, and egui insists
            // they are dropped explicitly.
            output.textures_delta.clear();
        }
        assert!(editor.status.is_empty(), "{}", editor.status);
    }
}
