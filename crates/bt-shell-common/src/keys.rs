//! Keystroke → PTY bytes or a function key, the scroll decision for Shift+PgUp/PgDn, and the
//! dock selection's keys ([`dock_key`]).
//! **Pure and AppKit-free**, and therefore testable.
//!
//! A function key's bytes are **not written** here, only which key it is and which modifiers
//! ride with it (`bt_core::FunctionKey`, the reason is there). A typing key that may mean more
//! than its text (Esc, ⏎, ⇥, ⌫, a Control key) carries its everyday bytes and its identity
//! (`bt_core::TextKey`): which of the two goes is the program's choice, in `bt-core`.

use std::borrow::Cow;

use bt_core::{Arrow, Chord, DockKey, FunctionKey, Key, Mods, NamedKey, TextKey};

/// AppKit's function key range: U+F700–U+F8FF. The named constants
/// (`NSUpArrowFunctionKey` … `NSModeSwitchFunctionKey`) use its first slice. The arrows, Home,
/// End, PgUp/PgDn, forward delete and F1–F20 are turned into [`KeyInput::Key`]; the
/// **unrecognised rest** of the range is swallowed on purpose, because turning a key code we do
/// not know into UTF-8 and sending it to the shell is always worse. The Private Use Area is wider
/// than this (it starts at U+E000) and is **out of scope**: a real character such as a powerline
/// glyph goes through the plain-text branch.
const FUNCTION_KEYS: std::ops::RangeInclusive<char> = '\u{f700}'..='\u{f8ff}';

/// `NSF1FunctionKey`–`NSF12FunctionKey`, and `NSF13FunctionKey`–`NSF20FunctionKey` after them.
/// xterm has no F13: `xterm-256color` calls ⇧F1 `kf13`, so F13–F20 go as ⇧F1–⇧F8.
const F1_TO_F12: std::ops::RangeInclusive<char> = '\u{f704}'..='\u{f70f}';
const F13_TO_F20: std::ops::RangeInclusive<char> = '\u{f710}'..='\u{f717}';

/// `NSHomeFunctionKey` and `NSEndFunctionKey` — fn+← and fn+→ on a laptop.
const HOME: char = '\u{f729}';
const END: char = '\u{f72b}';

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

/// `NSDeleteFunctionKey` — forward delete (⌦, fn-⌫). Read in three places: its key and its
/// Option gesture ([`encode_key`], [`LINE_EDITING`]) and the dock selection's key ([`dock_key`]).
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

/// A key that edits the line you type. It reaches the terminal as a gesture of its own
/// ([`encode_key`] gives it bytes), not as the menu's command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineKey {
    pub chord: Chord,
    /// The character AppKit reports for the key — the one [`encode_key`] and
    /// `view::reaches_terminal` read.
    pub character: char,
    /// What the gesture is called, as a row of the shortcut list.
    pub title: &'static str,
    /// What the key does, finishing "⌘← …".
    pub effect: &'static str,
}

const fn line_key(option: bool, command: bool, key: NamedKey) -> Chord {
    Chord::plain(Mods::new(false, option, false, command), Key::Named(key))
}

/// The keys [`encode_key`] gives a meaning of its own beyond the plain key: ⌘ with ⌫, ← and →, ⌥
/// with ⌫, ⌦, ←, → and ⏎. A modifier that only rides along with its key (⇧←, ⌃F5 — xterm's
/// encoding, [`KeyInput::Key`]) is not a meaning of its own and is not here. The list is
/// **named** because other readers need it: a shortcut given to one of these
/// takes it from the line (the menu catches the key first), and the shortcut list shows them
/// as built in. [`encode_key`] and this list are tied by a test — a key given a gesture there
/// has to be here, and the other way round.
pub const LINE_EDITING: [LineKey; 8] = [
    LineKey {
        chord: line_key(false, true, NamedKey::Backspace),
        character: BACKSPACE,
        title: "Delete to Start of Line",
        effect: "deletes the line you type, back to its start",
    },
    LineKey {
        chord: line_key(false, true, NamedKey::Left),
        character: ARROW_LEFT,
        title: "Move to Start of Line",
        effect: "moves to the start of the line you type",
    },
    LineKey {
        chord: line_key(false, true, NamedKey::Right),
        character: ARROW_RIGHT,
        title: "Move to End of Line",
        effect: "moves to the end of the line you type",
    },
    LineKey {
        chord: line_key(true, false, NamedKey::Backspace),
        character: BACKSPACE,
        title: "Delete Word Backward",
        effect: "deletes the word before the cursor",
    },
    LineKey {
        chord: line_key(true, false, NamedKey::Delete),
        character: FORWARD_DELETE,
        title: "Delete Word Forward",
        effect: "deletes the word after the cursor",
    },
    LineKey {
        chord: line_key(true, false, NamedKey::Left),
        character: ARROW_LEFT,
        title: "Move One Word Left",
        effect: "moves the cursor one word left",
    },
    LineKey {
        chord: line_key(true, false, NamedKey::Right),
        character: ARROW_RIGHT,
        title: "Move One Word Right",
        effect: "moves the cursor one word right",
    },
    LineKey {
        chord: line_key(true, false, NamedKey::Return),
        character: '\r',
        title: "Insert Line Break",
        effect: "starts a new line without running what you typed",
    },
];

