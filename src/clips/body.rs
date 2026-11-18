//! The avatar's body as one shape per humanoid bone, fitted to the mesh vertices that bone moves, for finding where a posed avatar's parts go into each other (a hand into the thigh, a forearm into the chest).
//!
//! Each vertex belongs to the part of its strongest skin joint: that joint's nearest humanoid ancestor, with fingers counted to the hand, eyes and jaw to the head and toes to the foot. A part's shape is star-shaped about the bone's axis: along the axis it is cut into slices, around it into sectors, and each slice and sector keeps the largest distance from the axis of the part's vertices in it. A point lies inside the part when it falls into a slice and is nearer the axis than that radius; its depth is how much nearer. A rounded capsule, as Unreal Engine's physics assets use, would also swallow arms hanging beside a flat chest; slices and sectors follow the chest's real outline.

use std::collections::HashMap;

use bevy::math::{Affine3A, IVec3, Mat4, Vec3};
use serde_json::Value;

use super::rig::Rig;
use crate::model_edit::gltf::{Glb, inverse_bind_matrices, read_accessor};

const SLICES: usize = 16;
const SECTORS: usize = 24;
/// Test points kept per part.
const POINTS: usize = 600;
/// Share of a vertex's skin weight its part must hold for the vertex to be a test point.
const RIGID: f64 = 0.95;

/// Humanoid bones whose vertices count to another part. Each mapped finger bone is a part of its own, so curled fingers are checked where they are.
fn part_name(bone: &str) -> &str {
    for side in ["left", "right"] {
        if let Some(rest) = bone.strip_prefix(side) {
            if rest == "Toes" {
                return if side == "left" { "leftFoot" } else { "rightFoot" };
            }
            if rest == "Eye" {
                return "head";
            }
        }
    }
    if bone == "jaw" { "head" } else { bone }
}

/// The bones a part's axis points to, first found wins; without one the axis points to the part's centroid.
fn axis_targets(part: &str) -> &'static [&'static str] {
    match part {
        "hips" => &["spine", "chest", "upperChest"],
        "spine" => &["chest", "upperChest", "neck"],
        "chest" => &["upperChest", "neck"],
        "upperChest" => &["neck", "head"],
        "neck" => &["head"],
        "leftShoulder" => &["leftUpperArm"],
        "rightShoulder" => &["rightUpperArm"],
        "leftUpperArm" => &["leftLowerArm"],
        "rightUpperArm" => &["rightLowerArm"],
        "leftLowerArm" => &["leftHand"],
        "rightLowerArm" => &["rightHand"],
        "leftUpperLeg" => &["leftLowerLeg"],
        "rightUpperLeg" => &["rightLowerLeg"],
        "leftLowerLeg" => &["leftFoot"],
        "rightLowerLeg" => &["rightFoot"],
        _ => &[],
    }
}

/// Parts that move together through the trunk and may press into each other by bending: their pairs are not checked.
const TRUNK: [&str; 8] = ["hips", "spine", "chest", "upperChest", "neck", "head", "leftShoulder", "rightShoulder"];

/// Grid cell size and the farthest distance to a part's vertex that counts, in metres.
const CELL: f32 = 0.02;
const SEARCH: f32 = 0.06;
/// Length, in metres, over which a part's smooth shape fades out at its ends.
const TAPER: f32 = 0.03;

/// The star shape's weight against the nearby vertices' (each up to 1), which only counts where hardly a vertex is near.
const STAR_WEIGHT: f32 = 0.02;

fn cell(p: Vec3) -> IVec3 {
    (p / CELL).floor().as_ivec3()
}

/// Parts whose shape is too coarse to collide with: only their points are checked.
const HOLLOW: [&str; 4] = ["leftHand", "rightHand", "leftFoot", "rightFoot"];

/// Whether the part is a finger bone's.
fn is_finger(name: &str) -> bool {
    ["left", "right"].iter().filter_map(|side| name.strip_prefix(side)).any(|rest| ["Thumb", "Index", "Middle", "Ring", "Little"].iter().any(|f| rest.starts_with(f)))
}

