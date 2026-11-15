//! Parts of a model cut away by `riggy install`: `[[cut]]` in the avatar's mapping file removes the triangles of some materials whose texture coordinates lie in a rectangle, e.g. a painted lining between two layers of a coat. Only a model whose own mapping file has entries is changed.

use serde::Deserialize;
use serde_json::json;

use super::gltf::{Glb, append_accessor, read_accessor};

/// One `[[cut]]` entry.
#[derive(Deserialize, Debug, Clone)]
pub struct CutSpec {
    /// Start of the names of the materials cut.
    pub material: String,
    /// Texture rectangle `[u0, v0, u1, v1]`; a triangle whose texture coordinates have their centre inside goes.
    pub uv: [f64; 4],
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Mapping {
    cut: Vec<CutSpec>,
}

/// The `[[cut]]` entries of a mapping file's text.
pub fn cut_specs(mapping: &str) -> Result<Vec<CutSpec>, String> {
    toml::from_str::<Mapping>(mapping).map(|m| m.cut).map_err(|err| format!("mapping file: {err}"))
}

/// The model `bytes` with the triangles of `specs` removed, and how many went.
pub fn cut(bytes: &[u8], specs: &[CutSpec]) -> Result<(Vec<u8>, usize), String> {
    let mut glb = Glb::parse(bytes)?;
    let materials: Vec<String> = glb.gltf["materials"].as_array().into_iter().flatten().map(|m| m["name"].as_str().unwrap_or("").to_owned()).collect();
    let mut removed = 0;
    for mesh in 0..glb.gltf["meshes"].as_array().map_or(0, Vec::len) {
        for primitive in 0..glb.gltf["meshes"][mesh]["primitives"].as_array().map_or(0, Vec::len) {
            let p = glb.gltf["meshes"][mesh]["primitives"][primitive].clone();
            let name = p["material"].as_u64().and_then(|m| materials.get(m as usize)).map_or("", String::as_str);
            let rects: Vec<[f64; 4]> = specs.iter().filter(|s| name.starts_with(&s.material)).map(|s| s.uv).collect();
            if rects.is_empty() {
                continue;
            }
            let (Some(indices), Some(uvs)) = (p["indices"].as_u64(), p["attributes"]["TEXCOORD_0"].as_u64()) else {
                return Err(format!("cut: material {name} needs indexed triangles with texture coordinates"));
            };
            let indices: Vec<[f64; 1]> = read_accessor(&glb.gltf, &glb.binary, indices as usize)?;
            let uvs: Vec<[f64; 2]> = read_accessor(&glb.gltf, &glb.binary, uvs as usize)?;
            let kept = kept_triangles(&indices.iter().map(|i| i[0] as u32).collect::<Vec<_>>(), &uvs, &rects);
            removed += (indices.len() - kept.len()) / 3;
            let bytes: Vec<u8> = kept.iter().flat_map(|i| i.to_le_bytes()).collect();
            let accessor = append_accessor(&mut glb, &bytes, 5125, "SCALAR", kept.len());
            glb.gltf["meshes"][mesh]["primitives"][primitive]["indices"] = json!(accessor);
        }
    }
    Ok((glb.to_bytes(), removed))
}

/// The triangles of `indices` whose texture-coordinate centre lies in none of `rects`.
fn kept_triangles(indices: &[u32], uvs: &[[f64; 2]], rects: &[[f64; 4]]) -> Vec<u32> {
    let inside = |tri: &[u32]| {
        let (u, v) = tri.iter().fold((0.0, 0.0), |(u, v), &i| (u + uvs[i as usize][0] / 3.0, v + uvs[i as usize][1] / 3.0));
        rects.iter().any(|r| (r[0]..=r[2]).contains(&u) && (r[1]..=r[3]).contains(&v))
    };
    indices.chunks_exact(3).filter(|tri| !inside(tri)).flatten().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_triangle_inside_the_rectangle_goes_and_one_outside_stays() {
        let uvs = [[0.5, 0.5], [0.52, 0.5], [0.5, 0.52], [0.9, 0.9], [0.92, 0.9], [0.9, 0.92]];
        assert_eq!(kept_triangles(&[0, 1, 2, 3, 4, 5], &uvs, &[[0.4, 0.4, 0.6, 0.6]]), vec![3, 4, 5]);
    }
}
