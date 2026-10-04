//! bateri's end of the **bound holder** (`[terminal] keep_running`): the
//! process that keeps a copy of every pane's PTY master while bateri lives,
//! so the programs outlive a crash (`"crash"`) or a quit (`"quit"`).
//!
//! The holder itself is `bt-shell-common`'s ([`handover::spawn_bound`],
//! [`Bound`]); this module drives it from the application: it is spawned at
//! launch and on a live switch away from `"update"`, every pane registers
//! when its session is born ([`Keeper::add`]) and releases at the head of its
//! close ([`Keeper::release`] — before the session's own hang-up: a program
//! with unread output cannot finish exiting while an unread copy of its
//! master is open), the layout goes on its edges behind one delayed trigger
//! ([`Keeper::layout_changed`]) and at once when a pane registers, the panes'
//! `bt-core` state at the shell's edges and, delayed, when the dock's mirror
//! moves (the pane's side), and each journaled pane's bases as its compaction
//! makes them (the pane's journal, through [`Keeper::add`]'s sink). A holder
//! that dies while bound is replaced and everything registered again, a
//! bounded number of times; past that, and on the switch to `"update"`, the
//! panes' journals break — no holder confirms their bases any more.
//!
//! Main thread only: the panes hold it in an `Rc`. The reader threads see
//! only [`Keeper::active_flag`], so in `"update"` (no holder) a shell's
//! edges and keystrokes post nothing.
//!
//! The decisions are pure functions beside it ([`wants_holder`],
//! [`quit_path`], [`spawns_for_quit`], [`pings_for_quit`], [`switch`],
//! [`may_respawn`], [`asks_notification_permission`]).

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bt_core::{KeepRunning, TabId};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::MainThreadMarker;

use crate::handover::{self, Bound, BoundPane};
use crate::journal::BaseSink;
use crate::ssh_route::Masters;

/// How many times one run replaces a holder that died while bound. A holder
/// dies for a reason (it was killed, or it crashes on something in what it
/// is sent): a fault that repeats would otherwise respawn it in a loop, each
/// spawn costing a process and a registration of every pane. Three covers a
/// stray kill or two; past it the run goes on unprotected and says so.
pub(crate) const RESPAWN_LIMIT: u32 = 3;

/// The layout's delay after an edge (a window, a tab, a split, the focus, a
/// directory, a close). Edges come in bursts — a split moves the focus, a
/// restore opens many tabs, `cd` retitles — and one second folds a burst into
/// one layout while a crash in that second loses at most the window
/// arrangement of that second; a new pane's layout does not wait
/// ([`Keeper::add`]).
pub(crate) const LAYOUT_DELAY: Duration = Duration::from_secs(1);

/// The delay of a pane's state after its dock mirror moved. The mirror moves
/// with every keystroke and its copy only serves the dock after a crash, which
/// marks a carried mirror stale anyway: one copy per second while typing
/// costs one blob encode a second and loses at most the last second's typing.
/// The shell's edges (a prompt, a command's start and end) do not wait — a
/// state behind them would miss a command.
pub(crate) const MIRROR_DELAY: Duration = Duration::from_secs(1);

/// How long ⌘Q waits for the holder's answer before it skips the question:
/// an idle holder answers in a scheduling round, and one that does not
/// within half a second is not one to trust the programs to.
pub(crate) const PING_WAIT: Duration = Duration::from_millis(500);

/// Whether a run keeps a bound holder: `"crash"` and `"quit"` only, in an
/// interactive run (a timed run keeps nothing — its tokens must not move) of
/// a bundled process (the holder's bundle is named by the layout).
pub(crate) fn wants_holder(keep: KeepRunning, timed: bool, bundled: bool) -> bool {
    keep != KeepRunning::Update && !timed && bundled
}

/// What a quit does with the running programs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuitPath {
    /// The panes are frozen and handed to the living bound holder over its
    /// connection; nothing is asked.
    ToBound,
    /// The update's holder is spawned at this moment and given the frozen
    /// panes; nothing is asked unless it cannot be born.
    ToUpdateHolder,
    /// Today's quit: the bound holder (if any) leaves quietly first, then the
    /// panes close — after today's question.
    Close,
}

