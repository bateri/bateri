//! The views of the card ([`super`]): drawn from the model ([`Card`]) every time it changes, in
//! the system's own colours, so that it follows the appearance of the window it is over.
//!
//! **A frame is a calculation.** The card is laid out by hand, top to bottom, in a flipped view:
//! the title (which wraps), a list of rows (a mark, a name that fades where it does not fit, and
//! under it the reason a row failed, which wraps), and the buttons at the right. The card is
//! [`WIDTH`] wide, or as wide as its pane lets it be ([`width_for`]); buttons that do not fit in a
//! line stand one under the other. A list of many rows longer than [`LIST_MAX`] scrolls inside its
//! box, so that the card never grows over the whole pane (a few rows grow with their words, up to
//! the pane's room).
//! Nothing here takes the keyboard: a button is not the key view's business and the shortcuts
//! (⌘Return, ⌘Escape) are the menu's.

use std::cell::{Cell, RefCell};

use bt_shell_common::apps::card::{Card, Line, Mark, Stage};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSAccessibility, NSAttributedStringNSStringDrawing, NSBezierPath, NSButton, NSButtonType,
    NSColor, NSControl, NSControlSize, NSEvent, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSGradient, NSLineBreakMode, NSLineCapStyle, NSLineJoinStyle,
    NSMutableParagraphStyle, NSParagraphStyleAttributeName, NSProgressIndicator,
    NSProgressIndicatorStyle, NSScrollView, NSShadow, NSStringDrawing, NSTextAlignment,
    NSTextField, NSUserInterfaceItemIdentification, NSView,
};
use objc2_foundation::{
    NSAttributedString, NSDictionary, NSMutableAttributedString, NSObjectProtocol, NSPoint,
    NSRange, NSRect, NSSize, NSString,
};

use super::Act;

/// The card's width, at most: in a narrower pane it is as wide as the pane lets it be
/// ([`width_for`]).
pub(crate) const WIDTH: f64 = 264.0;

/// The least the card is made narrower than that: below it the words would wrap at every other
/// letter; a pane narrower than this and the room kept at its sides loses a bit of the card's edge.
const MIN_WIDTH: f64 = 140.0;

/// How wide the card is over a pane `pane_width` wide: [`WIDTH`], or what is left of the pane
/// after the room kept at each of its sides ([`crate::pane::CORNER_INSET`]).
pub(crate) fn width_for(pane_width: f64) -> f64 {
    (pane_width - 2.0 * crate::pane::CORNER_INSET.1).clamp(MIN_WIDTH, WIDTH)
}
/// Whether a card made for a pane of the size `built` is right for one of the size `now`: as wide
/// as the pane lets it be ([`width_for`]), and with its list as high as the pane's height leaves
/// room for ([`ROOM_SHARE`]). A pane that became lower (a split across it) or higher is another
/// card's room, and the card is made again, so that its buttons are not pushed below the pane.
pub(crate) fn made_for(built: NSSize, now: NSSize) -> bool {
    (width_for(built.width) - width_for(now.width)).abs() <= 0.5
        && (built.height - now.height).abs() <= 0.5
}

/// The room at its sides, and above its title and under its buttons.
const PAD_X: f64 = 12.0;
const PAD_Y: f64 = 11.0;
/// The most the list is high before it scrolls, and the gap between two rows.
pub(crate) const LIST_MAX: f64 = 132.0;
const ROW_GAP: f64 = 6.0;
/// A mark's square and the gap after it.
const MARK: f64 = 16.0;
const MARK_GAP: f64 = 6.0;
/// A button's height, the gap between two, and the space above them.
const BUTTON_H: f64 = 24.0;
const BUTTON_GAP: f64 = 8.0;
const BUTTONS_TOP: f64 = 12.0;
/// The gap above each line a card says under its title.
const NOTE_GAP: f64 = 4.0;
/// How much of a name's end fades when it does not fit.
const FADE: f64 = 22.0;
/// The card's corners.
const RADIUS: f64 = 10.0;

// ─── what the buttons say to the application ──────────────────────────────────

pub(super) struct TargetIvars {
    account: String,
}

define_class!(
    /// The target of the card's buttons and boxes: each is sent to the application on the next turn
    /// of the loop, so that the view that was pressed is not taken away while it is pressed.
    // SAFETY: NSObject is designed for subclassing; Drop is not implemented.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriAppsCardTarget"]
    #[ivars = TargetIvars]
    pub(super) struct Target;

    unsafe impl NSObjectProtocol for Target {}

    impl Target {
        #[unsafe(method(toggle:))]
        fn toggle(&self, sender: Option<&AnyObject>) {
            let line = sender
                .and_then(|sender| sender.downcast_ref::<NSButton>())
                .and_then(|button| usize::try_from(button.tag()).ok());
            if let Some(line) = line {
                self.send(Act::Toggle(line));
            }
        }

        #[unsafe(method(install:))]
        fn install(&self, _sender: Option<&AnyObject>) {
            self.send(Act::Install);
        }

        #[unsafe(method(notHere:))]
        fn not_here(&self, _sender: Option<&AnyObject>) {
            self.send(Act::NotHere);
        }

        #[unsafe(method(retry:))]
        fn retry(&self, _sender: Option<&AnyObject>) {
            self.send(Act::Retry);
        }

        #[unsafe(method(close:))]
        fn close(&self, _sender: Option<&AnyObject>) {
            self.send(Act::Close);
        }
    }
);

