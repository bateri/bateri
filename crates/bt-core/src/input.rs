//! Input encoding: arrow key, mouse button and wheel report → PTY bytes.
//!
//! **Pure and lock-free.** The mode question (`TermMode`) is answered under
//! the caller's `Term` lock and only the answer arrives here; the decision
//! itself and the bytes live here, tested without a PTY. The keyboard's
//! arrows and the wheel's arrows go through the same [`arrow`] — the arrow
//! byte is written in exactly one place in the repo. Both mouse events come
//! out of one body ([`mouse_report`]): the wheel's press and the button's
//! press/release share the same encoding, the same limit and the same
//! rejection.

use alacritty_terminal::term::TermMode;

/// Arrow key: the keyboard's four, the wheel's two.
///
/// `bt-shell` translates the key into this, not into bytes: the format depends
/// on DECCKM ([`arrow`]) and the mode lives in `Term`
/// ([`crate::Session::write_arrow`]). If `bt-shell` wrote the byte it would
/// have to know the mode, meaning it would either keep it or see the alacritty
/// type. The wheel's arrows take the same path: the DECCKM question is
/// answered in one place in the repo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrow {
    Up,
    Down,
    Right,
    Left,
}

/// The arrow's bytes: SS3 (`\eOA`) when DECCKM (`\e[?1h`, `APP_CURSOR`) is
/// on, CSI (`\e[A`) when off.
///
/// The two formats exist because of `TERM`: `xterm-256color`'s terminfo says
/// `smkx=\E[?1h\E=` and `kcuu1=\EOA`, so an application that reads terminfo
/// (less, ncurses) turns DECCKM on at startup and expects SS3; an application
/// that doesn't turn the mode on expects CSI. alacritty's keyboard bindings
/// carry the same pair.
pub(crate) fn arrow(arrow: Arrow, mode: TermMode) -> [u8; 3] {
    let intro = if mode.contains(TermMode::APP_CURSOR) {
        b'O'
    } else {
        b'['
    };
    let last = match arrow {
        Arrow::Up => b'A',
        Arrow::Down => b'B',
        Arrow::Right => b'C',
        Arrow::Left => b'D',
    };
    [0x1b, intro, last]
}

/// The wheel's backward (up) button; forward (down) is [`WHEEL_DOWN`].
/// The wheel has no release event; the report is press only.
pub(crate) const WHEEL_UP: u8 = 64;
pub(crate) const WHEEL_DOWN: u8 = 65;

/// Mouse report encoding — the one the application picked with DECSET
/// 1006/1005.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MouseEncoding {
    /// 1006: decimal, unbounded.
    Sgr,
    /// 1005: coordinate is a UTF-8 character, cut off at 2015.
    Utf8,
    /// X10/normal: coordinate is a single byte, cut off at 223.
    Normal,
}

impl MouseEncoding {
    /// The first value the coordinate does **not** fit; SGR is unbounded
    /// because it is decimal.
    ///
    /// This is the number's only home and it has two consumers: the report's
    /// rejection ([`mouse_report`], `>= limit` → `None`) and the release's
    /// clamping ([`MouseEncoding::clamp`]). Written in two places, one could
    /// change while the other stayed, and the drift would be silent — the
    /// clamped coordinate would be rejected anyway.
    ///
    /// The values are alacritty's: in plain mode `32 + 1 + 222 = 255` is the
    /// last byte, in UTF-8 mode `32 + 1 + 2014 = 2047` is the last value of a
    /// two-byte UTF-8.
    fn limit(self) -> Option<u16> {
        match self {
            MouseEncoding::Sgr => None,
            MouseEncoding::Utf8 => Some(2015),
            MouseEncoding::Normal => Some(223),
        }
    }

    /// Lowers the coordinate to the last value the encoding fits.
    ///
    /// **Only the release's path** ([`crate::Session::mouse_button`]): a
    /// coordinate that doesn't fit is rejected on press and clamped on
    /// release. The asymmetry is there because the cases are asymmetric — a
    /// rejected press is a gesture that never started, while a dropped
    /// release is **a button stuck down** in the application. A slightly
    /// wrong coordinate is better than that.
    pub(crate) fn clamp(self, pos: u16) -> u16 {
        self.limit().map_or(pos, |limit| pos.min(limit - 1))
    }
}

