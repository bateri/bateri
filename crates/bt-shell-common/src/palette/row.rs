//! The palette's row: what one line of the list is, independent of how it is drawn.
//!
//! A row is data — an identity, a kind, a title and the words it is found by, what stands at
//! its right, and what ⏎ and → do with it. The toolkit turns it into a view; the matcher
//! ([`super::rank`]) and the state machine ([`super::Palette`]) read it as it is.

/// What a row stands for. The kind fixes the **identity prefix** (the part of a row's id before
/// the key, stable across versions: [`super::Recent`] keeps ids on disk) and the **cluster** the
/// row sorts under in a root search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A menu item (`cmd.<selector>`).
    Command,
    /// A setting (`set.<dotted key>`).
    Setting,
    /// A theme (`theme.<name>`).
    Theme,
    /// A tab of any window (`tab.<id>`).
    Tab,
    /// A tab group (`grp.<id>`).
    Group,
    /// A command a pane ran (`hist.<id>`).
    History,
    /// A folder (`dir.<path>`).
    Folder,
    /// A server (`host.<name>`).
    Host,
    /// A listening port (`port.<number>`).
    Port,
    /// A page of the palette that lists something (`scope.<name>`).
    Scope,
    /// A fallback that searches elsewhere (`search.<name>`).
    Search,
    /// A choice on a setting's page (`opt.<token>`).
    Choice,
    /// An action on a page about one thing (`act.<name>`).
    Action,
}

impl Kind {
    /// The id prefix, dot included.
    pub fn prefix(self) -> &'static str {
        match self {
            Kind::Command => "cmd.",
            Kind::Setting => "set.",
            Kind::Theme => "theme.",
            Kind::Tab => "tab.",
            Kind::Group => "grp.",
            Kind::History => "hist.",
            Kind::Folder => "dir.",
            Kind::Host => "host.",
            Kind::Port => "port.",
            Kind::Scope => "scope.",
            Kind::Search => "search.",
            Kind::Choice => "opt.",
            Kind::Action => "act.",
        }
    }

    /// The cluster heading in a root search; empty for rows that are never clustered.
    pub fn cluster(self) -> &'static str {
        match self {
            Kind::Command => "Commands",
            Kind::Setting => "Settings",
            Kind::Theme => "Themes",
            Kind::Tab => "Tabs",
            Kind::Group => "Groups",
            Kind::History => "Command History",
            Kind::Folder => "Folders",
            Kind::Host => "Hosts",
            Kind::Port => "Ports",
            Kind::Scope => "Go to",
            Kind::Search => "Search",
            Kind::Choice | Kind::Action => "",
        }
    }

    /// Whether choosing such a row is remembered in Recent: what the menu and the settings name
    /// — a command, a setting, a page. Never a tab (⌘P has its order), a command's text, a
    /// folder or a host.
    pub fn remembered(self) -> bool {
        matches!(self, Kind::Command | Kind::Setting | Kind::Scope)
    }
}

/// How a status reads: the toolkit maps it to a color of the theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Dim,
    Accent,
    Success,
    Warning,
    Error,
}

/// A short state at a row's right: "Asking", "✗ exit 1", "Production".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub text: String,
    pub tone: Tone,
}

/// A setting's value shown while a row is highlighted, without writing it: the dotted key and
/// the value's token ([`super::Setting::edit`] turns the pair into the edit).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    pub key: String,
    pub token: String,
}

/// What a theme looks like in a row: its ground and six of its colours (the ANSI red, green,
/// yellow, blue, magenta and cyan), as `0xRRGGBB`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Swatch {
    pub ground: u32,
    pub dots: [u32; 6],
}

/// Whose appearance a theme page chooses for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// `[appearance] theme`: the one theme, or "Match System".
    Active,
    /// `[appearance] dark_theme`: the theme while the system is dark.
    Dark,
    /// `[appearance] light_theme`: the theme while the system is light.
    Light,
}

/// A page of the palette. The palette is a stack of them; the root is the bottom and has no
/// scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Tabs,
    Folders,
    Hosts,
    Ports,
    Settings,
    /// The command history; `query` seeds the field ("Search Command History for …").
    History {
        jump: bool,
        query: Option<String>,
    },
    /// The themes, for one slot.
    Theme(Slot),
    /// The items of a submenu of the application menu, by the key of its page row
    /// (`Window.Resize-Split`, [`super::menu`]).
    Menu(String),
    /// The choices of the setting with this dotted key.
    Choices(String),
    /// Actions on one tab / group / history entry / host, by its row id.
    TabActions(String),
    GroupActions(String),
    HistoryActions(String),
    /// The menu of a menu command's row, by its row id (`cmd.copyLastOutput`): what can be done
    /// with the command besides running it (Change Shortcut…).
    CommandActions(String),
    Mark(String),
}

