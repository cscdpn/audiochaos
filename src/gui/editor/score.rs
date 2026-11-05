//! Editors for scores, drawn in their blocks: a song's sections and tracks
//! as a grid of cells, a `notes` module's patterns as a piano roll.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::engine::score::{
    self, Arrangement, Key, MAX_BARS, MAX_SECTIONS, MAX_STEPS, PATTERNS, Patterns, SCALES, Section,
    Step, TRACKS,
};

/// A song cell's width: a section's column.
const CELL_WIDTH: f32 = 36.0;
/// A piano roll step's width and a note's height.
pub(super) const STEP_WIDTH: f32 = 14.0;
const NOTE_HEIGHT: f32 = 10.0;
/// Room for note names left of the grid.
const LABEL_WIDTH: f32 = 26.0;
/// Steps shown at once; longer patterns turn pages.
const PAGE: usize = 16;

/// Where a block's piano roll is looking: editor state, not saved.
#[derive(Default)]
pub(super) struct NoteView {
    pattern: usize,
    /// The lowest note shown, once known.
    low: Option<u8>,
    page: usize,
    /// The note a drag started on, which it holds on.
    anchor: Option<(usize, u8)>,
}

/// A cell clicked: silent, then patterns A to D, then silent again.
fn next_cell(cell: Option<u8>) -> Option<u8> {
    match cell {
        None => Some(0),
        Some(p) if usize::from(p) + 1 < score::PATTERNS => Some(p + 1),
        Some(_) => None,
    }
}

/// Edits a song's arrangement. `tracks` names each track by what its output
/// feeds. Returns whether anything changed.
pub(super) fn arrangement(arrangement: &mut Arrangement, tracks: &[String], ui: &mut Ui) -> bool {
    let mut changed = false;
    egui::Grid::new(ui.id().with("arrangement"))
        .spacing([2.0, 2.0])
        .show(ui, |ui| {
            ui.label("");
            for section in &mut arrangement.sections {
                let name = egui::TextEdit::singleline(&mut section.name).desired_width(CELL_WIDTH);
                changed |= ui.add(name).changed();
            }
            ui.end_row();
            ui.label("bars");
            for section in &mut arrangement.sections {
                let bars = egui::DragValue::new(&mut section.bars).range(0..=MAX_BARS);
                changed |= ui.add_sized([CELL_WIDTH, 18.0], bars).changed();
            }
            ui.end_row();
            for (t, name) in tracks.iter().enumerate() {
                ui.label(name);
                for section in &mut arrangement.sections {
                    let cell = &mut section.tracks[t];
                    let text =
                        cell.map_or("·".to_string(), |p| score::pattern_letter(p).to_string());
                    let button = egui::Button::new(text).selected(cell.is_some());
                    let response = ui
                        .add_sized([CELL_WIDTH, 18.0], button)
                        .on_hover_text("Click: silent, then pattern A to D. Right-click: silent.");
                    if response.clicked() {
                        *cell = next_cell(*cell);
                        changed = true;
                    } else if response.secondary_clicked() {
                        *cell = None;
                        changed = true;
                    }
                }
                ui.end_row();
            }
        });
    ui.horizontal(|ui| {
        let count = arrangement.sections.len();
        if ui
            .add_enabled(count < MAX_SECTIONS, egui::Button::new("+ section"))
            .clicked()
        {
            arrangement.sections.push(Section {
                name: (count + 1).to_string(),
                bars: 4,
                tracks: [None; TRACKS],
            });
            changed = true;
        }
        if ui
            .add_enabled(count > 1, egui::Button::new("− section"))
            .on_hover_text("Removes the last section")
            .clicked()
        {
            arrangement.sections.pop();
            changed = true;
        }
    });
    changed
}

/// The notes a piano roll shows, highest first: two octaves up from `low`
/// in the key, and any other note a pattern plays there.
fn rows(patterns: &Patterns, low: u8) -> Vec<u8> {
    let high = low.saturating_add(24).min(127);
    let played = |note: u8| {
        patterns
            .patterns
            .iter()
            .flatten()
            .any(|&s| s == Step::Note(note))
    };
    (low..=high)
        .rev()
        .filter(|&n| patterns.key.contains(n) || played(n))
        .collect()
}

/// Where a piano roll starts: on the root at or below the lowest note
/// played, or in octave 3.
fn default_low(patterns: &Patterns) -> u8 {
    let lowest = patterns
        .patterns
        .iter()
        .flatten()
        .filter_map(|s| match s {
            Step::Note(note) => Some(*note),
            _ => None,
        })
        .min()
        .unwrap_or(48);
    let above_root = (lowest + 12 - patterns.key.root) % 12;
    lowest.saturating_sub(above_root)
}

