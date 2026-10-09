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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Easing {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    /// Keep this keyframe's value until the next keyframe, then jump.
    Hold,
}

impl Easing {
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
        }
    }

    /// Maps linear progress `t` in `0..=1` to eased progress.
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
        }
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
