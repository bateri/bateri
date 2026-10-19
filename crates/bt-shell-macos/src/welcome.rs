//! Help ▸ Welcome to bateri: a small window of four pages — who bateri is, the keys worth
//! knowing, a quick setup, where to go next — that opens by itself once, at the first launch on
//! a Mac, after the first terminal window is up ([`AppDelegate::welcome_at_launch`](crate::app)).
//!
//! **What it decides is not here.** The pages, the keys' moves, the shortcut cards (the menu's
//! own shortcuts, through `help::SHORTCUTS`), the setup's choices and the marker are
//! [`bt_shell_common::welcome`]'s; this module draws them and passes the keys on.
//!
//! **It wears the theme.** The window stands on the split tab's ground (`Theme::ground_srgb`,
//! the same diagonal gradient), its cards are the panes' background with the card's footing
//! under them ([`card::footing`], [`card::draw_footing`]) and the card's corner
//! ([`card::corner_pt`]), the main button and the chosen card carry the theme's accent, and the
//! keys on the cards are set in bateri's own font — the family the settings name, or the chain's
//! SF Mono, then Menlo. The window's appearance follows the theme's darkness, so the traffic
//! lights and the theme popup match. A theme chosen on the setup page re-dresses the window
//! with everything else. Nothing of the terminal windows changes: this is a window of its own.
//!
//! **It writes through the one writer.** A choice of the setup page is a `SettingsEdit` handed
//! to `AppDelegate::save_edit` — the settings window's path: the file's line is written, and
//! the watcher's read applies it to every window, this one included ([`WelcomeView::refresh`]).
//!
//! **Keys and VoiceOver.** ← and → walk the pages, ⏎ is the main button, esc and ⌘W close;
//! ⇥ moves through the cards and buttons (Full Keyboard Access), space presses. Every card and
//! button is an accessibility element with a role (a radio button for a setup choice, its value
//! the selection) and a spoken label (`⇧⌘P` is "Shift Command P").
//!
//! **Motion.** A page slides a little and fades in, the old one out the other way; under Reduce
//! Motion it only cross-fades.

use std::cell::{Cell, OnceCell, RefCell};

use bt_core::{Settings, SettingsEdit, Theme};
use bt_shell_common::welcome::{self, Debounce, Key, Move, Page, Row, Step, Tour, TypeSelect};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityAnnouncementKey,
    NSAccessibilityAnnouncementRequestedNotification, NSAccessibilityButtonRole,
    NSAccessibilityNotificationUserInfoKey, NSAccessibilityPostNotificationWithUserInfo,
    NSAccessibilityRadioButtonRole, NSAccessibilityRole, NSAccessibilityStaticTextRole,
    NSAnimatablePropertyContainer, NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua,
    NSAppearanceNameDarkAqua, NSApplication, NSBackingStoreType, NSBezierPath, NSColor, NSEvent,
    NSEventModifierFlags, NSEventType, NSFont, NSFontAttributeName, NSFontDescriptor,
    NSFontDescriptorAttributeName, NSFontFamilyAttribute, NSFontWeightBold, NSFontWeightMedium,
    NSFontWeightSemibold, NSForegroundColorAttributeName, NSGradient, NSImageScaling, NSImageView,
    NSMenuItem, NSPopUpButton, NSStringDrawing, NSTextAlignment, NSTextField, NSView, NSWindow,
    NSWindowDelegate, NSWindowStyleMask, NSWindowTabbingMode, NSWindowTitleVisibility,
};
use objc2_foundation::{
    NSAttributedStringKey, NSDictionary, NSNotification, NSNumber, NSObjectProtocol, NSPoint,
    NSRect, NSSize, NSString, ns_string,
};

use crate::card;

/// The window's content size, points. A design constant: room for three cards a row and a
/// line of buttons, small enough to sit over a terminal window without hiding it.
const SIZE: NSSize = NSSize::new(640.0, 470.0);
/// The bottom bar's height (the dots and the buttons).
const BAR: f64 = 70.0;
/// The pages' side margin.
const MARGIN: f64 = 50.0;
/// How far a page slides as it comes in, points.
const SLIDE: f64 = 26.0;
/// How long a page takes to come in, seconds; under Reduce Motion, the cross-fade's.
const PAGE_SECS: f64 = 0.34;
const FADE_SECS: f64 = 0.2;
/// Where the titles of the last page's rows start: past the widest lead
/// (`bateri config list`), so they line up.
const STEP_TEXT_LEFT: f64 = 186.0;

// ─── Colours ──────────────────────────────────────────────────────────────

/// The window's colours, all from the theme.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    /// The ground's two ends, top left and bottom right.
    ground: [[u8; 3]; 2],
    /// A card: the panes' own background.
    card: [u8; 3],
    text: [u8; 3],
    dim: [u8; 3],
    accent: [u8; 3],
    dark: bool,
}

impl Look {
    fn of(theme: &Theme) -> Look {
        Look {
            ground: theme.ground_srgb(),
            card: bytes(theme.background),
            text: bytes(theme.foreground),
            dim: bytes(theme.dim),
            accent: bytes(theme.accent),
            dark: theme.is_dark(),
        }
    }

    /// A hairline: the text, faint.
    fn line(self) -> Retained<NSColor> {
        color(self.text, if self.dark { 0.14 } else { 0.12 })
    }
}

fn bytes(hex: u32) -> [u8; 3] {
    [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8]
}

fn color([r, g, b]: [u8; 3], alpha: f64) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(
        f64::from(r) / 255.0,
        f64::from(g) / 255.0,
        f64::from(b) / 255.0,
        alpha,
    )
}

fn rounded(rect: NSRect, radius: f64) -> Retained<NSBezierPath> {
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, radius, radius)
}

fn inset(rect: NSRect, by: f64) -> NSRect {
    NSRect::new(
        NSPoint::new(rect.origin.x + by, rect.origin.y + by),
        NSSize::new(
            (rect.size.width - 2.0 * by).max(0.0),
            (rect.size.height - 2.0 * by).max(0.0),
        ),
    )
}

// ─── Text ─────────────────────────────────────────────────────────────────

fn ui_font(size: f64, weight: Option<&'static f64>) -> Retained<NSFont> {
    match weight {
        Some(weight) => NSFont::systemFontOfSize_weight(size, *weight),
        None => NSFont::systemFontOfSize(size),
    }
}

fn semibold(size: f64) -> Retained<NSFont> {
    // SAFETY: AppKit's constant weight, alive for the process.
    ui_font(size, Some(unsafe { &NSFontWeightSemibold }))
}

thread_local! {
    /// The family bateri draws the terminal in, for the keys on the cards ([`mono`]); set on
    /// every refresh from the settings.
    static FAMILY: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// bateri's own font at `size`: the family the settings name, or the chain's SF Mono, then
/// Menlo (`bt-atlas`'s order), then the system's monospaced face.
fn mono(size: f64) -> Retained<NSFont> {
    let wanted = FAMILY.with(|family| family.borrow().clone());
    for family in wanted
        .iter()
        .map(String::as_str)
        .chain(["SF Mono", "Menlo"])
    {
        if let Some(font) = family_font(family, size) {
            return font;
        }
    }
    // SAFETY: AppKit's constant weight, alive for the process.
    NSFont::monospacedSystemFontOfSize_weight(size, unsafe { NSFontWeightMedium })
}

fn family_font(family: &str, size: f64) -> Option<Retained<NSFont>> {
    let name = NSString::from_str(family);
    let values: [&AnyObject; 1] = [name.as_ref()];
    // SAFETY: the family key is AppKit's constant (an extern static), its value an `NSString`
    // as documented; the descriptor is matched when the font is made.
    let font = unsafe {
        let attributes = NSDictionary::<NSFontDescriptorAttributeName, AnyObject>::from_slices(
            &[NSFontFamilyAttribute],
            &values,
        );
        let descriptor = NSFontDescriptor::fontDescriptorWithFontAttributes(Some(&attributes));
        NSFont::fontWithDescriptor_size(&descriptor, size)
    }?;
    // A descriptor that matches nothing may still give a fallback face: only the family asked
    // for counts.
    let found = font.familyName()?.to_string();
    found.eq_ignore_ascii_case(family).then_some(font)
}

type Attributes = Retained<NSDictionary<NSAttributedStringKey, AnyObject>>;

fn attributes(font: &NSFont, color: &NSColor) -> Attributes {
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

fn measure(text: &str, attributes: &Attributes) -> NSSize {
    // SAFETY: `attributes` holds the keys and value types `sizeWithAttributes:` reads.
    unsafe { NSString::from_str(text).sizeWithAttributes(Some(attributes)) }
}

/// Draws `text` in `rect` (wrapped at its width), inside a `drawRect:`.
fn draw_text(text: &str, rect: NSRect, attributes: &Attributes) {
    // SAFETY: as `measure`; called while a view draws.
    unsafe { NSString::from_str(text).drawInRect_withAttributes(rect, Some(attributes)) };
}

/// A label of the page: no background, no selection, the theme's colour.
fn label(mtm: MainThreadMarker, text: &str, font: &NSFont, frame: NSRect) -> Retained<NSTextField> {
    let field = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
    field.setFont(Some(font));
    field.setAlignment(NSTextAlignment::Center);
    field.setSelectable(false);
    field.setFrame(frame);
    field
}

/// What a label is, for its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ink {
    Text,
    Dim,
}

// ─── Tiles ────────────────────────────────────────────────────────────────

/// A little drawing of what a setup choice does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sketch {
    TabsTop,
    TabsSide,
    SplitCards,
    SplitLines,
}

