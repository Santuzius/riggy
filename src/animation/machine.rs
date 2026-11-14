//! Motion of machine avatars ([`Machine`]): the body floats and wobbles slowly, parts spin about the vertical axis, and while riggy speaks the parts move apart (expression `open`) and the lights pulse with the voice (expression `glow`).

use std::collections::HashMap;
use std::f32::consts::TAU;

use bevy::prelude::*;

use crate::avatar::{AvatarSet, ExpressionName, Expressions, Machine, Parts};
use crate::speech::Voice;

/// Seconds the glow takes to follow the voice's level.
const GLOW_TIME: f32 = 0.06;
/// Rate in Hz at which the glow breathes while silent.
const BREATH_RATE: f32 = 0.25;

pub struct MachinePlugin;

impl Plugin for MachinePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (float_and_spin.in_set(AvatarSet::Pose), speak.in_set(AvatarSet::Express)));
    }
}

/// The angle of a spinning part in degrees and its current spin, which eases to the wanted one.
#[derive(Default)]
struct Spin {
    degrees: f32,
    rate: f32,
}

/// Eases `now` towards `wanted` with time constant `seconds` over the frame time `dt`.
fn ease(now: f32, wanted: f32, dt: f32, seconds: f32) -> f32 {
    now + (wanted - now) * (1.0 - (-dt / seconds.max(1e-3)).exp())
}

/// The spin of `part` in degrees per second, `None` if it does not spin.
fn spin_rate(machine: &Machine, part: &str, speaking: bool) -> Option<f32> {
    let speaking_rate = speaking.then(|| machine.speaking_spin.get(part)).flatten();
    speaking_rate.or_else(|| machine.spin.get(part)).copied()
}

fn float_and_spin(time: Res<Time>, voice: Res<Voice>, mut spins: Local<HashMap<String, Spin>>, avatars: Query<(&Machine, &Parts)>, mut transforms: Query<&mut Transform>) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    for (machine, parts) in &avatars {
        if let Some(body) = parts.bones.get(&machine.body)
            && let Ok(mut transform) = transforms.get_mut(body.entity)
        {
            // Rises from its rest height and sinks back, so it never dips below the floor.
            let lift = machine.float * 0.5 * (1.0 - (TAU * machine.float_rate * t).cos());
            let wobble = machine.wobble.to_radians();
            let tilt = Quat::from_euler(EulerRot::XYZ, wobble * (TAU * machine.float_rate * 0.7 * t).sin(), 0.0, wobble * (TAU * machine.float_rate * 0.53 * t + 1.0).sin());
            body.translate(&mut transform, Vec3::Y * lift);
            body.rotate(&mut transform, tilt);
        }
        for (part, bone) in &parts.bones {
            let Some(wanted) = spin_rate(machine, part, voice.speaking) else { continue };
            let spin = spins.entry(part.clone()).or_insert(Spin { degrees: 0.0, rate: wanted });
            spin.rate = ease(spin.rate, wanted, dt, machine.open_time);
            spin.degrees = (spin.degrees + spin.rate * dt).rem_euclid(360.0);
            if let Ok(mut transform) = transforms.get_mut(bone.entity) {
                bone.rotate(&mut transform, Quat::from_rotation_y(spin.degrees.to_radians()));
            }
        }
    }
}

/// How far open the machine is and how bright it glows, both 0..1.
#[derive(Default)]
struct Speaking {
    open: f32,
    glow: f32,
}

fn speak(time: Res<Time>, voice: Res<Voice>, mut state: Local<Speaking>, mut avatars: Query<(&Machine, &mut Expressions)>) {
    let dt = time.delta_secs();
    for (machine, mut expressions) in &mut avatars {
        state.open = ease(state.open, if voice.speaking { 1.0 } else { 0.0 }, dt, machine.open_time);
        let glow = if voice.speaking {
            machine.rest_glow + voice.level * machine.glow_gain
        } else {
            machine.rest_glow * (0.75 + 0.25 * (TAU * BREATH_RATE * time.elapsed_secs()).sin())
        };
        state.glow = ease(state.glow, glow, dt, GLOW_TIME);
        // Smoothstep, so the parts start and stop gently.
        let open = state.open * state.open * (3.0 - 2.0 * state.open);
        expressions.set(ExpressionName::Custom("open".into()), open);
        expressions.set(ExpressionName::Custom("glow".into()), state.glow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaking_spin_overrides_only_the_parts_it_lists() {
        let machine = Machine {
            spin: HashMap::from([("top".into(), 20.0), ("middle".into(), -8.0)]),
            speaking_spin: HashMap::from([("middle".into(), -2.0)]),
            ..Machine::default()
        };
        assert_eq!(spin_rate(&machine, "middle", true), Some(-2.0));
        assert_eq!(spin_rate(&machine, "middle", false), Some(-8.0));
        assert_eq!(spin_rate(&machine, "top", true), Some(20.0));
        assert_eq!(spin_rate(&machine, "bottom", true), None);
    }
}
