//! `riggy-tray`: the tray icon that controls speech and the avatar.
//!
//! Its menu switches speech on and off (the flag file `riggy-mcp` checks, and the `qwentts` user service, whose GPU memory it frees), picks voice and tempo, shows or closes the avatar, sets the avatar's click-through and lip-sync mode, and puts all of riggy's settings back to the defaults (keeping the old files as `*.old`). The choices persist in `riggy::settings`. A left click on the icon shows or closes the avatar. Quitting the tray closes the avatar too.

mod avatar;
mod icon;

use std::process::Command;
use std::time::Duration;

use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, RadioGroup, RadioItem, StandardItem, SubMenu};
use ksni::{Icon, MenuItem, ToolTip};
use riggy::control::{Command as AvatarCommand, LipsyncMode};
use riggy::settings::{self, Settings};

use avatar::Avatar;
use icon::Icons;

/// The speech server's systemd user unit.
const SPEECH_UNIT: &str = "qwentts";
const TEMPOS: [f32; 6] = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0];
/// How often the tray looks at the speech server and the avatar.
const POLL: Duration = Duration::from_secs(2);

fn main() {
    let tray = RiggyTray {
        settings: Settings::load(),
        speech_off: settings::speech_off(),
        server_up: false,
        voices: Vec::new(),
        avatar: Avatar::new(),
        avatar_running: false,
        restart_avatar: false,
        icons: Icons::draw(),
    };
    // At login the tray may start before the panel; it then appears as soon as the panel is there.
    let handle = match tray.assume_sni_available(true).spawn() {
        Ok(handle) => handle,
        Err(err) => {
            eprintln!("riggy-tray: cannot show the tray icon: {err}");
            std::process::exit(1);
        }
    };
    handle.update(|tray: &mut RiggyTray| {
        tray.avatar_running = tray.avatar.running();
        if tray.settings.avatar && !tray.avatar_running {
            tray.start_avatar();
        }
    });
    let url = tts_url();
    while !handle.is_closed() {
        let server_up = server_up(&url);
        let voices = match server_up {
            true if handle.update(|tray: &mut RiggyTray| tray.voices.is_empty()).unwrap_or(false) => voices(&url),
            _ => Vec::new(),
        };
        handle.update(|tray: &mut RiggyTray| {
            tray.server_up = server_up;
            if !server_up {
                tray.voices.clear();
            } else if !voices.is_empty() {
                tray.voices = voices;
            }
            tray.avatar_running = tray.avatar.running();
            if tray.restart_avatar && !tray.avatar_running {
                tray.restart_avatar = false;
                tray.start_avatar();
            }
        });
        std::thread::sleep(POLL);
    }
}

struct RiggyTray {
    settings: Settings,
    speech_off: bool,
    /// The speech server answers.
    server_up: bool,
    /// The speech server's voices, fetched once it answers.
    voices: Vec<String>,
    avatar: Avatar,
    avatar_running: bool,
    /// Start the avatar again once it has closed, so it reads its reset files.
    restart_avatar: bool,
    icons: Icons,
}

impl RiggyTray {
    fn save(&self) {
        if let Err(err) = self.settings.save() {
            eprintln!("riggy-tray: cannot save the settings: {err}");
        }
    }

    fn set_speech(&mut self, on: bool) {
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
        // --no-block: starting loads the model, which takes a while; the poll notices when the server answers.
        let action = if on { "start" } else { "stop" };
        if let Err(err) = Command::new("systemctl").args(["--user", "--no-block", action, SPEECH_UNIT]).status() {
            eprintln!("riggy-tray: systemctl {action} {SPEECH_UNIT}: {err}");
        }
    }

    fn set_avatar(&mut self, shown: bool) {
        self.settings.avatar = shown;
        self.save();
        if shown && !self.avatar_running {
            self.start_avatar();
        } else if !shown && self.avatar_running {
            self.avatar.stop();
        }
    }

    /// Puts riggy's settings, mapping files and the tray's choices back to the defaults and restarts the avatar with them.
    fn reset_to_defaults(&mut self) {
        if let Err(err) = riggy::files::reset_to_defaults() {
            eprintln!("riggy-tray: cannot reset to the defaults: {err}");
            return;
        }
        self.settings = Settings::default();
        if self.avatar_running {
            self.avatar.stop();
            self.restart_avatar = true;
        } else if self.settings.avatar {
            self.start_avatar();
        }
    }

    fn start_avatar(&mut self) {
        match self.avatar.start(|| avatar_commands(&Settings::load())) {
            Ok(()) => self.avatar_running = true,
            Err(err) => eprintln!("riggy-tray: cannot start the avatar: {err}"),
        }
    }

    fn status(&self) -> String {
        match (self.speech_off, self.server_up) {
            (true, _) => "Speech off".into(),
            (false, false) => "Speech starting…".into(),
            (false, true) => format!("Speaking as {} at {}×", self.settings.voice, self.settings.tempo),
        }
    }

    fn icon(&self) -> Vec<Icon> {
        if self.speech_off { self.icons.silent.clone() } else { self.icons.speaking.clone() }
    }