/// What a tile shows.
#[derive(Clone, Debug, PartialEq)]
enum Shows {
    /// A shortcut card: its keys, its title, its menu.
    Keys {
        glyphs: String,
        title: String,
        menu: String,
    },
    /// A setup choice.
    Choice {
        title: &'static str,
        detail: &'static str,
        sketch: Sketch,
        selected: bool,
    },
    /// A row of the last page.
    Step(Step),
    /// A button: the main one in the accent, Back as a quiet outline.
    Button { title: String, primary: bool },
}

/// What pressing a tile does.
#[derive(Clone, Debug, PartialEq)]
enum Act {
    Nothing,
    Back,
    Forward,
    Write(SettingsEdit),
}

struct TileIvars {
    shows: RefCell<Shows>,
    act: RefCell<Act>,
    look: Cell<Option<Look>>,
    hot: Cell<bool>,
    pressed: Cell<bool>,
    /// A button that is not there (Back on the first page): drawn as nothing, takes no press.
    absent: Cell<bool>,
}

define_class!(
    /// A card or a button of the welcome, drawn by hand in the theme's colours. Its footing —
    /// the shadow under a card — is its page's to draw ([`WelcomePage`]): it falls outside it.
    // SAFETY: NSView has no subclassing requirement; Drop is not implemented.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriWelcomeTile"]
    #[ivars = TileIvars]
    struct Tile;

    unsafe impl NSObjectProtocol for Tile {}

    impl Tile {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw();
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        /// Never the first responder: the keyboard is the window's own ([`WelcomeView`]), so
        /// it works the same whether or not Full Keyboard Access is on.
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            false
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            if self.pressable() {
                self.ivars().pressed.set(true);
                self.setNeedsDisplay(true);
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let pressed = self.ivars().pressed.replace(false);
            self.setNeedsDisplay(true);
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            let bounds = self.bounds();
            let inside = at.x >= 0.0
                && at.y >= 0.0
                && at.x <= bounds.size.width
                && at.y <= bounds.size.height;
            if pressed && inside {
                self.press();
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.set_hot(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.set_hot(false);
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_press(&self) -> bool {
            if self.pressable() {
                self.press();
            }
            self.pressable()
        }
    }
);

impl Tile {
    fn new(mtm: MainThreadMarker, frame: NSRect, shows: Shows, act: Act) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TileIvars {
            shows: RefCell::new(shows),
            act: RefCell::new(act),
            look: Cell::new(None),
            hot: Cell::new(false),
            pressed: Cell::new(false),
            absent: Cell::new(false),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        crate::tab_bar::track_hover(&this);
        this.setAccessibilityElement(true);
        this.describe();
        this
    }

    fn pressable(&self) -> bool {
        !self.ivars().absent.get() && *self.ivars().act.borrow() != Act::Nothing
    }

    /// A card on the ground: its page draws the footing.
    fn carded(&self) -> bool {
        !matches!(*self.ivars().shows.borrow(), Shows::Button { .. })
    }

    fn lifted(&self) -> bool {
        let selected = matches!(
            *self.ivars().shows.borrow(),
            Shows::Choice { selected: true, .. }
        );
        selected || (self.ivars().hot.get() && self.pressable())
    }

    fn radius(&self) -> f64 {
        match *self.ivars().shows.borrow() {
            Shows::Button { .. } => self.bounds().size.height / 2.0,
            _ => card::corner_pt().clamp(8.0, 14.0),
        }
    }

    fn set_hot(&self, hot: bool) {
        if self.ivars().hot.replace(hot) != hot {
            self.setNeedsDisplay(true);
            // The footing is the page's: a hot card stands nearer.
            if let Some(page) = parent(self) {
                page.setNeedsDisplay(true);
            }
        }
    }

    fn set_look(&self, look: Look) {
        self.ivars().look.set(Some(look));
        self.setNeedsDisplay(true);
    }

    fn set_shows(&self, shows: Shows) {
        if *self.ivars().shows.borrow() != shows {
            self.ivars().shows.replace(shows);
            self.describe();
            self.setNeedsDisplay(true);
            if let Some(page) = parent(self) {
                page.setNeedsDisplay(true);
            }
        }
    }

    fn set_absent(&self, absent: bool) {
        self.ivars().absent.set(absent);
        self.setAccessibilityElement(!absent);
        self.setNeedsDisplay(true);
    }

    /// The role, label and value VoiceOver reads.
    fn describe(&self) {
        let (role, label, value): (&NSAccessibilityRole, String, Option<bool>) =
            match &*self.ivars().shows.borrow() {
                // SAFETY (all three roles): AppKit's constant role strings, alive for the
                // process.
                Shows::Keys { glyphs, title, .. } => (
                    unsafe { NSAccessibilityStaticTextRole },
                    format!("{title}: {}", welcome::spoken_keys(glyphs)),
                    None,
                ),
                Shows::Choice {
                    title,
                    detail,
                    selected,
                    ..
                } => (
                    unsafe { NSAccessibilityRadioButtonRole },
                    format!("{title}. {detail}"),
                    Some(*selected),
                ),
                Shows::Step(step) => (
                    unsafe { NSAccessibilityStaticTextRole },
                    if step.command {
                        format!("{}: type {}. {}", step.title, step.lead, step.detail)
                    } else {
                        format!(
                            "{}: {}. {}",
                            step.title,
                            welcome::spoken_keys(&step.lead),
                            step.detail
                        )
                    },
                    None,
                ),
                Shows::Button { title, .. } => (
                    unsafe { NSAccessibilityButtonRole },
                    title.trim_end_matches(" →").to_owned(),
                    None,
                ),
            };
        self.setAccessibilityRole(Some(role));
        self.setAccessibilityLabel(Some(&NSString::from_str(&label)));
        if let Some(selected) = value {
            let number = NSNumber::new_bool(selected);
            // SAFETY: a radio button's accessibility value is an `NSNumber`, 1 when chosen.
            unsafe { self.setAccessibilityValue(Some(number.as_ref())) };
        }
    }

    fn press(&self) {
        let act = self.ivars().act.borrow().clone();
        let mut up = parent(self);
        while let Some(view) = up {
            let object: &AnyObject = view.as_ref();
            if let Some(welcome) = object.downcast_ref::<WelcomeView>() {
                welcome.act(act);
                return;
            }
            up = parent(&view);
        }
    }

    fn draw(&self) {
        let Some(look) = self.ivars().look.get() else {
            return;
        };
        if self.ivars().absent.get() {
            return;
        }
        let bounds = self.bounds();
        let radius = self.radius();
        let shows = self.ivars().shows.borrow().clone();
        let pressed = self.ivars().pressed.get();
        let hot = self.ivars().hot.get();
        match &shows {
            Shows::Button { title, primary } => {
                let (fill, ink) = if *primary {
                    (
                        color(look.accent, if pressed { 0.8 } else { 1.0 }),
                        color(look.card, 1.0),
                    )
                } else {
                    (
                        color(
                            look.text,
                            if pressed {
                                0.12
                            } else if hot {
                                0.08
                            } else {
                                0.0
                            },
                        ),
                        color(look.text, 1.0),
                    )
                };
                fill.setFill();
                rounded(bounds, radius).fill();
                if !*primary {
                    look.line().setStroke();
                    let ring = rounded(inset(bounds, 0.5), radius - 0.5);
                    ring.setLineWidth(1.0);
                    ring.stroke();
                }
                let text = attributes(&semibold(13.5), &ink);
                let size = measure(title, &text);
                draw_text(
                    title,
                    NSRect::new(
                        NSPoint::new(
                            (bounds.size.width - size.width) / 2.0,
                            (bounds.size.height - size.height) / 2.0,
                        ),
                        size,
                    ),
                    &text,
                );
            }
            _ => {
                // The card: the panes' background, lit along its top edge as a pane's card is.
                color(look.card, 1.0).setFill();
                rounded(bounds, radius).fill();
                let light = if look.dark { 0.07 } else { 0.5 };
                color([255, 255, 255], light).setStroke();
                let top = NSBezierPath::bezierPath();
                top.moveToPoint(NSPoint::new(radius, 0.5));
                top.lineToPoint(NSPoint::new(bounds.size.width - radius, 0.5));
                top.setLineWidth(1.0);
                top.stroke();
                let selected = matches!(shows, Shows::Choice { selected: true, .. });
                if selected || pressed {
                    color(look.accent, if selected { 1.0 } else { 0.5 }).setStroke();
                    let ring = rounded(inset(bounds, 1.0), radius - 1.0);
                    ring.setLineWidth(2.0);
                    ring.stroke();
                }
                self.draw_content(&shows, look, bounds);
            }
        }
    }

    /// The texts this tile would cut off: each measured in the font it is drawn in against
    /// the room [`Self::draw_content`] gives it (a setup choice's detail may take two lines).
    fn clipped(&self) -> Vec<String> {
        let width = self.bounds().size.width;
        let ink = color([0, 0, 0], 1.0);
        let fits = |text: &str, font: &NSFont, room: f64, lines: f64| {
            measure(text, &attributes(font, &ink)).width
                <= room * lines - if lines > 1.0 { 24.0 } else { 0.0 }
        };
        let small = NSFont::systemFontOfSize(11.5);
        let mut out = Vec::new();
        let mut check = |text: &str, font: &NSFont, room: f64, lines: f64| {
            if !fits(text, font, room, lines) {
                out.push(text.to_owned());
            }
        };
        match &*self.ivars().shows.borrow() {
            Shows::Keys {
                glyphs,
                title,
                menu,
            } => {
                check(glyphs, &mono(25.0), width - 36.0, 1.0);
                check(title, &semibold(13.5), width - 36.0, 1.0);
                check(menu, &small, width - 36.0, 1.0);
            }
            Shows::Choice { title, detail, .. } => {
                let room = width - (14.0 + 60.0 + 14.0) - 12.0;
                check(title, &semibold(13.5), room, 1.0);
                check(detail, &small, room, 2.0);
            }
            Shows::Step(step) => {
                let lead = mono(if step.command { 12.5 } else { 15.0 });
                let chip = measure(&step.lead, &attributes(&lead, &ink))
                    .width
                    .max(28.0)
                    + 18.0;
                let left = (16.0 + chip + 16.0).max(STEP_TEXT_LEFT);
                if left > STEP_TEXT_LEFT {
                    // The lead pushes the title column: the lead itself is too wide.
                    check(&step.lead, &lead, 0.0, 1.0);
                }
                check(step.title, &semibold(13.5), width - left - 14.0, 1.0);
                check(step.detail, &small, width - left - 14.0, 1.0);
            }
            Shows::Button { title, .. } => check(title, &semibold(13.5), width - 16.0, 1.0),
        }
        out
    }

    fn draw_content(&self, shows: &Shows, look: Look, bounds: NSRect) {
        let text = color(look.text, 1.0);
        let dim = color(look.dim, 1.0);
        let width = bounds.size.width;
        match shows {
            Shows::Keys {
                glyphs,
                title,
                menu,
            } => {
                let keys = attributes(&mono(25.0), &color(look.accent, 1.0));
                draw_text(
                    glyphs,
                    NSRect::new(NSPoint::new(18.0, 16.0), NSSize::new(width - 36.0, 34.0)),
                    &keys,
                );
                draw_text(
                    title,
                    NSRect::new(NSPoint::new(18.0, 56.0), NSSize::new(width - 36.0, 20.0)),
                    &attributes(&semibold(13.5), &text),
                );
                draw_text(
                    menu,
                    NSRect::new(NSPoint::new(18.0, 76.0), NSSize::new(width - 36.0, 18.0)),
                    &attributes(&NSFont::systemFontOfSize(11.5), &dim),
                );
            }
            Shows::Choice {
                title,
                detail,
                sketch,
                selected,
            } => {
                let art = NSRect::new(
                    NSPoint::new(14.0, (bounds.size.height - 42.0) / 2.0),
                    NSSize::new(60.0, 42.0),
                );
                draw_sketch(*sketch, art, look, *selected);
                let left = art.origin.x + art.size.width + 14.0;
                draw_text(
                    title,
                    NSRect::new(
                        NSPoint::new(left, 15.0),
                        NSSize::new(width - left - 12.0, 20.0),
                    ),
                    &attributes(&semibold(13.5), &text),
                );
                draw_text(
                    detail,
                    NSRect::new(
                        NSPoint::new(left, 35.0),
                        NSSize::new(width - left - 12.0, 32.0),
                    ),
                    &attributes(&NSFont::systemFontOfSize(11.5), &dim),
                );
            }
            Shows::Step(step) => {
                let lead_font = mono(if step.command { 12.5 } else { 15.0 });
                let lead = attributes(&lead_font, &color(look.accent, 1.0));
                let size = measure(&step.lead, &lead);
                let chip = NSRect::new(
                    NSPoint::new(16.0, (bounds.size.height - 28.0) / 2.0),
                    NSSize::new(size.width.max(28.0) + 18.0, 28.0),
                );
                color(look.accent, if look.dark { 0.14 } else { 0.10 }).setFill();
                rounded(chip, 7.0).fill();
                draw_text(
                    &step.lead,
                    NSRect::new(
                        NSPoint::new(
                            chip.origin.x + (chip.size.width - size.width) / 2.0,
                            chip.origin.y + (chip.size.height - size.height) / 2.0,
                        ),
                        size,
                    ),
                    &lead,
                );
                // One column for the titles, whatever the lead's width.
                let left = (chip.origin.x + chip.size.width + 16.0).max(STEP_TEXT_LEFT);
                draw_text(
                    step.title,
                    NSRect::new(
                        NSPoint::new(left, 9.0),
                        NSSize::new(width - left - 14.0, 20.0),
                    ),
                    &attributes(&semibold(13.5), &text),
                );
                draw_text(
                    step.detail,
                    NSRect::new(
                        NSPoint::new(left, 28.0),
                        NSSize::new(width - left - 14.0, 18.0),
                    ),
                    &attributes(&NSFont::systemFontOfSize(11.5), &dim),
                );
            }
            Shows::Button { .. } => {}
        }
    }
}

/// A small window drawn in the theme, showing what a setup choice does (flipped coordinates).
fn draw_sketch(sketch: Sketch, rect: NSRect, look: Look, selected: bool) {
    let line = look.line();
    let ink = color(look.accent, if selected { 0.9 } else { 0.45 });
    let quiet = color(look.text, if look.dark { 0.16 } else { 0.12 });
    // The window: the ground, a hairline round it.
    let ground = color(look.ground[0], 1.0);
    ground.setFill();
    rounded(rect, 6.0).fill();
    line.setStroke();
    let frame = rounded(inset(rect, 0.5), 5.5);
    frame.setLineWidth(1.0);
    frame.stroke();
    let (x, y, w, h) = (
        rect.origin.x,
        rect.origin.y,
        rect.size.width,
        rect.size.height,
    );
    let block = |r: NSRect, c: &NSColor, radius: f64| {
        c.setFill();
        rounded(r, radius).fill();
    };
    let pane = color(look.card, 1.0);
    match sketch {
        Sketch::TabsTop => {
            for (i, tab) in [0.0, 1.0, 2.0].iter().enumerate() {
                let fill = if i == 0 { &ink } else { &quiet };
                block(
                    NSRect::new(
                        NSPoint::new(x + 14.0 + tab * 14.0, y + 4.0),
                        NSSize::new(11.0, 5.0),
                    ),
                    fill,
                    2.0,
                );
            }
            block(
                NSRect::new(
                    NSPoint::new(x + 4.0, y + 12.0),
                    NSSize::new(w - 8.0, h - 16.0),
                ),
                &pane,
                3.0,
            );
        }
        Sketch::TabsSide => {
            for (i, row) in [0.0, 1.0, 2.0].iter().enumerate() {
                let fill = if i == 0 { &ink } else { &quiet };
                block(
                    NSRect::new(
                        NSPoint::new(x + 4.0, y + 12.0 + row * 8.0),
                        NSSize::new(12.0, 5.0),
                    ),
                    fill,
                    2.0,
                );
            }
            block(
                NSRect::new(
                    NSPoint::new(x + 20.0, y + 4.0),
                    NSSize::new(w - 24.0, h - 8.0),
                ),
                &pane,
                3.0,
            );
        }
        Sketch::SplitCards => {
            let half = (w - 12.0) / 2.0 - 1.5;
            for column in [0.0, 1.0] {
                block(
                    NSRect::new(
                        NSPoint::new(x + 5.0 + column * (half + 3.0), y + 5.0),
                        NSSize::new(half, h - 10.0),
                    ),
                    &pane,
                    3.0,
                );
            }
            block(
                NSRect::new(NSPoint::new(x + 9.0, y + 10.0), NSSize::new(8.0, 3.0)),
                &ink,
                1.5,
            );
        }
        Sketch::SplitLines => {
            block(
                NSRect::new(
                    NSPoint::new(x + 3.0, y + 3.0),
                    NSSize::new(w - 6.0, h - 6.0),
                ),
                &pane,
                3.0,
            );
            line.setFill();
            NSBezierPath::fillRect(NSRect::new(
                NSPoint::new(x + w / 2.0, y + 3.0),
                NSSize::new(1.0, h - 6.0),
            ));
            block(
                NSRect::new(NSPoint::new(x + 8.0, y + 8.0), NSSize::new(8.0, 3.0)),
                &ink,
                1.5,
            );
        }
    }
}

// ─── The page dots ────────────────────────────────────────────────────────

struct DotsIvars {
    at: Cell<usize>,
    look: Cell<Option<Look>>,
}

/// The width of a page dot, of the current one, and the gap between them.
const DOT: f64 = 7.0;
const DOT_ON: f64 = 20.0;
const DOT_GAP: f64 = 8.0;

define_class!(
    /// The page dots: the current page a short accent bar, the others dots; a click on one
    /// goes there.
    // SAFETY: NSView has no subclassing requirement; Drop is not implemented.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriWelcomeDots"]
    #[ivars = DotsIvars]
    struct Dots;

    unsafe impl NSObjectProtocol for Dots {}

    impl Dots {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let Some(look) = self.ivars().look.get() else {
                return;
            };
            let y = (self.bounds().size.height - DOT) / 2.0;
            for (index, x) in dot_places(self.ivars().at.get()) {
                let on = index == self.ivars().at.get();
                let fill = if on {
                    color(look.accent, 1.0)
                } else {
                    color(look.text, if look.dark { 0.25 } else { 0.2 })
                };
                fill.setFill();
                let width = if on { DOT_ON } else { DOT };
                rounded(
                    NSRect::new(NSPoint::new(x, y), NSSize::new(width, DOT)),
                    DOT / 2.0,
                )
                .fill();
            }
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            let current = self.ivars().at.get();
            let hit = dot_places(current).find(|&(index, x)| {
                let width = if index == current { DOT_ON } else { DOT };
                at.x >= x - DOT_GAP / 2.0 && at.x <= x + width + DOT_GAP / 2.0
            });
            if let Some((index, _)) = hit {
                let mut up = parent(self);
                while let Some(view) = up {
                    let object: &AnyObject = view.as_ref();
                    if let Some(welcome) = object.downcast_ref::<WelcomeView>() {
                        welcome.go(index);
                        return;
                    }
                    up = parent(&view);
                }
            }
        }
    }
);

/// Each dot's index and left edge, the current one wider.
fn dot_places(current: usize) -> impl Iterator<Item = (usize, f64)> {
    let mut x = 0.0;
    (0..Page::ALL.len()).map(move |index| {
        let at = x;
        x += if index == current { DOT_ON } else { DOT } + DOT_GAP;
        (index, at)
    })
}

impl Dots {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DotsIvars {
            at: Cell::new(0),
            look: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.setAccessibilityElement(true);
        // SAFETY: AppKit's constant role string, alive for the process.
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityStaticTextRole }));
        this
    }

    fn set(&self, tour: Tour, look: Look) {
        self.ivars().at.set(tour.index());
        self.ivars().look.set(Some(look));
        self.setAccessibilityLabel(Some(&NSString::from_str(&tour.position_label())));
        self.setNeedsDisplay(true);
    }
}

