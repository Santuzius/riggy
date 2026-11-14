//! Speaks texts with the speech server, in the voice and tempo chosen in the tray (`riggy::settings`): [`Player`] queues them for `riggy-mcp` and plays one after another, each to its end, with [`GAP`] of silence in between; [`say`] speaks one at once for `riggy say`.
//!
//! Audio is streamed: server PCM → ffmpeg (tempo) → the avatar's speech socket if the avatar runs, otherwise `pw-play`. Both programs must be on `PATH`.

use std::error::Error;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::settings::{self, Settings};
use serde_json::json;

const SAMPLE_RATE: u32 = 24_000;
const BYTES_PER_SECOND: f32 = 2.0 * SAMPLE_RATE as f32;
/// The avatar holds back the start of playback until its lip sync leads, by 0.2 to 0.3 s.
const AVATAR_DELAY: Duration = Duration::from_millis(500);
const GAP: Duration = Duration::from_secs(3);

/// A text to speak.
pub struct Speech {
    pub text: String,
    pub language: String,
}

pub struct Player {
    queue: Sender<Speech>,
    playing: Arc<Mutex<Playing>>,
}

/// The processes of the text playing now, so that [`Player::stop`] can end them.
#[derive(Default)]
struct Playing {
    stopped: bool,
    pids: Vec<u32>,
}

impl Player {
    pub fn start() -> Self {
        let (queue, texts) = mpsc::channel();
        let playing = Arc::new(Mutex::new(Playing::default()));
        let worker = Worker { tts_url: crate::speech_server::url(), speech_socket: speech_socket(), playing: playing.clone() };
        std::thread::spawn(move || worker.run(texts));
        Self { queue, playing }
    }

    pub fn play(&self, speech: Speech) {
        let _ = self.queue.send(speech);
    }

    /// Ends playback for good, at server exit.
    pub fn stop(&self) {
        let mut playing = self.playing.lock().unwrap();
        playing.stopped = true;
        for pid in playing.pids.drain(..) {
            // SAFETY: kill only sends a signal. The pids are cleared right after the children are reaped; in that short window the kernel does not hand the pid to a new process.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        }
    }
}

/// Speaks `speech` now and returns when it has been played.
pub fn say(speech: &Speech) -> Result<(), Box<dyn Error>> {
    let worker = Worker { tts_url: crate::speech_server::url(), speech_socket: speech_socket(), playing: Arc::default() };
    worker.speak(speech)
}

/// `RIGGY_SOCKET`, or the avatar's default speech socket.
fn speech_socket() -> PathBuf {
    std::env::var_os("RIGGY_SOCKET").map(PathBuf::from).unwrap_or_else(crate::control::default_speech_socket)
}

struct Worker {
    tts_url: String,
    speech_socket: PathBuf,
    playing: Arc<Mutex<Playing>>,
}

impl Worker {
    fn run(&self, texts: Receiver<Speech>) {
        let mut ready_at = Instant::now();
        for speech in texts {
            std::thread::sleep(ready_at.saturating_duration_since(Instant::now()));
            // Texts still waiting when the user switches speech off are dropped, not kept for later.
            if self.playing.lock().unwrap().stopped || settings::speech_off() {
                continue;
            }
            if let Err(error) = self.speak(&speech)
                && !self.playing.lock().unwrap().stopped
            {
                eprintln!("riggy-mcp: playback failed: {error}");
            }
            ready_at = Instant::now() + GAP;
        }
    }

    fn speak(&self, speech: &Speech) -> Result<(), Box<dyn Error>> {
        let settings = Settings::load();
        let body = json!({
            "voice": settings.voice,
            "instructions": "Speak in a calm, neutral, matter-of-fact tone. Do not laugh.",
            "temperature": 0.6,
            "subtalker_temperature": 0.6,
            "seed": 2,
            "input": speech.text,
            "language": speech.language,
            "response_format": "pcm",
        });
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(60))).build().into();
        let response = agent.post(format!("{}/v1/audio/speech", self.tts_url)).send_json(body)?;
        let mut audio = response.into_body().into_reader();

