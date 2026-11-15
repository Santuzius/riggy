//! Retargeting: turns a clip's bone rotations into rotations of an avatar's bones, whatever pose either skeleton rests in.
//!
//! Let W(t) be a clip bone's rotation at time t and W₀ its rest rotation. The clip turns the bone by M(t) = W(t)·W₀⁻¹. The avatar's bone rests in another pose; the shortest rotation C that turns the clip bone's rest direction (towards its child) onto the avatar bone's rest direction aligns the two. The avatar bone then turns by D(t) = M(t)·C⁻¹, so it points where the clip bone points; in the clip's rest pose D = C⁻¹ puts the avatar into that pose. Each bone is turned on top of its parent's turn: R(t) = D_parent(t)⁻¹·D(t), which is what [`crate::avatar::Humanoid::rotate`] applies.

use std::collections::HashMap;

use bevy::prelude::*;
use riggy::clips::Clip;

use crate::avatar::{Humanoid, HumanoidBone as B};

/// A clip as rotations of one avatar's bones.
#[derive(Debug)]
pub struct Retargeted {
    pub fps: f32,
    pub frames: usize,
    /// Each bone's rotation per frame, relative to its parent (avatar space).
    pub bones: Vec<(B, Vec<Quat>)>,
}

impl Retargeted {
    pub fn duration(&self) -> f32 {
        self.frames.saturating_sub(1) as f32 / self.fps
    }

    /// The rotations at `time` seconds, between the two nearest frames.
    pub fn sample(&self, time: f32) -> impl Iterator<Item = (B, Quat)> + '_ {
        let at = (time * self.fps).clamp(0.0, self.frames.saturating_sub(1) as f32);
        let (i, f) = (at.floor() as usize, at.fract());
        let j = (i + 1).min(self.frames.saturating_sub(1));
        self.bones.iter().map(move |(bone, q)| (*bone, q[i].slerp(q[j], f)))
    }
}

/// The humanoid bone a bone hangs from in the VRM 1.0 hierarchy.
pub fn parent(bone: B) -> Option<B> {
    use B::*;
    Some(match bone {
        Hips => return None,
        Spine | LeftUpperLeg | RightUpperLeg => Hips,
        Chest => Spine,
        UpperChest => Chest,
        Neck | LeftShoulder | RightShoulder => UpperChest,
        Head => Neck,
        LeftEye | RightEye | Jaw => Head,
        LeftLowerLeg => LeftUpperLeg,
        LeftFoot => LeftLowerLeg,
        LeftToes => LeftFoot,
        RightLowerLeg => RightUpperLeg,
        RightFoot => RightLowerLeg,
        RightToes => RightFoot,
        LeftUpperArm => LeftShoulder,
        LeftLowerArm => LeftUpperArm,
        LeftHand => LeftLowerArm,
        RightUpperArm => RightShoulder,
        RightLowerArm => RightUpperArm,
        RightHand => RightLowerArm,
        LeftThumbMetacarpal | LeftIndexProximal | LeftMiddleProximal | LeftRingProximal | LeftLittleProximal => LeftHand,
        RightThumbMetacarpal | RightIndexProximal | RightMiddleProximal | RightRingProximal | RightLittleProximal => RightHand,
        LeftThumbProximal => LeftThumbMetacarpal,
        LeftThumbDistal => LeftThumbProximal,
        LeftIndexIntermediate => LeftIndexProximal,
        LeftIndexDistal => LeftIndexIntermediate,
        LeftMiddleIntermediate => LeftMiddleProximal,
        LeftMiddleDistal => LeftMiddleIntermediate,
        LeftRingIntermediate => LeftRingProximal,
        LeftRingDistal => LeftRingIntermediate,
        LeftLittleIntermediate => LeftLittleProximal,
        LeftLittleDistal => LeftLittleIntermediate,
        RightThumbProximal => RightThumbMetacarpal,
        RightThumbDistal => RightThumbProximal,
        RightIndexIntermediate => RightIndexProximal,
        RightIndexDistal => RightIndexIntermediate,
        RightMiddleIntermediate => RightMiddleProximal,
        RightMiddleDistal => RightMiddleIntermediate,
        RightRingIntermediate => RightRingProximal,
        RightRingDistal => RightRingIntermediate,
        RightLittleIntermediate => RightLittleProximal,
        RightLittleDistal => RightLittleIntermediate,
    })
}