/// A click on `note` at `step`: removes it if it sounds there, else plays
/// it there.
fn toggle(steps: &mut [Step], step: usize, note: u8) {
    steps[step] = if sounding(steps, step) == Some(note) {
        Step::Rest
    } else {
        Step::Note(note)
    };
}

fn sounding(steps: &[Step], step: usize) -> Option<u8> {
    steps[..=step].iter().rev().find_map(|s| match s {
        Step::Hold => None,
        Step::Rest => Some(None),
        Step::Note(note) => Some(Some(*note)),
    })?
}

/// A drag from the note at `from` to `to`: holds it until there.
fn hold(steps: &mut [Step], from: usize, to: usize) -> bool {
    let mut changed = false;
    let to = to.min(steps.len() - 1);
    for step in &mut steps[from + 1..=to] {
        changed |= *step != Step::Hold;
        *step = Step::Hold;
    }
    changed
}

/// Edits a `notes` module's patterns. Returns whether anything changed.
pub(super) fn patterns(patterns: &mut Patterns, view: &mut NoteView, ui: &mut Ui) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        for p in 0..PATTERNS {
            let letter = score::pattern_letter(p as u8).to_string();
            let text = if patterns.patterns[p].is_empty() {
                egui::RichText::new(letter).weak()
            } else {
                egui::RichText::new(letter)
            };
            if ui.selectable_label(view.pattern == p, text).clicked() {
                view.pattern = p;
                view.page = 0;
            }
        }
        let steps = &mut patterns.patterns[view.pattern];
        let mut length = steps.len();
        let drag = egui::DragValue::new(&mut length)
            .range(0..=MAX_STEPS)
            .suffix(" steps");
        if ui.add(drag).changed() {
            steps.resize(length, Step::Rest);
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        changed |= key(&mut patterns.key, ui);
    });

    let low = *view.low.get_or_insert_with(|| default_low(patterns));
    let length = patterns.patterns[view.pattern].len();
    let pages = length.div_ceil(PAGE).max(1);
    view.page = view.page.min(pages - 1);
    ui.horizontal(|ui| {
        if ui
            .small_button("▼")
            .on_hover_text("An octave down")
            .clicked()
        {
            view.low = Some(low.saturating_sub(12));
        }
        if ui.small_button("▲").on_hover_text("An octave up").clicked() {
            view.low = Some(low.saturating_add(12).min(103));
        }
        if pages > 1 {
            if ui.small_button("‹").clicked() {
                view.page = view.page.saturating_sub(1);
            }
            ui.label(format!("{}/{pages}", view.page + 1));
            if ui.small_button("›").clicked() {
                view.page = (view.page + 1).min(pages - 1);
            }
        }
    });

    let rows = rows(patterns, low);
    let first = view.page * PAGE;
    let shown = (length - first.min(length)).min(PAGE);
    let size = vec2(
        LABEL_WIDTH + PAGE as f32 * STEP_WIDTH,
        rows.len() as f32 * NOTE_HEIGHT,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let steps = &mut patterns.patterns[view.pattern];
    let cell_at = |pos: egui::Pos2| {
        let column = ((pos.x - rect.left() - LABEL_WIDTH) / STEP_WIDTH).floor();
        let row = ((pos.y - rect.top()) / NOTE_HEIGHT).floor();
        (column >= 0.0 && (column as usize) < shown && row >= 0.0 && (row as usize) < rows.len())
            .then(|| (first + column as usize, rows[row as usize]))
    };
    let pointer = response.interact_pointer_pos().and_then(cell_at);
    if response.clicked()
        && let Some((step, note)) = pointer
    {
        toggle(steps, step, note);
        changed = true;
    }
    if response.drag_started()
        && let Some((step, note)) = pointer
    {
        steps[step] = Step::Note(note);
        view.anchor = Some((step, note));
        changed = true;
    }
    if response.dragged()
        && let (Some((from, _)), Some((to, _))) = (view.anchor, pointer)
        && to > from
    {
        changed |= hold(steps, from, to);
    }
    if response.drag_stopped() {
        view.anchor = None;
    }

    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    let text = visuals.text_color();
    let played = visuals.selection.bg_fill;
    for (r, &note) in rows.iter().enumerate() {
        let top = rect.top() + r as f32 * NOTE_HEIGHT;
        let in_key = patterns.key.contains(note);
        let on_root = (note + 12 - patterns.key.root).is_multiple_of(12);
        painter.text(
            pos2(rect.left() + LABEL_WIDTH - 3.0, top + NOTE_HEIGHT / 2.0),
            Align2::RIGHT_CENTER,
            score::note_name(note),
            FontId::proportional(8.0),
            if in_key { text } else { Color32::RED },
        );
        let background = match (on_root, in_key) {
            (true, _) => visuals.widgets.inactive.bg_fill,
            (false, true) => visuals.extreme_bg_color,
            (false, false) => Color32::from_rgb(70, 20, 20),
        };
        for column in 0..shown {
            let step = first + column;
            let cell = Rect::from_min_size(
                pos2(rect.left() + LABEL_WIDTH + column as f32 * STEP_WIDTH, top),
                vec2(STEP_WIDTH - 1.0, NOTE_HEIGHT - 1.0),
            );
            let fill = if sounding(steps, step) == Some(note) {
                played
            } else {
                background
            };
            painter.rect_filled(cell, 1.0, fill);
            if steps[step] == Step::Note(note) {
                painter.rect_stroke(cell, 1.0, Stroke::new(1.0, text), egui::StrokeKind::Inside);
            }
        }
    }
    // A line every beat of four steps.
    for column in (4..shown).step_by(4) {
        let x = rect.left() + LABEL_WIDTH + column as f32 * STEP_WIDTH - 1.0;
        painter.vline(
            x,
            rect.y_range(),
            Stroke::new(1.0, text.gamma_multiply(0.5)),
        );
    }
    changed
}

