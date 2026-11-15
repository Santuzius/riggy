//! Animation clips on humanoid avatars, as the behaviour files say: a turn clip while the avatar turns, talking clips while riggy speaks, now and then an idle clip in silence.
//!
//! A clip blends in and out over its set's `fade` and moves the avatar's bones only `intensity` of the way; riggy's procedural idle, stance and lip sync stay on top, except that the procedural arm pose gives way to a clip that moves the arms ([`ClipCover`]). Sets whose clips are not installed play nothing. The tray switches the behaviour's clips off and on (`Command::Clips`) and plays a single clip on request (`Command::Play`), also while they are off. The behaviour files, the installed clips and their bone maps are polled once a second; a change takes effect at once.

mod behaviour;
mod retarget;
#[cfg(test)]
mod sticks;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use bevy::prelude::*;
use rand::Rng;
use rand::seq::IndexedRandom;
use riggy::clips::{self, Clip, ClipFile};

use self::behaviour::{Behaviour, Bones, Playing};
pub use self::retarget::parent as parent_bone;
use self::retarget::{Retargeted, retarget};
use riggy::clips::calibration::Calibration;
use super::stance::Stance;
use crate::avatar::{AvatarSet, Humanoid, HumanoidBone as B};
use crate::config::Config;
use crate::commands::Received;
use crate::speech::Voice;
use riggy::control::Command;

/// Degrees left to turn below which a turn counts as done once the feet stand.
const TURN_DONE: f32 = 3.0;

/// Where the clip rotations go in the frame: before the procedural idle, which adds to them.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClipPose;

pub struct ClipsPlugin;

impl Plugin for ClipsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Library>().init_resource::<Requests>().add_systems(Update, (reload, listen, play.in_set(AvatarSet::Pose).in_set(ClipPose)).chain());
    }
}

/// The installed clips and the behaviour, reloaded when a file changes.
#[derive(Resource)]
struct Library {
    clips: Vec<ClipFile>,
    behaviour: Behaviour,
    /// Degrees the avatar's arms hang out at ease (`[idle] arm_angle`), written into new calibrations.
    arm_angle: f32,
    /// The avatar's name, which names its calibration files.
    avatar: String,
    /// The clips (`pack/clip`) approved on the avatar in their pack's calibration: the behaviour plays only these.
    approved: std::collections::HashSet<String>,
    /// Every file read, with its modification time.
    files: Vec<(PathBuf, Option<SystemTime>)>,
    /// Counts the reloads, so avatars drop what they built from older files.
    generation: u32,
    timer: Timer,
}

impl FromWorld for Library {
    fn from_world(_: &mut World) -> Self {
        Self { clips: Vec::new(), behaviour: Behaviour::default(), arm_angle: 0.0, avatar: String::new(), approved: Default::default(), files: Vec::new(), generation: 0, timer: Timer::from_seconds(1.0, TimerMode::Repeating) }
    }
}

