//! The welcome window's model: its pages, how the keys move through them, what each page shows
//! and writes, and when the window opens by itself. The window — AppKit, the theme's colours,
//! the transitions — is `bt-shell-macos`'s `welcome`; everything it decides is here, platformless
//! and tested.
//!
//! **Four pages** ([`Page`]): who bateri is; the keys worth knowing; a quick setup (where the
//! tabs sit, how a split looks, the theme); where to go next. Back, Next and, on the last page,
//! "Let's go" walk them; ← and → do too, ⏎ is the page's main button, esc closes ([`Tour::key`]).
//!
//! **No second list of anything.** The shortcut cards are entries of [`help::SHORTCUTS`] picked
//! by their action ([`KEY_CARDS`]) — the table a test holds to the menu, so a card's keys are the
//! menu's keys. The setup's choices are the settings model's values ([`TabPosition::NAMES`],
//! [`SplitStyle::NAMES`]; a test fails when a value has no card) and each choice is a
//! [`SettingsEdit`] the window hands to the application's one writer, the settings window's.
//!
//! **Once, by itself.** The application opens the window at its first launch on a Mac, after
//! its first window is up: [`first_launch`] creates the marker [`MARKER`] with `O_EXCL`, so the
//! creation is the decision and two processes can't both decide to greet. After that it opens
//! only when asked — Help ▸ Welcome to bateri, the palette, `bateri help --welcome`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bt_core::{Settings, SettingsEdit, SplitStyle, TabPosition};

use crate::help::{self, Shortcut};

/// The marker, relative to the home directory: present once the welcome has opened by itself
/// on this Mac. Beside bateri's other state (`remote-hosts`, the saved session).
pub const MARKER: &str = "Library/Application Support/bateri/welcomed";

/// The marker's path under `home`.
pub fn marker_path(home: &Path) -> PathBuf {
    home.join(MARKER)
}

/// Whether this launch is the first on this Mac, deciding it by creating the marker: `true`
/// only for the call that created it. `create_new` (`O_EXCL`) is the atomic step — the file is
/// empty, there is nothing to write half of. A marker that can't be made (no home, a read-only
/// disk) is `false`: a welcome that would open on every launch is worse than none.
pub fn first_launch(home: &Path) -> bool {
    let path = marker_path(home);
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return false;
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .is_ok()
}

/// The address that asks a running bateri to show the welcome (`bateri help --welcome` opens it
/// with `open`; the application's `application:openURLs:` shows the window). The scheme is the
/// one bateri already registers for `bateri://tab/…`.
pub const URL: &str = "bateri://welcome";

/// The Help menu item's action: Help ▸ Welcome to bateri, and so the palette's row.
pub const ACTION: &str = "showWelcome:";

/// The window's title — also what VoiceOver says for it and the Help menu's item.
pub const TITLE: &str = "Welcome to bateri";

/// The one-line pitch under the name.
pub const PITCH: &str = "A fast, GPU-rendered terminal for macOS";

/// The line under the pitch.
pub const TAGLINE: &str =
    "Calm by design: command blocks, a dock for the line you type, tabs and splits.";

/// A page of the welcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// The icon, the name, the pitch.
    Hello,
    /// The shortcut cards.
    Keys,
    /// Tabs, splits, theme.
    Setup,
    /// Where to go next.
    Next,
}

impl Page {
    /// The pages in order.
    pub const ALL: [Page; 4] = [Page::Hello, Page::Keys, Page::Setup, Page::Next];

    /// The page's heading (the hello page's is the name, drawn larger).
    pub fn title(self) -> &'static str {
        match self {
            Page::Hello => "bateri",
            Page::Keys => "The keys you'll reach for",
            Page::Setup => "Make it yours",
            Page::Next => "Where to go next",
        }
    }

    /// The line under the heading.
    pub fn subtitle(self) -> &'static str {
        match self {
            Page::Hello => PITCH,
            Page::Keys => "Every one of them is in the menus, and in the command palette.",
            Page::Setup => "It applies as you choose. Change it any time in Settings.",
            Page::Next => "Everything here is one keystroke or one command away.",
        }
    }
}

