//! Clothing: a GLB without a skeleton, worn by a skinned avatar. Each vertex of the clothing follows the bones of the nearest vertex of the avatar's body, so the clothing moves with it.

use std::collections::HashMap;
use std::path::Path;

use bevy::asset::AssetPath;
use bevy::asset::io::AssetSourceId;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::math::Affine3A;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Mesh, VertexAttributeValues};
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use serde::Deserialize;

use super::avatar_space;
use super::glb::{GlbRig, SOURCE, TransformTree};

/// Most joints one skinned mesh may have in Bevy.
const MAX_JOINTS: usize = 256;

/// Edge of a cell of the grid that finds the nearest body vertex, in model units of the avatar.
const CELL: f32 = 30.0;

/// One piece of clothing in the mapping file (`[[clothing]]`).
#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct ClothingSpec {
    /// File in the model's own folder (`models/NAME/`).
    pub file: String,
    /// Factor from the clothing's units to metres.
    pub scale: f32,
    /// Shift in avatar space, in model units of the avatar.
    pub offset: [f32; 3],
}

impl Default for ClothingSpec {
    fn default() -> Self {
        Self { file: String::new(), scale: 1.0, offset: [0.0; 3] }
    }
}

/// Root of a piece of clothing, child of the avatar root.
#[derive(Component)]
struct Clothing;

/// Spawns the clothing of `specs` on the avatar `root` of model `model`, whose units are `avatar_scale` metres.
pub fn spawn_clothing(specs: &[ClothingSpec], model: &str, avatar_scale: f32, root: Entity, commands: &mut Commands, asset_server: &AssetServer) {
    for spec in specs {
        let path = AssetPath::from_path(&Path::new(model).join(&spec.file)).into_owned().with_source(AssetSourceId::from(SOURCE));
        commands
            .spawn((
                Clothing,
                Name::new(format!("clothing {}", spec.file)),
                Transform::from_translation(Vec3::from(spec.offset)).with_scale(Vec3::splat(spec.scale / avatar_scale)),
                WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset(path))),
                ChildOf(root),
            ))
            .observe(fit_clothing);
    }
}

/// A body vertex in the rest pose: its avatar-space position and its bones as indices into [`Bones`].
#[derive(Clone, Copy)]
struct BodyVertex {
    position: Vec3,
    joints: [u16; 4],
    weights: [f32; 4],
}

/// The joints of all body skins in one list, each with its inverse bind matrix and its avatar-space rest transform.
#[derive(Default)]
struct Bones {
    joints: Vec<Entity>,
    inverse_bindposes: Vec<Mat4>,
    rest: Vec<Mat4>,
}

impl Bones {
    /// Matrix from bind space to avatar space for a vertex with these bones, in the rest pose.
    fn skinning(&self, joints: [u16; 4], weights: [f32; 4]) -> Mat4 {
        joints.iter().zip(weights).map(|(&j, w)| self.rest[j as usize] * self.inverse_bindposes[j as usize] * w).fold(Mat4::ZERO, |sum, m| sum + m)
    }
}

/// Grid over the body vertices for nearest-vertex queries.
struct BodyGrid {
    vertices: Vec<BodyVertex>,
    cells: HashMap<IVec3, Vec<usize>>,
}

impl BodyGrid {
    fn new(vertices: Vec<BodyVertex>) -> Self {
        let mut cells: HashMap<IVec3, Vec<usize>> = HashMap::new();
        for (i, v) in vertices.iter().enumerate() {
            cells.entry(cell(v.position)).or_default().push(i);
        }
        Self { vertices, cells }
    }

    fn nearest(&self, position: Vec3) -> Option<&BodyVertex> {
        let center = cell(position);
        let mut best: Option<(f32, usize)> = None;
        for ring in 0..64_i32 {
            if let Some((distance, _)) = best
                && (ring as f32 - 1.0) * CELL > distance
            {
                break;
            }
            for x in -ring..=ring {
                for y in -ring..=ring {
                    for z in -ring..=ring {
                        if x.abs().max(y.abs()).max(z.abs()) != ring {
                            continue;
                        }
                        for &i in self.cells.get(&(center + IVec3::new(x, y, z))).into_iter().flatten() {
                            let distance = self.vertices[i].position.distance(position);
                            if best.is_none_or(|(d, _)| distance < d) {
                                best = Some((distance, i));
                            }
                        }
                    }
                }
            }
        }
        best.map(|(_, i)| &self.vertices[i])
    }
}

fn cell(position: Vec3) -> IVec3 {
    (position / CELL).floor().as_ivec3()
}

