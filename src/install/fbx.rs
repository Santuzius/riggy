//! An FBX animation clip, converted to a GLB that holds only the skeleton and its animation, sampled at the file's frame rate.
//!
//! The GLB is standard glTF 2.0: Y up, metres, the skeleton's nodes under their FBX names with their rest pose, and one animation with a rotation channel per node, plus translation and scale channels where they change. Meshes, such as the stick figure many clip packs show the motion on, are left out. Which node is which humanoid bone is not decided here, so a clip keeps every bone of its source.

use bevy::math::{DQuat, DVec3};
use serde_json::{Value, json};

use crate::model_edit::gltf::{Glb, append_accessor};

const FLOAT: u32 = 5126;
/// Frame rate used when the file states none.
const DEFAULT_FPS: f64 = 30.0;
/// A channel whose samples all stay this close to the rest value is left out.
const STILL: f64 = 1e-5;

/// A node of the clip's skeleton, sampled over the clip.
pub(super) struct Track {
    pub(super) name: String,
    pub(super) parent: Option<usize>,
    pub(super) rest: Pose,
    pub(super) poses: Vec<Pose>,
}

impl Track {
    /// Whether any sample leaves the first one.
    fn moves(&self) -> bool {
        let Some(first) = self.poses.first() else { return false };
        self.poses.iter().any(|p| p.rotation.dot(first.rotation).abs() < 1.0 - STILL || p.translation.distance(first.translation) > STILL || p.scale.distance(first.scale) > STILL)
    }
}

#[derive(Clone, Copy)]
pub(super) struct Pose {
    pub(super) translation: DVec3,
    pub(super) rotation: DQuat,
    pub(super) scale: DVec3,
}

impl From<ufbx::Transform> for Pose {
    fn from(t: ufbx::Transform) -> Self {
        Self {
            translation: DVec3::new(t.translation.x, t.translation.y, t.translation.z),
            rotation: DQuat::from_xyzw(t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w).normalize(),
            scale: DVec3::new(t.scale.x, t.scale.y, t.scale.z),
        }
    }
}

/// The GLB of the first animation in the FBX file `bytes`, the animation named `name`.
pub fn animation_glb(bytes: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let opts = ufbx::LoadOpts {
        target_axes: ufbx::CoordinateAxes::right_handed_y_up(),
        target_unit_meters: 1.0,
        space_conversion: ufbx::SpaceConversion::ModifyGeometry,
        ignore_embedded: true,
        ..Default::default()
    };
    let scene = ufbx::load_memory(bytes, opts).map_err(|err| format!("not a readable FBX file: {}", err.description.as_ref() as &str))?;
    let stack = scene.anim_stacks.first().ok_or("the file holds no animation")?;
    let fps = if scene.settings.frames_per_second > 0.0 { scene.settings.frames_per_second } else { DEFAULT_FPS };
    let frames = ((stack.time_end - stack.time_begin) * fps).round().max(0.0) as usize + 1;
    let times: Vec<f64> = (0..frames).map(|i| i as f64 / fps).collect();

    let skeleton: Vec<&ufbx::Node> = scene.nodes.iter().filter(|node| !node.is_root && node.mesh.is_none()).map(|node| &**node).collect();
    if skeleton.is_empty() {
        return Err("the file holds no skeleton".into());
    }
    let index = |node: &ufbx::Node| skeleton.iter().position(|n| n.element.element_id == node.element.element_id);
    let tracks: Vec<Track> = skeleton
        .iter()
        .map(|node| Track {
            name: node.element.name.to_string(),
            parent: node.parent.as_ref().and_then(|p| index(p)),
            rest: node.local_transform.into(),
            poses: times.iter().map(|t| ufbx::evaluate_transform(&stack.anim, node, stack.time_begin + t).into()).collect(),
        })
        .collect();
    if !tracks.iter().any(Track::moves) {
        return Err("its animation does not move; it may be a character model rather than a clip".into());
    }
    Ok(glb(&tracks, &times, name).to_bytes())
}

