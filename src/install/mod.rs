//! `riggy install FILE`: copies a downloaded model to where riggy or the speech server looks for it.
//!
//! - `*.gguf`: a speech model, into the speech server's model folder (`riggy::speech_server::models_dir`).
//! - `*.glb`: an avatar, into `~/.local/share/riggy/models/`, named after the file. A model whose bone nodes were flattened gets its rest pose back on the way (see [`rest_pose`]).
//! - `*.glb` without a skeleton right after an avatar: clothing of that avatar, into its own folder (mapping file: `[[clothing]]`).
//! - `*.vrm`: a VRM 1.0 avatar, into `~/.local/share/riggy/models/`; a VRM 0.x file is refused with a hint to convert it. Its mapping file's `[[cut]]` removes parts of it (see [`cut`]), its `[[cloth]]` adds spring chains to a garment that has none (see [`cloth`]).
//! - `*.png`, `*.jpg`: a texture of the avatar installed before it, into that model's own folder (`riggy::files::model_textures`), for a model whose file comes without its textures.

mod cloth;
mod cut;
mod gltf;
mod rest_pose;
mod vrm;

use std::path::{Path, PathBuf};

use crate::{files, speech_server};

/// What [`install`] put where.
pub enum Installed {
    SpeechModel { path: PathBuf },
    Avatar { name: String, path: PathBuf, rebuilt_joints: Option<usize> },
    Texture { avatar: String, path: PathBuf },
    Clothing { avatar: String, path: PathBuf },
}

/// Installs `file` by its extension; a texture, or a GLB without a skeleton (clothing), goes to `avatar`, the avatar installed before it.
pub fn install(file: &Path, avatar: Option<&str>) -> Result<Installed, String> {
    let extension = file.extension().and_then(|e| e.to_str()).map(str::to_lowercase);
    let file_name = file.file_name().ok_or("no file name")?;
    match extension.as_deref() {
        Some("gguf") => {
            let path = speech_server::models_dir().join(file_name);
            copy(file, &path)?;
            Ok(Installed::SpeechModel { path })
        }
        Some("glb") => {
            let bytes = std::fs::read(file).map_err(|err| format!("{}: {err}", file.display()))?;
            if let Some(avatar) = avatar
                && !rest_pose::has_skeleton(&bytes).map_err(|err| format!("{}: {err}", file.display()))?
            {
                let path = files::model_textures(avatar).join(file_name);
                write(&path, bytes)?;
                return Ok(Installed::Clothing { avatar: avatar.to_owned(), path });
            }
            let name = file.file_stem().and_then(|s| s.to_str()).ok_or("the file name is not valid UTF-8")?.to_owned();
            let path = files::model(&name);
            let fix = rest_pose::rest_pose_fix(&bytes).map_err(|err| format!("{}: {err}", file.display()))?;
            let rebuilt_joints = fix.as_ref().map(|fix| fix.joints);
            write(&path, fix.map_or(bytes, |fix| fix.glb))?;
            Ok(Installed::Avatar { name, path, rebuilt_joints })
        }
        Some("vrm") => {
            let bytes = std::fs::read(file).map_err(|err| format!("{}: {err}", file.display()))?;
            vrm::check(&bytes).map_err(|err| format!("{}: {err}", file.display()))?;
            let name = file.file_stem().and_then(|s| s.to_str()).ok_or("the file name is not valid UTF-8")?.to_owned();
            let mapping = files::mapping_text(&name).unwrap_or_default();
            let cuts = cut::cut_specs(&mapping)?;
            let bytes = if cuts.is_empty() { bytes } else { cut::cut(&bytes, &cuts).map_err(|err| format!("{}: {err}", file.display()))?.0 };
            let specs = cloth::cloth_specs(&mapping)?;
            let bytes = if specs.is_empty() { bytes } else { cloth::add_cloth(&bytes, &specs).map_err(|err| format!("{}: {err}", file.display()))?.unwrap_or(bytes) };
            let path = files::models_dir().join(file_name);
            write(&path, bytes)?;
            Ok(Installed::Avatar { name, path, rebuilt_joints: None })
        }
        Some("png" | "jpg" | "jpeg") => {
            let avatar = avatar.ok_or_else(|| format!("{}: a texture belongs to an avatar; name the avatar's .glb before it", file.display()))?;
            let path = files::model_textures(avatar).join(file_name);
            copy(file, &path)?;
            Ok(Installed::Texture { avatar: avatar.to_owned(), path })
        }
        _ => Err(format!("{}: riggy installs speech models (.gguf), avatars (.glb, .vrm) and their textures (.png, .jpg)", file.display())),
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
