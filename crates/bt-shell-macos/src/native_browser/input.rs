//! What a person's keys, pointer and wheel are to the engine: the numbers its protocol speaks
//! (Windows virtual keys, the modifier bits, the shapes), from AppKit's. Pure, so it is tested
//! without a window.
//!
//! The keys are sent **as keys**: `keydown` for the page's own scripts, then the character, then
//! `keyup`, and the input method's text only where one is composing. A page that listens for keys
//! (an editor, a game, a form that validates as it is typed) hears a typist; one that is sent
//! only text hears a paste. The macOS editing commands (⌘← and the rest) cannot go as commands:
//! the protocol has no way to carry one, and the keys that stand for them in the engine's table
//! (Home for ⌘←) scroll a page that has no field, so that a key the page binds (browserpen's ⌘←
//! is back) would never come back to be the page's. The measurement is the test
//! `the_real_engine_edits_text_with_these_keys_when_one_is_at_hand`.

use browser_ipc::message::{Button, Cursor, modifier};
use bt_shell_common::native_view::Button as Pressed;
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

/// How far the engine scrolls for what the wheel says, in the engine's points: the pane's own
/// reading of the event is the page's (a line is a dozen points there, a trackpad's own), and a
/// line of a wheel is about forty points in a browser. The sign is the engine's, up positive.
pub(super) fn wheel_of(dx: f32, dy: f32, precise: bool) -> (i32, i32) {
    let scale = if precise { 1.0 } else { 40.0 / 12.0 };
    #[allow(clippy::cast_possible_truncation)]
    let part = |d: f32| (-d * scale).round() as i32;
    (part(dx), part(dy))
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
    #[allow(deprecated)]
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
        Cursor::ColumnResize | Cursor::ResizeEastWest => NSCursor::resizeLeftRightCursor(),
        Cursor::RowResize | Cursor::ResizeNorthSouth => NSCursor::resizeUpDownCursor(),
        _ => NSCursor::arrowCursor(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
