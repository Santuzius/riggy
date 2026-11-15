//! Standing: the feet keep their place on the floor while the body sways and turns, and step after the body when it has turned away from them.
//!
//! Places on the floor are in ground space: avatar space of the unturned figure. The figure turns about the vertical axis through the middle of its feet, so a turn rotates avatar space against ground space and the feet stay where they stand. [`stand`] moves the hips and bends hip, knee and ankle of each leg (two-bone inverse kinematics) so that the ankle reaches its foot's place and the foot keeps its heading, whatever the hips do. Once body and a foot point more than [`STEP_ANGLE`] apart, or a little apart after the turn has stopped, the feet step after the body one at a time.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use crate::avatar::{AvatarRoot, AvatarSet, Humanoid, HumanoidBone as B};
use crate::config::Config;

/// Degrees between body and a foot from which the foot steps at once, even while the figure still turns.
const STEP_ANGLE: f32 = 20.0;
/// Degrees between body and a foot from which the foot steps once the turn has rested for [`SETTLE_TIME`].
const SETTLE_ANGLE: f32 = 1.5;
/// Seconds the turn rests before the feet close a small difference.
const SETTLE_TIME: f32 = 0.3;
/// Largest turn of one step in degrees; a larger turn takes several.
const STEP_TURN: f32 = 35.0;
/// Seconds one step takes.
const STEP_TIME: f32 = 0.35;
/// How high a foot lifts in a step, as a fraction of the figure's height.
const STEP_HEIGHT: f32 = 0.02;
/// Fraction of the way from the hips to the standing ankle by which the hips move over the standing foot during a step.
const STEP_SHIFT: f32 = 0.3;
/// Degrees per second the body turns at most towards the turn asked for.
const TURN_RATE: f32 = 300.0;
/// Degrees the body leads the feet at most; beyond it the body waits for the feet.
const TURN_LEAD: f32 = 40.0;
/// Share of the angle between body and feet by which the pelvis stays turned towards the feet; the spine and chest turn it back, so the thighs twist less against the pelvis and a skirt does not kink at the waist.
const PELVIS_FOLLOW: f32 = 0.6;

pub struct StancePlugin;

impl Plugin for StancePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (stand_up, step).chain().before(AvatarSet::Pose));
    }
}

/// Where the figure stands: the turn of its body and the place and heading of each foot, all in ground space (lengths in model units, angles in radians, positive to the figure's left).
#[derive(Component, Debug)]
pub struct Stance {
    /// Turn of the body asked for (by the mouse or the control socket).
    pub wanted: f32,
    /// Turn of the body now.
    turn: f32,
    /// The turn's vertical axis: midway between the feet.
    pivot: Vec3,
    /// Rest positions of the ankles, left and right, in avatar space.
    ankles: [Vec3; 2],
    feet: [Foot; 2],
    /// Seconds since the body last turned.
    resting: f32,
    height: f32,
    /// Whether a clip turns the body and steps (a turn clip on an avatar whose legs the clips pose): the stance then waits, and takes the clip's turn over at its end ([`Stance::turn_by`]).
    pub held: bool,
}

#[derive(Clone, Copy, Debug)]
struct Foot {
    /// Ankle position in ground space, lifted during a step.
    place: Vec3,
    heading: f32,
    step: Option<Step>,
}

#[derive(Clone, Copy, Debug)]
struct Step {
    from: Vec3,
    from_heading: f32,
    to_heading: f32,
    /// 0 at lift-off, 1 on landing.
    progress: f32,
}

impl Stance {
    fn new(h: &Humanoid, turn: f32) -> Option<Self> {
        let ankles = [h.position(B::LeftFoot)?, h.position(B::RightFoot)?];
        let toes = [h.position(B::LeftToes).unwrap_or(ankles[0]), h.position(B::RightToes).unwrap_or(ankles[1])];
        let pivot = (ankles[0] + ankles[1] + toes[0] + toes[1]) / 4.0;
        let mut stance = Self {
            wanted: turn,
            turn,
            pivot: Vec3::new(pivot.x, 0.0, pivot.z),
            ankles,
            feet: [Foot { place: Vec3::ZERO, heading: turn, step: None }; 2],
            resting: 0.0,
            height: h.height,
            held: false,
        };
        for i in 0..2 {
            stance.feet[i].place = stance.home(i, turn);
        }
        Some(stance)
    }

    /// Whether a foot is in the air.
    pub fn stepping(&self) -> bool {
        self.feet.iter().any(|f| f.step.is_some())
    }

    /// Angle the figure still has to turn, from the middle of its feet to the turn asked for, positive to the figure's left.
    pub fn turning(&self) -> f32 {
        let feet_heading = self.feet[0].heading + wrapped(self.feet[1].heading - self.feet[0].heading) / 2.0;
        wrapped(self.wanted - feet_heading)
    }