/// The line-editing key a chord is, if it is one.
pub fn line_editing(chord: &Chord) -> Option<&'static LineKey> {
    LINE_EDITING.iter().find(|key| key.chord == *chord)
}

/// [`encode_key`]'s answer: a key with known bytes, a function key with its modifiers, or a
/// typing key whose bytes the program chooses.
#[derive(Debug, PartialEq, Eq)]
pub enum KeyInput {
    /// Bytes independent of the terminal's state: a letter, ⌥←, ⌘⌫…
    Bytes(Cow<'static, [u8]>),
    /// Goes to `Session::write_key`: its bytes depend on the mode and on whether a shell is
    /// reading the line. ⌘ never rides here — a ⌘ key that reaches the terminal is bytes.
    Key(FunctionKey, Mods),
    /// Goes to `Session::write_text_key`: Esc, ⏎, ⇥, ⌫ or a Control key, which a program that
    /// asked for the kitty keyboard protocol hears as the key it is (⇧⏎ apart from ⏎, ⌃[ apart
    /// from Esc) and everyone else as the bytes it always sent.
    Text(TextKey),
}

/// [`encode_key`]'s input: the half of `NSEvent` that concerns this function.
///
/// **One record**, not five separate parameters, because the arms ask the flags **together
/// with** the character — ⌘⌫ and ⌥⌫ differ only by flag from the same `characters` (`BACKSPACE`)
/// — and every new modifier would mean walking the call sites one by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPress<'a> {
    /// `NSEvent.characters` — the form with modifiers **applied** (Option-held `ø`, Ctrl-C →
    /// U+0003).
    pub chars: &'a str,
    /// Shift is held. A letter already carries it (`characters` is `A`, Shift+Tab is U+0019); a
    /// function key carries it in its modifiers ([`KeyInput::Key`]), and it chooses the dock's
    /// selection keys ([`dock_key`]).
    pub shift: bool,
    /// Control is held.
    pub ctrl: bool,
    /// Option (⌥) is held. It only enables the navigation/deletion class; it does not touch
    /// printable letters (below).
    pub option: bool,
    /// Command (⌘) is held. **Only** a key that passed the allow-list arrives here
    /// (`view::reaches_terminal`), so the flag's only job is to tell the list's three keys (⌘⌫,
    /// ⌘←, ⌘→) apart from their Option forms.
    pub command: bool,
    /// The key's character with **no** modifier applied (`charactersByApplyingModifiers:` with
    /// none): `c` for ⌃⇧C, `[` for ⌃[ whose `characters` is Esc's. It names a typing key to a
    /// program that asked for the kitty protocol ([`KeyInput::Text`]); `None` when AppKit
    /// gives none or more than one character.
    pub bare: Option<char>,
}

