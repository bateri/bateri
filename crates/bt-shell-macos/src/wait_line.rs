//! What a pane says while its terminal of this Mac has not come: a dim line of plain text in the
//! terminal's own font, led by three thin bars that rise and fall ("Starting the terminal",
//! "Bringing the terminal back"), and, when the wait is long, a link to an ordinary shell instead
//! ("Open an ordinary shell instead").
//!
//! **Nothing here is a control but the link.** The line and the bars take no click, no hover and
//! no cursor: a hit on them is the pane's ([`WaitRow`], [`WaitBars`]), so the line does not look
//! like a button and is not one. The link is a button of its own, beside the line and not inside
//! it, which refuses the keyboard (the terminal keeps it) — only a click on it does anything, and
//! nothing falls to an ordinary shell for any other reason (a timer, a key).
//!
//! **The bars are one character cell wide**, and a space and the words follow, as a terminal sets
//! a line; they stand on the baseline of the words and are no taller than its capitals. Each
//! rises and falls on its own, smoothly (slowing at the top and at the foot), a whole breath in a
//! second, and the three are a little behind one another, so that they ripple like a level meter.
//!
//! **No frame for the bars.** The motion is Core Animation's: a repeating animation of each
//! bar's height on three plain layers beside the terminal's Metal layer, drawn by the render
//! server at the display's rate with no timer on the main thread, so the renderer is asked for
//! nothing while they move. It stops with the line (the terminal has come), and while Reduce
//! Motion is on the bars do not move at all but stand at different heights. A layer's animation
//! does not outlive the view's window (a tab put in the background, a split, a tab moved from one
//! window to another), so it is put again where the view enters one — where the bars would be had
//! it never gone: every bar's animation begins at the same moment, the one the bars first moved
//! at, so an animation put again goes on in the same phase and the bars do not jump.
//!
//! **The line stays in its pane.** It is laid out again when the pane's size changes (a split, a
//! narrower window), the words shortened and the link moved under the line and shortened when the
//! pane is too narrow for them: nothing of it stands over the pane beside it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use block2::RcBlock;
use bt_core::FontOptions;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAppearanceCustomization, NSAutoresizingMaskOptions, NSColor, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSLineBreakMode, NSMutableParagraphStyle,
    NSParagraphStyleAttributeName, NSStringDrawing, NSTextField, NSView,
};
use objc2_core_graphics::CGColor;
use objc2_foundation::{
    NSAttributedString, NSAttributedStringKey, NSDictionary, NSNumber, NSObjectProtocol, NSPoint,
    NSRect, NSSize, NSString, ns_string,
};
use objc2_quartz_core::CAMediaTiming;
use objc2_quartz_core::{
    CABasicAnimation, CACurrentMediaTime, CALayer, CAMediaTimingFunction, CATransaction,
};

use crate::pane::{JumpButton, TerminalPane};

/// How far the line stands from the pane's corner, points.
const MARGIN: f64 = 8.0;
/// Between the words and the link, points.
const LINK_GAP: f64 = 12.0;
/// A bar's thickness and the room between two, as parts of one cell: three bars and their two
/// gaps are the cell (2.0 and 0.9 points in a cell of 7.8, that of a 13-point terminal font).
const BAR_WIDTH_OF_CELL: f64 = 0.26;
const BAR_GAP_OF_CELL: f64 = 0.11;
/// How low a bar falls and how high it rises, as parts of the tallest (the font's capital). Not
/// lower: three bars at a quarter read as "...", the very thing the line is not to say.
const LOW: f64 = 0.3;
const HIGH: f64 = 1.0;
/// How long a bar takes to go up, and as long to come down, seconds: a whole breath is a second.
const HALF_SECS: f64 = 0.5;
/// How far behind the one before it each bar is, seconds: a third of a breath, so that the three
/// are never low together and one of them is always near the top.
const LAG_SECS: f64 = 2.0 * HALF_SECS / 3.0;
/// The heights the bars stand at while they do not move (Reduce Motion), as parts of the tallest:
/// all different.
const STILL: [f64; 3] = [0.5, 1.0, 0.7];
/// The key the animation is added under on each bar's layer.
fn key() -> &'static NSString {
    ns_string!("bars")
}
/// The least a line's font is drawn at, and the most, points: near the terminal's own, where
/// that is a size a line of words can stand at beside bars one cell of 13 points wide.
const FONT_LEAST: f64 = 13.0;
const FONT_MOST: f64 = 16.0;

