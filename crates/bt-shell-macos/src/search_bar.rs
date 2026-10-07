//! The history search panel (⌘F): an AppKit surface **floating above**
//! the terminal at the window's top right - an `NSSearchField`, the `Aa` and
//! `.*` toggles, the count label, two arrows and a close button.
//!
//! **Why AppKit**: all
//! the correctness of text entry (dead keys, IME, pasteboard, undo,
//! VoiceOver) comes free with the field; a field drawn in Metal would have to
//! rewrite each of them. The panel does **not push** the content, it rides
//! on top - the PTY size does not change on ⌘F (the dock's "PTY stays fixed" rule).
//!
//! **This file holds looks, not decisions.** Compiling the query, the
//! current match and the window's move to a match live in `bt-core`
//! (`Session::set_search`, `search_next`, `search_reveal`); the owner of the
//! events is the pane (`pane::TerminalPane` is the field's delegate and the
//! buttons' target - both weak references, the pane holds the panel). What
//! remains here: building the views, painting them to the theme, the
//! placement and the open/close animation, reading and writing the toggles
//! and the label.
//!
//! State is **per tab**: when the panel closes it is hidden, the
//! query and the two toggles stay in the field and come back selected on the
//! next ⌘F. It is not written to the settings file.

use std::cell::Cell;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use bt_core::{SearchQuery, SearchReport, SearchStatus, Theme, escape_search};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSAutoresizingMaskOptions, NSBezelStyle,
    NSBox, NSBoxType, NSButton, NSButtonType, NSColor, NSControlSize, NSControlStateValueOff,
    NSControlStateValueOn, NSFont, NSFontWeightRegular, NSImage, NSSearchField,
    NSSearchFieldDelegate, NSShadow, NSStackView, NSTextField, NSTitlePosition,
    NSUserInterfaceLayoutOrientation, NSView, NSWindowOrderingMode,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString, ns_string};
use objc2_quartz_core::{
    CAMediaTimingFunction, kCAMediaTimingFunctionEaseIn, kCAMediaTimingFunctionEaseOut,
};

/// The panel's distance from the window's inner edges, in points. A design
/// constant: enough to breathe under the title bar and not stick to the right edge.
const INSET: f64 = 10.0;

/// The padding inside the surface (horizontal, vertical), in points - the
/// vertical padding is narrower than the horizontal so the field's rounded
/// corner stays concentric with the surface's corner.
const PADDING: (f64, f64) = (7.0, 6.0);

/// The surface's corner radius: close to the sum of the field's (≈ 6 pt) and
/// the padding's (≈ 6 pt), so the inner and outer corners read as concentric.
const RADIUS: f64 = 11.0;

/// The search field's width, in points. Enough to fit a long path or pattern
/// without covering the terminal's top right corner more than necessary.
const FIELD_WIDTH: f64 = 190.0;

/// The count label's **fixed** width: "Invalid pattern" and "999 of 9999…" fit,
/// so the panel width does not jump while the count progresses. A longer
/// count (five digits) is clipped from its tail.
const COUNT_WIDTH: f64 = 96.0;

/// The fixed width of the toggles and icon buttons, in points: the bezel's
/// default inner padding made a one- or two-letter title look wider than needed.
const BUTTON_WIDTH: f64 = 24.0;

/// The duration of the open and close, in seconds. It started from the
/// 240 ms floor found by eye for the dock's typing effects and was shortened
/// in the real window: the panel is small and its motion short, and
/// at 240 ms the eye that had already started typing still saw the area
/// settling into place.
const APPEAR_SECS: f64 = 0.18;

/// The distance the panel descends from above while appearing, in points -
/// enough for a "settling into place" feel, short enough not to read as a motion.
const SLIDE: f64 = 6.0;

