//! `GlbBoneAvatar`: a plain GLB plus a mapping file that names its bones in VRM terms.
//!
//! The mapping file (TOML) has:
//! - `scale`: factor from model units to metres;
//! - `[humanoid]`: VRM humanoid bone name → node name;
//! - `[parts]`: free part name → node name, for non-humanoid avatars;
//! - `[expressions.<name>]`: node name → `{ rotate = [x, y, z], translate = [x, y, z] }`, the bone change at weight 1. `rotate` is an axis-angle vector in degrees (direction = axis, length = angle), `translate` an offset in model units; both are in avatar space.
//!
//! Node names match exactly, or after dropping the `_<digits>` suffix that Sketchfab exports append (`head.x_010` matches `head.x`).
//!
//! The mapping file is re-read when it changes, so expressions can be tuned while riggy runs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bevy::asset::io::{AssetSourceBuilder, AssetSourceId};
use bevy::asset::AssetPath;
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use serde::Deserialize;

use super::{AvatarRoot, AvatarSet, ExpressionName, Expressions, Humanoid, HumanoidBone, PoseBone, avatar_space};
use crate::config::{AvatarKind, Config};

/// Asset source id under which the model's directory is registered.
const SOURCE: &str = "avatar";

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Mapping {
    pub scale: f32,
    pub humanoid: HashMap<String, String>,
    pub parts: HashMap<String, String>,
    pub expressions: HashMap<String, HashMap<String, BoneDelta>>,
}

impl Default for Mapping {
    fn default() -> Self {
        Self {
            scale: 1.0,
            humanoid: HashMap::new(),
            parts: HashMap::new(),
            expressions: HashMap::new(),
        }
    }
}

#[derive(Deserialize, Debug, Clone, Copy, Default)]
#[serde(default)]
pub struct BoneDelta {
    pub rotate: [f32; 3],
    pub translate: [f32; 3],
}

fn read_mapping(path: &Path) -> Option<Mapping> {
    let text = std::fs::read_to_string(path).map_err(|err| error!("mapping {}: {err}", path.display())).ok()?;
    toml::from_str(&text).map_err(|err| error!("mapping {}: {err}", path.display())).ok()
}

/// Named non-humanoid parts of the avatar, on the avatar root entity.
#[derive(Component, Debug, Default)]
pub struct Parts {
    pub bones: HashMap<String, PoseBone>,
}

/// One bone change of an expression at weight 1.
#[derive(Debug, Clone, Copy)]
struct Delta {
    bone: PoseBone,
    /// Axis-angle vector in radians, avatar space.
    rotate: Vec3,
    translate: Vec3,
}

/// Backend state on the avatar root: every bone it drives and the resolved expressions.
#[derive(Component)]
pub struct GlbRig {
    /// Node name (with and without export suffix) → entity, for all nodes of the model.
    nodes: HashMap<String, Entity>,
    /// Bones put back to rest each frame.
    driven: HashMap<Entity, PoseBone>,
    expressions: HashMap<ExpressionName, Vec<Delta>>,
}

#[derive(Resource)]
struct MappingWatch {
    path: PathBuf,
    modified: Option<SystemTime>,
    timer: Timer,
}

pub struct GlbAvatarPlugin;

impl Plugin for GlbAvatarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_avatar)
            .add_systems(Update, (reset_bones.in_set(AvatarSet::Reset), apply_expressions.in_set(AvatarSet::Apply), reload_mapping, dump_nodes));
    }
}

/// Registers the model's directory as asset source; must run before `DefaultPlugins` is added.
pub fn register_model_source(app: &mut App, model: &Path) {
    let dir = model.parent().map(Path::to_path_buf).unwrap_or_default();
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    app.register_asset_source(SOURCE, AssetSourceBuilder::platform_default(&dir.to_string_lossy(), None));
}

