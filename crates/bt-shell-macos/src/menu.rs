//! Main menu: the app menu (About, Check for Updates…, Settings…, Hide, Quit
//! and — ⌥ held, under `keep_running = "quit"` only — Quit and End Programs), Shell (New
//! Window, New Tab, New Local Tab, Mark Host as ▸, Shell Integration on Host,
//! Forget Password, Cancel Upload, Open Port ▸, Split
//! Right, Split Down, Close Tab/Close, Close Window), Edit (Undo Move, Cut, Copy, Paste, Paste
//! Escaped Text, Select All, Clear to Start, Clear Scrollback, Find ▸
//! Find…/Find Next/Find Previous/Use Selection for Find), View (Theme ▸,
//! Bigger, Smaller, Actual Size, Scroll to Top, Scroll to Bottom, Page Up,
//! Page Down) and Window (Minimize,
//! Zoom, Show Previous/Next Tab, Select Tab ▸, Show All Tabs, Rename Tab…, splits — Select Previous/Next
//! Split, Select Split ▸, Resize Split ▸, Equalize Splits, Zoom Split —, Move
//! Tab to New Window, Merge All Windows, Bring All to Front). Settings… (⌘,) opens the settings window
//! (`settings_window`; it used to open the file in the editor, that job is now
//! on the window's "Open settings.toml" button); the item and shortcut are the same.
//!
//! **With one exception no item has a target** (below). The action passes through the responder chain and reaches
//! the first object that defines it: `cut:`/`copy:`/`paste:`/`pasteEscaped:`/
//! `selectAll:` to the first responder `BateriView` (Cut's enablement is in its
//! `validateMenuItem:` — only while there is a dock selection and the editing gate
//! is open; Paste Escaped Text's while the pasteboard has text); the font-size
//! actions, Find ▸'s four actions, the two clearing actions, the four
//! scrolling actions (grey on the alternate screen) and `cancelUpload:`
//! to the focused pane (`pane::TerminalPane`, `BateriView`'s parent view);
//! `closeTab:`, `closeWindow:`, `selectTab:`, `showNextTab:`,
//! `showPreviousTab:`, `showTabList:` and `detachTab:` (all grey with a single
//! tab), `renameTab:` (a lone tab is named from here too), `splitRight:`,
//! `splitDown:` and the splits' navigation/layout actions
//! (`selectPreviousSplit:`, `selectNextSplit:`, `selectSplit:`,
//! `resizeSplit:`, `equalizeSplits:`, `toggleSplitZoom:`; grey with a single pane)
//! to the key window's delegate (`window::TerminalWindow`, which carries the
//! tabs — the tab actions are named apart from `NSWindow`'s own, which the
//! window would answer before its delegate); `performMiniaturize:` and
//! `performZoom:` to `NSWindow` itself; `mergeWindows:` (Merge All Windows,
//! grey with a single window) to the app delegate, which reaches every window;
//! `openSettings:`, the theme actions, `markHost:`, `toggleHostIntegration:`
//! (its title, checkmark and grey state in the app delegate's
//! `validateMenuItem:`, from [`integration_menu`]) and
//! `newWindow:`/`newTab:`/`newLocalTab:`/`quitAndEndPrograms:` to the app delegate
//! (the same path as the settings record's `settingsDidChange:` — they spread to all
//! windows or must work even when there is no window);
//! `terminate:`, `hide:`, `arrangeInFront:` and
//! `orderFrontStandardAboutPanel:` to `NSApp` itself. The menu therefore holds a
//! reference to no one; if nothing handles the action AppKit shows the item
//! disabled.
//!
//! **The one item with a target is "Check for Updates…"**: its action
//! (`checkForUpdates:`) is on Sparkle's updater and not in that responder chain
//! ([`crate::updater`]); its enablement comes from its own `validateMenuItem:`
//! too (grey while a check is running). If there is no updater (unbundled run,
//! timed run) the item is not added at all — a grey item would promise something
//! that will never work.
//!
//! Two exceptions are delegates. Theme ▸'s is the app delegate: the submenu is not
//! fixed, it is filled from `themes/` when opening ([`fill_themes`]). Shell's is
//! [`ShellMenuDelegate`]: the title, enablement and checkmark of Mark “{host}” as ▸
//! follow the active tab ([`mark_menu`]). The delegate is a weak
//! reference; the app delegate keeps both alive for the whole process.
//!
//! Shell's delegate is **a separate object and only `menuWillOpen:`**: a delegate
//! that defines `menuNeedsUpdate:` or `menuHasKeyEquivalent:…` enters AppKit's
//! shortcut search — the app delegate's says "no shortcut" and if it were
//! attached to Shell ⌘N/⌘T/⌘W would die. The submenu's holder does not go through
//! validation (measured: its target is the submenu itself,
//! `submenuAction:`, and `update` does not touch its `setEnabled`), so the
//! grey state and the title are set by hand on opening.
//!
//! Shortcuts come from here too: AppKit gives a Command key to the main menu
//! before `keyDown:` (`performKeyEquivalent:`), and the `view` swallows what is
//! not caught. **A Control key is also** asked of the menu first (measured):
//! ⌃⇥ and ⌃⇧⇥ switch tabs as hidden Window items and `keyDown:`'s Cmd
//! allow-list stays untouched; Ctrl-I still goes to zsh as a tab.
//! **A function key too**: ⌘Home/⌘End/⌘PgUp/⌘PgDn are View items, the
//! shortcut character is AppKit's function-key code point
//! (`NSHomeFunctionKey` U+F729 …). A menu shortcut, not a key encoding — the
//! swallowing of Home/End in `keyDown:` and the invariant of `bt_core::Arrow` are
//! untouched. The splits' shortcuts go the same way: ⌘[ / ⌘],
//! ⌥⌘/⌃⌘ + arrow (the arrow keys' code points U+F700–U+F703), ⌃⌘= and ⇧⌘↩ —
//! `keyDown:`'s three-key Cmd allow-list (⌘⌫, ⌘←, ⌘→) and the dock's ⇧⏎ do not
//! change, because the menu matches them together with their modifiers.
//!
//! **AppKit adds no tab items**: macOS's own tabs are off
//! (`NSWindow.allowsAutomaticWindowTabbing = false`, set before this menu is
//! built), so Show Tab Bar / Show All Tabs do not come to View — the tab
//! items here are bateri's own, acting on its own tab bar.
//!
//! Strings are English. The app menu's title in the menu bar
//! comes from the process name, not from here; the "bateri" in the items' names is
//! written by hand. AppKit adds its own items (dictation, emoji) to the menu named
//! "Edit" and the full-screen item to the one named "View"; to the Window menu
//! registered with `setWindowsMenu` it adds the window list and placement items.

