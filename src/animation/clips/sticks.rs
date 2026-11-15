//! Off-screen check of a clip on a VRM avatar: poses the avatar's node tree as `play` does and compares it, bone by bone, with the clip's own skeleton. Writes stick-figure CSVs for `tools/clip_sticks.py`.
//!
//! `RIGGY_CLIP=idle_neutral RIGGY_VRM=~/.local/share/riggy/models/cyber_ver101.vrm cargo test --bin riggy -- --ignored write_sticks` writes `temp/sticks/<clip>.{clip,avatar}.csv`.

use std::collections::HashMap;
use std::path::Path;

use bevy::math::Affine3A;
use bevy::prelude::*;
use riggy::clips::Clip;
use riggy::clips::rig::Rig;

use riggy::clips::calibration::Calibration;

use super::retarget::{Retargeted, POSTURE, arm_of, parent, retarget};
use super::{is_face, is_leg};
use crate::avatar::{Humanoid, HumanoidBone as B, PoseBone};

/// The avatar's humanoid as `resolve_avatar` builds it, with each bone's node in the rig.
pub fn humanoid(rig: &Rig) -> (Humanoid, HashMap<B, usize>) {
    let mut h = Humanoid::default();
    let mut nodes = HashMap::new();
    let rest = rig.worlds(&HashMap::new());
    for (name, &node) in &rig.humanoid {
        let Ok(bone) = name.parse::<B>() else { continue };
        let l = rig.locals[node];
        let parent_world = rig.parents[node].map_or(Affine3A::IDENTITY, |p| rest[p]);
        h.bones.insert(bone, PoseBone::new(Entity::PLACEHOLDER, Transform { translation: l.translation, rotation: l.rotation, scale: l.scale }, parent_world));
        nodes.insert(bone, node);
    }
    (h, nodes)
}

/// Avatar-space position of every humanoid bone at `frame`, with the clip applied fully as `play` applies it.
pub fn avatar_pose(rig: &Rig, h: &Humanoid, nodes: &HashMap<B, usize>, clip: &Retargeted, frame: usize) -> HashMap<B, Vec3> {
    let time = frame as f32 / clip.fps;
    let rotations: HashMap<usize, Quat> = clip.sample(time).map(|(b, r)| (nodes[&b], h.get(b).unwrap().local_rotation(r) * rig.locals[nodes[&b]].rotation)).collect();
    let worlds = rig.worlds(&rotations);
    nodes.iter().map(|(&b, &n)| (b, worlds[n].translation.into())).collect()
}

/// Position of every mapped bone of the clip at `frame`, built from the hips along the humanoid parents.
pub fn clip_pose(clip: &Clip, frame: usize) -> HashMap<B, Vec3> {
    let bones: HashMap<B, &riggy::clips::ClipBone> = clip.bones.iter().filter_map(|(n, b)| Some((n.parse::<B>().ok()?, b))).collect();
    let mut out = HashMap::new();
    for &b in B::ALL {
        let Some(bone) = bones.get(&b) else { continue };
        let mut p = parent(b);
        while p.is_some_and(|x| !bones.contains_key(&x)) {
            p = parent(p.unwrap());
        }
        let pos = match p {
            Some(p) => {
                let pb = bones[&p];
                out[&p] + pb.rotations[frame] * pb.rest_rotation.inverse() * (bone.rest_position - pb.rest_position)
            }
            None => bone.rest_position,
        };
        out.insert(b, pos);
    }
    out
}

/// riggy's default `[idle] arm_angle`, the avatar's hang of the upper arms at ease.
const ARM_ANGLE: f32 = 12.0;

/// The clip retargeted onto the rig as `play` does for a clip asked for from the tray: every bone but the legs, calibrated on the clip's neutral clip as a fresh calibration file would be.
pub fn retargeted(clip: &Clip, rig: &Rig) -> (Humanoid, HashMap<B, usize>, Retargeted) {
    let (h, nodes) = humanoid(rig);
    let calibration = clip.neutral.as_deref().map(|n| Calibration::measure(&load_clip(n), n, ARM_ANGLE));
    let r = retarget(clip, calibration.as_ref(), &h, |b| !is_leg(b) && !is_face(b));
    (h, nodes, r)
}

