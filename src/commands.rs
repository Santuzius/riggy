//! Commands from the tray: a listener on the control socket (see `riggy::control`) hands each one to the systems as a [`Received`] message.

use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};

use bevy::prelude::*;
use riggy::control::{self, Command};

use crate::config::Config;

/// A command that arrived on the control socket this frame.
#[derive(Message, Clone, Debug)]
pub struct Received(pub Command);

pub struct CommandsPlugin;

impl Plugin for CommandsPlugin {
    fn build(&self, app: &mut App) {
        let (sender, receiver) = mpsc::channel();
        listen(control::control_socket(&app.world().resource::<Config>().speech.socket_path()), sender);
        app.insert_resource(Inbox(Mutex::new(receiver)))
            .add_message::<Received>()
            .add_systems(PreUpdate, deliver);
    }
}

#[derive(Resource)]
struct Inbox(Mutex<Receiver<Command>>);

fn deliver(inbox: Res<Inbox>, mut received: MessageWriter<Received>) {
    for command in inbox.0.lock().unwrap().try_iter() {
        info!("command: {command:?}");
        received.write(Received(command));
    }
}

fn listen(path: PathBuf, sender: Sender<Command>) {
    if UnixStream::connect(&path).is_ok() {
        warn!("{} is in use by another program; the tray cannot reach this riggy", path.display());
        return;
    }
    // A socket file left over from a crashed run.
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(err) => {
            warn!("cannot listen on {}: {err}; the tray cannot reach this riggy", path.display());
            return;
        }
    };
    info!("control socket: {}", path.display());
    std::thread::Builder::new()
        .name("riggy-control".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    match serde_json::from_str(&line) {
                        Ok(command) => {
                            if sender.send(command).is_err() {
                                return;
                            }
                        }
                        Err(err) => warn!("control socket: {err}: {line}"),
                    }
                }
            }
        })
        .expect("spawn control thread");
}
