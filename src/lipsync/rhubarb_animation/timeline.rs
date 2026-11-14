//! Timelines of values, after Rhubarb's `time/` classes: `Timeline`, `BoundedTimeline`, `ContinuousTimeline` and their joining variants are one type here, configured by [`Timeline::bounded`] and [`Timeline::continuous`].

/// Time in centiseconds, Rhubarb's time unit.
pub type Cs = i32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeRange {
    pub start: Cs,
    pub end: Cs,
}

impl TimeRange {
    pub fn new(start: Cs, end: Cs) -> Self {
        debug_assert!(start <= end, "time range {start}..{end}");
        Self { start, end }
    }

    pub fn duration(self) -> Cs {
        self.end - self.start
    }

    pub fn middle(self) -> Cs {
        (self.start + self.end) / 2
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub fn set_start_if_earlier(&mut self, start: Cs) {
        self.start = self.start.min(start);
    }

    pub fn set_end_if_later(&mut self, end: Cs) {
        self.end = self.end.max(end);
    }

    pub fn trim_right(&mut self, value: Cs) {
        *self = TimeRange::new(self.start, self.end.min(value));
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Timed<T> {
    pub range: TimeRange,
    pub value: T,
}

impl<T> Timed<T> {
    pub fn new(start: Cs, end: Cs, value: T) -> Self {
        Self { range: TimeRange::new(start, end), value }
    }

    pub fn start(&self) -> Cs {
        self.range.start
    }

    pub fn end(&self) -> Cs {
        self.range.end
    }

    pub fn duration(&self) -> Cs {
        self.range.duration()
    }
}

/// How [`Timeline::find`] picks an element.
#[derive(Clone, Copy)]
pub enum FindMode {
    SampleLeft,
    SampleRight,
    SearchLeft,
    SearchRight,
}

/// Non-overlapping timed values, sorted by start.
///
/// - bounded: values are clipped to `range`;
/// - continuous: bounded, and cleared parts take `default`, so the range stays covered;
/// - joining: adjacent equal values merge when set.
#[derive(Clone, Debug, PartialEq)]
pub struct Timeline<T> {
    range: Option<TimeRange>,
    default: Option<T>,
    join: bool,
    elements: Vec<Timed<T>>,
}

impl<T: Clone + PartialEq> Timeline<T> {
    /// A timeline limited to `range`.
    pub fn bounded(range: TimeRange, join: bool) -> Self {
        Self { range: Some(range), default: None, join, elements: Vec::new() }
    }

    /// A timeline covering `range`, filled with `default`.
    pub fn continuous(range: TimeRange, default: T, join: bool) -> Self {
        let mut timeline = Self { range: Some(range), default: Some(default), join, elements: Vec::new() };
        timeline.clear(range);
        timeline
    }

    /// `self` with `elements` set one after another.
    pub fn with(mut self, elements: impl IntoIterator<Item = Timed<T>>) -> Self {
        for element in elements {
            self.set(element);
        }
        self
    }

    pub fn range(&self) -> TimeRange {
        self.range.unwrap_or_else(|| match (self.elements.first(), self.elements.last()) {
            (Some(first), Some(last)) => TimeRange::new(first.start(), last.end()),
            _ => TimeRange::new(0, 0),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    pub fn elements(&self) -> &[Timed<T>] {
        &self.elements
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Timed<T>> {
        self.elements.iter()
    }

    /// Index of the element found at `time`.
    pub fn find(&self, time: Cs, mode: FindMode) -> Option<usize> {
        match mode {
            FindMode::SampleLeft => self.find(time, FindMode::SearchLeft).filter(|&i| self.elements[i].end() >= time),
            FindMode::SampleRight => self.find(time, FindMode::SearchRight).filter(|&i| self.elements[i].start() <= time),
            // The last element starting before `time`.
            FindMode::SearchLeft => self.elements.partition_point(|e| e.start() < time).checked_sub(1),
            // The element containing `time`, or else the first one starting after it.
            FindMode::SearchRight => {
                let after = self.elements.partition_point(|e| e.start() <= time);
                if after > 0 && self.elements[after - 1].end() > time {
                    Some(after - 1)
                } else {
                    (after < self.elements.len()).then_some(after)
                }
            }
        }
    }

    /// The element containing `time`.
    pub fn get(&self, time: Cs) -> Option<&Timed<T>> {
        self.find(time, FindMode::SampleRight).map(|i| &self.elements[i])
    }

    /// Removes the values in `range`; a continuous timeline fills it with its default.
    pub fn clear(&mut self, range: TimeRange) {
        match self.default.clone() {
            Some(default) => {
                self.set(Timed { range, value: default });
            }
            None => self.erase(range),
        }
    }

    pub fn set(&mut self, mut timed: Timed<T>) {
        if let Some(bounds) = self.range {
            if timed.end() <= bounds.start || timed.start() >= bounds.end {
                return;
            }
            timed.range = TimeRange::new(timed.start().max(bounds.start), timed.end().min(bounds.end));
        }
        if timed.range.is_empty() {
            return;
        }
        if self.join {
            if let Some(i) = self.find(timed.start(), FindMode::SampleLeft)
                && self.elements[i].value == timed.value
            {
                timed.range = TimeRange::new(self.elements[i].start(), timed.end());
            }
            if let Some(i) = self.find(timed.end(), FindMode::SampleRight)
                && self.elements[i].value == timed.value
            {
                timed.range = TimeRange::new(timed.start(), self.elements[i].end());
            }
        }
        self.erase(timed.range);
        let at = self.elements.partition_point(|e| e.start() < timed.start());
        self.elements.insert(at, timed);
    }

    pub fn set_value(&mut self, start: Cs, end: Cs, value: T) {
        self.set(Timed::new(start, end, value));
    }

    /// Removes everything in `range`, splitting elements that stick out.
    fn erase(&mut self, range: TimeRange) {
        if range.is_empty() {
            return;
        }
        self.split_at(range.start);
        self.split_at(range.end);
        self.elements.retain(|e| e.start() < range.start || e.start() >= range.end);
    }

    fn split_at(&mut self, time: Cs) {
        let (Some(before), Some(after)) = (self.find(time - 1, FindMode::SampleRight), self.find(time, FindMode::SampleRight)) else { return };
        if before != after {
            return;
        }
        let element = self.elements[before].clone();
        self.elements[before].range = TimeRange::new(element.start(), time);
        self.elements.insert(before + 1, Timed::new(time, element.end(), element.value));
    }
}

/// Calls `f` on runs of `n` neighbouring elements exactly as Rhubarb's `for_each_adjacent` (tools.h) does, including its quirks: the second run repeats element `n - 1`, and the last run that would end with the last element is never visited. Rhubarb's pause and tween steps rely on this order, so their results match the original.
pub fn for_each_adjacent<T>(elements: &[T], n: usize, mut f: impl FnMut(&[&T])) {
    if elements.len() < n {
        return;
    }
    let mut window: std::collections::VecDeque<&T> = elements[..n].iter().collect();
    for element in &elements[n - 1..] {
        let run: Vec<&T> = window.iter().copied().collect();
        f(&run);
        window.pop_front();
        window.push_back(element);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_splits_and_joins() {
        let mut timeline = Timeline::continuous(TimeRange::new(0, 10), 'x', true);
        timeline.set_value(2, 4, 'a');
        timeline.set_value(4, 6, 'a');
        let parts: Vec<_> = timeline.iter().map(|e| (e.start(), e.end(), e.value)).collect();
        assert_eq!(parts, [(0, 2, 'x'), (2, 6, 'a'), (6, 10, 'x')]);
    }

    #[test]
    fn bounded_clips() {
        let mut timeline = Timeline::bounded(TimeRange::new(0, 10), false);
        timeline.set_value(-5, 3, 1);
        timeline.set_value(8, 20, 2);
        let parts: Vec<_> = timeline.iter().map(|e| (e.start(), e.end(), e.value)).collect();
        assert_eq!(parts, [(0, 3, 1), (8, 10, 2)]);
        assert_eq!(timeline.get(9).map(|e| e.value), Some(2));
        assert_eq!(timeline.get(5), None);
    }

    #[test]
    fn adjacent_runs_follow_rhubarb() {
        let mut runs = Vec::new();
        for_each_adjacent(&[0, 1, 2, 3], 2, |run| runs.push((*run[0], *run[1])));
        assert_eq!(runs, [(0, 1), (1, 1), (1, 2)]);
    }
}
