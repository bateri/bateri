//! A shortcut as the file writes it ↔ the key a menu item holds, and the glyphs of it.
//!
//! The menu holds a key as a **character** and a modifier mask: `}` with ⌘ is what the user
//! reads as ⇧⌘], and `+` with ⌘ is ⌘+ (typed with Shift on some layouts, without on others).
//! The file holds the same key as `shift+cmd+]` and `cmd+plus`. The rule that joins the two is
//! here, once — [`bt_core::Chord`] keeps one form for a key whichever way it was reached, and
//! [`menu_key`] / [`chord_of`] are the two directions.
//!
//! The glyphs are not written again: [`glyphs`] hands the menu's form to the palette's
//! [`shortcut`](crate::palette::shortcut), the function every row of the palette and the menu
//! bar already read, so a shortcut is spelled one way wherever it is shown.

use bt_core::{Chord, Key, Modifier, Mods, NamedKey, Shortcut};

use crate::palette::{Modifiers, shortcut as menu_glyphs};

/// A key as a menu item holds it: the key equivalent and the modifier mask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuKey {
    /// One character: the key's, or AppKit's private code of a function key (`U+F702` is ←).
    pub key: String,
    pub modifiers: Modifiers,
}

impl From<Mods> for Modifiers {
    fn from(mods: Mods) -> Self {
        Modifiers {
            control: mods.ctrl,
            option: mods.option,
            shift: mods.shift,
            command: mods.cmd,
        }
    }
}

/// Function keys F1 to F12 are `U+F704` to `U+F70F` (`NSF1FunctionKey` …).
const FUNCTION_KEY_BASE: u32 = 0xF703;

/// The character a menu item spells a named key as.
fn named_char(key: NamedKey) -> char {
    match key {
        NamedKey::Up => '\u{F700}',
        NamedKey::Down => '\u{F701}',
        NamedKey::Left => '\u{F702}',
        NamedKey::Right => '\u{F703}',
        NamedKey::Delete => '\u{F728}',
        NamedKey::Home => '\u{F729}',
        NamedKey::End => '\u{F72B}',
        NamedKey::PageUp => '\u{F72C}',
        NamedKey::PageDown => '\u{F72D}',
        NamedKey::Tab => '\t',
        NamedKey::Return => '\r',
        NamedKey::Space => ' ',
        NamedKey::Escape => '\u{1b}',
        // `NSBackspaceCharacter`; the event of the key reports U+007F, which a menu item
        // matches with this one.
        NamedKey::Backspace => '\u{8}',
        NamedKey::F(number) => {
            char::from_u32(FUNCTION_KEY_BASE + u32::from(number)).unwrap_or('\u{F704}')
        }
    }
}

/// The named key a menu item's character is, if it is one.
fn named_of(c: char) -> Option<NamedKey> {
    Some(match c {
        '\u{F700}' => NamedKey::Up,
        '\u{F701}' => NamedKey::Down,
        '\u{F702}' => NamedKey::Left,
        '\u{F703}' => NamedKey::Right,
        '\u{F728}' => NamedKey::Delete,
        '\u{F729}' => NamedKey::Home,
        '\u{F72B}' => NamedKey::End,
        '\u{F72C}' => NamedKey::PageUp,
        '\u{F72D}' => NamedKey::PageDown,
        '\t' => NamedKey::Tab,
        // U+0003 is the numeric keypad's Enter.
        '\r' | '\u{3}' => NamedKey::Return,
        ' ' => NamedKey::Space,
        '\u{1b}' => NamedKey::Escape,
        '\u{8}' | '\u{7f}' => NamedKey::Backspace,
        function @ '\u{F704}'..='\u{F70F}' => {
            NamedKey::F(u8::try_from(u32::from(function) - FUNCTION_KEY_BASE).ok()?)
        }
        _ => return None,
    })
}

