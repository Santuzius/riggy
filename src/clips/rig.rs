//! A VRM 1.0 model's node tree in its rest pose, without Bevy: for checking off-screen where a clip puts the avatar's bones (stick figures, regression tests).

use std::collections::HashMap;

use bevy::math::{Affine3A, Quat, Vec3};

use crate::model_edit::gltf::{Glb, floats, parents};

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
        let locals = nodes
            .iter()
            .map(|n| Local {
                translation: floats::<3>(&n["translation"]).map_or(Vec3::ZERO, |v| Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32)),
                rotation: floats::<4>(&n["rotation"]).map_or(Quat::IDENTITY, |q| Quat::from_xyzw(q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32)),
                scale: floats::<3>(&n["scale"]).map_or(Vec3::ONE, |v| Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32)),
            })
            .collect::<Vec<_>>();
        let map = parents(&glb.gltf);
        let parents = (0..locals.len()).map(|i| map.get(&i).copied()).collect();
        let bones = glb.gltf["extensions"]["VRMC_vrm"]["humanoid"]["humanBones"].as_object().ok_or("not a VRM 1.0 model")?;
        let humanoid = bones.iter().filter_map(|(name, b)| Some((name.clone(), b["node"].as_u64()? as usize))).collect();
        Ok(Self { locals, parents, humanoid })
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
