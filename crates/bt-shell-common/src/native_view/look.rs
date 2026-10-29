//! What the terminal tells a page of how it draws its cursor and its selection.
//!
//! The person sets the terminal's cursor in `settings.toml` (`cursor`, `cursor_blink`,
//! `cursor_blink_interval`, `cursor_radius`, `cursor_glow`, `cursor_unfocused`, and `[motion]
//! cursor_motion`), and a page of the kit can ask how, as `pencere.terminal_look()`, in the words
//! and units of its editor's options (`system.terminal-look`). **bateri says, the page decides**
//! (the user's rule, 2026-10-29): this is the whole of it, a record to be read. bateri applies
//! nothing to a page's editors; a page that wants its editors to look as the terminal does asks
//! (`terminal.editor` in `@pencere/terminal`) and takes what it likes.
//!
//! - [`terminal_look`] is the mapping from the settings, pure.
//! - [`caret_width_pt`] is the one field that is no setting: how thick the terminal draws its bar
//!   and its line, which is the font's own underline metric in pixels, as points of a page.
//! - [`LookSlot`] is what the page's program reads it from, **from the module's thread** (not the
//!   thread the settings are changed on), and what the pane changes it in when they are.

use std::sync::{Arc, Mutex, PoisonError};

use bt_core::{CaretShape, CursorBlink, CursorMotion, Settings, UnfocusedCaret};
use pencere_view::TerminalLook;
use pencere_view::edit::{CaretMotion, CaretShape as PageShape, CaretUnfocused};

/// The terminal's look, from the settings, in the kit's words and units — all but the caret's
/// width, which is no setting ([`caret_width_pt`]; `None` here).
///
/// - `cursor`: `block` and `underline` as they are, `beam` is the kit's `bar`.
/// - `cursor_blink`: `on` and `auto` blink (`auto` is what the program says by DECSCUSR, which
///   the page has no way to be told), `off` does not.
/// - `cursor_blink_interval`: seconds, the half period, as the kit's.
/// - `cursor_radius` and the selection's radius: a fraction of the height of a row (the terminal's
///   cell), as the kit's. The selection's is a design constant of the terminal's
///   ([`bt_gpu::SELECTION_RADIUS`]), no setting of the person's.
/// - `cursor_glow`: the kit's glow is one number too, and 1 is the terminal's.
/// - `cursor_unfocused`: `hollow` and `solid` as they are (`hidden` is the kit's own default for a
///   window that is not in front, which the terminal has no setting for).
/// - `cursor_motion`: the **setting**, not what Reduce Motion makes of it: every frame carries
///   Reduce Motion to the page and the kit's caret obeys it by itself. `ease` and `spring` are the
///   numbers the terminal's cursor moves by (`bt_gpu`'s `EASE_DURATION` and `OMEGA`, which the
///   kit's [`CaretMotion::EASE`] and [`CaretMotion::SPRING`] are: `bt-gpu`'s `motion` tests it).
pub fn terminal_look(settings: &Settings) -> TerminalLook {
    let mut look = TerminalLook::default();
    let caret = &mut look.caret;
    caret.shape = Some(match settings.cursor {
        CaretShape::Block => PageShape::Block,
        CaretShape::Underline => PageShape::Underline,
        CaretShape::Beam => PageShape::Bar,
    });
    caret.radius = Some(settings.caret.radius_ratio as f32);
    caret.glow = Some(settings.caret.glow as f32);
    caret.blink = Some(match settings.cursor_blink {
        CursorBlink::On | CursorBlink::Auto => true,
        CursorBlink::Off => false,
    });
    caret.blink_interval = Some(settings.blink_interval as f32);
    caret.unfocused = Some(match settings.caret.unfocused {
        UnfocusedCaret::Hollow => CaretUnfocused::Hollow,
        UnfocusedCaret::Solid => CaretUnfocused::Solid,
    });
    caret.motion = Some(match settings.cursor_motion {
        CursorMotion::Snap => CaretMotion::Snap,
        CursorMotion::Ease => CaretMotion::Ease(CaretMotion::EASE),
        CursorMotion::Spring => CaretMotion::Spring(CaretMotion::SPRING),
    });
    look.selection.radius = Some(bt_gpu::SELECTION_RADIUS);
    look
}

/// How thick the terminal draws its bar and its line, as points of a page: the thickness in
/// pixels ([`bt_gpu::caret_thickness_px`], the font's own underline metric held to the cell) at
/// the display's `scale`, and over `zoom`, how much larger than its own size a page is drawn in
/// this pane (`Placement::zoom_for`) — so that what a page draws at this width is the terminal's
/// thickness in pixels. `None` for a scale or a zoom that is no number worth dividing by.
pub fn caret_width_pt(cell_px: (u16, u16), rule_px: u16, scale: f64, zoom: f32) -> Option<f32> {
    let per_point = scale * f64::from(zoom);
    if !per_point.is_finite() || per_point <= 0.0 {
        return None;
    }
    let cell = (f32::from(cell_px.0), f32::from(cell_px.1));
    let thick = bt_gpu::caret_thickness_px(cell, f32::from(rule_px));
    #[allow(clippy::cast_possible_truncation)]
    Some((f64::from(thick) / per_point) as f32)
}