/// The encoding the application picked. The wheel ([`wheel_route`]) and the
/// button ([`button_route`]) read from the same table.
fn mouse_encoding(mode: TermMode) -> MouseEncoding {
    if mode.contains(TermMode::SGR_MOUSE) {
        MouseEncoding::Sgr
    } else if mode.contains(TermMode::UTF8_MOUSE) {
        MouseEncoding::Utf8
    } else {
        MouseEncoding::Normal
    }
}

/// Mouse button — the report's low two bits.
///
/// The wheel is **not** here: its button ([`WHEEL_UP`], [`WHEEL_DOWN`]) is a
/// number `bt-shell` never sees, because [`crate::Session::scroll_wheel`]
/// derives the wheel's direction from the line sign itself. There is nothing
/// beyond the third physical button either: X10 carries two bits and `3` is
/// reserved for release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

/// A mouse event's modifiers: two go into the report, one does the
/// arbitration.
///
/// **Shift never goes into the report** and that is not an omission but the
/// result of [`button_route`]: while Shift is held the event goes to the
/// selection, not the application, so there is no branch where it could
/// appear in the report. If bit (4) were set anyway we would write a value
/// no application can read. The field is here for that reason, not as a
/// separate `shift` argument: the two halves of the rule ("enters the
/// arbitration", "doesn't enter the report") sit side by side in one type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseModifiers {
    /// The terminal's escape hatch — see the type's doc.
    pub shift: bool,
    /// Option on macOS; xterm's bit 8.
    pub meta: bool,
    /// xterm's bit 16.
    pub control: bool,
}

/// The button's low two bits. `3` is **not** here: that code is reserved for
/// release ([`mouse_report`]) and for motion with no button ([`motion_byte`]).
fn button_base(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

/// xterm's modifier bits: Meta 8, Control 16. Shift's 4 is **never set** —
/// the reason is in [`MouseModifiers`]'s doc.
fn modifier_bits(modifiers: MouseModifiers) -> u8 {
    let meta = if modifiers.meta { 8 } else { 0 };
    let control = if modifiers.control { 16 } else { 0 };
    meta | control
}

/// The button byte of a press/release report: the button's code plus the
/// modifiers.
pub(crate) fn button_byte(button: MouseButton, modifiers: MouseModifiers) -> u8 {
    button_base(button) | modifier_bits(modifiers)
}

/// The button byte of a motion report: the **motion bit** (32) plus the held
/// button, or `3` when there is none.
///
/// That is xterm's encoding: the same `3` means both "release" and "no
/// button", and what tells them apart is bit 32. Motion with a held button
/// (`\e[<32;..M`) and motion without one (`\e[<35;..M`) therefore come out of
/// a single function.
pub(crate) fn motion_byte(button: Option<MouseButton>, modifiers: MouseModifiers) -> u8 {
    const MOTION: u8 = 32;
    let base = button.map_or(3, button_base);
    MOTION | base | modifier_bits(modifiers)
}

/// The button's decision table — [`WheelRoute`]'s sibling, same pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ButtonRoute {
    /// The application asked for mouse reports (1000/1002/1003) and Shift is
    /// not held.
    Report(MouseEncoding),
    /// Mode off or Shift held: the gesture is the terminal's, a selection
    /// starts.
    ///
    /// The arm does **not** carry Shift and doesn't need to: Shift's second
    /// meaning ("extend the existing selection", `Session::extend_selection`)
    /// is independent of the mode and `bt-shell`'s gesture ledger reads it.
    /// Shift+click in mouse mode landing here is therefore also the extension
    /// — there Shift is already the selection's only path.
    Select,
}

