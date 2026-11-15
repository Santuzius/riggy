//! Retargeting: turns a clip's bone rotations into rotations of an avatar's bones, whatever pose either skeleton rests in.
//!
//! Let W(t) be a clip bone's rotation at time t and W₀ its rest rotation. The clip turns the bone by M(t) = W(t)·W₀⁻¹. The avatar's bone rests in another pose; the shortest rotation C that turns the clip bone's rest direction (towards its child) onto the avatar bone's rest direction aligns the two. The avatar bone then turns by D(t) = M(t)·C⁻¹, so it points where the clip bone points; in the clip's rest pose D = C⁻¹ puts the avatar into that pose. Each bone is turned on top of its parent's turn: R(t) = D_parent(t)⁻¹·D(t), which is what [`crate::avatar::Humanoid::rotate`] applies.

use std::collections::HashMap;

use bevy::prelude::*;
use riggy::clips::Clip;
use riggy::clips::calibration::Calibration;

use crate::avatar::{Humanoid, HumanoidBone as B};

/// A clip as rotations of one avatar's bones.
#[derive(Debug)]
pub struct Retargeted {
    pub fps: f32,
    pub frames: usize,
    /// Each bone's rotation per frame, relative to its parent (avatar space).
    pub bones: Vec<(B, Vec<Quat>)>,
    // Read by the off-screen checks (`sticks`).
    #[cfg_attr(not(test), allow(dead_code))]
    /// Whether the trunk keeps the avatar's posture, calibrated on the pack's neutral clip (see [`retarget`]).
    pub calibrated: bool,
    #[cfg_attr(not(test), allow(dead_code))]
    /// Per frame, the swing of the left and right arm about the shoulder that brings the actor's hanging arms to the avatar's natural hang (see [`retarget`]).
    pub arm_swings: Vec<[Quat; 2]>,
}

/// The arm, 0 left and 1 right, that `bone` belongs to, from the upper arm down to the fingers.
pub fn arm_of(mut bone: B) -> Option<usize> {
    loop {
        match bone {
            B::LeftUpperArm => return Some(0),
            B::RightUpperArm => return Some(1),
            _ => bone = parent(bone)?,
        }
    }
}

impl Retargeted {
    pub fn duration(&self) -> f32 {
        self.frames.saturating_sub(1) as f32 / self.fps
    }

