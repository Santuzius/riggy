//! Mouth shapes and phones, after Rhubarb's `core/Shape.h` and `core/Phone.h`.

use serde::Deserialize;

/// The classic Hanna-Barbera mouth shapes A–F plus the common supplements G, H and the idle X.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Shape {
    /// Closed mouth (m, b, p).
    A,
    /// Clenched teeth (most consonants, some vowels like "ee" in "bee").
    B,
    /// Open mouth ("men", "sun", "say").
    C,
    /// Mouth wide open ("father", "bat", "why").
    D,
    /// Rounded mouth ("off").
    E,
    /// Puckered lips ("you", "boy", "way").
    F,
    /// Upper teeth on the lower lip (f, v).
    G,
    /// Tongue raised behind the teeth (l).
    H,
    /// Idle.
    X,
}

impl Shape {
    pub const ALL: [Shape; 9] = [Shape::A, Shape::B, Shape::C, Shape::D, Shape::E, Shape::F, Shape::G, Shape::H, Shape::X];
}

/// A set of mouth shapes, iterated in the order A..X like Rhubarb's `std::set<Shape>`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ShapeSet(u16);

impl ShapeSet {
    pub fn of(shapes: &[Shape]) -> Self {
        Self(shapes.iter().fold(0, |bits, &s| bits | 1 << s as u16))
    }

    pub fn all() -> Self {
        Self::of(&Shape::ALL)
    }

    pub fn contains(self, shape: Shape) -> bool {
        self.0 & 1 << shape as u16 != 0
    }

    pub fn insert(&mut self, shape: Shape) {
        self.0 |= 1 << shape as u16;
    }

    pub fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    pub fn iter(self) -> impl Iterator<Item = Shape> {
        Shape::ALL.into_iter().filter(move |&s| self.contains(s))
    }
}

/// A subset of the Arpabet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phone {
    // Vowels: monophthongs
    /// [ɔ] as in "off", "fall", "frost"
    AO,
    /// [ɑ] as in "father"
    AA,
    /// [i] as in "bee", "she"
    IY,
    /// [u] as in "you", "new", "food"
    UW,
    /// [ɛ] as in "red", "men"
    EH,
    /// [ɪ] as in "big", "win"
    IH,
    /// [ʊ] as in "should", "could"
    UH,
    /// [ʌ] as in "but", "sun"
    AH,
    /// [ə] as in "alone", "discus"
    Schwa,
    /// [æ] as in "at", "bat"
    AE,
    // Vowels: diphthongs
    /// [eɪ] as in "say", "eight"
    EY,
    /// [aɪ] as in "my", "why", "ride"
    AY,
    /// [oʊ] as in "show", "coat"
    OW,
    /// [aʊ] as in "how", "now"
    AW,
    /// [ɔɪ] as in "boy", "toy"
    OY,
    // Vowels: r-colored
    /// [ɝ] as in "her", "bird", "hurt"
    ER,
    // Consonants: stops
    P,
    B,
    T,
    D,
    K,
    G,
    // Affricates
    /// [tʃ] as in "chair"
    CH,
    /// [dʒ] as in "just"
    JH,
    // Fricatives
    F,
    V,
    /// [θ] as in "thanks"
    TH,
    /// [ð] as in "that"
    DH,
    S,
    Z,
    /// [ʃ] as in "show"
    SH,
    /// [ʒ] as in "measure"
    ZH,
    /// [h] as in "house"
    HH,
    // Nasals
    M,
    N,
    /// [ŋ] as in "sing"
    NG,
    // Liquids
    L,
    R,
    // Semivowels
    /// [j] as in "yes"
    Y,
    W,
    // Misc.; riggy's phone sources don't produce these yet.
    #[allow(dead_code)]
    Breath,
    #[allow(dead_code)]
    Cough,
    #[allow(dead_code)]
    Smack,
    #[allow(dead_code)]
    Noise,
}

impl Phone {
    pub fn is_vowel(self) -> bool {
        self as u8 <= Phone::ER as u8
    }
}
