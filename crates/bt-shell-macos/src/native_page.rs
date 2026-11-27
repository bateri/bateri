//! A page of the UI kit over a pane. The header over the pane says whose page it is and gives
//! the one way out, as over a program's web view (`pencere_view::ViewHead`): its button closes a
//! demo page here, where over a program's page it writes ⌃C to the program.
//!
//! The page itself is `bt-shell-common`'s (`native_view`): the engine's draw list on the
//! renderer, painted in place of the grid by the pane's link, over the whole pane — the header
//! is the owner's, outside it.
//!
//! What the pane adds to the page for it to be used: the text fields over the page's boxes for
//! them (AppKit's, in the terminal's face and the theme's ink — the text of the page is the
//! page's own, a field is not text to draw; a field of lines is a text view in a scroll view,
//! which scrolls the lines the box has no room for where a wrapping text field cannot), the
//! tracking area that tells the page where the
//! pointer is whoever has the keyboard, and the handle the pane's input goes to the page by
//! ([`bt_shell_common::native_view::PageHandle`]: pencere's drive of the page).
//!
//! A page is up for one of two owners ([`Owner`]): a debug build's demo of the kit (`Debug ▸
//! Show Container List Demo`), whose button and ⌃C close it, or the view a program opened — a
//! module the program sent — whose ⌃C and button go to the program, which ends it.
//!
//! **A page's editors are drawn by the page** (`bt_shell_common::native_view`), not by AppKit: the
//! keyboard stays in the terminal's view, which hands the editor with the keyboard its keys, its
//! input method and its pasteboard (`TerminalPane::page_editor_key`, the view's text input), and a
//! screen reader reads each editor as a text area ([`PageEditorElement`]).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ptr::NonNull;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use block2::RcBlock;
use bt_core::Theme;
use bt_shell_common::native_view::ax::{self, AxNode};
use bt_shell_common::native_view::{
    Accessible, AccessibleRole, Command, Field, FieldKind, FieldLine, FieldState, Fill, Id, Key,
    Mods, Outline, PageHandle, Point, Rect, Role, Shortcut, Sort, Tip, ViewFiles, ZOOM_TEXT,
    tone_linear,
};
use objc2::rc::{Retained, Weak as WeakId};
use objc2::runtime::{AnyObject, Bool, ProtocolObject, Sel};
use objc2::{
    AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class,
    msg_send, sel,
};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityAnnouncementKey,
    NSAccessibilityAnnouncementRequestedNotification, NSAccessibilityButtonRole,
    NSAccessibilityCellRole, NSAccessibilityCheckBoxRole, NSAccessibilityCustomAction,
    NSAccessibilityElement, NSAccessibilityGroupRole, NSAccessibilityImageRole,
    NSAccessibilityLayoutChangedNotification, NSAccessibilityLinkRole, NSAccessibilityListRole,
    NSAccessibilityMenuItemRole, NSAccessibilityOutlineRole, NSAccessibilityPostNotification,
    NSAccessibilityPostNotificationWithUserInfo, NSAccessibilityPriorityKey,
    NSAccessibilityPriorityLevel, NSAccessibilityProgressIndicatorRole,
    NSAccessibilityRadioButtonRole, NSAccessibilityRole, NSAccessibilityRowRole,
    NSAccessibilitySelectedTextChangedNotification, NSAccessibilitySliderRole,
    NSAccessibilitySortDirection, NSAccessibilityStaticTextRole, NSAccessibilitySubrole,
    NSAccessibilitySwitchSubrole, NSAccessibilityTabButtonSubrole, NSAccessibilityTabGroupRole,
    NSAccessibilityTableRole, NSAccessibilityTextAreaRole, NSAccessibilityValueChangedNotification,
    NSApplication, NSAutoresizingMaskOptions, NSBackingStoreType, NSBorderType, NSColor, NSControl,
    NSEvent, NSEventModifierFlags, NSEventType, NSFocusRingType, NSFont, NSLayoutManager,
    NSLayoutManagerDelegate, NSLineBreakMode, NSResponder, NSScrollView, NSScroller,
    NSScrollerPart, NSScrollerStyle, NSSecureTextField, NSText, NSTextContainer, NSTextField,
    NSTextStorage, NSTextView, NSTrackingArea, NSTrackingAreaOptions, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{
    NSArray, NSDictionary, NSInteger, NSNumber, NSObject, NSObjectProtocol, NSPoint, NSRange,
    NSRect, NSSize, NSString, NSUndoManager, ns_string,
};

use crate::pane::TerminalPane;
#[cfg(debug_assertions)]
use crate::pencere_view::{Exit, Words};

/// What the header says over the demo page: the made-up program's name and the title of its
/// page, and a button that closes the page.
#[cfg(debug_assertions)]
pub(crate) fn demo_words() -> Words {
    let words = bt_shell_common::view_strip::Strip {
        place: None,
        program: Some("dockerpen".to_owned()),
        title: Some("Docker".to_owned()),
        quit_tip: "Closes the demo page. The terminal comes back.".to_owned(),
    };
    Words::new(words, &[], Exit::Close)
}

/// Whose a page is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Owner {
    /// The debug menu's: the person opened it and closes it.
    #[cfg(debug_assertions)]
    Demo,
    /// A program's view: the program ends it (⌃C reaches the program, its `closed` or `bye`
    /// brings the page down), and what the page says goes to the program.
    Program,
    /// The page inspector's, which is all its pane has: no program holds it, what the page says is
    /// answered in this process ([`crate::inspector`]) and its closing is the pane's.
    Inspector,
}

/// What is up while a page of the kit is: the handle the pane's input goes to the page by, the
/// fields the page has, the tracking area over the pane, whose page it is.
pub(crate) struct Page {
    pub owner: Owner,
    pub handle: PageHandle,
    pub fields: Rc<PageFields>,
    pub area: Retained<NSTrackingArea>,
    /// The files beside the bundle of a program's view: the handles of what the person gave it
    /// and of the places it may save in, and the thread that reads and writes them. They live
    /// as long as the page: the demo has none.
    pub files: Option<Arc<ViewFiles>>,
    /// The browsers of the page, if the shell shows any: a program's view has them, a debug build's
    /// demo has none.
    pub browsers: Option<Rc<crate::native_browser::PageBrowsers>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; the host implements no `Drop` and is born with
    // `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPageFieldsHost"]
    #[ivars = ()]
    pub(crate) struct FieldsHost;

    unsafe impl NSObjectProtocol for FieldsHost {}

    impl FieldsHost {
        /// The page's own axis: y from the top, as the page's rooms are measured. A field's frame
        /// is its room, whatever the pane's height.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// Only the fields take a press; everywhere else the page under the host is hit. A field's
        /// room is all its own to the pointer, and its text stands inside it (the element's
        /// padding, the line in the middle of a taller room): a press in the room and outside
        /// the field's frame is the field's too, and gives it the keyboard as one in its text does.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            // SAFETY: reading the superview; we are on the main thread.
            let local = match unsafe { self.superview() } {
                Some(parent) => self.convertPoint_fromView(point, Some(&parent)),
                None => point,
            };
            let fields: Vec<_> = self.subviews().iter().collect();
            fields.into_iter().rev().find_map(|field| {
                if field.isHidden() {
                    return None;
                }
                let frame = field.frame();
                if inside(frame, local) {
                    return field.hitTest(local);
                }
                // In the room and outside the text: the press is the nearest one in the frame, so
                // that a field being typed in answers with its editor, as AppKit does for a press
                // in it, and not with the field, which would end the editing and begin it again
                // (the whole text selected, the page told the keyboard came a second time). A field
                // of lines is its text view, which is its own editor: it takes the press itself, not
                // what the nearest point of its frame holds (a scroller at the edge).
                room_of(&field)
                    .filter(|room| inside(*room, local))
                    .and_then(|_| match lines_text_in(&field) {
                        Some(text) => Some(Retained::into_super(Retained::into_super(
                            Retained::into_super(text),
                        ))),
                        None => field.hitTest(nearest_in(frame, local)),
                    })
            })
        }
    }
);

/// Whether `point` is in `rect`.
fn inside(rect: NSRect, point: NSPoint) -> bool {
    point.x >= rect.origin.x
        && point.y >= rect.origin.y
        && point.x < rect.origin.x + rect.size.width
        && point.y < rect.origin.y + rect.size.height
}

/// The point of `rect` nearest `point` (inside it, where `rect` is not empty).
fn nearest_in(rect: NSRect, point: NSPoint) -> NSPoint {
    let within = |at: f64, from: f64, across: f64| at.clamp(from, (from + across - 0.5).max(from));
    NSPoint::new(
        within(point.x, rect.origin.x, rect.size.width),
        within(point.y, rect.origin.y, rect.size.height),
    )
}

/// The room the page laid out for `view`, if it is one of the page's fields: all of it is the
/// field's for the pointer, its text stands in a part of it (`place_field`).
fn room_of(view: &NSView) -> Option<NSRect> {
    let object: &AnyObject = view;
    object
        .downcast_ref::<PageTextField>()
        .map(|field| field.ivars().room.get())
        .or_else(|| {
            object
                .downcast_ref::<PageSecretField>()
                .map(|field| field.ivars().room.get())
        })
        .or_else(|| lines_text_in(view).map(|text| text.ivars().room.get()))
}

/// The text view of a field of lines, if `view` is the scroll view it stands in.
fn lines_text_in(view: &NSView) -> Option<Retained<PageLinesText>> {
    let object: &AnyObject = view;
    object
        .downcast_ref::<NSScrollView>()?
        .documentView()?
        .downcast::<PageLinesText>()
        .ok()
}

impl FieldsHost {
    fn new(pane: &TerminalPane) -> Retained<Self> {
        Self::with_frame(pane.mtm(), pane.bounds())
    }

