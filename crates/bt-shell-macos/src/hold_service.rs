//! The application's end of `bateri ssh-hold`: the listener that panes' programs
//! connect to, started beside the focus listener in this instance's directory.
//!
//! The service itself, the steps over ssh and the install are
//! `bt-shell-common::kablo` (`service`, `ssh`, `install`); this module gives
//! them the three things only the application has: **which pane a process is
//! in** (the pane whose pseudo-terminal is the process's controlling
//! terminal — the kernel's record, asked on the main queue where the panes
//! are), the **settings** as the file says them now, and **kablo's binaries**
//! as the package carries them (`Contents/Resources/kablo`; a build without the
//! package has no kablo calls and every `ssh` is the plain one).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;

use crate::app::delegate;
use crate::pane::{KeptLogin, Login};
use crate::{child, settings, ssh_route};
use bt_core::PaneUuid;
use bt_shell_common::kablo::install::Bundle;
use bt_shell_common::kablo::service::{PaneInfo, PaneLink, Peer, Service, Shown};
use bt_shell_common::kablo::ssh::SshBackend;

/// The running service: for as long as the process lives.
static SERVICE: OnceLock<Service> = OnceLock::new();

/// The terminal the pane shows, if it shows one a kablo server keeps (the
/// program in front of it included, as the server last said). `None` for a pane
/// that shows its own shell and, in a process without the service, for every pane.
pub(crate) fn shown(pane: &PaneUuid) -> Option<Shown> {
    SERVICE.get()?.shown(pane)
}

/// The user closed the pane: the terminal it shows ends on the server. Called
/// **before** the pane's program is signalled (the service reads the pane from
/// the holder, which the signal ends); a pane that shows nothing is nothing to
/// do. A quit, a crash and an update do not say it.
pub(crate) fn end(pane: &PaneUuid) {
    if let Some(service) = SERVICE.get() {
        service.end(pane);
    }
}

/// ⏎ in a pane that waits for its connection: the account of the terminal it
/// shows dials now instead of waiting out its pause.
pub(crate) fn retry(pane: &PaneUuid) {
    if let Some(service) = SERVICE.get() {
        service.retry(pane);
    }
}

/// The user logged in for `account` (its master connection is up): every
/// terminal on the server comes back.
pub(crate) fn retry_account(account: &str) {
    if let Some(service) = SERVICE.get() {
        service.retry_account(account);
    }
}

/// The computer woke up: the accounts waiting for their pause dial now, and the
/// ones that look connected are asked whether they are.
pub(crate) fn wake() {
    if let Some(service) = SERVICE.get() {
        service.wake();
    }
}

/// The accounts a login sheet is open for, and the pane whose sheet it is: one
/// sheet per account, whichever of its panes' ⏎ asked for it. The sheet logs in
/// for the server's every terminal, so a second sheet for the same account
/// would ask the same question twice.
static LOGINS: Mutex<Option<HashMap<String, PaneUuid>>> = Mutex::new(None);

/// `pane` wants the login sheet of `account`.
fn begin_login(account: &str, pane: &PaneUuid) -> Login {
    let mut logins = LOGINS.lock().unwrap_or_else(|e| e.into_inner());
    match logins
        .get_or_insert_with(HashMap::new)
        .entry(account.to_owned())
    {
        std::collections::hash_map::Entry::Occupied(open) => Login::Raise(open.get().clone()),
        std::collections::hash_map::Entry::Vacant(free) => {
            free.insert(pane.clone());
            Login::Open
        }
    }
}

/// The account's login ended (logged in, cancelled, failed): the next ⏎ opens
/// a sheet again.
fn end_login(account: &str) {
    let mut logins = LOGINS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(logins) = logins.as_mut() {
        logins.remove(account);
    }
}

/// The pane's view of the login arbitration ([`KeptLogin`]).
pub(crate) struct Gate;