/// The panel's views and the animation's generation.
pub(crate) struct SearchBar {
    surface: Retained<NSBox>,
    field: Retained<NSSearchField>,
    case: Retained<NSButton>,
    regex: Retained<NSButton>,
    count: Retained<NSTextField>,
    /// Whether the panel is **open** (`false` while the close animation runs too).
    shown: Cell<bool>,
    /// Increases on every open and close: the close animation's completion
    /// block hides the panel only if its own generation is still current - an
    /// intervening ⌘F must not hide it.
    generation: Rc<Cell<u64>>,
    /// The query last given to the session - so the same query does not cause
    /// a second `set_search` (the field's action and the toggles can send the
    /// same query again, and every `set_search` is a frame).
    applied: std::cell::RefCell<Option<SearchQuery>>,
}

impl SearchBar {
    /// Builds the panel and adds it to `parent`, **above** `below`; it is born hidden.
    ///
    /// `target` is the action target of the buttons and the field, `delegate`
    /// the field's delegate - both are the pane and both are held weakly. The
    /// container is **not held** either: the container is the pane and the pane
    /// holds the panel - a back reference would be a cycle, and the pane (with
    /// its renderer and link) would never drop. The placement's measure comes
    /// from the surface's own `superview`.
    pub(crate) fn new(
        mtm: MainThreadMarker,
        parent: &NSView,
        below: &NSView,
        target: &AnyObject,
        delegate: &ProtocolObject<dyn NSSearchFieldDelegate>,
    ) -> Self {
        let field = NSSearchField::new(mtm);
        field.setPlaceholderString(Some(ns_string!("Find")));
        // Every change sends the action immediately: the highlight follows
        // typing and the field's ⊗ button goes the same way (clearing
        // the text does not cause `controlTextDidChange:`).
        field.setSendsSearchStringImmediately(true);
        field.setSendsWholeSearchString(false);
        // SAFETY: the target is weak and lives as long as the pane keeps the
        // panel alive; the selector is an action on the pane with a single
        // `Option<&AnyObject>` argument.
        unsafe {
            field.setTarget(Some(target));
            field.setAction(Some(sel!(searchFieldChanged:)));
            field.setDelegate(Some(delegate));
        }
        width(&field, FIELD_WIDTH);

        let case = toggle(mtm, "Aa", "Match Case", target);
        let regex = toggle(mtm, ".*", "Use Regular Expression", target);

        let count = NSTextField::labelWithString(ns_string!(""), mtm);
        count.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
            NSFont::smallSystemFontSize(),
            // SAFETY: a constant AppKit exposes, it lives for the whole process.
            unsafe { NSFontWeightRegular },
        )));
        count.setTextColor(Some(&NSColor::secondaryLabelColor()));
        width(&count, COUNT_WIDTH);

        // ⏎ = up, older: the up arrow is ⌘G's action, the down arrow
        // ⇧⌘G's - the same selectors as the menu.
        let older = symbol(
            mtm,
            "chevron.up",
            "Find Next (Older)",
            target,
            sel!(findNextMatch:),
        );
        let newer = symbol(
            mtm,
            "chevron.down",
            "Find Previous (Newer)",
            target,
            sel!(findPreviousMatch:),
        );
        let close = symbol(mtm, "xmark", "Close", target, sel!(closeSearch:));

        let views: [&NSView; 7] = [&field, &case, &regex, &count, &older, &newer, &close];
        let stack = NSStackView::stackViewWithViews(&NSArray::from_slice(&views), mtm);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        stack.setSpacing(2.0);
        // The field and the toggles are one group, the count and arrows a
        // second, the close button a third: the gap between groups is wider than within.
        stack.setCustomSpacing_afterView(6.0, &field);
        stack.setCustomSpacing_afterView(8.0, &regex);
        stack.setCustomSpacing_afterView(4.0, &count);
        stack.setCustomSpacing_afterView(6.0, &newer);

        let surface = NSBox::new(mtm);
        // **Its own layer is required**: a sibling of the container is the
        // terminal view hosting Metal's layer (layer-hosting), and a layerless
        // `NSBox`'s fill, border and shadow were never drawn in that hierarchy -
        // the field and buttons showed, the surface did not, and the controls
        // rode over the terminal text (the user saw it, measured in the real
        // window).
        surface.setWantsLayer(true);
        surface.setBoxType(NSBoxType::Custom);
        surface.setTitlePosition(NSTitlePosition::NoTitle);
        surface.setCornerRadius(RADIUS);
        surface.setBorderWidth(1.0);
        surface.setContentViewMargins(NSSize::new(PADDING.0, PADDING.1));
        surface.setContentView(Some(&stack));
        let shadow = NSShadow::new();
        shadow.setShadowBlurRadius(14.0);
        shadow.setShadowOffset(NSSize::new(0.0, -3.0));
        shadow.setShadowColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
            0.0, 0.0, 0.0, 0.28,
        )));
        surface.setShadow(Some(&shadow));
        let fit = stack.fittingSize();
        surface.setFrameSize(NSSize::new(
            fit.width + 2.0 * PADDING.0 + 2.0,
            fit.height + 2.0 * PADDING.1 + 2.0,
        ));
        // The container is not flipped (y up): sticking to the top right
        // corner = the left and bottom margins are flexible.
        surface.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewMinXMargin | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        surface.setHidden(true);
        parent.addSubview_positioned_relativeTo(&surface, NSWindowOrderingMode::Above, Some(below));

        SearchBar {
            surface,
            field,
            case,
            regex,
            count,
            shown: Cell::new(false),
            generation: Rc::new(Cell::new(0)),
            applied: std::cell::RefCell::new(None),
        }
    }

    /// Whether the panel is open.
    pub(crate) fn is_shown(&self) -> bool {
        self.shown.get()
    }

    /// The search field - the view to make first responder.
    pub(crate) fn field(&self) -> &NSSearchField {
        &self.field
    }

    /// The frame where the panel **rests**, in the container's coordinates:
    /// the top right corner, with the inset. While the animation runs the
    /// view's own frame is on its way; the covered cells must be asked from
    /// the destination.
    pub(crate) fn resting_frame(&self) -> NSRect {
        // SAFETY: reading the superview; the returned `Retained` keeps it
        // alive for this call and we are on the main thread. The surface is
        // attached to the container in the constructor and never removed; the
        // `None` arm is only defensive.
        let bounds =
            unsafe { self.surface.superview() }.map_or(NSRect::ZERO, |parent| parent.bounds());
        let size = self.surface.frame().size;
        let origin = NSPoint::new(
            (bounds.size.width - INSET - size.width).max(0.0),
            bounds.size.height - INSET - size.height,
        );
        NSRect::new(origin, size)
    }

    /// Opens the panel: places it at the top right and shows it (unless
    /// Reduce Motion) with a short fade + descent. No-op if already open.
    pub(crate) fn show(&self, animate: bool) {
        if self.shown.replace(true) {
            return;
        }
        self.generation.set(self.generation.get().wrapping_add(1));
        let place = self.resting_frame().origin;
        let surface = self.surface.clone();
        surface.setHidden(false);
        if !animate {
            surface.setAlphaValue(1.0);
            surface.setFrameOrigin(place);
            return;
        }
        surface.setAlphaValue(0.0);
        surface.setFrameOrigin(NSPoint::new(place.x, place.y + SLIDE));
        let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
            // SAFETY: AppKit gives the block a live context, for the block's duration.
            let context = unsafe { context.as_ref() };
            context.setDuration(APPEAR_SECS);
            // SAFETY: a constant name QuartzCore exposes, it lives for the whole process.
            let curve = unsafe { kCAMediaTimingFunctionEaseOut };
            context.setTimingFunction(Some(&CAMediaTimingFunction::functionWithName(curve)));
            let animator = surface.animator();
            animator.setAlphaValue(1.0);
            animator.setFrameOrigin(place);
        });
        NSAnimationContext::runAnimationGroup(&changes);
    }

    /// Closes the panel: hides it with a short fade + rise (instantly under
    /// Reduce Motion). No-op if closed.
    pub(crate) fn hide(&self, animate: bool) {
        if !self.shown.replace(false) {
            return;
        }
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let surface = self.surface.clone();
        if !animate {
            surface.setHidden(true);
            return;
        }
        let origin = surface.frame().origin;
        let moving = surface.clone();
        let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
            // SAFETY: AppKit gives the block a live context, for the block's duration.
            let context = unsafe { context.as_ref() };
            context.setDuration(APPEAR_SECS);
            // SAFETY: a constant name QuartzCore exposes, it lives for the whole process.
            let curve = unsafe { kCAMediaTimingFunctionEaseIn };
            context.setTimingFunction(Some(&CAMediaTimingFunction::functionWithName(curve)));
            let animator = moving.animator();
            animator.setAlphaValue(0.0);
            animator.setFrameOrigin(NSPoint::new(origin.x, origin.y + SLIDE));
        });
        let current = Rc::clone(&self.generation);
        let done = RcBlock::new(move || {
            // If an open came in between, the panel belongs to it.
            if current.get() == generation {
                surface.setHidden(true);
                surface.setFrameOrigin(origin);
            }
        });
        NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&done));
    }

    /// The state of the field and the two toggles - the query to go to the session.
    pub(crate) fn query(&self) -> SearchQuery {
        SearchQuery {
            text: self.field.stringValue().to_string(),
            regex: self.regex.state() == NSControlStateValueOn,
            case_sensitive: self.case.state() == NSControlStateValueOn,
        }
    }

    /// Writes the field's text (⌘E, the find pasteboard).
    pub(crate) fn set_text(&self, text: &str) {
        self.field.setStringValue(&NSString::from_str(text));
    }

    /// Whether the regex toggle is on - ⌘E's escaping decision.
    pub(crate) fn regex(&self) -> bool {
        self.regex.state() == NSControlStateValueOn
    }

    /// If `query` differs from the query last given to the session, records it
    /// and returns `true` - only then does the caller call `set_search`.
    pub(crate) fn take_change(&self, query: &SearchQuery) -> bool {
        let mut applied = self.applied.borrow_mut();
        if applied.as_ref() == Some(query) {
            return false;
        }
        *applied = Some(query.clone());
        true
    }

    /// Forgets the query given to the session: when the panel closes the
    /// search closes too, and reopening must apply the same query again.
    pub(crate) fn forget_applied(&self) {
        self.applied.borrow_mut().take();
    }

    /// Writes the count label.
    pub(crate) fn set_count(&self, status: SearchStatus, report: SearchReport) {
        self.count
            .setStringValue(&NSString::from_str(&count_label(status, report)));
    }

    /// Paints the surface to the theme: the fill is a step raised from the
    /// theme's background, the border a faint trace of the foreground. The
    /// field itself is a system control and its appearance comes from the
    /// window (`apply_chrome`'s Aqua/DarkAqua).
    pub(crate) fn paint(&self, theme: &Theme, dark: bool) {
        let (fill, border) = surface_colors(theme.background, theme.foreground, dark);
        self.surface.setFillColor(&srgb(fill, 1.0));
        self.surface.setBorderColor(&srgb(border.0, border.1));
    }
}

