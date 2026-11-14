//! A GLB file's glTF JSON and binary buffer, with the node and accessor helpers the install fixes share.

use bevy::math::{DMat4, DQuat, DVec3};
use serde_json::{Value, json};

const MAGIC: u32 = 0x4654_6C67;
const JSON_CHUNK: u32 = 0x4E4F_534A;
const BIN_CHUNK: u32 = 0x004E_4942;

/// A GLB file's two chunks: the glTF JSON and the binary buffer.
pub(super) struct Glb {
    pub(super) gltf: Value,
    pub(super) binary: Vec<u8>,
}

impl Glb {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self, String> {
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

    pub(super) fn to_bytes(&self) -> Vec<u8> {
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

pub(super) fn floats<const N: usize>(value: &Value) -> Option<[f64; N]> {
    let values: Vec<f64> = value.as_array()?.iter().filter_map(Value::as_f64).collect();
    values.try_into().ok()
}

/// A node's local transform, from its `matrix` or its translation, rotation and scale.
pub(super) fn local_matrix(node: &Value) -> DMat4 {
    if let Some(matrix) = floats::<16>(&node["matrix"]) {
        return DMat4::from_cols_array(&matrix);
    }
    let translation = floats::<3>(&node["translation"]).map_or(DVec3::ZERO, DVec3::from_array);
    let rotation = floats::<4>(&node["rotation"]).map_or(DQuat::IDENTITY, DQuat::from_array);
    let scale = floats::<3>(&node["scale"]).map_or(DVec3::ONE, DVec3::from_array);
    DMat4::from_scale_rotation_translation(scale, rotation, translation)
}

/// Writes `matrix` (rotation, scale and translation) into the node as translation, rotation and scale.
pub(super) fn set_trs(node: &mut Value, matrix: DMat4) {
    let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
    let node = node.as_object_mut().expect("glTF nodes are objects");
    node.remove("matrix");
    node.insert("translation".into(), json!(translation.to_array()));
    node.insert("rotation".into(), json!(rotation.normalize().to_array()));
    node.insert("scale".into(), json!(scale.to_array()));
}

pub(super) fn inverse_bind_matrices(gltf: &Value, binary: &[u8], skin: &Value) -> Result<Vec<DMat4>, String> {
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


/// The elements of accessor `index` as floats, `N` components each; integer components are converted as they are, or to 0..1 when the accessor is `normalized`.
pub(super) fn read_accessor<const N: usize>(gltf: &Value, binary: &[u8], index: usize) -> Result<Vec<[f64; N]>, String> {
    let accessor = &gltf["accessors"][index];
    if accessor.get("sparse").is_some() {
        return Err(format!("accessor {index} is sparse"));
    }
    let view = &gltf["bufferViews"][accessor["bufferView"].as_u64().ok_or("an accessor without a buffer view")? as usize];
    let (size, max): (usize, f64) = match accessor["componentType"].as_u64() {
        Some(5121) => (1, 255.0),
        Some(5123) => (2, 65535.0),
        Some(5125) => (4, u32::MAX as f64),
        Some(5126) => (4, 1.0),
        other => return Err(format!("accessor {index}: component type {other:?}")),
    };
    let float = accessor["componentType"].as_u64() == Some(5126);
    let scale = if accessor["normalized"].as_bool().unwrap_or(false) { 1.0 / max } else { 1.0 };
    let offset = (view["byteOffset"].as_u64().unwrap_or(0) + accessor["byteOffset"].as_u64().unwrap_or(0)) as usize;
    let stride = view["byteStride"].as_u64().map_or(size * N, |s| s as usize);
    let count = accessor["count"].as_u64().unwrap_or(0) as usize;
    (0..count)
        .map(|i| {
            let mut element = [0.0; N];
            for (c, value) in element.iter_mut().enumerate() {
                let at = offset + i * stride + c * size;
                let bytes = binary.get(at..at + size).ok_or_else(|| format!("accessor {index} reaches beyond the buffer"))?;
                *value = match (size, float) {
                    (4, true) => f32::from_le_bytes(bytes.try_into().unwrap()) as f64,
                    (4, false) => u32::from_le_bytes(bytes.try_into().unwrap()) as f64 * scale,
                    (2, _) => u16::from_le_bytes(bytes.try_into().unwrap()) as f64 * scale,
                    _ => bytes[0] as f64 * scale,
                };
            }
            Ok(element)
        })
        .collect()
}

/// Appends `bytes` to the binary buffer as a new buffer view and adds an accessor for it; returns the accessor's index.
pub(super) fn append_accessor(glb: &mut Glb, bytes: &[u8], component_type: u32, kind: &str, count: usize) -> usize {
    glb.binary.resize(glb.binary.len().next_multiple_of(4), 0);
    let offset = glb.binary.len();
    glb.binary.extend_from_slice(bytes);
    let views = glb.gltf["bufferViews"].as_array_mut().expect("a model with accessors has buffer views");
    views.push(json!({ "buffer": 0, "byteOffset": offset, "byteLength": bytes.len() }));
    let view = views.len() - 1;
    glb.gltf["buffers"][0]["byteLength"] = json!(glb.binary.len());
    let accessors = glb.gltf["accessors"].as_array_mut().expect("a skinned model has accessors");
    accessors.push(json!({ "bufferView": view, "componentType": component_type, "type": kind, "count": count }));
    accessors.len() - 1
}

/// Each node's parent.
pub(super) fn parents(gltf: &Value) -> std::collections::HashMap<usize, usize> {
    let mut parents = std::collections::HashMap::new();
    for (i, node) in gltf["nodes"].as_array().into_iter().flatten().enumerate() {
        for child in node["children"].as_array().into_iter().flatten().filter_map(Value::as_u64) {
            parents.insert(child as usize, i);
        }
    }
    parents
}

/// The world matrix of node `i` in the rest pose.
pub(super) fn world_matrix(gltf: &Value, parents: &std::collections::HashMap<usize, usize>, mut i: usize) -> DMat4 {
    let mut matrix = local_matrix(&gltf["nodes"][i]);
    while let Some(&parent) = parents.get(&i) {
        matrix = local_matrix(&gltf["nodes"][parent]) * matrix;
        i = parent;
    }
    matrix
}
