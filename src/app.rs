//! Top-level editor state and window layout.

use std::path::{Path, PathBuf};

use egui::{Context, Key, KeyboardShortcut, Modifiers, ViewportCommand};

use crate::compose::{self, Assets};
use crate::history::History;
use crate::model::{Project, ProjectSettings, ShapeKind};
use crate::recent::{self, RecentEntry, RecentProjects};
use crate::ui::{theme, timeline::TimelineState, viewport::ViewportState};

pub const PROJECT_EXTENSION: &str = "aether";
const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "bmp", "gif"];

/// Seconds of inactivity after an edit before the project is saved.
const AUTOSAVE_DELAY: f64 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Home,
    Editor,
}

/// A recent project as shown on the home screen; `project` is `None` when the
/// file can't be read.
pub struct HomeItem {
    pub entry: RecentEntry,
    pub project: Option<Project>,
    /// The project's rendered thumbnail, made the first time it's shown.
    pub thumbnail: Option<egui::TextureHandle>,
}

/// A keyframe picked in the timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeySelection {
    pub layer: u64,
    pub frame: i32,
}

pub struct AetherApp {
    pub screen: Screen,
    /// The "new project" dialog, while it is open.
    pub new_project_form: Option<ProjectSettings>,
    pub recent: RecentProjects,
    pub home_items: Vec<HomeItem>,

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

    pub assets: Assets,
    pub viewport: ViewportState,
    pub timeline: TimelineState,

    /// Last error or notice, shown in the toolbar.
    pub status: Option<String>,
    window_title: String,
    /// When the project first differed from its saved copy, for autosave.
    dirty_since: Option<f64>,
}

impl AetherApp {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        let project = Project::default();
        let mut app = Self {
            screen: Screen::Home,
            new_project_form: None,
            recent: RecentProjects::load(),
            home_items: Vec::new(),
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
            assets: Assets::default(),
            viewport: ViewportState::default(),
            timeline: TimelineState::default(),
            status: None,
            window_title: String::new(),
            dirty_since: None,
        };
        app.refresh_home();
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

    pub fn add_null(&mut self) {
        let id = self.project.add_null(self.frame);
        self.select(Some(id));
    }

    pub fn add_camera(&mut self) {
        let id = self.project.add_camera(self.frame);
        self.select(Some(id));
    }

