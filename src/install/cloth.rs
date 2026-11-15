//! Cloth chains for a VRM avatar whose garment has no working spring bones: `[[cloth]]` in the avatar's mapping file, applied by `riggy install`.
//!
//! Each entry hangs `chains` spring chains around the humanoid bone `bone`, from the height `top` down to the garment's hem, each set just inside the garment. The garment's vertices below `top` are bound to the two nearest chains, blending into their old binding over the first segment, and colliders on thighs and shins (rows of spheres) keep the chains out of the legs. bevy_vrm1 then swings them like hair. What to add depends on how a model was made, so only a model whose own mapping file has entries is changed.

use std::f64::consts::{PI, TAU};

use bevy::math::{DMat4, DVec3, DVec4};
use serde::Deserialize;
use serde_json::{Value, json};

use super::gltf::{Glb, append_accessor, inverse_bind_matrices, parents, read_accessor, world_matrix};

/// One `[[cloth]]` entry; lengths in model units.
#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct ClothSpec {
    /// Start of the names of the garment's materials.
    pub material: String,
    /// VRM humanoid bone the chains hang from.
    pub bone: String,
    /// Height above the floor where the garment starts to swing.
    pub top: f64,
    /// Chains around the body, or across the part `uv` selects.
    pub chains: usize,
    /// Texture rectangle `[u0, v0, u1, v1]` selecting one part of the garment, such as a flap, which then swings on chains of its own; an entry without one takes the rest of the garment.
    pub uv: Option<[f64; 4]>,
    /// Segments per chain.
    pub segments: usize,
    /// Share of the garment's distance from the body's axis at which the chains hang.
    pub inset: f64,
    pub stiffness: f64,
    pub drag: f64,
    pub gravity: f64,
    /// VRM humanoid bones whose binding of the garment moves to `bone`, so the garment's top does not follow them (e.g. the thighs).
    pub release: Vec<String>,
    /// Radius of each chain joint for collisions.
    pub hit_radius: f64,
    /// Radii of the colliders on thighs and shins; 0 adds none.
    pub legs: [f64; 2],
}

