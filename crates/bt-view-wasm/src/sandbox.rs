//! What a module is held to: how much memory it may have, how long a call of it may take,
//! how large a page it may hand out — and what is said when it goes past any of them.
//!
//! The module gets **nothing it was not given**: the linker holds the page's own interface and
//! no other (no clock, no file, no network, no environment, no way to print), so a module that
//! asks for anything else does not start. What it is given it is given on terms: a store that
//! refuses to grow past [`Limits::memory`], a call that is cut off after [`Limits::call`]
//! (the module's code is compiled to check for it at every loop head and function entry), and a
//! page that is read with its size, depth and strings counted.

use std::sync::Arc;
use std::time::Duration;

use bt_view::Laid;
use wasmtime::{Config, Engine, ResourceLimiter, Store, Trap};

use crate::watchdog;

/// What a module is held to.
#[derive(Clone, Debug)]
pub struct Limits {
    /// Bytes of memory and of tables the module may hold in all.
    pub memory: usize,
    /// How long one call (a frame, a settle, an event) may take.
    pub call: Duration,
    /// How long the module may take to start (its first code to run).
    pub start: Duration,
    /// Elements in one frame.
    pub nodes: usize,
    /// Depth of the tree of one frame (the root is 1).
    pub depth: usize,
    /// Bytes of one line of text, of a title, of text for the pasteboard.
    pub text: usize,
    /// Tracks of a grid's columns or rows.
    pub tracks: usize,
    /// Values of a sparkline.
    pub values: usize,
    /// Entries of a menu.
    pub menu: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            memory: 64 << 20,
            call: Duration::from_millis(250),
            start: Duration::from_secs(2),
            nodes: 20_000,
            depth: 128,
            text: 64 << 10,
            tracks: 64,
            values: 4096,
            menu: 256,
        }
    }
}

/// Why a module is gone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The bytes are not a component, or it needs more than the page's interface.
    Load(String),
    /// A call did not come back in time.
    Timeout,
    /// The module wanted more memory than it may have.
    Memory,
    /// The module trapped (an unreachable, a panic, a stack that ran out, a bad access).
    Trap(String),
    /// The module handed out something that is not a page, or one too large.
    Malformed(String),
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Load(why) => write!(f, "could not be loaded: {why}"),
            Self::Timeout => f.write_str("did not come back in time"),
            Self::Memory => f.write_str("asked for more memory than it may have"),
            Self::Trap(why) => write!(f, "trapped: {why}"),
            Self::Malformed(why) => write!(f, "handed out something that is not a page: {why}"),
        }
    }
}

impl std::error::Error for Fault {}

/// Refuses what the module may not have, and remembers that it did.
pub(crate) struct Limiter {
    memory: usize,
    pub(crate) exceeded: bool,
}

impl ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.memory {
            self.exceeded = true;
            // A trap and not a refusal: a module that is told "no" would go on asking.
            return Err(wasmtime::Error::msg("the module's memory limit"));
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        // A table's elements are pointers; their share of the memory is counted as eight bytes.
        if desired.saturating_mul(8) > self.memory {
            self.exceeded = true;
            return Err(wasmtime::Error::msg("the module's table limit"));
        }
        Ok(true)
    }

    fn instances(&self) -> usize {
        16
    }

    fn tables(&self) -> usize {
        8
    }

    fn memories(&self) -> usize {
        4
    }
}

/// What a store holds besides the module: the limits, and the layout the module may ask of.
pub(crate) struct Host {
    pub(crate) limiter: Limiter,
    pub(crate) laid: Option<Arc<Laid>>,
}

impl Host {
    pub(crate) fn new(limits: &Limits) -> Self {
        Self {
            limiter: Limiter {
                memory: limits.memory,
                exceeded: false,
            },
            laid: None,
        }
    }
}

/// An engine for modules: compiled to be interrupted by the epoch, speaking the component
/// model, with nothing of the host's to offer.
pub(crate) fn engine() -> Result<Engine, Fault> {
    let mut config = Config::new();
    config.epoch_interruption(true);
    config.wasm_component_model(true);
    Engine::new(&config).map_err(|why| Fault::Load(format!("{why:#}")))
}

