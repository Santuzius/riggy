//! Mouth: smooths target viseme weights (`aa ih ou ee oh`) and writes them into `Expressions`.
//!
//! Lip sync sets `MouthTarget`; for testing, keys 1–5 hold one viseme fully open and 0 returns to rest.

use bevy::prelude::*;

use crate::avatar::{AvatarSet, ExpressionPreset, Expressions, Humanoid};
use crate::config::Config;

pub struct MouthPlugin;

impl Plugin for MouthPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MouthTarget>()
            .add_systems(Update, (debug_keys, mouth.in_set(AvatarSet::Express)).chain());
    }
}

/// Target weight of each viseme, in the order of `ExpressionPreset::VISEMES`.
#[derive(Resource, Default)]
pub struct MouthTarget(pub [f32; 5]);

fn debug_keys(keys: Res<ButtonInput<KeyCode>>, mut target: ResMut<MouthTarget>) {
    const KEYS: [KeyCode; 5] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5];
    if keys.just_pressed(KeyCode::Digit0) {
        target.0 = [0.0; 5];
    }
    for (i, key) in KEYS.into_iter().enumerate() {
        if keys.just_pressed(key) {
            target.0 = [0.0; 5];
            target.0[i] = 1.0;
        }
    }
}

/// Moves the current weights towards the target with a first-order lag.
fn mouth(time: Res<Time>, config: Res<Config>, target: Res<MouthTarget>, mut current: Local<[f32; 5]>, mut avatars: Query<&mut Expressions, With<Humanoid>>) {
    let k = 1.0 - (-time.delta_secs() / config.mouth.time_constant.max(1e-3)).exp();
    for (w, t) in current.iter_mut().zip(target.0) {
        *w += (t - *w) * k;
    }
    for mut expressions in &mut avatars {
        for (viseme, &w) in ExpressionPreset::VISEMES.iter().zip(current.iter()) {
            expressions.set(*viseme, w);
        }
    }
}