impl Target {
    pub(super) fn new(mtm: MainThreadMarker, account: &str) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TargetIvars {
            account: account.to_owned(),
        });
        // SAFETY: `init` is NSObject's; the ivars are set.
        unsafe { msg_send![super(this), init] }
    }

    fn send(&self, act: Act) {
        let account = self.ivars().account.clone();
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            super::act(mtm, &account, act);
        });
    }
}

// ─── the card ─────────────────────────────────────────────────────────────────

pub(crate) struct CardIvars {
    /// The buttons' target is weak: the card holds it.
    _target: Retained<Target>,
}

define_class!(
    /// The card: a rounded plate of the window's colour with a hairline, flipped so that what it
    /// holds is laid out from its top.
    // SAFETY: NSView is designed for subclassing; Drop is not implemented.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriAppsCard"]
    #[ivars = CardIvars]
    pub(crate) struct CardView;

    unsafe impl NSObjectProtocol for CardView {}

    impl CardView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let plate = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                NSRect::new(
                    NSPoint::new(bounds.origin.x + 0.25, bounds.origin.y + 0.25),
                    NSSize::new(bounds.size.width - 0.5, bounds.size.height - 0.5),
                ),
                RADIUS,
                RADIUS,
            );
            ground().setFill();
            plate.fill();
            NSColor::separatorColor().setStroke();
            plate.setLineWidth(0.5);
            plate.stroke();
        }
    }
);

impl CardView {
    fn new(mtm: MainThreadMarker, size: NSSize, target: &Retained<Target>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(CardIvars {
            _target: Retained::clone(target),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer; the ivars are set.
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), size)]
        };
        this.setWantsLayer(true);
        let shadow = NSShadow::new();
        shadow.setShadowBlurRadius(14.0);
        shadow.setShadowOffset(NSSize::new(0.0, -5.0));
        shadow.setShadowColor(Some(&NSColor::blackColor().colorWithAlphaComponent(0.3)));
        this.setShadow(Some(&shadow));
        this
    }
}

/// The card's colour: the window's, which is the theme's in a window that has one.
pub(crate) fn ground() -> Retained<NSColor> {
    NSColor::windowBackgroundColor()
}

// ─── a mark ───────────────────────────────────────────────────────────────────

pub(super) struct MarkIvars {
    mark: Cell<Mark>,
}

define_class!(
    /// A row that came is a green dot and one that did not is an orange triangle; the turning ring
    /// of one that is on its way is AppKit's own spinner, a subview.
    // SAFETY: NSView is designed for subclassing; Drop is not implemented.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriAppsCardMark"]
    #[ivars = MarkIvars]
    struct MarkView;

    unsafe impl NSObjectProtocol for MarkView {}

    impl MarkView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            match self.ivars().mark.get() {
                Mark::Done => dot(&NSColor::systemGreenColor()),
                Mark::Failed => triangle(&NSColor::systemOrangeColor()),
                Mark::Idle | Mark::Busy => {}
            }
        }
    }
);

impl MarkView {
    fn new(mtm: MainThreadMarker, mark: Mark) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(MarkIvars {
            mark: Cell::new(mark),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer; the ivars are set.
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(MARK, MARK))]
        };
        if mark == Mark::Busy {
            let ring = NSProgressIndicator::initWithFrame(
                NSProgressIndicator::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(MARK, MARK)),
            );
            ring.setStyle(NSProgressIndicatorStyle::Spinning);
            ring.setControlSize(NSControlSize::Small);
            ring.setIndeterminate(true);
            // SAFETY: a plain call on a spinner this function made.
            unsafe { ring.startAnimation(None) };
            this.addSubview(&ring);
        }
        this
    }
}

/// A dot, seven points across, in the middle of the mark's square.
fn dot(color: &NSColor) {
    let at = (MARK - 7.0) / 2.0;
    color.setFill();
    NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
        NSPoint::new(at, at),
        NSSize::new(7.0, 7.0),
    ))
    .fill();
}