use bt_core::{HostMark, KeepRunning, MarkSubject, SYSTEM_THEME, bare_host};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSEventModifierFlags, NSMenu,
    NSMenuDelegate, NSMenuItem,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};

/// The title of Window ▸ Move Split to Tab ▸ — also how its delegate knows the
/// menu ([`ShellMenuDelegate`]): the list is the key window's other tabs, built
/// on opening.
const MOVE_TO_TAB_TITLE: &str = "Move Split to Tab";

/// The `tag` of the Shell ▸ Mark … as ▸ holder: [`ShellMenuDelegate`] finds it in
/// the Shell menu with this.
const MARK_HOLDER_TAG: isize = 37;

/// The `tag` of Shell ▸ Forget Password for “{host}”:
/// [`ShellMenuDelegate`] writes the host into its title.
const FORGET_TAG: isize = 47;

/// The `tag` of the Shell ▸ Open Port ▸ holder: [`ShellMenuDelegate`] fills
/// its submenu with the focused pane's listening ports.
const PORTS_HOLDER_TAG: isize = 58;

/// The `tag` of bateri ▸ Quit and End Programs: [`set_end_programs_visible`]
/// finds it in the app menu with this.
const END_PROGRAMS_TAG: isize = 56;

/// Whether bateri ▸ Quit and End Programs (⌥⌘Q) is there: only while ⌘Q
/// keeps the programs (`keep_running = "quit"`) — under the other values ⌘Q
/// already ends them, and a second item would promise a difference that is
/// not there.
pub(crate) fn shows_end_programs(keep: KeepRunning) -> bool {
    keep == KeepRunning::Quit
}

/// Shows or hides Quit and End Programs for `keep` ([`shows_end_programs`]):
/// at launch and on every settings save. Hidden, its shortcut is dead too
/// (AppKit ignores a hidden item's key equivalent), so ⌥⌘Q does nothing
/// outside `"quit"`. Set by hand: a hidden item is not validated.
pub(crate) fn set_end_programs_visible(mtm: MainThreadMarker, keep: KeepRunning) {
    let app_menu = NSApplication::sharedApplication(mtm)
        .mainMenu()
        .and_then(|bar| bar.itemAtIndex(0))
        .and_then(|holder| holder.submenu());
    if let Some(item) = app_menu.and_then(|menu| menu.itemWithTag(END_PROGRAMS_TAG)) {
        item.setHidden(!shows_end_programs(keep));
    }
}

/// Quit bateri's alternate (⌥ held, ⌥⌘Q): `quitAndEndPrograms:` on the app
/// delegate — today's quit with its question, the programs end. Hidden until
/// [`set_end_programs_visible`] says otherwise.
fn end_programs_item(mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    let item = with_modifiers(
        item(mtm, "Quit and End Programs", sel!(quitAndEndPrograms:), "q"),
        NSEventModifierFlags::Command | NSEventModifierFlags::Option,
    );
    item.setAlternate(true);
    item.setTag(END_PROGRAMS_TAG);
    item.setHidden(true);
    item
}

/// Forget Password's title on a remote tab (`Some` host, without `user@`) or
/// locally — a UI string. Its enablement is the pane's `validateMenuItem:`
/// (a saved password for the tab's account).
pub(crate) fn forget_title(host: Option<&str>) -> String {
    match host {
        Some(host) => format!("Forget Password for \u{201c}{}\u{201d}", bare_host(host)),
        None => "Forget Password".to_owned(),
    }
}

