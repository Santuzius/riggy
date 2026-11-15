//! A clip read for playback: the rest pose and the per-frame rotations of its humanoid bones, in the clip's own space (Y up, the figure facing +Z).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use bevy::math::{DQuat, Quat, Vec3};

use super::Skeleton;
use crate::model_edit::gltf::{Glb, floats, parents, read_accessor, world_matrix};

#[derive(Debug, Clone)]
pub struct Clip {
    pub fps: f32,
    pub frames: usize,
    /// Humanoid bone name (VRM 1.0) → the bone.
    pub bones: HashMap<String, ClipBone>,
    /// The neutral clip its bone map names ([`super::Skeleton::neutral`]).
    pub neutral: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClipBone {
    /// Position in the rest pose.
    pub rest_position: Vec3,
    /// Rotation in the rest pose.
    pub rest_rotation: Quat,
    /// Rotation in each frame.
    pub rotations: Vec<Quat>,
}

impl Clip {
    /// Seconds from the first frame to the last.
    pub fn duration(&self) -> f32 {
        (self.frames.saturating_sub(1)) as f32 / self.fps
    }

    /// Reads the clip GLB at `path` through the first bone map in `skeletons` that fits it.
    pub fn load(path: &Path, skeletons: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|err| format!("{}: {err}", path.display()))?;
        let glb = Glb::parse(&bytes)?;
        let names: Vec<&str> = glb.gltf["nodes"].as_array().ok_or("a clip without nodes")?.iter().map(|n| n["name"].as_str().unwrap_or("")).collect();
        let node_set: HashSet<&str> = names.iter().copied().collect();
        let skeleton = Skeleton::for_nodes(skeletons, &node_set).ok_or_else(|| format!("no bone map in {} fits {}", skeletons.display(), path.display()))?;
        Self::from_glb(&glb, &names, &skeleton)
    }

    fn from_glb(glb: &Glb, names: &[&str], skeleton: &Skeleton) -> Result<Self, String> {
        let gltf = &glb.gltf;
        let animation = &gltf["animations"][0];
        let samplers = animation["samplers"].as_array().ok_or("a clip without animation")?;
        let input = samplers.first().and_then(|s| s["input"].as_u64()).ok_or("a clip without samples")? as usize;
        let times = read_accessor::<1>(gltf, &glb.binary, input)?;
        let frames = times.len();
        let duration = times.last().map_or(0.0, |t| t[0]);
        let fps = if frames > 1 && duration > 0.0 { ((frames - 1) as f64 / duration) as f32 } else { 30.0 };

        // Local rotation of every node in every frame: its rotation channel, or its rest rotation.
        let rest_local: Vec<DQuat> = gltf["nodes"].as_array().unwrap().iter().map(|n| floats::<4>(&n["rotation"]).map_or(DQuat::IDENTITY, |q| DQuat::from_array(q))).collect();
        let mut local: Vec<Option<Vec<DQuat>>> = vec![None; names.len()];
        for channel in animation["channels"].as_array().into_iter().flatten() {
            if channel["target"]["path"] != "rotation" {
                continue;
            }
            let (Some(node), Some(sampler)) = (channel["target"]["node"].as_u64(), channel["sampler"].as_u64()) else { continue };
            let output = samplers[sampler as usize]["output"].as_u64().ok_or("a sampler without output")? as usize;
            local[node as usize] = Some(read_accessor::<4>(gltf, &glb.binary, output)?.into_iter().map(DQuat::from_array).collect());
        }

        let parents = parents(gltf);
        let chain = |mut i: usize| {
            let mut nodes = vec![i];
            while let Some(&p) = parents.get(&i) {
                nodes.push(p);
                i = p;
            }
            nodes.reverse();
            nodes
        };
        let mut bones = HashMap::new();
        for (bone, node_name) in &skeleton.bones {
            let Some(node) = names.iter().position(|n| n == node_name) else { continue };
            let path = chain(node);
            let rest = world_matrix(gltf, &parents, node);
            let (_, rest_rotation, rest_position) = rest.to_scale_rotation_translation();
            let rotations = (0..frames)
                .map(|f| {
                    let q = path.iter().fold(DQuat::IDENTITY, |q, &n| q * local[n].as_ref().map_or(rest_local[n], |r| r[f]));
                    q.normalize().as_quat()
                })
                .collect();
            bones.insert(bone.clone(), ClipBone { rest_position: rest_position.as_vec3(), rest_rotation: rest_rotation.normalize().as_quat(), rotations });
        }
        Ok(Self { fps, frames, bones, neutral: skeleton.neutral.clone() })
    }
}