/// Keystroke ([`KeyPress`]) → PTY bytes or a function key.
///
/// **Not every key comes here any more.** The text path goes through AppKit's stack
/// (`view::BateriView`'s `NSTextInputClient` conformance) and the usual producer of a printable
/// letter is `insertText:`, not this function. **Five** groups remain here:
///
/// 1. **An event with Control** — `keyDown:` never hands it to the stack (numpad Enter's U+0003
///    and Ctrl-Y's U+0019 are shared; leaving the arm to AppKit could interrupt every command).
/// 2. **A key the stack hands to `doCommandBySelector:`** — Enter, Tab, Escape, Backspace, the
///    arrows, Home/End, the F keys, Shift+Tab, PgUp/PgDn, fn+Backspace **and Option
///    navigation/deletion** (`moveWordLeft:`, `deleteWordBackward:`, `deleteWordForward:`,
///    `insertNewlineIgnoringFieldEditor:`). That method is a silent no-op and the event returns
///    unflagged, so its bytes come from here.
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
/// **Function keys carry their modifiers** ([`KeyInput::Key`]): ⇧← reaches a program as xterm's
/// `\e[1;2D`, ⌃F5 as `\e[15;5~` — except at the prompt of a shell that did not ask for the
/// kitty protocol, where `bt-core` drops them (`bt_core::FunctionKey`). Option's line gestures are not this: ⌥← stays the Meta `\eb` every
/// line editor binds, with or without Shift or Control beside it — under the kitty protocol too,
/// as a key binding of the terminal's.
///
/// **Typing keys that may mean more than their text** — Esc, ⏎, ⇥, ⌫ and every key with Control
/// — carry their everyday bytes and their bare character ([`KeyInput::Text`]): a program that
/// asked for the kitty keyboard protocol hears ⇧⏎ apart from ⏎ and ⌃[ apart from Esc
/// (`bt_core::TextKey`). A printable key without Control is text, Option or not: Option composes
/// a character on a Mac.
///
/// **Out of scope:** IME; **Option as Meta across the board** — Meta encoding only for the
/// navigation/deletion class, printable letters do not change (`Option+7` keeps typing `{` on
/// Turkish Q); xterm's `modifyOtherKeys` (where no program asked for the kitty protocol, a
/// modified ⏎, ⇥ or ⌫ keeps its plain byte); the kitty protocol's levels above the first
/// (release events, the shifted and base-layout keys, text as escape codes) and the keypad as
/// keys of its own — numpad Enter is ⏎; Insert, which a Mac keyboard does not have. **Dead keys stay out of scope and are no longer a debt**: AppKit's stack
/// completes the composition, it never passes through here.
pub fn encode_key(key: KeyPress<'_>) -> Option<KeyInput> {
    let c = key.chars.chars().next()?;
    // Whether `characters` is a single character — the shared guard of the arms that come out of
    // a single `c` (below: backspace, the function keys and Control):
    // multi-character input (a dead-key composition, marked text) is not read from its first
    // character. The criterion's owner is [`only_char`] — `page_scroll` and
    // `view::reaches_terminal` ask the same question from there too.
    let single = only_char(key.chars).is_some();
    // What rides with a function key. ⌘ never does: the three ⌘ keys that reach here are bytes.
    let mods = Mods::new(key.ctrl, key.option, key.shift, false);
    let function = |key: FunctionKey, mods: Mods| Some(KeyInput::Key(key, mods));
    // A typing key that may mean more than its text — Esc, ⏎, ⇥, ⌫, any key with Control —
    // carries what it is beside its everyday bytes; which of the two the program hears is
    // `bt-core`'s (`bt_core::TextKey`). Anything else is its bytes.
    // AppKit's layouts name ⌫ by BS (U+0008) when no modifier is applied, though `characters`
    // says DEL (measured): the key is ⌫ either way, and the protocol knows it as DEL. No other key
    // is BS bare — ⌃H is `h`.
    let bare_of =
        |bare: Option<char>| bare.map(|bare| if bare == '\u{8}' { BACKSPACE } else { bare });
    let typing = |legacy: Cow<'static, [u8]>, bare: Option<char>| {
        let bare = bare_of(bare);
        Some(match bare {
            Some(bare) if key.ctrl || matches!(bare, '\u{1b}' | '\r' | '\t' | BACKSPACE) => {
                KeyInput::Text(TextKey {
                    key: bare,
                    mods,
                    legacy,
                })
            }
            _ => KeyInput::Bytes(legacy),
        })
    };
    let bytes: Cow<'static, [u8]> = match (c, key.ctrl) {
        // The first key of Cmd's closed allow-list: ⌘⌫ → `\x15` (`^U`, `kill-whole-line` in
        // zsh). macOS's strict meaning is "delete **up to the start** of the line", but in zsh
        // `backward-kill-line` is not bound at all by default (measured) — the expectation is
        // that the line goes away, and `^U` does exactly that. The arm comes
        // **before** Option's: ⌘⌥⌫ deletes the line, not the word — the allow-list is a named
        // exception, Option's class is a rule.
        (BACKSPACE, _) if key.command && single => Cow::Borrowed(b"\x15"),
        // The allow-list's other two keys: ⌘← → `\x01` (`^A`, `beginning-of-line`), ⌘→ →
        // `\x05` (`^E`, `end-of-line`). The same decision as ⌘⌫ — macOS's start/end-of-line
        // gesture, with the byte that **actually** does that job in zsh.
        //
        // An earlier decision had rejected these two keys and its reasoning had two parts: "not asked
        // for" and "the Home/End sequences are unbound in zsh". The first fell away (the user
        // asked, 2026-09-21), the second **was never a reason about these keys**: the
        // measurement (a zsh without macOS's `/etc/zshrc`, which binds `^[OH`/`^[OF`) showed no
        // binding for Home/End, but `^A`/`^E` are bound to `beginning-of-line`/`end-of-line` in
        // the emacs keymap (zsh's default) — so the rejection's measurement belonged to the shape
        // of Home/End, not to these bytes.
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
        // is in Option **letters**, and those stay untouched. The sequences are
        // **lowercase**: the uppercase form is bound to other widgets in zsh (`\eA` =
        // `accept-and-hold`; measured).
        //
        // They come here from group 2: the stack hands Option+arrow/⌫/⌦/⏎ to
        // `doCommandBySelector:` (`moveWordLeft:`, `deleteWordBackward:`), that method is a
        // silent no-op and the event returns unconsumed. Giving that method a body would
        // silently kill this arm.
        //
        // Accompanying modifiers are not consulted (the `page_scroll` precedent): Option+arrow
        // with Ctrl or Shift also moves by word, ⌥ has no second meaning in this class.
        //
        // ⌥⌦ is ⌥⌫'s mirror, macOS's "delete the next word": `\ed` is `kill-word` in zsh, bash
        // and fish (measured; no garbage at their prompts). ⌥⏎ is `\e\r`, the sequence Codex and
        // Claude Code read as a new line without sending — and so do zsh and fish at their prompt
        // (measured: the line gets a line break); bash leaves it unbound and nothing happens.
        // Control beside ⌥⏎ is not this gesture: ⌃⌥⏎ stays a plain ⏎.
        (BACKSPACE, _) if key.option && single => Cow::Borrowed(b"\x1b\x7f"),
        (FORWARD_DELETE, _) if key.option && single => Cow::Borrowed(b"\x1bd"),
        (ARROW_LEFT, _) if key.option && single => Cow::Borrowed(b"\x1bb"),
        (ARROW_RIGHT, _) if key.option && single => Cow::Borrowed(b"\x1bf"),
        ('\r', false) if key.option && single => Cow::Borrowed(b"\x1b\r"),
        // The numeric keypad's Enter and Fn-Return give `NSEnterCharacter` = U+0003 — the same
        // as Ctrl-C's byte. If Ctrl is NOT held this is a line ending; without this arm it would
        // pass through the plain-text branch as 0x03 and numpad Enter would interrupt every
        // command instead of running it. To a program it is ⏎.
        ('\u{3}', false) => return typing(Cow::Borrowed(b"\r"), Some('\r')),
        // Shift+Tab: `characters` gives `NSBackTabCharacter` = U+0019. `xterm-256color`'s `kcbt`
        // is `\e[Z` — zsh's completion menu and readline read back-tab from this sequence, not
        // from a raw 0x19. U+0019 is also Ctrl-Y's byte (yank); again the Control flag tells them
        // apart, exactly like the U+0003 arm above. The Ctrl form goes out as 0x19 through the
        // plain-text branch. To a program that tells keys apart it is ⇧⇥.
        ('\u{19}', false) if single => return typing(Cow::Borrowed(b"\x1b[Z"), Some('\t')),
        // The function keys: which key, and what rides with it. Their bytes are `bt-core`'s
        // (DECCKM, the prompt), the reason is in `bt_core::FunctionKey`. They are all in the
        // function key range and are written **before** the swallowing arm below.
        ('\u{f700}', _) if single => return function(FunctionKey::Arrow(Arrow::Up), mods),
        ('\u{f701}', _) if single => return function(FunctionKey::Arrow(Arrow::Down), mods),
        (ARROW_LEFT, _) if single => return function(FunctionKey::Arrow(Arrow::Left), mods),
        (ARROW_RIGHT, _) if single => return function(FunctionKey::Arrow(Arrow::Right), mods),
        (HOME, _) if single => return function(FunctionKey::Home, mods),
        (END, _) if single => return function(FunctionKey::End, mods),
        // `xterm-256color`'s `kpp`/`knp`: less and vim read paging from these two sequences. The
        // Shift form **does not come** here — that is the terminal's scrolling
        // ([`page_scroll`]), which `view` asks first. When scrolling is rejected on the alternate
        // screen, the Shift key falls through to here and goes **without** its Shift: less reads
        // no `\e[5;2~`, and in less and vim Shift+PgUp turns the page like PgUp.
        //
        // `single` (above): the same criterion as `page_scroll`; multi-character input is
        // swallowed whole in the function key arm below.
        (PAGE_UP | PAGE_DOWN, _) if single => {
            let key = if c == PAGE_UP {
                FunctionKey::PageUp
            } else {
                FunctionKey::PageDown
            };
            return function(
                key,
                Mods {
                    shift: false,
                    ..mods
                },
            );
        }
        // `kdch1`: deletes the character to the right of the cursor.
        // `NSDeleteFunctionKey`: fn+Backspace, Delete (⌦) on a full keyboard.
        (FORWARD_DELETE, _) if single => return function(FunctionKey::Delete, mods),
        (c, _) if single && F1_TO_F12.contains(&c) => {
            return function(FunctionKey::F(fkey_number(c, *F1_TO_F12.start())), mods);
        }
        (c, _) if single && F13_TO_F20.contains(&c) => {
            let number = fkey_number(c, *F13_TO_F20.start());
            return function(
                FunctionKey::F(number),
                Mods {
                    shift: true,
                    ..mods
                },
            );
        }
        // AppKit applies Control to `characters` itself for most keys (Ctrl-C → U+0003) and that
        // form goes through the plain-text branch below. But not for all of them — with
        // Ctrl-Shift-C the letter stays a letter. The conversion is repeated here so that both
        // paths give the same byte.
        // `single`: this arm only produces bytes from `c`, so if a multi-character `characters`
        // came in, everything after the first character would be dropped without a trace. Such
        // input should go to the plain-text branch, where all of it passes through.
        (c, true) if c.is_ascii_alphabetic() && single => {
            let legacy = Cow::Owned(vec![(c.to_ascii_lowercase() as u8) & 0x1f]);
            return typing(legacy, key.bare.or(Some(c.to_ascii_lowercase())));
        }
        // A function key whose sequence we do not know (F21, Insert, Print Screen…). These are
        // not real characters but AppKit's private use codes: turning them into UTF-8 and writing
        // them to the PTY would send garbage to the shell.
        (c, _) if FUNCTION_KEYS.contains(&c) => return None,
        // Enter, Tab, Escape and Backspace also pass through here: AppKit has already turned
        // them into the right byte (U+000D, U+0009, U+001B, U+007F) and writing separate arms
        // would describe the same byte a second time.
        // `return_and_delete_are_single_bytes` pins the contract.
        _ if single => {
            return typing(
                Cow::Owned(key.chars.as_bytes().to_vec()),
                key.bare.or(Some(c)),
            );
        }
        _ => Cow::Owned(key.chars.as_bytes().to_vec()),
    };
    Some(KeyInput::Bytes(bytes))
}

