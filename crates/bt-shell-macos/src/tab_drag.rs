//! A tab carried away from its strip: the drag session's source, the type the tab travels under
//! on the pasteboard and the picture it is drawn as.
//!
//! **Only the second half of a drag is a session.** A tab dragged *along* its strip is not one:
//! the bar tracks the pointer itself and slides the chips (`tab_bar`, with `tabs::Grip` deciding
//! where the tab would land). The session starts when the pointer leaves the bar — a tab pulled
//! down or out — because only then can another window's bar, or nothing, receive it, and the
//! session is what finds the window under the pointer. A bar takes the drop through
//! `NSDraggingDestination`; a drop on no bar of ours ends at [`TabDragSource`] with no
//! operation, and the tab becomes a window where the pointer let go
//! (`AppDelegate::tab_drag_ended`) — unless it is a block of panes there ([`crate::tab_merge`]):
//! with ⌥⌘ held over any window's panes, or, for a row of the side column, over its own window's
//! panes without them.
//!
//! **What is carried is tabs or a group** ([`Carried`]): a tab, several picked together in the side
//! column, or a group by its header (or, in the title row, its button). Several tabs and a group
//! are never slid along a list as one chip is: they start a session as soon as they are pulled,
//! and the bar the pointer is over answers `draggingUpdated:` with a line at the place
//! ([`crate::groups::Groups::side_spots`]) and takes the drop ([`Landed`]); the application
//! carries it out as a move ([`crate::moves`]), so every one of them is undone with Undo Move.
//! One tab pulled out of a column that has groups slides in it as it always did, and becomes a
//! session when it leaves it.
//!
//! **What is on the pasteboard means something in this process only**: ids count from zero in
//! every process (a tab's, a group's and a window's alike), and a bateri beside a `bateri-dev`
//! would take each other's tabs for their own. AppKit gives a destination the source *object* only
//! inside the process, so a bar accepts a drop only when its source is a [`TabDragSource`]
//! ([`carried`]); the string is the type's content and a cross-check, not the proof.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
};
use objc2_app_kit::{
    NSApplication, NSCompositingOperation, NSDragOperation, NSDraggingContext, NSDraggingInfo,
    NSDraggingItem, NSDraggingSession, NSDraggingSource, NSEvent, NSEventType, NSFont,
    NSFontAttributeName, NSFontWeightSemibold, NSForegroundColorAttributeName, NSImage,
    NSPasteboardItem, NSStringDrawing, NSView,
};
use objc2_foundation::{
    NSArray, NSAttributedStringKey, NSDictionary, NSObject, NSObjectProtocol, NSPoint, NSRect,
    NSSize, NSString, ns_string,
};

use crate::app;
use crate::groups::{GroupId, Target};
use crate::tab_bar::{Tint, rounded};
use crate::tabs::Landing;

/// The Esc key's virtual key code (`kVK_Escape`).
const ESCAPE: u16 = 53;

/// The pasteboard type a carried tab is written as.
pub(crate) fn tab_type() -> &'static NSString {
    ns_string!("dev.bateri.tab")
}

/// What a drag session carries out of a bar. Tab and group identities are the window's own, so a
/// carry names the window it left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Carried {
    /// Tabs of window `window`, in the order of its strip: one, or the rows picked together.
    Tabs { window: u64, tabs: Vec<u64> },
    /// A group of window `window`, with all its tabs.
    Group { window: u64, group: GroupId },
}

impl Carried {
    /// The window it was carried out of.
    pub(crate) fn window(&self) -> u64 {
        match self {
            Self::Tabs { window, .. } | Self::Group { window, .. } => *window,
        }
    }

    /// The one tab carried, when it is one: the carry the old rules and the ⌥⌘ block know.
    pub(crate) fn lone(&self) -> Option<u64> {
        match self {
            Self::Tabs { tabs, .. } if tabs.len() == 1 => Some(tabs[0]),
            _ => None,
        }
    }

    /// What is written on the pasteboard: all of it, so a source and a pasteboard that disagree
    /// are not taken for one drag.
    fn word(&self) -> String {
        match self {
            Self::Tabs { window, tabs } => {
                let tabs: Vec<String> = tabs.iter().map(u64::to_string).collect();
                format!("t{window}:{}", tabs.join(","))
            }
            Self::Group { window, group } => format!("g{window}:{}", group.get()),
        }
    }
}

