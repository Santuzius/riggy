//! The speech server: [qwentts.cpp](https://github.com/ServeurpersoCom/qwentts.cpp) with Qwen3-TTS, an OpenAI-compatible HTTP server. qwentts-nix's NixOS module runs it as the systemd user unit [`UNIT`]; without that unit (e.g. after `nix profile add`) the tray runs it as its child ([`Runner`]).

use std::io;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// The speech server's systemd user unit.
pub const UNIT: &str = "qwentts";
/// The port riggy expects the server on.
pub const PORT: u16 = 8902;
/// The server program of qwentts-nix's package.
const PROGRAM: &str = "tts-server";
/// The models the server loads, in [`models_dir`].
const TALKER: &str = "qwen-talker-0.6b-customvoice-Q8_0.gguf";
const CODEC: &str = "qwen-tokenizer-12hz-Q8_0.gguf";
/// A child that exited is started again at most this often, so a broken model does not restart it every poll.
const RESTART_DELAY: Duration = Duration::from_secs(30);

/// The server's address: `TTS_URL`, by default `http://127.0.0.1:8902`.
pub fn url() -> String {
    std::env::var("TTS_URL").unwrap_or_else(|_| format!("http://127.0.0.1:{PORT}"))
}

/// The folder the server loads its models from, `~/.local/share/qwentts/models`.
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

/// Whether the systemd user unit [`UNIT`] exists.
pub fn unit_installed() -> bool {
    let output = Command::new("systemctl").args(["--user", "show", "--property=LoadState", "--value", UNIT]).output();
    output.is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "loaded")
}

/// Asks systemd to start, stop or restart the unit (`action`) without waiting: loading the model takes a while.
pub fn control_unit(action: &str) -> io::Result<()> {
    let status = Command::new("systemctl").args(["--user", "--no-block", action, UNIT]).status()?;
    if status.success() { Ok(()) } else { Err(io::Error::other(format!("systemctl {action} {UNIT}: {status}"))) }
}

/// Runs the speech server: through its systemd unit if there is one, otherwise as a child process.
pub enum Runner {
    Unit,
    Child { child: Option<Child>, started_at: Option<Instant> },
}

impl Runner {
    pub fn detect() -> Self {
        if unit_installed() { Self::Unit } else { Self::Child { child: None, started_at: None } }
    }

    pub fn start(&mut self) -> io::Result<()> {
        match self {
            Self::Unit => control_unit("start"),
            Self::Child { child, started_at } => {
                if child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None))) {
                    return Ok(());
                }
                *started_at = Some(Instant::now());
                let [talker, codec] = [TALKER, CODEC].map(|name| models_dir().join(name));
                if !talker.exists() || !codec.exists() {
                    return Err(io::Error::new(io::ErrorKind::NotFound, format!("the speech models are missing in {} (riggy install)", models_dir().display())));
                }
                let mut command = Command::new(PROGRAM);
                command.arg("--model").arg(talker).arg("--codec").arg(codec).args(["--host", "127.0.0.1", "--port", &PORT.to_string()]);
                // SAFETY: prctl is async-signal-safe; the server ends with the tray, even if the tray is killed.
                unsafe {
                    command.pre_exec(|| {
                        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                        Ok(())
                    });
                }
                *child = Some(command.spawn().map_err(|err| io::Error::new(err.kind(), format!("{PROGRAM}: {err}")))?);
                Ok(())
            }
        }
    }

    pub fn stop(&mut self) -> io::Result<()> {
        match self {
            Self::Unit => control_unit("stop"),
            Self::Child { child, .. } => {
                if let Some(mut child) = child.take() {
                    child.kill()?;
                    child.wait()?;
                }
                Ok(())
            }
        }
    }

    /// Starts a child that is not running, e.g. because the models were installed after the tray started; systemd looks after the unit.
    pub fn keep_running(&mut self) -> io::Result<()> {
        match self {
            Self::Child { started_at, .. } if started_at.is_none_or(|t| t.elapsed() > RESTART_DELAY) => self.start(),
            _ => Ok(()),
        }
    }
}
