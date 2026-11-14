//! Rebuilds the rest pose of a GLB model whose bone nodes were flattened to identity transforms.
//!
//! Some converted models (e.g. Fab's "fab-model-conversion" GLBs) keep the joint positions only in the skins' inverse bind matrices, while every bone node carries no transform. Such a model renders in its bind pose, but rotating a bone pivots around the model origin. Each joint node gets the local transform that makes its world matrix the bind pose, X · IBM⁻¹, where X is the world matrix of the skinned mesh node and IBM the joint's inverse bind matrix. Other nodes keep their transforms.

use std::collections::HashMap;

use bevy::math::{DMat4, DQuat, DVec3};
use serde_json::{Value, json};

const MAGIC: u32 = 0x4654_6C67;
const JSON_CHUNK: u32 = 0x4E4F_534A;
const BIN_CHUNK: u32 = 0x004E_4942;

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

/// A GLB file's two chunks: the glTF JSON and the binary buffer.
struct Glb {
    gltf: Value,
    binary: Vec<u8>,
}

impl Glb {
    fn parse(bytes: &[u8]) -> Result<Self, String> {
        let word = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or("the file ends early");
        if word(0)? != MAGIC {
            return Err("not a GLB file".into());
        }
        let json_length = word(12)? as usize;
        if word(16)? != JSON_CHUNK {
            return Err("the GLB file does not start with its JSON chunk".into());
        }
        let gltf = serde_json::from_slice(bytes.get(20..20 + json_length).ok_or("the file ends early")?).map_err(|err| err.to_string())?;
        let bin_at = 20 + json_length;
        let binary = match word(bin_at) {
            Ok(length) if word(bin_at + 4)? == BIN_CHUNK => bytes.get(bin_at + 8..bin_at + 8 + length as usize).ok_or("the file ends early")?.to_vec(),
            _ => Vec::new(),
        };
        Ok(Self { gltf, binary })
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut text = serde_json::to_vec(&self.gltf).expect("JSON values serialise");
        text.resize(text.len().next_multiple_of(4), b' ');
        let mut binary = self.binary.clone();
        binary.resize(binary.len().next_multiple_of(4), 0);
        let total = 12 + 8 + text.len() + if binary.is_empty() { 0 } else { 8 + binary.len() };

        let mut bytes = Vec::with_capacity(total);
        for word in [MAGIC, 2, total as u32, text.len() as u32, JSON_CHUNK] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&text);
        if !binary.is_empty() {
            bytes.extend_from_slice(&(binary.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&BIN_CHUNK.to_le_bytes());
            bytes.extend_from_slice(&binary);
        }
        bytes
    }
}

fn floats<const N: usize>(value: &Value) -> Option<[f64; N]> {
    let values: Vec<f64> = value.as_array()?.iter().filter_map(Value::as_f64).collect();
    values.try_into().ok()
}

/// A node's local transform, from its `matrix` or its translation, rotation and scale.
fn local_matrix(node: &Value) -> DMat4 {
    if let Some(matrix) = floats::<16>(&node["matrix"]) {
        return DMat4::from_cols_array(&matrix);
    }
    let translation = floats::<3>(&node["translation"]).map_or(DVec3::ZERO, DVec3::from_array);
    let rotation = floats::<4>(&node["rotation"]).map_or(DQuat::IDENTITY, DQuat::from_array);
    let scale = floats::<3>(&node["scale"]).map_or(DVec3::ONE, DVec3::from_array);
    DMat4::from_scale_rotation_translation(scale, rotation, translation)
}

/// Writes `matrix` (rotation, scale and translation) into the node as translation, rotation and scale.
fn set_trs(node: &mut Value, matrix: DMat4) {
    let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
    let node = node.as_object_mut().expect("glTF nodes are objects");
    node.remove("matrix");
    node.insert("translation".into(), json!(translation.to_array()));
    node.insert("rotation".into(), json!(rotation.normalize().to_array()));
    node.insert("scale".into(), json!(scale.to_array()));
}

fn inverse_bind_matrices(gltf: &Value, binary: &[u8], skin: &Value) -> Result<Vec<DMat4>, String> {
    let Some(accessor) = skin["inverseBindMatrices"].as_u64().map(|a| &gltf["accessors"][a as usize]) else {
        // Without them glTF takes identity matrices.
        return Ok(vec![DMat4::IDENTITY; skin["joints"].as_array().map_or(0, Vec::len)]);
    };
    let view = &gltf["bufferViews"][accessor["bufferView"].as_u64().ok_or("an accessor without a buffer view")? as usize];
    let offset = (view["byteOffset"].as_u64().unwrap_or(0) + accessor["byteOffset"].as_u64().unwrap_or(0)) as usize;
    let count = accessor["count"].as_u64().unwrap_or(0) as usize;
    let bytes = binary.get(offset..offset + count * 64).ok_or("inverse bind matrices beyond the buffer")?;
    let (matrices, _) = bytes.as_chunks::<64>();
    Ok(matrices
        .iter()
        .map(|matrix| {
            let (floats, _) = matrix.as_chunks::<4>();
            DMat4::from_cols_array(&floats.iter().map(|&f| f32::from_le_bytes(f) as f64).collect::<Vec<_>>().try_into().expect("16 floats"))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

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
