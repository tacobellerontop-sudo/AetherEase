//! Per-layer effects, applied in list order to the layer's rendered image.

use serde::{Deserialize, Serialize};

use super::{Animated, Color, KeyTrack};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub enabled: bool,
    pub kind: EffectKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EffectKind {
    /// Gaussian blur of the whole layer.
    Blur { radius: Animated<f32> },
    /// A blurred, offset copy of the layer's silhouette behind it.
    Shadow {
        color: Animated<Color>,
        /// Offset in pixels.
        distance: Animated<f32>,
        /// Direction of the offset in degrees; 90 casts straight down.
        angle: Animated<f32>,
        blur: Animated<f32>,
    },
    /// A soft halo of colour around the layer.
    Glow {
        color: Animated<Color>,
        radius: Animated<f32>,
        /// Multiplies the halo's opacity; above 1 makes it denser.
        strength: Animated<f32>,
    },
    /// Brightness, contrast, saturation and hue.
    AdjustColor {
        /// Added to every channel: -1 to 1.
        brightness: Animated<f32>,
        /// 1 leaves contrast alone; 0 is flat grey.
        contrast: Animated<f32>,
        /// 1 leaves colour alone; 0 is black and white.
        saturation: Animated<f32>,
        /// Hue rotation in degrees.
        hue: Animated<f32>,
    },
}

impl Effect {
    pub const PRESETS: [&'static str; 4] = ["Blur", "Drop shadow", "Glow", "Adjust color"];
    /// The index of "Adjust color" in [`Effect::PRESETS`].
    pub const ADJUST_COLOR: usize = 3;

    /// A new effect by its index in [`Effect::PRESETS`].
    pub fn preset(index: usize) -> Effect {
        let kind = match index {
            0 => EffectKind::Blur {
                radius: Animated::new(12.0),
            },
            1 => EffectKind::Shadow {
                color: Animated::new(Color::new(0.0, 0.0, 0.0, 0.6)),
                distance: Animated::new(18.0),
                angle: Animated::new(90.0),
                blur: Animated::new(16.0),
            },
            2 => EffectKind::Glow {
                color: Animated::new(Color::new(1.0, 0.85, 0.4, 1.0)),
                radius: Animated::new(24.0),
                strength: Animated::new(1.5),
            },
            _ => EffectKind::AdjustColor {
                brightness: Animated::new(0.0),
                contrast: Animated::new(1.0),
                saturation: Animated::new(1.0),
                hue: Animated::new(0.0),
            },
        };
        Effect {
            enabled: true,
            kind,
        }
    }

    pub fn name(&self) -> &'static str {
        match self.kind {
            EffectKind::Blur { .. } => "Blur",
            EffectKind::Shadow { .. } => "Drop shadow",
            EffectKind::Glow { .. } => "Glow",
            EffectKind::AdjustColor { .. } => "Adjust color",
        }
    }

    /// The effect's animatable parameters, in display order.
    pub fn tracks(&self) -> Vec<&dyn KeyTrack> {
        match &self.kind {
            EffectKind::Blur { radius } => vec![radius],
            EffectKind::Shadow {
                color,
                distance,
                angle,
                blur,
            } => vec![color, distance, angle, blur],
            EffectKind::Glow {
                color,
                radius,
                strength,
            } => vec![color, radius, strength],
            EffectKind::AdjustColor {
                brightness,
                contrast,
                saturation,
                hue,
            } => vec![brightness, contrast, saturation, hue],
        }
    }

    pub fn tracks_mut(&mut self) -> Vec<&mut dyn KeyTrack> {
        match &mut self.kind {
            EffectKind::Blur { radius } => vec![radius],
            EffectKind::Shadow {
                color,
                distance,
                angle,
                blur,
            } => vec![color, distance, angle, blur],
            EffectKind::Glow {
                color,
                radius,
                strength,
            } => vec![color, radius, strength],
            EffectKind::AdjustColor {
                brightness,
                contrast,
                saturation,
                hue,
            } => vec![brightness, contrast, saturation, hue],
        }
    }
}
