//! Backend-neutral avatar interface in VRM 1.0 terms.
//!
//! Idle, gaze and pose code address bones only through [`HumanoidBone`]; lip sync and blinking set only [`Expressions`] weights. A backend resolves these names on its model: `glb` through a mapping file, `vrm` (Cargo feature `vrm`) from the VRM file itself.
//!
//! An avatar is a humanoid ([`Humanoid`]) or a machine ([`Machine`]): a machine has named [`Parts`] instead of humanoid bones, which `animation::machine` floats and spins, and it speaks through the expressions `open` (the body parts apart) and `glow` (its lights).
//!
//! Per frame the systems run in [`AvatarSet`] order: the backend resets every driven bone to its rest pose, pose code (idle) adds rotations, expression code sets weights, and the backend applies the weights.

pub mod clothing;
pub mod glb;
#[cfg(feature = "vrm")]
pub mod vrm;

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use bevy::asset::AssetPath;
use bevy::asset::io::{AssetSourceBuilder, AssetSourceId};
use bevy::math::Affine3A;
use bevy::prelude::*;

/// Asset source id under which the model's directory is registered.
pub(crate) const SOURCE: &str = "avatar";

/// Registers the model's directory as asset source; must run before `DefaultPlugins` is added.
pub fn register_model_source(app: &mut App, model: &Path) {
    let dir = model.parent().map(Path::to_path_buf).unwrap_or_default();
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    app.register_asset_source(SOURCE, AssetSourceBuilder::platform_default(&dir.to_string_lossy(), None));
}

/// Asset path of the model file `model` in the registered model source.
pub(crate) fn model_asset_path(model: &Path) -> Option<AssetPath<'static>> {
    let Some(file) = model.file_name() else {
        error!("avatar path {} has no file name", model.display());
        return None;
    };
    Some(AssetPath::from_path(Path::new(file)).into_owned().with_source(AssetSourceId::from(SOURCE)))
}

macro_rules! names {
    ($(#[$meta:meta])* $vis:vis enum $name:ident { $($variant:ident => $text:literal),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        $vis enum $name { $($variant),* }

        impl $name {
            pub const ALL: &[$name] = &[$($name::$variant),*];

            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text),* }
            }
        }

        impl FromStr for $name {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, String> {
                match s { $($text => Ok($name::$variant),)* _ => Err(format!("unknown {}: {s}", stringify!($name))) }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { f.write_str(self.as_str()) }
        }
    };
}

names! {
    /// Humanoid bones of the VRM 1.0 specification (`VRMC_vrm` humanoid).
    pub enum HumanoidBone {
        Hips => "hips", Spine => "spine", Chest => "chest", UpperChest => "upperChest", Neck => "neck", Head => "head",
        LeftEye => "leftEye", RightEye => "rightEye", Jaw => "jaw",
        LeftUpperLeg => "leftUpperLeg", LeftLowerLeg => "leftLowerLeg", LeftFoot => "leftFoot", LeftToes => "leftToes",
        RightUpperLeg => "rightUpperLeg", RightLowerLeg => "rightLowerLeg", RightFoot => "rightFoot", RightToes => "rightToes",
        LeftShoulder => "leftShoulder", LeftUpperArm => "leftUpperArm", LeftLowerArm => "leftLowerArm", LeftHand => "leftHand",
        RightShoulder => "rightShoulder", RightUpperArm => "rightUpperArm", RightLowerArm => "rightLowerArm", RightHand => "rightHand",
        LeftThumbMetacarpal => "leftThumbMetacarpal", LeftThumbProximal => "leftThumbProximal", LeftThumbDistal => "leftThumbDistal",
        LeftIndexProximal => "leftIndexProximal", LeftIndexIntermediate => "leftIndexIntermediate", LeftIndexDistal => "leftIndexDistal",
        LeftMiddleProximal => "leftMiddleProximal", LeftMiddleIntermediate => "leftMiddleIntermediate", LeftMiddleDistal => "leftMiddleDistal",
        LeftRingProximal => "leftRingProximal", LeftRingIntermediate => "leftRingIntermediate", LeftRingDistal => "leftRingDistal",
        LeftLittleProximal => "leftLittleProximal", LeftLittleIntermediate => "leftLittleIntermediate", LeftLittleDistal => "leftLittleDistal",
        RightThumbMetacarpal => "rightThumbMetacarpal", RightThumbProximal => "rightThumbProximal", RightThumbDistal => "rightThumbDistal",
        RightIndexProximal => "rightIndexProximal", RightIndexIntermediate => "rightIndexIntermediate", RightIndexDistal => "rightIndexDistal",
        RightMiddleProximal => "rightMiddleProximal", RightMiddleIntermediate => "rightMiddleIntermediate", RightMiddleDistal => "rightMiddleDistal",
        RightRingProximal => "rightRingProximal", RightRingIntermediate => "rightRingIntermediate", RightRingDistal => "rightRingDistal",
        RightLittleProximal => "rightLittleProximal", RightLittleIntermediate => "rightLittleIntermediate", RightLittleDistal => "rightLittleDistal",
    }
}

