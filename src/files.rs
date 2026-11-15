//! Where riggy keeps its files, as Linux programs usually do:
//!
//! - `~/.config/riggy/` (`$XDG_CONFIG_HOME/riggy`): `riggy.toml` with the user's settings, only what differs from the defaults; the tray's `settings.toml`; and the file `speech-off` while speech is switched off.
//! - `~/.local/share/riggy/` (`$XDG_DATA_HOME/riggy`): the models, `models/NAME.glb` or `models/NAME.vrm`, which are not part of riggy, and their mapping files, `avatars/NAME.toml`, copied from riggy's default the first time a model is used.
//! - The defaults that come with riggy: `riggy.toml`, `assets/avatars/` and `assets/animations/` in `$RIGGY_DATA_DIR` (the package's `share/riggy`; the dev shell points it at the checkout), built into the program as well.

use std::io;
use std::path::{Path, PathBuf};

/// The default settings, also built in for a riggy that runs without its data folder.
const DEFAULT_CONFIG: &str = include_str!("../riggy.toml");
/// Default mapping files built in, by model name.
/// The behaviour file riggy starts with.
pub const DEFAULT_BEHAVIOUR: &str = include_str!("../assets/behaviour/behaviour.toml");

const DEFAULT_MAPPINGS: [(&str, &str); 3] = [
    ("cyborg_girl", include_str!("../assets/avatars/cyborg_girl.toml")),
    ("cyber_ver101", include_str!("../assets/avatars/cyber_ver101.toml")),
    ("Ballytic813c_Reforged_ByLio3DW", include_str!("../assets/avatars/Ballytic813c_Reforged_ByLio3DW.toml")),
];

/// Default calibration files built in, by pack and avatar (`clips::calibration`).
const DEFAULT_CALIBRATIONS: [(&str, &str, &str); 2] = [
    ("motifect_locomotion_complete_v1_0_fbx", "cyber_ver101", include_str!("../assets/animations/calibration/motifect_locomotion_complete_v1_0_fbx/cyber_ver101.toml")),
    ("motifect_locomotion_complete_v1_0_fbx", "cyborg_girl", include_str!("../assets/animations/calibration/motifect_locomotion_complete_v1_0_fbx/cyborg_girl.toml")),
];

/// riggy's default calibration of the animation pack `pack` for the avatar `avatar`, if it has one.
pub fn default_calibration(pack: &str, avatar: &str) -> Option<String> {
    let file = defaults_dir().map(|dir| dir.join("assets/animations/calibration").join(pack).join(format!("{avatar}.toml")));
    file.and_then(|file| std::fs::read_to_string(file).ok())
        .or_else(|| DEFAULT_CALIBRATIONS.iter().find(|(p, a, _)| *p == pack && *a == avatar).map(|(_, _, text)| (*text).into()))
}

/// What a fresh user config holds: nothing but a pointer to the defaults.
const USER_CONFIG_TEMPLATE: &str = "# Your riggy settings. Everything not set here comes from the defaults: riggy.toml in riggy's data folder (share/riggy in the package). Copy a setting from there to change it, e.g.\n#\n# [lipsync]\n# mode = \"text\"\n";

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn xdg_dir(variable: &str, fallback: &str) -> PathBuf {
    std::env::var_os(variable).map(PathBuf::from).filter(|dir| dir.is_absolute()).unwrap_or_else(|| home().join(fallback))
}

/// `$XDG_CACHE_HOME/riggy/models`, by default `~/.cache/riggy/models`: VRM models with their mapping file's edits applied, rebuilt whenever riggy starts or the mapping file changes.
pub fn edited_models_dir() -> PathBuf {
    xdg_dir("XDG_CACHE_HOME", ".cache").join("riggy/models")
}

/// `$XDG_CONFIG_HOME/riggy`, by default `~/.config/riggy`.
pub fn config_dir() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config").join("riggy")
}