// ─── Pages ────────────────────────────────────────────────────────────────

define_class!(
    /// One page: its labels and tiles, and under its cards their footing — the shadow a pane's
    /// card casts on the ground ([`card::draw_footing`]), nearer for a hot or chosen card.
    // SAFETY: NSView has no subclassing requirement; Drop is not implemented.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriWelcomePage"]
    struct WelcomePage;

    unsafe impl NSObjectProtocol for WelcomePage {}

    impl WelcomePage {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let dark = self
                .window()
                .and_then(|window| window.contentView())
                .and_then(|view| {
                    let object: &AnyObject = view.as_ref();
                    object.downcast_ref::<WelcomeView>().map(|w| w.ivars().look.get().dark)
                })
                .unwrap_or(true);
            if let Some(welcome) = self.welcome() {
                welcome.draw_focus(self);
            }
            for view in self.subviews().iter() {
                let object: &AnyObject = view.as_ref();
                let Some(tile) = object.downcast_ref::<Tile>() else {
                    continue;
                };
                if tile.carded() {
                    card::draw_footing(tile.frame(), card::footing(dark, tile.lifted()));
                }
            }
        }
    }
);

impl WelcomePage {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        // SAFETY: `initWithFrame:` is NSView's designated initializer.
        unsafe { msg_send![Self::alloc(mtm), initWithFrame: frame] }
    }

    fn welcome(&self) -> Option<Retained<WelcomeView>> {
        let view = self.window()?.contentView()?;
        view.downcast::<WelcomeView>().ok()
    }
}

