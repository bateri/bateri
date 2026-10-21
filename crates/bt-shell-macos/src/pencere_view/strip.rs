//! The strip the application keeps over a program's view: the AppKit half of
//! [`bt_shell_common::view_strip`].
//!
//! A native view of the application's own at the top of the pane, **outside the view's area**:
//! the web view is under it and never draws into it, so a view that imitates the terminal
//! cannot imitate this. It says where the program runs, which program it is and what the view
//! is, and has one button, **Quit ⌃C**, that writes ⌃C to the program
//! (`TerminalPane::quit_program`, the key's own path). The words, the cleaning of the names a
//! program gives and where the parts stand when the strip is narrow are `bt-shell-common`'s;
//! what is here is measuring, drawing and the colours — all from the theme, none of them
//! written down: the ground is the band tone the dock's folder row stands on and the hairline
//! under it the separator's, the place is the info role (the dock's `⇄ host`), the program the
//! foreground and the title the dim role, and the button a veil of the foreground over the
//! ground with its key in the dim role, the way a menu shows a shortcut.
//!
//! The names are of two sizes and sit on one baseline, the one that centres the program's
//! capitals on the strip: centring each field's box would set them at two heights.
//!
//! The strip takes no keyboard: the button refuses first responder, and a press anywhere on
//! the strip gives the keyboard to the view (`TerminalPane::focus`) as a press in the pane does.

use std::cell::{Cell, RefCell};

use bt_core::Theme;
use bt_shell_common::view_strip::{self, Strip, Widths};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBezierPath, NSBox, NSBoxType, NSColor, NSEvent, NSFont,
    NSFontAttributeName, NSFontWeightMedium, NSForegroundColorAttributeName, NSLineBreakMode,
    NSTextField, NSTitlePosition, NSView,
};
use objc2_foundation::{
    NSAttributedString, NSAttributedStringKey, NSDictionary, NSMutableAttributedString,
    NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};

use crate::pane::{JumpButton, TerminalPane};

/// The program's name's size, points: the one strong word on the strip.
const NAME_TEXT: f64 = 13.0;
/// The place's and the title's size, points: the canvas's 12.
const TEXT: f64 = 12.0;
/// The button's verb's size, points.
const BUTTON_TEXT: f64 = 11.5;
/// The button's key's size, points: a little under the verb, as a menu's shortcut is.
const KEY_TEXT: f64 = 11.0;
/// What stands between the verb and the key in the button: an en space, about 6 points at the
/// verb's size.
const KEY_GAP: &str = "\u{2002}";
/// The button's height, points.
const BUTTON_HEIGHT: f64 = 22.0;
/// The button's gap between its edge and its text, points.
const BUTTON_PAD: f64 = 9.0;
/// The button's corner, points.
const BUTTON_CORNER: f64 = 6.0;
/// How much of the foreground the button's fill and its edge are, over the strip's ground. A
/// design constant, tuned by eye on a dark and a light theme: the same veil reads as a raised
/// plate on both, which no fixed colour would.
const BUTTON_FILL: f64 = 0.10;
const BUTTON_EDGE: f64 = 0.22;

/// The colours the strip is drawn with, from the theme.
struct Palette {
    ground: Retained<NSColor>,
    hairline: Retained<NSColor>,
}

pub(super) struct StripIvars {
    palette: RefCell<Palette>,
    /// Where the program runs, its name and the view's title; a part the strip does not say
    /// is an empty field, never shown.
    place: Retained<NSTextField>,
    program: Retained<NSTextField>,
    title: Retained<NSTextField>,
    /// The button's box and the button that fills it ([`JumpButton`]: the pane's button with
    /// the hand cursor over all of it).
    surface: Retained<NSBox>,
    button: Retained<JumpButton>,
    /// The parts' natural widths, measured when the text or the colours changed.
    natural: Cell<Widths>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; the strip implements no `Drop` and is born
    // with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPencereStrip"]
    #[ivars = StripIvars]
    pub(super) struct StripView;

    unsafe impl NSObjectProtocol for StripView {}

    impl StripView {
        /// The ground and the hairline under it; the text and the button are subviews.
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let palette = self.ivars().palette.borrow();
            palette.ground.setFill();
            NSBezierPath::fillRect(bounds);
            palette.hairline.setFill();
            NSBezierPath::fillRect(NSRect::new(
                bounds.origin,
                NSSize::new(bounds.size.width, 1.0),
            ));
        }

