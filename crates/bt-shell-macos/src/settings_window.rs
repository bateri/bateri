//! bateri ▸ Settings… (Cmd-,): the settings window — a sidebar of nine pages on the left, the
//! chosen page on the right: titled groups of rows, each row a name and a description with its
//! control beside them (which rows a page holds and in what order is the table in [`pages`];
//! Keyboard is a table of its own: [`keyboard`]).
//!
//! The window **holds no state of its own**: every value it shows comes from
//! `AppDelegate`'s active settings ([`SettingsWindow::refresh`]) and every
//! control's action builds a [`SettingsEdit`] and hands it to
//! `AppDelegate::save_edit` — that one writes the file, and the one that
//! applies is today's file-reading path (`reload_settings`), so a control's
//! value reaches the screen only by coming back from the file.
//!
//! Which row a control is lives in its `tag` ([`Key`]); the action selector
//! is by the control's **kind** (popup, switch, slider, field, stepper), not
//! by row. The popup title ↔ enum variant mapping is in exhaustive `match`es
//! ([`Choice`]), the order of the items is the order of `bt-core`'s `NAMES`
//! table: a new variant is a compile error, it does not silently go missing
//! from the popup.
//!
//! Remote Files adds three row kinds that are not a single
//! control: a size popup (presets, [`size_items`]), a folder row (the path as
//! the file writes it, Change… → `NSOpenPanel`) and the preview folder's usage
//! with Clear Now. The usage is not a setting: it is measured off the main
//! thread by `AppDelegate` and only shown here ([`SettingsWindow::show_usage`]);
//! Clear Now and Show in Finder write nothing, so the lock does not disable them.

use std::cell::{Cell, OnceCell, RefCell};
use std::path::Path;
use std::ptr::NonNull;

use block2::RcBlock;
use bt_core::{
    CURSOR_BLINK_RANGE, CURSOR_GLOW_RANGE, CURSOR_RADIUS_RANGE, CaretShape, CommandLevel,
    ConfirmClose, ContentEdge, CursorBlink, CursorMotion, DockArrival, DownloadConflict, Erase,
    IconColors, KeepRunning, Keypress, LETTER_SPACING_RANGE, LINE_HEIGHT_RANGE, Osc52, PreviewKeep,
    ReduceMotion, RemoteStatsMode, RestoreWindows, SCROLLBACK_MAX, STATS_INTERVAL_RANGE,
    SYSTEM_THEME, ScrollBounce, Scrollbar, Settings, SettingsEdit, ShellIntegration, SmoothScroll,
    SplitStyle, TabDetails, TabPosition, TabRunning, Theme, UnfocusedCaret,
};
use bt_gpu::{FontNotice, ScrollbarMode};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject, Sel};
use objc2::{
    AnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class,
    msg_send, sel,
};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSAppearanceCustomization, NSApplication,
    NSBackingStoreType, NSBezierPath, NSBox, NSBoxType, NSButton, NSColor, NSControl,
    NSControlTextEditingDelegate, NSEventType, NSFont, NSFontAttributeName, NSFontDescriptor,
    NSImage, NSImageScaling, NSImageSymbolConfiguration, NSImageView, NSLayoutAttribute,
    NSLayoutConstraint, NSLineBreakMode, NSMenuItem, NSModalResponse, NSModalResponseOK,
    NSOpenPanel, NSPopUpButton, NSScrollView, NSSearchField, NSSegmentSwitchTracking,
    NSSegmentedControl, NSSlider, NSSplitViewController, NSSplitViewItem, NSStackView, NSStepper,
    NSTableColumn, NSTableView, NSTableViewDataSource, NSTableViewDelegate,
    NSTableViewSelectionHighlightStyle, NSTableViewStyle, NSTextField, NSTitlePosition,
    NSUserInterfaceItemIdentification, NSUserInterfaceLayoutOrientation, NSView, NSViewController,
    NSWindow, NSWindowDelegate, NSWindowOrderingMode, NSWindowStyleMask, NSWindowTabbingMode,
    NSWindowTitleVisibility, NSWorkspace,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSDictionary, NSEdgeInsets, NSIndexSet,
    NSInteger, NSNotification, NSNotificationCenter, NSObject, NSObjectNSDelayedPerforming,
    NSObjectProtocol, NSPoint, NSRect, NSRunLoopCommonModes, NSSize, NSString, NSURL, ns_string,
};

use crate::app;
use crate::child;
use crate::font_options::FontOption;
use crate::remote_files::Sweep;
use crate::settings::{self, FileState};
use crate::upload::format_bytes;
use crate::zoom::{MAX_SIZE, MIN_SIZE};
use look::Ink;
use pictures::{Palette, Picture, PictureChoice};
use switch::ThemeSwitch;
use theme_gallery::ThemeGallery;

mod font_options;
mod keyboard;
mod layout_check;
mod look;
mod pages;
mod pictures;
mod recorder;
mod search;
mod specimen;
mod switch;
mod tabs_specimen;
mod theme_gallery;

/// Whether the keyboard is in a field of Settings ▸ Keyboard that listens for a shortcut: then
/// a modifier pressed twice is a shortcut to give, not one to run.
pub(crate) fn is_recording(mtm: MainThreadMarker) -> bool {
    recorder::is_listening(mtm)
}

use keyboard::KeyboardPage;

/// The window's opening content size, in points. The width is fixed, the
/// height is not: the pages sit in a vertical scroll view, so a page taller
/// than the window (Servers) or a screen shorter than the window scrolls
/// instead of clipping the rows. A design constant, not a measured number.
const WINDOW_SIZE: NSSize = NSSize::new(760.0, 700.0);
/// The shortest the window can be dragged: the head, a few rows and the
/// sidebar's button still fit. A design constant.
const MIN_WINDOW_HEIGHT: f64 = 360.0;
/// The sidebar's width: a search field and ten short titles beside their tiles. A design
/// constant.
const SIDEBAR_WIDTH: f64 = 212.0;
/// The detail side's width: what the window leaves beside the sidebar.
const DETAIL_WIDTH: f64 = WINDOW_SIZE.width - SIDEBAR_WIDTH;
/// The strip's outer margin.
const MARGIN: f64 = 20.0;
/// A page's margin to the window's edges, and the room under its last group.
const PAGE_MARGIN: f64 = 22.0;
const PAGE_BOTTOM: f64 = 28.0;
/// The head row: the page's name, level with the traffic lights.
const HEAD_HEIGHT: f64 = 52.0;
/// The sidebar's rows: the room at each side of a row's face, how far in the tile stands, the
/// face's corner and the tile's (the tab tile's own: `tab_bar`'s `TILE_ROUND`).
const SIDE_PAD: f64 = 10.0;
const SIDE_TILE_LEFT: f64 = 8.0;
const SIDE_ROW_RADIUS: f64 = 14.0;
const SIDE_TILE_RADIUS: f64 = 6.0;
/// The point size of the mark in a sidebar tile: drawn at its own size in the tile's middle, with
/// room round it, not scaled down to the tile's edges (a symbol fitted to the edges stands where its
/// own bounds put it, off the middle).
const SIDE_SYMBOL_SIZE: f64 = 11.0;
const SIDE_ROW_HEIGHT: f64 = 30.0;
const SIDE_GAP_HEIGHT: f64 = 10.0;
/// A group's box: its corner, the gap between two groups and between a title and its box.
const GROUP_RADIUS: f64 = 10.0;
const GROUP_SPACING: f64 = 20.0;
const TITLE_SPACING: f64 = 7.0;
/// A row: its padding, the gap between its text and its control, and its least height.
const ROW_PAD_X: f64 = 12.0;
const ROW_PAD_Y: f64 = 9.0;
const ROW_GAP: f64 = 14.0;
const ROW_MIN_HEIGHT: f64 = 40.0;
/// The text of a row — its name and description — stands in what is left of the row's width
/// beside its control; never narrower than this, whatever the control. Wrapped text is given its
/// width up front: the window's width is fixed, so it is a number and not a layout pass.
const ROW_WIDTH: f64 = DETAIL_WIDTH - 2.0 * PAGE_MARGIN - 2.0 * ROW_PAD_X;
const MIN_TEXT_WIDTH: f64 = 160.0;
/// The popups' common width: popups of different sizes in the same column
/// look scattered. The longest item ("Only when a program is running") fits.
const POPUP_WIDTH: f64 = 230.0;
/// A number field's width, and the least the unit after it takes.
const NUMBER_WIDTH: f64 = 64.0;
const UNIT_WIDTH: f64 = 18.0;
/// The sliders' width; the reading sits next to it.
const SLIDER_WIDTH: f64 = 170.0;
const SLIDE_READING_WIDTH: f64 = 48.0;
/// The strip's button, and the text's wrap width beside it: the detail side's width minus two
/// margins, the box's padding, the symbol and its spacing, the button and its gap.
const BANNER_BUTTON_WIDTH: f64 = 150.0;
const BANNER_TEXT_WIDTH: f64 =
    DETAIL_WIDTH - 2.0 * MARGIN - 2.0 * 12.0 - 16.0 - 8.0 - BANNER_BUTTON_WIDTH - 10.0;
/// A folder row's path label: the end of a path is what tells folders apart, so the head is
/// what is cut when it does not fit.
const PATH_WIDTH: f64 = 230.0;
/// The size popups' presets, bytes (the keys' starting values among
/// them). A design constant; a value the file holds that is not here is
/// still shown ([`size_items`]).
const PREVIEW_SIZE_PRESETS: &[u64] = &[
    10_000_000,
    50_000_000,
    100_000_000,
    500_000_000,
    1_000_000_000,
    5_000_000_000,
];
const PREVIEW_LIMIT_PRESETS: &[u64] = &[
    500_000_000,
    1_000_000_000,
    2_000_000_000,
    5_000_000_000,
    10_000_000_000,
    20_000_000_000,
];
/// The contrast popup's presets: off, then WCAG 2's three levels for text
/// (the key's default among them). A design constant; a value the file holds
/// that is not here is still shown ([`contrast_items`]).
const CONTRAST_PRESETS: &[f64] = &[1.0, 3.0, 4.5, 7.0];

/// The pages of the window, in the sidebar's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    General,
    Appearance,
    Font,
    Tabs,
    Cursor,
    Motion,
    Shell,
    Keyboard,
    Servers,
}

impl Category {
    const ALL: [Category; 9] = [
        Category::General,
        Category::Appearance,
        Category::Font,
        Category::Tabs,
        Category::Cursor,
        Category::Motion,
        Category::Shell,
        Category::Keyboard,
        Category::Servers,
    ];

    /// The page's stack in `panes`: the pages that have rows, in order; the Keyboard page is a
    /// table of its own.
    fn grid(self) -> Option<usize> {
        if self == Category::Keyboard {
            return None;
        }
        Self::ALL
            .iter()
            .filter(|category| **category != Category::Keyboard)
            .position(|category| *category == self)
    }

    fn title(self) -> &'static str {
        match self {
            Category::General => "General",
            Category::Appearance => "Appearance",
            Category::Font => "Font",
            Category::Tabs => "Tabs and Panes",
            Category::Cursor => "Cursor",
            Category::Motion => "Motion",
            Category::Shell => "Shell",
            Category::Keyboard => "Keyboard",
            Category::Servers => "Servers",
        }
    }

    /// SF Symbol name; if the symbol is not found the tile stays empty, the row stays.
    fn symbol(self) -> &'static str {
        match self {
            Category::General => "gearshape",
            Category::Appearance => "paintpalette",
            Category::Font => "textformat",
            Category::Tabs => "rectangle.split.2x1",
            Category::Cursor => "character.cursor.ibeam",
            Category::Motion => "wind",
            Category::Shell => "terminal",
            Category::Keyboard => "keyboard",
            Category::Servers => "network",
        }
    }
}

/// Which settings row a control is — the control's `tag`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    ConfirmClose,
    Clipboard,
    Scrollback,
    ShellIntegration,
    Theme,
    LightTheme,
    DarkTheme,
    Font,
    Size,
    LineHeight,
    LetterSpacing,
    FontFeatures,
    FontSmoothing,
    Shape,
    Blink,
    BlinkSpeed,
    Radius,
    Glow,
    Unfocused,
    CursorMotion,
    SmoothScroll,
    ScrollBounce,
    ReduceMotion,
    Keypress,
    Erase,
    DockArrival,
    PreviewMaxSize,
    PreviewReadOnly,
    PreviewDir,
    PreviewKeep,
    PreviewLimit,
    DownloadDir,
    DownloadConflict,
    DownloadNotify,
    RemoteStats,
    StatsInterval,
    RemoteIntegration,
    RestoreWindows,
    KeepRunning,
    Scrollbar,
    ContentEdge,
    Ports,
    MinimumContrast,
    DimSplits,
    FocusFrame,
    SplitStyle,
    TabDetails,
    TabIcons,
    TabIconColors,
    TabRunning,
    ResizeInSteps,
    TabPosition,
    PythonEnv,
    NodeVersion,
    CommandLevel,
}

impl Key {
    /// The order is the `tag` itself: `ALL[tag]`.
    const ALL: [Key; 55] = [
        Key::ConfirmClose,
        Key::Clipboard,
        Key::Scrollback,
        Key::ShellIntegration,
        Key::Theme,
        Key::LightTheme,
        Key::DarkTheme,
        Key::Font,
        Key::Size,
        Key::LineHeight,
        Key::LetterSpacing,
        Key::FontFeatures,
        Key::FontSmoothing,
        Key::Shape,
        Key::Blink,
        Key::BlinkSpeed,
        Key::Radius,
        Key::Glow,
        Key::Unfocused,
        Key::CursorMotion,
        Key::SmoothScroll,
        Key::ScrollBounce,
        Key::ReduceMotion,
        Key::Keypress,
        Key::Erase,
        Key::DockArrival,
        Key::PreviewMaxSize,
        Key::PreviewReadOnly,
        Key::PreviewDir,
        Key::PreviewKeep,
        Key::PreviewLimit,
        Key::DownloadDir,
        Key::DownloadConflict,
        Key::DownloadNotify,
        Key::RemoteStats,
        Key::StatsInterval,
        Key::RemoteIntegration,
        Key::RestoreWindows,
        Key::KeepRunning,
        Key::Scrollbar,
        Key::ContentEdge,
        Key::Ports,
        Key::MinimumContrast,
        Key::DimSplits,
        Key::FocusFrame,
        Key::SplitStyle,
        Key::TabDetails,
        Key::TabIcons,
        Key::TabIconColors,
        Key::TabRunning,
        Key::ResizeInSteps,
        Key::TabPosition,
        Key::PythonEnv,
        Key::NodeVersion,
        Key::CommandLevel,
    ];

    fn tag(self) -> NSInteger {
        self as NSInteger
    }

    fn from_tag(tag: NSInteger) -> Option<Key> {
        Self::ALL.get(usize::try_from(tag).ok()?).copied()
    }

    /// The dotted path in the file — `Diagnostic::key`'s language; the row's
    /// diagnostic is found by this mapping. A test holds its tie to the parser
    /// (`every_row_receives_its_own_diagnostic`).
    fn path(self) -> &'static str {
        match self {
            Key::ConfirmClose => "terminal.confirm_close",
            Key::RestoreWindows => "terminal.restore_windows",
            Key::KeepRunning => "terminal.keep_running",
            Key::Scrollbar => "terminal.scrollbar",
            Key::Clipboard => "clipboard.osc52",
            Key::Scrollback => "terminal.scrollback",
            Key::ShellIntegration => "shell.integration",
            Key::Theme => "appearance.theme",
            Key::LightTheme => "appearance.light_theme",
            Key::DarkTheme => "appearance.dark_theme",
            Key::ContentEdge => "appearance.content_edge",
            Key::MinimumContrast => "appearance.minimum_contrast",
            Key::DimSplits => "appearance.dim_unfocused_splits",
            Key::FocusFrame => "appearance.focus_frame",
            Key::SplitStyle => "appearance.split_style",
            Key::TabDetails => "appearance.tab_details",
            Key::ResizeInSteps => "appearance.resize_in_steps",
            Key::TabIcons => "appearance.tab_icons",
            Key::TabIconColors => "appearance.tab_icon_colors",
            Key::TabRunning => "appearance.tab_running",
            Key::TabPosition => "appearance.tab_position",
            Key::Font => "font.family",
            Key::Size => "font.size",
            Key::LineHeight => "font.line_height",
            Key::LetterSpacing => "font.letter_spacing",
            Key::FontFeatures => "font.features",
            Key::FontSmoothing => "font.smoothing",
            Key::Shape => "terminal.cursor",
            Key::Blink => "terminal.cursor_blink",
            Key::BlinkSpeed => "terminal.cursor_blink_interval",
            Key::Radius => "terminal.cursor_radius",
            Key::Glow => "terminal.cursor_glow",
            Key::Unfocused => "terminal.cursor_unfocused",
            Key::CursorMotion => "motion.cursor_motion",
            Key::SmoothScroll => "motion.smooth_scroll",
            Key::ScrollBounce => "motion.scroll_bounce",
            Key::ReduceMotion => "motion.reduce_motion",
            Key::Keypress => "motion.keypress",
            Key::Erase => "motion.erase",
            Key::DockArrival => "motion.dock_arrival",
            Key::PreviewMaxSize => "remote.preview_max_size",
            Key::PreviewReadOnly => "remote.preview_read_only",
            Key::PreviewDir => "remote.preview_dir",
            Key::PreviewKeep => "remote.preview_keep",
            Key::PreviewLimit => "remote.preview_limit",
            Key::DownloadDir => "remote.download_dir",
            Key::DownloadConflict => "remote.download_conflict",
            Key::DownloadNotify => "remote.download_notify",
            Key::RemoteStats => "remote.stats",
            Key::StatsInterval => "remote.stats_interval",
            Key::RemoteIntegration => "remote.integration",
            Key::Ports => "shell.ports",
            Key::PythonEnv => "shell.python_env",
            Key::NodeVersion => "shell.node_version",
            Key::CommandLevel => "commands.level",
        }
    }
}

/// The sentence under the reason in a locked window's banner.
const LOCK_HINT: &str = "Fix the file and save it; this window follows.";

/// The strip above the page; invisible if it has no lines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Banner {
    /// What happened, in a word or two, above the lines.
    title: Option<&'static str>,
    /// The subtitle's texts, verbatim: a write error, the lock's reason, a
    /// diagnostic that lands on no row.
    lines: Vec<String>,
    /// Below it, in the secondary colour: what to do.
    hint: Option<&'static str>,
    /// The button the strip carries, if any.
    action: BannerAction,
}

/// The button of the strip: what the user can do about it right there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum BannerAction {
    #[default]
    None,
    /// The file cannot be read: opening it is one click — Enter — away.
    OpenFile,
    /// The file cannot be written: show where it is.
    ShowInFinder,
}

/// The title of a locked window's strip, and of the one for a refused write.
const LOCK_TITLE: &str = "settings.toml can\u{2019}t be read";
const WRITE_TITLE: &str = "Changes can\u{2019}t be saved";

/// What the window sees from the file's state — pure, so the
/// three states are tested without building a window.
#[derive(Debug, PartialEq, Eq)]
struct Status {
    /// All controls disabled; the strip's "Open settings.toml" is the default button.
    locked: bool,
    banner: Banner,
    /// Values that were not accepted: row → the diagnostic's message (in place
    /// of the note). No line number or file name — the row itself is the context.
    rows: Vec<(Key, String)>,
}

/// File state + write slot → what the window will see.
///
/// A diagnostic that lands on no row (a nonexistent section, a retired key)
/// goes to the banner: a diagnostic visible in the subtitle but not in the
/// window would force the user to look in two places. A write error is at the
/// head of the banner, because it is the answer to what the user just did
/// (the subtitle's order).
fn status(state: &FileState, write: &[String]) -> Status {
    let mut banner = Banner {
        lines: write.to_vec(),
        ..Banner::default()
    };
    let mut rows = Vec::new();
    let locked = match state {
        FileState::Missing => false,
        FileState::Locked(reason) => {
            banner.lines.push(reason.clone());
            banner.hint = Some(LOCK_HINT);
            banner.title = Some(LOCK_TITLE);
            banner.action = BannerAction::OpenFile;
            true
        }
        FileState::Usable(diagnostics) => {
            for diagnostic in diagnostics {
                let key = diagnostic
                    .key
                    .and_then(|path| Key::ALL.into_iter().find(|key| key.path() == path));
                match key {
                    Some(key) => rows.push((key, diagnostic.message.clone())),
                    None => banner.lines.push(settings::notice(diagnostic)),
                }
            }
            false
        }
    };
    if !locked && !write.is_empty() {
        banner.title = Some(WRITE_TITLE);
        banner.action = BannerAction::ShowInFinder;
    }
    Status {
        locked,
        banner,
        rows,
    }
}

/// A string enum chosen with a popup: the items from `bt-core`'s spelling
/// table (`NAMES`, order included), the titles from the exhaustive `match`
/// here. The title is a UI string, the spelling is the file's vocabulary — the two are separate.
trait Choice: Copy + PartialEq + 'static {
    fn names() -> &'static [(&'static str, Self)];
    fn title(self) -> &'static str;
}

impl Choice for ConfirmClose {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            ConfirmClose::Never => "Never",
            ConfirmClose::Running => "Only when a program is running",
            ConfirmClose::Always => "Always",
        }
    }
}

impl Choice for RestoreWindows {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            RestoreWindows::All => "Windows and scrollback",
            RestoreWindows::Layout => "Windows only",
            RestoreWindows::Off => "Nothing",
        }
    }
}

impl Choice for KeepRunning {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            KeepRunning::Update => "Only during updates",
            KeepRunning::Crash => "Also after a crash",
            KeepRunning::Quit => "Also after quitting",
        }
    }
}

/// A choice that is shown as pictures: what each option looks like.
trait Pictured: Choice {
    fn picture(self) -> Picture;
}

impl Pictured for ContentEdge {
    fn picture(self) -> Picture {
        match self {
            ContentEdge::Fade => Picture::EdgeFade,
            ContentEdge::Line => Picture::EdgeLine,
            ContentEdge::Cut => Picture::EdgeCut,
        }
    }
}

impl Pictured for TabPosition {
    fn picture(self) -> Picture {
        match self {
            TabPosition::Top => Picture::TabsTop,
            TabPosition::Side => Picture::TabsSide,
        }
    }
}

impl Pictured for SplitStyle {
    fn picture(self) -> Picture {
        match self {
            SplitStyle::Cards => Picture::Cards,
            SplitStyle::Lines => Picture::Lines,
        }
    }
}

impl Pictured for CaretShape {
    fn picture(self) -> Picture {
        match self {
            CaretShape::Block => Picture::Block,
            CaretShape::Underline => Picture::Underline,
            CaretShape::Beam => Picture::Beam,
        }
    }
}

impl Pictured for UnfocusedCaret {
    fn picture(self) -> Picture {
        match self {
            UnfocusedCaret::Hollow => Picture::Hollow,
            UnfocusedCaret::Solid => Picture::Solid,
        }
    }
}

impl Pictured for TabRunning {
    fn picture(self) -> Picture {
        match self {
            TabRunning::Tail => Picture::Tail,
            TabRunning::TailRing => Picture::TailRing,
            TabRunning::Breathe => Picture::Breathe,
        }
    }
}

/// The description under "Keep programs running" for the file's value: under
/// `quit` it says, for as long as the value stands, that quitting leaves the
/// programs behind and how to get back to them or end them — the reminder
/// that does not depend on a notification permission. Pure.
fn keep_running_note(keep: KeepRunning) -> &'static str {
    match keep {
        KeepRunning::Update | KeepRunning::Crash => "Restarting the computer ends them.",
        KeepRunning::Quit => {
            "Programs keep running after you quit; open bateri to return to them, \u{2325}\u{2318}Q \
             ends them. Restarting the computer ends them."
        }
    }
}

impl Choice for CommandLevel {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            CommandLevel::Off => "Do nothing",
            CommandLevel::Own => "Use their own panes",
            CommandLevel::Screen => "Use the whole screen",
            CommandLevel::Full => "Use everything",
        }
    }
}

/// The description under "Programs on this computer may" for the file's value: what a program
/// in a terminal of this computer may do with the bateri command. A server has its own level in
/// the `[remote]` hosts entry.
fn command_level_note(level: CommandLevel) -> &'static str {
    match level {
        CommandLevel::Off => "The bateri command does nothing.",
        CommandLevel::Own => "They see and change only this computer\u{2019}s panes.",
        CommandLevel::Screen => "They see and change every tab, group and window bateri has open.",
        CommandLevel::Full => {
            "They also start terminals on other machines and type into or read their panes. A server given this is trusted with your other machines too."
        }
    }
}

impl Choice for Scrollbar {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            Scrollbar::System => "Follow System Settings",
            Scrollbar::Auto => "When scrolling",
            Scrollbar::Always => "Always",
            Scrollbar::Never => "Never",
        }
    }
}

impl Choice for SplitStyle {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            SplitStyle::Cards => "Cards",
            SplitStyle::Lines => "Lines",
        }
    }
}

impl Choice for TabDetails {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            TabDetails::Off => "Off",
            TabDetails::Smart => "Smart",
            TabDetails::Always => "Always",
        }
    }
}

impl Choice for TabPosition {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            TabPosition::Top => "Top",
            TabPosition::Side => "Side",
        }
    }
}

impl Choice for IconColors {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            IconColors::Theme => "Theme",
            IconColors::Brand => "Brand",
        }
    }
}

impl Choice for TabRunning {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            TabRunning::Tail => "Tail",
            TabRunning::TailRing => "Tail and ring",
            TabRunning::Breathe => "Breathe",
        }
    }
}

impl Choice for ContentEdge {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            ContentEdge::Fade => "Fade",
            ContentEdge::Line => "Line",
            ContentEdge::Cut => "Cut",
        }
    }
}

/// The description under "Scroll bar": under "Follow System Settings" it says what
/// the system's preference gives **right now** (`resolved`, the setting and
/// the system merged), in the popup's own words — "Automatically based on
/// mouse or trackpad" resolves by device, so the choice in System Settings
/// alone would not tell. Nothing under the other values: they say it
/// themselves. Pure.
fn scrollbar_note(setting: Scrollbar, resolved: ScrollbarMode) -> Option<&'static str> {
    match (setting, resolved) {
        (Scrollbar::System, ScrollbarMode::Auto) => {
            Some("Right now that is When scrolling, from System Settings \u{203a} Appearance.")
        }
        (Scrollbar::System, ScrollbarMode::Always) => {
            Some("Right now that is Always, from System Settings \u{203a} Appearance.")
        }
        _ => None,
    }
}

/// The resolved answers some rows explain — the setting merged with the
/// system's (`app`'s resolvers). In the window they decide notes and
/// overrides, never a value: the value shown is always the file's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Resolved {
    /// Reduce Motion's resolved answer ([`motion_override`]).
    pub(crate) reduce: bool,
    /// The scroll bar's resolved form ([`scrollbar_note`]).
    pub(crate) scrollbar: ScrollbarMode,
}