    /// Angle from the body to the middle of the feet, positive to the figure's left.
    fn feet_lag(&self) -> f32 {
        let feet_heading = self.feet[0].heading + wrapped(self.feet[1].heading - self.feet[0].heading) / 2.0;
        wrapped(feet_heading - self.turn)
    }

    /// Ground-space place of ankle `i` when its foot points along `heading`.
    fn home(&self, i: usize, heading: f32) -> Vec3 {
        self.pivot + Quat::from_rotation_y(heading) * (self.ankles[i] - self.pivot)
    }

    /// Avatar-space target of ankle `i` and the foot's heading relative to the body.
    fn target(&self, i: usize) -> (Vec3, f32) {
        let foot = &self.feet[i];
        (self.pivot + Quat::from_rotation_y(-self.turn) * (foot.place - self.pivot), wrapped(foot.heading - self.turn))
    }

    /// The foot in the air and how far its step has come, as a weight rising from 0 at lift-off to 1 halfway and back to 0 on landing.
    fn step_weight(&self) -> Option<(usize, f32)> {
        self.feet.iter().enumerate().find_map(|(i, f)| f.step.map(|s| (i, (PI * s.progress).sin())))
    }

    /// Turns body and feet by `radians` at once, about the pivot: a clip's turn taken over.
    pub fn turn_by(&mut self, radians: f32) {
        let rotation = Quat::from_rotation_y(radians);
        self.turn += radians;
        for foot in &mut self.feet {
            foot.heading += radians;
            foot.place = self.pivot + rotation * (foot.place - self.pivot);
            foot.step = None;
        }
    }

    /// Makes the body's present turn the one asked for, so the stance does not turn on towards an older wish.
    pub fn stay(&mut self) {
        self.wanted = self.turn;
    }

    /// Turns the avatar root `transform` about the pivot as the body is turned.
    pub fn place(&self, transform: &mut Transform) {
        let rotation = Quat::from_rotation_y(self.turn);
        transform.rotation = rotation;
        transform.translation = transform.scale * (self.pivot - rotation * self.pivot);
    }

    /// Turns the body towards the turn asked for and moves the feet on by `dt` seconds.
    fn advance(&mut self, dt: f32) {
        if self.held {
            return;
        }
        let feet_heading = self.feet[0].heading + wrapped(self.feet[1].heading - self.feet[0].heading) / 2.0;
        let wanted = self.turn + wrapped(self.wanted - self.turn);
        let lead = TURN_LEAD.to_radians();
        let turn = self.turn + (wanted - self.turn).clamp(-TURN_RATE.to_radians() * dt, TURN_RATE.to_radians() * dt);
        let turn = feet_heading + wrapped(turn - feet_heading).clamp(-lead, lead);
        if (turn - self.turn).abs() > 1e-5 {
            self.resting = 0.0;
        } else {
            self.resting += dt;
        }
        self.turn = turn;

        if let Some(i) = self.feet.iter().position(|f| f.step.is_some()) {
            let height = STEP_HEIGHT * self.height;
            let foot = self.feet[i];
            let mut step = foot.step.unwrap();
            step.progress = (step.progress + dt / STEP_TIME).min(1.0);
            let eased = smooth(step.progress);
            let heading = step.from_heading + (step.to_heading - step.from_heading) * eased;
            let place = step.from.lerp(self.home(i, step.to_heading), eased) + Vec3::Y * height * (PI * step.progress).sin();
            self.feet[i] = Foot { place, heading, step: (step.progress < 1.0).then_some(step) };
            return;
        }
        let behind = |f: &Foot| wrapped(self.turn - f.heading);
        let (i, gap) = (0..2).map(|i| (i, behind(&self.feet[i]))).max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap();
        if gap.abs() > STEP_ANGLE.to_radians() || (self.resting > SETTLE_TIME && gap.abs() > SETTLE_ANGLE.to_radians()) {
            let foot = &mut self.feet[i];
            let limit = STEP_TURN.to_radians();
            foot.step = Some(Step {
                from: foot.place,
                from_heading: foot.heading,
                to_heading: foot.heading + gap.clamp(-limit, limit),
                progress: 0.0,
            });
        }
    }
}

