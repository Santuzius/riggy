//! Balance: the body's centre of mass stays above the part of the soles that touches the floor, as a standing person keeps it by turning at the ankles. A figure on its toes touches the floor with a few centimetres of forefoot only; built leaning forward, or leaning further in a clip, it would tip over.
//!
//! Every frame, after the whole pose is built, the centre of mass is estimated from the joints with average segment masses: each segment's share of the body's mass at a fixed fraction of its length. The soles' points that lie on the floor give the support, measured along the figure's forward direction. Where the centre lies outside the middle of that support, the whole body above the ankles turns about the line through both ankles until it lies on the middle's edge; the feet keep their place and angle. The turn follows with a short lag, as a person's balance does.
//!
//! Source: P. de Leva, "Adjustments to Zatsiorsky-Seluyanov's segment inertia parameters", Journal of Biomechanics 29(9), 1996.

use bevy::math::Affine3A;
use bevy::prelude::*;

use super::Library;
use crate::avatar::{AvatarRoot, Humanoid, HumanoidBone as B};

/// Share of the support's length at each end that the centre of mass is kept out of.
const MARGIN: f32 = 0.25;
/// Seconds the turn takes to follow the pose.
const LAG: f32 = 0.3;
/// Largest turn, in radians (about 8°).
const MOST: f32 = 0.14;
/// Share of the hips' height over the ankles at rest below which the figure is taken to sit, kneel or crouch, and is not balanced on its soles.
const LOW: f32 = 0.75;
/// Body segments: from joint, to joint (the first one the avatar has), share of the body's mass and the segment's centre as a fraction of the way between them (de Leva 1996, trunk as one segment).
const SEGMENTS: [(B, &[B], f32, f32); 14] = [
    (B::Hips, &[B::UpperChest, B::Chest, B::Spine], 0.432, 0.5),
    (B::Neck, &[B::Head], 0.069, 0.6),
    (B::LeftUpperArm, &[B::LeftLowerArm], 0.027, 0.45),
    (B::RightUpperArm, &[B::RightLowerArm], 0.027, 0.45),
    (B::LeftLowerArm, &[B::LeftHand], 0.016, 0.43),
    (B::RightLowerArm, &[B::RightHand], 0.016, 0.43),
    (B::LeftHand, &[B::LeftHand], 0.006, 0.0),
    (B::RightHand, &[B::RightHand], 0.006, 0.0),
    (B::LeftUpperLeg, &[B::LeftLowerLeg], 0.142, 0.43),
    (B::RightUpperLeg, &[B::RightLowerLeg], 0.142, 0.43),
    (B::LeftLowerLeg, &[B::LeftFoot], 0.043, 0.43),
    (B::RightLowerLeg, &[B::RightFoot], 0.043, 0.43),
    (B::LeftFoot, &[B::LeftToes, B::LeftFoot], 0.014, 0.45),
    (B::RightFoot, &[B::RightToes, B::RightFoot], 0.014, 0.45),
];

/// The turn at the ankles now, in radians, positive tipping the body backwards.
#[derive(Component, Default)]
pub(super) struct Tilt(f32);

type Tree<'w, 's> = Query<'w, 's, (&'static mut Transform, Option<&'static ChildOf>, Has<AvatarRoot>)>;

/// Avatar-space transform of `entity` as posed now.
fn posed(entity: Entity, tree: &Tree) -> Affine3A {
    let mut affine = Affine3A::IDENTITY;
    let mut current = entity;
    while let Ok((transform, parent, is_root)) = tree.get(current) {
        if is_root {
            break;
        }
        affine = transform.compute_affine() * affine;
        match parent {
            Some(p) => current = p.parent(),
            None => break,
        }
    }
    affine
}

pub(super) fn balance(mut commands: Commands, time: Res<Time>, library: Res<Library>, mut avatars: Query<(Entity, &Humanoid, Option<&mut Tilt>)>, mut tree: Tree) {
    if std::env::var_os("RIGGY_NO_BALANCE").is_some() {
        return;
    }
    let Some(clearance) = library.clearance.clone() else { return };
    // A long frame (a clip being prepared) must not make the turn jump.
    let follow = 1.0 - (-time.delta_secs().min(0.05) / LAG).exp();
    for (entity, h, tilt) in &mut avatars {
        let Some(mut tilt) = tilt else {
            commands.entity(entity).insert(Tilt::default());
            continue;
        };
        // Where the support cannot be told for a moment (both feet lifted), the turn stays.
        if let Some(wanted) = wanted_tilt(h, &clearance, &tree) {
            tilt.0 += (wanted - tilt.0) * follow;
        }
        tip(h, tilt.0, &mut tree);
    }
}

