//! The **upload card**: where a pane with no dock ([shell] integration = "blocks", or a program
//! that hides the band) shows its transfer line. The dock's row says it where there is a dock;
//! a pane without one showed nothing but the tab's fill, and a person who dropped a file on an
//! agent there could not tell whether it went.
//!
//! The card floats over the pane's bottom right corner on the surface bateri already floats over
//! a terminal — the find panel's ([`crate::search_bar`]: its fill lifted from the ground, its
//! line, radius and shadow, its inset). Inside: the line's words up to the bytes
//! ([`Transfer::short`]: what goes and how much of it has), and one verb button, *Cancel* with
//! its key dim in it, which is Shell ▸ Cancel Upload's (`cancelUpload:`, the whole queue). Its
//! ground fills from the left as the tab's chip does — the same fill ([`TransferFill`]): the accent
//! from tail to leading edge, following each report, running full, resting and fading at the
//! end — with its leading edge rounded at the card's corner. Its width is the upload's and does
//! not shrink while it shows: the numbers change inside it.
//!
//! **Looks, not decisions:** the line, its words and when it goes are the queue's
//! ([`crate::uploader`], `upload::Transfers`); the pane shows the card from the one place a line
//! is shown ([`crate::pane::TerminalPane::show_transfer`]) and plays its end where the tab's is
//! played.

use std::cell::Cell;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use bt_core::{Theme, Transfer, TransferKind, TransferTone};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSAutoresizingMaskOptions, NSBezelStyle,
    NSBox, NSBoxType, NSButton, NSControlSize, NSFont, NSFontAttributeName, NSFontWeightMedium,
    NSForegroundColorAttributeName, NSLineBreakMode, NSMutableParagraphStyle,
    NSParagraphStyleAttributeName, NSShadow, NSTextField, NSTitlePosition, NSView,
    NSWindowOrderingMode,
};
use objc2_foundation::{
    NSAttributedString, NSAttributedStringKey, NSDictionary, NSMutableAttributedString, NSPoint,
    NSRect, NSSize, NSString,
};

use crate::search_bar::{srgb, surface_colors};
use crate::tabs::Finish;
use crate::tint::Tint;
use crate::transfer_fill::{FillInks, TransferFill};

/// The card's distance from the pane's edges, its padding, its corner and its height: the find
/// panel's (`search_bar` INSET, PADDING, RADIUS; a control's 24 pt and the padding twice).
const INSET: f64 = 10.0;
const PAD_X: f64 = 7.0;
const RADIUS: f64 = 11.0;
const HEIGHT: f64 = 38.0;
/// The words' extra room on the left: the button's edge sits at the padding, the words' first
/// letter a little further in, where the rounded corner leaves it room.
const TEXT_LEFT: f64 = PAD_X + 5.0;
/// The gap between the words and the button.
const GAP: f64 = 10.0;
/// The card's narrowest and widest — **design constants**: a short name keeps the card a size the
/// eye finds again, a long one fades at its end instead of covering the pane.
const MIN_WIDTH: f64 = 260.0;
const MAX_WIDTH: f64 = 340.0;
/// The words' size, in points.
const TEXT_SIZE: f64 = 12.5;
/// How long the card takes to come and to go.
const APPEAR_SECS: f64 = 0.15;
/// How bright the words after the name are (the bytes; a failure's count).
const DIM: f64 = 0.62;

/// The card over one pane: built on the first line a pane with no dock shows.
pub(crate) struct UploadCard {
    surface: Retained<NSBox>,
    fill: Retained<TransferFill>,
    label: Retained<NSTextField>,
    cancel: Retained<NSButton>,
    /// Whether the card is up (or on its way up).
    shown: Cell<bool>,
    /// The theme it was last painted in: the words' colours and the fill's.
    theme: Cell<Option<Theme>>,
    /// Bumped by each show and hide: a fade that finishes late leaves a newer state alone.
    generation: Rc<Cell<u64>>,
}

