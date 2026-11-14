//! Rhubarb's animation rules (`animationRules.cpp`, `ShapeRule.cpp`, `targetShapeSet.cpp`): which shapes may show each phone, how shapes relax and blend.

use super::shape::{Phone, Shape, ShapeSet};
use super::timeline::{Cs, TimeRange, Timed, Timeline};

use Shape::{A, B, C, D, E, F, G, H, X};

/// The basic shape (A–F) a shape stands in for when extended shapes are not wanted.
pub fn basic_shape(shape: Shape) -> Shape {
    [A, B, C, D, E, F, A, C, A][shape as usize]
}

/// The shape the mouth relaxes into from `shape`.
pub fn relax(shape: Shape) -> Shape {
    [A, B, B, C, C, B, X, B, X][shape as usize]
}

/// The shape of `shapes` that needs the least effort to reach from `reference`.
pub fn closest_shape(reference: Shape, shapes: ShapeSet) -> Shape {
    // For each shape, all shapes in ascending order of the effort required to move to them.
    const EFFORT: [[Shape; 9]; 9] = [
        /* A */ [A, X, G, B, C, H, E, D, F],
        /* B */ [B, G, A, X, C, H, E, D, F],
        /* C */ [C, H, B, G, D, A, X, E, F],
        /* D */ [D, C, H, B, G, A, X, E, F],
        /* E */ [E, C, H, B, G, A, X, D, F],
        /* F */ [F, B, G, A, X, C, H, E, D],
        /* G */ [G, A, B, C, H, X, E, D, F],
        /* H */ [H, C, B, G, D, A, X, E, F], // Like C
        /* X */ [X, A, G, B, C, H, E, D, F], // Like A
    ];
    EFFORT[reference as usize].into_iter().find(|&s| shapes.contains(s)).expect("cannot select from an empty set of shapes")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TweenTiming {
    Early,
    Centered,
    Late,
}

/// The in-between shape for the transition `first` → `second`, if any. Most rules work in one direction only: in animation the mouth usually pops open without in-betweens, then closes slowly.
pub fn tween(first: Shape, second: Shape) -> Option<(Shape, TweenTiming)> {
    use TweenTiming::*;
    Some(match (first, second) {
        (D, A) => (C, Early),
        (D, B) => (C, Centered),
        (D, G) => (C, Early),
        (D, X) => (C, Late),
        (C, F) | (F, C) => (E, Centered),
        (D, F) => (E, Centered),
        (H, F) => (E, Late),
        (F, H) => (E, Early),
        _ => return None,
    })
}

/// The shapes allowed for one phone of `duration`, timed relative to the phone's start (plosives start before it).
pub fn shape_sets(phone: Phone, duration: Cs, previous_duration: Cs) -> Vec<Timed<ShapeSet>> {
    let set = ShapeSet::of;
    let single = |value: ShapeSet| vec![Timed::new(0, duration, value)];
    let diphthong = |first: ShapeSet, second: ShapeSet| {
        let first_duration = (duration as f64 * 0.6) as Cs;
        vec![Timed::new(0, first_duration, first), Timed::new(first_duration, duration, second)]
    };
    let plosive = |first: ShapeSet, second: ShapeSet| {
        let occlusion = (previous_duration / 2).clamp(4, 12);
        vec![Timed::new(-occlusion, 0, first), Timed::new(0, duration, second)]
    };
    let any = ShapeSet::all();
    let any_open = set(&[B, C, D, E, F, G, H]);

    // The shapes {A, B, G, X} are very similar; regular shape sets should not contain more than one of them, or the resulting shape is more or less random. A very flexible rule may contain all of them.
    match phone {
        Phone::AO => single(set(&[E])),
        Phone::AA => single(set(&[D])),
        Phone::IY => single(set(&[B])),
        Phone::UW => single(set(&[F])),
        Phone::EH => single(set(&[C])),
        Phone::IH => single(set(&[B])),
        Phone::UH => single(set(&[F])),
        Phone::AH => single(set(if duration < 20 { &[C] } else { &[D] })),
        Phone::Schwa => single(set(&[B, C])),
        Phone::AE => single(set(&[C])),
        Phone::EY => diphthong(set(&[C]), set(&[B])),
        Phone::AY => diphthong(set(if duration < 20 { &[C] } else { &[D] }), set(&[B])),
        Phone::OW => diphthong(set(&[E]), set(&[F])),
        Phone::AW => diphthong(set(if duration < 30 { &[C] } else { &[D] }), set(&[E])),
        Phone::OY => diphthong(set(&[E]), set(&[B])),
        Phone::ER if duration < 7 => shape_sets(Phone::Schwa, duration, previous_duration),
        Phone::ER => single(set(&[E])),

        Phone::P | Phone::B => plosive(set(&[A]), any),
        Phone::T | Phone::D => plosive(set(&[B, F]), any_open),
        Phone::K | Phone::G => plosive(set(&[B, C, E, F, H]), any_open),
        Phone::CH | Phone::JH => single(set(&[B, F])),
        Phone::F | Phone::V => single(set(&[G])),
        Phone::TH | Phone::DH | Phone::S | Phone::Z | Phone::SH | Phone::ZH => single(set(&[B, F])),
        Phone::HH => single(any), // think "m-hm"
        Phone::M => single(set(&[A])),
        Phone::N => single(set(&[B, C, F, H])),
        Phone::NG => single(set(&[B, C, E, F])),
        Phone::L => single(set(if duration < 20 { &[B, E, F, H] } else { &[H] })),
        Phone::R => single(set(&[B, E, F])),
        Phone::Y => single(set(&[B, C, F])),
        Phone::W => single(set(&[F])),

        Phone::Breath | Phone::Cough | Phone::Smack => single(set(&[C])),
        Phone::Noise => single(set(&[B])),
    }
}

/// The shapes allowed at a point in time, and the phone they come from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeRule {
    pub shape_set: ShapeSet,
    pub phone: Option<Phone>,
    pub phone_timing: TimeRange,
}