/// One body part in its bone's rest frame.
#[derive(Debug, Clone)]
pub struct Part {
    pub name: String,
    /// The humanoid bone's node, whose world transform moves the part.
    pub node: usize,
    /// The part's parent part, if any.
    pub parent: Option<usize>,
    /// The axis in the bone's frame: origin and unit direction.
    origin: Vec3,
    axis: Vec3,
    /// A unit vector across the axis, where sector 0 starts.
    across: Vec3,
    /// Range along the axis covered by the slices.
    from: f32,
    to: f32,
    /// Radius per slice and sector.
    radii: Vec<[f32; SECTORS]>,
    /// Test points on the part's surface, in the bone's frame.
    pub points: Vec<Vec3>,
    /// The part's vertices with their outward normals, in the bone's frame, and a grid of them for finding the nearest.
    surface: Vec<(Vec3, Vec3)>,
    grid: HashMap<IVec3, Vec<u32>>,
}

impl Part {
    /// How deep the bone-frame point `p` lies inside the part, in metres; `None` outside. Inside the star shape, the point is measured against the nearest vertex of the part: behind it along its normal by the depth. A point farther than [`SEARCH`] from every vertex lies deep inside and keeps the star's depth.
    pub fn depth(&self, p: Vec3) -> Option<f32> {
        self.contact(p).map(|(depth, _)| depth)
    }

    /// [`Part::depth`] with the outward direction of the surface there (bone frame): the nearest vertex's normal, or straight out from the axis deep inside.
    pub fn contact(&self, p: Vec3) -> Option<(f32, Vec3)> {
        let star = self.star_depth(p)?;
        let outward = || {
            let d = p - self.origin;
            (d - self.axis * d.dot(self.axis)).normalize_or(self.across)
        };
        if self.surface.is_empty() {
            return Some((star, outward()));
        }
        // The surface near `p`: every vertex within [`SEARCH`], weighted down to nothing at that distance, so depth and direction change smoothly as `p` moves past sparse vertices. Deep inside, where no vertex is near, the star's depth and the direction away from the axis take over, also smoothly.
        let (mut behind, mut normal, mut weight) = (STAR_WEIGHT * star, STAR_WEIGHT * outward(), STAR_WEIGHT);
        self.near(p, |v, n, d| {
            let w = (1.0 - d / SEARCH).powi(2);
            behind += w * -(p - v).dot(n);
            normal += w * n;
            weight += w;
        });
        let behind = behind / weight;
        (behind > 0.0).then_some((behind, normal.normalize_or(outward())))
    }

    /// Calls `visit` with every vertex within [`SEARCH`] of `p`: position, normal and distance.
    fn near(&self, p: Vec3, mut visit: impl FnMut(Vec3, Vec3, f32)) {
        let c = cell(p);
        let reach = (SEARCH / CELL).ceil() as i32;
        for x in -reach..=reach {
            for y in -reach..=reach {
                for z in -reach..=reach {
                    for &i in self.grid.get(&(c + IVec3::new(x, y, z))).into_iter().flatten() {
                        let (v, n) = self.surface[i as usize];
                        let d = v.distance(p);
                        if d < SEARCH {
                            visit(v, n, d);
                        }
                    }
                }
            }
        }
    }

    /// For checking: the star's depth of `p`, and the distance to the nearest vertex within [`SEARCH`] with how far `p` lies behind it.
    pub fn explain(&self, p: Vec3) -> (Option<f32>, Option<(f32, f32)>) {
        (self.star_depth(p), self.nearest(p).map(|(v, n)| (v.distance(p), -(p - v).dot(n))))
    }

    /// The part's vertices, in the bone's frame.
    pub fn vertices(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.surface.iter().map(|&(v, _)| v)
    }

    /// Number of surface vertices.
    pub fn surface_len(&self) -> usize {
        self.surface.len()
    }