impl KeptLogin for Gate {
    fn begin(&self, account: &str, pane: &PaneUuid) -> Login {
        begin_login(account, pane)
    }

    fn end(&self, account: &str) {
        end_login(account);
    }

    fn done(&self, account: &str) {
        retry_account(account);
    }

    fn raise(&self, pane: &PaneUuid) -> bool {
        MainThreadMarker::new()
            .and_then(delegate)
            .is_some_and(|delegate| delegate.raise_pane(pane))
    }
}

/// The bridge of the terminal `pane` shows went or came back: the pane on the
/// main queue, where the panes are. A pane that is gone has nothing to show.
fn link_changed(pane: &PaneUuid, link: PaneLink) {
    let pane = pane.clone();
    DispatchQueue::main().exec_async(move || {
        // audit: a block running on the main queue is on the main thread by definition.
        let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
        if let Some(pane) = delegate(mtm).and_then(|delegate| delegate.pane_with_uuid(&pane)) {
            pane.set_connection(link);
        }
    });
}

/// How long a folder waits for the `ssh` it is for.
const START_WAIT: Duration = Duration::from_secs(30);

/// The folders new panes ask their first terminal to start in: a pane opened
/// from a pane that shows a terminal on a server (⌘T, ⌘D) types the `ssh` line
/// and the terminal it asks for starts where the source was. Kept here and not
/// in the pane's environment because the one who asks (`ssh-hold`) never sees
/// what the shell function did with it; asked of by the pane, once, within
/// [`START_WAIT`] — a later `ssh` in the same pane is not the one it was for.
#[derive(Default)]
struct Starts(Vec<(u64, String, Instant)>);

impl Starts {
    fn note(&mut self, pane: u64, directory: String, now: Instant) {
        self.0.retain(|(owner, _, at)| {
            *owner != pane && now.saturating_duration_since(*at) < START_WAIT
        });
        self.0.push((pane, directory, now));
    }

    fn take(&mut self, pane: u64, now: Instant) -> Option<String> {
        self.0
            .retain(|(_, _, at)| now.saturating_duration_since(*at) < START_WAIT);
        let at = self.0.iter().position(|(owner, ..)| *owner == pane)?;
        Some(self.0.remove(at).1)
    }
}

static STARTS: Mutex<Starts> = Mutex::new(Starts(Vec::new()));

/// The new pane `pane`'s first terminal starts in `directory` on the server.
pub(crate) fn expect_directory(pane: u64, directory: String) {
    let mut starts = STARTS.lock().unwrap_or_else(|e| e.into_inner());
    starts.note(pane, directory, Instant::now());
}

/// The folder the pane's first terminal starts in, once.
pub(crate) fn take_directory(pane: u64) -> Option<String> {
    let mut starts = STARTS.lock().unwrap_or_else(|e| e.into_inner());
    starts.take(pane, Instant::now())
}

/// How long the main queue is waited for when a process asks which pane it is in:
/// a busy main thread is a request that is not served (the plain `ssh`).
const PANE_WAIT: Duration = Duration::from_secs(2);

/// Starts the service in `dir`, the instance's directory. Silent when it cannot
/// (no home, a path too long for a socket): the holders then find nobody and run
/// the plain `ssh`.
pub(crate) fn serve(
    dir: &std::path::Path,
    bases: Vec<std::path::PathBuf>,
    masters: Option<Arc<ssh_route::Masters>>,
) {
    let Some(home) = child::home() else {
        return;
    };
    let root = settings::config_root(&home);
    let settings_of = {
        let root = root.clone();
        move || settings::load(&root).at_launch().0
    };
    let history = {
        let settings_of = settings_of.clone();
        move || u32::try_from(settings_of().scrollback).ok()
    };
    let backend = SshBackend {
        ssh: "ssh".to_owned(),
        bases,
        state_path: crate::remote_hosts_path(&home),
        bundle: bundle(),
        settings: Box::new(settings_of),
        panes: Box::new(pane_of),
        runner: Box::new(ssh_route::SystemSsh),
        name: computer_name(),
        history: Box::new(history),
        masters,
        linked: Box::new(link_changed),
    };
    if let Ok(service) = Service::start(dir, Arc::new(backend)) {
        let _ = SERVICE.set(service);
    }
}

