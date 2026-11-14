//! The tray as a single process: it holds a lock on `$XDG_RUNTIME_DIR/riggy-tray.lock` while it runs, so a second start finds the first, and `riggy-mcp` can start it when none runs.

use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Held by the running tray; the lock ends with the process.
pub struct TrayLock(#[allow(dead_code)] File);

fn lock_file() -> PathBuf {
    crate::control::runtime_dir().join("riggy-tray.lock")
}

/// Takes the tray's lock; `None` while another tray holds it.
pub fn lock() -> io::Result<Option<TrayLock>> {
    let file = File::create(lock_file())?;
    // SAFETY: flock only reads the descriptor, which `file` keeps open.
    let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    Ok(locked.then_some(TrayLock(file)))
}

/// Whether a tray runs.
pub fn tray_running() -> bool {
    // Taking the lock and dropping it again tells whether another process holds it.
    matches!(lock(), Ok(None))
}

/// Starts the tray (`riggy-tray` next to the running program) in its own process group, so it outlives its starter.
pub fn start_tray() -> io::Result<()> {
    let program = std::env::current_exe()?.with_file_name("riggy-tray");
    Command::new(program).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn()?;
    Ok(())
}
