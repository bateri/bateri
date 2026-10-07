//! bateri ▸ Settings… (Cmd-,): the settings window — a sidebar with five
//! categories on the left, a label–control grid on the right.
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

use block2::RcBlock;
use bt_core::{
    CURSOR_BLINK_RANGE, CURSOR_GLOW_RANGE, CURSOR_RADIUS_RANGE, CaretShape, ConfirmClose,
    ContentEdge, CursorBlink, CursorMotion, DownloadConflict, Erase, KeepRunning, Keypress,
    LETTER_SPACING_RANGE, LINE_HEIGHT_RANGE, Osc52, PreviewKeep, ReduceMotion, RemoteStatsMode,
    RestoreWindows, SCROLLBACK_MAX, STATS_INTERVAL_RANGE, SYSTEM_THEME, Scrollbar, Settings,
    SettingsEdit, ShellIntegration, SmoothScroll, UnfocusedCaret,
};
use bt_gpu::{FontNotice, ScrollbarMode};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSBox, NSBoxType, NSButton, NSColor, NSControl,
    NSControlStateValueOff, NSControlStateValueOn, NSControlTextEditingDelegate, NSEventType,
    NSFont, NSGridCell, NSGridCellPlacement, NSGridRow, NSGridRowAlignment, NSGridView, NSImage,
    NSImageView, NSLayoutAttribute, NSLayoutConstraint, NSLineBreakMode, NSMenuItem,
    NSModalResponse, NSModalResponseOK, NSOpenPanel, NSPopUpButton, NSScrollView, NSSlider,
    NSSplitViewController, NSSplitViewItem, NSStackView, NSStepper, NSSwitch, NSTableCellView,
    NSTableColumn, NSTableView, NSTableViewDataSource, NSTableViewDelegate, NSTableViewStyle,
    NSTextField, NSTitlePosition, NSUserInterfaceLayoutOrientation, NSView, NSViewController,
    NSWindow, NSWindowStyleMask, NSWindowTabbingMode, NSWindowTitleVisibility, NSWorkspace,
};
use objc2_foundation::{
    NSArray, NSIndexSet, NSInteger, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect,
    NSSize, NSString, NSURL, ns_string,
};

use crate::app;
use crate::child;
use crate::remote_files::Sweep;
use crate::settings::{self, FileState};
use crate::upload::format_bytes;
use crate::zoom::{MAX_SIZE, MIN_SIZE};

/// The window's opening content size, in points. The width is fixed, the
/// height is not: the panes sit in a vertical scroll view, so a pane taller
/// than the window (Remote Files since its integration row) or a screen
/// shorter than the window scrolls instead of clipping the rows under the
/// button. A design constant, not a measured number.
const WINDOW_SIZE: NSSize = NSSize::new(680.0, 780.0);
/// The shortest the window can be dragged: the header, a few rows and the
/// button still fit. A design constant.
const MIN_WINDOW_HEIGHT: f64 = 360.0;
/// The sidebar's width: close to System Settings', roomy for four short
/// titles. A design constant.
const SIDEBAR_WIDTH: f64 = 180.0;
/// The right pane's inner margin. The 20-point margin of macOS forms.
const MARGIN: f64 = 20.0;
/// The label column's width: the same in all four panes, otherwise the
/// controls would shift sideways when the category changes. The width the
/// longest label ("Confirm before closing:") fits.
const LABEL_WIDTH: f64 = 170.0;
/// The popups' common width: popups of different sizes in the same column
/// look scattered. The longest item ("Only when a program is running") fits.
const POPUP_WIDTH: f64 = 230.0;
/// The sliders' width; the value label sits next to it.
const SLIDER_WIDTH: f64 = 170.0;
/// The banner text's wrap width: the right pane's width minus two margins,
/// the box's two inner paddings, the symbol and its spacing.
const BANNER_TEXT_WIDTH: f64 =
    WINDOW_SIZE.width - SIDEBAR_WIDTH - 2.0 * MARGIN - 2.0 * 10.0 - 16.0 - 8.0;
/// The note text's wrap width: the popup's width — the note must not exceed
/// the right edge of the control above it (it did in the first screenshot).
const NOTE_WIDTH: f64 = POPUP_WIDTH;
/// A folder row's path label: as wide as the popups' column allows, the
/// middle truncated beyond it (the end of a path is what tells folders apart).
const PATH_WIDTH: f64 = 280.0;
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

/// The sidebar's rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    General,
    Appearance,
    Cursor,
    Motion,
    RemoteFiles,
}

impl Category {
    const ALL: [Category; 5] = [
        Category::General,
        Category::Appearance,
        Category::Cursor,
        Category::Motion,
        Category::RemoteFiles,
    ];

    fn title(self) -> &'static str {
        match self {
            Category::General => "General",
            Category::Appearance => "Appearance",
            Category::Cursor => "Cursor",
            Category::Motion => "Motion",
            Category::RemoteFiles => "Remote Files",
        }
    }

    /// SF Symbol name; if the symbol is not found the icon stays empty, the row stays.
    fn symbol(self) -> &'static str {
        match self {
            Category::General => "gearshape",
            Category::Appearance => "paintpalette",
            Category::Cursor => "character.cursor.ibeam",
            Category::Motion => "wind",
            Category::RemoteFiles => "network",
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
    Shape,
    Blink,
    BlinkSpeed,
    Radius,
    Glow,
    Unfocused,
    CursorMotion,
    SmoothScroll,
    ReduceMotion,
    Keypress,
    Erase,
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
}

impl Key {
    /// The order is the `tag` itself: `ALL[tag]`.
    const ALL: [Key; 37] = [
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
        Key::Shape,
        Key::Blink,
        Key::BlinkSpeed,
        Key::Radius,
        Key::Glow,
        Key::Unfocused,
        Key::CursorMotion,
        Key::SmoothScroll,
        Key::ReduceMotion,
        Key::Keypress,
        Key::Erase,
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
            Key::Font => "font.family",
            Key::Size => "font.size",
            Key::LineHeight => "font.line_height",
            Key::LetterSpacing => "font.letter_spacing",
            Key::Shape => "terminal.cursor",
            Key::Blink => "terminal.cursor_blink",
            Key::BlinkSpeed => "terminal.cursor_blink_interval",
            Key::Radius => "terminal.cursor_radius",
            Key::Glow => "terminal.cursor_glow",
            Key::Unfocused => "terminal.cursor_unfocused",
            Key::CursorMotion => "motion.cursor_motion",
            Key::SmoothScroll => "motion.smooth_scroll",
            Key::ReduceMotion => "motion.reduce_motion",
            Key::Keypress => "motion.keypress",
            Key::Erase => "motion.erase",
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
        }
    }
}

/// The sentence under the reason in a locked window's banner.
const LOCK_HINT: &str = "Fix the file and save it; this window follows.";

/// The banner above the right pane; invisible if it has no lines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Banner {
    /// The subtitle's texts, verbatim: a write error, the lock's reason, a
    /// diagnostic that lands on no row.
    lines: Vec<String>,
    /// Below it, in the secondary colour: what to do.
    hint: Option<&'static str>,
}

