//! The document model: a [`Project`] is a canvas plus a stack of [`Layer`]s.

pub mod anim;
pub mod effects;
pub mod groups;
pub mod space;
pub mod vector;

use std::path::PathBuf;

use egui::{Color32, Vec2, vec2};
use serde::{Deserialize, Serialize};

pub use anim::{Animated, Easing, KeyTrack};
pub use effects::{Effect, EffectKind};
pub use vector::PathShape;

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

    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
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
    /// A vector path drawn with the pen or freehand tool. Filled with the
    /// layer's fill and stroked with its border.
    Path { path: Animated<PathShape> },
    Text {
        text: String,
        font_size: f32,
        #[serde(default, skip_serializing_if = "is_default")]
        font: crate::fonts::FontChoice,
    },
    Image {
        path: PathBuf,
        /// Pixel size of the image when it was imported.
        size: Vec2,
    },
    /// A video clip. Frame `start` is where the clip's first frame sits;
    /// the layer's in and out points trim it.
    Video {
        path: PathBuf,
        /// Pixel size of the video.
        size: Vec2,
        start: i32,
        /// The clip's own frames per second.
        rate: f32,
        seconds: f32,
        /// Whether the file has a sound track, played with the picture.
        has_audio: bool,
        /// Gain of its sound, 1.0 = as recorded.
        volume: f32,
    },
    /// A sound file, played from frame `start` (where the file's time zero
    /// sits; the layer's in point can trim its head).
    Audio {
        path: PathBuf,
        start: i32,
        /// Gain, 1.0 = as recorded.
        volume: f32,
    },
    /// An invisible layer that other layers can be parented to.
    Null,
    /// Holds other layers (those whose `group` is this layer's id). They move
    /// with the group and are composited together before the group's own
    /// opacity, blending and effects apply, which is also what masks clip.
    Group,
    /// Views 3D layers in perspective. Its transform places the camera; the
    /// top-most active camera is the one used.
    Camera {
        /// Distance in pixels at which a 3D layer appears at 100%.
        zoom: Animated<f32>,
    },
    /// Lights the layers. Once a composition has a light, every layer is
    /// shaded by its lights and is dark where none reach, as in After
    /// Effects. The light's colour is the layer's `fill`.
    Light {
        light: LightKind,
        /// 1.0 = 100%.
        intensity: Animated<f32>,
        /// Spot lights: the full width of the beam, in degrees.
        cone: Animated<f32>,
        /// Spot lights: how much of the beam's edge fades out, 0 to 1.
        feather: Animated<f32>,
    },
    /// Applies its effects to everything below it in its stack, inside its
    /// own area (the size of the canvas until moved or scaled).
    Adjustment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightKind {
    /// Shines in every direction from where it is.
    Point,
    /// A cone of light along the light's facing direction.
    Spot,
    /// Parallel rays, like sunlight, along the light's facing direction.
    Parallel,
    /// Lights everything evenly from no direction.
    Ambient,
}

impl LightKind {
    pub const ALL: [LightKind; 4] = [
        LightKind::Point,
        LightKind::Spot,
        LightKind::Parallel,
        LightKind::Ambient,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LightKind::Point => "Point",
            LightKind::Spot => "Spot",
            LightKind::Parallel => "Parallel",
            LightKind::Ambient => "Ambient",
        }
    }
}

impl LayerKind {
    /// Nulls, cameras and lights only exist in the editor; they never
    /// render themselves.
    pub fn is_visual(&self) -> bool {
        !matches!(
            self,
            LayerKind::Null
                | LayerKind::Camera { .. }
                | LayerKind::Light { .. }
                | LayerKind::Audio { .. }
        )
    }

    /// Whether the layer can have a fill colour (shapes, paths and text).
    pub fn has_fill(&self) -> bool {
        matches!(
            self,
            LayerKind::Shape { .. } | LayerKind::Path { .. } | LayerKind::Text { .. }
        )
    }

