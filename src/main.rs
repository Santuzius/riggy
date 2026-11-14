//! riggy: a desktop mascot that idles and lip-syncs to text-to-speech output.

mod audio;
mod avatar;
mod blink;
mod config;
mod idle;
mod lipsync;
mod mouth;
mod speech;
mod view;

use bevy::prelude::*;

fn main() {
    let config = config::load(&config::config_path());
    let w = &config.window;
    let [r, g, b, a] = w.background;

    let mut app = App::new();
    avatar::glb::register_model_source(&mut app, &config.avatar.path);
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "riggy".into(),
            resolution: (w.width, w.height).into(),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgba(r, g, b, a)))
    .insert_resource(config)
    .add_plugins((config::ConfigPlugin, avatar::AvatarPlugin, idle::IdlePlugin, blink::BlinkPlugin, mouth::MouthPlugin, speech::SpeechPlugin, view::ViewPlugin))
    .run();
}
