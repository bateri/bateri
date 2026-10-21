//! The command palette: a card over the window's content, one field, one list.
//!
//! **What it lists** is the application menu ([`menu::Snapshot`]): a command is a menu item, so a
//! command exists in the palette exactly when it exists in the menu, with the title, the shortcut
//! and the enabled state the menu would show — read before the palette takes the keyboard, because
//! it is the pane that tells the menu what is on (`validateMenuItem:` goes down the responder
//! chain from the first responder). What a query finds, in which order and under which
//! heading, and what the keys do to the highlight and the pages are `bt-shell-common`'s
//! ([`rules`]): this module draws the rows and feeds the machine ([`Palette`]) its keys.
//!
//! **Settings and themes** are rows of their own beside the menu's commands: a switch flips where
//! it stands and the list stays, a setting with values and the themes open a page whose
//! highlighted row is **shown** on every window as it is reached (`Effect::Preview` →
//! [`AppDelegate::preview_edit`](crate::app)), ⏎ writes it to `settings.toml` through the one
//! write path the menus and the settings window use, and anything that closes the palette takes
//! the preview back. A file that takes no write shows why above the page and previews nothing.
//!
//! **Tabs and groups** are rows too ([`bt_shell_common::palette::Places`], read from every window
//! when the palette opens): ⇧⌘P finds them by what they are called, where they are and which
//! group they are in; ⌘P ([`go_to_tab`]) opens the palette **on** the tabs — the one before the tab
//! on screen highlighted — and, open, takes the highlight one tab further. ⏎ goes there, → opens
//! the row's menu (what the tab's chip and the group's header offer), and what is chosen is done
//! a turn after the palette is down ([`AppDelegate::palette_act`](crate::app)). A tab's icon is
//! the one the bar draws ([`palette_icon`]).
//!
//! **Folders, servers and ports** ([`bt_shell_common::palette::Targets`], read when the palette
//! opens): ⌘O ([`go_to_folder`]) opens the palette on the folders — the directories panes
//! reported this run, newest first — and a field that holds a path (`~/Pro`, `/usr/lo`) lists the
//! folders inside the one it points into, read off the main thread ([`Open::listing`]); ⇥ goes
//! inside. ⏎ on a folder opens a new tab in it and ⌥⏎ pastes `cd '…'` on the line of the pane on
//! screen; ⏎ on a server opens a new local tab with `ssh host` left on the line; **no byte that
//! runs is sent anywhere** ([`AppDelegate::palette_go`](crate::app)). A port opens in the browser,
//! ⌘⏎ copies its address. `~/.ssh/config` is read in the background every time the palette opens.
//!
//! **Running a row** is the menu's own act: the palette closes, the keyboard goes back to the tab
//! on screen, and only then is the item's action sent — to the item's target, or down the
//! responder chain from that pane — with the item as the sender, so a tagged item (a direction,
//! a tab) reads its tag as from the menu.
//!
//! **Where it stands**: in the middle of the window's content ([`RootView::content`](crate::window)),
//! a few points under the top, at most [`MAX_WIDTH`] wide; with the tabs in the side column the
//! middle is the middle of what is right of the column. Its position and width are taken when it
//! opens and kept while it is open; only the window's resizing moves it. Its height follows the
//! list up to [`MAX_LIST`].
//!
//! **How it looks** comes from the theme's own arithmetic: the surface is the dock's plate
//! ([`Theme::dock_ground_srgb`]), the bottom band the step above it ([`Theme::band_srgb`]), the
//! highlight and the letters the query matched the accent. It opens and closes in
//! [`APPEAR_SECS`] on the search panel's curves — at once under Reduce Motion; while it fades out
//! the keyboard and the pointer are the tab's already.
//!
//! **The keyboard** is the field's: `control:textView:doCommandBySelector:` carries the arrows
//! (⌃P and ⌃N are `moveUp:` and `moveDown:` too), ⏎, esc, ⇥, → at the end of the text, and ← and ⌫
//! when the field is empty; everything else is the field editor's, IME included. The **pointer**
//! highlights a row only when it moves (`mouseMoved:`), never because a list slid under it; a
//! press runs the row.
//!
//! The palette is the window's and closes when the window stops being key (a question
//! opened over it, another application in front), when a press lands outside it, and at esc on
//! the root.

mod menu;

pub(crate) use menu::nodes as menu_nodes;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::OnceLock;

use block2::RcBlock;
use bt_core::{Settings, Theme};
use bt_shell_common::palette::{
    self as rules, Chosen, Commands, Effect, Fallbacks, Key, Kind, List, Modifier, Palette,
    PathQuery, Places, Row, Scope, Sources, Swatch, Targets, ThemeEntry,
};
use dispatch2::{DispatchQueue, DispatchRetained, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSApplication,
    NSAttributedStringNSStringDrawing, NSAutoresizingMaskOptions, NSBezierPath, NSBorderType,
    NSCompositingOperation, NSControlTextEditingDelegate, NSEvent, NSEventModifierFlags,
    NSFocusRingType, NSFont, NSFontAttributeName, NSFontWeightMedium, NSFontWeightSemibold,
    NSForegroundColorAttributeName, NSImage, NSScrollView, NSShadow, NSStringDrawing, NSTextField,
    NSTextFieldDelegate, NSTrackingArea, NSTrackingAreaOptions, NSView, NSWindowOrderingMode,
};
use objc2_foundation::{
    NSAttributedString, NSAttributedStringKey, NSDictionary, NSMutableAttributedString,
    NSNotification, NSObjectProtocol, NSPoint, NSRange, NSRect, NSSize, NSString,
};
use objc2_quartz_core::{
    CAMediaTimingFunction, kCAMediaTimingFunctionEaseIn, kCAMediaTimingFunctionEaseOut,
};

use self::menu::Snapshot;
use crate::app::{self, Origin};
use crate::tab_bar::{PALETTE_ICON_SIDE, Tint, inner_line, palette_icon, rounded};
use crate::window::{RootView, TerminalWindow};

// ─── Measures ─────────────────────────────────────────────────────────────

/// The card's widest: a command's title, its place in the menu and its shortcut fit with room.
pub(crate) const MAX_WIDTH: f64 = 600.0;
/// The least room between the card and the content's left and right edges.
const SIDE_MARGIN: f64 = 16.0;
/// The distance from the content's top to the card's.
const TOP: f64 = 8.0;
/// The field's band, its rule included.
const FIELD_BAND: f64 = 52.0;
/// The bottom band that names the keys.
const FOOTER: f64 = 36.0;
/// A row's height and a heading's.
const ROW: f64 = 32.0;
const HEADING: f64 = 26.0;
/// The air above the first row and below the last.
const LIST_PAD: f64 = 6.0;
/// The list's tallest: about ten rows; a longer list scrolls.
const MAX_LIST: f64 = 360.0;
/// What an empty list says takes this much room.
const EMPTY_BAND: f64 = 72.0;
const RADIUS: f64 = 12.0;
const ROW_RADIUS: f64 = 8.0;
/// A row's highlight stands this far in from the list's sides; its text, this far from the card's.
const ROW_INSET: f64 = 8.0;
const TEXT_INSET: f64 = 20.0;
const FIELD_INSET: f64 = 20.0;

const TITLE_SIZE: f64 = 14.0;
const DIM_SIZE: f64 = 12.5;
const HEADING_SIZE: f64 = 11.0;
const FIELD_SIZE: f64 = 16.0;
const FOOTER_SIZE: f64 = 11.5;

/// The duration of the opening and of the closing, in seconds: the search panel's
/// (`search_bar::APPEAR_SECS`).
const APPEAR_SECS: f64 = 0.18;
/// How far above its place the card begins, and rises to as it goes.
const SLIDE: f64 = 6.0;

/// A theme's colours beside its title: a rounded patch of its ground with six dots on it.
const SWATCH_WIDTH: f64 = 72.0;
const SWATCH_HEIGHT: f64 = 18.0;
const SWATCH_DOT: f64 = 7.0;
const SWATCH_GAP: f64 = 3.6;
/// The space between a tab's icon and its title.
const ICON_GAP: f64 = 9.0;

const RETURN: u16 = 36;
const ENTER: u16 = 76;

// ─── Where things stand (pure) ────────────────────────────────────────────

/// The card's frame in the root's flipped space, for a list `list` points high, over the
/// `content` rectangle the tabs leave: centred in it, [`TOP`] under its top, no wider than
/// [`MAX_WIDTH`] and [`SIDE_MARGIN`] clear of its sides, and no taller than the content.
fn card_frame(content: NSRect, list: f64) -> NSRect {
    let width = MAX_WIDTH.min((content.size.width - 2.0 * SIDE_MARGIN).max(0.0));
    let wanted = FIELD_BAND + list + FOOTER;
    let room = (content.size.height - TOP - SIDE_MARGIN).max(FIELD_BAND + FOOTER);
    NSRect::new(
        NSPoint::new(
            content.origin.x + (content.size.width - width) / 2.0,
            content.origin.y + TOP,
        ),
        NSSize::new(width, wanted.min(room)),
    )
}

/// What a slot of the list holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Line {
    Row(usize),
    /// The heading that stands over row `n`.
    Heading(usize),
}

/// A line of the list: a row or the heading above some.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Slot {
    top: f64,
    height: f64,
    line: Line,
}

/// The slots of `list` from the top, and the height they and their padding take.
fn slots(list: &List) -> (Vec<Slot>, f64) {
    let mut out = Vec::with_capacity(list.len() + list.headings.len());
    let mut top = LIST_PAD;
    for index in 0..list.len() {
        if list.heading_before(index).is_some() {
            out.push(Slot {
                top,
                height: HEADING,
                line: Line::Heading(index),
            });
            top += HEADING;
        }
        out.push(Slot {
            top,
            height: ROW,
            line: Line::Row(index),
        });
        top += ROW;
    }
    (out, top + LIST_PAD)
}

/// The height of the list's window for `total` points of slots: all of them up to [`MAX_LIST`],
/// and the band an empty list's message takes when there is none.
fn list_height(total: f64, empty: bool) -> f64 {
    if empty {
        EMPTY_BAND
    } else {
        total.min(MAX_LIST)
    }
}

/// The row under `y` (in the list's space), if one is.
fn row_at(slots: &[Slot], y: f64) -> Option<usize> {
    slots
        .iter()
        .find(|slot| y >= slot.top && y < slot.top + slot.height)
        .and_then(|slot| match slot.line {
            Line::Row(index) => Some(index),
            Line::Heading(_) => None,
        })
}

