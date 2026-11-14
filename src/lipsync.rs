//! Lip sync v1: the loudness and a coarse spectrum of the playing audio choose the mouth shape, in any language.
//!
//! The audio thread cuts the signal into 20 ms windows every 10 ms and measures per window:
//! - `level`: RMS, full scale = 1; sets how far the mouth opens.
//! - `f1`: power-weighted mean frequency in 200–1000 Hz, a stand-in for the first formant: low for "oo" and "ee", high for "aa".
//! - `front`: power in 1.8–3.5 kHz over power in 1.0–1.8 kHz plus 1 % of the power in 200–1000 Hz, in dB: high for "ee" and "ih", whose second formant lies above 1.8 kHz, low for "oo", "oh" and "aa". The 1 % share keeps a sound with almost nothing above 1 kHz ("oo") on the low side.
//!
//! The thresholds in [`visemes`] come from the medians of these measures over test words spoken by Qwen3-TTS (voice "serena").

use std::collections::VecDeque;
use std::f32::consts::PI;

/// Sound measures of one window.
#[derive(Clone, Copy, Debug, Default)]
pub struct Measure {
    pub level: f32,
    pub f1: f32,
    pub front: f32,
}

const WINDOW_SECONDS: f32 = 0.02;
/// Time between two measures.
pub const HOP_SECONDS: f32 = 0.01;
/// Spacing of the analysed frequencies in Hz.
const BIN_SPACING: f32 = 50.0;
/// Windows quieter than this RMS skip the spectrum.
const QUIET_LEVEL: f32 = 1e-3;

/// Measures a mono signal sample by sample.
pub struct Analyzer {
    rate: f32,
    window: VecDeque<f32>,
    size: usize,
    hop: usize,
    since_hop: usize,
    hann: Vec<f32>,
    /// Analysed frequencies (200 Hz to 3.5 kHz) with their Goertzel coefficients.
    bins: Vec<(f32, f32)>,
}