/// A key the window takes — by its own key handling, so the window works without the mouse
/// whether or not macOS's Full Keyboard Access is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Return,
    Escape,
    /// ⇥: the next page.
    Tab,
    /// ⇧⇥: the page before.
    BackTab,
    Space,
    /// ⌘1–⌘4: that page (0-based).
    Page(usize),
    /// ⌘[ and ⌘←: the page before, on every page — they never change a value.
    PreviousPage,
    /// ⌘] and ⌘→: the next page (holding on the last; it never closes).
    NextPage,
    /// ⌫: the page before — unless a theme type-select is being typed, which it ends.
    Delete,
    /// A letter or digit typed without ⌘: the theme row's type-select.
    Char(char),
}

/// A row of the setup page, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Tabs,
    Splits,
    Theme,
}

impl Row {
    pub const ALL: [Row; 3] = [Row::Tabs, Row::Splits, Row::Theme];

    /// The row's name, as its label shows it and VoiceOver says it.
    pub fn label(self) -> &'static str {
        match self {
            Row::Tabs => "Tabs",
            Row::Splits => "Splits",
            Row::Theme => "Theme",
        }
    }
}

/// What a key or a button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    /// Show the page at this index, coming from the one before it.
    To(usize),
    /// The last page's "Let's go", or esc: the window closes.
    Close,
    /// Nothing to do (← on the first page, → on the last).
    Stay,
    /// The setup page: this row takes the focus.
    Focus(Row),
    /// The setup page: the focused row's value steps by this much (← −1, → +1); `wrap` goes
    /// round at the ends (␣ on a card row), otherwise the ends hold.
    Change { row: Row, delta: i32, wrap: bool },
    /// The theme row: a typed character, for the type-select ([`TypeSelect`]).
    Type(char),
    /// The theme row's ␣: the popup opens, for the whole list.
    OpenList,
}

/// Where the reader is: the page, and on the setup page the focused row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tour {
    at: usize,
    row: usize,
}

impl Tour {
    /// The page shown.
    pub fn page(self) -> Page {
        Page::ALL[self.at]
    }

    /// Its index.
    pub fn index(self) -> usize {
        self.at
    }

    /// The setup page's focused row (the first one on arrival).
    pub fn row(self) -> Row {
        Row::ALL[self.row]
    }

    pub fn is_first(self) -> bool {
        self.at == 0
    }

    pub fn is_last(self) -> bool {
        self.at + 1 == Page::ALL.len()
    }