/// A store for one module under `limits`.
pub(crate) fn store(engine: &Engine, limits: &Limits) -> Store<Host> {
    let mut store = Store::new(engine, Host::new(limits));
    store.limiter(|host| &mut host.limiter);
    store
}

/// Runs `body` on `store` with `within` to come back; what it returns is what the module
/// said, and a failure is told for what it was.
pub(crate) fn call<R>(
    store: &mut Store<Host>,
    within: Duration,
    body: impl FnOnce(&mut Store<Host>) -> wasmtime::Result<R>,
) -> Result<R, Fault> {
    // The epoch moves once, `within` from now, if the call is still running.
    store.set_epoch_deadline(1);
    let guard = watchdog::arm(store.engine(), within);
    let outcome = body(store);
    drop(guard);
    outcome.map_err(|why| fault_of(store, &why))
}

fn fault_of(store: &Store<Host>, why: &wasmtime::Error) -> Fault {
    if store.data().limiter.exceeded {
        return Fault::Memory;
    }
    match why.downcast_ref::<Trap>() {
        Some(Trap::Interrupt) => Fault::Timeout,
        Some(trap) => Fault::Trap(trap.to_string()),
        None => Fault::Trap(format!("{why:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmtime::component::{Component, Linker};

    /// A component with one function, `run`, that does `body`.
    fn run(wat_body: &str, limits: &Limits) -> Result<(), Fault> {
        let engine = engine()?;
        let source = format!(
            r#"(component
                (core module $m
                    (memory (export "memory") 1)
                    (func (export "run") {wat_body}))
                (core instance $i (instantiate $m))
                (func (export "run") (canon lift (core func $i "run"))))"#
        );
        let component =
            Component::new(&engine, source).map_err(|why| Fault::Load(format!("{why:#}")))?;
        let mut store = store(&engine, limits);
        let linker = Linker::<Host>::new(&engine);
        let instance = call(&mut store, limits.start, |store| {
            linker.instantiate(store, &component)
        })?;
        let run = instance
            .get_typed_func::<(), ()>(&mut store, "run")
            .map_err(|why| Fault::Load(format!("{why:#}")))?;
        call(&mut store, limits.call, |store| run.call(store, ()))
    }

    fn quick() -> Limits {
        Limits {
            call: Duration::from_millis(50),
            memory: 4 << 20,
            ..Limits::default()
        }
    }

    #[test]
    fn a_call_that_returns_is_not_touched() {
        assert_eq!(run("nop", &quick()), Ok(()));
    }

    #[test]
    fn a_module_that_never_returns_is_cut_off_after_its_time() {
        let began = std::time::Instant::now();
        assert_eq!(run("(loop $l (br $l))", &quick()), Err(Fault::Timeout));
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "cut off in {:?}",
            began.elapsed()
        );
    }

    #[test]
    fn a_module_that_asks_for_more_memory_than_it_may_have_is_stopped() {
        // Sixty-four pages (4 MiB) at a time, forever.
        let grow = "(loop $l (drop (memory.grow (i32.const 64))) (br $l))";
        assert_eq!(run(grow, &quick()), Err(Fault::Memory));
    }

    #[test]
    fn a_module_that_traps_is_told_so_and_a_module_that_recurses_runs_out_of_stack() {
        assert!(matches!(run("unreachable", &quick()), Err(Fault::Trap(_))));
        let recursion = r#"(call 0)"#;
        // `run` is the component's function 0 of the core module: it calls itself.
        assert!(matches!(run(recursion, &quick()), Err(Fault::Trap(_))));
    }

    #[test]
    fn a_module_that_wants_something_the_page_does_not_offer_does_not_start() {
        let engine = engine().expect("engine");
        let component = Component::new(
            &engine,
            r#"(component
                (import "wasi:cli/environment@0.2.0" (instance (export "get-environment" (func))))
                (core module $m (func (export "run")))
                (core instance $i (instantiate $m))
                (func (export "run") (canon lift (core func $i "run"))))"#,
        )
        .expect("a component");
        let limits = quick();
        let mut store = store(&engine, &limits);
        let linker = Linker::<Host>::new(&engine);
        let why = linker
            .instantiate(&mut store, &component)
            .expect_err("an import nobody gave");
        assert!(
            format!("{why:#}").contains("wasi:cli/environment"),
            "{why:#}"
        );
    }
}