/// A warning triangle with its bar and its dot, in a line of one and a half points.
fn triangle(color: &NSColor) {
    color.setStroke();
    let path = NSBezierPath::bezierPath();
    path.setLineWidth(1.4);
    path.setLineCapStyle(NSLineCapStyle::Round);
    path.setLineJoinStyle(NSLineJoinStyle::Round);
    path.moveToPoint(NSPoint::new(8.0, 2.2));
    path.lineToPoint(NSPoint::new(14.3, 13.3));
    path.lineToPoint(NSPoint::new(1.7, 13.3));
    path.closePath();
    path.moveToPoint(NSPoint::new(8.0, 6.4));
    path.lineToPoint(NSPoint::new(8.0, 9.6));
    path.moveToPoint(NSPoint::new(8.0, 11.5));
    path.lineToPoint(NSPoint::new(8.0, 11.6));
    path.stroke();
}

// ─── a name that fades ────────────────────────────────────────────────────────

pub(super) struct NameIvars {
    text: String,
    font: Retained<NSFont>,
    /// The box a click on the name toggles, while the card asks.
    box_of: RefCell<Option<Retained<NSButton>>>,
}

define_class!(
    /// A line of text drawn to the end of its room: what does not fit is not cut with an ellipsis
    /// but fades into the card in the last 22 points.
    // SAFETY: NSView is designed for subclassing; Drop is not implemented.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriAppsCardName"]
    #[ivars = NameIvars]
    struct NameView;

    unsafe impl NSObjectProtocol for NameView {}

    impl NameView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            if let Some(button) = self.ivars().box_of.borrow().as_ref() {
                // SAFETY: a click on a button this card made, sent on the main thread.
                unsafe { button.performClick(None) };
            }
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let ivars = self.ivars();
            let bounds = self.bounds();
            // A view does not clip what it draws to its bounds: the text ends where its room does.
            NSBezierPath::clipRect(bounds);
            let size = self.measure();
            let attributes = attributes(&ivars.font, &NSColor::labelColor());
            let top = ((bounds.size.height - size.height) / 2.0).max(0.0);
            // SAFETY: `attributes` is a dictionary of the keys and value types the call reads;
            // this runs inside `drawRect:`.
            unsafe {
                NSString::from_str(&ivars.text)
                    .drawAtPoint_withAttributes(NSPoint::new(0.0, top), Some(&attributes));
            }
            if size.width > bounds.size.width + 0.5 {
                let from = (bounds.size.width - FADE).max(0.0);
                let band = NSRect::new(
                    NSPoint::new(from, 0.0),
                    NSSize::new(bounds.size.width - from, bounds.size.height),
                );
                let card = ground();
                if let Some(gradient) = NSGradient::initWithStartingColor_endingColor(
                    NSGradient::alloc(),
                    &card.colorWithAlphaComponent(0.0),
                    &card,
                ) {
                    gradient.drawInRect_angle(band, 0.0);
                }
            }
        }
    }
);

impl NameView {
    fn new(mtm: MainThreadMarker, text: &str, font: Retained<NSFont>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(NameIvars {
            text: text.to_owned(),
            font,
            box_of: RefCell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer; the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this
    }

    /// The text's own size: its width whole and its height a line's.
    fn measure(&self) -> NSSize {
        let ivars = self.ivars();
        let attributes = attributes(&ivars.font, &NSColor::labelColor());
        // SAFETY: as `draw_rect`.
        let size = unsafe { NSString::from_str(&ivars.text).sizeWithAttributes(Some(&attributes)) };
        NSSize::new(size.width.ceil(), size.height.ceil())
    }
}

fn attributes(
    font: &NSFont,
    color: &NSColor,
) -> Retained<NSDictionary<objc2_foundation::NSAttributedStringKey, AnyObject>> {
    let values: [&AnyObject; 2] = [color.as_ref(), font.as_ref()];
    // SAFETY: AppKit's two attribute keys (extern statics), each with the value type it documents
    // — an `NSColor` and an `NSFont`.
    unsafe {
        NSDictionary::<objc2_foundation::NSAttributedStringKey, AnyObject>::from_slices(
            &[NSForegroundColorAttributeName, NSFontAttributeName],
            &values,
        )
    }
}

// ─── parts ────────────────────────────────────────────────────────────────────

/// A label of `font` and `color` that wraps within `width`; its frame is as high as its lines.
fn text(
    mtm: MainThreadMarker,
    words: &str,
    font: &NSFont,
    color: &NSColor,
    width: f64,
) -> Retained<NSTextField> {
    let field = NSTextField::wrappingLabelWithString(&NSString::from_str(words), mtm);
    field.setFont(Some(font));
    field.setTextColor(Some(color));
    field.setPreferredMaxLayoutWidth(width);
    let size = field.sizeThatFits(NSSize::new(width, 10_000.0));
    field.setFrame(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(width, size.height.ceil()),
    ));
    field
}

/// The room of a button's title at each side.
const PILL_PAD: f64 = 10.0;

struct PillIvars {
    main: bool,
}

define_class!(
    /// A button of the card: a rounded plate that is the accent for the main one and a tint of the
    /// text for the other, drawn by the button itself so that it reads the same whether or not the
    /// window is the key one (AppKit's own accent button turns grey in a window that is not, and
    /// takes its white title with it).
    // SAFETY: NSButton is designed for subclassing; Drop is not implemented.
    #[unsafe(super(NSButton))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriAppsCardButton"]
    #[ivars = PillIvars]
    struct PillButton;

    unsafe impl NSObjectProtocol for PillButton {}

    impl PillButton {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty: NSRect) {
            let plate = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                self.bounds(),
                7.0,
                7.0,
            );
            let weak = if self.isEnabled() { 1.0 } else { 0.45 };
            let fill = if self.ivars().main {
                NSColor::controlAccentColor().colorWithAlphaComponent(weak)
            } else {
                NSColor::labelColor().colorWithAlphaComponent(0.1 * weak)
            };
            fill.setFill();
            plate.fill();
            if self.isHighlighted() {
                NSColor::blackColor().colorWithAlphaComponent(0.15).setFill();
                plate.fill();
            }
            // The title, as the button's cell draws it.
            // SAFETY: NSButton's own `drawRect:`, inside this one.
            unsafe { msg_send![super(self), drawRect: dirty] }
        }
    }
);

