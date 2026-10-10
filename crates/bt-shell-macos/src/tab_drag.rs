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
//! (`AppDelegate::tab_drag_ended`).
//!
//! **The tab's id on the pasteboard means something in this process only**: ids count from zero
//! in every process, and a bateri beside a `bateri-dev` would take each other's tabs for their
//! own. AppKit gives a destination the source *object* only inside the process, so a bar
//! accepts a drop only when its source is a [`TabDragSource`] ([`carried`]); the string is the
//! type's content and a cross-check, not the proof.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSDragOperation, NSDraggingContext, NSDraggingInfo, NSDraggingItem,
    NSDraggingSession, NSDraggingSource, NSEvent, NSEventType, NSImage, NSPasteboardItem, NSView,
};
use objc2_foundation::{
    NSArray, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, ns_string,
};

use crate::app;

/// The Esc key's virtual key code (`kVK_Escape`).
const ESCAPE: u16 = 53;

/// The pasteboard type a carried tab is written as.
pub(crate) fn tab_type() -> &'static NSString {
    ns_string!("dev.bateri.tab")
}

pub(crate) struct SourceIvars {
    /// The tab being carried.
    tab: u64,
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
                app.tab_drag_ended(self.ivars().tab, at, operation, taken_back);
            }
        }
    }
);

impl TabDragSource {
    fn new(mtm: MainThreadMarker, tab: u64) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SourceIvars { tab });
        // SAFETY: NSObject's init takes no arguments and the ivars are set.
        unsafe { msg_send![super(this), init] }
    }

    /// The tab being carried.
    pub(crate) fn tab(&self) -> u64 {
        self.ivars().tab
    }
}

/// The tab carried by the drag `info` describes, `None` for anything that is not a tab of this
/// process being dragged ([the module header](self)).
pub(crate) fn carried(info: &ProtocolObject<dyn NSDraggingInfo>) -> Option<u64> {
    let source = info.draggingSource()?.downcast::<TabDragSource>().ok()?;
    let id: u64 = info
        .draggingPasteboard()
        .stringForType(tab_type())?
        .to_string()
        .parse()
        .ok()?;
    (id == source.tab()).then_some(id)
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

/// Starts a drag session of one carried thing — the tab or pane `id`, written as a string
/// under `kind` — drawn as `image` (`size` points, or none) centred on the pointer that `event`
/// reports, in `view`'s window. The thing is in the hand that holds it, not where it was
/// pulled from, and the picture does not slide back to its start when nothing takes it: a
/// drop on nothing is the carrier's to read (a window where the pointer let go).
pub(crate) fn start(
    view: &NSView,
    kind: &NSString,
    id: u64,
    event: &NSEvent,
    image: Option<&NSImage>,
    size: NSSize,
    source: &ProtocolObject<dyn NSDraggingSource>,
) {
    let item = NSPasteboardItem::new();
    item.setString_forType(&NSString::from_str(&id.to_string()), kind);
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

/// Starts the session that carries tab `tab`, drawn as `view` (its chip) under the pointer that
/// `event` reports. The source comes back for its owner to keep alive until the session ends
/// (`AppDelegate::tab_drag_ended`) — AppKit's hold on it is not something to lean on when the
/// window it began in may close first.
pub(crate) fn begin(view: &NSView, tab: u64, event: &NSEvent) -> Retained<TabDragSource> {
    let source = TabDragSource::new(view.mtm(), tab);
    start(
        view,
        tab_type(),
        tab,
        event,
        picture(view).as_deref(),
        view.bounds().size,
        ProtocolObject::from_ref(&*source),
    );
    source
}
