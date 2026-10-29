use std::collections::{BTreeMap, HashSet};
use std::error::Error;

use eframe::egui::{self, Color32, Ui};
use egui_snarl::ui::{PinInfo, SnarlPin, SnarlStyle, SnarlViewer, SnarlWidget};
use egui_snarl::{InPin, InPinId, NodeId as BlockId, OutPin, OutPinId, Snarl};

use crate::engine::control::Controller;
use crate::engine::graph::Cable;
use crate::engine::param::Scale;
use crate::patch::Patch;

const AUDIO_PIN: Color32 = Color32::from_rgb(0xe0, 0x6c, 0x3c);
const CV_PIN: Color32 = Color32::from_rgb(0x4c, 0x9b, 0xd6);
const SLIDER_WIDTH: f32 = 150.0;

/// Default layout: blocks stacked in columns, in rack order (sources on the
/// left, master on the right), starting a new column past this height.
const COLUMN_HEIGHT: f32 = 900.0;
const COLUMN_WIDTH: f32 = 330.0;
const ROW_HEIGHT: f32 = 24.0;
const BLOCK_GAP: f32 = 50.0;

/// The node editor: one block per module, wires for cables.
///
/// The controller is the source of truth. The snarl (egui-snarl's graph) is
/// a view of it plus block positions; a wire is added to or removed from the
/// snarl only once the controller has accepted the change.
pub struct Editor {
    pub(super) controller: Controller,
    /// Each block's value is the index of its module in the controller.
    snarl: Snarl<usize>,
    /// Module index to block.
    blocks: Vec<BlockId>,
    pub(super) status: String,
}

