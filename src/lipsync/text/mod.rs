//! The `text` lip-sync mode: mouth shapes from the spoken text, timed to the audio while it streams in.
//!
//! 1. eSpeak NG speaks the text (`espeak`), which gives its phonemes, with their start times in that synthetic speech, in any of its languages.
//! 2. Both the synthetic and the real speech become MFCC frames (`features`); dynamic time warping pairs them (`align`), so each phoneme gets its start in the real speech.
//! 3. The phonemes map to Rhubarb's phones (`ipa`), and Rhubarb's animation rules (`rhubarb_animation`) turn them into mouth shapes.
//!
//! The alignment of the newest audio is still uncertain, so cues are committed `LOOKAHEAD` behind it; everything after the committed point is redone on each pass.

mod align;
mod espeak;
mod features;
mod ipa;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use bevy::log::{info, warn};

use super::rhubarb_animation::timeline::{Cs, TimeRange, Timeline};
use super::rhubarb_animation::{self, Phone, ShapeSet};
use super::{Cue, Request};
use crate::audio::Shared;
use features::Frame;

/// Real frames behind the newest one before their alignment counts as settled (0.3 s).
const LOOKAHEAD: usize = 30;
/// Silent frames added before and after the synthetic speech, so leading and trailing silence in the real speech has a partner.
const PADDING: usize = 30;
/// A silent stretch in the real speech at least this long (in frames) is a pause, even where the synthetic speech has none. Shorter ones are the closures of stops (p, t, k).
const MIN_PAUSE: usize = 15;
const POLL: Duration = Duration::from_millis(40);

/// The synthetic speech, prepared for alignment.
struct Reference {
    frames: Vec<Frame>,
    /// Rhubarb's phones with their first frame, sorted; a phoneme with two phones (ts) splits its frames.
    phones: Vec<(usize, Option<Phone>)>,
}

fn reference(request: &Request) -> Result<Reference, String> {
    let voice = espeak::voice_for(&request.language).ok_or_else(|| format!("no eSpeak NG voice for the language {:?}", request.language))?;
    let speech = espeak::speak(voice, &request.text)?;
    let silent = Frame { mfcc: [0.0; features::COEFFICIENTS], voiced: false, level: features::FLOOR_DB };
    let mut frames = vec![silent; PADDING];
    frames.extend(features::Extractor::new(speech.rate).push(&speech.samples));
    frames.extend(std::iter::repeat_n(silent, PADDING));

    // Frame k covers 10k..10k + 25 ms; a phoneme belongs to the first frame centred after its start.
    let frame_of = |seconds: f32| PADDING + ((seconds - features::WINDOW_SECONDS / 2.0).max(0.0) / features::HOP_SECONDS).round() as usize;
    let end = frames.len() - PADDING;
    let mut phones = Vec::new();
    let mut unknown = BTreeSet::new();
    for (k, phoneme) in speech.phonemes.iter().enumerate() {
        let start = frame_of(phoneme.start).min(end);
        let next = speech.phonemes.get(k + 1).map_or(end, |p| frame_of(p.start).min(end));
        let (mapped, missing) = ipa::phones(&phoneme.ipa);
        unknown.extend(missing.chars());
        if mapped.is_empty() {
            phones.push((start, None));
        }
        for (part, &phone) in mapped.iter().enumerate() {
            phones.push((start + (next.saturating_sub(start)) * part / mapped.len(), Some(phone)));
        }
    }
    phones.push((end, None));
    include_closures(&mut phones, &frames);
    if !unknown.is_empty() {
        warn!("text lip sync: no mouth shape for the IPA symbols {}", unknown.iter().collect::<String>());
    }
    Ok(Reference { frames, phones })
}

/// Frames below the speech's peak by this much count as a stop's closure.
const CLOSURE_DB: f32 = 25.0;
/// Longest closure looked for before a stop, in frames.
const MAX_CLOSURE: usize = 15;

fn is_stop(phone: Phone) -> bool {
    matches!(phone, Phone::P | Phone::B | Phone::T | Phone::D | Phone::K | Phone::G | Phone::CH | Phone::JH)
}

