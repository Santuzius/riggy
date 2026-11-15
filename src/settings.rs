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
    /// The avatar's model name (`~/.local/share/riggy/models/NAME.glb`); empty takes `[avatar] name` from `riggy.toml`.
    pub model: String,
    /// Whether clicks go through the avatar to the windows underneath.
    pub click_through: bool,
    pub lipsync: LipsyncMode,
    /// Window size relative to `[window] width` and `height`.
    pub size: f32,
    /// Framing between the whole figure (0) and the face (1); set with the mouse wheel.
    pub zoom: f32,
    /// Turn of the figure in degrees, positive to its left; set by dragging the figure with the right mouse button.
    pub turn: f32,
    /// Start the tray with the desktop session.
    pub autostart: bool,
    /// Start the tray, and with it the avatar, when an AI agent starts `riggy-mcp`.
    pub start_with_mcp: bool,
    /// Whether `riggy-mcp` tells the agent when and how to speak; off for users who write their own instructions.
    pub mcp_instructions: bool,
    /// Whether the avatar plays the clips of the behaviour files.
    pub clips: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            voice: "serena".into(),
            tempo: 1.25,
            avatar: true,
            model: String::new(),
            click_through: true,
            lipsync: LipsyncMode::Sound,
            size: 1.0,
            zoom: 0.0,
            turn: 0.0,
            autostart: false,
            start_with_mcp: true,
            mcp_instructions: true,
            clips: true,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_settings_file_keeps_its_choices_and_gets_the_new_defaults() {
        let settings: Settings = toml::from_str("voice = \"ryan\"\ntempo = 1.5").unwrap();
        assert_eq!(settings.voice, "ryan");
        assert_eq!(settings.size, 1.0);
        assert!(!settings.autostart);
        assert!(settings.mcp_instructions);
    }
}
