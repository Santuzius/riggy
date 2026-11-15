//! The tray icon and its menu, built from the tray's state each time the panel asks.

use ksni::menu::{CheckmarkItem, RadioGroup, RadioItem, StandardItem, SubMenu};
use ksni::{Icon, MenuItem, ToolTip};
use riggy::control::{Command as AvatarCommand, LipsyncMode};
use riggy::files;

use crate::icon::TrayState;
use crate::tray::{RiggyTray, lipsync_label, open};

const TEMPOS: [f32; 6] = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0];
const SIZES: [(&str, f32); 5] = [("Small", 0.75), ("Normal", 1.0), ("Large", 1.25), ("Larger", 1.5), ("Huge", 2.0)];

type Select<T> = fn(&mut RiggyTray, T);

/// A submenu of radio items, one per option, with `current` checked; `select` gets the chosen option's value.
fn radio_menu<T: PartialEq + Clone + Send + 'static>(label: &str, options: Vec<(String, T)>, current: &T, select: Select<T>) -> MenuItem<RiggyTray> {
    let selected = options.iter().position(|(_, value)| value == current).unwrap_or(usize::MAX);
    let items = options.iter().map(|(label, _)| RadioItem { label: label.clone(), ..Default::default() }).collect();
    let values: Vec<T> = options.into_iter().map(|(_, value)| value).collect();
    SubMenu {
        label: label.into(),
        enabled: !values.is_empty(),
        submenu: vec![RadioGroup { selected, select: Box::new(move |tray: &mut RiggyTray, i| select(tray, values[i].clone())), options: items }.into()],
        ..Default::default()
    }
    .into()
}

fn labelled<T: Copy>(options: &[(&str, T)]) -> Vec<(String, T)> {
    options.iter().map(|&(label, value)| (label.to_owned(), value)).collect()
}

fn checkmark(label: &str, checked: bool, activate: fn(&mut RiggyTray)) -> MenuItem<RiggyTray> {
    CheckmarkItem { label: label.into(), checked, activate: Box::new(activate), ..Default::default() }.into()
}

fn action(label: &str, icon: &str, activate: fn(&mut RiggyTray)) -> MenuItem<RiggyTray> {
    StandardItem { label: label.into(), icon_name: icon.into(), activate: Box::new(activate), ..Default::default() }.into()
}

fn submenu(label: &str, items: Vec<MenuItem<RiggyTray>>) -> MenuItem<RiggyTray> {
    SubMenu { label: label.into(), submenu: items, ..Default::default() }.into()
}

impl RiggyTray {
    fn icon(&self) -> Vec<Icon> {
        self.icons.pick(TrayState { speech: !self.speech_off, avatar: self.avatar_running }, self.dark_panel)
    }

    fn speech_items(&self) -> Vec<MenuItem<Self>> {
        // The voices come from the server; while it is down only the chosen one is listed.
        let voices = if self.voices.is_empty() { vec![self.settings.voice.clone()] } else { self.voices.clone() };
        let mut voice_menu = radio_menu("Voice", voices.into_iter().map(|v| (v.clone(), v)).collect(), &self.settings.voice, |tray, voice| tray.choose(|s| s.voice = voice));
        if let MenuItem::SubMenu(menu) = &mut voice_menu {
            menu.enabled = !self.voices.is_empty();
        }
        vec![
            checkmark("Speech", !self.speech_off, |tray| tray.set_speech(tray.speech_off)),
            voice_menu,
            radio_menu("Tempo", TEMPOS.iter().map(|&t| (format!("{t}×"), t)).collect(), &self.settings.tempo, |tray, tempo| tray.choose(|s| s.tempo = tempo)),
        ]
    }

    fn avatar_items(&self) -> Vec<MenuItem<Self>> {
        let models = files::installed_models().into_iter().map(|name| (name.replace('_', " "), name)).collect();
        let modes = LipsyncMode::available().into_iter().map(|m| (lipsync_label(m).to_owned(), m)).collect();
        vec![
            checkmark("Show Riggy", self.avatar_running, |tray| tray.set_avatar(!tray.avatar_running)),
            radio_menu("Avatar", models, &self.model(), RiggyTray::set_model),
            radio_menu("Lip sync", modes, &self.settings.lipsync, |tray, mode| tray.choose_and_send(|s| s.lipsync = mode, |s| AvatarCommand::Lipsync(s.lipsync))),
            radio_menu("Size", labelled(&SIZES), &self.settings.size, RiggyTray::set_size),
            // Off: the left mouse button moves Riggy, the right one turns it, the wheel zooms.
            checkmark("Animation clips", self.settings.clips, |tray| {
                let on = !tray.settings.clips;
                tray.choose_and_send(|s| s.clips = on, |s| AvatarCommand::Clips(s.clips));
            }),
            self.play_menu(),
            checkmark("Click-through", self.settings.click_through, |tray| {
                let on = !tray.settings.click_through;
                tray.choose_and_send(|s| s.click_through = on, |s| AvatarCommand::ClickThrough(s.click_through));
            }),
        ]
    }

