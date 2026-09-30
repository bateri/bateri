//! Keystroke → PTY bytes or an arrow, the scroll decision for Shift+PgUp/PgDn, and the dock
//! selection's keys ([`dock_key`]).
//! **Pure and AppKit-free**, and therefore testable.
//!
//! An arrow's bytes are **not written** here, only which arrow it is (`bt_core::Arrow`, the reason
//! is there).

use std::borrow::Cow;

use bt_core::{Arrow, DockKey};

/// AppKit's function key range: U+F700–U+F8FF. The named constants
/// (`NSUpArrowFunctionKey` … `NSModeSwitchFunctionKey`) use its first slice. Arrows are turned
/// into [`KeyInput::Arrow`], PgUp/PgDn and forward delete into their own sequences; the
/// **unrecognised rest** of the range is swallowed on purpose, because turning a key code we do
/// not know into UTF-8 and sending it to the shell is always worse. The Private Use Area is wider
/// than this (it starts at U+E000) and is **out of scope**: a real character such as a powerline
/// glyph goes through the plain-text branch.
const FUNCTION_KEYS: std::ops::RangeInclusive<char> = '\u{f700}'..='\u{f8ff}';

/// `NSPageUpFunctionKey` and `NSPageDownFunctionKey`. Read in two places — the plain form's
/// sequence ([`encode_key`]) and the Shift form's scroll ([`page_scroll`]) — and a number written
/// separately in two places would drift in one of them.
const PAGE_UP: char = '\u{f72c}';
const PAGE_DOWN: char = '\u{f72d}';

/// `NSDeleteCharacter` — the `characters` of backspace (⌫). Forward delete (⌦) is **not this**,
/// it is `NSDeleteFunctionKey` (U+F728) and lies in the function key range.
///
/// Like `PAGE_UP`, read in two places: Cmd's closed allow-list (`view::reaches_terminal` — "does
/// this key go to the terminal") and that list's byte ([`encode_key`] — "which byte"). Having the
/// decision and the encoding in separate places is the same split as `page_scroll`; writing the
/// literal in two places would leave the door open for it to drift in one of them.
pub const BACKSPACE: char = '\u{7f}';

/// `NSLeftArrowFunctionKey` and `NSRightArrowFunctionKey`.
///
/// Same precedent and same reason as `BACKSPACE` and `PAGE_UP`: both are read **in two places** —
/// Cmd's closed allow-list (`view::reaches_terminal`) and that list's byte ([`encode_key`]) — and a
/// literal written separately in two places would drift in one of them. The up/down arrows stay
/// literals: only one place reads them.
pub const ARROW_LEFT: char = '\u{f702}';
pub const ARROW_RIGHT: char = '\u{f703}';

/// `NSDeleteFunctionKey` — forward delete (⌦, fn-⌫). Read in two places: its byte
/// ([`encode_key`]) and the dock selection's key ([`dock_key`]).
const FORWARD_DELETE: char = '\u{f728}';