/// Binds the clothing's meshes to the body's bones once the clothing has loaded. Their bounding boxes stay those of the unbound meshes, so culling is off for them.
fn fit_clothing(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    parents: Query<&ChildOf>,
    rigs: Query<&GlbRig>,
    children: Query<&Children>,
    skins: Query<(&Mesh3d, &SkinnedMesh)>,
    surfaces: Query<&Mesh3d, Without<SkinnedMesh>>,
    tree: TransformTree,
    mut meshes: ResMut<Assets<Mesh>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let clothing = ready.entity;
    let Ok(root) = parents.get(clothing).map(ChildOf::parent) else { return };
    let Ok(rig) = rigs.get(root) else {
        warn!("clothing: the avatar has not loaded");
        return;
    };

    let mut bones = Bones::default();
    let mut body = Vec::new();
    for (mesh, skin) in children.iter_descendants(root).filter_map(|e| skins.get(e).ok()) {
        let (Some(mesh), Some(inverse)) = (meshes.get(&mesh.0), bindposes.get(&skin.inverse_bindposes)) else { continue };
        let first = bones.joints.len() as u16;
        for (&joint, &inverse) in skin.joints.iter().zip(inverse.iter()) {
            bones.joints.push(joint);
            bones.inverse_bindposes.push(inverse);
            bones.rest.push(Mat4::from(rig.rest_space(joint)));
        }
        let (Some(VertexAttributeValues::Float32x3(positions)), Some(VertexAttributeValues::Uint16x4(joints)), Some(VertexAttributeValues::Float32x4(weights))) =
            (mesh.attribute(Mesh::ATTRIBUTE_POSITION), mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX), mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT))
        else {
            continue;
        };
        for ((&position, &joints), &weights) in positions.iter().zip(joints).zip(weights) {
            let joints = joints.map(|j| first + j);
            body.push(BodyVertex { position: bones.skinning(joints, weights).transform_point3(Vec3::from(position)), joints, weights });
        }
    }
    if body.is_empty() || bones.joints.len() > MAX_JOINTS {
        warn!("clothing: the avatar has no skinned body to wear it, or more than {MAX_JOINTS} joints ({})", bones.joints.len());
        return;
    }
    let grid = BodyGrid::new(body);
    let inverse_bindposes = bindposes.add(SkinnedMeshInverseBindposes::from(bones.inverse_bindposes.clone()));

    for entity in children.iter_descendants(clothing) {
        let Ok(mesh) = surfaces.get(entity) else { continue };
        let Some(mesh) = meshes.get(&mesh.0) else { continue };
        let Some(fitted) = fitted_mesh(mesh, avatar_space(entity, &tree), &grid, &bones) else { continue };
        commands.entity(entity).insert((NoFrustumCulling, Mesh3d(meshes.add(fitted)), SkinnedMesh { inverse_bindposes: inverse_bindposes.clone(), joints: bones.joints.clone() }));
    }
    info!("clothing fitted to {} body vertices", grid.vertices.len());
}

/// `mesh`, placed in avatar space by `space`, with each vertex bound to the bones of the nearest body vertex and moved into bind space.
fn fitted_mesh(mesh: &Mesh, space: Affine3A, grid: &BodyGrid, bones: &Bones) -> Option<Mesh> {
    let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
    let mut fitted = mesh.clone();
    let mut joint_indices = Vec::with_capacity(positions.len());
    let mut joint_weights = Vec::with_capacity(positions.len());
    let mut to_bind = Vec::with_capacity(positions.len());
    let mut new_positions = Vec::with_capacity(positions.len());
    for &position in positions {
        let position = space.transform_point3(Vec3::from(position));
        let nearest = grid.nearest(position)?;
        let unskin = bones.skinning(nearest.joints, nearest.weights).inverse();
        new_positions.push(unskin.transform_point3(position).to_array());
        joint_indices.push(nearest.joints);
        joint_weights.push(nearest.weights);
        to_bind.push(unskin * Mat4::from(space));
    }
    fitted.insert_attribute(Mesh::ATTRIBUTE_POSITION, new_positions);
    if let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        let normals: Vec<[f32; 3]> = normals.iter().zip(&to_bind).map(|(&n, m)| m.transform_vector3(Vec3::from(n)).normalize_or_zero().to_array()).collect();
        fitted.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    }
    if let Some(VertexAttributeValues::Float32x4(tangents)) = mesh.attribute(Mesh::ATTRIBUTE_TANGENT) {
        let tangents: Vec<[f32; 4]> = tangents.iter().zip(&to_bind).map(|(&[x, y, z, w], m)| m.transform_vector3(Vec3::new(x, y, z)).normalize_or_zero().extend(w).to_array()).collect();
        fitted.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
    }
    fitted.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(joint_indices));
    fitted.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, joint_weights);
    Some(fitted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_body_vertex_is_found_across_cells() {
        let vertex = |x: f32| BodyVertex { position: Vec3::new(x, 0.0, 0.0), joints: [x as u16, 0, 0, 0], weights: [1.0, 0.0, 0.0, 0.0] };
        let grid = BodyGrid::new(vec![vertex(0.0), vertex(100.0), vertex(250.0)]);
        assert_eq!(grid.nearest(Vec3::new(140.0, 10.0, 0.0)).unwrap().joints[0], 100);
        assert_eq!(grid.nearest(Vec3::new(1000.0, 0.0, 0.0)).unwrap().joints[0], 250);
    }
}
