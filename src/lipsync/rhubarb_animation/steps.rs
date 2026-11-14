//! Rhubarb's animation steps: `roughAnimation.cpp`, `timingOptimization.cpp`, `pauseAnimation.cpp` and `tweening.cpp`.

use std::collections::BTreeMap;

use super::rules::{ShapeRule, TweenTiming, closest_shape, relax, tween};
use super::shape::Shape;
use super::timeline::{Cs, TimeRange, Timed, Timeline, for_each_adjacent};

/// Shapes from shape rules, by a bidirectional pass:
///
/// - Most consonants allow several shapes, most vowels one.
/// - Speakers slur mouth shapes into each other, so going forwards, each rule takes the allowed shape that resembles the last one and is somewhat relaxed.
/// - Speakers anticipate vowels, forming their shape before the vowel, so at each one-shape vowel the pass goes back a little, spreading that shape to the left.
pub fn animate_rough(rules: &Timeline<ShapeRule>) -> Timeline<Shape> {
    const MAX_ANTICIPATION: Cs = 20;
    let rules = rules.elements();
    let mut animation = Timeline::continuous(range_of(rules), Shape::X, true);

    let mut reference = Shape::X;
    let mut last_anticipated_start: Cs = -1;
    for (i, timed) in rules.iter().enumerate() {
        let rule = timed.value;
        let shape = closest_shape(reference, rule.shape_set);
        animation.set(Timed { range: timed.range, value: shape });
        let anticipate = rule.phone.is_some_and(|p| p.is_vowel()) && rule.shape_set.len() == 1;
        if anticipate {
            // Animate backwards a little.
            let anticipated_start = timed.start();
            reference = shape;
            for earlier in rules[..i].iter().rev() {
                // Not too far back.
                if earlier.start() == last_anticipated_start || anticipated_start - earlier.start() > MAX_ANTICIPATION {
                    break;
                }
                // Overwrite the forward-animated shape with one anticipating the vowel.
                let anticipating = closest_shape(reference, earlier.value.shape_set);
                animation.set(Timed { range: earlier.range, value: anticipating });
                // Stop once the new shape no longer resembles the vowel's.
                if super::rules::basic_shape(anticipating) != super::rules::basic_shape(shape) {
                    break;
                }
                reference = anticipating;
            }
            last_anticipated_start = anticipated_start;
        }
        reference = if anticipate { shape } else { relax(shape) };
    }
    animation
}

fn range_of<T>(elements: &[Timed<T>]) -> TimeRange {
    match (elements.first(), elements.last()) {
        (Some(first), Some(last)) => TimeRange::new(first.start(), last.end()),
        _ => TimeRange::new(0, 0),
    }
}

/// The shape that best represents `shapes`: the one shown longest, with D preferred over the similar C.
fn representative_shape(shapes: &[Timed<Shape>]) -> Shape {
    assert!(!shapes.is_empty(), "cannot determine a representative shape from an empty timeline");
    let mut weights: BTreeMap<Shape, Cs> = BTreeMap::new();
    for timed in shapes {
        *weights.entry(timed.value).or_default() += timed.duration();
    }
    // The first shape (in A..X order) with the highest total duration, as std::max_element picks it.
    let mut best = (Shape::X, Cs::MIN);
    for (&shape, &weight) in &weights {
        if weight > best.1 {
            best = (shape, weight);
        }
    }
    let substitute_d = best.0 == Shape::C && weights.get(&Shape::D).is_some_and(|&w| w > 0);
    if substitute_d { Shape::D } else { best.0 }
}

/// Source shapes in a candidate range, reduced to one shape.
struct ShapeReduction {
    /// The source shapes clipped to the candidate range; their extent is what Rhubarb's sliced `JoiningTimeline` copy reports as its range.
    source_shapes: Vec<Timed<Shape>>,
    shape: Shape,
}

impl ShapeReduction {
    fn new(source: &Timeline<Shape>, candidate: TimeRange) -> Self {
        let clipped = Timeline::bounded(candidate, true).with(source.iter().cloned());
        let source_shapes = clipped.elements().to_vec();
        let shape = representative_shape(&source_shapes);
        Self { source_shapes, shape }
    }

