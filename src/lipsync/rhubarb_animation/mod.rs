//! Mouth animation from timed phones: a port of Rhubarb Lip Sync's animation stage (`rhubarb/src/animation` of https://github.com/DanielSWolf/rhubarb-lip-sync, version 1.14.0, MIT license, see `LICENSE` in this folder).
//!
//! Rhubarb recognises phones in recorded speech and then turns them into mouth shapes with hand-written rules; this folder holds the second half, so that riggy can feed it phones from its own source (the text-driven lip sync in `lipsync::text`). The behaviour follows the original step by step, including the order in which Rhubarb visits neighbouring shapes (`timeline::for_each_adjacent`).

mod rules;
mod shape;
mod steps;
pub mod timeline;

pub use shape::{Phone, Shape, ShapeSet};

use rules::{ShapeRule, animation_to_target_shape_set, rules_to_target_shape_set, shape_rules};
use timeline::{Cs, FindMode, TimeRange, Timed, Timeline};

/// Mouth shapes for `phones` (a bounded timeline; gaps are silence), using the shapes in `target` (it must contain A–F).
pub fn animate(phones: &Timeline<Phone>, target: ShapeSet) -> Timeline<Shape> {
    // Only allowed shapes, plus X, which pauses need and which is replaced later.
    let mut target_plus_x = target;
    target_plus_x.insert(Shape::X);
    let rules = rules_to_target_shape_set(&shape_rules(phones), target_plus_x);

    let main_steps = |rules: &Timeline<ShapeRule>| {
        let animation = steps::animate_rough(rules);
        let animation = steps::optimize_timing(&animation);
        let animation = steps::animate_pauses(&animation);
        let animation = steps::insert_tweens(&animation);
        animation_to_target_shape_set(&animation, target)
    };
    avoid_static_segments(&rules, &main_steps)
}

// Static segments (`staticSegments.cpp`): prolonged stretches during which the mouth shape doesn't change.

fn syllable_count(rules: &Timeline<ShapeRule>, range: TimeRange) -> usize {
    if range.is_empty() {
        return 0;
    }
    let (Some(begin), Some(last)) = (rules.find(range.start, FindMode::SampleRight), rules.find(range.end, FindMode::SampleLeft)) else { return 0 };
    // Every vowel is one syllable; phones mostly outside the range don't count.
    rules.elements()[begin..=last]
        .iter()
        .filter(|timed| {
            let middle = timed.value.phone_timing.middle();
            middle >= range.start && middle < range.end && timed.value.phone.is_some_and(|p| p.is_vowel())
        })
        .count()
}

fn static_segments(rules: &Timeline<ShapeRule>, animation: &Timeline<Shape>) -> Vec<TimeRange> {
    // At least this many syllables and this long to look distractingly static; the same number of syllables in fast speech usually looks fine.
    const MIN_SYLLABLES: usize = 3;
    const MIN_DURATION: Cs = 75;
    animation.iter().map(|t| t.range).filter(|&r| r.duration() >= MIN_DURATION && syllable_count(rules, r) >= MIN_SYLLABLES).collect()
}

/// A rule that may be replaced to break up a static segment.
fn can_change(rule: &ShapeRule) -> bool {
    rule.phone.is_some_and(|p| p.is_vowel()) && rule.shape_set.len() == 1
}

/// The same rule with a slightly different look. So far only B has shown up as a static shape.
fn changed_rule(rule: &ShapeRule) -> ShapeRule {
    let mut result = *rule;
    if rule.shape_set == ShapeSet::of(&[Shape::B]) {
        result.shape_set = ShapeSet::of(&[Shape::C]);
    }
    result
}

fn apply_changes(rules: &Timeline<ShapeRule>, changes: &[Cs]) -> Timeline<ShapeRule> {
    let mut result = rules.clone();
    for &start in changes {
        let original = rules.get(start).expect("changed rule exists").clone();
        result.set(Timed { range: original.range, value: changed_rule(&original.value) });
    }
    result
}

struct Scenario {
    rules: Timeline<ShapeRule>,
    animation: Timeline<Shape>,
    static_segments: usize,
}

impl Scenario {
    fn new(original: &Timeline<ShapeRule>, changes: &[Cs], animate: &dyn Fn(&Timeline<ShapeRule>) -> Timeline<Shape>) -> Self {
        let rules = apply_changes(original, changes);
        let animation = animate(&rules);
        let static_segments = static_segments(&rules, &animation).len();
        Self { rules, animation, static_segments }
    }

    fn is_better_than(&self, other: &Scenario) -> bool {
        // Zero static segments first; then short shapes over long ones (smallest sum of squared durations).
        if self.static_segments == 0 && other.static_segments != 0 {
            return true;
        }
        self.duration_squares() < other.duration_squares()
    }

    fn duration_squares(&self) -> f64 {
        self.animation.iter().map(|t| (t.duration() as f64 / 100.0).powi(2)).sum()
    }
}

