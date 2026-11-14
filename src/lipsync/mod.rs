//! Lip sync: how the mouth follows speech.
//!
//! Every utterance has a cue source, chosen by `[lipsync] mode`:
//! - `sound`: no cues; the mouth follows loudness and spectrum of the sound being heard (`sound`).
//! - `rhubarb`: Rhubarb Lip Sync's mouth shapes, recognised from the audio (`rhubarb_program`, Cargo feature `rhubarb`).
//! - `text`: mouth shapes from the text, timed to the audio (`text`, Cargo feature `espeak`), animated with Rhubarb's rules (`rhubarb_animation`).
//!
//! Cue sources attach timed shapes (`Cue`) to the utterance; wherever cues are missing, the sound-driven lip sync takes over.

mod cues;
// Only the text mode animates phones; without it, only `Shape` is used.
#[cfg_attr(not(feature = "espeak"), allow(dead_code))]
pub mod rhubarb_animation;
#[cfg(feature = "rhubarb")]
mod rhubarb_program;
pub mod sound;
#[cfg(feature = "espeak")]
mod text;

use std::sync::Arc;

use bevy::log::warn;

pub use cues::{Cue, cue_at};
pub use rhubarb_animation::Shape;

use crate::audio::Shared;
use crate::config::{LipsyncConfig, LipsyncMode};

/// RMS level (full scale 1) below which a stretch of audio counts as silent.
#[cfg_attr(not(any(feature = "rhubarb", feature = "espeak")), allow(dead_code))]
pub const QUIET_LEVEL: f32 = 0.01;

/// What is being said, from the speech socket's header.
#[cfg_attr(not(feature = "espeak"), allow(dead_code))]
#[derive(Clone, Debug, Default)]
pub struct Request {
    pub text: String,
    pub language: String,
}

/// Where an utterance's mouth shapes come from.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum CueSource {
    #[default]
    Sound,
    #[cfg(feature = "rhubarb")]
    Rhubarb(rhubarb_program::Settings),
    #[cfg(feature = "espeak")]
    Text,
}

impl CueSource {
    /// The source for `mode`; a mode left out of this build falls back to `Sound`.
    pub fn new(mode: LipsyncMode, config: &LipsyncConfig) -> Self {
        match mode {
            LipsyncMode::Sound => Self::Sound,
            #[cfg(feature = "rhubarb")]
            LipsyncMode::Rhubarb => Self::Rhubarb(rhubarb_program::Settings { program: config.rhubarb.clone(), recognizer: config.recognizer.clone() }),
            #[cfg(feature = "espeak")]
            LipsyncMode::Text => Self::Text,
            #[allow(unreachable_patterns)]
            other => {
                let _ = config;
                warn!("lip-sync mode {other:?} is not in this build (Cargo features rhubarb, espeak); using sound");
                Self::Sound
            }
        }
    }

    /// The modes this build offers, in the order F3 cycles through them.
    pub fn available_modes() -> Vec<LipsyncMode> {
        let mut modes = vec![LipsyncMode::Sound];
        if cfg!(feature = "rhubarb") {
            modes.push(LipsyncMode::Rhubarb);
        }
        if cfg!(feature = "espeak") {
            modes.push(LipsyncMode::Text);
        }
        modes
    }

    /// Playback waits for the first cues.
    pub fn wants_cues(&self) -> bool {
        *self != Self::Sound
    }

    /// Starts adding cues to utterance `id`, which is already queued.
    pub fn start(&self, shared: &Arc<Shared>, id: u64, request: &Request) {
        let _ = (shared, id, request);
        match self {
            Self::Sound => {}
            #[cfg(feature = "rhubarb")]
            Self::Rhubarb(settings) => rhubarb_program::start(shared.clone(), id, settings.clone()),
            #[cfg(feature = "espeak")]
            Self::Text => text::start(shared.clone(), id, request.clone()),
        }
    }
}
