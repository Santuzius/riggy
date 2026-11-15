//! Riggy: a desktop mascot that idles and lip-syncs to text-to-speech output. `riggy` shows the avatar; its subcommands (see `cli`) install models and speak texts.

mod animation;
mod audio;
mod avatar;
mod cli;
mod commands;
mod config;
mod lipsync;
mod speech;
mod view;
mod window;

use std::process::ExitCode;

use bevy::prelude::*;

use cli::Invocation;

fn main() -> ExitCode {
    match cli::invocation(std::env::args().skip(1)) {
        Ok(Invocation::Avatar) => {
            run_avatar();
            ExitCode::SUCCESS
        }
        Ok(Invocation::Install(files)) => cli::install(&files),
        Ok(Invocation::Say { language, text }) => cli::say(language, text),
        Ok(Invocation::Help) => cli::usage(),
        Err(message) => cli::usage_error(&message),
    }
}

fn run_avatar() {
    let config_path = config::config_path();
    let mut config = config::load(&config_path);
    if let Err(err) = riggy::files::ensure_user_files(&config.avatar.name) {
        eprintln!("riggy: cannot create the files in {}: {err}", riggy::files::config_dir().display());
    }
    // The window opens as high as the avatar wants it on the desktop; changing the height later leaves the picture cut until the next resize.
    if config.on_desktop {
        config.window.height = (config.window.height as f32 * avatar::glb::window_share(&config.avatar.mapping)).round() as u32;
    }

    let mut app = App::new();
    // A VRM model is loaded from its edited copy (`avatar::vrm`).
    let model_path = if config.avatar.kind == config::AvatarKind::Vrm {
        let _ = std::fs::create_dir_all(riggy::files::edited_models_dir());
        riggy::files::edited_models_dir().join("model")
    } else {
        config.avatar.path.clone()
    };
    avatar::register_model_source(&mut app, &model_path);
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