/// Each bone's largest angle, in degrees, between its direction from its humanoid parent on the avatar and in the clip, over every frame.
pub fn direction_errors(clip: &Clip, rig: &Rig) -> HashMap<B, f32> {
    let (h, nodes, r) = retargeted(clip, rig);
    let mut worst = HashMap::new();
    for f in 0..clip.frames {
        let a = avatar_pose(rig, &h, &nodes, &r, f);
        let c = clip_pose(clip, f);
        for (&b, &pos) in &a {
            let Some(p) = parent(b) else { continue };
            let (Some(&ap), Some(&cb), Some(&cp)) = (a.get(&p), c.get(&b), c.get(&p)) else { continue };
            // Legs are the stance's; a calibrated trunk and shoulders keep the avatar's posture (see `posture_errors`), so where the upper arm starts follows the avatar's shoulder.
            if is_leg(b) || (r.calibrated && (POSTURE.contains(&b) || matches!(p, B::LeftShoulder | B::RightShoulder))) {
                continue;
            }
            // An arm is swung about the shoulder to the avatar's hang; a segment below the shoulder points where the clip's points after that swing.
            let wanted = arm_of(p).map_or(cb - cp, |side| r.arm_swings[f][side] * (cb - cp));
            let angle = (pos - ap).angle_between(wanted).to_degrees();
            let w = worst.entry(b).or_insert(0.0f32);
            *w = w.max(angle);
        }
    }
    worst
}

/// Bones whose turn about their own axis shows in the pose, each with its tip (the axis) and two bones beside each other that span its width.
const WIDTHS: [(B, B, B, B); 5] = [
    (B::Hips, B::Spine, B::LeftUpperLeg, B::RightUpperLeg),
    (B::UpperChest, B::Neck, B::LeftShoulder, B::RightShoulder),
    (B::Head, B::Neck, B::LeftEye, B::RightEye),
    (B::LeftHand, B::LeftMiddleProximal, B::LeftIndexProximal, B::LeftLittleProximal),
    (B::RightHand, B::RightMiddleProximal, B::RightIndexProximal, B::RightLittleProximal),
];

/// For each bone of [`WIDTHS`], the largest angle, in degrees, by which its width on the avatar is turned about the bone's axis against the clip's, over every frame.
pub fn twist_errors(clip: &Clip, rig: &Rig) -> HashMap<B, f32> {
    let (h, nodes, r) = retargeted(clip, rig);
    let mut worst = HashMap::new();
    for f in 0..clip.frames {
        let a = avatar_pose(rig, &h, &nodes, &r, f);
        let c = clip_pose(clip, f);
        for (bone, tip, one, other) in WIDTHS.into_iter().filter(|w| !(r.calibrated && POSTURE.contains(&w.0))) {
            let ([Some(ab), Some(at), Some(a1), Some(a2)], [Some(cb), Some(ct), Some(c1), Some(c2)]) = ([bone, tip, one, other].map(|x| a.get(&x).copied()), [bone, tip, one, other].map(|x| c.get(&x).copied())) else { continue };
            let swing = arm_of(bone).map_or(Quat::IDENTITY, |side| r.arm_swings[f][side]);
            let (ct, c1, c2) = (cb + swing * (ct - cb), cb + swing * (c1 - cb), cb + swing * (c2 - cb));
            let axis = (ct - cb).normalize();
            let flat = |v: Vec3| (v - axis * v.dot(axis)).normalize_or_zero();
            let angle = flat(a1 - a2).angle_between(flat(c1 - c2)).to_degrees();
            // The avatar's axis may point a little elsewhere; that error shows in the direction errors.
            let _ = (ab, at);
            let w = worst.entry(bone).or_insert(0.0f32);
            *w = w.max(angle);
        }
    }
    worst
}

fn write_csv(path: &Path, frames: impl Iterator<Item = (usize, HashMap<B, Vec3>)>) {
    let mut text = String::from("frame,bone,parent,x,y,z\n");
    for (f, pose) in frames {
        for &b in B::ALL {
            let Some(p) = pose.get(&b) else { continue };
            let parent = parent(b).filter(|x| pose.contains_key(x)).map_or("", |x| x.as_str());
            text += &format!("{f},{},{parent},{},{},{}\n", b.as_str(), p.x, p.y, p.z);
        }
    }
    std::fs::write(path, text).unwrap();
}

pub fn load_clip(name: &str) -> Clip {
    let clips = riggy::clips::installed();
    let file = riggy::clips::find(&clips, name).unwrap_or_else(|| panic!("no clip {name}"));
    Clip::load(&file.path, &file.skeletons_dir()).unwrap()
}

