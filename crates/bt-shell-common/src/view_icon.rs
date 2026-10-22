//! A program's own icon on the application's chrome — the strip over its view, its tab's tile, the
//! tabs menu's row and the tab's card — drawn in the program's colours but to the application's
//! measures: the tile's size, corners, face, deck and badges stay the application's, and only the
//! picture is the program's.
//!
//! A program gives a picture and nothing about how it is meant to stand (MCP's `icons`: bytes, a
//! theme it suits, sizes), so how it stands is read from the picture itself ([`lay`]): an icon
//! whose edges are opaque at their middles brings its own ground — a square application icon, its
//! own corners rounded or not — and becomes the tile, cut to the tile's corners; one that is clear
//! there is a shape on nothing and stands on the application's face where the application's own
//! marks stand ([`crate::icon`]), at their size. The middles and not the corners: an icon that
//! rounds its own corners is clear at the corners and still brings its ground.
//!
//! Platform-free: the shell decodes the picture and hands over its pixels' alpha.

/// How a program's icon stands on the application's chrome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lay {
    /// It brings its own ground: it fills the tile, cut to the tile's shape, and the tile's line
    /// and badges stand over it.
    Ground,
    /// It is a shape on nothing: it stands on the application's face, at a mark's size.
    Mark,
}

/// The alpha (`0..=255`) from which an edge's middle counts as covered: an edge drawn with a
/// soft rim still counts, a faint shadow does not.
pub const OPAQUE: u8 = 230;

/// How an icon of `width` by `height` pixels stands, read from `alpha(x, y)` (pixels, `x` from
/// the left, `y` from the top): [`Lay::Ground`] when the middle of every edge is covered, one
/// pixel in from it (the outermost row of an image is often its anti-aliased rim), else
/// [`Lay::Mark`]. A picture too small to have an inside is a mark.
pub fn lay(width: u32, height: u32, alpha: impl Fn(u32, u32) -> u8) -> Lay {
    if width < 3 || height < 3 {
        return Lay::Mark;
    }
    let (middle_x, middle_y) = (width / 2, height / 2);
    let edges = [
        (middle_x, 1),
        (middle_x, height - 2),
        (1, middle_y),
        (width - 2, middle_y),
    ];
    if edges.iter().all(|&(x, y)| alpha(x, y) >= OPAQUE) {
        Lay::Ground
    } else {
        Lay::Mark
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture `side` pixels square whose alpha is `covered(x, y)`: 255 or 0.
    fn picture(side: u32, covered: impl Fn(u32, u32) -> bool) -> Lay {
        lay(side, side, |x, y| if covered(x, y) { 255 } else { 0 })
    }

    #[test]
    fn a_full_square_brings_its_ground() {
        assert_eq!(picture(64, |_, _| true), Lay::Ground);
    }

    #[test]
    fn an_icon_that_rounds_its_own_corners_still_brings_its_ground() {
        let rounded = |x: u32, y: u32| {
            let (r, last) = (12i64, 63i64);
            let (x, y) = (i64::from(x), i64::from(y));
            let cx = x.clamp(r, last - r);
            let cy = y.clamp(r, last - r);
            (x - cx).pow(2) + (y - cy).pow(2) <= r * r
        };
        assert_eq!(picture(64, rounded), Lay::Ground);
    }

    #[test]
    fn a_shape_on_nothing_stands_on_the_face() {
        // Three bars with a clear margin, as the example tool's icon.
        let bars = |x: u32, y: u32| {
            (6..=58).contains(&x) && [8..=22, 24..=38, 40..=54].iter().any(|r| r.contains(&y))
        };
        assert_eq!(picture(64, bars), Lay::Mark);
        // Covered left and right but clear above: not a ground either.
        assert_eq!(picture(64, |_, y| (4..60).contains(&y)), Lay::Mark);
    }

    #[test]
    fn a_soft_rim_counts_and_a_shadow_does_not() {
        // The outermost row is the rim; one in, the colour is solid.
        let rimmed = |x: u32, y: u32| {
            if x == 0 || y == 0 || x == 31 || y == 31 {
                90
            } else {
                250
            }
        };
        assert_eq!(lay(32, 32, rimmed), Lay::Ground);
        assert_eq!(lay(32, 32, |_, _| OPAQUE - 1), Lay::Mark);
    }

    #[test]
    fn a_picture_with_no_inside_is_a_mark() {
        assert_eq!(lay(2, 2, |_, _| 255), Lay::Mark);
        assert_eq!(lay(0, 0, |_, _| 255), Lay::Mark);
        assert_eq!(lay(64, 1, |_, _| 255), Lay::Mark);
    }
}
