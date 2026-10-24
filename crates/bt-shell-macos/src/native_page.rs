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

use std::rc::Rc;

use bt_core::Theme;
use bt_shell_common::native_view::{Key, Mods, PageDrive, Rect, Tone, tone_linear};
use bt_shell_common::view_strip::Strip;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AllocAnyThread, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSColor, NSEventModifierFlags, NSFocusRingType, NSFont, NSTextField,
    NSTrackingArea, NSTrackingAreaOptions, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, ns_string};

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

/// Where the page modules are: `BT_VIEW_GUEST_DIR`, else the `kit/guest/dist` of the pencere
/// checkout beside this one (`make guest` there builds them; bateri carries none of its own). A
/// development convenience of the debug menu's demos.
#[cfg(debug_assertions)]
pub(crate) fn component_path(name: &str) -> std::path::PathBuf {
    bt_shell_common::native_view::guest_dir().join(format!("{name}.wasm"))
}

/// The height of the filter field, points: a line of the page's text, centred in the box.
const FIELD_HEIGHT: f64 = 18.0;

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

/// What is up while the container-list page is: the handle the pane's input goes to the page
/// by, the filter's field, the tracking area over the pane, whose page it is.
pub(crate) struct Page {
    pub owner: Owner,
    pub handle: Rc<dyn PageDrive>,
    pub field: Retained<NSTextField>,
    pub area: Retained<NSTrackingArea>,
}

/// A colour of the theme as AppKit's: the engine's tone in the theme, as sRGB.
fn ink(theme: &Theme, tone: Tone) -> Retained<NSColor> {
    let [r, g, b, _] = tone_linear(theme, tone).to_srgb().map(f64::from);
    NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0)
}

/// The filter's field for `pane`: no border and no ground (the page draws the box), the
/// terminal's face at the size of the page's text, the theme's ink. The pane is its delegate
/// (`TerminalPane::page_field_changed`, `page_field_command`).
pub(crate) fn filter_field(
    pane: &TerminalPane,
    theme: &Theme,
    family: Option<&str>,
) -> Retained<NSTextField> {
    let field = NSTextField::new(pane.mtm());
    field.setBordered(false);
    field.setBezeled(false);
    field.setDrawsBackground(false);
    field.setFocusRingType(NSFocusRingType::None);
    let size = 12.5;
    let font = family
        .and_then(|name| NSFont::fontWithName_size(&NSString::from_str(name), size))
        .unwrap_or_else(|| NSFont::monospacedSystemFontOfSize_weight(size, 0.0));
    field.setFont(Some(&font));
    field.setPlaceholderString(Some(ns_string!("Filter by name, image, port\u{2026}")));
    field.setTextColor(Some(&ink(theme, Tone::Foreground)));
    field.setAutoresizingMask(NSAutoresizingMaskOptions::empty());
    // SAFETY: the pane implements the text field's delegate methods it is given (`impl
    // NSTextFieldDelegate for TerminalPane`) and outlives the field, which it removes with the page.
    unsafe { field.setDelegate(Some(ProtocolObject::from_ref(pane))) };
    field
}

/// The theme changed: the field's ink with it.
pub(crate) fn paint(field: &NSTextField, theme: &Theme) {
    field.setTextColor(Some(&ink(theme, Tone::Foreground)));
}

/// Puts the field where the page's box for it is (`place`, the pane's points from its top,
/// `None`: the page has none now, so the field is hidden). The pane's own coordinates grow
/// upward, the page's downward.
pub(crate) fn place_field(field: &NSTextField, place: Option<Rect>) {
    let (Some(place), Some(pane)) = (place, unsafe { field.superview() }) else {
        field.setHidden(true);
        return;
    };
    let height = f64::from(place.h).min(FIELD_HEIGHT);
    let middle = f64::from(place.y + place.h / 2.0);
    let top = middle - height / 2.0;
    let y = pane.bounds().size.height - (top + height);
    field.setFrame(NSRect::new(
        NSPoint::new(f64::from(place.x), y),
        NSSize::new(f64::from(place.w), height),
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
        other if other.is_control() || ('\u{f700}'..='\u{f8ff}').contains(&other) => return None,
        other => Key::Char(other),
    })
}

/// The modifiers held, as the page reads them.
pub(crate) fn mods_of(flags: NSEventModifierFlags) -> Mods {
    Mods {
        shift: flags.contains(NSEventModifierFlags::Shift),
        control: flags.contains(NSEventModifierFlags::Control),
        option: flags.contains(NSEventModifierFlags::Option),
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
        assert_eq!(key_of(" "), Some(Key::Char(' ')));
        assert_eq!(key_of("G"), Some(Key::Char('G')));
        assert_eq!(key_of("/"), Some(Key::Char('/')));
        // A function key the page has no use for, a control character, an empty or composed text.
        assert_eq!(key_of("\u{f704}"), None);
        assert_eq!(key_of("\u{7f}"), None);
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