/// The slot of row `index`.
fn slot_of(slots: &[Slot], index: usize) -> Option<Slot> {
    slots
        .iter()
        .find(|slot| slot.line == Line::Row(index))
        .copied()
}

/// The keys the bottom band names for the highlighted `row`, at page `depth`. `appearance`: the
/// system's, when ⌥⏎ on a theme row writes the theme for it only.
fn hints(
    row: Option<&Row>,
    depth: usize,
    appearance: Option<&'static str>,
) -> Vec<(&'static str, String)> {
    let mut hints = Vec::new();
    if let Some(row) = row {
        // A command says its own keys: where it ran and whether its output is still there decide
        // what each does.
        hints.extend(row.hints.iter().cloned());
        let verb = if !row.hints.is_empty() {
            ""
        } else if row.opens.is_some() && row.enters {
            "Open"
        } else if matches!(row.kind, Kind::Tab | Kind::Group) {
            "Go to"
        } else if row.kind == Kind::Folder {
            "New Tab Here"
        } else if row.kind == Kind::Host {
            "ssh in New Tab"
        } else if row.kind == Kind::Port {
            "Open"
        } else if row.kind == Kind::Search {
            "Find"
        } else if matches!(row.kind, Kind::Theme | Kind::Choice) {
            "Use"
        } else if row.toggle.is_some() {
            "Switch"
        } else {
            "Run"
        };
        if !verb.is_empty() {
            hints.push(("↵", verb.to_owned()));
        }
        // A row that does something by ⏎ and keeps a menu of its own behind →.
        if row.opens.is_some() && !row.enters {
            hints.push(("→", "Actions".to_owned()));
        }
        if let Some(appearance) = appearance
            && row.kind == Kind::Theme
            && row.id != "theme.system"
        {
            hints.push(("⌥↵", format!("Use for {appearance}")));
        }
        // What the other keys do to a folder and to a port.
        match row.kind {
            Kind::Folder => {
                hints.push(("⌥↵", "Type cd".to_owned()));
                hints.push(("⌘↵", "Show in Finder".to_owned()));
                // The folder the field is already inside (`~/Desktop/`'s own row) has nowhere
                // for ⇥ to step.
                if row.completion.is_some() {
                    hints.push(("⇥", "Go inside".to_owned()));
                }
            }
            Kind::Host => hints.push(("⌥↵", "ssh in Split".to_owned())),
            Kind::Port => hints.push(("⌘↵", "Copy URL".to_owned())),
            _ => {}
        }
    }
    hints.push(("↑↓", "Move".to_owned()));
    hints.push(("esc", if depth > 0 { "Back" } else { "Close" }.to_owned()));
    hints
}

/// What the list says when it has no rows. `looking`: a folder's names are being read.
fn empty_message(scope: Option<&Scope>, depth: usize, query: &str, looking: bool) -> &'static str {
    if looking {
        return "Reading the folder…";
    }
    let typed = !query.trim().is_empty();
    match scope {
        Some(Scope::Folders) if !typed => "No folders yet: type a path such as ~/",
        Some(Scope::Hosts) if !typed => "No servers yet: add Host lines to ~/.ssh/config",
        Some(Scope::Ports) if !typed => "Nothing in your tabs is listening",
        Some(Scope::History { jump: true, .. }) if !typed => "No command output to jump to",
        Some(Scope::History { .. }) if !typed => "Nothing has run in your tabs yet",
        _ if typed || depth > 0 => "No matches",
        _ => "Type a command, or press ↓ ↵ to run a recent one",
    }
}

// ─── Colours ──────────────────────────────────────────────────────────────

/// The palette's colours, all from the theme.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    ground: Tint,
    band: Tint,
    line: Tint,
    text: Tint,
    dim: Tint,
    accent: Tint,
    /// The theme's status roles, for a row's status text.
    success: Tint,
    warning: Tint,
    error: Tint,
    lit: Tint,
    shadow: Tint,
}

impl Look {
    fn of(theme: &Theme) -> Look {
        let hex = |[r, g, b]: [u8; 3]| (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
        let dark = theme.is_dark();
        Look {
            ground: Tint::of(hex(theme.dock_ground_srgb()), 1.0),
            band: Tint::of(hex(theme.band_srgb()), 1.0),
            line: Tint::of(theme.foreground, if dark { 0.14 } else { 0.12 }),
            text: Tint::of(theme.foreground, 1.0),
            dim: Tint::of(theme.dim, 1.0),
            accent: Tint::of(theme.accent, 1.0),
            success: Tint::of(theme.success, 1.0),
            warning: Tint::of(theme.warning, 1.0),
            error: Tint::of(theme.error, 1.0),
            lit: Tint::of(theme.accent, if dark { 0.22 } else { 0.15 }),
            shadow: Tint::of(0x000000, if dark { 0.5 } else { 0.18 }),
        }
    }
}

// ─── Text ─────────────────────────────────────────────────────────────────

type Attributes = Retained<NSDictionary<NSAttributedStringKey, AnyObject>>;

/// A font and a colour as the attributes AppKit draws a string with.
fn attributes(font: &NSFont, color: Tint) -> Attributes {
    let color = color.color();
    let values: [&AnyObject; 2] = [color.as_ref(), font.as_ref()];
    // SAFETY: AppKit's two attribute keys (extern statics), each with the value type it
    // documents — an `NSColor` and an `NSFont`.
    unsafe {
        NSDictionary::<NSAttributedStringKey, AnyObject>::from_slices(
            &[NSForegroundColorAttributeName, NSFontAttributeName],
            &values,
        )
    }
}

fn text_width(text: &str, attributes: &Attributes) -> f64 {
    // SAFETY: `attributes` is a dictionary of the keys and value types `sizeWithAttributes:` reads.
    unsafe { NSString::from_str(text).sizeWithAttributes(Some(attributes)) }.width
}

/// Draws `text` with its left edge at `x`, centred in the slot `[top, top + height)`; returns
/// where it ended.
fn draw_text(text: &str, x: f64, top: f64, height: f64, attributes: &Attributes) -> f64 {
    let string = NSString::from_str(text);
    // SAFETY: as `text_width`.
    let size = unsafe { string.sizeWithAttributes(Some(attributes)) };
    // SAFETY: as `text_width`; drawing happens inside the view's `drawRect:`.
    unsafe {
        string.drawAtPoint_withAttributes(
            NSPoint::new(x, top + (height - size.height) / 2.0),
            Some(attributes),
        );
    }
    x + size.width
}

/// Draws `text` ending at `right`; returns where it began.
fn draw_text_right(text: &str, right: f64, top: f64, height: f64, attributes: &Attributes) -> f64 {
    let left = right - text_width(text, attributes);
    draw_text(text, left, top, height, attributes);
    left
}

/// `chars` as an attributed string with the `marks` in `marked`, cut to `keep` characters and
/// ended with an ellipsis when that is fewer than there are.
fn styled(
    chars: &[char],
    marks: &[usize],
    keep: usize,
    plain: &Attributes,
    marked: &Attributes,
) -> Retained<NSMutableAttributedString> {
    let out = NSMutableAttributedString::new();
    let mut run = String::new();
    let mut run_marked = false;
    let flush = |run: &mut String, run_marked: bool| {
        if run.is_empty() {
            return;
        }
        let attributes = if run_marked { marked } else { plain };
        // SAFETY: the attributes are `NSColor`/`NSFont` under AppKit's two keys.
        let piece = unsafe {
            NSAttributedString::new_with_attributes(&NSString::from_str(run), attributes)
        };
        out.appendAttributedString(&piece);
        run.clear();
    };
    for (index, c) in chars.iter().take(keep).enumerate() {
        let is_marked = marks.contains(&index);
        if is_marked != run_marked {
            flush(&mut run, run_marked);
            run_marked = is_marked;
        }
        run.push(*c);
    }
    flush(&mut run, run_marked);
    if keep < chars.len() {
        // The ellipsis is plain, whatever the last run was.
        // SAFETY: as above.
        let ellipsis =
            unsafe { NSAttributedString::new_with_attributes(&NSString::from_str("…"), plain) };
        out.appendAttributedString(&ellipsis);
    }
    out
}

/// Draws a row's title with the letters the query matched marked, within `room` points from
/// `x`; returns where it ended.
fn draw_title(
    title: &str,
    marks: &[usize],
    x: f64,
    room: f64,
    top: f64,
    plain: &Attributes,
    marked: &Attributes,
) -> f64 {
    let chars: Vec<char> = title.chars().collect();
    let mut keep = chars.len();
    let mut string = styled(&chars, marks, keep, plain, marked);
    let mut width = string.size().width;
    while width > room && keep > 1 {
        // A guess that lands close, then single steps.
        let guess = ((keep as f64) * room / width) as usize;
        keep = guess.clamp(1, keep - 1);
        string = styled(&chars, marks, keep, plain, marked);
        width = string.size().width;
    }
    let height = string.size().height;
    string.drawAtPoint(NSPoint::new(x, top + (ROW - height) / 2.0));
    x + width
}

// ─── The list ─────────────────────────────────────────────────────────────

/// The icons of the rows that have one (a tab's, as its chip wears it), by row id.
type Icons = Rc<HashMap<String, Retained<NSImage>>>;

/// What the list view draws.
struct Shown {
    list: List,
    slots: Vec<Slot>,
    highlight: usize,
    look: Option<Look>,
    empty: &'static str,
    /// A heading in the warning colour: why the page cannot be written.
    warning: Option<String>,
    icons: Icons,
}

pub(crate) struct ListIvars {
    shown: RefCell<Shown>,
    /// Where the press that is going on went down, as the row it was on.
    pressed: Cell<Option<usize>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; the list implements no `Drop` and is born with
    // `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPaletteList"]
    #[ivars = ListIvars]
    struct PaletteList;

    unsafe impl NSObjectProtocol for PaletteList {}

    impl PaletteList {
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

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty: NSRect) {
            self.draw(dirty);
        }

        /// The highlight follows the pointer **when it moves** and never because a list slid
        /// under it at rest: typing or the arrows change what is under a still pointer, and
        /// no event comes for that.
        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            if let Some(row) = self.row_under(event)
                && let Some(card) = self.card()
            {
                card.pointer_over(row);
            }
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            self.ivars().pressed.set(self.row_under(event));
        }

        /// A press and release on the same row runs it.
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let pressed = self.ivars().pressed.take();
            let row = self.row_under(event);
            if pressed.is_some()
                && pressed == row
                && let (Some(row), Some(card)) = (row, self.card())
            {
                card.clicked(row);
            }
        }
    }
);