/// Which quit this is — the one input of the quit's path besides the setting
/// and the holder's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuitKind {
    /// ⌘Q, the red buttons' last window, Dock ▸ Quit, logging out.
    Quit,
    /// bateri ▸ Quit and End Programs (⌥⌘Q, shown under `"quit"`): the
    /// programs end whatever the setting says, after today's question.
    EndPrograms,
    /// Sparkle's relaunch: it never ends the programs — it wins over an
    /// ⌥⌘Q that happened to be pending.
    Relaunch,
}

impl QuitKind {
    /// The kind from Sparkle's relaunch flag and the ⌥⌘Q flag.
    pub(crate) fn of(relaunch: bool, end_programs: bool) -> QuitKind {
        match (relaunch, end_programs) {
            (true, _) => QuitKind::Relaunch,
            (false, true) => QuitKind::EndPrograms,
            (false, false) => QuitKind::Quit,
        }
    }
}

/// The quit's path from the setting, the kind of quit, and whether a bound
/// holder answered ([`Keeper::verified`]). A relaunch never ends the
/// programs: `"crash"` and `"quit"` hand them to the holder they already
/// have, `"update"` (or a run whose holder is gone) to the update's. ⌘Q
/// keeps them only in `"quit"` and only with a holder that answered —
/// programs must not end without the question because a holder was assumed.
/// ⌥⌘Q always closes.
pub(crate) fn quit_path(keep: KeepRunning, kind: QuitKind, bound: bool) -> QuitPath {
    match (kind, keep, bound) {
        (QuitKind::Relaunch, KeepRunning::Crash | KeepRunning::Quit, true) => QuitPath::ToBound,
        (QuitKind::Relaunch, _, _) => QuitPath::ToUpdateHolder,
        (QuitKind::Quit, KeepRunning::Quit, true) => QuitPath::ToBound,
        (QuitKind::Quit | QuitKind::EndPrograms, _, _) => QuitPath::Close,
    }
}

/// Whether ⌘Q spawns a holder before choosing its path: `"quit"` without a
/// holder that answered — it died, or never started — gets one now, so the
/// question is skipped only for programs that are actually kept. ⌥⌘Q spawns
/// nothing: the holder would only be told to leave.
pub(crate) fn spawns_for_quit(keep: KeepRunning, kind: QuitKind, bound: bool) -> bool {
    keep == KeepRunning::Quit && kind == QuitKind::Quit && !bound
}

/// Whether the quit asks the bound holder for an answer first — a ping is a
/// wait on the main thread, so only where the answer can change the path: a
/// relaunch, or ⌘Q under `"quit"`.
pub(crate) fn pings_for_quit(keep: KeepRunning, kind: QuitKind) -> bool {
    match kind {
        QuitKind::Relaunch => true,
        QuitKind::Quit => keep == KeepRunning::Quit,
        QuitKind::EndPrograms => false,
    }
}

/// Whether the permission to notify is asked for now: when `keep_running`
/// becomes `"quit"` — `before` is the value it had, `None` at launch. ⌘Q's
/// reminder needs the permission and its own moment (quitting) must not ask,
/// so the question comes right after the user chose the value, or at the
/// launch that finds it, while bateri is in front. The system asks once and
/// answers from its record afterwards.
pub(crate) fn asks_notification_permission(before: Option<KeepRunning>, now: KeepRunning) -> bool {
    now == KeepRunning::Quit && before != Some(KeepRunning::Quit)
}

/// What a live change of `keep_running` does to the holder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Switch {
    /// Away from `"update"`: spawn it and register every live pane.
    Spawn,
    /// To `"update"`: it leaves quietly — the programs stay with bateri.
    Leave,
    /// `"crash"` ↔ `"quit"` (or no change): only ⌘Q reads the difference.
    Stay,
}

pub(crate) fn switch(old: KeepRunning, new: KeepRunning) -> Switch {
    match (old == KeepRunning::Update, new == KeepRunning::Update) {
        (true, false) => Switch::Spawn,
        (false, true) => Switch::Leave,
        _ => Switch::Stay,
    }
}

/// Whether a holder that died while bound is replaced, after `respawns`
/// replacements this run ([`RESPAWN_LIMIT`]).
pub(crate) fn may_respawn(respawns: u32) -> bool {
    respawns < RESPAWN_LIMIT
}