/// The button's route, from the mode and Shift.
///
/// **Asymmetric with the wheel, and the asymmetry is deliberate:** in
/// `wheel_route` Shift does not override mouse mode
/// (`mouse_mode_comes_first_on_either_screen`), here it does. The reason is
/// that the things competing in the two arms differ — on the wheel there is
/// no second consumer to stack on Shift (scrolling is the terminal's anyway)
/// and macOS turns Shift+wheel into a horizontal delta on a classic mouse, so
/// Shift on that arm is unreliable already; on the button there are two real
/// consumers (the application's mouse and the user's selection) and Shift is
/// the **only** escape path. xterm's convention; iTerm2, kitty, WezTerm and
/// ghostty do the same. Its guard is `shift_overrides_the_button_but_not_the_wheel`.
pub(crate) fn button_route(mode: TermMode, shift: bool) -> ButtonRoute {
    // `intersects`, not `contains` — `wheel_route`'s written rationale.
    if mode.intersects(TermMode::MOUSE_MODE) && !shift {
        ButtonRoute::Report(mouse_encoding(mode))
    } else {
        ButtonRoute::Select
    }
}

/// Motion's route, from the mode and the held button. The answer is **not**
/// [`ButtonRoute`] but an `Option`: motion starts no gesture, so there is no
/// "select" arm — if no report is wanted the event is dropped and the mouse
/// carries on with its current job (moving the selection, or nothing).
///
/// The three modes **part ways** here and `MOUSE_MODE` can't be asked as a
/// union: 1003 (`MOUSE_MOTION`) wants every motion, 1002 (`MOUSE_DRAG`) only
/// the pressed ones, 1000 (`MOUSE_REPORT_CLICK`) none. On the button path the
/// three gave the same answer ([`button_route`]'s single `intersects`), here
/// they don't.
///
/// **Shift is not asked**: the route is locked at press
/// ([`crate::Session::mouse_button`]) and doesn't change mid-gesture; and
/// motion with no button has no gesture anyway.
pub(crate) fn motion_route(mode: TermMode, pressed: bool) -> Option<MouseEncoding> {
    let wanted =
        mode.contains(TermMode::MOUSE_MOTION) || (pressed && mode.contains(TermMode::MOUSE_DRAG));
    wanted.then(|| mouse_encoding(mode))
}

/// The wheel's decision table — the order is the
/// table's order and the same as alacritty's `scroll_terminal`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WheelRoute {
    /// The application asked for mouse reports (1000/1002/1003), whichever
    /// screen.
    Report(MouseEncoding),
    /// Alternate screen + DECSET 1007, Shift not held: arrow key (the format
    /// is read from the mode by [`arrow`]).
    Arrows,
    /// Alternate screen, but 1007 is off or Shift is held.
    Ignore,
    /// Primary screen: the visible window scrolls.
    Scroll,
}

/// The wheel's route, from the mode and Shift.
pub(crate) fn wheel_route(mode: TermMode, shift: bool) -> WheelRoute {
    // `intersects`, not `contains`: `MOUSE_MODE` is the union of three bits
    // and the application usually turns on only one (`\e[?1000h`).
    if mode.intersects(TermMode::MOUSE_MODE) {
        WheelRoute::Report(mouse_encoding(mode))
    } else if !mode.contains(TermMode::ALT_SCREEN) {
        WheelRoute::Scroll
    } else if mode.contains(TermMode::ALTERNATE_SCROLL) && !shift {
        WheelRoute::Arrows
    } else {
        WheelRoute::Ignore
    }
}