    fn with_frame(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:` is NSView's designated initializer.
        let host: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        host.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        host
    }
}

/// One text field of the page's: its name, its kind, the field of AppKit's that stands in its
/// room.
pub(crate) struct PageField {
    pub id: Id,
    pub kind: FieldKind,
    pub view: FieldView,
}

/// The page's fields and what a screen reader says of it, as its outline last said
/// ([`PageFields::apply`]), and how they are to look: the theme's ink, the terminal's face, as
/// large as the page is drawn.
pub(crate) struct PageFields {
    pane: WeakId<TerminalPane>,
    /// The view the fields stand in: over the whole pane, on the page's axis (y from the top),
    /// so a field's frame is the room the page laid out for it and nothing of AppKit's moves it
    /// when the pane changes size or place: the fields are where the page is drawn, always.
    host: Retained<FieldsHost>,
    theme: Cell<Theme>,
    family: Option<String>,
    zoom: Cell<f32>,
    /// How tall one line of a field is at this zoom, in the page's points: said to the page's
    /// measure ([`FieldLine`]), which lays out a field of rows with it.
    line: FieldLine,
    fields: RefCell<Vec<PageField>>,
    /// The outline the fields were last put by: where they go again when the pane's height
    /// changes and the page's outline does not ([`PageFields::place_again`]).
    outline: RefCell<Option<Outline>>,
    accessible: RefCell<Vec<Accessible>>,
    /// What a screen reader is given of the page as its own: an element for a button or words,
    /// the field itself for a field, those that nothing holds (a dialog, a table, a button on the
    /// page); what a group holds is under it ([`ax::plan`]).
    elements: RefCell<Vec<Retained<AnyObject>>>,
    /// The elements made for the items that have an id, by that id: the next outline that has the
    /// item gives it the same element again, so that a screen reader that is on it stays on it while
    /// the page changes around it.
    by_id: RefCell<HashMap<Id, Retained<PageElement>>>,
    /// Every element made for `accessible`, those inside a group too, for what asks which of the
    /// pane's accessibility children are the page's.
    owned: RefCell<Vec<Retained<AnyObject>>>,
    /// The page's tooltips as its outline last said, and the ones put on the pane's view for
    /// them: AppKit's tag of each and the words it shows (the view does not keep them).
    tips: RefCell<Vec<Tip>>,
    tip_tags: RefCell<Vec<(isize, Retained<NSString>)>>,
    /// The field the keyboard was last known to be in.
    focused: Cell<Option<Id>>,
    /// The page's own reply is moving the keyboard: the field that takes it does not tell the
    /// page it was clicked into.
    moving: Cell<bool>,
}

impl PageFields {
    pub(crate) fn new(
        pane: &TerminalPane,
        theme: Theme,
        family: Option<String>,
        zoom: f32,
        line: FieldLine,
    ) -> Rc<Self> {
        let host = FieldsHost::new(pane);
        pane.add_page_fields_host(&host);
        // Said before the page's first frame is laid out: it is made with the page.
        line.set(line_of(pane.mtm(), family.as_deref(), zoom));
        Rc::new(Self {
            pane: WeakId::new(pane),
            host,
            theme: Cell::new(theme),
            family,
            zoom: Cell::new(zoom),
            line,
            fields: RefCell::new(Vec::new()),
            outline: RefCell::new(None),
            accessible: RefCell::new(Vec::new()),
            elements: RefCell::new(Vec::new()),
            by_id: RefCell::new(HashMap::new()),
            owned: RefCell::new(Vec::new()),
            tips: RefCell::new(Vec::new()),
            tip_tags: RefCell::new(Vec::new()),
            focused: Cell::new(None),
            moving: Cell::new(false),
        })
    }

    /// The page's outline changed: a field for each the page names, of its kind, where its room
    /// is, saying its hint; the fields it no longer names are taken away (the keyboard in one
    /// goes back to the page). What a screen reader says is kept for when it asks.
    pub(crate) fn apply(self: &Rc<Self>, outline: &Outline) {
        let Some(pane) = self.pane.load() else {
            return;
        };
        self.cover(&pane);
        let mut fields = self.fields.borrow_mut();
        let kept: Vec<(Id, FieldKind)> = outline.fields.iter().map(|f| (f.id, f.kind)).collect();
        fields.retain(|field| {
            let stays = kept.contains(&(field.id, field.kind));
            if !stays {
                if field.view.editing()
                    && let Some(window) = pane.window()
                {
                    window.makeFirstResponder(Some(pane.view()));
                }
                field.view.take_away();
            }
            stays
        });
        for said in &outline.fields {
            let at = fields
                .iter()
                .position(|field| field.id == said.id && field.kind == said.kind);
            let index = match at {
                Some(index) => index,
                None => {
                    let view = text_field(
                        &pane,
                        said.kind,
                        &self.theme.get(),
                        self.family.as_deref(),
                        self,
                    );
                    view.ivars().id.set(Some(said.id));
                    self.host.addSubview(&view);
                    fields.push(PageField {
                        id: said.id,
                        kind: said.kind,
                        view,
                    });
                    fields.len() - 1
                }
            };
            if let Some(field) = fields.get(index) {
                place_field(&field.view, said, self.zoom.get(), self.family.as_deref());
            }
        }
        drop(fields);
        if *self.tips.borrow() != outline.tips {
            self.tips.replace(outline.tips.clone());
            self.put_tips(&pane);
        }
        self.outline.replace(Some(outline.clone()));
        if *self.accessible.borrow() != outline.accessible {
            // What the page says out loud, and was not saying: told to the screen reader at once.
            let said: Vec<String> = ax::alerts(&self.accessible.borrow())
                .into_iter()
                .map(str::to_owned)
                .collect();
            let alerts: Vec<String> = ax::alerts(&outline.accessible)
                .into_iter()
                .filter(|words| !said.iter().any(|before| before == words))
                .map(str::to_owned)
                .collect();
            self.accessible.replace(outline.accessible.clone());
            let nodes = ax::plan(&outline.accessible);
            // What the last outline made is given again to what is still there (by id), and what
            // is left of it goes with the outline.
            let before = RefCell::new(self.by_id.take());
            let made: Vec<Option<Made>> = outline
                .accessible
                .iter()
                .zip(&nodes)
                .map(|(item, node)| self.element_of(&pane, item, node, &before))
                .collect();
            // An id that two items of the outline carry (a ref that clashes) is kept for neither: the elements
            // would be given to each other's item the next time, and the screen reader would be on the other.
            let mut seen: HashMap<Id, usize> = HashMap::new();
            for id in outline.accessible.iter().filter_map(|item| item.id) {
                *seen.entry(id).or_default() += 1;
            }
            self.by_id.replace(
                made.iter()
                    .zip(&outline.accessible)
                    .filter_map(|(made, item)| match (made, item.id) {
                        (Some(Made::Page(element)), Some(id)) if seen.get(&id) == Some(&1) => {
                            Some((id, element.clone()))
                        }
                        _ => None,
                    })
                    .collect(),
            );
            relate(&pane, &outline.accessible, &nodes, &made);
            let seen = |at: usize| made.get(at).and_then(Option::as_ref).map(Made::object);
            // The page's own: what nothing that was made holds (a node whose group has no element
            // is not under it).
            let top = outline
                .accessible
                .iter()
                .enumerate()
                .filter(|(at, _)| nearest(&nodes, &made, *at).is_none())
                .filter_map(|(at, _)| seen(at))
                .collect();
            self.elements.replace(top);
            self.owned
                .replace((0..made.len()).filter_map(seen).collect());
            // SAFETY: the pane is an element of the accessibility tree (a view), and the name is
            // one of AppKit's own.
            unsafe {
                NSAccessibilityPostNotification(&pane, NSAccessibilityLayoutChangedNotification)
            };
            for words in alerts {
                announce(&pane, &words);
            }
        }
        // Whatever the outline changed: a field that came behind a modal that stays, or one that was made
        // again, is shut out as well. (A host that gives no accessible items, an older one, shuts nothing out.)
        if !outline.accessible.is_empty() {
            self.hide_shut_out_fields(&outline.accessible);
        }
    }

    /// A field the page's outline does not read (the page shut it out: it is behind a modal in front)
    /// is not an element of the accessibility tree at all, and one it reads again is.
    fn hide_shut_out_fields(&self, accessible: &[Accessible]) {
        let read: Vec<Id> = accessible
            .iter()
            .filter(|item| matches!(item.role, AccessibleRole::Field(_)))
            .filter_map(|item| item.id)
            .collect();
        for field in self.fields.borrow().iter() {
            field.view.set_read(read.contains(&field.id));
        }
    }

    /// The pane changed size or place: the host covers it again and each field stands where the
    /// last outline says. The host's axis is the page's, so a field's frame does not depend on the
    /// pane's height; this only settles a host whose frame was set while the pane was out of its
    /// window. Nothing is made or taken away here.
    pub(crate) fn place_again(&self) {
        let Some(pane) = self.pane.load() else {
            return;
        };
        self.cover(&pane);
        self.put_tips(&pane);
        let outline = self.outline.borrow();
        let Some(outline) = outline.as_ref() else {
            return;
        };
        let fields = self.fields.borrow();
        for said in &outline.fields {
            if let Some(field) = fields
                .iter()
                .find(|field| field.id == said.id && field.kind == said.kind)
            {
                place_field(&field.view, said, self.zoom.get(), self.family.as_deref());
            }
        }
    }

    /// The host over the whole pane, as the pane is now (its autoresizing keeps it so; this
    /// settles what a frame set while the pane was out of its window left).
    fn cover(&self, pane: &TerminalPane) {
        let bounds = pane.bounds();
        if self.host.frame() != bounds {
            self.host.setFrame(bounds);
        }
    }

    /// Puts the page's tooltips on the pane's view, where the last outline says, in place of the
    /// ones it had: AppKit shows each while the pointer rests in its rectangle, as the system's own.
    fn put_tips(&self, pane: &TerminalPane) {
        let view = pane.view();
        for (tag, _) in self.tip_tags.take() {
            view.removeToolTip(tag);
        }
        let tags = self
            .tips
            .borrow()
            .iter()
            .map(|tip| {
                // The page's rooms are from the pane's top, the pane's frames from its bottom.
                let in_pane = rect_in_pane(pane, tip.rect);
                let words = NSString::from_str(&tip.words);
                let at = view.convertRect_fromView(in_pane, Some(pane));
                // SAFETY: an owner that does not answer `view:stringForToolTip:point:userData:`
                // is shown by its description, which a string's is; it is kept in `tip_tags` for
                // as long as the tooltip is on the view, and the data is null.
                let tag =
                    unsafe { view.addToolTipRect_owner_userData(at, &words, std::ptr::null_mut()) };
                (tag, words)
            })
            .collect();
        self.tip_tags.replace(tags);
    }

    /// What a screen reader is given for `item`: the field itself for a field (it reads and edits
    /// it as AppKit's), an element of the pane for the rest, where it stands in the pane, as what
    /// the page says it is ([`accessibility_role`]) and with its state: checked, chosen, open, of
    /// no use now, the value it shows.
    fn element_of(
        &self,
        pane: &TerminalPane,
        item: &Accessible,
        node: &AxNode,
        before: &RefCell<HashMap<Id, Retained<PageElement>>>,
    ) -> Option<Made> {
        if item.role == AccessibleRole::Editor {
            return editor_element(pane, item).map(Made::Other);
        }
        if let AccessibleRole::Field(_) = item.role {
            // What AppKit reads of the field, whichever class it is: its cell ([`read_of`]), or the
            // text view of lines.
            return Some(Made::Other(self.field(item.id?)?.read()));
        }
        let state = &item.state;
        let shape = shape_of(item, node);
        // What answers the pointer is pressed whatever it says it is (an option, a tree's row, a
        // header that sorts), and what a role has always meant pressed is, with no word of the page.
        let press =
            ((item.pressable || pressed_by_role(item.role)) && !state.disabled).then(|| {
                let rect = item.rect;
                (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
            });
        // Only what stands partly outside what the pane shows is asked for to be brought in: an
        // element that is wholly in view is not (a screen reader that lands on each of them would
        // make the page draw again for nothing).
        let target = item.id.filter(|_| {
            let (width, height) = {
                let size = pane.bounds().size;
                (size.width as f32, size.height as f32)
            };
            let rect = item.rect;
            rect.x < 0.0 || rect.y < 0.0 || rect.x + rect.w > width || rect.y + rect.h > height
        });
        let disclosed = state
            .expanded
            .filter(|_| matches!(item.role, AccessibleRole::TreeItem));
        // The element the last outline made for this id, if it is of the same shape (what is set only
        // where it is said, and is a number or a flag that cannot be unset, is the same): the screen reader
        // stays on it. What can be set to nothing is set again each time, so it does not tell elements apart.
        let kept = item
            .id
            .and_then(|id| before.borrow_mut().remove(&id))
            .filter(|element| element.ivars().shape == shape);
        let element = if let Some(element) = kept {
            let ivars = element.ivars();
            ivars.press.set(press);
            ivars.target.set(target);
            ivars.disclosed.set(disclosed);
            element
        } else {
            let element = PageElement::alloc(pane.mtm()).set_ivars(PageElementIvars {
                press: Cell::new(press),
                pane: WeakId::new(pane),
                target: Cell::new(target),
                disclosed: Cell::new(disclosed),
                cells: RefCell::new(Vec::new()),
                shape,
            });
            // SAFETY: `init` is NSObject's designated initializer and the ivars are set.
            unsafe { msg_send![super(element), init] }
        };
        let words = NSString::from_str(&item.words);
        let (role, subrole) = accessibility_role(item.role);
        // SAFETY: the roles are AppKit's own constants; the parent is the pane, a view that
        // outlives the element (the element goes with the page's outline).
        unsafe {
            element.setAccessibilityRole(Some(role));
            if let Some(subrole) = subrole {
                element.setAccessibilitySubrole(Some(subrole));
            }
            element.setAccessibilityParent(Some(pane));
        }
        element.setAccessibilityLabel(Some(&words));
        if let AccessibleRole::Heading(level) = item.role {
            // AppKit has no role for a heading: static text that says it is one, and its level.
            let said = NSString::from_str(&format!("heading, level {level}"));
            element.setAccessibilityRoleDescription(Some(&said));
        }
        // SAFETY: the values are those AppKit's own controls of each role have: a number for what
        // is checked (0 or 1, and 2 for a box some of what it stands for is checked), a string for
        // words and what a progress or a slider shows.
        unsafe {
            if let Some(checked) = state.checked {
                element.setAccessibilityValue(Some(&NSNumber::new_i32(i32::from(checked))));
            } else if state.mixed {
                element.setAccessibilityValue(Some(&NSNumber::new_i32(2)));
            } else if let Some(value) = &state.value {
                element.setAccessibilityValue(Some(&NSString::from_str(value)));
            } else if matches!(item.role, AccessibleRole::Text | AccessibleRole::Heading(_)) {
                element.setAccessibilityValue(Some(&words));
            } else if matches!(item.role, AccessibleRole::Tab | AccessibleRole::Radio) {
                // A tab or a radio button is on or off by its value, as AppKit's own are: what a
                // screen reader reads as "selected".
                element.setAccessibilityValue(Some(&NSNumber::new_i32(i32::from(state.selected))));
            } else {
                // What a kept element showed before (a box that was checked, a value that went) is gone.
                element.setAccessibilityValue(None);
            }
        }
        element.setAccessibilitySelected(state.selected);
        if let Some(open) = state.expanded {
            element.setAccessibilityExpanded(open);
            // An outline's row is open or closed as AXDisclosing says (set after the element knows
            // which it is, so that the request below sees no change in what is set here).
            if matches!(item.role, AccessibleRole::TreeItem) {
                element.setAccessibilityDisclosed(open);
            }
        }
        element.setAccessibilityEnabled(!state.disabled);
        element.setAccessibilityFrameInParentSpace(frame_in_pane(pane, item));
        Some(Made::Page(element))
    }

    /// What the page can be told to do now, as its outline last said: the nearest the keyboard
    /// first (Page Commands ▸, and the palette that lists the menu).
    pub(crate) fn commands(&self) -> Vec<Command> {
        self.outline
            .borrow()
            .as_ref()
            .map(|outline| outline.commands.clone())
            .unwrap_or_default()
    }

    /// What a screen reader is given of the page as its own, in the order it reads it: what nothing
    /// holds (what a group holds is under it).
    pub(crate) fn elements(&self) -> Vec<Retained<AnyObject>> {
        self.elements.borrow().clone()
    }

    /// Every object that stands for the page to a screen reader, those inside a group too, and the
    /// views and cells of all its fields: what the pane's own answer is cleared of, so that a field a
    /// modal in front shuts out is not given back as the pane's.
    pub(crate) fn owned(&self) -> Vec<Retained<AnyObject>> {
        let mut all = self.owned.borrow().clone();
        all.extend(
            self.fields
                .borrow()
                .iter()
                .flat_map(|field| field.view.owned()),
        );
        all
    }

    /// The editor named `id` may have changed its text or moved its selection: a screen reader is
    /// told, of the text area it reads it as.
    pub(crate) fn editor_changed(&self, id: Id) {
        let elements = self.elements.borrow();
        let Some(element) = elements.iter().find_map(|element| {
            element
                .downcast_ref::<PageEditorElement>()
                .filter(|element| element.ivars().id == id)
        }) else {
            return;
        };
        // SAFETY: the element is one of the accessibility tree's, and the names are AppKit's.
        unsafe {
            NSAccessibilityPostNotification(element, NSAccessibilityValueChangedNotification);
            NSAccessibilityPostNotification(
                element,
                NSAccessibilitySelectedTextChangedNotification,
            );
        }
    }

    /// The field named `id`.
    pub(crate) fn field(&self, id: Id) -> Option<FieldView> {
        self.fields
            .borrow()
            .iter()
            .find(|field| field.id == id)
            .map(|field| field.view.clone())
    }

    /// The name and the kind of the field `control` is, if it is one of the page's.
    pub(crate) fn of(&self, control: &AnyObject) -> Option<(Id, FieldKind)> {
        self.fields
            .borrow()
            .iter()
            .find(|field| {
                let is = |view: &NSView| {
                    std::ptr::eq(
                        std::ptr::from_ref(control).cast::<()>(),
                        std::ptr::from_ref(view).cast::<()>(),
                    )
                };
                is(&field.view) || is(field.view.inner())
            })
            .map(|field| (field.id, field.kind))
    }

    /// What the field `id` holds, for an agent that reads the page. A secret field gives none, and
    /// none is asked of it: what a password holds is the person's and is not read from the screen.
    pub(crate) fn text_of(&self, id: Id) -> Option<String> {
        let fields = self.fields.borrow();
        let field = fields.iter().find(|field| field.id == id)?;
        if field.kind == FieldKind::Secret {
            return None;
        }
        Some(field.view.text())
    }

    /// Puts `text` in the field `id`, in place of what it holds, as the page's own reply does
    /// ([`PageFields::fill`]), for an agent's `fill`.
    pub(crate) fn put(&self, id: Id, text: &str) {
        self.fill(id, &Fill::text(text));
    }

    /// Puts `text` after what the field `id` holds, and gives what it holds now: for the page to be
    /// told, and for nobody else — the field may be a password's. `None` for a field that is not
    /// on the screen.
    pub(crate) fn append(&self, id: Id, text: &str) -> Option<String> {
        let field = self.field(id)?;
        let now = field.held(&format!("{}{text}", field.text()));
        field.set_text(&now);
        Some(now)
    }

    /// Whether the pointer is in the room of a field that is shown: all of the room is the field's
    /// to the pointer ([`FieldsHost`]), so is its cursor.
    pub(crate) fn pointer_in_a_room(&self) -> bool {
        let Some(window) = self.pane.load().and_then(|pane| pane.window()) else {
            return false;
        };
        let at = self
            .host
            .convertPoint_fromView(window.mouseLocationOutsideOfEventStream(), None);
        self.fields
            .borrow()
            .iter()
            .any(|field| !field.view.isHidden() && inside(field.view.ivars().room.get(), at))
    }

    /// The field the keyboard is in, where it is in one of the page's.
    pub(crate) fn keyboard_in(&self) -> Option<Id> {
        self.focused.get()
    }

    /// Whether the keyboard is in one of the page's fields.
    pub(crate) fn typing(&self) -> bool {
        self.fields
            .borrow()
            .iter()
            .any(|field| field.view.editing())
    }

    /// Every field is taken away (the page goes down).
    pub(crate) fn clear(&self) {
        for field in self.fields.take() {
            field.view.take_away();
        }
        self.host.removeFromSuperview();
        self.outline.take();
        self.accessible.take();
        self.elements.take();
        self.tips.take();
        if let Some(pane) = self.pane.load() {
            for (tag, _) in self.tip_tags.take() {
                pane.view().removeToolTip(tag);
            }
        }
    }

    /// The theme changed: the fields' ink with it.
    pub(crate) fn paint(&self, theme: &Theme) {
        self.theme.set(*theme);
        for field in self.fields.borrow().iter() {
            field.view.set_ink(&ink(theme, Role::Foreground));
        }
    }

    /// The page is drawn `zoom` times its own size now: the fields' text with it (they are put
    /// where the next outline says), and the line the page lays its rows out with is measured again
    /// (a face's line is not in proportion to its size). The pane's face does not change while a page
    /// is up (the fields and the page's measure keep the one it was opened with), so the zoom is
    /// all that moves it.
    pub(crate) fn set_zoom(&self, zoom: f32) {
        if self.zoom.replace(zoom) == zoom {
            return;
        }
        if let Some(pane) = self.pane.load() {
            self.line
                .set(line_of(pane.mtm(), self.family.as_deref(), zoom));
        }
    }

    /// The keyboard went into the field named `id`: the page is told, unless its own reply sent
    /// it there.
    fn focused_by_person(&self, id: Id) {
        self.focused.set(Some(id));
        if self.moving.get() {
            return;
        }
        if let Some(pane) = self.pane.load() {
            pane.page_field_focused(Some(id));
        }
    }

    /// Whether the keyboard left the fields of itself since it was last looked at (the person
    /// clicked on the page): the page is to be told, and is from now on taken to know.
    pub(crate) fn left_of_itself(&self) -> bool {
        if self.focused.get().is_some() && !self.typing() {
            self.focused.set(None);
            return true;
        }
        false
    }

    /// Puts the keyboard where the page's reply says: in its field `id`, or back in the page.
    /// The page is not told of a move it asked for.
    pub(crate) fn send_keyboard(&self, to: Option<Id>) {
        let Some(pane) = self.pane.load() else {
            return;
        };
        let Some(window) = pane.window() else {
            return;
        };
        self.moving.set(true);
        match to.and_then(|id| self.field(id)) {
            Some(field) => {
                window.makeFirstResponder(Some(field.inner()));
                field.select_waiting();
                self.focused.set(to);
            }
            None => {
                window.makeFirstResponder(Some(pane.view()));
                self.focused.set(None);
            }
        }
        self.moving.set(false);
    }

    /// What a reply puts in the field named `id` (a text in place of what it holds, what is
    /// selected in it, or both: [`Fill`]); a name that is not a field is passed over. The page is
    /// not told of a text it put in itself (setting the value sends no change). Where a text is put
    /// in and none selected, the caret goes to its end, where AppKit's own rule would select it all
    /// once the keyboard comes. A field that does not have the keyboard has no editor to select in:
    /// the selection waits for the keyboard the page sends it (`send_keyboard`). A field of one line
    /// holds a text's line breaks as spaces ([`one_line`]), one for one, so a selection counted in
    /// the page's text selects the same characters.
    pub(crate) fn fill(&self, id: Id, fill: &Fill) {
        let Some(field) = self.field(id) else {
            return;
        };
        let text = match &fill.text {
            Some(text) => {
                let text = field.held(text);
                field.set_text(&text);
                text
            }
            None => field.text(),
        };
        let selected = match fill.selection_in(text.chars().count()) {
            Some(range) => Some(utf16_range(&text, range)),
            None if fill.text.is_some() => Some((text.encode_utf16().count(), 0)),
            None => None,
        };
        if let Some(range) = selected {
            field.select(range);
        }
    }
}

/// A range of characters (code points, as the kit counts a selection) of `text` as the range of its
/// UTF-16 units that AppKit's text counts in: `(location, length)`.
pub(crate) fn utf16_range(text: &str, chars: std::ops::Range<usize>) -> (usize, usize) {
    let units = |upto: usize| -> usize { text.chars().take(upto).map(char::len_utf16).sum() };
    let (from, to) = (units(chars.start), units(chars.end));
    (from, to.saturating_sub(from))
}

/// `text` with each line and paragraph separator (`\n`, `\r`, U+2028, U+2029) as a space, one for
/// one, as AppKit's single-line cells take a paste; `None` where it has none. What a field of one
/// line holds, whoever puts it there: the person (its editor, [`one_line_words`]) or the page
/// ([`PageFields::fill`]).
fn one_line(text: &str) -> Option<String> {
    let breaks = |c: char| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}');
    text.contains(breaks).then(|| text.replace(breaks, " "))
}

/// A field of the page's as AppKit has it: a field of text, or a secret's, which is an
/// `NSSecureTextField`. That is the one class AppKit gives the editor that hides what is typed: it
/// refuses it to a plain field that only has a secure cell, and the refusal is an exception, which
/// ended bateri when a page sent the keyboard into a password field. Lines are not a text field's:
/// a wrapping `NSTextField` cannot scroll ("the cell wraps text and makes the cell non-scrollable"),
/// so they are a text view in a scroll view ([`LinesView`]), which is its own editor. To the page and
/// the pane the three are one field ([`page_field_class`]): what they keep and do is the same, only
/// AppKit's class differs.
#[derive(Clone)]
pub(crate) enum FieldView {
    Text(Retained<PageTextField>),
    Secret(Retained<PageSecretField>),
    Lines(LinesView),
}

/// A field of lines: the text view, and the scroll view that shows as much of it as the box has room
/// for. The scroll view is what stands in the host, in the page's padding box (the room less the
/// border: what scrolls is cut there, the padding going with the text); the text view takes the
/// keyboard and holds the text in the text box, at the padding of each side.
#[derive(Clone)]
pub(crate) struct LinesView {
    scroll: Retained<NSScrollView>,
    text: Retained<PageLinesText>,
}

/// What the page's field is to the host: the view it puts in its room (a field of lines is the
/// scroll view).
impl std::ops::Deref for FieldView {
    type Target = NSView;

    fn deref(&self) -> &NSView {
        match self {
            Self::Text(field) => field,
            Self::Secret(field) => field,
            Self::Lines(lines) => &lines.scroll,
        }
    }
}

impl FieldView {
    /// What the field keeps, whichever class it is.
    fn ivars(&self) -> &PageTextFieldIvars {
        match self {
            Self::Text(field) => field.ivars(),
            Self::Secret(field) => field.ivars(),
            Self::Lines(lines) => lines.text.ivars(),
        }
    }

    /// The field of AppKit's it is, unless it is lines.
    fn text_field(&self) -> Option<&NSTextField> {
        match self {
            Self::Text(field) => Some(field),
            Self::Secret(field) => Some(field),
            Self::Lines(_) => None,
        }
    }

    /// The view that takes the keyboard: the field itself, or the text view of lines.
    pub(crate) fn inner(&self) -> &NSView {
        match self {
            Self::Lines(lines) => &lines.text,
            _ => self,
        }
    }

    /// The field leaves the page: what was typed in a field of lines is not undone in a field that is
    /// not there.
    pub(crate) fn take_away(&self) {
        if let Self::Lines(lines) = self {
            lines.text.forget_undo();
        }
        self.removeFromSuperview();
    }

    /// Whether the keyboard is in the field.
    pub(crate) fn editing(&self) -> bool {
        match self.text_field() {
            Some(field) => field.currentEditor().is_some(),
            None => first_responder(self.inner()),
        }
    }

    /// What the field holds.
    pub(crate) fn text(&self) -> String {
        match self {
            Self::Text(field) => field.stringValue().to_string(),
            Self::Secret(field) => field.stringValue().to_string(),
            Self::Lines(lines) => lines.text.string().to_string(),
        }
    }

    /// Puts `text` in the field in place of what it holds, as it is (see [`FieldView::held`]). The
    /// page is not told of it. A field of lines has its caret at the end of it, in view.
    fn set_text(&self, text: &str) {
        match self {
            Self::Text(field) => field.setStringValue(&NSString::from_str(text)),
            Self::Secret(field) => field.setStringValue(&NSString::from_str(text)),
            Self::Lines(lines) => {
                if lines.text.string().to_string() != text {
                    lines.text.setString(&NSString::from_str(text));
                    lines.text.forget_undo();
                }
                lines
                    .text
                    .setSelectedRange(NSRange::new(text.encode_utf16().count(), 0));
                lines.text.scroll_to_caret();
                lines.text.show_hint();
            }
        }
    }

    /// The ink of what the field says.
    fn set_ink(&self, color: &NSColor) {
        match self {
            Self::Text(field) => field.setTextColor(Some(color)),
            Self::Secret(field) => field.setTextColor(Some(color)),
            Self::Lines(lines) => {
                lines.text.setTextColor(Some(color));
                lines.text.setInsertionPointColor(Some(color));
            }
        }
    }

    /// Whether a screen reader reads the field (it does not where the page shut it out).
    fn set_read(&self, read: bool) {
        match self {
            // The cell is what is read of the field; the view stays what AppKit makes it (none).
            Self::Text(_) | Self::Secret(_) => {
                if let Some(cell) = self.text_field().and_then(|field| field.cell()) {
                    cell.setAccessibilityElement(read);
                }
            }
            Self::Lines(lines) => lines.text.setAccessibilityElement(read),
        }
    }

    /// What a screen reader is given of the field ([`read_of`]): the text view of lines, which is
    /// a text area to it (the scroll view around it is no element).
    fn read(&self) -> Retained<AnyObject> {
        match self {
            Self::Text(field) => read_of(field),
            Self::Secret(field) => read_of(field),
            // SAFETY: an upcast to the root class.
            Self::Lines(lines) => unsafe { Retained::cast_unchecked(lines.text.clone()) },
        }
    }

    /// Everything of the field that stands for the page to a screen reader: its view and what is
    /// read of it.
    fn owned(&self) -> Vec<Retained<AnyObject>> {
        // SAFETY: an upcast to the root class.
        let view: Retained<AnyObject> = unsafe { Retained::cast_unchecked(self.retain_view()) };
        vec![view, self.read()]
    }

    /// The view the field puts in the host, retained.
    fn retain_view(&self) -> Retained<NSView> {
        let view: &NSView = self;
        view.retain()
    }

    /// What the field holds of `text`: a field of one line its line breaks as spaces
    /// ([`one_line`]), another the text itself.
    fn held(&self, text: &str) -> String {
        self.ivars()
            .one_line
            .get()
            .then(|| one_line(text))
            .flatten()
            .unwrap_or_else(|| text.to_owned())
    }

    /// Selects `(location, length)` (UTF-16 units) in the field: at once where the keyboard is in
    /// it (and kept as what the page asked, for a click still going on: see
    /// [`keep_asked_selection`]), else when the page sends it the keyboard
    /// ([`FieldView::select_waiting`]). Lines have their selection with or without the keyboard (a
    /// text view is its own editor and AppKit selects nothing of it when it comes), and show it.
    fn select(&self, range: (usize, usize)) {
        if let Self::Lines(lines) = self {
            lines.text.setSelectedRange(NSRange::new(range.0, range.1));
            lines.text.scroll_to_caret();
            if self.editing() {
                self.ivars().asked.set(Some(range));
            }
            return;
        }
        match self.text_field().and_then(|field| field.currentEditor()) {
            Some(editor) => {
                editor.setSelectedRange(NSRange::new(range.0, range.1));
                self.ivars().asked.set(Some(range));
            }
            None => self.ivars().selection.set(Some(range)),
        }
    }

    /// The selection a fill left for the keyboard, put where the keyboard now is: after AppKit made
    /// its own (it selects all of a field it gives the keyboard to).
    fn select_waiting(&self) {
        let Some(field) = self.text_field() else {
            return;
        };
        if let (Some(range), Some(editor)) = (self.ivars().selection.take(), field.currentEditor())
        {
            editor.setSelectedRange(NSRange::new(range.0, range.1));
        }
    }

    /// Where the keyboard is in the field, as the kit's rule of keys asks it ([`FieldState`]), in
    /// UTF-16 units; `None` where the keyboard is not in it.
    pub(crate) fn state(&self, kind: FieldKind) -> Option<FieldState> {
        let editor = match self {
            Self::Lines(lines) => lines_editor(&lines.text)?,
            _ => self.text_field()?.currentEditor()?,
        };
        let range = editor.selectedRange();
        Some(FieldState {
            kind,
            length: editor.string().length(),
            selection: range.location..range.location + range.length,
        })
    }

    /// The height of one line of the field, as it draws it, points.
    fn line_height(&self) -> f64 {
        match self {
            Self::Lines(lines) => lines_line(&lines.text),
            _ => self.text_field().map_or(0.0, line_height),
        }
    }
}

/// Whether `view` is the window's first responder.
fn first_responder(view: &NSView) -> bool {
    view.window()
        .and_then(|window| window.firstResponder())
        .is_some_and(|responder| {
            std::ptr::eq(
                Retained::as_ptr(&responder).cast::<()>(),
                std::ptr::from_ref(view).cast::<()>(),
            )
        })
}

/// The editor of a field of lines while the keyboard is in it: the text view itself.
fn lines_editor(text: &PageLinesText) -> Option<Retained<NSText>> {
    first_responder(text).then(|| Retained::into_super(Retained::into_super(text.retain())))
}

/// The height of one line of a field of lines: the page's ([`PageTextFieldIvars::line`]) once the
/// field is placed, before that its text view's own, which has no leading (the cell of a text field
/// has none either: Monaco's 12.5 points have 16 in both, and some faces a point less than the cell).
fn lines_line(text: &PageLinesText) -> f64 {
    match text.ivars().line.get() {
        line if line > 0.0 => line,
        _ => lines_natural_line(text),
    }
}

/// The height of one line of a field of lines as its text view would lay it out.
fn lines_natural_line(text: &PageLinesText) -> f64 {
    let Some(font) = text.font() else {
        return 0.0;
    };
    // SAFETY: the text view was made with its layout manager.
    match unsafe { text.layoutManager() } {
        Some(manager) => manager.defaultLineHeightForFont(&font),
        None => 0.0,
    }
}

/// The line of a text field's cell in `family` at `size`, and where its baseline stands in it:
/// what a field of lines is laid out in, so that the two stand alike in every face.
fn cell_line(mtm: MainThreadMarker, family: Option<&str>, size: f64) -> (f64, f64) {
    let probe = field_of(mtm, FieldKind::Line, Weak::new(), family, size);
    let line = probe.line_height();
    probe.setFrame(NSRect::new(NSPoint::ZERO, NSSize::new(100.0, line)));
    // SAFETY: NSView's `firstBaselineOffsetFromTop`, which takes nothing and gives a CGFloat.
    let baseline: f64 = unsafe { msg_send![&*probe, firstBaselineOffsetFromTop] };
    (line, baseline)
}

impl PageLinesText {
    /// AppKit is in one of this view's methods (a key, a press) when the page is told, and the page
    /// may answer by taking the field away (a reply to Esc that closes it): the view (and the text
    /// it holds) lives to the end of the event, as a text field's editor outlives its field.
    fn outlive_the_event(&self) {
        let _ = Retained::autorelease_ptr(self.retain());
    }

    /// What was typed is no more to be undone: ⌘Z would replay a range of a text that is another now
    /// and AppKit raises. The undo manager is the field's own (the window's would lose what the
    /// other fields in it have typed too, and `removeAllActions` of an open group of the event
    /// raises when the event ends): it is let go, and the next typing is in a new one.
    fn forget_undo(&self) {
        self.ivars().undo.replace(None);
        self.breakUndoCoalescing();
    }

    /// A press in the room outside what the box shows (its border: the scroll view is the padding
    /// box): it is the field's (`FieldsHost`), and the caret goes to the nearest point of the text
    /// box in view, not to where the point is in the text (a box scrolled down has a line past its
    /// edge there, and one at the edge of the padding box is shown only in part). Whether it was
    /// one: no tracking of the press follows then.
    fn press_in_padding(&self, event: &NSEvent) -> bool {
        let at = self.convertPoint_fromView(event.locationInWindow(), None);
        let seen = self.visibleRect();
        if inside(seen, at) || seen.size.width < 1.0 || seen.size.height < 1.0 {
            return false;
        }
        // The nearest point of the text box in view, not of the padding box the box shows: the
        // padding at the edge of a box scrolled to the middle shows a sliver of the line past it, and
        // the caret goes to a line that is seen.
        let [_, top, _, bottom] = self.ivars().pad.get();
        let (low, high) = (
            seen.origin.y + top,
            seen.origin.y + seen.size.height - bottom,
        );
        let (low, high) = if high - low >= 1.0 {
            (low, high)
        } else {
            (seen.origin.y, seen.origin.y + seen.size.height)
        };
        let near = NSPoint::new(
            at.x.clamp(seen.origin.x, seen.origin.x + seen.size.width - 1.0),
            at.y.clamp(low, high - 1.0),
        );
        if let Some(window) = self.window() {
            let itself: &NSResponder = self;
            window.makeFirstResponder(Some(itself));
        }
        let index = self.characterIndexForInsertionAtPoint(near);
        // A line in view, in part or whole, as it is for a press in the box: no scrolling.
        self.setSelectedRange(NSRange::new(index, 0));
        true
    }

    /// The line the caret is in, in view, with the padding above and below it (the box's text
    /// stands in its text box and the padding scrolls with it: the last line is scrolled to with
    /// the padding under it, and the first with the padding over it). A box too short for the line
    /// and its padding keeps the line whole and as much of the padding as is left.
    fn scroll_to_caret(&self) {
        // SAFETY: the text view was made with its layout manager and container.
        let (Some(manager), Some(container)) = (unsafe { self.layoutManager() }, unsafe {
            self.textContainer()
        }) else {
            return;
        };
        // The line the caret is in, as laid out now: a layout still to be done (the text was just
        // changed) would have the line break at the end of the text not yet in the last line.
        manager.ensureLayoutForTextContainer(&container);
        let at = self.selectedRange().location;
        let length = self.string().length();
        let line = if at >= length && manager.extraLineFragmentRect().size.height > 0.0 {
            manager.extraLineFragmentRect()
        } else if length == 0 {
            return;
        } else {
            let glyph = manager.glyphIndexForCharacterAtIndex(at.min(length - 1));
            // SAFETY: a null effective range is allowed.
            unsafe {
                manager.lineFragmentRectForGlyphAtIndex_effectiveRange(glyph, std::ptr::null_mut())
            }
        };
        let [_, top, _, bottom] = self.ivars().pad.get();
        let seen = self
            .enclosingScrollView()
            .map_or(0.0, |scroll| scroll.contentSize().height);
        let spare = (seen - line.size.height).max(0.0);
        let (above, below) = if top + bottom <= spare || top + bottom <= 0.0 {
            (top, bottom)
        } else {
            let share = spare / (top + bottom);
            (top * share, bottom * share)
        };
        let origin = self.textContainerOrigin();
        let shown = NSRect::new(
            NSPoint::new(0.0, line.origin.y + origin.y - above),
            NSSize::new(1.0, line.size.height + above + below),
        );
        self.scrollRectToVisible(shown);
    }

    /// The hint is there while the text is not.
    fn show_hint(&self) {
        if let Some((_, hint)) = self.ivars().hint.borrow().as_ref() {
            hint.setHidden(self.string().length() > 0);
        }
    }
}

/// What a page's field keeps: its name, and the fields it belongs to (told when the person
/// clicks into it).
pub(crate) struct PageTextFieldIvars {
    id: Cell<Option<Id>>,
    /// What a fill selected while the keyboard was elsewhere, `(location, length)` in UTF-16 units:
    /// put in the field when the page's reply sends the keyboard to it.
    selection: Cell<Option<(usize, usize)>>,
    /// What a fill selected while the keyboard was in the field, since a press in it began: put
    /// there again when a press that gave it the keyboard is over.
    asked: Cell<Option<(usize, usize)>>,
    /// The time of the press that gave the field the keyboard: AppKit makes the field the first
    /// responder as the button goes down, before its `mouseDown:`, so the press is known by the
    /// event it was given the keyboard in, not by whether the field had an editor when it began.
    given_by_press: Cell<Option<f64>>,
    fields: RefCell<Weak<PageFields>>,
    /// The field is one line (a line or a secret, not lines): it takes no line break.
    one_line: Cell<bool>,
    /// The room the page laid out for it, from the pane's top: the pointer's, whose text stands in
    /// a part of it ([`FieldsHost`]'s hit test).
    room: Cell<NSRect>,
    /// What a field of lines says while it is empty: a text view has no hint of its own, so a text
    /// field with the words stands over its text, which is the one that draws them, as a field of
    /// text's own are drawn (the ink of a hint is AppKit's, whatever the theme, and what a text
    /// field draws it in is not a colour to name: the cell alone draws it lighter).
    hint: RefCell<Option<(Retained<NSString>, Retained<PageHintField>)>>,
    /// What a field of lines' text is held in: its text view does not hold the storage of a stack
    /// made by hand (the storage holds the layout manager, which holds the container), and the text
    /// view must not outlive it (a screen reader may ask it after the page took the field away).
    storage: RefCell<Option<Retained<NSTextStorage>>>,
    /// What a field of lines' typing is undone with ([`PageLinesText::forget_undo`]).
    undo: RefCell<Option<Retained<NSUndoManager>>>,
    /// How tall a line of a field of lines is, and where its baseline stands in it, points: the
    /// cell's of a field of text in the same face, which the page was told ([`line_of`]); 0 while
    /// the field has no place yet, and its text view's own lines stand.
    line: Cell<f64>,
    baseline: Cell<f64>,
    /// How far a field of lines' text stands in from the edges of its box (the padding box: what the
    /// scroll view shows), `[left, top, right, bottom]` in points, the page's: a text view insets
    /// by one size on both sides, which is not what the page lays out ([`PageLinesText`]).
    pad: Cell<[f64; 4]>,
}

/// Runs `body`, which is the part of a field's method that reaches the page or AppKit's views of
/// it, in the page's error boundary: AppKit calls these methods, and a panic or an exception that
/// unwound out of them would unwind through AppKit's frames. A failure is the
/// page's and no more ([`bt_shell_common::native_view::PageHandle::guard`]); the answer is
/// `otherwise` then. Where the page is down already there is nothing to guard.
fn guarded<R>(ivars: &PageTextFieldIvars, what: &str, otherwise: R, body: impl FnOnce() -> R) -> R {
    let handle = ivars
        .fields
        .borrow()
        .upgrade()
        .and_then(|fields| fields.pane.load())
        .and_then(|pane| pane.page_handle());
    match handle {
        Some(handle) => handle.guard(what, body).unwrap_or(otherwise),
        None => body(),
    }
}

/// What the program whose view the field is in calls itself, for what is made to fail by name.
fn program_of(ivars: &PageTextFieldIvars) -> Option<String> {
    ivars
        .fields
        .borrow()
        .upgrade()
        .and_then(|fields| fields.pane.load())
        .and_then(|pane| pane.view_program_name())
}

/// The keyboard came to a field (the person clicked into it, or the page sent it) and AppKit's own
/// `becomeFirstResponder` says whether it `took` it: the page's fields are told which.
fn keyboard_came(ivars: &PageTextFieldIvars, took: bool) {
    if took && let (Some(id), Some(fields)) = (ivars.id.get(), ivars.fields.borrow().upgrade()) {
        // What a fill selected waits for the page's own move of the keyboard
        // (`PageFields::send_keyboard`); the person's click or Tab makes its own.
        if !fields.moving.get() {
            ivars.selection.set(None);
        }
        fields.focused_by_person(id);
    }
}

/// A click that gave the field the keyboard is over: what the page selected in it meanwhile (its
/// reply to the keyboard coming, which a display link brings in the middle of the click: the
/// tracking of the press runs until the button is up) is selected again, for the press's own
/// selection, made from where it began in the text the field held then, is made last. Not for
/// a press that went on to a drag (the dragged selection is the person's), nor for a click in
/// a field that had the keyboard already (its caret is where it was put).
fn keep_asked_selection(
    field: &NSView,
    ivars: &PageTextFieldIvars,
    down: NSPoint,
    editor: Option<Retained<NSText>>,
) {
    let Some(range) = ivars.asked.take() else {
        return;
    };
    let (Some(window), Some(editor)) = (field.window(), editor) else {
        return;
    };
    let now = window.mouseLocationOutsideOfEventStream();
    let dragged = (now.x - down.x).abs() > 4.0 || (now.y - down.y).abs() > 4.0;
    if !dragged && range.0 + range.1 <= editor.string().length() {
        editor.setSelectedRange(NSRange::new(range.0, range.1));
    }
}

/// The keyboard came to a field (`becomeFirstResponder` of its class answered `took`): taken by a
/// press of the main button, that press's own end must not undo what the page selects in answer to
/// the keyboard coming ([`press_in`]); and the page's fields are told which ([`keyboard_came`]).
fn keyboard_taken(mtm: MainThreadMarker, ivars: &PageTextFieldIvars, took: bool) {
    if took {
        let press = NSApplication::sharedApplication(mtm)
            .currentEvent()
            .filter(|event| event.r#type() == NSEventType::LeftMouseDown)
            .map(|event| event.timestamp());
        ivars.given_by_press.set(press);
        ivars.asked.set(None);
    }
    guarded(ivars, "a field's keyboard", (), || {
        keyboard_came(ivars, took);
    });
}

/// A press in a field (`mouseDown:`), `press` being its class's own. Where it gives the field the
/// keyboard, the page is told (see [`keyboard_taken`]) and answers with the whole address filled in
/// and selected, which arrives before the button is up; the press's own tracking would then end by
/// selecting from where it began, so what the page selected is put back when it is over. `editor` is
/// the field's editor: there while the keyboard is in the field.
fn press_in(
    view: &NSView,
    ivars: &PageTextFieldIvars,
    event: &NSEvent,
    editor: &dyn Fn() -> Option<Retained<NSText>>,
    press: &dyn Fn(),
) {
    let gave_the_keyboard = ivars.given_by_press.take() == Some(event.timestamp());
    let had_the_keyboard = editor().is_some() && !gave_the_keyboard;
    let down = event.locationInWindow();
    if had_the_keyboard {
        ivars.asked.set(None);
    }
    // In the page's boundary, as `becomeFirstResponder` is: the press's tracking is AppKit's and so
    // is what it raises.
    guarded(ivars, "a press in a field", (), press);
    if !had_the_keyboard {
        guarded(ivars, "a press in a field", (), || {
            keep_asked_selection(view, ivars, down, editor());
        });
    }
}

/// The pane a field of the page is in.
fn pane_of(ivars: &PageTextFieldIvars) -> Option<Retained<TerminalPane>> {
    ivars
        .fields
        .borrow()
        .upgrade()
        .and_then(|fields| fields.pane.load())
}

/// What the editor of a field puts in place of `words`, where the field is one line and they hold
/// a line break: the words with each break a space ([`one_line`]); `None` where AppKit's own
/// answer stands.
fn one_line_words(
    ivars: &PageTextFieldIvars,
    words: Option<&NSString>,
) -> Option<Retained<NSString>> {
    words
        .filter(|_| ivars.one_line.get())
        .and_then(|words| one_line(&words.to_string()))
        .map(|flat| NSString::from_str(&flat))
}

/// A class of a page's field ([`FieldView`]): `$name`, named `$objc` to the runtime, under the
/// superclasses `$super`. Both classes keep the same ivars and answer the same two messages, with
/// the same functions ([`keyboard_came`], [`one_line_words`]); a second copy of either would drift.
macro_rules! page_field_class {
    ($name:ident, $objc:literal, $($super:ident),+) => {
        define_class!(
            // SAFETY: NSTextField and NSSecureTextField are designed for subclassing; the class
            // implements no `Drop` and is born with `initWithFrame:`.
            #[unsafe(super($($super),+))]
            #[thread_kind = MainThreadOnly]
            #[name = $objc]
            #[ivars = PageTextFieldIvars]
            pub(crate) struct $name;

            impl $name {
                /// The keyboard comes to the field: the page's fields are told which.
                #[unsafe(method(becomeFirstResponder))]
                fn become_first_responder(&self) -> bool {
                    // The superclass's own call is where AppKit raised on 2026-11-23: it is in the
                    // page's boundary, and a failure of it is a field that did not take the keyboard.
                    let took = guarded(self.ivars(), "a field's keyboard", false, || {
                        // A development copy told to fail the keyboard's handover does it here,
                        // where AppKit's own chain does (`makeFirstResponder` calls this).
                        crate::dev_fault::keyboard_handed(program_of(self.ivars()).as_deref());
                        // SAFETY: the superclass's own `becomeFirstResponder`, which takes nothing
                        // and answers whether the field took the keyboard.
                        unsafe { msg_send![super(self), becomeFirstResponder] }
                    });
                    keyboard_taken(self.mtm(), self.ivars(), took);
                    took
                }

                /// A press in the field ([`press_in`]).
                #[unsafe(method(mouseDown:))]
                fn mouse_down(&self, event: &NSEvent) {
                    press_in(self, self.ivars(), event, &|| self.currentEditor(), &|| {
                        // SAFETY: the superclass's own `mouseDown:`, which takes the event and
                        // tracks the press until the button is up.
                        let _: () = unsafe { msg_send![super(self), mouseDown: event] };
                    });
                }

                /// What the person puts in a field of one line (a paste, a drop, what the keyboard
                /// makes) takes no line break: each line or paragraph separator becomes a space
                /// ([`one_line`]), as in AppKit's single-line cells. The field is not one of those
                /// (see [`text_field`]), so it does it itself: the words go in through the editor in
                /// place of the ones it was asked for, which leaves the caret after them and makes
                /// one step of undo, and the editor asks again with words that now pass.
                #[unsafe(method(textView:shouldChangeTextInRange:replacementString:))]
                fn should_change_text(
                    &self,
                    editor: &AnyObject,
                    range: NSRange,
                    words: Option<&NSString>,
                ) -> bool {
                    if let Some(flat) = one_line_words(self.ivars(), words) {
                        // SAFETY: a field's editor is an NSTextView, which takes
                        // `insertText:replacementRange:` (NSTextInputClient) with a string and a
                        // range of its own text: the range it asked this field about.
                        let _: () = unsafe {
                            msg_send![editor, insertText: &*flat, replacementRange: range]
                        };
                        false
                    } else {
                        // SAFETY: NSTextField's own answer to its editor (it implements the
                        // method), asked with what the editor asked.
                        unsafe {
                            msg_send![
                                super(self),
                                textView: editor,
                                shouldChangeTextInRange: range,
                                replacementString: words
                            ]
                        }
                    }
                }
            }
        );
    };
}

page_field_class!(
    PageTextField,
    "BateriPageTextField",
    NSTextField,
    NSControl,
    NSView,
    NSResponder,
    NSObject
);
page_field_class!(
    PageSecretField,
    "BateriPageSecretField",
    NSSecureTextField,
    NSTextField,
    NSControl,
    NSView,
    NSResponder,
    NSObject
);

define_class!(
    // SAFETY: NSTextField is designed for subclassing; the class implements no `Drop` and is born
    // with `initWithFrame:`.
    #[unsafe(super(NSTextField, NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPageHintField"]
    #[ivars = ()]
    /// The words a field of lines says while it is empty ([`PageTextFieldIvars::hint`]): a text
    /// field over its text view that nothing reaches. A press goes through it to the text view, and
    /// it takes no keyboard.
    pub(crate) struct PageHintField;

    impl PageHintField {
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            false
        }
    }
);

define_class!(
    // SAFETY: NSScrollView is designed for subclassing; the class implements no `Drop` and is born
    // with `initWithFrame:`.
    #[unsafe(super(NSScrollView, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPageLinesScroll"]
    #[ivars = ()]
    /// The scroll view of a field of lines. A wheel over a box whose text fits is the page's to
    /// scroll (a text field passes it up, and a scroll view would take it and bounce); a box that
    /// overflows scrolls. And the bar is the overlay the page's rooms count on, whatever the
    /// system's preference for scroll bars says.
    pub(crate) struct PageLinesScroll;

    impl PageLinesScroll {
        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let overflows = self
                .documentView()
                .is_some_and(|text| text.frame().size.height > self.contentSize().height + 0.5);
            // SAFETY: reading the next responder, on the main thread.
            let next = unsafe { self.nextResponder() };
            if overflows {
                // SAFETY: the superclass's own `scrollWheel:`, which takes the event.
                let _: () = unsafe { msg_send![super(self), scrollWheel: event] };
            } else if let Some(next) = next {
                next.scrollWheel(event);
            }
        }

        #[unsafe(method(setScrollerStyle:))]
        fn set_scroller_style(&self, _style: NSScrollerStyle) {
            // SAFETY: the superclass's own `setScrollerStyle:`, which takes a style.
            let _: () =
                unsafe { msg_send![super(self), setScrollerStyle: NSScrollerStyle::Overlay] };
        }
    }
);

define_class!(
    // SAFETY: NSTextView is designed for subclassing; the class implements no `Drop` and is born
    // with `initWithFrame:textContainer:`.
    #[unsafe(super(NSTextView, NSText, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPageLinesText"]
    #[ivars = PageTextFieldIvars]
    /// The text view of a field of lines: the same keyboard and press as the fields of text
    /// ([`page_field_class`]), and what a field's delegate does for them, done here — a text view
    /// is its own editor and sends its commands and changes to itself first.
    pub(crate) struct PageLinesText;

    impl PageLinesText {
        /// The keyboard comes to the field: the page's fields are told which.
        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            self.outlive_the_event();
            let took = guarded(self.ivars(), "a field's keyboard", false, || {
                crate::dev_fault::keyboard_handed(program_of(self.ivars()).as_deref());
                // SAFETY: the superclass's own `becomeFirstResponder`, which takes nothing and
                // answers whether the view took the keyboard.
                unsafe { msg_send![super(self), becomeFirstResponder] }
            });
            if took {
                // As the editor of a field of text takes what is dropped on it while the field is
                // typed in (text, a file as its path), and not before: the page takes a drop on a
                // field that is not.
                self.registerForDraggedTypes(&self.acceptableDragTypes());
            }
            keyboard_taken(self.mtm(), self.ivars(), took);
            took
        }

        /// The keyboard leaves the field: what a drop on it is, is the page's again.
        #[unsafe(method(resignFirstResponder))]
        fn resign_first_responder(&self) -> bool {
            self.outlive_the_event();
            let left = guarded(self.ivars(), "a field's keyboard", true, || {
                // SAFETY: the superclass's own `resignFirstResponder`, which takes nothing and
                // answers whether the view gave the keyboard up.
                unsafe { msg_send![super(self), resignFirstResponder] }
            });
            if left {
                self.unregisterDraggedTypes();
            }
            left
        }

        /// Where the text starts in the text view: the padding box's left and top padding. The
        /// view's inset is the same on both sides (half of the two paddings, so that the frame is the
        /// text, the paddings and no more, and the container the box's width); the text starts at
        /// the padding of its own.
        #[unsafe(method(textContainerOrigin))]
        fn text_container_origin(&self) -> NSPoint {
            let [left, top, ..] = self.ivars().pad.get();
            NSPoint::new(left, top)
        }

        /// A press in the field ([`press_in`]).
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            self.outlive_the_event();
            press_in(self, self.ivars(), event, &|| lines_editor(self), &|| {
                if !self.press_in_padding(event) {
                    // SAFETY: the superclass's own `mouseDown:`, which takes the event and tracks
                    // the press until the button is up.
                    let _: () = unsafe { msg_send![super(self), mouseDown: event] };
                }
            });
        }

        /// A command of the keyboard (⏎, Tab, Esc, an arrow): the page's where the kit's rule of
        /// the keys of a field says so (`TerminalPane::page_field_command`), else the text view's own.
        #[unsafe(method(doCommandBySelector:))]
        fn do_command(&self, command: Sel) {
            self.outlive_the_event();
            let handled = guarded(self.ivars(), "a field's command", false, || {
                let control: &AnyObject = self;
                pane_of(self.ivars())
                    .and_then(|pane| pane.page_field_do_command(control, self, command))
                    .unwrap_or(false)
            });
            if !handled {
                guarded(self.ivars(), "a field's command", (), || {
                    // SAFETY: the superclass's own `doCommandBySelector:`, which takes a selector.
                    let _: () = unsafe { msg_send![super(self), doCommandBySelector: command] };
                });
            }
        }

        /// The field's own undo manager, which Edit's Undo asks of the keyboard's view.
        #[unsafe(method_id(undoManager))]
        fn undo_manager(&self) -> Option<Retained<NSUndoManager>> {
            let mut own = self.ivars().undo.borrow_mut();
            Some(own.get_or_insert_with(|| NSUndoManager::new(self.mtm())).clone())
        }

        /// The person changed the text (typing, a paste, an undo): the caret's line is in view, the
        /// hint is drawn or not, and the page is told what the field holds.
        #[unsafe(method(didChangeText))]
        fn did_change_text(&self) {
            self.outlive_the_event();
            guarded(self.ivars(), "a field's text", (), || {
                // SAFETY: the superclass's own `didChangeText`, which takes nothing.
                let _: () = unsafe { msg_send![super(self), didChangeText] };
                self.scroll_to_caret();
                self.show_hint();
            });
            if let Some(pane) = pane_of(self.ivars()) {
                let control: &AnyObject = self;
                pane.page_field_changed(control);
            }
        }
    }

    unsafe impl NSObjectProtocol for PageLinesText {}

    unsafe impl NSLayoutManagerDelegate for PageLinesText {
        /// Every line of the text is as tall as the page was told a line is, with its baseline where
        /// a text field's cell has it: a text view's line is the font's with its leading left out,
        /// which is the cell's in some faces and a point less in others, and rows are laid out as
        /// that many of the page's lines.
        #[unsafe(method(layoutManager:shouldSetLineFragmentRect:lineFragmentUsedRect:baselineOffset:inTextContainer:forGlyphRange:))]
        unsafe fn should_set_line_fragment(
            &self,
            _manager: &NSLayoutManager,
            rect: NonNull<NSRect>,
            used: NonNull<NSRect>,
            baseline: NonNull<CGFloat>,
            _container: &NSTextContainer,
            _glyphs: NSRange,
        ) -> bool {
            let line = self.ivars().line.get();
            // No `return`: `define_class!` converts the `bool` at the end of the body.
            if line > 0.0 {
                // SAFETY: AppKit gives pointers to values of its own that it takes back changed,
                // when this answers `true`.
                unsafe {
                    (*rect.as_ptr()).size.height = line;
                    (*used.as_ptr()).size.height = line;
                    *baseline.as_ptr() = self.ivars().baseline.get();
                }
                true
            } else {
                false
            }
        }
    }
);

/// What an element a screen reader is given keeps: where a press on it lands (a button's middle,
/// in the pane's points from its top), and the pane it presses.
pub(crate) struct PageElementIvars {
    press: Cell<Option<(f32, f32)>>,
    pane: WeakId<TerminalPane>,
    /// The page's name for an element that is partly out of view, which the page is asked to show when
    /// the screen reader moves to it (`reveal`).
    target: Cell<Option<Id>>,
    /// What the element was made as: an element is given again to the same item of the next outline only
    /// while it is ([`shape_of`]).
    shape: Shape,
    /// Whether the row of an outline is open now, where it can be opened (a tree's row that holds
    /// others): what a screen reader's request to open or close it is measured against.
    disclosed: Cell<Option<bool>>,
    /// A table's cells, by the place of their row among its rows and of the cell in its row (from 0),
    /// which `accessibilityCellForColumn:row:` answers from.
    cells: RefCell<Vec<(i64, i64, Retained<AnyObject>)>>,
}

/// What an element was made as, of what cannot be undone by setting it again: its role, and which of
/// the numbers, ranges and flags that are set only where the page says them it has. What can be set to
/// nothing (the value, the children it selects, the rows it discloses, the actions, the headers) is
/// set again each time, so that a row that opens or a box that is checked is the same element.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Shape {
    role: AccessibleRole,
    said: u32,
}

fn shape_of(item: &Accessible, node: &AxNode) -> Shape {
    let state = &item.state;
    let said = [
        state.expanded.is_some(),
        node.role_description.is_some(),
        node.help.is_some(),
        node.index.is_some(),
        node.disclosure_level.is_some(),
        node.disclosed_by.is_some(),
        node.row_count.is_some(),
        node.column_count.is_some(),
        node.row_range.is_some(),
        node.column_range.is_some(),
        node.sort.is_some(),
    ]
    .iter()
    .enumerate()
    .fold(0, |said, (at, has)| said | (u32::from(*has) << at));
    Shape {
        role: item.role,
        said,
    }
}

define_class!(
    // SAFETY: NSAccessibilityElement is designed for subclassing; PageElement implements no `Drop`
    // and is born with `init`.
    #[unsafe(super(NSAccessibilityElement, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPageElement"]
    #[ivars = PageElementIvars]
    pub(crate) struct PageElement;

    impl PageElement {
        /// The screen reader presses a button of the page: a click on its middle, as the
        /// pointer's would be.
        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_press(&self) -> bool {
            // No `return`: `define_class!` converts the `bool` at the end of the body.
            match (self.ivars().press.get(), self.ivars().pane.load()) {
                (Some((x, y)), Some(pane)) => {
                    pane.press_page_at(Point::new(x, y));
                    true
                }
                _ => false,
            }
        }

        /// The screen reader opens or closes a row of an outline: the page is pressed there, which is
        /// what a click on a row that holds others does (it opens or closes it). A request that says
        /// what the row is already is not one.
        #[unsafe(method(setAccessibilityDisclosed:))]
        fn set_accessibility_disclosed(&self, open: bool) {
            let was = self.ivars().disclosed.get();
            // SAFETY: NSAccessibilityElement's own setter, with the same argument.
            let _: () = unsafe { msg_send![super(self), setAccessibilityDisclosed: open] };
            if was.is_some_and(|was| was != open)
                && let (Some((x, y)), Some(pane)) =
                    (self.ivars().press.get(), self.ivars().pane.load())
            {
                pane.press_page_at(Point::new(x, y));
            }
        }

        /// A table's cell at a column and a row (each from 0, among the rows and the cells the page
        /// has built), which a screen reader asks for to go from cell to cell.
        #[unsafe(method_id(accessibilityCellForColumn:row:))]
        fn accessibility_cell(&self, column: NSInteger, row: NSInteger) -> Option<Retained<AnyObject>> {
            self.ivars()
                .cells
                .borrow()
                .iter()
                .find(|(at_row, at_column, _)| *at_row == row as i64 && *at_column == column as i64)
                .map(|(_, _, cell)| cell.clone())
        }

        /// The screen reader moves to the element, or away from it: where it moves to, the page is
        /// asked to show it (a row that stands half out of what its list shows, or one the list has
        /// built past its edge), and to move its cursor there. AppKit has no request for scrolling an
        /// element into view that an element can answer.
        #[unsafe(method(setAccessibilityFocused:))]
        fn set_accessibility_focused(&self, focused: bool) {
            // SAFETY: NSAccessibilityElement's own setter, with the same argument.
            let _: () = unsafe { msg_send![super(self), setAccessibilityFocused: focused] };
            if let (true, Some(target), Some(pane)) =
                (focused, self.ivars().target.get(), self.ivars().pane.load())
            {
                pane.page_reveal(target, None);
            }
        }
    }
);

/// What a text area a screen reader is given for an editor keeps: the editor's name, and the pane
/// whose page has it.
pub(crate) struct PageEditorElementIvars {
    id: Id,
    pane: WeakId<TerminalPane>,
}

define_class!(
    // SAFETY: NSAccessibilityElement is designed for subclassing; PageEditorElement implements no
    // `Drop` and is born with `init`.
    #[unsafe(super(NSAccessibilityElement, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPageEditorElement"]
    #[ivars = PageEditorElementIvars]
    pub(crate) struct PageEditorElement;

    impl PageEditorElement {
        /// The editor's text, as it is now (the page's editor keeps it, not the element).
        #[unsafe(method_id(accessibilityValue))]
        fn accessibility_value(&self) -> Option<Retained<NSString>> {
            self.contents()
                .map(|(text, _)| NSString::from_str(&text))
        }

        /// How long the text is, in the units of UTF-16 the ranges below count.
        #[unsafe(method(accessibilityNumberOfCharacters))]
        fn accessibility_number_of_characters(&self) -> NSInteger {
            NSInteger::try_from(self.length()).unwrap_or(NSInteger::MAX)
        }

        /// The main selection: where it starts and how long it is.
        #[unsafe(method(accessibilitySelectedTextRange))]
        fn accessibility_selected_text_range(&self) -> NSRange {
            self.contents()
                .map_or(NSRange::new(0, 0), |(_, (start, len))| NSRange::new(start, len))
        }

        /// What is selected.
        #[unsafe(method_id(accessibilitySelectedText))]
        fn accessibility_selected_text(&self) -> Option<Retained<NSString>> {
            self.selected_text()
        }

        /// What of it the screen reader may read as shown: all of it (the editor scrolls it).
        #[unsafe(method(accessibilityVisibleCharacterRange))]
        fn accessibility_visible_character_range(&self) -> NSRange {
            NSRange::new(0, self.length())
        }

        /// Whether the editor has the keyboard.
        #[unsafe(method(isAccessibilityFocused))]
        fn accessibility_focused(&self) -> bool {
            // No `return`: `define_class!` converts the `bool` at the end of the body.
            self.ivars()
                .pane
                .load()
                .and_then(|pane| pane.page_editing())
                .is_some_and(|editors| editors.focused() == Some(self.ivars().id))
        }
    }
);

impl PageEditorElement {
    /// The editor's text and its main selection (units of UTF-16), from the page's editors.
    fn contents(&self) -> Option<(String, (usize, usize))> {
        let pane = self.ivars().pane.load()?;
        pane.page_editors()?.contents(self.ivars().id)
    }

    /// How long its text is, units of UTF-16.
    fn length(&self) -> usize {
        self.contents()
            .map_or(0, |(text, _)| text.encode_utf16().count())
    }

    /// What it has selected.
    fn selected_text(&self) -> Option<Retained<NSString>> {
        let (text, (start, len)) = self.contents()?;
        let units: Vec<u16> = text.encode_utf16().skip(start).take(len).collect();
        Some(NSString::from_str(&String::from_utf16_lossy(&units)))
    }
}

/// The text area a screen reader is given for the editor `item` is: where it stands in the pane,
/// what the page says of it (its hint), and its text and selection read from the editor when asked.
fn editor_element(pane: &TerminalPane, item: &Accessible) -> Option<Retained<AnyObject>> {
    let id = item.id?;
    let element = PageEditorElement::alloc(pane.mtm()).set_ivars(PageEditorElementIvars {
        id,
        pane: WeakId::new(pane),
    });
    // SAFETY: `init` is NSObject's designated initializer and the ivars are set.
    let element: Retained<PageEditorElement> = unsafe { msg_send![super(element), init] };
    // SAFETY: the role is AppKit's own constant; the parent is the pane, a view that outlives the
    // element (the element goes with the page's outline).
    unsafe {
        element.setAccessibilityRole(Some(NSAccessibilityTextAreaRole));
        element.setAccessibilityParent(Some(pane));
    }
    element.setAccessibilityLabel(Some(&NSString::from_str(&item.words)));
    element.setAccessibilityEnabled(!item.state.disabled);
    // The pane's own coordinates grow upward, the page's downward.
    let rect = item.rect;
    let height = pane.bounds().size.height;
    element.setAccessibilityFrameInParentSpace(NSRect::new(
        NSPoint::new(f64::from(rect.x), height - f64::from(rect.y + rect.h)),
        NSSize::new(f64::from(rect.w), f64::from(rect.h)),
    ));
    Some(Retained::into_super(Retained::into_super(
        Retained::into_super(element),
    )))
}

/// Whether the keyboard of `window` is in an editor of a page of the kit: in the terminal's view of
/// a pane whose page has an editor with the keyboard. Edit's Undo is the editor's then, not the
/// application's undoing of a move.
pub(crate) fn keyboard_in_page_editor(window: &NSWindow) -> bool {
    window
        .firstResponder()
        .and_then(|responder| responder.downcast::<crate::view::BateriView>().ok())
        .and_then(|view| view.pane())
        .is_some_and(|pane| pane.page_editing().is_some())
}

/// The pane of the window's first responder, when a browser of its page has the keyboard.
fn pane_with_browser_keyboard(window: &NSWindow) -> Option<Retained<crate::pane::TerminalPane>> {
    window
        .firstResponder()
        .and_then(|responder| responder.downcast::<crate::view::BateriView>().ok())
        .and_then(|view| view.pane())
        .filter(|pane| pane.browser_has_keyboard())
}

/// Whether the keyboard is in a browser of a page: Edit's items are the browser's.
pub(crate) fn keyboard_in_browser(window: &NSWindow) -> bool {
    pane_with_browser_keyboard(window).is_some()
}

/// Edit ▸ Undo for the browser that has the keyboard. `false` where none has it.
pub(crate) fn undo_in_browser(window: &NSWindow) -> bool {
    pane_with_browser_keyboard(window).is_some_and(|pane| pane.browser_undo())
}

/// AppKit's role, and subrole where one says more, for what the page says an element is.
fn accessibility_role(
    role: AccessibleRole,
) -> (
    &'static NSAccessibilityRole,
    Option<&'static NSAccessibilitySubrole>,
) {
    // SAFETY: AppKit's own constants, read once each.
    unsafe {
        match role {
            AccessibleRole::Button => (NSAccessibilityButtonRole, None),
            AccessibleRole::Link => (NSAccessibilityLinkRole, None),
            AccessibleRole::Checkbox => (NSAccessibilityCheckBoxRole, None),
            AccessibleRole::Switch => (
                NSAccessibilityCheckBoxRole,
                Some(NSAccessibilitySwitchSubrole),
            ),
            AccessibleRole::Radio => (NSAccessibilityRadioButtonRole, None),
            AccessibleRole::Tab => (
                NSAccessibilityRadioButtonRole,
                Some(NSAccessibilityTabButtonSubrole),
            ),
            AccessibleRole::MenuItem => (NSAccessibilityMenuItemRole, None),
            AccessibleRole::Image => (NSAccessibilityImageRole, None),
            AccessibleRole::Progress => (NSAccessibilityProgressIndicatorRole, None),
            AccessibleRole::Slider => (NSAccessibilitySliderRole, None),
            AccessibleRole::Editor => (NSAccessibilityTextAreaRole, None),
            // The helper's page is a picture to AppKit; a group is what a screen reader skips into.
            AccessibleRole::Browser => (NSAccessibilityGroupRole, None),
            AccessibleRole::Text | AccessibleRole::Heading(_) | AccessibleRole::Field(_) => {
                (NSAccessibilityStaticTextRole, None)
            }
            // What the page says is no control to a person who reads by ear is not read at all (it is
            // not in the outline); a group is the plain container.
            AccessibleRole::None | AccessibleRole::Group | AccessibleRole::ListItem => {
                (NSAccessibilityGroupRole, None)
            }
            // A dialog inside a window is a group with the subrole Safari and Chrome give a web
            // page's: AppKit's own dialog subrole is a window's, and a screen reader takes what
            // carries it for a window. Its kind is said in words (`ax::plan` gives them as its role
            // description).
            AccessibleRole::Dialog => (
                NSAccessibilityGroupRole,
                Some(ns_string!("AXApplicationDialog")),
            ),
            AccessibleRole::AlertDialog => (
                NSAccessibilityGroupRole,
                Some(ns_string!("AXApplicationAlertDialog")),
            ),
            AccessibleRole::List | AccessibleRole::ListBox => (NSAccessibilityListRole, None),
            // An option is chosen and pressed as a row of a list is.
            AccessibleRole::ListOption | AccessibleRole::Row | AccessibleRole::TreeItem => {
                (NSAccessibilityRowRole, None)
            }
            AccessibleRole::Table => (NSAccessibilityTableRole, None),
            AccessibleRole::Cell | AccessibleRole::ColumnHeader => (NSAccessibilityCellRole, None),
            AccessibleRole::Tree => (NSAccessibilityOutlineRole, None),
            AccessibleRole::TabList => (NSAccessibilityTabGroupRole, None),
            // What is said in place: static text, the page's own words (`status` is read where it
            // stands, not announced; `alert` is announced as it comes up).
            AccessibleRole::Status | AccessibleRole::Alert => (NSAccessibilityStaticTextRole, None),
        }
    }
}

/// What a screen reader is given for an element of the page: the element made for it
/// ([`PageElement`], which can be given a place in a tree), or an object that is the page's already (a
/// field, an editor's text area).
pub(crate) enum Made {
    Page(Retained<PageElement>),
    Other(Retained<AnyObject>),
}

impl Made {
    fn object(&self) -> Retained<AnyObject> {
        match self {
            Self::Page(element) => {
                Retained::into_super(Retained::into_super(Retained::into_super(element.clone())))
            }
            Self::Other(object) => object.clone(),
        }
    }
}

/// What `at` holds that was made, in its order; of what it holds that was not made (a group that has no
/// element), what that holds is taken as held by `at`.
fn built_under(nodes: &[AxNode], made: &[Option<Made>], at: usize) -> Vec<usize> {
    let mut found = Vec::new();
    for child in &nodes[at].children {
        if made.get(*child).is_some_and(Option::is_some) {
            found.push(*child);
        } else {
            found.extend(built_under(nodes, made, *child));
        }
    }
    found
}

/// The element of the nearest of what holds `at`, at any depth, that was made.
fn nearest(nodes: &[AxNode], made: &[Option<Made>], at: usize) -> Option<usize> {
    let mut outer = nodes.get(at)?.parent;
    while let Some(held) = outer {
        if made.get(held).is_some_and(Option::is_some) {
            return Some(held);
        }
        outer = nodes.get(held)?.parent;
    }
    None
}

/// Where `item` stands, in the pane's coordinates, which grow upward where the page's go downward.
fn frame_in_pane(pane: &TerminalPane, item: &Accessible) -> NSRect {
    rect_in_pane(pane, item.rect)
}

/// Where `rect`, in the page's points (from the pane's top), stands in the pane's own coordinates
/// (from its bottom): the one place the page's rooms are turned over.
pub(crate) fn rect_in_pane(
    pane: &TerminalPane,
    rect: bt_shell_common::native_view::Rect,
) -> NSRect {
    let height = pane.bounds().size.height;
    NSRect::new(
        NSPoint::new(f64::from(rect.x), height - f64::from(rect.y + rect.h)),
        NSSize::new(f64::from(rect.w), f64::from(rect.h)),
    )
}

/// What a screen reader is given of the page's field `field`: its cell, which is what AppKit reads of
/// a text field (a text field, a secure one for a password, with its value and selection), as the
/// view itself is no element and has no role. The view where it has no cell.
fn read_of(field: &NSTextField) -> Retained<AnyObject> {
    match field.cell() {
        Some(cell) => Retained::into_super(Retained::into_super(cell)),
        None => {
            let view: Retained<NSTextField> = field.retain();
            Retained::into_super(Retained::into_super(Retained::into_super(
                Retained::into_super(Retained::into_super(view)),
            )))
        }
    }
}

/// The innermost of `objects` and what they hold (an element, a field) whose frame on the screen
/// holds `point`: of two that overlap, the later in the reading, which is drawn over the other.
pub(crate) fn element_at(
    objects: &[Retained<AnyObject>],
    point: NSPoint,
) -> Option<Retained<AnyObject>> {
    objects.iter().rev().find_map(|object| {
        // SAFETY: each is an element of the page's tree or an AppKit view, and both answer
        // NSAccessibility's `accessibilityFrame` and `accessibilityChildren`; what is asked first
        // is whether this one does.
        unsafe {
            let answers: bool = msg_send![&**object, respondsToSelector: sel!(accessibilityFrame)];
            if !answers {
                return None;
            }
            let frame: NSRect = msg_send![&**object, accessibilityFrame];
            let inside = point.x >= frame.origin.x
                && point.y >= frame.origin.y
                && point.x < frame.origin.x + frame.size.width
                && point.y < frame.origin.y + frame.size.height;
            if !inside {
                return None;
            }
            let held: Option<Retained<NSArray>> = msg_send![&**object, accessibilityChildren];
            let inner = held.and_then(|held| element_at(&held.to_vec(), point));
            Some(inner.unwrap_or_else(|| object.clone()))
        }
    })
}

/// An array of the elements made for `places`.
fn array_of(made: &[Option<Made>], places: &[usize]) -> Retained<NSArray> {
    let objects: Vec<Retained<AnyObject>> = places
        .iter()
        .filter_map(|at| made.get(*at).and_then(Option::as_ref).map(Made::object))
        .collect();
    NSArray::from_retained_slice(&objects)
}

/// The tree the plan says, put on the elements: who holds whom (with each place in its holder's
/// space), and what a table, an outline, a row, a header, a cell and a list say of themselves; the
/// kind the page gave, the help, the commands that can be done on it.
fn relate(pane: &TerminalPane, items: &[Accessible], nodes: &[AxNode], made: &[Option<Made>]) {
    for (at, (item, node)) in items.iter().zip(nodes).enumerate() {
        // A field or an editor's text area (not made here) stands under what holds it in the tree, as
        // that holds it among its children.
        if let Some(Made::Other(object)) = made.get(at).and_then(Option::as_ref) {
            if let Some(held) =
                nearest(nodes, made, at).and_then(|holder| made.get(holder)?.as_ref())
            {
                let parent = held.object();
                // SAFETY: `setAccessibilityParent:` is the setter of NSAccessibility's property of
                // that name, which an NSView and an NSAccessibilityElement answer; what is asked
                // first is whether this object does, and the argument is an element of this tree.
                unsafe {
                    let answers: bool =
                        msg_send![&**object, respondsToSelector: sel!(setAccessibilityParent:)];
                    if answers {
                        let _: () = msg_send![&**object, setAccessibilityParent: &*parent];
                    }
                }
            } else {
                // Held by nothing made now: the pane's, as it was before something held it.
                // SAFETY: as above, with the pane for the parent.
                unsafe {
                    let answers: bool =
                        msg_send![&**object, respondsToSelector: sel!(setAccessibilityParent:)];
                    if answers {
                        let _: () = msg_send![&**object, setAccessibilityParent: pane];
                    }
                }
            }
            continue;
        }
        let Some(Made::Page(element)) = made.get(at).and_then(Option::as_ref) else {
            continue;
        };
        // SAFETY: each setter takes what AppKit's own elements of that property are given: elements
        // of this tree (the made ones), numbers and ranges; the parent and the children outlive
        // them (they are the page's outline's, and go with it).
        unsafe {
            if let Some((holder, held)) = nearest(nodes, made, at)
                .and_then(|holder| Some((holder, made.get(holder)?.as_ref()?)))
            {
                element.setAccessibilityParent(Some(&*held.object()));
                // In its holder's space: where it stands less where the holder does.
                let (mine, theirs) = (
                    frame_in_pane(pane, item),
                    frame_in_pane(pane, &items[holder]),
                );
                element.setAccessibilityFrameInParentSpace(NSRect::new(
                    NSPoint::new(
                        mine.origin.x - theirs.origin.x,
                        mine.origin.y - theirs.origin.y,
                    ),
                    mine.size,
                ));
            }
            // What it holds that was made; what a node held that was not made holds stands under it.
            let children = built_under(nodes, made, at);
            element.setAccessibilityChildren(
                (!children.is_empty())
                    .then(|| array_of(made, &children))
                    .as_deref(),
            );
            if let Some(words) = &node.role_description {
                element.setAccessibilityRoleDescription(Some(&NSString::from_str(words)));
            }
            if let Some(help) = &node.help {
                element.setAccessibilityHelp(Some(&NSString::from_str(help)));
            }
            if let Some(index) = node.index {
                element.setAccessibilityIndex(index as NSInteger);
            }
            if let Some(level) = node.disclosure_level {
                element.setAccessibilityDisclosureLevel(level as NSInteger);
            }
            if let Some(by) = node.disclosed_by.and_then(|by| made.get(by)?.as_ref()) {
                element.setAccessibilityDisclosedByRow(Some(&*by.object()));
            }
            if matches!(item.role, AccessibleRole::TreeItem) {
                // Set to nothing where the row holds none now (it was closed).
                let rows =
                    (!node.disclosed_rows.is_empty()).then(|| array_of(made, &node.disclosed_rows));
                element.setAccessibilityDisclosedRows(
                    rows.as_deref().map(|rows| -> &AnyObject { rows }),
                );
            }
            if matches!(item.role, AccessibleRole::Table | AccessibleRole::Tree) {
                let rows = array_of(made, &node.rows);
                element.setAccessibilityRows(Some(&rows));
                element.setAccessibilityVisibleRows(Some(&rows));
                element.setAccessibilitySelectedRows(Some(&array_of(made, &node.selected_rows)));
                if let Some(count) = node.row_count {
                    element.setAccessibilityRowCount(count as NSInteger);
                }
                if let Some(count) = node.column_count {
                    element.setAccessibilityColumnCount(count as NSInteger);
                }
                element.setAccessibilityColumnHeaderUIElements(
                    (!node.column_headers.is_empty())
                        .then(|| array_of(made, &node.column_headers))
                        .as_deref(),
                );
                let head = node
                    .header
                    .and_then(|head| made.get(head)?.as_ref())
                    .map(Made::object);
                element.setAccessibilityHeader(head.as_deref());
                // The cells by their row and their column, counted as the table counts them, which is
                // how a cell's own row and column ranges say them and so how a screen reader asks for
                // the next: a row the page built past the top of a scrolled table is not the first.
                // Where the page gives no number, the place among the rows and the cells built.
                let mut cells = Vec::new();
                for (row_at, row) in node.rows.iter().enumerate() {
                    let Some(row_node) = nodes.get(*row) else {
                        continue;
                    };
                    let mut column = 0;
                    for child in &row_node.children {
                        if !matches!(
                            items[*child].role,
                            AccessibleRole::Cell | AccessibleRole::ColumnHeader
                        ) {
                            continue;
                        }
                        let Some(cell) = made.get(*child).and_then(Option::as_ref) else {
                            continue;
                        };
                        let place = nodes[*child]
                            .column_range
                            .map(|(first, _)| first)
                            .or(nodes[*child].index)
                            .unwrap_or(column);
                        let line = row_node.index.unwrap_or(row_at as i64);
                        cells.push((line, place, cell.object()));
                        column += 1;
                    }
                }
                element.ivars().cells.replace(cells);
            }
            element.setAccessibilitySelectedChildren(
                (!node.selected_children.is_empty())
                    .then(|| array_of(made, &node.selected_children))
                    .as_deref(),
            );
            if let Some((first, many)) = node.row_range {
                element.setAccessibilityRowIndexRange(NSRange::new(first as usize, many as usize));
            }
            if let Some((first, many)) = node.column_range {
                element
                    .setAccessibilityColumnIndexRange(NSRange::new(first as usize, many as usize));
            }
            if let Some(sort) = node.sort {
                element.setAccessibilitySortDirection(match sort {
                    Sort::Ascending => NSAccessibilitySortDirection::Ascending,
                    Sort::Descending => NSAccessibilitySortDirection::Descending,
                });
            }
        }
        if let (Some(target), false) = (item.id, node.actions.is_empty()) {
            let actions: Vec<Retained<NSAccessibilityCustomAction>> = node
                .actions
                .iter()
                .map(|(title, command)| custom_action(pane, title, *command, target))
                .collect();
            element.setAccessibilityCustomActions(Some(&NSArray::from_retained_slice(&actions)));
        } else {
            element.setAccessibilityCustomActions(None);
        }
    }
}

/// The command the page defined on `target`, as a screen reader's action of its own: its name is the
/// command's, and doing it tells the page so (`PageHandle`'s page, `command_on`).
fn custom_action(
    pane: &TerminalPane,
    title: &str,
    command: Id,
    target: Id,
) -> Retained<NSAccessibilityCustomAction> {
    let pane = WeakId::new(pane);
    let handler = RcBlock::new(move || -> Bool {
        Bool::new(
            pane.load()
                .is_some_and(|pane| pane.page_command_on(command, target)),
        )
    });
    let name = NSString::from_str(title);
    NSAccessibilityCustomAction::initWithName_handler(
        NSAccessibilityCustomAction::alloc(),
        &name,
        Some(&handler),
    )
}

/// The page says `words` out loud (an `alert`): the screen reader is told at once and above what it is
/// saying.
fn announce(pane: &TerminalPane, words: &str) {
    let message = NSString::from_str(words);
    let priority = NSNumber::new_isize(NSAccessibilityPriorityLevel::High.0);
    let objects: [&AnyObject; 2] = [message.as_ref(), priority.as_ref()];
    // SAFETY: AppKit's announcement and priority keys (extern statics) with the `NSString` and the
    // `NSNumber` they document.
    let info = unsafe {
        NSDictionary::<objc2_app_kit::NSAccessibilityNotificationUserInfoKey, AnyObject>::from_slices(
            &[NSAccessibilityAnnouncementKey, NSAccessibilityPriorityKey],
            &objects,
        )
    };
    // SAFETY: the pane is an accessibility element (a view) and the user info is the announcement the
    // notification documents.
    unsafe {
        NSAccessibilityPostNotificationWithUserInfo(
            pane,
            NSAccessibilityAnnouncementRequestedNotification,
            Some(&info),
        );
    }
}

/// Whether a screen reader presses an element of this role: what a click does to it.
fn pressed_by_role(role: AccessibleRole) -> bool {
    matches!(
        role,
        AccessibleRole::Button
            | AccessibleRole::Link
            | AccessibleRole::Checkbox
            | AccessibleRole::Switch
            | AccessibleRole::Radio
            | AccessibleRole::Tab
            | AccessibleRole::MenuItem
    )
}

/// A colour of the theme as AppKit's: the engine's tone in the theme, as sRGB.
fn ink(theme: &Theme, tone: Role) -> Retained<NSColor> {
    let [r, g, b, _] = tone_linear(theme, tone).to_srgb().map(f64::from);
    NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0)
}

/// A field of `kind` for `pane`: no border and no ground (the page draws the box), the
/// terminal's face, the theme's ink. One line scrolls as it is typed and cuts a hint that does
/// not fit with "…"; lines wrap and scroll; a secret shows no characters. The pane is its delegate
/// (`TerminalPane::page_field_changed`, `page_field_command`), or, for lines, which are a text view
/// and not a text field, hears of it from the view itself ([`PageLinesText`]).
fn text_field(
    pane: &TerminalPane,
    kind: FieldKind,
    theme: &Theme,
    family: Option<&str>,
    fields: &Rc<PageFields>,
) -> FieldView {
    let view = field_of(
        pane.mtm(),
        kind,
        Rc::downgrade(fields),
        family,
        f64::from(ZOOM_TEXT),
    );
    view.set_ink(&ink(theme, Role::Foreground));
    if let Some(field) = view.text_field() {
        // SAFETY: the pane implements the text field's delegate methods it is given (`impl
        // NSTextFieldDelegate for TerminalPane`) and outlives the field, which it removes with the
        // page.
        unsafe { field.setDelegate(Some(ProtocolObject::from_ref(pane))) };
    }
    view
}