        /// The parts are placed again for the new width.
        #[unsafe(method(resizeSubviewsWithOldSize:))]
        fn resize_subviews(&self, _old: NSSize) {
            self.arrange();
        }

        /// A press on the strip is a press in the pane: the keyboard goes to the view.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            // SAFETY: reading the superview on the main thread; the `Retained` keeps it alive.
            let parent = unsafe { self.superview() };
            if let Some(pane) = parent.and_then(|parent| parent.downcast::<TerminalPane>().ok()) {
                pane.focus();
            }
        }
    }
);

impl StripView {
    /// A strip saying `text`, painted with `theme`, of the width of `frame`.
    pub(super) fn new(
        mtm: MainThreadMarker,
        frame: NSRect,
        text: &Strip,
        theme: &Theme,
    ) -> Retained<Self> {
        let label = |string: Option<&String>, font: Retained<NSFont>| {
            let string = string.map_or("", String::as_str);
            let field = NSTextField::labelWithString(&NSString::from_str(string), mtm);
            field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            field.setFont(Some(&font));
            field
        };
        let place = label(text.place.as_ref(), NSFont::systemFontOfSize(TEXT));
        let program = label(text.program.as_ref(), name_font());
        let title = label(text.title.as_ref(), NSFont::systemFontOfSize(TEXT));

        let surface = NSBox::new(mtm);
        surface.setBoxType(NSBoxType::Custom);
        surface.setTitlePosition(NSTitlePosition::NoTitle);
        surface.setBorderWidth(1.0);
        surface.setCornerRadius(BUTTON_CORNER);
        // No margins: the button fills the box and carries the padding itself, so the whole
        // box takes the click and the hand cursor.
        surface.setContentViewMargins(NSSize::new(0.0, 0.0));
        // SAFETY: `NSButton`'s `init`; the subclass has no ivar.
        let button = JumpButton::alloc(mtm).set_ivars(());
        let button: Retained<JumpButton> = unsafe { msg_send![super(button), init] };
        button.setBordered(false);
        // The keyboard stays with the view.
        button.setRefusesFirstResponder(true);
        // What the button does, for whoever does not know ⌃C: it writes the key to the program.
        button.setToolTip(Some(&NSString::from_str(&text.quit_tip)));
        surface.setContentView(Some(&button));

        let this = Self::alloc(mtm).set_ivars(StripIvars {
            palette: RefCell::new(palette(theme)),
            place,
            program,
            title,
            surface,
            button,
            natural: Cell::new(Widths {
                place: 0.0,
                program: 0.0,
                title: 0.0,
                button: 0.0,
            }),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        // The strip stays at the top of the pane and as wide as it, whatever the pane's height.
        this.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        let ivars = this.ivars();
        this.addSubview(&ivars.place);
        this.addSubview(&ivars.program);
        this.addSubview(&ivars.title);
        this.addSubview(&ivars.surface);
        this.colour(theme);
        this.arrange();
        this
    }

    /// Points the button at the pane's `quitProgram:`.
    pub(super) fn aim(&self, pane: &TerminalPane) {
        let target: &AnyObject = pane.as_ref();
        // SAFETY: the selector is the pane's `quitProgram:`, which takes a single
        // `Option<&AnyObject>`; the pane holds the strip through its view host (the target is
        // weak).
        unsafe {
            self.ivars().button.setTarget(Some(target));
            self.ivars().button.setAction(Some(sel!(quitProgram:)));
        }
    }

    /// Paints the strip with `theme` again, as the pane's other surfaces are.
    pub(super) fn paint(&self, theme: &Theme) {
        self.ivars().palette.replace(palette(theme));
        self.colour(theme);
        self.arrange();
        self.setNeedsDisplay(true);
    }

    /// The text's colours, the button's fill and label, and the measures of them all.
    fn colour(&self, theme: &Theme) {
        let ivars = self.ivars();
        ivars.place.setTextColor(Some(&srgb(theme.info, 1.0)));
        ivars
            .program
            .setTextColor(Some(&srgb(theme.foreground, 1.0)));
        ivars.title.setTextColor(Some(&srgb(theme.dim, 1.0)));
        ivars
            .surface
            .setFillColor(&srgb(theme.foreground, BUTTON_FILL));
        ivars
            .surface
            .setBorderColor(&srgb(theme.foreground, BUTTON_EDGE));
        let label = NSMutableAttributedString::new();
        for (text, color, font) in [
            (
                format!("{}{KEY_GAP}", view_strip::QUIT),
                theme.foreground,
                // SAFETY: a constant AppKit exposes, it lives for the whole process.
                NSFont::systemFontOfSize_weight(BUTTON_TEXT, unsafe { NSFontWeightMedium }),
            ),
            (
                view_strip::QUIT_KEY.to_owned(),
                theme.dim,
                NSFont::systemFontOfSize(KEY_TEXT),
            ),
        ] {
            let color = srgb(color, 1.0);
            let values: [&AnyObject; 2] = [color.as_ref(), font.as_ref()];
            // SAFETY: AppKit's two attribute keys (extern statics), each with the value type it
            // documents — an `NSColor` and an `NSFont`.
            let piece = unsafe {
                let attributes = NSDictionary::<NSAttributedStringKey, AnyObject>::from_slices(
                    &[NSForegroundColorAttributeName, NSFontAttributeName],
                    &values,
                );
                NSAttributedString::new_with_attributes(&NSString::from_str(&text), &attributes)
            };
            label.appendAttributedString(&piece);
        }
        ivars.button.setAttributedTitle(&label);
        let fit = ivars.button.fittingSize();
        ivars.surface.setFrameFromContentFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(fit.width + 2.0 * BUTTON_PAD, BUTTON_HEIGHT),
        ));
        // A field with nothing to say takes no room.
        let width = |field: &NSTextField| {
            if field.stringValue().length() == 0 {
                0.0
            } else {
                field.fittingSize().width.ceil()
            }
        };
        ivars.natural.set(Widths {
            place: width(&ivars.place),
            program: width(&ivars.program),
            title: width(&ivars.title),
            button: ivars.surface.frame().size.width,
        });
    }