    /// The vertex nearest to `p` within [`SEARCH`], with its normal.
    fn nearest(&self, p: Vec3) -> Option<(Vec3, Vec3)> {
        let c = cell(p);
        let reach = (SEARCH / CELL).ceil() as i32;
        let mut best: Option<(f32, usize)> = None;
        for x in -reach..=reach {
            for y in -reach..=reach {
                for z in -reach..=reach {
                    for &i in self.grid.get(&(c + IVec3::new(x, y, z))).into_iter().flatten() {
                        let d = self.surface[i as usize].0.distance_squared(p);
                        if d <= SEARCH * SEARCH && best.is_none_or(|b| d < b.0) {
                            best = Some((d, i as usize));
                        }
                    }
                }
            }
        }
        best.map(|(_, i)| self.surface[i])
    }

    /// How deep the bone-frame point `p` lies inside the star shape (slices and sectors), in metres; `None` outside.
    fn star_depth(&self, p: Vec3) -> Option<f32> {
        let d = p - self.origin;
        let t = d.dot(self.axis);
        if t < self.from || t > self.to {
            return None;
        }
        let slice = (((t - self.from) / (self.to - self.from)) * SLICES as f32).min(SLICES as f32 - 1.0) as usize;
        let radial = d - self.axis * t;
        let r = radial.length();
        let radius = self.radii[slice][self.sector(radial)];
        (r < radius).then_some(radius - r)
    }

    /// How deep the bone-frame point `p` lies in the part's smooth shape, and the direction out of it (bone frame); `None` outside. The smooth shape interpolates the star's radii between slice and sector centres, and tapers to nothing at the ends of the slices, so depth and direction change continuously with `p` and the direction points away from the axis however deep `p` lies. For pushing points out; [`Part::depth`] measures more exactly near the surface.
    pub fn field(&self, p: Vec3) -> Option<(f32, Vec3)> {
        let d = p - self.origin;
        let t = d.dot(self.axis);
        if t <= self.from || t >= self.to {
            return None;
        }
        let radial = d - self.axis * t;
        let r = radial.length();
        let u = ((t - self.from) / (self.to - self.from) * SLICES as f32 - 0.5).clamp(0.0, SLICES as f32 - 1.0);
        let side = self.axis.cross(self.across);
        let angle = radial.dot(side).atan2(radial.dot(self.across));
        let v = (angle + std::f32::consts::PI) / std::f32::consts::TAU * SECTORS as f32 - 0.5;
        let (u0, v0) = (u.floor() as usize, v.floor());
        let (fu, fv) = (u - u0 as f32, v - v0);
        let u1 = (u0 + 1).min(SLICES - 1);
        let wrap = |s: f32| (s as i32).rem_euclid(SECTORS as i32) as usize;
        let (s0, s1) = (wrap(v0), wrap(v0 + 1.0));
        let radius = (1.0 - fu) * ((1.0 - fv) * self.radii[u0][s0] + fv * self.radii[u0][s1]) + fu * ((1.0 - fv) * self.radii[u1][s0] + fv * self.radii[u1][s1]);
        let sideways = radius - r;
        let along = (t - self.from).min(self.to - t);
        if sideways <= 0.0 {
            return None;
        }
        // The way out is always away from the axis; towards an end of the part the depth fades out over [`TAPER`], so a point passing an end leaves without a jump.
        let fade = (along / TAPER).min(1.0);
        Some((sideways * fade * fade * (3.0 - 2.0 * fade), radial.normalize_or(self.across)))
    }

    /// Name, axis range and largest radius per slice, in centimetres, for checking the shapes by eye.
    pub fn summary(&self) -> String {
        let radii: Vec<String> = self.radii.iter().map(|s| format!("{:.0}", s.iter().copied().fold(0.0, f32::max) * 100.0)).collect();
        format!("{}: {} points, axis {:.2?}, {:.0}..{:.0} cm, max radius per slice [{}]", self.name, self.points.len(), self.axis, self.from * 100.0, self.to * 100.0, radii.join(" "))
    }

    /// Where the bone-frame point `p` lies along the part: 0 at the bone's joint, 1 at the far end of the slices.
    pub fn along(&self, p: Vec3) -> f32 {
        (p - self.origin).dot(self.axis) / self.to.max(1e-4)
    }