/// How tall one line of a field is at `zoom` times the kit's text ([`ZOOM_TEXT`], the page's own
/// size), in the page's points: the line of the very cell a page's field is drawn with (a field made
/// as a page's, which nobody sees), at the size the field is set in, over the zoom. The line of a
/// face is not in proportion to its size (Monaco's 12.5 points has 16, its 25 has 31), so it is asked
/// of AppKit at each zoom, not scaled. A field of text and a field of lines each have theirs (the
/// cell's and the text view's, which agree in the faces tried: `field_problems`); the page is told
/// the taller, so that no box of rows is shorter than the lines it holds.
pub(crate) fn line_of(mtm: MainThreadMarker, family: Option<&str>, zoom: f32) -> f32 {
    let size = f64::from(ZOOM_TEXT) * f64::from(zoom);
    let line = [FieldKind::Line, FieldKind::Lines]
        .into_iter()
        .map(|kind| field_of(mtm, kind, Weak::new(), family, size).line_height())
        .fold(0.0, f64::max);
    (line / f64::from(zoom)) as f32
}

/// A text field of `kind` as a page's are made, set in the terminal's face at `size` points: no
/// border and no ground, the cell the way every field of a page has it. `fields` are told when the
/// person clicks into it.
fn field_of(
    mtm: MainThreadMarker,
    kind: FieldKind,
    fields: Weak<PageFields>,
    family: Option<&str>,
    size: f64,
) -> FieldView {
    let ivars = PageTextFieldIvars {
        id: Cell::new(None),
        selection: Cell::new(None),
        asked: Cell::new(None),
        given_by_press: Cell::new(None),
        fields: RefCell::new(fields),
        one_line: Cell::new(kind != FieldKind::Lines),
        room: Cell::new(NSRect::ZERO),
        hint: RefCell::new(None),
        storage: RefCell::new(None),
        undo: RefCell::new(None),
        line: Cell::new(0.0),
        baseline: Cell::new(0.0),
        pad: Cell::new([0.0; 4]),
    };
    if kind == FieldKind::Lines {
        return FieldView::Lines(lines_of(mtm, ivars, family, size));
    }
    // SAFETY: `initWithFrame:` is NSTextField's designated initializer (NSSecureTextField's too),
    // and the ivars are set.
    let view = if kind == FieldKind::Secret {
        // A secret's field draws dots for the characters, and its editor copies nothing.
        let this = PageSecretField::alloc(mtm).set_ivars(ivars);
        FieldView::Secret(unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] })
    } else {
        let this = PageTextField::alloc(mtm).set_ivars(ivars);
        FieldView::Text(unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] })
    };
    let field = view.text_field().expect("a field of text");
    field.setBordered(false);
    field.setBezeled(false);
    field.setDrawsBackground(false);
    field.setEditable(true);
    field.setSelectable(true);
    field.setFocusRingType(NSFocusRingType::None);
    field.setFont(Some(&face(family, size)));
    if let Some(cell) = field.cell() {
        // Not AppKit's single-line mode, for any kind: its cell draws the text at a baseline set
        // by the control's size whatever the face, and in most of the terminal's faces that stood
        // a point or more above the room's middle (four at 17 points), and moved down once the
        // field was typed into, where the editor draws it at the face's own baseline, the one
        // `text_top` centres. The one other thing that mode does, turning a pasted line break
        // into a space, the field does itself (`one_line`).
        cell.setUsesSingleLineMode(false);
        // One line: it scrolls as it is typed, and a hint that is too long for the room is cut with
        // "…" (the break mode below), not wrapped onto a line below it that the room does not show
        // (which left the first line high in the box).
        cell.setWraps(false);
        cell.setScrollable(true);
    }
    field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    // Pinned to the pane's top, where the page's rooms are measured from: the pane's own
    // coordinates grow upward, so a field with a fixed bottom margin would slide with the
    // pane's bottom edge while it is resized, until the next outline put it back.
    // The host is on the page's axis and covers the pane: a field keeps the frame its room gives
    // it, and AppKit moves nothing.
    field.setAutoresizingMask(NSAutoresizingMaskOptions::empty());
    view
}