define_class!(
    // SAFETY: NSView is designed for subclassing; WaitRow implements no `Drop`, has no ivar and
    // is born with NSView's `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriWaitRow"]
    pub(crate) struct WaitRow;

    unsafe impl NSObjectProtocol for WaitRow {}

    impl WaitRow {
        /// Never takes part in hit testing, and with it nothing inside it: a click on the line
        /// or on the bars is a click on the pane under them.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

/// What the bars know of themselves.
pub(crate) struct BarsState {
    /// The three bars' layers, left to right.
    layers: RefCell<Vec<Retained<CALayer>>>,
    /// Whether they are to move (the line is up and Reduce Motion is off).
    moving: Cell<bool>,
    /// The width of the cell they stand in, points.
    cell: Cell<f64>,
    /// How tall the tallest is: the font's capital, points.
    top: Cell<f64>,
    /// When the bars began to move, in Core Animation's own time base: every animation put on
    /// them begins there, so that one put again goes on where the bars would be. Forgotten when
    /// they stop.
    epoch: Cell<Option<f64>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; WaitBars implements no `Drop` and its ivars
    // are plain state of the main thread.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriWaitBars"]
    #[ivars = BarsState]
    pub(crate) struct WaitBars;

    unsafe impl NSObjectProtocol for WaitBars {}

    impl WaitBars {
        /// Takes no click: the pane's own view is hit.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        /// The view came into a window, or left one (a tab put in the background, a split, a
        /// tab moved): the layers' animation is put again, as the window changed under it.
        #[unsafe(method(viewDidMoveToWindow))]
        fn did_move_to_window(&self) {
            // SAFETY: the superclass's own method, called as an override does.
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            self.settle();
        }

        /// A view that was hidden is shown: the same.
        #[unsafe(method(viewDidUnhide))]
        fn did_unhide(&self) {
            // SAFETY: the superclass's own method, called as an override does.
            let _: () = unsafe { msg_send![super(self), viewDidUnhide] };
            self.settle();
        }

        /// The appearance of the window changed (the theme's, a dark theme gives the window
        /// `DarkAqua`): the bars take the colour of the words again.
        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn did_change_appearance(&self) {
            // SAFETY: the superclass's own method, called as an override does.
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.recolor();
        }

        /// AppKit lays the layer out (and may flip it): the bars stand again.
        #[unsafe(method(layout))]
        fn layout_layers(&self) {
            // SAFETY: the superclass's own method, called as an override does.
            let _: () = unsafe { msg_send![super(self), layout] };
            self.place();
        }
    }
);

impl WaitBars {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(BarsState {
            layers: RefCell::new(Vec::new()),
            moving: Cell::new(false),
            cell: Cell::new(0.0),
            top: Cell::new(0.0),
            epoch: Cell::new(None),
        });
        // SAFETY: `NSView`'s designated initializer; the ivars are set.
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0))]
        };
        this.setWantsLayer(true);
        if let Some(host) = this.layer() {
            let mut layers = this.ivars().layers.borrow_mut();
            for _ in 0..3 {
                let bar = CALayer::new();
                host.addSublayer(&bar);
                layers.push(bar);
            }
        }
        this.recolor();
        this.place();
        this
    }

    /// The bars stand in a cell `width` points wide, the tallest of them `top` points; bars that
    /// move go on moving in that height (the animation is made for a height), in their phase.
    fn set_size(&self, width: f64, top: f64) {
        self.ivars().cell.set(width);
        self.ivars().top.set(top);
        self.setFrameSize(NSSize::new(width, top));
        self.place();
        self.settle();
    }

    /// Where the bars stand, and how tall: each in its own place, bottom on the view's bottom,
    /// the three filling the cell. The layer is AppKit's to flip (it does once the view stands
    /// in a flipped root): the bottom is found through the view's own conversion.
    fn place(&self) {
        let layers = self.ivars().layers.borrow();
        if layers.is_empty() {
            return;
        }
        let cell = self.ivars().cell.get();
        let top = self.ivars().top.get();
        let width = cell * BAR_WIDTH_OF_CELL;
        let step = width + cell * BAR_GAP_OF_CELL;
        let flipped = self.convertPointToLayer(NSPoint::new(0.0, top)).y
            < self.convertPointToLayer(NSPoint::new(0.0, 0.0)).y;
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        for (index, bar) in layers.iter().enumerate() {
            let foot =
                self.convertPointToLayer(NSPoint::new(index as f64 * step + width / 2.0, 0.0));
            // The bar grows from its foot: the anchor is the bottom edge, whichever way the
            // layer's y runs.
            bar.setAnchorPoint(NSPoint::new(0.5, if flipped { 1.0 } else { 0.0 }));
            bar.setPosition(foot);
            // What the layer says it is (and what stands where nothing animates it): the
            // heights of the bars at rest.
            bar.setBounds(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(width, top * STILL[index]),
            ));
        }
        CATransaction::commit();
    }

    /// The colour of the words, for the appearance this view is drawn in.
    fn recolor(&self) {
        let color = dim_color(self);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        for bar in self.ivars().layers.borrow().iter() {
            bar.setBackgroundColor(Some(&color));
        }
        CATransaction::commit();
    }

    /// The bars move, or stand (and when they move again, they start again).
    fn set_moving(&self, on: bool) {
        self.ivars().moving.set(on);
        if !on {
            self.ivars().epoch.set(None);
        }
        self.place();
        self.settle();
    }

    /// Makes the animation what it should be: playing where the bars are to move and the view
    /// is in a window, gone where they stand.
    fn settle(&self) {
        let layers = self.ivars().layers.borrow();
        if layers.is_empty() {
            return;
        }
        if !(self.ivars().moving.get() && self.window().is_some()) {
            for bar in layers.iter() {
                bar.removeAnimationForKey(key());
            }
            return;
        }
        let epoch = self.ivars().epoch.get().unwrap_or_else(|| {
            let now = CACurrentMediaTime();
            self.ivars().epoch.set(Some(now));
            now
        });
        let top = self.ivars().top.get();
        for (index, bar) in layers.iter().enumerate() {
            // The first bar is the furthest on and each is `LAG_SECS` behind the one before: the
            // wave runs from left to right, and none waits for its turn to begin. From the same
            // moment whenever it is put: the animation repeats without end, so one put again is
            // where the one before it would be now.
            let begin = epoch - (layers.len() - 1 - index) as f64 * LAG_SECS;
            let animation = breathing(top);
            animation.setBeginTime(bar.convertTime_fromLayer(begin, None));
            bar.addAnimation_forKey(&animation, Some(key()));
        }
    }

    /// Takes the animation off every bar, as a layer may lose it when its window changes: a
    /// test does it by hand, to see it put again.
    pub(crate) fn drop_animation(&self) {
        for bar in self.ivars().layers.borrow().iter() {
            bar.removeAnimationForKey(key());
        }
    }

    /// Whether every bar has its animation, as the view is in a window.
    pub(crate) fn is_playing(&self) -> bool {
        let layers = self.ivars().layers.borrow();
        !layers.is_empty()
            && self.window().is_some()
            // SAFETY: a plain read of a layer this view made.
            && layers
                .iter()
                .all(|bar| unsafe { bar.animationForKey(key()) }.is_some())
    }

    /// Whether the bars are to move.
    pub(crate) fn is_moving(&self) -> bool {
        self.ivars().moving.get()
    }

    /// How low and how high each bar's animation goes, as parts of the tallest the bars are now:
    /// an animation made for another height (a font that changed) is off. `None` for a bar that
    /// has none.
    pub(crate) fn reach(&self) -> Vec<Option<(f64, f64)>> {
        let top = self.ivars().top.get();
        self.ivars()
            .layers
            .borrow()
            .iter()
            .map(|bar| {
                // SAFETY: a plain read of a layer this view made.
                let animation = unsafe { bar.animationForKey(key()) }?;
                // SAFETY: the animation is the `CABasicAnimation` `breathing` made, whose two
                // values are the `NSNumber`s it was given.
                let (from, to): (Option<Retained<NSNumber>>, Option<Retained<NSNumber>>) = unsafe {
                    (
                        msg_send![&*animation, fromValue],
                        msg_send![&*animation, toValue],
                    )
                };
                Some((from?.doubleValue() / top, to?.doubleValue() / top))
            })
            .collect()
    }

    /// When each bar's animation begins, in Core Animation's own time base: the same for an
    /// animation put again as for the one before it. `None` for a bar that has none.
    pub(crate) fn begins(&self) -> Vec<Option<f64>> {
        self.ivars()
            .layers
            .borrow()
            .iter()
            .map(|bar| {
                // SAFETY: a plain read of a layer this view made.
                let animation = unsafe { bar.animationForKey(key()) }?;
                Some(bar.convertTime_toLayer(animation.beginTime(), None))
            })
            .collect()
    }

    /// The heights the bars stand at, as parts of the tallest: what each layer's model says,
    /// which is the heights at rest.
    pub(crate) fn heights(&self) -> Vec<f64> {
        let top = self.ivars().top.get();
        self.ivars()
            .layers
            .borrow()
            .iter()
            .map(|bar| bar.bounds().size.height / top)
            .collect()
    }

    /// The heights the bars are drawn at now, as parts of the tallest, as the animation has them
    /// (the model's where there is none).
    pub(crate) fn drawn_heights(&self) -> Vec<f64> {
        let top = self.ivars().top.get();
        self.ivars()
            .layers
            .borrow()
            .iter()
            .map(|bar| {
                // SAFETY: a plain read of a layer this view made.
                let shown = unsafe { bar.presentationLayer() };
                shown.as_ref().unwrap_or(bar).bounds().size.height / top
            })
            .collect()
    }

    /// The colour the first bar is, as red, green, blue and alpha.
    pub(crate) fn color(&self) -> Option<[f64; 4]> {
        let layers = self.ivars().layers.borrow();
        let color = layers.first()?.backgroundColor()?;
        rgba(&color)
    }
}

