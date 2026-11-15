//! The command line of `riggy`: without arguments it shows the avatar; `riggy install FILE ...` installs models, `riggy say TEXT` speaks a text.

use std::path::PathBuf;
use std::process::ExitCode;

use riggy::install::{self, Installed, SkeletonMap};
use riggy::speaker::{self, Speech};
use riggy::{settings, speech_server};

const USAGE: &str = "\
Usage:
  riggy                                   show the avatar (the tray icon riggy-tray does this for you)
  riggy install FILE ...                  install speech models (.gguf) and avatars (.glb, VRM 1.0 .vrm), each avatar followed by its textures (.png, .jpg) if its file has none and its clothing (.glb without a skeleton), and animation clips (.fbx)
  riggy say [--language LANGUAGE] TEXT    speak TEXT (LANGUAGE: English, German, auto; default English)";

/// What the command line asks for.
pub enum Invocation {
    Avatar,
    Install(Vec<PathBuf>),
    Say { language: String, text: String },
    Help,
}

/// The invocation `args` (without the program name) asks for.
pub fn invocation(args: impl IntoIterator<Item = String>) -> Result<Invocation, String> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else { return Ok(Invocation::Avatar) };
    match command.as_str() {
        "install" => {
            let files: Vec<PathBuf> = args.map(PathBuf::from).collect();
            if files.is_empty() { Err("install needs a file".into()) } else { Ok(Invocation::Install(files)) }
        }
        "say" => {
            let mut language = "English".to_string();
            let mut words = Vec::new();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--language" | "-l" => language = args.next().ok_or("--language needs a value")?,
                    _ => words.push(arg),
                }
            }
            let text = words.join(" ");
            if text.trim().is_empty() { Err("say needs a text".into()) } else { Ok(Invocation::Say { language, text }) }
        }
        "help" | "--help" | "-h" => Ok(Invocation::Help),
        other => Err(format!("unknown command {other:?}")),
    }
}

pub fn usage() -> ExitCode {
    println!("{USAGE}");
    ExitCode::SUCCESS
}

pub fn usage_error(message: &str) -> ExitCode {
    eprintln!("riggy: {message}\n\n{USAGE}");
    ExitCode::from(2)
}

/// Installs every file and restarts the speech server if it got a model.
pub fn install(files: &[PathBuf]) -> ExitCode {
    let mut failed = false;
    let mut speech_models = false;
    let mut avatar = None;
    for file in files {
        match install::install(file, avatar.as_deref()) {
            Ok(Installed::SpeechModel { path }) => {
                println!("Installed the speech model {}", path.display());
                speech_models = true;
            }
            Ok(Installed::Avatar { name, path, rebuilt_joints }) => {
                println!("Installed the avatar {name:?} as {}", path.display());
                if let Some(joints) = rebuilt_joints {
                    println!("  Its {joints} bones got their rest pose back, so they can move.");
                }
                avatar = Some(name);
            }
            Ok(Installed::Texture { avatar, path }) => println!("Installed a texture of {avatar:?} as {}", path.display()),
            Ok(Installed::Clothing { avatar, path }) => println!("Installed clothing of {avatar:?} as {}; name it in the mapping file under [[clothing]]", path.display()),
            Ok(Installed::Animation { pack, name, path, skeleton }) => {
                println!("Installed the animation {name:?} of {pack:?} as {}", path.display());
                match skeleton {
                    SkeletonMap::Fits => {}
                    SkeletonMap::Added(path) => println!("  Added the bone map {}", path.display()),
                    SkeletonMap::Missing(dir) => println!("  riggy does not know this skeleton; write a bone map for it into {} (see doc/animations.md)", dir.display()),
                }
            }
            Ok(Installed::UnityPack { pack, dir, clips, skipped, skeleton }) => {
                println!("Installed {} animations of {pack:?} into {}: {}", clips.len(), dir.display(), clips.join(", "));
                for asset in skipped {
                    println!("  Skipped {asset}");
                }
                if let SkeletonMap::Added(path) = skeleton {
                    println!("  Added the bone map {}", path.display());
                }
            }
            Err(err) => {
                eprintln!("riggy: {err}");
                failed = true;
            }
        }
    }
    if speech_models && !settings::speech_off() {
        if !speech_server::unit_installed() {
            println!("The tray starts the speech server with the new models.");
        } else {
            match speech_server::control_unit("restart") {
                Ok(()) => println!("Restarting the speech server with the new models."),
                Err(err) => eprintln!("riggy: cannot restart the speech server: {err}"),
            }
        }
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

/// Speaks `text` and returns once it has been played.
pub fn say(language: String, text: String) -> ExitCode {
    if settings::speech_off() {
        eprintln!("riggy: speech is switched off in the tray menu");
        return ExitCode::FAILURE;
    }
    let url = speech_server::url();
    if !speech_server::up(&url) {
        eprintln!("riggy: the speech server does not answer at {url}. It may still be loading; otherwise check that its models are installed (riggy install).");
        return ExitCode::FAILURE;
    }
    match speaker::say(&Speech { text, language }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("riggy: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Result<Invocation, String> {
        invocation(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn say_joins_the_words_and_takes_a_language() {
        let Ok(Invocation::Say { language, text }) = parsed(&["say", "--language", "German", "Hallo", "Welt"]) else { panic!() };
        assert_eq!((language.as_str(), text.as_str()), ("German", "Hallo Welt"));
    }

    #[test]
    fn no_arguments_show_the_avatar_and_unknown_ones_fail() {
        assert!(matches!(parsed(&[]), Ok(Invocation::Avatar)));
        assert!(parsed(&["dance"]).is_err());
        assert!(parsed(&["install"]).is_err());
    }
}