impl UploadCard {
    /// Builds the card hidden in `parent`, right above `below` (the terminal's view, as the find
    /// panel is), with its button's action sent to `target` (the pane, `cancelUpload:`). The
    /// container is not held: the pane holds the card.
    pub(crate) fn new(
        mtm: MainThreadMarker,
        parent: &NSView,
        below: &NSView,
        target: &AnyObject,
    ) -> Self {
        let surface = NSBox::new(mtm);
        // Its own layer: a sibling of the view hosting Metal's layer draws nothing without one
        // (`SearchBar::new`'s note).
        surface.setWantsLayer(true);
        surface.setBoxType(NSBoxType::Custom);
        surface.setTitlePosition(NSTitlePosition::NoTitle);
        surface.setCornerRadius(RADIUS);
        surface.setBorderWidth(1.0);
        surface.setContentViewMargins(NSSize::ZERO);
        let shadow = NSShadow::new();
        shadow.setShadowBlurRadius(14.0);
        shadow.setShadowOffset(NSSize::new(0.0, -3.0));
        shadow.setShadowColor(Some(&srgb(0x000000, 0.28)));
        surface.setShadow(Some(&shadow));
        surface.setFrameSize(NSSize::new(MIN_WIDTH, HEIGHT));
        // The container is not flipped (y up): the bottom right corner keeps the left and top
        // margins flexible.
        surface.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewMinXMargin | NSAutoresizingMaskOptions::ViewMaxYMargin,
        );
        surface.setHidden(true);