impl PillButton {
    /// The title as the button shows it. Its colours are the system's own and not derived from
    /// them: a colour made from a dynamic one (`colorWithAlphaComponent`) is resolved at once,
    /// in the appearance of the moment, and would stay that when the card is over a dark window.
    fn titled(
        title: &str,
        key: Option<&str>,
        main: bool,
        enabled: bool,
    ) -> Retained<NSMutableAttributedString> {
        let (ink, dim) = if main {
            let white = NSColor::whiteColor();
            if enabled {
                (white, NSColor::colorWithWhite_alpha(1.0, 0.7))
            } else {
                (
                    NSColor::colorWithWhite_alpha(1.0, 0.7),
                    NSColor::colorWithWhite_alpha(1.0, 0.5),
                )
            }
        } else {
            (NSColor::labelColor(), NSColor::secondaryLabelColor())
        };
        let titled = NSMutableAttributedString::from_attributed_nsstring(&plain(
            title,
            &NSFont::systemFontOfSize(12.0),
            &ink,
        ));
        if let Some(key) = key {
            titled.appendAttributedString(&plain(
                &format!("  {key}"),
                &NSFont::systemFontOfSize(11.0),
                &dim,
            ));
        }
        titled
    }
}

/// The width a button wants for its title.
pub(crate) fn wanted_width(button: &NSButton) -> f64 {
    button.attributedTitle().size().width.ceil() + 2.0 * PILL_PAD
}

/// A button the card may have: what it says, its key, and what it does.
#[derive(Clone, Copy)]
struct Spec {
    title: &'static str,
    key: Option<&'static str>,
    /// The accent one.
    main: bool,
    /// One that cannot be pressed is dim.
    enabled: bool,
    action: objc2::runtime::Sel,
}

impl Spec {
    fn new(
        title: &'static str,
        key: Option<&'static str>,
        main: bool,
        enabled: bool,
        action: objc2::runtime::Sel,
    ) -> Self {
        Self {
            title,
            key,
            main,
            enabled,
            action,
        }
    }

    /// The width its title wants in one line, with its key's name or without.
    fn natural(&self, keyed: bool) -> f64 {
        let key = self.key.filter(|_| keyed);
        PillButton::titled(self.title, key, self.main, self.enabled)
            .size()
            .width
            .ceil()
            + 2.0 * PILL_PAD
    }
}

/// Whether a button of the card holds its title: in one line when it is as wide as the title wants,
/// else in the lines it wraps into, which none of its words may be wider than.
pub(crate) fn holds_title(button: &NSButton) -> bool {
    let width = button.frame().size.width;
    if wanted_width(button) <= width + 0.5 {
        return true;
    }
    let font = NSFont::systemFontOfSize(12.0);
    button.title().to_string().split_whitespace().all(|word| {
        plain(word, &font, &NSColor::labelColor())
            .size()
            .width
            .ceil()
            + 2.0 * PILL_PAD
            <= width + 0.5
    }) && button.frame().size.height > BUTTON_H + 0.5
}