/// Shell ▸ Shell Integration on “{host}”: the toggle's state, pure.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct IntegrationMenu {
    pub(crate) title: String,
    pub(crate) enabled: bool,
    pub(crate) checked: bool,
}

/// The toggle's model: on a remote tab (`Some((host, on))`, `on` the
/// **resolved** answer — `Settings::integration_for`, so a production-marked
/// host without an entry of its own shows unchecked) the title carries the
/// host without `user@`; locally "Shell Integration on Host", grey and
/// unchecked. The action writes the opposite of `on` for this host.
pub(crate) fn integration_menu(remote: Option<(&str, bool)>) -> IntegrationMenu {
    match remote {
        Some((host, on)) => IntegrationMenu {
            title: format!("Shell Integration on \u{201c}{}\u{201d}", bare_host(host)),
            enabled: true,
            checked: on,
        },
        None => IntegrationMenu {
            title: "Shell Integration on Host".to_owned(),
            enabled: false,
            checked: false,
        },
    }
}

/// The items of Mark … as ▸, in order; an item's `tag` is the index here and the
/// action (`markHost:`) reads the mark from it ([`mark_of_tag`]). No direct color:
/// the menu never writes it.
const MARKS: [(&str, HostMark); 4] = [
    ("Production", HostMark::Production),
    ("Staging", HostMark::Staging),
    ("Development", HostMark::Development),
    ("None", HostMark::None),
];

/// The mark from a Mark … as ▸ item's `tag`; an unknown `tag` gives `None`.
pub(crate) fn mark_of_tag(tag: isize) -> Option<HostMark> {
    usize::try_from(tag)
        .ok()
        .and_then(|index| MARKS.get(index))
        .map(|(_, mark)| *mark)
}

/// Mark … as ▸ as it stands ([`mark_menu`]).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MarkMenu {
    pub(crate) title: String,
    pub(crate) enabled: bool,
    /// Index in [`MARKS`] of the checked item; absent for a direct color and locally.
    pub(crate) checked: Option<usize>,
}

/// The current state of Mark … as ▸ ([`mark_menu`]).
///
/// The menu's model, pure: with a markable host (a remote tab's, the server
/// of a database client's guide bar or a Kubernetes context) the title
/// carries the name the menu writes — a host without its `user@`, a context
/// whole ([`MarkSubject::name`]) — and the checkmark is on the **effective
/// resolution** (even if it comes from a glob); without one (`None`) "Mark
/// Host as" and grey.
pub(crate) fn mark_menu(remote: Option<(&str, HostMark, MarkSubject)>) -> MarkMenu {
    match remote {
        Some((host, mark, subject)) => MarkMenu {
            title: format!("Mark \u{201c}{}\u{201d} as", subject.name(host)),
            enabled: true,
            checked: MARKS.iter().position(|(_, candidate)| *candidate == mark),
        },
        None => MarkMenu {
            title: "Mark Host as".to_owned(),
            enabled: false,
            checked: None,
        },
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing condition; Drop is not implemented.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriShellMenuDelegate"]
    pub(crate) struct ShellMenuDelegate;

    unsafe impl NSObjectProtocol for ShellMenuDelegate {}

    unsafe impl NSMenuDelegate for ShellMenuDelegate {
        /// Shell is opening: Mark … as ▸ is built from the active tab's
        /// markable host — its remote host, else its database client's
        /// server — and Forget Password from its remote host alone (a
        /// database's password is not bateri's). Only on opening — the
        /// shortcut search does not come through here.
        #[unsafe(method(menuWillOpen:))]
        fn menu_will_open(&self, menu: &NSMenu) {
            let app = crate::app::delegate(self.mtm());
            if menu.title().to_string() == MOVE_TO_TAB_TITLE {
                let targets = app.as_ref().map(|app| app.key_move_targets());
                fill_move_targets(self.mtm(), menu, &targets.unwrap_or_default());
                return;
            }
            // Open Port ▸: the focused pane's listening ports, grey without
            // one — also while a full-screen program hides the dock.
            if let Some(holder) = menu.itemWithTag(PORTS_HOLDER_TAG) {
                let model = app.as_ref().map(|app| app.key_ports()).unwrap_or_default();
                holder.setEnabled(!model.is_empty());
                if let Some(submenu) = holder.submenu() {
                    crate::footer::fill_ports_menu(&submenu, &model, None);
                }
            }
            if let Some(forget) = menu.itemWithTag(FORGET_TAG) {
                let remote = app.as_ref().and_then(|app| app.key_remote_mark());
                let host = remote.as_ref().map(|(host, _)| host.as_str());
                forget.setTitle(&NSString::from_str(&forget_title(host)));
            }
            let Some(holder) = menu.itemWithTag(MARK_HOLDER_TAG) else {
                return;
            };
            let target = app.and_then(|app| app.key_mark_target());
            let model = mark_menu(
                target
                    .as_ref()
                    .map(|(host, mark, subject)| (host.as_str(), *mark, *subject)),
            );
            holder.setTitle(&NSString::from_str(&model.title));
            holder.setEnabled(model.enabled);
            if let Some(submenu) = holder.submenu() {
                for (index, item) in submenu.itemArray().iter().enumerate() {
                    item.setState(if model.checked == Some(index) {
                        NSControlStateValueOn
                    } else {
                        NSControlStateValueOff
                    });
                }
            }
        }
    }
);

/// Fills Window ▸ Move Split to Tab ▸ with the tabs a split can go to: one
/// item each, its `tag` the tab ([`crate::tabs::menu_tag`]) — the window's
/// `movePaneToTab:` reads it. A window with one tab lists a grey placeholder.
fn fill_move_targets(mtm: MainThreadMarker, menu: &NSMenu, targets: &[(u64, String)]) {
    menu.removeAllItems();
    for (id, title) in targets {
        let entry = item(mtm, title, sel!(movePaneToTab:), "");
        entry.setTag(crate::tabs::menu_tag(*id));
        menu.addItem(&entry);
    }
    if targets.is_empty() {
        // No tab tag: the window's validation greys it too, but this menu is
        // filled as it opens, so it is said here as well.
        let none = item(mtm, "No Other Tabs", sel!(movePaneToTab:), "");
        none.setEnabled(false);
        menu.addItem(&none);
    }
}

impl ShellMenuDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `NSObject`'s `init`; the subclass has no ivars.
        unsafe { msg_send![super(this), init] }
    }
}