/// All `k`-element combinations of `0..n`, in lexicographic order (as Rhubarb's `next_combination` walks them).
fn combinations(n: usize, k: usize) -> Vec<Vec<usize>> {
    let mut result = Vec::new();
    let mut current: Vec<usize> = (0..k).collect();
    if k == 0 || k > n {
        return result;
    }
    loop {
        result.push(current.clone());
        let Some(i) = (0..k).rev().find(|&i| current[i] < n - k + i) else { return result };
        current[i] += 1;
        for j in i + 1..k {
            current[j] = current[j - 1] + 1;
        }
    }
}

fn fix_static_segment_rules(rules: &Timeline<ShapeRule>, animate: &dyn Fn(&Timeline<ShapeRule>) -> Timeline<Shape>) -> Timeline<ShapeRule> {
    // The cost grows exponentially with the number of replacements.
    const MAX_REPLACEMENTS: usize = 3;
    let possible: Vec<Cs> = rules.iter().filter(|t| can_change(&t.value)).map(|t| t.start()).collect();

    // Start with one replacement, then more as needed.
    let mut best = Scenario::new(rules, &[], animate);
    let mut count = 1;
    while best.static_segments > 0 && count <= possible.len().min(MAX_REPLACEMENTS) {
        for combination in combinations(possible.len(), count) {
            let changes: Vec<Cs> = combination.iter().map(|&i| possible[i]).collect();
            let scenario = Scenario::new(rules, &changes, animate);
            if scenario.is_better_than(&best) {
                best = scenario;
            }
        }
        count += 1;
    }
    best.rules
}

/// A rule that may give different shapes depending on context.
fn is_flexible(rule: &ShapeRule) -> bool {
    rule.shape_set.len() > 1
}

/// `range` extended until it starts and ends with a rule that is not flexible, if possible.
fn extend_to_fixed_rules(range: TimeRange, rules: &Timeline<ShapeRule>) -> TimeRange {
    let elements = rules.elements();
    let mut first = rules.find(range.start, FindMode::SampleRight).expect("rule at segment start");
    while first > 0 && is_flexible(&elements[first].value) {
        first -= 1;
    }
    let mut last = rules.find(range.end, FindMode::SampleLeft).expect("rule at segment end");
    while last + 1 < elements.len() && is_flexible(&elements[last].value) {
        last += 1;
    }
    TimeRange::new(elements[first].start(), elements[last].end())
}

fn avoid_static_segments(rules: &Timeline<ShapeRule>, animate: &dyn Fn(&Timeline<ShapeRule>) -> Timeline<Shape>) -> Timeline<Shape> {
    let animation = animate(rules);
    let segments = static_segments(rules, &animation);
    if segments.is_empty() {
        return animation;
    }

    // Change shape rules to break up the static segments.
    let mut fixed = rules.clone();
    for segment in segments {
        // Extend the range so that adjacent rules that influence the animation stay in.
        let extended = extend_to_fixed_rules(segment, rules);
        let segment_rules = Timeline::continuous(extended, ShapeRule::invalid(), false).with(fixed.iter().cloned());
        for timed in fix_static_segment_rules(&segment_rules, animate).iter() {
            fixed.set(timed.clone());
        }
    }
    animate(&fixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phones(list: &[(Cs, Cs, Phone)]) -> Timeline<Phone> {
        let end = list.last().map_or(0, |p| p.1 + 50);
        Timeline::bounded(TimeRange::new(0, end), false).with(list.iter().map(|&(s, e, p)| Timed::new(s, e, p)))
    }

    fn shapes(animation: &Timeline<Shape>) -> Vec<Shape> {
        animation.iter().map(|t| t.value).collect()
    }

    #[test]
    fn closes_the_lips_before_a_plosive() {
        // "a pa": the lips close (A) before the p, for half the previous phone (10 cs), and open for the vowel.
        let animation = animate(&phones(&[(10, 30, Phone::AA), (30, 40, Phone::P), (40, 70, Phone::AA)]), ShapeSet::all());
        let at = |t: Cs| animation.get(t).unwrap().value;
        assert_eq!(at(15), Shape::D, "{:?}", shapes(&animation));
        assert_eq!(at(25), Shape::A);
        assert_eq!(at(55), Shape::D);
        assert_eq!(*shapes(&animation).last().unwrap(), Shape::X);
    }

    #[test]
    fn rounds_the_lips_for_oo() {
        let animation = animate(&phones(&[(10, 40, Phone::UW)]), ShapeSet::all());
        assert_eq!(animation.get(25).unwrap().value, Shape::F);
    }

    #[test]
    fn combinations_are_lexicographic() {
        assert_eq!(combinations(4, 2), [[0, 1], [0, 2], [0, 3], [1, 2], [1, 3], [2, 3]]);
        assert!(combinations(2, 3).is_empty());
    }
}