/// The label's text. Empty for an empty query; "Invalid
/// pattern" for an invalid pattern; "No matches" if there is no match;
/// otherwise the count over the whole scrollback - "3 of 17", or "17
/// matches" if the current match's ordinal is not yet known. While the count
/// is in progress (the index advances chunk by chunk or the scrollback
/// changed) a trailing "…": the number is what has been counted so far.
pub(crate) fn count_label(status: SearchStatus, report: SearchReport) -> String {
    let more = if report.complete { "" } else { "…" };
    match status {
        SearchStatus::Empty => String::new(),
        SearchStatus::Invalid => "Invalid pattern".to_owned(),
        SearchStatus::Ready if !report.found => "No matches".to_owned(),
        SearchStatus::Ready => match (report.ordinal, report.total) {
            (_, 0) if !report.complete => "…".to_owned(),
            (Some(ordinal), total) if ordinal <= total => {
                format!("{ordinal} of {total}{more}")
            }
            (_, 1) => format!("1 match{more}"),
            (_, total) => format!("{total} matches{more}"),
        },
    }
}

/// ⌘E's query: the selection's **first line** - search does not
/// cross a hard line break, so later lines could never match anything - and,
/// in regex mode, its escaped form, so the selected text matches itself
/// literally. `None` for a first line that is empty or **only whitespace**:
/// an inkless match is neither highlighted nor counted (`search::has_ink`),
/// so such a query would fill the field with invisible text and show "no
/// matches". The find pasteboard's text ([`crate::window`]) passes through the
/// same filter - another application's ⌘E can leave whitespace there (the
/// user saw it).
pub(crate) fn selection_query(selection: &str, regex: bool) -> Option<String> {
    let line = selection.lines().next().unwrap_or_default();
    if line.trim().is_empty() {
        return None;
    }
    Some(if regex {
        escape_search(line)
    } else {
        line.to_owned()
    })
}

