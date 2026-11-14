//! Speech playback: plays queued utterances on the default output device and measures what it plays, on the same sample clock.
//!
//! The output callback resamples each utterance linearly to the device rate, feeds every played sample to the lip-sync [`Analyzer`], and stamps each measure with the wall-clock time the device reports for its playback. The lip-sync system then only has to pick the measure whose time has come.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::log::{info, warn};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, OutputCallbackInfo, SampleFormat, SizedSample, StreamConfig};

use crate::lipsync::sound::{Analyzer, Measure};
use crate::lipsync::{Cue, CueSource, Shape, cue_at};

/// Audio that waits before playback starts, so a slow TTS stream does not stutter.
const PREBUFFER_SECONDS: f32 = 0.2;
/// Cues that must exist before playback starts, when cues are wanted, so the mouth does not fall back to sound-driven lip sync right away.
const CUE_LEAD_SECONDS: f32 = 0.2;
/// Measures kept for the lip-sync system (2 s at 10 ms hops).
const MAX_FRAMES: usize = 200;

/// One spoken text as mono samples, filled while it arrives.
pub struct Utterance {
    /// Cue sources find their utterance by it.
    #[cfg_attr(not(any(feature = "rhubarb", feature = "espeak")), allow(dead_code))]
    pub id: u64,
    pub rate: u32,
    pub samples: Vec<f32>,
    /// All samples have arrived.
    pub finished: bool,
    started: bool,
    /// Playback waits for the first cues.
    pub wants_cues: bool,
    /// Mouth shapes, sorted by start.
    pub cues: Vec<Cue>,
    /// Samples the cues cover so far.
    pub cued_until: usize,
    pub cue_failed: bool,
}

impl Utterance {
    pub fn new(id: u64, rate: u32, wants_cues: bool) -> Self {
        Self {
            id,
            rate,
            samples: Vec::new(),
            finished: false,
            started: false,
            wants_cues,
            cues: Vec::new(),
            cued_until: 0,
            cue_failed: false,
        }
    }

    #[cfg_attr(not(any(feature = "rhubarb", feature = "espeak")), allow(dead_code))]
    /// Replaces the cues from sample `from` on with `cues` (sorted, all starting at or after `from`), which cover the audio up to `until`.
    pub fn set_cues_from(&mut self, from: usize, cues: &[Cue], until: usize) {
        self.cues.truncate(self.cues.partition_point(|c| c.start < from));
        self.cues.extend_from_slice(cues);
        self.cued_until = until;
    }

    /// Ready to play: enough audio is buffered and, when cues are wanted, the cues lead by `CUE_LEAD_SECONDS` or cover everything (or their source gave up).
    fn ready(&self) -> bool {
        let buffered = self.finished || self.samples.len() as f32 >= self.rate as f32 * PREBUFFER_SECONDS;
        let lead = (self.rate as f32 * CUE_LEAD_SECONDS) as usize;
        let cued = !self.wants_cues || self.cue_failed || self.cued_until >= lead.min(self.samples.len()) && (self.cued_until > 0 || self.finished);
        buffered && cued
    }
}

/// Sound measures with the time they are heard.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub at: Instant,
    pub measure: Measure,
    /// An utterance was playing (or stalled waiting for data) in this window.
    pub active: bool,
    /// The cued mouth shape at the window's centre, where cues exist.
    pub cue: Option<Shape>,
}

/// State shared by the socket thread (fills `queue`), the audio thread and the lip-sync system (reads `frames`).
#[derive(Default)]
pub struct Shared {
    pub queue: Mutex<VecDeque<Utterance>>,
    pub frames: Mutex<VecDeque<Frame>>,
    /// Where the mouth shapes of new utterances come from.
    pub cue_source: Mutex<CueSource>,
}

/// Opens the default output device on its own thread, which keeps the stream alive for the program's lifetime.
pub fn start(shared: Arc<Shared>) {
    std::thread::Builder::new()
        .name("riggy-audio".into())
        .spawn(move || match open(shared) {
            Ok(_stream) => loop {
                std::thread::park();
            },
            Err(err) => warn!("audio output unavailable, speech stays silent: {err}"),
        })
        .expect("spawn audio thread");
}

fn open(shared: Arc<Shared>) -> Result<cpal::Stream, Box<dyn std::error::Error>> {
    let device = cpal::default_host().default_output_device().ok_or("no default output device")?;
    let supported = device.default_output_config()?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    info!("audio output: {} Hz, {} channels, {format:?}", config.sample_rate, config.channels);
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config, shared)?,
        SampleFormat::I16 => build::<i16>(&device, config, shared)?,
        SampleFormat::I32 => build::<i32>(&device, config, shared)?,
        other => return Err(format!("unsupported sample format {other:?}").into()),
    };
    stream.play()?;
    Ok(stream)
}

