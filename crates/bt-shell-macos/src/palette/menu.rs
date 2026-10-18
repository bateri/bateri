//! Reading the application menu as it stands: the palette's commands are the menu's items.
//!
//! The reading happens **before** the palette takes the keyboard. An item without a target is
//! enabled, titled and ticked by whoever its action reaches along the responder chain from
//! the key window's first responder (`validateMenuItem:`: the pane, the window, the
//! application); the moment the palette's field is the first responder the answers would be a
//! text field's. So the menu is validated while the pane still has the keyboard, and what is read
//! is carried in the palette.
//!
//! A submenu the shell fills as it opens (the themes, the tabs a split can go to, Open Port) is
//! filled the way a menu opening fills it: the delegate is asked, then the menu is updated.

use bt_shell_common::palette::{MenuNode, MenuRows, Modifiers, menu_rows};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, Message, sel};
use objc2_app_kit::{
    NSApplication, NSControlStateValueOn, NSEventModifierFlags, NSMenu, NSMenuDelegate, NSMenuItem,
};

use objc2_foundation::NSObjectProtocol;

use crate::menu::{MARK_HOLDER_TAG, PORTS_HOLDER_TAG};

/// The menu read: the rows the palette lists and the items they run.
pub(super) struct Snapshot {
    pub(super) rows: MenuRows,
    /// Indexed by [`MenuNode::token`].
    items: Vec<Retained<NSMenuItem>>,
}

impl Snapshot {
    /// Validates and reads the whole menu bar.
    pub(super) fn take(mtm: MainThreadMarker) -> Snapshot {
        let (nodes, items) = read(mtm);
        Snapshot {
            rows: menu_rows(&nodes),
            items,
        }
    }

    /// The menu item the row with this id runs.
    pub(super) fn item(&self, id: &str) -> Option<Retained<NSMenuItem>> {
        self.items.get(self.rows.token_of(id)?).cloned()
    }
}

/// The whole menu bar as the toolkit-free nodes the rules read, and the items they stand for
/// (indexed by [`MenuNode::token`]).
fn read(mtm: MainThreadMarker) -> (Vec<MenuNode>, Vec<Retained<NSMenuItem>>) {
    let mut items = Vec::new();
    let nodes: Vec<MenuNode> = NSApplication::sharedApplication(mtm)
        .mainMenu()
        .map(|bar| {
            bar.itemArray()
                .iter()
                .map(|holder| node(&holder, &mut items))
                .collect()
        })
        .unwrap_or_default();
    (nodes, items)
}

/// The menu bar as it stands, every item: the greyed, the hidden and the alternates too.
/// Settings ▸ Keyboard lists a command by the title and the place the menu gives it.
pub(crate) fn nodes(mtm: MainThreadMarker) -> Vec<MenuNode> {
    read(mtm).0
}

/// Fills and validates `menu` the way opening it would, so its items say what they are now.
fn prepare(menu: &NSMenu) {
    if let Some(delegate) = menu.delegate() {
        // Both are `NSMenuDelegate`'s optional methods, each taking the menu; they are sent only
        // to a delegate that answers to them.
        if delegate.respondsToSelector(sel!(menuNeedsUpdate:)) {
            delegate.menuNeedsUpdate(menu);
        }
        if delegate.respondsToSelector(sel!(menuWillOpen:)) {
            delegate.menuWillOpen(menu);
        }
    }
    menu.update();
}

/// One item and, under it, the items of its submenu.
fn node(item: &NSMenuItem, items: &mut Vec<Retained<NSMenuItem>>) -> MenuNode {
    let token = items.len();
    items.push(item.retain());
    let submenu = item.submenu();
    let mask = item.keyEquivalentModifierMask();
    let children = submenu.as_ref().map(|menu| {
        prepare(menu);
        menu.itemArray()
            .iter()
            .map(|child| node(&child, items))
            .collect()
    });
    // A submenu with a delegate is filled as it opens, and Open Port and Mark … as are filled or
    // titled by the Shell menu's: their items mean something only under their holder.
    let paged = submenu
        .as_ref()
        .is_some_and(|menu| menu.delegate().is_some())
        || item.tag() == MARK_HOLDER_TAG
        || item.tag() == PORTS_HOLDER_TAG;
    MenuNode {
        token,
        title: item.title().to_string(),
        selector: item
            .action()
            .filter(|_| submenu.is_none())
            .map(|action| action.name().to_string_lossy().into_owned()),
        tag: item.tag(),
        key: item.keyEquivalent().to_string(),
        modifiers: Modifiers {
            control: mask.contains(NSEventModifierFlags::Control),
            option: mask.contains(NSEventModifierFlags::Option),
            shift: mask.contains(NSEventModifierFlags::Shift),
            command: mask.contains(NSEventModifierFlags::Command),
        },
        enabled: item.isEnabled(),
        hidden: item.isHidden(),
        alternate: item.isAlternate(),
        separator: item.isSeparatorItem(),
        checked: item.state() == NSControlStateValueOn,
        children,
        paged,
    }
}
