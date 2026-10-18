//! `[keys]`: the shortcuts of the menu's commands, by the command's fixed name.
//!
//! This file owns the **syntax** of a shortcut and the shape of the table; what a name
//! means, which shortcuts a command has by default and which ones may be given are the
//! shell layer's rules (`bt-shell-common`'s `shortcuts`) — the settings model does not know
//! the commands.
//!
//! A shortcut is written as the modifiers and the key joined by `+`, modifiers first:
//!
//! - `shift+cmd+o`, `ctrl+tab`, `cmd+left`, `f5` — a **chord**: [`Chord`];
//! - `option`, `option+cmd` — **only modifiers**: the shortcut of a family of commands whose
//!   keys (the digits of the tabs, the arrows of the splits) are fixed ([`Shortcut::Mods`]);
//! - `shift+shift` — one modifier **twice**: that key pressed twice ([`Shortcut::Twice`]).
//!
//! The modifiers are `ctrl`, `option`, `shift` and `cmd` (any order, one of each); a key is
//! one character (`d`, `]`, `-`, `ş`) or a name from [`NamedKey`]. The plus key is `plus`.
//! A shifted symbol is written as the key plus `shift`: `shift+cmd+]`, not `cmd+}`.
//!
//! A command's value is a string, `""` for no shortcut, or a list of at most two strings.
//! A line that cannot be read leaves a diagnostic and is dropped, alone: it does not take
//! the other lines with it.

use std::fmt;
use std::ops::Range;

use toml_edit::{DocumentMut, Item, TableLike, Value};

use super::{Diagnostic, document, kind, line_of, rendered, section};

/// One of the four modifier keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Modifier {
    Ctrl,
    Option,
    Shift,
    Cmd,
}

impl Modifier {
    /// In the order they are written and shown (⌃⌥⇧⌘).
    pub const ALL: [Modifier; 4] = [Self::Ctrl, Self::Option, Self::Shift, Self::Cmd];

    /// The file's spelling.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ctrl => "ctrl",
            Self::Option => "option",
            Self::Shift => "shift",
            Self::Cmd => "cmd",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|modifier| modifier.name() == name)
    }
}

/// A set of modifier keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub ctrl: bool,
    pub option: bool,
    pub shift: bool,
    pub cmd: bool,
}

impl Mods {
    pub const NONE: Mods = Mods::new(false, false, false, false);
    pub const CMD: Mods = Mods::new(false, false, false, true);

    pub const fn new(ctrl: bool, option: bool, shift: bool, cmd: bool) -> Self {
        Self {
            ctrl,
            option,
            shift,
            cmd,
        }
    }

    pub const fn has(self, modifier: Modifier) -> bool {
        match modifier {
            Modifier::Ctrl => self.ctrl,
            Modifier::Option => self.option,
            Modifier::Shift => self.shift,
            Modifier::Cmd => self.cmd,
        }
    }

    pub const fn with(mut self, modifier: Modifier) -> Self {
        match modifier {
            Modifier::Ctrl => self.ctrl = true,
            Modifier::Option => self.option = true,
            Modifier::Shift => self.shift = true,
            Modifier::Cmd => self.cmd = true,
        }
        self
    }

    pub const fn without(mut self, modifier: Modifier) -> Self {
        match modifier {
            Modifier::Ctrl => self.ctrl = false,
            Modifier::Option => self.option = false,
            Modifier::Shift => self.shift = false,
            Modifier::Cmd => self.cmd = false,
        }
        self
    }

    pub const fn is_empty(self) -> bool {
        !(self.ctrl || self.option || self.shift || self.cmd)
    }

    /// The members, in the order they are written.
    pub fn iter(self) -> impl Iterator<Item = Modifier> {
        Modifier::ALL
            .into_iter()
            .filter(move |modifier| self.has(*modifier))
    }
}

impl fmt::Display for Mods {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, modifier) in self.iter().enumerate() {
            if index > 0 {
                f.write_str("+")?;
            }
            f.write_str(modifier.name())?;
        }
        Ok(())
    }
}

/// The keys that are not one printable character.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NamedKey {
    Left,
    Right,
    Up,
    Down,
    Tab,
    Return,
    Space,
    Escape,
    /// ⌫.
    Backspace,
    /// ⌦ (fn-⌫).
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    /// F1–F12.
    F(u8),
}

