//! Opening a terminal pane — the one way a pane comes to be. bateri's tabs open theirs here, and
//! so do the tests that open a pane with no application object: the same birth, the same events,
//! and nothing of bateri's application on the way. A pane that reached into the application
//! (`app::delegate()`) would work in a tab and silently not where there is none.
//!
//! **What an owner gives** ([`Config`]): an id for the pane, unique for the process's life; the
//! owner that hears its events ([`Host`]); who the application is ([`Identity`]: its name, the
//! helper program the shell integration asks, the integration's scripts); the settings and the
//! theme; where the shell starts and what it is first given. [`Config::new`] fills the rest the
//! way bateri does — the shell integration from the identity and the settings, motion and the
//! scroll bar from the settings and the system.
//!
//! **What an owner is told** ([`Host`]): every event a pane has for its owner, on the main
//! thread, with the pane's id. Only the events whose default is a working answer have one (the
//! remote copy goes to the general pasteboard; a pane that is never moved between owners hears no
//! move).
//!
//! **What an owner does after [`open`]**: it puts the pane — an `NSView` — in its view hierarchy,
//! tells it its frame is set ([`TerminalPane::observe_frame`]) and starts its shell
//! ([`TerminalPane::start`]); [`TerminalPane::close`] ends it. The owner tells the pane what its
//! window does (key, occlusion, scale, a question ending) through the pane's own methods.
//!
//! **A pane is found by its id** ([`pane`]): the jobs a pane sends to the main queue from its
//! reader thread and its background work find it here, whoever its owner is. A pane opened here
//! is found until its closing begins.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use bt_core::{InitialInput, PaneUuid, Settings, Theme};
use bt_gpu::{GpuError, ScrollbarMode, Stats};
use bt_shell_common::ssh_route::Masters;
use bt_shell_common::zoom::Zoom;
use objc2::MainThreadMarker;
use objc2::rc::{Retained, Weak};
use objc2_foundation::NSRect;

use crate::Run;
use crate::child;
use crate::launch::{self, Held, Launch};
use crate::pane::PaneLaunch;

pub use crate::footer::ListeningPort;
pub use crate::launch::{Identity, Integration};
pub use crate::pane::{PaneHost as Host, TerminalPane};
pub use crate::sheets::{Cover, Owner, OwnerSlot, WeakCover};
pub use bt_shell_common::jobs::Foreground;
pub use bt_shell_common::notices::Source;

/// What a pane is opened with.
pub struct Config {
    /// The pane's id: what every event names it by, and what [`pane`] finds it by. Unique for
    /// the process's life.
    pub id: u64,
    /// The owner of the pane's events.
    pub host: Rc<dyn Host>,
    /// Who the application opening the pane is.
    pub identity: Rc<Identity>,
    /// The settings at the pane's birth; later changes go through the pane's own methods.
    pub settings: Settings,
    /// The pane's colours.
    pub theme: Theme,
    /// Where the shell starts; `None` is the user's home.
    pub working_directory: Option<PathBuf>,
    /// The shell's first input and whether it runs; `None` is an ordinary shell.
    pub initial_input: Option<InitialInput>,
    /// The pane's persistent identity (`TERM_SESSION_ID`, `bateri://tab/<id>`); `None` is a
    /// new one. A restored pane keeps its saved one.
    pub uuid: Option<PaneUuid>,
    /// A previous session's history, replayed before the shell starts.
    pub replay: Option<Vec<u8>>,
    /// Variables added to the shell's environment. The locale's and the shell
    /// integration's own win over them on a clash; `TERM` and the identity
    /// family (`TERM_SESSION_ID`, `BATERI_TAB_URL`, …) are never overridden.
    pub env: Vec<(String, String)>,
    /// The temporary point-size step the pane is born with (an inherited ⌘+/⌘−).
    pub zoom: Zoom,
    /// The application's ssh masters, one registry for every pane, if it keeps them: remote
    /// file jobs ride the user's own connection through them.
    pub masters: Option<Arc<Masters>>,
    /// The shell integration's environment, the arguments the shell is started with and the dock
    /// rows it is born with ([`shell_integration`]).
    pub integration: Integration,
    /// Reduce Motion, resolved.
    pub reduce_motion: bool,
    /// Whether the wheel scrolls smoothly, resolved.
    pub smooth_scroll: bool,
    /// The scroll bar's form, resolved.
    pub scrollbar: ScrollbarMode,
    /// bateri's timed run's recipe; `None` → interactive.
    pub(crate) run: Option<Run>,
    /// bateri's measurement ledger (the timed run's `BT_FRAME_STATS`).
    pub(crate) stats: Option<Arc<Stats>>,
    /// A terminal a server keeps shown as the pane's first program, the shell following it
    /// ([`Held`]); `None` → the shell is the first program.
    pub(crate) held: Option<Held>,
}

