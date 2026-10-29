//! The application's strip over a page of the UI kit: the strip a program's view has
//! (`pencere_view::strip`), saying whose page it is and giving the one way out — its button
//! closes the page here, where over a program's view it writes ⌃C to the program.
//!
//! The page itself is `bt-shell-common`'s (`native_view`): the engine's draw list on the
//! renderer, painted in place of the grid by the pane's link. What the pane keeps for it is
//! the strip's place at the top and the room it takes (`view_strip::HEIGHT`), which the page
//! never draws into.
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
use std::rc::{Rc, Weak};

use bt_core::Theme;
use bt_shell_common::native_view::{
    Accessible, AccessibleRole, Command, Field, FieldKind, FieldState, Fill, Id, Key, Mods,
    Outline, PageHandle, Point, Role, Shortcut, Tip, tone_linear,
};
use bt_shell_common::view_strip::Strip;
use objc2::rc::{Retained, Weak as WeakId};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityButtonRole, NSAccessibilityCheckBoxRole,
    NSAccessibilityElement, NSAccessibilityImageRole, NSAccessibilityLayoutChangedNotification,
    NSAccessibilityLinkRole, NSAccessibilityMenuItemRole, NSAccessibilityPostNotification,
    NSAccessibilityProgressIndicatorRole, NSAccessibilityRadioButtonRole, NSAccessibilityRole,
    NSAccessibilitySelectedTextChangedNotification, NSAccessibilitySliderRole,
    NSAccessibilityStaticTextRole, NSAccessibilitySubrole, NSAccessibilitySwitchSubrole,
    NSAccessibilityTabButtonSubrole, NSAccessibilityTextAreaRole,
    NSAccessibilityValueChangedNotification, NSAutoresizingMaskOptions, NSColor, NSControl,
    NSEvent, NSEventModifierFlags, NSFocusRingType, NSFont, NSLineBreakMode, NSResponder,
    NSSecureTextFieldCell, NSTextField, NSTrackingArea, NSTrackingAreaOptions, NSView, NSWindow,
};
use objc2_foundation::{
    NSInteger, NSNumber, NSObject, NSObjectProtocol, NSPoint, NSRange, NSRect, NSSize, NSString,
};

use crate::pane::TerminalPane;
use crate::pencere_view::face::Faces;
use crate::pencere_view::strip::StripView;
use crate::pencere_view::strip_area;

/// The strip over the view a program opened: the words of `text` (`view_strip::strip`, the
/// same as over a web view) and the one button, which sends ⌃C to the program.
pub(crate) fn program_strip(
    pane: &TerminalPane,
    theme: &Theme,
    text: &Strip,
) -> Retained<StripView> {
    let strip = StripView::new(
        pane.mtm(),
        strip_area(pane.bounds()),
        text,
        Rc::new(Faces::new(&[])),
        theme,
    );
    strip.aim(pane, sel!(quitProgram:));
    strip
}

/// The demo's strip over `pane`: the made-up program's name and the title of its page, a button
/// that closes the page, painted with `theme`.
#[cfg(debug_assertions)]
pub(crate) fn strip(
    pane: &TerminalPane,
    theme: &Theme,
    program: &str,
    title: &str,
) -> Retained<StripView> {
    let text = Strip {
        place: None,
        program: Some(program.to_owned()),
        title: Some(title.to_owned()),
        quit_tip: "Closes the demo page. The terminal comes back.".to_owned(),
    };
    let strip = StripView::new(
        pane.mtm(),
        strip_area(pane.bounds()),
        &text,
        Rc::new(Faces::new(&[])),
        theme,
    );
    strip.aim(pane, sel!(closeUiPage:));
    strip
}

