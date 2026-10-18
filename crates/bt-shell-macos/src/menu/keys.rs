//! The keys of the menu bar are the shortcut table's.
//!
//! No item of the bar is built with a key: [`apply`] writes them — to the whole bar once it is
//! built (the table's defaults) and again whenever `[keys]` changes, and to a submenu the shell
//! fills as it opens. It finds an item's command by the id the palette gives a menu command
//! (`menu_item_id`: the selector, and `:tag` when the tag is not 0), asks the table which keys the
//! command has and writes the first to the item.
//!
//! **The second key is a hidden copy** of the item (Safari's idiom, ⌃⇥ for Show Next Tab): the
//! item is invisible and its key works — a hidden item's key is ignored unless the flag says
//! otherwise. [`apply`] takes every copy away and makes them again, so the copies are always
//! the table's and never the last read's. A copy has the action, the tag and the target of its
//! item, which is all the validation and the action look at.
//!
//! An item whose command the table does not know (the themes, the open ports, the tabs a split
//! can go to) is left as it is; the table lists commands, not the entries of a list that changes.

use std::collections::{HashMap, HashSet};

use bt_core::{Chord, Mods};
use bt_shell_common::palette::{Modifiers, menu_item_id};
use bt_shell_common::shortcuts::{
    self, COMMANDS, Resolved, by_id, by_name, family_default, menu_key,
};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSEventModifierFlags, NSMenu, NSMenuItem, NSUserInterfaceItemIdentification};
use objc2_foundation::NSString;

use super::END_PROGRAMS_TAG;

/// What marks an item as the second key of another: set on the copies [`apply`] makes.
const COPY: &str = "bateri.shortcut.second";

/// The modifier flags a menu item's mask holds for `modifiers`.
pub(crate) fn flags_of(modifiers: Modifiers) -> NSEventModifierFlags {
    let mut flags = NSEventModifierFlags::empty();
    if modifiers.control {
        flags |= NSEventModifierFlags::Control;
    }
    if modifiers.option {
        flags |= NSEventModifierFlags::Option;
    }
    if modifiers.shift {
        flags |= NSEventModifierFlags::Shift;
    }
    if modifiers.command {
        flags |= NSEventModifierFlags::Command;
    }
    flags
}

/// The command id of a menu item, the palette's (`None` for a separator and a submenu's holder).
fn id_of(item: &NSMenuItem) -> Option<String> {
    if item.submenu().is_some() {
        return None;
    }
    let action = item.action()?;
    Some(menu_item_id(&action.name().to_string_lossy(), item.tag()))
}

fn is_copy(item: &NSMenuItem) -> bool {
    item.identifier()
        .is_some_and(|identifier| identifier.to_string() == COPY)
}

/// Writes `chord` to the item, or takes its key away.
fn set_key(item: &NSMenuItem, chord: Option<&Chord>) {
    match chord {
        Some(chord) => {
            let held = menu_key(chord);
            item.setKeyEquivalent(&NSString::from_str(&held.key));
            item.setKeyEquivalentModifierMask(flags_of(held.modifiers));
        }
        None => {
            item.setKeyEquivalent(&NSString::new());
            item.setKeyEquivalentModifierMask(NSEventModifierFlags::empty());
        }
    }
}

/// The hidden item that carries `primary`'s second key.
fn copy_of(mtm: MainThreadMarker, primary: &NSMenuItem, chord: &Chord) -> Retained<NSMenuItem> {
    // SAFETY: the action is the primary's own, an action the responders it reaches define; the
    // target is the primary's (`nil` but for Check for Updates, whose updater the app delegate
    // keeps alive).
    let copy = unsafe {
        let copy = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &primary.title(),
            primary.action(),
            &NSString::new(),
        );
        copy.setTarget(primary.target().as_deref());
        copy
    };
    copy.setTag(primary.tag());
    set_key(&copy, Some(chord));
    copy.setHidden(true);
    copy.setAllowsKeyEquivalentWhenHidden(true);
    copy.setIdentifier(Some(&NSString::from_str(COPY)));
    copy
}