/// The look a page's program reads, shared with the pane that changes it. Cloned, it is the same
/// slot: the program holds one, the pane another. A lock, not a cell, because the program reads
/// it from the module's thread (the kit's `Program::terminal_look`) and the pane changes it from
/// the main one.
#[derive(Clone, Debug, Default)]
pub struct LookSlot(Arc<Mutex<TerminalLook>>);

impl LookSlot {
    pub fn new(look: TerminalLook) -> Self {
        Self(Arc::new(Mutex::new(look)))
    }

    /// The look as it is now.
    pub fn get(&self) -> TerminalLook {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Makes the look `look`; `true` when it is not what it was, which is when a page that
    /// stands is to be told ([`pencere_drive::Page::terminal_look_changed`]) and not otherwise
    /// (the kit's rule: the event is told when what is said changed, and only then).
    pub fn set(&self, look: TerminalLook) -> bool {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let changed = *held != look;
        *held = look;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_core::CaretStyle;

    fn caret(settings: &Settings) -> pencere_view::TerminalCaret {
        terminal_look(settings).caret
    }

    #[test]
    fn the_defaults_come_to_the_terminals_own_look() {
        let look = terminal_look(&Settings::default());
        let caret = look.caret;
        assert_eq!(caret.shape, Some(PageShape::Block));
        assert_eq!(caret.width, None, "the width is no setting");
        assert_eq!(caret.radius, Some(0.10));
        assert_eq!(caret.glow, Some(1.0));
        assert_eq!(
            caret.blink,
            Some(false),
            "bateri's cursor does not blink by default"
        );
        assert_eq!(caret.blink_interval, Some(0.5));
        assert_eq!(caret.unfocused, Some(CaretUnfocused::Hollow));
        assert_eq!(caret.motion, Some(CaretMotion::Spring(30.0)));
        assert_eq!(look.selection.radius, Some(0.22));
    }

    #[test]
    fn a_beam_is_the_kits_bar() {
        let beam = Settings {
            cursor: CaretShape::Beam,
            ..Settings::default()
        };
        assert_eq!(caret(&beam).shape, Some(PageShape::Bar));
        let line = Settings {
            cursor: CaretShape::Underline,
            ..Settings::default()
        };
        assert_eq!(caret(&line).shape, Some(PageShape::Underline));
    }

    #[test]
    fn on_and_auto_blink_and_off_does_not() {
        let blink = |value| {
            caret(&Settings {
                cursor_blink: value,
                ..Settings::default()
            })
            .blink
        };
        assert_eq!(blink(CursorBlink::On), Some(true));
        assert_eq!(blink(CursorBlink::Auto), Some(true));
        assert_eq!(blink(CursorBlink::Off), Some(false));
    }

    #[test]
    fn the_numbers_are_the_persons_in_the_kits_units() {
        let settings = Settings {
            blink_interval: 0.25,
            caret: CaretStyle {
                radius_ratio: 0.5,
                glow: 2.0,
                unfocused: UnfocusedCaret::Solid,
            },
            cursor_motion: CursorMotion::Ease,
            ..Settings::default()
        };
        let caret = caret(&settings);
        assert_eq!(caret.blink_interval, Some(0.25));
        assert_eq!(caret.radius, Some(0.5));
        assert_eq!(caret.glow, Some(2.0));
        assert_eq!(caret.unfocused, Some(CaretUnfocused::Solid));
        assert_eq!(caret.motion, Some(CaretMotion::Ease(0.18)));
        let snap = Settings {
            cursor_motion: CursorMotion::Snap,
            ..Settings::default()
        };
        assert_eq!(terminal_look(&snap).caret.motion, Some(CaretMotion::Snap));
    }

    #[test]
    fn the_width_is_the_terminals_thickness_in_a_pages_points() {
        // 2 px thick at 2x on a page drawn at its own size: one point.
        assert_eq!(caret_width_pt((18, 36), 2, 2.0, 1.0), Some(1.0));
        // The page is drawn larger by `zoom`, so the points to say are fewer.
        assert_eq!(caret_width_pt((18, 36), 3, 2.0, 1.5), Some(1.0));
        // Held to the cell, and to a pixel at least, as the terminal draws it.
        assert_eq!(caret_width_pt((4, 36), 9, 1.0, 1.0), Some(4.0));
        assert_eq!(caret_width_pt((18, 36), 0, 1.0, 1.0), Some(1.0));
        assert_eq!(caret_width_pt((18, 36), 2, 0.0, 1.0), None);
        assert_eq!(caret_width_pt((18, 36), 2, 2.0, f32::NAN), None);
    }

    #[test]
    fn the_slot_says_when_the_look_changed_and_only_then() {
        let slot = LookSlot::new(terminal_look(&Settings::default()));
        let program = slot.clone();
        assert!(!slot.set(terminal_look(&Settings::default())));
        let beam = Settings {
            cursor: CaretShape::Beam,
            ..Settings::default()
        };
        assert!(slot.set(terminal_look(&beam)), "a shape is a change");
        assert_eq!(
            program.get().caret.shape,
            Some(PageShape::Bar),
            "one slot, two holders"
        );
        assert!(!slot.set(terminal_look(&beam)));
        let mut wide = terminal_look(&beam);
        wide.caret.width = Some(1.0);
        assert!(slot.set(wide), "a width is a change");
    }
}
