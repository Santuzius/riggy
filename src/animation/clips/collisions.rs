//! Off-screen collision report of a clip on an avatar: poses the avatar as `play` does, with the pack's calibration for the avatar, and lists where body parts go into each other ([`riggy::clips::body`]).
//!
//! `RIGGY_CLIP=idle_neutral RIGGY_VRM=~/.local/share/riggy/models/cyborg_girl.glb cargo test --bin riggy -- --ignored collision_report --nocapture`

use std::collections::HashMap;
use std::path::Path;

use bevy::math::Affine3A;
use bevy::prelude::*;
use riggy::clips::Calibration;
use riggy::clips::body::Body;
use riggy::clips::rig::Rig;

use super::clearance::Clearance;
use super::retarget::{Retargeted, retarget, retarget_with};
use super::sticks::{humanoid, load_clip, load_rig};
use super::{is_face, is_leg};
use crate::avatar::{Humanoid, HumanoidBone as B};

/// Depths below this, in metres, are skin brushing skin, not a collision.
pub const TOLERANCE: f32 = 0.01;

/// The clip whose collisions on an avatar count as acceptable: the user judged it almost right on both avatars.
pub const REFERENCE: &str = "motifect_locomotion_complete_v1_0_fbx/idle_neutral";

/// Every node's world transform at `frame` with the clip applied fully.
pub fn pose_worlds(rig: &Rig, h: &Humanoid, nodes: &HashMap<B, usize>, clip: &Retargeted, frame: usize) -> Vec<Affine3A> {
    let time = frame as f32 / clip.fps;
    let rotations: HashMap<usize, Quat> = clip.sample(time).map(|(b, r)| (nodes[&b], h.get(b).unwrap().local_rotation(r) * rig.locals[nodes[&b]].rotation)).collect();
    rig.worlds(&rotations)
}

/// One pair of parts that collides somewhere in the clip.
#[derive(Debug, Clone)]
pub struct Collision {
    pub parts: (String, String),
    /// The largest depth, in metres, and the frame it occurs in.
    pub depth: f32,
    pub frame: usize,
    /// Which part's point went into which part's shape at the deepest frame.
    pub into: String,
    /// Frame ranges where the depth exceeds [`TOLERANCE`].
    pub ranges: Vec<(usize, usize)>,
}

/// Whether the clips move the legs, as with the legs switch on (`clip_legs`): the user's setting, or `RIGGY_LEGS=0`/`1`. Off, the legs stay in the rest pose, where riggy's stance would bend them.
pub fn clip_legs() -> bool {
    match std::env::var("RIGGY_LEGS") {
        Ok(v) => v != "0",
        Err(_) => riggy::settings::Settings::load().clip_legs,
    }
}

/// The pack's calibration of the clip `name` for the avatar file `avatar`, as the tray reads it: the user's file, else the default.
pub fn pack_calibration(name: &str, avatar: &str) -> Option<Calibration> {
    let stem = Path::new(avatar).file_stem().unwrap().to_str().unwrap().to_string();
    let clips = riggy::clips::installed();
    riggy::clips::find(&clips, name).unwrap().calibration(&stem).or_else(|| load_clip(name).neutral.is_none().then(|| Calibration::of_itself(&load_clip(name), 12.0)))
}

/// The clip `name` on the avatar file `avatar`, played as the tray plays it: the pack's calibration for the avatar (the user's file, else the default), else none.
pub fn collisions(name: &str, avatar: &str) -> (Vec<Collision>, usize, f32) {
    collisions_with(name, avatar, pack_calibration(name, avatar).as_ref())
}