/// eSpeak NG starts a stop (p, t, k, ...) at its burst, which leaves the silent closure before it to the previous phone; the lips close during the closure, so the stop starts where its closure starts.
fn include_closures(phones: &mut [(usize, Option<Phone>)], frames: &[Frame]) {
    let peak = frames.iter().map(|f| f.level).fold(features::FLOOR_DB, f32::max);
    let quiet = |k: usize| frames[k].level < peak - CLOSURE_DB;
    for n in 1..phones.len() {
        if !phones[n].1.is_some_and(is_stop) {
            continue;
        }
        let (start, earliest) = (phones[n].0, phones[n - 1].0 + 1);
        // The closure ends at most a few frames before the burst.
        let Some(last_quiet) = (start.saturating_sub(4)..start).rev().find(|&k| k >= earliest && quiet(k)) else { continue };
        let mut first = last_quiet;
        while first > earliest && last_quiet - first < MAX_CLOSURE && quiet(first - 1) {
            first -= 1;
        }
        phones[n].0 = first;
    }
}

/// Starts the worker that adds cues to utterance `id` while its audio arrives.
pub fn start(shared: Arc<Shared>, id: u64, request: Request) {
    std::thread::Builder::new()
        .name("riggy-text-lipsync".into())
        .spawn(move || {
            if let Err(err) = work(&shared, id, &request) {
                warn!("text lip sync failed, falling back to sound-driven lip sync: {err}");
                if let Some(u) = shared.queue.lock().unwrap().iter_mut().find(|u| u.id == id) {
                    u.cue_failed = true;
                }
            }
        })
        .expect("spawn text lip-sync thread");
}

fn work(shared: &Shared, id: u64, request: &Request) -> Result<(), String> {
    let started = std::time::Instant::now();
    let reference = reference(request)?;
    info!("text lip sync: {} phones from eSpeak NG in {:.2} s", reference.phones.len(), started.elapsed().as_secs_f32());
    let mut aligner = align::Aligner::new(&reference.frames);
    let mut extractor: Option<features::Extractor> = None;
    let mut real_voiced = Vec::new();
    let mut consumed = 0;
    let mut committed = 0;

    loop {
        let (samples, rate, finished) = {
            let queue = shared.queue.lock().unwrap();
            let Some(u) = queue.iter().find(|u| u.id == id) else { return Ok(()) };
            (u.samples[consumed..].to_vec(), u.rate, u.finished)
        };
        consumed += samples.len();
        for frame in extractor.get_or_insert_with(|| features::Extractor::new(rate)).push(&samples) {
            real_voiced.push(frame.voiced);
            aligner.push(frame);
        }

        let done = finished && samples.is_empty();
        let settled = if done { aligner.len() } else { aligner.len().saturating_sub(LOOKAHEAD) };
        if settled > committed || (done && aligner.len() > 0) {
            let phones = real_phones(&reference, &aligner.first_real_frames(done), &real_voiced);
            let animation = rhubarb_animation::animate(&phones, ShapeSet::all());
            let to_sample = |cs: Cs| cs as usize * rate as usize / 100;
            let cues: Vec<Cue> = animation.iter().filter(|t| (t.end() as usize) > committed && (t.start() as usize) < settled).map(|t| Cue { start: to_sample(t.start().max(committed as Cs)), shape: t.value }).collect();
            let mut queue = shared.queue.lock().unwrap();
            let Some(u) = queue.iter_mut().find(|u| u.id == id) else { return Ok(()) };
            let until = if done { u.samples.len() } else { to_sample(settled as Cs) };
            u.set_cues_from(to_sample(committed as Cs), &cues, until);
            committed = settled;
        }
        if done {
            info!("text lip sync: {:.1} s of speech aligned", consumed as f32 / rate as f32);
            return Ok(());
        }
        std::thread::sleep(POLL);
    }
}

