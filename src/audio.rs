//! Audio layers: playback in the editor and waveforms for the timeline.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::{Decoder, MixerDeviceSink, Player, Source};

use crate::model::{LayerKind, Project};

/// Audio file types offered when importing.
pub const EXTENSIONS: [&str; 6] = ["mp3", "wav", "ogg", "flac", "m4a", "aac"];

/// How many waveform peaks are kept per second of audio.
pub const PEAKS_PER_SECOND: f32 = 100.0;

/// Plays the project's audio layers in step with the playhead.
#[derive(Default)]
pub struct AudioEngine {
    device: Option<MixerDeviceSink>,
    /// Set after opening the sound device failed, so it isn't retried
    /// every time playback starts.
    unavailable: bool,
    players: Vec<Player>,
}

impl AudioEngine {
    /// Starts every audio layer from `frame`. Layers that start later are
    /// delayed; their silence is part of the source.
    pub fn play_from(&mut self, project: &Project, frame: i32) {
        self.stop();
        let fps = project.fps.max(1) as f32;
        let clips: Vec<_> = project
            .layers
            .iter()
            .filter(|l| project.is_shown_at(l, l.in_frame) && l.out_frame > frame)
            .filter_map(|l| match &l.kind {
                LayerKind::Audio {
                    path,
                    start,
                    volume,
                } => Some((path.clone(), *start, *volume, l.in_frame, l.out_frame)),
                _ => None,
            })
            .collect();
        if clips.is_empty() {
            return;
        }
        if self.device.is_none() && !self.unavailable {
            match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(mut device) => {
                    device.log_on_drop(false);
                    self.device = Some(device);
                }
                Err(_) => self.unavailable = true,
            }
        }
        let Some(device) = &self.device else {
            return;
        };
        for (path, start, volume, in_frame, out_frame) in clips {
            let Ok(file) = File::open(&path) else {
                continue;
            };
            let Ok(decoder) = Decoder::try_from(BufReader::new(file)) else {
                continue;
            };
            let from = frame.max(in_frame);
            let offset = (from - start).max(0) as f32 / fps;
            let delay = (from - frame) as f32 / fps;
            let length = (out_frame - from) as f32 / fps;
            let source = decoder
                .skip_duration(Duration::from_secs_f32(offset))
                .take_duration(Duration::from_secs_f32(length))
                .amplify(volume)
                .delay(Duration::from_secs_f32(delay));
            let player = Player::connect_new(device.mixer());
            player.append(source);
            self.players.push(player);
        }
    }

    pub fn stop(&mut self) {
        for player in self.players.drain(..) {
            player.stop();
        }
    }
}

/// A sound file's loudness over time, for drawing on the timeline.
pub struct Waveform {
    pub seconds: f32,
    /// Peak level (0..=1) in each 1/[`PEAKS_PER_SECOND`] s window.
    pub peaks: Vec<f32>,
}

/// Decodes a whole file into a [`Waveform`].
pub fn analyse(path: &Path) -> Result<Waveform, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let decoder = Decoder::try_from(BufReader::new(file)).map_err(|e| e.to_string())?;
    let channels = decoder.channels().get() as usize;
    let rate = decoder.sample_rate().get() as f32;
    let window = ((rate / PEAKS_PER_SECOND) as usize * channels).max(1);
    let mut peaks = Vec::new();
    let mut peak = 0.0_f32;
    let mut count = 0;
    let mut samples = 0usize;
    for sample in decoder {
        peak = peak.max(sample.abs());
        count += 1;
        samples += 1;
        if count == window {
            peaks.push(peak.min(1.0));
            peak = 0.0;
            count = 0;
        }
    }
    if count > 0 {
        peaks.push(peak.min(1.0));
    }
    Ok(Waveform {
        seconds: samples as f32 / channels.max(1) as f32 / rate,
        peaks,
    })
}

/// Waveforms by file, analysed on a background thread the first time
/// they're asked for.
#[derive(Default, Clone)]
pub struct Waveforms {
    cache: Arc<Mutex<HashMap<PathBuf, Option<Arc<Waveform>>>>>,
}

impl Waveforms {
    /// The waveform if it's ready; starts analysing it otherwise.
    pub fn get(&self, path: &Path, ctx: &egui::Context) -> Option<Arc<Waveform>> {
        let mut cache = self.cache.lock().ok()?;
        if let Some(entry) = cache.get(path) {
            return entry.clone();
        }
        cache.insert(path.to_owned(), None);
        let (cache, path, ctx) = (self.cache.clone(), path.to_owned(), ctx.clone());
        std::thread::spawn(move || {
            if let Ok(waveform) = analyse(&path)
                && let Ok(mut cache) = cache.lock()
            {
                cache.insert(path, Some(Arc::new(waveform)));
                ctx.request_repaint();
            }
        });
        None
    }

    pub fn insert(&self, path: &Path, waveform: Waveform) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(path.to_owned(), Some(Arc::new(waveform)));
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Writes a mono 16-bit WAV of `seconds` of a 440 Hz tone at half volume.
    pub fn write_tone(path: &Path, seconds: f32) {
        let rate = 8000u32;
        let n = (rate as f32 * seconds) as u32;
        let mut data = Vec::new();
        for i in 0..n {
            let v = (i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 0.5;
            data.extend_from_slice(&((v * i16::MAX as f32) as i16).to_le_bytes());
        }
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        std::fs::write(path, wav).unwrap();
    }

    #[test]
    fn analyses_a_wav_file() {
        let dir = std::env::temp_dir().join(format!("aether-audio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        write_tone(&path, 1.5);
        let wave = analyse(&path).unwrap();
        assert!((wave.seconds - 1.5).abs() < 0.01, "{}", wave.seconds);
        assert_eq!(wave.peaks.len(), 150);
        assert!(wave.peaks.iter().all(|&p| (0.45..=0.51).contains(&p)));
        std::fs::remove_dir_all(dir).ok();
    }
}