/// The surface's two colours, sRGB: the fill and (border, alpha).
///
/// The fill is a blend of the theme's background toward the foreground -
/// light in a dark theme, dark in a light theme; not a separate theme role
/// (`Theme`'s "no undrawn roles are added" rule). The ratios are a design
/// constant and **enough to read as a surface apart from the terminal**: the
/// first values (0.11 / 0.045) made the panel's fill invisible on a pure
/// black background and the controls read like terminal text (the user saw
/// it). A test holds the lower bound.
fn surface_colors(background: u32, foreground: u32, dark: bool) -> (u32, (u32, f64)) {
    let (lift, edge) = if dark { (0.20, 0.30) } else { (0.08, 0.24) };
    (mix(background, foreground, lift), (foreground, edge))
}

/// `t` of the way from `a` to `b`, in sRGB bytes.
fn mix(a: u32, b: u32, t: f64) -> u32 {
    let channel = |shift: u32| {
        let (x, y) = (
            f64::from((a >> shift) & 0xff),
            f64::from((b >> shift) & 0xff),
        );
        ((x + (y - x) * t).round() as u32).min(0xff) << shift
    };
    channel(16) | channel(8) | channel(0)
}

fn srgb(color: u32, alpha: f64) -> Retained<NSColor> {
    let byte = |shift: u32| f64::from((color >> shift) & 0xff) / 255.0;
    NSColor::colorWithSRGBRed_green_blue_alpha(byte(16), byte(8), byte(0), alpha)
}

