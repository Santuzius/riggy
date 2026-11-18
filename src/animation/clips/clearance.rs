//! Contact: the arms do not pass through the body. An actor with narrower hips or a slimmer trunk than the avatar lets the hands rest and the upper arms press where the avatar's body is; riggy lets them land on its surface instead.
//!
//! The avatar's body parts come from its mesh ([`riggy::clips::body`]). Every frame, after the whole pose is built (clips, riggy's own motion, the stance), each arm's parts (upper arm, forearm, hand, fingers) are checked against the rest of the body. Where a point of the arm lies behind the body's surface, the joint just above it turns just far enough to bring it back onto the surface, along the surface's outward direction there: a touching finger bends at its own joint, a hand at the wrist, each by up to about 20°, and the shoulder turns the whole arm for the rest and for the forearm and upper arm. Only the motion into the body is taken away, so a finger or hand slides over the surface. A few rounds settle a second point a turn pushed in. The correction grows from nothing as the arm reaches the surface, so it needs no looking ahead: while a clip pushes inwards, the hand slides along the body, and it lifts off as the clip moves away.

use std::collections::HashMap;
use std::path::Path;

use bevy::math::{Affine3A, Mat3};
use bevy::prelude::*;
use riggy::clips::body::Body;
use riggy::clips::rig::Rig;

use super::retarget::Retargeted;
use crate::avatar::{Humanoid, HumanoidBone as B};

/// Depth, in metres, that the arm may keep: the skin's give. Pushes start from it, so they grow from nothing. An upper arm resting against the body squashes more, and lifting it by a turn at the shoulder close by would swing the whole arm far.
const SKIN: f32 = 0.002;
const UPPER_ARM_SKIN: f32 = 0.012;
/// Keeps a joint's turn small where its contacts barely tell which way to turn (metres squared, about a 1 cm lever).
const STIFF: f32 = 1e-3;
/// Turns per arm and frame, and the largest of each, in radians.
const ROUNDS: usize = 3;
const MOST: f32 = 0.35;
/// How far a finger joint and the wrist may give way at a contact before the shoulder takes the rest, in radians (about 20°).
const FINGER: f32 = 0.35;
const WRIST: f32 = 0.35;
const ARMS: [B; 2] = [B::LeftUpperArm, B::RightUpperArm];
/// The adaptation of a clip: depth into the hips and legs (and of the upper arm into the trunk's side) left to the runtime contact, in metres; the largest swing, in radians; and the seconds it moves out before such a contact and back after it.
pub const ADAPT_ALLOWED: f32 = 0.01;
const ADAPT_MOST: f32 = 0.7;
const RAMP: f32 = 0.3;
/// The parts a clip is adapted to keep the arms out of, and those only the upper arm is kept out of (a hand or forearm in front of the chest is a gesture a swing to the side would spoil).
const OBSTACLES: [&str; 5] = ["hips", "leftUpperLeg", "rightUpperLeg", "leftLowerLeg", "rightLowerLeg"];
const FLANKS: [&str; 3] = ["spine", "chest", "upperChest"];
const HANDS: [B; 2] = [B::LeftHand, B::RightHand];

/// The rig of the avatar file `path`: a VRM's own, or a GLB's with riggy's mapping file of the same name.
pub fn load_rig(path: &Path) -> Result<Rig, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("{}: {err}", path.display()))?;
    if path.extension().is_some_and(|e| e == "vrm") {
        return Rig::from_vrm(&bytes);
    }
    let name = path.file_stem().and_then(|s| s.to_str()).ok_or("a model file without a name")?;
    let mapping_path = riggy::files::data_dir().join("avatars").join(format!("{name}.toml"));
    let text = std::fs::read_to_string(&mapping_path).map_err(|err| format!("{}: {err}", mapping_path.display()))?;
    let mapping: crate::avatar::glb::Mapping = toml::from_str(&text).map_err(|err| format!("{}: {err}", mapping_path.display()))?;
    let rest = mapping.rest.iter().map(|(n, d)| (n.clone(), Vec3::from(d.rotate))).collect();
    Rig::from_glb(&bytes, &mapping.humanoid, mapping.scale, &rest)
}

/// Humanoid bone → node in `rig`.
fn nodes_of(rig: &Rig) -> HashMap<B, usize> {
    rig.humanoid.iter().filter_map(|(name, &node)| Some((name.parse::<B>().ok()?, node))).collect()
}

