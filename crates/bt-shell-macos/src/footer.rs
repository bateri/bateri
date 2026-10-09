//! The dock's context row on the pane's side: its controls and the listening
//! ports.
//!
//! **One way in for the row's controls.** The hand cursor's rectangles
//! ([`TerminalPane::footer_spans`]), the hover ([`TerminalPane::footer_hover`])
//! and the click ([`TerminalPane::footer_click`]) all ask `bt-core`'s one plan
//! of the row (`Session::footer_spans`/`footer_hit`/`footer_span`) — the
//! drawing's — and dispatch on what lies under the pointer. A new part of the
//! row is a new `FooterControl` arm here, not another path through the view.
//!
//! **The listening ports.** After an edge in the pane (output, a command's
//! start or end — `pane::PortProbe`, delayed, at most one waiting) the pane's
//! process tree is scanned on a queue of its own (`ports::pane_listeners`;
//! never on the main thread: a large tree is milliseconds), the result comes
//! back to the main queue, the dock gets the numbers (`Session::set_ports`)
//! and each listening process's exit is watched — a thread blocked on its exit
//! descriptor (`program::wait_for_exit`) — so a port goes with its server, with
//! no timer. The thread of a server that outlives the pane (`nohup`) waits for
//! that server.
//!
//! **In a remote pane the server's ports join them** (`crate::remote_ports`):
//! green when they open from this Mac, dim when they have to be forwarded
//! through the ssh connection first.
//!
//! **Their list is a menu**: at the ports on a click, and Shell ▸ Open Port ▸
//! always — also while a full-screen program or an agent hides the dock. An
//! item opens its address (a server's dim port is forwarded first), its ⌥
//! alternate copies the address; ⌘-click on a port opens it at once.

use std::sync::OnceLock;

use bt_core::{FooterControl, FooterPort};
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{NSEventModifierFlags, NSMenu, NSMenuItem, NSPasteboard, NSWorkspace};
use objc2_foundation::{NSPoint, NSString, NSURL, ns_string};

use crate::clipboard;
use crate::jobs::{self, SystemTable};
use crate::pane::TerminalPane;
use crate::ports::{self, Bound, Listener};
use crate::program;
use crate::remote_ports::{RemoteItem, remote_title};

/// The queue the scans run on — serial and shared by every pane: scans are
/// short and rare, one at a time is enough.
fn scan_queue() -> &'static DispatchRetained<DispatchQueue> {
    static QUEUE: OnceLock<DispatchRetained<DispatchQueue>> = OnceLock::new();
    QUEUE.get_or_init(|| DispatchQueue::new("dev.bateri.ports", None))
}

impl TerminalPane {
    /// The row's clickable parts and their dock-local ranges on a
    /// `context`-column row — the hand cursor's input.
    pub(crate) fn footer_spans(&self, context: u16) -> Vec<(FooterControl, u16, u16)> {
        self.session()
            .map(|session| session.footer_spans(context))
            .unwrap_or_default()
    }

    /// The pointer is at dock-local column `col` of the context row (`None` →
    /// off it; `context` is the row's budget): the part under it becomes the
    /// row's hover — a frame only on the edge. Over the row the hand cursor's
    /// rectangles are checked too: the row can change under a still pointer
    /// (a refresh, the band gliding).
    pub(crate) fn footer_hover(&self, at: Option<(u16, u16)>) {
        let Some(session) = self.session() else {
            return;
        };
        let hover = at
            .and_then(|(col, context)| session.footer_hit(context, col))
            .map(|hit| hit.control);
        session.set_footer_hover(hover);
        if at.is_some() {
            self.view().sync_cursor_rects();
        }
    }

    /// Recomputes the hover from the pointer's **current** place: the row
    /// changed under it (its buttons' widths follow the queue, a port came or
    /// went).
    pub(crate) fn rehover_footer(&self) {
        let at = self.view().pointer_context_column();
        self.footer_hover(at);
    }

    /// The window stopped being key: `mouseMoved:` no longer arrives, no part
    /// may hang in hover.
    pub(crate) fn unhover_footer(&self) {
        self.footer_hover(None);
    }

