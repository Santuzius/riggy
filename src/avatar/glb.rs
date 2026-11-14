//! `GlbBoneAvatar`: a plain GLB plus a mapping file that names its bones in VRM terms.
//!
//! The mapping file (TOML) has:
//! - `scale`: factor from model units to metres;
//! - `window_height`: share of the configured window height the avatar takes on the desktop (default 1);
//! - `[humanoid]`: VRM humanoid bone name → node name;
//! - `[parts]`: free part name → node name, for non-humanoid avatars;
//! - `[machine]`: how a machine avatar moves (see [`Machine`]); with it the avatar is a machine, otherwise a humanoid;
//! - `[material]`: textures for a model that comes without them (see [`MaterialSpec`]);
//! - `[[clothing]]`: clothing without a skeleton worn by the avatar (see [`ClothingSpec`]);
//! - `[[props]]`: extra shapes attached to a bone, such as a dark mouth interior or teeth the model lacks: `{ bone, shape = "sphere" | "cube", center = [x, y, z], size = [x, y, z], color = [r, g, b], unlit }`. Centre and size (radii for a sphere, half extents for a cube) are in avatar space and model units, as the shape sits in the rest pose; it then follows the bone.
//! - `[rest]`: node name → bone change as in an expression, applied permanently; corrects the model's rest pose (for example lips that do not quite close).
//! - `[expressions.<name>]`: node name → `{ rotate = [x, y, z], translate = [x, y, z] }`, the bone change at weight 1. `rotate` is an axis-angle vector in degrees (direction = axis, length = angle), `translate` an offset in model units; both are in avatar space.
//!
//! Node names match exactly, or after dropping the `_<digits>` suffix that Sketchfab exports append (`head.x_010` matches `head.x`).
//!
//! The mapping file is re-read when it changes, so expressions can be tuned while riggy runs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bevy::asset::io::AssetSourceId;
use bevy::asset::AssetPath;
use bevy::math::Affine3A;
use bevy::image::ImageLoaderSettings;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use serde::Deserialize;

use super::clothing::{ClothingSpec, spawn_clothing};
use super::{AvatarReady, AvatarRoot, AvatarSet, ExpressionName, Expressions, Humanoid, HumanoidBone, Machine, Parts, PoseBone, SOURCE, TransformTree, avatar_space, model_asset_path, pose_bone};
use crate::config::{AvatarKind, Config};

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Mapping {
    pub scale: f32,
    pub window_height: f32,
    pub humanoid: HashMap<String, String>,
    pub parts: HashMap<String, String>,
    pub rest: HashMap<String, BoneDelta>,
    pub expressions: HashMap<String, HashMap<String, BoneDelta>>,
    pub props: Vec<PropSpec>,
    pub machine: Option<Machine>,
    pub material: MaterialSpec,
    pub clothing: Vec<ClothingSpec>,
}

/// Textures laid onto every material of the model, as files in the model's own folder (`models/NAME/`, where `riggy install NAME.glb TEXTURE ...` puts them). A model whose file brings its textures needs none of this.
#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct MaterialSpec {
    pub base_color: Option<String>,
    /// Packed occlusion (red), roughness (green) and metalness (blue).
    pub orm: Option<String>,
    /// Normal map in OpenGL convention (green up).
    pub normal: Option<String>,
    /// Where the model glows; its brightness follows the expression `glow`.
    pub emission: Option<String>,
    /// Linear RGB of the emission at `glow` weight 1; it may exceed 1 for a brighter glow.
    pub emission_color: [f32; 3],
}

impl Default for MaterialSpec {
    fn default() -> Self {
        Self { base_color: None, orm: None, normal: None, emission: None, emission_color: [1.0; 3] }
    }
}

