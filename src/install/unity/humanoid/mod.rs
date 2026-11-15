//! Unity humanoid poses (muscle values and the body rotation) to bone rotations on riggy's fixed Unity rig (`rig.json`), in Unity space.
//!
//! Ported from unity-asset-reader's `humanoid.ts` and `muscle.ts` (MIT, see `LICENSE` and `README.md` here). A bone's local rotation is `PreQ · swingTwist(sign · angles) · PostQ⁻¹`, each angle its muscle's share of the axis' limit; the arm and leg twists are split between a bone and its child; the Hips turn so that the body frame (up from the upper legs to the upper arms, right from left to right) has the pose's body rotation.
//!
//! Sources (Unity's documentation, as the port cites them):
//! - Muscle definitions: https://docs.unity3d.com/Manual/MuscleDefinitions.html
//! - HumanPose: https://docs.unity3d.com/ScriptReference/HumanPose.html
//! - HumanLimit: https://docs.unity3d.com/ScriptReference/HumanLimit.html

use std::sync::OnceLock;

use bevy::math::{DQuat, DVec3};
use serde::Deserialize;

/// The 95 muscles in `HumanTrait.MuscleName` order, as an `.anim` file names their curves: the body's 55, then 20 per hand.
pub fn muscle_index(name: &str) -> Option<usize> {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    let names = NAMES.get_or_init(|| {
        let mut names: Vec<String> = BODY_MUSCLES.iter().map(|n| n.to_string()).collect();
        for hand in ["LeftHand", "RightHand"] {
            for finger in ["Thumb", "Index", "Middle", "Ring", "Little"] {
                for dof in ["1 Stretched", "Spread", "2 Stretched", "3 Stretched"] {
                    names.push(format!("{hand}.{finger}.{dof}"));
                }
            }
        }
        names
    });
    names.iter().position(|n| n == name)
}

pub const MUSCLE_COUNT: usize = 95;

const BODY_MUSCLES: [&str; 55] = [
    "Spine Front-Back", "Spine Left-Right", "Spine Twist Left-Right",
    "Chest Front-Back", "Chest Left-Right", "Chest Twist Left-Right",
    "UpperChest Front-Back", "UpperChest Left-Right", "UpperChest Twist Left-Right",
    "Neck Nod Down-Up", "Neck Tilt Left-Right", "Neck Turn Left-Right",
    "Head Nod Down-Up", "Head Tilt Left-Right", "Head Turn Left-Right",
    "Left Eye Down-Up", "Left Eye In-Out", "Right Eye Down-Up", "Right Eye In-Out",
    "Jaw Close", "Jaw Left-Right",
    "Left Upper Leg Front-Back", "Left Upper Leg In-Out", "Left Upper Leg Twist In-Out",
    "Left Lower Leg Stretch", "Left Lower Leg Twist In-Out",
    "Left Foot Up-Down", "Left Foot Twist In-Out", "Left Toes Up-Down",
    "Right Upper Leg Front-Back", "Right Upper Leg In-Out", "Right Upper Leg Twist In-Out",
    "Right Lower Leg Stretch", "Right Lower Leg Twist In-Out",
    "Right Foot Up-Down", "Right Foot Twist In-Out", "Right Toes Up-Down",
    "Left Shoulder Down-Up", "Left Shoulder Front-Back",
    "Left Arm Down-Up", "Left Arm Front-Back", "Left Arm Twist In-Out",
    "Left Forearm Stretch", "Left Forearm Twist In-Out", "Left Hand Down-Up", "Left Hand In-Out",
    "Right Shoulder Down-Up", "Right Shoulder Front-Back",
    "Right Arm Down-Up", "Right Arm Front-Back", "Right Arm Twist In-Out",
    "Right Forearm Stretch", "Right Forearm Twist In-Out", "Right Hand Down-Up", "Right Hand In-Out",
];

