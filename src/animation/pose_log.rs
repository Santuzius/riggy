//! For checking animations against their clips: with `RIGGY_POSE_LOG=FILE`, riggy writes the avatar-space position and the local rotation of every humanoid bone, as finally posed, ten times a second (every `RIGGY_POSE_LOG_STEP` seconds) to FILE (CSV: `time,bone,parent,x,y,z,qx,qy,qz,qw`, time in seconds since start). `tools/clip_shots.sh` sets it; `tools/clip_balance.py` and `tools/clip_sticks.py` read it.

use std::io::Write;

use bevy::prelude::*;

use super::clips::parent_bone;
use crate::avatar::{AvatarSet, Humanoid, HumanoidBone as B, TransformTree, avatar_space};

pub struct PoseLogPlugin;

impl Plugin for PoseLogPlugin {
    fn build(&self, app: &mut App) {
        if let Some(path) = std::env::var_os("RIGGY_POSE_LOG") {
            match std::fs::File::create(&path) {
                Ok(mut file) => {
                    let _ = writeln!(file, "time,bone,parent,x,y,z,qx,qy,qz,qw");
                    let step = std::env::var("RIGGY_POSE_LOG_STEP").ok().and_then(|s| s.parse().ok()).unwrap_or(0.1);
                    app.insert_resource(PoseLog { file, next: 0.0, step }).add_systems(Update, log.after(AvatarSet::Apply).after(super::clips::Contact));
                }
                Err(err) => error!("{}: {err}", path.display()),
            }
        }
    }
}

#[derive(Resource)]
struct PoseLog {
    file: std::fs::File,
    next: f32,
    step: f32,
}

fn log(time: Res<Time>, mut out: ResMut<PoseLog>, avatars: Query<&Humanoid>, tree: TransformTree, transforms: Query<&Transform>) {
    let t = time.elapsed_secs();
    if t < out.next {
        return;
    }
    out.next = t + out.step;
    let mut text = String::new();
    for h in &avatars {
        for &b in B::ALL {
            let Some(bone) = h.get(b) else { continue };
            let p = avatar_space(bone.entity, &tree).translation;
            let parent = parent_bone(b).filter(|&x| h.get(x).is_some()).map_or("", |x| x.as_str());
            let q = transforms.get(bone.entity).map_or(Quat::IDENTITY, |t| t.rotation);
            text += &format!("{t:.3},{},{parent},{},{},{},{},{},{},{}\n", b.as_str(), p.x, p.y, p.z, q.x, q.y, q.z, q.w);
        }
    }
    let _ = out.file.write_all(text.as_bytes());
}