/// A button of the card: `title` and, dimmer after it, the name of its key when `keyed`. One whose
/// title is wider than `room` is `room` wide and its title wraps into as many lines as it needs,
/// the button as high as they are.
fn button(
    mtm: MainThreadMarker,
    spec: &Spec,
    keyed: bool,
    target: &Target,
    room: f64,
) -> Retained<NSButton> {
    let this = PillButton::alloc(mtm).set_ivars(PillIvars { main: spec.main });
    // SAFETY: `initWithFrame:` is NSView's designated initializer; the ivars are set.
    let this: Retained<PillButton> = unsafe {
        msg_send![super(this), initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(80.0, BUTTON_H))]
    };
    this.setBordered(false);
    this.setButtonType(NSButtonType::MomentaryPushIn);
    // A click on the card does not take the terminal's keys, whatever the keyboard access is.
    this.setRefusesFirstResponder(true);
    // SAFETY: the target is the card's `Target`, which the card's view keeps; the selector is one
    // of its own actions.
    unsafe {
        this.setTarget(Some(target.as_ref()));
        this.setAction(Some(spec.action));
    }
    let titled = PillButton::titled(
        spec.title,
        spec.key.filter(|_| keyed),
        spec.main,
        spec.enabled,
    );
    let wraps = spec.natural(keyed) > room + 0.01;
    if wraps {
        // The lines keep the room of the title's padding at each side, and stand in the middle.
        let style = NSMutableParagraphStyle::new();
        style.setAlignment(NSTextAlignment::Center);
        style.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        style.setFirstLineHeadIndent(PILL_PAD);
        style.setHeadIndent(PILL_PAD);
        style.setTailIndent(-PILL_PAD);
        // SAFETY: the one attribute and the value type AppKit documents for it, over the whole
        // of the string.
        unsafe {
            titled.addAttribute_value_range(
                NSParagraphStyleAttributeName,
                &style,
                NSRange::new(0, titled.length()),
            );
        }
    }
    this.setAttributedTitle(&titled);
    this.setEnabled(spec.enabled);
    // The key is said whether or not it is shown: the shortcut works.
    this.setAccessibilityLabel(Some(&NSString::from_str(&match spec.key {
        Some(key) => format!("{}, {key}", spec.title),
        None => spec.title.to_owned(),
    })));
    let button: Retained<NSButton> = Retained::into_super(this);
    if !wraps {
        button.setFrameSize(NSSize::new(wanted_width(&button), BUTTON_H));
    } else {
        // Wider than the card's room: the title wraps at its words.
        button.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        if let Some(cell) = button.cell() {
            cell.setWraps(true);
        }
        // As high as the lines the title wraps into, which a label of the same font measures.
        let lines = text(
            mtm,
            spec.title,
            &NSFont::systemFontOfSize(12.0),
            &NSColor::labelColor(),
            (room - 2.0 * PILL_PAD).max(1.0),
        );
        button.setFrameSize(NSSize::new(
            room,
            BUTTON_H.max(lines.frame().size.height + BUTTON_PAD_Y),
        ));
    }
    button
}

/// What a button has above and below its title's lines.
const BUTTON_PAD_Y: f64 = 8.0;

/// The card's buttons, made and measured but not yet placed.
struct Buttons {
    /// In the order they stand in a line, left to right: the one that is the card's answer last.
    views: Vec<Retained<NSButton>>,
    /// One under the other, the last on top; else in a line.
    stacked: bool,
    /// How high they stand together.
    height: f64,
}

impl Buttons {
    /// The buttons of `specs` for a card `inner` wide inside: in a line where they all fit in one,
    /// else one under the other, the answer first. The names of the keys go where one of them
    /// would be wider than the card, and a title that is still wider wraps.
    fn make(
        mtm: MainThreadMarker,
        specs: &[Spec],
        target: &Target,
        inner: f64,
        stacked_always: bool,
    ) -> Self {
        let line: f64 = specs.iter().map(|spec| spec.natural(true)).sum::<f64>()
            + BUTTON_GAP * specs.len().saturating_sub(1) as f64;
        if !stacked_always && line <= inner {
            return Self {
                views: specs
                    .iter()
                    .map(|spec| button(mtm, spec, true, target, inner))
                    .collect(),
                stacked: false,
                height: BUTTON_H,
            };
        }
        let widest = specs
            .iter()
            .map(|spec| spec.natural(true))
            .fold(0.0, f64::max);
        let keyed = widest <= inner;
        let views: Vec<_> = specs
            .iter()
            .map(|spec| button(mtm, spec, keyed, target, inner))
            .collect();
        let height = views
            .iter()
            .map(|view| view.frame().size.height)
            .sum::<f64>()
            + BUTTON_GAP * views.len().saturating_sub(1) as f64;
        Self {
            views,
            stacked: true,
            height,
        }
    }
}

fn plain(words: &str, font: &NSFont, color: &NSColor) -> Retained<NSAttributedString> {
    let attributes = attributes(font, color);
    // SAFETY: `attributes` is a dictionary of attribute keys and values AppKit reads.
    unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(words),
            Some(&attributes),
        )
    }
}

/// Where the rows of a list stand and what they are wired to.
#[derive(Clone, Copy)]
struct Rows<'a> {
    mtm: MainThreadMarker,
    /// The list's view.
    parent: &'a NSView,
    width: f64,
    /// Whether the card asks: a row then has a box and not a mark.
    asking: bool,
    target: &'a Target,
}

