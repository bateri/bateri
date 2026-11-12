//! The system's drag of what a view of the kit hands over (`pencere_drive::Host::carry`): the
//! words or the path of something the person pulls out of a page (a file of textpen, an issue of
//! linearpen), to a terminal pane beside it or to another application. It is the person's own act,
//! as a copy is, and held to the rule a copy is held to.
//!
//! **The drag begins in the event that moved the pointer**: the drive says it in the `input` of the
//! `mouseDragged:` it was told, the pane drains the page in that same event, and the session is
//! begun with that event (`beginDraggingSessionWithItems:event:source:` takes no other). A press
//! that cannot begin one (the event being handled is not a left drag) is told to the page as a drag
//! that was not done, or the page would wait for an end that never comes.
//!
//! **What a terminal pane is handed is the process's own, not the pasteboard's**: the pasteboard
//! type [`carry_type`] is a name anyone can write, so a pane takes a drop only where AppKit says
//! its source is a [`CarrySource`] of this process (AppKit gives the source object inside the
//! process alone: [`taken`], the pattern of `tab_drag::carried`), and takes the words from that
//! source, not from what the pasteboard holds under the type. The words are held to the rule once
//! more where they are taken ([`bytes_of`], `pencere_drive::carriable`). Another application gets
//! the pasteboard's plain words and, if the element said one, its address.
//!
//! **The picture is the carry's own card** (a small card of the pane's card look, one line of what
//! is carried, the end of a long line faded): what the person sees is the words that will be
//! entered, not a label the page chose. It is drawn here and not by `pane_drag`'s card, which a
//! pane's module does not reach (`make audit`); the card's corner and edge are that card's.

use std::cell::RefCell;

use block2::RcBlock;
use bt_shell_common::native_view::{Carry, CarryOut, Payload, carriable};
use bt_shell_common::quote::shell_quote;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
};
use objc2_app_kit::{
    NSApplication, NSBezierPath, NSColor, NSDragOperation, NSDraggingContext, NSDraggingInfo,
    NSDraggingItem, NSDraggingSession, NSDraggingSource, NSEventType, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSImage, NSPasteboardItem, NSPasteboardTypeString,
    NSPasteboardTypeURL, NSStringDrawing, NSView,
};
use objc2_foundation::{
    NSArray, NSAttributedStringKey, NSDictionary, NSObject, NSObjectProtocol, NSPoint, NSRect,
    NSSize, NSString, ns_string,
};

use crate::pane::TerminalPane;

/// How far the card stands from the pointer, right and below it.
const CARD_OFFSET: f64 = 14.0;
/// The card's height, the room its words keep each side, its widest and its narrowest, and the
/// stretch at its end where a long line fades.
const CARD_HEIGHT: f64 = 30.0;
const CARD_PAD: f64 = 12.0;
const CARD_WIDEST: f64 = 320.0;
const CARD_NARROWEST: f64 = 96.0;
const CARD_FADE: f64 = 36.0;
/// The card's corner, as the pane's card has it.
const CARD_CORNER: f64 = 9.0;
/// How much of a long line the card measures and draws: it is faded away long before, and 64 KiB of
/// words is not laid out to show a few dozen characters.
const SHOWN_CHARS: usize = 200;

/// The pasteboard type a bateri's carry is written as: its content is the kind of what is carried
/// ([`kind_word`]), the cross-check of the source and not the proof ([`taken`]).
pub(crate) fn carry_type() -> &'static NSString {
    ns_string!("dev.bateri.carry")
}

/// The word the pasteboard's type holds for a carry: what it is (`text`, or `path`, which a terminal
/// escapes where it is entered).
fn kind_word(carry: &Carry) -> &'static str {
    match carry.payload {
        Payload::Text(_) => "text",
        Payload::Path(_) => "path",
    }
}

/// What a terminal pane enters for `carry`: the words as they are for text, escaped as a file
/// dropped from Finder is for a path ([`shell_quote`]), and nothing for what breaks the rule of a
/// carry (one line, no control character, at most 64 KiB), which was held to it twice already. One
/// line cannot be a second command.
pub(crate) fn bytes_of(carry: &Carry) -> Option<Vec<u8>> {
    if !carriable(carry) {
        return None;
    }
    Some(match &carry.payload {
        Payload::Text(words) => words.clone().into_bytes(),
        Payload::Path(path) => shell_quote(std::slice::from_ref(path)).into_bytes(),
    })
}

