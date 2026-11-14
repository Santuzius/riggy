//! Alignment of the real speech to the synthetic one by dynamic time warping (DTW), after the approach of the forced aligner aeneas (https://github.com/readbeyond/aeneas/blob/master/wiki/HOWITWORKS.md): both are turned into MFCC frames, and DTW finds the monotonic pairing of frames with the least total difference.
//!
//! The real speech arrives while it plays, so the aligner grows by one column per real frame and can report the best path so far ("open end"); its latest part is still uncertain, so callers trust it only some frames behind the newest.

use super::features::{self, COEFFICIENTS, Frame};

/// Spectral cost when either frame is silent, where the coefficients only describe noise; the RMS difference of normalised coefficients is about 1.2 between frames of different sounds.
const SILENT_SPECTRUM: f32 = 1.0;
/// Levels are compared relative to each speech's peak, in steps of 10 dB, down to this many steps below it.
const LEVEL_RANGE: f32 = 4.0;
/// Weight of one 10 dB step of level difference. The level separates pauses and the closures of stops (quiet in both voices, though eSpeak NG's closures are not silent) from vowels.
const LEVEL_WEIGHT: f32 = 0.5;
/// The real speech's peak level assumed before louder frames arrive, in dB relative to full scale.
const INITIAL_PEAK_DB: f32 = -20.0;

/// Weight of the synthetic voice's statistics in the real voice's normalisation, in frames (0.5 s of speech).
const PRIOR_FRAMES: f32 = 50.0;

/// How the path reached a pair (reference frame i, real frame j). The diagonal steps keep the path's slope between 1/2 and 2 (Sakoe and Chiba's slope constraint P = 1), so it cannot pile many phonemes onto one real frame or stretch one phoneme over many; pure vertical and horizontal steps are free only over silence, where one speaker pauses longer than the other.
#[derive(Clone, Copy, Debug)]
enum Step {
    Start,
    /// From (i-1, j-1).
    Diagonal,
    /// From (i-2, j-1) via (i-1, j).
    UpDiagonal,
    /// From (i-1, j-2) via (i, j-1).
    LeftDiagonal,
    /// From (i-1, j), over a silent reference frame.
    Up,
    /// From (i, j-1), over a silent real frame.
    Left,
}

/// One real frame's row of the DTW matrix.
struct Column {
    /// Accumulated cost of the best path to each reference frame.
    totals: Vec<f32>,
    /// Cost of pairing each reference frame with this real frame.
    costs: Vec<f32>,
}

/// Mean and spread of the coefficients over voiced frames, to compare two voices on equal terms (cepstral mean and variance normalisation).
#[derive(Clone, Default)]
struct Normaliser {
    count: f32,
    mean: [f32; COEFFICIENTS],
    m2: [f32; COEFFICIENTS],
}

impl Normaliser {
    fn add(&mut self, frame: &Frame) {
        if !frame.voiced {
            return;
        }
        self.count += 1.0;
        for k in 0..COEFFICIENTS {
            let delta = frame.mfcc[k] - self.mean[k];
            self.mean[k] += delta / self.count;
            self.m2[k] += delta * (frame.mfcc[k] - self.mean[k]);
        }
    }

    /// The same statistics, weighted as if they came from at most `count` frames: a starting point that new frames soon outweigh.
    fn as_prior(&self, count: f32) -> Self {
        let scale = if self.count > count { count / self.count } else { 1.0 };
        Self { count: self.count * scale, mean: self.mean, m2: self.m2.map(|m| m * scale) }
    }

    fn apply(&self, frame: &Frame) -> Frame {
        let mut out = *frame;
        for k in 0..COEFFICIENTS {
            let spread = if self.count > 1.0 { (self.m2[k] / (self.count - 1.0)).sqrt() } else { 1.0 };
            out.mfcc[k] = (frame.mfcc[k] - self.mean[k]) / spread.max(1e-3);
        }
        out
    }
}

/// Frame levels in steps of 10 dB relative to `peak`, between `-LEVEL_RANGE` and 0.
fn relative_level(level: f32, peak: f32) -> f32 {
    ((level - peak) / 10.0).clamp(-LEVEL_RANGE, 0.0)
}

/// Difference of two normalised frames, whose `level` is relative to their speech's peak.
fn cost(a: &Frame, b: &Frame) -> f32 {
    let spectrum = if a.voiced && b.voiced { (a.mfcc.iter().zip(&b.mfcc).map(|(x, y)| (x - y).powi(2)).sum::<f32>() / COEFFICIENTS as f32).sqrt() } else { SILENT_SPECTRUM };
    spectrum + LEVEL_WEIGHT * (a.level - b.level).abs()
}

pub struct Aligner {
    reference: Vec<Frame>,
    normaliser: Normaliser,
    /// Peak level of the real speech so far.
    real_peak: f32,
    /// The newest real frame's column and the one before.
    columns: [Option<Column>; 2],
    /// Per real frame and reference frame: the step that led there.
    steps: Vec<Vec<Step>>,
}