/// A row's description: the one sentence under its name, chosen on every refresh from the
/// settings the window shows (and, for "Scroll bar" and "Reduce motion", what the system resolves
/// them to). It tells what the **selected** value does, never the list of them all, and a value
/// that explains itself has none. The window keeps no state of its own.
fn row_description(key: Key, settings: &Settings, resolved: Resolved) -> Option<&'static str> {
    match key {
        Key::RestoreWindows => Some(match settings.restore_windows {
            RestoreWindows::All => {
                "Windows, tabs and splits come back with what each pane showed. Scrollback is saved as plain text on disk."
            }
            RestoreWindows::Layout => {
                "Windows, tabs and splits come back. Nothing you saw is written to disk."
            }
            RestoreWindows::Off => "bateri opens with a new window and deletes what was saved.",
        }),
        Key::KeepRunning => Some(keep_running_note(settings.keep_running)),
        Key::ConfirmClose => match settings.confirm_close {
            ConfirmClose::Running => {
                Some("Asks while a program other than the shell runs, and names it.")
            }
            ConfirmClose::Always => Some("Asks even at an empty prompt."),
            ConfirmClose::Never => None,
        },
        Key::Clipboard => Some("Also over ssh (OSC 52)."),
        Key::MinimumContrast => Some(
            "Text a program colors too close to its background is drawn darker or lighter until it reads.",
        ),
        Key::ContentEdge => Some(match settings.content_edge {
            ContentEdge::Fade => "The line scrolling up thins out, so no letter is cut in half.",
            // The line stands only where the panes meet the tab bar's edge: under a bar at the
            // top, with panes as lines (`split_view`'s hairline). Elsewhere Line is Cut, and the
            // row says so rather than promising a line nobody sees.
            ContentEdge::Line if settings.split_style == SplitStyle::Cards => {
                "Same as Cut while panes are cards: the thin line is drawn only with panes as lines."
            }
            ContentEdge::Line if settings.tab_position == TabPosition::Side => {
                "Same as Cut while tabs are at the side: there is no tab bar above the text for the line."
            }
            ContentEdge::Line => "Text is cut under a thin line along the tab bar.",
            ContentEdge::Cut => "Text is cut where the pane ends.",
        }),
        Key::Scrollbar => scrollbar_note(settings.scrollbar, resolved.scrollbar),
        Key::ResizeInSteps => Some(
            "While you drag a window\u{2019}s edge, so the text keeps the same margin on both sides.",
        ),
        Key::FontSmoothing => Some("Thickens the strokes the way Terminal does."),
        Key::TabPosition => Some(match settings.tab_position {
            TabPosition::Side => {
                "A column down the left edge, tall as the window; the title row goes."
            }
            TabPosition::Top => "A bar along the top of the window.",
        }),
        Key::TabIcons => Some(if settings.tab_icons {
            "The program a tab runs, on a tile before its title."
        } else {
            "Only a small mark while a tab works, waits or has finished."
        }),
        Key::TabIconColors => Some(match settings.tab_icon_colors {
            IconColors::Brand => "Programs with a colored brand (Claude, Gemini) keep it.",
            IconColors::Theme => "Icons take the theme\u{2019}s colors.",
        }),
        Key::TabRunning => {
            Some("The line round the tab\u{2019}s icon. Still while Reduce motion is on.")
        }
        Key::TabDetails => match settings.tab_details {
            TabDetails::Off => None,
            TabDetails::Smart => {
                Some("Where the program stands and what it asks, in Activity only.")
            }
            TabDetails::Always => {
                Some("Where the program stands and what it asks, under every tab.")
            }
        },
        Key::SplitStyle => Some(match settings.split_style {
            SplitStyle::Lines => "Panes touch, with one thin line between them.",
            SplitStyle::Cards => "Each pane stands on the ground with a gap round it.",
        }),
        Key::FocusFrame => Some("A line in the theme\u{2019}s accent."),
        Key::Shape => Some("Programs like vim can change it while they run."),
        Key::ScrollBounce => Some(match settings.scroll_bounce {
            ScrollBounce::On => "Lists in a program's view spring back past their ends.",
            ScrollBounce::Off => "Lists in a program's view stop at their ends.",
        }),
        Key::Blink => match settings.cursor_blink {
            CursorBlink::Auto => Some("Blinks until the running program asks it to stop."),
            CursorBlink::On => Some("Blinks whatever the program asks."),
            CursorBlink::Off => None,
        },
        Key::CursorMotion => Some(match settings.cursor_motion {
            CursorMotion::Spring => "Travels to its new place with a little give.",
            CursorMotion::Ease => "Glides to its new place.",
            CursorMotion::Snap => "Jumps. Typing and deleting effects are off too.",
        }),
        Key::ReduceMotion => match settings.reduce_motion {
            ReduceMotion::System if resolved.reduce => {
                Some("On right now, from System Settings \u{203a} Accessibility.")
            }
            ReduceMotion::System => {
                Some("Off right now, from System Settings \u{203a} Accessibility.")
            }
            ReduceMotion::On => Some("Animations become fades and instant jumps."),
            ReduceMotion::Off => None,
        },
        Key::ShellIntegration => Some(match settings.shell_integration {
            ShellIntegration::Auto => {
                "bateri draws the prompt and the line you type sits in the dock. New tabs only."
            }
            ShellIntegration::Blocks => {
                "Commands become blocks; the prompt and the line stay your shell\u{2019}s. New tabs only."
            }
            ShellIntegration::Off => "bateri sets nothing up in your shell. New tabs only.",
        }),
        Key::PythonEnv => Some("As activate prints it."),
        Key::NodeVersion => {
            Some("In the warning color when the project asks for another (.nvmrc).")
        }
        Key::Ports => Some("Click to list them, \u{2318}-click to open one."),
        Key::CommandLevel => Some(command_level_note(settings.commands_level)),
        Key::RemoteIntegration => {
            Some("Not on servers marked Production, unless the Shell menu turns it on.")
        }
        Key::RemoteStats => match settings.remote_stats.mode {
            RemoteStatsMode::Sparkline => {
                Some("CPU and memory as a line and numbers, in the ssh bar.")
            }
            RemoteStatsMode::Numbers => Some("CPU and memory as numbers, in the ssh bar."),
            RemoteStatsMode::Alerts => Some("Nothing until a value passes its threshold."),
            RemoteStatsMode::Off => None,
        },
        Key::StatsInterval => Some("Pauses in hidden tabs and while idle."),
        Key::PreviewReadOnly => Some("A preview you change is kept, never cleaned up."),
        Key::PreviewLimit => Some("Oldest go first, when bateri starts."),
        Key::DownloadNotify => Some("Only while bateri is in the background."),
        Key::Theme if settings.theme == SYSTEM_THEME => {
            Some("The light theme while the system is light, the dark one while it is dark.")
        }
        Key::Theme => Some("The same theme in light and dark."),
        Key::Scrollback
        | Key::LightTheme
        | Key::DarkTheme
        | Key::Font
        | Key::Size
        | Key::LineHeight
        | Key::LetterSpacing
        | Key::FontFeatures
        | Key::DimSplits
        | Key::BlinkSpeed
        | Key::Radius
        | Key::Glow
        | Key::Unfocused
        | Key::Keypress
        | Key::Erase
        | Key::DockArrival
        | Key::SmoothScroll
        | Key::PreviewMaxSize
        | Key::PreviewDir
        | Key::PreviewKeep
        | Key::DownloadDir
        | Key::DownloadConflict => None,
    }
}

impl Choice for ShellIntegration {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            ShellIntegration::Auto => "Auto",
            ShellIntegration::Blocks => "Blocks only",
            ShellIntegration::Off => "Off",
        }
    }
}

impl Choice for CaretShape {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            CaretShape::Block => "Block",
            CaretShape::Underline => "Underline",
            CaretShape::Beam => "Beam",
        }
    }
}

impl Choice for CursorBlink {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            CursorBlink::Auto => "Follow program",
            CursorBlink::On => "On",
            CursorBlink::Off => "Off",
        }
    }
}

impl Choice for UnfocusedCaret {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            UnfocusedCaret::Hollow => "Hollow",
            UnfocusedCaret::Solid => "Solid",
        }
    }
}

impl Choice for CursorMotion {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            CursorMotion::Snap => "Snap",
            CursorMotion::Ease => "Ease",
            CursorMotion::Spring => "Spring",
        }
    }
}

impl Choice for ReduceMotion {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            ReduceMotion::System => "Match System",
            ReduceMotion::On => "On",
            ReduceMotion::Off => "Off",
        }
    }
}

impl Choice for Keypress {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            Keypress::Off => "Off",
            Keypress::Fade => "Fade",
            Keypress::Rise => "Rise",
            Keypress::Pop => "Pop",
            Keypress::Extrude => "Extrude",
            Keypress::Heat => "Heat",
            Keypress::Echo => "Echo",
            Keypress::Drop => "Drop",
            Keypress::Ink => "Ink",
            Keypress::Squeeze => "Squeeze",
        }
    }
}

impl Choice for Erase {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            Erase::Off => "Off",
            Erase::Iris => "Iris",
            Erase::Undertow => "Undertow",
            Erase::Echo => "Echo",
            Erase::Bleed => "Bleed",
            Erase::Unravel => "Unravel",
            Erase::Recede => "Recede",
            Erase::Sublime => "Sublime",
            Erase::Shatter => "Shatter",
        }
    }
}

impl Choice for DockArrival {
    fn names() -> &'static [(&'static str, Self)] {
        Self::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            DockArrival::Off => "Off",
            DockArrival::Type => "Type",
            DockArrival::Dust => "Dust",
            DockArrival::Ripple => "Ripple",
        }
    }
}

impl Choice for PreviewKeep {
    fn names() -> &'static [(&'static str, Self)] {
        PreviewKeep::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            PreviewKeep::UntilLaunch => "Until next launch",
            PreviewKeep::Day => "1 day",
            PreviewKeep::Week => "7 days",
            PreviewKeep::Month => "30 days",
        }
    }
}

impl Choice for DownloadConflict {
    fn names() -> &'static [(&'static str, Self)] {
        DownloadConflict::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            DownloadConflict::Ask => "Ask",
            DownloadConflict::KeepBoth => "Keep both",
            DownloadConflict::Replace => "Replace",
        }
    }
}

impl Choice for RemoteStatsMode {
    fn names() -> &'static [(&'static str, Self)] {
        RemoteStatsMode::NAMES
    }
    fn title(self) -> &'static str {
        match self {
            RemoteStatsMode::Sparkline => "Sparkline",
            RemoteStatsMode::Numbers => "Numbers",
            RemoteStatsMode::Alerts => "Alerts only",
            RemoteStatsMode::Off => "Off",
        }
    }
}

/// A row overridden by an input that turns motion off: whether it is enabled
/// and what it says in place of its note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Override {
    enabled: bool,
    note: &'static str,
}

/// Whether `cursor_motion = "snap"` or an **active** Reduce Motion (`reduce`,
/// the setting and the system's merged answer) overrides this row's value —
/// pure, so the rule is tested without building a window.
///
/// An overridden row is not hidden, it is left **disabled** and says why:
/// the value stays visible and comes back when the overriding input goes
/// away. The rule is that of the reduction's owners (`bt_gpu`'s
/// `Motion::glyph_fx`, `app::resolve_smooth_scroll`): `snap` is above both;
/// Reduce Motion turns off the ghost and smooth scrolling, while it
/// **reduces** typing to a fade-in — that row stays enabled, because the
/// choice between `off` and an effect is a difference there too, and since
/// the disabled typing stays disabled there is nothing to say there.
fn motion_override(key: Key, settings: &Settings, reduce: bool) -> Option<Override> {
    let disabled = |note| {
        Some(Override {
            enabled: false,
            note,
        })
    };
    let snap = settings.cursor_motion == CursorMotion::Snap;
    match key {
        Key::Keypress | Key::Erase | Key::DockArrival if snap => {
            disabled("Off while Cursor motion is Snap.")
        }
        Key::SmoothScroll if snap => disabled("Line by line while Cursor motion is Snap."),
        Key::Erase if reduce => disabled("Off while Reduce motion is on."),
        Key::SmoothScroll if reduce => disabled("Line by line while Reduce motion is on."),
        Key::ScrollBounce if reduce => disabled("Stops at the ends while Reduce motion is on."),
        Key::Keypress if reduce && settings.keypress != Keypress::Off => Some(Override {
            enabled: true,
            note: "Letters only fade in while Reduce motion is on.",
        }),
        Key::DockArrival if reduce && settings.dock_arrival != DockArrival::Off => Some(Override {
            enabled: true,
            note: "Fades in while Reduce motion is on.",
        }),
        _ => None,
    }
}

/// The popup's item titles, in `NAMES` order.
fn choice_titles<T: Choice>() -> Vec<&'static str> {
    T::names().iter().map(|&(_, value)| value.title()).collect()
}

/// The names of `key`'s choices, for the search; none for a key whose control is not a list of
/// names (a switch, a number, the lists the machine fills).
fn choice_labels(key: Key) -> Vec<&'static str> {
    match key {
        Key::ConfirmClose => choice_titles::<ConfirmClose>(),
        Key::RestoreWindows => choice_titles::<RestoreWindows>(),
        Key::KeepRunning => choice_titles::<KeepRunning>(),
        Key::Scrollbar => choice_titles::<Scrollbar>(),
        Key::ContentEdge => choice_titles::<ContentEdge>(),
        Key::SplitStyle => choice_titles::<SplitStyle>(),
        Key::TabDetails => choice_titles::<TabDetails>(),
        Key::TabIconColors => choice_titles::<IconColors>(),
        Key::TabRunning => choice_titles::<TabRunning>(),
        Key::TabPosition => choice_titles::<TabPosition>(),
        Key::ShellIntegration => choice_titles::<ShellIntegration>(),
        Key::CommandLevel => choice_titles::<CommandLevel>(),
        Key::Shape => choice_titles::<CaretShape>(),
        Key::Blink => choice_titles::<CursorBlink>(),
        Key::Unfocused => choice_titles::<UnfocusedCaret>(),
        Key::CursorMotion => choice_titles::<CursorMotion>(),
        Key::ReduceMotion => choice_titles::<ReduceMotion>(),
        Key::Keypress => choice_titles::<Keypress>(),
        Key::Erase => choice_titles::<Erase>(),
        Key::DockArrival => choice_titles::<DockArrival>(),
        Key::PreviewKeep => choice_titles::<PreviewKeep>(),
        Key::DownloadConflict => choice_titles::<DownloadConflict>(),
        Key::RemoteStats => choice_titles::<RemoteStatsMode>(),
        _ => Vec::new(),
    }
}

/// The selected item's variant; `-1` (no selection) or an overflowing index is `None`.
fn choice_at<T: Choice>(index: NSInteger) -> Option<T> {
    let index = usize::try_from(index).ok()?;
    T::names().get(index).map(|&(_, value)| value)
}

/// The variant's item index.
fn choice_index<T: Choice>(value: T) -> Option<usize> {
    T::names()
        .iter()
        .position(|&(_, candidate)| candidate == value)
}

/// A switch for two-valued settings: on ↔ `copy`.
fn osc52_on(mode: Osc52) -> bool {
    match mode {
        Osc52::Copy => true,
        Osc52::Off => false,
    }
}

fn smooth_on(smooth: SmoothScroll) -> bool {
    match smooth {
        SmoothScroll::On => true,
        SmoothScroll::Off => false,
    }
}

fn bounce_on(bounce: ScrollBounce) -> bool {
    match bounce {
        ScrollBounce::On => true,
        ScrollBounce::Off => false,
    }
}

/// The blink-speed slider's position (`0..=1`, right is **fast**, i.e. a
/// short half period) ↔ half period, in seconds. The scale is
/// **logarithmic**: the range is a hundredfold (`CURSOR_BLINK_RANGE`) and on
/// a linear scale all the useful values would be squeezed into the left's
/// first percent. The ends return explicitly, because
/// `exp(ln(x))` is not bit for bit `x` and the ends being the range's ends
/// is the contract.
fn blink_from_position(position: f64) -> f64 {
    let (min, max) = (*CURSOR_BLINK_RANGE.start(), *CURSOR_BLINK_RANGE.end());
    let t = position.clamp(0.0, 1.0);
    if t <= 0.0 {
        return max;
    }
    if t >= 1.0 {
        return min;
    }
    (max.ln() - t * (max.ln() - min.ln())).exp().clamp(min, max)
}

fn blink_to_position(seconds: f64) -> f64 {
    let (min, max) = (*CURSOR_BLINK_RANGE.start(), *CURSOR_BLINK_RANGE.end());
    let seconds = seconds.clamp(min, max);
    ((max.ln() - seconds.ln()) / (max.ln() - min.ln())).clamp(0.0, 1.0)
}

/// A decimal to two places, without trailing zeros (`0.5`, `1.25`, `13`) —
/// the same precision as what is written to the file (`SettingsEdit`'s two places).
fn decimal_label(value: f64) -> String {
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    text.to_owned()
}

/// The `scrollback` the field accepts: an integer, `0..=SCROLLBACK_MAX`.
fn parse_scrollback(text: &str) -> Option<usize> {
    text.trim()
        .parse::<usize>()
        .ok()
        .filter(|&lines| lines <= SCROLLBACK_MAX)
}

/// The `stats_interval` the field accepts: a whole number of seconds inside
/// [`STATS_INTERVAL_RANGE`] — the parser's own rule (a decimal is refused, not
/// rounded).
fn parse_interval(text: &str) -> Option<u8> {
    text.trim()
        .parse::<u8>()
        .ok()
        .filter(|seconds| STATS_INTERVAL_RANGE.contains(seconds))
}

/// The decimal the field accepts: finite and inside the range.
fn parse_decimal(text: &str, range: std::ops::RangeInclusive<f64>) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && range.contains(value))
}

/// Which themes a gallery offers: all of them (one theme in light and dark), or the light or the
/// dark ones (the pair that follows the system).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shade {
    All,
    Light,
    Dark,
}

impl Shade {
    fn takes(self, theme: &Theme) -> bool {
        match self {
            Shade::All => true,
            Shade::Light => !theme.is_dark(),
            Shade::Dark => theme.is_dark(),
        }
    }
}

/// A gallery's sections for `shade`: bateri's own themes, then the adapted dark and light ones
/// (each group's own order kept), then the user's, each under its title; a theme is light or dark
/// by its own colours ([`Theme::is_dark`]). If the name in the file is in none of them it gets a
/// card of its own: a theme of the other shade under "Chosen", in its colours, a deleted one under
/// "Not found", in the colours on screen — the gallery does not hide what the user wrote (the
/// Font popup's rule).
fn theme_sections(selected: &str, shade: Shade, lists: &ThemeLists) -> Vec<theme_gallery::Section> {
    let card = |name: &str, theme: &Theme| theme_gallery::Card {
        name: name.to_owned(),
        title: Theme::embedded_title(name).unwrap_or(name).to_owned(),
        palette: Palette::of(theme),
    };
    let embedded = |names: &[&str]| -> Vec<theme_gallery::Card> {
        names
            .iter()
            .filter_map(|name| Theme::embedded(name).map(|theme| (*name, theme)))
            .filter(|(_, theme)| shade.takes(theme))
            .map(|(name, theme)| card(name, &theme))
            .collect()
    };
    let [own, dark, light] = [0, 1, 2].map(|at| {
        lists
            .embedded
            .get(at)
            .map(|names| embedded(names))
            .unwrap_or_default()
    });
    let user: Vec<theme_gallery::Card> = lists
        .user
        .iter()
        .zip(lists.user_themes)
        .filter(|(_, theme)| shade.takes(theme))
        .map(|(name, theme)| card(name, theme))
        .collect();
    // Under the Light or the Dark row the adapted themes need no shade in their title: the row
    // says it.
    let (dark_title, light_title) = match shade {
        Shade::All => ("Dark", "Light"),
        Shade::Light | Shade::Dark => ("Well known", "Well known"),
    };
    let mut sections: Vec<theme_gallery::Section> = [
        ("bateri", own),
        (dark_title, dark),
        (light_title, light),
        ("Your themes", user),
    ]
    .into_iter()
    .filter(|(_, cards)| !cards.is_empty())
    .map(|(title, cards)| theme_gallery::Section {
        title: Some(title.to_owned()),
        cards,
    })
    .collect();
    let listed = sections
        .iter()
        .flat_map(|section| &section.cards)
        .any(|card| card.name == selected);
    if !listed && selected != SYSTEM_THEME {
        // A theme of the other shade the file names for this one keeps its colours; a name no
        // list has is drawn in the colours on screen.
        let known = Theme::embedded(selected).or_else(|| {
            lists
                .user
                .iter()
                .position(|name| name == selected)
                .and_then(|at| lists.user_themes.get(at).copied())
        });
        let (title, theme) = match known {
            Some(theme) => ("Chosen", theme),
            None => ("Not found", *lists.on_screen),
        };
        sections.push(theme_gallery::Section {
            title: Some(title.to_owned()),
            cards: vec![card(selected, &theme)],
        });
    }
    sections
}

/// An item of the font popup.
#[derive(Clone, Debug, PartialEq, Eq)]
enum FontItem {
    /// The chain: `family = ""`.
    Default,
    Separator,
    /// A line of text that is not a choice: the title of the section under it.
    Header(&'static str),
    Family(String),
    /// A family that is in the file but not in the list; choosing it writes nothing (it already is).
    Missing(String),
}

/// The font popup's items and the selected one's index: the default, then the families that ship
/// with the app under "Included with bateri" and the rest under "On this computer", a separator
/// before each; a section nobody fills has no heading. Matching is case insensitive: the chain
/// finds the name that way too (`bt-atlas`'s `same_family`), so `family = "menlo"` is the listed
/// `Menlo`.
fn font_items(
    current: Option<&str>,
    families: &[String],
    bundled: &[String],
) -> (Vec<FontItem>, usize) {
    let ships = |name: &String| {
        bundled
            .iter()
            .any(|own| own.to_lowercase() == name.to_lowercase())
    };
    let (included, local): (Vec<&String>, Vec<&String>) =
        families.iter().partition(|name| ships(name));
    let mut items = vec![FontItem::Default];
    for (title, names) in [
        ("Included with bateri", included),
        ("On this computer", local),
    ] {
        if names.is_empty() {
            continue;
        }
        items.push(FontItem::Separator);
        items.push(FontItem::Header(title));
        items.extend(names.into_iter().cloned().map(FontItem::Family));
    }
    let Some(current) = current else {
        return (items, 0);
    };
    let wanted = current.to_lowercase();
    if let Some(index) = items
        .iter()
        .position(|item| matches!(item, FontItem::Family(name) if name.to_lowercase() == wanted))
    {
        return (items, index);
    }
    items.push(FontItem::Separator);
    items.push(FontItem::Missing(current.to_owned()));
    let index = items.len() - 1;
    (items, index)
}

/// The title of a family not in the list: the name and what the chain will say for it.
fn missing_font_title(name: &str, notice: Option<FontNotice>) -> String {
    match notice {
        Some(FontNotice::FamilyNotFound { .. }) => format!("{name} — not found"),
        Some(FontNotice::NotMonospaced { .. }) => format!("{name} — not monospaced"),
        // The family question never answers a feature, but the type can.
        Some(FontNotice::NoFeature { .. }) | None => name.to_owned(),
    }
}

/// An item of a size popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SizeItem {
    Preset(u64),
    Separator,
    /// The file's value that is not a preset; choosing it writes nothing (it already is).
    Current(u64),
}

/// A size popup's items and the selected one's index: the presets, and the
/// file's value appended after a separator when it is not one of them — the
/// popup does not hide what the user wrote (the Font popup's rule).
fn size_items(current: u64, presets: &[u64]) -> (Vec<SizeItem>, usize) {
    let mut items: Vec<SizeItem> = presets.iter().copied().map(SizeItem::Preset).collect();
    if let Some(index) = presets.iter().position(|&bytes| bytes == current) {
        return (items, index);
    }
    items.push(SizeItem::Separator);
    items.push(SizeItem::Current(current));
    let index = items.len() - 1;
    (items, index)
}

/// The action's size item → the bytes to write.
fn size_edit(items: &[SizeItem], index: NSInteger) -> Option<u64> {
    match items.get(usize::try_from(index).ok()?)? {
        SizeItem::Preset(bytes) => Some(*bytes),
        SizeItem::Separator | SizeItem::Current(_) => None,
    }
}

/// A size as the popup shows it: the file's spelling (`bt_core::format_size`,
/// so the title is the value written) with a space before the unit — `100 MB`.
fn size_title(bytes: u64) -> String {
    let text = bt_core::format_size(bytes);
    match text.find(|c: char| !c.is_ascii_digit()) {
        Some(at) => format!("{} {}", &text[..at], &text[at..]),
        None => text,
    }
}

/// An item of the contrast popup — [`SizeItem`]'s shape for a ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ContrastItem {
    Preset(f64),
    Separator,
    /// The file's value that is not a preset; choosing it writes nothing (it already is).
    Current(f64),
}

/// The contrast popup's items and the selected one's index: the presets, and
/// the file's value after a separator when it is not one of them —
/// [`size_items`]'s rule. Exact comparison is sound: the presets are exact in
/// binary and the window writes two decimals, which a preset reads back as
/// itself.
fn contrast_items(current: f64) -> (Vec<ContrastItem>, usize) {
    let mut items: Vec<ContrastItem> = CONTRAST_PRESETS
        .iter()
        .copied()
        .map(ContrastItem::Preset)
        .collect();
    if let Some(index) = CONTRAST_PRESETS.iter().position(|&ratio| ratio == current) {
        return (items, index);
    }
    items.push(ContrastItem::Separator);
    items.push(ContrastItem::Current(current));
    let index = items.len() - 1;
    (items, index)
}

/// The action's contrast item → the ratio to write.
fn contrast_edit(items: &[ContrastItem], index: NSInteger) -> Option<f64> {
    match items.get(usize::try_from(index).ok()?)? {
        ContrastItem::Preset(ratio) => Some(*ratio),
        ContrastItem::Separator | ContrastItem::Current(_) => None,
    }
}

/// A ratio as the popup shows it: the presets by name with their ratio, so
/// the file's spelling stays readable from the window; any other value as the
/// ratio alone.
fn contrast_title(ratio: f64) -> String {
    let name = match ratio {
        1.0 => return "Off".to_owned(),
        3.0 => "Standard",
        4.5 => "Strong",
        7.0 => "Strongest",
        _ => return format!("{}:1", decimal_label(ratio)),
    };
    format!("{name} ({}:1)", decimal_label(ratio))
}

/// A folder the panel chose → the file's spelling: under the home directory
/// as `~/…` (the template's convention, and the file stays valid on another
/// account), anywhere else absolute.
fn folder_text(path: &Path, home: Option<&Path>) -> String {
    if let Some(rest) = home.and_then(|home| path.strip_prefix(home).ok()) {
        let rest = rest.to_string_lossy();
        return if rest.is_empty() {
            "~".to_owned()
        } else {
            format!("~/{rest}")
        };
    }
    path.to_string_lossy().into_owned()
}

/// The "In use" row: the preview copies' total and count; `None` until the
/// first measurement arrives (or the folder cannot be resolved).
fn usage_label(usage: Option<(u64, usize)>) -> String {
    match usage {
        None => "—".to_owned(),
        Some((_, 0)) => "Empty".to_owned(),
        Some((bytes, 1)) => format!("{} · 1 file", format_bytes(bytes)),
        Some((bytes, files)) => format!("{} · {files} files", format_bytes(bytes)),
    }
}

/// A folder row: the path as written in the file and its Change… button.
struct Folder {
    path: Retained<NSTextField>,
    change: Retained<NSButton>,
}

/// A number field and its stepper.
struct Number {
    field: Retained<NSTextField>,
    stepper: Retained<NSStepper>,
    /// The stepper's own range; if the file's value is outside it, it is
    /// widened to include that value too ([`set_number`]).
    range: (f64, f64),
    /// The file's value and the spelling shown in the field — from the last
    /// refresh. The action looks at this so that passing through an unchanged
    /// field (Tab) does not write.
    shown: RefCell<(f64, String)>,
}

impl Number {
    /// Whether the field's text is the same as the file's: the same spelling
    /// or a value that is the same at the precision it will be written (two places).
    fn unchanged(&self, text: &str, value: Option<f64>) -> bool {
        let shown = self.shown.borrow();
        let round = |value: f64| (value * 100.0).round();
        text.trim() == shown.1 || value.is_some_and(|value| round(value) == round(shown.0))
    }
}

/// A slider and its value label.
struct Slide {
    slider: Retained<NSSlider>,
    value: Retained<NSTextField>,
}

/// The place above the pages' scroll where the open page's example stands: the stack of the
/// examples (one shown at most), the room between it and the rows, and the hairline the rows
/// scroll under.
struct Sticky {
    stack: Retained<NSStackView>,
    gap: Retained<NSLayoutConstraint>,
    rule: Retained<NSBox>,
}

impl Sticky {
    /// The room under a shown example, before the first group.
    const GAP: f64 = 14.0;

    fn shows(&self) -> bool {
        self.stack
            .arrangedSubviews()
            .iter()
            .any(|view| !view.isHidden())
    }

    /// Fits the room to what shows, and shows the hairline when rows have scrolled under it.
    fn settle(&self, scrolled: bool) {
        let shows = self.shows();
        self.gap.setConstant(if shows { Self::GAP } else { 0.0 });
        self.rule.setHidden(!(shows && scrolled));
    }
}

