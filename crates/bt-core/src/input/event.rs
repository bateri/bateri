//! A key as the kitty keyboard protocol hears it: pressed, repeated or
//! released, with the key it is, the modifiers and locks that were on, the keys
//! it would be in another layout and the text it typed.
//!
//! `bt-shell` says what happened ([`KeyEvent`]); this module decides what the
//! program is told, because that depends on the flags the program asked for
//! and those live in `Term`. **Pure and lock-free** like the rest of
//! [`crate::input`]: the caller reads the mode under its lock and the answer
//! is bytes.
//!
//! What the flags change, one level on top of the other:
//!
//! - nothing asked, or only "disambiguate" (`1`): the bytes a key has always
//!   sent ([`KeyEvent::legacy`], through [`super::function_key`] and
//!   [`super::text_key`]) and nothing for a release;
//! - "event types" (`2`): the keys that go as escape codes say whether they
//!   were pressed, repeated or released; a text key is still text, and ⏎, ⇥
//!   and ⌫ are never released unless `8` is on, so that a shell left in the
//!   mode by a program that died can still run `reset`;
//! - "alternate keys" (`4`): the shifted key and the key in the standard PC
//!   layout ride with the key code;
//! - "all keys as escape codes" (`8`): every key is an escape code, text keys
//!   and the modifier keys by themselves too, and the locks ride with them;
//! - "associated text" (`16`, on top of `8`): the text a key typed rides in
//!   the escape code, and text no key made (a dead key's accent, an input
//!   method's commit) is `CSI 0 ; ; text u` ([`typed_text`]).

use std::borrow::Cow;

use alacritty_terminal::term::TermMode;

use super::{FunctionKey, TextKey, function_key, text_key};
use crate::{Modifier, Mods};

/// What a key did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyPhase {
    Press,
    /// Held down long enough for the system to repeat it.
    Repeat,
    Release,
}

/// Which of the two keys of a pair — ⇧, ⌃, ⌥ or ⌘ — an event is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySide {
    Left,
    Right,
}

/// A key of the keypad. Only what a Mac's keypad has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keypad {
    /// `0`–`9`; another number is no key.
    Digit(u8),
    Decimal,
    Divide,
    Multiply,
    Subtract,
    Add,
    Enter,
    Equal,
}

/// The key an event is about, by what it is and not by what it typed: `a` for
/// the key that types `a` and `A` and `å`, in the layout in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keycap {
    /// A key of characters: the character it types with nothing held. Its
    /// code in the protocol is that character in lower case.
    Char(char),
    Escape,
    Enter,
    Tab,
    Backspace,
    Insert,
    /// ⌦ (fn-⌫).
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Left,
    Right,
    Up,
    Down,
    /// F1–F35; another number is no key.
    F(u8),
    CapsLock,
    ScrollLock,
    NumLock,
    PrintScreen,
    Pause,
    Menu,
    Keypad(Keypad),
    /// A modifier key by itself.
    Modifier(Modifier, KeySide),
    /// A key the protocol has no number for: it goes by its legacy bytes
    /// ([`KeyEvent::legacy`]) and as nothing else.
    Unknown,
}

/// What a key sends where nobody asked for more than the keys told apart. The
/// shell's key table says it; this module and the session write it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyInput {
    /// Bytes independent of the terminal's state: a letter, ⌥←, ⌘⌫…
    Bytes(Cow<'static, [u8]>),
    /// A function key: its bytes depend on the mode and on whether a shell is
    /// reading the line ([`super::function_key`]). ⌘ never rides here — a ⌘ key
    /// that reaches the terminal is bytes.
    Key(FunctionKey, Mods),
    /// Esc, ⏎, ⇥, ⌫ or a Control key, which a program that asked for the kitty
    /// keyboard protocol hears as the key it is (⇧⏎ apart from ⏎, ⌃[ apart
    /// from Esc) and everyone else as the bytes it always sent
    /// ([`super::text_key`]).
    Text(TextKey),
}