/// Each human bone's muscles by axis (x, y, z), `None` where the axis has none; by human bone (Hips, LeftUpperLeg, RightUpperLeg, LeftLowerLeg, RightLowerLeg, LeftFoot, RightFoot, Spine, Chest, UpperChest, Neck, Head, LeftShoulder, RightShoulder, LeftUpperArm, RightUpperArm, LeftLowerArm, RightLowerArm, LeftHand, RightHand, LeftToes, RightToes, LeftEye, RightEye, Jaw).
const BONE_MUSCLES: [[i8; 3]; 25] = [
    [-1, -1, -1],
    [23, 22, 21],
    [31, 30, 29],
    [25, -1, 24],
    [33, -1, 32],
    [-1, 27, 26],
    [-1, 35, 34],
    [2, 1, 0],
    [5, 4, 3],
    [8, 7, 6],
    [11, 10, 9],
    [14, 13, 12],
    [-1, 38, 37],
    [-1, 47, 46],
    [41, 40, 39],
    [50, 49, 48],
    [43, -1, 42],
    [52, -1, 51],
    [-1, 45, 44],
    [-1, 54, 53],
    [-1, -1, 28],
    [-1, -1, 36],
    [-1, 16, 15],
    [-1, 18, 17],
    [-1, 20, 19],
];
const HIPS: usize = 0;
const LEFT_UPPER_LEG: usize = 1;
const RIGHT_UPPER_LEG: usize = 2;
const CHEST: usize = 8;
const UPPER_CHEST: usize = 9;
const LEFT_UPPER_ARM: usize = 14;
const RIGHT_UPPER_ARM: usize = 15;
/// The arm and leg bones whose twist is shared with their child: (bone, child).
const TWISTS: [(usize, usize); 8] = [(14, 16), (15, 17), (16, 18), (17, 19), (1, 3), (2, 4), (3, 5), (4, 6)];
/// The share of its twist a bone keeps (HumanDescription's twist settings, Unity's default for all four).
const TWIST_SHARE: f64 = 0.5;
/// A finger bone's muscles by axis, as an offset from the finger's first muscle, by phalanx.
const FINGER_MUSCLES: [[i8; 3]; 3] = [[-1, 1, 0], [-1, -1, 2], [-1, -1, 3]];
const FIRST_FINGER_MUSCLE: [usize; 2] = [55, 75];

/// A bone's muscle axes: the frames around its swing-twist rotation, the signs and the limits per axis (radians).
#[derive(Deserialize)]
struct Axes {
    pre: [f64; 4],
    post: [f64; 4],
    sgn: [f64; 3],
    min: [f64; 3],
    max: [f64; 3],
}

#[derive(Deserialize)]
pub struct Node {
    pub name: String,
    pub parent: i32,
    pub t: [f64; 3],
    pub q: [f64; 4],
    axes: Option<Axes>,
}

/// riggy's Unity rig: see `README.md`.
#[derive(Deserialize)]
pub struct Rig {
    /// Node index per human bone, -1 where the rig has none.
    human: [i32; 25],
    /// Node index of each hand's 15 finger bones (thumb, index, middle, ring, little; proximal to distal).
    hands: [[usize; 15]; 2],
    /// The body frame of the rig at rest (`m_RootX`).
    root_x: [f64; 4],
    pub nodes: Vec<Node>,
}

