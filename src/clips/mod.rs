//! Installed animation clips: `~/.local/share/riggy/animations/<pack>/assets/<clip>.glb`, each with the bone maps of its pack in `skeletons/`.
//!
//! A clip is a GLB of a skeleton and one animation (written by `riggy install`, see `install::fbx`). A bone map ([`Skeleton`]) says which of its nodes is which humanoid bone; [`Clip::load`] reads a clip through the first map of its pack whose nodes all exist in it, and gives each mapped bone's avatar-space rotation in every frame.

mod clip;
mod skeleton;

use std::path::{Path, PathBuf};

pub use clip::{Clip, ClipBone};
pub use skeleton::{DEFAULT_SKELETONS, Skeleton};

use crate::files;

/// An installed clip: its pack and file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipFile {
    pub pack: String,
    pub name: String,
    pub path: PathBuf,
}

impl ClipFile {
    /// The clip's name as the behaviour files give it: `pack/clip`.
    pub fn key(&self) -> String {
        format!("{}/{}", self.pack, self.name)
    }

    /// The folder of the pack's bone maps.
    pub fn skeletons_dir(&self) -> PathBuf {
        files::animation_pack(&self.pack).join("skeletons")
    }
}

/// Every installed clip, sorted by pack and name.
pub fn installed() -> Vec<ClipFile> {
    installed_in(&files::animations_dir())
}

fn installed_in(dir: &Path) -> Vec<ClipFile> {
    let mut clips = Vec::new();
    for pack in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Some(pack_name) = pack.file_name().to_str().map(str::to_owned) else { continue };
        for clip in std::fs::read_dir(pack.path().join("assets")).into_iter().flatten().flatten() {
            let path = clip.path();
            if path.extension().is_some_and(|e| e == "glb")
                && let Some(name) = path.file_stem().and_then(|s| s.to_str())
            {
                clips.push(ClipFile { pack: pack_name.clone(), name: name.to_owned(), path: path.clone() });
            }
        }
    }
    clips.sort_by(|a, b| (&a.pack, &a.name).cmp(&(&b.pack, &b.name)));
    clips
}

/// The clip `wanted` names: `pack/clip`, or just `clip` for the first pack that has one of that name.
pub fn find<'a>(clips: &'a [ClipFile], wanted: &str) -> Option<&'a ClipFile> {
    match wanted.split_once('/') {
        Some((pack, name)) => clips.iter().find(|c| c.pack == pack && c.name == name),
        None => clips.iter().find(|c| c.name == wanted),
    }
}

/// A pack folder name from `text`: lower case, runs of other characters than letters and digits turned into one `_`.
pub fn pack_name(text: &str) -> String {
    let mut name = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            name.extend(c.to_lowercase());
        } else if !name.ends_with('_') {
            name.push('_');
        }
    }
    name.trim_matches('_').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pack_name_is_plain() {
        assert_eq!(pack_name("Realistic Acting Animation Mocap Pack (9 Animations)"), "realistic_acting_animation_mocap_pack_9_animations");
        assert_eq!(pack_name("motifect_locomotion_complete_v1_0_fbx"), "motifect_locomotion_complete_v1_0_fbx");
    }

    #[test]
    fn a_clip_is_found_with_or_without_its_pack() {
        let clip = |pack: &str, name: &str| ClipFile { pack: pack.into(), name: name.into(), path: PathBuf::new() };
        let clips = [clip("a", "wave"), clip("b", "wave")];
        assert_eq!(find(&clips, "wave").unwrap().pack, "a");
        assert_eq!(find(&clips, "b/wave").unwrap().pack, "b");
        assert!(find(&clips, "c/wave").is_none());
    }
}

#[cfg(test)]
mod installed_tests {
    use super::*;

    /// Reads every installed clip through its pack's bone map: `cargo test -- --ignored every_installed_clip`.
    #[test]
    #[ignore = "reads the user's installed clips"]
    fn every_installed_clip_reads_through_its_bone_map() {
        let clips = installed();
        assert!(!clips.is_empty());
        for file in &clips {
            let clip = Clip::load(&file.path, &file.skeletons_dir()).unwrap_or_else(|err| panic!("{}: {err}", file.key()));
            assert!(clip.bones.len() >= 20, "{}: {} bones", file.key(), clip.bones.len());
            assert!(clip.bones.values().all(|b| b.rotations.len() == clip.frames && b.rotations.iter().all(|q| q.is_finite())));
        }
    }
}
