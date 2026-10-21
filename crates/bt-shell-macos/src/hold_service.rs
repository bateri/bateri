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

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;

use crate::app::delegate;
use crate::{child, settings, ssh_route};
use bt_shell_common::kablo::install::Bundle;
use bt_shell_common::kablo::service::{PaneInfo, Peer, Service};
use bt_shell_common::kablo::ssh::SshBackend;

/// The running service: for as long as the process lives.
static SERVICE: OnceLock<Service> = OnceLock::new();

/// How long the main queue is waited for when a process asks which pane it is in:
/// a busy main thread is a request that is not served (the plain `ssh`).
const PANE_WAIT: Duration = Duration::from_secs(2);

/// Starts the service in `dir`, the instance's directory. Silent when it cannot
/// (no home, a path too long for a socket): the holders then find nobody and run
/// the plain `ssh`.
pub(crate) fn serve(dir: &std::path::Path, bases: Vec<std::path::PathBuf>) {
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