impl Analyzer {
    pub fn new(rate: u32) -> Self {
        let rate = rate as f32;
        let size = (rate * WINDOW_SECONDS) as usize;
        let hann = (0..size).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / size as f32).cos()).collect();
        let bins = (4..=70).map(|k| k as f32 * BIN_SPACING).map(|f| (f, 2.0 * (2.0 * PI * f / rate).cos())).collect();
        Self {
            rate,
            window: VecDeque::with_capacity(size),
            size,
            hop: (rate * HOP_SECONDS) as usize,
            since_hop: 0,
            hann,
            bins,
        }
    }

    /// Length of a window in seconds.
    pub fn window_seconds(&self) -> f32 {
        self.size as f32 / self.rate
    }

    /// Adds one sample; returns the measures of the window that ends with it, once per hop.
    pub fn push(&mut self, sample: f32) -> Option<Measure> {
        if self.window.len() == self.size {
            self.window.pop_front();
        }
        self.window.push_back(sample);
        self.since_hop += 1;
        if self.since_hop < self.hop || self.window.len() < self.size {
            return None;
        }
        self.since_hop = 0;
        Some(self.measure())
    }

    fn measure(&self) -> Measure {
        let level = (self.window.iter().map(|x| x * x).sum::<f32>() / self.size as f32).sqrt();
        if level < QUIET_LEVEL {
            return Measure { level, ..Measure::default() };
        }

        let (mut low, mut low_weighted, mut mid, mut high) = (0.0, 0.0, 0.0, 0.0);
        for &(freq, coeff) in &self.bins {
            let power = self.goertzel(coeff);
            match freq {
                f if f < 1000.0 => {
                    low += power;
                    low_weighted += power * f;
                }
                f if f < 1800.0 => mid += power,
                _ => high += power,
            }
        }

        Measure {
            level,
            f1: low_weighted / low.max(1e-12),
            front: 10.0 * ((high + 1e-12) / (mid + 0.01 * low + 1e-12)).log10(),
        }
    }

    /// Power of the windowed signal at one frequency.
    fn goertzel(&self, coeff: f32) -> f32 {
        let (mut s1, mut s2) = (0.0f32, 0.0f32);
        for (x, w) in self.window.iter().zip(&self.hann) {
            let s = x * w + coeff * s1 - s2;
            s2 = s1;
            s1 = s;
        }
        s1 * s1 + s2 * s2 - coeff * s1 * s2
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Viseme weights in the order of `ExpressionPreset::VISEMES` (aa, ih, ou, ee, oh): the [`shape`] scaled by the [`opening`].
pub fn visemes(m: &Measure, silence: f32, loud: f32) -> [f32; 5] {
    let open = opening(m, silence, loud);
    shape(m).map(|w| w * open)
}

/// How far the mouth opens (0–1): the level between `silence` and `loud`, on a dB scale, reduced for nasal hums.
///
/// "m" and "n" are voiced through the nose with the mouth (nearly) closed; their sound has almost all power at the voice's fundamental (`f1` near the 200 Hz band edge) and very little above 1.8 kHz. Hiss ("s", "th", "f"), with `front` far above any vowel's, comes from a narrow gap at the teeth and also keeps the jaw nearly closed.
pub fn opening(m: &Measure, silence: f32, loud: f32) -> f32 {
    let db = |x: f32| 20.0 * x.max(1e-6).log10();
    let open = ((db(m.level) - db(silence)) / (db(loud) - db(silence))).clamp(0.0, 1.0);
    let nasal = (1.0 - smoothstep(235.0, 270.0, m.f1)) * (1.0 - smoothstep(-18.0, -12.0, m.front));
    open * (1.0 - 0.85 * nasal) * (1.0 - 0.6 * hiss(m))
}

/// How much the sound is hiss rather than voice (0–1): vowels reach at most about +5 dB in `front`, "s" and "th" +10 dB and more.
fn hiss(m: &Measure) -> f32 {
    smoothstep(6.0, 12.0, m.front)
}

/// Which mouth shape the sound calls for, as viseme weights summing to 1: `front` splits front vowels (ee, ih) from back ones (ou, oh, aa), and `f1` orders each group from closed to open.
pub fn shape(m: &Measure) -> [f32; 5] {
    // Measured medians of f1: ee 330, ih 500 Hz; ou 375, oh 550, aa 640 Hz.
    // Hiss has no formants; its f1 is noise, so it takes the narrow "ee" shape.
    let front_height = smoothstep(350.0, 480.0, m.f1) * (1.0 - hiss(m));
    let height = smoothstep(350.0, 650.0, m.f1);
    // Measured medians of front: ee +2, ih +1.5 dB; aa −7, ou −22, oh −33 dB. Unstressed "ee" at a word's end ("Riggy") drops to about −5 dB, so for closed vowels, where the choice is ee or ou, the split lies lower than for open ones, where it is ih or aa.
    let split = -12.0 + 9.0 * height;
    let front = smoothstep(split - 4.0, split + 4.0, m.front);
    let ou = (1.0 - height / 0.4).clamp(0.0, 1.0);
    let aa = ((height - 0.7) / 0.3).clamp(0.0, 1.0);
    let oh = 1.0 - ou - aa;
    let back = 1.0 - front;
    [aa * back, front_height * front, ou * back, (1.0 - front_height) * front, oh * back]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sum of sines at the given frequencies and amplitudes, measured over one window.
    fn measure(rate: u32, tones: &[(f32, f32)]) -> Measure {
        let mut analyzer = Analyzer::new(rate);
        let mut last = None;
        for i in 0..rate as usize / 10 {
            let t = i as f32 / rate as f32;
            let x = tones.iter().map(|&(f, a)| a * (2.0 * PI * f * t).sin()).sum();
            last = analyzer.push(x).or(last);
        }
        last.unwrap()
    }

    #[test]
    fn level_is_rms() {
        let m = measure(48_000, &[(500.0, 0.1)]);
        assert!((m.level - 0.1 / 2f32.sqrt()).abs() < 0.005, "{m:?}");
    }

    #[test]
    fn vowels_pick_their_viseme() {
        let largest = |w: [f32; 5]| (0..5).max_by(|&a, &b| w[a].total_cmp(&w[b])).unwrap();
        // Two-formant stand-ins: (F1, F2) with F2 about 16 dB weaker, roughly as in the TTS voice.
        let cases = [(700.0, 1200.0, 0), (450.0, 2400.0, 1), (300.0, 800.0, 2), (300.0, 2700.0, 3), (520.0, 900.0, 4)];
        for rate in [24_000, 48_000] {
            for (f1, f2, expected) in cases {
                let m = measure(rate, &[(f1, 0.1), (f2, 0.015)]);
                let w = visemes(&m, 0.01, 0.12);
                assert_eq!(largest(w), expected, "rate {rate}, F1 {f1}, F2 {f2}: {m:?} → {w:?}");
            }
        }
    }

    #[test]
    fn hums_keep_the_mouth_nearly_closed() {
        // "m": power at the fundamental only.
        let hum = measure(48_000, &[(220.0, 0.1)]);
        let vowel = measure(48_000, &[(300.0, 0.1), (2700.0, 0.015)]);
        assert!(opening(&hum, 0.01, 0.12) < 0.25 * opening(&vowel, 0.01, 0.12), "{hum:?} vs {vowel:?}");
    }

    #[test]
    fn hiss_keeps_the_jaw_low() {
        let hiss = Measure { level: 0.05, f1: 600.0, front: 15.0 };
        let vowel = Measure { level: 0.05, f1: 600.0, front: -10.0 };
        assert!(opening(&hiss, 0.01, 0.12) < 0.5 * opening(&vowel, 0.01, 0.12));
        assert!(shape(&hiss)[3] > 0.9, "{:?}", shape(&hiss));
    }

    #[test]
    fn silence_closes_the_mouth() {
        assert_eq!(visemes(&measure(48_000, &[(500.0, 0.005)]), 0.01, 0.12), [0.0; 5]);
    }
}