/// What the window sees from the file's state — pure, so the
/// three states are tested without building a window.
#[derive(Debug, PartialEq, Eq)]
struct Status {
    /// All controls disabled; "Open settings.toml" is the default button.
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
        hint: None,
    };
    let mut rows = Vec::new();
    let locked = match state {
        FileState::Missing => false,
        FileState::Locked(reason) => {
            banner.lines.push(reason.clone());
            banner.hint = Some(LOCK_HINT);
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

/// The note under "Keep programs running:" for the file's value: under
/// `quit` it says, for as long as the value stands, that quitting leaves the
/// programs behind and how to get back to them or end them — the reminder
/// that does not depend on a notification permission. Pure.
fn keep_running_note(keep: KeepRunning) -> &'static str {
    match keep {
        KeepRunning::Update | KeepRunning::Crash => "Restarting the Mac ends them.",
        KeepRunning::Quit => {
            "Programs keep running after you quit; open bateri to return to them, \u{2325}\u{2318}Q \
             ends them. Restarting the Mac ends them."
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

/// The note under "Scroll bar:": under "Follow System Settings" it says what
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

/// A row's description that follows the file's value; `None` → the row's
/// fixed one. The window keeps no state of its own: the text is chosen on
/// every refresh from the settings it shows (and, for "Scroll bar:", what
/// the system resolves them to).
fn row_description(key: Key, settings: &Settings, resolved: Resolved) -> Option<&'static str> {
    match key {
        Key::KeepRunning => Some(keep_running_note(settings.keep_running)),
        Key::Scrollbar => scrollbar_note(settings.scrollbar, resolved.scrollbar),
        _ => None,
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
        Key::Keypress | Key::Erase if snap => disabled("Off while cursor motion is Snap."),
        Key::SmoothScroll if snap => disabled("Line by line while cursor motion is Snap."),
        Key::Erase if reduce => disabled("Off while Reduce motion is on."),
        Key::SmoothScroll if reduce => disabled("Line by line while Reduce motion is on."),
        Key::Keypress if reduce && settings.keypress != Keypress::Off => Some(Override {
            enabled: true,
            note: "Letters only fade in while Reduce motion is on.",
        }),
        _ => None,
    }
}

/// The popup's item titles, in `NAMES` order.
fn choice_titles<T: Choice>() -> Vec<&'static str> {
    T::names().iter().map(|&(_, value)| value.title()).collect()
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

/// An item of the theme popup.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ThemeItem {
    /// "Match System" — writes `SYSTEM_THEME`.
    System,
    Separator,
    Named(String),
}

/// The theme popup's items and the selected one's index — the order of the
/// Theme ▸ menu (`menu::fill_themes`): Match System, embedded ones, the
/// user's. If the name in the file is not in the list (a deleted theme) it is
/// appended at the end: the popup does not hide what the user wrote (the
/// Font popup's rule).
fn theme_items(
    selected: &str,
    with_system: bool,
    embedded: &[&str],
    user: &[String],
) -> (Vec<ThemeItem>, usize) {
    let mut items = Vec::new();
    if with_system {
        items.push(ThemeItem::System);
        items.push(ThemeItem::Separator);
    }
    items.extend(
        embedded
            .iter()
            .map(|name| ThemeItem::Named((*name).to_owned())),
    );
    if !user.is_empty() {
        items.push(ThemeItem::Separator);
        items.extend(user.iter().cloned().map(ThemeItem::Named));
    }
    let position = |items: &[ThemeItem]| {
        items.iter().position(|item| match item {
            ThemeItem::System => selected == SYSTEM_THEME,
            ThemeItem::Separator => false,
            ThemeItem::Named(name) => name == selected,
        })
    };
    if let Some(index) = position(&items) {
        return (items, index);
    }
    items.push(ThemeItem::Separator);
    items.push(ThemeItem::Named(selected.to_owned()));
    let index = items.len() - 1;
    (items, index)
}

/// An item of the font popup.
#[derive(Clone, Debug, PartialEq, Eq)]
enum FontItem {
    /// The chain: `family = ""`.
    Default,
    Separator,
    Family(String),
    /// A family that is in the file but not in the list; choosing it writes nothing (it already is).
    Missing(String),
}

/// The font popup's items and the selected one's index. Matching is case
/// insensitive: the chain finds the name that way too (`bt-atlas`'s
/// `same_family`), so `family = "menlo"` is the listed `Menlo`.
fn font_items(current: Option<&str>, families: &[String]) -> (Vec<FontItem>, usize) {
    let mut items = vec![FontItem::Default];
    if !families.is_empty() {
        items.push(FontItem::Separator);
        items.extend(families.iter().cloned().map(FontItem::Family));
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
        None => name.to_owned(),
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

/// The window's controls — what `refresh` writes and the actions read.
struct Controls {
    confirm_close: Retained<NSPopUpButton>,
    restore_windows: Retained<NSPopUpButton>,
    keep_running: Retained<NSPopUpButton>,
    clipboard: Retained<NSSwitch>,
    scrollback: Number,
    shell_integration: Retained<NSPopUpButton>,
    theme: Retained<NSPopUpButton>,
    light_theme: Retained<NSPopUpButton>,
    dark_theme: Retained<NSPopUpButton>,
    font: Retained<NSPopUpButton>,
    size: Number,
    line_height: Number,
    letter_spacing: Number,
    scrollbar: Retained<NSPopUpButton>,
    content_edge: Retained<NSPopUpButton>,
    shape: Retained<NSPopUpButton>,
    blink: Retained<NSPopUpButton>,
    blink_speed: Slide,
    radius: Slide,
    glow: Slide,
    unfocused: Retained<NSPopUpButton>,
    cursor_motion: Retained<NSPopUpButton>,
    smooth_scroll: Retained<NSSwitch>,
    reduce_motion: Retained<NSPopUpButton>,
    keypress: Retained<NSPopUpButton>,
    erase: Retained<NSPopUpButton>,
    preview_max_size: Retained<NSPopUpButton>,
    preview_read_only: Retained<NSSwitch>,
    preview_dir: Folder,
    preview_keep: Retained<NSPopUpButton>,
    preview_limit: Retained<NSPopUpButton>,
    /// The preview folder's usage; not a setting, so no row of its own.
    usage: Retained<NSTextField>,
    download_dir: Folder,
    download_conflict: Retained<NSPopUpButton>,
    download_notify: Retained<NSSwitch>,
    remote_stats: Retained<NSPopUpButton>,
    stats_interval: Number,
    remote_integration: Retained<NSSwitch>,
    /// The panes' rows: the lock, the dependent row and the row's
    /// diagnostic come from here.
    rows: Vec<Row>,
}

/// A row of the grid: the label, its controls and the note row below it.
struct Row {
    key: Key,
    label: Retained<NSTextField>,
    controls: Vec<Retained<NSControl>>,
    /// Description or diagnostic; if there is neither the note row is hidden
    /// (it leaves no gap).
    note: Retained<NSTextField>,
    note_row: Retained<NSGridRow>,
    description: Option<&'static str>,
}

impl Row {
    /// Whether the row's controls are enabled, whether its label is dimmed
    /// (a dependent row and the lock go through the same gate).
    fn set_enabled(&self, enabled: bool) {
        for control in &self.controls {
            control.setEnabled(enabled);
        }
        let color = if enabled {
            NSColor::labelColor()
        } else {
            NSColor::disabledControlTextColor()
        };
        self.label.setTextColor(Some(&color));
    }

    /// Sets the note to the diagnostic, the reason the overriding input gives
    /// ([`motion_override`]) or the description (`description`: the one that
    /// follows the value, [`row_description`], else the row's own) — in that
    /// order; if there is none it hides the row. A disabled row's description
    /// dims together with its label, the reason does not dim: it is the only
    /// text that says why the row is disabled.
    fn set_note(
        &self,
        diagnostic: Option<&str>,
        reason: Option<&str>,
        description: Option<&str>,
        enabled: bool,
    ) {
        let (text, color) = match (diagnostic, reason, description.or(self.description)) {
            (Some(diagnostic), _, _) => (diagnostic, NSColor::systemOrangeColor()),
            (None, Some(reason), _) => (reason, NSColor::secondaryLabelColor()),
            (None, None, Some(description)) if enabled => {
                (description, NSColor::secondaryLabelColor())
            }
            (None, None, Some(description)) => (description, NSColor::tertiaryLabelColor()),
            (None, None, None) => {
                self.note_row.setHidden(true);
                return;
            }
        };
        self.note.setStringValue(&NSString::from_str(text));
        self.note.setTextColor(Some(&color));
        self.note_row.setHidden(false);
    }
}

pub(crate) struct Ivars {
    window: OnceCell<Retained<NSWindow>>,
    sidebar: OnceCell<Retained<NSTableView>>,
    header: OnceCell<Retained<NSTextField>>,
    banner: OnceCell<BannerView>,
    /// Whether the window has been shown once (the gate of centring).
    shown_once: Cell<bool>,
    pane_tops: OnceCell<PaneTops>,
    /// The scroll view the panes sit in; scrolled back to the top when the
    /// category changes.
    pane_scroll: OnceCell<Retained<NSScrollView>>,
    /// One constraint per pane tying the document's bottom to that pane's
    /// bottom; only the selected pane's is active, so each pane scrolls exactly
    /// as far as its own rows.
    pane_bottoms: OnceCell<Vec<Retained<NSLayoutConstraint>>>,
    /// "Open settings.toml": the default button (Enter) while locked.
    open: OnceCell<Retained<NSButton>>,
    /// One grid per category; only the selected one is visible.
    panes: OnceCell<Vec<Retained<NSGridView>>>,
    controls: OnceCell<Controls>,
    /// The popups' item lists — the meaning of the selected index. Rebuilt on
    /// every `refresh` (a new file in the theme directory).
    themes: RefCell<Vec<ThemeItem>>,
    light_themes: RefCell<Vec<ThemeItem>>,
    dark_themes: RefCell<Vec<ThemeItem>>,
    fonts: RefCell<Vec<FontItem>>,
    max_sizes: RefCell<Vec<SizeItem>>,
    limits: RefCell<Vec<SizeItem>>,
    /// `preview_dir` and `download_dir` as the file writes them — what Change…
    /// starts the panel in and Show in Finder opens. From the last refresh.
    folders: RefCell<(String, String)>,
    /// The last usage measurement asked for; an older answer arriving later is
    /// dropped ([`SettingsWindow::show_usage`]).
    usage_generation: Cell<u64>,
    /// The monospaced families on the machine — **once** when the window is
    /// born: the list opens every candidate with CoreText and is not worth rebuilding on every save.
    families: Vec<String>,
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
            Category::ALL.len() as NSInteger
        }
    }

    unsafe impl NSControlTextEditingDelegate for SettingsWindow {}

    unsafe impl NSTableViewDelegate for SettingsWindow {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn view_for_row(
            &self,
            _table: &NSTableView,
            _column: Option<&NSTableColumn>,
            row: NSInteger,
        ) -> Option<Retained<NSView>> {
            let category = usize::try_from(row)
                .ok()
                .and_then(|row| Category::ALL.get(row).copied());
            category.map(|category| Retained::into_super(sidebar_cell(self.mtm(), category)))
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_did_change(&self, _note: &NSNotification) {
            self.show_selected();
        }
    }

    impl SettingsWindow {
        #[unsafe(method(popupChanged:))]
        fn popup_changed(&self, sender: Option<&AnyObject>) {
            let Some(popup) = sender.and_then(|s| s.downcast_ref::<NSPopUpButton>()) else {
                return;
            };
            let index = popup.indexOfSelectedItem();
            let Some(key) = Key::from_tag(popup.tag()) else {
                return;
            };
            let edit = match key {
                Key::ConfirmClose => choice_at(index).map(SettingsEdit::ConfirmClose),
                Key::RestoreWindows => choice_at(index).map(SettingsEdit::RestoreWindows),
                Key::KeepRunning => choice_at(index).map(SettingsEdit::KeepRunning),
                Key::Scrollbar => choice_at(index).map(SettingsEdit::Scrollbar),
                Key::ContentEdge => choice_at(index).map(SettingsEdit::ContentEdge),
                Key::ShellIntegration => choice_at(index).map(SettingsEdit::ShellIntegration),
                Key::Shape => choice_at(index).map(SettingsEdit::Cursor),
                Key::Blink => choice_at(index).map(SettingsEdit::CursorBlink),
                Key::Unfocused => choice_at(index).map(SettingsEdit::CursorUnfocused),
                Key::CursorMotion => choice_at(index).map(SettingsEdit::CursorMotion),
                Key::ReduceMotion => choice_at(index).map(SettingsEdit::ReduceMotion),
                Key::Keypress => choice_at(index).map(SettingsEdit::Keypress),
                Key::Erase => choice_at(index).map(SettingsEdit::Erase),
                Key::Theme => theme_edit(&self.ivars().themes.borrow(), index)
                    .map(SettingsEdit::Theme),
                Key::LightTheme => theme_edit(&self.ivars().light_themes.borrow(), index)
                    .map(SettingsEdit::LightTheme),
                Key::DarkTheme => theme_edit(&self.ivars().dark_themes.borrow(), index)
                    .map(SettingsEdit::DarkTheme),
                Key::Font => font_edit(&self.ivars().fonts.borrow(), index)
                    .map(SettingsEdit::FontFamily),
                Key::PreviewMaxSize => size_edit(&self.ivars().max_sizes.borrow(), index)
                    .map(SettingsEdit::PreviewMaxSize),
                Key::PreviewLimit => size_edit(&self.ivars().limits.borrow(), index)
                    .map(SettingsEdit::PreviewLimit),
                Key::PreviewKeep => choice_at(index).map(SettingsEdit::PreviewKeep),
                Key::DownloadConflict => choice_at(index).map(SettingsEdit::DownloadConflict),
                Key::RemoteStats => choice_at(index).map(SettingsEdit::RemoteStats),
                _ => None,
            };
            self.save(edit);
        }

        #[unsafe(method(switchChanged:))]
        fn switch_changed(&self, sender: Option<&AnyObject>) {
            let Some(switch) = sender.and_then(|s| s.downcast_ref::<NSSwitch>()) else {
                return;
            };
            let on = switch.state() == NSControlStateValueOn;
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
                Some(Key::PreviewReadOnly) => Some(SettingsEdit::PreviewReadOnly(on)),
                Some(Key::DownloadNotify) => Some(SettingsEdit::DownloadNotify(on)),
                Some(Key::RemoteIntegration) => Some(SettingsEdit::RemoteIntegration(on)),
                _ => None,
            };
            self.save(edit);
        }

        /// While a slider is dragged only the value label changes; it is
        /// written to the file **on release**. `continuous` is on,
        /// otherwise the label would freeze during the drag: the decision to
        /// write or not is from the event's type — if the mouse is down and
        /// dragging it is an intermediate value. The event of a slider changed
        /// with the keyboard (arrow key) is a key event and it writes.
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
                    Key::Radius => Some((&controls.radius.value, decimal_label(value))),
                    Key::Glow => Some((&controls.glow.value, decimal_label(value))),
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
fn theme_edit(items: &[ThemeItem], index: NSInteger) -> Option<String> {
    match items.get(usize::try_from(index).ok()?)? {
        ThemeItem::System => Some(SYSTEM_THEME.to_owned()),
        ThemeItem::Named(name) => Some(name.clone()),
        ThemeItem::Separator => None,
    }
}

/// The action's font item → the family to write; there is nothing to write
/// when the file's value that is not in the list is selected again.
fn font_edit(items: &[FontItem], index: NSInteger) -> Option<String> {
    match items.get(usize::try_from(index).ok()?)? {
        FontItem::Default => Some(String::new()),
        FontItem::Family(name) => Some(name.clone()),
        FontItem::Separator | FontItem::Missing(_) => None,
    }
}

/// The blink-speed label: seconds (`0.5 s`).
fn seconds_label(seconds: f64) -> String {
    format!("{} s", decimal_label(seconds))
}

impl SettingsWindow {
    /// Builds the window and all the controls; does not make it visible.
    pub(crate) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            window: OnceCell::new(),
            sidebar: OnceCell::new(),
            header: OnceCell::new(),
            banner: OnceCell::new(),
            shown_once: Cell::new(false),
            pane_tops: OnceCell::new(),
            pane_scroll: OnceCell::new(),
            pane_bottoms: OnceCell::new(),
            open: OnceCell::new(),
            panes: OnceCell::new(),
            controls: OnceCell::new(),
            themes: RefCell::new(Vec::new()),
            light_themes: RefCell::new(Vec::new()),
            dark_themes: RefCell::new(Vec::new()),
            fonts: RefCell::new(Vec::new()),
            max_sizes: RefCell::new(Vec::new()),
            limits: RefCell::new(Vec::new()),
            folders: RefCell::new((String::new(), String::new())),
            usage_generation: Cell::new(0),
            families: bt_gpu::monospaced_families(),
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
    pub(crate) fn refresh(
        &self,
        settings: &Settings,
        resolved: Resolved,
        state: &FileState,
        write: &[String],
        embedded: &[&str],
        user: &[String],
    ) {
        let Some(c) = self.ivars().controls.get() else {
            return;
        };
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

        let (items, index) = theme_items(&settings.theme, true, embedded, user);
        fill_themes(&c.theme, &items, index);
        self.ivars().themes.replace(items);
        let follows = settings.follows_system();
        let (items, index) = theme_items(&settings.light_theme, false, embedded, user);
        fill_themes(&c.light_theme, &items, index);
        self.ivars().light_themes.replace(items);
        let (items, index) = theme_items(&settings.dark_theme, false, embedded, user);
        fill_themes(&c.dark_theme, &items, index);
        self.ivars().dark_themes.replace(items);

        let (items, index) = font_items(settings.font.family.as_deref(), &self.ivars().families);
        fill_fonts(&c.font, &items, index);
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
        select_choice(&c.content_edge, settings.content_edge);

        select_choice(&c.shape, settings.cursor);
        select_choice(&c.blink, settings.cursor_blink);
        set_slide(
            &c.blink_speed,
            blink_to_position(settings.blink_interval),
            &seconds_label(settings.blink_interval),
        );
        let blinks = settings.cursor_blink != CursorBlink::Off;
        set_slide(
            &c.radius,
            settings.caret.radius_ratio,
            &decimal_label(settings.caret.radius_ratio),
        );
        set_slide(
            &c.glow,
            settings.caret.glow,
            &decimal_label(settings.caret.glow),
        );
        select_choice(&c.unfocused, settings.caret.unfocused);

        select_choice(&c.cursor_motion, settings.cursor_motion);
        set_switch(&c.smooth_scroll, smooth_on(settings.smooth_scroll));
        select_choice(&c.reduce_motion, settings.reduce_motion);
        select_choice(&c.keypress, settings.keypress);
        select_choice(&c.erase, settings.erase);

        set_switch(&c.remote_integration, settings.remote_integration);
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
        let sampling = settings.remote_stats.mode != RemoteStatsMode::Off;
        self.ivars()
            .folders
            .replace((files.preview_dir.clone(), files.download_dir.clone()));

        let status = status(state, write);
        for row in &c.rows {
            let forced = motion_override(row.key, settings, resolved.reduce);
            let depends = match row.key {
                Key::LightTheme | Key::DarkTheme => follows,
                Key::BlinkSpeed => blinks,
                Key::StatsInterval => sampling,
                _ => forced.is_none_or(|forced| forced.enabled),
            };
            let enabled = !status.locked && depends;
            row.set_enabled(enabled);
            let diagnostic = status
                .rows
                .iter()
                .find(|(key, _)| *key == row.key)
                .map(|(_, message)| message.as_str());
            row.set_note(
                diagnostic,
                forced.map(|forced| forced.note),
                row_description(row.key, settings, resolved),
                enabled,
            );
        }
        // The value label is a label, not a control: dimming it is by hand.
        let value_color = if !status.locked && blinks {
            NSColor::secondaryLabelColor()
        } else {
            NSColor::disabledControlTextColor()
        };
        c.blink_speed.value.setTextColor(Some(&value_color));
        for slide in [&c.radius, &c.glow] {
            let color = if status.locked {
                NSColor::disabledControlTextColor()
            } else {
                NSColor::secondaryLabelColor()
            };
            slide.value.setTextColor(Some(&color));
        }
        if let Some(banner) = self.ivars().banner.get() {
            banner.show(&status.banner);
        }
        self.layout_panes(!status.banner.lines.is_empty());
        if let Some(open) = self.ivars().open.get() {
            // On a lock, repairing the file is one click — Enter — away.
            open.setKeyEquivalent(if status.locked {
                ns_string!("\r")
            } else {
                ns_string!("")
            });
        }
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
        let Some(index) = usize::try_from(sidebar.selectedRow())
            .ok()
            .filter(|&index| index < Category::ALL.len())
        else {
            return;
        };
        header.setStringValue(&NSString::from_str(Category::ALL[index].title()));
        for (i, pane) in panes.iter().enumerate() {
            pane.setHidden(i != index);
        }
        if let Some(bottoms) = self.ivars().pane_bottoms.get() {
            for (i, bottom) in bottoms.iter().enumerate() {
                if i != index {
                    bottom.setActive(false);
                }
            }
            if let Some(bottom) = bottoms.get(index) {
                bottom.setActive(true);
            }
        }
        if let Some(scroll) = self.ivars().pane_scroll.get() {
            let clip = scroll.contentView();
            clip.scrollToPoint(NSPoint::new(0.0, 0.0));
            scroll.reflectScrolledClipView(&clip);
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
        let path = NSTextField::labelWithString(ns_string!(""), self.mtm());
        path.setTextColor(Some(&NSColor::secondaryLabelColor()));
        path.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
        path.widthAnchor()
            .constraintLessThanOrEqualToConstant(PATH_WIDTH)
            .setActive(true);
        let change = self.button("Change…", Some(key), sel!(chooseFolder:));
        Folder { path, change }
    }

    fn switch(&self, key: Key) -> Retained<NSSwitch> {
        let switch = NSSwitch::new(self.mtm());
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
        slider.setMinValue(min);
        slider.setMaxValue(max);
        slider.setContinuous(true);
        width_constraint(&slider, SLIDER_WIDTH);
        self.wire(&slider, key, sel!(sliderChanged:));
        let value = NSTextField::labelWithString(ns_string!(""), mtm);
        value.setTextColor(Some(&NSColor::secondaryLabelColor()));
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
        // The title is in the pane's header; the window title is for the Window menu.
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setTitlebarAppearsTransparent(true);
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
        table.setStyle(NSTableViewStyle::SourceList);
        table.setHeaderView(None);
        table.setAllowsEmptySelection(false);
        let column = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), ns_string!("c"));
        table.addTableColumn(&column);
        // SAFETY: the source and delegate are weak references; this object
        // lives for the whole process (`AppDelegate`'s ivar).
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(self)));
            table.setDelegate(Some(ProtocolObject::from_ref(self)));
        }
        let scroll = NSScrollView::new(mtm);
        scroll.setDocumentView(Some(&table));
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(false);
        let _ = self.ivars().sidebar.set(table);
        Retained::into_super(scroll)
    }

    fn build_detail(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let detail = NSView::new(mtm);

        let header = NSTextField::labelWithString(ns_string!(""), mtm);
        header.setFont(Some(&NSFont::boldSystemFontOfSize(17.0)));
        add_pinned(&detail, &header);
        let safe = detail.safeAreaLayoutGuide();
        activate(&[
            header
                .topAnchor()
                .constraintEqualToAnchor_constant(&safe.topAnchor(), 4.0),
            header
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&detail.leadingAnchor(), MARGIN),
        ]);

        let banner = BannerView::new(mtm);
        add_pinned(&detail, &banner.frame);
        activate(&[
            banner
                .frame
                .topAnchor()
                .constraintEqualToAnchor_constant(&header.bottomAnchor(), 12.0),
            banner
                .frame
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&detail.leadingAnchor(), MARGIN),
            banner
                .frame
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&detail.trailingAnchor(), -MARGIN),
        ]);

        // The panes' top is tied to one of two places: to the header when there
        // is no banner, to the banner when there is ([`SettingsWindow::layout_panes`]).
        // A hidden banner takes no room, so the panes move back up under the header.
        // The grids live in a flipped document inside a vertical scroll view
        // that fills the space between the header (or banner) and the button.
        let (panes, controls) = self.build_panes();
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
                    .constraintEqualToAnchor_constant(&document.leadingAnchor(), MARGIN),
                pane.topAnchor()
                    .constraintEqualToAnchor(&document.topAnchor()),
            ]);
            pane_bottoms.push(
                document
                    .bottomAnchor()
                    .constraintEqualToAnchor_constant(&pane.bottomAnchor(), MARGIN),
            );
        }
        let under_header = vec![
            scroll
                .topAnchor()
                .constraintEqualToAnchor_constant(&header.bottomAnchor(), 18.0),
        ];
        let under_banner = vec![
            scroll
                .topAnchor()
                .constraintEqualToAnchor_constant(&banner.frame.bottomAnchor(), 16.0),
        ];
        activate(&under_header);
        banner.frame.setHidden(true);

        // SAFETY: the target is a weak reference and lives for the whole
        // process; the selector is this class's `openFile:`.
        let open = unsafe {
            NSButton::buttonWithTitle_target_action(
                ns_string!("Open settings.toml"),
                Some(self.target()),
                Some(sel!(openFile:)),
                mtm,
            )
        };
        add_pinned(&detail, &open);
        activate(&[
            open.trailingAnchor()
                .constraintEqualToAnchor_constant(&detail.trailingAnchor(), -MARGIN),
            open.bottomAnchor()
                .constraintEqualToAnchor_constant(&detail.bottomAnchor(), -MARGIN),
            scroll
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&open.topAnchor(), -12.0),
        ]);

        let _ = self.ivars().header.set(header);
        let _ = self.ivars().banner.set(banner);
        let _ = self.ivars().pane_tops.set(PaneTops {
            under_header,
            under_banner,
        });
        let _ = self.ivars().open.set(open);
        let _ = self.ivars().pane_scroll.set(scroll);
        let _ = self.ivars().pane_bottoms.set(pane_bottoms);
        let _ = self.ivars().panes.set(panes);
        let _ = self.ivars().controls.set(controls);
        detail
    }

    fn build_panes(&self) -> (Vec<Retained<NSGridView>>, Controls) {
        let mtm = self.mtm();

        // General
        let confirm_close = self.popup::<ConfirmClose>(Key::ConfirmClose);
        let restore_windows = self.popup::<RestoreWindows>(Key::RestoreWindows);
        let keep_running = self.popup::<KeepRunning>(Key::KeepRunning);
        let clipboard = self.switch(Key::Clipboard);
        let scrollback = self.number(Key::Scrollback, 0.0, SCROLLBACK_MAX as f64, 1000.0, 80.0);
        let shell_integration = self.popup::<ShellIntegration>(Key::ShellIntegration);
        let mut general = Form::new(mtm);
        general.row(
            Key::ConfirmClose,
            "Confirm before closing:",
            &confirm_close,
            &[&confirm_close],
            None,
        );
        general.row(
            Key::RestoreWindows,
            "Reopen after quitting:",
            &restore_windows,
            &[&restore_windows],
            Some("Panes whose programs were not kept start a new shell. Saved scrollback is plain text on disk."),
        );
        // The description follows the value ([`row_description`]).
        general.row(
            Key::KeepRunning,
            "Keep programs running:",
            &keep_running,
            &[&keep_running],
            None,
        );
        general.row(
            Key::Clipboard,
            "Clipboard access:",
            &clipboard,
            &[&clipboard],
            Some("Lets programs copy to the clipboard, even over ssh (OSC 52)."),
        );
        general.row(
            Key::Scrollback,
            "Scrollback lines:",
            &number_view(mtm, &scrollback),
            &number_controls(&scrollback),
            None,
        );
        general.row(
            Key::ShellIntegration,
            "Shell integration:",
            &shell_integration,
            &[&shell_integration],
            Some("Takes effect in new tabs and windows."),
        );

        // Appearance
        let theme = self.string_popup(Key::Theme);
        let light_theme = self.string_popup(Key::LightTheme);
        let dark_theme = self.string_popup(Key::DarkTheme);
        let font = self.string_popup(Key::Font);
        let size = self.number(Key::Size, MIN_SIZE, MAX_SIZE, 1.0, 56.0);
        let line_height = self.number(
            Key::LineHeight,
            *LINE_HEIGHT_RANGE.start(),
            *LINE_HEIGHT_RANGE.end(),
            0.1,
            56.0,
        );
        let letter_spacing = self.number(
            Key::LetterSpacing,
            *LETTER_SPACING_RANGE.start(),
            *LETTER_SPACING_RANGE.end(),
            0.1,
            56.0,
        );
        let scrollbar = self.popup::<Scrollbar>(Key::Scrollbar);
        let content_edge = self.popup::<ContentEdge>(Key::ContentEdge);
        let mut appearance = Form::new(mtm);
        appearance.row(Key::Theme, "Theme:", &theme, &[&theme], None);
        appearance.row(
            Key::LightTheme,
            "Light theme:",
            &light_theme,
            &[&light_theme],
            None,
        );
        appearance.row(
            Key::DarkTheme,
            "Dark theme:",
            &dark_theme,
            &[&dark_theme],
            Some("Used when Theme is Match System."),
        );
        appearance.row(Key::Font, "Font:", &font, &[&font], None);
        appearance.row(
            Key::Size,
            "Size:",
            &number_view(mtm, &size),
            &number_controls(&size),
            None,
        );
        appearance.row(
            Key::LineHeight,
            "Line height:",
            &number_view(mtm, &line_height),
            &number_controls(&line_height),
            None,
        );
        appearance.row(
            Key::LetterSpacing,
            "Letter spacing:",
            &number_view(mtm, &letter_spacing),
            &number_controls(&letter_spacing),
            None,
        );
        // Under "Follow System Settings" the note says what that gives right
        // now ([`row_description`]).
        appearance.row(
            Key::Scrollbar,
            "Scroll bar:",
            &scrollbar,
            &[&scrollbar],
            None,
        );
        appearance.row(
            Key::ContentEdge,
            "Content edge:",
            &content_edge,
            &[&content_edge],
            Some("Where text meets the tab bar."),
        );

        // Cursor
        let shape = self.popup::<CaretShape>(Key::Shape);
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
        let unfocused = self.popup::<UnfocusedCaret>(Key::Unfocused);
        let mut cursor = Form::new(mtm);
        cursor.row(
            Key::Shape,
            "Shape:",
            &shape,
            &[&shape],
            Some("Programs like vim can change it while they run."),
        );
        cursor.row(
            Key::Blink,
            "Blink:",
            &blink,
            &[&blink],
            Some("Follow program blinks only when the running program asks."),
        );
        cursor.row(
            Key::BlinkSpeed,
            "Blink speed:",
            &slide_view(mtm, &blink_speed),
            &[&blink_speed.slider],
            None,
        );
        cursor.row(
            Key::Radius,
            "Corner radius:",
            &slide_view(mtm, &radius),
            &[&radius.slider],
            None,
        );
        cursor.row(
            Key::Glow,
            "Glow:",
            &slide_view(mtm, &glow),
            &[&glow.slider],
            None,
        );
        cursor.row(
            Key::Unfocused,
            "When unfocused:",
            &unfocused,
            &[&unfocused],
            Some("How the cursor looks in a window that is not active."),
        );

        // Motion
        let cursor_motion = self.popup::<CursorMotion>(Key::CursorMotion);
        let smooth_scroll = self.switch(Key::SmoothScroll);
        let reduce_motion = self.popup::<ReduceMotion>(Key::ReduceMotion);
        let keypress = self.popup::<Keypress>(Key::Keypress);
        let erase = self.popup::<Erase>(Key::Erase);
        let mut motion = Form::new(mtm);
        motion.row(
            Key::CursorMotion,
            "Cursor motion:",
            &cursor_motion,
            &[&cursor_motion],
            Some("How the cursor travels to its new place."),
        );
        motion.row(
            Key::Keypress,
            "Keypress:",
            &keypress,
            &[&keypress],
            Some("How a letter you type at the prompt appears."),
        );
        motion.row(
            Key::Erase,
            "Erase:",
            &erase,
            &[&erase],
            Some("How a letter you delete at the prompt goes."),
        );
        motion.row(
            Key::SmoothScroll,
            "Smooth scrolling:",
            &smooth_scroll,
            &[&smooth_scroll],
            None,
        );
        motion.row(
            Key::ReduceMotion,
            "Reduce motion:",
            &reduce_motion,
            &[&reduce_motion],
            Some("On turns animations into fades and instant jumps."),
        );

        // Remote Files: the shell integration on servers, preview, cleanup,
        // downloads.
        let remote_integration = self.switch(Key::RemoteIntegration);
        let preview_max_size = self.string_popup(Key::PreviewMaxSize);
        let preview_read_only = self.switch(Key::PreviewReadOnly);
        let preview_dir = self.folder(Key::PreviewDir);
        let show_previews = self.button("Show in Finder", None, sel!(showPreviewFolder:));
        let preview_keep = self.popup::<PreviewKeep>(Key::PreviewKeep);
        let preview_limit = self.string_popup(Key::PreviewLimit);
        let usage = NSTextField::labelWithString(&NSString::from_str(&usage_label(None)), mtm);
        usage.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
            NSFont::systemFontSize(),
            0.0,
        )));
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
            56.0,
        );
        let mut remote = Form::new(mtm);
        remote.row(
            Key::RemoteIntegration,
            "Set up shell integration on servers:",
            &remote_integration,
            &[&remote_integration],
            Some("Off on hosts marked Production unless the Shell menu turns it on."),
        );
        remote.row(
            Key::PreviewMaxSize,
            "Preview without asking:",
            &preview_max_size,
            &[&preview_max_size],
            Some("Cmd-click a remote file name. Larger files ask first."),
        );
        remote.row(
            Key::PreviewReadOnly,
            "Open read-only:",
            &preview_read_only,
            &[&preview_read_only],
            Some("A hint: a preview you change is kept, never cleaned up."),
        );
        remote.row(
            Key::PreviewDir,
            "Preview folder:",
            &preview_dir.path,
            &[&preview_dir.change],
            None,
        );
        remote.actions(&hstack(
            mtm,
            &[
                preview_dir.change.as_super().as_super(),
                show_previews.as_super().as_super(),
            ],
            8.0,
        ));
        remote.row(
            Key::PreviewKeep,
            "Keep previews:",
            &preview_keep,
            &[&preview_keep],
            Some("Checked when bateri starts and once a day."),
        );
        remote.row(
            Key::PreviewLimit,
            "Size limit:",
            &preview_limit,
            &[&preview_limit],
            Some("Applied when bateri starts, oldest first."),
        );
        remote.plain_row(
            "In use:",
            &hstack(
                mtm,
                &[usage.as_super().as_super(), clear.as_super().as_super()],
                10.0,
            ),
        );
        remote.row(
            Key::DownloadDir,
            "Download folder:",
            &download_dir.path,
            &[&download_dir.change],
            None,
        );
        remote.actions(&download_dir.change);
        remote.row(
            Key::DownloadConflict,
            "If the name exists:",
            &download_conflict,
            &[&download_conflict],
            None,
        );
        remote.row(
            Key::DownloadNotify,
            "Notify when done:",
            &download_notify,
            &[&download_notify],
            Some("Only while bateri is in the background."),
        );
        // The load indicator.
        remote.row(
            Key::RemoteStats,
            "Server load:",
            &remote_stats,
            &[&remote_stats],
            Some("The server's CPU and memory, in the ssh status bar."),
        );
        remote.row(
            Key::StatsInterval,
            "Sample every:",
            &number_view(mtm, &stats_interval),
            &number_controls(&stats_interval),
            Some("Seconds. Pauses in hidden tabs and when idle."),
        );

        let rows = [
            general.rows,
            appearance.rows,
            cursor.rows,
            motion.rows,
            remote.rows,
        ]
        .into_iter()
        .flatten()
        .collect();
        let panes = vec![
            general.grid,
            appearance.grid,
            cursor.grid,
            motion.grid,
            remote.grid,
        ];
        let controls = Controls {
            confirm_close,
            restore_windows,
            keep_running,
            clipboard,
            scrollback,
            shell_integration,
            theme,
            light_theme,
            dark_theme,
            font,
            size,
            line_height,
            letter_spacing,
            scrollbar,
            content_edge,
            shape,
            blink,
            blink_speed,
            radius,
            glow,
            unfocused,
            cursor_motion,
            smooth_scroll,
            reduce_motion,
            keypress,
            erase,
            preview_max_size,
            preview_read_only,
            preview_dir,
            preview_keep,
            preview_limit,
            usage,
            download_dir,
            download_conflict,
            download_notify,
            remote_stats,
            stats_interval,
            remote_integration,
            rows,
        };
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