/// The animation of a bar's height: from low to high and back, each way [`HALF_SECS`] and slow at
/// either end, over and over.
fn breathing(top: f64) -> Retained<CABasicAnimation> {
    let animation = CABasicAnimation::animationWithKeyPath(Some(ns_string!("bounds.size.height")));
    // SAFETY: `NSNumber`s, what a height takes.
    unsafe {
        animation.setFromValue(Some(&NSNumber::numberWithDouble(top * LOW)));
        animation.setToValue(Some(&NSNumber::numberWithDouble(top * HIGH)));
    }
    // Ease in, ease out.
    animation.setTimingFunction(Some(&CAMediaTimingFunction::functionWithControlPoints(
        0.42, 0.0, 0.58, 1.0,
    )));
    animation.setDuration(HALF_SECS);
    animation.setAutoreverses(true);
    animation.setRepeatCount(f32::INFINITY);
    animation.setRemovedOnCompletion(false);
    animation
}

/// The words' dim colour as the view's appearance resolves it, a `CGColor` a layer can take.
fn dim_color(view: &NSView) -> Retained<CGColor> {
    let seen: Rc<RefCell<Option<Retained<CGColor>>>> = Rc::new(RefCell::new(None));
    let into = Rc::clone(&seen);
    let block = RcBlock::new(move || {
        *into.borrow_mut() = Some(NSColor::secondaryLabelColor().CGColor());
    });
    view.effectiveAppearance()
        .performAsCurrentDrawingAppearance(&block);
    let color = seen.borrow_mut().take();
    color.unwrap_or_else(|| NSColor::secondaryLabelColor().CGColor())
}