/// The key a menu item holds for `chord`.
pub fn menu_key(chord: &Chord) -> MenuKey {
    match chord.key {
        Key::Named(named) => MenuKey {
            key: named_char(named).to_string(),
            modifiers: chord.mods.into(),
        },
        Key::Char(c) => match chord.menu_symbol() {
            // `shift+cmd+]` is `}` with ⌘: the symbol carries the Shift.
            Some((symbol, mods)) => MenuKey {
                key: symbol.to_string(),
                modifiers: mods.into(),
            },
            None => MenuKey {
                key: c.to_string(),
                modifiers: chord.mods.into(),
            },
        },
    }
}

/// The chord a menu item's key equivalent and mask are — `None` for an item with no key, and
/// for a key that is no key by itself.
pub fn chord_of(key: &str, modifiers: Modifiers) -> Option<Chord> {
    let mut chars = key.chars();
    let (c, None) = (chars.next()?, chars.next()) else {
        return None;
    };
    let mods = Mods::new(
        modifiers.control,
        modifiers.option,
        modifiers.shift,
        modifiers.command,
    );
    let key = match named_of(c) {
        Some(named) => Key::Named(named),
        None => Key::of_char(c)?,
    };
    Some(Chord::new(mods, key))
}

/// ⌃⌥⇧⌘ — the modifiers in the order macOS writes them.
fn modifier_glyph(modifier: Modifier) -> char {
    match modifier {
        Modifier::Ctrl => '⌃',
        Modifier::Option => '⌥',
        Modifier::Shift => '⇧',
        Modifier::Cmd => '⌘',
    }
}

/// The key a family's item shows with the family's modifiers: `⌘3`, `⌃⌘0`, `⌥1`. The tab strip's
/// hint, the digit of a row in the palette and the groups' keys are written by this.
pub fn digit_glyphs(mods: Mods, digit: u8) -> String {
    let mut text: String = mods.iter().map(modifier_glyph).collect();
    text.push_str(&digit.to_string());
    text
}