/// The banner's view: a rounded box with a light orange fill, a warning
/// symbol on the left, the text on the right and what to do below it in the
/// secondary colour. The colours are the system's semantic colours — each
/// correct in the light and dark appearance.
struct BannerView {
    frame: Retained<NSBox>,
    lines: Retained<NSTextField>,
    hint: Retained<NSTextField>,
}

impl BannerView {
    fn new(mtm: MainThreadMarker) -> Self {
        let frame = NSBox::new(mtm);
        frame.setBoxType(NSBoxType::Custom);
        frame.setTitlePosition(NSTitlePosition::NoTitle);
        frame.setCornerRadius(8.0);
        frame.setBorderWidth(1.0);
        let orange = NSColor::systemOrangeColor();
        frame.setFillColor(&orange.colorWithAlphaComponent(0.10));
        frame.setBorderColor(&orange.colorWithAlphaComponent(0.35));
        frame.setContentViewMargins(NSSize::new(10.0, 8.0));

        let icon = NSImageView::new(mtm);
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            ns_string!("exclamationmark.triangle.fill"),
            None,
        ) {
            icon.setImage(Some(&image));
        }
        icon.setContentTintColor(Some(&orange));
        let lines = NSTextField::wrappingLabelWithString(ns_string!(""), mtm);
        lines.setPreferredMaxLayoutWidth(BANNER_TEXT_WIDTH);
        let hint = NSTextField::wrappingLabelWithString(ns_string!(""), mtm);
        hint.setFont(Some(&NSFont::systemFontOfSize(
            NSFont::smallSystemFontSize(),
        )));
        hint.setTextColor(Some(&NSColor::secondaryLabelColor()));
        hint.setPreferredMaxLayoutWidth(BANNER_TEXT_WIDTH);

