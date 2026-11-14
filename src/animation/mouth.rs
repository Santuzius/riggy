//! Mouth: smooths target viseme weights (`aa ih ou ee oh`) and writes them into `Expressions`.
//!
//! Lip sync sets `MouthTarget`; for testing, keys 1–5 hold one viseme fully open and 0 returns to rest.

use bevy::prelude::*;

use crate::avatar::{AvatarSet, ExpressionName, ExpressionPreset, Expressions, Humanoid};
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
pub struct MouthTarget {
    pub weights: [f32; 5],
    /// The weights come from Rhubarb cues: discrete shapes that are followed with `[lipsync] cue_time` both ways, so short closures (m, b, p) show.
    pub cued: bool,
}

fn debug_keys(keys: Res<ButtonInput<KeyCode>>, mut target: ResMut<MouthTarget>) {
    const KEYS: [KeyCode; 5] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5];
    if keys.just_pressed(KeyCode::Digit0) {
        *target = MouthTarget::default();
    }
    for (i, key) in KEYS.into_iter().enumerate() {
        if keys.just_pressed(key) {
            *target = MouthTarget::default();
            target.weights[i] = 1.0;
        }
    }
}

/// Cheek raise per unit of the "ee" and "ih" weights.
const CHEEKS_PER_VISEME: [f32; 5] = [0.0, 0.4, 0.0, 1.0, 0.0];

/// Moves the current weights towards the target with a first-order lag: `time_constant` while a weight rises, the slower `release_time` while it falls, as a jaw opens quickly and closes more slowly; `[lipsync] cue_time` both ways for Rhubarb cues.
///
/// The custom expression `cheeks` follows the spread shapes with the much slower `cheek_time`: cheeks are pushed up by the held lip shape, not by each syllable. Rhubarb's shapes do not pump with loudness, so with cues the cheeks follow faster (`[lipsync] cue_cheek_time`).
fn mouth(time: Res<Time>, config: Res<Config>, target: Res<MouthTarget>, mut current: Local<[f32; 5]>, mut cheeks: Local<f32>, mut avatars: Query<&mut Expressions, With<Humanoid>>) {
    let gain = |tau: f32| 1.0 - (-time.delta_secs() / tau.max(1e-3)).exp();
    let (rise, fall) = if target.cued {
        let k = gain(config.lipsync.cue_time);
        (k, k)
    } else {
        (gain(config.mouth.time_constant), gain(config.mouth.release_time))
    };
    for (w, t) in current.iter_mut().zip(target.weights) {
        *w += (t - *w) * if t > *w { rise } else { fall };
    }
    let cheek_target: f32 = current.iter().zip(CHEEKS_PER_VISEME).map(|(w, c)| w * c).sum();
    let cheek_time = if target.cued { config.lipsync.cue_cheek_time } else { config.mouth.cheek_time };
    *cheeks += (cheek_target.min(1.0) - *cheeks) * gain(cheek_time);
    for mut expressions in &mut avatars {
        for (viseme, &w) in ExpressionPreset::VISEMES.iter().zip(current.iter()) {
            expressions.set(*viseme, w);
        }
        expressions.set(ExpressionName::Custom("cheeks".into()), *cheeks);
    }
}
