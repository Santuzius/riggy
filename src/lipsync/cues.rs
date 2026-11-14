//! Mouth-shape cues: timed Rhubarb shapes attached to an utterance, and how each shape poses riggy's mouth.

use std::collections::HashMap;

use bevy::log::warn;

use super::Shape;
use crate::avatar::ExpressionPreset;

/// A mouth shape from `start` (sample index in the utterance) until the next cue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    pub start: usize,
    pub shape: Shape,
}

/// The cue in force at sample `position`, if cues reach that far.
pub fn cue_at(cues: &[Cue], cued_until: usize, position: usize) -> Option<Shape> {
    if position >= cued_until {
        return None;
    }
    let i = cues.partition_point(|c| c.start <= position);
    Some(if i == 0 { Shape::X } else { cues[i - 1].shape })
}

impl Shape {
    /// Viseme weights in the order of `ExpressionPreset::VISEMES` (aa, ih, ou, ee, oh); `overrides` (`[lipsync.cues]`) replaces a shape's defaults, e.g. `B = { ee = 0.9 }`.
    pub fn visemes(self, overrides: &HashMap<Shape, HashMap<String, f32>>) -> [f32; 5] {
        let Some(weights) = overrides.get(&self) else { return self.default_visemes() };
        let mut out = [0.0; 5];
        for (name, &w) in weights {
            match ExpressionPreset::VISEMES.iter().position(|v| v.as_str() == name) {
                Some(i) => out[i] = w,
                None => warn!("[lipsync.cues] {self:?}: {name} is not a mouth shape (aa, ih, ou, ee, oh)"),
            }
        }
        out
    }

    fn default_visemes(self) -> [f32; 5] {
        match self {
            Self::A | Self::X => [0.0; 5],
            // Rhubarb uses B for the long "ee" as well as most consonants: spread lips, teeth showing.
            Self::B => [0.0, 0.0, 0.0, 0.85, 0.0],
            Self::C => [0.3, 0.4, 0.0, 0.0, 0.0],
            Self::D => [0.9, 0.0, 0.0, 0.0, 0.0],
            Self::E => [0.0, 0.0, 0.0, 0.0, 0.7],
            Self::F => [0.0, 0.0, 0.9, 0.0, 0.0],
            // No shape for the lower lip under the teeth yet; a narrow opening comes closest.
            Self::G => [0.0, 0.0, 0.0, 0.25, 0.0],
            Self::H => [0.25, 0.25, 0.0, 0.0, 0.0],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_cue_in_force() {
        let cues = [Cue { start: 0, shape: Shape::X }, Cue { start: 100, shape: Shape::A }, Cue { start: 250, shape: Shape::D }];
        assert_eq!(cue_at(&cues, 400, 99), Some(Shape::X));
        assert_eq!(cue_at(&cues, 400, 100), Some(Shape::A));
        assert_eq!(cue_at(&cues, 400, 399), Some(Shape::D));
        assert_eq!(cue_at(&cues, 400, 400), None);
    }

    #[test]
    fn config_overrides_a_shape() {
        let config: crate::config::Config = toml::from_str("[lipsync.cues]\nB = { ee = 0.9, ih = 0.1 }").unwrap();
        assert_eq!(Shape::B.visemes(&config.lipsync.cues), [0.0, 0.1, 0.0, 0.9, 0.0]);
        assert_eq!(Shape::D.visemes(&config.lipsync.cues), Shape::D.default_visemes());
    }
}
