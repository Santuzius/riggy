//! A bone map: which node of a clip's skeleton is which humanoid bone.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::Deserialize;

/// The bone maps riggy brings, by file name, for the skeletons of known packs; `riggy install` copies the one that fits into a new pack.
pub const DEFAULT_SKELETONS: [(&str, &str); 2] = [("motifect", include_str!("../../assets/animations/skeletons/motifect.toml")), ("mixamo", include_str!("../../assets/animations/skeletons/mixamo.toml"))];

#[derive(Deserialize, Debug, Clone, Default)]
pub struct Skeleton {
    /// Humanoid bone name (VRM 1.0) → node name.
    pub bones: BTreeMap<String, String>,
    /// A clip in which the actor stands still in their natural posture (name as in the behaviour files); riggy calibrates each avatar's trunk posture on it for every clip of this skeleton.
    pub neutral: Option<String>,
}

impl Skeleton {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|err| err.to_string())
    }

    /// Whether every mapped node exists among `nodes`.
    pub fn fits(&self, nodes: &HashSet<&str>) -> bool {
        !self.bones.is_empty() && self.bones.values().all(|node| nodes.contains(node.as_str()))
    }

    /// The first map in `dir` (by file name) that fits `nodes`.
    pub fn for_nodes(dir: &Path, nodes: &HashSet<&str>) -> Option<Self> {
        let mut files: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "toml")).collect();
        files.sort();
        files.iter().filter_map(|file| std::fs::read_to_string(file).ok()).filter_map(|text| Self::parse(&text).ok()).find(|s| s.fits(nodes))
    }

    /// The name and text of riggy's default map that fits `nodes`.
    pub fn default_for(nodes: &HashSet<&str>) -> Option<(&'static str, &'static str)> {
        DEFAULT_SKELETONS.into_iter().find(|(_, text)| Self::parse(text).is_ok_and(|s| s.fits(nodes)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_maps_read_and_tell_the_packs_apart() {
        for (_, text) in DEFAULT_SKELETONS {
            assert!(Skeleton::parse(text).unwrap().bones.contains_key("hips"));
        }
        let motifect = Skeleton::parse(DEFAULT_SKELETONS[0].1).unwrap();
        let nodes: HashSet<&str> = motifect.bones.values().map(String::as_str).collect();
        assert_eq!(Skeleton::default_for(&nodes).unwrap().0, "motifect");
    }
}
