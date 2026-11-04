//! Where blocks go when nobody placed them: in columns, in processing order.

use egui::{Pos2, Vec2};

use crate::engine::control::Controller;
use crate::engine::module::ModuleInfo;

/// Layout: blocks stacked in columns, in processing order (sources first,
/// master last), starting a new column past this height.
pub(super) const COLUMN_HEIGHT: f32 = 900.0;
const COLUMN_WIDTH: f32 = 460.0;
const ROW_HEIGHT: f32 = 24.0;
pub(super) const BLOCK_GAP: f32 = 50.0;

/// Columns of blocks in the controller's module order, before any is drawn:
/// their sizes are guessed.
pub(super) fn default_layout(controller: &Controller) -> Vec<Pos2> {
    columns(controller.nodes().iter().map(|n| estimated_size(n.info)))
}

/// A block's size from its pin count, for blocks never drawn.
pub(super) fn estimated_size(info: &ModuleInfo) -> Vec2 {
    let rows = info.num_input_ports().max(info.outputs.len());
    egui::vec2(COLUMN_WIDTH - BLOCK_GAP, ROW_HEIGHT * (rows + 1) as f32)
}

/// Where blocks of these sizes go, stacked top to bottom in columns as wide
/// as their widest block.
pub(super) fn columns(sizes: impl IntoIterator<Item = Vec2>) -> Vec<Pos2> {
    let (mut x, mut y, mut width) = (0.0, 0.0, 0.0_f32);
    sizes
        .into_iter()
        .map(|size| {
            if y > 0.0 && y + size.y > COLUMN_HEIGHT {
                (x, y, width) = (x + width + BLOCK_GAP, 0.0, 0.0);
            }
            let position = egui::pos2(x, y);
            y += size.y + BLOCK_GAP;
            width = width.max(size.x);
            position
        })
        .collect()
}