    /// A click at dock-local column `col` of the context row; `command` is ⌘.
    /// `true` if it landed on a part and did its work.
    pub(crate) fn footer_click(&self, col: u16, context: u16, command: bool) -> bool {
        let Some(session) = self.session() else {
            return false;
        };
        let Some(hit) = session.footer_hit(context, col) else {
            return false;
        };
        let span = session.footer_span(context, hit.control);
        match hit.control {
            FooterControl::Ports => match hit.port.filter(|_| command) {
                Some(port) => self.open_port_number(port),
                None => {
                    if let Some(span) = span {
                        self.pop_ports_menu(span);
                    }
                }
            },
            FooterControl::Stats => {
                if let Some(span) = span {
                    self.toggle_stats_popover(span);
                }
            }
            FooterControl::SignIn => self.sign_in(),
            FooterControl::List => self.toggle_upload_list(span),
            FooterControl::Cancel => self.request_stop(true),
        }
        true
    }

    /// `[shell] ports` changed: on, the next scan runs at once; off, the
    /// ports leave the dock and the menu.
    pub(crate) fn set_ports_shown(&self, on: bool) {
        self.port_probe().set_on(on);
        if on {
            self.poke_ports();
        } else {
            self.end_remote_ports();
            self.show_listeners(Vec::new());
        }
    }

    /// ⌘-click on a port: the tab's own listener if one has the number,
    /// otherwise the server's ([`TerminalPane::open_remote_port`]).
    fn open_port_number(&self, port: u16) {
        let local = self
            .listeners()
            .borrow()
            .iter()
            .any(|listener| listener.port == port);
        if local {
            open_port(port);
        } else {
            self.open_remote_port(port);
        }
    }

    /// The ports the dock shows: the tab's own (but not the ones our
    /// forwards listen on — those are the server's) and, in a remote pane,
    /// the server's.
    pub(crate) fn publish_ports(&self) {
        let (mut shown, forwarded) = {
            let remote = self.remote_ports().borrow();
            (remote.footer_ports(), remote.forwarded_locals())
        };
        shown.extend(
            self.listeners()
                .borrow()
                .iter()
                .filter(|listener| !forwarded.contains(&listener.port))
                .map(|listener| FooterPort {
                    port: listener.port,
                    open: true,
                }),
        );
        if let Some(session) = self.session() {
            session.set_ports(&shown);
        }
        // The ports take room the load indicator may have had: its popover
        // follows it, the hand cursor's rectangles too.
        self.stats_gauge_changed();
    }

    /// The ports menu's content: the tab's own listeners and the server's
    /// ports with the remote host's name.
    pub(crate) fn ports_model(&self) -> PortsModel {
        let (remote, forwarded) = {
            let state = self.remote_ports().borrow();
            (state.menu_items(), state.forwarded_locals())
        };
        let local = self
            .listeners()
            .borrow()
            .iter()
            .filter(|listener| !forwarded.contains(&listener.port))
            .cloned()
            .collect();
        let host = self
            .session()
            .and_then(|session| session.remote_target())
            // The host without `user@`: `db1:8080` reads as an address,
            // `root@db1:8080` as an scp path.
            .map(|(_, target, _)| bt_core::bare_host(&target.host).to_owned())
            .unwrap_or_default();
        PortsModel {
            local,
            remote,
            host,
        }
    }

