//! Settings from `riggy.toml`, reloaded when the file changes.
//!
//! The defaults come with riggy (`riggy.toml` in the repository); the user's `~/.config/riggy/riggy.toml` (or `$RIGGY_CONFIG`) holds only what differs and is laid over them table by table. See `riggy::files` for all paths. `$RIGGY_MODEL` overrides the model file. `RIGGY_DESKTOP=1`, with which the tray starts riggy, makes the window transparent and keeps it above the others, and lays the tray's choices (`riggy::settings`: model, size, zoom, turn) over the settings.

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
    pub lipsync: LipsyncConfig,
    pub debug: DebugConfig,
    /// Started by the tray (`RIGGY_DESKTOP=1`): a zoom or turn made with the mouse goes into the tray's settings.
    #[serde(skip)]
    pub on_desktop: bool,
}

pub use riggy::control::LipsyncMode;

/// Which lip sync drives the mouth; applies to the next utterance (F3 toggles the mode while running).
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LipsyncConfig {
    pub mode: LipsyncMode,
    /// The `rhubarb` program (from the Nix shell, or a path).
    pub rhubarb: PathBuf,
    /// Rhubarb's recognizer: `phonetic` (any language) or `pocketSphinx` (English only, slower than real time).
    pub recognizer: String,
    /// Time constant in seconds with which the mouth follows Rhubarb's shapes.
    pub cue_time: f32,
    /// Time constant in seconds with which the cheeks follow Rhubarb's spread shapes.
    pub cue_cheek_time: f32,
    /// Mouth shape weights per Rhubarb shape, replacing the built-in ones, e.g. `B = { ee = 0.9 }`.
    pub cues: std::collections::HashMap<crate::lipsync::Shape, std::collections::HashMap<String, f32>>,
}

impl Default for LipsyncConfig {
    fn default() -> Self {
        Self {
            mode: LipsyncMode::Sound,
            rhubarb: "rhubarb".into(),
            recognizer: "phonetic".into(),
            cue_time: 0.05,
            cue_cheek_time: 0.12,
            cues: Default::default(),
        }
    }
}

/// Tuning aids.
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct DebugConfig {
    /// Expression weights forced on top of blink and mouth, e.g. `{ blink = 1.0 }`.
    pub expressions: std::collections::HashMap<String, f32>,
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
    /// The model's name: its file is `~/.local/share/riggy/models/NAME.glb`, its mapping file `~/.local/share/riggy/avatars/NAME.toml`.
    pub name: String,
    /// Another model file instead (`~/` and paths relative to `~/.config/riggy/` work); filled in from `name` when empty.
    pub path: PathBuf,
    /// Another mapping file for the `glb` backend (humanoid bones and bone-driven expressions); filled in from `name` when empty.
    pub mapping: PathBuf,
}

