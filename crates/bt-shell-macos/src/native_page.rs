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
//! page's own, a field is not text to draw), the tracking area that tells the page where the
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
use std::rc::{Rc, Weak};
use std::sync::Arc;

use block2::RcBlock;
use bt_core::Theme;
use bt_shell_common::native_view::ax::{self, AxNode};
use bt_shell_common::native_view::{
    Accessible, AccessibleRole, Command, Field, FieldKind, FieldState, Fill, Id, Key, Mods,
    Outline, PageHandle, Point, Role, Shortcut, Sort, Tip, ViewFiles, tone_linear,
};
use objc2::rc::{Retained, Weak as WeakId};
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityAnnouncementKey,
    NSAccessibilityAnnouncementRequestedNotification, NSAccessibilityButtonRole,
    NSAccessibilityCellRole, NSAccessibilityCheckBoxRole, NSAccessibilityCustomAction,
    NSAccessibilityDialogSubrole, NSAccessibilityElement, NSAccessibilityGroupRole,
    NSAccessibilityImageRole, NSAccessibilityLayoutChangedNotification, NSAccessibilityLinkRole,
    NSAccessibilityListRole, NSAccessibilityMenuItemRole, NSAccessibilityOutlineRole,
    NSAccessibilityPostNotification, NSAccessibilityPostNotificationWithUserInfo,
    NSAccessibilityPriorityKey, NSAccessibilityPriorityLevel, NSAccessibilityProgressIndicatorRole,
    NSAccessibilityRadioButtonRole, NSAccessibilityRole, NSAccessibilityRowRole,
    NSAccessibilitySelectedTextChangedNotification, NSAccessibilitySliderRole,
    NSAccessibilitySortDirection, NSAccessibilityStaticTextRole, NSAccessibilitySubrole,
    NSAccessibilitySwitchSubrole, NSAccessibilityTabButtonSubrole, NSAccessibilityTabGroupRole,
    NSAccessibilityTableRole, NSAccessibilityTextAreaRole, NSAccessibilityValueChangedNotification,
    NSApplication, NSAutoresizingMaskOptions, NSColor, NSControl, NSEvent, NSEventModifierFlags,
    NSEventType, NSFocusRingType, NSFont, NSLineBreakMode, NSResponder, NSSecureTextField,
    NSTextField, NSTrackingArea, NSTrackingAreaOptions, NSView, NSWindow,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSInteger, NSNumber, NSObject, NSObjectProtocol, NSPoint, NSRange,
    NSRect, NSSize, NSString,
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

        /// Only the fields take a press; everywhere else the page under the host is hit.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            // SAFETY: reading the superview; we are on the main thread.
            let local = match unsafe { self.superview() } {
                Some(parent) => self.convertPoint_fromView(point, Some(&parent)),
                None => point,
            };
            let fields: Vec<_> = self.subviews().iter().collect();
            fields.into_iter().rev().find_map(|field| {
                let frame = field.frame();
                let inside = local.x >= frame.origin.x
                    && local.y >= frame.origin.y
                    && local.x < frame.origin.x + frame.size.width
                    && local.y < frame.origin.y + frame.size.height;
                if inside && !field.isHidden() {
                    field.hitTest(local)
                } else {
                    None
                }
            })
        }
    }
);

