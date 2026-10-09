//! Keyframed values.
//!
//! Every property that can change over time is an [`Animated<T>`]: a static
//! value plus an optional, frame-sorted list of [`Keyframe`]s. Once a property
//! has at least one keyframe, its value comes from the keyframes and editing it
//! writes a keyframe at the current frame (the same "auto key" behaviour as
//! Alight Motion and After Effects' stopwatch).

use egui::Vec2;
use serde::{Deserialize, Serialize};

use super::Color;

/// How a keyframe eases into the next one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Easing {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    /// Keep this keyframe's value until the next keyframe, then jump.
    Hold,
    /// A curve drawn in the graph editor.
    Custom(Bezier),
}

impl Easing {
    /// The presets offered next to the graph editor.
    pub const ALL: [Easing; 5] = [
        Easing::Linear,
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
        Easing::Hold,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Easing::Linear => "Linear",
            Easing::EaseIn => "Ease in",
            Easing::EaseOut => "Ease out",
            Easing::EaseInOut => "Ease in & out",
            Easing::Hold => "Hold",
            Easing::Custom(_) => "Custom",
        }
    }

    /// Maps linear progress `t` in `0..=1` to eased progress. Custom curves
    /// may overshoot below 0 or above 1.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::EaseIn => t * t * t,
            Easing::EaseOut => 1.0 - (1.0 - t).powi(3),
            Easing::EaseInOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
            Easing::Hold => 0.0,
            Easing::Custom(curve) => curve.apply(t),
        }
    }

    /// The curve to start editing from in the graph editor: the preset's
    /// shape as a bezier.
    pub fn as_curve(self) -> Bezier {
        match self {
            Easing::Linear | Easing::Hold => {
                Bezier::new(1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0)
            }
            Easing::EaseIn => Bezier::new(1.0 / 3.0, 0.0, 2.0 / 3.0, 0.0),
            Easing::EaseOut => Bezier::new(1.0 / 3.0, 1.0, 2.0 / 3.0, 1.0),
            Easing::EaseInOut => Bezier::new(0.66, 0.0, 0.34, 1.0),
            Easing::Custom(curve) => curve,
        }
    }
}

/// An easing curve from (0, 0) to (1, 1) with two control points, as in CSS
/// `cubic-bezier()`. x is time and stays within `0..=1`; y is progress and
/// may overshoot.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bezier {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl Bezier {
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    /// Named curves offered under the graph editor.
    pub const PRESETS: [(&'static str, Bezier); 6] = [
        ("Smooth", Bezier::new(0.25, 0.1, 0.25, 1.0)),
        ("Snappy", Bezier::new(0.8, 0.0, 0.2, 1.0)),
        ("Expo out", Bezier::new(0.16, 1.0, 0.3, 1.0)),
        ("Back in", Bezier::new(0.6, -0.28, 0.735, 0.045)),
        ("Back out", Bezier::new(0.175, 0.885, 0.32, 1.275)),
        ("Back in & out", Bezier::new(0.68, -0.55, 0.265, 1.55)),
    ];

    fn axis(a: f32, b: f32, s: f32) -> f32 {
        let u = 1.0 - s;
        3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
    }

    /// The point on the curve at parameter `s`.
    pub fn point(&self, s: f32) -> (f32, f32) {
        (
            Self::axis(self.x1, self.x2, s),
            Self::axis(self.y1, self.y2, s),
        )
    }

    /// Progress at time `t`: finds where the curve crosses `t` and reads its
    /// height there.
    pub fn apply(&self, t: f32) -> f32 {
        let (x1, x2) = (self.x1.clamp(0.0, 1.0), self.x2.clamp(0.0, 1.0));
        let x = |s: f32| Self::axis(x1, x2, s);
        let slope = |s: f32| {
            let u = 1.0 - s;
            3.0 * u * u * x1 + 6.0 * u * s * (x2 - x1) + 3.0 * s * s * (1.0 - x2)
        };
        // Newton's method converges fast on most curves; bisection catches
        // the flat ones. x(s) only rises because x1 and x2 are in 0..=1.
        let mut s = t;
        for _ in 0..8 {
            let d = slope(s);
            if d.abs() < 1e-6 {
                break;
            }
            s = (s - (x(s) - t) / d).clamp(0.0, 1.0);
        }
        if (x(s) - t).abs() > 1e-5 {
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..30 {
                s = (lo + hi) / 2.0;
                if x(s) < t { lo = s } else { hi = s }
            }
        }
        Self::axis(self.y1, self.y2, s)
    }
}

