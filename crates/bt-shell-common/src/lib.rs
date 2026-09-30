//! bt-shell-common — the shell layer's platform-independent half.
//!
//! The modules here decide how the terminal application behaves without
//! touching a UI toolkit: the settings file and its diagnostics (`settings`,
//! `notices`), the split layout tree (`split`), temporary font size (`zoom`),
//! the mouse gesture ledger (`gesture`), shell quoting (`quote`), key
//! encoding (`keys`), the remote upload rules and processes (`upload`), the
//! process table (`jobs`), the shell's birth (`child`) and file watching
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
//! {bt-atlas, bt-core}`; nothing here depends upward (`make audit` checks it). The rationale is in
//! `.tasks/043-bt-shell-ayrimi/discussion.md` → Karar 1–2.

pub mod child;
pub mod gesture;
pub mod jobs;
pub mod keys;
pub mod notices;
pub mod quote;
pub mod settings;
pub mod split;
pub mod upload;
// Bodies exist for macOS and Linux only; elsewhere the module is absent
// rather than half-present (a `Notify` without a `Watch`).
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub mod watch;
pub mod zoom;