    /// The main button's title: "Next", then "Let's go" on the last page.
    pub fn primary(self) -> &'static str {
        if self.is_last() { "Let's go" } else { "Next" }
    }

    /// Next / Let's go.
    pub fn forward(self) -> Move {
        if self.is_last() {
            Move::Close
        } else {
            Move::To(self.at + 1)
        }
    }

    /// Back.
    pub fn back(self) -> Move {
        if self.is_first() {
            Move::Stay
        } else {
            Move::To(self.at - 1)
        }
    }

    /// A page dot, ⌘1–⌘4: any page.
    pub fn go(self, index: usize) -> Move {
        if index < Page::ALL.len() && index != self.at {
            Move::To(index)
        } else {
            Move::Stay
        }
    }

    /// What a key does. On every page ⏎ is the main button, esc closes, ⇥ / ⇧⇥ step a page
    /// (stopping at the ends) and ⌘1–⌘4 jump. On the setup page ↑ ↓ move between its rows and
    /// ← → change the focused row's value; ␣ steps a card row round, or opens the theme list;
    /// letters type-select a theme. Elsewhere ← → step a page.
    pub fn key(self, key: Key) -> Move {
        self.key_while(key, false)
    }

    /// [`Self::key`], knowing whether a theme type-select is in progress (`typing`): then ⌫
    /// ends it ([`Move::Stay`], the caller clears it) instead of going back a page.
    pub fn key_while(self, key: Key, typing: bool) -> Move {
        let setup = self.page() == Page::Setup;
        let row = self.row();
        match key {
            Key::Return => self.forward(),
            Key::Escape => Move::Close,
            Key::Tab if self.is_last() => Move::Stay,
            Key::Tab => self.forward(),
            Key::BackTab => self.back(),
            Key::Page(index) => self.go(index),
            Key::PreviousPage => self.back(),
            Key::NextPage if self.is_last() => Move::Stay,
            Key::NextPage => self.forward(),
            Key::Delete if typing && setup && self.row() == Row::Theme => Move::Stay,
            Key::Delete => self.back(),
            Key::Up if setup && self.row > 0 => Move::Focus(Row::ALL[self.row - 1]),
            Key::Down if setup && self.row + 1 < Row::ALL.len() => {
                Move::Focus(Row::ALL[self.row + 1])
            }
            Key::Left if setup => Move::Change {
                row,
                delta: -1,
                wrap: row == Row::Theme,
            },
            Key::Right if setup => Move::Change {
                row,
                delta: 1,
                wrap: row == Row::Theme,
            },
            Key::Space if setup && row == Row::Theme => Move::OpenList,
            Key::Space if setup => Move::Change {
                row,
                delta: 1,
                wrap: true,
            },
            Key::Char(c) if setup && row == Row::Theme && c.is_alphanumeric() => Move::Type(c),
            Key::Left => self.back(),
            Key::Right if self.is_last() => Move::Stay,
            Key::Right => self.forward(),
            _ => Move::Stay,
        }
    }

    /// Moves to what `step` says; `Some(page)` when the page changed. Arriving at the setup
    /// page focuses its first row, so the highlight is there from the start.
    pub fn apply(&mut self, step: Move) -> Option<usize> {
        match step {
            Move::To(index) if index < Page::ALL.len() => {
                self.at = index;
                self.row = 0;
                Some(index)
            }
            Move::Focus(row) => {
                self.row = Row::ALL.iter().position(|r| *r == row).unwrap_or(0);
                None
            }
            _ => None,
        }
    }

    /// What VoiceOver says for the page dots.
    pub fn position_label(self) -> String {
        format!(
            "Page {} of {}: {}",
            self.at + 1,
            Page::ALL.len(),
            self.page().title()
        )
    }

    /// The page's keys, for the hint line under it.
    pub fn hint(self) -> &'static str {
        match self.page() {
            Page::Setup => "↑↓ choose  ·  ←→ change  ·  ⌘← back  ·  ⏎ next",
            Page::Next => "← back  ·  ⏎ let's go  ·  esc close",
            _ => "←→ or ⇥ move  ·  ⏎ next  ·  esc close",
        }
    }
}

/// The value a card row steps to: from the selected choice by `delta`, holding at the ends or
/// going round (`wrap`). `None` when it stays — a key held at an end writes nothing.
pub fn step_choice(choices: &[Choice], delta: i32, wrap: bool) -> Option<&Choice> {
    let len = choices.len() as i32;
    if len == 0 {
        return None;
    }
    let at = choices.iter().position(|c| c.selected).unwrap_or(0) as i32;
    let to = if wrap {
        (at + delta).rem_euclid(len)
    } else {
        (at + delta).clamp(0, len - 1)
    };
    (to != at).then(|| &choices[to as usize])
}

/// The theme a step of `delta` reaches from `current` in the popup's `names` (`None` for a
/// separator, skipped), going round at the ends. `None` when there is nowhere else to go.
pub fn step_theme(names: &[Option<String>], current: &str, delta: i32) -> Option<String> {
    let themes: Vec<&String> = names.iter().flatten().collect();
    let len = themes.len() as i32;
    if len < 2 {
        return None;
    }
    let at = themes.iter().position(|name| *name == current).unwrap_or(0) as i32;
    Some(themes[(at + delta).rem_euclid(len) as usize].clone())
}

/// How long typed letters keep adding to one type-select, as in a Finder list.
pub const TYPE_GAP: Duration = Duration::from_millis(900);

/// The theme row's type-select: letters typed close together spell the start of a theme's
/// title (or name); a pause starts over.
#[derive(Clone, Debug, Default)]
pub struct TypeSelect {
    typed: String,
    last: Option<Instant>,
}

