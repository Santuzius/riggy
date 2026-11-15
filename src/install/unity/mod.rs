//! A `.unitypackage`'s humanoid animation clips, each converted to a GLB of riggy's Unity rig and its animation.
//!
//! A `.unitypackage` is a gzipped tar with one folder per asset: `pathname` (the asset's path in the Unity project) and `asset` (its file). A humanoid `.anim` clip holds muscle curves, not bone rotations; Unity turns them into rotations through the Avatar it computes when it imports a character. That Avatar is not in the package, so the clips are played on riggy's own Unity rig (see `humanoid/README.md`): arms, legs and fingers come out as on the pack's character within a few degrees, the trunk and head as on that rig. Clips set to play mirrored (`m_Mirror`) are mirrored as Unity does: left and right muscles swap, the trunk's and head's left-right muscles change sign, and the body rotation mirrors.
//!
//! The GLB is in glTF space (Unity's X mirrored), with the rig's nodes under their Unity humanoid names, sampled at 30 frames per second.

mod anim;
mod humanoid;

use std::io::Read;

use bevy::math::{DQuat, DVec3};

use super::fbx::{Pose, Track, glb};
use anim::Anim;
use humanoid::{MUSCLE_COUNT, Rig, muscle_index};

const FPS: f64 = 30.0;

/// A clip of the package: its name (the file's, without `.anim`) and its GLB.
pub struct Clip {
    pub name: String,
    pub glb: Vec<u8>,
}

/// Every humanoid clip in the `.unitypackage` `bytes`; clips that are not humanoid (no muscle curves) are skipped and listed in the second part.
pub fn clips(bytes: &[u8]) -> Result<(Vec<Clip>, Vec<String>), String> {
    let mut clips = Vec::new();
    let mut skipped = Vec::new();
    for (path, asset) in anim_assets(bytes)? {
        let name = path.rsplit('/').next().unwrap_or(&path).trim_end_matches(".anim").to_owned();
        let text = String::from_utf8_lossy(&asset);
        match Anim::parse(&text).and_then(|anim| clip_glb(&anim, &name)) {
            Ok(glb) => clips.push(Clip { name, glb }),
            Err(err) => skipped.push(format!("{path}: {err}")),
        }
    }
    clips.sort_by(|a, b| a.name.cmp(&b.name));
    Ok((clips, skipped))
}

/// The `.anim` assets of the package: (path in the Unity project, file).
fn anim_assets(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    let mut paths = std::collections::HashMap::new();
    let mut assets = std::collections::HashMap::new();
    for entry in archive.entries().map_err(|err| format!("not a .unitypackage: {err}"))? {
        let mut entry = entry.map_err(|err| format!("not a .unitypackage: {err}"))?;
        let path = entry.path().map_err(|err| err.to_string())?.to_string_lossy().trim_start_matches("./").to_owned();
        let Some((guid, file)) = path.split_once('/') else { continue };
        if file != "pathname" && file != "asset" {
            continue;
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data).map_err(|err| err.to_string())?;
        if file == "pathname" {
            let pathname = String::from_utf8_lossy(&data).lines().next().unwrap_or_default().to_owned();
            paths.insert(guid.to_owned(), pathname);
        } else {
            assets.insert(guid.to_owned(), data);
        }
    }
    let mut anims: Vec<(String, Vec<u8>)> = paths.into_iter().filter(|(_, p)| p.ends_with(".anim")).filter_map(|(guid, p)| assets.remove(&guid).map(|a| (p, a))).collect();
    anims.sort();
    Ok(anims)
}

