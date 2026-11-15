//! Edits a VRM model's mapping file asks for, applied to the model as riggy loads it (`avatar::vrm`), so the installed file stays as downloaded and a change to the mapping file shows at once:
//!
//! - `[[cut]]`: removes parts of the model (see [`cut`]);
//! - `[[cloth]]`: hangs spring chains in a garment that has none (see [`cloth`]).
//!
//! [`gltf`] holds the GLB helpers these share with `install`.

pub mod cloth;
pub mod cut;
pub(crate) mod gltf;

/// The VRM file `bytes` with the edits of the mapping file text `mapping`, or `None` if it asks for none.
pub fn edited_vrm(bytes: &[u8], mapping: &str) -> Result<Option<Vec<u8>>, String> {
    let cuts = cut::cut_specs(mapping)?;
    let cloth = cloth::cloth_specs(mapping)?;
    if cuts.is_empty() && cloth.is_empty() {
        return Ok(None);
    }
    let bytes = if cuts.is_empty() { bytes.to_vec() } else { cut::cut(bytes, &cuts)?.0 };
    Ok(Some(if cloth.is_empty() { bytes } else { cloth::add_cloth(&bytes, &cloth)?.unwrap_or(bytes) }))
}