/// kablo's binaries in the package, beside the executable's `Resources`.
fn bundle() -> Option<Bundle> {
    let exe = std::env::current_exe().ok()?;
    let contents = exe.parent()?.parent()?;
    Bundle::open(&contents.join("Resources/kablo")).ok()
}

/// What this computer calls itself, as the servers list who is attached to a
/// terminal.
fn computer_name() -> Option<String> {
    let mut buffer = [0u8; 256];
    // SAFETY: the buffer is ours and its size goes to the call as is.
    let status = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
    if status != 0 {
        return None;
    }
    let end = buffer.iter().position(|&b| b == 0)?;
    let name = String::from_utf8_lossy(&buffer[..end]).into_owned();
    let name = name.strip_suffix(".local").unwrap_or(&name).to_owned();
    (!name.is_empty()).then_some(name)
}

/// The pane a process is in: one hop to the main queue, where the panes are.
/// `None` for a process in no pane's terminal, and for a main thread too busy
/// to say.
fn pane_of(peer: &Peer) -> Option<PaneInfo> {
    let pid = peer.pid;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    DispatchQueue::main().exec_async(move || {
        // audit: a block running on the main queue is on the main thread by definition.
        let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
        let _ = sender.send(delegate(mtm).and_then(|delegate| delegate.pane_of_process(pid)));
    });
    receiver.recv_timeout(PANE_WAIT).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_folder_is_taken_once_by_the_pane_it_is_for() {
        let now = Instant::now();
        let mut starts = Starts::default();
        starts.note(7, "/srv/app".into(), now);
        starts.note(8, "/var/log".into(), now);
        assert_eq!(starts.take(9, now), None, "another pane's");
        assert_eq!(starts.take(7, now).as_deref(), Some("/srv/app"));
        assert_eq!(starts.take(7, now), None, "once");
        assert_eq!(starts.take(8, now).as_deref(), Some("/var/log"));
    }

    #[test]
    fn one_login_sheet_is_open_per_account_and_the_next_enter_raises_it() {
        let first = PaneUuid::parse("0f3c1a52-7d0e-4c1b-9b57-2f7a8c4d6e10").expect("a uuid");
        let second = PaneUuid::parse("5a7e9c40-1b2d-4e3f-8a6b-0c9d1e2f3a4b").expect("a uuid");
        assert_eq!(begin_login("test-one@rasp:22", &first), Login::Open);
        // The same account from another pane: the sheet that is open.
        assert_eq!(
            begin_login("test-one@rasp:22", &second),
            Login::Raise(first.clone())
        );
        // Another account (`ssh root@rasp`) is its own login.
        assert_eq!(begin_login("test-root@rasp:22", &second), Login::Open);
        // Cancelled or done: ⏎ opens it again, on whichever pane.
        end_login("test-one@rasp:22");
        assert_eq!(begin_login("test-one@rasp:22", &second), Login::Open);
        end_login("test-one@rasp:22");
        end_login("test-root@rasp:22");
    }

    #[test]
    fn a_start_folder_that_waited_too_long_is_not_for_the_next_ssh() {
        let now = Instant::now();
        let mut starts = Starts::default();
        starts.note(7, "/srv/app".into(), now);
        assert_eq!(starts.take(7, now + START_WAIT), None);
        // A newer note for the pane replaces the older.
        starts.note(7, "/a".into(), now);
        starts.note(7, "/b".into(), now);
        assert_eq!(starts.take(7, now).as_deref(), Some("/b"));
        assert_eq!(starts.take(7, now), None);
    }
}