    fn sector(&self, radial: Vec3) -> usize {
        let side = self.axis.cross(self.across);
        let angle = radial.dot(side).atan2(radial.dot(self.across));
        (((angle + std::f32::consts::PI) / std::f32::consts::TAU * SECTORS as f32) as usize).min(SECTORS - 1)
    }
}

/// The avatar's body parts.
#[derive(Debug, Clone)]
pub struct Body {
    pub parts: Vec<Part>,
}

impl Body {
    /// The parts of the avatar `bytes` (VRM or GLB) on `rig`, which must be read from the same file.
    pub fn new(bytes: &[u8], rig: &Rig) -> Result<Self, String> {
        let glb = Glb::parse(bytes)?;
        let rest = rig.worlds(&HashMap::new());

        // Each node's part: its own humanoid bone's, else its nearest humanoid ancestor's.
        let bone_names: HashMap<usize, &str> = rig.humanoid.iter().map(|(name, &node)| (node, name.as_str())).collect();
        let mut names: Vec<String> = Vec::new();
        let mut part_nodes: Vec<usize> = Vec::new();
        let mut part_of_name: HashMap<String, usize> = HashMap::new();
        for (name, &node) in &rig.humanoid {
            if part_name(name) == name && !part_of_name.contains_key(name.as_str()) {
                part_of_name.insert(name.clone(), names.len());
                names.push(name.clone());
                part_nodes.push(node);
            }
        }
        let node_part = |mut node: usize| -> Option<usize> {
            loop {
                if let Some(name) = bone_names.get(&node) {
                    return part_of_name.get(part_name(name)).copied();
                }
                node = rig.parents[node]?;
            }
        };

        // Spring-bone chains (hair, coat skirts, bust) swing aside by themselves: their vertices are neither obstacles nor test points.
        let mut soft: std::collections::HashSet<usize> = glb.gltf["extensions"]["VRMC_springBone"]["springs"].as_array().into_iter().flatten().flat_map(|s| s["joints"].as_array().into_iter().flatten()).filter_map(|j| j["node"].as_u64()).map(|n| n as usize).collect();
        for node in 0..rig.parents.len() {
            let mut up = rig.parents[node];
            while let Some(n) = up {
                if soft.contains(&n) {
                    soft.insert(node);
                    break;
                }
                up = rig.parents[n];
            }
        }

        // Every vertex in the rest pose, with its part.
        let mut vertices: Vec<Vec<(Vec3, Option<Vec3>, bool)>> = vec![Vec::new(); names.len()];
        for (node_index, node) in glb.gltf["nodes"].as_array().into_iter().flatten().enumerate() {
            let Some(mesh) = node["mesh"].as_u64() else { continue };
            let skin = node["skin"].as_u64().map(|s| &glb.gltf["skins"][s as usize]);
            let (joints, binds) = match skin {
                Some(skin) => {
                    let joints: Vec<usize> = skin["joints"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|j| j as usize).collect();
                    let binds: Vec<Affine3A> = inverse_bind_matrices(&glb.gltf, &glb.binary, skin)?.iter().map(|m| Affine3A::from_mat4(m.as_mat4())).collect();
                    (joints, binds)
                }
                None => (Vec::new(), Vec::new()),
            };
            for primitive in glb.gltf["meshes"][mesh as usize]["primitives"].as_array().into_iter().flatten() {
                let attributes = &primitive["attributes"];
                let Some(position) = attributes["POSITION"].as_u64() else { continue };
                let positions = read_accessor::<3>(&glb.gltf, &glb.binary, position as usize)?;
                let normals = match attributes["NORMAL"].as_u64() {
                    Some(n) => read_accessor::<3>(&glb.gltf, &glb.binary, n as usize)?,
                    None => Vec::new(),
                };
                let skinning = match (skin, attributes["JOINTS_0"].as_u64(), attributes["WEIGHTS_0"].as_u64()) {
                    (Some(_), Some(j), Some(w)) => Some((read_accessor::<4>(&glb.gltf, &glb.binary, j as usize)?, read_accessor::<4>(&glb.gltf, &glb.binary, w as usize)?)),
                    _ => None,
                };
                for (i, p) in positions.iter().enumerate() {
                    let p = Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
                    let n = normals.get(i).map(|n| Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32));
                    // The part of the strongest joint, and how much of the vertex's weight lies on that part's joints: 1 where the part alone moves it.
                    let world = match &skinning {
                        Some((js, ws)) => {
                            let mut matrix = Mat4::ZERO;
                            let mut by_part: Vec<(Option<usize>, f64)> = Vec::new();
                            let mut strongest = (None, -1.0f64);
                            let mut strongest_joint = 0;
                            for k in 0..4 {
                                let (j, w) = (js[i][k] as usize, ws[i][k]);
                                if w <= 0.0 || j >= joints.len() {
                                    continue;
                                }
                                matrix += Mat4::from(rest[joints[j]] * binds[j]) * w as f32;
                                let part = node_part(joints[j]);
                                match by_part.iter_mut().find(|e| e.0 == part) {
                                    Some(e) => e.1 += w,
                                    None => by_part.push((part, w)),
                                }
                                if w > strongest.1 {
                                    strongest = (part, w);
                                    strongest_joint = joints[j];
                                }
                            }
                            if soft.contains(&strongest_joint) {
                                continue;
                            }
                            let total: f64 = by_part.iter().map(|e| e.1).sum();
                            let share = by_part.iter().find(|e| e.0 == strongest.0).map_or(0.0, |e| e.1 / total.max(1e-9));
                            (matrix.transform_point3(p), n.map(|n| matrix.transform_vector3(n).normalize_or_zero()), strongest.0, share)
                        }
                        None => (rest[node_index].transform_point3(p), n.map(|n| rest[node_index].transform_vector3(n).normalize_or_zero()), node_part(node_index), 1.0),
                    };
                    let (world, normal, owner, share) = (world.0, world.1, world.2, world.3);
                    if let Some(part) = owner {
                        vertices[part].push((world, normal, share >= RIGID));
                    }
                }
            }
        }

