//! Top-level editor state and window layout.

use std::path::{Path, PathBuf};

use egui::{Context, Key, KeyboardShortcut, Modifiers, ViewportCommand};

use crate::history::History;
use crate::model::{Project, PropId, ShapeKind};
use crate::render::{self, TextureCache};
use crate::ui::{theme, timeline::TimelineState, viewport::ViewportState};

pub const PROJECT_EXTENSION: &str = "aether";
const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "bmp", "gif"];

/// A keyframe picked in the timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeySelection {
    pub layer: u64,
    pub prop: PropId,
    pub frame: i32,
}

pub struct AetherApp {
    pub project: Project,
    pub history: History,
    /// The project as last saved or opened, to tell whether there are unsaved changes.
    saved: Project,
    pub path: Option<PathBuf>,

    pub selected: Option<u64>,
    pub selected_key: Option<KeySelection>,

    /// The playhead.
    pub frame: i32,
    pub playing: bool,
    pub looping: bool,
    play_clock: f32,

    pub textures: TextureCache,
    pub viewport: ViewportState,
    pub timeline: TimelineState,

    /// Last error or notice, shown in the toolbar.
    pub status: Option<String>,
    window_title: String,
}

impl AetherApp {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        let project = Project::default();
        let mut app = Self {
            history: History::new(&project),
            saved: project.clone(),
            project,
            path: None,
            selected: None,
            selected_key: None,
            frame: 0,
            playing: false,
            looping: true,
            play_clock: 0.0,
            textures: TextureCache::default(),
            viewport: ViewportState::default(),
            timeline: TimelineState::default(),
            status: None,
            window_title: String::new(),
        };
        if let Some(path) = path {
            app.open_path(&path);
        }
        app
    }

    pub fn is_dirty(&self) -> bool {
        self.project != self.saved
    }

    pub fn selected_layer(&self) -> Option<&crate::model::Layer> {
        self.selected.and_then(|id| self.project.layer(id))
    }

    pub fn set_frame(&mut self, frame: i32) {
        self.frame = frame.clamp(0, (self.project.duration - 1).max(0));
    }

    pub fn select(&mut self, id: Option<u64>) {
        if self.selected != id {
            self.selected = id;
            self.selected_key = None;
        }
    }

    // ---- Layer commands -------------------------------------------------

    pub fn add_shape(&mut self, shape: ShapeKind) {
        let id = self.project.add_shape(shape, self.frame);
        self.select(Some(id));
    }

    pub fn add_text(&mut self) {
        let id = self.project.add_text("Your text", self.frame);
        self.select(Some(id));
    }

    pub fn import_image(&mut self, ctx: &Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &IMAGE_EXTENSIONS)
            .pick_file()
        else {
            return;
        };
        match render::load_image(&path) {
            Ok(image) => {
                let size = egui::vec2(image.size[0] as f32, image.size[1] as f32);
                self.textures.insert(ctx, &path, image);
                let id = self.project.add_image(path, size, self.frame);
                self.select(Some(id));
            }
            Err(err) => self.status = Some(format!("Couldn't import {}: {err}", path.display())),
        }
    }

    pub fn delete_selection(&mut self) {
        if let Some(key) = self.selected_key.take() {
            if let Some(track) = self
                .project
                .layer_mut(key.layer)
                .and_then(|l| l.track_mut(key.prop))
            {
                track.remove_key(key.frame);
            }
        } else if let Some(id) = self.selected.take() {
            self.project.remove_layer(id);
        }
    }

    pub fn duplicate_selected(&mut self) {
        if let Some(id) = self.selected
            && let Some(copy) = self.project.duplicate_layer(id)
        {
            self.select(Some(copy));
        }
    }

    pub fn undo(&mut self) {
        self.history.undo(&mut self.project);
        self.forget_missing_selection();
    }

    pub fn redo(&mut self) {
        self.history.redo(&mut self.project);
        self.forget_missing_selection();
    }

    fn forget_missing_selection(&mut self) {
        if self
            .selected
            .is_some_and(|id| self.project.layer(id).is_none())
        {
            self.select(None);
        }
        self.selected_key = None;
        self.set_frame(self.frame);
    }

    // ---- Files ----------------------------------------------------------

    /// Asks before throwing away unsaved work. Returns whether to go ahead.
    fn confirm_discard(&self) -> bool {
        !self.is_dirty()
            || rfd::MessageDialog::new()
                .set_title("Unsaved changes")
                .set_description("Discard the changes to this project?")
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                == rfd::MessageDialogResult::Yes
    }

    fn replace_project(&mut self, project: Project, path: Option<PathBuf>) {
        self.history = History::new(&project);
        self.saved = project.clone();
        self.project = project;
        self.path = path;
        self.selected = None;
        self.selected_key = None;
        self.frame = 0;
        self.playing = false;
        self.textures.clear();
        self.viewport = ViewportState::default();
        self.timeline = TimelineState::default();
    }

    pub fn new_project(&mut self) {
        if self.confirm_discard() {
            self.replace_project(Project::default(), None);
        }
    }

    pub fn open_dialog(&mut self) {
        if !self.confirm_discard() {
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("AetherEase project", &[PROJECT_EXTENSION])
            .pick_file()
        {
            self.open_path(&path);
        }
    }

    pub fn open_path(&mut self, path: &Path) {
        let result = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|json| Project::from_json(&json).map_err(|e| e.to_string()));
        match result {
            Ok(project) => {
                self.replace_project(project, Some(path.to_owned()));
                self.status = None;
            }
            Err(err) => self.status = Some(format!("Couldn't open {}: {err}", path.display())),
        }
    }

    pub fn save(&mut self) {
        match self.path.clone() {
            Some(path) => self.save_to(&path),
            None => self.save_as(),
        }
    }

    pub fn save_as(&mut self) {
        let file_name = format!("{}.{PROJECT_EXTENSION}", self.project.name);
        if let Some(mut path) = rfd::FileDialog::new()
            .add_filter("AetherEase project", &[PROJECT_EXTENSION])
            .set_file_name(file_name)
            .save_file()
        {
            if path.extension().is_none() {
                path.set_extension(PROJECT_EXTENSION);
            }
            self.save_to(&path);
        }
    }

    fn save_to(&mut self, path: &Path) {
        let result = self
            .project
            .to_json()
            .map_err(|e| e.to_string())
            .and_then(|json| std::fs::write(path, json).map_err(|e| e.to_string()));
        match result {
            Ok(()) => {
                self.saved = self.project.clone();
                self.path = Some(path.to_owned());
                self.status = None;
            }
            Err(err) => self.status = Some(format!("Couldn't save {}: {err}", path.display())),
        }
    }

    // ---- Per-frame plumbing ---------------------------------------------

    fn handle_shortcuts(&mut self, ctx: &Context) {
        let ctrl = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        let ctrl_shift = |key| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, key);
        let shortcut = |s: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&s));

        if shortcut(ctrl_shift(Key::S)) {
            self.save_as();
        }
        if shortcut(ctrl(Key::S)) {
            self.save();
        }
        if shortcut(ctrl(Key::O)) {
            self.open_dialog();
        }
        if shortcut(ctrl(Key::N)) {
            self.new_project();
        }

        // Single-key shortcuts must not steal keystrokes from text fields.
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if shortcut(ctrl_shift(Key::Z)) || shortcut(ctrl(Key::Y)) {
            self.redo();
        }
        if shortcut(ctrl(Key::Z)) {
            self.undo();
        }
        if shortcut(ctrl(Key::D)) {
            self.duplicate_selected();
        }
        let key = |k| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
        if key(Key::Space) {
            self.toggle_playback();
        }
        if key(Key::Delete) || key(Key::Backspace) {
            self.delete_selection();
        }
        if key(Key::ArrowLeft) {
            self.set_frame(self.frame - 1);
        }
        if key(Key::ArrowRight) {
            self.set_frame(self.frame + 1);
        }
        if key(Key::Home) {
            self.set_frame(0);
        }
        if key(Key::End) {
            self.set_frame(self.project.duration - 1);
        }
        if key(Key::Escape) {
            self.select(None);
        }
    }

    pub fn toggle_playback(&mut self) {
        self.playing = !self.playing;
        self.play_clock = 0.0;
        if self.playing && self.frame >= self.project.duration - 1 {
            self.frame = 0;
        }
    }

    fn advance_playback(&mut self, ctx: &Context) {
        if !self.playing {
            return;
        }
        let frame_time = 1.0 / self.project.fps.max(1) as f32;
        self.play_clock += ctx.input(|i| i.stable_dt).min(0.25);
        while self.play_clock >= frame_time {
            self.play_clock -= frame_time;
            self.frame += 1;
            if self.frame >= self.project.duration {
                if self.looping {
                    self.frame = 0;
                } else {
                    self.frame = self.project.duration - 1;
                    self.playing = false;
                }
            }
        }
        ctx.request_repaint();
    }

    fn commit_history(&mut self, ctx: &Context) {
        let gesture_in_progress =
            ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input();
        if !gesture_in_progress {
            self.history.commit(&self.project);
        }
    }

    fn handle_close(&mut self, ctx: &Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.confirm_discard() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        }
    }

    fn update_title(&mut self, ctx: &Context) {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.project.name.clone());
        let dirty = if self.is_dirty() { "*" } else { "" };
        let title = format!("{name}{dirty} - AetherEase");
        if title != self.window_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }
}

impl eframe::App for AetherApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_close(&ctx);
        self.handle_shortcuts(&ctx);
        self.advance_playback(&ctx);

        egui::Panel::top("menu_bar").show(ui, |ui| self.menu_bar_ui(ui));
        egui::Panel::top("toolbar")
            .frame(theme::toolbar_frame(&ctx))
            .show(ui, |ui| self.toolbar_ui(ui));
        egui::Panel::bottom("timeline")
            .resizable(true)
            .default_size(300.0)
            .size_range(160.0..=800.0)
            .frame(theme::panel_frame(&ctx))
            .show(ui, |ui| self.timeline_ui(ui));
        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(320.0)
            .size_range(260.0..=520.0)
            .frame(theme::panel_frame(&ctx))
            .show(ui, |ui| self.inspector_ui(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.viewport_ui(ui));

        self.commit_history(&ctx);
        self.update_title(&ctx);
    }
}