fn width(view: &NSView, points: f64) {
    view.widthAnchor()
        .constraintEqualToConstant(points)
        .setActive(true);
}

/// A small two-state toggle (`Aa`, `.*`): its bezel is **always** visible,
/// filled when on. The version that was only framed under the mouse read
/// like terminal text, not a button (the user saw it) - that the toggle has
/// two states is only understood from a visible surface.
fn toggle(mtm: MainThreadMarker, title: &str, tip: &str, target: &AnyObject) -> Retained<NSButton> {
    // SAFETY: the target is weak and lives as long as the window keeps the
    // panel alive; the selector is an action on the window with a single
    // `Option<&AnyObject>` argument.
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(sel!(searchOptionsChanged:)),
            mtm,
        )
    };
    button.setButtonType(NSButtonType::PushOnPushOff);
    width(&button, BUTTON_WIDTH);
    button.setBezelStyle(NSBezelStyle::AccessoryBar);
    button.setShowsBorderOnlyWhileMouseInside(false);
    button.setControlSize(NSControlSize::Small);
    button.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(
        NSFont::smallSystemFontSize(),
        // SAFETY: a constant AppKit exposes, it lives for the whole process.
        unsafe { NSFontWeightRegular },
    )));
    button.setState(NSControlStateValueOff);
    button.setToolTip(Some(&NSString::from_str(tip)));
    button
}

/// A frameless button with an SF Symbol (arrows, close): a frame on hover.
fn symbol(
    mtm: MainThreadMarker,
    name: &str,
    tip: &str,
    target: &AnyObject,
    action: Sel,
) -> Retained<NSButton> {
    let tip = NSString::from_str(tip);
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(name),
        Some(&tip),
    )
    .unwrap_or_default();
    // SAFETY: the target is weak and lives as long as the window keeps the
    // panel alive; the selector is an action on the window with a single
    // `Option<&AnyObject>` argument.
    let button =
        unsafe { NSButton::buttonWithImage_target_action(&image, Some(target), Some(action), mtm) };
    button.setBezelStyle(NSBezelStyle::AccessoryBar);
    button.setShowsBorderOnlyWhileMouseInside(true);
    button.setControlSize(NSControlSize::Small);
    button.setToolTip(Some(&tip));
    width(&button, BUTTON_WIDTH);
    button
}