impl Default for ClothSpec {
    fn default() -> Self {
        Self { material: String::new(), bone: "hips".into(), top: 0.0, chains: 8, uv: None, release: Vec::new(), segments: 3, inset: 0.85, stiffness: 0.5, drag: 0.3, gravity: 0.3, hit_radius: 0.02, legs: [0.0; 2] }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Mapping {
    cloth: Vec<ClothSpec>,
}

/// The `[[cloth]]` entries of a mapping file's text.
pub fn cloth_specs(mapping: &str) -> Result<Vec<ClothSpec>, String> {
    toml::from_str::<Mapping>(mapping).map(|m| m.cloth).map_err(|err| format!("mapping file: {err}"))
}

/// Prefix of the names of the nodes this module adds; a model that has them is left as it is.
const NODE_PREFIX: &str = "riggy_cloth";

/// The VRM file `bytes` with the cloth chains of `specs` added, or `None` if it has them already.
pub fn add_cloth(bytes: &[u8], specs: &[ClothSpec]) -> Result<Option<Vec<u8>>, String> {
    let mut glb = Glb::parse(bytes)?;
    if glb.gltf["nodes"].as_array().into_iter().flatten().any(|n| n["name"].as_str().is_some_and(|name| name.starts_with(NODE_PREFIX))) {
        return Ok(None);
    }
    let legs = specs.iter().map(|s| s.legs).fold([0.0f64; 2], |a, b| [a[0].max(b[0]), a[1].max(b[1])]);
    let group = add_leg_colliders(&mut glb, legs)?;
    let claimed: Vec<(String, [f64; 4])> = specs.iter().filter_map(|s| s.uv.map(|uv| (s.material.clone(), uv))).collect();
    for (index, spec) in specs.iter().enumerate() {
        add_garment(&mut glb, index, spec, &claimed, group)?;
    }
    Ok(Some(glb.to_bytes()))
}

fn human_bone(gltf: &Value, name: &str) -> Result<usize, String> {
    gltf["extensions"]["VRMC_vrm"]["humanoid"]["humanBones"][name]["node"].as_u64().map(|n| n as usize).ok_or_else(|| format!("the model has no humanoid bone {name}"))
}

fn spring_bone(gltf: &mut Value) -> &mut Value {
    let extension = &mut gltf["extensions"]["VRMC_springBone"];
    if extension.is_null() {
        *extension = json!({ "specVersion": "1.0" });
    }
    for list in ["colliders", "colliderGroups", "springs"] {
        if extension[list].is_null() {
            extension[list] = json!([]);
        }
    }
    extension
}

/// Adds colliders from hip to knee and knee to ankle with radii `radii` as one collider group, each a row of spheres; returns its index.
fn add_leg_colliders(glb: &mut Glb, radii: [f64; 2]) -> Result<Option<usize>, String> {
    if radii == [0.0; 2] {
        return Ok(None);
    }
    let parents = parents(&glb.gltf);
    let mut colliders = Vec::new();
    for side in ["left", "right"] {
        let bones = [human_bone(&glb.gltf, &format!("{side}UpperLeg"))?, human_bone(&glb.gltf, &format!("{side}LowerLeg"))?, human_bone(&glb.gltf, &format!("{side}Foot"))?];
        for (i, radius) in radii.into_iter().enumerate() {
            if radius > 0.0 {
                let tail = world_matrix(&glb.gltf, &parents, bones[i]).inverse().transform_point3(world_matrix(&glb.gltf, &parents, bones[i + 1]).w_axis.truncate());
                // bevy_vrm1 ignores capsule colliders, so a row of spheres a radius apart stands in for one.
                let count = (tail.length() / radius).ceil() as usize;
                for n in 0..=count {
                    let offset = tail * n as f64 / count as f64;
                    colliders.push(json!({ "node": bones[i], "shape": { "sphere": { "offset": offset.to_array(), "radius": radius } } }));
                }
            }
        }
    }
    let extension = spring_bone(&mut glb.gltf);
    let first = extension["colliders"].as_array().map_or(0, Vec::len);
    let count = colliders.len();
    extension["colliders"].as_array_mut().unwrap().extend(colliders);
    let groups = extension["colliderGroups"].as_array_mut().unwrap();
    groups.push(json!({ "name": format!("{NODE_PREFIX}_legs"), "colliders": (first..first + count).collect::<Vec<_>>() }));
    Ok(Some(groups.len() - 1))
}

/// A skinned primitive of the garment: mesh index, primitive index and skin index.
struct Part {
    material: String,
    mesh: usize,
    primitive: usize,
    skin: usize,
}

fn garment_parts(gltf: &Value, material: &str) -> Vec<Part> {
    let materials = gltf["materials"].as_array().cloned().unwrap_or_default();
    let mut parts = Vec::new();
    for node in gltf["nodes"].as_array().into_iter().flatten() {
        let (Some(mesh), Some(skin)) = (node["mesh"].as_u64(), node["skin"].as_u64()) else { continue };
        for (primitive, p) in gltf["meshes"][mesh as usize]["primitives"].as_array().into_iter().flatten().enumerate() {
            let name = p["material"].as_u64().and_then(|m| materials.get(m as usize)).and_then(|m| m["name"].as_str()).unwrap_or("");
            if name.starts_with(material) {
                parts.push(Part { material: name.to_owned(), mesh: mesh as usize, primitive, skin: skin as usize });
            }
        }
    }
    parts
}

/// Whether a vertex of a garment material `material` with texture coordinates `uv` belongs to `spec`, given the parts `claimed` (material, rectangle) by entries with a `uv`.
fn takes(spec: &ClothSpec, claimed: &[(String, [f64; 4])], material: &str, uv: [f64; 2]) -> bool {
    let inside = |r: [f64; 4]| (r[0]..=r[2]).contains(&uv[0]) && (r[1]..=r[3]).contains(&uv[1]);
    match spec.uv {
        Some(rect) => inside(rect),
        None => !claimed.iter().any(|(prefix, rect)| material.starts_with(prefix.as_str()) && inside(*rect)),
    }
}

/// Where the chains hang: `positions[chain][joint]` in world space, top joint first.
struct Chains {
    positions: Vec<Vec<DVec3>>,
    /// Heading of each chain; around the body when `closed`, else across one part.
    angles: Vec<f64>,
    closed: bool,
    center: DVec3,
    top: f64,
    segment: f64,
}

impl Chains {
    /// Chains around the vertical axis through `center`, each just inside the garment whose rest positions are `vertices`.
    fn new(spec: &ClothSpec, center: DVec3, vertices: &[DVec3]) -> Result<Self, String> {
        let hem = vertices.iter().map(|v| v.y).fold(f64::MAX, f64::min);
        let closed = spec.uv.is_none();
        if vertices.is_empty() || spec.top <= hem || spec.chains < if closed { 2 } else { 1 } || spec.segments < 1 {
            return Err(format!("cloth {}: top must lie above the hem ({hem:.3}), with at least 1 segment and 2 chains around the body or 1 across a part", spec.material));
        }
        let segment = (spec.top - hem) / spec.segments as f64;
        // A part spans the headings of its vertices below the top; its chains sit in the middles of equal slices of that span.
        let (start, width) = if closed {
            (0.0, TAU)
        } else {
            let headings: Vec<f64> = vertices.iter().filter(|v| v.y < spec.top).map(|v| heading(*v - center)).collect();
            let low = headings.iter().copied().fold(f64::MAX, f64::min);
            (low, headings.iter().copied().fold(f64::MIN, f64::max) - low)
        };
        let sector = width / spec.chains as f64;
        let angles: Vec<f64> = (0..spec.chains).map(|k| start + if closed { k as f64 } else { k as f64 + 0.5 } * sector).collect();
        let widest = vertices.iter().map(|v| horizontal(*v - center).length()).fold(0.0, f64::max);
        let positions = angles
            .iter()
            .map(|&angle| {
                let mut radius = widest;
                (0..=spec.segments)
                    .map(|i| {
                        let y = spec.top - i as f64 * segment;
                        let near = vertices.iter().filter(|v| (v.y - y).abs() < segment / 2.0 && wrapped(heading(**v - center) - angle).abs() < sector / 2.0);
                        if let Some(r) = near.map(|v| horizontal(*v - center).length()).reduce(f64::max) {
                            radius = r;
                        }
                        DVec3::new(center.x + spec.inset * radius * angle.sin(), y, center.z + spec.inset * radius * angle.cos())
                    })
                    .collect()
            })
            .collect();
        Ok(Self { positions, angles, closed, center, top: spec.top, segment })
    }

    /// Chain and joint indices with weights for a vertex at `v` below the top: the two chains on either side, and on each the two joints above and below it.
    fn binding(&self, v: DVec3) -> [((usize, usize), f64); 4] {
        let chains = self.positions.len();
        let segments = self.positions[0].len() - 1;
        let (k0, a, k1) = if self.closed {
            let around = heading(v - self.center).rem_euclid(TAU) / (TAU / chains as f64);
            (around.floor() as usize % chains, around.fract(), (around.floor() as usize + 1) % chains)
        } else {
            // Across a part: beyond the outer chains a vertex follows the outer one.
            let spacing = if chains > 1 { self.angles[1] - self.angles[0] } else { 1.0 };
            let around = ((heading(v - self.center) - self.angles[0]) / spacing).clamp(0.0, (chains - 1) as f64);
            (around.floor() as usize, around.fract(), (around.floor() as usize + 1).min(chains - 1))
        };
        let down = ((self.top - v.y) / self.segment - 0.5).clamp(0.0, (segments - 1) as f64);
        let (i0, f) = (down.floor() as usize, down.fract());
        let i1 = (i0 + 1).min(segments - 1);
        [((k0, i0), (1.0 - a) * (1.0 - f)), ((k0, i1), (1.0 - a) * f), ((k1, i0), a * (1.0 - f)), ((k1, i1), a * f)]
    }

    /// Share of the chains' binding for a vertex at height `y`: 0 at the top, 1 one segment below.
    fn share(&self, y: f64) -> f64 {
        let x = ((self.top - y) / self.segment).clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    }
}

fn horizontal(v: DVec3) -> DVec3 {
    DVec3::new(v.x, 0.0, v.z)
}

/// Angle about the vertical from +Z towards +X.
fn heading(v: DVec3) -> f64 {
    v.x.atan2(v.z)
}

fn wrapped(radians: f64) -> f64 {
    (radians + PI).rem_euclid(TAU) - PI
}

fn add_garment(glb: &mut Glb, index: usize, spec: &ClothSpec, claimed: &[(String, [f64; 4])], colliders: Option<usize>) -> Result<(), String> {
    let parts = garment_parts(&glb.gltf, &spec.material);
    if parts.is_empty() {
        return Err(format!("cloth: no skinned material whose name starts with {}", spec.material));
    }
    let anchor = human_bone(&glb.gltf, &spec.bone)?;
    let parents = parents(&glb.gltf);
    let anchor_world = world_matrix(&glb.gltf, &parents, anchor);

    // Each skin's bind space → world, from the anchor's inverse bind matrix.
    let mut skins: Vec<usize> = parts.iter().map(|p| p.skin).collect();
    skins.sort_unstable();
    skins.dedup();
    let mut bind_to_world = std::collections::HashMap::new();
    for &skin in &skins {
        let joints = skin_joints(&glb.gltf, skin);
        let slot = joints.iter().position(|&j| j == anchor).ok_or_else(|| format!("cloth: bone {} is not in skin {skin}", spec.bone))?;
        bind_to_world.insert(skin, anchor_world * inverse_bind_matrices(&glb.gltf, &glb.binary, &glb.gltf["skins"][skin])?[slot]);
    }

    let mut vertices = Vec::new();
    for part in &parts {
        let positions: Vec<[f64; 3]> = read_accessor(&glb.gltf, &glb.binary, attribute(&glb.gltf, part, "POSITION")?)?;
        let uvs = texture_coordinates(&glb.gltf, &glb.binary, part, positions.len())?;
        let taken = positions.iter().zip(&uvs).filter(|(_, uv)| takes(spec, claimed, &part.material, **uv));
        vertices.extend(taken.map(|(p, _)| bind_to_world[&part.skin].transform_point3(DVec3::from_array(*p))));
    }
    let chains = Chains::new(spec, anchor_world.w_axis.truncate(), &vertices)?;

    // Joint nodes: the top one a child of the anchor, each further one a child of the one above; all unrotated in world space.
    let mut nodes = vec![Vec::new(); chains.positions.len()];
    for (k, chain) in chains.positions.iter().enumerate() {
        let mut parent = (anchor, anchor_world);
        for (i, &p) in chain.iter().enumerate() {
            let world = DMat4::from_translation(p);
            let local = (parent.1.inverse() * world).to_scale_rotation_translation();
            let list = glb.gltf["nodes"].as_array_mut().unwrap();
            list.push(json!({ "name": format!("{NODE_PREFIX}_{index}_{k}_{i}"), "translation": local.2.to_array(), "rotation": local.1.normalize().to_array() }));
            let node = list.len() - 1;
            let children = &mut glb.gltf["nodes"][parent.0]["children"];
            if children.is_null() {
                *children = json!([]);
            }
            children.as_array_mut().unwrap().push(json!(node));
            nodes[k].push(node);
            parent = (node, world);
        }
    }

    // Skins: the joints that bind vertices (all but each chain's tail), with their inverse bind matrices.
    // Each skin's slots of the released bones and of the anchor.
    let mut released = std::collections::HashMap::new();
    for &skin in &skins {
        let joints = skin_joints(&glb.gltf, skin);
        let slot = |node: usize| joints.iter().position(|&j| j == node).map(|s| s as u16);
        let bones = spec.release.iter().map(|name| human_bone(&glb.gltf, name)).collect::<Result<Vec<_>, _>>()?;
        released.insert(skin, (bones.into_iter().filter_map(slot).collect::<Vec<u16>>(), slot(anchor).expect("the anchor is in every garment skin")));
    }

    let mut slots = std::collections::HashMap::new();
    for &skin in &skins {
        let mut matrices = inverse_bind_matrices(&glb.gltf, &glb.binary, &glb.gltf["skins"][skin])?;
        let mut joints = skin_joints(&glb.gltf, skin);
        for (k, chain) in chains.positions.iter().enumerate() {
            for (i, &p) in chain.iter().enumerate().take(spec.segments) {
                slots.insert((skin, k, i), joints.len());
                joints.push(nodes[k][i]);
                matrices.push(DMat4::from_translation(-p) * bind_to_world[&skin]);
            }
        }
        let bytes: Vec<u8> = matrices.iter().flat_map(|m| m.to_cols_array()).flat_map(|f| (f as f32).to_le_bytes()).collect();
        let accessor = append_accessor(glb, &bytes, 5126, "MAT4", matrices.len());
        glb.gltf["skins"][skin]["joints"] = json!(joints);
        glb.gltf["skins"][skin]["inverseBindMatrices"] = json!(accessor);
    }

    for part in &parts {
        let positions: Vec<[f64; 3]> = read_accessor(&glb.gltf, &glb.binary, attribute(&glb.gltf, part, "POSITION")?)?;
        let old_joints: Vec<[f64; 4]> = read_accessor(&glb.gltf, &glb.binary, attribute(&glb.gltf, part, "JOINTS_0")?)?;
        let old_weights: Vec<[f64; 4]> = read_accessor(&glb.gltf, &glb.binary, attribute(&glb.gltf, part, "WEIGHTS_0")?)?;
        let uvs = texture_coordinates(&glb.gltf, &glb.binary, part, positions.len())?;
        let (mut joint_bytes, mut weight_bytes) = (Vec::new(), Vec::new());
        for (((p, joints), weights), uv) in positions.iter().zip(&old_joints).zip(&old_weights).zip(&uvs) {
            let v = bind_to_world[&part.skin].transform_point3(DVec3::from_array(*p));
            let mut influences: Vec<(u16, f64)> = joints.iter().zip(weights).map(|(&j, &w)| (j as u16, w)).collect();
            let ours = takes(spec, claimed, &part.material, *uv);
            if ours {
                let (bones, anchor) = &released[&part.skin];
                influences.iter_mut().filter(|e| bones.contains(&e.0)).for_each(|e| e.0 = *anchor);
            }
            if v.y < chains.top && ours {
                let share = chains.share(v.y);
                influences.iter_mut().for_each(|e| e.1 *= 1.0 - share);
                influences.extend(chains.binding(v).map(|((k, i), w)| (slots[&(part.skin, k, i)] as u16, w * share)));
            }
            let (joints, weights) = strongest(influences);
            joint_bytes.extend(joints.iter().flat_map(|j| j.to_le_bytes()));
            weight_bytes.extend(weights.to_array().iter().flat_map(|&w| (w as f32).to_le_bytes()));
        }
        let count = positions.len();
        let joints = append_accessor(glb, &joint_bytes, 5123, "VEC4", count);
        let weights = append_accessor(glb, &weight_bytes, 5126, "VEC4", count);
        let attributes = &mut glb.gltf["meshes"][part.mesh]["primitives"][part.primitive]["attributes"];
        attributes["JOINTS_0"] = json!(joints);
        attributes["WEIGHTS_0"] = json!(weights);
    }

    let joint = |node: usize| json!({ "node": node, "hitRadius": spec.hit_radius, "stiffness": spec.stiffness, "gravityPower": spec.gravity, "gravityDir": [0.0, -1.0, 0.0], "dragForce": spec.drag });
    let springs: Vec<Value> = nodes
        .iter()
        .enumerate()
        .map(|(k, chain)| {
            let mut spring = json!({ "name": format!("{NODE_PREFIX}_{index}_{k}"), "joints": chain.iter().map(|&n| joint(n)).collect::<Vec<_>>() });
            if let Some(group) = colliders {
                spring["colliderGroups"] = json!([group]);
            }
            spring
        })
        .collect();
    spring_bone(&mut glb.gltf)["springs"].as_array_mut().unwrap().extend(springs);
    Ok(())
}

/// The texture coordinates of a part's vertices; zeros if it has none.
fn texture_coordinates(gltf: &Value, binary: &[u8], part: &Part, count: usize) -> Result<Vec<[f64; 2]>, String> {
    match attribute(gltf, part, "TEXCOORD_0") {
        Ok(accessor) => read_accessor(gltf, binary, accessor),
        Err(_) => Ok(vec![[0.0; 2]; count]),
    }
}

fn skin_joints(gltf: &Value, skin: usize) -> Vec<usize> {
    gltf["skins"][skin]["joints"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|j| j as usize).collect()
}

fn attribute(gltf: &Value, part: &Part, name: &str) -> Result<usize, String> {
    gltf["meshes"][part.mesh]["primitives"][part.primitive]["attributes"][name].as_u64().map(|a| a as usize).ok_or_else(|| format!("cloth: a garment primitive without {name}"))
}

/// The four heaviest of `influences`, joints merged, with weights that sum to 1.
fn strongest(influences: Vec<(u16, f64)>) -> ([u16; 4], DVec4) {
    let mut merged: Vec<(u16, f64)> = Vec::new();
    for (j, w) in influences {
        match merged.iter_mut().find(|e| e.0 == j) {
            Some(e) => e.1 += w,
            None => merged.push((j, w)),
        }
    }
    merged.sort_by(|a, b| b.1.total_cmp(&a.1));
    merged.truncate(4);
    let total: f64 = merged.iter().map(|e| e.1).sum();
    let (mut joints, mut weights) = ([0u16; 4], DVec4::ZERO);
    for (i, (j, w)) in merged.into_iter().enumerate() {
        joints[i] = j;
        weights[i] = if total > 0.0 { w / total } else { 0.0 };
    }
    (joints, weights)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chains() -> Chains {
        let spec = ClothSpec { top: 1.0, chains: 4, segments: 2, inset: 1.0, ..ClothSpec::default() };
        let ring = |y: f64| (0..16).map(move |i| DVec3::new((i as f64 / 16.0 * TAU).sin() * 0.2, y, (i as f64 / 16.0 * TAU).cos() * 0.2));
        let vertices: Vec<DVec3> = ring(0.4).chain(ring(0.7)).chain(ring(1.0)).collect();
        Chains::new(&spec, DVec3::ZERO, &vertices).unwrap()
    }

    #[test]
    fn the_chains_hang_around_the_garment_down_to_its_hem() {
        let c = chains();
        assert_eq!((c.positions.len(), c.positions[0].len()), (4, 3));
        assert!(c.positions[0][0].abs_diff_eq(DVec3::new(0.0, 1.0, 0.2), 1e-9));
        assert!(c.positions[1][2].abs_diff_eq(DVec3::new(0.2, 0.4, 0.0), 1e-9));
    }

    #[test]
    fn a_vertex_between_two_chains_binds_to_both() {
        let binding = chains().binding(DVec3::new(0.1, 0.85, 0.1));
        let weight = |k: usize| binding.iter().filter(|b| b.0.0 == k).map(|b| b.1).sum::<f64>();
        assert!((weight(0) - 0.5).abs() < 1e-9 && (weight(1) - 0.5).abs() < 1e-9);
        assert!((binding.iter().map(|b| b.1).sum::<f64>() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_chains_take_over_within_the_first_segment() {
        let c = chains();
        assert_eq!((c.share(1.0), c.share(0.7)), (0.0, 1.0));
    }

    #[test]
    fn the_strongest_four_influences_are_kept_and_normalised() {
        let (joints, weights) = strongest(vec![(1, 0.2), (2, 0.2), (1, 0.2), (3, 0.1), (4, 0.05), (5, 0.05)]);
        assert_eq!(joints, [1, 2, 3, 4]);
        assert!((weights.element_sum() - 1.0).abs() < 1e-9 && (weights[0] - 0.4 / 0.75).abs() < 1e-9);
    }
}