/// `$XDG_DATA_HOME`, by default `~/.local/share`.
pub fn xdg_data_home() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share")
}

/// `$XDG_DATA_HOME/riggy`, by default `~/.local/share/riggy`.
pub fn data_dir() -> PathBuf {
    xdg_data_home().join("riggy")
}

/// `$XDG_CONFIG_HOME/autostart`, where the desktop session looks for programs to start at login.
pub fn autostart_dir() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config").join("autostart")
}

/// The folder with riggy's own defaults, if it is known.
fn defaults_dir() -> Option<PathBuf> {
    std::env::var_os("RIGGY_DATA_DIR").map(PathBuf::from)
}

/// The user's settings: `$RIGGY_CONFIG`, by default `~/.config/riggy/riggy.toml`.
pub fn user_config() -> PathBuf {
    std::env::var_os("RIGGY_CONFIG").map(PathBuf::from).unwrap_or_else(|| config_dir().join("riggy.toml"))
}

/// The defaults file in the data folder, if there is one; riggy watches it for changes, so the defaults can be tuned in the checkout.
pub fn default_config_file() -> Option<PathBuf> {
    defaults_dir().map(|dir| dir.join("riggy.toml")).filter(|file| file.exists())
}

/// The text of the default settings.
pub fn default_config() -> String {
    default_config_file().and_then(|file| std::fs::read_to_string(file).ok()).unwrap_or_else(|| DEFAULT_CONFIG.into())
}

/// The folder of the installed animation packs.
pub fn animations_dir() -> PathBuf {
    data_dir().join("animations")
}

/// The folder of the animation pack `pack`: its clips in `assets/`, its bone maps in `skeletons/`.
pub fn animation_pack(pack: &str) -> PathBuf {
    animations_dir().join(pack)
}

/// The file of the installed clip `name` of `pack`.
pub fn animation(pack: &str, name: &str) -> PathBuf {
    animation_pack(pack).join("assets").join(format!("{name}.glb"))
}

/// The folder of the behaviour files: `behaviour.toml` for every avatar, `<avatar>.toml` for one.
pub fn behaviour_dir() -> PathBuf {
    data_dir().join("behaviour")
}

/// The behaviour file of every avatar.
pub fn behaviour() -> PathBuf {
    behaviour_dir().join("behaviour.toml")
}

/// The behaviour file of the avatar `name`, which overrides `behaviour.toml` key by key.
pub fn avatar_behaviour(name: &str) -> PathBuf {
    behaviour_dir().join(format!("{name}.toml"))
}

/// The folder of the models.
pub fn models_dir() -> PathBuf {
    data_dir().join("models")
}

/// File extensions of avatar models: GLB with a mapping file, VRM 1.0 without.
pub const MODEL_EXTENSIONS: [&str; 2] = ["glb", "vrm"];

/// The model file of `name`: the installed `NAME.vrm` if there is one, else `NAME.glb`.
pub fn model(name: &str) -> PathBuf {
    let vrm = models_dir().join(format!("{name}.vrm"));
    if vrm.exists() { vrm } else { models_dir().join(format!("{name}.glb")) }
}

/// The folder of the textures of model `name`, for a model whose file comes without them (mapping file: `[material]`).
pub fn model_textures(name: &str) -> PathBuf {
    models_dir().join(name)
}

/// The names of the installed models, sorted.
pub fn installed_models() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(models_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()).is_some_and(|e| MODEL_EXTENSIONS.contains(&e)))
        .filter_map(|path| path.file_stem().and_then(|s| s.to_str()).map(str::to_owned))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The model `[avatar] name` picks: the user's setting, else the default.
pub fn configured_model() -> String {
    let name = |text: &str| toml::from_str::<toml::Table>(text).ok()?.get("avatar")?.get("name")?.as_str().map(str::to_owned);
    std::fs::read_to_string(user_config()).ok().and_then(|text| name(&text)).or_else(|| name(&default_config())).unwrap_or_default()
}

