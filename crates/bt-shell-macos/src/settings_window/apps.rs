//! Settings ▸ Apps: the programs bateri follows (`[apps] sources`), their signs, and the sheets that
//! ask before a key is approved or a program taken off.
//!
//! What a row says and what the lists are is decided by `bt_shell_common::apps::page`, which a
//! test can reach; this file draws it and sends what the person does. Like the rest of the
//! window it **holds no state of its own**: the list is the file's, the statuses are the tracker's,
//! and what a button does is written to `settings.toml` (`SettingsEdit::AppsSources`) or kept in the
//! apps folder (an approved key). A row comes back on screen only from there.
//!
//! **A row is a name, a version and a source, and at the right a sign** — a dot, a ring or a
//! triangle — whose words are AppKit's own tool tip and the screen reader's label: what is, and
//! under it what will happen. What does not fit a line fades in its last 22 points and stands in
//! full in the tool tip ([`FadeText`]).
//!
//! **Add looks before it writes.** The source typed is read once, off the main thread
//! ([`bt_shell_common::apps::look`]); a program signed with the key that comes in the application
//! is followed at once, any other waits for a sheet that says who signed it. Saying no writes
//! nothing; saying yes keeps the key and then writes the list. Remove asks first, and what it
//! writes is the list without the entry: the tracker's next check deletes the program.

use std::cell::{Cell, RefCell};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use block2::RcBlock;
use bt_core::SettingsEdit;
use bt_shell_common::apps::page::{self, Button, Review, Row as AppRow, Sign, Tracked};
use bt_shell_common::apps::{Config, Curl, Failure, Look};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSAccessibility, NSAppearanceCustomization, NSBackingStoreType, NSBezelStyle, NSBezierPath,
    NSBox, NSBoxType, NSButton, NSButtonType, NSColor, NSControl, NSControlSize, NSEvent, NSFont,
    NSGradient, NSImage, NSImageSymbolConfiguration, NSLayoutAttribute, NSLayoutConstraint,
    NSLineBreakMode, NSLineCapStyle, NSLineJoinStyle, NSModalResponse, NSProgressIndicator,
    NSProgressIndicatorStyle, NSShadow, NSStackView, NSStringDrawing, NSTextField, NSTitlePosition,
    NSTrackingArea, NSTrackingAreaOptions, NSUserInterfaceLayoutOrientation, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{
    NSAttributedString, NSCalendar, NSDate, NSDateFormatter, NSLocale, NSMutableAttributedString,
    NSObjectNSDelayedPerforming, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, ns_string,
};

use super::look::{self, Ink};
use super::{
    BoxedField, Row, SettingsWindow, activate, add_pinned, ink_button, pictures, stack, surface,
    text_label, theme_box_sized, width_constraint, wrapping_text_label,
};
use crate::app;

/// The side of a sign's square.
const SIGN: f64 = 16.0;
/// A button of a row: its height, and the width of the one that holds a picture only.
const BUTTON_HEIGHT: f64 = 24.0;
const TRASH_WIDTH: f64 = 26.0;
/// How much of a line's end fades when it does not fit.
const FADE: f64 = 22.0;
/// A sheet's width, and the width of its text.
const SHEET_WIDTH: f64 = 460.0;
const SHEET_PAD: f64 = 24.0;
const SHEET_TEXT: f64 = SHEET_WIDTH - 2.0 * SHEET_PAD;
/// The width of the names of the lines that tell who signed.
const KEY_LABEL: f64 = 104.0;
/// How long the toast after a removal stays.
const TOAST_SECONDS: f64 = 2.6;
/// How long the pointer rests on a sign, or on a line cut short, before its words show.
const TIP_DELAY: f64 = 0.45;
/// The widest a tip grows before its lines wrap.
const TIP_WIDTH: f64 = 250.0;

// ─── a line that fades ────────────────────────────────────────────────────────

pub(super) struct FadeIvars {
    text: String,
    font: Retained<NSFont>,
    ink: Ink,
}

define_class!(
    /// One line of text drawn to the end of its room: what does not fit is not cut with an
    /// ellipsis but fades into the card in the last 22 points. When it is cut, the pointer
    /// resting on it shows the whole text in a tip ([`show_tip`]).
    // SAFETY: NSView is designed for subclassing; Drop is not implemented.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriAppsFadeText"]
    #[ivars = FadeIvars]
    struct FadeText;

    unsafe impl NSObjectProtocol for FadeText {}

    impl FadeText {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            tip_wait(self);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            tip_leave(self);
        }

        #[unsafe(method(showTip:))]
        fn show_tip(&self, _sender: Option<&AnyObject>) {
            if self.measure().width > self.bounds().size.width + 0.5 {
                show_tip(self, &self.ivars().text, None);
            }
        }

        #[unsafe(method(intrinsicContentSize))]
        fn intrinsic_content_size(&self) -> NSSize {
            self.measure()
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let ivars = self.ivars();
            let bounds = self.bounds();
            // A view does not clip what it draws to its bounds: the text ends where its room does.
            NSBezierPath::clipRect(bounds);
            let size = self.measure();
            let attributes = pictures::attributes(&ivars.font, &look::ink(ivars.ink));
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
                let clear = look::faded(Ink::Card, 0.0);
                let card = look::ink(Ink::Card);
                if let Some(gradient) = NSGradient::initWithStartingColor_endingColor(
                    NSGradient::alloc(),
                    &clear,
                    &card,
                ) {
                    gradient.drawInRect_angle(band, 0.0);
                }
            }
        }
    }
);

impl FadeText {
    fn new(mtm: MainThreadMarker, text: &str, font: Retained<NSFont>, ink: Ink) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(FadeIvars {
            text: text.to_owned(),
            font,
            ink,
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer; the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        let said = NSString::from_str(text);
        this.setAccessibilityElement(true);
        this.setAccessibilityLabel(Some(&said));
        tip_track(&this);
        // The text gives way before what stands beside it.
        this.setContentCompressionResistancePriority_forOrientation(
            250.0,
            objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
        );
        this.setContentHuggingPriority_forOrientation(
            250.0,
            objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
        );
        this
    }

    /// The text's own size: its width whole and its height a line's.
    fn measure(&self) -> NSSize {
        let ivars = self.ivars();
        let attributes = pictures::attributes(&ivars.font, &look::ink(ivars.ink));
        // SAFETY: as `draw_rect`.
        let size = unsafe { NSString::from_str(&ivars.text).sizeWithAttributes(Some(&attributes)) };
        NSSize::new(size.width.ceil(), size.height.ceil())
    }
}

// ─── a sign ───────────────────────────────────────────────────────────────────

pub(super) struct SignIvars {
    sign: Sign,
    /// The tip's two lines ([`page::Tip`]).
    title: String,
    more: Option<String>,
}

define_class!(
    /// A row's state as a mark: a dot (green when all is well, grey when there is nothing to do
    /// here), a turning ring while the shell looks, a triangle (orange, red when a key changed).
    /// Its words are the tip the pointer resting on it shows ([`show_tip`]) and its
    /// accessibility label.
    // SAFETY: NSView is designed for subclassing; Drop is not implemented.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriAppsSign"]
    #[ivars = SignIvars]
    struct SignView;

    unsafe impl NSObjectProtocol for SignView {}

    impl SignView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            tip_wait(self);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            tip_leave(self);
        }

        #[unsafe(method(showTip:))]
        fn show_tip(&self, _sender: Option<&AnyObject>) {
            let ivars = self.ivars();
            show_tip(self, &ivars.title, ivars.more.as_deref());
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            match self.ivars().sign {
                Sign::Good => dot(&look::ink(Ink::Success)),
                Sign::Quiet => dot(&look::faded(Ink::Dim, 0.7)),
                Sign::Warn => triangle(&look::ink(Ink::Warning)),
                Sign::Bad => triangle(&look::ink(Ink::Error)),
                // The ring is a spinner of AppKit's, a subview.
                Sign::Busy => {}
            }
        }
    }
);

/// A dot, seven points across, in the middle of the sign's square.
fn dot(color: &objc2_app_kit::NSColor) {
    let at = (SIGN - 7.0) / 2.0;
    color.setFill();
    NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
        NSPoint::new(at, at),
        NSSize::new(7.0, 7.0),
    ))
    .fill();
}