names! {
    /// Expression presets of the VRM 1.0 specification (`VRMC_vrm` expressions).
    pub enum ExpressionPreset {
        Happy => "happy", Angry => "angry", Sad => "sad", Relaxed => "relaxed", Surprised => "surprised",
        Aa => "aa", Ih => "ih", Ou => "ou", Ee => "ee", Oh => "oh",
        Blink => "blink", BlinkLeft => "blinkLeft", BlinkRight => "blinkRight",
        LookUp => "lookUp", LookDown => "lookDown", LookLeft => "lookLeft", LookRight => "lookRight",
        Neutral => "neutral",
    }
}

impl ExpressionPreset {
    /// The five mouth shapes (visemes) in the order of the debug keys 1–5.
    pub const VISEMES: [ExpressionPreset; 5] = [Self::Aa, Self::Ih, Self::Ou, Self::Ee, Self::Oh];
}

/// A preset or a model-specific custom expression.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ExpressionName {
    Preset(ExpressionPreset),
    Custom(String),
}

impl From<ExpressionPreset> for ExpressionName {
    fn from(p: ExpressionPreset) -> Self {
        Self::Preset(p)
    }
}

impl FromStr for ExpressionName {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(s.parse().map(Self::Preset).unwrap_or_else(|_| Self::Custom(s.to_owned())))
    }
}

impl fmt::Display for ExpressionName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Preset(p) => f.write_str(p.as_str()),
            Self::Custom(s) => f.write_str(s),
        }
    }
}

/// A bone that pose code may move, with what is needed to move it in avatar space.
///
/// Avatar space is the model's own space below the avatar root: +Y up, +Z the direction the figure faces, +X the figure's left. Rotations and offsets given in avatar space are converted into the bone's parent space, using the parent's rest orientation (good enough for the small idle and face motions).
#[derive(Clone, Copy, Debug)]
pub struct PoseBone {
    pub entity: Entity,
    pub rest: Transform,
    /// Rest position of the bone in avatar space.
    pub position: Vec3,
    /// Rest rotation of the parent in avatar space.
    parent_rotation: Quat,
    /// Uniform scale of the parent in avatar space.
    parent_scale: f32,
}

impl PoseBone {
    pub fn new(entity: Entity, rest: Transform, parent: Affine3A) -> Self {
        let (scale, rotation, _) = parent.to_scale_rotation_translation();
        Self {
            entity,
            rest,
            position: parent.transform_point3(rest.translation),
            parent_rotation: rotation,
            parent_scale: scale.x,
        }
    }

    /// Converts a rotation given in avatar space into the parent's space.
    pub fn local_rotation(&self, rotation: Quat) -> Quat {
        self.parent_rotation.inverse() * rotation * self.parent_rotation
    }

    /// Converts an offset given in avatar space into the parent's space.
    pub fn local_offset(&self, offset: Vec3) -> Vec3 {
        self.parent_rotation.inverse() * offset / self.parent_scale
    }

    /// Rotates the bone about its own pivot by `rotation` (avatar space), on top of what was applied this frame.
    pub fn rotate(&self, transform: &mut Transform, rotation: Quat) {
        transform.rotation = self.local_rotation(rotation) * transform.rotation;
    }