/// A colour as red, green, blue and alpha (the colour spaces AppKit's own colours come in have
/// these four).
fn rgba(color: &CGColor) -> Option<[f64; 4]> {
    let count = CGColor::number_of_components(Some(color));
    if count < 2 {
        return None;
    }
    let parts = CGColor::components(Some(color));
    if parts.is_null() {
        return None;
    }
    // SAFETY: Core Graphics hands as many components as `number_of_components` said.
    let parts = unsafe { std::slice::from_raw_parts(parts, count) };
    Some(match count {
        2 => [parts[0], parts[0], parts[0], parts[1]],
        _ => [parts[0], parts[1], parts[2], *parts.get(3).unwrap_or(&1.0)],
    })
}

/// The font the words of the line are set in: the terminal's, the very face the grid opens
/// ([`bt_gpu::chain_face`]: the family the person's `font.family` names when it opens by its name,
/// else the one the app ships where its fonts are registered for the process, else the system's
/// chain). The size is the terminal's, held between [`FONT_LEAST`] and [`FONT_MOST`].
pub(crate) fn line_font(font: &FontOptions) -> Retained<NSFont> {
    resolve(
        font.family.as_deref(),
        font.size.clamp(FONT_LEAST, FONT_MOST),
    )
}

/// [`line_font`] of the family `family` at `size`: the face the grid opens, else the system's
/// monospaced font.
pub(crate) fn resolve(family: Option<&str>, size: f64) -> Retained<NSFont> {
    let face = bt_gpu::chain_face(family);
    face.postscript
        .as_deref()
        .and_then(|name| open_face(name, &face.family, size))
        .unwrap_or_else(|| NSFont::monospacedSystemFontOfSize_weight(size, 0.0))
}

