//! The pane's half of the **server's load**: the seat it holds in its server's measuring
//! ([`crate::server_stats`]) and the events that move it.
//!
//! - **The load is the server's, not the pane's.** However many panes show terminals of one
//!   server, it is measured once ([`bt_shell_common::remote_stats::Account`]) and drawn on all
//!   of them; a pane here only says when it joins (its ssh logged in), when it leaves, whether it
//!   is on screen, that it was touched, and which card is open.
//! - **The events** come from the pane: the remote edge (a generation started or ended),
//!   visibility (`SplitView::apply_visibility`), the settings, interaction (the view's keys,
//!   presses, wheel and mouse moves, the window becoming key), the sign-in and the close.
//! - **The door** (`⇄ rasp` is clickable) is not here: the remote edge tells the session
//!   ([`TerminalPane::sync_server_door`]), from the same gate as everything bateri does on a
//!   server ([`TerminalPane::server_target`]).
//! - **Zero frames when idle**: no timer wakes the link; a frame is asked only by a value that
//!   changed ([`crate::server_stats`]).

use std::rc::Rc;

use bt_core::RemoteStatsSettings;
use bt_shell_common::remote_helper::Reply;

use crate::focus::Moment;
use crate::pane::TerminalPane;
use crate::remote_helper::{Query, Request};
use crate::server_stats;

/// The pane's seat in its server's measuring.
#[derive(Debug)]
pub(crate) struct StatsSeat {
    /// The server's account and the generation of the pane's ssh while the pane is a member of
    /// that server's measuring; `None` before its ssh has logged in and after it ended.
    joined: Option<(Rc<str>, u64)>,
    /// The pane is on screen (not a background tab, a minimised window, behind a zoomed split).
    visible: bool,
    /// The settings' value: from the birth package, refreshed live
    /// ([`TerminalPane::set_stats_settings`]).
    settings: RemoteStatsSettings,
}

impl StatsSeat {
    /// From the settings' snapshot at the pane's birth (`PaneLaunch`).
    pub(crate) fn new(settings: RemoteStatsSettings) -> Self {
        Self {
            joined: None,
            visible: true,
            settings,
        }
    }

    /// The server the pane is a member of.
    pub(crate) fn account(&self) -> Option<Rc<str>> {
        self.joined.as_ref().map(|(account, _)| Rc::clone(account))
    }

    /// The settings' value.
    pub(crate) fn settings(&self) -> RemoteStatsSettings {
        self.settings
    }
}

/// The sampling half of the pane.
impl TerminalPane {
    /// The remote edge and the user's login: the pane is a member of its server's measuring
    /// **once the user's ssh has logged in** (or not at all). The first pane of a server takes
    /// the first sample at once; a later one joins and is given the value already there; the
    /// last to leave ends it (the value itself `Session` already dropped on `C`/`D`/`A`).
    /// Before the login the load does not connect — the login probe calls this again
    /// ([`TerminalPane::login_check`]).
    pub(crate) fn sync_stats_generation(&self) {
        let wanted = self
            .session()
            .and_then(|session| {
                let (command, ..) = self.server_target()?;
                (crate::jobs::remote_login(session) == Some(command)).then_some(command)
            })
            .zip(
                self.host()
                    .kept_terminal(self.uuid())
                    .map(|shown| Rc::<str>::from(shown.account)),
            );
        let (leaving, joining, visible, settings) = {
            let mut seat = self.stats_seat().borrow_mut();
            let before = seat.joined.clone();
            let after = wanted.map(|(command, account)| (account, command));
            if before == after {
                (None, None, seat.visible, seat.settings)
            } else {
                seat.joined = after.clone();
                let left = before.filter(|(account, _)| {
                    after
                        .as_ref()
                        .is_none_or(|(account_after, _)| account_after != account)
                });
                (
                    left.map(|(account, _)| account),
                    after,
                    seat.visible,
                    seat.settings,
                )
            }
        };
        if let Some(account) = leaving {
            server_stats::leave(&account, self.id());
        }
        if let Some((account, command)) = joining {
            server_stats::join(self, &account, command, visible, &settings);
        }
        self.stats_gauge_changed();
        // The server's ports start with the login and end with the session.
        self.request_remote_ports();
    }

    /// The pane became visible or covered (a background tab, a minimised
    /// window, behind a zoomed split — `SplitView::apply_visibility`).
    pub(crate) fn set_visible(&self, visible: bool) {
        let account = {
            let mut seat = self.stats_seat().borrow_mut();
            seat.visible = visible;
            seat.account()
        };
        if let Some(account) = account {
            server_stats::set_visible(&account, self.id(), visible);
        }
        // The same answer stops the scroll bar's block index.
        self.set_seen(visible);
    }

    /// A key, a press, the wheel, a mouse move or the window becoming key.
    /// Called at mouse-move rate: while sampling runs it only stamps the time
    /// (no allocation, no `Term` lock).
    pub(crate) fn note_interaction(&self) {
        // The focus query's `idle`: the same moments, its own clock.
        self.input_stamp().set(Moment::now());
        let account = self.stats_seat().borrow().account();
        if let Some(account) = account {
            server_stats::interaction(&account);
        }
    }

    /// `[remote] stats`/`stats_interval` changed (`AppDelegate::reload_settings`, once per
    /// pane). `off` hides the indicator and stops the measuring; another form redraws the last
    /// value at once; a new interval holds from the next tick.
    pub(crate) fn set_stats_settings(&self, settings: &RemoteStatsSettings) {
        self.stats_seat().borrow_mut().settings = *settings;
        server_stats::set_settings(settings);
        self.stats_gauge_changed();
        self.refresh_server_card();
    }

    /// The server card opened or closed ([`crate::server_card`]): while it is open every
    /// request asks for the details, and opening asks at once.
    pub(crate) fn set_stats_detail(&self, open: bool) {
        let account = self.stats_seat().borrow().account();
        if let Some(account) = account {
            server_stats::set_card(&account, self.id(), open);
        }
    }

    /// The user signed in: a run that ended on a failed open samples again at once.
    pub(crate) fn retry_stats(&self) {
        let account = self.stats_seat().borrow().account();
        if let Some(account) = account {
            server_stats::retry(&account);
        }
    }

    /// The pane closes: it leaves its server's measuring. The armed tick goes stale and the
    /// pane lookup would not find a closed pane anyway.
    pub(crate) fn stop_stats(&self) {
        let account = self.stats_seat().borrow_mut().joined.take();
        if let Some((account, _)) = account {
            server_stats::leave(&account, self.id());
        }
    }

    /// A load request on this pane's helper session, as the server's carrier. `command` is the
    /// generation of the ssh the member was registered with: the pane answers an error when its
    /// session has moved on, which the schedule counts as a failure of the run.
    pub(crate) fn ask_load(&self, command: u64, detail: bool, reply: Reply) {
        let target = self
            .server_target()
            .filter(|(current, ..)| *current == command);
        let Some((command, target, _)) = target else {
            reply(Err("The remote session ended".to_owned()), &[]);
            return;
        };
        self.remote_helper().borrow_mut().ask(Request {
            command,
            host: target.host.clone(),
            // A background job: rides a live master or today's argv, never asks.
            dial: self.dial(target, None),
            query: Query::Load { detail },
            reply,
        });
    }
}
