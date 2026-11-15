//! Animation clips on humanoid avatars, as the behaviour files say: a turn clip while the avatar turns, talking clips while riggy speaks, now and then an idle clip in silence.
//!
//! A clip blends in and out over its set's `fade` and moves the avatar's bones only `intensity` of the way; riggy's procedural idle, stance and lip sync stay on top. Sets whose clips are not installed play nothing. The behaviour files, the installed clips and their bone maps are polled once a second; a change takes effect at once.

mod behaviour;
mod retarget;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use bevy::prelude::*;
use rand::Rng;
use rand::seq::IndexedRandom;
use riggy::clips::{self, Clip, ClipFile};

use self::behaviour::{Behaviour, Bones, Playing};
use self::retarget::{Retargeted, retarget};
use super::stance::Stance;
use crate::avatar::{AvatarSet, Humanoid};
use crate::config::Config;
use crate::speech::Voice;

/// Degrees left to turn below which a turn counts as done once the feet stand.
const TURN_DONE: f32 = 3.0;

/// Where the clip rotations go in the frame: before the procedural idle, which adds to them.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClipPose;

pub struct ClipsPlugin;

impl Plugin for ClipsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Library>().add_systems(Update, (reload, play.in_set(AvatarSet::Pose).in_set(ClipPose)).chain());
    }
}

/// The installed clips and the behaviour, reloaded when a file changes.
#[derive(Resource)]
struct Library {
    clips: Vec<ClipFile>,
    behaviour: Behaviour,
    /// Every file read, with its modification time.
    files: Vec<(PathBuf, Option<SystemTime>)>,
    /// Counts the reloads, so avatars drop what they built from older files.
    generation: u32,
    timer: Timer,
}

impl FromWorld for Library {
    fn from_world(_: &mut World) -> Self {
        Self { clips: Vec::new(), behaviour: Behaviour::default(), files: Vec::new(), generation: 0, timer: Timer::from_seconds(1.0, TimerMode::Repeating) }
    }
}

/// The behaviour files, the clip folders and the bone maps, with their modification times.
fn watched_files() -> Vec<(PathBuf, Option<SystemTime>)> {
    let mut files = vec![riggy::files::behaviour_dir(), riggy::files::animations_dir()];
    let list = |dir: &PathBuf| std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>();
    files.extend(list(&riggy::files::behaviour_dir()));
    for pack in list(&riggy::files::animations_dir()) {
        for sub in ["assets", "skeletons"] {
            files.push(pack.join(sub));
            files.extend(list(&pack.join(sub)));
        }
    }
    files.sort();
    files.into_iter().map(|f| (f.clone(), crate::config::modified(&f))).collect()
}

fn reload(time: Res<Time>, config: Res<Config>, mut library: ResMut<Library>) {
    let first = library.generation == 0;
    if !library.timer.tick(time.delta()).just_finished() && !first {
        return;
    }
    let files = watched_files();
    if !first && files == library.files {
        return;
    }
    library.files = files;
    library.clips = clips::installed();
    library.behaviour = Behaviour::load(&riggy::files::behaviour(), &riggy::files::avatar_behaviour(&config.avatar.name));
    library.generation += 1;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Talk,
    Idle,
    Turn,
}

/// A clip being played on an avatar.
struct Layer {
    kind: Kind,
    clip: Arc<Retargeted>,
    time: f32,
    /// How far it is blended in, 0 to 1.
    weight: f32,
    /// Whether it blends in (true) or out.
    on: bool,
    intensity: f32,
    fade: f32,
}

/// What an avatar plays, on the avatar root.
#[derive(Component, Default)]
struct Player {
    generation: u32,
    /// Retargeted clips by name and bones, `None` where a clip could not be read.
    built: HashMap<(String, Bones), Option<Arc<Retargeted>>>,
    layers: Vec<Layer>,
    /// Seconds until the next idle clip.
    idle_wait: Option<f32>,
    last: Option<String>,
}

impl Player {
    /// The clip `name` retargeted onto `h`, built once.
    fn clip(&mut self, library: &Library, name: &str, bones: Bones, h: &Humanoid) -> Option<Arc<Retargeted>> {
        self.built
            .entry((name.to_owned(), bones))
            .or_insert_with(|| {
                let file = clips::find(&library.clips, name)?;
                match Clip::load(&file.path, &file.skeletons_dir()) {
                    Ok(clip) => Some(Arc::new(retarget(&clip, h, |b| bones.includes(b)))),
                    Err(err) => {
                        eprintln!("riggy: animation {}: {err}", file.key());
                        None
                    }
                }
            })
            .clone()
    }