/// The part of `words` the card draws.
fn shown(words: &str) -> &str {
    words
        .char_indices()
        .nth(SHOWN_CHARS)
        .map_or(words, |(end, _)| &words[..end])
}

/// The card's width for words `measured` points wide.
fn card_width(measured: f64) -> f64 {
    (measured + 2.0 * CARD_PAD).clamp(CARD_NARROWEST, CARD_WIDEST)
}

/// The colours of the pane's card: its ground and its ink, the theme's own.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Look {
    pub ground: u32,
    pub ink: u32,
}

fn color(rgb: u32, alpha: f64) -> Retained<NSColor> {
    let byte = |shift: u32| f64::from((rgb >> shift) & 0xff) / 255.0;
    NSColor::colorWithSRGBRed_green_blue_alpha(byte(16), byte(8), byte(0), alpha)
}

/// The card for `words`, and its size in points: a rounded card in the ground, with an edge of the
/// ink, the words on one line in the ink and the end of a line that does not fit faded into the
/// ground. A vector drawing, so it is sharp on every screen the pointer takes it to.
fn card(words: &str, look: Look) -> (Retained<NSImage>, NSSize) {
    let word = NSString::from_str(shown(words));
    let font = NSFont::systemFontOfSize(12.0);
    let ink = color(look.ink, 1.0);
    let values: [&AnyObject; 2] = [font.as_ref(), ink.as_ref()];
    // SAFETY: AppKit's font and colour attribute keys (extern statics) with the `NSFont` and
    // `NSColor` they document.
    let attributes = unsafe {
        NSDictionary::<NSAttributedStringKey, AnyObject>::from_slices(
            &[NSFontAttributeName, NSForegroundColorAttributeName],
            &values,
        )
    };
    // SAFETY: the dictionary holds a font and a colour under their keys.
    let line = unsafe { word.sizeWithAttributes(Some(&attributes)) };
    let size = NSSize::new(card_width(line.width), CARD_HEIGHT);
    let fits = line.width <= size.width - 2.0 * CARD_PAD;
    let draw = RcBlock::new(move |_: NSRect| -> Bool {
        let bounds = NSRect::new(NSPoint::ZERO, size);
        let shape = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            bounds,
            CARD_CORNER,
            CARD_CORNER,
        );
        color(look.ground, 0.97).setFill();
        shape.fill();
        // SAFETY: the same dictionary; drawing happens inside the image's handler.
        unsafe {
            word.drawAtPoint_withAttributes(
                NSPoint::new(CARD_PAD, ((size.height - line.height) / 2.0).floor()),
                Some(&attributes),
            );
        }
        if !fits {
            // The end of a line that does not fit goes into the ground, a strip at a time.
            const STRIPS: usize = 18;
            let strip = CARD_FADE / STRIPS as f64;
            let from = size.width - CARD_FADE;
            for at in 0..STRIPS {
                let alpha = 0.97 * (at as f64 + 1.0) / STRIPS as f64;
                color(look.ground, alpha).setFill();
                NSBezierPath::fillRect(NSRect::new(
                    NSPoint::new(from + at as f64 * strip, 1.0),
                    NSSize::new(strip + 0.5, size.height - 2.0),
                ));
            }
        }
        let edge = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            NSRect::new(
                NSPoint::new(0.5, 0.5),
                NSSize::new(size.width - 1.0, size.height - 1.0),
            ),
            CARD_CORNER - 0.5,
            CARD_CORNER - 0.5,
        );
        edge.setLineWidth(1.0);
        color(look.ink, 0.2).setStroke();
        edge.stroke();
        Bool::YES
    });
    (
        NSImage::imageWithSize_flipped_drawingHandler(size, true, &draw),
        size,
    )
}

pub(crate) struct SourceIvars {
    /// What is carried: the proof of a drop in a terminal pane (the pasteboard's is not).
    carry: Carry,
    /// The pane the drag began in, whose page is told it is over.
    pane: Weak<TerminalPane>,
}