        let mut avatar = UnixStream::connect(&self.speech_socket).ok();
        if let Some(avatar) = &mut avatar {
            // The avatar's protocol: one JSON header line, then raw PCM until the connection closes.
            writeln!(avatar, "{}", json!({ "text": speech.text, "language": speech.language, "rate": SAMPLE_RATE }))?;
        }
        let mut chain = Chain::start(settings.tempo, avatar.as_ref())?;
        if !self.register(&chain) {
            return Ok(());
        }
        let result = (|| -> Result<(Option<Instant>, usize), Box<dyn Error>> {
            let mut started = None;
            let mut received = 0;
            let mut buffer = [0u8; 4800];
            let input = chain.input.as_mut().expect("ffmpeg stdin");
            loop {
                let n = audio.read(&mut buffer)?;
                if n == 0 {
                    return Ok((started, received));
                }
                started.get_or_insert_with(Instant::now);
                received += n;
                input.write_all(&buffer[..n])?;
            }
        })();
        let finished = chain.finish();
        self.playing.lock().unwrap().pids.clear();
        let (started, received) = result?;
        finished?;
        if let (Some(avatar), Some(started)) = (avatar, started) {
            drop(avatar);
            // The avatar plays at its own pace; wait for the estimated end, so the gap before the next text stays.
            let end = started + AVATAR_DELAY + Duration::from_secs_f32(received as f32 / BYTES_PER_SECOND / settings.tempo);
            std::thread::sleep(end.saturating_duration_since(Instant::now()));
        }
        Ok(())
    }

    /// Records the chain's processes for [`Player::stop`]; false (and the chain killed) if the player has stopped already.
    fn register(&self, chain: &Chain) -> bool {
        let mut playing = self.playing.lock().unwrap();
        if playing.stopped {
            chain.kill();
            return false;
        }
        playing.pids = chain.pids();
        true
    }
}

/// ffmpeg changing the tempo, and `pw-play` after it unless ffmpeg writes straight into the avatar's socket.
struct Chain {
    input: Option<ChildStdin>,
    ffmpeg: Child,
    pw_play: Option<Child>,
}

impl Chain {
    fn start(tempo: f32, avatar: Option<&UnixStream>) -> Result<Self, Box<dyn Error>> {
        let rate = SAMPLE_RATE.to_string();
        let raw = ["-f", "s16le", "-ar", &rate, "-ac", "1"];
        let output = match avatar {
            Some(avatar) => Stdio::from(OwnedFd::from(avatar.try_clone()?)),
            None => Stdio::piped(),
        };
        let mut ffmpeg = Command::new("ffmpeg")
            .args(["-loglevel", "error"])
            .args(raw)
            .args(["-i", "-", "-filter:a", &format!("atempo={tempo}")])
            .args(raw)
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(output)
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| format!("ffmpeg: {err}"))?;
        let pw_play = match ffmpeg.stdout.take() {
            Some(sound) => Some(
                Command::new("pw-play")
                    .args(["--raw", "--rate", &rate, "--channels", "1", "--format", "s16", "-"])
                    .stdin(sound)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|err| {
                        let _ = ffmpeg.kill();
                        let _ = ffmpeg.wait();
                        format!("pw-play: {err}")
                    })?,
            ),
            None => None,
        };
        Ok(Self { input: ffmpeg.stdin.take(), ffmpeg, pw_play })
    }

    fn pids(&self) -> Vec<u32> {
        std::iter::once(&self.ffmpeg).chain(&self.pw_play).map(Child::id).collect()
    }

    fn kill(&self) {
        for pid in self.pids() {
            // SAFETY: kill only sends a signal, and these children have not been reaped yet (that happens in `finish`).
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        }
    }

    /// Closes ffmpeg's input and waits until the sound has left the chain.
    fn finish(mut self) -> std::io::Result<()> {
        drop(self.input.take());
        self.ffmpeg.wait()?;
        if let Some(pw_play) = &mut self.pw_play {
            pw_play.wait()?;
        }
        Ok(())
    }
}