// ─── The window ───────────────────────────────────────────────────────────

define_class!(
    /// The welcome's window: it hands every key-down to the welcome before AppKit does anything
    /// with it. Without this, ⇥ and ⇧⇥ were AppKit's — the key-view loop, a control that took
    /// the keyboard — and never reached the page; now the window's keys work the same with or
    /// without Full Keyboard Access, whatever was clicked last. A key the welcome does not take
    /// goes on as usual. (⌘-keys come through `performKeyEquivalent:`, which the application
    /// asks the key window before the menus.)
    // SAFETY: NSWindow has no subclassing requirement; Drop is not implemented.
    #[unsafe(super(NSWindow))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriWelcomeWindow"]
    struct WelcomeWindow;

    unsafe impl NSObjectProtocol for WelcomeWindow {}

    impl WelcomeWindow {
        #[unsafe(method(sendEvent:))]
        fn send_event(&self, event: &NSEvent) {
            if event.r#type() == NSEventType::KeyDown
                && let Some(welcome) = self.welcome()
                && welcome.handle_key(event)
            {
                return;
            }
            // SAFETY: NSWindow's `sendEvent:` with the event it was given.
            unsafe { msg_send![super(self), sendEvent: event] }
        }

        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> Bool {
            if let Some(welcome) = self.welcome()
                && welcome.handle_key(event)
            {
                return Bool::YES;
            }
            // SAFETY: NSWindow's `performKeyEquivalent:` with the event it was given.
            unsafe { msg_send![super(self), performKeyEquivalent: event] }
        }
    }
);

impl WelcomeWindow {
    fn welcome(&self) -> Option<Retained<WelcomeView>> {
        self.contentView()?.downcast::<WelcomeView>().ok()
    }
}

pub(crate) struct WelcomeIvars {
    window: OnceCell<Retained<NSWindow>>,
    pages: OnceCell<Vec<Retained<WelcomePage>>>,
    tiles: RefCell<Vec<Retained<Tile>>>,
    tab_tiles: RefCell<Vec<Retained<Tile>>>,
    split_tiles: RefCell<Vec<Retained<Tile>>>,
    labels: RefCell<Vec<(Retained<NSTextField>, Ink)>>,
    back: OnceCell<Retained<Tile>>,
    forward: OnceCell<Retained<Tile>>,
    dots: OnceCell<Retained<Dots>>,
    themes: OnceCell<Retained<NSPopUpButton>>,
    /// The theme popup's items: the name each writes, `None` for a separator.
    theme_names: RefCell<Vec<Option<String>>>,
    tour: Cell<Tour>,
    look: Cell<Look>,
    reduce: Cell<bool>,
    shown_once: Cell<bool>,
    /// The settings on screen (a theme preview's included), for the setup rows' keys.
    settings: RefCell<Settings>,
    /// The setup page's rows, in its coordinates, for the focus highlight; and their labels.
    row_frames: RefCell<Vec<NSRect>>,
    row_labels: RefCell<Vec<Retained<NSTextField>>>,
    /// The key hint above the bar ([`Tour::hint`]).
    hint: OnceCell<Retained<NSTextField>>,
    /// The theme row's type-select and its held-back write.
    typed: RefCell<TypeSelect>,
    pending: RefCell<Debounce<String>>,
    /// Which wait is the latest: an older timer that fires finds a newer change and does nothing.
    generation: Cell<u64>,
}

define_class!(
    /// The welcome window's content: the ground, the pages, the dots and the buttons; it takes
    /// the keys and every press.
    // SAFETY: NSView has no subclassing requirement; Drop is not implemented.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriWelcomeView"]
    #[ivars = WelcomeIvars]
    pub(crate) struct WelcomeView;

    unsafe impl NSObjectProtocol for WelcomeView {}

    impl WelcomeView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            // The split tab's ground: top left to bottom right.
            let look = self.ivars().look.get();
            let gradient = NSGradient::initWithStartingColor_endingColor(
                NSGradient::alloc(),
                &color(look.ground[0], 1.0),
                &color(look.ground[1], 1.0),
            );
            match gradient {
                Some(gradient) => gradient.drawInRect_angle(self.bounds(), -45.0),
                None => {
                    color(look.ground[0], 1.0).setFill();
                    NSBezierPath::fillRect(self.bounds());
                }
            }
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        /// The fallback path: a key the window let through ([`WelcomeWindow`] hands every key to
        /// [`WelcomeView::handle_key`] first).
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if !self.handle_key(event) {
                // SAFETY: NSResponder's `keyDown:` passes the event on (and beeps at the end).
                unsafe { msg_send![super(self), keyDown: event] }
            }
        }

        #[unsafe(method(cancelOperation:))]
        fn cancel_operation(&self, _sender: Option<&AnyObject>) {
            self.close();
        }

        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> Bool {
            if self.handle_key(event) {
                return Bool::YES;
            }
            // SAFETY: NSView's `performKeyEquivalent:` asks the subviews.
            unsafe { msg_send![super(self), performKeyEquivalent: event] }
        }

        #[unsafe(method(themeChosen:))]
        fn theme_chosen(&self, sender: Option<&AnyObject>) {
            let Some(popup) = sender.and_then(|s| s.downcast_ref::<NSPopUpButton>()) else {
                return;
            };
            let index = usize::try_from(popup.indexOfSelectedItem()).ok();
            let name = index.and_then(|index| {
                self.ivars()
                    .theme_names
                    .borrow()
                    .get(index)
                    .cloned()
                    .flatten()
            });
            // The list's choice supersedes a held-back one.
            self.ivars().pending.borrow_mut().flush();
            if let Some(name) = name {
                self.write(SettingsEdit::Theme(name));
            }
            // The keyboard stays the window's.
            if let Some(window) = self.ivars().window.get() {
                window.makeFirstResponder(Some(self));
            }
        }
    }

    unsafe impl NSWindowDelegate for WelcomeView {
        /// However the window goes (esc, ⌘W, the red button), a theme still held back is
        /// written: it is what the reader stopped on.
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _note: &NSNotification) {
            self.flush();
        }
    }
);