/// What a bar that took the drop decided, until the session ends and the application carries it out
/// ([`crate::app::AppDelegate::tab_drag_ended`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Landed {
    /// One tab, let go on a window that has no group: the strip's place ([`crate::tabs::landing`]).
    Strip(Landing),
    /// Tabs, let go where this target says ([`crate::groups::Groups::side_spots`]).
    Tabs(Target<u64>),
    /// A group, let go at this place among the window's groups.
    Group(usize),
}

pub(crate) struct SourceIvars {
    /// What is being carried.
    carried: Carried,
}

define_class!(
    // SAFETY: NSObject subclassing has no requirements; `TabDragSource` implements no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabDragSource"]
    #[ivars = SourceIvars]
    pub(crate) struct TabDragSource;

    unsafe impl NSObjectProtocol for TabDragSource {}

    unsafe impl NSDraggingSource for TabDragSource {
        /// A move inside the application, nothing outside it: no other application takes a tab,
        /// and the cursor over one says so.
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn source_operation_mask(
            &self,
            _session: &NSDraggingSession,
            context: NSDraggingContext,
        ) -> NSDragOperation {
            if context == NSDraggingContext::WithinApplication {
                NSDragOperation::Move
            } else {
                NSDragOperation::None
            }
        }

        /// The picture moved: the application reads what the tab is over — with ⌥⌘ down,
        /// panes that would take its block ([`crate::tab_merge`]).
        #[unsafe(method(draggingSession:movedToPoint:))]
        fn session_moved(&self, _session: &NSDraggingSession, _at: NSPoint) {
            // Read at the pointer, not at the picture's reported place: the regions are the
            // pointer's.
            if let Some(app) = app::delegate(self.mtm()) {
                app.tab_merge_read(None);
            }
        }

        /// ⌘ or ⌥ held at the drop would turn a move into a copy or a link; a tab is only moved.
        #[unsafe(method(ignoreModifierKeysForDraggingSession:))]
        fn ignore_modifier_keys(&self, _session: &NSDraggingSession) -> bool {
            true
        }

        /// The drag is over — dropped on a bar (operation `Move`: the bar has already taken
        /// the drop), on nothing (`None`, the pointer's place on screen) or taken back with Esc,
        /// which also reports `None`: that is the user changing their mind, not asking for a
        /// window. AppKit does not say which, so it is read off what ended the session — the
        /// last event is an Esc key press, or the button is still held (Esc ends the session
        /// with the finger still down). Either one is a tab taken back.
        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn session_ended(
            &self,
            _session: &NSDraggingSession,
            at: NSPoint,
            operation: NSDragOperation,
        ) {
            let taken_back = taken_back(self.mtm());
            if let Some(app) = app::delegate(self.mtm()) {
                app.tab_drag_ended(self.ivars().carried.clone(), at, operation, taken_back);
            }
        }
    }
);

impl TabDragSource {
    fn new(mtm: MainThreadMarker, carried: Carried) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SourceIvars { carried });
        // SAFETY: NSObject's init takes no arguments and the ivars are set.
        unsafe { msg_send![super(this), init] }
    }
}

/// What the drag `info` describes carries, `None` for anything that is not tabs or a group of this
/// process being dragged ([the module header](self)).
pub(crate) fn carried(info: &ProtocolObject<dyn NSDraggingInfo>) -> Option<Carried> {
    let source = info.draggingSource()?.downcast::<TabDragSource>().ok()?;
    let word = info
        .draggingPasteboard()
        .stringForType(tab_type())?
        .to_string();
    let carried = &source.ivars().carried;
    (word == carried.word()).then(|| carried.clone())
}

/// `view` as a picture: what the drag shows under the pointer. A vector one (the same route as the
/// remote-file drag's image), so it is sharp on every screen the pointer takes it to.
pub(crate) fn picture(view: &NSView) -> Option<Retained<NSImage>> {
    let data = view.dataWithPDFInsideRect(view.bounds());
    NSImage::initWithData(NSImage::alloc(), &data)
}