/// The face named `postscript` at `size`, if AppKit opens it as a face of `family` by the
/// terminal's own rule ([`bt_gpu::same_family`]). By its name: AppKit's matching of a family by a
/// descriptor alone now and then answered nothing while other threads asked it for fonts too, and
/// a name is the one face, with nothing to match.
fn open_face(postscript: &str, family: &str, size: f64) -> Option<Retained<NSFont>> {
    let font = NSFont::fontWithName_size(&NSString::from_str(postscript), size)?;
    let opened = font.familyName()?.to_string();
    bt_gpu::same_family(&opened, family).then_some(font)
}

/// A font as the family it is, whether it has one width for every character, and the width of one
/// cell, points.
pub(crate) fn describe(font: &NSFont) -> (String, bool, f64) {
    (
        font.familyName()
            .map(|name| name.to_string())
            .unwrap_or_default(),
        font.isFixedPitch(),
        cell_of(font),
    )
}

/// The width of one cell of `font`: what a `0` is across, points.
fn cell_of(font: &NSFont) -> f64 {
    let values: [&AnyObject; 1] = [font.as_ref()];
    // SAFETY: AppKit's font attribute key, with the `NSFont` it documents as its value.
    let attributes = unsafe {
        NSDictionary::<NSAttributedStringKey, AnyObject>::from_slices(
            &[NSFontAttributeName],
            &values,
        )
    };
    // SAFETY: attributes of the types `sizeWithAttributes:` documents.
    unsafe { ns_string!("0").sizeWithAttributes(Some(&attributes)) }.width
}

/// The line of the wait and the link beside it, both born hidden.
#[derive(Clone)]
pub(crate) struct WaitLine {
    row: Retained<WaitRow>,
    bars: Retained<WaitBars>,
    text: Retained<NSTextField>,
    link: Retained<JumpButton>,
    /// The person's Reduce Motion, as the pane was last told.
    reduced: Rc<Cell<bool>>,
    /// The font of the words and of the link.
    font: Rc<RefCell<Retained<NSFont>>>,
}