/// The ankles' line (pivot and unit axis, to the figure's left) and the figure's forward direction across it.
fn ankle_line(h: &Humanoid, tree: &Tree) -> Option<(Vec3, Vec3, Vec3)> {
    let left: Vec3 = posed(h.get(B::LeftFoot)?.entity, tree).translation.into();
    let right: Vec3 = posed(h.get(B::RightFoot)?.entity, tree).translation.into();
    let axis = (left - right).normalize_or_zero();
    let forward = axis.cross(Vec3::Y).with_y(0.0).normalize_or_zero();
    (axis != Vec3::ZERO && forward != Vec3::ZERO).then_some(((left + right) / 2.0, axis, forward))
}

/// The turn at the ankles that brings the centre of mass into the middle of the support.
fn wanted_tilt(h: &Humanoid, clearance: &super::clearance::Clearance, tree: &Tree) -> Option<f32> {
    let at = |b: B| -> Option<Vec3> { Some(posed(h.get(b)?.entity, tree).translation.into()) };
    let (pivot, axis, forward) = ankle_line(h, tree)?;
    // Sitting, kneeling or crouching low, the body rests on more than the soles: no turn.
    let hips_over_ankles = |hips: Vec3, foot: Vec3| hips.y - foot.y;
    if hips_over_ankles(at(B::Hips)?, pivot) < LOW * hips_over_ankles(h.position(B::Hips)?, h.position(B::LeftFoot)?) {
        return Some(0.0);
    }

    let (mut centre, mut mass) = (Vec3::ZERO, 0.0);
    for (from, to, share, fraction) in SEGMENTS {
        let (Some(a), Some(b)) = (at(from), to.iter().find_map(|&b| at(b))) else { continue };
        centre += share * a.lerp(b, fraction);
        mass += share;
    }
    let centre = centre / mass;

    // Metres in the soles' frames to avatar-space units.
    let scale = (h.position(B::Hips)? - h.position(B::LeftFoot)?).length() / (clearance.rest_position(B::Hips)? - clearance.rest_position(B::LeftFoot)?).length();
    let mut points = Vec::new();
    for (b, sole) in &clearance.soles {
        let (_, rotation, translation) = posed(h.get(*b)?.entity, tree).to_scale_rotation_translation();
        points.extend(sole.iter().map(|&q| translation + rotation * (scale * q)));
    }
    let floor = points.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    let along: Vec<f32> = points.iter().filter(|p| p.y < floor + 0.01 * h.height).map(|&p| (p - pivot).dot(forward)).collect();
    if along.is_empty() {
        return None;
    }
    let (back, front) = along.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &a| (lo.min(a), hi.max(a)));
    let margin = MARGIN * (front - back);
    let ahead = (centre - pivot).dot(forward);
    let lever = axis.cross(centre - pivot).dot(forward);
    if lever.abs() < 1e-3 * h.height {
        return None;
    }
    // A turn by `tilt` about `-axis` (backwards) moves the centre by about `-tilt * lever` along `forward`.
    Some(((ahead - ahead.clamp(back + margin, front - margin)) / lever).clamp(-MOST, MOST))
}

/// Turns everything above the ankles backwards by `tilt` about the ankles' line; the feet keep their place and angle.
fn tip(h: &Humanoid, tilt: f32, tree: &mut Tree) {
    let Some((pivot, axis, _)) = ankle_line(h, tree) else { return };
    let turn = Quat::from_axis_angle(axis, -tilt);
    let about = Affine3A::from_translation(pivot) * Affine3A::from_quat(turn) * Affine3A::from_translation(-pivot);
    let Some(hips) = h.get(B::Hips) else { return };
    let feet: Vec<(Entity, Quat, Quat)> = [B::LeftFoot, B::RightFoot]
        .into_iter()
        .filter_map(|b| {
            let entity = h.get(b)?.entity;
            let parent = tree.get(entity).ok()?.1?.parent();
            Some((entity, posed(entity, tree).to_scale_rotation_translation().1, posed(parent, tree).to_scale_rotation_translation().1))
        })
        .collect();
    let hips_now = posed(hips.entity, tree);
    let local = tree.get(hips.entity).map(|(t, _, _)| t.compute_affine()).unwrap_or_default();
    let parent = hips_now * local.inverse();
    if let Ok((mut t, _, _)) = tree.get_mut(hips.entity) {
        let (scale, rotation, translation) = (parent.inverse() * about * hips_now).to_scale_rotation_translation();
        *t = Transform { translation, rotation, scale };
    }
    // Each foot's parent turned with the body; the foot turns back so it keeps its angle to the floor.
    for (entity, foot, parent) in feet {
        if let Ok((mut t, _, _)) = tree.get_mut(entity) {
            t.rotation = ((turn * parent).inverse() * foot).normalize();
        }
    }
}