/// Where the page modules are: `BT_VIEW_GUEST_DIR`, else `target/guest/dist`, where `make guest`
/// builds them from the pencere checkout the build is pinned to (bateri carries none of its
/// own). A development convenience of the debug menu's demos.
#[cfg(debug_assertions)]
pub(crate) fn component_path(name: &str) -> std::path::PathBuf {
    bt_shell_common::native_view::guest_dir().join(format!("{name}.wasm"))
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
    pub view: Retained<PageTextField>,
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
    /// What a screen reader is given for each of `accessible`, in order: an element for a button
    /// or words, the field itself for a field.
    elements: RefCell<Vec<Retained<AnyObject>>>,
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
            self.accessible.replace(outline.accessible.clone());
            let elements = outline
                .accessible
                .iter()
                .filter_map(|item| self.element_of(&pane, item))
                .collect();
            self.elements.replace(elements);
            // SAFETY: the pane is an element of the accessibility tree (a view), and the name is
            // one of AppKit's own.
            unsafe {
                NSAccessibilityPostNotification(&pane, NSAccessibilityLayoutChangedNotification)
            };
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
    fn element_of(&self, pane: &TerminalPane, item: &Accessible) -> Option<Retained<AnyObject>> {
        if item.role == AccessibleRole::Editor {
            return editor_element(pane, item);
        }
        if let AccessibleRole::Field(_) = item.role {
            let field = self.field(item.id?)?;
            return Some(Retained::into_super(Retained::into_super(
                Retained::into_super(Retained::into_super(Retained::into_super(
                    Retained::into_super(field),
                ))),
            )));
        }
        let state = &item.state;
        let element = PageElement::alloc(pane.mtm()).set_ivars(PageElementIvars {
            press: Cell::new((pressed_by_role(item.role) && !state.disabled).then(|| {
                let rect = item.rect;
                (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
            })),
            pane: WeakId::new(pane),
        });
        // SAFETY: `init` is NSObject's designated initializer and the ivars are set.
        let element: Retained<PageElement> = unsafe { msg_send![super(element), init] };
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
        // is checked (0 or 1), a string for words and what a progress or a slider shows.
        unsafe {
            if let Some(checked) = state.checked {
                element.setAccessibilityValue(Some(&NSNumber::new_i32(i32::from(checked))));
            } else if let Some(value) = &state.value {
                element.setAccessibilityValue(Some(&NSString::from_str(value)));
            } else if matches!(item.role, AccessibleRole::Text | AccessibleRole::Heading(_)) {
                element.setAccessibilityValue(Some(&words));
            }
        }
        if state.selected {
            element.setAccessibilitySelected(true);
        }
        if let Some(open) = state.expanded {
            element.setAccessibilityExpanded(open);
        }
        element.setAccessibilityEnabled(!state.disabled);
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

    /// What the page can be told to do now, as its outline last said: the nearest the keyboard
    /// first (Page Commands ▸, and the palette that lists the menu).
    pub(crate) fn commands(&self) -> Vec<Command> {
        self.outline
            .borrow()
            .as_ref()
            .map(|outline| outline.commands.clone())
            .unwrap_or_default()
    }

    /// What a screen reader is given of the page, in the order it reads it.
    pub(crate) fn elements(&self) -> Vec<Retained<AnyObject>> {
        self.elements.borrow().clone()
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
    pub(crate) fn field(&self, id: Id) -> Option<Retained<PageTextField>> {
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
    /// the selection waits for the keyboard the page sends it (`send_keyboard`).
    pub(crate) fn fill(&self, id: Id, fill: &Fill) {
        let Some(field) = self.field(id) else {
            return;
        };
        let text = match &fill.text {
            Some(text) => {
                field.setStringValue(&NSString::from_str(text));
                text.clone()
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

impl PageTextField {
    /// Selects `(location, length)` (UTF-16 units) in the field: at once where the keyboard is in
    /// it, else when the page sends it the keyboard ([`PageTextField::select_waiting`]).
    fn select(&self, range: (usize, usize)) {
        match self.currentEditor() {
            Some(editor) => editor.setSelectedRange(NSRange::new(range.0, range.1)),
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
    fields: RefCell<Weak<PageFields>>,
}

define_class!(
    // SAFETY: NSTextField is designed for subclassing; PageTextField implements no `Drop` and is
    // born with `initWithFrame:`.
    #[unsafe(super(NSTextField, NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPageTextField"]
    #[ivars = PageTextFieldIvars]
    pub(crate) struct PageTextField;

    impl PageTextField {
        /// The keyboard comes to the field (the person clicked into it, or the page sent it):
        /// the page's fields are told which.
        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            // SAFETY: the superclass's own `becomeFirstResponder`, which takes nothing and
            // answers whether the field took the keyboard.
            let took: bool = unsafe { msg_send![super(self), becomeFirstResponder] };
            if took
                && let (Some(id), Some(fields)) = (self.ivars().id.get(), self.ivars().fields.borrow().upgrade())
            {
                // What a fill selected waits for the page's own move of the keyboard
                // (`PageFields::send_keyboard`); the person's click or Tab makes its own.
                if !fields.moving.get() {
                    self.ivars().selection.set(None);
                }
                fields.focused_by_person(id);
            }
            took
        }
    }
);

/// What an element a screen reader is given keeps: where a press on it lands (a button's middle,
/// in the pane's points from its top), and the pane it presses.
pub(crate) struct PageElementIvars {
    press: Cell<Option<(f32, f32)>>,
    pane: WeakId<TerminalPane>,
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
            AccessibleRole::Text | AccessibleRole::Heading(_) | AccessibleRole::Field(_) => {
                (NSAccessibilityStaticTextRole, None)
            }
        }
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
) -> Retained<PageTextField> {
    let mtm = pane.mtm();
    let this = PageTextField::alloc(mtm).set_ivars(PageTextFieldIvars {
        id: Cell::new(None),
        selection: Cell::new(None),
        fields: RefCell::new(Rc::downgrade(fields)),
    });
    // SAFETY: `initWithFrame:` is NSTextField's designated initializer and the ivars are set.
    let field: Retained<PageTextField> =
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
    if kind == FieldKind::Secret {
        // A secret's cell draws dots for the characters, and its field editor copies nothing.
        let cell = NSSecureTextFieldCell::new(mtm);
        field.setCell(Some(&cell));
    }
    field.setBordered(false);
    field.setBezeled(false);
    field.setDrawsBackground(false);
    field.setEditable(true);
    field.setSelectable(true);
    field.setFocusRingType(NSFocusRingType::None);
    field.setFont(Some(&face(family, FIELD_TEXT)));
    if let Some(cell) = field.cell() {
        match kind {
            // One line: a hint that is too long for the room is cut, not wrapped onto a line
            // below it that the room does not show (which leaves the first line high in the box).
            FieldKind::Line | FieldKind::Secret => {
                cell.setUsesSingleLineMode(true);
                cell.setScrollable(true);
            }
            FieldKind::Lines => {
                cell.setUsesSingleLineMode(false);
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
    field
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
fn place_field(field: &PageTextField, place: &Field, zoom: f32, family: Option<&str>) {
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
        ((middle - height / 2.0).round(), height)
    };
    // The host's y is the page's, from the top.
    let frame = NSRect::new(
        NSPoint::new(f64::from(room.x), top),
        NSSize::new(f64::from(room.w), height),
    );
    if field.frame() != frame {
        field.setFrame(frame);
    }
    field.setHidden(false);
}

/// The height of one line of `field`, as its cell draws it: its font's line and the cell's
/// margins. Asked of the cell, not of Auto Layout (`fittingSize`), which answers for a field out
/// of its window with a height of nothing: a pane moved into a split (⌘D) is out of its window for
/// a moment, and a field placed then stood half a line low until the page's outline changed.
fn line_height(field: &PageTextField) -> f64 {
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
        let responder = body("        fn become_first_responder(&self) -> bool {");
        assert!(responder.contains("if !fields.moving.get()"), "{responder}");
        assert!(responder.contains("selection.set(None)"), "{responder}");
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