/// One row: its mark, its name, and under the name what went wrong. Placed at `top` of the
/// list; returns how high it is.
fn row(rows: &Rows, top: f64, index: usize, line: &Line) -> f64 {
    let Rows {
        mtm,
        parent,
        width,
        asking,
        target,
    } = *rows;
    let name_font = NSFont::systemFontOfSize(12.5);
    let name_at = MARK + MARK_GAP;
    let name_width = (width - name_at).max(0.0);
    if asking {
        // SAFETY: the target is the card's `Target`; the selector is its own.
        let boxed = unsafe {
            NSButton::checkboxWithTitle_target_action(
                ns_empty(),
                Some(target.as_ref()),
                Some(sel!(toggle:)),
                mtm,
            )
        };
        boxed.setControlSize(NSControlSize::Small);
        boxed.setRefusesFirstResponder(true);
        boxed.setState(if line.on { 1 } else { 0 });
        boxed.setTag(isize::try_from(index).unwrap_or(0));
        boxed.setAccessibilityLabel(Some(&NSString::from_str(&line.row.name)));
        boxed.setFrame(NSRect::new(NSPoint::new(0.0, top), NSSize::new(MARK, MARK)));
        parent.addSubview(&boxed);
        let name = NameView::new(mtm, &line.row.name, name_font);
        *name.ivars().box_of.borrow_mut() = Some(boxed);
        name.setFrame(NSRect::new(
            NSPoint::new(name_at, top),
            NSSize::new(name_width, MARK),
        ));
        parent.addSubview(&name);
        return MARK;
    }
    let mark = MarkView::new(mtm, line.mark);
    mark.setFrame(NSRect::new(NSPoint::new(0.0, top), NSSize::new(MARK, MARK)));
    parent.addSubview(&mark);
    let name = NameView::new(mtm, &line.row.name, name_font);
    // What the mark and the line under the name show is said with the name: a screen reader finds
    // no dot or triangle, and the line under it is a label of its own that is not read with it.
    name.setAccessibilityLabel(Some(&NSString::from_str(&spoken(line))));
    name.setIdentifier(Some(&NSString::from_str(&format!("name-{index}"))));
    name.setFrame(NSRect::new(
        NSPoint::new(name_at, top),
        NSSize::new(name_width, MARK),
    ));
    parent.addSubview(&name);
    let mut height = MARK;
    // Under the name: why it failed, or how far it has come.
    let under = if line.why.is_empty() {
        &line.note
    } else {
        &line.why
    };
    if !under.is_empty() {
        let why = text(
            mtm,
            under,
            &NSFont::systemFontOfSize(11.5),
            &NSColor::secondaryLabelColor(),
            name_width,
        );
        why.setIdentifier(Some(&NSString::from_str(&format!("note-{index}"))));
        // The label's own inset of two points is taken back, so that it begins where the name does.
        why.setFrameOrigin(NSPoint::new(name_at - 2.0, top + MARK + 1.0));
        height = MARK + 1.0 + why.frame().size.height;
        parent.addSubview(&why);
    }
    height
}

/// What a row says to a screen reader: its name, how it stands, and what is under the name.
pub(crate) fn spoken(line: &Line) -> String {
    let stands = match line.mark {
        Mark::Idle => "",
        Mark::Busy => "on its way",
        Mark::Done => "ready",
        Mark::Failed => "failed",
    };
    let under = if line.why.is_empty() {
        &line.note
    } else {
        &line.why
    };
    let mut said = line.row.name.clone();
    for part in [stands, under.as_str()] {
        if !part.is_empty() {
            said.push_str(", ");
            said.push_str(part);
        }
    }
    said
}

fn ns_empty() -> &'static NSString {
    objc2_foundation::ns_string!("")
}

/// How high the card may be of the pane it hangs over: the rest is the terminal's.
const ROOM_SHARE: f64 = 0.8;

/// A list of at most this many rows does not scroll for being long ([`LIST_MAX`]).
const FEW_ROWS: usize = 3;

/// The least a list is let shrink to for a small pane: about two rows.
const LIST_MIN: f64 = 44.0;