    /// Scans the pane's process tree for listening ports on [`scan_queue`]
    /// and shows the answer back on the main queue. No shell yet, a dead
    /// reader or the probe turned off: nothing.
    pub(crate) fn scan_ports(&self) {
        let (Some(session), Some(parent)) = (self.session(), self.shell_parent()) else {
            return;
        };
        if !session.reader_alive() || !self.port_probe().is_on() {
            return;
        }
        // The server's ports ride the same edges, on their own throttle.
        self.request_remote_ports();
        let child = session.child_pid();
        let (id, lookup) = (self.id(), self.lookup());
        scan_queue().exec_async(move || {
            let found = ports::pane_listeners(parent, child, &SystemTable);
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(pane) = lookup(mtm, id)
                    && pane.port_probe().is_on()
                {
                    pane.show_listeners(found);
                }
            });
        });
    }

    /// Shows `found`: the numbers to the dock, the records to the menu, a
    /// watch on each new listening process's exit.
    fn show_listeners(&self, found: Vec<Listener>) {
        self.watch_exits(&found);
        self.listeners().replace(found);
        self.publish_ports();
    }

    /// One thread per listening process not yet watched: it waits for the
    /// process's exit and schedules a scan, so a port goes with its server.
    /// A process that is already gone schedules one at once.
    fn watch_exits(&self, found: &[Listener]) {
        let mut pids: Vec<u32> = found.iter().map(|listener| listener.pid).collect();
        pids.dedup();
        for pid in pids {
            let Some(start) = jobs::start_time(pid) else {
                continue;
            };
            if !self.watched().borrow_mut().insert((pid, start)) {
                continue;
            }
            let (id, lookup) = (self.id(), self.lookup());
            let spawned = std::thread::Builder::new()
                .name("port exit".into())
                .spawn(move || {
                    program::wait_for_exit(pid, Some(start));
                    DispatchQueue::main().exec_async(move || {
                        // audit: a block running on the main queue is on the main thread by definition.
                        let mtm =
                            MainThreadMarker::new().expect("the main queue is the main thread");
                        if let Some(pane) = lookup(mtm, id) {
                            pane.watched().borrow_mut().remove(&(pid, start));
                            pane.poke_ports();
                        }
                    });
                });
            // No thread: the port goes at the next edge instead.
            if spawned.is_err() {
                self.watched().borrow_mut().remove(&(pid, start));
            }
        }
    }

    /// The ports' menu over the ports' range (`(start, end)`, dock-local
    /// columns): its last item sits on the row, the rest above it — the dock
    /// is at the window's bottom.
    fn pop_ports_menu(&self, (start, end): (u16, u16)) {
        let model = self.ports_model();
        if model.is_empty() {
            return;
        }
        let Some(rect) = self.view().context_span_rect(start, end) else {
            return;
        };
        let menu = NSMenu::new(self.mtm());
        menu.setAutoenablesItems(false);
        fill_ports_menu(&menu, &model, Some(self));
        let last = menu.itemArray().lastObject();
        // The view is flipped: the range's origin is its top-left.
        let at = NSPoint::new(rect.origin.x, rect.origin.y);
        menu.popUpMenuPositioningItem_atLocation_inView(last.as_deref(), at, Some(self.view()));
    }

    /// An item's `openPort:` (the ports menu, Shell ▸ Open Port ▸): its `tag`
    /// is the port.
    pub(crate) fn open_port_sent(&self, sender: Option<&AnyObject>) {
        if let Some(port) = sender_port(sender) {
            open_port(port);
        }
    }

    /// An item's ⌥ alternate, `copyPortURL:`: the address to the pasteboard.
    pub(crate) fn copy_port_url_sent(&self, sender: Option<&AnyObject>) {
        if let Some(port) = sender_port(sender) {
            clipboard::copy(&NSPasteboard::generalPasteboard(), Some(ports::url(port)));
        }
    }
}

/// The port an `openPort:`/`copyPortURL:` item carries in its `tag`.
pub(crate) fn sender_port(sender: Option<&AnyObject>) -> Option<u16> {
    let item = sender?.downcast_ref::<NSMenuItem>()?;
    u16::try_from(item.tag()).ok().filter(|&port| port > 0)
}

/// Opens `http://localhost:{port}` in its default application.
fn open_port(port: u16) {
    open_url(&ports::url(port));
}