/// The report of a single mouse event: the wheel's press as well as the
/// button's press/release. `col`/`row` are 0-based and in the **application's**
/// screen (grid row, not the visible window). `None` for a coordinate the
/// encoding doesn't fit — no report is sent, and none goes to a clamped cell
/// either; clamping on release is the caller's job ([`MouseEncoding::clamp`]).
///
/// **The two encodings say release in two ways.** SGR's final byte is `m`
/// instead of `M` and the button code is kept, so the application knows
/// which button was released. X10/UTF-8 has no such place: release is said by
/// writing the button bits as `3` and **which** button it was is lost. That
/// is not a deficiency but the protocol's own limit; the modifier bits are
/// kept.
pub(crate) fn mouse_report(
    encoding: MouseEncoding,
    button: u8,
    pressed: bool,
    col: u16,
    row: u16,
) -> Option<Vec<u8>> {
    let Some(limit) = encoding.limit() else {
        // `u32`: so that `u16::MAX + 1` doesn't overflow.
        let (col, row) = (u32::from(col) + 1, u32::from(row) + 1);
        let last = if pressed { 'M' } else { 'm' };
        return Some(format!("\x1b[<{button};{col};{row}{last}").into_bytes());
    };
    if col >= limit || row >= limit {
        return None;
    }
    let utf8 = encoding == MouseEncoding::Utf8;
    // The button bits (low two) become `3`, the modifiers and the wheel bit
    // stay as they are.
    let button = if pressed {
        button
    } else {
        (button & !0b11) | 3
    };
    let mut report = vec![0x1b, b'[', b'M', 32 + button];
    for pos in [col, row] {
        let value = 32 + 1 + u32::from(pos);
        if utf8 {
            // A single byte below `128`, two above — UTF-8 itself
            // (alacritty writes it by hand as `0xC0 + v/64`, `0x80 + v&63`).
            // There are no surrogate code points in the range, `from_u32`
            // doesn't drop.
            let mut buf = [0; 4];
            report.extend_from_slice(char::from_u32(value)?.encode_utf8(&mut buf).as_bytes());
        } else {
            // `limit` above guarantees `value <= 255`.
            report.push(value as u8);
        }
    }
    Some(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_follows_decckm() {
        let (csi, ss3) = (TermMode::empty(), TermMode::APP_CURSOR);
        assert_eq!(&arrow(Arrow::Up, csi), b"\x1b[A");
        assert_eq!(&arrow(Arrow::Down, csi), b"\x1b[B");
        assert_eq!(&arrow(Arrow::Right, csi), b"\x1b[C");
        assert_eq!(&arrow(Arrow::Left, csi), b"\x1b[D");
        assert_eq!(&arrow(Arrow::Up, ss3), b"\x1bOA");
        assert_eq!(&arrow(Arrow::Down, ss3), b"\x1bOB");
        assert_eq!(&arrow(Arrow::Right, ss3), b"\x1bOC");
        assert_eq!(&arrow(Arrow::Left, ss3), b"\x1bOD");
    }

    #[test]
    fn mouse_mode_comes_first_on_either_screen() {
        // `MOUSE_MODE` is the **union** of three bits: `contains` wants all
        // three at once and would take the wrong arm for an application that
        // turns on only `\e[?1000h`. Each bit is tested on its own.
        for bit in [
            TermMode::MOUSE_REPORT_CLICK,
            TermMode::MOUSE_DRAG,
            TermMode::MOUSE_MOTION,
        ] {
            for screen in [TermMode::empty(), TermMode::ALT_SCREEN] {
                // 1007 and Shift do not override mouse mode.
                let mode = bit | screen | TermMode::ALTERNATE_SCROLL;
                for shift in [false, true] {
                    assert_eq!(
                        wheel_route(mode, shift),
                        WheelRoute::Report(MouseEncoding::Normal),
                        "{mode:?} shift={shift}"
                    );
                }
            }
        }
    }

    #[test]
    fn shift_overrides_the_button_but_not_the_wheel() {
        // The set's **asymmetry** and its guard deliberately sit next to the
        // wheel's: `mouse_mode_comes_first_on_either_screen` says "Shift does
        // not override mouse mode" and that sentence belongs **to the wheel
        // only**. Whoever applies the rule to the button too breaks this test;
        // if it didn't break, the ability to select text with the mouse inside
        // an application would silently die.
        for bit in [
            TermMode::MOUSE_REPORT_CLICK,
            TermMode::MOUSE_DRAG,
            TermMode::MOUSE_MOTION,
        ] {
            for screen in [TermMode::empty(), TermMode::ALT_SCREEN] {
                let mode = bit | screen | TermMode::ALTERNATE_SCROLL;
                assert_eq!(
                    button_route(mode, false),
                    ButtonRoute::Report(MouseEncoding::Normal),
                    "{mode:?}"
                );
                // Button: Shift gives the terminal back.
                assert_eq!(button_route(mode, true), ButtonRoute::Select, "{mode:?}");
                // Wheel: in the same mode, with the same Shift, the report stays.
                assert_eq!(
                    wheel_route(mode, true),
                    WheelRoute::Report(MouseEncoding::Normal),
                    "{mode:?}"
                );
            }
        }
    }

    #[test]
    fn button_selects_whenever_no_mouse_mode_is_set() {
        // With the mode off Shift has no effect: selection on both arms.
        // Alternate screen and 1007 don't concern the button at all — the
        // wheel's arrows/ignore branches are **not** here, because the button
        // has no third place to go.
        for mode in [
            TermMode::empty(),
            TermMode::ALT_SCREEN,
            TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL,
            TermMode::SGR_MOUSE,
            TermMode::APP_CURSOR,
        ] {
            for shift in [false, true] {
                assert_eq!(
                    button_route(mode, shift),
                    ButtonRoute::Select,
                    "{mode:?} shift={shift}"
                );
            }
        }
    }

    #[test]
    fn button_and_wheel_share_the_encoding_table() {
        // `mouse_encoding` lives in one place; both paths read from it.
        let click = TermMode::MOUSE_REPORT_CLICK;
        for (mode, encoding) in [
            (click, MouseEncoding::Normal),
            (click | TermMode::SGR_MOUSE, MouseEncoding::Sgr),
            (click | TermMode::UTF8_MOUSE, MouseEncoding::Utf8),
            (
                click | TermMode::SGR_MOUSE | TermMode::UTF8_MOUSE,
                MouseEncoding::Sgr,
            ),
        ] {
            assert_eq!(button_route(mode, false), ButtonRoute::Report(encoding));
            assert_eq!(wheel_route(mode, false), WheelRoute::Report(encoding));
        }
    }

    #[test]
    fn modifier_bits_are_meta_and_control_never_shift() {
        let (left, right) = (MouseButton::Left, MouseButton::Right);
        let none = MouseModifiers::default();
        assert_eq!(button_byte(left, none), 0);
        assert_eq!(button_byte(MouseButton::Middle, none), 1);
        assert_eq!(button_byte(right, none), 2);
        let meta = MouseModifiers { meta: true, ..none };
        let control = MouseModifiers {
            control: true,
            ..none
        };
        assert_eq!(button_byte(left, meta), 8);
        assert_eq!(button_byte(left, control), 16);
        assert_eq!(
            button_byte(
                right,
                MouseModifiers {
                    meta: true,
                    control: true,
                    shift: true,
                }
            ),
            // 2 | 8 | 16 — Shift's 4 is **absent**: that bit is never set,
            // because a Shift event is sent to selection in `button_route` and
            // never reaches the report.
            26
        );
    }

    #[test]
    fn motion_route_splits_the_three_mouse_modes() {
        // On the button path the three bits give the same answer
        // (`mouse_mode_comes_first_on_either_screen`); on the motion path they
        // part ways and the difference is the mode's **meaning**: 1000 click,
        // 1002 drag, 1003 every motion.
        let (click, drag, motion) = (
            TermMode::MOUSE_REPORT_CLICK,
            TermMode::MOUSE_DRAG,
            TermMode::MOUSE_MOTION,
        );
        let normal = Some(MouseEncoding::Normal);
        // 1000: no motion.
        assert_eq!(motion_route(click, false), None);
        assert_eq!(motion_route(click, true), None);
        // 1002: only while pressed.
        assert_eq!(motion_route(drag, false), None);
        assert_eq!(motion_route(drag, true), normal);
        // 1003: always.
        assert_eq!(motion_route(motion, false), normal);
        assert_eq!(motion_route(motion, true), normal);
        // With no mode, none either.
        assert_eq!(motion_route(TermMode::empty(), true), None);
        // The encoding comes from the same table as the button's.
        assert_eq!(
            motion_route(motion | TermMode::SGR_MOUSE, false),
            Some(MouseEncoding::Sgr)
        );
    }

    #[test]
    fn motion_sets_bit_thirtytwo_and_three_without_a_button() {
        let none = MouseModifiers::default();
        // Motion with no button: `32 | 3`.
        assert_eq!(motion_byte(None, none), 35);
        // Held button: `32` plus the button's code.
        assert_eq!(motion_byte(Some(MouseButton::Left), none), 32);
        assert_eq!(motion_byte(Some(MouseButton::Middle), none), 33);
        assert_eq!(motion_byte(Some(MouseButton::Right), none), 34);
        // The modifiers are the same bits as in the press report; still no Shift.
        assert_eq!(
            motion_byte(
                Some(MouseButton::Left),
                MouseModifiers {
                    meta: true,
                    control: true,
                    shift: true,
                }
            ),
            32 | 8 | 16
        );
        // The same byte comes out as motion in all three encodings; `pressed =
        // true` because motion has no release form.
        assert_eq!(
            mouse_report(MouseEncoding::Sgr, 35, true, 4, 2).unwrap(),
            b"\x1b[<35;5;3M"
        );
        assert_eq!(
            mouse_report(MouseEncoding::Normal, 35, true, 4, 2).unwrap(),
            [0x1b, b'[', b'M', 32 + 35, 37, 35]
        );
        assert_eq!(
            mouse_report(MouseEncoding::Utf8, 32, true, 95, 0).unwrap(),
            [0x1b, b'[', b'M', 32 + 32, 0xc2, 0x80, 33]
        );
    }

    #[test]
    fn release_says_m_in_sgr_and_button_three_elsewhere() {
        // SGR: the button code is kept, the final byte is `m`.
        assert_eq!(
            mouse_report(MouseEncoding::Sgr, 2, false, 4, 2).unwrap(),
            b"\x1b[<2;5;3m"
        );
        assert_eq!(
            mouse_report(MouseEncoding::Sgr, 2, true, 4, 2).unwrap(),
            b"\x1b[<2;5;3M"
        );
        // X10/UTF-8: the button bits are `3`, so **which** button it was is
        // lost; the modifiers stay.
        assert_eq!(
            mouse_report(MouseEncoding::Normal, 2, false, 4, 2).unwrap(),
            [0x1b, b'[', b'M', 32 + 3, 37, 35]
        );
        assert_eq!(
            mouse_report(MouseEncoding::Normal, 2 | 16, false, 4, 2).unwrap(),
            [0x1b, b'[', b'M', 32 + 3 + 16, 37, 35]
        );
        assert_eq!(
            mouse_report(MouseEncoding::Utf8, 1, false, 95, 0).unwrap(),
            [0x1b, b'[', b'M', 32 + 3, 0xc2, 0x80, 33]
        );
    }

    #[test]
    fn clamp_lands_on_the_last_coordinate_the_encoding_accepts() {
        // The clamped value must be an **accepted** value: `mouse_report` must
        // not reject the same number, or the release would be dropped again.
        for encoding in [MouseEncoding::Normal, MouseEncoding::Utf8] {
            let far = encoding.clamp(u16::MAX);
            assert_eq!(far, encoding.limit().unwrap() - 1);
            assert!(mouse_report(encoding, 0, false, far, far).is_some());
            // A coordinate that fits passes the clamp untouched.
            assert_eq!(encoding.clamp(7), 7);
        }
        // SGR is unbounded: clamping is the identity.
        assert_eq!(MouseEncoding::Sgr.clamp(u16::MAX), u16::MAX);
    }

    #[test]
    fn mouse_encoding_follows_the_mode() {
        let click = TermMode::MOUSE_REPORT_CLICK;
        assert_eq!(
            wheel_route(click | TermMode::SGR_MOUSE, false),
            WheelRoute::Report(MouseEncoding::Sgr)
        );
        assert_eq!(
            wheel_route(click | TermMode::UTF8_MOUSE, false),
            WheelRoute::Report(MouseEncoding::Utf8)
        );
        // alacritty sets the two as mutually exclusive; if both came at once
        // SGR wins (alacritty's `mouse_report` asks for SGR first too).
        assert_eq!(
            wheel_route(click | TermMode::SGR_MOUSE | TermMode::UTF8_MOUSE, false),
            WheelRoute::Report(MouseEncoding::Sgr)
        );
    }

    #[test]
    fn alternate_screen_turns_the_wheel_into_arrows() {
        let alt = TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL;
        assert_eq!(wheel_route(alt, false), WheelRoute::Arrows);
        // DECCKM doesn't change the route, only the arrow's format (`arrow`).
        assert_eq!(
            wheel_route(alt | TermMode::APP_CURSOR, false),
            WheelRoute::Arrows
        );
        // Shift and `\e[?1007l` cut the arrow; it doesn't fall to the primary
        // screen either.
        assert_eq!(wheel_route(alt, true), WheelRoute::Ignore);
        assert_eq!(wheel_route(TermMode::ALT_SCREEN, false), WheelRoute::Ignore);
    }

    #[test]
    fn primary_screen_scrolls_whatever_else_is_set() {
        // 1007 is meaningful only on the alternate screen; Shift doesn't change
        // scrolling on the primary screen (alacritty is the same).
        for mode in [
            TermMode::empty(),
            TermMode::ALTERNATE_SCROLL,
            TermMode::ALTERNATE_SCROLL | TermMode::APP_CURSOR,
        ] {
            for shift in [false, true] {
                assert_eq!(wheel_route(mode, shift), WheelRoute::Scroll, "{mode:?}");
            }
        }
    }

    #[test]
    fn sgr_report_is_decimal_and_one_based() {
        assert_eq!(
            mouse_report(MouseEncoding::Sgr, WHEEL_UP, true, 4, 2).unwrap(),
            b"\x1b[<64;5;3M"
        );
        assert_eq!(
            mouse_report(MouseEncoding::Sgr, WHEEL_DOWN, true, 0, 0).unwrap(),
            b"\x1b[<65;1;1M"
        );
        // There is no limit and `+ 1` doesn't overflow at the top of `u16`.
        assert_eq!(
            mouse_report(MouseEncoding::Sgr, WHEEL_UP, true, u16::MAX, 2015).unwrap(),
            b"\x1b[<64;65536;2016M"
        );
    }

    #[test]
    fn normal_report_is_one_byte_per_coordinate_up_to_222() {
        // `32 + button`, `32 + 1 + position`.
        assert_eq!(
            mouse_report(MouseEncoding::Normal, WHEEL_UP, true, 4, 2).unwrap(),
            [0x1b, b'[', b'M', 96, 37, 35]
        );
        // 222 is the last that fits: `32 + 1 + 222 = 255`.
        assert_eq!(
            mouse_report(MouseEncoding::Normal, WHEEL_DOWN, true, 222, 222).unwrap(),
            [0x1b, b'[', b'M', 97, 255, 255]
        );
        // 223 doesn't fit a byte: no report is sent — in the column or the row.
        assert_eq!(
            mouse_report(MouseEncoding::Normal, WHEEL_UP, true, 223, 0),
            None
        );
        assert_eq!(
            mouse_report(MouseEncoding::Normal, WHEEL_UP, true, 0, 223),
            None
        );
    }

    #[test]
    fn utf8_report_takes_two_bytes_from_95() {
        // 94 → `32 + 1 + 94 = 127`, one byte; 95 → 128, two bytes.
        assert_eq!(
            mouse_report(MouseEncoding::Utf8, WHEEL_UP, true, 94, 0).unwrap(),
            [0x1b, b'[', b'M', 96, 127, 33]
        );
        assert_eq!(
            mouse_report(MouseEncoding::Utf8, WHEEL_UP, true, 95, 95).unwrap(),
            [0x1b, b'[', b'M', 96, 0xc2, 0x80, 0xc2, 0x80]
        );
        // 2014 is the last that fits: `32 + 1 + 2014 = 2047`, the top of a
        // two-byte UTF-8.
        assert_eq!(
            mouse_report(MouseEncoding::Utf8, WHEEL_DOWN, true, 2014, 0).unwrap(),
            [0x1b, b'[', b'M', 97, 0xdf, 0xbf, 33]
        );
        assert_eq!(
            mouse_report(MouseEncoding::Utf8, WHEEL_UP, true, 2015, 0),
            None
        );
        assert_eq!(
            mouse_report(MouseEncoding::Utf8, WHEEL_UP, true, 0, 2015),
            None
        );
        // The plain mode's limit doesn't apply in UTF-8.
        assert!(mouse_report(MouseEncoding::Utf8, WHEEL_UP, true, 223, 0).is_some());
    }
}
