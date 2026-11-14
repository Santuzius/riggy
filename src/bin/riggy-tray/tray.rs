//! The tray's state and what its menu entries do; `menu` builds the menu from it.

use std::path::Path;
use std::process::{Command, Stdio};

use riggy::control::{Command as AvatarCommand, LipsyncMode};
use riggy::settings::{self, Settings};
use riggy::{autostart, files, speech_server};

use crate::avatar::Avatar;
use crate::icon::Icons;

pub struct RiggyTray {
    pub settings: Settings,
    pub speech_off: bool,
    /// The speech server answers.
    pub server_up: bool,
    /// The speech server's voices, fetched once it answers.
    pub voices: Vec<String>,
    pub avatar: Avatar,
    pub avatar_running: bool,
    /// Start the avatar again once it has closed, so it reads changed files or choices.
    pub restart_avatar: bool,
    pub icons: Icons,
    /// The desktop uses a dark colour scheme, so the icon is drawn light.
    pub dark_panel: bool,
    pub server: speech_server::Runner,
}

impl RiggyTray {
    pub fn new() -> Self {
        Self {
            settings: Settings::load(),
            speech_off: settings::speech_off(),
            server_up: false,
            voices: Vec::new(),
            avatar: Avatar::new(),
            avatar_running: false,
            restart_avatar: false,
            icons: Icons::draw(),
            dark_panel: false,
            server: speech_server::Runner::detect(),
        }
    }

    fn save(&self) {
        if let Err(err) = self.settings.save() {
            eprintln!("riggy-tray: cannot save the settings: {err}");
        }
    }

    /// Changes the settings with `change` and keeps them; reads them first, as the avatar keeps its turn there.
    pub fn choose(&mut self, change: impl FnOnce(&mut Settings)) {
        self.settings = Settings::load();
        change(&mut self.settings);
        self.save();
    }

    /// Takes the speech server's state, the avatar's and the colour scheme, as the poll found them; starts an avatar that should run.
    pub fn refresh(&mut self, server_up: bool, voices: Vec<String>, dark_panel: bool) {
        self.server_up = server_up;
        self.dark_panel = dark_panel;
        if !server_up {
            self.voices.clear();
        } else if !voices.is_empty() {
            self.voices = voices;
        }
        if !self.speech_off
            && let Err(err) = self.server.keep_running()
        {
            eprintln!("riggy-tray: cannot start the speech server: {err}");
        }
        self.avatar_running = self.avatar.running();
        if self.restart_avatar && !self.avatar_running {
            self.restart_avatar = false;
            self.start_avatar();
        }
    }

    pub fn set_speech(&mut self, on: bool) {
        let flag = settings::speech_off_flag();
        // The flag goes first on the way in, because the unit refuses to start while it exists.
        let result = if on {
            std::fs::remove_file(&flag).or_else(|err| if err.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(err) })
        } else {
            std::fs::create_dir_all(settings::config_dir()).and_then(|()| std::fs::write(&flag, ""))
        };
        if let Err(err) = result {
            eprintln!("riggy-tray: {}: {err}", flag.display());
            return;
        }
        self.speech_off = !on;
        if let Err(err) = if on { self.server.start() } else { self.server.stop() } {
            eprintln!("riggy-tray: speech server: {err}");
        }
    }

    pub fn set_avatar(&mut self, shown: bool) {
        self.choose(|s| s.avatar = shown);
        if shown && !self.avatar_running {
            self.start_avatar();
        } else if !shown && self.avatar_running {
            self.avatar.stop();
        }
    }

    pub fn start_avatar(&mut self) {
        match self.avatar.start(|| avatar_commands(&Settings::load())) {
            Ok(()) => self.avatar_running = true,
            Err(err) => eprintln!("riggy-tray: cannot start the avatar: {err}"),
        }
    }

    /// Closes a running avatar and starts it again, for choices it reads only at startup.
    fn restart_running_avatar(&mut self) {
        if self.avatar_running {
            self.avatar.stop();
            self.restart_avatar = true;
        }
    }

    /// The model the avatar shows: the tray's choice, else `[avatar] name`.
    pub fn model(&self) -> String {
        if self.settings.model.is_empty() { files::configured_model() } else { self.settings.model.clone() }
    }

    pub fn set_model(&mut self, name: String) {
        self.choose(|s| s.model = name);
        self.restart_running_avatar();
    }

    pub fn set_size(&mut self, size: f32) {
        self.choose(|s| s.size = size);
        self.restart_running_avatar();
    }

    /// Sends a command to the avatar after `change` has stored the choice behind it.
    pub fn choose_and_send(&mut self, change: impl FnOnce(&mut Settings), command: impl FnOnce(&Settings) -> AvatarCommand) {
        self.choose(change);
        self.avatar.send(&[command(&self.settings)]);
    }

    pub fn set_autostart(&mut self, on: bool) {
        self.choose(|s| s.autostart = on);
        if let Err(err) = autostart::set(on) {
            eprintln!("riggy-tray: cannot change the autostart entry: {err}");
        }
    }

    /// Puts riggy's settings, mapping files and the tray's choices back to the defaults and restarts the avatar with them.
    pub fn reset_to_defaults(&mut self) {
        if let Err(err) = files::reset_to_defaults() {
            eprintln!("riggy-tray: cannot reset to the defaults: {err}");
            return;
        }
        self.settings = Settings::default();
        self.set_autostart(self.settings.autostart);
        if self.avatar_running {
            self.restart_running_avatar();
        } else if self.settings.avatar {
            self.start_avatar();
        }
    }

    pub fn quit(&mut self) -> ! {
        if self.avatar_running {
            self.avatar.stop();
        }
        // A child server goes with the tray; the systemd unit stays as systemd runs it.
        if matches!(self.server, speech_server::Runner::Child { .. }) {
            let _ = self.server.stop();
        }
        std::process::exit(0);
    }

    pub fn status(&self) -> String {
        match (self.speech_off, self.server_up) {
            (true, _) => "Speech off".into(),
            (false, false) => "Speech starting…".into(),
            (false, true) => format!("Speaking as {} at {}×", self.settings.voice, self.settings.tempo),
        }
    }
}

/// What a freshly started avatar needs to match the choices it does not read at startup.
fn avatar_commands(settings: &Settings) -> Vec<AvatarCommand> {
    vec![AvatarCommand::ClickThrough(settings.click_through), AvatarCommand::Lipsync(settings.lipsync)]
}

/// The label of a lip-sync mode in the menu.
pub fn lipsync_label(mode: LipsyncMode) -> &'static str {
    match mode {
        LipsyncMode::Sound => "From the sound",
        LipsyncMode::Rhubarb => "Rhubarb (starts later)",
        LipsyncMode::Text => "From the text",
    }
}

/// Opens a file or folder in the user's default application; creates a missing folder first.
pub fn open(path: &Path) {
    if path.extension().is_none() {
        let _ = std::fs::create_dir_all(path);
    }
    match Command::new("xdg-open").arg(path).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        // Reaped in the background, so no zombie stays behind.
        Ok(mut child) => drop(std::thread::spawn(move || child.wait())),
        Err(err) => eprintln!("riggy-tray: xdg-open {}: {err}", path.display()),
    }
}
