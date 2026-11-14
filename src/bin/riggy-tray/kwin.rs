//! KDE Plasma: switches on the KWin script that places the avatar on the panel (`kwin/riggy`, installed with the package), once, so the avatar stands on the desktop without setup.

use std::process::{Command, Stdio};

const SCRIPT_KEY: &str = "riggyEnabled";

/// Enables the script in `kwinrc` if this is a Plasma session and it is not enabled yet, and tells KWin to load it.
pub fn enable_script() {
    let plasma = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktops| desktops.split(':').any(|d| d == "KDE"));
    if !plasma || enabled() {
        return;
    }
    let written = Command::new("kwriteconfig6").args(["--file", "kwinrc", "--group", "Plugins", "--key", SCRIPT_KEY, "--type", "bool", "true"]).status();
    if !written.is_ok_and(|status| status.success()) {
        eprintln!("riggy-tray: cannot enable the KWin script; see doc/usage.md");
        return;
    }
    let _ = Command::new("busctl").args(["--user", "call", "org.kde.KWin", "/KWin", "org.kde.KWin", "reconfigure"]).stdout(Stdio::null()).status();
}

fn enabled() -> bool {
    let output = Command::new("kreadconfig6").args(["--file", "kwinrc", "--group", "Plugins", "--key", SCRIPT_KEY]).output();
    output.is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "true")
}
