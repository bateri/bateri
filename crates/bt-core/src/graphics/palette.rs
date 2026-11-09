//! Images drawn from the terminal's colours (`f=8` and `f=16`).
//!
//! Every pixel of such an image is an index into the palette, not a colour:
//! the image is drawn in the colours the terminal has now and is made again
//! when they change, a switch from a dark theme to a light one or a program's
//! own `OSC 4`. The indices are kept as they came, and the RGBA copy that is
//! drawn ([`super::store::Frame`]) is made from them for each palette: one
//! copy the size of the image, as for every other format, and nothing here
//! runs while no such image is stored.
//!
//! The palette is the 256 colours of `SGR 38;5`, but for the last eight,
//! which kitty gives their own meaning ([`Palette::of`]). The numbering is
//! not alacritty's `term::color` table, whose 248 and up are other colours.

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

use crate::color::{Theme, resolve};

/// The pixels of an image drawn from the palette, as they came.
pub(crate) struct Indexed {
    /// A palette index for each pixel, each followed by its opacity when
    /// `alpha`.
    pub(crate) data: Vec<u8>,
    /// `f=16`: an opacity follows each index; else every pixel is opaque.
    pub(crate) alpha: bool,
    /// The palette the RGBA copy was made from ([`Images::repaint`]), 0 for
    /// none yet.
    ///
    /// [`Images::repaint`]: super::store::Images::repaint
    pub(super) painted: u64,
}

impl Indexed {
    pub(crate) fn new(data: Vec<u8>, alpha: bool) -> Self {
        Self {
            data,
            alpha,
            painted: 0,
        }
    }
}

/// The 256 colours an indexed image is drawn in.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Palette([Rgb; 256]);

fn rgb(hex: u32) -> Rgb {
    Rgb {
        r: (hex >> 16) as u8,
        g: (hex >> 8) as u8,
        b: hex as u8,
    }
}

impl Palette {
    /// The palette the terminal has: what the program set (`colors`) or else
    /// the theme's.
    ///
    /// 0 to 247 are the `SGR` 256-colour table, 248 and 249 the default
    /// background and foreground, 250 the selection's ground, 252 the cursor's,
    /// 254 the colour that marks a link (the theme's accent) and 255 the
    /// default foreground. The text of a selection and of the cursor has no
    /// colour of its own here — it is drawn in the cell's — so, as kitty says for
    /// that case, 251 and 253 are the default background.
    pub(crate) fn of(colors: &Colors, theme: &Theme) -> Self {
        let named = |color: NamedColor| resolve(Color::Named(color), colors, theme);
        let background = named(NamedColor::Background);
        let foreground = named(NamedColor::Foreground);
        let mut table = [background; 256];
        for (index, slot) in table.iter_mut().enumerate().take(248) {
            *slot = resolve(Color::Indexed(index as u8), colors, theme); // audit: bounded by `take`
        }
        table[248] = background;
        table[249] = foreground;
        table[250] = rgb(theme.selection);
        table[251] = background;
        table[252] = named(NamedColor::Cursor);
        table[253] = background;
        table[254] = rgb(theme.accent);
        table[255] = foreground;
        Self(table)
    }

    /// The straight RGBA of `indexed` in this palette.
    pub(crate) fn paint(&self, indexed: &Indexed) -> Vec<u8> {
        let step = if indexed.alpha { 2 } else { 1 };
        let mut pixels = Vec::with_capacity(indexed.data.len() / step * 4);
        for pixel in indexed.data.chunks_exact(step) {
            let Rgb { r, g, b } = self.0[usize::from(pixel[0])]; // audit: a byte, 256 entries
            let opacity = pixel.get(1).copied().unwrap_or(u8::MAX);
            pixels.extend_from_slice(&[r, g, b, opacity]);
        }
        pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::BATERI
    }

    #[test]
    fn the_first_248_are_the_256_colour_table_and_the_rest_are_the_terminals_own() {
        let theme = theme();
        let palette = Palette::of(&Colors::default(), &theme);
        for index in [0, 7, 15, 16, 100, 231, 232, 247] {
            assert_eq!(
                palette.0[index],
                theme.default(index),
                "{index} is the table's"
            );
        }
        assert_eq!(palette.0[248], rgb(theme.background));
        assert_eq!(palette.0[249], rgb(theme.foreground));
        assert_eq!(palette.0[250], rgb(theme.selection));
        assert_eq!(palette.0[251], rgb(theme.background));
        assert_eq!(palette.0[252], rgb(theme.cursor));
        assert_eq!(palette.0[253], rgb(theme.background));
        assert_eq!(palette.0[254], rgb(theme.accent));
        assert_eq!(palette.0[255], rgb(theme.foreground));
    }

    #[test]
    fn what_a_program_set_wins_over_the_theme() {
        let theme = theme();
        let mut colors = Colors::default();
        colors[1] = Some(Rgb { r: 1, g: 2, b: 3 });
        colors[NamedColor::Background as usize] = Some(Rgb { r: 9, g: 8, b: 7 });
        let palette = Palette::of(&colors, &theme);
        assert_eq!(palette.0[1], Rgb { r: 1, g: 2, b: 3 });
        assert_eq!(palette.0[248], Rgb { r: 9, g: 8, b: 7 });
        assert_eq!(palette.0[251], Rgb { r: 9, g: 8, b: 7 });
    }

    #[test]
    fn a_pixel_is_its_colour_and_an_opacity_that_is_full_with_no_alpha() {
        let palette = Palette::of(&Colors::default(), &theme());
        let [red, blue] = [palette.0[1], palette.0[4]];
        let opaque = palette.paint(&Indexed::new(vec![1, 4], false));
        assert_eq!(
            opaque,
            [red.r, red.g, red.b, 255, blue.r, blue.g, blue.b, 255]
        );
        let faint = palette.paint(&Indexed::new(vec![1, 0, 4, 128], true));
        assert_eq!(faint, [red.r, red.g, red.b, 0, blue.r, blue.g, blue.b, 128]);
    }
}