    /// Every installed clip approved on the avatar (all with "Show not approved animations in the list"), one submenu per pack; choosing one plays it once.
    fn play_menu(&self) -> MenuItem<Self> {
        let model = self.model();
        let all = self.settings.unapproved_in_list;
        let clips: Vec<_> = riggy::clips::installed().into_iter().filter(|c| all || c.approved(&model)).collect();
        let mut packs: Vec<MenuItem<Self>> = Vec::new();
        for pack in clips.iter().map(|c| c.pack.clone()).collect::<std::collections::BTreeSet<_>>() {
            let items = clips
                .iter()
                .filter(|c| c.pack == pack)
                .map(|c| {
                    let key = c.key();
                    StandardItem { label: c.name.replace('_', " "), activate: Box::new(move |tray: &mut Self| tray.avatar.send(&[AvatarCommand::Play(key.clone())])), ..Default::default() }.into()
                })
                .collect();
            packs.push(submenu(&pack.replace('_', " "), items));
        }
        let mut menu = submenu("Play animation", packs);
        if let MenuItem::SubMenu(m) = &mut menu {
            m.enabled = self.avatar_running && !clips.is_empty();
        }
        menu
    }

    fn start_menu(&self) -> MenuItem<Self> {
        submenu(
            "Start automatically",
            vec![
                checkmark("At login", self.settings.autostart, |tray| tray.set_autostart(!tray.settings.autostart)),
                checkmark("When an AI agent starts riggy-mcp", self.settings.start_with_mcp, |tray| {
                    let on = !tray.settings.start_with_mcp;
                    tray.choose(|s| s.start_with_mcp = on);
                }),
            ],
        )
    }

    fn files_menu(&self) -> MenuItem<Self> {
        submenu(
            "Advanced settings",
            vec![
                // riggy-mcp reads it when an agent starts it.
                checkmark("Default MCP instructions", self.settings.mcp_instructions, |tray| {
                    let on = !tray.settings.mcp_instructions;
                    tray.choose(|s| s.mcp_instructions = on);
                }),
                checkmark("Show not approved animations in behaviour", self.settings.unapproved_in_behaviour, |tray| {
                    let on = !tray.settings.unapproved_in_behaviour;
                    tray.choose_and_send(|s| s.unapproved_in_behaviour = on, |s| AvatarCommand::Unapproved(s.unapproved_in_behaviour));
                }),
                checkmark("Show not approved animations in the list", self.settings.unapproved_in_list, |tray| {
                    let on = !tray.settings.unapproved_in_list;
                    tray.choose(|s| s.unapproved_in_list = on);
                }),
                MenuItem::Separator,
                action("Settings file", "document-edit", |_| open(&files::user_config())),
                action("Avatar's mapping file", "document-edit", |tray| open(&files::mapping(&tray.model()))),
                action("Avatars folder", "folder", |_| open(&files::models_dir())),
                action("Behaviour folder", "folder", |_| open(&files::behaviour_dir())),
                action("Animations folder", "folder", |_| open(&files::animations_dir())),
                MenuItem::Separator,
                action("Install KWin script of this version", "kwin", |_| crate::kwin::install_script()),
            ],
        )
    }
}

impl ksni::Tray for RiggyTray {
    fn id(&self) -> String {
        "riggy".into()
    }

    fn title(&self) -> String {
        "Riggy".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icon()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip { title: "Riggy".into(), description: self.status(), icon_pixmap: self.icon(), ..Default::default() }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.set_avatar(!self.avatar_running);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items = vec![StandardItem { label: self.status(), enabled: false, ..Default::default() }.into(), MenuItem::Separator];
        items.extend(self.speech_items());
        items.push(MenuItem::Separator);
        items.extend(self.avatar_items());
        items.push(MenuItem::Separator);
        items.push(self.start_menu());
        items.push(self.files_menu());
        items.push(action("Reset to defaults", "edit-reset", RiggyTray::reset_to_defaults));
        items.push(action("Quit", "application-exit", |tray| tray.quit()));
        items
    }
}