        // The fill's corner is the card's less its line (1 pt): the two are concentric.
        let fill = TransferFill::new(mtm, RADIUS - 1.0, true);
        let label = NSTextField::labelWithString(&NSString::new(), mtm);
        label.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
        label.setUsesSingleLineMode(true);
        label.setMaximumNumberOfLines(1);
        // SAFETY: the target is weak and lives as long as the pane keeps the card; the selector
        // is the pane's action with a single `Option<&AnyObject>` argument.
        let cancel = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Cancel"),
                Some(target),
                Some(sel!(cancelUpload:)),
                mtm,
            )
        };
        cancel.setBezelStyle(NSBezelStyle::AccessoryBar);
        cancel.setShowsBorderOnlyWhileMouseInside(false);
        cancel.setControlSize(NSControlSize::Small);
        if let Some(content) = surface.contentView() {
            content.addSubview(&fill);
            content.addSubview(&label);
            content.addSubview(&cancel);
        }
        parent.addSubview_positioned_relativeTo(&surface, NSWindowOrderingMode::Above, Some(below));
        UploadCard {
            surface,
            fill,
            label,
            cancel,
            shown: Cell::new(false),
            theme: Cell::new(None),
            generation: Rc::new(Cell::new(0)),
        }
    }

    /// Paints the surface to the theme, the find panel's way, and keeps the theme for the words
    /// and the fill.
    pub(crate) fn paint(&self, theme: &Theme) {
        let (fill, border) = surface_colors(theme.background, theme.foreground, theme.is_dark());
        self.surface.setFillColor(&srgb(fill, 1.0));
        self.surface.setBorderColor(&srgb(border.0, border.1));
        self.theme.set(Some(*theme));
        self.paint_cancel(theme);
    }

    /// Shows `line` (an upload's: the first connection's lines have buttons of their own and
    /// stay the dock's), or takes the card down for `None`. `reduce`: Reduce Motion — the card
    /// comes and goes at once and the fill only fades.
    pub(crate) fn show(&self, line: Option<&Transfer>, reduce: bool) {
        let Some(line) = line.filter(|line| line.kind == TransferKind::Upload) else {
            self.hide(reduce);
            return;
        };
        let Some(theme) = self.theme.get() else {
            return;
        };
        self.label.setAttributedStringValue(&words(line, &theme));
        let buttons = line.has_buttons();
        self.cancel.setHidden(!buttons);
        let coming = !self.shown.get();
        let width = self.width_for(buttons, coming);
        self.lay_out(width, buttons);
        let share = line.progress.map(|progress| f64::from(progress) / 10_000.0);
        self.fill
            .follow(self.fill.frame().size, share, inks(&theme), true, reduce);
        if coming {
            self.appear(reduce);
        }
    }

    /// The transfer ended `finish`'s way: the fill plays its end as the tab's does — runs full
    /// and rests for an arrival, turns to the error's colour for a failure, then fades.
    pub(crate) fn end(&self, finish: Finish, reduce: bool) {
        if let Some(theme) = self.theme.get() {
            self.fill.end(finish, reduce, inks(&theme));
        }
    }

    /// The card's width: what the words and the button need, between [`MIN_WIDTH`] and
    /// [`MAX_WIDTH`]; while the card is up it only grows (a number that gains a digit), so its
    /// edge does not wander. The words' need is the label's own, its cell's margins in it: a
    /// width of the bare text left the last word (the bytes' `MB`) no room.
    fn width_for(&self, buttons: bool, coming: bool) -> f64 {
        let words = self.label.fittingSize().width.ceil();
        let button = if buttons {
            self.cancel.fittingSize().width + GAP
        } else {
            0.0
        };
        let needed = (TEXT_LEFT + words + button + PAD_X + 2.0).clamp(MIN_WIDTH, MAX_WIDTH);
        if coming {
            needed
        } else {
            needed.max(self.surface.frame().size.width)
        }
    }

    /// Places the card at the container's bottom right and its parts inside it.
    ///
    /// The parts live in the box's content view, which the box keeps inside its line: each is
    /// placed by the card's outer measure turned into the content's, and centred on the
    /// content's middle — centred on the box's own, they stood a point high (the user saw it).
    fn lay_out(&self, width: f64, buttons: bool) {
        // SAFETY: reading the superview on the main thread; the returned `Retained` keeps it
        // alive for this call. The card is attached in the constructor and never removed.
        let bounds =
            unsafe { self.surface.superview() }.map_or(NSRect::ZERO, |parent| parent.bounds());
        self.surface.setFrame(NSRect::new(
            NSPoint::new((bounds.size.width - INSET - width).max(0.0), INSET),
            NSSize::new(width, HEIGHT),
        ));
        let Some(content) = self.surface.contentView() else {
            return;
        };
        let inner = content.frame();
        // The card's outer x and y in the content's measure.
        let (dx, dy) = (inner.origin.x, inner.origin.y);
        let middle = |height: f64| (HEIGHT / 2.0 - dy - height / 2.0).round();
        // The fill lies inside the line (1 pt), concentric with the card's corner.
        self.fill.setFrame(NSRect::new(
            NSPoint::new(1.0 - dx, 1.0 - dy),
            NSSize::new(width - 2.0, HEIGHT - 2.0),
        ));
        let mut right = width - PAD_X - dx;
        if buttons {
            let size = self.cancel.fittingSize();
            right -= size.width;
            self.cancel
                .setFrame(NSRect::new(NSPoint::new(right, middle(size.height)), size));
            right -= GAP;
        }
        let height = self.label.fittingSize().height;
        let left = TEXT_LEFT - dx;
        self.label.setFrame(NSRect::new(
            NSPoint::new(left, middle(height)),
            NSSize::new((right - left).max(0.0), height),
        ));
    }

    /// The button's title: the verb in the foreground, its key dim after it.
    fn paint_cancel(&self, theme: &Theme) {
        let font = font(12.0);
        let title = NSMutableAttributedString::new();
        for (text, ink) in [
            ("Cancel", Tint::of(theme.foreground, 1.0)),
            ("  ⌘.", Tint::of(theme.foreground, 0.45)),
        ] {
            title.appendAttributedString(&piece(text, &attributes(&font, ink)));
        }
        self.cancel.setAttributedTitle(&title);
    }

    fn appear(&self, reduce: bool) {
        self.shown.set(true);
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let surface = self.surface.clone();
        surface.setHidden(false);
        if reduce {
            surface.setAlphaValue(1.0);
            return;
        }
        surface.setAlphaValue(0.0);
        let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
            // SAFETY: AppKit gives the block a live context, for the block's duration.
            let context = unsafe { context.as_ref() };
            context.setDuration(APPEAR_SECS);
            surface.animator().setAlphaValue(1.0);
        });
        NSAnimationContext::runAnimationGroup(&changes);
    }

    fn hide(&self, reduce: bool) {
        if !self.shown.replace(false) {
            return;
        }
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let surface = self.surface.clone();
        let fill = self.fill.clone();
        if reduce {
            surface.setHidden(true);
            fill.reset();
            return;
        }
        let moving = surface.clone();
        let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
            // SAFETY: AppKit gives the block a live context, for the block's duration.
            let context = unsafe { context.as_ref() };
            context.setDuration(APPEAR_SECS);
            moving.animator().setAlphaValue(0.0);
        });
        let current = Rc::clone(&self.generation);
        let done = RcBlock::new(move || {
            // A line that came in between owns the card now.
            if current.get() == generation {
                surface.setHidden(true);
                fill.reset();
            }
        });
        NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&done));
    }
}

