//! A VRM 1.0 or mapped GLB model's node tree in its rest pose, without Bevy: for checking off-screen where a clip puts the avatar's bones (stick figures, regression tests).

use std::collections::HashMap;

use bevy::math::{Affine3A, Quat, Vec3};

use crate::model_edit::gltf::{Glb, local_matrix, parents};

/// A node's local transform.
#[derive(Debug, Clone, Copy)]
pub struct Local {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Local {
    pub fn affine(&self) -> Affine3A {
        Affine3A::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

#[derive(Debug, Clone)]
pub struct Rig {
    /// Each node's rest transform relative to its parent.
    pub locals: Vec<Local>,
    pub parents: Vec<Option<usize>>,
    /// Humanoid bone name (VRM 1.0) → node.
    pub humanoid: HashMap<String, usize>,
}

impl Rig {
    /// The rig of the VRM 1.0 file `bytes`.
    pub fn from_vrm(bytes: &[u8]) -> Result<Self, String> {
        let glb = Glb::parse(bytes)?;
        let nodes = glb.gltf["nodes"].as_array().ok_or("a model without nodes")?;
        let mut rig = Self::from_nodes(nodes, &glb.gltf);
        let bones = glb.gltf["extensions"]["VRMC_vrm"]["humanoid"]["humanBones"].as_object().ok_or("not a VRM 1.0 model")?;
        rig.humanoid = bones.iter().filter_map(|(name, b)| Some((name.clone(), b["node"].as_u64()? as usize))).collect();
        Ok(rig)
    }

    /// The rig of the GLB file `bytes` with riggy's mapping file: `humanoid` (VRM bone name → node name), `scale` (model units to metres) and `rest` (node name → turn in avatar space as axis times degrees, the mapping's `[rest]` corrections).
    pub fn from_glb(bytes: &[u8], humanoid: &HashMap<String, String>, scale: f32, rest: &HashMap<String, Vec3>) -> Result<Self, String> {
        let glb = Glb::parse(bytes)?;
        let nodes = glb.gltf["nodes"].as_array().ok_or("a model without nodes")?;
        let mut names: HashMap<&str, usize> = HashMap::new();
        for (i, n) in nodes.iter().enumerate() {
            let Some(name) = n["name"].as_str() else { continue };
            names.insert(name, i);
            names.entry(base_name(name)).or_insert(i);
        }
        let mut rig = Self::from_nodes(nodes, &glb.gltf);
        rig.humanoid = humanoid.iter().filter_map(|(bone, node)| Some((bone.clone(), *names.get(node.as_str())?))).collect();
        for i in 0..rig.locals.len() {
            if rig.parents[i].is_none() {
                rig.locals[i].translation *= scale;
                rig.locals[i].scale *= scale;
            }
        }
        // Each correction turns its node about its pivot in avatar space, on top of the rest pose, as riggy does on load.
        for (node, turn) in rest {
            let Some(&i) = names.get(node.as_str()) else { continue };
            if turn.length() < 1e-6 {
                continue;
            }
            let parent = rig.parent_world(i).to_scale_rotation_translation().1;
            let q = Quat::from_axis_angle(turn.normalize(), turn.length().to_radians());
            rig.locals[i].rotation = parent.inverse() * q * parent * rig.locals[i].rotation;
        }
        Ok(rig)
    }

    fn from_nodes(nodes: &[serde_json::Value], gltf: &serde_json::Value) -> Self {
        let locals = nodes
            .iter()
            .map(|n| {
                let (scale, rotation, translation) = local_matrix(n).as_mat4().to_scale_rotation_translation();
                Local { translation, rotation, scale }
            })
            .collect::<Vec<_>>();
        let map = parents(gltf);
        let parents = (0..locals.len()).map(|i| map.get(&i).copied()).collect();
        Self { locals, parents, humanoid: HashMap::new() }
    }

    /// Every node's world transform with the local rotations `rotations` in place of the rest ones.
    pub fn worlds(&self, rotations: &HashMap<usize, Quat>) -> Vec<Affine3A> {
        let mut worlds: Vec<Option<Affine3A>> = vec![None; self.locals.len()];
        fn world(rig: &Rig, rotations: &HashMap<usize, Quat>, worlds: &mut Vec<Option<Affine3A>>, i: usize) -> Affine3A {
            if let Some(w) = worlds[i] {
                return w;
            }
            let mut local = rig.locals[i];
            if let Some(&r) = rotations.get(&i) {
                local.rotation = r;
            }
            let parent = rig.parents[i].map_or(Affine3A::IDENTITY, |p| world(rig, rotations, worlds, p));
            let w = parent * local.affine();
            worlds[i] = Some(w);
            w
        }
        (0..self.locals.len()).map(|i| world(self, rotations, &mut worlds, i)).collect()
    }

    /// The world transform of node `i`'s parent in the rest pose.
    pub fn parent_world(&self, i: usize) -> Affine3A {
        self.parents[i].map_or(Affine3A::IDENTITY, |p| self.worlds(&HashMap::new())[p])
    }
}

/// Drops the `_<digits>` suffix Sketchfab appends to node names.
pub fn base_name(name: &str) -> &str {
    match name.rsplit_once('_') {
        Some((base, digits)) if !base.is_empty() && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => name,
    }
}
