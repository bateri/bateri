//! A transfer's **fill**: a ground filling from the left as its transfer goes, and playing out the
//! end when it is over — running full and resting for an arrival, turning to the error's colour
//! for a failure, then fading ([`Ending`]). A tab's chip wears it in the tab bar, and the upload
//! card ([`crate::upload_card`]) wears the same one over a pane with no dock: one sign of an
//! upload, in the two places it shows.
//!
//! Its own module, beside both: the card is the pane's, the bar the window's, and a pane does
//! not reach into the window's modules (`make audit`).

use std::cell::Cell;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{
    AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
};
use objc2_app_kit::{NSAccessibility, NSGradient, NSView};
use objc2_foundation::{NSArray, NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize, ns_string};
use objc2_quartz_core::{
    CAKeyframeAnimation, CALayer, CAMediaTiming, CAMediaTimingFunction, CATransaction,
};

use crate::tabs::{self, Ending, Finish};
use crate::tint::{Tint, paint_in};

/// The colours a fill is drawn in: its accent, the error's for a failure, and the tone of the
/// way still to go (a tab's hover).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FillInks {
    pub(crate) accent: u32,
    pub(crate) error: u32,
    pub(crate) way: Tint,
}

impl FillInks {
    /// The fill's two ends: the accent's tail and leading edge, or the error's once the transfer
    /// has failed ([`FILL_TAIL`], [`FILL_HEAD`]).
    pub(crate) fn ends(self, failed: bool) -> (Tint, Tint) {
        let rgb = if failed { self.error } else { self.accent };
        (Tint::of(rgb, FILL_TAIL), Tint::of(rgb, FILL_HEAD))
    }
}

/// The accent at the fill's tail and at its leading edge: 7 % and 14 %. On the bar's ladder of
/// grounds these are the hover's tone (the foreground at 5 %) and the selected face's (10 %) in
/// both themes, so a tab in the background is never brighter than the selected one and the
/// leading edge is the darkest place, which tells the way it goes. The way still to go is the
/// hover's tone itself ([`FillInks::way`]).
const FILL_TAIL: f64 = 0.07;
const FILL_HEAD: f64 = 0.14;

/// The width of the pictures the fill's layers hold: a gradient stretched over the layer is
/// smooth at any width, and one is painted only when its colours change.
const FILL_PICTURE: f64 = 64.0;

/// What a chip's fill is doing ([`TransferFill`]).
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
    /// The corner the fill takes: its holder's, the chip's or the card's it lies in.
    radius: f64,
    /// The fill's leading edge is rounded at [`Self::radius`] too (the upload card's,
    /// [`crate::upload_card`]); a chip's is straight, so the way still to go meets it.
    round_head: bool,
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
    // SAFETY: NSView is designed for subclassing; TransferFill implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTransferFill"]
    #[ivars = FillIvars]
    pub(crate) struct TransferFill;

    unsafe impl NSObjectProtocol for TransferFill {}

    impl TransferFill {
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
impl TransferFill {
    /// A fill in a shape whose corner is `radius`; `round_head`: its leading edge is rounded too.
    pub(crate) fn new(mtm: MainThreadMarker, radius: f64, round_head: bool) -> Retained<Self> {
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
            radius,
            round_head,
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
        layer.setCornerRadius(self.ivars().radius);
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
    pub(crate) fn lit(&self, now: Instant) -> bool {
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
    pub(crate) fn reset(&self) {
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
        if iv.round_head {
            // No wider a curve than half the fill: a narrow start is a small round, not a lens.
            iv.fill
                .setCornerRadius(iv.radius.min(size.width * share / 2.0).max(0.0));
        }
        CATransaction::commit();
    }

    /// The pictures of the fill and of the way still to go, in the palette's colours
    /// (`failed`: the error's) and at the scale of the screen the chip is on; painted again only
    /// when one of them changed.
    fn ink(&self, inks: FillInks, failed: bool) {
        let iv = self.ivars();
        let scale = self
            .window()
            .map_or(2.0, |window| window.backingScaleFactor());
        let (tail, head) = inks.ends(failed);
        let key = (tail, head, inks.way, scale);
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
        paint_in(&iv.track, picture, scale, gradient(inks.way, inks.way));
    }

    /// Shows the transfer at `share` (`None`: none flows) in a chip of `size`, lit by `inks`.
    /// A fill already at `share` is left alone, one that moved follows at an even pace; one that
    /// begins starts there at once, from a clean slate. A transfer that stops flowing without
    /// its end having been told ([`Self::end`]) is a stop. `selected`: the chip's own face is the
    /// ground, so there is no way still to go to show.
    pub(crate) fn follow(
        &self,
        size: NSSize,
        share: Option<f64>,
        inks: FillInks,
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
                self.end(Finish::Stopped, reduce, inks);
            }
            (None, _) => {}
        }
        let state = self.state_at(now);
        match state {
            Filling::Flowing(_) => self.ink(inks, false),
            Filling::Ending { failed, .. } => self.ink(inks, failed),
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
    pub(crate) fn end(&self, finish: Finish, reduce: bool, inks: FillInks) -> f64 {
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
        self.ink(inks, ending.failed);
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
