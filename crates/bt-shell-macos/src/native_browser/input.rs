//! What a person's keys, pointer and wheel are to the engine: the numbers its protocol speaks
//! (Windows virtual keys, the modifier bits, the shapes), from AppKit's. Pure, so it is tested
//! without a window.
//!
//! The keys are sent **as keys**: `keydown` for the page's own scripts, then the character, then
//! `keyup`, and the input method's text only where one is composing. A page that listens for keys
//! (an editor, a game, a form that validates as it is typed) hears a typist; one that is sent
//! only text hears a paste. The macOS editing commands (⌘← and the rest) ride with the key's
//! `keydown` as the names the system's text handling gave them (`doCommandBySelector:`), as
//! Chrome sends them ([`edit_command`]): the engine runs them where a field has the keyboard and
//! the page did not stop the key, and a key it did not use comes back as before, so that a key the
//! page binds (browserpen's ⌘← is back) is still the page's outside a field. The measurement is
//! the test `the_real_engine_edits_text_with_these_keys_when_one_is_at_hand`.

use browser_ipc::message::{Button, Cursor, EditAction, PinchKind, modifier};
use bt_shell_common::native_view::{Button as Pressed, GesturePhase};
use objc2::rc::Retained;
use objc2_app_kit::{NSCursor, NSEventModifierFlags};

/// The Windows virtual key of the key at the Mac's key code `code`, which is how the engine names
/// the key: the key's place on a US keyboard for the letters and the punctuation (a Turkish `ş`
/// is where `;` is), and the letter itself where the layout puts an ASCII one (Dvorak's).
pub(super) fn vk_of(code: u16, ignoring: &str) -> i32 {
    let ascii = ignoring.chars().next().filter(char::is_ascii_alphanumeric);
    if let Some(c) = ascii {
        return i32::from(c.to_ascii_uppercase() as u8);
    }
    match code {
        // Letters and digits by their place.
        0 => 65,
        11 => 66,
        8 => 67,
        2 => 68,
        14 => 69,
        3 => 70,
        5 => 71,
        4 => 72,
        34 => 73,
        38 => 74,
        40 => 75,
        37 => 76,
        46 => 77,
        45 => 78,
        31 => 79,
        35 => 80,
        12 => 81,
        15 => 82,
        1 => 83,
        17 => 84,
        32 => 85,
        9 => 86,
        13 => 87,
        7 => 88,
        16 => 89,
        6 => 90,
        18 => 49,
        19 => 50,
        20 => 51,
        21 => 52,
        23 => 53,
        22 => 54,
        26 => 55,
        28 => 56,
        25 => 57,
        29 => 48,
        // Whitespace, editing and movement.
        36 | 76 => 13,
        48 => 9,
        49 => 32,
        51 => 8,
        53 => 27,
        117 => 46,
        123 => 37,
        126 => 38,
        124 => 39,
        125 => 40,
        115 => 36,
        119 => 35,
        116 => 33,
        121 => 34,
        // F1 to F12.
        122 => 112,
        120 => 113,
        99 => 114,
        118 => 115,
        96 => 116,
        97 => 117,
        98 => 118,
        100 => 119,
        101 => 120,
        109 => 121,
        103 => 122,
        111 => 123,
        // Punctuation, the OEM keys.
        41 => 186,
        24 => 187,
        43 => 188,
        27 => 189,
        47 => 190,
        44 => 191,
        50 => 192,
        33 => 219,
        42 => 220,
        30 => 221,
        39 => 222,
        _ => 0,
    }
}

/// The modifier bits of the protocol for the flags AppKit holds.
pub(super) fn mods_of(flags: NSEventModifierFlags) -> u32 {
    let mut mods = 0;
    for (flag, bit) in [
        (NSEventModifierFlags::Shift, modifier::SHIFT),
        (NSEventModifierFlags::Control, modifier::CONTROL),
        (NSEventModifierFlags::Option, modifier::ALT),
        (NSEventModifierFlags::Command, modifier::COMMAND),
        (NSEventModifierFlags::CapsLock, modifier::CAPS_LOCK),
    ] {
        if flags.contains(flag) {
            mods |= bit;
        }
    }
    mods
}