impl PaletteList {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ListIvars {
            shown: RefCell::new(Shown {
                list: List::default(),
                slots: Vec::new(),
                highlight: 0,
                look: None,
                empty: "",
                warning: None,
                icons: Icons::default(),
            }),
            pressed: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        let options = NSTrackingAreaOptions::MouseMoved
            | NSTrackingAreaOptions::ActiveInKeyWindow
            | NSTrackingAreaOptions::InVisibleRect;
        // SAFETY: `owner` is the list, which owns the area and outlives it; no user info.
        // `InVisibleRect` follows the visible part of a scrolled list, so the rect is ignored.
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                NSRect::ZERO,
                options,
                Some(&this),
                None,
            )
        };
        this.addTrackingArea(&area);
        this
    }

    /// The card this list stands in: list ▸ clip view ▸ scroll view ▸ card.
    fn card(&self) -> Option<Retained<PaletteCard>> {
        // SAFETY: reading superviews; we are on the main thread.
        let clip = unsafe { self.superview() }?;
        let scroll = unsafe { clip.superview() }?;
        let card = unsafe { scroll.superview() }?;
        card.downcast::<PaletteCard>().ok()
    }

    fn row_under(&self, event: &NSEvent) -> Option<usize> {
        let at = self.convertPoint_fromView(event.locationInWindow(), None);
        row_at(&self.ivars().shown.borrow().slots, at.y)
    }

    /// Shows `list` with row `highlight` lit; returns the height its slots take.
    fn set(
        &self,
        list: &List,
        highlight: usize,
        look: Look,
        empty: &'static str,
        warning: Option<String>,
        icons: &Icons,
    ) -> f64 {
        let (slots, total) = slots(list);
        {
            let mut shown = self.ivars().shown.borrow_mut();
            shown.list = list.clone();
            shown.slots = slots;
            shown.highlight = highlight;
            shown.look = Some(look);
            shown.empty = empty;
            shown.warning = warning;
            shown.icons = icons.clone();
        }
        self.setNeedsDisplay(true);
        total
    }

    /// Lights row `highlight`, scrolling it into sight (with the heading over it, when it is
    /// the first of its cluster) when `scroll`: the keys scroll, the pointer does not — a row
    /// it points at is already where it is.
    fn light(&self, highlight: usize, scroll: bool) {
        let span = {
            let mut shown = self.ivars().shown.borrow_mut();
            shown.highlight = highlight;
            slot_of(&shown.slots, highlight).map(|slot| {
                let heading = shown
                    .slots
                    .iter()
                    .find(|other| other.line == Line::Heading(highlight))
                    .map_or(0.0, |other| other.height);
                (slot.top - heading, slot.height + heading)
            })
        };
        self.setNeedsDisplay(true);
        if scroll && let Some((top, height)) = span {
            let width = self.bounds().size.width;
            self.scrollRectToVisible(NSRect::new(
                NSPoint::new(0.0, top),
                NSSize::new(width, height),
            ));
        }
    }

    fn draw(&self, dirty: NSRect) {
        let shown = self.ivars().shown.borrow();
        let Some(look) = shown.look else {
            return;
        };
        let width = self.bounds().size.width;
        let plain = attributes(&NSFont::systemFontOfSize(TITLE_SIZE), look.text);
        // SAFETY: AppKit's constant font weights.
        let (semibold, medium) = unsafe { (NSFontWeightSemibold, NSFontWeightMedium) };
        let marked = attributes(
            &NSFont::systemFontOfSize_weight(TITLE_SIZE, semibold),
            look.accent,
        );
        let quiet = attributes(&NSFont::systemFontOfSize(DIM_SIZE), look.dim);
        let heading = attributes(
            &NSFont::systemFontOfSize_weight(HEADING_SIZE, medium),
            look.dim,
        );
        let tick = attributes(
            &NSFont::systemFontOfSize_weight(TITLE_SIZE, semibold),
            look.accent,
        );
        let warning = attributes(
            &NSFont::systemFontOfSize_weight(HEADING_SIZE, medium),
            look.warning,
        );
        if shown.list.is_empty() {
            let message = attributes(&NSFont::systemFontOfSize(DIM_SIZE + 0.5), look.dim);
            let at = draw_width_centered(shown.empty, width, &message);
            draw_text(shown.empty, at, 0.0, EMPTY_BAND, &message);
            return;
        }
        for slot in &shown.slots {
            if slot.top + slot.height < dirty.origin.y
                || slot.top > dirty.origin.y + dirty.size.height
            {
                continue;
            }
            let index = match slot.line {
                Line::Row(index) => index,
                Line::Heading(over) => {
                    if let Some(title) = shown.list.heading_before(over) {
                        let attrs = if shown.warning.as_deref() == Some(title) {
                            &warning
                        } else {
                            &heading
                        };
                        draw_text(title, TEXT_INSET, slot.top + 2.0, slot.height - 2.0, attrs);
                    }
                    continue;
                }
            };
            let hit = &shown.list.hits[index];
            if index == shown.highlight {
                rounded(
                    NSRect::new(
                        NSPoint::new(ROW_INSET, slot.top + 1.0),
                        NSSize::new(width - 2.0 * ROW_INSET, slot.height - 2.0),
                    ),
                    ROW_RADIUS,
                    look.lit,
                    None,
                );
            }
            let right = draw_row_end(&hit.row, width - TEXT_INSET, slot, &quiet, &tick, look);
            // A tab's icon, the one its chip wears, before its title.
            let mut left = TEXT_INSET;
            if let Some(icon) = shown.icons.get(&hit.row.id) {
                // SAFETY: no hints.
                unsafe {
                    icon.drawInRect_fromRect_operation_fraction_respectFlipped_hints(
                        NSRect::new(
                            NSPoint::new(left, slot.top + (slot.height - PALETTE_ICON_SIDE) / 2.0),
                            NSSize::new(PALETTE_ICON_SIDE, PALETTE_ICON_SIDE),
                        ),
                        NSRect::ZERO,
                        NSCompositingOperation::SourceOver,
                        1.0,
                        true,
                        None,
                    );
                }
                left += PALETTE_ICON_SIDE + ICON_GAP;
            }
            let room = right - left - 14.0;
            let end = draw_title(
                &hit.row.title,
                &hit.marks,
                left,
                room,
                slot.top,
                &plain,
                &marked,
            );
            // The second line, after the title, when it fits whole.
            let after = end + 10.0;
            if !hit.row.subtitle.is_empty()
                && after + text_width(&hit.row.subtitle, &quiet) <= right - 14.0
            {
                draw_text(&hit.row.subtitle, after, slot.top, ROW, &quiet);
            }
        }
    }
}

/// The x that centres `text` in `width`.
fn draw_width_centered(text: &str, width: f64, attributes: &Attributes) -> f64 {
    ((width - text_width(text, attributes)) / 2.0).max(TEXT_INSET)
}

/// A theme's patch, ending at `right`: its ground, edged, with its six colours in a row on it.
/// Returns the x where it begins.
fn draw_swatch(swatch: &Swatch, right: f64, slot: &Slot, look: Look) -> f64 {
    let left = right - SWATCH_WIDTH;
    let top = slot.top + (slot.height - SWATCH_HEIGHT) / 2.0;
    rounded(
        NSRect::new(
            NSPoint::new(left, top),
            NSSize::new(SWATCH_WIDTH, SWATCH_HEIGHT),
        ),
        6.0,
        Tint::of(swatch.ground, 1.0),
        Some(look.line),
    );
    let row = SWATCH_DOT * 6.0 + SWATCH_GAP * 5.0;
    let start = left + (SWATCH_WIDTH - row) / 2.0;
    for (n, dot) in swatch.dots.iter().enumerate() {
        let rect = NSRect::new(
            NSPoint::new(
                start + n as f64 * (SWATCH_DOT + SWATCH_GAP),
                top + (SWATCH_HEIGHT - SWATCH_DOT) / 2.0,
            ),
            NSSize::new(SWATCH_DOT, SWATCH_DOT),
        );
        Tint::of(*dot, 1.0).color().setFill();
        NSBezierPath::bezierPathWithOvalInRect(rect).fill();
    }
    left
}

/// The right-hand side of a row, from `right` inward: the theme's patch, the › of a page, the
/// status, the value, a switch's state, the shortcut and the ✓. Returns the x where it begins.
fn draw_row_end(
    row: &Row,
    right: f64,
    slot: &Slot,
    quiet: &Attributes,
    tick: &Attributes,
    look: Look,
) -> f64 {
    let mut x = right;
    if let Some(swatch) = &row.swatch {
        x = draw_swatch(swatch, x, slot, look) - 12.0;
    }
    // A command's own menu (Change Shortcut…) is behind → and said in the band below, not on
    // every row of the list.
    if row.opens.is_some()
        && row.shortcut.is_none()
        && !matches!(row.opens, Some(rules::Scope::CommandActions(_)))
    {
        x = draw_text_right("›", x, slot.top, slot.height, quiet) - 12.0;
    }
    if let Some(status) = &row.status {
        let tone = match status.tone {
            rules::Tone::Dim => look.dim,
            rules::Tone::Accent => look.accent,
            rules::Tone::Success => look.success,
            rules::Tone::Warning => look.warning,
            rules::Tone::Error => look.error,
        };
        let font = NSFont::systemFontOfSize(DIM_SIZE);
        let attrs = attributes(&font, tone);
        x = draw_text_right(&status.text, x, slot.top, slot.height, &attrs) - 12.0;
    }
    if let Some(value) = &row.value {
        x = draw_text_right(value, x, slot.top, slot.height, quiet) - 12.0;
    }
    if let Some(on) = row.toggle {
        let (text, attrs) = if on { ("On", tick) } else { ("Off", quiet) };
        x = draw_text_right(text, x, slot.top, slot.height, attrs) - 12.0;
    }
    if let Some(shortcut) = &row.shortcut {
        x = draw_text_right(shortcut, x, slot.top, slot.height, quiet) - 12.0;
    }
    if row.checked {
        x = draw_text_right("✓", x, slot.top, slot.height, tick) - 10.0;
    }
    x + 2.0
}

// ─── The card ─────────────────────────────────────────────────────────────