/// The user's mapping file of `name`.
pub fn mapping(name: &str) -> PathBuf {
    data_dir().join("avatars").join(format!("{name}.toml"))
}

/// The text of the mapping file of `name`: the user's, or else riggy's default one.
pub fn mapping_text(name: &str) -> Option<String> {
    std::fs::read_to_string(mapping(name)).ok().or_else(|| default_mapping(name))
}

/// riggy's default mapping file of `name`, if it has one.
fn default_mapping(name: &str) -> Option<String> {
    let file = defaults_dir().map(|dir| dir.join("assets/avatars").join(format!("{name}.toml")));
    file.and_then(|file| std::fs::read_to_string(file).ok())
        .or_else(|| DEFAULT_MAPPINGS.iter().find(|(known, _)| *known == name).map(|(_, text)| (*text).into()))
}

/// Creates the user's config, the behaviour file and the mapping file of `name` from the defaults where they are missing.
pub fn ensure_user_files(name: &str) -> io::Result<()> {
    write_new(&user_config(), USER_CONFIG_TEMPLATE)?;
    write_new(&behaviour(), DEFAULT_BEHAVIOUR)?;
    if let Some(text) = default_mapping(name) {
        write_new(&mapping(name), &text)?;
    }
    Ok(())
}

/// Puts the user's config, the behaviour file, every mapping file riggy has a default for, the animation packs' calibrations, and the tray's choices back to the defaults. The replaced files are kept as `*.old` next to them.
pub fn reset_to_defaults() -> io::Result<()> {
    let mut files = vec![(user_config(), USER_CONFIG_TEMPLATE.to_string()), (config_dir().join("settings.toml"), String::new()), (behaviour(), DEFAULT_BEHAVIOUR.to_string())];
    let avatars = data_dir().join("avatars");
    for entry in std::fs::read_dir(&avatars).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = path.file_stem().and_then(|s| s.to_str()).filter(|_| path.extension().is_some_and(|e| e == "toml"));
        if let Some(text) = name.and_then(default_mapping) {
            files.push((path, text));
        }
    }
    // Each animation pack's calibrations: riggy's default where it has one; the others go, so riggy measures them again.
    for pack in std::fs::read_dir(animations_dir()).into_iter().flatten().flatten() {
        let Some(pack_name) = pack.file_name().to_str().map(str::to_owned) else { continue };
        for entry in std::fs::read_dir(pack.path().join("calibration")).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "toml") {
                let avatar = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
                files.push((path.clone(), default_calibration(&pack_name, avatar).unwrap_or_default()));
            }
        }
    }
    for (file, text) in files {
        if file.exists() {
            std::fs::rename(&file, old(&file))?;
        }
        if !text.is_empty() {
            write_new(&file, &text)?;
        }
    }
    Ok(())
}

fn old(file: &Path) -> PathBuf {
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    file.with_file_name(name)
}

fn write_new(file: &Path, text: &str) -> io::Result<()> {
    if file.exists() {
        return Ok(());
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(file, text)
}

/// `path` with `~/` expanded, and a relative one taken from the user's config folder.
pub fn resolve(path: &Path) -> PathBuf {
    let path = match path.strip_prefix("~") {
        Ok(rest) => home().join(rest),
        Err(_) => path.to_path_buf(),
    };
    if path.is_absolute() { path } else { config_dir().join(path) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_defaults_parse() {
        let _: toml::Table = toml::from_str(DEFAULT_CONFIG).unwrap();
        let _: toml::Table = toml::from_str(USER_CONFIG_TEMPLATE).unwrap();
        for (_, mapping) in DEFAULT_MAPPINGS {
            let _: toml::Table = toml::from_str(mapping).unwrap();
        }
    }

    #[test]
    fn old_copies_sit_next_to_the_file() {
        assert_eq!(old(Path::new("/c/riggy/riggy.toml")), Path::new("/c/riggy/riggy.toml.old"));
    }
}