/// The root and the scale, from two menus.
fn key(key: &mut Key, ui: &mut Ui) -> bool {
    let mut changed = false;
    let id = ui.id();
    egui::ComboBox::from_id_salt(id.with("root"))
        .width(40.0)
        .selected_text(key.name().split(' ').next().unwrap_or_default().to_string())
        .show_ui(ui, |ui| {
            for root in 0..12 {
                let name = score::note_name(root + 60)
                    .trim_end_matches('4')
                    .to_string();
                changed |= ui.selectable_value(&mut key.root, root, name).changed();
            }
        });
    egui::ComboBox::from_id_salt(id.with("scale"))
        .selected_text(SCALES[key.scale].0)
        .show_ui(ui, |ui| {
            for (s, (name, _)) in SCALES.iter().enumerate() {
                changed |= ui.selectable_value(&mut key.scale, s, *name).changed();
            }
        });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::score::parse_steps;

    #[test]
    fn a_cell_goes_round_the_patterns() {
        let mut cell = None;
        let mut seen = Vec::new();
        for _ in 0..6 {
            cell = next_cell(cell);
            seen.push(cell);
        }
        assert_eq!(seen, [Some(0), Some(1), Some(2), Some(3), None, Some(0)]);
    }

    #[test]
    fn clicks_play_a_note_or_take_it_away() {
        let mut steps = parse_steps("C4 - . .").unwrap();
        toggle(&mut steps, 2, 64);
        assert_eq!(score::steps_text(&steps), "C4 - E4 .");
        // Clicking a held note cuts it; another note replaces it.
        toggle(&mut steps, 1, 60);
        assert_eq!(score::steps_text(&steps), "C4 . E4 .");
        toggle(&mut steps, 0, 62);
        assert_eq!(score::steps_text(&steps), "D4 . E4 .");
    }

    #[test]
    fn a_drag_holds_the_note() {
        let mut steps = parse_steps("C4 . E4 . .").unwrap();
        assert!(hold(&mut steps, 0, 3));
        assert_eq!(score::steps_text(&steps), "C4 - - - .");
        assert!(!hold(&mut steps, 0, 2), "nothing new");
        assert!(hold(&mut steps, 3, 99), "up to the end");
        assert_eq!(score::steps_text(&steps), "C4 - - - -");
    }

    #[test]
    fn the_roll_shows_two_octaves_of_the_key_and_whatever_plays() {
        let patterns = Patterns {
            key: Key::parse("A minor").unwrap(),
            patterns: [
                parse_steps("A2 C#3").unwrap(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
        };
        let low = default_low(&patterns);
        assert_eq!(score::note_name(low), "A2");
        let rows = rows(&patterns, low);
        assert_eq!(rows.len(), 7 * 2 + 1 + 1, "two octaves, the top A, and C#3");
        assert_eq!(score::note_name(rows[0]), "A4");
        assert!(rows.contains(&score::parse_note("C#3").unwrap()));
        // Nothing played yet: from the root in octave 3.
        assert_eq!(score::note_name(default_low(&Patterns::default())), "C3");
    }
}