/// What a card holds that changes while it is open.
struct Open {
    machine: Palette,
    menu: Snapshot,
    /// What the file says, as the rows show it (values, ✓): taken when the palette opens and
    /// after each write, never from what a preview shows.
    settings: Settings,
    themes: Vec<ThemeEntry>,
    /// The system's appearance: ⌥⏎ on a theme writes the theme for it.
    dark: bool,
    /// Why nothing can be written ([`AppDelegate::palette_lock`](crate::app)), shown over the
    /// pages of settings.
    lock: Option<String>,
    /// What the last refused or failed write said, in the bottom band until the next key.
    notice: Option<String>,
    /// The root's rows: the menu's commands and pages, the settings, the themes.
    root: Vec<Row>,
    recent: Vec<String>,
    /// The tabs and groups of every window, as they were when the palette opened.
    places: Places,
    /// The tabs' icons, by row id ([`tab_icons`]).
    icons: Icons,
    /// The folders, servers and ports ([`AppDelegate::palette_targets`](crate::app)), as they
    /// were when the palette opened and when `~/.ssh/config` was last read.
    targets: Targets,
    /// The commands the open panes ran, as they were when the palette opened, and where each ran
    /// (same order: [`AppDelegate::palette_commands`](crate::app)).
    commands: Commands,
    origins: Vec<Origin>,
    /// The ports that started listening since the palette was last opened.
    fresh_ports: Vec<String>,
    /// The names inside the folder a typed path points into, read off the main thread.
    listing: Option<Listing>,
    list: List,
    rows: Vec<Row>,
    look: Look,
}

/// The path `query` is on the page `scope` (`None`: the root): only the root and the folders page
/// read a query that begins with `~` or `/` as one.
fn path_in(scope: Option<&Scope>, query: &str) -> Option<PathQuery> {
    matches!(scope, None | Some(Scope::Folders))
        .then(|| PathQuery::parse(query))
        .flatten()
}

/// A directory a typed path is listing: the base as typed (`~/Projects`) and, once the background
/// read is back, the directories inside it — `None` inside when the directory cannot be read.
struct Listing {
    base: String,
    names: Option<Option<Vec<String>>>,
}

/// The serial queue directories are listed on.
fn listing_queue() -> &'static DispatchRetained<DispatchQueue> {
    static QUEUE: OnceLock<DispatchRetained<DispatchQueue>> = OnceLock::new();
    QUEUE.get_or_init(|| DispatchQueue::new("dev.bateri.palette", None))
}

impl Open {
    fn sources(&self) -> Sources<'_> {
        Sources {
            menu: &self.menu.rows,
            settings: &self.settings,
            themes: &self.themes,
            places: &self.places,
            targets: &self.targets,
            commands: &self.commands,
            fresh_ports: &self.fresh_ports,
        }
    }

    /// The path the field holds when it is read as a folder: on the root and on the folders page
    /// a query that begins with `~` or `/` is a path ([`PathQuery`]).
    fn typed_path(&self) -> Option<PathQuery> {
        path_in(self.machine.scope(), self.machine.query())
    }

    /// Whether the folder the field points into is being read.
    fn looking(&self) -> bool {
        self.typed_path().is_some_and(|path| {
            self.listing
                .as_ref()
                .is_some_and(|listing| listing.base == path.base && listing.names.is_none())
        })
    }

    /// The root's rows afresh, after the settings they show changed.
    fn rebuild_root(&mut self) {
        self.root = rules::root_rows(&self.sources());
    }

    /// The list the machine's field and page ask for.
    fn derive(&self) -> List {
        let query = self.machine.query();
        // A path typed on the root or on the folders page lists the folders inside the one it
        // points into; until they are read it lists none.
        if let Some(path) = self.typed_path() {
            return match &self.listing {
                Some(Listing {
                    base,
                    names: Some(names),
                }) if *base == path.base => {
                    rules::path_list(&path, names.as_deref(), &self.targets.home)
                }
                _ => List::default(),
            };
        }
        let Some(scope) = self.machine.scope() else {
            return rules::root_list(
                &self.root,
                query,
                &self.recent,
                &rules::suggested_rows(&self.sources()),
                Fallbacks::ALL,
            );
        };
        // The tabs and the command history pages have their own order and headings; the others
        // list their rows.
        if matches!(scope, Scope::Tabs) {
            return rules::tab_list(&self.places, query);
        }
        // A server's page has its own sections and headings, too.
        if let Scope::Server(host) = scope {
            return rules::server_list(host, &self.targets, query);
        }
        if let Scope::History { jump, .. } = scope {
            return rules::history_list(&self.commands, query, *jump);
        }
        let rows = rules::page_rows(scope, &self.sources());
        let mut list = rules::page_list(&rows, query);
        // A tab's, a group's or a command's menu says whose it is.
        if let Some(whose) = rules::actions_heading(scope, &self.places)
            .or_else(|| rules::history_actions_heading(scope, &self.commands))
            .or_else(|| rules::command_actions_heading(scope, self.sources().menu))
        {
            list.headings.insert(0, (0, whose));
        }
        // A page that writes says over its rows why it cannot.
        if let Some(reason) = &self.lock
            && matches!(scope, Scope::Settings | Scope::Theme(_) | Scope::Choices(_))
        {
            list.headings.insert(0, (0, reason.clone()));
        }
        list
    }

    /// The text of the heading that says why the page cannot be written.
    fn warning(&self) -> Option<String> {
        let page = matches!(
            self.machine.scope(),
            Some(Scope::Settings | Scope::Theme(_) | Scope::Choices(_))
        );
        self.lock.clone().filter(|_| page)
    }

    /// The appearance ⌥⏎ writes a theme for, on a page where it does.
    fn appearance(&self) -> Option<&'static str> {
        matches!(
            self.machine.scope(),
            None | Some(Scope::Theme(rules::Slot::Active))
        )
        .then_some(if self.dark { "Dark" } else { "Light" })
    }
}

pub(crate) struct CardIvars {
    /// The window the card is in: where closing and refocusing go.
    window: u64,
    open: RefCell<Open>,
    field: Retained<NSTextField>,
    scroll: Retained<NSScrollView>,
    list: Retained<PaletteList>,
    /// Set when the card is on its way out: late news from the field is ignored.
    closing: Cell<bool>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; the card implements no `Drop` and is born with
    // `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPaletteCard"]
    #[ivars = CardIvars]
    pub(crate) struct PaletteCard;

    unsafe impl NSObjectProtocol for PaletteCard {}
    unsafe impl NSControlTextEditingDelegate for PaletteCard {}
    unsafe impl NSTextFieldDelegate for PaletteCard {}

    impl PaletteCard {
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

        /// A press on the card's own surface (the field's band, the bottom band) is the card's:
        /// it must not reach the scrim behind it, which would close the palette.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {}

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.paint();
        }

        /// ⌘⏎ is a key equivalent: it would reach the menu before the field's command hook.
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            self.equivalent(event)
        }

        /// The letters changed.
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_changed(&self, _notification: &NSNotification) {
            if self.ivars().closing.get() {
                return;
            }
            let text = self.ivars().field.stringValue().to_string();
            let effect = {
                let mut open = self.ivars().open.borrow_mut();
                open.notice = None;
                open.machine.typed(&text)
            };
            self.settle(effect);
            self.rest_later(text);
        }

        /// The keyboard went somewhere else (a click in a pane, another field): the palette is
        /// done.
        #[unsafe(method(controlTextDidEndEditing:))]
        fn control_end_editing(&self, _notification: &NSNotification) {
            if !self.ivars().closing.get() {
                self.dismiss();
            }
        }

        /// The field's command hook: the keys the palette owns. The rest — letters, the caret's
        /// moves inside the text, selection, undo, input methods — are the field's own.
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn control_do_command(
            &self,
            _control: &AnyObject,
            _text_view: &AnyObject,
            command: Sel,
        ) -> bool {
            self.command(command)
        }
    }
);

/// The modifier ⏎ is pressed with, read from the event being handled.
fn held_modifier() -> Modifier {
    let Some(mtm) = MainThreadMarker::new() else {
        return Modifier::None;
    };
    let flags = NSApplication::sharedApplication(mtm)
        .currentEvent()
        .map_or(NSEventModifierFlags::empty(), |event| event.modifierFlags());
    if flags.contains(NSEventModifierFlags::Command) {
        Modifier::Command
    } else if flags.contains(NSEventModifierFlags::Option) {
        Modifier::Option
    } else {
        Modifier::None
    }
}

impl PaletteCard {
    /// ⌘⏎ ([`PaletteCard::perform_key_equivalent`]).
    fn equivalent(&self, event: &NSEvent) -> bool {
        // A card fading out is still in the window, and the keys are the tab's again.
        if self.ivars().closing.get() {
            return false;
        }
        let code = event.keyCode();
        let flags = event.modifierFlags();
        if (code == RETURN || code == ENTER)
            && flags.contains(NSEventModifierFlags::Command)
            && !flags.intersects(NSEventModifierFlags::Option | NSEventModifierFlags::Control)
        {
            self.press(Key::Enter(Modifier::Command));
            return true;
        }
        false
    }

    /// The field's command `command` ([`PaletteCard::control_do_command`]): `true` when the
    /// palette took it.
    fn command(&self, command: Sel) -> bool {
        let empty = self.ivars().field.stringValue().length() == 0;
        let key = if command == sel!(moveUp:) {
            Key::Up
        } else if command == sel!(moveDown:) {
            Key::Down
        } else if command == sel!(insertNewline:)
            || command == sel!(insertNewlineIgnoringFieldEditor:)
            || command == sel!(insertLineBreak:)
        {
            Key::Enter(held_modifier())
        } else if command == sel!(cancelOperation:) {
            Key::Escape
        } else if command == sel!(insertTab:) {
            Key::Tab
        } else if command == sel!(insertBacktab:) {
            // Shift-Tab goes nowhere: the field is the palette's only stop.
            return true;
        } else if command == sel!(moveRight:) {
            Key::Right {
                at_end: self.caret_at_end(),
            }
        } else if command == sel!(moveLeft:) && empty {
            Key::Left
        } else if command == sel!(deleteBackward:) && empty {
            Key::Backspace
        } else {
            return false;
        };
        self.press(key);
        true
    }

