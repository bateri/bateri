//! A server's load, **measured once** however many panes show it: the registry of
//! [`Account`]s, one per server, and the binding of their actions to the main queue and
//! the panes' helper sessions.
//!
//! - **The decision is pure and elsewhere** (`bt-shell-common::remote_stats::Account`): which
//!   panes are members, when a sample is taken, what a sample means. Here only the binding — a
//!   tick is a `dispatch` `after` carrying a **token** (`after` cannot be cancelled, a stale
//!   token is ignored), a request is a [`Query::Load`] on **one** pane's helper session (the
//!   carrier: a pane that is on screen if there is one), the reply comes back to the main queue
//!   with the account and the run it belongs to, and the one sample is drawn on every member
//!   pane (`Session::set_remote_stats` gates it by that pane's own ssh and by equality: a value
//!   that rounds to the one shown costs no frame).
//! - **The events** come from the panes ([`crate::stats`]): a pane's ssh logging in or ending,
//!   its visibility, its interactions, the settings, and the card opening ([`set_card`]).
//! - **Zero frames when idle**: no timer wakes the link; a frame is asked only by a changed
//!   value, and the schedule's stop conditions are the pane-level ones (a covered or untouched
//!   server is not sampled).
//!
//! Main thread only: the registry is a `thread_local`, and nothing in it is held while a pane
//! is called.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use bt_core::RemoteStatsSettings;
use bt_shell_common::remote_helper::{Answer, LoadReply, Reply as HelperReply};
use bt_shell_common::remote_stats::{Account, Action, Detail, Member, Reply};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::MainThreadMarker;

use crate::pane::{PaneLookup, TerminalPane};

/// One server in the registry: its account, and the pane whose helper session the request in
/// flight rides (the one an unreachable server is told to).
struct Slot {
    account: Account,
    carrier: Option<u64>,
}

#[derive(Default)]
struct Registry {
    slots: HashMap<String, Slot>,
    /// How to find a member pane again (the panes are `Retained` by their tabs, not here).
    lookups: HashMap<u64, PaneLookup>,
    /// The settings' value of the last pane that told: the form of the accounts to come.
    settings: Option<RemoteStatsSettings>,
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
}

fn with<R>(edit: impl FnOnce(&mut Registry) -> R) -> R {
    REGISTRY.with(|registry| edit(&mut registry.borrow_mut()))
}

/// `stats_interval` as a duration.
fn interval(settings: &RemoteStatsSettings) -> Duration {
    Duration::from_secs(u64::from(settings.interval))
}

/// A pane shows a terminal of `account` and its ssh (`command`) has logged in: it is drawn the
/// server's load from now on. The first pane of a server starts its measuring; a later one joins
/// it and is given the value already there.
pub(crate) fn join(
    pane: &TerminalPane,
    account: &str,
    command: u64,
    visible: bool,
    settings: &RemoteStatsSettings,
) {
    let (actions, shown) = with(|registry| {
        registry.lookups.insert(pane.id(), pane.lookup());
        registry.settings = Some(*settings);
        let slot = registry
            .slots
            .entry(account.to_owned())
            .or_insert_with(|| Slot {
                account: Account::new(settings.mode.form(), interval(settings)),
                carrier: None,
            });
        let member = Member {
            pane: pane.id(),
            command,
            visible,
        };
        let actions = slot.account.join(Instant::now(), member);
        (actions, slot.account.shown())
    });
    // A pane that joins a run in progress is given its value at once.
    if let (Some(stats), Some(session)) = (shown, pane.session()) {
        session.set_remote_stats(command, Some(&stats));
    }
    run(account, actions);
}

/// The pane no longer shows a terminal of `account` (closed, its terminal went, its ssh ended).
pub(crate) fn leave(account: &str, pane: u64) {
    let actions = with(|registry| {
        registry.lookups.remove(&pane);
        let slot = registry.slots.get_mut(account)?;
        let actions = slot.account.leave(Instant::now(), pane);
        if slot.account.members().is_empty() {
            registry.slots.remove(account);
        }
        Some(actions)
    });
    if let Some(actions) = actions {
        run(account, actions);
    }
}

/// The pane became visible or covered.
pub(crate) fn set_visible(account: &str, pane: u64, visible: bool) {
    let actions = with(|registry| {
        let slot = registry.slots.get_mut(account)?;
        Some(slot.account.set_visible(Instant::now(), pane, visible))
    });
    if let Some(actions) = actions {
        run(account, actions);
    }
}

