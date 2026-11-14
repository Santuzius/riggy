//! The speech server: [qwentts.cpp](https://github.com/ServeurpersoCom/qwentts.cpp) with Qwen3-TTS, an OpenAI-compatible HTTP server, run as the systemd user unit [`UNIT`] by qwentts-nix's NixOS module.

use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// The speech server's systemd user unit.
pub const UNIT: &str = "qwentts";

/// The server's address: `TTS_URL`, by default `http://127.0.0.1:8902`.
pub fn url() -> String {
    std::env::var("TTS_URL").unwrap_or_else(|_| "http://127.0.0.1:8902".into())
}

/// The folder the unit loads its models from, `~/.local/share/qwentts/models`.
pub fn models_dir() -> PathBuf {
    crate::files::xdg_data_home().join("qwentts").join("models")
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).build().into()
}

/// Whether the server answers within a second.
pub fn up(url: &str) -> bool {
    agent(Duration::from_secs(1)).get(format!("{url}/health")).call().is_ok()
}

/// The server's voices; empty if it does not answer.
pub fn voices(url: &str) -> Vec<String> {
    let Ok(mut response) = agent(Duration::from_secs(1)).get(format!("{url}/v1/audio/voices")).call() else {
        return Vec::new();
    };
    let Ok(body) = response.body_mut().read_json::<serde_json::Value>() else {
        return Vec::new();
    };
    // Accept {"voices": [{"name": …}]} and plain lists of names or objects.
    let list = body.get("voices").unwrap_or(&body);
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|voice| voice.as_str().or_else(|| voice["name"].as_str()).map(str::to_owned))
        .collect()
}

/// Asks systemd to start, stop or restart the unit (`action`) without waiting: loading the model takes a while.
pub fn control(action: &str) -> io::Result<()> {
    let status = Command::new("systemctl").args(["--user", "--no-block", action, UNIT]).status()?;
    if status.success() { Ok(()) } else { Err(io::Error::other(format!("systemctl {action} {UNIT}: {status}"))) }
}