/// A value type that can be interpolated between keyframes.
pub trait Lerp: Clone + PartialEq {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        a + (b - a) * t
    }
}

impl Lerp for Vec2 {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        *a + (*b - *a) * t
    }
}

impl Lerp for Color {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        Color::new(
            f32::lerp(&a.r, &b.r, t),
            f32::lerp(&a.g, &b.g, t),
            f32::lerp(&a.b, &b.b, t),
            f32::lerp(&a.a, &b.a, t),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keyframe<T> {
    pub frame: i32,
    pub value: T,
    /// Easing used from this keyframe to the next one.
    #[serde(default)]
    pub easing: Easing,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animated<T> {
    /// The value used while the property has no keyframes.
    pub value: T,
    /// Keyframes, always sorted by frame with no duplicate frames.
    #[serde(default = "Vec::new", skip_serializing_if = "Vec::is_empty")]
    pub keyframes: Vec<Keyframe<T>>,
}

impl<T: Lerp> Animated<T> {
    pub fn new(value: T) -> Self {
        Self {
            value,
            keyframes: Vec::new(),
        }
    }

    pub fn is_animated(&self) -> bool {
        !self.keyframes.is_empty()
    }

    /// The value at `frame`, interpolating between keyframes.
    pub fn sample(&self, frame: f32) -> T {
        let keys = &self.keyframes;
        let (Some(first), Some(last)) = (keys.first(), keys.last()) else {
            return self.value.clone();
        };
        if frame <= first.frame as f32 {
            return first.value.clone();
        }
        if frame >= last.frame as f32 {
            return last.value.clone();
        }
        // First keyframe strictly after `frame`; there is at least one before it.
        let next = keys.partition_point(|k| k.frame as f32 <= frame);
        let a = &keys[next - 1];
        let b = &keys[next];
        let t = (frame - a.frame as f32) / (b.frame - a.frame) as f32;
        T::lerp(&a.value, &b.value, a.easing.apply(t))
    }

    /// Sets the property's value as seen at `frame`. Animated properties get a
    /// keyframe at `frame` (added or updated); static ones change their value.
    pub fn set(&mut self, frame: i32, value: T) {
        if self.is_animated() {
            self.upsert_key(frame, value);
        } else {
            self.value = value;
        }
    }

    pub fn key_index(&self, frame: i32) -> Option<usize> {
        self.keyframes
            .binary_search_by_key(&frame, |k| k.frame)
            .ok()
    }

    pub fn upsert_key(&mut self, frame: i32, value: T) {
        match self.keyframes.binary_search_by_key(&frame, |k| k.frame) {
            Ok(i) => self.keyframes[i].value = value,
            Err(i) => {
                // A new key inherits the easing of the key before it, so adding
                // keys inside an eased segment keeps the motion's feel.
                let easing = i
                    .checked_sub(1)
                    .map(|p| self.keyframes[p].easing)
                    .unwrap_or_default();
                self.keyframes.insert(
                    i,
                    Keyframe {
                        frame,
                        value,
                        easing,
                    },
                );
            }
        }
    }
}

/// Type-erased access to the keyframes of one property, used by the timeline
/// so it can list, move and delete keys without knowing the value type.
pub trait KeyTrack {
    fn key_frames(&self) -> Vec<i32>;
    fn has_key(&self, frame: i32) -> bool;
    /// Adds a key at `frame` holding the current value there, or removes the
    /// key if one already exists. Returns whether a key exists afterwards.
    fn toggle_key(&mut self, frame: i32) -> bool;
    fn remove_key(&mut self, frame: i32);
    /// Moves the key at `from` to `to`, replacing any key already at `to`.
    fn move_key(&mut self, from: i32, to: i32);
    /// Shifts every key by `delta` frames.
    fn shift_keys(&mut self, delta: i32);
    fn easing(&self, frame: i32) -> Option<Easing>;
    fn set_easing(&mut self, frame: i32, easing: Easing);
}

impl<T: Lerp> KeyTrack for Animated<T> {
    fn key_frames(&self) -> Vec<i32> {
        self.keyframes.iter().map(|k| k.frame).collect()
    }

    fn has_key(&self, frame: i32) -> bool {
        self.key_index(frame).is_some()
    }

    fn toggle_key(&mut self, frame: i32) -> bool {
        if self.has_key(frame) {
            self.remove_key(frame);
            false
        } else {
            let value = self.sample(frame as f32);
            self.upsert_key(frame, value);
            true
        }
    }

    fn remove_key(&mut self, frame: i32) {
        if let Some(i) = self.key_index(frame) {
            let removed = self.keyframes.remove(i);
            // Keep showing the removed value once the last key is gone,
            // instead of snapping back to a stale static value.
            if self.keyframes.is_empty() {
                self.value = removed.value;
            }
        }
    }

    fn move_key(&mut self, from: i32, to: i32) {
        if from == to {
            return;
        }
        if let Some(i) = self.key_index(from) {
            let mut key = self.keyframes.remove(i);
            key.frame = to;
            match self.keyframes.binary_search_by_key(&to, |k| k.frame) {
                Ok(j) => self.keyframes[j] = key,
                Err(j) => self.keyframes.insert(j, key),
            }
        }
    }

    fn shift_keys(&mut self, delta: i32) {
        for key in &mut self.keyframes {
            key.frame += delta;
        }
    }

    fn easing(&self, frame: i32) -> Option<Easing> {
        self.key_index(frame).map(|i| self.keyframes[i].easing)
    }

    fn set_easing(&mut self, frame: i32, easing: Easing) {
        if let Some(i) = self.key_index(frame) {
            self.keyframes[i].easing = easing;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Animated<f32> {
        let mut a = Animated::new(5.0);
        a.upsert_key(10, 0.0);
        a.upsert_key(20, 100.0);
        a
    }

    #[test]
    fn static_value_without_keys() {
        let a = Animated::new(3.0_f32);
        assert_eq!(a.sample(0.0), 3.0);
        assert_eq!(a.sample(1000.0), 3.0);
    }

    #[test]
    fn linear_interpolation_and_clamping() {
        let a = track();
        assert_eq!(a.sample(0.0), 0.0);
        assert_eq!(a.sample(10.0), 0.0);
        assert_eq!(a.sample(15.0), 50.0);
        assert_eq!(a.sample(20.0), 100.0);
        assert_eq!(a.sample(99.0), 100.0);
    }

    #[test]
    fn hold_and_ease() {
        let mut a = track();
        a.set_easing(10, Easing::Hold);
        assert_eq!(a.sample(19.0), 0.0);
        a.set_easing(10, Easing::EaseInOut);
        assert!(a.sample(12.0) < 20.0);
        assert!((a.sample(15.0) - 50.0).abs() < 1e-3);
    }

    #[test]
    fn custom_curves() {
        let linear = Easing::Linear.as_curve();
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            assert!((linear.apply(t) - t).abs() < 1e-4);
        }
        // The bezier stand-ins stay close to the presets they replace.
        for e in [Easing::EaseIn, Easing::EaseOut, Easing::EaseInOut] {
            for i in 0..=20 {
                let t = i as f32 / 20.0;
                assert!(
                    (e.as_curve().apply(t) - e.apply(t)).abs() < 0.01,
                    "{e:?} at {t}"
                );
            }
        }
        let back = Easing::Custom(Bezier::PRESETS[4].1);
        assert!(
            (0..100).any(|i| back.apply(i as f32 / 100.0) > 1.0),
            "overshoots"
        );
        assert_eq!(back.apply(1.0), 1.0);

        let mut a = track();
        a.set_easing(10, back);
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Animated<f32>>(&json).unwrap(), a);
    }

    #[test]
    fn set_auto_keys_when_animated() {
        let mut a = Animated::new(1.0_f32);
        a.set(5, 2.0);
        assert!(!a.is_animated());
        assert_eq!(a.value, 2.0);

        let mut a = track();
        a.set(15, 7.0);
        assert_eq!(a.key_frames(), vec![10, 15, 20]);
        assert_eq!(a.sample(15.0), 7.0);
    }

    #[test]
    fn move_toggle_shift() {
        let mut a = track();
        a.move_key(10, 25);
        assert_eq!(a.key_frames(), vec![20, 25]);
        a.move_key(25, 20); // replaces the key at 20
        assert_eq!(a.key_frames(), vec![20]);
        assert!(a.toggle_key(30));
        assert!(!a.toggle_key(30));
        a.shift_keys(-5);
        assert_eq!(a.key_frames(), vec![15]);
        a.remove_key(15);
        assert!(!a.is_animated());
        assert_eq!(a.value, 0.0);
    }
}
