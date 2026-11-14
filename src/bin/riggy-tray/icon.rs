//! The tray icon, drawn from SVG at startup: a head and shoulders, with sound waves while speech is on. Pixmaps instead of a theme icon name keep the colours, which KDE would otherwise recolour.

use ksni::Icon;
use resvg::{tiny_skia, usvg};

const SIZES: [u32; 6] = [16, 22, 24, 32, 48, 64];
const SPEAKING: &str = "#3daee9";
const SILENT: &str = "#8c8c8c";

pub struct Icons {
    pub speaking: Vec<Icon>,
    pub silent: Vec<Icon>,
}

impl Icons {
    pub fn draw() -> Self {
        Self { speaking: rasterise(&svg(SPEAKING, true)), silent: rasterise(&svg(SILENT, false)) }
    }
}

fn svg(colour: &str, waves: bool) -> String {
    let waves = if waves {
        format!(r#"<g fill="none" stroke="{colour}" stroke-width="5" stroke-linecap="round"><path d="M45 13 Q51 22 45 31"/><path d="M53 6 Q63 22 53 38"/></g>"#)
    } else {
        String::new()
    };
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><circle cx="25" cy="21" r="13" fill="{colour}"/><path d="M3 62 C3 45 12 38 25 38 C38 38 47 45 47 62 Z" fill="{colour}"/>{waves}</svg>"#)
}

fn rasterise(svg: &str) -> Vec<Icon> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).expect("the tray icon is valid SVG");
    SIZES
        .iter()
        .filter_map(|&size| {
            let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
            let scale = size as f32 / tree.size().width();
            resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
            Some(Icon { width: size as i32, height: size as i32, data: argb(pixmap.data()) })
        })
        .collect()
}

/// tiny-skia's premultiplied RGBA as the StatusNotifierItem pixmap format: ARGB, not premultiplied, in network byte order.
fn argb(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|pixel| {
            let [r, g, b, a] = [pixel[0], pixel[1], pixel[2], pixel[3]];
            let straight = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
            [a, straight(r), straight(g), straight(b)]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_icons_have_visible_pixels_at_every_size() {
        let icons = Icons::draw();
        for set in [&icons.speaking, &icons.silent] {
            assert_eq!(set.len(), SIZES.len());
            for icon in set {
                assert_eq!(icon.data.len(), (icon.width * icon.height * 4) as usize);
                assert!(icon.data.chunks_exact(4).any(|pixel| pixel[0] == 255));
            }
        }
    }

    #[test]
    fn argb_undoes_premultiplication() {
        assert_eq!(argb(&[64, 0, 32, 128]), [128, 128, 0, 64]);
        assert_eq!(argb(&[0, 0, 0, 0]), [0, 0, 0, 0]);
    }
}