        let positions: HashMap<&str, Vec3> = rig.humanoid.iter().map(|(name, &node)| (name.as_str(), rest[node].translation.into())).collect();
        let mut parts = Vec::new();
        for (index, name) in names.iter().enumerate() {
            let node = part_nodes[index];
            let frame = rigid(rest[node]);
            let to_bone = frame.inverse();
            let local: Vec<Vec3> = vertices[index].iter().map(|&(v, _, _)| to_bone.transform_point3(v)).collect();
            if local.len() < 8 {
                continue;
            }
            let origin = Vec3::ZERO;
            let target = axis_targets(name).iter().find_map(|t| positions.get(t)).map(|&p| to_bone.transform_point3(p));
            let centroid = local.iter().copied().sum::<Vec3>() / local.len() as f32;
            let axis = target.unwrap_or(centroid).normalize_or(Vec3::Y);
            let across = axis.any_orthonormal_vector();
            let ts: Vec<f32> = local.iter().map(|v| (*v - origin).dot(axis)).collect();
            let from = ts.iter().copied().fold(f32::MAX, f32::min);
            let to = ts.iter().copied().fold(f32::MIN, f32::max);
            let mut part = Part { name: name.clone(), node, parent: None, origin, axis, across, from, to: to.max(from + 1e-4), radii: vec![[0.0; SECTORS]; SLICES], points: Vec::new(), surface: Vec::new(), grid: HashMap::new() };
            part.surface = local.iter().zip(&vertices[index]).filter_map(|(&v, &(_, n, _))| Some((v, to_bone.transform_vector3(n?).normalize_or_zero()))).collect();
            for (i, &(v, _)) in part.surface.iter().enumerate() {
                part.grid.entry(cell(v)).or_default().push(i as u32);
            }
            for (v, &t) in local.iter().zip(&ts) {
                let slice = (((t - part.from) / (part.to - part.from)) * SLICES as f32).min(SLICES as f32 - 1.0) as usize;
                let radial = *v - origin - axis * t;
                let sector = part.sector(radial);
                part.radii[slice][sector] = part.radii[slice][sector].max(radial.length());
            }
            fill_gaps(&mut part.radii);
            // Test points only where the part alone moves the skin: near a joint the skin blends between bones, and a point carried rigidly by one bone would sink into its neighbour.
            let rigid_points: Vec<Vec3> = local.iter().zip(&vertices[index]).filter(|(_, (_, _, rigid))| *rigid).map(|(v, _)| *v).collect();
            let step = rigid_points.len().div_ceil(POINTS).max(1);
            part.points = rigid_points.into_iter().step_by(step).collect();
            parts.push(part);
        }