    fn new(mtm: MainThreadMarker, window: u64, open: Open) -> Retained<Self> {
        let field = NSTextField::new(mtm);
        field.setBordered(false);
        field.setBezeled(false);
        field.setDrawsBackground(false);
        field.setFocusRingType(NSFocusRingType::None);
        field.setFont(Some(&NSFont::systemFontOfSize(FIELD_SIZE)));
        field.setPlaceholderString(Some(&NSString::from_str("Type a command or search")));
        field.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        let list = PaletteList::new(mtm);
        let scroll = NSScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        scroll.setHasHorizontalScroller(false);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        scroll.setBorderType(NSBorderType::NoBorder);
        scroll.setDocumentView(Some(&list));
        let this = Self::alloc(mtm).set_ivars(CardIvars {
            window,
            open: RefCell::new(open),
            field: field.clone(),
            scroll: scroll.clone(),
            list: list.clone(),
            closing: Cell::new(false),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        // Its own layer, which carries the shadow, as the groups' list.
        this.setWantsLayer(true);
        this.addSubview(&scroll);
        this.addSubview(&field);
        // SAFETY: the card implements the field's delegate protocols (the three methods it uses
        // are defined above); the field holds its delegate weakly and the card owns the field.
        unsafe { field.setDelegate(Some(ProtocolObject::from_ref(&*this))) };
        this
    }

    fn shown_look(&self) -> Look {
        self.ivars().open.borrow().look
    }

    /// Applies `theme` (the window's theme changed while the palette is open).
    fn set_theme(&self, theme: &Theme) {
        let look = Look::of(theme);
        {
            let mut open = self.ivars().open.borrow_mut();
            open.look = look;
            // The tabs' icons wear the theme's colours.
            open.icons = tab_icons(self.mtm(), &open.places, theme);
        }
        self.restyle(look);
        // The theme a preview shows repaints the card while the pointer is on a row of it: the
        // list must not slide under the pointer.
        self.refresh_scrolling(false);
    }

    /// The colours the AppKit views carry: the field's text and the shadow.
    fn restyle(&self, look: Look) {
        self.ivars().field.setTextColor(Some(&look.text.color()));
        let shadow = NSShadow::new();
        shadow.setShadowOffset(NSSize::new(0.0, -6.0));
        shadow.setShadowBlurRadius(24.0);
        shadow.setShadowColor(Some(&look.shadow.color()));
        self.setShadow(Some(&shadow));
        self.setNeedsDisplay(true);
    }

    /// The card, with the machine's list derived afresh and put on screen: the field's text
    /// (a page left or a folder completed puts text in it), the list, the card's height, the
    /// highlight in sight.
    fn refresh(&self) {
        self.refresh_scrolling(true);
    }

    /// [`PaletteCard::refresh`], with the highlighted row scrolled into sight or not.
    fn refresh_scrolling(&self, scroll: bool) {
        let iv = self.ivars();
        self.want_listing();
        let (list, highlight, empty, look, query, warning, icons) = {
            let mut open = iv.open.borrow_mut();
            let list = open.derive();
            let rows = list.rows();
            open.machine.arrive(&rows);
            let highlight = open.machine.cursor(rows.len());
            open.list = list.clone();
            open.rows = rows;
            (
                list,
                highlight,
                empty_message(
                    open.machine.scope(),
                    open.machine.depth(),
                    open.machine.query(),
                    open.looking(),
                ),
                open.look,
                open.machine.query().to_owned(),
                open.warning(),
                open.icons.clone(),
            )
        };
        if iv.field.stringValue().to_string() != query {
            iv.field.setStringValue(&NSString::from_str(&query));
            if let Some(editor) = iv.field.currentEditor() {
                let length = query.encode_utf16().count();
                editor.setSelectedRange(NSRange::new(length, 0));
            }
        }
        let total = iv.list.set(&list, highlight, look, empty, warning, &icons);
        let visible = list_height(total, list.is_empty());
        self.fit(total, visible);
        iv.list.light(highlight, scroll);
        self.setNeedsDisplay(true);
    }

    /// Sizes the card and what is in it for a list of `total` points shown `visible` high.
    fn fit(&self, total: f64, visible: f64) {
        let iv = self.ivars();
        let Some(content) = self.root_content() else {
            return;
        };
        let frame = card_frame(content, visible);
        // The position and width are the ones the card opened with; only the height follows
        // the list.
        let mut now = self.frame();
        if now.size.width == 0.0 {
            now = frame;
        } else {
            now.size.height = frame.size.height;
        }
        self.setFrame(now);
        let width = now.size.width;
        iv.field.setFrame(NSRect::new(
            NSPoint::new(FIELD_INSET, (FIELD_BAND - 26.0) / 2.0 - 1.0),
            NSSize::new(width - 2.0 * FIELD_INSET, 26.0),
        ));
        iv.scroll.setFrame(NSRect::new(
            NSPoint::new(0.0, FIELD_BAND),
            NSSize::new(width, visible),
        ));
        iv.list.setFrame(NSRect::new(
            NSPoint::ZERO,
            NSSize::new(width, total.max(visible)),
        ));
    }

    /// The part of the window the tabs leave: card ▸ scrim ▸ root.
    fn root_content(&self) -> Option<NSRect> {
        // SAFETY: reading superviews; we are on the main thread.
        let scrim = unsafe { self.superview() }?;
        let root = unsafe { scrim.superview() }?;
        root.downcast_ref::<RootView>().map(RootView::content)
    }

    /// Re-centres the card in `content` after the window was resized.
    fn refit(&self, content: NSRect) {
        let visible = self.ivars().scroll.frame().size.height;
        let frame = card_frame(content, visible);
        self.setFrame(frame);
        self.fit(self.ivars().list.frame().size.height, visible);
    }

    /// The caret stands after the last letter.
    fn caret_at_end(&self) -> bool {
        let iv = self.ivars();
        let length = iv.field.stringValue().length();
        iv.field.currentEditor().is_none_or(|editor| {
            let range = editor.selectedRange();
            range.location + range.length >= length
        })
    }

    /// Opening a page by ⏎ or a press is a choice like running a command: the page's id goes to
    /// Recent. Called with the row the highlight is on, before the machine acts on it.
    fn note_page(&self, row: Option<&Row>) {
        if let Some(row) = row
            && row.enters
            && row.opens.is_some()
            && let Some(app) = app::delegate(self.mtm())
        {
            app.palette_chose(&row.id);
        }
    }

    /// ⌘P or ⌘O with the palette up: on that page (the tabs, the folders) the highlight goes one
    /// row on (the keys' ↓, which wraps); anywhere else the page comes, fresh, as when the
    /// shortcut opens the palette.
    fn go_to_page(&self, page: Scope) {
        let _keep = self.retain();
        let on_page = self.ivars().open.borrow().machine.scope() == Some(&page);
        if on_page {
            self.press(Key::Down);
            return;
        }
        let effect = {
            let mut open = self.ivars().open.borrow_mut();
            open.notice = None;
            open.machine.start_on(page)
        };
        self.settle(effect);
    }

    /// Reads the folder the field points into, off the main thread, unless it is the one already
    /// read or being read ([`Open::listing`]).
    fn want_listing(&self) {
        let wanted = {
            let mut open = self.ivars().open.borrow_mut();
            match open.typed_path() {
                Some(path)
                    if open
                        .listing
                        .as_ref()
                        .is_none_or(|listing| listing.base != path.base) =>
                {
                    open.listing = Some(Listing {
                        base: path.base.clone(),
                        names: None,
                    });
                    Some((path.base, open.targets.home.clone()))
                }
                _ => None,
            }
        };
        let Some((base, home)) = wanted else {
            return;
        };
        let window = self.ivars().window;
        listing_queue().exec_async(move || {
            let dir = rules::expand_home(&base, &home);
            let names = rules::list_dirs(Path::new(&dir));
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(app) = app::delegate(mtm)
                    && let Some(window) = app.window(window)
                    && let Some(scrim) = window.root().palette()
                {
                    scrim.ivars().card.listed(&base, names);
                }
            });
        });
    }

    /// The names inside `base` are read. Late news about a folder the field has left is dropped.
    fn listed(&self, base: &str, names: Option<Vec<String>>) {
        if self.ivars().closing.get() {
            return;
        }
        {
            let mut open = self.ivars().open.borrow_mut();
            match &mut open.listing {
                Some(listing) if listing.base == base => listing.names = Some(names),
                _ => return,
            }
        }
        // The list changes under a pointer at rest: the highlight stays where the keys left it.
        self.refresh_scrolling(false);
    }

    /// The folders, servers and ports changed while the palette is open (`~/.ssh/config` was
    /// read): the rows are made afresh.
    fn retarget(&self, targets: Targets) {
        if self.ivars().closing.get() {
            return;
        }
        {
            let mut open = self.ivars().open.borrow_mut();
            open.targets = targets;
            open.rebuild_root();
        }
        self.refresh_scrolling(false);
    }

    /// A key to the machine, and what it asks for done.
    fn press(&self, key: Key) {
        if self.ivars().closing.get() {
            return;
        }
        let _keep = self.retain();
        self.ivars().open.borrow_mut().notice = None;
        if key == Key::Enter(Modifier::None) {
            let row = {
                let open = self.ivars().open.borrow();
                open.rows.get(open.machine.cursor(open.rows.len())).cloned()
            };
            self.note_page(row.as_ref());
        }
        let effect = {
            let mut open = self.ivars().open.borrow_mut();
            let rows = std::mem::take(&mut open.rows);
            let effect = open.machine.key(key, &rows);
            open.rows = rows;
            effect
        };
        self.settle(effect);
    }

    /// The pointer moved over row `row`.
    fn pointer_over(&self, row: usize) {
        // The list's tracking area still reports moves while the card fades out.
        if self.ivars().closing.get() {
            return;
        }
        let effect = {
            let mut open = self.ivars().open.borrow_mut();
            let rows = std::mem::take(&mut open.rows);
            let effect = open.machine.point(row, &rows);
            open.rows = rows;
            effect
        };
        match effect {
            Effect::None => self.refresh_scrolling(false),
            Effect::Preview(preview) => {
                self.preview(preview);
                self.refresh_scrolling(false);
            }
            other => self.settle(other),
        }
    }

    /// A press on row `row`.
    fn clicked(&self, row: usize) {
        if self.ivars().closing.get() {
            return;
        }
        let _keep = self.retain();
        let pressed = self.ivars().open.borrow().rows.get(row).cloned();
        self.note_page(pressed.as_ref());
        let effect = {
            let mut open = self.ivars().open.borrow_mut();
            let rows = std::mem::take(&mut open.rows);
            let effect = open.machine.click(row, &rows);
            open.rows = rows;
            effect
        };
        self.settle(effect);
    }

    /// What a call on the machine asked for.
    fn settle(&self, effect: Effect) {
        match effect {
            Effect::None => self.refresh(),
            Effect::Preview(preview) => {
                self.preview(preview);
                self.refresh();
            }
            // The preview comes down with the palette, whichever way it closes.
            Effect::Close { .. } => self.dismiss(),
            Effect::Activate { id, modifier } => self.activate(&id, modifier),
        }
    }