/// `radians` as an angle between -π and π.
fn wrapped(radians: f32) -> f32 {
    (radians + PI).rem_euclid(TAU) - PI
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Gives a newly resolved humanoid with both feet mapped its stance, turned as `[window] turn` says.
fn stand_up(mut commands: Commands, config: Res<Config>, avatars: Query<(Entity, &Humanoid), Without<Stance>>) {
    for (entity, h) in &avatars {
        if let Some(stance) = Stance::new(h, config.window.turn.to_radians()) {
            commands.entity(entity).insert(stance);
        }
    }
}

/// Moves the stance on and turns the avatar root about the stance's pivot.
fn step(time: Res<Time>, mut avatars: Query<(&mut Stance, &mut Transform), With<AvatarRoot>>) {
    for (mut stance, mut transform) in &mut avatars {
        stance.advance(time.delta_secs());
        stance.place(&mut transform);
    }
}

/// Moves and turns the hips as idle motion asks (`sway` in avatar space, `roll` about the hips' pivot), turns the pelvis partly towards the feet and the spine back, shifts them over the standing foot during a step, lowers them as far as the legs need to reach the floor, and bends the legs so the ankles reach their places.
pub fn stand(h: &Humanoid, stance: &Stance, sway: Vec3, roll: Quat, clip_hips: Quat, transforms: &mut Query<&mut Transform>) {
    const LEGS: [(B, B, B); 2] = [(B::LeftUpperLeg, B::LeftLowerLeg, B::LeftFoot), (B::RightUpperLeg, B::RightLowerLeg, B::RightFoot)];
    let Some(hips) = h.position(B::Hips) else { return };
    let twist = Quat::from_rotation_y(PELVIS_FOLLOW * stance.feet_lag());
    let roll = twist * roll;
    let Some(legs) = LEGS.iter().map(|&(upper, lower, foot)| Some((upper, lower, foot, h.position(upper)?, h.position(lower)?, h.position(foot)?))).collect::<Option<Vec<_>>>() else {
        return;
    };

    let mut shift = sway;
    if let Some((stepping, weight)) = stance.step_weight() {
        let standing = stance.target(1 - stepping).0;
        shift += STEP_SHIFT * weight * Vec3::new(standing.x - hips.x, 0.0, standing.z - hips.z);
    }
    // The pelvis as it ends up: a clip's turn of the hips (already applied), then the roll.
    let pelvis = roll * clip_hips;
    let hip_joint = |rest: Vec3| hips + shift + pelvis * (rest - hips);
    // No leg stretches further than in the rest pose: where one would, the hips sink.
    let mut sink = 0.0f32;
    for (i, &(_, _, _, hip0, _, ankle0)) in legs.iter().enumerate() {
        let reach = (ankle0 - hip0).length();
        let to = stance.target(i).0 - hip_joint(hip0);
        let across = Vec2::new(to.x, to.z).length();
        if across < reach {
            sink = sink.max(-to.y - (reach * reach - across * across).sqrt());
        }
    }
    let offset = shift - Vec3::Y * sink;
    h.translate(B::Hips, offset, transforms);
    h.rotate(B::Hips, roll, transforms);
    let back = Quat::IDENTITY.slerp(twist.inverse(), 0.5);
    h.rotate(B::Spine, back, transforms);
    h.rotate(B::Chest, back, transforms);

    for (i, &(upper, lower, foot, hip0, knee0, ankle0)) in legs.iter().enumerate() {
        let (target, heading) = stance.target(i);
        let pose = leg_pose(hip0, knee0, ankle0, hip_joint(hip0) - Vec3::Y * sink, pelvis, target, heading);
        h.rotate(upper, pose.thigh, transforms);
        h.rotate(lower, pose.knee, transforms);
        h.rotate(foot, pose.foot, transforms);
    }
}

/// Avatar-space rotations of a leg's bones, each about its own pivot and applied before the rotations of the bones above it.
#[derive(Debug)]
struct LegPose {
    thigh: Quat,
    knee: Quat,
    foot: Quat,
}

/// Bends a leg with rest joints `hip0`, `knee0`, `ankle0` so its ankle reaches `target` from the hip joint's new place `hip`, with the foot level as at rest and turned by `heading` about the vertical. `parent` is the rotation the hips gave the leg.
///
/// The knee is a hinge about the avatar's X axis; it bends forward only. Out of reach the leg points straight at the target.
fn leg_pose(hip0: Vec3, knee0: Vec3, ankle0: Vec3, hip: Vec3, parent: Quat, target: Vec3, heading: f32) -> LegPose {
    let (thigh, shin) = (knee0 - hip0, ankle0 - knee0);
    let (l1, l2) = (thigh.length(), shin.length());
    let to = target - hip;
    let distance = to.length().clamp((l1 - l2).abs() + 1e-3, l1 + l2);

    // Knee: the shin swings about X until hip and ankle are `distance` apart. Seen along X, `bend` is the angle from the thigh to the shin, positive with the knee forward.
    let side = |v: Vec3| Vec2::new(v.y, v.z);
    let angle = |v: Vec3| v.z.atan2(v.y);
    let rest_bend = wrapped(angle(shin) - angle(thigh));
    let cos = ((distance * distance - l1 * l1 - l2 * l2) / 2.0 - thigh.x * shin.x) / (side(thigh).length() * side(shin).length()).max(1e-6);
    let knee = Quat::from_rotation_x(cos.clamp(-1.0, 1.0).acos() - rest_bend);

    // Whole leg: turned with the foot, then swung so the ankle lies on the line to the target.
    let yaw = Quat::from_rotation_y(heading);
    let reach = yaw * (thigh + knee * shin);
    let total = Quat::from_rotation_arc(reach.normalize(), to.normalize()) * yaw;
    LegPose {
        thigh: parent.inverse() * total,
        knee,
        foot: (total * knee).inverse() * yaw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HIP: Vec3 = Vec3::new(88.3, 889.1, -80.4);
    const KNEE: Vec3 = Vec3::new(134.3, 534.1, -101.2);
    const ANKLE: Vec3 = Vec3::new(182.5, 179.5, -154.7);

    /// Ankle position and foot rotation that a pose gives, by forward kinematics.
    fn ankle_and_foot(pose: &LegPose, hip: Vec3, parent: Quat) -> (Vec3, Quat) {
        let leg = parent * pose.thigh;
        (hip + leg * ((KNEE - HIP) + pose.knee * (ANKLE - KNEE)), leg * pose.knee * pose.foot)
    }

    #[test]
    fn the_ankle_reaches_its_place_whatever_the_hips_do() {
        let parent = Quat::from_rotation_z(0.03);
        let hip = HIP + Vec3::new(12.0, -15.0, 3.0);
        for heading in [0.0, 0.3, -0.5] {
            let target = ANKLE + Vec3::new(-20.0, 10.0, 15.0);
            let pose = leg_pose(HIP, KNEE, ANKLE, hip, parent, target, heading);
            let (ankle, foot) = ankle_and_foot(&pose, hip, parent);
            assert!(ankle.distance(target) < 0.05, "ankle at {ankle}, target {target}");
            assert!(foot.angle_between(Quat::from_rotation_y(heading)) < 1e-3, "foot turned by {foot}");
        }
    }

    #[test]
    fn the_rest_pose_stays_the_rest_pose() {
        let pose = leg_pose(HIP, KNEE, ANKLE, HIP, Quat::IDENTITY, ANKLE, 0.0);
        for rotation in [pose.thigh, pose.knee, pose.foot] {
            assert!(rotation.angle_between(Quat::IDENTITY) < 1e-3, "{pose:?}");
        }
    }

    #[test]
    fn a_lower_hip_bends_the_knee_forward() {
        let hip = HIP - Vec3::Y * 20.0;
        let pose = leg_pose(HIP, KNEE, ANKLE, hip, Quat::IDENTITY, ANKLE, 0.0);
        let knee = hip + pose.thigh * (KNEE - HIP);
        assert!(knee.z > KNEE.z + 5.0, "knee at {knee}");
    }

    fn stance() -> Stance {
        let mut h = Humanoid { height: 1620.0, ..default() };
        let mut bone = |b, position| {
            h.bones.insert(b, crate::avatar::PoseBone::new(Entity::PLACEHOLDER, Transform::from_translation(position), bevy::math::Affine3A::IDENTITY));
        };
        bone(B::LeftFoot, ANKLE);
        bone(B::RightFoot, ANKLE * Vec3::new(-1.0, 1.0, 1.0));
        Stance::new(&h, 0.0).unwrap()
    }

    #[test]
    fn the_feet_step_after_a_turn_and_end_under_the_body() {
        let mut stance = stance();
        stance.wanted = 90f32.to_radians();
        let mut steps = 0;
        let mut was_stepping = false;
        for _ in 0..600 {
            stance.advance(1.0 / 60.0);
            if stance.stepping() && !was_stepping {
                steps += 1;
            }
            was_stepping = stance.stepping();
            let lead = stance.feet.iter().map(|f| wrapped(stance.turn - f.heading).abs()).fold(0.0, f32::max);
            assert!(lead <= (TURN_LEAD + STEP_TURN).to_radians() + 1e-3);
        }
        assert!(!stance.stepping());
        assert!((stance.turn - 90f32.to_radians()).abs() < 1e-4);
        assert!(steps >= 4, "{steps} steps");
        for i in 0..2 {
            let (target, heading) = stance.target(i);
            assert!(target.distance(stance.ankles[i]) < 0.5, "foot {i} at {target}");
            assert!(heading.abs() < SETTLE_ANGLE.to_radians());
        }
    }

    #[test]
    fn the_feet_stay_put_while_the_body_stands_still() {
        let mut stance = stance();
        let before: Vec<Vec3> = stance.feet.iter().map(|f| f.place).collect();
        for _ in 0..120 {
            stance.advance(1.0 / 60.0);
        }
        assert!(!stance.stepping());
        assert_eq!(before, stance.feet.iter().map(|f| f.place).collect::<Vec<_>>());
    }
}
