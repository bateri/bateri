//! Colours by name. A page never holds a colour value: it names a [`Tone`] of the pane's
//! theme and, where it wants the tone translucent, an alpha — `color-mix(accent 13%,
//! transparent)` is `Tone::Accent.alpha(0.13)`. What the tone is, in a given theme, is the
//! host's to say (`bt-shell-common` asks `bt_core::Theme`), so the page follows the theme
//! the terminal wears and the arithmetic of colours stays in one place.

/// A tone of the theme: its nine roles, and the values the theme derives from them for
/// surfaces, lines and shadows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    /// The ground of the pane.
    Background,
    /// The text.
    Foreground,
    /// The quieter text (the grid's SGR 2).
    Dim,
    /// The theme's accent.
    Accent,
    /// The cursor's colour.
    Cursor,
    /// Good: running, done.
    Success,
    /// Bad: failed.
    Error,
    /// Neutral news: a link, a place.
    Info,
    /// Needs a look.
    Warning,
    /// The ground raised by that many levels: `Raised(1)` is a card on the pane, `Raised(2)`
    /// a field or a key cap on the card. A dark theme's surface is lighter than the ground,
    /// a light theme's darker (the theme's one rule for "a step above").
    Raised(u8),
    /// The dim text a step further toward the ground: ink that must be readable and not
    /// stand out.
    Quiet,
    /// The hairline: seen, never read.
    Line,
    /// The colour a shadow is cast in: the theme's dark end (black on a dark theme, the
    /// foreground on a light one).
    Shadow,
}

/// A tone with an opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub tone: Tone,
    /// `0.0` transparent, `1.0` solid.
    pub alpha: f32,
}

impl Tone {
    /// This tone at `alpha` of its opacity.
    pub const fn alpha(self, alpha: f32) -> Color {
        Color { tone: self, alpha }
    }

    /// This tone, solid.
    pub const fn solid(self) -> Color {
        self.alpha(1.0)
    }
}

impl From<Tone> for Color {
    fn from(tone: Tone) -> Self {
        tone.solid()
    }
}