impl WaitLine {
    pub(crate) fn new(mtm: MainThreadMarker, reduce_motion: bool, font: Retained<NSFont>) -> Self {
        // SAFETY: `NSView`'s designated initializer; the subclass has no ivar.
        let row = WaitRow::alloc(mtm).set_ivars(());
        let row: Retained<WaitRow> = unsafe {
            msg_send![super(row), initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0))]
        };
        // Anchored to the top left: the pane's y is up.
        row.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewMaxXMargin | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        row.setHidden(true);
        let bars = WaitBars::new(mtm);
        let text = NSTextField::labelWithString(ns_string!(""), mtm);
        text.setFont(Some(&font));
        text.setTextColor(Some(&NSColor::secondaryLabelColor()));
        text.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        row.addSubview(&bars);
        row.addSubview(&text);

        // SAFETY: `NSButton`'s `init`; the class (the jump button's) has no ivar.
        let link = JumpButton::alloc(mtm).set_ivars(());
        let link: Retained<JumpButton> = unsafe { msg_send![super(link), init] };
        link.setBordered(false);
        // The keyboard stays with the terminal.
        link.setRefusesFirstResponder(true);
        link.setAttributedTitle(&link_title(LINK_WORDS, &font));
        link.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewMaxXMargin | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        link.setHidden(true);
        bars.set_size(cell_of(&font), font.capHeight());
        Self {
            row,
            bars,
            text,
            link,
            reduced: Rc::new(Cell::new(reduce_motion)),
            font: Rc::new(RefCell::new(font)),
        }
    }

    /// The pane has this theme (at its birth, and whenever it changes): the bars take the colour
    /// of the words again. (The words follow the window's appearance by themselves; the bars are
    /// layers, so they are told.)
    pub(crate) fn paint(&self) {
        self.bars.recolor();
    }

    /// The views the pane puts over its terminal: the line and the link.
    pub(crate) fn views(&self) -> [&NSView; 2] {
        [&self.row, &self.link]
    }

    /// Points the link at the pane's `openOrdinaryShell:`.
    pub(crate) fn aim(&self, pane: &TerminalPane) {
        let target: &AnyObject = pane.as_ref();
        // SAFETY: the selector is the pane's `openOrdinaryShell:`, which takes a single
        // `Option<&AnyObject>`; the pane holds the link (the target is weak).
        unsafe {
            self.link.setTarget(Some(target));
            self.link.setAction(Some(sel!(openOrdinaryShell:)));
        }
    }

    /// The terminal's font changed (its family, its size, a zoom): the words and the link are set
    /// in it, and a line that is up stands again in a pane of `bounds`.
    pub(crate) fn set_font(&self, font: Retained<NSFont>, bounds: NSRect) {
        self.text.setFont(Some(&font));
        self.link.setAttributedTitle(&link_title(LINK_WORDS, &font));
        self.bars.set_size(cell_of(&font), font.capHeight());
        self.font.replace(font);
        if !self.row.isHidden() {
            self.layout(bounds);
        }
    }

    /// The pane is `bounds` now (a split, a narrower window): a line that is up stands again in
    /// it, and so does its link.
    pub(crate) fn fit(&self, bounds: NSRect) {
        if !self.row.isHidden() {
            self.layout(bounds);
        }
    }

    /// The line says `words`, in the corner of a pane of `bounds`; the bars move unless the
    /// person has Reduce Motion on.
    pub(crate) fn show(&self, words: &str, bounds: NSRect) {
        self.text.setStringValue(&NSString::from_str(words));
        self.row.setHidden(false);
        self.layout(bounds);
        self.bars.set_moving(!self.reduced.get());
    }

    /// The line stands in the corner of a pane of `bounds`: the bars in a cell, a space, the
    /// words; the bars on the words' baseline. Words that do not fit the pane are shortened.
    fn layout(&self, bounds: NSRect) {
        let font = self.font.borrow();
        let cell = cell_of(&font);
        let fit = self.text.fittingSize();
        let first = 2.0 * cell;
        let room = bounds.size.width - 2.0 * MARGIN - first;
        let width = fit.width.min(room.max(0.0));
        // The words' baseline stands the font's descent above the bottom of the field.
        let baseline = -font.descender();
        let height = fit.height.max(baseline + font.capHeight());
        let text_y = (height - fit.height) / 2.0;
        self.bars
            .setFrameOrigin(NSPoint::new(0.0, text_y + baseline));
        self.text.setFrame(NSRect::new(
            NSPoint::new(first, text_y),
            NSSize::new(width, fit.height),
        ));
        self.row.setFrame(NSRect::new(
            NSPoint::new(MARGIN, (bounds.size.height - MARGIN - height).max(0.0)),
            NSSize::new(first + width, height),
        ));
        self.place_link(bounds);
    }

    /// The link to an ordinary shell appears beside the line (below it when the pane is too
    /// narrow for both). Only while the line is up.
    pub(crate) fn offer(&self, bounds: NSRect) {
        if self.row.isHidden() {
            return;
        }
        self.link.setHidden(false);
        self.place_link(bounds);
    }

    /// The wait is over: the line goes, and the link with it, and the bars stop.
    pub(crate) fn hide(&self) {
        self.bars.set_moving(false);
        self.row.setHidden(true);
        self.link.setHidden(true);
    }

    /// Clicks the link as a person does, through the control's own action, if it shows.
    pub(crate) fn press_link(&self) {
        if !self.link.isHidden() {
            // SAFETY: a plain call on a button this line made, with no sender.
            unsafe { self.link.performClick(None) };
        }
    }

    /// The link alone goes (it was clicked).
    pub(crate) fn withdraw(&self) {
        self.link.setHidden(true);
    }

    /// Reduce Motion changed: bars that are up move or stand.
    pub(crate) fn set_reduce_motion(&self, reduce: bool) {
        self.reduced.set(reduce);
        if !self.row.isHidden() {
            self.bars.set_moving(!reduce);
        }
    }

    /// Where the link stands: after the words on their line, or under the line when the pane is
    /// too narrow for both. Under it, a pane too narrow for the whole link gets its short words
    /// ([`LINK_SHORT`]) rather than words cut in the middle of the action, and only a pane too
    /// narrow for those too cuts them, never past the pane's edge.
    fn place_link(&self, bounds: NSRect) {
        if self.link.isHidden() {
            return;
        }
        let font = self.font.borrow().clone();
        self.link.setAttributedTitle(&link_title(LINK_WORDS, &font));
        let mut fit = self.link.fittingSize();
        let line = self.row.frame();
        let beside = line.origin.x + line.size.width + LINK_GAP;
        let (origin, width) = if beside + fit.width + MARGIN <= bounds.size.width {
            (
                NSPoint::new(
                    beside,
                    line.origin.y + (line.size.height - fit.height) / 2.0,
                ),
                fit.width,
            )
        } else {
            let room = (bounds.size.width - 2.0 * MARGIN).max(0.0);
            if fit.width > room {
                self.link.setAttributedTitle(&link_title(LINK_SHORT, &font));
                fit = self.link.fittingSize();
            }
            (
                NSPoint::new(MARGIN, (line.origin.y - fit.height).max(0.0)),
                fit.width.min(room),
            )
        };
        self.link
            .setFrame(NSRect::new(origin, NSSize::new(width, fit.height)));
    }

    /// The words of the line, while it shows.
    pub(crate) fn words(&self) -> Option<String> {
        (!self.row.isHidden()).then(|| self.text.stringValue().to_string())
    }

    /// The words of the link, while it shows.
    pub(crate) fn link_words(&self) -> Option<String> {
        (!self.link.isHidden()).then(|| self.link.title().to_string())
    }

    /// Whether the bars are to move.
    pub(crate) fn is_moving(&self) -> bool {
        self.bars.is_moving()
    }

    /// Whether the bars' animation is on every bar, in a window.
    pub(crate) fn is_playing(&self) -> bool {
        self.bars.is_playing()
    }

    /// How low and how high each bar's animation goes (see [`WaitBars::reach`]).
    pub(crate) fn reach(&self) -> Vec<Option<(f64, f64)>> {
        self.bars.reach()
    }

    /// When each bar's animation begins (see [`WaitBars::begins`]).
    pub(crate) fn begins(&self) -> Vec<Option<f64>> {
        self.bars.begins()
    }

    /// Takes the animation off the bars (see [`WaitBars::drop_animation`]).
    pub(crate) fn drop_animation(&self) {
        self.bars.drop_animation();
    }

    /// The bars' heights at rest, as parts of the tallest: what the model says.
    pub(crate) fn heights(&self) -> Vec<f64> {
        self.bars.heights()
    }

    /// The bars' heights as parts of the tallest, as they are drawn now.
    pub(crate) fn drawn_heights(&self) -> Vec<f64> {
        self.bars.drawn_heights()
    }

    /// The bars' colour, and the words' (red, green, blue, alpha), while they show.
    pub(crate) fn colors(&self) -> Option<([f64; 4], [f64; 4])> {
        let words = self.text.textColor()?;
        let words = rgba_of(&words, &self.text)?;
        Some((self.bars.color()?, words))
    }

    /// The family the words are set in.
    pub(crate) fn family(&self) -> String {
        self.font
            .borrow()
            .familyName()
            .map(|name| name.to_string())
            .unwrap_or_default()
    }

    /// Whether the font the words are set in has one width for every character.
    pub(crate) fn is_fixed_pitch(&self) -> bool {
        self.font.borrow().isFixedPitch()
    }

    /// The width of a cell of the font, points.
    pub(crate) fn cell(&self) -> f64 {
        cell_of(&self.font.borrow())
    }

    /// Where the line is, in the pane's coordinates, while it shows.
    pub(crate) fn line_frame(&self) -> Option<NSRect> {
        (!self.row.isHidden()).then(|| self.row.frame())
    }

    /// Where the bars are, in the pane's coordinates, while the line shows.
    pub(crate) fn bars_frame(&self) -> Option<NSRect> {
        (!self.row.isHidden()).then(|| {
            let line = self.row.frame();
            let bars = self.bars.frame();
            NSRect::new(
                NSPoint::new(line.origin.x + bars.origin.x, line.origin.y + bars.origin.y),
                bars.size,
            )
        })
    }

    /// Where the link is, in the pane's coordinates, while it shows.
    pub(crate) fn link_frame(&self) -> Option<NSRect> {
        (!self.link.isHidden()).then(|| self.link.frame())
    }
}