impl Scope {
    /// Where the highlight lands on arriving: a page of values opens on the value in effect,
    /// the others on the first row.
    pub(crate) fn lands_on_checked(&self) -> bool {
        matches!(self, Scope::Theme(_) | Scope::Choices(_))
    }
}

/// One line of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The stable identity: the kind's prefix and a key (`cmd.splitRight`, `theme.nord`).
    pub id: String,
    pub kind: Kind,
    pub title: String,
    /// The words the row is found by besides its title, space separated ("zoom bigger smaller").
    pub keywords: String,
    /// The second line; found by a query too.
    pub subtitle: String,
    /// A value at the right ("Beam", "13 pt").
    pub value: Option<String>,
    /// A key combination at the right, as glyphs ("⇧⌘P").
    pub shortcut: Option<String>,
    pub status: Option<Status>,
    /// `Some` for an on/off setting: whether it is on.
    pub toggle: Option<bool>,
    /// The ✓: the tab on screen, the theme in effect, the value chosen.
    pub checked: bool,
    /// The page this row opens (→, ⇥, and ⏎ when [`Self::enters`]).
    pub opens: Option<Scope>,
    /// Whether ⏎ opens the page. A row that does something by itself (goes to a tab, writes a
    /// line) keeps ⏎ for that and opens its page on → or ⇥.
    pub enters: bool,
    /// What ⇥ puts in the field instead of opening a page (a folder's path and a slash).
    pub completion: Option<String>,
    /// Shown, not written, while the row is highlighted.
    pub preview: Option<Preview>,
    /// A theme's colours, for the row to show beside its title.
    pub swatch: Option<Swatch>,
    /// The keys the bottom band names for this row, with what each does, in place of the ones its
    /// kind implies (a command's ⏎, ⌥⏎ and ⌘⏎ depend on where it ran and whether its output is
    /// still there). Empty: the kind's own.
    pub hints: Vec<(&'static str, String)>,
}

impl Row {
    /// A row with the id `kind.prefix() + key`.
    pub fn new(kind: Kind, key: &str, title: impl Into<String>) -> Row {
        Row {
            id: format!("{}{key}", kind.prefix()),
            kind,
            title: title.into(),
            keywords: String::new(),
            subtitle: String::new(),
            value: None,
            shortcut: None,
            status: None,
            toggle: None,
            checked: false,
            opens: None,
            enters: false,
            completion: None,
            preview: None,
            swatch: None,
            hints: Vec::new(),
        }
    }

    pub fn keywords(mut self, words: impl Into<String>) -> Row {
        self.keywords = words.into();
        self
    }

    pub fn subtitle(mut self, text: impl Into<String>) -> Row {
        self.subtitle = text.into();
        self
    }

    pub fn value(mut self, text: impl Into<String>) -> Row {
        self.value = Some(text.into());
        self
    }

    pub fn shortcut(mut self, glyphs: impl Into<String>) -> Row {
        self.shortcut = Some(glyphs.into());
        self
    }

    pub fn status(mut self, text: impl Into<String>, tone: Tone) -> Row {
        self.status = Some(Status {
            text: text.into(),
            tone,
        });
        self
    }

    pub fn toggle(mut self, on: bool) -> Row {
        self.toggle = Some(on);
        self
    }

    pub fn checked(mut self, checked: bool) -> Row {
        self.checked = checked;
        self
    }

    /// The row opens `scope` on →, ⇥ and — when `enters` — on ⏎.
    pub fn opens(mut self, scope: Scope, enters: bool) -> Row {
        self.opens = Some(scope);
        self.enters = enters;
        self
    }

    pub fn completion(mut self, text: impl Into<String>) -> Row {
        self.completion = Some(text.into());
        self
    }

    pub fn preview(mut self, key: impl Into<String>, token: impl Into<String>) -> Row {
        self.preview = Some(Preview {
            key: key.into(),
            token: token.into(),
        });
        self
    }

    pub fn swatch(mut self, swatch: Swatch) -> Row {
        self.swatch = Some(swatch);
        self
    }

    /// The bottom band names `key` as doing `verb` while this row is highlighted.
    pub fn hint(mut self, key: &'static str, verb: impl Into<String>) -> Row {
        self.hints.push((key, verb.into()));
        self
    }

    /// Whether choosing this row goes to Recent.
    pub fn remembered(&self) -> bool {
        self.kind.remembered()
    }
}

/// The id of a menu command: `cmd.` and the selector without its trailing colon
/// (`splitRight:` → `cmd.splitRight`).
pub fn command_id(selector: &str) -> String {
    format!("cmd.{}", selector.trim_end_matches(':'))
}