    /// Shows the setting value the highlight reached on every window, or takes back what was
    /// shown. Nothing is written; a value the catalog cannot read is no preview.
    fn preview(&self, preview: Option<rules::Preview>) {
        let edit = preview.and_then(|shown| rules::setting(&shown.key)?.edit(&shown.token));
        if let Some(app) = app::delegate(self.mtm()) {
            app.preview_edit(edit.as_ref());
        }
    }

    /// The letters have rested for [`rules::REST`]: a highlighted theme shows itself, as if an
    /// arrow had landed on it.
    fn rest_later(&self, seen: String) {
        let Ok(when) = DispatchTime::try_from(rules::REST) else {
            return;
        };
        let window = self.ivars().window;
        let _ = DispatchQueue::main().after(when, move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(app) = app::delegate(mtm)
                && let Some(window) = app.window(window)
                && let Some(scrim) = window.root().palette()
            {
                scrim.ivars().card.rested(&seen);
            }
        });
    }

    fn rested(&self, seen: &str) {
        if self.ivars().closing.get() {
            return;
        }
        let effect = {
            let mut open = self.ivars().open.borrow_mut();
            let rows = std::mem::take(&mut open.rows);
            let effect = open.machine.rested(seen, &rows);
            open.rows = rows;
            effect
        };
        if let Effect::Preview(preview) = effect {
            self.preview(preview);
            self.refresh_scrolling(false);
        }
    }

    /// Row `id` was chosen with `modifier`.
    fn activate(&self, id: &str, modifier: Modifier) {
        let _keep = self.retain();
        if id == "search.find" {
            if modifier != Modifier::None {
                return;
            }
            let query = self.ivars().open.borrow().machine.query().trim().to_owned();
            self.dismiss();
            if let Some(window) = self.owner() {
                window.find_in_focused_pane(&query);
            }
            return;
        }
        let (scope, settings, dark) = {
            let open = self.ivars().open.borrow();
            (
                open.machine.scope().cloned(),
                open.settings.clone(),
                open.dark,
            )
        };
        // A tab, a group, or an action of the menu of one: the palette comes down, the keyboard
        // goes back to the tab on screen, and a turn later the act is done (a name field, a
        // question or a new window opens over a window that has its keyboard back).
        if let Some(act) = rules::act(id, scope.as_ref(), modifier) {
            self.dismiss();
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(app) = app::delegate(mtm) {
                    app.palette_act(act);
                }
            });
            return;
        }
        // A folder, a server or a port.
        if let Some(go) = rules::go(id, modifier) {
            self.go(go);
            return;
        }
        // A command a pane ran, or an action of its menu: the line is left on a prompt a turn
        // after the palette is down (never run), the output gone to or copied.
        let run = {
            let open = self.ivars().open.borrow();
            rules::history_run(id, scope.as_ref(), modifier, &open.commands).and_then(|run| {
                open.origins
                    .get(run.index)
                    .cloned()
                    .map(|origin| (run, origin))
            })
        };
        if let Some((run, origin)) = run {
            let window = self.ivars().window;
            self.dismiss();
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(app) = app::delegate(mtm) {
                    app.palette_history(window, run.how, &origin);
                }
            });
            return;
        }
        if id.starts_with(Kind::History.prefix()) {
            // A command without the use this key stands for (no output to go to): the key does
            // nothing, and the palette stays.
            crate::preview::beep();
            self.refresh();
            return;
        }
        if let Some(chosen) = rules::chosen(id, scope.as_ref(), modifier, &settings, dark) {
            self.write(id, &chosen);
            return;
        }
        // Only the plain ⏎ and a press run a command; ⌘⏎ and ⌥⏎ belong to the rows that use
        // them (the history, the folders), not to a menu item.
        if modifier != Modifier::None {
            self.refresh();
            return;
        }
        let item = self.ivars().open.borrow().menu.item(id);
        let Some(item) = item else {
            self.refresh();
            return;
        };
        if let Some(app) = app::delegate(self.mtm()) {
            app.palette_chose(id);
        }
        self.dismiss();
        let Some(action) = item.action() else {
            return;
        };
        let sender: &AnyObject = item.as_ref();
        let target = item.target();
        // SAFETY: the item's own action, sent as the menu sends it: to its target, or — with none —
        // down the responder chain from the key window's first responder, which `dismiss` has
        // just given back to the tab on screen; the sender is the item, which a tagged action
        // reads its tag from.
        unsafe {
            NSApplication::sharedApplication(self.mtm()).sendAction_to_from(
                action,
                target.as_deref(),
                Some(sender),
            );
        }
    }

    /// A folder, a server or a port was chosen. A port is opened or copied at once, from the
    /// address its row holds; the others are done a turn after the palette is down — they open a
    /// tab or write to a line, over a window that has its keyboard back.
    fn go(&self, go: rules::Go) {
        use rules::Go;
        let port = |key: &str| {
            self.ivars()
                .open
                .borrow()
                .targets
                .ports
                .iter()
                .find(|port| port.key == key)
                .map(|port| port.url.clone())
        };
        match go {
            Go::OpenPort(key) => {
                let url = port(&key);
                self.dismiss();
                if let Some(url) = url {
                    crate::footer::open_url(&url);
                }
            }
            Go::CopyPort(key) => {
                let url = port(&key);
                self.dismiss();
                if let Some(url) = url {
                    crate::clipboard::copy(
                        &objc2_app_kit::NSPasteboard::generalPasteboard(),
                        Some(url),
                    );
                }
            }
            later => {
                let window = self.ivars().window;
                self.dismiss();
                DispatchQueue::main().exec_async(move || {
                    // audit: a block running on the main queue is on the main thread by definition.
                    let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                    if let Some(app) = app::delegate(mtm) {
                        app.palette_go(window, later);
                    }
                });
            }
        }
    }

    /// A settings row was chosen: its edits go to the file through the one write path, and the
    /// preview that showed them is the file's now. A file that takes no write, or a write that
    /// fails, leaves the palette open with the reason in the bottom band. A switch stays where it
    /// is, flipped; anything else closes the palette.
    fn write(&self, id: &str, chosen: &Chosen) {
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
        let lock = self.ivars().open.borrow().lock.clone();
        let outcome = match lock {
            Some(reason) => Err(reason),
            None => app.save_edits(&chosen.edits),
        };
        match outcome {
            Ok(()) => {
                // A setting row is a choice like a command: it goes to Recent.
                if id.starts_with(Kind::Setting.prefix()) {
                    app.palette_chose(id);
                }
                {
                    let mut open = self.ivars().open.borrow_mut();
                    open.machine.commit_preview();
                    open.settings = app.file_settings();
                    open.rebuild_root();
                }
                if chosen.close {
                    self.dismiss();
                } else {
                    self.refresh();
                }
            }
            Err(reason) => {
                self.ivars().open.borrow_mut().notice = Some(reason);
                self.refresh();
            }
        }
    }

    fn owner(&self) -> Option<Retained<TerminalWindow>> {
        app::delegate(self.mtm())?.window(self.ivars().window)
    }

    /// Closes the palette and gives the keyboard back to the tab on screen.
    fn dismiss(&self) {
        let _keep = self.retain();
        if self.ivars().closing.replace(true) {
            return;
        }
        if let Some(window) = self.owner() {
            window.close_palette();
        }
    }

    fn paint(&self) {
        let look = self.shown_look();
        let bounds = self.bounds();
        // The surface, the bottom band cut to the card's rounded corners, then the edge over both.
        rounded(bounds, RADIUS, look.ground, None);
        let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, RADIUS, RADIUS);
        let band = NSRect::new(
            NSPoint::new(0.0, bounds.size.height - FOOTER),
            NSSize::new(bounds.size.width, FOOTER),
        );
        objc2_app_kit::NSGraphicsContext::saveGraphicsState_class();
        path.addClip();
        look.band.color().setFill();
        NSBezierPath::fillRect(band);
        objc2_app_kit::NSGraphicsContext::restoreGraphicsState_class();
        inner_line(bounds, RADIUS, look.line);
        // The rule under the field.
        look.line.color().setFill();
        NSBezierPath::fillRect(NSRect::new(
            NSPoint::new(0.0, FIELD_BAND - 1.0),
            NSSize::new(bounds.size.width, 1.0),
        ));
        self.paint_hints(look, band);
    }

    /// The keys the bottom band names for the highlighted row; or, after a refused or failed
    /// write, why, until the next key.
    fn paint_hints(&self, look: Look, band: NSRect) {
        let (row, depth, appearance, notice) = {
            let open = self.ivars().open.borrow();
            let at = open.machine.cursor(open.rows.len());
            (
                open.rows.get(at).cloned(),
                open.machine.depth(),
                open.appearance(),
                open.notice.clone(),
            )
        };
        let font = NSFont::systemFontOfSize(FOOTER_SIZE);
        if let Some(notice) = notice {
            let warning = attributes(&font, look.warning);
            draw_text(
                &notice,
                TEXT_INSET,
                band.origin.y,
                band.size.height,
                &warning,
            );
            return;
        }
        // SAFETY: AppKit's constant font weight.
        let bold = NSFont::systemFontOfSize_weight(FOOTER_SIZE, unsafe { NSFontWeightMedium });
        let key = attributes(&bold, look.text);
        let verb = attributes(&font, look.dim);
        let mut x = TEXT_INSET;
        for (keys, name) in hints(row.as_ref(), depth, appearance) {
            x = draw_text(keys, x, band.origin.y, band.size.height, &key) + 6.0;
            x = draw_text(&name, x, band.origin.y, band.size.height, &verb) + 18.0;
        }
    }
}

// ─── The scrim ────────────────────────────────────────────────────────────

pub(crate) struct ScrimIvars {
    card: Retained<PaletteCard>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; the scrim implements no `Drop` and is born with
    // `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPaletteScrim"]
    #[ivars = ScrimIvars]
    pub(crate) struct PaletteScrim;

    unsafe impl NSObjectProtocol for PaletteScrim {}

    impl PaletteScrim {
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

        /// While the card fades out, a press goes to what is under it.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            if self.ivars().card.ivars().closing.get() {
                None
            } else {
                // SAFETY: `NSView`'s `hitTest:` takes a point in the superview's space.
                unsafe { msg_send![super(self), hitTest: point] }
            }
        }

        /// A press away from the card closes the palette and is taken by it.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.ivars().card.dismiss();
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, _event: &NSEvent) {
            self.ivars().card.dismiss();
        }

        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, _event: &NSEvent) {
            self.ivars().card.dismiss();
        }
    }
);