/// An avatar's rig and body, for keeping the arms out of it.
pub struct Clearance {
    rig: Rig,
    body: Body,
    rest: Vec<Affine3A>,
    /// Humanoid bone → node in the rig.
    pub nodes: HashMap<B, usize>,
    /// For each arm, the pairs of one of its parts with a part outside both arms.
    pairs: [Vec<(usize, usize)>; 2],
    /// For each arm, every node from the upper arm down.
    arm_nodes: [Vec<usize>; 2],
    /// Node → humanoid bone.
    bones: HashMap<usize, B>,
    /// For each arm, the pairs a clip is adapted for: its parts with the hips and legs, its upper arm with the trunk's side.
    adapt: [Vec<(usize, usize)>; 2],
}

impl Clearance {
    /// The clearance of the avatar file `path`.
    pub fn load(path: &Path) -> Result<Self, String> {
        let rig = load_rig(path)?;
        let body = Body::new(&std::fs::read(path).map_err(|err| err.to_string())?, &rig)?;
        Ok(Self::new(rig, body))
    }

    pub fn new(rig: Rig, body: Body) -> Self {
        let rest = rig.worlds(&HashMap::new());
        let nodes = nodes_of(&rig);
        let below = |node: usize, top: usize| {
            let mut n = Some(node);
            while let Some(x) = n {
                if x == top {
                    return true;
                }
                n = rig.parents[x];
            }
            false
        };
        let arm_nodes: [Vec<usize>; 2] = std::array::from_fn(|side| nodes.get(&ARMS[side]).map_or(Vec::new(), |&top| (0..rig.parents.len()).filter(|&n| below(n, top)).collect()));
        let in_arm = |part: usize, side: usize| arm_nodes[side].contains(&body.parts[part].node);
        let all = body.pairs();
        let pairs = std::array::from_fn(|side| {
            all.iter()
                .filter_map(|&(a, b)| {
                    let outside = |x: usize| !in_arm(x, 0) && !in_arm(x, 1);
                    if in_arm(a, side) && outside(b) {
                        Some((a, b))
                    } else if in_arm(b, side) && outside(a) {
                        Some((b, a))
                    } else {
                        None
                    }
                })
                .collect()
        });
        let bones = nodes.iter().map(|(&b, &n)| (n, b)).collect();
        let index = |name: &str| body.parts.iter().position(|p| p.name == name);
        let adapt = std::array::from_fn(|side: usize| {
            let arm: Vec<usize> = (0..body.parts.len()).filter(|&i| in_arm(i, side)).collect();
            let upper = nodes.get(&ARMS[side]).and_then(|&n| body.parts.iter().position(|p| p.node == n));
            let mut pairs: Vec<(usize, usize)> = arm.iter().flat_map(|&a| OBSTACLES.iter().filter_map(|n| index(n)).map(move |o| (a, o))).collect();
            pairs.extend(upper.into_iter().flat_map(|u| FLANKS.iter().filter_map(|n| index(n)).map(move |f| (u, f))));
            pairs
        });
        Self { rig, body, rest, nodes, pairs, arm_nodes, bones, adapt }
    }

    /// Every node's world transform at `frame` of `clip` on `h`, played fully.
    pub fn worlds(&self, h: &Humanoid, clip: &Retargeted, frame: usize) -> Vec<Affine3A> {
        let time = frame as f32 / clip.fps;
        let rotations: HashMap<usize, Quat> = clip
            .sample(time)
            .filter_map(|(b, r)| {
                let node = *self.nodes.get(&b)?;
                Some((node, h.get(b)?.local_rotation(r) * self.rig.locals[node].rotation))
            })
            .collect();
        self.rig.worlds(&rotations)
    }

    /// How deep the arm of `side` goes into what a clip is adapted for ([`Clearance::swings`]) at `worlds`, beyond the rest pose, in metres.
    fn adapt_depth(&self, worlds: &[Affine3A], side: usize) -> f32 {
        self.body.penetrations(&self.adapt[side], worlds, &self.rest).iter().map(|p| p.depth).fold(0.0, f32::max)
    }

    /// `worlds` with the arm of `side` swung out at the shoulder by `angle` radians about the avatar's front axis.
    fn swung(&self, worlds: &[Affine3A], side: usize, angle: f32) -> Vec<Affine3A> {
        let Some(&shoulder) = self.nodes.get(&ARMS[side]) else { return worlds.to_vec() };
        let pivot = Vec3::from(worlds[shoulder].translation);
        let turn = Affine3A::from_translation(pivot) * Affine3A::from_rotation_z(if side == 0 { angle } else { -angle }) * Affine3A::from_translation(-pivot);
        let mut out = worlds.to_vec();
        for &n in &self.arm_nodes[side] {
            out[n] = turn * worlds[n];
        }
        out
    }

