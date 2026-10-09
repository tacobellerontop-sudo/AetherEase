//! Video clip layers: probing files and decoding frames through ffmpeg.
//!
//! Frames are streamed from an ffmpeg process as raw RGBA. Each file keeps
//! one decoder positioned where it stopped, so playback and export read
//! frames in order without seeking; jumping elsewhere restarts it there.

use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Stdio};

use crate::export::quiet_command;
use std::sync::{Arc, OnceLock};

use tiny_skia::{IntSize, Pixmap};

/// Video file types offered when importing.
pub const EXTENSIONS: [&str; 7] = ["mp4", "mov", "m4v", "mkv", "webm", "avi", "gif"];

/// Frames are decoded no larger than this on their long side.
const MAX_DECODE: u32 = 1920;

/// How many decoded frames each file keeps around.
const CACHE_PER_FILE: usize = 6;

/// Reading ahead this many frames is cheaper than restarting ffmpeg.
const MAX_SKIP: i64 = 45;

/// The ffmpeg program, looked up once.
pub fn ffmpeg() -> Option<&'static Path> {
    static FFMPEG: OnceLock<Option<PathBuf>> = OnceLock::new();
    FFMPEG.get_or_init(crate::export::find_ffmpeg).as_deref()
}

/// What a video file holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    /// Frames per second.
    pub rate: f32,
    pub seconds: f32,
    pub has_audio: bool,
}

/// Reads a video's size, frame rate, length and whether it has sound.
pub fn probe(path: &Path) -> Result<VideoInfo, String> {
    let ffmpeg = ffmpeg().ok_or("video needs ffmpeg, which wasn't found")?;
    let output = quiet_command(ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    // With no output file ffmpeg exits with an error after describing the input.
    parse_probe(&String::from_utf8_lossy(&output.stderr))
}

fn parse_probe(text: &str) -> Result<VideoInfo, String> {
    let video = text
        .lines()
        .find(|l| l.contains("Stream #") && l.contains("Video:"))
        .ok_or("no video stream found")?;
    let (width, height) = video
        .split([',', ' '])
        .filter_map(|word| {
            let (w, h) = word.split_once('x')?;
            Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
        })
        .find(|&(w, h)| w > 0 && h > 0)
        .ok_or("couldn't read the video size")?;
    let number_before = |unit: &str| {
        video.split(',').find_map(|part| {
            part.trim()
                .strip_suffix(unit)
                .and_then(|n| n.trim().parse::<f32>().ok())
        })
    };
    let rate = number_before(" fps")
        .or_else(|| number_before(" tbr"))
        .filter(|r| *r > 0.0)
        .unwrap_or(30.0);
    let seconds = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("Duration: "))
        .and_then(|rest| rest.split(',').next())
        .and_then(parse_timestamp)
        .ok_or("couldn't read the video length")?;
    let has_audio = text
        .lines()
        .any(|l| l.contains("Stream #") && l.contains("Audio:"));
    Ok(VideoInfo {
        width,
        height,
        rate,
        seconds,
        has_audio,
    })
}

/// `HH:MM:SS.ss` to seconds.
fn parse_timestamp(s: &str) -> Option<f32> {
    let mut parts = s.trim().split(':');
    let h: f32 = parts.next()?.parse().ok()?;
    let m: f32 = parts.next()?.parse().ok()?;
    let sec: f32 = parts.next()?.parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + sec)
}

/// The video's sound as a WAV file the audio engine can play, extracted
/// once with ffmpeg and kept in the temp folder. (The audio decoder can't
/// skip a video's picture track itself.)
pub fn audio_track(path: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(path).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    meta.modified().ok().hash(&mut hasher);
    let dir = std::env::temp_dir().join("aetherease-audio");
    let out = dir.join(format!("{:016x}.wav", hasher.finish()));
    if out.is_file() {
        return Some(out);
    }
    std::fs::create_dir_all(&dir).ok()?;
    // Write under another name first so a half-written file is never used.
    let partial = out.with_extension("partial.wav");
    let ok = quiet_command(ffmpeg()?)
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "2", "-ar", "48000"])
        .arg(&partial)
        .stdin(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    (ok && std::fs::rename(&partial, &out).is_ok()).then_some(out)
}

