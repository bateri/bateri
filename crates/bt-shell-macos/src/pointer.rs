//! What the pointer's modifier keys say over a pane: ⌘ without ⌥ is a link's key. One reading
//! for every surface that takes the pointer (the pane's view and its links), so the key never
//! means two things.
//!
//! And which cursor a program's pointer shape (OSC 22, `bt_core::PointerShape`) shows
//! ([`cursor`]).

use bt_core::PointerShape;
use objc2::available;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSCursor, NSCursorFrameResizeDirections, NSCursorFrameResizePosition, NSEventModifierFlags,
};

/// Whether ⌘ is down **as a link's key**: with ⌥ it is a shortcut's (⌥⌘ + an arrow selects a
/// split), and a link does not open under it.
pub(crate) fn link_command(flags: NSEventModifierFlags) -> bool {
    flags.contains(NSEventModifierFlags::Command) && !flags.contains(NSEventModifierFlags::Option)
}

/// The cursor a program's pointer shape shows. AppKit has no cursor for `help`, `progress`
/// and `wait` (the spinning wait cursor is the system's, not an app's): those show the
/// arrow. The zoom, divider and frame-resize cursors came with macOS 15; on 14 a zoom and a
/// diagonal show the arrow, an edge or a divider the older resize cursors.
pub(crate) fn cursor(shape: PointerShape) -> Retained<NSCursor> {
    use PointerShape as S;
    if available!(macos = 15.0) {
        let frame = |position| {
            NSCursor::frameResizeCursorFromPosition_inDirections(
                position,
                NSCursorFrameResizeDirections::All,
            )
        };
        match shape {
            S::ZoomIn => return NSCursor::zoomInCursor(),
            S::ZoomOut => return NSCursor::zoomOutCursor(),
            S::EwResize | S::ColResize => return NSCursor::columnResizeCursor(),
            S::NsResize | S::RowResize => return NSCursor::rowResizeCursor(),
            S::NResize => return frame(NSCursorFrameResizePosition::Top),
            S::SResize => return frame(NSCursorFrameResizePosition::Bottom),
            S::EResize => return frame(NSCursorFrameResizePosition::Right),
            S::WResize => return frame(NSCursorFrameResizePosition::Left),
            S::NeResize | S::NeswResize => return frame(NSCursorFrameResizePosition::TopRight),
            S::NwResize | S::NwseResize => return frame(NSCursorFrameResizePosition::TopLeft),
            S::SeResize => return frame(NSCursorFrameResizePosition::BottomRight),
            S::SwResize => return frame(NSCursorFrameResizePosition::BottomLeft),
            _ => {}
        }
    }
    match shape {
        S::ContextMenu => NSCursor::contextualMenuCursor(),
        S::Pointer => NSCursor::pointingHandCursor(),
        S::Cell | S::Crosshair => NSCursor::crosshairCursor(),
        S::Text => NSCursor::IBeamCursor(),
        S::VerticalText => NSCursor::IBeamCursorForVerticalLayout(),
        S::Alias => NSCursor::dragLinkCursor(),
        S::Copy => NSCursor::dragCopyCursor(),
        S::Move | S::Grab | S::AllScroll => NSCursor::openHandCursor(),
        S::Grabbing => NSCursor::closedHandCursor(),
        S::NoDrop | S::NotAllowed => NSCursor::operationNotAllowedCursor(),
        S::EResize
        | S::WResize
        | S::NResize
        | S::SResize
        | S::EwResize
        | S::ColResize
        | S::NsResize
        | S::RowResize => older_resize(shape),
        S::Default
        | S::Help
        | S::Progress
        | S::Wait
        | S::ZoomIn
        | S::ZoomOut
        | S::NeResize
        | S::NwResize
        | S::SeResize
        | S::SwResize
        | S::NeswResize
        | S::NwseResize => NSCursor::arrowCursor(),
    }
}

/// An edge's or a divider's cursor before macOS 15: the ones AppKit deprecated in 15 for
/// the divider and frame cursors, which 14 does not have.
#[allow(deprecated)]
fn older_resize(shape: PointerShape) -> Retained<NSCursor> {
    match shape {
        PointerShape::EResize => NSCursor::resizeRightCursor(),
        PointerShape::WResize => NSCursor::resizeLeftCursor(),
        PointerShape::NResize => NSCursor::resizeUpCursor(),
        PointerShape::SResize => NSCursor::resizeDownCursor(),
        PointerShape::NsResize | PointerShape::RowResize => NSCursor::resizeUpDownCursor(),
        _ => NSCursor::resizeLeftRightCursor(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_links_key_is_command_without_option() {
        let both = NSEventModifierFlags::Command | NSEventModifierFlags::Option;
        // The link's key is ⌘ alone of the two.
        assert!(link_command(NSEventModifierFlags::Command));
        assert!(!link_command(both));
        assert!(!link_command(NSEventModifierFlags::empty()));
    }
}