        let text = NSStackView::stackViewWithViews(
            &NSArray::from_slice(&[lines.as_super().as_super(), hint.as_super().as_super()]),
            mtm,
        );
        text.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        text.setAlignment(NSLayoutAttribute::Leading);
        text.setSpacing(2.0);
        let content = NSView::new(mtm);
        add_pinned(&content, &icon);
        add_pinned(&content, &text);
        activate(&[
            icon.leadingAnchor()
                .constraintEqualToAnchor(&content.leadingAnchor()),
            icon.firstBaselineAnchor()
                .constraintEqualToAnchor(&lines.firstBaselineAnchor()),
            icon.widthAnchor().constraintEqualToConstant(16.0),
            text.leadingAnchor()
                .constraintEqualToAnchor_constant(&icon.trailingAnchor(), 8.0),
            text.trailingAnchor()
                .constraintLessThanOrEqualToAnchor(&content.trailingAnchor()),
            text.topAnchor()
                .constraintEqualToAnchor(&content.topAnchor()),
            text.bottomAnchor()
                .constraintEqualToAnchor(&content.bottomAnchor()),
        ]);
        frame.setContentView(Some(&content));
        BannerView { frame, lines, hint }
    }

    /// Sets the banner; hides it if there are no lines.
    fn show(&self, banner: &Banner) {
        if banner.lines.is_empty() {
            self.frame.setHidden(true);
            return;
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
        self.frame.setHidden(false);
    }
}