/// Whether the string is **exactly one character** — if so that character, otherwise `None`.
///
/// This is the criterion's **single owner** and it has three consumers: [`encode_key`]'s
/// `single` discipline, [`page_scroll`]'s scroll decision and `view::reaches_terminal`'s ⌘
/// allow-list. All three ask the same question — "is this one key's character, or a composition's
/// multi-character output" — and all three had been written separately; the same reason
/// `BACKSPACE` and `PAGE_UP` live in one place: a criterion drifting in one place would silently
/// split the others off.
///
/// `chars().next()` is **not enough**: if a multi-character `characters` (the output of a dead-key
/// composition, marked text) is read from its first character, the rest is dropped without a
/// trace.
pub fn only_char(chars: &str) -> Option<char> {
    let mut it = chars.chars();
    match (it.next(), it.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// [`encode_key`]'s answer: a key with known bytes, or an arrow.
#[derive(Debug, PartialEq, Eq)]
pub enum KeyInput {
    /// Bytes independent of the mode: a letter, Enter, PgUp…
    Bytes(Cow<'static, [u8]>),
    /// Goes to `Session::write_arrow`.
    Arrow(Arrow),
}

/// [`encode_key`]'s input: the half of `NSEvent` that concerns this function.
///
/// **One record**, not four separate parameters, because the arms now ask the flags **together
/// with** the character — ⌘⌫ and ⌥⌫ differ only by flag from the same `characters` (`BACKSPACE`)
/// — and every new modifier would mean walking the call sites one by one.
///
/// **No Shift**, and that is not an omission: no arm asks for it. The only place Shift means
/// anything is the scroll decision ([`page_scroll`]), which cannot be thought of without Shift; an
/// unused flag here would loosen the record's contract ("these flags are read").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPress<'a> {
    /// `NSEvent.characters` — the form with modifiers **applied** (Option-held `ø`, Ctrl-C →
    /// U+0003).
    pub chars: &'a str,
    /// Control is held.
    pub ctrl: bool,
    /// Option (⌥) is held. It only enables the navigation/deletion class; it does not touch
    /// printable letters (below, R3.2).
    pub option: bool,
    /// Command (⌘) is held. **Only** a key that passed the allow-list arrives here
    /// (`view::reaches_terminal`), so the flag's only job is to tell the list's three keys (⌘⌫,
    /// ⌘←, ⌘→) apart from their Option forms.
    pub command: bool,
}

/// Keystroke ([`KeyPress`]) → PTY bytes or an arrow.
///
/// **Not every key comes here any more.** The text path goes through AppKit's stack
/// (`view::BateriView`'s `NSTextInputClient` conformance) and the usual producer of a printable
/// letter is `insertText:`, not this function. **Five** groups remain here:
///
/// 1. **An event with Control** — `keyDown:` never hands it to the stack (numpad Enter's U+0003
///    and Ctrl-Y's U+0019 are shared; leaving the arm to AppKit could interrupt every command).
/// 2. **A key the stack hands to `doCommandBySelector:`** — Enter, Tab, Escape, Backspace, the
///    arrows, Shift+Tab, PgUp/PgDn, fn+Backspace **and Option navigation/deletion**
///    (`moveWordLeft:`, `deleteWordBackward:`). That method is a silent no-op and the event
///    returns unflagged, so its bytes come from here.
/// 3. **Shift+PgUp/PgDn rejected by the session** — there is no scrolling on the alternate
///    screen, the key goes to the application as the plain sequence.
/// 4. **Text the stack hands over with a type we do not recognise** — if `insertText:`'s
///    downcast fails, the event is not counted as consumed and falls through to here.
/// 5. **A key that passes Cmd's closed allow-list** — ⌘⌫, ⌘← and ⌘→; an event with Cmd
///    **never enters** the stack, `view::reaches_terminal` makes the decision.
///
/// That is why the plain-text branch **was not removed**: the bytes of groups 2 and 4 come out of
/// it, and a Ctrl letter (1.) can fall into the same branch too.
///
/// `None` → the key is swallowed; the caller writes nothing.
///
/// **Out of scope:** IME; **Option as Meta across the board** — Meta encoding only for the
/// navigation/deletion class, printable letters do not change (`Option+7` keeps typing `{` on
/// Turkish Q, R3.2); the kitty keyboard protocol; **modified arrows** (`\e[1;5A`) — Option+arrow's
/// `\eb` does not replace them, it is a Meta sequence, not xterm's modifier encoding; **backspace
/// with Control** and **forward delete with Option/Control** (⌦, U+F728); Home/End (their
/// sequences are not written yet — a debt; swallowed below). **Dead keys stay out of scope and are
/// no longer a debt**: AppKit's stack completes the composition, it never passes through here.
pub fn encode_key(key: KeyPress<'_>) -> Option<KeyInput> {
    let c = key.chars.chars().next()?;
    // Whether `characters` is a single character — the shared guard of the arms that come out of
    // a single `c` (below: backspace, arrows, PgUp/PgDn, forward delete and Control):
    // multi-character input (a dead-key composition, marked text) is not read from its first
    // character. The criterion's owner is [`only_char`] — `page_scroll` and
    // `view::reaches_terminal` ask the same question from there too.
    let single = only_char(key.chars).is_some();
    let bytes: Cow<'static, [u8]> = match (c, key.ctrl) {
        // The first key of Cmd's closed allow-list: ⌘⌫ → `\x15` (`^U`, `kill-whole-line` in
        // zsh). macOS's strict meaning is "delete **up to the start** of the line", but in zsh
        // `backward-kill-line` is not bound at all by default (measured) — the expectation is
        // that the line goes away, and `^U` does exactly that (018 Karar 3). The arm comes
        // **before** Option's: ⌘⌥⌫ deletes the line, not the word — the allow-list is a named
        // exception, Option's class is a rule.
        (BACKSPACE, _) if key.command && single => Cow::Borrowed(b"\x15"),
        // The allow-list's other two keys: ⌘← → `\x01` (`^A`, `beginning-of-line`), ⌘→ →
        // `\x05` (`^E`, `end-of-line`). The same decision as ⌘⌫ — macOS's start/end-of-line
        // gesture, with the byte that **actually** does that job in zsh.
        //
        // 018 Karar 3 had rejected these two keys and its reasoning had two parts: "not asked
        // for" and "the Home/End sequences are unbound in zsh". The first fell away (the user
        // asked, 2026-09-21), the second **was never a reason about these keys**: the
        // measurement shows zero bindings for `^[[H`/`^[[F`/`^[OH`/`^[OF`, but `^A`/`^E` are
        // bound to `beginning-of-line`/`end-of-line` in the emacs keymap (zsh's default) — so
        // the rejection's measurement belonged to the shape of Home/End, not to these bytes.
        // Ghostty, VS Code and Warp send the same two bytes too.
        //
        // **Known cost, in the same class as ⌘⌫'s:** in the `viins` keymap `^A`/`^E` are
        // `self-insert`, so in vi mode a control character lands in the line (measured).
        // Option's `\eb`/`\ef` are `undefined-key` there too and that trade-off was accepted;
        // vi mode's start/end-of-line keys are `0`/`$`.
        (ARROW_LEFT, _) if key.command && single => Cow::Borrowed(b"\x01"),
        (ARROW_RIGHT, _) if key.command && single => Cow::Borrowed(b"\x05"),
        // Option's **navigation/deletion** class → Meta sequences. No setting is consulted,
        // because these keys produce no printable character on any keyboard layout: the conflict
        // is in Option **letters**, and those stay untouched (018 Karar 2). The sequences are
        // **lowercase**: the uppercase form is bound to other widgets in zsh (`\eA` =
        // `accept-and-hold`; measured).
        //
        // They come here from group 2: the stack hands Option+arrow/⌫ to
        // `doCommandBySelector:` (`moveWordLeft:`, `deleteWordBackward:`), that method is a
        // silent no-op and the event returns unconsumed. Giving that method a body would
        // silently kill this arm.
        //
        // Accompanying modifiers are not consulted (the `page_scroll` precedent): Option+arrow
        // with Ctrl or Shift also moves by word, ⌥ has no second meaning in this class.
        (BACKSPACE, _) if key.option && single => Cow::Borrowed(b"\x1b\x7f"),
        (ARROW_LEFT, _) if key.option && single => Cow::Borrowed(b"\x1bb"),
        (ARROW_RIGHT, _) if key.option && single => Cow::Borrowed(b"\x1bf"),
        // The numeric keypad's Enter and Fn-Return give `NSEnterCharacter` = U+0003 — the same
        // as Ctrl-C's byte. If Ctrl is NOT held this is a line ending; without this arm it would
        // pass through the plain-text branch as 0x03 and numpad Enter would interrupt every
        // command instead of running it.
        ('\u{3}', false) => Cow::Borrowed(b"\r"),
        // Shift+Tab: `characters` gives `NSBackTabCharacter` = U+0019. `xterm-256color`'s `kcbt`
        // is `\e[Z` — zsh's completion menu and readline read back-tab from this sequence, not
        // from a raw 0x19. U+0019 is also Ctrl-Y's byte (yank); again the Control flag tells them
        // apart, exactly like the U+0003 arm above. The Ctrl form goes out as 0x19 through the
        // plain-text branch.
        ('\u{19}', false) if single => Cow::Borrowed(b"\x1b[Z"),
        ('\u{f700}', _) if single => return Some(KeyInput::Arrow(Arrow::Up)),
        ('\u{f701}', _) if single => return Some(KeyInput::Arrow(Arrow::Down)),
        (ARROW_LEFT, _) if single => return Some(KeyInput::Arrow(Arrow::Left)),
        (ARROW_RIGHT, _) if single => return Some(KeyInput::Arrow(Arrow::Right)),
        // `xterm-256color`'s `kpp`/`knp`: less and vim read paging from these two sequences. The
        // Shift form **does not come** here — that is the terminal's scrolling
        // ([`page_scroll`]), which `view` asks first. When scrolling is rejected on the alternate
        // screen, the Shift key falls through to here too and the application receives a plain
        // PgUp: Shift's `;2` encoding, like the modifiers on arrows, is out of scope.
        //
        // `single` (above): the same criterion as `page_scroll`; multi-character input is
        // swallowed whole in the function key arm below.
        (PAGE_UP, _) if single => Cow::Borrowed(b"\x1b[5~"),
        (PAGE_DOWN, _) if single => Cow::Borrowed(b"\x1b[6~"),
        // `kdch1`: deletes the character to the right of the cursor. Because it is in the
        // function key range it is written **before** the swallowing arm below; its modified form
        // (`\e[3;5~`) is out of scope like the arrows and gets the plain sequence.
        // `NSDeleteFunctionKey`: fn+Backspace, Delete (⌦) on a full keyboard.
        (FORWARD_DELETE, _) if single => Cow::Borrowed(b"\x1b[3~"),
        // AppKit applies Control to `characters` itself for most keys (Ctrl-C → U+0003) and that
        // form goes through the plain-text branch below. But not for all of them — with
        // Ctrl-Shift-C the letter stays a letter. The conversion is repeated here so that both
        // paths give the same byte.
        // `single`: this arm only produces bytes from `c`, so if a multi-character `characters`
        // came in, everything after the first character would be dropped without a trace. Such
        // input should go to the plain-text branch, where all of it passes through.
        (c, true) if c.is_ascii_alphabetic() && single => {
            Cow::Owned(vec![(c.to_ascii_lowercase() as u8) & 0x1f])
        }
        // A function key whose sequence we do not know (F1, Home, End…). These are not real
        // characters but AppKit's private use codes: turning them into UTF-8 and writing them to
        // the PTY would send garbage to the shell.
        (c, _) if FUNCTION_KEYS.contains(&c) => return None,
        // Enter, Tab, Escape and Backspace also pass through here: AppKit has already turned
        // them into the right byte (U+000D, U+0009, U+001B, U+007F) and writing separate arms
        // would describe the same byte a second time.
        // `return_and_delete_are_single_bytes` pins the contract.
        _ => Cow::Owned(key.chars.as_bytes().to_vec()),
    };
    Some(KeyInput::Bytes(bytes))
}

/// Shift+PgUp/PgDn → the number of pages to scroll (±1); `None` → not a scroll key. Plus is
/// backwards, the same sign as `Session::scroll_page`.
///
/// **A pure decision**, it does not know the page size: how many rows a page has is `bt-core`'s
/// decision (`Session::scroll_page`). Modifiers other than Shift are not consulted — Shift+PgUp
/// with Control or Option scrolls too; the scroll key has no other meaning.
///
/// The match is on the **whole string** ([`only_char`]): a multi-character `characters`
/// (a composition) is not read as a scroll from its first character — the same as
/// [`encode_key`]'s `single` discipline, now from the same place.
pub fn page_scroll(chars: &str, shift: bool) -> Option<i32> {
    if !shift {
        return None;
    }
    match only_char(chars)? {
        PAGE_UP => Some(1),
        PAGE_DOWN => Some(-1),
        _ => None,
    }
}

/// A key the terminal can handle while there is a dock selection (031 Karar 8) — what each one
/// does lives in `bt-core` (`Session::dock_key`), this is only `NSEvent`'s dictionary.
///
/// **Unmodified** ⌫, ⌦, ←, →, the two Shift arrows, ⏎ and ⇧⏎. A key carrying Option, Control or
/// Command is never a dock key: ⌥⌫ is `backward-kill-word`, ⌘⌫ is `kill-whole-line`, and both go
/// their current way and remove the selection — the "any other key" arm. Shift+⌫ counts as a
/// plain ⌫: it is the same key on macOS too.
pub fn dock_key(key: KeyPress<'_>, shift: bool) -> Option<DockKey> {
    if key.ctrl || key.option || key.command {
        return None;
    }
    match (only_char(key.chars)?, shift) {
        (BACKSPACE, _) => Some(DockKey::Backspace),
        (FORWARD_DELETE, _) => Some(DockKey::Delete),
        (ARROW_LEFT, false) => Some(DockKey::Left),
        (ARROW_RIGHT, false) => Some(DockKey::Right),
        (ARROW_LEFT, true) => Some(DockKey::ShiftLeft),
        (ARROW_RIGHT, true) => Some(DockKey::ShiftRight),
        // ⇧⏎: a new line in the dock without running the line (iTerm's and Claude Code's
        // habit). If the gate is closed, `bt-core` does not consume it and the key goes out as
        // Enter, as it does today.
        ('\r', true) => Some(DockKey::NewLine),
        // Plain ⏎: consumed only while a reconnect offer is showing (037 Karar 8); otherwise
        // `bt-core` says `false` on the first question and Enter goes its current way.
        // Numpad Enter's `characters` is U+0003 (without Control; `encode_key` turns it into
        // `\r`) — the second face of the same key.
        ('\r' | '\u{3}', false) => Some(DockKey::Enter),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dock_keys_are_the_plain_eight() {
        assert_eq!(dock_key(plain("\u{7f}"), false), Some(DockKey::Backspace));
        assert_eq!(dock_key(plain("\u{7f}"), true), Some(DockKey::Backspace));
        assert_eq!(dock_key(plain("\u{f728}"), false), Some(DockKey::Delete));
        assert_eq!(dock_key(plain("\u{f702}"), false), Some(DockKey::Left));
        assert_eq!(dock_key(plain("\u{f703}"), false), Some(DockKey::Right));
        assert_eq!(dock_key(plain("\u{f702}"), true), Some(DockKey::ShiftLeft));
        assert_eq!(dock_key(plain("\u{f703}"), true), Some(DockKey::ShiftRight));
        assert_eq!(dock_key(plain("\r"), true), Some(DockKey::NewLine));
        assert_eq!(dock_key(plain("\r"), false), Some(DockKey::Enter));
        assert_eq!(dock_key(plain("\u{3}"), false), Some(DockKey::Enter));
        // Ctrl-C (the same `characters`, with Control) is not a dock key.
        assert_eq!(dock_key(ctrl("\u{3}"), false), None);
        // ⌥⏎ and ⌘⏎ are not dock keys.
        for flag in [
            KeyPress {
                option: true,
                ..plain("\r")
            },
            KeyPress {
                command: true,
                ..plain("\r")
            },
        ] {
            assert_eq!(dock_key(flag, false), None);
        }
        // ⌥⇧⏎ and ⌃⇧⏎ are not dock keys.
        assert_eq!(
            dock_key(
                KeyPress {
                    option: true,
                    ..plain("\r")
                },
                true
            ),
            None
        );
        // "Any other key": modified ones and text.
        for key in [
            KeyPress {
                option: true,
                ..plain("\u{7f}")
            },
            KeyPress {
                command: true,
                ..plain("\u{7f}")
            },
            KeyPress {
                ctrl: true,
                ..plain("\u{f702}")
            },
            plain("a"),
            plain("\u{f700}"),
            plain("\u{7f}\u{7f}"),
        ] {
            assert_eq!(dock_key(key, false), None, "{:?}", key.chars);
        }
    }

    /// An unmodified keystroke. Arms that turn on a single flag are written with `..plain(chars)`,
    /// so the test line names which flag is the subject.
    fn plain(chars: &str) -> KeyPress<'_> {
        KeyPress {
            chars,
            ctrl: false,
            option: false,
            command: false,
        }
    }

    fn ctrl(chars: &str) -> KeyPress<'_> {
        KeyPress {
            ctrl: true,
            ..plain(chars)
        }
    }

    fn option(chars: &str) -> KeyPress<'_> {
        KeyPress {
            option: true,
            ..plain(chars)
        }
    }

    fn command(chars: &str) -> KeyPress<'_> {
        KeyPress {
            command: true,
            ..plain(chars)
        }
    }

    fn encode(key: KeyPress<'_>) -> Vec<u8> {
        match encode_key(key) {
            Some(KeyInput::Bytes(bytes)) => bytes.into_owned(),
            other => panic!("{key:?} gave no bytes: {other:?}"),
        }
    }

    #[test]
    fn only_char_is_the_single_owner_of_the_one_character_test() {
        // The criterion has three consumers (`encode_key`'s `single`, `page_scroll`,
        // `view::reaches_terminal`) and all three depend on this;
        // the contract is pinned here.
        assert_eq!(only_char("a"), Some('a'));
        // A multi-byte **single** character is a single character too: the criterion is the
        // character count, not bytes.
        assert_eq!(only_char("ğ"), Some('ğ'));
        assert_eq!(only_char("\u{7f}"), Some('\u{7f}'));
        // Two characters are not one — that is exactly the criterion, so that a composition's
        // output is not read from its first character.
        assert_eq!(only_char("ab"), None);
        assert_eq!(only_char("\u{f72c}x"), None);
        // The empty string (a bare modifier key) is not a single character either.
        assert_eq!(only_char(""), None);
    }

    #[test]
    fn return_and_delete_are_single_bytes() {
        assert_eq!(encode(plain("\r")), b"\r");
        assert_eq!(encode(plain("\u{7f}")), b"\x7f");
        assert_eq!(encode(plain("\t")), b"\t");
        assert_eq!(encode(plain("\u{1b}")), b"\x1b");
    }

    #[test]
    fn arrows_are_keys_not_bytes() {
        // An arrow's bytes depend on DECCKM (`\e[A` or `\eOA`) and the mode is in `bt-core`:
        // `\e[A` used to be written here unconditionally, whereas less, reading
        // `xterm-256color`'s terminfo, turns DECCKM on at startup and expects `\eOA`.
        assert_eq!(
            encode_key(plain("\u{f700}")),
            Some(KeyInput::Arrow(Arrow::Up))
        );
        assert_eq!(
            encode_key(plain("\u{f701}")),
            Some(KeyInput::Arrow(Arrow::Down))
        );
        assert_eq!(
            encode_key(plain("\u{f702}")),
            Some(KeyInput::Arrow(Arrow::Left))
        );
        assert_eq!(
            encode_key(plain("\u{f703}")),
            Some(KeyInput::Arrow(Arrow::Right))
        );
        // An arrow with Control is a plain arrow too: modified arrows (`\e[1;5A`) are out of scope.
        assert_eq!(
            encode_key(ctrl("\u{f700}")),
            Some(KeyInput::Arrow(Arrow::Up))
        );
        // Single-character match, the same criterion as PgUp: a composition starting with an
        // arrow is not read as an arrow from its first character.
        assert_eq!(encode_key(plain("\u{f700}x")), None);
    }

    #[test]
    fn ctrl_letter_yields_control_char_both_ways() {
        // AppKit applies Control itself for most keys: `characters` arrives directly as
        // U+0003. Both paths must give the same byte, otherwise whether Ctrl-C works would
        // depend on which path AppKit picked that day.
        assert_eq!(encode(ctrl("\u{3}")), b"\x03");
        assert_eq!(encode(ctrl("c")), b"\x03");
        assert_eq!(encode(ctrl("C")), b"\x03", "same with Shift");
    }

    #[test]
    fn numpad_enter_sends_newline_not_interrupt() {
        // U+0003 is the `characters` of two different keys: Ctrl-C and numpad Enter.
        // The only thing telling them apart is the Control flag; if they get mixed up, numpad
        // Enter interrupts every command instead of running it.
        assert_eq!(encode(plain("\u{3}")), b"\r");
        assert_eq!(encode(ctrl("\u{3}")), b"\x03");
    }

    #[test]
    fn plain_text_passes_as_utf8() {
        // The **usual** producer of a plain letter is no longer here but the AppKit stack's
        // `insertText:` (018). This test is still a guard: the plain-text branch is the only
        // path for groups 2 and 4 (see [`encode_key`]) and both groups can carry multi-byte
        // letters — when `insertText:`'s downcast fails, `ğ` passes through here.
        assert_eq!(encode(plain("a")), b"a");
        // A Turkish character is multi-byte: it must pass byte by byte, not truncated by `as u8`.
        assert_eq!(encode(plain("ğ")), "ğ".as_bytes());
        assert_eq!(encode(plain("İ")), "İ".as_bytes());
    }

    #[test]
    fn keys_without_sequences_are_swallowed() {
        // A bare modifier key: `characters` is empty. On the stack path this event reaches
        // `keyDown:`'s fallback as `None` and never comes here; the test still pins the
        // swallowing contract.
        assert!(encode_key(plain("")).is_none());
        // F1 and Home are in the private use area. Turning them into UTF-8 and writing them to
        // the PTY would send garbage to the shell; their sequences are in 00X. **The stack does
        // not swallow these**: both fall to `doCommandBySelector:`, pass through the no-op, and
        // the swallowing decision is still here.
        assert!(encode_key(plain("\u{f704}")).is_none(), "F1");
        assert!(encode_key(plain("\u{f729}")).is_none(), "Home");
    }

    #[test]
    fn page_keys_emit_xterm_sequences() {
        // PgUp/PgDn are no longer swallowed in the function key range: less and vim expect these
        // two sequences to page through. The sequences are `xterm-256color`'s `kpp`/`knp` —
        // `TERM` does not change.
        assert_eq!(encode(plain("\u{f72c}")), b"\x1b[5~");
        assert_eq!(encode(plain("\u{f72d}")), b"\x1b[6~");
        // Single-character match, the same criterion as `page_scroll`: a multi-character
        // `characters` starting with PgUp does not produce a sequence and drop the rest without a
        // trace — it is swallowed whole, like an unrecognised function key.
        assert!(encode_key(plain("\u{f72c}x")).is_none());
    }

    #[test]
    fn back_tab_and_forward_delete_emit_xterm_sequences() {
        // `xterm-256color`'s `kcbt` and `kdch1` (`infocmp`). Shift+Tab's `characters` is
        // `NSBackTabCharacter` (U+0019): a raw `0x19` used to go out through the plain-text
        // branch, and zsh's menu does not read it as going back.
        // fn+Backspace is `NSDeleteFunctionKey` (U+F728): it used to be swallowed in the function
        // key arm.
        assert_eq!(encode(plain("\u{19}")), b"\x1b[Z");
        assert_eq!(encode(plain("\u{f728}")), b"\x1b[3~");
        // U+0019 is also Ctrl-Y's `characters` (`'y' & 0x1f`) — readline's yank. Only the
        // Control flag tells them apart, like the numpad Enter/Ctrl-C pair; if mixed up, Ctrl-Y
        // becomes back-tab.
        assert_eq!(encode(ctrl("\u{19}")), b"\x19");
        assert_eq!(encode(ctrl("y")), b"\x19");
        // Single-character match, the same criterion as PgUp.
        assert!(encode_key(plain("\u{f728}x")).is_none());
        // Forward delete (⌦) with Option is a plain `kdch1` too: the Meta class is for backspace
        // and the arrows, forward delete is **out of scope** (`encode_key`'s doc).
        assert_eq!(encode(option("\u{f728}")), b"\x1b[3~");
    }

    #[test]
    fn option_navigation_sends_meta_sequences() {
        // Option's navigation/deletion class produces no printable character on any layout, so
        // it is Meta-encoded without consulting a setting (018 Karar 2).
        // The sequences were measured in default zsh: `\eb` `backward-word`, `\ef`
        // `forward-word`, `\e\x7f` `backward-kill-word`. The letter is **lowercase** —
        // the uppercase form is bound to other widgets (`\eA` =
        // `accept-and-hold`), so `\eB` would not move by word.
        assert_eq!(encode(option("\u{f702}")), b"\x1bb", "Option+←");
        assert_eq!(encode(option("\u{f703}")), b"\x1bf", "Option+→");
        assert_eq!(encode(option("\u{7f}")), b"\x1b\x7f", "Option+Delete");
        // The form without Option stays untouched: an arrow is still an arrow (its bytes depend
        // on DECCKM), ⌫ is still a single byte.
        assert_eq!(
            encode_key(plain("\u{f702}")),
            Some(KeyInput::Arrow(Arrow::Left))
        );
        assert_eq!(encode(plain("\u{7f}")), b"\x7f");
        // Up/down arrows with Option are still arrows: moving by word is a horizontal gesture,
        // it has no vertical counterpart.
        assert_eq!(
            encode_key(option("\u{f700}")),
            Some(KeyInput::Arrow(Arrow::Up))
        );
        // Single-character match, the same criterion as PgUp.
        assert!(encode_key(option("\u{f702}x")).is_none());
    }

    #[test]
    fn option_printable_characters_are_untouched() {
        // R3.2: on Turkish Q `{` = Option+7, `∫` = Option+b. If Option were Meta across the
        // board, the shell's metacharacters would become untypable (018
        // Karar 2) — Meta is only for the navigation/deletion class.
        //
        // The **usual** producer of these letters is now `insertText:`; only a type the stack
        // cannot resolve falls through to here (group 4), and even then the letter must pass
        // through as a letter.
        assert_eq!(encode(option("{")), b"{");
        assert_eq!(encode(option("∫")), "∫".as_bytes());
    }

    #[test]
    fn command_backspace_kills_the_whole_line() {
        // The allow-list's first key. `\x15` = `^U`, `kill-whole-line` in zsh: macOS's
        // "delete to the start of the line" is not bound by default in zsh (measured) and the
        // user's criterion is that the line goes away (018 Karar 3).
        assert_eq!(encode(command("\u{7f}")), b"\x15");
        // Cmd comes **before** Option: ⌘⌥⌫ deletes the line, not the word.
        assert_eq!(
            encode(KeyPress {
                option: true,
                ..command("\u{7f}")
            }),
            b"\x15"
        );
        // ⌫ without Cmd is still a single byte — the flag carries the decision, not the character.
        assert_eq!(encode(plain("\u{7f}")), b"\x7f");
        // **Only** a key that passed the allow-list comes here
        // (`view::reaches_terminal`); if another character with Cmd came in, the flag would not
        // change it.
        assert_eq!(encode(command("t")), b"t");
    }

    #[test]
    fn command_arrows_jump_to_the_line_edges() {
        // The allow-list's other two keys: `\x01` = `^A` (`beginning-of-line`),
        // `\x05` = `^E` (`end-of-line`). The shape is **not** `Arrow`: these two bytes are
        // independent of DECCKM, just like ⌘⌫'s `\x15` — the arrow byte depending on the mode is
        // the subject of the Cmd-less arm below.
        let (left, right) = (ARROW_LEFT.to_string(), ARROW_RIGHT.to_string());
        assert_eq!(encode(command(&left)), b"\x01");
        assert_eq!(encode(command(&right)), b"\x05");
        // Cmd comes **before** Option (the ⌘⌥⌫ precedent): ⌘⌥← goes to the start of the line,
        // not the start of the word.
        assert_eq!(
            encode(KeyPress {
                option: true,
                ..command(&left)
            }),
            b"\x01"
        );
        // An arrow without Cmd is still an **arrow**, not bytes: `bt-core` knows the mode.
        assert_eq!(encode_key(plain(&left)), Some(KeyInput::Arrow(Arrow::Left)));
        assert_eq!(
            encode_key(plain(&right)),
            Some(KeyInput::Arrow(Arrow::Right))
        );
        // Single-character match, the same criterion as PgUp and ⌘⌫: a composition's first
        // character is not read as start-of-line.
        assert!(encode_key(command(&format!("{ARROW_LEFT}x"))).is_none());
    }

    #[test]
    fn shift_page_keys_scroll_the_view() {
        // Shift+PgUp/PgDn is the terminal's own key: scroll by one page. The direction matches
        // `Session::scroll_page`'s sign — plus is backwards.
        assert_eq!(page_scroll("\u{f72c}", true), Some(1));
        assert_eq!(page_scroll("\u{f72d}", true), Some(-1));
        // The form without Shift belongs to the application (the sequences above).
        assert_eq!(page_scroll("\u{f72c}", false), None);
        assert_eq!(page_scroll("\u{f72d}", false), None);
        // Any other key, even with Shift, is not a scroll.
        assert_eq!(page_scroll("\u{f700}", true), None);
        assert_eq!(page_scroll("a", true), None);
        // Single-character match: a composition's first character is not read as
        // PgUp.
        assert_eq!(page_scroll("\u{f72c}x", true), None);
    }
}
