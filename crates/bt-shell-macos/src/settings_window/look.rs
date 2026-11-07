//! The settings window's colours: the roles of the theme the terminal shows.
//!
//! The window is drawn in the terminal's own language, not the system's: the sidebar stands on
//! the band (what the tab bar stands on), a page on the ground (what the cards stand on), a group
//! is a card (the theme's background, framed by its separator), text is the foreground and what
//! says less is dim, a choice is marked in the accent, and the file's warnings are the theme's
//! warning and error. The window's appearance is the theme's light or dark, so what AppKit draws
//! itself (a pop-up, a field, a stepper) stands right on it.
//!
//! **No view keeps a colour, but a button's title.** Each colour here is dynamic: it reads the
//! window's palette when it is drawn ([`ink`]), so a theme that changes is the palette set
//! ([`set`]) and the window drawn again ([`redraw`]); nothing has to find the labels, boxes and
//! controls it coloured. A button is the exception: it reads its title's colour when it is first
//! drawn on the screen and keeps it — drawing it again does not read it again, nor does a title
//! given the same colour object — so a window shown before its first palette kept the default
//! theme's light text on a light button. A button's title is given the palette's colour as it is
//! ([`fixed`]) and given it again when the palette changes (`ink_button`, `reink_buttons`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::Mutex;

use block2::RcBlock;
use bt_core::Theme;
use objc2::rc::Retained;
use objc2_app_kit::{NSAppearance, NSColor, NSView};

use super::pictures::Palette;
use crate::tab_bar::Tint;

/// The palette the window's colours read; `None` until the first refresh.
static PALETTE: Mutex<Option<Palette>> = Mutex::new(None);

/// A role the window draws in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Ink {
    /// Text: the theme's foreground.
    Text,
    /// What says less than the text beside it: the dim role.
    Dim,
    /// A choice, the open page, a value's fill.
    Accent,
    /// What stands on the accent: the theme's background or foreground, whichever reads better.
    OnAccent,
    /// A page's ground: what the terminal's cards stand on.
    Ground,
    /// The sidebar: what the tab bar stands on.
    Band,
    /// A group, a card: the theme's background.
    Card,
    /// A group's frame and the rule between two rows.
    Line,
    /// Inside a field the user types in: two steps over the card, as a tab chip stands over the
    /// content.
    Field,
    Warning,
    Error,
}

impl Ink {
    fn rgb(self, palette: &Palette) -> u32 {
        match self {
            Ink::Text => palette.foreground,
            Ink::Dim => palette.dim,
            Ink::Accent => palette.accent,
            Ink::OnAccent => on_accent(palette),
            Ink::Ground => palette.ground,
            Ink::Band => palette.band,
            Ink::Card => palette.background,
            Ink::Line => palette.separator,
            Ink::Field => palette.raised,
            Ink::Warning => palette.warning,
            Ink::Error => palette.error,
        }
    }
}

/// Of the theme's background and foreground, the one that stands out more on the accent: a mark
/// on the open page's tile.
pub(super) fn on_accent(palette: &Palette) -> u32 {
    let against = |rgb| bt_core::contrast_ratio(rgb, palette.accent);
    if against(palette.background) >= against(palette.foreground) {
        palette.background
    } else {
        palette.foreground
    }
}

/// The palette the colours read now: the last one set, else the default theme's.
pub(super) fn palette() -> Palette {
    let palette = PALETTE.lock().ok().and_then(|palette| *palette);
    palette.unwrap_or_else(|| Palette::of(&Theme::BATERI))
}

/// Makes `palette` the window's; `true` when it was another (the window is to be drawn again).
pub(super) fn set(palette: Palette) -> bool {
    let Ok(mut slot) = PALETTE.lock() else {
        return false;
    };
    slot.replace(palette) != Some(palette)
}

/// The theme's light or dark, as the window's appearance: what AppKit draws itself follows it.
pub(super) fn appearance(theme: &Theme) -> Option<Retained<NSAppearance>> {
    // SAFETY: two constant `NSString`s AppKit exposes; they live for the whole process.
    let name = unsafe {
        if crate::window::is_dark_background(theme) {
            objc2_app_kit::NSAppearanceNameDarkAqua
        } else {
            objc2_app_kit::NSAppearanceNameAqua
        }
    };
    NSAppearance::appearanceNamed(name)
}

thread_local! {
    /// The dynamic colours made so far, one per role and strength.
    static INKS: RefCell<HashMap<(Ink, u64), Retained<NSColor>>> = RefCell::new(HashMap::new());
}

/// `ink` as a colour that reads the window's palette when it is drawn.
pub(super) fn ink(ink: Ink) -> Retained<NSColor> {
    faded(ink, 1.0)
}

/// `ink` at `alpha` of its own, read when drawn like [`ink`].
pub(super) fn faded(ink: Ink, alpha: f64) -> Retained<NSColor> {
    let key = (ink, alpha.to_bits());
    if let Some(color) = INKS.with(|inks| inks.borrow().get(&key).cloned()) {
        return color;
    }
    let provider = RcBlock::new(move |_: NonNull<NSAppearance>| -> NonNull<NSColor> {
        let color = Tint::of(ink.rgb(&palette()), alpha).color();
        // SAFETY: an autoreleased object's pointer is the one the `Retained` held, never null;
        // the block's caller takes it at +0, inside the pool it draws in.
        unsafe { NonNull::new_unchecked(Retained::autorelease_ptr(color)) }
    });
    // SAFETY: the provider returns a valid, autoreleased colour on every call.
    let color = unsafe { NSColor::colorWithName_dynamicProvider(None, &provider) };
    INKS.with(|inks| inks.borrow_mut().insert(key, color.clone()));
    color
}

/// `ink` at `alpha` in the palette as it is now, a plain colour: for a button's title, which keeps
/// the colour it is first drawn in and is titled again when the palette changes.
pub(super) fn fixed(ink: Ink, alpha: f64) -> Retained<NSColor> {
    Tint::of(ink.rgb(&palette()), alpha).color()
}

/// Marks `view` and everything in it to be drawn again: after a palette change, the colours read
/// the new one.
pub(super) fn redraw(view: &NSView) {
    view.setNeedsDisplay(true);
    for subview in view.subviews() {
        redraw(&subview);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mark on the accent is whichever of the theme's background and foreground stands out
    /// more on it.
    #[test]
    fn the_mark_on_the_accent_is_the_one_that_reads() {
        for theme in [Theme::BATERI, Theme::ULTRAVIOLET] {
            let palette = Palette::of(&theme);
            let mark = on_accent(&palette);
            assert!([palette.background, palette.foreground].contains(&mark));
            let other = if mark == palette.background {
                palette.foreground
            } else {
                palette.background
            };
            assert!(
                bt_core::contrast_ratio(mark, palette.accent)
                    >= bt_core::contrast_ratio(other, palette.accent)
            );
        }
    }

    /// Setting the palette says whether it changed.
    #[test]
    fn setting_the_palette_says_whether_it_changed() {
        let dark = Palette::of(&Theme::BATERI);
        let other = Palette::of(&Theme::ULTRAVIOLET);
        set(dark);
        assert!(!set(dark));
        assert!(set(other));
        assert_eq!(palette(), other);
    }
}