/// Quit and End Programs is Quit's alternate (⌥ held) only while its key is Quit's key with
/// other modifiers; with a key of its own it is an item like the others (hidden or not by
/// `keep_running`). AppKit wants an alternate to share its key with the item before it.
fn align_alternate(menu: &NSMenu, item: &NSMenuItem, index: isize) {
    let key = item.keyEquivalent().to_string();
    // The item before it, but for the hidden copies of the one it follows.
    let before = (0..index)
        .rev()
        .filter_map(|at| menu.itemAtIndex(at))
        .find(|before| !is_copy(before));
    let paired = !key.is_empty()
        && before.is_some_and(|before| {
            before.keyEquivalent().to_string() == key
                && before.keyEquivalentModifierMask() != item.keyEquivalentModifierMask()
        });
    item.setAlternate(paired);
}

/// Writes the keys `resolved` gives to every command item of `menu` and its submenus.
pub(crate) fn apply(mtm: MainThreadMarker, menu: &NSMenu, resolved: &Resolved) {
    let keys = resolved.item_keys();
    walk(mtm, menu, &keys);
}

fn walk(mtm: MainThreadMarker, menu: &NSMenu, keys: &HashMap<String, Vec<Chord>>) {
    for item in menu.itemArray().iter() {
        if is_copy(&item) {
            menu.removeItem(&item);
        }
    }
    let mut at: isize = 0;
    while at < menu.numberOfItems() {
        let Some(item) = menu.itemAtIndex(at) else {
            break;
        };
        at += 1;
        if let Some(submenu) = item.submenu() {
            walk(mtm, &submenu, keys);
            continue;
        }
        let Some(id) = id_of(&item) else {
            continue;
        };
        if by_id(&id).is_none() {
            continue;
        }
        let chords = keys.get(&id).map_or(&[][..], Vec::as_slice);
        set_key(&item, chords.first());
        if item.tag() == END_PROGRAMS_TAG {
            align_alternate(menu, &item, at - 1);
        }
        if let Some(second) = chords.get(1) {
            menu.insertItem_atIndex(&copy_of(mtm, &item, second), at);
            at += 1;
        }
    }
}

/// What disagrees between the bar and the table: a keyed item the table does not list (its key
/// would be one the settings page cannot move), and a command of the table with no item (its
/// key would do nothing). Check for Updates is in the bar only with an updater. Empty when the
/// bar is the table's; the installer asserts it in a debug build, which the smoke run is.
pub(crate) fn problems(bar: &NSMenu, updater: bool) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    collect(bar, &mut seen, &mut out);
    for command in COMMANDS {
        if !updater && command.name == "check_for_updates" {
            continue;
        }
        if !seen.iter().any(|id| by_id(id) == Some(command)) {
            out.push(format!(
                "`{}` ({}) has no menu item",
                command.name, command.id
            ));
        }
    }
    out
}

fn collect(menu: &NSMenu, seen: &mut HashSet<String>, out: &mut Vec<String>) {
    for item in menu.itemArray().iter() {
        if let Some(submenu) = item.submenu() {
            collect(&submenu, seen, out);
        } else if let Some(id) = id_of(&item) {
            if !item.keyEquivalent().to_string().is_empty() && by_id(&id).is_none() {
                out.push(format!("`{id}` has a key and is no command of the table"));
            }
            seen.insert(id);
        }
    }
}

/// The glyphs of the first shortcut the command `name` has, as the bar shows it (`⇧⌘D`), for
/// the texts that name a key — a tooltip, a row of a list. `None` when the command has no key
/// or no application object is up (an embedded pane has none).
pub(crate) fn glyphs_of(mtm: MainThreadMarker, name: &str) -> Option<String> {
    let app = crate::app::delegate(mtm)?;
    let keys = app.keys();
    let binding = keys.binding(name)?;
    binding.shortcuts.iter().find_map(shortcuts::glyphs)
}

/// The modifiers the family `name` (`select_tab`, `select_group`) has now: its items' keys, the
/// hints that show them. `None` when the file took them away, and then nothing shows a hint.
/// Without an application object the table's own.
pub(crate) fn family_mods(mtm: MainThreadMarker, name: &str) -> Option<Mods> {
    match crate::app::delegate(mtm) {
        Some(app) => app.keys().family_mods(name),
        None => by_name(name).and_then(family_default),
    }
}
