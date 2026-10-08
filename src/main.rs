//! AetherEase: a layer-and-keyframe motion graphics editor.

// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod audio;
mod compose;
mod export;
mod history;
mod model;
mod path;
mod recent;
mod render;
mod text;
mod ui;

fn main() -> eframe::Result {
    // A project file can be passed on the command line (e.g. by "Open with").
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AetherEase")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "AetherEase",
        options,
        Box::new(|cc| Ok(Box::new(app::AetherApp::new(cc, path)))),
    )
}