/// A pane's grid: the left column a right-aligned label, the right column
/// the control; the note below the control on its own row, small and in the secondary colour.
struct Form {
    mtm: MainThreadMarker,
    grid: Retained<NSGridView>,
    rows: Vec<Row>,
}

impl Form {
    fn new(mtm: MainThreadMarker) -> Self {
        let grid = NSGridView::new(mtm);
        grid.setRowSpacing(6.0);
        grid.setColumnSpacing(10.0);
        grid.setRowAlignment(NSGridRowAlignment::FirstBaseline);
        Form {
            mtm,
            grid,
            rows: Vec::new(),
        }
    }

    /// Adds the row and the note row below it. The note row exists even in a
    /// row with no description — hidden; the diagnostic of a value that was
    /// not accepted appears there.
    fn row(
        &mut self,
        key: Key,
        label: &str,
        control: &NSView,
        controls: &[&NSControl],
        description: Option<&'static str>,
    ) {
        let mtm = self.mtm;
        let text = NSTextField::labelWithString(&NSString::from_str(label), mtm);
        let first = self.grid.numberOfRows() == 0;
        let row = self
            .grid
            .addRowWithViews(&NSArray::from_slice(&[text.as_super().as_super(), control]));
        if !first {
            // A gap between row groups wider than the note's padding.
            row.setTopPadding(10.0);
        }
        if self.grid.numberOfRows() == 1 {
            let labels = self.grid.columnAtIndex(0);
            labels.setXPlacement(NSGridCellPlacement::Trailing);
            labels.setWidth(LABEL_WIDTH);
        }
        let empty = NSGridCell::emptyContentView(mtm);
        let note = NSTextField::wrappingLabelWithString(ns_string!(""), mtm);
        note.setFont(Some(&NSFont::systemFontOfSize(
            NSFont::smallSystemFontSize(),
        )));
        note.setPreferredMaxLayoutWidth(NOTE_WIDTH);
        let note_row = self
            .grid
            .addRowWithViews(&NSArray::from_slice(&[&*empty, note.as_super().as_super()]));
        note_row.setTopPadding(-2.0);
        let row = Row {
            key,
            label: text,
            controls: controls.iter().map(|control| control.retain()).collect(),
            note,
            note_row,
            description,
        };
        row.set_note(None, None, None, true);
        self.rows.push(row);
    }

