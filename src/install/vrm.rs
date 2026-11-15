//! Checks a VRM file before it is installed: riggy's `vrm` backend reads VRM 1.0 only.

use crate::model_edit::gltf::Glb;

/// Accepts a VRM 1.0 file; explains how to convert a VRM 0.x one.
pub fn check(bytes: &[u8]) -> Result<(), String> {
    let extensions = &Glb::parse(bytes)?.gltf["extensions"];
    if extensions.get("VRMC_vrm").is_some() {
        Ok(())
    } else if extensions.get("VRM").is_some() {
        Err("this is a VRM 0.x file; riggy reads VRM 1.0. Convert it with the VRM Add-on for Blender: import it, set its VRM version to 1.0 and export it again (see doc/avatars.md)".into())
    } else {
        Err("not a VRM file (no VRMC_vrm extension)".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glb(json: &str) -> Vec<u8> {
        let mut text = json.as_bytes().to_vec();
        text.resize(text.len().next_multiple_of(4), b' ');
        let mut bytes = Vec::new();
        for word in [0x4654_6C67_u32, 2, 20 + text.len() as u32, text.len() as u32, 0x4E4F_534A] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&text);
        bytes
    }

    #[test]
    fn vrm_1_passes_and_vrm_0_is_refused() {
        assert!(check(&glb(r#"{"extensions":{"VRMC_vrm":{"specVersion":"1.0"}}}"#)).is_ok());
        assert!(check(&glb(r#"{"extensions":{"VRM":{}}}"#)).unwrap_err().contains("VRM 0.x"));
        assert!(check(&glb(r#"{"asset":{}}"#)).is_err());
    }
}