/// The number of an F key in a run of AppKit's codes that starts at F1's (`first` is the run's
/// first code): `NSF5FunctionKey` → 5.
fn fkey_number(c: char, first: char) -> u8 {
    u8::try_from(u32::from(c) - u32::from(first) + 1).unwrap_or(0)
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

/// A key the terminal can handle while there is a dock selection — what each one
/// does lives in `bt-core` (`Session::dock_key`), this is only `NSEvent`'s dictionary.
///
/// **Unmodified** ⌫, ⌦, ←, →, the two Shift arrows, ⏎ and ⇧⏎. A key carrying Option, Control or
/// Command is never a dock key: ⌥⌫ is `backward-kill-word`, ⌘⌫ is `kill-whole-line`, and both go
/// their current way and remove the selection — the "any other key" arm. Shift+⌫ counts as a
/// plain ⌫: it is the same key on macOS too.
pub fn dock_key(key: KeyPress<'_>) -> Option<DockKey> {
    if key.ctrl || key.option || key.command {
        return None;
    }
    match (only_char(key.chars)?, key.shift) {
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
        // Plain ⏎: consumed only while a reconnect offer is showing; otherwise
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

    /// `encode_key` with ⌘ or ⌥ held, and with nothing held, for one character.
    fn held_and_plain(
        c: char,
        option: bool,
        command: bool,
    ) -> (Option<KeyInput>, Option<KeyInput>) {
        let chars = c.to_string();
        let held = KeyPress {
            chars: &chars,
            shift: false,
            ctrl: false,
            option,
            command,
            bare: Some(c),
        };
        let plain = KeyPress {
            chars: &chars,
            shift: false,
            ctrl: false,
            option: false,
            command: false,
            bare: Some(c),
        };
        (encode_key(held), encode_key(plain))
    }

    /// Whether `held` is only `plain` with the held modifier riding along — the same function key
    /// in xterm's encoding (⌥↑, ⌥F5), or the same typing key with the same everyday bytes (⌥⇥,
    /// which only a program that asked tells apart) — not a meaning of its own.
    fn rides_along(held: &Option<KeyInput>, plain: &Option<KeyInput>) -> bool {
        match (held, plain) {
            (Some(KeyInput::Key(held, _)), Some(KeyInput::Key(plain, _))) => held == plain,
            (Some(KeyInput::Text(held)), Some(KeyInput::Text(plain))) => {
                held.key == plain.key && held.legacy == plain.legacy
            }
            _ => false,
        }
    }

    #[test]
    fn every_line_editing_key_is_coded_by_encode_key_and_no_other_is() {
        // Each key of the list gets bytes of its own under its modifier.
        for key in &LINE_EDITING {
            let (held, plain) =
                held_and_plain(key.character, key.chord.mods.option, key.chord.mods.cmd);
            assert!(held.is_some(), "{}", key.title);
            assert_ne!(held, plain, "{} is coded like the plain key", key.title);
            assert!(
                !key.chord.mods.ctrl && !key.chord.mods.shift,
                "{}",
                key.title
            );
            assert_eq!(line_editing(&key.chord), Some(key));
        }
        // And a key that gets bytes of its own under ⌘ or ⌥ is in the list: every printable
        // ASCII character, AppKit's function keys, and the control characters a key reports. A
        // modifier that only rides along with a function key is not a meaning of its own.
        let candidates = (' '..='~')
            .chain('\u{f700}'..='\u{f8ff}')
            .chain(['\r', '\t', '\u{1b}', '\u{3}', '\u{8}', '\u{7f}']);
        for c in candidates {
            for (option, command) in [(true, false), (false, true)] {
                let (held, plain) = held_and_plain(c, option, command);
                if held != plain && !rides_along(&held, &plain) {
                    assert!(
                        LINE_EDITING.iter().any(|key| key.character == c
                            && key.chord.mods.option == option
                            && key.chord.mods.cmd == command),
                        "{c:?} with {} has a meaning in encode_key and is not in LINE_EDITING",
                        if option { "⌥" } else { "⌘" }
                    );
                }
            }
        }
    }

    #[test]
    fn the_commands_allow_list_is_the_lists_command_keys() {
        // `view::reaches_terminal` lets ⌘⌫, ⌘← and ⌘→ through to the terminal: the ⌘ keys of
        // the list, by their characters.
        let mut with_command: Vec<char> = LINE_EDITING
            .iter()
            .filter(|key| key.chord.mods.cmd)
            .map(|key| key.character)
            .collect();
        with_command.sort_unstable();
        let mut allowed = vec![ARROW_LEFT, ARROW_RIGHT, BACKSPACE];
        allowed.sort_unstable();
        assert_eq!(with_command, allowed);
    }

    #[test]
    fn the_dock_keys_are_the_plain_eight() {
        assert_eq!(dock_key(plain("\u{7f}")), Some(DockKey::Backspace));
        assert_eq!(dock_key(shifted(plain("\u{7f}"))), Some(DockKey::Backspace));
        assert_eq!(dock_key(plain("\u{f728}")), Some(DockKey::Delete));
        assert_eq!(dock_key(plain("\u{f702}")), Some(DockKey::Left));
        assert_eq!(dock_key(plain("\u{f703}")), Some(DockKey::Right));
        assert_eq!(
            dock_key(shifted(plain("\u{f702}"))),
            Some(DockKey::ShiftLeft)
        );
        assert_eq!(
            dock_key(shifted(plain("\u{f703}"))),
            Some(DockKey::ShiftRight)
        );
        assert_eq!(dock_key(shifted(plain("\r"))), Some(DockKey::NewLine));
        assert_eq!(dock_key(plain("\r")), Some(DockKey::Enter));
        assert_eq!(dock_key(plain("\u{3}")), Some(DockKey::Enter));
        // Ctrl-C (the same `characters`, with Control) is not a dock key.
        assert_eq!(dock_key(ctrl("\u{3}")), None);
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
            assert_eq!(dock_key(flag), None);
        }
        // ⌥⇧⏎ and ⌃⇧⏎ are not dock keys.
        assert_eq!(dock_key(shifted(option("\r"))), None);
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
            assert_eq!(dock_key(key), None, "{:?}", key.chars);
        }
    }

    /// An unmodified keystroke. Arms that turn on a single flag are written with `..plain(chars)`,
    /// so the test line names which flag is the subject.
    fn plain(chars: &str) -> KeyPress<'_> {
        KeyPress {
            chars,
            shift: false,
            ctrl: false,
            option: false,
            command: false,
            bare: only_char(chars),
        }
    }

    fn shifted(key: KeyPress<'_>) -> KeyPress<'_> {
        KeyPress { shift: true, ..key }
    }

    /// A function key with its modifiers, as [`encode_key`] answers.
    fn key(key: FunctionKey, mods: Mods) -> Option<KeyInput> {
        Some(KeyInput::Key(key, mods))
    }

    const SHIFT: Mods = Mods::new(false, false, true, false);
    const OPTION: Mods = Mods::new(false, true, false, false);
    const CTRL: Mods = Mods::new(true, false, false, false);

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

    /// What a key sends where no program asked for the kitty protocol: its bytes, or a typing
    /// key's everyday bytes. A function key has none here.
    fn encode(key: KeyPress<'_>) -> Vec<u8> {
        match encode_key(key) {
            Some(KeyInput::Bytes(bytes)) => bytes.into_owned(),
            Some(KeyInput::Text(key)) => key.legacy.into_owned(),
            other => panic!("{key:?} gave no bytes: {other:?}"),
        }
    }

    /// A typing key, as [`encode_key`] answers.
    fn typing(key: char, mods: Mods, legacy: &'static [u8]) -> Option<KeyInput> {
        Some(KeyInput::Text(TextKey {
            key,
            mods,
            legacy: Cow::Borrowed(legacy),
        }))
    }

    #[test]
    fn a_typing_key_that_may_mean_more_says_what_it_is() {
        // Esc, ⏎, ⇥ and ⌫, with or without a modifier: `bt-core` decides whether a program hears
        // more than the byte.
        assert_eq!(
            encode_key(plain("\u{1b}")),
            typing('\u{1b}', Mods::NONE, b"\x1b")
        );
        assert_eq!(encode_key(plain("\r")), typing('\r', Mods::NONE, b"\r"));
        assert_eq!(encode_key(shifted(plain("\r"))), typing('\r', SHIFT, b"\r"));
        assert_eq!(encode_key(ctrl("\r")), typing('\r', CTRL, b"\r"));
        assert_eq!(encode_key(option("\t")), typing('\t', OPTION, b"\t"));
        assert_eq!(
            encode_key(shifted(ctrl("\u{7f}"))),
            typing('\u{7f}', Mods::new(true, false, true, false), b"\x7f")
        );
        // ⇧⇥ and numpad Enter keep their everyday bytes and say which key they are.
        assert_eq!(
            encode_key(shifted(plain("\u{19}"))),
            typing('\t', SHIFT, b"\x1b[Z")
        );
        assert_eq!(encode_key(plain("\u{3}")), typing('\r', Mods::NONE, b"\r"));
        // A Control key is named by its bare character, not by its control byte: ⌃[ is not Esc.
        let bare = |key: KeyPress<'static>, bare| KeyPress {
            bare: Some(bare),
            ..key
        };
        assert_eq!(
            encode_key(bare(ctrl("\u{3}"), 'c')),
            typing('c', CTRL, b"\x03")
        );
        assert_eq!(
            encode_key(bare(ctrl("\u{1b}"), '[')),
            typing('[', CTRL, b"\x1b")
        );
        assert_eq!(
            encode_key(bare(shifted(ctrl("C")), 'c')),
            typing('c', Mods::new(true, false, true, false), b"\x03")
        );
        // ⌫ is BS in the layout and DEL to the protocol.
        assert_eq!(
            encode_key(bare(shifted(plain("\u{7f}")), '\u{8}')),
            typing('\u{7f}', SHIFT, b"\x7f")
        );
        // Without a bare character a Control letter is named by its own.
        assert_eq!(
            encode_key(KeyPress {
                bare: None,
                ..ctrl("C")
            }),
            typing('c', CTRL, b"\x03")
        );
        // Text is text, with Shift or Option: a Mac composes a character with Option.
        assert_eq!(
            encode_key(shifted(plain("A"))),
            Some(KeyInput::Bytes(Cow::Borrowed(b"A")))
        );
        assert_eq!(
            encode_key(bare(option("å"), 'a')),
            Some(KeyInput::Bytes(Cow::Borrowed("å".as_bytes())))
        );
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
        let arrow = |direction| key(FunctionKey::Arrow(direction), Mods::NONE);
        assert_eq!(encode_key(plain("\u{f700}")), arrow(Arrow::Up));
        assert_eq!(encode_key(plain("\u{f701}")), arrow(Arrow::Down));
        assert_eq!(encode_key(plain("\u{f702}")), arrow(Arrow::Left));
        assert_eq!(encode_key(plain("\u{f703}")), arrow(Arrow::Right));
        // Single-character match, the same criterion as PgUp: a composition starting with an
        // arrow is not read as an arrow from its first character.
        assert_eq!(encode_key(plain("\u{f700}x")), None);
    }

    #[test]
    fn a_function_key_carries_shift_and_control() {
        // `bt-core` turns them into xterm's parameter (`\e[1;2D`), or drops them at a prompt.
        let left = FunctionKey::Arrow(Arrow::Left);
        assert_eq!(encode_key(shifted(plain("\u{f702}"))), key(left, SHIFT));
        assert_eq!(encode_key(ctrl("\u{f702}")), key(left, CTRL));
        let up = FunctionKey::Arrow(Arrow::Up);
        let both = Mods::new(true, false, true, false);
        assert_eq!(encode_key(shifted(ctrl("\u{f700}"))), key(up, both));
        assert_eq!(
            encode_key(shifted(plain("\u{f729}"))),
            key(FunctionKey::Home, SHIFT)
        );
        assert_eq!(encode_key(ctrl("\u{f728}")), key(FunctionKey::Delete, CTRL));
        // A ⌘ key that reaches the terminal is bytes; ⌘ never rides with a function key.
        assert_eq!(encode(shifted(command("\u{f702}"))), b"\x01");
    }

    #[test]
    fn home_end_and_the_f_keys_are_keys() {
        // fn+← and fn+→ on a laptop. They used to be swallowed.
        assert_eq!(
            encode_key(plain("\u{f729}")),
            key(FunctionKey::Home, Mods::NONE)
        );
        assert_eq!(
            encode_key(plain("\u{f72b}")),
            key(FunctionKey::End, Mods::NONE)
        );
        // `NSF1FunctionKey` … `NSF12FunctionKey`.
        for n in 1..=12u8 {
            let c = char::from_u32(0xf703 + u32::from(n)).expect("a function key");
            assert_eq!(
                encode_key(plain(&c.to_string())),
                key(FunctionKey::F(n), Mods::NONE),
                "F{n}"
            );
        }
        // F13–F20 are xterm's ⇧F1–⇧F8 (`kf13=\E[1;2P`).
        assert_eq!(encode_key(plain("\u{f710}")), key(FunctionKey::F(1), SHIFT));
        assert_eq!(encode_key(plain("\u{f717}")), key(FunctionKey::F(8), SHIFT));
        assert_eq!(
            encode_key(ctrl("\u{f714}")),
            key(FunctionKey::F(5), Mods::new(true, false, true, false))
        );
        assert_eq!(encode_key(plain("\u{f704}x")), None);
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
        // `insertText:`. This test is still a guard: the plain-text branch is the only
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
        // F21 and Insert are in the private use area and have no sequence here. Turning them
        // into UTF-8 and writing them to the PTY would send garbage to the shell. **The stack does
        // not swallow these**: both fall to `doCommandBySelector:`, pass through the no-op, and
        // the swallowing decision is still here.
        assert!(encode_key(plain("\u{f718}")).is_none(), "F21");
        assert!(encode_key(plain("\u{f727}")).is_none(), "Insert");
    }

    #[test]
    fn page_keys_emit_xterm_sequences() {
        // PgUp/PgDn are no longer swallowed in the function key range: less and vim expect these
        // two sequences to page through. The sequences are `xterm-256color`'s `kpp`/`knp` —
        // `TERM` does not change.
        assert_eq!(
            encode_key(plain("\u{f72c}")),
            key(FunctionKey::PageUp, Mods::NONE)
        );
        assert_eq!(
            encode_key(plain("\u{f72d}")),
            key(FunctionKey::PageDown, Mods::NONE)
        );
        // Shift is the terminal's scrolling: when the session refuses it (the alternate
        // screen) the key goes without it — less reads no `\e[5;2~`. Control rides along.
        assert_eq!(
            encode_key(shifted(plain("\u{f72c}"))),
            key(FunctionKey::PageUp, Mods::NONE)
        );
        assert_eq!(
            encode_key(ctrl("\u{f72d}")),
            key(FunctionKey::PageDown, CTRL)
        );
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
        assert_eq!(
            encode_key(plain("\u{f728}")),
            key(FunctionKey::Delete, Mods::NONE)
        );
        // U+0019 is also Ctrl-Y's `characters` (`'y' & 0x1f`) — readline's yank. Only the
        // Control flag tells them apart, like the numpad Enter/Ctrl-C pair; if mixed up, Ctrl-Y
        // becomes back-tab.
        assert_eq!(encode(ctrl("\u{19}")), b"\x19");
        assert_eq!(encode(ctrl("y")), b"\x19");
        // Single-character match, the same criterion as PgUp.
        assert!(encode_key(plain("\u{f728}x")).is_none());
    }

    #[test]
    fn option_navigation_sends_meta_sequences() {
        // Option's navigation/deletion class produces no printable character on any layout, so
        // it is Meta-encoded without consulting a setting.
        // The sequences were measured in default zsh: `\eb` `backward-word`, `\ef`
        // `forward-word`, `\e\x7f` `backward-kill-word`. The letter is **lowercase** —
        // the uppercase form is bound to other widgets (`\eA` =
        // `accept-and-hold`), so `\eB` would not move by word.
        assert_eq!(encode(option("\u{f702}")), b"\x1bb", "Option+←");
        assert_eq!(encode(option("\u{f703}")), b"\x1bf", "Option+→");
        assert_eq!(encode(option("\u{7f}")), b"\x1b\x7f", "Option+Backspace");
        // ⌫'s mirror: `\ed` is `kill-word` in zsh, bash and fish.
        assert_eq!(
            encode(option("\u{f728}")),
            b"\x1bd",
            "Option+Forward Delete"
        );
        // Accompanying modifiers are not consulted: ⇧⌥← still moves by word.
        assert_eq!(encode(shifted(option("\u{f702}"))), b"\x1bb");
        // The form without Option stays untouched: an arrow is still an arrow (its bytes depend
        // on DECCKM), ⌫ is still a single byte.
        assert_eq!(
            encode_key(plain("\u{f702}")),
            key(FunctionKey::Arrow(Arrow::Left), Mods::NONE)
        );
        assert_eq!(encode(plain("\u{7f}")), b"\x7f");
        // Up/down arrows with Option are not this class: moving by word is a horizontal gesture,
        // and ⌥ rides along with ↑ in xterm's encoding (Codex's ⌥↑).
        assert_eq!(
            encode_key(option("\u{f700}")),
            key(FunctionKey::Arrow(Arrow::Up), OPTION)
        );
        // Single-character match, the same criterion as PgUp.
        assert!(encode_key(option("\u{f702}x")).is_none());
    }

    #[test]
    fn option_return_is_a_line_break() {
        // `\e\r`: Codex and Claude Code read it as a new line without sending; zsh and fish put
        // a line break in the line at their prompt.
        assert_eq!(encode(option("\r")), b"\x1b\r");
        assert_eq!(encode(shifted(option("\r"))), b"\x1b\r");
        // Not with Control, and plain Return still runs the line.
        assert_eq!(
            encode(KeyPress {
                option: true,
                ..ctrl("\r")
            }),
            b"\r"
        );
        assert_eq!(encode(plain("\r")), b"\r");
        assert_eq!(encode(shifted(plain("\r"))), b"\r");
    }

    #[test]
    fn option_printable_characters_are_untouched() {
        // On Turkish Q `{` = Option+7, `∫` = Option+b. If Option were Meta across the
        // board, the shell's metacharacters would become untypable — Meta is only for the navigation/deletion class.
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
        // user's criterion is that the line goes away.
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
        assert_eq!(
            encode_key(plain(&left)),
            key(FunctionKey::Arrow(Arrow::Left), Mods::NONE)
        );
        assert_eq!(
            encode_key(plain(&right)),
            key(FunctionKey::Arrow(Arrow::Right), Mods::NONE)
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