impl Editor {
    pub fn new(controller: Controller, positions: &BTreeMap<String, [f32; 2]>) -> Self {
        let mut editor = Self {
            controller,
            snarl: Snarl::new(),
            blocks: Vec::new(),
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
        self.blocks = self
            .controller
            .nodes()
            .iter()
            .enumerate()
            .map(|(index, node)| {
                let [x, y] = positions.get(&node.name).copied().unwrap_or(layout[index]);
                self.snarl.insert_node(egui::pos2(x, y), index)
            })
            .collect();
        for &cable in self.controller.cables() {
            let (from, to) = self.wire(cable);
            self.snarl.connect(from, to);
        }
    }

    /// The pins a cable is drawn between.
    fn wire(&self, cable: Cable) -> (OutPinId, InPinId) {
        let from = OutPinId {
            node: self.blocks[cable.from.0],
            output: cable.from_port,
        };
        let to = InPinId {
            node: self.blocks[cable.to.0],
            input: cable.to_port,
        };
        (from, to)
    }

    /// egui-snarl removes wires on its own, without asking the viewer, when
    /// the user yanks them off a pin with Ctrl+drag and drops them on nothing.
    /// Cables whose wire is gone are disconnected, so what plays is always
    /// what is drawn.
    fn disconnect_cables_without_wires(&mut self) {
        let drawn: HashSet<_> = self.snarl.wires().collect();
        for cable in self.controller.cables().to_vec() {
            let (from, to) = self.wire(cable);
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
            .map(|(pos, &index)| (self.controller.nodes()[index].name.clone(), [pos.x, pos.y]))
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

/// Columns of blocks, sized from each block's pin count.
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

/// Draws blocks for egui-snarl and turns wire edits into controller calls.
struct Viewer<'a> {
    controller: &'a mut Controller,
    status: &'a mut String,
}

impl Viewer<'_> {
    fn cable(&self, snarl: &Snarl<usize>, from: OutPinId, to: InPinId) -> Cable {
        let nodes = self.controller.nodes();
        Cable {
            from: nodes[snarl[from.node]].id,
            from_port: from.output,
            to: nodes[snarl[to.node]].id,
            to_port: to.input,
        }
    }

    fn unplug(&mut self, from: OutPinId, to: InPinId, snarl: &mut Snarl<usize>) {
        match self.controller.disconnect(self.cable(snarl, from, to)) {
            Ok(_) => {
                snarl.disconnect(from, to);
            }
            Err(e) => *self.status = format!("cannot disconnect: {e}"),
        }
    }
}

impl SnarlViewer<usize> for Viewer<'_> {
    fn title(&mut self, node: &usize) -> String {
        self.controller.nodes()[*node].name.clone()
    }

    fn inputs(&mut self, node: &usize) -> usize {
        self.controller.nodes()[*node].info.num_input_ports()
    }

    fn outputs(&mut self, node: &usize) -> usize {
        self.controller.nodes()[*node].info.outputs.len()
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut Ui,
        snarl: &mut Snarl<usize>,
    ) -> impl SnarlPin + 'static {
        let info = self.controller.nodes()[snarl[pin.id.node]].info;
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
        snarl: &mut Snarl<usize>,
    ) -> impl SnarlPin + 'static {
        let info = self.controller.nodes()[snarl[pin.id.node]].info;
        ui.label(info.outputs[pin.id.output]);
        PinInfo::circle().with_fill(AUDIO_PIN)
    }

    fn has_body(&mut self, node: &usize) -> bool {
        !self.controller.nodes()[*node].info.params.is_empty()
    }

    fn show_body(
        &mut self,
        block: BlockId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<usize>,
    ) {
        let node = &self.controller.nodes()[snarl[block]];
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

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<usize>) {
        match self.controller.connect(self.cable(snarl, from.id, to.id)) {
            Ok(()) => {
                snarl.connect(from.id, to.id);
            }
            Err(e) => *self.status = format!("cannot connect: {e}"),
        }
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<usize>) {
        self.unplug(from.id, to.id, snarl);
    }

    fn drop_outputs(&mut self, pin: &OutPin, snarl: &mut Snarl<usize>) {
        for &to in &pin.remotes {
            self.unplug(pin.id, to, snarl);
        }
    }

    fn drop_inputs(&mut self, pin: &InPin, snarl: &mut Snarl<usize>) {
        for &from in &pin.remotes {
            self.unplug(from, pin.id, snarl);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::control::Engine;
    use crate::modules::{filter, master, oscillator};
    use crate::rack;

    fn editor(preset: &str) -> Editor {
        let patch = rack::preset(preset).unwrap();
        let (graph, out) = rack::build(48_000.0, &patch).unwrap();
        // Large enough for every command these tests send without an engine
        // draining the queue.
        let (_, controller) = Engine::new(graph, out, 1024);
        Editor::new(controller, &patch.positions)
    }

    fn module(editor: &Editor, name: &str) -> usize {
        let nodes = editor.controller.nodes();
        nodes.iter().position(|n| n.name == name).unwrap()
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
            .map(|&c| editor.wire(c))
            .collect();
        wires.sort();
        wires
    }

    /// Runs `f` with a viewer, as egui-snarl would when the user edits wires.
    fn with_viewer(editor: &mut Editor, f: impl FnOnce(&mut Viewer, &mut Snarl<usize>)) {
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
                node: editor.blocks[module(editor, "osc 1")],
                output: oscillator::OUT_SAW,
            },
            InPinId {
                node: editor.blocks[module(editor, "filter")],
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
        let out = editor.blocks[module(&editor, "master")];
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
            node: editor.blocks[module(&editor, "osc 1")],
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
            node: editor.blocks[module(&editor, "osc 1")],
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
        let mut bass = rack::preset("bass").unwrap();
        bass.positions.insert("osc 1".to_string(), [123.0, 456.0]);
        editor.load(&bass).unwrap();
        assert_eq!(wires(&editor), expected_wires(&editor));
        assert_eq!(editor.controller.cables().len(), bass.cables.len());
        let osc = editor.blocks[module(&editor, "osc 1")];
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