define_class!(
    // SAFETY: NSObject subclassing has no requirements; `CarrySource` implements no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriCarrySource"]
    #[ivars = SourceIvars]
    pub(crate) struct CarrySource;

    unsafe impl NSObjectProtocol for CarrySource {}

    unsafe impl NSDraggingSource for CarrySource {
        /// A copy, inside the application and out of it: what is carried stays where it was.
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn source_operation_mask(
            &self,
            _session: &NSDraggingSession,
            _context: NSDraggingContext,
        ) -> NSDragOperation {
            NSDragOperation::Copy
        }

        /// The drag is over, taken where some operation was done and not where it was let go on
        /// nothing or taken back with Esc: the page of the pane is told, and hears the pointer
        /// again.
        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn session_ended(
            &self,
            _session: &NSDraggingSession,
            _at: NSPoint,
            operation: NSDragOperation,
        ) {
            let done = operation != NSDragOperation::None;
            if let Some(pane) = self.ivars().pane.load() {
                pane.carry_ended(done);
            }
        }
    }
);

impl CarrySource {
    fn new(mtm: MainThreadMarker, carry: Carry, pane: &TerminalPane) -> Retained<Self> {
        let pane = Weak::from_retained(&pane.retain());
        let this = Self::alloc(mtm).set_ivars(SourceIvars { carry, pane });
        // SAFETY: NSObject's init takes no arguments and the ivars are set.
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    /// The source of the drag that goes, kept until the next one replaces it: AppKit's hold on it is
    /// not something to lean on (`tab_drag::begin`), and a source let go of in its own callback
    /// would be gone before the callback returned. The system has one drag at a time.
    static SOURCE: RefCell<Option<Retained<CarrySource>>> = const { RefCell::new(None) };
}

/// What the drag `info` carries, `None` for anything that is not a carry of this process
/// ([the module header](self)): its source is a [`CarrySource`], and the type on the pasteboard
/// names the kind of what the source holds.
pub(crate) fn taken(info: &ProtocolObject<dyn NSDraggingInfo>) -> Option<Carry> {
    let source = info.draggingSource()?.downcast::<CarrySource>().ok()?;
    let word = info
        .draggingPasteboard()
        .stringForType(carry_type())?
        .to_string();
    let carry = &source.ivars().carry;
    (word == kind_word(carry)).then(|| carry.clone())
}

/// Begins the system's drag of `out` from `view`, under the pointer the event being handled reports:
/// the drive said it in the `input` of a left drag, and a session is begun with that event alone.
/// `false` where it cannot be begun (the event is not the person's left drag, the pane has no
/// window): the page is to be told the drag is not done.
pub(crate) fn begin(pane: &TerminalPane, out: &CarryOut, look: Look) -> bool {
    let view: &NSView = pane.view();
    let mtm = view.mtm();
    let Some(event) = NSApplication::sharedApplication(mtm).currentEvent() else {
        return false;
    };
    if event.r#type() != NSEventType::LeftMouseDragged || view.window().is_none() {
        return false;
    }
    let carry = out.carry.clone();
    let words = carry.words().to_owned();
    let item = NSPasteboardItem::new();
    // SAFETY: AppKit's pasteboard type constants (extern statics).
    unsafe {
        item.setString_forType(&NSString::from_str(&words), NSPasteboardTypeString);
        if let Some(url) = &carry.url {
            item.setString_forType(&NSString::from_str(url), NSPasteboardTypeURL);
        }
    }
    item.setString_forType(&NSString::from_str(kind_word(&carry)), carry_type());
    let dragged = NSDraggingItem::initWithPasteboardWriter(
        NSDraggingItem::alloc(),
        ProtocolObject::from_ref(&*item),
    );
    let (image, size) = card(&words, look);
    let at = view.convertPoint_fromView(event.locationInWindow(), None);
    let y = if view.isFlipped() {
        at.y + CARD_OFFSET
    } else {
        at.y - CARD_OFFSET - size.height
    };
    let frame = NSRect::new(NSPoint::new(at.x + CARD_OFFSET, y), size);
    // SAFETY: an `NSImage` is a documented dragging frame content.
    unsafe { dragged.setDraggingFrame_contents(frame, Some(image.as_ref())) };
    let source = CarrySource::new(mtm, carry, pane);
    SOURCE.with_borrow_mut(|held| *held = Some(source.clone()));
    let session = view.beginDraggingSessionWithItems_event_source(
        &NSArray::from_retained_slice(&[dragged]),
        &event,
        ProtocolObject::from_ref(&*source),
    );
    session.setAnimatesToStartingPositionsOnCancelOrFail(false);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a terminal is handed for a carry: words as they are, a path as a dropped file's is
    /// (escaped, so that one word it stays), nothing for what breaks the rule, and never a line
    /// end, which would be a second command.
    #[test]
    fn a_carry_is_entered_as_words_or_as_an_escaped_path_and_never_as_two_lines() {
        let words = Carry::text("ACM-412 Fix the login redirect https://linear.app/acm/issue/412");
        assert_eq!(
            bytes_of(&words).as_deref(),
            Some(&b"ACM-412 Fix the login redirect https://linear.app/acm/issue/412"[..]),
            "text as it is"
        );
        let path = Carry::path("/Users/ayşe/Belgeler/iki kelime (1).txt");
        assert_eq!(
            bytes_of(&path).as_deref(),
            Some("/Users/ayşe/Belgeler/iki\\ kelime\\ \\(1\\).txt".as_bytes()),
            "a path escaped as Finder's is, a non-ASCII letter untouched"
        );
        assert_eq!(
            bytes_of(&Carry::path("/tmp/a b")),
            bytes_of(&Carry::text("/tmp/a\\ b")),
            "the same bytes as the words that are already escaped"
        );
        for broken in [
            "",
            "two\nlines",
            "carriage\rreturn",
            "tab\there",
            "escape\u{1b}[201~",
            "end\u{3}of text",
            "separator\u{2028}line",
            "bidi\u{202e}override",
        ] {
            assert_eq!(bytes_of(&Carry::text(broken)), None, "{broken:?}");
            assert_eq!(bytes_of(&Carry::path(broken)), None, "{broken:?}");
        }
        assert_eq!(
            bytes_of(&Carry::text("a".repeat(64 << 10))).map(|b| b.len()),
            Some(64 << 10)
        );
        assert_eq!(bytes_of(&Carry::text("a".repeat((64 << 10) + 1))), None);
        // An address that is no link's refuses the whole of it, whatever the words.
        assert_eq!(
            bytes_of(&Carry::text("words").url("javascript:alert(1)")),
            None
        );
        for bytes in [&words, &path].iter().filter_map(|carry| bytes_of(carry)) {
            assert!(!bytes.contains(&b'\n') && !bytes.contains(&b'\r'));
        }
    }