/// The GLB of the humanoid clip `anim` on riggy's Unity rig.
fn clip_glb(anim: &Anim, name: &str) -> Result<Vec<u8>, String> {
    let muscles: Vec<(usize, &str)> = anim.attributes().filter_map(|a| muscle_index(a).map(|m| (m, a))).collect();
    if muscles.is_empty() {
        return Err("no muscle curves (not a humanoid clip)".into());
    }
    let mirror = anim.mirror.then(|| mirror_map(&muscles));
    let rig = Rig::get();
    let frames = ((anim.stop - anim.start) * FPS).round().max(0.0) as usize + 1;
    let times: Vec<f64> = (0..frames).map(|i| i as f64 / FPS).collect();
    let mut tracks: Vec<Track> = rig
        .nodes
        .iter()
        .map(|n| Track {
            name: if n.name.is_empty() { "Root".to_owned() } else { n.name.clone() },
            parent: usize::try_from(n.parent).ok(),
            rest: gltf_pose(DVec3::from_array(n.t), DQuat::from_array(n.q)),
            poses: Vec::with_capacity(frames),
        })
        .collect();
    for &time in &times {
        let t = anim.start + time;
        let mut values = [0.0; MUSCLE_COUNT];
        for &(m, attribute) in &muscles {
            values[m] = anim.value(attribute, t).unwrap_or(0.0);
        }
        let root = ["x", "y", "z", "w"].map(|c| anim.value(&format!("RootQ.{c}"), t));
        let mut body = match root {
            [Some(x), Some(y), Some(z), Some(w)] => DQuat::from_xyzw(x, y, z, w),
            _ => DQuat::IDENTITY,
        };
        if let Some(map) = &mirror {
            values = std::array::from_fn(|m| map[m].map_or(values[m], |(from, sign)| sign * values[from]));
            body = DQuat::from_xyzw(body.x, -body.y, -body.z, body.w);
        }
        for (track, rotation) in tracks.iter_mut().zip(rig.rotations(&values, body)) {
            let rest = track.rest;
            track.poses.push(Pose { rotation: gltf_rotation(rotation), ..rest });
        }
    }
    Ok(glb(&tracks, &times, name).to_bytes())
}

/// Where each muscle of a mirrored clip takes its value from: the other side's muscle, the sign flipped for the trunk's and head's left-right muscles; `None` keeps the muscle's own value.
fn mirror_map(muscles: &[(usize, &str)]) -> [Option<(usize, f64)>; MUSCLE_COUNT] {
    let mut map = [None; MUSCLE_COUNT];
    for &(m, name) in muscles {
        let other = swap_side(name);
        let from = muscle_index(&other).unwrap_or(m);
        let sided = name.starts_with("Left") || name.starts_with("Right");
        let sign = if name.contains("Left-Right") && !sided { -1.0 } else { 1.0 };
        map[m] = Some((from, sign));
    }
    map
}

/// A muscle name with Left and Right swapped where they name the side (`Left Arm Down-Up`, `LeftHand.Index.Spread`), not a direction (`Spine Left-Right`).
fn swap_side(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("Left") {
        format!("Right{rest}")
    } else if let Some(rest) = name.strip_prefix("Right") {
        format!("Left{rest}")
    } else {
        name.to_owned()
    }
}

/// A Unity rotation in glTF space: Unity's X axis mirrored.
fn gltf_rotation(q: DQuat) -> DQuat {
    DQuat::from_xyzw(q.x, -q.y, -q.z, q.w)
}

fn gltf_pose(t: DVec3, q: DQuat) -> Pose {
    Pose { translation: DVec3::new(-t.x, t.y, t.z), rotation: gltf_rotation(q), scale: DVec3::ONE }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirroring_swaps_sides_and_flips_the_trunks_side_bend() {
        let muscles = [(muscle_index("Left Arm Down-Up").unwrap(), "Left Arm Down-Up"), (muscle_index("Spine Left-Right").unwrap(), "Spine Left-Right")];
        let map = mirror_map(&muscles);
        assert_eq!(map[muscles[0].0], Some((muscle_index("Right Arm Down-Up").unwrap(), 1.0)));
        assert_eq!(map[muscles[1].0], Some((muscles[1].0, -1.0)));
        assert_eq!(swap_side("LeftHand.Index.Spread"), "RightHand.Index.Spread");
    }
}