    /// A row below the last one with only a right-hand view: a folder row's
    /// buttons under its path. Its label column is empty.
    fn actions(&mut self, view: &NSView) {
        let empty = NSGridCell::emptyContentView(self.mtm);
        let row = self
            .grid
            .addRowWithViews(&NSArray::from_slice(&[&*empty, view]));
        row.setTopPadding(2.0);
    }

    /// A row that is not a settings key (the preview folder's usage): label
    /// and view, no note and no [`Row`] — the lock and the diagnostics do not
    /// reach it, because it writes nothing.
    fn plain_row(&mut self, label: &str, view: &NSView) {
        let text = NSTextField::labelWithString(&NSString::from_str(label), self.mtm);
        let row = self
            .grid
            .addRowWithViews(&NSArray::from_slice(&[text.as_super().as_super(), view]));
        row.setTopPadding(10.0);
    }
}

/// Field + stepper side by side.
fn number_view(mtm: MainThreadMarker, number: &Number) -> Retained<NSView> {
    hstack(
        mtm,
        &[
            number.field.as_super().as_super(),
            number.stepper.as_super().as_super(),
        ],
        4.0,
    )
}

/// The field and the stepper are both the row's control (the lock disables
/// both at once).
fn number_controls(number: &Number) -> [&NSControl; 2] {
    [&number.field, &number.stepper]
}