fn build<T: SizedSample + FromSample<f32>>(device: &cpal::Device, config: StreamConfig, shared: Arc<Shared>) -> Result<cpal::Stream, cpal::Error> {
    let rate = config.sample_rate;
    let channels = config.channels as usize;
    let mut player = Player::default();
    let mut analyzer = Analyzer::new(rate);
    let half_window = analyzer.window_seconds() / 2.0;
    let mut measures: Vec<(usize, Measure, bool, Option<Shape>)> = Vec::with_capacity(64);

    let callback = move |data: &mut [T], info: &OutputCallbackInfo| {
        let timestamp = info.timestamp();
        let start = Instant::now() + timestamp.playback.saturating_duration_since(timestamp.callback);

        measures.clear();
        {
            let mut queue = shared.queue.lock().unwrap();
            for (i, frame) in data.chunks_mut(channels).enumerate() {
                let (value, active) = player.next(&mut queue, rate);
                frame.fill(T::from_sample(value));
                if let Some(measure) = analyzer.push(value) {
                    measures.push((i, measure, active, player.cue(&queue, half_window)));
                }
            }
        }

        let mut frames = shared.frames.lock().unwrap();
        for &(i, measure, active, cue) in &measures {
            // The window ends at sample i; it is heard best at its centre.
            let offset = i as f32 / rate as f32 - half_window;
            let at = if offset >= 0.0 { start + Duration::from_secs_f32(offset) } else { start - Duration::from_secs_f32(-offset) };
            frames.push_back(Frame { at, measure, active, cue });
        }
        let excess = frames.len().saturating_sub(MAX_FRAMES);
        frames.drain(..excess);
    };
    device.build_output_stream(config, callback, |err| warn!("audio stream: {err}"), None)
}

/// Read position in the front utterance.
#[derive(Default)]
struct Player {
    position: f64,
}

impl Player {
    /// The next output sample at `rate`, and whether an utterance is playing.
    fn next(&mut self, queue: &mut VecDeque<Utterance>, rate: u32) -> (f32, bool) {
        while let Some(utterance) = queue.front_mut() {
            if !utterance.started {
                utterance.started = utterance.ready();
                if !utterance.started {
                    // Waiting before the start counts as silence; only a stall mid-utterance counts as speaking.
                    return (0.0, false);
                }
            }
            let i = self.position as usize;
            if i + 1 < utterance.samples.len() {
                let t = (self.position - i as f64) as f32;
                let value = utterance.samples[i] * (1.0 - t) + utterance.samples[i + 1] * t;
                self.position += utterance.rate as f64 / rate as f64;
                return (value, true);
            }
            if !utterance.finished {
                // The stream is behind: wait without moving on.
                return (0.0, true);
            }
            queue.pop_front();
            self.position = 0.0;
        }
        (0.0, false)
    }

    /// The cued mouth shape `back` seconds before the current read position in the front utterance.
    fn cue(&self, queue: &VecDeque<Utterance>, back: f32) -> Option<Shape> {
        let utterance = queue.front().filter(|u| u.started)?;
        let position = self.position - (back * utterance.rate as f32) as f64;
        (position >= 0.0).then(|| cue_at(&utterance.cues, utterance.cued_until, position as usize)).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_utterances_in_order_and_resamples() {
        let mut queue = VecDeque::new();
        let mut first = Utterance::new(0, 24_000, false);
        first.samples = vec![0.0, 1.0, 0.0];
        first.finished = true;
        let mut second = Utterance::new(1, 48_000, false);
        second.samples = vec![0.5, 0.5];
        second.finished = true;
        queue.extend([first, second]);

        let mut player = Player::default();
        let played: Vec<_> = (0..7).map(|_| player.next(&mut queue, 48_000)).collect();
        let values: Vec<f32> = played.iter().map(|p| p.0).collect();
        assert_eq!(values, [0.0, 0.5, 1.0, 0.5, 0.5, 0.0, 0.0]);
        assert_eq!(played.iter().map(|p| p.1).collect::<Vec<_>>(), [true, true, true, true, true, false, false]);
    }

    #[test]
    fn waits_for_the_first_cues() {
        let mut queue = VecDeque::from([Utterance::new(0, 24_000, true)]);
        queue[0].samples = vec![0.3; 24_000];
        let mut player = Player::default();
        assert_eq!(player.next(&mut queue, 24_000), (0.0, false));
        queue[0].cued_until = 2_400;
        assert_eq!(player.next(&mut queue, 24_000), (0.0, false), "cues must lead by 0.2 s");
        queue[0].cued_until = 12_000;
        assert_eq!(player.next(&mut queue, 24_000), (0.3, true));
    }

    #[test]
    fn waits_for_the_prebuffer() {
        let mut queue = VecDeque::from([Utterance::new(0, 24_000, false)]);
        queue[0].samples = vec![0.3; 100];
        let mut player = Player::default();
        assert_eq!(player.next(&mut queue, 24_000), (0.0, false));
        queue[0].finished = true;
        assert_eq!(player.next(&mut queue, 24_000), (0.3, true));
    }
}