    fn range(&self) -> TimeRange {
        range_of(&self.source_shapes)
    }
}

/// The time range of candidate shapes for the next shape to draw; never empty.
fn next_minimal_candidate_range(source: &Timeline<Shape>, target: TimeRange, write_position: Cs) -> TimeRange {
    assert!(!source.is_empty(), "cannot determine a candidate range for an empty source timeline");
    // Too short gives flickering, too long loses too many shapes; 5–7 cs work well.
    const MIN_SHAPE_DURATION: Cs = 7;

    // If the remaining time can hold more than one shape but not two, split it evenly.
    let remaining = write_position - target.start;
    let fits_one_or_less = remaining <= MIN_SHAPE_DURATION;
    let fits_two = remaining >= 2 * MIN_SHAPE_DURATION;
    let duration = if fits_one_or_less || fits_two { MIN_SHAPE_DURATION } else { remaining / 2 };

    let source_range = source.range();
    let mut candidate = TimeRange::new(write_position - duration, write_position);
    if write_position == target.end {
        // First iteration: include all source shapes after the target range.
        candidate.set_end_if_later(source_range.end);
    }
    if candidate.start >= source_range.end {
        // Not yet at the source range: reach back to the right-most source shape.
        candidate = TimeRange::new(source.elements().last().unwrap().start(), candidate.end);
    }
    if candidate.end <= source_range.start {
        // Past the source range (corner cases): reach forward to the left-most source shape.
        candidate = TimeRange::new(candidate.start, source.elements()[0].end());
    }
    candidate
}

fn next_shape_reduction(source: &Timeline<Shape>, target: TimeRange, write_position: Cs) -> ShapeReduction {
    // The shortest possible candidate range...
    let min = ShapeReduction::new(source, next_minimal_candidate_range(source, target, write_position));
    // ...one extended to the left to fully contain its left-most shape...
    let extended = ShapeReduction::new(source, TimeRange::new(min.source_shapes[0].start(), min.range().end));
    // ...and the shape that might come next if the shortest range is taken now.
    let next = ShapeReduction::new(source, next_minimal_candidate_range(source, target, min.range().start));

    let min_equals_extended = min.shape == extended.shape;
    let extended_is_special = extended.shape != min.shape && extended.shape != next.shape;
    if min_equals_extended || extended_is_special { extended } else { min }
}