impl FieldsHost {
    fn new(pane: &TerminalPane) -> Retained<Self> {
        let this = Self::alloc(pane.mtm()).set_ivars(());
        // SAFETY: `initWithFrame:` is NSView's designated initializer.
        let host: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: pane.bounds()] };
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
    ) -> Rc<Self> {
        let host = FieldsHost::new(pane);
        pane.add_page_fields_host(&host);
        Rc::new(Self {
            pane: WeakId::new(pane),
            host,
            theme: Cell::new(theme),
            family,
            zoom: Cell::new(zoom),
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
                if field.view.currentEditor().is_some()
                    && let Some(window) = pane.window()
                {
                    window.makeFirstResponder(Some(pane.view()));
                }
                field.view.removeFromSuperview();
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
            field.view.setAccessibilityElement(read.contains(&field.id));
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
        let height = pane.bounds().size.height;
        let tags = self
            .tips
            .borrow()
            .iter()
            .map(|tip| {
                // The page's rooms are from the pane's top, the pane's frames from its bottom.
                let rect = tip.rect;
                let in_pane = NSRect::new(
                    NSPoint::new(f64::from(rect.x), height - f64::from(rect.y + rect.h)),
                    NSSize::new(f64::from(rect.w), f64::from(rect.h)),
                );
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
            // The field itself, whichever class it is: NSTextField, then up to AnyObject.
            let field: Retained<NSTextField> = self.field(item.id?)?.retain();
            return Some(Made::Other(Retained::into_super(Retained::into_super(
                Retained::into_super(Retained::into_super(Retained::into_super(field))),
            ))));
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
    /// views of all its fields: what the pane's own answer is cleared of, so that a field a modal in
    /// front shuts out is not given back as the pane's.
    pub(crate) fn owned(&self) -> Vec<Retained<AnyObject>> {
        let mut all = self.owned.borrow().clone();
        all.extend(self.fields.borrow().iter().map(|field| {
            let view: Retained<NSTextField> = field.view.retain();
            let object: Retained<AnyObject> = Retained::into_super(Retained::into_super(
                Retained::into_super(Retained::into_super(Retained::into_super(view))),
            ));
            object
        }));
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
                std::ptr::eq(
                    std::ptr::from_ref(control).cast::<()>(),
                    std::ptr::from_ref(&*field.view).cast::<()>(),
                )
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
        Some(field.view.stringValue().to_string())
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
        let now = field.held(&format!("{}{text}", field.stringValue()));
        field.setStringValue(&NSString::from_str(&now));
        Some(now)
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
            .any(|field| field.view.currentEditor().is_some())
    }

    /// Every field is taken away (the page goes down).
    pub(crate) fn clear(&self) {
        for field in self.fields.take() {
            field.view.removeFromSuperview();
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
            field.view.setTextColor(Some(&ink(theme, Role::Foreground)));
        }
    }

    /// The page is drawn `zoom` times its own size now: the fields' text with it (they are put
    /// where the next outline says).
    pub(crate) fn set_zoom(&self, zoom: f32) {
        self.zoom.set(zoom);
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
                window.makeFirstResponder(Some(&field));
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
                field.setStringValue(&NSString::from_str(&text));
                text
            }
            None => field.stringValue().to_string(),
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
/// ended bateri when a page sent the keyboard into a password field. To the page and the pane the
/// two are one field ([`page_field_class`]): what they keep and do is the same, only AppKit's class
/// differs.
#[derive(Clone)]
pub(crate) enum FieldView {
    Text(Retained<PageTextField>),
    Secret(Retained<PageSecretField>),
}

impl std::ops::Deref for FieldView {
    type Target = NSTextField;

    fn deref(&self) -> &NSTextField {
        match self {
            Self::Text(field) => field,
            Self::Secret(field) => field,
        }
    }
}

impl FieldView {
    /// What the field keeps, whichever class it is.
    fn ivars(&self) -> &PageTextFieldIvars {
        match self {
            Self::Text(field) => field.ivars(),
            Self::Secret(field) => field.ivars(),
        }
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
    /// ([`FieldView::select_waiting`]).
    fn select(&self, range: (usize, usize)) {
        match self.currentEditor() {
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
        if let (Some(range), Some(editor)) = (self.ivars().selection.take(), self.currentEditor()) {
            editor.setSelectedRange(NSRange::new(range.0, range.1));
        }
    }

    /// Where the keyboard is in the field, as the kit's rule of keys asks it ([`FieldState`]), in
    /// UTF-16 units; `None` where the keyboard is not in it.
    pub(crate) fn state(&self, kind: FieldKind) -> Option<FieldState> {
        let editor = self.currentEditor()?;
        let range = editor.selectedRange();
        Some(FieldState {
            kind,
            length: editor.string().length(),
            selection: range.location..range.location + range.length,
        })
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
fn keep_asked_selection(field: &NSTextField, ivars: &PageTextFieldIvars, down: NSPoint) {
    let Some(range) = ivars.asked.take() else {
        return;
    };
    let (Some(window), Some(editor)) = (field.window(), field.currentEditor()) else {
        return;
    };
    let now = window.mouseLocationOutsideOfEventStream();
    let dragged = (now.x - down.x).abs() > 4.0 || (now.y - down.y).abs() > 4.0;
    if !dragged && range.0 + range.1 <= editor.string().length() {
        editor.setSelectedRange(NSRange::new(range.0, range.1));
    }
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
                    // SAFETY: the superclass's own `becomeFirstResponder`, which takes nothing and
                    // answers whether the field took the keyboard.
                    let took: bool = unsafe { msg_send![super(self), becomeFirstResponder] };
                    if took {
                        // Taken by a press of the main button: that press's own end must not undo
                        // what the page selects in answer to the keyboard coming (`mouse_down`).
                        let press = NSApplication::sharedApplication(self.mtm())
                            .currentEvent()
                            .filter(|event| event.r#type() == NSEventType::LeftMouseDown)
                            .map(|event| event.timestamp());
                        self.ivars().given_by_press.set(press);
                        self.ivars().asked.set(None);
                    }
                    keyboard_came(self.ivars(), took);
                    took
                }

                /// A press in the field. Where it gives the field the keyboard, the page is told
                /// (see `becomeFirstResponder`) and answers with the whole address filled in and
                /// selected, which arrives before the button is up; the press's own tracking would
                /// then end by selecting from where it began, so what the page selected is put back
                /// when it is over.
                #[unsafe(method(mouseDown:))]
                fn mouse_down(&self, event: &NSEvent) {
                    let gave_the_keyboard =
                        self.ivars().given_by_press.take() == Some(event.timestamp());
                    let had_the_keyboard = self.currentEditor().is_some() && !gave_the_keyboard;
                    let down = event.locationInWindow();
                    if had_the_keyboard {
                        self.ivars().asked.set(None);
                    }
                    // SAFETY: the superclass's own `mouseDown:`, which takes the event and tracks
                    // the press until the button is up.
                    let _: () = unsafe { msg_send![super(self), mouseDown: event] };
                    if !had_the_keyboard {
                        keep_asked_selection(self, self.ivars(), down);
                    }
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
            // A dialog is a group that says it is one; an alert dialog says so in its words
            // (`ax::plan` gives them as its role description).
            AccessibleRole::Dialog | AccessibleRole::AlertDialog => {
                (NSAccessibilityGroupRole, Some(NSAccessibilityDialogSubrole))
            }
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
    let rect = item.rect;
    let height = pane.bounds().size.height;
    NSRect::new(
        NSPoint::new(f64::from(rect.x), height - f64::from(rect.y + rect.h)),
        NSSize::new(f64::from(rect.w), f64::from(rect.h)),
    )
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
                // The cells by their row's place among the rows and their own in the row.
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
                        let place = nodes[*child].index.unwrap_or(column);
                        cells.push((row_at as i64, place, cell.object()));
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

/// The size of a field's text at the page's own size, points: the kit's text.
const FIELD_TEXT: f64 = 12.5;

/// A field of `kind` for `pane`: no border and no ground (the page draws the box), the
/// terminal's face, the theme's ink. One line scrolls as it is typed and cuts a hint that does
/// not fit with "…"; lines wrap; a secret shows no characters. The pane is its delegate
/// (`TerminalPane::page_field_changed`, `page_field_command`).
fn text_field(
    pane: &TerminalPane,
    kind: FieldKind,
    theme: &Theme,
    family: Option<&str>,
    fields: &Rc<PageFields>,
) -> FieldView {
    let mtm = pane.mtm();
    let ivars = PageTextFieldIvars {
        id: Cell::new(None),
        selection: Cell::new(None),
        asked: Cell::new(None),
        given_by_press: Cell::new(None),
        fields: RefCell::new(Rc::downgrade(fields)),
        one_line: Cell::new(kind != FieldKind::Lines),
    };
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
    let field: &NSTextField = &view;
    field.setBordered(false);
    field.setBezeled(false);
    field.setDrawsBackground(false);
    field.setEditable(true);
    field.setSelectable(true);
    field.setFocusRingType(NSFocusRingType::None);
    field.setFont(Some(&face(family, FIELD_TEXT)));
    if let Some(cell) = field.cell() {
        // Not AppKit's single-line mode, for any kind: its cell draws the text at a baseline set
        // by the control's size whatever the face, and in most of the terminal's faces that stood
        // a point or more above the room's middle (four at 17 points), and moved down once the
        // field was typed into, where the editor draws it at the face's own baseline, the one
        // `text_top` centres. The one other thing that mode does, turning a pasted line break
        // into a space, the field does itself (`one_line`).
        cell.setUsesSingleLineMode(false);
        match kind {
            // One line: it scrolls as it is typed, and a hint that is too long for the room is
            // cut with "…" (the break mode below), not wrapped onto a line below it that the room
            // does not show (which left the first line high in the box).
            FieldKind::Line | FieldKind::Secret => {
                cell.setWraps(false);
                cell.setScrollable(true);
            }
            FieldKind::Lines => {
                cell.setWraps(true);
                cell.setScrollable(false);
            }
        }
    }
    field.setLineBreakMode(if kind == FieldKind::Lines {
        NSLineBreakMode::ByWordWrapping
    } else {
        NSLineBreakMode::ByTruncatingTail
    });
    field.setTextColor(Some(&ink(theme, Role::Foreground)));
    // Pinned to the pane's top, where the page's rooms are measured from: the pane's own
    // coordinates grow upward, so a field with a fixed bottom margin would slide with the
    // pane's bottom edge while it is resized, until the next outline put it back.
    // The host is on the page's axis and covers the pane: a field keeps the frame its room gives
    // it, and AppKit moves nothing.
    field.setAutoresizingMask(NSAutoresizingMaskOptions::empty());
    // SAFETY: the pane implements the text field's delegate methods it is given (`impl
    // NSTextFieldDelegate for TerminalPane`) and outlives the field, which it removes with the page.
    unsafe { field.setDelegate(Some(ProtocolObject::from_ref(pane))) };
    view
}

/// The terminal's face (the system's monospaced one where it has none) at `size` points.
fn face(family: Option<&str>, size: f64) -> Retained<NSFont> {
    family
        .and_then(|name| NSFont::fontWithName_size(&NSString::from_str(name), size))
        .unwrap_or_else(|| NSFont::monospacedSystemFontOfSize_weight(size, 0.0))
}

/// Puts the field where the page's room for it is (the pane's points from its top), saying the
/// page's hint while it is empty, and what it is to a screen reader. A line is as tall as its
/// own line (its font's, which AppKit draws from the top of the frame), so it is that line that
/// is centred in the room, not a taller frame with the text at its top; lines take the whole
/// room. The text is as large as the page is drawn (`zoom`). The field stands in the host
/// ([`FieldsHost`]), whose axis is the page's: its frame is its room, from the top.
fn place_field(field: &NSTextField, place: &Field, zoom: f32, family: Option<&str>) {
    // SAFETY: reading the superview; we are on the main thread.
    if unsafe { field.superview() }.is_none() {
        return;
    }
    let size = FIELD_TEXT * f64::from(zoom);
    if field
        .font()
        .is_none_or(|font| (font.pointSize() - size).abs() > 0.01)
    {
        field.setFont(Some(&face(family, size)));
    }
    let hint = place.hint.as_deref().map(NSString::from_str);
    field.setPlaceholderString(hint.as_deref());
    let said = place
        .label
        .as_deref()
        .or(place.hint.as_deref())
        .unwrap_or_default();
    field.setAccessibilityLabel(Some(&NSString::from_str(said)));
    let room = place.rect;
    let (top, height) = if place.kind == FieldKind::Lines {
        (f64::from(room.y), f64::from(room.h))
    } else {
        let height = line_height(field).min(f64::from(room.h));
        let middle = f64::from(room.y + room.h / 2.0);
        (text_top(field, middle, height), height)
    };
    // The host's y is the page's, from the top.
    let frame = NSRect::new(
        NSPoint::new(f64::from(room.x), top),
        NSSize::new(f64::from(room.w), height),
    );
    if field.frame() != frame {
        field.setFrame(frame);
    }
    // AppKit draws the field above the page: one the page's popover or dialog lies over is not
    // shown, or its text would stand over what covers it.
    field.setHidden(place.covered);
}

/// Where the frame of a one-line `field` stands so that its capitals are centred on the room's
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
                .find("window.makeFirstResponder(Some(&field))")
                .expect(keyboard)
                < keyboard.find("field.select_waiting()").expect(keyboard),
            "{keyboard}"
        );
        let fill = body("    pub(crate) fn fill(");
        assert!(fill.contains("field.select(range)"), "{fill}");
        assert!(fill.contains("field.setStringValue("), "{fill}");
        // The text is put in before it is selected in, and a text with none selected has its caret
        // at its end.
        assert!(fill.find("setStringValue").unwrap() < fill.find("field.select(range)").unwrap());
        assert!(fill.contains("text.encode_utf16().count(), 0"), "{fill}");
        let responder = body("                fn become_first_responder(&self) -> bool {");
        assert!(
            responder.contains("keyboard_came(self.ivars(), took)"),
            "{responder}"
        );
        let came = body("fn keyboard_came(");
        assert!(came.contains("if !fields.moving.get()"), "{came}");
        assert!(came.contains("selection.set(None)"), "{came}");
        // A press that gives the field the keyboard ends with what the page selected meanwhile, unless
        // it was a drag; one in a field that had the keyboard is AppKit's.
        let press = body("                fn mouse_down(&self, event: &NSEvent) {");
        assert!(press.contains("self.currentEditor().is_some()"), "{press}");
        assert!(press.contains("self.ivars().asked.set(None)"), "{press}");
        assert!(
            press.find("mouseDown: event").expect(press)
                < press
                    .find("keep_asked_selection(self, self.ivars(), down)")
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
        let kept = body("fn keep_asked_selection(");
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