impl TypeSelect {
    /// Whether letters typed at `now` would add to the current search.
    pub fn is_active(&self, now: Instant) -> bool {
        !self.typed.is_empty()
            && self
                .last
                .is_some_and(|last| now.duration_since(last) <= TYPE_GAP)
    }

    /// Ends the current search.
    pub fn clear(&mut self) {
        self.typed.clear();
        self.last = None;
    }

    /// Adds `c` typed at `now` and returns the name of the first theme whose title or name
    /// starts with what is typed. `themes` are `(name, title)` in the popup's order.
    pub fn push(&mut self, c: char, now: Instant, themes: &[(String, String)]) -> Option<String> {
        if self
            .last
            .is_none_or(|last| now.duration_since(last) > TYPE_GAP)
        {
            self.typed.clear();
        }
        self.last = Some(now);
        self.typed.extend(c.to_lowercase());
        let typed = self.typed.as_str();
        themes
            .iter()
            .find(|(name, title)| {
                title.to_lowercase().starts_with(typed) || name.to_lowercase().starts_with(typed)
            })
            .map(|(name, _)| name.clone())
    }
}

/// How long the theme row waits after the last change before it writes: a key held down, or
/// letters typed, preview every theme they pass but write only where they stop.
pub const WRITE_QUIET: Duration = Duration::from_millis(600);

/// A write held back until the changes stop ([`WRITE_QUIET`]): `note` each change, write what
/// `due` gives when the quiet has passed, and `flush` when the row, the page or the window is
/// left — so the file sees one write for a run of changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Debounce<T> {
    pending: Option<(T, Instant)>,
}

impl<T> Default for Debounce<T> {
    fn default() -> Self {
        Self { pending: None }
    }
}

impl<T> Debounce<T> {
    /// A change at `now`: it replaces any pending one and restarts the wait.
    pub fn note(&mut self, value: T, now: Instant) {
        self.pending = Some((value, now + WRITE_QUIET));
    }

    /// The pending value, if the quiet has passed by `now`.
    pub fn due(&mut self, now: Instant) -> Option<T> {
        match &self.pending {
            Some((_, at)) if now >= *at => self.pending.take().map(|(value, _)| value),
            _ => None,
        }
    }

    /// The pending value now, whatever the time.
    pub fn flush(&mut self) -> Option<T> {
        self.pending.take().map(|(value, _)| value)
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
}

/// What VoiceOver announces when a setup row takes the focus or its value changes.
pub fn announce(row: Row, value: &str) -> String {
    format!("{}: {value}", row.label())
}

/// The actions of the shortcut cards, in reading order (three to a row).
pub const KEY_CARDS: [&str; 6] = [
    "newTab:",
    "splitRight:",
    "toggleCommandPalette:",
    "goToTab:",
    "splitDown:",
    "findInScrollback:",
];

/// The shortcut cards: [`KEY_CARDS`]'s entries of [`help::SHORTCUTS`].
pub fn key_cards() -> Vec<&'static Shortcut> {
    KEY_CARDS
        .iter()
        .filter_map(|selector| help::shortcut_for(selector))
        .collect()
}

/// A card's short name: the menu title without its ellipsis.
pub fn card_title(entry: &Shortcut) -> &'static str {
    entry.title.trim_end_matches('…')
}

/// A choice of the setup page.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub title: &'static str,
    pub detail: &'static str,
    /// What choosing it writes.
    pub edit: SettingsEdit,
    /// The file holds it now.
    pub selected: bool,
}

/// Where the tabs sit: one card per [`TabPosition`].
pub fn tab_choices(settings: &Settings) -> Vec<Choice> {
    TabPosition::NAMES
        .iter()
        .map(|&(_, position)| {
            let (title, detail) = match position {
                TabPosition::Top => ("Tabs on top", "A bar in the title row"),
                TabPosition::Side => ("Side column", "A column down the left edge"),
            };
            Choice {
                title,
                detail,
                edit: SettingsEdit::TabPosition(position),
                selected: settings.tab_position == position,
            }
        })
        .collect()
}

