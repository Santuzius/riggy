//! For checking animations against their clips: with `RIGGY_POSE_LOG=FILE`, riggy writes the avatar-space position of every humanoid bone, as finally posed, ten times a second to FILE (CSV: `time,bone,parent,x,y,z`, time in seconds since start). `tools/clip_shots.sh` sets it; `tools/clip_balance.py` and `tools/clip_sticks.py` read it.

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
                    let _ = writeln!(file, "time,bone,parent,x,y,z");
                    app.insert_resource(PoseLog { file, next: 0.0 }).add_systems(Update, log.after(AvatarSet::Apply));
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
}

fn log(time: Res<Time>, mut out: ResMut<PoseLog>, avatars: Query<&Humanoid>, tree: TransformTree) {
    let t = time.elapsed_secs();
    if t < out.next {
        return;
    }
    out.next = t + 0.1;
    let mut text = String::new();
    for h in &avatars {
        for &b in B::ALL {
            let Some(bone) = h.get(b) else { continue };
            let p = avatar_space(bone.entity, &tree).translation;
            let parent = parent_bone(b).filter(|&x| h.get(x).is_some()).map_or("", |x| x.as_str());
            text += &format!("{t:.2},{},{parent},{},{},{}\n", b.as_str(), p.x, p.y, p.z);
        }
    }
    let _ = out.file.write_all(text.as_bytes());
}