/// The clip `name` on the avatar file `avatar` with `calibration`.
pub fn collisions_with(name: &str, avatar: &str, calibration: Option<&Calibration>) -> (Vec<Collision>, usize, f32) {
    let rig = load_rig(avatar);
    let path = avatar.replacen('~', &std::env::var("HOME").unwrap(), 1);
    let body = Body::new(&std::fs::read(&path).unwrap(), &rig).unwrap();
    let clip = load_clip(name);
    let (h, nodes) = humanoid(&rig);
    let legs = clip_legs();
    let include = |b| (legs || !is_leg(b)) && !is_face(b);
    let mut r = retarget(&clip, calibration, &h, include);
    // The clip adapted to the avatar's body and the arms landed on its surface as riggy does (`RIGGY_CLEAR=0`: as the clip has them).
    let clearance = std::env::var("RIGGY_CLEAR").map_or(true, |v| v != "0").then(|| Clearance::new(rig.clone(), body.clone()));
    if let Some(c) = &clearance {
        let swings = c.swings(&h, &r);
        if !swings.is_empty() {
            println!("adapted: arms swung out by up to {:.0}°", swings.iter().flat_map(|s| s.iter()).copied().fold(0.0, f32::max).to_degrees());
            r = retarget_with(&clip, calibration, &h, include, &swings);
        }
    }
    let mut most = 0.0f32;

    let rest = rig.worlds(&HashMap::new());
    let pairs = body.pairs();
    let mut depths: Vec<Vec<(f32, String)>> = vec![Vec::new(); pairs.len()];
    for f in 0..clip.frames {
        let mut worlds = pose_worlds(&rig, &h, &nodes, &r, f);
        if let Some(c) = &clearance {
            let before = worlds.clone();
            let turned = c.resolve(&mut worlds);
            for (side, t) in turned.into_iter().enumerate() {
                if t.to_degrees() > 25.0 && std::env::var("RIGGY_VERBOSE").is_ok() {
                    let first = body.penetrations(&pairs, &before, &rest).into_iter().filter(|p| p.depth > 0.005).map(|p| format!("{:.1} cm {} into {}", p.depth * 100.0, body.parts[p.point].name, body.parts[p.shape].name)).collect::<Vec<_>>().join("; ");
                    println!("  {:.2} s side {side}: {:.0}° for {first}", f as f32 / clip.fps, t.to_degrees());
                }
                most = most.max(t.to_degrees());
            }
        }
        for (i, p) in body.penetrations(&pairs, &worlds, &rest).into_iter().enumerate() {
            depths[i].push((p.depth, format!("{} into {} ({:.0} % down {})", body.parts[p.point].name, body.parts[p.shape].name, p.along * 100.0, body.parts[p.point].name)));
        }
    }
    if clearance.is_some() {
        println!("arm joints turned by up to {most:.0}° in all to land on the body");
    }
    let mut out = Vec::new();
    for (i, &(a, b)) in pairs.iter().enumerate() {
        let (frame, (depth, into)) = depths[i].iter().enumerate().max_by(|x, y| x.1.0.total_cmp(&y.1.0)).map(|(f, d)| (f, d.clone())).unwrap_or((0, (0.0, String::new())));
        if depth < TOLERANCE {
            continue;
        }
        let mut ranges = Vec::new();
        let mut start = None;
        for (f, (d, _)) in depths[i].iter().enumerate() {
            match (start, *d >= TOLERANCE) {
                (None, true) => start = Some(f),
                (Some(s), false) => {
                    ranges.push((s, f - 1));
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(s) = start {
            ranges.push((s, clip.frames - 1));
        }
        out.push(Collision { parts: (body.parts[a].name.clone(), body.parts[b].name.clone()), depth, frame, into, ranges });
    }
    out.sort_by(|x, y| y.depth.total_cmp(&x.depth));
    (out, clip.frames, clip.fps)
}

/// See the module documentation.
#[test]
#[ignore = "reads the user's installed clips and avatar"]
fn collision_report() {
    let name = std::env::var("RIGGY_CLIP").unwrap_or("idle_neutral".into());
    let avatar = std::env::var("RIGGY_VRM").unwrap_or("~/.local/share/riggy/models/cyborg_girl.glb".into());
    let (found, frames, fps) = collisions(&name, &avatar);
    println!("legs from the clip: {}", clip_legs());
    println!("{name} on {avatar}: {frames} frames at {fps} fps, {} colliding pairs (deeper than {} cm)", found.len(), TOLERANCE * 100.0);
    for c in &found {
        let seconds: Vec<String> = c.ranges.iter().map(|&(s, e)| format!("{:.2}–{:.2} s", s as f32 / fps, e as f32 / fps)).collect();
        println!("  {:.1} cm  {} at {:.2} s; during {}", c.depth * 100.0, c.into, c.frame as f32 / fps, seconds.join(", "));
    }
}

#[test]
#[ignore = "reads the user's avatar"]
fn print_parts() {
    let avatar = std::env::var("RIGGY_VRM").unwrap_or("~/.local/share/riggy/models/cyborg_girl.glb".into());
    let rig = load_rig(&avatar);
    let path = avatar.replacen('~', &std::env::var("HOME").unwrap(), 1);
    let body = Body::new(&std::fs::read(&path).unwrap(), &rig).unwrap();
    for p in &body.parts {
        println!("{}", p.summary());
    }
}

/// Prints, for every installed clip, its deepest collisions on the avatar beyond the reference clip `idle_neutral`'s on the same pair of parts.
#[test]
#[ignore = "reads the user's installed clips and avatar"]
fn collision_survey() {
    let avatar = std::env::var("RIGGY_VRM").unwrap_or("~/.local/share/riggy/models/cyborg_girl.glb".into());
    let (reference, _, _) = collisions(REFERENCE, &avatar);
    let allowed = |c: &Collision| reference.iter().find(|r| r.parts == c.parts).map_or(0.0, |r| r.depth);
    for file in riggy::clips::installed() {
        let key = file.key();
        let (found, _, fps) = collisions(&key, &avatar);
        let worse: Vec<String> = found.iter().filter(|c| c.depth > allowed(c) + TOLERANCE).take(3).map(|c| format!("{:.1} cm {} at {:.2} s", (c.depth - allowed(c)) * 100.0, c.into, c.frame as f32 / fps)).collect();
        println!("{key}: {}", if worse.is_empty() { "clean".into() } else { worse.join("; ") });
    }
}

/// Whether a pair of parts involves the arm of `side` (0 left, 1 right) and a part that is not of that arm.
fn arm_against_body(c: &Collision, side: usize) -> bool {
    let prefix = ["left", "right"][side];
    let arm = |p: &str| p.starts_with(prefix) && ["UpperArm", "LowerArm", "Hand"].iter().any(|s| p.ends_with(s));
    arm(&c.parts.0) != arm(&c.parts.1)
}

/// For each arm, the smallest swing out at the shoulder, in whole degrees up to 30, that keeps the arm's collisions with the rest of the body within the reference clip's: the `actor.arm_hang` that makes the pack's calibration for the avatar clear the clip `name`.
#[test]
#[ignore = "reads the user's installed clips and avatar"]
fn clearing_arm_hang() {
    let name = std::env::var("RIGGY_CLIP").unwrap_or("idle_neutral".into());
    let avatar = std::env::var("RIGGY_VRM").unwrap_or("~/.local/share/riggy/models/cyborg_girl.glb".into());
    let (reference, _, _) = collisions(REFERENCE, &avatar);
    let allowed = |c: &Collision| reference.iter().find(|r| r.parts == c.parts).map_or(0.0, |r| r.depth);
    let base = pack_calibration(&name, &avatar).unwrap_or(Calibration { neutral: None, approved: Vec::new(), actor: Default::default(), avatar: Default::default() });
    let current = base.actor.arm_hang.unwrap_or([base.avatar.arm_hang; 2]);
    let mut found = current;
    for side in 0..2 {
        for swing in 0..=30 {
            let mut c = base.clone();
            let mut hang = current;
            hang[side] = current[side] - swing as f32;
            c.actor.arm_hang = Some(hang);
            let (hits, _, fps) = collisions_with(&name, &avatar, Some(&c));
            let worst = hits.iter().filter(|h| arm_against_body(h, side) && h.depth > allowed(h) + TOLERANCE).max_by(|a, b| a.depth.total_cmp(&b.depth));
            match worst {
                None => {
                    found[side] = hang[side];
                    println!("{} arm: clear with {swing} degrees more swing (actor.arm_hang {:.1})", ["left", "right"][side], hang[side]);
                    break;
                }
                Some(w) if swing % 5 == 0 && std::env::var("RIGGY_VERBOSE").is_ok() => println!("  {swing} degrees: {:.1} cm {} at {:.2} s", w.depth * 100.0, w.into, w.frame as f32 / fps),
                Some(w) if swing == 30 => println!("{} arm: still {:.1} cm {} at {:.2} s with 30 degrees", ["left", "right"][side], w.depth * 100.0, w.into, w.frame as f32 / fps),
                Some(_) => {}
            }
        }
    }
    println!("actor.arm_hang = [{:.1}, {:.1}] (was {:?})", found[0], found[1], base.actor.arm_hang);
}

/// For checking one collision: `RIGGY_PARTS=leftLowerArm,leftUpperLeg RIGGY_AT=15.8` prints each point of the first part inside the second at that second.
#[test]
#[ignore = "reads the user's installed clips and avatar"]
fn explain_collision() {
    let name = std::env::var("RIGGY_CLIP").unwrap_or("idle_neutral".into());
    let avatar = std::env::var("RIGGY_VRM").unwrap_or("~/.local/share/riggy/models/cyborg_girl.glb".into());
    let parts = std::env::var("RIGGY_PARTS").unwrap();
    let (a, b) = parts.split_once(',').unwrap();
    let at: f32 = std::env::var("RIGGY_AT").unwrap().parse().unwrap();
    let rig = load_rig(&avatar);
    let path = avatar.replacen('~', &std::env::var("HOME").unwrap(), 1);
    let body = Body::new(&std::fs::read(&path).unwrap(), &rig).unwrap();
    let clip = load_clip(&name);
    let (h, nodes) = humanoid(&rig);
    let legs = clip_legs();
    let r = retarget(&clip, pack_calibration(&name, &avatar).as_ref(), &h, |b| (legs || !is_leg(b)) && !is_face(b));
    let worlds = pose_worlds(&rig, &h, &nodes, &r, (at * clip.fps).round() as usize);
    let index = |n: &str| body.parts.iter().position(|p| p.name == n).unwrap();
    for bone in [B::Hips, B::LeftUpperLeg, B::LeftLowerLeg, B::LeftLowerArm, B::LeftHand] {
        let t = worlds[nodes[&bone]].translation * 1000.0;
        println!("{bone:?}: {:.1} {:.1} {:.1} mm", t.x, t.y, t.z);
    }
    println!("{b}: {} surface vertices", body.parts[index(b)].surface_len());
    for (q, star, near) in body.explain(index(a), index(b), &worlds) {
        println!("  point {:.3?}: star depth {:.1?} cm, nearest vertex {:?}", q * 100.0, star.map(|d| d * 100.0), near.map(|(d, behind)| format!("{:.1} cm away, {:.1} cm behind", d * 100.0, behind * 100.0)));
    }
}

/// Collisions in what the app showed: reads a pose log with rotations (`RIGGY_POSE_CSV`, written with `RIGGY_POSE_LOG`, e.g. by `tools/clip_shots.sh`) and lists, for each logged moment from `RIGGY_FROM` seconds of app time on, the pairs of parts that go more than [`TOLERANCE`] into each other beyond the rest pose.
#[test]
#[ignore = "reads a pose log of the app and the user's avatar"]
fn app_collisions() {
    let avatar = std::env::var("RIGGY_VRM").unwrap_or("~/.local/share/riggy/models/cyborg_girl.glb".into());
    let csv = std::env::var("RIGGY_POSE_CSV").unwrap();
    let from: f32 = std::env::var("RIGGY_FROM").ok().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let rig = load_rig(&avatar);
    let path = avatar.replacen('~', &std::env::var("HOME").unwrap(), 1);
    let body = Body::new(&std::fs::read(&path).unwrap(), &rig).unwrap();
    let (_, nodes) = humanoid(&rig);
    let mut moments: Vec<(f32, HashMap<usize, Quat>)> = Vec::new();
    for line in std::fs::read_to_string(&csv).unwrap().lines().skip(1) {
        let f: Vec<&str> = line.split(',').collect();
        let (Ok(t), Ok(b)) = (f[0].parse::<f32>(), f[1].parse::<B>()) else { continue };
        let Some(&node) = nodes.get(&b) else { continue };
        let q: Vec<f32> = f[6..10].iter().map(|x| x.parse().unwrap()).collect();
        if moments.last().is_none_or(|m| m.0 != t) {
            moments.push((t, HashMap::new()));
        }
        moments.last_mut().unwrap().1.insert(node, Quat::from_xyzw(q[0], q[1], q[2], q[3]));
    }
    let rest = rig.worlds(&HashMap::new());
    let pairs = body.pairs();
    for (t, rotations) in moments.iter().filter(|m| m.0 >= from) {
        let worlds = rig.worlds(rotations);
        let hits: Vec<String> = body
            .penetrations(&pairs, &worlds, &rest)
            .iter()
            .filter(|p| p.depth > TOLERANCE)
            .map(|p| format!("{:.1} cm {} into {} ({:.0} %)", p.depth * 100.0, body.parts[p.point].name, body.parts[p.shape].name, p.along * 100.0))
            .collect();
        if !hits.is_empty() {
            println!("{t:6.2} s: {}", hits.join("; "));
        }
    }
}

/// How smoothly the contact rule moves the arms: plays the clip at its frame rate with [`Clearance::corrections`] smoothed as riggy smooths them (`CONTACT_LAG`), and prints the largest frame-to-frame jumps of the correction's own displacement of the hands and fingertips (second differences, in millimetres).
#[test]
#[ignore = "reads the user's installed clips and avatar"]
fn contact_smoothness() {
    let name = std::env::var("RIGGY_CLIP").unwrap_or("idle_neutral".into());
    let avatar = std::env::var("RIGGY_VRM").unwrap_or("~/.local/share/riggy/models/cyborg_girl.glb".into());
    let lag: f32 = std::env::var("RIGGY_LAG").ok().and_then(|s| s.parse().ok()).unwrap_or(super::CONTACT_LAG);
    let rig = load_rig(&avatar);
    let path = avatar.replacen('~', &std::env::var("HOME").unwrap(), 1);
    let body = Body::new(&std::fs::read(&path).unwrap(), &rig).unwrap();
    let clearance = Clearance::new(rig.clone(), body);
    let clip = load_clip(&name);
    let (h, nodes) = humanoid(&rig);
    let legs = clip_legs();
    let include = |b| (legs || !is_leg(b)) && !is_face(b);
    let calibration = pack_calibration(&name, &avatar);
    let mut r = retarget(&clip, calibration.as_ref(), &h, include);
    let swings = clearance.swings(&h, &r);
    if !swings.is_empty() {
        r = retarget_with(&clip, calibration.as_ref(), &h, include, &swings);
    }
    let tips = [B::LeftHand, B::RightHand, B::LeftIndexDistal, B::RightIndexDistal, B::LeftMiddleDistal, B::RightMiddleDistal, B::LeftThumbDistal, B::RightThumbDistal];
    let follow = 1.0 - (-1.0 / clip.fps / lag).exp();
    let mut applied: HashMap<B, Quat> = HashMap::new();
    let mut offsets: Vec<HashMap<B, Vec3>> = Vec::new();
    for f in 0..clip.frames {
        let plain = pose_worlds(&rig, &h, &nodes, &r, f);
        let rotations: HashMap<B, Quat> = nodes.iter().map(|(&b, &n)| (b, plain_local(&rig, &plain, n))).collect();
        let mut wanted: HashMap<B, Quat> = clearance.corrections(&rotations).into_iter().map(|(b, q)| (b, (q * rotations[&b].inverse()).normalize())).collect();
        if let Ok(range) = std::env::var("RIGGY_AT") {
            let (from, to) = range.split_once('-').unwrap();
            let t = f as f32 / clip.fps;
            if t >= from.parse().unwrap() && t <= to.parse().unwrap() {
                let mut list: Vec<String> = wanted.iter().map(|(b, q)| format!("{b:?} {:.0}°", q.to_axis_angle().1.to_degrees())).collect();
                list.sort();
                let worlds = clearance.worlds_of(&rotations);
                let hits = clearance.contacts(&worlds);
                println!("{t:.2} s: {} | {hits}", list.join(", "));
            }
        }
        for &b in applied.keys() {
            wanted.entry(b).or_insert(Quat::IDENTITY);
        }
        applied = wanted.into_iter().map(|(b, t)| (b, applied.get(&b).copied().unwrap_or(Quat::IDENTITY).slerp(t, follow))).collect();
        let corrected: HashMap<B, Quat> = rotations.iter().map(|(&b, &q)| (b, applied.get(&b).map_or(q, |a| (*a * q).normalize()))).collect();
        let worlds = clearance.worlds_of(&corrected);
        offsets.push(tips.iter().filter_map(|b| Some((*b, (Vec3::from(worlds[*nodes.get(b)?].translation) - Vec3::from(plain[*nodes.get(b)?].translation)) * 1000.0))).collect());
    }
    let mut kicks: Vec<(f32, usize, B)> = Vec::new();
    for f in 1..offsets.len().saturating_sub(1) {
        for b in tips {
            let (Some(a), Some(m), Some(c)) = (offsets[f - 1].get(&b), offsets[f].get(&b), offsets[f + 1].get(&b)) else { continue };
            kicks.push(((*c - 2.0 * *m + *a).length(), f, b));
        }
    }
    kicks.sort_by(|x, y| y.0.total_cmp(&x.0));
    let largest = offsets.iter().flat_map(|o| o.values()).map(|v| v.length()).fold(0.0, f32::max);
    println!("{name}, lag {lag} s: largest displacement {largest:.0} mm; frames with a jump over 5/10/20 mm: {}/{}/{}", kicks.iter().filter(|k| k.0 > 5.0).count(), kicks.iter().filter(|k| k.0 > 10.0).count(), kicks.iter().filter(|k| k.0 > 20.0).count());
    for (d, f, b) in kicks.iter().take(8) {
        println!("  {d:5.1} mm at {:.2} s {b:?}", *f as f32 / clip.fps);
    }
}

/// Node `n`'s local rotation in `worlds`.
fn plain_local(rig: &Rig, worlds: &[Affine3A], n: usize) -> Quat {
    let rotation = |a: &Affine3A| a.to_scale_rotation_translation().1;
    let parent = rig.parents[n].map_or(Quat::IDENTITY, |p| rotation(&worlds[p]));
    (parent.inverse() * rotation(&worlds[n])).normalize()
}