/// Builds the menu bar. At the start of `applicationDidFinishLaunching:`, before the
/// window is brought to the front: the menu should be in place before the app
/// activates.
///
/// It is built in a timed run too: none of the items runs by itself, and the
/// items that touch the user (Settings…, Theme ▸) make the decision in their own
/// action and fill (`app::Inputs`).
///
/// `themes`: Theme ▸'s delegate — fills it with `menuNeedsUpdate:`.
/// `updater`: Sparkle's updater; if present the target of "Check for Updates…"
/// (the item holds it weakly, the caller keeps it alive).
/// The returned Shell menu's delegate; the delegate is a weak reference, the
/// caller holds it for the whole process.
pub(crate) fn install(
    mtm: MainThreadMarker,
    themes: &ProtocolObject<dyn NSMenuDelegate>,
    updater: Option<&AnyObject>,
    handover_test: bool,
) -> Retained<ShellMenuDelegate> {
    let command = NSEventModifierFlags::Command;
    let mut app_items = vec![item(
        mtm,
        "About bateri",
        sel!(orderFrontStandardAboutPanel:),
        "",
    )];
    // macOS's place: right under About (Sparkle's documentation points
    // there too).
    if let Some(updater) = updater {
        let check = item(mtm, "Check for Updates…", sel!(checkForUpdates:), "");
        // SAFETY: the target is `SPUStandardUpdaterController`; `checkForUpdates:` is
        // its documented, single-`id`-argument, no-return action. The item holds the
        // target weakly and the app delegate keeps the updater alive for the whole
        // process.
        unsafe { check.setTarget(Some(updater)) };
        app_items.push(check);
    }
    // The handover's test item: only with the defaults key
    // `BateriHandoverTestMenu` (the caller reads it); the update's quit
    // without Sparkle — the same path, then bateri starts itself again.
    if handover_test {
        app_items.push(item(
            mtm,
            "Relaunch with Handover",
            sel!(relaunchWithHandover:),
            "",
        ));
    }
    app_items.extend([
        NSMenuItem::separatorItem(mtm),
        item(mtm, "Settings…", sel!(openSettings:), ","),
        NSMenuItem::separatorItem(mtm),
        item(mtm, "Hide bateri", sel!(hide:), "h"),
        with_modifiers(
            item(mtm, "Hide Others", sel!(hideOtherApplications:), "h"),
            command | NSEventModifierFlags::Option,
        ),
        item(mtm, "Show All", sel!(unhideAllApplications:), ""),
        NSMenuItem::separatorItem(mtm),
        item(mtm, "Quit bateri", sel!(terminate:), "q"),
        end_programs_item(mtm),
    ]);
    let app_menu = submenu(mtm, "bateri", &app_items);
    // Edit ▸ Find: macOS's submenu and shortcuts.
    // The selectors are **our own names** — `performFindPanelAction:` would be
    // swallowed by AppKit's field editor while the field is focused; the handler is
    // `TerminalPane` (the field's ancestor, above the field in the responder chain).
    // `keyDown:`'s Cmd allow-list does not change: the menu catches the key first
    // (⌘A's precedent).
    let find_menu = submenu(
        mtm,
        "Find",
        &[
            item(mtm, "Find…", sel!(findInScrollback:), "f"),
            item(mtm, "Find Next", sel!(findNextMatch:), "g"),
            with_modifiers(
                item(mtm, "Find Previous", sel!(findPreviousMatch:), "g"),
                command | NSEventModifierFlags::Shift,
            ),
            item(
                mtm,
                "Use Selection for Find",
                sel!(useSelectionForFind:),
                "e",
            ),
        ],
    );
    let edit_menu = submenu(
        mtm,
        "Edit",
        &[
            // Not the standard `undo:`: `NSWindow` answers that one itself (through
            // its own, empty, undo manager) before the chain reaches the
            // application, and the item would be grey for good. The application
            // (`AppDelegate`'s `validateMenuItem:`) takes the last move of splits
            // back, and is grey with none or while a text field is being edited.
            item(mtm, "Undo", sel!(undoMove:), "z"),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Cut", sel!(cut:), "x"),
            item(mtm, "Copy", sel!(copy:), "c"),
            item(mtm, "Paste", sel!(paste:), "v"),
            with_modifiers(
                item(mtm, "Paste Escaped Text", sel!(pasteEscaped:), "v"),
                command | NSEventModifierFlags::Control,
            ),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Select All", sel!(selectAll:), "a"),
            NSMenuItem::separatorItem(mtm),
            // Terminal.app's place and shortcuts; the selectors are
            // our own names, the handler is `TerminalPane`.
            item(mtm, "Clear to Start", sel!(clearToStart:), "k"),
            with_modifiers(
                item(mtm, "Clear Scrollback", sel!(clearScrollback:), "k"),
                command | NSEventModifierFlags::Option,
            ),
            NSMenuItem::separatorItem(mtm),
            find_menu,
        ],
    );
    // Empty at the start: `fill_themes` builds the items on every opening.
    let theme_menu = submenu(mtm, "Theme", &[]);
    if let Some(menu) = theme_menu.submenu() {
        menu.setDelegate(Some(themes));
    }
    let view_menu = submenu(
        mtm,
        "View",
        &[
            theme_menu,
            NSMenuItem::separatorItem(mtm),
            // `+` is a Shift character: the menu matches Cmd-Shift-= and shows
            // "⌘+". `-` is an ASCII hyphen, not the minus sign (U+2212) — the keyboard does
            // not produce it.
            item(mtm, "Bigger", sel!(makeFontBigger:), "+"),
            item(mtm, "Smaller", sel!(makeFontSmaller:), "-"),
            item(mtm, "Actual Size", sel!(resetFontSize:), "0"),
            NSMenuItem::separatorItem(mtm),
            // AppKit's function-key code points (`NSHomeFunctionKey`,
            // `NSEndFunctionKey`, `NSPageUpFunctionKey`,
            // `NSPageDownFunctionKey`): the menu shows them like "⌘↖".
            item(mtm, "Scroll to Top", sel!(scrollToTop:), "\u{F729}"),
            item(mtm, "Scroll to Bottom", sel!(scrollToBottom:), "\u{F72B}"),
            item(mtm, "Page Up", sel!(scrollPageUp:), "\u{F72C}"),
            item(mtm, "Page Down", sel!(scrollPageDown:), "\u{F72D}"),
        ],
    );
    let shell_menu = submenu(
        mtm,
        "Shell",
        &[
            item(mtm, "New Window", sel!(newWindow:), "n"),
            item(mtm, "New Tab", sel!(newTab:), "t"),
            // On a remote tab ⌘T goes to the same host; this is always local.
            with_modifiers(
                item(mtm, "New Local Tab", sel!(newLocalTab:), "t"),
                command | NSEventModifierFlags::Option,
            ),
            NSMenuItem::separatorItem(mtm),
            // Its title and grey state on opening ([`ShellMenuDelegate`]); the items go to the
            // app delegate with `markHost:` (the active tab's host).
            mark_holder(mtm),
            // Whether a plain ssh to the tab's host sets up the shell
            // integration; handled by the app delegate
            // (`toggleHostIntegration:`), its title and state from its
            // `validateMenuItem:`. Takes effect from the next ssh.
            item(
                mtm,
                &integration_menu(None).title,
                sel!(toggleHostIntegration:),
                "",
            ),
            // The tab's saved ssh password: the handler is the focused
            // pane (`forgetPassword:`), grey without one (`validateMenuItem:`).
            {
                let forget = item(mtm, &forget_title(None), sel!(forgetPassword:), "");
                forget.setTag(FORGET_TAG);
                forget
            },
            // The whole queue of uploads to the remote directory; enabled only
            // while there is a queue (`TerminalPane`'s `validateMenuItem:`).
            item(mtm, "Cancel Upload", sel!(cancelUpload:), "."),
            // The focused pane's listening ports (`[shell] ports`): filled on
            // opening ([`ShellMenuDelegate`]); an item opens its address in the
            // browser, the focused pane handles it (`openPort:`).
            {
                let ports = submenu(mtm, "Open Port", &[]);
                ports.setTag(PORTS_HOLDER_TAG);
                ports
            },
            NSMenuItem::separatorItem(mtm),
            // Splits (Ghostty/iTerm2 precedent): the handler is
            // `TerminalWindow` (splits the focused pane); grey at the smallest pane
            // limit (`validateMenuItem:`).
            item(mtm, "Split Right", sel!(splitRight:), "d"),
            with_modifiers(
                item(mtm, "Split Down", sel!(splitDown:), "d"),
                command | NSEventModifierFlags::Shift,
            ),
            NSMenuItem::separatorItem(mtm),
            // Not `performClose:` (measured): after the red button's cancelled
            // group close AppKit broadcasts `performClose:` to the whole group, so ⌘W would
            // ask about the window instead of a tab. Its title is "Close" with many panes
            // (it closes the focused pane; `TerminalWindow`'s `validateMenuItem:`).
            item(mtm, "Close Tab", sel!(closeTab:), "w"),
            with_modifiers(
                item(mtm, "Close Window", sel!(closeWindow:), "w"),
                command | NSEventModifierFlags::Shift,
            ),
        ],
    );
    let mut select_tab: Vec<_> = (1..=8)
        .map(|n| {
            tagged(
                item(mtm, &format!("Tab {n}"), sel!(selectTab:), &n.to_string()),
                n,
            )
        })
        .collect();
    select_tab.push(tagged(item(mtm, "Last Tab", sel!(selectTab:), "9"), 9));
    // Direction items: `tag` is the order of `split::Direction::from_tag` (left,
    // right, up, down); the shortcut is AppKit's arrow-key code points
    // (`NSLeftArrowFunctionKey` U+F702, `NSRightArrowFunctionKey` U+F703,
    // `NSUpArrowFunctionKey` U+F700, `NSDownArrowFunctionKey` U+F701).
    let arrows = [
        ("Left", "\u{F702}"),
        ("Right", "\u{F703}"),
        ("Up", "\u{F700}"),
        ("Down", "\u{F701}"),
    ];
    let directed = |action, modifiers| -> Vec<_> {
        (0u8..)
            .zip(arrows)
            .map(|(tag, (title, key))| {
                tagged(
                    with_modifiers(item(mtm, title, action, key), modifiers),
                    tag,
                )
            })
            .collect()
    };
    let select_split = directed(sel!(selectSplit:), command | NSEventModifierFlags::Option);
    let resize_split = directed(sel!(resizeSplit:), command | NSEventModifierFlags::Control);
    let swap_split = directed(
        sel!(swapSplit:),
        command | NSEventModifierFlags::Option | NSEventModifierFlags::Shift,
    );
    // The tabs a split can go to are filled when the menu opens
    // ([`ShellMenuDelegate`]); the placeholder is what a menu that never
    // opened would show.
    let move_to_tab = submenu(
        mtm,
        MOVE_TO_TAB_TITLE,
        &[item(mtm, "No Other Tabs", sel!(movePaneToTab:), "")],
    );
    let window_menu = submenu(
        mtm,
        "Window",
        &[
            item(mtm, "Minimize", sel!(performMiniaturize:), "m"),
            item(mtm, "Zoom", sel!(performZoom:), ""),
            NSMenuItem::separatorItem(mtm),
            // `{`/`}` are Shift characters: the menu matches ⇧⌘[ / ⇧⌘] (the same idiom as
            // Bigger's `+`).
            item(mtm, "Show Previous Tab", sel!(showPreviousTab:), "{"),
            item(mtm, "Show Next Tab", sel!(showNextTab:), "}"),
            hidden_shortcut(with_modifiers(
                item(mtm, "Show Previous Tab", sel!(showPreviousTab:), "\t"),
                NSEventModifierFlags::Control | NSEventModifierFlags::Shift,
            )),
            hidden_shortcut(with_modifiers(
                item(mtm, "Show Next Tab", sel!(showNextTab:), "\t"),
                NSEventModifierFlags::Control,
            )),
            submenu(mtm, "Select Tab", &select_tab),
            // `|` is the Shift character of `\`: the menu matches ⇧⌘\. Grey with a single
            // tab (`TerminalWindow`'s `validateMenuItem:`).
            item(mtm, "Show All Tabs", sel!(showTabList:), "|"),
            item(mtm, "Rename Tab\u{2026}", sel!(renameTab:), ""),
            NSMenuItem::separatorItem(mtm),
            // Splits (Ghostty/iTerm2 precedent): the handler is
            // `TerminalWindow`; grey with a single pane (`validateMenuItem:`).
            item(
                mtm,
                "Select Previous Split",
                sel!(selectPreviousSplit:),
                "[",
            ),
            item(mtm, "Select Next Split", sel!(selectNextSplit:), "]"),
            submenu(mtm, "Select Split", &select_split),
            submenu(mtm, "Resize Split", &resize_split),
            submenu(mtm, "Swap Split", &swap_split),
            with_modifiers(
                item(mtm, "Equalize Splits", sel!(equalizeSplits:), "="),
                command | NSEventModifierFlags::Control,
            ),
            with_modifiers(
                item(mtm, "Zoom Split", sel!(toggleSplitZoom:), "\r"),
                command | NSEventModifierFlags::Shift,
            ),
            NSMenuItem::separatorItem(mtm),
            // A split leaves for a tab or a window (`TerminalWindow`'s
            // appliers); the shortcuts are Previous / Next Tab's with ⌥ —
            // `{`/`}` carry the Shift as there. Greyed by `validateMenuItem:`.
            item(mtm, "Move Split to New Tab", sel!(movePaneToNewTab:), ""),
            move_to_tab.clone(),
            item(
                mtm,
                "Move Split to New Window",
                sel!(movePaneToNewWindow:),
                "",
            ),
            with_modifiers(
                item(
                    mtm,
                    "Move Split to Previous Tab",
                    sel!(movePaneToPreviousTab:),
                    "{",
                ),
                command | NSEventModifierFlags::Option,
            ),
            with_modifiers(
                item(mtm, "Move Split to Next Tab", sel!(movePaneToNextTab:), "}"),
                command | NSEventModifierFlags::Option,
            ),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Move Tab to New Window", sel!(detachTab:), ""),
            item(mtm, "Merge All Windows", sel!(mergeWindows:), ""),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Bring All to Front", sel!(arrangeInFront:), ""),
        ],
    );
    let bar = NSMenu::new(mtm);
    bar.addItem(&app_menu);
    bar.addItem(&shell_menu);
    bar.addItem(&edit_menu);
    bar.addItem(&view_menu);
    bar.addItem(&window_menu);
    let app = NSApplication::sharedApplication(mtm);
    app.setMainMenu(Some(&bar));
    app.setWindowsMenu(window_menu.submenu().as_deref());
    let shell_delegate = ShellMenuDelegate::new(mtm);
    if let Some(menu) = shell_menu.submenu() {
        menu.setDelegate(Some(ProtocolObject::from_ref(&*shell_delegate)));
    }
    if let Some(menu) = move_to_tab.submenu() {
        menu.setDelegate(Some(ProtocolObject::from_ref(&*shell_delegate)));
    }
    shell_delegate
}