    #[test]
    fn the_pasteboards_type_says_the_kind_and_the_card_holds_to_a_line_of_its_own() {
        assert_eq!(kind_word(&Carry::text("a")), "text");
        assert_eq!(kind_word(&Carry::path("/a")), "path");
        assert_eq!(carry_type().to_string(), "dev.bateri.carry");
        // A long line is cut before it is measured, at a character and not a byte.
        let long = "é".repeat(SHOWN_CHARS + 50);
        assert_eq!(shown(&long).chars().count(), SHOWN_CHARS);
        assert_eq!(shown("short"), "short");
        // The card is as wide as its words with the room each side, within a range.
        assert_eq!(card_width(0.0), CARD_NARROWEST);
        assert_eq!(card_width(100.0), 100.0 + 2.0 * CARD_PAD);
        assert_eq!(card_width(10_000.0), CARD_WIDEST);
    }

    /// The picture is the words the terminal will be entered, on one line, within a range: narrow
    /// for a few, the widest for a line that does not fit (its end is faded, not wrapped).
    #[test]
    fn the_card_is_as_wide_as_its_line_within_a_range() {
        let look = Look {
            ground: 0x101010,
            ink: 0xeeeeee,
        };
        let (_, few) = card("a", look);
        let (_, many) = card(&"w".repeat(500), look);
        let (_, some) = card("ACM-412 Fix the login redirect", look);
        for size in [few, many, some] {
            assert_eq!(size.height, CARD_HEIGHT);
        }
        assert_eq!(few.width, CARD_NARROWEST, "{few:?}");
        assert_eq!(many.width, CARD_WIDEST, "{many:?}");
        assert!(
            some.width > few.width && some.width < many.width,
            "{some:?}"
        );
    }
}
