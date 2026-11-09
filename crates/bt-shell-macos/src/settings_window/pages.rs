//! The pages of Settings: which groups and rows each page holds and in what order, each row's label
//! and unit, and which rows lean on another setting.
//!
//! The window is built from this table and nothing else: [`super::SettingsWindow::build_panes`]
//! walks [`groups`] page by page, and a test holds every settings key to exactly one place in it. A
//! row keeps its place on its page whether or not it applies right now — a row that leans on
//! another setting is dimmed and says why ([`dependency`]); only a row that has no meaning at all
//! goes away (Light and Dark while the theme is a theme of its own).

use bt_core::{CursorBlink, RemoteStatsMode, Settings, ShellIntegration, TabDetails, TabPosition};

use super::{Category, Key};
use crate::program::identity::Kind;
use crate::program::reply;

/// What a group holds: a settings key's row, or one of the rows that are not a setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Item {
    Key(Key),
    /// "In use": the preview folder's size, with Clear Now. Measured, never written.
    Usage,
    /// What the Agents switch does, and an example of the row it adds a line to. Fixed.
    AgentAbout,
    /// Where on the screen the line shows: it follows Tab Details and the tabs' position, with
    /// the button that opens the page those are on.
    AgentWhere,
    /// Apps ▸ Following: a row for each entry of `[apps] sources`, with its sign, Review and
    /// Remove ([`super::apps`]). Written only by those buttons, through the list.
    Followed,
    /// Apps ▸ Add: the field that takes a new source, and its button.
    Add,
}

/// A titled run of rows in one rounded box. A group without a title is a page's first.
pub(super) struct Group {
    pub(super) title: Option<&'static str>,
    /// The small text beside the title, when the settings call for one.
    pub(super) aside: fn(&Settings) -> Option<&'static str>,
    pub(super) items: &'static [Item],
}

fn no_aside(_: &Settings) -> Option<&'static str> {
    None
}

/// The dock's rows lean on Shell integration being Auto; the group says so once instead of each row.
fn dock_aside(settings: &Settings) -> Option<&'static str> {
    (settings.shell_integration != ShellIntegration::Auto)
        .then_some("needs Shell integration: Auto")
}

fn previews_aside(_: &Settings) -> Option<&'static str> {
    Some("\u{2318}-click a file name on a server")
}

const fn row(key: Key) -> Item {
    Item::Key(key)
}

const fn group(title: Option<&'static str>, items: &'static [Item]) -> Group {
    Group {
        title,
        aside: no_aside,
        items,
    }
}

const GENERAL: &[Group] = &[
    group(
        Some("Quitting and reopening"),
        &[
            row(Key::RestoreWindows),
            row(Key::KeepRunning),
            row(Key::ConfirmClose),
        ],
    ),
    group(
        Some("Terminal"),
        &[row(Key::Scrollback), row(Key::Clipboard)],
    ),
];

const APPEARANCE: &[Group] = &[
    group(
        None,
        &[
            row(Key::Theme),
            row(Key::LightTheme),
            row(Key::DarkTheme),
            row(Key::MinimumContrast),
        ],
    ),
    group(
        Some("Window"),
        &[
            row(Key::ContentEdge),
            row(Key::Scrollbar),
            row(Key::ResizeInSteps),
        ],
    ),
];

const FONT: &[Group] = &[
    group(
        None,
        &[
            row(Key::Font),
            row(Key::Size),
            row(Key::LineHeight),
            row(Key::LetterSpacing),
            row(Key::FontSmoothing),
        ],
    ),
    // The selected font's own options; the group goes with them when it has none.
    group(Some("Options"), &[row(Key::FontFeatures)]),
];

const TABS: &[Group] = &[
    group(
        Some("Tabs"),
        &[
            row(Key::TabPosition),
            row(Key::TabIcons),
            row(Key::TabIconColors),
            row(Key::TabRunning),
            row(Key::TabDetails),
        ],
    ),
    group(
        Some("Split tabs"),
        &[
            row(Key::SplitStyle),
            row(Key::DimSplits),
            row(Key::FocusFrame),
            row(Key::MinimizedPanes),
        ],
    ),
];

const CURSOR: &[Group] = &[group(
    None,
    &[
        row(Key::Shape),
        row(Key::Blink),
        row(Key::BlinkSpeed),
        row(Key::Radius),
        row(Key::Glow),
        row(Key::Unfocused),
    ],
)];

const MOTION: &[Group] = &[
    group(None, &[row(Key::ReduceMotion)]),
    group(
        Some("At the prompt"),
        &[
            row(Key::Keypress),
            row(Key::Erase),
            row(Key::CursorMotion),
            row(Key::DockArrival),
        ],
    ),
    group(
        Some("Scrolling"),
        &[row(Key::SmoothScroll), row(Key::ScrollBounce)],
    ),
];