/// A key, a press, the wheel, a mouse move or the window becoming key in a pane of the server.
/// Called at mouse-move rate: while sampling runs it only stamps the time.
pub(crate) fn interaction(account: &str) {
    let actions = with(|registry| {
        let slot = registry.slots.get_mut(account)?;
        Some(slot.account.interaction(Instant::now()))
    });
    if let Some(actions) = actions.filter(|actions| !actions.is_empty()) {
        run(account, actions);
    }
}

/// The card of `pane` opened or closed: while one is open every request asks for the details,
/// and opening asks at once.
pub(crate) fn set_card(account: &str, pane: u64, open: bool) {
    let actions = with(|registry| {
        let slot = registry.slots.get_mut(account)?;
        Some(slot.account.set_card(Instant::now(), pane, open))
    });
    if let Some(actions) = actions {
        run(account, actions);
    }
}

/// The login changed (Forget Password): a run that ended on a failed open samples again at once.
pub(crate) fn retry(account: &str) {
    let actions = with(|registry| {
        let slot = registry.slots.get_mut(account)?;
        Some(slot.account.retry(Instant::now()))
    });
    if let Some(actions) = actions {
        run(account, actions);
    }
}

/// `[remote] stats`/`stats_interval` changed (`AppDelegate::reload_settings`, once per pane).
/// `off` hides the value everywhere and stops the measuring; another form redraws the last
/// value at once; a new interval holds from the next tick.
pub(crate) fn set_settings(settings: &RemoteStatsSettings) {
    let form = settings.mode.form();
    let accounts: Vec<String> = with(|registry| {
        registry.settings = Some(*settings);
        registry.slots.keys().cloned().collect()
    });
    for account in accounts {
        let update = with(|registry| {
            let slot = registry.slots.get_mut(&account)?;
            Some(
                slot.account
                    .set_form(Instant::now(), form, interval(settings)),
            )
        });
        let Some(update) = update else {
            continue;
        };
        if let Some(stats) = update.redraw {
            draw(&account, Some(&stats));
        }
        run(&account, update.actions);
    }
}

/// The last sample's details — the card's content.
pub(crate) fn detail(account: &str) -> Option<Detail> {
    with(|registry| registry.slots.get(account)?.account.detail().cloned())
}

/// Runs the account's actions: the platform half of the schedule.
fn run(account: &str, actions: Vec<Action>) {
    for action in actions {
        match action {
            Action::Arm { token, after } => arm(account, token, after),
            Action::Request { detail, .. } => request(account, detail),
            Action::Hide => draw(account, None),
        }
    }
}

/// A tick after `after`, by account; the schedule ignores a stale token.
fn arm(account: &str, token: u64, after: Duration) {
    let Ok(when) = DispatchTime::try_from(after) else {
        return;
    };
    let account = account.to_owned();
    // The error arm is not represented today (the link clock's rationale); if it dropped,
    // sampling would resume at the next interaction.
    let _ = DispatchQueue::main().after(when, move || {
        let actions = with(|registry| {
            let slot = registry.slots.get_mut(&account)?;
            Some(slot.account.tick(Instant::now(), token))
        });
        if let Some(actions) = actions {
            run(&account, actions);
        }
    });
}

/// A load request on the carrier's helper session. The reply always comes back on the main queue
/// — also when there is no pane to ask (every member went, the remote edge has not reached the
/// schedule yet): the schedule's one request in flight must be answered, otherwise it would wait
/// forever.
fn request(account: &str, detail: bool) {
    let target = with(|registry| {
        let slot = registry.slots.get_mut(account)?;
        let epoch = slot.account.generation()?;
        let carrier = slot.account.carrier();
        slot.carrier = carrier.map(|member| member.pane);
        let lookup = carrier.and_then(|member| registry.lookups.get(&member.pane).copied());
        Some((epoch, carrier, lookup))
    });
    let Some((epoch, carrier, lookup)) = target else {
        return;
    };
    let key = account.to_owned();
    let reply: HelperReply = Box::new(move |answer: Result<Answer, String>, _: &[String]| {
        DispatchQueue::main().exec_async(move || answered(&key, epoch, answer));
    });
    // audit: the pane's methods run on the main thread; so does the caller.
    let mtm = MainThreadMarker::new().expect("the schedule runs on the main thread");
    let pane = carrier
        .zip(lookup)
        .and_then(|(member, lookup)| Some((member, lookup(mtm, member.pane)?)));
    match pane {
        Some((member, pane)) => pane.ask_load(member.command, detail, reply),
        None => reply(Err("The remote session ended".to_owned()), &[]),
    }
}