impl Rig {
    pub fn get() -> &'static Rig {
        static RIG: OnceLock<Rig> = OnceLock::new();
        RIG.get_or_init(|| serde_json::from_str(include_str!("rig.json")).expect("rig.json is valid"))
    }

    fn node_of(&self, bone: usize) -> Option<usize> {
        usize::try_from(self.human[bone]).ok()
    }

    fn axes_of(&self, bone: usize) -> Option<&Axes> {
        self.node_of(bone).and_then(|n| self.nodes[n].axes.as_ref())
    }

    /// Each node's local rotation, in Unity space, for the 95 `muscles` and the body rotation `body` (the clip's `RootQ`).
    pub fn rotations(&self, muscles: &[f64; MUSCLE_COUNT], body: DQuat) -> Vec<DQuat> {
        let mut rotations: Vec<DQuat> = self.nodes.iter().map(|n| quat(n.q)).collect();

        let mut twist = [0.0; 25];
        for (bone, &ids) in BONE_MUSCLES.iter().enumerate() {
            let Some(axes) = self.axes_of(bone) else { continue };
            if ids.iter().all(|&m| m < 0) {
                continue;
            }
            let mut bone_angles = angles(axes, ids.map(muscle), 1.0, muscles);
            if bone == CHEST && self.node_of(UPPER_CHEST).is_none() {
                // An avatar without an UpperChest adds its muscles to the Chest's, over half the Chest's limit.
                let upper = angles(axes, BONE_MUSCLES[UPPER_CHEST].map(muscle), 0.5, muscles);
                bone_angles = std::array::from_fn(|d| bone_angles[d] + upper[d]);
            }
            twist[bone] = axes.sgn[0] * bone_angles[0];
            rotations[self.node_of(bone).unwrap()] = axes_rotation(axes, bone_angles);
        }

        for (hand, nodes) in self.hands.iter().enumerate() {
            for (i, &node) in nodes.iter().enumerate() {
                let Some(axes) = self.nodes[node].axes.as_ref() else { continue };
                let first = FIRST_FINGER_MUSCLE[hand] + 4 * (i / 3);
                let ids = FINGER_MUSCLES[i % 3].map(|m| muscle(m).map(|m| first + m));
                rotations[node] = axes_rotation(axes, angles(axes, ids, 1.0, muscles));
            }
        }

        for (bone, child) in TWISTS {
            let (Some(node), Some(child_node), Some(axes)) = (self.node_of(bone), self.node_of(child), self.axes_of(bone)) else { continue };
            if twist[bone] == 0.0 {
                continue;
            }
            let moved = (1.0 - TWIST_SHARE) * twist[bone];
            let post = quat(axes.post);
            let r = post * DQuat::from_xyzw((moved / 2.0).sin(), 0.0, 0.0, (moved / 2.0).cos()) * post.conjugate();
            rotations[node] = (rotations[node] * r.conjugate()).normalize();
            rotations[child_node] = (r * rotations[child_node]).normalize();
        }

        if let Some(hips) = self.node_of(HIPS) {
            rotations[hips] = DQuat::IDENTITY;
            let frame = self.body_frame(&self.hips_space(&rotations));
            // The body rotation is in the root's space; the Hips' is local to its parent, so divide by the rest rotations above it (the root excluded).
            let mut above = DQuat::IDENTITY;
            let mut at = self.nodes[hips].parent;
            while at >= 0 && self.nodes[at as usize].parent >= 0 {
                above = quat(self.nodes[at as usize].q) * above;
                at = self.nodes[at as usize].parent;
            }
            // Unity's body frame of this rig at rest is `root_x`, which the measured frame need not equal.
            let body = body.normalize() * quat(self.root_x);
            rotations[hips] = (above.conjugate() * body * frame.conjugate()).normalize();
        }
        rotations
    }

    /// Each node's position relative to the Hips at the local rotations `local`; `None` outside the Hips' subtree.
    fn hips_space(&self, local: &[DQuat]) -> Vec<Option<DVec3>> {
        let hips = self.node_of(HIPS).unwrap();
        let mut t: Vec<Option<DVec3>> = vec![None; self.nodes.len()];
        let mut q = vec![DQuat::IDENTITY; self.nodes.len()];
        for (i, node) in self.nodes.iter().enumerate() {
            if i == hips {
                t[i] = Some(DVec3::ZERO);
                continue;
            }
            let Ok(parent) = usize::try_from(node.parent) else { continue };
            let Some(base) = t[parent] else { continue };
            t[i] = Some(base + q[parent] * DVec3::from_array(node.t));
            q[i] = q[parent] * local[i];
        }
        t
    }

    /// The body frame: up from the middle of the upper legs to the middle of the upper arms, right from the left upper leg and arm to the right ones.
    fn body_frame(&self, t: &[Option<DVec3>]) -> DQuat {
        let at = |bone: usize| self.node_of(bone).and_then(|n| t[n]).unwrap_or(DVec3::ZERO);
        let legs = at(LEFT_UPPER_LEG) + at(RIGHT_UPPER_LEG);
        let arms = at(LEFT_UPPER_ARM) + at(RIGHT_UPPER_ARM);
        let up = (arms - legs).normalize();
        let across = (at(RIGHT_UPPER_LEG) - at(LEFT_UPPER_LEG)) + (at(RIGHT_UPPER_ARM) - at(LEFT_UPPER_ARM));
        let front = across.normalize().cross(up).normalize();
        DQuat::from_mat3(&bevy::math::DMat3::from_cols(up.cross(front), up, front))
    }
}

