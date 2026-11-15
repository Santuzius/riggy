//! The avatar's sockets: speech comes in on one, commands from the tray on the other.
//!
//! The control socket takes one JSON command per line, e.g. `{"click_through":true}`, `{"lipsync":"text"}`, `{"zoom":0.5}`, `{"turn":-20.0}`, `{"clips":false}`, `{"unapproved":true}`, `{"clip_legs":true}`, `{"play":"pack/clip"}` or `"quit"`. It sits next to the speech socket: `riggy.sock` has `riggy-control.sock`.

use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LipsyncMode {
    /// From loudness and spectrum while the sound plays; speech starts at once.
    #[default]
    Sound,
    /// From Rhubarb Lip Sync's mouth shapes; speech starts about 1–2 s later.
    Rhubarb,
    /// From the spoken text, timed to the audio, with Rhubarb's mouth shapes; speech starts about 0.3 s later.
    Text,
}

impl LipsyncMode {
    /// The modes this build offers, in the order F3 cycles through them.
    pub fn available() -> Vec<Self> {
        let mut modes = vec![Self::Sound];
        if cfg!(feature = "rhubarb") {
            modes.push(Self::Rhubarb);
        }
        if cfg!(feature = "espeak") {
            modes.push(Self::Text);
        }
        modes
    }
}

/// A command for the running avatar.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    /// Let clicks through to the windows underneath, or take them (then dragging moves the window).
    ClickThrough(bool),
    /// Lip-sync mode from the next utterance on.
    Lipsync(LipsyncMode),
    /// Framing between the whole figure (0) and the face (1).
    Zoom(f32),
    /// Turns the figure about its vertical axis, in degrees; positive turns it to its left.
    Turn(f32),
    /// Whether the behaviour files' clips play (off: only riggy's own motion).
    Clips(bool),
    /// Whether the behaviour also plays clips not approved on the avatar.
    Unapproved(bool),
    /// Whether clips move the legs instead of riggy's stance.
    ClipLegs(bool),
    /// Plays an installed clip once, by `pack/clip`, whether clips are on or not.
    Play(String),
    /// Close the avatar.
    Quit,
}

/// `$XDG_RUNTIME_DIR`, or the temp directory without one.
pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// The speech socket when the config names none.
pub fn default_speech_socket() -> PathBuf {
    runtime_dir().join("riggy.sock")
}

/// The control socket that belongs to a speech socket.
pub fn control_socket(speech_socket: &Path) -> PathBuf {
    let stem = speech_socket.file_stem().unwrap_or_default().to_string_lossy();
    speech_socket.with_file_name(format!("{stem}-control.sock"))
}

/// Sends commands to the avatar listening on `socket`; fails if none listens.
pub fn send(socket: &Path, commands: &[Command]) -> io::Result<()> {
    let mut stream = UnixStream::connect(socket)?;
    for command in commands {
        writeln!(stream, "{}", serde_json::to_string(command).map_err(io::Error::other)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_read_as_documented() {
        assert_eq!(serde_json::to_string(&Command::ClickThrough(true)).unwrap(), r#"{"click_through":true}"#);
        assert_eq!(serde_json::to_string(&Command::Lipsync(LipsyncMode::Text)).unwrap(), r#"{"lipsync":"text"}"#);
        assert_eq!(serde_json::to_string(&Command::Zoom(0.5)).unwrap(), r#"{"zoom":0.5}"#);
        assert_eq!(serde_json::to_string(&Command::Turn(-20.0)).unwrap(), r#"{"turn":-20.0}"#);
        assert_eq!(serde_json::to_string(&Command::Play("pack/wave".into())).unwrap(), r#"{"play":"pack/wave"}"#);
        assert_eq!(serde_json::to_string(&Command::Quit).unwrap(), r#""quit""#);
    }

    #[test]
    fn control_socket_sits_next_to_the_speech_socket() {
        assert_eq!(control_socket(Path::new("/run/user/1000/riggy.sock")), Path::new("/run/user/1000/riggy-control.sock"));
    }
}