impl WelcomeView {
    /// Builds the window (hidden). [`Self::refresh`] fills it and [`Self::show`] shows it.
    pub(crate) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(WelcomeIvars {
            window: OnceCell::new(),
            pages: OnceCell::new(),
            tiles: RefCell::new(Vec::new()),
            tab_tiles: RefCell::new(Vec::new()),
            split_tiles: RefCell::new(Vec::new()),
            labels: RefCell::new(Vec::new()),
            back: OnceCell::new(),
            forward: OnceCell::new(),
            dots: OnceCell::new(),
            themes: OnceCell::new(),
            theme_names: RefCell::new(Vec::new()),
            tour: Cell::new(Tour::default()),
            look: Cell::new(Look::of(&Theme::BATERI)),
            reduce: Cell::new(false),
            shown_once: Cell::new(false),
            settings: RefCell::new(Settings::default()),
            row_frames: RefCell::new(Vec::new()),
            row_labels: RefCell::new(Vec::new()),
            hint: OnceCell::new(),
            typed: RefCell::new(TypeSelect::default()),
            pending: RefCell::new(Debounce::default()),
            generation: Cell::new(0),
        });
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), SIZE);
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.build();
        this
    }

    fn mtm(&self) -> MainThreadMarker {
        MainThreadMarker::from(self)
    }

    /// Whether the window is on screen.
    pub(crate) fn is_open(&self) -> bool {
        self.ivars()
            .window
            .get()
            .is_some_and(|window| window.isVisible() || window.isMiniaturized())
    }

    /// Brings the window up on its first page. `take_keys`: it becomes the key window — not
    /// when it opens by itself over a restored session, where a keystroke meant for a pane
    /// must not land here; it comes to the front and waits for a click.
    pub(crate) fn show(&self, take_keys: bool) {
        let Some(window) = self.ivars().window.get() else {
            return;
        };
        if !self.is_open() {
            self.jump(0);
        }
        if !self.ivars().shown_once.replace(true) {
            window.center();
        }
        if take_keys {
            NSApplication::sharedApplication(self.mtm()).activate();
            window.makeKeyAndOrderFront(None);
            window.makeFirstResponder(Some(self));
        } else {
            window.orderFront(None);
        }
    }

    fn close(&self) {
        self.flush();
        if let Some(window) = self.ivars().window.get() {
            window.performClose(None);
            // A window without a close button's answer still goes.
            window.orderOut(None);
        }
    }

    /// Dresses the window in `theme`, marks the file's choices and lists the themes:
    /// `embedded` are `Theme::embedded_groups`, `user` the files in `themes/`.
    pub(crate) fn refresh(
        &self,
        settings: &Settings,
        theme: &Theme,
        reduce: bool,
        embedded: &[Vec<&str>],
        user: &[String],
    ) {
        let iv = self.ivars();
        iv.settings.replace(settings.clone());
        FAMILY.with(|family| family.replace(settings.font.family.clone()));
        let look = Look::of(theme);
        iv.look.set(look);
        iv.reduce.set(reduce);
        if let Some(window) = iv.window.get() {
            // SAFETY: AppKit's constant appearance names, alive for the process.
            let name = unsafe {
                if look.dark {
                    NSAppearanceNameDarkAqua
                } else {
                    NSAppearanceNameAqua
                }
            };
            window.setAppearance(NSAppearance::appearanceNamed(name).as_deref());
            window.setBackgroundColor(Some(&color(look.ground[0], 1.0)));
        }
        for (tile, choice) in iv
            .tab_tiles
            .borrow()
            .iter()
            .zip(welcome::tab_choices(settings))
        {
            tile.set_shows(choice_shows(&choice, false));
        }
        for (tile, choice) in iv
            .split_tiles
            .borrow()
            .iter()
            .zip(welcome::split_choices(settings))
        {
            tile.set_shows(choice_shows(&choice, true));
        }
        for tile in iv.tiles.borrow().iter() {
            tile.set_look(look);
        }
        for (field, ink) in iv.labels.borrow().iter() {
            let tone = match ink {
                Ink::Text => look.text,
                Ink::Dim => look.dim,
            };
            field.setTextColor(Some(&color(tone, 1.0)));
        }
        self.fill_themes(&settings.theme, embedded, user);
        if let Some(hint) = iv.hint.get() {
            hint.setTextColor(Some(&color(look.dim, 1.0)));
        }
        self.paint_rows();
        self.sync_bar();
        self.setNeedsDisplay(true);
        if let Some(pages) = iv.pages.get() {
            for page in pages {
                page.setNeedsDisplay(true);
            }
        }
    }

    /// The labels whose text is wider than their field allows (wrapping labels: the lines
    /// their height holds).
    fn clipped_labels(&self) -> Vec<String> {
        self.ivars()
            .labels
            .borrow()
            .iter()
            .filter_map(|(field, _)| {
                let text = field.stringValue().to_string();
                let font = field.font()?;
                let frame = field.frame();
                let size = measure(&text, &attributes(&font, &color([0, 0, 0], 1.0)));
                let lines = (frame.size.height / (size.height.max(1.0)))
                    .floor()
                    .max(1.0);
                (size.width > (frame.size.width - 4.0) * lines).then_some(text)
            })
            .collect()
    }

    fn fill_themes(&self, selected: &str, embedded: &[Vec<&str>], user: &[String]) {
        let Some(popup) = self.ivars().themes.get() else {
            return;
        };
        let mut names: Vec<Option<String>> = vec![Some(bt_core::SYSTEM_THEME.to_owned())];
        let mut titles = vec!["Match System".to_owned()];
        let groups = embedded
            .iter()
            .map(|group| {
                group
                    .iter()
                    .map(|name| (*name).to_owned())
                    .collect::<Vec<_>>()
            })
            .chain(std::iter::once(user.to_vec()));
        for group in groups.filter(|group| !group.is_empty()) {
            names.push(None);
            titles.push(String::new());
            for name in group {
                titles
                    .push(Theme::embedded_title(&name).map_or_else(|| name.clone(), str::to_owned));
                names.push(Some(name));
            }
        }
        if !names.iter().flatten().any(|name| name == selected) {
            names.push(None);
            titles.push(String::new());
            names.push(Some(selected.to_owned()));
            titles.push(selected.to_owned());
        }
        if *self.ivars().theme_names.borrow() != names {
            popup.removeAllItems();
            let menu = popup.menu();
            for (name, title) in names.iter().zip(&titles) {
                match (name, &menu) {
                    (None, Some(menu)) => menu.addItem(&NSMenuItem::separatorItem(self.mtm())),
                    _ => popup.addItemWithTitle(&NSString::from_str(title)),
                }
            }
            self.ivars().theme_names.replace(names.clone());
        }
        if let Some(index) = names
            .iter()
            .position(|name| name.as_deref() == Some(selected))
        {
            popup.selectItemAtIndex(index as isize);
        }
    }

    // ─── Moving ───────────────────────────────────────────────────────────

    fn act(&self, act: Act) {
        match act {
            Act::Nothing => {}
            Act::Back => self.step(self.ivars().tour.get().back()),
            Act::Forward => self.step(self.ivars().tour.get().forward()),
            Act::Write(edit) => {
                self.flush();
                self.write(edit);
            }
        }
    }

    fn go(&self, index: usize) {
        self.step(self.ivars().tour.get().go(index));
    }

    fn step(&self, step: Move) {
        match step {
            Move::Stay => {}
            Move::Close => self.close(),
            Move::To(_) | Move::Focus(_) => {
                // Leaving the theme row (or the page) writes the theme it stopped on.
                self.flush();
                let from = self.ivars().tour.get().index();
                let mut tour = self.ivars().tour.get();
                let page = tour.apply(step);
                self.ivars().tour.set(tour);
                if let Some(to) = page {
                    self.turn(from, to);
                }
                if tour.page() == Page::Setup {
                    self.paint_rows();
                    self.announce(&welcome::announce(tour.row(), &self.row_value(tour.row())));
                } else if page.is_some() {
                    self.announce(&tour.position_label());
                }
            }
            Move::Change { row, delta, wrap } => self.change(row, delta, wrap),
            Move::Type(c) => {
                let themes = self.theme_titles();
                let found =
                    self.ivars()
                        .typed
                        .borrow_mut()
                        .push(c, std::time::Instant::now(), &themes);
                if let Some(name) = found {
                    self.preview_theme(name);
                }
            }
            Move::OpenList => {
                if let Some(popup) = self.ivars().themes.get() {
                    // SAFETY: a click with no sender opens the popup's menu, as a press does.
                    unsafe { popup.performClick(None) };
                }
            }
        }
    }

    /// Every key of the welcome, decided here and nowhere else — the window sends each key-down
    /// here before AppKit's own handling ([`WelcomeWindow`]), so no control, key-view loop or
    /// Full Keyboard Access setting can take one. `false` for a key that is not the welcome's
    /// (⌘Q, ⌘,, a ⌃ or ⌥ chord): it goes on its usual way, to the menus.
    fn handle_key(&self, event: &NSEvent) -> bool {
        let flags = event.modifierFlags();
        if flags.contains(NSEventModifierFlags::Control)
            || flags.contains(NSEventModifierFlags::Option)
        {
            return false;
        }
        let shift = flags.contains(NSEventModifierFlags::Shift);
        let chars = event
            .charactersIgnoringModifiers()
            .map(|text| text.to_string().to_lowercase())
            .unwrap_or_default();
        let code = event.keyCode();
        let key = if flags.contains(NSEventModifierFlags::Command) {
            match (code, chars.as_str()) {
                (_, "w") => {
                    self.close();
                    return true;
                }
                (123, _) | (_, "[") | (_, "{") => Key::PreviousPage,
                (124, _) | (_, "]") | (_, "}") => Key::NextPage,
                (_, digit) => match digit.parse::<usize>() {
                    Ok(n) if (1..=Page::ALL.len()).contains(&n) => Key::Page(n - 1),
                    _ => return false,
                },
            }
        } else {
            match code {
                123 => Key::Left,
                124 => Key::Right,
                125 => Key::Down,
                126 => Key::Up,
                36 | 76 => Key::Return,
                53 => Key::Escape,
                48 if shift => Key::BackTab,
                48 => Key::Tab,
                49 => Key::Space,
                51 => Key::Delete,
                _ => match chars.chars().next().filter(|c| c.is_alphanumeric()) {
                    Some(c) => Key::Char(c),
                    None => return false,
                },
            }
        };
        let now = std::time::Instant::now();
        let typing = self.ivars().typed.borrow().is_active(now);
        if key == Key::Delete && typing {
            self.ivars().typed.borrow_mut().clear();
        }
        self.step(self.ivars().tour.get().key_while(key, typing));
        true
    }

    /// ← → on a setup row. The card rows write at once (a held key at an end writes
    /// nothing, [`welcome::step_choice`]); the theme row previews and writes when it stops.
    fn change(&self, row: Row, delta: i32, wrap: bool) {
        let settings = self.ivars().settings.borrow().clone();
        let choices = match row {
            Row::Tabs => welcome::tab_choices(&settings),
            Row::Splits => welcome::split_choices(&settings),
            Row::Theme => {
                let current = settings.theme.clone();
                let next = welcome::step_theme(&self.ivars().theme_names.borrow(), &current, delta);
                if let Some(name) = next {
                    self.preview_theme(name);
                }
                return;
            }
        };
        if let Some(choice) = welcome::step_choice(&choices, delta, wrap) {
            self.flush();
            self.write(choice.edit.clone());
            self.announce(&welcome::announce(row, choice.title));
        }
    }

    /// Shows theme `name` on every window at once, as the palette's preview does, and holds
    /// its write back until the keys stop ([`Debounce`]).
    fn preview_theme(&self, name: String) {
        let Some(app) = crate::app::delegate(self.mtm()) else {
            return;
        };
        let edit = SettingsEdit::Theme(name.clone());
        app.preview_edit(Some(&edit));
        app.refresh_welcome();
        self.announce(&welcome::announce(Row::Theme, &self.theme_title(&name)));
        self.ivars()
            .pending
            .borrow_mut()
            .note(name, std::time::Instant::now());
        let generation = self.ivars().generation.get() + 1;
        self.ivars().generation.set(generation);
        let wait = welcome::WRITE_QUIET + std::time::Duration::from_millis(20);
        let Ok(when) = dispatch2::DispatchTime::try_from(wait) else {
            self.flush();
            return;
        };
        let _ = dispatch2::DispatchQueue::main().after(when, move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(view) = crate::app::delegate(mtm).and_then(|app| app.welcome_view()) {
                view.write_due(generation);
            }
        });
    }

    /// The held-back theme's wait is over: written, unless a newer change restarted it.
    fn write_due(&self, generation: u64) {
        if self.ivars().generation.get() != generation {
            return;
        }
        let due = self
            .ivars()
            .pending
            .borrow_mut()
            .due(std::time::Instant::now());
        if let Some(name) = due {
            self.write(SettingsEdit::Theme(name));
        }
    }

    /// Writes a held-back theme now.
    fn flush(&self) {
        let pending = self.ivars().pending.borrow_mut().flush();
        if let Some(name) = pending {
            self.write(SettingsEdit::Theme(name));
        }
    }

    /// The popup's themes as `(name, title)`, in its order.
    fn theme_titles(&self) -> Vec<(String, String)> {
        self.ivars()
            .theme_names
            .borrow()
            .iter()
            .flatten()
            .map(|name| (name.clone(), self.theme_title(name)))
            .collect()
    }

    fn theme_title(&self, name: &str) -> String {
        if name == bt_core::SYSTEM_THEME {
            return "Match System".to_owned();
        }
        Theme::embedded_title(name).map_or_else(|| name.to_owned(), str::to_owned)
    }

    /// The value a setup row shows now, as words.
    fn row_value(&self, row: Row) -> String {
        let settings = self.ivars().settings.borrow().clone();
        let selected = |choices: Vec<welcome::Choice>| {
            choices
                .into_iter()
                .find(|choice| choice.selected)
                .map_or_else(String::new, |choice| choice.title.to_owned())
        };
        match row {
            Row::Tabs => selected(welcome::tab_choices(&settings)),
            Row::Splits => selected(welcome::split_choices(&settings)),
            Row::Theme => self.theme_title(&settings.theme),
        }
    }

    /// The focused row's label in the accent, the others quiet; the page redraws its highlight.
    fn paint_rows(&self) {
        let iv = self.ivars();
        let look = iv.look.get();
        let row = iv.tour.get().row();
        for (index, label) in iv.row_labels.borrow().iter().enumerate() {
            let focused = Row::ALL.get(index) == Some(&row);
            label.setTextColor(Some(&color(
                if focused { look.accent } else { look.dim },
                1.0,
            )));
        }
        if let Some(page) = iv.pages.get().and_then(|pages| pages.get(2)) {
            page.setNeedsDisplay(true);
        }
    }

    /// The setup page's focus highlight, under the focused row (drawn by the page, before the
    /// cards' footings).
    fn draw_focus(&self, page: &WelcomePage) {
        let iv = self.ivars();
        let is_setup = iv
            .pages
            .get()
            .and_then(|pages| pages.get(2))
            .is_some_and(|setup| std::ptr::eq(&**setup, page));
        if !is_setup {
            return;
        }
        let row = iv.tour.get().row();
        let Some(index) = Row::ALL.iter().position(|r| *r == row) else {
            return;
        };
        let Some(frame) = iv.row_frames.borrow().get(index).copied() else {
            return;
        };
        let look = iv.look.get();
        color(look.accent, if look.dark { 0.12 } else { 0.10 }).setFill();
        rounded(frame, 14.0).fill();
        color(look.accent, 0.9).setStroke();
        let ring = rounded(inset(frame, 0.75), 13.25);
        ring.setLineWidth(1.5);
        ring.stroke();
    }

    /// Tells VoiceOver `text` (the focused row and its value, a page).
    fn announce(&self, text: &str) {
        let Some(window) = self.ivars().window.get() else {
            return;
        };
        let message = NSString::from_str(text);
        let objects: [&AnyObject; 1] = [message.as_ref()];
        // SAFETY: AppKit's announcement key (an extern static) with the `NSString` it documents.
        let info = unsafe {
            NSDictionary::<NSAccessibilityNotificationUserInfoKey, AnyObject>::from_slices(
                &[NSAccessibilityAnnouncementKey],
                &objects,
            )
        };
        let element: &AnyObject = window.as_ref();
        // SAFETY: the window is an accessibility element and the user info is the announcement
        // the notification documents.
        unsafe {
            NSAccessibilityPostNotificationWithUserInfo(
                element,
                NSAccessibilityAnnouncementRequestedNotification,
                Some(&info),
            );
        }
    }

    /// Shows page `to` at once (opening, or a reopened window).
    fn jump(&self, to: usize) {
        let mut tour = Tour::default();
        tour.apply(Move::To(to));
        self.ivars().tour.set(tour);
        self.paint_rows();
        if let Some(pages) = self.ivars().pages.get() {
            for (index, page) in pages.iter().enumerate() {
                page.setHidden(index != to);
                page.setAlphaValue(1.0);
                page.setFrameOrigin(NSPoint::new(0.0, BAR));
            }
        }
        self.sync_bar();
    }

    /// From page `from` to `to`: the new page slides in from the side it lies on and fades in,
    /// the old one the other way out; under Reduce Motion a cross-fade alone.
    fn turn(&self, from: usize, to: usize) {
        self.sync_bar();
        let Some(pages) = self.ivars().pages.get() else {
            return;
        };
        let (Some(old), Some(new)) = (pages.get(from), pages.get(to)) else {
            return;
        };
        let reduce = self.ivars().reduce.get();
        let direction = if to > from { 1.0 } else { -1.0 };
        let offset = if reduce { 0.0 } else { SLIDE * direction };
        let secs = if reduce { FADE_SECS } else { PAGE_SECS };
        for (index, page) in pages.iter().enumerate() {
            if index != from && index != to {
                page.setHidden(true);
            }
        }
        new.setAlphaValue(0.0);
        new.setFrameOrigin(NSPoint::new(offset, BAR));
        new.setHidden(false);
        let (old_view, new_view) = (old.retain(), new.retain());
        card::animate(
            secs,
            move || {
                old_view.animator().setAlphaValue(0.0);
                old_view
                    .animator()
                    .setFrameOrigin(NSPoint::new(-offset, BAR));
                new_view.animator().setAlphaValue(1.0);
                new_view.animator().setFrameOrigin(NSPoint::new(0.0, BAR));
            },
            {
                let old_view = old.retain();
                let this = self.retain();
                move || {
                    // Unless the reader came straight back to it.
                    if this.ivars().tour.get().index() != from {
                        old_view.setHidden(true);
                        old_view.setFrameOrigin(NSPoint::new(0.0, BAR));
                    }
                }
            },
        );
        // The keyboard stays on the window, not on a card of the page that went.
        if let Some(window) = self.ivars().window.get() {
            window.makeFirstResponder(Some(self));
        }
    }

    /// The dots and the two buttons for the page shown.
    fn sync_bar(&self) {
        let iv = self.ivars();
        let tour = iv.tour.get();
        let look = iv.look.get();
        if let Some(dots) = iv.dots.get() {
            dots.set(tour, look);
        }
        if let Some(hint) = iv.hint.get() {
            hint.setStringValue(&NSString::from_str(tour.hint()));
        }
        if let Some(back) = iv.back.get() {
            back.set_absent(tour.is_first());
        }
        if let Some(forward) = iv.forward.get() {
            let title = if tour.is_last() {
                format!("{} →", tour.primary())
            } else {
                tour.primary().to_owned()
            };
            forward.set_shows(Shows::Button {
                title,
                primary: true,
            });
        }
    }

    fn write(&self, edit: SettingsEdit) {
        if let Some(app) = crate::app::delegate(self.mtm()) {
            app.save_edit(&edit);
        }
    }

    // ─── Building ─────────────────────────────────────────────────────────

    fn build(&self) {
        let mtm = self.mtm();
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::FullSizeContentView;
        // SAFETY: with defer=false the window is created at once; `releasedWhenClosed` is turned
        // off right below — the delegate keeps the view, the view the window.
        let window: Retained<WelcomeWindow> = unsafe {
            msg_send![
                WelcomeWindow::alloc(mtm),
                initWithContentRect: NSRect::new(NSPoint::new(0.0, 0.0), SIZE),
                styleMask: style,
                backing: NSBackingStoreType::Buffered,
                defer: false,
            ]
        };
        let window = Retained::into_super(window);
        // SAFETY: only the ownership semantics; we own the Retained.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(welcome::TITLE));
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setTitlebarAppearsTransparent(true);
        window.setMovableByWindowBackground(true);
        window.setTabbingMode(NSWindowTabbingMode::Disallowed);
        window.setContentView(Some(self));
        window.setInitialFirstResponder(Some(self));
        window.setDelegate(Some(ProtocolObject::from_ref(self)));

        let page_frame = NSRect::new(
            NSPoint::new(0.0, BAR),
            NSSize::new(SIZE.width, SIZE.height - BAR - 28.0),
        );
        let pages: Vec<Retained<WelcomePage>> = Page::ALL
            .iter()
            .map(|page| {
                let view = WelcomePage::new(mtm, page_frame);
                self.fill_page(&view, *page);
                view.setHidden(*page != Page::Hello);
                self.addSubview(&view);
                view
            })
            .collect();
        let _ = self.ivars().pages.set(pages);

        // The page's keys, just above the bar.
        let hint = label(
            mtm,
            "",
            &NSFont::systemFontOfSize(11.0),
            NSRect::new(
                NSPoint::new(MARGIN, BAR - 12.0),
                NSSize::new(SIZE.width - 2.0 * MARGIN, 16.0),
            ),
        );
        hint.setAccessibilityElement(false);
        self.addSubview(&hint);
        let _ = self.ivars().hint.set(hint);
        // The bar: the dots at the left, Back and the main button at the right.
        let dots = Dots::new(
            mtm,
            NSRect::new(NSPoint::new(MARGIN, 30.0), NSSize::new(100.0, 12.0)),
        );
        self.addSubview(&dots);
        let _ = self.ivars().dots.set(dots);
        let forward = Tile::new(
            mtm,
            NSRect::new(
                NSPoint::new(SIZE.width - MARGIN - 124.0, 19.0),
                NSSize::new(124.0, 34.0),
            ),
            Shows::Button {
                title: "Next".to_owned(),
                primary: true,
            },
            Act::Forward,
        );
        let back = Tile::new(
            mtm,
            NSRect::new(
                NSPoint::new(SIZE.width - MARGIN - 124.0 - 12.0 - 92.0, 19.0),
                NSSize::new(92.0, 34.0),
            ),
            Shows::Button {
                title: "Back".to_owned(),
                primary: false,
            },
            Act::Back,
        );
        self.addSubview(&back);
        self.addSubview(&forward);
        self.ivars()
            .tiles
            .borrow_mut()
            .extend([back.clone(), forward.clone()]);
        let _ = self.ivars().back.set(back);
        let _ = self.ivars().forward.set(forward);
        let _ = self.ivars().window.set(window);
        self.jump(0);
    }

    /// A label on `page`, `top` points under the page's top edge.
    fn add_label(&self, page: &NSView, text: &str, font: &NSFont, ink: Ink, top: f64, height: f64) {
        let page_height = page.frame().size.height;
        let field = label(
            self.mtm(),
            text,
            font,
            NSRect::new(
                NSPoint::new(MARGIN, page_height - top - height),
                NSSize::new(SIZE.width - 2.0 * MARGIN, height),
            ),
        );
        page.addSubview(&field);
        self.ivars().labels.borrow_mut().push((field, ink));
    }

    /// A tile on `page` at `(left, top)` from the page's top left.
    fn add_tile(
        &self,
        page: &NSView,
        left: f64,
        top: f64,
        size: NSSize,
        shows: Shows,
        act: Act,
    ) -> Retained<Tile> {
        let page_height = page.frame().size.height;
        let tile = Tile::new(
            self.mtm(),
            NSRect::new(NSPoint::new(left, page_height - top - size.height), size),
            shows,
            act,
        );
        page.addSubview(&tile);
        self.ivars().tiles.borrow_mut().push(tile.clone());
        tile
    }

    fn heading(&self, page: &NSView, which: Page) {
        self.add_label(page, which.title(), &semibold(24.0), Ink::Text, 14.0, 32.0);
        self.add_label(
            page,
            which.subtitle(),
            &NSFont::systemFontOfSize(13.0),
            Ink::Dim,
            50.0,
            20.0,
        );
    }

    fn fill_page(&self, page: &NSView, which: Page) {
        let mtm = self.mtm();
        let height = page.frame().size.height;
        match which {
            Page::Hello => {
                if let Some(icon) = NSApplication::sharedApplication(mtm).applicationIconImage() {
                    let view = NSImageView::imageViewWithImage(&icon, mtm);
                    view.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
                    let side = 128.0;
                    view.setFrame(NSRect::new(
                        NSPoint::new((SIZE.width - side) / 2.0, height - 20.0 - side),
                        NSSize::new(side, side),
                    ));
                    // The name below says it; the icon is decoration.
                    view.setAccessibilityElement(false);
                    page.addSubview(&view);
                }
                // SAFETY: AppKit's constant weight, alive for the process.
                let name = ui_font(46.0, Some(unsafe { &NSFontWeightBold }));
                self.add_label(page, which.title(), &name, Ink::Text, 152.0, 58.0);
                self.add_label(
                    page,
                    which.subtitle(),
                    &NSFont::systemFontOfSize(17.0),
                    Ink::Text,
                    214.0,
                    24.0,
                );
                self.add_label(
                    page,
                    welcome::TAGLINE,
                    &NSFont::systemFontOfSize(13.0),
                    Ink::Dim,
                    244.0,
                    20.0,
                );
            }
            Page::Keys => {
                self.heading(page, which);
                let size = NSSize::new(168.0, 100.0);
                let gap = (SIZE.width - 2.0 * MARGIN - 3.0 * size.width) / 2.0;
                for (index, entry) in welcome::key_cards().into_iter().enumerate() {
                    let (row, column) = (index / 3, index % 3);
                    let menu = entry.menu.split(" ▸ ").next().unwrap_or(entry.menu);
                    self.add_tile(
                        page,
                        MARGIN + column as f64 * (size.width + gap),
                        96.0 + row as f64 * (size.height + 20.0),
                        size,
                        Shows::Keys {
                            glyphs: entry.glyphs(),
                            title: welcome::card_title(entry).to_owned(),
                            menu: format!("{menu} menu"),
                        },
                        Act::Nothing,
                    );
                }
            }
            Page::Setup => {
                self.heading(page, which);
                let defaults = Settings::default();
                let label_width = 80.0;
                let left = MARGIN + label_width;
                let size = NSSize::new((SIZE.width - MARGIN - left - 14.0) / 2.0, 70.0);
                let rows: [(&str, Vec<welcome::Choice>, bool); 2] = [
                    ("Tabs", welcome::tab_choices(&defaults), false),
                    ("Splits", welcome::split_choices(&defaults), true),
                ];
                for (row, (title, choices, split)) in rows.into_iter().enumerate() {
                    let top = 94.0 + row as f64 * (size.height + 18.0);
                    self.add_row_frame(page, top, size.height);
                    self.row_label(page, title, top + (size.height - 18.0) / 2.0);
                    let tiles: Vec<Retained<Tile>> = choices
                        .iter()
                        .enumerate()
                        .map(|(index, choice)| {
                            self.add_tile(
                                page,
                                left + index as f64 * (size.width + 14.0),
                                top,
                                size,
                                choice_shows(choice, split),
                                Act::Write(choice.edit.clone()),
                            )
                        })
                        .collect();
                    if split {
                        self.ivars().split_tiles.replace(tiles);
                    } else {
                        self.ivars().tab_tiles.replace(tiles);
                    }
                }
                let top = 94.0 + 2.0 * (size.height + 18.0) + 6.0;
                self.add_row_frame(page, top, 28.0);
                self.row_label(page, "Theme", top + 4.0);
                let popup = NSPopUpButton::initWithFrame_pullsDown(
                    NSPopUpButton::alloc(mtm),
                    NSRect::new(
                        NSPoint::new(left - 3.0, height - top - 28.0),
                        NSSize::new(size.width + 6.0, 28.0),
                    ),
                    false,
                );
                popup.setAccessibilityLabel(Some(ns_string!("Theme")));
                // The keyboard is the window's: ← → and letters choose, ␣ opens the list.
                popup.setRefusesFirstResponder(true);
                // SAFETY: the target is this view, which lives as long as the popup (it is an
                // ancestor); `themeChosen:` takes the sender.
                unsafe {
                    popup.setTarget(Some(self));
                    popup.setAction(Some(sel!(themeChosen:)));
                }
                page.addSubview(&popup);
                let _ = self.ivars().themes.set(popup);
            }
            Page::Next => {
                self.heading(page, which);
                let size = NSSize::new(SIZE.width - 2.0 * MARGIN, 52.0);
                for (index, step) in welcome::next_steps().into_iter().enumerate() {
                    self.add_tile(
                        page,
                        MARGIN,
                        92.0 + index as f64 * (size.height + 14.0),
                        size,
                        Shows::Step(step),
                        Act::Nothing,
                    );
                }
            }
        }
    }

    fn row_label(&self, page: &NSView, text: &str, top: f64) {
        let height = page.frame().size.height;
        let field = label(
            self.mtm(),
            text,
            &semibold(13.0),
            NSRect::new(
                NSPoint::new(MARGIN, height - top - 18.0),
                NSSize::new(76.0, 18.0),
            ),
        );
        field.setAlignment(NSTextAlignment::Left);
        page.addSubview(&field);
        // Coloured by the focus ([`Self::paint_rows`]), not with the other labels.
        self.ivars().row_labels.borrow_mut().push(field);
    }

    /// A setup row's highlight frame: `height` points from `top` (page coordinates from the
    /// top), padded round the label and the row's controls.
    fn add_row_frame(&self, page: &NSView, top: f64, height: f64) {
        let page_height = page.frame().size.height;
        let pad = 8.0;
        self.ivars().row_frames.borrow_mut().push(NSRect::new(
            NSPoint::new(MARGIN - 12.0, page_height - top - height - pad),
            NSSize::new(SIZE.width - 2.0 * MARGIN + 24.0, height + 2.0 * pad),
        ));
    }
}