/// A muscle table entry: -1 for none.
fn muscle(id: i8) -> Option<usize> {
    usize::try_from(id).ok()
}

/// The angles per axis of the muscles `ids`, each over its axis' limit times `scale`.
fn angles(axes: &Axes, ids: [Option<usize>; 3], scale: f64, muscles: &[f64; MUSCLE_COUNT]) -> [f64; 3] {
    std::array::from_fn(|d| ids[d].map_or(0.0, |m| muscle_angle(muscles[m], scale * axes.min[d], scale * axes.max[d])))
}

/// A muscle value as an angle on one axis: 0 at the limit's centre, 1 at `max`, -1 at `min`.
fn muscle_angle(value: f64, min: f64, max: f64) -> f64 {
    if value >= 0.0 { value * max } else { -value * min }
}

/// Unity's swing-twist rotation of three axis angles: the X angle twists about the bone, then Y and Z swing it, each entering as the tangent of its half angle.
fn swing_twist(x: f64, y: f64, z: f64) -> DQuat {
    let (tx, ty, tz) = ((x / 2.0).tan(), (y / 2.0).tan(), (z / 2.0).tan());
    DQuat::from_xyzw(tx, ty + tx * tz, tz - tx * ty, 1.0).normalize()
}

fn axes_rotation(axes: &Axes, angles: [f64; 3]) -> DQuat {
    let r = swing_twist(axes.sgn[0] * angles[0], axes.sgn[1] * angles[1], axes.sgn[2] * angles[2]);
    (quat(axes.pre) * r * quat(axes.post).conjugate()).normalize()
}

fn quat(q: [f64; 4]) -> DQuat {
    DQuat::from_xyzw(q[0], q[1], q[2], q[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn muscles_are_named_as_unity_names_them() {
        assert_eq!(muscle_index("Spine Front-Back"), Some(0));
        assert_eq!(muscle_index("Right Hand In-Out"), Some(54));
        assert_eq!(muscle_index("LeftHand.Thumb.1 Stretched"), Some(55));
        assert_eq!(muscle_index("RightHand.Little.3 Stretched"), Some(94));
        assert_eq!(muscle_index("RootQ.x"), None);
    }

    #[test]
    fn the_rest_pose_keeps_the_rig_upright() {
        let rig = Rig::get();
        let rotations = rig.rotations(&[0.0; MUSCLE_COUNT], DQuat::IDENTITY);
        let hips = rig.node_of(HIPS).unwrap();
        assert!(rotations[hips].angle_between(DQuat::IDENTITY) < 1e-3);
    }

    #[test]
    fn a_single_axis_turns_by_its_angle() {
        let q = swing_twist(0.0, 0.0, 0.5);
        assert!((q.angle_between(DQuat::IDENTITY) - 0.5).abs() < 1e-12);
    }
}
