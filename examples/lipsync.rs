//! Tuning aid for lip sync v1: prints the sound measures and the share of each mouth shape for raw PCM files (24 kHz signed 16-bit little-endian mono).
//!
//! ```text
//! cargo run --example lipsync -- [--frames] FILE.pcm ...
//! ```
//!
//! `--frames` prints every 10 ms window; otherwise one summary line per file with the median measures of the windows above the silence level and the mean weight of each mouth shape.

#[path = "../src/lipsync/sound.rs"]
#[allow(dead_code)]
mod lipsync;

const SILENCE: f32 = 0.01;
const LOUD: f32 = 0.12;
const NAMES: [&str; 5] = ["aa", "ih", "ou", "ee", "oh"];

fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(f32::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let frames = args.iter().any(|a| a == "--frames");
    for path in args.iter().filter(|a| *a != "--frames") {
        let bytes = std::fs::read(path)?;
        let mut analyzer = lipsync::Analyzer::new(24_000);
        let measures: Vec<_> = bytes.chunks_exact(2).filter_map(|b| analyzer.push(i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)).collect();
        let voiced: Vec<_> = measures.iter().filter(|m| m.level > SILENCE).collect();

        if frames {
            for (i, m) in measures.iter().enumerate() {
                let w = lipsync::visemes(m, SILENCE, LOUD);
                let shapes: Vec<String> = NAMES.iter().zip(w).map(|(n, w)| format!("{n} {w:.2}")).collect();
                println!("{path} {:5.2} s  level {:.3}  f1 {:4.0}  front {:6.1}  {}", i as f32 * 0.01, m.level, m.f1, m.front, shapes.join("  "));
            }
            continue;
        }

        let mut sums = [0.0f32; 5];
        for m in &voiced {
            for (sum, w) in sums.iter_mut().zip(lipsync::visemes(m, SILENCE, LOUD)) {
                *sum += w;
            }
        }
        let total: f32 = sums.iter().sum::<f32>().max(1e-6);
        let shares: Vec<String> = NAMES.iter().zip(sums).map(|(n, s)| format!("{n} {:3.0} %", 100.0 * s / total)).collect();
        println!(
            "{path}: level {:.3}  f1 {:4.0}  front {:6.1}  |  {}",
            median(voiced.iter().map(|m| m.level).collect()),
            median(voiced.iter().map(|m| m.f1).collect()),
            median(voiced.iter().map(|m| m.front).collect()),
            shares.join("  ")
        );
    }
    Ok(())
}
