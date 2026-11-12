//! The terminal's context menu. A right press the terminal owns opens it wherever it lands: in
//! the grid with mouse mode off, or with Shift (the mode's escape); always on the fill band and
//! the dock, which are never the application's screen. A program that asked for the mouse (vim,
//! htop) still gets the right press as a report.
//!
//! - **What it holds**: on a link the link's items first ([`crate::hyperlink`]), then the edit
//!   items ([`edit_items`]): Cut while the dock has a selection it can cut, Copy (grey without a
//!   selection), Paste, Select All. The press selects nothing, so a right click on selected text
//!   finds that text still selected for Copy.
//! - **The items act on the view right-clicked**, not on the first responder: in a split the
//!   press may land on a pane that does not have the keyboard, and its Copy copies that pane's
//!   selection. They are the main menu's own actions (`cut:`, `copy:`, `selectAll:`), so a page
//!   editor, the files and the image a paste can carry go the one way.
//! - **Paste gives the pane the keyboard first** (`pasteFromMenu:`, [`BateriView`]'s): pasted
//!   text waits for a Return, which must reach the pane it was pasted into.
//! - **No keys are shown**: the keys are the shortcut table's, and the main menu shows them.
//!
//! An `impl BateriView` block next to `view.rs`, as `hyperlink` and `uploader` are.

use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{MainThreadOnly, sel};
use objc2_app_kit::{NSEvent, NSMenu, NSMenuItem};
use objc2_foundation::{NSString, ns_string};

use crate::view::BateriView;

/// An edit item of the context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edit {
    Cut,
    Copy,
    Paste,
    SelectAll,
}

impl Edit {
    fn title(self) -> &'static NSString {
        match self {
            Self::Cut => ns_string!("Cut"),
            Self::Copy => ns_string!("Copy"),
            Self::Paste => ns_string!("Paste"),
            Self::SelectAll => ns_string!("Select All"),
        }
    }

    /// The action sent to the view right-clicked. Paste is the menu's own: the pane takes the
    /// keyboard before it pastes.
    fn action(self) -> Sel {
        match self {
            Self::Cut => sel!(cut:),
            Self::Copy => sel!(copy:),
            Self::Paste => sel!(pasteFromMenu:),
            Self::SelectAll => sel!(selectAll:),
        }
    }
}

/// The edit items in their order, each with whether it is enabled. Cut is there only while the
/// dock has a selection it can cut: the grid has nothing to cut, and a grey Cut would stand in
/// every menu. Copy is grey without a selection. Paste is always on, as the main menu's is (an
/// empty pasteboard pastes nothing), and so is Select All.
fn edit_items(can_cut: bool, has_selection: bool) -> Vec<(Edit, bool)> {
    let mut items = Vec::with_capacity(4);
    if can_cut {
        items.push((Edit::Cut, true));
    }
    items.extend([
        (Edit::Copy, has_selection),
        (Edit::Paste, true),
        (Edit::SelectAll, true),
    ]);
    items
}

impl BateriView {
    /// A right press the terminal owns: the menu at the press, headed by the link's items if a
    /// link is under it ([`BateriView::link_menu`]), the edit items alone otherwise.
    pub(crate) fn context_menu(&self, event: &NSEvent) {
        let in_window = event.locationInWindow();
        let at = self.convertPoint_fromView(in_window, None);
        if !self.link_menu(in_window, at) {
            self.pop_menu(None, &[], at);
        }
    }

    /// Appends the edit items to `menu`. `has_selection` and not the selection's text: the
    /// menu opens without turning a whole selected history into a string.
    pub(crate) fn add_edit_items(&self, menu: &NSMenu) {
        let session = self.session();
        let can_cut = session.is_some_and(|session| session.can_cut());
        let has_selection = session.is_some_and(|session| session.has_selection());
        for (edit, enabled) in edit_items(can_cut, has_selection) {
            menu.addItem(&self.menu_item(edit.title(), edit.action(), enabled));
        }
    }

    /// An item of the context menu: `action` sent to this view, enabled as `enabled` says.
    pub(crate) fn menu_item(
        &self,
        title: &NSString,
        action: Sel,
        enabled: bool,
    ) -> Retained<NSMenuItem> {
        let mtm = self.mtm();
        // SAFETY: `initWithTitle:action:keyEquivalent:` takes two `NSString`s and a selector the
        // target implements (`BateriView`'s context menu selectors, checked by
        // `the_view_answers_every_action_of_its_context_menu`); `setTarget:` keeps the target
        // weakly and the view outlives the menu's modal tracking.
        let item = unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                title,
                Some(action),
                ns_string!(""),
            );
            item.setTarget(Some(self));
            item
        };
        item.setEnabled(enabled);
        item
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::ClassType;

    #[test]
    fn copy_is_grey_without_a_selection_and_paste_and_select_all_are_always_on() {
        assert_eq!(
            edit_items(false, false),
            [
                (Edit::Copy, false),
                (Edit::Paste, true),
                (Edit::SelectAll, true)
            ]
        );
        assert_eq!(
            edit_items(false, true),
            [
                (Edit::Copy, true),
                (Edit::Paste, true),
                (Edit::SelectAll, true)
            ]
        );
    }

    #[test]
    fn cut_heads_the_items_only_while_the_dock_can_cut() {
        assert_eq!(
            edit_items(true, true),
            [
                (Edit::Cut, true),
                (Edit::Copy, true),
                (Edit::Paste, true),
                (Edit::SelectAll, true)
            ]
        );
    }

    /// The items are targeted at the view, so an action it does not answer is an item that does
    /// nothing, without a word: the edit items' and the link items' alike.
    #[test]
    fn the_view_answers_every_action_of_its_context_menu() {
        let edits = [Edit::Cut, Edit::Copy, Edit::Paste, Edit::SelectAll].map(Edit::action);
        let links = [
            sel!(openLinkFromMenu:),
            sel!(revealLinkFromMenu:),
            sel!(copyLinkFromMenu:),
            sel!(copyScpPathFromMenu:),
            sel!(downloadLinkFromMenu:),
            sel!(downloadLinkToFromMenu:),
        ];
        for action in edits.into_iter().chain(links) {
            assert!(
                BateriView::class().responds_to(action),
                "the view does not answer `{action:?}`"
            );
        }
    }
}
