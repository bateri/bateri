//! The window's own tab bar: the title row's view, drawn in the theme's
//! colours, holding one chip per tab and the buttons on the right.
//!
//! **Where it sits.** The window's content reaches under its title row
//! (`FullSizeContentView`) and an empty compact toolbar raises that row and
//! centres the traffic lights in it (`window::TerminalWindow`); the bar is
//! the root view's top strip, the row's height tall — the window reads that
//! height from AppKit, it is not a constant here. The strip starts right of
//! the zoom button ([`LIGHTS_GAP`] past its right edge); in full screen the
//! lights leave the row and the strip starts at the gap alone.
//!
//! **The bar has a second place**, a column down the window's left edge
//! (`[appearance] tab_position = "side"`): the same bar, its tabs, chips and doings, laid out
//! from `tabs::Side::layout` and `tabs::Row` instead ([`TabBar::set_side`]; `tab_side.rs`). The
//! dragging below is the column's as well, read along y. Where this file says "the strip", "the
//! chip" or "the title row", it means the bar's own place.
//!
//! **What is where** comes from the pure model (`tabs::Bar::layout`): every
//! chip's span, the separators, `+` and the settings warning, and how `+`
//! meets the window's rounded corner — given here as measured, square in
//! full screen ([`TabBar::corner`]). With one tab the bar is today's title
//! bar — the title centred, a settings diagnostic beside it
//! ([`single_label`]), `+` on the right — and the title takes no click: a
//! press anywhere but `+` moves the window, a double click does what the
//! system says ([`title_double_click`]). With several tabs each tab is a
//! chip: a press selects it (on the press, like macOS's tabs), a middle
//! click closes it, a double click names it, a right click opens its menu,
//! and while the pointer is over it a `×` shows at its left. A diagnostic
//! becomes a `⚠` left of `+`: its tooltip is the text and a click opens the
//! settings window.
//!
//! **When the tabs do not fit** they stop at their narrowest, the strip
//! scrolls (a wheel or a trackpad, a vertical wheel too) and the chips run
//! under its edges, cut by the strip's own view ([`ChipStrip`]) and faded out
//! where more lies beyond ([`EdgeFade`]); Show All Tabs joins the buttons.
//! A **change of the selection** scrolls the strip to the selected chip, and
//! so does the tabs' ceasing to fit. The list is a menu under its button (or
//! where the button would be, from ⇧⌘\): every tab, the selected one ticked,
//! a dot in the colour of what it reports, its ⌘ key. Where a chip is and how
//! far the strip scrolls are `tabs`' arithmetic; this file places views at it.
//!
//! **A tab is named in place** (a double click, Rename Tab…, the chip's menu):
//! a text field over the chip's title, Return keeps the text, Esc leaves the
//! name, the keyboard going elsewhere keeps it. What the text means — empty
//! or the tab's own title is no name — is `tabs::custom_name`, the window's
//! applier writes it ([`TerminalWindow::rename_tab`]). The chip does not
//! draw its title while the field is up, so the layouts that come with every
//! tick and hover never overwrite what is being typed.
//!
//! **What a chip shows besides its title** (several tabs only — one tab is
//! today's title bar and shows none of it): left of the title one
//! indicator, the most urgent of the tab's signals (`tabs::indicator`) — a
//! question waiting, a running command's ring, a tick or a dot for a
//! command that ended while the tab was away, a transfer's arrow; a marked
//! host's colour as a line along its top; a transfer's progress as its
//! ground filling from the left ([`ChipFill`]) — and, when it is over, the
//! ground running to the end, resting and fading away, a tick or a dot left
//! on the tab if the user was elsewhere. Between two quiet tabs a separator
//! (a tab filling with a transfer is lit, like a hovered one). While ⌘ is held
//! ([`HINT_DELAY`]) each tab ⌘1…⌘9 reaches shows its key. Over a chip for
//! [`CARD_DELAY`] the tab's summary card opens below it (`tabs::card_lines`);
//! with a card open, the next chip's comes at once. A split tab's card holds a **map** of its
//! panes under its lines, each where it stands in the tab, with its program's tile, its name and
//! what it reports (`tabs::CardPane`, `tabs::map_tile`); that card takes the pointer: it stays
//! open while the pointer is on it and for [`CARD_BRIDGE`] after the pointer left the chip, so
//! the way across the gap is free, and a press on a pane of the map shows the tab with that
//! pane focused ([`TabBar::pick_pane`]).
//!
//! **The indicator comes and goes** (`tabs::passage` says how, one rule for every shell): the
//! badge on the tile's corner grows from small past its size and settles, and goes by
//! shrinking as it fades; a badge that turns into another keeps its circle, the colour passes
//! and the new sign grows; the line round a running tile fades in and out; a tick or a dot stays
//! [`tabs::LOOKED_HOLD`] after the tab was looked at, a question does not. All of it is Core
//! Animation, set up when the indicator changes and never again — a layout that asks for what
//! the glyph already shows plays nothing, and the app draws no frame ([`Glyph`] has how). Under
//! Reduce Motion nothing grows or springs: what comes or goes fades, briefly.
//!
//! **The clock.** A running ring turns by itself — a Core Animation turn on
//! the window server's side, set once and left to run ([`Glyph`]) — so the
//! app is not woken to move it. The bar's one delayed wake
//! ([`TabBar::arm_clock`]) is for what is *not* motion: the open card's
//! running time, which counts on the duration counter's own tick
//! (`bt_core::next_tick`), and the hour, where a ring's pace changes and its
//! turn is set again; `tabs::Clock` says when. It is AppKit's redraw of a few
//! views, never a pane's frame: nothing here touches a pane's `Waker`. It
//! stops when nothing runs, when the window is not visible — the turns are
//! taken off the rings too, and set again at the right angle when the window
//! comes back — and, for the rings but not the card's seconds, under Reduce
//! Motion, where the ring stands still at the top.
//!
//! **A tab is dragged** by its chip. A press that moves past the slop along the
//! strip lifts the chip: it follows the pointer at once, the others slide to
//! make room (the same slide as any change of order — [`TabBar::lay_out`] lays
//! the tabs out in the preview order and the chip at the pointer), and the
//! window's list changes **once**, at the release, through its applier
//! ([`TerminalWindow::move_tab`]). The arithmetic — where the chip is, the
//! place it would take, when the pointer has left the bar — is `tabs::Grip`.
//! Out of the bar a drag session takes over (`tab_drag`): the chip stays,
//! faint, in its place; another window's bar opens a gap where the tab would
//! go ([`TabBar::open_gap`]) and takes the drop, anywhere else the tab becomes
//! a window of its own (`AppDelegate::tab_drag_ended`).
//!
//! **Motion** is AppKit's (`NSAnimationContext`, the design's curve): the
//! hover fill fades in 80 ms, `×` in 120 ms, chips slide when tabs come,
//! go or move, and a new one fades in, in 200 ms. Only the applier's
//! changes slide — a resize or a hover re-lays out at once. Under Reduce
//! Motion every duration is zero.
//!
//! **Drawing** is `drawRect:` with `NSBezierPath` and `NSShadow` — no layer
//! colours, which would want `CGColor` and with it an
//! `objc2-core-graphics` edge. What Core Animation moves (the running ring,
//! the transfer's fill, the tile and its badge) holds a picture painted once,
//! not a colour; so does a mask, which is why the ring cut round a badge is a
//! picture too ([`Glyph`]). The
//! colours are the theme's own roles at fixed strengths, so no theme role is
//! added; the strengths are the design's.
//!
//! **Who acts.** The bar knows its window by id and calls the window's one
//! applier (`TerminalWindow::select_tab`, `close_tab_asking`, `rename_tab`,
//! `move_tab`) —
//! the bar never changes the tab list itself. The chip's menu and the list act
//! through the window too: each item's target is the window, its `tag` the
//! tab it was opened for (`tabs::menu_tag`). A close or a new tab is done **one
//! main-queue turn later**: either rebuilds the chips, and the chip whose
//! event is being handled would be taken apart under it.
//!
//! **VoiceOver**: the bar is a tab group, each chip a radio button with the
//! tab-button subrole and its selection as its value; its label says the
//! title, the indicator and a marked host (`tabs::spoken`); `×` and `+`
//! buttons with names. A `×` that is not shown is still an element (drawn
//! invisible, taking no click), so a tab can be closed without a pointer.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use block2::RcBlock;
use bt_core::{HostMark, IconColors, Theme};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject, Sel};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityButtonRole, NSAccessibilityRadioButtonRole,
    NSAccessibilityTabButtonSubrole, NSAccessibilityTabGroupRole,
    NSAffineTransformNSAppKitAdditions, NSAnimatablePropertyContainer, NSAnimationContext,
    NSApplication, NSAutoresizingMaskOptions, NSBezierPath, NSBox, NSBoxType, NSColor,
    NSCompositingOperation, NSControl, NSControlStateValueOn, NSControlTextEditingDelegate,
    NSCursor, NSDragOperation, NSDraggingDestination, NSDraggingInfo, NSEvent, NSEventMask,
    NSEventModifierFlags, NSEventType, NSFocusRingType, NSFont, NSFontWeightMedium,
    NSFontWeightRegular, NSFontWeightSemibold, NSGradient, NSGradientDrawingOptions,
    NSGraphicsContext, NSImage, NSImageScaling, NSImageView, NSLineBreakMode, NSLineCapStyle,
    NSLineJoinStyle, NSMenu, NSMenuItem, NSShadow, NSTextAlignment, NSTextField,
    NSTextFieldDelegate, NSTitlePosition, NSTrackingArea, NSTrackingAreaOptions, NSView,
    NSWindowButton, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;
use objc2_foundation::{
    NSAffineTransform, NSArray, NSData, NSNotification, NSNumber, NSObjectProtocol, NSPoint,
    NSRect, NSSize, NSString, NSUserDefaults, NSValue, ns_string,
};
use objc2_quartz_core::{
    CAAnimation, CABasicAnimation, CACurrentMediaTime, CAKeyframeAnimation, CALayer, CAMediaTiming,
    CAMediaTimingFunction, CATransaction, CATransform3D, NSValueCATransform3DAdditions,
    kCAFillModeBackwards,
};

use crate::app;
use crate::arrange;
use crate::card::{IDENTITY, fitted};
use crate::groups::{Click, GroupId, group_place, group_spot_at, spot_at, spot_slot};
use crate::icon;
use crate::program::identity::Kind as ProgramKind;
use crate::search_bar::mix;
use crate::split::Rect;
use crate::tab_drag::{self, Carried, Landed};
use crate::tabs::{
    self, BUTTON, Bar, CHIP_PAD, Card, CardCommand, CardPane, Clock, Column, Drag, Ending, FADE,
    Finish, Grip, Indicator, Over, Stir, Strip, TAB_RADIUS, Tide, Tone, Turn,
};
use crate::window::{Seat, TerminalWindow, is_dark_background};

// The bar's other mode, the tabs in a column down the window's left edge. A child module, not a
// sibling, for the bar's private state it works on.
#[path = "tab_side.rs"]
mod tab_side;
// A group's header in that column.
#[path = "tab_group.rs"]
mod tab_group;
// The group button of the title row and the list of groups under it.
#[path = "tab_switch.rs"]
mod tab_switch;
// A title that does not fit: faded at its end, slid under the pointer.
#[path = "tab_title.rs"]
mod tab_title;

/// Space between the zoom button's right edge and the strip — the design's
/// strip starts at 84 pt beside lights ending at 72 pt. In full screen the
/// lights are not in the row and the strip starts this far from the edge.
const LIGHTS_GAP: f64 = 12.0;

/// The radius of the window's top-right corner outside full screen, which
/// `+` is fitted to (`tabs::Fit`). AppKit does not say it; it was measured
/// on macOS 26.4.1 from the alpha of a screenshot of a window built like
/// ours (titled, content under the title row, an empty compact toolbar):
/// 20 pt — and the same zoomed and tiled to a half or to fill the screen.
/// Without the toolbar the corner is 16 pt. In full screen it is square
/// ([`TabBar::corner`]). Not measured: macOS 14 and 15, and a full-screen
/// split. A smaller corner than this lies outside this one, so where the
/// real corner is smaller the gap round `+` only widens.
const WINDOW_CORNER: f64 = 20.0;

/// A chip's height, points (the design's anatomy); its corner radius is
/// `tabs::TAB_RADIUS`, shared with the buttons on a square corner. The side column's rows are
/// this high too, so the height is `tabs`'.
const CHIP_HEIGHT: f64 = tabs::TAB_HEIGHT;

/// The single tab's title box inset — it has no `×` to make room for.
const SINGLE_PAD: f64 = 8.0;

/// The `×` button: its side, its inset from the chip's corner and its
/// corner radius.
const CLOSE_SIDE: f64 = 20.0;
const CLOSE_INSET: f64 = 4.0;
const CLOSE_RADIUS: f64 = 5.0;

/// Type sizes: a chip's title, the single tab's slightly larger one, a ⌘
/// hint and the card's lines below its title.
const CHIP_TEXT: f64 = 12.5;
const SINGLE_TEXT: f64 = 13.0;
const HINT_TEXT: f64 = 11.0;
const CARD_TEXT: f64 = 11.5;

/// The host line along a chip's top and the dwell line under it (the design's 2 pt), and a
/// separator's height between two quiet tabs.
const LINE: f64 = 2.0;
const SEPARATOR_HEIGHT: f64 = 16.0;

/// The design's timings: the hover fill, `×`, and a chip's slide or fade
/// when tabs come, go or move.
const HOVER_FADE: f64 = 0.08;
const CLOSE_FADE: f64 = 0.12;
const REFLOW: f64 = 0.2;
/// How long the glow of a chip a pane joined takes to fade.
const PULSE_SECS: f64 = 0.7;

/// How long an end told to the bar waits for the tab to show nothing flowing
/// ([`TabBar::transfer_ended`]): the pane's news of it follows in the same turn.
const END_WAIT: Duration = Duration::from_secs(1);

/// How much of a chip shows while its tab is carried away in a drag session: the place it
/// left stays, faint, so the strip does not close up under the pointer that may come back.
const GHOST_ALPHA: f64 = 0.35;

/// How long the pointer rests on a chip before its summary card opens (the
/// design's 450 ms), and how long after a card closed the next chip's opens
/// at once (the prototype's 300 ms "warm" window): moving along the strip
/// with a card open does not wait again.
const CARD_DELAY: Duration = Duration::from_millis(450);
const CARD_WARM: Duration = Duration::from_millis(300);
/// How long a card with a picture outlives the pointer's leaving its chip, so the pointer can cross
/// the gap to the card (the design's 0.18 s).
const CARD_BRIDGE: Duration = Duration::from_millis(180);

/// The summary card: its corner, inner margins, the space between its lines and its gap below
/// the chip. The numbers that size the card are `tabs`' (width and height, the picture's
/// included).
const CARD_RADIUS: f64 = 10.0;
const CARD_PAD_X: f64 = tabs::CARD_PAD_X;
const CARD_PAD_Y: f64 = tabs::CARD_PAD_Y;
const CARD_LINE_GAP: f64 = tabs::CARD_LINE_GAP;
const CARD_DROP: f64 = 8.0;
/// How a card comes and goes. It arrives over [`CARD_IN`] at [`CARD_SCALE`] of its size, a
/// [`CARD_SLIDE`] nearer its chip, and fading in; it goes by fading over [`CARD_OUT`] (and is
/// taken down only when that is over); when it turns to another chip it glides there over
/// [`CARD_MOVE`] while its lines change at once, and does not arrive again.
const CARD_IN: f64 = 0.2;
const CARD_OUT: f64 = 0.12;
const CARD_MOVE: f64 = 0.15;
const CARD_SCALE: f64 = 0.96;
const CARD_SLIDE: f64 = 4.0;
/// What a card's going waits beyond its fade before taking the view down: the fade is Core
/// Animation's, begins with the next commit and is not timed by this clock.
const CARD_SLACK: f64 = 0.05;
/// The way from a card to the chip it opened under, as a step in the card's superview's own
/// (top-down) axes: the chip is above it.
const CHIP_ABOVE: (f64, f64) = (0.0, -1.0);
/// The same for a card opened beside a row of the side column: the row is to its left, so the
/// card comes in from there.
const ROW_LEFT: (f64, f64) = (-1.0, 0.0);
/// A card beside its row stands this far right of the column's edge, with its top this far above
/// the row's (the row sits level with the card's first line), and keeps this far off the
/// window's top and bottom.
const CARD_BESIDE: f64 = 8.0;
const CARD_RISE: f64 = 4.0;
const CARD_EDGE: f64 = 8.0;

/// A pane on the card's map: where its program's tile, its name and its state stand in it, the
/// type sizes, and the strengths of the foreground its face, its hovered face and its ring wear —
/// on a dark theme and on a light one (the design's).
const MAP_TILE_AT: f64 = 7.0;
const MAP_TEXT_AT: f64 = 29.0;
const MAP_TEXT_RIGHT: f64 = 7.0;
const MAP_NAME_HEIGHT: f64 = 16.0;
const MAP_STATE_AT: f64 = 23.0;
const MAP_STATE_HEIGHT: f64 = 14.0;
const MAP_STATE_TEXT: f64 = 11.0;
const MAP_FACE: (f64, f64) = (0.08, 0.035);
const MAP_FACE_HOT: (f64, f64) = (0.14, 0.08);
const MAP_RING: (f64, f64) = (0.32, 0.28);
/// A pane's picture on its tile: how strongly it is laid over the tile's face until the pointer is
/// on the tile (then fully), the scrim of the face's own colour that quiets it under the tile's
/// mark, name and state — opaque for [`MAP_SCRIM_HOLD`] of its height, then fading to nothing —
/// and how tall that is, with a state line under the name and without.
const MAP_SHOT_DIM: f64 = 0.86;
const MAP_SCRIM_HOLD: f64 = 0.55;
const MAP_SCRIM_STRENGTH: f64 = 0.94;
const MAP_SCRIM_WITH_STATE: f64 = 44.0;
const MAP_SCRIM_NAME_ONLY: f64 = 30.0;

/// How far a notch of a mouse wheel scrolls the strip, points: a wheel reports
/// lines where a trackpad reports points.
const WHEEL_STEP: f64 = 12.0;

/// How long ⌘ is held alone before the tabs show their keys — a design
/// constant: ⌘ goes down before every shortcut (⌘C, ⌘V), and hints shown
/// at once would flash across the bar at each; a hand that holds ⌘ to read
/// them waits longer than this anyway.
pub(crate) const HINT_DELAY: Duration = Duration::from_millis(300);

/// What a double click on the title row does — the system's "Double-click
/// a window's title bar to" setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TitleAction {
    Zoom,
    Minimize,
    Nothing,
}

/// The setting's answer from its two defaults keys: `AppleActionOnDoubleClick`
/// (`"Minimize"`, `"None"`, and the zoom and fill values), and the older
/// `AppleMiniaturizeOnDoubleClick` that only counts when the newer key is
/// absent. Absent both, the system zooms. The row is our own view, and
/// AppKit's title-bar handling does not reach it — measured: a double click
/// on a view in the row, taking the press or letting it through, neither
/// zoomed nor minimized the window.
pub(crate) fn title_double_click(action: Option<&str>, legacy_minimize: bool) -> TitleAction {
    match action {
        Some("Minimize") => TitleAction::Minimize,
        Some("None") => TitleAction::Nothing,
        Some(_) => TitleAction::Zoom,
        None if legacy_minimize => TitleAction::Minimize,
        None => TitleAction::Zoom,
    }
}

/// The single tab's title line: the title, and a settings diagnostic
/// beside it the way the window's subtitle used to sit
/// ("bateri – settings.toml: …").
pub(crate) fn single_label(title: &str, notice: &str) -> String {
    if notice.is_empty() {
        title.to_owned()
    } else {
        format!("{title} \u{2013} {notice}")
    }
}

/// An sRGB colour with its strength.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tint {
    pub(crate) rgb: u32,
    pub(crate) alpha: f64,
}

impl Tint {
    pub(crate) const fn of(rgb: u32, alpha: f64) -> Self {
        Self { rgb, alpha }
    }

    pub(crate) fn color(self) -> Retained<NSColor> {
        let byte = |shift: u32| f64::from((self.rgb >> shift) & 0xff) / 255.0;
        NSColor::colorWithSRGBRed_green_blue_alpha(byte(16), byte(8), byte(0), self.alpha)
    }
}

/// The bar's colours, from the theme's roles: the foreground at the
/// design's strengths for fills and lines, the dim role for a tab that is
/// not selected, `warning` for the settings warning, `accent`, `success`
/// and `error` for the indicators, the background for the card. The light
/// theme's selected chip is a white face with a thin shadow instead.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Palette {
    title: u32,
    dim: u32,
    /// The theme's background, which a scrolled strip's edges fade out to.
    ground: u32,
    warning: u32,
    accent: u32,
    success: u32,
    error: u32,
    /// The theme's `info` role and its normal eight ANSI colours (black … white): what the
    /// kinds of programs wear on a tile ([`icon::Tint`]).
    info: u32,
    ansi: [u32; 8],
    /// A dark theme: the tabs' tiles are a raised grey, on a light one white.
    dark: bool,
    selected: Tint,
    selected_line: Tint,
    /// The selected chip's drop shadow; `None` on a dark theme.
    shadow: Option<Tint>,
    /// A chip lifted by the pointer (the design's dragged tab): its face, its edge and the shadow
    /// it casts on the row.
    lifted: Tint,
    lifted_line: Tint,
    lift_shadow: Tint,
    hover: Tint,
    /// A row picked together with others in the side column: the accent's quiet tone, a face
    /// under the selected one's.
    picked: Tint,
    picked_line: Tint,
    close: Tint,
    close_hover: Tint,
    button: Tint,
    button_line: Tint,
    button_hover: Tint,
    /// The line between two quiet tabs (the design's 13 %).
    separator: Tint,
    /// The ring's track under its turning arc.
    track: Tint,
    /// The summary card's face, edge and shadow.
    card: Tint,
    card_line: Tint,
    card_shadow: Tint,
}

impl Palette {
    fn of(theme: &Theme) -> Self {
        let fg = theme.foreground;
        let ink = |alpha| Tint::of(fg, alpha);
        let dark = is_dark_background(theme);
        let base = Self {
            title: fg,
            dim: theme.dim,
            ground: theme.background,
            warning: theme.warning,
            accent: theme.accent,
            success: theme.success,
            error: theme.error,
            info: theme.info,
            ansi: std::array::from_fn(|index| theme.ansi[index]),
            dark,
            selected: ink(0.10),
            selected_line: ink(0.07),
            shadow: None,
            lifted: ink(0.14),
            lifted_line: ink(0.10),
            lift_shadow: Tint::of(0x000000, 0.6),
            hover: ink(0.05),
            picked: Tint::of(theme.accent, 0.16),
            picked_line: Tint::of(theme.accent, 0.28),
            close: ink(0.07),
            close_hover: ink(0.16),
            button: ink(0.06),
            button_line: ink(0.08),
            button_hover: ink(0.13),
            separator: ink(0.13),
            track: Tint::of(theme.accent, 0.25),
            card: Tint::of(theme.background, 1.0),
            card_line: ink(0.12),
            card_shadow: Tint::of(0x000000, 0.35),
        };
        if dark {
            base
        } else {
            Self {
                selected: Tint::of(0xffffff, 1.0),
                selected_line: ink(0.08),
                shadow: Some(ink(0.08)),
                lifted: Tint::of(0xffffff, 1.0),
                lifted_line: ink(0.08),
                lift_shadow: ink(0.18),
                close: ink(0.06),
                close_hover: ink(0.14),
                button: ink(0.05),
                button_hover: ink(0.11),
                card_shadow: Tint::of(0x000000, 0.14),
                ..base
            }
        }
    }

    /// The two ends of a transfer's fill ([`ChipFill`]): the accent's tail and leading edge, or
    /// the error's once the transfer has failed.
    fn fill_tints(self, failed: bool) -> (Tint, Tint) {
        let rgb = if failed { self.error } else { self.accent };
        (Tint::of(rgb, FILL_TAIL), Tint::of(rgb, FILL_HEAD))
    }

    /// A card line's colour: its tone's role, a marked host's own colour
    /// (`mark`, from `Theme::mark_rgb`) for the host line.
    fn tone(self, tone: Tone, mark: u32) -> u32 {
        match tone {
            Tone::Title => self.title,
            Tone::Dim => self.dim,
            Tone::Accent => self.accent,
            Tone::Warning => self.warning,
            Tone::Success => self.success,
            Tone::Error => self.error,
            Tone::Mark => mark,
        }
    }

    /// The colour of a kind's tint: the foreground, a status role or one of the normal ANSI colours.
    fn tint(self, tint: icon::Tint) -> u32 {
        match tint {
            icon::Tint::Foreground => self.title,
            icon::Tint::Info => self.info,
            icon::Tint::Error => self.error,
            icon::Tint::Red => self.ansi[1],
            icon::Tint::Green => self.ansi[2],
            icon::Tint::Yellow => self.ansi[3],
            icon::Tint::Blue => self.ansi[4],
            icon::Tint::Magenta => self.ansi[5],
            icon::Tint::Cyan => self.ansi[6],
        }
    }

    /// A tab's tile, a raised face like an application's icon: the front tile's face, the
    /// face of the deck's tiles behind it, the line round them and the shadow they cast.
    fn tile_face(self) -> u32 {
        if self.dark {
            mix(self.ground, self.title, TILE_FACE_DARK)
        } else {
            0xffffff
        }
    }

    fn tile_back(self) -> u32 {
        if self.dark {
            mix(self.ground, self.title, TILE_BACK_DARK)
        } else {
            mix(0xffffff, self.title, TILE_BACK_LIGHT)
        }
    }

    fn tile_line(self) -> Tint {
        if self.dark {
            // Light, not dark: a dark theme's ground may be black, and a black line on it is none.
            Tint::of(self.title, 0.16)
        } else {
            Tint::of(self.title, 0.16)
        }
    }

    fn tile_shadow(self) -> Tint {
        if self.dark {
            Tint::of(0x000000, 0.45)
        } else {
            Tint::of(0x000000, 0.16)
        }
    }
}

/// The durations of the bar's motions now — all zero under Reduce Motion
/// (`AppDelegate::reduce_motion`: the setting and the system's answer).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Motion {
    hover: f64,
    close: f64,
    reflow: f64,
}

impl Motion {
    fn of(reduce_motion: bool) -> Self {
        if reduce_motion {
            Self {
                hover: 0.0,
                close: 0.0,
                reflow: 0.0,
            }
        } else {
            Self {
                hover: HOVER_FADE,
                close: CLOSE_FADE,
                reflow: REFLOW,
            }
        }
    }
}

/// Runs `change` as an AppKit animation of `secs` on the design's curve
/// (`cubic-bezier(0.2, 0.8, 0.2, 1)`: quick out, slow to rest).
fn animate(secs: f64, change: impl Fn() + 'static) {
    let curve = CAMediaTimingFunction::functionWithControlPoints(0.2, 0.8, 0.2, 1.0);
    run_group(secs, curve, change);
}

/// Like [`animate`], at a constant pace: a line filling to tell how long a wait has left.
fn animate_linear(secs: f64, change: impl Fn() + 'static) {
    let curve = CAMediaTimingFunction::functionWithControlPoints(0.0, 0.0, 1.0, 1.0);
    run_group(secs, curve, change);
}

/// Like [`animate`], on one of the two curves of the comings and goings ([`Ease`]).
fn animate_eased(ease: Ease, secs: f64, change: impl Fn() + 'static) {
    run_group(secs, curve(ease), change);
}

/// Runs `change` as an AppKit animation of `secs` on `curve`.
fn run_group(secs: f64, curve: Retained<CAMediaTimingFunction>, change: impl Fn() + 'static) {
    let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
        // SAFETY: AppKit gives the block a live context, for the block's duration.
        let context = unsafe { context.as_ref() };
        context.setDuration(secs);
        context.setTimingFunction(Some(&curve));
        change();
    });
    NSAnimationContext::runAnimationGroup(&changes);
}

/// A plain filled, rounded box: a mark laid over a chip.
fn plain_box(mtm: MainThreadMarker, fill: Tint, radius: f64) -> Retained<NSBox> {
    let plate = NSBox::new(mtm);
    plate.setBoxType(NSBoxType::Custom);
    plate.setTitlePosition(NSTitlePosition::NoTitle);
    plate.setBorderWidth(0.0);
    plate.setCornerRadius(radius);
    plate.setFillColor(&fill.color());
    plate
}

/// Brings `view`'s opacity to `to`, over `secs` (at once for zero). A view
/// already at (or heading to) `to` is left alone, so the hover's re-lay-outs
/// do not restart a fade.
fn fade(view: &NSView, to: f64, secs: f64) {
    if view.alphaValue() == to {
        return;
    }
    if secs <= 0.0 {
        view.setAlphaValue(to);
        return;
    }
    let view = view.retain();
    animate(secs, move || view.animator().setAlphaValue(to));
}

/// A tracking area over `view`'s visible rect for its enter and exit —
/// in an inactive window too, like the system's tabs.
pub(crate) fn track_hover(view: &NSView) {
    let options = NSTrackingAreaOptions::MouseEnteredAndExited
        | NSTrackingAreaOptions::ActiveInActiveApp
        | NSTrackingAreaOptions::InVisibleRect;
    // SAFETY: `owner` is the view, which owns the area and outlives it; no
    // user info. `InVisibleRect` follows the view's bounds, so the rect is
    // ignored and the area never needs replacing.
    let area = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            NSRect::ZERO,
            options,
            Some(view),
            None,
        )
    };
    view.addTrackingArea(&area);
}

/// A label that sits inside a bar view and never takes a click: the view
/// under it decides what a press means.
fn label(mtm: MainThreadMarker) -> Retained<NSTextField> {
    let text = NSTextField::labelWithString(ns_string!(""), mtm);
    text.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    text.setAlignment(NSTextAlignment::Center);
    text.setMaximumNumberOfLines(1);
    text
}

/// A rounded rect `rect` filled with `fill`, its inside edge stroked with
/// `line` if any.
pub(crate) fn rounded(rect: NSRect, radius: f64, fill: Tint, line: Option<Tint>) {
    let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, radius, radius);
    fill.color().setFill();
    path.fill();
    if let Some(line) = line {
        inner_line(rect, radius, line);
    }
}

/// The 1 pt `line` along the inside edge of a rounded `rect` — [`rounded`]'s line, alone, for what
/// is painted over something else.
pub(crate) fn inner_line(rect: NSRect, radius: f64, line: Tint) {
    let inner = NSRect::new(
        NSPoint::new(rect.origin.x + 0.5, rect.origin.y + 0.5),
        NSSize::new(rect.size.width - 1.0, rect.size.height - 1.0),
    );
    let path =
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(inner, radius - 0.5, radius - 0.5);
    path.setLineWidth(1.0);
    line.color().setStroke();
    path.stroke();
}

/// A glyph's stroked path in `color`, round caps and joins.
pub(crate) fn stroke(path: &NSBezierPath, width: f64, color: Tint) {
    path.setLineWidth(width);
    path.setLineCapStyle(NSLineCapStyle::Round);
    path.setLineJoinStyle(NSLineJoinStyle::Round);
    color.color().setStroke();
    path.stroke();
}

/// Whether `rect` holds `point` (the half-open rule AppKit's hit test uses).
pub(crate) fn contains(rect: NSRect, point: NSPoint) -> bool {
    point.x >= rect.origin.x
        && point.y >= rect.origin.y
        && point.x < rect.origin.x + rect.size.width
        && point.y < rect.origin.y + rect.size.height
}

/// The centre of `rect`.
fn centre(rect: NSRect) -> NSPoint {
    NSPoint::new(
        rect.origin.x + rect.size.width / 2.0,
        rect.origin.y + rect.size.height / 2.0,
    )
}

/// Runs `job` on the main queue after `delay`, with the bar of window
/// `window` — found again by id, as the bar is not `Send` and may be gone
/// by then. `false` when `dispatch` did not take the job (a delay it cannot
/// represent).
fn after(delay: Duration, window: u64, job: impl FnOnce(&TabBar) + Send + 'static) -> bool {
    let Ok(when) = DispatchTime::try_from(delay) else {
        return false;
    };
    DispatchQueue::main()
        .after(when, move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window(window)) {
                job(window.bar());
            }
        })
        .is_ok()
}

// ─── The `×` of a chip ────────────────────────────────────────────────────

pub(crate) struct CloseIvars {
    /// The pointer is over the chip: drawn and clickable.
    shown: Cell<bool>,
    /// The pointer is over the `×` itself: the stronger fill.
    hot: Cell<bool>,
    /// A press began here; the release inside closes.
    pressed: Cell<bool>,
    palette: Cell<Option<Palette>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; CloseButton implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabClose"]
    #[ivars = CloseIvars]
    pub(crate) struct CloseButton;

    unsafe impl NSObjectProtocol for CloseButton {}

    impl CloseButton {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// Drawn always; whether it shows is its opacity, which fades
        /// ([`CloseButton::set`]).
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let Some(palette) = iv.palette.get() else {
                return;
            };
            let hot = iv.hot.get();
            let fill = if hot { palette.close_hover } else { palette.close };
            rounded(self.bounds(), CLOSE_RADIUS, fill, None);
            let at = centre(self.bounds());
            let path = NSBezierPath::bezierPath();
            path.moveToPoint(NSPoint::new(at.x - 3.0, at.y - 3.0));
            path.lineToPoint(NSPoint::new(at.x + 3.0, at.y + 3.0));
            path.moveToPoint(NSPoint::new(at.x + 3.0, at.y - 3.0));
            path.lineToPoint(NSPoint::new(at.x - 3.0, at.y + 3.0));
            let color = if hot { palette.title } else { palette.dim };
            stroke(&path, 1.5, Tint::of(color, 1.0));
        }

        /// Only while shown: a hidden `×` must not swallow the chip's press.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            self.hit_test_local(point)
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.ivars().pressed.set(true);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let pressed = self.ivars().pressed.replace(false);
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            if pressed && contains(self.bounds(), at) {
                self.close();
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.ivars().hot.set(true);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.ivars().hot.set(false);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_press(&self) -> bool {
            self.close();
            true
        }
    }
);

impl CloseButton {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(CloseIvars {
            shown: Cell::new(false),
            hot: Cell::new(false),
            pressed: Cell::new(false),
            palette: Cell::new(None),
        });
        let frame = NSRect::new(
            NSPoint::new(CLOSE_INSET, CLOSE_INSET),
            NSSize::new(CLOSE_SIDE, CLOSE_SIDE),
        );
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.setAlphaValue(0.0);
        this.setAccessibilityElement(true);
        // SAFETY: AppKit's constant role string, alive for the process.
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        this.setAccessibilityLabel(Some(ns_string!("Close Tab")));
        this.setToolTip(Some(ns_string!("Close Tab  \u{2318}W")));
        track_hover(&this);
        this
    }

    /// Shown (clickable, and faded in over `secs`) or not.
    fn set(&self, shown: bool, palette: Palette, secs: f64) {
        let iv = self.ivars();
        iv.shown.set(shown);
        if iv.palette.replace(Some(palette)) != Some(palette) {
            self.setNeedsDisplay(true);
        }
        fade(self, if shown { 1.0 } else { 0.0 }, secs);
    }

    /// Closes the chip's tab, through the chip.
    fn close(&self) {
        // SAFETY: reading the superview; we are on the main thread.
        if let Some(chip) =
            unsafe { self.superview() }.and_then(|view| view.downcast::<Chip>().ok())
        {
            chip.close_tab();
        }
    }
}

// ─── A chip's icon ────────────────────────────────────────────────────────

/// Room the glyph's view keeps round the icon's square, above, below and to the right, for what
/// overhangs it: the badge on its corner (and while it comes it grows past its size). The view
/// is as high as a chip.
const GLYPH_OVERHANG: f64 = 6.0;

/// Room it keeps to the left: the line that runs round a tile's edge stands a little outside it.
const GLYPH_LEFT: f64 = 2.0;

/// The mark's own side on its tile, and on the smaller tile of a menu row.
const MARK_TILED: f64 = 12.0;
const MARK_LISTED: f64 = 11.0;

/// The width of the line that runs round a tile's edge while a command runs.
const EDGE_STROKE: f64 = 1.4;

/// The badge on the icon's corner: its diameter (the failure's dot is smaller, a shape apart from
/// the rest) and the ring cut clear round it.
const BADGE_SIDE: f64 = 10.0;
const BADGE_DOT: f64 = 7.0;
const BADGE_HALO: f64 = 1.5;

/// The ring cut clear round the biggest badge, as a radius: the picture of the hollow is drawn
/// for it, and scaled for the smaller dot ([`Badge::hollow_share`]).
const HOLLOW_RADIUS: f64 = BADGE_SIDE / 2.0 + BADGE_HALO;

/// Half the side of the square the tile and its deck are painted in, about the badge's middle:
/// room for the tile's far corner, its shadow and a deck's cards to either side of it.
const TILE_ROOM: f64 = 24.0;

/// Half the side of the hollow's picture: opaque to this far from the badge in every direction.
/// The hollow is scaled down with the badge as it comes and goes, to [`tabs::BADGE_FROM`] of its
/// size and the dot's smaller share, and the opaque part must still reach the tile's far
/// corner ([`TILE_ROOM`]) then, or the tile loses its corner while the badge comes.
const HOLLOW_ROOM: f64 = 110.0;

/// The corner radius of a menu row's tile, and the hairline round every tile.
const TILE_RADIUS: f64 = 4.8;
const TILE_LINE: f64 = 0.75;

/// A tab's tile ([`tabs::TILE_WIDTH`] by [`tabs::TILE_HEIGHT`]): its corner radius, the
/// shadow it casts (how far down and how soft) and how much of the text's colour its face and the
/// faces behind it in a deck take over the ground on a dark theme (on a light one the face is
/// white and the ones behind take a little of the text's colour).
const TILE_ROUND: f64 = 5.0;
const TILE_DROP: f64 = 0.75;
const TILE_BLUR: f64 = 1.5;
const TILE_FACE_DARK: f64 = 0.20;
const TILE_BACK_DARK: f64 = 0.12;
const TILE_BACK_LIGHT: f64 = 0.06;

/// How far in from the tile's corner the badge's centre sits: the tile is taller than the icon's
/// square, and a badge farther out would stand out of the glyph's room above.
const TILE_BADGE_INSET: f64 = 2.5;

/// The line that runs round a tile's edge while a command runs: a rounded rectangle a little
/// outside the tile (its width, height and corner radius), drawn in a picture [`EDGE_ROOM`] on a
/// side so the stroke is not cut, leaning too. What shows of it is what a wedge —
/// [`WEDGE_HALF`] degrees to each side of its middle, a picture [`WEDGE_ROOM`] on a side that
/// reaches past the corners — lets through as it turns about the tile's centre. The tile does not
/// turn, only what shows of its edge.
const EDGE_WIDTH: f64 = tabs::TILE_WIDTH + 1.6;
const EDGE_HEIGHT: f64 = tabs::TILE_HEIGHT + 1.6;
const EDGE_RADIUS: f64 = TILE_ROUND + 0.8;
const EDGE_ROOM: f64 = 28.0;
const WEDGE_ROOM: f64 = 40.0;
const WEDGE_HALF: f64 = 38.0;

/// Into how many straight runs the wedge's turn is cut: between two the window server turns it at
/// an even rate, and `tabs::edge_angle` puts the runs where the edge's pace comes out even.
const EDGE_STEPS: usize = 64;

/// The icon's side in a menu row, and the dot on its corner.
const LIST_SIDE: f64 = 16.0;
const LIST_DOT: f64 = 5.0;

/// How the chips draw their icons (`[appearance] tab_icons` and `tab_icon_colors`): whether they
/// show them at all — without, a chip wears the small indicator alone — and in which colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IconStyle {
    icons: bool,
    colors: IconColors,
}

impl Default for IconStyle {
    /// The settings' defaults: icons, in the theme's colours.
    fn default() -> Self {
        Self {
            icons: true,
            colors: IconColors::default(),
        }
    }
}

impl IconStyle {
    /// The settings now; the defaults without an application (the tests).
    fn current(mtm: MainThreadMarker) -> Self {
        app::delegate(mtm).map_or(Self::default(), |app| {
            let settings = app.settings();
            Self {
                icons: settings.tab_icons,
                colors: settings.tab_icon_colors,
            }
        })
    }
}

/// The colours an icon is drawn in: its tile's face and its mark.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Inks {
    tile: Tint,
    mark: Tint,
}

/// What `icon` is drawn in under `style`: a raised face ([`Palette::tile_face`]) with the mark in
/// its kind's colour, or, in brand colours and where the program has one, the brand colour as its
/// face with a white mark.
fn inks(icon: &icon::Icon, style: IconStyle, palette: Palette) -> Inks {
    match icon.brand.filter(|_| style.colors == IconColors::Brand) {
        Some(rgb) => Inks {
            tile: Tint::of(rgb, 1.0),
            mark: Tint::of(0xffffff, 1.0),
        },
        None => Inks {
            tile: Tint::of(palette.tile_face(), 1.0),
            mark: Tint::of(palette.tint(icon.tint), 1.0),
        },
    }
}

/// What a chip's icon is of ([`TerminalTab::emblem`](crate::tab::TerminalTab::emblem)): the
/// program its focused pane runs, and how many panes' edges peek out behind it (a deck,
/// `tabs::deck_edges`; none for a tab of one pane).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Emblem {
    pub(crate) kind: ProgramKind,
    pub(crate) edges: usize,
}

/// What the glyph draws.
#[derive(Clone, Copy, Debug, PartialEq)]
struct GlyphLook {
    emblem: Emblem,
    style: IconStyle,
    /// What the tab reports, on the icon: the line round it while a command runs, a badge on its
    /// corner for the rest.
    indicator: Option<Indicator>,
    /// A transfer is a download: the badge's arrow points down.
    down: bool,
}

/// A badge on a tile's corner: what the tab reports other than a command running, and for a
/// transfer which way it flows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Badge {
    indicator: Indicator,
    down: bool,
}

/// The circles a badge can be, one per colour: a badge that becomes another one keeps its circle
/// and the colour passes from one picture to the other.
const DISCS: [Indicator; 4] = [
    Indicator::Question,
    Indicator::Finished,
    Indicator::Failed,
    Indicator::Uploading,
];

/// The side of the circle `disc` is: the failure's dot is smaller than the rest.
fn disc_side(disc: usize) -> f64 {
    if DISCS[disc] == Indicator::Failed {
        BADGE_DOT
    } else {
        BADGE_SIDE
    }
}

impl Badge {
    /// The badge `look` wears, if it wears one.
    fn of(look: Option<GlyphLook>) -> Option<Self> {
        let look = look?;
        let indicator = look
            .indicator
            .filter(|&indicator| tabs::is_badge(indicator))?;
        Some(Self {
            indicator,
            down: look.down && indicator == Indicator::Uploading,
        })
    }

    /// Which of the [`DISCS`] it is.
    fn disc(self) -> usize {
        match self.indicator {
            Indicator::Question => 0,
            Indicator::Finished => 1,
            Indicator::Failed => 2,
            Indicator::Running | Indicator::Uploading => 3,
        }
    }

    /// Which of the four signs is on it — a question, a tick, an arrow up or down; the failure's
    /// dot has none.
    fn sign(self) -> Option<usize> {
        match self.indicator {
            Indicator::Question => Some(0),
            Indicator::Finished => Some(1),
            Indicator::Uploading => Some(if self.down { 3 } else { 2 }),
            Indicator::Running | Indicator::Failed => None,
        }
    }

    /// How large the hollow round it is, as a share of the hollow's picture (drawn for the
    /// full circle): the ring cut clear round a badge is [`BADGE_HALO`] wide whatever its size.
    fn hollow_share(self) -> f64 {
        (disc_side(self.disc()) / 2.0 + BADGE_HALO) / HOLLOW_RADIUS
    }
}

/// A badge that is on the tile, and when it will be gone if it is on its way out.
#[derive(Clone, Copy, Debug)]
struct Worn {
    badge: Badge,
    gone: Option<Instant>,
}

pub(crate) struct GlyphIvars {
    look: Cell<Option<GlyphLook>>,
    /// The tile with its deck, a picture, laid out twice over the same place: `plain` is what
    /// shows while the tile wears no badge, `notched` what shows while it does — the same picture
    /// seen through the `hollow`, the ring cut clear round the badge, which comes and goes with it.
    /// A layer's mask cannot take a shape out of a layer, only a picture with the shape gone from
    /// it, and the ring must not be the chip's colour (a chip is lit differently selected,
    /// hovered and not).
    plain: Retained<CALayer>,
    notched: Retained<CALayer>,
    /// The notched tile's mask: opaque all round, clear in the middle. It stands at the middle
    /// of the badge and is scaled with it.
    hollow: Retained<CALayer>,
    /// The badge: its circles and its signs, all in place and all but the ones that show clear, so
    /// that what changes is opacity and size. It comes and goes as one.
    pop: Retained<CALayer>,
    discs: [Retained<CALayer>; 4],
    signs: [Retained<CALayer>; 4],
    /// The badge on the tile now, with its way out if it is on one. A badge on its way out is
    /// still on the tile until it is gone, and one that comes meanwhile becomes it.
    shown: Cell<Option<Worn>>,
    /// The tile has been shown: what changes from here plays, what a glyph is born with or comes
    /// back to from being hidden (a tab being named) is laid out at rest.
    seen: Cell<bool>,
    /// The number of cards behind the tile the layers stand for (they lean the badge's place).
    placed: Cell<Option<usize>>,
    /// What the tile's and the badge's pictures were drawn in: the style, the colours, the scale
    /// of the screen and the deck.
    tile_ink: Cell<Option<(Emblem, IconStyle, Palette, f64)>>,
    badge_ink: Cell<Option<(Palette, f64, usize)>>,
    /// The running ring's arc, a layer of its own that the turn is played on: the glyph's
    /// own backing layer is AppKit's (its anchor is a corner, and AppKit resets what it sets).
    arc: Retained<CALayer>,
    /// A tile's edge while a command runs, a picture that stands still, and the wedge that lets a
    /// part of it show ([`EDGE_WIDTH`]): the wedge is the edge's mask and the turn is played on it.
    edge: Retained<CALayer>,
    wedge: Retained<CALayer>,
    /// The pace of the turn playing and on what (one whole turn takes this long), `None` while
    /// nothing turns. A turn set again at the same pace is left alone: setting it anew at every
    /// layout would make it jump back.
    turning: Cell<Option<(Duration, Spin)>>,
    /// What the arc's picture was drawn in: its colour and the scale of the screen it is on.
    arc_ink: Cell<Option<(u32, f64)>>,
    /// What the edge's and the wedge's pictures were drawn in: the colour, the scale and the deck's
    /// cards behind (they turn the edge with the front tile).
    edge_ink: Cell<Option<(u32, f64, usize)>>,
    palette: Cell<Option<Palette>>,
}

/// What turns on a running glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Spin {
    /// Without icons, the small indicator's accent arc: a picture that turns.
    Ring,
    /// A tile's edge: a picture that stands still, seen through a wedge that turns; with the
    /// number of cards behind the tile, which turns the front one (`tabs::deck_card`).
    Edge(usize),
}

/// What would turn on `look`, if anything: a tile's edge, or without icons the small indicator's
/// ring.
fn spin_of(look: Option<GlyphLook>) -> Option<Spin> {
    let look = look.filter(|look| look.indicator == Some(Indicator::Running))?;
    Some(if look.style.icons {
        Spin::Edge(look.emblem.edges.min(tabs::DECK_EDGES))
    } else {
        Spin::Ring
    })
}

define_class!(
    // SAFETY: NSView is designed for subclassing; Glyph implements no `Drop`
    // and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabGlyph"]
    #[ivars = GlyphIvars]
    pub(crate) struct Glyph;

    unsafe impl NSObjectProtocol for Glyph {}

    impl Glyph {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The chip under it takes the press.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        /// The glyph moved to a screen of another scale (or into a window): the pictures of what
        /// is a layer are drawn again at it.
        #[unsafe(method(viewDidChangeBackingProperties))]
        fn view_did_change_backing_properties(&self) {
            self.ink();
        }

        /// The small indicator of a tab without icons. Everything else the glyph shows — the
        /// tile, the badge, the line round it — is a layer of its own, which is what lets it come
        /// and go.
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let (Some(look), Some(palette)) = (iv.look.get(), iv.palette.get()) else {
                return;
            };
            if look.style.icons {
                return;
            }
            if let Some(indicator) = look.indicator {
                draw_bare(indicator, look.down, indicator_slot().origin, palette);
            }
        }
    }
);

/// The tile the icon stands on, in the glyph's own (flipped) space: [`GLYPH_LEFT`] in from its
/// left edge, at the middle of the [`tabs::ICON_SIDE`] square that has [`GLYPH_OVERHANG`] above.
fn glyph_slot() -> NSRect {
    NSRect::new(
        NSPoint::new(
            GLYPH_LEFT,
            GLYPH_OVERHANG + (tabs::ICON_SIDE - tabs::TILE_HEIGHT) / 2.0,
        ),
        NSSize::new(tabs::TILE_WIDTH, tabs::TILE_HEIGHT),
    )
}

/// The square the small indicator sits in when there is no icon (`tab_icons = false`): the icon's
/// square less what [`tabs::INDICATOR_SIDE`] leaves, at the same middle.
fn indicator_slot() -> NSRect {
    let room = (tabs::ICON_SIDE - tabs::INDICATOR_SIDE) / 2.0;
    NSRect::new(
        NSPoint::new(GLYPH_LEFT, GLYPH_OVERHANG + room),
        NSSize::new(tabs::INDICATOR_SIDE, tabs::INDICATOR_SIDE),
    )
}

/// `rect`'s middle, as a square of the given `side`.
fn centred_square(rect: NSRect, side: f64) -> NSRect {
    NSRect::new(
        NSPoint::new(
            rect.origin.x + (rect.size.width - side) / 2.0,
            rect.origin.y + (rect.size.height - side) / 2.0,
        ),
        NSSize::new(side, side),
    )
}

/// The glyph's tile: behind it the deck if the tab holds more panes (its cards smaller and less
/// turned the further back, `tabs::deck_card`), and on it the program's mark. What the tab
/// reports is not in it: the badge on its corner and the line round its edge are layers of their
/// own, which is what lets them come and go. With no icons (`tab_icons = false`) there is no
/// tile, only the small indicator.
fn draw_tile(emblem: Emblem, style: IconStyle, palette: Palette) {
    let slot = glyph_slot();
    let Emblem { kind, edges } = emblem;
    let inks = inks(icon::of(kind), style, palette);
    // The cards behind, back to front, each turned about its own middle.
    for step in (1..=edges.min(tabs::DECK_EDGES)).rev() {
        let card = tabs::deck_card(slot.size.width, step, edges);
        let rect = card_rect(slot, card);
        NSGraphicsContext::saveGraphicsState_class();
        turn(rect, card.turn);
        draw_face(rect, TILE_ROUND, palette.tile_back(), palette);
        NSGraphicsContext::restoreGraphicsState_class();
    }
    NSGraphicsContext::saveGraphicsState_class();
    turn(slot, tabs::deck_card(slot.size.width, 0, edges).turn);
    draw_face(slot, TILE_ROUND, inks.tile.rgb, palette);
    draw_mark(kind, centred_square(slot, MARK_TILED), &inks.mark.color());
    NSGraphicsContext::restoreGraphicsState_class();
}

/// [`draw_tile`] in a picture [`TILE_ROOM`] to each side of the badge's middle, which is the
/// middle of the layers the picture is laid in: the badge, its hollow and the tile all turn and
/// scale about the same point.
fn draw_tile_about(emblem: Emblem, style: IconStyle, palette: Palette) {
    let at = badge_centre(emblem.edges);
    NSGraphicsContext::saveGraphicsState_class();
    let shift = NSAffineTransform::transform();
    shift.translateXBy_yBy(TILE_ROOM - at.x, TILE_ROOM - at.y);
    shift.concat();
    draw_tile(emblem, style, palette);
    NSGraphicsContext::restoreGraphicsState_class();
}

/// Where the badge's middle is in the glyph's space: the tile's corner, [`TILE_BADGE_INSET`] in
/// from it, turned with the front card of the deck (the badge sits on the card, which leans).
fn badge_centre(edges: usize) -> NSPoint {
    let slot = glyph_slot();
    let corner = NSPoint::new(
        slot.origin.x + slot.size.width - TILE_BADGE_INSET,
        slot.origin.y + TILE_BADGE_INSET,
    );
    let middle = NSPoint::new(
        slot.origin.x + slot.size.width / 2.0,
        slot.origin.y + slot.size.height / 2.0,
    );
    // The card is drawn with `turn`, which turns counter-clockwise on screen: in the glyph's
    // flipped space that is the negative angle.
    let angle = -tabs::deck_card(slot.size.width, 0, edges).turn.to_radians();
    let (dx, dy) = (corner.x - middle.x, corner.y - middle.y);
    NSPoint::new(
        middle.x + dx * angle.cos() - dy * angle.sin(),
        middle.y + dx * angle.sin() + dy * angle.cos(),
    )
}

/// Where a deck's `card` stands, from the front card's `slot`: its size a share of the front
/// card's, its middle `dx` to the right on the same middle line.
fn card_rect(slot: NSRect, card: tabs::DeckCard) -> NSRect {
    let (width, height) = (slot.size.width * card.scale, slot.size.height * card.scale);
    NSRect::new(
        NSPoint::new(
            slot.origin.x + slot.size.width / 2.0 + card.dx - width / 2.0,
            slot.origin.y + (slot.size.height - height) / 2.0,
        ),
        NSSize::new(width, height),
    )
}

/// Turns what is drawn next `degrees` counter-clockwise on screen about `rect`'s middle (the
/// glyph's space is flipped: a positive angle there goes clockwise).
fn turn(rect: NSRect, degrees: f64) {
    if degrees == 0.0 {
        return;
    }
    let (x, y) = (
        rect.origin.x + rect.size.width / 2.0,
        rect.origin.y + rect.size.height / 2.0,
    );
    let turn = NSAffineTransform::transform();
    turn.translateXBy_yBy(x, y);
    turn.rotateByDegrees(-degrees);
    turn.translateXBy_yBy(-x, -y);
    turn.concat();
}

/// One raised face in `rect`, `radius` round — a tab's tile, like an application's icon:
/// filled in `fill` over its shadow, with a hairline round it.
fn draw_face(rect: NSRect, radius: f64, fill: u32, palette: Palette) {
    let inset = TILE_LINE / 2.0;
    let rect = NSRect::new(
        NSPoint::new(rect.origin.x + inset, rect.origin.y + inset),
        NSSize::new(rect.size.width - TILE_LINE, rect.size.height - TILE_LINE),
    );
    let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
        rect,
        radius - inset,
        radius - inset,
    );
    NSGraphicsContext::saveGraphicsState_class();
    let shadow = NSShadow::new();
    shadow.setShadowOffset(NSSize::new(0.0, -TILE_DROP));
    shadow.setShadowBlurRadius(TILE_BLUR);
    shadow.setShadowColor(Some(&palette.tile_shadow().color()));
    shadow.set();
    Tint::of(fill, 1.0).color().setFill();
    path.fill();
    NSGraphicsContext::restoreGraphicsState_class();
    path.setLineWidth(TILE_LINE);
    palette.tile_line().color().setStroke();
    path.stroke();
}

/// The program's mark in `rect` (a square): its path data scaled into it and filled or stroked as
/// its source has it, or the one mark that exists only as a picture, tinted.
fn draw_mark(kind: ProgramKind, rect: NSRect, ink: &NSColor) {
    match icon::of(kind).art {
        icon::Art::Path(shape) => {
            let Ok(segments) = shape.segments() else {
                return;
            };
            let fit = shape.view_box.fit(rect.size.width);
            let place = |p: icon::Point| NSPoint::new(rect.origin.x + p.x, rect.origin.y + p.y);
            let path = NSBezierPath::bezierPath();
            for segment in segments {
                match fit.segment(segment) {
                    icon::Segment::Move(to) => path.moveToPoint(place(to)),
                    icon::Segment::Line(to) => path.lineToPoint(place(to)),
                    icon::Segment::Cubic { c1, c2, to } => path
                        .curveToPoint_controlPoint1_controlPoint2(place(to), place(c1), place(c2)),
                    icon::Segment::Close => path.closePath(),
                }
            }
            match shape.paint {
                icon::Paint::Fill => {
                    ink.setFill();
                    path.fill();
                }
                icon::Paint::Stroke { width } => {
                    path.setLineWidth(fit.length(width));
                    path.setLineCapStyle(NSLineCapStyle::Round);
                    path.setLineJoinStyle(NSLineJoinStyle::Round);
                    ink.setStroke();
                    path.stroke();
                }
            }
        }
        icon::Art::Mask(png) => draw_mask(png, rect, ink),
    }
}

/// A picture whose alpha is the shape, in `rect` and in `ink`: the picture is laid on a clear
/// image of its own and the colour poured over what it covers.
fn draw_mask(png: &'static [u8], rect: NSRect, ink: &NSColor) {
    let data = NSData::with_bytes(png);
    let Some(mask) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let color = ink.retain();
    let draw = RcBlock::new(move |bounds: NSRect| -> Bool {
        mask.drawInRect(bounds);
        NSGraphicsContext::saveGraphicsState_class();
        if let Some(context) = NSGraphicsContext::currentContext() {
            context.setCompositingOperation(NSCompositingOperation::SourceIn);
        }
        color.setFill();
        NSBezierPath::fillRect(bounds);
        NSGraphicsContext::restoreGraphicsState_class();
        Bool::YES
    });
    let tinted = NSImage::imageWithSize_flipped_drawingHandler(rect.size, true, &draw);
    tinted.drawInRect(rect);
}

/// "Running", without icons: the small indicator's quarter arc in `accent`, resting at the top and
/// running clockwise from there — the picture the turning layer carries. Drawn in a flipped square
/// like the glyph's own, where angles grow clockwise on screen and −90° is the top.
fn draw_arc(color: u32) {
    let (at, radius) = bare_circle(NSPoint::ZERO);
    let arc = NSBezierPath::bezierPath();
    arc.appendBezierPathWithArcWithCenter_radius_startAngle_endAngle_clockwise(
        at, radius, -90.0, 0.0, false,
    );
    stroke(&arc, BARE_WIDTH, Tint::of(color, 1.0));
}

/// A tile's edge: the rounded rectangle a little outside the tile, in `accent`, stroked whole and
/// turned with the front tile of a deck with `edges` cards behind it. Only a part of it shows,
/// through [`draw_wedge`].
fn draw_edge(color: u32, edges: usize) {
    let rect = NSRect::new(
        NSPoint::new(
            (EDGE_ROOM - EDGE_WIDTH) / 2.0,
            (EDGE_ROOM - EDGE_HEIGHT) / 2.0,
        ),
        NSSize::new(EDGE_WIDTH, EDGE_HEIGHT),
    );
    turn(rect, tabs::deck_card(tabs::TILE_WIDTH, 0, edges).turn);
    let path =
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, EDGE_RADIUS, EDGE_RADIUS);
    stroke(&path, EDGE_STROKE, Tint::of(color, 1.0));
}

/// The wedge the edge is seen through: a slice from the middle of its square, pointing at the top
/// and [`WEDGE_HALF`] degrees to each side. Only its alpha counts.
fn draw_wedge() {
    let middle = NSPoint::new(WEDGE_ROOM / 2.0, WEDGE_ROOM / 2.0);
    let wedge = NSBezierPath::bezierPath();
    wedge.moveToPoint(middle);
    wedge.appendBezierPathWithArcWithCenter_radius_startAngle_endAngle_clockwise(
        middle,
        WEDGE_ROOM / 2.0,
        -90.0 - WEDGE_HALF,
        -90.0 + WEDGE_HALF,
        false,
    );
    wedge.closePath();
    NSColor::blackColor().setFill();
    wedge.fill();
}

// ─── The small indicator, for a tab with no icon ──────────────────────────

/// The small indicator's strokes: what a tab wore before it had icons — the ring and the tick, a
/// little heavier than the icon's ring.
const BARE_WIDTH: f64 = 1.6;

/// A point of the indicator's own 12-unit grid, `at` the origin of its square.
fn bare_point(at: NSPoint, x: f64, y: f64) -> NSPoint {
    NSPoint::new(at.x + x, at.y + y)
}

/// The indicator for `indicator`, in the role of its meaning (`tabs::list_dot`, the Show All Tabs
/// dots' table too), in the square at `at`.
fn draw_bare(indicator: Indicator, down: bool, at: NSPoint, palette: Palette) {
    let role = palette.tone(tabs::list_dot(indicator), 0);
    match indicator {
        Indicator::Question => draw_bare_question(at, role),
        Indicator::Running => draw_bare_track(at, palette),
        Indicator::Finished => draw_bare_tick(at, role),
        Indicator::Failed => draw_bare_dot(at, role),
        Indicator::Uploading => draw_bare_arrow(at, down, role),
    }
}

/// "Waiting for an answer": a ring with a question mark in it.
fn draw_bare_question(at: NSPoint, color: u32) {
    let side = tabs::INDICATOR_SIDE;
    let ink = Tint::of(color, 1.0);
    let ring = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
        bare_point(at, 0.75, 0.75),
        NSSize::new(side - 1.5, side - 1.5),
    ));
    stroke(&ring, 1.3, ink);
    let mark = NSBezierPath::bezierPath();
    mark.moveToPoint(bare_point(at, 4.3, 4.6));
    mark.curveToPoint_controlPoint1_controlPoint2(
        bare_point(at, 7.7, 4.6),
        bare_point(at, 4.3, 2.5),
        bare_point(at, 7.7, 2.5),
    );
    mark.curveToPoint_controlPoint1_controlPoint2(
        bare_point(at, 6.0, 7.0),
        bare_point(at, 7.7, 5.8),
        bare_point(at, 6.0, 5.8),
    );
    stroke(&mark, 1.3, ink);
    let dot = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
        bare_point(at, 5.2, 8.05),
        NSSize::new(1.6, 1.6),
    ));
    ink.color().setFill();
    dot.fill();
}

/// "Running", the small indicator's ring: its centre and radius in the square at `at`.
fn bare_circle(at: NSPoint) -> (NSPoint, f64) {
    let side = tabs::INDICATOR_SIDE;
    (bare_point(at, side / 2.0, side / 2.0), side / 2.0 - 1.4)
}

/// "Running": the faint ring under the turning arc ([`draw_arc`]).
fn draw_bare_track(at: NSPoint, palette: Palette) {
    let (centre, radius) = bare_circle(at);
    let track = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
        NSPoint::new(centre.x - radius, centre.y - radius),
        NSSize::new(2.0 * radius, 2.0 * radius),
    ));
    stroke(&track, BARE_WIDTH, palette.track);
}

/// "Finished while you were away": a tick.
fn draw_bare_tick(at: NSPoint, color: u32) {
    let path = NSBezierPath::bezierPath();
    path.moveToPoint(bare_point(at, 2.6, 6.4));
    path.lineToPoint(bare_point(at, 5.0, 8.8));
    path.lineToPoint(bare_point(at, 9.4, 3.6));
    stroke(&path, 1.8, Tint::of(color, 1.0));
}

/// "Failed while you were away": a full dot — a shape apart from the tick, so the two part
/// without their colours too.
fn draw_bare_dot(at: NSPoint, color: u32) {
    let side = tabs::INDICATOR_SIDE;
    let dot = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
        bare_point(at, 2.5, 2.5),
        NSSize::new(side - 5.0, side - 5.0),
    ));
    Tint::of(color, 1.0).color().setFill();
    dot.fill();
}

/// "A transfer flows": an arrow, up for an upload and down for a download (the title prefix's
/// `↑`/`↓`).
fn draw_bare_arrow(at: NSPoint, down: bool, color: u32) {
    let (tip, tail, head) = if down {
        (9.8, 2.2, 7.0)
    } else {
        (2.2, 9.8, 5.0)
    };
    let path = NSBezierPath::bezierPath();
    path.moveToPoint(bare_point(at, 6.0, tail));
    path.lineToPoint(bare_point(at, 6.0, tip));
    path.moveToPoint(bare_point(at, 2.8, head));
    path.lineToPoint(bare_point(at, 6.0, tip));
    path.lineToPoint(bare_point(at, 9.2, head));
    stroke(&path, 1.6, Tint::of(color, 1.0));
}

// ─── The icon in a menu row ───────────────────────────────────────────────

/// The icon of tab `label` for a menu row (Show All Tabs): its mark on its tile, as the chip wears
/// it, and what the tab reports as a dot on the corner.
fn list_image(label: &Label, style: IconStyle, palette: Palette) -> Retained<NSImage> {
    listed_image(label.emblem.kind, label.indicator, style, palette)
}

/// The icon of a tab for a row of the command palette: the Show All Tabs row's, drawn in the
/// colours of `theme` and the icon settings now, so a tab looks the same in the bar, in that
/// list and in the palette.
pub(crate) fn palette_icon(
    mtm: MainThreadMarker,
    emblem: Emblem,
    indicator: Option<Indicator>,
    theme: &Theme,
) -> Retained<NSImage> {
    listed_image(
        emblem.kind,
        indicator,
        IconStyle::current(mtm),
        Palette::of(theme),
    )
}

/// The side of the icon [`palette_icon`] draws, in points.
pub(crate) const PALETTE_ICON_SIDE: f64 = LIST_SIDE;

/// A program's mark on its tile with what it reports as a dot on the corner, [`LIST_SIDE`]
/// across: the menu row's icon, and a pane's on the card's map.
fn listed_image(
    kind: ProgramKind,
    indicator: Option<Indicator>,
    style: IconStyle,
    palette: Palette,
) -> Retained<NSImage> {
    if !style.icons {
        let dot = indicator.map(|indicator| palette.tone(tabs::list_dot(indicator), 0));
        return dot_image(dot);
    }
    let draw = RcBlock::new(move |_rect: NSRect| -> Bool {
        draw_listed(kind, indicator, style, palette);
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(LIST_SIDE, LIST_SIDE), true, &draw)
}

/// [`list_image`]'s drawing, in a [`LIST_SIDE`] square at the origin.
fn draw_listed(
    kind: ProgramKind,
    indicator: Option<Indicator>,
    style: IconStyle,
    palette: Palette,
) {
    let slot = NSRect::new(NSPoint::ZERO, NSSize::new(LIST_SIDE, LIST_SIDE));
    let inks = inks(icon::of(kind), style, palette);
    draw_face(slot, TILE_RADIUS, inks.tile.rgb, palette);
    draw_mark(kind, centred_square(slot, MARK_LISTED), &inks.mark.color());
    if let Some(indicator) = indicator {
        draw_corner_dot(indicator, slot, palette);
    }
}

/// What the tab reports as a dot in the role of its meaning on the corner of `slot`, with the
/// icon cut clear round it.
fn draw_corner_dot(indicator: Indicator, slot: NSRect, palette: Palette) {
    let role = palette.tone(tabs::list_dot(indicator), 0);
    let centre = NSPoint::new(
        slot.origin.x + slot.size.width - LIST_DOT / 2.0,
        slot.origin.y + LIST_DOT / 2.0,
    );
    let circle = |side: f64| {
        NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
            NSPoint::new(centre.x - side / 2.0, centre.y - side / 2.0),
            NSSize::new(side, side),
        ))
    };
    NSGraphicsContext::saveGraphicsState_class();
    if let Some(context) = NSGraphicsContext::currentContext() {
        context.setCompositingOperation(NSCompositingOperation::Clear);
    }
    circle(LIST_DOT + 2.0).fill();
    NSGraphicsContext::restoreGraphicsState_class();
    Tint::of(role, 1.0).color().setFill();
    circle(LIST_DOT).fill();
}

/// A badge's circle, `side` across, filled in the colour of its meaning (`tabs::list_dot`).
fn draw_disc(color: u32, side: f64) {
    let circle =
        NSBezierPath::bezierPathWithOvalInRect(NSRect::new(NSPoint::ZERO, NSSize::new(side, side)));
    Tint::of(color, 1.0).color().setFill();
    circle.fill();
}

/// One of a badge's four signs — a question, a tick, an arrow up, an arrow down — in a
/// [`BADGE_SIDE`] square, leaning as far as the front card of the deck does.
fn draw_sign(sign: usize, lean: f64, ink: Tint) {
    let square = NSRect::new(NSPoint::ZERO, NSSize::new(BADGE_SIDE, BADGE_SIDE));
    let centre = NSPoint::new(BADGE_SIDE / 2.0, BADGE_SIDE / 2.0);
    NSGraphicsContext::saveGraphicsState_class();
    turn(square, lean);
    match sign {
        0 => draw_query(centre, ink),
        1 => draw_check(centre, ink),
        2 => draw_arrow(centre, false, ink),
        _ => draw_arrow(centre, true, ink),
    }
    NSGraphicsContext::restoreGraphicsState_class();
}

/// The hollow: opaque all round, and clear in a circle at the middle that is the biggest badge
/// with the ring cut round it. Only its alpha counts, as the notched tile's mask.
fn draw_hollow() {
    let side = 2.0 * HOLLOW_ROOM;
    NSColor::blackColor().setFill();
    NSBezierPath::fillRect(NSRect::new(NSPoint::ZERO, NSSize::new(side, side)));
    NSGraphicsContext::saveGraphicsState_class();
    if let Some(context) = NSGraphicsContext::currentContext() {
        context.setCompositingOperation(NSCompositingOperation::Clear);
    }
    NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
        NSPoint::new(HOLLOW_ROOM - HOLLOW_RADIUS, HOLLOW_ROOM - HOLLOW_RADIUS),
        NSSize::new(2.0 * HOLLOW_RADIUS, 2.0 * HOLLOW_RADIUS),
    ))
    .fill();
    NSGraphicsContext::restoreGraphicsState_class();
}

thread_local! {
    /// The hollow's picture and the scale it was drawn at: one for every glyph (it is big, for
    /// the reason in [`HOLLOW_ROOM`], and the same for all).
    static HOLLOW: RefCell<Option<(f64, Retained<AnyObject>)>> = const { RefCell::new(None) };
}

/// The hollow's picture at `scale`, drawn once and shared.
fn hollow_picture(scale: f64) -> Retained<AnyObject> {
    HOLLOW.with(|shared| {
        if let Some((at, picture)) = shared.borrow().as_ref()
            && *at == scale
        {
            return picture.clone();
        }
        let block = RcBlock::new(|_rect: NSRect| -> Bool {
            draw_hollow();
            Bool::YES
        });
        let side = 2.0 * HOLLOW_ROOM;
        let image =
            NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(side, side), true, &block);
        let picture = image.layerContentsForContentsScale(scale);
        *shared.borrow_mut() = Some((scale, picture.clone()));
        picture
    })
}

/// A point of the 12-unit grid the marks below are drawn in, placed at `scale` about `centre`.
fn on_grid(centre: NSPoint, scale: f64, x: f64, y: f64) -> NSPoint {
    NSPoint::new(centre.x + (x - 6.0) * scale, centre.y + (y - 6.0) * scale)
}

/// "Waiting for an answer": a question mark in the badge.
fn draw_query(centre: NSPoint, ink: Tint) {
    let at = |x, y| on_grid(centre, 0.7, x, y);
    let mark = NSBezierPath::bezierPath();
    mark.moveToPoint(at(4.3, 4.6));
    mark.curveToPoint_controlPoint1_controlPoint2(at(7.7, 4.6), at(4.3, 2.5), at(7.7, 2.5));
    mark.curveToPoint_controlPoint1_controlPoint2(at(6.0, 7.0), at(7.7, 5.8), at(6.0, 5.8));
    stroke(&mark, 1.2, ink);
    let dot = at(6.0, 8.85);
    let dot = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
        NSPoint::new(dot.x - 0.7, dot.y - 0.7),
        NSSize::new(1.4, 1.4),
    ));
    ink.color().setFill();
    dot.fill();
}

/// "Finished while you were away": a tick in the badge.
fn draw_check(centre: NSPoint, ink: Tint) {
    let at = |x, y| on_grid(centre, 0.6, x, y);
    let path = NSBezierPath::bezierPath();
    path.moveToPoint(at(2.5, 6.4));
    path.lineToPoint(at(5.0, 8.8));
    path.lineToPoint(at(9.8, 3.4));
    stroke(&path, 1.4, ink);
}

/// "A transfer flows": an arrow in the badge, up for an upload and down for a download (the
/// title prefix's `↑`/`↓`).
fn draw_arrow(centre: NSPoint, down: bool, ink: Tint) {
    let at = |x, y| on_grid(centre, 0.55, x, y);
    let (tip, tail, head) = if down {
        (9.8, 2.2, 7.0)
    } else {
        (2.2, 9.8, 5.0)
    };
    let path = NSBezierPath::bezierPath();
    path.moveToPoint(at(6.0, tail));
    path.lineToPoint(at(6.0, tip));
    path.moveToPoint(at(2.8, head));
    path.lineToPoint(at(6.0, tip));
    path.lineToPoint(at(9.2, head));
    stroke(&path, 1.2, ink);
}

/// The key the turn is played under on the arc's layer.
fn turn_key() -> &'static NSString {
    ns_string!("turn")
}

/// A tab's icon: the tile and its deck, the badge on its corner and the line round its edge — each a
/// layer of its own holding a picture painted when its colours change, so that each can come and
/// go (the module's note on the indicator).
///
/// **The model is always the end of what plays; the animation shows the way.** A badge that goes
/// is the plain tile in the model at once and Core Animation shows the rest: the notched tile
/// held up for the going and the badge's own shrink, both after a delay when the tab was
/// looked at ([`tabs::LOOKED_HOLD`]) — Core Animation's own `beginTime`, with the layer showing
/// its first value until then, so no timer, no later undoing and no wake of the app. Whatever
/// comes next while it plays finds the badge still on the tile ([`Worn::gone`]) and turns it
/// into the new one, or brings it back, from where it stands. A glyph born with what it shows,
/// or back from being hidden (a tab being named), is laid out at rest and plays nothing.
///
/// **The ring cut clear round the badge is the tile's mask.** A mask only keeps what it covers,
/// so the notched tile (the same picture as the plain one) is seen through a picture that is
/// opaque but for a circle at the badge's middle, scaled about that middle with the badge: the
/// ring is cut out of the tile and the chip's own ground shows through it whatever its colour
/// (selected, hovered, lifted). The picture is shared by every glyph.
impl Glyph {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // The layers' own anchors are the middle (a view's backing layer's is a corner), so a turn
        // about z is a turn about the ring's or the tile's centre. Hidden until a command runs. The
        // icon's square is mid-way down the glyph, so the centre is the glyph's.
        let middle = NSPoint::new(
            GLYPH_LEFT + tabs::ICON_SIDE / 2.0,
            GLYPH_OVERHANG + tabs::ICON_SIDE / 2.0,
        );
        let layer = |side: f64, at: NSPoint| {
            let layer = CALayer::new();
            layer.setBounds(NSRect::new(NSPoint::ZERO, NSSize::new(side, side)));
            layer.setPosition(at);
            layer
        };
        // The arc is the small indicator's, in its square.
        let arc = layer(
            tabs::INDICATOR_SIDE,
            NSPoint::new(GLYPH_LEFT + tabs::INDICATOR_SIDE / 2.0, middle.y),
        );
        // The edge goes round the tile, wider than the square: its middle is the tile's.
        let edge = layer(
            EDGE_ROOM,
            NSPoint::new(GLYPH_LEFT + tabs::TILE_WIDTH / 2.0, middle.y),
        );
        // The wedge is in the edge's own space, at its middle.
        let wedge = layer(WEDGE_ROOM, NSPoint::new(EDGE_ROOM / 2.0, EDGE_ROOM / 2.0));
        // SAFETY: the wedge is a plain layer of its own; the edge holds the one reference a mask
        // takes.
        unsafe { edge.setMask(Some(&wedge)) };
        // The tile, the badge and the hollow all have the badge's middle for theirs; the layers
        // are put there ([`Self::place`]) once the deck is known. The hollow lies in the notched
        // tile's own space, and the tile's middle is the badge's, so it stands at the middle.
        let plain = layer(2.0 * TILE_ROOM, NSPoint::ZERO);
        let notched = layer(2.0 * TILE_ROOM, NSPoint::ZERO);
        let hollow = layer(2.0 * HOLLOW_ROOM, NSPoint::new(TILE_ROOM, TILE_ROOM));
        // SAFETY: as the wedge: a plain layer of its own, held by the one it masks.
        unsafe { notched.setMask(Some(&hollow)) };
        let pop = layer(BADGE_SIDE, NSPoint::ZERO);
        // The circles and their signs in the badge's square, one over another; the sign over its
        // circle, and all but the one that shows clear.
        let in_pop = |side: f64| {
            let one = layer(side, NSPoint::new(BADGE_SIDE / 2.0, BADGE_SIDE / 2.0));
            one.setOpacity(0.0);
            pop.addSublayer(&one);
            one
        };
        let discs: [Retained<CALayer>; 4] = std::array::from_fn(|disc| in_pop(disc_side(disc)));
        let signs: [Retained<CALayer>; 4] = std::array::from_fn(|_| {
            let sign = in_pop(BADGE_SIDE);
            // Over every circle, whichever one is raised while it comes over the old (`morph`).
            sign.setZPosition(2.0);
            sign
        });
        for hidden in [&plain, &notched, &pop, &edge, &arc] {
            hidden.setHidden(true);
        }
        for clear in [&notched, &pop, &edge] {
            clear.setOpacity(0.0);
        }
        let this = Self::alloc(mtm).set_ivars(GlyphIvars {
            look: Cell::new(None),
            plain,
            notched,
            hollow,
            pop,
            discs,
            signs,
            shown: Cell::new(None),
            seen: Cell::new(false),
            placed: Cell::new(None),
            tile_ink: Cell::new(None),
            badge_ink: Cell::new(None),
            arc,
            edge,
            wedge,
            turning: Cell::new(None),
            arc_ink: Cell::new(None),
            edge_ink: Cell::new(None),
            palette: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        // Said through the chip, after its title.
        this.setAccessibilityElement(false);
        this.setWantsLayer(true);
        this.hang();
        this
    }

    /// Hangs the layers in the glyph's backing layer, if they are not there: AppKit makes that
    /// layer itself and may make it anew, and a ring whose arc hangs nowhere shows no arc. In
    /// the order they lie: the tile, the line round it, the badge.
    fn hang(&self) {
        let iv = self.ivars();
        let Some(layer) = self.layer() else {
            return;
        };
        for hung in [&iv.plain, &iv.notched, &iv.edge, &iv.pop, &iv.arc] {
            if hung.superlayer().is_none() {
                layer.addSublayer(hung);
            }
        }
    }

    /// Shows `look` (nothing, for `None`). `turn` is how what runs goes round
    /// (`tabs::ring_turn`) and is read only for a running glyph: `None` for one that stands still
    /// — Reduce Motion, a window nobody sees. `reduce`: Reduce Motion, where what comes and goes
    /// does it in one short fade.
    ///
    /// **A change of what the tab reports plays once, and only then** (`tabs::passage`): a glyph
    /// set again to what it shows is left alone, as at every layout.
    fn set(&self, look: Option<GlyphLook>, turn: Option<Turn>, palette: Palette, reduce: bool) {
        let iv = self.ivars();
        let before = iv.look.replace(look);
        let repainted = iv.palette.replace(Some(palette)) != Some(palette);
        if before != look || repainted {
            self.setNeedsDisplay(true);
        }
        self.setHidden(look.is_none());
        let spin = spin_of(look);
        let Some(look) = look.filter(|look| look.style.icons) else {
            // Nothing, or the small indicator of a tab without icons: it is drawn, its ring a
            // layer that turns, and it comes and goes at once.
            self.untile();
            // The layers' own properties move without Core Animation's 0.25 s of implicit fading.
            still(|| iv.arc.setHidden(spin != Some(Spin::Ring)));
            if spin.is_some() {
                self.hang();
                self.ink();
            }
            self.turn(turn.zip(spin));
            return;
        };
        still(|| {
            for shown in [&iv.plain, &iv.notched, &iv.pop] {
                shown.setHidden(false);
            }
            iv.arc.setHidden(true);
        });
        self.hang();
        self.place(look.emblem.edges);
        self.ink();
        let badge = Badge::of(Some(look));
        let moved = if iv.seen.replace(true) {
            before.map(|before| tabs::passage(before.indicator, look.indicator))
        } else {
            None
        };
        let Some(moved) = moved else {
            // Born with what it shows, or back from being hidden: at rest, nothing plays.
            self.rest(badge);
            still(|| {
                iv.wedge.setTransform(scaled(1.0));
                iv.edge.removeAllAnimations();
                iv.edge.setHidden(spin.is_none());
                iv.edge.setOpacity(if spin.is_some() { 1.0 } else { 0.0 });
            });
            self.turn(turn.zip(spin));
            return;
        };
        self.stir(badge, moved, reduce);
        // The line going out is seen going round until it is gone: where the turn stands now is
        // read before the turn is taken off.
        let frozen = (moved.line == Tide::Goes).then(|| self.wedge_angle());
        self.turn(turn.zip(spin));
        match moved.line {
            Tide::Comes => self.line_in(reduce),
            Tide::Goes => self.line_out(frozen.flatten(), reduce),
            Tide::Still => {}
        }
    }

    /// The glyph shows no tile now (hidden, or the small indicator of a tab without icons): the
    /// tile's layers are out of sight and what plays is over, and what comes next is laid out at
    /// rest.
    fn untile(&self) {
        let iv = self.ivars();
        if !iv.seen.replace(false) {
            return;
        }
        still(|| {
            for out in [&iv.plain, &iv.notched, &iv.pop, &iv.edge] {
                out.removeAllAnimations();
                out.setHidden(true);
            }
        });
        iv.shown.set(None);
    }

    /// Puts the tile's, the badge's and the hollow's layers on the badge's middle for a deck of
    /// `edges` cards behind the front tile; it leans with the deck.
    fn place(&self, edges: usize) {
        let iv = self.ivars();
        if iv.placed.replace(Some(edges)) == Some(edges) {
            return;
        }
        let at = badge_centre(edges);
        still(|| {
            for layer in [&iv.plain, &iv.notched, &iv.pop] {
                layer.setPosition(at);
            }
        });
    }

    /// The pictures of what is a layer, in the colours the bar has now and at the scale of the
    /// screen the glyph is on; drawn again only when one of them changed.
    fn ink(&self) {
        let iv = self.ivars();
        let (Some(look), Some(palette)) = (iv.look.get(), iv.palette.get()) else {
            return;
        };
        let scale = self
            .window()
            .map_or(2.0, |window| window.backingScaleFactor());
        if look.style.icons {
            let (emblem, style) = (look.emblem, look.style);
            let key = (emblem, style, palette, scale);
            if iv.tile_ink.replace(Some(key)) != Some(key) {
                let side = NSSize::new(2.0 * TILE_ROOM, 2.0 * TILE_ROOM);
                for tile in [&iv.plain, &iv.notched] {
                    paint_in(tile, side, scale, move |_| {
                        draw_tile_about(emblem, style, palette);
                    });
                }
            }
            let edges = emblem.edges;
            let key = (palette, scale, edges);
            if iv.badge_ink.replace(Some(key)) != Some(key) {
                let lean = tabs::deck_card(tabs::TILE_WIDTH, 0, edges).turn;
                for (disc, layer) in iv.discs.iter().enumerate() {
                    let color = palette.tone(tabs::list_dot(DISCS[disc]), 0);
                    let side = disc_side(disc);
                    paint(layer, side, scale, move || draw_disc(color, side));
                }
                let ink = Tint::of(palette.ground, 1.0);
                for (sign, layer) in iv.signs.iter().enumerate() {
                    paint(layer, BADGE_SIDE, scale, move || draw_sign(sign, lean, ink));
                }
                still(|| {
                    iv.hollow.setContentsScale(scale);
                    // SAFETY: what `layerContentsForContentsScale` returns is what `contents`
                    // takes.
                    unsafe { iv.hollow.setContents(Some(&hollow_picture(scale))) };
                });
            }
        }
        let Some(spin) = spin_of(Some(look)) else {
            return;
        };
        let color = palette.tone(tabs::list_dot(Indicator::Running), 0);
        match spin {
            Spin::Ring => {
                if iv.arc_ink.replace(Some((color, scale))) == Some((color, scale)) {
                    return;
                }
                paint(&iv.arc, tabs::INDICATOR_SIDE, scale, move || {
                    draw_arc(color);
                });
            }
            Spin::Edge(edges) => {
                let ink = Some((color, scale, edges));
                if iv.edge_ink.replace(ink) == ink {
                    return;
                }
                paint(&iv.edge, EDGE_ROOM, scale, move || draw_edge(color, edges));
                paint(&iv.wedge, WEDGE_ROOM, scale, draw_wedge);
            }
        }
    }

    /// Lays the badge out at rest, with none for `None`: the plain tile or the notched one, the
    /// badge whole with its one circle and sign showing, the hollow at its size. Whatever played
    /// is dropped.
    fn rest(&self, badge: Option<Badge>) {
        let iv = self.ivars();
        let level = |on: bool| if on { 1.0 } else { 0.0 };
        for layer in [&iv.plain, &iv.notched, &iv.pop, &iv.hollow] {
            layer.removeAllAnimations();
        }
        still(|| {
            iv.plain.setOpacity(level(badge.is_none()));
            iv.notched.setOpacity(level(badge.is_some()));
            iv.pop.setOpacity(level(badge.is_some()));
            iv.pop.setTransform(scaled(1.0));
            iv.hollow
                .setTransform(scaled(badge.map_or(1.0, Badge::hollow_share)));
            for (disc, layer) in iv.discs.iter().enumerate() {
                layer.removeAllAnimations();
                layer.setOpacity(level(badge.is_some_and(|badge| badge.disc() == disc)));
                layer.setTransform(scaled(1.0));
                layer.setZPosition(0.0);
            }
            for (sign, layer) in iv.signs.iter().enumerate() {
                layer.removeAllAnimations();
                layer.setOpacity(level(badge.is_some_and(|badge| badge.sign() == Some(sign))));
                layer.setTransform(scaled(1.0));
            }
        });
        iv.shown.set(badge.map(|badge| Worn { badge, gone: None }));
    }

    /// Plays what the tab's change of indicator does to the badge (`moved`, from
    /// `tabs::passage`): the badge comes, goes or becomes another. `to`: the badge it shows now.
    fn stir(&self, to: Option<Badge>, moved: tabs::Passage, reduce: bool) {
        let iv = self.ivars();
        // A badge on its way out is on the tile until it is gone.
        let on = iv
            .shown
            .get()
            .filter(|shown| shown.gone.is_none_or(|gone| Instant::now() < gone));
        match (moved.badge, to, on) {
            (Stir::Comes | Stir::Turns, Some(to), None) => self.arrive(to, reduce),
            (Stir::Comes | Stir::Turns, Some(to), Some(on)) => self.morph(on.badge, to, reduce),
            (Stir::Goes { wait }, _, Some(on)) if on.gone.is_none() => {
                self.depart(on.badge, wait, reduce);
            }
            // A badge that waits to go and a command begins: it goes now.
            (Stir::Still, _, Some(on)) if on.gone.is_some() && moved.line == Tide::Comes => {
                self.depart(on.badge, 0.0, reduce);
            }
            // The same indicator — a transfer's arrow turns round, or the badge on its way out
            // is wanted again.
            (Stir::Still, Some(to), Some(on)) if on.gone.is_some() || on.badge != to => {
                self.morph(on.badge, to, reduce);
            }
            _ => {}
        }
    }

    /// A badge comes where there was none: it grows from [`tabs::BADGE_FROM`] of its size past
    /// its own and settles, coming into view as it goes, the sign inside it growing too, and the
    /// ring cut round it grows with it. Reduce Motion: it fades in.
    fn arrive(&self, to: Badge, reduce: bool) {
        let iv = self.ivars();
        self.rest(Some(to));
        if reduce {
            play(
                &iv.pop,
                OPACITY,
                0.0,
                1.0,
                tabs::REDUCED_FADE,
                Ease::Out,
                0.0,
            );
            return;
        }
        let grow = [tabs::BADGE_FROM, tabs::BADGE_PEAK, 1.0];
        let at = [0.0, tabs::BADGE_PEAK_AT, 1.0];
        play_keys(&iv.pop, SCALE, &grow, &at, tabs::BADGE_COME, Ease::Out);
        play_keys(
            &iv.pop,
            OPACITY,
            &[0.0, 1.0, 1.0],
            &[0.0, tabs::BADGE_OPAQUE_AT, 1.0],
            tabs::BADGE_COME,
            Ease::Out,
        );
        let hollow = grow.map(|size| size * to.hollow_share());
        play_keys(&iv.hollow, SCALE, &hollow, &at, tabs::BADGE_COME, Ease::Out);
        if let Some(sign) = to.sign() {
            self.grow(sign);
        }
    }

    /// The sign comes into its circle: it grows from [`tabs::SIGN_FROM`] of its size.
    fn grow(&self, sign: usize) {
        let layer = &self.ivars().signs[sign];
        play(
            layer,
            SCALE,
            tabs::SIGN_FROM,
            1.0,
            tabs::SIGN_GROW,
            Ease::Out,
            0.0,
        );
        play(layer, OPACITY, 0.0, 1.0, tabs::SIGN_GROW, Ease::Out, 0.0);
    }

    /// The badge `was` goes after `wait` seconds (the tab was looked at: a tick or a dot rests
    /// first): it shrinks to [`tabs::BADGE_GO_TO`] of its size as it fades, the ring cut round it
    /// with it. Reduce Motion: it fades.
    ///
    /// The model is the plain tile at once, and the animation shows the rest of it: the notched
    /// tile is held up for as long as the badge is on the way out, and falls back to the model
    /// when it is over — nothing has to be undone afterwards, nothing wakes the app.
    fn depart(&self, was: Badge, wait: f64, reduce: bool) {
        let iv = self.ivars();
        let secs = if reduce {
            tabs::REDUCED_FADE
        } else {
            tabs::BADGE_GO
        };
        let (size, light) = (shown_scale(&iv.pop), shown_opacity(&iv.pop));
        let hollow = shown_scale(&iv.hollow);
        still(|| {
            iv.plain.setOpacity(1.0);
            iv.notched.setOpacity(0.0);
            iv.pop.setOpacity(0.0);
            if !reduce {
                iv.pop.setTransform(scaled(tabs::BADGE_GO_TO));
            }
        });
        play(&iv.pop, OPACITY, light, 0.0, secs, Ease::In, wait);
        if !reduce {
            play(
                &iv.pop,
                SCALE,
                size,
                tabs::BADGE_GO_TO,
                secs,
                Ease::In,
                wait,
            );
            let to = was.hollow_share() * tabs::BADGE_GO_TO;
            play(&iv.hollow, SCALE, hollow, to, secs, Ease::In, wait);
        }
        hold(&iv.plain, 0.0, wait + secs);
        hold(&iv.notched, 1.0, wait + secs);
        iv.shown.set(Some(Worn {
            badge: was,
            gone: Some(Instant::now() + Duration::from_secs_f64(wait + secs)),
        }));
    }

    /// The badge `was` becomes `to`, which may be the badge itself coming back from the way out.
    /// The circle stays: the colour passes (the new circle comes over the old, which stays whole
    /// beneath it, so there is no dip between), the size slides when the two differ (the failure's
    /// dot is smaller), the old sign fades and the new one grows. Reduce Motion: a fade.
    fn morph(&self, was: Badge, to: Badge, reduce: bool) {
        let iv = self.ivars();
        let leaving = iv.shown.get().is_some_and(|shown| shown.gone.is_some());
        let (size, light) = (shown_scale(&iv.pop), shown_opacity(&iv.pop));
        let hollow = shown_scale(&iv.hollow);
        let level = |on: bool| if on { 1.0 } else { 0.0 };
        if leaving {
            // Wanted again: what plays of the way out is dropped, and the badge goes back to
            // whole from where it stands.
            for layer in [&iv.plain, &iv.notched] {
                layer.removeAnimationForKey(ns_string!("hold"));
            }
            for layer in [&iv.pop, &iv.hollow] {
                layer.removeAnimationForKey(&NSString::from_str(OPACITY));
                layer.removeAnimationForKey(&NSString::from_str(SCALE));
            }
        }
        still(|| {
            iv.plain.setOpacity(0.0);
            iv.notched.setOpacity(1.0);
            iv.pop.setOpacity(1.0);
            iv.pop.setTransform(scaled(1.0));
            iv.hollow.setTransform(scaled(to.hollow_share()));
            for (disc, layer) in iv.discs.iter().enumerate() {
                layer.setOpacity(level(disc == to.disc()));
                layer.setZPosition(if disc == to.disc() { 1.0 } else { 0.0 });
            }
            for (sign, layer) in iv.signs.iter().enumerate() {
                layer.setOpacity(level(to.sign() == Some(sign)));
            }
        });
        if leaving {
            let back = if reduce { tabs::REDUCED_FADE } else { 0.15 };
            play(&iv.pop, OPACITY, light, 1.0, back, Ease::Out, 0.0);
            if !reduce {
                play(&iv.pop, SCALE, size, 1.0, back, Ease::Out, 0.0);
                play(
                    &iv.hollow,
                    SCALE,
                    hollow,
                    to.hollow_share(),
                    back,
                    Ease::Out,
                    0.0,
                );
            }
        }
        if was.disc() != to.disc() {
            let (old, new) = (&iv.discs[was.disc()], &iv.discs[to.disc()]);
            if reduce {
                play(old, OPACITY, 1.0, 0.0, tabs::REDUCED_FADE, Ease::Out, 0.0);
                play(new, OPACITY, 0.0, 1.0, tabs::REDUCED_FADE, Ease::Out, 0.0);
            } else {
                let (from, to_side) = (disc_side(was.disc()), disc_side(to.disc()));
                let secs = tabs::BADGE_TURN;
                hold(old, 1.0, secs);
                play(old, SCALE, 1.0, to_side / from, secs, Ease::Out, 0.0);
                play(new, OPACITY, 0.0, 1.0, secs, Ease::Out, 0.0);
                play(new, SCALE, from / to_side, 1.0, secs, Ease::Out, 0.0);
                if !leaving {
                    let (was, to) = (was.hollow_share(), to.hollow_share());
                    play(&iv.hollow, SCALE, was, to, secs, Ease::Out, 0.0);
                }
            }
        }
        if was.sign() != to.sign() {
            if let Some(old) = was.sign() {
                let secs = if reduce { tabs::REDUCED_FADE } else { SIGN_OUT };
                play(&iv.signs[old], OPACITY, 1.0, 0.0, secs, Ease::In, 0.0);
            }
            if let Some(new) = to.sign() {
                if reduce {
                    play(
                        &iv.signs[new],
                        OPACITY,
                        0.0,
                        1.0,
                        tabs::REDUCED_FADE,
                        Ease::Out,
                        0.0,
                    );
                } else {
                    self.grow(new);
                }
            }
        }
        iv.shown.set(Some(Worn {
            badge: to,
            gone: None,
        }));
    }

    /// The line round the tile comes: it fades in from where it stands.
    fn line_in(&self, reduce: bool) {
        let iv = self.ivars();
        let light = shown_opacity(&iv.edge);
        still(|| {
            iv.wedge.setTransform(scaled(1.0));
            iv.edge.setHidden(false);
            iv.edge.setOpacity(1.0);
        });
        let secs = if reduce {
            tabs::REDUCED_FADE
        } else {
            tabs::LINE_COME
        };
        play(&iv.edge, OPACITY, light, 1.0, secs, Ease::Out, 0.0);
    }

    /// The line round the tile goes: it fades out, standing where it stood (`angle`, if it went
    /// round) rather than snapping to the top.
    fn line_out(&self, angle: Option<f64>, reduce: bool) {
        let iv = self.ivars();
        let light = shown_opacity(&iv.edge);
        still(|| {
            if let Some(angle) = angle {
                iv.wedge.setTransform(rotated(angle));
            }
            iv.edge.setOpacity(0.0);
        });
        let secs = if reduce {
            tabs::REDUCED_FADE
        } else {
            tabs::LINE_GO
        };
        play(&iv.edge, OPACITY, light, 0.0, secs, Ease::In, 0.0);
    }

    /// How far round the line's turn stands now, on screen, if it goes round.
    fn wedge_angle(&self) -> Option<f64> {
        let iv = self.ivars();
        // SAFETY: reading the presentation copy of a layer we hold, on the main thread.
        let shown = unsafe { iv.wedge.presentationLayer() }?;
        // SAFETY: reading the animation a layer holds, on the main thread.
        unsafe { iv.wedge.animationForKey(turn_key()) }?;
        let turned = shown.transform();
        Some(turned.m12.atan2(turned.m11))
    }

    /// Plays the turn on what runs, or takes it off. The window server carries it round at an
    /// even pace with the app asleep; it is set only when the pace changes, nothing turned
    /// before or the animation is gone — set again at every layout it would jump back to its start.
    fn turn(&self, turn: Option<(Turn, Spin)>) {
        let iv = self.ivars();
        let Some((turn, spin)) = turn else {
            self.stop();
            return;
        };
        let layer = match spin {
            Spin::Ring => &iv.arc,
            Spin::Edge(_) => &iv.wedge,
        };
        // SAFETY: reading the animation a layer holds, on the main thread.
        let playing = unsafe { layer.animationForKey(turn_key()) }.is_some();
        if playing && iv.turning.get() == Some((turn.period, spin)) {
            return;
        }
        self.stop();
        let seconds = turn.period.as_secs_f64();
        match spin {
            Spin::Ring => {
                let from = turn.phase * std::f64::consts::TAU;
                let round = CABasicAnimation::animationWithKeyPath(Some(ns_string!(
                    "transform.rotation.z"
                )));
                // SAFETY: plain `NSNumber`s, what a rotation takes. In the glyph's flipped space
                // a positive angle goes clockwise on screen.
                unsafe {
                    round.setFromValue(Some(&NSNumber::numberWithDouble(from)));
                    round.setToValue(Some(&NSNumber::numberWithDouble(
                        from + std::f64::consts::TAU,
                    )));
                }
                round.setDuration(seconds);
                round.setRepeatCount(f32::INFINITY);
                let even = CAMediaTimingFunction::functionWithControlPoints(0.0, 0.0, 1.0, 1.0);
                round.setTimingFunction(Some(&even));
                layer.addAnimation_forKey(&round, Some(turn_key()));
            }
            Spin::Edge(edges) => {
                // Between two angles the wedge turns at an even rate; the angles are where the
                // edge's pace comes out even (`tabs::edge_angle`), turned with the edge when a deck
                // turns its front tile (counter-clockwise: back against the glyph's flipped space).
                // A turn already under way starts where the command's age puts it, by its time
                // offset.
                let lean = -tabs::deck_card(tabs::TILE_WIDTH, 0, edges)
                    .turn
                    .to_radians();
                let angles: Vec<Retained<AnyObject>> = (0..=EDGE_STEPS)
                    .map(|step| {
                        let share = step as f64 / EDGE_STEPS as f64;
                        let angle = lean
                            + tabs::edge_angle(
                                EDGE_WIDTH / 2.0,
                                EDGE_HEIGHT / 2.0,
                                EDGE_RADIUS,
                                share,
                            );
                        // `NSNumber` → `NSValue` → `NSObject` → the root: what `values` takes.
                        Retained::into_super(Retained::into_super(Retained::into_super(
                            NSNumber::numberWithDouble(angle),
                        )))
                    })
                    .collect();
                let round = CAKeyframeAnimation::animationWithKeyPath(Some(ns_string!(
                    "transform.rotation.z"
                )));
                let values = NSArray::from_retained_slice(&angles);
                // SAFETY: plain `NSNumber`s, what a rotation takes.
                unsafe { round.setValues(Some(&values)) };
                round.setDuration(seconds);
                round.setRepeatCount(f32::INFINITY);
                round.setTimeOffset(turn.phase * seconds);
                layer.addAnimation_forKey(&round, Some(turn_key()));
            }
        }
        iv.turning.set(Some((turn.period, spin)));
    }

    /// Takes the turn off whatever it plays on; both rest at the top (or where the line going
    /// out stands: [`Self::line_out`] sets that after).
    fn stop(&self) {
        let iv = self.ivars();
        if iv.turning.take().is_some() {
            iv.arc.removeAnimationForKey(turn_key());
            iv.wedge.removeAnimationForKey(turn_key());
        }
    }
}

// ─── What a glyph's layers play ───────────────────────────────────────────

/// The key paths the animations are played on, which are also the keys they are played under: a
/// layer's size and its opacity.
const SCALE: &str = "transform.scale";
const OPACITY: &str = "opacity";

/// How long a sign that gives way to another takes to fade.
const SIGN_OUT: f64 = 0.12;

/// The curves of the comings (out: quick, then slow to rest) and goings (in: slow, then quick):
/// the design's CSS `ease-out` and `ease-in`.
#[derive(Clone, Copy)]
enum Ease {
    Out,
    In,
}

fn curve(ease: Ease) -> Retained<CAMediaTimingFunction> {
    match ease {
        Ease::Out => CAMediaTimingFunction::functionWithControlPoints(0.0, 0.0, 0.58, 1.0),
        Ease::In => CAMediaTimingFunction::functionWithControlPoints(0.42, 0.0, 1.0, 1.0),
    }
}

/// Runs `change` without Core Animation's implicit 0.25 s of fading of whatever it sets.
fn still(change: impl FnOnce()) {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    change();
    CATransaction::commit();
}

/// `by` times the size, about the middle.
fn scaled(by: f64) -> CATransform3D {
    CATransform3D {
        m11: by,
        m12: 0.0,
        m13: 0.0,
        m14: 0.0,
        m21: 0.0,
        m22: by,
        m23: 0.0,
        m24: 0.0,
        m31: 0.0,
        m32: 0.0,
        m33: 1.0,
        m34: 0.0,
        m41: 0.0,
        m42: 0.0,
        m43: 0.0,
        m44: 1.0,
    }
}

/// Turned by `angle` radians about the middle, the way a layer's `transform.rotation.z` turns.
fn rotated(angle: f64) -> CATransform3D {
    CATransform3D {
        m11: angle.cos(),
        m12: angle.sin(),
        m21: -angle.sin(),
        m22: angle.cos(),
        ..scaled(1.0)
    }
}

/// How big `layer` shows now, and how opaque: where an animation under way stands, or the layer's
/// own value.
fn shown_scale(layer: &CALayer) -> f64 {
    // SAFETY: reading the presentation copy of a layer we hold, on the main thread.
    let shown = unsafe { layer.presentationLayer() };
    shown.map_or_else(|| layer.transform().m11, |shown| shown.transform().m11)
}

fn shown_opacity(layer: &CALayer) -> f64 {
    // SAFETY: as above.
    let shown = unsafe { layer.presentationLayer() };
    f64::from(shown.map_or_else(|| layer.opacity(), |shown| shown.opacity()))
}

/// Delays `animation` by `wait` seconds, `layer` showing its first value until then.
fn delayed(animation: &CAAnimation, layer: &CALayer, wait: f64) {
    if wait > 0.0 {
        animation.setBeginTime(layer.convertTime_fromLayer(CACurrentMediaTime(), None) + wait);
        // SAFETY: a constant `NSString` Core Animation exposes, only read.
        animation.setFillMode(unsafe { kCAFillModeBackwards });
    }
}

/// Plays `from` to `to` of `path` on `layer` over `secs`, after `wait`. The layer's model value
/// is already the end of it, and the animation removes itself on completion: nothing is left to
/// clean up.
fn play(layer: &CALayer, path: &str, from: f64, to: f64, secs: f64, ease: Ease, wait: f64) {
    let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str(path)));
    // SAFETY: plain `NSNumber`s, what these key paths take.
    unsafe {
        animation.setFromValue(Some(&NSNumber::numberWithDouble(from)));
        animation.setToValue(Some(&NSNumber::numberWithDouble(to)));
    }
    animation.setDuration(secs);
    animation.setTimingFunction(Some(&curve(ease)));
    delayed(&animation, layer, wait);
    layer.addAnimation_forKey(&animation, Some(&NSString::from_str(path)));
}

/// Plays `layer` from the look `from` (a matrix that [`fitted`] checked) to rest over `secs`.
/// The layer's own transform is already the rest, and the animation removes itself on completion.
fn play_look(layer: &CALayer, from: CATransform3D, secs: f64, ease: Ease) {
    let animation = CABasicAnimation::animationWithKeyPath(Some(ns_string!("transform")));
    // SAFETY: plain `NSValue`s around `CATransform3D` values, what this key path takes.
    unsafe {
        animation.setFromValue(Some(&NSValue::valueWithCATransform3D(from)));
        animation.setToValue(Some(&NSValue::valueWithCATransform3D(IDENTITY)));
    }
    animation.setDuration(secs);
    animation.setTimingFunction(Some(&curve(ease)));
    layer.addAnimation_forKey(&animation, Some(ns_string!("transform")));
}

/// Keeps `layer` showing `value` for `secs`, whatever its own opacity is: what the model has
/// returns when it is over. This is how a layer that goes is held up for the length of the going.
fn hold(layer: &CALayer, value: f64, secs: f64) {
    let animation = CABasicAnimation::animationWithKeyPath(Some(ns_string!("opacity")));
    // SAFETY: plain `NSNumber`s, what opacity takes.
    unsafe {
        animation.setFromValue(Some(&NSNumber::numberWithDouble(value)));
        animation.setToValue(Some(&NSNumber::numberWithDouble(value)));
    }
    animation.setDuration(secs);
    layer.addAnimation_forKey(&animation, Some(ns_string!("hold")));
}

/// [`play`] through several `values` at the shares `times` of `secs`, each leg on the same curve.
fn play_keys(layer: &CALayer, path: &str, values: &[f64], times: &[f64], secs: f64, ease: Ease) {
    let animation = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str(path)));
    let number = |value: &f64| NSNumber::numberWithDouble(*value);
    let values: Vec<Retained<AnyObject>> = values
        .iter()
        .map(|value| {
            Retained::into_super(Retained::into_super(Retained::into_super(number(value))))
        })
        .collect();
    let times: Vec<Retained<NSNumber>> = times.iter().map(number).collect();
    let curves: Vec<Retained<CAMediaTimingFunction>> =
        (1..values.len()).map(|_| curve(ease)).collect();
    // SAFETY: plain `NSNumber`s, what these key paths take.
    unsafe { animation.setValues(Some(&NSArray::from_retained_slice(&values))) };
    animation.setKeyTimes(Some(&NSArray::from_retained_slice(&times)));
    animation.setTimingFunctions(Some(&NSArray::from_retained_slice(&curves)));
    animation.setDuration(secs);
    layer.addAnimation_forKey(&animation, Some(&NSString::from_str(path)));
}

/// `layer`'s picture: what `draw` draws in a `side` square (a flipped one, like the glyph's own),
/// at `scale`, laid in without Core Animation's implicit fade.
fn paint(layer: &CALayer, side: f64, scale: f64, draw: impl Fn() + 'static) {
    paint_in(layer, NSSize::new(side, side), scale, move |_| draw());
}

/// [`paint`] for a picture of any size: `draw` is given the rect to fill (a flipped one).
fn paint_in(layer: &CALayer, size: NSSize, scale: f64, draw: impl Fn(NSRect) + 'static) {
    let block = RcBlock::new(move |rect: NSRect| -> Bool {
        draw(rect);
        Bool::YES
    });
    let image = NSImage::imageWithSize_flipped_drawingHandler(size, true, &block);
    let picture = image.layerContentsForContentsScale(scale);
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    layer.setContentsScale(scale);
    // SAFETY: what `NSImage::layerContentsForContentsScale` returns is what `contents` takes.
    unsafe { layer.setContents(Some(&picture)) };
    CATransaction::commit();
}

// ─── A chip's top and bottom lines ───────────────────────────────────────

/// What a chip's lines show ([`ChipLines`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Lines {
    /// A marked host's colour along the top; `None` for an unmarked host.
    top: Option<u32>,
    /// The same colour as a vertical stripe on the left edge, which is where a side row wears it.
    left: Option<u32>,
}

/// Where a 2 pt line runs along a `width` chip: inside its rounded corners.
fn line_span(width: f64) -> (f64, f64) {
    (TAB_RADIUS, (width - 2.0 * TAB_RADIUS).max(0.0))
}

pub(crate) struct LinesIvars {
    lines: Cell<Lines>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; ChipLines implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabLines"]
    #[ivars = LinesIvars]
    pub(crate) struct ChipLines;

    unsafe impl NSObjectProtocol for ChipLines {}

    impl ChipLines {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        /// The host's line along the top, or its stripe down the left edge in a side row — above
        /// the chip's faces, so a light theme's white face does not cover it.
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let Lines { top, left } = self.ivars().lines.get();
            let (x, width) = line_span(self.bounds().size.width);
            if let Some(rgb) = top {
                rounded(
                    NSRect::new(NSPoint::new(x, 0.0), NSSize::new(width, LINE)),
                    LINE / 2.0,
                    Tint::of(rgb, 1.0),
                    None,
                );
            }
            if let Some(rgb) = left {
                let stripe = tabs::Row::of(self.bounds().size.width, None, 0.0, false).host_mark;
                rounded(
                    NSRect::new(
                        NSPoint::new(stripe.x, stripe.y),
                        NSSize::new(stripe.width, stripe.height),
                    ),
                    stripe.width / 2.0,
                    Tint::of(rgb, 1.0),
                    None,
                );
            }
        }
    }
);

impl ChipLines {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(LinesIvars {
            lines: Cell::new(Lines::default()),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }

    fn set(&self, lines: Lines) {
        if self.ivars().lines.replace(lines) != lines {
            self.setNeedsDisplay(true);
        }
    }
}

// ─── A chip's fill while a transfer flows ────────────────────────────────

/// The accent at the fill's tail and at its leading edge: 7 % and 14 %. On the bar's ladder of
/// grounds these are the hover's tone (the foreground at 5 %) and the selected face's (10 %) in
/// both themes, so a tab in the background is never brighter than the selected one and the
/// leading edge is the darkest place, which tells the way it goes. The way still to go is the
/// hover's tone itself ([`Palette::hover`]).
const FILL_TAIL: f64 = 0.07;
const FILL_HEAD: f64 = 0.14;

/// The width of the pictures the fill's layers hold: a gradient stretched over the layer is
/// smooth at any width, and one is painted only when its colours change.
const FILL_PICTURE: f64 = 64.0;

/// What a chip's fill is doing ([`ChipFill`]).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Filling {
    /// Nothing shows.
    Idle,
    /// A transfer flows; the fill stands at this share of the chip's width.
    Flowing(f64),
    /// The transfer ended and the fill plays its end ([`Ending`]): it is the tab's ground until
    /// `lit` and gone at `gone`. `failed`: it has turned to the error colour.
    Ending {
        failed: bool,
        lit: Instant,
        gone: Instant,
    },
}

pub(crate) struct FillIvars {
    state: Cell<Filling>,
    /// Holds the two layers; its opacity carries the end's rest and fade.
    holder: Retained<CALayer>,
    /// The way still to go, right of the fill, up to the chip's right edge.
    track: Retained<CALayer>,
    /// The filled part, from the chip's left edge: the accent from [`FILL_TAIL`] to [`FILL_HEAD`].
    fill: Retained<CALayer>,
    /// The size the layers were last placed for.
    size: Cell<NSSize>,
    /// What the two pictures were last painted in: the fill's two ends, the track's tone and
    /// the screen's scale.
    inked: Cell<Option<(Tint, Tint, Tint, f64)>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; ChipFill implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabFill"]
    #[ivars = FillIvars]
    pub(crate) struct ChipFill;

    unsafe impl NSObjectProtocol for ChipFill {}

    impl ChipFill {
        /// Takes no click: the chip does.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

/// A chip's fill: a tab's own ground filling from the left as its transfer goes, and playing
/// out the end ([`Ending`]) when it is over. It lies above the chip's faces and below its
/// glyph and title.
///
/// **It moves in Core Animation only, and only when something changed.** A report moves the two
/// layers' widths at an even pace in the time to the next ([`tabs::FILL_FLOW`]); the end is one
/// animation of the holder's opacity, rest and fade in one, set up when the end is told. The
/// app draws no frame, and a layout that asks for what the fill already shows leaves it to
/// arrive ([`Self::follow`]). The pictures the layers hold are painted when their colours
/// change — layers carry no colour of their own (the module's note on drawing).
impl ChipFill {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let holder = CALayer::new();
        let track = CALayer::new();
        let fill = CALayer::new();
        // The layers hold their widths from an edge each: the fill from the left, the way still
        // to go from the right, both about the middle of the chip's height.
        holder.setAnchorPoint(NSPoint::ZERO);
        holder.setPosition(NSPoint::ZERO);
        fill.setAnchorPoint(NSPoint::new(0.0, 0.5));
        track.setAnchorPoint(NSPoint::new(1.0, 0.5));
        holder.addSublayer(&track);
        holder.addSublayer(&fill);
        let this = Self::alloc(mtm).set_ivars(FillIvars {
            state: Cell::new(Filling::Idle),
            holder,
            track,
            fill,
            size: Cell::new(NSSize::ZERO),
            inked: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setAccessibilityElement(false);
        this.setWantsLayer(true);
        this.setHidden(true);
        this.hang();
        this
    }

    /// Hangs the holder in the view's backing layer, rounded to the chip's corner so the fill
    /// takes the chip's shape, if it is not there: AppKit makes that layer itself and may make
    /// it anew (as for the [`Glyph`]'s).
    fn hang(&self) {
        let Some(layer) = self.layer() else {
            return;
        };
        layer.setCornerRadius(TAB_RADIUS);
        layer.setMasksToBounds(true);
        let holder = &self.ivars().holder;
        if holder.superlayer().is_none() {
            layer.addSublayer(holder);
        }
    }

    /// What the fill is doing at `now`: an end that is over is nothing.
    fn state_at(&self, now: Instant) -> Filling {
        match self.ivars().state.get() {
            Filling::Ending { gone, .. } if now >= gone => Filling::Idle,
            state => state,
        }
    }

    /// Whether the fill is the tab's ground now: a transfer flows, or its end rests. A fading
    /// fill is not — the tab's title and its separators are the ordinary ones again.
    fn lit(&self, now: Instant) -> bool {
        match self.state_at(now) {
            Filling::Idle => false,
            Filling::Flowing(_) => true,
            Filling::Ending { lit, .. } => now < lit,
        }
    }

    fn flowing(&self) -> bool {
        matches!(self.ivars().state.get(), Filling::Flowing(_))
    }

    /// The fill is gone at once (the chip took its one-tab form).
    fn reset(&self) {
        let iv = self.ivars();
        iv.holder.removeAllAnimations();
        iv.state.set(Filling::Idle);
        self.setHidden(true);
    }

    /// Lays the layers out for a chip of `size` with the fill at `share`, at once.
    fn place(&self, size: NSSize, share: f64) {
        let iv = self.ivars();
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let middle = size.height / 2.0;
        iv.holder.setBounds(NSRect::new(NSPoint::ZERO, size));
        iv.fill.setPosition(NSPoint::new(0.0, middle));
        iv.track.setPosition(NSPoint::new(size.width, middle));
        CATransaction::commit();
        iv.size.set(size);
        self.run(share, 0.0);
    }

    /// Moves the boundary between the fill and the way still to go to `share`, over `secs` at an
    /// even pace (at once for zero).
    fn run(&self, share: f64, secs: f64) {
        let iv = self.ivars();
        let size = iv.size.get();
        CATransaction::begin();
        if secs > 0.0 {
            CATransaction::setAnimationDuration(secs);
            let even = CAMediaTimingFunction::functionWithControlPoints(0.0, 0.0, 1.0, 1.0);
            CATransaction::setAnimationTimingFunction(Some(&even));
        } else {
            CATransaction::setDisableActions(true);
        }
        let part =
            |share: f64| NSRect::new(NSPoint::ZERO, NSSize::new(size.width * share, size.height));
        iv.fill.setBounds(part(share));
        iv.track.setBounds(part(1.0 - share));
        CATransaction::commit();
    }

    /// The pictures of the fill and of the way still to go, in the palette's colours
    /// (`failed`: the error's) and at the scale of the screen the chip is on; painted again only
    /// when one of them changed.
    fn ink(&self, palette: Palette, failed: bool) {
        let iv = self.ivars();
        let scale = self
            .window()
            .map_or(2.0, |window| window.backingScaleFactor());
        let (tail, head) = palette.fill_tints(failed);
        let key = (tail, head, palette.hover, scale);
        if iv.inked.replace(Some(key)) == Some(key) {
            return;
        }
        let gradient = |from: Tint, to: Tint| {
            move |rect: NSRect| {
                let gradient = NSGradient::initWithStartingColor_endingColor(
                    NSGradient::alloc(),
                    &from.color(),
                    &to.color(),
                );
                if let Some(gradient) = gradient {
                    gradient.drawInRect_angle(rect, 0.0);
                }
            }
        };
        let picture = NSSize::new(FILL_PICTURE, 1.0);
        paint_in(&iv.fill, picture, scale, gradient(tail, head));
        paint_in(
            &iv.track,
            picture,
            scale,
            gradient(palette.hover, palette.hover),
        );
    }

    /// Shows the transfer at `share` (`None`: none flows) in a chip of `size`, lit by `palette`.
    /// A fill already at `share` is left alone, one that moved follows at an even pace; one that
    /// begins starts there at once, from a clean slate. A transfer that stops flowing without
    /// its end having been told ([`Self::end`]) is a stop. `selected`: the chip's own face is the
    /// ground, so there is no way still to go to show.
    fn follow(
        &self,
        size: NSSize,
        share: Option<f64>,
        palette: Palette,
        selected: bool,
        reduce: bool,
    ) {
        let iv = self.ivars();
        let now = Instant::now();
        let share = share.map(|share| share.clamp(0.0, 1.0));
        let flow = if reduce { 0.0 } else { tabs::FILL_FLOW };
        if share.is_none() && self.state_at(now) == Filling::Idle {
            // Nothing to show, and the common case: every chip, at every layout.
            iv.state.set(Filling::Idle);
            self.setHidden(true);
            return;
        }
        self.hang();
        match (share, self.state_at(now)) {
            (Some(share), Filling::Flowing(shown)) => {
                if iv.size.get() != size {
                    self.place(size, shown);
                }
                if share != shown {
                    self.run(share, flow);
                    iv.state.set(Filling::Flowing(share));
                }
            }
            (Some(share), _) => {
                iv.holder.removeAllAnimations();
                CATransaction::begin();
                CATransaction::setDisableActions(true);
                iv.holder.setOpacity(1.0);
                CATransaction::commit();
                self.place(size, share);
                iv.state.set(Filling::Flowing(share));
            }
            (None, Filling::Flowing(_)) => {
                self.end(Finish::Stopped, reduce, palette);
            }
            (None, _) => {}
        }
        let state = self.state_at(now);
        match state {
            Filling::Flowing(_) => self.ink(palette, false),
            Filling::Ending { failed, .. } => self.ink(palette, failed),
            Filling::Idle => {}
        }
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        iv.track.setHidden(selected);
        CATransaction::commit();
        self.setHidden(state == Filling::Idle);
    }

    /// Plays the end of the transfer `finish` says ([`Ending`]); the seconds the fill stays the
    /// tab's ground (the caller lays the bar out again then). Nothing flows in the fill: none.
    fn end(&self, finish: Finish, reduce: bool, palette: Palette) -> f64 {
        let iv = self.ivars();
        if !self.flowing() {
            return 0.0;
        }
        let ending = Ending::of(finish, reduce);
        let now = Instant::now();
        let after = |secs: f64| now + Duration::from_secs_f64(secs);
        iv.state.set(Filling::Ending {
            failed: ending.failed,
            lit: after(ending.lit()),
            gone: after(ending.total()),
        });
        self.ink(palette, ending.failed);
        if ending.fills_up {
            self.run(1.0, ending.run);
        }
        // The model goes to nothing at once and the animation shows rest and fade on the way:
        // when it is over there is nothing left to snap back to.
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        iv.holder.setOpacity(0.0);
        CATransaction::commit();
        let number = |value: f64| NSNumber::numberWithDouble(value);
        let total = ending.total();
        let values: Vec<Retained<AnyObject>> = [1.0, 1.0, 0.0]
            .map(|value| {
                Retained::into_super(Retained::into_super(Retained::into_super(number(value))))
            })
            .into();
        let times = [
            0.0,
            if total > 0.0 {
                ending.lit() / total
            } else {
                0.0
            },
            1.0,
        ]
        .map(number);
        let fade = CAKeyframeAnimation::animationWithKeyPath(Some(ns_string!("opacity")));
        // SAFETY: plain `NSNumber`s, what opacity takes.
        unsafe { fade.setValues(Some(&NSArray::from_retained_slice(&values))) };
        fade.setKeyTimes(Some(&NSArray::from_retained_slice(&times)));
        fade.setDuration(total);
        iv.holder
            .addAnimation_forKey(&fade, Some(ns_string!("end")));
        ending.lit()
    }
}

// ─── A chip: one tab ──────────────────────────────────────────────────────

pub(crate) struct ChipIvars {
    /// The tab's id (`TerminalTab::id`): the bar keeps a tab's chip across
    /// changes of the list, so a reflow can slide it.
    tab: Cell<u64>,
    selected: Cell<bool>,
    hovered: Cell<bool>,
    /// Picked together with other tabs in the side column ([`TabBar::pick`]).
    picked: Cell<bool>,
    /// The one-tab form: a centred title, no fill, no `×`, no click.
    single: Cell<bool>,
    /// Born in this layout: it fades in rather than sliding from nowhere.
    fresh: Cell<bool>,
    /// The frame the bar last gave it ([`Chip::set`]).
    target: Cell<NSRect>,
    palette: Cell<Option<Palette>>,
    /// The hover fill, below everything: its own view, so it fades.
    hover: Retained<ChipFace>,
    /// The picked fill, above the hover and below the selected one.
    pick: Retained<ChipFace>,
    /// The selected fill, below the label: its own view so the light
    /// theme's shadow falls from the face alone, not from the text.
    face: Retained<ChipFace>,
    /// The fill of a chip lifted by the pointer, in place of the selected one.
    lift: Retained<ChipFace>,
    /// Its tab is carried away: only a faint place is left ([`GHOST_ALPHA`]).
    ghost: Cell<bool>,
    /// The fill of the chip's own ground while a transfer flows, above the faces.
    fill: Retained<ChipFill>,
    /// The tab's ground is its transfer's fill now (`Look::glowing`): the title is in full ink.
    glowing: Cell<bool>,
    /// The host's line along the top.
    lines: Retained<ChipLines>,
    /// The indicator, left of the title.
    glyph: Retained<Glyph>,
    title: tab_title::Title,
    /// The ⌘ key that reaches the tab, right of the title while ⌘ is held.
    hint: Retained<NSTextField>,
    close: Retained<CloseButton>,
    /// The line that fills under the chip while a carried pane waits on it to
    /// open the tab ([`Chip::dwell`]).
    dwell: RefCell<Option<Retained<NSBox>>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; Chip implements no `Drop`
    // and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabChip"]
    #[ivars = ChipIvars]
    pub(crate) struct Chip;

    unsafe impl NSObjectProtocol for Chip {}

    impl Chip {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The chip takes every press inside it — its label never does —
        /// except on a shown `×`. The single tab's title takes none: the row
        /// under it moves the window.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            // No `return`: `define_class!` converts the body's last expression.
            self.hit(point)
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }

        /// A press selects — on the press, like the system's tabs — and
        /// puts the summary card away. The second press of a double click
        /// names the tab; any other may become a drag. With ⇧ or ⌘ held, in the side
        /// column, it picks the row together with others instead and selects nothing
        /// ([`TabBar::pick`]); a plain press on a row that is picked waits for the button to
        /// come up — what it selects would let the picks go before they could be carried
        /// ([`TabBar::hold_picked`]).
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if let Some(bar) = self.bar() {
                let tab = self.ivars().tab.get();
                bar.pressed(tab);
                let chord = pick_click(event.modifierFlags());
                if chord.is_some_and(|click| bar.pick(tab, click)) {
                    return;
                }
                if chord.is_none() && event.clickCount() < 2 && bar.hold_picked(tab, event) {
                    return;
                }
            }
            let selected = self.select_tab();
            let Some(bar) = self.bar() else {
                return;
            };
            if event.clickCount() == 2 {
                bar.begin_rename(self.ivars().tab.get());
            } else if selected {
                bar.grip(self.ivars().tab.get(), event);
            }
        }

        /// The pointer moves with the button down: the tab follows it along the
        /// strip, or comes away from the bar ([`TabBar::drag`]).
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            if let Some(bar) = self.bar() {
                bar.drag(self.ivars().tab.get(), event);
            }
        }

        /// The button is let go: a tab moved along the strip settles in its place.
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            if let Some(bar) = self.bar() {
                bar.release(self.ivars().tab.get(), event);
            }
        }

        /// The chip's own menu, on a right click or a control click: what
        /// can be done to this tab, selected or not.
        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, _event: &NSEvent) -> Option<Retained<NSMenu>> {
            // No `?`: `define_class!` converts the body's last expression.
            self.bar()
                .and_then(|bar| bar.context_menu(self.ivars().tab.get()))
        }

        /// The middle button closes the tab.
        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, event: &NSEvent) {
            if event.buttonNumber() == 2 {
                self.close_tab();
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            if let Some(bar) = self.bar() {
                bar.hover(Some(self.ivars().tab.get()));
            }
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            if let Some(bar) = self.bar() {
                bar.unhover(self.ivars().tab.get());
            }
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_press(&self) -> bool {
            let _ = self.select_tab();
            true
        }
    }
);

impl CloseButton {
    /// `hitTest:`'s answer for a point in the chip's space (the `×`'s
    /// superview): itself while shown and under the point.
    fn hit_test_local(&self, point: NSPoint) -> Option<Retained<NSView>> {
        let inside = self.ivars().shown.get() && contains(self.frame(), point);
        inside.then(|| Retained::into_super(self.retain()))
    }
}

impl Chip {
    fn new(mtm: MainThreadMarker, tab: u64) -> Retained<Self> {
        let hover = ChipFace::new(mtm, Face::Hovered);
        let pick = ChipFace::new(mtm, Face::Picked);
        let face = ChipFace::new(mtm, Face::Selected);
        let lift = ChipFace::new(mtm, Face::Lifted);
        let lines = ChipLines::new(mtm);
        let fill_view = ChipFill::new(mtm);
        let glyph = Glyph::new(mtm);
        let title = tab_title::Title::new(mtm, NSTextAlignment::Center);
        let hint = label(mtm);
        let close = CloseButton::new(mtm);
        let this = Self::alloc(mtm).set_ivars(ChipIvars {
            tab: Cell::new(tab),
            selected: Cell::new(false),
            hovered: Cell::new(false),
            picked: Cell::new(false),
            single: Cell::new(false),
            fresh: Cell::new(true),
            target: Cell::new(NSRect::ZERO),
            palette: Cell::new(None),
            hover: hover.clone(),
            pick: pick.clone(),
            face: face.clone(),
            lift: lift.clone(),
            ghost: Cell::new(false),
            fill: fill_view.clone(),
            glowing: Cell::new(false),
            lines: lines.clone(),
            glyph: glyph.clone(),
            title,
            hint: hint.clone(),
            close: close.clone(),
            dwell: RefCell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        hover.setAlphaValue(0.0);
        pick.setHidden(true);
        lift.setHidden(true);
        let resizing = NSAutoresizingMaskOptions::ViewWidthSizable
            | NSAutoresizingMaskOptions::ViewHeightSizable;
        for view in [
            &**hover as &NSView,
            &**pick,
            &**face,
            &**lift,
            &**fill_view,
            &**lines,
        ] {
            view.setAutoresizingMask(resizing);
        }
        this.addSubview(&hover);
        this.addSubview(&pick);
        this.addSubview(&face);
        this.addSubview(&lift);
        this.addSubview(&fill_view);
        this.addSubview(&lines);
        this.addSubview(&glyph);
        this.addSubview(this.ivars().title.view());
        this.addSubview(&hint);
        this.addSubview(&close);
        hint.setHidden(true);
        hint.setAccessibilityElement(false);
        this.setAccessibilityElement(true);
        // SAFETY: AppKit's constant role strings, alive for the process.
        unsafe {
            this.setAccessibilityRole(Some(NSAccessibilityRadioButtonRole));
            this.setAccessibilitySubrole(Some(NSAccessibilityTabButtonSubrole));
        }
        // The label is read through the chip; its own element would say the
        // title twice.
        this.ivars().title.field().setAccessibilityElement(false);
        track_hover(&this);
        this
    }

    fn tab(&self) -> u64 {
        self.ivars().tab.get()
    }

    /// The tab's transfer ended as `finish` says: the fill plays its end. The seconds it stays
    /// the tab's ground, the bar's cue to lay out again (`0` when it plays none, or has none).
    fn fill_end(&self, finish: Finish, reduce: bool) -> f64 {
        self.ivars().palette.get().map_or(0.0, |palette| {
            self.ivars().fill.end(finish, reduce, palette)
        })
    }

    /// Whether the chip's ground is its transfer's fill at `now`.
    fn fill_lit(&self, now: Instant) -> bool {
        self.ivars().fill.lit(now)
    }

    /// The line under the chip fills over `secs` (`Some(0.0)`: it stands full at
    /// once), or is taken away (`None`): a carried pane has waited on the chip,
    /// and the tab opens when the line is full.
    fn dwell(&self, secs: Option<f64>) {
        let iv = self.ivars();
        if let Some(line) = iv.dwell.take() {
            line.removeFromSuperview();
        }
        let (Some(secs), Some(palette)) = (secs, iv.palette.get()) else {
            return;
        };
        let size = self.bounds().size;
        let (x, width) = line_span(size.width);
        let y = size.height - LINE;
        let full = NSRect::new(NSPoint::new(x, y), NSSize::new(width, LINE));
        let line = plain_box(self.mtm(), Tint::of(palette.accent, 1.0), LINE / 2.0);
        line.setFrame(if secs > 0.0 {
            NSRect::new(NSPoint::new(x, y), NSSize::new(0.0, LINE))
        } else {
            full
        });
        self.addSubview(&line);
        iv.dwell.replace(Some(line.clone()));
        if secs > 0.0 {
            animate_linear(secs, move || line.animator().setFrame(full));
        }
    }

    /// The chip glows once and fades: a pane has joined its tab, or become a
    /// tab beside it, and the user stayed where they were.
    fn pulse(&self) {
        let Some(palette) = self.ivars().palette.get() else {
            return;
        };
        let glow = plain_box(self.mtm(), Tint::of(palette.accent, 0.34), TAB_RADIUS);
        glow.setFrame(self.bounds());
        glow.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        self.addSubview(&glow);
        let fading = glow.clone();
        crate::card::animate(
            PULSE_SECS,
            move || fading.animator().setAlphaValue(0.0),
            move || glow.removeFromSuperview(),
        );
    }

    /// The bar the chip is in: its strip's superview.
    fn bar(&self) -> Option<Retained<TabBar>> {
        // SAFETY: reading the superviews; we are on the main thread.
        unsafe { self.superview()?.superview() }?
            .downcast::<TabBar>()
            .ok()
    }

    /// `hitTest:`'s body: `point` is in the strip's space.
    fn hit(&self, point: NSPoint) -> Option<Retained<NSView>> {
        let iv = self.ivars();
        if iv.single.get() || self.isHidden() || !contains(self.frame(), point) {
            return None;
        }
        // SAFETY: reading the superview; we are on the main thread.
        let local = self.convertPoint_fromView(point, unsafe { self.superview() }.as_deref());
        iv.close
            .hit_test_local(local)
            .or_else(|| Some(Retained::into_super(self.retain())))
    }

    /// Moves the chip to `along` in the strip's space — its x in the bar, its y (`upright`) in the
    /// column — and nothing else: the held chip following the pointer while the others stay where
    /// the last layout put them. The frame is recorded as the one asked for, so the next layout
    /// moves it from here.
    fn follow(&self, along: f64, upright: bool) {
        let mut frame = self.frame();
        if upright {
            frame.origin.y = along;
        } else {
            frame.origin.x = along;
        }
        self.ivars().target.set(frame);
        self.setFrame(frame);
    }

    /// Selects the chip's tab; `false` if the window would not (a question of its own is up).
    fn select_tab(&self) -> bool {
        self.bar()
            .is_some_and(|bar| bar.select(self.ivars().tab.get()))
    }

    fn close_tab(&self) {
        if let Some(bar) = self.bar() {
            bar.close(self.ivars().tab.get());
        }
    }

    /// Places the chip at `frame` (the bar's space) and gives it its tab's
    /// look. `slide` is the reflow's duration when the applier changed the
    /// list: an old chip slides to its place, a new one fades in; `None`
    /// puts it there at once.
    fn set(
        &self,
        frame: NSRect,
        look: &Look<'_>,
        palette: Palette,
        motion: Motion,
        slide: Option<f64>,
    ) {
        let Look {
            title,
            emblem,
            indicator,
            turn,
            down,
            style,
            lines,
            fill,
            glowing,
            reduce,
            hint,
            spoken,
            selected,
            hovered,
            picked,
            single,
            lifted,
            ghost,
            side,
        } = look;
        let (style, side) = (*style, *side);
        let (selected, hovered, single) = (*selected, *hovered, *single);
        // Picked is a side row's: the one-tab form has no face to tint.
        let picked = *picked && !single;
        let (lifted, ghost) = (*lifted && !single, *ghost && !single);
        let glowing = *glowing && !single;
        // The one-tab form is today's title bar: no indicator, no lines, no
        // hint — the icon stays, it says what the window runs.
        let indicator = indicator.filter(|_| !single);
        let lines = if single { Lines::default() } else { *lines };
        let hint = hint.as_deref().filter(|_| !single);
        let iv = self.ivars();
        let changed = iv.selected.replace(selected) != selected
            || iv.hovered.replace(hovered) != hovered
            || iv.picked.replace(picked) != picked
            || iv.single.replace(single) != single
            || iv.glowing.replace(glowing) != glowing
            || iv.palette.replace(Some(palette)) != Some(palette);
        let fresh = iv.fresh.replace(false);
        // The frame last asked for: a slide in flight is heading there, and
        // a layout that asks for the same place (a clock tick, a hover, a
        // hint) leaves it to arrive rather than snapping it.
        let moved = iv.target.replace(frame) != frame;
        // Whether the chip was put at `frame` at once in this call: only then
        // are the faces and lines sized here. Otherwise they fill the chip
        // by their autoresizing — a sliding chip carries them, and sizing
        // them to the end of a slide in flight would add its rest twice.
        let placed = match slide {
            Some(secs) if fresh => {
                self.setFrame(frame);
                self.setAlphaValue(0.0);
                fade(self, 1.0, secs);
                true
            }
            Some(secs) if moved => {
                let this = self.retain();
                animate(secs, move || this.animator().setFrame(frame));
                false
            }
            Some(_) => false,
            None if moved => {
                // A slide still on its way (the chip was brought into view a
                // moment ago) would carry on to its old place and snap back.
                if let Some(layer) = self.layer() {
                    layer.removeAllAnimations();
                }
                self.setFrame(frame);
                true
            }
            None => false,
        };
        let size = frame.size;
        if placed {
            let whole = NSRect::new(NSPoint::ZERO, size);
            iv.hover.setFrame(whole);
            iv.pick.setFrame(whole);
            iv.face.setFrame(whole);
            iv.lift.setFrame(whole);
            iv.fill.setFrame(whole);
            iv.lines.setFrame(whole);
        }
        iv.hover.set(palette);
        fade(
            &iv.hover,
            f64::from(u8::from(hovered && !selected && !single)),
            motion.hover,
        );
        iv.pick.set(palette);
        // The tab on screen keeps its own face: picked or not, it is the one the others pick from.
        iv.pick.setHidden(!picked || selected || lifted);
        iv.face.set(palette);
        iv.face.setHidden(single || !selected || lifted);
        iv.lift.set(palette);
        iv.lift.setHidden(!lifted);
        if iv.ghost.replace(ghost) != ghost {
            self.setAlphaValue(if ghost { GHOST_ALPHA } else { 1.0 });
        }
        iv.lines.set(lines);
        if single {
            iv.fill.reset();
        } else {
            iv.fill.follow(size, *fill, palette, selected, *reduce);
        }
        let text = iv.title.field();
        let (size_pt, weight, color, pad) = if single {
            // SAFETY: AppKit's constant font weight.
            (
                SINGLE_TEXT,
                unsafe { NSFontWeightSemibold },
                palette.dim,
                SINGLE_PAD,
            )
        } else if selected {
            // SAFETY: as above.
            (
                CHIP_TEXT,
                unsafe { NSFontWeightSemibold },
                palette.title,
                CHIP_PAD,
            )
        } else {
            let color = if hovered || glowing || picked {
                palette.title
            } else {
                palette.dim
            };
            // SAFETY: as above.
            (CHIP_TEXT, unsafe { NSFontWeightMedium }, color, CHIP_PAD)
        };
        if changed || text.stringValue().to_string() != *title {
            text.setFont(Some(&NSFont::systemFontOfSize_weight(size_pt, weight)));
            text.setTextColor(Some(&Tint::of(color, 1.0).color()));
            text.setStringValue(&NSString::from_str(title));
        }
        // The frame the whole title draws in, not `intrinsicContentSize`:
        // that is the alignment rect, 2 pt in from each side, and a frame
        // that narrow truncates the text it was measured for to "…".
        let natural = text.fittingSize();
        let inner = (size.width - 2.0 * pad).max(0.0);
        // Without icons the small indicator takes the place, and only while the tab reports
        // something; with them the icon is always there (a deck widens it).
        let bare = !style.icons;
        let lead = if bare && side {
            // A row keeps the indicator's room whether it shows or not, so the title stays put
            // as the indicator comes and goes.
            Some(tabs::INDICATOR_SIDE)
        } else if bare {
            indicator.map(|_| tabs::INDICATOR_SIDE)
        } else {
            emblem.map(|emblem| tabs::lead_width(tabs::TILE_WIDTH, emblem.edges))
        };
        // Where the parts stand: a chip's in the strip's arithmetic (`×` at the left, a title
        // centred in what the padding leaves), a side row's in `tabs::Row`'s (`×` and the ⌘ key at
        // the right end, the title after the icon).
        let row = side.then(|| {
            // The title gives way to the `×` while it shows and to the key while that does.
            tabs::Row::of(size.width, lead, natural.width, hovered || hint.is_some())
        });
        let (glyph_x, text_x, text_width) = match &row {
            Some(row) => (row.lead.map(|lead| lead.x), row.title.x, row.title.width),
            None => tabs::title_row(inner, natural.width, lead),
        };
        let pad = if row.is_some() { 0.0 } else { pad };
        iv.title.place(
            NSRect::new(
                NSPoint::new(
                    pad + text_x,
                    ((size.height - natural.height) / 2.0).max(0.0),
                ),
                NSSize::new(text_width, natural.height),
            ),
            natural.width,
        );
        // The fade hides the end of a title that does not fit, and nothing moves to show it: the
        // tooltip says the whole of it.
        let tip = iv.title.overflows().then(|| title.to_string());
        if self.toolTip().map(|now| now.to_string()) != tip {
            self.setToolTip(tip.as_deref().map(NSString::from_str).as_deref());
        }
        if let (Some(x), Some(lead)) = (glyph_x, lead) {
            let high = tabs::ICON_SIDE + 2.0 * GLYPH_OVERHANG;
            iv.glyph.setFrame(NSRect::new(
                NSPoint::new(pad + x - GLYPH_LEFT, ((size.height - high) / 2.0).round()),
                NSSize::new(GLYPH_LEFT + lead + GLYPH_OVERHANG, high),
            ));
        }
        let look = emblem
            .filter(|_| !bare || indicator.is_some())
            .map(|emblem| GlyphLook {
                emblem,
                style,
                indicator,
                down: *down,
            });
        iv.glyph.set(look, *turn, palette, *reduce);
        self.set_hint(hint, size, palette, row.as_ref().map(|row| row.key));
        // The ×'s place: the chip's left corner, or a row's right end — where the ⌘ key takes
        // its room, so while the key shows the × does not.
        iv.close.setFrame(match &row {
            Some(row) => tab_side::ns_rect(row.close),
            None => NSRect::new(
                NSPoint::new(CLOSE_INSET, CLOSE_INSET),
                NSSize::new(CLOSE_SIDE, CLOSE_SIDE),
            ),
        });
        let keyed = row.is_some() && hint.is_some();
        iv.close
            .set(!single && hovered && !keyed, palette, motion.close);
        iv.close.setHidden(single);
        self.setAccessibilityLabel(Some(&NSString::from_str(spoken)));
        let value = NSNumber::numberWithBool(selected);
        // SAFETY: an `NSNumber` is a radio button's accessibility value.
        unsafe { self.setAccessibilityValue(Some(&value)) };
        self.setAccessibilityElement(!single);
    }

    /// The ⌘ key that reaches the tab, right-aligned in the chip's right
    /// padding while ⌘ is held; hidden otherwise. A side row gives the box it is aligned in
    /// (`key`, from the row's left edge).
    fn set_hint(
        &self,
        hint: Option<&str>,
        size: NSSize,
        palette: Palette,
        key: Option<tabs::Span>,
    ) {
        let field = &self.ivars().hint;
        let Some(hint) = hint else {
            field.setHidden(true);
            return;
        };
        if field.stringValue().to_string() != hint {
            // SAFETY: AppKit's constant font weight.
            let weight = unsafe { NSFontWeightMedium };
            field.setFont(Some(&NSFont::systemFontOfSize_weight(HINT_TEXT, weight)));
            field.setStringValue(&NSString::from_str(hint));
        }
        field.setTextColor(Some(&Tint::of(palette.dim, 1.0).color()));
        // The drawing frame, as the title's (an alignment-rect width cut
        // "⌘1" to "…").
        let natural = field.fittingSize();
        let (room, end) = match key {
            Some(key) => (key.width, key.end()),
            None => (CHIP_PAD, size.width - CLOSE_INSET),
        };
        let width = natural.width.ceil().min(room);
        field.setFrame(NSRect::new(
            NSPoint::new(
                (end - width).max(0.0),
                ((size.height - natural.height) / 2.0).max(0.0),
            ),
            NSSize::new(width, natural.height),
        ));
        field.setHidden(false);
    }
}

/// A chip's tab and state ([`Chip::set`]).
#[derive(Clone, Debug)]
struct Look<'a> {
    title: &'a str,
    /// What the icon before the title is of; `None` while the tab is being named.
    emblem: Option<Emblem>,
    indicator: Option<Indicator>,
    /// How the running ring goes round; `None` stands it still (`tabs::ring_turn`).
    turn: Option<Turn>,
    /// A transfer's arrow points down.
    down: bool,
    /// How icons are drawn now (the settings).
    style: IconStyle,
    lines: Lines,
    /// How far its transfer is, `0..=1`: its ground fills to there; `None` when none flows.
    fill: Option<f64>,
    /// Its ground is the transfer's fill now — it flows or its end rests ([`ChipFill::lit`]): the
    /// title is in full ink, as a hovered tab's.
    glowing: bool,
    /// Reduce Motion: the fill does not run, and ends in one short fade.
    reduce: bool,
    /// The ⌘ key that reaches the tab, while ⌘ is held.
    hint: Option<String>,
    /// What VoiceOver says (`tabs::spoken`).
    spoken: String,
    selected: bool,
    hovered: bool,
    /// Picked together with other tabs ([`TabBar::pick`]): a quiet accent face and full ink.
    picked: bool,
    /// The one-tab form: a centred title, no fill, no `×`, no click.
    single: bool,
    /// Lifted by the pointer: the tab is being dragged along the strip.
    lifted: bool,
    /// Its tab is carried away from the strip in a drag session.
    ghost: bool,
    /// The chip is a row of the side column: its parts sit where `tabs::Row` puts them, the `×`
    /// and the ⌘ hint at the right end.
    side: bool,
}

/// What a layout knows that is the same for every chip: the part of a [`Look`] that comes from
/// the window and the settings, not from the chip's place.
struct Seen {
    theme: Theme,
    count: usize,
    /// The window is on screen: a ring turns only then.
    visible: bool,
    reduce: bool,
    style: IconStyle,
    /// ⌘ is held: the chips show their keys.
    hints: bool,
}

/// How one chip stands in the layout ([`Seen::look`]).
struct Standing {
    index: usize,
    /// Its tab is being named: the field takes the place of icon and title.
    naming: bool,
    selected: bool,
    hovered: bool,
    /// Picked together with other tabs ([`Look::picked`]).
    picked: bool,
    glowing: bool,
    single: bool,
    lifted: bool,
    ghost: bool,
    /// A row of the side column ([`Look::side`]).
    side: bool,
}

impl Seen {
    /// Chip `standing.index`'s look: its tab's `label`, shown with `title`. The one place a
    /// bar's chip and a column's row are given theirs.
    fn look<'a>(&self, label: &'a Label, title: &'a str, standing: Standing) -> Look<'a> {
        let Standing {
            index,
            naming,
            selected,
            hovered,
            picked,
            glowing,
            single,
            lifted,
            ghost,
            side,
        } = standing;
        let mark = (label.mark != HostMark::None).then(|| self.theme.mark_rgb(label.mark));
        Look {
            title,
            emblem: Some(label.emblem).filter(|_| !naming),
            indicator: label.indicator.filter(|_| !naming),
            turn: label
                .running
                .filter(|_| self.visible)
                .and_then(|elapsed| tabs::ring_turn(elapsed, self.reduce)),
            down: label.upload.is_some_and(|upload| upload.down),
            style: self.style,
            lines: if side {
                Lines {
                    top: None,
                    left: mark,
                }
            } else {
                Lines {
                    top: mark,
                    left: None,
                }
            },
            fill: label.upload.map(|upload| upload.fraction),
            glowing,
            reduce: self.reduce,
            hint: self
                .hints
                .then(|| tabs::shortcut_hint(index, self.count))
                .flatten(),
            spoken: tabs::spoken(
                &label.title,
                label.indicator,
                label.upload.map(|upload| upload.percent),
                label.mark,
            ),
            selected,
            hovered,
            picked,
            single,
            lifted,
            ghost,
            side,
        }
    }
}

/// Which fill a chip's face view draws: the hover's, which fades in and
/// out, the selected tab's, shown or hidden, or the dragged tab's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Face {
    Hovered,
    /// A row picked together with others: the accent's quiet tone.
    Picked,
    Selected,
    /// A chip lifted by the pointer: the fill of a tab being dragged.
    Lifted,
}

pub(crate) struct FaceIvars {
    face: Face,
    palette: Cell<Option<Palette>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; ChipFace implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabFace"]
    #[ivars = FaceIvars]
    pub(crate) struct ChipFace;

    unsafe impl NSObjectProtocol for ChipFace {}

    impl ChipFace {
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let Some(palette) = self.ivars().palette.get() else {
                return;
            };
            let bounds = self.bounds();
            match self.ivars().face {
                Face::Hovered => rounded(bounds, TAB_RADIUS, palette.hover, None),
                Face::Picked => rounded(
                    bounds,
                    TAB_RADIUS,
                    palette.picked,
                    Some(palette.picked_line),
                ),
                Face::Selected => rounded(
                    bounds,
                    TAB_RADIUS,
                    palette.selected,
                    Some(palette.selected_line),
                ),
                Face::Lifted => rounded(
                    bounds,
                    TAB_RADIUS,
                    palette.lifted,
                    Some(palette.lifted_line),
                ),
            }
        }
    }
);

impl ChipFace {
    fn new(mtm: MainThreadMarker, face: Face) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(FaceIvars {
            face,
            palette: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }

    fn set(&self, palette: Palette) {
        let iv = self.ivars();
        if iv.palette.replace(Some(palette)) == Some(palette) {
            return;
        }
        // The light theme's selected face lifts off the row with a thin
        // shadow, a dragged one with a wider, softer one (as much as the
        // strip's margin lets fall); every other face lies flat.
        let cast = |tint: Tint, blur: f64, drop: f64| {
            let shadow = NSShadow::new();
            shadow.setShadowOffset(NSSize::new(0.0, -drop));
            shadow.setShadowBlurRadius(blur);
            shadow.setShadowColor(Some(&tint.color()));
            shadow
        };
        let shadow = match iv.face {
            Face::Selected => palette.shadow.map(|tint| cast(tint, 2.0, 1.0)),
            Face::Lifted => Some(cast(palette.lift_shadow, 5.0, 2.0)),
            Face::Hovered | Face::Picked => None,
        };
        self.setShadow(shadow.as_deref());
        self.setNeedsDisplay(true);
    }
}

// ─── The summary card ─────────────────────────────────────────────────────

/// What a card's map is drawn from: the panes, the shape of the tab's area, how icons are worn
/// and the colour of a marked host (no pane's state wears it, the lines' roles want it).
pub(crate) struct MapDraw<'a> {
    panes: &'a [CardPane],
    aspect: f64,
    style: IconStyle,
    mark: u32,
}

pub(crate) struct CardIvars {
    labels: RefCell<Vec<Retained<NSTextField>>>,
    /// The map's panes, in the card's order; empty without a map.
    tiles: RefCell<Vec<Retained<MapTile>>>,
    palette: Cell<Option<Palette>>,
    /// The window whose bar the card belongs to and the tab it tells ([`SummaryCard::aim`]):
    /// where a press on a pane of the map goes.
    window: Cell<u64>,
    tab: Cell<u64>,
    /// The card is fading away ([`TabBar::close_card`]): still on screen, no longer there for the
    /// pointer.
    leaving: Cell<bool>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; SummaryCard implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabCard"]
    #[ivars = CardIvars]
    pub(crate) struct SummaryCard;

    unsafe impl NSObjectProtocol for SummaryCard {}

    impl SummaryCard {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// Without a map it only tells: the pointer and its clicks go to what is under it. With
        /// one, a pane of the map takes them and the rest of the card holds them (a press on
        /// its margin does nothing, and does not reach the content under it); `point` is in
        /// the card's superview's space.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            self.hit(point)
        }

        /// A press on the margin of a card with a picture is held, not passed on: it neither moves
        /// the window nor reaches the content under the card.
        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {}

        /// The pointer came onto the card: its bar keeps it open.
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            if !self.ivars().tiles.borrow().is_empty() && !self.leaving() {
                self.on_bar(|bar, _| bar.card_entered());
            }
        }

        /// The pointer left the card: its bar closes it, unless the pointer is back on the chip.
        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            if !self.ivars().tiles.borrow().is_empty() && !self.leaving() {
                self.on_bar(|bar, _| bar.card_left());
            }
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            if let Some(palette) = self.ivars().palette.get() {
                rounded(self.bounds(), CARD_RADIUS, palette.card, Some(palette.card_line));
            }
        }
    }
);

impl SummaryCard {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(CardIvars {
            labels: RefCell::new(Vec::new()),
            tiles: RefCell::new(Vec::new()),
            palette: Cell::new(None),
            window: Cell::new(0),
            tab: Cell::new(0),
            leaving: Cell::new(false),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        // Its own layer from the start (a view under a layer-backed root has none until its
        // first display): the card's coming and going play on it, with its shadow and lines,
        // which fade as one picture (the lines over the fill do not show through it).
        this.setWantsLayer(true);
        if let Some(layer) = this.layer() {
            layer.setAllowsGroupOpacity(true);
        }
        this.setAccessibilityElement(false);
        // For the pointer's coming and going once there is a map; the events do nothing before.
        track_hover(&this);
        this
    }

    /// The card is fading away.
    fn leaving(&self) -> bool {
        self.ivars().leaving.get()
    }

    fn set_leaving(&self, leaving: bool) {
        self.ivars().leaving.set(leaving);
    }

    /// `hitTest:`'s body: `point` is in the card's superview's space.
    fn hit(&self, point: NSPoint) -> Option<Retained<NSView>> {
        let tiles = self.ivars().tiles.borrow();
        if tiles.is_empty() || self.leaving() || !contains(self.frame(), point) {
            return None;
        }
        // SAFETY: reading the superview; we are on the main thread.
        let local = self.convertPoint_fromView(point, unsafe { self.superview() }.as_deref());
        let tile = tiles.iter().find(|tile| contains(tile.frame(), local));
        Some(match tile {
            Some(tile) => Retained::into_super(tile.clone()),
            None => Retained::into_super(self.retain()),
        })
    }

    /// The card belongs to the bar of window `window` and tells tab `tab`.
    fn aim(&self, window: u64, tab: u64) {
        let iv = self.ivars();
        iv.window.set(window);
        iv.tab.set(tab);
    }

    /// Runs `job` with the card's bar and tab, if the window is still there.
    fn on_bar(&self, job: impl FnOnce(&TabBar, u64)) {
        let iv = self.ivars();
        let window = app::delegate(self.mtm()).and_then(|app| app.window(iv.window.get()));
        if let Some(window) = window {
            job(window.bar(), iv.tab.get());
        }
    }

    /// A press on the map's pane `pane`: the bar shows its tab with that pane focused.
    fn press(&self, pane: u64) {
        self.on_bar(|bar, tab| bar.pick_pane(tab, pane));
    }

    /// Each pane's new picture to its tile; a pane that gave none this time keeps the one it had.
    fn set_shots(&self, shots: &[(u64, Option<CFRetained<CGImage>>)]) {
        let tiles = self.ivars().tiles.borrow();
        for (pane, image) in shots {
            let tile = tiles.iter().find(|tile| tile.ivars().pane.get() == *pane);
            if let (Some(image), Some(tile)) = (image, tile) {
                tile.set_shot(image);
            }
        }
    }

    /// Some tile of the map has no picture yet (it is new, or its pane was not ready).
    fn lacks_shots(&self) -> bool {
        self.ivars()
            .tiles
            .borrow()
            .iter()
            .any(|tile| !tile.has_shot())
    }

    /// The tiles let go of their pictures.
    fn clear_shots(&self) {
        for tile in self.ivars().tiles.borrow().iter() {
            tile.clear_shot();
        }
    }

    /// Writes `lines` (text and colour) top to bottom and the `map` — a tile per pane of the tab,
    /// a tab of one pane's as much as a split tab's — under them, and sizes the card to them
    /// (`tabs::card_width`, `tabs::card_height`); returns its size.
    fn write(
        &self,
        lines: &[(String, u32)],
        map: Option<&MapDraw<'_>>,
        palette: Palette,
    ) -> NSSize {
        let iv = self.ivars();
        if iv.palette.replace(Some(palette)) != Some(palette) {
            let shadow = NSShadow::new();
            shadow.setShadowOffset(NSSize::new(0.0, -3.0));
            shadow.setShadowBlurRadius(12.0);
            shadow.setShadowColor(Some(&palette.card_shadow.color()));
            self.setShadow(Some(&shadow));
            self.setNeedsDisplay(true);
        }
        let width = tabs::card_width(map.is_some());
        let mut labels = iv.labels.borrow_mut();
        while labels.len() > lines.len() {
            if let Some(gone) = labels.pop() {
                gone.removeFromSuperview();
            }
        }
        while labels.len() < lines.len() {
            let field = label(self.mtm());
            field.setAlignment(NSTextAlignment::Left);
            self.addSubview(&field);
            labels.push(field);
        }
        let mut y = CARD_PAD_Y;
        for (index, (field, (text, rgb))) in labels.iter().zip(lines).enumerate() {
            // SAFETY: AppKit's constant font weights.
            let (size, weight) = if index == 0 {
                (CHIP_TEXT, unsafe { NSFontWeightSemibold })
            } else {
                (CARD_TEXT, unsafe { NSFontWeightRegular })
            };
            field.setFont(Some(&NSFont::systemFontOfSize_weight(size, weight)));
            field.setTextColor(Some(&Tint::of(*rgb, 1.0).color()));
            field.setStringValue(&NSString::from_str(text));
            let height = field.intrinsicContentSize().height;
            field.setFrame(NSRect::new(
                NSPoint::new(CARD_PAD_X, y),
                NSSize::new(width - 2.0 * CARD_PAD_X, height),
            ));
            y += height + CARD_LINE_GAP;
        }
        let bottom = y - CARD_LINE_GAP;
        let Some(map) = map else {
            self.set_tiles(None, 0.0, palette);
            return NSSize::new(width, tabs::card_height(bottom, None));
        };
        self.set_tiles(Some(map), tabs::map_top(bottom), palette);
        NSSize::new(width, tabs::card_height(bottom, Some(map.aspect)))
    }

    /// The map's panes at `top` in the card, in place if they are the same panes as before (the
    /// pointer may be on one of them), new otherwise; none without a `map`.
    fn set_tiles(&self, map: Option<&MapDraw<'_>>, top: f64, palette: Palette) {
        let iv = self.ivars();
        let mut tiles = iv.tiles.borrow_mut();
        let panes = map.map_or(&[][..], |map| map.panes);
        let same = tiles.len() == panes.len()
            && tiles
                .iter()
                .zip(panes)
                .all(|(tile, pane)| tile.ivars().pane.get() == pane.id);
        if !same {
            for gone in tiles.drain(..) {
                gone.removeFromSuperview();
            }
            for _ in panes {
                let tile = MapTile::new(self.mtm());
                self.addSubview(&tile);
                tiles.push(tile);
            }
        }
        let Some(map) = map else {
            return;
        };
        let height = tabs::map_height(map.aspect);
        for (tile, pane) in tiles.iter().zip(panes) {
            let at = tabs::map_tile(pane.place, height);
            let frame = NSRect::new(
                NSPoint::new(CARD_PAD_X + at.x, top + at.y),
                NSSize::new(at.width, at.height),
            );
            tile.set(pane, frame, map, palette);
        }
    }
}

pub(crate) struct TileIvars {
    pane: Cell<u64>,
    /// The pointer is over it.
    hot: Cell<bool>,
    focused: Cell<bool>,
    palette: Cell<Option<Palette>>,
    /// The pane's picture, in the tile's size in points; none until the first one is taken.
    shot: RefCell<Option<Retained<NSImage>>>,
    /// The scrim's height, points: the tile says a state under its name or does not.
    scrim: Cell<f64>,
    icon: Retained<NSImageView>,
    name: Retained<NSTextField>,
    state: Retained<NSTextField>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; MapTile implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabCardPane"]
    #[ivars = TileIvars]
    pub(crate) struct MapTile;

    unsafe impl NSObjectProtocol for MapTile {}

    impl MapTile {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }

        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            self.addCursorRect_cursor(self.bounds(), &NSCursor::pointingHandCursor());
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.ivars().hot.set(true);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.ivars().hot.set(false);
            self.setNeedsDisplay(true);
        }

        /// A press on the pane: its tab is shown with the pane focused.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.press();
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_press(&self) -> bool {
            self.press();
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let Some(palette) = iv.palette.get() else {
                return;
            };
            let (face, hot, ring) = (MAP_FACE, MAP_FACE_HOT, MAP_RING);
            let pick = |(dark, light): (f64, f64)| if palette.dark { dark } else { light };
            let lit = iv.hot.get();
            let strength = if lit { pick(hot) } else { pick(face) };
            let line = iv
                .focused
                .get()
                .then(|| Tint::of(palette.title, pick(ring)));
            let picture = iv.shot.borrow();
            paint_tile(
                self.bounds(),
                Tint::of(palette.title, strength),
                picture.as_deref().map(|image| TileShot {
                    image,
                    fraction: if lit { 1.0 } else { MAP_SHOT_DIM },
                    // The face as it looks over the card — opaque, for the scrim to hide the
                    // picture behind.
                    scrim: (mix(palette.card.rgb, palette.title, strength), iv.scrim.get()),
                }),
                line,
            );
        }
    }
);

impl MapTile {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let icon = NSImageView::new(mtm);
        icon.setImageScaling(NSImageScaling::ScaleNone);
        let name = label(mtm);
        let state = label(mtm);
        for field in [&name, &state] {
            field.setAlignment(NSTextAlignment::Left);
        }
        let this = Self::alloc(mtm).set_ivars(TileIvars {
            pane: Cell::new(0),
            hot: Cell::new(false),
            focused: Cell::new(false),
            palette: Cell::new(None),
            shot: RefCell::new(None),
            scrim: Cell::new(MAP_SCRIM_NAME_ONLY),
            icon: icon.clone(),
            name: name.clone(),
            state: state.clone(),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.addSubview(&icon);
        this.addSubview(&name);
        this.addSubview(&state);
        track_hover(&this);
        this.setAccessibilityElement(true);
        // SAFETY: AppKit's constant role string, alive for the process.
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        this
    }

    /// The card this pane is on.
    fn card(&self) -> Option<Retained<SummaryCard>> {
        // SAFETY: reading the superview; we are on the main thread.
        unsafe { self.superview() }?.downcast::<SummaryCard>().ok()
    }

    fn press(&self) {
        if let Some(card) = self.card() {
            card.press(self.ivars().pane.get());
        }
    }

    /// The pane's picture, laid in the tile at the tile's own size.
    fn set_shot(&self, image: &CGImage) {
        let picture = NSImage::initWithCGImage_size(NSImage::alloc(), image, self.frame().size);
        self.ivars().shot.replace(Some(picture));
        self.setNeedsDisplay(true);
    }

    fn has_shot(&self) -> bool {
        self.ivars().shot.borrow().is_some()
    }

    fn clear_shot(&self) {
        if self.ivars().shot.take().is_some() {
            self.setNeedsDisplay(true);
        }
    }

    /// Shows `pane` at `frame` (in the card's space): its program's tile and what it reports,
    /// its name, and under it the word for its state in the role's colour. What does not fit the
    /// pane's height is left out rather than cut: the name needs the tile's mark beside it, the
    /// state a line under the name.
    fn set(&self, pane: &CardPane, frame: NSRect, map: &MapDraw<'_>, palette: Palette) {
        let iv = self.ivars();
        iv.pane.set(pane.id);
        // A tab of one pane is a map of one tile: its pane is the focused one and wears the ring.
        iv.focused.set(pane.focused);
        iv.palette.set(Some(palette));
        self.setFrame(frame);
        let (width, height) = (frame.size.width, frame.size.height);
        let image = listed_image(pane.kind, pane.indicator, map.style, palette);
        iv.icon.setImage(Some(&image));
        iv.icon.setFrame(NSRect::new(
            NSPoint::new(MAP_TILE_AT, MAP_TILE_AT),
            NSSize::new(LIST_SIDE, LIST_SIDE),
        ));
        iv.icon
            .setHidden(height < MAP_TILE_AT + LIST_SIDE || width < MAP_TILE_AT + LIST_SIDE);
        let room = width - MAP_TEXT_AT - MAP_TEXT_RIGHT;
        let text_ink = if pane.focused {
            palette.title
        } else {
            mix(palette.title, palette.dim, 0.5)
        };
        // SAFETY: AppKit's constant font weights.
        let weight = if pane.focused {
            unsafe { NSFontWeightSemibold }
        } else {
            unsafe { NSFontWeightMedium }
        };
        let name = &iv.name;
        name.setFont(Some(&NSFont::systemFontOfSize_weight(CARD_TEXT, weight)));
        name.setTextColor(Some(&Tint::of(text_ink, 1.0).color()));
        name.setStringValue(&NSString::from_str(&pane.name));
        name.setFrame(NSRect::new(
            NSPoint::new(MAP_TEXT_AT, MAP_TILE_AT),
            NSSize::new(room.max(0.0), MAP_NAME_HEIGHT),
        ));
        name.setHidden(height < MAP_TILE_AT + MAP_NAME_HEIGHT || room < 12.0);
        let state = &iv.state;
        let word = pane.state();
        iv.scrim.set(if word.is_some() {
            MAP_SCRIM_WITH_STATE
        } else {
            MAP_SCRIM_NAME_ONLY
        });
        if let Some((text, tone)) = &word {
            state.setFont(Some(&NSFont::systemFontOfSize_weight(
                MAP_STATE_TEXT,
                unsafe { NSFontWeightRegular },
            )));
            state.setTextColor(Some(&Tint::of(palette.tone(*tone, map.mark), 1.0).color()));
            state.setStringValue(&NSString::from_str(text));
        }
        state.setFrame(NSRect::new(
            NSPoint::new(MAP_TEXT_AT, MAP_STATE_AT),
            NSSize::new(room.max(0.0), MAP_STATE_HEIGHT),
        ));
        state.setHidden(word.is_none() || height < MAP_STATE_AT + MAP_STATE_HEIGHT || room < 12.0);
        let said = match &word {
            Some((text, _)) => format!("{}, {text}", pane.name),
            None => pane.name.clone(),
        };
        self.setAccessibilityLabel(Some(&NSString::from_str(&said)));
        self.setNeedsDisplay(true);
    }
}

/// What a map tile lays over its face: its pane's picture and how strongly, and the scrim that
/// quiets the picture's top under the tile's mark, name and state: the face's own colour, and its
/// height.
struct TileShot<'a> {
    image: &'a NSImage,
    fraction: f64,
    scrim: (u32, f64),
}

/// A map tile, bottom to top: its face; the pane's picture over it, clipped to the tile's corners;
/// the scrim (opaque for [`MAP_SCRIM_HOLD`] of its height, then fading to nothing);
/// and the ring
/// of the focused pane over all of it — drawn under the picture the picture would cover it. The
/// mark, name and state are the tile's own subviews, over this.
///
/// The picture is drawn with the flip of the view respected, so it stands the right way up in a
/// flipped view (a tile is one) and the picture's top is the tile's top.
fn paint_tile(bounds: NSRect, face: Tint, shot: Option<TileShot<'_>>, ring: Option<Tint>) {
    rounded(bounds, tabs::MAP_RADIUS, face, None);
    if let Some(TileShot {
        image,
        fraction,
        scrim,
    }) = shot
    {
        NSGraphicsContext::saveGraphicsState_class();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            bounds,
            tabs::MAP_RADIUS,
            tabs::MAP_RADIUS,
        )
        .addClip();
        // SAFETY: no hints.
        unsafe {
            image.drawInRect_fromRect_operation_fraction_respectFlipped_hints(
                bounds,
                NSRect::ZERO,
                NSCompositingOperation::SourceOver,
                fraction,
                true,
                None,
            );
        }
        {
            let (rgb, height) = scrim;
            let top = bounds.origin.y;
            let solid = height * MAP_SCRIM_HOLD;
            Tint::of(rgb, MAP_SCRIM_STRENGTH).color().setFill();
            NSBezierPath::fillRect(NSRect::new(
                bounds.origin,
                NSSize::new(bounds.size.width, solid),
            ));
            let gradient = NSGradient::initWithStartingColor_endingColor(
                NSGradient::alloc(),
                &Tint::of(rgb, MAP_SCRIM_STRENGTH).color(),
                &Tint::of(rgb, 0.0).color(),
            );
            if let Some(gradient) = gradient {
                // From the end of the solid part to the scrim's foot, down the (flipped) view.
                gradient.drawFromPoint_toPoint_options(
                    NSPoint::new(bounds.origin.x, top + solid),
                    NSPoint::new(bounds.origin.x, top + height),
                    NSGradientDrawingOptions::empty(),
                );
            }
        }
        NSGraphicsContext::restoreGraphicsState_class();
    }
    if let Some(line) = ring {
        inner_line(bounds, tabs::MAP_RADIUS, line);
    }
}

/// Where the summary card (`card` points across and down) of the row of the side column that
/// spans `row_top` down from the window's top sits in a `root` (width, height) window whose
/// column ends at `column_right`: [`CARD_BESIDE`] right of the column, [`CARD_RISE`] above the
/// row, slid up or down as far as it takes to fit the window with [`CARD_EDGE`] to spare (the
/// top wins when the card is taller than the window). A window too narrow for it pushes it left
/// only as far as the column's edge, never over the rows.
fn card_beside(row_top: f64, column_right: f64, root: (f64, f64), card: (f64, f64)) -> (f64, f64) {
    let x = (column_right + CARD_BESIDE)
        .min(root.0 - card.0 - CARD_BESIDE)
        .max(column_right);
    let y = (row_top - CARD_RISE)
        .min(root.1 - card.1 - CARD_EDGE)
        .max(CARD_EDGE);
    (x, y)
}

/// Where the summary card, `card_width` wide, of a chip spanning
/// `chip_x`…`chip_x + chip_width` sits in a `bar_width` bar: under the
/// chip's left edge, kept [`CARD_PAD_X`] inside both window edges.
fn card_x(chip_x: f64, bar_width: f64, card_width: f64) -> f64 {
    chip_x
        .min(bar_width - card_width - CARD_PAD_X)
        .max(CARD_PAD_X)
}

/// What a card at `frame` looks like when it begins to arrive: [`CARD_SCALE`] of its size about
/// the middle of the edge that faces its chip (`toward` is the step from the card to the chip,
/// so which edge that is), and [`CARD_SLIDE`] farther along it. A rectangle in the same space
/// as `frame`, for [`fitted`] to turn into the layer's transform.
fn card_arrival(frame: Rect, toward: (f64, f64)) -> Rect {
    let (dx, dy) = toward;
    let (cx, cy) = frame.center();
    let pivot = (cx + dx * frame.width / 2.0, cy + dy * frame.height / 2.0);
    let (width, height) = (frame.width * CARD_SCALE, frame.height * CARD_SCALE);
    let middle = (
        pivot.0 + (cx - pivot.0) * CARD_SCALE + dx * CARD_SLIDE,
        pivot.1 + (cy - pivot.1) * CARD_SCALE + dy * CARD_SLIDE,
    );
    Rect::new(
        middle.0 - width / 2.0,
        middle.1 - height / 2.0,
        width,
        height,
    )
}

// ─── `+` and the settings warning ────────────────────────────────────────

/// What a bar button is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    NewTab,
    /// Show All Tabs, only while the tabs do not fit. The side column has no such button: it
    /// lists every tab itself and scrolls, and ⇧⌘\ opens the same menu at the list's head.
    List,
    Warning,
}

pub(crate) struct ButtonIvars {
    kind: Kind,
    hot: Cell<bool>,
    pressed: Cell<bool>,
    palette: Cell<Option<Palette>>,
    /// The corner radius, from how the bar meets the window's corner.
    radius: Cell<f64>,
    /// The bottom-left corner's own radius where it differs from the others: the side column's
    /// New Tab sits in the window's bottom-left corner and keeps its centre.
    foot: Cell<Option<f64>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; BarButton implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabBarButton"]
    #[ivars = ButtonIvars]
    pub(crate) struct BarButton;

    unsafe impl NSObjectProtocol for BarButton {}

    impl BarButton {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let Some(palette) = iv.palette.get() else {
                return;
            };
            let hot = iv.hot.get();
            let fill = if hot { palette.button_hover } else { palette.button };
            match iv.foot.get() {
                Some(foot) => {
                    let radius = iv.radius.get();
                    tab_side::rounded_corners(
                        self.bounds(),
                        [radius, radius, radius, foot],
                        fill,
                        Some(palette.button_line),
                    );
                }
                None => rounded(self.bounds(), iv.radius.get(), fill, Some(palette.button_line)),
            }
            let at = centre(self.bounds());
            let path = NSBezierPath::bezierPath();
            match iv.kind {
                Kind::NewTab => {
                    path.moveToPoint(NSPoint::new(at.x, at.y - 4.5));
                    path.lineToPoint(NSPoint::new(at.x, at.y + 4.5));
                    path.moveToPoint(NSPoint::new(at.x - 4.5, at.y));
                    path.lineToPoint(NSPoint::new(at.x + 4.5, at.y));
                    let color = if hot { palette.title } else { palette.dim };
                    stroke(&path, 1.6, Tint::of(color, 1.0));
                }
                Kind::List => {
                    // A chevron pointing down: the list opens below.
                    path.moveToPoint(NSPoint::new(at.x - 4.5, at.y - 2.0));
                    path.lineToPoint(NSPoint::new(at.x, at.y + 2.5));
                    path.lineToPoint(NSPoint::new(at.x + 4.5, at.y - 2.0));
                    let color = if hot { palette.title } else { palette.dim };
                    stroke(&path, 1.6, Tint::of(color, 1.0));
                }
                Kind::Warning => {
                    // A triangle with an exclamation mark, in `warning`.
                    path.moveToPoint(NSPoint::new(at.x, at.y - 5.0));
                    path.lineToPoint(NSPoint::new(at.x + 5.5, at.y + 4.5));
                    path.lineToPoint(NSPoint::new(at.x - 5.5, at.y + 4.5));
                    path.closePath();
                    path.moveToPoint(NSPoint::new(at.x, at.y - 1.5));
                    path.lineToPoint(NSPoint::new(at.x, at.y + 1.0));
                    stroke(&path, 1.4, Tint::of(palette.warning, 1.0));
                    let dot = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                        NSPoint::new(at.x - 0.75, at.y + 2.4),
                        NSSize::new(1.5, 1.5),
                    ));
                    Tint::of(palette.warning, 1.0).color().setFill();
                    dot.fill();
                }
            }
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.ivars().pressed.set(true);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let pressed = self.ivars().pressed.replace(false);
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            if pressed && contains(self.bounds(), at) {
                self.act();
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.ivars().hot.set(true);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.ivars().hot.set(false);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_press(&self) -> bool {
            self.act();
            true
        }
    }
);

impl BarButton {
    fn new(mtm: MainThreadMarker, kind: Kind) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ButtonIvars {
            kind,
            hot: Cell::new(false),
            pressed: Cell::new(false),
            palette: Cell::new(None),
            radius: Cell::new(TAB_RADIUS),
            foot: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setAccessibilityElement(true);
        // SAFETY: AppKit's constant role string, alive for the process.
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        match kind {
            Kind::NewTab => {
                this.setAccessibilityLabel(Some(ns_string!("New Tab")));
                this.setToolTip(Some(ns_string!("New Tab  \u{2318}T")));
            }
            Kind::List => {
                this.setAccessibilityLabel(Some(ns_string!("Show All Tabs")));
                this.setToolTip(Some(ns_string!("Show All Tabs  \u{21e7}\u{2318}\\")));
            }
            Kind::Warning => {}
        }
        track_hover(&this);
        this
    }

    fn set_look(&self, palette: Palette, radius: f64) {
        let iv = self.ivars();
        let repaint = iv.palette.replace(Some(palette)) != Some(palette);
        if iv.radius.replace(radius) != radius || repaint {
            self.setNeedsDisplay(true);
        }
    }

    /// The bottom-left corner's radius where it is not the others' (`None`: it is).
    fn set_foot(&self, foot: Option<f64>) {
        if self.ivars().foot.replace(foot) != foot {
            self.setNeedsDisplay(true);
        }
    }

    fn act(&self) {
        // SAFETY: reading the superview; we are on the main thread.
        let bar = || unsafe { self.superview() }.and_then(|view| view.downcast::<TabBar>().ok());
        match self.ivars().kind {
            Kind::NewTab => {
                if let Some(bar) = bar() {
                    bar.new_tab();
                }
            }
            Kind::List => {
                if let Some(bar) = bar() {
                    bar.show_list();
                }
                // The menu's tracking swallowed the pointer's exit.
                self.ivars().hot.set(false);
                self.setNeedsDisplay(true);
            }
            // The settings window shows the file's state; the target-less
            // action reaches the application delegate like the menu's.
            Kind::Warning => {
                let app = NSApplication::sharedApplication(self.mtm());
                let sender: &AnyObject = self;
                // SAFETY: `openSettings:` takes one optional sender argument.
                unsafe { app.sendAction_to_from(sel!(openSettings:), None, Some(sender)) };
            }
        }
    }
}

/// A menu item that acts through `window`'s applier: `action` is one of the
/// window's tab selectors, `tag` names the tab ([`tabs::menu_tag`]).
fn window_item(
    mtm: MainThreadMarker,
    window: &TerminalWindow,
    title: &str,
    action: Sel,
    tag: isize,
) -> Retained<NSMenuItem> {
    let item = NSMenuItem::new(mtm);
    item.setTitle(&NSString::from_str(title));
    item.setTag(tag);
    let target: &AnyObject = window;
    // SAFETY: every `action` passed is defined by `TerminalWindow` as a
    // no-return action with a single `Option<&AnyObject>` argument; the
    // item holds its target weakly and the application's window list owns it.
    unsafe {
        item.setAction(Some(action));
        item.setTarget(Some(target));
    }
    item
}

/// A status dot for a menu row, a few points across, in `rgb`: the colour of
/// what the tab reports, as the chip's own indicator wears it. `None` is a
/// clear image of the same size.
fn dot_image(rgb: Option<u32>) -> Retained<NSImage> {
    let draw = RcBlock::new(move |rect: NSRect| -> Bool {
        if let Some(rgb) = rgb {
            Tint::of(rgb, 1.0).color().setFill();
            NSBezierPath::bezierPathWithOvalInRect(rect).fill();
        }
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(8.0, 8.0), false, &draw)
}

// ─── The strip the chips scroll in ───────────────────────────────────────

pub(crate) struct StripIvars {
    palette: Cell<Option<Palette>>,
    /// The separators' centres, in the strip's own space.
    separators: RefCell<Vec<f64>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; ChipStrip implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabStrip"]
    #[ivars = StripIvars]
    pub(crate) struct ChipStrip;

    unsafe impl NSObjectProtocol for ChipStrip {}

    impl ChipStrip {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The separators between two quiet tabs; the chips draw over the
        /// rest. Drawn here, not in the bar, so the strip's clip cuts a
        /// separator of a scrolled-away tab too.
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let Some(palette) = iv.palette.get() else {
                return;
            };
            let height = self.bounds().size.height;
            let y = ((height - SEPARATOR_HEIGHT) / 2.0).max(0.0);
            palette.separator.color().setFill();
            for &x in iv.separators.borrow().iter() {
                let line = NSBezierPath::bezierPathWithRect(NSRect::new(
                    NSPoint::new(x - 0.5, y),
                    NSSize::new(1.0, SEPARATOR_HEIGHT.min(height)),
                ));
                line.fill();
            }
        }

        /// Like the bar under it: the first click on an inactive window
        /// moves it (or selects a chip).
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        /// A press on the strip's empty part goes on to the bar, which moves
        /// the window itself ([`TabBar`]'s `mouseDown:`).
        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }
    }
);

impl ChipStrip {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(StripIvars {
            palette: Cell::new(None),
            separators: RefCell::new(Vec::new()),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        // The chips of a scrolled strip run under its edges and are cut
        // there, not drawn over the traffic lights or the buttons.
        this.setClipsToBounds(true);
        this
    }

    /// The separators (centres in the strip's space) and the colours they
    /// are drawn in.
    fn set_separators(&self, separators: Vec<f64>, palette: Palette) {
        let iv = self.ivars();
        let repaint = iv.palette.replace(Some(palette)) != Some(palette)
            || *iv.separators.borrow() != separators;
        if repaint {
            iv.separators.replace(separators);
            self.setNeedsDisplay(true);
        }
    }
}

// ─── The fade at a scrolled strip's edge ─────────────────────────────────

pub(crate) struct FadeIvars {
    /// The strip's leading edge: opaque on the left. Otherwise the trailing
    /// edge, opaque on the right.
    leading: bool,
    palette: Cell<Option<Palette>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; EdgeFade implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabFade"]
    #[ivars = FadeIvars]
    pub(crate) struct EdgeFade;

    unsafe impl NSObjectProtocol for EdgeFade {}

    impl EdgeFade {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// Takes no click: the chips under it do.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        /// The theme's background, running out to nothing towards the strip's
        /// middle: what runs under the edge dissolves into the window.
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let Some(palette) = iv.palette.get() else {
                return;
            };
            let solid = Tint::of(palette.ground, 1.0).color();
            let clear = Tint::of(palette.ground, 0.0).color();
            let (from, to) = if iv.leading {
                (&solid, &clear)
            } else {
                (&clear, &solid)
            };
            let gradient = NSGradient::initWithStartingColor_endingColor(NSGradient::alloc(), from, to);
            if let Some(gradient) = gradient {
                gradient.drawInRect_angle(self.bounds(), 0.0);
            }
        }
    }
);

impl EdgeFade {
    fn new(mtm: MainThreadMarker, leading: bool) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(FadeIvars {
            leading,
            palette: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setHidden(true);
        this.setAccessibilityElement(false);
        this
    }

    fn set_palette(&self, palette: Palette) {
        if self.ivars().palette.replace(Some(palette)) != Some(palette) {
            self.setNeedsDisplay(true);
        }
    }
}

// ─── The bar's claim on the title row ────────────────────────────────────

define_class!(
    /// An empty control the bar's size, under everything in it, taking no click: it keeps the
    /// title row from the window server, so a press in the bar is the bar's to decide.
    ///
    /// **Why.** The window's content reaches under the title row (`FullSizeContentView`), and
    /// there the window server moves the window itself, before any view sees the drag. It moves
    /// it from the whole row less what the views in it claim for themselves — AppKit asks each
    /// visible view (`_opaqueRectForWindowMoveWhenInTitlebar`, a private method) — and a plain
    /// `NSView` claims nothing; its `mouseDownCanMoveWindow` returning `NO` does not count
    /// there. So a press on a chip reached the chip, but the drag that followed moved the
    /// window: a tab could not be moved along the strip, torn off or dropped on another
    /// window's bar. An enabled `NSControl` that accepts the keyboard claims its bounds —
    /// measured on macOS 26.4.1: `NSView` claims nothing, `NSControl`, `NSButton` and
    /// `NSTextField` their bounds, and a control disabled or refusing the keyboard nothing.
    /// This one claims the whole bar, and every press goes on to the bar as
    /// before ([`TabBar`]'s `mouseDown:`): a chip's drag is the tab's, the empty part's moves
    /// the window (`performWindowDragWithEvent:`), a double click does the system's setting.
    ///
    /// **Not** `NSWindow.isMovable = NO`, which also stops the server: the system then greys out
    /// Window ▸ Move & Resize, taking a window it believes cannot move. **Not** the bar itself
    /// as a control: `NSControl` changes how a view takes the mouse and the keyboard; this one
    /// only claims, its `hitTest:` is `nil`, so neither a click nor the keyboard reaches it.
    ///
    /// Should a macOS stop honouring the claim, the tabs move the window again and no test sees
    /// it: a synthetic event never reaches the window server's drag, only a real pointer does.
    // SAFETY: NSControl is designed for subclassing; RowClaim implements no
    // `Drop`, has no ivar and is born with `initWithFrame:`.
    #[unsafe(super(NSControl))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabRowClaim"]
    pub(crate) struct RowClaim;

    unsafe impl NSObjectProtocol for RowClaim {}

    impl RowClaim {
        /// Takes no click: the bar and what is in it do.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl RowClaim {
    /// The claim, following the size of the view it is put in.
    pub(crate) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:` is NSView's designated initializer; the
        // subclass has no ivar.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        // It still accepts the keyboard, as a bare control does: the claim
        // holds only while it does (measured — refusing it, by the flag or by
        // `acceptsFirstResponder`, drops the claim to nothing). The keyboard
        // never comes: no click reaches it and it is no key view.
        this.setAccessibilityElement(false);
        this
    }
}

// ─── The bar ─────────────────────────────────────────────────────────────

/// A transfer flowing in a tab ([`Label::upload`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Upload {
    /// Only downloads flow: the arrow points down (the title prefix's `↓`).
    pub(crate) down: bool,
    /// The title prefix's percentage — what VoiceOver says.
    pub(crate) percent: u8,
    /// How far all of the tab's transfers are, `0..=1` — how far its ground is filled.
    pub(crate) fraction: f64,
}

/// One tab as the bar shows it ([`TabBar::show`]).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Label {
    /// The tab's id (`TerminalTab::id`).
    pub(crate) tab: u64,
    pub(crate) title: String,
    /// What its chip shows left of the title (`TerminalTab::indicator`).
    pub(crate) indicator: Option<Indicator>,
    /// What the icon before the title is of (`TerminalTab::emblem`).
    pub(crate) emblem: Emblem,
    /// How long its running command has run (`TerminalTab::running_for`), when it was read:
    /// the angle a ring is set turning from and the bar's clock.
    pub(crate) running: Option<Duration>,
    /// A transfer flowing in it (`TerminalTab::upload`).
    pub(crate) upload: Option<Upload>,
    /// Its focused pane's marked host (`TerminalTab::host_mark`).
    pub(crate) mark: HostMark,
}

/// One group's header as the bar shows it ([`TabBar::show`]).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Heading {
    pub(crate) group: GroupId,
    pub(crate) name: String,
    /// The column lists the header and, while the tab on screen is of the group, that tab.
    pub(crate) collapsed: bool,
    /// How many tabs the group holds, the hidden ones too.
    pub(crate) tabs: usize,
    /// What a closed group's dot says: the most urgent thing that asks for the user in any of its
    /// tabs (`tabs::group_indicator`).
    pub(crate) indicator: Option<Indicator>,
    /// The digit of the key that reaches the group with ⌃⌘ ([`tabs::shortcut_digit`]).
    pub(crate) hint: Option<u8>,
    /// The index in [`Shown::tabs`] of the first row below the header; the number of rows when
    /// nothing is below it (a closed group whose tab is not the one on screen).
    pub(crate) first_row: usize,
}

/// What the title row of a window with groups adds to the bar: the group button's group and the
/// list it opens ([`TabBar::show`]).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Switcher {
    /// The group on screen, whose tabs the bar lists; `None` on the ungrouped tabs.
    pub(crate) shown: Option<GroupId>,
    /// Its name; `None` on the ungrouped tabs, whose button is the stack alone.
    pub(crate) name: Option<String>,
    /// What the button's dot says: the most urgent thing that asks for the user in any tab
    /// outside the group on screen (`tabs::group_indicator`).
    pub(crate) elsewhere: Option<Indicator>,
    /// The list's rows: the ungrouped tabs' first, then each group's.
    pub(crate) rows: Vec<SwitchRow>,
}

/// One row of the group list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SwitchRow {
    /// The group the row brings on screen; `None` for the ungrouped tabs.
    pub(crate) group: Option<GroupId>,
    pub(crate) name: String,
    /// What the group asks of the user (`tabs::group_indicator`).
    pub(crate) indicator: Option<Indicator>,
    /// The digit of the key that reaches it with ⌃⌘ ([`tabs::shortcut_digit`]; 0 for the ungrouped
    /// tabs); none for a group past the ninth that no key reaches.
    pub(crate) hint: Option<u8>,
}

/// What the bar shows; the window gives it ([`TabBar::show`]).
#[derive(Clone, Debug, Default)]
struct Shown {
    /// The tabs the bar lists, in the order it lists them: every tab in the title row, and in the
    /// column those under an open header or the one on screen under a closed one.
    tabs: Vec<Label>,
    /// The groups' headers of the column, each with the row it stands above; none in the title
    /// row and none while no group exists.
    headings: Vec<Heading>,
    /// The group button and its list, in the title row of a window with groups; none in the
    /// column and none while no group exists.
    switcher: Option<Switcher>,
    selected: usize,
    /// The settings diagnostic; empty without one.
    notice: String,
}

/// The tabs picked together in the side column, and the tab on screen when they were picked: the
/// selection ends the moment that tab is another ([`kept_picks`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Picks {
    /// In the order the column lists them.
    tabs: Vec<u64>,
    shown: Option<u64>,
}

/// What of `picks` holds when the bar is shown anew (`selected` indexes `tabs`): nothing in the
/// title row, where tabs are not picked together; nothing once the tab on screen is another than
/// the one the picks were made from — a pick is relative to the tab on screen, ⇧ counts from it;
/// the picked tabs still listed otherwise (a closed group hides its tabs, a closed tab is
/// gone); and nothing when what is left is the tab on screen alone, which is no selection.
fn kept_picks(picks: &Picks, tabs: &[Label], selected: usize, side: bool) -> Picks {
    let screen = tabs.get(selected).map(|label| label.tab);
    if !side || picks.shown != screen {
        return Picks::default();
    }
    let kept: Vec<u64> = picks
        .tabs
        .iter()
        .copied()
        .filter(|&tab| tabs.iter().any(|label| label.tab == tab))
        .collect();
    if kept.is_empty() || kept.as_slice() == screen.as_slice() {
        return Picks::default();
    }
    Picks {
        tabs: kept,
        shown: screen,
    }
}

/// How a press on a row with `flags` held picks, if it does: ⇧ (a range from the tab on screen)
/// or ⌘ (the row joins or leaves), with nothing else a shortcut uses — ⌥⌘ is the arrangement's,
/// ⌃ a context menu's, and Caps Lock and the like do not count. ⇧⌘ is ⇧: a range, as in a list.
fn pick_click(flags: NSEventModifierFlags) -> Option<Click> {
    let chord = NSEventModifierFlags::Command
        | NSEventModifierFlags::Shift
        | NSEventModifierFlags::Option
        | NSEventModifierFlags::Control;
    let held = flags.intersection(chord);
    let other = NSEventModifierFlags::Option | NSEventModifierFlags::Control;
    if held.is_empty() || !held.intersection(other).is_empty() {
        None
    } else if held.contains(NSEventModifierFlags::Shift) {
        Some(Click::Range)
    } else {
        Some(Click::Toggle)
    }
}

/// A menu title that counts tabs: "Close 3 Tabs…".
pub(crate) fn counted(verb: &str, count: usize, tail: &str) -> String {
    let noun = if count == 1 { "Tab" } else { "Tabs" };
    format!("{verb} {count} {noun}{tail}")
}

/// The open summary card: whose, and what it tells (`TerminalTab::card`,
/// read again when what the tab's chip shows changes; between two reads its
/// running time moves by the time since the read — the command's own clock
/// is monotonic, so no pane is read for it).
#[derive(Clone, Debug)]
struct OpenCard {
    tab: u64,
    card: Card,
    read_at: Instant,
}

impl OpenCard {
    /// How long the card's command has run now; `None` unless it runs — and, with a map of
    /// panes, the card tells no time (each pane says only that it runs), so the clock has nothing
    /// to wake for.
    fn running(&self) -> Option<Duration> {
        match &self.card.command {
            Some(CardCommand::Running { elapsed, .. }) if !self.pictured() => {
                Some(elapsed.saturating_add(self.read_at.elapsed()))
            }
            _ => None,
        }
    }

    /// The card carries a map of its tab's panes under its lines. Pictured, it is a thing the
    /// pointer can cross to and press.
    fn pictured(&self) -> bool {
        self.card.pictured()
    }

    /// The card as it reads now: its running time moved on.
    fn now(&self) -> Card {
        let mut card = self.card.clone();
        if let (Some(CardCommand::Running { elapsed, .. }), Some(now)) =
            (&mut card.command, self.running())
        {
            *elapsed = now;
        }
        card
    }
}

/// What a pane carried over the strip is let go as ([`TabBar::pane_target`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaneTarget {
    /// The middle of tab `tab`'s chip: the pane joins that tab.
    Tab(u64),
    /// Between chips, at slot `gap`: the pane becomes a tab there (a tab's only pane
    /// moves its tab).
    Between(usize),
}

/// What a carry over a bar would come to ([`TabBar::aim`]).
#[derive(Default)]
struct Aim {
    /// Room opens at this slot of the list (a tab from another window, on a plain strip).
    room: Option<usize>,
    /// A line marks the place at this height of the column's content ([`content_line`]).
    line: Option<f64>,
    /// What the bar takes the drop as; `None`: nothing comes of it, though it is taken.
    landed: Option<Landed>,
}

/// `line` (the window's space, from the column's own geometry) as a height in the list's content
/// — not scrolled and from its top — which is where the bar keeps the line that marks a place.
fn content_line(line: f64, column: &Column) -> f64 {
    line - column.list.y + column.scroll
}

/// A press on a chip that may be, or has become, a drag ([`TabBar::grip`]).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Dragging {
    tab: u64,
    grip: Grip,
    /// Once the tab moves along the strip: where its chip is drawn (the bar's space) and the
    /// place it would take if let go now (a slot of the column's spots, [`Self::grouped`]). The
    /// other chips lay out around that place.
    at: Option<(f64, usize)>,
    /// The tab came away from the bar: a drag session carries it and its chip waits, faint,
    /// in the place it left until the session ends ([`TabBar::drag_ended`]).
    torn: bool,
    /// The press was on a row picked together with others: the pull carries them all, as a
    /// session from the first, and a press let go without one is the plain click it would have
    /// been ([`TabBar::release`]).
    multi: bool,
    /// The press was on a row of the column of a window that has groups: the place the row would
    /// take ([`Self::at`]) is a slot of [`crate::groups::Groups::side_spots`], not a place of the
    /// list — the gap opens over a group's header or under it, and a drop puts the row in the
    /// group that was shown ([`TabBar::release`]).
    grouped: bool,
    /// In the side column, the tab of the row whose middle the held row is over, lit: let go
    /// there, the held tab joins it as a split instead of taking a place ([`TabBar::aim_row`]).
    split: Option<u64>,
}

pub(crate) struct BarIvars {
    /// The window's id (`TerminalWindow::id`) — the way to its applier.
    window: u64,
    shown: RefCell<Shown>,
    /// The tab under the pointer.
    hovered: Cell<Option<u64>>,
    /// The press on a chip that is, or may become, a drag.
    drag: Cell<Option<Dragging>>,
    /// A tab carried over this bar from another window: the place it would be
    /// inserted at, where the chips open a gap ([`TabBar::open_gap`]).
    gap: Cell<Option<usize>>,
    /// The tab a carried pane is over the middle of: its chip is lit like a hovered
    /// one ([`TabBar::show_pane_target`]).
    target: Cell<Option<u64>>,
    /// The tab a carried pane waits on to open it (the line under its chip).
    dwelling: Cell<Option<u64>>,
    /// In the side column, the gap a carried pane would become a tab at: a line stands there
    /// instead of room opening ([`TabBar::show_pane_target`]; [`tab_side::Insertion`]).
    mark: Cell<Option<usize>>,
    /// The line's views.
    insertion: tab_side::Insertion,
    /// In the side column, where a line marks the place what is carried over the column would be
    /// put (in the list's content, not scrolled; [`TabBar::show_cast`]).
    cast: Cell<Option<f64>>,
    /// The group whose header is carried away in a session: its header waits, faint, in its place
    /// until the session ends ([`TabBar::drag_ended`]).
    group_torn: Cell<Option<GroupId>>,
    /// The side column's mode: `Some(top)` — the title row's height, where the list starts —
    /// puts the tabs in a column the window's height at its left edge, `None` in the bar in
    /// the title row ([`TabBar::set_side`]; `tab_side.rs`).
    side: Cell<Option<f64>>,
    /// The column as last laid out, in the side mode: the wheel, the list and the name field work
    /// from it.
    column: RefCell<Option<Column>>,
    /// The mask that fades the column's list where rows run under its ends.
    mask: tab_side::Mask,
    /// The tab on screen when the last press on a chip began — a press selects, so
    /// the tab a chip was pulled from is the one before it ([`TabBar::selected_before`]).
    before: Cell<Option<u64>>,
    /// The next layout slides every chip to its place, though the order is the
    /// same: a drag ended, or a gap opened or closed.
    glide: Cell<bool>,
    palette: Cell<Option<Palette>>,
    /// The theme the palette came from — a host mark's colour is its
    /// mapping (`Theme::mark_rgb`), the dock's.
    theme: Cell<Option<Theme>>,
    /// One chip per tab, in strip order, each keeping its tab.
    chips: RefCell<Vec<Retained<Chip>>>,
    /// The ends of transfers told and not played yet (tab, how it ended, when it was told): each
    /// plays on its tab's chip once the tab shows nothing flowing, and is gone — a layout that
    /// comes of a hover or a tick must not play it again ([`TabBar::transfer_ended`]).
    ends: RefCell<Vec<(u64, Finish, Instant)>>,
    /// The view the chips sit in and are cut by.
    strip: Retained<ChipStrip>,
    /// The fades at the strip's two edges, lit where tabs run under them.
    fade_leading: Retained<EdgeFade>,
    fade_trailing: Retained<EdgeFade>,
    /// The strip's scroll as last applied (the layout clamps it).
    scroll: Cell<f64>,
    /// The layout last made: the wheel and the list work from it.
    laid: RefCell<Option<Strip>>,
    /// The tab whose selection was last brought into view — a layout scrolls
    /// to the selection only when it changed (or when the tabs stopped
    /// fitting), so a hover or a clock tick never undoes the user's
    /// scrolling.
    revealed: Cell<Option<u64>>,
    /// The tabs did not fit at the last layout.
    overflowed: Cell<bool>,
    new_tab: Retained<BarButton>,
    /// Show All Tabs, only while the tabs do not fit.
    list: Retained<BarButton>,
    warning: Retained<BarButton>,
    /// The field a tab is named in, in the bar while `renaming` says whose.
    field: Retained<NSTextField>,
    renaming: Cell<Option<u64>>,
    /// What the field held when it opened: unchanged text is not a name
    /// (the title it showed may be stale by the time the field closes).
    opened_with: RefCell<String>,
    /// The bar's width at the last layout: narrowing a window with
    /// overflowing tabs brings the selected one back into view.
    width: Cell<f64>,
    /// The bar's one delayed wake ([`TabBar::arm_clock`]): the generation a
    /// fire must carry to count, and when the live one is due.
    clock: Cell<u64>,
    clock_due: Cell<Option<Instant>>,
    card_view: Retained<SummaryCard>,
    card: RefCell<Option<OpenCard>>,
    /// A card's pending opening — a hover change makes an older one stale.
    card_wait: Cell<u64>,
    /// When the last card closed: the warm window ([`CARD_WARM`]).
    card_closed: Cell<Option<Instant>>,
    /// The tab a press put its card away for, until the pointer leaves it.
    card_quiet: Cell<Option<u64>>,
    /// The pointer is on the open card (one with a picture): it stays while it is.
    card_inside: Cell<bool>,
    /// Where the card view was last put, while it is on screen (fading away included): a new
    /// place that differs makes it glide there, the same one nothing — a layout asks again.
    card_at: Cell<Option<NSPoint>>,
    /// The card's going: a going's removal counts only while this is the generation it was
    /// scheduled for (a card that came back, or went again, makes it stale).
    card_out: Cell<u64>,
    /// ⌘ is held (past [`HINT_DELAY`]): the tabs show their keys.
    hints: Cell<bool>,
    /// A pending hint — ⌘'s release or a chord makes it stale.
    hint_wait: Cell<u64>,
    /// A show is pending ([`HINT_DELAY`]).
    hint_pending: Cell<bool>,
    /// One header per group of the column, in order, each keeping its group.
    headers: RefCell<Vec<Retained<tab_group::GroupHeader>>>,
    /// The group whose name the field holds, in place of a tab's ([`BarIvars::renaming`]).
    renaming_group: Cell<Option<GroupId>>,
    /// The name field of `renaming_group` was opened by the move that made the group: what it
    /// says is the group's birth name, not a rename ([`TerminalWindow::name_new_group`]).
    renaming_born: Cell<bool>,
    /// The group button of the title row, always in the bar and shown only with a switcher.
    switch: Retained<tab_switch::GroupButton>,
    /// The list of groups under it and the view over the window that closes it, the list being
    /// up while the view is in the window ([`TabBar::open_group_list`]).
    group_list: Retained<tab_switch::GroupList>,
    scrim: Retained<tab_switch::ListScrim>,
    list_open: Cell<bool>,
    /// ⌃⌘ is held (past [`HINT_DELAY`]): the headers show their keys.
    group_hints: Cell<bool>,
    /// ⌘'s `hint_wait` and `hint_pending`, for ⌃⌘.
    group_wait: Cell<u64>,
    group_pending: Cell<bool>,
    /// The tabs picked together in the column ([`TabBar::pick`]).
    picks: RefCell<Picks>,
    /// The watch of presses that lets the picks go when one lands off the column: a local monitor
    /// that is there only while something is picked.
    pick_watch: RefCell<Option<Retained<AnyObject>>>,
}

impl Drop for BarIvars {
    fn drop(&mut self) {
        if let Some(token) = self.pick_watch.get_mut().take() {
            // SAFETY: the token is the one `addLocalMonitor…` gave.
            unsafe { NSEvent::removeMonitor(&token) };
        }
    }
}

define_class!(
    // SAFETY: NSView is designed for subclassing; TabBar implements no `Drop`
    // and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabBar"]
    #[ivars = BarIvars]
    pub(crate) struct TabBar;

    unsafe impl NSObjectProtocol for TabBar {}

    impl TabBar {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The first click on an inactive window moves it (or selects a
        /// chip), as on the system's title bar.
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        /// The bar decides itself ([`Self::mouse_down`]): AppKit's own
        /// move would also swallow the double click.
        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }

        /// A press on the row's empty part (and on the single tab's title):
        /// one click moves the window, a double click does what the
        /// system's setting says.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let Some(window) = self.window() else {
                return;
            };
            if event.clickCount() < 2 {
                window.performWindowDragWithEvent(event);
                return;
            }
            let defaults = NSUserDefaults::standardUserDefaults();
            let action = defaults
                .stringForKey(ns_string!("AppleActionOnDoubleClick"))
                .map(|action| action.to_string());
            let legacy = defaults.boolForKey(ns_string!("AppleMiniaturizeOnDoubleClick"));
            match title_double_click(action.as_deref(), legacy) {
                TitleAction::Zoom => window.performZoom(None),
                TitleAction::Minimize => window.performMiniaturize(None),
                TitleAction::Nothing => {}
            }
        }

        /// A right click on the single tab's title: the tab's own menu, as on a chip — the way
        /// to rename the window's only tab or start its first group there. The press that moves
        /// the window stays the row's ([`Self::mouse_down`]); the row's empty part has no menu.
        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, event: &NSEvent) -> Option<Retained<NSMenu>> {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            // No `?`: `define_class!` converts the body's last expression.
            self.tab_under(point)
                .filter(|_| self.lone_title(self.ivars().shown.borrow().tabs.len()))
                .and_then(|tab| self.tab_menu(tab))
        }

        /// A wheel or a trackpad moves the strip once the tabs no longer fit
        /// ([`Strip::wheeled`]: a vertical wheel scrolls it too). Without
        /// overflow nothing scrolls and the event goes on.
        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            if !self.wheel(event) {
                // SAFETY: `NSView`'s `scrollWheel:` takes the event.
                let _: () = unsafe { msg_send![super(self), scrollWheel: event] };
            }
        }

        /// The rename field's command hook: Return keeps the name, Esc
        /// leaves the tab's as it was. The rest is the field's own.
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn control_do_command(
            &self,
            _control: &AnyObject,
            _text_view: &AnyObject,
            command: Sel,
        ) -> bool {
            if command == sel!(insertNewline:) {
                self.end_rename(true);
                true
            } else if command == sel!(cancelOperation:) {
                self.end_rename(false);
                true
            } else {
                false
            }
        }

        /// The field lost the keyboard to something else — a click in a
        /// pane, another tab: the name typed is kept.
        #[unsafe(method(controlTextDidEndEditing:))]
        fn control_end_editing(&self, _notification: &NSNotification) {
            self.end_rename(true);
        }

        /// What is typed over the group button's name widens the button, and the tabs move
        /// aside for it.
        #[unsafe(method(controlTextDidChange:))]
        fn control_changed(&self, _notification: &NSNotification) {
            if self.ivars().renaming_group.get().is_some() && !self.is_side() {
                self.lay_out();
            }
        }
    }

    // The rename field's delegate: both protocols' methods are optional;
    // the ones used are above.
    unsafe impl NSControlTextEditingDelegate for TabBar {}
    unsafe impl NSTextFieldDelegate for TabBar {}

    /// The side of a tab dropped on this bar from a drag session ([`tab_drag`]): a tab of this
    /// process only ([`tab_drag::carried`]). The pointer over the bar opens a gap in the strip
    /// at the place the tab would take; letting go records the landing, which is carried out
    /// when the session has ended (`AppDelegate::tab_drag_ended`).
    unsafe impl NSDraggingDestination for TabBar {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(
            &self,
            info: &ProtocolObject<dyn NSDraggingInfo>,
        ) -> NSDragOperation {
            self.carried_over(info)
        }

        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(
            &self,
            info: &ProtocolObject<dyn NSDraggingInfo>,
        ) -> NSDragOperation {
            self.carried_over(info)
        }

        /// The tab went away from this bar (or was taken back): the gap closes.
        #[unsafe(method(draggingExited:))]
        fn dragging_exited(&self, _info: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
            self.clear_drop();
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            self.accept_drop(info)
        }
    }
);

impl TabBar {
    pub(crate) fn new(mtm: MainThreadMarker, window: u64) -> Retained<Self> {
        let new_tab = BarButton::new(mtm, Kind::NewTab);
        let switch = tab_switch::GroupButton::new(mtm);
        let list = BarButton::new(mtm, Kind::List);
        let warning = BarButton::new(mtm, Kind::Warning);
        let strip = ChipStrip::new(mtm);
        let fade_leading = EdgeFade::new(mtm, true);
        let fade_trailing = EdgeFade::new(mtm, false);
        let field = NSTextField::textFieldWithString(ns_string!(""), mtm);
        // A tab's name is one line that scrolls as it is typed, on the chip's
        // own face: no bezel, no ring, no ground of its own.
        field.setBezeled(false);
        field.setDrawsBackground(false);
        field.setFocusRingType(NSFocusRingType::None);
        field.setAlignment(NSTextAlignment::Center);
        if let Some(cell) = field.cell() {
            cell.setUsesSingleLineMode(true);
            cell.setScrollable(true);
        }
        field.setAccessibilityLabel(Some(ns_string!("Tab name")));
        let this = Self::alloc(mtm).set_ivars(BarIvars {
            window,
            shown: RefCell::new(Shown::default()),
            hovered: Cell::new(None),
            drag: Cell::new(None),
            gap: Cell::new(None),
            glide: Cell::new(false),
            palette: Cell::new(None),
            theme: Cell::new(None),
            chips: RefCell::new(Vec::new()),
            ends: RefCell::new(Vec::new()),
            strip: strip.clone(),
            fade_leading: fade_leading.clone(),
            fade_trailing: fade_trailing.clone(),
            scroll: Cell::new(0.0),
            laid: RefCell::new(None),
            revealed: Cell::new(None),
            overflowed: Cell::new(false),
            new_tab: new_tab.clone(),
            list: list.clone(),
            warning: warning.clone(),
            field: field.clone(),
            renaming: Cell::new(None),
            opened_with: RefCell::new(String::new()),
            width: Cell::new(0.0),
            clock: Cell::new(0),
            clock_due: Cell::new(None),
            card_view: SummaryCard::new(mtm),
            card: RefCell::new(None),
            card_wait: Cell::new(0),
            card_closed: Cell::new(None),
            card_quiet: Cell::new(None),
            card_inside: Cell::new(false),
            card_at: Cell::new(None),
            card_out: Cell::new(0),
            hints: Cell::new(false),
            hint_wait: Cell::new(0),
            hint_pending: Cell::new(false),
            headers: RefCell::new(Vec::new()),
            renaming_group: Cell::new(None),
            renaming_born: Cell::new(false),
            switch: switch.clone(),
            group_list: tab_switch::GroupList::new(mtm, window),
            scrim: tab_switch::ListScrim::new(mtm, window),
            list_open: Cell::new(false),
            group_hints: Cell::new(false),
            group_wait: Cell::new(0),
            group_pending: Cell::new(false),
            picks: RefCell::new(Picks::default()),
            pick_watch: RefCell::new(None),
            target: Cell::new(None),
            dwelling: Cell::new(None),
            mark: Cell::new(None),
            insertion: tab_side::Insertion::default(),
            cast: Cell::new(None),
            group_torn: Cell::new(None),
            side: Cell::new(None),
            column: RefCell::new(None),
            mask: tab_side::Mask::default(),
            before: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        // Under everything: the title row is the bar's, not the window server's.
        this.addSubview(&RowClaim::new(mtm));
        this.addSubview(&new_tab);
        this.addSubview(&switch);
        this.addSubview(&list);
        this.addSubview(&warning);
        // The fades over the strip: what runs under an edge dissolves.
        this.addSubview(&strip);
        this.addSubview(&fade_leading);
        this.addSubview(&fade_trailing);
        list.setHidden(true);
        warning.setHidden(true);
        switch.setHidden(true);
        // SAFETY: `TabBar` implements the field's delegate protocols (the
        // two optional methods it uses are defined above); the field holds
        // its delegate weakly and the bar owns the field.
        unsafe { field.setDelegate(Some(ProtocolObject::from_ref(&*this))) };
        this.setAccessibilityElement(true);
        // SAFETY: AppKit's constant role string, alive for the process.
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityTabGroupRole }));
        this.setAccessibilityLabel(Some(ns_string!("Tabs")));
        // A tab carried out of another window's strip may be let go here.
        this.registerForDraggedTypes(&NSArray::from_slice(&[tab_drag::tab_type()]));
        this
    }

    fn terminal_window(&self) -> Option<Retained<TerminalWindow>> {
        app::delegate(self.mtm())?.window(self.ivars().window)
    }

    /// Whether a bar of `count` tabs is just the window's title: one tab in the title row. It
    /// has no menu, no card, no ring and no `×`. The side column has no title bar, so there a
    /// lone tab is a row like the rest, and so is the one tab of a group: the group button leads
    /// the bar then and the tab is a chip beside it.
    fn lone_title(&self, count: usize) -> bool {
        let iv = self.ivars();
        count < 2 && iv.side.get().is_none() && iv.shown.borrow().switcher.is_none()
    }

    /// Reduce Motion now — the setting and the system's answer, one `bool`.
    fn reduce_motion(&self) -> bool {
        app::delegate(self.mtm()).is_some_and(|app| app.reduce_motion())
    }

    /// What to show: the tabs in order, the selected one's position and the
    /// settings diagnostic (empty without one). An open card reads its tab
    /// again (`TerminalTab::card`) when what its chip shows changed — a
    /// command started or ended, a title, a transfer's percent, a mark: the
    /// read is a `Term` round, and a title news can come often while output
    /// streams. A card with a map of panes reads at every news: what a pane reports
    /// can change while the chip, which shows the most urgent of them, does
    /// not. Lays out.
    pub(crate) fn show(
        &self,
        tabs: Vec<Label>,
        selected: usize,
        headings: Vec<Heading>,
        switcher: Option<Switcher>,
        notice: String,
    ) {
        let iv = self.ivars();
        let open = iv.card.borrow().as_ref().map(|open| open.tab);
        let told = |shown: &Shown, tab: u64| {
            shown
                .tabs
                .iter()
                .find(|label| label.tab == tab)
                .map(|label| {
                    (
                        label.title.clone(),
                        label.indicator,
                        label.running.is_some(),
                        label.upload,
                        label.mark,
                    )
                })
        };
        let before = open.and_then(|tab| told(&iv.shown.borrow(), tab));
        let listed = iv.list_open.get().then(|| self.listing());
        let kept = kept_picks(&iv.picks.borrow(), &tabs, selected, self.is_side());
        self.store_picks(kept);
        iv.shown.replace(Shown {
            tabs,
            headings,
            switcher,
            selected,
            notice,
        });
        self.refresh_group_list(listed);
        let mapped = iv.card.borrow().as_ref().is_some_and(OpenCard::pictured);
        if let Some(tab) = open
            && (mapped || before != told(&iv.shown.borrow(), tab))
        {
            self.read_card(tab);
            self.draw_card();
            self.shoot_missing();
        }
        self.lay_out();
    }

    /// The transfer of tab `tab` ended as `finish` says. Told once, **before** the pane tells the
    /// title that nothing flows (the tab's label still says it does until then): the layout that
    /// finds nothing flowing in the tab any more plays the end on the tab's chip
    /// ([`ChipFill`]), and the layouts after it do not. The window lays out
    /// (`TerminalWindow::transfer_ended`).
    pub(crate) fn transfer_ended(&self, tab: u64, finish: Finish) {
        self.ivars()
            .ends
            .borrow_mut()
            .push((tab, finish, Instant::now()));
    }

    /// The settings diagnostic alone changed.
    pub(crate) fn set_notice(&self, notice: String) {
        self.ivars().shown.borrow_mut().notice = notice;
        self.lay_out();
    }

    /// The theme the bar wears: the window's (the bar is the first thing painted with it). The
    /// embedded base before any is set.
    pub(crate) fn theme(&self) -> Theme {
        self.ivars().theme.get().unwrap_or(Theme::BATERI)
    }

    /// The theme changed: every part repaints in its roles.
    pub(crate) fn set_theme(&self, theme: &Theme) {
        self.ivars().palette.set(Some(Palette::of(theme)));
        self.ivars().theme.set(Some(*theme));
        self.lay_out();
        self.draw_card();
    }

    /// Where the strip may start: past the zoom button, or at the gap alone
    /// in full screen, where the lights live in another window.
    fn leading(&self) -> f64 {
        let Some(window) = self.window() else {
            return LIGHTS_GAP;
        };
        if window.styleMask().contains(NSWindowStyleMask::FullScreen) {
            return LIGHTS_GAP;
        }
        window
            .standardWindowButton(NSWindowButton::ZoomButton)
            .map_or(LIGHTS_GAP, |zoom| {
                let in_window = zoom.convertRect_toView(zoom.bounds(), None);
                let here = self.convertRect_fromView(in_window, None);
                here.origin.x + here.size.width + LIGHTS_GAP
            })
    }

    /// The radius of the window's top-right corner: square in full screen,
    /// [`WINDOW_CORNER`] otherwise. The window lays the bar out again when
    /// it enters and leaves full screen, so `+` follows the corner.
    fn corner(&self) -> f64 {
        match self.window() {
            Some(window) if window.styleMask().contains(NSWindowStyleMask::FullScreen) => 0.0,
            _ => WINDOW_CORNER,
        }
    }

    /// One chip per label, in its order: a tab keeps its chip, a new tab
    /// gets a new one and a closed tab's goes. `true` when the list changed
    /// and there were chips before — the applier's change, which slides.
    fn chips_for(&self, tabs: &[Label]) -> (Vec<Retained<Chip>>, bool) {
        let iv = self.ivars();
        let mut old = iv.chips.take();
        let before: Vec<u64> = old.iter().map(|chip| chip.tab()).collect();
        let mut chips = Vec::with_capacity(tabs.len());
        for label in tabs {
            match old.iter().position(|chip| chip.tab() == label.tab) {
                Some(at) => chips.push(old.remove(at)),
                None => {
                    let chip = Chip::new(self.mtm(), label.tab);
                    iv.strip.addSubview(&chip);
                    chips.push(chip);
                }
            }
        }
        for gone in old {
            gone.removeFromSuperview();
        }
        let changed = !before.is_empty()
            && !before
                .iter()
                .copied()
                .eq(tabs.iter().map(|label| label.tab));
        iv.chips.replace(chips.clone());
        (chips, changed)
    }

    /// The group button's width, in whole points; `None` where the bar has no such button — in
    /// the column, and while no group exists. It follows the group's name, or what is typed in
    /// its place ([`tabs::group_button_width`]).
    fn switch_width(&self) -> Option<f64> {
        let iv = self.ivars();
        if iv.side.get().is_some() {
            return None;
        }
        let shown = iv.shown.borrow();
        let switcher = shown.switcher.as_ref()?;
        let naming = switcher.shown.is_some() && iv.renaming_group.get() == switcher.shown;
        let name = match (&switcher.name, naming) {
            (None, _) => None,
            (Some(_), true) => Some(iv.switch.text_width(&iv.field.stringValue().to_string())),
            (Some(name), false) => Some(iv.switch.text_width(name)),
        };
        Some(tabs::group_button_width(name, naming))
    }

    /// What the group button takes of the strip's room: its width and the space after it.
    fn switch_lead(&self) -> f64 {
        self.switch_width()
            .map_or(0.0, |width| width + tabs::GROUP_BUTTON_GAP)
    }

    /// The strip of `bar`: a lone tab is the window's title, unless a group button leads the bar,
    /// when it is a chip ([`Bar::layout_chips_with`]).
    fn strip_for(&self, bar: &Bar, glowing: impl Fn(usize) -> bool) -> Strip {
        if self.switch_width().is_some() {
            bar.layout_chips_with(glowing)
        } else {
            bar.layout_with(glowing)
        }
    }

    /// Puts the group button where the strip starts, with the look its group gives it, and the
    /// name field in its place while the group is being named; hides it where there is none.
    /// Returns how long the button takes to come to its new width (`secs`, the strip's reflow):
    /// another group's name widens or narrows it, and the strip after it goes along over the
    /// same time instead of jumping — at once while a name is typed, the first time it shows,
    /// and under Reduce Motion (`secs` is zero then).
    fn place_switch(&self, palette: Palette, top: f64, secs: f64) -> f64 {
        let iv = self.ivars();
        let (Some(width), Some(switcher)) =
            (self.switch_width(), iv.shown.borrow().switcher.clone())
        else {
            iv.switch.setHidden(true);
            return 0.0;
        };
        let naming = switcher.shown.is_some() && iv.renaming_group.get() == switcher.shown;
        let frame = NSRect::new(
            NSPoint::new(self.leading(), top),
            NSSize::new(width, CHIP_HEIGHT),
        );
        let shown = !iv.switch.isHidden();
        let resized = (iv.switch.frame().size.width - width).abs() > 0.5;
        let slide = if shown && resized && !naming {
            secs
        } else {
            0.0
        };
        iv.switch.setHidden(false);
        iv.switch.set(
            frame,
            slide,
            &tab_switch::ButtonLook {
                group: switcher.shown,
                name: switcher.name,
                open: iv.list_open.get(),
                naming,
                elsewhere: switcher.elsewhere,
            },
            palette,
        );
        if naming {
            // Where the name stands, a little wider than the text so the caret has room; the
            // button's own fill is the field's ground.
            let field = &iv.field;
            field.setAlignment(NSTextAlignment::Left);
            self.dress_field(palette, false);
            // SAFETY: AppKit's constant font weight.
            let weight = unsafe { NSFontWeightSemibold };
            field.setFont(Some(&NSFont::systemFontOfSize_weight(
                tab_switch::NAME_TEXT,
                weight,
            )));
            let height = field.intrinsicContentSize().height;
            let (x, width) = tab_switch::name_span(frame.size.width);
            field.setFrame(NSRect::new(
                NSPoint::new(
                    frame.origin.x + x,
                    top + ((CHIP_HEIGHT - height) / 2.0).max(0.0),
                ),
                NSSize::new(width, height),
            ));
        }
        slide
    }

    /// The pure layout's input for `count` places: the bar's width, where the strip may start,
    /// the settings diagnostic and the corner, and the scroll as it stands.
    fn bar_for(
        &self,
        count: usize,
        selected: usize,
        hovered: Option<usize>,
        dragged: Option<usize>,
    ) -> Bar {
        let iv = self.ivars();
        Bar {
            width: self.bounds().size.width,
            leading: self.leading() + self.switch_lead(),
            count,
            selected,
            hovered,
            dragged,
            scroll: iv.scroll.get(),
            warning: !iv.shown.borrow().notice.is_empty(),
            corner: self.corner(),
        }
    }

    /// Places every part from the pure layout ([`Bar::layout`]) and gives
    /// the chips their tabs; then the card follows its chip and the clock is
    /// set again ([`Self::arm_clock`]).
    ///
    /// The strip scrolls by [`BarIvars::scroll`]. A **change of the selection**
    /// scrolls it to the selected chip (`Strip::revealing`) and, like the
    /// applier's changes, slides there; nothing else moves it but the wheel,
    /// which does not slide.
    ///
    /// **Two things in flight change what is laid out, neither of them the window's list.** A tab
    /// dragged along the strip (or down the column) is laid out at the place it would take, and
    /// its chip at the pointer, not at its place ([`Dragging::at`]); the others slide around it
    /// the way they slide
    /// for any change of order, so a drag has no animation path of its own. A tab carried over
    /// from another window opens a gap at its place ([`BarIvars::gap`]): the layout has one more
    /// place and the chips from the gap on take the next. The list itself changes once, when the
    /// tab is let go, through the window's applier.
    pub(crate) fn lay_out(&self) {
        let iv = self.ivars();
        let (Some(palette), Some(theme)) = (iv.palette.get(), iv.theme.get()) else {
            return;
        };
        let mut shown = iv.shown.borrow().clone();
        let count = shown.tabs.len();
        if count == 0 {
            return;
        }
        // A tab gone takes its name field away, and its drag; so does a group gone, or a column
        // gone that held the group's header.
        if let Some(tab) = iv.renaming.get()
            && !shown.tabs.iter().any(|label| label.tab == tab)
        {
            self.end_rename(false);
            return;
        }
        if let Some(group) = iv.renaming_group.get() {
            let there = if iv.side.get().is_some() {
                shown.headings.iter().any(|heading| heading.group == group)
            } else {
                shown
                    .switcher
                    .as_ref()
                    .is_some_and(|switcher| switcher.shown == Some(group))
            };
            if !there {
                self.end_rename(false);
                return;
            }
        }
        let side = iv.side.get();
        let drag = iv
            .drag
            .get()
            .filter(|drag| shown.tabs.iter().any(|label| label.tab == drag.tab));
        iv.drag.set(drag);
        // The tabs in the order the strip shows them now: the dragged one at its place — in the
        // column of a window with groups, with each header over the row it would stand over.
        // A row lit for a split leaves the others where they stood: the held row hovers over the
        // lit one, not in a place of the list.
        let lit = drag.is_some_and(|drag| drag.split.is_some());
        if let Some(Dragging {
            tab,
            at: Some((_, to)),
            torn: false,
            grouped,
            ..
        }) = drag
            && !lit
        {
            let selected = shown.tabs.get(shown.selected).map(|label| label.tab);
            if grouped {
                let preview = self
                    .terminal_window()
                    .and_then(|window| window.side_preview(tab, to));
                if let Some(preview) = preview {
                    let rows: Vec<Label> = preview
                        .rows
                        .iter()
                        .filter_map(|tab| shown.tabs.iter().find(|label| label.tab == *tab))
                        .cloned()
                        .collect();
                    // A list that is not the window's now (it changed under the button) is left.
                    if rows.len() == shown.tabs.len() {
                        shown.tabs = rows;
                        for heading in &mut shown.headings {
                            if let Some(&(_, first)) = preview
                                .firsts
                                .iter()
                                .find(|(group, _)| *group == heading.group)
                            {
                                heading.first_row = first;
                            }
                        }
                    }
                }
            } else if let Some(from) = shown.tabs.iter().position(|label| label.tab == tab) {
                tabs::reordered(&mut shown.tabs, from, to);
            }
            shown.selected = shown
                .tabs
                .iter()
                .position(|label| Some(label.tab) == selected)
                .unwrap_or(0);
        }
        let position = |tab: u64| shown.tabs.iter().position(|label| label.tab == tab);
        let dragged = drag.and_then(|drag| position(drag.tab));
        let hovered = iv.hovered.get().and_then(position);
        let gap = iv.gap.get().map(|gap| gap.min(count));
        // The lone tab is today's title bar; with a tab on its way in it is the first chip.
        let single = self.lone_title(count) && gap.is_none();
        // The layout's place for the tab at `index`: past the gap, one on.
        let seat = |index: usize| tabs::seat(index, gap);
        let (chips, reflow) = self.chips_for(&shown.tabs);
        let reduce = self.reduce_motion();
        // The ends told play now, each once: a tab with nothing flowing in it plays its fill's
        // end, and while it rests it is the tab's ground — the separators and the title know it
        // from here on, and this layout is asked for again where the rest is over. A tab that
        // still shows a transfer flowing keeps the end for the layout that finds it gone (the
        // pane tells the title a moment after the end), and drops it if that does not come: the
        // flow is another pane's and the fill goes on.
        let now = Instant::now();
        let mut waiting = Vec::new();
        for (tab, finish, told) in iv.ends.take() {
            let Some(index) = position(tab) else {
                continue;
            };
            if now.duration_since(told) > END_WAIT {
                continue;
            }
            if shown.tabs[index].upload.is_some() {
                waiting.push((tab, finish, told));
                continue;
            }
            let lit = chips[index].fill_end(finish, reduce);
            if lit > 0.0 {
                // A moment past the rest, so the fill is no longer the ground when it comes.
                after(
                    Duration::from_secs_f64(lit + 0.02),
                    iv.window,
                    TabBar::lay_out,
                );
            }
        }
        iv.ends.replace(waiting);
        let glowing: Vec<bool> = chips
            .iter()
            .zip(&shown.tabs)
            .map(|(chip, label)| !single && (label.upload.is_some() || chip.fill_lit(now)))
            .collect();
        // The fill is lit by the layout's places, which are the gap's too.
        let seats: Vec<bool> = {
            let mut seats = vec![false; count + usize::from(gap.is_some())];
            for (index, &lit) in glowing.iter().enumerate() {
                seats[seat(index)] = lit;
            }
            seats
        };
        if let Some(top) = side {
            iv.switch.setHidden(true);
            let flight = tab_side::Flight { drag, gap };
            self.lay_out_column(top, &shown, &chips, &glowing, reflow, flight);
            return;
        }
        // The group button leads the strip, beside the lights.
        let switched = self.place_switch(
            palette,
            ((self.bounds().size.height - CHIP_HEIGHT) / 2.0).max(0.0),
            Motion::of(reduce).reflow,
        );
        let lit_at = |place: usize| seats.get(place).copied().unwrap_or(false);
        let bar = self.bar_for(
            count + usize::from(gap.is_some()),
            seat(shown.selected),
            hovered.map(seat),
            dragged.map(seat),
        );
        let mut strip = self.strip_for(&bar, lit_at);
        let selected_tab = shown.tabs.get(shown.selected).map(|label| label.tab);
        let mut revealed = false;
        let was_overflowing = iv.overflowed.replace(strip.overflow);
        let resized = (iv.width.replace(bar.width) - bar.width).abs() > 0.5;
        if single {
            iv.revealed.set(None);
        } else {
            let selection_changed = iv.revealed.replace(selected_tab) != selected_tab;
            // The tabs stopped fitting, or the room they scroll in changed
            // while they do not: the selected one stays in sight. Only a
            // change of the selection slides there.
            let kept_in_sight = strip.overflow && (!was_overflowing || resized);
            if selection_changed || kept_in_sight {
                let target = strip.revealing(seat(shown.selected));
                if target != strip.scroll {
                    strip = self.strip_for(
                        &Bar {
                            scroll: target,
                            ..bar
                        },
                        lit_at,
                    );
                    revealed = selection_changed;
                }
            }
        }
        iv.scroll.set(strip.scroll);
        let bounds = self.bounds();
        let top = ((bounds.size.height - CHIP_HEIGHT) / 2.0).max(0.0);
        // The chips live in the strip's space: its left edge is theirs, and it goes along with a
        // group button that is coming to another name's width.
        let span = NSRect::new(
            NSPoint::new(strip.span.x, 0.0),
            NSSize::new(strip.span.width, bounds.size.height),
        );
        if switched > 0.0 && iv.strip.frame() != span {
            let strip = iv.strip.retain();
            animate(switched, move || strip.animator().setFrame(span));
        } else if iv.strip.frame() != span {
            // A slide still on its way would carry on to its old place and snap back.
            if let Some(layer) = iv.strip.layer() {
                layer.removeAllAnimations();
            }
            iv.strip.setFrame(span);
        }
        // A window nobody sees turns no ring: the turns are off and come back at the right angle.
        let visible = self
            .terminal_window()
            .is_some_and(|window| window.window_visible());
        let motion = Motion::of(reduce);
        let style = IconStyle::current(self.mtm());
        let glide = iv.glide.take();
        let slide = ((reflow || revealed || glide) && motion.reflow > 0.0).then_some(motion.reflow);
        let seen = Seen {
            theme,
            count,
            visible,
            reduce,
            style,
            hints: iv.hints.get(),
        };
        for (index, chip) in chips.iter().enumerate() {
            let label = &shown.tabs[index];
            let span = strip.chips[seat(index)];
            // The tab being named shows its field instead of title and indicator.
            let naming = iv.renaming.get() == Some(label.tab);
            let title = if naming {
                String::new()
            } else if single {
                single_label(&label.title, &shown.notice)
            } else {
                label.title.clone()
            };
            // A tab dragged along the strip is at the pointer, at once; the rest slide.
            let carried = drag
                .filter(|drag| !drag.torn && dragged == Some(index))
                .and_then(|drag| drag.at);
            let frame = NSRect::new(
                NSPoint::new(carried.map_or(span.x, |(left, _)| left) - strip.span.x, top),
                NSSize::new(span.width, CHIP_HEIGHT),
            );
            let look = seen.look(
                label,
                &title,
                Standing {
                    index,
                    naming,
                    selected: index == shown.selected,
                    hovered: (hovered == Some(index)
                        || carried.is_some()
                        || iv.target.get() == Some(label.tab))
                        && !naming,
                    picked: false,
                    glowing: glowing[index],
                    single,
                    lifted: carried.is_some(),
                    ghost: drag.is_some_and(|drag| drag.torn && dragged == Some(index)),
                    side: false,
                },
            );
            chip.set(
                frame,
                &look,
                palette,
                motion,
                slide.filter(|_| carried.is_none()),
            );
            if naming {
                self.place_field(span, top, palette, single);
            }
        }
        let inside = |x: f64| x - strip.span.x;
        iv.strip.set_separators(
            strip.separators.iter().map(|&x| inside(x)).collect(),
            palette,
        );
        let height = bounds.size.height;
        let edge = |x: f64| NSRect::new(NSPoint::new(x, 0.0), NSSize::new(FADE, height));
        for (fade, on, x) in [
            (&iv.fade_leading, strip.fade_leading, strip.span.x),
            (
                &iv.fade_trailing,
                strip.fade_trailing,
                strip.span.end() - FADE,
            ),
        ] {
            fade.setFrame(edge(x));
            fade.set_palette(palette);
            fade.setHidden(!on);
        }
        let button = |x: f64| NSRect::new(NSPoint::new(x, top), NSSize::new(BUTTON, BUTTON));
        iv.new_tab.setFrame(button(strip.new_tab));
        iv.new_tab.set_look(palette, strip.fit.radius);
        match strip.list {
            Some(x) => {
                iv.list.setFrame(button(x));
                iv.list.set_look(palette, strip.fit.radius);
                iv.list.setHidden(false);
            }
            None => iv.list.setHidden(true),
        }
        match strip.warning {
            Some(x) => {
                iv.warning.setFrame(button(x));
                iv.warning.set_look(palette, strip.fit.radius);
                let text = NSString::from_str(&shown.notice);
                iv.warning.setToolTip(Some(&text));
                iv.warning.setAccessibilityLabel(Some(&text));
                iv.warning.setHidden(false);
            }
            None => iv.warning.setHidden(true),
        }
        iv.laid.replace(Some(strip));
        self.place_card();
        self.arm_clock();
    }

    /// A wheel or trackpad step over the bar ([`Strip::wheeled`]); `false`
    /// when nothing scrolls (the tabs fit) and the event is not the bar's.
    fn wheel(&self, event: &NSEvent) -> bool {
        let iv = self.ivars();
        // A trackpad reports points, a wheel lines. The strip moves the way
        // the fingers (or the wheel) take the content: positive towards the
        // later tabs, the opposite of AppKit's delta.
        let unit = if event.hasPreciseScrollingDeltas() {
            1.0
        } else {
            WHEEL_STEP
        };
        let step = if iv.side.get().is_some() {
            // An upright list scrolls on the vertical axis alone.
            let column = iv.column.borrow();
            let Some(column) = column.as_ref().filter(|column| column.overflow) else {
                return false;
            };
            let to = column.wheeled(-event.scrollingDeltaY() * unit);
            (to != column.scroll).then_some(to)
        } else {
            let laid = iv.laid.borrow();
            let Some(laid) = laid.as_ref().filter(|laid| laid.overflow) else {
                return false;
            };
            let to = laid.wheeled(
                -event.scrollingDeltaX() * unit,
                -event.scrollingDeltaY() * unit,
            );
            (to != laid.scroll).then_some(to)
        };
        if let Some(to) = step {
            iv.scroll.set(to);
            self.lay_out();
        }
        true
    }

    // ─── Naming a tab ────────────────────────────────────────────────────

    /// Opens the name field over tab `tab`'s chip, its title selected: a
    /// double click, Rename Tab…. A lone tab's title is the window's and is
    /// named from the menu only (the title takes no click). A name under way
    /// elsewhere is kept first, and the chip is brought into view.
    pub(crate) fn begin_rename(&self, tab: u64) {
        let iv = self.ivars();
        let (index, shown_title) = {
            let shown = iv.shown.borrow();
            let Some(index) = shown.tabs.iter().position(|label| label.tab == tab) else {
                return;
            };
            (index, shown.tabs[index].title.clone())
        };
        // The window's title carries a transfer's prefix; a name does not.
        let title = self
            .terminal_window()
            .and_then(|window| window.tab_title(tab))
            .unwrap_or(shown_title);
        if iv.renaming.get() == Some(tab) {
            return;
        }
        self.end_rename(true);
        self.pressed(tab);
        let to = if iv.side.get().is_some() {
            iv.column
                .borrow()
                .as_ref()
                .map(|column| column.revealing(index))
        } else {
            iv.laid
                .borrow()
                .as_ref()
                .map(|laid| laid.revealing(index))
                .filter(|_| iv.shown.borrow().tabs.len() > 1)
        };
        if let Some(to) = to {
            iv.scroll.set(to);
        }
        iv.renaming.set(Some(tab));
        iv.opened_with.replace(title.clone());
        iv.field.setStringValue(&NSString::from_str(&title));
        self.addSubview(&iv.field);
        self.lay_out();
        // SAFETY: `selectText:` takes an optional sender, unused.
        unsafe { iv.field.selectText(None) };
    }

    /// Opens the name field over group `group`'s header, its name selected: a double click on the
    /// header, Rename Group…, a group just made. A name under way elsewhere is kept first, and
    /// the header is brought into view. In the title row there is no header: the field opens in
    /// the group button, over its name, and only for the group on screen.
    pub(crate) fn begin_group_rename(&self, group: GroupId, born: bool) {
        let iv = self.ivars();
        if iv.renaming_group.get() == Some(group) {
            return;
        }
        // In the column the header, in the title row the group button: the group on screen.
        let named = {
            let shown = iv.shown.borrow();
            if iv.side.get().is_some() {
                shown
                    .headings
                    .iter()
                    .find(|heading| heading.group == group)
                    .map(|heading| (heading.name.clone(), Some(heading.first_row)))
            } else {
                shown
                    .switcher
                    .as_ref()
                    .filter(|switcher| switcher.shown == Some(group))
                    .and_then(|switcher| switcher.name.clone())
                    .map(|name| (name, None))
            }
        };
        let Some((name, first_row)) = named else {
            return;
        };
        self.end_rename(true);
        // The row below the header comes into sight with it; a closed group with none shows the
        // header where it is.
        let to = first_row.and_then(|first_row| {
            iv.column.borrow().as_ref().and_then(|column| {
                (first_row < column.rows.len()).then(|| column.revealing(first_row))
            })
        });
        if let Some(to) = to {
            iv.scroll.set(to);
        }
        iv.renaming_group.set(Some(group));
        iv.renaming_born.set(born);
        iv.opened_with.replace(name.clone());
        iv.field.setStringValue(&NSString::from_str(&name));
        self.addSubview(&iv.field);
        self.lay_out();
        // SAFETY: `selectText:` takes an optional sender, unused.
        unsafe { iv.field.selectText(None) };
    }

    /// Takes the name field away — Return and a lost keyboard keep what was
    /// typed (`commit`), Esc leaves the tab as it was. The window's applier
    /// decides what the text means ([`TerminalWindow::rename_tab`]).
    fn end_rename(&self, commit: bool) {
        let iv = self.ivars();
        // A group's name is the field's too: the window's applier decides what it comes to.
        if let Some(group) = iv.renaming_group.take() {
            let draft = iv.field.stringValue().to_string();
            let commit = commit && draft != *iv.opened_with.borrow();
            let born = iv.renaming_born.take();
            let window = self.terminal_window();
            if iv.field.currentEditor().is_some()
                && let Some(window) = &window
            {
                window.focus_selected_tab();
            }
            iv.field.removeFromSuperview();
            if commit && let Some(window) = &window {
                if born {
                    window.name_new_group(group, &draft);
                } else {
                    window.rename_group(group, &draft);
                }
            }
            self.lay_out();
            return;
        }
        // Taken first: handing the keyboard back ends the field's editing,
        // which says so again ([`TabBar`]'s `controlTextDidEndEditing:`).
        let Some(tab) = iv.renaming.take() else {
            return;
        };
        let draft = iv.field.stringValue().to_string();
        // Text nobody changed is not a name: the title it was opened with may
        // be stale by now (a shell that rewrites it on every command).
        let commit = commit && draft != *iv.opened_with.borrow();
        let window = self.terminal_window();
        if iv.field.currentEditor().is_some()
            && let Some(window) = &window
        {
            window.focus_selected_tab();
        }
        iv.field.removeFromSuperview();
        if commit && let Some(window) = &window {
            window.rename_tab(tab, &draft);
        }
        self.lay_out();
    }

    /// Puts the name field over `chip` (the bar's space), in the type the
    /// title wears there: the selected chip's, or the lone tab's.
    fn place_field(&self, chip: tabs::Span, top: f64, palette: Palette, single: bool) {
        let field = &self.ivars().field;
        field.setAlignment(NSTextAlignment::Center);
        let pad = self.dress_field(palette, single);
        let height = field.intrinsicContentSize().height;
        field.setFrame(NSRect::new(
            NSPoint::new(chip.x + pad, top + ((CHIP_HEIGHT - height) / 2.0).max(0.0)),
            NSSize::new((chip.width - 2.0 * pad).max(0.0), height),
        ));
    }

    /// The type the name field wears — the title's, selected or lone — and the padding the
    /// title keeps from a chip's sides.
    fn dress_field(&self, palette: Palette, single: bool) -> f64 {
        let field = &self.ivars().field;
        // SAFETY: AppKit's constant font weight.
        let weight = unsafe { NSFontWeightSemibold };
        let (size, color, pad) = if single {
            (SINGLE_TEXT, palette.dim, SINGLE_PAD)
        } else {
            (CHIP_TEXT, palette.title, CHIP_PAD)
        };
        field.setFont(Some(&NSFont::systemFontOfSize_weight(size, weight)));
        field.setTextColor(Some(&Tint::of(color, 1.0).color()));
        pad
    }

    // ─── The menus ───────────────────────────────────────────────────────

    /// The chip's menu for tab `tab` — what can be done to that tab whether
    /// or not it is selected. Every item names it in its `tag`
    /// ([`tabs::menu_tag`]) and acts through the window's applier.
    fn context_menu(&self, tab: u64) -> Option<Retained<NSMenu>> {
        if self.lone_title(self.ivars().shown.borrow().tabs.len()) {
            return None;
        }
        self.tab_menu(tab)
    }

    /// [`Self::context_menu`]'s body, which the single tab's title gives too: its press is the
    /// row's, so its menu comes from the bar ([`TabBar`]'s `menuForEvent:`), not from the chip.
    fn tab_menu(&self, tab: u64) -> Option<Retained<NSMenu>> {
        let window = self.terminal_window()?;
        // A menu is no place for the card; and the pointer resting on the
        // chip after it closes is not a reason to open it.
        self.pressed(tab);
        // A row among the ones picked together speaks for all of them.
        let picks = self.picks();
        if picks.contains(&tab) {
            return Some(self.picks_menu(&window, &picks));
        }
        let mtm = self.mtm();
        let menu = NSMenu::new(mtm);
        let tag = tabs::menu_tag(tab);
        let add = |title: &str, action: Sel| {
            menu.addItem(&window_item(mtm, &window, title, action, tag));
        };
        add("Close Tab", sel!(closeChipTab:));
        add("Close Other Tabs", sel!(closeOtherTabs:));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        add("Move Tab to New Window", sel!(detachTab:));
        // The tab on screen is the one it would join: its own chip has
        // nothing to merge into.
        if !window.is_selected(tab) {
            let merge = NSMenu::new(mtm);
            for (title, action) in [
                ("Split Right", sel!(mergeTabRight:)),
                ("Split Down", sel!(mergeTabDown:)),
            ] {
                merge.addItem(&window_item(mtm, &window, title, action, tag));
            }
            let holder = NSMenuItem::new(mtm);
            holder.setTitle(&NSString::from_str("Merge into Current Tab"));
            holder.setSubmenu(Some(&merge));
            menu.addItem(&holder);
        }
        add("Rename Tab\u{2026}", sel!(renameTab:));
        self.add_group_items(&menu, &window, tab);
        Some(menu)
    }

    /// The menu of the tabs picked together, on a right click on one of them: what can be done
    /// to all of them at once — into a group, out of it, to a window of their own, closed. The
    /// items act on the picks as they stand when one is chosen ([`TabBar::picks`]); the picks
    /// are of one cluster, so the first one's group is theirs ([`TerminalWindow::group_targets`]).
    fn picks_menu(&self, window: &TerminalWindow, picks: &[u64]) -> Retained<NSMenu> {
        let mtm = self.mtm();
        let menu = NSMenu::new(mtm);
        let count = picks.len();
        let targets = window.group_targets(picks.first().copied());
        if targets.is_empty() {
            menu.addItem(&window_item(
                mtm,
                window,
                &counted("Move", count, " to New Group"),
                sel!(movePickedToNewGroup:),
                0,
            ));
        } else {
            let submenu = NSMenu::new(mtm);
            for (group, name, _) in &targets {
                submenu.addItem(&window_item(
                    mtm,
                    window,
                    name,
                    sel!(movePickedToGroup:),
                    *group,
                ));
            }
            submenu.addItem(&NSMenuItem::separatorItem(mtm));
            submenu.addItem(&window_item(
                mtm,
                window,
                "New Group\u{2026}",
                sel!(movePickedToNewGroup:),
                0,
            ));
            let holder = NSMenuItem::new(mtm);
            holder.setTitle(&NSString::from_str(&counted("Move", count, " to Group")));
            holder.setSubmenu(Some(&submenu));
            menu.addItem(&holder);
        }
        if targets.iter().any(|(_, _, here)| *here) {
            menu.addItem(&window_item(
                mtm,
                window,
                &counted("Remove", count, " from Group"),
                sel!(removePickedFromGroup:),
                0,
            ));
        }
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&window_item(
            mtm,
            window,
            &counted("Move", count, " to New Window"),
            sel!(detachPicked:),
            0,
        ));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let close = window_item(
            mtm,
            window,
            &counted("Close", count, "\u{2026}"),
            sel!(closePicked:),
            0,
        );
        // The key that does it from the keyboard: ⌘W closes what is picked.
        close.setKeyEquivalent(ns_string!("w"));
        close.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
        menu.addItem(&close);
        menu
    }

    /// The chip menu's group items for tab `tab`: Move to New Group while no group exists, else
    /// Move to Group ▸ (the groups, the tab's own ticked and grey, and New Group…), and Remove
    /// from Group for a grouped tab. The submenu's group items name their group by `tag`
    /// ([`TerminalWindow::group_targets`]); the tab is the one its parent item names.
    fn add_group_items(&self, menu: &NSMenu, window: &TerminalWindow, tab: u64) {
        let mtm = self.mtm();
        let tag = tabs::menu_tag(tab);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let targets = window.group_targets(Some(tab));
        if targets.is_empty() {
            menu.addItem(&window_item(
                mtm,
                window,
                "Move to New Group",
                sel!(moveTabToNewGroup:),
                tag,
            ));
            return;
        }
        let submenu = NSMenu::new(mtm);
        for (group, name, _) in &targets {
            submenu.addItem(&window_item(
                mtm,
                window,
                name,
                sel!(moveTabToGroup:),
                *group,
            ));
        }
        submenu.addItem(&NSMenuItem::separatorItem(mtm));
        submenu.addItem(&window_item(
            mtm,
            window,
            "New Group\u{2026}",
            sel!(moveTabToNewGroup:),
            tag,
        ));
        let holder = NSMenuItem::new(mtm);
        holder.setTitle(&NSString::from_str("Move to Group"));
        holder.setTag(tag);
        holder.setSubmenu(Some(&submenu));
        menu.addItem(&holder);
        if targets.iter().any(|(_, _, here)| *here) {
            menu.addItem(&window_item(
                mtm,
                window,
                "Remove from Group",
                sel!(removeTabFromGroup:),
                tag,
            ));
        }
    }

    /// A group header's menu — what can be done to that group: a tab in it, its name, its
    /// dissolving and its closing. Every item names the group in its `tag`.
    fn group_menu(&self, group: GroupId) -> Option<Retained<NSMenu>> {
        let window = self.terminal_window()?;
        // A menu is no place for the card.
        self.close_card();
        let mtm = self.mtm();
        let menu = NSMenu::new(mtm);
        let tag = isize::try_from(group.get()).unwrap_or(isize::MAX);
        let add = |title: &str, action: Sel| {
            menu.addItem(&window_item(mtm, &window, title, action, tag));
        };
        // The title row has ⌘T for it: a new tab opens in the group on screen.
        if self.is_side() {
            add("New Tab in Group", sel!(newTabInGroup:));
        }
        add("Rename Group\u{2026}", sel!(renameGroup:));
        add("Ungroup", sel!(ungroupGroup:));
        add("Move Group to New Window", sel!(moveGroupToNewWindow:));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        add("Close Group", sel!(closeGroup:));
        Some(menu)
    }

    /// Show All Tabs: every tab in a menu under the list button (or where it
    /// would be), the selected one ticked, a status dot in the colour of what
    /// the tab reports, its ⌘ key — a click selects it, the strip scrolls to
    /// it. Among several tabs only.
    pub(crate) fn show_list(&self) {
        let iv = self.ivars();
        // The list opens under its button; the column has none, and it opens at the list's head.
        let slot = if iv.side.get().is_some() {
            iv.column
                .borrow()
                .as_ref()
                .map(|column| column.list.x + tabs::EDGE)
        } else {
            iv.laid.borrow().as_ref().map(Strip::list_slot)
        };
        let (shown, palette) = (iv.shown.borrow().clone(), iv.palette.get());
        let (Some(window), Some(slot), Some(palette)) = (self.terminal_window(), slot, palette)
        else {
            return;
        };
        // Every tab, a closed group's hidden ones too; the keys are the listed tabs' own.
        let labels = window.all_labels();
        if labels.len() < 2 {
            return;
        }
        let on_screen = shown.tabs.get(shown.selected).map(|label| label.tab);
        self.close_card();
        let mtm = self.mtm();
        let style = IconStyle::current(mtm);
        let menu = NSMenu::new(mtm);
        for label in &labels {
            let item = window_item(
                mtm,
                &window,
                &label.title,
                sel!(pickTab:),
                tabs::menu_tag(label.tab),
            );
            if Some(label.tab) == on_screen {
                item.setState(NSControlStateValueOn);
            }
            // Every row has an image, so the titles start at one x: the tab's icon, or
            // without icons a status dot — a clear one where the tab reports nothing.
            item.setImage(Some(&list_image(label, style, palette)));
            let listed = shown.tabs.iter().position(|shown| shown.tab == label.tab);
            if let Some(digit) = listed.and_then(|at| tabs::shortcut_digit(at, shown.tabs.len())) {
                item.setKeyEquivalent(&NSString::from_str(&digit.to_string()));
                item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            }
            menu.addItem(&item);
        }
        let height = iv
            .column
            .borrow()
            .as_ref()
            .filter(|_| iv.side.get().is_some())
            .map_or(self.bounds().size.height, |column| column.list.y);
        let at = NSPoint::new(slot, height);
        menu.popUpMenuPositioningItem_atLocation_inView(None, at, Some(self));
    }

    /// The pointer came over tab `tab`'s chip (`Some`), or left the bar;
    /// the summary card follows it ([`Self::hover_card`]).
    fn hover(&self, tab: Option<u64>) {
        if self.ivars().hovered.replace(tab) != tab {
            self.lay_out();
            self.hover_card(tab);
        }
    }

    /// The pointer left tab `tab`'s chip — unless it already entered
    /// another, whose enter can come first.
    fn unhover(&self, tab: u64) {
        if self.ivars().hovered.get() == Some(tab) {
            self.hover(None);
        }
    }

    /// A chip was pressed: the window's applier selects it; `false` if it would not. Whatever
    /// was picked together lets go: a press that opens a tab is a plain one.
    fn select(&self, tab: u64) -> bool {
        self.drop_picks();
        self.terminal_window()
            .is_some_and(|window| window.select_tab(tab))
    }

    // ─── Picking tabs together ───────────────────────────────────────────

    /// The tabs picked together in the column, in its order; none while nothing is.
    pub(crate) fn picks(&self) -> Vec<u64> {
        self.ivars().picks.borrow().tabs.clone()
    }

    /// A press on tab `tab`'s row with ⇧ or ⌘ held: `true` if it picked (the tab on screen stays
    /// where it is and the press is no drag), `false` if it comes to a plain press — a press of
    /// another cluster than the picks', or one in the title row — which the caller goes on to
    /// handle as it always did. The rule is [`TerminalWindow::pick`]'s, [`crate::groups::Groups::pick`].
    fn pick(&self, tab: u64, click: Click) -> bool {
        if !self.is_side() {
            return false;
        }
        let Some(window) = self.terminal_window() else {
            return false;
        };
        let picked = window.pick(&self.picks(), click, tab);
        if picked.open.is_some() {
            return false;
        }
        let shown = window.try_selected_tab().map(|selected| selected.id());
        self.ivars().drag.set(None);
        self.store_picks(Picks {
            tabs: picked.picks,
            shown,
        });
        self.lay_out();
        true
    }

    /// Lets the picks go, if there are any.
    pub(crate) fn drop_picks(&self) {
        if !self.ivars().picks.borrow().tabs.is_empty() {
            self.store_picks(Picks::default());
            self.lay_out();
        }
    }

    /// Keeps `picks`, with the watch of presses there while something is picked. No layout: the
    /// callers have one coming or ask for it.
    fn store_picks(&self, picks: Picks) {
        let iv = self.ivars();
        let picked = !picks.tabs.is_empty();
        if iv.picks.replace(picks.clone()) == picks {
            return;
        }
        let watching = iv.pick_watch.borrow().is_some();
        if picked && !watching {
            let window = iv.window;
            let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
                // audit: a local monitor runs on the main thread, before `sendEvent:`.
                let mtm =
                    MainThreadMarker::new().expect("a local event monitor runs on the main thread");
                if let Some(window) = app::delegate(mtm).and_then(|app| app.window(window)) {
                    // SAFETY: AppKit gives the monitor a valid event.
                    window.bar().pressed_off_picks(unsafe { event.as_ref() });
                }
                event.as_ptr()
            });
            // SAFETY: the block returns the valid event it was given.
            let token = unsafe {
                NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                    NSEventMask::LeftMouseDown,
                    &block,
                )
            };
            iv.pick_watch.replace(token);
        } else if !picked && let Some(token) = iv.pick_watch.take() {
            // SAFETY: the token is the one `addLocalMonitor…` gave.
            unsafe { NSEvent::removeMonitor(&token) };
        }
    }

    /// A press of the window, as the watch sees it before the window does: one that lands off
    /// the column — in the terminal — lets the picks go, and goes on to its own work. A press in
    /// the column is the rows' and the headers' to take.
    fn pressed_off_picks(&self, event: &NSEvent) {
        let (Some(own), Some(from)) = (self.window(), event.window(self.mtm())) else {
            return;
        };
        if !std::ptr::eq(&*own, &*from) {
            return;
        }
        let at = self.convertPoint_fromView(event.locationInWindow(), None);
        if !contains(self.bounds(), at) {
            self.drop_picks();
        }
    }

    // ─── Dragging a tab ──────────────────────────────────────────────────

    /// The chip of tab `tab`.
    fn chip(&self, tab: u64) -> Option<Retained<Chip>> {
        self.ivars()
            .chips
            .borrow()
            .iter()
            .find(|chip| chip.tab() == tab)
            .cloned()
    }

    /// The tab whose chip is under `point` (the bar's space), if any — a chip scrolled out of
    /// the strip's sight is not under anything.
    fn tab_under(&self, point: NSPoint) -> Option<u64> {
        let iv = self.ivars();
        let local = self.convertPoint_toView(point, Some(&iv.strip));
        if !contains(iv.strip.bounds(), local) {
            return None;
        }
        let chips = iv.chips.borrow();
        chips
            .iter()
            .find(|chip| contains(chip.frame(), local))
            .map(|chip| chip.tab())
    }

    /// The strip as the window's list lays it out — without a drag's preview or a gap — with
    /// `room` more places than it has tabs: 0 is the strip as it is, 1 the strip a carried tab
    /// would make. Places are measured on the one that is on screen when the tab lets go
    /// ([`Strip::slot_at`]), so the room that opens is under the pointer.
    fn strip_with_room(&self, room: usize) -> Option<Strip> {
        let count = self.ivars().shown.borrow().tabs.len();
        (count > 0).then(|| self.strip_for(&self.bar_for(count + room, 0, None, None), |_| false))
    }

    /// The place under `point` (the bar's space) of the list as it would be with `room` more
    /// places: the strip's along x, the column's along y — the one rule ([`tabs::Run::slot_at`])
    /// whichever way the tabs lie. 0 for a bar with no tabs.
    fn slot_under(&self, room: usize, point: NSPoint) -> usize {
        if self.ivars().side.get().is_some() {
            self.column_with_room(room)
                .map_or(0, |column| column.slot_at(point.y))
        } else {
            self.strip_with_room(room)
                .map_or(0, |strip| strip.slot_at(point.x))
        }
    }

    /// The press on tab `tab`'s chip, as `event` reports it, may become a drag. Not in a bar of
    /// fewer than two tabs (a lone title is the window's; a column's lone row is a row, and the
    /// way to another window), not while a name is being typed, and not while the window holds a
    /// question of its own: the tab could not move then.
    fn grip(&self, tab: u64, event: &NSEvent) {
        let iv = self.ivars();
        iv.drag.set(None);
        let free = self
            .terminal_window()
            .is_some_and(|window| window.selection_free());
        if iv.renaming.get().is_some() || !free {
            return;
        }
        let press = self.convertPoint_fromView(event.locationInWindow(), None);
        let index = iv
            .shown
            .borrow()
            .tabs
            .iter()
            .position(|label| label.tab == tab);
        let grip = index.and_then(|index| {
            if iv.side.get().is_some() {
                // The column is read along y.
                let column = iv.column.borrow();
                Grip::new(column.as_ref()?, index, press.y)
            } else {
                let laid = iv.laid.borrow();
                Grip::new(laid.as_ref()?, index, press.x)
            }
        });
        if let Some(grip) = grip {
            iv.drag.set(Some(Dragging {
                tab,
                grip,
                at: None,
                torn: false,
                multi: iv.picks.borrow().tabs.contains(&tab) && self.is_side(),
                grouped: self.is_side()
                    && self
                        .terminal_window()
                        .is_some_and(|window| window.has_groups()),
                split: None,
            }));
        }
    }

    /// A plain press on tab `tab`'s row, with rows picked together: if it is one of them the
    /// press is the picks' to carry, and nothing is selected until it is known not to be a pull
    /// ([`Self::release`]). `false` for any other row, or when nothing can be pulled (the
    /// window holds a question, a name is being typed): the press is then a plain one.
    fn hold_picked(&self, tab: u64, event: &NSEvent) -> bool {
        if !self.is_side() || !self.ivars().picks.borrow().tabs.contains(&tab) {
            return false;
        }
        self.grip(tab, event);
        self.ivars()
            .drag
            .get()
            .is_some_and(|drag| drag.tab == tab && drag.multi)
    }

    /// The slot of the column's places the pointer at height `y` (the bar's space) is over
    /// ([`crate::groups::Groups::side_spots`]), read on the column as it is: the preview of a
    /// row slid along it is laid out from the answer, never the other way round. `None` for a
    /// list the window has changed under the held button.
    fn column_slot(&self, y: f64) -> Option<usize> {
        let window = self.terminal_window()?;
        let column = self.column_with_room(0)?;
        spot_slot(&window.side_spots(&column), y)
    }

    /// The pointer moved with the button down on tab `tab`'s chip ([`Grip::track`]): past
    /// the slop the chip follows it along the strip and the others make room; out of the bar the
    /// tab is carried away in a drag session — and past the slop at once for rows picked together,
    /// which are carried, not slid. In the column of a window that has groups the place the others
    /// make room for is a slot of the column ([`Self::column_slot`]): at a group's border it is
    /// the end of the group above while the pointer is over the upper half of the header, the head
    /// of the group below while it is over the lower.
    fn drag(&self, tab: u64, event: &NSEvent) {
        let iv = self.ivars();
        let Some(mut drag) = iv.drag.get().filter(|drag| drag.tab == tab && !drag.torn) else {
            return;
        };
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let size = self.bounds().size;
        let upright = iv.side.get().is_some();
        // The pointer `(along, across)` the list and the bar's `(along, across)` extent; and where
        // the list starts, which the chip's place is counted from.
        let (tracked, origin) = if upright {
            let column = iv.column.borrow();
            let Some(column) = column.as_ref() else {
                return;
            };
            (
                drag.grip
                    .track(column, (point.y, point.x), (size.height, size.width)),
                column.list.y,
            )
        } else {
            let laid = iv.laid.borrow();
            let Some(laid) = laid.as_ref() else {
                return;
            };
            (
                drag.grip
                    .track(laid, (point.x, point.y), (size.width, size.height)),
                laid.span.x,
            )
        };
        let tracked = match tracked {
            Drag::Reorder { .. } if drag.multi => Drag::TearOff,
            Drag::Reorder { left, to } if drag.grouped => Drag::Reorder {
                left,
                to: self
                    .column_slot(point.y)
                    .or_else(|| drag.at.map(|(_, slot)| slot))
                    .unwrap_or(to),
            },
            other => other,
        };
        match tracked {
            Drag::Press => iv.drag.set(Some(drag)),
            Drag::Reorder { left, to } => {
                let first = drag.at.is_none();
                // In the column the row is over another row's middle — lit, to split into — or
                // a place of the list, read where the rows stood before the drag; a row lit or no
                // longer lit changes the whole column ([`Self::aim_row`]).
                let (relit, slot) = if upright {
                    self.aim_row(&mut drag, point.y)
                } else {
                    (false, None)
                };
                let to = if drag.grouped { to } else { slot.unwrap_or(to) };
                let same_place = drag.at.is_some_and(|(_, place)| place == to) && !relit;
                drag.at = Some((left, to));
                iv.drag.set(Some(drag));
                let chip = self.chip(tab);
                if first && let Some(chip) = &chip {
                    // Above its neighbours for as long as it is held.
                    iv.strip.addSubview_positioned_relativeTo(
                        chip,
                        NSWindowOrderingMode::Above,
                        None,
                    );
                }
                match chip {
                    // Only the held chip moved: the rest of the strip is as it was laid out.
                    Some(chip) if same_place && !first => chip.follow(left - origin, upright),
                    _ => self.lay_out(),
                }
            }
            Drag::TearOff => {
                // Only a tab a session has taken is torn: were none started, the press goes on
                // as a drag along the strip and no faint place is left behind.
                if self.tear_off(tab, drag.multi, event) {
                    drag.at = None;
                    drag.torn = true;
                    if drag.split.take().is_some() {
                        iv.target.set(None);
                    }
                    iv.drag.set(Some(drag));
                    iv.glide.set(true);
                    self.lay_out();
                }
            }
        }
    }

    /// The tab leaves the bar: a drag session carries it (`tab_drag`), and the application keeps
    /// the session's source until it ends. With `multi` the rows picked together are carried
    /// with it, drawn as a stack with their count. `false` if none could start.
    fn tear_off(&self, tab: u64, multi: bool, event: &NSEvent) -> bool {
        let iv = self.ivars();
        let (Some(chip), Some(app), Some(palette)) =
            (self.chip(tab), app::delegate(self.mtm()), iv.palette.get())
        else {
            return false;
        };
        let tabs = if multi { self.picks() } else { vec![tab] };
        if tabs.is_empty() {
            return false;
        }
        let carried = Carried::Tabs {
            window: iv.window,
            tabs,
        };
        let lone = carried.lone();
        let face = tab_drag::picture(&chip);
        let count = match &carried {
            Carried::Tabs { tabs, .. } => tabs.len(),
            Carried::Group { .. } => 1,
        };
        let image = match face {
            Some(face) if count > 1 => Some(tab_drag::stack(
                &face,
                count,
                palette.accent,
                crate::pane_drag::ink_on(palette.accent),
            )),
            face => face,
        };
        let size = image
            .as_ref()
            .map_or(chip.bounds().size, |image| image.size());
        app.hold_tab_drag(tab_drag::begin(
            &chip,
            carried,
            event,
            image.as_deref(),
            size,
        ));
        // The block of one tab can be merged into panes with ⌥⌘ — or, out of the side column,
        // into its own window's panes without them; several tabs have none.
        if let Some(tab) = lone {
            app.tab_merge_begin(tab, self.is_side());
        }
        true
    }

    /// A group's header — or, in the title row, its button `view` — is pulled: the group is
    /// carried in a drag session, with all its tabs, and the header waits, faint, in its place
    /// until the session ends ([`Self::drag_ended`]). `faced` for a header, which has no face of
    /// its own to be drawn with.
    pub(crate) fn carry_group(&self, group: GroupId, view: &NSView, event: &NSEvent, faced: bool) {
        let iv = self.ivars();
        let (Some(app), Some(window), Some(palette)) = (
            app::delegate(self.mtm()),
            self.terminal_window(),
            iv.palette.get(),
        ) else {
            return;
        };
        if !window.selection_free() || iv.renaming_group.get().is_some() {
            return;
        }
        self.close_group_list(false);
        let face = tab_drag::picture(view);
        let image = match face {
            Some(face) if faced => Some(tab_drag::faced(&face, palette.card, palette.card_line)),
            face => face,
        };
        let size = image
            .as_ref()
            .map_or(view.bounds().size, |image| image.size());
        let carried = Carried::Group {
            window: iv.window,
            group,
        };
        app.hold_tab_drag(tab_drag::begin(
            view,
            carried,
            event,
            image.as_deref(),
            size,
        ));
        iv.group_torn.set(Some(group));
        self.lay_out();
    }

    /// The button is let go on tab `tab`'s chip: a tab moved along the strip settles in the place
    /// it was carried to — the window's applier orders it, once, however many places it passed
    /// — and the chip glides there from the pointer.
    fn release(&self, tab: u64, event: &NSEvent) {
        let iv = self.ivars();
        let Some(drag) = iv.drag.get().filter(|drag| drag.tab == tab) else {
            return;
        };
        if drag.torn {
            // The session has it; this release is not its end.
            return;
        }
        iv.drag.set(None);
        if let Some(into) = drag.split {
            // Let go on a lit row: the held tab joins that row's tab as a split on its right, a
            // turn later (this chip goes with its tab); until then it glides back to its place.
            iv.target.set(None);
            iv.glide.set(true);
            self.lay_out();
            if let Some(window) = self.terminal_window() {
                window.split_tab_into(tab, into);
            }
            return;
        }
        if drag.multi {
            // A press on a picked row that never pulled them: the plain click it would have been,
            // which opens the tab and lets the picks go.
            self.select(tab);
            return;
        }
        // A click that never moved the tab is only a click.
        let Some((_, to)) = drag.at else {
            return;
        };
        iv.glide.set(true);
        // Where the tab stands in the list now, not where it stood at the press: the list can
        // change under a held button (a background tab's shell exits).
        let now = iv
            .shown
            .borrow()
            .tabs
            .iter()
            .position(|label| label.tab == tab);
        match self.terminal_window() {
            // A slot of the column: the row goes where the slot's target says, in the group that
            // was shown — one move, which Undo Move takes back. A slot that changes nothing is
            // refused, and the row glides back.
            Some(window) if drag.grouped => {
                let target = self
                    .column_with_room(0)
                    .and_then(|column| window.side_spots(&column).into_iter().nth(to))
                    .map(|spot| spot.target);
                if !target.is_some_and(|target| window.regroup_tab(tab, target)) {
                    self.lay_out();
                }
            }
            // The place in the list is the listed tab's in the strip the applier counts.
            Some(window) if now.is_some_and(|now| now != to) => {
                window.move_tab_listed(tab, to);
            }
            _ => self.lay_out(),
        }
        // No enter or exit comes while the button is down: the chip under the pointer now is
        // the hovered one.
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        self.hover(self.tab_under(point));
    }

    /// The drag session that carried a tab away from this bar is over, wherever the tab went: its
    /// chip returns to full strength in its place, or is gone with its tab, and the chip under
    /// the pointer is the hovered one (no enter came during the drag).
    pub(crate) fn drag_ended(&self) {
        let iv = self.ivars();
        iv.drag.set(None);
        iv.group_torn.set(None);
        let under = self.window().and_then(|window| {
            let point =
                self.convertPoint_fromView(window.mouseLocationOutsideOfEventStream(), None);
            self.tab_under(point)
        });
        iv.hovered.set(under);
        iv.glide.set(true);
        self.lay_out();
    }

    /// A tab carried over this bar opens room at slot `gap` (`None` closes it).
    pub(crate) fn open_gap(&self, gap: Option<usize>) {
        let iv = self.ivars();
        if iv.gap.replace(gap) != gap {
            iv.glide.set(true);
            self.lay_out();
        }
    }

    /// Puts the line that marks a place of the column at `content` — a place of the list's
    /// content, not scrolled ([`content_line`]) — what is carried over the column would be put
    /// there; or takes it away (`None`). A layout finds the line again where it was, whatever the
    /// column's scroll.
    pub(crate) fn show_cast(&self, content: Option<f64>) {
        if self.ivars().cast.replace(content) != content {
            self.lay_out();
        }
    }

    /// What a carry over the bar showed, room opened or a line, goes: the carry left, or ended.
    pub(crate) fn clear_drop(&self) {
        let iv = self.ivars();
        let room = iv.gap.replace(None).is_some();
        let line = iv.cast.replace(None).is_some();
        if room {
            iv.glide.set(true);
        }
        if room || line {
            self.lay_out();
        }
    }

    /// Closes the gap **without** a layout of its own — the window's list is about to take the
    /// tab in, and that layout is the one that matters: the chips move from where they stand
    /// (gap open) to where they will stand (the tab there), not through a gap closing first.
    pub(crate) fn close_gap_quietly(&self) {
        self.ivars().gap.set(None);
    }

    // ─── A pane carried over the strip ───────────────────────────────────

    /// What a pane carried with the pointer at `at` (window coordinates) is over in
    /// the strip (or the column), `None` when the pointer is not in the bar. The list is read as
    /// it is, without a gap: the room a gap opens is not under the pointer's reach, so
    /// the answer does not depend on it ([`tabs::Strip::pane_over`], [`tabs::Column::pane_over`]).
    pub(crate) fn pane_target(&self, at: NSPoint) -> Option<PaneTarget> {
        let point = self.convertPoint_fromView(at, None);
        if !contains(self.bounds(), point) {
            return None;
        }
        let over = if self.ivars().side.get().is_some() {
            self.column_with_room(0)?.pane_over(point.y)
        } else {
            self.strip_with_room(0)?.pane_over(point.x)
        };
        match over {
            tabs::PaneOver::OnTab { index } => self
                .ivars()
                .shown
                .borrow()
                .tabs
                .get(index)
                .map(|label| PaneTarget::Tab(label.tab)),
            tabs::PaneOver::Between { gap } => Some(PaneTarget::Between(gap)),
        }
    }

    /// Whether `at` (window coordinates) is in the bar — in the side mode, the column down the
    /// window's edge: a drop there is the bar's.
    pub(crate) fn holds(&self, at: NSPoint) -> bool {
        contains(self.bounds(), self.convertPoint_fromView(at, None))
    }

    /// The strip's answer to a carried pane: the chip it is over the middle of is lit,
    /// or room opens between the chips where it would become a tab; `None` when it
    /// is over neither (or has left). The column marks the place with a line and moves no row
    /// ([`tab_side::Insertion`]).
    pub(crate) fn show_pane_target(&self, target: Option<PaneTarget>) {
        let iv = self.ivars();
        let tab = match target {
            Some(PaneTarget::Tab(tab)) => Some(tab),
            _ => None,
        };
        let between = match target {
            Some(PaneTarget::Between(gap)) => Some(gap),
            _ => None,
        };
        let mut changed = iv.target.replace(tab) != tab;
        if iv.side.get().is_some() {
            changed |= iv.mark.replace(between) != between;
        }
        if changed {
            self.lay_out();
        }
        if iv.side.get().is_none() {
            self.open_gap(between);
        }
    }

    /// The pane is about to become a tab here: the lit chip and the room between chips are
    /// forgotten **without** a layout of their own — the window's list is about to take the
    /// tab in, and that layout is the one that matters ([`Self::close_gap_quietly`]). If the
    /// pane does not become a tab after all, [`Self::lay_out`] puts the chips right.
    pub(crate) fn settle_pane_target(&self) {
        let iv = self.ivars();
        iv.target.set(None);
        iv.gap.set(None);
        iv.mark.set(None);
    }

    /// A carried pane waits on tab `tab`'s chip (`Some`): the line under it fills over
    /// [`tabs::SPRING_DELAY`] and the carrier opens the tab when it is full. Under Reduce
    /// Motion there is no line to fill: the tab opens all the same, and the chip is lit.
    /// `None` takes the line away.
    pub(crate) fn dwell(&self, tab: Option<u64>) {
        let iv = self.ivars();
        let before = iv.dwelling.replace(tab);
        if before == tab {
            return;
        }
        if let Some(chip) = before.and_then(|before| self.chip(before)) {
            chip.dwell(None);
        }
        if !self.reduce_motion()
            && let Some(chip) = tab.and_then(|tab| self.chip(tab))
        {
            chip.dwell(Some(tabs::SPRING_DELAY.as_secs_f64()));
        }
    }

    /// Tab `tab`'s chip glows once: a pane joined it or became a tab beside it, and the
    /// selection stayed where it was. Not under Reduce Motion.
    pub(crate) fn pulse(&self, tab: u64) {
        if self.reduce_motion() {
            return;
        }
        if let Some(chip) = self.chip(tab) {
            chip.pulse();
        }
    }

    /// The tab that was on screen before the press on tab `tab`'s chip that is, or was,
    /// a drag: a press selects, so it is the tab the chip's tab can be let go into.
    /// `None` if it was `tab` itself.
    pub(crate) fn selected_before(&self, tab: u64) -> Option<u64> {
        self.ivars().before.get().filter(|before| *before != tab)
    }

    /// What a carry at `point` (the bar's space) would come to on this bar `window`, which both
    /// halves of a drop read — the pointer over the bar ([`Self::carried_over`]) and the drop
    /// ([`Self::accept_drop`]) — so what is shown is what is done.
    ///
    /// **One tab on a window with no group** is the strip's own: room opens at the slot under the
    /// pointer of the strip it would make (a tab from another window), the window's own tab shows
    /// no gap (its faint chip is its place), and the place is the strip's ([`tabs::landing`]).
    /// Everything else — several tabs, a group, any tab on a window that has groups — is a place
    /// the column names ([`crate::groups::Groups::side_spots`], marked by a line, or among the
    /// groups, [`crate::groups::Groups::group_spots`]) or, in the title row, a place of the
    /// cluster on screen.
    fn aim(&self, carried: &Carried, window: &TerminalWindow, point: NSPoint) -> Aim {
        let own = carried.window() == window.id();
        match carried {
            Carried::Group { .. } if !self.is_side() => {
                // The title row has no place among groups to point at: another window takes the
                // group last, and a window's own group is where it is.
                Aim {
                    landed: (!own).then(|| Landed::Group(window.group_count())),
                    ..Aim::default()
                }
            }
            Carried::Group { group, .. } => {
                let Some(column) = self.column_with_room(0) else {
                    return Aim::default();
                };
                let spots = window.group_spots(&column);
                let Some(spot) = group_spot_at(&spots, point.y) else {
                    // A window with no group takes the first one below its tabs.
                    let last = column.rows.last();
                    return Aim {
                        line: (!own && window.group_count() == 0).then(|| {
                            content_line(
                                last.map_or(column.list.y, |row| row.y + row.height)
                                    + tabs::GAP / 2.0,
                                &column,
                            )
                        }),
                        landed: (!own && window.group_count() == 0).then_some(Landed::Group(0)),
                        ..Aim::default()
                    };
                };
                let to = match window.group_position(*group) {
                    Some(from) if own => group_place(from, spot.slot),
                    _ => spot.slot,
                };
                Aim {
                    line: Some(content_line(spot.line, &column)),
                    landed: Some(Landed::Group(to)),
                    ..Aim::default()
                }
            }
            Carried::Tabs { tabs, .. } if tabs.len() == 1 && !window.has_groups() => {
                let tab = tabs[0];
                if window.index_of(tab).is_some() {
                    let place = window.strip_place(self.slot_under(0, point));
                    Aim {
                        landed: Some(Landed::Strip(tabs::landing(Some(Over::Own { place })))),
                        ..Aim::default()
                    }
                } else {
                    let gap = self.slot_under(1, point);
                    let place = window.strip_place(gap);
                    Aim {
                        room: Some(gap),
                        landed: Some(Landed::Strip(tabs::landing(Some(Over::Other {
                            gap: place,
                        })))),
                        ..Aim::default()
                    }
                }
            }
            Carried::Tabs { .. } if self.is_side() => {
                let Some(column) = self.column_with_room(0) else {
                    return Aim::default();
                };
                let spots = window.side_spots(&column);
                let Some(spot) = spot_at(&spots, point.y) else {
                    return Aim::default();
                };
                Aim {
                    line: Some(content_line(spot.line, &column)),
                    landed: Some(Landed::Tabs(spot.target.clone())),
                    ..Aim::default()
                }
            }
            Carried::Tabs { .. } => {
                // The title row lists the cluster on screen: the place is one of it, its end the
                // cluster's own. A tab of this window ends at the place under the pointer; one
                // from another is put before the tab there.
                let gap = self.slot_under(usize::from(!own), point);
                let target = match carried
                    .lone()
                    .filter(|tab| own && window.index_of(*tab).is_some())
                {
                    Some(tab) => window.own_drop_target(tab, gap),
                    None => window.drop_target(gap),
                };
                Aim {
                    room: (!own).then_some(gap),
                    landed: Some(Landed::Tabs(target)),
                    ..Aim::default()
                }
            }
        }
    }

    /// `draggingEntered:` and `draggingUpdated:`: what is carried over this bar ([`Self::aim`]):
    /// a tab from another window opens room at the slot under the pointer **of the strip it
    /// would make**, so the room is under the pointer and the answer does not depend on the room;
    /// a window's own tab shows no gap (its faint chip is its place); in the column of a window
    /// with groups, and for several tabs or a group, a line marks the place. Anything that is not
    /// tabs or a group of this process, or that this window cannot take now (it holds a question
    /// of its own), is not taken: the drop is then on nothing, and what is carried becomes a
    /// window.
    fn carried_over(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
        let Some(carried) = tab_drag::carried(info) else {
            return NSDragOperation::None;
        };
        let Some(window) = self
            .terminal_window()
            .filter(|window| window.selection_free())
        else {
            self.clear_drop();
            return NSDragOperation::None;
        };
        let point = self.convertPoint_fromView(info.draggingLocation(), None);
        let aim = self.aim(&carried, &window, point);
        self.show_cast(aim.line);
        self.open_gap(aim.room);
        NSDragOperation::Move
    }

    /// `performDragOperation:`: what is carried is let go here. What it comes to is decided now
    /// ([`Self::aim`]) and carried out when the session has ended, in a turn of its own; the
    /// gap or the line stays until then, so the list does not close up and open again. Always
    /// accepted if it is tabs or a group this window takes: a refusal would read as a drop on
    /// nothing and make a window — a group let go on its own title row decides nothing, and is
    /// accepted all the same.
    fn accept_drop(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
        let Some(carried) = tab_drag::carried(info) else {
            return false;
        };
        let (Some(window), Some(app)) = (self.terminal_window(), app::delegate(self.mtm())) else {
            return false;
        };
        let point = self.convertPoint_fromView(info.draggingLocation(), None);
        if let Some(landed) = self.aim(&carried, &window, point).landed {
            app.tab_dropped(window.id(), landed);
        }
        true
    }

    /// A chip's `×` or middle click: the window asks and closes, a turn
    /// later (the module header).
    fn close(&self, tab: u64) {
        let window = self.ivars().window;
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window(window)) {
                window.close_tab_asking(tab);
            }
        });
    }

    /// `+`: ⌘T's job, in this window, a turn later.
    fn new_tab(&self) {
        let window = self.ivars().window;
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(app) = app::delegate(mtm) {
                app.new_tab_in(window);
            }
        });
    }

    /// A header's `+`: a tab at the end of group `group`, a turn later.
    fn new_tab_in_group(&self, group: GroupId) {
        let window = self.ivars().window;
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(app) = app::delegate(mtm) {
                app.new_tab_seated(window, Seat::Group(group));
            }
        });
    }

    // ─── The clock ───────────────────────────────────────────────────────

    /// Sets the bar's one delayed wake from what changes with time on it
    /// (`tabs::Clock::delay`): the open card's running time and, for the
    /// running rings of several tabs on a visible window outside Reduce
    /// Motion, the hour where a ring's pace changes (the rings turn by
    /// themselves in between, [`Glyph`]). A wake already due sooner is kept;
    /// one due later gives way — `after` cannot be cancelled, so a wake that
    /// is not wanted any more is made stale by the generation and does
    /// nothing when it comes. Nothing to show: no wake.
    pub(crate) fn arm_clock(&self) {
        let iv = self.ivars();
        let visible = self
            .terminal_window()
            .is_some_and(|window| window.window_visible());
        let shown = iv.shown.borrow();
        // The rings that show: a running tab whose glyph is not a more
        // urgent one (a question), among several tabs.
        let running = if !self.lone_title(shown.tabs.len()) {
            shown
                .tabs
                .iter()
                .filter(|label| label.indicator == Some(Indicator::Running))
                .filter_map(|label| label.running)
                .collect()
        } else {
            Vec::new()
        };
        drop(shown);
        let card = iv.card.borrow().as_ref().and_then(OpenCard::running);
        let map = iv.card.borrow().as_ref().is_some_and(OpenCard::pictured);
        let delay = Clock {
            visible,
            reduce_motion: self.reduce_motion(),
            running,
            card,
            map,
        }
        .delay();
        let Some(delay) = delay else {
            if iv.clock_due.take().is_some() {
                iv.clock.set(iv.clock.get().wrapping_add(1));
            }
            return;
        };
        let now = Instant::now();
        let due = now + delay;
        // A wake due in the past was lost (its job found no window, or
        // `dispatch` could not take it): it does not hold the place.
        if iv
            .clock_due
            .get()
            .is_some_and(|armed| armed > now && armed <= due)
        {
            return;
        }
        let generation = iv.clock.get().wrapping_add(1);
        iv.clock.set(generation);
        let armed = after(delay, iv.window, move |bar| bar.clock_fired(generation));
        iv.clock_due.set(armed.then_some(due));
    }

    /// The wake came: if it is the live one, every tab's running time is
    /// read again (`TerminalWindow::running_times`, a leaf lock per pane),
    /// a ring whose pace changed is set turning anew, the open card's time
    /// moves and the next wake is set.
    fn clock_fired(&self, generation: u64) {
        let iv = self.ivars();
        if iv.clock.get() != generation {
            return;
        }
        iv.clock_due.set(None);
        if let Some(window) = self.terminal_window() {
            let times = window.running_times();
            let mut shown = iv.shown.borrow_mut();
            for label in &mut shown.tabs {
                if let Some((_, running)) = times.iter().find(|(tab, _)| *tab == label.tab) {
                    label.running = *running;
                }
            }
        }
        self.time_card();
        self.shoot_card();
        self.lay_out();
    }

    // ─── The summary card ────────────────────────────────────────────────

    /// The pointer's chip changed: the card opens for the new one after
    /// [`CARD_DELAY`] — at once while a card is open or one closed within
    /// [`CARD_WARM`] — and closes when the pointer leaves the chips. A
    /// pending opening for another chip is made stale.
    fn hover_card(&self, tab: Option<u64>) {
        let iv = self.ivars();
        let generation = iv.card_wait.get().wrapping_add(1);
        iv.card_wait.set(generation);
        if iv.card_quiet.get() != tab {
            iv.card_quiet.set(None);
        }
        let Some(tab) = tab else {
            // A card with a picture is where the pointer is heading: it gets a moment to arrive.
            let mapped = iv.card.borrow().as_ref().is_some_and(OpenCard::pictured);
            if mapped && iv.card_inside.get() {
                return;
            }
            if mapped {
                after(CARD_BRIDGE, iv.window, move |bar| {
                    let iv = bar.ivars();
                    if iv.card_wait.get() == generation && !iv.card_inside.get() {
                        bar.close_card();
                    }
                });
                return;
            }
            self.close_card();
            return;
        };
        if iv.card_quiet.get() == Some(tab) {
            return;
        }
        let warm = iv.card.borrow().is_some()
            || iv
                .card_closed
                .get()
                .is_some_and(|closed| closed.elapsed() < CARD_WARM);
        if warm {
            self.open_card(tab);
            return;
        }
        after(CARD_DELAY, iv.window, move |bar| {
            let iv = bar.ivars();
            if iv.card_wait.get() == generation && iv.hovered.get() == Some(tab) {
                bar.open_card(tab);
            }
        });
    }

    /// A chip was pressed: its card goes and stays away until the pointer
    /// leaves the chip.
    fn pressed(&self, tab: u64) {
        let iv = self.ivars();
        iv.before.set(
            self.terminal_window()
                .and_then(|window| window.try_selected_tab())
                .map(|selected| selected.id()),
        );
        iv.card_wait.set(iv.card_wait.get().wrapping_add(1));
        iv.card_quiet.set(Some(tab));
        self.close_card();
    }

    /// Opens (or turns) the card to tab `tab`'s — only where a tab is a chip: among several
    /// tabs, or in the side column, where a lone tab is a row like the rest.
    fn open_card(&self, tab: u64) {
        if self.lone_title(self.ivars().shown.borrow().tabs.len()) {
            return;
        }
        self.read_card(tab);
        self.draw_card();
        // Before the card is put on screen, so it arrives with its pictures in.
        self.shoot_card();
        self.place_card();
        self.arm_clock();
    }

    /// Reads tab `tab`'s card (`TerminalTab::card`: a `Term` round for its
    /// newest command's row) into the open card; a tab no longer in the
    /// window has none, and the card goes.
    fn read_card(&self, tab: u64) {
        let card = self
            .terminal_window()
            .and_then(|window| window.tab_card(tab));
        match card {
            Some(card) => {
                let before = self.ivars().card.replace(Some(OpenCard {
                    tab,
                    card,
                    read_at: Instant::now(),
                }));
                // The card turned to another tab: the first one's panes let their pictures go.
                if let Some(before) = before.filter(|before| before.tab != tab) {
                    self.release_shots(before.tab);
                }
            }
            None => self.close_card(),
        }
    }

    /// Takes a picture of each pane of the open card's map and gives it to its tile
    /// (`TerminalPane::snapshot`, through the window and the tab). Called when the card opens and
    /// by the bar's clock every [`tabs::MAP_REFRESH`] after, and nowhere else: a card without a
    /// map, a window nobody sees and a card that is gone take none — the clock stops asking with
    /// them (`Clock::map`), and the panes' textures go with the card ([`Self::release_shots`]).
    fn shoot_card(&self) {
        let iv = self.ivars();
        let (tab, tiles) = {
            let card = iv.card.borrow();
            let Some(open) = card.as_ref().filter(|open| open.pictured()) else {
                return;
            };
            let height = tabs::map_height(open.card.aspect);
            let tiles: Vec<(u64, (f64, f64))> = open
                .card
                .panes
                .iter()
                .map(|pane| {
                    let at = tabs::map_tile(pane.place, height);
                    (pane.id, (at.width, at.height))
                })
                .collect();
            (open.tab, tiles)
        };
        let Some(window) = self.terminal_window() else {
            return;
        };
        if !window.window_visible() {
            return;
        }
        let scale = self
            .window()
            .map_or(2.0, |window| window.backingScaleFactor());
        iv.card_view
            .set_shots(&window.tab_shots(tab, &tiles, scale));
    }

    /// The map got tiles that have no picture (new panes, or a pane that was not ready): one now,
    /// not at the clock's next turn.
    fn shoot_missing(&self) {
        if self.ivars().card_view.lacks_shots() {
            self.shoot_card();
        }
    }

    /// The panes of tab `tab` let go of what they kept for the card's pictures.
    fn release_shots(&self, tab: u64) {
        if let Some(window) = self.terminal_window() {
            window.release_tab_shots(tab);
        }
    }

    /// The clock moved: an open card's running time follows its tab's,
    /// without reading its row again.
    fn time_card(&self) {
        if self
            .ivars()
            .card
            .borrow()
            .as_ref()
            .is_some_and(|open| open.running().is_some())
        {
            self.draw_card();
        }
    }

    /// Writes the open card's lines into its view, in the theme's colours.
    fn draw_card(&self) {
        let iv = self.ivars();
        let (Some(palette), Some(theme)) = (iv.palette.get(), iv.theme.get()) else {
            return;
        };
        let card = iv.card.borrow();
        let Some(open) = card.as_ref() else {
            return;
        };
        let mark = theme.mark_rgb(open.card.mark);
        let lines: Vec<(String, u32)> = tabs::card_lines(&open.now())
            .into_iter()
            .map(|(text, tone)| (text, palette.tone(tone, mark)))
            .collect();
        let map = open.pictured().then(|| MapDraw {
            panes: &open.card.panes,
            aspect: open.card.aspect,
            style: IconStyle::current(self.mtm()),
            mark,
        });
        iv.card_view.aim(iv.window, open.tab);
        let size = iv.card_view.write(&lines, map.as_ref(), palette);
        // Only a change of size sets the frame: a card gliding to its place is not touched again.
        let mut frame = iv.card_view.frame();
        if frame.size != size {
            frame.size = size;
            iv.card_view.setFrame(frame);
        }
    }

    /// Puts the open card under its chip, over the content (the root view's
    /// top subview); closes it when its tab is no longer a chip.
    ///
    /// Where the card plays is decided here, by what it was doing: not on screen, it arrives
    /// ([`Self::card_arrives`]); fading away, it comes back from where the fade stands;
    /// standing at another place, it glides there (only its origin: the lines it was just given
    /// are already the new ones, at the new size); standing where it is asked to, nothing —
    /// layouts ask often, and a motion begun again on each would never get anywhere. Reduce
    /// Motion keeps the fades and drops the slide.
    fn place_card(&self) {
        let iv = self.ivars();
        let Some(tab) = iv.card.borrow().as_ref().map(|open| open.tab) else {
            return;
        };
        let chip = iv
            .chips
            .borrow()
            .iter()
            .find(|chip| chip.tab() == tab)
            .cloned();
        // SAFETY: reading the superview; we are on the main thread.
        let root = unsafe { self.superview() };
        let chips = iv.chips.borrow().len();
        let (Some(chip), Some(root), false) = (chip, root, self.lone_title(chips)) else {
            self.close_card();
            return;
        };
        // The chip's frame is in the strip's space.
        let at = chip.frame();
        let size = iv.card_view.frame().size;
        let origin = if iv.side.get().is_some() {
            // Beside the row, on its right: the column's edge and the row's top in the root
            // view's space (the bar fills the column, the strip is placed in the bar).
            let column_right = self.frame().origin.x + self.bounds().size.width;
            let row_top = self.frame().origin.y + iv.strip.frame().origin.y + at.origin.y;
            let window = root.bounds().size;
            let (x, y) = card_beside(
                row_top,
                column_right,
                (window.width, window.height),
                (size.width, size.height),
            );
            NSPoint::new(x, y)
        } else {
            NSPoint::new(
                card_x(
                    iv.strip.frame().origin.x + at.origin.x,
                    self.bounds().size.width,
                    size.width,
                ),
                self.frame().origin.y + at.origin.y + at.size.height + CARD_DROP,
            )
        };
        let card = &iv.card_view;
        let reduce = self.reduce_motion();
        // SAFETY: reading the superview; we are on the main thread.
        let up = unsafe { card.superview() }.is_some();
        let was = iv.card_at.replace(Some(origin));
        let moved =
            was.is_some_and(|was| (was.x - origin.x).abs() > 0.5 || (was.y - origin.y).abs() > 0.5);
        if up {
            if card.leaving() {
                self.card_returns(reduce);
            }
            if moved && !reduce {
                let gliding = card.clone();
                animate_eased(Ease::Out, CARD_MOVE, move || {
                    gliding.animator().setFrameOrigin(origin);
                });
            } else if card.frame() != NSRect::new(origin, size) {
                card.setFrame(NSRect::new(origin, size));
            }
        } else {
            still(|| card.setAlphaValue(1.0));
            card.setFrame(NSRect::new(origin, size));
        }
        // Over every container — a tab added since joined above it.
        let card_ptr: *const NSView = &***card;
        let on_top = root
            .subviews()
            .lastObject()
            .is_some_and(|top| std::ptr::eq(Retained::as_ptr(&top), card_ptr));
        if !on_top {
            card.removeFromSuperview();
            root.addSubview_positioned_relativeTo(card, NSWindowOrderingMode::Above, None);
        }
        if !up {
            self.card_arrives(NSRect::new(origin, size), reduce);
        }
    }

    /// The card, just put on screen and standing at `frame`, comes in: it fades in; and, unless
    /// Reduce Motion asks for the fade alone, grows from [`CARD_SCALE`] of its size about the
    /// middle of the edge that faces its chip, and slides [`CARD_SLIDE`] from the chip's side to
    /// its place. It all plays on the card's layer — the shadow and the lines with it — and is
    /// begun here once; the layer's own values are the rest.
    fn card_arrives(&self, frame: NSRect, reduce: bool) {
        let Some(layer) = self.ivars().card_view.layer() else {
            return;
        };
        layer.removeAllAnimations();
        let secs = if reduce { tabs::REDUCED_FADE } else { CARD_IN };
        play(&layer, OPACITY, 0.0, 1.0, secs, Ease::Out, 0.0);
        if reduce {
            return;
        }
        let to = Rect::new(
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        );
        let toward = if self.ivars().side.get().is_some() {
            ROW_LEFT
        } else {
            CHIP_ABOVE
        };
        let look = card_arrival(to, toward);
        if let Some(from) = fitted(&layer, look, to) {
            play_look(&layer, from, secs, Ease::Out);
        }
    }

    /// A card that was fading away is wanted again: it fades back in from the opacity it shows
    /// now (not from none, and not as an arrival), and its going is called off.
    fn card_returns(&self, reduce: bool) {
        let iv = self.ivars();
        let card = &iv.card_view;
        card.set_leaving(false);
        iv.card_out.set(iv.card_out.get().wrapping_add(1));
        let Some(layer) = card.layer() else {
            return;
        };
        let light = shown_opacity(&layer);
        still(|| card.setAlphaValue(1.0));
        let secs = if reduce { tabs::REDUCED_FADE } else { CARD_OUT };
        play(&layer, OPACITY, light, 1.0, secs, Ease::Out, 0.0);
    }

    /// Takes the card away; the warm window starts. The card fades out over [`CARD_OUT`] and is
    /// taken off the screen only then; until then it takes no part in the pointer's business,
    /// and a card asked for in the meantime comes back from the fade ([`Self::card_returns`]).
    fn close_card(&self) {
        let iv = self.ivars();
        if let Some(open) = iv.card.take() {
            iv.card_closed.set(Some(Instant::now()));
            // The textures go now; the tiles keep their last pictures until the card is down.
            self.release_shots(open.tab);
        }
        iv.card_inside.set(false);
        let card = &iv.card_view;
        // SAFETY: reading the superview; we are on the main thread.
        if unsafe { card.superview() }.is_none() || card.leaving() {
            return;
        }
        let Some(layer) = card.layer() else {
            self.take_card_down();
            return;
        };
        let secs = if self.reduce_motion() {
            tabs::REDUCED_FADE
        } else {
            CARD_OUT
        };
        let generation = iv.card_out.get().wrapping_add(1);
        iv.card_out.set(generation);
        card.set_leaving(true);
        // The layer's own opacity is the end of the fade, so nothing flashes back when it is over.
        let light = shown_opacity(&layer);
        still(|| card.setAlphaValue(0.0));
        play(&layer, OPACITY, light, 0.0, secs, Ease::In, 0.0);
        let delay = Duration::from_secs_f64(secs + CARD_SLACK);
        let scheduled = after(delay, iv.window, move |bar| {
            let iv = bar.ivars();
            if iv.card_out.get() == generation && iv.card_view.leaving() {
                bar.take_card_down();
            }
        });
        if !scheduled {
            self.take_card_down();
        }
    }

    /// The card view leaves the screen at once, with whatever it was playing.
    fn take_card_down(&self) {
        let iv = self.ivars();
        let card = &iv.card_view;
        card.set_leaving(false);
        iv.card_at.set(None);
        if let Some(layer) = card.layer() {
            layer.removeAllAnimations();
        }
        card.clear_shots();
        card.removeFromSuperview();
    }

    /// The pointer came onto the open card (one with a picture): it stays open while the pointer
    /// is on it, and a close that was waiting for the pointer to arrive is off.
    fn card_entered(&self) {
        let iv = self.ivars();
        iv.card_inside.set(true);
        iv.card_wait.set(iv.card_wait.get().wrapping_add(1));
    }

    /// The pointer left the open card: it goes, unless the pointer is back on the chip of its tab.
    fn card_left(&self) {
        let iv = self.ivars();
        iv.card_inside.set(false);
        let tab = iv.card.borrow().as_ref().map(|open| open.tab);
        if tab.is_some() && iv.hovered.get() == tab {
            return;
        }
        iv.card_wait.set(iv.card_wait.get().wrapping_add(1));
        self.close_card();
    }

    /// A press on pane `pane` of the open card's map: the card goes, the window's applier selects
    /// `tab` and gives the keyboard to the pane (its chip's tile turns to the pane's program
    /// with it). Done now, not a turn later: the card is only put away, kept by the bar, and the
    /// chips are not rebuilt by a selection.
    fn pick_pane(&self, tab: u64, pane: u64) {
        let iv = self.ivars();
        iv.card_wait.set(iv.card_wait.get().wrapping_add(1));
        self.close_card();
        if let Some(window) = self.terminal_window() {
            window.show_pane(tab, pane);
        }
    }

    /// The window stopped being key: the key hints and the card go — the
    /// release of ⌘ or the pointer's leaving may reach another application.
    pub(crate) fn resigned(&self) {
        self.command_held(false);
        self.group_held(false);
        let iv = self.ivars();
        iv.card_wait.set(iv.card_wait.get().wrapping_add(1));
        self.close_card();
        self.close_group_list(false);
    }

    // ─── ⌘ hints ─────────────────────────────────────────────────────────

    /// ⌘ is held alone in this window (`true`) or not — from the
    /// application's key watch (`AppDelegate::command_held`) and the window
    /// resigning key. Holding shows the keys after [`HINT_DELAY`]; anything
    /// else hides them at once and makes a pending show stale.
    pub(crate) fn command_held(&self, held: bool) {
        let iv = self.ivars();
        if !held {
            // A pending show goes stale; nothing shown or pending, nothing
            // to do (every key press comes here).
            if iv.hint_pending.replace(false) {
                iv.hint_wait.set(iv.hint_wait.get().wrapping_add(1));
            }
            if iv.hints.replace(false) {
                self.lay_out();
            }
            return;
        }
        if iv.hints.get() || iv.hint_pending.replace(true) {
            return;
        }
        let generation = iv.hint_wait.get().wrapping_add(1);
        iv.hint_wait.set(generation);
        after(HINT_DELAY, iv.window, move |bar| {
            let iv = bar.ivars();
            if iv.hint_wait.get() == generation {
                iv.hint_pending.set(false);
                if !iv.hints.replace(true) {
                    bar.lay_out();
                }
            }
        });
    }
}

impl TabBar {
    /// ⌃⌘ is held in this window (`true`) or not: [`Self::command_held`] for the groups' keys
    /// (⌃⌘1…⌃⌘9), which the column's headers show.
    pub(crate) fn group_held(&self, held: bool) {
        let iv = self.ivars();
        if !held {
            if iv.group_pending.replace(false) {
                iv.group_wait.set(iv.group_wait.get().wrapping_add(1));
            }
            if iv.group_hints.replace(false) {
                self.lay_out();
            }
            return;
        }
        if iv.group_hints.get() || iv.group_pending.replace(true) {
            return;
        }
        let generation = iv.group_wait.get().wrapping_add(1);
        iv.group_wait.set(generation);
        after(HINT_DELAY, iv.window, move |bar| {
            let iv = bar.ivars();
            if iv.group_wait.get() == generation {
                iv.group_pending.set(false);
                if !iv.group_hints.replace(true) {
                    bar.lay_out();
                }
            }
        });
    }
}

/// Whether `flags` hold ⌘ and no other modifier a shortcut uses (⇧, ⌥,
/// ⌃) — Caps Lock and the keypad's and the function key's bits do not
/// count: with Caps Lock on, ⌘ alone still shows the hints.
pub(crate) fn command_alone(flags: NSEventModifierFlags) -> bool {
    let chord = NSEventModifierFlags::Command
        | NSEventModifierFlags::Shift
        | NSEventModifierFlags::Option
        | NSEventModifierFlags::Control;
    flags.intersection(chord) == NSEventModifierFlags::Command
}

/// Watches ⌘ and ⌥⌘ for the tabs' key hints and the arrangement moment: a
/// local monitor of flag changes and key presses, the application's for its
/// lifetime (the caller keeps the token). ⌘ alone held is a hint's start,
/// ⌥⌘ the arrangement's ([`arrange::Hold`]); any other modifier, its release
/// or a key press (a chord: ⌘C, ⌥⌘←) is the end of either
/// (`AppDelegate::hold_changed`, which tells the key window). The event
/// passes on unchanged.
pub(crate) fn watch_command_key() -> Option<Retained<AnyObject>> {
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit gives the monitor a valid event.
        let event_ref = unsafe { event.as_ref() };
        let hold = if event_ref.r#type() == NSEventType::FlagsChanged {
            arrange::hold_of(event_ref.modifierFlags())
        } else {
            arrange::Hold::Nothing
        };
        // audit: a local monitor runs on the main thread, before `sendEvent:`.
        let mtm = MainThreadMarker::new().expect("a local event monitor runs on the main thread");
        if let Some(app) = app::delegate(mtm) {
            app.hold_changed(hold);
        }
        event.as_ptr()
    });
    // SAFETY: the block returns the valid event it was given.
    unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            NSEventMask::FlagsChanged | NSEventMask::KeyDown,
            &block,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BADGE_DOT, BADGE_HALO, BADGE_SIDE, Badge, CARD_BESIDE, CARD_EDGE, CARD_PAD_X, CARD_RISE,
        CARD_SCALE, CARD_SLIDE, CHIP_ABOVE, CHIP_HEIGHT, DISCS, Emblem, GLYPH_OVERHANG, GlyphLook,
        HOLLOW_ROOM, IconStyle, Label, Motion, Palette, Picks, ROW_LEFT, Spin, TAB_RADIUS,
        TILE_BADGE_INSET, TILE_ROOM, Tint, TitleAction, badge_centre, card_arrival, card_beside,
        card_x, command_alone, contains, content_line, counted, disc_side, glyph_slot, inks,
        kept_picks, line_span, pick_click, single_label, spin_of, title_double_click,
    };
    use super::{MAP_SCRIM_NAME_ONLY, TileShot, paint_tile};
    use crate::groups::Click;
    use crate::icon;
    use crate::program::identity::{Agent, Kind as ProgramKind};
    use crate::split::Rect;
    use crate::tabs::{self, Indicator, Tone};
    use bt_core::{IconColors, Theme};
    use objc2_app_kit::NSEventModifierFlags;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    /// A frame holds its left and bottom edge and not its right and top — the rule AppKit's hit
    /// test uses, which is also how the window under a screen point is found.
    #[test]
    fn a_rect_holds_its_near_edges_and_not_its_far_ones() {
        let rect = NSRect::new(NSPoint::new(100.0, 50.0), NSSize::new(400.0, 300.0));
        assert!(contains(rect, NSPoint::new(100.0, 50.0)));
        assert!(contains(rect, NSPoint::new(499.9, 349.9)));
        assert!(!contains(rect, NSPoint::new(500.0, 100.0)));
        assert!(!contains(rect, NSPoint::new(200.0, 350.0)));
        assert!(!contains(rect, NSPoint::new(99.9, 100.0)));
        // A second screen to the left has negative x: nothing of the first holds a pointer there.
        assert!(!contains(rect, NSPoint::new(-300.0, 100.0)));
    }

    /// The system's setting decides; the newer key wins over the older one,
    /// and absent both the row zooms.
    #[test]
    fn a_double_click_on_the_row_does_what_the_system_says() {
        assert_eq!(
            title_double_click(Some("Maximize"), false),
            TitleAction::Zoom
        );
        assert_eq!(title_double_click(Some("Fill"), true), TitleAction::Zoom);
        assert_eq!(
            title_double_click(Some("Minimize"), false),
            TitleAction::Minimize
        );
        assert_eq!(title_double_click(Some("None"), true), TitleAction::Nothing);
        assert_eq!(title_double_click(None, true), TitleAction::Minimize);
        assert_eq!(title_double_click(None, false), TitleAction::Zoom);
    }

    /// One tab keeps the diagnostic beside its title, the old subtitle's
    /// place; without one the title stands alone.
    #[test]
    fn the_single_title_carries_the_settings_diagnostic() {
        assert_eq!(single_label("bateri", ""), "bateri");
        assert_eq!(
            single_label("bateri", "settings.toml: line 2: bad"),
            "bateri \u{2013} settings.toml: line 2: bad"
        );
    }

    /// A dark theme lights the selected chip with the foreground; a light
    /// theme gives it a white face with a shadow instead. No role is new:
    /// the indicators and the card are the theme's own roles.
    #[test]
    fn the_selected_chip_follows_the_themes_lightness() {
        let dark = Palette::of(&Theme::BATERI);
        assert_eq!(dark.selected.rgb, Theme::BATERI.foreground);
        assert_eq!(dark.shadow, None);
        let light = Palette::of(&Theme::BATERI_LIGHT);
        assert_eq!(light.selected.rgb, 0xffffff);
        assert!(light.shadow.is_some());
        assert_eq!(light.title, Theme::BATERI_LIGHT.foreground);
        assert_eq!(light.warning, Theme::BATERI_LIGHT.warning);
        assert_eq!(light.accent, Theme::BATERI_LIGHT.accent);
        assert_eq!(light.success, Theme::BATERI_LIGHT.success);
        assert_eq!(light.error, Theme::BATERI_LIGHT.error);
        assert_eq!(light.card.rgb, Theme::BATERI_LIGHT.background);
        assert_eq!(dark.separator.rgb, Theme::BATERI.foreground);
    }

    /// A scrolled strip's edge fades out to the window's own background —
    /// the theme's, light or dark — or it would show as a band.
    #[test]
    fn the_strips_edges_fade_to_the_themes_background() {
        for theme in [Theme::BATERI, Theme::BATERI_LIGHT, Theme::LINEN] {
            assert_eq!(Palette::of(&theme).ground, theme.background);
        }
    }

    /// A Show All Tabs row's dot is drawn in the same role the chip's
    /// indicator wears for what it reports.
    #[test]
    fn a_list_dot_takes_the_colour_of_its_role() {
        let palette = Palette::of(&Theme::BATERI);
        assert_eq!(
            palette.tone(tabs::list_dot(Indicator::Running), 0),
            palette.accent
        );
        assert_eq!(
            palette.tone(tabs::list_dot(Indicator::Failed), 0),
            palette.error
        );
        assert_eq!(
            palette.tone(tabs::list_dot(Indicator::Finished), 0),
            palette.success
        );
    }

    /// A card line's colour is its tone's role; the host line is the mark's
    /// own colour, the dock's mapping.
    #[test]
    fn card_lines_wear_their_roles() {
        let palette = Palette::of(&Theme::BATERI);
        let mark = Theme::BATERI.mark_rgb(bt_core::HostMark::Production);
        assert_eq!(palette.tone(Tone::Error, mark), Theme::BATERI.error);
        assert_eq!(palette.tone(Tone::Accent, mark), Theme::BATERI.accent);
        assert_eq!(palette.tone(Tone::Dim, mark), Theme::BATERI.dim);
        assert_eq!(palette.tone(Tone::Mark, mark), Theme::BATERI.error);
    }

    /// The glyph's view is as high as a chip, so the icon's square sits in the middle of it with
    /// the badge's room above — the layout puts the view at the chip's top.
    #[test]
    fn the_glyph_is_as_high_as_a_chip() {
        assert_eq!(
            tabs::ICON_SIDE + 2.0 * GLYPH_OVERHANG,
            CHIP_HEIGHT,
            "a chip's height is the icon's square and the room for its badge on both sides"
        );
        // The badge and the ring cut round it fit that room above the tile, which stands taller
        // than the square, with a point to spare: a deck's front tile turns its corner up by less.
        const {
            assert!(
                BADGE_SIDE / 2.0 + BADGE_HALO - TILE_BADGE_INSET + 1.0
                    <= GLYPH_OVERHANG - (tabs::TILE_HEIGHT - tabs::ICON_SIDE) / 2.0
            )
        };
    }

    /// The question waits in the theme's `warning`, as its badge and its list dot wear it.
    #[test]
    fn a_question_wears_the_warning_role() {
        let palette = Palette::of(&Theme::BATERI);
        assert_eq!(
            palette.tone(tabs::list_dot(Indicator::Question), 0),
            palette.warning
        );
    }

    /// A tile is a raised face with the mark in its kind's colour; in brand colours a program that
    /// has one makes it the face, with a white mark, and the rest keep the theme's.
    #[test]
    fn a_tile_takes_its_kinds_colour_or_the_brand() {
        let palette = Palette::of(&Theme::BATERI);
        let task = icon::of(ProgramKind::Task);
        let claude = icon::of(ProgramKind::Agent(Agent::ClaudeCode));
        let codex = icon::of(ProgramKind::Agent(Agent::Codex));
        let theme = IconStyle {
            icons: true,
            colors: IconColors::Theme,
        };
        let brand = IconStyle {
            icons: true,
            colors: IconColors::Brand,
        };
        let task_ink = inks(task, theme, palette);
        assert_eq!(task_ink.mark, Tint::of(palette.tint(task.tint), 1.0));
        assert_eq!(task_ink.tile, Tint::of(palette.tile_face(), 1.0));
        assert_eq!(
            inks(claude, theme, palette).tile,
            Tint::of(palette.tile_face(), 1.0),
            "theme colours: no brand on a tile"
        );
        let branded = inks(claude, brand, palette);
        assert_eq!(branded.tile, Tint::of(claude.brand.unwrap(), 1.0));
        assert_eq!(branded.mark, Tint::of(0xffffff, 1.0));
        assert_eq!(
            inks(codex, brand, palette),
            inks(codex, theme, palette),
            "a one-colour mark does not change"
        );
        // A dark theme's tile is raised off its ground; a light theme's is white.
        assert_ne!(palette.tile_face(), palette.ground);
        assert_eq!(Palette::of(&Theme::BATERI_LIGHT).tile_face(), 0xffffff);
    }

    /// A kind's tint reads the palette: the foreground, a status role or a normal ANSI colour.
    #[test]
    fn a_kinds_tint_is_a_role_of_the_palette() {
        let theme = Theme::BATERI;
        let palette = Palette::of(&theme);
        assert_eq!(palette.tint(icon::Tint::Foreground), theme.foreground);
        assert_eq!(palette.tint(icon::Tint::Info), theme.info);
        assert_eq!(palette.tint(icon::Tint::Error), theme.error);
        assert_eq!(palette.tint(icon::Tint::Red), theme.ansi[1]);
        assert_eq!(palette.tint(icon::Tint::Green), theme.ansi[2]);
        assert_eq!(palette.tint(icon::Tint::Yellow), theme.ansi[3]);
        assert_eq!(palette.tint(icon::Tint::Blue), theme.ansi[4]);
        assert_eq!(palette.tint(icon::Tint::Magenta), theme.ansi[5]);
        assert_eq!(palette.tint(icon::Tint::Cyan), theme.ansi[6]);
        for kind in icon::KINDS {
            let _ = palette.tint(icon::of(kind).tint);
        }
    }

    /// Only a running glyph turns: a tile's edge, its deck's cards counted, or without icons the
    /// small indicator's ring.
    #[test]
    fn a_running_glyph_turns_its_arc_or_its_edge() {
        let look = |icons, indicator| {
            Some(GlyphLook {
                emblem: Emblem {
                    kind: ProgramKind::Shell,
                    edges: 1,
                },
                style: IconStyle {
                    icons,
                    colors: IconColors::Theme,
                },
                indicator,
                down: false,
            })
        };
        let running = Some(Indicator::Running);
        assert_eq!(spin_of(look(true, running)), Some(Spin::Edge(1)));
        assert_eq!(spin_of(look(false, running)), Some(Spin::Ring));
        assert_eq!(spin_of(look(true, Some(Indicator::Finished))), None);
        assert_eq!(spin_of(look(true, None)), None);
        assert_eq!(spin_of(None), None);
    }

    /// A badge is whatever the tab reports but a running command, which is the line round the
    /// tile; an arrow points down only for a download.
    #[test]
    fn a_badge_is_what_the_tab_reports_but_a_running_command() {
        let look = |indicator, down| {
            Some(GlyphLook {
                emblem: Emblem {
                    kind: ProgramKind::Shell,
                    edges: 0,
                },
                style: IconStyle::default(),
                indicator,
                down,
            })
        };
        assert_eq!(Badge::of(look(Some(Indicator::Running), false)), None);
        assert_eq!(Badge::of(look(None, false)), None);
        assert_eq!(Badge::of(None), None);
        let arrow = |down| Badge::of(look(Some(Indicator::Uploading), down));
        assert_eq!(arrow(false).map(|badge| badge.down), Some(false));
        assert_eq!(arrow(true).map(|badge| badge.down), Some(true));
        // Only a transfer has a direction: a tick is the same tick on a download's tab.
        let tick = Badge::of(look(Some(Indicator::Finished), true));
        assert_eq!(tick.map(|badge| badge.down), Some(false));
    }

    /// Every badge sits on its own circle, and signs are shared out by what they say; the
    /// failure is a dot without a sign, and smaller.
    #[test]
    fn each_badge_has_its_circle_and_its_sign() {
        let badge = |indicator, down| Badge { indicator, down };
        let mut discs: Vec<usize> = [
            Indicator::Question,
            Indicator::Finished,
            Indicator::Failed,
            Indicator::Uploading,
        ]
        .map(|indicator| badge(indicator, false).disc())
        .into();
        discs.sort_unstable();
        assert_eq!(discs, [0, 1, 2, 3]);
        for (disc, indicator) in DISCS.into_iter().enumerate() {
            assert_eq!(badge(indicator, false).disc(), disc);
        }
        assert_eq!(badge(Indicator::Failed, false).sign(), None);
        let signs = [
            badge(Indicator::Question, false).sign(),
            badge(Indicator::Finished, false).sign(),
            badge(Indicator::Uploading, false).sign(),
            badge(Indicator::Uploading, true).sign(),
        ];
        assert_eq!(signs, [Some(0), Some(1), Some(2), Some(3)]);
        assert_eq!(disc_side(badge(Indicator::Failed, false).disc()), BADGE_DOT);
        assert_eq!(
            disc_side(badge(Indicator::Finished, false).disc()),
            BADGE_SIDE
        );
    }

    /// The ring cut clear is the same width round a dot as round the bigger circle, so the dot's
    /// hollow is the smaller share of the picture's, and the biggest badge fills it.
    #[test]
    fn the_hollow_is_a_ring_of_the_same_width_round_every_badge() {
        let badge = |indicator| Badge {
            indicator,
            down: false,
        };
        assert_eq!(badge(Indicator::Finished).hollow_share(), 1.0);
        let dot = badge(Indicator::Failed).hollow_share();
        assert!(dot > BADGE_DOT / BADGE_SIDE && dot < 1.0, "{dot}");
        let radius = (BADGE_DOT / 2.0 + BADGE_HALO) / dot;
        assert!((radius - (BADGE_SIDE / 2.0 + BADGE_HALO)).abs() < 1e-9);
    }

    /// The tile's picture reaches all round the badge's middle as far as the tile, its shadow and
    /// its deck do, and the hollow's opaque part still reaches the tile's far corner when it is
    /// scaled to its smallest (a dot's, as it comes): otherwise the badge comes with a piece of the
    /// tile gone.
    #[test]
    fn the_pictures_reach_as_far_as_the_tile_does() {
        let slot = glyph_slot();
        let shadow = super::TILE_BLUR + super::TILE_DROP + 1.0;
        for edges in 0..=tabs::DECK_EDGES {
            let at = badge_centre(edges);
            let reach = |x: f64, y: f64| (x - at.x).abs().max((y - at.y).abs());
            let (left, right) = (slot.origin.x, slot.origin.x + slot.size.width);
            let (top, bottom) = (slot.origin.y, slot.origin.y + slot.size.height);
            // The deck's cards peek out to the right of the front tile.
            let deck = right + tabs::lead_width(tabs::TILE_WIDTH, edges) - tabs::TILE_WIDTH;
            let far = reach(left, bottom)
                .max(reach(deck, bottom))
                .max(reach(left, top))
                .max(reach(deck, top))
                + shadow;
            assert!(far <= TILE_ROOM, "{edges} cards: {far} > {TILE_ROOM}");
            let smallest = tabs::BADGE_FROM
                * Badge {
                    indicator: Indicator::Failed,
                    down: false,
                }
                .hollow_share();
            assert!(HOLLOW_ROOM * smallest >= far, "{edges} cards");
        }
    }

    /// The badge stands on the tile's corner, and on a deck leans with the front card about the
    /// tile's middle: it keeps its distance from it.
    #[test]
    fn the_badge_stands_on_the_front_cards_corner() {
        let slot = glyph_slot();
        let flat = badge_centre(0);
        assert!((flat.x - (slot.origin.x + slot.size.width - TILE_BADGE_INSET)).abs() < 1e-9);
        assert!((flat.y - (slot.origin.y + TILE_BADGE_INSET)).abs() < 1e-9);
        let middle = (
            slot.origin.x + slot.size.width / 2.0,
            slot.origin.y + slot.size.height / 2.0,
        );
        let away = |at: NSPoint| ((at.x - middle.0).powi(2) + (at.y - middle.1).powi(2)).sqrt();
        let leaning = badge_centre(tabs::DECK_EDGES);
        assert!((away(leaning) - away(flat)).abs() < 1e-9);
        // Counter-clockwise on screen: the corner at the top right moves up and left.
        assert!(
            leaning.y < flat.y && leaning.x < flat.x,
            "{leaning:?} {flat:?}"
        );
    }

    /// The lines run inside a chip's rounded corners and never go negative.
    #[test]
    fn a_chips_lines_stay_inside_its_corners() {
        assert_eq!(line_span(184.0), (TAB_RADIUS, 184.0 - 2.0 * TAB_RADIUS));
        assert_eq!(line_span(4.0).1, 0.0);
    }

    /// The card hangs under its chip and stays inside the window.
    #[test]
    fn the_card_stays_inside_the_window() {
        assert_eq!(card_x(270.0, 1000.0, tabs::CARD_WIDTH), 270.0);
        assert_eq!(
            card_x(900.0, 1000.0, tabs::CARD_WIDTH),
            1000.0 - tabs::CARD_WIDTH - CARD_PAD_X
        );
        assert_eq!(
            card_x(900.0, 1000.0, tabs::CARD_MAP_WIDTH),
            1000.0 - tabs::CARD_MAP_WIDTH - CARD_PAD_X,
            "the wider card with a picture"
        );
        assert_eq!(
            card_x(-40.0, 1000.0, tabs::CARD_WIDTH),
            CARD_PAD_X,
            "a strip scrolled off"
        );
        assert_eq!(
            card_x(100.0, 200.0, tabs::CARD_WIDTH),
            CARD_PAD_X,
            "a window narrower than the card"
        );
    }

    /// A card beside a row stands right of the column at the row's height and slides to stay
    /// inside the window.
    #[test]
    fn a_card_beside_its_row_stays_inside_the_window() {
        let window = (1000.0, 700.0);
        let card = (300.0, 200.0);
        // At the row, a little above it, [`CARD_BESIDE`] right of the column.
        assert_eq!(
            card_beside(120.0, 200.0, window, card),
            (200.0 + CARD_BESIDE, 120.0 - CARD_RISE)
        );
        // A row near the bottom: the card slides up to clear the window's foot.
        assert_eq!(
            card_beside(650.0, 200.0, window, card).1,
            700.0 - 200.0 - CARD_EDGE
        );
        // A row under the window's top: the card keeps off the top.
        assert_eq!(card_beside(2.0, 200.0, window, card).1, CARD_EDGE);
        // A card taller than the window starts at the top (that edge wins).
        assert_eq!(
            card_beside(300.0, 200.0, window, (300.0, 900.0)).1,
            CARD_EDGE
        );
        // A window too narrow for the card: pushed left, but never over the column.
        assert_eq!(card_beside(120.0, 200.0, (400.0, 700.0), card).0, 200.0);
        assert_eq!(
            card_beside(120.0, 200.0, (510.0, 700.0), card).0,
            510.0 - 300.0 - CARD_BESIDE
        );
    }

    /// The fill is the accent at 7 % to 14 %, the error's once the transfer failed; the way still
    /// to go is the hover's own tone — on the bar's ladder of grounds the fill's two ends are the
    /// hover's (the foreground at 5 %) and the selected face's (10 %) in both themes, so the
    /// strengths are the ladder's and nothing here is a new tone.
    #[test]
    fn a_fill_runs_from_seven_to_fourteen_percent_of_its_colour() {
        for theme in [Theme::BATERI, Theme::BATERI_LIGHT] {
            let palette = Palette::of(&theme);
            assert_eq!(
                palette.fill_tints(false),
                (Tint::of(theme.accent, 0.07), Tint::of(theme.accent, 0.14))
            );
            assert_eq!(
                palette.fill_tints(true),
                (Tint::of(theme.error, 0.07), Tint::of(theme.error, 0.14))
            );
            assert_eq!(palette.hover, Tint::of(theme.foreground, 0.05));
        }
        assert_eq!(
            Palette::of(&Theme::BATERI).selected,
            Tint::of(Theme::BATERI.foreground, 0.10)
        );
    }

    /// Reduce Motion: every duration is zero, the bar changes at once.
    #[test]
    fn reduce_motion_makes_every_motion_instant() {
        assert_eq!(
            Motion::of(true),
            Motion {
                hover: 0.0,
                close: 0.0,
                reflow: 0.0
            }
        );
        let motion = Motion::of(false);
        assert_eq!(
            (motion.hover, motion.close, motion.reflow),
            (0.08, 0.12, 0.2),
            "the design's 80, 120 and 200 ms"
        );
    }

    /// A card arrives from the middle of the edge that faces its chip: that point stays where it
    /// is, the rest of the card is [`CARD_SCALE`] of its size round it, and all of it starts a
    /// [`CARD_SLIDE`] nearer the chip.
    #[test]
    fn a_card_arrives_about_the_edge_facing_its_chip() {
        let near = |got: f64, want: f64| assert!((got - want).abs() < 1e-9, "{got} is not {want}");
        let frame = Rect::new(100.0, 50.0, 272.0, 120.0);
        // Under its chip, in the top bar: the top edge's middle, 4 pt up.
        let look = card_arrival(frame, CHIP_ABOVE);
        near(look.width, 272.0 * CARD_SCALE);
        near(look.height, 120.0 * CARD_SCALE);
        near(look.x + look.width / 2.0, 100.0 + 136.0);
        near(look.y, 50.0 - CARD_SLIDE);
        // Beside its row, on the row's right: the left edge's middle, 4 pt left.
        let look = card_arrival(frame, ROW_LEFT);
        near(look.x, 100.0 - CARD_SLIDE);
        near(look.y + look.height / 2.0, 50.0 + 60.0);
        near(look.width, 272.0 * CARD_SCALE);
    }

    // ─── a map tile with its pane's picture ──────────────────────────────

    use std::ptr::null_mut;

    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSBezierPath, NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace,
        NSGraphicsContext, NSImage,
    };
    use objc2_core_graphics::CGContext;

    /// The tile the paint is drawn in, points, at 2x.
    const TILE: (f64, f64) = (100.0, 60.0);

    /// What `draw` leaves in a transparent `TILE` at 2x — in a context flipped as a view's is (the
    /// origin top left, `y` down), as a tile is drawn in — as `(width, rows from the top,
    /// premultiplied RGBA)`.
    fn in_a_flipped_view(draw: impl FnOnce()) -> (usize, Vec<[u8; 4]>) {
        let (wide, high) = (TILE.0 as usize * 2, TILE.1 as usize * 2);
        // SAFETY: no planes of our own (the bitmap allocates them), 8-bit RGBA at the size given,
        // and a constant colour space name AppKit exposes.
        let rep = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                null_mut(),
                wide as isize,
                high as isize,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                0,
                0,
            )
        }
        .expect("a bitmap");
        rep.setSize(NSSize::new(TILE.0, TILE.1));
        let upright = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)
            .expect("a context on the bitmap");
        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(Some(&upright));
        // Transparent to begin with: a new bitmap holds whatever was in the memory.
        upright.setCompositingOperation(NSCompositingOperation::Clear);
        NSBezierPath::fillRect(NSRect::new(NSPoint::ZERO, NSSize::new(TILE.0, TILE.1)));
        upright.setCompositingOperation(NSCompositingOperation::SourceOver);
        // What AppKit does for a flipped view: the transform that turns `y` down, and a context
        // that says so.
        let cg = upright.CGContext();
        CGContext::translate_ctm(Some(&cg), 0.0, TILE.1);
        CGContext::scale_ctm(Some(&cg), 1.0, -1.0);
        let flipped = NSGraphicsContext::graphicsContextWithCGContext_flipped(&cg, true);
        NSGraphicsContext::setCurrentContext(Some(&flipped));
        draw();
        NSGraphicsContext::restoreGraphicsState_class();
        let row = rep.bytesPerRow() as usize;
        // SAFETY: the bitmap owns `row * high` bytes at `bitmapData`, which `rep` keeps alive.
        let bytes = unsafe { std::slice::from_raw_parts(rep.bitmapData(), row * high) };
        let pixels = (0..high)
            .flat_map(|y| (0..wide).map(move |x| (x, y)))
            .map(|(x, y)| {
                let at = y * row + x * 4;
                [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]
            })
            .collect();
        (wide, pixels)
    }

    /// The pixel at `(x, y)` points, `y` down from the top.
    fn at(picture: &(usize, Vec<[u8; 4]>), x: f64, y: f64) -> [u8; 4] {
        picture.1[(y * 2.0) as usize * picture.0 + (x * 2.0) as usize]
    }

    /// A pane's picture, 200 × 120 pixels: red down to 40 pt, blue below.
    fn a_picture() -> Retained<NSImage> {
        let rgba: Vec<u8> = (0..120)
            .flat_map(|y| {
                (0..200).flat_map(move |_| {
                    if y < 80 {
                        [255, 0, 0, 255]
                    } else {
                        [0, 0, 255, 255]
                    }
                })
            })
            .collect();
        let still = bt_gpu::Snapshot {
            width: 200,
            height: 120,
            rgba,
        };
        let image = crate::snapshot::image(&still).expect("an image");
        NSImage::initWithCGImage_size(NSImage::alloc(), &image, NSSize::new(TILE.0, TILE.1))
    }

    fn painted(fraction: f64, ring: Option<Tint>) -> (usize, Vec<[u8; 4]>) {
        let scrim = (0x00ff00, MAP_SCRIM_NAME_ONLY);
        let image = a_picture();
        in_a_flipped_view(|| {
            paint_tile(
                NSRect::new(NSPoint::ZERO, NSSize::new(TILE.0, TILE.1)),
                Tint::of(0xffffff, 0.08),
                Some(TileShot {
                    image: &image,
                    fraction,
                    scrim,
                }),
                ring,
            )
        })
    }

    /// The picture stands the right way up in a flipped view, the scrim of the face's colour
    /// hides its top under the tile's name, the ring is over the picture, and the picture keeps
    /// to the tile's rounded corners.
    #[test]
    fn a_tile_wears_its_picture_under_a_scrim_and_a_ring() {
        let picture = painted(1.0, Some(Tint::of(0xffffff, 1.0)));
        // Below the scrim (30 pt): the picture's top half is red and its bottom blue — the way
        // up it was made, in a view that is flipped.
        let (red, blue) = (at(&picture, 50.0, 36.0), at(&picture, 50.0, 55.0));
        assert!(
            red[0] > 240 && red[1] < 15 && red[2] < 15,
            "the top of the picture is not at the top: {red:?}"
        );
        assert!(
            blue[2] > 240 && blue[0] < 15 && blue[1] < 15,
            "the bottom of the picture is not at the bottom: {blue:?}"
        );
        // The scrim's solid part (the first 55 % of 30 pt, 16.5 pt) is the face's colour over the
        // picture, 94 % of it: a little of the red is still there.
        let solid = at(&picture, 50.0, 8.0);
        assert!(
            solid[1] > 225 && solid[0] > 5 && solid[0] < 40,
            "the scrim's solid part: {solid:?}"
        );
        // Its foot fades to the picture: halfway down the fade is both.
        let fading = at(&picture, 50.0, 23.0);
        assert!(
            fading[0] > 60 && fading[1] > 60 && fading[0] < 220 && fading[1] < 220,
            "the scrim does not fade out: {fading:?}"
        );
        // The ring is over the picture: a white line along the tile's inside edge, where the
        // picture is blue.
        let ring = at(&picture, 0.25, 45.0);
        assert!(
            ring[0] > 240 && ring[1] > 240 && ring[2] > 240,
            "the picture covers the ring: {ring:?}"
        );
        let inside = at(&picture, 3.0, 45.0);
        assert!(inside[2] > 240 && inside[0] < 15, "{inside:?}");
        // The corner is rounded: nothing is painted in the tile's very corner.
        assert_eq!(
            at(&picture, 0.0, 0.0)[3],
            0,
            "the picture is square at the corner"
        );
        assert_eq!(at(&picture, 99.5, 59.5)[3], 0);
    }

    /// The picture lies over the face at 86 % until the pointer is on the tile, then all of it.
    #[test]
    fn a_tile_wears_its_picture_dimmed_until_the_pointer_is_on_it() {
        let alpha = |fraction| at(&painted(fraction, None), 50.0, 55.0)[3];
        let (dim, lit) = (alpha(0.86), alpha(1.0));
        assert!(lit >= 253, "the lit picture is not whole: {lit}");
        // The face (8 %) shows through the rest: 1 − 0.92 · 0.14 of full.
        assert!((215..=230).contains(&dim), "the dimmed picture is at {dim}");
    }

    /// ⌘ alone shows the hints, Caps Lock or not; a chord's other modifier
    /// hides them.
    #[test]
    fn command_alone_ignores_caps_lock() {
        let command = NSEventModifierFlags::Command;
        assert!(command_alone(command));
        assert!(command_alone(command | NSEventModifierFlags::CapsLock));
        assert!(command_alone(command | NSEventModifierFlags::Function));
        assert!(!command_alone(command | NSEventModifierFlags::Shift));
        assert!(!command_alone(command | NSEventModifierFlags::Option));
        assert!(!command_alone(NSEventModifierFlags::empty()));
    }

    /// ⇧ picks a range and ⌘ one row, alone or with Caps Lock; ⌥⌘ is the arrangement's and ⌃ opens
    /// a context menu, so neither picks; no modifier is a plain press.
    #[test]
    fn shift_and_command_pick_and_nothing_else_does() {
        let (shift, command) = (NSEventModifierFlags::Shift, NSEventModifierFlags::Command);
        assert_eq!(pick_click(shift), Some(Click::Range));
        assert_eq!(pick_click(command), Some(Click::Toggle));
        assert_eq!(pick_click(command | shift), Some(Click::Range));
        assert_eq!(
            pick_click(command | NSEventModifierFlags::CapsLock),
            Some(Click::Toggle)
        );
        assert_eq!(pick_click(NSEventModifierFlags::empty()), None);
        assert_eq!(pick_click(NSEventModifierFlags::CapsLock), None);
        assert_eq!(pick_click(command | NSEventModifierFlags::Option), None);
        assert_eq!(pick_click(shift | NSEventModifierFlags::Control), None);
    }

    fn row(tab: u64) -> Label {
        Label {
            tab,
            title: format!("tab {tab}"),
            indicator: None,
            emblem: Emblem {
                kind: ProgramKind::Shell,
                edges: 1,
            },
            running: None,
            upload: None,
            mark: bt_core::HostMark::None,
        }
    }

    /// The picks live with the tab on screen they were made from: another tab on screen, a mode
    /// without a column, a tab no longer listed, or what is left being the tab on screen alone
    /// ends them; otherwise what is listed stays.
    #[test]
    fn picks_hold_only_while_the_tab_they_count_from_is_on_screen() {
        let rows: Vec<Label> = (1..=5).map(row).collect();
        let picks = Picks {
            tabs: vec![2, 3, 4],
            shown: Some(2),
        };
        // The tab on screen is the third row's tab 3: the picks were made from tab 2.
        assert_eq!(kept_picks(&picks, &rows, 2, true), Picks::default());
        assert_eq!(kept_picks(&picks, &rows, 1, true), picks);
        // In the title row nothing is picked together.
        assert_eq!(kept_picks(&picks, &rows, 1, false), Picks::default());
        // A closed tab or a closed group's hidden tab leaves the picks.
        let fewer: Vec<Label> = [1, 2, 4, 5].into_iter().map(row).collect();
        assert_eq!(
            kept_picks(&picks, &fewer, 1, true),
            Picks {
                tabs: vec![2, 4],
                shown: Some(2),
            }
        );
        // What is left being the tab on screen alone is no selection; none left is none.
        let alone: Vec<Label> = [1, 2, 5].into_iter().map(row).collect();
        assert_eq!(kept_picks(&picks, &alone, 1, true), Picks::default());
        let none: Vec<Label> = [1, 2, 5].into_iter().map(row).collect();
        let others = Picks {
            tabs: vec![3, 4],
            shown: Some(2),
        };
        assert_eq!(kept_picks(&others, &none, 1, true), Picks::default());
        // A pick that is not the tab on screen survives alone: ⌘ took the tab on screen out.
        let apart = Picks {
            tabs: vec![4],
            shown: Some(2),
        };
        assert_eq!(kept_picks(&apart, &rows, 1, true), apart);
    }

    #[test]
    fn menu_titles_count_the_tabs() {
        assert_eq!(counted("Close", 3, "\u{2026}"), "Close 3 Tabs\u{2026}");
        assert_eq!(counted("Move", 2, " to Group"), "Move 2 Tabs to Group");
        assert_eq!(counted("Close", 1, ""), "Close 1 Tab");
    }
    /// The line that marks a carried drop's place is kept in the list's content, so a layout that
    /// finds the column scrolled somewhere else puts it back at the same place of the rows.
    #[test]
    fn a_drops_line_is_kept_in_the_content_and_comes_back_at_any_scroll() {
        let column = |scroll: f64| {
            tabs::Side {
                width: tabs::SIDE_WIDTH,
                height: 200.0,
                top: 40.0,
                count: 12,
                scroll,
                warning: false,
                corner: 20.5,
            }
            .layout()
        };
        let still = column(0.0);
        // The gap above the fifth row, as the column says it.
        let line = still.between(4);
        let content = content_line(line, &still);
        assert_eq!(content, line - still.list.y);
        // Scrolled 30 points: the same place of the rows is 30 higher in the window.
        let moved = column(30.0);
        assert_eq!(moved.scroll, 30.0);
        assert_eq!(moved.between(4), line - 30.0);
        assert_eq!(
            content_line(moved.between(4), &moved),
            content,
            "the content's height does not move with the wheel"
        );
        // What the layout draws it at: the list's space.
        assert_eq!(content - moved.scroll, moved.between(4) - moved.list.y);
    }

    /// What a carry shows over a bar and what the bar takes the drop as are read by one function:
    /// were each to read the pointer for itself, room or a line would open at one place and the
    /// tabs go to another.
    #[test]
    fn a_carry_over_a_bar_shows_what_the_drop_does() {
        let source = include_str!("tab_bar.rs");
        let body = |name: &str| {
            let at = source
                .find(&format!("    fn {name}("))
                .unwrap_or_else(|| panic!("{name} is a method of the bar"));
            let rest = &source[at..];
            &rest[..rest.find("\n    }\n").expect("the method ends")]
        };
        for name in ["carried_over", "accept_drop"] {
            assert!(
                body(name).contains("self.aim("),
                "{name} reads the carry's place from `aim`, as its other half does"
            );
        }
    }
}