    /// Whether the clip turns any of `bones`.
    pub fn moves_any(&self, bones: &[B]) -> bool {
        self.bones.iter().any(|(b, _)| bones.contains(b))
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

/// The trunk from the hips up and the shoulders: they keep the avatar's own posture where the pack names a neutral clip.
pub const POSTURE: [B; 8] = [B::Hips, B::Spine, B::Chest, B::UpperChest, B::Neck, B::Head, B::LeftShoulder, B::RightShoulder];

/// Two bones beside each other that span a bone's width across its axis: the hips' and the legs' between the thighs, the hands' and the arms' and fingers' across the knuckles, the head's between the eyes. Aligning the width too turns each bone about its own axis as the clip's, so a palm faces where the clip's faces.
fn width(bone: B) -> Option<(B, B)> {
    use B::*;
    let side = |left: B, right: B| if bone.as_str().starts_with("left") { left } else { right };
    Some(match bone {
        Hips | Spine | LeftUpperLeg | RightUpperLeg | LeftLowerLeg | RightLowerLeg | LeftFoot | RightFoot | LeftToes | RightToes => (LeftUpperLeg, RightUpperLeg),
        Chest | UpperChest | Neck => (LeftShoulder, RightShoulder),
        Head => (LeftEye, RightEye),
        LeftEye | RightEye | Jaw => return None,
        _ => (side(LeftIndexProximal, RightIndexProximal), side(LeftLittleProximal, RightLittleProximal)),
    })
}

/// The rotation that turns `from` onto `to` and, about it, `from_width` as near onto `to_width` as it goes; only the first where a width is missing or lies along its direction.
fn aligned(from: Vec3, to: Vec3, widths: Option<(Vec3, Vec3)>) -> Quat {
    let arc = Quat::from_rotation_arc(from, to);
    let frame = |d: Vec3, w: Vec3| {
        let z = d.cross(w).normalize_or_zero();
        (z != Vec3::ZERO).then(|| Quat::from_mat3(&Mat3::from_cols(d, z.cross(d), z)))
    };
    match widths.and_then(|(wf, wt)| Some((frame(from, wf)?, frame(to, wt)?))) {
        Some((f, t)) => (t * f.inverse()).normalize(),
        None => arc,
    }
}

/// The clip `clip` as rotations of the bones `include` picks among those that both the avatar `h` and the clip have; `calibration` is the pack's posture calibration for the avatar ([`riggy::clips::calibration`]), without which trunk and arms point where the actor's do.
pub fn retarget(clip: &Clip, calibration: Option<&Calibration>, h: &Humanoid, include: impl Fn(B) -> bool) -> Retargeted {
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
                let widths = width(b).filter(|&(l, r)| shared(l) && shared(r)).map(|(l, r)| (bones[&l].rest_position - bones[&r].rest_position, h.position(l).unwrap() - h.position(r).unwrap()));
                if from == Vec3::ZERO || to == Vec3::ZERO { None } else { Some(aligned(from, to, widths)) }
            }
            None => None,
        };
        let inherited = || nearest(parent(b), parent).and_then(|p| align.get(&p).copied()).unwrap_or(Quat::IDENTITY);
        align.insert(b, c.unwrap_or_else(inherited));
    }

    let own_turn = |b: B, f: usize| {
        let bone = bones[&b];
        bone.rotations[f] * bone.rest_rotation.inverse() * align[&b].inverse()
    };
    // Arm roll: aligned to the avatar's rest pose, the actor's upper arm in the neutral clip, G_N = N·C⁻¹, may be rolled about its own axis against how riggy's idle hangs the avatar's arm, I_N: the shortest turn from the arm's rest direction a to G_N·a (the shoulders then face elsewhere). The avatar's upper arm turns by G(t)·G_N⁻¹·I_N, which points it where G(t) does and is I_N in the neutral clip.
    let unroll: HashMap<B, Quat> = [(B::LeftUpperArm, B::LeftLowerArm), (B::RightUpperArm, B::RightLowerArm)]
        .into_iter()
        .filter(|&(u, _)| shared(u))
        .filter_map(|(u, l)| {
            let n = calibration?.actor.posture.get(u.as_str())?.quat();
            let a = (h.position(l)? - h.position(u)?).normalize_or_zero();
            let g = n * align[&u].inverse();
            Some((u, g.inverse() * Quat::from_rotation_arc(a, (g * a).normalize_or_zero())))
        })
        .collect();
    let own_turn = |b: B, f: usize| match unroll.get(&b) {
        Some(&u) => (own_turn(b, f) * u).normalize(),
        None => own_turn(b, f),
    };

    // Posture: the trunk (hips to head) and the shoulders keep the avatar's own posture and moves only as far as the actor leaves theirs. Q(t) = M_parent(t)⁻¹·M(t) is how the clip turns such a bone against its parent; N = N_parent⁻¹·N_bone, from the calibration's mean turns in the neutral clip, is the actor's natural posture. The avatar's bone turns against its parent by E(t) = Q(t)·N⁻¹, which is no turn in the actor's natural posture. Limbs, hanging from the trunk, still point where the clip's point.
    let trunk_parent = |b: B| nearest(parent(b), parent).filter(|p| POSTURE.contains(p));
    let m = |b: B, f: usize| bones[&b].rotations[f] * bones[&b].rest_rotation.inverse();
    let relative = |b: B, f: usize| match trunk_parent(b) {
        Some(p) => m(p, f).inverse() * m(b, f),
        None => m(b, f),
    };
    let natural = |b: B| calibration?.actor.posture.get(b.as_str()).map(|t| t.quat());
    let trunk_posture: HashMap<B, Quat> = POSTURE
        .iter()
        .copied()
        .filter(|&b| shared(b))
        .filter_map(|b| {
            let n = natural(b)?;
            Some((b, match trunk_parent(b) { Some(p) => natural(p)?.inverse() * n, None => n }))
        })
        .collect();
    let trunk_turns: Vec<HashMap<B, Quat>> = (0..clip.frames)
        .map(|f| {
            let mut d: HashMap<B, Quat> = HashMap::new();
            for &b in POSTURE.iter().filter(|b| trunk_posture.contains_key(b)) {
                let e = relative(b, f) * trunk_posture[&b].inverse();
                let above = trunk_parent(b).and_then(|p| d.get(&p).copied()).unwrap_or(Quat::IDENTITY);
                d.insert(b, (above * e).normalize());
            }
            d
        })
        .collect();
    // Arm hang: the actor's arms may hang closer to the body than the avatar's can (wide hips, a coat). The line from shoulder to wrist hangs out from vertical by the calibration's `actor.arm_hang` in the actor's natural posture and by `avatar.arm_hang` on the avatar at ease, as riggy's idle holds its upper arm (the elbow bends forward, so the wrist stays in that plane). Each arm swings out about the shoulder by the difference, in full where the arm hangs straight down and not at all where it is raised to horizontal or above, so gestures keep their shape.
    let arm_line = |side: usize, f: usize| {
        let [upper, lower, hand] = [[B::LeftUpperArm, B::LeftLowerArm, B::LeftHand], [B::RightUpperArm, B::RightLowerArm, B::RightHand]][side];
        let (u, l, w) = (bones.get(&upper)?, bones.get(&lower)?, bones.get(&hand)?);
        let m = |b: &riggy::clips::ClipBone| b.rotations[f] * b.rest_rotation.inverse();
        Some((m(u) * (l.rest_position - u.rest_position) + m(l) * (w.rest_position - l.rest_position)).normalize_or_zero())
    };
    let hang: [Option<f32>; 2] = std::array::from_fn(|side| {
        let c = calibration?;
        Some((c.avatar.arm_hang - c.actor.arm_hang?[side]).to_radians())
    });
    let arm_swings: Vec<[Quat; 2]> = (0..clip.frames)
        .map(|f| {
            std::array::from_fn(|side| match (hang[side], arm_line(side, f)) {
                (Some(delta), Some(d)) => Quat::from_rotation_z(if side == 0 { 1.0 } else { -1.0 } * delta * (-d.y).max(0.0)),
                _ => Quat::IDENTITY,
            })
        })
        .collect();
    let turn = |b: B, f: usize| {
        let d = trunk_turns[f].get(&b).copied().unwrap_or_else(|| own_turn(b, f));
        match arm_of(b) {
            Some(side) => arm_swings[f][side] * d,
            None => d,
        }
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
    Retargeted { fps: clip.fps, frames: clip.frames, bones: out, calibrated: !trunk_posture.is_empty(), arm_swings }
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
            neutral: None,
        };
        let h = humanoid(&[(B::LeftUpperArm, Vec3::new(0.2, 1.4, 0.0)), (B::LeftLowerArm, Vec3::new(0.5, 1.4, 0.0))]);
        let r = retarget(&clip, None, &h, |_| true);
        let (b, q) = &r.bones[0];
        assert_eq!(*b, B::LeftUpperArm);
        // Rest frame: the avatar's arm (pointing +X) turns to point down.
        assert!((q[0] * Vec3::X - Vec3::NEG_Y).length() < 1e-5);
        // Lifted frame: back along +X.
        assert!((q[1] * Vec3::X - Vec3::X).length() < 1e-5);
        // The forearm follows its parent without turning against it.
        assert!(r.bones[1].1.iter().all(|q| q.angle_between(Quat::IDENTITY) < 1e-4));
    }

    /// The clip's hand rests palm down, the avatar's palm forward, both pointing +X: the hand turns about its axis too, so the knuckles lie as the clip's.
    #[test]
    fn a_hand_turns_about_its_axis_as_the_clip_s() {
        let c = aligned(Vec3::X, Vec3::X, Some((Vec3::Z, Vec3::Y)));
        assert!((c * Vec3::X - Vec3::X).length() < 1e-5);
        assert!((c * Vec3::Z - Vec3::Y).length() < 1e-5);
        // Without a width only the direction is aligned.
        assert!(aligned(Vec3::X, Vec3::Y, None).angle_between(Quat::from_rotation_arc(Vec3::X, Vec3::Y)) < 1e-3);
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
        let r = retarget(&clip, None, &h, |_| true);
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