/// A field of lines as a page's are made, set in the terminal's face at `size` points: a text view
/// in a scroll view, which scrolls the lines the box has no room for ("the field scrolls the lines it
/// has no room for", the kit's specification) where a wrapping `NSTextField` draws them past it.
/// It is what AppKit's field is to the person: no border and no ground, the terminal's face, a
/// caret that stays in view, a line of the very height of a text field's cell (no leading) and the
/// two points before the text that its cell has, the same substitutions its editor has and a hint
/// drawn in the same ink ([`PageLinesText`]). The scroll bar is the system's overlay one in every
/// setting: a bar that takes room would wrap the text at a width the page did not lay it out for.
fn lines_of(
    mtm: MainThreadMarker,
    ivars: PageTextFieldIvars,
    family: Option<&str>,
    size: f64,
) -> LinesView {
    let storage = NSTextStorage::new();
    let manager = NSLayoutManager::new();
    manager.setUsesFontLeading(false);
    let container =
        NSTextContainer::initWithSize(NSTextContainer::alloc(), NSSize::new(0.0, 1.0e7));
    container.setWidthTracksTextView(true);
    container.setLineFragmentPadding(SIDE_PADDING);
    storage.addLayoutManager(&manager);
    manager.addTextContainer(&container);
    let this = PageLinesText::alloc(mtm).set_ivars(ivars);
    // SAFETY: `initWithFrame:textContainer:` is NSTextView's designated initializer, and the ivars
    // are set.
    let text: Retained<PageLinesText> = unsafe {
        msg_send![super(this), initWithFrame: NSRect::ZERO, textContainer: Some(&*container)]
    };
    manager.setDelegate(Some(ProtocolObject::from_ref(&*text)));
    text.setFont(Some(&face(family, size)));
    text.setRichText(false);
    text.setImportsGraphics(false);
    text.setDrawsBackground(false);
    text.setAllowsUndo(true);
    text.setUsesFontPanel(false);
    text.setFocusRingType(NSFocusRingType::None);
    text.setTextContainerInset(NSSize::ZERO);
    // What a field's editor has of the substitutions: AppKit's own `NSTextField` editor, asked in
    // `field_problems`' day, turned off quotes, dashes, links and spelling and left text
    // replacements on.
    text.setAutomaticQuoteSubstitutionEnabled(false);
    text.setAutomaticDashSubstitutionEnabled(false);
    text.setAutomaticTextReplacementEnabled(true);
    text.setAutomaticLinkDetectionEnabled(false);
    text.setAutomaticDataDetectionEnabled(false);
    text.setContinuousSpellCheckingEnabled(false);
    // The text grows downwards as far as it needs and is the width of the box, and no shorter
    // than it, so that a press below its last line is still the text view's.
    text.setMinSize(NSSize::ZERO);
    text.setMaxSize(NSSize::new(1.0e7, 1.0e7));
    text.setVerticallyResizable(true);
    text.setHorizontallyResizable(false);
    text.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    // A drop on the page is the page's (the files of a drag are its to take, `page_files`), as it is
    // over a text field that is not being typed in; a text view would take the paths as text. While
    // the keyboard is in it, it takes what its editor does, as a text field's editor does
    // ([`PageLinesText`]'s `becomeFirstResponder` and `resignFirstResponder`).
    text.unregisterDraggedTypes();
    let scroll = {
        let this = PageLinesScroll::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:` is NSView's designated initializer, which NSScrollView keeps.
        let scroll: Retained<PageLinesScroll> =
            unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        scroll
    };
    scroll.setHasVerticalScroller(true);
    scroll.setHasHorizontalScroller(false);
    scroll.setAutohidesScrollers(true);
    scroll.setScrollerStyle(NSScrollerStyle::Overlay);
    scroll.setBorderType(NSBorderType::NoBorder);
    scroll.setDrawsBackground(false);
    scroll.setAutomaticallyAdjustsContentInsets(false);
    scroll.setFocusRingType(NSFocusRingType::None);
    scroll.setDocumentView(Some(&text));
    // The scroll view is no element for a screen reader: the text view is the text area it reads.
    scroll.setAccessibilityElement(false);
    // The host is on the page's axis and covers the pane: the field keeps the frame its box gives
    // it, and AppKit moves nothing.
    scroll.setAutoresizingMask(NSAutoresizingMaskOptions::empty());
    // The text is held by the text view that shows it, as long as the view lives: what a text view
    // made with its own stack is, and not one made with a stack of its own, which holds none of it.
    text.ivars().storage.replace(Some(storage));
    LinesView {
        scroll: Retained::into_super(scroll),
        text,
    }
}

/// How far a field's text stands from the left of its frame, points: AppKit's cell of a text field
/// has two (measured from what it draws), a text view five.
const SIDE_PADDING: f64 = 2.0;

/// The terminal's face (the system's monospaced one where it has none) at `size` points.
fn face(family: Option<&str>, size: f64) -> Retained<NSFont> {
    family
        .and_then(|name| NSFont::fontWithName_size(&NSString::from_str(name), size))
        .unwrap_or_else(|| NSFont::monospacedSystemFontOfSize_weight(size, 0.0))
}

/// Puts the field where the page's room for it is (the pane's points from its top), saying the
/// page's hint while it is empty, and what it is to a screen reader. Its text stands in the box the
/// page says (`Field::text`: the room less the element's padding and border), not in all of the
/// room, which is the pointer's ([`FieldsHost`]): lines take that box, from its top, in a scroll view
/// the size of the padding box (`Field::padding_box`: the room less the border), which cuts what
/// scrolls, the padding going with the text; a line is as
/// tall as its own line (its font's, which AppKit draws from the top of the frame), so it is that
/// line that is centred in the box, not a taller frame with the text at its top. The text is as
/// large as the page is drawn (`zoom`). The field stands in the host ([`FieldsHost`]), whose axis
/// is the page's: its frame is in the room, from the top.
fn place_field(field: &FieldView, place: &Field, zoom: f32, family: Option<&str>) {
    let room = place.rect;
    field.ivars().room.set(NSRect::new(
        NSPoint::new(f64::from(room.x), f64::from(room.y)),
        NSSize::new(f64::from(room.w), f64::from(room.h)),
    ));
    // SAFETY: reading the superview; we are on the main thread.
    if unsafe { field.superview() }.is_none() {
        return;
    }
    let size = f64::from(ZOOM_TEXT) * f64::from(zoom);
    let hint = place.hint.as_deref().map(NSString::from_str);
    let said = place
        .label
        .as_deref()
        .or(place.hint.as_deref())
        .unwrap_or_default();
    let text = place.text;
    let frame = |top: f64, height: f64| {
        // The host's y is the page's, from the top.
        NSRect::new(
            NSPoint::new(f64::from(text.x), top),
            NSSize::new(f64::from(text.w), height),
        )
    };
    let lines = match field {
        FieldView::Lines(lines) => lines,
        _ => {
            let Some(field) = field.text_field() else {
                return;
            };
            if field
                .font()
                .is_none_or(|font| (font.pointSize() - size).abs() > 0.01)
            {
                field.setFont(Some(&face(family, size)));
            }
            field.setPlaceholderString(hint.as_deref());
            // On the cell, which is what a screen reader reads of the field ([`read_of`]).
            if let Some(cell) = field.cell() {
                cell.setAccessibilityLabel(Some(&NSString::from_str(said)));
            }
            // A box shorter than a line (the padding is more than the room has over a line) still
            // holds the line, as the room does: the frame goes past the box, not the text out of
            // the frame.
            let line = line_height(field).min(f64::from(room.h));
            let middle = f64::from(text.y + text.h / 2.0);
            let frame = frame(text_top(field, middle, line), line);
            if field.frame() != frame {
                field.setFrame(frame);
            }
            // AppKit draws the field above the page: one the page's popover or dialog lies over is
            // not shown, or its text would stand over what covers it.
            field.setHidden(place.covered);
            return;
        }
    };
    // Lines: the box, from its top, and no shorter than a line (as above). Its text view draws the
    // hint, is the text area a screen reader reads, and keeps the caret's line in view whenever its
    // box or its face changes.
    let (scroll, view) = (&lines.scroll, &lines.text);
    let mut changed = false;
    // The system's preference for scroll bars (Always show) moves a scroll view to its own style, and
    // the text would be narrower than the box by the bar: the page's rooms are not AppKit's to take.
    if scroll.scrollerStyle() != NSScrollerStyle::Overlay {
        scroll.setScrollerStyle(NSScrollerStyle::Overlay);
        changed = true;
    }
    let wanted = face(family, size);
    // The face or its size changed, or the line was never measured: the line is the face's.
    if view.ivars().line.get() <= 0.0 || view.font().is_none_or(|font| font != wanted) {
        view.setFont(Some(&wanted));
        if let Some((_, hint)) = view.ivars().hint.borrow().as_ref() {
            hint.setFont(view.font().as_deref());
        }
        // The line of the page's and the cell's baseline in it, in this face at this size: what every
        // line of the text is laid out in from now on.
        let (cell, baseline) = cell_line(view.mtm(), family, size);
        view.ivars().line.set(cell.max(lines_natural_line(view)));
        view.ivars().baseline.set(baseline);
        // SAFETY: the text view was made with its layout manager, and the range is the text's.
        if let Some(manager) = unsafe { view.layoutManager() } {
            let all = NSRange::new(0, view.string().length());
            // SAFETY: a null actual range is allowed.
            unsafe {
                manager.invalidateLayoutForCharacterRange_actualCharacterRange(
                    all,
                    std::ptr::null_mut(),
                );
            }
        }
        changed = true;
    }
    if view.ivars().hint.borrow().as_ref().map(|(words, _)| words) != hint.as_ref() {
        if let Some((_, old)) = view.ivars().hint.take() {
            old.removeFromSuperview();
        }
        let made = hint.as_ref().map(|words| {
            let this = PageHintField::alloc(view.mtm()).set_ivars(());
            // SAFETY: `initWithFrame:` is NSTextField's designated initializer.
            let field: Retained<PageHintField> =
                unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
            field.setBordered(false);
            field.setBezeled(false);
            field.setDrawsBackground(false);
            field.setEditable(true);
            field.setSelectable(false);
            field.setFocusRingType(NSFocusRingType::None);
            field.setFont(view.font().as_deref());
            field.setPlaceholderString(Some(words));
            field.setAccessibilityElement(false);
            if let Some(cell) = field.cell() {
                // As the field of lines was a text field's before it was a text view's: its cell
                // wraps at the box's width, from its top.
                cell.setUsesSingleLineMode(false);
                cell.setWraps(true);
                cell.setScrollable(false);
                // The cell is what a screen reader reads of a text field ([`read_of`]), and this
                // one is not a field: the text view is, and says the hint.
                cell.setAccessibilityElement(false);
            }
            field.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
            field.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            view.addSubview(&field);
            (words.clone(), field)
        });
        view.ivars().hint.replace(made);
        view.show_hint();
        view.setAccessibilityPlaceholderValue(hint.as_deref());
    }
    view.setAccessibilityLabel(Some(&NSString::from_str(said)));
    // The scroll view is the padding box (what scrolls is cut there, and the padding goes with the
    // text); the text stands in it at the padding of each side, which are not all the same.
    let shown = place.padding_box;
    let pads = [
        text.x - shown.x,
        text.y - shown.y,
        shown.x + shown.w - (text.x + text.w),
        shown.y + shown.h - (text.y + text.h),
    ]
    .map(|pad| f64::from(pad.max(0.0)));
    if view.ivars().pad.get() != pads {
        view.ivars().pad.set(pads);
        view.invalidateTextContainerOrigin();
        changed = true;
    }
    // The inset is the same on both sides (the text view's frame is the text and its two paddings, and
    // its container the width the text box has); where the text starts is the left and top padding
    // alone ([`PageLinesText`]'s `textContainerOrigin`).
    let inset = NSSize::new((pads[0] + pads[2]) / 2.0, (pads[1] + pads[3]) / 2.0);
    if view.textContainerInset() != inset {
        view.setTextContainerInset(inset);
        changed = true;
    }
    // A box shorter than its first line (the padding is more than the room has over a line) still
    // holds the line, as the room does.
    let line = lines_line(view);
    let height = f64::from(shown.h).max((pads[1] + line).min(f64::from(room.y + room.h - shown.y)));
    let frame = NSRect::new(
        NSPoint::new(f64::from(shown.x), f64::from(shown.y)),
        NSSize::new(f64::from(shown.w), height),
    );
    if scroll.frame() != frame {
        scroll.setFrame(frame);
        changed = true;
    }
    let shown = NSSize::new(0.0, scroll.contentSize().height);
    if view.minSize() != shown {
        view.setMinSize(shown);
        changed = true;
    }
    if changed {
        view.sizeToFit();
        view.scroll_to_caret();
    }
    // The hint stands over the text box, from the padding of the box's top and left, as wide as the
    // text box is and as tall as the text view is below the padding there.
    if let Some((_, hint)) = view.ivars().hint.borrow().as_ref() {
        let bounds = view.bounds();
        let stands = NSRect::new(
            NSPoint::new(pads[0], pads[1]),
            NSSize::new(
                f64::from(text.w),
                (bounds.size.height - pads[1] - pads[3]).max(0.0),
            ),
        );
        if hint.frame() != stands {
            hint.setFrame(stands);
        }
    }
    scroll.setHidden(place.covered);
}

/// Where the frame of a one-line `field` stands so that its capitals are centred on the text box's
/// middle, as the page's own words are on theirs: AppKit's baseline is asked of the field
/// (`firstBaselineOffsetFromTop`, not the font's ascent, which AppKit rounds up), and the frame goes
/// where that baseline is half a capital below the middle. Centring the frame instead put the text a
/// point and a half below the words beside the field in a face whose line AppKit draws lower.
/// Rounded to the screen's pixels. That offset is where the field draws its text, hint or value,
/// typed into or not, because its cell is not in single-line mode ([`text_field`]).
fn text_top(field: &NSTextField, middle: f64, height: f64) -> f64 {
    let Some(font) = field.font() else {
        return (middle - height / 2.0).round();
    };
    let baseline = middle + font.capHeight() / 2.0;
    // SAFETY: `firstBaselineOffsetFromTop` is NSView's, takes nothing and returns a CGFloat; on the
    // main thread, as every call here.
    let offset: f64 = unsafe { msg_send![field, firstBaselineOffsetFromTop] };
    let scale = field
        .window()
        .map_or(2.0, |window| window.backingScaleFactor());
    ((baseline - offset) * scale).round() / scale
}

/// The height of one line of `field`, as its cell draws it: its font's line and the cell's
/// margins. Asked of the cell, not of Auto Layout (`fittingSize`), which answers for a field out
/// of its window with a height of nothing: a pane moved into a split (⌘D) is out of its window for
/// a moment, and a field placed then stood half a line low until the page's outline changed.
fn line_height(field: &NSTextField) -> f64 {
    let font_line = || {
        field.font().map_or(0.0, |font| {
            (font.ascender() - font.descender() + font.leading()).ceil()
        })
    };
    field
        .cell()
        .map(|cell| cell.cellSize().height)
        .filter(|height| height.is_finite() && *height > 0.0)
        .map_or_else(font_line, f64::ceil)
}

/// The tracking area that tells `view` where the pointer is over its whole visible rectangle,
/// whoever has the keyboard (a field with it takes the window's own pointer motion away).
pub(crate) fn pointer_area(view: &NSView) -> Retained<NSTrackingArea> {
    let options = NSTrackingAreaOptions::MouseMoved
        | NSTrackingAreaOptions::MouseEnteredAndExited
        | NSTrackingAreaOptions::ActiveInKeyWindow
        | NSTrackingAreaOptions::InVisibleRect;
    // SAFETY: `owner` is the view, which outlives the area — the pane removes the area from the
    // view before it lets go of it; no user info.
    let area = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            view.bounds(),
            options,
            Some(view),
            None,
        )
    };
    view.addTrackingArea(&area);
    area
}

/// The key the characters of a key event make, for the page: the arrows and the keys beside
/// them are private-use characters in AppKit's text (`NSUpArrowFunctionKey` and the rest).
pub(crate) fn key_of(characters: &str) -> Option<Key> {
    let mut chars = characters.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(match c {
        '\u{f700}' => Key::Up,
        '\u{f701}' => Key::Down,
        '\u{f702}' => Key::Left,
        '\u{f703}' => Key::Right,
        '\u{f729}' => Key::Home,
        '\u{f72b}' => Key::End,
        '\u{f72c}' => Key::PageUp,
        '\u{f72d}' => Key::PageDown,
        // The numpad's enter makes U+0003 (and ⌃C does too: the caller has taken that one).
        '\r' | '\n' | '\u{3}' => Key::Enter,
        '\u{1b}' => Key::Escape,
        '\t' => Key::Tab,
        // ⌫ makes DEL in AppKit's text; ⌦ (fn-⌫) is `NSDeleteFunctionKey`.
        '\u{7f}' | '\u{8}' => Key::Backspace,
        '\u{f728}' => Key::Delete,
        // `NSF1FunctionKey` to `NSF20FunctionKey`, one after the other.
        #[allow(clippy::cast_possible_truncation)]
        f @ '\u{f704}'..='\u{f717}' => Key::Function((u32::from(f) - 0xf704 + 1) as u8),
        other if other.is_control() || ('\u{f700}'..='\u{f8ff}').contains(&other) => return None,
        other => Key::Character(other),
    })
}

/// The key a command of a page's field stands for, to be asked of the kit's rule of the keys a field
/// keeps ([`FieldState::takes`], the kit's specification, §2.1 and §4): from the selector AppKit's
/// text names for the command, and the key of the event in hand (`event`, with the modifiers held)
/// for the commands that name no key by their selector (`moveLeft:` is ← and also ⌃B, `deleteBackward:`
/// is ⌫ and also ⌃H).
///
/// The selector decides for Esc, Tab (and ⇧Tab), ⏎, and for the keys that go up and down, and
/// page up and down: they are the keys they are however they are written (`moveToBeginningOfDocument:`
/// is ↑ in a line). The rest are the event's key. `None` where the field keeps the command whatever
/// the rule says: a chord of ⌃ and a character that AppKit's text binds to an editing command (⌃A,
/// ⌃E, ⌃K, ⌃D, ⌃F, ⌃B, ⌃H, ⌃T, ⌃Y: a Mac's own, which the rule allows a host to keep as well),
/// and a command made by no key (`event` is `None`: a dead key's first stroke).
pub(crate) fn field_command_key(command: &str, event: Option<(Key, Mods)>) -> Option<(Key, Mods)> {
    let mods = event.map(|(_, mods)| mods).unwrap_or_default();
    let named = match command {
        "cancelOperation:" => Some(Key::Escape),
        "insertTab:" | "insertBacktab:" => Some(Key::Tab),
        "insertNewline:" | "insertNewlineIgnoringFieldEditor:" | "insertLineBreak:" => {
            Some(Key::Enter)
        }
        "moveUp:"
        | "moveUpAndModifySelection:"
        | "moveToBeginningOfDocument:"
        | "moveToBeginningOfDocumentAndModifySelection:"
        | "moveParagraphBackwardAndModifySelection:" => Some(Key::Up),
        "moveDown:"
        | "moveDownAndModifySelection:"
        | "moveToEndOfDocument:"
        | "moveToEndOfDocumentAndModifySelection:"
        | "moveParagraphForwardAndModifySelection:" => Some(Key::Down),
        "scrollPageUp:" | "pageUp:" | "pageUpAndModifySelection:" => Some(Key::PageUp),
        "scrollPageDown:" | "pageDown:" | "pageDownAndModifySelection:" => Some(Key::PageDown),
        _ => None,
    };
    if let Some(key) = named {
        let mods = Mods {
            shift: mods.shift || command == "insertBacktab:",
            ..mods
        };
        return Some((key, mods));
    }
    let (key, mods) = event?;
    if mods.control && !mods.command && matches!(key, Key::Character(_)) && command != "noop:" {
        return None;
    }
    Some((key, mods))
}

/// The keys of a menu's entry as AppKit's menu items have them: the characters of the key
/// equivalent (a letter in lower case, with Shift in the mask; the arrows and the keys beside them
/// are private-use characters, the reverse of [`key_of`]) and the modifier flags, so that the menu
/// shows them in its own glyphs.
pub(crate) fn key_equivalent(shortcut: &Shortcut) -> (String, NSEventModifierFlags) {
    let key = match shortcut.key {
        Key::Character(c) => c.to_lowercase().collect(),
        Key::Up => "\u{f700}".to_owned(),
        Key::Down => "\u{f701}".to_owned(),
        Key::Left => "\u{f702}".to_owned(),
        Key::Right => "\u{f703}".to_owned(),
        Key::Home => "\u{f729}".to_owned(),
        Key::End => "\u{f72b}".to_owned(),
        Key::PageUp => "\u{f72c}".to_owned(),
        Key::PageDown => "\u{f72d}".to_owned(),
        Key::Enter => "\r".to_owned(),
        Key::Escape => "\u{1b}".to_owned(),
        Key::Tab => "\t".to_owned(),
        Key::Backspace => "\u{8}".to_owned(),
        Key::Delete => "\u{f728}".to_owned(),
        // `NSF1FunctionKey` and the rest, one after the other.
        Key::Function(n) => char::from_u32(0xf704 + u32::from(n.saturating_sub(1)))
            .map(String::from)
            .unwrap_or_default(),
    };
    let mut mask = NSEventModifierFlags::empty();
    for (held, flag) in [
        (shortcut.mods.shift, NSEventModifierFlags::Shift),
        (shortcut.mods.control, NSEventModifierFlags::Control),
        (shortcut.mods.alt, NSEventModifierFlags::Option),
        (shortcut.mods.command, NSEventModifierFlags::Command),
    ] {
        if held {
            mask |= flag;
        }
    }
    (key, mask)
}

/// The modifiers held, as the page reads them.
pub(crate) fn mods_of(flags: NSEventModifierFlags) -> Mods {
    Mods {
        shift: flags.contains(NSEventModifierFlags::Shift),
        control: flags.contains(NSEventModifierFlags::Control),
        alt: flags.contains(NSEventModifierFlags::Option),
        command: flags.contains(NSEventModifierFlags::Command),
    }
}

/// The modifiers the system holds now, whichever window has the keyboard: what the page is caught
/// up with before the pointer is told anything ([`PageHandle::input`]), since `flagsChanged` is the
/// key window's alone and a ⌘ pressed elsewhere is not heard.
pub(crate) fn held_mods() -> Mods {
    mods_of(NSEvent::modifierFlags_class())
}

/// The key `event` is for a page, or for an editor of it, and the modifiers held ([`event_key`]).
/// `None` for a key that makes none (the first stroke of a dead key has no characters): in an
/// editor it is the system's handling of text's.
pub(crate) fn event_key_of(event: &NSEvent) -> Option<(Key, Mods)> {
    let mods = mods_of(event.modifierFlags());
    let characters = event.characters().map(|text| text.to_string());
    let ignoring = event
        .charactersIgnoringModifiers()
        .map(|text| text.to_string());
    let key = event_key(characters.as_deref(), ignoring.as_deref(), mods)?;
    Some((key, mods))
}

/// The key `characters` make for a page or an editor of it ([`key_of`]), with `mods` held: ⇧⇥
/// makes the backtab character, which is ⌃Y's too, and is the tab with ⇧; where ⌃ makes a control
/// character (⌃W, ⌃K), the key is the one without the modifiers (`ignoring`), so that a page's
/// command on `ctrl+w` is done wherever the keyboard is on the page.
fn event_key(characters: Option<&str>, ignoring: Option<&str>, mods: Mods) -> Option<Key> {
    let key = match characters {
        Some("\u{19}") if mods.shift && !mods.control => Some(Key::Tab),
        other => other.and_then(key_of),
    };
    key.or_else(|| mods.control.then(|| ignoring.and_then(key_of)).flatten())
}

/// What is wrong with how a page's fields stand in their rooms, as AppKit lays them out (the page
/// itself is not here: the rooms are what the kit says of a one-line field, a box of lines and the
/// padding of each): the line of the face the page's fields are set in, in the page's points at a
/// zoom; the lines a box of rows holds without cutting the last; a one-line field's capitals in the
/// middle of its box; and a press in the room, outside the field's frame, which is the field's.
/// Only for `tests/page_field.rs`: a field is AppKit's and wants the main thread.
pub(crate) fn field_problems(mtm: MainThreadMarker) -> Vec<String> {
    let mut problems = Vec::new();
    let mut said = |words: String| problems.push(words);
    let room_of_text = |text: &Rect, above: f32, below: f32| {
        Rect::new(
            text.x - 8.0,
            text.y - above,
            text.w + 16.0,
            text.h + above + below,
        )
    };
    // The line of a face is asked of AppKit, not scaled: Monaco has 16 points to a line at the kit's
    // 12.5 and 31 at twice that (15.5 of the page's at zoom 2); and a field of any kind has the same.
    for (zoom, line) in [(1.0_f32, 16.0_f32), (2.0, 15.5)] {
        let got = line_of(mtm, Some("Monaco"), zoom);
        if (got - line).abs() > 0.01 {
            said(format!(
                "Monaco at zoom {zoom}: a line is {got} points, not {line}"
            ));
        }
        let size = f64::from(ZOOM_TEXT) * f64::from(zoom);
        for kind in [FieldKind::Line, FieldKind::Lines, FieldKind::Secret] {
            let field = field_of(mtm, kind, Weak::new(), Some("Monaco"), size);
            let own = (field.line_height() / f64::from(zoom)) as f32;
            if (own - got).abs() > 0.01 {
                said(format!(
                    "Monaco at zoom {zoom}: a {kind:?} field has a line of {own}, the page is told {got}"
                ));
            }
        }
    }
    let host_frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 300.0));
    let id = Id::named("a field");
    let field = |kind: FieldKind, room: Rect, text: Rect, covered: bool| Field {
        id,
        kind,
        rect: room,
        padding_box: room,
        text,
        hint: None,
        label: None,
        covered,
    };
    // Three rows at padding 6, as the kit lays them out from the line the page was told: the box
    // is three lines and the room is that and the padding. Each text of up to three rows is held
    // whole in it (the cell's own measure of the text at the box's width), and one of four is not,
    // which is what shows the measure sees a cut.
    for (zoom, line) in [(1.0_f32, 16.0_f32), (2.0, 31.0)] {
        let host = FieldsHost::with_frame(mtm, host_frame);
        let view = field_of(mtm, FieldKind::Lines, Weak::new(), Some("Monaco"), 12.5);
        host.addSubview(&view);
        let pad = 6.0 * zoom;
        let text = Rect::new(20.0, 30.0 + pad, 200.0, 3.0 * line);
        let room = room_of_text(&text, pad, pad);
        place_field(
            &view,
            &field(FieldKind::Lines, room, text, false),
            zoom,
            Some("Monaco"),
        );
        // The scroll view stands in the padding box (the room, here: no border), the text in its
        // text box.
        let frame = view.frame();
        if frame.origin.x != f64::from(room.x)
            || frame.origin.y != f64::from(room.y)
            || frame.size.width != f64::from(room.w)
            || frame.size.height != f64::from(room.h)
        {
            said(format!(
                "rows = 3 at zoom {zoom}: the field stands at {frame:?}, not in its padding box {room:?}"
            ));
        }
        // What the text needs of a box of this width, as the text view lays it out.
        let FieldView::Lines(lines) = &view else {
            unreachable!("a field of lines");
        };
        let held = |words: &str, across: f64| {
            let text = Rect::new(20.0, 30.0 + pad, across as f32, 3.0 * line);
            place_field(
                &view,
                &field(FieldKind::Lines, room_of_text(&text, pad, pad), text, false),
                zoom,
                Some("Monaco"),
            );
            lines.text.setString(&NSString::from_str(words));
            // SAFETY: the text view was made with its layout manager and container.
            let (manager, container) =
                unsafe { (lines.text.layoutManager(), lines.text.textContainer()) };
            let (manager, container) = (
                manager.expect("a layout manager"),
                container.expect("a container"),
            );
            manager.ensureLayoutForTextContainer(&container);
            manager.usedRectForTextContainer(&container).size.height
        };
        for words in [
            "one\ntwo\nthree",
            "one\ntwo",
            "a long line that breaks at words, and so is three",
        ] {
            let across = if words.starts_with("a long") {
                160.0 * f64::from(zoom)
            } else {
                200.0
            };
            let needs = held(words, across);
            if needs > f64::from(3.0 * line) + 0.01 {
                said(format!(
                    "rows = 3 at zoom {zoom}: {words:?} needs {needs} points of the box's {}",
                    3.0 * line
                ));
            }
            // The long one breaks at words into three rows, not two: it fills the box.
            if words.starts_with("a long") && needs <= f64::from(2.0 * line) + 0.01 {
                said(format!(
                    "rows = 3 at zoom {zoom}: the long line breaks into {needs} points, not three rows"
                ));
            }
        }
        let four = held("one\ntwo\nthree\nfour", 200.0);
        if four <= f64::from(3.0 * line) {
            said(format!(
                "rows = 3 at zoom {zoom}: four lines need {four} points, the measure does not see a cut"
            ));
        }
    }
    // One row with 4 above and 10 below (the room's middle is not the box's), and a field of one
    // line in the same box: the capitals' middle is the box's middle, ±0.5 point, across the box's
    // width and not the room's.
    for kind in [FieldKind::Line, FieldKind::Lines, FieldKind::Secret] {
        let host = FieldsHost::with_frame(mtm, host_frame);
        let view = field_of(mtm, kind, Weak::new(), Some("Monaco"), 12.5);
        host.addSubview(&view);
        let text = Rect::new(20.0, 36.0, 200.0, 16.0);
        place_field(
            &view,
            &field(kind, room_of_text(&text, 4.0, 10.0), text, false),
            1.0,
            Some("Monaco"),
        );
        let frame = view.frame();
        let middle = if matches!(view, FieldView::Lines(_)) {
            // A text view has no one baseline to ask of it as a field has: what it draws is measured
            // (the capitals' ink, from the top of the one to the bottom).
            view.set_text("XXXX");
            let window = window_over(mtm, &host);
            let ink = ink_in(&host);
            window.setContentView(None);
            ink.map_or(f64::NAN, |(_, top, _, bottom)| (top + bottom) / 2.0)
        } else {
            let font = view
                .text_field()
                .and_then(|field| field.font())
                .expect("a font");
            // SAFETY: NSView's `firstBaselineOffsetFromTop`, which takes nothing and gives a CGFloat.
            let baseline: f64 = unsafe { msg_send![&*view, firstBaselineOffsetFromTop] };
            frame.origin.y + baseline - font.capHeight() / 2.0
        };
        let aimed = f64::from(text.y + text.h / 2.0);
        if middle.is_nan() || (middle - aimed).abs() > 0.5 {
            said(format!(
                "rows = 1 at padding 4 and 10, a {kind:?}: the capitals' middle is {middle}, the box's {aimed}"
            ));
        }
        // A field of text stands in its text box; the scroll view of a field of lines is its
        // padding box (here the room: the box's 8 on each side), with the text in the box.
        let (across, wide) = if matches!(view, FieldView::Lines(_)) {
            (text.x - 8.0, text.w + 16.0)
        } else {
            (text.x, text.w)
        };
        if frame.origin.x != f64::from(across) || frame.size.width != f64::from(wide) {
            said(format!(
                "a {kind:?}: the field stands across {frame:?}, not at {across} and {wide} wide"
            ));
        }
    }
    // A box shorter than a line (padding 6 in a room of 24 leaves 12) still holds the line: a field
    // of one line's frame is a line tall, in the room; the scroll view of lines is the padding box
    // (the room), which holds the line below the padding over it; capitals in the box's middle.
    for kind in [FieldKind::Line, FieldKind::Lines] {
        let host = FieldsHost::with_frame(mtm, host_frame);
        let view = field_of(mtm, kind, Weak::new(), Some("Monaco"), 12.5);
        host.addSubview(&view);
        let text = Rect::new(20.0, 36.0, 200.0, 12.0);
        let room = Rect::new(12.0, 30.0, 216.0, 24.0);
        place_field(&view, &field(kind, room, text, false), 1.0, Some("Monaco"));
        let frame = view.frame();
        let wanted = if kind == FieldKind::Lines { 24.0 } else { 16.0 };
        if frame.size.height != wanted {
            said(format!(
                "a {kind:?} in a box of 12 points is {} points tall, not {wanted}",
                frame.size.height
            ));
        }
        if let FieldView::Lines(lines) = &view {
            // The first line stands whole in the frame: the padding over it and the line.
            if lines.text.ivars().pad.get()[1] + 16.0 > frame.size.height {
                said("a Lines in a box of 12 points holds less than its first line".to_owned());
            }
        }
    }
    // A press in the room is the field's wherever the text is: in the padding above and below, and
    // beside it; a field the page covers takes none, and the pane's is where no room is.
    for (kind, text) in [
        (FieldKind::Lines, Rect::new(20.0, 36.0, 200.0, 48.0)),
        (FieldKind::Line, Rect::new(20.0, 36.0, 200.0, 16.0)),
        (FieldKind::Secret, Rect::new(20.0, 36.0, 200.0, 16.0)),
    ] {
        let host = FieldsHost::with_frame(mtm, host_frame);
        let view = field_of(mtm, kind, Weak::new(), Some("Monaco"), 12.5);
        host.addSubview(&view);
        let room = room_of_text(&text, 6.0, 6.0);
        place_field(&view, &field(kind, room, text, false), 1.0, Some("Monaco"));
        // `isDescendantOf` is true of the view itself.
        let mine = |at: NSPoint| {
            host.hitTest(at)
                .is_some_and(|hit| hit.isDescendantOf(&view))
        };
        let (left, top, right, bottom) = (
            f64::from(room.x),
            f64::from(room.y),
            f64::from(room.x + room.w),
            f64::from(room.y + room.h),
        );
        let across = (left + right) / 2.0;
        for (what, at) in [
            ("the padding above", NSPoint::new(across, top + 2.0)),
            ("the padding below", NSPoint::new(across, bottom - 2.0)),
            (
                "the padding beside",
                NSPoint::new(left + 2.0, (top + bottom) / 2.0),
            ),
            (
                "the text",
                NSPoint::new(across, f64::from(text.y + text.h / 2.0)),
            ),
        ] {
            if !mine(at) {
                said(format!(
                    "a {kind:?}: a press in {what} ({at:?}) is not the field's"
                ));
            }
        }
        for (what, at) in [
            ("above the room", NSPoint::new(across, top - 2.0)),
            ("below the room", NSPoint::new(across, bottom + 2.0)),
            (
                "left of the room",
                NSPoint::new(left - 2.0, (top + bottom) / 2.0),
            ),
        ] {
            if host.hitTest(at).is_some() {
                said(format!(
                    "a {kind:?}: a press {what} ({at:?}) is taken by the field"
                ));
            }
        }
        // A field being typed in answers a press in its padding with its editor, as it does for
        // one in its text: the field itself would end the editing and begin it again, with the
        // whole text selected. It has an editor only in a window, where it can be the keyboard's.
        // SAFETY: with `defer` false the window exists at once; the `Retained` is its owner.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                host_frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: only changes who owns the window; the `Retained` above does.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setContentView(Some(&host));
        if window.makeFirstResponder(Some(view.inner())) && view.editing() {
            let field_itself: &NSView = &view;
            for at in [
                NSPoint::new(across, top + 2.0),
                NSPoint::new(across, bottom - 2.0),
                NSPoint::new(left + 2.0, (top + bottom) / 2.0),
            ] {
                // `hitTest:` is asked in the superview's points, which the window's has turned.
                // SAFETY: reading the superview, on the main thread.
                let at = match unsafe { host.superview() } {
                    Some(parent) => parent.convertPoint_fromView(at, Some(&host)),
                    None => at,
                };
                match host.hitTest(at) {
                    Some(hit) if std::ptr::eq(&*hit, field_itself) => said(format!(
                        "a {kind:?} being typed in: a press in its padding ({at:?}) is the field, which ends the typing, not its editor"
                    )),
                    // A field of lines is its own editor: its text view takes the press itself,
                    // not whatever of the scroll view is nearest (a scroller).
                    Some(hit)
                        if matches!(view, FieldView::Lines(_))
                            && !std::ptr::eq(&*hit, view.inner()) =>
                    {
                        said(format!(
                            "a {kind:?}: a press in its padding ({at:?}) is {hit:?}, not its text view"
                        ));
                    }
                    Some(_) => {}
                    None => said(format!(
                        "a {kind:?} being typed in: a press in its padding ({at:?}) is not the field's"
                    )),
                }
            }
            window.makeFirstResponder(None);
        } else {
            said(format!(
                "a {kind:?} took no keyboard in a window of its own: its editor is not tried"
            ));
        }
        window.setContentView(None);
        place_field(&view, &field(kind, room, text, true), 1.0, Some("Monaco"));
        if host.hitTest(NSPoint::new(across, top + 2.0)).is_some() {
            said(format!(
                "a {kind:?}: a press in the room of a field the page covers is taken by it"
            ));
        }
    }
    drop(said);
    lines_overflow_problems(mtm, &mut problems);
    lines_padding_problems(mtm, &mut problems);
    lines_changing_problems(mtm, &mut problems);
    lines_drop_problems(mtm, &mut problems);
    lines_bar_problems(mtm, &mut problems);
    lines_problems(mtm, &mut problems);
    problems
}

/// What is asked of a text view in `lines_problems`: one of its settings.
type Ask = dyn Fn(&NSTextView) -> bool;

/// A bare window of `frame`'s size over `host`: a field has a keyboard only in one.
fn window_over(mtm: MainThreadMarker, host: &NSView) -> Retained<NSWindow> {
    // SAFETY: with `defer` false the window exists at once; the `Retained` is its owner.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            host.frame(),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: only changes who owns the window; the `Retained` above does.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setContentView(Some(host));
    window
}

/// Where `view` draws, in its own points: the box of every pixel that is not clear, as AppKit
/// renders the view and what it holds (a field's editor too) without a screen to show it on.
/// `None` where nothing is drawn.
fn ink_in(view: &NSView) -> Option<(f64, f64, f64, f64)> {
    ink_of(view).map(|(x0, y0, x1, y1, _)| (x0, y0, x1, y1))
}

/// [`ink_in`], and the strongest pixel of it (the first of the most opaque), as the bytes the
/// rendering holds: the ink's own colour where a letter's stroke covers a whole pixel.
fn ink_of(view: &NSView) -> Option<(f64, f64, f64, f64, [u8; 4])> {
    let bounds = view.bounds();
    let rep = view.bitmapImageRepForCachingDisplayInRect(bounds)?;
    view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
    let (wide, high) = (rep.pixelsWide() as usize, rep.pixelsHigh() as usize);
    let (per, row) = (rep.samplesPerPixel() as usize, rep.bytesPerRow() as usize);
    let data = rep.bitmapData();
    if data.is_null() || per < 4 {
        return None;
    }
    let scale = wide as f64 / bounds.size.width;
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    let mut any = false;
    let mut strongest = [0u8; 4];
    for y in 0..high {
        for x in 0..wide {
            // SAFETY: inside the representation's own buffer: `row` bytes to a row, `per` to a pixel.
            let pixel = unsafe { std::slice::from_raw_parts(data.add(y * row + x * per), per) };
            let alpha = pixel[per - 1];
            if alpha > 8 {
                any = true;
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                if alpha > strongest[3] {
                    strongest = [pixel[0], pixel[1], pixel[2], alpha];
                }
            }
        }
    }
    any.then(|| {
        (
            x0 as f64 / scale,
            y0 as f64 / scale,
            (x1 + 1) as f64 / scale,
            (y1 + 1) as f64 / scale,
            strongest,
        )
    })
}

/// A box of rows draws no ink outside its text box, and the line the caret is in is in it: four lines
/// in a box of three, the fourth is the caret's, so the box shows it (the page's `lines` "scrolls
/// the lines it has no room for"). With the keyboard in the field and without it. The text is made
/// of three empty lines and `XXXX` so that all the ink there is, is that line's; the same text in a
/// box of four rows, and one line in a box of one, are the controls: they have ink where the
/// measure reads it, which a measure that sees nothing would not.
fn lines_overflow_problems(mtm: MainThreadMarker, problems: &mut Vec<String>) {
    let host_frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 300.0));
    let id = Id::named("a field");
    for (zoom, line) in [(1.0_f32, 16.0_f32), (2.0, 31.0)] {
        // (rows of the box, the text, whether the ink must be there at all)
        for (rows, words, what) in [
            (3.0_f32, "\n\n\nXXXX", "four lines in three rows"),
            (4.0, "\n\n\nXXXX", "four lines in four rows"),
            (1.0, "XXXX", "one line in one row"),
        ] {
            for keyboard in [false, true] {
                let host = FieldsHost::with_frame(mtm, host_frame);
                let view = field_of(mtm, FieldKind::Lines, Weak::new(), Some("Monaco"), 12.5);
                host.addSubview(&view);
                let pad = 6.0 * zoom;
                let text = Rect::new(20.0, 30.0 + pad, 200.0, rows * line);
                let room = Rect::new(
                    text.x - 8.0,
                    text.y - pad,
                    text.w + 16.0,
                    text.h + 2.0 * pad,
                );
                place_field(
                    &view,
                    &Field {
                        id,
                        kind: FieldKind::Lines,
                        rect: room,
                        padding_box: room,
                        text,
                        hint: None,
                        label: None,
                        covered: false,
                    },
                    zoom,
                    Some("Monaco"),
                );
                let window = window_over(mtm, &host);
                let end = words.encode_utf16().count();
                view.set_text(words);
                view.select((end, 0));
                if keyboard {
                    window.makeFirstResponder(Some(view.inner()));
                    view.select_waiting();
                }
                let state = if keyboard {
                    "with the keyboard"
                } else {
                    "without it"
                };
                match ink_in(&host) {
                    None => problems.push(format!(
                        "rows = {rows} at zoom {zoom}, {what}, {state}: nothing is drawn"
                    )),
                    Some((x0, y0, x1, y1)) => {
                        let (left, top, right, bottom) = (
                            f64::from(text.x),
                            f64::from(text.y),
                            f64::from(text.x + text.w),
                            f64::from(text.y + text.h),
                        );
                        if x0 < left - 0.5
                            || y0 < top - 0.5
                            || x1 > right + 0.5
                            || y1 > bottom + 0.5
                        {
                            problems.push(format!(
                                "rows = {rows} at zoom {zoom}, {what}, {state}: ink in ({x0}, {y0})–({x1}, {y1}), outside the text box ({left}, {top})–({right}, {bottom})"
                            ));
                        }
                    }
                }
                window.makeFirstResponder(None);
                window.setContentView(None);
            }
        }
    }
}

/// The room of a field of three rows with a border and a padding of its own on each side, as the kit
/// lays it out (`Field`: the room, its padding box — the room less the border — and the text box,
/// that less the padding), the four paddings all different: `(room, padding box, text, [left, top,
/// right, bottom])`, at `zoom` with `line` the page's line.
fn padded_boxes(zoom: f32, line: f32) -> (Rect, Rect, Rect, [f32; 4]) {
    let pads = [6.0 * zoom, 4.0 * zoom, 2.0 * zoom, 10.0 * zoom];
    let text = Rect::new(20.0, 40.0, 200.0, 3.0 * line);
    let padding_box = Rect::new(
        text.x - pads[0],
        text.y - pads[1],
        text.w + pads[0] + pads[2],
        text.h + pads[1] + pads[3],
    );
    let border = zoom;
    let room = Rect::new(
        padding_box.x - border,
        padding_box.y - border,
        padding_box.w + 2.0 * border,
        padding_box.h + 2.0 * border,
    );
    (room, padding_box, text, pads)
}

/// Scrolls the text of a field of lines so that `y` of it is the top of what the box shows.
fn scroll_lines_to(lines: &LinesView, y: f64) {
    let clip = lines.scroll.contentView();
    clip.scrollToPoint(NSPoint::new(0.0, y));
    lines.scroll.reflectScrolledClipView(&clip);
}

/// A box of rows that scrolls is cut at its padding box, not at its text box, and the padding goes
/// with the text (CSS: what scrolls is cut at the padding box): five lines in a box of three, with a
/// border and a padding of each size, are drawn from the padding box's top to its bottom between the
/// lines, at the top the first line is at the top of the text box and at the end the last line is
/// at its bottom, with the padding below it; nothing is drawn over the border. And the text is
/// wrapped at the width of the text box, which is not the padding box less the same padding on both
/// sides when the two differ.
fn lines_padding_problems(mtm: MainThreadMarker, problems: &mut Vec<String>) {
    let host_frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 300.0));
    let id = Id::named("a field");
    for (zoom, line) in [(1.0_f32, 16.0_f32), (2.0, 31.0)] {
        let (room, padding_box, text, pads) = padded_boxes(zoom, line);
        let host = FieldsHost::with_frame(mtm, host_frame);
        let view = field_of(mtm, FieldKind::Lines, Weak::new(), Some("Monaco"), 12.5);
        host.addSubview(&view);
        place_field(
            &view,
            &Field {
                id,
                kind: FieldKind::Lines,
                rect: room,
                padding_box,
                text,
                hint: None,
                label: None,
                covered: false,
            },
            zoom,
            Some("Monaco"),
        );
        let window = window_over(mtm, &host);
        let FieldView::Lines(lines) = &view else {
            continue;
        };
        view.set_text("XXXX\nXXXX\nXXXX\nXXXX\nXXXX");
        let (top, bottom) = (
            f64::from(padding_box.y),
            f64::from(padding_box.y + padding_box.h),
        );
        let (text_top, text_bottom) = (f64::from(text.y), f64::from(text.y + text.h));
        let (left, right) = (
            f64::from(padding_box.x),
            f64::from(padding_box.x + padding_box.w),
        );
        let frame = view.frame();
        if (frame.origin.x - left).abs() > 0.01
            || (frame.origin.y - top).abs() > 0.01
            || (frame.size.width - f64::from(padding_box.w)).abs() > 0.01
            || (frame.size.height - f64::from(padding_box.h)).abs() > 0.01
        {
            problems.push(format!(
                "padding at zoom {zoom}: the scroll view stands at {frame:?}, not in the padding box {padding_box:?}"
            ));
        }
        let longest = f64::from(pads[1] + pads[3]) + 5.0 * f64::from(line);
        let end = longest - f64::from(padding_box.h);
        for (what, at) in [
            ("at the top", 0.0),
            ("between lines", 0.75 * f64::from(line)),
            ("at the end", end),
        ] {
            scroll_lines_to(lines, at);
            let Some((x0, y0, x1, y1)) = ink_in(&host) else {
                problems.push(format!("padding at zoom {zoom}, {what}: nothing is drawn"));
                continue;
            };
            let said = format!("padding at zoom {zoom}, {what}: ink in ({x0}, {y0})–({x1}, {y1})");
            if y0 < top - 0.5 || y1 > bottom + 0.5 || x0 < left - 0.5 || x1 > right + 0.5 {
                problems.push(format!(
                    "{said}, outside the padding box ({left}, {top})–({right}, {bottom})"
                ));
            }
            match what {
                "at the top" => {
                    if y0 < text_top - 0.5 || y0 > text_top + f64::from(line) / 2.0 {
                        problems.push(format!(
                            "{said}: the first line is not at the top of the text box ({text_top})"
                        ));
                    }
                }
                "between lines" => {
                    if y0 > text_top - 1.0 {
                        problems.push(format!(
                            "{said}: the padding above the text box shows no line ({text_top})"
                        ));
                    }
                    if y1 < text_bottom + 1.0 {
                        problems.push(format!(
                            "{said}: the padding below the text box shows no line ({text_bottom})"
                        ));
                    }
                }
                _ => {
                    if y1 > text_bottom + 0.5 || y1 < text_bottom - f64::from(line) / 2.0 {
                        problems.push(format!(
                            "{said}: the last line is not at the bottom of the text box ({text_bottom}), with the padding below it"
                        ));
                    }
                }
            }
        }
        // The caret's line is scrolled to with the padding beyond it, so that it stands in the text
        // box: the box scrolled by a line to show the fourth, and back to the top to show the first
        // (the text view's own inset is the mean of the two paddings, which is neither).
        for (what, caret, from, wanted) in [
            ("the fourth line", 15, 0.0, f64::from(line)),
            ("the last line", 24, 0.0, end),
            ("the first line", 0, end, 0.0),
        ] {
            scroll_lines_to(lines, from);
            lines.text.setSelectedRange(NSRange::new(caret, 0));
            lines.text.scroll_to_caret();
            let at = lines.scroll.contentView().bounds().origin.y;
            if (at - wanted).abs() > 0.5 {
                problems.push(format!(
                    "padding at zoom {zoom}: the caret in {what} scrolls the box to {at}, not {wanted}"
                ));
            }
        }
        // The text wraps at the text box's width: the box is `text.w` of the container, however the
        // padding is shared, and a word that does not fit breaks where the width ends.
        // SAFETY: the text view was made with its container.
        let width = unsafe { lines.text.textContainer() }.map_or(0.0, |c| c.size().width);
        if (width - f64::from(text.w)).abs() > 0.01 {
            problems.push(format!(
                "padding at zoom {zoom}: the text wraps at {width} points, the text box is {} wide",
                text.w
            ));
        }
        view.set_text(&"X".repeat(70));
        scroll_lines_to(lines, 0.0);
        if let Some((x0, _, x1, _)) = ink_in(&host) {
            let (box_left, box_right) = (f64::from(text.x), f64::from(text.x + text.w));
            if x0 < box_left - 0.5 || x1 > box_right + 0.5 {
                problems.push(format!(
                    "padding at zoom {zoom}: a long word is drawn from {x0} to {x1}, the text box is {box_left}–{box_right}"
                ));
            }
        }
        // Where the text is, by every way AppKit has of saying it: a press at a point of the text
        // box is the character under it, the selection is painted from the text box's top left, and
        // the caret's rectangle is where the character is — with the paddings all different, the
        // text view's own inset is not where the text starts.
        view.set_text("XXXX\nXXXX\nXXXX");
        scroll_lines_to(lines, 0.0);
        let font = lines.text.font();
        let advance = font
            .as_ref()
            .map_or(7.5, |font| font.maximumAdvancement().width);
        let (to_left, to_top) = (f64::from(pads[0]) + SIDE_PADDING, f64::from(pads[1]));
        for (row, column) in [(0_usize, 0_usize), (1, 2), (2, 3)] {
            let at = lines.text.convertPoint_fromView(
                host.convertPoint_toView(
                    NSPoint::new(
                        f64::from(padding_box.x) + to_left + advance * (column as f64 + 0.4),
                        f64::from(padding_box.y) + to_top + f64::from(line) * (row as f64 + 0.1),
                    ),
                    None,
                ),
                None,
            );
            let got = lines.text.characterIndexForInsertionAtPoint(at);
            let wanted = row * 5 + column;
            if got != wanted {
                problems.push(format!(
                    "padding at zoom {zoom}: a press at line {row}, column {column} + 0.4 is the character {got}, not {wanted}"
                ));
            }
        }
        window.makeFirstResponder(Some(view.inner()));
        lines.text.setSelectedRange(NSRange::new(0, 4));
        match ink_in(&host) {
            Some((x0, y0, _, _)) => {
                let (want_x, want_y) = (
                    f64::from(padding_box.x) + to_left,
                    f64::from(padding_box.y) + to_top,
                );
                if (x0 - want_x).abs() > 0.6 || (y0 - want_y).abs() > 0.6 {
                    problems.push(format!(
                        "padding at zoom {zoom}: the selection of the first four characters starts at ({x0}, {y0}), the text box's top left plus the line's padding is ({want_x}, {want_y})"
                    ));
                }
            }
            None => problems.push(format!("padding at zoom {zoom}: a selection draws nothing")),
        }
        lines.text.setSelectedRange(NSRange::new(7, 0));
        // SAFETY: NSTextInputClient's `firstRectForCharacterRange:actualRange:`, which an
        // `NSTextView` answers: a range, and a null for the actual range (not wanted).
        let screen: NSRect = unsafe {
            msg_send![&*lines.text, firstRectForCharacterRange: NSRange::new(7, 0), actualRange: std::ptr::null_mut::<NSRange>()]
        };
        let in_view = host.convertPoint_fromView(window.convertRectFromScreen(screen).origin, None);
        // The caret's rectangle is in the screen's points, from the bottom: its top is the line's.
        let top = in_view.y - screen.size.height;
        let (want_x, want_y) = (
            f64::from(padding_box.x) + to_left + advance * 2.0,
            f64::from(padding_box.y) + to_top + f64::from(line),
        );
        if (in_view.x - want_x).abs() > 1.0 || (top - want_y).abs() > 1.0 {
            problems.push(format!(
                "padding at zoom {zoom}: the caret of the character 7 is at ({}, {top}), the text box's third character of the second line is at ({want_x}, {want_y})",
                in_view.x
            ));
        }
        window.makeFirstResponder(None);
        window.setContentView(None);
    }
}

/// A box of rows that scrolls has its bar, and a press on it is the bar's, which AppKit's own scroller
/// then tracks (the knob dragged, the track paged): not the text's, which the room's press is
/// (`FieldsHost`, where a point of the scroll view's own frame is asked of the scroll view first,
/// and a point of the room outside it is the text's). The press itself is routed here, as the
/// pointer is over the bar; what the scroller does with it is AppKit's, and dragging it is not
/// measured without a mouse (a bar tracks the events of the system, which a test cannot send).
fn lines_bar_problems(mtm: MainThreadMarker, problems: &mut Vec<String>) {
    let host_frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 300.0));
    let id = Id::named("a field");
    let (room, padding_box, text, _) = padded_boxes(1.0, 16.0);
    let host = FieldsHost::with_frame(mtm, host_frame);
    let view = field_of(mtm, FieldKind::Lines, Weak::new(), Some("Monaco"), 12.5);
    host.addSubview(&view);
    place_field(
        &view,
        &Field {
            id,
            kind: FieldKind::Lines,
            rect: room,
            padding_box,
            text,
            hint: None,
            label: None,
            covered: false,
        },
        1.0,
        Some("Monaco"),
    );
    let window = window_over(mtm, &host);
    let FieldView::Lines(lines) = &view else {
        return;
    };
    view.set_text("1\n2\n3\n4\n5\n6\n7\n8\n9\n10");
    lines.scroll.tile();
    // The overlay bar is there while it is shown (it is shown as the text is scrolled, and where the
    // pointer is over it).
    lines.scroll.flashScrollers();
    let Some(scroller) = lines.scroll.verticalScroller() else {
        problems.push("bar: a field of lines has no vertical scroller".to_owned());
        window.setContentView(None);
        return;
    };
    // SAFETY: reading the superview, on the main thread.
    let above = unsafe { host.superview() };
    let hit = |at: NSPoint| host.hitTest(host.convertPoint_toView(at, above.as_deref()));
    // The bar has a knob and a track to press: a measure of nothing would put every point at the
    // bar's top.
    let (knob, slot) = (
        scroller.rectForPart(NSScrollerPart::Knob),
        scroller.rectForPart(NSScrollerPart::KnobSlot),
    );
    if knob.size.height < 1.0 || slot.size.height <= knob.size.height + 1.0 {
        problems.push(format!(
            "bar: the bar has a knob of {knob:?} in a track of {slot:?}, which a press is not measured on"
        ));
    }
    let centre_of = |part: NSScrollerPart| {
        let rect = scroller.rectForPart(part);
        host.convertPoint_fromView(
            NSPoint::new(
                rect.origin.x + rect.size.width / 2.0,
                rect.origin.y + rect.size.height / 2.0,
            ),
            Some(&scroller),
        )
    };
    // The text is scrolled to the end, where the knob is at the bottom of its track: the track
    // above the knob is a press to page up.
    for (what, part) in [
        ("knob", NSScrollerPart::Knob),
        ("track", NSScrollerPart::KnobSlot),
    ] {
        let at = if part == NSScrollerPart::KnobSlot {
            // The middle of the track above the knob.
            let (slot, knob) = (
                scroller.rectForPart(NSScrollerPart::KnobSlot),
                scroller.rectForPart(NSScrollerPart::Knob),
            );
            host.convertPoint_fromView(
                NSPoint::new(
                    slot.origin.x + slot.size.width / 2.0,
                    slot.origin.y + (knob.origin.y - slot.origin.y) / 2.0,
                ),
                Some(&scroller),
            )
        } else {
            centre_of(part)
        };
        let got = hit(at);
        if !got
            .as_ref()
            .is_some_and(|view| view.downcast_ref::<NSScroller>().is_some())
        {
            problems.push(format!(
                "bar: a press on the {what} of the bar at {at:?} is {:?}, not the bar's",
                got.as_ref().map(|view| view.class())
            ));
        }
    }
    // In the room and outside the scroll view, on the border: the text's.
    let border = hit(NSPoint::new(
        f64::from(room.x) + 0.2,
        f64::from(room.y + room.h / 2.0),
    ));
    if !border.is_some_and(|view| view.downcast_ref::<PageLinesText>().is_some()) {
        problems.push("bar: a press on the border of the room is not the text's".to_owned());
    }
    window.setContentView(None);
}

