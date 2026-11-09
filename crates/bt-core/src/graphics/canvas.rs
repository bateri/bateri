//! The pixels of a frame: laying a rectangle of one image on another.
//!
//! Every frame of an animation is kept whole, as straight (not premultiplied)
//! RGBA, so that what is drawn and what is composed on later is one buffer.
//! The work is per row of the rectangle and reads nothing outside it; a buffer
//! that is too short for the rectangle it is asked to give or take makes the
//! call fail, it never panics.

use std::ops::Range;

/// A rectangle in an image's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Rect {
    pub(super) x: u32,
    pub(super) y: u32,
    pub(super) width: u32,
    pub(super) height: u32,
}

impl Rect {
    /// Whether it lies inside an image of `size`: 64 bits wide, so that no
    /// pair of 32-bit numbers a program writes overflows.
    pub(super) fn fits(self, (width, height): (u32, u32)) -> bool {
        u64::from(self.x) + u64::from(self.width) <= u64::from(width)
            && u64::from(self.y) + u64::from(self.height) <= u64::from(height)
    }

    /// Whether the two share a pixel.
    pub(super) fn overlaps(self, other: Self) -> bool {
        let across = |a: u32, a_len: u32, b: u32, b_len: u32| {
            u64::from(a.max(b))
                < (u64::from(a) + u64::from(a_len)).min(u64::from(b) + u64::from(b_len))
        };
        across(self.x, self.width, other.x, other.width)
            && across(self.y, self.height, other.y, other.height)
    }
}

/// How a rectangle is laid on another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    /// Over what is there, by the alpha of what is laid.
    Blend,
    /// In place of what is there.
    Replace,
}

/// An image of `size` of one color.
pub(super) fn filled((width, height): (u32, u32), color: [u8; 4]) -> Vec<u8> {
    let pixels = u64::from(width) * u64::from(height);
    let pixels = usize::try_from(pixels).unwrap_or(0);
    let mut out = Vec::with_capacity(pixels.saturating_mul(4));
    for _ in 0..pixels {
        out.extend_from_slice(&color);
    }
    out
}

/// The bytes of one row of a rectangle of an image `stride` pixels wide.
fn row(stride: u32, x: u32, y: u32, width: u32) -> Option<Range<usize>> {
    let start = (u64::from(y) * u64::from(stride) + u64::from(x)).checked_mul(4)?;
    let end = start.checked_add(u64::from(width).checked_mul(4)?)?;
    Some(usize::try_from(start).ok()?..usize::try_from(end).ok()?)
}

/// Lays on `under` (an image `under_width` wide) the pixels of `over` (`over_width`
/// wide) that are as many as `to` is big and start at `from`; they land on `to`.
pub(super) fn lay(
    under: &mut [u8],
    under_width: u32,
    to: Rect,
    over: &[u8],
    over_width: u32,
    from: (u32, u32),
    mode: Mode,
) -> Option<()> {
    for line in 0..to.height {
        let below = under.get_mut(row(under_width, to.x, to.y.checked_add(line)?, to.width)?)?;
        let above = over.get(row(
            over_width,
            from.0,
            from.1.checked_add(line)?,
            to.width,
        )?)?;
        match mode {
            Mode::Replace => below.copy_from_slice(above),
            Mode::Blend => {
                let (below, _) = below.as_chunks_mut::<4>();
                let (above, _) = above.as_chunks::<4>();
                for (under, over) in below.iter_mut().zip(above) {
                    blend(under, over);
                }
            }
        }
    }
    Some(())
}

