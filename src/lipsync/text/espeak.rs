//! Speech synthesis with eSpeak NG (libespeak-ng, GPL-3.0-or-later; linked with the `espeak` feature): the text spoken in a robot voice, with the start of each phoneme.

use std::ffi::{CString, c_char, c_int, c_short, c_uint, c_void};
use std::sync::Mutex;

// From espeak-ng's speak_lib.h.
const AUDIO_OUTPUT_SYNCHRONOUS: c_int = 2;
const INITIALIZE_PHONEME_EVENTS: c_int = 0x0001;
const INITIALIZE_PHONEME_IPA: c_int = 0x0002;
const INITIALIZE_DONT_EXIT: c_int = 0x8000;
const POS_CHARACTER: c_int = 1;
const CHARS_UTF8: c_uint = 1;
const EVENT_LIST_TERMINATED: c_int = 0;
const EVENT_PHONEME: c_int = 7;
const EE_OK: c_int = 0;
const PARAMETER_RATE: c_int = 1;
/// Words per minute (eSpeak NG's default is 175); slower comes closer to the TTS voice, so the alignment has less to stretch.
const RATE: c_int = 130;

/// `espeak_EVENT`.
#[repr(C)]
struct Event {
    kind: c_int,
    unique_identifier: c_uint,
    text_position: c_int,
    length: c_int,
    /// Milliseconds from the start of the generated speech.
    audio_position: c_int,
    sample: c_int,
    user_data: *mut c_void,
    /// Union of `int number`, `const char *name` and `char string[8]`; phoneme events use the zero-terminated string.
    id: [u8; 8],
}

type SynthCallback = unsafe extern "C" fn(*mut c_short, c_int, *mut Event) -> c_int;

#[link(name = "espeak-ng")]
unsafe extern "C" {
    fn espeak_Initialize(output: c_int, buflength: c_int, path: *const c_char, options: c_int) -> c_int;
    fn espeak_SetSynthCallback(callback: SynthCallback);
    fn espeak_SetVoiceByName(name: *const c_char) -> c_int;
    fn espeak_Synth(text: *const c_void, size: usize, position: c_uint, position_type: c_int, end_position: c_uint, flags: c_uint, unique_identifier: *mut c_uint, user_data: *mut c_void) -> c_int;
    fn espeak_Synchronize() -> c_int;
    fn espeak_SetParameter(parameter: c_int, value: c_int, relative: c_int) -> c_int;
}

/// One phoneme of the synthetic speech.
#[derive(Clone, Debug, PartialEq)]
pub struct Phoneme {
    /// IPA name, e.g. `ʃ` or `aɪ`.
    pub ipa: String,
    /// Start in seconds.
    pub start: f32,
}

/// The text spoken by eSpeak NG.
pub struct Speech {
    pub rate: u32,
    pub samples: Vec<f32>,
    pub phonemes: Vec<Phoneme>,
}

#[derive(Default)]
struct Collected {
    samples: Vec<i16>,
    phonemes: Vec<(String, c_int)>,
}

/// eSpeak NG keeps global state: one synthesis at a time. Holds the sample rate once initialised.
static ENGINE: Mutex<Option<u32>> = Mutex::new(None);
/// Filled by the callback during `espeak_Synth`, which runs on the caller's thread.
static COLLECTED: Mutex<Collected> = Mutex::new(Collected { samples: Vec::new(), phonemes: Vec::new() });

unsafe extern "C" fn collect(wav: *mut c_short, count: c_int, events: *mut Event) -> c_int {
    let mut collected = COLLECTED.lock().unwrap();
    if !wav.is_null() && count > 0 {
        // SAFETY: eSpeak passes `count` valid samples at `wav`.
        collected.samples.extend_from_slice(unsafe { std::slice::from_raw_parts(wav, count as usize) });
    }
    let mut event = events;
    // SAFETY: the event list ends with an event of type LIST_TERMINATED.
    while !event.is_null() && unsafe { (*event).kind } != EVENT_LIST_TERMINATED {
        let e = unsafe { &*event };
        if e.kind == EVENT_PHONEME {
            let end = e.id.iter().position(|&b| b == 0).unwrap_or(e.id.len());
            collected.phonemes.push((String::from_utf8_lossy(&e.id[..end]).into_owned(), e.audio_position));
        }
        event = unsafe { event.add(1) };
    }
    0
}

/// Speaks `text` with the eSpeak NG voice `voice` (e.g. `en-us`, `de`).
pub fn speak(voice: &str, text: &str) -> Result<Speech, String> {
    let voice = CString::new(voice).map_err(|e| e.to_string())?;
    let text = CString::new(text).map_err(|e| e.to_string())?;
    let mut engine = ENGINE.lock().unwrap();
    // SAFETY: calls into libespeak-ng in its documented order, serialised by ENGINE; the callback only touches COLLECTED.
    unsafe {
        let rate = match *engine {
            Some(rate) => rate,
            None => {
                let rate = espeak_Initialize(AUDIO_OUTPUT_SYNCHRONOUS, 0, std::ptr::null(), INITIALIZE_PHONEME_EVENTS | INITIALIZE_PHONEME_IPA | INITIALIZE_DONT_EXIT);
                if rate <= 0 {
                    return Err("cannot initialise eSpeak NG".into());
                }
                espeak_SetSynthCallback(collect);
                *engine.insert(rate as u32)
            }
        };
        if espeak_SetVoiceByName(voice.as_ptr()) != EE_OK {
            return Err(format!("eSpeak NG has no voice {}", voice.to_string_lossy()));
        }
        espeak_SetParameter(PARAMETER_RATE, RATE, 0);
        *COLLECTED.lock().unwrap() = Collected::default();
        let bytes = text.as_bytes_with_nul();
        if espeak_Synth(bytes.as_ptr().cast(), bytes.len(), 0, POS_CHARACTER, 0, CHARS_UTF8, std::ptr::null_mut(), std::ptr::null_mut()) != EE_OK {
            return Err("eSpeak NG synthesis failed".into());
        }
        espeak_Synchronize();
        let collected = std::mem::take(&mut *COLLECTED.lock().unwrap());
        Ok(Speech {
            rate,
            samples: collected.samples.iter().map(|&s| s as f32 / 32768.0).collect(),
            phonemes: collected.phonemes.into_iter().map(|(ipa, ms)| Phoneme { ipa, start: ms as f32 / 1000.0 }).collect(),
        })
    }
}

/// The eSpeak NG voice for a TTS language name (`English`, `German`, ...); an empty name means English.
pub fn voice_for(language: &str) -> Option<&'static str> {
    Some(match language.trim().to_lowercase().as_str() {
        "" | "english" | "en" => "en-us",
        "german" | "deutsch" | "de" => "de",
        "french" | "fr" => "fr",
        "spanish" | "es" => "es",
        "italian" | "it" => "it",
        "portuguese" | "pt" => "pt",
        "russian" | "ru" => "ru",
        "japanese" | "ja" => "ja",
        "korean" | "ko" => "ko",
        "chinese" | "zh" => "cmn",
        _ => return None,
    })
}
