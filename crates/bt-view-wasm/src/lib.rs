//! `bt-view-wasm`: a page of the UI kit that lives in a sandbox.
//!
//! A page's logic can be a module — a WebAssembly component of the interface in `wit/view.wit`
//! — that the application runs on a thread of its own and drives with what the person does,
//! and that answers with the page to draw: a tree the engine ([`bt_view`]) lays out and paints
//! in the terminal's own font and theme. The module reaches nothing but that interface:
//! no file, no network, no clock, no environment. It is held to a memory limit and a time for
//! every call, and a module that goes past either, traps or hands out something that is not a
//! page is closed — the application is told why and goes on.
//!
//! This crate knows no window, no renderer and no terminal: it depends on the engine and on the
//! runtime, and moves out of the workspace with the engine.

mod module;
mod sandbox;
mod watchdog;
mod world;

#[cfg(test)]
mod guests_tests;

pub use module::{Answer, Frame, Module, Runner, Update};
pub use sandbox::{Fault, Limits};
