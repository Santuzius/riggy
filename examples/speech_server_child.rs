//! Starts the speech server as a child (as the tray does without a systemd unit), waits until it answers, and stops it.
//!
//! ```text
//! PATH=/path/to/qwentts/bin:$PATH cargo run --example speech_server_child
//! ```

use std::time::{Duration, Instant};

use riggy::speech_server::{self, Runner};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut runner = Runner::Child { child: None, started_at: None };
    runner.start()?;
    let started = Instant::now();
    while !speech_server::up(&speech_server::url()) {
        if started.elapsed() > Duration::from_secs(120) {
            runner.stop()?;
            return Err("the server did not answer within 120 s".into());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    println!("answers after {:.1} s", started.elapsed().as_secs_f32());
    runner.stop()?;
    Ok(())
}