    pub fn import_image(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &IMAGE_EXTENSIONS)
            .pick_file()
        else {
            return;
        };
        match compose::load_image(&path) {
            Ok(image) => {
                let size = egui::vec2(image.width() as f32, image.height() as f32);
                self.assets.insert(&path, image);
                let id = self.project.add_image(path, size, self.frame);
                self.select(Some(id));
            }
            Err(err) => self.status = Some(format!("Couldn't import {}: {err}", path.display())),
        }
    }

    pub fn delete_selection(&mut self) {
        if let Some(key) = self.selected_key.take() {
            if let Some(layer) = self.project.layer_mut(key.layer) {
                layer.remove_keys_at(key.frame);
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

    // ---- Screens and files ----------------------------------------------

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

    /// Gets ready to close the open project: saves it if it has a file,
    /// otherwise asks before discarding changes. Returns whether to go ahead.
    fn leave_current(&mut self) -> bool {
        if self.screen != Screen::Editor || !self.is_dirty() {
            return true;
        }
        if let Some(path) = self.path.clone() {
            self.save_to(&path);
        }
        self.confirm_discard()
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
        self.dirty_since = None;
        self.assets.clear();
        self.viewport = ViewportState::default();
        self.timeline = TimelineState::default();
    }

    /// Opens the "new project" dialog.
    pub fn new_project(&mut self) {
        self.new_project_form = Some(ProjectSettings::default());
    }

    /// Creates a project from the dialog's settings, saves it to the projects
    /// folder and opens it in the editor.
    pub fn create_project(&mut self, settings: &ProjectSettings) {
        if !self.leave_current() {
            return;
        }
        let project = settings.build();
        let path = recent::projects_dir().and_then(|dir| {
            std::fs::create_dir_all(&dir).ok()?;
            Some(recent::unique_project_path(
                &dir,
                &project.name,
                PROJECT_EXTENSION,
            ))
        });
        self.replace_project(project, None);
        match path {
            Some(path) => self.save_to(&path),
            None => {
                self.status = Some(
                    "Couldn't create the projects folder; use Save as to keep this project.".into(),
                )
            }
        }
        self.screen = Screen::Editor;
    }

    pub fn go_home(&mut self) {
        if self.leave_current() {
            self.screen = Screen::Home;
            self.playing = false;
            self.refresh_home();
        }
    }

    /// Reloads the recent list and the projects it points to. Files that no
    /// longer exist are dropped from the list.
    pub fn refresh_home(&mut self) {
        let before = self.recent.entries.len();
        self.recent.entries.retain(|e| e.path.is_file());
        if self.recent.entries.len() != before {
            self.recent.save();
        }
        self.home_items = self
            .recent
            .entries
            .iter()
            .map(|entry| HomeItem {
                entry: entry.clone(),
                project: std::fs::read_to_string(&entry.path)
                    .ok()
                    .and_then(|json| Project::from_json(&json).ok()),
                thumbnail: None,
            })
            .collect();
    }

    pub fn forget_recent(&mut self, path: &Path) {
        self.recent.remove(path);
        self.recent.save();
        self.refresh_home();
    }

    fn remember_recent(&mut self, path: &Path) {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        self.recent.touch(&path, recent::now_secs());
        self.recent.save();
    }

    pub fn open_dialog(&mut self) {
        if !self.leave_current() {
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
                self.remember_recent(path);
                self.status = None;
                self.screen = Screen::Editor;
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
                self.dirty_since = None;
                self.status = None;
                self.remember_recent(path);
            }
            Err(err) => self.status = Some(format!("Couldn't save {}: {err}", path.display())),
        }
    }

    // ---- Per-frame plumbing ---------------------------------------------

    fn handle_shortcuts(&mut self, ctx: &Context) {
        let ctrl = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        let ctrl_shift = |key| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, key);
        let shortcut = |s: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&s));

        if self.screen == Screen::Editor {
            if shortcut(ctrl_shift(Key::S)) {
                self.save_as();
            }
            if shortcut(ctrl(Key::S)) {
                self.save();
            }
        }
        if shortcut(ctrl(Key::O)) {
            self.open_dialog();
        }
        if shortcut(ctrl(Key::N)) {
            self.new_project();
        }
        if self.screen != Screen::Editor || self.new_project_form.is_some() {
            return;
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

    /// Saves the project shortly after the last edit, once no drag or text
    /// entry is in progress, like Alight Motion does. Projects without a file
    /// (only possible if the projects folder couldn't be created) are skipped.
    fn autosave(&mut self, ctx: &Context) {
        if !self.is_dirty() {
            self.dirty_since = None;
            return;
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        let now = ctx.input(|i| i.time);
        let since = *self.dirty_since.get_or_insert(now);
        let busy = ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input();
        if now - since >= AUTOSAVE_DELAY && !busy {
            self.save_to(&path);
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(AUTOSAVE_DELAY / 2.0));
        }
    }

    fn handle_close(&mut self, ctx: &Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.leave_current() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        }
    }

    fn update_title(&mut self, ctx: &Context) {
        let title = if self.screen == Screen::Home {
            "AetherEase".to_owned()
        } else {
            self.editor_title()
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    fn editor_title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.project.name.clone());
        let dirty = if self.is_dirty() { "*" } else { "" };
        format!("{name}{dirty} - AetherEase")
    }
}

impl eframe::App for AetherApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_close(&ctx);
        self.handle_shortcuts(&ctx);

        match self.screen {
            Screen::Home => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.fill(ctx.global_style().visuals.extreme_bg_color))
                    .show(ui, |ui| self.home_ui(ui));
            }
            Screen::Editor => {
                self.advance_playback(&ctx);
                self.editor_ui(ui, &ctx);
                self.commit_history(&ctx);
                self.autosave(&ctx);
            }
        }
        self.new_project_dialog(&ctx);
        self.update_title(&ctx);
    }
}

impl AetherApp {
    fn editor_ui(&mut self, ui: &mut egui::Ui, ctx: &Context) {
        let ctx = ctx.clone();
        egui::Panel::top("top_bar")
            .frame(theme::top_bar_frame(&ctx))
            .exact_size(48.0)
            .show_separator_line(false)
            .show(ui, |ui| self.top_bar_ui(ui));
        egui::Panel::bottom("timeline")
            .resizable(true)
            .show_separator_line(false)
            .default_size(320.0)
            .size_range(160.0..=800.0)
            .frame(theme::panel_frame(&ctx))
            .show(ui, |ui| self.timeline_ui(ui));
        egui::Panel::right("inspector")
            .resizable(true)
            .show_separator_line(false)
            .default_size(330.0)
            .size_range(260.0..=520.0)
            .frame(theme::panel_frame(&ctx))
            .show(ui, |ui| self.inspector_ui(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.viewport_ui(ui));
    }
}