const SHELL: &[Group] = &[
    group(None, &[row(Key::ShellIntegration)]),
    Group {
        title: Some("In the dock"),
        aside: dock_aside,
        items: &[row(Key::PythonEnv), row(Key::NodeVersion), row(Key::Ports)],
    },
    group(Some("bateri command"), &[row(Key::CommandLevel)]),
];

const SERVERS: &[Group] = &[
    group(
        Some("Over ssh"),
        &[
            row(Key::RemoteIntegration),
            row(Key::RemoteStats),
            row(Key::StatsInterval),
        ],
    ),
    Group {
        title: Some("Previews"),
        aside: previews_aside,
        items: &[
            row(Key::PreviewMaxSize),
            row(Key::PreviewReadOnly),
            row(Key::PreviewDir),
            row(Key::PreviewKeep),
            row(Key::PreviewLimit),
            Item::Usage,
        ],
    },
    group(
        Some("Downloads"),
        &[
            row(Key::DownloadDir),
            row(Key::DownloadConflict),
            row(Key::DownloadNotify),
        ],
    ),
];

/// The Agents page: the switch of each agent, then what it does and where it shows. The two rows
/// under the switch are no rows of their own to the eye — they read as the switch's own
/// explanation, so they have no rule above them.
const AGENTS: &[Group] = &[group(
    None,
    &[
        row(Key::ClaudeIntegration),
        Item::AgentAbout,
        Item::AgentWhere,
    ],
)];

fn following_aside(_: &Settings) -> Option<&'static str> {
    Some("kept on this computer; a server gets one when you ask")
}

const APPS: &[Group] = &[
    Group {
        title: Some("Following"),
        aside: following_aside,
        items: &[Item::Followed],
    },
    group(Some("Add"), &[Item::Add]),
];

/// A page's groups, top to bottom. Keyboard is a table of its own and has none.
pub(super) fn groups(category: Category) -> &'static [Group] {
    match category {
        Category::General => GENERAL,
        Category::Appearance => APPEARANCE,
        Category::Font => FONT,
        Category::Tabs => TABS,
        Category::Cursor => CURSOR,
        Category::Motion => MOTION,
        Category::Shell => SHELL,
        Category::Keyboard => &[],
        Category::Servers => SERVERS,
        Category::Agents => AGENTS,
        Category::Apps => APPS,
    }
}

/// The row's name, as the page words it.
pub(super) fn label(key: Key) -> &'static str {
    match key {
        Key::RestoreWindows => "Reopen after quitting",
        Key::KeepRunning => "Keep programs running",
        Key::ConfirmClose => "Ask before closing",
        Key::Scrollback => "Scrollback",
        Key::Clipboard => "Let programs copy to the clipboard",
        Key::Theme => "Theme",
        Key::LightTheme => "Light",
        Key::DarkTheme => "Dark",
        Key::MinimumContrast => "Minimum contrast",
        Key::ContentEdge => "Where text meets the tab bar",
        Key::Scrollbar => "Scroll bar",
        Key::ResizeInSteps => "Resize a whole cell at a time",
        Key::Font => "Font",
        Key::Size => "Size",
        Key::LineHeight => "Line height",
        Key::LetterSpacing => "Letter spacing",
        Key::FontSmoothing => "Bolder text",
        // The options' own rows carry their names; the group's title says whose they are.
        Key::FontFeatures => "",
        Key::TabPosition => "Tabs",
        Key::TabIcons => "Program icons",
        Key::TabIconColors => "Icon colors",
        Key::TabRunning => "While a command runs",
        Key::TabDetails => "Details under a tab",
        Key::SplitStyle => "Panes",
        Key::MinimizedPanes => "Minimized floating panes",
        Key::DimSplits => "Dim the other panes",
        Key::FocusFrame => "Frame the focused pane",
        Key::Shape => "Shape",
        Key::Blink => "Blink",
        Key::BlinkSpeed => "Blink speed",
        Key::Radius => "Corner radius",
        Key::Glow => "Glow",
        Key::Unfocused => "In an inactive window",
        Key::ReduceMotion => "Reduce motion",
        Key::Keypress => "A letter you type",
        Key::Erase => "A letter you delete",
        Key::CursorMotion => "Cursor",
        Key::DockArrival => "When a shell starts",
        Key::SmoothScroll => "Smooth scrolling",
        Key::ScrollBounce => "Bounce at the ends",
        Key::ShellIntegration => "Shell integration",
        Key::PythonEnv => "Python environment",
        Key::NodeVersion => "Node version",
        Key::Ports => "Listening ports",
        Key::CommandLevel => "Programs on this computer may",
        Key::RemoteIntegration => "Set up shell integration on servers",
        Key::RemoteStats => "Server load",
        Key::StatsInterval => "Sample every",
        Key::PreviewMaxSize => "Open without asking up to",
        Key::PreviewReadOnly => "Open read-only",
        Key::PreviewDir | Key::DownloadDir => "Folder",
        Key::PreviewKeep => "Keep previews",
        Key::PreviewLimit => "At most",
        Key::DownloadConflict => "If the name exists",
        Key::DownloadNotify => "Notify when done",
        Key::ClaudeIntegration => "Claude Code",
    }
}