/// The window's controls — what `refresh` writes and the actions read.
struct Controls {
    confirm_close: Retained<NSPopUpButton>,
    restore_windows: Retained<NSPopUpButton>,
    keep_running: Retained<NSPopUpButton>,
    clipboard: Retained<ThemeSwitch>,
    scrollback: Number,
    shell_integration: Retained<NSPopUpButton>,
    command_level: Retained<NSPopUpButton>,
    /// Theme: Match System or one theme, and the gallery of every theme for the one.
    theme_mode: Retained<NSSegmentedControl>,
    theme: Retained<ThemeGallery>,
    /// The pair that follows the system: the light themes and the dark ones.
    light_theme: Retained<ThemeGallery>,
    dark_theme: Retained<ThemeGallery>,
    font: Retained<NSPopUpButton>,
    /// The switches of the options the selected font offers.
    font_options: font_options::Options,
    size: Number,
    line_height: Number,
    letter_spacing: Number,
    scrollbar: Retained<NSPopUpButton>,
    content_edge: Retained<PictureChoice>,
    split_style: Retained<PictureChoice>,
    tab_details: Retained<NSSegmentedControl>,
    tab_icons: Retained<ThemeSwitch>,
    font_smoothing: Retained<ThemeSwitch>,
    tab_icon_colors: Retained<NSSegmentedControl>,
    tab_running: Retained<PictureChoice>,
    tab_position: Retained<PictureChoice>,
    minimum_contrast: Retained<NSPopUpButton>,
    shape: Retained<PictureChoice>,
    blink: Retained<NSPopUpButton>,
    blink_speed: Slide,
    radius: Slide,
    glow: Slide,
    unfocused: Retained<PictureChoice>,
    cursor_motion: Retained<NSSegmentedControl>,
    smooth_scroll: Retained<ThemeSwitch>,
    scroll_bounce: Retained<ThemeSwitch>,
    reduce_motion: Retained<NSPopUpButton>,
    keypress: Retained<NSPopUpButton>,
    erase: Retained<NSPopUpButton>,
    dock_arrival: Retained<NSPopUpButton>,
    preview_max_size: Retained<NSPopUpButton>,
    preview_read_only: Retained<ThemeSwitch>,
    preview_dir: Folder,
    /// The preview folder's Show in Finder, beside its path; it writes nothing.
    show_previews: Retained<NSButton>,
    preview_keep: Retained<NSPopUpButton>,
    preview_limit: Retained<NSPopUpButton>,
    /// The preview folder's usage; not a setting, so no row of its own.
    usage: Retained<NSTextField>,
    /// Clear Now, beside the usage; it writes nothing.
    clear: Retained<NSButton>,
    download_dir: Folder,
    download_conflict: Retained<NSPopUpButton>,
    download_notify: Retained<ThemeSwitch>,
    remote_stats: Retained<NSPopUpButton>,
    stats_interval: Number,
    remote_integration: Retained<ThemeSwitch>,
    ports: Retained<ThemeSwitch>,
    python_env: Retained<ThemeSwitch>,
    node_version: Retained<ThemeSwitch>,
    dim_splits: Retained<ThemeSwitch>,
    focus_frame: Retained<ThemeSwitch>,
    resize_in_steps: Retained<ThemeSwitch>,
    /// The pages' rows: the lock, the dependent row and the row's
    /// diagnostic come from here.
    rows: Vec<Row>,
    /// The pages' groups, in the order of `rows` they cut spans from.
    groups: Vec<GroupView>,
}

/// A row of a group's box: the name, its controls and the note under the name.
struct Row {
    /// `None` for the one row that is not a setting ("In use").
    key: Option<Key>,
    view: Retained<NSView>,
    /// The hairline above the row; the group's first shown row has none.
    separator: Retained<NSBox>,
    label: Retained<NSTextField>,
    controls: Vec<Retained<NSControl>>,
    /// Labels beside the controls (a unit, a slider's reading) that dim with the row.
    dim: Vec<Retained<NSTextField>>,
    /// Description or diagnostic; if there is neither the note is hidden
    /// (it leaves no gap).
    note: Retained<NSTextField>,
    hidden: Cell<bool>,
}

impl Row {
    /// Whether the row's controls are enabled, whether its label is dimmed
    /// (a dependent row and the lock go through the same gate).
    fn set_enabled(&self, enabled: bool) {
        for control in &self.controls {
            control.setEnabled(enabled);
        }
        let color = if enabled {
            look::ink(Ink::Text)
        } else {
            look::faded(Ink::Dim, 0.55)
        };
        self.label.setTextColor(Some(&color));
        let reading = if enabled {
            look::ink(Ink::Dim)
        } else {
            look::faded(Ink::Dim, 0.55)
        };
        for label in &self.dim {
            label.setTextColor(Some(&reading));
        }
    }

    /// Lights the row for a moment — a wash of the accent that fades — so a result that sent the
    /// reader here shows which row it meant. With Reduce Motion the row is not animated.
    fn glow(&self, mtm: MainThreadMarker) {
        if self.hidden.get() || app::delegate(mtm).is_none_or(|app| app.reduce_motion()) {
            return;
        }
        let wash = NSBox::new(mtm);
        wash.setBoxType(NSBoxType::Custom);
        wash.setTitlePosition(NSTitlePosition::NoTitle);
        wash.setBorderWidth(0.0);
        wash.setCornerRadius(0.0);
        wash.setFillColor(&look::faded(Ink::Accent, 0.22));
        // Under the row's own views: a wash on top would take the next click.
        wash.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.view
            .addSubview_positioned_relativeTo(&wash, NSWindowOrderingMode::Below, None);
        activate(&[
            wash.leadingAnchor()
                .constraintEqualToAnchor(&self.view.leadingAnchor()),
            wash.trailingAnchor()
                .constraintEqualToAnchor(&self.view.trailingAnchor()),
            wash.topAnchor()
                .constraintEqualToAnchor(&self.view.topAnchor()),
            wash.bottomAnchor()
                .constraintEqualToAnchor(&self.view.bottomAnchor()),
        ]);
        let fading = wash.clone();
        let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
            // SAFETY: AppKit gives the block a live context, for the block's duration.
            let context = unsafe { context.as_ref() };
            context.setDuration(1.4);
            fading.animator().setAlphaValue(0.0);
        });
        let done = RcBlock::new(move || wash.removeFromSuperview());
        NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&done));
    }

    /// Takes the row out of its group (it leaves no gap) or puts it back.
    fn set_hidden(&self, hidden: bool) {
        self.hidden.set(hidden);
        self.view.setHidden(hidden);
    }

    /// Sets the note to the diagnostic, the reason the row is dimmed
    /// ([`pages::dependency`], [`motion_override`]) or the description
    /// ([`row_description`]) — in that order; if there is none it hides
    /// the note. A disabled row's description dims together with its label,
    /// the reason does not dim: it is the only text that says why the row is
    /// disabled.
    fn set_note(
        &self,
        diagnostic: Option<&str>,
        reason: Option<&str>,
        description: Option<&str>,
        enabled: bool,
    ) {
        let (text, color) = match (diagnostic, reason, description) {
            (Some(diagnostic), _, _) => (diagnostic, look::ink(Ink::Warning)),
            (None, Some(reason), _) => (reason, look::ink(Ink::Dim)),
            (None, None, Some(description)) if enabled => (description, look::ink(Ink::Dim)),
            (None, None, Some(description)) => (description, look::faded(Ink::Dim, 0.7)),
            (None, None, None) => {
                self.note.setHidden(true);
                return;
            }
        };
        self.note.setStringValue(&NSString::from_str(text));
        self.note.setTextColor(Some(&color));
        self.note.setHidden(false);
    }
}

/// What the theme lists and the pictures are made from.
pub(crate) struct ThemeLists<'a> {
    /// The embedded themes' names, in groups (`Theme::embedded_groups`).
    pub(crate) embedded: &'a [Vec<&'a str>],
    /// The names of the themes in the user's folder, and their colours (read by the app; the
    /// window reads no theme file), in the same order.
    pub(crate) user: &'a [String],
    pub(crate) user_themes: &'a [Theme],
    /// The theme on screen for the file's choice: the pictures are drawn in its colours.
    pub(crate) on_screen: &'a Theme,
    /// The themes the file names for the light and for the dark appearance, and whether the
    /// system is in the dark one: Appearance's example shows the pair.
    pub(crate) light: &'a Theme,
    pub(crate) dark: &'a Theme,
    pub(crate) dark_appearance: bool,
}

pub(crate) struct Ivars {
    window: OnceCell<Retained<NSWindow>>,
    sidebar: OnceCell<Retained<NSTableView>>,
    /// The search field above the pages' list, and what it holds: a query shows the results in
    /// place of the open page.
    search: OnceCell<Retained<NSSearchField>>,
    query: RefCell<String>,
    /// Every row of every page as the search sees it, taken at each refresh.
    entries: RefCell<Vec<search::Entry>>,
    /// The results on screen: the page and the row each result opens.
    found: RefCell<Vec<(Category, Option<Key>)>>,
    /// The stack of results, the pages' last.
    results: OnceCell<Retained<NSStackView>>,
    header: OnceCell<Retained<NSTextField>>,
    banner: OnceCell<BannerView>,
    /// Whether the window has been shown once (the gate of centring).
    shown_once: Cell<bool>,
    pane_tops: OnceCell<PaneTops>,
    /// Where the open page's example stands, above the pages' scroll.
    sticky: OnceCell<Sticky>,
    /// The scroll view the panes sit in; scrolled back to the top when the
    /// category changes.
    pane_scroll: OnceCell<Retained<NSScrollView>>,
    /// One constraint per pane tying the document's bottom to that pane's
    /// bottom; only the selected pane's is active, so each pane scrolls exactly
    /// as far as its own rows.
    pane_bottoms: OnceCell<Vec<Retained<NSLayoutConstraint>>>,
    /// "Open settings.toml" at the foot of the sidebar; hidden while the file is locked, when the
    /// strip carries it as the default button (Enter).
    side_open: OnceCell<Retained<NSButton>>,
    /// One stack per page that has rows; only the selected one is visible.
    panes: OnceCell<Vec<Retained<NSStackView>>>,
    /// The live examples at the top of five pages, and what the last refresh gave them: an example
    /// is drawn when its page opens and when a refresh finds its page open.
    specimens: OnceCell<specimen::Specimens>,
    specimen_inputs: RefCell<Option<specimen::Inputs>>,
    /// Keyboard: the table of the commands and their shortcuts, shown in the pages' place.
    keyboard: OnceCell<Retained<KeyboardPage>>,
    controls: OnceCell<Controls>,
    /// The popups' item lists — the meaning of the selected index. Rebuilt on
    /// every `refresh`.
    fonts: RefCell<Vec<FontItem>>,
    /// The options the Options row shows, in the order of the switches' tags.
    /// Rebuilt on every `refresh`.
    font_options: RefCell<Vec<FontOption>>,
    max_sizes: RefCell<Vec<SizeItem>>,
    limits: RefCell<Vec<SizeItem>>,
    contrasts: RefCell<Vec<ContrastItem>>,
    /// `preview_dir` and `download_dir` as the file writes them — what Change…
    /// starts the panel in and Show in Finder opens. From the last refresh.
    folders: RefCell<(String, String)>,
    /// The last usage measurement asked for; an older answer arriving later is
    /// dropped ([`SettingsWindow::show_usage`]).
    usage_generation: Cell<u64>,
    /// The monospaced families on the machine — **once** when the window is
    /// born: the list opens every candidate with CoreText and is not worth rebuilding on every save.
    /// The palette shows the same list (`font_options::families`, warmed at launch).
    families: Vec<String>,
    /// The families that ship with the app: the Font list's first section.
    bundled: Vec<String>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirement; Drop is not implemented.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriSettingsWindow"]
    #[ivars = Ivars]
    pub(crate) struct SettingsWindow;

    unsafe impl NSObjectProtocol for SettingsWindow {}

    unsafe impl NSTableViewDataSource for SettingsWindow {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _table: &NSTableView) -> NSInteger {
            SideItem::ALL.len() as NSInteger
        }
    }

    unsafe impl NSControlTextEditingDelegate for SettingsWindow {}

    unsafe impl NSWindowDelegate for SettingsWindow {
        /// A closed window gives the examples' renderer and sessions back; opening it draws them
        /// again from the refresh that reopening makes.
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _note: &NSNotification) {
            self.cancel_blink();
            if let Some(specimens) = self.ivars().specimens.get() {
                specimens.release();
            }
        }
    }

    unsafe impl NSTableViewDelegate for SettingsWindow {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn view_for_row(
            &self,
            table: &NSTableView,
            _column: Option<&NSTableColumn>,
            row: NSInteger,
        ) -> Option<Retained<NSView>> {
            match SideItem::at(row) {
                Some(SideItem::Page(category)) => Some(Retained::into_super(SideRow::new(
                    self.mtm(),
                    category,
                    table.selectedRow() == row,
                ))),
                Some(SideItem::Gap) => Some(NSView::new(self.mtm())),
                None => None,
            }
        }

        #[unsafe(method(tableView:heightOfRow:))]
        fn height_of_row(&self, _table: &NSTableView, row: NSInteger) -> f64 {
            match SideItem::at(row) {
                Some(SideItem::Gap) => SIDE_GAP_HEIGHT,
                _ => SIDE_ROW_HEIGHT,
            }
        }

        #[unsafe(method(tableView:shouldSelectRow:))]
        fn should_select_row(&self, _table: &NSTableView, row: NSInteger) -> bool {
            matches!(SideItem::at(row), Some(SideItem::Page(_)))
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_did_change(&self, _note: &NSNotification) {
            // A page chosen in the list ends the search.
            self.clear_search();
            self.show_selected();
        }
    }

    impl SettingsWindow {
        /// A click on the sidebar. A different page ends the search through the selection; the
        /// page that is already open reports no change, so a click on it, while the results
        /// cover it, brings the page back here.
        #[unsafe(method(sidebarClicked:))]
        fn sidebar_clicked(&self, sender: Option<&AnyObject>) {
            let Some(table) = sender.and_then(|s| s.downcast_ref::<NSTableView>()) else {
                return;
            };
            if self.searching() && matches!(SideItem::at(table.clickedRow()), Some(SideItem::Page(_)))
            {
                self.clear_search();
                self.show_selected();
            }
        }

        #[unsafe(method(popupChanged:))]
        fn popup_changed(&self, sender: Option<&AnyObject>) {
            let Some(popup) = sender.and_then(|s| s.downcast_ref::<NSPopUpButton>()) else {
                return;
            };
            let index = popup.indexOfSelectedItem();
            let Some(key) = Key::from_tag(popup.tag()) else {
                return;
            };
            self.save(self.choice_edit(key, index));
        }

        /// A pictured choice: the chosen picture's place is the option's place in the list.
        #[unsafe(method(pictureChanged:))]
        fn picture_changed(&self, sender: Option<&AnyObject>) {
            let Some(choice) = sender.and_then(|s| s.downcast_ref::<PictureChoice>()) else {
                return;
            };
            let (Some(key), Some(index)) = (Key::from_tag(choice.tag()), choice.selected()) else {
                return;
            };
            self.save(self.choice_edit(key, index as NSInteger));
        }

        /// A segmented choice: the segment's place is the option's place in the list. The key is
        /// in the control's identifier, because a segmented control's tag is its selected
        /// segment's.
        #[unsafe(method(segmentChanged:))]
        fn segment_changed(&self, sender: Option<&AnyObject>) {
            let Some(control) = sender.and_then(|s| s.downcast_ref::<NSSegmentedControl>()) else {
                return;
            };
            let Some(key) = control
                .identifier()
                .and_then(|identifier| identifier.to_string().parse().ok())
                .and_then(Key::from_tag)
            else {
                return;
            };
            self.save(self.choice_edit(key, control.selectedSegment()));
        }

        /// The search field's text changed: the results take the page's place, or give it back.
        #[unsafe(method(searchSettings:))]
        fn search_settings(&self, sender: Option<&AnyObject>) {
            let Some(field) = sender.and_then(|s| s.downcast_ref::<NSSearchField>()) else {
                return;
            };
            let text = field.stringValue().to_string();
            if self.ivars().query.replace(text.clone()) == text {
                return;
            }
            self.rebuild_results();
            self.show_selected();
        }

        /// A result was clicked: open its page, and bring its row into view.
        #[unsafe(method(resultClicked:))]
        fn result_clicked(&self, sender: Option<&AnyObject>) {
            let Some(row) = sender.and_then(|s| s.downcast_ref::<search::ResultRow>()) else {
                return;
            };
            let target = usize::try_from(row.tag())
                .ok()
                .and_then(|at| self.ivars().found.borrow().get(at).copied());
            if let Some((category, key)) = target {
                self.open_row(category, key);
            }
        }

        /// An option of the selected font: its switch's tag is the option's place in the
        /// list the last refresh showed.
        #[unsafe(method(fontOptionChanged:))]
        fn font_option_changed(&self, sender: Option<&AnyObject>) {
            let Some(switch) = switch::of(sender) else {
                return;
            };
            let on = switch.is_on();
            let option = usize::try_from(switch.tag())
                .ok()
                .and_then(|at| self.ivars().font_options.borrow().get(at).cloned());
            let Some(option) = option else {
                return;
            };
            let Some(delegate) = app::delegate(self.mtm()) else {
                return;
            };
            // The file's choices, not what a preview shows: the option may write two lines.
            let font = delegate.file_settings().font;
            // A refused write puts the window back to the file by itself.
            let _ = delegate.save_edits(&option.edits(&font, on));
        }

        /// A tile of alternate letters was clicked: the option it is flips, from the file's
        /// choice, so a click is a toggle whatever a preview shows.
        #[unsafe(method(fontTileClicked:))]
        fn font_tile_clicked(&self, sender: Option<&AnyObject>) {
            let Some(tiles) = sender.and_then(|s| s.downcast_ref::<font_options::LetterTiles>())
            else {
                return;
            };
            let option = tiles
                .clicked()
                .and_then(|at| self.ivars().font_options.borrow().get(at).cloned());
            let Some(option) = option else {
                return;
            };
            let Some(delegate) = app::delegate(self.mtm()) else {
                return;
            };
            let font = delegate.file_settings().font;
            let _ = delegate.save_edits(&option.edits(&font, !option.is_on(&font)));
        }

        /// "Show All" and "Show Fewer" under the tiles.
        #[unsafe(method(showAllTiles:))]
        fn show_all_tiles(&self, _sender: Option<&AnyObject>) {
            let Some(controls) = self.ivars().controls.get() else {
                return;
            };
            controls.font_options.toggle_all();
            if let Some(delegate) = app::delegate(self.mtm()) {
                delegate.refresh_settings_window();
            }
        }

        #[unsafe(method(switchChanged:))]
        fn switch_changed(&self, sender: Option<&AnyObject>) {
            let Some(switch) = switch::of(sender) else {
                return;
            };
            let on = switch.is_on();
            let edit = match Key::from_tag(switch.tag()) {
                Some(Key::Clipboard) => Some(SettingsEdit::Osc52(if on {
                    Osc52::Copy
                } else {
                    Osc52::Off
                })),
                Some(Key::SmoothScroll) => Some(SettingsEdit::SmoothScroll(if on {
                    SmoothScroll::On
                } else {
                    SmoothScroll::Off
                })),
                Some(Key::ScrollBounce) => Some(SettingsEdit::ScrollBounce(if on {
                    ScrollBounce::On
                } else {
                    ScrollBounce::Off
                })),
                Some(Key::PreviewReadOnly) => Some(SettingsEdit::PreviewReadOnly(on)),
                Some(Key::DownloadNotify) => Some(SettingsEdit::DownloadNotify(on)),
                Some(Key::RemoteIntegration) => Some(SettingsEdit::RemoteIntegration(on)),
                Some(Key::Ports) => Some(SettingsEdit::ShellPorts(on)),
                Some(Key::PythonEnv) => Some(SettingsEdit::ShellPythonEnv(on)),
                Some(Key::NodeVersion) => Some(SettingsEdit::ShellNodeVersion(on)),
                Some(Key::DimSplits) => Some(SettingsEdit::DimUnfocusedSplits(on)),
                Some(Key::FocusFrame) => Some(SettingsEdit::FocusFrame(on)),
                Some(Key::ResizeInSteps) => Some(SettingsEdit::ResizeInSteps(on)),
                Some(Key::TabIcons) => Some(SettingsEdit::TabIcons(on)),
                Some(Key::FontSmoothing) => Some(SettingsEdit::FontSmoothing(on)),
                _ => None,
            };
            self.save(edit);
        }

        /// While a slider is dragged the value label and the page's example follow the hand
        /// ([`SettingsWindow::preview_specimen`]); the value is written to the file **on
        /// release**. `continuous` is on, otherwise both would freeze during the drag: the
        /// decision to write or not is from the event's type — if the mouse is down and dragging
        /// it is an intermediate value. The event of a slider changed with the keyboard (arrow
        /// key) is a key event and it writes.
        #[unsafe(method(sliderChanged:))]
        fn slider_changed(&self, sender: Option<&AnyObject>) {
            let Some(slider) = sender.and_then(|s| s.downcast_ref::<NSSlider>()) else {
                return;
            };
            let Some(key) = Key::from_tag(slider.tag()) else {
                return;
            };
            let position = slider.doubleValue();
            let value = match key {
                Key::BlinkSpeed => blink_from_position(position),
                _ => position,
            };
            if let Some(controls) = self.ivars().controls.get() {
                let label = match key {
                    Key::BlinkSpeed => Some((&controls.blink_speed.value, seconds_label(value))),
                    Key::Radius => Some((&controls.radius.value, radius_label(value))),
                    Key::Glow => Some((&controls.glow.value, glow_label(value))),
                    _ => None,
                };
                if let Some((field, text)) = label {
                    field.setStringValue(&NSString::from_str(&text));
                }
            }
            // Besides dragging, the tracking loop can carry other events
            // (Force Touch pressure, a periodic event): those are mid-gesture too.
            let dragging = NSApplication::sharedApplication(self.mtm())
                .currentEvent()
                .is_some_and(|event| {
                    matches!(
                        event.r#type(),
                        NSEventType::LeftMouseDragged
                            | NSEventType::LeftMouseDown
                            | NSEventType::Pressure
                            | NSEventType::Periodic
                    )
                });
            if dragging {
                self.preview_specimen(key, value);
                return;
            }
            let edit = match key {
                Key::BlinkSpeed => Some(SettingsEdit::BlinkInterval(value)),
                Key::Radius => Some(SettingsEdit::CursorRadius(value)),
                Key::Glow => Some(SettingsEdit::CursorGlow(value)),
                _ => None,
            };
            self.save(edit);
        }

        /// Number field: on Enter or on leaving focus. An input that
        /// is not accepted is not written, the field returns to the active value.
        #[unsafe(method(fieldChanged:))]
        fn field_changed(&self, sender: Option<&AnyObject>) {
            let Some(field) = sender.and_then(|s| s.downcast_ref::<NSTextField>()) else {
                return;
            };
            let text = field.stringValue().to_string();
            let Some(controls) = self.ivars().controls.get() else {
                return;
            };
            let (number, value) = match Key::from_tag(field.tag()) {
                Some(Key::Scrollback) => (
                    &controls.scrollback,
                    parse_scrollback(&text).map(|lines| lines as f64),
                ),
                Some(Key::Size) => (&controls.size, parse_decimal(&text, MIN_SIZE..=MAX_SIZE)),
                Some(Key::LineHeight) => (
                    &controls.line_height,
                    parse_decimal(&text, LINE_HEIGHT_RANGE),
                ),
                Some(Key::LetterSpacing) => (
                    &controls.letter_spacing,
                    parse_decimal(&text, LETTER_SPACING_RANGE),
                ),
                Some(Key::StatsInterval) => (
                    &controls.stats_interval,
                    parse_interval(&text).map(f64::from),
                ),
                _ => return,
            };
            // Passing through an unchanged field does not write: the rounded
            // spelling (`1.125` → "1.13") would silently change the file's value.
            if number.unchanged(&text, value) {
                return;
            }
            let edit = value.and_then(|value| match Key::from_tag(field.tag()) {
                Some(Key::Scrollback) => Some(SettingsEdit::Scrollback(value as usize)),
                Some(Key::Size) => Some(SettingsEdit::FontSize(value)),
                Some(Key::LineHeight) => Some(SettingsEdit::LineHeight(value)),
                Some(Key::LetterSpacing) => Some(SettingsEdit::LetterSpacing(value)),
                // `parse_interval` accepted it: a whole number inside the range.
                Some(Key::StatsInterval) => Some(SettingsEdit::StatsInterval(value as u8)),
                _ => None,
            });
            match edit {
                Some(edit) => self.save(Some(edit)),
                // An input that is not accepted: the field returns to the
                // spelling in the file. Directly, not through refresh — a
                // refresh does not touch the field being edited ([`set_number`]).
                None => {
                    let shown = number.shown.borrow().1.clone();
                    field.setStringValue(&NSString::from_str(&shown));
                }
            }
        }

        /// The pages scrolled: the hairline under the example shows while rows are under it.
        #[unsafe(method(pagesScrolled:))]
        fn pages_scrolled_note(&self, _note: Option<&NSNotification>) {
            if let Some(sticky) = self.ivars().sticky.get() {
                sticky.settle(self.pages_scrolled());
            }
        }

        /// The example's blinking caret turns to its other half, and the next turn is asked for
        /// unless the blink has stopped.
        #[unsafe(method(blinkSpecimen:))]
        fn blink_specimen(&self, _sender: Option<&AnyObject>) {
            if let Some(delay) = self
                .ivars()
                .specimens
                .get()
                .and_then(specimen::Specimens::blink_step)
            {
                self.blink_after(delay);
            }
        }

        /// A theme's card clicked in one of the galleries: written for the gallery's row.
        #[unsafe(method(themeChosen:))]
        fn theme_chosen(&self, sender: Option<&AnyObject>) {
            let Some(gallery) = sender.and_then(|s| s.downcast_ref::<ThemeGallery>()) else {
                return;
            };
            let Some(name) = gallery.chosen() else {
                return;
            };
            let edit = match Key::from_tag(gallery.tag()) {
                Some(Key::Theme) => Some(SettingsEdit::Theme(name)),
                Some(Key::LightTheme) => Some(SettingsEdit::LightTheme(name)),
                Some(Key::DarkTheme) => Some(SettingsEdit::DarkTheme(name)),
                _ => None,
            };
            self.save(edit);
        }

        /// Match System or One Theme: one theme is the one on screen now, so nothing changes
        /// but the following.
        #[unsafe(method(themeModeChanged:))]
        fn theme_mode_changed(&self, sender: Option<&AnyObject>) {
            let Some(control) = sender.and_then(|s| s.downcast_ref::<NSSegmentedControl>()) else {
                return;
            };
            let edit = if control.selectedSegment() == 0 {
                Some(SettingsEdit::Theme(SYSTEM_THEME.to_owned()))
            } else {
                // Read from the app now, not from what the window last showed.
                app::delegate(self.mtm()).map(|app| {
                    let file = app.file_settings();
                    SettingsEdit::Theme(file.theme_for(app.dark_appearance()).to_owned())
                })
            };
            self.save(edit);
        }

        #[unsafe(method(stepperChanged:))]
        fn stepper_changed(&self, sender: Option<&AnyObject>) {
            let Some(stepper) = sender.and_then(|s| s.downcast_ref::<NSStepper>()) else {
                return;
            };
            let value = stepper.doubleValue();
            let edit = match Key::from_tag(stepper.tag()) {
                // The stepper's bounds are `0..=SCROLLBACK_MAX` and its step is
                // a whole number: the value cannot be negative or fractional.
                Some(Key::Scrollback) => Some(SettingsEdit::Scrollback(value.round() as usize)),
                Some(Key::Size) => Some(SettingsEdit::FontSize(value)),
                Some(Key::LineHeight) => Some(SettingsEdit::LineHeight(value)),
                Some(Key::LetterSpacing) => Some(SettingsEdit::LetterSpacing(value)),
                // The stepper's bounds are the accepted range and its step is 1.
                Some(Key::StatsInterval) => Some(SettingsEdit::StatsInterval(value.round() as u8)),
                _ => None,
            };
            self.save(edit);
        }

        /// "Open settings.toml": today's "Settings…" path.
        #[unsafe(method(openFile:))]
        fn open_file(&self, _sender: Option<&AnyObject>) {
            if let Some(delegate) = app::delegate(self.mtm()) {
                delegate.edit_settings();
            }
        }

        /// The strip's Show in Finder, for a settings file that cannot be written.
        #[unsafe(method(showSettingsFile:))]
        fn show_settings_file(&self, _sender: Option<&AnyObject>) {
            if let Some(delegate) = app::delegate(self.mtm()) {
                delegate.reveal_settings();
            }
        }

        /// A folder row's Change…: a folder panel as a sheet, started in the
        /// folder the file names; the chosen one is written as one edit.
        #[unsafe(method(chooseFolder:))]
        fn choose_folder(&self, sender: Option<&AnyObject>) {
            let Some(button) = sender.and_then(|s| s.downcast_ref::<NSButton>()) else {
                return;
            };
            let Some(key) = Key::from_tag(button.tag()) else {
                return;
            };
            self.choose_folder_for(key);
        }

        /// The preview folder's Show in Finder.
        #[unsafe(method(showPreviewFolder:))]
        fn show_preview_folder(&self, _sender: Option<&AnyObject>) {
            let text = self.ivars().folders.borrow().0.clone();
            let folder = bt_core::expand_home(&text, child::home().as_deref());
            match folder.filter(|folder| folder.is_dir()) {
                Some(folder) => {
                    let url = NSURL::fileURLWithPath(&NSString::from_str(
                        &folder.to_string_lossy(),
                    ));
                    NSWorkspace::sharedWorkspace().openURL(&url);
                }
                // Nothing is created to be shown (no folder
                // before a preview lands); the folder appears with the first one.
                None => crate::preview::beep(),
            }
        }

        /// Clear Now: the preview cache's single sweep method;
        /// it measures the usage again when it ends.
        #[unsafe(method(clearPreviews:))]
        fn clear_previews(&self, _sender: Option<&AnyObject>) {
            if let Some(delegate) = app::delegate(self.mtm()) {
                delegate.sweep_previews(Sweep::ClearNow);
            }
        }
    }
);

