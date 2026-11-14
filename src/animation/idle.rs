//! Procedural idle motion on humanoid bones: breathing, a slow shift of weight between the legs, a drifting head, and relaxed arms. While riggy speaks, sway and head drift shrink to `[idle] speaking_calm`, so the figure looks at the listener.
//!
//! Uses only VRM humanoid bone names, so it works the same on every backend. Angles from `[idle]` in `riggy.toml` are degrees, rates Hz. The legs follow the hips through `stance`, which keeps the feet on the floor.

use std::f32::consts::TAU;

use bevy::prelude::*;

use super::stance::{self, Stance};
use crate::avatar::{AvatarSet, ExpressionPreset, Expressions, Humanoid, HumanoidBone as B};
use crate::config::Config;
use crate::speech::Voice;

/// Seconds the motion takes to calm down when speech starts, and to come back after it.
const CALM_TIME: f32 = 0.8;

pub struct IdlePlugin;

impl Plugin for IdlePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (idle.in_set(AvatarSet::Pose), smile.in_set(AvatarSet::Express)));
    }
}

/// Smooth pseudo-noise in −1..1: two sines with an irrational frequency ratio, so the pattern does not visibly repeat.
fn drift(t: f32, rate: f32, phase: f32) -> f32 {
    0.65 * (TAU * rate * t + phase).sin() + 0.35 * (TAU * rate * 1.618 * t + 2.0 * phase + 1.3).sin()
}

/// Holds the expression `happy` at `[idle] smile`, eased to `speaking_smile` while speaking.
fn smile(time: Res<Time>, config: Res<Config>, voice: Res<Voice>, mut weight: Local<Option<f32>>, mut avatars: Query<&mut Expressions, With<Humanoid>>) {
    let c = &config.idle;
    let wanted = if voice.speaking { c.speaking_smile } else { c.smile };
    let k = 1.0 - (-time.delta_secs() / CALM_TIME).exp();
    let now = weight.get_or_insert(wanted);
    *now += (wanted - *now) * k;
    for mut expressions in &mut avatars {
        expressions.set(ExpressionPreset::Happy, *now);
    }
}

/// Scale of sway and head drift: 1 in silence, `[idle] speaking_calm` while speaking, eased between them.
struct Liveliness(f32);

impl Default for Liveliness {
    fn default() -> Self {
        Self(1.0)
    }
}

fn idle(time: Res<Time>, config: Res<Config>, voice: Res<Voice>, mut liveliness: Local<Liveliness>, avatars: Query<(&Humanoid, Option<&Stance>)>, mut transforms: Query<&mut Transform>) {
    let c = &config.idle;
    let t = time.elapsed_secs();
    let rad = f32::to_radians;
    let wanted = if voice.speaking { c.speaking_calm.clamp(0.0, 1.0) } else { 1.0 };
    liveliness.0 += (wanted - liveliness.0) * (1.0 - (-time.delta_secs() / CALM_TIME).exp());
    let live = liveliness.0;
    for (h, stance) in &avatars {
        // Breathing: the chest lifts and leans back a little on the in-breath.
        let breath = (TAU * c.breath_rate * t).sin();
        let lift = Quat::from_rotation_x(-rad(c.breath_angle) * 0.5 * breath);
        h.rotate(B::Chest, lift, &mut transforms);
        h.rotate(B::UpperChest, lift, &mut transforms);
        h.rotate(B::Neck, lift.inverse(), &mut transforms);

        // Weight shift: the pelvis moves sideways and its loaded side rises; the legs bend so the feet stay put, the spine so the head stays upright.
        let sway = live * drift(t, c.sway_rate, 0.0);
        let shift = Vec3::X * c.sway_shift * h.height * sway;
        let roll = rad(c.sway_angle) * sway;
        match stance {
            Some(stance) => stance::stand(h, stance, shift, Quat::from_rotation_z(roll), &mut transforms),
            None => {
                h.translate(B::Hips, shift, &mut transforms);
                h.rotate(B::Hips, Quat::from_rotation_z(roll), &mut transforms);
            }
        }
        h.rotate(B::Spine, Quat::from_rotation_z(-roll * 0.6), &mut transforms);
        h.rotate(B::Chest, Quat::from_rotation_z(-roll * 0.4), &mut transforms);

        // Head: slow drift in all three axes, split between neck and head.
        let angle = live * rad(c.head_angle);
        let head = Quat::from_euler(
            EulerRot::YXZ,
            angle * drift(t, c.head_rate, 0.4),
            angle * 0.6 * drift(t, c.head_rate * 0.8, 2.1),
            angle * 0.4 * drift(t, c.head_rate * 0.6, 4.0),
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