#[cfg(test)]
mod tests {
    use super::{count_label, mix, selection_query, surface_colors};
    use bt_core::{SearchReport, SearchStatus, Theme};

    #[test]
    fn the_label_says_what_the_query_found() {
        let report = |found, total, ordinal, complete| SearchReport {
            found,
            total,
            ordinal,
            complete,
            marks_changed: false,
        };
        let ready = |r| count_label(SearchStatus::Ready, r);
        assert_eq!(
            count_label(SearchStatus::Empty, report(false, 0, None, true)),
            ""
        );
        assert_eq!(
            count_label(SearchStatus::Invalid, report(false, 0, None, true)),
            "Invalid pattern"
        );
        assert_eq!(ready(report(false, 0, None, true)), "No matches");
        assert_eq!(ready(report(true, 17, Some(3), true)), "3 of 17");
        assert_eq!(ready(report(true, 17, Some(3), false)), "3 of 17…");
        assert_eq!(ready(report(true, 17, None, true)), "17 matches");
        assert_eq!(ready(report(true, 1, None, true)), "1 match");
        assert_eq!(ready(report(true, 40, None, false)), "40 matches…");
        assert_eq!(ready(report(true, 0, None, false)), "…");
    }

    #[test]
    fn use_selection_escapes_in_regex_mode_and_keeps_the_first_line() {
        assert_eq!(selection_query("a.b(c)", false).as_deref(), Some("a.b(c)"));
        assert_eq!(
            selection_query("a.b(c)", true).as_deref(),
            Some("a\\.b\\(c\\)"),
            "in regex mode the selected text must match itself literally"
        );
        assert_eq!(
            selection_query("first\nsecond", false).as_deref(),
            Some("first")
        );
        assert_eq!(selection_query("", true), None);
        assert_eq!(selection_query("\nx", false), None);
        // Whitespace only: `"     \n"` turned up in the find pasteboard and the
        // field opened with it.
        assert_eq!(selection_query("     \n", false), None);
        assert_eq!(selection_query(" \t ", true), None);
        // A query with whitespace inside stays as it is.
        assert_eq!(selection_query(" a b ", false).as_deref(), Some(" a b "));
    }

    #[test]
    fn the_surface_steps_toward_the_foreground() {
        assert_eq!(mix(0x000000, 0xffffff, 0.0), 0x000000);
        assert_eq!(mix(0x000000, 0xffffff, 1.0), 0xffffff);
        let (dark, _) = surface_colors(0x000000, 0xe6e6e6, true);
        assert!(dark > 0x000000 && dark < 0x404040, "{dark:06x}");
        let (light, _) = surface_colors(0xffffff, 0x1a1a1a, false);
        assert!(light < 0xffffff && light > 0xe0e0e0, "{light:06x}");
    }

    #[test]
    fn the_surface_stands_apart_from_the_terminal() {
        // The panel floats above the terminal; if its fill does not stand
        // apart from the terminal's, the controls read like terminal text
        // (the user saw it: the 0.11 step was ≈ #191919 on pure black). The
        // lower bounds are a design constant, by WCAG ratio: a clear step in
        // the dark theme, as much as Safari's find bar in the light theme.
        for (theme, dark, floor) in [
            (Theme::BATERI, true, 1.4),
            (Theme::BATERI_LIGHT, false, 1.12),
        ] {
            let (fill, _) = surface_colors(theme.background, theme.foreground, dark);
            let ratio = contrast(fill, theme.background);
            assert!(
                ratio >= floor,
                "{fill:06x} / {:06x}: {ratio:.2}",
                theme.background
            );
        }
    }

    /// The WCAG contrast ratio, between two `0xRRGGBB` values — the
    /// production measure, not a copy of it.
    fn contrast(a: u32, b: u32) -> f64 {
        bt_core::contrast_ratio(a, b)
    }
}