/// Draws every page in bateri's dark and light themes into PNGs in `dir`, without showing a
/// window — the pictures the docs and a review look at, and the check that every page draws
/// (`tests/welcome_pictures.rs`). The setup page shows the side column chosen, its first row
/// focused as on arrival, and once more with the theme row focused.
pub(crate) fn pictures(
    mtm: MainThreadMarker,
    dir: &std::path::Path,
) -> std::io::Result<Vec<std::path::PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut written = Vec::new();
    for (name, theme) in [("dark", Theme::BATERI), ("light", Theme::BATERI_LIGHT)] {
        let view = WelcomeView::new(mtm);
        let settings = Settings {
            tab_position: bt_core::TabPosition::Side,
            ..Settings::default()
        };
        view.refresh(&settings, &theme, true, &Theme::embedded_groups(), &[]);
        let clipped: Vec<String> = view
            .ivars()
            .tiles
            .borrow()
            .iter()
            .flat_map(|tile| tile.clipped())
            .chain(view.clipped_labels())
            .collect();
        if !clipped.is_empty() {
            return Err(std::io::Error::other(format!("cut off: {clipped:?}")));
        }
        // Every page, then the setup page again with its theme row focused.
        let shots = (0..Page::ALL.len())
            .map(|index| (index, None, format!("{}", index + 1)))
            .chain(std::iter::once((
                2,
                Some(Row::Theme),
                "3-theme-row".to_owned(),
            )));
        for (index, row, suffix) in shots {
            view.jump(index);
            if let Some(row) = row {
                view.step(Move::Focus(row));
            }
            let bounds = view.bounds();
            let Some(rep) = view.bitmapImageRepForCachingDisplayInRect(bounds) else {
                return Err(std::io::Error::other("no bitmap for the view"));
            };
            view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
            let properties = NSDictionary::new();
            // SAFETY: an empty property dictionary is a valid `properties` for PNG.
            let data = unsafe {
                rep.representationUsingType_properties(
                    objc2_app_kit::NSBitmapImageFileType::PNG,
                    &properties,
                )
            }
            .ok_or_else(|| std::io::Error::other("no PNG for the bitmap"))?;
            let path = dir.join(format!("welcome-{name}-{suffix}.png"));
            std::fs::write(&path, data.to_vec())?;
            written.push(path);
        }
    }
    Ok(written)
}

