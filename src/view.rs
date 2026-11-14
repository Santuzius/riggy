//! Camera, lights and the figure's turn. The camera projects orthographically, so the floor is seen edge-on: every point on it, near or far, lies on one line of the window, as on a panel. Once the avatar's meshes are in place the camera frames the whole figure with the lowest sole on the window's bottom edge, so on the desktop the figure stands on the panel; the mouse wheel or the control socket's `zoom` moves it between that full view and a close-up of the face, which follows the eyes. The figure turns (see `animation::stance`) as `[window] turn` says, while the right mouse button drags it, or on the control socket's `turn`. On the desktop the zoom and turn made with the mouse are kept for the next start.

use bevy::camera::primitives::{Aabb, Sphere};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::VertexAttributeValues;
use bevy::camera::ScalingMode;
use bevy::prelude::*;
use bevy::window::{CursorMoved, PrimaryWindow};

use riggy::control::Command;
use riggy::settings::Settings;

use crate::animation::stance::Stance;
use crate::avatar::{AvatarRoot, Humanoid, HumanoidBone};
use crate::commands::Received;
use crate::config::Config;

/// Relative margin above the figure in the full view.
const MARGIN: f32 = 0.06;
/// Height of the face close-up in metres.
const FACE_HEIGHT: f32 = 0.32;
/// Degrees the figure turns per logical pixel of a horizontal drag.
const TURN_PER_PIXEL: f32 = 0.5;
/// Seconds after the last wheel step at which the zoom is kept, so one turn of the wheel writes the settings once.
const ZOOM_SETTLE: f32 = 0.5;
/// Metres between the camera and what it looks at; with an orthographic projection only the clipping depends on it.
const CAMERA_DISTANCE: f32 = 20.0;
/// Height in metres above the lowest vertex up to which vertices count as soles.
const SOLE_HEIGHT: f32 = 0.02;
/// Seconds the camera takes to follow the soles after a step.
const GROUND_TIME: f32 = 0.25;
/// Seconds the face close-up takes to follow the eyes.
const FACE_TIME: f32 = 0.6;

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GlobalAmbientLight {
            color: Color::WHITE,
            brightness: 400.0,
            ..default()
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (turn_avatar, frame_avatar, ground, follow_face, zoom, place_camera).chain());
    }
}

/// The two framings and where between them the camera is (0 = full figure, 1 = face).
#[derive(Component, Default)]
struct Framing {
    full: Option<Shot>,
    face: Option<Shot>,
    zoom: f32,
    /// Vertical shift of the full shot that puts the lowest sole as seen on the bottom edge.
    ground: Option<f32>,
    /// The vertices the figure stands on.
    soles: Vec<Vertex>,
    /// Bones whose midpoint the face close-up follows.
    eyes: Vec<Entity>,
}

/// A mesh vertex as the weighted sum of points that move with entities: the joints that skin it, or the mesh itself.
type Vertex = [(Entity, Vec3, f32); 4];