/// A load reply of run `epoch`, on the main queue. A sample is drawn on every member pane; an
/// unreachable server is told to the pane whose helper asked.
fn answered(account: &str, epoch: u64, answer: Result<Answer, String>) {
    let reply = reply_of(&answer);
    let result = with(|registry| {
        let slot = registry.slots.get_mut(account)?;
        let answered = slot.account.answered(Instant::now(), epoch, reply);
        Some((
            answered,
            slot.account.members().to_vec(),
            registry.lookups.clone(),
        ))
    });
    let Some((answered, members, lookups)) = result else {
        return;
    };
    // audit: a block running on the main queue is on the main thread by definition.
    let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
    // A server nobody is logged in for (`Unreachable`) shows nothing: no load, and no
    // button — the connection's login is the pane's own business (`Needs your password`).
    if let Some(stats) = answered.shown {
        for member in &members {
            let Some(pane) = lookups
                .get(&member.pane)
                .and_then(|lookup| lookup(mtm, member.pane))
            else {
                continue;
            };
            if let Some(session) = pane.session() {
                session.set_remote_stats(member.command, Some(&stats));
            }
            pane.refresh_server_card();
            // Every sample, not only a changed one: the name's rectangle also moves with the
            // window and the hand cursor must follow.
            pane.stats_gauge_changed();
        }
    }
    run(account, answered.actions);
}

/// Draws `stats` (`None`: nothing) on every member pane of the account.
fn draw(account: &str, stats: Option<&bt_core::RemoteStats>) {
    let (members, lookups) = with(|registry| {
        let members = registry
            .slots
            .get(account)
            .map(|slot| slot.account.members().to_vec())
            .unwrap_or_default();
        (members, registry.lookups.clone())
    });
    // audit: called from the main thread (the schedule's actions, the settings).
    let mtm = MainThreadMarker::new().expect("the registry is used on the main thread");
    for member in members {
        let Some(pane) = lookups
            .get(&member.pane)
            .and_then(|lookup| lookup(mtm, member.pane))
        else {
            continue;
        };
        if let Some(session) = pane.session() {
            session.set_remote_stats(member.command, stats);
        }
        pane.refresh_server_card();
        pane.stats_gauge_changed();
    }
}

/// A load answer → what the account needs. Another answer cannot come for a load request; it
/// counts as a failure.
fn reply_of(answer: &Result<Answer, String>) -> Reply<'_> {
    match answer {
        Ok(Answer::Load(LoadReply::Sample(sample))) => Reply::Sample(sample),
        Ok(Answer::Load(LoadReply::NoProc)) => Reply::NoProc,
        Ok(Answer::Load(LoadReply::Unreachable(_))) => Reply::Unreachable,
        Ok(_) | Err(_) => Reply::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_failure_of_a_load_request_is_named_for_the_schedule() {
        // A server nobody is logged in for is an unreachable one and nothing more: the
        // schedule hides the load, and no pane is told to offer anything.
        for text in ["no", bt_shell_common::ssh_route::SIGN_IN_NEEDED] {
            let unreachable = Ok(Answer::Load(LoadReply::Unreachable(text.to_owned())));
            assert!(matches!(reply_of(&unreachable), Reply::Unreachable));
        }
        assert!(matches!(
            reply_of(&Ok(Answer::Load(LoadReply::NoProc))),
            Reply::NoProc
        ));
        assert!(matches!(reply_of(&Err("closed".to_owned())), Reply::Failed));
        assert!(matches!(
            reply_of(&Ok(Answer::Counted(None))),
            Reply::Failed
        ));
        let sample = Ok(Answer::Load(LoadReply::Sample(Default::default())));
        assert!(matches!(reply_of(&sample), Reply::Sample(_)));
    }

    #[test]
    fn a_new_account_is_born_with_the_settings_it_is_asked_for() {
        // The interval is the settings' seconds.
        let settings = RemoteStatsSettings {
            interval: 9,
            ..RemoteStatsSettings::default()
        };
        assert_eq!(interval(&settings), Duration::from_secs(9));
    }
}