/// The action's theme item → the name to write.
/// The action's font item → the family to write; there is nothing to write
/// when the file's value that is not in the list is selected again.
fn font_edit(items: &[FontItem], index: NSInteger) -> Option<String> {
    match items.get(usize::try_from(index).ok()?)? {
        FontItem::Default => Some(String::new()),
        FontItem::Family(name) => Some(name.clone()),
        FontItem::Separator | FontItem::Header(_) | FontItem::Missing(_) => None,
    }
}

/// The blink-speed reading: seconds, to two places (`0.50 s`).
fn seconds_label(seconds: f64) -> String {
    format!("{seconds:.2} s")
}

/// The corner-radius reading: the ratio to two places (`0.10`).
fn radius_label(ratio: f64) -> String {
    format!("{ratio:.2}")
}

/// The glow reading: one place, or "Off" when there is none (`1.0`).
fn glow_label(glow: f64) -> String {
    if glow > 0.0 {
        format!("{glow:.1}")
    } else {
        "Off".to_owned()
    }
}

impl SettingsWindow {
    /// Builds the window and all the controls; does not make it visible.
    pub(crate) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            search: OnceCell::new(),
            query: RefCell::new(String::new()),
            entries: RefCell::new(Vec::new()),
            found: RefCell::new(Vec::new()),
            results: OnceCell::new(),
            window: OnceCell::new(),
            sidebar: OnceCell::new(),
            header: OnceCell::new(),
            banner: OnceCell::new(),
            shown_once: Cell::new(false),
            pane_tops: OnceCell::new(),
            sticky: OnceCell::new(),
            pane_scroll: OnceCell::new(),
            pane_bottoms: OnceCell::new(),
            side_open: OnceCell::new(),
            panes: OnceCell::new(),
            specimens: OnceCell::new(),
            specimen_inputs: RefCell::new(None),
            keyboard: OnceCell::new(),
            controls: OnceCell::new(),
            fonts: RefCell::new(Vec::new()),
            font_options: RefCell::new(Vec::new()),
            max_sizes: RefCell::new(Vec::new()),
            contrasts: RefCell::new(Vec::new()),
            limits: RefCell::new(Vec::new()),
            folders: RefCell::new((String::new(), String::new())),
            usage_generation: Cell::new(0),
            families: crate::font_options::families(),
            bundled: crate::font_options::bundled_families(),
        });
        // SAFETY: NSObject's init takes no arguments and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.build();
        this
    }

    /// Whether the window is open (miniaturized too) — refreshing while closed
    /// is pointless, reopening refreshes.
    pub(crate) fn is_open(&self) -> bool {
        self.ivars()
            .window
            .get()
            .is_some_and(|window| window.isVisible() || window.isMiniaturized())
    }

    /// Brings the window to the front (centres it on first open); the category is the last selected.
    pub(crate) fn show(&self) {
        let Some(window) = self.ivars().window.get() else {
            return;
        };
        // Only on the first open: closing and miniaturizing also drop
        // `isVisible`, and the place the user moved it to would be lost on every open.
        if !self.ivars().shown_once.replace(true) {
            window.center();
        }
        NSApplication::sharedApplication(self.mtm()).activate();
        window.makeKeyAndOrderFront(None);
        // The Keyboard page lists the menu's titles: read again, as they may have changed.
        if let Some(keyboard) = self.ivars().keyboard.get() {
            keyboard.invalidate_menu();
        }
    }

    /// Opens the Keyboard page on the command named `name` (a name of the shortcut table), ready
    /// for its new shortcut.
    pub(crate) fn show_shortcut(&self, name: &'static str) {
        let (Some(sidebar), Some(keyboard)) =
            (self.ivars().sidebar.get(), self.ivars().keyboard.get())
        else {
            return;
        };
        sidebar.selectRowIndexes_byExtendingSelection(
            &NSIndexSet::indexSetWithIndex(SideItem::row_of(Category::Keyboard)),
            false,
        );
        self.show_selected();
        keyboard.show_command(name);
    }

    /// Fills the controls with the active settings — the single source of the
    /// value the window shows. A value set programmatically triggers no
    /// action, so calling it from inside a control's action (write →
    /// `reload_settings` → here) does not create a loop.
    ///
    /// The file's state (`state`) and the write slot (`write`) build the lock,
    /// the banner and the row diagnostics ([`status`]); since every refresh
    /// rebuilds all of them from scratch no trace of a healed state is left.
    /// `resolved` holds the **resolved** answers (setting + system): whether
    /// the motion rows are overridden comes from Reduce Motion's
    /// ([`motion_override`]), not from the setting itself, and the scroll
    /// bar's note from the form "system" gives right now ([`scrollbar_note`]).
    /// `themes` is what the theme lists and the pictures are made from.
    pub(crate) fn refresh(
        &self,
        settings: &Settings,
        resolved: Resolved,
        state: &FileState,
        write: &[String],
        themes: &ThemeLists,
    ) {
        let Some(c) = self.ivars().controls.get() else {
            return;
        };
        // The window and its pictures are drawn in the theme that is on screen: its light or dark
        // for what AppKit draws, its roles for the rest.
        let palette = Palette::of(themes.on_screen);
        if let Some(window) = self.ivars().window.get() {
            window.setAppearance(look::appearance(themes.on_screen).as_deref());
            // The title strip and the room round the sidebar are the window's own drawing, out of
            // the content's reach, and the window draws them again only for another colour: it is
            // given the theme's ground itself, not the dynamic colour, which is one object for
            // every theme (one light theme for another changed nothing it could see).
            window.setBackgroundColor(Some(&crate::tab_bar::Tint::of(palette.ground, 1.0).color()));
            if look::set(palette)
                && let Some(content) = window.contentView()
            {
                look::redraw(&content);
            }
        }
        for choice in [
            &c.content_edge,
            &c.split_style,
            &c.tab_running,
            &c.tab_position,
            &c.shape,
            &c.unfocused,
        ] {
            choice.set_palette(palette);
        }
        select_choice(&c.confirm_close, settings.confirm_close);
        select_choice(&c.restore_windows, settings.restore_windows);
        select_choice(&c.keep_running, settings.keep_running);
        set_switch(&c.clipboard, osc52_on(settings.osc52));
        set_number(
            &c.scrollback,
            settings.scrollback as f64,
            &settings.scrollback.to_string(),
        );
        select_choice(&c.shell_integration, settings.shell_integration);
        select_choice(&c.command_level, settings.commands_level);

        // Theme: Match System shows the pair's galleries in the rows under it; one theme shows
        // every theme here.
        let follows = settings.theme == SYSTEM_THEME;
        c.theme_mode.setSelectedSegment(if follows { 0 } else { 1 });
        c.theme.setHidden(follows);
        c.theme.show(
            theme_sections(&settings.theme, Shade::All, themes),
            &settings.theme,
        );
        c.light_theme.show(
            theme_sections(&settings.light_theme, Shade::Light, themes),
            &settings.light_theme,
        );
        c.dark_theme.show(
            theme_sections(&settings.dark_theme, Shade::Dark, themes),
            &settings.dark_theme,
        );
        let (items, index) = contrast_items(settings.minimum_contrast);
        fill_contrasts(&c.minimum_contrast, &items, index);
        self.ivars().contrasts.replace(items);

        let (items, index) = font_items(
            settings.font.family.as_deref(),
            &self.ivars().families,
            &self.ivars().bundled,
        );
        // Each family is drawn in its own font, which costs a pass over the whole list: the
        // popup is rebuilt only when the list changed (a family the file names that is not on
        // the machine says why with the font notice, which can change on its own).
        let same = *self.ivars().fonts.borrow() == items
            && !items
                .iter()
                .any(|item| matches!(item, FontItem::Missing(_)));
        if same {
            c.font.selectItemAtIndex(index as NSInteger);
        } else {
            fill_fonts(&c.font, &items, index);
        }
        self.ivars().fonts.replace(items);
        set_number(
            &c.size,
            settings.font.size,
            &decimal_label(settings.font.size),
        );
        set_number(
            &c.line_height,
            settings.font.line_height,
            &decimal_label(settings.font.line_height),
        );
        set_number(
            &c.letter_spacing,
            settings.font.letter_spacing,
            &decimal_label(settings.font.letter_spacing),
        );
        select_choice(&c.scrollbar, settings.scrollbar);
        select_picture(&c.content_edge, settings.content_edge);
        select_picture(&c.split_style, settings.split_style);
        select_segment(&c.tab_details, settings.tab_details);
        set_switch(&c.tab_icons, settings.tab_icons);
        set_switch(&c.font_smoothing, settings.font.smoothing);
        select_segment(&c.tab_icon_colors, settings.tab_icon_colors);
        select_picture(&c.tab_running, settings.tab_running);
        select_picture(&c.tab_position, settings.tab_position);

        select_picture(&c.shape, settings.cursor);
        select_choice(&c.blink, settings.cursor_blink);
        set_slide(
            &c.blink_speed,
            blink_to_position(settings.blink_interval),
            &seconds_label(settings.blink_interval),
        );
        set_slide(
            &c.radius,
            settings.caret.radius_ratio,
            &radius_label(settings.caret.radius_ratio),
        );
        set_slide(
            &c.glow,
            settings.caret.glow,
            &glow_label(settings.caret.glow),
        );
        select_picture(&c.unfocused, settings.caret.unfocused);

        select_segment(&c.cursor_motion, settings.cursor_motion);
        set_switch(&c.smooth_scroll, smooth_on(settings.smooth_scroll));
        set_switch(&c.scroll_bounce, bounce_on(settings.scroll_bounce));
        select_choice(&c.reduce_motion, settings.reduce_motion);
        select_choice(&c.keypress, settings.keypress);
        select_choice(&c.erase, settings.erase);
        select_choice(&c.dock_arrival, settings.dock_arrival);

        set_switch(&c.remote_integration, settings.remote_integration);
        set_switch(&c.ports, settings.shell_ports);
        set_switch(&c.python_env, settings.shell_python_env);
        set_switch(&c.node_version, settings.shell_node_version);
        set_switch(&c.dim_splits, settings.dim_unfocused_splits);
        set_switch(&c.focus_frame, settings.focus_frame);
        set_switch(&c.resize_in_steps, settings.resize_in_steps);
        let files = &settings.remote_files;
        let (items, index) = size_items(files.preview_max_size, PREVIEW_SIZE_PRESETS);
        fill_sizes(&c.preview_max_size, &items, index);
        self.ivars().max_sizes.replace(items);
        set_switch(&c.preview_read_only, files.preview_read_only);
        c.preview_dir
            .path
            .setStringValue(&NSString::from_str(&files.preview_dir));
        select_choice(&c.preview_keep, files.preview_keep);
        let (items, index) = size_items(files.preview_limit, PREVIEW_LIMIT_PRESETS);
        fill_sizes(&c.preview_limit, &items, index);
        self.ivars().limits.replace(items);
        c.download_dir
            .path
            .setStringValue(&NSString::from_str(&files.download_dir));
        select_choice(&c.download_conflict, files.download_conflict);
        set_switch(&c.download_notify, files.download_notify);
        select_choice(&c.remote_stats, settings.remote_stats.mode);
        let interval = settings.remote_stats.interval;
        set_number(
            &c.stats_interval,
            f64::from(interval),
            &interval.to_string(),
        );
        self.ivars()
            .folders
            .replace((files.preview_dir.clone(), files.download_dir.clone()));

        let status = status(state, write);
        // The font's options: the selected font's own list, shown as the file chooses.
        let options = crate::font_options::options(settings.font.family.as_deref());
        for row in &c.rows {
            let Some(key) = row.key else {
                // "In use" is measured, not written: neither the lock nor a dependency reaches it.
                continue;
            };
            let forced = motion_override(key, settings, resolved.reduce);
            let leans = pages::dependency(key, settings);
            row.set_hidden(leans.hidden || (key == Key::FontFeatures && options.is_empty()));
            let enabled =
                !status.locked && leans.enabled && forced.is_none_or(|forced| forced.enabled);
            row.set_enabled(enabled);
            let diagnostic = status
                .rows
                .iter()
                .find(|(row_key, _)| *row_key == key)
                .map(|(_, message)| message.as_str());
            row.set_note(
                diagnostic,
                leans.reason.or(forced.map(|forced| forced.note)),
                row_description(key, settings, resolved),
                enabled,
            );
        }
        // A group is as visible as its rows: none shown, no title and no empty box. Its first
        // shown row has no hairline above it, the others do.
        for group in &c.groups {
            let rows = &c.rows[group.rows.clone()];
            let mut first = true;
            for row in rows.iter().filter(|row| !row.hidden.get()) {
                row.separator.setHidden(first);
                first = false;
            }
            group.view.setHidden(first);
            if let Some(title) = &group.title
                && rows.iter().any(|row| row.key == Some(Key::FontFeatures))
            {
                title.setStringValue(&NSString::from_str(&font_options::heading(&settings.font)));
            }
            if let Some(aside) = &group.aside {
                aside.setStringValue(&NSString::from_str(
                    (group.spec.aside)(settings).unwrap_or_default(),
                ));
            }
        }
        c.font_options
            .show(self, &options, &settings.font, !status.locked);
        self.ivars().font_options.replace(options);
        // What the search can find follows the file: the descriptions and the rows that apply.
        self.ivars()
            .entries
            .replace(search::entries(settings, resolved));
        if self.searching() {
            self.rebuild_results();
        }
        if let Some(keyboard) = self.ivars().keyboard.get() {
            keyboard.refresh(settings, status.locked);
        }
        if let Some(banner) = self.ivars().banner.get() {
            banner.show(&status.banner);
        }
        self.layout_panes(!status.banner.lines.is_empty());
        if let Some(open) = self.ivars().side_open.get() {
            // The strip carries the button, as the default one, when the file is locked.
            open.setHidden(status.locked);
        }
        // The open page's example is drawn from the file as it is now.
        self.ivars().specimen_inputs.replace(Some(specimen::Inputs {
            settings: settings.clone(),
            light: *themes.light,
            dark: *themes.dark,
            on_screen: *themes.on_screen,
            dark_appearance: themes.dark_appearance,
            reduce_motion: resolved.reduce,
        }));
        self.show_specimen();
    }

    /// A new usage measurement starts: its generation, which
    /// [`SettingsWindow::show_usage`] compares with the answer's.
    pub(crate) fn next_usage_generation(&self) -> u64 {
        let generation = self.ivars().usage_generation.get().wrapping_add(1);
        self.ivars().usage_generation.set(generation);
        generation
    }

    /// Shows a usage measurement (`AppDelegate::measure_preview_usage`) unless
    /// a newer one was asked for meanwhile — measurements run on their own
    /// threads and can arrive out of order.
    pub(crate) fn show_usage(&self, generation: u64, usage: Option<(u64, usize)>) {
        if generation != self.ivars().usage_generation.get() {
            return;
        }
        if let Some(controls) = self.ivars().controls.get() {
            controls
                .usage
                .setStringValue(&NSString::from_str(&usage_label(usage)));
        }
    }

    /// Change… for `key`'s folder: an `NSOpenPanel` sheet on this window
    /// (dropped if a sheet is already open), started in the current folder.
    fn choose_folder_for(&self, key: Key) {
        let Some(window) = self
            .ivars()
            .window
            .get()
            .filter(|window| window.attachedSheet().is_none())
        else {
            return;
        };
        let current = match key {
            Key::PreviewDir => self.ivars().folders.borrow().0.clone(),
            Key::DownloadDir => self.ivars().folders.borrow().1.clone(),
            _ => return,
        };
        let mtm = self.mtm();
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseDirectories(true);
        panel.setCanChooseFiles(false);
        panel.setAllowsMultipleSelection(false);
        panel.setCanCreateDirectories(true);
        panel.setPrompt(Some(ns_string!("Choose")));
        let home = child::home();
        if let Some(folder) = bt_core::expand_home(&current, home.as_deref()) {
            panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(
                &folder.to_string_lossy(),
            ))));
        }
        let chosen = panel.clone();
        let answered = RcBlock::new(move |response: NSModalResponse| {
            if response != NSModalResponseOK {
                return;
            }
            let Some(folder) = chosen
                .URLs()
                .firstObject()
                .and_then(|url| url.path())
                .map(|path| std::path::PathBuf::from(path.to_string()))
            else {
                return;
            };
            let text = folder_text(&folder, home.as_deref());
            let edit = match key {
                Key::PreviewDir => SettingsEdit::PreviewDir(text),
                _ => SettingsEdit::DownloadDir(text),
            };
            // audit: the panel's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the panel block is on the main thread");
            if let Some(delegate) = app::delegate(mtm) {
                delegate.save_edit(&edit);
            }
        });
        panel.beginSheetModalForWindow_completionHandler(window, &answered);
    }

    /// Ends the search: the field is emptied and the open page comes back.
    fn clear_search(&self) {
        if self.ivars().query.borrow().is_empty() {
            return;
        }
        self.ivars().query.borrow_mut().clear();
        if let Some(field) = self.ivars().search.get() {
            field.setStringValue(ns_string!(""));
        }
    }

    /// Whether a query is being shown.
    fn searching(&self) -> bool {
        !self.ivars().query.borrow().trim().is_empty()
    }

    /// Lists what the query finds, one box of rows with the page each is on; nothing found says so.
    fn rebuild_results(&self) {
        let Some(results) = self.ivars().results.get() else {
            return;
        };
        for view in results.arrangedSubviews().iter() {
            results.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }
        let mtm = self.mtm();
        let query = self.ivars().query.borrow().clone();
        let entries = self.ivars().entries.borrow();
        let hits = search::hits(&query, &entries);
        let mut found = Vec::with_capacity(hits.len());
        if hits.is_empty() {
            if !query.trim().is_empty() {
                let none = text_label(
                    &NSString::from_str(&format!(
                        "No settings match \u{201c}{}\u{201d}.",
                        query.trim()
                    )),
                    mtm,
                );
                none.setTextColor(Some(&look::ink(Ink::Dim)));
                results.addArrangedSubview(&none);
            }
        } else {
            let (boxed, inner) = group_box(mtm);
            for (at, entry) in hits.iter().enumerate() {
                let row = search::ResultRow::new(mtm, entry, at > 0);
                row.setTag(NSInteger::try_from(at).unwrap_or_default());
                // SAFETY: the target is a weak reference to this object, which lives for the
                // whole process; the selector is this class's own.
                unsafe {
                    row.setTarget(Some(self.target()));
                    row.setAction(Some(sel!(resultClicked:)));
                }
                inner.addArrangedSubview(search::as_view(&row));
                search::as_view(&row)
                    .widthAnchor()
                    .constraintEqualToAnchor(&inner.widthAnchor())
                    .setActive(true);
                found.push((entry.page, entry.key));
            }
            results.addArrangedSubview(&boxed);
            boxed
                .widthAnchor()
                .constraintEqualToAnchor(&results.widthAnchor())
                .setActive(true);
        }
        drop(entries);
        self.ivars().found.replace(found);
    }

    /// Opens `category`'s page and brings the row of `key` into view, with a short glow on it.
    fn open_row(&self, category: Category, key: Option<Key>) {
        let Some(sidebar) = self.ivars().sidebar.get() else {
            return;
        };
        self.clear_search();
        self.rebuild_results();
        sidebar.selectRowIndexes_byExtendingSelection(
            &NSIndexSet::indexSetWithIndex(SideItem::row_of(category)),
            false,
        );
        // Selecting the row that is already open changes nothing, so the page is shown here.
        self.show_selected();
        let (Some(key), Some(c), Some(scroll)) = (
            key,
            self.ivars().controls.get(),
            self.ivars().pane_scroll.get(),
        ) else {
            return;
        };
        let Some(row) = c.rows.iter().find(|row| row.key == Some(key)) else {
            return;
        };
        let Some(document) = scroll.documentView() else {
            return;
        };
        document.layoutSubtreeIfNeeded();
        let rect = row
            .view
            .convertRect_toView(row.view.bounds(), Some(&document));
        // A row's own band plus some air, so its group's title is not cut off.
        let wanted = NSRect::new(
            NSPoint::new(rect.origin.x, (rect.origin.y - 40.0).max(0.0)),
            NSSize::new(rect.size.width, rect.size.height + 56.0),
        );
        document.scrollRectToVisible(wanted);
        row.glow(self.mtm());
    }

    /// The edit a choice of `key` at `index` writes: the place in the list the control was
    /// filled from, which each key's type or list reads; `None` for a separator or a header.
    fn choice_edit(&self, key: Key, index: NSInteger) -> Option<SettingsEdit> {
        match key {
            Key::ConfirmClose => choice_at(index).map(SettingsEdit::ConfirmClose),
            Key::RestoreWindows => choice_at(index).map(SettingsEdit::RestoreWindows),
            Key::KeepRunning => choice_at(index).map(SettingsEdit::KeepRunning),
            Key::Scrollbar => choice_at(index).map(SettingsEdit::Scrollbar),
            Key::ContentEdge => choice_at(index).map(SettingsEdit::ContentEdge),
            Key::SplitStyle => choice_at(index).map(SettingsEdit::SplitStyle),
            Key::TabDetails => choice_at(index).map(SettingsEdit::TabDetails),
            Key::TabIconColors => choice_at(index).map(SettingsEdit::IconColors),
            Key::TabRunning => choice_at(index).map(SettingsEdit::TabRunning),
            Key::TabPosition => choice_at(index).map(SettingsEdit::TabPosition),
            Key::ShellIntegration => choice_at(index).map(SettingsEdit::ShellIntegration),
            Key::CommandLevel => choice_at(index).map(SettingsEdit::CommandLevel),
            Key::Shape => choice_at(index).map(SettingsEdit::Cursor),
            Key::Blink => choice_at(index).map(SettingsEdit::CursorBlink),
            Key::Unfocused => choice_at(index).map(SettingsEdit::CursorUnfocused),
            Key::CursorMotion => choice_at(index).map(SettingsEdit::CursorMotion),
            Key::ReduceMotion => choice_at(index).map(SettingsEdit::ReduceMotion),
            Key::Keypress => choice_at(index).map(SettingsEdit::Keypress),
            Key::Erase => choice_at(index).map(SettingsEdit::Erase),
            Key::DockArrival => choice_at(index).map(SettingsEdit::DockArrival),
            Key::Font => {
                font_edit(&self.ivars().fonts.borrow(), index).map(SettingsEdit::FontFamily)
            }
            Key::MinimumContrast => contrast_edit(&self.ivars().contrasts.borrow(), index)
                .map(SettingsEdit::MinimumContrast),
            Key::PreviewMaxSize => {
                size_edit(&self.ivars().max_sizes.borrow(), index).map(SettingsEdit::PreviewMaxSize)
            }
            Key::PreviewLimit => {
                size_edit(&self.ivars().limits.borrow(), index).map(SettingsEdit::PreviewLimit)
            }
            Key::PreviewKeep => choice_at(index).map(SettingsEdit::PreviewKeep),
            Key::DownloadConflict => choice_at(index).map(SettingsEdit::DownloadConflict),
            Key::RemoteStats => choice_at(index).map(SettingsEdit::RemoteStats),
            _ => None,
        }
    }

    fn save(&self, edit: Option<SettingsEdit>) {
        let Some(delegate) = app::delegate(self.mtm()) else {
            return;
        };
        match edit {
            Some(edit) => delegate.save_edit(&edit),
            // A separator or an unrecognised control: the on-screen selection
            // must not diverge from the file.
            None => delegate.refresh_settings_window(),
        }
    }

    /// Ties the panes' scroll view under the banner or under the header. The old set is
    /// released first: if both were active for a moment they would conflict.
    fn layout_panes(&self, banner_shown: bool) {
        let Some(tops) = self.ivars().pane_tops.get() else {
            return;
        };
        let (on, off) = if banner_shown {
            (&tops.under_banner, &tops.under_header)
        } else {
            (&tops.under_header, &tops.under_banner)
        };
        for constraint in off {
            constraint.setActive(false);
        }
        activate(on);
    }

    /// Changes the header and the grid according to the sidebar's selection.
    fn show_selected(&self) {
        let (Some(sidebar), Some(header), Some(panes)) = (
            self.ivars().sidebar.get(),
            self.ivars().header.get(),
            self.ivars().panes.get(),
        ) else {
            return;
        };
        let selected = sidebar.selectedRow();
        let Some(SideItem::Page(category)) = SideItem::at(selected) else {
            return;
        };
        let searching = self.searching();
        // The rows draw their own selection: tell each whether it is the open page.
        for row in 0..SideItem::ALL.len() {
            let at = NSInteger::try_from(row).unwrap_or_default();
            let view = sidebar.viewAtColumn_row_makeIfNecessary(0, at, false);
            if let Some(view) = view
                .as_deref()
                .and_then(|view| view.downcast_ref::<SideRow>())
            {
                view.set_selected(at == selected && !searching);
            }
        }
        // A search shows its results in the last pane, whatever page is open behind it.
        header.setStringValue(&NSString::from_str(if searching {
            "Search"
        } else {
            category.title()
        }));
        let grid = if searching {
            panes.len().checked_sub(1)
        } else {
            category.grid()
        };
        for (i, pane) in panes.iter().enumerate() {
            pane.setHidden(grid != Some(i));
        }
        if let Some(scroll) = self.ivars().pane_scroll.get() {
            scroll.setHidden(grid.is_none());
        }
        if let Some(keyboard) = self.ivars().keyboard.get() {
            let shown = category == Category::Keyboard && !searching;
            keyboard.setHidden(!shown);
            if shown {
                keyboard.appear();
            }
        }
        if let Some(bottoms) = self.ivars().pane_bottoms.get() {
            for (i, bottom) in bottoms.iter().enumerate() {
                if grid != Some(i) {
                    bottom.setActive(false);
                }
            }
            if let Some(bottom) = grid.and_then(|i| bottoms.get(i)) {
                bottom.setActive(true);
            }
        }
        if let Some(scroll) = self.ivars().pane_scroll.get() {
            let clip = scroll.contentView();
            clip.scrollToPoint(NSPoint::new(0.0, 0.0));
            scroll.reflectScrolledClipView(&clip);
        }
        self.show_specimen();
    }

    /// Draws the open page's example from what the last refresh gave it; a page that is not open,
    /// the search results and a page without an example draw nothing.
    fn show_specimen(&self) {
        let inputs = self.ivars().specimen_inputs.borrow().clone();
        self.show_specimen_with(inputs.as_ref());
    }

    /// The open page's example drawn while a slider is dragged: with the value under the hand for
    /// `key`, the file's for the rest. The file is written on release, and the refresh that
    /// follows draws the example from it again.
    fn preview_specimen(&self, key: Key, value: f64) {
        let Some(mut inputs) = self.ivars().specimen_inputs.borrow().clone() else {
            return;
        };
        let settings = &mut inputs.settings;
        match key {
            Key::BlinkSpeed => settings.blink_interval = value,
            Key::Radius => settings.caret.radius_ratio = value,
            Key::Glow => settings.caret.glow = value,
            _ => return,
        }
        self.show_specimen_with(Some(&inputs));
    }

    /// [`SettingsWindow::show_specimen`] from `inputs`.
    fn show_specimen_with(&self, inputs: Option<&specimen::Inputs>) {
        let (Some(sidebar), Some(specimens)) =
            (self.ivars().sidebar.get(), self.ivars().specimens.get())
        else {
            return;
        };
        // A blink belongs to the Cursor page as it was drawn: whatever is shown now starts it
        // again or leaves it stopped.
        self.cancel_blink();
        specimens.stop_blink();
        let settle = || {
            if let Some(sticky) = self.ivars().sticky.get() {
                sticky.settle(self.pages_scrolled());
            }
        };
        let shown = match SideItem::at(sidebar.selectedRow()) {
            Some(SideItem::Page(category)) if !self.searching() => {
                specimen::Kind::of(category).zip(inputs)
            }
            _ => None,
        };
        // Only the open page's example shows; it is not hidden on the way, or the rows under it
        // would move up and back and lose where they were scrolled.
        specimens.hide_except(shown.map(|(kind, _)| kind));
        let Some((kind, inputs)) = shown else {
            settle();
            return;
        };
        let scale = self
            .ivars()
            .window
            .get()
            .map_or(2.0, |window| window.backingScaleFactor());
        // As wide as the page's groups. The place's width is the window's, whatever it holds; a
        // window not laid out yet is laid out first.
        let width = self.ivars().sticky.get().map_or(0.0, |sticky| {
            if sticky.stack.frame().size.width < 1.0
                && let Some(content) = self
                    .ivars()
                    .window
                    .get()
                    .and_then(|window| window.contentView())
            {
                content.layoutSubtreeIfNeeded();
            }
            sticky.stack.frame().size.width
        });
        specimens.show(kind, inputs, scale, width);
        settle();
        // The cursor rows' options are the renderer's caret too, drawn with the page like its
        // example.
        if kind == specimen::Kind::Cursor
            && let Some(controls) = self.ivars().controls.get()
        {
            let stills = specimens.caret_stills(inputs, scale);
            controls.shape.set_stills(&stills);
            controls.unfocused.set_stills(&stills);
        }
        if let Some(half) = specimens.blink_half() {
            self.blink_after(half);
        }
    }

    /// Whether the pages are scrolled off their top: rows are under the example.
    fn pages_scrolled(&self) -> bool {
        self.ivars()
            .pane_scroll
            .get()
            .is_some_and(|scroll| scroll.contentView().bounds().origin.y > 0.5)
    }

    /// Turns the example's blinking caret after `delay` seconds, in every run-loop mode: a
    /// slider being dragged keeps the blink going.
    fn blink_after(&self, delay: f64) {
        let modes = NSArray::from_slice(&[
            // SAFETY: Foundation's constant mode name, alive for the process.
            unsafe { NSRunLoopCommonModes },
        ]);
        let this: &NSObject = self.as_super();
        // SAFETY: `blinkSpecimen:` is this object's own method and takes an object argument,
        // nil here; the request retains the target until it runs or is cancelled.
        unsafe {
            this.performSelector_withObject_afterDelay_inModes(
                sel!(blinkSpecimen:),
                None,
                delay,
                &modes,
            );
        }
    }

    /// Drops the example's waiting blink turn, if any.
    fn cancel_blink(&self) {
        // SAFETY: the target is this object and the selector its own; the argument is the nil
        // the request was made with.
        unsafe {
            NSObject::cancelPreviousPerformRequestsWithTarget_selector_object(
                self.as_ref(),
                sel!(blinkSpecimen:),
                None,
            );
        }
    }

    fn target(&self) -> &AnyObject {
        self.as_ref()
    }

    /// Wires a control to this object's action.
    fn wire(&self, control: &NSControl, key: Key, action: Sel) {
        control.setTag(key.tag());
        // SAFETY: the target is a weak reference; this object lives for the
        // whole process in `AppDelegate`'s ivar.
        unsafe {
            control.setTarget(Some(self.target()));
            control.setAction(Some(action));
        }
    }

    fn popup<T: Choice>(&self, key: Key) -> Retained<NSPopUpButton> {
        let popup = new_popup(self.mtm());
        for title in choice_titles::<T>() {
            popup.addItemWithTitle(&NSString::from_str(title));
        }
        self.wire(&popup, key, sel!(popupChanged:));
        popup
    }

    fn string_popup(&self, key: Key) -> Retained<NSPopUpButton> {
        let popup = new_popup(self.mtm());
        self.wire(&popup, key, sel!(popupChanged:));
        popup
    }

    /// A pictured choice: one picture for each of `T`'s options, in `NAMES` order.
    fn picture_choice<T: Pictured>(&self, key: Key) -> Retained<PictureChoice> {
        let items = T::names()
            .iter()
            .map(|&(_, value)| (value.title(), value.picture()))
            .collect();
        let choice = PictureChoice::new(self.mtm(), items, Palette::of(&Theme::BATERI));
        self.wire(&choice, key, sel!(pictureChanged:));
        choice
    }

    /// A segmented choice: one segment for each of `T`'s options, in `NAMES` order.
    fn segmented<T: Choice>(&self, key: Key) -> Retained<NSSegmentedControl> {
        let labels = NSArray::from_retained_slice(
            &choice_titles::<T>()
                .into_iter()
                .map(NSString::from_str)
                .collect::<Vec<_>>(),
        );
        // SAFETY: the target is a weak reference to this object, which lives for the whole
        // process in `AppDelegate`'s ivar; the selector is this class's own.
        let control = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &labels,
                NSSegmentSwitchTracking::SelectOne,
                Some(self.target()),
                Some(sel!(segmentChanged:)),
                self.mtm(),
            )
        };
        control.setIdentifier(Some(&NSString::from_str(&key.tag().to_string())));
        control.setSelectedSegmentBezelColor(Some(&look::ink(Ink::Accent)));
        control
    }

    /// Theme's two ways: Match System, or one theme in light and dark.
    fn theme_mode(&self) -> Retained<NSSegmentedControl> {
        let labels = NSArray::from_retained_slice(&[
            NSString::from_str("Match System"),
            NSString::from_str("One Theme"),
        ]);
        // SAFETY: the target is a weak reference to this object, which lives for the whole
        // process in `AppDelegate`'s ivar; the selector is this class's own.
        let control = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &labels,
                NSSegmentSwitchTracking::SelectOne,
                Some(self.target()),
                Some(sel!(themeModeChanged:)),
                self.mtm(),
            )
        };
        control.setTag(Key::Theme.tag());
        control.setSelectedSegmentBezelColor(Some(&look::ink(Ink::Accent)));
        control
    }

    /// A gallery of themes for `key`'s row; a card clicked is written by `themeChosen:`.
    fn theme_gallery(&self, key: Key) -> Retained<ThemeGallery> {
        let gallery = ThemeGallery::new(self.mtm());
        self.wire(&gallery, key, sel!(themeChosen:));
        gallery
    }

    /// A push button with this object's `action`; `key` only when the button
    /// belongs to a row (its tag names the row).
    fn button(&self, title: &str, key: Option<Key>, action: Sel) -> Retained<NSButton> {
        // SAFETY: the target is a weak reference and lives for the whole
        // process; the selector is one of this class's actions.
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(title),
                Some(self.target()),
                Some(action),
                self.mtm(),
            )
        };
        if let Some(key) = key {
            button.setTag(key.tag());
        }
        button
    }

    /// A folder row's path label and Change… button.
    fn folder(&self, key: Key) -> Folder {
        let path = text_label(ns_string!(""), self.mtm());
        path.setTextColor(Some(&look::ink(Ink::Dim)));
        path.setLineBreakMode(NSLineBreakMode::ByTruncatingHead);
        path.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(12.0, 0.0)));
        path.widthAnchor()
            .constraintLessThanOrEqualToConstant(PATH_WIDTH)
            .setActive(true);
        let change = self.button("Change…", Some(key), sel!(chooseFolder:));
        Folder { path, change }
    }

    fn switch(&self, key: Key) -> Retained<ThemeSwitch> {
        let switch = ThemeSwitch::new(self.mtm());
        self.wire(&switch, key, sel!(switchChanged:));
        switch
    }

    fn number(&self, key: Key, min: f64, max: f64, step: f64, width: f64) -> Number {
        let mtm = self.mtm();
        let field = NSTextField::new(mtm);
        field.setAlignment(objc2_app_kit::NSTextAlignment::Right);
        width_constraint(&field, width);
        self.wire(&field, key, sel!(fieldChanged:));
        if let Some(cell) = field.cell() {
            // An action on leaving focus too: "on Enter or on
            // leaving focus".
            cell.setSendsActionOnEndEditing(true);
        }
        let stepper = NSStepper::new(mtm);
        stepper.setMinValue(min);
        stepper.setMaxValue(max);
        stepper.setIncrement(step);
        stepper.setValueWraps(false);
        self.wire(&stepper, key, sel!(stepperChanged:));
        Number {
            field,
            stepper,
            range: (min, max),
            shown: RefCell::new((0.0, String::new())),
        }
    }

    fn slide(&self, key: Key, min: f64, max: f64) -> Slide {
        let mtm = self.mtm();
        let slider = NSSlider::new(mtm);
        // The filled part of the track is the value: the accent.
        slider.setTrackFillColor(Some(&look::ink(Ink::Accent)));
        slider.setMinValue(min);
        slider.setMaxValue(max);
        slider.setContinuous(true);
        width_constraint(&slider, SLIDER_WIDTH);
        self.wire(&slider, key, sel!(sliderChanged:));
        let value = text_label(ns_string!(""), mtm);
        value.setTextColor(Some(&look::ink(Ink::Dim)));
        value.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
            NSFont::systemFontSize(),
            0.0,
        )));
        Slide { slider, value }
    }

    /// The window's skeleton and all the controls.
    fn build(&self) {
        let mtm = self.mtm();
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), WINDOW_SIZE);
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable
            | NSWindowStyleMask::FullSizeContentView;
        // SAFETY: with defer=false the window is created immediately;
        // `releasedWhenClosed` is turned off right below (the terminal window's reason).
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: only changes the ownership semantics; we are the Retained's owner.
        // Closing hides, reopening returns on the same category.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(ns_string!("Settings"));
        window.setDelegate(Some(ProtocolObject::from_ref(self)));
        // The title is in the pane's header; the window title is for the Window menu.
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setTitlebarAppearsTransparent(true);
        // A page stands on the theme's ground, the head row with it.
        window.setBackgroundColor(Some(&look::ink(Ink::Ground)));
        // ⌘T must not add a tab to the settings window.
        window.setTabbingMode(NSWindowTabbingMode::Disallowed);

        let sidebar = self.build_sidebar();
        let detail = self.build_detail();

        let sidebar_controller = NSViewController::new(mtm);
        sidebar_controller.setView(&sidebar);
        let detail_controller = NSViewController::new(mtm);
        detail_controller.setView(&detail);
        let sidebar_item = NSSplitViewItem::sidebarWithViewController(&sidebar_controller);
        sidebar_item.setCanCollapse(false);
        sidebar_item.setMinimumThickness(SIDEBAR_WIDTH);
        sidebar_item.setMaximumThickness(SIDEBAR_WIDTH);
        let detail_item = NSSplitViewItem::splitViewItemWithViewController(&detail_controller);
        let split = NSSplitViewController::new(mtm);
        split.addSplitViewItem(&sidebar_item);
        split.addSplitViewItem(&detail_item);
        window.setContentViewController(Some(&split));
        window.setContentSize(WINDOW_SIZE);
        // Only the height moves: the grids' column widths and the wrap
        // widths (`BANNER_TEXT_WIDTH`, `NOTE_WIDTH`) derive from the fixed width.
        window.setContentMinSize(NSSize::new(WINDOW_SIZE.width, MIN_WINDOW_HEIGHT));
        window.setContentMaxSize(NSSize::new(WINDOW_SIZE.width, f64::MAX));
        if let Some(table) = self.ivars().sidebar.get() {
            window.setInitialFirstResponder(Some(table));
        }
        let _ = self.ivars().window.set(window);

        if let Some(table) = self.ivars().sidebar.get() {
            table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(0), false);
        }
        self.show_selected();
    }

    fn build_sidebar(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let table = NSTableView::new(mtm);
        table.setStyle(NSTableViewStyle::Plain);
        // The rows draw their own selection: a pale face, and the page's tile in the accent.
        table.setSelectionHighlightStyle(NSTableViewSelectionHighlightStyle::None);
        table.setBackgroundColor(&NSColor::clearColor());
        table.setHeaderView(None);
        table.setAllowsEmptySelection(false);
        table.setIntercellSpacing(NSSize::new(0.0, 1.0));
        let column = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), ns_string!("c"));
        table.addTableColumn(&column);
        // SAFETY: the source, delegate and target are weak references; this object
        // lives for the whole process (`AppDelegate`'s ivar); the selector is this class's
        // `sidebarClicked:`.
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(self)));
            table.setDelegate(Some(ProtocolObject::from_ref(self)));
            table.setTarget(Some(self.target()));
            table.setAction(Some(sel!(sidebarClicked:)));
        }
        let scroll = NSScrollView::new(mtm);
        scroll.setDocumentView(Some(&table));
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(false);

        // "Open settings.toml" sits at the foot of the column; a locked file puts it, as the
        // default button, in the strip instead.
        // SAFETY: the target is a weak reference and lives for the whole process; the selector is
        // this class's `openFile:`.
        let open = unsafe {
            NSButton::buttonWithTitle_target_action(
                ns_string!("Open settings.toml"),
                Some(self.target()),
                Some(sel!(openFile:)),
                mtm,
            )
        };
        let search = NSSearchField::new(mtm);
        search.setPlaceholderString(Some(ns_string!("Search")));
        search.setSendsSearchStringImmediately(true);
        // SAFETY: the target is a weak reference and lives for the whole process; the selector is
        // this class's `searchSettings:`.
        unsafe {
            search.setTarget(Some(self.target()));
            search.setAction(Some(sel!(searchSettings:)));
        }
        let side = NSView::new(mtm);
        // The column stands on the theme's band, over the system's material.
        let ground = surface(mtm, Ink::Band);
        add_pinned(&side, &ground);
        pin_to_edges(&ground, &side);
        add_pinned(&side, &search);
        add_pinned(&side, &scroll);
        add_pinned(&side, &open);
        let safe = side.safeAreaLayoutGuide();
        activate(&[
            search
                .topAnchor()
                .constraintEqualToAnchor_constant(&safe.topAnchor(), 6.0),
            search
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&side.leadingAnchor(), SIDE_PAD),
            search
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&side.trailingAnchor(), -SIDE_PAD),
            scroll
                .topAnchor()
                .constraintEqualToAnchor_constant(&search.bottomAnchor(), 8.0),
            scroll
                .leadingAnchor()
                .constraintEqualToAnchor(&side.leadingAnchor()),
            scroll
                .trailingAnchor()
                .constraintEqualToAnchor(&side.trailingAnchor()),
            scroll
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&open.topAnchor(), -8.0),
            open.leadingAnchor()
                .constraintEqualToAnchor_constant(&side.leadingAnchor(), SIDE_PAD),
            open.trailingAnchor()
                .constraintEqualToAnchor_constant(&side.trailingAnchor(), -SIDE_PAD),
            open.bottomAnchor()
                .constraintEqualToAnchor_constant(&side.bottomAnchor(), -12.0),
        ]);
        let _ = self.ivars().side_open.set(open);
        let _ = self.ivars().search.set(search);
        let _ = self.ivars().sidebar.set(table);
        side
    }

    fn build_detail(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let detail = NSView::new(mtm);
        // The pages stand on the theme's ground, as the terminal's cards do.
        let ground = surface(mtm, Ink::Ground);
        add_pinned(&detail, &ground);
        pin_to_edges(&ground, &detail);

        // The page's name: the row 52 points high at the top, which the traffic lights share.
        let header = text_label(ns_string!(""), mtm);
        header.setFont(Some(&NSFont::boldSystemFontOfSize(15.0)));
        add_pinned(&detail, &header);
        activate(&[
            header
                .centerYAnchor()
                .constraintEqualToAnchor_constant(&detail.topAnchor(), HEAD_HEIGHT / 2.0),
            header
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&detail.leadingAnchor(), 24.0),
        ]);

        let banner = BannerView::new(mtm, self.target());
        add_pinned(&detail, &banner.frame);
        activate(&[
            banner
                .frame
                .topAnchor()
                .constraintEqualToAnchor_constant(&detail.topAnchor(), HEAD_HEIGHT),
            banner
                .frame
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&detail.leadingAnchor(), MARGIN),
            banner
                .frame
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&detail.trailingAnchor(), -MARGIN),
        ]);

        // The panes' top is tied to one of two places: to the head row when there is no banner, to
        // the banner when there is ([`SettingsWindow::layout_panes`]). A hidden banner takes no
        // room, so the panes move back up. The pages live in a flipped document inside a vertical
        // scroll view that fills the space below.
        let (mut panes, controls) = self.build_panes();
        // The results of a search are one more pane, the last: shown in place of the open page.
        let results = stack(
            mtm,
            &[],
            NSUserInterfaceLayoutOrientation::Vertical,
            GROUP_SPACING,
            NSLayoutAttribute::Leading,
        );
        panes.push(results.clone());
        let _ = self.ivars().results.set(results);
        let scroll = NSScrollView::new(mtm);
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setHasHorizontalScroller(false);
        scroll.setAutohidesScrollers(true);
        add_pinned(&detail, &scroll);
        let document = PaneDocument::new(mtm);
        document.setTranslatesAutoresizingMaskIntoConstraints(false);
        scroll.setDocumentView(Some(&document));
        let clip = scroll.contentView();
        activate(&[
            scroll
                .leadingAnchor()
                .constraintEqualToAnchor(&detail.leadingAnchor()),
            scroll
                .trailingAnchor()
                .constraintEqualToAnchor(&detail.trailingAnchor()),
            scroll
                .bottomAnchor()
                .constraintEqualToAnchor(&detail.bottomAnchor()),
            document
                .topAnchor()
                .constraintEqualToAnchor(&clip.topAnchor()),
            document
                .leadingAnchor()
                .constraintEqualToAnchor(&clip.leadingAnchor()),
            document
                .trailingAnchor()
                .constraintEqualToAnchor(&clip.trailingAnchor()),
        ]);
        let mut pane_bottoms = Vec::new();
        for pane in &panes {
            add_pinned(&document, pane);
            activate(&[
                pane.leadingAnchor()
                    .constraintEqualToAnchor_constant(&document.leadingAnchor(), PAGE_MARGIN),
                pane.trailingAnchor()
                    .constraintEqualToAnchor_constant(&document.trailingAnchor(), -PAGE_MARGIN),
                pane.topAnchor()
                    .constraintEqualToAnchor_constant(&document.topAnchor(), 6.0),
            ]);
            pane_bottoms.push(
                document
                    .bottomAnchor()
                    .constraintEqualToAnchor_constant(&pane.bottomAnchor(), PAGE_BOTTOM),
            );
        }
        // The open page's example stands still above its rows: the rows scroll under it, so a
        // change made far down the page shows in the example as it is made. It is as wide as the
        // page's groups, and a hairline under it marks the edge the rows scroll under.
        let sticky = stack(
            mtm,
            &[],
            NSUserInterfaceLayoutOrientation::Vertical,
            0.0,
            NSLayoutAttribute::Leading,
        );
        add_pinned(&detail, &sticky);
        if let Some(specimens) = self.ivars().specimens.get() {
            for view in specimens.views() {
                sticky.addArrangedSubview(view);
                view.widthAnchor()
                    .constraintEqualToAnchor(&sticky.widthAnchor())
                    .setActive(true);
                view.setHidden(true);
            }
        }
        let sticky_gap = scroll
            .topAnchor()
            .constraintEqualToAnchor_constant(&sticky.bottomAnchor(), 0.0);
        activate(&[
            sticky
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&detail.leadingAnchor(), PAGE_MARGIN),
            sticky
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&detail.trailingAnchor(), -PAGE_MARGIN),
            sticky_gap.clone(),
        ]);
        let rule = surface(mtm, Ink::Line);
        add_pinned(&detail, &rule);
        activate(&[
            rule.topAnchor()
                .constraintEqualToAnchor(&scroll.topAnchor()),
            rule.leadingAnchor()
                .constraintEqualToAnchor(&detail.leadingAnchor()),
            rule.trailingAnchor()
                .constraintEqualToAnchor(&detail.trailingAnchor()),
            rule.heightAnchor().constraintEqualToConstant(1.0),
        ]);
        rule.setHidden(true);
        clip.setPostsBoundsChangedNotifications(true);
        // SAFETY: the observer is this object, which lives for the whole process, and the
        // selector is its own `pagesScrolled:`, taking the notification.
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                self.as_ref(),
                sel!(pagesScrolled:),
                Some(objc2_app_kit::NSViewBoundsDidChangeNotification),
                Some(&clip),
            );
        }
        let under_header = vec![
            sticky
                .topAnchor()
                .constraintEqualToAnchor_constant(&detail.topAnchor(), HEAD_HEIGHT),
        ];
        let under_banner = vec![
            sticky
                .topAnchor()
                .constraintEqualToAnchor_constant(&banner.frame.bottomAnchor(), 8.0),
        ];
        activate(&under_header);
        let _ = self.ivars().sticky.set(Sticky {
            stack: sticky,
            gap: sticky_gap,
            rule,
        });
        banner.frame.setHidden(true);

        // Keyboard is a table of its own, in the place the pages' scroll view has.
        let keyboard = KeyboardPage::new(mtm);
        detail.addSubview(&keyboard);
        activate(&[
            keyboard
                .topAnchor()
                .constraintEqualToAnchor(&scroll.topAnchor()),
            keyboard
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&detail.bottomAnchor(), -16.0),
            keyboard
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&detail.leadingAnchor(), PAGE_MARGIN),
            keyboard
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&detail.trailingAnchor(), -PAGE_MARGIN),
        ]);
        keyboard.setHidden(true);
        let _ = self.ivars().keyboard.set(keyboard);

        let _ = self.ivars().header.set(header);
        let _ = self.ivars().banner.set(banner);
        let _ = self.ivars().pane_tops.set(PaneTops {
            under_header,
            under_banner,
        });
        let _ = self.ivars().pane_scroll.set(scroll);
        let _ = self.ivars().pane_bottoms.set(pane_bottoms);
        let _ = self.ivars().panes.set(panes);
        let _ = self.ivars().controls.set(controls);
        detail
    }

    /// The page stacks, one per category with a grid (Keyboard is a table of its own), and the
    /// controls: every control is made here, the rows around them come from [`pages`].
    fn build_panes(&self) -> (Vec<Retained<NSStackView>>, Controls) {
        let mtm = self.mtm();

        let confirm_close = self.popup::<ConfirmClose>(Key::ConfirmClose);
        let restore_windows = self.popup::<RestoreWindows>(Key::RestoreWindows);
        let keep_running = self.popup::<KeepRunning>(Key::KeepRunning);
        let clipboard = self.switch(Key::Clipboard);
        let scrollback = self.number(Key::Scrollback, 0.0, SCROLLBACK_MAX as f64, 1000.0, 76.0);
        let shell_integration = self.popup::<ShellIntegration>(Key::ShellIntegration);
        let ports = self.switch(Key::Ports);
        let python_env = self.switch(Key::PythonEnv);
        let node_version = self.switch(Key::NodeVersion);
        let command_level = self.popup::<CommandLevel>(Key::CommandLevel);

        let theme_mode = self.theme_mode();
        let theme = self.theme_gallery(Key::Theme);
        let light_theme = self.theme_gallery(Key::LightTheme);
        let dark_theme = self.theme_gallery(Key::DarkTheme);
        let minimum_contrast = self.string_popup(Key::MinimumContrast);
        let font = self.string_popup(Key::Font);
        // Half a point a step: the size is a decimal, and a whole point is a large step at the
        // sizes a terminal is read at.
        let size = self.number(Key::Size, MIN_SIZE, MAX_SIZE, 0.5, NUMBER_WIDTH);
        let line_height = self.number(
            Key::LineHeight,
            *LINE_HEIGHT_RANGE.start(),
            *LINE_HEIGHT_RANGE.end(),
            0.1,
            NUMBER_WIDTH,
        );
        let letter_spacing = self.number(
            Key::LetterSpacing,
            *LETTER_SPACING_RANGE.start(),
            *LETTER_SPACING_RANGE.end(),
            0.05,
            NUMBER_WIDTH,
        );
        let scrollbar = self.popup::<Scrollbar>(Key::Scrollbar);
        let content_edge = self.picture_choice::<ContentEdge>(Key::ContentEdge);
        let dim_splits = self.switch(Key::DimSplits);
        let focus_frame = self.switch(Key::FocusFrame);
        let split_style = self.picture_choice::<SplitStyle>(Key::SplitStyle);
        let tab_details = self.segmented::<TabDetails>(Key::TabDetails);
        let resize_in_steps = self.switch(Key::ResizeInSteps);
        let tab_icons = self.switch(Key::TabIcons);
        let font_smoothing = self.switch(Key::FontSmoothing);
        let tab_icon_colors = self.segmented::<IconColors>(Key::TabIconColors);
        let tab_running = self.picture_choice::<TabRunning>(Key::TabRunning);
        let tab_position = self.picture_choice::<TabPosition>(Key::TabPosition);
        // The font's own options: its switches are rebuilt on every refresh (a new font is other
        // options), and the group is hidden when the font offers none.
        let font_options = font_options::Options::new(font_options::Options::stack(mtm));

        let shape = self.picture_choice::<CaretShape>(Key::Shape);
        let blink = self.popup::<CursorBlink>(Key::Blink);
        let blink_speed = self.slide(Key::BlinkSpeed, 0.0, 1.0);
        let radius = self.slide(
            Key::Radius,
            *CURSOR_RADIUS_RANGE.start(),
            *CURSOR_RADIUS_RANGE.end(),
        );
        let glow = self.slide(
            Key::Glow,
            *CURSOR_GLOW_RANGE.start(),
            *CURSOR_GLOW_RANGE.end(),
        );
        let unfocused = self.picture_choice::<UnfocusedCaret>(Key::Unfocused);

        let cursor_motion = self.segmented::<CursorMotion>(Key::CursorMotion);
        let smooth_scroll = self.switch(Key::SmoothScroll);
        let scroll_bounce = self.switch(Key::ScrollBounce);
        let reduce_motion = self.popup::<ReduceMotion>(Key::ReduceMotion);
        let keypress = self.popup::<Keypress>(Key::Keypress);
        let erase = self.popup::<Erase>(Key::Erase);
        let dock_arrival = self.popup::<DockArrival>(Key::DockArrival);

        let remote_integration = self.switch(Key::RemoteIntegration);
        let preview_max_size = self.string_popup(Key::PreviewMaxSize);
        let preview_read_only = self.switch(Key::PreviewReadOnly);
        let preview_dir = self.folder(Key::PreviewDir);
        let show_previews = self.button("Show in Finder", None, sel!(showPreviewFolder:));
        let preview_keep = self.popup::<PreviewKeep>(Key::PreviewKeep);
        let preview_limit = self.string_popup(Key::PreviewLimit);
        let usage = text_label(&NSString::from_str(&usage_label(None)), mtm);
        usage.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
            NSFont::systemFontSize(),
            0.0,
        )));
        usage.setTextColor(Some(&look::ink(Ink::Dim)));
        let clear = self.button("Clear Now", None, sel!(clearPreviews:));
        let download_dir = self.folder(Key::DownloadDir);
        let download_conflict = self.popup::<DownloadConflict>(Key::DownloadConflict);
        let download_notify = self.switch(Key::DownloadNotify);
        let remote_stats = self.popup::<RemoteStatsMode>(Key::RemoteStats);
        let stats_interval = self.number(
            Key::StatsInterval,
            f64::from(*STATS_INTERVAL_RANGE.start()),
            f64::from(*STATS_INTERVAL_RANGE.end()),
            1.0,
            NUMBER_WIDTH,
        );

        let mut controls = Controls {
            confirm_close,
            restore_windows,
            keep_running,
            clipboard,
            scrollback,
            shell_integration,
            command_level,
            theme_mode,
            theme,
            light_theme,
            dark_theme,
            font,
            font_options,
            size,
            line_height,
            letter_spacing,
            scrollbar,
            content_edge,
            split_style,
            tab_details,
            tab_icons,
            font_smoothing,
            tab_icon_colors,
            tab_running,
            tab_position,
            minimum_contrast,
            shape,
            blink,
            blink_speed,
            radius,
            glow,
            unfocused,
            cursor_motion,
            smooth_scroll,
            scroll_bounce,
            reduce_motion,
            keypress,
            erase,
            dock_arrival,
            preview_max_size,
            preview_read_only,
            preview_dir,
            show_previews,
            preview_keep,
            preview_limit,
            usage,
            clear,
            download_dir,
            download_conflict,
            download_notify,
            remote_stats,
            stats_interval,
            remote_integration,
            ports,
            python_env,
            node_version,
            dim_splits,
            focus_frame,
            resize_in_steps,
            rows: Vec::new(),
            groups: Vec::new(),
        };

        let mut panes = Vec::new();
        let mut rows = Vec::new();
        let mut groups = Vec::new();
        let specimens = specimen::Specimens::new(mtm);
        for category in Category::ALL {
            if category.grid().is_none() {
                continue;
            }
            let page = stack(
                mtm,
                &[],
                NSUserInterfaceLayoutOrientation::Vertical,
                GROUP_SPACING,
                NSLayoutAttribute::Leading,
            );
            // The page's live example stands above the page, out of its scroll
            // ([`SettingsWindow::build_detail`]).
            for spec in pages::groups(category) {
                let first = rows.len();
                let (boxed, inner) = group_box(mtm);
                for item in spec.items {
                    let row = match *item {
                        pages::Item::Key(key) => {
                            let parts = controls.parts(mtm, key);
                            // A row whose control is a list of switches or of pictures has the width
                            // to itself.
                            build_row(mtm, Some(key), pages::label(key), parts, pages::below(key))
                        }
                        pages::Item::Usage => build_row(
                            mtm,
                            None,
                            "In use",
                            Parts {
                                view: hstack(
                                    mtm,
                                    &[
                                        controls.usage.as_super().as_super(),
                                        controls.clear.as_super().as_super(),
                                    ],
                                    10.0,
                                ),
                                controls: Vec::new(),
                                dim: Vec::new(),
                            },
                            false,
                        ),
                    };
                    inner.addArrangedSubview(&row.view);
                    row.view
                        .widthAnchor()
                        .constraintEqualToAnchor(&inner.widthAnchor())
                        .setActive(true);
                    rows.push(row);
                }
                let (view, aside, title) = group_view(mtm, spec, &boxed);
                page.addArrangedSubview(&view);
                view.widthAnchor()
                    .constraintEqualToAnchor(&page.widthAnchor())
                    .setActive(true);
                groups.push(GroupView {
                    view,
                    aside,
                    title,
                    spec,
                    rows: first..rows.len(),
                });
            }
            panes.push(page);
        }
        controls.rows = rows;
        controls.groups = groups;
        let _ = self.ivars().specimens.set(specimens);
        (panes, controls)
    }
}