/// One thing a key did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    /// The physical key, as the platform numbers it. A press and its release
    /// carry the same one, and the session pairs them by it: a release is told
    /// to the program only for a key whose press it was told.
    pub id: u16,
    pub phase: KeyPhase,
    pub key: Keycap,
    /// What was held **with** the event: a modifier key's own bit is on when it
    /// went down and off when it came up. ⌘ is the protocol's super.
    pub mods: Mods,
    pub caps_lock: bool,
    /// What the key types with Shift, in the layout in use (`A` for `a`).
    pub shifted: Option<char>,
    /// The key in the standard PC layout (`c` for the Cyrillic `с`).
    pub base: Option<char>,
    /// What the event typed: no control character in it, `None` for a key that
    /// types nothing or where the system composed the text elsewhere.
    pub text: Option<String>,
    /// What a press sends where nobody asked for more; `None` for a key that
    /// sends nothing (a modifier key by itself, a release).
    pub legacy: Option<KeyInput>,
}

/// The flags a program asked for, as the keyboard modes `Term` keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Flags {
    disambiguate: bool,
    events: bool,
    alternates: bool,
    all: bool,
    text: bool,
}

impl Flags {
    pub(crate) fn of(mode: TermMode) -> Self {
        Self {
            disambiguate: mode.contains(TermMode::DISAMBIGUATE_ESC_CODES),
            events: mode.contains(TermMode::REPORT_EVENT_TYPES),
            alternates: mode.contains(TermMode::REPORT_ALTERNATE_KEYS),
            all: mode.contains(TermMode::REPORT_ALL_KEYS_AS_ESC),
            text: mode.contains(TermMode::REPORT_ASSOCIATED_TEXT),
        }
    }

    /// Whether the program asked for anything the first level does not give.
    fn beyond_the_first(self) -> bool {
        self.events || self.alternates || self.all || self.text
    }

    /// Whether the event goes as an escape code in the form the protocol gives
    /// it, and not as the bytes a key has always sent.
    fn codes(self, event: &KeyEvent) -> bool {
        let mods = event.mods;
        let told_apart = mods.shift || mods.option || mods.ctrl;
        // A chord the terminal binds to bytes of its own (⌥← as the Meta byte
        // line editors read) stays that, whatever level of the protocol the
        // program asked for, until it asks for every key.
        let bound = matches!(event.legacy, Some(KeyInput::Bytes(_)));
        match event.key {
            Keycap::Unknown => false,
            _ if self.all => true,
            // They are told only when every key is.
            Keycap::Modifier(..)
            | Keycap::Keypad(_)
            | Keycap::CapsLock
            | Keycap::NumLock
            | Keycap::ScrollLock => false,
            Keycap::Escape => self.disambiguate,
            Keycap::Enter | Keycap::Tab | Keycap::Backspace => {
                self.disambiguate && told_apart && !bound
            }
            Keycap::Char(_) => self.disambiguate && (mods.option || mods.ctrl),
            // They are escape codes in the old encoding too.
            Keycap::Insert
            | Keycap::Delete
            | Keycap::Home
            | Keycap::End
            | Keycap::PageUp
            | Keycap::PageDown
            | Keycap::Left
            | Keycap::Right
            | Keycap::Up
            | Keycap::Down
            | Keycap::F(_)
            | Keycap::PrintScreen
            | Keycap::Pause
            | Keycap::Menu => (self.disambiguate || self.events) && !bound,
        }
    }
}

/// How a key is written: `CSI code u`, `CSI number ~` or `CSI 1 letter`.
#[derive(Clone, Copy)]
enum Form {
    U(u32),
    Tilde(u32),
    Letter(char),
}

/// The code of a key of characters: its lower-case character.
fn char_code(key: char) -> u32 {
    let mut lower = key.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(c), None) => u32::from(c),
        _ => u32::from(key),
    }
}

