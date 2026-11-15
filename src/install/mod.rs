//! `riggy install FILE`: copies a downloaded model to where riggy or the speech server looks for it.
//!
//! - `*.gguf`: a speech model, into the speech server's model folder (`riggy::speech_server::models_dir`).
//! - `*.glb`: an avatar, into `~/.local/share/riggy/models/`, named after the file. A model whose bone nodes were flattened gets its rest pose back on the way (see [`rest_pose`]).
//! - `*.glb` without a skeleton right after an avatar: clothing of that avatar, into its own folder (mapping file: `[[clothing]]`).
//! - `*.vrm`: a VRM 1.0 avatar, into `~/.local/share/riggy/models/`; a VRM 0.x file is refused with a hint to convert it.
//! - `*.fbx`: an animation clip, converted to a GLB of its skeleton and animation (see [`fbx`]), into `~/.local/share/riggy/animations/<pack>/assets/`, named after the file, the pack after its folder; the pack's bone map goes into `skeletons/` next to it where riggy knows the skeleton.
//! - `*.png`, `*.jpg`: a texture of the avatar installed before it, into that model's own folder (`riggy::files::model_textures`), for a model whose file comes without its textures.

mod fbx;
mod rest_pose;
mod vrm;

use std::path::{Path, PathBuf};

use std::collections::HashSet;

use crate::{clips, files, speech_server};

/// What [`install`] put where.
pub enum Installed {
    SpeechModel { path: PathBuf },
    Avatar { name: String, path: PathBuf, rebuilt_joints: Option<usize> },
    Texture { avatar: String, path: PathBuf },
    Clothing { avatar: String, path: PathBuf },
    Animation { pack: String, name: String, path: PathBuf, skeleton: SkeletonMap },
}

/// Whether an installed clip's pack has a bone map that fits it.
pub enum SkeletonMap {
    /// One of the pack's maps fits.
    Fits,
    /// None did; riggy's default map for this skeleton was added as this file.
    Added(PathBuf),
    /// None did and riggy knows no map for this skeleton; one has to be written into this folder.
    Missing(PathBuf),
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
            let path = files::models_dir().join(file_name);
            write(&path, bytes)?;
            Ok(Installed::Avatar { name, path, rebuilt_joints: None })
        }
        Some("fbx") => {
            let bytes = std::fs::read(file).map_err(|err| format!("{}: {err}", file.display()))?;
            let name = file.file_stem().and_then(|s| s.to_str()).ok_or("the file name is not valid UTF-8")?.to_owned();
            let pack = pack_of(file)?;
            let glb = fbx::animation_glb(&bytes, &name).map_err(|err| format!("{}: {err}", file.display()))?;
            let skeleton = skeleton_for(&pack, &glb)?;
            let path = files::animation(&pack, &name);
            write(&path, glb)?;
            Ok(Installed::Animation { pack, name, path, skeleton })
        }
        Some("png" | "jpg" | "jpeg") => {
            let avatar = avatar.ok_or_else(|| format!("{}: a texture belongs to an avatar; name the avatar's .glb before it", file.display()))?;
            let path = files::model_textures(avatar).join(file_name);
            copy(file, &path)?;
            Ok(Installed::Texture { avatar: avatar.to_owned(), path })
        }
        _ => Err(format!("{}: riggy installs speech models (.gguf), avatars (.glb, .vrm), their textures (.png, .jpg) and animation clips (.fbx)", file.display())),
    }
}

/// The pack a clip file belongs to: its folder, or the folder above when that one is named like `Animations` or `fbx`.
fn pack_of(file: &Path) -> Result<String, String> {
    let file = std::path::absolute(file).map_err(|err| format!("{}: {err}", file.display()))?;
    let mut dirs = file.ancestors().skip(1).filter_map(|d| d.file_name()).filter_map(|n| n.to_str());
    let generic = |name: &str| matches!(name.to_lowercase().as_str(), "animations" | "animation" | "anims" | "fbx" | "clips");
    let name = dirs.find(|name| !generic(name)).ok_or_else(|| format!("{}: no folder to name its pack after", file.display()))?;
    Ok(clips::pack_name(name))
}

/// Makes sure `pack` has a bone map that fits the clip `glb`, adding riggy's default one where it knows the skeleton.
fn skeleton_for(pack: &str, glb: &[u8]) -> Result<SkeletonMap, String> {
    let glb = crate::model_edit::gltf::Glb::parse(glb)?;
    let nodes: HashSet<&str> = glb.gltf["nodes"].as_array().into_iter().flatten().filter_map(|n| n["name"].as_str()).collect();
    let dir = files::animation_pack(pack).join("skeletons");
    if clips::Skeleton::for_nodes(&dir, &nodes).is_some() {
        return Ok(SkeletonMap::Fits);
    }
    match clips::Skeleton::default_for(&nodes) {
        Some((name, text)) => {
            let path = dir.join(format!("{name}.toml"));
            write(&path, text.as_bytes().to_vec())?;
            Ok(SkeletonMap::Added(path))
        }
        None => Ok(SkeletonMap::Missing(dir)),
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