/// The phones in the real speech: each starts at the first real frame paired with its first reference frame; long silent stretches in the real speech become pauses.
fn real_phones(reference: &Reference, firsts: &[Option<usize>], real_voiced: &[bool]) -> Timeline<Phone> {
    let total = real_voiced.len() as Cs;
    let mut phones = Timeline::bounded(TimeRange::new(0, total), false);
    let mut starts: Vec<(Option<usize>, Option<Phone>)> = reference.phones.iter().map(|&(frame, phone)| (firsts.get(frame).copied().flatten(), phone)).collect();
    // A stop is aligned from its closure (`include_closures`), but Rhubarb's rules expect it to start at the burst and close the lips before it themselves.
    for k in 0..starts.len() {
        let (Some(start), Some(phone)) = starts[k] else { continue };
        if !is_stop(phone) {
            continue;
        }
        let limit = starts[k + 1..].iter().find_map(|&(s, _)| s).unwrap_or(real_voiced.len()).min(start + MAX_CLOSURE);
        if let Some(burst) = (start..limit).find(|&f| real_voiced[f]) {
            starts[k].0 = Some(burst);
        }
    }
    for (k, &(start, phone)) in starts.iter().enumerate() {
        let (Some(start), Some(phone)) = (start, phone) else { continue };
        // Up to the next phone placed on the path; the newest one runs to the end of the audio so far.
        let end = starts[k + 1..].iter().find_map(|&(s, _)| s).unwrap_or(total as usize);
        if end > start {
            phones.set_value(start as Cs, end as Cs, phone);
        } else if is_stop(phone) && end > 0 {
            // A stop squeezed to nothing still closes the lips (Rhubarb times the closure before it): give it the frame before the next phone.
            phones.set_value(end as Cs - 1, end as Cs, phone);
        }
    }

    let mut run = 0;
    for (i, &voiced) in real_voiced.iter().chain(std::iter::once(&true)).enumerate() {
        if !voiced {
            run += 1;
            continue;
        }
        if run >= MIN_PAUSE {
            // A stop right before the speech resumes is its closure; keep it.
            let end = if phones.get(i as Cs - 1).is_some_and(|t| is_stop(t.value)) { i - 1 } else { i };
            phones.clear(TimeRange::new((i - run) as Cs, end as Cs));
        }
        run = 0;
    }
    phones
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tuning aid, see doc/development.md: prints the timed phones and mouth shapes for a recording.
    #[test]
    #[ignore]
    fn prints_the_alignment_of_a_recording() {
        let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {name}"));
        let bytes = std::fs::read(var("RIGGY_PCM")).unwrap();
        let samples: Vec<f32> = bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
        let request = Request { text: var("RIGGY_TEXT"), language: std::env::var("RIGGY_LANGUAGE").unwrap_or_default() };

        let speech = espeak::speak(espeak::voice_for(&request.language).unwrap(), &request.text).unwrap();
        println!("espeak: {:.2} s, {}", speech.samples.len() as f32 / speech.rate as f32, speech.phonemes.iter().map(|p| format!("{:.2}:{}", p.start, p.ipa)).collect::<Vec<_>>().join(" "));
        let reference = reference(&request).unwrap();
        let pattern = |v: &mut dyn Iterator<Item = bool>| v.map(|v| if v { '#' } else { '.' }).collect::<String>();
        println!("espeak voiced: {}", pattern(&mut reference.frames.iter().map(|f| f.voiced)));
        let mut aligner = align::Aligner::new(&reference.frames);
        let frames = features::Extractor::new(24_000).push(&samples);
        let voiced: Vec<bool> = frames.iter().map(|f| f.voiced).collect();
        frames.into_iter().for_each(|f| aligner.push(f));
        println!("real voiced:   {}{}", ".".repeat(PADDING), pattern(&mut voiced.iter().copied()));
        let phones = real_phones(&reference, &aligner.first_real_frames(true), &voiced);
        let animation = rhubarb_animation::animate(&phones, ShapeSet::all());
        let line = |items: Vec<String>| items.join(" ");
        println!("phones: {}", line(phones.iter().map(|t| format!("{:.2}:{:?}", t.start() as f32 / 100.0, t.value)).collect()));
        println!("shapes: {}", line(animation.iter().map(|t| format!("{:.2}:{:?}", t.start() as f32 / 100.0, t.value)).collect()));
    }

    #[test]
    fn long_silences_become_pauses() {
        let voiced = |v: bool| Frame { mfcc: [0.0; features::COEFFICIENTS], voiced: v, level: 0.0 };
        let reference = Reference { frames: vec![voiced(true); 4], phones: vec![(0, Some(Phone::AA)), (2, Some(Phone::M)), (4, None)] };
        let real: Vec<bool> = [vec![true; 10], vec![false; 20], vec![true; 10]].concat();
        let firsts = [Some(0), Some(5), Some(30), Some(35)];
        let phones = real_phones(&reference, &firsts, &real);
        let parts: Vec<_> = phones.iter().map(|t| (t.start(), t.end(), t.value)).collect();
        assert_eq!(parts, [(0, 10, Phone::AA), (30, 40, Phone::M)]);
    }
}
