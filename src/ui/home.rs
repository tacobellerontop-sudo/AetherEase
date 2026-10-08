//! The start screen (create a project or reopen a recent one) and the
//! "new project" dialog.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Modal, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind, Ui, Vec2, vec2,
};

use crate::app::AetherApp;
use crate::model::{Color, ProjectSettings};
use crate::recent;
use crate::render::{self, View};
use crate::ui::icons::{self, Icon};
use crate::ui::theme;

const MAX_WIDTH: f32 = 1080.0;
const CARD_MIN_WIDTH: f32 = 230.0;
const CARD_GAP: f32 = 16.0;
const CARD_TEXT_HEIGHT: f32 = 56.0;

const BACKGROUNDS: [Color32; 8] = [
    Color32::BLACK,
    Color32::WHITE,
    Color32::from_rgb(30, 30, 36),
    Color32::from_rgb(43, 45, 66),
    Color32::from_rgb(124, 92, 255),
    Color32::from_rgb(255, 84, 112),
    Color32::from_rgb(46, 196, 182),
    Color32::from_rgb(255, 193, 69),
];

enum HomeAction {
    Open(std::path::PathBuf),
    Forget(std::path::PathBuf),
}

impl AetherApp {
    pub fn home_ui(&mut self, ui: &mut Ui) {
        let mut action = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                let width = (ui.available_width() - 48.0).min(MAX_WIDTH);
                let margin = (ui.available_width() - width) * 0.5;
                ui.add_space(36.0);
                ui.horizontal(|ui| {
                    ui.add_space(margin);
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        action = self.home_contents(ui, width);
                    });
                });
                ui.add_space(36.0);
            });
        match action {
            Some(HomeAction::Open(path)) => self.open_path(&path),
            Some(HomeAction::Forget(path)) => self.forget_recent(&path),
            None => {}
        }
    }

    fn home_contents(&mut self, ui: &mut Ui, width: f32) -> Option<HomeAction> {
        ui.label(
            RichText::new("AetherEase")
                .size(30.0)
                .strong()
                .color(theme::ACCENT),
        );
        ui.add_space(20.0);

        // The one big call to action, like Alight Motion's "+" on its home.
        let (rect, create) = ui.allocate_exact_size(vec2(width, 72.0), Sense::click());
        let fill = if create.hovered() {
            theme::ACCENT
        } else {
            theme::ACCENT_SOFT
        };
        ui.painter().rect_filled(rect, CornerRadius::same(14), fill);
        let galley = ui.painter().layout_no_wrap(
            "Create new project".into(),
            FontId::proportional(18.0),
            Color32::WHITE,
        );
        let content_width = 28.0 + 12.0 + galley.size().x;
        let left = rect.center().x - content_width * 0.5;
        let circle = Pos2::new(left + 14.0, rect.center().y);
        ui.painter()
            .circle_filled(circle, 14.0, Color32::WHITE.gamma_multiply(0.2));
        icons::paint(
            ui.painter(),
            Rect::from_center_size(circle, Vec2::splat(16.0)),
            Icon::Plus,
            Color32::WHITE,
        );
        ui.painter().galley(
            Pos2::new(left + 40.0, rect.center().y - galley.size().y * 0.5),
            galley,
            Color32::WHITE,
        );
        if create.on_hover_cursor(CursorIcon::PointingHand).clicked() {
            self.new_project();
        }

        if let Some(status) = self.status.clone() {
            ui.add_space(8.0);
            ui.colored_label(theme::PLAYHEAD, status);
        }

        ui.add_space(28.0);
        ui.label(RichText::new("Recent projects").size(18.0).strong());
        ui.add_space(10.0);

        if self.home_items.is_empty() {
            ui.label(RichText::new("Projects you create or open will show up here.").weak());
            return None;
        }

        let columns = (((width + CARD_GAP) / (CARD_MIN_WIDTH + CARD_GAP)).floor() as usize).max(1);
        let card_width = (width - CARD_GAP * (columns - 1) as f32) / columns as f32;
        let thumb_height = card_width * 9.0 / 16.0;
        let card_size = vec2(card_width, thumb_height + CARD_TEXT_HEIGHT);
        let now = recent::now_secs();

        let mut action = None;
        for row in 0..self.home_items.len().div_ceil(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = CARD_GAP;
                for i in row * columns..((row + 1) * columns).min(self.home_items.len()) {
                    if let Some(a) = self.project_card(ui, i, card_size, now) {
                        action = Some(a);
                    }
                }
            });
            ui.add_space(CARD_GAP);
        }
        action
    }

    fn project_card(
        &mut self,
        ui: &mut Ui,
        index: usize,
        size: Vec2,
        now: u64,
    ) -> Option<HomeAction> {
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let response = response.on_hover_cursor(CursorIcon::PointingHand);
        let item = &self.home_items[index];
        let path = item.entry.path.clone();
        let painter = ui.painter_at(rect);
        let visuals = ui.visuals();

        painter.rect_filled(rect, CornerRadius::same(12), visuals.panel_fill);
        let thumb =
            Rect::from_min_size(rect.min, vec2(size.x, size.y - CARD_TEXT_HEIGHT)).shrink(6.0);
        painter.rect_filled(thumb, CornerRadius::same(6), Color32::from_gray(10));

        let (title, detail) = match &item.project {
            Some(project) => {
                // A live render of the project as its thumbnail.
                let canvas = vec2(project.width as f32, project.height as f32);
                let zoom = (thumb.width() / canvas.x).min(thumb.height() / canvas.y);
                let view = View {
                    origin: thumb.center() - canvas * zoom * 0.5,
                    zoom,
                };
                let thumb_painter = painter.with_clip_rect(thumb);
                let frame = project.duration / 3;
                render::draw_project(
                    &thumb_painter,
                    view,
                    project,
                    frame,
                    &mut self.textures,
                    false,
                );
                (
                    project.name.clone(),
                    format!(
                        "{}×{} · {} fps · {}",
                        project.width,
                        project.height,
                        project.fps,
                        recent::relative_time(item.entry.last_used, now)
                    ),
                )
            }
            None => {
                painter.text(
                    thumb.center(),
                    Align2::CENTER_CENTER,
                    "Can't read this file",
                    FontId::proportional(13.0),
                    theme::PLAYHEAD,
                );
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (name, path.display().to_string())
            }
        };

        let visuals = ui.visuals();
        let text_painter = painter.with_clip_rect(rect.shrink(6.0));
        let text_top = thumb.bottom() + 8.0;
        text_painter.text(
            Pos2::new(rect.left() + 10.0, text_top),
            Align2::LEFT_TOP,
            title,
            FontId::proportional(15.0),
            visuals.strong_text_color(),
        );
        text_painter.text(
            Pos2::new(rect.left() + 10.0, text_top + 22.0),
            Align2::LEFT_TOP,
            detail,
            FontId::proportional(12.0),
            visuals.weak_text_color(),
        );
        if response.hovered() {
            painter.rect_stroke(
                rect,
                CornerRadius::same(10),
                Stroke::new(1.5, theme::ACCENT),
                StrokeKind::Inside,
            );
        }

        let mut action = None;
        if response.clicked() && self.home_items[index].project.is_some() {
            action = Some(HomeAction::Open(path.clone()));
        }
        let response = response.on_hover_text(path.display().to_string());
        response.context_menu(|ui| {
            if ui.button("Open").clicked() {
                action = Some(HomeAction::Open(path.clone()));
            }
            if ui.button("Remove from recent").clicked() {
                action = Some(HomeAction::Forget(path.clone()));
            }
        });
        action
    }

    /// The "new project" dialog, shown over whichever screen is open.
    pub fn new_project_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.new_project_form.take() else {
            return;
        };
        let mut create = false;
        let mut cancel = false;
        let modal = Modal::new(Id::new("new_project_dialog")).show(ctx, |ui| {
            ui.set_width(460.0);
            settings_form(ui, &mut form);
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let (w, h) = form.size();
                ui.label(RichText::new(format!("{w} × {h} px · {} fps", form.fps)).weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let button = egui::Button::new(RichText::new("Create project").strong())
                        .fill(theme::ACCENT_SOFT);
                    if ui.add(button).clicked() {
                        create = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                create = true;
            }
        });
        if create {
            self.create_project(&form);
        } else if !cancel && !modal.should_close() {
            self.new_project_form = Some(form);
        }
    }
}