    /// Whether the layer's border is drawn (shapes and paths).
    pub fn has_border(&self) -> bool {
        matches!(self, LayerKind::Shape { .. } | LayerKind::Path { .. })
    }

    pub fn label(&self) -> &'static str {
        match self {
            LayerKind::Shape { .. } => "Shape",
            LayerKind::Text { .. } => "Text",
            LayerKind::Path { .. } => "Path",
            LayerKind::Image { .. } => "Image",
            LayerKind::Video { .. } => "Video",
            LayerKind::Null => "Null",
            LayerKind::Group => "Group",
            LayerKind::Audio { .. } => "Audio",
            LayerKind::Camera { .. } => "Camera",
            LayerKind::Light { .. } => "Light",
            LayerKind::Adjustment => "Adjustment",
        }
    }
}

/// The side length of a null layer's on-canvas box, in layer pixels.
pub const NULL_SIZE: f32 = 100.0;

/// The side length of a light's on-canvas marker, in layer pixels.
pub const LIGHT_SIZE: f32 = 70.0;

/// Camera distance that shows the z = 0 plane at 100%, scaled to the canvas
/// width the way a 50 mm lens is in After Effects.
pub fn default_camera_zoom(width: u32) -> f32 {
    width as f32 * 1.3889
}

fn zero_radius() -> Animated<f32> {
    Animated::new(0.0)
}

fn zero() -> Animated<f32> {
    Animated::new(0.0)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    /// Canvas position (pixels from the top-left) of the anchor point.
    pub position: Animated<Vec2>,
    pub scale: Animated<Vec2>,
    /// Clockwise rotation in degrees (around the Z axis for 3D layers).
    pub rotation: Animated<f32>,
    /// The pivot, in layer-local pixels relative to the layer's centre.
    pub anchor: Vec2,
    /// Depth of a 3D layer; positive values are further from the camera.
    #[serde(default = "zero")]
    pub z: Animated<f32>,
    /// Tilt of a 3D layer around its X axis, in degrees.
    #[serde(default = "zero")]
    pub rotation_x: Animated<f32>,
    /// Turn of a 3D layer around its Y axis, in degrees.
    #[serde(default = "zero")]
    pub rotation_y: Animated<f32>,
}

/// How a layer's colours combine with the layers below it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Add,
    Hue,
    Saturation,
    Color,
    Luminosity,
    /// Keeps what's below (in the same group) only where this layer is.
    Mask,
    /// Hides what's below (in the same group) where this layer is.
    MaskInvert,
}

impl BlendMode {
    pub const ALL: [BlendMode; 19] = [
        BlendMode::Normal,
        BlendMode::Mask,
        BlendMode::MaskInvert,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Add,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Screen => "Screen",
            BlendMode::Overlay => "Overlay",
            BlendMode::Darken => "Darken",
            BlendMode::Lighten => "Lighten",
            BlendMode::ColorDodge => "Color dodge",
            BlendMode::ColorBurn => "Color burn",
            BlendMode::HardLight => "Hard light",
            BlendMode::SoftLight => "Soft light",
            BlendMode::Difference => "Difference",
            BlendMode::Exclusion => "Exclusion",
            BlendMode::Add => "Add",
            BlendMode::Hue => "Hue",
            BlendMode::Saturation => "Saturation",
            BlendMode::Color => "Color",
            BlendMode::Luminosity => "Luminosity",
            BlendMode::Mask => "Mask",
            BlendMode::MaskInvert => "Mask (inverted)",
        }
    }
}

/// How shapes and text are filled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FillStyle {
    #[default]
    Solid,
    /// From `fill` to `fill_end` along `gradient_angle`.
    Linear,
    /// From `fill` at the centre to `fill_end` at the edge.
    Radial,
}

impl FillStyle {
    pub const ALL: [FillStyle; 3] = [FillStyle::Solid, FillStyle::Linear, FillStyle::Radial];