define_class!(
    /// The scroll view's document: flipped, so the panes hang from the top and
    /// a pane shorter than the window does not sink to the bottom.
    // SAFETY: NSView has no subclassing requirement; Drop is not implemented.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriSettingsPaneDocument"]
    struct PaneDocument;

    unsafe impl NSObjectProtocol for PaneDocument {}

    impl PaneDocument {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl PaneDocument {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // SAFETY: `init` is NSView's designated initializer (zero frame);
        // the size comes from the constraints.
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

/// The two constraint sets that tie the panes' scroll view's top; one is
/// active.
struct PaneTops {
    under_header: Vec<Retained<NSLayoutConstraint>>,
    under_banner: Vec<Retained<NSLayoutConstraint>>,
}

/// The strip's view: a rounded box with a light fill (orange for a file that cannot be read,
/// red for a write that was refused), a warning symbol on the left, what happened in bold with
/// the lines under it and what to do below them in the secondary colour, and the button that does
/// it at the right. The colours are the system's semantic colours — each correct in the light and
/// dark appearance.
struct BannerView {
    frame: Retained<NSBox>,
    icon: Retained<NSImageView>,
    title: Retained<NSTextField>,
    lines: Retained<NSTextField>,
    hint: Retained<NSTextField>,
    action: Retained<NSButton>,
}

impl BannerView {
    fn new(mtm: MainThreadMarker, target: &AnyObject) -> Self {
        let frame = NSBox::new(mtm);
        frame.setBoxType(NSBoxType::Custom);
        frame.setTitlePosition(NSTitlePosition::NoTitle);
        frame.setCornerRadius(10.0);
        frame.setBorderWidth(1.0);
        frame.setContentViewMargins(NSSize::new(12.0, 10.0));

        let icon = NSImageView::new(mtm);
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            ns_string!("exclamationmark.triangle.fill"),
            None,
        ) {
            icon.setImage(Some(&image));
        }
        let title = wrapping_text_label(ns_string!(""), mtm);
        title.setFont(Some(&NSFont::boldSystemFontOfSize(
            NSFont::smallSystemFontSize() + 1.5,
        )));
        title.setPreferredMaxLayoutWidth(BANNER_TEXT_WIDTH);
        let lines = wrapping_text_label(ns_string!(""), mtm);
        lines.setFont(Some(&NSFont::systemFontOfSize(
            NSFont::smallSystemFontSize() + 1.5,
        )));
        lines.setPreferredMaxLayoutWidth(BANNER_TEXT_WIDTH);
        let hint = wrapping_text_label(ns_string!(""), mtm);
        hint.setFont(Some(&NSFont::systemFontOfSize(
            NSFont::smallSystemFontSize() + 1.5,
        )));
        hint.setTextColor(Some(&look::ink(Ink::Dim)));
        hint.setPreferredMaxLayoutWidth(BANNER_TEXT_WIDTH);

        let text = NSStackView::stackViewWithViews(
            &NSArray::from_slice(&[
                title.as_super().as_super(),
                lines.as_super().as_super(),
                hint.as_super().as_super(),
            ]),
            mtm,
        );
        text.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        text.setAlignment(NSLayoutAttribute::Leading);
        text.setSpacing(2.0);
        // SAFETY: the target is a weak reference and lives for the whole process; the selector
        // is this class's `openFile:`, and `BannerView::show` picks the action for the strip.
        let action = unsafe {
            NSButton::buttonWithTitle_target_action(
                ns_string!("Open settings.toml"),
                Some(target),
                Some(sel!(openFile:)),
                mtm,
            )
        };
        let content = NSView::new(mtm);
        add_pinned(&content, &icon);
        add_pinned(&content, &text);
        add_pinned(&content, &action);
        activate(&[
            icon.leadingAnchor()
                .constraintEqualToAnchor(&content.leadingAnchor()),
            icon.firstBaselineAnchor()
                .constraintEqualToAnchor(&title.firstBaselineAnchor()),
            icon.widthAnchor().constraintEqualToConstant(16.0),
            text.leadingAnchor()
                .constraintEqualToAnchor_constant(&icon.trailingAnchor(), 8.0),
            text.topAnchor()
                .constraintEqualToAnchor(&content.topAnchor()),
            text.bottomAnchor()
                .constraintEqualToAnchor(&content.bottomAnchor()),
            action
                .trailingAnchor()
                .constraintEqualToAnchor(&content.trailingAnchor()),
            action
                .centerYAnchor()
                .constraintEqualToAnchor(&content.centerYAnchor()),
            text.trailingAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&action.leadingAnchor(), -10.0),
        ]);
        frame.setContentView(Some(&content));
        BannerView {
            frame,
            icon,
            title,
            lines,
            hint,
            action,
        }
    }

    /// Sets the strip; hides it if there are no lines.
    fn show(&self, banner: &Banner) {
        if banner.lines.is_empty() {
            self.frame.setHidden(true);
            return;
        }
        let tint = match banner.action {
            BannerAction::ShowInFinder => look::ink(Ink::Error),
            BannerAction::None | BannerAction::OpenFile => look::ink(Ink::Warning),
        };
        self.frame.setFillColor(&tint.colorWithAlphaComponent(0.10));
        self.frame
            .setBorderColor(&tint.colorWithAlphaComponent(0.35));
        self.icon.setContentTintColor(Some(&tint));
        match banner.title {
            Some(title) => {
                self.title.setStringValue(&NSString::from_str(title));
                self.title.setHidden(false);
            }
            None => self.title.setHidden(true),
        }
        self.lines
            .setStringValue(&NSString::from_str(&banner.lines.join("\n")));
        match banner.hint {
            Some(hint) => {
                self.hint.setStringValue(&NSString::from_str(hint));
                self.hint.setHidden(false);
            }
            None => self.hint.setHidden(true),
        }
        match banner.action {
            BannerAction::None => self.action.setHidden(true),
            BannerAction::OpenFile => {
                self.action.setTitle(ns_string!("Open settings.toml"));
                // SAFETY: the selector is one of the window's actions; the target was set at birth.
                unsafe { self.action.setAction(Some(sel!(openFile:))) };
                // On a lock, repairing the file is one click — Enter — away.
                self.action.setKeyEquivalent(ns_string!("\r"));
                self.action.setHidden(false);
            }
            BannerAction::ShowInFinder => {
                self.action.setTitle(ns_string!("Show in Finder"));
                // SAFETY: as above.
                unsafe { self.action.setAction(Some(sel!(showSettingsFile:))) };
                self.action.setKeyEquivalent(ns_string!(""));
                self.action.setHidden(false);
            }
        }
        self.frame.setHidden(false);
    }
}

/// A group of rows on a page: its title line, if it has one, and the rounded box that holds the
/// rows.
struct GroupView {
    view: Retained<NSStackView>,
    /// The text beside the title ([`pages::Group::aside`]).
    aside: Option<Retained<NSTextField>>,
    /// The title itself, for the group whose title follows the settings (the font's options
    /// are named for the font).
    title: Option<Retained<NSTextField>>,
    spec: &'static pages::Group,
    /// The group's rows, as a span of `Controls::rows`.
    rows: std::ops::Range<usize>,
}

/// What a row's control side is made of.
struct Parts {
    view: Retained<NSView>,
    /// The controls the lock and a dependent row's dimming reach.
    controls: Vec<Retained<NSControl>>,
    /// The labels beside the controls (a unit, a slider's reading) that dim with the row.
    dim: Vec<Retained<NSTextField>>,
}

impl Parts {
    /// A single control that is the whole of the row's control side.
    fn of(control: &NSControl) -> Parts {
        let view: &NSView = control;
        Parts {
            view: view.retain(),
            controls: vec![control.retain()],
            dim: Vec::new(),
        }
    }
}

impl Controls {
    /// The control side of `key`'s row. Each control sits in one row, so this is asked once per key.
    fn parts(&self, mtm: MainThreadMarker, key: Key) -> Parts {
        let number = |number: &Number| {
            let unit = pages::unit(key).map(|unit| {
                let label = text_label(&NSString::from_str(unit), mtm);
                label.setTextColor(Some(&look::ink(Ink::Dim)));
                label
                    .widthAnchor()
                    .constraintGreaterThanOrEqualToConstant(UNIT_WIDTH)
                    .setActive(true);
                label
            });
            let mut views: Vec<&NSView> = vec![&number.field, &number.stepper];
            if let Some(unit) = &unit {
                views.push(unit);
            }
            Parts {
                view: hstack(mtm, &views, 6.0),
                controls: vec![
                    number.field.as_super().retain(),
                    number.stepper.as_super().retain(),
                ],
                dim: unit.into_iter().collect(),
            }
        };
        let slide = |slide: &Slide| {
            slide
                .value
                .setAlignment(objc2_app_kit::NSTextAlignment::Right);
            width_constraint(&slide.value, SLIDE_READING_WIDTH);
            Parts {
                view: hstack(mtm, &[&slide.slider, &slide.value], 10.0),
                controls: vec![slide.slider.as_super().retain()],
                dim: vec![slide.value.clone()],
            }
        };
        let folder = |folder: &Folder, show: Option<&NSButton>| {
            let mut views: Vec<&NSView> = vec![&folder.path, &folder.change];
            if let Some(show) = show {
                views.push(show);
            }
            Parts {
                view: hstack(mtm, &views, 8.0),
                controls: vec![folder.change.as_super().retain()],
                dim: Vec::new(),
            }
        };
        match key {
            Key::ConfirmClose => Parts::of(&self.confirm_close),
            Key::RestoreWindows => Parts::of(&self.restore_windows),
            Key::KeepRunning => Parts::of(&self.keep_running),
            Key::Clipboard => Parts::of(&self.clipboard),
            Key::Scrollback => number(&self.scrollback),
            Key::ShellIntegration => Parts::of(&self.shell_integration),
            Key::CommandLevel => Parts::of(&self.command_level),
            Key::Theme => {
                let view = stack(
                    mtm,
                    &[
                        self.theme_mode.as_super().as_super(),
                        self.theme.as_super().as_super(),
                    ],
                    NSUserInterfaceLayoutOrientation::Vertical,
                    12.0,
                    NSLayoutAttribute::Leading,
                );
                Parts {
                    view: Retained::into_super(view),
                    controls: vec![
                        Retained::into_super(self.theme_mode.clone()),
                        Retained::into_super(self.theme.clone()),
                    ],
                    dim: Vec::new(),
                }
            }
            Key::LightTheme => Parts::of(&self.light_theme),
            Key::DarkTheme => Parts::of(&self.dark_theme),
            Key::Font => Parts::of(&self.font),
            Key::FontFeatures => Parts {
                view: self.font_options.view(),
                controls: Vec::new(),
                dim: Vec::new(),
            },
            Key::Size => number(&self.size),
            Key::LineHeight => number(&self.line_height),
            Key::LetterSpacing => number(&self.letter_spacing),
            Key::FontSmoothing => Parts::of(&self.font_smoothing),
            Key::Scrollbar => Parts::of(&self.scrollbar),
            Key::ContentEdge => Parts::of(&self.content_edge),
            Key::SplitStyle => Parts::of(&self.split_style),
            Key::TabDetails => Parts::of(&self.tab_details),
            Key::TabIcons => Parts::of(&self.tab_icons),
            Key::TabIconColors => Parts::of(&self.tab_icon_colors),
            Key::TabRunning => Parts::of(&self.tab_running),
            Key::TabPosition => Parts::of(&self.tab_position),
            Key::MinimumContrast => Parts::of(&self.minimum_contrast),
            Key::Shape => Parts::of(&self.shape),
            Key::Blink => Parts::of(&self.blink),
            Key::BlinkSpeed => slide(&self.blink_speed),
            Key::Radius => slide(&self.radius),
            Key::Glow => slide(&self.glow),
            Key::Unfocused => Parts::of(&self.unfocused),
            Key::CursorMotion => Parts::of(&self.cursor_motion),
            Key::SmoothScroll => Parts::of(&self.smooth_scroll),
            Key::ScrollBounce => Parts::of(&self.scroll_bounce),
            Key::ReduceMotion => Parts::of(&self.reduce_motion),
            Key::Keypress => Parts::of(&self.keypress),
            Key::Erase => Parts::of(&self.erase),
            Key::DockArrival => Parts::of(&self.dock_arrival),
            Key::PreviewMaxSize => Parts::of(&self.preview_max_size),
            Key::PreviewReadOnly => Parts::of(&self.preview_read_only),
            Key::PreviewDir => folder(&self.preview_dir, Some(&self.show_previews)),
            Key::PreviewKeep => Parts::of(&self.preview_keep),
            Key::PreviewLimit => Parts::of(&self.preview_limit),
            Key::DownloadDir => folder(&self.download_dir, None),
            Key::DownloadConflict => Parts::of(&self.download_conflict),
            Key::DownloadNotify => Parts::of(&self.download_notify),
            Key::RemoteStats => Parts::of(&self.remote_stats),
            Key::StatsInterval => number(&self.stats_interval),
            Key::RemoteIntegration => Parts::of(&self.remote_integration),
            Key::Ports => Parts::of(&self.ports),
            Key::PythonEnv => Parts::of(&self.python_env),
            Key::NodeVersion => Parts::of(&self.node_version),
            Key::DimSplits => Parts::of(&self.dim_splits),
            Key::FocusFrame => Parts::of(&self.focus_frame),
            Key::ResizeInSteps => Parts::of(&self.resize_in_steps),
        }
    }
}

/// A box for a group's rows: rounded, a hair of edge, the ground a control stands on. Returns it
/// and the stack inside it that the rows go into.
fn group_box(mtm: MainThreadMarker) -> (Retained<NSBox>, Retained<NSStackView>) {
    let boxed = NSBox::new(mtm);
    boxed.setBoxType(NSBoxType::Custom);
    boxed.setTitlePosition(NSTitlePosition::NoTitle);
    boxed.setCornerRadius(GROUP_RADIUS);
    boxed.setBorderWidth(0.5);
    boxed.setBorderColor(&look::ink(Ink::Line));
    boxed.setFillColor(&look::ink(Ink::Card));
    boxed.setContentViewMargins(NSSize::new(0.0, 0.0));
    let inner = stack(
        mtm,
        &[],
        NSUserInterfaceLayoutOrientation::Vertical,
        0.0,
        NSLayoutAttribute::Leading,
    );
    let content: &NSView = &inner;
    boxed.setContentView(Some(content));
    // The box sizes its content view by its autoresizing mask, which gives the box no height of
    // its own: left so, the box is laid out at none and the rows draw over one another and over
    // the next group. Tied to the box's four edges, the stack's rows are the box's height.
    inner.setTranslatesAutoresizingMaskIntoConstraints(false);
    activate(&[
        inner
            .topAnchor()
            .constraintEqualToAnchor(&boxed.topAnchor()),
        inner
            .bottomAnchor()
            .constraintEqualToAnchor(&boxed.bottomAnchor()),
        inner
            .leadingAnchor()
            .constraintEqualToAnchor(&boxed.leadingAnchor()),
        inner
            .trailingAnchor()
            .constraintEqualToAnchor(&boxed.trailingAnchor()),
    ]);
    (boxed, inner)
}

/// A group as the page shows it: the title line (title, then its aside) above the box.
fn group_view(
    mtm: MainThreadMarker,
    spec: &pages::Group,
    boxed: &NSBox,
) -> (
    Retained<NSStackView>,
    Option<Retained<NSTextField>>,
    Option<Retained<NSTextField>>,
) {
    let group = stack(
        mtm,
        &[],
        NSUserInterfaceLayoutOrientation::Vertical,
        TITLE_SPACING,
        NSLayoutAttribute::Leading,
    );
    let mut aside = None;
    let mut heading = None;
    if let Some(title) = spec.title {
        let title = text_label(&NSString::from_str(title), mtm);
        title.setFont(Some(
            &NSFont::boldSystemFontOfSize(NSFont::systemFontSize()),
        ));
        heading = Some(title.clone());
        let note = text_label(ns_string!(""), mtm);
        note.setTextColor(Some(&look::ink(Ink::Dim)));
        note.setFont(Some(&NSFont::systemFontOfSize(12.0)));
        let line = stack(
            mtm,
            &[&title, &note],
            NSUserInterfaceLayoutOrientation::Horizontal,
            8.0,
            NSLayoutAttribute::FirstBaseline,
        );
        line.setEdgeInsets(NSEdgeInsets {
            top: 0.0,
            left: 4.0,
            bottom: 0.0,
            right: 4.0,
        });
        group.addArrangedSubview(&line);
        aside = Some(note);
    }
    group.addArrangedSubview(boxed);
    boxed
        .widthAnchor()
        .constraintEqualToAnchor(&group.widthAnchor())
        .setActive(true);
    (group, aside, heading)
}

/// A row of a group's box: the name on the left with its description under it, the control on
/// the right (or, for `below`, under the text and as wide as the row), a hairline above it.
fn build_row(
    mtm: MainThreadMarker,
    key: Option<Key>,
    label: &str,
    parts: Parts,
    below: bool,
) -> Row {
    let view = NSView::new(mtm);
    let name = text_label(&NSString::from_str(label), mtm);
    name.setHidden(label.is_empty());
    let note = wrapping_text_label(ns_string!(""), mtm);
    note.setFont(Some(&NSFont::systemFontOfSize(
        NSFont::smallSystemFontSize(),
    )));
    let side = if below {
        0.0
    } else {
        parts.view.fittingSize().width
    };
    note.setPreferredMaxLayoutWidth(if below {
        ROW_WIDTH
    } else {
        (ROW_WIDTH - ROW_GAP - side).max(MIN_TEXT_WIDTH)
    });
    let text = stack(
        mtm,
        &[&name, &note],
        NSUserInterfaceLayoutOrientation::Vertical,
        2.0,
        NSLayoutAttribute::Leading,
    );
    // The rule between two rows, in the theme's separator: a pane's split line.
    let separator = surface(mtm, Ink::Line);
    add_pinned(&view, &separator);
    add_pinned(&view, &text);
    add_pinned(&view, &parts.view);
    let min_height = view
        .heightAnchor()
        .constraintGreaterThanOrEqualToConstant(ROW_MIN_HEIGHT);
    let mut constraints = vec![
        min_height,
        separator
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&view.leadingAnchor(), ROW_PAD_X),
        separator
            .trailingAnchor()
            .constraintEqualToAnchor_constant(&view.trailingAnchor(), -ROW_PAD_X),
        separator
            .topAnchor()
            .constraintEqualToAnchor(&view.topAnchor()),
        separator.heightAnchor().constraintEqualToConstant(1.0),
        text.leadingAnchor()
            .constraintEqualToAnchor_constant(&view.leadingAnchor(), ROW_PAD_X),
        text.topAnchor()
            .constraintGreaterThanOrEqualToAnchor_constant(&view.topAnchor(), ROW_PAD_Y),
    ];
    if below {
        constraints.extend([
            text.trailingAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&view.trailingAnchor(), -ROW_PAD_X),
            parts
                .view
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&view.leadingAnchor(), ROW_PAD_X),
            parts
                .view
                .trailingAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&view.trailingAnchor(), -ROW_PAD_X),
            parts.view.topAnchor().constraintEqualToAnchor_constant(
                &text.bottomAnchor(),
                if label.is_empty() { 4.0 } else { 10.0 },
            ),
            parts
                .view
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&view.bottomAnchor(), -ROW_PAD_Y),
        ]);
    } else {
        constraints.extend([
            text.bottomAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&view.bottomAnchor(), -ROW_PAD_Y),
            text.centerYAnchor()
                .constraintEqualToAnchor(&view.centerYAnchor()),
            parts
                .view
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&view.trailingAnchor(), -ROW_PAD_X),
            parts
                .view
                .centerYAnchor()
                .constraintEqualToAnchor(&view.centerYAnchor()),
            parts
                .view
                .topAnchor()
                .constraintGreaterThanOrEqualToAnchor_constant(&view.topAnchor(), 6.0),
            parts
                .view
                .bottomAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&view.bottomAnchor(), -6.0),
            text.trailingAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&parts.view.leadingAnchor(), -ROW_GAP),
        ]);
    }
    activate(&constraints);
    let row = Row {
        key,
        view,
        separator,
        label: name,
        controls: parts.controls,
        dim: parts.dim,
        note,
        hidden: Cell::new(false),
    };
    row.set_note(None, None, None, true);
    row
}

