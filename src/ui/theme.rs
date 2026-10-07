//! The editor's dark look, loosely following Alight Motion: near-black
//! panels around the canvas and a violet accent for selection and keys.

use egui::{
    Color32, Context, CornerRadius, Frame, Margin, Stroke, Theme, ThemePreference, Visuals,
};

use crate::model::LayerKind;

pub const ACCENT: Color32 = Color32::from_rgb(132, 94, 255);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(92, 70, 170);
pub const KEYFRAME: Color32 = Color32::from_rgb(255, 196, 64);
pub const PLAYHEAD: Color32 = Color32::from_rgb(255, 84, 112);

const BG_DEEP: Color32 = Color32::from_rgb(14, 14, 18);
const BG_PANEL: Color32 = Color32::from_rgb(26, 26, 32);
const BG_WIDGET: Color32 = Color32::from_rgb(40, 40, 50);

pub fn apply(ctx: &Context) {
    let mut v = Visuals::dark();
    v.panel_fill = BG_PANEL;
    v.window_fill = BG_PANEL;
    v.extreme_bg_color = BG_DEEP;
    v.faint_bg_color = Color32::from_rgb(32, 32, 40);
    v.selection.bg_fill = ACCENT_SOFT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = ACCENT;
    v.widgets.inactive.weak_bg_fill = BG_WIDGET;
    v.widgets.inactive.bg_fill = BG_WIDGET;
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(56, 52, 76);
    v.widgets.active.weak_bg_fill = ACCENT_SOFT;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(6);
    }
    ctx.set_visuals_of(Theme::Dark, v);
    ctx.set_theme(ThemePreference::Dark);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
    });
}

pub fn panel_frame(ctx: &Context) -> Frame {
    Frame::side_top_panel(&ctx.global_style()).inner_margin(Margin::same(8))
}

pub fn toolbar_frame(ctx: &Context) -> Frame {
    Frame::side_top_panel(&ctx.global_style())
        .fill(BG_DEEP)
        .inner_margin(Margin::symmetric(8, 6))
}

/// Colour used for a layer's bar in the timeline.
pub fn layer_color(kind: &LayerKind) -> Color32 {
    match kind {
        LayerKind::Shape { .. } => Color32::from_rgb(70, 120, 230),
        LayerKind::Text { .. } => Color32::from_rgb(222, 132, 60),
        LayerKind::Image { .. } => Color32::from_rgb(64, 168, 116),
    }
}

pub fn layer_icon(kind: &LayerKind) -> &'static str {
    match kind {
        LayerKind::Shape { .. } => "⬛",
        LayerKind::Text { .. } => "T",
        LayerKind::Image { .. } => "🖼",
    }
}