/// The child a bone points to: along the spine, the limb or the finger; a hand points to its middle finger. Last bones of a chain have none.
fn chain_child(bone: B) -> Option<B> {
    use B::*;
    match bone {
        Hips => Some(Spine),
        Chest => Some(UpperChest),
        UpperChest => Some(Neck),
        LeftHand => Some(LeftMiddleProximal),
        RightHand => Some(RightMiddleProximal),
        _ => B::ALL.iter().copied().find(|&c| parent(c) == Some(bone) && !matches!(c, LeftEye | RightEye | Jaw)),
    }
}

/// The clip `clip` as rotations of the bones `include` picks among those that both the avatar `h` and the clip have.
pub fn retarget(clip: &Clip, h: &Humanoid, include: impl Fn(B) -> bool) -> Retargeted {
    let bones: HashMap<B, &riggy::clips::ClipBone> = clip.bones.iter().filter_map(|(name, bone)| Some((name.parse::<B>().ok()?, bone))).filter(|(b, _)| h.get(*b).is_some()).collect();
    let shared = |b: B| bones.contains_key(&b);
    // The nearest shared bone along a direction, skipping bones one side lacks.
    let nearest = |mut b: Option<B>, step: fn(B) -> Option<B>| {
        while let Some(x) = b {
            if shared(x) {
                return Some(x);
            }
            b = step(x);
        }
        None
    };

    // Rest alignment C of every shared bone, parents first (B::ALL lists them so).
    let mut align: HashMap<B, Quat> = HashMap::new();
    for &b in B::ALL.iter().filter(|&&b| shared(b)) {
        let c = match nearest(chain_child(b), chain_child) {
            Some(tip) => {
                let from = (bones[&tip].rest_position - bones[&b].rest_position).normalize_or_zero();
                let to = (h.position(tip).unwrap() - h.position(b).unwrap()).normalize_or_zero();
                if from == Vec3::ZERO || to == Vec3::ZERO { None } else { Some(Quat::from_rotation_arc(from, to)) }
            }
            None => None,
        };
        let inherited = || nearest(parent(b), parent).and_then(|p| align.get(&p).copied()).unwrap_or(Quat::IDENTITY);
        align.insert(b, c.unwrap_or_else(inherited));
    }

    let turn = |b: B, f: usize| {
        let bone = bones[&b];
        bone.rotations[f] * bone.rest_rotation.inverse() * align[&b].inverse()
    };
    let out = B::ALL
        .iter()
        .copied()
        .filter(|&b| shared(b) && include(b))
        .map(|b| {
            let above = nearest(parent(b), parent);
            let rotations = (0..clip.frames)
                .map(|f| {
                    let d = turn(b, f);
                    let r = match above {
                        Some(p) => turn(p, f).inverse() * d,
                        None => d,
                    };
                    r.normalize()
                })
                .collect();
            (b, rotations)
        })
        .collect();
    Retargeted { fps: clip.fps, frames: clip.frames, bones: out }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::avatar::PoseBone;
    use bevy::math::Affine3A;
    use riggy::clips::ClipBone;

    fn humanoid(points: &[(B, Vec3)]) -> Humanoid {
        let mut h = Humanoid::default();
        for &(b, p) in points {
            h.bones.insert(b, PoseBone::new(Entity::PLACEHOLDER, Transform::from_translation(p), Affine3A::IDENTITY));
        }
        h
    }

    #[test]
    fn every_bone_has_a_parent_but_the_hips() {
        assert!(B::ALL.iter().all(|&b| (b == B::Hips) == parent(b).is_none()));
    }

    /// The clip's arm rests hanging down, the avatar's sideways (T-pose). A clip frame that lifts the arm sideways leaves the avatar's arm at rest; the clip's rest frame lowers it.
    #[test]
    fn an_arm_from_an_a_pose_clip_points_where_the_clip_points() {
        let lift = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let bone = |p: Vec3, rotations: Vec<Quat>| ClipBone { rest_position: p, rest_rotation: Quat::IDENTITY, rotations };
        let clip = Clip {
            fps: 30.0,
            frames: 2,
            bones: [
                ("leftUpperArm".to_owned(), bone(Vec3::new(0.2, 1.4, 0.0), vec![Quat::IDENTITY, lift])),
                ("leftLowerArm".to_owned(), bone(Vec3::new(0.2, 1.1, 0.0), vec![Quat::IDENTITY, lift])),
            ]
            .into(),
        };
        let h = humanoid(&[(B::LeftUpperArm, Vec3::new(0.2, 1.4, 0.0)), (B::LeftLowerArm, Vec3::new(0.5, 1.4, 0.0))]);
        let r = retarget(&clip, &h, |_| true);
        let (b, q) = &r.bones[0];
        assert_eq!(*b, B::LeftUpperArm);
        // Rest frame: the avatar's arm (pointing +X) turns to point down.
        assert!((q[0] * Vec3::X - Vec3::NEG_Y).length() < 1e-5);
        // Lifted frame: back along +X.
        assert!((q[1] * Vec3::X - Vec3::X).length() < 1e-5);
        // The forearm follows its parent without turning against it.
        assert!(r.bones[1].1.iter().all(|q| q.angle_between(Quat::IDENTITY) < 1e-4));
    }

    /// Retargets a Mixamo-named mocap clip onto the rest pose of a Motifect skeleton and checks, through the parent chain, that every bone with a child points where the clip's bone points: `cargo test -- --ignored a_real_clip`.
    #[test]
    #[ignore = "reads the user's installed clips"]
    fn a_real_clip_points_the_bones_where_it_points_them() {
        let clips = riggy::clips::installed();
        let load = |name: &str| {
            let file = riggy::clips::find(&clips, name).unwrap();
            Clip::load(&file.path, &file.skeletons_dir()).unwrap()
        };
        let clip = load("07_01_002_talking normal");
        let target = load("idle_neutral");
        let h = humanoid(&target.bones.iter().filter_map(|(n, b)| Some((n.parse::<B>().ok()?, b.rest_position))).collect::<Vec<_>>());
        let r = retarget(&clip, &h, |_| true);
        let rotations: HashMap<B, &Vec<Quat>> = r.bones.iter().map(|(b, q)| (*b, q)).collect();
        let mut worst = 0.0f32;
        for f in (0..clip.frames).step_by(37) {
            for (&b, _) in &rotations {
                let Some(tip) = chain_child(b).filter(|t| rotations.contains_key(t)) else { continue };
                // The avatar bone's turn: its own and its ancestors', the outermost applied last.
                let mut world = Quat::IDENTITY;
                let mut x = Some(b);
                while let Some(y) = x {
                    if let Some(q) = rotations.get(&y) {
                        world = q[f] * world;
                    }
                    x = parent(y);
                }
                let avatar = world * (h.position(tip).unwrap() - h.position(b).unwrap()).normalize();
                let cb = &clip.bones[b.as_str()];
                let ct = &clip.bones[tip.as_str()];
                let wanted = cb.rotations[f] * cb.rest_rotation.inverse() * (ct.rest_position - cb.rest_position).normalize();
                worst = worst.max(avatar.angle_between(wanted).to_degrees());
            }
        }
        assert!(worst < 1.0, "a bone points {worst:.2}° away from the clip");
    }
}

