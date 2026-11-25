//! The editor's looks: egui's own dark and light themes, and Chaos, the
//! project's own. Pin, light and wire colours mean something, orange for
//! audio and blue for CV, so they stay the same in every theme. So do fonts
//! and spacing: blocks keep their size, and the positions a patch saves
//! still leave room between them whatever the theme.

use egui::epaint::Shadow;
use egui::{Color32, Stroke, Style, Visuals};
use serde::{Deserialize, Serialize};

/// A look for the whole window, saved by name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
    Chaos,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::Dark, Theme::Light, Theme::Chaos];

    pub fn name(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::Chaos => "Chaos",
        }
    }

    /// Draws `ctx` in this theme from now on, unless it already is.
    pub fn apply(self, ctx: &egui::Context) {
        let id = egui::Id::new("audiochaos theme");
        if ctx.data(|d| d.get_temp::<Theme>(id)) == Some(self) {
            return;
        }
        // egui keeps a style for its dark and one for its light mode, and
        // may follow the desktop's: pin the mode, then restyle it whole, so
        // that nothing of the previous theme stays.
        let (mode, style) = match self {
            Self::Dark => (egui::Theme::Dark, plain(Visuals::dark())),
            Self::Light => (egui::Theme::Light, plain(Visuals::light())),
            Self::Chaos => (egui::Theme::Dark, chaos()),
        };
        ctx.set_theme(mode);
        ctx.set_style_of(mode, style);
        ctx.data_mut(|d| d.insert_temp(id, self));
        ctx.request_repaint();
    }
}

fn plain(visuals: Visuals) -> Style {
    Style {
        visuals,
        ..Style::default()
    }
}

/// Near-black violet, acid green and hot pink: a synth's front panel under
/// a black light.
fn chaos() -> Style {
    const INK: Color32 = Color32::from_rgb(0x0b, 0x08, 0x10);
    const PANEL: Color32 = Color32::from_rgb(0x14, 0x0f, 0x1c);
    const BLOCK: Color32 = Color32::from_rgb(0x1c, 0x15, 0x28);
    const RAISED: Color32 = Color32::from_rgb(0x2b, 0x20, 0x3e);
    const LINE: Color32 = Color32::from_rgb(0x48, 0x35, 0x6a);
    // Separators and the canvas grid, which should stay in the background.
    const FAINT: Color32 = Color32::from_rgb(0x2c, 0x21, 0x40);
    const TEXT: Color32 = Color32::from_rgb(0xe6, 0xdc, 0xff);
    const DIM: Color32 = Color32::from_rgb(0xa8, 0x98, 0xc8);
    const ACID: Color32 = Color32::from_rgb(0xc6, 0xff, 0x2e);
    const HOT: Color32 = Color32::from_rgb(0xff, 0x2d, 0x95);

    let mut style = plain(Visuals::dark());
    let glow = Shadow {
        offset: [0, 0],
        blur: 18,
        spread: 0,
        color: HOT.gamma_multiply(0.18),
    };
    let v = &mut style.visuals;
    v.panel_fill = PANEL;
    v.window_fill = BLOCK;
    v.faint_bg_color = PANEL;
    v.extreme_bg_color = INK;
    v.code_bg_color = INK;
    v.hyperlink_color = ACID;
    v.window_stroke = Stroke::new(1.0, LINE);
    v.window_shadow = glow;
    v.popup_shadow = glow;
    v.window_corner_radius = 8.into();
    v.menu_corner_radius = 6.into();
    v.selection.bg_fill = HOT.gamma_multiply(0.7);
    v.selection.stroke = Stroke::new(1.0, ACID);
    v.slider_trailing_fill = true;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BLOCK;
    w.noninteractive.weak_bg_fill = BLOCK;
    w.noninteractive.bg_stroke = Stroke::new(1.0, FAINT);
    w.noninteractive.fg_stroke = Stroke::new(1.0, DIM);
    w.inactive.bg_fill = RAISED;
    w.inactive.weak_bg_fill = RAISED;
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.hovered.bg_fill = LINE;
    w.hovered.weak_bg_fill = LINE;
    w.hovered.bg_stroke = Stroke::new(1.0, HOT);
    w.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    w.active.bg_fill = HOT;
    w.active.weak_bg_fill = HOT;
    w.active.bg_stroke = Stroke::new(1.0, ACID);
    w.active.fg_stroke = Stroke::new(2.0, INK);
    w.open.bg_fill = RAISED;
    w.open.weak_bg_fill = RAISED;
    w.open.bg_stroke = Stroke::new(1.0, HOT);
    w.open.fg_stroke = Stroke::new(1.0, ACID);
    for widget in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        widget.corner_radius = 4.into();
    }

    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_theme_draws_in_its_own_mode() {
        let ctx = egui::Context::default();
        for (theme, dark) in [
            (Theme::Light, false),
            (Theme::Chaos, true),
            (Theme::Dark, true),
        ] {
            theme.apply(&ctx);
            assert_eq!(ctx.global_style().visuals.dark_mode, dark, "{theme:?}");
        }
    }

    #[test]
    fn leaving_chaos_leaves_nothing_of_it() {
        let ctx = egui::Context::default();
        Theme::Chaos.apply(&ctx);
        Theme::Dark.apply(&ctx);
        assert_eq!(ctx.global_style().visuals, Visuals::dark());
    }

    #[test]
    fn blocks_keep_their_size_in_every_theme() {
        let ctx = egui::Context::default();
        let plain = Style::default();
        for theme in Theme::ALL {
            theme.apply(&ctx);
            let style = ctx.global_style();
            assert_eq!(style.text_styles, plain.text_styles, "{theme:?}");
            assert_eq!(style.spacing, plain.spacing, "{theme:?}");
        }
    }

    #[test]
    fn themes_are_saved_by_name() {
        let saved: Vec<_> = Theme::ALL
            .iter()
            .map(|theme| serde_json::to_string(theme).unwrap())
            .collect();
        assert_eq!(saved, [r#""dark""#, r#""light""#, r#""chaos""#]);
        assert_eq!(
            serde_json::from_str::<Theme>(r#""chaos""#).unwrap(),
            Theme::Chaos
        );
    }
}