    /// Adapts a clip to the avatar's body: for each frame of `clip` on `h`, the swing out at the shoulder of the left and right arm, in radians, that keeps hands, forearms and fingers out of the hips and legs and the upper arms out of the trunk's side where the actor's slimmer body let them in. The swings are widened by [`RAMP`] on both sides and averaged over as much, so the arm moves out smoothly before such a contact and back after it, far enough at every frame. Empty where no frame needs one. The runtime contact ([`Clearance::resolve`]) handles what is left.
    pub fn swings(&self, h: &Humanoid, clip: &Retargeted) -> Vec<[f32; 2]> {
        let needed: Vec<[f32; 2]> = (0..clip.frames)
            .map(|f| {
                let worlds = self.worlds(h, clip, f);
                std::array::from_fn(|side| {
                    if self.adapt_depth(&worlds, side) <= ADAPT_ALLOWED {
                        return 0.0;
                    }
                    if self.adapt_depth(&self.swung(&worlds, side, ADAPT_MOST), side) > ADAPT_ALLOWED {
                        return ADAPT_MOST;
                    }
                    let (mut low, mut high) = (0.0, ADAPT_MOST);
                    for _ in 0..8 {
                        let mid = 0.5 * (low + high);
                        if self.adapt_depth(&self.swung(&worlds, side, mid), side) > ADAPT_ALLOWED { low = mid } else { high = mid }
                    }
                    high
                })
            })
            .collect();
        if needed.iter().all(|n| n[0] == 0.0 && n[1] == 0.0) {
            return Vec::new();
        }
        let reach = (RAMP * clip.fps).round().max(1.0) as usize;
        let window = |len: usize, f: usize| f.saturating_sub(reach)..(f + reach + 1).min(len);
        let per_side: [Vec<f32>; 2] = std::array::from_fn(|side| {
            let values: Vec<f32> = needed.iter().map(|n| n[side]).collect();
            let widened: Vec<f32> = (0..values.len()).map(|f| values[window(values.len(), f)].iter().copied().fold(0.0, f32::max)).collect();
            (0..widened.len())
                .map(|f| {
                    let w = &widened[window(widened.len(), f)];
                    w.iter().sum::<f32>() / w.len() as f32
                })
                .collect()
        });
        (0..clip.frames).map(|f| [per_side[0][f], per_side[1][f]]).collect()
    }

    /// Every node's world transform with the humanoid bones' local rotations `rotations`.
    pub fn worlds_of(&self, rotations: &HashMap<B, Quat>) -> Vec<Affine3A> {
        self.rig.worlds(&rotations.iter().filter_map(|(b, &q)| Some((*self.nodes.get(b)?, q))).collect())
    }