impl PaletteScrim {
    /// Re-centres the card after the window was resized.
    pub(crate) fn refit(&self, content: NSRect) {
        self.setFrame(NSRect::new(NSPoint::ZERO, self.superview_size()));
        self.ivars().card.refit(content);
    }

    fn superview_size(&self) -> NSSize {
        // SAFETY: reading the superview; we are on the main thread.
        unsafe { self.superview() }.map_or(self.frame().size, |root| root.bounds().size)
    }

    /// The window's theme changed.
    pub(crate) fn set_theme(&self, theme: &Theme) {
        self.ivars().card.set_theme(theme);
    }
}

// ─── Opening and closing ──────────────────────────────────────────────────

/// Whether the window can take the palette now: not while a question is up on it or on the tab
/// on screen (the palette would sit over a sheet it cannot answer, and the keyboard is the
/// sheet's).
pub(crate) fn ready(window: &TerminalWindow) -> bool {
    window.palette_may_open()
}

/// Opens the palette over `window`'s content, or closes it if it is open.
pub(crate) fn toggle(window: &TerminalWindow) {
    if window.root().palette().is_some() {
        window.close_palette();
    } else if ready(window) {
        open(window, None);
    }
}

/// ⌘P: opens the palette on the tabs, or — open — takes its highlight one tab on
/// ([`PaletteCard::go_to_page`]).
pub(crate) fn go_to_tab(window: &TerminalWindow) {
    go_to(window, Scope::Tabs);
}

/// ⌘O: opens the palette on the folders, or — open — takes its highlight one folder on.
pub(crate) fn go_to_folder(window: &TerminalWindow) {
    go_to(window, Scope::Folders);
}

/// Window ▸ Jump to Command…: opens the palette on the command history with ⏎ going to the
/// output, or — open — takes its highlight one command on.
pub(crate) fn jump_to_command(window: &TerminalWindow) {
    go_to(
        window,
        Scope::History {
            jump: true,
            query: None,
        },
    );
}

/// Edit ▸ Command History…: opens the palette on the commands the panes ran (⏎ writes the
/// line), or — open — comes to that page.
pub(crate) fn show_command_history(window: &TerminalWindow) {
    go_to(
        window,
        Scope::History {
            jump: false,
            query: None,
        },
    );
}

/// View ▸ Themes…: opens the palette on the themes, or — open — comes to that page.
pub(crate) fn show_themes(window: &TerminalWindow) {
    go_to(window, Scope::Theme(rules::Slot::Active));
}

fn go_to(window: &TerminalWindow, page: Scope) {
    if let Some(scrim) = window.root().palette() {
        scrim.ivars().card.go_to_page(page);
    } else if ready(window) {
        open(window, Some(page));
    }
}

/// The folders, servers and ports changed (`~/.ssh/config` was read): an open palette of `window`
/// lists them afresh.
pub(crate) fn retarget(window: &TerminalWindow, targets: Targets) {
    if let Some(scrim) = window.root().palette() {
        scrim.ivars().card.retarget(targets);
    }
}

/// The icon of each tab row of `places`, as its chip wears it, in `theme`'s colours; a tab that
/// is gone has none.
fn tab_icons(mtm: MainThreadMarker, places: &Places, theme: &Theme) -> Icons {
    let Some(app) = app::delegate(mtm) else {
        return Icons::default();
    };
    Rc::new(
        places
            .tabs
            .iter()
            .filter_map(|facts| {
                let tab = app.tab(facts.tab)?;
                let indicator = tab.indicator();
                let image = palette_icon(mtm, tab.emblem(indicator), indicator, theme);
                Some((rules::tab_id(facts.tab), image))
            })
            .collect(),
    )
}

/// Opens the palette over `window`'s content; `start`: on that page, which esc closes from
/// (⌘P on the tabs, ⌘O on the folders) rather than going back to the root.
fn open(window: &TerminalWindow, start: Option<Scope>) {
    let mtm = window.mtm();
    let Some(app) = app::delegate(mtm) else {
        return;
    };
    let root = window.root();
    let theme = window.bar().theme();
    let look = Look::of(&theme);
    // Before the field takes the keyboard: the menu is validated against the pane's.
    let menu = Snapshot::take(mtm);
    let settings = app.file_settings();
    let themes = app.palette_themes();
    let recent = app.palette_recent();
    let animate = !app.reduce_motion();
    let places = app.palette_places();
    let icons = tab_icons(mtm, &places, &theme);
    let targets = app.palette_targets();
    let fresh_ports = app.palette_fresh_ports(&targets.ports);
    let (commands, origins) = app.palette_commands();
    // The file is read as the palette opens; the answer, if it differs, reaches the open card.
    app.refresh_ssh_hosts();
    // The same for what the servers keep running: the list shown now is what was last heard.
    app.refresh_background();
    let mut state = Open {
        machine: Palette::new(),
        root: Vec::new(),
        menu,
        settings,
        themes,
        dark: app.dark_appearance(),
        lock: app.palette_lock(),
        notice: None,
        recent,
        places,
        icons,
        targets,
        commands,
        origins,
        fresh_ports,
        listing: None,
        list: List::default(),
        rows: Vec::new(),
        look,
    };
    if let Some(page) = start {
        state.machine.start_on(page);
    }
    state.rebuild_root();
    state.list = state.derive();
    state.rows = state.list.rows();
    let card = PaletteCard::new(mtm, window.id(), state);
    let scrim = {
        let this = PaletteScrim::alloc(mtm).set_ivars(ScrimIvars { card: card.clone() });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<PaletteScrim> =
            unsafe { msg_send![super(this), initWithFrame: root.bounds()] };
        this
    };
    scrim.addSubview(&card);
    // Over everything the window holds, the side column's edge included.
    root.addSubview_positioned_relativeTo(&scrim, NSWindowOrderingMode::Above, None);
    root.seat_palette(Some(scrim));
    let content = root.content();
    card.setFrame(card_frame(content, EMPTY_BAND));
    card.restyle(look);
    card.refresh();
    let place = card.frame().origin;
    if let Some(ns_window) = card.window() {
        ns_window.makeFirstResponder(Some(&card.ivars().field));
    }
    if animate {
        card.setAlphaValue(0.0);
        card.setFrameOrigin(NSPoint::new(place.x, place.y - SLIDE));
        let moving = card.clone();
        let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
            // SAFETY: AppKit gives the block a live context, for the block's duration.
            let context = unsafe { context.as_ref() };
            context.setDuration(APPEAR_SECS);
            // SAFETY: a constant name QuartzCore exposes, it lives for the whole process.
            let curve = unsafe { kCAMediaTimingFunctionEaseOut };
            context.setTimingFunction(Some(&CAMediaTimingFunction::functionWithName(curve)));
            let animator = moving.animator();
            animator.setAlphaValue(1.0);
            animator.setFrameOrigin(place);
        });
        NSAnimationContext::runAnimationGroup(&changes);
    }
}