/// The size frames of a `width`×`height` video are decoded at.
fn decode_size(width: u32, height: u32) -> (u32, u32) {
    let fit = (MAX_DECODE as f32 / width.max(height) as f32).min(1.0);
    let even = |v: u32| ((v as f32 * fit).round() as u32 / 2 * 2).max(2);
    (even(width), even(height))
}

/// An ffmpeg process streaming frames from `next` onwards.
struct Decoder {
    child: Child,
    stdout: ChildStdout,
    next: i64,
    size: (u32, u32),
}

impl Decoder {
    fn start(path: &Path, from: i64, rate: f32, size: (u32, u32)) -> Option<Decoder> {
        let mut child = quiet_command(ffmpeg()?)
            .args(["-hide_banner", "-loglevel", "error", "-ss"])
            .arg(format!("{:.4}", from as f32 / rate))
            .arg("-i")
            .arg(path)
            .args(["-an", "-vf"])
            .arg(format!("scale={}:{}", size.0, size.1))
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let stdout = child.stdout.take()?;
        Some(Decoder {
            child,
            stdout,
            next: from,
            size,
        })
    }

    /// The next frame, premultiplied, or `None` at the end of the file.
    fn read(&mut self) -> Option<Pixmap> {
        let (w, h) = self.size;
        let mut data = vec![0u8; w as usize * h as usize * 4];
        self.stdout.read_exact(&mut data).ok()?;
        self.next += 1;
        for px in data.chunks_exact_mut(4) {
            let a = px[3] as u16;
            if a < 255 {
                for c in &mut px[..3] {
                    *c = ((*c as u16 * a + 127) / 255) as u8;
                }
            }
        }
        Pixmap::from_vec(data, IntSize::from_wh(w, h)?)
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Decoders and recently decoded frames, per file.
#[derive(Default)]
pub struct Videos {
    decoders: HashMap<PathBuf, Decoder>,
    frames: HashMap<PathBuf, VecDeque<(i64, Arc<Pixmap>)>>,
}

impl Videos {
    /// Frame `index` of the video at `path` (a `width`×`height` file at
    /// `rate` fps with `count` frames). Indices past either end show the
    /// first or last frame.
    pub fn frame(
        &mut self,
        path: &Path,
        index: i64,
        rate: f32,
        count: i64,
        width: u32,
        height: u32,
    ) -> Option<Arc<Pixmap>> {
        let index = index.clamp(0, (count - 1).max(0));
        let cached = self.frames.entry(path.to_owned()).or_default();
        if let Some((_, f)) = cached.iter().find(|(i, _)| *i == index) {
            return Some(f.clone());
        }
        let size = decode_size(width, height);
        let reusable = self
            .decoders
            .get(path)
            .is_some_and(|d| d.size == size && index >= d.next && index - d.next <= MAX_SKIP);
        if !reusable {
            self.decoders.remove(path);
            let decoder = Decoder::start(path, index, rate.max(1.0), size)?;
            self.decoders.insert(path.to_owned(), decoder);
        }
        let decoder = self.decoders.get_mut(path)?;
        let mut frame = None;
        while decoder.next <= index {
            match decoder.read() {
                Some(f) => frame = Some(f),
                // Ran off the end: keep the last frame that came out.
                None => break,
            }
        }
        let cached = self.frames.entry(path.to_owned()).or_default();
        let Some(frame) = frame else {
            // The file ended early (its length is rounded): show the last
            // frame there is.
            self.decoders.remove(path);
            return cached
                .iter()
                .max_by_key(|(i, _)| *i)
                .map(|(_, f)| f.clone());
        };
        let frame = Arc::new(frame);
        if cached.len() >= CACHE_PER_FILE {
            cached.pop_front();
        }
        cached.push_back((index, frame.clone()));
        Some(frame)
    }

    pub fn clear(&mut self) {
        self.decoders.clear();
        self.frames.clear();
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::process::Command;

    /// Writes a short test clip (moving test pattern plus a tone) with
    /// ffmpeg, or returns `None` when ffmpeg isn't installed.
    pub fn write_clip(dir: &Path, seconds: f32) -> Option<PathBuf> {
        let ffmpeg = ffmpeg()?;
        std::fs::create_dir_all(dir).ok()?;
        let out = dir.join("clip.mp4");
        let status = Command::new(ffmpeg)
            .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i"])
            .arg(format!("testsrc=size=320x240:rate=25:duration={seconds}"))
            .args(["-f", "lavfi", "-i"])
            .arg(format!("sine=frequency=440:duration={seconds}"))
            .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"])
            .arg(&out)
            .status()
            .ok()?;
        status.success().then_some(out)
    }

    #[test]
    fn parses_ffmpeg_descriptions() {
        let text = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'a.mp4':\n  \
            Duration: 00:01:02.50, start: 0.000000, bitrate: 900 kb/s\n  \
            Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(progressive), 1280x720 [SAR 1:1 DAR 16:9], 800 kb/s, 29.97 fps, 29.97 tbr, 30k tbn (default)\n  \
            Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 44100 Hz, stereo, fltp, 128 kb/s (default)\n";
        let info = parse_probe(text).unwrap();
        assert_eq!((info.width, info.height), (1280, 720));
        assert!((info.rate - 29.97).abs() < 1e-3);
        assert!((info.seconds - 62.5).abs() < 1e-3);
        assert!(info.has_audio);
    }

    #[test]
    fn decodes_frames_in_order_and_after_jumps() {
        let dir = std::env::temp_dir().join(format!("aether-clip-{}", std::process::id()));
        let Some(clip) = write_clip(&dir, 2.0) else {
            return;
        };
        let info = probe(&clip).unwrap();
        assert_eq!((info.width, info.height), (320, 240));
        assert_eq!(info.rate, 25.0);
        assert!(info.has_audio);
        let count = (info.seconds * info.rate) as i64;
        let mut videos = Videos::default();
        let mut get = |i| videos.frame(&clip, i, info.rate, count, 320, 240).unwrap();
        let first = get(0);
        assert_eq!((first.width(), first.height()), (320, 240));
        let second = get(1);
        assert_ne!(first.data(), second.data(), "the test pattern moves");
        let third = get(2);
        // A jump back restarts the decoder at the right frame.
        let late = get(40);
        assert_ne!(late.data(), third.data());
        let mut fresh = Videos::default();
        let again = fresh.frame(&clip, 2, info.rate, count, 320, 240).unwrap();
        assert_eq!(third.data(), again.data());
        let mut jumped = Videos::default();
        jumped.frame(&clip, 40, info.rate, count, 320, 240).unwrap();
        let back = jumped.frame(&clip, 2, info.rate, count, 320, 240).unwrap();
        assert_eq!(third.data(), back.data());
        // Past the end shows the last frame.
        assert!(get(10_000).width() > 0);

        // The sound track plays through the audio engine like any sound file.
        let wav = audio_track(&clip).unwrap();
        let wave = crate::audio::analyse(&wav).unwrap();
        assert!((wave.seconds - 2.0).abs() < 0.1, "{}", wave.seconds);

        // And the compositor shows the clip's picture.
        let mut project = crate::model::Project {
            background: crate::model::Color::BLACK,
            ..Default::default()
        };
        project.add_video(clip.clone(), info, 0);
        let out = crate::compose::render(&project, 10, 0.25, &mut Default::default());
        let px = out.pixel(240, 135).unwrap();
        assert!(px.red() as u32 + px.green() as u32 + px.blue() as u32 > 60);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