pub(super) fn glb(tracks: &[Track], times: &[f64], name: &str) -> Glb {
    let nodes: Vec<Value> = tracks
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let children: Vec<usize> = (0..tracks.len()).filter(|&c| tracks[c].parent == Some(i)).collect();
            let r = track.rest;
            let mut node = json!({
                "name": track.name,
                "translation": r.translation.to_array(),
                "rotation": r.rotation.to_array(),
                "scale": r.scale.to_array(),
            });
            if !children.is_empty() {
                node["children"] = json!(children);
            }
            node
        })
        .collect();
    let roots: Vec<usize> = (0..tracks.len()).filter(|&i| tracks[i].parent.is_none()).collect();
    let mut glb = Glb {
        gltf: json!({
            "asset": { "version": "2.0", "generator": "riggy install" },
            "scene": 0,
            "scenes": [{ "nodes": roots }],
            "nodes": nodes,
            "buffers": [{ "byteLength": 0 }],
            "bufferViews": [],
            "accessors": [],
        }),
        binary: Vec::new(),
    };

    let input = append_accessor(&mut glb, &floats(times.iter().map(|&t| [t])), FLOAT, "SCALAR", times.len());
    let duration = times.last().copied().unwrap_or(0.0);
    glb.gltf["accessors"][input]["min"] = json!([0.0]);
    glb.gltf["accessors"][input]["max"] = json!([duration]);

    let mut channels = Vec::new();
    let mut samplers = Vec::new();
    for (node, track) in tracks.iter().enumerate() {
        let rotations: Vec<[f64; 4]> = track.poses.iter().map(|p| p.rotation.to_array()).collect();
        let translations: Vec<[f64; 3]> = track.poses.iter().map(|p| p.translation.to_array()).collect();
        let scales: Vec<[f64; 3]> = track.poses.iter().map(|p| p.scale.to_array()).collect();
        let mut add = |path: &str, kind: &str, data: Vec<u8>, glb: &mut Glb| {
            let output = append_accessor(glb, &data, FLOAT, kind, times.len());
            samplers.push(json!({ "input": input, "output": output, "interpolation": "LINEAR" }));
            channels.push(json!({ "sampler": samplers.len() - 1, "target": { "node": node, "path": path } }));
        };
        add("rotation", "VEC4", floats(continuous(&rotations).into_iter()), &mut glb);
        if moves(&translations, track.rest.translation.to_array()) {
            add("translation", "VEC3", floats(translations.into_iter()), &mut glb);
        }
        if moves(&scales, track.rest.scale.to_array()) {
            add("scale", "VEC3", floats(scales.into_iter()), &mut glb);
        }
    }
    glb.gltf["animations"] = json!([{ "name": name, "channels": channels, "samplers": samplers }]);
    glb
}

/// Whether any sample leaves `rest`.
fn moves<const N: usize>(samples: &[[f64; N]], rest: [f64; N]) -> bool {
    samples.iter().any(|s| s.iter().zip(rest).any(|(a, b)| (a - b).abs() > STILL))
}

/// The rotations with each one's sign flipped where needed, so neighbouring samples lie on the same side and linear interpolation takes the short way.
fn continuous(rotations: &[[f64; 4]]) -> Vec<[f64; 4]> {
    let mut out: Vec<[f64; 4]> = Vec::with_capacity(rotations.len());
    for &q in rotations {
        let flip = out.last().is_some_and(|p| p.iter().zip(q).map(|(a, b)| a * b).sum::<f64>() < 0.0);
        out.push(if flip { q.map(|c| -c) } else { q });
    }
    out
}

fn floats<const N: usize>(values: impl Iterator<Item = [f64; N]>) -> Vec<u8> {
    values.flat_map(|v| v.into_iter()).flat_map(|c| (c as f32).to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_edit::gltf::read_accessor;

    fn pose(y: f64, angle: f64) -> Pose {
        Pose { translation: DVec3::new(0.0, y, 0.0), rotation: DQuat::from_rotation_y(angle), scale: DVec3::ONE }
    }

    #[test]
    fn a_moving_root_keeps_its_translation_and_a_still_child_only_its_rotation() {
        let tracks = [
            Track { name: "Hips".into(), parent: None, rest: pose(1.0, 0.0), poses: vec![pose(1.0, 0.0), pose(1.1, 0.5)] },
            Track { name: "Spine".into(), parent: Some(0), rest: pose(0.1, 0.0), poses: vec![pose(0.1, 0.0), pose(0.1, 0.2)] },
        ];
        let glb = Glb::parse(&glb(&tracks, &[0.0, 1.0 / 30.0], "Take").to_bytes()).unwrap();
        assert_eq!(glb.gltf["nodes"][0]["children"], json!([1]));
        assert_eq!(glb.gltf["scenes"][0]["nodes"], json!([0]));
        let paths: Vec<(u64, &str)> = glb.gltf["animations"][0]["channels"].as_array().unwrap().iter().map(|c| (c["target"]["node"].as_u64().unwrap(), c["target"]["path"].as_str().unwrap())).collect();
        assert_eq!(paths, [(0, "rotation"), (0, "translation"), (1, "rotation")]);
        let heights = read_accessor::<3>(&glb.gltf, &glb.binary, 2).unwrap();
        assert!((heights[1][1] - 1.1).abs() < 1e-6);
    }

    #[test]
    fn rotations_keep_to_one_side() {
        let q = [0.0, 0.0, 0.0, 1.0];
        assert_eq!(continuous(&[q, q.map(|c| -c)]), [q, q]);
    }
}