/// Takes the palette down: the window has no palette from here on and presses go through it, and
/// the card leaves the way it came, fading and rising over the opening's time (at once with
/// Reduce Motion), over whatever ⏎ did. The keyboard is the caller's to give back.
pub(crate) fn close(root: &RootView) {
    let Some(scrim) = root.palette() else {
        return;
    };
    let card = scrim.ivars().card.clone();
    card.ivars().closing.set(true);
    root.seat_palette(None);
    let app = app::delegate(root.mtm());
    // Whatever the highlight was showing goes back to what the file says (a write has ended the
    // preview already, and then this does nothing). The card keeps its own look: the window no
    // longer passes its theme on.
    if let Some(app) = &app {
        app.end_preview();
    }
    if app.is_none_or(|app| app.reduce_motion()) {
        scrim.removeFromSuperview();
        return;
    }
    let origin = card.frame().origin;
    let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
        // SAFETY: AppKit gives the block a live context, for the block's duration.
        let context = unsafe { context.as_ref() };
        context.setDuration(APPEAR_SECS);
        // SAFETY: a constant name QuartzCore exposes, it lives for the whole process.
        let curve = unsafe { kCAMediaTimingFunctionEaseIn };
        context.setTimingFunction(Some(&CAMediaTimingFunction::functionWithName(curve)));
        let animator = card.animator();
        animator.setAlphaValue(0.0);
        animator.setFrameOrigin(NSPoint::new(origin.x, origin.y - SLIDE));
    });
    let done = RcBlock::new(move || scrim.removeFromSuperview());
    NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&done));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_shell_common::palette::Hit;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
    }

    fn list_of(titles: &[&str], headings: &[(usize, &str)]) -> List {
        List {
            hits: titles
                .iter()
                .map(|title| Hit {
                    row: Row::new(Kind::Command, title, *title),
                    score: 0.0,
                    marks: Vec::new(),
                })
                .collect(),
            headings: headings
                .iter()
                .map(|(at, heading)| (*at, (*heading).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn the_card_stands_in_the_middle_of_the_content_under_the_title_row() {
        // The tabs in the title row: the content is all of the window below the row.
        let top = rect(0.0, 38.0, 1000.0, 700.0);
        let frame = card_frame(top, 200.0);
        assert_eq!(frame.size.width, MAX_WIDTH);
        assert_eq!(frame.origin.x, 200.0);
        assert_eq!(frame.origin.y, 38.0 + TOP);
        assert_eq!(frame.size.height, FIELD_BAND + 200.0 + FOOTER);
    }

    #[test]
    fn with_the_tabs_in_the_side_column_the_middle_is_the_middle_of_what_is_right_of_it() {
        let side = rect(240.0, 0.0, 760.0, 738.0);
        let frame = card_frame(side, 200.0);
        assert_eq!(frame.origin.x, 240.0 + (760.0 - MAX_WIDTH) / 2.0);
        assert_eq!(frame.origin.y, TOP);
        // The centre is the content's, not the window's.
        let middle = frame.origin.x + frame.size.width / 2.0;
        assert_eq!(middle, 240.0 + 760.0 / 2.0);
        assert_ne!(middle, 1000.0 / 2.0);
    }

    #[test]
    fn the_card_follows_the_windows_own_content_in_both_places_of_the_tabs() {
        let window = NSSize::new(1000.0, 738.0);
        let in_row = card_frame(crate::window::content_frame(window, 38.0, None), 100.0);
        assert_eq!(in_row.origin, NSPoint::new(200.0, 38.0 + TOP));
        let at_side = card_frame(
            crate::window::content_frame(window, 38.0, Some(240.0)),
            100.0,
        );
        assert_eq!(at_side.origin, NSPoint::new(240.0 + 80.0, TOP));
        // Opened with the tabs at the side and kept: a later change of place does not move it
        // (the card's frame is taken once; only the window's resizing refits it).
        assert_ne!(in_row.origin.x, at_side.origin.x);
    }

    #[test]
    fn a_narrow_window_narrows_the_card_and_a_short_one_cuts_its_height() {
        let narrow = rect(0.0, 38.0, 400.0, 700.0);
        let frame = card_frame(narrow, 200.0);
        assert_eq!(frame.size.width, 400.0 - 2.0 * SIDE_MARGIN);
        assert_eq!(frame.origin.x, SIDE_MARGIN);
        let short = rect(0.0, 38.0, 1000.0, 200.0);
        let frame = card_frame(short, 360.0);
        assert_eq!(frame.size.height, 200.0 - TOP - SIDE_MARGIN);
        // Never less than the field and the band.
        let tiny = rect(0.0, 0.0, 100.0, 40.0);
        assert_eq!(card_frame(tiny, 100.0).size.height, FIELD_BAND + FOOTER);
    }

    #[test]
    fn rows_stand_under_their_headings_and_are_found_by_height() {
        let list = list_of(&["a", "b", "c"], &[(0, "Recent"), (2, "Commands")]);
        let (slots, total) = slots(&list);
        // heading, a, b, heading, c
        assert_eq!(slots.len(), 5);
        assert_eq!(slots[0].line, Line::Heading(0));
        assert_eq!(slots[1].top, LIST_PAD + HEADING);
        assert_eq!(
            slot_of(&slots, 2).map(|slot| slot.top),
            Some(LIST_PAD + HEADING + 2.0 * ROW + HEADING)
        );
        assert_eq!(total, LIST_PAD + 2.0 * HEADING + 3.0 * ROW + LIST_PAD);
        // A press on a heading, or on the padding, is on no row.
        assert_eq!(row_at(&slots, 2.0), None);
        assert_eq!(row_at(&slots, LIST_PAD + 5.0), None);
        assert_eq!(row_at(&slots, LIST_PAD + HEADING + 1.0), Some(0));
        assert_eq!(row_at(&slots, LIST_PAD + HEADING + ROW + 1.0), Some(1));
        assert_eq!(row_at(&slots, total + 10.0), None);
    }

    #[test]
    fn the_list_grows_to_ten_rows_and_then_scrolls() {
        assert_eq!(
            list_height(3.0 * ROW + 2.0 * LIST_PAD, false),
            3.0 * ROW + 2.0 * LIST_PAD
        );
        assert_eq!(list_height(40.0 * ROW, false), MAX_LIST);
        assert_eq!(list_height(2.0 * LIST_PAD, true), EMPTY_BAND);
    }

    fn pairs(hints: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
        hints
            .iter()
            .map(|(keys, verb)| (*keys, (*verb).to_owned()))
            .collect()
    }

    #[test]
    fn the_bottom_band_names_the_keys_of_the_highlighted_row() {
        let command = Row::new(Kind::Command, "newTab", "New Tab");
        assert_eq!(
            hints(Some(&command), 0, None),
            pairs(&[("↵", "Run"), ("↑↓", "Move"), ("esc", "Close")])
        );
        let page =
            Row::new(Kind::Scope, "menu.Edit", "Edit").opens(Scope::Menu("Edit".into()), true);
        assert_eq!(hints(Some(&page), 1, None)[0], ("↵", "Open".to_owned()));
        assert_eq!(hints(Some(&page), 1, None)[2], ("esc", "Back".to_owned()));
        let find = Row::new(Kind::Search, "find", "Find");
        assert_eq!(hints(Some(&find), 0, None)[0], ("↵", "Find".to_owned()));
        assert_eq!(
            hints(None, 0, None),
            pairs(&[("↑↓", "Move"), ("esc", "Close")])
        );
    }

    #[test]
    fn a_tab_goes_by_enter_and_keeps_its_menu_behind_the_right_arrow() {
        let tab = Row::new(Kind::Tab, "7", "api").opens(Scope::TabActions("tab.7".into()), false);
        assert_eq!(
            hints(Some(&tab), 0, None),
            pairs(&[
                ("↵", "Go to"),
                ("→", "Actions"),
                ("↑↓", "Move"),
                ("esc", "Close")
            ])
        );
        // The ungrouped tabs' row has no menu to point at.
        let loose = Row::new(Kind::Group, "1.0", "Ungrouped");
        assert_eq!(hints(Some(&loose), 0, None)[0], ("↵", "Go to".to_owned()));
        assert_eq!(hints(Some(&loose), 0, None).len(), 3);
        // A page opened on (⌘P) is where esc closes: depth 0 says Close there.
        assert_eq!(
            hints(Some(&tab), 0, None).last(),
            Some(&("esc", "Close".to_owned()))
        );
    }

    #[test]
    fn a_path_is_read_as_one_on_the_root_and_the_folders_page_only() {
        assert_eq!(
            path_in(None, "~/Pro").map(|p| p.stem),
            Some("Pro".to_owned())
        );
        assert!(path_in(Some(&Scope::Folders), "/usr/lo").is_some());
        // Elsewhere the same text is a search, as on any other page.
        assert!(path_in(Some(&Scope::Tabs), "~/").is_none());
        assert!(path_in(Some(&Scope::Settings), "/x").is_none());
        // And a query that is not a path never is.
        assert!(path_in(None, "api").is_none());
        assert!(path_in(Some(&Scope::Folders), "api").is_none());
    }

    #[test]
    fn a_folder_names_the_keys_it_answers_and_a_server_and_a_port_theirs() {
        let folder = Row::new(Kind::Folder, "/Users/a/api", "api").completion("~/api/");
        assert_eq!(
            hints(Some(&folder), 0, None),
            pairs(&[
                ("↵", "New Tab Here"),
                ("⌥↵", "Type cd"),
                ("⌘↵", "Show in Finder"),
                ("⇥", "Go inside"),
                ("↑↓", "Move"),
                ("esc", "Close")
            ])
        );
        let host = Row::new(Kind::Host, "db-1", "db-1");
        assert_eq!(
            hints(Some(&host), 0, None)[0],
            ("↵", "ssh in New Tab".to_owned())
        );
        assert_eq!(
            hints(Some(&host), 0, None)[1],
            ("⌥↵", "ssh in Split".to_owned())
        );
        assert_eq!(hints(Some(&host), 0, None).len(), 4);
        // The folder a path ending in a slash points into: ⇥ has nowhere to step.
        let here = Row::new(Kind::Folder, "/Users/a/api", "~/api");
        assert!(
            !hints(Some(&here), 0, None)
                .iter()
                .any(|(key, _)| *key == "⇥")
        );
        let port = Row::new(Kind::Port, "3000", "localhost:3000");
        assert_eq!(
            hints(Some(&port), 0, None),
            pairs(&[
                ("↵", "Open"),
                ("⌘↵", "Copy URL"),
                ("↑↓", "Move"),
                ("esc", "Close")
            ])
        );
    }

    #[test]
    fn a_theme_names_its_third_use_for_the_appearance_the_system_has() {
        let theme = Row::new(Kind::Theme, "nord", "Nord");
        assert_eq!(
            hints(Some(&theme), 1, Some("Dark")),
            pairs(&[
                ("↵", "Use"),
                ("⌥↵", "Use for Dark"),
                ("↑↓", "Move"),
                ("esc", "Back")
            ])
        );
        // Match System has no appearance to pin; a page that is not the one theme's has no
        // third use to name.
        let system = Row::new(Kind::Theme, "system", "Match System");
        assert_eq!(hints(Some(&system), 1, Some("Light")).len(), 3);
        assert_eq!(hints(Some(&theme), 1, None).len(), 3);
        let switch = Row::new(Kind::Setting, "shell.ports", "Show Ports").toggle(true);
        assert_eq!(hints(Some(&switch), 0, None)[0], ("↵", "Switch".to_owned()));
        let choice = Row::new(Kind::Choice, "beam", "Beam");
        assert_eq!(hints(Some(&choice), 1, None)[0], ("↵", "Use".to_owned()));
    }

    #[test]
    fn a_command_names_its_own_keys_and_the_menu_behind_it() {
        // Where it ran and whether its output is still there decide what each key does: the row
        // says, the band names exactly that, then the menu and the movement keys.
        let command = Row::new(Kind::History, "0", "make")
            .opens(Scope::HistoryActions("hist.0".to_owned()), false)
            .hint("↵", "Write to the Line")
            .hint("⌥↵", "Split Right")
            .hint("⌘↵", "Show Output");
        assert_eq!(
            hints(Some(&command), 0, None),
            pairs(&[
                ("↵", "Write to the Line"),
                ("⌥↵", "Split Right"),
                ("⌘↵", "Show Output"),
                ("→", "Actions"),
                ("↑↓", "Move"),
                ("esc", "Close")
            ])
        );
        // A server's command whose pane is busy can only be copied; no key is named twice.
        let remote = Row::new(Kind::History, "1", "systemctl status")
            .opens(Scope::HistoryActions("hist.1".to_owned()), false)
            .hint("↵", "Copy Command");
        let keys: Vec<&str> = hints(Some(&remote), 0, None)
            .iter()
            .map(|(key, _)| *key)
            .collect();
        assert_eq!(keys, ["↵", "→", "↑↓", "esc"]);
    }

    #[test]
    fn the_history_pages_say_what_an_empty_list_means() {
        let history = Scope::History {
            jump: false,
            query: None,
        };
        let jump = Scope::History {
            jump: true,
            query: None,
        };
        assert!(empty_message(Some(&history), 0, "", false).contains("run"));
        assert!(empty_message(Some(&jump), 0, "", false).contains("jump"));
        assert_eq!(empty_message(Some(&history), 0, "zz", false), "No matches");
    }

    #[test]
    fn an_empty_list_says_why() {
        assert_eq!(empty_message(None, 0, "zzz", false), "No matches");
        assert_eq!(empty_message(None, 1, "", false), "No matches");
        assert!(empty_message(None, 0, "", false).starts_with("Type"));
        // The pages of folders, servers and ports say what would fill them.
        assert!(empty_message(Some(&Scope::Folders), 0, "", false).contains("path"));
        assert!(empty_message(Some(&Scope::Hosts), 0, "", false).contains(".ssh/config"));
        assert!(empty_message(Some(&Scope::Ports), 0, "", false).contains("listening"));
        assert_eq!(
            empty_message(Some(&Scope::Hosts), 0, "zz", false),
            "No matches"
        );
        assert!(empty_message(Some(&Scope::Folders), 0, "~/Pro", true).contains("Reading"));
    }
}
