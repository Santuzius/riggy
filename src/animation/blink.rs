//! Blinking: sets the expression `blink` at random intervals; the backend turns it into eyelid motion.

use bevy::prelude::*;
use rand::Rng;

use crate::avatar::{AvatarSet, ExpressionPreset, Expressions, Humanoid};
use crate::config::Config;

pub struct BlinkPlugin;

impl Plugin for BlinkPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, blink.in_set(AvatarSet::Express));
    }
}

/// Time of the next blink start, in seconds of elapsed time.
#[derive(Default)]
struct NextBlink(Option<f32>);

fn blink(time: Res<Time>, config: Res<Config>, mut next: Local<NextBlink>, mut avatars: Query<&mut Expressions, With<Humanoid>>) {
    let c = &config.blink;
    let now = time.elapsed_secs();
    let mut rng = rand::rng();
    let start = *next.0.get_or_insert_with(|| now + rng.random_range(0.5..c.max_interval.max(0.6)));

    // Lids close in the first third and open more slowly in the rest.
    let phase = (now - start) / c.duration.max(0.01);
    let weight = if phase < 0.0 || phase >= 1.0 {
        0.0
    } else if phase < 1.0 / 3.0 {
        smooth(phase * 3.0)
    } else {
        1.0 - smooth((phase - 1.0 / 3.0) * 1.5)
    };
    if phase >= 1.0 {
        let (lo, hi) = (c.min_interval.max(0.1), c.max_interval.max(c.min_interval.max(0.1) + 0.01));
        next.0 = Some(now + rng.random_range(lo..hi));
    }
    for mut expressions in &mut avatars {
        expressions.set(ExpressionPreset::Blink, weight);
    }
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}
