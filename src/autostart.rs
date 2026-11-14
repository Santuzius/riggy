//! Starting the tray with the desktop session: an entry in `~/.config/autostart/`, which desktop sessions such as KDE Plasma start at login.

use std::io;
use std::path::PathBuf;

/// The start menu entry riggy installs; the autostart entry is a copy of it.
pub const DESKTOP_ENTRY: &str = include_str!("../assets/riggy.desktop");

fn entry_file() -> PathBuf {
    crate::files::autostart_dir().join("riggy.desktop")
}

/// Adds or removes the autostart entry.
pub fn set(on: bool) -> io::Result<()> {
    let file = entry_file();
    if on {
        std::fs::create_dir_all(crate::files::autostart_dir())?;
        std::fs::write(file, DESKTOP_ENTRY)
    } else {
        match std::fs::remove_file(file) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    }
}
