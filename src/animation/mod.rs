//! Per-frame motion of the avatar: procedural idle, blinking, and the mouth, which follows the viseme targets that lip sync (`crate::lipsync`) sets.

mod blink;
mod idle;
pub mod mouth;

use bevy::prelude::*;

pub struct AnimationPlugin;

impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((idle::IdlePlugin, blink::BlinkPlugin, mouth::MouthPlugin));
    }
}