/// Builds Shell ▸ Mark … as ▸ from scratch: four items ([`MARKS`]), their `tag`s
/// their order. Initially the local tab's state; on opening [`ShellMenuDelegate`]
/// sets it.
fn mark_holder(mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    let items: Vec<_> = (0u8..)
        .zip(MARKS)
        .map(|(tag, (title, _))| tagged(item(mtm, title, sel!(markHost:), ""), tag))
        .collect();
    let model = mark_menu(None);
    let holder = submenu(mtm, &model.title, &items);
    holder.setTag(MARK_HOLDER_TAG);
    holder.setEnabled(model.enabled);
    holder
}

/// Builds Theme ▸ from scratch: "Match System", a separator, the embedded themes and
/// (if any) the user themes after a separator. `selected` is the `theme` value in
/// the settings; the matching item is checked.
///
/// The item's title is the theme's name and the action reads it (`selectTheme:`):
/// the title comes from the file name, it is not translated. "Match System" is a
/// separate action, because its title is not a name.
pub(crate) fn fill_themes(
    mtm: MainThreadMarker,
    menu: &NSMenu,
    selected: &str,
    embedded: &[&str],
    user: &[String],
) {
    menu.removeAllItems();
    let checked = |item: Retained<NSMenuItem>, on: bool| {
        if on {
            item.setState(NSControlStateValueOn);
        }
        item
    };
    menu.addItem(&checked(
        item(mtm, "Match System", sel!(matchSystemTheme:), ""),
        selected == SYSTEM_THEME,
    ));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    for name in embedded {
        menu.addItem(&checked(
            item(mtm, name, sel!(selectTheme:), ""),
            *name == selected,
        ));
    }
    if !user.is_empty() {
        menu.addItem(&NSMenuItem::separatorItem(mtm));
    }
    for name in user {
        menu.addItem(&checked(
            item(mtm, name, sel!(selectTheme:), ""),
            name == selected,
        ));
    }
}

