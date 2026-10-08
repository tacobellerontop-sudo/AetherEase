//! The export dialog: pick a format and size, then watch the progress.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use egui::{Id, Modal, RichText, Ui, vec2};

use crate::app::AetherApp;
use crate::export::{self, Format, Job, Settings};
use crate::ui::{icons, theme};

const SCALES: [(&str, f32); 4] = [("100%", 1.0), ("75%", 0.75), ("50%", 0.5), ("25%", 0.25)];

/// The dialog's state while it is open.
pub struct ExportDialog {
    settings: Settings,
    ffmpeg: Option<PathBuf>,
    job: Option<Job>,
    outcome: Option<Result<PathBuf, String>>,
}

impl ExportDialog {
    pub fn new() -> Self {
        let ffmpeg = export::find_ffmpeg();
        Self {
            settings: Settings {
                format: if ffmpeg.is_some() {
                    Format::Mp4
                } else {
                    Format::Gif
                },
                scale: 1.0,
            },
            ffmpeg,
            job: None,
            outcome: None,
        }
    }
}

impl AetherApp {
    pub fn open_export(&mut self) {
        self.playing = false;
        self.export = Some(ExportDialog::new());
    }

    pub fn export_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.export.take() else {
            return;
        };
        let mut close = false;
        let modal = Modal::new(Id::new("export_dialog")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new("Export").size(20.0).strong());
            ui.add_space(10.0);
            match (&dialog.job, &dialog.outcome) {
                (_, Some(outcome)) => close = self.outcome_ui(ui, outcome),
                (Some(job), None) => {
                    let done = job.done.load(Ordering::Relaxed);
                    ui.label(format!("Rendering frame {done} of {}…", job.total));
                    ui.add(
                        egui::ProgressBar::new(done as f32 / job.total as f32)
                            .fill(theme::ACCENT)
                            .show_percentage(),
                    );
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Cancel").clicked() {
                            job.cancel();
                        }
                    });
                    if let Some(result) = job.result() {
                        dialog.outcome = Some(result);
                        dialog.job = None;
                    }
                }
                (None, None) => {
                    if let Some(start) = self.export_form(ui, &mut dialog) {
                        dialog.job = Some(start);
                    } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                    if ui.data(|d| d.get_temp::<bool>(Id::new("export_cancel"))) == Some(true) {
                        ui.data_mut(|d| d.remove::<bool>(Id::new("export_cancel")));
                        close = true;
                    }
                }
            }
        });
        let busy = dialog.job.is_some();
        if !close && (busy || !modal.should_close()) {
            self.export = Some(dialog);
        }
    }

    /// Format and size choices; returns the job once the user picks a file.
    fn export_form(&mut self, ui: &mut Ui, dialog: &mut ExportDialog) -> Option<Job> {
        let tile = vec2(130.0, 64.0);
        ui.horizontal(|ui| {
            for format in Format::ALL {
                let enabled = format != Format::Mp4 || dialog.ffmpeg.is_some();
                let icon = match format {
                    Format::Mp4 => icons::Icon::Play,
                    Format::Gif => icons::Icon::Loop,
                    Format::PngSequence => icons::Icon::Image,
                };
                let response = ui
                    .add_enabled_ui(enabled, |ui| {
                        icons::tile(
                            ui,
                            icon,
                            format.label(),
                            dialog.settings.format == format,
                            tile,
                        )
                    })
                    .inner
                    .on_hover_text(format.detail());
                if response.clicked() {
                    dialog.settings.format = format;
                }
            }
        });
        if dialog.ffmpeg.is_none() {
            ui.label(
                RichText::new(
                    "MP4 needs ffmpeg. Install it, or put ffmpeg.exe next to AetherEase.",
                )
                .small()
                .weak(),
            );
        }
        ui.add_space(10.0);
        ui.label(RichText::new("Size").weak());
        ui.horizontal(|ui| {
            for (label, scale) in SCALES {
                let (w, h) = (
                    (self.project.width as f32 * scale).round(),
                    (self.project.height as f32 * scale).round(),
                );
                ui.selectable_value(&mut dialog.settings.scale, scale, label)
                    .on_hover_text(format!("{w} × {h} px"));
            }
        });
        ui.add_space(6.0);
        let seconds = self.project.duration as f32 / self.project.fps.max(1) as f32;
        let (w, h) = (
            (self.project.width as f32 * dialog.settings.scale).round(),
            (self.project.height as f32 * dialog.settings.scale).round(),
        );
        ui.label(
            RichText::new(format!(
                "{w} × {h} px · {} fps · {seconds:.1} s",
                self.project.fps
            ))
            .weak(),
        );
        ui.add_space(14.0);
        let mut start = None;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let button =
                egui::Button::new(RichText::new("Export").strong()).fill(theme::ACCENT_SOFT);
            if ui.add(button).clicked() {
                start = Some(());
            }
            if ui.button("Cancel").clicked() {
                ui.data_mut(|d| d.insert_temp(Id::new("export_cancel"), true));
            }
        });
        start?;
        let (extension, filter) = match dialog.settings.format {
            Format::Mp4 => ("mp4", "MP4 video"),
            Format::Gif => ("gif", "GIF"),
            Format::PngSequence => ("png", "PNG images"),
        };
        let name = format!("{}.{extension}", self.project.name);
        let out = rfd::FileDialog::new()
            .add_filter(filter, &[extension])
            .set_file_name(name)
            .save_file()?;
        let out = if out.extension().is_none() {
            out.with_extension(extension)
        } else {
            out
        };
        Some(export::start(
            self.project.clone(),
            dialog.settings.clone(),
            out,
            dialog.ffmpeg.clone(),
            ui.ctx().clone(),
        ))
    }

    /// Shows how the export went; returns true when the dialog should close.
    fn outcome_ui(&mut self, ui: &mut Ui, outcome: &Result<PathBuf, String>) -> bool {
        match outcome {
            Ok(path) => {
                ui.label(RichText::new("Done").strong().color(theme::ACCENT));
                ui.label(path.display().to_string());
            }
            Err(err) => {
                ui.colored_label(theme::PLAYHEAD, err);
            }
        }
        ui.add_space(10.0);
        let mut close = false;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                close = true;
            }
        });
        close || ui.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter))
    }
}