/// A stack of `views` with the window's usual manners: hidden members take no room.
fn stack(
    mtm: MainThreadMarker,
    views: &[&NSView],
    orientation: NSUserInterfaceLayoutOrientation,
    spacing: f64,
    alignment: NSLayoutAttribute,
) -> Retained<NSStackView> {
    let stack = NSStackView::stackViewWithViews(&NSArray::from_slice(views), mtm);
    stack.setOrientation(orientation);
    stack.setSpacing(spacing);
    stack.setAlignment(alignment);
    stack
}

fn hstack(mtm: MainThreadMarker, views: &[&NSView], spacing: f64) -> Retained<NSView> {
    let stack = NSStackView::stackViewWithViews(&NSArray::from_slice(views), mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    stack.setSpacing(spacing);
    Retained::into_super(stack)
}

fn new_popup(mtm: MainThreadMarker) -> Retained<NSPopUpButton> {
    let popup = NSPopUpButton::new(mtm);
    popup.setPullsDown(false);
    width_constraint(&popup, POPUP_WIDTH);
    popup
}

fn width_constraint(view: &NSView, width: f64) {
    view.widthAnchor()
        .constraintEqualToConstant(width)
        .setActive(true);
}

/// A label in the theme's text colour; a caller that says less sets the dim one after.
fn text_label(text: &NSString, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(text, mtm);
    label.setTextColor(Some(&look::ink(Ink::Text)));
    label
}

/// [`text_label`] that wraps.
fn wrapping_text_label(text: &NSString, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let label = NSTextField::wrappingLabelWithString(text, mtm);
    label.setTextColor(Some(&look::ink(Ink::Text)));
    label
}

/// A plain surface in `ink`, to stand behind the views of a column or a page.
fn surface(mtm: MainThreadMarker, ink: Ink) -> Retained<NSBox> {
    let surface = NSBox::new(mtm);
    surface.setBoxType(NSBoxType::Custom);
    surface.setTitlePosition(NSTitlePosition::NoTitle);
    surface.setBorderWidth(0.0);
    surface.setCornerRadius(0.0);
    surface.setFillColor(&look::ink(ink));
    surface
}

/// Ties `child` to `parent`'s four edges.
fn pin_to_edges(child: &NSView, parent: &NSView) {
    activate(&[
        child
            .topAnchor()
            .constraintEqualToAnchor(&parent.topAnchor()),
        child
            .bottomAnchor()
            .constraintEqualToAnchor(&parent.bottomAnchor()),
        child
            .leadingAnchor()
            .constraintEqualToAnchor(&parent.leadingAnchor()),
        child
            .trailingAnchor()
            .constraintEqualToAnchor(&parent.trailingAnchor()),
    ]);
}

fn add_pinned(parent: &NSView, child: &NSView) {
    child.setTranslatesAutoresizingMaskIntoConstraints(false);
    parent.addSubview(child);
}

fn activate(constraints: &[Retained<NSLayoutConstraint>]) {
    for constraint in constraints {
        constraint.setActive(true);
    }
}

/// What a row of the sidebar is: a page, or the gap that sets Servers apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SideItem {
    Page(Category),
    Gap,
}

impl SideItem {
    /// The sidebar's rows, top to bottom: the pages in order, a gap before Servers.
    const ALL: [SideItem; 10] = [
        SideItem::Page(Category::General),
        SideItem::Page(Category::Appearance),
        SideItem::Page(Category::Font),
        SideItem::Page(Category::Tabs),
        SideItem::Page(Category::Cursor),
        SideItem::Page(Category::Motion),
        SideItem::Page(Category::Shell),
        SideItem::Page(Category::Keyboard),
        SideItem::Gap,
        SideItem::Page(Category::Servers),
    ];

    /// The table row of `category`.
    fn row_of(category: Category) -> usize {
        Self::ALL
            .iter()
            .position(|item| *item == SideItem::Page(category))
            .unwrap_or_default()
    }

    fn at(row: NSInteger) -> Option<SideItem> {
        usize::try_from(row)
            .ok()
            .and_then(|row| Self::ALL.get(row).copied())
    }
}

pub(crate) struct SideRowIvars {
    selected: Cell<bool>,
    icon: OnceCell<Retained<NSImageView>>,
    title: OnceCell<Retained<NSTextField>>,
}

define_class!(
    /// A row of the sidebar. It draws what the table would: the pale face of the selected row and
    /// the page's tile — bateri's own tab tile, in the accent when the page is the open one.
    // SAFETY: NSView has no subclassing requirement; Drop is not implemented.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriSettingsSideRow"]
    #[ivars = SideRowIvars]
    struct SideRow;

    unsafe impl NSObjectProtocol for SideRow {}

    impl SideRow {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let selected = self.ivars().selected.get();
            if selected {
                let face = NSRect::new(
                    NSPoint::new(SIDE_PAD, 0.0),
                    NSSize::new(bounds.size.width - 2.0 * SIDE_PAD, bounds.size.height),
                );
                look::faded(Ink::Text, 0.08)
                    .setFill();
                crate::tab_bar::shaped(face, SIDE_ROW_RADIUS).fill();
            }
            let tile = NSRect::new(
                NSPoint::new(
                    SIDE_PAD + SIDE_TILE_LEFT,
                    (bounds.size.height - bt_shell_common::tabs::TILE_HEIGHT) / 2.0,
                ),
                NSSize::new(
                    bt_shell_common::tabs::TILE_WIDTH,
                    bt_shell_common::tabs::TILE_HEIGHT,
                ),
            );
            let fill = if selected {
                look::ink(Ink::Accent)
            } else {
                look::faded(Ink::Text, 0.14)
            };
            fill.setFill();
            crate::tab_bar::shaped(tile, SIDE_TILE_RADIUS).fill();
        }
    }
);

impl SideRow {
    fn new(mtm: MainThreadMarker, category: Category, selected: bool) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SideRowIvars {
            selected: Cell::new(selected),
            icon: OnceCell::new(),
            title: OnceCell::new(),
        });
        // SAFETY: `init` is NSView's designated initializer (zero frame); the size comes from the
        // table and the constraints below.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        let label = text_label(&NSString::from_str(category.title()), mtm);
        let icon = NSImageView::new(mtm);
        let config =
            NSImageSymbolConfiguration::configurationWithPointSize_weight(SIDE_SYMBOL_SIZE, 0.0);
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str(category.symbol()),
            None,
        )
        .and_then(|image| image.imageWithSymbolConfiguration(&config))
        {
            icon.setImage(Some(&image));
        }
        icon.setImageScaling(NSImageScaling::ScaleNone);
        add_pinned(&this, &icon);
        add_pinned(&this, &label);
        activate(&[
            icon.leadingAnchor()
                .constraintEqualToAnchor_constant(&this.leadingAnchor(), SIDE_PAD + SIDE_TILE_LEFT),
            icon.centerYAnchor()
                .constraintEqualToAnchor(&this.centerYAnchor()),
            icon.widthAnchor()
                .constraintEqualToConstant(bt_shell_common::tabs::TILE_WIDTH),
            icon.heightAnchor()
                .constraintEqualToConstant(bt_shell_common::tabs::TILE_HEIGHT),
            label.leadingAnchor().constraintEqualToAnchor_constant(
                &this.leadingAnchor(),
                SIDE_PAD + SIDE_TILE_LEFT + bt_shell_common::tabs::TILE_WIDTH + 9.0,
            ),
            label
                .centerYAnchor()
                .constraintEqualToAnchor(&this.centerYAnchor()),
        ]);
        let _ = this.ivars().icon.set(icon);
        let _ = this.ivars().title.set(label);
        this.paint();
        this
    }

    /// Marks the row as the open page (or not) and redraws it.
    fn set_selected(&self, selected: bool) {
        if self.ivars().selected.replace(selected) != selected {
            self.paint();
        }
    }

    /// The parts that are views rather than drawing: the tile's mark and the title.
    fn paint(&self) {
        let selected = self.ivars().selected.get();
        if let Some(icon) = self.ivars().icon.get() {
            let tint = if selected {
                look::ink(Ink::OnAccent)
            } else {
                look::ink(Ink::Dim)
            };
            icon.setContentTintColor(Some(&tint));
        }
        if let Some(title) = self.ivars().title.get() {
            let size = NSFont::systemFontSize();
            let font = if selected {
                NSFont::boldSystemFontOfSize(size)
            } else {
                NSFont::systemFontOfSize(size)
            };
            title.setFont(Some(&font));
        }
        self.setNeedsDisplay(true);
    }
}