/// Opens an `http://` address in its default application.
pub(crate) fn open_url(url: &str) {
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

/// The ports menu's content ([`TerminalPane::ports_model`]).
#[derive(Clone, Debug, Default)]
pub(crate) struct PortsModel {
    pub(crate) local: Vec<Listener>,
    pub(crate) remote: Vec<RemoteItem>,
    /// The remote host as the tab names it; empty locally.
    pub(crate) host: String,
}

impl PortsModel {
    pub(crate) fn is_empty(&self) -> bool {
        self.local.is_empty() && self.remote.is_empty()
    }
}

/// An item's title: `localhost:3000 — next-server (v16.0.10) · all interfaces`.
pub(crate) fn port_title(listener: &Listener) -> String {
    let reach = match listener.bound {
        Bound::Any => "all interfaces",
        Bound::Loopback => "this Mac only",
        Bound::Address => "one address",
    };
    format!(
        "localhost:{} \u{2014} {} \u{b7} {reach}",
        listener.port, listener.name
    )
}

/// Fills `menu` with an item per port — `openPort:` (the tab's own) or
/// `openRemotePort:` (the server's) and, for one that opens as it is, a ⌥
/// alternate that copies the address; the port in the `tag`. With both kinds
/// a heading names each half ("On this Mac", "On {host}"). `target` is the
/// pane for the pop-up menu; the main menu's items have none and reach the
/// focused pane through the responder chain.
pub(crate) fn fill_ports_menu(menu: &NSMenu, model: &PortsModel, target: Option<&TerminalPane>) {
    let mtm = menu.mtm();
    menu.removeAllItems();
    let headed = !model.local.is_empty() && !model.remote.is_empty();
    let heading = |title: &str| {
        let item = NSMenuItem::new(mtm);
        item.setTitle(&NSString::from_str(title));
        item.setEnabled(false);
        menu.addItem(&item);
    };
    if headed {
        heading("On this Mac");
    }
    for listener in &model.local {
        let tag = isize::try_from(listener.port).unwrap_or(0);
        let open = action_item(mtm, &port_title(listener), sel!(openPort:), tag, target);
        menu.addItem(&open);
        let copy = action_item(
            mtm,
            &format!("Copy {}", listener.url()),
            sel!(copyPortURL:),
            tag,
            target,
        );
        copy.setKeyEquivalentModifierMask(NSEventModifierFlags::Option);
        copy.setAlternate(true);
        menu.addItem(&copy);
    }
    if headed {
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        heading(&format!("On {}", model.host));
    }
    for item in &model.remote {
        let tag = isize::try_from(item.port).unwrap_or(0);
        let open = action_item(
            mtm,
            &remote_title(&model.host, item),
            sel!(openRemotePort:),
            tag,
            target,
        );
        menu.addItem(&open);
        if let Some(url) = &item.url {
            let copy = action_item(
                mtm,
                &format!("Copy {url}"),
                sel!(copyRemotePortURL:),
                tag,
                target,
            );
            copy.setKeyEquivalentModifierMask(NSEventModifierFlags::Option);
            copy.setAlternate(true);
            menu.addItem(&copy);
        }
    }
}

/// A menu item that sends `action` with `tag`.
fn action_item(
    mtm: MainThreadMarker,
    title: &str,
    action: objc2::runtime::Sel,
    tag: isize,
    target: Option<&TerminalPane>,
) -> Retained<NSMenuItem> {
    // SAFETY: `initWithTitle:action:keyEquivalent:` takes two `NSString`s and a
    // selector the receiver implements (`TerminalPane`'s `openPort:` and
    // `copyPortURL:`); `setTarget:` keeps the target weakly and the pane
    // outlives the menu's modal tracking.
    let item = unsafe {
        let item = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            ns_string!(""),
        );
        if let Some(target) = target {
            item.setTarget(Some(target));
        }
        item
    };
    item.setTag(tag);
    item.setEnabled(true);
    item
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_names_the_port_the_program_and_the_reach() {
        let listener = Listener {
            port: 3000,
            bound: Bound::Any,
            pid: 7,
            name: "next-server (v16.0.10)".into(),
        };
        assert_eq!(
            port_title(&listener),
            "localhost:3000 \u{2014} next-server (v16.0.10) \u{b7} all interfaces"
        );
        assert_eq!(
            port_title(&Listener {
                bound: Bound::Loopback,
                ..listener
            }),
            "localhost:3000 \u{2014} next-server (v16.0.10) \u{b7} this Mac only"
        );
    }
}
