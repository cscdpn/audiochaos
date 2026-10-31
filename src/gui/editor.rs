use std::collections::{BTreeMap, HashSet};
use std::error::Error;

use eframe::egui::{self, Color32, Pos2, Ui};
use egui_snarl::ui::{PinInfo, PinPlacement, SnarlPin, SnarlStyle, SnarlViewer, SnarlWidget};
use egui_snarl::{InPin, InPinId, NodeId as BlockId, OutPin, OutPinId, Snarl};

use crate::engine::control::{Controller, SampleState};
use crate::engine::graph::{Cable, NodeId};
use crate::engine::param::{ParamInfo, Scale};
use crate::engine::sample::Sample;
use crate::modules::{self, Context, master, sampler};
use crate::patch::{self, Patch};

const AUDIO_PIN: Color32 = Color32::from_rgb(0xe0, 0x6c, 0x3c);
const CV_PIN: Color32 = Color32::from_rgb(0x4c, 0x9b, 0xd6);
const SLIDER_WIDTH: f32 = 150.0;

/// Default layout: blocks stacked in columns, in processing order (sources
/// first, master last in the presets), starting a new column past this
/// height.
const COLUMN_HEIGHT: f32 = 900.0;
const COLUMN_WIDTH: f32 = 460.0;
const ROW_HEIGHT: f32 = 24.0;
const BLOCK_GAP: f32 = 50.0;

/// The node editor: one block per module, wires for cables.
///
/// The controller is the source of truth. The snarl (egui-snarl's graph) is
/// a view of it plus block positions; a block or wire is added to or removed
/// from the snarl only once the controller has accepted the change.
pub struct Editor {
    pub(super) controller: Controller,
    /// For building the modules of loaded patches and new blocks.
    context: Context,
    /// Each block holds the id of its module.
    snarl: Snarl<NodeId>,
    pub(super) status: String,
}

