//! The controls drawn inside blocks: knobs with their modulation tick,
//! switches, and lights.

use egui::{Color32, Rect, Ui};

use crate::engine::param::{ParamInfo, Scale};

pub(super) const AUDIO_PIN: Color32 = Color32::from_rgb(0xe0, 0x6c, 0x3c);
pub(super) const CV_PIN: Color32 = Color32::from_rgb(0x4c, 0x9b, 0xd6);
pub(super) const SLIDER_WIDTH: f32 = 150.0;
const LED_OFF: Color32 = Color32::from_rgb(0x40, 0x10, 0x10);
const LED_ON: Color32 = Color32::from_rgb(0xff, 0x30, 0x20);
pub(super) const LED_RADIUS: f32 = 5.0;
/// Closer than this to the knob, in knob travel, the modulation mark would
/// only hide the handle.
const MARKER_MIN_GAP: f32 = 0.01;

/// Where on its travel (0..1) a knob set to `value` gets a modulation tick,
/// if its `modulated` value is far enough from it to show.
pub(super) fn marked(param: &ParamInfo, value: f32, modulated: Option<f32>) -> Option<f32> {
    let position = param.to_normalized(modulated?);
    ((position - param.to_normalized(value)).abs() > MARKER_MIN_GAP).then_some(position)
}

/// A tick across a slider's rail at `position`, 0..1 of its travel, in the
/// colour of CV inputs. `slider` is the whole widget, rail first, then the
/// value and name; the geometry follows egui's own slider.
fn mark_modulation(position: f32, slider: Rect, ui: &mut Ui) {
    let height = ui.spacing().interact_size.y;
    let handle_radius = match ui.visuals().handle_shape {
        egui::style::HandleShape::Circle => height / 2.5,
        egui::style::HandleShape::Rect { aspect_ratio } => height / 2.5 * aspect_ratio,
    };
    let rail = egui::Rangef::new(slider.left(), slider.left() + ui.spacing().slider_width);
    let travel = rail.shrink(handle_radius);
    let x = egui::lerp(travel.min..=travel.max, position.clamp(0.0, 1.0));
    let y = slider.center().y;
    let stroke = egui::Stroke::new(2.0, CV_PIN);
    ui.painter().line_segment(
        [
            egui::pos2(x, y - height / 2.0),
            egui::pos2(x, y + height / 2.0),
        ],
        stroke,
    );
}

/// A round light, dark to bright red.
pub(super) fn led(brightness: f32, ui: &mut Ui) {
    let size = egui::Vec2::splat(2.0 * LED_RADIUS);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let colour = LED_OFF.lerp_to_gamma(LED_ON, brightness.clamp(0.0, 1.0));
    ui.painter()
        .circle_filled(rect.center(), LED_RADIUS, colour);
}

/// A slider whose right-click menu resets it, marked where CV actually
/// takes the knob (`modulated`) when that is elsewhere. Returns whether
/// `value` changed.
pub(super) fn knob(
    param: &ParamInfo,
    value: &mut f32,
    modulated: Option<f32>,
    ui: &mut Ui,
) -> bool {
    let mark = marked(param, *value, modulated);
    let slider = egui::Slider::new(value, param.min..=param.max)
        .text(param.name)
        .logarithmic(param.scale == Scale::Exponential)
        .custom_formatter(|v, _| param.format(v as f32));
    // Sliders only sense drags, and a menu opens on a click.
    let response = ui.add(slider).on_hover_text(param.help);
    if let Some(position) = mark {
        mark_modulation(position, response.rect, ui);
    }
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

/// A toggle switch for an on/off knob, with its name beside it. Returns
/// whether `value` changed.
pub(super) fn switch(
    param: &ParamInfo,
    value: &mut f32,
    modulated: Option<f32>,
    ui: &mut Ui,
) -> bool {
    ui.horizontal(|ui| {
        let changed = toggle(param, value, modulated, ui);
        ui.add(egui::Label::new(param.name).selectable(false))
            .on_hover_text(param.help);
        changed
    })
    .inner
}

/// A toggle switch alone, outlined in the colour of CV inputs when CV turns
/// it the other way (`modulated`). Returns whether `value` changed.
pub(super) fn toggle(
    param: &ParamInfo,
    value: &mut f32,
    modulated: Option<f32>,
    ui: &mut Ui,
) -> bool {
    let on = ParamInfo::is_on(*value);
    let flipped = modulated.is_some_and(|m| ParamInfo::is_on(m) != on);
    let size = ui.spacing().interact_size.y * egui::vec2(1.8, 1.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let how_on = ui.ctx().animate_bool_responsive(response.id, on);
    let visuals = ui.style().interact_selectable(&response, on);
    let radius = rect.height() / 2.0;
    let stroke = if flipped {
        egui::Stroke::new(2.0, CV_PIN)
    } else {
        visuals.bg_stroke
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        radius,
        visuals.bg_fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
    let handle = egui::pos2(x, rect.center().y);
    painter.circle(handle, 0.75 * radius, visuals.bg_fill, visuals.fg_stroke);
    if response.on_hover_text(param.help).clicked() {
        *value = if on { param.min } else { param.max };
        return true;
    }
    false
}