impl NamedKey {
    /// The number of function keys.
    pub const FUNCTION_KEYS: u8 = 12;

    const FIXED: [(&'static str, NamedKey); 14] = [
        ("left", Self::Left),
        ("right", Self::Right),
        ("up", Self::Up),
        ("down", Self::Down),
        ("tab", Self::Tab),
        ("return", Self::Return),
        ("space", Self::Space),
        ("escape", Self::Escape),
        ("backspace", Self::Backspace),
        ("delete", Self::Delete),
        ("home", Self::Home),
        ("end", Self::End),
        ("pageup", Self::PageUp),
        ("pagedown", Self::PageDown),
    ];

    fn from_name(name: &str) -> Option<Self> {
        if let Some((_, key)) = Self::FIXED.iter().find(|(spelling, _)| *spelling == name) {
            return Some(*key);
        }
        let number: u8 = name.strip_prefix('f')?.parse().ok()?;
        // `f01` is not how anyone writes it, and `parse` accepts it.
        (name == format!("f{number}") && (1..=Self::FUNCTION_KEYS).contains(&number))
            .then_some(Self::F(number))
    }
}

impl fmt::Display for NamedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::F(number) = self {
            return write!(f, "f{number}");
        }
        match Self::FIXED.iter().find(|(_, key)| key == self) {
            Some((name, _)) => f.write_str(name),
            None => Ok(()),
        }
    }
}

/// A key: one character, or a [`NamedKey`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// The character the key types **without** Shift, lowercase: `d`, `]`, `1`, `ş`.
    Char(char),
    Named(NamedKey),
}

impl Key {
    /// The key a character is, in the form a chord holds it (lowercase) — `None` for a
    /// character that is no key by itself: a control character, a space, AppKit's private
    /// function-key range.
    pub fn of_char(c: char) -> Option<Key> {
        Self::char_of(c).ok().map(Key::Char)
    }

    /// The character a spelling of one character stands for, or why it is not a key.
    fn char_of(c: char) -> Result<char, ShortcutError> {
        // The function-key range is AppKit's private encoding of Home, F1 …: those keys have
        // names. A control character or a space is no key one writes by character.
        if c.is_control() || c.is_whitespace() || ('\u{f700}'..='\u{f8ff}').contains(&c) {
            return Err(ShortcutError::new(format!(
                "`{}` is not a key that can be written by itself (see the key names)",
                c.escape_default()
            )));
        }
        let mut lower = c.to_lowercase();
        Ok(match (lower.next(), lower.next()) {
            (Some(single), None) => single,
            _ => c,
        })
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // `+` joins the parts of a shortcut.
            Self::Char('+') => f.write_str("plus"),
            Self::Char(c) => write!(f, "{c}"),
            Self::Named(named) => fmt::Display::fmt(named, f),
        }
    }
}

/// Modifiers and one key: `shift+cmd+o`.
///
/// Built through [`Chord::new`] a chord is in **one** form whichever way the key was reached:
/// a shifted symbol is its base key with Shift (`}` is `shift+]` — the menu spells it `}`, the
/// event of the key reports it so) and the plus key has no Shift of its own (it is typed with
/// Shift on some layouts and without on others, and the same shortcut either way). A
/// struct literal skips this; the two forms would then be two chords.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub mods: Mods,
    pub key: Key,
}

impl Chord {
    /// The shifted symbols and the keys that type them with Shift (on the layouts that have
    /// them there): the three the menu spells as the symbol.
    const SHIFTED: [(char, char); 3] = [('{', '['), ('}', ']'), ('|', '\\')];

    pub const fn plain(mods: Mods, key: Key) -> Self {
        Self { mods, key }
    }

    pub fn new(mods: Mods, key: Key) -> Self {
        let Key::Char(c) = key else {
            return Self { mods, key };
        };
        if let Some((_, base)) = Self::SHIFTED.iter().find(|(symbol, _)| *symbol == c) {
            return Self {
                mods: mods.with(Modifier::Shift),
                key: Key::Char(*base),
            };
        }
        if c == '+' {
            return Self {
                mods: mods.without(Modifier::Shift),
                key,
            };
        }
        Self { mods, key }
    }