/// The card as the model says it is, as wide as the pane lets it be ([`width_for`]) and as high as
/// it needs, with its buttons wired to `target`. `room` is the size of the pane it hangs over: a
/// list that would not fit scrolls in less than [`LIST_MAX`]. Its origin is the corner of the
/// pane's ([`crate::pane::CORNER_INSET`]) and is the pane's to set.
pub(crate) fn build(
    mtm: MainThreadMarker,
    card: &Card,
    target: &Retained<Target>,
    room: NSSize,
) -> Retained<CardView> {
    let width = width_for(room.width);
    let inner = width - 2.0 * PAD_X;
    let asking = card.stage() == Stage::Ask;
    // The card of a click says its one program in its title, and asks nothing about it: a list
    // would say it twice, and a box would offer what the click did not. The card of the first
    // click's question has no list at all: what it says is in its lines under the title.
    let here = card.is_here();
    let link_ask = asking && card.link().is_some() && !here;
    // A yes that could not be written started nothing: no row says it did.
    let lines: &[Line] = if link_ask || (here && asking) || card.not_saved() {
        &[]
    } else {
        card.lines()
    };
    let view = CardView::new(mtm, NSSize::new(width, 40.0), target);
    let mut y = PAD_Y;

    let title = text(
        mtm,
        &card.title(),
        &NSFont::systemFontOfSize_weight(12.5, 0.3),
        &NSColor::labelColor(),
        inner,
    );
    title.setFrameOrigin(NSPoint::new(PAD_X, y));
    view.addSubview(&title);
    y += title.frame().size.height;

    // What the first click's card says besides its question: what it would download, how far it
    // has come, why it did not, and where the answer can be changed.
    for note in card.notes() {
        y += NOTE_GAP;
        let note = text(
            mtm,
            &note,
            &NSFont::systemFontOfSize(11.5),
            &NSColor::secondaryLabelColor(),
            inner,
        );
        note.setFrameOrigin(NSPoint::new(PAD_X, y));
        view.addSubview(&note);
        y += note.frame().size.height;
    }

    // The list, in a box of its own so that a long one scrolls.
    let list = FlippedView::new(mtm);
    let rows = Rows {
        mtm,
        parent: &list,
        width: inner,
        asking,
        target,
    };
    let scrolls = {
        let mut at = 0.0;
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                at += ROW_GAP;
            }
            at += row(&rows, at, i, line);
        }
        at
    };
    // The two answers of a click's card are too wide for one line of a card this wide: one under
    // the other, the one that installs first.
    let buttons = Buttons::make(
        mtm,
        &specs(card),
        target,
        inner,
        link_ask || (here && asking),
    );
    // What is around the list: the title above (`y` so far), the gap, the buttons and the padding.
    let around = y
        + 8.0
        + if buttons.views.is_empty() {
            0.0
        } else {
            BUTTONS_TOP + buttons.height
        }
        + PAD_Y;
    // A list of a few rows is as high as its words make it (in a narrow card they wrap into more
    // lines, and a reason is not to be cut off to keep the list short); a long one scrolls beyond
    // [`LIST_MAX`]. Both keep to the pane's room.
    let cap = if lines.len() <= FEW_ROWS {
        f64::INFINITY
    } else {
        LIST_MAX
    };
    let list_max = (room.height * ROOM_SHARE - around).clamp(LIST_MIN, cap);
    let shown = scrolls.min(list_max);
    y += 8.0;
    if scrolls > list_max {
        // Room for the scroller beside the rows is not kept: the scroller floats over them and
        // hides when the list is not moving.
        list.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(inner, scrolls),
        ));
        let scroll = NSScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        scroll.setDocumentView(Some(&list));
        scroll.setFrame(NSRect::new(
            NSPoint::new(PAD_X, y),
            NSSize::new(inner, shown),
        ));
        view.addSubview(&scroll);
    } else {
        list.setFrame(NSRect::new(
            NSPoint::new(PAD_X, y),
            NSSize::new(inner, scrolls),
        ));
        view.addSubview(&list);
    }
    y += shown;

    if !buttons.views.is_empty() {
        y += BUTTONS_TOP;
        if buttons.stacked {
            for (at, button) in buttons.views.iter().rev().enumerate() {
                let size = button.frame().size;
                button.setFrameOrigin(NSPoint::new(width - PAD_X - size.width, y));
                view.addSubview(button);
                y += size.height;
                if at + 1 < buttons.views.len() {
                    y += BUTTON_GAP;
                }
            }
        } else {
            let mut right = width - PAD_X;
            for button in buttons.views.iter().rev() {
                let size = button.frame().size;
                button.setFrameOrigin(NSPoint::new(right - size.width, y));
                view.addSubview(button);
                right -= size.width + BUTTON_GAP;
            }
            y += BUTTON_H;
        }
    }
    y += PAD_Y;
    view.setFrameSize(NSSize::new(width, y));
    view.setAccessibilityElement(true);
    view.setAccessibilityLabel(Some(&NSString::from_str(&card.title())));
    view
}

/// The buttons the card has in its stage, in the order they stand in a line, left to right: the
/// card's answer last.
fn specs(card: &Card) -> Vec<Spec> {
    let here = card.is_here();
    let enter = Some("\u{2318}Enter");
    let escape = Some("\u{2318}Esc");
    match card.stage() {
        Stage::Ask if here => vec![
            Spec::new("Open in Your Browser", escape, false, true, sel!(notHere:)),
            Spec::new(card.yes_title(), enter, true, true, sel!(install:)),
        ],
        // The download goes on, and the address need not wait for it.
        Stage::Installing if here => vec![Spec::new(
            "Open in Your Browser Now",
            None,
            false,
            true,
            sel!(notHere:),
        )],
        // As a server's failed card has its two: the way out, and the try again.
        Stage::Failed if here => vec![
            Spec::new("Open in Your Browser", None, false, true, sel!(notHere:)),
            Spec::new("Try Again", None, true, true, sel!(retry:)),
        ],
        // A click's card that failed has the address to open, and says so: closing it opens it in
        // the system's browser.
        Stage::Failed if card.link().is_some() => vec![
            Spec::new("Open in Your Browser", None, false, true, sel!(close:)),
            Spec::new("Try Again", None, true, true, sel!(retry:)),
        ],
        Stage::Ask if card.link().is_some() => vec![
            Spec::new("Open in Your Browser", escape, false, true, sel!(notHere:)),
            Spec::new(
                "Install and Open",
                enter,
                true,
                card.can_install(),
                sel!(install:),
            ),
        ],
        Stage::Ask => vec![
            Spec::new("Not Here", escape, false, true, sel!(notHere:)),
            Spec::new("Install", enter, true, card.can_install(), sel!(install:)),
        ],
        Stage::Failed => vec![
            Spec::new("Close", None, false, true, sel!(close:)),
            Spec::new("Try Again", None, true, true, sel!(retry:)),
        ],
        Stage::Installing | Stage::Done => Vec::new(),
    }
}

