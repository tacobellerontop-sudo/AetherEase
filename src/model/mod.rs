//! The document model: a [`Project`] is a canvas plus a stack of [`Layer`]s.

pub mod anim;

use std::path::PathBuf;

use egui::{Color32, Vec2, vec2};
use serde::{Deserialize, Serialize};

pub use anim::{Animated, Easing, KeyTrack};

/// Straight-alpha sRGB colour with components in `0..=1`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const WHITE: Color = Color::new(1.0, 1.0, 1.0, 1.0);
    pub const BLACK: Color = Color::new(0.0, 0.0, 0.0, 1.0);

    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub fn to_color32(self) -> Color32 {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color32::from_rgba_unmultiplied(c(self.r), c(self.g), c(self.b), c(self.a))
    }

    pub fn from_color32(c: Color32) -> Self {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        Self::new(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        )
    }

    pub fn with_alpha_factor(self, factor: f32) -> Self {
        Self {
            a: self.a * factor,
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    Polygon { sides: u32 },
    Star { points: u32, inner_ratio: f32 },
}

impl ShapeKind {
    pub const PRESETS: [(&'static str, ShapeKind); 5] = [
        ("Rectangle", ShapeKind::Rectangle),
        ("Ellipse", ShapeKind::Ellipse),
        ("Triangle", ShapeKind::Polygon { sides: 3 }),
        ("Hexagon", ShapeKind::Polygon { sides: 6 }),
        (
            "Star",
            ShapeKind::Star {
                points: 5,
                inner_ratio: 0.45,
            },
        ),
    ];

    pub fn name(&self) -> &'static str {
        match self {
            ShapeKind::Rectangle => "Rectangle",
            ShapeKind::Ellipse => "Ellipse",
            ShapeKind::Polygon { sides: 3 } => "Triangle",
            ShapeKind::Polygon { .. } => "Polygon",
            ShapeKind::Star { .. } => "Star",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LayerKind {
    Shape {
        shape: ShapeKind,
        size: Animated<Vec2>,
        /// Only used by rectangles.
        #[serde(default = "zero_radius")]
        corner_radius: Animated<f32>,
    },
    Text {
        text: String,
        font_size: f32,
    },
    Image {
        path: PathBuf,
        /// Pixel size of the image when it was imported.
        size: Vec2,
    },
}

fn zero_radius() -> Animated<f32> {
    Animated::new(0.0)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    /// Canvas position (pixels from the top-left) of the anchor point.
    pub position: Animated<Vec2>,
    pub scale: Animated<Vec2>,
    /// Clockwise rotation in degrees.
    pub rotation: Animated<f32>,
    /// The pivot, in layer-local pixels relative to the layer's centre.
    pub anchor: Vec2,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Border {
    pub enabled: bool,
    pub width: Animated<f32>,
    pub color: Animated<Color>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: u64,
    pub name: String,
    pub kind: LayerKind,
    pub visible: bool,
    pub locked: bool,
    /// First frame the layer is shown on.
    pub in_frame: i32,
    /// Frame after the last one the layer is shown on.
    pub out_frame: i32,
    pub transform: Transform,
    pub opacity: Animated<f32>,
    /// Fill colour of shapes and text; a tint multiplier for images.
    pub fill: Animated<Color>,
    pub border: Border,
}

/// Identifies one animatable property of a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropId {
    Position,
    Scale,
    Rotation,
    Opacity,
    Fill,
    Size,
    CornerRadius,
    BorderWidth,
    BorderColor,
}

impl Layer {
    fn base(id: u64, name: String, kind: LayerKind, position: Vec2, frames: (i32, i32)) -> Self {
        Self {
            id,
            name,
            kind,
            visible: true,
            locked: false,
            in_frame: frames.0,
            out_frame: frames.1,
            transform: Transform {
                position: Animated::new(position),
                scale: Animated::new(vec2(1.0, 1.0)),
                rotation: Animated::new(0.0),
                anchor: Vec2::ZERO,
            },
            opacity: Animated::new(1.0),
            fill: Animated::new(Color::WHITE),
            border: Border {
                enabled: false,
                width: Animated::new(4.0),
                color: Animated::new(Color::BLACK),
            },
        }
    }

    pub fn is_active_at(&self, frame: i32) -> bool {
        self.visible && frame >= self.in_frame && frame < self.out_frame
    }

    /// Every animatable property this layer has, in display order.
    pub fn props(&self) -> Vec<PropId> {
        let mut props = vec![
            PropId::Position,
            PropId::Scale,
            PropId::Rotation,
            PropId::Opacity,
            PropId::Fill,
        ];
        if let LayerKind::Shape { shape, .. } = &self.kind {
            props.push(PropId::Size);
            if *shape == ShapeKind::Rectangle {
                props.push(PropId::CornerRadius);
            }
            props.push(PropId::BorderWidth);
            props.push(PropId::BorderColor);
        }
        props
    }

    pub fn track(&self, prop: PropId) -> Option<&dyn KeyTrack> {
        Some(match prop {
            PropId::Position => &self.transform.position,
            PropId::Scale => &self.transform.scale,
            PropId::Rotation => &self.transform.rotation,
            PropId::Opacity => &self.opacity,
            PropId::Fill => &self.fill,
            PropId::BorderWidth => &self.border.width,
            PropId::BorderColor => &self.border.color,
            PropId::Size => match &self.kind {
                LayerKind::Shape { size, .. } => size,
                _ => return None,
            },
            PropId::CornerRadius => match &self.kind {
                LayerKind::Shape { corner_radius, .. } => corner_radius,
                _ => return None,
            },
        })
    }

    pub fn track_mut(&mut self, prop: PropId) -> Option<&mut dyn KeyTrack> {
        Some(match prop {
            PropId::Position => &mut self.transform.position,
            PropId::Scale => &mut self.transform.scale,
            PropId::Rotation => &mut self.transform.rotation,
            PropId::Opacity => &mut self.opacity,
            PropId::Fill => &mut self.fill,
            PropId::BorderWidth => &mut self.border.width,
            PropId::BorderColor => &mut self.border.color,
            PropId::Size => match &mut self.kind {
                LayerKind::Shape { size, .. } => size,
                _ => return None,
            },
            PropId::CornerRadius => match &mut self.kind {
                LayerKind::Shape { corner_radius, .. } => corner_radius,
                _ => return None,
            },
        })
    }

    /// Sorted, de-duplicated frames that have a key on any property.
    pub fn all_key_frames(&self) -> Vec<i32> {
        let mut frames: Vec<i32> = self
            .props()
            .into_iter()
            .filter_map(|p| self.track(p))
            .flat_map(|t| t.key_frames())
            .collect();
        frames.sort_unstable();
        frames.dedup();
        frames
    }

    /// Moves every property's key at `from` to `to`, the way a keyframe on a
    /// layer's timeline bar moves as one. Refuses (returns false) when some
    /// property already has a key at `to`, so dragging never swallows keys.
    pub fn move_keys_at(&mut self, from: i32, to: i32) -> bool {
        let props = self.props();
        let blocked = props
            .iter()
            .filter_map(|&p| self.track(p))
            .any(|t| t.has_key(from) && t.has_key(to));
        if from == to || blocked {
            return false;
        }
        for prop in props {
            if let Some(track) = self.track_mut(prop) {
                track.move_key(from, to);
            }
        }
        true
    }

    pub fn remove_keys_at(&mut self, frame: i32) {
        for prop in self.props() {
            if let Some(track) = self.track_mut(prop) {
                track.remove_key(frame);
            }
        }
    }

    /// Easing of the keys at `frame` (the first property's, if they differ).
    pub fn easing_at(&self, frame: i32) -> Option<Easing> {
        self.props()
            .into_iter()
            .filter_map(|p| self.track(p))
            .find_map(|t| t.easing(frame))
    }

    pub fn set_easing_at(&mut self, frame: i32, easing: Easing) {
        for prop in self.props() {
            if let Some(track) = self.track_mut(prop) {
                track.set_easing(frame, easing);
            }
        }
    }

    /// Moves the layer in time, keyframes included.
    pub fn shift_in_time(&mut self, delta: i32) {
        self.in_frame += delta;
        self.out_frame += delta;
        for prop in self.props() {
            if let Some(track) = self.track_mut(prop) {
                track.shift_keys(delta);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Length of the composition in frames.
    pub duration: i32,
    pub background: Color,
    /// Bottom-most layer first (render order).
    pub layers: Vec<Layer>,
    pub next_id: u64,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            name: "Untitled".into(),
            width: 1920,
            height: 1080,
            fps: 30,
            duration: 30 * 10,
            background: Color::new(0.08, 0.08, 0.1, 1.0),
            layers: Vec::new(),
            next_id: 1,
        }
    }
}

impl Project {
    pub fn center(&self) -> Vec2 {
        vec2(self.width as f32, self.height as f32) * 0.5
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn unique_name(&self, base: &str) -> String {
        let count = self
            .layers
            .iter()
            .filter(|l| l.name.starts_with(base))
            .count();
        if count == 0 {
            base.to_owned()
        } else {
            format!("{base} {}", count + 1)
        }
    }

    /// Pushes `layer` on top of the stack and returns its id.
    fn push(&mut self, layer: Layer) -> u64 {
        let id = layer.id;
        self.layers.push(layer);
        id
    }

    /// New layers span from `frame` to the end of the composition.
    fn new_layer_frames(&self, frame: i32) -> (i32, i32) {
        let start = frame.clamp(0, (self.duration - 1).max(0));
        (start, self.duration.max(start + 1))
    }

    pub fn add_shape(&mut self, shape: ShapeKind, frame: i32) -> u64 {
        let id = self.alloc_id();
        let name = self.unique_name(shape.name());
        let side = self.width.min(self.height) as f32 * 0.25;
        let mut layer = Layer::base(
            id,
            name,
            LayerKind::Shape {
                shape,
                size: Animated::new(vec2(side, side)),
                corner_radius: zero_radius(),
            },
            self.center(),
            self.new_layer_frames(frame),
        );
        layer.fill = Animated::new(Color::new(0.33, 0.55, 1.0, 1.0));
        self.push(layer)
    }

    pub fn add_text(&mut self, text: &str, frame: i32) -> u64 {
        let id = self.alloc_id();
        let name = self.unique_name("Text");
        let font_size = self.height as f32 * 0.08;
        let layer = Layer::base(
            id,
            name,
            LayerKind::Text {
                text: text.to_owned(),
                font_size,
            },
            self.center(),
            self.new_layer_frames(frame),
        );
        self.push(layer)
    }

    pub fn add_image(&mut self, path: PathBuf, pixel_size: Vec2, frame: i32) -> u64 {
        let id = self.alloc_id();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Image".into());
        let name = self.unique_name(&stem);
        // Fit large images inside the canvas.
        let fit = (self.width as f32 / pixel_size.x)
            .min(self.height as f32 / pixel_size.y)
            .min(1.0);
        let mut layer = Layer::base(
            id,
            name,
            LayerKind::Image {
                path,
                size: pixel_size,
            },
            self.center(),
            self.new_layer_frames(frame),
        );
        layer.transform.scale = Animated::new(vec2(fit, fit));
        self.push(layer)
    }

    pub fn layer(&self, id: u64) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: u64) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    pub fn remove_layer(&mut self, id: u64) {
        self.layers.retain(|l| l.id != id);
    }

    /// Duplicates a layer directly above the original; returns the copy's id.
    pub fn duplicate_layer(&mut self, id: u64) -> Option<u64> {
        let index = self.index_of(id)?;
        let mut copy = self.layers[index].clone();
        copy.id = self.alloc_id();
        copy.name = format!("{} copy", copy.name);
        let new_id = copy.id;
        self.layers.insert(index + 1, copy);
        Some(new_id)
    }

    /// Moves a layer up (towards the front, `delta > 0`) or down the stack.
    pub fn reorder_layer(&mut self, id: u64, delta: isize) {
        if let Some(index) = self.index_of(id) {
            let target = (index as isize + delta).clamp(0, self.layers.len() as isize - 1) as usize;
            let layer = self.layers.remove(index);
            self.layers.insert(target, layer);
        }
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }
}

/// Choices offered when creating a project, in the style of Alight Motion:
/// the short side's resolution plus an aspect ratio, rather than raw pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectSettings {
    pub name: String,
    /// Pixels on the shorter side (1080 for "1080p").
    pub resolution: u32,
    /// Aspect ratio as width:height.
    pub aspect: (u32, u32),
    pub fps: u32,
    pub background: Color,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            name: "Untitled project".into(),
            resolution: 1080,
            aspect: (16, 9),
            fps: 30,
            background: Color::BLACK,
        }
    }
}

impl ProjectSettings {
    pub const RESOLUTIONS: [(&'static str, u32); 6] = [
        ("480p", 480),
        ("540p", 540),
        ("720p", 720),
        ("1080p", 1080),
        ("1440p", 1440),
        ("4K", 2160),
    ];
    pub const ASPECTS: [(u32, u32); 7] =
        [(16, 9), (9, 16), (1, 1), (4, 3), (3, 4), (4, 5), (21, 9)];
    pub const FRAME_RATES: [u32; 8] = [12, 15, 24, 25, 30, 48, 50, 60];

    /// Canvas size in pixels; both sides are rounded to even numbers, which
    /// video encoders require.
    pub fn size(&self) -> (u32, u32) {
        let (aw, ah) = (self.aspect.0.max(1) as f32, self.aspect.1.max(1) as f32);
        let short = self.resolution as f32;
        let even = |v: f32| ((v / 2.0).round() * 2.0).max(2.0) as u32;
        if aw >= ah {
            (even(short * aw / ah), even(short))
        } else {
            (even(short), even(short * ah / aw))
        }
    }

    pub fn build(&self) -> Project {
        let (width, height) = self.size();
        let name = self.name.trim();
        Project {
            name: if name.is_empty() {
                "Untitled project".into()
            } else {
                name.to_owned()
            },
            width,
            height,
            fps: self.fps,
            duration: self.fps as i32 * 10,
            background: self.background,
            ..Project::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let mut project = Project::default();
        let shape = project.add_shape(ShapeKind::Rectangle, 0);
        project.add_text("Hello", 10);
        project.add_image("pic.png".into(), vec2(4000.0, 1000.0), 0);
        project
            .layer_mut(shape)
            .unwrap()
            .transform
            .rotation
            .upsert_key(15, 90.0);

        let json = project.to_json().unwrap();
        assert_eq!(Project::from_json(&json).unwrap(), project);
    }

    #[test]
    fn layer_editing() {
        let mut project = Project::default();
        let a = project.add_shape(ShapeKind::Ellipse, 0);
        let b = project.add_text("B", 0);
        let c = project.duplicate_layer(a).unwrap();
        assert_eq!(project.index_of(c), Some(1));
        project.reorder_layer(a, 10);
        assert_eq!(project.layers.last().unwrap().id, a);
        project.remove_layer(b);
        assert_eq!(project.layers.len(), 2);
    }

    #[test]
    fn shifting_a_layer_moves_its_keys() {
        let mut project = Project::default();
        let id = project.add_shape(ShapeKind::Rectangle, 0);
        let layer = project.layer_mut(id).unwrap();
        layer.opacity.upsert_key(5, 0.0);
        layer.transform.position.upsert_key(20, Vec2::ZERO);
        layer.shift_in_time(10);
        assert_eq!(layer.in_frame, 10);
        assert_eq!(layer.all_key_frames(), vec![15, 30]);
    }

    #[test]
    fn project_settings_sizes() {
        let mut settings = ProjectSettings::default();
        assert_eq!(settings.size(), (1920, 1080));
        settings.aspect = (9, 16);
        assert_eq!(settings.size(), (1080, 1920));
        settings.aspect = (1, 1);
        settings.resolution = 720;
        assert_eq!(settings.size(), (720, 720));
        settings.aspect = (21, 9);
        settings.resolution = 1080;
        assert_eq!(settings.size(), (2520, 1080));
        settings.aspect = (4, 5);
        assert_eq!(settings.size(), (1080, 1350));

        settings.name = "  ".into();
        settings.fps = 24;
        let project = settings.build();
        assert_eq!(project.name, "Untitled project");
        assert_eq!(project.duration, 240);
    }

    #[test]
    fn keys_on_a_bar_move_together() {
        let mut project = Project::default();
        let id = project.add_shape(ShapeKind::Rectangle, 0);
        let layer = project.layer_mut(id).unwrap();
        layer.opacity.upsert_key(10, 0.5);
        layer.transform.rotation.upsert_key(10, 45.0);
        layer.transform.rotation.upsert_key(20, 90.0);

        assert!(layer.move_keys_at(10, 15));
        assert_eq!(layer.all_key_frames(), vec![15, 20]);
        // Rotation already has a key at 20, so this would swallow it.
        assert!(!layer.move_keys_at(15, 20));

        layer.set_easing_at(15, Easing::EaseOut);
        assert_eq!(layer.easing_at(15), Some(Easing::EaseOut));
        assert_eq!(layer.opacity.easing(15), Some(Easing::EaseOut));

        layer.remove_keys_at(15);
        assert_eq!(layer.all_key_frames(), vec![20]);
    }

    #[test]
    fn large_images_fit_the_canvas() {
        let mut project = Project::default();
        let id = project.add_image("big.png".into(), vec2(3840.0, 2160.0), 0);
        let scale = project.layer(id).unwrap().transform.scale.value;
        assert_eq!(scale, vec2(0.5, 0.5));
    }
}
