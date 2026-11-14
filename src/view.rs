//! Camera, lights and the figure's turn. Once the avatar's meshes are in place the camera frames the whole figure; the mouse wheel or the tray moves it between that full view and a close-up of the face. The figure turns about its vertical axis as `[window] turn` or the tray says.

use bevy::camera::primitives::{Aabb, Sphere};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;

use riggy::control::Command;

use crate::avatar::{AvatarRoot, Humanoid, HumanoidBone};
use crate::commands::Received;
use crate::config::Config;

/// Relative margin above the figure in the full view.
const MARGIN: f32 = 0.06;
/// Height of the face close-up in metres.
const FACE_HEIGHT: f32 = 0.32;

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GlobalAmbientLight {
            color: Color::WHITE,
            brightness: 400.0,
            ..default()
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (turn_avatar, frame_avatar, zoom, place_camera).chain());
    }
}

/// The two framings and where between them the camera is (0 = full figure, 1 = face).
#[derive(Component, Default)]
struct Framing {
    full: Option<Shot>,
    face: Option<Shot>,
    zoom: f32,
}

/// Point the camera looks at and how much height it shows there.
#[derive(Clone, Copy)]
struct Shot {
    target: Vec3,
    height: f32,
}

fn setup(mut commands: Commands, config: Res<Config>) {
    let framing = Framing {
        zoom: config.window.zoom.clamp(0.0, 1.0),
        ..default()
    };
    commands.spawn((Camera3d::default(), framing, Transform::from_xyz(0.0, 1.0, 4.0)));
    // Key light from the front left above, fill from the right, rim from behind.
    commands.spawn((
        DirectionalLight {
            illuminance: 6000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(1.5, 2.5, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 1500.0,
            ..default()
        },
        Transform::from_xyz(-3.0, 1.0, 1.5).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 2500.0,
            ..default()
        },
        Transform::from_xyz(0.0, 2.0, -3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Sets the full-figure framing from the mesh bounds once they exist, and the face framing from the humanoid's head.
fn frame_avatar(
    mut cameras: Query<(&mut Framing, &Projection)>,
    avatars: Query<(Entity, &GlobalTransform, Option<&Humanoid>), With<AvatarRoot>>,
    children: Query<&Children>,
    meshes: Query<(&GlobalTransform, Option<&Aabb>, &Mesh3d, Option<&SkinnedMesh>)>,
    mesh_assets: Res<Assets<Mesh>>,
    bindposes: Res<Assets<SkinnedMeshInverseBindposes>>,
    joints: Query<&GlobalTransform>,
) {
    let Ok((mut framing, projection)) = cameras.single_mut() else { return };
    if framing.full.is_some() {
        return;
    }
    let Projection::Perspective(perspective) = projection else { return };
    let Ok((root, root_transform, humanoid)) = avatars.single() else { return };

    let mut min = Vec3A::splat(f32::MAX);
    let mut max = Vec3A::splat(f32::MIN);
    let mut count = 0;
    for entity in children.iter_descendants(root) {
        let Ok((transform, aabb, _, _)) = meshes.get(entity) else { continue };
        // Bounds appear a frame after the mesh; wait until all are there.
        let Some(aabb) = aabb else { return };
        let sphere = Sphere {
            center: Vec3A::from(transform.transform_point(Vec3::from(aabb.center))),
            radius: transform.radius_vec3a(aabb.half_extents),
        };
        let bounds = Aabb::from(sphere);
        min = min.min(bounds.min());
        max = max.max(bounds.max());
        count += 1;
    }
    if count == 0 {
        return;
    }
    // The sphere conversion overestimates; take the vertical extent from the bones where known.
    let (bottom, top) = match humanoid {
        Some(h) if h.height > 0.0 => {
            let scale = root_transform.scale().y;
            let floor = root_transform.translation().y + h.floor * scale;
            (floor, floor + h.height * scale * 1.06)
        }
        _ => (min.y, max.y),
    };
    let center = Vec3::new((min.x + max.x) / 2.0, (bottom + top) / 2.0, (min.z + max.z) / 2.0);
    // The feet stand on the window's bottom edge, so on the desktop the figure stands on the panel; the margin stays above the head.
    let height = (top - bottom) * (1.0 + MARGIN);
    let mut target = Vec3::new(center.x, bottom + height / 2.0, center.z);
    // In perspective the soles nearer the camera than the target appear lower and those further away higher; shift the view so that the lowest one as seen touches the edge.
    let vertices: Vec<Vec3> = children
        .iter_descendants(root)
        .filter_map(|e| {
            let (transform, _, mesh, skin) = meshes.get(e).ok()?;
            Some(world_vertices(mesh_assets.get(&mesh.0)?, transform, skin.map(|s| (s, &*bindposes, &joints))))
        })
        .flatten()
        .collect();
    let distance = height / 2.0 / (perspective.fov / 2.0).tan();
    let camera_z = target.z + distance;
    let seen_lowest = vertices.iter().filter(|v| v.z < camera_z).map(|v| target.y + (v.y - target.y) * distance / (camera_z - v.z)).reduce(f32::min);
    if let Some(seen) = seen_lowest {
        target.y += seen - (target.y - height / 2.0);
    }
    let full = Shot { target, height };
    info!("avatar bounds: {:.2} m high, centre {:.2?}", top - bottom, center);
    framing.full = Some(full);

    let face = humanoid.and_then(|h| {
        let eyes = [HumanoidBone::LeftEye, HumanoidBone::RightEye].map(|b| h.position(b));
        let p = match eyes {
            [Some(l), Some(r)] => (l + r) / 2.0,
            _ => h.position(HumanoidBone::Head)?,
        };
        Some(root_transform.transform_point(p))
    });
    framing.face = Some(Shot {
        target: face.unwrap_or(center),
        height: FACE_HEIGHT.min(full.height),
    });
}

/// A mesh's vertices in world space, skinned with the joints' current transforms.
fn world_vertices(mesh: &Mesh, transform: &GlobalTransform, skin: Option<(&SkinnedMesh, &Assets<SkinnedMeshInverseBindposes>, &Query<&GlobalTransform>)>) -> Vec<Vec3> {
    let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return Vec::new() };
    let skinning = skin.and_then(|(skin, bindposes, joints)| {
        let inverse = bindposes.get(&skin.inverse_bindposes)?;
        let matrices: Vec<Mat4> = skin.joints.iter().zip(inverse.iter()).map(|(&joint, ibm)| Some(joints.get(joint).ok()?.to_matrix() * *ibm)).collect::<Option<_>>()?;
        let Some(VertexAttributeValues::Uint16x4(indices)) = mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX) else { return None };
        let Some(VertexAttributeValues::Float32x4(weights)) = mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT) else { return None };
        Some((matrices, indices, weights))
    });
    positions
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let p = Vec3::from(p);
            match &skinning {
                Some((matrices, indices, weights)) => (0..4).map(|k| matrices[indices[i][k] as usize].transform_point3(p) * weights[i][k]).sum(),
                None => transform.transform_point(p),
            }
        })
        .collect()
}