/// A box of rows that the page lays out again, with the paddings changed (the text box moves in the
/// padding box, and may not be as wide), shows its text where the new text box is: the text start,
/// the width it wraps at and a display to draw it again; a caret after a line break at the end of the
/// text is scrolled to with the empty line it is in, and a box shorter than its line (a border
/// around it) holds the line and stays in its room.
fn lines_changing_problems(mtm: MainThreadMarker, problems: &mut Vec<String>) {
    let host_frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 300.0));
    let id = Id::named("a field");
    let field = |room: Rect, padding_box: Rect, text: Rect| Field {
        id,
        kind: FieldKind::Lines,
        rect: room,
        padding_box,
        text,
        hint: None,
        label: None,
        covered: false,
    };
    for (zoom, line) in [(1.0_f32, 16.0_f32), (2.0, 31.0)] {
        let (room, padding_box, text, pads) = padded_boxes(zoom, line);
        let host = FieldsHost::with_frame(mtm, host_frame);
        let view = field_of(mtm, FieldKind::Lines, Weak::new(), Some("Monaco"), 12.5);
        host.addSubview(&view);
        place_field(&view, &field(room, padding_box, text), zoom, Some("Monaco"));
        let window = window_over(mtm, &host);
        let FieldView::Lines(lines) = &view else {
            continue;
        };
        // A line break at the end of the text: the caret is in the empty line after it, which the
        // box is scrolled to with the padding below.
        let longest = f64::from(pads[1] + pads[3]) + 5.0 * f64::from(line);
        let end = longest - f64::from(padding_box.h);
        view.set_text("XXXX\nXXXX\nXXXX\nXXXX\n");
        scroll_lines_to(lines, 0.0);
        lines.text.setSelectedRange(NSRange::new(20, 0));
        lines.text.scroll_to_caret();
        let at = lines.scroll.contentView().bounds().origin.y;
        if (at - end).abs() > 0.5 {
            problems.push(format!(
                "changing at zoom {zoom}: the caret in the empty line after the last line break scrolls the box to {at}, not {end}"
            ));
        }
        // The paddings swapped (the same sums: the box and the text view are as large as they were)
        // and then the sides more than the text box was: where the text starts, what it wraps at.
        for (what, new) in [
            ("swapped", [pads[2], pads[3], pads[0], pads[1]]),
            (
                "wider",
                [pads[0] + 4.0 * zoom, pads[1], pads[2] + 4.0 * zoom, pads[3]],
            ),
        ] {
            let text = Rect::new(
                padding_box.x + new[0],
                padding_box.y + new[1],
                padding_box.w - new[0] - new[2],
                padding_box.h - new[1] - new[3],
            );
            lines.text.setNeedsDisplay(false);
            place_field(&view, &field(room, padding_box, text), zoom, Some("Monaco"));
            if !lines.text.needsDisplay() {
                problems.push(format!(
                    "changing at zoom {zoom}, paddings {what}: the text is not to be drawn again"
                ));
            }
            // SAFETY: the text view was made with its container.
            let width = unsafe { lines.text.textContainer() }.map_or(0.0, |c| c.size().width);
            if (width - f64::from(text.w)).abs() > 0.01 {
                problems.push(format!(
                    "changing at zoom {zoom}, paddings {what}: the text wraps at {width} points, the text box is {} wide",
                    text.w
                ));
            }
            view.set_text(&"X".repeat(70));
            scroll_lines_to(lines, 0.0);
            match ink_in(&host) {
                Some((x0, y0, x1, _)) => {
                    let (left, right) = (f64::from(text.x), f64::from(text.x + text.w));
                    let top = f64::from(text.y);
                    if x0 < left - 0.5
                        || x1 > right + 0.5
                        || y0 < top - 0.5
                        || y0 > top + f64::from(line) / 2.0
                    {
                        problems.push(format!(
                            "changing at zoom {zoom}, paddings {what}: ink from ({x0}, {y0}) to {x1} across, the text box is {left}–{right} from {top}"
                        ));
                    }
                }
                None => problems.push(format!(
                    "changing at zoom {zoom}, paddings {what}: nothing is drawn"
                )),
            }
        }
        window.setContentView(None);
    }
    // A box shorter than its line, with a border: the scroll view holds the line below the padding
    // over it and stays in the room (the border and what is below are not its), and the line is in
    // view with the caret in it.
    let host = FieldsHost::with_frame(mtm, host_frame);
    let view = field_of(mtm, FieldKind::Lines, Weak::new(), Some("Monaco"), 12.5);
    host.addSubview(&view);
    let room = Rect::new(12.0, 30.0, 216.0, 24.0);
    let padding_box = Rect::new(13.0, 31.0, 214.0, 22.0);
    let text = Rect::new(19.0, 40.0, 202.0, 4.0);
    place_field(&view, &field(room, padding_box, text), 1.0, Some("Monaco"));
    let window = window_over(mtm, &host);
    if let FieldView::Lines(lines) = &view {
        let frame = view.frame();
        if frame.origin.y + frame.size.height > f64::from(room.y + room.h) + 0.01 {
            problems.push(format!(
                "short box: the scroll view stands at {frame:?}, past the room {room:?}"
            ));
        }
        view.set_text("XXXX");
        lines.text.setSelectedRange(NSRange::new(2, 0));
        lines.text.scroll_to_caret();
        let seen = lines.scroll.contentView().bounds();
        let (top, bottom) = (9.0, 25.0);
        if seen.origin.y > top + 0.5 || seen.origin.y + frame.size.height < bottom - 0.5 {
            problems.push(format!(
                "short box: the caret's line ({top}–{bottom} of the text) is not in view: {}–{}",
                seen.origin.y,
                seen.origin.y + frame.size.height
            ));
        }
    }
    window.setContentView(None);
}