/// The card's font: the system's, medium, with digits of one width so the numbers do not jitter.
fn font(size: f64) -> Retained<NSFont> {
    // SAFETY: a constant AppKit exposes, it lives for the whole process.
    NSFont::monospacedDigitSystemFontOfSize_weight(size, unsafe { NSFontWeightMedium })
}

/// The fill's colours in the theme's: the accent, the error's for a failure. The card's fill
/// has no way still to go (its ground is the card's own), so that tone is never drawn.
fn inks(theme: &Theme) -> FillInks {
    FillInks {
        accent: theme.accent,
        error: theme.error,
        way: Tint::of(theme.foreground, 0.0),
    }
}

/// Text attributes: AppKit's keys to their values.
type Attributes = Retained<NSDictionary<NSAttributedStringKey, AnyObject>>;

/// A font and a colour.
fn attributes(font: &NSFont, color: Tint) -> Attributes {
    let ink = color.color();
    // SAFETY: AppKit's constant attribute keys, alive for the process, each with the value type
    // it documents.
    let keys: [&NSAttributedStringKey; 2] =
        unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
    let values: [&AnyObject; 2] = [font, &ink];
    NSDictionary::from_slices(&keys, &values)
}

fn piece(text: &str, attributes: &Attributes) -> Retained<NSAttributedString> {
    // SAFETY: the attributes are `NSColor`/`NSFont` under AppKit's two keys.
    unsafe { NSAttributedString::new_with_attributes(&NSString::from_str(text), attributes) }
}

/// The line's words as the card says them ([`runs`]), in the theme's colours.
fn words(line: &Transfer, theme: &Theme) -> Retained<NSMutableAttributedString> {
    let font = font(TEXT_SIZE);
    let out = NSMutableAttributedString::new();
    for (text, ink) in runs(line) {
        let color = match ink {
            Ink::Accent => Tint::of(theme.accent, 1.0),
            Ink::Success => Tint::of(theme.success, 1.0),
            Ink::Error => Tint::of(theme.error, 1.0),
            Ink::Plain => Tint::of(theme.foreground, 1.0),
            Ink::Dim => Tint::of(theme.foreground, DIM),
        };
        out.appendAttributedString(&piece(&text, &kept_whole_at_ends(&font, color)));
    }
    out
}

/// The words' attributes: their font and colour, and a line that never wraps and, when it does
/// not fit, gives up its middle — the arrow and the start of the name on the left, the bytes on
/// the right stay.
fn kept_whole_at_ends(font: &NSFont, color: Tint) -> Attributes {
    let paragraph = NSMutableParagraphStyle::new();
    paragraph.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
    let ink = color.color();
    // SAFETY: AppKit's constant attribute keys, alive for the process, each with the value type
    // it documents.
    let keys: [&NSAttributedStringKey; 3] = unsafe {
        [
            NSFontAttributeName,
            NSForegroundColorAttributeName,
            NSParagraphStyleAttributeName,
        ]
    };
    let values: [&AnyObject; 3] = [font, &ink, &paragraph];
    NSDictionary::from_slices(&keys, &values)
}

/// What a run of the card's words is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ink {
    Accent,
    Success,
    Error,
    Plain,
    Dim,
}

