//! The editor's look, modelled on Alight Motion: near-black surfaces, flat
//! rounded controls, coloured layer clips and a violet accent, with more
//! breathing room than the phone app since a desktop has the space.

use egui::{
    Color32, Context, CornerRadius, Frame, Margin, Stroke, Theme, ThemePreference, Visuals,
};

use crate::model::LayerKind;
use crate::ui::icons::Icon;

pub const ACCENT: Color32 = Color32::from_rgb(138, 99, 255);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(98, 72, 196);
pub const KEYFRAME: Color32 = Color32::from_rgb(255, 196, 64);
pub const PLAYHEAD: Color32 = Color32::from_rgb(255, 84, 112);

/// Behind the canvas and on the home screen.
pub const BG_DEEP: Color32 = Color32::from_rgb(10, 10, 12);
/// Panels (inspector, timeline, top bar).
pub const BG_PANEL: Color32 = Color32::from_rgb(20, 20, 24);
/// Cards, inputs and buttons sitting on panels.
pub const BG_SURFACE: Color32 = Color32::from_rgb(32, 32, 38);
const BG_HOVER: Color32 = Color32::from_rgb(48, 46, 60);

pub fn apply(ctx: &Context) {
    let mut v = Visuals::dark();
    v.panel_fill = BG_PANEL;
    v.window_fill = BG_PANEL;
    v.window_stroke = Stroke::new(1.0, Color32::from_rgb(40, 40, 48));
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(10);
    v.extreme_bg_color = BG_DEEP;
    v.faint_bg_color = Color32::from_rgb(24, 24, 29);
    v.selection.bg_fill = ACCENT_SOFT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = ACCENT;
    v.slider_trailing_fill = true;

    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(34, 34, 40));
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.bg_stroke = Stroke::NONE;
    }
    v.widgets.inactive.weak_bg_fill = BG_SURFACE;
    v.widgets.inactive.bg_fill = BG_SURFACE;
    v.widgets.hovered.weak_bg_fill = BG_HOVER;
    v.widgets.hovered.bg_fill = BG_HOVER;
    v.widgets.active.weak_bg_fill = ACCENT_SOFT;
    v.widgets.active.bg_fill = ACCENT_SOFT;
    v.widgets.open.weak_bg_fill = BG_HOVER;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(8);
    }
    ctx.set_visuals_of(Theme::Dark, v);
    ctx.set_theme(ThemePreference::Dark);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 26.0;
        style.spacing.menu_margin = Margin::same(8);
    });
}

pub fn panel_frame(_ctx: &Context) -> Frame {
    Frame::NONE.fill(BG_PANEL).inner_margin(Margin::same(10))
}

pub fn top_bar_frame(_ctx: &Context) -> Frame {
    Frame::NONE
        .fill(BG_PANEL)
        .inner_margin(Margin::symmetric(10, 6))
}

/// Colour used for a layer's clip in the timeline.
pub fn layer_color(kind: &LayerKind) -> Color32 {
    match kind {
        LayerKind::Shape { .. } => Color32::from_rgb(76, 126, 240),
        LayerKind::Text { .. } => Color32::from_rgb(232, 138, 64),
        LayerKind::Path { .. } => Color32::from_rgb(64, 150, 220),
        LayerKind::Image { .. } => Color32::from_rgb(52, 176, 124),
        LayerKind::Video { .. } => Color32::from_rgb(214, 84, 150),
        LayerKind::Null => Color32::from_rgb(200, 72, 92),
        LayerKind::Group => Color32::from_rgb(150, 110, 230),
        LayerKind::Audio { .. } => Color32::from_rgb(38, 166, 154),
        LayerKind::Camera { .. } => Color32::from_rgb(120, 128, 150),
        LayerKind::Light { .. } => Color32::from_rgb(214, 162, 40),
        LayerKind::Adjustment => Color32::from_rgb(96, 132, 170),
    }
}

pub fn layer_icon(kind: &LayerKind) -> Icon {
    use crate::model::ShapeKind;
    match kind {
        LayerKind::Shape { shape, .. } => match shape {
            ShapeKind::Rectangle => Icon::Rectangle,
            ShapeKind::Ellipse => Icon::Ellipse,
            ShapeKind::Polygon { sides: 3 } => Icon::Triangle,
            ShapeKind::Polygon { .. } => Icon::Hexagon,
            ShapeKind::Star { .. } => Icon::Star,
        },
        LayerKind::Text { .. } => Icon::Text,
        LayerKind::Path { .. } => Icon::Pen,
        LayerKind::Image { .. } => Icon::Image,
        LayerKind::Video { .. } => Icon::Video,
        LayerKind::Null => Icon::Null,
        LayerKind::Group => Icon::Group,
        LayerKind::Audio { .. } => Icon::Audio,
        LayerKind::Camera { .. } => Icon::Camera,
        LayerKind::Light { .. } => Icon::Light,
        LayerKind::Adjustment => Icon::Adjustment,
    }
}
