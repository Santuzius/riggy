//! `riggy-tray`: the tray icon that controls speech and the avatar, started from the start menu entry "Riggy", at login, or by `riggy-mcp`.
//!
//! It runs the speech server as its child where no systemd unit does (`riggy::speech_server::Runner`). Its menu switches speech on and off (the flag file `riggy-mcp` checks, and the speech server, whose GPU memory it frees), picks voice and tempo, shows or closes the avatar, picks the avatar's model, lip-sync mode and how it stands on the desktop, chooses when the tray starts by itself, opens the files for advanced settings and puts all of Riggy's settings back to the defaults (keeping the old files as `*.old`). The choices persist in `riggy::settings`. A left click on the icon shows or closes the avatar. Quitting the tray closes the avatar too. Only one tray runs at a time (`riggy::tray_process`).

mod avatar;
mod icon;
mod kwin;
mod menu;
mod theme;
mod tray;

use std::time::Duration;

use ksni::blocking::TrayMethods;
use riggy::{autostart, speech_server, tray_process};

use tray::RiggyTray;

/// How often the tray looks at the speech server and the avatar.
const POLL: Duration = Duration::from_secs(2);

fn main() {
    let _lock = match tray_process::lock() {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            eprintln!("riggy-tray: Riggy is already in the tray");
            return;
        }
        Err(err) => {
            eprintln!("riggy-tray: cannot take the tray lock: {err}");
            std::process::exit(1);
        }
    };
    let tray = RiggyTray::new();
    // The autostart entry follows the choice, also the first time the tray runs.
    if let Err(err) = autostart::set(tray.settings.autostart) {
        eprintln!("riggy-tray: cannot change the autostart entry: {err}");
    }
    kwin::enable_script();

    // At login the tray may start before the panel; it then appears as soon as the panel is there.
    let handle = match tray.assume_sni_available(true).spawn() {
        Ok(handle) => handle,
        Err(err) => {
            eprintln!("riggy-tray: cannot show the tray icon: {err}");
            std::process::exit(1);
        }
    };
    handle.update(|tray: &mut RiggyTray| {
        tray.dark_panel = theme::dark_scheme();
        tray.avatar_running = tray.avatar.running();
        if tray.settings.avatar && !tray.avatar_running {
            tray.start_avatar();
        }
    });
    let url = speech_server::url();
    while !handle.is_closed() {
        let server_up = speech_server::up(&url);
        let voices = match server_up {
            true if handle.update(|tray: &mut RiggyTray| tray.voices.is_empty()).unwrap_or(false) => speech_server::voices(&url),
            _ => Vec::new(),
        };
        let dark_panel = theme::dark_scheme();
        handle.update(|tray: &mut RiggyTray| tray.refresh(server_up, voices, dark_panel));
        std::thread::sleep(POLL);
    }
}
