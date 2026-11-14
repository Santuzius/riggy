//! The tray icon, rasterised at startup from `assets/icons/`: a ring, thick while the avatar is shown, around a dot while speech is on.
//!
//! The SVGs carry KDE's `current-color-scheme` classes. Pixmaps instead of theme icon names keep the orange dot, which KDE would otherwise recolour; so each state is drawn twice, with a dark and a light text colour, and [`Icons::pick`] hands out the one that contrasts with the panel.

use ksni::Icon;
use resvg::{tiny_skia, usvg};

const SIZES: [u32; 6] = [16, 22, 24, 32, 48, 64];
/// The text colour in the SVGs, used on light panels.
const DARK_TEXT: &str = "#232629";
/// Breeze Dark's text colour, used on dark panels.
const LIGHT_TEXT: &str = "#fcfcfc";

/// What the icon shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrayState {
    pub speech: bool,
    pub avatar: bool,
}

impl TrayState {
    fn svg(self) -> &'static str {
        match (self.speech, self.avatar) {
            (true, true) => include_str!("../../../assets/icons/tray-tts-avatar.svg"),
            (true, false) => include_str!("../../../assets/icons/tray-tts.svg"),
            (false, true) => include_str!("../../../assets/icons/tray-avatar.svg"),
            (false, false) => include_str!("../../../assets/icons/tray-off.svg"),
        }
    }

    const ALL: [Self; 4] = [
        Self { speech: true, avatar: true },
        Self { speech: true, avatar: false },
        Self { speech: false, avatar: true },
        Self { speech: false, avatar: false },
    ];
}

/// Every state in both colour variants, each at every size in [`SIZES`].
pub struct Icons {
    /// Per state as in [`TrayState::ALL`]: (for light panels, for dark panels).
    variants: Vec<(TrayState, Vec<Icon>, Vec<Icon>)>,
}

impl Icons {
    pub fn draw() -> Self {
        let variants = TrayState::ALL
            .into_iter()
            .map(|state| {
                let svg = state.svg();
                (state, rasterise(&with_plain_colours(svg)), rasterise(&with_plain_colours(&svg.replace(DARK_TEXT, LIGHT_TEXT))))
            })
            .collect();
        Self { variants }
    }

    /// The icon of `state` that contrasts with a dark or light panel.
    pub fn pick(&self, state: TrayState, dark_panel: bool) -> Vec<Icon> {
        let (_, on_light, on_dark) = self.variants.iter().find(|(s, _, _)| *s == state).expect("every state is drawn");
        if dark_panel { on_dark.clone() } else { on_light.clone() }
    }
}

/// The SVG with each `class="NAME" fill="currentColor"` filled in the colour its `.NAME{color:…}` rule in the `<style>` block sets, which resvg does not resolve.
fn with_plain_colours(svg: &str) -> String {
    let style = svg.split_once("<style").and_then(|(_, rest)| rest.split_once("</style>")).map_or("", |(style, _)| style);
    let mut svg = svg.to_owned();
    for rule in style.split('.').skip(1) {
        let Some((class, body)) = rule.split_once('{') else { continue };
        let Some(colour) = body.split_once("color:").map(|(_, value)| value.trim_end_matches(['}', ' ']).split(['}', ';']).next().unwrap_or_default().trim()) else { continue };
        svg = svg.replace(&format!(r#"class="{class}" fill="currentColor""#), &format!(r#"fill="{colour}""#));
    }
    svg
}

fn rasterise(svg: &str) -> Vec<Icon> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).expect("the tray icons are valid SVG");
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
    let (pixels, _) = rgba.as_chunks::<4>();
    pixels
        .iter()
        .flat_map(|&[r, g, b, a]| {
            let straight = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
            [a, straight(r), straight(g), straight(b)]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_has_visible_pixels_at_every_size_in_both_variants() {
        let icons = Icons::draw();
        for state in TrayState::ALL {
            for set in [icons.pick(state, false), icons.pick(state, true)] {
                assert_eq!(set.len(), SIZES.len());
                for icon in set {
                    assert_eq!(icon.data.len(), (icon.width * icon.height * 4) as usize);
                    // A one-unit ring drawn at 16 px covers no pixel fully.
                    assert!(icon.data.as_chunks::<4>().0.iter().any(|pixel| pixel[0] > 100), "{state:?} at {} px is blank", icon.width);
                }
            }
        }
    }

    #[test]
    fn the_dark_panel_variant_is_light() {
        let icons = Icons::draw();
        let state = TrayState { speech: false, avatar: false };
        let brightest = |set: Vec<Icon>| set[0].data.as_chunks::<4>().0.iter().filter(|p| p[0] > 100).map(|p| p[1]).max().unwrap();
        assert!(brightest(icons.pick(state, true)) > 200);
        assert!(brightest(icons.pick(state, false)) < 60);
    }

    #[test]
    fn style_colours_replace_current_color() {
        let svg = r#"<svg><style>.ColorScheme-Text{color:#232629}.ColorScheme-NeutralText{color:#f67400}</style><path class="ColorScheme-Text" fill="currentColor"/><circle class="ColorScheme-NeutralText" fill="currentColor"/></svg>"#;
        let plain = with_plain_colours(svg);
        assert!(plain.contains(r##"<path fill="#232629"/>"##) && plain.contains(r##"<circle fill="#f67400"/>"##), "{plain}");
    }

    #[test]
    fn argb_undoes_premultiplication() {
        assert_eq!(argb(&[64, 0, 32, 128]), [128, 128, 0, 64]);
        assert_eq!(argb(&[0, 0, 0, 0]), [0, 0, 0, 0]);
    }
}