impl Default for Mapping {
    fn default() -> Self {
        Self {
            scale: 1.0,
            window_height: 1.0,
            humanoid: HashMap::new(),
            parts: HashMap::new(),
            rest: HashMap::new(),
            expressions: HashMap::new(),
            props: Vec::new(),
            machine: None,
            material: MaterialSpec::default(),
            clothing: Vec::new(),
        }
    }
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PropShape {
    #[default]
    Sphere,
    Cube,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct PropSpec {
    pub bone: String,
    pub shape: PropShape,
    pub center: [f32; 3],
    pub size: [f32; 3],
    /// Linear RGB, 0..1.
    pub color: [f32; 3],
    /// Ignore lighting (for a mouth interior that should stay dark).
    pub unlit: bool,
}

impl Default for PropSpec {
    fn default() -> Self {
        Self {
            bone: String::new(),
            shape: PropShape::Sphere,
            center: [0.0; 3],
            size: [1.0; 3],
            color: [0.0; 3],
            unlit: false,
        }
    }
}

/// Marks a shape spawned from `[[props]]`.
#[derive(Component)]
struct Prop;

#[derive(Deserialize, Debug, Clone, Copy, Default)]
#[serde(default)]
pub struct BoneDelta {
    pub rotate: [f32; 3],
    pub translate: [f32; 3],
}

/// Share of the configured window height the avatar of mapping file `path` takes on the desktop (`window_height`), 1 if the file cannot be read.
pub fn window_share(path: &Path) -> f32 {
    std::fs::read_to_string(path).ok().and_then(|text| toml::from_str::<Mapping>(&text).ok()).map_or(1.0, |m| m.window_height.clamp(0.1, 1.0))
}

fn read_mapping(path: &Path) -> Option<Mapping> {
    let text = std::fs::read_to_string(path).map_err(|err| error!("mapping {}: {err}", path.display())).ok()?;
    toml::from_str(&text).map_err(|err| error!("mapping {}: {err}", path.display())).ok()
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
    /// Rest pose corrections from `[rest]`, applied right after the reset.
    rest_deltas: Vec<Delta>,
    /// Avatar-space transform of every node at load time (the rest pose), for placing props.
    rest_spaces: HashMap<Entity, Affine3A>,
    /// Materials with an emission texture, whose emission follows the expression `glow`, and its colour at weight 1.
    glowing: Vec<Handle<StandardMaterial>>,
    glow_color: LinearRgba,
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

    let Some(path) = model_asset_path(&config.avatar.path) else { return };
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

fn resolve_avatar(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    watch: Res<MappingWatch>,
    children: Query<&Children>,
    names: Query<&Name>,
    tree: TransformTree,
    surfaces: Query<(&Mesh3d, &MeshMaterial3d<StandardMaterial>)>,
    config: Res<Config>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
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

    humanoid.set_extent(nodes.values().copied(), &tree);

    let parts = Parts {
        bones: mapping.parts.iter().filter_map(|(part, node)| Some((part.clone(), lookup(node)?))).collect(),
    };

    let rest_spaces = nodes.values().map(|&e| (e, avatar_space(e, &tree))).collect();
    let mut rig = GlbRig {
        nodes,
        driven: HashMap::new(),
        expressions: HashMap::new(),
        rest_deltas: Vec::new(),
        rest_spaces,
        glowing: Vec::new(),
        glow_color: {
            let [r, g, b] = mapping.material.emission_color;
            LinearRgba::rgb(r, g, b)
        },
    };
    rig.driven.extend(humanoid.bones.values().chain(parts.bones.values()).map(|b| (b.entity, *b)));
    rig.set_expressions(&mapping, &tree);
    rig.spawn_props(&mapping, &mut commands, &mut meshes, &mut materials);
    let model = config.avatar.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let surfaces = children.iter_descendants(root).filter_map(|e| surfaces.get(e).ok()).map(|(mesh, material)| (mesh.0.clone(), material.0.clone()));
    rig.glowing = apply_material(&mapping.material, &model, surfaces, &asset_server, &mut meshes, &mut materials);
    spawn_clothing(&mapping.clothing, &model, mapping.scale, root, &mut commands, &asset_server);

    let mut avatar = commands.entity(root);
    avatar.insert((rig, parts, AvatarReady));
    match mapping.machine {
        Some(machine) => {
            avatar.insert(machine);
        }
        None if !humanoid.bones.is_empty() => {
            avatar.insert(humanoid);
        }
        None => {}
    }
}

/// Lays the textures of `spec` onto the materials of `surfaces` (mesh and material of each) and returns the materials that got an emission texture.
fn apply_material(
    spec: &MaterialSpec,
    model: &str,
    surfaces: impl Iterator<Item = (Handle<Mesh>, Handle<StandardMaterial>)>,
    asset_server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Vec<Handle<StandardMaterial>> {
    let load = |file: &Option<String>, srgb: bool| {
        file.as_ref().map(|file| {
            let path = AssetPath::from_path(&Path::new(model).join(file)).into_owned().with_source(AssetSourceId::from(SOURCE));
            asset_server.load_builder().with_settings(move |s: &mut ImageLoaderSettings| s.is_srgb = srgb).load::<Image>(path)
        })
    };
    let base_color = load(&spec.base_color, true);
    let orm = load(&spec.orm, false);
    let normal = load(&spec.normal, false);
    let emission = load(&spec.emission, true);
    let mut glowing = Vec::new();
    for (mesh, handle) in surfaces {
        if normal.is_some()
            && let Some(mut mesh) = meshes.get_mut(&mesh)
            && let Err(err) = mesh.generate_tangents()
        {
            warn!("material: no tangents for the normal map: {err}");
        }
        let Some(mut material) = materials.get_mut(&handle) else { continue };
        if base_color.is_some() {
            material.base_color = Color::WHITE;
            material.base_color_texture = base_color.clone();
        }
        if orm.is_some() {
            material.metallic = 1.0;
            material.perceptual_roughness = 1.0;
            material.metallic_roughness_texture = orm.clone();
            material.occlusion_texture = orm.clone();
        }
        if normal.is_some() {
            material.normal_map_texture = normal.clone();
        }
        if emission.is_some() {
            material.emissive_texture = emission.clone();
            glowing.push(handle.clone());
        }
    }
    glowing
}

impl GlbRig {
    /// Avatar-space transform of `entity` in the rest pose, identity for an unknown entity.
    pub(super) fn rest_space(&self, entity: Entity) -> Affine3A {
        self.rest_spaces.get(&entity).copied().unwrap_or(Affine3A::IDENTITY)
    }

    fn spawn_props(&self, mapping: &Mapping, commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
        for prop in &mapping.props {
            let Some((&bone, &space)) = self.nodes.get(&prop.bone).and_then(|e| Some((e, self.rest_spaces.get(e)?))) else {
                warn!("prop: bone '{}' not found in the model", prop.bone);
                continue;
            };
            let placement = Affine3A::from_scale_rotation_translation(Vec3::from(prop.size), Quat::IDENTITY, Vec3::from(prop.center));
            let mesh = match prop.shape {
                PropShape::Sphere => meshes.add(Sphere::new(1.0).mesh().uv(32, 18)),
                PropShape::Cube => meshes.add(Cuboid::new(2.0, 2.0, 2.0)),
            };
            let [r, g, b] = prop.color;
            let material = materials.add(StandardMaterial {
                base_color: Color::linear_rgb(r, g, b),
                unlit: prop.unlit,
                perceptual_roughness: 0.6,
                ..default()
            });
            commands.spawn((
                Prop,
                Name::new(format!("prop on {}", prop.bone)),
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_matrix(Mat4::from(space.inverse() * placement)),
                bevy::light::NotShadowCaster,
                ChildOf(bone),
            ));
        }
    }

    fn set_expressions(&mut self, mapping: &Mapping, tree: &TransformTree) {
        self.rest_deltas = self.deltas("rest", &mapping.rest, tree);
        self.expressions = mapping.expressions.iter().map(|(name, deltas)| (name.parse().unwrap(), self.deltas(name, deltas, tree))).collect();
    }

    /// Resolves node names to bones, which from now on are reset to rest each frame.
    fn deltas(&mut self, context: &str, deltas: &HashMap<String, BoneDelta>, tree: &TransformTree) -> Vec<Delta> {
        let mut resolved = Vec::new();
        for (node, delta) in deltas {
            let Some(&entity) = self.nodes.get(node) else {
                warn!("{context}: node '{node}' not found in the model");
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
        resolved
    }
}

impl Delta {
    fn apply(&self, transforms: &mut Query<&mut Transform>, weight: f32) {
        let Ok(mut t) = transforms.get_mut(self.bone.entity) else { return };
        let angle = self.rotate.length() * weight;
        if angle > 0.0 {
            self.bone.rotate(&mut t, Quat::from_axis_angle(self.rotate.normalize(), angle));
        }
        self.bone.translate(&mut t, self.translate * weight);
    }
}

fn reset_bones(rigs: Query<&GlbRig>, mut transforms: Query<&mut Transform>) {
    for rig in &rigs {
        for bone in rig.driven.values() {
            if let Ok(mut t) = transforms.get_mut(bone.entity) {
                *t = bone.rest;
            }
        }
        for delta in &rig.rest_deltas {
            delta.apply(&mut transforms, 1.0);
        }
    }
}

fn apply_expressions(rigs: Query<(&GlbRig, &Expressions)>, mut transforms: Query<&mut Transform>, mut materials: ResMut<Assets<StandardMaterial>>) {
    for (rig, expressions) in &rigs {
        let glow = rig.glow_color * expressions.get(ExpressionName::Custom("glow".into()));
        for handle in &rig.glowing {
            if materials.get(handle).is_some_and(|m| m.emissive != glow)
                && let Some(mut material) = materials.get_mut(handle)
            {
                material.emissive = glow;
            }
        }
        for (name, &weight) in &expressions.weights {
            if weight <= 0.0 {
                continue;
            }
            let Some(deltas) = rig.expressions.get(name) else { continue };
            for delta in deltas {
                delta.apply(&mut transforms, weight);
            }
        }
    }
}

fn reload_mapping(
    time: Res<Time>,
    watch: Option<ResMut<MappingWatch>>,
    mut rigs: Query<&mut GlbRig>,
    tree: TransformTree,
    mut commands: Commands,
    props: Query<Entity, With<Prop>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
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
    info!("reloaded {} (expressions and props; bones and scale need a restart)", watch.path.display());
    for entity in &props {
        commands.entity(entity).despawn();
    }
    for mut rig in &mut rigs {
        rig.set_expressions(&mapping, &tree);
        rig.spawn_props(&mapping, &mut commands, &mut meshes, &mut materials);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_mappings_read_as_a_humanoid_and_a_machine() {
        let humanoid: Mapping = toml::from_str(include_str!("../../assets/avatars/cyborg_girl.toml")).unwrap();
        assert!(humanoid.machine.is_none() && !humanoid.humanoid.is_empty());
        let machine: Mapping = toml::from_str(include_str!("../../assets/avatars/Ballytic813c_Reforged_ByLio3DW.toml")).unwrap();
        let motion = machine.machine.unwrap();
        assert_eq!(machine.parts[&motion.body], "MassCenterControl");
        assert!(motion.spin.keys().all(|part| machine.parts.contains_key(part)));
        assert!(machine.material.emission.is_some());
    }
}