fn select_choice<T: Choice>(popup: &NSPopUpButton, value: T) {
    if let Some(index) = choice_index(value) {
        popup.selectItemAtIndex(index as NSInteger);
    }
}

fn select_picture<T: Choice>(choice: &PictureChoice, value: T) {
    choice.select(choice_index(value));
}

fn select_segment<T: Choice>(control: &NSSegmentedControl, value: T) {
    if let Some(index) = choice_index(value) {
        control.setSelectedSegment(index as NSInteger);
    }
}

fn set_switch(switch: &ThemeSwitch, on: bool) {
    switch.set_on(on);
}

/// Sets the field and the stepper to the file's value.
///
/// - **The field being edited is not touched**: a refresh comes on every
///   save (a save from outside, another control's write, the event after the
///   watcher) and assigning a value would cancel the edit and erase what was
///   typed. The shown value is still updated; the field's action looks at it.
/// - **The stepper's range includes the file's value**: `size = 100` is valid
///   for the parser and if the stepper clamped it to 72 the "up" click would
///   make it smaller.
fn set_number(number: &Number, value: f64, text: &str) {
    *number.shown.borrow_mut() = (value, text.to_owned());
    if number.field.currentEditor().is_none() {
        number.field.setStringValue(&NSString::from_str(text));
    }
    let (min, max) = number.range;
    number.stepper.setMinValue(min.min(value));
    number.stepper.setMaxValue(max.max(value));
    number.stepper.setDoubleValue(value);
}

fn set_slide(slide: &Slide, position: f64, text: &str) {
    slide.slider.setDoubleValue(position);
    slide.value.setStringValue(&NSString::from_str(text));
}

fn fill_sizes(popup: &NSPopUpButton, items: &[SizeItem], selected: usize) {
    let entries = items.iter().map(|item| match item {
        SizeItem::Preset(bytes) | SizeItem::Current(bytes) => Entry::item(size_title(*bytes)),
        SizeItem::Separator => Entry::Separator,
    });
    fill_popup(popup, entries, selected);
}

fn fill_contrasts(popup: &NSPopUpButton, items: &[ContrastItem], selected: usize) {
    let entries = items.iter().map(|item| match item {
        ContrastItem::Preset(ratio) | ContrastItem::Current(ratio) => {
            Entry::item(contrast_title(*ratio))
        }
        ContrastItem::Separator => Entry::Separator,
    });
    fill_popup(popup, entries, selected);
}

/// The Font popup's first entry: the default font, named. The app's own default family where
/// its fonts are registered; the system's chain, which is what opens otherwise, in a copy
/// run without them.
fn default_font_title() -> String {
    bt_gpu::app_default_family().map_or_else(
        || "Default (SF Mono, or Menlo)".to_owned(),
        |family| format!("Default ({family})"),
    )
}

fn fill_fonts(popup: &NSPopUpButton, items: &[FontItem], selected: usize) {
    let entries = items.iter().map(|item| match item {
        FontItem::Default => Entry::item(default_font_title()),
        FontItem::Separator => Entry::Separator,
        FontItem::Header(title) => Entry::Header(title),
        // Each family is named in its own letters.
        FontItem::Family(name) => Entry::Item(
            name.clone(),
            Decor {
                family: Some(name.clone()),
                swatch: None,
            },
        ),
        FontItem::Missing(name) => {
            Entry::item(missing_font_title(name, bt_gpu::family_notice(name)))
        }
    });
    fill_popup(popup, entries, selected);
}

/// What a popup's menu item has besides its title.
#[derive(Default)]
struct Decor {
    /// The family the title is written in.
    family: Option<String>,
    /// Four colours drawn in front of the title.
    swatch: Option<[u32; 4]>,
}

/// An item of a popup's menu as [`fill_popup`] builds it.
enum Entry {
    Item(String, Decor),
    Separator,
    /// A line of text that cannot be chosen: the title of the section under it.
    Header(&'static str),
}

impl Entry {
    fn item(title: impl Into<String>) -> Entry {
        Entry::Item(title.into(), Decor::default())
    }
}

/// The title `name` in the family's own font, as the font list shows it; `None` when the system
/// has no such family to draw with (the title stays plain).
fn family_title(title: &str, family: &str) -> Option<Retained<NSAttributedString>> {
    let descriptor = NSFontDescriptor::new().fontDescriptorWithFamily(&NSString::from_str(family));
    let font = NSFont::fontWithDescriptor_size(&descriptor, NSFont::systemFontSize())?;
    // SAFETY: AppKit's font attribute key (an extern static) with the value type it documents.
    let attributes = unsafe {
        NSDictionary::<NSAttributedStringKey, AnyObject>::from_slices(
            &[NSFontAttributeName],
            &[font.as_ref()],
        )
    };
    // SAFETY: `attributes` is a dictionary of attribute keys and values AppKit reads.
    Some(unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(title),
            Some(&attributes),
        )
    })
}

/// The picture of four colours in a menu item: a theme's ground, red, green and blue as stripes.
fn swatch_image(colors: [u32; 4]) -> Retained<NSImage> {
    const WIDTH: f64 = 4.0 * 7.0;
    const HEIGHT: f64 = 12.0;
    let draw = RcBlock::new(move |_: NSRect| -> Bool {
        let face = NSRect::new(NSPoint::ZERO, NSSize::new(WIDTH, HEIGHT));
        for (index, rgb) in colors.iter().enumerate() {
            let stripe = NSRect::new(
                NSPoint::new(index as f64 * 7.0, 0.0),
                NSSize::new(7.0, HEIGHT),
            );
            crate::tab_bar::Tint::of(*rgb, 1.0).color().setFill();
            NSBezierPath::fillRect(stripe);
        }
        look::faded(Ink::Text, 0.25).setStroke();
        let outline = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            NSRect::new(
                NSPoint::new(face.origin.x + 0.5, face.origin.y + 0.5),
                NSSize::new(WIDTH - 1.0, HEIGHT - 1.0),
            ),
            2.0,
            2.0,
        );
        outline.stroke();
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(WIDTH, HEIGHT), true, &draw)
}

/// Rebuilds the popup from scratch. Adding directly to the menu, **not**
/// `addItemWithTitle:`: that method dedupes items with the same title and cannot add a separator.
fn fill_popup(popup: &NSPopUpButton, entries: impl Iterator<Item = Entry>, selected: usize) {
    let mtm = popup.mtm();
    popup.removeAllItems();
    let Some(menu) = popup.menu() else {
        return;
    };
    for entry in entries {
        let item = match entry {
            Entry::Item(title, decor) => {
                // SAFETY: an action-less item; the popup's own action carries the selection.
                let item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str(&title),
                        None,
                        ns_string!(""),
                    )
                };
                if let Some(styled) = decor
                    .family
                    .as_deref()
                    .and_then(|family| family_title(&title, family))
                {
                    item.setAttributedTitle(Some(&styled));
                }
                if let Some(colors) = decor.swatch {
                    item.setImage(Some(&swatch_image(colors)));
                }
                item
            }
            Entry::Separator => NSMenuItem::separatorItem(mtm),
            Entry::Header(title) => {
                NSMenuItem::sectionHeaderWithTitle(&NSString::from_str(title), mtm)
            }
        };
        menu.addItem(&item);
    }
    popup.selectItemAtIndex(selected as NSInteger);
}

#[cfg(test)]
mod tests {
    use bt_core::Diagnostic;

    use super::*;

    /// Every popup's titles cover every variant of `NAMES`, there is no empty
    /// or duplicate title, and index ↔ variant is consistent in both directions.
    fn check<T: Choice + std::fmt::Debug>() {
        let titles = choice_titles::<T>();
        assert_eq!(titles.len(), T::names().len());
        for (i, title) in titles.iter().enumerate() {
            assert!(!title.is_empty(), "empty title: {i}");
            assert_eq!(
                titles.iter().filter(|other| *other == title).count(),
                1,
                "duplicate title: {title}"
            );
            let value = choice_at::<T>(i as NSInteger).expect("the index is a variant");
            assert_eq!(choice_index(value), Some(i), "{value:?}");
        }
        assert_eq!(choice_at::<T>(-1), None);
        assert_eq!(choice_at::<T>(titles.len() as NSInteger), None);
    }

    /// Every row's key is the very key that appears in the parser's
    /// diagnostic: the diagnostics of a file that writes all keys with the
    /// wrong type land one to one on the rows, and neither an unmatched
    /// diagnostic nor an unmatched row remains.
    #[test]
    fn every_row_receives_its_own_diagnostic() {
        let text = "[terminal]\nscrollback = []\ncursor = []\ncursor_blink = []\n\
                    cursor_radius = []\ncursor_glow = []\ncursor_unfocused = []\n\
                    cursor_blink_interval = []\nconfirm_close = []\n\
                    restore_windows = []\nkeep_running = []\nscrollbar = []\n\
                    [appearance]\ntheme = []\nlight_theme = []\ndark_theme = []\n\
                    content_edge = []\nminimum_contrast = []\ndim_unfocused_splits = []\n\
                    focus_frame = []\nsplit_style = []\ntab_details = []\nresize_in_steps = []\ntab_icons = []\ntab_icon_colors = []\ntab_running = []\ntab_position = []\n\
                    [font]\nfamily = []\nsize = []\nline_height = []\nletter_spacing = []\nfeatures = []\nsmoothing = []\n\
                    [clipboard]\nosc52 = []\n\
                    [motion]\ncursor_motion = []\nreduce_motion = []\nsmooth_scroll = []\nscroll_bounce = []\n\
                    keypress = []\nerase = []\ndock_arrival = []\n\
                    [shell]\nintegration = []\nports = []\npython_env = []\nnode_version = []\n\
                    [commands]\nlevel = []\n\
                    [remote]\npreview_max_size = []\npreview_read_only = []\n\
                    preview_dir = []\npreview_keep = []\npreview_limit = []\n\
                    download_dir = []\ndownload_conflict = []\ndownload_notify = []\n\
                    stats = []\nstats_interval = []\nintegration = []\n";
        let parsed = Settings::parse_keeping(text, &Settings::default()).expect("it parses");
        let seen = status(&FileState::Usable(parsed.diagnostics), &[]);
        assert_eq!(seen.banner, Banner::default(), "no unmatched diagnostic");
        let mut keys: Vec<Key> = seen.rows.iter().map(|(key, _)| *key).collect();
        keys.sort_by_key(|key| key.tag());
        assert_eq!(keys, Key::ALL);
        // The write side names the same path too: the row's edit is the row's
        // key (so the diagnostic of a write refusal lands on that row too).
        for key in Key::ALL {
            let edit = match key {
                Key::ConfirmClose => SettingsEdit::ConfirmClose(ConfirmClose::Never),
                Key::RestoreWindows => SettingsEdit::RestoreWindows(RestoreWindows::Off),
                Key::KeepRunning => SettingsEdit::KeepRunning(KeepRunning::Quit),
                Key::Scrollbar => SettingsEdit::Scrollbar(Scrollbar::Never),
                Key::ContentEdge => SettingsEdit::ContentEdge(ContentEdge::Cut),
                Key::MinimumContrast => SettingsEdit::MinimumContrast(4.5),
                Key::Clipboard => SettingsEdit::Osc52(Osc52::Off),
                Key::Scrollback => SettingsEdit::Scrollback(1),
                Key::ShellIntegration => SettingsEdit::ShellIntegration(ShellIntegration::Off),
                Key::Theme => SettingsEdit::Theme(String::new()),
                Key::LightTheme => SettingsEdit::LightTheme(String::new()),
                Key::DarkTheme => SettingsEdit::DarkTheme(String::new()),
                Key::Font => SettingsEdit::FontFamily(String::new()),
                Key::Size => SettingsEdit::FontSize(13.0),
                Key::LineHeight => SettingsEdit::LineHeight(1.0),
                Key::LetterSpacing => SettingsEdit::LetterSpacing(1.0),
                Key::FontSmoothing => SettingsEdit::FontSmoothing(true),
                Key::FontFeatures => SettingsEdit::FontFeature {
                    family: String::new(),
                    tag: "ss01".to_owned(),
                    on: None,
                },
                Key::Shape => SettingsEdit::Cursor(CaretShape::Beam),
                Key::Blink => SettingsEdit::CursorBlink(CursorBlink::On),
                Key::BlinkSpeed => SettingsEdit::BlinkInterval(0.5),
                Key::Radius => SettingsEdit::CursorRadius(0.1),
                Key::Glow => SettingsEdit::CursorGlow(0.5),
                Key::Unfocused => SettingsEdit::CursorUnfocused(UnfocusedCaret::Solid),
                Key::CursorMotion => SettingsEdit::CursorMotion(CursorMotion::Snap),
                Key::SmoothScroll => SettingsEdit::SmoothScroll(SmoothScroll::On),
                Key::ScrollBounce => SettingsEdit::ScrollBounce(ScrollBounce::On),
                Key::ReduceMotion => SettingsEdit::ReduceMotion(ReduceMotion::On),
                Key::Keypress => SettingsEdit::Keypress(Keypress::Off),
                Key::Erase => SettingsEdit::Erase(Erase::Off),
                Key::DockArrival => SettingsEdit::DockArrival(DockArrival::Off),
                Key::PreviewMaxSize => SettingsEdit::PreviewMaxSize(1),
                Key::PreviewReadOnly => SettingsEdit::PreviewReadOnly(false),
                Key::PreviewDir => SettingsEdit::PreviewDir("~".to_owned()),
                Key::PreviewKeep => SettingsEdit::PreviewKeep(PreviewKeep::Day),
                Key::PreviewLimit => SettingsEdit::PreviewLimit(1),
                Key::DownloadDir => SettingsEdit::DownloadDir("~".to_owned()),
                Key::DownloadConflict => SettingsEdit::DownloadConflict(DownloadConflict::Ask),
                Key::DownloadNotify => SettingsEdit::DownloadNotify(false),
                Key::RemoteStats => SettingsEdit::RemoteStats(RemoteStatsMode::Off),
                Key::StatsInterval => SettingsEdit::StatsInterval(3),
                Key::RemoteIntegration => SettingsEdit::RemoteIntegration(false),
                Key::Ports => SettingsEdit::ShellPorts(false),
                Key::PythonEnv => SettingsEdit::ShellPythonEnv(false),
                Key::NodeVersion => SettingsEdit::ShellNodeVersion(false),
                Key::CommandLevel => SettingsEdit::CommandLevel(CommandLevel::Off),
                Key::DimSplits => SettingsEdit::DimUnfocusedSplits(true),
                Key::FocusFrame => SettingsEdit::FocusFrame(true),
                Key::ResizeInSteps => SettingsEdit::ResizeInSteps(true),
                Key::SplitStyle => SettingsEdit::SplitStyle(SplitStyle::Lines),
                Key::TabDetails => SettingsEdit::TabDetails(TabDetails::Always),
                Key::TabIcons => SettingsEdit::TabIcons(false),
                Key::TabIconColors => SettingsEdit::IconColors(IconColors::Brand),
                Key::TabRunning => SettingsEdit::TabRunning(TabRunning::Breathe),
                Key::TabPosition => SettingsEdit::TabPosition(TabPosition::Side),
            };
            assert_eq!(edit.path(), key.path(), "{key:?}");
        }
    }

    #[test]
    fn file_state_decides_lock_banner_and_rows() {
        // No file: enabled, no banner, rows with their descriptions.
        assert_eq!(
            status(&FileState::Missing, &[]),
            Status {
                locked: false,
                banner: Banner::default(),
                rows: Vec::new(),
            }
        );

        // Lock: the reason is the same as the subtitle's, below it what to do.
        let reason = "settings.toml: line 1: invalid TOML".to_owned();
        let seen = status(&FileState::Locked(reason.clone()), &[]);
        assert!(seen.locked);
        assert_eq!(seen.banner.lines, [reason]);
        assert_eq!(seen.banner.hint, Some(LOCK_HINT));
        assert!(seen.rows.is_empty());

        // A rejected value on its own row, with only its message; a diagnostic
        // that lands on no row (a retired key) in the banner, in the subtitle's form.
        let rejected = Diagnostic {
            key: Some("terminal.cursor"),
            line: Some(2),
            message: "`terminal.cursor` must be one of …".to_owned(),
        };
        let retired = Diagnostic {
            key: None,
            line: Some(4),
            message: "`shell.prompt` is no longer read".to_owned(),
        };
        let seen = status(&FileState::Usable(vec![rejected, retired]), &[]);
        assert!(!seen.locked);
        assert_eq!(
            seen.rows,
            [(Key::Shape, "`terminal.cursor` must be one of …".to_owned())]
        );
        assert_eq!(
            seen.banner.lines,
            ["settings.toml: line 4: `shell.prompt` is no longer read"]
        );
        assert_eq!(seen.banner.hint, None);

        // A write error at the head of the banner, unlocked.
        let write = ["settings.toml could not be written: denied".to_owned()];
        let seen = status(&FileState::Usable(Vec::new()), &write);
        assert!(!seen.locked);
        assert_eq!(seen.banner.lines, write);
    }

    #[test]
    fn popup_titles_cover_every_name() {
        check::<ConfirmClose>();
        check::<RestoreWindows>();
        check::<KeepRunning>();
        check::<Scrollbar>();
        check::<ContentEdge>();
        check::<SplitStyle>();
        check::<IconColors>();
        check::<TabRunning>();
        check::<TabPosition>();
        check::<ShellIntegration>();
        check::<CommandLevel>();
        check::<CaretShape>();
        check::<CursorBlink>();
        check::<UnfocusedCaret>();
        check::<CursorMotion>();
        check::<ReduceMotion>();
        check::<Keypress>();
        check::<Erase>();
        check::<DockArrival>();
        check::<PreviewKeep>();
        check::<DownloadConflict>();
        check::<RemoteStatsMode>();
    }

    /// "Keep programs running"'s description follows the value: under `quit` it says what
    /// quitting leaves behind and how to end it, under the others only what ends the programs
    /// anyway; no other row's description moves with it.
    #[test]
    fn keep_running_note_follows_the_value() {
        let with = |keep| Settings {
            keep_running: keep,
            ..Settings::default()
        };
        let resolved = Resolved {
            reduce: false,
            scrollbar: ScrollbarMode::Auto,
        };
        assert_eq!(
            row_description(Key::KeepRunning, &with(KeepRunning::Update), resolved),
            Some("Restarting the computer ends them.")
        );
        assert_eq!(
            row_description(Key::KeepRunning, &with(KeepRunning::Crash), resolved),
            Some("Restarting the computer ends them.")
        );
        let quit = row_description(Key::KeepRunning, &with(KeepRunning::Quit), resolved)
            .expect("the row has a note");
        assert!(
            quit.starts_with("Programs keep running after you quit"),
            "{quit}"
        );
        assert!(quit.contains("\u{2325}\u{2318}Q ends them"), "{quit}");
        assert!(
            quit.ends_with("Restarting the computer ends them."),
            "{quit}"
        );
        for key in Key::ALL.into_iter().filter(|key| *key != Key::KeepRunning) {
            assert_eq!(
                row_description(key, &with(KeepRunning::Quit), resolved),
                row_description(key, &with(KeepRunning::Update), resolved),
                "{key:?}"
            );
        }
    }

    /// "Scroll bar"'s description says what "Follow System Settings" gives right
    /// now — the system's resolved form, which a mouse plugged in can change —
    /// and nothing under the values that say it themselves; no other row's
    /// description moves with it.
    #[test]
    fn scrollbar_note_follows_what_the_system_gives() {
        let with = |scrollbar| Settings {
            scrollbar,
            ..Settings::default()
        };
        let resolved = |scrollbar| Resolved {
            reduce: false,
            scrollbar,
        };
        let system = with(Scrollbar::System);
        let hiding = row_description(Key::Scrollbar, &system, resolved(ScrollbarMode::Auto))
            .expect("the system's form has a note");
        assert!(hiding.contains("When scrolling"), "{hiding}");
        let always = row_description(Key::Scrollbar, &system, resolved(ScrollbarMode::Always))
            .expect("the system's form has a note");
        assert!(always.contains("Always"), "{always}");
        assert_ne!(hiding, always, "the note did not follow the system");
        // The note names the popup's own titles.
        assert!(hiding.contains(Scrollbar::Auto.title()), "{hiding}");
        assert!(always.contains(Scrollbar::Always.title()), "{always}");
        for (setting, mode) in [
            (Scrollbar::Auto, ScrollbarMode::Auto),
            (Scrollbar::Always, ScrollbarMode::Always),
            (Scrollbar::Never, ScrollbarMode::Never),
        ] {
            assert_eq!(
                row_description(Key::Scrollbar, &with(setting), resolved(mode)),
                None,
                "{setting:?}"
            );
        }
        for key in Key::ALL.into_iter().filter(|&key| key != Key::Scrollbar) {
            assert_eq!(
                row_description(key, &system, resolved(ScrollbarMode::Always)),
                row_description(key, &system, resolved(ScrollbarMode::Auto)),
                "{key:?}"
            );
        }
    }

    /// The size popups show the file's value even when it is not a preset,
    /// and only a preset writes; the defaults are presets, and every title is
    /// the spelling written with a space before the unit.
    #[test]
    fn the_contrast_popup_keeps_the_file_value_visible() {
        // A preset is selected in place; the default is one of them.
        let (items, index) = contrast_items(bt_core::MINIMUM_CONTRAST);
        assert_eq!(
            items[index],
            ContrastItem::Preset(bt_core::MINIMUM_CONTRAST)
        );
        assert_eq!(contrast_edit(&items, index as NSInteger), Some(3.0));
        // A value written by hand is shown after the presets and choosing it
        // writes nothing (it already is the file's).
        let (items, index) = contrast_items(3.5);
        assert_eq!(items[index], ContrastItem::Current(3.5));
        assert_eq!(items[index - 1], ContrastItem::Separator);
        assert_eq!(contrast_edit(&items, index as NSInteger), None);
        // Every preset is one the file accepts, and its title says its ratio.
        for &ratio in CONTRAST_PRESETS {
            assert!(bt_core::MINIMUM_CONTRAST_RANGE.contains(&ratio), "{ratio}");
        }
        assert_eq!(contrast_title(1.0), "Off");
        assert_eq!(contrast_title(4.5), "Strong (4.5:1)");
        assert_eq!(contrast_title(3.25), "3.25:1");
    }

    #[test]
    fn size_popups_keep_the_file_value_visible() {
        let defaults = bt_core::RemoteFiles::default();
        for (current, presets) in [
            (defaults.preview_max_size, PREVIEW_SIZE_PRESETS),
            (defaults.preview_limit, PREVIEW_LIMIT_PRESETS),
        ] {
            let (items, index) = size_items(current, presets);
            assert_eq!(items.len(), presets.len(), "the default is a preset");
            assert_eq!(size_edit(&items, index as NSInteger), Some(current));
        }
        let (items, index) = size_items(1_500_000, PREVIEW_SIZE_PRESETS);
        assert_eq!(index, items.len() - 1);
        assert_eq!(items[index], SizeItem::Current(1_500_000));
        assert_eq!(items[index - 1], SizeItem::Separator);
        assert_eq!(size_edit(&items, index as NSInteger), None);
        assert_eq!(size_edit(&items, (index - 1) as NSInteger), None);
        assert_eq!(size_edit(&items, -1), None);
        assert_eq!(size_edit(&items, 0), Some(PREVIEW_SIZE_PRESETS[0]));
        assert_eq!(size_title(100_000_000), "100 MB");
        assert_eq!(size_title(1_500_000), "1500 KB");
        assert_eq!(size_title(2_000_000_000), "2 GB");
        for &bytes in PREVIEW_SIZE_PRESETS.iter().chain(PREVIEW_LIMIT_PRESETS) {
            let written = bt_core::format_size(bytes);
            assert_eq!(size_title(bytes).replace(' ', ""), written);
            assert_eq!(bt_core::parse_size(&written), Some(bytes));
        }
    }

    /// A chosen folder is written the way the parser accepts it: under the
    /// home directory as `~/…`, elsewhere absolute.
    #[test]
    fn chosen_folders_are_written_relative_to_home() {
        let home = Path::new("/Users/me");
        assert_eq!(
            folder_text(Path::new("/Users/me/Downloads/remote"), Some(home)),
            "~/Downloads/remote"
        );
        assert_eq!(folder_text(home, Some(home)), "~");
        // A sibling whose name starts like the home is not under it.
        assert_eq!(
            folder_text(Path::new("/Users/meg/x"), Some(home)),
            "/Users/meg/x"
        );
        assert_eq!(
            folder_text(Path::new("/Volumes/Disk/x"), None),
            "/Volumes/Disk/x"
        );
    }

    #[test]
    fn usage_says_how_much_and_how_many() {
        assert_eq!(usage_label(None), "—");
        assert_eq!(usage_label(Some((0, 0))), "Empty");
        assert_eq!(usage_label(Some((512, 1))), "512 B · 1 file");
        assert_eq!(usage_label(Some((340_000_000, 12))), "340.0 MB · 12 files");
    }

    /// The two inputs that turn motion off: the overridden row
    /// is disabled and says why; under Reduce Motion the typing row stays
    /// enabled, because the choice between `off` and an effect is a
    /// difference there too.
    #[test]
    fn motion_rows_say_what_turns_them_off() {
        let plain = Settings::default();
        for key in Key::ALL {
            assert_eq!(motion_override(key, &plain, false), None, "{key:?}");
        }
        let snap = Settings {
            cursor_motion: CursorMotion::Snap,
            ..Settings::default()
        };
        for key in [
            Key::Keypress,
            Key::Erase,
            Key::SmoothScroll,
            Key::DockArrival,
        ] {
            let forced = motion_override(key, &snap, false).expect("snap overrides");
            assert!(!forced.enabled, "{key:?}");
            // `snap` is above Reduce Motion: with both inputs on, the reason
            // given is `snap`.
            assert_eq!(motion_override(key, &snap, true), Some(forced), "{key:?}");
        }
        for key in [Key::Erase, Key::SmoothScroll, Key::ScrollBounce] {
            let forced = motion_override(key, &plain, true).expect("Reduce Motion overrides");
            assert!(!forced.enabled, "{key:?}");
        }
        let fades = motion_override(Key::Keypress, &plain, true).expect("fades in");
        assert!(fades.enabled);
        assert!(fades.note.contains("fade"), "{}", fades.note);
        // The arrival is reduced to a short fade too, and the row stays
        // enabled for the same reason: `off` is still a difference.
        let arrives = motion_override(Key::DockArrival, &plain, true).expect("fades in");
        assert!(arrives.enabled);
        assert!(arrives.note.contains("Fades in"), "{}", arrives.note);
        let arrival_off = Settings {
            dock_arrival: DockArrival::Off,
            ..Settings::default()
        };
        assert_eq!(motion_override(Key::DockArrival, &arrival_off, true), None);
        // Disabled typing stays disabled: Reduce Motion adds no animation,
        // there is nothing to say.
        let off = Settings {
            keypress: Keypress::Off,
            ..Settings::default()
        };
        assert_eq!(motion_override(Key::Keypress, &off, true), None);
        // It does not touch the other rows.
        for key in [Key::CursorMotion, Key::ReduceMotion, Key::Shape] {
            assert_eq!(motion_override(key, &snap, true), None, "{key:?}");
        }
    }

    #[test]
    fn switches_cover_both_names() {
        for &(_, mode) in Osc52::NAMES {
            assert_eq!(osc52_on(mode), mode == Osc52::Copy);
        }
        for &(_, smooth) in SmoothScroll::NAMES {
            assert_eq!(smooth_on(smooth), smooth == SmoothScroll::On);
        }
        for &(_, bounce) in ScrollBounce::NAMES {
            assert_eq!(bounce_on(bounce), bounce == ScrollBounce::On);
        }
    }

    #[test]
    fn keys_round_trip_through_their_tags() {
        for (i, key) in Key::ALL.iter().enumerate() {
            assert_eq!(key.tag(), i as NSInteger);
            assert_eq!(Key::from_tag(key.tag()), Some(*key));
        }
        assert_eq!(Key::from_tag(-1), None);
        assert_eq!(Key::from_tag(Key::ALL.len() as NSInteger), None);
    }

    #[test]
    fn blink_slider_ends_are_the_range_ends() {
        let (min, max) = (*CURSOR_BLINK_RANGE.start(), *CURSOR_BLINK_RANGE.end());
        // Left is slow (a long half period), right is fast.
        assert_eq!(blink_from_position(0.0), max);
        assert_eq!(blink_from_position(1.0), min);
        assert_eq!(blink_from_position(-3.0), max);
        assert_eq!(blink_from_position(7.0), min);
        assert_eq!(blink_to_position(max), 0.0);
        assert_eq!(blink_to_position(min), 1.0);
        // Logarithmic: the midpoint is the geometric mean.
        let middle = blink_from_position(0.5);
        assert!((middle - (min * max).sqrt()).abs() < 1e-9, "{middle}");
        for seconds in [0.1, 0.5, 1.0, 2.5] {
            let back = blink_from_position(blink_to_position(seconds));
            assert!((back - seconds).abs() < 1e-9, "{seconds} → {back}");
        }
    }

