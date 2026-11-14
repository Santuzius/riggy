//! `riggy install FILE`: copies a downloaded model to where riggy or the speech server looks for it.
//!
//! - `*.gguf`: a speech model, into the speech server's model folder (`riggy::speech_server::models_dir`).
//! - `*.glb`: an avatar, into `~/.local/share/riggy/models/`, named after the file. A model whose bone nodes were flattened gets its rest pose back on the way (see [`rest_pose`]).

mod rest_pose;

use std::path::{Path, PathBuf};

use crate::{files, speech_server};

/// What [`install`] put where.
pub enum Installed {
    SpeechModel { path: PathBuf },
    Avatar { name: String, path: PathBuf, rebuilt_joints: Option<usize> },
}

/// Installs `file` by its extension.
pub fn install(file: &Path) -> Result<Installed, String> {
    let extension = file.extension().and_then(|e| e.to_str()).map(str::to_lowercase);
    let file_name = file.file_name().ok_or("no file name")?;
    match extension.as_deref() {
        Some("gguf") => {
            let path = speech_server::models_dir().join(file_name);
            copy(file, &path)?;
            Ok(Installed::SpeechModel { path })
        }
        Some("glb") => {
            let name = file.file_stem().and_then(|s| s.to_str()).ok_or("the file name is not valid UTF-8")?.to_owned();
            let path = files::model(&name);
            let bytes = std::fs::read(file).map_err(|err| format!("{}: {err}", file.display()))?;
            let fix = rest_pose::rest_pose_fix(&bytes).map_err(|err| format!("{}: {err}", file.display()))?;
            let rebuilt_joints = fix.as_ref().map(|fix| fix.joints);
            write(&path, fix.map_or(bytes, |fix| fix.glb))?;
            Ok(Installed::Avatar { name, path, rebuilt_joints })
        }
        _ => Err(format!("{}: riggy installs speech models (.gguf) and avatars (.glb)", file.display())),
    }
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    write(to, std::fs::read(from).map_err(|err| format!("{}: {err}", from.display()))?)
}

fn write(path: &Path, bytes: Vec<u8>) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    std::fs::write(path, bytes).map_err(|err| format!("{}: {err}", path.display()))
}