impl Config {
    /// A pane as an application that hosts one opens it: a new shell in the user's home, the
    /// shell integration from `identity` and `settings` ([`shell_integration`]), motion and the
    /// scroll bar as the settings and the system say — what bateri gives a pane of its own, but
    /// for what only bateri keeps (its ssh masters).
    pub fn new(
        mtm: MainThreadMarker,
        id: u64,
        host: Rc<dyn Host>,
        identity: Rc<Identity>,
        settings: Settings,
        theme: Theme,
    ) -> Self {
        let reduce_motion =
            launch::reduce_motion(settings.reduce_motion, launch::system_reduce_motion);
        Self {
            integration: shell_integration(&identity, &settings, None),
            smooth_scroll: launch::smooth_scroll(&settings, reduce_motion),
            scrollbar: launch::scrollbar(settings.scrollbar, || launch::overlay_scrollers(mtm)),
            reduce_motion,
            id,
            host,
            identity,
            settings,
            theme,
            working_directory: child::working_directory(),
            initial_input: None,
            uuid: None,
            replay: None,
            env: Vec::new(),
            zoom: Zoom::default(),
            masters: None,
            run: None,
            stats: None,
            held: None,
        }
    }
}

impl Config {
    /// The same pane with `launch`'s start: its directory, first input,
    /// identity and replayed history.
    pub(crate) fn with_launch(self, launch: Launch) -> Self {
        let Launch {
            working_directory,
            initial_input,
            uuid,
            replay,
            env,
            held,
        } = launch;
        Self {
            working_directory,
            initial_input,
            uuid,
            replay,
            env,
            held,
            ..self
        }
    }
}

/// What the shell integration adds to a new pane's shell, and the dock rows it is born with:
/// the wrapper for the user's shell when the settings install it, the helper program beside it
/// (`identity`), and the masters' instance if the application keeps masters.
pub fn shell_integration(
    identity: &Identity,
    settings: &Settings,
    masters: Option<&Masters>,
) -> Integration {
    let setting = settings.shell_integration;
    let wrapper = launch::integration_env(
        setting,
        child::shell,
        |kind| identity.script_dir(kind),
        child::bash_version,
        launch::ProcessEnv::current(),
    );
    launch::with_dock(identity, setting, wrapper, masters.map(Masters::instance))
}

/// Opens a pane in `frame` as `config` says: born, not started. Its shell starts when the host
/// has placed it ([`TerminalPane::start`]).
pub fn open(
    mtm: MainThreadMarker,
    frame: NSRect,
    config: Config,
) -> Result<Retained<TerminalPane>, GpuError> {
    let Config {
        id,
        host,
        identity,
        settings,
        theme,
        working_directory,
        initial_input,
        uuid,
        replay,
        env,
        zoom,
        masters,
        integration,
        reduce_motion,
        smooth_scroll,
        scrollbar,
        run,
        stats,
        held,
    } = config;
    let pane = TerminalPane::new(
        mtm,
        frame,
        PaneLaunch {
            id,
            run,
            host,
            lookup: pane,
            stats,
            settings,
            theme,
            launch: Launch {
                working_directory,
                initial_input,
                uuid,
                replay,
                env,
                held,
            },
            integration,
            reduce_motion,
            smooth_scroll,
            scrollbar,
            zoom,
            masters,
            identity,
        },
    )?;
    OPENED.with(|opened| opened.borrow_mut().push((id, Weak::new(&pane))));
    Ok(pane)
}

thread_local! {
    /// Every pane opened here, by id, held weakly: the panes are their hosts'.
    static OPENED: RefCell<Vec<(u64, Weak<TerminalPane>)>> = const { RefCell::new(Vec::new()) };
}

/// The open pane whose id is `id` — not one whose closing has begun: a job that comes back to
/// the main queue after that must not act on a closed session.
pub fn pane(_mtm: MainThreadMarker, id: u64) -> Option<Retained<TerminalPane>> {
    OPENED.with(|opened| {
        let mut opened = opened.borrow_mut();
        opened.retain(|(_, pane)| pane.load().is_some());
        opened
            .iter()
            .find(|(known, _)| *known == id)
            .and_then(|(_, pane)| pane.load())
            .filter(|pane| !pane.is_closed())
    })
}