pub fn load_rig(path: &str) -> Rig {
    let path = path.replacen('~', &std::env::var("HOME").unwrap(), 1);
    let bytes = std::fs::read(&path).unwrap();
    if !path.ends_with(".glb") {
        return Rig::from_vrm(&bytes).unwrap();
    }
    // A GLB avatar reads its bones, scale and rest corrections from riggy's mapping file of the same name.
    let name = Path::new(&path).file_stem().unwrap().to_str().unwrap();
    let text = std::fs::read_to_string(riggy::files::data_dir().join("avatars").join(format!("{name}.toml"))).unwrap();
    let mapping: crate::avatar::glb::Mapping = toml::from_str(&text).unwrap();
    let rest = mapping.rest.iter().map(|(n, d)| (n.clone(), Vec3::from(d.rotate))).collect();
    Rig::from_glb(&bytes, &mapping.humanoid, mapping.scale, &rest).unwrap()
}

/// How far each shoulder line (shoulder to upper arm) is raised on the avatar in the clip `neutral` against its rest pose, in degrees, mean over the frames: left, right.
pub fn shoulder_lifts(neutral: &Clip, rig: &Rig) -> [f32; 2] {
    let (h, nodes, r) = retargeted(neutral, rig);
    let worlds = rig.worlds(&HashMap::new());
    let rest: HashMap<B, Vec3> = nodes.iter().map(|(&b, &n)| (b, worlds[n].translation.into())).collect();
    let lift = |pose: &HashMap<B, Vec3>, s: B, u: B| {
        let d = pose[&u] - pose[&s];
        d.y.atan2(Vec2::new(d.x, d.z).length()).to_degrees()
    };
    let sides = [(B::LeftShoulder, B::LeftUpperArm), (B::RightShoulder, B::RightUpperArm)];
    let mut sums = [0.0; 2];
    for f in 0..neutral.frames {
        let a = avatar_pose(rig, &h, &nodes, &r, f);
        for (i, &(s, u)) in sides.iter().enumerate() {
            sums[i] += lift(&a, s, u) - lift(&rest, s, u);
        }
    }
    sums.map(|x| x / neutral.frames.max(1) as f32)
}

/// See the module documentation.
#[test]
#[ignore = "writes stick figures of a clip on the user's avatar"]
fn write_sticks() {
    let name = std::env::var("RIGGY_CLIP").unwrap_or("idle_neutral".into());
    let rig = load_rig(&std::env::var("RIGGY_VRM").unwrap_or("~/.local/share/riggy/models/cyber_ver101.vrm".into()));
    let clip = load_clip(&name);
    let (h, nodes, r) = retargeted(&clip, &rig);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("temp/sticks");
    std::fs::create_dir_all(&dir).unwrap();
    let stem = name.replace('/', "_");
    write_csv(&dir.join(format!("{stem}.clip.csv")), (0..clip.frames).map(|f| (f, clip_pose(&clip, f))));
    write_csv(&dir.join(format!("{stem}.avatar.csv")), (0..clip.frames).map(|f| (f, avatar_pose(&rig, &h, &nodes, &r, f))));
    let mut errors: Vec<_> = direction_errors(&clip, &rig).into_iter().collect();
    errors.sort_by(|a, b| b.1.total_cmp(&a.1));
    println!("largest direction errors (degrees): {errors:?}");
    println!("largest twist errors (degrees): {:?}", twist_errors(&clip, &rig));
    if let Some(neutral) = &clip.neutral {
        println!("trunk posture in {neutral} against the avatar's rest (degrees): {:?}", posture_errors(&load_clip(neutral), &rig));
        println!("arm hang in {neutral} against the avatar's (degrees, left and right): {:?}", hang_errors(&load_clip(neutral), &rig));
        println!("calibration measured on {neutral}:\n{}", toml::to_string_pretty(&Calibration::measure(&load_clip(neutral), neutral, ARM_ANGLE)).unwrap());
        println!("shoulders in {neutral}, raised against the avatar's rest (degrees, left and right): {:?}", shoulder_lifts(&load_clip(neutral), &rig));
    }
}

