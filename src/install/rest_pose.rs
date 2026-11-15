//! Rebuilds the rest pose of a GLB model whose bone nodes were flattened to identity transforms.
//!
//! Some converted models (e.g. Fab's "fab-model-conversion" GLBs) keep the joint positions only in the skins' inverse bind matrices, while every bone node carries no transform. Such a model renders in its bind pose, but rotating a bone pivots around the model origin. Each joint node gets the local transform that makes its world matrix the bind pose, X · IBM⁻¹, where X is the world matrix of the skinned mesh node and IBM the joint's inverse bind matrix. Other nodes keep their transforms.

use std::collections::HashMap;

use bevy::math::DMat4;
use serde_json::Value;

use crate::model_edit::gltf::{Glb, inverse_bind_matrices, local_matrix, set_trs};

/// A model with its rest pose rebuilt.
pub struct RestPoseFix {
    /// The repaired GLB file.
    pub glb: Vec<u8>,
    /// How many joint nodes got a transform.
    pub joints: usize,
}

/// The model with its rest pose rebuilt, or `None` if its bone nodes already carry transforms.
pub fn rest_pose_fix(glb: &[u8]) -> Result<Option<RestPoseFix>, String> {
    let Glb { mut gltf, binary } = Glb::parse(glb)?;
    let nodes = gltf["nodes"].as_array().cloned().unwrap_or_default();
    let skins = gltf["skins"].as_array().cloned().unwrap_or_default();
    let joints: Vec<usize> = skins.iter().flat_map(|skin| skin["joints"].as_array().into_iter().flatten()).filter_map(|j| j.as_u64()).map(|j| j as usize).collect();
    if joints.is_empty() || joints.iter().any(|&j| !local_matrix(&nodes[j]).abs_diff_eq(DMat4::IDENTITY, 1e-9)) {
        return Ok(None);
    }

    let mut parents = HashMap::new();
    for (i, node) in nodes.iter().enumerate() {
        for child in node["children"].as_array().into_iter().flatten().filter_map(Value::as_u64) {
            parents.insert(child as usize, i);
        }
    }
    let world = |mut i: usize| {
        let mut matrix = local_matrix(&nodes[i]);
        while let Some(&parent) = parents.get(&i) {
            matrix = local_matrix(&nodes[parent]) * matrix;
            i = parent;
        }
        matrix
    };

    // The first skin that lists a joint sets its bind pose; the skins of the models seen so far agree.
    let mut bind_worlds: HashMap<usize, DMat4> = HashMap::new();
    for (i, node) in nodes.iter().enumerate() {
        let Some(skin) = node["skin"].as_u64().and_then(|s| skins.get(s as usize)) else { continue };
        let mesh_world = world(i);
        let skin_joints = skin["joints"].as_array().into_iter().flatten().filter_map(Value::as_u64);
        for (joint, ibm) in skin_joints.zip(inverse_bind_matrices(&gltf, &binary, skin)?) {
            bind_worlds.entry(joint as usize).or_insert(mesh_world * ibm.inverse());
        }
    }

    // Top-down, so that each parent's new world matrix is known before its children's.
    let mut new_nodes = nodes.clone();
    let mut stack: Vec<(usize, DMat4)> = gltf["scenes"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|scene| scene["nodes"].as_array().cloned().unwrap_or_default())
        .filter_map(|root| root.as_u64())
        .map(|root| (root as usize, DMat4::IDENTITY))
        .collect();
    while let Some((i, parent_world)) = stack.pop() {
        if let Some(bind) = bind_worlds.get(&i) {
            set_trs(&mut new_nodes[i], parent_world.inverse() * *bind);
        }
        let node_world = parent_world * local_matrix(&new_nodes[i]);
        for child in new_nodes[i]["children"].as_array().into_iter().flatten().filter_map(Value::as_u64) {
            stack.push((child as usize, node_world));
        }
    }
    gltf["nodes"] = Value::Array(new_nodes);

    Ok(Some(RestPoseFix { glb: Glb { gltf, binary }.to_bytes(), joints: bind_worlds.len() }))
}

/// Whether the GLB `glb` has a skeleton (a skin); one without is clothing or a rigid model.
pub fn has_skeleton(glb: &[u8]) -> Result<bool, String> {
    Ok(Glb::parse(glb)?.gltf["skins"].as_array().is_some_and(|skins| !skins.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::DVec3;
    use serde_json::json;

    /// A mesh node skinned to one bone node without a transform, whose inverse bind matrix places the bone at (0, 1, 0).
    fn flattened_model() -> Vec<u8> {
        let ibm = DMat4::from_translation(DVec3::new(0.0, -1.0, 0.0));
        let binary: Vec<u8> = ibm.to_cols_array().iter().flat_map(|&v| (v as f32).to_le_bytes()).collect();
        let gltf = json!({
            "scenes": [{ "nodes": [0, 1] }],
            "nodes": [{ "name": "mesh", "skin": 0 }, { "name": "bone" }],
            "skins": [{ "joints": [1], "inverseBindMatrices": 0 }],
            "accessors": [{ "bufferView": 0, "count": 1 }],
            "bufferViews": [{ "buffer": 0, "byteLength": 64 }],
        });
        Glb { gltf, binary }.to_bytes()
    }

    #[test]
    fn a_flattened_bone_gets_its_bind_pose() {
        let fix = rest_pose_fix(&flattened_model()).unwrap().unwrap();
        assert_eq!(fix.joints, 1);
        let bone = &Glb::parse(&fix.glb).unwrap().gltf["nodes"][1];
        assert!(local_matrix(bone).abs_diff_eq(DMat4::from_translation(DVec3::Y), 1e-6));
    }

    #[test]
    fn a_model_with_a_rest_pose_stays_as_it_is() {
        let fix = rest_pose_fix(&flattened_model()).unwrap().unwrap();
        assert!(rest_pose_fix(&fix.glb).unwrap().is_none());
    }
}