/// A target-less item. If `key` is empty there is no shortcut; the modifier default is Command.
fn item(mtm: MainThreadMarker, title: &str, action: Sel, key: &str) -> Retained<NSMenuItem> {
    // SAFETY: `action` is a valid selector built with `sel!` and each of its
    // receivers (`BateriView`, `TerminalPane`, `TerminalWindow`, `NSWindow`,
    // `AppDelegate`, `NSApplication`) defines it as a no-return action with a single
    // `Option<&AnyObject>` argument.
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    }
}

/// Sets the item's shortcut modifiers to `modifiers`.
fn with_modifiers(
    item: Retained<NSMenuItem>,
    modifiers: NSEventModifierFlags,
) -> Retained<NSMenuItem> {
    item.setKeyEquivalentModifierMask(modifiers);
    item
}

/// An item that is invisible but whose shortcut **works**: it attaches a second
/// shortcut to the menu without showing the title twice (⌃⇥ / ⌃⇧⇥ — Safari's
/// idiom). A hidden item's shortcut is ignored by default; the flag turns it back
/// on.
fn hidden_shortcut(item: Retained<NSMenuItem>) -> Retained<NSMenuItem> {
    item.setHidden(true);
    item.setAllowsKeyEquivalentWhenHidden(true);
    item
}