    pub fn translate(&self, transform: &mut Transform, offset: Vec3) {
        transform.translation += self.local_offset(offset);
    }
}

/// The resolved humanoid skeleton, on the avatar root entity.
#[derive(Component, Debug, Default)]
pub struct Humanoid {
    pub bones: HashMap<HumanoidBone, PoseBone>,
    /// Height of the figure in avatar-space units (floor to top of head), for scaling motion amplitudes.
    pub height: f32,
    /// Lowest point of the figure in avatar space (the floor).
    pub floor: f32,
}

impl Humanoid {
    pub fn get(&self, bone: HumanoidBone) -> Option<&PoseBone> {
        self.bones.get(&bone)
    }

    /// Rotates a bone in avatar space if the model has it.
    pub fn rotate(&self, bone: HumanoidBone, rotation: Quat, transforms: &mut Query<&mut Transform>) {
        if let Some(b) = self.bones.get(&bone)
            && let Ok(mut t) = transforms.get_mut(b.entity)
        {
            b.rotate(&mut t, rotation);
        }
    }

    pub fn translate(&self, bone: HumanoidBone, offset: Vec3, transforms: &mut Query<&mut Transform>) {
        if let Some(b) = self.bones.get(&bone)
            && let Ok(mut t) = transforms.get_mut(b.entity)
        {
            b.translate(&mut t, offset);
        }
    }

    /// Sets height and floor from the joint positions of `nodes` (avatar space).
    pub fn set_extent(&mut self, nodes: impl Iterator<Item = Entity>, tree: &TransformTree) {
        let ys: Vec<f32> = nodes.filter_map(|e| pose_bone(e, tree)).map(|b| b.position.y).collect();
        self.floor = ys.iter().copied().fold(f32::MAX, f32::min);
        self.height = ys.iter().copied().fold(f32::MIN, f32::max) - self.floor;
    }

    /// Avatar-space rest position of a bone.
    pub fn position(&self, bone: HumanoidBone) -> Option<Vec3> {
        self.bones.get(&bone).map(|b| b.position)
    }
}

/// Named non-humanoid parts of the avatar (mapping file: `[parts]`), on the avatar root entity.
#[derive(Component, Debug, Default)]
pub struct Parts {
    pub bones: HashMap<String, PoseBone>,
}

/// How a machine avatar moves (mapping file: `[machine]`), on the avatar root entity. Distances are in model units, angles in degrees, rates in Hz.
#[derive(Component, serde::Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Machine {
    /// The part that floats and wobbles, carrying the others.
    pub body: String,
    /// Height the body rises and sinks by while floating.
    pub float: f32,
    pub float_rate: f32,
    /// Tilt of the body while floating.
    pub wobble: f32,
    /// Turn of the body as an axis-angle vector in degrees, applied before everything else, e.g. `[90, 0, 0]` shows the viewer what faced up; floating stays vertical, spins stay about the body's own axis.
    pub facing: [f32; 3],
    /// Parts not shown, such as a cap over what should be seen.
    pub hidden: Vec<String>,
    /// Spin of parts about the vertical axis in degrees per second, positive anticlockwise seen from above.
    pub spin: HashMap<String, f32>,
    /// Spin while speaking, for the parts listed; the others keep `spin`.
    pub speaking_spin: HashMap<String, f32>,
    /// Seconds the parts take to open when speech starts, and to close after it.
    pub open_time: f32,
    /// Weight of `glow` while silent; it breathes slowly around this.
    pub rest_glow: f32,
    /// Weight of `glow` per unit of the voice's RMS level (full scale = 1).
    pub glow_gain: f32,
    /// Strength of the halo around what glows (0 = none, about 0.3 strong); only light brighter than white spreads.
    pub bloom: f32,
    /// A light at the glowing part that shines on the parts around it, as bright as `glow` says.
    pub light: Option<MachineLight>,
}

