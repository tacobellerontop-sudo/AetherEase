//! Exporting a project as video, GIF or numbered PNG frames.
//!
//! Frames come from the same compositor as the canvas. Video goes through
//! ffmpeg (found on the PATH or next to the app), which also mixes in the
//! audio layers. GIF and PNG need nothing extra.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::compose::{self, Assets};
use crate::model::{LayerKind, Project};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Mp4,
    Gif,
    PngSequence,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Mp4, Format::Gif, Format::PngSequence];

    pub fn label(self) -> &'static str {
        match self {
            Format::Mp4 => "MP4 video",
            Format::Gif => "GIF",
            Format::PngSequence => "PNG frames",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Format::Mp4 => "H.264 with sound",
            Format::Gif => "Loops, no sound",
            Format::PngSequence => "One image per frame",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub format: Format,
    /// Output size relative to the project's.
    pub scale: f32,
}

/// The ffmpeg program to use, if one can be found.
pub fn find_ffmpeg() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let beside_app = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .filter(|p| p.is_file());
    let candidates = beside_app.into_iter().chain([PathBuf::from(name)]);
    candidates.into_iter().find(|p| {
        Command::new(p)
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// A running export, shared with the background thread doing the work.
pub struct Job {
    pub total: u32,
    pub done: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Result<PathBuf, String>>>>,
}

impl Job {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// The outcome, once the export has finished.
    pub fn result(&self) -> Option<Result<PathBuf, String>> {
        self.result.lock().ok()?.clone()
    }
}

/// Starts exporting `project` to `out` on a background thread.
pub fn start(
    project: Project,
    settings: Settings,
    out: PathBuf,
    ffmpeg: Option<PathBuf>,
    ctx: egui::Context,
) -> Job {
    let job = Job {
        total: project.duration.max(1) as u32,
        done: Arc::default(),
        cancel: Arc::default(),
        result: Arc::default(),
    };
    let (done, cancel, result) = (job.done.clone(), job.cancel.clone(), job.result.clone());
    std::thread::spawn(move || {
        let progress = |n: u32| {
            done.store(n, Ordering::Relaxed);
            ctx.request_repaint();
        };
        let outcome = match settings.format {
            Format::Mp4 => match ffmpeg {
                Some(ffmpeg) => {
                    export_video(&project, settings.scale, &out, &ffmpeg, &cancel, progress)
                }
                None => Err("ffmpeg wasn't found".into()),
            },
            Format::Gif => export_gif(&project, settings.scale, &out, &cancel, progress),
            Format::PngSequence => export_png(&project, settings.scale, &out, &cancel, progress),
        };
        let outcome = outcome.map(|()| out);
        if let Ok(mut slot) = result.lock() {
            *slot = Some(outcome);
        }
        ctx.request_repaint();
    });
    job
}

/// Renders every frame in order, handing each to `sink`.
fn for_each_frame(
    project: &Project,
    scale: f32,
    cancel: &AtomicBool,
    progress: impl Fn(u32),
    mut sink: impl FnMut(i32, tiny_skia::Pixmap) -> Result<(), String>,
) -> Result<(), String> {
    let mut assets = Assets::default();
    for frame in 0..project.duration {
        if cancel.load(Ordering::Relaxed) {
            return Err("Export cancelled".into());
        }
        let pixmap = compose::render(project, frame, scale, &mut assets);
        sink(frame, pixmap)?;
        progress(frame as u32 + 1);
    }
    Ok(())
}

/// Frames are opaque (the background is always drawn), so premultiplied
/// pixels are already the straight RGBA that encoders expect.
fn rgba(pixmap: tiny_skia::Pixmap) -> image::RgbaImage {
    let (w, h) = (pixmap.width(), pixmap.height());
    image::RgbaImage::from_raw(w, h, pixmap.take()).expect("buffer matches size")
}

fn export_png(
    project: &Project,
    scale: f32,
    out: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(u32),
) -> Result<(), String> {
    // `out` is the first file; the rest are numbered alongside it.
    let dir = out.parent().unwrap_or(Path::new("."));
    let stem = out
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "frame".into());
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for_each_frame(project, scale, cancel, progress, |frame, pixmap| {
        let path = dir.join(format!("{stem}_{:05}.png", frame + 1));
        rgba(pixmap).save(&path).map_err(|e| e.to_string())
    })
}

fn export_gif(
    project: &Project,
    scale: f32,
    out: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(u32),
) -> Result<(), String> {
    use image::codecs::gif::{GifEncoder, Repeat};
    let file = std::fs::File::create(out).map_err(|e| e.to_string())?;
    let mut encoder = GifEncoder::new_with_speed(std::io::BufWriter::new(file), 10);
    encoder
        .set_repeat(Repeat::Infinite)
        .map_err(|e| e.to_string())?;
    let delay = image::Delay::from_numer_denom_ms(1000, project.fps.max(1));
    for_each_frame(project, scale, cancel, progress, |_, pixmap| {
        let frame = image::Frame::from_parts(rgba(pixmap), 0, 0, delay);
        encoder.encode_frame(frame).map_err(|e| e.to_string())
    })
}

fn export_video(
    project: &Project,
    scale: f32,
    out: &Path,
    ffmpeg: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(u32),
) -> Result<(), String> {
    let probe = compose::render(project, 0, scale, &mut Assets::default());
    let (w, h) = (probe.width(), probe.height());
    let args = ffmpeg_args(project, w, h, out);
    let mut child = Command::new(ffmpeg)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't start ffmpeg: {e}"))?;
    // Drain ffmpeg's log on its own thread so it can never block on a full
    // pipe; keep it for the error message.
    let mut stderr = child.stderr.take().expect("piped");
    let log = std::thread::spawn(move || {
        let mut text = String::new();
        stderr.read_to_string(&mut text).ok();
        text
    });
    let mut stdin = child.stdin.take().expect("piped");
    let written = for_each_frame(project, scale, cancel, progress, |_, pixmap| {
        stdin
            .write_all(pixmap.data())
            .map_err(|e| format!("ffmpeg stopped reading frames: {e}"))
    });
    drop(stdin);
    if written.is_err() {
        child.kill().ok();
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let log = log.join().unwrap_or_default();
    written?;
    if status.success() {
        Ok(())
    } else {
        let tail: Vec<&str> = log.lines().rev().take(3).collect();
        Err(format!(
            "ffmpeg failed: {}",
            tail.into_iter().rev().collect::<Vec<_>>().join(" / ")
        ))
    }
}

/// ffmpeg arguments: raw frames on stdin, each audio layer as an input
/// trimmed and delayed into place, all mixed together.
pub fn ffmpeg_args(project: &Project, width: u32, height: u32, out: &Path) -> Vec<String> {
    let fps = project.fps.max(1);
    let seconds = |frames: i32| frames as f32 / fps as f32;
    let mut args: Vec<String> = [
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-s",
        &format!("{width}x{height}"),
        "-framerate",
        &fps.to_string(),
        "-i",
        "-",
    ]
    .map(String::from)
    .to_vec();

    let clips: Vec<_> = project
        .layers
        .iter()
        .filter(|l| project.is_shown_at(l, l.in_frame) && l.in_frame < project.duration)
        .filter_map(|l| match &l.kind {
            LayerKind::Audio {
                path,
                start,
                volume,
            } => Some((path, *start, *volume, l.in_frame.max(0), l.out_frame)),
            _ => None,
        })
        .collect();
    let mut filters = Vec::new();
    for (i, (path, start, volume, in_frame, out_frame)) in clips.iter().enumerate() {
        args.extend(["-i".into(), path.display().to_string()]);
        let offset = seconds((in_frame - start).max(0));
        let length = seconds(out_frame - in_frame);
        let delay_ms = (seconds(*in_frame) * 1000.0).round() as i64;
        filters.push(format!(
            "[{input}:a]atrim=start={offset:.4}:duration={length:.4},asetpts=PTS-STARTPTS,\
             adelay={delay_ms}:all=1,volume={volume:.3}[a{i}]",
            input = i + 1,
        ));
    }
    // yuv420p needs even dimensions.
    let video = "pad=ceil(iw/2)*2:ceil(ih/2)*2";
    if clips.is_empty() {
        args.extend(["-vf".into(), video.into()]);
    } else {
        let inputs: String = (0..clips.len()).map(|i| format!("[a{i}]")).collect();
        filters.push(format!(
            "{inputs}amix=inputs={}:normalize=0:duration=longest[aout]",
            clips.len()
        ));
        filters.push(format!("[0:v]{video}[vout]"));
        args.extend([
            "-filter_complex".into(),
            filters.join(";"),
            "-map".into(),
            "[vout]".into(),
            "-map".into(),
            "[aout]".into(),
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "192k".into(),
        ]);
    }
    args.extend(
        [
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
            "-t",
            &format!("{:.4}", seconds(project.duration)),
        ]
        .map(String::from),
    );
    args.push(out.display().to_string());
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ShapeKind;

    fn tiny_project() -> Project {
        let mut project = Project {
            width: 64,
            height: 36,
            fps: 10,
            duration: 5,
            ..Project::default()
        };
        project.add_shape(ShapeKind::Ellipse, 0);
        project
    }

    #[test]
    fn audio_layers_are_placed_in_the_mix() {
        let mut project = tiny_project();
        let id = project.add_audio("song.mp3".into(), 3.0, 2);
        project.layer_mut(id).unwrap().in_frame = 4;
        let args = ffmpeg_args(&project, 64, 36, Path::new("out.mp4"));
        let filter = &args[args.iter().position(|a| a == "-filter_complex").unwrap() + 1];
        // Trimmed by 2 frames (0.2 s) and delayed to frame 4 (400 ms).
        assert!(filter.contains("atrim=start=0.2000"), "{filter}");
        assert!(filter.contains("adelay=400:all=1"), "{filter}");
        assert!(filter.contains("amix=inputs=1"), "{filter}");
        assert!(args.contains(&"song.mp3".to_string()));
    }

    #[test]
    fn exports_png_frames_and_gif() {
        let dir = std::env::temp_dir().join(format!("aether-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let project = tiny_project();
        let cancel = AtomicBool::new(false);
        export_png(&project, 1.0, &dir.join("shot.png"), &cancel, |_| {}).unwrap();
        assert!(dir.join("shot_00001.png").is_file());
        assert!(dir.join("shot_00005.png").is_file());
        export_gif(&project, 0.5, &dir.join("loop.gif"), &cancel, |_| {}).unwrap();
        let gif = std::fs::read(dir.join("loop.gif")).unwrap();
        assert!(gif.starts_with(b"GIF89a"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn exports_video_when_ffmpeg_is_available() {
        let Some(ffmpeg) = find_ffmpeg() else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("aether-video-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("clip.mp4");
        let tone = dir.join("tone.wav");
        crate::audio::tests::write_tone(&tone, 1.0);
        let mut project = tiny_project();
        project.add_audio(tone, 1.0, 1);
        let cancel = AtomicBool::new(false);
        export_video(&project, 1.0, &out, &ffmpeg, &cancel, |_| {}).unwrap();
        assert!(std::fs::metadata(&out).unwrap().len() > 0);
        // The file has both a video and an audio stream.
        let probe = Command::new(ffmpeg).arg("-i").arg(&out).output().unwrap();
        let info = String::from_utf8_lossy(&probe.stderr);
        assert!(info.contains("Video: h264"), "{info}");
        assert!(info.contains("Audio: aac"), "{info}");
        std::fs::remove_dir_all(dir).ok();
    }
}