    fn voice_menu(&self) -> MenuItem<Self> {
        let voices = self.voices_or_current();
        let options = voices.iter().map(|label| RadioItem { label: label.clone(), ..Default::default() }).collect();
        SubMenu {
            label: "Voice".into(),
            // The list comes from the server.
            enabled: !self.voices.is_empty(),
            submenu: vec![
                RadioGroup {
                    selected: voices.iter().position(|v| v == &self.settings.voice).unwrap_or(usize::MAX),
                    select: Box::new(move |tray: &mut Self, i| {
                        tray.settings.voice = voices[i].clone();
                        tray.save();
                    }),
                    options,
                }
                .into(),
            ],
            ..Default::default()
        }
        .into()
    }

    /// The server's voices, or only the chosen one while the server is down.
    fn voices_or_current(&self) -> Vec<String> {
        if self.voices.is_empty() { vec![self.settings.voice.clone()] } else { self.voices.clone() }
    }

    fn tempo_menu(&self) -> MenuItem<Self> {
        SubMenu {
            label: "Tempo".into(),
            submenu: vec![
                RadioGroup {
                    selected: TEMPOS.iter().position(|&t| t == self.settings.tempo).unwrap_or(usize::MAX),
                    select: Box::new(|tray: &mut Self, i| {
                        tray.settings.tempo = TEMPOS[i];
                        tray.save();
                    }),
                    options: TEMPOS.iter().map(|t| RadioItem { label: format!("{t}×"), ..Default::default() }).collect(),
                }
                .into(),
            ],
            ..Default::default()
        }
        .into()
    }

    fn lipsync_menu(&self) -> MenuItem<Self> {
        let modes = LipsyncMode::available();
        let options = modes.iter().map(|&m| RadioItem { label: lipsync_label(m).into(), ..Default::default() }).collect();
        SubMenu {
            label: "Lip sync".into(),
            submenu: vec![
                RadioGroup {
                    selected: modes.iter().position(|&m| m == self.settings.lipsync).unwrap_or(usize::MAX),
                    select: Box::new(move |tray: &mut Self, i| {
                        tray.settings.lipsync = modes[i];
                        tray.save();
                        tray.avatar.send(&[AvatarCommand::Lipsync(modes[i])]);
                    }),
                    options,
                }
                .into(),
            ],
            ..Default::default()
        }
        .into()
    }
}

impl ksni::Tray for RiggyTray {
    fn id(&self) -> String {
        "riggy".into()
    }

    fn title(&self) -> String {
        "riggy".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icon()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip { title: "riggy".into(), description: self.status(), icon_pixmap: self.icon(), ..Default::default() }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.set_avatar(!self.avatar_running);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem { label: self.status(), enabled: false, ..Default::default() }.into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "Speech".into(),
                checked: !self.speech_off,
                activate: Box::new(|tray: &mut Self| tray.set_speech(tray.speech_off)),
                ..Default::default()
            }
            .into(),
            self.voice_menu(),
            self.tempo_menu(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "Show riggy".into(),
                checked: self.avatar_running,
                activate: Box::new(|tray: &mut Self| tray.set_avatar(!tray.avatar_running)),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Click-through".into(),
                checked: self.settings.click_through,
                activate: Box::new(|tray: &mut Self| {
                    tray.settings.click_through = !tray.settings.click_through;
                    tray.save();
                    tray.avatar.send(&[AvatarCommand::ClickThrough(tray.settings.click_through)]);
                }),
                ..Default::default()
            }
            .into(),
            self.lipsync_menu(),
            MenuItem::Separator,
            StandardItem {
                label: "Reset to defaults".into(),
                icon_name: "edit-reset".into(),
                activate: Box::new(|tray: &mut Self| tray.reset_to_defaults()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|tray: &mut Self| {
                    if tray.avatar_running {
                        tray.avatar.stop();
                    }
                    std::process::exit(0);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// What a freshly started avatar needs to match the settings.
fn avatar_commands(settings: &Settings) -> Vec<AvatarCommand> {
    vec![AvatarCommand::ClickThrough(settings.click_through), AvatarCommand::Lipsync(settings.lipsync)]
}

fn lipsync_label(mode: LipsyncMode) -> &'static str {
    match mode {
        LipsyncMode::Sound => "From the sound",
        LipsyncMode::Rhubarb => "Rhubarb (starts later)",
        LipsyncMode::Text => "From the text",
    }
}

fn tts_url() -> String {
    std::env::var("TTS_URL").unwrap_or_else(|_| "http://127.0.0.1:8902".into())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(1))).build().into()
}

fn server_up(url: &str) -> bool {
    agent().get(format!("{url}/health")).call().is_ok()
}

/// The server's voices; empty if it does not answer.
fn voices(url: &str) -> Vec<String> {
    let Ok(mut response) = agent().get(format!("{url}/v1/audio/voices")).call() else {
        return Vec::new();
    };
    let Ok(body) = response.body_mut().read_json::<serde_json::Value>() else {
        return Vec::new();
    };
    // Accept {"voices": [{"name": …}]} and plain lists of names or objects.
    let list = body.get("voices").unwrap_or(&body);
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|voice| voice.as_str().or_else(|| voice["name"].as_str()).map(str::to_owned))
        .collect()
}
