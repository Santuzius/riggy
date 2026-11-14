//! Procedural idle motion on humanoid bones: breathing, a slow shift of weight between the legs, a drifting head, and relaxed arms.
//!
//! Uses only VRM humanoid bone names, so it works the same on every backend. Angles from `[idle]` in `riggy.toml` are degrees, rates Hz.

use std::f32::consts::TAU;

use bevy::prelude::*;

use crate::avatar::{AvatarSet, Humanoid, HumanoidBone as B};
use crate::config::Config;

pub struct IdlePlugin;

impl Plugin for IdlePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, idle.in_set(AvatarSet::Pose));
    }
}

/// Smooth pseudo-noise in −1..1: two sines with an irrational frequency ratio, so the pattern does not visibly repeat.
fn drift(t: f32, rate: f32, phase: f32) -> f32 {
    0.65 * (TAU * rate * t + phase).sin() + 0.35 * (TAU * rate * 1.618 * t + 2.0 * phase + 1.3).sin()
}

fn idle(time: Res<Time>, config: Res<Config>, avatars: Query<&Humanoid>, mut transforms: Query<&mut Transform>) {
    let c = &config.idle;
    let t = time.elapsed_secs();
    let rad = f32::to_radians;
    for h in &avatars {
        // Breathing: the chest lifts and leans back a little on the in-breath.
        let breath = (TAU * c.breath_rate * t).sin();
        let lift = Quat::from_rotation_x(-rad(c.breath_angle) * 0.5 * breath);
        h.rotate(B::Chest, lift, &mut transforms);
        h.rotate(B::UpperChest, lift, &mut transforms);
        h.rotate(B::Neck, lift.inverse(), &mut transforms);

        // Weight shift: the pelvis moves sideways and its loaded side rises; the thighs counter-rotate so the feet stay put, the spine so the head stays upright.
        let sway = drift(t, c.sway_rate, 0.0);
        let shift = c.sway_shift * h.height * sway;
        let roll = rad(c.sway_angle) * sway;
        h.translate(B::Hips, Vec3::X * shift, &mut transforms);
        h.rotate(B::Hips, Quat::from_rotation_z(roll), &mut transforms);
        if let (Some(hip), Some(foot)) = (h.position(B::LeftUpperLeg), h.position(B::LeftFoot)) {
            let lean = (shift / (hip.y - foot.y).max(1e-3)).atan();
            let legs = Quat::from_rotation_z(-roll - lean);
            h.rotate(B::LeftUpperLeg, legs, &mut transforms);
            h.rotate(B::RightUpperLeg, legs, &mut transforms);
        }
        h.rotate(B::Spine, Quat::from_rotation_z(-roll * 0.6), &mut transforms);
        h.rotate(B::Chest, Quat::from_rotation_z(-roll * 0.4), &mut transforms);

        // Head: slow drift in all three axes, split between neck and head.
        let head = Quat::from_euler(
            EulerRot::YXZ,
            rad(c.head_angle) * drift(t, c.head_rate, 0.4),
            rad(c.head_angle) * 0.6 * drift(t, c.head_rate * 0.8, 2.1),
            rad(c.head_angle) * 0.4 * drift(t, c.head_rate * 0.6, 4.0),
        );
        let half = Quat::IDENTITY.slerp(head, 0.4);
        h.rotate(B::Neck, half, &mut transforms);
        h.rotate(B::Head, half.inverse() * head, &mut transforms);

        // Arms: bring the upper arms to `arm_angle` from vertical, bend the elbows a little; they swing slightly with the breath.
        for (upper, lower, side) in [(B::LeftUpperArm, B::LeftLowerArm, 1.0), (B::RightUpperArm, B::RightLowerArm, -1.0)] {
            let (Some(shoulder), Some(elbow)) = (h.position(upper), h.position(lower)) else { continue };
            let arm = elbow - shoulder;
            let rest = (arm.x * side).atan2(-arm.y);
            let target = rad(c.arm_angle) + rad(0.3 * c.breath_angle) * breath;
            h.rotate(upper, Quat::from_rotation_z(side * (target - rest)), &mut transforms);
            h.rotate(lower, Quat::from_rotation_x(-rad(c.elbow_angle)), &mut transforms);
        }
    }
}
