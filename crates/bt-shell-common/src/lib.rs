//! bt-shell-common — the shell layer's platform-independent half.
//!
//! The modules here decide how the terminal application behaves without
//! touching a UI toolkit: the settings file and its diagnostics (`settings`,
//! `notices`), the split layout tree (`split`), temporary font size (`zoom`),
//! the mouse gesture ledger (`gesture`), shell quoting (`quote`), key
//! encoding (`keys`), what a ⌘-clicked link resolves to and what opening it
//! does (`links`), the remote upload rules, processes and the two-way transfer
//! queue (`upload`), the download's stream (`download`), the rules of
//! previewing and downloading a remote file (`remote_files`), its helper
//! ssh session (`remote_helper`), the preview cache on disk
//! (`preview_cache`), the remote host's load sampling (`remote_stats`), which
//! ssh connection a remote job rides on and the askpass wire (`ssh_route`), whether
//! the user's `ssh` gets the remote shell integration (`ssh_wrap`), what an
//! outside process may ask about one pane's focus (`focus`), the update's
//! live handover and its holder process (`handover`), a pane's journal in
//! shared memory and `bateri compact` (`journal`), the saved
//! session layout and its file (`restore`), the
//! process table (`jobs`), which program reads the keyboard and its guide
//! bar (`program`), the shell's birth (`child`) and file watching
//! (`watch`).
//!
//! **The boundary:** no AppKit, Foundation, Quartz or notification centre —
//! whatever needs the toolkit stays in the platform shell (`bt-shell-macos` on
//! macOS). Where a module needs an operating system service, its body is
//! behind `cfg(target_os = …)` and named as such (`jobs::SystemTable`, `watch`);
//! the only macOS-specific dependency is `dispatch2`, for `watch`'s vnode
//! sources. Values the toolkit reads (the system's language and region) come
//! in as arguments (`child::locale_env`).
//!
//! **Layer:** `bateri → bt-shell-{macos,linux} → bt-shell-common → bt-gpu →
//! {bt-atlas, bt-core}`; nothing here depends upward (`make audit` checks it).

pub mod child;
pub mod download;
pub mod focus;
pub mod gesture;
pub mod handover;
pub mod jobs;
pub mod journal;
pub mod keys;
pub mod links;
pub mod notices;
pub mod preview_cache;
pub mod program;
pub mod quote;
pub mod remote_files;
pub mod remote_helper;
pub mod remote_stats;
pub mod restore;
pub mod settings;
pub mod split;
pub mod ssh_route;
pub mod ssh_wrap;
pub mod upload;
// Bodies exist for macOS and Linux only; elsewhere the module is absent
// rather than half-present (a `Notify` without a `Watch`).
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub mod watch;
pub mod zoom;