/// How a split looks: one card per [`SplitStyle`].
pub fn split_choices(settings: &Settings) -> Vec<Choice> {
    SplitStyle::NAMES
        .iter()
        .map(|&(_, style)| {
            let (title, detail) = match style {
                SplitStyle::Cards => ("Cards", "Panes stand apart, lit from above"),
                SplitStyle::Lines => ("Lines", "Panes touch, one thin line between"),
            };
            Choice {
                title,
                detail,
                edit: SettingsEdit::SplitStyle(style),
                selected: settings.split_style == style,
            }
        })
        .collect()
}

/// A row of the last page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// The keys (`⌘,`) or the command (`bateri config list`) the row starts with.
    pub lead: String,
    /// Whether `lead` is a command to type rather than keys.
    pub command: bool,
    pub title: &'static str,
    pub detail: &'static str,
}

/// Where to go next: the settings window and the palette by their menu keys, then the two
/// commands of the terminal.
pub fn next_steps() -> Vec<Step> {
    let keys = |selector: &str| {
        help::shortcut_for(selector)
            .map(Shortcut::glyphs)
            .unwrap_or_default()
    };
    vec![
        Step {
            lead: keys("openSettings:"),
            command: false,
            title: "Settings",
            detail: "Every option, in a window",
        },
        Step {
            lead: keys("toggleCommandPalette:"),
            command: false,
            title: "Command palette",
            detail: "Commands, settings, themes and tabs, by name",
        },
        Step {
            lead: "bateri config list".to_owned(),
            command: true,
            title: "Settings from the terminal",
            detail: "Every setting, its values and its meaning",
        },
        Step {
            lead: "bateri help".to_owned(),
            command: true,
            title: "A guide in the terminal",
            detail: "Shortcuts and docs, whenever you need them",
        },
    ]
}