/// The first UTF-16 unit of `text`: the character a key event of the engine carries (the arrows
/// and their kin are AppKit's private-use characters, as the engine's own events have them).
pub(super) fn unit_of(text: &str) -> u16 {
    text.encode_utf16().next().unwrap_or(0)
}

/// Whether `text` is the character of a function key (an arrow, F5, Home): AppKit makes those
/// from the private-use block, and they are keys and no text.
pub(super) fn is_function_key(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|c| ('\u{f700}'..='\u{f8ff}').contains(&c))
}

/// The keys whose press makes a character event of its own, after the key's: ⏎ and ⇥ (the page's
/// `keypress`, and a form's submit). The rest of what is typed has the character it makes.
pub(super) fn has_character(vk: i32) -> bool {
    matches!(vk, 9 | 13)
}

/// The most commands one key carries (the engine's own limit).
pub(super) const COMMANDS_MAX: usize = 8;

/// What the system's text handling named (`moveToBeginningOfLine:`) as the command the engine is
/// sent, or `None` for the ones the engine must not run: the ones that insert text (`insertTab:`,
/// `insertNewline:`: the key's text is sent as the key's own, and a tab is no tab character in a
/// field) and `noop:`. The colon goes (Chrome's names have none); anything that is not a plain
/// name is no command.
pub(super) fn edit_command(selector: &str) -> Option<String> {
    let name = selector.strip_suffix(':').unwrap_or(selector);
    let plain =
        !name.is_empty() && name.len() <= 48 && name.bytes().all(|b| b.is_ascii_alphanumeric());
    (plain && !name.starts_with("insert") && name != "noop").then(|| name.to_owned())
}

/// The command of the Edit menu's item that has `key` as its shortcut (⌘C, ⌘X, ⌘V, ⌘Z; ⌘A is
/// every other).
pub(super) fn edit_action(key: char) -> EditAction {
    match key {
        'c' => EditAction::Copy,
        'x' => EditAction::Cut,
        'v' => EditAction::Paste,
        'z' => EditAction::Undo,
        _ => EditAction::SelectAll,
    }
}

/// Whether the event that ran the Edit menu's item for `item` (`c`, `x`, `v`, `a`, `z`) was the
/// item's own key: ⌘ and that key, nothing else held. A click on the row has no key; ⇧⌘Z is not
/// Undo's.
pub(super) fn is_key_of(
    item: char,
    ignoring: Option<&str>,
    flags: objc2_app_kit::NSEventModifierFlags,
) -> bool {
    use objc2_app_kit::NSEventModifierFlags as Flag;
    flags.contains(Flag::Command)
        && !flags.intersects(Flag::Shift | Flag::Option | Flag::Control)
        && ignoring.is_some_and(|text| {
            let mut chars = text.chars();
            chars.next().is_some_and(|c| c.eq_ignore_ascii_case(&item)) && chars.next().is_none()
        })
}

/// A length in points, rounded to the whole points the engine counts in. `as` from a float
/// saturates, and a length on a screen is nowhere near what an `i32` holds.
#[allow(clippy::cast_possible_truncation)]
pub(super) fn rounded(length: f32) -> i32 {
    length.round() as i32
}

/// How far the engine scrolls for what the wheel says, in the engine's points: the pane's own
/// reading of the event is the page's (a line is a dozen points there, a trackpad's own), and a
/// line of a wheel is about forty points in a browser. The sign is the engine's, up positive.
pub(super) fn wheel_of(dx: f32, dy: f32, precise: bool) -> (i32, i32) {
    let scale = if precise { 1.0 } else { 40.0 / 12.0 };
    (rounded(-dx * scale), rounded(-dy * scale))
}

/// What one `magnifyWithEvent:` is to the engine's pinch: `by` is AppKit's `magnification`, the
/// change since the event before (0.02 is two percent), and the engine takes the factor of the
/// change (`1 + by`, never less than a hundredth so that a wild reading cannot flip a page's
/// zoom). An event that changes nothing says nothing but the ends of the gesture.
pub(super) fn pinch_of(phase: GesturePhase, by: f32) -> Vec<(PinchKind, f64)> {
    let change =
        (by != 0.0 && by.is_finite()).then(|| (PinchKind::Update, (1.0 + f64::from(by)).max(0.01)));
    match phase {
        GesturePhase::Begin => [Some((PinchKind::Begin, 1.0)), change]
            .into_iter()
            .flatten()
            .collect(),
        GesturePhase::Move => change.into_iter().collect(),
        GesturePhase::End => vec![(PinchKind::End, 1.0)],
    }
}