    pub fn label(self) -> &'static str {
        match self {
            FillStyle::Solid => "Solid",
            FillStyle::Linear => "Linear gradient",
            FillStyle::Radial => "Radial gradient",
        }
    }
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn white() -> Animated<Color> {
    Animated::new(Color::WHITE)
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
    /// Fill colour of shapes and text (the start colour of a gradient).
    pub fill: Animated<Color>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub fill_style: FillStyle,
    /// End colour of a gradient fill.
    #[serde(default = "white")]
    pub fill_end: Animated<Color>,
    /// Direction of a linear gradient in degrees; 0 runs left to right.
    #[serde(default = "zero")]
    pub gradient_angle: Animated<f32>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub blend: BlendMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    pub border: Border,
    /// The layer whose transform this layer follows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    /// The group layer this layer belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<u64>,
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
    Depth,
    RotationX,
    RotationY,
    CameraZoom,
    PathShape,
    LightIntensity,
    ConeAngle,
    ConeFeather,
    FillEnd,
    GradientAngle,
    /// Parameter `param` of the effect at `index` in the layer's list.
    Effect {
        index: u16,
        param: u8,
    },
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
                z: zero(),
                rotation_x: zero(),
                rotation_y: zero(),
            },
            opacity: Animated::new(1.0),
            fill: Animated::new(Color::WHITE),
            fill_style: FillStyle::Solid,
            fill_end: white(),
            gradient_angle: zero(),
            blend: BlendMode::Normal,
            effects: Vec::new(),
            border: Border {
                enabled: false,
                width: Animated::new(4.0),
                color: Animated::new(Color::BLACK),
            },
            parent: None,
            group: None,
        }
    }

    /// Whether the layer is placed in 3D space. Like After Effects with every
    /// layer's 3D switch on: all layers live in 3D, flat on z = 0 until given
    /// depth or tilt. Audio has no place, and adjustment layers stay flat over
    /// the frame so they cover it whatever the camera does.
    pub fn is_3d(&self) -> bool {
        !matches!(self.kind, LayerKind::Audio { .. } | LayerKind::Adjustment)
    }

    /// The sound this layer plays, if any: the file, the frame its time
    /// zero sits on, and its gain. Audio layers and videos with sound have one.
    pub fn sound(&self) -> Option<(&std::path::Path, i32, f32)> {
        match &self.kind {
            LayerKind::Audio {
                path,
                start,
                volume,
            } => Some((path, *start, *volume)),
            LayerKind::Video {
                path,
                start,
                volume,
                has_audio: true,
                ..
            } => Some((path, *start, *volume)),
            _ => None,
        }
    }

    pub fn is_active_at(&self, frame: i32) -> bool {
        self.visible && frame >= self.in_frame && frame < self.out_frame
    }

    /// Every animatable property this layer has, in display order.
    pub fn props(&self) -> Vec<PropId> {
        if matches!(self.kind, LayerKind::Audio { .. }) {
            return Vec::new();
        }
        let mut props = vec![PropId::Position];
        if self.is_3d() {
            props.push(PropId::Depth);
        }
        if !matches!(
            self.kind,
            LayerKind::Camera { .. } | LayerKind::Light { .. }
        ) {
            props.push(PropId::Scale);
        }
        props.push(PropId::Rotation);
        if self.is_3d() {
            props.extend([PropId::RotationX, PropId::RotationY]);
        }
        match &self.kind {
            LayerKind::Null => return props,
            LayerKind::Camera { .. } => {
                props.push(PropId::CameraZoom);
                return props;
            }
            LayerKind::Light { light, .. } => {
                props.extend([PropId::Fill, PropId::LightIntensity]);
                if *light == LightKind::Spot {
                    props.extend([PropId::ConeAngle, PropId::ConeFeather]);
                }
                return props;
            }
            LayerKind::Image { .. }
            | LayerKind::Video { .. }
            | LayerKind::Group
            | LayerKind::Adjustment => props.push(PropId::Opacity),
            LayerKind::Audio { .. } => {}
            LayerKind::Shape { .. } | LayerKind::Path { .. } | LayerKind::Text { .. } => {
                props.extend([PropId::Opacity, PropId::Fill]);
                match self.fill_style {
                    FillStyle::Solid => {}
                    FillStyle::Linear => props.extend([PropId::FillEnd, PropId::GradientAngle]),
                    FillStyle::Radial => props.push(PropId::FillEnd),
                }
            }
        }
        for (index, effect) in self.effects.iter().enumerate() {
            for param in 0..effect.tracks().len() {
                props.push(PropId::Effect {
                    index: index as u16,
                    param: param as u8,
                });
            }
        }
        if let LayerKind::Shape { shape, .. } = &self.kind {
            props.push(PropId::Size);
            if *shape == ShapeKind::Rectangle {
                props.push(PropId::CornerRadius);
            }
        }
        if let LayerKind::Path { .. } = &self.kind {
            props.push(PropId::PathShape);
        }
        if self.kind.has_border() {
            props.push(PropId::BorderWidth);
            props.push(PropId::BorderColor);
        }
        props
    }

    pub fn track(&self, prop: PropId) -> Option<&dyn KeyTrack> {
        Some(match prop {
            PropId::Effect { index, param } => {
                return self
                    .effects
                    .get(index as usize)?
                    .tracks()
                    .into_iter()
                    .nth(param as usize);
            }
            PropId::Position => &self.transform.position,
            PropId::Scale => &self.transform.scale,
            PropId::Rotation => &self.transform.rotation,
            PropId::Opacity => &self.opacity,
            PropId::Fill => &self.fill,
            PropId::BorderWidth => &self.border.width,
            PropId::BorderColor => &self.border.color,
            PropId::Depth => &self.transform.z,
            PropId::FillEnd => &self.fill_end,
            PropId::GradientAngle => &self.gradient_angle,
            PropId::RotationX => &self.transform.rotation_x,
            PropId::RotationY => &self.transform.rotation_y,
            PropId::CameraZoom => match &self.kind {
                LayerKind::Camera { zoom } => zoom,
                _ => return None,
            },
            PropId::PathShape => match &self.kind {
                LayerKind::Path { path } => path,
                _ => return None,
            },
            PropId::LightIntensity => match &self.kind {
                LayerKind::Light { intensity, .. } => intensity,
                _ => return None,
            },
            PropId::ConeAngle => match &self.kind {
                LayerKind::Light { cone, .. } => cone,
                _ => return None,
            },
            PropId::ConeFeather => match &self.kind {
                LayerKind::Light { feather, .. } => feather,
                _ => return None,
            },
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
            PropId::Effect { index, param } => {
                return self
                    .effects
                    .get_mut(index as usize)?
                    .tracks_mut()
                    .into_iter()
                    .nth(param as usize);
            }
            PropId::Position => &mut self.transform.position,
            PropId::Scale => &mut self.transform.scale,
            PropId::Rotation => &mut self.transform.rotation,
            PropId::Opacity => &mut self.opacity,
            PropId::Fill => &mut self.fill,
            PropId::BorderWidth => &mut self.border.width,
            PropId::BorderColor => &mut self.border.color,
            PropId::Depth => &mut self.transform.z,
            PropId::FillEnd => &mut self.fill_end,
            PropId::GradientAngle => &mut self.gradient_angle,
            PropId::RotationX => &mut self.transform.rotation_x,
            PropId::RotationY => &mut self.transform.rotation_y,
            PropId::CameraZoom => match &mut self.kind {
                LayerKind::Camera { zoom } => zoom,
                _ => return None,
            },
            PropId::PathShape => match &mut self.kind {
                LayerKind::Path { path } => path,
                _ => return None,
            },
            PropId::LightIntensity => match &mut self.kind {
                LayerKind::Light { intensity, .. } => intensity,
                _ => return None,
            },
            PropId::ConeAngle => match &mut self.kind {
                LayerKind::Light { cone, .. } => cone,
                _ => return None,
            },
            PropId::ConeFeather => match &mut self.kind {
                LayerKind::Light { feather, .. } => feather,
                _ => return None,
            },
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
        if let LayerKind::Audio { start, .. } | LayerKind::Video { start, .. } = &mut self.kind {
            *start += delta;
        }
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
                font: Default::default(),
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

    /// Adds a video clip starting at `frame`, fitted inside the canvas.
    pub fn add_video(&mut self, path: PathBuf, info: crate::video::VideoInfo, frame: i32) -> u64 {
        let id = self.alloc_id();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Video".into());
        let name = self.unique_name(&stem);
        let size = vec2(info.width as f32, info.height as f32);
        let fit = (self.width as f32 / size.x)
            .min(self.height as f32 / size.y)
            .min(1.0);
        let start = frame.max(0);
        let length = ((info.seconds * self.fps as f32).round() as i32).max(1);
        let mut layer = Layer::base(
            id,
            name,
            LayerKind::Video {
                path,
                size,
                start,
                rate: info.rate,
                seconds: info.seconds,
                has_audio: info.has_audio,
                volume: 1.0,
            },
            self.center(),
            (start, start + length),
        );
        layer.transform.scale = Animated::new(vec2(fit, fit));
        self.push(layer)
    }

    /// Adds a sound that starts at `frame` and lasts `seconds`.
    pub fn add_audio(&mut self, path: PathBuf, seconds: f32, frame: i32) -> u64 {
        let id = self.alloc_id();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Audio".into());
        let name = self.unique_name(&stem);
        let start = frame.max(0);
        let length = ((seconds * self.fps as f32).ceil() as i32).max(1);
        let mut layer = Layer::base(
            id,
            name,
            LayerKind::Audio {
                path,
                start,
                volume: 1.0,
            },
            self.center(),
            (start, start + length),
        );
        layer.out_frame = start + length;
        self.push(layer)
    }

    pub fn add_null(&mut self, frame: i32) -> u64 {
        let id = self.alloc_id();
        let name = self.unique_name("Null");
        let layer = Layer::base(
            id,
            name,
            LayerKind::Null,
            self.center(),
            self.new_layer_frames(frame),
        );
        self.push(layer)
    }

    /// Adds a camera that frames the canvas exactly as it looks in 2D.
    pub fn add_camera(&mut self, frame: i32) -> u64 {
        let id = self.alloc_id();
        let name = self.unique_name("Camera");
        let zoom = default_camera_zoom(self.width);
        let mut layer = Layer::base(
            id,
            name,
            LayerKind::Camera {
                zoom: Animated::new(zoom),
            },
            self.center(),
            self.new_layer_frames(frame),
        );
        layer.transform.z = Animated::new(-zoom);
        self.push(layer)
    }

    /// Adds a path layer whose points are given in canvas pixels. The layer
    /// sits at the middle of the points, which are stored relative to it.
    pub fn add_path(&mut self, mut shape: PathShape, frame: i32, freehand: bool) -> u64 {
        let id = self.alloc_id();
        let name = self.unique_name(if freehand { "Drawing" } else { "Path" });
        let center = shape
            .bounds()
            .map_or(self.center(), |r| r.center().to_vec2());
        shape.translate(-center);
        let mut layer = Layer::base(
            id,
            name,
            LayerKind::Path {
                path: Animated::new(shape),
            },
            center,
            self.new_layer_frames(frame),
        );
        layer.border = Border {
            enabled: true,
            width: Animated::new(if freehand { 10.0 } else { 6.0 }),
            color: Animated::new(Color::new(0.33, 0.55, 1.0, 1.0)),
        };
        // Freehand lines are strokes; pen paths start filled and outlined.
        layer.fill = Animated::new(if freehand {
            Color::new(1.0, 1.0, 1.0, 0.0)
        } else {
            Color::new(1.0, 0.75, 0.3, 1.0)
        });
        if !freehand {
            layer.border.color = Animated::new(Color::WHITE);
        }
        self.push(layer)
    }

    /// Adds a light in front of the canvas, shining onto it.
    pub fn add_light(&mut self, light: LightKind, frame: i32) -> u64 {
        let id = self.alloc_id();
        let name = self.unique_name(&format!("{} light", light.label()));
        let mut layer = Layer::base(
            id,
            name,
            LayerKind::Light {
                light,
                intensity: Animated::new(1.0),
                cone: Animated::new(90.0),
                feather: Animated::new(0.5),
            },
            self.center(),
            self.new_layer_frames(frame),
        );
        layer.fill = Animated::new(Color::new(1.0, 0.97, 0.9, 1.0));
        layer.transform.z = Animated::new(-(self.width.min(self.height) as f32) * 0.5);
        self.push(layer)
    }

    /// Adds an adjustment layer covering the canvas.
    pub fn add_adjustment(&mut self, frame: i32) -> u64 {
        let id = self.alloc_id();
        let name = self.unique_name("Adjustment");
        let mut layer = Layer::base(
            id,
            name,
            LayerKind::Adjustment,
            self.center(),
            self.new_layer_frames(frame),
        );
        layer.effects.push(Effect::preset(Effect::ADJUST_COLOR));
        self.push(layer)
    }

    /// Adds a solid: a rectangle of colour exactly the size of the canvas.
    pub fn add_solid(&mut self, frame: i32) -> u64 {
        let id = self.add_shape(ShapeKind::Rectangle, frame);
        let size = vec2(self.width as f32, self.height as f32);
        let name = self.unique_name("Solid");
        let layer = self.layer_mut(id).expect("just added");
        layer.name = name;
        if let LayerKind::Shape { size: s, .. } = &mut layer.kind {
            *s = Animated::new(size);
        }
        layer.fill = Animated::new(Color::new(0.16, 0.18, 0.24, 1.0));
        id
    }

    /// Whether `ancestor` is `id` itself or one of its parents.
    pub fn is_ancestor(&self, ancestor: u64, id: u64) -> bool {
        let mut current = Some(id);
        // The depth limit guards against cycles in hand-edited files.
        for _ in 0..=self.layers.len() {
            match current {
                Some(c) if c == ancestor => return true,
                Some(c) => current = self.layer(c).and_then(|l| l.parent),
                None => return false,
            }
        }
        false
    }

    /// Parents `id` to `parent`, refusing anything that would form a loop.
    /// Both layers must be in the same group.
    pub fn set_parent(&mut self, id: u64, parent: Option<u64>) -> bool {
        let group = self.layer(id).and_then(|l| l.group);
        if let Some(p) = parent
            && (self.layer(p).is_none_or(|p| p.group != group) || self.is_ancestor(id, p))
        {
            return false;
        }
        match self.layer_mut(id) {
            Some(layer) => {
                layer.parent = parent;
                true
            }
            None => false,
        }
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

    /// Removes a layer; a group goes with everything in it.
    pub fn remove_layer(&mut self, id: u64) {
        let doomed: Vec<u64> = self
            .layers
            .iter()
            .filter(|l| self.is_in_group(l.id, id))
            .map(|l| l.id)
            .collect();
        self.layers.retain(|l| !doomed.contains(&l.id));
        for layer in &mut self.layers {
            if layer.parent.is_some_and(|p| doomed.contains(&p)) {
                layer.parent = None;
            }
        }
    }

    /// Duplicates a layer directly above the original; returns the copy's id.
    /// A group is copied with everything in it.
    pub fn duplicate_layer(&mut self, id: u64) -> Option<u64> {
        let index = self.index_of(id)?;
        // The layer and, for groups, its contents, in stack order.
        let originals: Vec<Layer> = self
            .layers
            .iter()
            .filter(|l| self.is_in_group(l.id, id))
            .cloned()
            .collect();
        let mut ids = std::collections::HashMap::new();
        for layer in &originals {
            ids.insert(layer.id, self.alloc_id());
        }
        let mut insert_at = index + 1;
        for mut copy in originals {
            copy.id = ids[&copy.id];
            // Links inside the copied set point at the copies.
            copy.parent = copy.parent.map(|p| *ids.get(&p).unwrap_or(&p));
            copy.group = copy.group.map(|g| *ids.get(&g).unwrap_or(&g));
            if copy.id == ids[&id] {
                copy.name = format!("{} copy", copy.name);
                self.layers.insert(insert_at, copy);
                insert_at += 1;
            } else {
                self.layers.push(copy);
            }
        }
        Some(ids[&id])
    }

    /// Moves a layer up (towards the front, `delta > 0`) or down among the
    /// layers that share its group.
    pub fn reorder_layer(&mut self, id: u64, delta: isize) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        let group = self.layers[index].group;
        let siblings: Vec<usize> = (0..self.layers.len())
            .filter(|&i| self.layers[i].group == group)
            .collect();
        let Some(pos) = siblings.iter().position(|&i| i == index) else {
            return;
        };
        let target = (pos as isize + delta).clamp(0, siblings.len() as isize - 1) as usize;
        if target != pos {
            let layer = self.layers.remove(index);
            self.layers.insert(siblings[target], layer);
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
    fn parenting_refuses_loops() {
        let mut project = Project::default();
        let a = project.add_null(0);
        let b = project.add_shape(ShapeKind::Rectangle, 0);
        let c = project.add_text("C", 0);
        assert!(project.set_parent(b, Some(a)));
        assert!(project.set_parent(c, Some(b)));
        assert!(!project.set_parent(a, Some(c)));
        assert!(!project.set_parent(a, Some(a)));
        project.remove_layer(b);
        assert_eq!(project.layer(c).unwrap().parent, None);
    }

    #[test]
    fn every_visual_layer_is_3d() {
        let mut project = Project::default();
        let id = project.add_shape(ShapeKind::Rectangle, 0);
        let layer = project.layer(id).unwrap();
        assert!(layer.props().contains(&PropId::Depth));
        assert!(layer.props().contains(&PropId::RotationY));
        assert_eq!(layer.transform.z.value, 0.0);
        let cam = project.add_camera(0);
        let props = project.layer(cam).unwrap().props();
        assert!(props.contains(&PropId::CameraZoom));
        assert!(!props.contains(&PropId::Opacity));
    }

    #[test]
    fn bundled_examples_load() {
        for name in ["demo", "3d-demo", "lights-demo"] {
            let path = format!("{}/examples/{name}.aether", env!("CARGO_MANIFEST_DIR"));
            let json = std::fs::read_to_string(&path).unwrap();
            Project::from_json(&json).unwrap_or_else(|e| panic!("{path}: {e}"));
        }
    }

    #[test]
    fn old_files_without_3d_fields_still_load() {
        let mut project = Project::default();
        project.add_shape(ShapeKind::Ellipse, 0);
        let mut json: serde_json::Value =
            serde_json::from_str(&project.to_json().unwrap()).unwrap();
        let t = &mut json["layers"][0]["transform"];
        for key in ["z", "rotation_x", "rotation_y"] {
            t.as_object_mut().unwrap().remove(key);
        }
        let loaded = Project::from_json(&json.to_string()).unwrap();
        assert_eq!(loaded, project);
    }

    #[test]
    fn effect_keys_move_with_the_layer() {
        let mut project = Project::default();
        let id = project.add_shape(ShapeKind::Rectangle, 0);
        let layer = project.layer_mut(id).unwrap();
        layer.effects.push(Effect::preset(1));
        if let EffectKind::Shadow { distance, .. } = &mut layer.effects[0].kind {
            distance.upsert_key(12, 40.0);
        }
        assert_eq!(layer.all_key_frames(), vec![12]);
        layer.shift_in_time(3);
        assert_eq!(layer.all_key_frames(), vec![15]);
        layer.remove_keys_at(15);
        assert!(layer.all_key_frames().is_empty());
    }

    #[test]
    fn large_images_fit_the_canvas() {
        let mut project = Project::default();
        let id = project.add_image("big.png".into(), vec2(3840.0, 2160.0), 0);
        let scale = project.layer(id).unwrap().transform.scale.value;
        assert_eq!(scale, vec2(0.5, 0.5));
    }
}