/// Keys as VoiceOver should say them: `⇧⌘P` → "Shift Command P".
pub fn spoken_keys(glyphs: &str) -> String {
    glyphs
        .chars()
        .map(|c| match c {
            '⌃' => "Control".to_owned(),
            '⌥' => "Option".to_owned(),
            '⇧' => "Shift".to_owned(),
            '⌘' => "Command".to_owned(),
            '↩' => "Return".to_owned(),
            '⇥' => "Tab".to_owned(),
            '←' => "Left Arrow".to_owned(),
            '→' => "Right Arrow".to_owned(),
            '↑' => "Up Arrow".to_owned(),
            '↓' => "Down Arrow".to_owned(),
            ',' => "Comma".to_owned(),
            other => other.to_uppercase().collect(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::TempRoot;

    /// Every key on every page: (page, focused row) → what the key does.
    #[test]
    fn every_key_on_every_page() {
        let at = |page: usize, row: usize| {
            let mut tour = Tour::default();
            tour.apply(Move::To(page));
            tour.apply(Move::Focus(Row::ALL[row]));
            tour
        };
        // Every page: ⏎ the main button, esc closes, ⇥ ⇧⇥ step and stop, ⌘n jumps.
        for page in 0..Page::ALL.len() {
            let tour = at(page, 0);
            let last = page + 1 == Page::ALL.len();
            assert_eq!(
                tour.key(Key::Return),
                if last {
                    Move::Close
                } else {
                    Move::To(page + 1)
                }
            );
            assert_eq!(tour.key(Key::Escape), Move::Close);
            assert_eq!(
                tour.key(Key::Tab),
                if last { Move::Stay } else { Move::To(page + 1) }
            );
            assert_eq!(
                tour.key(Key::BackTab),
                if page == 0 {
                    Move::Stay
                } else {
                    Move::To(page - 1)
                }
            );
            for jump in 0..Page::ALL.len() {
                let expected = if jump == page {
                    Move::Stay
                } else {
                    Move::To(jump)
                };
                assert_eq!(tour.key(Key::Page(jump)), expected);
            }
            assert_eq!(tour.key(Key::Page(9)), Move::Stay);
        }
        // Pages 1, 2 and 4: ← → step pages; ↑ ↓, ␣ and letters do nothing.
        for page in [0, 1, 3] {
            let tour = at(page, 0);
            assert_eq!(
                tour.key(Key::Left),
                if page == 0 {
                    Move::Stay
                } else {
                    Move::To(page - 1)
                }
            );
            assert_eq!(
                tour.key(Key::Right),
                if page == 3 {
                    Move::Stay
                } else {
                    Move::To(page + 1)
                }
            );
            for key in [Key::Up, Key::Down, Key::Space, Key::Char('n')] {
                assert_eq!(tour.key(key), Move::Stay, "{key:?} on page {page}");
            }
        }
        // The setup page: ↑ ↓ move between its rows and stop at the ends.
        assert_eq!(at(2, 0).key(Key::Up), Move::Stay);
        assert_eq!(at(2, 0).key(Key::Down), Move::Focus(Row::Splits));
        assert_eq!(at(2, 1).key(Key::Down), Move::Focus(Row::Theme));
        assert_eq!(at(2, 1).key(Key::Up), Move::Focus(Row::Tabs));
        assert_eq!(at(2, 2).key(Key::Down), Move::Stay);
        // ← → change the focused row: card rows hold at the ends, the theme goes round.
        for (row, which) in [(0, Row::Tabs), (1, Row::Splits)] {
            assert_eq!(
                at(2, row).key(Key::Right),
                Move::Change {
                    row: which,
                    delta: 1,
                    wrap: false
                }
            );
            assert_eq!(
                at(2, row).key(Key::Left),
                Move::Change {
                    row: which,
                    delta: -1,
                    wrap: false
                }
            );
            assert_eq!(
                at(2, row).key(Key::Space),
                Move::Change {
                    row: which,
                    delta: 1,
                    wrap: true
                }
            );
            assert_eq!(at(2, row).key(Key::Char('n')), Move::Stay);
        }
        assert_eq!(
            at(2, 2).key(Key::Right),
            Move::Change {
                row: Row::Theme,
                delta: 1,
                wrap: true
            }
        );
        assert_eq!(at(2, 2).key(Key::Space), Move::OpenList);
        assert_eq!(at(2, 2).key(Key::Char('n')), Move::Type('n'));
        assert_eq!(at(2, 2).key(Key::Char('-')), Move::Stay);
    }

    #[test]
    fn the_back_and_next_keys_work_on_every_page_and_never_change_a_value() {
        for page in 0..Page::ALL.len() {
            for row in 0..Row::ALL.len() {
                let mut tour = Tour::default();
                tour.apply(Move::To(page));
                tour.apply(Move::Focus(Row::ALL[row]));
                let back = if page == 0 {
                    Move::Stay
                } else {
                    Move::To(page - 1)
                };
                let next = if page + 1 == Page::ALL.len() {
                    Move::Stay
                } else {
                    Move::To(page + 1)
                };
                assert_eq!(tour.key(Key::PreviousPage), back, "⌘[ on page {page}");
                assert_eq!(tour.key(Key::NextPage), next, "⌘] on page {page}");
                assert_eq!(tour.key(Key::Delete), back, "⌫ on page {page}");
                assert_eq!(tour.key(Key::BackTab), back, "⇧⇥ on page {page}");
            }
        }
        // On the setup page ← at the first value stays a value key: it does not go back.
        let mut setup = Tour::default();
        setup.apply(Move::To(2));
        assert_eq!(
            setup.key(Key::Left),
            Move::Change {
                row: Row::Tabs,
                delta: -1,
                wrap: false
            }
        );
        // ⌫ while a theme is being typed ends the typing, it does not leave the page.
        setup.apply(Move::Focus(Row::Theme));
        assert_eq!(setup.key_while(Key::Delete, true), Move::Stay);
        assert_eq!(setup.key_while(Key::Delete, false), Move::To(1));
        setup.apply(Move::Focus(Row::Tabs));
        assert_eq!(setup.key_while(Key::Delete, true), Move::To(1));
        assert!(setup.hint().contains("⌘← back"));
    }

    #[test]
    fn a_type_select_is_active_until_a_pause() {
        let themes = vec![("nord".to_owned(), "Nord".to_owned())];
        let start = Instant::now();
        let mut select = TypeSelect::default();
        assert!(!select.is_active(start));
        select.push('n', start, &themes);
        assert!(select.is_active(start + TYPE_GAP));
        assert!(!select.is_active(start + TYPE_GAP + Duration::from_millis(1)));
        select.clear();
        assert!(!select.is_active(start));
    }

    #[test]
    fn arriving_at_the_setup_page_focuses_its_first_row() {
        let mut tour = Tour::default();
        tour.apply(Move::To(2));
        tour.apply(Move::Focus(Row::Theme));
        assert_eq!(tour.row(), Row::Theme);
        tour.apply(Move::To(3));
        tour.apply(Move::To(2));
        assert_eq!(
            tour.row(),
            Row::Tabs,
            "the highlight starts on the first row"
        );
        assert_eq!(tour.apply(Move::Close), None);
        assert_eq!(tour.primary(), "Next");
        assert!(tour.hint().starts_with("↑↓ choose"));
        tour.apply(Move::To(3));
        assert_eq!(tour.primary(), "Let's go");
        assert_eq!(tour.position_label(), "Page 4 of 4: Where to go next");
    }

    #[test]
    fn a_card_row_steps_once_and_holds_at_its_ends() {
        let choices = tab_choices(&Settings::default()); // top selected
        assert_eq!(
            step_choice(&choices, -1, false),
            None,
            "held ← at the end writes nothing"
        );
        assert_eq!(
            step_choice(&choices, 1, false).map(|c| c.title),
            Some("Side column")
        );
        assert_eq!(
            step_choice(&choices, 1, true).map(|c| c.title),
            Some("Side column")
        );
        let side = tab_choices(&Settings {
            tab_position: TabPosition::Side,
            ..Settings::default()
        });
        assert_eq!(step_choice(&side, 1, false), None);
        assert_eq!(
            step_choice(&side, 1, true).map(|c| c.title),
            Some("Tabs on top")
        );
    }

    #[test]
    fn the_theme_row_goes_round_and_skips_separators() {
        let names = vec![
            Some("system".to_owned()),
            None,
            Some("bateri".to_owned()),
            Some("nord".to_owned()),
        ];
        assert_eq!(step_theme(&names, "system", 1).as_deref(), Some("bateri"));
        assert_eq!(step_theme(&names, "nord", 1).as_deref(), Some("system"));
        assert_eq!(step_theme(&names, "system", -1).as_deref(), Some("nord"));
        assert_eq!(step_theme(&[Some("x".to_owned())], "x", 1), None);
    }

    #[test]
    fn typed_letters_select_a_theme_and_a_pause_starts_over() {
        let themes = vec![
            ("system".to_owned(), "Match System".to_owned()),
            ("nord".to_owned(), "Nord".to_owned()),
            ("catppuccin-mocha".to_owned(), "Catppuccin Mocha".to_owned()),
            ("catppuccin-latte".to_owned(), "Catppuccin Latte".to_owned()),
        ];
        let start = Instant::now();
        let mut select = TypeSelect::default();
        assert_eq!(select.push('N', start, &themes).as_deref(), Some("nord"));
        let mut select = TypeSelect::default();
        assert_eq!(
            select.push('c', start, &themes).as_deref(),
            Some("catppuccin-mocha")
        );
        // A second letter narrows the same search: "ca" is still the first Catppuccin.
        assert_eq!(
            select
                .push('a', start + Duration::from_millis(200), &themes)
                .as_deref(),
            Some("catppuccin-mocha")
        );
        // After a pause the letters start over.
        let later = start + Duration::from_millis(200) + TYPE_GAP + Duration::from_millis(1);
        assert_eq!(select.push('m', later, &themes).as_deref(), Some("system"));
        assert_eq!(select.push('z', later, &themes), None);
    }

    #[test]
    fn a_held_key_writes_once_where_it_stops() {
        let start = Instant::now();
        let mut writes = Vec::new();
        let mut debounce = Debounce::default();
        // Thirty key repeats, 40 ms apart: a preview each, no write while they come.
        for step in 0..30u64 {
            let now = start + Duration::from_millis(40 * step);
            debounce.note(format!("theme-{step}"), now);
            writes.extend(debounce.due(now));
        }
        assert!(writes.is_empty(), "no write while the key is held");
        let last = start + Duration::from_millis(40 * 29);
        assert_eq!(debounce.due(last + WRITE_QUIET / 2), None);
        writes.extend(debounce.due(last + WRITE_QUIET));
        assert_eq!(writes, ["theme-29"], "one write, of where it stopped");
        assert!(!debounce.is_pending());
        // Leaving the row writes what is pending at once, and only once.
        debounce.note("nord".to_owned(), last);
        assert_eq!(debounce.flush().as_deref(), Some("nord"));
        assert_eq!(debounce.flush(), None);
        assert_eq!(announce(Row::Theme, "Nord"), "Theme: Nord");
    }

    #[test]
    fn the_cards_are_the_menus_shortcuts() {
        let cards = key_cards();
        assert_eq!(
            cards.len(),
            KEY_CARDS.len(),
            "a card's action is not in SHORTCUTS"
        );
        let glyphs: Vec<String> = cards.iter().map(|card| card.glyphs()).collect();
        assert_eq!(glyphs, ["⌘T", "⌘D", "⇧⌘P", "⌘P", "⇧⌘D", "⌘F"]);
        assert_eq!(card_title(cards[2]), "Command Palette");
    }

    #[test]
    fn every_setup_value_has_a_card_that_writes_it() {
        let mut settings = Settings::default();
        let tabs = tab_choices(&settings);
        assert_eq!(tabs.len(), TabPosition::NAMES.len());
        assert_eq!(
            tabs.iter().filter(|choice| choice.selected).count(),
            1,
            "exactly the file's value is selected"
        );
        assert_eq!(split_choices(&settings).len(), SplitStyle::NAMES.len());
        // Each card's edit, written and read back, is the value it stands for.
        for choice in tab_choices(&settings)
            .into_iter()
            .chain(split_choices(&settings))
        {
            let written =
                Settings::with_edit(Settings::TEMPLATE, &choice.edit).expect("writable template");
            let read = Settings::parse(&written).expect("parses").settings;
            match &choice.edit {
                SettingsEdit::TabPosition(position) => assert_eq!(read.tab_position, *position),
                SettingsEdit::SplitStyle(style) => assert_eq!(read.split_style, *style),
                other => panic!("unexpected edit {other:?}"),
            }
        }
        settings.tab_position = TabPosition::Side;
        assert!(
            tab_choices(&settings)
                .iter()
                .any(|choice| choice.selected && choice.title == "Side column")
        );
    }

    #[test]
    fn the_next_steps_lead_with_the_menus_keys() {
        let steps = next_steps();
        assert_eq!(steps[0].lead, "⌘,");
        assert_eq!(steps[1].lead, "⇧⌘P");
        assert!(steps[2].command && steps[2].lead.starts_with("bateri config"));
        assert!(steps.iter().all(|step| !step.lead.is_empty()));
    }

    #[test]
    fn keys_are_spoken_in_words() {
        assert_eq!(spoken_keys("⇧⌘P"), "Shift Command P");
        assert_eq!(spoken_keys("⌘,"), "Command Comma");
        assert_eq!(spoken_keys("⇧⌘↩"), "Shift Command Return");
    }

    #[test]
    fn the_first_launch_is_decided_once() {
        let home = TempRoot::new("welcome-first");
        assert!(first_launch(&home.0));
        assert!(marker_path(&home.0).is_file());
        assert!(!first_launch(&home.0), "a second launch is not the first");
        // A home where the marker can't be made never counts as a first launch.
        let blocked = TempRoot::new("welcome-blocked");
        std::fs::write(blocked.0.join("Library"), "a file").expect("write");
        assert!(!first_launch(&blocked.0));
        assert!(!first_launch(&blocked.0));
    }
}