fn spawn_avatar(mut commands: Commands, config: Res<Config>, asset_server: Res<AssetServer>) {
    if config.avatar.kind != AvatarKind::Glb {
        return;
    }
    let mapping_path = config.avatar.mapping.clone();
    let mapping = read_mapping(&mapping_path).unwrap_or_default();
    commands.insert_resource(MappingWatch {
        modified: crate::config::modified(&mapping_path),
        path: mapping_path,
        timer: Timer::new(Duration::from_secs(1), TimerMode::Repeating),
    });

    let Some(file) = config.avatar.path.file_name() else {
        error!("avatar path {} has no file name", config.avatar.path.display());
        return;
    };
    let path = AssetPath::from_path(Path::new(file)).into_owned().with_source(AssetSourceId::from(SOURCE));
    info!("loading {}", config.avatar.path.display());
    commands
        .spawn((
            AvatarRoot,
            Name::new("avatar"),
            Transform::from_scale(Vec3::splat(mapping.scale)),
            Expressions::default(),
            WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset(path))),
        ))
        .observe(resolve_avatar);
}

/// Drops the `_<digits>` suffix Sketchfab appends to node names.
fn base_name(name: &str) -> &str {
    match name.rsplit_once('_') {
        Some((base, digits)) if !base.is_empty() && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => name,
    }
}

type TransformTree<'w, 's> = Query<'w, 's, (&'static Transform, Option<&'static ChildOf>, Has<AvatarRoot>)>;

fn pose_bone(entity: Entity, tree: &TransformTree) -> Option<PoseBone> {
    let (transform, parent, _) = tree.get(entity).ok()?;
    let parent_space = parent.map(|p| avatar_space(p.parent(), tree)).unwrap_or(Affine3A::IDENTITY);
    Some(PoseBone::new(entity, *transform, parent_space))
}

fn resolve_avatar(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    watch: Res<MappingWatch>,
    children: Query<&Children>,
    names: Query<&Name>,
    tree: TransformTree,
) {
    let root = ready.entity;
    let mut nodes = HashMap::new();
    for entity in children.iter_descendants(root) {
        if let Ok(name) = names.get(entity) {
            nodes.insert(name.as_str().to_owned(), entity);
            nodes.entry(base_name(name.as_str()).to_owned()).or_insert(entity);
        }
    }
    info!("avatar loaded: {} nodes", nodes.len());

    let mapping = read_mapping(&watch.path).unwrap_or_default();
    let lookup = |node: &str| {
        let found = nodes.get(node).and_then(|&e| pose_bone(e, &tree));
        if found.is_none() {
            warn!("mapping: node '{node}' not found in the model");
        }
        found
    };

    let mut humanoid = Humanoid::default();
    for (bone, node) in &mapping.humanoid {
        match bone.parse::<HumanoidBone>() {
            Ok(b) => {
                if let Some(pb) = lookup(node) {
                    humanoid.bones.insert(b, pb);
                }
            }
            Err(err) => warn!("mapping: {err}"),
        }
    }
    let missing: Vec<_> = HumanoidBone::ALL.iter().filter(|b| !humanoid.bones.contains_key(b)).map(|b| b.as_str()).collect();
    if !mapping.humanoid.is_empty() && !missing.is_empty() {
        info!("humanoid bones not mapped: {}", missing.join(", "));
    }

    // Height and floor from the joint positions of all nodes (avatar space).
    let ys: Vec<f32> = nodes.values().filter_map(|&e| pose_bone(e, &tree)).map(|b| b.position.y).collect();
    humanoid.floor = ys.iter().copied().fold(f32::MAX, f32::min);
    humanoid.height = ys.iter().copied().fold(f32::MIN, f32::max) - humanoid.floor;

    let parts = Parts {
        bones: mapping.parts.iter().filter_map(|(part, node)| Some((part.clone(), lookup(node)?))).collect(),
    };

    let mut rig = GlbRig {
        nodes,
        driven: HashMap::new(),
        expressions: HashMap::new(),
    };
    rig.driven.extend(humanoid.bones.values().chain(parts.bones.values()).map(|b| (b.entity, *b)));
    rig.set_expressions(&mapping, &tree);

    let mut avatar = commands.entity(root);
    avatar.insert((rig, parts));
    if !humanoid.bones.is_empty() {
        avatar.insert(humanoid);
    }
}