fn position(vertex: &Vertex, transforms: &Query<&GlobalTransform>) -> Vec3 {
    vertex.iter().filter(|(_, _, w)| *w > 0.0).filter_map(|&(e, p, w)| Some(transforms.get(e).ok()?.transform_point(p) * w)).sum()
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
    let projection = OrthographicProjection {
        scaling_mode: ScalingMode::FixedVertical { viewport_height: 2.0 },
        ..OrthographicProjection::default_3d()
    };
    commands.spawn((Camera3d::default(), Projection::Orthographic(projection), framing, Transform::from_xyz(0.0, 1.0, CAMERA_DISTANCE)));
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
    mut cameras: Query<&mut Framing>,
    avatars: Query<(Entity, &GlobalTransform, Option<&Humanoid>), With<AvatarRoot>>,
    children: Query<&Children>,
    meshes: Query<(&GlobalTransform, Option<&Aabb>, &Mesh3d, Option<&SkinnedMesh>)>,
    mesh_assets: Res<Assets<Mesh>>,
    bindposes: Res<Assets<SkinnedMeshInverseBindposes>>,
    transforms: Query<&GlobalTransform>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let Ok(mut framing) = cameras.single_mut() else { return };
    if framing.full.is_some() {
        return;
    }
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
    let vertices: Vec<Vertex> = children
        .iter_descendants(root)
        .filter_map(|e| {
            let (_, _, mesh, skin) = meshes.get(e).ok()?;
            Some(mesh_vertices(e, mesh_assets.get(&mesh.0)?, skin.map(|s| (s, &*bindposes))))
        })
        .flatten()
        .collect();
    let positions: Vec<Vec3> = vertices.iter().map(|v| position(v, &transforms)).collect();
    let lowest = positions.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    // The sphere conversion overestimates; take the vertical extent from the bones where known, else from the vertices.
    let (bottom, top) = match humanoid {
        Some(h) if h.height > 0.0 => {
            let scale = root_transform.scale().y;
            let floor = root_transform.translation().y + h.floor * scale;
            (floor, floor + h.height * scale * 1.06)
        }
        _ if !positions.is_empty() => (lowest, positions.iter().map(|p| p.y).fold(f32::MIN, f32::max)),
        _ => (min.y, max.y),
    };
    let center = Vec3::new((min.x + max.x) / 2.0, (bottom + top) / 2.0, (min.z + max.z) / 2.0);
    // The widest the figure gets as it turns about its vertical axis; a wide figure such as a machine fills the window's width instead of its height.
    let axis = root_transform.translation();
    let radius = positions.iter().map(|p| Vec2::new(p.x - axis.x, p.z - axis.z).length()).fold(0.0, f32::max);
    let aspect = windows.single().map_or(1.0, |w| w.width() / w.height().max(1.0));
    // The feet stand on the window's bottom edge, so on the desktop the figure stands on the panel; the margin stays above the head.
    let height = (top - bottom).max(2.0 * radius / aspect) * (1.0 + MARGIN);
    let target = Vec3::new(axis.x, bottom + height / 2.0, axis.z);
    // The soles of a humanoid: the vertices near the lowest one; `ground` keeps the lowest of them as seen on the window's edge. A machine floats, so it has none.
    if humanoid.is_some() {
        framing.soles = vertices.into_iter().zip(&positions).filter(|&(_, p)| p.y < lowest + SOLE_HEIGHT).map(|(v, _)| v).collect();
    }
    let full = Shot { target, height };
    info!("avatar bounds: {:.2} m high, centre {:.2?}, {} sole vertices", top - bottom, center, framing.soles.len());
    framing.full = Some(full);

    let bones = |list: &[HumanoidBone]| -> Vec<Entity> { humanoid.map(|h| list.iter().filter_map(|&b| Some(h.get(b)?.entity)).collect()).unwrap_or_default() };
    framing.eyes = match bones(&[HumanoidBone::LeftEye, HumanoidBone::RightEye]) {
        eyes if eyes.len() == 2 => eyes,
        _ => bones(&[HumanoidBone::Head]),
    };
    let eyes: Vec<Vec3> = framing.eyes.iter().filter_map(|&e| Some(transforms.get(e).ok()?.translation())).collect();
    framing.face = Some(Shot {
        target: if eyes.is_empty() { center } else { eyes.iter().sum::<Vec3>() / eyes.len() as f32 },
        // Without eyes (a machine) the close-up shows the middle half of the figure.
        height: if eyes.is_empty() { full.height / 2.0 } else { FACE_HEIGHT.min(full.height) },
    });
}

/// Keeps the lowest sole on the bottom edge of the full shot, so a sole that sits a little higher or lower after a turn or a step still touches it; while a foot is in the air the shift stays.
fn ground(time: Res<Time>, mut cameras: Query<&mut Framing>, stances: Query<&Stance>, transforms: Query<&GlobalTransform>) {
    let Ok(mut framing) = cameras.single_mut() else { return };
    let Some(full) = framing.full else { return };
    if stances.iter().any(Stance::stepping) {
        return;
    }
    let Some(lowest) = framing.soles.iter().map(|v| position(v, &transforms).y).reduce(f32::min) else { return };
    let wanted = lowest - (full.target.y - full.height / 2.0);
    let k = 1.0 - (-time.delta_secs() / GROUND_TIME).exp();
    framing.ground = Some(framing.ground.map_or(wanted, |now| now + (wanted - now) * k));
}

/// Lets the face close-up follow the eyes, slowly enough that it does not shake with every motion of the head.
fn follow_face(time: Res<Time>, mut cameras: Query<&mut Framing>, transforms: Query<&GlobalTransform>) {
    let Ok(mut framing) = cameras.single_mut() else { return };
    let points: Vec<Vec3> = framing.eyes.iter().filter_map(|&e| Some(transforms.get(e).ok()?.translation())).collect();
    let Some(face) = framing.face.as_mut() else { return };
    if points.is_empty() {
        return;
    }
    let eyes = points.iter().sum::<Vec3>() / points.len() as f32;
    let k = 1.0 - (-time.delta_secs() / FACE_TIME).exp();
    face.target += (eyes - face.target) * k;
}

/// The vertices of mesh entity `entity`, each tied to the joints that skin it, or to the mesh entity itself.
fn mesh_vertices(entity: Entity, mesh: &Mesh, skin: Option<(&SkinnedMesh, &Assets<SkinnedMeshInverseBindposes>)>) -> Vec<Vertex> {
    let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return Vec::new() };
    let skinning = skin.and_then(|(skin, bindposes)| {
        let inverse = bindposes.get(&skin.inverse_bindposes)?;
        let Some(VertexAttributeValues::Uint16x4(indices)) = mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX) else { return None };
        let Some(VertexAttributeValues::Float32x4(weights)) = mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT) else { return None };
        Some((skin, inverse, indices, weights))
    });
    positions
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let p = Vec3::from(p);
            match &skinning {
                Some((skin, inverse, indices, weights)) => std::array::from_fn(|k| {
                    let joint = indices[i][k] as usize;
                    (skin.joints[joint], inverse[joint].transform_point3(p), weights[i][k])
                }),
                None => [(entity, p, 1.0), (entity, p, 0.0), (entity, p, 0.0), (entity, p, 0.0)],
            }
        })
        .collect()
}