/// Turns the figure: when it appears as `[window] turn` says, later as the tray asks.
fn turn_avatar(config: Res<Config>, mut commands: MessageReader<Received>, mut avatars: Query<(&mut Transform, Ref<AvatarRoot>)>) {
    let asked = commands.read().filter_map(|Received(command)| if let Command::Turn(degrees) = *command { Some(degrees) } else { None }).last();
    for (mut transform, root) in &mut avatars {
        let degrees = match asked {
            Some(degrees) => degrees,
            None if root.is_added() => config.window.turn,
            None => continue,
        };
        transform.rotation = Quat::from_rotation_y(degrees.to_radians());
    }
}

/// Zoom from the mouse wheel and the tray.
fn zoom(mut wheel: MessageReader<MouseWheel>, mut commands: MessageReader<Received>, mut cameras: Query<&mut Framing>) {
    let Ok(mut framing) = cameras.single_mut() else { return };
    for Received(command) in commands.read() {
        if let Command::Zoom(zoom) = *command {
            framing.zoom = zoom.clamp(0.0, 1.0);
        }
    }
    for event in wheel.read() {
        let steps = match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / 100.0,
        };
        framing.zoom = (framing.zoom + steps * 0.1).clamp(0.0, 1.0);
    }
}

/// Places the camera in front of the avatar (+Z) so the current shot fills the window height.
fn place_camera(mut cameras: Query<(&Framing, &Projection, &mut Transform)>) {
    let Ok((framing, projection, mut transform)) = cameras.single_mut() else { return };
    let (Some(full), Some(face)) = (framing.full, framing.face) else { return };
    let Projection::Perspective(perspective) = projection else { return };
    // Ease so that equal wheel steps feel like equal zoom steps.
    let t = framing.zoom;
    let height = full.height * (face.height / full.height).powf(t);
    let target = full.target.lerp(face.target, ((full.height - height) / (full.height - face.height).max(1e-6)).clamp(0.0, 1.0));
    let distance = height / 2.0 / (perspective.fov / 2.0).tan();
    *transform = Transform::from_translation(target + Vec3::Z * distance).looking_at(target, Vec3::Y);
}
