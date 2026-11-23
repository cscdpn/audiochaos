//! Where blocks go: in columns, in processing order, when nobody placed them;
//! following the signal when the user arranges them; on a grid when they
//! snap.

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

/// How far apart snapped blocks are, and the background grid's spacing.
pub(super) const GRID: f32 = 25.0;

/// The grid point nearest to `pos`.
pub(super) fn snapped(pos: Pos2) -> Pos2 {
    (pos.to_vec2() / GRID).round().to_pos2() * GRID
}

/// Rounds of reordering the columns; more change little.
const SWEEPS: usize = 12;

/// Where blocks of these `sizes`, in processing order, go when arranged by
/// signal flow: each in a column right of the blocks that feed it, the
/// `output` alone in the last, and within columns in the order that brings
/// each block level with its neighbours, which uncrosses most wires.
///
/// `cables` join block indices. One from a block processed later is
/// feedback and does not move its destination right, as it does not in the
/// engine either.
pub(super) fn flow(sizes: &[Vec2], cables: &[(usize, usize)], output: usize) -> Vec<Pos2> {
    let n = sizes.len();
    let forward: Vec<(usize, usize)> = cables.iter().copied().filter(|&(a, b)| a < b).collect();
    // Each block one column past the furthest that feeds it, in processing
    // order, so feeders are placed first.
    let mut column = vec![0; n];
    for b in 0..n {
        for &(a, _) in forward.iter().filter(|&&(_, to)| to == b) {
            column[b] = column[b].max(column[a] + 1);
        }
    }
    // Blocks fed by nothing, as LFOs, go just before what they feed rather
    // than all the way left.
    for a in (0..n).rev() {
        let fed = forward.iter().any(|&(_, to)| to == a);
        let first_fed = forward
            .iter()
            .filter(|&&(from, _)| from == a)
            .map(|&(_, to)| column[to])
            .min();
        if let (false, Some(first)) = (fed, first_fed) {
            column[a] = first.max(1) - 1;
        }
    }
    if output < n {
        column[output] = (0..n)
            .filter(|&i| i != output)
            .map(|i| column[i] + 1)
            .max()
            .unwrap_or(0);
    }
    let columns = column.iter().max().map_or(0, |&c| c + 1);
    let mut order: Vec<Vec<usize>> = vec![Vec::new(); columns];
    for (i, &c) in column.iter().enumerate() {
        order[c].push(i);
    }
    let neighbours: Vec<Vec<usize>> = (0..n)
        .map(|i| {
            cables
                .iter()
                .filter_map(|&(a, b)| match (a == i, b == i) {
                    (true, false) => Some(b),
                    (false, true) => Some(a),
                    _ => None,
                })
                .collect()
        })
        .collect();
    let mut y = vec![0.0; n];
    for blocks in &order {
        restack(blocks, sizes, &mut y);
    }
    for _ in 0..SWEEPS {
        // Each column moves to its neighbours as they now stand, so two
        // columns never swap past each other.
        for blocks in &mut order {
            let centre = |i: usize| y[i] + sizes[i].y / 2.0;
            let keys: Vec<(usize, f32)> = blocks
                .iter()
                .map(|&i| {
                    let near = &neighbours[i];
                    let key = if near.is_empty() {
                        centre(i)
                    } else {
                        near.iter().map(|&j| centre(j)).sum::<f32>() / near.len() as f32
                    };
                    (i, key)
                })
                .collect();
            let mut sorted = keys;
            sorted.sort_by(|a, b| a.1.total_cmp(&b.1));
            *blocks = sorted.into_iter().map(|(i, _)| i).collect();
            restack(blocks, sizes, &mut y);
        }
    }
    let mut x = 0.0;
    let mut positions = vec![Pos2::ZERO; n];
    for blocks in &order {
        for &i in blocks {
            positions[i] = egui::pos2(x, y[i]);
        }
        let width = blocks.iter().map(|&i| sizes[i].x).fold(0.0, f32::max);
        x += width + BLOCK_GAP;
    }
    positions
}

/// Sets the top of each of a column's `blocks`, stacked in their order.
fn restack(blocks: &[usize], sizes: &[Vec2], y: &mut [f32]) {
    let mut top = 0.0;
    for &i in blocks {
        y[i] = top;
        top += sizes[i].y + BLOCK_GAP;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: Vec2 = egui::vec2(200.0, 100.0);

    /// Pairs of cables whose wires cross: going between the same columns,
    /// one starts above the other and ends below it.
    fn crossings(positions: &[Pos2], cables: &[(usize, usize)]) -> usize {
        let mut count = 0;
        for (k, &(a, b)) in cables.iter().enumerate() {
            for &(c, d) in &cables[k + 1..] {
                let same_columns =
                    positions[a].x == positions[c].x && positions[b].x == positions[d].x;
                let above = |p: usize, q: usize| positions[p].y < positions[q].y;
                if same_columns && above(a, c) != above(b, d) && a != c && b != d {
                    count += 1;
                }
            }
        }
        count
    }

    #[test]
    fn blocks_go_right_of_what_feeds_them_and_the_output_last() {
        // 0 → 1 → 3 (output), 2 → 3, and feedback from 3 into 1.
        let cables = [(0, 1), (1, 3), (2, 3), (3, 1)];
        let at = flow(&[SIZE; 4], &cables, 3);
        assert!(at[0].x < at[1].x && at[1].x < at[3].x);
        assert!(at[2].x < at[3].x);
        // Fed by nothing, it sits just before what it feeds.
        assert_eq!(at[2].x, at[1].x);
    }

    #[test]
    fn columns_are_ordered_to_uncross_wires() {
        // Two chains, processed in an order that crosses them in columns.
        let cables = [(0, 3), (1, 2), (2, 4), (3, 4)];
        let at = flow(&[SIZE; 5], &cables, 4);
        assert_eq!(crossings(&at, &cables), 0);
        assert_eq!(at[0].y, at[3].y, "a chain runs level");
    }

    #[test]
    fn snapping_goes_to_the_nearest_grid_point() {
        assert_eq!(snapped(egui::pos2(12.0, 13.0)), egui::pos2(0.0, 25.0));
        assert_eq!(snapped(egui::pos2(-40.0, 260.0)), egui::pos2(-50.0, 250.0));
    }
}