/// The behaviour files, the clip folders and the bone maps, with their modification times.
fn watched_files() -> Vec<(PathBuf, Option<SystemTime>)> {
    let mut files = vec![riggy::files::behaviour_dir(), riggy::files::animations_dir()];
    let list = |dir: &PathBuf| std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>();
    files.extend(list(&riggy::files::behaviour_dir()));
    for pack in list(&riggy::files::animations_dir()) {
        for sub in ["assets", "skeletons", "calibration"] {
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
    let (arm_angle, avatar) = (config.idle.arm_angle, &config.avatar.name);
    if !first && files == library.files && arm_angle == library.arm_angle && *avatar == library.avatar {
        return;
    }
    library.arm_angle = arm_angle;
    library.avatar = avatar.clone();
    library.files = files;
    library.clips = clips::installed();
    library.approved = library.clips.iter().filter(|c| c.approved(avatar)).map(ClipFile::key).collect();
    library.behaviour = Behaviour::load(&riggy::files::behaviour(), &riggy::files::avatar_behaviour(&config.avatar.name));
    library.generation += 1;
}

/// What the tray asked for: whether the behaviour's clips play, and clips to play once.
#[derive(Resource)]
struct Requests {
    clips_on: bool,
    /// Whether the behaviour also plays clips not approved on the avatar.
    unapproved: bool,
    play: Vec<String>,
}

impl Default for Requests {
    fn default() -> Self {
        Self { clips_on: true, unapproved: false, play: Vec::new() }
    }
}

fn listen(mut received: MessageReader<Received>, mut requests: ResMut<Requests>) {
    for Received(command) in received.read() {
        match command {
            Command::Clips(on) => requests.clips_on = *on,
            Command::Unapproved(on) => requests.unapproved = *on,
            Command::Play(name) => requests.play.push(name.clone()),
            _ => {}
        }
    }
}

/// The calibration of `file`'s pack for the avatar: its file as set by hand (`auto = false`), else measured on the neutral clip that `clip`'s bone map names and written to the file where it changed. `None` where there is neither.
fn calibration(library: &Library, file: &ClipFile, clip: &Clip) -> Option<Calibration> {
    let path = file.calibration_file(&library.avatar);
    let stored = file.calibration(&library.avatar);
    if let Some(c) = stored.as_ref().filter(|c| !c.auto) {
        return Some(c.clone());
    }
    let Some(name) = clip.neutral.as_deref() else { return stored };
    let found = clips::find(&library.clips, &format!("{}/{name}", file.pack)).or_else(|| clips::find(&library.clips, name));
    let neutral = match found.map(|n| Clip::load(&n.path, &n.skeletons_dir())) {
        Some(Ok(n)) => n,
        Some(Err(err)) => {
            eprintln!("riggy: neutral animation {name}: {err}");
            return stored;
        }
        None => {
            eprintln!("riggy: the neutral animation {name} of {} is not installed; the trunk follows the actor's posture", file.key());
            return stored;
        }
    };
    let mut measured = Calibration::measure(&neutral, name, library.arm_angle);
    measured.approved = stored.as_ref().map(|c| c.approved.clone()).unwrap_or_default();
    if (stored.as_ref() != Some(&measured) || !path.exists())
        && let Err(err) = measured.save(&path)
    {
        eprintln!("riggy: {}: {err}", path.display());
    }
    Some(measured)
}

/// Whether `bone` belongs to a leg. Clips leave the legs to the stance, which bends them from the clip's pelvis to where the feet stand, so the feet stay planted and under the body however the actor stood.
pub(crate) fn is_leg(bone: B) -> bool {
    matches!(bone, B::LeftUpperLeg | B::LeftLowerLeg | B::LeftFoot | B::LeftToes | B::RightUpperLeg | B::RightLowerLeg | B::RightFoot | B::RightToes)
}

/// Whether `bone` is an eye or the jaw. Clips leave them to riggy's gaze and lip sync: an actor's eye bones turn about other axes than the avatar's, so copied they look aside.
pub(crate) fn is_face(bone: B) -> bool {
    matches!(bone, B::LeftEye | B::RightEye | B::Jaw)
}

/// How a clip asked for from the tray plays: on the whole body, fully.
const ASKED: Playing = Playing { intensity: 1.0, fade: 0.3, bones: Bones::All };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Talk,
    Idle,
    Turn,
    /// Asked for from the tray: plays to its end, whatever the behaviour says.
    Asked,
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

/// How strongly the clips pose each part of an avatar now, from 0 to 1, on the avatar root: riggy's procedural idle motion of that part gives way by as much, so it does not add to the clip's. The stance, which keeps the feet on the floor, stays.
#[derive(Component, Default, Debug, Clone, Copy)]
pub struct ClipCover {
    /// Hips, spine and chest: breathing and weight shift.
    pub torso: f32,
    /// Neck and head: the head's drift.
    pub head: f32,
    /// Upper and lower arms: the arm pose.
    pub arms: f32,
    /// The clips' turn of the hips (avatar space), from which the stance bends the legs to the feet.
    pub hips: Quat,
}

/// What an avatar plays, on the avatar root.
#[derive(Component, Default)]
struct Player {
    generation: u32,
    /// Retargeted clips by name and bones, `None` where a clip could not be read.
    built: HashMap<(String, Bones), Option<Arc<Retargeted>>>,
    layers: Vec<Layer>,
    /// The behaviour's base idle clip, looping under the others.
    base: Option<Base>,
    /// Whether the behaviour may start clips not approved on the avatar.
    unapproved: bool,
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
                    Ok(clip) => {
                        let calibration = calibration(library, file, &clip);
                        Some(Arc::new(retarget(&clip, calibration.as_ref(), h, |b| bones.includes(b) && !is_leg(b) && !is_face(b))))
                    }
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
        let allowed = |c: &ClipFile| kind == Kind::Asked || self.unapproved || library.approved.contains(&c.key());
        let available: Vec<&String> = names.iter().filter(|n| clips::find(&library.clips, n).is_some_and(allowed)).collect();
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
    mut requests: ResMut<Requests>,
    voice: Res<Voice>,
    mut avatars: Query<(Entity, &Humanoid, Option<&Stance>, Option<&mut Player>, Option<&mut ClipCover>)>,
    mut transforms: Query<&mut Transform>,
) {
    let dt = time.delta_secs();
    // An ask waits for the next frame while there is no avatar to play it on: one sent right after startup arrives before the avatar is loaded.
    if !avatars.iter_mut().any(|(_, _, _, player, _)| player.is_some()) {
        for (entity, _, _, player, _) in &mut avatars {
            if player.is_none() {
                commands.entity(entity).insert((Player::default(), ClipCover::default()));
            }
        }
        return;
    }
    let asked = std::mem::take(&mut requests.play);
    for (entity, h, stance, player, cover) in &mut avatars {
        let (Some(mut player), Some(mut cover)) = (player, cover) else {
            commands.entity(entity).insert((Player::default(), ClipCover::default()));
            continue;
        };
        if player.generation != library.generation {
            *player = Player { generation: library.generation, ..Default::default() };
        }
        for name in &asked {
            if !player.start(&library, Kind::Asked, std::slice::from_ref(name), &ASKED, h, false) {
                eprintln!("riggy: no animation {name}");
            }
        }
        player.unapproved = requests.unapproved;
        let b = &library.behaviour;
        // A clip asked for plays to its end; otherwise the behaviour chooses, unless its clips are off.
        let asked_playing = player.current().is_some_and(|l| l.kind == Kind::Asked);
        if !asked_playing {
            if b.enabled && requests.clips_on {
                choose(&mut player, &library, h, stance, voice.speaking, dt);
            } else {
                player.layers.iter_mut().for_each(|l| l.on = false);
            }
        }

        *cover = ClipCover::default();
        // The layers' rotations of each bone, combined, on top of the base clip.
        let mut pose: HashMap<B, Quat> = HashMap::new();
        let base_wanted = b.enabled && requests.clips_on && !asked_playing;
        update_base(&mut player, &library, h, base_wanted, dt);
        let over: f32 = player.layers.iter().map(|l| smooth(l.weight) * l.intensity).sum();
        if let Some(base) = &player.base {
            let amount = smooth(base.weight) * (1.0 - over).clamp(0.0, 1.0);
            cover.torso = cover.torso.max(if base.clip.moves_any(&[B::Hips, B::Spine, B::Chest, B::UpperChest]) { amount } else { 0.0 });
            cover.head = cover.head.max(if base.clip.moves_any(&[B::Neck, B::Head]) { amount } else { 0.0 });
            cover.arms = cover.arms.max(if base.clip.moves_any(&[B::LeftUpperArm, B::RightUpperArm]) { amount } else { 0.0 });
            for (bone, rotation) in base.sample() {
                pose.insert(bone, Quat::IDENTITY.slerp(rotation, amount));
            }
        }
        for layer in &mut player.layers {
            layer.time += dt;
            if layer.time >= layer.clip.duration() - layer.fade {
                layer.on = false;
            }
            let step = dt / layer.fade;
            layer.weight = if layer.on { (layer.weight + step).min(1.0) } else { (layer.weight - step).max(0.0) };
            let amount = smooth(layer.weight) * layer.intensity;
            let covers = |bones: &[B]| if layer.clip.moves_any(bones) { amount } else { 0.0 };
            cover.torso = (cover.torso + covers(&[B::Hips, B::Spine, B::Chest, B::UpperChest])).min(1.0);
            cover.head = (cover.head + covers(&[B::Neck, B::Head])).min(1.0);
            cover.arms = (cover.arms + covers(&[B::LeftUpperArm, B::RightUpperArm, B::LeftLowerArm, B::RightLowerArm])).min(1.0);
            for (bone, rotation) in layer.clip.sample(layer.time) {
                let q = pose.entry(bone).or_default();
                *q = Quat::IDENTITY.slerp(rotation, amount) * *q;
            }
        }
        cover.hips = pose.get(&B::Hips).copied().unwrap_or_default();
        for (bone, rotation) in pose {
            h.rotate(bone, rotation, &mut transforms);
        }
        player.layers.retain(|l| l.on || l.weight > 0.0);
    }
}

/// The behaviour's base idle clip, looping.
struct Base {
    name: String,
    clip: Arc<Retargeted>,
    time: f32,
    /// How far it is blended in, 0 to 1.
    weight: f32,
    fade: f32,
}

impl Base {
    /// The clip's rotations now; over its last `fade` seconds it blends into its start, where it goes on, so the loop shows no seam.
    fn sample(&self) -> Vec<(B, Quat)> {
        let (d, f) = (self.clip.duration(), self.fade.min(self.clip.duration() / 3.0));
        let now: Vec<(B, Quat)> = self.clip.sample(self.time).collect();
        if self.time <= d - f {
            return now;
        }
        let k = smooth((self.time - (d - f)) / f);
        now.into_iter().zip(self.clip.sample(self.time - (d - f))).map(|((b, q), (_, start))| (b, q.slerp(start, k))).collect()
    }
}

/// Blends the base idle clip (`[idle] base`) in while `wanted` and it is installed and allowed on the avatar, out otherwise, and moves it on.
fn update_base(player: &mut Player, library: &Library, h: &Humanoid, wanted: bool, dt: f32) {
    let b = &library.behaviour;
    let name = b.idle.base.as_ref().filter(|_| wanted).and_then(|n| clips::find(&library.clips, n)).filter(|c| player.unapproved || library.approved.contains(&c.key())).map(ClipFile::key);
    if let Some(name) = &name
        && player.base.as_ref().is_none_or(|base| base.name != *name && base.weight <= 0.0)
        && let Some(clip) = player.clip(library, name, Bones::All, h)
    {
        let fade = b.idle.playing.fade.max(0.05);
        player.base = Some(Base { name: name.clone(), clip, time: 0.0, weight: player.base.as_ref().map_or(0.0, |b| b.weight), fade });
    }
    let Some(base) = &mut player.base else { return };
    let on = name.as_ref() == Some(&base.name);
    let step = dt / base.fade;
    base.weight = if on { (base.weight + step).min(1.0) } else { (base.weight - step).max(0.0) };
    base.time += dt;
    let (d, f) = (base.clip.duration(), base.fade.min(base.clip.duration() / 3.0));
    if base.time >= d {
        base.time -= d - f;
    }
    if !on && base.weight <= 0.0 {
        player.base = None;
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
