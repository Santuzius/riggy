//! The avatar process: started by the tray as a child, or found running already (started by hand), and reached through its control socket.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use riggy::control::{self, Command as AvatarCommand};

/// How long the avatar may take from start to listening: loading the model takes a few seconds.
const START_TIME: Duration = Duration::from_secs(120);
/// How long the avatar gets to close after `quit` before it is killed.
const QUIT_TIME: Duration = Duration::from_secs(5);

pub struct Avatar {
    child: Option<Child>,
    socket: PathBuf,
    quit_at: Option<Instant>,
}

impl Avatar {
    pub fn new() -> Self {
        Self { child: None, socket: control::control_socket(&control::default_speech_socket()), quit_at: None }
    }

    /// Whether an avatar runs, ours or one started by hand; reaps ours once it has exited, and kills it if it ignored `quit`.
    pub fn running(&mut self) -> bool {
        if let Some(child) = &mut self.child {
            if self.quit_at.is_some_and(|t| t.elapsed() > QUIT_TIME) {
                let _ = child.kill();
            }
            match child.try_wait() {
                Ok(None) => return true,
                _ => {
                    self.child = None;
                    self.quit_at = None;
                }
            }
        }
        UnixStream::connect(&self.socket).is_ok()
    }

    /// Starts the avatar on the desktop (the `riggy` program next to this one) and hands it `commands()` once it listens; taken then, they include changes made while it loads.
    pub fn start(&mut self, commands: impl Fn() -> Vec<AvatarCommand> + Send + 'static) -> io::Result<()> {
        let program = std::env::current_exe()?.with_file_name("riggy");
        self.child = Some(Command::new(program).env("RIGGY_DESKTOP", "1").spawn()?);
        self.quit_at = None;
        let socket = self.socket.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            while started.elapsed() < START_TIME {
                if control::send(&socket, &commands()).is_ok() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            eprintln!("riggy-tray: the avatar did not open {} in time", socket.display());
        });
        Ok(())
    }

    pub fn stop(&mut self) {
        self.send(&[AvatarCommand::Quit]);
        self.quit_at = Some(Instant::now());
    }

    /// Sends commands if an avatar listens; one that is still starting gets the settings when it is ready.
    pub fn send(&self, commands: &[AvatarCommand]) {
        let _ = control::send(&self.socket, commands);
    }
}
