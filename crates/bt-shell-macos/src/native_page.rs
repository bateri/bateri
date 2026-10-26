//! The application's strip over a page of the UI kit: the strip a program's view has
//! (`pencere_view::strip`), saying whose page it is and giving the one way out — its button
//! closes the page here, where over a program's view it writes ⌃C to the program.
//!
//! The page itself is `bt-shell-common`'s (`native_view`): the engine's draw list on the
//! renderer, painted in place of the grid by the pane's link. What the pane keeps for it is
//! the strip's place at the top and the room it takes (`view_strip::HEIGHT`), which the page
//! never draws into.
//!
//! What the pane adds to the page for it to be used: the filter's text field over the page's
//! box for it (AppKit's, in the terminal's face and the theme's ink — the text of the page is
//! the page's own, a field is not text to draw), the tracking area that tells the page where the
//! pointer is whoever has the keyboard, and the handle the pane's input goes to the page by
//! ([`bt_shell_common::native_view::ContainersHandle`]).
//!
//! A page is up for one of two owners ([`Owner`]): a debug build's demo of the kit (`Debug ▸
//! Show Container List Demo`), whose button and ⌃C close it, or the view a program opened — a
//! module the program sent — whose ⌃C and button go to the program, which ends it.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use bt_core::Theme;
use bt_shell_common::native_view::{
    Accessible, AccessibleRole, Field, FieldKind, Id, Key, Mods, Outline, PageDrive, Point, Role,
    tone_linear,
};
use bt_shell_common::view_strip::Strip;
use objc2::rc::{Retained, Weak as WeakId};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityButtonRole, NSAccessibilityElement,
    NSAccessibilityLayoutChangedNotification, NSAccessibilityPostNotification,
    NSAccessibilityStaticTextRole, NSAutoresizingMaskOptions, NSColor, NSControl,
    NSEventModifierFlags, NSFocusRingType, NSFont, NSLineBreakMode, NSResponder,
    NSSecureTextFieldCell, NSTextField, NSTrackingArea, NSTrackingAreaOptions, NSView,
};
use objc2_foundation::{NSObject, NSPoint, NSRect, NSSize, NSString};

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
    pub handle: Rc<dyn PageDrive>,
    pub fields: Rc<PageFields>,
    pub area: Retained<NSTrackingArea>,
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
    theme: Cell<Theme>,
    family: Option<String>,
    zoom: Cell<f32>,
    fields: RefCell<Vec<PageField>>,
    accessible: RefCell<Vec<Accessible>>,
    /// What a screen reader is given for each of `accessible`, in order: an element for a button
    /// or words, the field itself for a field.
    elements: RefCell<Vec<Retained<AnyObject>>>,
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
        Rc::new(Self {
            pane: WeakId::new(pane),
            theme: Cell::new(theme),
            family,
            zoom: Cell::new(zoom),
            fields: RefCell::new(Vec::new()),
            accessible: RefCell::new(Vec::new()),
            elements: RefCell::new(Vec::new()),
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
                    pane.add_page_field(&view);
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

    /// What a screen reader is given for `item`: the field itself for a field (it reads and edits
    /// it as AppKit's), an element of the pane for the rest, where it stands in the pane.
    fn element_of(&self, pane: &TerminalPane, item: &Accessible) -> Option<Retained<AnyObject>> {
        if let AccessibleRole::Field(_) = item.role {
            let field = self.field(item.id?)?;
            return Some(Retained::into_super(Retained::into_super(
                Retained::into_super(Retained::into_super(Retained::into_super(
                    Retained::into_super(field),
                ))),
            )));
        }
        let element = PageElement::alloc(pane.mtm()).set_ivars(PageElementIvars {
            press: Cell::new((item.role == AccessibleRole::Button).then(|| {
                let rect = item.rect;
                (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
            })),
            pane: WeakId::new(pane),
        });
        // SAFETY: `init` is NSObject's designated initializer and the ivars are set.
        let element: Retained<PageElement> = unsafe { msg_send![super(element), init] };
        let words = NSString::from_str(&item.words);
        // SAFETY: the roles are AppKit's own constants; the parent is the pane, a view that
        // outlives the element (the element goes with the page's outline).
        unsafe {
            element.setAccessibilityRole(Some(if item.role == AccessibleRole::Button {
                NSAccessibilityButtonRole
            } else {
                NSAccessibilityStaticTextRole
            }));
            element.setAccessibilityParent(Some(pane));
        }
        element.setAccessibilityLabel(Some(&words));
        if item.role == AccessibleRole::Text {
            // SAFETY: a string is a value static text has.
            unsafe { element.setAccessibilityValue(Some(&words)) };
        }
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

    /// What a screen reader is given of the page, in the order it reads it.
    pub(crate) fn elements(&self) -> Vec<Retained<AnyObject>> {
        self.elements.borrow().clone()
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
        self.accessible.take();
        self.elements.take();
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
                self.focused.set(to);
            }
            None => {
                window.makeFirstResponder(Some(pane.view()));
                self.focused.set(None);
            }
        }
        self.moving.set(false);
    }

    /// The texts a reply puts in fields, by their names; a name that is not a field is passed
    /// over. The page is not told of a text it put in itself (setting the value sends no change).
    pub(crate) fn fill(&self, texts: &[(Id, String)]) {
        for (id, text) in texts {
            if let Some(field) = self.field(*id) {
                field.setStringValue(&NSString::from_str(text));
            }
        }
    }
}

/// What a page's field keeps: its name, and the fields it belongs to (told when the person
/// clicks into it).
pub(crate) struct PageTextFieldIvars {
    id: Cell<Option<Id>>,
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
/// room. The text is as large as the page is drawn (`zoom`). The pane's own coordinates grow
/// upward, the page's downward.
fn place_field(field: &PageTextField, place: &Field, zoom: f32, family: Option<&str>) {
    let Some(pane) = (unsafe { field.superview() }) else {
        return;
    };
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
        let height = field.fittingSize().height.ceil().min(f64::from(room.h));
        let middle = f64::from(room.y + room.h / 2.0);
        ((middle - height / 2.0).round(), height)
    };
    let y = pane.bounds().size.height - (top + height);
    field.setFrame(NSRect::new(
        NSPoint::new(f64::from(room.x), y),
        NSSize::new(f64::from(room.w), height),
    ));
    field.setHidden(false);
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

/// The modifiers held, as the page reads them.
pub(crate) fn mods_of(flags: NSEventModifierFlags) -> Mods {
    Mods {
        shift: flags.contains(NSEventModifierFlags::Shift),
        control: flags.contains(NSEventModifierFlags::Control),
        alt: flags.contains(NSEventModifierFlags::Option),
        command: flags.contains(NSEventModifierFlags::Command),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
