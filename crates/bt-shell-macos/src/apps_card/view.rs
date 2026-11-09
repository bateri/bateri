//! The views of the card ([`super`]): drawn from the model ([`Card`]) every time it changes, in
//! the system's own colours, so that it follows the appearance of the window it is over.
//!
//! **A frame is a calculation.** The card is laid out by hand, top to bottom, in a flipped view:
//! the title (which wraps), a list of rows (a mark, a name that fades where it does not fit, and
//! under it the reason a row failed), and the buttons at the right. A list longer than
//! [`LIST_MAX`] scrolls inside its box, so that the card never grows over the whole pane.
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
    NSForegroundColorAttributeName, NSGradient, NSLineCapStyle, NSLineJoinStyle,
    NSProgressIndicator, NSProgressIndicatorStyle, NSScrollView, NSShadow, NSStringDrawing,
    NSTextField, NSView,
};
use objc2_foundation::{
    NSAttributedString, NSDictionary, NSMutableAttributedString, NSObjectProtocol, NSPoint, NSRect,
    NSSize, NSString,
};

use super::Act;

/// The card's width.
pub(crate) const WIDTH: f64 = 264.0;
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

/// A button of the card: `title` and, dimmer after it, the name of its key. `main` is the accent
/// one; one that cannot be pressed is dim.
fn button(
    mtm: MainThreadMarker,
    title: &str,
    key: Option<&str>,
    main: bool,
    enabled: bool,
    target: &Target,
    action: objc2::runtime::Sel,
) -> Retained<NSButton> {
    let this = PillButton::alloc(mtm).set_ivars(PillIvars { main });
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
        this.setAction(Some(action));
    }
    this.setAttributedTitle(&PillButton::titled(title, key, main, enabled));
    this.setEnabled(enabled);
    this.setAccessibilityLabel(Some(&NSString::from_str(&match key {
        Some(key) => format!("{title}, {key}"),
        None => title.to_owned(),
    })));
    let button: Retained<NSButton> = Retained::into_super(this);
    button.setFrameSize(NSSize::new(wanted_width(&button), BUTTON_H));
    button
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
    name.setAccessibilityLabel(Some(&NSString::from_str(&line.row.name)));
    name.setFrame(NSRect::new(
        NSPoint::new(name_at, top),
        NSSize::new(name_width, MARK),
    ));
    parent.addSubview(&name);
    let mut height = MARK;
    if !line.why.is_empty() {
        let why = text(
            mtm,
            &line.why,
            &NSFont::systemFontOfSize(11.5),
            &NSColor::secondaryLabelColor(),
            name_width,
        );
        // The label's own inset of two points is taken back, so that it begins where the name does.
        why.setFrameOrigin(NSPoint::new(name_at - 2.0, top + MARK + 1.0));
        height = MARK + 1.0 + why.frame().size.height;
        parent.addSubview(&why);
    }
    height
}

fn ns_empty() -> &'static NSString {
    objc2_foundation::ns_string!("")
}

/// How high the card may be of the pane it hangs over: the rest is the terminal's.
const ROOM_SHARE: f64 = 0.8;

/// The least a list is let shrink to for a small pane: about two rows.
const LIST_MIN: f64 = 44.0;

/// The card as the model says it is, `WIDTH` wide and as high as it needs, with its buttons wired
/// to `target`. `room` is the height of the pane it hangs over: a list that would not fit scrolls
/// in less than [`LIST_MAX`]. Its origin is the corner of the pane's
/// ([`crate::pane::CORNER_INSET`]) and is the pane's to set.
pub(crate) fn build(
    mtm: MainThreadMarker,
    card: &Card,
    target: &Retained<Target>,
    room: f64,
) -> Retained<CardView> {
    let inner = WIDTH - 2.0 * PAD_X;
    let asking = card.stage() == Stage::Ask;
    let view = CardView::new(mtm, NSSize::new(WIDTH, 40.0), target);
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
        for (i, line) in card.lines().iter().enumerate() {
            if i > 0 {
                at += ROW_GAP;
            }
            at += row(&rows, at, i, line);
        }
        at
    };
    // What is around the list: the title above (`y` so far), the gap, the buttons and the padding.
    let buttons_wanted = matches!(card.stage(), Stage::Ask | Stage::Failed);
    let around = y
        + 8.0
        + if buttons_wanted {
            BUTTONS_TOP + BUTTON_H
        } else {
            0.0
        }
        + PAD_Y;
    let list_max = (room * ROOM_SHARE - around).clamp(LIST_MIN, LIST_MAX);
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

    let buttons: Vec<Retained<NSButton>> = match card.stage() {
        Stage::Ask => vec![
            button(
                mtm,
                "Not Here",
                Some("\u{2318}Esc"),
                false,
                true,
                target,
                sel!(notHere:),
            ),
            button(
                mtm,
                "Install",
                Some("\u{2318}Enter"),
                true,
                card.can_install(),
                target,
                sel!(install:),
            ),
        ],
        Stage::Failed => vec![
            button(mtm, "Close", None, false, true, target, sel!(close:)),
            button(mtm, "Try Again", None, true, true, target, sel!(retry:)),
        ],
        Stage::Installing | Stage::Done => Vec::new(),
    };
    if !buttons.is_empty() {
        y += BUTTONS_TOP;
        let mut right = WIDTH - PAD_X;
        for button in buttons.iter().rev() {
            let size = button.frame().size;
            button.setFrameOrigin(NSPoint::new(right - size.width, y));
            view.addSubview(button);
            right -= size.width + BUTTON_GAP;
        }
        y += BUTTON_H;
    }
    y += PAD_Y;
    view.setFrameSize(NSSize::new(WIDTH, y));
    view.setAccessibilityElement(true);
    view.setAccessibilityLabel(Some(&NSString::from_str(&card.title())));
    view
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