    #[test]
    fn decimals_are_shown_as_written() {
        assert_eq!(decimal_label(0.5), "0.5");
        assert_eq!(decimal_label(13.0), "13");
        assert_eq!(decimal_label(1.25), "1.25");
        assert_eq!(decimal_label(1.2000000000000002), "1.2");
        // The readings beside the sliders: fixed places, and "Off" for no glow.
        assert_eq!(seconds_label(0.5), "0.50 s");
        assert_eq!(radius_label(0.1), "0.10");
        assert_eq!(glow_label(1.0), "1.0");
        assert_eq!(glow_label(0.0), "Off");
    }

    #[test]
    fn fields_refuse_what_the_file_would_refuse() {
        assert_eq!(parse_interval(" 3 "), Some(3));
        assert_eq!(parse_interval("2"), Some(2));
        assert_eq!(parse_interval("60"), Some(60));
        assert_eq!(parse_interval("1"), None);
        assert_eq!(parse_interval("61"), None);
        assert_eq!(parse_interval("2.5"), None);
        assert_eq!(parse_scrollback(" 2500 "), Some(2500));
        assert_eq!(parse_scrollback("0"), Some(0));
        assert_eq!(parse_scrollback("-1"), None);
        assert_eq!(parse_scrollback("abc"), None);
        assert_eq!(parse_scrollback(&(SCROLLBACK_MAX + 1).to_string()), None);
        assert_eq!(parse_decimal("14.5", MIN_SIZE..=MAX_SIZE), Some(14.5));
        assert_eq!(parse_decimal("NaN", MIN_SIZE..=MAX_SIZE), None);
        assert_eq!(parse_decimal("500", MIN_SIZE..=MAX_SIZE), None);
        assert_eq!(parse_decimal("0.4", LINE_HEIGHT_RANGE), None);
        assert_eq!(parse_decimal("0.4", LETTER_SPACING_RANGE), None);
        assert_eq!(parse_decimal("0.9", LINE_HEIGHT_RANGE), Some(0.9));
        assert_eq!(parse_decimal("1.3", LETTER_SPACING_RANGE), Some(1.3));
    }

    fn lists<'a>(
        embedded: &'a [Vec<&'a str>],
        user: &'a [String],
        user_themes: &'a [Theme],
        on_screen: &'a Theme,
    ) -> ThemeLists<'a> {
        ThemeLists {
            embedded,
            user,
            user_themes,
            on_screen,
            light: on_screen,
            dark: on_screen,
            dark_appearance: false,
        }
    }

    /// One theme's gallery has every theme, bateri's own first, then the dark, the light and the
    /// user's, each under its title; the pair's galleries have only the light or only the dark
    /// ones, by each theme's own colours.
    #[test]
    fn the_galleries_hold_their_themes_under_titles() {
        let embedded = Theme::embedded_groups();
        let user = vec!["paper".to_owned(), "ink".to_owned()];
        let user_themes = [Theme::LINEN, Theme::BATERI];
        let lists = lists(&embedded, &user, &user_themes, &Theme::BATERI);
        let titles = |sections: &[theme_gallery::Section]| -> Vec<String> {
            sections.iter().filter_map(|s| s.title.clone()).collect()
        };
        let all = theme_sections("bateri", Shade::All, &lists);
        assert_eq!(titles(&all), ["bateri", "Dark", "Light", "Your themes"]);
        let count: usize = all.iter().map(|s| s.cards.len()).sum();
        assert_eq!(count, embedded.iter().flatten().count() + user.len());
        assert_eq!(all[0].cards[0].name, "bateri");
        assert_eq!(all[0].cards[0].title, "bateri");
        for (shade, dark, chosen) in [
            (Shade::Light, false, "bateri-light"),
            (Shade::Dark, true, "bateri"),
        ] {
            let sections = theme_sections(chosen, shade, &lists);
            for card in sections.iter().flat_map(|s| &s.cards) {
                let theme = Theme::embedded(&card.name).unwrap_or(if card.name == "paper" {
                    Theme::LINEN
                } else {
                    Theme::BATERI
                });
                assert_eq!(
                    theme.is_dark(),
                    dark,
                    "{} in the {shade:?} gallery",
                    card.name
                );
            }
            assert!(
                sections
                    .iter()
                    .any(|s| s.title.as_deref() == Some("Your themes"))
            );
        }
    }

    /// A theme the file names that no list has (a deleted file) gets a card of its own, so the
    /// gallery still shows what the file says; Match System is no card.
    #[test]
    fn a_theme_the_lists_lack_keeps_a_card() {
        let embedded = Theme::embedded_groups();
        let lists = lists(&embedded, &[], &[], &Theme::BATERI);
        let sections = theme_sections("gone", Shade::All, &lists);
        let last = sections.last().expect("a section");
        assert_eq!(last.title.as_deref(), Some("Not found"));
        assert_eq!(last.cards[0].name, "gone");
        let system = theme_sections(SYSTEM_THEME, Shade::All, &lists);
        assert!(
            !system
                .iter()
                .any(|s| s.title.as_deref() == Some("Not found"))
        );
        // A dark theme the file names for the light appearance is shown, in its own colours.
        let other = theme_sections("bateri", Shade::Light, &lists);
        let last = other.last().expect("a section");
        assert_eq!(last.title.as_deref(), Some("Chosen"));
        assert_eq!(last.cards[0].palette, Palette::of(&Theme::BATERI));
    }

    /// Each option of a pictured choice has a picture of its own, and the rows whose control is
    /// pictures (or the font's switches) are the ones that stand under their text.
    #[test]
    fn every_pictured_option_has_its_own_picture() {
        fn check<T: Pictured>() -> usize {
            let pictures: Vec<Picture> = T::names().iter().map(|&(_, v)| v.picture()).collect();
            for (at, picture) in pictures.iter().enumerate() {
                assert!(!pictures[..at].contains(picture), "{picture:?} twice");
            }
            pictures.len()
        }
        assert_eq!(check::<ContentEdge>(), 3);
        assert_eq!(check::<TabPosition>(), 2);
        assert_eq!(check::<SplitStyle>(), 2);
        assert_eq!(check::<CaretShape>(), 3);
        assert_eq!(check::<UnfocusedCaret>(), 2);
        assert_eq!(check::<TabRunning>(), 3);
        let under: Vec<Key> = Key::ALL
            .into_iter()
            .filter(|key| pages::below(*key))
            .collect();
        assert_eq!(
            under,
            [
                Key::Theme,
                Key::LightTheme,
                Key::DarkTheme,
                Key::FontFeatures,
                Key::Shape,
                Key::Unfocused,
                Key::ContentEdge,
                Key::SplitStyle,
                Key::TabRunning,
                Key::TabPosition,
            ]
        );
    }

    #[test]
    fn font_list_matches_case_insensitively_and_keeps_unknowns() {
        let families = vec!["Menlo".to_owned(), "SF Mono".to_owned()];
        let (items, index) = font_items(None, &families, &[]);
        assert_eq!((items[index].clone(), index), (FontItem::Default, 0));
        assert_eq!(font_edit(&items, 0).as_deref(), Some(""));
        let (items, index) = font_items(Some("menlo"), &families, &[]);
        assert_eq!(items[index], FontItem::Family("Menlo".to_owned()));
        let (items, index) = font_items(Some("Comic Sans"), &families, &[]);
        assert_eq!(items[index], FontItem::Missing("Comic Sans".to_owned()));
        assert_eq!(font_edit(&items, index as NSInteger), None);
        assert_eq!(
            missing_font_title(
                "Helvetica",
                Some(FontNotice::NotMonospaced {
                    family: "Helvetica".to_owned()
                })
            ),
            "Helvetica — not monospaced"
        );
    }

    /// The Font list has the app's own families first, under their heading, and the machine's after
    /// them; a heading is not a choice, and the file's unknown family stays last.
    #[test]
    fn the_font_list_has_the_apps_families_and_the_machines() {
        let families: Vec<String> = ["Fira Code", "JetBrains Mono", "Menlo", "Monaco"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let bundled = vec!["jetbrains mono".to_owned(), "Fira Code".to_owned()];
        let (items, index) = font_items(Some("Menlo"), &families, &bundled);
        assert_eq!(
            items,
            [
                FontItem::Default,
                FontItem::Separator,
                FontItem::Header("Included with bateri"),
                FontItem::Family("Fira Code".to_owned()),
                FontItem::Family("JetBrains Mono".to_owned()),
                FontItem::Separator,
                FontItem::Header("On this computer"),
                FontItem::Family("Menlo".to_owned()),
                FontItem::Family("Monaco".to_owned()),
            ]
        );
        assert_eq!(items[index], FontItem::Family("Menlo".to_owned()));
        assert_eq!(font_edit(&items, 2), None);
        // The file's family that is nowhere in the list comes after both sections.
        let (items, index) = font_items(Some("Comic Sans"), &families, &bundled);
        assert_eq!(index, items.len() - 1);
        assert_eq!(items[index - 1], FontItem::Separator);
        // Nothing of the app's own (a run without its fonts): one section, no empty heading.
        let (items, _) = font_items(None, &families, &[]);
        assert!(!items.contains(&FontItem::Header("Included with bateri")));
        assert!(items.contains(&FontItem::Header("On this computer")));
    }

    /// Every settings key stands once in the page table, and the Keyboard page, a table of its
    /// own, holds none; the one row that is not a key ("In use") stands once too.
    #[test]
    fn every_key_has_exactly_one_place_on_the_pages() {
        let mut keys = Vec::new();
        let mut usage = 0;
        for category in Category::ALL {
            for group in pages::groups(category) {
                assert!(!group.items.is_empty(), "{category:?} has an empty group");
                for item in group.items {
                    match item {
                        pages::Item::Key(key) => keys.push(*key),
                        pages::Item::Usage => usage += 1,
                    }
                }
            }
        }
        assert!(pages::groups(Category::Keyboard).is_empty());
        assert_eq!(usage, 1);
        keys.sort_by_key(|key| key.tag());
        assert_eq!(keys, Key::ALL);
        // Each key sits on the page its topic belongs to.
        let page_of = |wanted: Key| {
            Category::ALL
                .into_iter()
                .find(|category| {
                    pages::groups(*category)
                        .iter()
                        .any(|group| group.items.contains(&pages::Item::Key(wanted)))
                })
                .expect("the key has a page")
        };
        assert_eq!(page_of(Key::KeepRunning), Category::General);
        assert_eq!(page_of(Key::MinimumContrast), Category::Appearance);
        assert_eq!(page_of(Key::FontSmoothing), Category::Font);
        assert_eq!(page_of(Key::FontFeatures), Category::Font);
        assert_eq!(page_of(Key::SplitStyle), Category::Tabs);
        assert_eq!(page_of(Key::Glow), Category::Cursor);
        assert_eq!(page_of(Key::SmoothScroll), Category::Motion);
        assert_eq!(page_of(Key::Ports), Category::Shell);
        assert_eq!(page_of(Key::CommandLevel), Category::Shell);
        assert_eq!(page_of(Key::StatsInterval), Category::Servers);
        assert_eq!(page_of(Key::DownloadNotify), Category::Servers);
    }

    /// Every row has a name that is a name — none ends in the old colon — except the options' row,
    /// whose switches carry their own; units stand beside the control.
    #[test]
    fn rows_are_named_without_colons_and_units_stand_beside_the_control() {
        for key in Key::ALL {
            let label = pages::label(key);
            assert_eq!(label.is_empty(), key == Key::FontFeatures, "{key:?}");
            assert!(!label.ends_with(':'), "{label}");
        }
        assert_eq!(pages::unit(Key::Scrollback), Some("lines"));
        assert_eq!(pages::unit(Key::Size), Some("pt"));
        assert_eq!(pages::unit(Key::LineHeight), Some("\u{d7}"));
        assert_eq!(pages::unit(Key::LetterSpacing), Some("\u{d7}"));
        assert_eq!(pages::unit(Key::StatsInterval), Some("seconds"));
        assert_eq!(
            Key::ALL
                .into_iter()
                .filter(|k| pages::unit(*k).is_some())
                .count(),
            5
        );
    }

    /// The sidebar lists the nine pages in their order with a gap before Servers, and every
    /// page that has rows has a stack of its own.
    #[test]
    fn the_sidebar_lists_every_page_with_a_gap_before_servers() {
        let pages: Vec<Category> = SideItem::ALL
            .iter()
            .filter_map(|item| match item {
                SideItem::Page(category) => Some(*category),
                SideItem::Gap => None,
            })
            .collect();
        assert_eq!(pages, Category::ALL);
        let gap = SideItem::ALL
            .iter()
            .position(|item| *item == SideItem::Gap)
            .expect("a gap");
        assert_eq!(SideItem::ALL[gap + 1], SideItem::Page(Category::Servers));
        for (row, item) in SideItem::ALL.iter().enumerate() {
            assert_eq!(SideItem::at(row as NSInteger), Some(*item));
            if let SideItem::Page(category) = item {
                assert_eq!(SideItem::row_of(*category), row);
            }
        }
        assert_eq!(SideItem::at(-1), None);
        assert_eq!(SideItem::at(SideItem::ALL.len() as NSInteger), None);
        let grids: Vec<Option<usize>> = Category::ALL.iter().map(|c| c.grid()).collect();
        assert_eq!(
            grids,
            [
                Some(0),
                Some(1),
                Some(2),
                Some(3),
                Some(4),
                Some(5),
                Some(6),
                None,
                Some(7)
            ]
        );
    }

    /// The rows that lean on another setting: dimmed with their reason, or gone.
    #[test]
    fn a_row_that_leans_on_another_setting_is_dimmed_and_says_why() {
        let with = |edit: &dyn Fn(&mut Settings)| {
            let mut settings = Settings::default();
            edit(&mut settings);
            settings
        };
        let free = pages::Dependency {
            hidden: false,
            enabled: true,
            reason: None,
        };
        let dimmed = |reason| pages::Dependency {
            hidden: false,
            enabled: false,
            reason,
        };
        // Under the defaults only two rows lean: the cursor does not blink, and the tabs are on top.
        let plain = with(&|_| {});
        for key in Key::ALL {
            let leans = pages::dependency(key, &plain);
            match key {
                Key::BlinkSpeed => assert_eq!(leans, dimmed(Some("Blink is Off."))),
                Key::TabDetails => assert_eq!(leans, dimmed(Some("Only in the side column."))),
                Key::LightTheme | Key::DarkTheme => {
                    assert_eq!(leans.hidden, !plain.follows_system(), "{key:?}");
                }
                _ => assert_eq!(leans, free, "{key:?}"),
            }
        }
        // Blink Off: the speed has nothing to set.
        let blink = |mode| with(&|s: &mut Settings| s.cursor_blink = mode);
        assert_eq!(
            pages::dependency(Key::BlinkSpeed, &blink(CursorBlink::Off)),
            dimmed(Some("Blink is Off."))
        );
        assert_eq!(
            pages::dependency(Key::BlinkSpeed, &blink(CursorBlink::On)),
            free
        );
        // A theme of its own: Light and Dark mean nothing and go; Match System brings them back.
        let named = with(&|s: &mut Settings| s.theme = "bateri".to_owned());
        for key in [Key::LightTheme, Key::DarkTheme] {
            assert!(pages::dependency(key, &named).hidden, "{key:?}");
        }
        let system = with(&|s: &mut Settings| s.theme = SYSTEM_THEME.to_owned());
        for key in [Key::LightTheme, Key::DarkTheme] {
            assert_eq!(pages::dependency(key, &system), free, "{key:?}");
        }
        // Server load Off: no interval to sample at.
        let load = |mode| with(&|s: &mut Settings| s.remote_stats.mode = mode);
        assert_eq!(
            pages::dependency(Key::StatsInterval, &load(RemoteStatsMode::Off)),
            dimmed(Some("Server load is Off."))
        );
        assert_eq!(
            pages::dependency(Key::StatsInterval, &load(RemoteStatsMode::Alerts)),
            free
        );
        // Tabs on top: the details have no column to stand under; on the side they do.
        let tabs = |position| with(&|s: &mut Settings| s.tab_position = position);
        assert_eq!(
            pages::dependency(Key::TabDetails, &tabs(TabPosition::Top)),
            dimmed(Some("Only in the side column."))
        );
        assert_eq!(
            pages::dependency(Key::TabDetails, &tabs(TabPosition::Side)),
            free
        );
        // No program icons: the colors have nothing to color.
        let icons = |on| with(&|s: &mut Settings| s.tab_icons = on);
        assert_eq!(
            pages::dependency(Key::TabIconColors, &icons(false)),
            dimmed(Some("Program icons are off."))
        );
        assert_eq!(pages::dependency(Key::TabIconColors, &icons(true)), free);
        // The dock's three rows need Shell integration to be Auto; the group says so, not the rows.
        let shell = |mode| with(&|s: &mut Settings| s.shell_integration = mode);
        for key in [Key::PythonEnv, Key::NodeVersion, Key::Ports] {
            for mode in [ShellIntegration::Blocks, ShellIntegration::Off] {
                assert_eq!(
                    pages::dependency(key, &shell(mode)),
                    dimmed(None),
                    "{key:?}"
                );
            }
            assert_eq!(
                pages::dependency(key, &shell(ShellIntegration::Auto)),
                free,
                "{key:?}"
            );
        }
        let dock = pages::groups(Category::Shell)
            .iter()
            .find(|group| group.title == Some("In the dock"))
            .expect("the dock's group");
        assert_eq!(
            (dock.aside)(&shell(ShellIntegration::Off)),
            Some("needs Shell integration: Auto")
        );
        assert_eq!((dock.aside)(&shell(ShellIntegration::Auto)), None);
    }

    /// Each row's description for each value is its own sentence — the window says what the
    /// selected value does, not the list of them all.
    #[test]
    fn every_value_has_its_own_description() {
        let resolved = |reduce| Resolved {
            reduce,
            scrollbar: ScrollbarMode::Auto,
        };
        let say = |edit: &dyn Fn(&mut Settings)| {
            let mut settings = Settings::default();
            edit(&mut settings);
            settings
        };
        let of = |key, settings: &Settings| row_description(key, settings, resolved(false));
        let restore = |v| say(&|s: &mut Settings| s.restore_windows = v);
        assert_eq!(
            of(Key::RestoreWindows, &restore(RestoreWindows::All)),
            Some(
                "Windows, tabs and splits come back with what each pane showed. Scrollback is saved as plain text on disk."
            )
        );
        assert_eq!(
            of(Key::RestoreWindows, &restore(RestoreWindows::Layout)),
            Some("Windows, tabs and splits come back. Nothing you saw is written to disk.")
        );
        assert_eq!(
            of(Key::RestoreWindows, &restore(RestoreWindows::Off)),
            Some("bateri opens with a new window and deletes what was saved.")
        );
        let confirm = |v| say(&|s: &mut Settings| s.confirm_close = v);
        assert_eq!(
            of(Key::ConfirmClose, &confirm(ConfirmClose::Running)),
            Some("Asks while a program other than the shell runs, and names it.")
        );
        assert_eq!(
            of(Key::ConfirmClose, &confirm(ConfirmClose::Always)),
            Some("Asks even at an empty prompt.")
        );
        assert_eq!(of(Key::ConfirmClose, &confirm(ConfirmClose::Never)), None);
        let edge = |v| say(&|s: &mut Settings| s.content_edge = v);
        assert_eq!(
            of(Key::ContentEdge, &edge(ContentEdge::Fade)),
            Some("The line scrolling up thins out, so no letter is cut in half.")
        );
        // Line draws its line only with panes as lines and the tabs at the top; elsewhere the row
        // says it is Cut. The default panes are cards.
        assert_eq!(
            of(Key::ContentEdge, &edge(ContentEdge::Line)),
            Some(
                "Same as Cut while panes are cards: the thin line is drawn only with panes as lines."
            )
        );
        let line_with = |split_style, tab_position| {
            say(&|s: &mut Settings| {
                s.content_edge = ContentEdge::Line;
                s.split_style = split_style;
                s.tab_position = tab_position;
            })
        };
        assert_eq!(
            of(
                Key::ContentEdge,
                &line_with(SplitStyle::Lines, TabPosition::Top)
            ),
            Some("Text is cut under a thin line along the tab bar.")
        );
        assert_eq!(
            of(
                Key::ContentEdge,
                &line_with(SplitStyle::Lines, TabPosition::Side)
            ),
            Some(
                "Same as Cut while tabs are at the side: there is no tab bar above the text for the line."
            )
        );
        assert_eq!(
            of(Key::ContentEdge, &edge(ContentEdge::Cut)),
            Some("Text is cut where the pane ends.")
        );
        let position = |v| say(&|s: &mut Settings| s.tab_position = v);
        assert_eq!(
            of(Key::TabPosition, &position(TabPosition::Side)),
            Some("A column down the left edge, tall as the window; the title row goes.")
        );
        assert_eq!(
            of(Key::TabPosition, &position(TabPosition::Top)),
            Some("A bar along the top of the window.")
        );
        let icons = |v| say(&|s: &mut Settings| s.tab_icons = v);
        assert_eq!(
            of(Key::TabIcons, &icons(true)),
            Some("The program a tab runs, on a tile before its title.")
        );
        assert_eq!(
            of(Key::TabIcons, &icons(false)),
            Some("Only a small mark while a tab works, waits or has finished.")
        );
        let colors = |v| say(&|s: &mut Settings| s.tab_icon_colors = v);
        assert_eq!(
            of(Key::TabIconColors, &colors(IconColors::Brand)),
            Some("Programs with a colored brand (Claude, Gemini) keep it.")
        );
        assert_eq!(
            of(Key::TabIconColors, &colors(IconColors::Theme)),
            Some("Icons take the theme\u{2019}s colors.")
        );
        let details = |v| say(&|s: &mut Settings| s.tab_details = v);
        assert_eq!(of(Key::TabDetails, &details(TabDetails::Off)), None);
        assert_eq!(
            of(Key::TabDetails, &details(TabDetails::Smart)),
            Some("Where the program stands and what it asks, in Activity only.")
        );
        assert_eq!(
            of(Key::TabDetails, &details(TabDetails::Always)),
            Some("Where the program stands and what it asks, under every tab.")
        );
        let split = |v| say(&|s: &mut Settings| s.split_style = v);
        assert_eq!(
            of(Key::SplitStyle, &split(SplitStyle::Lines)),
            Some("Panes touch, with one thin line between them.")
        );
        assert_eq!(
            of(Key::SplitStyle, &split(SplitStyle::Cards)),
            Some("Each pane stands on the ground with a gap round it.")
        );
        let blink = |v| say(&|s: &mut Settings| s.cursor_blink = v);
        assert_eq!(
            of(Key::Blink, &blink(CursorBlink::Auto)),
            Some("Blinks until the running program asks it to stop.")
        );
        assert_eq!(
            of(Key::Blink, &blink(CursorBlink::On)),
            Some("Blinks whatever the program asks.")
        );
        assert_eq!(of(Key::Blink, &blink(CursorBlink::Off)), None);
        let cursor = |v| say(&|s: &mut Settings| s.cursor_motion = v);
        assert_eq!(
            of(Key::CursorMotion, &cursor(CursorMotion::Spring)),
            Some("Travels to its new place with a little give.")
        );
        assert_eq!(
            of(Key::CursorMotion, &cursor(CursorMotion::Ease)),
            Some("Glides to its new place.")
        );
        assert_eq!(
            of(Key::CursorMotion, &cursor(CursorMotion::Snap)),
            Some("Jumps. Typing and deleting effects are off too.")
        );
        let reduce = |v| say(&|s: &mut Settings| s.reduce_motion = v);
        assert_eq!(
            row_description(
                Key::ReduceMotion,
                &reduce(ReduceMotion::System),
                resolved(true)
            ),
            Some("On right now, from System Settings \u{203a} Accessibility.")
        );
        assert_eq!(
            row_description(
                Key::ReduceMotion,
                &reduce(ReduceMotion::System),
                resolved(false)
            ),
            Some("Off right now, from System Settings \u{203a} Accessibility.")
        );
        assert_eq!(
            of(Key::ReduceMotion, &reduce(ReduceMotion::On)),
            Some("Animations become fades and instant jumps.")
        );
        assert_eq!(of(Key::ReduceMotion, &reduce(ReduceMotion::Off)), None);
        let shell = |v| say(&|s: &mut Settings| s.shell_integration = v);
        assert_eq!(
            of(Key::ShellIntegration, &shell(ShellIntegration::Auto)),
            Some("bateri draws the prompt and the line you type sits in the dock. New tabs only.")
        );
        assert_eq!(
            of(Key::ShellIntegration, &shell(ShellIntegration::Blocks)),
            Some(
                "Commands become blocks; the prompt and the line stay your shell\u{2019}s. New tabs only."
            )
        );
        assert_eq!(
            of(Key::ShellIntegration, &shell(ShellIntegration::Off)),
            Some("bateri sets nothing up in your shell. New tabs only.")
        );
        let level = |v| say(&|s: &mut Settings| s.commands_level = v);
        assert_eq!(
            of(Key::CommandLevel, &level(CommandLevel::Off)),
            Some("The bateri command does nothing.")
        );
        assert_eq!(
            of(Key::CommandLevel, &level(CommandLevel::Own)),
            Some("They see and change only this computer\u{2019}s panes.")
        );
        assert_eq!(
            of(Key::CommandLevel, &level(CommandLevel::Screen)),
            Some("They see and change every tab, group and window bateri has open.")
        );
        assert_eq!(
            of(Key::CommandLevel, &level(CommandLevel::Full)),
            Some(
                "They also start terminals on other machines and type into or read their panes. A server given this is trusted with your other machines too."
            )
        );
        let stats = |v| say(&|s: &mut Settings| s.remote_stats.mode = v);
        assert_eq!(
            of(Key::RemoteStats, &stats(RemoteStatsMode::Sparkline)),
            Some("CPU and memory as a line and numbers, in the ssh bar.")
        );
        assert_eq!(
            of(Key::RemoteStats, &stats(RemoteStatsMode::Numbers)),
            Some("CPU and memory as numbers, in the ssh bar.")
        );
        assert_eq!(
            of(Key::RemoteStats, &stats(RemoteStatsMode::Alerts)),
            Some("Nothing until a value passes its threshold.")
        );
        assert_eq!(of(Key::RemoteStats, &stats(RemoteStatsMode::Off)), None);
        // Fixed sentences.
        let plain = Settings::default();
        assert_eq!(of(Key::Clipboard, &plain), Some("Also over ssh (OSC 52)."));
        assert_eq!(
            of(Key::FontSmoothing, &plain),
            Some("Thickens the strokes the way Terminal does.")
        );
        assert_eq!(
            of(Key::Ports, &plain),
            Some("Click to list them, \u{2318}-click to open one.")
        );
        assert_eq!(
            of(Key::StatsInterval, &plain),
            Some("Pauses in hidden tabs and while idle.")
        );
        assert_eq!(
            of(Key::Theme, &plain),
            Some("The light theme while the system is light, the dark one while it is dark.")
        );
        // No text anywhere still speaks of "this Mac" or lists the values.
        for key in Key::ALL {
            if let Some(text) = of(key, &plain) {
                assert!(!text.contains("Mac"), "{text}");
            }
        }
    }

    /// The strip says what happened and offers what can be done about it right there: Enter opens
    /// a file that cannot be read, a refused write shows where the file is.
    #[test]
    fn the_strip_says_what_happened_and_offers_what_to_do() {
        let locked = status(
            &FileState::Locked("line 12: expected \"=\"".to_owned()),
            &[],
        );
        assert_eq!(locked.banner.title, Some(LOCK_TITLE));
        assert_eq!(locked.banner.action, BannerAction::OpenFile);
        let write = ["settings.toml could not be written: denied".to_owned()];
        let refused = status(&FileState::Usable(Vec::new()), &write);
        assert_eq!(refused.banner.title, Some(WRITE_TITLE));
        assert_eq!(refused.banner.action, BannerAction::ShowInFinder);
        // A write error under a lock: the lock is the news, the file the way out.
        let both = status(&FileState::Locked("bad".to_owned()), &write);
        assert_eq!(both.banner.action, BannerAction::OpenFile);
        // A diagnostic that lands on no row is a note: no title, no button.
        let retired = Diagnostic {
            key: None,
            line: Some(4),
            message: "`shell.prompt` is no longer read".to_owned(),
        };
        let note = status(&FileState::Usable(vec![retired]), &[]);
        assert_eq!(note.banner.title, None);
        assert_eq!(note.banner.action, BannerAction::None);
        assert_eq!(status(&FileState::Missing, &[]).banner, Banner::default());
    }
}