        // Each part's parent part: the part of the nearest humanoid ancestor of its node.
        let by_node: HashMap<usize, usize> = parts.iter().enumerate().map(|(i, p)| (p.node, i)).collect();
        for i in 0..parts.len() {
            let mut node = rig.parents[parts[i].node];
            while let Some(n) = node {
                if let Some(&p) = by_node.get(&n) {
                    parts[i].parent = Some(p);
                    break;
                }
                node = rig.parents[n];
            }
        }
        Ok(Self { parts })
    }

    /// Pairs of parts to check: not neighbours in the skeleton and not both in the trunk.
    pub fn pairs(&self) -> Vec<(usize, usize)> {
        let mut pairs = Vec::new();
        for a in 0..self.parts.len() {
            for b in a + 1..self.parts.len() {
                let (pa, pb) = (&self.parts[a], &self.parts[b]);
                let neighbours = pa.parent == Some(b) || pb.parent == Some(a);
                let trunk = TRUNK.contains(&pa.name.as_str()) && TRUNK.contains(&pb.name.as_str());
                if !neighbours && !trunk {
                    pairs.push((a, b));
                }
            }
        }
        pairs
    }

    /// Every point of part `points` inside part `shape`'s smooth shape ([`Part::field`]) at `worlds`, deeper than it lies there in the rest pose `rest`: avatar-space position, outward direction and depth beyond the rest pose.
    pub fn pushes(&self, points: usize, shape: usize, worlds: &[Affine3A], rest: &[Affine3A]) -> Vec<(Vec3, Vec3, f32)> {
        let (p, s) = (&self.parts[points], &self.parts[shape]);
        if HOLLOW.contains(&s.name.as_str()) || is_finger(&s.name) {
            return Vec::new();
        }
        let (shape_world, point_world) = (rigid(worlds[s.node]), rigid(worlds[p.node]));
        let posed = shape_world.inverse() * point_world;
        let still = rigid(rest[s.node]).inverse() * rigid(rest[p.node]);
        p.points
            .iter()
            .filter_map(|&q| {
                let (depth, out) = s.field(posed.transform_point3(q))?;
                let before = s.field(still.transform_point3(q)).map_or(0.0, |f| f.0);
                (depth > before).then(|| (point_world.transform_point3(q), shape_world.transform_vector3(out), depth - before))
            })
            .collect()
    }

    /// For each pair of [`Body::pairs`], the deepest point of either part inside the other with every node posed at `worlds`, beyond how deep it lies in the rest pose `rest`; in metres, 0 when apart.
    pub fn penetrations(&self, pairs: &[(usize, usize)], worlds: &[Affine3A], rest: &[Affine3A]) -> Vec<Penetration> {
        pairs
            .iter()
            .map(|&(a, b)| {
                let one = self.deepest(a, b, worlds, rest);
                let other = self.deepest(b, a, worlds, rest);
                if one.depth >= other.depth { one } else { other }
            })
            .collect()
    }

    /// For checking: every point of part `points` inside part `shape` at `worlds`, with [`Part::explain`]'s values.
    pub fn explain(&self, points: usize, shape: usize, worlds: &[Affine3A]) -> Vec<(Vec3, Option<f32>, Option<(f32, f32)>)> {
        let (p, s) = (&self.parts[points], &self.parts[shape]);
        let posed = rigid(worlds[s.node]).inverse() * rigid(worlds[p.node]);
        p.points.iter().map(|&q| posed.transform_point3(q)).filter_map(|q| { let (star, near) = s.explain(q); star.map(|_| (q, star, near)) }).collect()
    }

    fn deepest(&self, points: usize, shape: usize, worlds: &[Affine3A], rest: &[Affine3A]) -> Penetration {
        let (p, s) = (&self.parts[points], &self.parts[shape]);
        // Spread fingers and toes fill a star about the bone's axis far beyond the hand or foot: these parts only touch others with their points.
        if HOLLOW.contains(&s.name.as_str()) || is_finger(&s.name) {
            return Penetration { depth: 0.0, point: points, shape, at: Vec3::ZERO, along: 0.0, normal: Vec3::ZERO };
        }
        let posed = rigid(worlds[s.node]).inverse() * rigid(worlds[p.node]);
        let still = rigid(rest[s.node]).inverse() * rigid(rest[p.node]);
        let mut worst = Penetration { depth: 0.0, point: points, shape, at: Vec3::ZERO, along: 0.0, normal: Vec3::ZERO };
        for &q in &p.points {
            let Some((depth, normal)) = s.contact(posed.transform_point3(q)) else { continue };
            let before = s.depth(still.transform_point3(q)).unwrap_or(0.0).max(0.0);
            if depth - before > worst.depth {
                worst = Penetration { depth: depth - before, point: points, shape, at: rigid(worlds[p.node]).transform_point3(q), along: p.along(q), normal: rigid(worlds[s.node]).transform_vector3(normal) };
            }
        }
        worst
    }
}

