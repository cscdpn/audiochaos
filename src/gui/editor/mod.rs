//! The node editor: one block per module, wires for cables. This file holds
//! the editor's state and the edits it makes on whole groups of blocks;
//! `viewer` draws single blocks and handles single edits, `widgets` the
//! controls inside blocks, `layout` where unplaced blocks go.

mod layout;
mod viewer;
mod widgets;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::time::Duration;

use egui::emath::TSTransform;
use egui::{Pos2, Rect, Ui, Vec2};
use egui_snarl::ui::{PinPlacement, SnarlStyle, SnarlWidget};
use egui_snarl::{InPinId, NodeId as BlockId, OutPinId, Snarl};

use crate::engine::control::Controller;
use crate::engine::graph::{Cable, NodeId};
use crate::modules::Context;
use crate::modules::clock::Tap;
use crate::patch::Patch;

use layout::{BLOCK_GAP, columns, default_layout, estimated_size};
use viewer::Viewer;
use widgets::marked;

/// How often to redraw without input, to keep the status current.
const REFRESH: Duration = Duration::from_millis(250);
/// How often to redraw while lights blink or ticks move: fast enough to
/// follow a beat.
const LIVE_REFRESH: Duration = Duration::from_millis(33);

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
    /// Each block's size as last drawn, to arrange them.
    sizes: HashMap<BlockId, Vec2>,
    /// Set by `arrange`, so the next frame brings the blocks into view.
    show_origin: bool,
    /// Each clock's Tap button.
    taps: HashMap<NodeId, Tap>,
    /// From graph to screen coordinates, as last drawn.
    to_global: TSTransform,
    /// Where the graph is drawn, on screen.
    view: Rect,
    /// Blocks to copy, asked for from a block's menu while drawing.
    to_duplicate: Option<Vec<BlockId>>,
    /// The last thing worth telling the user, for the status bar.
    pub status: String,
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
            sizes: HashMap::new(),
            show_origin: false,
            taps: HashMap::new(),
            to_global: TSTransform::IDENTITY,
            view: Rect::ZERO,
            to_duplicate: None,
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
        self.sizes.clear();
        for (node, default) in self.controller.nodes().iter().zip(layout) {
            let pos = positions.get(&node.name).map(|&[x, y]| egui::pos2(x, y));
            self.snarl.insert_node(pos.unwrap_or(default), node.id);
        }
        for &cable in self.controller.cables() {
            let (from, to) = wire(&self.snarl, cable);
            self.snarl.connect(from, to);
        }
    }

    /// Moves every block into columns where none overlaps another, the
    /// master last, and brings them into view.
    pub fn arrange(&mut self) {
        let output = self.controller.output();
        let mut blocks: Vec<_> = self
            .controller
            .nodes()
            .iter()
            .map(|n| (block(&self.snarl, n.id), n.id == output, n.info))
            .collect();
        // Stable, so the others keep their processing order.
        blocks.sort_by_key(|&(_, is_output, _)| is_output);
        let sizes = blocks.iter().map(|&(block, _, info)| {
            // Blocks not drawn yet, if any, are guessed.
            self.sizes
                .get(&block)
                .copied()
                .unwrap_or_else(|| estimated_size(info))
        });
        let layout = columns(sizes);
        for (&(block, _, _), pos) in blocks.iter().zip(layout) {
            if let Some(node) = self.snarl.get_node_info_mut(block) {
                node.pos = pos;
            }
        }
        self.show_origin = true;
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

    /// Edits another engine, playing the same patch: blocks stay in place.
    pub fn replace_engine(&mut self, controller: Controller, context: Context) {
        let positions = self.patch().positions;
        self.controller = controller;
        self.context = context;
        // Tempos tapped on the old clocks are gone with them.
        self.taps.clear();
        self.rebuild(&positions);
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

    /// How soon to redraw, input or not.
    pub fn refresh(&self) -> Duration {
        if self.moves() { LIVE_REFRESH } else { REFRESH }
    }

    /// Whether some block shows a light or a modulation tick, which need
    /// frequent redraws to follow the sound.
    pub fn moves(&self) -> bool {
        self.controller.nodes().iter().any(|n| {
            self.controller.led(n.id).is_some()
                || n.info.params.iter().enumerate().any(|(p, param)| {
                    marked(param, n.params[p], self.controller.modulated(n.id, p)).is_some()
                })
        })
    }

    /// The selected blocks: egui-snarl's selection, made with Shift.
    pub fn selection(&self, ctx: &egui::Context) -> Vec<BlockId> {
        egui_snarl::ui::get_selected_nodes(snarl_id(), ctx)
            .into_iter()
            .filter(|&b| self.snarl.get_node(b).is_some())
            .collect()
    }

    /// `blocks` as a group of modules with the cables between them, to copy
    /// or save; `None` when that leaves nothing, the master staying out.
    pub fn group(&self, blocks: &[BlockId]) -> Option<Patch> {
        let names: Vec<&str> = blocks
            .iter()
            .map(|&b| self.controller.node(self.snarl[b]).name.as_str())
            .collect();
        let group = self.patch().group(&names);
        (!group.modules.is_empty()).then_some(group)
    }

    /// Copies `blocks`, knobs and the cables between them included, just
    /// below them.
    pub fn duplicate(&mut self, blocks: &[BlockId]) {
        let Some(group) = self.group(blocks) else {
            self.status = NOTHING_SELECTED.to_string();
            return;
        };
        let rects: Vec<Rect> = blocks
            .iter()
            .filter_map(|&b| {
                let pos = self.snarl.get_node_info(b)?.pos;
                Some(Rect::from_min_size(pos, self.size(b)))
            })
            .collect();
        let top = rects.iter().map(|r| r.min.y).fold(f32::INFINITY, f32::min);
        let bottom = rects
            .iter()
            .map(|r| r.max.y)
            .fold(f32::NEG_INFINITY, f32::max);
        self.insert(
            &group,
            Vec2::new(0.0, bottom - top + BLOCK_GAP),
            "duplicated",
        );
    }

    /// Adds the modules of a group, or of a whole patch but its master, to
    /// the playing patch, in the middle of the view.
    pub fn paste(&mut self, group: &Patch) {
        let corner = group
            .positions
            .values()
            .map(|&[x, y]| egui::pos2(x, y))
            .reduce(Pos2::min)
            .unwrap_or(Pos2::ZERO);
        let middle = self.to_global.inverse() * self.view.center();
        self.insert(group, middle - corner, "added");
    }

    /// Adds `group`'s modules and blocks, moved by `offset` from where they
    /// were in it.
    fn insert(&mut self, group: &Patch, offset: Vec2, done: &str) {
        let added = match group.insert_into(&mut self.controller, &self.context) {
            Ok(added) => added,
            Err(e) => {
                self.status = format!("cannot add the blocks: {e}");
                return;
            }
        };
        // Blocks a hand-written group places nowhere go in columns.
        let infos = added
            .iter()
            .map(|&(_, id)| estimated_size(self.controller.node(id).info));
        for ((name, id), default) in added.iter().zip(columns(infos)) {
            let pos = group.positions.get(name).map(|&[x, y]| egui::pos2(x, y));
            self.snarl.insert_node(pos.unwrap_or(default) + offset, *id);
        }
        let ids: Vec<NodeId> = added.iter().map(|&(_, id)| id).collect();
        for &cable in self.controller.cables() {
            if ids.contains(&cable.from) {
                let (from, to) = wire(&self.snarl, cable);
                self.snarl.connect(from, to);
            }
        }
        self.status = match added.len() {
            1 => format!("{done} {}", self.controller.node(ids[0]).name),
            n => format!("{done} {n} blocks"),
        };
    }

    fn size(&self, block: BlockId) -> Vec2 {
        self.sizes
            .get(&block)
            .copied()
            .unwrap_or_else(|| estimated_size(self.controller.node(self.snarl[block]).info))
    }

    /// Ctrl+C, Ctrl+V and Ctrl+D on the selected blocks, through the system
    /// clipboard as patch JSON, so blocks also go from one window to another.
    fn handle_shortcuts(&mut self, ui: &mut Ui) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::Copy => match self.group(&self.selection(ui.ctx())) {
                    Some(group) => {
                        ui.ctx().copy_text(group.to_json());
                        self.status = format!("copied {} blocks", group.modules.len());
                    }
                    None => self.status = NOTHING_SELECTED.to_string(),
                },
                egui::Event::Paste(text) => match Patch::from_json(&text) {
                    Ok(group) => self.paste(&group),
                    Err(_) => self.status = "the clipboard holds no blocks".to_string(),
                },
                egui::Event::Key {
                    key: egui::Key::D,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } if modifiers.command => self.duplicate(&self.selection(ui.ctx())),
                _ => {}
            }
        }
    }

    pub fn show_graph(&mut self, ui: &mut Ui) {
        self.view = ui.max_rect();
        self.handle_shortcuts(ui);
        let mut viewer = Viewer {
            controller: &mut self.controller,
            context: &self.context,
            status: &mut self.status,
            sizes: &mut self.sizes,
            show_origin: &mut self.show_origin,
            taps: &mut self.taps,
            to_global: &mut self.to_global,
            to_duplicate: &mut self.to_duplicate,
            view: self.view,
        };
        SnarlWidget::new()
            .id(snarl_id())
            .style(SnarlStyle {
                // Inside the block, the default, egui-snarl leaves no room for
                // the pins of right-aligned outputs: labels run into them.
                pin_placement: Some(PinPlacement::Edge),
                ..SnarlStyle::new()
            })
            .show(&mut self.snarl, &mut viewer, ui);
        self.disconnect_cables_without_wires();
        if let Some(blocks) = self.to_duplicate.take() {
            self.duplicate(&blocks);
        }
    }
}

const NOTHING_SELECTED: &str = "select blocks first: Shift+click, or Shift+drag around them";

/// The node editor widget's, to read its selection.
fn snarl_id() -> egui::Id {
    egui::Id::new("patch")
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

#[cfg(test)]
mod tests;
