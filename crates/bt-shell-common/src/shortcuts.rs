//! Keyboard shortcuts: which menu commands can have one, which they have, and whether a key
//! can be given.
//!
//! The rules live here, platform-free and tried without a window; the toolkit writes the menu
//! from them and draws the page that changes them.
//!
//! - [`COMMANDS`] — the table: every menu command with its fixed name (the key in `[keys]`),
//!   its menu id and the shortcuts it has by default; [`Family`] for the commands that are one
//!   row for several items (the digits of the tabs, the arrows of the splits).
//! - [`resolve`] — the settings read against the table: the shortcuts each command has, what is
//!   wrong with the file, the keys two commands share ([`Resolved`]).
//! - [`Resolved::judge`] — what recording a key comes to ([`Verdict`]): free, someone else's,
//!   the line's, the program's, one that types, macOS's, or one that is no shortcut.
//! - [`menu_key`] / [`chord_of`] / [`glyphs`] — the file's spelling ↔ the key a menu item holds,
//!   and how a shortcut is shown (the palette's own glyph function).
//!
//! The syntax of a shortcut (`shift+cmd+o`) is the settings model's (`bt_core::Shortcut`); the
//! keys that edit the line you type are `keys::LINE_EDITING`.

mod mapping;
mod resolve;
mod table;
mod verdict;

pub use mapping::{MenuKey, chord_of, glyphs, menu_key};
pub use resolve::{Assignment, Binding, Conflict, Resolved, Source, resolve};
pub use table::{COMMANDS, Command, Family, Item, by_id, by_name, family_default};
pub use verdict::{Clash, MACOS_KEYS, MacKey, Refusal, Ruling, Verdict, refusal};