/// The current layout as the frame carries it ([`handover::layout_blob`]):
/// the owner's — the live windows, read on the main thread. `None` if it
/// cannot be built (no bundle identifier).
pub(crate) type LayoutSource = fn(MainThreadMarker) -> Option<Vec<u8>>;

/// The bound holder's driver ([module](self)).
pub(crate) struct Keeper {
    exe: PathBuf,
    /// The instance directories come from here, prepared at the first spawn:
    /// the holder is born into directories this process owns.
    masters: Arc<Masters>,
    layout: LayoutSource,
    bound: RefCell<Option<Bound>>,
    /// The bound holder's start time (`jobs::start_time`): what makes ending
    /// it by pid safe ([`HolderId::kill`]).
    holder_start: Cell<Option<u64>>,
    /// Counts the spawns: a death reported for an earlier holder is not this
    /// one's.
    generation: Cell<u64>,
    respawns: Cell<u32>,
    /// Whether a holder is bound — the reader threads' gate.
    active: Arc<AtomicBool>,
    /// The panes the current holder has (a state or a release for another
    /// is dropped).
    registered: RefCell<Vec<TabId>>,
    /// The panes registered as taken from another holder, until that holder
    /// is acknowledged ([`Keeper::confirm_taken`]).
    unconfirmed: RefCell<Vec<TabId>>,
    /// The layout last sent: an edge that changed nothing sends nothing.
    sent_layout: RefCell<Option<Vec<u8>>>,
    /// A delayed layout job is in the main queue (at most one).
    layout_pending: Cell<bool>,
}

impl Keeper {
    pub(crate) fn new(exe: PathBuf, masters: Arc<Masters>, layout: LayoutSource) -> Keeper {
        Keeper {
            exe,
            masters,
            layout,
            bound: RefCell::new(None),
            holder_start: Cell::new(None),
            generation: Cell::new(0),
            respawns: Cell::new(0),
            active: Arc::new(AtomicBool::new(false)),
            registered: RefCell::new(Vec::new()),
            unconfirmed: RefCell::new(Vec::new()),
            sent_layout: RefCell::new(None),
            layout_pending: Cell::new(false),
        }
    }

