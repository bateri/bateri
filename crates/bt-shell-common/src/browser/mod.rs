//! The browser element's side that is the same on every platform: the helper that draws it, what
//! bateri installs and when (plan 034, Karar 1, 4, 5).
//!
//! The element is a page of Chromium, which runs in a helper of its own (`bateri-browser`, a hidden
//! application in a repository of its own), not in bateri. bateri finds it, checks it, starts it
//! with a key only that start knows and a loopback proxy that every request of it goes through,
//! and speaks to it on a socket. The pieces:
//!
//! - [`version`]: a helper's version, compared as numbers.
//! - [`notice`]: the release notice's offer, and the rules that decide whether it is taken: no
//!   downgrade, no protocol bateri does not speak, no older notice replayed.
//! - [`layout`]: where the installed helpers live, one folder per version, what is in use, what is
//!   deleted.
//! - [`profile`]: a profile's key (machine, application, name) and the folder and the identity
//!   the helper is given for it.
//! - [`proxy`]: the loopback proxy that is open before the helper starts, and its credentials.
//! - `helper` (macOS): the helper's life — started with the first browser, ended some time after
//!   the last one is closed, and what the browsers are told when it dies. It owns no operating
//!   system service of its own: starting a process and checking a signature are given to it.
//!
//! Downloading, unpacking and the signature are the platform shell's (`bt-shell-macos`).

#[cfg(target_os = "macos")]
pub mod helper;
pub mod layout;
pub mod notice;
pub mod profile;
pub mod proxy;
pub mod version;