    /// Starts a random clip of `names` that is installed, another than the last one where there is a choice, and blends the others out over its fade or quicker.
    fn start(&mut self, library: &Library, kind: Kind, names: &[String], playing: &Playing, h: &Humanoid, from_anywhere: bool) -> bool {
        let available: Vec<&String> = names.iter().filter(|n| clips::find(&library.clips, n).is_some()).collect();
        let fresh: Vec<&String> = available.iter().copied().filter(|n| available.len() < 2 || Some(*n) != self.last.as_ref()).collect();
        let mut rng = rand::rng();
        let Some(name) = fresh.choose(&mut rng).map(|n| (*n).clone()) else { return false };
        let Some(clip) = self.clip(library, &name, playing.bones, h) else { return false };
        let fade = playing.fade.max(0.05);
        let room = clip.duration() - 2.0 * fade;
        let time = if from_anywhere && room > 1.0 { rng.random_range(0.0..room * 0.8) } else { 0.0 };
        // The clips it replaces fade out no slower than it fades in, so a quick turn is not held back by a slow idle clip.
        for layer in &mut self.layers {
            layer.on = false;
            layer.fade = layer.fade.min(fade);
        }
        self.layers.push(Layer { kind, clip, time, weight: 0.0, on: true, intensity: playing.intensity.clamp(0.0, 1.0), fade });
        self.last = Some(name);
        true
    }

    /// The clip blending in or playing, if any.
    fn current(&self) -> Option<&Layer> {
        self.layers.iter().rev().find(|l| l.on)
    }

    fn stop(&mut self, kind: Kind) {
        for layer in self.layers.iter_mut().filter(|l| l.kind == kind) {
            layer.on = false;
        }
    }
}

fn play(
    mut commands: Commands,
    time: Res<Time>,
    library: Res<Library>,
    voice: Res<Voice>,
    mut avatars: Query<(Entity, &Humanoid, Option<&Stance>, Option<&mut Player>)>,
    mut transforms: Query<&mut Transform>,
) {
    let dt = time.delta_secs();
    for (entity, h, stance, player) in &mut avatars {
        let Some(mut player) = player else {
            commands.entity(entity).insert(Player::default());
            continue;
        };
        if player.generation != library.generation {
            *player = Player { generation: library.generation, ..Default::default() };
        }
        let b = &library.behaviour;
        if b.enabled {
            choose(&mut player, &library, h, stance, voice.speaking, dt);
        } else {
            player.layers.iter_mut().for_each(|l| l.on = false);
        }

        for layer in &mut player.layers {
            layer.time += dt;
            if layer.time >= layer.clip.duration() - layer.fade {
                layer.on = false;
            }
            let step = dt / layer.fade;
            layer.weight = if layer.on { (layer.weight + step).min(1.0) } else { (layer.weight - step).max(0.0) };
            let amount = smooth(layer.weight) * layer.intensity;
            for (bone, rotation) in layer.clip.sample(layer.time) {
                h.rotate(bone, Quat::IDENTITY.slerp(rotation, amount), &mut transforms);
            }
        }
        player.layers.retain(|l| l.on || l.weight > 0.0);
    }
}

/// Starts and stops clips: a turn before talking, talking before idling.
fn choose(player: &mut Player, library: &Library, h: &Humanoid, stance: Option<&Stance>, speaking: bool, dt: f32) {
    let b = &library.behaviour;
    let current = player.current().map(|l| l.kind);
    let left = stance.map_or(0.0, |s| s.turning().to_degrees());
    let standing = stance.is_none_or(|s| !s.stepping());

    if current == Some(Kind::Turn) {
        if left.abs() < TURN_DONE && standing {
            player.stop(Kind::Turn);
        }
        return;
    }
    if left.abs() > b.turn.min_angle {
        let names = if left.abs() >= b.turn.around_angle && !b.turn.around.is_empty() {
            &b.turn.around
        } else if left > 0.0 {
            &b.turn.left
        } else {
            &b.turn.right
        };
        if player.start(library, Kind::Turn, names, &b.turn.playing, h, false) {
            return;
        }
    }

    if speaking {
        if current != Some(Kind::Talk) {
            player.start(library, Kind::Talk, &b.talk.clips, &b.talk.playing, h, true);
        }
        player.idle_wait = None;
        return;
    }
    player.stop(Kind::Talk);

    if current.is_some() {
        return;
    }
    let [lo, hi] = b.idle.pause;
    let (lo, hi) = (lo.max(0.0), hi.max(lo.max(0.0) + 0.01));
    let wait = player.idle_wait.get_or_insert_with(|| rand::rng().random_range(lo..hi));
    *wait -= dt;
    if *wait <= 0.0 {
        player.idle_wait = None;
        player.start(library, Kind::Idle, &b.idle.clips, &b.idle.playing, h, false);
    }
}

fn smooth(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}