    /// The flag the panes' reader threads read: `true` while a holder is
    /// bound.
    pub(crate) fn active_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.active)
    }

    pub(crate) fn is_active(&self) -> bool {
        self.bound.borrow().is_some()
    }

    /// Spawns a bound holder with the current layout; `false` (and a line
    /// on stderr) if none runs — the programs are then not protected. The
    /// caller registers the live panes after it.
    pub(crate) fn spawn(&self, mtm: MainThreadMarker) -> bool {
        if self.is_active() {
            return true;
        }
        let Some(layout) = (self.layout)(mtm) else {
            return false;
        };
        let dirs = self.masters.bases().to_vec();
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let on_death = move || {
            // The handle's reader thread, possibly the one that drops it:
            // one hop to the main queue, the handle untouched here.
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(app) = crate::app::delegate(mtm) {
                    app.holder_died(generation);
                }
            });
        };
        match handover::spawn_bound(&self.exe, &dirs, layout.clone(), on_death) {
            Ok(bound) => {
                self.holder_start.set(crate::jobs::start_time(bound.pid()));
                self.bound.replace(Some(bound));
                self.registered.borrow_mut().clear();
                self.unconfirmed.borrow_mut().clear();
                self.sent_layout.replace(Some(layout));
                self.active.store(true, Ordering::Release);
                true
            }
            Err(error) => {
                eprintln!("bateri: the holder could not start ({error}); programs end with bateri");
                false
            }
        }
    }

    /// The holder of `generation` died while bound: `true` if it was the
    /// current one and a replacement may be spawned ([`may_respawn`]) — the
    /// count goes up; `false` otherwise (the run goes on unprotected once the
    /// limit is spent, said on stderr).
    pub(crate) fn died(&self, generation: u64) -> bool {
        if generation != self.generation.get() {
            return false;
        }
        let dead = self
            .bound
            .borrow()
            .as_ref()
            .is_some_and(|bound| !bound.alive());
        if !dead {
            return false;
        }
        self.forget();
        if !may_respawn(self.respawns.get()) {
            eprintln!("bateri: the holder keeps dying; programs end with bateri from now on");
            return false;
        }
        self.respawns.set(self.respawns.get() + 1);
        true
    }

    /// Registers a pane, then sends the layout at once: a crash before the
    /// delayed one would carry a program no window places, and the next
    /// bateri would release it. Returns where the pane's later journal bases
    /// go ([`Bound::sink`]); `None` without a holder.
    pub(crate) fn add(&self, mtm: MainThreadMarker, pane: BoundPane) -> Option<Box<dyn BaseSink>> {
        let sink = {
            let bound = self.bound.borrow();
            let bound = bound.as_ref()?;
            let tab = pane.tab.clone();
            if pane.taken_from.is_some() {
                self.unconfirmed.borrow_mut().push(tab.clone());
            }
            let sink = bound.sink(&tab);
            let mut registered = self.registered.borrow_mut();
            if !registered.contains(&tab) {
                registered.push(tab);
            }
            bound.add(pane);
            sink
        };
        self.send_layout(mtm);
        Some(sink)
    }

    /// The pane closes: the holder lets its copy go.
    pub(crate) fn release(&self, tab: &TabId) {
        let mut registered = self.registered.borrow_mut();
        let Some(index) = registered.iter().position(|known| known == tab) else {
            return;
        };
        registered.remove(index);
        self.unconfirmed.borrow_mut().retain(|known| known != tab);
        if let Some(bound) = self.bound.borrow().as_ref() {
            bound.release(tab);
        }
    }

    /// A registered pane's newer `bt-core` state.
    pub(crate) fn state(&self, tab: &TabId, blob: Vec<u8>) {
        if !self.registered.borrow().contains(tab) {
            return;
        }
        if let Some(bound) = self.bound.borrow().as_ref() {
            bound.state(tab, blob);
        }
    }

    /// The holders the panes were taken from are acknowledged: the panes
    /// registered from them are this holder's to drain from now on.
    pub(crate) fn confirm_taken(&self) {
        let taken = std::mem::take(&mut *self.unconfirmed.borrow_mut());
        if let Some(bound) = self.bound.borrow().as_ref() {
            for tab in &taken {
                bound.confirm(tab);
            }
        }
    }

    /// A layout edge: the layout goes after [`LAYOUT_DELAY`], once per
    /// burst. Nothing while no holder is bound.
    pub(crate) fn layout_changed(&self) {
        if !self.is_active() || self.layout_pending.replace(true) {
            return;
        }
        let Ok(when) = DispatchTime::try_from(LAYOUT_DELAY) else {
            self.layout_pending.set(false);
            return;
        };
        let _ = DispatchQueue::main().after(when, || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            let Some(app) = crate::app::delegate(mtm) else {
                return;
            };
            if let Some(keeper) = app.keeper() {
                keeper.layout_pending.set(false);
                keeper.send_layout(mtm);
            }
        });
    }

    /// The current layout, if it differs from the last sent.
    fn send_layout(&self, mtm: MainThreadMarker) {
        if !self.is_active() {
            return;
        }
        let Some(layout) = (self.layout)(mtm) else {
            return;
        };
        if self.sent_layout.borrow().as_ref() == Some(&layout) {
            return;
        }
        if let Some(bound) = self.bound.borrow().as_ref() {
            bound.layout(layout.clone());
        }
        self.sent_layout.replace(Some(layout));
    }

    /// Whether a holder is bound **and** answers within [`PING_WAIT`].
    pub(crate) fn verified(&self) -> bool {
        self.bound
            .borrow()
            .as_ref()
            .is_some_and(|bound| bound.alive() && bound.ping(PING_WAIT))
    }

    /// The holder leaves without touching a program, and this waits for it
    /// (at most [`handover::HAND_WAIT`] once its queue is written): ⌘Q's way,
    /// before the panes close — a close that hangs must not leave a copy of
    /// a master held anywhere.
    pub(crate) fn quit(&self) {
        if let Some(bound) = self.forget() {
            bound.quit();
        }
    }

    /// [`Keeper::quit`] without waiting, on a thread of its own: the live
    /// switch to `"update"` must not hold the main thread on a slow holder.
    pub(crate) fn leave(&self) {
        if let Some(bound) = self.forget() {
            let _ = std::thread::Builder::new()
                .name("bateri-holder-leave".to_owned())
                .spawn(move || bound.quit());
        }
    }

    /// The holder for a deliberate handover ([`Bound::hand_over`]), with
    /// what ending it needs if the handover fails; this side forgets it.
    /// `None` if none is bound.
    pub(crate) fn take(&self) -> Option<(Bound, HolderId)> {
        let start = self.holder_start.take();
        self.forget().map(|bound| {
            let id = HolderId {
                pid: bound.pid(),
                start,
            };
            (bound, id)
        })
    }

    /// Lets go of a holder that did not answer ([`Keeper::verified`]): a
    /// dead one is dropped, one still connected but silent is **ended** —
    /// asked to leave, it might never read the request before this process
    /// exits, take the closed connection for a crash and keep copies of the
    /// same programs as the holder that replaces it. Its copies going costs
    /// nothing: bateri still holds every master.
    pub(crate) fn discard(&self) {
        if let Some((bound, id)) = self.take()
            && bound.alive()
        {
            id.kill();
        }
    }

    /// Lets the holder go from this side's books.
    fn forget(&self) -> Option<Bound> {
        self.active.store(false, Ordering::Release);
        self.registered.borrow_mut().clear();
        self.unconfirmed.borrow_mut().clear();
        self.sent_layout.replace(None);
        self.bound.take()
    }
}