impl GlbRig {
    fn set_expressions(&mut self, mapping: &Mapping, tree: &TransformTree) {
        self.expressions.clear();
        for (name, deltas) in &mapping.expressions {
            let mut resolved = Vec::new();
            for (node, delta) in deltas {
                let Some(&entity) = self.nodes.get(node) else {
                    warn!("expression {name}: node '{node}' not found in the model");
                    continue;
                };
                // Keep the rest pose captured at load time; the bone may be moved right now.
                let bone = match self.driven.get(&entity) {
                    Some(b) => *b,
                    None => match pose_bone(entity, tree) {
                        Some(b) => b,
                        None => continue,
                    },
                };
                self.driven.insert(entity, bone);
                resolved.push(Delta {
                    bone,
                    rotate: Vec3::from(delta.rotate).map(f32::to_radians),
                    translate: Vec3::from(delta.translate),
                });
            }
            self.expressions.insert(name.parse().unwrap(), resolved);
        }
    }
}

fn reset_bones(rigs: Query<&GlbRig>, mut transforms: Query<&mut Transform>) {
    for rig in &rigs {
        for bone in rig.driven.values() {
            if let Ok(mut t) = transforms.get_mut(bone.entity) {
                *t = bone.rest;
            }
        }
    }
}

fn apply_expressions(rigs: Query<(&GlbRig, &Expressions)>, mut transforms: Query<&mut Transform>) {
    for (rig, expressions) in &rigs {
        for (name, &weight) in &expressions.weights {
            if weight <= 0.0 {
                continue;
            }
            let Some(deltas) = rig.expressions.get(name) else { continue };
            for delta in deltas {
                let Ok(mut t) = transforms.get_mut(delta.bone.entity) else { continue };
                let angle = delta.rotate.length() * weight;
                if angle > 0.0 {
                    delta.bone.rotate(&mut t, Quat::from_axis_angle(delta.rotate.normalize(), angle));
                }
                delta.bone.translate(&mut t, delta.translate * weight);
            }
        }
    }
}

fn reload_mapping(time: Res<Time>, watch: Option<ResMut<MappingWatch>>, mut rigs: Query<&mut GlbRig>, tree: TransformTree) {
    let Some(mut watch) = watch else { return };
    if !watch.timer.tick(time.delta()).just_finished() {
        return;
    }
    let modified = crate::config::modified(&watch.path);
    if modified == watch.modified {
        return;
    }
    watch.modified = modified;
    let Some(mapping) = read_mapping(&watch.path) else { return };
    info!("reloaded {} (expressions only; bones and scale need a restart)", watch.path.display());
    for mut rig in &mut rigs {
        rig.set_expressions(&mapping, &tree);
    }
}

/// F1 (or `RIGGY_DUMP_NODES` at startup) prints the model's node tree with avatar-space joint positions.
fn dump_nodes(
    keys: Res<ButtonInput<KeyCode>>,
    mut dumped: Local<bool>,
    roots: Query<Entity, With<GlbRig>>,
    children: Query<&Children>,
    names: Query<&Name>,
    tree: TransformTree,
) {
    // `RIGGY_DUMP_NODES=1` dumps once right after loading.
    let auto = !*dumped && std::env::var_os("RIGGY_DUMP_NODES").is_some() && !roots.is_empty();
    if !keys.just_pressed(KeyCode::F1) && !auto {
        return;
    }
    *dumped = true;
    fn walk(entity: Entity, depth: usize, out: &mut String, children: &Query<&Children>, names: &Query<&Name>, tree: &TransformTree) {
        if let Ok(name) = names.get(entity) {
            let p = avatar_space(entity, tree).translation;
            out.push_str(&format!("{:indent$}{name}  ({:.1}, {:.1}, {:.1})\n", "", p.x, p.y, p.z, indent = depth * 2));
        }
        for &child in children.get(entity).into_iter().flatten() {
            walk(child, depth + 1, out, children, names, tree);
        }
    }
    for root in &roots {
        let mut out = String::from("avatar nodes (avatar-space position):\n");
        walk(root, 0, &mut out, &children, &names, &tree);
        info!("{out}");
    }
}
