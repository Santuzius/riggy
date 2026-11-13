//! riggy: a desktop mascot that idles and lip-syncs to text-to-speech output.

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "riggy".into(),
                resolution: (400, 700).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.08, 0.08, 0.10)))
        .run();
}