fn settings_form(ui: &mut Ui, form: &mut ProjectSettings) {
    ui.label(RichText::new("New project").size(20.0).strong());
    ui.add_space(10.0);

    field_label(ui, "Project name");
    let name = ui.add(
        egui::TextEdit::singleline(&mut form.name)
            .id(Id::new("new_project_name"))
            .desired_width(f32::INFINITY),
    );
    // Focus the name on open so typing works straight away.
    if ui.memory(|m| m.focused().is_none()) {
        name.request_focus();
    }

    field_label(ui, "Resolution");
    ui.horizontal_wrapped(|ui| {
        for (label, value) in ProjectSettings::RESOLUTIONS {
            ui.selectable_value(&mut form.resolution, value, label);
        }
    });

    field_label(ui, "Aspect ratio");
    ui.horizontal_wrapped(|ui| {
        for aspect in ProjectSettings::ASPECTS {
            if aspect_tile(ui, aspect, form.aspect == aspect).clicked() {
                form.aspect = aspect;
            }
        }
    });

    field_label(ui, "Frame rate");
    ui.horizontal_wrapped(|ui| {
        for fps in ProjectSettings::FRAME_RATES {
            ui.selectable_value(&mut form.fps, fps, fps.to_string());
        }
    });

    field_label(ui, "Background");
    ui.horizontal_wrapped(|ui| {
        let current = form.background.to_color32();
        for color in BACKGROUNDS {
            if swatch(ui, color, color == current).clicked() {
                form.background = Color::from_color32(color);
            }
        }
        ui.add_space(6.0);
        let mut rgb = [current.r(), current.g(), current.b()];
        if ui
            .color_edit_button_srgb(&mut rgb)
            .on_hover_text("Custom color")
            .changed()
        {
            form.background = Color::from_color32(Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
        }
    });
}

fn field_label(ui: &mut Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(text).weak());
}