/// The glyphs a shortcut is shown as: `⇧⌘D`, `⌥⇧⌘[`, `⌃⇥`, `F5`. Only modifiers (a family's)
/// give just the modifiers (`⌥⌘`), and a double press its key twice (`⇧ ⇧`). `None` for a chord
/// that is no shortcut a user could press (a letter with no modifier).
pub fn glyphs(shortcut: &Shortcut) -> Option<String> {
    match shortcut {
        Shortcut::Chord(chord) => {
            let held = menu_key(chord);
            menu_glyphs(&held.key, held.modifiers)
        }
        Shortcut::Mods(mods) => Some(mods.iter().map(modifier_glyph).collect()),
        Shortcut::Twice(modifier) => {
            let glyph = modifier_glyph(*modifier);
            Some(format!("{glyph} {glyph}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Chord {
        match Shortcut::parse(text) {
            Ok(Shortcut::Chord(chord)) => chord,
            other => panic!("`{text}` is not a chord: {other:?}"),
        }
    }

    #[test]
    fn a_shifted_symbol_is_the_menus_symbol_with_no_shift() {
        let held = menu_key(&parse("shift+cmd+]"));
        assert_eq!(held.key, "}");
        assert_eq!(held.modifiers, Modifiers::COMMAND);
        let held = menu_key(&parse("option+shift+cmd+["));
        assert_eq!(held.key, "{");
        assert_eq!(
            held.modifiers,
            Modifiers {
                option: true,
                command: true,
                ..Modifiers::default()
            }
        );
        let held = menu_key(&parse("shift+cmd+\\"));
        assert_eq!(held.key, "|");
        // Shift on a key that is not one of the three stays in the mask: ⇧⌘D.
        let held = menu_key(&parse("shift+cmd+d"));
        assert_eq!(held.key, "d");
        assert!(held.modifiers.shift && held.modifiers.command);
        // The plus key is what the menu shows, ⌘+.
        let held = menu_key(&parse("cmd+plus"));
        assert_eq!(
            (held.key.as_str(), held.modifiers),
            ("+", Modifiers::COMMAND)
        );
    }

    #[test]
    fn a_menu_key_is_the_chord_it_was_made_from() {
        for text in [
            "shift+cmd+]",
            "option+shift+cmd+[",
            "shift+cmd+\\",
            "cmd+plus",
            "cmd+-",
            "ctrl+cmd+=",
            "cmd+home",
            "cmd+end",
            "cmd+pageup",
            "cmd+pagedown",
            "option+cmd+left",
            "ctrl+shift+tab",
            "shift+cmd+return",
            "cmd+space",
            "cmd+backspace",
            "cmd+delete",
            "cmd+escape",
            "f1",
            "f12",
            "shift+f5",
            "cmd+ş",
        ] {
            let chord = parse(text);
            let held = menu_key(&chord);
            assert_eq!(chord_of(&held.key, held.modifiers), Some(chord), "{text}");
        }
    }

    #[test]
    fn the_menu_may_spell_a_key_in_more_than_one_way() {
        // The menu's `}` with ⌘ and the event's `]` with ⇧⌘ are one chord.
        assert_eq!(
            chord_of("}", Modifiers::COMMAND),
            Some(parse("shift+cmd+]"))
        );
        // The event of ⌫ and of the keypad's Enter.
        assert_eq!(
            chord_of("\u{7f}", Modifiers::COMMAND),
            Some(parse("cmd+backspace"))
        );
        assert_eq!(
            chord_of("\u{3}", Modifiers::COMMAND),
            Some(parse("cmd+return"))
        );
        // Upper case is the same key.
        assert_eq!(chord_of("D", Modifiers::COMMAND), Some(parse("cmd+d")));
        // The plus key typed with Shift is the plus key.
        assert_eq!(
            chord_of(
                "+",
                Modifiers {
                    shift: true,
                    command: true,
                    ..Modifiers::default()
                }
            ),
            Some(parse("cmd+plus"))
        );
        // No key, and no key by itself.
        assert_eq!(chord_of("", Modifiers::COMMAND), None);
        assert_eq!(chord_of("ab", Modifiers::COMMAND), None);
        assert_eq!(chord_of("\u{F8FF}", Modifiers::COMMAND), None);
    }

    #[test]
    fn the_glyphs_are_the_palettes() {
        for (text, glyphs_of) in [
            ("shift+cmd+d", "⇧⌘D"),
            ("shift+cmd+]", "⇧⌘]"),
            ("option+shift+cmd+[", "⌥⇧⌘["),
            ("shift+cmd+\\", "⇧⌘\\"),
            ("cmd+plus", "⌘+"),
            ("ctrl+tab", "⌃⇥"),
            ("ctrl+shift+tab", "⌃⇧⇥"),
            ("option+cmd+left", "⌥⌘←"),
            ("cmd+home", "⌘↖"),
            ("cmd+pageup", "⌘⇞"),
            ("shift+cmd+return", "⇧⌘↩"),
            ("cmd+backspace", "⌘⌫"),
            ("cmd+delete", "⌘⌦"),
            ("cmd+space", "⌘Space"),
            ("f5", "F5"),
            ("shift+f12", "⇧F12"),
        ] {
            let shortcut = Shortcut::parse(text).expect("a shortcut");
            assert_eq!(glyphs(&shortcut).as_deref(), Some(glyphs_of), "{text}");
        }
        // The same function the palette's rows read: a menu item's `}` with ⌘.
        assert_eq!(
            glyphs(&Shortcut::parse("shift+cmd+]").expect("a shortcut")),
            menu_glyphs("}", Modifiers::COMMAND)
        );
        // A bare letter is no shortcut a user could press.
        assert_eq!(glyphs(&Shortcut::parse("d").expect("a shortcut")), None);
        assert_eq!(
            glyphs(&Shortcut::parse("option+cmd").expect("a shortcut")).as_deref(),
            Some("⌥⌘")
        );
        assert_eq!(
            glyphs(&Shortcut::parse("shift+shift").expect("a shortcut")).as_deref(),
            Some("⇧ ⇧")
        );
    }
}