/// Walks the welcome with synthesized key-downs sent through its window's `sendEvent:` — the
/// path a real key takes once AppKit hands it to the window — and returns the page each one
/// left it on (1-based), for `tests/welcome_pictures.rs`. Nothing is shown and, without the
/// application's delegate, nothing is written.
pub(crate) fn key_walk(mtm: MainThreadMarker, keys: &[(u16, &str, bool, bool)]) -> Vec<usize> {
    let view = WelcomeView::new(mtm);
    view.refresh(
        &Settings::default(),
        &Theme::BATERI,
        true,
        &Theme::embedded_groups(),
        &[],
    );
    let Some(window) = view.ivars().window.get().cloned() else {
        return Vec::new();
    };
    keys.iter()
        .map(|&(code, chars, shift, command)| {
            let mut flags = NSEventModifierFlags::empty();
            if shift {
                flags |= NSEventModifierFlags::Shift;
            }
            if command {
                flags |= NSEventModifierFlags::Command;
            }
            let text = NSString::from_str(chars);
            let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
                NSEventType::KeyDown,
                NSPoint::new(0.0, 0.0),
                flags,
                0.0,
                window.windowNumber(),
                None,
                &text,
                &text,
                false,
                code,
            );
            if let Some(event) = event {
                if command {
                    window.performKeyEquivalent(&event);
                } else {
                    window.sendEvent(&event);
                }
            }
            view.ivars().tour.get().index() + 1
        })
        .collect()
}