/// Sets the item's `tag` to `tag` — Select Tab ▸'s order
/// (`tabs::tab_index`) and Mark … as ▸'s mark ([`mark_of_tag`]).
fn tagged(item: Retained<NSMenuItem>, tag: u8) -> Retained<NSMenuItem> {
    item.setTag(isize::from(tag));
    item
}

/// The menu carrying `items` and the item attaching it to the parent menu. The
/// item's title is `title` too: the menu bar shows the menu's title, a nested menu
/// shows the item's.
fn submenu(
    mtm: MainThreadMarker,
    title: &str,
    items: &[Retained<NSMenuItem>],
) -> Retained<NSMenuItem> {
    let title = NSString::from_str(title);
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &title);
    for item in items {
        menu.addItem(item);
    }
    let holder = NSMenuItem::new(mtm);
    holder.setTitle(&title);
    holder.setSubmenu(Some(&menu));
    holder
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::ClassType;
    use objc2_app_kit::NSWindow;

    /// Edit ▸ Undo Move has a selector of its own because `NSWindow` answers the
    /// standard `undo:` before the chain reaches the application (through its
    /// own, empty, undo manager: the item would be grey for good, and nothing
    /// shows it but a menu opened by hand); the application's delegate must
    /// answer the one the item carries.
    #[test]
    fn undo_move_is_answered_by_the_application_and_not_by_the_window() {
        assert!(NSWindow::class().responds_to(sel!(undo:)));
        assert!(!NSWindow::class().responds_to(sel!(undoMove:)));
        assert!(crate::app::AppDelegate::class().responds_to(sel!(undoMove:)));
    }

    #[test]
    fn quit_and_end_programs_shows_only_while_quit_keeps_the_programs() {
        assert!(shows_end_programs(KeepRunning::Quit));
        assert!(!shows_end_programs(KeepRunning::Crash));
        assert!(!shows_end_programs(KeepRunning::Update));
    }

    #[test]
    fn forget_password_names_the_host_without_its_user() {
        assert_eq!(
            forget_title(Some("deploy@prod-web")),
            "Forget Password for \u{201c}prod-web\u{201d}"
        );
        assert_eq!(forget_title(None), "Forget Password");
    }

    #[test]
    fn the_mark_menu_follows_the_active_tab() {
        // Local tab: the generic name and grey.
        assert_eq!(
            mark_menu(None),
            MarkMenu {
                title: "Mark Host as".to_owned(),
                enabled: false,
                checked: None,
            }
        );
        // Remote tab: the host without `user@` in the title, the checkmark on the effective resolution.
        assert_eq!(
            mark_menu(Some((
                "deploy@prod-web",
                HostMark::Staging,
                MarkSubject::Host
            ))),
            MarkMenu {
                title: "Mark \u{201c}prod-web\u{201d} as".to_owned(),
                enabled: true,
                checked: Some(1),
            }
        );
        // A Kubernetes context: its whole name, the one the menu writes.
        assert_eq!(
            mark_menu(Some((
                "kubernetes-admin@kubernetes",
                HostMark::None,
                MarkSubject::Whole
            )))
            .title,
            "Mark \u{201c}kubernetes-admin@kubernetes\u{201d} as"
        );
        // A host with no mark is checked on "None"; no direct color on any item.
        assert_eq!(
            mark_menu(Some(("vm", HostMark::None, MarkSubject::Host))).checked,
            Some(3)
        );
        assert_eq!(
            mark_menu(Some(("vm", HostMark::Rgb(0xc678dd), MarkSubject::Host))).checked,
            None
        );
    }

    #[test]
    fn the_integration_toggle_follows_the_resolution() {
        let settings = |text: &str| {
            bt_core::Settings::parse(text)
                .expect("parseable text")
                .settings
        };
        let model = |text: &str, host: &str| {
            integration_menu(Some((host, settings(text).integration_for(host))))
        };
        // Locally: grey, unchecked.
        assert_eq!(
            integration_menu(None),
            IntegrationMenu {
                title: "Shell Integration on Host".to_owned(),
                enabled: false,
                checked: false,
            }
        );
        // An unmarked host follows `[remote] integration` (on by default).
        assert_eq!(
            model("", "deploy@web"),
            IntegrationMenu {
                title: "Shell Integration on \u{201c}web\u{201d}".to_owned(),
                enabled: true,
                checked: true,
            }
        );
        assert!(!model("[remote]\nintegration = false\n", "web").checked);
        // A production mark turns it off — through a glob too —, an entry of the
        // host's own turns it back on.
        let prod = "[remote]\nhosts = [{ host = \"prod-*\", mark = \"production\" }]\n";
        assert!(!model(prod, "prod-web").checked);
        let edit = bt_core::SettingsEdit::RemoteHostIntegration {
            host: "prod-web".to_owned(),
            on: true,
        };
        let written = bt_core::Settings::with_edit(prod, &edit).expect("writable text");
        assert!(model(&written, "prod-web").checked);
        assert!(!model(&written, "prod-db").checked);
    }

    #[test]
    fn every_mark_item_reads_back_its_mark() {
        for (index, (_, mark)) in MARKS.iter().enumerate() {
            let tag = isize::try_from(index).expect("small index");
            assert_eq!(mark_of_tag(tag), Some(*mark));
            assert_eq!(
                mark_menu(Some(("h", *mark, MarkSubject::Host))).checked,
                Some(index)
            );
        }
        assert_eq!(mark_of_tag(-1), None);
        assert_eq!(mark_of_tag(4), None);
    }
}