/// Slider + value label side by side.
fn slide_view(mtm: MainThreadMarker, slide: &Slide) -> Retained<NSView> {
    hstack(
        mtm,
        &[
            slide.slider.as_super().as_super(),
            slide.value.as_super().as_super(),
        ],
        8.0,
    )
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

fn add_pinned(parent: &NSView, child: &NSView) {
    child.setTranslatesAutoresizingMaskIntoConstraints(false);
    parent.addSubview(child);
}

fn activate(constraints: &[Retained<NSLayoutConstraint>]) {
    for constraint in constraints {
        constraint.setActive(true);
    }
}

/// A row of the sidebar: SF Symbol + title.
fn sidebar_cell(mtm: MainThreadMarker, category: Category) -> Retained<NSTableCellView> {
    let cell = NSTableCellView::new(mtm);
    let label = NSTextField::labelWithString(&NSString::from_str(category.title()), mtm);
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(category.symbol()),
        None,
    );
    let icon = NSImageView::new(mtm);
    if let Some(image) = image {
        icon.setImage(Some(&image));
    }
    add_pinned(&cell, &icon);
    add_pinned(&cell, &label);
    activate(&[
        icon.leadingAnchor()
            .constraintEqualToAnchor_constant(&cell.leadingAnchor(), 4.0),
        icon.centerYAnchor()
            .constraintEqualToAnchor(&cell.centerYAnchor()),
        icon.widthAnchor().constraintEqualToConstant(18.0),
        label
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&icon.trailingAnchor(), 6.0),
        label
            .centerYAnchor()
            .constraintEqualToAnchor(&cell.centerYAnchor()),
    ]);
    // SAFETY: both fields are weak; the views live with the cell as the
    // cell's subviews.
    unsafe {
        cell.setImageView(Some(&icon));
        cell.setTextField(Some(&label));
    }
    cell
}

fn select_choice<T: Choice>(popup: &NSPopUpButton, value: T) {
    if let Some(index) = choice_index(value) {
        popup.selectItemAtIndex(index as NSInteger);
    }
}

