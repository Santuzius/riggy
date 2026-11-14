//! Sound features for alignment: mel-frequency cepstral coefficients (MFCCs) every 10 ms, which describe the spectral envelope of each slice of speech, its level, and whether it is voiced at all.
//!
//! The mel filters are defined in hertz, so speech at different sample rates (eSpeak NG's 22.05 kHz, the TTS's 24 kHz) gives comparable features.

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};

use crate::lipsync::QUIET_LEVEL;

/// One frame per centisecond, Rhubarb's time unit.
pub const HOP_SECONDS: f32 = 0.01;
pub const WINDOW_SECONDS: f32 = 0.025;
const MEL_BANDS: usize = 26;
const LOW_HZ: f32 = 100.0;
const HIGH_HZ: f32 = 7000.0;
/// Coefficients c1..c12; c0 (overall level) is left out, as the two voices differ in loudness.
pub const COEFFICIENTS: usize = 12;

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub mfcc: [f32; COEFFICIENTS],
    pub voiced: bool,
    /// RMS level in dB relative to full scale, at least `FLOOR_DB`.
    pub level: f32,
}

pub const FLOOR_DB: f32 = -80.0;

/// Turns a stream of samples into frames.
pub struct Extractor {
    hop: usize,
    window: Vec<f32>,
    fft: Arc<dyn RealToComplex<f32>>,
    /// Per mel band: first FFT bin and the triangle weights from there.
    filters: Vec<(usize, Vec<f32>)>,
    pending: Vec<f32>,
}

fn mel(hz: f32) -> f32 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

fn hz(mel: f32) -> f32 {
    700.0 * (10f32.powf(mel / 2595.0) - 1.0)
}

impl Extractor {
    pub fn new(rate: u32) -> Self {
        let length = (WINDOW_SECONDS * rate as f32).round() as usize;
        let size = length.next_power_of_two();
        let window = (0..length).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / length as f32).cos()).collect();
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(size);

        let bin_hz = rate as f32 / size as f32;
        let high = HIGH_HZ.min(rate as f32 / 2.0);
        let edges: Vec<f32> = (0..MEL_BANDS + 2).map(|i| hz(mel(LOW_HZ) + (mel(high) - mel(LOW_HZ)) * i as f32 / (MEL_BANDS + 1) as f32)).collect();
        let filters = edges
            .windows(3)
            .map(|e| {
                let first = (e[0] / bin_hz).ceil() as usize;
                let last = ((e[2] / bin_hz).floor() as usize).min(size / 2);
                let weights = (first..=last)
                    .map(|b| {
                        let f = b as f32 * bin_hz;
                        if f <= e[1] { (f - e[0]) / (e[1] - e[0]) } else { (e[2] - f) / (e[2] - e[1]) }.max(0.0)
                    })
                    .collect();
                (first, weights)
            })
            .collect();
        Self { hop: (HOP_SECONDS * rate as f32).round() as usize, window, fft, filters, pending: Vec::new() }
    }

    /// Adds samples and returns the frames that became complete.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Frame> {
        self.pending.extend_from_slice(samples);
        let mut frames = Vec::new();
        let mut input = self.fft.make_input_vec();
        let mut spectrum = self.fft.make_output_vec();
        let mut start = 0;
        while start + self.window.len() <= self.pending.len() {
            let slice = &self.pending[start..start + self.window.len()];
            let rms = (slice.iter().map(|x| x * x).sum::<f32>() / slice.len() as f32).sqrt();
            input.fill(0.0);
            for ((x, s), w) in input.iter_mut().zip(slice).zip(&self.window) {
                *x = s * w;
            }
            self.fft.process(&mut input, &mut spectrum).expect("FFT buffer sizes");
            let bands: Vec<f32> = self.filters.iter().map(|(first, weights)| weights.iter().zip(&spectrum[*first..]).map(|(w, c)| w * c.norm_sqr()).sum::<f32>().max(1e-10).ln()).collect();
            let level = (20.0 * rms.max(1e-9).log10()).max(FLOOR_DB);
            frames.push(Frame { mfcc: cepstrum(&bands), voiced: rms >= QUIET_LEVEL, level });
            start += self.hop;
        }
        self.pending.drain(..start);
        frames
    }
}

/// DCT-II of the log mel energies, coefficients 1..=COEFFICIENTS.
fn cepstrum(bands: &[f32]) -> [f32; COEFFICIENTS] {
    let n = bands.len() as f32;
    std::array::from_fn(|k| {
        let k = (k + 1) as f32;
        bands.iter().enumerate().map(|(i, b)| b * (std::f32::consts::PI * k * (i as f32 + 0.5) / n).cos()).sum::<f32>() * (2.0 / n).sqrt()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, hz: f32, seconds: f32) -> Vec<f32> {
        (0..(rate as f32 * seconds) as usize).map(|i| 0.3 * (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin()).collect()
    }

    #[test]
    fn one_frame_per_centisecond_and_streaming_matches() {
        let samples = tone(24_000, 440.0, 0.5);
        let whole = Extractor::new(24_000).push(&samples);
        let mut parts = Extractor::new(24_000);
        let mut streamed = parts.push(&samples[..1000]);
        streamed.extend(parts.push(&samples[1000..]));
        assert_eq!(whole.len(), 48);
        assert_eq!(streamed.len(), whole.len());
        assert_eq!(streamed[20].mfcc, whole[20].mfcc);
        assert!(whole.iter().all(|f| f.voiced));
    }

    #[test]
    fn rates_give_similar_features() {
        let a = Extractor::new(22_050).push(&tone(22_050, 700.0, 0.2));
        let b = Extractor::new(24_000).push(&tone(24_000, 700.0, 0.2));
        let distance: f32 = a[10].mfcc.iter().zip(b[10].mfcc).map(|(x, y)| (x - y).powi(2)).sum::<f32>().sqrt();
        let size: f32 = a[10].mfcc.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(distance < 0.2 * size, "{distance} vs {size}");
    }
}