impl ShapeRule {
    pub fn invalid() -> Self {
        Self { shape_set: ShapeSet::default(), phone: None, phone_timing: TimeRange::new(0, 0) }
    }
}

/// Shape rules for a whole timeline of phones; silences get X.
pub fn shape_rules(phones: &Timeline<Phone>) -> Timeline<ShapeRule> {
    // A continuous copy, so that silences are visited too.
    let continuous_phones = Timeline::continuous(phones.range(), None, false).with(phones.iter().map(|p| Timed { range: p.range, value: Some(p.value) }));

    let mut rules = Timeline::continuous(phones.range(), ShapeRule { shape_set: ShapeSet::of(&[X]), phone: None, phone_timing: TimeRange::new(0, 0) }, false);
    let mut previous_duration = 0;
    for timed_phone in continuous_phones.iter() {
        let duration = timed_phone.duration();
        if let Some(phone) = timed_phone.value {
            // Later shape sets may overwrite earlier ones if they overlap.
            for timed_set in shape_sets(phone, duration, previous_duration) {
                let range = TimeRange::new(timed_set.start() + timed_phone.start(), timed_set.end() + timed_phone.start());
                rules.set(Timed { range, value: ShapeRule { shape_set: timed_set.value, phone: Some(phone), phone_timing: timed_phone.range } });
            }
        }
        previous_duration = duration;
    }
    rules
}

/// `shape`, or its basic shape if `target` does not contain it.
pub fn to_target_shape(shape: Shape, target: ShapeSet) -> Shape {
    if target.contains(shape) {
        return shape;
    }
    let basic = basic_shape(shape);
    assert!(target.contains(basic), "target shape set must contain basic shape {basic:?}");
    basic
}

pub fn to_target_shape_set(shapes: ShapeSet, target: ShapeSet) -> ShapeSet {
    ShapeSet::of(&shapes.iter().map(|s| to_target_shape(s, target)).collect::<Vec<_>>())
}

pub fn rules_to_target_shape_set(rules: &Timeline<ShapeRule>, target: ShapeSet) -> Timeline<ShapeRule> {
    let mut result = rules.clone();
    for timed in rules.iter() {
        let rule = ShapeRule { shape_set: to_target_shape_set(timed.value.shape_set, target), ..timed.value };
        result.set(Timed { range: timed.range, value: rule });
    }
    result
}

pub fn animation_to_target_shape_set(animation: &Timeline<Shape>, target: ShapeSet) -> Timeline<Shape> {
    let mut result = animation.clone();
    for timed in animation.iter() {
        result.set(Timed { range: timed.range, value: to_target_shape(timed.value, target) });
    }
    result
}