/// The protocol's number for a key, in the form it is written in; `None` for a
/// key it has none for.
fn form(key: Keycap) -> Option<Form> {
    Some(match key {
        Keycap::Char(c) => Form::U(char_code(c)),
        Keycap::Escape => Form::U(27),
        Keycap::Enter => Form::U(13),
        Keycap::Tab => Form::U(9),
        Keycap::Backspace => Form::U(127),
        Keycap::Insert => Form::Tilde(2),
        Keycap::Delete => Form::Tilde(3),
        Keycap::PageUp => Form::Tilde(5),
        Keycap::PageDown => Form::Tilde(6),
        Keycap::Left => Form::Letter('D'),
        Keycap::Right => Form::Letter('C'),
        Keycap::Up => Form::Letter('A'),
        Keycap::Down => Form::Letter('B'),
        Keycap::Home => Form::Letter('H'),
        Keycap::End => Form::Letter('F'),
        Keycap::F(1) => Form::Letter('P'),
        Keycap::F(2) => Form::Letter('Q'),
        // `CSI 1;2 R` is also a cursor position report, so F3 is a number.
        Keycap::F(3) => Form::Tilde(13),
        Keycap::F(4) => Form::Letter('S'),
        Keycap::F(n @ 5..=12) => Form::Tilde([15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)]),
        Keycap::F(n @ 13..=35) => Form::U(57376 + u32::from(n - 13)),
        Keycap::F(_) => return None,
        Keycap::CapsLock => Form::U(57358),
        Keycap::ScrollLock => Form::U(57359),
        Keycap::NumLock => Form::U(57360),
        Keycap::PrintScreen => Form::U(57361),
        Keycap::Pause => Form::U(57362),
        Keycap::Menu => Form::U(57363),
        Keycap::Keypad(pad) => Form::U(match pad {
            Keypad::Digit(digit @ 0..=9) => 57399 + u32::from(digit),
            Keypad::Digit(_) => return None,
            Keypad::Decimal => 57409,
            Keypad::Divide => 57410,
            Keypad::Multiply => 57411,
            Keypad::Subtract => 57412,
            Keypad::Add => 57413,
            Keypad::Enter => 57414,
            Keypad::Equal => 57415,
        }),
        Keycap::Modifier(modifier, side) => Form::U(match (modifier, side) {
            (Modifier::Shift, KeySide::Left) => 57441,
            (Modifier::Ctrl, KeySide::Left) => 57442,
            (Modifier::Option, KeySide::Left) => 57443,
            (Modifier::Cmd, KeySide::Left) => 57444,
            (Modifier::Shift, KeySide::Right) => 57447,
            (Modifier::Ctrl, KeySide::Right) => 57448,
            (Modifier::Option, KeySide::Right) => 57449,
            (Modifier::Cmd, KeySide::Right) => 57450,
        }),
        Keycap::Unknown => return None,
    })
}

/// The protocol's modifier bits: Shift 1, Option (alt) 2, Control 4, Command
/// (super) 8, Caps Lock 64.
fn modifier_bits(mods: Mods, caps_lock: bool) -> u32 {
    u32::from(mods.shift)
        | (u32::from(mods.option) << 1)
        | (u32::from(mods.ctrl) << 2)
        | (u32::from(mods.cmd) << 3)
        | (u32::from(caps_lock) << 6)
}

/// What the key sends where nobody asked for more.
fn legacy(event: &KeyEvent, mode: TermMode, at_prompt: bool) -> Vec<u8> {
    match &event.legacy {
        None => Vec::new(),
        Some(KeyInput::Bytes(bytes)) => bytes.to_vec(),
        Some(KeyInput::Key(key, mods)) => function_key(*key, *mods, mode, at_prompt),
        Some(KeyInput::Text(key)) => text_key(key, mode),
    }
}

/// What the program is told of an event; empty for nothing.
pub(crate) fn key_event(event: &KeyEvent, mode: TermMode, at_prompt: bool) -> Vec<u8> {
    let flags = Flags::of(mode);
    let release = event.phase == KeyPhase::Release;
    let old = || {
        if release {
            Vec::new()
        } else {
            legacy(event, mode, at_prompt)
        }
    };
    if !flags.beyond_the_first() || !flags.codes(event) {
        return old();
    }
    // Where a program that died left the mode on, ⏎ must still run `reset`.
    if release
        && (!flags.events
            || (!flags.all && matches!(event.key, Keycap::Enter | Keycap::Tab | Keycap::Backspace)))
    {
        return Vec::new();
    }
    coded(event, mode, at_prompt, flags).unwrap_or_else(old)
}

