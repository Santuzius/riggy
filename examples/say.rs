//! Test client for riggy's speech socket: speaks a text with the local Qwen3-TTS server, or sends a raw PCM file.
//!
//! ```text
//! cargo run --example say -- [--language English] [--file speech.pcm] [TEXT ...]
//! ```
//!
//! `TTS_URL` sets the server (default `http://127.0.0.1:8902`), `RIGGY_SOCKET` the socket (default `$XDG_RUNTIME_DIR/riggy.sock`). Files are 24 kHz signed 16-bit little-endian mono, the server's `pcm` format.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut language = "English".to_string();
    let mut file = None;
    let mut words = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--language" => language = args.next().ok_or("--language needs a value")?,
            "--file" => file = Some(PathBuf::from(args.next().ok_or("--file needs a path")?)),
            _ => words.push(arg),
        }
    }
    let text = words.join(" ");

    let mut audio: Box<dyn Read> = match &file {
        Some(path) => Box::new(std::fs::File::open(path)?),
        None if text.is_empty() => return Err("give a text or --file".into()),
        None => {
            let url = std::env::var("TTS_URL").unwrap_or_else(|_| "http://127.0.0.1:8902".into());
            let body = serde_json::json!({
                "voice": "serena",
                "instructions": "Speak in a calm, neutral, matter-of-fact tone. Do not laugh.",
                "temperature": 0.6,
                "subtalker_temperature": 0.6,
                "seed": 2,
                "input": text,
                "language": language,
                "response_format": "pcm",
            });
            Box::new(ureq::post(format!("{url}/v1/audio/speech")).send_json(body)?.into_body().into_reader())
        }
    };

    let socket = std::env::var_os("RIGGY_SOCKET").map(PathBuf::from).unwrap_or_else(|| {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        runtime.join("riggy.sock")
    });
    let mut stream = UnixStream::connect(&socket).map_err(|err| format!("{}: {err}", socket.display()))?;
    let header = serde_json::json!({ "text": text, "language": language, "rate": 24000 });
    writeln!(stream, "{header}")?;
    let bytes = std::io::copy(&mut audio, &mut stream)?;
    eprintln!("sent {:.1} s of audio", bytes as f32 / 48_000.0);
    Ok(())
}