/// The card's words in runs: the line up to [`Transfer::short`]. While it flows, its arrow in
/// the accent (the fill's colour), what goes in the foreground, the bytes after the line's
/// two-space gap dim; at its end, the line's lead in its tone (`✓ …` the success's, `Failed —
/// why` the error's, a cancel dim) and the rest dim, as the dock's row draws it.
fn runs(line: &Transfer) -> Vec<(String, Ink)> {
    let chars: Vec<char> = line.body.chars().collect();
    let said = if line.short == 0 {
        chars.len()
    } else {
        line.short.min(chars.len())
    };
    let text: String = chars[..said].iter().collect();
    let mut out = Vec::new();
    let mut push = |text: &str, ink: Ink| {
        if !text.is_empty() {
            out.push((text.to_owned(), ink));
        }
    };
    if line.progress.is_some() {
        let mut rest = text.as_str();
        if let Some(arrow) = rest.chars().next().filter(|c| matches!(c, '↑' | '↓')) {
            push(&rest[..arrow.len_utf8()], Ink::Accent);
            rest = &rest[arrow.len_utf8()..];
        }
        match rest.rfind("  ") {
            Some(gap) => {
                push(&rest[..gap], Ink::Plain);
                push(&rest[gap..], Ink::Dim);
            }
            None => push(rest, Ink::Plain),
        }
        return out;
    }
    let lead: String = text.chars().take(line.lead).collect();
    let tone = match line.tone {
        TransferTone::Success => Ink::Success,
        TransferTone::Error => Ink::Error,
        TransferTone::Plain => Ink::Plain,
        TransferTone::Quiet => Ink::Dim,
    };
    push(&lead, tone);
    push(&text[lead.len()..], Ink::Dim);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(body: &str, short: usize) -> Transfer {
        Transfer {
            body: body.into(),
            short,
            progress: Some(3_500),
            ..Transfer::default()
        }
    }

    fn run(text: &str, ink: Ink) -> (String, Ink) {
        (text.to_owned(), ink)
    }

    #[test]
    fn a_flowing_line_says_what_goes_and_how_much_and_leaves_the_rate_out() {
        let body = "↑ 2 of 3 · rapor.pdf  4.1 / 12.6 MB · 1.2 MB/s · 7s";
        let short = "↑ 2 of 3 · rapor.pdf  4.1 / 12.6 MB".chars().count();
        assert_eq!(
            runs(&running(body, short)),
            [
                run("↑", Ink::Accent),
                run(" 2 of 3 · rapor.pdf", Ink::Plain),
                run("  4.1 / 12.6 MB", Ink::Dim),
            ]
        );
        // A line that says no short form is said whole.
        assert_eq!(
            runs(&running("↑ shot.png  1.0 / 3.4 MB", 0)).concat_text(),
            "↑ shot.png  1.0 / 3.4 MB"
        );
    }

    #[test]
    fn an_ended_line_has_its_lead_in_its_tone_and_the_rest_dim() {
        let line = |body: &str, tone, lead| Transfer {
            body: String::from(body),
            tone,
            lead,
            ..Transfer::default()
        };
        assert_eq!(
            runs(&line("✓ 3 files", TransferTone::Success, 9)),
            [run("✓ 3 files", Ink::Success)]
        );
        let failed = "Failed — disk full on rasp";
        assert_eq!(
            runs(&line(
                "Failed — disk full on rasp · 1 of 3 uploaded",
                TransferTone::Error,
                failed.chars().count()
            )),
            [run(failed, Ink::Error), run(" · 1 of 3 uploaded", Ink::Dim)]
        );
        assert_eq!(
            runs(&line(
                "Cancelled — partial file removed",
                TransferTone::Quiet,
                32
            )),
            [run("Cancelled — partial file removed", Ink::Dim)]
        );
    }

    trait Text {
        fn concat_text(&self) -> String;
    }

    impl Text for Vec<(String, Ink)> {
        fn concat_text(&self) -> String {
            self.iter().map(|(text, _)| text.as_str()).collect()
        }
    }
}