/// The view `view` sits in.
fn parent(view: &NSView) -> Option<Retained<NSView>> {
    // SAFETY: reading the superview has no precondition here: the views are only touched on
    // the main thread and the hierarchy is not being mutated while it is read.
    unsafe { view.superview() }
}

fn choice_shows(choice: &welcome::Choice, split: bool) -> Shows {
    let sketch = match (&choice.edit, split) {
        (SettingsEdit::TabPosition(bt_core::TabPosition::Side), _) => Sketch::TabsSide,
        (SettingsEdit::SplitStyle(bt_core::SplitStyle::Lines), _) => Sketch::SplitLines,
        (_, true) => Sketch::SplitCards,
        (_, false) => Sketch::TabsTop,
    };
    Shows::Choice {
        title: choice.title,
        detail: choice.detail,
        sketch,
        selected: choice.selected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dots_widen_the_current_page() {
        let places: Vec<(usize, f64)> = dot_places(1).collect();
        assert_eq!(places.len(), Page::ALL.len());
        assert_eq!(places[0], (0, 0.0));
        assert_eq!(places[1], (1, DOT + DOT_GAP));
        assert_eq!(places[2], (2, DOT + DOT_GAP + DOT_ON + DOT_GAP));
    }

    #[test]
    fn every_choice_has_its_own_sketch() {
        let settings = Settings::default();
        let sketches: Vec<Sketch> = welcome::tab_choices(&settings)
            .iter()
            .map(|choice| choice_shows(choice, false))
            .chain(
                welcome::split_choices(&settings)
                    .iter()
                    .map(|choice| choice_shows(choice, true)),
            )
            .filter_map(|shows| match shows {
                Shows::Choice { sketch, .. } => Some(sketch),
                _ => None,
            })
            .collect();
        assert_eq!(
            sketches,
            [
                Sketch::TabsTop,
                Sketch::TabsSide,
                Sketch::SplitCards,
                Sketch::SplitLines
            ]
        );
    }

    #[test]
    fn the_look_is_the_themes() {
        let theme = Theme::BATERI;
        let look = Look::of(&theme);
        assert_eq!(look.card, bytes(theme.background));
        assert_eq!(look.accent, bytes(theme.accent));
        assert_eq!(look.ground, theme.ground_srgb());
        assert_eq!(look.dark, theme.is_dark());
        assert_eq!(bytes(0x12_34_56), [0x12, 0x34, 0x56]);
    }
}