fn set_switch(switch: &NSSwitch, on: bool) {
    switch.setState(if on {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
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

fn fill_themes(popup: &NSPopUpButton, items: &[ThemeItem], selected: usize) {
    let titles = items.iter().map(|item| match item {
        ThemeItem::System => Some("Match System".to_owned()),
        ThemeItem::Separator => None,
        ThemeItem::Named(name) => Some(name.clone()),
    });
    fill_popup(popup, titles, selected);
}

fn fill_sizes(popup: &NSPopUpButton, items: &[SizeItem], selected: usize) {
    let titles = items.iter().map(|item| match item {
        SizeItem::Preset(bytes) | SizeItem::Current(bytes) => Some(size_title(*bytes)),
        SizeItem::Separator => None,
    });
    fill_popup(popup, titles, selected);
}

fn fill_fonts(popup: &NSPopUpButton, items: &[FontItem], selected: usize) {
    let titles = items.iter().map(|item| match item {
        FontItem::Default => Some("Default (SF Mono, or Menlo)".to_owned()),
        FontItem::Separator => None,
        FontItem::Family(name) => Some(name.clone()),
        FontItem::Missing(name) => Some(missing_font_title(name, bt_gpu::family_notice(name))),
    });
    fill_popup(popup, titles, selected);
}

/// Rebuilds the popup from scratch. Adding directly to the menu, **not**
/// `addItemWithTitle:`: that method dedupes items with the same title and cannot add a separator.
fn fill_popup(
    popup: &NSPopUpButton,
    titles: impl Iterator<Item = Option<String>>,
    selected: usize,
) {
    let mtm = popup.mtm();
    popup.removeAllItems();
    let Some(menu) = popup.menu() else {
        return;
    };
    for title in titles {
        let item = match title {
            Some(title) => {
                // SAFETY: an action-less item; the popup's own action carries the selection.
                unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str(&title),
                        None,
                        ns_string!(""),
                    )
                }
            }
            None => NSMenuItem::separatorItem(mtm),
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
                    content_edge = []\n\
                    [font]\nfamily = []\nsize = []\nline_height = []\nletter_spacing = []\n\
                    [clipboard]\nosc52 = []\n\
                    [motion]\ncursor_motion = []\nreduce_motion = []\nsmooth_scroll = []\n\
                    keypress = []\nerase = []\n\
                    [shell]\nintegration = []\n\
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
                Key::Shape => SettingsEdit::Cursor(CaretShape::Beam),
                Key::Blink => SettingsEdit::CursorBlink(CursorBlink::On),
                Key::BlinkSpeed => SettingsEdit::BlinkInterval(0.5),
                Key::Radius => SettingsEdit::CursorRadius(0.1),
                Key::Glow => SettingsEdit::CursorGlow(0.5),
                Key::Unfocused => SettingsEdit::CursorUnfocused(UnfocusedCaret::Solid),
                Key::CursorMotion => SettingsEdit::CursorMotion(CursorMotion::Snap),
                Key::SmoothScroll => SettingsEdit::SmoothScroll(SmoothScroll::On),
                Key::ReduceMotion => SettingsEdit::ReduceMotion(ReduceMotion::On),
                Key::Keypress => SettingsEdit::Keypress(Keypress::Off),
                Key::Erase => SettingsEdit::Erase(Erase::Off),
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
        check::<ShellIntegration>();
        check::<CaretShape>();
        check::<CursorBlink>();
        check::<UnfocusedCaret>();
        check::<CursorMotion>();
        check::<ReduceMotion>();
        check::<Keypress>();
        check::<Erase>();
        check::<PreviewKeep>();
        check::<DownloadConflict>();
        check::<RemoteStatsMode>();
    }

    /// "Keep programs running:"'s note follows the value: under `quit` it
    /// says what quitting leaves behind and how to end it, under the others
    /// only what ends the programs anyway; no other row's note moves.
    #[test]
    fn keep_running_note_follows_the_value() {
        // A scroll bar value with no note of its own: this test asks about
        // `keep_running` alone.
        let with = |keep| Settings {
            keep_running: keep,
            scrollbar: Scrollbar::Auto,
            ..Settings::default()
        };
        let resolved = Resolved {
            reduce: false,
            scrollbar: ScrollbarMode::Auto,
        };
        assert_eq!(
            row_description(Key::KeepRunning, &with(KeepRunning::Update), resolved),
            Some("Restarting the Mac ends them.")
        );
        assert_eq!(
            row_description(Key::KeepRunning, &with(KeepRunning::Crash), resolved),
            Some("Restarting the Mac ends them.")
        );
        let quit = row_description(Key::KeepRunning, &with(KeepRunning::Quit), resolved)
            .expect("the row has a note");
        assert!(
            quit.starts_with("Programs keep running after you quit"),
            "{quit}"
        );
        assert!(quit.contains("\u{2325}\u{2318}Q ends them"), "{quit}");
        assert!(quit.ends_with("Restarting the Mac ends them."), "{quit}");
        for key in Key::ALL.into_iter().filter(|&key| key != Key::KeepRunning) {
            assert_eq!(
                row_description(key, &with(KeepRunning::Quit), resolved),
                None,
                "{key:?}"
            );
        }
    }

    /// "Scroll bar:"'s note says what "Follow System Settings" gives right
    /// now — the system's resolved form, which a mouse plugged in can change —
    /// and nothing under the values that say it themselves; no other row's
    /// note moves.
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
        for key in Key::ALL
            .into_iter()
            .filter(|&key| key != Key::Scrollbar && key != Key::KeepRunning)
        {
            assert_eq!(
                row_description(key, &system, resolved(ScrollbarMode::Always)),
                None,
                "{key:?}"
            );
        }
    }

    /// The size popups show the file's value even when it is not a preset,
    /// and only a preset writes; the defaults are presets, and every title is
    /// the spelling written with a space before the unit.
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
        for key in [Key::Keypress, Key::Erase, Key::SmoothScroll] {
            let forced = motion_override(key, &snap, false).expect("snap overrides");
            assert!(!forced.enabled, "{key:?}");
            // `snap` is above Reduce Motion: with both inputs on, the reason
            // given is `snap`.
            assert_eq!(motion_override(key, &snap, true), Some(forced), "{key:?}");
        }
        for key in [Key::Erase, Key::SmoothScroll] {
            let forced = motion_override(key, &plain, true).expect("Reduce Motion overrides");
            assert!(!forced.enabled, "{key:?}");
        }
        let fades = motion_override(Key::Keypress, &plain, true).expect("fades in");
        assert!(fades.enabled);
        assert!(fades.note.contains("fade"), "{}", fades.note);
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
        assert_eq!(seconds_label(0.5), "0.5 s");
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

    #[test]
    fn theme_list_keeps_the_file_value_visible() {
        let user = vec!["paper".to_owned()];
        let (items, index) = theme_items(SYSTEM_THEME, true, &["bateri", "bateri-light"], &user);
        assert_eq!(items[index], ThemeItem::System);
        assert_eq!(
            theme_edit(&items, index as NSInteger).as_deref(),
            Some(SYSTEM_THEME)
        );
        let (items, index) = theme_items("paper", true, &["bateri"], &user);
        assert_eq!(
            theme_edit(&items, index as NSInteger).as_deref(),
            Some("paper")
        );
        // A deleted theme: appended at the end and selected.
        let (items, index) = theme_items("gone", false, &["bateri"], &[]);
        assert_eq!(index, items.len() - 1);
        assert_eq!(items[index], ThemeItem::Named("gone".to_owned()));
        assert!(!items.contains(&ThemeItem::System));
        // A separator writes nothing.
        let (items, _) = theme_items("bateri", true, &["bateri"], &[]);
        assert_eq!(items[1], ThemeItem::Separator);
        assert_eq!(theme_edit(&items, 1), None);
    }

    #[test]
    fn font_list_matches_case_insensitively_and_keeps_unknowns() {
        let families = vec!["Menlo".to_owned(), "SF Mono".to_owned()];
        let (items, index) = font_items(None, &families);
        assert_eq!((items[index].clone(), index), (FontItem::Default, 0));
        assert_eq!(font_edit(&items, 0).as_deref(), Some(""));
        let (items, index) = font_items(Some("menlo"), &families);
        assert_eq!(items[index], FontItem::Family("Menlo".to_owned()));
        let (items, index) = font_items(Some("Comic Sans"), &families);
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
}