/// What a field of lines takes of what is dropped on it is what a field of one line takes: nothing
/// while the keyboard is elsewhere (a drop is the page's: its files are the page's to take), and
/// what its editor takes while it is typed in (text, and a file as its path). The field's editor is
/// AppKit's own text view, so what it has registered for is the measure; a field of one line is
/// the control.
fn lines_drop_problems(mtm: MainThreadMarker, problems: &mut Vec<String>) {
    let host_frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 300.0));
    let id = Id::named("a field");
    let types = |view: &NSView| {
        let mut names: Vec<String> = view
            .registeredDraggedTypes()
            .iter()
            .map(|name| name.to_string())
            .collect();
        names.sort();
        names
    };
    // What each kind has registered, `[elsewhere, typed in, elsewhere again]`.
    let mut seen: Vec<(FieldKind, [Vec<String>; 3])> = Vec::new();
    for kind in [FieldKind::Line, FieldKind::Lines] {
        let host = FieldsHost::with_frame(mtm, host_frame);
        let view = field_of(mtm, kind, Weak::new(), Some("Monaco"), 12.5);
        host.addSubview(&view);
        let text = Rect::new(20.0, 36.0, 200.0, 48.0);
        let room = Rect::new(12.0, 30.0, 216.0, 60.0);
        place_field(
            &view,
            &Field {
                id,
                kind,
                rect: room,
                padding_box: room,
                text,
                hint: None,
                label: None,
                covered: false,
            },
            1.0,
            Some("Monaco"),
        );
        let window = window_over(mtm, &host);
        let own = |view: &FieldView| match view {
            FieldView::Lines(lines) => types(&lines.text),
            _ => view
                .text_field()
                .and_then(|field| field.currentEditor())
                .and_then(|editor| editor.downcast_ref::<NSTextView>().map(|text| types(text)))
                .unwrap_or_default(),
        };
        let away = (types(&view), types(view.inner()));
        window.makeFirstResponder(Some(view.inner()));
        let typed = own(&view);
        window.makeFirstResponder(None);
        let after = (types(&view), types(view.inner()));
        if !away.0.is_empty() || !away.1.is_empty() || !after.0.is_empty() || !after.1.is_empty() {
            problems.push(format!(
                "drop on a {kind:?} with the keyboard elsewhere: registered {away:?}, {after:?} after"
            ));
        }
        seen.push((kind, [away.1, typed, after.1]));
        window.setContentView(None);
    }
    if let [(_, line), (_, lines)] = &seen[..] {
        if line[1].is_empty() {
            problems
                .push("drop: a field of one line takes nothing while it is typed in".to_owned());
        }
        if line[1] != lines[1] {
            problems.push(format!(
                "drop while typed in: a field of lines takes {:?}, a field of one line {:?}",
                lines[1], line[1]
            ));
        }
    }
}

