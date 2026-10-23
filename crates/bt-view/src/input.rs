//! What the host tells a page, and what a page tells the host back.
//!
//! The engine reads no device: the host turns what the person did (a pointer move, a press, a
//! wheel, a key) into an [`Input`], in the page's points, and a page answers with a [`Reply`] —
//! whether anything it shows changed (so a frame is worth drawing), and the few things only the
//! host can do: pop up a menu, move the keyboard into a text field, copy, close the page.

use crate::Point;

/// A key the page reads. Letters and symbols come as [`Key::Char`] — the character the key makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Escape,
    Tab,
    Char(char),
}

/// The modifiers held with a key or a press.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub control: bool,
    pub option: bool,
    pub command: bool,
}

impl Mods {
    /// Whether ⌃, ⌥ or ⌘ is held: the key is a shortcut and not a letter being typed.
    pub fn any_command(self) -> bool {
        self.control || self.option || self.command
    }
}

/// A pointer button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

/// One thing the person did, in the page's points (the page's top-left corner is the origin).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    /// The pointer moved over the page.
    Move(Point),
    /// The pointer left the page.
    Leave,
    Press {
        at: Point,
        button: Button,
        mods: Mods,
    },
    Release {
        at: Point,
        button: Button,
    },
    /// The wheel or the trackpad: `dy` points the content moves *up* by (positive scrolls
    /// down the page).
    Scroll {
        at: Point,
        dy: f32,
    },
    Key {
        key: Key,
        mods: Mods,
    },
}

/// Everything a page is told: what the person did to the page, the text a text field of the
/// host's now holds, the entry picked of a menu the page asked for. A page that lives apart from
/// the host is told by this one type.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Input(Input),
    /// The host's text field holds this.
    Text(String),
    /// The person picked the entry with this index of the menu the page asked for.
    MenuPick(usize),
}

/// The shape the pointer takes over the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cursor {
    #[default]
    Arrow,
    /// Over something that answers a click.
    Pointer,
    /// Over the room of a text field the host has put over the page.
    Text,
}

/// Where the keyboard goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyboard {
    /// The page's own keys.
    Page,
    /// The page's filter field, a text field of the host's: it asks for the keyboard and
    /// types into it.
    Filter,
}

/// One line of a menu the host pops up for the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuEntry {
    Item {
        title: String,
        /// The key that does the same on the page, shown at the end of the line.
        key: Option<char>,
        enabled: bool,
    },
    Separator,
}

/// A menu to pop up at `at` (the page's points); the host answers with the index of the entry
/// picked ([`MenuEntry`]s in order, separators counted).
#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub at: Point,
    pub entries: Vec<MenuEntry>,
}

/// What a page says back after an [`Input`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    /// Something the page shows changed: a frame is worth drawing. An input that changed
    /// nothing (a pointer moving inside the same row) answers `false`, and a page at rest
    /// costs no frames.
    pub redraw: bool,
    /// The page asks to be closed.
    pub close: bool,
    /// A menu to pop up.
    pub menu: Option<Menu>,
    /// The keyboard goes somewhere else than where it was.
    pub keyboard: Option<Keyboard>,
    /// Text for the pasteboard.
    pub copy: Option<String>,
    /// The pointer's shape after this input.
    pub cursor: Cursor,
}

impl Reply {
    /// Nothing changed.
    pub fn none() -> Self {
        Self::default()
    }

    /// Something changed: draw.
    pub fn redraw() -> Self {
        Self {
            redraw: true,
            ..Self::default()
        }
    }

    /// The same reply with the pointer's shape `cursor`.
    #[must_use]
    pub fn with_cursor(mut self, cursor: Cursor) -> Self {
        self.cursor = cursor;
        self
    }
}