/// The shapes of `animation` within `source_range`, retimed to fit `target` without jitter.
fn retime(animation: &Timeline<Shape>, source_range: TimeRange, target: TimeRange) -> Timeline<Shape> {
    let source = Timeline::continuous(source_range, Shape::X, true).with(animation.iter().cloned());
    let mut result = Timeline::continuous(target, Shape::X, true);
    if source.is_empty() {
        return result;
    }

    // Animate backwards.
    let mut write_position = target.end;
    while write_position > target.start {
        // Decide which shape to show next, possibly dropping short shapes.
        let reduction = next_shape_reduction(&source, target, write_position);

        // How long to show it.
        let mut shape_range = reduction.range();
        if shape_range.start <= source.range().start {
            // The left-most source shape is used up: fill the rest of the target range.
            shape_range.set_start_if_earlier(target.start);
        }
        shape_range.trim_right(write_position);

        result.set(Timed { range: shape_range, value: reduction.shape });
        write_position = shape_range.start;
    }
    result
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MouthState {
    Idle,
    Closed,
    Open,
}

/// Makes every run of open or closed shapes long enough to register visually.
pub fn optimize_timing(animation: &Timeline<Shape>) -> Timeline<Shape> {
    // The minimum duration of an open or closed segment, and how far its start may move forward.
    const MIN_SEGMENT_DURATION: Cs = 8;
    const MAX_EXTENSION_DURATION: Cs = 6;

    let mut segments = Timeline::continuous(animation.range(), MouthState::Idle, true);
    for timed in animation.iter() {
        let state = match timed.value {
            Shape::X => MouthState::Idle,
            Shape::A => MouthState::Closed,
            _ => MouthState::Open,
        };
        segments.set(Timed { range: timed.range, value: state });
    }

    // Filled from right to left; `result_start` is the earliest time already written.
    let mut result = Timeline::continuous(animation.range(), Shape::X, true);
    let mut result_start = result.range().end;
    let reversed: Vec<&Timed<MouthState>> = segments.iter().rev().collect();
    let mut i = 0;
    while i < reversed.len() {
        let segment = reversed[i];
        if segment.value == MouthState::Idle {
            i += 1;
            continue;
        }

        result_start = segment.end().min(result_start);
        if result_start - segment.start() >= MIN_SEGMENT_DURATION {
            // Long enough: no need to extend it to the left.
            let target = TimeRange::new(segment.start(), result_start);
            for timed in retime(animation, segment.range, target).iter() {
                result.set(timed.clone());
            }
            result_start = target.start;
            i += 1;
        } else {
            // Too short: take all adjacent short segments to the left and distribute them evenly.
            let begin = i;
            let mut end = begin + 1;
            while end < reversed.len() && reversed[end].value != MouthState::Idle && reversed[end].duration() < MIN_SEGMENT_DURATION {
                end += 1;
            }

            // How far to extend the short segments to the left.
            let short_count = (end - begin) as Cs;
            let desired_duration = MIN_SEGMENT_DURATION * short_count;
            let current_duration = reversed[begin].end() - reversed[end - 1].start();
            let desired_extension = desired_duration - current_duration;
            let available_extension = if end < reversed.len() { reversed[end].duration() - 1 } else { 0 };
            let extension = desired_extension.min(available_extension).min(MAX_EXTENSION_DURATION);

            // Distribute the time evenly among the short segments.
            let targets_start = reversed[end - 1].start() - extension;
            for (k, short) in reversed[begin..end].iter().enumerate() {
                let remaining = (end - begin - k) as Cs;
                let duration = (result_start - targets_start) / remaining;
                let target = TimeRange::new(result_start - duration, result_start);
                for timed in retime(animation, short.range, target).iter() {
                    result.set(timed.clone());
                }
                result_start = target.start;
            }
            i = end;
        }
    }
    result
}

/// The shape for a pause of `duration` between `previous` and `next`.
fn pause_shape(previous: Shape, next: Shape, duration: Cs) -> Shape {
    // Very short pauses hold the previous shape.
    if duration < 12 {
        return previous;
    }
    // Short pauses relax the mouth, to a shape other than the next one, which would look odd.
    if duration <= 35 {
        let mut current = previous;
        loop {
            let relaxed = relax(current);
            if relaxed != next {
                return relaxed;
            }
            if relaxed == current {
                // Going in circles.
                break;
            }
            current = relaxed;
        }
    }
    // Longer pauses close the mouth.
    Shape::X
}

pub fn animate_pauses(animation: &Timeline<Shape>) -> Timeline<Shape> {
    let mut result = animation.clone();
    for_each_adjacent(animation.elements(), 3, |run| {
        let (previous, pause, next) = (run[0], run[1], run[2]);
        if pause.value == Shape::X {
            result.set(Timed { range: pause.range, value: pause_shape(previous.value, next.value, pause.duration()) });
        }
    });
    result
}

pub fn insert_tweens(animation: &Timeline<Shape>) -> Timeline<Shape> {
    const MIN_TWEEN_DURATION: Cs = 4;
    const MAX_TWEEN_DURATION: Cs = 8;

    let mut result = animation.clone();
    for_each_adjacent(animation.elements(), 2, |run| {
        let (first, second) = (run[0], run[1]);
        let Some((shape, timing)) = tween(first.value, second.value) else { return };
        let (start, duration) = match timing {
            TweenTiming::Early => {
                let duration = (first.duration() / 3).min(MAX_TWEEN_DURATION);
                (first.end() - duration, duration)
            }
            TweenTiming::Centered => {
                let duration = (first.duration() / 4).min(second.duration() / 4).min(MAX_TWEEN_DURATION);
                (first.end() - duration / 2, duration)
            }
            TweenTiming::Late => {
                let duration = (second.duration() / 3).min(MAX_TWEEN_DURATION);
                (second.start(), duration)
            }
        };
        if duration >= MIN_TWEEN_DURATION {
            result.set_value(start, start + duration, shape);
        }
    });
    result
}
