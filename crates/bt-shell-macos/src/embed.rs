//! Opening a terminal pane — the one way a pane comes to be. bateri's tabs open theirs here, and
//! so does an application that hosts a pane in a view of its own: the same birth, the same
//! events, and nothing of bateri's application object on the way.
//!
//! **What a host gives** ([`Config`]): an id for the pane, unique for the process's life; the
//! owner that hears its events ([`Host`]); who the application is ([`Identity`]: its name, the
//! helper program the shell integration asks, the integration's scripts); the settings and the
//! theme; where the shell starts and what it is first given. [`Config::new`] fills the rest the
//! way bateri does — the shell integration from the identity and the settings, motion and the
//! scroll bar from the settings and the system.
//!
//! **What a host is told** ([`Host`]): every event a pane has for its owner, on the main thread,
//! with the pane's id. No event has a default a host could forget to answer, except the two
//! whose default is a working answer (the remote copy goes to the general pasteboard; a pane
//! that is never moved between owners hears no move).
//!
//! **What a host does after [`open`]**: it puts the pane — an `NSView` — in its view hierarchy,
//! tells it its frame is set ([`TerminalPane::observe_frame`]) and starts its shell
//! ([`TerminalPane::start`]); [`TerminalPane::close`] ends it.
//!
//! **A pane is found by its id** ([`pane`]): the jobs a pane sends to the main queue from its
//! reader thread and its background work find it here, whoever its host is. A pane opened here is
//! found until its closing begins.

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
use crate::launch::{self, Adopted, Launch};
use crate::pane::{Holder, PaneLaunch};

pub use crate::launch::Identity;
pub use crate::pane::{PaneHost as Host, TerminalPane};
pub use crate::sheets::{Cover, Owner, OwnerSlot, WeakCover};
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
    /// The temporary point-size step the pane is born with (an inherited ⌘+/⌘−).
    pub zoom: Zoom,
    /// The application's ssh masters, one registry for every pane, if it keeps them: remote
    /// file jobs ride the user's own connection through them.
    pub masters: Option<Arc<Masters>>,
    /// The shell integration's environment and the dock rows it is born with
    /// ([`shell_integration`]).
    pub integration: (Vec<(String, String)>, u16),
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
    /// bateri's bound holder, which keeps the pane's programs through a crash or a quit.
    pub(crate) keeper: Option<Rc<dyn Holder>>,
    /// A program a previous bateri handed over, carried on instead of a new shell.
    pub(crate) adopt: Option<Adopted>,
}

impl Config {
    /// A pane as an application that hosts one opens it: a new shell in the user's home, the
    /// shell integration from `identity` and `settings` ([`shell_integration`]), motion and the
    /// scroll bar as the settings and the system say — what bateri gives a pane of its own, but
    /// for what only bateri keeps (its holder, its ssh masters).
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
            zoom: Zoom::default(),
            masters: None,
            run: None,
            stats: None,
            keeper: None,
            adopt: None,
        }
    }
}

impl Config {
    /// The same pane with `launch`'s start: its directory, first input,
    /// identity, replayed history and handed-over program.
    pub(crate) fn with_launch(self, launch: Launch) -> Self {
        let Launch {
            working_directory,
            initial_input,
            uuid,
            replay,
            adopt,
        } = launch;
        Self {
            working_directory,
            initial_input,
            uuid,
            replay,
            adopt,
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
) -> (Vec<(String, String)>, u16) {
    let setting = settings.shell_integration;
    let wrapper = launch::integration_env(
        setting,
        child::shell,
        || identity.zsh_wrapper_dir.clone(),
        std::env::var_os("ZDOTDIR"),
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
        zoom,
        masters,
        integration,
        reduce_motion,
        smooth_scroll,
        scrollbar,
        run,
        stats,
        keeper,
        adopt,
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
                adopt,
            },
            integration,
            reduce_motion,
            smooth_scroll,
            scrollbar,
            zoom,
            masters,
            keeper,
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
