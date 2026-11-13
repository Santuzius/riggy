//! Settings from `riggy.toml`, reloaded when the file changes.
//!
//! The file is looked up in `$RIGGY_CONFIG`, then `./riggy.toml`. `$RIGGY_MODEL` overrides `[avatar] path`.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bevy::prelude::*;
use serde::Deserialize;

#[derive(Resource, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Config {
    pub avatar: AvatarConfig,
    pub window: WindowConfig,
    pub idle: IdleConfig,
    pub blink: BlinkConfig,
    pub mouth: MouthConfig,
    pub speech: SpeechConfig,
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AvatarKind {
    #[default]
    Glb,
    Vrm,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct AvatarConfig {
    pub kind: AvatarKind,
    /// Model file (GLB or VRM), absolute or relative to the working directory.
    pub path: PathBuf,
    /// Mapping file for the `glb` backend (humanoid bones and bone-driven expressions).
    pub mapping: PathBuf,
}

impl Default for AvatarConfig {
    fn default() -> Self {
        Self {
            kind: AvatarKind::Glb,
            path: "assets/models/cyborg_girl.glb".into(),
            mapping: "assets/avatars/cyborg_girl.toml".into(),
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
    /// Borderless window with a transparent background.
    pub transparent: bool,
    pub always_on_top: bool,
    /// Let mouse clicks pass through to the windows underneath (toggle with F2).
    pub click_through: bool,
    /// Background colour as sRGB with alpha, used when `transparent` is off.
    pub background: [f32; 4],
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 400,
            height: 700,
            transparent: false,
            always_on_top: false,
            click_through: false,
            background: [0.08, 0.08, 0.10, 1.0],
        }
    }
}

/// Procedural idle motion; angles in degrees, rates in Hz.
#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct IdleConfig {
    pub breath_rate: f32,
    pub breath_angle: f32,
    pub sway_rate: f32,
    /// Sideways hip shift as a fraction of the figure's height.
    pub sway_shift: f32,
    pub sway_angle: f32,
    pub head_rate: f32,
    pub head_angle: f32,
    /// Extra downward rotation of the upper arms from the rest pose (positive lowers the arms).
    pub arm_down_angle: f32,
    /// Bend of the elbows.
    pub elbow_angle: f32,
}

impl Default for IdleConfig {
    fn default() -> Self {
        Self {
            breath_rate: 0.25,
            breath_angle: 1.0,
            sway_rate: 0.07,
            sway_shift: 0.004,
            sway_angle: 1.2,
            head_rate: 0.15,
            head_angle: 2.5,
            arm_down_angle: 0.0,
            elbow_angle: 0.0,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct BlinkConfig {
    pub min_interval: f32,
    pub max_interval: f32,
    pub duration: f32,
}

impl Default for BlinkConfig {
    fn default() -> Self {
        Self {
            min_interval: 2.0,
            max_interval: 6.0,
            duration: 0.15,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct MouthConfig {
    /// Smoothing time constant of the viseme weights in seconds.
    pub time_constant: f32,
    /// RMS level (full scale = 1) below which the mouth closes.
    pub silence_level: f32,
    /// RMS level that opens the mouth fully.
    pub loud_level: f32,
    /// Delay added to the audio clock in seconds, to match the sound card's output latency.
    pub audio_latency: f32,
}

impl Default for MouthConfig {
    fn default() -> Self {
        Self {
            time_constant: 0.04,
            silence_level: 0.01,
            loud_level: 0.15,
            audio_latency: 0.05,
        }
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct SpeechConfig {
    /// Unix socket path; empty means `$XDG_RUNTIME_DIR/riggy.sock`.
    pub socket: PathBuf,
}

impl SpeechConfig {
    pub fn socket_path(&self) -> PathBuf {
        if !self.socket.as_os_str().is_empty() {
            return self.socket.clone();
        }
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        runtime.join("riggy.sock")
    }
}

pub fn config_path() -> PathBuf {
    std::env::var_os("RIGGY_CONFIG").map(PathBuf::from).unwrap_or_else(|| "riggy.toml".into())
}

/// Reads the config; a missing file gives the defaults, a broken one is reported and gives the defaults too.
pub fn load(path: &Path) -> Config {
    let mut config = match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|err| {
            eprintln!("riggy: {}: {err}", path.display());
            Config::default()
        }),
        Err(_) => Config::default(),
    };
    if let Some(model) = std::env::var_os("RIGGY_MODEL") {
        config.avatar.path = model.into();
    }
    config
}

pub fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Polls `riggy.toml` once a second and swaps in the new settings (window and avatar changes need a restart).
pub struct ConfigPlugin;

#[derive(Resource)]
struct ConfigWatch {
    path: PathBuf,
    modified: Option<SystemTime>,
    timer: Timer,
}

impl Plugin for ConfigPlugin {
    fn build(&self, app: &mut App) {
        let path = config_path();
        app.insert_resource(ConfigWatch {
            modified: modified(&path),
            path,
            timer: Timer::new(Duration::from_secs(1), TimerMode::Repeating),
        })
        .add_systems(Update, reload_config);
    }
}

fn reload_config(time: Res<Time>, mut watch: ResMut<ConfigWatch>, mut config: ResMut<Config>) {
    if !watch.timer.tick(time.delta()).just_finished() {
        return;
    }
    let modified = modified(&watch.path);
    if modified == watch.modified {
        return;
    }
    watch.modified = modified;
    let fresh = load(&watch.path);
    info!("reloaded {}", watch.path.display());
    // Keep the parts that only take effect at startup.
    *config = Config {
        avatar: config.avatar.clone(),
        window: config.window.clone(),
        speech: config.speech.clone(),
        ..fresh
    };
}