/// A colour of AppKit as red, green, blue and alpha, as `view`'s appearance resolves it.
fn rgba_of(color: &NSColor, view: &NSView) -> Option<[f64; 4]> {
    let seen: Rc<RefCell<Option<Retained<CGColor>>>> = Rc::new(RefCell::new(None));
    let into = Rc::clone(&seen);
    let block = RcBlock::new(move || {
        *into.borrow_mut() = Some(color.CGColor());
    });
    view.effectiveAppearance()
        .performAsCurrentDrawingAppearance(&block);
    let resolved = seen.borrow_mut().take()?;
    rgba(&resolved)
}

/// The link's words, and the ones it has in a pane too narrow for them.
const LINK_WORDS: &str = "Open an ordinary shell instead";
const LINK_SHORT: &str = "Open an ordinary shell";

/// `words`, in the colour a link has and the font of the line, cut at their end only where the
/// pane is too narrow even for the short words.
fn link_title(words: &str, font: &NSFont) -> Retained<NSAttributedString> {
    let color = NSColor::linkColor();
    let paragraph = NSMutableParagraphStyle::new();
    paragraph.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    let values: [&AnyObject; 3] = [color.as_ref(), font.as_ref(), paragraph.as_ref()];
    // SAFETY: AppKit's three attribute keys (extern statics), each with the value type it
    // documents — an `NSColor`, an `NSFont` and an `NSParagraphStyle`.
    unsafe {
        let attributes = NSDictionary::<NSAttributedStringKey, AnyObject>::from_slices(
            &[
                NSForegroundColorAttributeName,
                NSFontAttributeName,
                NSParagraphStyleAttributeName,
            ],
            &values,
        );
        NSAttributedString::new_with_attributes(&NSString::from_str(words), &attributes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The words are set in a font that has one width for every character, whatever the machine
    /// has to give: the system's, a family that exists, a family that does not.
    #[test]
    fn the_font_of_the_line_has_one_width_for_every_character() {
        for family in [None, Some("Menlo"), Some("a family nobody has")] {
            let font = resolve(family, 13.0);
            assert!(
                font.isFixedPitch(),
                "{family:?} opened {:?}",
                font.familyName()
            );
        }
    }

    /// A face opens by its name as itself, of its family by the terminal's rule (any case), and
    /// a name nobody has, or a face of another family, is not some other font under it.
    #[test]
    fn a_face_opens_by_its_name_or_not_at_all() {
        let menlo = open_face("Menlo-Regular", "menlo", 13.0).expect("Menlo is on every Mac");
        assert_eq!(menlo.fontName().to_string(), "Menlo-Regular");
        assert!(open_face("NobodysFace-Regular", "Menlo", 13.0).is_none());
        assert!(open_face("Menlo-Regular", "Courier", 13.0).is_none());
    }

    /// The words are set in the very face the terminal's grid opens, by the terminal's own chain
    /// and case rule: the family written (in any case), and for none and for one that does not
    /// exist the chain's default — here, with the app's fonts not registered, the system's chain.
    #[test]
    fn the_line_is_set_in_the_face_the_terminal_opens() {
        let mut options = FontOptions::default();
        for family in [
            None,
            Some("Menlo"),
            Some("menlo"),
            Some("a family nobody has"),
        ] {
            options.family = family.map(str::to_owned);
            let face = bt_gpu::chain_face(family);
            let font = line_font(&options);
            assert_eq!(
                Some(font.fontName().to_string()),
                face.postscript,
                "{family:?}: the terminal opens {face:?}"
            );
        }
    }

    /// The size follows the terminal's, between two limits.
    #[test]
    fn the_size_of_the_line_follows_the_terminal_between_two_limits() {
        let mut options = FontOptions::default();
        for (asked, drawn) in [(9.0, 13.0), (13.0, 13.0), (14.5, 14.5), (30.0, 16.0)] {
            options.size = asked;
            assert!(
                (line_font(&options).pointSize() - drawn).abs() < 0.01,
                "{asked} -> {drawn}"
            );
        }
    }

    /// The three bars and their two gaps are one cell, in a cell of any width; they fall to
    /// [`LOW`] of the capital and rise to all of it, a third of a breath apart, and at rest they
    /// are all different.
    #[test]
    fn the_bars_fit_one_cell_and_rest_at_different_heights() {
        // What does not depend on a machine is checked as the code is built.
        const _: () = {
            assert!(LOW > 0.0 && LOW < HIGH && HIGH <= 1.0);
            assert!(2.0 * HALF_SECS >= 0.8 && 2.0 * HALF_SECS <= 1.2);
            assert!(LOW >= 0.3);
            assert!(3.0 * LAG_SECS == 2.0 * HALF_SECS);
            assert!(STILL[0] != STILL[1] && STILL[1] != STILL[2] && STILL[0] != STILL[2]);
        };
        let group = 3.0 * BAR_WIDTH_OF_CELL + 2.0 * BAR_GAP_OF_CELL;
        assert!((group - 1.0).abs() < 1e-9, "the bars are {group} of a cell");
        assert!(
            (BAR_WIDTH_OF_CELL * 7.8 - 2.0).abs() < 0.05,
            "2 points in 7.8"
        );
        assert!(STILL.iter().all(|height| (LOW..=HIGH).contains(height)));
        // The capital of a terminal font fits the line it stands in.
        let font = resolve(Some("Menlo"), 13.0);
        assert!(font.capHeight() > 0.0 && font.capHeight() < font.pointSize());
    }
}