/// The program whose tile stands before the row's name, when the row is about one program: the
/// agent's row of the Agents page ([`reply::REPLIERS`]).
pub(super) fn lead(key: Key) -> Option<Kind> {
    match key {
        Key::ClaudeIntegration => reply::REPLIERS.first().map(|agent| agent.kind),
        _ => None,
    }
}

/// What the Agents switch does: the sentence under the switch, over the example.
pub(super) const AGENT_ABOUT: &str = "When Claude Code finishes a turn, its reply appears in its row, right under its title. Sessions already running show it after a restart.";

/// Where the reply shows now, and whether the way to change that is worth a button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AgentWhere {
    pub(super) text: &'static str,
    /// The answer lies in Tabs and Panes, and a click there can change it.
    pub(super) button: bool,
}

/// The reply shows in the side column only, in Activity and — with Details under a tab on Always —
/// in the tab list; Off shows it nowhere. The one thing on the Agents page that leans on other
/// settings, so nothing else on the page moves with the switch.
pub(super) fn agent_where(settings: &Settings) -> AgentWhere {
    if settings.tab_position == TabPosition::Top {
        return AgentWhere {
            text: "Tabs are on top. It shows only with tabs in the side column.",
            button: true,
        };
    }
    match settings.tab_details {
        TabDetails::Off => AgentWhere {
            text: "Details under a tab is Off, so it shows nowhere.",
            button: true,
        },
        TabDetails::Always => AgentWhere {
            text: "Shows in Activity and in the tab list.",
            button: false,
        },
        TabDetails::Smart => AgentWhere {
            text: "Shows in Activity. To see it in the tab list too, set Details under a tab to Always.",
            button: true,
        },
    }
}

/// The unit that stands beside the row's control, so the description never repeats it.
pub(super) fn unit(key: Key) -> Option<&'static str> {
    match key {
        Key::Scrollback => Some("lines"),
        Key::Size => Some("pt"),
        Key::LineHeight | Key::LetterSpacing => Some("\u{d7}"),
        Key::StatsInterval => Some("seconds"),
        _ => None,
    }
}

/// Whether the row's control has the width of the row to itself, under the name and the
/// description: the font's list of switches and the pictured choices.
pub(super) fn below(key: Key) -> bool {
    matches!(
        key,
        Key::FontFeatures
            | Key::Theme
            | Key::LightTheme
            | Key::DarkTheme
            | Key::ContentEdge
            | Key::TabPosition
            | Key::SplitStyle
            | Key::Shape
            | Key::Unfocused
            | Key::TabRunning
    )
}

/// How another setting changes a row: gone, or left in place but dimmed with its reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Dependency {
    /// The row means nothing now and is not shown.
    pub(super) hidden: bool,
    /// The row's controls take input.
    pub(super) enabled: bool,
    /// What the dimmed row says in place of its description; `None` when the group already says it.
    pub(super) reason: Option<&'static str>,
}

impl Dependency {
    const FREE: Dependency = Dependency {
        hidden: false,
        enabled: true,
        reason: None,
    };

    fn off(reason: Option<&'static str>) -> Dependency {
        Dependency {
            hidden: false,
            enabled: false,
            reason,
        }
    }
}

/// Whether a row leans on another setting, and what it does about it — pure, like
/// [`super::motion_override`], which owns the rows that Cursor motion and Reduce motion turn off.
pub(super) fn dependency(key: Key, settings: &Settings) -> Dependency {
    match key {
        // Light and Dark answer "which theme when the system is dark or light": with a theme of
        // its own chosen they have no meaning at all.
        Key::LightTheme | Key::DarkTheme if !settings.follows_system() => Dependency {
            hidden: true,
            enabled: true,
            reason: None,
        },
        Key::BlinkSpeed if settings.cursor_blink == CursorBlink::Off => {
            Dependency::off(Some("Blink is Off."))
        }
        Key::TabIconColors if !settings.tab_icons => {
            Dependency::off(Some("Program icons are off."))
        }
        Key::TabDetails if settings.tab_position == TabPosition::Top => {
            Dependency::off(Some("Only in the side column."))
        }
        Key::StatsInterval if settings.remote_stats.mode == RemoteStatsMode::Off => {
            Dependency::off(Some("Server load is Off."))
        }
        Key::PythonEnv | Key::NodeVersion | Key::Ports
            if settings.shell_integration != ShellIntegration::Auto =>
        {
            Dependency::off(None)
        }
        _ => Dependency::FREE,
    }
}
