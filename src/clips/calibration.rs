//! A pack's posture calibration for one avatar: `~/.local/share/riggy/animations/<pack>/calibration/<avatar>.toml`.
//!
//! The actor of a pack stands in their own natural posture (in the neutral clip its bone map names), which an avatar should not copy: riggy keeps the avatar's posture and plays only how far a clip moves away from the actor's. The file holds what that takes:
//!
//! - `actor.posture`: each trunk bone's mean turn in the neutral clip against the clip's rest pose, in avatar space (degrees about X, Y and Z, applied Y, X, Z);
//! - `actor.arm_hang`: degrees the line from the actor's shoulder to wrist hangs out from vertical there, seen from the front, left and right;
//! - `approved`: the pack's clips checked on this avatar; riggy lists only these in the tray and plays only these from the behaviour;
//! - `avatar.arm_hang`: degrees the avatar's arms hang out at ease (riggy's `[idle] arm_angle`).
//!
//! riggy never changes the file once it is there, so its values can be edited by hand. Where the user has none, riggy copies its default from `assets/animations/calibration/<pack>/<avatar>.toml`, or, without one, measures the values on the neutral clip and writes them. A file riggy cannot read is reported and the default used until it is fixed. "Reset to defaults" in the tray puts the default back (keeping the old file as `*.old`).

use std::collections::BTreeMap;
use std::path::Path;

use bevy::math::{EulerRot, Quat, Vec3, Vec4};
use serde::{Deserialize, Serialize};

use super::{Clip, ClipBone};

/// The trunk from the hips up, as humanoid bone names (VRM 1.0).
pub const TRUNK: [&str; 6] = ["hips", "spine", "chest", "upperChest", "neck", "head"];

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Calibration {
    /// The neutral clip the values come from.
    #[serde(default)]
    pub neutral: Option<String>,
    /// The pack's clips checked on this avatar: only these are listed in the tray and played by the behaviour.
    #[serde(default)]
    pub approved: Vec<String>,
    #[serde(default)]
    pub actor: Actor,
    #[serde(default)]
    pub avatar: Avatar,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Actor {
    /// Trunk bone → its turn in the actor's natural posture, degrees about X, Y and Z.
    #[serde(default)]
    pub posture: BTreeMap<String, Turn>,
    /// Degrees the actor's arms hang out from vertical, left and right.
    #[serde(default)]
    pub arm_hang: Option<[f32; 2]>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Avatar {
    /// Degrees the avatar's arms hang out from vertical at ease.
    pub arm_hang: f32,
}

impl Default for Avatar {
    fn default() -> Self {
        Self { arm_hang: 12.0 }
    }
}

/// A turn in degrees about avatar space's X (positive leans forward), Y (positive turns to the figure's left) and Z axes, applied in the order Y, X, Z.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Turn {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Turn {
    pub fn quat(self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.y.to_radians(), self.x.to_radians(), self.z.to_radians())
    }

    /// The turn of `q`, rounded to hundredths of a degree so a rewritten file stays the same.
    fn of(q: Quat) -> Self {
        let (y, x, z) = q.to_euler(EulerRot::YXZ);
        let round = |a: f32| (a.to_degrees() * 100.0).round() / 100.0;
        Self { x: round(x), y: round(y), z: round(z) }
    }
}

impl Calibration {
    /// The calibration measured on the neutral clip `neutral` (named `name`), for an avatar whose arms hang out `arm_hang` degrees at ease.
    pub fn measure(neutral: &Clip, name: &str, arm_hang: f32) -> Self {
        let turn = |b: &ClipBone, f: usize| b.rotations[f] * b.rest_rotation.inverse();
        let posture = TRUNK
            .iter()
            .filter_map(|&name| {
                let bone = neutral.bones.get(name)?;
                Some((name.to_owned(), Turn::of(mean((0..neutral.frames).map(|f| turn(bone, f))))))
            })
            .collect();
        let hang = |side: usize| {
            let [upper, lower, hand] = [["leftUpperArm", "leftLowerArm", "leftHand"], ["rightUpperArm", "rightLowerArm", "rightHand"]][side].map(|n| neutral.bones.get(n));
            let (u, l, w) = (upper?, lower?, hand?);
            let angles: Vec<f32> = (0..neutral.frames)
                .map(|f| {
                    let d = turn(u, f) * (l.rest_position - u.rest_position) + turn(l, f) * (w.rest_position - l.rest_position);
                    out_angle(d, side).to_degrees()
                })
                .collect();
            Some((angles.iter().sum::<f32>() / angles.len().max(1) as f32 * 100.0).round() / 100.0)
        };
        let arm_hang_actor = match (hang(0), hang(1)) {
            (Some(l), Some(r)) => Some([l, r]),
            _ => None,
        };
        Self { neutral: Some(name.to_owned()), approved: Vec::new(), actor: Actor { posture, arm_hang: arm_hang_actor }, avatar: Avatar { arm_hang } }
    }

    pub fn load(path: &Path) -> Option<Self> {
        Self::parse(&std::fs::read_to_string(path).ok()?, &path.display().to_string())
    }

    /// The calibration in `text`, read from `origin`.
    pub fn parse(text: &str, origin: &str) -> Option<Self> {
        toml::from_str(text).map_err(|err| eprintln!("riggy: {origin}: {err}")).ok()
    }

    /// Whether the clip `name` is approved on this avatar.
    pub fn approves(&self, name: &str) -> bool {
        self.approved.iter().any(|a| a == name)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let header = "# riggy's posture calibration of this pack for one avatar (see doc/animations.md). riggy reads it as it is; \"Reset to defaults\" in the tray puts the default back.\n\n";
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
        std::fs::write(path, format!("{header}{text}"))
    }
}

/// Radians a direction hangs out from vertical, seen from the front: towards the figure's left for the left arm (`side` 0), its right for the right arm.
pub fn out_angle(d: Vec3, side: usize) -> f32 {
    (if side == 0 { d.x } else { -d.x }).atan2(-d.y)
}

/// The mean of rotations near each other: their sum, each on the first one's side, normalised.
pub fn mean(rotations: impl Iterator<Item = Quat>) -> Quat {
    let mut sum = Vec4::ZERO;
    let mut first: Option<Vec4> = None;
    for q in rotations {
        let v = Vec4::from(q);
        let f = *first.get_or_insert(v);
        sum += if v.dot(f) < 0.0 { -v } else { v };
    }
    if sum.length() < 1e-9 { Quat::IDENTITY } else { Quat::from_vec4(sum.normalize()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_survives_the_file() {
        let q = Quat::from_euler(EulerRot::YXZ, 0.2, -0.1, 0.05);
        let c = Calibration { neutral: Some("idle".into()), approved: vec!["idle".into()], actor: Actor { posture: [("hips".to_owned(), Turn::of(q))].into(), arm_hang: Some([5.0, 6.0]) }, avatar: Avatar::default() };
        let back: Calibration = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
        assert_eq!(back, c);
        assert!(back.actor.posture["hips"].quat().angle_between(q) < 1e-3);
    }
}
