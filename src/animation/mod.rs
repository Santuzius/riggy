//! Per-frame motion of the avatar: standing on the floor, procedural idle, blinking, and the mouth, which follows the viseme targets that lip sync (`crate::lipsync`) sets.

mod blink;
mod idle;
pub mod mouth;
pub mod stance;

use bevy::prelude::*;

pub struct AnimationPlugin;

impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((stance::StancePlugin, idle::IdlePlugin, blink::BlinkPlugin, mouth::MouthPlugin));
    }
}
