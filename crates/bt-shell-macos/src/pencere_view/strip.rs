//! The strip the application keeps over a program's view: the AppKit half of
//! [`bt_shell_common::view_strip`].
//!
//! A native view of the application's own at the top of the pane, **outside the view's area**:
//! the web view is under it and never draws into it, so a view that imitates the terminal
//! cannot imitate this. It says that what is shown is a program's view, where the program runs
//! and how to leave; its button writes ⌃C to the program (`TerminalPane::quit_program`, the
//! key's own path). The words, the cleaning of the names a program gives and where the parts
//! stand when the strip is narrow are `bt-shell-common`'s; what is here is measuring, drawing
//! and the colours — all from the theme, none of them written down: the ground is the band
//! tone the dock's folder row stands on and the hairline under it the separator's, the title is
//! the foreground and the rest the dim role, the button a veil of the foreground over the
//! ground.
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
    NSAttributedString, NSAttributedStringKey, NSDictionary, NSObjectProtocol, NSPoint, NSRect,
    NSSize, NSString,
};

use crate::pane::{JumpButton, TerminalPane};

/// The text's size, points: the canvas's 12.
const TEXT: f64 = 12.0;
/// The button's text size, points.
const BUTTON_TEXT: f64 = 11.5;
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
    icon: Retained<NSColor>,
}

pub(super) struct StripIvars {
    palette: RefCell<Palette>,
    title: Retained<NSTextField>,
    context: Retained<NSTextField>,
    hint: Retained<NSTextField>,
    /// The button's box and the button that fills it ([`JumpButton`]: the pane's button with
    /// the hand cursor over all of it).
    surface: Retained<NSBox>,
    button: Retained<JumpButton>,
    /// The button's label, kept to say it again when the colours change.
    quit: RefCell<String>,
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
        /// The ground, the hairline under it and the icon; the text and the button are
        /// subviews.
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
            palette.icon.setStroke();
            Self::icon(bounds.size.height).stroke();
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
        let label = |string: &str| {
            let field = NSTextField::labelWithString(&NSString::from_str(string), mtm);
            field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            field
        };
        let title = label(&text.title);
        // SAFETY: a constant AppKit exposes, it lives for the whole process.
        title.setFont(Some(&NSFont::systemFontOfSize_weight(TEXT, unsafe {
            NSFontWeightMedium
        })));
        let context = label(&text.context);
        context.setFont(Some(&NSFont::systemFontOfSize(TEXT)));
        let hint = label(text.hint);
        hint.setFont(Some(&NSFont::systemFontOfSize(TEXT)));

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
        surface.setContentView(Some(&button));

        let this = Self::alloc(mtm).set_ivars(StripIvars {
            palette: RefCell::new(palette(theme)),
            title,
            context,
            hint,
            surface,
            button,
            quit: RefCell::new(text.quit.clone()),
            natural: Cell::new(Widths {
                title: 0.0,
                context: 0.0,
                hint: 0.0,
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
        this.addSubview(&ivars.title);
        this.addSubview(&ivars.context);
        this.addSubview(&ivars.hint);
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
        ivars.title.setTextColor(Some(&srgb(theme.foreground, 1.0)));
        for field in [&ivars.context, &ivars.hint] {
            field.setTextColor(Some(&srgb(theme.dim, 1.0)));
        }
        ivars
            .surface
            .setFillColor(&srgb(theme.foreground, BUTTON_FILL));
        ivars
            .surface
            .setBorderColor(&srgb(theme.foreground, BUTTON_EDGE));
        let font =
            // SAFETY: a constant AppKit exposes, it lives for the whole process.
            NSFont::systemFontOfSize_weight(BUTTON_TEXT, unsafe { NSFontWeightMedium });
        let color = srgb(theme.foreground, 1.0);
        let values: [&AnyObject; 2] = [color.as_ref(), font.as_ref()];
        // SAFETY: AppKit's two attribute keys (extern statics), each with the value type it
        // documents — an `NSColor` and an `NSFont`.
        let title = unsafe {
            let attributes = NSDictionary::<NSAttributedStringKey, AnyObject>::from_slices(
                &[NSForegroundColorAttributeName, NSFontAttributeName],
                &values,
            );
            NSAttributedString::new_with_attributes(
                &NSString::from_str(&ivars.quit.borrow()),
                &attributes,
            )
        };
        ivars.button.setAttributedTitle(&title);
        let fit = ivars.button.fittingSize();
        ivars.surface.setFrameFromContentFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(fit.width + 2.0 * BUTTON_PAD, BUTTON_HEIGHT),
        ));
        let width = |field: &NSTextField| field.fittingSize().width.ceil();
        ivars.natural.set(Widths {
            title: width(&ivars.title),
            context: width(&ivars.context),
            hint: width(&ivars.hint),
            button: ivars.surface.frame().size.width,
        });
    }

    /// Puts the parts where [`view_strip::arrange`] says for the strip's width, centred on
    /// its height.
    fn arrange(&self) {
        let ivars = self.ivars();
        let layout = view_strip::arrange(self.bounds().size.width, ivars.natural.get());
        let height = self.bounds().size.height - 1.0;
        let place = |field: &NSTextField, span: view_strip::Span| {
            let line = field.fittingSize().height;
            field.setFrame(NSRect::new(
                NSPoint::new(span.x, 1.0 + ((height - line) / 2.0).max(0.0)),
                NSSize::new(span.width, line),
            ));
        };
        place(&ivars.title, layout.title);
        place(&ivars.context, layout.context);
        match layout.hint {
            Some(span) => {
                place(&ivars.hint, span);
                ivars.hint.setHidden(false);
            }
            None => ivars.hint.setHidden(true),
        }
        let button = ivars.surface.frame().size;
        ivars.surface.setFrameOrigin(NSPoint::new(
            layout.button.x,
            1.0 + ((height - button.height) / 2.0).max(0.0),
        ));
        if let Some(window) = self.window() {
            window.invalidateCursorRectsForView(&ivars.button);
        }
    }

    /// The icon's outline: a window with its title row, in a square of [`view_strip::ICON`]
    /// at the strip's left, centred on its `height` (unflipped points).
    fn icon(height: f64) -> Retained<NSBezierPath> {
        let side = view_strip::ICON;
        let (x, y) = (view_strip::LEFT, (height - side) / 2.0);
        let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            NSRect::new(
                NSPoint::new(x + 1.5, y + 2.0),
                NSSize::new(side - 3.0, side - 4.0),
            ),
            2.0,
            2.0,
        );
        // The title row's line, 5 points under the square's top.
        path.moveToPoint(NSPoint::new(x + 1.5, y + side - 5.0));
        path.lineToPoint(NSPoint::new(x + side - 1.5, y + side - 5.0));
        path.setLineWidth(1.3);
        path
    }
}

/// The strip's colours: the band tone, the separator's hairline and the quiet text colour.
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
        icon: srgb(theme.dim, 1.0),
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
