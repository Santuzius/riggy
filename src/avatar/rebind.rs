//! `[[rebind]]` in a VRM avatar's mapping file: moves a share of the skin binding of some materials from some humanoid bones to another, when the model loads.
//!
//! Only a model whose own mapping file has entries is changed; what to rebind depends on how that model was made. The `cyber` model, for example, binds its long coat to the thighs like trousers, so its mapping file moves most of that binding to the hips.

use bevy::gltf::GltfMaterialName;
use bevy::mesh::VertexAttributeValues;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use serde::Deserialize;

use super::{Humanoid, HumanoidBone};

/// One `[[rebind]]` entry.
#[derive(Deserialize, Debug, Clone)]
pub struct Rebind {
    /// Part of the names of the materials whose vertices are rebound.
    pub material: String,
    /// VRM humanoid bone names the binding is taken from.
    pub from: Vec<String>,
    /// VRM humanoid bone name the binding goes to.
    pub to: String,
    /// Share of the binding that moves (0..1).
    pub share: f32,
}

/// The parts of a mapping file a VRM avatar reads.
#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct VrmMapping {
    pub rebind: Vec<Rebind>,
}

/// Applies `rebinds` to the skinned primitives, given by material name, mesh and skin.
pub fn apply<'a>(rebinds: &[Rebind], humanoid: &Humanoid, primitives: impl Iterator<Item = (&'a GltfMaterialName, &'a Mesh3d, &'a SkinnedMesh)>, meshes: &mut Assets<Mesh>) {
    let entity = |name: &str| name.parse::<HumanoidBone>().ok().and_then(|bone| humanoid.bones.get(&bone)).map(|b| b.entity);
    for (material, mesh, skin) in primitives {
        for rebind in rebinds.iter().filter(|r| material.0.contains(&r.material)) {
            let index = |e: Entity| skin.joints.iter().position(|&j| j == e).map(|i| i as u16);
            let Some(to) = entity(&rebind.to).and_then(index) else {
                warn!("rebind: bone {} is not in the skin of {}", rebind.to, material.0);
                continue;
            };
            let from: Vec<u16> = rebind.from.iter().filter_map(|name| entity(name)).filter_map(index).collect();
            info!("rebind: {} from {:?} to {}", material.0, rebind.from, rebind.to);
            if let Some(mut mesh) = meshes.get_mut(&mesh.0) {
                rebind_mesh(&mut mesh, &from, to, rebind.share.clamp(0.0, 1.0));
            }
        }
    }
}

fn rebind_mesh(mesh: &mut Mesh, from: &[u16], to: u16, share: f32) {
    let Some(VertexAttributeValues::Uint16x4(joints)) = mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX).cloned() else { return };
    let Some(VertexAttributeValues::Float32x4(weights)) = mesh.attribute_mut(Mesh::ATTRIBUTE_JOINT_WEIGHT) else { return };
    let mut new_joints = joints.clone();
    for (j, w) in new_joints.iter_mut().zip(weights.iter_mut()) {
        (*j, *w) = rebound(*j, *w, from, to, share);
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(new_joints));
}

/// One vertex's four joints and weights with `share` of the weight on `from` moved to `to`; the four heaviest joints are kept and their weights sum to 1.
fn rebound(joints: [u16; 4], weights: [f32; 4], from: &[u16], to: u16, share: f32) -> ([u16; 4], [f32; 4]) {
    let mut entries: Vec<(u16, f32)> = Vec::with_capacity(5);
    let mut moved = 0.0;
    for (j, w) in joints.into_iter().zip(weights) {
        let w = if from.contains(&j) {
            moved += w * share;
            w * (1.0 - share)
        } else {
            w
        };
        match entries.iter_mut().find(|(e, _)| *e == j) {
            Some(entry) => entry.1 += w,
            None => entries.push((j, w)),
        }
    }
    match entries.iter_mut().find(|(e, _)| *e == to) {
        Some(entry) => entry.1 += moved,
        None => entries.push((to, moved)),
    }
    entries.sort_by(|a, b| b.1.total_cmp(&a.1));
    entries.truncate(4);
    let total: f32 = entries.iter().map(|e| e.1).sum();
    let (mut js, mut ws) = ([0u16; 4], [0f32; 4]);
    for (i, (j, w)) in entries.into_iter().enumerate() {
        js[i] = j;
        ws[i] = if total > 0.0 { w / total } else { 0.0 };
    }
    (js, ws)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thigh_binding_moves_to_the_hips_by_its_share() {
        let (joints, weights) = rebound([5, 1, 0, 0], [0.6, 0.4, 0.0, 0.0], &[5], 1, 0.75);
        assert_eq!(joints[..2], [1, 5]);
        assert!((weights[0] - 0.85).abs() < 1e-6 && (weights[1] - 0.15).abs() < 1e-6);
    }

    #[test]
    fn a_vertex_without_the_from_bones_keeps_its_binding() {
        let (joints, weights) = rebound([3, 2, 0, 0], [0.7, 0.3, 0.0, 0.0], &[5], 1, 0.75);
        assert_eq!((joints[..2].to_vec(), weights[..2].to_vec()), (vec![3, 2], vec![0.7, 0.3]));
    }
}
