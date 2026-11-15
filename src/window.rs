//! The desktop window: optionally borderless and transparent, kept above other windows and letting clicks through to the desktop. F2 or the tray toggles click-through; while it is off, dragging with the left mouse button moves the window (and with the right one turns the figure, see `view`). The tray can also close it.

use bevy::prelude::*;
use bevy::window::{CompositeAlphaMode, CursorOptions, PrimaryWindow, WindowLevel};

use riggy::control::Command;

use crate::commands::Received;
use crate::config::WindowConfig;

/// Wayland app ID and X11 class, which KWin window rules match on.
pub const APP_ID: &str = "riggy";

/// The primary window as `[window]` describes it.
pub fn primary_window(config: &WindowConfig) -> Window {
    Window {
        title: "Riggy".into(),
        name: Some(APP_ID.into()),
        resolution: (config.width, config.height).into(),
        decorations: !config.transparent,
        transparent: config.transparent,
        // The renderer writes premultiplied colour, and NVIDIA's Wayland Vulkan surface offers no post-multiplied mode.
        composite_alpha_mode: if config.transparent { CompositeAlphaMode::PreMultiplied } else { CompositeAlphaMode::Auto },
        // Wayland ignores the level; there the KWin script in kwin/riggy keeps riggy above.
        window_level: if config.always_on_top { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal },
        ..default()
    }
}

/// Cursor options of the primary window: without hit testing, clicks go to the windows underneath.
pub fn primary_cursor_options(config: &WindowConfig) -> CursorOptions {
    CursorOptions {
        hit_test: !config.click_through,
        ..default()
    }
}

/// The background behind the figure: fully transparent in a transparent window.
pub fn clear_color(config: &WindowConfig) -> ClearColor {
    let [r, g, b, a] = config.background;
    ClearColor(if config.transparent { Color::NONE } else { Color::srgba(r, g, b, a) })
}

pub struct DesktopWindowPlugin;

impl Plugin for DesktopWindowPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (toggle_click_through, obey, drag_move));
    }
}

fn toggle_click_through(keys: Res<ButtonInput<KeyCode>>, mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    if !keys.just_pressed(KeyCode::F2) {
        return;
    }
    for mut cursor in &mut cursors {
        cursor.hit_test = !cursor.hit_test;
        info!("click-through {}", if cursor.hit_test { "off" } else { "on" });
    }
}

/// Click-through and closing as the tray asks.
fn obey(mut commands: MessageReader<Received>, mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>, mut exit: MessageWriter<AppExit>) {
    for Received(command) in commands.read() {
        match command {
            Command::ClickThrough(on) => {
                for mut cursor in &mut cursors {
                    cursor.hit_test = !*on;
                }
            }
            Command::Quit => {
                exit.write(AppExit::Success);
            }
            Command::Lipsync(_) | Command::Zoom(_) | Command::Turn(_) | Command::Clips(_) | Command::Play(_) => {}
        }
    }
}

fn drag_move(buttons: Res<ButtonInput<MouseButton>>, mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    if buttons.just_pressed(MouseButton::Left) {
        for mut window in &mut windows {
            window.start_drag_move();
        }
    }
}
