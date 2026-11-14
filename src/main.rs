//! riggy: a desktop mascot that idles and lip-syncs to text-to-speech output.

mod animation;
mod audio;
mod avatar;
mod commands;
mod config;
mod lipsync;
mod speech;
mod view;
mod window;

use bevy::prelude::*;

fn main() {
    let config_path = config::config_path();
    let config = config::load(&config_path);
    if let Err(err) = riggy::files::ensure_user_files(&config.avatar.name) {
        eprintln!("riggy: cannot create the files in {}: {err}", riggy::files::config_dir().display());
    }

    let mut app = App::new();
    avatar::glb::register_model_source(&mut app, &config.avatar.path);
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(window::primary_window(&config.window)),
        primary_cursor_options: Some(window::primary_cursor_options(&config.window)),
        ..default()
    }))
    .insert_resource(window::clear_color(&config.window))
    .insert_resource(config)
    .add_plugins((config::ConfigPlugin, commands::CommandsPlugin, avatar::AvatarPlugin, animation::AnimationPlugin, speech::SpeechPlugin, view::ViewPlugin, window::DesktopWindowPlugin))
    .run();
}