/// `over` on `under`, one pixel each: straight alpha, source over, rounded.
fn blend(under: &mut [u8; 4], over: &[u8; 4]) {
    let (over_alpha, under_alpha) = (u32::from(over[3]), u32::from(under[3]));
    if over_alpha == 255 {
        *under = *over;
        return;
    }
    if over_alpha == 0 {
        return;
    }
    // Both alphas count in 255ths and the sum in 255 × 255ths.
    let alpha = over_alpha * 255 + under_alpha * (255 - over_alpha);
    for channel in 0..3 {
        let sum = u32::from(over[channel]) * over_alpha * 255
            + u32::from(under[channel]) * under_alpha * (255 - over_alpha);
        under[channel] = u8::try_from((sum + alpha / 2) / alpha).unwrap_or(u8::MAX);
    }
    under[3] = u8::try_from(alpha.div_ceil(255).min(255)).unwrap_or(u8::MAX);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: u32, y: u32, width: u32, height: u32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_translucent_pixel_over_an_opaque_one_is_their_mix() {
        let mut under = [100, 0, 0, 255];
        lay(
            &mut under,
            1,
            rect(0, 0, 1, 1),
            &[0, 200, 0, 128],
            1,
            (0, 0),
            Mode::Blend,
        )
        .expect("lays");
        assert_eq!(under, [50, 100, 0, 255]);
    }

    #[test]
    fn what_is_laid_over_nothing_is_what_is_laid() {
        let mut under = [0, 0, 0, 0];
        lay(
            &mut under,
            1,
            rect(0, 0, 1, 1),
            &[10, 20, 30, 128],
            1,
            (0, 0),
            Mode::Blend,
        )
        .expect("lays");
        assert_eq!(under, [10, 20, 30, 128]);
    }

    #[test]
    fn an_invisible_pixel_leaves_what_is_there_and_an_opaque_one_replaces_it() {
        let mut under = [1, 2, 3, 4, 5, 6, 7, 8];
        lay(
            &mut under,
            2,
            rect(0, 0, 2, 1),
            &[9, 9, 9, 0, 9, 9, 9, 255],
            2,
            (0, 0),
            Mode::Blend,
        )
        .expect("lays");
        assert_eq!(under, [1, 2, 3, 4, 9, 9, 9, 255]);
    }

    #[test]
    fn replacing_copies_alpha_too() {
        let mut under = [1, 2, 3, 255];
        lay(
            &mut under,
            1,
            rect(0, 0, 1, 1),
            &[9, 9, 9, 0],
            1,
            (0, 0),
            Mode::Replace,
        )
        .expect("lays");
        assert_eq!(under, [9, 9, 9, 0]);
    }

    #[test]
    fn a_rectangle_goes_row_by_row_from_one_offset_to_another() {
        // A 3 × 2 image of numbered pixels; its right column goes to the left.
        let over: Vec<u8> = (0..6u8).flat_map(|pixel| [pixel, 0, 0, 255]).collect();
        let mut under = filled((3, 2), [7, 7, 7, 7]);
        lay(
            &mut under,
            3,
            rect(0, 0, 1, 2),
            &over,
            3,
            (2, 0),
            Mode::Replace,
        )
        .expect("lays");
        let reds: Vec<u8> = under
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[0])
            .collect();
        assert_eq!(reds, [2, 7, 7, 5, 7, 7]);
    }

    #[test]
    fn a_buffer_too_short_for_the_rectangle_fails_and_does_not_panic() {
        let mut under = [0; 4];
        assert!(
            lay(
                &mut under,
                1,
                rect(0, 0, 2, 1),
                &[0; 8],
                2,
                (0, 0),
                Mode::Replace
            )
            .is_none()
        );
        assert!(
            lay(
                &mut under,
                1,
                rect(u32::MAX, 0, 1, 1),
                &[0; 4],
                1,
                (0, 0),
                Mode::Replace
            )
            .is_none()
        );
    }

    #[test]
    fn rectangles_fit_and_overlap_without_overflowing() {
        assert!(rect(1, 1, 2, 1).fits((3, 2)));
        assert!(!rect(2, 0, 2, 1).fits((3, 2)));
        assert!(!rect(u32::MAX, 0, 2, 1).fits((u32::MAX, 2)));
        assert!(rect(0, 0, 2, 2).overlaps(rect(1, 1, 2, 2)));
        assert!(!rect(0, 0, 2, 2).overlaps(rect(2, 0, 2, 2)));
        assert!(!rect(0, 0, 2, 2).overlaps(rect(0, 2, 2, 2)));
        assert!(rect(1, 1, 1, 1).overlaps(rect(0, 0, 3, 3)));
    }
}