/// The figure's turn in degrees, and the cursor's last x while the right mouse button drags.
struct Turning {
    degrees: f32,
    drag_x: Option<f32>,
}

impl FromWorld for Turning {
    fn from_world(world: &mut World) -> Self {
        Self { degrees: world.resource::<Config>().window.turn, drag_x: None }
    }
}

/// `degrees` as an angle between -180° and 180°.
fn wrapped(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

/// Turns the figure: when it appears, while the right mouse button drags it (to the right turns it to its left, as if pushing its front), and on the control socket's `turn`.
fn turn_avatar(
    config: Res<Config>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut cursor: MessageReader<CursorMoved>,
    mut commands: MessageReader<Received>,
    mut turning: Local<Turning>,
    mut stances: Query<&mut Stance>,
    mut unstanced: Query<(&mut Transform, Ref<AvatarRoot>), Without<Stance>>,
) {
    let mut changed = false;
    for Received(command) in commands.read() {
        if let Command::Turn(degrees) = *command {
            turning.degrees = wrapped(degrees);
            changed = true;
        }
    }
    for moved in cursor.read() {
        if !buttons.pressed(MouseButton::Right) {
            turning.drag_x = None;
            continue;
        }
        if let Some(x) = turning.drag_x {
            turning.degrees = wrapped(turning.degrees + (moved.position.x - x) * TURN_PER_PIXEL);
            changed = true;
        }
        turning.drag_x = Some(moved.position.x);
    }
    if buttons.just_released(MouseButton::Right) {
        turning.drag_x = None;
        if config.on_desktop {
            let degrees = turning.degrees;
            remember(|settings| settings.turn = degrees);
        }
    }
    for mut stance in &mut stances {
        if changed || stance.is_added() {
            stance.wanted = turning.degrees.to_radians();
        }
    }
    // A figure without feet, such as a machine, turns at once.
    for (mut transform, root) in &mut unstanced {
        if changed || root.is_added() {
            transform.rotation = Quat::from_rotation_y(turning.degrees.to_radians());
        }
    }
}

/// Keeps a view made with the mouse in the tray's settings, from which the next start takes it.
fn remember(change: impl FnOnce(&mut Settings)) {
    let mut settings = Settings::load();
    change(&mut settings);
    if let Err(err) = settings.save() {
        warn!("cannot keep the view: {err}");
    }
}

/// Zoom from the mouse wheel and the control socket; on the desktop the wheel's zoom is kept once the wheel rests.
fn zoom(
    config: Res<Config>,
    time: Res<Time>,
    mut wheel: MessageReader<MouseWheel>,
    mut commands: MessageReader<Received>,
    mut cameras: Query<&mut Framing>,
    mut unsaved_for: Local<Option<f32>>,
) {
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
        *unsaved_for = Some(0.0);
    }
    if let Some(seconds) = unsaved_for.as_mut() {
        *seconds += time.delta_secs();
        if *seconds >= ZOOM_SETTLE {
            *unsaved_for = None;
            if config.on_desktop {
                let zoom = framing.zoom;
                remember(|settings| settings.zoom = zoom);
            }
        }
    }
}

/// Places the camera in front of the avatar (+Z) so the current shot fills the window height.
fn place_camera(mut cameras: Query<(&Framing, &mut Projection, &mut Transform)>) {
    let Ok((framing, mut projection, mut transform)) = cameras.single_mut() else { return };
    let (Some(mut full), Some(face)) = (framing.full, framing.face) else { return };
    full.target.y += framing.ground.unwrap_or(0.0);
    let Projection::Orthographic(orthographic) = &mut *projection else { return };
    // Ease so that equal wheel steps feel like equal zoom steps.
    let t = framing.zoom;
    let height = full.height * (face.height / full.height).powf(t);
    let target = full.target.lerp(face.target, ((full.height - height) / (full.height - face.height).max(1e-6)).clamp(0.0, 1.0));
    orthographic.scaling_mode = ScalingMode::FixedVertical { viewport_height: height };
    *transform = Transform::from_translation(target + Vec3::Z * CAMERA_DISTANCE).looking_at(target, Vec3::Y);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_wrap_to_half_a_circle_either_way() {
        assert_eq!(wrapped(30.0), 30.0);
        assert_eq!(wrapped(190.0), -170.0);
        assert_eq!(wrapped(-200.0), 160.0);
    }
}
