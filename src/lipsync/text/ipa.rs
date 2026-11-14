//! eSpeak NG's IPA phoneme names → Rhubarb's phones (an Arpabet subset).
//!
//! Rhubarb's rules only know English sounds; other languages map by the lip shape, which is what shows: the German rounded front vowels ü, ö keep the lips of u, o (the tongue position that makes them sound different is invisible), ä is the open e.

use super::super::rhubarb_animation::Phone;

/// Multi-letter names first, so that the longest match wins.
const TABLE: &[(&str, &[Phone])] = &[
    // Diphthongs
    ("aɪ", &[Phone::AY]),
    ("aʊ", &[Phone::AW]),
    ("eɪ", &[Phone::EY]),
    ("oʊ", &[Phone::OW]),
    ("əʊ", &[Phone::OW]),
    ("ɔɪ", &[Phone::OY]),
    ("ɔʏ", &[Phone::OY]),
    ("ɔø", &[Phone::OY]),
    ("ɪə", &[Phone::IH, Phone::Schwa]),
    ("eə", &[Phone::EH]),
    ("ʊə", &[Phone::UH]),
    // Affricates
    ("tʃ", &[Phone::CH]),
    ("dʒ", &[Phone::JH]),
    ("ts", &[Phone::T, Phone::S]),
    ("pf", &[Phone::P, Phone::F]),
    // Vowels
    ("i", &[Phone::IY]),
    ("ɪ", &[Phone::IH]),
    ("ᵻ", &[Phone::IH]),
    ("ɨ", &[Phone::IH]),
    ("e", &[Phone::IH]),
    ("ɛ", &[Phone::EH]),
    ("æ", &[Phone::AE]),
    ("a", &[Phone::AA]),
    ("ɑ", &[Phone::AA]),
    ("ɐ", &[Phone::AH]),
    ("ʌ", &[Phone::AH]),
    ("ɒ", &[Phone::AO]),
    ("ɔ", &[Phone::AO]),
    ("o", &[Phone::AO]),
    ("ø", &[Phone::AO]),
    ("œ", &[Phone::AO]),
    ("u", &[Phone::UW]),
    ("y", &[Phone::UW]),
    ("ʊ", &[Phone::UH]),
    ("ʏ", &[Phone::UH]),
    ("ɵ", &[Phone::UH]),
    ("ə", &[Phone::Schwa]),
    ("ɘ", &[Phone::Schwa]),
    ("ɜ", &[Phone::ER]),
    ("ɝ", &[Phone::ER]),
    ("ɚ", &[Phone::ER]),
    // Stops
    ("p", &[Phone::P]),
    ("b", &[Phone::B]),
    ("t", &[Phone::T]),
    ("d", &[Phone::D]),
    ("ɾ", &[Phone::T]),
    ("k", &[Phone::K]),
    ("q", &[Phone::K]),
    ("g", &[Phone::G]),
    ("ɡ", &[Phone::G]),
    // Fricatives
    ("f", &[Phone::F]),
    ("v", &[Phone::V]),
    ("β", &[Phone::V]),
    ("θ", &[Phone::TH]),
    ("ð", &[Phone::DH]),
    ("s", &[Phone::S]),
    ("z", &[Phone::Z]),
    ("ʃ", &[Phone::SH]),
    ("ʒ", &[Phone::ZH]),
    ("ç", &[Phone::SH]),
    ("x", &[Phone::HH]),
    ("χ", &[Phone::HH]),
    ("ɣ", &[Phone::G]),
    ("h", &[Phone::HH]),
    ("ɦ", &[Phone::HH]),
    // Nasals
    ("m", &[Phone::M]),
    ("ɱ", &[Phone::M]),
    ("n", &[Phone::N]),
    ("ɲ", &[Phone::N]),
    ("ŋ", &[Phone::NG]),
    // Liquids and semivowels
    ("l", &[Phone::L]),
    ("ɫ", &[Phone::L]),
    ("ʎ", &[Phone::L]),
    ("r", &[Phone::R]),
    ("ɹ", &[Phone::R]),
    ("ʁ", &[Phone::R]),
    ("ʀ", &[Phone::R]),
    ("j", &[Phone::Y]),
    ("w", &[Phone::W]),
];

/// Marks that don't change the lips: stress, length, syllable and tie marks, the glottal stop, pauses.
const IGNORED: &[char] = &['ˈ', 'ˌ', 'ː', 'ˑ', '.', '‿', '͡', '\u{0361}', '\u{0329}', 'ʔ', '_', '-', ' '];

/// The phones for one eSpeak NG phoneme name, and the parts of it this table does not know.
pub fn phones(ipa: &str) -> (Vec<Phone>, String) {
    let cleaned: String = ipa.chars().filter(|c| !IGNORED.contains(c)).collect();
    let mut rest = cleaned.as_str();
    let mut phones = Vec::new();
    let mut unknown = String::new();
    while let Some(c) = rest.chars().next() {
        match TABLE.iter().find(|(name, _)| rest.starts_with(name)) {
            Some((name, mapped)) => {
                phones.extend_from_slice(mapped);
                rest = &rest[name.len()..];
            }
            None => {
                unknown.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    (phones, unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_english_and_german() {
        assert_eq!(phones("ˈaɪ").0, [Phone::AY]);
        assert_eq!(phones("iː").0, [Phone::IY]);
        assert_eq!(phones("ts").0, [Phone::T, Phone::S]);
        // ü and ö keep rounded lips.
        assert_eq!(phones("yː").0, [Phone::UW]);
        assert_eq!(phones("œ").0, [Phone::AO]);
        assert_eq!(phones("ʔ"), (vec![], String::new()));
        assert_eq!(phones("ʘ").1, "ʘ");
    }
}
