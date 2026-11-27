//! Full screen's title strip: how far it pushes the window's content down.
//!
//! In full screen the system hides the title bar and the menu bar. With the pointer at the top
//! of the screen it slides the menu bar down, and under it a plain strip with the traffic lights
//! — a window of its own (`NSToolbarFullScreenWindow`, the "host"), as wide as the screen and
//! a little over thirty points tall. The host's frame is all the system tells: at rest hidden it
//! stands with its top at the screen's top, and fully revealed with its top at the bottom of the
//! menu bar, and `NSWindowDidMoveNotification` comes with every step between.
//!
//! The strip would cover the window's top row. Instead the content slides down with it and
//! stands below it, then slides back. The terminals do not change size: it is a shift of how
//! the window is seen, and this module answers only how far (the shell is `window`'s root
//! view). Pure; the frames come in as numbers, in the screen's coordinates (y up).
//!
//! Measured on macOS 26.4 only (a 2560×1440 screen, a 30-point menu bar, a 32-point host).

/// How far the content is pushed down by a strip `height` tall whose frame's bottom edge is at
/// `y`, on a screen that reaches `screen_top` and whose usable area (under the menu bar) reaches
/// `visible_top`.
///
/// - **Hidden** — the host's top at the screen's top, `y` at `screen_top - height` or above:
///   nothing.
/// - **Fully revealed** — the host's top at the menu bar's bottom, `y` at `visible_top - height`
///   or below: the menu bar's height and the strip's, `screen_top - visible_top + height`.
/// - **Between**, linear in `y`.
///
/// A menu bar kept shown in full screen leaves the host revealed, and so the offset stays at
/// the full value: the host's place decides, not whether it moved. A taller menu bar (a notched
/// screen) is `visible_top` lower and the full value larger.
///
/// When the screen has no menu bar to travel (`visible_top` as high as `screen_top`) the host's
/// progress cannot be read from its frame: the offset is nothing while the host stands hidden
/// and the strip's height once it has left. Anything that is not a number is nothing.
pub fn strip_offset(screen_top: f64, visible_top: f64, height: f64, y: f64) -> f64 {
    if ![screen_top, visible_top, height, y]
        .iter()
        .all(|number| number.is_finite())
    {
        return 0.0;
    }
    let menu_bar = (screen_top - visible_top).max(0.0);
    let height = height.max(0.0);
    let full = menu_bar + height;
    let hidden = screen_top - height;
    let revealed = visible_top - height;
    let progress = if y >= hidden {
        0.0
    } else if hidden - revealed <= 0.0 {
        1.0
    } else {
        ((hidden - y) / (hidden - revealed)).clamp(0.0, 1.0)
    };
    full * progress
}

/// `offset` on a whole device pixel of a screen `scale` times denser than points: a layer that
/// sits on a fraction of a pixel is resampled, and the panes' text would blur for the length of
/// the slide. A scale that is not above nothing leaves the offset as it is.
pub fn on_pixels(offset: f64, scale: f64) -> f64 {
    if scale > 0.0 && scale.is_finite() && offset.is_finite() {
        (offset * scale).round() / scale
    } else {
        offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The measured screen: 2560×1440 points, the menu bar 30, the host 32 tall.
    const TOP: f64 = 1440.0;
    const VISIBLE: f64 = 1410.0;
    const HOST: f64 = 32.0;
    const HIDDEN: f64 = 1408.0;
    const REVEALED: f64 = 1378.0;

    #[test]
    fn a_hidden_strip_pushes_nothing() {
        assert_eq!(strip_offset(TOP, VISIBLE, HOST, HIDDEN), 0.0);
        // Above its rest (a spring past it) is the same.
        assert_eq!(strip_offset(TOP, VISIBLE, HOST, HIDDEN + 6.0), 0.0);
    }

    #[test]
    fn a_revealed_strip_pushes_the_menu_bar_and_itself() {
        assert_eq!(strip_offset(TOP, VISIBLE, HOST, REVEALED), 62.0);
        // Below its stop is the same, not more.
        assert_eq!(strip_offset(TOP, VISIBLE, HOST, REVEALED - 9.0), 62.0);
    }

    #[test]
    fn between_the_two_it_is_linear() {
        assert_eq!(strip_offset(TOP, VISIBLE, HOST, 1393.0), 31.0);
        let quarter = strip_offset(TOP, VISIBLE, HOST, HIDDEN - 7.5);
        assert!((quarter - 15.5).abs() < 1e-9, "{quarter}");
        // The slide down, as it was measured: never less as the host goes lower, and the way
        // back is the same numbers.
        let down = [
            1406.0, 1403.0, 1400.0, 1395.0, 1390.0, 1384.0, 1380.0, 1378.0,
        ];
        let mut last = 0.0;
        for y in down {
            let now = strip_offset(TOP, VISIBLE, HOST, y);
            assert!(now > last && now <= 62.0, "{y}: {now} after {last}");
            last = now;
        }
        assert_eq!(last, 62.0);
    }

    /// The menu bar set to stay in full screen: the host sits revealed and never moves, so the
    /// offset is the full value from the first time it is asked, with nothing to wait for.
    #[test]
    fn a_menu_bar_kept_shown_keeps_the_full_offset() {
        for _ in 0..3 {
            assert_eq!(strip_offset(TOP, VISIBLE, HOST, REVEALED), 62.0);
        }
    }

    /// A notched screen's menu bar is taller (37 points on a 14-inch at its default scale).
    #[test]
    fn a_taller_menu_bar_makes_a_larger_offset() {
        let (top, visible) = (982.0, 945.0);
        assert_eq!(strip_offset(top, visible, HOST, top - HOST), 0.0);
        assert_eq!(strip_offset(top, visible, HOST, visible - HOST), 69.0);
        assert_eq!(
            strip_offset(top, visible, HOST, (top + visible) / 2.0 - HOST),
            34.5
        );
    }

    /// No menu bar to travel (it hides on the desktop too): the frame cannot say how far the
    /// host got, and the rule neither divides by nothing nor invents a number.
    #[test]
    fn a_screen_with_no_menu_bar_to_travel_steps() {
        assert_eq!(strip_offset(TOP, TOP, HOST, TOP - HOST), 0.0);
        assert_eq!(strip_offset(TOP, TOP, HOST, TOP - HOST - 1.0), HOST);
        // A visible area reaching higher than the screen is no menu bar either.
        assert_eq!(strip_offset(TOP, TOP + 5.0, HOST, TOP - HOST - 1.0), HOST);
    }

    #[test]
    fn what_is_not_a_number_pushes_nothing() {
        assert_eq!(strip_offset(f64::NAN, VISIBLE, HOST, REVEALED), 0.0);
        assert_eq!(strip_offset(TOP, VISIBLE, HOST, f64::NAN), 0.0);
        assert_eq!(strip_offset(TOP, VISIBLE, HOST, f64::NEG_INFINITY), 0.0);
        assert_eq!(strip_offset(TOP, VISIBLE, -HOST, REVEALED), 30.0);
    }

    #[test]
    fn the_offset_stands_on_a_whole_device_pixel() {
        assert_eq!(on_pixels(31.26, 2.0), 31.5);
        assert_eq!(on_pixels(31.24, 2.0), 31.0);
        assert_eq!(on_pixels(62.0, 2.0), 62.0);
        assert_eq!(on_pixels(0.2, 1.0), 0.0);
        assert_eq!(on_pixels(31.26, 0.0), 31.26);
        assert_eq!(on_pixels(31.26, f64::NAN), 31.26);
    }
}