/// A point light carried by a machine's part (mapping file: `[machine.light]`).
#[derive(serde::Deserialize, Debug, Clone)]
#[serde(default)]
pub struct MachineLight {
    pub part: String,
    /// Position relative to the part, in the part's own axes and model units.
    pub offset: [f32; 3],
    /// Linear RGB.
    pub color: [f32; 3],
    /// Luminous power in lumens at `glow` weight 1.
    pub lumens: f32,
    /// Distance in metres beyond which it lights nothing.
    pub range: f32,
}

impl Default for MachineLight {
    fn default() -> Self {
        Self { part: "body".into(), offset: [0.0; 3], color: [1.0; 3], lumens: 100_000.0, range: 5.0 }
    }
}

impl Default for Machine {
    fn default() -> Self {
        Self {
            body: "body".into(),
            float: 0.0,
            float_rate: 0.2,
            wobble: 0.0,
            facing: [0.0; 3],
            hidden: Vec::new(),
            spin: HashMap::new(),
            speaking_spin: HashMap::new(),
            open_time: 0.3,
            rest_glow: 0.3,
            glow_gain: 6.0,
            bloom: 0.0,
            light: None,
        }
    }
}

/// Expression weights from 0 to 1, set each frame by blink and mouth code and applied by the backend.
#[derive(Component, Debug, Default)]
pub struct Expressions {
    pub weights: HashMap<ExpressionName, f32>,
}

impl Expressions {
    pub fn set(&mut self, name: impl Into<ExpressionName>, weight: f32) {
        self.weights.insert(name.into(), weight.clamp(0.0, 1.0));
    }

    pub fn get(&self, name: impl Into<ExpressionName>) -> f32 {
        self.weights.get(&name.into()).copied().unwrap_or(0.0)
    }
}

/// Marks the entity the model scene is spawned under.
#[derive(Component)]
pub struct AvatarRoot;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum AvatarSet {
    /// The backend puts every driven bone back to its rest pose.
    Reset,
    /// Pose code (idle) rotates humanoid bones.
    Pose,
    /// Blink and mouth code set expression weights.
    Express,
    /// Weights forced from `[debug] expressions` in `riggy.toml` override them.
    Override,
    /// The backend turns expression weights into bone or morph changes.
    Apply,
}

pub struct AvatarPlugin;

impl Plugin for AvatarPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(Update, (AvatarSet::Reset, AvatarSet::Pose, AvatarSet::Express, AvatarSet::Override, AvatarSet::Apply).chain())
            .add_systems(Update, force_expressions.in_set(AvatarSet::Override))
            .add_plugins(glb::GlbAvatarPlugin);
        #[cfg(feature = "vrm")]
        app.add_plugins(vrm::VrmAvatarPlugin);
    }
}

fn force_expressions(config: Res<crate::config::Config>, mut avatars: Query<&mut Expressions>) {
    if config.debug.expressions.is_empty() {
        return;
    }
    for mut expressions in &mut avatars {
        for (name, &weight) in &config.debug.expressions {
            expressions.set(name.parse::<ExpressionName>().unwrap(), weight);
        }
    }
}

pub(crate) type TransformTree<'w, 's> = Query<'w, 's, (&'static Transform, Option<&'static ChildOf>, Has<AvatarRoot>)>;

/// The bone `entity` with its current transform as rest pose.
pub(crate) fn pose_bone(entity: Entity, tree: &TransformTree) -> Option<PoseBone> {
    let (transform, parent, _) = tree.get(entity).ok()?;
    let parent_space = parent.map(|p| avatar_space(p.parent(), tree)).unwrap_or(Affine3A::IDENTITY);
    Some(PoseBone::new(entity, *transform, parent_space))
}

/// Avatar-space transform of `entity`: the product of local transforms from below the avatar root down to it.
pub fn avatar_space(entity: Entity, transforms: &TransformTree) -> Affine3A {
    let mut affine = Affine3A::IDENTITY;
    let mut current = entity;
    while let Ok((transform, parent, is_root)) = transforms.get(current) {
        if is_root {
            break;
        }
        affine = transform.compute_affine() * affine;
        match parent {
            Some(p) => current = p.parent(),
            None => break,
        }
    }
    affine
}
