//! Backend-neutral avatar interface in VRM 1.0 terms.
//!
//! Idle, gaze and pose code address bones only through [`HumanoidBone`]; lip sync and blinking set only [`Expressions`] weights. A backend (`glb` now, `vrm` later) resolves these names on its model.
//!
//! Per frame the systems run in [`AvatarSet`] order: the backend resets every driven bone to its rest pose, pose code (idle) adds rotations, expression code sets weights, and the backend applies the weights.

pub mod glb;

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use bevy::math::Affine3A;
use bevy::prelude::*;

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

    /// Avatar-space rest position of a bone.
    pub fn position(&self, bone: HumanoidBone) -> Option<Vec3> {
        self.bones.get(&bone).map(|b| b.position)
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
    /// The backend turns expression weights into bone or morph changes.
    Apply,
}

pub struct AvatarPlugin;

impl Plugin for AvatarPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(Update, (AvatarSet::Reset, AvatarSet::Pose, AvatarSet::Express, AvatarSet::Apply).chain())
            .add_plugins(glb::GlbAvatarPlugin);
    }
}

/// Avatar-space transform of `entity`: the product of local transforms from below the avatar root down to it.
pub fn avatar_space(entity: Entity, transforms: &Query<(&Transform, Option<&ChildOf>, Has<AvatarRoot>)>) -> Affine3A {
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