impl Default for AvatarConfig {
    fn default() -> Self {
        Self { kind: AvatarKind::Glb, name: "cyborg_girl".into(), path: PathBuf::new(), mapping: PathBuf::new() }
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
    /// Zoom at startup: 0 shows the whole figure, 1 the face (the mouse wheel changes it).
    pub zoom: f32,
    /// Turn of the figure in degrees at startup; positive turns it to its left.
    pub turn: f32,
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
            zoom: 0.0,
            turn: 0.0,
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
    /// Angle of the upper arms out from vertical, whatever the rest pose (T or A pose) is.
    pub arm_angle: f32,
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
            arm_angle: 12.0,
            elbow_angle: 8.0,
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
    /// Smoothing time constant of the viseme weights in seconds while they rise.
    pub time_constant: f32,
    /// Smoothing time constant in seconds while they fall (the mouth closing).
    pub release_time: f32,
    /// Time constant in seconds with which the mouth shape (not the opening) follows a fully open sound; quieter sounds move it more slowly.
    pub shape_time: f32,
    /// Time constant in seconds with which the cheeks follow the spread mouth shapes.
    pub cheek_time: f32,
    /// Opening (0–1) kept during quiet stretches within speech.
    pub pause_opening: f32,
    /// Quiet stretches longer than this (seconds) close the mouth fully.
    pub pause_hold: f32,
    /// RMS level (full scale = 1) below which the mouth closes.
    pub silence_level: f32,
    /// RMS level that opens the mouth fully.
    pub loud_level: f32,
    /// Extra delay in seconds between the sound and the mouth, on top of the output latency the sound device reports.
    pub audio_latency: f32,
}

impl Default for MouthConfig {
    fn default() -> Self {
        Self {
            time_constant: 0.04,
            release_time: 0.12,
            shape_time: 0.07,
            cheek_time: 0.3,
            pause_opening: 0.15,
            pause_hold: 0.4,
            silence_level: 0.01,
            loud_level: 0.12,
            audio_latency: 0.0,
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
        riggy::control::default_speech_socket()
    }
}

pub fn config_path() -> PathBuf {
    riggy::files::user_config()
}

/// The defaults with the user's settings in `path` laid over them; a broken user file is reported and ignored.
pub fn load(path: &Path) -> Config {
    let mut merged: toml::Table = toml::from_str(&riggy::files::default_config()).unwrap_or_else(|err| {
        eprintln!("riggy: default settings: {err}");
        toml::Table::new()
    });
    match std::fs::read_to_string(path).map(|text| toml::from_str::<toml::Table>(&text)) {
        Ok(Ok(user)) => overlay(&mut merged, user),
        Ok(Err(err)) => eprintln!("riggy: {}: {err}", path.display()),
        Err(_) => {}
    }
    let mut config: Config = toml::Value::Table(merged).try_into().unwrap_or_else(|err| {
        eprintln!("riggy: {}: {err}", path.display());
        Config::default()
    });
    // The tray starts riggy this way: on the desktop, as the tray's choices say.
    if std::env::var_os("RIGGY_DESKTOP").is_some_and(|v| v == "1") {
        apply_tray_choices(&mut config, &riggy::settings::Settings::load());
    }
    if let Some(model) = std::env::var_os("RIGGY_MODEL") {
        let model = PathBuf::from(model);
        config.avatar.path = std::path::absolute(&model).unwrap_or(model);
    }
    let avatar = &mut config.avatar;
    avatar.path = if avatar.path.as_os_str().is_empty() { riggy::files::model(&avatar.name) } else { riggy::files::resolve(&avatar.path) };
    avatar.mapping = if avatar.mapping.as_os_str().is_empty() { riggy::files::mapping(&avatar.name) } else { riggy::files::resolve(&avatar.mapping) };
    config
}

/// A transparent window above the others, with the model and size chosen in the tray and the zoom and turn last made with the mouse.
fn apply_tray_choices(config: &mut Config, choices: &riggy::settings::Settings) {
    config.on_desktop = true;
    let window = &mut config.window;
    window.transparent = true;
    window.always_on_top = true;
    let size = choices.size.clamp(0.25, 4.0);
    window.width = (window.width as f32 * size).round() as u32;
    window.height = (window.height as f32 * size).round() as u32;
    window.zoom = choices.zoom;
    window.turn = choices.turn;
    if !choices.model.is_empty() {
        config.avatar = AvatarConfig { name: choices.model.clone(), ..AvatarConfig::default() };
    }
}

/// Lays `over` onto `base`: tables merge key by key, everything else replaces.
fn overlay(base: &mut toml::Table, over: toml::Table) {
    for (key, value) in over {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(inner)), toml::Value::Table(value)) => overlay(inner, value),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

pub fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Polls the user's settings and the defaults once a second and swaps in the new settings (window and avatar changes need a restart).
pub struct ConfigPlugin;

#[derive(Resource)]
struct ConfigWatch {
    path: PathBuf,
    /// Modification times of the user's file and of the defaults file.
    modified: [Option<SystemTime>; 2],
    timer: Timer,
}

impl ConfigWatch {
    fn modified(&self) -> [Option<SystemTime>; 2] {
        [modified(&self.path), riggy::files::default_config_file().and_then(|file| modified(&file))]
    }
}

impl Plugin for ConfigPlugin {
    fn build(&self, app: &mut App) {
        let mut watch = ConfigWatch { path: config_path(), modified: [None; 2], timer: Timer::new(Duration::from_secs(1), TimerMode::Repeating) };
        watch.modified = watch.modified();
        app.insert_resource(watch)
        .add_systems(Update, reload_config);
    }
}

fn reload_config(time: Res<Time>, mut watch: ResMut<ConfigWatch>, mut config: ResMut<Config>) {
    if !watch.timer.tick(time.delta()).just_finished() {
        return;
    }
    let modified = watch.modified();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_settings_override_only_what_they_set() {
        let mut base: toml::Table = toml::from_str("[idle]\nbreath_rate = 0.25\nhead_angle = 2.5\n[lipsync]\nmode = \"sound\"").unwrap();
        overlay(&mut base, toml::from_str("[idle]\nhead_angle = 15.0").unwrap());
        assert_eq!(base["idle"]["breath_rate"].as_float(), Some(0.25));
        assert_eq!(base["idle"]["head_angle"].as_float(), Some(15.0));
        assert_eq!(base["lipsync"]["mode"].as_str(), Some("sound"));
    }

    #[test]
    fn the_tray_choices_scale_the_window_and_replace_the_model() {
        let mut config = Config::default();
        config.avatar.path = "~/other.glb".into();
        let choices = riggy::settings::Settings { size: 1.5, model: "robot".into(), ..Default::default() };
        apply_tray_choices(&mut config, &choices);
        assert_eq!((config.window.width, config.window.height), (600, 1050));
        assert_eq!(config.avatar.name, "robot");
        assert!(config.avatar.path.as_os_str().is_empty());
        assert!(config.window.transparent);
    }

    #[test]
    fn the_defaults_parse() {
        let config: Config = toml::from_str(include_str!("../riggy.toml")).unwrap();
        assert_eq!(config.avatar.name, "cyborg_girl");
    }
}