/// For each trunk bone, the angle, in degrees, of its mean turn against its parent while `neutral` plays: 0 where the avatar keeps its own posture in the actor's natural one.
pub fn posture_errors(neutral: &Clip, rig: &Rig) -> HashMap<B, f32> {
    let (h, _) = humanoid(rig);
    let r = retarget(neutral, Some(&Calibration::measure(neutral, "neutral", ARM_ANGLE)), &h, |b| !is_leg(b) && !is_face(b));
    r.bones
        .iter()
        .filter(|(b, _)| POSTURE.contains(b))
        .map(|(b, q)| {
            let first = Vec4::from(q[0]);
            let sum = q.iter().fold(Vec4::ZERO, |s, x| s + Vec4::from(*x) * Vec4::from(*x).dot(first).signum());
            (*b, Quat::from_vec4(sum.normalize()).angle_between(Quat::IDENTITY).to_degrees())
        })
        .collect()
}

/// For each arm, left and right, the mean angle, in degrees, by which the line from the avatar's shoulder to wrist hangs out from vertical (seen from the front) while `neutral` plays, less riggy's `arm_angle`: 0 where the actor's natural hang becomes the avatar's.
pub fn hang_errors(neutral: &Clip, rig: &Rig) -> [f32; 2] {
    let (h, nodes) = humanoid(rig);
    let r = retarget(neutral, Some(&Calibration::measure(neutral, "neutral", ARM_ANGLE)), &h, |b| !is_leg(b) && !is_face(b));
    std::array::from_fn(|side| {
        let (upper, hand) = [(B::LeftUpperArm, B::LeftHand), (B::RightUpperArm, B::RightHand)][side];
        let angles: Vec<f32> = (0..neutral.frames)
            .map(|f| {
                let a = avatar_pose(rig, &h, &nodes, &r, f);
                let d = a[&hand] - a[&upper];
                (if side == 0 { d.x } else { -d.x }).atan2(-d.y).to_degrees()
            })
            .collect();
        angles.iter().sum::<f32>() / angles.len() as f32 - ARM_ANGLE
    })
}

/// Bones placed beside their parent's axis rather than along it (shoulders, thighs, eyes, finger roots, thumb roots): their direction from the parent depends on the two skeletons' proportions, so it is left out of the direction check.
fn beside(b: B) -> bool {
    use B::*;
    matches!(b, LeftShoulder | RightShoulder | LeftUpperLeg | RightUpperLeg | LeftEye | RightEye | Jaw | LeftThumbMetacarpal | RightThumbMetacarpal | LeftIndexProximal | RightIndexProximal | LeftRingProximal | RightRingProximal | LeftLittleProximal | RightLittleProximal)
}

/// Checks a clip on a VRM avatar, as listed in doc/animation-checks.md: every limb bone along a chain points where the clip's points, the hands turn about their axes as the clip's, and in the pack's neutral clip the trunk keeps the avatar's posture and the arms hang as the avatar's.
fn check(clip: &str, vrm: &str) {
    let rig = load_rig(vrm);
    let clip = load_clip(clip);
    let directions = direction_errors(&clip, &rig);
    let bad: Vec<_> = directions.iter().filter(|(b, e)| !beside(**b) && **e > 1.0).collect();
    assert!(bad.is_empty(), "bones pointing away from the clip (degrees): {bad:?}");
    let twists = twist_errors(&clip, &rig);
    // The head's width runs between the eyes, which sit at other heights and depths on the two skeletons.
    let bad: Vec<_> = twists.iter().filter(|(b, e)| **e > if **b == B::Head { 6.0 } else { 1.0 }).collect();
    assert!(bad.is_empty(), "bones turned about their axes against the clip (degrees): {bad:?}");
    let neutral = clip.neutral.as_deref().expect("the clip's bone map names no neutral clip");
    let posture = posture_errors(&load_clip(neutral), &rig);
    assert!(posture.len() == POSTURE.len() && posture.values().all(|e| *e < 0.5), "trunk bones off the avatar's posture in {neutral} (degrees): {posture:?}");
    let hang = hang_errors(&load_clip(neutral), &rig);
    assert!(hang.iter().all(|e| e.abs() < 1.0), "upper arms off the avatar's hang in {neutral} (degrees, left and right): {hang:?}");
}

#[test]
#[ignore = "reads the user's installed clips and avatar"]
fn idle_neutral_on_cyber_ver101() {
    check("idle_neutral", "~/.local/share/riggy/models/cyber_ver101.vrm");
}

#[test]
#[ignore = "reads the user's installed clips and avatar"]
fn idle_neutral_on_cyborg_girl() {
    check("idle_neutral", "~/.local/share/riggy/models/cyborg_girl.glb");
}