impl Editor {
    pub fn new(
        controller: Controller,
        context: Context,
        positions: &BTreeMap<String, [f32; 2]>,
    ) -> Self {
        let mut editor = Self {
            controller,
            context,
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

    /// Replaces the playing patch, block positions included. Returns what went
    /// wrong with its sound files, which leaves their samplers silent.
    pub fn load(&mut self, patch: &Patch) -> Result<Vec<String>, Box<dyn Error>> {
        let problems = patch.load_into(&mut self.controller, &self.context)?;
        self.rebuild(&patch.positions);
        Ok(problems)
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
            context: &self.context,
            status: &mut self.status,
        };
        SnarlWidget::new()
            .id(egui::Id::new("patch"))
            .style(SnarlStyle {
                // Inside the block, the default, egui-snarl leaves no room for
                // the pins of right-aligned outputs: labels run into them.
                pin_placement: Some(PinPlacement::Edge),
                ..SnarlStyle::new()
            })
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

/// A slider whose right-click menu resets it. Returns whether `value` changed.
fn knob(param: &ParamInfo, value: &mut f32, ui: &mut Ui) -> bool {
    let slider = egui::Slider::new(value, param.min..=param.max)
        .text(param.name)
        .logarithmic(param.scale == Scale::Exponential)
        .custom_formatter(|v, _| param.format(v as f32));
    // Sliders only sense drags, and a menu opens on a click.
    let response = ui.add(slider);
    let mut changed = response.changed();
    response.interact(egui::Sense::click()).context_menu(|ui| {
        for (label, reset) in param.resets() {
            if ui.button(label).clicked() {
                *value = reset;
                changed = true;
                ui.close();
            }
        }
    });
    changed
}

/// Draws blocks for egui-snarl and turns edits into controller calls.
struct Viewer<'a> {
    controller: &'a mut Controller,
    context: &'a Context,
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
    fn load_sample(&mut self, id: NodeId, path: &str) {
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
        // Laid out from the pin leftwards.
        ui.label(info.outputs[pin.id.output]);
        ui.weak(info.output_details[pin.id.output]);
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
        let sample = node.sample.clone();
        ui.spacing_mut().slider_width = SLIDER_WIDTH;
        ui.vertical(|ui| {
            for (p, (param, value)) in info.params.iter().zip(&mut values).enumerate() {
                if knob(param, value, ui)
                    && let Err(e) = self.controller.set_param(id, p, *value)
                {
                    *self.status = format!("knob not changed: {e}");
                }
            }
            if info.name == sampler::INFO.name {
                self.show_sample_button(id, sample.as_ref(), ui);
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
    use crate::modules::{delay, filter, master, oscillator};
    use crate::presets;

    fn editor(preset: &str) -> Editor {
        let patch = presets::get(preset).unwrap();
        let (graph, out) = patch.build(&Context::new(48_000.0)).unwrap();
        // Large enough for every command these tests send without an engine
        // draining the queue.
        let (_, controller) = Engine::new(graph, out, 1024);
        Editor::new(controller, Context::new(48_000.0), &patch.positions)
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
            context: &editor.context,
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
    fn samplers_take_a_sound_from_a_file_or_say_why_not() {
        let mut editor = editor("bass");
        with_viewer(&mut editor, |viewer, snarl| {
            viewer.add("sampler", egui::pos2(0.0, 0.0), snarl);
        });
        let id = module(&editor, "sampler");

        with_viewer(&mut editor, |viewer, _| {
            viewer.load_sample(id, "/nowhere/kick.wav");
        });
        assert!(
            editor.status.starts_with("cannot load kick.wav: "),
            "{}",
            editor.status
        );
        assert!(editor.controller.node(id).sample.is_none());

        let path =
            std::env::temp_dir().join(format!("audiochaos-editor-{}.wav", std::process::id()));
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav = hound::WavWriter::create(&path, spec).unwrap();
        wav.write_sample(1000_i16).unwrap();
        wav.finalize().unwrap();
        let path = path.to_str().unwrap();
        with_viewer(&mut editor, |viewer, _| viewer.load_sample(id, path));
        assert!(editor.status.starts_with("loaded audiochaos-editor-"));
        let saved = editor.patch();
        let entry = saved.modules.iter().find(|m| m.kind == "sampler").unwrap();
        assert_eq!(entry.file.as_deref(), Some(path));
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
    fn right_clicking_a_knob_opens_its_menu() {
        let param = &filter::INFO.params[filter::CUTOFF];
        let mut value = param.default;
        let ctx = egui::Context::default();
        // Returns where the knob is and whether a menu is open.
        let mut frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut seen = (egui::Rect::NOTHING, false);
            let mut output = ctx.run_ui(input, |ui| {
                knob(param, &mut value, ui);
                seen = (ui.min_rect(), egui::Popup::is_any_open(ui.ctx()));
            });
            output.textures_delta.clear();
            seen
        };
        let (slider, opened) = frame(vec![]);
        assert!(!opened);
        let pos = egui::pos2(slider.left() + 20.0, slider.center().y);
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(pos)]);
        frame(vec![button(true)]);
        frame(vec![button(false)]);
        assert!(frame(vec![]).1);
    }

    /// One frame of the editor as a window would draw it, flattened into what
    /// gets painted.
    fn paint(
        editor: &mut Editor,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> Vec<egui::Shape> {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| editor.show_graph(ui));
        // No renderer consumes the font textures here, and egui insists they
        // are dropped explicitly.
        output.textures_delta.clear();
        let mut shapes = Vec::new();
        fn flatten(shape: egui::Shape, into: &mut Vec<egui::Shape>) {
            match shape {
                egui::Shape::Vec(inner) => inner.into_iter().for_each(|s| flatten(s, into)),
                other => into.push(other),
            }
        }
        for clipped in output.shapes {
            flatten(clipped.shape, &mut shapes);
        }
        shapes
    }

    fn texts(shapes: &[egui::Shape]) -> Vec<(String, egui::Rect)> {
        shapes
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Text(t) if !t.galley.text().is_empty() => {
                    Some((t.galley.text().to_string(), t.visual_bounding_rect()))
                }
                _ => None,
            })
            .collect()
    }

    fn text_at(shapes: &[egui::Shape], text: &str) -> egui::Rect {
        texts(shapes)
            .into_iter()
            .find(|(t, _)| t == text)
            .unwrap_or_else(|| panic!("{text:?} is not on screen"))
            .1
    }

    fn click(pos: Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn no_label_runs_into_a_pin_or_another_label() {
        for preset in presets::names() {
            let mut editor = editor(preset);
            let ctx = egui::Context::default();
            paint(&mut editor, &ctx, vec![]);
            let shapes = paint(&mut editor, &ctx, vec![]);
            let pins: Vec<_> = shapes
                .iter()
                .filter_map(|shape| match shape {
                    egui::Shape::Circle(c) if c.fill == AUDIO_PIN || c.fill == CV_PIN => {
                        Some(shape.visual_bounding_rect())
                    }
                    _ => None,
                })
                .collect();
            assert!(!pins.is_empty());
            let texts = texts(&shapes);
            for (i, (text, rect)) in texts.iter().enumerate() {
                assert!(
                    pins.iter().all(|pin| !pin.intersects(*rect)),
                    "{preset}: {text:?} touches a pin"
                );
                for (other, other_rect) in &texts[i + 1..] {
                    assert!(
                        !rect.intersects(*other_rect),
                        "{preset}: {text:?} touches {other:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_menu_of_a_knob_resets_it_in_the_playing_patch() {
        use egui::PointerButton::{Primary, Secondary};

        let mut editor = editor("keys");
        let delay = module(&editor, "delay");
        let param = &delay::INFO.params[delay::FEEDBACK];
        assert_ne!(
            editor.controller.node(delay).params[delay::FEEDBACK],
            param.min
        );

        let ctx = egui::Context::default();
        paint(&mut editor, &ctx, vec![]);
        let shapes = paint(&mut editor, &ctx, vec![]);
        // The slider lies left of its value and name.
        let name = text_at(&shapes, param.name);
        let on_slider = egui::pos2(name.left() - 100.0, name.center().y);
        paint(
            &mut editor,
            &ctx,
            vec![egui::Event::PointerMoved(on_slider)],
        );
        paint(&mut editor, &ctx, vec![click(on_slider, Secondary, true)]);
        paint(&mut editor, &ctx, vec![click(on_slider, Secondary, false)]);
        let shapes = paint(&mut editor, &ctx, vec![]);

        let (label, _) = param.resets()[0];
        let item = text_at(&shapes, label).center();
        paint(&mut editor, &ctx, vec![egui::Event::PointerMoved(item)]);
        paint(&mut editor, &ctx, vec![click(item, Primary, true)]);
        paint(&mut editor, &ctx, vec![click(item, Primary, false)]);

        assert_eq!(
            editor.controller.node(delay).params[delay::FEEDBACK],
            param.min
        );
        assert!(editor.status.is_empty(), "{}", editor.status);
    }

    #[test]
    fn renders_without_a_window() {
        let mut editor = editor("chaos");
        let ctx = egui::Context::default();
        for _ in 0..2 {
            paint(&mut editor, &ctx, vec![]);
        }
        assert!(editor.status.is_empty(), "{}", editor.status);
    }
}