    /// The symbol the menu spells this chord's key as, when it is not the key itself: `]` with
    /// Shift is `}`. The Shift then stays out of the menu's modifiers.
    pub fn menu_symbol(&self) -> Option<(char, Mods)> {
        let Key::Char(c) = self.key else {
            return None;
        };
        if !self.mods.shift {
            return None;
        }
        Self::SHIFTED
            .iter()
            .find(|(_, base)| *base == c)
            .map(|(symbol, _)| (*symbol, self.mods.without(Modifier::Shift)))
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.mods.is_empty() {
            write!(f, "{}+", self.mods)?;
        }
        fmt::Display::fmt(&self.key, f)
    }
}

/// A shortcut as the file writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shortcut {
    /// Modifiers and a key.
    Chord(Chord),
    /// Only modifiers: a family's shortcut (the digits or the arrows are the family's).
    Mods(Mods),
    /// One modifier pressed twice.
    Twice(Modifier),
}

impl Shortcut {
    /// A shortcut's text → the shortcut. `""` is not one: that is the value that means *no
    /// shortcut*, and the caller reads it before it gets here.
    pub fn parse(text: &str) -> Result<Self, ShortcutError> {
        let lower = text.trim().to_lowercase();
        if lower.is_empty() {
            return Err(ShortcutError::new("a shortcut cannot be empty".to_owned()));
        }
        let parts: Vec<&str> = lower.split('+').map(str::trim).collect();
        if parts.iter().any(|part| part.is_empty()) {
            return Err(ShortcutError::new(
                "a `+` has nothing beside it (the plus key is written `plus`)".to_owned(),
            ));
        }
        let modifiers: Vec<Option<Modifier>> =
            parts.iter().map(|part| Modifier::from_name(part)).collect();
        if modifiers.iter().all(Option::is_some) {
            let modifiers: Vec<Modifier> = modifiers.into_iter().flatten().collect();
            if let [first, second] = modifiers[..]
                && first == second
            {
                return Ok(Self::Twice(first));
            }
            return Ok(Self::Mods(Self::mods_of(&modifiers)?));
        }
        let (key, held) = parts
            .split_last()
            .ok_or_else(|| ShortcutError::new("a shortcut cannot be empty".to_owned()))?;
        let mut mods = Mods::NONE;
        for part in held {
            let Some(modifier) = Modifier::from_name(part) else {
                return Err(ShortcutError::new(format!(
                    "`{part}` is not a modifier (ctrl, option, shift, cmd)"
                )));
            };
            if mods.has(modifier) {
                return Err(ShortcutError::new(format!(
                    "`{}` is written twice",
                    modifier.name()
                )));
            }
            mods = mods.with(modifier);
        }
        Ok(Self::Chord(Chord::new(mods, parse_key(key)?)))
    }

    fn mods_of(modifiers: &[Modifier]) -> Result<Mods, ShortcutError> {
        let mut mods = Mods::NONE;
        for modifier in modifiers {
            if mods.has(*modifier) {
                return Err(ShortcutError::new(format!(
                    "`{}` is written twice",
                    modifier.name()
                )));
            }
            mods = mods.with(*modifier);
        }
        Ok(mods)
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Chord(chord) => fmt::Display::fmt(chord, f),
            Self::Mods(mods) => fmt::Display::fmt(mods, f),
            Self::Twice(modifier) => write!(f, "{0}+{0}", modifier.name()),
        }
    }
}

fn parse_key(text: &str) -> Result<Key, ShortcutError> {
    let mut chars = text.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Key::char_of(c).map(Key::Char);
    }
    if text == "plus" {
        return Ok(Key::Char('+'));
    }
    NamedKey::from_name(text).map(Key::Named).ok_or_else(|| {
        ShortcutError::new(format!(
            "`{text}` is not a key (one character, or left, right, up, down, tab, return, \
             space, escape, backspace, delete, home, end, pageup, pagedown, f1 to f12, plus)"
        ))
    })
}

/// Why a shortcut's text could not be read. English: it ends up in the window's subtitle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutError {
    message: String,
}