    /// Lands the arms on the body's surface in `worlds` (see the module documentation) and returns how far each arm's joints were turned in all, in radians.
    pub fn resolve(&self, worlds: &mut [Affine3A]) -> [f32; 2] {
        std::array::from_fn(|side| {
            let mut turned = 0.0;
            let Some(&upper) = self.nodes.get(&ARMS[side]) else { return turned };
            let hand = self.nodes.get(&HANDS[side]).copied();
            let width = {
                let at = |b: B| self.nodes.get(&b).map(|&n| Vec3::from(worlds[n].translation));
                let [index, little] = [[B::LeftIndexProximal, B::LeftLittleProximal], [B::RightIndexProximal, B::RightLittleProximal]][side];
                at(index).zip(at(little)).map(|(i, l)| (i - l).normalize_or_zero())
            };
            // What each joint may still give way this frame.
            let mut left: HashMap<usize, f32> = HashMap::new();
            for _ in 0..ROUNDS {
                // Every point of the arm in the body: its part's node, position, way out and depth.
                let mut contacts: Vec<(usize, Vec3, Vec3, f32)> = self.pairs[side]
                    .iter()
                    .flat_map(|&(a, b)| self.body.pushes(a, b, worlds, &self.rest).into_iter().map(move |(at, out, depth)| (self.body.parts[a].node, at, out, depth)))
                    .collect();
                // Depth beyond the give.
                for c in &mut contacts {
                    c.3 -= if c.0 == upper { UPPER_ARM_SKIN } else { SKIN };
                }
                if contacts.iter().all(|c| c.3 <= 0.0) {
                    break;
                }
                // The joints that give way, nearest to the contacts first: each touching finger's own joint, the wrist, the shoulder.
                let mut joints: Vec<(usize, f32, Option<Vec3>)> = Vec::new();
                for c in &contacts {
                    if self.bones.get(&c.0).is_some_and(|b| is_finger(*b)) && !joints.iter().any(|j| j.0 == c.0) {
                        joints.push((c.0, FINGER, Some(width.unwrap_or(Vec3::ZERO))));
                    }
                }
                joints.extend(hand.map(|h| (h, WRIST, None)));
                joints.push((upper, MOST * ROUNDS as f32, None));
                for (joint, budget, only) in joints {
                    let room = *left.entry(joint).or_insert(budget);
                    let pivot = Vec3::from(worlds[joint].translation);
                    // The turn ω (axis times angle) that best lifts every contact below the joint by its depth: a turn moves a point at lever r along its way out n by ω·(r × n).
                    let (mut a, mut b) = (Mat3::from_diagonal(Vec3::splat(STIFF)), Vec3::ZERO);
                    for c in contacts.iter().filter(|c| self.below(c.0, joint)) {
                        let mut j = (c.1 - pivot).cross(c.2);
                        if let Some(w) = only {
                            j = w * j.dot(w);
                        }
                        a += Mat3::from_cols(j * j.x, j * j.y, j * j.z);
                        b += j * c.3.max(0.0);
                    }
                    if b == Vec3::ZERO {
                        continue;
                    }
                    let mut omega = a.inverse() * b;
                    if let Some(w) = only {
                        omega = w * omega.dot(w);
                    }
                    // The shoulder swings the arm but does not twist it about its own length.
                    if joint == upper {
                        if let Some(&lower) = self.nodes.get(&[B::LeftLowerArm, B::RightLowerArm][side]) {
                            let along = (Vec3::from(worlds[lower].translation) - pivot).normalize_or_zero();
                            omega -= along * omega.dot(along);
                        }
                    }
                    let angle = omega.length();
                    if angle < 1e-5 || room <= 0.0 {
                        continue;
                    }
                    let angle = angle.min(room).min(MOST);
                    *left.get_mut(&joint).unwrap() -= angle;
                    turned += angle;
                    let turn = Quat::from_axis_angle(omega.normalize(), angle);
                    let about = Affine3A::from_translation(pivot) * Affine3A::from_quat(turn) * Affine3A::from_translation(-pivot);
                    for &n in self.arm_nodes[side].iter().filter(|&&n| self.below(n, joint)) {
                        worlds[n] = about * worlds[n];
                    }
                    // The contacts move with the turn; how far out they came is taken off their depth.
                    for c in contacts.iter_mut().filter(|c| self.below(c.0, joint)) {
                        let moved = about.transform_point3(c.1);
                        c.3 -= (moved - c.1).dot(c.2);
                        c.1 = moved;
                    }
                }
            }
            turned
        })
    }

    /// For checking: the arm contacts deeper than half a centimetre in `worlds`, with their surface directions.
    #[cfg(test)]
    pub fn contacts(&self, worlds: &[Affine3A]) -> String {
        (0..2)
            .flat_map(|side| self.body.penetrations(&self.pairs[side], worlds, &self.rest))
            .filter(|p| p.depth > 0.005)
            .map(|p| format!("{:.1} cm {} into {} n=({:.2},{:.2},{:.2})", p.depth * 100.0, self.body.parts[p.point].name, self.body.parts[p.shape].name, p.normal.x, p.normal.y, p.normal.z))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Whether `node` is `top` or hangs below it.
    fn below(&self, node: usize, top: usize) -> bool {
        let mut n = Some(node);
        while let Some(x) = n {
            if x == top {
                return true;
            }
            n = self.rig.parents[x];
        }
        false
    }

    /// For the humanoid bones' local rotations `rotations`, the corrected local rotation of each arm bone that [`Clearance::resolve`] turned.
    pub fn corrections(&self, rotations: &HashMap<B, Quat>) -> Vec<(B, Quat)> {
        let mut worlds = self.worlds_of(rotations);
        let turned = self.resolve(&mut worlds);
        let rotation = |a: &Affine3A| a.to_scale_rotation_translation().1;
        (0..2)
            .filter(|&side| turned[side] > 0.0)
            .flat_map(|side| self.arm_nodes[side].iter().copied())
            .filter_map(|node| {
                let &b = self.bones.get(&node)?;
                let parent = self.rig.parents[node].map_or(Quat::IDENTITY, |p| rotation(&worlds[p]));
                let local = (parent.inverse() * rotation(&worlds[node])).normalize();
                let was = rotations.get(&b).copied().unwrap_or(self.rig.locals[node].rotation);
                (local.angle_between(was) > 1e-4).then_some((b, local))
            })
            .collect()
    }
}

/// Whether `b` is a finger bone.
fn is_finger(b: B) -> bool {
    let name = b.as_str();
    ["Thumb", "Index", "Middle", "Ring", "Little"].iter().any(|f| name.contains(f))
}
