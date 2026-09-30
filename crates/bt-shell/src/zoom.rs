//! View ▸ Bigger / Smaller / Actual Size (Cmd +/−/0): a **temporary** offset kept on top of
//! the point size from the settings.
//!
//! It is not written to the file and is gone when the app quits: enlarging the screen for a
//! moment is not a settings change. So that two point-size sources do not compete, the offset
//! is reset when `size` in the file changes ([`Zoom::after_reload`]) — a user who writes `size`
//! in the editor sees what they wrote. Pure; the one holding it and handing it to the renderer
//! is `app`.

use bt_core::FontOptions;

/// The point size of one press. A chosen constant, not a measured number.
const STEP: f64 = 1.0;

/// The range of the presses, in points. The ends come from the atlas's `point size × scale`
/// clamp (`bt-atlas`, 4–144 pixels; that is the owner of the range): 72 points is the ceiling
/// on a 2× display, 4 points the floor on a 1× display. Inside the range **every press is
/// visible**; a press outside it would hit the clamp and change nothing, and a user holding the
/// key down would have to press each of them back one by one to return.
///
/// `size` in the settings is not bound to this range (the clamp is silent there too): from a
/// point size outside the range a press inward works, one outward does not.
///
/// The second consumer is the Size row of the settings window (`settings_window`): the range
/// the stepper and the field accept is this one, no second number was invented (029 Karar 2).
/// If the file holds a value outside the range, the field shows it as is.
pub(crate) const MIN_SIZE: f64 = 4.0;
pub(crate) const MAX_SIZE: f64 = 72.0;

/// How many steps above the settings' point size we went (negative: below). A step count, not
/// a point size: so that repeated addition does not leave decimal accumulation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Zoom {
    steps: i32,
}

impl Zoom {
    /// The font to hand to the renderer: the settings' one, its point size offset.
    pub(crate) fn apply(self, font: &FontOptions) -> FontOptions {
        FontOptions {
            family: font.family.clone(),
            size: self.size(font),
            // The point size is temporary, the line height is **not**: Cmd +/− moves the
            // point size and the multiplier already scales with it (the cell height derives
            // from the font's metrics).
            line_height: font.line_height,
        }
    }

    /// Bigger: one step larger, if it does not exceed the ceiling.
    pub(crate) fn bigger(self, font: &FontOptions) -> Zoom {
        let next = Zoom {
            steps: self.steps.saturating_add(1),
        };
        if next.size(font) <= MAX_SIZE {
            next
        } else {
            self
        }
    }

    /// Smaller: one step smaller, if it does not go below the floor. The floor is above zero,
    /// so [`FontOptions::size`]'s "greater than zero" rule is also protected from here.
    pub(crate) fn smaller(self, font: &FontOptions) -> Zoom {
        let next = Zoom {
            steps: self.steps.saturating_sub(1),
        };
        if next.size(font) >= MIN_SIZE {
            next
        } else {
            self
        }
    }

    /// The settings file was reread: if `size` changed the offset is reset, otherwise it stays —
    /// a user who changes the family does not lose the point size they enlarged.
    pub(crate) fn after_reload(self, old: &FontOptions, new: &FontOptions) -> Zoom {
        if old.size == new.size {
            self
        } else {
            Zoom::default()
        }
    }

    fn size(self, font: &FontOptions) -> f64 {
        font.size + f64::from(self.steps) * STEP
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(size: f64) -> FontOptions {
        FontOptions {
            family: Some("Menlo".to_owned()),
            size,
            line_height: 1.0,
        }
    }

    #[test]
    fn steps_move_the_size_and_actual_size_returns() {
        let base = font(13.0);
        let zoom = Zoom::default().bigger(&base).bigger(&base);
        assert_eq!(zoom.apply(&base), font(15.0));
        assert_eq!(zoom.smaller(&base).apply(&base), font(14.0));
        // The family stays the settings' one; Actual Size resets the offset.
        assert_eq!(Zoom::default().apply(&base), base);
        assert_eq!(Zoom::default(), Zoom { steps: 0 });
    }

    #[test]
    fn size_change_in_the_file_resets_the_difference() {
        let zoom = Zoom::default().bigger(&font(13.0)).bigger(&font(13.0));
        // `size` was written in the editor: what was written shows, the offset goes.
        assert_eq!(zoom.after_reload(&font(13.0), &font(16.0)), Zoom::default());
        // The point size stayed the same (the family or another key changed): the offset stays.
        let other = FontOptions {
            family: None,
            size: 13.0,
            line_height: 1.0,
        };
        assert_eq!(zoom.after_reload(&font(13.0), &other), zoom);
        assert_eq!(zoom.after_reload(&font(13.0), &font(13.0)), zoom);
    }

    #[test]
    fn steps_stop_at_the_ends() {
        // Ceiling: goes up to 72, one more ignores the press.
        let base = font(70.0);
        let top = Zoom::default().bigger(&base).bigger(&base);
        assert_eq!(top.apply(&base).size, 72.0);
        assert_eq!(top.bigger(&base), top);
        // Floor: goes down to 4.
        let base = font(5.0);
        let bottom = Zoom::default().smaller(&base);
        assert_eq!(bottom.apply(&base).size, 4.0);
        assert_eq!(bottom.smaller(&base), bottom);
        // A press at the end is **not** counted at all: the way back is visible immediately.
        assert_eq!(
            top.bigger(&font(70.0))
                .smaller(&font(70.0))
                .apply(&font(70.0))
                .size,
            71.0
        );
        // From a setting outside the range a press inward works; one outward does
        // not.
        assert_eq!(
            Zoom::default()
                .smaller(&font(100.0))
                .apply(&font(100.0))
                .size,
            99.0
        );
        assert_eq!(Zoom::default().bigger(&font(100.0)), Zoom::default());
        assert_eq!(
            Zoom::default().bigger(&font(2.0)).apply(&font(2.0)).size,
            3.0
        );
        assert_eq!(Zoom::default().smaller(&font(2.0)), Zoom::default());
    }
}