impl Aligner {
    pub fn new(reference: &[Frame]) -> Self {
        let mut normaliser = Normaliser::default();
        reference.iter().for_each(|f| normaliser.add(f));
        let peak = reference.iter().map(|f| f.level).fold(features::FLOOR_DB, f32::max);
        let reference = reference.iter().map(|f| Frame { level: relative_level(f.level, peak), ..normaliser.apply(f) }).collect();
        // The real voice starts with the synthetic voice's statistics, so its first frames are not compared against an empty mean.
        let real_normaliser = normaliser.as_prior(PRIOR_FRAMES);
        Self { reference, normaliser: real_normaliser, real_peak: INITIAL_PEAK_DB, columns: [None, None], steps: Vec::new() }
    }

    /// Real frames so far.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn push(&mut self, frame: Frame) {
        self.normaliser.add(&frame);
        self.real_peak = self.real_peak.max(frame.level);
        let frame = Frame { level: relative_level(frame.level, self.real_peak), ..self.normaliser.apply(&frame) };
        let n = self.reference.len();
        let costs: Vec<f32> = self.reference.iter().map(|r| cost(r, &frame)).collect();
        let mut totals = vec![f32::INFINITY; n];
        let mut steps = vec![Step::Start; n];
        let [previous, before] = &self.columns;
        for i in 0..n {
            let d = costs[i];
            // A diagonal pair counts its cost twice, as it covers a frame of each speech.
            let mut best = if previous.is_none() && i == 0 { (2.0 * d, Step::Start) } else { (f32::INFINITY, Step::Start) };
            let mut offer = |total: f32, step: Step| {
                if total < best.0 {
                    best = (total, step);
                }
            };
            if let Some(p) = previous {
                if i >= 1 {
                    offer(p.totals[i - 1] + 2.0 * d, Step::Diagonal);
                }
                if i >= 2 {
                    offer(p.totals[i - 2] + 2.0 * costs[i - 1] + d, Step::UpDiagonal);
                }
                if !frame.voiced {
                    offer(p.totals[i] + d, Step::Left);
                }
                if let Some(b) = before
                    && i >= 1
                {
                    offer(b.totals[i - 1] + 2.0 * p.costs[i] + d, Step::LeftDiagonal);
                }
            }
            if i >= 1 && !self.reference[i].voiced {
                offer(totals[i - 1] + d, Step::Up);
            }
            (totals[i], steps[i]) = best;
        }
        self.columns = [Some(Column { totals, costs }), self.columns[0].take()];
        self.steps.push(steps);
    }

    /// For each reference frame, the first real frame paired with it, on the best path to the newest real frame. With `closed`, the path ends at the last reference frame; otherwise wherever the cost per step is lowest. Reference frames past the path's end get `None`.
    pub fn first_real_frames(&self, closed: bool) -> Vec<Option<usize>> {
        let n = self.reference.len();
        let mut firsts = vec![None; n];
        let (Some(last), Some(column)) = (self.steps.len().checked_sub(1), &self.columns[0]) else { return firsts };
        let per_step = |i: usize| column.totals[i] / (i + last + 2) as f32;
        let mut i = if closed && column.totals[n - 1].is_finite() { n - 1 } else { (0..n).filter(|&i| column.totals[i].is_finite()).min_by(|&a, &b| per_step(a).total_cmp(&per_step(b))).unwrap_or(0) };
        let mut j = last;
        loop {
            firsts[i] = Some(j);
            match self.steps[j][i] {
                Step::Start => break,
                Step::Diagonal => (i, j) = (i - 1, j - 1),
                Step::UpDiagonal => {
                    firsts[i - 1] = Some(j);
                    (i, j) = (i - 2, j - 1);
                }
                Step::LeftDiagonal => {
                    firsts[i] = Some(j - 1);
                    (i, j) = (i - 1, j - 2);
                }
                Step::Up => i -= 1,
                Step::Left => j -= 1,
            }
        }
        firsts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(value: f32, voiced: bool) -> Frame {
        let mut mfcc = [0.0; COEFFICIENTS];
        mfcc[0] = value;
        mfcc[1] = -value;
        Frame { mfcc, voiced, level: if voiced { -20.0 } else { features::FLOOR_DB } }
    }

    #[test]
    fn stretches_a_slower_copy() {
        // Reference: silence, sounds 1, 2, 3, silence. The real speech holds each sound twice as long.
        let reference: Vec<Frame> = [(0.0, false), (1.0, true), (2.0, true), (3.0, true), (0.0, false)].iter().map(|&(v, s)| frame(v, s)).collect();
        let mut aligner = Aligner::new(&reference);
        for &(v, s) in &[(0.0, false), (0.0, false), (1.0, true), (1.0, true), (2.0, true), (2.0, true), (3.0, true), (3.0, true), (0.0, false)] {
            aligner.push(frame(v, s));
        }
        assert_eq!(aligner.first_real_frames(true), [Some(0), Some(2), Some(4), Some(6), Some(8)]);
    }

    #[test]
    fn open_end_stops_where_the_speech_is() {
        let reference: Vec<Frame> = (0..6).map(|v| frame(v as f32, true)).collect();
        let mut aligner = Aligner::new(&reference);
        for v in 0..3 {
            aligner.push(frame(v as f32, true));
        }
        let firsts = aligner.first_real_frames(false);
        assert_eq!(&firsts[..3], [Some(0), Some(1), Some(2)]);
        assert_eq!(firsts[4], None);
    }
}
