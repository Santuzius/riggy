//! Speech input: a Unix socket takes spoken audio, which riggy plays and lip-syncs to.
//!
//! Protocol, one connection per utterance (later connections wait their turn):
//! 1. one line of JSON: `{"text": "...", "language": "English", "rate": 24000}` (all optional; `rate` in Hz, default 24000),
//! 2. raw PCM, signed 16-bit little-endian mono, until the sender closes the connection.
//!
//! The socket is `[speech] socket` from `riggy.toml`, by default `$XDG_RUNTIME_DIR/riggy.sock`.

use std::io::{self, BufRead, BufReader, Read};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use serde::Deserialize;

use crate::audio::{self, Shared, Utterance};
use crate::avatar::AvatarSet;
use crate::config::{Config, LipsyncConfig, LipsyncMode};
use crate::lipsync::{CueSource, Request, sound};
use crate::mouth::MouthTarget;

pub struct SpeechPlugin;

impl Plugin for SpeechPlugin {
    fn build(&self, app: &mut App) {
        let shared = Arc::new(Shared::default());
        audio::start(shared.clone());
        listen(app.world().resource::<Config>().speech.socket_path(), shared.clone());
        app.insert_resource(Feed(shared))
            .init_resource::<Voice>()
            .add_systems(Update, (lipsync_mode, lip_sync.before(AvatarSet::Express)));
    }
}

/// What riggy is saying right now, for code that reacts to speech beyond the mouth.
#[derive(Resource, Default, Debug)]
pub struct Voice {
    pub speaking: bool,
    /// RMS level of the sound being heard, full scale = 1.
    pub level: f32,
}

#[derive(Resource)]
struct Feed(Arc<Shared>);

#[derive(Deserialize)]
#[serde(default)]
struct Header {
    text: String,
    language: String,
    rate: u32,
}

impl Default for Header {
    fn default() -> Self {
        Self { text: String::new(), language: String::new(), rate: 24_000 }
    }
}

/// Applies `[lipsync]` from the config when it changes; F3 cycles through the modes of this build until the next change.
fn lipsync_mode(feed: Res<Feed>, config: Res<Config>, keys: Res<ButtonInput<KeyCode>>, mut applied: Local<Option<LipsyncConfig>>, mut mode: Local<LipsyncMode>) {
    let mut changed = false;
    if applied.as_ref() != Some(&config.lipsync) {
        *applied = Some(config.lipsync.clone());
        *mode = config.lipsync.mode;
        changed = true;
    }
    if keys.just_pressed(KeyCode::F3) {
        let modes = CueSource::available_modes();
        let next = modes.iter().position(|m| m == &*mode).map_or(0, |i| (i + 1) % modes.len());
        *mode = modes[next];
        changed = true;
    }
    if !changed {
        return;
    }
    *feed.0.cue_source.lock().unwrap() = CueSource::new(*mode, &config.lipsync);
    info!("lip sync for the next utterance: {:?}", *mode);
}

/// Without a measure for this long, the audio thread has stopped and riggy counts as silent.
const STALE: Duration = Duration::from_millis(200);

/// Memory of the lip-sync system between frames.
#[derive(Default)]
struct LipState {
    /// When the last measure arrived.
    last: Option<Instant>,
    /// Smoothed mouth shape, viseme weights summing to 1 (all 0 before the first sound).
    shape: [f32; 5],
    /// Start of the current quiet stretch within speech.
    quiet_since: Option<Instant>,
    /// Measures of the current speech, and how many of them had a Rhubarb cue.
    frames: u32,
    cued_frames: u32,
}