/// Brings the view of a card up to date with the model where only how far a download has come
/// changed: the number under the name of a row is written in its label, and nothing is made
/// again, so that the ring beside it goes on turning. `false` when something else changed (or the
/// number comes or goes, which changes the row's height, or the pane is not the size the card was
/// made for, `built`, but `now` ([`made_for`])): the card is to be made again.
pub(crate) fn refresh_notes(
    view: &NSView,
    before: &Card,
    after: &Card,
    built: NSSize,
    now: NSSize,
) -> bool {
    // The pane's size is what the card was made for: a card of another room is made again.
    if !made_for(built, now) {
        return false;
    }
    if before.stage() != after.stage()
        || before.title() != after.title()
        || before.notes() != after.notes()
        || before.lines().len() != after.lines().len()
    {
        return false;
    }
    for (old, new) in before.lines().iter().zip(after.lines()) {
        if old.row != new.row || old.on != new.on || old.mark != new.mark || old.why != new.why {
            return false;
        }
    }
    for (index, (old, new)) in before.lines().iter().zip(after.lines()).enumerate() {
        if old.note == new.note {
            continue;
        }
        if old.note.is_empty() || new.note.is_empty() {
            return false;
        }
        let Some(field) = labelled(view, &format!("note-{index}"))
            .and_then(|found| found.downcast::<NSTextField>().ok())
        else {
            return false;
        };
        field.setStringValue(&NSString::from_str(&new.note));
        let fits = field
            .sizeThatFits(NSSize::new(field.preferredMaxLayoutWidth(), 10_000.0))
            .height
            .ceil();
        if (fits - field.frame().size.height).abs() > 0.5 {
            return false;
        }
        if let Some(name) = labelled(view, &format!("name-{index}")) {
            name.setAccessibilityLabel(Some(&NSString::from_str(&spoken(new))));
        }
    }
    true
}

/// The view under `view` whose identifier is `identifier`.
pub(crate) fn labelled(view: &NSView, identifier: &str) -> Option<Retained<NSView>> {
    for sub in view.subviews() {
        if sub
            .identifier()
            .is_some_and(|found| found.to_string() == identifier)
        {
            return Some(sub);
        }
        if let Some(found) = labelled(&sub, identifier) {
            return Some(found);
        }
    }
    None
}

// ─── a flipped container ──────────────────────────────────────────────────────

define_class!(
    /// A plain view whose origin is its top left, which the rows are laid out from.
    // SAFETY: NSView is designed for subclassing; Drop is not implemented.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriAppsCardList"]
    #[ivars = ()]
    struct FlippedView;

    unsafe impl NSObjectProtocol for FlippedView {}

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl FlippedView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:` is NSView's designated initializer; the ivars are set.
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }
}

/// How far the list of the card is scrolled, if it scrolls: kept across a redraw, which builds the
/// card again.
pub(crate) fn scrolled(card: &NSView) -> Option<f64> {
    card.subviews().iter().find_map(|sub| {
        sub.downcast_ref::<NSScrollView>()
            .map(|scroll| scroll.contentView().bounds().origin.y)
    })
}

/// Scrolls the list of the card, if it scrolls, to `offset` ([`scrolled`]).
pub(crate) fn scroll_to(card: &NSView, offset: f64) {
    for sub in card.subviews() {
        if let Some(scroll) = sub.downcast_ref::<NSScrollView>() {
            let clip = scroll.contentView();
            clip.scrollToPoint(NSPoint::new(0.0, offset));
            scroll.reflectScrolledClipView(&clip);
        }
    }
}

/// The controls of a card that are pressed, for the layout check to count.
pub(crate) fn controls(view: &NSView) -> Vec<Retained<NSControl>> {
    let mut found = Vec::new();
    collect(view, &mut found);
    found
}

fn collect(view: &NSView, found: &mut Vec<Retained<NSControl>>) {
    for sub in view.subviews() {
        if let Some(control) = sub.downcast_ref::<NSControl>() {
            found.push(control.retain());
        }
        collect(&sub, found);
    }
}