/// Whether the drag session that is ending was taken back with Esc rather than let go.
/// Esc ends a session with no operation, as a drop on nothing does, and AppKit does not say
/// which: it is read off what ended the session — the last event is an Esc key press, or the
/// button is still held (Esc ends the session with the finger still down).
pub(crate) fn taken_back(mtm: MainThreadMarker) -> bool {
    let escaped = NSApplication::sharedApplication(mtm)
        .currentEvent()
        .is_some_and(|event| event.r#type() == NSEventType::KeyDown && event.keyCode() == ESCAPE);
    escaped || NSEvent::pressedMouseButtons() & 1 != 0
}

/// Starts a drag session of one carried thing — the pane `id`, written as a string under `kind` —
/// drawn as `image` (`size` points, or none) centred on the pointer that `event` reports, in
/// `view`'s window. The thing is in the hand that holds it, not where it was pulled from, and the
/// picture does not slide back to its start when nothing takes it: a drop on nothing is the
/// carrier's to read (a window where the pointer let go).
pub(crate) fn start(
    view: &NSView,
    kind: &NSString,
    id: u64,
    event: &NSEvent,
    image: Option<&NSImage>,
    size: NSSize,
    source: &ProtocolObject<dyn NSDraggingSource>,
) {
    start_with(view, kind, &id.to_string(), event, image, size, source);
}

/// [`start`] for a thing that is written as `word` rather than an id.
fn start_with(
    view: &NSView,
    kind: &NSString,
    word: &str,
    event: &NSEvent,
    image: Option<&NSImage>,
    size: NSSize,
    source: &ProtocolObject<dyn NSDraggingSource>,
) {
    let item = NSPasteboardItem::new();
    item.setString_forType(&NSString::from_str(word), kind);
    let dragged = NSDraggingItem::initWithPasteboardWriter(
        NSDraggingItem::alloc(),
        ProtocolObject::from_ref(&*item),
    );
    let at = view.convertPoint_fromView(event.locationInWindow(), None);
    let frame = NSRect::new(
        NSPoint::new(at.x - size.width / 2.0, at.y - size.height / 2.0),
        size,
    );
    // SAFETY: an `NSImage` (or nothing) is a documented dragging frame content.
    unsafe { dragged.setDraggingFrame_contents(frame, image.map(|image| image.as_ref())) };
    let session = view.beginDraggingSessionWithItems_event_source(
        &NSArray::from_retained_slice(&[dragged]),
        event,
        source,
    );
    session.setAnimatesToStartingPositionsOnCancelOrFail(false);
}

/// Starts the session that carries `carried`, drawn as `image` (`size` points) under the pointer
/// that `event` reports, in `view`'s window. The source comes back for its owner to keep alive
/// until the session ends (`AppDelegate::tab_drag_ended`) — AppKit's hold on it is not something to
/// lean on when the window it began in may close first.
pub(crate) fn begin(
    view: &NSView,
    carried: Carried,
    event: &NSEvent,
    image: Option<&NSImage>,
    size: NSSize,
) -> Retained<TabDragSource> {
    let word = carried.word();
    let source = TabDragSource::new(view.mtm(), carried);
    start_with(
        view,
        tab_type(),
        &word,
        event,
        image,
        size,
        ProtocolObject::from_ref(&*source),
    );
    source
}

/// What a picture without a face of its own (a group's header, which is drawn on the column's
/// ground) is held as: `image` on a card's fill and line, with a margin.
pub(crate) fn faced(image: &NSImage, fill: Tint, line: Tint) -> Retained<NSImage> {
    let inner = image.size();
    let size = NSSize::new(inner.width + 2.0 * FACE_PAD, inner.height);
    let image = image.retain();
    let draw = RcBlock::new(move |_: NSRect| -> Bool {
        rounded(
            NSRect::new(NSPoint::ZERO, size),
            FACE_RADIUS,
            fill,
            Some(line),
        );
        // SAFETY: no hints; drawing happens inside the image's handler.
        unsafe {
            image.drawInRect_fromRect_operation_fraction_respectFlipped_hints(
                NSRect::new(NSPoint::new(FACE_PAD, 0.0), inner),
                NSRect::ZERO,
                NSCompositingOperation::SourceOver,
                1.0,
                true,
                None,
            );
        }
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(size, true, &draw)
}

/// The margin a face keeps each side, and its corners.
const FACE_PAD: f64 = 6.0;
const FACE_RADIUS: f64 = 6.0;

/// How far each picture of the stack stands from the one over it, and how many show behind the
/// first.
const STACK_STEP: f64 = 5.0;
const STACK_BEHIND: usize = 2;
/// The count's badge: its height and the room the digits keep each side.
const BADGE_HEIGHT: f64 = 18.0;
const BADGE_PAD: f64 = 6.0;

/// Several rows as one picture: `lead`'s with the next ones standing behind it, a little lower and
/// to the right, and the count in a badge on its top right corner — what the pointer holds while
/// the picked rows are carried. `accent` is the badge's fill and `ink` its digits' colour.
pub(crate) fn stack(lead: &NSImage, count: usize, accent: u32, ink: u32) -> Retained<NSImage> {
    let face = lead.size();
    let behind = (count.saturating_sub(1)).min(STACK_BEHIND);
    let reach = STACK_STEP * behind as f64;
    // Room for the stack behind the face and for the badge, which hangs a little over the corner.
    let size = NSSize::new(
        face.width + reach + BADGE_HEIGHT / 3.0,
        face.height + reach + BADGE_HEIGHT / 3.0,
    );
    let lead = lead.retain();
    let word = NSString::from_str(&count.to_string());
    let draw = RcBlock::new(move |_: NSRect| -> Bool {
        // The view is flipped: the face stands at the top left, the others below and right of it.
        for step in (0..=behind).rev() {
            let at = NSPoint::new(
                STACK_STEP * step as f64,
                BADGE_HEIGHT / 3.0 + STACK_STEP * step as f64,
            );
            let faded = 1.0 - 0.28 * step as f64;
            // SAFETY: no hints; drawing happens inside the image's handler.
            unsafe {
                lead.drawInRect_fromRect_operation_fraction_respectFlipped_hints(
                    NSRect::new(at, face),
                    NSRect::ZERO,
                    NSCompositingOperation::SourceOver,
                    faded,
                    true,
                    None,
                );
            }
        }
        badge(&word, size.width, accent, ink);
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(size, true, &draw)
}

/// The count's badge: a pill with `word` in it, its right edge at `right` and its top at the top.
fn badge(word: &NSString, right: f64, accent: u32, ink: u32) {
    // SAFETY: AppKit's constant font weight.
    let font = NSFont::systemFontOfSize_weight(11.0, unsafe { NSFontWeightSemibold });
    let color = Tint::of(ink, 1.0).color();
    let values: [&AnyObject; 2] = [font.as_ref(), color.as_ref()];
    // SAFETY: AppKit's font and colour attribute keys (extern statics) with the `NSFont` and
    // `NSColor` they document.
    let attributes = unsafe {
        NSDictionary::<NSAttributedStringKey, AnyObject>::from_slices(
            &[NSFontAttributeName, NSForegroundColorAttributeName],
            &values,
        )
    };
    // SAFETY: the dictionary holds a font and a colour under their keys.
    let text = unsafe { word.sizeWithAttributes(Some(&attributes)) };
    let width = (text.width + 2.0 * BADGE_PAD).max(BADGE_HEIGHT);
    let pill = NSRect::new(
        NSPoint::new(right - width, 0.0),
        NSSize::new(width, BADGE_HEIGHT),
    );
    rounded(pill, BADGE_HEIGHT / 2.0, Tint::of(accent, 1.0), None);
    // SAFETY: the same dictionary; drawing happens inside the image's handler.
    unsafe {
        word.drawAtPoint_withAttributes(
            NSPoint::new(
                pill.origin.x + (width - text.width) / 2.0,
                (BADGE_HEIGHT - text.height) / 2.0,
            ),
            Some(&attributes),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::Carried;
    use crate::groups::{Groups, Target};
    use crate::tabs::Tabs;

    /// A group identity of some window.
    fn group() -> crate::groups::GroupId {
        let mut tabs = Tabs::new(1u64);
        let mut groups = Groups::new();
        groups.place(&mut tabs, &[1], Target::New("g".to_owned()));
        groups.groups()[0].id
    }

    #[test]
    fn a_carry_is_written_whole_and_no_two_are_written_alike() {
        let one = Carried::Tabs {
            window: 3,
            tabs: vec![7],
        };
        let several = Carried::Tabs {
            window: 3,
            tabs: vec![7, 9],
        };
        let other_window = Carried::Tabs {
            window: 4,
            tabs: vec![7],
        };
        let whole = Carried::Group {
            window: 3,
            group: group(),
        };
        let words = [
            one.word(),
            several.word(),
            other_window.word(),
            whole.word(),
        ];
        assert_eq!(words, ["t3:7", "t3:7,9", "t4:7", "g3:1"]);
        // The same tab's id in another process's window, or the same id as a group's, is not
        // taken for it.
        for (at, word) in words.iter().enumerate() {
            assert!(
                words
                    .iter()
                    .enumerate()
                    .all(|(other, same)| (other == at) == (same == word))
            );
        }
    }

    #[test]
    fn one_tab_is_the_carry_the_old_rules_know_and_a_group_never_is() {
        let tabs = |tabs: Vec<u64>| Carried::Tabs { window: 1, tabs };
        assert_eq!(tabs(vec![5]).lone(), Some(5));
        assert_eq!(tabs(vec![5, 6]).lone(), None);
        let group = Carried::Group {
            window: 2,
            group: group(),
        };
        assert_eq!(group.lone(), None);
        assert_eq!(group.window(), 2);
        assert_eq!(tabs(vec![5]).window(), 1);
    }
}