pub(super) fn button_of(button: Pressed) -> Button {
    match button {
        Pressed::Left => Button::Left,
        Pressed::Right => Button::Right,
        Pressed::Middle => Button::Middle,
    }
}

/// The modifier bit of a button held down: what a drag carries.
pub(super) fn held_bit(button: Pressed) -> u32 {
    match button {
        Pressed::Left => modifier::LEFT_BUTTON,
        Pressed::Right => modifier::RIGHT_BUTTON,
        Pressed::Middle => modifier::MIDDLE_BUTTON,
    }
}

/// The pointer the page asks for, as AppKit has it.
pub(super) fn cursor_of(cursor: Cursor) -> Retained<NSCursor> {
    match cursor {
        Cursor::Hand => NSCursor::pointingHandCursor(),
        Cursor::IBeam | Cursor::VerticalText => NSCursor::IBeamCursor(),
        Cursor::Cross | Cursor::Cell => NSCursor::crosshairCursor(),
        Cursor::NotAllowed => NSCursor::operationNotAllowedCursor(),
        Cursor::Grab => NSCursor::openHandCursor(),
        Cursor::Grabbing | Cursor::Move => NSCursor::closedHandCursor(),
        Cursor::Copy => NSCursor::dragCopyCursor(),
        Cursor::Alias => NSCursor::dragLinkCursor(),
        Cursor::ContextMenu => NSCursor::contextualMenuCursor(),
        // The two resize cursors' replacements are macOS 15's; bateri runs on 14.
        #[allow(deprecated)]
        Cursor::ColumnResize | Cursor::ResizeEastWest => NSCursor::resizeLeftRightCursor(),
        #[allow(deprecated)]
        Cursor::RowResize | Cursor::ResizeNorthSouth => NSCursor::resizeUpDownCursor(),
        _ => NSCursor::arrowCursor(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pinch_is_a_begin_the_changes_and_an_end() {
        use PinchKind::{Begin, End, Update};
        // The fingers come (AppKit says no change yet), spread, and go.
        assert_eq!(pinch_of(GesturePhase::Begin, 0.0), [(Begin, 1.0)]);
        assert_eq!(
            pinch_of(GesturePhase::Begin, 0.5),
            [(Begin, 1.0), (Update, 1.5)]
        );
        assert_eq!(pinch_of(GesturePhase::Move, 0.25), [(Update, 1.25)]);
        assert_eq!(pinch_of(GesturePhase::Move, -0.5), [(Update, 0.5)]);
        assert_eq!(pinch_of(GesturePhase::End, 0.0), [(End, 1.0)]);
        // No change is no message, and a reading that cannot be a pinch is held.
        assert_eq!(pinch_of(GesturePhase::Move, 0.0), []);
        assert_eq!(pinch_of(GesturePhase::Move, f32::NAN), []);
        assert_eq!(pinch_of(GesturePhase::Move, -3.0), [(Update, 0.01)]);
    }

    #[test]
    fn the_edit_menus_items_are_the_engines_commands() {
        assert_eq!(edit_action('c'), EditAction::Copy);
        assert_eq!(edit_action('x'), EditAction::Cut);
        assert_eq!(edit_action('v'), EditAction::Paste);
        assert_eq!(edit_action('z'), EditAction::Undo);
        assert_eq!(edit_action('a'), EditAction::SelectAll);
    }

    #[test]
    fn an_edit_item_is_run_by_its_key_only_when_the_event_is_that_key() {
        use objc2_app_kit::NSEventModifierFlags as Flag;
        assert!(is_key_of('z', Some("z"), Flag::Command));
        assert!(is_key_of('c', Some("C"), Flag::Command | Flag::CapsLock));
        // A click on the row (no characters), another key, ⇧⌘Z, ⌥⌘Z, ⌃⌘Z and a bare z are not.
        assert!(!is_key_of('z', None, Flag::Command));
        assert!(!is_key_of('z', Some("x"), Flag::Command));
        assert!(!is_key_of('z', Some("z"), Flag::Command | Flag::Shift));
        assert!(!is_key_of('z', Some("z"), Flag::Command | Flag::Option));
        assert!(!is_key_of('z', Some("z"), Flag::Command | Flag::Control));
        assert!(!is_key_of('z', Some("z"), Flag::empty()));
        assert!(!is_key_of('z', Some("zz"), Flag::Command));
    }

    #[test]
    fn the_commands_the_system_names_are_sent_as_chrome_sends_them() {
        assert_eq!(
            edit_command("moveToBeginningOfLine:").as_deref(),
            Some("moveToBeginningOfLine")
        );
        assert_eq!(
            edit_command("deleteWordBackward:").as_deref(),
            Some("deleteWordBackward")
        );
        assert_eq!(edit_command("selectAll").as_deref(), Some("selectAll"));
        // Text the key makes is the key's; nothing is a command.
        for not in [
            "insertTab:",
            "insertNewline:",
            "insertText:",
            "noop:",
            "noop",
            "",
            ":",
        ] {
            assert_eq!(edit_command(not), None, "{not}");
        }
        // A selector the engine would not take as a name.
        assert_eq!(edit_command("move:to:"), None);
        assert_eq!(edit_command(&"a".repeat(49)), None);
    }

    #[test]
    fn letters_digits_and_the_keys_beside_them_are_the_engines_virtual_keys() {
        // A letter is the letter the layout makes (Dvorak's `'` key makes a `q`)...
        assert_eq!(vk_of(0, "a"), 65);
        assert_eq!(vk_of(12, "'"), 81, "the key where Q is on a US keyboard");
        assert_eq!(vk_of(12, "q"), 81);
        assert_eq!(vk_of(18, "1"), 49);
        // ...a letter of another script is where the key is.
        assert_eq!(vk_of(41, "ş"), 186);
        assert_eq!(vk_of(0, "ğ"), 65);
        assert_eq!(vk_of(36, "\r"), 13);
        assert_eq!(vk_of(48, "\t"), 9);
        assert_eq!(vk_of(49, " "), 32);
        assert_eq!(vk_of(51, "\u{7f}"), 8);
        assert_eq!(vk_of(53, "\u{1b}"), 27);
        assert_eq!(vk_of(123, "\u{f702}"), 37);
        assert_eq!(vk_of(126, "\u{f700}"), 38);
        assert_eq!(vk_of(122, "\u{f704}"), 112);
        assert_eq!(vk_of(111, "\u{f70f}"), 123);
        assert_eq!(vk_of(255, ""), 0, "a key the table does not know is none");
    }

    #[test]
    fn modifiers_are_the_protocols_bits() {
        assert_eq!(mods_of(NSEventModifierFlags::empty()), 0);
        assert_eq!(
            mods_of(NSEventModifierFlags::Command | NSEventModifierFlags::Shift),
            modifier::COMMAND | modifier::SHIFT
        );
        assert_eq!(
            mods_of(NSEventModifierFlags::Option | NSEventModifierFlags::Control),
            modifier::ALT | modifier::CONTROL
        );
        assert_eq!(mods_of(NSEventModifierFlags::CapsLock), modifier::CAPS_LOCK);
    }

    #[test]
    fn function_keys_are_keys_and_not_text() {
        assert!(is_function_key("\u{f702}"));
        assert!(is_function_key("\u{f729}"));
        assert!(!is_function_key("a"));
        assert!(!is_function_key("ş"));
        assert!(!is_function_key(""));
        assert_eq!(unit_of("\u{f702}"), 0xf702);
        assert_eq!(unit_of("ş"), 0x15f);
        assert_eq!(unit_of(""), 0);
        assert!(has_character(13) && has_character(9));
        assert!(!has_character(65) && !has_character(8) && !has_character(27));
    }

    #[test]
    fn the_wheel_is_the_engines_way_up() {
        // The page's reading is down positive; the engine's is up positive.
        assert_eq!(wheel_of(0.0, 30.0, true), (0, -30));
        assert_eq!(wheel_of(-12.0, -12.0, true), (12, 12));
        // A wheel's line (a dozen in the page's reading) is forty.
        assert_eq!(wheel_of(0.0, 12.0, false), (0, -40));
        assert_eq!(wheel_of(0.0, 0.0, true), (0, 0));
    }
}
