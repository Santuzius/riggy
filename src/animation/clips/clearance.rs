//! Clearance: the arms do not pass through the body. An actor with narrower hips or a slimmer trunk than the avatar lets the hands rest and the upper arms press where the avatar's body is. When riggy first plays a clip, it swings each arm out at the shoulder in the frames where a hand, forearm or finger would go into the hips or legs, or the upper arm into the trunk's side, just far enough, moving out before and back after such a contact ([`Clearance::swings`]).
//!
//! The avatar's body parts come from its mesh ([`riggy::clips::body`]).

use std::collections::HashMap;
use std::path::Path;

use bevy::math::Affine3A;
use bevy::prelude::*;
use riggy::clips::body::Body;
use riggy::clips::rig::Rig;

use super::retarget::Retargeted;
use crate::avatar::{Humanoid, HumanoidBone as B};

const ARMS: [B; 2] = [B::LeftUpperArm, B::RightUpperArm];
/// The adaptation of a clip: depth into the hips and legs (and of the upper arm into the trunk's side) left to the runtime contact, in metres; the largest swing, in radians; and the seconds it moves out before such a contact and back after it.
pub const ADAPT_ALLOWED: f32 = 0.01;
const ADAPT_MOST: f32 = 0.7;
const RAMP: f32 = 0.3;
/// The parts a clip is adapted to keep the arms out of, and those only the upper arm is kept out of (a hand or forearm in front of the chest is a gesture a swing to the side would spoil).
const OBSTACLES: [&str; 5] = ["hips", "leftUpperLeg", "rightUpperLeg", "leftLowerLeg", "rightLowerLeg"];
const FLANKS: [&str; 3] = ["spine", "chest", "upperChest"];

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
    /// For each arm, every node from the upper arm down.
    arm_nodes: [Vec<usize>; 2],
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
        let index = |name: &str| body.parts.iter().position(|p| p.name == name);
        let adapt = std::array::from_fn(|side: usize| {
            let arm: Vec<usize> = (0..body.parts.len()).filter(|&i| in_arm(i, side)).collect();
            let upper = nodes.get(&ARMS[side]).and_then(|&n| body.parts.iter().position(|p| p.node == n));
            let mut pairs: Vec<(usize, usize)> = arm.iter().flat_map(|&a| OBSTACLES.iter().filter_map(|n| index(n)).map(move |o| (a, o))).collect();
            pairs.extend(upper.into_iter().flat_map(|u| FLANKS.iter().filter_map(|n| index(n)).map(move |f| (u, f))));
            pairs
        });
        Self { rig, body, rest, nodes, arm_nodes, adapt }
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

    /// Adapts a clip to the avatar's body: for each frame of `clip` on `h`, the swing out at the shoulder of the left and right arm, in radians, that keeps hands, forearms and fingers out of the hips and legs and the upper arms out of the trunk's side where the actor's slimmer body let them in. The swings are widened by [`RAMP`] on both sides and averaged over as much, so the arm moves out smoothly before such a contact and back after it, far enough at every frame. Empty where no frame needs one.
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


}