/// The escape code of an event: `CSI code:alternates ; modifiers:event ; text u`,
/// `CSI number ; modifiers:event ~` or `CSI 1 ; modifiers:event letter`, with
/// the fields that are the default left out.
fn coded(event: &KeyEvent, mode: TermMode, at_prompt: bool, flags: Flags) -> Option<Vec<u8>> {
    let form = form(event.key)?;
    let suffix = match event.phase {
        KeyPhase::Repeat if flags.events => Some(2),
        KeyPhase::Release if flags.events => Some(3),
        _ => None,
    };
    // A lock is not told for a key that types: legacy programs read those.
    let types = matches!(event.key, Keycap::Char(_) | Keycap::Keypad(_));
    let caps_lock = event.caps_lock && (flags.all || !types);
    let param = 1 + modifier_bits(event.mods, caps_lock);
    // A function key pressed with nothing held keeps the form it has always
    // had, which follows the cursor key mode.
    if matches!(form, Form::Tilde(_) | Form::Letter(_))
        && param == 1
        && suffix.is_none()
        && !flags.all
        && event.legacy.is_some()
    {
        return Some(legacy(event, mode, at_prompt));
    }
    let modifiers = match suffix {
        Some(kind) => format!("{param}:{kind}"),
        None if param > 1 => param.to_string(),
        None => String::new(),
    };
    let text = if flags.all && flags.text && event.phase != KeyPhase::Release {
        event
            .text
            .as_deref()
            .and_then(code_points)
            .unwrap_or_default()
    } else {
        String::new()
    };
    let out = match form {
        Form::U(code) => {
            let mut fields = vec![format!("{code}{}", alternates(event, code, flags))];
            if !modifiers.is_empty() || !text.is_empty() {
                fields.push(modifiers);
            }
            if !text.is_empty() {
                fields.push(text);
            }
            format!("\x1b[{}u", fields.join(";"))
        }
        Form::Tilde(number) if modifiers.is_empty() => format!("\x1b[{number}~"),
        Form::Tilde(number) => format!("\x1b[{number};{modifiers}~"),
        Form::Letter(last) if modifiers.is_empty() => format!("\x1b[{last}"),
        Form::Letter(last) => format!("\x1b[1;{modifiers}{last}"),
    };
    Some(out.into_bytes())
}

/// `:shifted:base` — the keys the key is in another state or layout, for the
/// program that asked for them. The shifted key rides only with Shift, the
/// base layout key only where it is another key; an empty sub-field says the
/// shifted key is not there.
fn alternates(event: &KeyEvent, code: u32, flags: Flags) -> String {
    if !flags.alternates || !matches!(event.key, Keycap::Char(_)) {
        return String::new();
    }
    let other = |key: char| (u32::from(key) != code).then_some(u32::from(key));
    let shifted = event
        .shifted
        .filter(|_| event.mods.shift)
        .and_then(other)
        .map_or_else(String::new, |code| code.to_string());
    match event.base.and_then(other) {
        Some(base) => format!(":{shifted}:{base}"),
        None if shifted.is_empty() => String::new(),
        None => format!(":{shifted}"),
    }
}

/// The code points of a text, colon-separated; `None` where it has a control
/// character, which the protocol never carries.
fn code_points(text: &str) -> Option<String> {
    if text.is_empty() || text.chars().any(char::is_control) {
        return None;
    }
    Some(
        text.chars()
            .map(|c| u32::from(c).to_string())
            .collect::<Vec<_>>()
            .join(":"),
    )
}