/// How far part `point` goes into part `shape`, and where (avatar space).
#[derive(Debug, Clone, Copy)]
pub struct Penetration {
    pub depth: f32,
    pub point: usize,
    pub shape: usize,
    pub at: Vec3,
    /// Where along its own part the point lies: 0 at the part's joint, 1 at its far end.
    pub along: f32,
    /// The outward direction of `shape`'s surface there (avatar space).
    pub normal: Vec3,
}

/// `transform` without its scale: a bone's frame in metres, whatever units the model's nodes use.
pub fn rigid(transform: Affine3A) -> Affine3A {
    let (_, rotation, translation) = transform.to_scale_rotation_translation();
    Affine3A::from_rotation_translation(rotation, translation)
}

/// Gives each empty sector the larger radius of its nearest filled neighbours in the same slice, so a sparse slice has no holes.
fn fill_gaps(radii: &mut [[f32; SECTORS]]) {
    for slice in radii.iter_mut() {
        if slice.iter().all(|&r| r == 0.0) {
            continue;
        }
        let filled = *slice;
        for s in 0..SECTORS {
            if filled[s] > 0.0 {
                continue;
            }
            let near = |step: isize| (1..SECTORS as isize).map(|k| filled[(s as isize + step * k).rem_euclid(SECTORS as isize) as usize]).find(|&r| r > 0.0).unwrap_or(0.0);
            slice[s] = near(1).min(near(-1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cylinder() -> Part {
        Part { name: "test".into(), node: 0, parent: None, origin: Vec3::ZERO, axis: Vec3::Y, across: Vec3::X, from: 0.0, to: 1.0, radii: vec![[0.1; SECTORS]; SLICES], points: Vec::new(), surface: Vec::new(), grid: HashMap::new() }
    }

    #[test]
    fn a_point_inside_a_part_has_its_distance_to_the_surface_as_depth() {
        let depth = cylinder().depth(Vec3::new(0.04, 0.5, 0.0)).unwrap();
        assert!((depth - 0.06).abs() < 1e-5);
    }

    #[test]
    fn a_point_beyond_the_slices_or_the_radius_is_outside() {
        assert!(cylinder().depth(Vec3::new(0.0, 1.2, 0.0)).is_none());
        assert!(cylinder().depth(Vec3::new(0.0, 0.5, 0.15)).is_none());
    }

    #[test]
    fn toes_count_to_the_foot_and_the_jaw_to_the_head() {
        assert_eq!(part_name("leftIndexProximal"), "leftIndexProximal");
        assert!(is_finger("leftIndexProximal"));
        assert_eq!(part_name("rightToes"), "rightFoot");
        assert_eq!(part_name("jaw"), "head");
        assert_eq!(part_name("leftUpperArm"), "leftUpperArm");
    }
}