/// Sets the mouth from the measures heard up to now; leaves it alone while nothing plays, so the debug keys still work.
///
/// With Rhubarb cues the mouth takes each cue's shape directly. Otherwise the opening follows the loudness of each measure, while the shape follows the sound only with the lag `[mouth] shape_time`, scaled by the opening: a single 10 ms window decides the shape poorly, real lips glide between vowel shapes, and loud stressed vowels shape the lips more than weak syllables such as the "e" in "the". Speakers rarely close the mouth between words: in a quiet stretch shorter than `pause_hold` the shape stays open at `pause_opening`.
fn lip_sync(feed: Res<Feed>, config: Res<Config>, mut voice: ResMut<Voice>, mut target: ResMut<MouthTarget>, mut state: Local<LipState>) {
    let now = Instant::now();
    let heard = now.checked_sub(Duration::from_secs_f32(config.mouth.audio_latency.max(0.0))).unwrap_or(now);
    let mouth = &config.mouth;
    let k = 1.0 - (-sound::HOP_SECONDS / mouth.shape_time.max(1e-3)).exp();

    let was_speaking = voice.speaking;
    let mut received = false;
    {
        let mut frames = feed.0.frames.lock().unwrap();
        while let Some(frame) = frames.pop_front_if(|f| f.at <= heard) {
            received = true;
            voice.speaking = frame.active;
            voice.level = frame.measure.level;
            if !frame.active {
                continue;
            }
            state.frames += 1;
            state.cued_frames += frame.cue.is_some() as u32;
            if let Some(cue) = frame.cue {
                *target = MouthTarget { weights: cue.visemes(&config.lipsync.cues), cued: true };
                continue;
            }
            target.cued = false;
            let open = sound::opening(&frame.measure, mouth.silence_level, mouth.loud_level);
            let open = if open > 0.0 {
                state.quiet_since = None;
                let fresh = sound::shape(&frame.measure);
                let first = state.shape.iter().sum::<f32>() == 0.0;
                for (w, f) in state.shape.iter_mut().zip(fresh) {
                    *w = if first { f } else { *w + (f - *w) * k * open };
                }
                open
            } else {
                let since = *state.quiet_since.get_or_insert(frame.at);
                if (frame.at - since).as_secs_f32() < mouth.pause_hold { mouth.pause_opening } else { 0.0 }
            };
            target.weights = state.shape.map(|w| w * open);
        }
    }

    if received {
        state.last = Some(now);
    } else if state.last.is_none_or(|t| now - t > STALE) {
        voice.speaking = false;
        voice.level = 0.0;
    }
    if was_speaking && !voice.speaking {
        *target = MouthTarget::default();
        info!("speech ended; cues drove {} % of it", 100 * state.cued_frames / state.frames.max(1));
        *state = LipState { last: state.last, ..default() };
    }
    if !was_speaking && voice.speaking {
        info!("speech started");
    }
}

fn listen(path: PathBuf, shared: Arc<Shared>) {
    if UnixStream::connect(&path).is_ok() {
        warn!("{} is in use by another program; speech input is off", path.display());
        return;
    }
    // A socket file left over from a crashed run.
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(err) => {
            warn!("cannot listen on {}: {err}; speech input is off", path.display());
            return;
        }
    };
    info!("speech socket: {}", path.display());
    std::thread::Builder::new()
        .name("riggy-speech".into())
        .spawn(move || {
            for connection in listener.incoming() {
                let result = connection.and_then(|stream| receive(stream, &shared));
                if let Err(err) = result {
                    warn!("speech input: {err}");
                }
            }
        })
        .expect("spawn speech thread");
}

/// Reads one utterance and queues its samples as they arrive.
fn receive(stream: UnixStream, shared: &Arc<Shared>) -> io::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let header: Header = serde_json::from_str(&line).map_err(io::Error::other)?;
    if header.rate == 0 {
        return Err(io::Error::other("rate must be positive"));
    }
    info!("saying ({}): {}", header.language, header.text);

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let source = shared.cue_source.lock().unwrap().clone();
    shared.queue.lock().unwrap().push_back(Utterance::new(id, header.rate, source.wants_cues()));
    source.start(shared, id, &Request { text: header.text, language: header.language });
    let result = read_pcm(&mut reader, shared);
    if let Some(utterance) = shared.queue.lock().unwrap().back_mut() {
        utterance.finished = true;
    }
    result
}

fn read_pcm(reader: &mut impl Read, shared: &Shared) -> io::Result<()> {
    let mut buffer = [0u8; 4800];
    let mut odd_byte = None;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            return Ok(());
        }
        let mut bytes: Vec<u8> = odd_byte.take().into_iter().chain(buffer[..n].iter().copied()).collect();
        if bytes.len() % 2 == 1 {
            odd_byte = bytes.pop();
        }
        let samples = bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0);
        if let Some(utterance) = shared.queue.lock().unwrap().back_mut() {
            utterance.samples.extend(samples);
        }
    }
}