/// A button showing a small rectangle in the given aspect ratio.
fn aspect_tile(ui: &mut Ui, (w, h): (u32, u32), selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(56.0, 58.0), Sense::click());
    let visuals = ui.visuals();
    let fill = if selected {
        theme::ACCENT_SOFT
    } else if response.hovered() {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        visuals.widgets.inactive.weak_bg_fill
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), fill);

    let icon_area = Rect::from_center_size(rect.center() - vec2(0.0, 8.0), Vec2::splat(26.0));
    let scale = 26.0 / w.max(h) as f32;
    let icon = Rect::from_center_size(icon_area.center(), vec2(w as f32 * scale, h as f32 * scale));
    let color = if selected {
        Color32::WHITE
    } else {
        visuals.text_color()
    };
    painter.rect_stroke(
        icon,
        CornerRadius::same(2),
        Stroke::new(1.5, color),
        StrokeKind::Inside,
    );
    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - 10.0),
        Align2::CENTER_CENTER,
        format!("{w}:{h}"),
        FontId::proportional(11.0),
        color,
    );
    response.on_hover_cursor(CursorIcon::PointingHand)
}

fn swatch(ui: &mut Ui, color: Color32, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    let painter = ui.painter();
    let center = rect.center();
    if selected {
        painter.circle_stroke(center, 13.0, Stroke::new(2.0, theme::ACCENT));
    }
    painter.circle(
        center,
        10.0,
        color,
        Stroke::new(1.0, Color32::from_gray(90)),
    );
    response.on_hover_cursor(CursorIcon::PointingHand)
}
