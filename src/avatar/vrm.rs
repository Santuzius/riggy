//! `vrm` backend: a VRM 1.0 model loaded by bevy_vrm1, which takes the humanoid bones and the expressions from the file itself, and moves the spring bones (hair, cloth) by itself.
//!
//! MToon shades by the direction of each directional light and ignores its strength, so a VRM avatar is lit by one key light (`view`). bevy_vrm1 draws no outline on a double-sided material (most of a VRoid model), so the outlined ones are made single-sided; the inside of a skirt then shows in the outline colour, as in VRoid Hub's viewer.
//!
//! Pose code moves the humanoid bones as on a `glb` avatar; the expression weights go to bevy_vrm1, which turns them into the model's morph targets and material changes. A VRM model needs no mapping file; one next to it may set `window_height`.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::render::render_resource::Face;
use bevy_vrm1::prelude::{Initialized, MToonMaterial, OutlineWidthMode, SetExpressions, VrmBone, VrmExpression, VrmHandle, VrmPlugin};

use super::{AvatarReady, AvatarRoot, AvatarSet, ExpressionName, Expressions, Humanoid, HumanoidBone, PoseBone, TransformTree, model_asset_path, pose_bone};
use crate::config::{AvatarKind, Config};

/// Backend state on the avatar root.
#[derive(Component)]
struct VrmRig {
    /// Humanoid bones put back to rest each frame.
    driven: Vec<PoseBone>,
    /// Weights last handed to bevy_vrm1, so unchanged ones are not sent again.
    sent: HashMap<ExpressionName, f32>,
}

pub struct VrmAvatarPlugin;

impl Plugin for VrmAvatarPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(VrmPlugin)
            .add_systems(Startup, spawn_avatar)
            .add_systems(Update, (reset_bones.in_set(AvatarSet::Reset), apply_expressions.in_set(AvatarSet::Apply)));
    }
}

fn spawn_avatar(mut commands: Commands, config: Res<Config>, asset_server: Res<AssetServer>) {
    if config.avatar.kind != AvatarKind::Vrm {
        return;
    }
    let Some(path) = model_asset_path(&config.avatar.path) else { return };
    info!("loading {}", config.avatar.path.display());
    commands.spawn((AvatarRoot, Name::new("avatar"), Expressions::default(), VrmHandle(asset_server.load(path)))).observe(resolve_avatar);
}

/// Builds the [`Humanoid`] from the bones bevy_vrm1 marked, once the model is attached.
fn resolve_avatar(
    ready: On<Insert<Initialized>>,
    mut commands: Commands,
    children: Query<&Children>,
    bones: Query<&VrmBone>,
    tree: TransformTree,
    surfaces: Query<&MeshMaterial3d<MToonMaterial>>,
    mut materials: ResMut<Assets<MToonMaterial>>,
) {
    let root = ready.entity;
    add_outlines(children.iter_descendants(root).filter_map(|e| surfaces.get(e).ok()), &mut materials);
    let mut humanoid = Humanoid::default();
    for entity in children.iter_descendants(root) {
        if let Ok(bone) = bones.get(entity)
            && let Ok(bone) = bone.0.parse::<HumanoidBone>()
            && let Some(pose) = pose_bone(entity, &tree)
        {
            humanoid.bones.insert(bone, pose);
        }
    }
    humanoid.set_extent(children.iter_descendants(root), &tree);
    info!("VRM avatar loaded: {} humanoid bones", humanoid.bones.len());
    let driven = humanoid.bones.values().copied().collect();
    commands.entity(root).insert((humanoid, VrmRig { driven, sent: HashMap::new() }, AvatarReady));
}

/// Makes each double-sided material with an outline single-sided, so bevy_vrm1 draws its outline.
fn add_outlines<'a>(surfaces: impl Iterator<Item = &'a MeshMaterial3d<MToonMaterial>>, materials: &mut Assets<MToonMaterial>) {
    for surface in surfaces {
        if materials.get(&surface.0).is_some_and(|m| m.outline.mode == OutlineWidthMode::WorldCoordinates && m.outline.width_factor > 0.0 && m.cull_mode.is_none())
            && let Some(mut material) = materials.get_mut(&surface.0)
        {
            material.cull_mode = Some(Face::Back);
        }
    }
}

fn reset_bones(rigs: Query<&VrmRig>, mut transforms: Query<&mut Transform>) {
    for bone in rigs.iter().flat_map(|rig| &rig.driven) {
        if let Ok(mut t) = transforms.get_mut(bone.entity) {
            *t = bone.rest;
        }
    }
}

fn apply_expressions(mut commands: Commands, mut rigs: Query<(Entity, &mut VrmRig, &Expressions)>) {
    for (entity, mut rig, expressions) in &mut rigs {
        if rig.sent == expressions.weights {
            continue;
        }
        rig.sent.clone_from(&expressions.weights);
        commands.trigger(SetExpressions::from_iter(entity, expressions.weights.iter().map(|(name, &weight)| (VrmExpression(name.to_string()), weight))));
    }
}
