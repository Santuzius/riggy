//! KDE Plasma: switches on the KWin script that places the avatar on the panel (`kwin/riggy`, installed with the package), once, so the avatar stands on the desktop without setup.

use std::path::PathBuf;
use std::process::{Command, Stdio};

const SCRIPT_KEY: &str = "riggyEnabled";
/// The script of this build, so the tray can install it over an older copy.
const SCRIPT_FILES: [(&str, &str); 2] = [("metadata.json", include_str!("../../../kwin/riggy/metadata.json")), ("contents/code/main.js", include_str!("../../../kwin/riggy/contents/code/main.js"))];

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
    reconfigure();
}

/// Writes this build's script to `~/.local/share/kwin/scripts/riggy/`, where it takes precedence over the package's, and makes KWin load it anew.
pub fn install_script() {
    let dir = riggy::files::xdg_data_home().join("kwin/scripts/riggy");
    for (file, text) in SCRIPT_FILES {
        let path: PathBuf = dir.join(file);
        if let Err(err) = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&path, text)) {
            eprintln!("riggy-tray: cannot write {}: {err}", path.display());
            return;
        }
    }
    let _ = Command::new("busctl").args(["--user", "call", "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting", "unloadScript", "s", "riggy"]).stdout(Stdio::null()).status();
    enable_script();
    reconfigure();
}

fn reconfigure() {
    let _ = Command::new("busctl").args(["--user", "call", "org.kde.KWin", "/KWin", "org.kde.KWin", "reconfigure"]).stdout(Stdio::null()).status();
}

fn enabled() -> bool {
    let output = Command::new("kreadconfig6").args(["--file", "kwinrc", "--group", "Plugins", "--key", SCRIPT_KEY]).output();
    output.is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "true")
}
