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
use std::time::{Duration, Instant};

use bevy::prelude::*;
use serde::Deserialize;

use crate::audio::{self, Shared, Utterance};
use crate::avatar::AvatarSet;
use crate::config::Config;
use crate::lipsync;
use crate::mouth::MouthTarget;

pub struct SpeechPlugin;

impl Plugin for SpeechPlugin {
    fn build(&self, app: &mut App) {
        let shared = Arc::new(Shared::default());
        audio::start(shared.clone());
        listen(app.world().resource::<Config>().speech.socket_path(), shared.clone());
        app.insert_resource(Feed(shared)).init_resource::<Voice>().add_systems(Update, lip_sync.before(AvatarSet::Express));
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

/// Without a measure for this long, the audio thread has stopped and riggy counts as silent.
const STALE: Duration = Duration::from_millis(200);

/// Memory of the lip-sync system between frames.
#[derive(Default)]
struct LipState {
    /// When the last measure arrived.
    last: Option<Instant>,
    /// Mouth shape of the last sound, scaled to a total weight of 1.
    shape: [f32; 5],
    /// Start of the current quiet stretch within speech.
    quiet_since: Option<Instant>,
}

/// Sets the mouth from the measure being heard now; leaves it alone while nothing plays, so the debug keys still work.
///
/// Speakers rarely close the mouth between words: in a quiet stretch shorter than `[mouth] pause_hold` the last shape stays open at `pause_opening`.
fn lip_sync(feed: Res<Feed>, config: Res<Config>, mut voice: ResMut<Voice>, mut target: ResMut<MouthTarget>, mut state: Local<LipState>) {
    let now = Instant::now();
    let heard = now.checked_sub(Duration::from_secs_f32(config.mouth.audio_latency.max(0.0))).unwrap_or(now);

    let mut current = None;
    {
        let mut frames = feed.0.frames.lock().unwrap();
        while frames.front().is_some_and(|f| f.at <= heard) {
            current = frames.pop_front();
        }
    }

    let was_speaking = voice.speaking;
    match current {
        Some(frame) => {
            state.last = Some(now);
            voice.speaking = frame.active;
            voice.level = frame.measure.level;
            if frame.active {
                let weights = lipsync::visemes(&frame.measure, config.mouth.silence_level, config.mouth.loud_level);
                let total: f32 = weights.iter().sum();
                if total > 0.0 {
                    state.shape = weights.map(|w| w / total);
                    state.quiet_since = None;
                    target.0 = weights;
                } else {
                    let since = *state.quiet_since.get_or_insert(now);
                    let open = if (now - since).as_secs_f32() < config.mouth.pause_hold { config.mouth.pause_opening } else { 0.0 };
                    target.0 = state.shape.map(|w| w * open);
                }
            }
        }
        None if state.last.is_none_or(|t| now - t > STALE) => {
            voice.speaking = false;
            voice.level = 0.0;
        }
        None => {}
    }
    if was_speaking && !voice.speaking {
        target.0 = [0.0; 5];
        *state = LipState { last: state.last, ..default() };
        info!("speech ended");
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
fn receive(stream: UnixStream, shared: &Shared) -> io::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let header: Header = serde_json::from_str(&line).map_err(io::Error::other)?;
    if header.rate == 0 {
        return Err(io::Error::other("rate must be positive"));
    }
    info!("saying ({}): {}", header.language, header.text);

    shared.queue.lock().unwrap().push_back(Utterance::new(header.rate));
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