/// A warning triangle with its bar and its dot, drawn in a line of one and a half points.
fn triangle(color: &objc2_app_kit::NSColor) {
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

impl SignView {
    fn new(mtm: MainThreadMarker, row: &AppRow) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SignIvars {
            sign: row.sign,
            title: row.tip.title.clone(),
            more: row.tip.more.clone(),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer; the ivars are set.
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(SIGN, SIGN))]
        };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        width_constraint(&this, SIGN);
        this.heightAnchor()
            .constraintEqualToConstant(SIGN)
            .setActive(true);
        tip_track(&this);
        this.setAccessibilityElement(true);
        this.setAccessibilityLabel(Some(&NSString::from_str(&row.tip.spoken())));
        if row.sign == Sign::Busy {
            let ring = NSProgressIndicator::initWithFrame(
                NSProgressIndicator::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(SIGN, SIGN)),
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

// ─── when ─────────────────────────────────────────────────────────────────────

/// When a check ended, in the words of a tip: "today at 12:31", "yesterday at 9:05", or
/// "on 5 Nov at 12:31". The clock follows the person's 12- or 24-hour setting; the words are
/// English, as the page is.
fn when(at: SystemTime) -> Option<String> {
    let seconds = at.duration_since(UNIX_EPOCH).ok()?.as_secs_f64();
    let date = NSDate::dateWithTimeIntervalSince1970(seconds);
    let calendar = NSCalendar::currentCalendar();
    let clock = NSDateFormatter::new();
    clock.setLocalizedDateFormatFromTemplate(ns_string!("jmm"));
    let clock = clock.stringFromDate(&date);
    let day = if calendar.isDateInToday(&date) {
        "today".to_owned()
    } else if calendar.isDateInYesterday(&date) {
        "yesterday".to_owned()
    } else {
        let day = NSDateFormatter::new();
        day.setLocale(Some(&NSLocale::localeWithLocaleIdentifier(ns_string!(
            "en_US_POSIX"
        ))));
        day.setDateFormat(Some(ns_string!("d MMM")));
        format!("on {}", day.stringFromDate(&date))
    };
    Some(format!("{day} at {clock}"))
}

// ─── the tip ──────────────────────────────────────────────────────────────────

thread_local! {
    /// The tip on screen: one for the window.
    static TIP: RefCell<Option<Retained<NSBox>>> = const { RefCell::new(None) };
}

/// Lets `view` hear the pointer come and go, for its tip.
fn tip_track(view: &NSView) {
    let options = NSTrackingAreaOptions::MouseEnteredAndExited
        | NSTrackingAreaOptions::ActiveInActiveApp
        | NSTrackingAreaOptions::InVisibleRect;
    // SAFETY: the owner is the view, which owns the area and outlives it; no user info.
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

/// The pointer came onto `view`: its `showTip:` is sent after [`TIP_DELAY`], unless it leaves.
fn tip_wait(view: &NSView) {
    tip_leave(view);
    // SAFETY: `showTip:` is the view's own method and takes an object argument, nil here; the
    // request retains the view until it runs or is cancelled.
    unsafe { view.performSelector_withObject_afterDelay(sel!(showTip:), None, TIP_DELAY) };
}

/// The pointer left `view`: a tip on its way does not come, one on screen goes.
fn tip_leave(view: &NSView) {
    // SAFETY: the target is the view and the selector its own; the argument is the nil the
    // request was made with.
    unsafe {
        NSObject::cancelPreviousPerformRequestsWithTarget_selector_object(
            view.as_ref(),
            sel!(showTip:),
            None,
        );
    }
    hide_tip();
}

/// The tip goes, if one is on screen.
pub(super) fn hide_tip() {
    if let Some(tip) = TIP.with(|tip| tip.borrow_mut().take()) {
        tip.removeFromSuperview();
    }
}

/// `title`, and `more` under it in the dim colour, in a bubble of the window's colours under
/// `over`, centred on it and kept inside the window (above it when there is no room below).
/// AppKit's tool tip is not used: it waits longer and wants the pointer still on a 16-point mark,
/// so a person passing over a sign saw nothing.
fn show_tip(over: &NSView, title: &str, more: Option<&str>) {
    hide_tip();
    let mtm = over.mtm();
    let Some(content) = over.window().and_then(|window| window.contentView()) else {
        return;
    };
    let line = |text: &str, ink: Ink, weight: f64| {
        let label = wrapping_text_label(&NSString::from_str(text), mtm);
        label.setFont(Some(&NSFont::systemFontOfSize_weight(11.5, weight)));
        label.setTextColor(Some(&look::ink(ink)));
        label.setPreferredMaxLayoutWidth(TIP_WIDTH - 16.0);
        label
    };
    let title = line(title, Ink::Text, 0.3);
    let mut lines: Vec<&NSView> = vec![&title];
    let more = more
        .filter(|more| !more.is_empty())
        .map(|more| line(more, Ink::Dim, 0.0));
    if let Some(more) = &more {
        lines.push(more);
    }
    let column = stack(
        mtm,
        &lines,
        NSUserInterfaceLayoutOrientation::Vertical,
        1.0,
        NSLayoutAttribute::Leading,
    );
    let tip = NSBox::new(mtm);
    tip.setBoxType(NSBoxType::Custom);
    tip.setTitlePosition(NSTitlePosition::NoTitle);
    tip.setCornerRadius(6.0);
    tip.setBorderWidth(0.5);
    tip.setBorderColor(&look::ink(Ink::Line));
    tip.setFillColor(&look::ink(Ink::Card));
    fill(&tip, &column, 8.0, 5.5);
    let shadow = NSShadow::new();
    shadow.setShadowBlurRadius(10.0);
    shadow.setShadowOffset(NSSize::new(0.0, -3.0));
    shadow.setShadowColor(Some(&NSColor::colorWithWhite_alpha(0.0, 0.22)));
    tip.setWantsLayer(true);
    tip.setShadow(Some(&shadow));
    tip.setAccessibilityElement(false);
    let size = tip.fittingSize();
    let mark = content.convertRect_fromView(over.bounds(), Some(over));
    let room = content.bounds();
    let x = (mark.origin.x + mark.size.width / 2.0 - size.width / 2.0)
        .min(room.size.width - size.width - 8.0)
        .max(8.0);
    // Under the mark: lower on screen, which is a smaller y unless the view is flipped.
    let (below, above) = if content.isFlipped() {
        (
            mark.origin.y + mark.size.height + 6.0,
            mark.origin.y - 6.0 - size.height,
        )
    } else {
        (
            mark.origin.y - 6.0 - size.height,
            mark.origin.y + mark.size.height + 6.0,
        )
    };
    let fits = |y: f64| y >= 0.0 && y + size.height <= room.size.height;
    let y = if fits(below) { below } else { above };
    tip.setTranslatesAutoresizingMaskIntoConstraints(true);
    tip.setFrame(NSRect::new(NSPoint::new(x, y), size));
    content.addSubview(&tip);
    TIP.with(|slot| *slot.borrow_mut() = Some(tip));
}

// ─── the trash can ────────────────────────────────────────────────────────────

define_class!(
    /// The button that takes a program off: a trash can that turns red under the pointer.
    // SAFETY: NSButton is designed for subclassing; Drop is not implemented.
    #[unsafe(super(NSButton))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriAppsTrash"]
    struct TrashButton;

    unsafe impl NSObjectProtocol for TrashButton {}

    impl TrashButton {
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.setContentTintColor(Some(&look::ink(Ink::Error)));
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.setContentTintColor(Some(&look::ink(Ink::Text)));
        }
    }
);

impl TrashButton {
    fn new(
        mtm: MainThreadMarker,
        name: &str,
        at: isize,
        window: &SettingsWindow,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:` is NSButton's designated initializer.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.setButtonType(NSButtonType::MomentaryPushIn);
        this.setBezelStyle(NSBezelStyle::Push);
        let words = NSString::from_str(&format!("Remove {name}"));
        let config = NSImageSymbolConfiguration::configurationWithPointSize_weight(12.0, 0.0);
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            ns_string!("trash"),
            Some(&words),
        )
        .and_then(|image| image.imageWithSymbolConfiguration(&config))
        {
            this.setImage(Some(&image));
        }
        this.setImagePosition(objc2_app_kit::NSCellImagePosition::ImageOnly);
        this.setContentTintColor(Some(&look::ink(Ink::Text)));
        this.setToolTip(Some(&words));
        this.setAccessibilityLabel(Some(&words));
        this.setTag(at);
        // SAFETY: the target is a weak reference to the window, which lives for the whole
        // process; the selector is its own action.
        unsafe {
            this.setTarget(Some(window.as_ref()));
            this.setAction(Some(sel!(appsRemove:)));
        }
        width_constraint(&this, TRASH_WIDTH);
        this.heightAnchor()
            .constraintEqualToConstant(BUTTON_HEIGHT)
            .setActive(true);
        let options = NSTrackingAreaOptions::MouseEnteredAndExited
            | NSTrackingAreaOptions::ActiveInKeyWindow
            | NSTrackingAreaOptions::InVisibleRect;
        // SAFETY: the owner is the button, which owns the area and outlives it; no user info.
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
}

// ─── the sheets' window ───────────────────────────────────────────────────────

define_class!(
    /// The window of a sheet: Esc is a Cancel.
    // SAFETY: NSWindow is designed for subclassing; Drop is not implemented.
    #[unsafe(super(NSWindow))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriAppsSheet"]
    struct SheetWindow;

    unsafe impl NSObjectProtocol for SheetWindow {}

    impl SheetWindow {
        #[unsafe(method(cancelOperation:))]
        fn cancel_operation(&self, _sender: Option<&AnyObject>) {
            if let Some(parent) = self.sheetParent() {
                parent.endSheet_returnCode(self, CANCEL);
            }
        }
    }
);

/// What a sheet's buttons answer: its tag.
const CANCEL: NSModalResponse = 0;
const CONFIRM: NSModalResponse = 1;

/// What a sheet asks.
#[derive(Clone)]
enum Ask {
    /// To follow a program, or to trust a new key for one: `add` when the person is adding it.
    Follow { review: Review, add: bool },
    /// To take a program off.
    Remove { source: String, name: String },
}

// ─── the page ─────────────────────────────────────────────────────────────────

/// What the page shows now.
#[derive(Default)]
struct Shown {
    rows: Vec<AppRow>,
    locked: bool,
    /// The lookup in progress: its generation, so an answer that comes late is not taken.
    looking: Option<u64>,
    generation: u64,
}

/// The Apps page: the list's stack, the Add field and what stands by it.
pub(super) struct Apps {
    list: Retained<NSStackView>,
    /// The end of the Following title: Check Now, or the ring while a check runs.
    head: Retained<NSStackView>,
    check: Retained<NSButton>,
    checking: Retained<NSStackView>,
    /// Whether this bateri can look for a newer one (Sparkle is in the bundle): the rows'
    /// Update bateri. `None` asks the app; the layout check says yes.
    updates: Cell<Option<bool>>,
    field: Retained<NSTextField>,
    add: Retained<NSButton>,
    ring: Retained<NSStackView>,
    note: Retained<NSTextField>,
    /// The note in a line of its own, in a little from the box's edge; hidden with it.
    note_line: Retained<NSStackView>,
    shown: RefCell<Shown>,
}

impl Apps {
    pub(super) fn new(window: &SettingsWindow) -> Apps {
        let mtm = window.mtm();
        let list = stack(
            mtm,
            &[],
            NSUserInterfaceLayoutOrientation::Vertical,
            0.0,
            NSLayoutAttribute::Leading,
        );
        let field = BoxedField::bare(mtm);
        field.setTextColor(Some(&look::ink(Ink::Text)));
        let font = field
            .font()
            .unwrap_or_else(|| NSFont::systemFontOfSize(NSFont::systemFontSize()));
        let hint = pictures::attributes(&font, &look::ink(Ink::Dim));
        // SAFETY: `hint` is a dictionary of attribute keys and values AppKit reads.
        let hint = unsafe {
            NSAttributedString::initWithString_attributes(
                NSAttributedString::alloc(),
                ns_string!("user/repo or the address of an app.json"),
                Some(&hint),
            )
        };
        field.setPlaceholderAttributedString(Some(&hint));
        field.setAccessibilityLabel(Some(ns_string!("A program to follow")));
        // SAFETY: the delegate is a weak reference to the window, which lives for the whole process.
        unsafe { field.setDelegate(Some(ProtocolObject::from_ref(window))) };
        // SAFETY: the target is a weak reference to the window, which lives for the whole
        // process; the selector is its own action.
        unsafe {
            field.setTarget(Some(window.as_ref()));
            field.setAction(Some(sel!(appsAdd:)));
        }
        // SAFETY: as above.
        let add = unsafe {
            NSButton::buttonWithTitle_target_action(
                ns_string!("Add"),
                Some(window.as_ref()),
                Some(sel!(appsAdd:)),
                mtm,
            )
        };
        width_constraint(&add, 52.0);
        ink_button(&add, false);
        let spinner = NSProgressIndicator::initWithFrame(
            NSProgressIndicator::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(SIGN, SIGN)),
        );
        spinner.setStyle(NSProgressIndicatorStyle::Spinning);
        spinner.setControlSize(NSControlSize::Small);
        spinner.setIndeterminate(true);
        width_constraint(&spinner, SIGN);
        let looking = text_label(ns_string!("Looking up"), mtm);
        looking.setTextColor(Some(&look::ink(Ink::Dim)));
        looking.setFont(Some(&NSFont::systemFontOfSize(12.0)));
        let ring = stack(
            mtm,
            &[&spinner, &looking],
            NSUserInterfaceLayoutOrientation::Horizontal,
            6.0,
            NSLayoutAttribute::CenterY,
        );
        ring.setHidden(true);
        // SAFETY: as above.
        let check = unsafe {
            NSButton::buttonWithTitle_target_action(
                ns_string!("Check Now"),
                Some(window.as_ref()),
                Some(sel!(appsCheckNow:)),
                mtm,
            )
        };
        check.setTranslatesAutoresizingMaskIntoConstraints(false);
        check
            .heightAnchor()
            .constraintEqualToConstant(22.0)
            .setActive(true);
        ink_button(&check, true);
        let wheel = NSProgressIndicator::initWithFrame(
            NSProgressIndicator::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(SIGN, SIGN)),
        );
        wheel.setStyle(NSProgressIndicatorStyle::Spinning);
        wheel.setControlSize(NSControlSize::Small);
        wheel.setIndeterminate(true);
        width_constraint(&wheel, SIGN);
        let words = text_label(ns_string!("Checking"), mtm);
        words.setTextColor(Some(&look::ink(Ink::Dim)));
        words.setFont(Some(&NSFont::systemFontOfSize(12.0)));
        let checking = stack(
            mtm,
            &[&wheel, &words],
            NSUserInterfaceLayoutOrientation::Horizontal,
            6.0,
            NSLayoutAttribute::CenterY,
        );
        checking.setHidden(true);
        let head = stack(
            mtm,
            &[&check, &checking],
            NSUserInterfaceLayoutOrientation::Horizontal,
            0.0,
            NSLayoutAttribute::CenterY,
        );
        head.setTranslatesAutoresizingMaskIntoConstraints(false);
        let note = wrapping_text_label(ns_string!(""), mtm);
        note.setTextColor(Some(&look::ink(Ink::Error)));
        note.setFont(Some(&NSFont::systemFontOfSize(11.5)));
        note.setHidden(true);
        let note_line = stack(
            mtm,
            &[&note],
            NSUserInterfaceLayoutOrientation::Horizontal,
            0.0,
            NSLayoutAttribute::Top,
        );
        note_line.setEdgeInsets(objc2_foundation::NSEdgeInsets {
            top: 0.0,
            left: 6.0,
            bottom: 0.0,
            right: 6.0,
        });
        note_line.setHidden(true);
        Apps {
            list,
            head,
            check,
            checking,
            updates: Cell::new(None),
            field,
            add,
            ring,
            note,
            note_line,
            shown: RefCell::new(Shown::default()),
        }
    }

    /// The end of the Following group's title line: Check Now, or the ring while a check runs.
    pub(super) fn head(&self) -> &NSStackView {
        &self.head
    }

    /// The row of the Following group: the list's own stack, as wide as the box.
    pub(super) fn followed_row(&self, mtm: MainThreadMarker) -> Row {
        bare_row(mtm, Retained::into_super(self.list.clone()))
    }

    /// The row of the Add group: the field in its box, and the button (or the ring) beside it.
    pub(super) fn add_row(&self, mtm: MainThreadMarker) -> Row {
        let view = NSView::new(mtm);
        let frame = theme_box_sized(mtm, &self.field, 8.0, BUTTON_HEIGHT, 6.0);
        let buttons = stack(
            mtm,
            &[&self.add, &self.ring],
            NSUserInterfaceLayoutOrientation::Horizontal,
            8.0,
            NSLayoutAttribute::CenterY,
        );
        add_pinned(&view, &frame);
        add_pinned(&view, &buttons);
        activate(&[
            view.heightAnchor()
                .constraintGreaterThanOrEqualToConstant(super::ROW_MIN_HEIGHT),
            frame
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&view.leadingAnchor(), super::ROW_PAD_X),
            frame
                .topAnchor()
                .constraintEqualToAnchor_constant(&view.topAnchor(), super::ROW_PAD_Y),
            frame
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&view.bottomAnchor(), -super::ROW_PAD_Y),
            buttons
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&view.trailingAnchor(), -super::ROW_PAD_X),
            buttons
                .centerYAnchor()
                .constraintEqualToAnchor(&frame.centerYAnchor()),
            frame
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&buttons.leadingAnchor(), -8.0),
        ]);
        bare_row(mtm, view)
    }

    /// The line under the Add box, where a refusal is said.
    pub(super) fn note_line(&self) -> &NSStackView {
        &self.note_line
    }

    /// [`Apps::show`] with the lock the last full refresh found: a check ended, nothing else did.
    pub(super) fn refresh(
        &self,
        window: &SettingsWindow,
        sources: &[String],
        tracked: Option<&Tracked>,
    ) {
        let locked = self.shown.borrow().locked;
        self.show(window, sources, tracked, locked);
    }

    /// Draws the page from the list, what the tracker says (`None`: no check could start) and
    /// the lock. A page that already shows it is left as it is (a hover, a spinning ring, a typed
    /// word stay).
    pub(super) fn show(
        &self,
        window: &SettingsWindow,
        sources: &[String],
        tracked: Option<&Tracked>,
        locked: bool,
    ) {
        let root = crate::apps_root().unwrap_or_default();
        let when = tracked.and_then(|tracked| tracked.checked).and_then(when);
        let checks = tracked.map(|tracked| tracked.checks(when.as_deref()));
        let rows = page::rows(sources, checks.as_ref(), &root);
        // Check Now does not write: a locked page has it too. An empty list has nothing to check.
        let running = tracked.is_some_and(|tracked| tracked.running);
        self.head.setHidden(sources.is_empty());
        self.check.setHidden(running);
        self.checking.setHidden(!running);
        if let Some(wheel) = self.checking.arrangedSubviews().iter().next()
            && let Some(wheel) = wheel.downcast_ref::<NSProgressIndicator>()
        {
            // SAFETY: plain calls on the spinner this page made.
            unsafe {
                if running {
                    wheel.startAnimation(None);
                } else {
                    wheel.stopAnimation(None);
                }
            }
        }
        let updates = self
            .updates
            .get()
            .unwrap_or_else(|| app::delegate(window.mtm()).is_some_and(|app| app.can_update()));
        {
            let shown = self.shown.borrow();
            if shown.rows == rows
                && shown.locked == locked
                && !self.list.arrangedSubviews().is_empty()
            {
                drop(shown);
                self.style_add();
                return;
            }
        }
        let mtm = window.mtm();
        hide_tip();
        for view in self.list.arrangedSubviews().iter() {
            self.list.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }
        if rows.is_empty() {
            let none = text_label(ns_string!("No programs yet."), mtm);
            none.setTextColor(Some(&look::ink(Ink::Dim)));
            let view = NSView::new(mtm);
            add_pinned(&view, &none);
            activate(&[
                none.leadingAnchor()
                    .constraintEqualToAnchor_constant(&view.leadingAnchor(), super::ROW_PAD_X),
                none.topAnchor()
                    .constraintEqualToAnchor_constant(&view.topAnchor(), 12.0),
                none.bottomAnchor()
                    .constraintEqualToAnchor_constant(&view.bottomAnchor(), -12.0),
            ]);
            self.list.addArrangedSubview(&view);
            view.widthAnchor()
                .constraintEqualToAnchor(&self.list.widthAnchor())
                .setActive(true);
        }
        for (at, row) in rows.iter().enumerate() {
            let view = app_row(mtm, window, row, at, locked, updates);
            self.list.addArrangedSubview(&view);
            view.widthAnchor()
                .constraintEqualToAnchor(&self.list.widthAnchor())
                .setActive(true);
        }
        {
            let mut shown = self.shown.borrow_mut();
            shown.rows = rows;
            shown.locked = locked;
        }
        self.style_add();
    }

    /// The window closed: the rows go, with the rings that turn in them (the next opening draws
    /// the page again), and a lookup under way is let go: its answer, when it comes, finds the
    /// page not waiting for it.
    pub(super) fn closed(&self) {
        hide_tip();
        for view in self.list.arrangedSubviews().iter() {
            self.list.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }
        {
            let mut shown = self.shown.borrow_mut();
            shown.rows.clear();
            if shown.looking.take().is_some() {
                shown.generation += 1;
            }
        }
        self.style_add();
    }

    /// The field, the button and the ring as the page's state has them.
    fn style_add(&self) {
        let shown = self.shown.borrow();
        let typed = !self.field.stringValue().to_string().trim().is_empty();
        let looking = shown.looking.is_some();
        self.field.setEnabled(!shown.locked && !looking);
        let ready = typed && !shown.locked && !looking;
        self.add.setEnabled(ready);
        self.add.setHidden(looking);
        self.ring.setHidden(!looking);
        if let Some(spinner) = self.ring.arrangedSubviews().iter().next()
            && let Some(spinner) = spinner.downcast_ref::<NSProgressIndicator>()
        {
            if looking {
                // SAFETY: a plain call on the spinner this page made.
                unsafe { spinner.startAnimation(None) };
            } else {
                // SAFETY: as above.
                unsafe { spinner.stopAnimation(None) };
            }
        }
        // The default button, in the theme's accent, only when Enter would do something.
        self.add.setKeyEquivalent(if ready {
            ns_string!("\r")
        } else {
            ns_string!("")
        });
        self.add
            .setBezelColor(ready.then(|| look::ink(Ink::Accent)).as_deref());
        let (ink, strength) = if ready {
            (Ink::OnAccent, 1.0)
        } else {
            (Ink::Dim, 0.55)
        };
        let font = self
            .add
            .font()
            .unwrap_or_else(|| NSFont::systemFontOfSize(NSFont::systemFontSize()));
        self.add
            .setAttributedTitle(&titled("Add", None, &font, &look::fixed(ink, strength)));
    }

    /// Says why the last Add did not go on, under the box; `None` clears the line.
    fn say(&self, text: Option<&str>) {
        match text {
            Some(text) => {
                self.note.setStringValue(&NSString::from_str(text));
                self.note.setHidden(false);
                self.note_line.setHidden(false);
                self.note
                    .setAccessibilityLabel(Some(&NSString::from_str(text)));
            }
            None => {
                self.note.setStringValue(ns_string!(""));
                self.note.setHidden(true);
                self.note_line.setHidden(true);
            }
        }
    }

    /// The field changed: a refusal said before no longer applies, and Add is ready or not.
    pub(super) fn typed(&self) {
        self.say(None);
        self.style_add();
    }

    /// Add (the button or Enter in the field): the source typed is looked at, off the main
    /// thread, and the answer comes back to [`Apps::looked`].
    pub(super) fn add_clicked(&self, window: &SettingsWindow) {
        let mtm = window.mtm();
        {
            let shown = self.shown.borrow();
            if shown.locked || shown.looking.is_some() {
                return;
            }
        }
        let typed = self.field.stringValue().to_string();
        let source = typed.trim().to_owned();
        if source.is_empty() {
            return;
        }
        let Some(delegate) = app::delegate(mtm) else {
            return;
        };
        let list = delegate.file_settings().apps_sources;
        if let Err(refused) = page::added(&list, &source) {
            self.say(Some(refused.text()));
            self.focus(window);
            return;
        }
        let Some(config) = crate::apps_root().and_then(Config::for_this_machine) else {
            self.say(Some("bateri cannot run programs on this computer."));
            return;
        };
        self.say(None);
        let generation = {
            let mut shown = self.shown.borrow_mut();
            shown.generation += 1;
            shown.looking = Some(shown.generation);
            shown.generation
        };
        self.style_add();
        let spawned = thread::Builder::new()
            .name("bateri-apps-look".into())
            .spawn(move || {
                let found = bt_shell_common::apps::look(&config, &source, &Curl);
                DispatchQueue::main().exec_async(move || {
                    // audit: a block running on the main queue is on the main thread by definition.
                    let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                    if let Some(window) = app::delegate(mtm).and_then(|app| app.settings_window()) {
                        window.apps_looked(generation, &source, found);
                    }
                });
            });
        if spawned.is_err() {
            self.shown.borrow_mut().looking = None;
            self.say(Some("bateri could not look at it."));
            self.style_add();
        }
    }

    /// The lookup of `source` ended. A program the person already trusts is followed; any other
    /// asks first; what could not be read is said under the field.
    pub(super) fn looked(
        &self,
        window: &SettingsWindow,
        generation: u64,
        source: &str,
        found: Result<Look, Failure>,
    ) {
        {
            let mut shown = self.shown.borrow_mut();
            if shown.looking != Some(generation) {
                return;
            }
            shown.looking = None;
        }
        self.style_add();
        // A name a source of the list holds already, landed or still waiting for a yes, is not
        // taken by a second: the first would stand red the moment the second was approved.
        let statuses = app::delegate(window.mtm())
            .and_then(|app| app.apps_tracked())
            .map(|tracked| tracked.statuses)
            .unwrap_or_default();
        let found = found.and_then(|look| {
            if page::name_claimed(&statuses, &look.name, source) {
                Err(Failure::NameTaken)
            } else {
                Ok(look)
            }
        });
        match found {
            Err(failure) => {
                self.say(Some(&page::refusal(source, &failure)));
                self.focus(window);
            }
            Ok(look) if look.trusted => self.write_added(window, source),
            Ok(look) => {
                let review = Review::from_look(source, &look);
                self.ask(window, Ask::Follow { review, add: true });
            }
        }
    }

    /// The list with `source` at its end is written, and the field is emptied.
    fn write_added(&self, window: &SettingsWindow, source: &str) {
        let Some(delegate) = app::delegate(window.mtm()) else {
            return;
        };
        let list = delegate.file_settings().apps_sources;
        match page::added(&list, source) {
            Ok(list) => {
                // The field is emptied once the list is written. A list that could not be
                // written is said by the notice every failed write posts, and what was typed
                // stays to be tried again.
                if delegate
                    .save_edits(&[SettingsEdit::AppsSources(list)])
                    .is_ok()
                {
                    self.field.setStringValue(ns_string!(""));
                    self.say(None);
                }
            }
            Err(refused) => {
                self.say(Some(refused.text()));
                self.focus(window);
            }
        }
        self.style_add();
    }

    fn focus(&self, window: &SettingsWindow) {
        if let Some(parent) = window.ivars().window.get() {
            parent.makeFirstResponder(Some(&self.field));
        }
    }

    /// Review on row `at`: the sheet that says who signed the release.
    pub(super) fn review(&self, window: &SettingsWindow, at: isize) {
        let review = usize::try_from(at).ok().and_then(|at| {
            self.shown
                .borrow()
                .rows
                .get(at)
                .and_then(|row| row.review.clone())
        });
        if let Some(review) = review {
            self.ask(window, Ask::Follow { review, add: false });
        }
    }

    /// The trash can of row `at`: the question first.
    pub(super) fn remove(&self, window: &SettingsWindow, at: isize) {
        let row = usize::try_from(at)
            .ok()
            .and_then(|at| self.shown.borrow().rows.get(at).cloned());
        if let Some(row) = row {
            self.ask(
                window,
                Ask::Remove {
                    source: row.source,
                    name: row.name,
                },
            );
        }
    }

    /// Opens the sheet for `ask` on the settings window; its answer is acted on here. One sheet
    /// at a time.
    fn ask(&self, window: &SettingsWindow, ask: Ask) {
        let Some(parent) = window.ivars().window.get() else {
            return;
        };
        if parent.attachedSheet().is_some() {
            return;
        }
        let mtm = window.mtm();
        let sheet = sheet_window(mtm, window, &ask);
        sheet.setAppearance(parent.appearance().as_deref());
        let window = window.retain();
        let answered = RcBlock::new(move |response: NSModalResponse| {
            let Some(apps) = window.ivars().apps.get() else {
                return;
            };
            apps.answered(&window, &ask, response == CONFIRM);
        });
        parent.beginSheet_completionHandler(&sheet, Some(&answered));
    }

    /// The sheet ended: `yes` when the person confirmed.
    fn answered(&self, window: &SettingsWindow, ask: &Ask, yes: bool) {
        match (ask, yes) {
            (Ask::Follow { add: true, .. }, false) => {
                // Nothing was written; the field keeps what was typed.
                self.focus(window);
            }
            (Ask::Follow { review, add }, true) => {
                let Some(root) = crate::apps_root() else {
                    return;
                };
                if let Err(e) = page::approve(&root, review) {
                    self.say(Some(&format!("The key could not be kept: {e}.")));
                    return;
                }
                if *add {
                    self.write_added(window, &review.source);
                } else if let Some(delegate) = app::delegate(window.mtm()) {
                    delegate.apps_check_now();
                }
            }
            (Ask::Remove { source, name, .. }, true) => {
                let Some(delegate) = app::delegate(window.mtm()) else {
                    return;
                };
                let list = delegate.file_settings().apps_sources;
                // Said only when it is so: a list that could not be written still has it.
                if delegate
                    .save_edits(&[SettingsEdit::AppsSources(page::without(&list, source))])
                    .is_ok()
                {
                    self.toast(window, &format!("{name} removed."));
                    // The program is kablo's on this computer: it is asked to take it off (a
                    // server's copy is not touched: it is that machine's).
                    let name = name.to_string();
                    let spawned = std::thread::Builder::new()
                        .name("bateri programs".into())
                        .spawn(move || {
                            if let Some(backend) = crate::hold_service::backend() {
                                let _ = bt_shell_common::apps::followed::remove(&backend, &name);
                            }
                        });
                    if let Err(error) = spawned {
                        eprintln!("bateri: no thread to take a program off this computer: {error}");
                    }
                }
            }
            _ => {}
        }
    }

    /// A line over the window's foot for a moment: what was done.
    fn toast(&self, window: &SettingsWindow, text: &str) {
        let mtm = window.mtm();
        let Some(content) = window
            .ivars()
            .window
            .get()
            .and_then(|parent| parent.contentView())
        else {
            return;
        };
        let label = text_label(&NSString::from_str(text), mtm);
        label.setTextColor(Some(&look::ink(Ink::Card)));
        label.setFont(Some(&NSFont::systemFontOfSize(12.5)));
        let pill = NSBox::new(mtm);
        pill.setBoxType(NSBoxType::Custom);
        pill.setTitlePosition(NSTitlePosition::NoTitle);
        pill.setBorderWidth(0.0);
        pill.setCornerRadius(9.0);
        pill.setFillColor(&look::ink(Ink::Text));
        fill(&pill, &label, 13.0, 7.0);
        pill.setAccessibilityLabel(Some(&NSString::from_str(text)));
        pill.setTranslatesAutoresizingMaskIntoConstraints(false);
        content.addSubview(&pill);
        activate(&[
            pill.centerXAnchor()
                .constraintEqualToAnchor(&content.centerXAnchor()),
            pill.bottomAnchor()
                .constraintEqualToAnchor_constant(&content.bottomAnchor(), -18.0),
        ]);
        // SAFETY: the selector is NSView's own, takes no argument, and the view stays alive until
        // the run loop has sent it.
        unsafe {
            pill.performSelector_withObject_afterDelay(
                sel!(removeFromSuperview),
                None,
                TOAST_SECONDS,
            );
        }
    }

    /// What a sheet's button answers when it is clicked: ends the sheet with the button's tag.
    pub(super) fn sheet_done(sender: Option<&AnyObject>) {
        let Some(button) = sender.and_then(|sender| sender.downcast_ref::<NSButton>()) else {
            return;
        };
        if let Some(sheet) = button.window()
            && let Some(parent) = sheet.sheetParent()
        {
            parent.endSheet_returnCode(&sheet, button.tag());
        }
    }

    /// The tag of a control: the row a button belongs to.
    pub(super) fn tag_of(sender: Option<&AnyObject>) -> Option<isize> {
        sender
            .and_then(|sender| sender.downcast_ref::<NSControl>())
            .map(NSControl::tag)
    }
}

/// A row of a group's box that is not a setting: `view` is the whole of it.
fn bare_row(mtm: MainThreadMarker, view: Retained<NSView>) -> Row {
    // The row's own height is the view's: this constraint is never active, so it asks for none.
    let min_height = view
        .heightAnchor()
        .constraintGreaterThanOrEqualToConstant(0.0);
    Row {
        key: None,
        view,
        separator: surface(mtm, Ink::Line),
        label: text_label(ns_string!(""), mtm),
        controls: Vec::new(),
        dim: Vec::new(),
        note: text_label(ns_string!(""), mtm),
        hidden: Cell::new(false),
        joined: false,
        min_height,
    }
}

/// `content` in `boxed`, in from its four edges by `x` and `y`.
fn fill(boxed: &NSBox, content: &NSView, x: f64, y: f64) {
    boxed.setContentView(Some(content));
    boxed.setContentViewMargins(NSSize::new(0.0, 0.0));
    content.setTranslatesAutoresizingMaskIntoConstraints(false);
    activate(&[
        content
            .topAnchor()
            .constraintEqualToAnchor_constant(&boxed.topAnchor(), y),
        content
            .bottomAnchor()
            .constraintEqualToAnchor_constant(&boxed.bottomAnchor(), -y),
        content
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&boxed.leadingAnchor(), x),
        content
            .trailingAnchor()
            .constraintEqualToAnchor_constant(&boxed.trailingAnchor(), -x),
    ]);
}

/// A button's title in `color`, with the name of its key after it, dimmer and smaller:
/// `Cancel Esc`.
fn titled(
    title: &str,
    key: Option<&str>,
    font: &NSFont,
    color: &objc2_app_kit::NSColor,
) -> Retained<NSAttributedString> {
    let main = NSMutableAttributedString::from_attributed_nsstring(&plain(title, font, color));
    if let Some(key) = key {
        let small = NSFont::systemFontOfSize(11.5);
        let attributes = pictures::attributes(&small, &faded_like(color, 0.55));
        // SAFETY: `attributes` is a dictionary of attribute keys and values AppKit reads.
        let hint = unsafe {
            NSAttributedString::initWithString_attributes(
                NSAttributedString::alloc(),
                &NSString::from_str(&format!("  {key}")),
                Some(&attributes),
            )
        };
        main.appendAttributedString(&hint);
    }
    Retained::into_super(main)
}

fn plain(
    title: &str,
    font: &NSFont,
    color: &objc2_app_kit::NSColor,
) -> Retained<NSAttributedString> {
    let attributes = pictures::attributes(font, color);
    // SAFETY: `attributes` is a dictionary of attribute keys and values AppKit reads.
    unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(title),
            Some(&attributes),
        )
    }
}

/// `color` at `alpha` of its own.
fn faded_like(color: &objc2_app_kit::NSColor, alpha: f64) -> Retained<objc2_app_kit::NSColor> {
    color.colorWithAlphaComponent(alpha)
}

// ─── a row ────────────────────────────────────────────────────────────────────

/// One row of the Following box: the name, the version and the source on the left; the sign,
/// Review (when a decision is asked for) and the trash can on the right.
fn app_row(
    mtm: MainThreadMarker,
    window: &SettingsWindow,
    row: &AppRow,
    at: usize,
    locked: bool,
    updates: bool,
) -> Retained<NSView> {
    let tag = isize::try_from(at).unwrap_or_default();
    let view = NSView::new(mtm);
    let name = FadeText::new(
        mtm,
        &row.name,
        NSFont::systemFontOfSize_weight(13.0, 0.23),
        Ink::Text,
    );
    let source = FadeText::new(mtm, &row.source, NSFont::systemFontOfSize(11.5), Ink::Dim);
    let sign = SignView::new(mtm, row);
    let trash = TrashButton::new(mtm, &row.name, tag, window);
    trash.setEnabled(!locked);
    // The row's one button, left of its sign: Review (which writes, so not on a locked page),
    // Try Again, or Update bateri where this bateri can look for a newer one.
    let action = match (&row.review, row.button) {
        (Some(_), _) => Some(("Review", sel!(appsReview:), !locked)),
        (None, Some(Button::TryAgain)) => Some(("Try Again", sel!(appsCheckNow:), true)),
        (None, Some(Button::UpdateBateri)) if updates => {
            Some(("Update bateri", sel!(appsUpdateBateri:), true))
        }
        _ => None,
    };
    let button = action.map(|(title, action, enabled)| {
        // SAFETY: the target is a weak reference to the window, which lives for the whole
        // process; the selector is its own action.
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(title),
                Some(window.as_ref()),
                Some(action),
                mtm,
            )
        };
        button.setTag(tag);
        button.setTranslatesAutoresizingMaskIntoConstraints(false);
        button
            .heightAnchor()
            .constraintEqualToConstant(BUTTON_HEIGHT)
            .setActive(true);
        ink_button(&button, enabled);
        button.setEnabled(enabled);
        button
    });
    add_pinned(&view, &name);
    add_pinned(&view, &source);
    add_pinned(&view, &sign);
    add_pinned(&view, &trash);
    let separator = surface(mtm, Ink::Line);
    add_pinned(&view, &separator);
    let mut constraints: Vec<Retained<NSLayoutConstraint>> = vec![
        view.heightAnchor()
            .constraintGreaterThanOrEqualToConstant(super::ROW_MIN_HEIGHT),
        separator
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&view.leadingAnchor(), super::ROW_PAD_X),
        separator
            .trailingAnchor()
            .constraintEqualToAnchor_constant(&view.trailingAnchor(), -super::ROW_PAD_X),
        separator
            .topAnchor()
            .constraintEqualToAnchor(&view.topAnchor()),
        separator.heightAnchor().constraintEqualToConstant(1.0),
        trash
            .trailingAnchor()
            .constraintEqualToAnchor_constant(&view.trailingAnchor(), -super::ROW_PAD_X),
        trash
            .centerYAnchor()
            .constraintEqualToAnchor(&view.centerYAnchor()),
        sign.centerYAnchor()
            .constraintEqualToAnchor(&view.centerYAnchor()),
        name.leadingAnchor()
            .constraintEqualToAnchor_constant(&view.leadingAnchor(), super::ROW_PAD_X),
        name.topAnchor()
            .constraintEqualToAnchor_constant(&view.topAnchor(), super::ROW_PAD_Y),
        source
            .leadingAnchor()
            .constraintEqualToAnchor(&name.leadingAnchor()),
        source
            .topAnchor()
            .constraintEqualToAnchor_constant(&name.bottomAnchor(), 2.0),
        source
            .bottomAnchor()
            .constraintEqualToAnchor_constant(&view.bottomAnchor(), -super::ROW_PAD_Y),
    ];
    // The right side, from the trash can inward: the sign by the trash can, so every row's sign
    // stands in one column, and the button left of it; the text ends before the leftmost.
    constraints.push(
        sign.trailingAnchor()
            .constraintEqualToAnchor_constant(&trash.leadingAnchor(), -8.0),
    );
    let mut leftmost = sign.leadingAnchor();
    if let Some(button) = &button {
        add_pinned(&view, button);
        constraints.push(
            button
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&leftmost, -8.0),
        );
        constraints.push(
            button
                .centerYAnchor()
                .constraintEqualToAnchor(&view.centerYAnchor()),
        );
        leftmost = button.leadingAnchor();
    }
    let reach = |anchor: &objc2_app_kit::NSLayoutXAxisAnchor| {
        anchor.constraintLessThanOrEqualToAnchor_constant(&leftmost, -super::ROW_GAP)
    };
    constraints.push(reach(&source.trailingAnchor()));
    match &row.version {
        Some(version) => {
            let ver = text_label(&NSString::from_str(version), mtm);
            ver.setTextColor(Some(&look::ink(Ink::Dim)));
            ver.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
                12.0, 0.0,
            )));
            ver.setLineBreakMode(NSLineBreakMode::ByClipping);
            ver.setContentCompressionResistancePriority_forOrientation(
                750.0,
                objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
            );
            add_pinned(&view, &ver);
            constraints.push(
                ver.leadingAnchor()
                    .constraintEqualToAnchor_constant(&name.trailingAnchor(), 8.0),
            );
            constraints.push(
                ver.centerYAnchor()
                    .constraintEqualToAnchor(&name.centerYAnchor()),
            );
            constraints.push(reach(&ver.trailingAnchor()));
        }
        None => constraints.push(reach(&name.trailingAnchor())),
    }
    activate(&constraints);
    // The first row has no hairline over it.
    separator.setHidden(at == 0);
    view
}

// ─── the sheets ───────────────────────────────────────────────────────────────

/// The window of the sheet that asks `ask`, with its buttons: Cancel answers 0, the other 1.
fn sheet_window(
    mtm: MainThreadMarker,
    window: &SettingsWindow,
    ask: &Ask,
) -> Retained<SheetWindow> {
    let content = sheet_content(mtm, window, ask);
    content.layoutSubtreeIfNeeded();
    let size = content.fittingSize();
    let this = SheetWindow::alloc(mtm).set_ivars(());
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), size);
    // SAFETY: `initWithContentRect:styleMask:backing:defer:` is NSWindow's designated
    // initializer; with defer false the window is made at once.
    let sheet: Retained<SheetWindow> = unsafe {
        msg_send![
            super(this),
            initWithContentRect: rect,
            styleMask: NSWindowStyleMask::Titled,
            backing: NSBackingStoreType::Buffered,
            defer: false
        ]
    };
    // SAFETY: only changes the ownership semantics; the Retained is the owner.
    unsafe { sheet.setReleasedWhenClosed(false) };
    sheet.setBackgroundColor(Some(&look::ink(Ink::Card)));
    sheet.setContentView(Some(&content));
    sheet.setContentSize(size);
    sheet
}

/// A button of a sheet: `title` with the name of its key after it. `default` is the one Enter
/// presses, in the accent; `risky` is red, for what cannot be taken back.
fn sheet_button(
    mtm: MainThreadMarker,
    window: &SettingsWindow,
    title: &str,
    key: Option<&str>,
    answer: NSModalResponse,
    default: bool,
    risky: bool,
) -> Retained<NSButton> {
    // SAFETY: the target is a weak reference to the window, which lives for the whole process;
    // the selector is its own action.
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(window.as_ref()),
            Some(sel!(appsSheetDone:)),
            mtm,
        )
    };
    button.setTag(answer);
    button.setTranslatesAutoresizingMaskIntoConstraints(false);
    button
        .heightAnchor()
        .constraintEqualToConstant(26.0)
        .setActive(true);
    let font = NSFont::systemFontOfSize(12.5);
    let ink = if default {
        Ink::OnAccent
    } else if risky {
        Ink::Error
    } else {
        Ink::Text
    };
    button.setAttributedTitle(&titled(title, key, &font, &look::fixed(ink, 1.0)));
    if default {
        button.setKeyEquivalent(ns_string!("\r"));
        button.setBezelColor(Some(&look::ink(Ink::Accent)));
    }
    button.setAccessibilityLabel(Some(&NSString::from_str(&match key {
        Some(key) => format!("{title}, {key}"),
        None => title.to_owned(),
    })));
    button
}

/// A wrapping label as wide as a sheet's text; `chars` breaks a line wherever it must, for an
/// address or a key that has no space to break at.
fn sheet_text(text: &str, size: f64, ink: Ink, width: f64, chars: bool) -> Retained<NSTextField> {
    let mtm = MainThreadMarker::new().expect("the settings window is on the main thread");
    let label = wrapping_text_label(&NSString::from_str(text), mtm);
    label.setFont(Some(&NSFont::systemFontOfSize(size)));
    label.setTextColor(Some(&look::ink(ink)));
    label.setPreferredMaxLayoutWidth(width);
    if chars {
        label.setLineBreakMode(NSLineBreakMode::ByCharWrapping);
    }
    label
}

/// The key as the sheet shows it: monospaced, in a box a step over the card, selectable so it can
/// be compared; `risky` frames it in red.
fn key_box(mtm: MainThreadMarker, key: &str, risky: bool) -> Retained<NSBox> {
    let label = sheet_text(
        key,
        11.5,
        Ink::Text,
        SHEET_TEXT - KEY_LABEL - 12.0 - 16.0,
        true,
    );
    label.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(11.5, 0.0)));
    label.setSelectable(true);
    let boxed = NSBox::new(mtm);
    boxed.setBoxType(NSBoxType::Custom);
    boxed.setTitlePosition(NSTitlePosition::NoTitle);
    boxed.setCornerRadius(7.0);
    boxed.setFillColor(&look::ink(Ink::Field));
    if risky {
        boxed.setBorderWidth(1.0);
        boxed.setBorderColor(&look::faded(Ink::Error, 0.55));
    } else {
        boxed.setBorderWidth(0.0);
    }
    fill(&boxed, &label, 8.0, 5.0);
    boxed
}

/// A line of the list that says who signed: its name, and the key beside it.
fn key_line(mtm: MainThreadMarker, name: &str, key: &str, risky: bool) -> Retained<NSStackView> {
    let label = text_label(&NSString::from_str(name), mtm);
    label.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    label.setTextColor(Some(&look::ink(Ink::Dim)));
    width_constraint(&label, KEY_LABEL);
    let boxed = key_box(mtm, key, risky);
    stack(
        mtm,
        &[&label, &boxed],
        NSUserInterfaceLayoutOrientation::Horizontal,
        12.0,
        NSLayoutAttribute::FirstBaseline,
    )
}

/// A box of a warning or of a refusal (`risky`): the card's tint of the theme's warning or error,
/// a sentence in bold and what follows it.
fn warn_box(mtm: MainThreadMarker, bold: &str, rest: &str, risky: bool) -> Retained<NSBox> {
    let label = sheet_text("", 12.5, Ink::Text, SHEET_TEXT - 24.0, false);
    let strong = NSFont::boldSystemFontOfSize(12.5);
    let regular = NSFont::systemFontOfSize(12.5);
    let color = look::ink(Ink::Text);
    let text = NSMutableAttributedString::from_attributed_nsstring(&plain(bold, &strong, &color));
    if !rest.is_empty() {
        text.appendAttributedString(&plain(&format!(" {rest}"), &regular, &color));
    }
    label.setAttributedStringValue(&text);
    let boxed = NSBox::new(mtm);
    boxed.setBoxType(NSBoxType::Custom);
    boxed.setTitlePosition(NSTitlePosition::NoTitle);
    boxed.setBorderWidth(0.0);
    boxed.setCornerRadius(10.0);
    boxed.setFillColor(&look::faded(
        if risky { Ink::Error } else { Ink::Warning },
        if risky { 0.14 } else { 0.16 },
    ));
    fill(&boxed, &label, 12.0, 10.0);
    boxed
}

/// What stands in a sheet, top to bottom, `SHEET_WIDTH` wide.
fn sheet_content(mtm: MainThreadMarker, window: &SettingsWindow, ask: &Ask) -> Retained<NSView> {
    let column = stack(
        mtm,
        &[],
        NSUserInterfaceLayoutOrientation::Vertical,
        0.0,
        NSLayoutAttribute::Leading,
    );
    column.setEdgeInsets(objc2_foundation::NSEdgeInsets {
        top: SHEET_PAD,
        left: SHEET_PAD,
        bottom: 18.0,
        right: SHEET_PAD,
    });
    let add = |view: &NSView, before: f64| {
        column.addArrangedSubview(view);
        if before > 0.0 {
            column.setCustomSpacing_afterView(before, view);
        }
    };
    let title = |name: &str, version: Option<&str>| {
        let line = stack(
            mtm,
            &[],
            NSUserInterfaceLayoutOrientation::Horizontal,
            8.0,
            NSLayoutAttribute::FirstBaseline,
        );
        let name = sheet_text(name, 15.0, Ink::Text, SHEET_TEXT, true);
        name.setFont(Some(&NSFont::boldSystemFontOfSize(15.0)));
        if let Some(version) = version {
            name.setPreferredMaxLayoutWidth(SHEET_TEXT - 60.0);
            let version = text_label(&NSString::from_str(version), mtm);
            version.setTextColor(Some(&look::ink(Ink::Dim)));
            version.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
                13.0, 0.0,
            )));
            line.addArrangedSubview(&name);
            line.addArrangedSubview(&version);
        } else {
            line.addArrangedSubview(&name);
        }
        line
    };
    let buttons = |cancel: Retained<NSButton>, confirm: Retained<NSButton>| {
        let row = stack(
            mtm,
            &[&cancel, &confirm],
            NSUserInterfaceLayoutOrientation::Horizontal,
            8.0,
            NSLayoutAttribute::CenterY,
        );
        let wrapper = NSView::new(mtm);
        add_pinned(&wrapper, &row);
        activate(&[
            row.trailingAnchor()
                .constraintEqualToAnchor(&wrapper.trailingAnchor()),
            row.topAnchor()
                .constraintEqualToAnchor(&wrapper.topAnchor()),
            row.bottomAnchor()
                .constraintEqualToAnchor(&wrapper.bottomAnchor()),
            row.leadingAnchor()
                .constraintGreaterThanOrEqualToAnchor(&wrapper.leadingAnchor()),
            wrapper.widthAnchor().constraintEqualToConstant(SHEET_TEXT),
        ]);
        wrapper
    };
    match ask {
        Ask::Remove { source, name, .. } => {
            add(&title(&format!("Remove {name}?"), None), 2.0);
            add(
                &sheet_text(&format!("from {source}"), 12.0, Ink::Dim, SHEET_TEXT, true),
                10.0,
            );
            add(
                &sheet_text(
                    "It is deleted from this computer now. The servers keep their copies: take \
                     one off there with bateri package remove. Adding it again brings it back.",
                    11.5,
                    Ink::Dim,
                    SHEET_TEXT,
                    false,
                ),
                18.0,
            );
            add(
                &buttons(
                    sheet_button(mtm, window, "Cancel", Some("Esc"), CANCEL, true, false),
                    sheet_button(mtm, window, "Remove", None, CONFIRM, false, true),
                ),
                0.0,
            );
        }
        Ask::Follow { review, .. } => {
            let changed = review.was.is_some();
            add(&title(&review.name, Some(&review.version)), 2.0);
            add(
                &sheet_text(
                    &format!("from {}", review.source),
                    12.0,
                    Ink::Dim,
                    SHEET_TEXT,
                    true,
                ),
                16.0,
            );
            let keys = stack(
                mtm,
                &[],
                NSUserInterfaceLayoutOrientation::Vertical,
                9.0,
                NSLayoutAttribute::Leading,
            );
            match &review.was {
                Some(was) => {
                    keys.addArrangedSubview(&key_line(mtm, "Approved key", was, false));
                    keys.addArrangedSubview(&key_line(mtm, "Signed now with", &review.key, true));
                }
                None => keys.addArrangedSubview(&key_line(mtm, "Signed with", &review.key, false)),
            }
            add(&keys, 16.0);
            let warning = if changed {
                warn_box(
                    mtm,
                    "This release is not signed with the key you approved.",
                    "Updates stopped. It is the author\u{2019}s new key, or someone else\u{2019}s.",
                    true,
                )
            } else {
                warn_box(
                    mtm,
                    "It runs on this computer with your permissions. A server gets it only when \
                     you ask it to.",
                    "",
                    false,
                )
            };
            add(&warning, 10.0);
            add(
                &sheet_text(
                    if changed {
                        "Trust the new key only if the author told you about it. Until you \
                         decide, the release you have stays."
                    } else {
                        "Later releases are taken only when signed with this key. If it changes, \
                         updates stop and bateri asks again."
                    },
                    11.5,
                    Ink::Dim,
                    SHEET_TEXT,
                    false,
                ),
                18.0,
            );
            let (cancel, confirm) = if changed {
                (
                    sheet_button(mtm, window, "Cancel", Some("Esc"), CANCEL, true, false),
                    sheet_button(mtm, window, "Trust New Key", None, CONFIRM, false, true),
                )
            } else {
                (
                    sheet_button(mtm, window, "Cancel", Some("Esc"), CANCEL, false, false),
                    sheet_button(mtm, window, "Follow", Some("Enter"), CONFIRM, true, false),
                )
            };
            add(&buttons(cancel, confirm), 0.0);
        }
    }
    column.setTranslatesAutoresizingMaskIntoConstraints(false);
    let host = NSView::new(mtm);
    host.addSubview(&column);
    activate(&[
        column
            .topAnchor()
            .constraintEqualToAnchor(&host.topAnchor()),
        column
            .bottomAnchor()
            .constraintEqualToAnchor(&host.bottomAnchor()),
        column
            .leadingAnchor()
            .constraintEqualToAnchor(&host.leadingAnchor()),
        column
            .trailingAnchor()
            .constraintEqualToAnchor(&host.trailingAnchor()),
        host.widthAnchor().constraintEqualToConstant(SHEET_WIDTH),
    ]);
    host
}

// ─── what the layout check looks at ───────────────────────────────────────────

/// The sheets, one of each kind, on a card of the theme's: for the layout check to measure and
/// to picture, as they are built for the person.
pub(super) fn sample_sheets(
    mtm: MainThreadMarker,
    window: &SettingsWindow,
) -> Vec<(&'static str, Retained<NSView>)> {
    let key = "ed25519:q7Hc2yTn0uZk4Vd1XbRw8LpM3sJf6EaN9gYtOiKh5vU=";
    let other = "ed25519:Zk3mP9wLq1xD7sVb0nJ4tHc8RaE2yUf6OgIiKd5Yh+A=";
    let review = |name: &str, source: &str, was: Option<&str>| Review {
        source: source.to_owned(),
        name: name.to_owned(),
        version: "0.3.1".to_owned(),
        key: key.to_owned(),
        was: was.map(str::to_owned),
    };
    let long = "https://downloads.internal.example-company.org/releases/latest/lab/app.json";
    let asks = [
        (
            "follow",
            Ask::Follow {
                review: review("notes", "acme/notes", None),
                add: true,
            },
        ),
        (
            "follow-long",
            Ask::Follow {
                review: review("lab", long, None),
                add: false,
            },
        ),
        (
            "changed",
            Ask::Follow {
                review: review("draw", "kerem/draw", Some(other)),
                add: false,
            },
        ),
        (
            "remove",
            Ask::Remove {
                source: "acme/notes".to_owned(),
                name: "notes".to_owned(),
            },
        ),
    ];
    asks.iter()
        .map(|(name, ask)| {
            let content = sheet_content(mtm, window, ask);
            content.layoutSubtreeIfNeeded();
            let card = surface(mtm, Ink::Card);
            card.setContentView(Some(&content));
            card.setFrameSize(content.fittingSize());
            (*name, Retained::into_super(card))
        })
        .collect()
}

impl Apps {
    /// For the layout check: the Add field as it stands with `typed` in it, a refusal under it, or
    /// the lookup going on.
    pub(super) fn sample(&self, typed: &str, error: Option<&str>, looking: bool) {
        self.field.setStringValue(&NSString::from_str(typed));
        self.say(error);
        self.shown.borrow_mut().looking = looking.then_some(0);
        self.style_add();
    }

    /// For the layout check: whether rows offer Update bateri, whatever the app has.
    pub(super) fn sample_updates(&self, updates: bool) {
        self.updates.set(Some(updates));
    }

    /// For the layout check: the Following box's rows.
    pub(super) fn rows_view(&self) -> &NSStackView {
        &self.list
    }

    /// For the layout check: the tip of row `at`'s sign, as the pointer resting on it shows it;
    /// `false` when there is no such row.
    pub(super) fn sample_tip(&self, at: usize) -> bool {
        hide_tip();
        let rows = self.list.arrangedSubviews();
        let Some(sign) = rows.iter().nth(at).and_then(|row| {
            row.subviews()
                .iter()
                .find_map(|view| view.downcast::<SignView>().ok())
        }) else {
            return false;
        };
        let ivars = sign.ivars();
        show_tip(&sign, &ivars.title, ivars.more.as_deref());
        true
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    #[test]
    fn a_check_is_told_by_its_day_and_its_clock() {
        let now = super::when(SystemTime::now()).expect("now has words");
        assert!(now.starts_with("today at "), "{now}");
        let long_ago = SystemTime::now() - Duration::from_secs(10 * 24 * 60 * 60);
        let then = super::when(long_ago).expect("ten days ago has words");
        assert!(then.starts_with("on "), "{then}");
        assert!(then.contains(" at "), "{then}");
    }
}