/// Which process a holder is: its pid and its start time at the spawn.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HolderId {
    pid: u32,
    start: Option<u64>,
}

impl HolderId {
    /// Ends the holder (`SIGKILL`) — only while the pid is still the
    /// process that was spawned (the same start time): a holder is this
    /// process's child, but once reaped its pid may be another's. Its copies
    /// close with it; a program whose master this process holds lives on.
    pub(crate) fn kill(self) {
        let Some(start) = self.start else {
            return;
        };
        if crate::jobs::start_time(self.pid) != Some(start) {
            return;
        }
        let Ok(pid) = libc::pid_t::try_from(self.pid) else {
            return;
        };
        // SAFETY: `kill` has no memory preconditions; the pid is the holder
        // this process spawned, checked by its start time just above.
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALUES: [KeepRunning; 3] = [KeepRunning::Update, KeepRunning::Crash, KeepRunning::Quit];

    #[test]
    fn only_an_interactive_bundled_crash_or_quit_keeps_a_holder() {
        assert!(!wants_holder(KeepRunning::Update, false, true));
        assert!(wants_holder(KeepRunning::Crash, false, true));
        assert!(wants_holder(KeepRunning::Quit, false, true));
        for keep in VALUES {
            assert!(!wants_holder(keep, true, true), "timed run {keep:?}");
            assert!(!wants_holder(keep, false, false), "unbundled {keep:?}");
        }
    }

    #[test]
    fn command_q_keeps_the_programs_only_in_quit_and_only_with_an_answer() {
        use QuitKind::Quit as CommandQ;
        // ⌘Q (no relaunch): the question unless `quit` has a holder that
        // answered.
        assert_eq!(
            quit_path(KeepRunning::Update, CommandQ, false),
            QuitPath::Close
        );
        assert_eq!(
            quit_path(KeepRunning::Crash, CommandQ, false),
            QuitPath::Close
        );
        assert_eq!(
            quit_path(KeepRunning::Crash, CommandQ, true),
            QuitPath::Close
        );
        assert_eq!(
            quit_path(KeepRunning::Quit, CommandQ, true),
            QuitPath::ToBound
        );
        // `quit` with its holder dead: today's quit, never a silent end.
        assert_eq!(
            quit_path(KeepRunning::Quit, CommandQ, false),
            QuitPath::Close
        );
    }

    #[test]
    fn quit_and_end_programs_always_closes_and_never_spawns_or_pings() {
        for keep in VALUES {
            for bound in [false, true] {
                assert_eq!(
                    quit_path(keep, QuitKind::EndPrograms, bound),
                    QuitPath::Close,
                    "{keep:?} {bound}"
                );
                assert!(!spawns_for_quit(keep, QuitKind::EndPrograms, bound));
            }
            assert!(!pings_for_quit(keep, QuitKind::EndPrograms), "{keep:?}");
        }
    }

    #[test]
    fn the_kind_of_quit_lets_a_relaunch_win() {
        assert_eq!(QuitKind::of(false, false), QuitKind::Quit);
        assert_eq!(QuitKind::of(false, true), QuitKind::EndPrograms);
        assert_eq!(QuitKind::of(true, false), QuitKind::Relaunch);
        // An update never ends the programs, even with ⌥⌘Q pending.
        assert_eq!(QuitKind::of(true, true), QuitKind::Relaunch);
    }

    #[test]
    fn only_a_relaunch_or_command_q_in_quit_pings() {
        for keep in VALUES {
            assert!(pings_for_quit(keep, QuitKind::Relaunch), "{keep:?}");
        }
        assert!(pings_for_quit(KeepRunning::Quit, QuitKind::Quit));
        assert!(!pings_for_quit(KeepRunning::Crash, QuitKind::Quit));
        assert!(!pings_for_quit(KeepRunning::Update, QuitKind::Quit));
    }

    #[test]
    fn a_relaunch_never_ends_the_programs() {
        use QuitKind::Relaunch;
        assert_eq!(
            quit_path(KeepRunning::Update, Relaunch, false),
            QuitPath::ToUpdateHolder
        );
        // `update` never has a bound holder; if one answered all the same the
        // update's own path stands.
        assert_eq!(
            quit_path(KeepRunning::Update, Relaunch, true),
            QuitPath::ToUpdateHolder
        );
        for keep in [KeepRunning::Crash, KeepRunning::Quit] {
            assert_eq!(
                quit_path(keep, Relaunch, true),
                QuitPath::ToBound,
                "{keep:?}"
            );
            // The holder is gone: the update's own holder takes them.
            assert_eq!(
                quit_path(keep, Relaunch, false),
                QuitPath::ToUpdateHolder,
                "{keep:?}"
            );
        }
    }

    #[test]
    fn only_command_q_in_quit_without_an_answer_spawns_a_holder() {
        assert!(spawns_for_quit(KeepRunning::Quit, QuitKind::Quit, false));
        assert!(!spawns_for_quit(KeepRunning::Quit, QuitKind::Quit, true));
        assert!(!spawns_for_quit(
            KeepRunning::Quit,
            QuitKind::Relaunch,
            false
        ));
        for keep in [KeepRunning::Update, KeepRunning::Crash] {
            for kind in [QuitKind::Quit, QuitKind::EndPrograms, QuitKind::Relaunch] {
                for bound in [false, true] {
                    assert!(!spawns_for_quit(keep, kind, bound), "{keep:?}");
                }
            }
        }
        // After the spawn the same question answers `ToBound`: the holder
        // that was just born answered its handshake.
        assert_eq!(
            quit_path(KeepRunning::Quit, QuitKind::Quit, true),
            QuitPath::ToBound
        );
    }

    #[test]
    fn the_live_switch_spawns_or_dismisses_the_holder_only_across_update() {
        use KeepRunning::{Crash, Quit, Update};
        assert_eq!(switch(Update, Crash), Switch::Spawn);
        assert_eq!(switch(Update, Quit), Switch::Spawn);
        assert_eq!(switch(Crash, Update), Switch::Leave);
        assert_eq!(switch(Quit, Update), Switch::Leave);
        assert_eq!(switch(Crash, Quit), Switch::Stay);
        assert_eq!(switch(Quit, Crash), Switch::Stay);
        for keep in VALUES {
            assert_eq!(switch(keep, keep), Switch::Stay, "{keep:?}");
        }
    }

    #[test]
    fn the_permission_is_asked_only_on_becoming_quit() {
        use KeepRunning::{Crash, Quit, Update};
        // At launch: only a value that is already `quit`.
        assert!(asks_notification_permission(None, Quit));
        assert!(!asks_notification_permission(None, Crash));
        assert!(!asks_notification_permission(None, Update));
        // On a save: only the switch to `quit`, not a save that keeps it.
        assert!(asks_notification_permission(Some(Update), Quit));
        assert!(asks_notification_permission(Some(Crash), Quit));
        assert!(!asks_notification_permission(Some(Quit), Quit));
        for before in VALUES {
            for now in [Update, Crash] {
                assert!(!asks_notification_permission(Some(before), now));
            }
        }
    }

    #[test]
    fn a_dying_holder_is_replaced_a_bounded_number_of_times() {
        let allowed = (0..10).take_while(|&count| may_respawn(count)).count();
        assert_eq!(allowed, RESPAWN_LIMIT as usize);
        assert!(!may_respawn(RESPAWN_LIMIT));
    }
}