impl ShortcutError {
    fn new(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for ShortcutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ShortcutError {}

/// One line of `[keys]`: a command's name and the shortcuts the file gives it.
///
/// An empty `shortcuts` is the file saying *none* (`""`, `[]`); a name that is missing from the
/// file has no entry at all, and that means the command's own default.
#[derive(Clone, Debug, Eq)]
pub struct KeyEntry {
    pub name: String,
    /// At most two, never the same one twice.
    pub shortcuts: Vec<Shortcut>,
    /// The 1-based line it was read from. A position, not a value: it is **left out of the
    /// comparison**, or moving a line of the file would read as a change of the shortcuts.
    pub line: Option<usize>,
}

impl PartialEq for KeyEntry {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.shortcuts == other.shortcuts
    }
}

/// How many shortcuts a command may have.
pub const MAX_SHORTCUTS: usize = 2;

/// Reads `[keys]`. A line that cannot be read leaves a diagnostic (its `key` is `keys`, the
/// line's name is in the message) and gives no entry — the command stays as the shell layer
/// has it by default, and the other lines are read as usual.
pub(super) fn read(
    text: &str,
    keys: &dyn TableLike,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<KeyEntry> {
    let mut entries = Vec::new();
    for (name, item) in keys.iter() {
        let line = item.span().and_then(|span| line_of(text, span.start));
        match shortcuts_of(item) {
            Ok(shortcuts) => entries.push(KeyEntry {
                name: name.to_owned(),
                shortcuts,
                line,
            }),
            Err(reason) => diagnostics.push(Diagnostic {
                key: Some("keys"),
                line,
                message: format!("`keys.{name}`: {reason}; the line is ignored"),
            }),
        }
    }
    entries
}

fn shortcuts_of(item: &Item) -> Result<Vec<Shortcut>, String> {
    let wrong = || {
        format!(
            "must be a string or a list of up to {MAX_SHORTCUTS} strings, found {}",
            kind(item)
        )
    };
    let Some(value) = item.as_value() else {
        return Err(wrong());
    };
    let texts: Vec<&str> = match value {
        // `""`: no shortcut.
        Value::String(text) if text.value().trim().is_empty() => return Ok(Vec::new()),
        Value::String(text) => vec![text.value().as_str()],
        Value::Array(array) => {
            if array.len() > MAX_SHORTCUTS {
                return Err(format!(
                    "a command has at most {MAX_SHORTCUTS} shortcuts, found {}",
                    array.len()
                ));
            }
            let mut texts = Vec::new();
            for element in array {
                let Some(text) = element.as_str() else {
                    return Err(wrong());
                };
                if text.trim().is_empty() {
                    return Err("a list holds shortcuts, not empty strings".to_owned());
                }
                texts.push(text);
            }
            texts
        }
        _ => return Err(wrong()),
    };
    let mut shortcuts: Vec<Shortcut> = Vec::new();
    for text in texts {
        let shortcut = Shortcut::parse(text).map_err(|error| format!("`{text}`: {error}"))?;
        if !shortcuts.contains(&shortcut) {
            shortcuts.push(shortcut);
        }
    }
    Ok(shortcuts)
}

/// The value that stands for `shortcuts` in the file: `""`, a string, or a list of two.
pub(super) fn value_of(shortcuts: Option<&[Shortcut]>) -> Value {
    match shortcuts {
        // `None` removes the line and writes no value; the empty string is what the type
        // wants of this arm.
        None | Some([]) => String::new().into(),
        Some([only]) => only.to_string().into(),
        Some(many) => many
            .iter()
            .map(ToString::to_string)
            .collect::<toml_edit::Array>()
            .into(),
    }
}

/// The `[keys]` table of an editable document, if there is one that is a section.
fn table_of(doc: &mut DocumentMut) -> Option<&mut dyn TableLike> {
    doc.get_mut("keys").and_then(Item::as_table_like_mut)
}

/// A `[keys]` that is not a section, or a name that is one: writing in its place would erase
/// what is there, so the write is refused (the rule of every other key).
fn refuse_unwritable(
    text: &str,
    parsed: &toml_edit::Document<&str>,
    names: &[&str],
) -> Option<Diagnostic> {
    let mut refused = Vec::new();
    if let Some(keys) = section(text, parsed.as_table(), "keys", &mut refused) {
        for name in names {
            if let Some(item) = keys
                .get(name)
                .filter(|item| !item.is_value() || item.is_inline_table())
            {
                refused.push(Diagnostic {
                    key: Some("keys"),
                    line: item.span().and_then(|span| line_of(text, span.start)),
                    message: format!(
                        "`keys.{name}` must be a string or a list of strings, found {}",
                        kind(item)
                    ),
                });
            }
        }
    }
    refused.pop()
}

/// The bytes of the line `name = …` takes in a `[keys]` written as a header: from the start of
/// its line to the end of the line the value ends on, the comment after the value and the line
/// break included. A comment on the lines above is **not** part of it — it may be about
/// something else, or the explanation of the whole table.
///
/// `None` when the entry is not alone on its line (a dotted key, an inline table): those go
/// through `toml_edit`.
fn line_of_entry(
    text: &str,
    parsed: &toml_edit::Document<&str>,
    name: &str,
) -> Option<Range<usize>> {
    let Item::Table(table) = parsed.as_table().get("keys")? else {
        return None;
    };
    let (key, item) = table.get_key_value(name)?;
    let start = key.span()?.start;
    let end = item.span()?.end;
    let line_start = text[..start].rfind('\n').map_or(0, |at| at + 1);
    if !text[line_start..start]
        .chars()
        .all(|c| c == ' ' || c == '\t')
    {
        return None;
    }
    let line_end = text[end..].find('\n').map_or(text.len(), |at| end + at + 1);
    Some(line_start..line_end)
}

/// `text` without the bytes of `ranges`.
fn without_ranges(text: &str, mut ranges: Vec<Range<usize>>) -> String {
    ranges.sort_by_key(|range| std::cmp::Reverse(range.start));
    let mut out = text.to_owned();
    for range in ranges {
        out.replace_range(range, "");
    }
    out
}

/// Removes the entries `names` from the `[keys]` of an editable document.
fn remove_entries(doc: &mut DocumentMut, names: &[&str]) {
    match doc.get_mut("keys") {
        Some(Item::Value(Value::InlineTable(table))) => {
            for name in names {
                super::inline_remove(table, name);
            }
        }
        _ => {
            if let Some(keys) = table_of(doc) {
                for name in names {
                    keys.remove(name);
                }
            }
        }
    }
}

/// The text without the `[keys]` lines of `names`. The lines that are alone on theirs are cut
/// out of the text itself, so what is around them is untouched to the byte; `toml_edit` takes
/// out the rest.
fn removed(text: &str, parsed: toml_edit::Document<&str>, names: &[&str]) -> String {
    let ranges: Option<Vec<Range<usize>>> = names
        .iter()
        .filter(|name| {
            parsed
                .as_table()
                .get("keys")
                .and_then(Item::as_table_like)
                .is_some_and(|keys| keys.contains_key(name))
        })
        .map(|name| line_of_entry(text, &parsed, name))
        .collect();
    if let Some(ranges) = ranges {
        return without_ranges(text, ranges);
    }
    let mut doc = parsed.into_mut();
    remove_entries(&mut doc, names);
    rendered(text, &doc)
}

/// Settings ▸ Keyboard's write: `shortcuts` becomes `name`'s value (`""` for an empty slice);
/// `None` removes the line, so the command is back to its default. Everything else in the file
/// stays in place.
pub(super) fn with_shortcut(
    text: &str,
    name: &str,
    shortcuts: Option<&[Shortcut]>,
) -> Result<String, Diagnostic> {
    let parsed = document(text)?;
    if let Some(refused) = refuse_unwritable(text, &parsed, &[name]) {
        return Err(refused);
    }
    let Some(shortcuts) = shortcuts else {
        return Ok(removed(text, parsed, &[name]));
    };
    let mut doc = parsed.into_mut();
    super::ensure_section(&mut doc, "keys");
    let value = value_of(Some(shortcuts));
    // `ensure_section` added or found a section, and `refuse_unwritable` has seen that it is
    // one.
    match doc.get_mut("keys") {
        Some(Item::Value(Value::InlineTable(table))) if !table.contains_key(name) => {
            super::inline_insert(table, name, value);
        }
        Some(keys) => {
            if let Some(keys) = keys.as_table_like_mut() {
                match keys.get_mut(name).and_then(Item::as_value_mut) {
                    // The decor (the space after `=`, the comment at the end of the line) sits
                    // on the value, and the new one inherits it.
                    Some(old) => {
                        let decor = old.decor().clone();
                        *old = value;
                        *old.decor_mut() = decor;
                    }
                    None => {
                        keys.insert(name, Item::Value(value));
                    }
                }
            }
        }
        None => {}
    }
    Ok(rendered(text, &doc))
}

/// Restore Defaults…: removes the lines of `names` from `[keys]`. A name that is not there, a
/// comment and a line that is none of `names` stay as they are.
pub(super) fn without_shortcuts(text: &str, names: &[String]) -> Result<String, Diagnostic> {
    let parsed = document(text)?;
    if let Some(refused) = refuse_unwritable(text, &parsed, &[]) {
        return Err(refused);
    }
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    Ok(removed(text, parsed, &names))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(mods: &str, key: Key) -> Shortcut {
        let mut set = Mods::NONE;
        for modifier in mods.split('+').filter(|part| !part.is_empty()) {
            set = set.with(Modifier::from_name(modifier).expect("a modifier"));
        }
        Shortcut::Chord(Chord::new(set, key))
    }

    fn parse(text: &str) -> Shortcut {
        Shortcut::parse(text).unwrap_or_else(|error| panic!("`{text}`: {error}"))
    }

    #[test]
    fn a_chord_is_modifiers_then_a_key() {
        assert_eq!(parse("shift+cmd+o"), chord("shift+cmd", Key::Char('o')));
        assert_eq!(parse("ctrl+tab"), chord("ctrl", Key::Named(NamedKey::Tab)));
        assert_eq!(parse("f5"), chord("", Key::Named(NamedKey::F(5))));
        assert_eq!(parse("cmd+,"), chord("cmd", Key::Char(',')));
        assert_eq!(parse("cmd+-"), chord("cmd", Key::Char('-')));
        assert_eq!(parse("cmd+plus"), chord("cmd", Key::Char('+')));
        assert_eq!(
            parse("cmd+pagedown"),
            chord("cmd", Key::Named(NamedKey::PageDown))
        );
        // Any order, any case, spaces around the parts.
        assert_eq!(parse("Cmd + Shift + O"), parse("shift+cmd+o"));
        // A key that is a letter of a layout other than the US one.
        assert_eq!(parse("cmd+ş"), chord("cmd", Key::Char('ş')));
    }

    #[test]
    fn a_shortcut_is_written_in_one_canonical_form() {
        for text in [
            "shift+cmd+o",
            "option+shift+cmd+[",
            "ctrl+cmd+=",
            "cmd+plus",
            "cmd+-",
            "shift+cmd+\\",
            "ctrl+shift+tab",
            "shift+cmd+return",
            "f12",
            "option",
            "option+cmd",
            "ctrl+option+shift+cmd",
            "shift+shift",
            "ctrl+ctrl",
        ] {
            assert_eq!(parse(text).to_string(), text);
        }
        // The order of the parts is the glyphs' (⌃⌥⇧⌘), whatever was typed.
        assert_eq!(
            parse("cmd+shift+option+ctrl+x").to_string(),
            "ctrl+option+shift+cmd+x"
        );
    }

    #[test]
    fn a_shifted_symbol_is_its_base_key_with_shift() {
        assert_eq!(parse("cmd+}"), parse("shift+cmd+]"));
        assert_eq!(parse("cmd+{"), parse("shift+cmd+["));
        assert_eq!(parse("cmd+|"), parse("shift+cmd+\\"));
        assert_eq!(parse("cmd+}").to_string(), "shift+cmd+]");
        // The plus key has no Shift of its own.
        assert_eq!(parse("shift+cmd+plus"), parse("cmd+plus"));
        let bracket = Chord::new(Mods::CMD.with(Modifier::Shift), Key::Char(']'));
        assert_eq!(bracket.menu_symbol(), Some(('}', Mods::CMD)));
        assert_eq!(Chord::new(Mods::CMD, Key::Char(']')).menu_symbol(), None);
    }

    #[test]
    fn only_modifiers_is_a_family_and_one_twice_is_a_double_press() {
        assert_eq!(
            parse("option"),
            Shortcut::Mods(Mods::NONE.with(Modifier::Option))
        );
        assert_eq!(
            parse("cmd+option"),
            Shortcut::Mods(Mods::CMD.with(Modifier::Option))
        );
        assert_eq!(parse("shift+shift"), Shortcut::Twice(Modifier::Shift));
        assert_eq!(parse("CMD+cmd"), Shortcut::Twice(Modifier::Cmd));
    }

    #[test]
    fn what_cannot_be_read_says_why() {
        for (text, needle) in [
            ("", "empty"),
            ("cmd+", "`+` has nothing"),
            ("+d", "`+` has nothing"),
            ("cmd++", "`+` has nothing"),
            ("cmd+shift+cmd+d", "`cmd` is written twice"),
            ("shift+shift+cmd", "`shift` is written twice"),
            ("shift+shift+shift", "`shift` is written twice"),
            ("meta+d", "`meta` is not a modifier"),
            ("cmd+enter", "`enter` is not a key"),
            ("cmd+f13", "`f13` is not a key"),
            ("cmd+f01", "`f01` is not a key"),
            ("cmd+\u{f729}", "not a key that can be written"),
            ("cmd+ ", "`+` has nothing"),
        ] {
            let error = Shortcut::parse(text).expect_err(text);
            assert!(error.to_string().contains(needle), "{text}: {error}");
        }
    }

    fn entries(text: &str) -> (Vec<KeyEntry>, Vec<Diagnostic>) {
        let doc = document(text).expect("TOML");
        let mut diagnostics = Vec::new();
        let keys = section(text, doc.as_table(), "keys", &mut diagnostics);
        let entries = keys.map_or_else(Vec::new, |keys| read(text, keys, &mut diagnostics));
        (entries, diagnostics)
    }

    #[test]
    fn the_table_is_read_with_the_line_of_each_entry() {
        let text = "[keys]\n# a comment\ncopy_last_output = \"shift+cmd+o\"\n\
                    show_all_tabs = \"\"\nshow_next_tab = [\"shift+cmd+]\", \"ctrl+tab\"]\n\
                    select_tab = \"option\"\nsplit_right = []\n";
        let (entries, diagnostics) = entries(text);
        assert_eq!(diagnostics, Vec::new());
        let shown: Vec<(&str, Vec<String>, Option<usize>)> = entries
            .iter()
            .map(|entry| {
                (
                    entry.name.as_str(),
                    entry.shortcuts.iter().map(ToString::to_string).collect(),
                    entry.line,
                )
            })
            .collect();
        assert_eq!(
            shown,
            vec![
                ("copy_last_output", vec!["shift+cmd+o".to_owned()], Some(3)),
                ("show_all_tabs", vec![], Some(4)),
                (
                    "show_next_tab",
                    vec!["shift+cmd+]".to_owned(), "ctrl+tab".to_owned()],
                    Some(5)
                ),
                ("select_tab", vec!["option".to_owned()], Some(6)),
                ("split_right", vec![], Some(7)),
            ]
        );
    }

    #[test]
    fn a_line_that_cannot_be_read_spoils_only_itself() {
        let text = "[keys]\ncopy_last_output = \"shift+cmd+\"\nsplit_right = \"cmd+d\"\n\
                    a = 5\nb = [\"cmd+a\", \"cmd+b\", \"cmd+c\"]\nc = [1]\nd = [\"\"]\n\
                    e = { x = 1 }\n[keys.f]\nx = 1\n";
        // A duplicate of `[keys.f]` is a table inside the table, not a duplicate key.
        let (entries, diagnostics) = entries(text);
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["split_right"]
        );
        let lines: Vec<Option<usize>> = diagnostics.iter().map(|d| d.line).collect();
        assert_eq!(
            lines,
            [
                Some(2),
                Some(4),
                Some(5),
                Some(6),
                Some(7),
                Some(8),
                Some(9)
            ]
        );
        assert!(diagnostics.iter().all(|d| d.key == Some("keys")));
        assert!(
            diagnostics[0].message.contains("`keys.copy_last_output`"),
            "{}",
            diagnostics[0]
        );
        assert!(
            diagnostics[0].message.contains("ignored"),
            "{}",
            diagnostics[0]
        );
        assert!(
            diagnostics[2].message.contains("at most 2"),
            "{}",
            diagnostics[2]
        );
    }

    #[test]
    fn the_same_shortcut_twice_in_a_list_is_one() {
        let (entries, diagnostics) = entries("[keys]\nx = [\"cmd+d\", \"CMD+D\"]\n");
        assert_eq!(diagnostics, Vec::new());
        assert_eq!(entries[0].shortcuts.len(), 1);
    }

    #[test]
    fn a_moved_line_is_not_a_changed_shortcut() {
        let (before, _) = entries("[keys]\nx = \"cmd+d\"\n");
        let (after, _) = entries("[keys]\n\n\nx = \"cmd+d\"\n");
        assert_ne!(before[0].line, after[0].line);
        assert_eq!(before, after);
    }

    #[test]
    fn a_value_is_written_the_way_it_is_read() {
        let none: &[Shortcut] = &[];
        let one = [parse("shift+cmd+o")];
        let two = [parse("shift+cmd+]"), parse("ctrl+tab")];
        for (shortcuts, written) in [
            (none, "x = \"\"\n"),
            (&one[..], "x = \"shift+cmd+o\"\n"),
            (&two[..], "x = [\"shift+cmd+]\", \"ctrl+tab\"]\n"),
        ] {
            let text = with_shortcut("", "x", Some(shortcuts)).expect("writable");
            assert_eq!(text, format!("[keys]\n{written}"));
            let (read_back, diagnostics) = entries(&text);
            assert_eq!(diagnostics, Vec::new());
            assert_eq!(read_back[0].shortcuts, shortcuts);
        }
    }

    const RICH: &str = "# my settings\n[terminal]\nscrollback = 500 # lots\n\n\
                        [keys]\n# the lines below are mine\n\
                        copy_last_output = \"shift+cmd+o\" # output\n\
                        split_right = \"cmd+d\"\nfuture_command = \"cmd+9\"\n\n\
                        [notes]\nx = 1\n";

    #[test]
    fn writing_a_shortcut_keeps_comments_and_unknown_keys() {
        let written = with_shortcut(RICH, "copy_last_output", Some(&[parse("shift+cmd+k")]))
            .expect("writable");
        assert_eq!(
            written,
            RICH.replace("\"shift+cmd+o\" # output", "\"shift+cmd+k\" # output")
        );
        let added =
            with_shortcut(RICH, "rename_tab", Some(&[parse("shift+cmd+r")])).expect("writable");
        assert!(added.contains("rename_tab = \"shift+cmd+r\""), "{added}");
        for kept in [
            "# my settings",
            "# the lines below are mine",
            "future_command = \"cmd+9\"",
            "scrollback = 500 # lots",
            "[notes]",
        ] {
            assert!(added.contains(kept), "{kept}: {added}");
        }
    }

    #[test]
    fn a_reset_removes_the_line_and_only_the_line() {
        let written = with_shortcut(RICH, "split_right", None).expect("writable");
        assert_eq!(written, RICH.replace("split_right = \"cmd+d\"\n", ""));
        // Nothing to remove: the text comes back untouched, in a file with no `[keys]` too.
        assert_eq!(
            with_shortcut(RICH, "rename_tab", None).expect("writable"),
            RICH
        );
        assert_eq!(
            with_shortcut("[terminal]\n", "rename_tab", None).expect("writable"),
            "[terminal]\n"
        );
    }

    #[test]
    fn restore_defaults_keeps_comments_and_unknown_keys() {
        let names = vec![
            "copy_last_output".to_owned(),
            "split_right".to_owned(),
            "rename_tab".to_owned(),
        ];
        let written = without_shortcuts(RICH, &names).expect("writable");
        assert_eq!(
            written,
            "# my settings\n[terminal]\nscrollback = 500 # lots\n\n[keys]\n\
             # the lines below are mine\nfuture_command = \"cmd+9\"\n\n[notes]\nx = 1\n"
        );
        assert_eq!(without_shortcuts(RICH, &[]).expect("writable"), RICH);
        assert_eq!(without_shortcuts("", &names).expect("writable"), "");
    }

    #[test]
    fn a_keys_that_is_not_a_section_is_not_written_over() {
        for text in ["keys = 5\n", "[[keys]]\nx = 1\n"] {
            let error = with_shortcut(text, "x", Some(&[parse("cmd+d")])).expect_err(text);
            assert_eq!(error.key, Some("keys"), "{text}");
            assert!(error.message.contains("must be a section"), "{error}");
        }
        let error = with_shortcut("[keys.x]\ny = 1\n", "x", Some(&[parse("cmd+d")]))
            .expect_err("a section where the value goes");
        assert!(error.message.contains("found a section"), "{error}");
        // Text that cannot be parsed is never written over.
        assert!(with_shortcut("[keys\n", "x", None).is_err());
    }

    #[test]
    fn the_inline_spelling_is_kept() {
        let written = with_shortcut("keys = { x = \"cmd+d\" }\n", "y", Some(&[parse("cmd+e")]))
            .expect("writable");
        assert_eq!(written, "keys = { x = \"cmd+d\", y = \"cmd+e\" }\n");
    }
}
