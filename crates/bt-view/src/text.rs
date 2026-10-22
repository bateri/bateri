//! Text, as the engine sees it: a font, a string and the size the host says it takes. The
//! engine has no fonts of its own — the host shapes and measures (it is also what draws, so
//! what was measured is what appears) and answers through [`Measure`].

/// The face a line is set in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// The system's interface font.
    System,
    /// The terminal's own face: the page reads as part of the terminal it sits in.
    Terminal,
}

/// A font as the host is asked for it: the size is in **pixels** (the engine multiplies the
/// points a page is written in by the scale) and the weight on the CSS scale (100–900,
/// 400 regular, 700 bold).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Font {
    pub family: Family,
    pub size: f32,
    pub weight: u16,
}

impl Eq for Font {}

impl std::hash::Hash for Font {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.family.hash(state);
        self.size.to_bits().hash(state);
        self.weight.hash(state);
    }
}

/// What a line of text takes, in pixels: the width of the line and the height of its line
/// box (ascent, descent and leading).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
    pub width: f32,
    pub height: f32,
}

/// The host's side of text: how large one line is.
///
/// A host with no proportional text answers [`Metrics::default`] (nothing): the boxes of the
/// page are laid out and drawn and its words are not.
pub trait Measure {
    /// The size of `text` set in one line in `font`.
    fn measure(&mut self, font: &Font, text: &str) -> Metrics;
}

/// Where a line stands in the box it is given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}
