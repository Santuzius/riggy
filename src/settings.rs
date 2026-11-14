//! What the tray keeps between runs, in `~/.config/riggy/` (see `riggy::files`): the choices in `settings.toml`, and the file `speech-off` while speech is switched off. The speech server's systemd unit checks that file, so speech stays off across logins.

use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::control::LipsyncMode;
pub use crate::files::config_dir;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Voice of the speech server.
    pub voice: String,
    /// Playback speed, 1 = as the server speaks.
    pub tempo: f32,
    /// Whether the avatar is shown.
    pub avatar: bool,
    /// Whether clicks go through the avatar to the windows underneath.
    pub click_through: bool,
    pub lipsync: LipsyncMode,
}

impl Default for Settings {
    fn default() -> Self {
        Self { voice: "serena".into(), tempo: 1.25, avatar: true, click_through: true, lipsync: LipsyncMode::Sound }
    }
}

impl Settings {
    /// The stored settings; defaults for a missing or broken file.
    pub fn load() -> Self {
        std::fs::read_to_string(settings_file()).ok().and_then(|text| toml::from_str(&text).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        std::fs::create_dir_all(config_dir())?;
        std::fs::write(settings_file(), toml::to_string(self).map_err(io::Error::other)?)
    }
}

fn settings_file() -> PathBuf {
    config_dir().join("settings.toml")
}

/// Exists while speech is switched off.
pub fn speech_off_flag() -> PathBuf {
    config_dir().join("speech-off")
}

pub fn speech_off() -> bool {
    speech_off_flag().exists()
}
