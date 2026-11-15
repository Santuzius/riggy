//! `vrm` backend: a VRM 1.0 model loaded by bevy_vrm1, which takes the humanoid bones and the expressions from the file itself, and moves the spring bones (hair, cloth) by itself.
//!
//! MToon shades by the direction of each directional light and ignores its strength, so a VRM avatar is lit by one key light (`view`). bevy_vrm1 draws no outline on a double-sided material (most of a VRoid model), so the outlined ones are made single-sided; the inside of a skirt then shows in the outline colour, as in VRoid Hub's viewer.
//!
//! Pose code moves the humanoid bones as on a `glb` avatar; the expression weights go to bevy_vrm1, which turns them into the model's morph targets and material changes. A VRM model needs no mapping file; its own mapping file may set `window_height`, and its `[[cut]]` and `[[cloth]]` edit the model (`riggy::model_edit`): riggy loads an edited copy from `files::edited_models_dir`, rebuilt when the mapping file changes. Spring chains measure their inertia relative to the avatar root, so turning the figure does not fling hair and cloth outward.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bevy::prelude::*;
use bevy::render::render_resource::Face;
use bevy_vrm1::vrm::spring_bone::{SpringCenterNode, SpringRoot};
use bevy_vrm1::prelude::{Initialized, MToonMaterial, OutlineWidthMode, SetExpressions, VrmBone, VrmExpression, VrmHandle, VrmPlugin};

use super::{AvatarReady, AvatarRoot, AvatarSet, ExpressionName, ExpressionPreset, Expressions, Humanoid, HumanoidBone, PoseBone, TransformTree, model_asset_path, pose_bone};
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
            .add_systems(Update, (reset_bones.in_set(AvatarSet::Reset), apply_expressions.in_set(AvatarSet::Apply), reload_model));
    }
}

/// The installed model, its mapping file, and the edited copy riggy loads; the copy is rebuilt when the mapping file changes.
#[derive(Resource)]
struct ModelWatch {
    model: PathBuf,
    mapping: PathBuf,
    modified: Option<SystemTime>,
    /// Counts the copies written; each gets its own file name, so the asset server loads it afresh.
    generation: u32,
    timer: Timer,
}

fn spawn_avatar(mut commands: Commands, config: Res<Config>, asset_server: Res<AssetServer>) {
    if config.avatar.kind != AvatarKind::Vrm {
        return;
    }
    let watch = ModelWatch {
        model: config.avatar.path.clone(),
        mapping: config.avatar.mapping.clone(),
        modified: crate::config::modified(&config.avatar.mapping),
        generation: 0,
        timer: Timer::new(Duration::from_secs(1), TimerMode::Repeating),
    };
    spawn_model(&mut commands, &asset_server, &watch);
    commands.insert_resource(watch);
}

/// Writes the edited copy of the model and spawns the avatar from it.
fn spawn_model(commands: &mut Commands, asset_server: &AssetServer, watch: &ModelWatch) {
    let Some(copy) = write_edited_model(&watch.model, &watch.mapping, watch.generation) else { return };
    let Some(path) = model_asset_path(&copy) else { return };
    info!("loading {}", watch.model.display());
    commands.spawn((AvatarRoot, Name::new("avatar"), Expressions::default(), VrmHandle(asset_server.load(path)))).observe(resolve_avatar);
}

/// Writes `model` with the edits of its mapping file (`riggy::model_edit`) as copy number `generation` and removes older copies; returns the copy's path. A mapping file that cannot be applied leaves the model as it is.
fn write_edited_model(model: &Path, mapping: &Path, generation: u32) -> Option<PathBuf> {
    let bytes = std::fs::read(model).map_err(|err| error!("{}: {err}", model.display())).ok()?;
    let text = std::fs::read_to_string(mapping).unwrap_or_default();
    let bytes = match riggy::model_edit::edited_vrm(&bytes, &text) {
        Ok(edited) => edited.unwrap_or(bytes),
        Err(err) => {
            error!("{}: {err}; the model is loaded without its edits", mapping.display());
            bytes
        }
    };
    let dir = riggy::files::edited_models_dir();
    let stem = model.file_stem()?.to_string_lossy().into_owned();
    for old in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        if old.file_name().to_string_lossy().starts_with(&format!("{stem}.")) {
            let _ = std::fs::remove_file(old.path());
        }
    }
    let copy = dir.join(format!("{stem}.{generation}.vrm"));
    std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&copy, bytes)).map_err(|err| error!("{}: {err}", copy.display())).ok()?;
    Some(copy)
}

/// Rebuilds the edited copy and replaces the avatar when the mapping file changes.
fn reload_model(mut commands: Commands, time: Res<Time>, asset_server: Res<AssetServer>, watch: Option<ResMut<ModelWatch>>, roots: Query<Entity, With<AvatarRoot>>) {
    let Some(mut watch) = watch else { return };
    if !watch.timer.tick(time.delta()).just_finished() {
        return;
    }
    let modified = crate::config::modified(&watch.mapping);
    if modified == watch.modified {
        return;
    }
    watch.modified = modified;
    watch.generation += 1;
    info!("{} changed; reloading the avatar", watch.mapping.display());
    for root in &roots {
        commands.entity(root).despawn();
    }
    spawn_model(&mut commands, &asset_server, &watch);
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
    mut springs: Query<&mut SpringRoot>,
) {
    let root = ready.entity;
    follow_root(root, children.iter_descendants(root), &mut springs);
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

/// Makes `root` the center of each spring chain that has none, so hair and cloth swing with the figure's own movement but not with its turn or its shift on the screen, which riggy gives the avatar root.
fn follow_root(root: Entity, entities: impl Iterator<Item = Entity>, springs: &mut Query<&mut SpringRoot>) {
    for entity in entities {
        if let Ok(mut spring) = springs.get_mut(entity)
            && spring.center_node.0.is_none()
        {
            spring.center_node = SpringCenterNode(Some(root));
        }
    }
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

/// The VRM expression for riggy's `name`. Riggy's `happy` is the idle smile with the mouth closed; VRM's `happy` (VRoid's "Joy") laughs with an open mouth, its `relaxed` smiles with it closed.
fn vrm_expression(name: &ExpressionName) -> VrmExpression {
    match name {
        ExpressionName::Preset(ExpressionPreset::Happy) => VrmExpression::from(ExpressionPreset::Relaxed.as_str()),
        name => VrmExpression(name.to_string()),
    }
}

fn apply_expressions(mut commands: Commands, mut rigs: Query<(Entity, &mut VrmRig, &Expressions)>) {
    for (entity, mut rig, expressions) in &mut rigs {
        if rig.sent == expressions.weights {
            continue;
        }
        rig.sent.clone_from(&expressions.weights);
        commands.trigger(SetExpressions::from_iter(entity, expressions.weights.iter().map(|(name, &weight)| (vrm_expression(name), weight))));
    }
}