/// The bytes of text no key made — a dead key's accent, an input method's
/// commit, a character Option composed. A program that asked for every key
/// and for the text with it hears `CSI 0 ; ; text u`: it has no key to name.
/// Everyone else gets the text.
pub(crate) fn typed_text(text: &str, mode: TermMode) -> Vec<u8> {
    let flags = Flags::of(mode);
    if flags.all
        && flags.text
        && let Some(points) = code_points(text)
    {
        return format!("\x1b[0;;{points}u").into_bytes();
    }
    text.as_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Arrow;

    const NONE: Mods = Mods::NONE;
    const SHIFT: Mods = Mods::new(false, false, true, false);
    const CTRL: Mods = Mods::new(true, false, false, false);
    const OPTION: Mods = Mods::new(false, true, false, false);

    const D: TermMode = TermMode::DISAMBIGUATE_ESC_CODES;
    const EVENTS: TermMode = TermMode::REPORT_EVENT_TYPES;
    const ALTERNATES: TermMode = TermMode::REPORT_ALTERNATE_KEYS;
    const ALL: TermMode = TermMode::REPORT_ALL_KEYS_AS_ESC;
    const TEXT: TermMode = TermMode::REPORT_ASSOCIATED_TEXT;

    fn event(key: Keycap, mods: Mods, text: Option<&str>, legacy: KeyInput) -> KeyEvent {
        KeyEvent {
            id: 0,
            phase: KeyPhase::Press,
            key,
            mods,
            caps_lock: false,
            shifted: None,
            base: None,
            text: text.map(str::to_owned),
            legacy: Some(legacy),
        }
    }

    fn bytes(bytes: &'static [u8]) -> KeyInput {
        KeyInput::Bytes(Cow::Borrowed(bytes))
    }

    fn typing(key: char, mods: Mods, legacy: &'static [u8]) -> KeyInput {
        KeyInput::Text(TextKey {
            key,
            mods,
            legacy: Cow::Borrowed(legacy),
        })
    }

    /// `a`, ⇧A, Esc, ⏎, ⌫, F3, ←, keypad 5 and ⌃C, as `bt-shell` sends them.
    fn keys() -> [(&'static str, KeyEvent); 9] {
        let mut shifted = event(Keycap::Char('a'), SHIFT, Some("A"), bytes(b"A"));
        shifted.shifted = Some('A');
        [
            ("a", event(Keycap::Char('a'), NONE, Some("a"), bytes(b"a"))),
            ("⇧A", shifted),
            (
                "Esc",
                event(Keycap::Escape, NONE, None, typing('\u{1b}', NONE, b"\x1b")),
            ),
            (
                "⏎",
                event(Keycap::Enter, NONE, None, typing('\r', NONE, b"\r")),
            ),
            (
                "⌫",
                event(
                    Keycap::Backspace,
                    NONE,
                    None,
                    typing('\u{7f}', NONE, b"\x7f"),
                ),
            ),
            (
                "F3",
                event(
                    Keycap::F(3),
                    NONE,
                    None,
                    KeyInput::Key(FunctionKey::F(3), NONE),
                ),
            ),
            (
                "←",
                event(
                    Keycap::Left,
                    NONE,
                    None,
                    KeyInput::Key(FunctionKey::Arrow(Arrow::Left), NONE),
                ),
            ),
            (
                "keypad 5",
                event(
                    Keycap::Keypad(Keypad::Digit(5)),
                    NONE,
                    Some("5"),
                    bytes(b"5"),
                ),
            ),
            (
                "⌃C",
                event(Keycap::Char('c'), CTRL, None, typing('c', CTRL, b"\x03")),
            ),
        ]
    }

    fn sent(event: &KeyEvent, mode: TermMode) -> String {
        String::from_utf8(key_event(event, mode, false)).expect("keys are text")
    }

    fn table(mode: TermMode) -> Vec<String> {
        keys().iter().map(|(_, event)| sent(event, mode)).collect()
    }

    #[test]
    fn with_nothing_asked_or_the_first_level_a_key_sends_the_bytes_it_always_sent() {
        // a, ⇧A, Esc, ⏎, ⌫, F3, ←, keypad 5, ⌃C.
        assert_eq!(
            table(TermMode::empty()),
            [
                "a", "A", "\x1b", "\r", "\x7f", "\x1bOR", "\x1b[D", "5", "\x03"
            ]
        );
        assert_eq!(
            table(D),
            [
                "a",
                "A",
                "\x1b[27u",
                "\r",
                "\x7f",
                "\x1b[13~",
                "\x1b[D",
                "5",
                "\x1b[99;5u"
            ]
        );
        // Caps Lock, a repeat and a release change nothing at these levels, and
        // a release says nothing.
        let mut caps = keys()[8].1.clone();
        caps.caps_lock = true;
        assert_eq!(sent(&caps, D), "\x1b[99;5u");
        caps.phase = KeyPhase::Repeat;
        assert_eq!(sent(&caps, D), "\x1b[99;5u");
        caps.phase = KeyPhase::Release;
        assert_eq!(sent(&caps, D), "");
    }

    #[test]
    fn event_types_tell_the_escape_codes_apart_and_leave_text_alone() {
        assert_eq!(
            table(D | EVENTS),
            [
                "a",
                "A",
                "\x1b[27u",
                "\r",
                "\x7f",
                "\x1b[13~",
                "\x1b[D",
                "5",
                "\x1b[99;5u"
            ]
        );
        let at = |name: &str, phase| {
            let mut event = keys().into_iter().find(|(n, _)| *n == name).unwrap().1;
            event.phase = phase;
            sent(&event, D | EVENTS)
        };
        assert_eq!(at("Esc", KeyPhase::Repeat), "\x1b[27;1:2u");
        assert_eq!(at("Esc", KeyPhase::Release), "\x1b[27;1:3u");
        assert_eq!(at("←", KeyPhase::Repeat), "\x1b[1;1:2D");
        assert_eq!(at("←", KeyPhase::Release), "\x1b[1;1:3D");
        assert_eq!(at("F3", KeyPhase::Release), "\x1b[13;1:3~");
        assert_eq!(at("⌃C", KeyPhase::Release), "\x1b[99;5:3u");
        // A key that is text has no release; ⏎ and ⌫ have none until every
        // key is asked for.
        assert_eq!(at("a", KeyPhase::Release), "");
        assert_eq!(at("a", KeyPhase::Repeat), "a");
        assert_eq!(at("⇧A", KeyPhase::Release), "");
        assert_eq!(at("keypad 5", KeyPhase::Release), "");
        assert_eq!(at("⏎", KeyPhase::Release), "");
        assert_eq!(at("⌫", KeyPhase::Release), "");
        let mut shifted_enter = keys()[3].1.clone();
        shifted_enter.mods = SHIFT;
        assert_eq!(sent(&shifted_enter, D | EVENTS), "\x1b[13;2u");
        shifted_enter.phase = KeyPhase::Release;
        assert_eq!(sent(&shifted_enter, D | EVENTS), "");
        // A program that asked for events and not for the keys told apart is
        // told only what is an escape code anyway.
        assert_eq!(sent(&keys()[2].1, EVENTS), "\x1b");
        let mut left = keys()[6].1.clone();
        left.phase = KeyPhase::Release;
        assert_eq!(sent(&left, EVENTS), "\x1b[1;1:3D");
        // And no flag at all hears no release.
        assert_eq!(sent(&left, D), "");
    }

    #[test]
    fn every_key_as_an_escape_code_has_text_keys_modifier_keys_and_the_locks() {
        assert_eq!(
            table(ALL),
            [
                "\x1b[97u",
                "\x1b[97;2u",
                "\x1b[27u",
                "\x1b[13u",
                "\x1b[127u",
                "\x1b[13~",
                "\x1b[D",
                "\x1b[57404u",
                "\x1b[99;5u"
            ]
        );
        // Releases and repeats, ⏎ and ⌫ now with them.
        let at = |name: &str, phase| {
            let mut event = keys().into_iter().find(|(n, _)| *n == name).unwrap().1;
            event.phase = phase;
            sent(&event, ALL | EVENTS)
        };
        assert_eq!(at("a", KeyPhase::Release), "\x1b[97;1:3u");
        assert_eq!(at("a", KeyPhase::Repeat), "\x1b[97;1:2u");
        assert_eq!(at("⏎", KeyPhase::Release), "\x1b[13;1:3u");
        assert_eq!(at("⌫", KeyPhase::Release), "\x1b[127;1:3u");
        assert_eq!(at("⇧A", KeyPhase::Release), "\x1b[97;2:3u");
        // Without the event types a release is not told, and a press is.
        let mut release = keys()[0].1.clone();
        release.phase = KeyPhase::Release;
        assert_eq!(sent(&release, ALL), "");
        // A lock rides with every key, a text key too.
        let mut caps = keys()[0].1.clone();
        caps.caps_lock = true;
        assert_eq!(sent(&caps, ALL), "\x1b[97;65u");
        assert_eq!(sent(&caps, D | EVENTS), "a");
        // The modifier keys by themselves: the bit is the key's own, on the
        // way down and off on the way up.
        let mut shift = event(
            Keycap::Modifier(Modifier::Shift, KeySide::Left),
            SHIFT,
            None,
            bytes(b""),
        );
        shift.legacy = None;
        assert_eq!(sent(&shift, ALL), "\x1b[57441;2u");
        shift.phase = KeyPhase::Release;
        shift.mods = NONE;
        assert_eq!(sent(&shift, ALL | EVENTS), "\x1b[57441;1:3u");
        assert_eq!(sent(&shift, ALL), "");
        let mut command = event(
            Keycap::Modifier(Modifier::Cmd, KeySide::Right),
            Mods::CMD,
            None,
            bytes(b""),
        );
        command.legacy = None;
        assert_eq!(sent(&command, ALL), "\x1b[57450;9u");
        // Nowhere else is a modifier key told.
        assert_eq!(sent(&command, D | EVENTS), "");
    }

    #[test]
    fn the_text_a_key_typed_rides_in_the_escape_code_when_it_is_asked_for() {
        assert_eq!(
            table(ALL | TEXT),
            [
                "\x1b[97;;97u",
                "\x1b[97;2;65u",
                "\x1b[27u",
                "\x1b[13u",
                "\x1b[127u",
                "\x1b[13~",
                "\x1b[D",
                "\x1b[57404;;53u",
                "\x1b[99;5u"
            ]
        );
        let mut repeat = keys()[0].1.clone();
        repeat.phase = KeyPhase::Repeat;
        assert_eq!(sent(&repeat, ALL | TEXT | EVENTS), "\x1b[97;1:2;97u");
        // Not on a release, and not for the flag that is only an enhancement
        // of the one it rides on.
        repeat.phase = KeyPhase::Release;
        assert_eq!(sent(&repeat, ALL | TEXT | EVENTS), "\x1b[97;1:3u");
        assert_eq!(sent(&keys()[0].1, TEXT), "a");
        assert_eq!(sent(&keys()[0].1, D | TEXT), "a");
        // Text with a control character in it is not carried.
        let mut control = keys()[0].1.clone();
        control.text = Some("\r".to_owned());
        assert_eq!(sent(&control, ALL | TEXT), "\x1b[97u");
    }

    #[test]
    fn the_alternate_keys_ride_with_the_key_code_when_they_are_asked_for() {
        let mut cyrillic = event(Keycap::Char('с'), CTRL, None, typing('с', CTRL, b"\x03"));
        cyrillic.base = Some('c');
        assert_eq!(sent(&cyrillic, D | ALTERNATES), "\x1b[1089::99;5u");
        assert_eq!(sent(&cyrillic, D), "\x1b[1089;5u");
        cyrillic.mods = Mods::new(true, false, true, false);
        cyrillic.shifted = Some('С');
        assert_eq!(sent(&cyrillic, D | ALTERNATES), "\x1b[1089:1057:99;6u");
        // Shift is the only thing that makes the shifted key matter; a key that
        // is the base key says only what is different.
        cyrillic.mods = CTRL;
        assert_eq!(sent(&cyrillic, D | ALTERNATES), "\x1b[1089::99;5u");
        let mut plain = event(Keycap::Char('c'), CTRL, None, typing('c', CTRL, b"\x03"));
        plain.base = Some('c');
        assert_eq!(sent(&plain, D | ALTERNATES), "\x1b[99;5u");
        plain.mods = Mods::new(true, false, true, false);
        plain.shifted = Some('C');
        assert_eq!(sent(&plain, D | ALTERNATES), "\x1b[99:67;6u");
        // Under every key: ⇧A names both its forms.
        let mut shifted = keys()[1].1.clone();
        shifted.base = Some('a');
        assert_eq!(sent(&shifted, ALL | ALTERNATES), "\x1b[97:65;2u");
        // Only a key of characters has any.
        assert_eq!(sent(&keys()[6].1, D | ALTERNATES), "\x1b[D");
    }

    #[test]
    fn a_function_key_with_something_held_or_without_a_legacy_form_is_the_protocols_number() {
        let key = |key, mods| {
            let mut event = event(key, mods, None, bytes(b""));
            event.legacy = None;
            event
        };
        assert_eq!(sent(&key(Keycap::Insert, NONE), D | EVENTS), "\x1b[2~");
        assert_eq!(sent(&key(Keycap::Delete, SHIFT), D | EVENTS), "\x1b[3;2~");
        assert_eq!(sent(&key(Keycap::Up, SHIFT), D | EVENTS), "\x1b[1;2A");
        assert_eq!(sent(&key(Keycap::F(1), NONE), ALL), "\x1b[P");
        assert_eq!(sent(&key(Keycap::F(5), CTRL), D | EVENTS), "\x1b[15;5~");
        assert_eq!(sent(&key(Keycap::F(13), NONE), D | EVENTS), "\x1b[57376u");
        assert_eq!(
            sent(&key(Keycap::F(35), SHIFT), D | EVENTS),
            "\x1b[57398;2u"
        );
        assert_eq!(sent(&key(Keycap::F(36), NONE), ALL), "");
        assert_eq!(sent(&key(Keycap::Home, NONE), ALL), "\x1b[H");
        assert_eq!(sent(&key(Keycap::End, OPTION), ALL), "\x1b[1;3F");
        assert_eq!(sent(&key(Keycap::PageDown, NONE), ALL), "\x1b[6~");
        // A plain arrow with nothing asked of the key's phase follows the
        // cursor key mode; with every key asked for it is the one form.
        let mut left = keys()[6].1.clone();
        assert_eq!(sent(&left, D | EVENTS | TermMode::APP_CURSOR), "\x1bOD");
        assert_eq!(sent(&left, ALL | TermMode::APP_CURSOR), "\x1b[D");
        left.caps_lock = true;
        assert_eq!(sent(&left, D | EVENTS), "\x1b[1;65D");
    }

    #[test]
    fn the_keypad_is_text_until_every_key_is_asked_for() {
        let pad = |pad, text: &str| {
            event(
                Keycap::Keypad(pad),
                NONE,
                Some(text),
                bytes(match pad {
                    Keypad::Enter => b"\r",
                    _ => b"x",
                }),
            )
        };
        assert_eq!(sent(&pad(Keypad::Enter, ""), D | EVENTS), "\r");
        assert_eq!(sent(&pad(Keypad::Enter, ""), ALL), "\x1b[57414u");
        assert_eq!(sent(&pad(Keypad::Digit(0), "0"), ALL), "\x1b[57399u");
        assert_eq!(sent(&pad(Keypad::Decimal, "."), ALL), "\x1b[57409u");
        assert_eq!(sent(&pad(Keypad::Divide, "/"), ALL), "\x1b[57410u");
        assert_eq!(sent(&pad(Keypad::Multiply, "*"), ALL), "\x1b[57411u");
        assert_eq!(sent(&pad(Keypad::Subtract, "-"), ALL), "\x1b[57412u");
        assert_eq!(sent(&pad(Keypad::Add, "+"), ALL), "\x1b[57413u");
        assert_eq!(sent(&pad(Keypad::Equal, "="), ALL), "\x1b[57415u");
        assert_eq!(sent(&pad(Keypad::Digit(10), "x"), ALL), "x");
    }

    #[test]
    fn a_chord_the_terminal_binds_to_bytes_keeps_them_until_every_key_is_asked_for() {
        // ⌥← is the Meta byte line editors read, at every level of the protocol
        // below the one that asks for every key; ⌥⌫ too.
        let option_left = event(Keycap::Left, OPTION, None, bytes(b"\x1bb"));
        let option_backspace = event(Keycap::Backspace, OPTION, None, bytes(b"\x1b\x7f"));
        for mode in [D, D | EVENTS, D | EVENTS | ALTERNATES, EVENTS] {
            assert_eq!(sent(&option_left, mode), "\x1bb");
            assert_eq!(sent(&option_backspace, mode), "\x1b\x7f");
        }
        assert_eq!(sent(&option_left, ALL), "\x1b[1;3D");
        assert_eq!(sent(&option_backspace, ALL), "\x1b[127;3u");
        // Nothing of a release for a chord that was bytes.
        let mut release = option_left;
        release.phase = KeyPhase::Release;
        assert_eq!(sent(&release, D | EVENTS), "");
        assert_eq!(sent(&release, ALL | EVENTS), "\x1b[1;3:3D");
    }

    #[test]
    fn the_lock_keys_are_told_only_with_every_key() {
        let mut caps = event(Keycap::CapsLock, NONE, None, bytes(b""));
        caps.legacy = None;
        assert_eq!(sent(&caps, D | EVENTS), "");
        assert_eq!(sent(&caps, ALL), "\x1b[57358u");
        let mut num = event(Keycap::NumLock, NONE, None, bytes(b""));
        num.legacy = None;
        assert_eq!(sent(&num, ALL), "\x1b[57360u");
        // The keys that have no encoding of their own are told with the first level.
        let mut pause = event(Keycap::Pause, NONE, None, bytes(b""));
        pause.legacy = None;
        assert_eq!(sent(&pause, D | EVENTS), "\x1b[57362u");
    }

    #[test]
    fn a_key_the_protocol_has_no_number_for_goes_by_its_bytes() {
        let unknown = event(Keycap::Unknown, NONE, None, bytes(b"?"));
        assert_eq!(sent(&unknown, ALL | EVENTS), "?");
        let mut release = unknown;
        release.phase = KeyPhase::Release;
        assert_eq!(sent(&release, ALL | EVENTS), "");
    }

    #[test]
    fn text_no_key_made_is_a_text_event_for_a_program_that_asked_for_it() {
        assert_eq!(typed_text("é", TermMode::empty()), "é".as_bytes());
        assert_eq!(typed_text("é", ALL), "é".as_bytes());
        assert_eq!(
            typed_text("é", D | EVENTS | ALTERNATES | TEXT),
            "é".as_bytes()
        );
        assert_eq!(typed_text("é", ALL | TEXT), b"\x1b[0;;233u");
        assert_eq!(typed_text("日本", ALL | TEXT), b"\x1b[0;;26085:26412u");
        // A control character is no text: it goes as it came.
        assert_eq!(typed_text("\r", ALL | TEXT), b"\r");
    }
}