/// What a field of lines is, beside a field of text (the one it replaces): its text stands where a
/// line's does and says its hint in the same ink; it is a text area to a screen reader, once, with
/// the name and the hint of the page's; it is as wide as the box, whatever the scroll bar; the line of
/// its text is the line the page is told in every face tried; it has the substitutions a field's
/// editor has; and the caret stays in view as the person types.
fn lines_problems(mtm: MainThreadMarker, problems: &mut Vec<String>) {
    let host_frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 300.0));
    let id = Id::named("a field");
    let field = |kind: FieldKind, text: Rect, hint: Option<&str>, label: Option<&str>| Field {
        id,
        kind,
        rect: Rect::new(text.x - 8.0, text.y - 6.0, text.w + 16.0, text.h + 12.0),
        padding_box: Rect::new(text.x - 8.0, text.y - 6.0, text.w + 16.0, text.h + 12.0),
        text,
        hint: hint.map(str::to_owned),
        label: label.map(str::to_owned),
        covered: false,
    };
    let close = |one: f64, other: f64| (one - other).abs() <= 0.5;
    // Standing beside a field of one line, in each face and at each size: the first letter and the
    // hint, across and down (the capitals' middle is the box's middle, as `text_top` puts a line's;
    // a box of one row is a line tall), and the colour of the hint.
    for family in [Some("Monaco"), Some("SF Mono"), None] {
        for zoom in [1.0_f32, 1.25, 2.0] {
            let pad = 6.0 * zoom;
            let line = (line_of(mtm, family, zoom) * zoom).round();
            let text = Rect::new(20.0, 30.0 + pad, 200.0, line);
            let ink = |kind: FieldKind, words: &str, hint: Option<&str>| {
                let host = FieldsHost::with_frame(mtm, host_frame);
                let view = field_of(mtm, kind, Weak::new(), family, 12.5);
                host.addSubview(&view);
                place_field(&view, &field(kind, text, hint, None), zoom, family);
                view.set_text(words);
                let window = window_over(mtm, &host);
                let ink = ink_of(&host);
                window.setContentView(None);
                ink
            };
            for (what, words, hint) in [("its text", "XXXX", None), ("its hint", "", Some("XXXX"))]
            {
                let (Some(one), Some(many)) = (
                    ink(FieldKind::Line, words, hint),
                    ink(FieldKind::Lines, words, hint),
                ) else {
                    problems.push(format!(
                        "{family:?} at zoom {zoom}: {what} of a field is not drawn"
                    ));
                    continue;
                };
                if !(close(one.0, many.0) && close(one.2, many.2)) {
                    problems.push(format!(
                        "{family:?} at zoom {zoom}: {what} of lines stands across {:?}, a line's across {:?}",
                        (many.0, many.2),
                        (one.0, one.2)
                    ));
                }
                if !close((one.1 + one.3) / 2.0, (many.1 + many.3) / 2.0) {
                    problems.push(format!(
                        "{family:?} at zoom {zoom}: {what} of lines stands {:?} down, a line's {:?}",
                        (many.1, many.3),
                        (one.1, one.3)
                    ));
                }
                if one.4.iter().zip(&many.4).any(|(a, b)| a.abs_diff(*b) > 8) {
                    problems.push(format!(
                        "{family:?} at zoom {zoom}: {what} of lines is in {:?}, a line's in {:?}",
                        many.4, one.4
                    ));
                }
            }
        }
    }
    // The box: the text is as wide as it, the scroll bar the overlay one, the field a text area read once.
    for zoom in [1.0_f32, 2.0] {
        let host = FieldsHost::with_frame(mtm, host_frame);
        let view = field_of(mtm, FieldKind::Lines, Weak::new(), Some("Monaco"), 12.5);
        host.addSubview(&view);
        let text = Rect::new(20.0, 36.0, 200.0, 48.0 * zoom);
        place_field(
            &view,
            &field(FieldKind::Lines, text, Some("Write here"), Some("Notes")),
            zoom,
            Some("Monaco"),
        );
        let FieldView::Lines(lines) = &view else {
            unreachable!("a field of lines");
        };
        view.set_text("one\ntwo\nthree\nfour\nfive\nsix");
        // The scroll view is the padding box (the room here: 8 across on each side), and the text
        // wraps at the text box's width, whatever the bar.
        let across = lines.scroll.contentSize().width;
        // SAFETY: the text view was made with its container.
        let wraps = unsafe { lines.text.textContainer() }.map_or(0.0, |c| c.size().width);
        if (across - f64::from(text.w + 16.0)).abs() > 0.01
            || (lines.text.frame().size.width - f64::from(text.w + 16.0)).abs() > 0.01
            || (wraps - f64::from(text.w)).abs() > 0.01
        {
            problems.push(format!(
                "lines at zoom {zoom}: the scroll view is {across} ({}) across and wraps at {wraps}, the box {} and its text box {}",
                lines.text.frame().size.width,
                text.w + 16.0,
                text.w
            ));
        }
        if lines.scroll.scrollerStyle() != NSScrollerStyle::Overlay {
            problems.push("lines: the scroll bar takes room of the text's".to_owned());
        }
        if lines.text.frame().size.height < f64::from(text.h) {
            problems.push(format!(
                "lines at zoom {zoom}: the text is {} tall, less than the box's {}",
                lines.text.frame().size.height,
                text.h
            ));
        }
        // What a screen reader is given: the text view, as a text area, named and with the hint, and
        // nothing of the scroll view; each object of the field once.
        let read = view.read();
        let said = || -> Option<String> {
            // SAFETY: the object is one of AppKit's views, which answers NSAccessibility.
            let words: Option<Retained<NSString>> =
                unsafe { msg_send![&*read, accessibilityLabel] };
            words.map(|words| words.to_string())
        };
        if said().as_deref() != Some("Notes") {
            problems.push("lines: a screen reader is not told the field's name".to_owned());
        }
        // SAFETY: as above.
        let (role, hint): (Option<Retained<NSString>>, Option<Retained<NSString>>) = unsafe {
            (
                msg_send![&*read, accessibilityRole],
                msg_send![&*read, accessibilityPlaceholderValue],
            )
        };
        if role.map(|role| role.to_string())
            != Some(unsafe { NSAccessibilityTextAreaRole }.to_string())
        {
            problems.push("lines: a screen reader does not read a text area".to_owned());
        }
        if hint.map(|hint| hint.to_string()).as_deref() != Some("Write here") {
            problems.push("lines: a screen reader is not told the field's hint".to_owned());
        }
        let owned = view.owned();
        let has = |object: &AnyObject| {
            owned
                .iter()
                .filter(|held| std::ptr::eq(&***held, object))
                .count()
        };
        let (scroll, text_view): (&AnyObject, &AnyObject) = (&lines.scroll, &lines.text);
        if has(scroll) != 1 || has(text_view) != 1 {
            problems.push(format!(
                "lines: the field holds its scroll view {} times and its text view {}, of the page's own",
                has(scroll),
                has(text_view)
            ));
        }
        if lines.scroll.isAccessibilityElement() {
            problems.push(
                "lines: the scroll view is an element of its own to a screen reader".to_owned(),
            );
        }
        let hint_is_read = lines
            .text
            .ivars()
            .hint
            .borrow()
            .as_ref()
            .and_then(|(_, hint)| hint.cell())
            .is_none_or(|cell| cell.isAccessibilityElement())
            || lines
                .text
                .ivars()
                .hint
                .borrow()
                .as_ref()
                .is_none_or(|(_, hint)| hint.isAccessibilityElement());
        if hint_is_read {
            problems.push(
                "lines: the hint's own text field is an element of its own to a screen reader"
                    .to_owned(),
            );
        }
        view.set_read(false);
        if lines.text.isAccessibilityElement() {
            problems.push("lines: a field the page shut out is read".to_owned());
        }
        view.set_read(true);
        if !lines.text.isAccessibilityElement() {
            problems.push("lines: a field the page lets in is not read".to_owned());
        }
    }
    // N rows of a field of lines are N of the lines the page is told (`line_of`), in the faces the
    // terminal is set in, at the sizes the page is drawn at: the page lays its rows out with one
    // number, and the text is laid out in it.
    for family in [
        Some("Monaco"),
        Some("Menlo"),
        Some("SF Mono"),
        Some("Courier New"),
        None,
    ] {
        for zoom in [1.0_f32, 1.25, 1.5, 2.0, 3.0] {
            let told = f64::from(line_of(mtm, family, zoom)) * f64::from(zoom);
            let host = FieldsHost::with_frame(mtm, host_frame);
            let view = field_of(mtm, FieldKind::Lines, Weak::new(), family, 12.5);
            host.addSubview(&view);
            let text = Rect::new(20.0, 36.0, 200.0, (3.0 * told) as f32);
            place_field(
                &view,
                &field(FieldKind::Lines, text, None, None),
                zoom,
                family,
            );
            let FieldView::Lines(lines) = &view else {
                unreachable!("a field of lines");
            };
            lines.text.setString(&NSString::from_str("x\nx\nx"));
            // SAFETY: the text view was made with its layout manager and container.
            let (manager, container) =
                unsafe { (lines.text.layoutManager(), lines.text.textContainer()) };
            let (manager, container) = (
                manager.expect("a layout manager"),
                container.expect("a container"),
            );
            manager.ensureLayoutForTextContainer(&container);
            let laid = manager.usedRectForTextContainer(&container).size.height;
            if (laid - 3.0 * told).abs() > 0.01 {
                problems.push(format!(
                    "{family:?} at zoom {zoom}: three lines of lines are {laid} points, three of the page's {told}"
                ));
            }
        }
    }
    // The substitutions of the field's editor, asked of one in a window: the text view has them.
    {
        let host = FieldsHost::with_frame(mtm, host_frame);
        let view = field_of(mtm, FieldKind::Line, Weak::new(), Some("Monaco"), 12.5);
        host.addSubview(&view);
        view.setFrame(NSRect::new(
            NSPoint::new(20.0, 30.0),
            NSSize::new(200.0, 16.0),
        ));
        let window = window_over(mtm, &host);
        let editor = window
            .makeFirstResponder(Some(view.inner()))
            .then(|| view.text_field().and_then(|field| field.currentEditor()))
            .flatten()
            .and_then(|editor| editor.downcast::<NSTextView>().ok());
        if let Some(editor) = editor {
            let lines = field_of(mtm, FieldKind::Lines, Weak::new(), Some("Monaco"), 12.5);
            let FieldView::Lines(lines) = &lines else {
                unreachable!("a field of lines");
            };
            let asks: [(&str, &Ask); 6] = [
                ("quotes", &|text| text.isAutomaticQuoteSubstitutionEnabled()),
                ("dashes", &|text| text.isAutomaticDashSubstitutionEnabled()),
                ("text replacement", &|text| {
                    text.isAutomaticTextReplacementEnabled()
                }),
                ("links", &|text| text.isAutomaticLinkDetectionEnabled()),
                ("spelling", &|text| text.isContinuousSpellCheckingEnabled()),
                ("undo", &|text| text.allowsUndo()),
            ];
            for (name, ask) in asks {
                let (field, own) = (ask(&editor), ask(&lines.text));
                if field != own {
                    problems.push(format!(
                        "lines: {name} are {own}, a field's editor's {field}"
                    ));
                }
            }
            window.makeFirstResponder(None);
        } else {
            problems.push(
                "a line took no keyboard in a window of its own: its editor is not tried"
                    .to_owned(),
            );
        }
        window.setContentView(None);
    }
    // The caret stays in view as the person types: four lines typed in a box of three, the last is
    // seen, the box scrolled by a line; a line more, and by two.
    for zoom in [1.0_f32, 2.0] {
        let line = (line_of(mtm, Some("Monaco"), zoom) * zoom).round();
        let host = FieldsHost::with_frame(mtm, host_frame);
        let view = field_of(mtm, FieldKind::Lines, Weak::new(), Some("Monaco"), 12.5);
        host.addSubview(&view);
        let text = Rect::new(20.0, 36.0, 200.0, 3.0 * line);
        place_field(
            &view,
            &field(FieldKind::Lines, text, None, None),
            zoom,
            Some("Monaco"),
        );
        let window = window_over(mtm, &host);
        window.makeFirstResponder(Some(view.inner()));
        let FieldView::Lines(lines) = &view else {
            unreachable!("a field of lines");
        };
        for typed in 1..=5 {
            let words = NSString::from_str(if typed == 1 { "X" } else { "\nX" });
            // SAFETY: `insertText:replacementRange:` is NSTextInputClient's, which an `NSTextView`
            // answers; `NSNotFound` as the location stands for the selection.
            let _: () = unsafe {
                msg_send![&*lines.text, insertText: &*words, replacementRange: NSRange::new(isize::MAX as usize, 0)]
            };
            let seen = lines.scroll.contentView().bounds().origin.y;
            let low = f64::from(typed.max(3) - 3) * f64::from(line);
            if (seen - low).abs() > 0.01 {
                problems.push(format!(
                    "lines at zoom {zoom}: {typed} lines typed in a box of three, the box is scrolled {seen} down, to {low}"
                ));
            }
        }
        // Drawn in the padding box (the room: the text box and 8 across, 6 up and down), and the
        // line the caret is in, the last, is in the text box with the padding below it.
        match ink_in(&host) {
            Some((x0, y0, x1, y1))
                if x0 >= f64::from(text.x) - 8.5
                    && y0 >= f64::from(text.y) - 6.5
                    && x1 <= f64::from(text.x + text.w) + 8.5
                    && y1 <= f64::from(text.y + text.h) + 0.5 => {}
            other => problems.push(format!(
                "lines at zoom {zoom}: five lines typed in a box of three draw {other:?}, not within the box"
            )),
        }
        window.makeFirstResponder(None);
        window.setContentView(None);
    }
    // A field that is told another face of the same size is laid out in that one's line, and one the
    // system moved to the classic scroll bar is the page's overlay again.
    {
        let host = FieldsHost::with_frame(mtm, host_frame);
        let text = Rect::new(20.0, 36.0, 200.0, 48.0);
        let place = |view: &FieldView, family: Option<&str>| {
            place_field(
                view,
                &field(FieldKind::Lines, text, None, None),
                1.25,
                family,
            );
        };
        let moved = field_of(mtm, FieldKind::Lines, Weak::new(), Some("Monaco"), 12.5);
        host.addSubview(&moved);
        place(&moved, Some("Monaco"));
        place(&moved, Some("SF Mono"));
        let fresh = field_of(mtm, FieldKind::Lines, Weak::new(), Some("SF Mono"), 12.5);
        host.addSubview(&fresh);
        place(&fresh, Some("SF Mono"));
        let (a, b) = (moved.ivars(), fresh.ivars());
        if a.line.get() != b.line.get() || a.baseline.get() != b.baseline.get() {
            problems.push(format!(
                "lines: a field moved from Monaco to SF Mono at the same size has a line of {} and a baseline of {}, one made in it {} and {}",
                a.line.get(),
                a.baseline.get(),
                b.line.get(),
                b.baseline.get()
            ));
        }
        if let FieldView::Lines(lines) = &moved {
            lines.scroll.setScrollerStyle(NSScrollerStyle::Legacy);
            place(&moved, Some("SF Mono"));
            if lines.scroll.scrollerStyle() != NSScrollerStyle::Overlay
                || (lines.scroll.contentSize().width - f64::from(text.w + 16.0)).abs() > 0.01
            {
                problems.push(format!(
                    "lines: a scroll view the system moved to the classic bar is not the page's overlay again: its style is {:?} and the text {} wide, not {}",
                    lines.scroll.scrollerStyle(),
                    lines.scroll.contentSize().width,
                    text.w + 16.0
                ));
            }
        }
    }
    // What the person typed is not undone onto a text the page put in after it.
    {
        let host = FieldsHost::with_frame(mtm, host_frame);
        let view = field_of(mtm, FieldKind::Lines, Weak::new(), Some("Monaco"), 12.5);
        host.addSubview(&view);
        let text = Rect::new(20.0, 36.0, 200.0, 48.0);
        place_field(
            &view,
            &field(FieldKind::Lines, text, None, None),
            1.0,
            Some("Monaco"),
        );
        let window = window_over(mtm, &host);
        window.makeFirstResponder(Some(view.inner()));
        let FieldView::Lines(lines) = &view else {
            unreachable!("a field of lines");
        };
        let words = NSString::from_str("typed words");
        // SAFETY: `insertText:replacementRange:` is NSTextInputClient's, which an `NSTextView`
        // answers; `NSNotFound` as the location stands for the selection.
        let _: () = unsafe {
            msg_send![&*lines.text, insertText: &*words, replacementRange: NSRange::new(isize::MAX as usize, 0)]
        };
        view.set_text("x");
        let typed = NSString::from_str("y");
        // SAFETY: as above.
        let _: () = unsafe {
            msg_send![&*lines.text, insertText: &*typed, replacementRange: NSRange::new(isize::MAX as usize, 0)]
        };
        if let Some(manager) = lines.text.undoManager() {
            // Only what is typed after the text was put in is undone, and without AppKit raising.
            if manager.canUndo() {
                manager.undo();
            }
        }
        // The end of the event: what the managers have queued, with the field's old one let go.
        objc2_foundation::NSRunLoop::currentRunLoop().runUntilDate(
            &objc2_foundation::NSDate::dateWithTimeIntervalSinceNow(0.05),
        );
        if view.text() != "x" {
            problems.push(format!(
                "lines: undo after the page put a text in left {:?}, not that text",
                view.text()
            ));
        }
        window.makeFirstResponder(None);
        window.setContentView(None);
    }
    // A press in the padding of a box that is scrolled down is where it is in the text, which the
    // padding scrolls with (the lines in view are the caret's), and a press in the border, outside what
    // the box shows, puts the caret in the lines in view, not on the line past the edge of the text there
    // that the box scrolled away from; and a field that left the page is freed.
    {
        let host = FieldsHost::with_frame(mtm, host_frame);
        let view = field_of(mtm, FieldKind::Lines, Weak::new(), Some("Monaco"), 12.5);
        host.addSubview(&view);
        let text = Rect::new(20.0, 36.0, 200.0, 48.0);
        let room = Rect::new(12.0, 30.0, 216.0, 60.0);
        let line = Field {
            id,
            kind: FieldKind::Lines,
            rect: room,
            padding_box: Rect::new(15.0, 33.0, 210.0, 54.0),
            text,
            hint: None,
            label: None,
            covered: false,
        };
        place_field(&view, &line, 1.0, Some("Monaco"));
        view.set_text("1\n2\n3\n4\n5\n6\n7\n8");
        let window = window_over(mtm, &host);
        if let FieldView::Lines(lines) = &view {
            if lines.text.registeredDraggedTypes().count() != 0 {
                problems.push("lines: the text view takes a drop of files itself".to_owned());
            }
            // Scrolled to the end: the first of the lines in view is the sixth.
            lines.text.setSelectedRange(NSRange::new(15, 0));
            lines.text.scroll_to_caret();
            let origin = lines.scroll.contentView().bounds().origin.y;
            for (name, y, from, to) in [
                ("below, in the border,", 89.0, 14, 15),
                ("above, in the border,", 31.0, 10, 11),
                ("below, in the padding,", 86.0, 14, 15),
                ("above, in the padding,", 34.0, 8, 11),
            ] {
                let in_window = host.convertPoint_toView(NSPoint::new(100.0, y), None);
                let event = |kind| {
                    NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                        kind,
                        in_window,
                        NSEventModifierFlags::empty(),
                        0.0,
                        window.windowNumber(),
                        None,
                        0,
                        1,
                        1.0,
                    )
                };
                if let (Some(press), Some(release)) = (
                    event(NSEventType::LeftMouseDown),
                    event(NSEventType::LeftMouseUp),
                ) {
                    // A press the text view tracks (one in what the box shows) follows the mouse
                    // until it is released: the release is in the queue before the press is.
                    NSApplication::sharedApplication(mtm).postEvent_atStart(&release, true);
                    lines.text.setSelectedRange(NSRange::new(0, 0));
                    lines.text.mouseDown(&press);
                    let at = lines.text.selectedRange().location;
                    if at < from || at > to {
                        problems.push(format!(
                            "lines: a press {name} of the box, scrolled to the end, puts the caret at {at}, not in the lines in view ({from}..={to})"
                        ));
                    }
                    if (lines.scroll.contentView().bounds().origin.y - origin).abs() > 0.5 {
                        problems.push(format!("lines: a press {name} of the box scrolls it"));
                    }
                    lines.text.scrollPoint(NSPoint::new(0.0, origin));
                } else {
                    problems.push("lines: no mouse event to press the padding with".to_owned());
                }
            }
        }
        window.makeFirstResponder(None);
        window.setContentView(None);
    }
    // Nor what was typed in a field that has left the page.
    {
        let host = FieldsHost::with_frame(mtm, host_frame);
        let view = field_of(mtm, FieldKind::Lines, Weak::new(), Some("Monaco"), 12.5);
        host.addSubview(&view);
        let text = Rect::new(20.0, 36.0, 200.0, 48.0);
        place_field(
            &view,
            &field(FieldKind::Lines, text, None, None),
            1.0,
            Some("Monaco"),
        );
        let window = window_over(mtm, &host);
        window.makeFirstResponder(Some(view.inner()));
        if let FieldView::Lines(lines) = &view {
            let words = NSString::from_str("typed words");
            // SAFETY: as above.
            let _: () = unsafe {
                msg_send![&*lines.text, insertText: &*words, replacementRange: NSRange::new(isize::MAX as usize, 0)]
            };
            if !lines
                .text
                .undoManager()
                .is_some_and(|manager| manager.canUndo())
            {
                problems.push(
                    "lines: typing registers nothing to undo, so the test below says nothing"
                        .to_owned(),
                );
            }
            view.take_away();
            if lines
                .text
                .undoManager()
                .is_some_and(|manager| manager.canUndo())
            {
                problems.push(
                    "lines: what was typed in a field that was taken away can be undone".to_owned(),
                );
            }
        }
        window.makeFirstResponder(None);
        window.setContentView(None);
    }
    // Where the caret is, for the kit's rule of keys (`FieldState`): none while the keyboard is
    // elsewhere, the length and the selection in UTF-16 units while it is in the field; and a
    // command the page did not take (⏎, here no page) is the text view's own, a line break.
    {
        let host = FieldsHost::with_frame(mtm, host_frame);
        let view = field_of(mtm, FieldKind::Lines, Weak::new(), Some("Monaco"), 12.5);
        host.addSubview(&view);
        let text = Rect::new(20.0, 36.0, 200.0, 48.0);
        place_field(
            &view,
            &field(FieldKind::Lines, text, None, None),
            1.0,
            Some("Monaco"),
        );
        let window = window_over(mtm, &host);
        view.set_text("a\u{e9}😀\nb");
        if view.state(FieldKind::Lines).is_some() {
            problems.push(
                "lines: a field without the keyboard has a caret for the rule of keys".to_owned(),
            );
        }
        window.makeFirstResponder(Some(view.inner()));
        view.select((1, 3));
        match view.state(FieldKind::Lines) {
            Some(state) if state.length == 6 && state.selection == (1..4) => {}
            other => problems.push(format!(
                "lines: the state of a field with the keyboard is {other:?}, not 6 units long with 1..4 selected"
            )),
        }
        view.select((6, 0));
        // SAFETY: `doCommandBySelector:` is NSResponder's, which takes a selector.
        let _: () = unsafe { msg_send![view.inner(), doCommandBySelector: sel!(insertNewline:)] };
        if view.text() != "a\u{e9}😀\nb\n" {
            problems.push(format!(
                "lines: ⏎ the page did not take left {:?}, not a line break at the end",
                view.text()
            ));
        }
        window.makeFirstResponder(None);
        window.setContentView(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the page is given of a command of a field: the key, or `None` where the field keeps it,
    /// by the kit's rule with the field's kind, length and selection.
    fn page_gets(
        command: &str,
        event: Option<(Key, Mods)>,
        state: &FieldState,
    ) -> Option<(Key, Mods)> {
        let (key, mods) = field_command_key(command, event)?;
        (!state.takes(key, mods)).then_some((key, mods))
    }

    fn said_item(role: AccessibleRole, words: &str, parent: Option<usize>) -> Accessible {
        Accessible {
            role,
            id: Some(Id::named(words)),
            rect: bt_shell_common::native_view::Rect::new(0.0, 0.0, 10.0, 10.0),
            words: words.to_owned(),
            state: bt_shell_common::native_view::State::default(),
            pressable: false,
            parent,
            detail: Default::default(),
        }
    }

    /// The shape of the item at `at` of `items`, as the next outline would give it.
    fn shape_at(items: &[Accessible], at: usize) -> Shape {
        shape_of(&items[at], &ax::plan(items)[at])
    }

    #[test]
    fn a_row_that_opens_and_a_box_that_turns_keep_the_element_a_screen_reader_is_on() {
        // An outline's row closed, and open with a row under it: the same element (what it discloses is set again).
        let row = |open: bool| {
            let mut row = said_item(AccessibleRole::TreeItem, "src", Some(0));
            row.state.level = Some(1);
            row.state.expanded = Some(open);
            row
        };
        let mut child = said_item(AccessibleRole::TreeItem, "main.rs", Some(0));
        child.state.level = Some(2);
        let tree = said_item(AccessibleRole::Tree, "files", None);
        let closed = [tree.clone(), row(false)];
        let open = [tree, row(true), child];
        assert!(
            shape_at(&closed, 1) == shape_at(&open, 1),
            "a row that opens is the same element"
        );
        // A box that is mixed, then checked, then on its way to neither: the same element.
        let boxed = |checked: Option<bool>, mixed: bool| {
            let mut item = said_item(AccessibleRole::Checkbox, "all", None);
            item.state.checked = checked;
            item.state.mixed = mixed;
            [item]
        };
        assert!(shape_at(&boxed(None, true), 0) == shape_at(&boxed(Some(true), false), 0));
        assert!(shape_at(&boxed(Some(false), false), 0) == shape_at(&boxed(Some(true), false), 0));
        // An open or closed mark cannot be taken back once it is set, and a role cannot be changed: a different
        // element.
        let mut placed = boxed(Some(false), false);
        placed[0].state.expanded = Some(false);
        assert!(shape_at(&placed, 0) != shape_at(&boxed(Some(false), false), 0));
        let mut button = boxed(Some(false), false);
        button[0].role = AccessibleRole::Button;
        assert!(shape_at(&button, 0) != shape_at(&boxed(Some(false), false), 0));
    }

    fn state(kind: FieldKind, length: usize, selection: std::ops::Range<usize>) -> FieldState {
        FieldState {
            kind,
            length,
            selection,
        }
    }

    #[test]
    fn a_line_gives_the_page_the_keys_that_do_not_edit_it() {
        let none = Mods::default();
        let command = Mods {
            command: true,
            ..Mods::default()
        };
        let control = Mods {
            control: true,
            ..Mods::default()
        };
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };
        let line = state(FieldKind::Line, 4, 2..2);
        for (selector, key) in [
            ("insertNewline:", Key::Enter),
            ("cancelOperation:", Key::Escape),
            ("insertBacktab:", Key::Tab),
            ("moveUp:", Key::Up),
            ("moveDownAndModifySelection:", Key::Down),
            ("moveToBeginningOfDocument:", Key::Up),
            ("scrollPageDown:", Key::PageDown),
            ("pageUp:", Key::PageUp),
        ] {
            let (given, _) = page_gets(selector, Some((key, shift)), &line)
                .unwrap_or_else(|| panic!("{selector}"));
            assert_eq!(given, key, "{selector}");
        }
        assert!(page_gets("insertBacktab:", None, &line).is_some_and(|(_, mods)| mods.shift));
        // Keys the line has no binding for: a function key, ⌘K, ⌃1, ⌘C with nothing selected.
        for (key, mods) in [
            (Key::Function(1), none),
            (Key::Character('k'), command),
            (Key::Character('1'), control),
            (Key::Character('c'), command),
        ] {
            assert_eq!(
                page_gets("noop:", Some((key, mods)), &line),
                Some((key, mods)),
                "{key:?}"
            );
        }
        // What edits the line stays the line's where the caret is in the middle of it, and a key that
        // types is never the page's.
        for (selector, key) in [
            ("moveLeft:", Key::Left),
            ("deleteBackward:", Key::Backspace),
            ("moveToBeginningOfLine:", Key::Left),
            ("moveRight:", Key::Right),
            ("deleteForward:", Key::Delete),
        ] {
            assert_eq!(
                page_gets(selector, Some((key, none)), &line),
                None,
                "{selector}"
            );
        }
        assert_eq!(
            page_gets("noop:", Some((Key::Character('é'), none)), &line),
            None
        );
    }

    #[test]
    fn a_field_gives_the_page_the_keys_it_has_no_use_for_where_its_caret_is() {
        let none = Mods::default();
        let event = |key| Some((key, none));
        // ⌫ in an empty field, ← at the start, → at the end: the page's (a dialog's field gives ⌫
        // back to the page once it is empty; a list under a filter moves with the arrows).
        let empty = state(FieldKind::Line, 0, 0..0);
        assert!(page_gets("deleteBackward:", event(Key::Backspace), &empty).is_some());
        assert!(page_gets("moveLeft:", event(Key::Left), &empty).is_some());
        assert!(page_gets("moveRight:", event(Key::Right), &empty).is_some());
        let start = state(FieldKind::Line, 4, 0..0);
        assert!(page_gets("moveLeft:", event(Key::Left), &start).is_some());
        assert!(page_gets("deleteBackward:", event(Key::Backspace), &start).is_some());
        assert!(page_gets("moveRight:", event(Key::Right), &start).is_none());
        assert!(page_gets("deleteForward:", event(Key::Delete), &start).is_none());
        let end = state(FieldKind::Line, 4, 4..4);
        assert!(page_gets("moveRight:", event(Key::Right), &end).is_some());
        assert!(page_gets("deleteForward:", event(Key::Delete), &end).is_some());
        assert!(page_gets("deleteBackward:", event(Key::Backspace), &end).is_none());
        // What is selected is taken away or let go of: the field's, at either end.
        let all = state(FieldKind::Line, 4, 0..4);
        for (selector, key) in [
            ("deleteBackward:", Key::Backspace),
            ("moveLeft:", Key::Left),
            ("moveRight:", Key::Right),
            ("deleteForward:", Key::Delete),
        ] {
            assert!(
                page_gets(selector, event(key), &all).is_none(),
                "{selector}"
            );
        }
        // Lines keep their enter and their arrows inside the text, and give the page the ends.
        let lines = state(FieldKind::Lines, 9, 4..4);
        assert!(page_gets("insertNewline:", event(Key::Enter), &lines).is_none());
        assert!(page_gets("moveUp:", event(Key::Up), &lines).is_none());
        assert!(page_gets("moveUp:", event(Key::Up), &state(FieldKind::Lines, 9, 0..0)).is_some());
        assert!(
            page_gets(
                "moveDown:",
                event(Key::Down),
                &state(FieldKind::Lines, 9, 9..9)
            )
            .is_some()
        );
        assert!(page_gets("insertTab:", event(Key::Tab), &lines).is_some());
        let command = Mods {
            command: true,
            ..none
        };
        assert!(page_gets("insertNewline:", Some((Key::Enter, command)), &lines).is_some());
    }

    #[test]
    fn a_macs_own_chords_of_a_field_stay_the_fields() {
        // ⌃A, ⌃E, ⌃K and the rest are what AppKit's text binds to editing commands: the rule leaves
        // them to a host whose system gives them, and a ⌃ chord AppKit binds to nothing is the page's.
        let control = Mods {
            control: true,
            ..Mods::default()
        };
        let line = state(FieldKind::Line, 4, 4..4);
        for (selector, key) in [
            ("moveToEndOfLine:", 'e'),
            ("deleteToEndOfParagraph:", 'k'),
            ("moveToBeginningOfParagraph:", 'a'),
            ("moveRight:", 'f'),
        ] {
            assert_eq!(
                page_gets(selector, Some((Key::Character(key), control)), &line),
                None,
                "{selector}"
            );
        }
        assert_eq!(
            page_gets("noop:", Some((Key::Character('w'), control)), &line),
            Some((Key::Character('w'), control))
        );
        // A key with no key behind it (a dead key's first stroke) is the field's.
        assert_eq!(page_gets("noop:", None, &line), None);
    }

    #[test]
    fn the_keys_of_a_menus_entry_are_the_key_equivalent_of_a_menu_item() {
        let shortcut = |words: &str| Shortcut::parse(words).expect("a key");
        assert_eq!(
            key_equivalent(&shortcut("cmd+shift+k")),
            (
                "k".to_owned(),
                NSEventModifierFlags::Command | NSEventModifierFlags::Shift
            )
        );
        assert_eq!(
            key_equivalent(&shortcut("ctrl+alt+right")),
            (
                "\u{f703}".to_owned(),
                NSEventModifierFlags::Control | NSEventModifierFlags::Option
            )
        );
        // The key `key_of` makes of the character is the key the entry was written with.
        for words in [
            "enter",
            "escape",
            "tab",
            "backspace",
            "delete",
            "up",
            "page_down",
            "home",
            "f5",
            "d",
        ] {
            let shortcut = shortcut(words);
            let (characters, mask) = key_equivalent(&shortcut);
            assert_eq!(key_of(&characters), Some(shortcut.key), "{words}");
            assert!(mask.is_empty(), "{words}");
        }
        assert_eq!(key_equivalent(&shortcut("K")).0, "k");
    }

    /// A field the page's reply fills and gives the keyboard has its selection after AppKit made its
    /// own (it selects all of a field it gives the keyboard to), and a selection a fill left for a
    /// keyboard that never came is not put on the person's later click or Tab; the key rule is the
    /// kit's (`FieldState::takes`), asked of the pane with the field's own state. A source scan:
    /// the field is AppKit's and needs a window server.
    #[test]
    fn a_selection_a_fill_made_comes_after_the_keyboard_and_a_keys_rule_is_the_kits() {
        let source = include_str!("native_page.rs");
        let code = &source[..source.find("#[cfg(test)]").expect("the tests follow")];
        let body = |signature: &str| {
            let at = code.find(signature).expect(signature);
            &code[at..at + code[at..].find("\n    }\n").expect("its end")]
        };
        let keyboard = body("    pub(crate) fn send_keyboard(");
        assert!(
            keyboard
                .find("window.makeFirstResponder(Some(field.inner()))")
                .expect(keyboard)
                < keyboard.find("field.select_waiting()").expect(keyboard),
            "{keyboard}"
        );
        let fill = body("    pub(crate) fn fill(");
        assert!(fill.contains("field.select(range)"), "{fill}");
        assert!(fill.contains("field.set_text("), "{fill}");
        // The text is put in before it is selected in, and a text with none selected has its caret
        // at its end.
        assert!(fill.find("set_text").unwrap() < fill.find("field.select(range)").unwrap());
        assert!(fill.contains("text.encode_utf16().count(), 0"), "{fill}");
        // Every class of field answers the keyboard and a press with the same functions, which are
        // where the rule is: the keyboard comes, the page is told which field, and what the press
        // gives is known by its event.
        assert_eq!(
            code.matches("keyboard_taken(self.mtm(), self.ivars(), took)")
                .count(),
            2,
            "the text fields' class (once for both of its instances) and the text view of lines"
        );
        assert_eq!(
            code.matches("press_in(self, self.ivars(), event,").count(),
            2,
            "the text fields' class (once for both of its instances) and the text view of lines"
        );
        let free = |signature: &str| {
            let at = code.find(signature).expect(signature);
            &code[at..at + code[at..].find("\n}\n").expect("its end")]
        };
        let responder = free("fn keyboard_taken(");
        assert!(
            responder.contains("keyboard_came(ivars, took)"),
            "{responder}"
        );
        let came = body("fn keyboard_came(");
        assert!(came.contains("if !fields.moving.get()"), "{came}");
        assert!(came.contains("selection.set(None)"), "{came}");
        // A press that gives the field the keyboard ends with what the page selected meanwhile, unless
        // it was a drag; one in a field that had the keyboard is AppKit's.
        let press = free("fn press_in(");
        assert!(press.contains("editor().is_some()"), "{press}");
        assert!(press.contains("ivars.asked.set(None)"), "{press}");
        assert!(
            press
                .find("guarded(ivars, \"a press in a field\", (), press)")
                .expect(press)
                < press
                    .find("keep_asked_selection(view, ivars, down, editor())")
                    .expect(press),
            "{press}"
        );
        assert!(press.contains("if !had_the_keyboard {"), "{press}");
        // AppKit gives the field the keyboard as the button goes down, before `mouseDown:`: the press
        // that gave it is known by its event, not by the editor being there already.
        assert!(
            press.contains("given_by_press.take() == Some(event.timestamp())"),
            "{press}"
        );
        let kept = free("fn keep_asked_selection(");
        assert!(kept.contains("let dragged ="), "{kept}");
        assert!(kept.contains("if !dragged"), "{kept}");
        assert!(
            responder.contains("NSEventType::LeftMouseDown"),
            "{responder}"
        );
        assert!(
            responder.contains("given_by_press.set(press)"),
            "{responder}"
        );
        assert!(
            !code.contains("fn field_key("),
            "the kit's rule replaced it"
        );
        let pane = include_str!("pane.rs");
        let at = pane
            .find("    fn page_field_command(")
            .expect("page_field_command");
        let command = &pane[at..at + pane[at..].find("\n    }\n").expect("its end")];
        assert!(command.contains("state.takes(key, mods)"), "{command}");
        assert!(command.contains("field_command_key("), "{command}");
        assert!(command.contains("field.state(kind)"), "{command}");
    }

    /// A field of lines is no text field and has no delegate: what its delegate hears of a text
    /// field, the pane hears from the text view itself, by the same two entries, and the pane finds
    /// the field by its text view. A source scan: the pane is what `page_field_of` is asked of.
    #[test]
    fn a_field_of_lines_tells_the_pane_what_a_text_fields_delegate_hears() {
        let source = include_str!("native_page.rs");
        let code = &source[..source.find("#[cfg(test)]").expect("the tests follow")];
        let body = |signature: &str| {
            let at = code.find(signature).expect(signature);
            &code[at..at + code[at..].find("\n        }\n").expect("its end")]
        };
        let command = body("        fn do_command(&self, command: Sel) {");
        assert!(
            command.contains("page_field_do_command(control, self, command)"),
            "{command}"
        );
        assert!(
            command.contains("doCommandBySelector: command"),
            "{command}"
        );
        let changed = body("        fn did_change_text(&self) {");
        assert!(
            changed.contains("pane.page_field_changed(control)"),
            "{changed}"
        );
        let of = body("    pub(crate) fn of(&self, control: &AnyObject)");
        assert!(of.contains("is(field.view.inner())"), "{of}");
        let pane = include_str!("pane.rs");
        let at = pane
            .find("    pub(crate) fn page_field_do_command(")
            .expect("page_field_do_command");
        let route = &pane[at..at + pane[at..].find("\n    }\n").expect("its end")];
        assert!(
            route.contains("self.page_field_command(id, kind, text_view, command)"),
            "{route}"
        );
        // The delegate's own entry goes through the same one.
        assert!(pane.contains("self.page_field_changed(&object);"));
        // The writing cursor is the field's over the whole of its room.
        let at = pane
            .find("    fn cursor(&mut self, cursor: PageCursor) {")
            .expect("cursor");
        let cursor = &pane[at..at + pane[at..].find("\n    }\n").expect("its end")];
        assert!(
            cursor.contains("PageCursor::Arrow if self.0.pointer_in_a_field_room()"),
            "{cursor}"
        );
    }

    /// A field of one line takes no line break, as AppKit's single-line cells take a paste: each
    /// separator a space, one for one (so a selection counted in the page's text still fits it).
    #[test]
    fn a_line_holds_its_breaks_as_spaces_one_for_one() {
        assert_eq!(one_line("plain words"), None);
        assert_eq!(one_line("one\ntwo").as_deref(), Some("one two"));
        assert_eq!(one_line("a\r\nb").as_deref(), Some("a  b"));
        assert_eq!(one_line("a\u{2028}b\u{2029}c").as_deref(), Some("a b c"));
        assert_eq!(
            one_line("\nlead and tail\n").as_deref(),
            Some(" lead and tail ")
        );
        let text = "x\n😀\ny";
        let flat = one_line(text).expect("it has breaks");
        assert_eq!(flat.chars().count(), text.chars().count());
    }

    /// The text of a page's field is drawn where `text_top` puts its baseline only while its cell
    /// is not in AppKit's single-line mode (that mode's cell draws at a baseline set by the
    /// control's size: a point or more high in most of the terminal's faces, and lower once typed
    /// into); a field of one line takes no break from the person's editor nor from the page. A
    /// source scan: the field is AppKit's and needs a window server (the drawn text is measured in
    /// `tests/pencere.rs`).
    #[test]
    fn a_fields_cell_is_never_appkits_single_line_cell() {
        let source = include_str!("native_page.rs");
        let code = &source[..source.find("#[cfg(test)]").expect("the tests follow")];
        assert!(!code.contains("setUsesSingleLineMode(true)"));
        assert!(code.contains("cell.setUsesSingleLineMode(false);"));
        let body = |signature: &str| {
            let at = code.find(signature).expect(signature);
            &code[at..at + code[at..].find("\n    }\n").expect("its end")]
        };
        let changes = body("                fn should_change_text(");
        assert!(
            changes.contains("one_line_words(self.ivars(), words)"),
            "{changes}"
        );
        assert!(changes.contains("insertText: &*flat"), "{changes}");
        assert!(body("fn one_line_words(").contains("one_line("));
        for setter in ["    pub(crate) fn fill(", "    pub(crate) fn append("] {
            let body = body(setter);
            assert!(body.contains("field.held("), "{body}");
        }
    }

    /// A secret is an `NSSecureTextField` of its own class, not a plain field with a secure cell:
    /// AppKit gives the editor that hides what is typed only to that class, and threw (ending
    /// bateri) when a page sent the keyboard into a plain field with a secure cell. Both classes
    /// come from the one definition. A source scan: the field is AppKit's and needs a window server
    /// (the keyboard in a secret is tried in `tests/pencere.rs`).
    #[test]
    fn a_secret_is_appkits_secure_field_and_one_with_a_line() {
        let source = include_str!("native_page.rs");
        let code = &source[..source.find("#[cfg(test)]").expect("the tests follow")];
        assert!(!code.contains("NSSecureTextFieldCell"));
        assert!(!code.contains("setCell("));
        assert_eq!(
            code.matches("page_field_class!(\n").count(),
            2,
            "two classes, one definition"
        );
        let secret = &code[code
            .find("    PageSecretField,\n")
            .expect("the secret's class")..];
        assert!(
            secret.contains("    NSSecureTextField,\n    NSTextField,"),
            "{secret}"
        );
        let made = &code[code.find("fn text_field(").expect("text_field")..];
        assert!(made.contains("PageSecretField::alloc(mtm)"));
    }

    #[test]
    fn a_fills_selection_is_in_the_units_appkit_counts() {
        // Characters (code points, as the kit counts a selection) to UTF-16 units: 😀 is two.
        assert_eq!(utf16_range("abc", 0..3), (0, 3));
        assert_eq!(utf16_range("a😀b", 1..2), (1, 2));
        assert_eq!(utf16_range("a😀b", 2..3), (3, 1));
        assert_eq!(utf16_range("a😀b", 3..3), (4, 0));
        // `Fill::selection_in` holds it to the text first: a selection to the end, `0..usize::MAX`.
        let fill = Fill::text("a😀b").select_all();
        assert_eq!(
            fill.selection_in(3).map(|range| utf16_range("a😀b", range)),
            Some((0, 4))
        );
    }

    #[test]
    fn the_keys_of_the_page_come_from_the_characters_appkit_makes() {
        assert_eq!(key_of("\u{f701}"), Some(Key::Down));
        assert_eq!(key_of("\u{f700}"), Some(Key::Up));
        assert_eq!(key_of("\r"), Some(Key::Enter));
        assert_eq!(key_of("\u{1b}"), Some(Key::Escape));
        assert_eq!(key_of("\t"), Some(Key::Tab));
        assert_eq!(key_of(" "), Some(Key::Character(' ')));
        assert_eq!(key_of("G"), Some(Key::Character('G')));
        assert_eq!(key_of("/"), Some(Key::Character('/')));
        assert_eq!(key_of("\u{7f}"), Some(Key::Backspace));
        assert_eq!(key_of("\u{f728}"), Some(Key::Delete));
        assert_eq!(key_of("\u{f704}"), Some(Key::Function(1)));
        assert_eq!(key_of("\u{f717}"), Some(Key::Function(20)));
        // A function key past F20, a control character, an empty or composed text.
        assert_eq!(key_of("\u{f718}"), None);
        assert_eq!(key_of("\u{1}"), None);
        assert_eq!(key_of(""), None);
        assert_eq!(key_of("ab"), None);
    }

    #[test]
    fn a_page_and_its_editors_read_shift_tab_and_the_control_letters_as_their_keys() {
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };
        let control = Mods {
            control: true,
            ..Mods::default()
        };
        // ⇧⇥ is the tab with ⇧; ⌃Y, which makes the same character, is not.
        assert_eq!(event_key(Some("\u{19}"), Some("\t"), shift), Some(Key::Tab));
        assert_eq!(
            event_key(Some("\u{19}"), Some("y"), control),
            Some(Key::Character('y'))
        );
        // ⌃A and ⌃K are their letters; what ⌥ makes is itself.
        assert_eq!(
            event_key(Some("\u{1}"), Some("a"), control),
            Some(Key::Character('a'))
        );
        assert_eq!(
            event_key(Some("\u{b}"), Some("k"), control),
            Some(Key::Character('k'))
        );
        let alt = Mods {
            alt: true,
            ..Mods::default()
        };
        assert_eq!(
            event_key(Some("@"), Some("q"), alt),
            Some(Key::Character('@'))
        );
        // A dead key's first stroke makes nothing: the system's text has it.
        assert_eq!(event_key(Some(""), Some("e"), alt), None);
        assert_eq!(
            event_key(Some("\u{7f}"), Some("\u{7f}"), alt),
            Some(Key::Backspace)
        );
    }

    #[test]
    fn the_modifiers_are_the_four_the_page_reads() {
        let flags = NSEventModifierFlags::Shift | NSEventModifierFlags::Command;
        assert_eq!(
            mods_of(flags),
            Mods {
                shift: true,
                command: true,
                ..Mods::default()
            }
        );
        assert_eq!(mods_of(NSEventModifierFlags::empty()), Mods::default());
    }
}