    /// Puts the parts where [`view_strip::arrange`] says for the strip's width: the names on
    /// one baseline that centres the program's capitals on the strip's height, the button
    /// centred on it.
    fn arrange(&self) {
        let ivars = self.ivars();
        let layout = view_strip::arrange(self.bounds().size.width, ivars.natural.get());
        let height = self.bounds().size.height - 1.0;
        let baseline = (1.0 + (height - name_font().capHeight()) / 2.0).round();
        let place = |field: &NSTextField, span: Option<view_strip::Span>| match span {
            Some(span) => {
                let line = field.fittingSize().height;
                field.setFrameSize(NSSize::new(span.width, line));
                let below = line - field.firstBaselineOffsetFromTop();
                field.setFrameOrigin(NSPoint::new(span.x, baseline - below));
                field.setHidden(false);
            }
            None => field.setHidden(true),
        };
        place(&ivars.place, layout.place);
        place(&ivars.program, layout.program);
        place(&ivars.title, layout.title);
        let button = ivars.surface.frame().size;
        ivars.surface.setFrameOrigin(NSPoint::new(
            layout.button.x,
            1.0 + ((height - button.height) / 2.0).max(0.0),
        ));
        if let Some(window) = self.window() {
            window.invalidateCursorRectsForView(&ivars.button);
        }
    }
}

/// The program's name's font: the strip's one strong word.
fn name_font() -> Retained<NSFont> {
    // SAFETY: a constant AppKit exposes, it lives for the whole process.
    NSFont::systemFontOfSize_weight(NAME_TEXT, unsafe { NSFontWeightMedium })
}

/// The strip's colours: the band tone and the separator's hairline.
fn palette(theme: &Theme) -> Palette {
    let bytes = |[r, g, b]: [u8; 3]| {
        NSColor::colorWithSRGBRed_green_blue_alpha(
            f64::from(r) / 255.0,
            f64::from(g) / 255.0,
            f64::from(b) / 255.0,
            1.0,
        )
    };
    Palette {
        ground: bytes(theme.band_srgb()),
        hairline: bytes(theme.separator_srgb()),
    }
}

/// An sRGB `NSColor` from a theme role (`0xRRGGBB`) at `alpha`.
fn srgb(color: u32, alpha: f64) -> Retained<NSColor> {
    let [_, r, g, b] = color.to_be_bytes();
    NSColor::colorWithSRGBRed_green_blue_alpha(
        f64::from(r) / 255.0,
        f64::from(g) / 255.0,
        f64::from(b) / 255.0,
        alpha,
    )
}
