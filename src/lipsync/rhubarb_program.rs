//! The `rhubarb` lip-sync mode: mouth shapes with timings from the Rhubarb Lip Sync program, recognised from the speech itself.
//!
//! Rhubarb needs finished audio, while the TTS streams it. A worker thread per utterance therefore cuts the arriving audio at pauses (at least 1 s, at most 4 s per piece), runs `rhubarb` on each piece and adds the resulting cues to the utterance. Playback starts once the first piece has its cues, so speech starts later than in the sound-driven mode. Where cues are missing (Rhubarb failed or fell behind), the sound-driven lip sync takes over.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use bevy::log::{info, warn};
use serde::Deserialize;

use super::{Cue, QUIET_LEVEL, Shape};
use crate::audio::Shared;

/// How to run Rhubarb.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub program: PathBuf,
    /// `phonetic` (any language) or `pocketSphinx` (English only, slower than real time).
    pub recognizer: String,
}

const MIN_PIECE_SECONDS: f32 = 1.0;
const MAX_PIECE_SECONDS: f32 = 4.0;
/// A pause to cut at: this long below `QUIET_LEVEL`.
const PAUSE_SECONDS: f32 = 0.12;
const POLL: Duration = Duration::from_millis(40);

/// Length of the next piece to analyse, cut in the middle of the first pause after `MIN_PIECE_SECONDS`, or at the quietest 10 ms before `MAX_PIECE_SECONDS`; `None` while more audio is needed.
pub fn next_cut(samples: &[f32], rate: u32, finished: bool) -> Option<usize> {
    let block = (rate / 100).max(1) as usize;
    let rms = |b: usize| {
        let chunk = &samples[b * block..(b + 1) * block];
        (chunk.iter().map(|x| x * x).sum::<f32>() / block as f32).sqrt()
    };
    let blocks = samples.len() / block;
    let first = (MIN_PIECE_SECONDS * 100.0) as usize;
    let last = (MAX_PIECE_SECONDS * 100.0) as usize;
    let pause = (PAUSE_SECONDS * 100.0) as usize;

    let mut run = 0;
    for b in first..blocks.min(last) {
        run = if rms(b) < QUIET_LEVEL { run + 1 } else { 0 };
        if run == pause {
            return Some((b + 1 - pause / 2) * block);
        }
    }
    if blocks >= last {
        let quietest = (first..last).min_by(|&a, &b| rms(a).total_cmp(&rms(b))).unwrap();
        return Some(quietest * block + block / 2);
    }
    (finished && !samples.is_empty()).then_some(samples.len())
}

/// Starts the worker that adds cues to utterance `id` while its audio arrives.
pub fn start(shared: Arc<Shared>, id: u64, settings: Settings) {
    std::thread::Builder::new()
        .name("riggy-rhubarb".into())
        .spawn(move || work(&shared, id, &settings))
        .expect("spawn rhubarb thread");
}

fn work(shared: &Shared, id: u64, settings: &Settings) {
    let mut piece_start = 0;
    loop {
        let (samples, rate, finished) = {
            let queue = shared.queue.lock().unwrap();
            let Some(u) = queue.iter().find(|u| u.id == id) else { return };
            (u.samples[piece_start..].to_vec(), u.rate, u.finished)
        };
        let Some(length) = next_cut(&samples, rate, finished) else {
            if finished {
                return;
            }
            std::thread::sleep(POLL);
            continue;
        };

        let result = analyse(&samples[..length], rate, settings, &format!("{id}-{piece_start}"));
        let mut queue = shared.queue.lock().unwrap();
        let Some(u) = queue.iter_mut().find(|u| u.id == id) else { return };
        match result {
            Ok(cues) => {
                let cues: Vec<Cue> = cues.into_iter().map(|c| Cue { start: piece_start + c.start, ..c }).collect();
                u.set_cues_from(piece_start, &cues, piece_start + length);
            }
            Err(err) => {
                warn!("rhubarb failed, falling back to sound-driven lip sync: {err}");
                u.cue_failed = true;
                return;
            }
        }
        piece_start += length;
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    mouth_cues: Vec<OutputCue>,
}

#[derive(Deserialize)]
struct OutputCue {
    start: f32,
    value: Shape,
}

/// Runs Rhubarb on one piece; cue starts are sample indices within the piece.
fn analyse(samples: &[f32], rate: u32, settings: &Settings, name: &str) -> Result<Vec<Cue>, String> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let wav = runtime.join(format!("riggy-rhubarb-{}-{name}.wav", std::process::id()));
    std::fs::write(&wav, wav_bytes(samples, rate)).map_err(|e| format!("{}: {e}", wav.display()))?;

    let started = std::time::Instant::now();
    let output = Command::new(&settings.program).args(["-q", "-f", "json", "--extendedShapes", "GHX", "-r", &settings.recognizer]).arg(&wav).output();
    let _ = std::fs::remove_file(&wav);
    let output = output.map_err(|e| format!("{}: {e}", settings.program.display()))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let parsed: Output = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    info!("rhubarb: {:.1} s of speech in {:.2} s", samples.len() as f32 / rate as f32, started.elapsed().as_secs_f32());
    Ok(parsed.mouth_cues.into_iter().map(|c| Cue { start: (c.start * rate as f32).round() as usize, shape: c.value }).collect())
}

/// 16-bit mono WAV file contents.
fn wav_bytes(samples: &[f32], rate: u32) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for x in samples {
        bytes.extend_from_slice(&((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 1000;

    fn speech(seconds: f32) -> Vec<f32> {
        vec![0.1; (seconds * RATE as f32) as usize]
    }

    fn silence(seconds: f32) -> Vec<f32> {
        vec![0.0; (seconds * RATE as f32) as usize]
    }

    #[test]
    fn cuts_in_the_first_pause_after_one_second() {
        let samples = [speech(0.5), silence(0.2), speech(1.0), silence(0.2), speech(0.5)].concat();
        let cut = next_cut(&samples, RATE, false).unwrap();
        // Second pause: 1.7–1.9 s; cut in the middle of its first 120 ms.
        assert!((1750..=1770).contains(&cut), "{cut}");
    }

    #[test]
    fn waits_for_more_audio_without_a_pause() {
        assert_eq!(next_cut(&speech(2.0), RATE, false), None);
        assert_eq!(next_cut(&speech(2.0), RATE, true), Some(2000));
        assert!(next_cut(&speech(4.5), RATE, false).is_some_and(|c| (1000..4000).contains(&c)));
    }

    #[test]
    fn wav_header_matches_the_data() {
        let bytes = wav_bytes(&[0.0, 0.5, -0.5], 24_000);
        assert_eq!(bytes.len(), 44 + 6);
        assert_eq!(&bytes[40..44], &6u32.to_le_bytes());
    }
}
