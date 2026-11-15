//! The behaviour files: which clips an avatar plays when, and how strongly (`~/.local/share/riggy/behaviour/behaviour.toml`, overridden key by key by `<avatar>.toml`).

use std::path::Path;

use serde::Deserialize;

use crate::avatar::HumanoidBone as B;

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Behaviour {
    pub enabled: bool,
    pub talk: Set,
    pub idle: IdleSet,
    pub turn: TurnSet,
}

/// How a set's clips are played.
#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Playing {
    pub intensity: f32,
    pub fade: f32,
    pub bones: Bones,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct Set {
    pub clips: Vec<String>,
    #[serde(flatten)]
    pub playing: Playing,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct IdleSet {
    /// The clip that loops whenever no other clip plays, in place of riggy's own idle motion; only where it is approved on the avatar.
    pub base: Option<String>,
    pub clips: Vec<String>,
    pub pause: [f32; 2],
    #[serde(flatten)]
    pub playing: Playing,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct TurnSet {
    pub left: Vec<String>,
    pub right: Vec<String>,
    pub around: Vec<String>,
    pub min_angle: f32,
    pub around_angle: f32,
    #[serde(flatten)]
    pub playing: Playing,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum Bones {
    #[default]
    Upper,
    All,
}

impl Bones {
    /// Whether a clip moves `bone`.
    pub fn includes(self, bone: B) -> bool {
        match self {
            Bones::All => true,
            Bones::Upper => !matches!(
                bone,
                B::Hips | B::LeftUpperLeg | B::LeftLowerLeg | B::LeftFoot | B::LeftToes | B::RightUpperLeg | B::RightLowerLeg | B::RightFoot | B::RightToes | B::LeftEye | B::RightEye | B::Jaw
            ),
        }
    }
}

impl Default for Behaviour {
    fn default() -> Self {
        Self { enabled: true, talk: Set::default(), idle: IdleSet::default(), turn: TurnSet::default() }
    }
}

impl Default for Playing {
    fn default() -> Self {
        Self { intensity: 0.5, fade: 0.8, bones: Bones::Upper }
    }
}

impl Default for IdleSet {
    fn default() -> Self {
        Self { base: None, clips: Vec::new(), pause: [10.0, 30.0], playing: Playing::default() }
    }
}

impl Default for TurnSet {
    fn default() -> Self {
        Self { left: Vec::new(), right: Vec::new(), around: Vec::new(), min_angle: 40.0, around_angle: 135.0, playing: Playing::default() }
    }
}

impl Behaviour {
    /// The behaviour of the avatar `avatar`: `general` (or riggy's default) with `avatar`'s file laid over it; a broken file is reported and left out.
    pub fn load(general: &Path, avatar: &Path) -> Self {
        let read = |path: &Path| -> Option<toml::Table> {
            let text = std::fs::read_to_string(path).ok()?;
            toml::from_str(&text).map_err(|err| eprintln!("riggy: {}: {err}", path.display())).ok()
        };
        let mut table = read(general).unwrap_or_else(|| toml::from_str(riggy::files::DEFAULT_BEHAVIOUR).unwrap_or_default());
        if let Some(over) = read(avatar) {
            crate::config::overlay(&mut table, over);
        }
        toml::Value::Table(table).try_into().unwrap_or_else(|err| {
            eprintln!("riggy: {}: {err}", general.display());
            Self::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_file_reads_and_an_avatar_file_overrides_single_keys() {
        let dir = std::env::temp_dir().join(format!("riggy-behaviour-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let general = dir.join("behaviour.toml");
        let avatar = dir.join("robot.toml");
        std::fs::write(&general, riggy::files::DEFAULT_BEHAVIOUR).unwrap();
        std::fs::write(&avatar, "[talk]\nintensity = 0.2\n").unwrap();
        let b = Behaviour::load(&general, &avatar);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(b.talk.playing.intensity, 0.2);
        assert_eq!(b.talk.clips.len(), 3);
        assert_eq!(b.turn.left, ["turn_left"]);
        assert!(b.enabled);
    }
}
