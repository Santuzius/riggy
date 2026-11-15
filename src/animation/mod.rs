//! Per-frame motion of the avatar. A humanoid stands on the floor, plays animation clips as the behaviour files say, idles, blinks and moves its mouth to the viseme targets that lip sync (`crate::lipsync`) sets; a machine floats, spins and opens and glows while speaking.

mod blink;
mod clips;
mod idle;
mod machine;
mod pose_log;
pub mod mouth;
pub mod stance;

use bevy::prelude::*;

pub struct AnimationPlugin;

impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((stance::StancePlugin, clips::ClipsPlugin, idle::IdlePlugin, blink::BlinkPlugin, mouth::MouthPlugin, machine::MachinePlugin, pose_log::PoseLogPlugin));
    }
}
