//! Terminal pane: the **whole core** of a single terminal session — the
//! session, the display link that drives frames, its own `Renderer`, the
//! `CAMetalLayer` surface, `BateriView`, the shell's wake end (`ShellWake`),
//! the dock reserve, the temporary point-size delta, the pane's identity, the
//! scrollback search panel and the upload queue.
//!
//! `TerminalPane` is an `NSView` subclass and is the very same thing as
//! today's content container: `BateriView` is its child that
//! fills it via autoresizing, and the search panel floats inside it as a
//! sibling of the Metal layer. The tab (`tab::TerminalTab`) plugs the pane
//! into the splits container (`split_view::SplitView`) and keeps the focused
//! pane and the title's read; the window (`window::TerminalWindow`) keeps
//! chrome, the title bar and the close question; geometry, occlusion and
//! focus are distributed from the window to all panes. A tab can hold
//! several panes (splits): each with its own session, link and renderer.
//!
//! **The boundary has three parts**: the pane takes its inputs
//! at birth in a single package ([`PaneLaunch`]: settings snapshot, theme,
//! timed-run recipe, measurement ledger, integration environment + dock
//! reserve, identity, start directory and first input, motion flags), hands
//! its events to its owner through [`PaneHost`] (title, the shell's exit,
//! upload status, notification, subtitle notices, OSC 52 copy) and every job
//! the menu fulfils is a named method here — the selector is a line that
//! calls it. Pane-level selectors (point size, find, clear, scroll, upload
//! cancel) live on the pane, because the responder chain is
//! `BateriView` → **pane** → window → delegate: a targetless menu item
//! reaches them from the focused pane, even while the search field has focus
//! (the field is a descendant of the pane). There is no path in this module
//! that reaches `AppDelegate`: main-queue returns find the pane by id through
//! the lookup function the owner supplies ([`PaneLookup`]).
//!
//! **Renderer per pane**: the atlas key
//! includes scale and point size, and the point-size delta belongs to the pane.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use bt_core::pencere::{Action, ShellGroup, ViewOptions, font_stack};
use bt_core::{
    BlockHandle, BlockInfo, Chord, CommandId, CommandPhase, ContentEdge, FontOptions, PaneUuid,
    ProgramBar, RemoteFiles, RemoteTarget, SearchCover, SearchDirection, SearchReport,
    SearchStatus, Session, SessionOptions, Settings, Theme, TtyModes, Wake,
};
use bt_core::{load_shell, smoke_shell};
use bt_gpu::{
    CellMetrics, DisplayLink, GpuError, Layout, Pacer, Renderer, ScrollbarMode, Shot, Stats,
    Surface, Waker,
};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSApplication, NSAutoresizingMaskOptions, NSBox, NSBoxType, NSButton, NSColor,
    NSControlTextEditingDelegate, NSCursor, NSEventModifierFlags, NSFont, NSFontAttributeName,
    NSFontWeightRegular, NSForegroundColorAttributeName, NSLineBreakMode, NSMenuItem, NSPasteboard,
    NSPasteboardNameFind, NSPopoverDelegate, NSSearchFieldDelegate, NSTextField,
    NSTextFieldDelegate, NSTitlePosition, NSView, NSViewFrameDidChangeNotification,
    NSWindowDidBecomeKeyNotification, NSWindowDidChangeBackingPropertiesNotification,
    NSWindowDidChangeOcclusionStateNotification, NSWindowDidEndSheetNotification,
    NSWindowDidResignKeyNotification, NSWindowOcclusionState, NSWindowOrderingMode,
};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;
use objc2_foundation::{
    NSAttributedString, NSAttributedStringKey, NSDate, NSDateFormatter, NSDateFormatterStyle,
    NSDictionary, NSMutableAttributedString, NSNotification, NSNotificationCenter,
    NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSUUID, ns_string,
};
use objc2_quartz_core::CAMetalLayer;

use crate::card::{self, FrameBox};
use crate::environment::Wants;

use crate::clipboard::{self, PendingCopy};
use crate::focus::Moment;
use crate::grid::{self, Grid, split_into_grid};
use bt_shell_common::kablo::roster::Background;
use bt_shell_common::kablo::service::{PaneLink, Shown};
use bt_shell_common::view_strip;

use crate::jobs::{self, Foreground, Probe, ShellParent, SystemTable};
use crate::journal::PaneJournal;
use crate::launch::{Adopted, Closing, Identity, Integration, Launch};
use crate::notices::{Source, font_messages};
use crate::pacer::MacPacer;
use crate::password_sheet::PasswordSheet;
use crate::pencere_view::{self, ViewHost, Waits};
use crate::ports::Listener;
use crate::preview::PreviewTicket;
use crate::program;
use crate::program::identity::{self, Kind, Seen};
use crate::promise::FinderDrops;
use crate::quote;
use crate::remote_helper::RemoteHelper;
use crate::remote_ports::RemotePorts;
use crate::restore::{SavedKept, SavedPane};
use crate::search_bar::{SearchBar, selection_query};
use crate::server_card::ServerCard;
use crate::ssh_route::Masters;
use crate::stats::StatsSeat;
use crate::upload::Transfers;
use crate::uploader::{StopSheet, UploadPopover};
use crate::view::BateriView;
use crate::zoom::Zoom;
use crate::{Run, Workload};
use crate::{child, locale};

/// Events the pane hands to its owner — today
/// `tab::TabHost`, tomorrow an embedding application.
///
/// All of them are called **on the main thread** and with the pane's id
/// ([`TerminalPane::id`]): the owner holds several panes (splits) and must
/// know which one the event came from. The methods carry no AppKit types, so
/// the owner can be tested with a fake application. Sheets go through the
/// sheet gate (`crate::sheets`), which resolves where they sit from the pane
/// itself; the popover uses the pane's own view. The owner is not asked for
/// either.
pub trait PaneHost {
    /// Title, working directory, remote state or upload percentage changed:
    /// the window's title and the tab's label must be re-read from the pane.
    fn title_changed(&self, pane: u64);
    /// The shell exited: the pane has nothing left to stand on and must close —
    /// only this pane, not the tab.
    fn shell_exited(&self, pane: u64);
    /// The keyboard arrived at this pane's terminal (`BateriView` became first
    /// responder): the focused pane is now this one — the title and the new
    /// split's inheritance come from it.
    fn focused(&self, pane: u64);
    /// The upload queue's progress or existence changed — the application's
    /// Dock icon is the total of all panes ([`TerminalPane::upload_totals`]).
    fn uploads_changed(&self, pane: u64);
    /// A command started or ended in this pane — the local shell's `C`, our
    /// `A` or a `D`, or our remote shell's `C`/`D` — or the pane went onto or
    /// off the alternate screen: the tab's running ring and its "finished
    /// while you were away" mark are read again (`Session::activity`). A
    /// command's edge comes for a background tab too, being the reader
    /// thread's news; the screen's is the frame path's, so a tab that draws
    /// nothing hears it when it is next drawn.
    fn activity_changed(&self, pane: u64);
    /// What the pane runs changed ([`TerminalPane::program_kind`]): the tab's icon is read again.
    /// Nothing for an owner that draws no such icon — the embedding interface has no event for
    /// it, and `activity_changed`'s is a command's or the screen's.
    fn program_changed(&self, _pane: u64) {}
    /// The pane's transfer queue ended, as `finish` says: what the tab's chip plays out and
    /// leaves on the tab while the user is elsewhere. Called **before** the pane tells its
    /// title the transfer is gone, so the tab reads the end together with that news. Nothing for
    /// an owner that draws no such chip — the embedding interface has no event for it.
    fn upload_ended(&self, _pane: u64, _finish: crate::tabs::Finish) {}
    /// Notification to the user (upload finished, failed, connection lost).
    fn notify(&self, pane: u64, title: &str, body: &str);
    /// Subtitle notices (today only the font's, `sync_geometry`).
    fn post_notices(&self, pane: u64, source: Source, messages: Vec<String>);
    /// Remote copy (OSC 52). The default arm writes to the general pasteboard
    /// — Cmd-C's pasteboard; separating the pasteboard stays open as an owner
    /// decision.
    fn copy_to_clipboard(&self, _pane: u64, text: String) {
        clipboard::copy(&NSPasteboard::generalPasteboard(), Some(text));
    }
    /// The pane now belongs to tab `tab` (it was moved there): every event
    /// from here on reaches that tab. Without it a moved pane's shell exit
    /// would close nothing and its news would land on the tab it left.
    fn rehomed(&self, _pane: u64, _tab: u64) {}
    /// Files are being dragged over the pane (`over`) or no longer are: while
    /// they are, ⌥⌘ are the drag's own keys, so nothing the owner starts with
    /// ⌥⌘ may start (bateri's arrangement of panes).
    fn files_dragged(&self, pane: u64, over: bool);
    /// A press with ⌥⌘ landed on the pane ([`crate::pointer::swallows`]) —
    /// never the program's: the owner's to carry the pane, if it carries
    /// panes. `at` is the press in the window's points.
    fn carry_press(&self, pane: u64, at: (f64, f64));
    /// A question of this pane was put off (its pane is not on screen) or
    /// taken up: a mark the owner shows for a question waiting to be seen
    /// may have changed.
    fn questions_changed(&self, pane: u64);
    /// The ports the pane's programs listen on — and, in a remote session,
    /// the server's — changed ([`TerminalPane::listening_ports`]): a scan
    /// found new ones or a server went. Scans run while `[shell] ports` is on.
    fn ports_changed(&self, pane: u64);
    /// The user opened a link (⌘-click, or the link menu's Open): `true` if
    /// the owner took it — a file shown in its own editor at its line — and
    /// the pane does nothing more. The default leaves it to the pane, which
    /// opens it as bateri does (`links::action`'s white list: a URL or a
    /// document in its default application, the rest revealed, an uncommon
    /// scheme asked about first).
    fn open_link(&self, _pane: u64, _link: &LinkRequest) -> bool {
        false
    }
    /// Whether `chord`, pressed while a program's view has the keyboard, is the owner's own
    /// shortcut — a tab, a window, a split, the palette — and not the view's: the view never
    /// sees it and the owner's main menu runs it (`pencere_view`). The owner answers from its
    /// shortcut table (`shortcuts::Resolved::kept_from_view`: the commands that work on the
    /// pane's content are the view's). The default is an owner with no shortcuts of its own,
    /// whose menu still gets the keys the view leaves unhandled.
    fn keeps_key(&self, _chord: &Chord) -> bool {
        false
    }
    /// The terminal's bell rang (`BEL`), at most once per main-queue turn.
    fn bell(&self, pane: u64);
    /// A program asked for a notification (`OSC 9`'s text, `OSC 777`'s
    /// title and body) — not bateri's own ([`Self::notify`]): what the
    /// program wrote, cleaned of control characters and bounded.
    fn program_notification(&self, pane: u64, title: Option<&str>, body: &str);
    /// What this pane's questions cover ([`crate::sheets::Cover`]): bateri's
    /// tab container, so a question blocks that tab and not the window.
    /// `None` puts them on the pane's window, blocking all of it. Asked
    /// from where the pane stands, even while it closes.
    fn cover(&self, pane: &TerminalPane) -> Option<Rc<dyn crate::sheets::Cover>>;
    /// The terminal a server keeps that the pane shows, if it shows one: the pane's
    /// local shell is waiting behind a program that only carries that terminal's
    /// bytes, so what is running in front is the server's to say. The default
    /// is an owner that has no such terminals.
    fn kept_terminal(&self, _pane: &PaneUuid) -> Option<Shown> {
        None
    }
    /// The terminals the server of `account` keeps that no pane of this Mac shows and no other
    /// Mac has open, the last to write first: what the server's card lists. The default is an
    /// owner that has no such terminals.
    fn background(&self, _account: &str) -> Vec<Background> {
        Vec::new()
    }
    /// The terminals the server of `account` keeps that another Mac has open and no pane of this
    /// Mac shows, the last to write first: the card's *On other Macs*. The default is an owner
    /// that has no such terminals.
    fn elsewhere(&self, _account: &str) -> Vec<Background> {
        Vec::new()
    }
    /// The server's card is about to list them: the servers are asked for their terminals
    /// again, and the answer reaches the card (`TerminalPane::refresh_server_card`).
    fn refresh_background(&self) {}
    /// The card's row: the terminal `terminal` of `account` opens in a new tab of the pane's
    /// window.
    fn open_background(&self, _pane: u64, _account: &str, _terminal: &str) {}
    /// The card's row of a terminal another Mac has open: the terminal `terminal` of `account`
    /// is taken from that Mac and opens in a new tab of the pane's window.
    fn move_here(&self, _pane: u64, _account: &str, _terminal: &str) {}
    /// The card's *Open All in New Tab*: every terminal the server `host` keeps that no pane
    /// shows, together in a new tab of the pane's window.
    fn open_all_background(&self, _pane: u64, _host: &str) {}
    /// The card's *End Terminal*: the terminal `terminal` of `account`, which no pane shows,
    /// ends on the server (asked **after** the user said so).
    fn end_background(&self, _account: &str, _terminal: &str) {}
    /// The user is closing the pane (not the application quitting, not the
    /// pane moving): the terminal it shows ends with it, on the server. Asked
    /// **before** [`TerminalPane::begin_close`], whose signal lets the terminal go.
    fn end_kept_terminal(&self, _pane: &PaneUuid) {}
    /// The user is closing the pane and chose to leave the terminal it shows running: it
    /// is not ended, and nothing keeps a record that a quit or a crash would end it by.
    /// Asked **before** [`TerminalPane::begin_close`], as [`PaneHost::end_kept_terminal`].
    fn leave_kept_terminal(&self, _pane: &PaneUuid) {}
    /// ⏎ in a pane that waits for its connection: the account of the terminal it
    /// shows dials now instead of waiting out its pause. The default is an
    /// owner that has no such terminals.
    fn retry_kept_terminal(&self, _pane: &PaneUuid) {}
    /// ⏎ in a pane whose terminal another computer took: the terminal is taken back, and the
    /// pane goes on from where it was. The default is an owner that has no such terminals.
    fn bring_back_kept_terminal(&self, _pane: &PaneUuid) {}
    /// The login sheet's arbitration, for a pane whose terminal waits for a
    /// password ([`KeptLogin`]). The default is an owner that has no such terminals.
    fn kept_login(&self) -> Arc<dyn KeptLogin> {
        Arc::new(NoKeptLogin)
    }
}

/// What a pane's ⏎ on "Needs your password" does ([`KeptLogin::begin`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Login {
    /// No sheet is open for the account: this pane's is.
    Open,
    /// The account's sheet is open on this pane: that one is brought forward.
    Raise(PaneUuid),
}

/// The owner's side of the login that brings the terminals of a server back: one
/// sheet per account, whichever of its panes asked ([`TerminalPane::log_in_to_reconnect`]).
/// Shared with the sheet's thread, so `Send + Sync`; [`KeptLogin::raise`] is the
/// main thread's.
pub trait KeptLogin: Send + Sync {
    /// `pane` wants the account's login sheet.
    fn begin(&self, account: &str, pane: &PaneUuid) -> Login;
    /// The account's login ended (logged in, cancelled, failed).
    fn end(&self, account: &str);
    /// The user logged in: every terminal on the account dials now.
    fn done(&self, account: &str);
    /// Brings the tab of `pane` forward, the keyboard to the pane; `false` if it
    /// is gone. Main thread.
    fn raise(&self, pane: &PaneUuid) -> bool;
}

/// A host with no terminals a server keeps: nothing waits for a login.
struct NoKeptLogin;

impl KeptLogin for NoKeptLogin {
    fn begin(&self, _: &str, _: &PaneUuid) -> Login {
        Login::Open
    }
    fn end(&self, _: &str) {}
    fn done(&self, _: &str) {}
    fn raise(&self, _: &PaneUuid) -> bool {
        false
    }
}

/// A link the user opened, as the pane's owner may take it
/// ([`PaneHost::open_link`]): the link as found and checked — a path exists
/// by the time it is asked about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkRequest {
    /// A URL as written: plain text, or an OSC 8 link's target. Its scheme
    /// is anything the program wrote.
    Url(String),
    /// A local file or directory, with the line and column a
    /// `path:line:col` named. `path` is the link as it names it — a symlink
    /// stays the symlink; `target` is the object it leads to once every
    /// symlink and Finder alias on it is followed (`links::Resolved::target`).
    /// An owner that hands the link to the system judges and opens `target`:
    /// Launch Services follows `path` and would launch an `.app` or run a
    /// script a harmless-looking name points at.
    Path {
        path: std::path::PathBuf,
        target: std::path::PathBuf,
        directory: bool,
        line: Option<u32>,
        col: Option<u32>,
    },
    /// A path on the remote session's server, absolute there.
    RemotePath {
        path: String,
        directory: bool,
        line: Option<u32>,
        col: Option<u32>,
    },
}

/// Column count of the smallest pane: a split that would drop
/// below it is not made. Not measured, a design constant — room for the
/// prompt's two columns, a short command and the folder name in the dock's
/// context line; narrower makes the shell's own line wrapping meaningless.
/// Tuned by eye.
const MIN_PANE_COLS: u16 = 20;

/// Row count of the smallest pane, grid rows **excluding** the
/// dock's reserve. Design constant: one command and a few lines of its
/// output; a full-screen program (vim, htop) can show nothing but a status
/// line below it.
const MIN_PANE_ROWS: u16 = 5;

/// Opacity of the unfocused pane's veil: the theme's
/// background overlays the text at this ratio. Not measured, a design
/// constant (like `GUTTER_PT`) — Ghostty's `unfocused-split-opacity` default
/// is `0.7`, i.e. a veil of `0.3`; the same ratio: focus reads at a glance
/// and the dimmed pane's text is still readable. Tuned by eye.
const DIM_ALPHA: f64 = 0.3;

/// Opacity of the veil over a grid that is no longer live: a terminal the
/// server keeps while the connection to it is gone, and the pane shows its last
/// screen. Stronger than [`DIM_ALPHA`] on purpose and a separate constant: an
/// unfocused pane is a live one that is merely not the keyboard's, this one
/// cannot be typed to, and the two must not be read as each other when a
/// split has both. Not measured, a design constant (the user's choice of the
/// number: 0.5).
const LINK_VEIL_ALPHA: f64 = 0.5;

define_class!(
    // SAFETY: NSBox is designed for subclassing; DimOverlay implements no
    // `Drop`, has no ivar and is born with NSBox's constructor (`new`).
    #[unsafe(super(NSBox))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriDimOverlay"]
    pub(crate) struct DimOverlay;

    unsafe impl NSObjectProtocol for DimOverlay {}

    impl DimOverlay {
        /// Never takes part in hit testing: clicks, drags and the wheel fall
        /// through to the `BateriView` underneath — clicking a dimmed pane
        /// focuses it (the click path) and the veil must not cut that.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl DimOverlay {
    /// Born hidden; the colour is [`DimOverlay::paint`], the visibility is the
    /// owner's ([`TerminalPane::set_dimmed`]).
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // SAFETY: `NSBox`'s `init`; the subclass has no ivar.
        let this = Self::alloc(mtm).set_ivars(());
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setBoxType(NSBoxType::Custom);
        this.setTitlePosition(NSTitlePosition::NoTitle);
        this.setBorderWidth(0.0);
        this.setHidden(true);
        this
    }

    /// The theme's background at [`DIM_ALPHA`] opacity. `NSColor` takes sRGB
    /// (the colour-space boundary; like the separator's `separator_srgb`).
    fn paint(&self, theme: &Theme) {
        self.paint_with(theme, DIM_ALPHA);
    }

    /// The theme's background at `alpha` opacity.
    fn paint_with(&self, theme: &Theme, alpha: f64) {
        let [r, g, b] = theme.background_srgb().map(|byte| f64::from(byte) / 255.0);
        self.setFillColor(&NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, alpha));
    }
}

/// What a pane does with a key or a paste, by how its terminal's connection
/// stands.
#[derive(Debug, PartialEq, Eq)]
enum Wait {
    /// The terminal is there: the input goes to it.
    Pass,
    /// ⏎ while the bridge tries again: dial now.
    TryNow,
    /// ⏎ while the server wants a login nobody saved: the sheet.
    LogIn,
    /// ⏎ while another computer has the terminal: take it back.
    BringBack,
    /// Anything else while it waits: nothing goes to the terminal, the system beeps.
    Beep,
}

fn on_wait(link: PaneLink, enter: bool) -> Wait {
    match (link.is_passive(), enter, link) {
        (false, ..) => Wait::Pass,
        (true, true, PaneLink::NeedsLogin) => Wait::LogIn,
        (true, true, PaneLink::Taken) => Wait::BringBack,
        (true, true, _) => Wait::TryNow,
        (true, false, _) => Wait::Beep,
    }
}

/// The target label's distance from the pane's bottom-left corner and the
/// text's inset inside it, in points. Design constants (not measured) — a
/// browser's status bubble.
const LINK_LABEL_MARGIN: f64 = 6.0;
const LINK_LABEL_PAD_X: f64 = 6.0;
const LINK_LABEL_PAD_Y: f64 = 2.0;

define_class!(
    // SAFETY: NSBox is designed for subclassing; LinkLabel implements no
    // `Drop`, has no ivar and is born with NSBox's constructor (`new`).
    #[unsafe(super(NSBox))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriLinkLabel"]
    pub(crate) struct LinkLabel;

    unsafe impl NSObjectProtocol for LinkLabel {}

    impl LinkLabel {
        /// Never takes part in hit testing ([`DimOverlay`]'s rule): the label
        /// sits over the dock's context line and a click there must reach the
        /// `BateriView` underneath.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl LinkLabel {
    /// The ⌘-hovered OSC 8 link's target: a small box in the
    /// pane's bottom-left corner, born hidden, its single child the text. The
    /// text is the whole target, cut in the **middle** when it does not fit —
    /// the scheme and host on the left and the file name on the right are what
    /// tells a link apart.
    fn new(mtm: MainThreadMarker) -> (Retained<Self>, Retained<NSTextField>) {
        // SAFETY: `NSBox`'s `init`; the subclass has no ivar.
        let this = Self::alloc(mtm).set_ivars(());
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setBoxType(NSBoxType::Custom);
        this.setTitlePosition(NSTitlePosition::NoTitle);
        this.setBorderWidth(1.0);
        this.setCornerRadius(4.0);
        this.setHidden(true);
        this.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewMaxXMargin | NSAutoresizingMaskOptions::ViewMaxYMargin,
        );
        let text = NSTextField::labelWithString(ns_string!(""), mtm);
        text.setFont(Some(&NSFont::systemFontOfSize(
            NSFont::smallSystemFontSize(),
        )));
        text.setTextColor(Some(&NSColor::secondaryLabelColor()));
        text.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
        // The text is the box's **content view**, not a plain subview: `NSBox`
        // forwards `addSubview:` into its content view, which sits inset by the
        // default margins and the border, so frames set in the box's own space
        // landed shifted right and up and the text overflowed the frame. The
        // padding is the margins; [`TerminalPane::set_link_target`] sizes the box
        // around the content with `setFrameFromContentFrame:`.
        this.setContentViewMargins(NSSize::new(LINK_LABEL_PAD_X, LINK_LABEL_PAD_Y));
        this.setContentView(Some(&text));
        (this, text)
    }

    /// The terminal's own surface ([`paint_surface`]).
    fn paint(&self, theme: &Theme) {
        paint_surface(self, theme);
    }
}

/// The block tip's inset from its border and the gaps between its parts,
/// points — design constants (not measured), the link label's padding.
const TIP_PAD_X: f64 = 8.0;
const TIP_PAD_Y: f64 = 4.0;
const TIP_GAP: f64 = 6.0;
/// The colour dot's diameter, points.
const TIP_DOT: f64 = 8.0;
/// How far left of the scroll bar's strip the tip ends, points: clear of the
/// thumb the pointer is next to.
const TIP_OFFSET: f64 = 24.0;
/// The tip's widest, points: a long command is cut in its middle, the
/// metadata never.
const TIP_MAX_WIDTH: f64 = 440.0;
/// The tip's least distance from the pane's edges, points.
const TIP_MARGIN: f64 = 4.0;

define_class!(
    // SAFETY: NSBox is designed for subclassing; BlockTip implements no
    // `Drop`, has no ivar and is born with NSBox's constructor (`new`).
    #[unsafe(super(NSBox))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriBlockTip"]
    pub(crate) struct BlockTip;

    unsafe impl NSObjectProtocol for BlockTip {}

    impl BlockTip {
        /// Never takes part in hit testing ([`LinkLabel`]'s rule): the tip
        /// floats over the grid, and a click under it is the grid's.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

/// The scroll bar block mark's tip: a box on the terminal's own surface
/// ([`LinkLabel`]'s paint), born hidden — a dot in the block's stripe
/// colour, the command in bold (cut in the middle), then quieter
/// "exit N · 3.4s · 14:04" or "running · since 14:04"; what the ledger does
/// not know is left out. Shown by [`TerminalPane::show_block_tip`].
pub(crate) struct BlockTipParts {
    tip: Retained<BlockTip>,
    dot: Retained<NSBox>,
    command: Retained<NSTextField>,
    meta: Retained<NSTextField>,
}

impl BlockTipParts {
    fn new(mtm: MainThreadMarker) -> Self {
        // SAFETY: `NSBox`'s `init`; the subclass has no ivar.
        let tip = BlockTip::alloc(mtm).set_ivars(());
        let tip: Retained<BlockTip> = unsafe { msg_send![super(tip), init] };
        tip.setBoxType(NSBoxType::Custom);
        tip.setTitlePosition(NSTitlePosition::NoTitle);
        tip.setBorderWidth(1.0);
        tip.setCornerRadius(4.0);
        tip.setHidden(true);
        tip.setContentViewMargins(NSSize::new(TIP_PAD_X, TIP_PAD_Y));
        let content = NSView::new(mtm);
        let dot = NSBox::new(mtm);
        dot.setBoxType(NSBoxType::Custom);
        dot.setTitlePosition(NSTitlePosition::NoTitle);
        dot.setBorderWidth(0.0);
        dot.setCornerRadius(TIP_DOT / 2.0);
        let size = NSFont::smallSystemFontSize();
        let command = NSTextField::labelWithString(ns_string!(""), mtm);
        command.setFont(Some(&NSFont::boldSystemFontOfSize(size)));
        command.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
        let meta = NSTextField::labelWithString(ns_string!(""), mtm);
        meta.setFont(Some(&NSFont::systemFontOfSize(size)));
        meta.setTextColor(Some(&NSColor::secondaryLabelColor()));
        content.addSubview(&dot);
        content.addSubview(&command);
        content.addSubview(&meta);
        // The content is the box's **content view** (the link label's
        // reason): frames inside it are its own, unshifted by the margins.
        tip.setContentView(Some(&content));
        Self {
            tip,
            dot,
            command,
            meta,
        }
    }

    /// The terminal's own surface ([`paint_surface`]).
    fn paint(&self, theme: &Theme) {
        paint_surface(&self.tip, theme);
    }

    /// Writes `info` into the parts and lays them out on one line, at most
    /// `room` points wide in all; returns the tip's content size.
    fn fill(&self, info: &BlockInfo, meta: &str, room: f64) -> NSSize {
        self.dot.setFillColor(&srgb_color(info.color));
        self.command
            .setStringValue(&NSString::from_str(&info.command));
        self.meta.setStringValue(&NSString::from_str(meta));
        self.meta.setHidden(meta.is_empty());
        let command = self.command.fittingSize();
        let meta_size = if meta.is_empty() {
            NSSize::new(0.0, 0.0)
        } else {
            self.meta.fittingSize()
        };
        let meta_room = if meta.is_empty() {
            0.0
        } else {
            TIP_GAP + meta_size.width
        };
        let room = (room - 2.0 * TIP_PAD_X - TIP_DOT - TIP_GAP - meta_room).max(0.0);
        let command_width = command.width.min(room);
        let height = command.height.max(meta_size.height).max(TIP_DOT);
        let middle = |h: f64| ((height - h) / 2.0).max(0.0);
        self.dot.setFrame(NSRect::new(
            NSPoint::new(0.0, middle(TIP_DOT)),
            NSSize::new(TIP_DOT, TIP_DOT),
        ));
        let x = TIP_DOT + TIP_GAP;
        self.command.setFrame(NSRect::new(
            NSPoint::new(x, middle(command.height)),
            NSSize::new(command_width, command.height),
        ));
        self.meta.setFrame(NSRect::new(
            NSPoint::new(x + command_width + TIP_GAP, middle(meta_size.height)),
            meta_size,
        ));
        NSSize::new(x + command_width + meta_room, height)
    }
}

/// A floating box painted as the terminal's own surface — the theme's
/// background and its separator tone (sRGB, `DimOverlay::paint`'s rule): the
/// link label and the block tip, one rule.
fn paint_surface(surface: &NSBox, theme: &Theme) {
    surface.setFillColor(&srgb_color(theme.background_srgb()));
    surface.setBorderColor(&srgb_color(theme.separator_srgb()));
}

/// An sRGB `NSColor` from three bytes, opaque — the theme's colours cross
/// the boundary as sRGB for AppKit (`Theme::background_srgb`).
fn srgb_color([r, g, b]: [u8; 3]) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(
        f64::from(r) / 255.0,
        f64::from(g) / 255.0,
        f64::from(b) / 255.0,
        1.0,
    )
}

/// "Jump to latest"'s distance from the pane's right edge — and from the
/// always-up track's left edge, when there is one — and from the top of the
/// dock's reserve, points. Design constants (not measured): clear of the
/// thin bar's strip, floating above the dock's hairline.
const JUMP_RIGHT: f64 = 20.0;
const JUMP_ABOVE: f64 = 12.0;
/// The box's inset around its button, points — the block tip's.
const JUMP_PAD_X: f64 = TIP_PAD_X;
const JUMP_PAD_Y: f64 = TIP_PAD_Y;

define_class!(
    // SAFETY: NSButton is designed for subclassing; JumpButton implements no
    // `Drop`, has no ivar and is born with NSButton's constructor (`new`).
    #[unsafe(super(NSButton))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriJumpButton"]
    pub(crate) struct JumpButton;

    unsafe impl NSObjectProtocol for JumpButton {}

    impl JumpButton {
        /// The hand cursor over the whole button: AppKit's cursor rect, the
        /// upload buttons' rule (`BateriView::hand_cursor_rects` — a cursor
        /// set by hand turns back into the arrow at the next evaluation).
        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            if !self.isHiddenOrHasHiddenAncestor() {
                self.addCursorRect_cursor(self.bounds(), &NSCursor::pointingHandCursor());
            }
        }
    }
);

/// "Jump to latest": a real button on the terminal's own surface (the block
/// tip's paint) in the pane's bottom-right corner, above the dock — a down
/// arrow and the verb, then the quieter count of the lines that came below
/// a window scrolled up ([`Session::unseen_rows`]). Born hidden; shown,
/// relabelled and hidden by [`TerminalPane::refresh_jump`]; a click is
/// [`TerminalPane::jump_to_latest`]. AppKit's, outside the frame path: it
/// asks for no frame.
pub(crate) struct JumpLatest {
    surface: Retained<NSBox>,
    button: Retained<JumpButton>,
    /// The count the title says; `None` before the first. While output
    /// streams under a scrolled window the count moves every drawn frame,
    /// and an unchanged one is not laid out again.
    count: Cell<Option<u32>>,
}

impl JumpLatest {
    /// The box and its button; the target is set once the pane is born
    /// ([`JumpLatest::aim`]).
    fn new(mtm: MainThreadMarker) -> Self {
        let surface = NSBox::new(mtm);
        surface.setBoxType(NSBoxType::Custom);
        surface.setTitlePosition(NSTitlePosition::NoTitle);
        surface.setBorderWidth(1.0);
        surface.setCornerRadius(4.0);
        surface.setHidden(true);
        // The corner it sits in stays put while the pane resizes.
        surface.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewMinXMargin | NSAutoresizingMaskOptions::ViewMaxYMargin,
        );
        // No margins: the button fills the box and carries the padding
        // itself, so the whole box takes the click and the hand cursor.
        surface.setContentViewMargins(NSSize::new(0.0, 0.0));
        // SAFETY: `NSButton`'s `init`; the subclass has no ivar.
        let button = JumpButton::alloc(mtm).set_ivars(());
        let button: Retained<JumpButton> = unsafe { msg_send![super(button), init] };
        button.setBordered(false);
        // The keyboard stays with the terminal.
        button.setRefusesFirstResponder(true);
        // The button is the box's **content view** (the link label's
        // reason): its frame is the content's, unshifted by the margins.
        surface.setContentView(Some(&button));
        Self {
            surface,
            button,
            count: Cell::new(None),
        }
    }

    /// Points the button at the pane's `jumpToLatest:`.
    fn aim(&self, pane: &TerminalPane) {
        let target: &AnyObject = pane.as_ref();
        // SAFETY: the selector is the pane's `jumpToLatest:`, which takes a
        // single `Option<&AnyObject>`; the pane holds the button (the target
        // is weak).
        unsafe {
            self.button.setTarget(Some(target));
            self.button.setAction(Some(sel!(jumpToLatest:)));
        }
    }

    /// The terminal's own surface ([`paint_surface`]).
    fn paint(&self, theme: &Theme) {
        paint_surface(&self.surface, theme);
    }

    /// Shows the box saying `rows` where `place` puts a box of its size (the
    /// pane's points), or hides it (`None` either way). The hand cursor's
    /// rect is rebuilt only when the box appeared, went or moved.
    fn set(&self, rows: Option<u32>, place: impl FnOnce(NSSize) -> Option<NSPoint>) {
        let before = (!self.surface.isHidden()).then(|| self.surface.frame());
        let origin = rows.and_then(|rows| place(self.fill(rows)));
        match origin {
            Some(origin) => {
                self.surface.setFrameOrigin(origin);
                self.surface.setHidden(false);
            }
            None => self.surface.setHidden(true),
        }
        let after = origin.map(|_| self.surface.frame());
        if before != after
            && let Some(window) = self.button.window()
        {
            window.invalidateCursorRectsForView(&self.button);
        }
    }

    /// Writes the count into the title and sizes the box around it — only
    /// when it changed; returns the box's size. The count's digits are
    /// fixed-width, so the box keeps its width while the number climbs.
    fn fill(&self, rows: u32) -> NSSize {
        if self.count.replace(Some(rows)) == Some(rows) {
            return self.surface.frame().size;
        }
        let size = NSFont::smallSystemFontSize();
        let verb = NSFont::systemFontOfSize(size);
        // SAFETY: a constant AppKit exposes, it lives for the whole process.
        let digits =
            NSFont::monospacedDigitSystemFontOfSize_weight(size, unsafe { NSFontWeightRegular });
        let part = |text: &str, color: &NSColor, font: &NSFont| {
            let values: [&AnyObject; 2] = [color.as_ref(), font.as_ref()];
            // SAFETY: AppKit's two attribute keys (extern statics), each with
            // the value type it documents — an `NSColor` and an `NSFont`.
            unsafe {
                let attributes = NSDictionary::<NSAttributedStringKey, AnyObject>::from_slices(
                    &[NSForegroundColorAttributeName, NSFontAttributeName],
                    &values,
                );
                NSAttributedString::new_with_attributes(&NSString::from_str(text), &attributes)
            }
        };
        let title = NSMutableAttributedString::new();
        title.appendAttributedString(&part("↓  Jump to latest", &NSColor::labelColor(), &verb));
        title.appendAttributedString(&part(
            &format!("   {}", jump_count(rows)),
            &NSColor::secondaryLabelColor(),
            &digits,
        ));
        self.button.setAttributedTitle(&title);
        let fit = self.button.fittingSize();
        let content = NSSize::new(fit.width + 2.0 * JUMP_PAD_X, fit.height + 2.0 * JUMP_PAD_Y);
        self.surface
            .setFrameFromContentFrame(NSRect::new(NSPoint::new(0.0, 0.0), content));
        self.surface.frame().size
    }
}

/// The quieter half of "Jump to latest": "1 new line", "12 new lines".
fn jump_count(rows: u32) -> String {
    if rows == 1 {
        "1 new line".to_owned()
    } else {
        format!("{rows} new lines")
    }
}

/// Where "Jump to latest" stands in the pane's (unflipped) points: its right
/// edge [`JUMP_RIGHT`] in from the pane's right, and further in by the
/// always-up track (`track`, points; zero in the other forms), its bottom
/// [`JUMP_ABOVE`] above the dock's reserve (`dock`, points; zero without a
/// dock) — never past the pane's left edge.
fn jump_origin(pane: NSSize, size: NSSize, track: f64, dock: f64) -> NSPoint {
    NSPoint::new(
        (pane.width - JUMP_RIGHT - track - size.width).max(TIP_MARGIN),
        dock + JUMP_ABOVE,
    )
}

/// The tip's quieter half: "exit N · 3.4s · 14:04" for a finished block,
/// "running · since 14:04" for a running one, each part only when known —
/// a restored block says nothing. `clock` renders a start (Unix seconds) as
/// the user's short time of day.
fn block_meta(info: &BlockInfo, clock: impl Fn(u32) -> String) -> String {
    if info.running {
        return match info.started {
            Some(started) => format!("running · since {}", clock(started)),
            None => "running".to_owned(),
        };
    }
    let mut parts = Vec::new();
    if let Some(exit) = info.exit {
        parts.push(format!("exit {exit}"));
    }
    if let Some(duration) = &info.duration {
        parts.push(duration.clone());
    }
    if info.exit.is_some()
        && let Some(started) = info.started
    {
        parts.push(clock(started));
    }
    parts.join(" · ")
}

/// The path by which main-queue returns find the pane by id — every pane's is
/// [`crate::embed::pane`], whoever its owner. A plain `fn` pointer, not a
/// closure: `Send` and `Copy`, so every job thrown from the reader thread to
/// the main queue can capture it and it opens no reference cycle. It must not
/// find a pane whose close has begun ([`TerminalPane::is_closed`]).
pub(crate) type PaneLookup = fn(MainThreadMarker, u64) -> Option<Retained<TerminalPane>>;

/// How many times the `posix` row's write is tried ([`TerminalPane::check_remote_up`]);
/// each attempt waits up to the state file's lock patience. A design constant.
const POSIX_ATTEMPTS: usize = 3;

/// The pane's half of the bootstrap's proof: the bootstrap's
/// `8133;i;up;{nonce}` (in the session, [`bt_core::Session::remote_up`]) and
/// the wrapped `ssh` the remote probe found arrive in either order — `up` is
/// the bootstrap's first byte and routinely beats the probe — so whichever
/// comes second does the check.
#[derive(Debug, Default)]
struct WrapProof {
    /// The probe's command generation, the wrapped argv's nonce
    /// (`jobs::Target::nonce`) and the user's argv (`ssh -G`'s input for the
    /// server's key). Only a call bateri wrapped has one.
    wrapped: Option<(u64, String, Vec<String>)>,
    /// The generation whose server was recorded: once per remote generation.
    recorded: Option<u64>,
    /// The last `up` whose nonce was marked seen ([`bt_shell_common::ssh_wrap::mark_up`]):
    /// once per arrival.
    marked: Option<(u64, String)>,
    /// The generation whose attempt was marked used — the user typed after
    /// the login ([`bt_shell_common::ssh_wrap::mark_used`]): once per
    /// generation.
    used: Option<u64>,
    /// The generation whose attempt was marked logged in
    /// ([`bt_shell_common::ssh_wrap::mark_login`]): once per generation.
    login: Option<u64>,
}

/// The delay of a pane's state after its dock mirror moved. The mirror moves
/// with every keystroke and its copy only serves the dock after a crash, which
/// marks a carried mirror stale anyway: one copy per second while typing
/// costs one blob encode a second and loses at most the last second's typing.
/// The shell's edges (a prompt, a command's start and end) do not wait — a
/// state behind them would miss a command.
const MIRROR_DELAY: Duration = Duration::from_secs(1);

/// The bound holder a pane registers with — bateri's own (`keeper::Keeper`),
/// which keeps the pane's programs through a crash or a quit. A host that
/// keeps nothing gives none ([`PaneLaunch::keeper`]); the pane then holds its
/// programs itself, as an unbundled bateri does.
pub(crate) trait Holder {
    /// The flag the reader thread reads to know whether a holder keeps the
    /// pane's journal.
    fn active_flag(&self) -> Arc<AtomicBool>;
    /// Whether a holder is bound now.
    fn is_active(&self) -> bool;
    /// The pane's program joins the holder; the journal's sink if the
    /// holder keeps one.
    fn add(
        &self,
        mtm: MainThreadMarker,
        pane: crate::handover::BoundPane,
    ) -> Option<Box<dyn crate::journal::BaseSink>>;
    /// The pane is gone: the holder lets its program go.
    fn release(&self, pane: &PaneUuid);
    /// The pane's state now (`Session::state_blob`) and the program it shows
    /// ([`TerminalPane::program_kind`], the shell and the server left out),
    /// for a restore after a crash.
    fn state(&self, pane: &PaneUuid, blob: Vec<u8>, program: Option<Kind>);
    /// The layout changed: the holder's copy of it is written again once the
    /// burst settles.
    fn layout_changed(&self);
}

/// The pane's birth package: all inputs in a single struct,
/// from the owner. Live changes go a separate way, through the pane's `set_*`
/// methods.
pub(crate) struct PaneLaunch {
    /// In-process identity ([`TerminalPane::id`]).
    pub(crate) id: u64,
    /// Timed-run recipe; `None` → interactive.
    pub(crate) run: Option<Run>,
    /// Owner of the events.
    pub(crate) host: Rc<dyn PaneHost>,
    /// The path by which main-queue returns find the pane.
    pub(crate) lookup: PaneLookup,
    /// Measurement ledger (the timed run's `BT_FRAME_STATS`), goes to the link.
    pub(crate) stats: Option<Arc<Stats>>,
    /// Copy of the settings at birth.
    pub(crate) settings: Settings,
    /// The session's theme.
    pub(crate) theme: Theme,
    /// Start directory and first input.
    pub(crate) launch: Launch,
    /// Shell integration's environment, the shell's arguments and the dock reserve — from **a
    /// single question** (`AppDelegate::shell_integration`): separate calls could diverge.
    pub(crate) integration: Integration,
    /// Resolved value of Reduce Motion.
    pub(crate) reduce_motion: bool,
    /// Resolved mode of the wheel.
    pub(crate) smooth_scroll: bool,
    /// Resolved form of the scroll bar — in the birth package because the
    /// grid's first size depends on it (the always-up form's reserve), and
    /// that size is computed before the session is born.
    pub(crate) scrollbar: ScrollbarMode,
    /// Inherited temporary point-size delta.
    pub(crate) zoom: Zoom,
    /// bateri's ssh masters: the application's registry, shared by every
    /// pane — one opening per host. `None` in a timed run: the remote jobs
    /// take today's argv.
    pub(crate) masters: Option<Arc<Masters>>,
    /// The bound holder's driver, the application's: the pane registers
    /// with it when its session is born and releases at its close. `None` in
    /// a timed run and an unbundled process.
    pub(crate) keeper: Option<Rc<dyn Holder>>,
    /// Who the application opening the pane is ([`Identity`]).
    pub(crate) identity: Rc<Identity>,
}

/// The half of the birth package that only [`TerminalPane::start`] consumes.
struct Birth {
    stats: Option<Arc<Stats>>,
    settings: Settings,
    theme: Theme,
    launch: Launch,
    integration: Integration,
}

/// Main-queue half of the title notification: the flag drops **before every
/// read** (a change arriving after the read wants a new job and is not
/// missed), then the pane's own edge (`edge`: the upload queue's connection)
/// and the owner re-read the title. The edge must also come after the flag:
/// an ssh that ends in between spawns no job and the queue would stay on a
/// dead connection. `swap`, because the read-modify-write
/// pairs with the writer's `swap` and makes what it wrote to the slot visible.
fn announce_title(pending: &AtomicBool, edge: impl FnOnce(), host: &dyn PaneHost, pane: u64) {
    pending.swap(false, Ordering::AcqRel);
    edge();
    host.title_changed(pane);
}

/// Main-queue half of the OSC 52 copy: the text in the slot goes to the owner
/// ([`PaneHost::copy_to_clipboard`]); if the slot is empty (another job took
/// it in a race) there is no event.
fn announce_copy(pending: &PendingCopy, host: &dyn PaneHost, pane: u64) {
    if let Some(text) = pending.take() {
        host.copy_to_clipboard(pane, text);
    }
}

/// Text on the system's find pasteboard (⌘E's cross-application
/// norm), through the same filter as ⌘E's query: its first line, `None` if
/// empty or only whitespace ([`selection_query`]).
fn find_pasteboard_text() -> Option<String> {
    // SAFETY: a constant name AppKit exposes, lives for the whole process.
    let name = unsafe { NSPasteboardNameFind };
    clipboard::read(&NSPasteboard::pasteboardWithName(name))
        .and_then(|text| selection_query(&text, false))
}

/// The six items that are greyed out on the alternate screen:
/// the two clear modes and the four scrolls — all touch the primary
/// scrollback and that scrollback is unreachable on the alternate screen.
fn is_scrollback_action(action: Sel) -> bool {
    [
        sel!(clearToStart:),
        sel!(clearScrollback:),
        sel!(scrollToTop:),
        sel!(scrollToBottom:),
        sel!(scrollPageUp:),
        sel!(scrollPageDown:),
    ]
    .contains(&action)
}

/// `bt-core`'s wake end — one per pane, together with its session.
///
/// `Session::spawn` asks for the `Wake` **before** the link, while the
/// `Waker` is born after the link; the slot's `None` closes the gap. No
/// frame is lost: the opening frame is requested by hand anyway and every
/// byte read up to that point has accumulated in the damage flag.
struct ShellWake {
    /// The pane's id: main-queue jobs find the pane with it
    /// ([`PaneLookup`], the pattern of the alternate-screen notifier) —
    /// holding a reference to the `Session` or the pane would close
    /// `wake.rs`'s ownership cycle.
    id: u64,
    /// The path from id to pane, from the owner ([`PaneLaunch::lookup`]).
    lookup: PaneLookup,
    /// Whether this is a timed run: `child_exit` splits into two paths
    /// ([`Wake::child_exit`]'s body). A digest of the recipe in the birth
    /// package (`PaneLaunch::run`); the reader thread cannot reach the pane.
    timed: bool,
    /// The link's `Waker` — under a **leaf lock** and **detachable**.
    ///
    /// When the pane closes it is `take()`n on the main thread
    /// ([`ShellWake::detach`]): the last copy of this object can drop on the
    /// `"PTY teardown"` thread (`wake.rs` → ownership) and if the `Waker`'s
    /// `MainThreadBound` dropped there, its `Drop` would throw a synchronous
    /// job at the main queue. A detached slot pins that `Drop` to the main
    /// thread structurally; before, the only protection was the window list
    /// outliving `app.run()`.
    ///
    /// The lock is a leaf: `wake()` takes it under the `Term` lock and
    /// releases it, and no other lock is taken under it (like `Theme`'s leaf
    /// lock).
    waker: Mutex<Option<Waker>>,
    /// Text OSC 52 has pending for the main queue. `Arc`, because the main
    /// queue's job wants `'static` and `Wake`'s call only gives `&self`; the
    /// job holds not `ShellWake` but only the slot.
    pending_copy: Arc<PendingCopy>,
    /// Whether the title job is waiting on the main queue — **at most one**
    /// job in the queue (`PendingCopy`'s pattern, a flag instead of a
    /// payload: the title itself is in the session, the job reads it).
    title_pending: Arc<AtomicBool>,
    /// Whether the search count's scrollback news is waiting on the main
    /// queue — `title_pending`'s twin.
    search_pending: Arc<AtomicBool>,
    /// Whether the command memory's turn is waiting on the main queue
    /// ([`TerminalPane::remember_commands`]) — `search_pending`'s twin: at most one job.
    memory_pending: Arc<AtomicBool>,
    /// The remote-session probe's arm and pending job; `Arc`, because
    /// the main queue's job holds it.
    remote_probe: Arc<RemoteProbe>,
    /// The login probe of the remote session: the same two bits as
    /// [`Self::remote_probe`] — armed on the remote edge while the user's ssh
    /// has not logged in, every output throws one check
    /// ([`TerminalPane::login_check`]); the login is the indicator's start.
    login_probe: Arc<RemoteProbe>,
    /// Whether the running program reads the keyboard itself: the same two
    /// bits again — armed on the `C` edge, settled by the raw modes
    /// ([`TerminalPane::probe_program`]) — but its job is **delayed**
    /// ([`PROBE_DELAY`]) and reads the moment it runs, so a burst of output
    /// is one probe and an idle pane runs none.
    program_probe: Arc<RemoteProbe>,
    /// Which program the pane runs, for its tab's icon: the same two bits again — armed on the
    /// `C` edge of a local command and on the edges of the remote shell's, a first look at once
    /// (the command's line is on the screen), then one look per [`IDENTITY_DELAY`] of output
    /// until the process table names the program ([`TerminalPane::probe_identity`]).
    identity_probe: Arc<RemoteProbe>,
    /// The listening ports' probe ([`PortProbe`]): on with `[shell] ports`,
    /// every edge (output, `C`, a prompt) schedules one delayed scan.
    pub(crate) port_probe: Arc<PortProbe>,
    /// Whether the environment's news is waiting on the main queue
    /// ([`TerminalPane::resolve_environment`]) — at most one job.
    environment_pending: Arc<AtomicBool>,
    /// Whether the stale-link news is waiting on the main queue —
    /// `search_pending`'s twin: at most one job.
    link_pending: Arc<AtomicBool>,
    /// Whether the news of a program's view is waiting on the main queue —
    /// `search_pending`'s twin: at most one job ([`TerminalPane::drain_views`]).
    views_pending: Arc<AtomicBool>,
    /// Whether the pointer shape's news is waiting on the main queue —
    /// `search_pending`'s twin: at most one job.
    pointer_pending: Arc<AtomicBool>,
    /// Whether the block index's news is waiting on the main queue —
    /// `search_pending`'s twin: at most one job.
    blocks_pending: Arc<AtomicBool>,
    /// Whether "Jump to latest"'s news is waiting on the main queue —
    /// `search_pending`'s twin: at most one job.
    unseen_pending: Arc<AtomicBool>,
    /// Whether the bootstrap's `up` check is waiting on the main queue
    /// ([`TerminalPane::check_remote_up`]) — at most one job.
    up_pending: Arc<AtomicBool>,
    /// Whether the "typed after the login" check is waiting on the main queue
    /// ([`TerminalPane::check_remote_typed`]) — at most one job.
    typed_pending: Arc<AtomicBool>,
    /// Whether a bound holder keeps the programs ([`Keeper::active_flag`]):
    /// the gate of the two state sends below — without a holder a shell's
    /// edges and keystrokes post nothing. `None` in a timed run.
    kept: Option<Arc<AtomicBool>>,
    /// Whether the state send of a shell edge is waiting on the main queue
    /// ([`TerminalPane::push_state`]) — at most one job.
    state_pending: Arc<AtomicBool>,
    /// Whether the delayed state send of a mirror change is waiting
    /// ([`MIRROR_DELAY`]) — at most one.
    mirror_pending: Arc<AtomicBool>,
    /// Whether the activity news ([`PaneHost::activity_changed`]) is waiting
    /// on the main queue — `title_pending`'s twin: at most one job, which
    /// reads the latest activity.
    activity_pending: Arc<AtomicBool>,
    /// Whether the bell's news ([`PaneHost::bell`]) is waiting on the main
    /// queue: a program ringing in a loop makes one job.
    bell_pending: Arc<AtomicBool>,
    /// Whether the notifications' news ([`PaneHost::program_notification`])
    /// is waiting: one job, which takes every notification waiting.
    notes_pending: Arc<AtomicBool>,
}

/// The two bits of the remote-session probe — and of the login
/// probe (the same semantics with "logged in" for "decided") and the program
/// probe (with "raw" for it, its job delayed): the **arm** (no
/// definitive answer yet for this command) and the **pending job** (a probe is
/// in the main queue — at most one, `title_pending`'s pattern).
///
/// The arm is set on the `C` edge; while set, every `wake` (output from the
/// PTY) throws a job, a definitive answer drops it and later output does not
/// probe. The cost of a running `cat` is a single probe.
///
/// **The job drops the arm before probing**, not after, and re-arms on an
/// undecided answer: while a probe runs, a new `C` on the reader thread can
/// set the arm and throw a new job, and if the ending old command's definitive
/// answer dropped it the new command would never be probed.
#[derive(Debug, Default)]
struct RemoteProbe {
    armed: AtomicBool,
    pending: AtomicBool,
}

impl RemoteProbe {
    /// The `C` edge: sets the arm; `true` if a job is to be thrown at the main queue.
    fn command_started(&self) -> bool {
        self.armed.store(true, Ordering::Release);
        self.claim()
    }

    /// The output edge (reader thread, possibly under the `Term` lock): `true`
    /// if the arm is set and no job is waiting. A single atomic read when unarmed.
    fn output(&self) -> bool {
        self.armed.load(Ordering::Acquire) && self.claim()
    }

    /// Takes the pending job's slot; `false` if one is already pending.
    fn claim(&self) -> bool {
        !self.pending.swap(true, Ordering::AcqRel)
    }

    /// The head of the main-queue job: releases the slot and drops the arm; if
    /// the arm is not set (a definitive answer was given) there is no probe.
    fn begin(&self) -> bool {
        self.pending.store(false, Ordering::Release);
        self.armed.swap(false, Ordering::AcqRel)
    }

    /// Undecided answer: the arm is set back, no job is thrown — the next
    /// output throws one. Also the login probe's arming on the remote edge.
    fn rearm(&self) {
        self.armed.store(true, Ordering::Release);
    }

    /// A claimed job that could not be scheduled gives its slot back; the arm
    /// stays, so the next edge tries again.
    fn release(&self) {
        self.pending.store(false, Ordering::Release);
    }
}

/// The listening ports' probe: whether it is **on** (`[shell] ports`; never in a
/// timed run) and the **pending** delayed job — at most one in the main
/// queue ([`PORTS_DELAY`]), `title_pending`'s pattern. Not armed per command
/// like [`RemoteProbe`]: a background server outlives its command, so every
/// edge may have opened or closed a port. An idle pane has no edge and runs
/// no scan; an edge costs one atomic swap.
#[derive(Debug, Default)]
pub(crate) struct PortProbe {
    on: AtomicBool,
    pending: AtomicBool,
}

impl PortProbe {
    fn new(on: bool) -> Self {
        Self {
            on: AtomicBool::new(on),
            pending: AtomicBool::new(false),
        }
    }

    /// An edge: `true` if the probe is on and no job is waiting — the caller
    /// schedules one.
    fn edge(&self) -> bool {
        self.on.load(Ordering::Acquire) && !self.pending.swap(true, Ordering::AcqRel)
    }

    /// The head of the job: releases the slot; `false` if the probe was
    /// turned off meanwhile.
    fn begin(&self) -> bool {
        self.pending.store(false, Ordering::Release);
        self.on.load(Ordering::Acquire)
    }

    /// A claimed job that could not be scheduled gives its slot back.
    fn release(&self) {
        self.pending.store(false, Ordering::Release);
    }

    pub(crate) fn is_on(&self) -> bool {
        self.on.load(Ordering::Acquire)
    }

    pub(crate) fn set_on(&self, on: bool) {
        self.on.store(on, Ordering::Release);
    }
}

/// How long after an edge (output, `C`, a prompt) the listening ports are
/// scanned — a **design constant**, not a measurement. A server prints its
/// address once its port is open (measured on `next dev`: the port is open
/// when `Local:` prints), so the scan after that line finds it; the delay
/// folds a burst of output into one scan and stays short next to the time it
/// takes to read the line and reach for the mouse.
const PORTS_DELAY: std::time::Duration = std::time::Duration::from_millis(300);

/// How long an unfinished block index waits for its next step while output
/// streams ([`TerminalPane::drive_chunk`]) — a display frame at 60 Hz, a
/// **design constant**: the frame path tells the index of output at most
/// once a frame, and a scan that keeps restarting under streaming output
/// must not take the main thread and `Term` turn after turn.
const BLOCK_PACE: std::time::Duration = std::time::Duration::from_millis(16);

/// How long after an edge (`C`, output) the program probe reads the PTY's
/// modes — a **design constant**, not a measurement.
///
/// Why not at once: at `C` the modes are still the shell's own, cooked from
/// ZLE's hand-off, and a REPL prints its banner in canonical mode and goes
/// raw only for its first prompt. A job that read at the edge would say "not
/// raw" exactly when the program is about to be; read a little later, it
/// sees the program's own mode, and the prompt printed after the switch is a
/// new edge that schedules the next look. The value folds a burst of output
/// into one probe and stays well under the time a person takes to start
/// typing at a prompt that just appeared, so the dock is gone before the first
/// key. A program that goes raw after its last output and later than this
/// stays unseen — the dock stays, today's behaviour, the safe direction.
const PROBE_DELAY: std::time::Duration = std::time::Duration::from_millis(100);

/// How long the identity probe lets a burst of output gather before it looks at the process
/// table again. Slower than [`PROBE_DELAY`]: a command whose program the table does not know (a
/// build tool's helper, a script of its own) is asked once per delay for as long as it prints,
/// and the icon is not a decision the next keystroke waits for.
const IDENTITY_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

/// How many times the identity probe looks at the process table for one command before it gives
/// up on a program nobody names: at least ten seconds of output at [`IDENTITY_DELAY`].
const IDENTITY_LOOKS: u16 = 40;

impl ShellWake {
    /// Throws the remote-session probe to the main queue ([`RemoteProbe`]).
    fn dispatch_remote_probe(&self) {
        let probe = Arc::clone(&self.remote_probe);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            if !probe.begin() {
                return;
            }
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            // If the pane closed in the meantime there is no shell to probe.
            let Some(pane) = lookup(mtm, id) else {
                return;
            };
            let outcome = pane.probe_remote();
            // The remote state's edge is the edge of the upload queue, the
            // window title and the tab's label ([`TerminalPane::remote_or_title_changed`]).
            if outcome.changed {
                pane.remote_or_title_changed();
            }
            if outcome.undecided {
                probe.rearm();
            }
        });
    }

    /// Throws the login check to the main queue ([`ShellWake::login_probe`]).
    fn dispatch_login_probe(&self) {
        let probe = Arc::clone(&self.login_probe);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            if !probe.begin() {
                return;
            }
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            let Some(pane) = lookup(mtm, id) else {
                return;
            };
            if pane.login_check() {
                probe.rearm();
            }
        });
    }

    /// Schedules the listening ports' scan on the main queue **after
    /// [`PORTS_DELAY`]** if the probe is on and none is waiting
    /// ([`PortProbe`]); the job releases its slot before scanning, so an edge
    /// that arrives meanwhile schedules the next one.
    fn poke_ports(&self) {
        if !self.port_probe.edge() {
            return;
        }
        let Ok(when) = DispatchTime::try_from(PORTS_DELAY) else {
            self.port_probe.release();
            return;
        };
        let probe = Arc::clone(&self.port_probe);
        let (id, lookup) = (self.id, self.lookup);
        let _ = DispatchQueue::main().after(when, move || {
            if !probe.begin() {
                return;
            }
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.scan_ports();
            }
        });
    }

    /// Throws the program probe to the main queue **after [`PROBE_DELAY`]**
    /// ([`ShellWake::program_probe`]); the job drops its slot before reading,
    /// so an edge that arrives while it runs schedules the next look.
    fn dispatch_program_probe(&self) {
        let Ok(when) = DispatchTime::try_from(PROBE_DELAY) else {
            self.program_probe.release();
            return;
        };
        let probe = Arc::clone(&self.program_probe);
        let (id, lookup) = (self.id, self.lookup);
        let _ = DispatchQueue::main().after(when, move || {
            if !probe.begin() {
                return;
            }
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            let Some(pane) = lookup(mtm, id) else {
                return;
            };
            if pane.probe_program() {
                probe.rearm();
            }
        });
    }

    /// Throws the identity probe to the main queue ([`ShellWake::identity_probe`]): at once for a
    /// command's start (`delay` is `None`), after [`IDENTITY_DELAY`] for output. A look that
    /// changed what the pane is says so to the tab; one that could not tell arms the next.
    fn dispatch_identity_probe(&self, delay: Option<std::time::Duration>) {
        let probe = Arc::clone(&self.identity_probe);
        let (id, lookup) = (self.id, self.lookup);
        let job = move || {
            if !probe.begin() {
                return;
            }
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            let Some(pane) = lookup(mtm, id) else {
                return;
            };
            let outcome = pane.probe_identity();
            if outcome.changed {
                pane.host().program_changed(id);
                pane.push_state();
            }
            if outcome.undecided {
                probe.rearm();
            }
        };
        match delay {
            None => DispatchQueue::main().exec_async(job),
            Some(delay) => {
                let Ok(when) = DispatchTime::try_from(delay) else {
                    self.identity_probe.release();
                    return;
                };
                let _ = DispatchQueue::main().after(when, job);
            }
        }
    }

    /// Takes the leaf lock; if poisoned it continues with what is inside — the
    /// slot's only invariant is "either a `Waker` exists or not" and it cannot
    /// have a half-written state.
    fn slot(&self) -> MutexGuard<'_, Option<Waker>> {
        self.waker.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Detaches the `Waker` from the slot and hands it to the caller — so that
    /// it drops on the main thread ([`ShellWake::waker`]). The second call is `None`.
    fn detach(&self) -> Option<Waker> {
        self.slot().take()
    }

    /// A command's edge: the owner reads the pane's activity again on the
    /// next main-queue turn ([`PaneHost::activity_changed`]). The flag drops
    /// before the job reads, so an edge after the read schedules the next.
    fn announce_activity(&self) {
        if self.activity_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.activity_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.swap(false, Ordering::AcqRel);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.host().activity_changed(id);
            }
        });
    }

    /// Whether a holder keeps this pane's program — one atomic read.
    fn kept(&self) -> bool {
        !self.timed
            && self
                .kept
                .as_ref()
                .is_some_and(|kept| kept.load(Ordering::Acquire))
    }

    /// A command started: the main queue's next turn reads its line into the pane's command
    /// memory ([`bt_core::Session::remember_commands`]) while the rows that carry it are still
    /// on the grid. At most one job waits; a second start folds into it (the memory reads
    /// every command since its last look). Reader thread, the `Term` lock may be held.
    fn remember_soon(&self) {
        if self.memory_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.memory_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.store(false, Ordering::Release);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.remember_commands();
            }
        });
    }

    /// A shell edge: the pane's state goes to the holder on the next
    /// main-queue turn — `state_blob` takes the ledger's lock and must not
    /// run here (reader thread, possibly under the `Term` lock).
    fn push_state_soon(&self) {
        if !self.kept() || self.state_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.state_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.store(false, Ordering::Release);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.push_state();
            }
        });
    }

    /// A mirror change: the pane's state goes after [`MIRROR_DELAY`], once
    /// per interval however fast the keys come. The flag drops when the job
    /// runs, before the read: a change after it schedules the next.
    fn push_state_later(&self) {
        if !self.kept() || self.mirror_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let Ok(when) = DispatchTime::try_from(MIRROR_DELAY) else {
            self.mirror_pending.store(false, Ordering::Release);
            return;
        };
        let pending = Arc::clone(&self.mirror_pending);
        let (id, lookup) = (self.id, self.lookup);
        let _ = DispatchQueue::main().after(when, move || {
            pending.store(false, Ordering::Release);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.push_state();
            }
        });
    }
}

impl Wake for ShellWake {
    fn wake(&self) {
        // Reader thread; the `Term` lock may be held. One job: throw the
        // "open the link" job to the main queue and return at once.
        // The waker is **not copied**, it is called under the lock: a copy
        // could end up as the last reference here and drop on the reader
        // thread — the very path detaching closed.
        if let Some(waker) = self.slot().as_ref() {
            waker.wake();
        }
        // If the remote-session probe stayed undecided this output re-triggers
        // it; when unarmed the cost is one atomic read.
        if self.remote_probe.output() {
            self.dispatch_remote_probe();
        }
        // The user's ssh logs in with output (a prompt, the MOTD): one check
        // per output edge while armed; one atomic read otherwise.
        if self.login_probe.output() {
            self.dispatch_login_probe();
        }
        // A program that goes raw prints its prompt after the switch: while
        // armed, an edge with no look pending schedules one.
        if self.program_probe.output() {
            self.dispatch_program_probe();
        }
        // The program the process table has not named yet: one look per burst of output.
        if self.identity_probe.output() {
            self.dispatch_identity_probe(Some(IDENTITY_DELAY));
        }
        // A server prints its address once it listens; a burst is one scan.
        self.poke_ports();
    }

    fn child_exit(&self, code: Option<i32>) {
        // The shell is gone, the pane has nothing to stand on: **that window**
        // closes, not the application. Closing goes through the
        // window's own `windowWillClose:` — the red button, ⌘W and `exit` reach
        // the same sequence.
        //
        // **In a timed run** the old path: directly `terminate:`. The report
        // reads the window's counters and if the window dropped off the list
        // before the report, when the smoke recipe ended shorter than the
        // deadline the report would run with an empty list (`will_terminate`'s doc).
        //
        // It is thrown to the main queue for two reasons and both are required:
        // AppKit wants the main thread, and this call arrives **on the reader
        // thread** — a synchronous path to closing would make the reader thread
        // wait on its own closing (`wake.rs` → ownership).
        //
        // **Known limit:** the shell's last output may not reach the screen.
        // alacritty orders it `ChildExit` → `Wakeup`, so when we get here the
        // last byte may not have been drawn yet; and closing runs without a
        // vsync in between. Guaranteeing it would be either a magic delay or
        // adding "damage exhausted, exit now" semantics to the display link —
        // the second would put terminal knowledge into the renderer. When the
        // `bateri -e cmd` path arrives it will be designed together with
        // `drain_on_exit`.
        let (timed, id, lookup) = (self.timed, self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if timed {
                NSApplication::sharedApplication(mtm).terminate(None);
                return;
            }
            // If the pane closed in the meantime (⌘W's `SIGHUP` killed the
            // shell and the news came later) there is nothing to close. Closing
            // is the owner's job ([`PaneHost::shell_exited`]).
            if let Some(pane) = lookup(mtm, id) {
                pane.ivars().exit_status.set(code);
                pane.host().shell_exited(id);
            }
        });
    }

    fn copy_to_clipboard(&self, text: String) {
        // Reader thread, the `Term` lock may be held: the text goes to the
        // lock-free slot, at most **one** job to the main queue (`PendingCopy`'s
        // doc). If there is text waiting in the slot the job that will take it
        // is already in the queue.
        //
        // The owner chooses the pasteboard ([`PaneHost::copy_to_clipboard`];
        // default the general pasteboard, the same as Cmd-C's); the job's order
        // is for the same reason as `child_exit`'s: the main queue. If the pane
        // closed in the meantime the text is dropped.
        if self.pending_copy.put(text) {
            let pending = Arc::clone(&self.pending_copy);
            let (id, lookup) = (self.id, self.lookup);
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(pane) = lookup(mtm, id) {
                    announce_copy(&pending, pane.host(), id);
                }
            });
        }
    }

    fn title_changed(&self) {
        // Reader thread (or the settings watcher's thread), the `Term` lock
        // may be held: set the flag, return if a job is already waiting.
        if self.title_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.title_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            // If the pane closed in the meantime there is no title to write either
            // (the flag stays set; a closed pane's news is dropped anyway).
            if let Some(pane) = lookup(mtm, id) {
                // The remote state's edge (`D`/`A`'s deletion) is also the
                // upload queue's edge — it first, then the owner reads the title.
                announce_title(&pending, || pane.remote_edge(), pane.host(), id);
            }
        });
    }

    fn search_changed(&self) {
        // Reader thread, the `Term` lock may be held (or the main thread's
        // `resize`): `title_changed`'s pattern — at most one job in the main
        // queue. The core already reports on the edge; this flag folds the
        // second of two reports while the job waits in the queue.
        if self.search_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.search_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.swap(false, Ordering::AcqRel);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            // Also works in a background tab: the news is not tied to the frame
            // path. If the pane closed in the meantime there is nothing to count.
            if let Some(pane) = lookup(mtm, id) {
                pane.kick_search();
            }
        });
    }

    fn command_started(&self) {
        // The tab's ring hears every start; it asks for no frame, so the
        // timed run's tokens are not its concern.
        self.announce_activity();
        // Reader thread, lock-free. A timed run does not detect: its tokens
        // must stay as today (and the fixed script has no integration either).
        if self.timed {
            return;
        }
        self.push_state_soon();
        self.remember_soon();
        if self.remote_probe.command_started() {
            self.dispatch_remote_probe();
        }
        if self.program_probe.command_started() {
            self.dispatch_program_probe();
        }
        if self.identity_probe.command_started() {
            self.dispatch_identity_probe(None);
        }
        self.poke_ports();
    }

    fn phase_edge(&self) {
        // Reader thread, lock-free: a prompt or a command's end — the
        // holder's copy of the state must not lag a command behind, and the
        // tab's ring stops or its "finished" mark comes.
        self.push_state_soon();
        self.announce_activity();
        // A command's end: its servers stopped listening, or a background
        // one started without a word.
        self.poke_ports();
    }

    fn remote_command_edge(&self) {
        // Reader thread, lock-free: a command on the far end of ssh started
        // or ended — the tab's indicator alone hears it, and its icon: the command's line is
        // all there is to know the program by on this side.
        self.announce_activity();
        if !self.timed {
            self.remember_soon();
        }
        if !self.timed && self.identity_probe.command_started() {
            self.dispatch_identity_probe(None);
        }
    }

    fn mirror_changed(&self) {
        // Reader thread, per keystroke: one delayed send per interval.
        self.push_state_later();
    }

    fn environment_changed(&self) {
        // Reader thread, after the ledger's lock: the report or its directory
        // changed — one job, which reads the report itself.
        if self.timed || self.environment_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.environment_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.store(false, Ordering::Release);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.resolve_environment();
            }
        });
    }

    fn bell(&self) {
        // Reader thread, under the `Term` lock: one job, no lock taken here.
        if self.timed || self.bell_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.bell_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.store(false, Ordering::Release);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.host().bell(id);
            }
        });
    }

    fn notification(&self) {
        // Reader thread: one job, which takes what waits in the ledger.
        if self.timed || self.notes_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.notes_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.store(false, Ordering::Release);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            let Some(pane) = lookup(mtm, id) else {
                return;
            };
            let Some(session) = pane.session().cloned() else {
                return;
            };
            for note in session.take_notifications() {
                pane.host()
                    .program_notification(id, note.title.as_deref(), &note.body);
            }
        });
    }

    fn program_status_changed(&self) {
        // Reader thread, lock-free: a program said what it is doing — the
        // tab's ring, its waiting mark and its finished tick hear it, and the
        // holder, which carries the program it named on.
        self.announce_activity();
        self.push_state_soon();
    }

    fn remote_up(&self) {
        // Reader thread, lock-free: `command_started`'s gate (a timed run
        // learns nothing) and `title_changed`'s at-most-one job.
        if self.timed || self.up_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.up_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.store(false, Ordering::Release);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.check_remote_up();
            }
        });
    }

    fn remote_typed(&self) {
        // The main thread, inside the input call: `remote_up`'s gate and its
        // at-most-one job, the check on the next main-queue turn.
        if self.timed || self.typed_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.typed_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.store(false, Ordering::Release);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.check_remote_typed();
            }
        });
    }

    fn link_hover_lost(&self) {
        // The frame path (main thread, after the `Term` lock): the hover's stamp
        // went stale and the slot was dropped. The view re-finds the link if ⌘
        // is still down (`BateriView::link_lost`) — on the next main-queue turn,
        // not inside the frame; at most one job (`search_changed`'s pattern).
        if self.link_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.link_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.swap(false, Ordering::AcqRel);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.view().link_lost();
            }
        });
    }

    fn view_changed(&self) {
        // The reader thread (or the thread of the call the news answers), and the
        // `Term` lock may be held — a command's end or the child's exit that tore
        // the view down says so under it — while the desk's own locks are not: a
        // program's view has something for the pane, or a wait that only the pane
        // has a clock for. Nothing is taken here (the three prohibitions of
        // `Wake`); the pane takes everything on the next main-queue turn — no
        // frame; at most one job (`search_changed`'s pattern). The flag goes down
        // **before** the take, so news that comes while the pane acts on it is
        // another job.
        if self.views_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.views_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.swap(false, Ordering::AcqRel);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.drain_views();
            }
        });
    }

    fn pointer_changed(&self) {
        // The reader thread, under the `Term` lock or right after the scan:
        // the shape the program asks for changed. The view redoes its cursor
        // rectangles on the next main-queue turn — no frame; at most one job
        // (`search_changed`'s pattern).
        if self.pointer_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.pointer_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.swap(false, Ordering::AcqRel);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.view().sync_cursor_rects();
            }
        });
    }

    fn blocks_changed(&self) {
        // The frame path (main thread, after the `Term` lock): the history
        // moved while the block marks are wanted. The index is driven on the
        // next main-queue turn, not inside the frame; at most one job
        // (`search_changed`'s pattern).
        if self.blocks_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.blocks_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.swap(false, Ordering::AcqRel);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.kick_search();
            }
        });
    }

    fn unseen_changed(&self) {
        // The frame path (main thread, after the `Term` lock): the count of
        // lines below a scrolled window changed. The button follows on the
        // next main-queue turn, not inside the frame; at most one job
        // (`search_changed`'s pattern) — the job reads the latest count.
        if self.unseen_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.unseen_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.swap(false, Ordering::AcqRel);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.refresh_jump();
            }
        });
    }
}

/// `bt-gpu`'s alternate-screen notifier: throws the work **to the main queue**.
///
/// Its call comes from the frame path, i.e. already from the main thread —
/// the queue is not for a thread hop but to **defer by one turn**: at the
/// moment of the call the frame has been drawn and changing the window
/// geometry (drawable size, grid, `DisplayLink` layout) there would pull the
/// rug from under the drawn frame.
///
/// **Pane id, not a targetless action.** The responder chain goes to the key
/// window: exiting vim in a background tab would resize the wrong pane. The
/// job captures the id (`id`), finds the pane through the owner's path
/// ([`PaneLookup`]) and drops if it cannot — if the pane closed in the
/// meantime there is nothing to resize either.
///
/// The only thing it captures is an integer. The reason for the old "captures
/// nothing" rule was a **reference cycle** (`DisplayLink` sits in the pane's
/// ivar, a closure holding the pane would tie it to itself); an integer opens
/// no cycle. Besides, `exec_async` wants `Send` and the pane object is pinned
/// to the main thread — there was nothing else it could hold anyway.
///
/// It carries no payload: the receiver re-reads the truth
/// ([`TerminalPane::alt_screen_did_change`]), so two transitions chasing each
/// other (vim open-close) cannot act on a stale value.
fn alt_screen_notifier(id: u64, lookup: PaneLookup) -> Box<dyn Fn()> {
    Box::new(move || {
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.alt_screen_did_change();
            }
        });
    })
}

/// `bt-gpu`'s block-marks notifier ([`alt_screen_notifier`]'s pattern): the
/// drawn block marks changed under a still pointer, so the view asks again
/// where the pointer is — on the next main-queue turn, not inside the frame.
fn marks_notifier(id: u64, lookup: PaneLookup) -> Box<dyn Fn()> {
    Box::new(move || {
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.view().recheck_block_hover();
            }
        });
    })
}

/// Whether the identity probe looks again after `looks` looks at the process table of one
/// command, `wait` being what the table left it ([`after_table`]): a command nobody names (a
/// server, `tail -f`) is looked at this many times while it prints and then left alone — what it
/// says of itself (`OSC 7501`) is still heard.
fn keep_looking(wait: bool, looks: u16) -> bool {
    wait && looks < IDENTITY_LOOKS
}

/// What a look at the process table leaves the identity probe to keep and to do: the program it
/// named, if it did, and whether to look again at the next output — only while neither it nor the
/// command's line (or the decision carried over from the bateri before) has named one. A line
/// that names a program is enough to rest on: the table would only second it. A shell in the
/// foreground is no answer and looks again: it is most often a program's launcher in the moment
/// before it replaces itself with the program (Pi's `sh`, Cursor CLI's `bash`), and kept as one
/// it would stand over the line that named the program.
fn after_table(seen: Seen, line: Option<Kind>) -> (Option<Kind>, bool) {
    match seen {
        Seen::Kind(Kind::Shell) => (None, true),
        Seen::Kind(kind) => (Some(kind), false),
        Seen::Waiting | Seen::Nothing => (None, line.is_none()),
    }
}

/// The program of the command that just ended, from what [`TerminalPane::program_kind`] last saw
/// (`ran`: the command it was seen during, and the kind) and the newest command now: what it saw
/// counts only if that is the command that ended; anything older is a command that came and went
/// unseen, and the pane was at its prompt. By the command, not by a count of ends: a program that
/// says it is done and then the command's own end are two ends of one command, and they may be
/// noticed together.
fn kind_of_end<K: PartialEq>(ran: Option<(K, Kind)>, newest: Option<K>) -> Kind {
    ran.filter(|(seen, _)| newest.as_ref() == Some(seen))
        .map_or(Kind::Shell, |(_, kind)| kind)
}

/// The kind carried over from the bateri before (`carried`: the command it ran then, and the
/// kind) while `newest` is still that command; any other command is one it never saw.
fn carried_kind<K: PartialEq>(carried: Option<(K, Kind)>, newest: Option<K>) -> Option<Kind> {
    carried
        .filter(|(seen, _)| newest.as_ref() == Some(seen))
        .map(|(_, kind)| kind)
}

/// What the pane has read of the program it runs, kept for the tab's icon
/// ([`TerminalPane::program_kind`]). Main thread only.
#[derive(Clone, Copy, Debug, Default)]
struct ProgramMemo {
    /// What the line of the newest command (ours or the remote shell's) said, by its id: read
    /// once, from the screen, when the command was first looked at — the row scrolls away.
    line: Option<(CommandId, Option<Kind>)>,
    /// What the process table named, by the local command's generation. Only a program it named
    /// is kept: "nothing known" is asked again.
    group: Option<(u64, Kind)>,
    /// What the pane was last seen to run, and during which command: the program of the command
    /// that ends next, which is what the tab keeps showing for it ([`TerminalPane::ended_kind`]).
    ran: Option<(CommandId, Kind)>,
    /// How many times the process table was looked at for the local command of this generation
    /// without naming a program (bounded by [`IDENTITY_LOOKS`]).
    looks: Option<(u64, u16)>,
    /// What the bateri this pane was carried over from showed it ran, by the command that ran
    /// then: by now that command's line has mostly scrolled away and the table does not see
    /// past `ssh`, so the old decision stands in for the line while that command runs.
    carried: Option<(CommandId, Kind)>,
}

/// Result of the remote-session probe ([`TerminalPane::probe_remote`]): two
/// separate answers, because their consumers are separate — undecidedness
/// re-arms (the pane's job), a change in the remote state refreshes the title
/// and the tab's label (the window's job).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RemoteProbeOutcome {
    /// The answer is undecided: the arm stays set, the next output probes again.
    pub(crate) undecided: bool,
    /// The session's remote state changed (the return of `Session::set_remote`).
    pub(crate) changed: bool,
}

/// The pane's state. `OnceCell`: the session and link are born once inside
/// `start`, then only read. The view, surface and renderer are born in the
/// constructor.
///
/// **The window is not held here**: the pane is the window's `contentView`,
/// i.e. the window holds it strongly and a back reference would be a cycle.
/// The window is looked at with `NSView::window` whenever needed (scale, key bit).
pub struct PaneIvars {
    /// Our own counter ([`AppDelegate`] hands it out, from the same counter as
    /// the windows'): the key by which jobs returning from the reader thread
    /// to the main queue find the pane (`AppDelegate::pane`).
    id: u64,
    /// The timed run's recipe, from the birth package (`Copy`): the focus path
    /// asks it on every application switch ([`TerminalPane::apply_focus`]).
    run: Option<Run>,
    /// Owner of the events ([`PaneHost`]).
    host: Rc<dyn PaneHost>,
    /// The path by which main-queue returns find the pane ([`PaneLookup`]).
    lookup: PaneLookup,
    /// The half of the birth package that `start` consumes; `None` after `start`.
    birth: RefCell<Option<Birth>>,
    /// The settings' font — the point-size delta is applied to it
    /// ([`TerminalPane::change_zoom`]); its live change is [`TerminalPane::set_font`].
    font: RefCell<FontOptions>,
    /// Resolved value of Reduce Motion — the search panel's animation looks
    /// at it too; its live change is [`TerminalPane::set_reduce_motion`].
    reduce_motion: Cell<bool>,
    /// Resolved mode of the wheel — the search's trip to a match looks at it
    /// too; its live change is [`TerminalPane::set_smooth_scroll`].
    smooth_scroll: Cell<bool>,
    /// Resolved form of the scroll bar: the grid's reserve
    /// ([`TerminalPane::sync_geometry`]) reads it from the first geometry on,
    /// the link gets it in `start_session`; its live change is
    /// [`TerminalPane::set_scrollbar_mode`].
    scrollbar: Cell<ScrollbarMode>,
    /// What the content does at the pane's top edge (`[appearance]
    /// content_edge`), the scroll bar's sibling: the grid's top reserve
    /// ([`TerminalPane::sync_geometry`]) reads it from the first geometry on,
    /// so it is the birth settings' from `new`; the link gets it in
    /// `start_session`; its live change is [`TerminalPane::set_content_edge`].
    content_edge: Cell<ContentEdge>,
    /// The radii, in points, of the pane's bottom left and right corners:
    /// what the container tells it ([`TerminalPane::set_corners`]) and the
    /// grid's and the dock's side margin follows ([`CellMetrics::with_corners`]).
    /// Zero until told: a square corner, the gutter alone.
    corners: Cell<(f64, f64)>,
    /// `Rc`: the renderer is pinned to the main thread (see `bt_gpu::DisplayLink`)
    /// and the link holds a copy too.
    renderer: Rc<Renderer>,
    /// The terminal view's layer — this pane owns it: it is
    /// hung on the view here and its scale is set from the window
    /// (`sync_geometry`); `bt-gpu` draws into it through [`Surface`].
    layer: Retained<CAMetalLayer>,
    /// The wgpu surface over `layer`; shared with the link, which acquires
    /// each frame's texture from it.
    surface: Rc<Surface>,
    /// The inputs of the mouse translation are refreshed with the pane's size
    /// (`set_metrics`); this view is also the source of the geometry (`sync_geometry`).
    view: Retained<BateriView>,
    /// The unfocused pane's dim veil (`[appearance] dim_unfocused_splits`),
    /// a sibling of the Metal layer — it is not in the frame path, its
    /// composition is CoreAnimation's. The owner determines its visibility.
    dim: Retained<DimOverlay>,
    /// The veil over the **grid** while the connection to the server of the
    /// terminal the pane shows is gone ([`TerminalPane::set_connection`]): not
    /// the dock — its context row says what is happening and stays at full
    /// clarity. CoreAnimation's, like [`PaneIvars::dim`]: it asks for no frame.
    link_veil: Retained<DimOverlay>,
    /// How the connection stands for the terminal the pane shows, as the
    /// application's bridge last said (`Live` for a pane that shows its own
    /// shell). Passive states ([`PaneLink::is_passive`]) take the keys.
    connection: Cell<PaneLink>,
    /// The card's frame: the topmost child, above the veil
    /// ([`crate::card`]). Invisible unless the pane is one of several.
    card_frame: Retained<FrameBox>,
    /// The ⌘-hovered OSC 8 link's target and its text: above the
    /// terminal and the search panel, below the dim veil. AppKit's, outside the
    /// frame path; shown by [`TerminalPane::set_link_target`].
    link_label: (Retained<LinkLabel>, Retained<NSTextField>),
    link: OnceCell<DisplayLink>,
    /// What a still of the pane keeps between two ([`TerminalPane::snapshot`]): its frame, its
    /// buffers and the textures the picture is shrunk through. Empty until a card first asks and
    /// given back when the card goes ([`TerminalPane::release_snapshot`]).
    shot: RefCell<Option<Shot>>,
    /// The second step of the closing sequence is called from here; `DisplayLink`
    /// holds a copy too but reaching there after `stop()` would be wrong.
    session: OnceCell<Arc<Session>>,
    /// Whether the shell is the PTY's child or its child's child — written at
    /// the same moment as the session, **from the command**
    /// ([`TerminalPane::start_session`]); detecting the running job finds the
    /// shell with it ([`TerminalPane::foreground`]).
    shell_parent: OnceCell<ShellParent>,
    wake: Arc<ShellWake>,
    /// Cmd +/−/0's temporary point-size delta — **this pane's**: the font that
    /// goes to the renderer is `zoom.apply(&settings.font)` ([`TerminalPane::apply_font`]).
    /// Reset when `size` in the file changes ([`TerminalPane::zoom_after_reload`]).
    zoom: Cell<Zoom>,
    /// How many rows the dock has; `0` → this pane has no dock.
    ///
    /// **Decided when the session is born**: the source is
    /// whether the integration was installed and it is asked **once** in
    /// [`TerminalPane::start`]. The slot exists for that reason: `sync_geometry`
    /// runs on every geometry event and must know the answer when computing the
    /// grid height; asking a second time would mean, in a future where the two
    /// calls can diverge, "the pane lost two rows but there is no dock".
    ///
    /// Consequence: the smoke recipe running `/bin/sh` **does not get** a dock,
    /// so `smoke_shell` and the `cells=8 glyphs=6 rules=15` contract tied to it
    /// stay untouched.
    ///
    /// `Cell`, not `OnceCell`: its pre-launch value is `0` and that is the
    /// **right** answer (no session yet, no first frame either); `OnceCell`
    /// would close this path with an `unwrap`.
    ///
    /// **This field is the current reserve**, not the birth value: it drops to
    /// zero on the alternate screen and comes back on exit. The birth
    /// value is in a separate field ([`PaneIvars::dock_rows_at_birth`]) and
    /// keeping the two apart is required — otherwise leaving the alternate
    /// screen would conjure a dock in a pane that never had one.
    dock_rows: Cell<u16>,
    /// The dock reserve decided when the session is born: `DOCK_ROWS` if the
    /// integration was installed, `0` if not.
    ///
    /// It does **not move** during the run; this is the value the alternate
    /// screen will bring back and its only writer is the session's birth.
    dock_rows_at_birth: Cell<u16>,
    /// The session's persistent identity: goes to the
    /// shell as `TERM_SESSION_ID` and `BATERI_TAB_URL`, `bateri://tab/<id>`
    /// finds the pane with it. Constant for the pane's lifetime; the
    /// in-process [`id`] is a separate thing (the key of main-queue returns).
    uuid: PaneUuid,
    /// Closing has begun ([`TerminalPane::begin_close`]): the pane's window
    /// leaves the list one turn later (`forget_window`) and in the meantime
    /// `AppDelegate::pane` must not find it — so that a stale report from the
    /// reader thread does not act on a closed session and `bateri://tab/`
    /// does not bring a sessionless window to the screen.
    closed: Cell<bool>,
    /// The user chose **Leave Running** for this pane ([`TerminalPane::leave_running`]):
    /// the close that follows lets the terminal a server keeps for it run on instead of
    /// ending it. Spent by that close ([`TerminalPane::end_kept_terminal`]).
    leaving: Cell<bool>,
    /// The scrollback search's panel — born on the first ⌘F: a pane
    /// that never searches carries no views. The query and keys are in the
    /// panel, i.e. **per pane**, and are not forgotten on close.
    search: OnceCell<SearchBar>,
    /// The state of the last query given to the session — the label's input.
    search_status: Cell<SearchStatus>,
    /// Whether the driver of the two indexes — the search's count and the
    /// scroll bar's blocks — is waiting one turn in the main queue
    /// ([`TerminalPane::kick_search`]): so that a second driver is not set
    /// up.
    driving: Cell<bool>,
    /// The pointer over the scroll bar's strip, and the thumb held — what
    /// the view last said ([`TerminalPane::set_scrollbar_hover`],
    /// [`TerminalPane::set_scrollbar_drag`]); with the form and the pane's
    /// visibility, whether the block marks are wanted.
    bar_pointer: Cell<(bool, bool)>,
    /// Whether the pane can be seen — [`TerminalPane::set_visible`]'s last
    /// word; a pane is born seen.
    seen: Cell<bool>,
    /// The view a program in this pane shows, over the terminal, while it does
    /// ([`TerminalPane::drain_views`]); the terminal's frames stop for as long as
    /// it is there.
    pencere: RefCell<Option<ViewHost>>,
    /// The deadlines set on the conversation with that program's view
    /// ([`TerminalPane::wait_for_view`]).
    view_waits: RefCell<Waits>,
    /// The questions the program's view has put to the person and that are up
    /// ([`pencere_view::ask`]).
    view_asking: pencere_view::ask::Asking,
    /// The size, in points, the conversation was last told the view's container
    /// is ([`TerminalPane::sync_view_size`]).
    view_size: Cell<(u32, u32)>,
    /// The shell's exit code once it exited normally; `None` before, and for one killed by a
    /// signal ([`TerminalPane::exit_status`]).
    exit_status: Cell<Option<i32>>,
    /// Whether the block marks are wanted, as last told to the session
    /// ([`TerminalPane::refresh_marks_wanted`]).
    marks_wanted: Cell<bool>,
    /// When the block index last stepped while output streamed and it was
    /// unfinished — the start of its pace ([`TerminalPane::blocks_chunk`]);
    /// `None` otherwise.
    block_paced: Cell<Option<Instant>>,
    /// The scroll bar block mark's tip ([`BlockTipParts`]) and the mark it
    /// shows — the same mark again is a no-op.
    block_tip: BlockTipParts,
    tip_for: Cell<Option<BlockHandle>>,
    /// The tip's time of day: the user's short time style (12 or 24 hours),
    /// made at the first tip.
    time_format: OnceCell<Retained<NSDateFormatter>>,
    /// "Jump to latest" ([`JumpLatest`]): above the terminal, under the
    /// dim veil.
    jump: JumpLatest,
    /// "Jump to latest" was clicked and the window is on its way down, with
    /// the count it had then: the button stays hidden until the count is
    /// zero — output during the glide must not bring it back for a moment
    /// — or another scroll input takes the window
    /// ([`TerminalPane::poke_scrollbar`]).
    jumping: Cell<Option<u32>>,
    /// Upload of a Finder drop to the remote directory: queue,
    /// progress and result line ([`crate::upload::Transfers`]). The queue is
    /// **this pane's ssh connection's** — switching to another tab does not stop it.
    uploads: RefCell<Transfers>,
    /// The open upload sheet (confirmation or error): lives for the sheet's duration.
    upload_alert: RefCell<Option<Retained<NSAlert>>>,
    /// The open stop question ([`crate::uploader`]).
    upload_stop: RefCell<Option<StopSheet>>,
    /// The open password sheet ([`crate::password_sheet`]): its sender is
    /// what the waiting job's thread blocks on — dropping it answers "nobody".
    password: RefCell<Option<PasswordSheet>>,
    /// The application's ssh masters ([`PaneLaunch::masters`]).
    masters: Option<Arc<Masters>>,
    /// The bound holder's driver ([`PaneLaunch::keeper`]).
    keeper: Option<Rc<dyn Holder>>,
    /// Who the application that opened the pane is ([`PaneLaunch::identity`]).
    identity: Rc<Identity>,
    /// The socket of the holder a carried-on pane was taken from, until
    /// its registration ([`TerminalPane::register_with_holder`]): the new
    /// holder must not drain the master while that one still does.
    taken_from: RefCell<Option<PathBuf>>,
    /// What made the pane a program's, from its start ([`crate::launch::Held`]): it closes when
    /// its terminal ends, and who asked for it. Session restore saves both, so a pane born again
    /// after a restart is still that program's.
    asked_for: RefCell<AskedFor>,
    /// The session's journal in shared memory, for a pane born while a
    /// bound holder kept the programs: after a crash the holder rebuilds the
    /// screen from it. `None` for every other pane.
    journal: RefCell<Option<PaneJournal>>,
    /// The remote generation this pane reported to the masters' registry
    /// ([`Masters::session_started`]); `None` locally.
    ssh_session: Cell<Option<u64>>,
    /// The wrapped `ssh` the probe found and whether its server was recorded
    /// ([`TerminalPane::check_remote_up`]).
    wrap_proof: RefCell<WrapProof>,
    /// What was read of the program the pane runs ([`ProgramMemo`]).
    program_memo: Cell<ProgramMemo>,
    /// The open "Show files (N)" popover.
    upload_list: RefCell<Option<UploadPopover>>,
    /// Time of the event that closed the popover (`popoverWillClose:`): so that
    /// pressing the button again does not reopen the popover.
    list_closed_at: Cell<Option<f64>>,
    /// The open server card ([`crate::server_card`]).
    server_card: RefCell<Option<ServerCard>>,
    /// The ports the pane's programs listen on, as the last scan found them
    /// ([`crate::footer`]): the dock shows their numbers, the ports menu
    /// these records.
    listeners: RefCell<Vec<Listener>>,
    /// What the context row shows of the active environment
    /// (`[shell] python_env`, `node_version`; [`crate::environment`]).
    environment_wants: Cell<Wants>,
    /// The listening processes whose exit is watched — `(pid, start)`, one
    /// thread each ([`crate::footer`]).
    watched: RefCell<HashSet<(u32, u64)>>,
    /// The server's listening ports in a remote session ([`crate::remote_ports`]).
    remote_ports: RefCell<RemotePorts>,
    /// Time of the event that closed the server card — its own slot, so a
    /// press on one control never swallows the other's.
    card_closed_at: Cell<Option<f64>>,
    /// The helper ssh session that verifies remote links and counts a download:
    /// its worker is born at the first question, its session
    /// closes on another generation, when idle and with the pane — while the
    /// load indicator samples it is never idle.
    remote_helper: RefCell<RemoteHelper>,
    /// The pane's seat in its server's load sampling ([`crate::stats`]): the
    /// load is measured once for the server, not per pane. The settings' value
    /// from the birth package, refreshed live ([`TerminalPane::set_stats_settings`]).
    stats: RefCell<StatsSeat>,
    /// The pane's last input: a key, a press, the wheel, a mouse move or
    /// its window becoming key — set at birth, then only by
    /// [`TerminalPane::note_interaction`]. On the sleep-counting clock, so the
    /// focus query's `idle` does not stop with the lid closed.
    last_input: Cell<Moment>,
    /// `[remote]`'s preview and download keys: from the birth package,
    /// refreshed live with the host marks ([`TerminalPane::set_host_marks`]).
    remote_files: RefCell<RemoteFiles>,
    /// The previews this pane downloaded, by landing path: how each
    /// opens and where its index is ([`crate::preview::PreviewTicket`]).
    previews: RefCell<HashMap<PathBuf, PreviewTicket>>,
    /// The file promises of ⌘-dragged remote links: the delegates
    /// kept alive and the Finder downloads that fulfil them.
    finder: RefCell<FinderDrops>,
    /// The questions this pane asked while its tab was not on screen,
    /// waiting to open when it is ([`crate::sheets`]); emptied with a
    /// `Cancel` answer when the pane closes.
    parked: RefCell<crate::sheets::ParkedQueue>,
    /// The questions this pane has up on its tab's owner right now, so a
    /// pane that moves to another tab can lower them and ask again where it
    /// lands ([`crate::sheets::lift`]).
    opened: RefCell<Vec<crate::sheets::Opened>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; TerminalPane implements no `Drop`
    // and offers no constructor besides `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTerminalPane"]
    #[ivars = PaneIvars]
    pub struct TerminalPane;

    unsafe impl NSObjectProtocol for TerminalPane {}

    impl TerminalPane {
        /// The terminal view's frame changed (`NSViewFrameDidChangeNotification`,
        /// the observer is set up in [`TerminalPane::observe_frame`]).
        ///
        /// The source is **not** `windowDidResize:`, because the content changes
        /// without the window size changing too: when a second tab opens the tab
        /// bar enters the title area and the content shortens, when only one tab
        /// is left the bar goes away and the content lengthens — the window's
        /// frame is the same in both. Bound to the window notification the
        /// drawable stayed at the old size, the layer **stretched** it to the new
        /// size and the text blurred vertically (measured). The view's
        /// notification also covers window
        /// resizing, so a single source.
        #[unsafe(method(viewFrameDidChange:))]
        fn view_frame_did_change(&self, _n: &NSNotification) {
            self.refresh_geometry();
        }

        /// A window became key — this pane's, if [`TerminalPane::follow_window`]
        /// set it up: what bateri's window does for the panes of its tab on
        /// screen. A host hides the pane itself, not a container: hidden, the
        /// pane is not the user's.
        #[unsafe(method(hostWindowDidBecomeKey:))]
        fn host_window_did_become_key(&self, n: &NSNotification) {
            if self.is_my_window(n) && !self.isHiddenOrHasHiddenAncestor() {
                self.window_became_key();
            }
        }

        /// This pane's window resigned key ([`TerminalPane::follow_window`]).
        #[unsafe(method(hostWindowDidResignKey:))]
        fn host_window_did_resign_key(&self, n: &NSNotification) {
            if self.is_my_window(n) {
                self.window_resigned_key();
            }
        }

        /// This pane's window was covered, uncovered, miniaturized or brought
        /// back ([`TerminalPane::follow_window`]): a window the compositor may
        /// have emptied needs a frame, and a covered one draws none.
        #[unsafe(method(hostWindowDidChangeOcclusionState:))]
        fn host_window_did_change_occlusion(&self, n: &NSNotification) {
            if self.is_my_window(n) {
                self.set_visibility(self.window_seen() && !self.isHiddenOrHasHiddenAncestor());
            }
        }

        /// This pane's window moved to a screen of another scale
        /// ([`TerminalPane::follow_window`]): the size in points did not change,
        /// so no frame notification comes.
        #[unsafe(method(hostWindowDidChangeBackingProperties:))]
        fn host_window_did_change_backing(&self, n: &NSNotification) {
            if self.is_my_window(n) {
                self.refresh_geometry();
            }
        }

        /// A question on this pane's window ended ([`TerminalPane::follow_window`]):
        /// one the pane parked behind it opens now.
        #[unsafe(method(hostWindowDidEndSheet:))]
        fn host_window_did_end_sheet(&self, n: &NSNotification) {
            if self.is_my_window(n) {
                crate::sheets::open_parked(self);
            }
        }
    }

    /// Closing of the "Show files (N)" popover and of the server card, told
    /// apart by the notification's object: AppKit also closes a `transient`
    /// popover (click outside) and its Esc monitor (and the list button's
    /// pressed tone) must go away then too.
    unsafe impl NSPopoverDelegate for TerminalPane {
        #[unsafe(method(popoverWillClose:))]
        fn popover_will_close(&self, n: &NSNotification) {
            if self.is_server_card(n) {
                self.server_card_will_close();
            } else {
                self.upload_list_will_close();
            }
        }

        #[unsafe(method(popoverDidClose:))]
        fn popover_did_close(&self, n: &NSNotification) {
            if self.is_server_card(n) {
                self.close_server_card();
            } else {
                self.close_upload_list();
            }
        }
    }

    // The search field's delegate: all methods of all three protocols
    // are optional; the ones used are in the `impl` below.
    unsafe impl NSControlTextEditingDelegate for TerminalPane {}
    unsafe impl NSTextFieldDelegate for TerminalPane {}
    unsafe impl NSSearchFieldDelegate for TerminalPane {}

    // **Pane-level menu selectors**: each is a one-line wrapper
    // around a named method — an owner without a menu can call the
    // same method directly. The responder chain of a targetless action is
    // `BateriView` → pane → window → delegate, and while the search field has
    // focus field editor → field → … → pane; so the item reaches the focused
    // pane and the "while the field has focus the chain does not pass
    // through `BateriView`" reason is moot. Application-wide ones
    // (`settingsDidChange:`, theme) are in `AppDelegate`, tab jobs
    // (`closeTab:`, `selectTab:`) in the window.
    impl TerminalPane {
        /// View ▸ Bigger (Cmd +).
        #[unsafe(method(makeFontBigger:))]
        fn make_font_bigger(&self, _sender: Option<&AnyObject>) {
            self.zoom_in();
        }

        /// View ▸ Smaller (Cmd −).
        #[unsafe(method(makeFontSmaller:))]
        fn make_font_smaller(&self, _sender: Option<&AnyObject>) {
            self.zoom_out();
        }

        /// View ▸ Actual Size (Cmd 0).
        #[unsafe(method(resetFontSize:))]
        fn reset_font_size(&self, _sender: Option<&AnyObject>) {
            self.zoom_reset();
        }

        /// Edit ▸ Find ▸ Find… (⌘F).
        ///
        /// The selectors are **our own names**, not `performFindPanelAction:`:
        /// while the field has focus the first responder is
        /// AppKit's field editor and it would implement that selector itself and swallow it.
        #[unsafe(method(findInScrollback:))]
        fn find_in_scrollback(&self, _sender: Option<&AnyObject>) {
            self.find();
        }

        /// Edit ▸ Find ▸ Find Next (⌘G) and the panel's up arrow.
        #[unsafe(method(findNextMatch:))]
        fn find_next_match(&self, _sender: Option<&AnyObject>) {
            self.find_next();
        }

        /// Edit ▸ Find ▸ Find Previous (⇧⌘G) and the panel's down arrow.
        #[unsafe(method(findPreviousMatch:))]
        fn find_previous_match(&self, _sender: Option<&AnyObject>) {
            self.find_previous();
        }

        /// Edit ▸ Find ▸ Use Selection for Find (⌘E).
        #[unsafe(method(useSelectionForFind:))]
        fn use_selection_for_find_action(&self, _sender: Option<&AnyObject>) {
            self.use_selection_for_find();
        }

        /// Edit ▸ Clear to Start (⌘K).
        #[unsafe(method(clearToStart:))]
        fn clear_to_start_action(&self, _sender: Option<&AnyObject>) {
            self.clear_to_start();
        }

        /// Edit ▸ Clear Scrollback (⌥⌘K).
        #[unsafe(method(clearScrollback:))]
        fn clear_scrollback_action(&self, _sender: Option<&AnyObject>) {
            self.clear_scrollback();
        }

        /// View ▸ Scroll to Top (⌘Home).
        #[unsafe(method(scrollToTop:))]
        fn scroll_to_top_action(&self, _sender: Option<&AnyObject>) {
            self.scroll_to_top();
        }

        /// View ▸ Scroll to Bottom (⌘End).
        #[unsafe(method(scrollToBottom:))]
        fn scroll_to_bottom_action(&self, _sender: Option<&AnyObject>) {
            self.scroll_to_bottom();
        }

        /// View ▸ Page Up (⌘PgUp).
        #[unsafe(method(scrollPageUp:))]
        fn scroll_page_up_action(&self, _sender: Option<&AnyObject>) {
            self.page_up();
        }

        /// View ▸ Page Down (⌘PgDn).
        #[unsafe(method(scrollPageDown:))]
        fn scroll_page_down_action(&self, _sender: Option<&AnyObject>) {
            self.page_down();
        }

        /// The panel's close button — the same path as Esc.
        #[unsafe(method(closeSearch:))]
        fn close_search_action(&self, _sender: Option<&AnyObject>) {
            self.close_search();
        }

        /// The field's action: every text change (`sendsSearchStringImmediately`)
        /// and the ⊗ button.
        #[unsafe(method(searchFieldChanged:))]
        fn search_field_changed(&self, _sender: Option<&AnyObject>) {
            // Typing in the field is input to this pane: the focus
            // query counts the field as the pane's focus, so its `idle` too.
            self.note_interaction();
            self.apply_search();
        }

        /// The `Aa` or `.*` switch changed.
        #[unsafe(method(searchOptionsChanged:))]
        fn search_options_changed(&self, _sender: Option<&AnyObject>) {
            self.apply_search();
        }

        /// The field's command hook: ⏎ the previous (older), ⇧⏎
        /// the next (newer) match; Esc closes the panel — instead of
        /// `NSSearchField`'s "clear the text" default. The remaining commands
        /// go to the field itself (`false`).
        ///
        /// Shift cannot be read from the selector — both keys are
        /// `insertNewline:` — so it is read from the event itself.
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn control_do_command(
            &self,
            _control: &AnyObject,
            _text_view: &AnyObject,
            command: Sel,
        ) -> bool {
            self.note_interaction();
            if command == sel!(insertNewline:) {
                let shift = NSApplication::sharedApplication(self.mtm())
                    .currentEvent()
                    .is_some_and(|event| event.modifierFlags().contains(NSEventModifierFlags::Shift));
                self.search_step(if shift {
                    SearchDirection::Newer
                } else {
                    SearchDirection::Older
                });
                true
            } else if command == sel!(cancelOperation:) {
                self.close_search();
                true
            } else {
                false
            }
        }

        /// Enabled state of the Find items, clearing, scrolling and upload
        /// cancel; **an unknown item is `true`** — point size is always enabled.
        /// Clearing and scrolling are greyed out on the alternate screen: the
        /// primary scrollback is unreachable there, a grey item is
        /// an honest "not here"; they are grey without a session too.
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            let action = item.action();
            // No `return`: `define_class!` converts the `bool` at the end of the body.
            if self.shows_view() && pencere_view::is_content_item(item) {
                // A program's view is the pane's content: Edit, scrolling and the text size
                // are the view's while it is shown (the shortcut table's mark), so these
                // items are grey and a key the view did not handle does not run them here.
                false
            } else if action.is_some_and(is_scrollback_action) {
                self.session()
                    .is_some_and(|session| !session.alt_screen())
            } else if action == Some(sel!(findNextMatch:)) || action == Some(sel!(findPreviousMatch:)) {
                self.has_query()
            } else if action == Some(sel!(useSelectionForFind:)) {
                self.session()
                    .is_some_and(|session| session.has_selection())
            } else if action == Some(sel!(cancelUpload:)) {
                // ⌘. only while this pane has a queue; the grey
                // item's shortcut falls to `keyDown:` and is swallowed there.
                self.ivars().uploads.borrow().active()
            } else if action == Some(sel!(forgetPassword:)) {
                // A remote tab with a saved password.
                self.can_forget_password()
            } else if action == Some(sel!(copyLastCommand:)) || action == Some(sel!(copyLastOutput:))
            {
                // Something has run in this pane.
                self.session().is_some_and(|session| session.has_commands())
            } else if action == Some(sel!(copyFolderPath:)) {
                self.folder_text().is_some()
            } else if action == Some(sel!(showFolderInFinder:)) {
                // A folder of this Mac: a remote tab's is the server's.
                self.local_folder().is_some_and(|folder| folder.is_dir())
            } else {
                true
            }
        }

        /// Shell ▸ Cancel Upload (⌘.) and the popover's `Cancel all ⌘.`: this
        /// pane's **whole** upload queue;
        /// if the flowing item has gone past 30 seconds it asks first.
        /// Not Esc, because the keyboard goes to the remote shell at
        /// that moment. The menu shortcut is caught before `keyDown:`, so it
        /// also works on the alternate screen (vim) — its only gate is the queue
        /// (`validateMenuItem:`).
        #[unsafe(method(cancelUpload:))]
        fn cancel_upload(&self, _sender: Option<&AnyObject>) {
            self.cancel_uploads();
        }

        /// Shell ▸ Forget Password for “{host}”: this pane's remote
        /// account's saved password (`validateMenuItem:` greys it without one).
        #[unsafe(method(forgetPassword:))]
        fn forget_password_sent(&self, _sender: Option<&AnyObject>) {
            self.forget_password();
        }

        /// Edit ▸ Copy Last Command: the line of the newest command this pane ran, on the
        /// pasteboard.
        #[unsafe(method(copyLastCommand:))]
        fn copy_last_command_sent(&self, _sender: Option<&AnyObject>) {
            match self.session().and_then(|session| session.last_remembered()) {
                Some((_, text)) => {
                    clipboard::copy(&NSPasteboard::generalPasteboard(), Some(text));
                }
                None => crate::preview::beep(),
            }
        }

        /// Edit ▸ Copy Last Output: the newest command's output, on the pasteboard — nothing
        /// (a beep) once its rows have left the history or when it printed nothing (a program
        /// that holds the screen has its own screen, not rows to read).
        #[unsafe(method(copyLastOutput:))]
        fn copy_last_output_sent(&self, _sender: Option<&AnyObject>) {
            let output = self.session().and_then(|session| {
                let (id, _) = session.last_remembered()?;
                session.command_output(id)
            });
            match output {
                Some(text) if !text.is_empty() => {
                    clipboard::copy(&NSPasteboard::generalPasteboard(), Some(text));
                }
                _ => crate::preview::beep(),
            }
        }

        /// Shell ▸ Copy Folder Path: this pane's folder on the pasteboard — the path, or
        /// `host:path` in a remote session ([`TerminalPane::folder_text`]).
        #[unsafe(method(copyFolderPath:))]
        fn copy_folder_path_sent(&self, _sender: Option<&AnyObject>) {
            if let Some(text) = self.folder_text() {
                clipboard::copy(&NSPasteboard::generalPasteboard(), Some(text));
            }
        }

        /// Shell ▸ Show Folder in Finder (grey in a remote session).
        #[unsafe(method(showFolderInFinder:))]
        fn show_folder_in_finder_sent(&self, _sender: Option<&AnyObject>) {
            if let Some(folder) = self.local_folder() {
                crate::hyperlink::show_folder(&folder);
            }
        }

        /// A listening port's item (the dock's ports menu, Shell ▸ Open
        /// Port ▸): opens `http://localhost:{tag}` ([`crate::footer`]).
        #[unsafe(method(openPort:))]
        fn open_port_action(&self, sender: Option<&AnyObject>) {
            self.open_port_sent(sender);
        }

        /// The item's ⌥ alternate: copies the address.
        #[unsafe(method(copyPortURL:))]
        fn copy_port_url_action(&self, sender: Option<&AnyObject>) {
            self.copy_port_url_sent(sender);
        }

        /// A server's port (the ports menu's "On {host}" half): opens it,
        /// forwarding it first when it does not open from this Mac
        /// ([`crate::remote_ports`]).
        #[unsafe(method(openRemotePort:))]
        fn open_remote_port_action(&self, sender: Option<&AnyObject>) {
            self.open_remote_port_sent(sender);
        }

        /// Its ⌥ alternate: copies the address that opens it.
        #[unsafe(method(copyRemotePortURL:))]
        fn copy_remote_port_url_action(&self, sender: Option<&AnyObject>) {
            self.copy_remote_port_url_sent(sender);
        }

        /// "Jump to latest"'s button ([`JumpLatest`]).
        #[unsafe(method(jumpToLatest:))]
        fn jump_to_latest_sent(&self, _sender: Option<&AnyObject>) {
            self.jump_to_latest();
        }

        /// The strip's "Quit ⌃C" over a program's view: ⌃C, as the key.
        #[unsafe(method(quitProgram:))]
        fn quit_program_sent(&self, _sender: Option<&AnyObject>) {
            self.interrupt_program();
        }

        /// The popover row's button (`Cancel`/`Remove`): `tag` is the item's
        /// id, not its position — positions shift with finished and removed items.
        #[unsafe(method(uploadRowAction:))]
        fn upload_row_action_sent(&self, sender: Option<&AnyObject>) {
            let Some(button) = sender.and_then(|sender| sender.downcast_ref::<NSButton>()) else {
                return;
            };
            if let Ok(id) = u64::try_from(button.tag()) {
                self.upload_row_action(id);
            }
        }
    }
);

impl TerminalPane {
    /// Builds the view, the surface and the renderer; the session and link are
    /// **not there yet** ([`TerminalPane::start`]). The frame observer is not
    /// there yet either: the window sets it up after making the pane the
    /// `contentView` ([`TerminalPane::observe_frame`]).
    ///
    /// The renderer is born here and its error returns to the caller: if the
    /// GPU device or the pipelines cannot be built the pane has nothing to
    /// draw. The font is given to the renderer here as a **request**, with
    /// the inherited point-size delta ([`TerminalPane::request_font`]): the
    /// first atlas opens in `start`'s geometry with the enlarged point size.
    pub(crate) fn new(
        mtm: MainThreadMarker,
        frame: NSRect,
        launch: PaneLaunch,
    ) -> Result<Retained<Self>, GpuError> {
        let PaneLaunch {
            id,
            run,
            host,
            lookup,
            stats,
            settings,
            theme,
            mut launch,
            integration,
            reduce_motion,
            smooth_scroll,
            scrollbar,
            zoom,
            masters,
            keeper,
            identity,
        } = launch;
        // The saved identity of a restored pane, a new one otherwise.
        let uuid = launch.uuid.take().unwrap_or_else(new_pane_uuid);
        let renderer = Rc::new(Renderer::system_default()?);
        // The layer is ours: wgpu configures its device,
        // format and drawable size, the scale stays with its owner.
        let layer = CAMetalLayer::new();
        // SAFETY: `layer` is a live `CAMetalLayer`; wgpu retains it.
        let surface = Rc::new(unsafe {
            Surface::from_layer(&renderer, NonNull::from(&*layer).cast::<c_void>())
        }?);
        let view = BateriView::new(mtm, frame);
        // Order matters: layer first, then wantsLayer — the reverse makes
        // AppKit build its own layer and the CAMetalLayer is dropped.
        view.setLayer(Some(&layer));
        view.setWantsLayer(true);
        let font = settings.font.clone();
        let content_edge = settings.content_edge;
        let remote_files = settings.remote_files.clone();
        let stats_seat = StatsSeat::new(settings.remote_stats);
        let ports_on = run.is_none() && settings.shell_ports;
        // A timed run reads no file for the row, like it scans no port.
        let environment_wants = Wants {
            python: run.is_none() && settings.shell_python_env,
            node: run.is_none() && settings.shell_node_version,
        };
        let dim = DimOverlay::new(mtm);
        dim.paint(&theme);
        let link_veil = DimOverlay::new(mtm);
        link_veil.paint_with(&theme, LINK_VEIL_ALPHA);
        let card_frame = FrameBox::new(mtm, &theme);
        let link_label = LinkLabel::new(mtm);
        link_label.0.paint(&theme);
        let block_tip = BlockTipParts::new(mtm);
        block_tip.paint(&theme);
        let tip = block_tip.tip.clone();
        let jump = JumpLatest::new(mtm);
        jump.paint(&theme);
        let jump_surface = jump.surface.clone();
        let this = Self::alloc(mtm).set_ivars(PaneIvars {
            id,
            run,
            host,
            lookup,
            birth: RefCell::new(Some(Birth {
                stats,
                settings,
                theme,
                launch,
                integration,
            })),
            font: RefCell::new(font),
            reduce_motion: Cell::new(reduce_motion),
            smooth_scroll: Cell::new(smooth_scroll),
            scrollbar: Cell::new(scrollbar),
            content_edge: Cell::new(content_edge),
            corners: Cell::new((0.0, 0.0)),
            renderer,
            layer,
            surface,
            view: view.clone(),
            dim: dim.clone(),
            link_veil: link_veil.clone(),
            connection: Cell::new(PaneLink::Live),
            card_frame: card_frame.clone(),
            link_label: link_label.clone(),
            link: OnceCell::new(),
            shot: RefCell::new(None),
            session: OnceCell::new(),
            shell_parent: OnceCell::new(),
            wake: Arc::new(ShellWake {
                id,
                lookup,
                timed: run.is_some(),
                waker: Mutex::new(None),
                pending_copy: Arc::default(),
                title_pending: Arc::default(),
                search_pending: Arc::default(),
                memory_pending: Arc::default(),
                remote_probe: Arc::default(),
                login_probe: Arc::default(),
                program_probe: Arc::default(),
                identity_probe: Arc::default(),
                port_probe: Arc::new(PortProbe::new(ports_on)),
                environment_pending: Arc::default(),
                link_pending: Arc::default(),
                views_pending: Arc::default(),
                pointer_pending: Arc::default(),
                blocks_pending: Arc::default(),
                unseen_pending: Arc::default(),
                up_pending: Arc::default(),
                typed_pending: Arc::default(),
                kept: keeper.as_ref().map(|keeper| keeper.active_flag()),
                state_pending: Arc::default(),
                mirror_pending: Arc::default(),
                activity_pending: Arc::default(),
                bell_pending: Arc::default(),
                notes_pending: Arc::default(),
            }),
            zoom: Cell::new(zoom),
            // No dock at launch: `start` decides and computes the geometry
            // after that.
            dock_rows: Cell::new(0),
            dock_rows_at_birth: Cell::new(0),
            uuid,
            closed: Cell::new(false),
            leaving: Cell::new(false),
            search: OnceCell::new(),
            search_status: Cell::new(SearchStatus::Empty),
            driving: Cell::new(false),
            bar_pointer: Cell::new((false, false)),
            seen: Cell::new(true),
            pencere: RefCell::new(None),
            view_waits: RefCell::new(Waits::default()),
            view_asking: pencere_view::ask::Asking::default(),
            view_size: Cell::new((0, 0)),
            exit_status: Cell::new(None),
            marks_wanted: Cell::new(false),
            block_paced: Cell::new(None),
            block_tip,
            tip_for: Cell::new(None),
            time_format: OnceCell::new(),
            jump,
            jumping: Cell::new(None),
            uploads: RefCell::new(Transfers::default()),
            upload_alert: RefCell::new(None),
            upload_stop: RefCell::new(None),
            password: RefCell::new(None),
            masters,
            keeper,
            identity,
            taken_from: RefCell::new(None),
            asked_for: RefCell::new(AskedFor::default()),
            journal: RefCell::new(None),
            ssh_session: Cell::new(None),
            wrap_proof: RefCell::new(WrapProof::default()),
            program_memo: Cell::new(ProgramMemo::default()),
            upload_list: RefCell::new(None),
            list_closed_at: Cell::new(None),
            server_card: RefCell::new(None),
            card_closed_at: Cell::new(None),
            listeners: RefCell::new(Vec::new()),
            environment_wants: Cell::new(environment_wants),
            watched: RefCell::new(HashSet::new()),
            remote_ports: RefCell::new(RemotePorts::default()),
            remote_helper: RefCell::new(RemoteHelper::default()),
            stats: RefCell::new(stats_seat),
            last_input: Cell::new(Moment::now()),
            remote_files: RefCell::new(remote_files),
            previews: RefCell::new(HashMap::new()),
            finder: RefCell::new(FinderDrops::default()),
            parked: RefCell::new(crate::sheets::ParkedQueue::new()),
            opened: RefCell::new(Vec::new()),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        // The pane is a plain **container**, `BateriView` is its child: the
        // search panel will float above the terminal and must be a
        // sibling of the Metal layer, not its child — the subviews of a
        // layer-hosting view are outside AppKit's contract. The pane is
        // layer-backed, otherwise the sibling panel could end up **below** the
        // Metal layer. It draws nothing itself and receives no events:
        // `BateriView` fills it completely, hit testing falls to the topmost child.
        this.setWantsLayer(true);
        // The child is fitted to the pane and follows its size by
        // autoresizing. The source of the geometry is still `BateriView`
        // (`sync_geometry`), and so is the notification's frame.
        view.setFrame(this.bounds());
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        this.addSubview(&view);
        // The link label under the veil (an unfocused pane gets no hover anyway).
        this.addSubview(&link_label.0);
        // The block tip beside it, under the veil too.
        this.addSubview(&tip);
        // "Jump to latest" too; its button's target is this pane.
        this.addSubview(&jump_surface);
        this.ivars().jump.aim(&this);
        // The veil is on top: the search panel goes right above `view`
        // (`SearchBar::new`), so it too stays under the veil and the dimmed
        // pane's panel is dimmed too.
        // The connection's veil lies under the dim one and over the grid only:
        // [`TerminalPane::place_link_veil`] keeps the dock out of it.
        link_veil.setFrame(this.bounds());
        this.addSubview(&link_veil);
        dim.setFrame(this.bounds());
        dim.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        this.addSubview(&dim);
        // The card's frame lies over the veil: a quiet pane keeps its frame
        // at full clarity, the veil dims only the grid.
        card_frame.setFrame(this.bounds());
        card_frame.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        this.addSubview(&card_frame);
        let font = this.ivars().font.borrow().clone();
        this.request_font(&font);
        Ok(this)
    }

    /// Subscribes to the terminal view's frame notification
    /// (`viewFrameDidChange:`). The content's size changes independently of the
    /// window too (the title row, a split); so the geometry comes from the view's own
    /// notification. `postsFrameChangedNotifications` is on by default.
    ///
    /// The **last** step of the window's constructor: a notification arriving
    /// before the pane becomes `contentView` would try to build the geometry
    /// without a window. The observer is removed when the pane closes
    /// ([`TerminalPane::begin_close`]), without waiting for the window's closing.
    pub fn observe_frame(&self) {
        // SAFETY: the selector is defined on this class and takes a single
        // `&NSNotification`; the name is a constant AppKit exposes, the object
        // is this pane's view.
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                self,
                sel!(viewFrameDidChange:),
                Some(NSViewFrameDidChangeNotification),
                Some(&self.ivars().view),
            );
        }
    }

    /// The pane follows its window by itself: its focus with the window's
    /// key state, its drawing with the window's occlusion, its geometry with
    /// the window's scale, and a question it parked behind the window's own
    /// question opens when that one ends. bateri's window does all of it for
    /// its tabs' panes and does not call this; a host that only places the
    /// pane in a view of its own needs it, or its pane silently keeps the
    /// state it was born with. Ends with the pane's closing.
    pub fn follow_window(&self) {
        let center = NSNotificationCenter::defaultCenter();
        // SAFETY: each selector is defined on this class and takes a single
        // `&NSNotification`; the names are constants AppKit exposes. No
        // object: the pane may change windows, so each handler checks the
        // notification's window is the pane's own.
        unsafe {
            for (selector, name) in [
                (
                    sel!(hostWindowDidBecomeKey:),
                    NSWindowDidBecomeKeyNotification,
                ),
                (
                    sel!(hostWindowDidResignKey:),
                    NSWindowDidResignKeyNotification,
                ),
                (
                    sel!(hostWindowDidChangeOcclusionState:),
                    NSWindowDidChangeOcclusionStateNotification,
                ),
                (
                    sel!(hostWindowDidChangeBackingProperties:),
                    NSWindowDidChangeBackingPropertiesNotification,
                ),
                (
                    sel!(hostWindowDidEndSheet:),
                    NSWindowDidEndSheetNotification,
                ),
            ] {
                center.addObserver_selector_name_object(self, selector, Some(name), None);
            }
        }
    }

    /// Whether `n` is about the window this pane is in.
    fn is_my_window(&self, n: &NSNotification) -> bool {
        let (Some(object), Some(window)) = (n.object(), self.window()) else {
            return false;
        };
        std::ptr::eq(
            Retained::as_ptr(&object).cast::<u8>(),
            Retained::as_ptr(&window).cast::<u8>(),
        )
    }

    /// Whether the pane's window can be seen — not covered, not miniaturized.
    fn window_seen(&self) -> bool {
        self.window().is_some_and(|window| {
            window
                .occlusionState()
                .contains(NSWindowOcclusionState::Visible)
        })
    }

    /// The pane's window became key and the pane is on screen in it: the
    /// caret fills, the pointer's footer hover comes back, and coming back
    /// counts as an interaction (the remote load indicator samples at once).
    pub fn window_became_key(&self) {
        self.apply_focus(true);
        self.rehover_footer();
        self.note_interaction();
    }

    /// The pane's window resigned key: focus off, and what follows the
    /// pointer or ⌘ goes — ⌘'s release, or the pointer's leaving, may reach
    /// another application: the ⌘-hovered link, the footer's hover and the
    /// scroll bar's wide strip (tracked in the key window only).
    pub fn window_resigned_key(&self) {
        self.apply_focus(false);
        self.unhover_footer();
        self.view().clear_link();
        self.view().release_scrollbar_hover();
    }

    /// Whether the pane can be seen (`visible`): a covered pane draws no
    /// frame and drives no sampling or block index, and one coming back is
    /// drawn again — the compositor may have emptied its layer.
    pub fn set_visibility(&self, visible: bool) {
        // A program's view over the terminal keeps its frames stopped whatever
        // the pane's visibility says; they come back with the view's end.
        if let Some(link) = self.link() {
            link.set_visible(visible && !self.shows_view());
        }
        self.set_visible(visible);
    }

    /// Gives the keyboard to the pane: its terminal view becomes its window's
    /// first responder, and the pane's host hears [`PaneHost::focused`].
    /// `false` when the pane is in no window or the window refused.
    pub fn focus(&self) -> bool {
        let Some(window) = self.window() else {
            return false;
        };
        // The keyboard belongs to a program's view while it covers the terminal.
        match self.view_host() {
            Some(host) => {
                let taken = window.makeFirstResponder(Some(host.web()));
                host.focus_page();
                taken
            }
            None => window.makeFirstResponder(Some(self.view())),
        }
    }

    /// The host hid or showed the pane, or a view above it: the pane reads
    /// where it stands again. Hidden, it leaves the screen
    /// ([`Self::leave_screen`]); shown, it draws if its window is seen, takes
    /// the focus if its window is key, and a question it parked opens.
    pub fn refresh_visibility(&self) {
        if self.isHiddenOrHasHiddenAncestor() {
            self.set_visibility(false);
            self.leave_screen();
            return;
        }
        self.set_visibility(self.window_seen());
        let key = self.is_active();
        self.apply_focus(key);
        if key {
            self.rehover_footer();
        }
        crate::sheets::open_parked(self);
    }

    pub fn id(&self) -> u64 {
        self.ivars().id
    }

    /// The session's persistent identity (`TERM_SESSION_ID`, `bateri://tab/<id>`).
    /// Separate from the in-process [`TerminalPane::id`]: that one is
    /// the key of main-queue returns, this is the name given outward.
    pub fn uuid(&self) -> &PaneUuid {
        &self.ivars().uuid
    }

    /// The shell's exit code, once it has exited normally; `None` before, and
    /// for a shell killed by a signal — read when [`PaneHost::shell_exited`]
    /// comes.
    pub fn exit_status(&self) -> Option<i32> {
        self.ivars().exit_status.get()
    }

    /// Whether closing has begun ([`PaneIvars::closed`]).
    pub fn is_closed(&self) -> bool {
        self.ivars().closed.get()
    }

    /// Whether this pane's **tab** is on screen in its window: the tab's
    /// container (the pane's superview) is not hidden, nor anything above
    /// it. A window carries several tabs and only the selected one's
    /// container is shown; the others stay in the hierarchy, hidden.
    ///
    /// The container, not the pane itself: a pane hidden behind the zoom
    /// (⇧⌘↩) is still in the tab on screen — its focus bit must be right
    /// the moment the zoom drops, and nothing else would send it again.
    pub(crate) fn tab_shown(&self) -> bool {
        // SAFETY: reading the superview; we are on the main thread (`MainThreadOnly`).
        unsafe { self.superview() }
            .is_some_and(|container| !container.isHiddenOrHasHiddenAncestor())
    }

    /// Whether this pane is the user's: its window is key **and** its tab
    /// is the one on screen ([`Self::tab_shown`]). The single answer to
    /// "is this pane focused" — a pane of a background tab sits in the key
    /// window too, so reading `isKeyWindow` alone would call it focused.
    pub(crate) fn is_active(&self) -> bool {
        self.window().is_some_and(|window| window.isKeyWindow()) && self.tab_shown()
    }

    /// This pane's temporary point-size delta — a new tab inherits it.
    pub(crate) fn zoom(&self) -> Zoom {
        self.ivars().zoom.get()
    }

    /// What session restore saves of this pane and, with
    /// `with_history`, its scrollback as VT bytes ([`Session::final_history`];
    /// an empty one is no file). `None` for a pane without a session or whose
    /// closing has begun: there is nothing live to save.
    ///
    /// **Quit only**: `final_history` leaves the alternate screen for good
    /// (`AppDelegate::shutdown` calls this once, before `begin_close`).
    pub(crate) fn saved(&self, with_history: bool) -> Option<(SavedPane, Option<Vec<u8>>)> {
        if self.is_closed() {
            return None;
        }
        let session = self.session()?;
        let history = with_history
            .then(|| session.final_history())
            .filter(|bytes| !bytes.is_empty());
        let pane = SavedPane {
            uuid: self.ivars().uuid.clone(),
            dir: session.working_directory(),
            zoom_steps: self.zoom().steps(),
            remote_line: session.remote_line(),
            kept: self.saved_kept(),
            history: history.is_some(),
        };
        Some((pane, history))
    }

    /// The terminal a server keeps that this pane shows, as session restore saves it: the
    /// account that holds it, its name there and the `ssh` arguments that reach the server
    /// (the program's name left out). `None` for a pane that shows its own shell, and for
    /// a terminal whose server has not been named by arguments.
    pub(crate) fn saved_kept(&self) -> Option<SavedKept> {
        let shown = self.host().kept_terminal(self.uuid())?;
        let args = shown.argv.get(1..)?.to_vec();
        let asked_for = self.ivars().asked_for.borrow().clone();
        (!args.is_empty()).then_some(SavedKept {
            account: shown.account,
            terminal: shown.terminal,
            args,
            close_at_end: asked_for.close_at_end,
            opener: asked_for.opener,
        })
    }

    /// Who asked for this pane, when a program on a server did ("Claude (t4)"), while the pane
    /// shows the terminal it was asked for: what its tab's card says.
    pub(crate) fn opener(&self) -> Option<String> {
        self.host().kept_terminal(self.uuid())?;
        self.ivars().asked_for.borrow().opener.clone()
    }

    /// Whether this pane shows a terminal a server keeps (rather than its own shell).
    pub(crate) fn shows_kept_terminal(&self) -> bool {
        self.host().kept_terminal(self.uuid()).is_some()
    }

    /// The remote session bateri does things on a server for, if the pane has
    /// one: [`Session::remote_target`] (the running command's generation, the
    /// server, the folder it reported) **when the pane shows a terminal that
    /// server keeps** ([`Shown::serves`]). `None` for a local pane and for a plain
    /// `ssh` — an `ssh` the integration is off for, a `production` host, a
    /// server kablo cannot be kept on, a call that runs a command — which gets
    /// nothing of bateri's on the server: no file drop or download, no preview,
    /// no load indicator, no ports, no ⌘ check of a path, and no
    /// connection of bateri's own to the host.
    ///
    /// **The one gate.** Every add-on that works on a server starts from this and
    /// not from `remote_target` (which says only that an `ssh` is running, and
    /// is for what the pane *shows*: the line under it, the tab's mark). The
    /// pane learns it is a server pane when the terminal appears
    /// ([`Self::remote_edge`] is run again by `hold_service`), so whichever of the
    /// process-table probe and the terminal comes second still opens the gate.
    pub(crate) fn server_target(&self) -> Option<(u64, RemoteTarget, String)> {
        let target = self.session()?.remote_target()?;
        let shown = self.host().kept_terminal(self.uuid())?;
        shown.serves(&target.1).then_some(target)
    }

    /// Whether the pane is in a remote session that is a plain `ssh`: an `ssh`
    /// runs and the pane is not a server pane ([`Self::server_target`]). What
    /// the pane *does* with a thing the user hands it (a Finder drop) differs
    /// from a local pane's: a local path means nothing to the remote shell, so
    /// nothing is written to it either.
    pub(crate) fn is_plain_ssh(&self) -> bool {
        self.session()
            .is_some_and(|session| session.remote_target().is_some())
            && self.server_target().is_none()
    }

    /// Owner of the events ([`PaneHost`]).
    pub(crate) fn host(&self) -> &dyn PaneHost {
        &*self.ivars().host
    }

    /// Who the application that opened the pane is.
    pub(crate) fn identity(&self) -> &Identity {
        &self.ivars().identity
    }

    /// The path by which main-queue returns find the pane (`uploader`'s jobs
    /// capture it too).
    pub(crate) fn lookup(&self) -> PaneLookup {
        self.ivars().lookup
    }

    /// The report path reads the counters from here (`AppDelegate::report_and_exit`).
    pub(crate) fn renderer(&self) -> &Renderer {
        &self.ivars().renderer
    }

    pub(crate) fn link(&self) -> Option<&DisplayLink> {
        self.ivars().link.get()
    }

    /// The pane as it is now, as an image for a tile `size` points across at `scale` pixels to the
    /// point (a split tab's summary card, `tab_bar`): the whole of its frame — grid, history band,
    /// dock, pictures — drawn at the pane's own size and shrunk on the GPU, so the pixels that
    /// cross are the tile's. `None` before the pane has been laid out or if the GPU declined.
    ///
    /// Pulled, never pushed: it asks the link for no frame and takes none of its damage, counts in
    /// none of its tokens and steps none of its animations (`bt_gpu::DisplayLink::snapshot`). A
    /// pane in a tab that is not shown answers like any other — its renderer, surface and session
    /// live while it is hidden; only its frames stop. The cursor is where it is heading, drawn in
    /// the open phase of its blink.
    ///
    /// The textures stay between two calls (the card asks every half second) until
    /// [`TerminalPane::release_snapshot`].
    ///
    /// **A pane that shows a program's view is the view's picture**, not the terminal's under it:
    /// the web view's own, asked for and kept (`pencere_view::still`). The view takes its first
    /// picture when it has loaded, so there is one almost always; for the call or two before
    /// any has come (or after the card let go of them) the pane's terminal is what the tile
    /// shows, below, since a tile is never empty.
    pub fn snapshot(&self, size: (f64, f64), scale: f64) -> Option<CFRetained<CGImage>> {
        let tile = |points: f64| (points * scale).round().max(1.0) as u32;
        if let Some(host) = self.view_host()
            && let Some(image) = host.still((tile(size.0), tile(size.1)), scale)
        {
            return Some(image);
        }
        let link = self.link()?;
        // A still asked for from inside a still (the session's notices run on this thread) gets
        // none rather than a second borrow.
        let mut shot = self.ivars().shot.try_borrow_mut().ok()?;
        let still = link.snapshot(
            shot.get_or_insert_with(Shot::default),
            (tile(size.0), tile(size.1)),
        )?;
        crate::snapshot::image(&still)
    }

    /// Lets go of what [`TerminalPane::snapshot`] kept: its textures and buffers. The next
    /// snapshot makes them again.
    pub fn release_snapshot(&self) {
        self.ivars().shot.replace(None);
        if let Some(host) = self.view_host() {
            host.release_still();
        }
    }

    /// Scrolling **input** reached this pane's grid — the wheel, a page
    /// scroll, a search jump: the scroll bar shows
    /// (`bt_gpu::DisplayLink::poke_scrollbar`). Every scroll gate calls this
    /// and nothing on the output path does, so streaming output never lights
    /// the bar. Silent before the link is born.
    ///
    /// Scrolling input also ends a jump to the latest line on its way
    /// ([`PaneIvars::jumping`]): the window is the user's again, so "Jump to
    /// latest" may come back at once.
    pub(crate) fn poke_scrollbar(&self) {
        if self.ivars().jumping.take().is_some() {
            self.refresh_jump();
        }
        if let Some(link) = self.link() {
            link.poke_scrollbar();
        }
    }

    /// Shows, relabels or hides "Jump to latest" ([`JumpLatest`]) from the
    /// session's count of lines below a scrolled window
    /// (`bt_core::Session::unseen_rows`) and puts it in the pane's
    /// bottom-right corner, above the dock. A zero count — the window back
    /// at the bottom — hides it and ends a jump on its way.
    ///
    /// **A count that moved during the jump lands it at once**: the glide
    /// was booked as a distance, and a window scrolled up stays on its rows
    /// while output comes, so it would stop that many rows short of a
    /// bottom that moved — under a flood, never arrive. The rest of the way
    /// is a jump. Asks for no frame itself.
    pub(crate) fn refresh_jump(&self) {
        let ivars = self.ivars();
        let rows = self.session().map_or(0, |session| session.unseen_rows());
        if rows == 0 {
            ivars.jumping.set(None);
        }
        match ivars.jumping.get() {
            _ if rows == 0 => ivars.jump.set(None, |_| None),
            Some(at) => {
                if rows != at
                    && let Some(session) = self.session()
                {
                    ivars.jumping.set(Some(rows));
                    session.scroll_to(f32::MAX, false);
                }
                ivars.jump.set(None, |_| None);
            }
            None => ivars.jump.set(Some(rows), |size| self.jump_place(size)),
        }
    }

    /// Where "Jump to latest" of `size` stands ([`jump_origin`]), from the
    /// grid's own two reserves in this pane's cell
    /// ([`TerminalPane::sync_geometry`]): the always-up track's
    /// (`ScrollbarMode::reserve_px`) and the dock's (`bt_gpu::dock_px`) —
    /// so a resize or a new point size places it without waiting for a
    /// frame. The dock's band grows past its reserve only while a command is
    /// typed, and typing returns the window to the bottom, which hides the
    /// button. `None` off a window.
    fn jump_place(&self, size: NSSize) -> Option<NSPoint> {
        let scale = self.window()?.backingScaleFactor();
        let ivars = self.ivars();
        let cell = self.cell_metrics(scale);
        let track = f64::from(ivars.scrollbar.get().reserve_px(cell)) / scale;
        let dock = f64::from(bt_gpu::dock_px(ivars.dock_rows.get(), cell)) / scale;
        Some(jump_origin(self.bounds().size, size, track, dock))
    }

    /// "Jump to latest" was clicked: the window goes to the bottom — gliding,
    /// or at once where the scroll is not smooth (Reduce Motion, `snap`) —
    /// and the bar shows the move (`go_to_block`'s shape). A window that
    /// moved hides the button until the count is zero
    /// ([`PaneIvars::jumping`]); one that did not keeps it, or nothing would
    /// bring it back.
    pub(crate) fn jump_to_latest(&self) {
        let Some(session) = self.session() else {
            return;
        };
        let rows = session.unseen_rows();
        // The travel's end is the bottom: `scroll_to` clamps past it.
        let moved = session.scroll_to(f32::MAX, self.ivars().smooth_scroll.get());
        self.poke_scrollbar();
        if moved == Some(true) {
            self.ivars().jumping.set(Some(rows));
            self.ivars().jump.set(None, |_| None);
        }
    }

    /// The pointer is over this pane's scroll bar strip, or not
    /// (`bt_gpu::DisplayLink::set_scrollbar_hover`) — the view's tracking
    /// area says so in an unfocused pane too. Silent before the link is born.
    pub(crate) fn set_scrollbar_hover(&self, on: bool) {
        let (_, drag) = self.ivars().bar_pointer.get();
        self.ivars().bar_pointer.set((on, drag));
        if let Some(link) = self.link() {
            link.set_scrollbar_hover(on);
        }
        self.refresh_marks_wanted();
    }

    /// The scroll bar's thumb is held, or let go
    /// (`bt_gpu::DisplayLink::set_scrollbar_drag`). Silent before the link
    /// is born.
    pub(crate) fn set_scrollbar_drag(&self, on: bool) {
        let (hover, _) = self.ivars().bar_pointer.get();
        self.ivars().bar_pointer.set((hover, on));
        if let Some(link) = self.link() {
            link.set_scrollbar_drag(on);
        }
        self.refresh_marks_wanted();
    }

    /// Whether the block marks are wanted — the bar is wide, so its block
    /// lane is on screen: the always-up form, or the pointer over the strip
    /// or holding the thumb; in a pane that can be seen and in a form that
    /// draws at all. A change goes to the session
    /// (`bt_core::Session::set_block_marks`); becoming wanted drives the index
    /// at once — a motion frame never reaches `bt-core`, so waiting for one
    /// would never bring the marks. While not wanted the index stops where it
    /// is; the tip goes with the marks.
    pub(crate) fn refresh_marks_wanted(&self) {
        let ivars = self.ivars();
        let (hover, drag) = ivars.bar_pointer.get();
        let wanted = ivars.seen.get()
            && match ivars.scrollbar.get() {
                ScrollbarMode::Always => true,
                ScrollbarMode::Auto => hover || drag,
                ScrollbarMode::Never => false,
            };
        // Before the session there is nobody to tell: its start asks again.
        let Some(session) = self.session() else {
            return;
        };
        if ivars.marks_wanted.replace(wanted) == wanted {
            return;
        }
        session.set_block_marks(wanted);
        if wanted {
            self.kick_search();
        } else {
            self.hide_block_tip();
        }
    }

    /// Whether the scroll bar's thumb is held — a drag in progress.
    pub(crate) fn thumb_held(&self) -> bool {
        self.ivars().bar_pointer.get().1
    }

    /// The pane became seen or covered ([`TerminalPane::set_visible`]'s
    /// half for the block marks): a covered pane drives no index.
    pub(crate) fn set_seen(&self, seen: bool) {
        self.ivars().seen.set(seen);
        self.refresh_marks_wanted();
    }

    /// Shows the tip of the block mark `handle` beside it — `mark` is the
    /// mark's target and `strip` the bar's left edge, both in the terminal
    /// view's points — or hides it (`None`, or a block that is gone). The
    /// same mark again is a no-op; asks for no frame.
    pub(crate) fn show_block_tip(&self, handle: Option<BlockHandle>, mark: NSRect, strip: f64) {
        let ivars = self.ivars();
        if handle.is_some() && ivars.tip_for.get() == handle {
            return;
        }
        let info = handle
            .zip(self.session())
            .and_then(|(handle, session)| session.block_info(handle));
        let Some(info) = info else {
            self.hide_block_tip();
            return;
        };
        ivars.tip_for.set(handle);
        let parts = &ivars.block_tip;
        let meta = block_meta(&info, |started| self.time_of_day(started));
        let border = parts.tip.borderWidth();
        let room = (strip - TIP_OFFSET - TIP_MARGIN - 2.0 * border).min(TIP_MAX_WIDTH);
        let size = parts.fill(&info, &meta, room);
        parts
            .tip
            .setFrameFromContentFrame(NSRect::new(NSPoint::new(0.0, 0.0), size));
        let frame = parts.tip.frame();
        // The view is flipped, the pane is not: the mark's middle in the
        // pane's own space, then the tip clamped inside the pane.
        let view = self.view();
        let middle = NSPoint::new(strip, mark.origin.y + mark.size.height / 2.0);
        let at = self.convertPoint_fromView(middle, Some(view));
        let bounds = self.bounds();
        let x = (at.x - TIP_OFFSET - frame.size.width)
            .min(bounds.size.width - TIP_MARGIN - frame.size.width)
            .max(TIP_MARGIN);
        let y = (at.y - frame.size.height / 2.0)
            .min(bounds.size.height - TIP_MARGIN - frame.size.height)
            .max(TIP_MARGIN);
        parts.tip.setFrameOrigin(NSPoint::new(x, y));
        parts.tip.setHidden(false);
    }

    /// Whether the block tip is showing.
    pub(crate) fn block_tip_shown(&self) -> bool {
        self.ivars().tip_for.get().is_some()
    }

    /// Hides the block tip.
    pub(crate) fn hide_block_tip(&self) {
        self.ivars().tip_for.set(None);
        self.ivars().block_tip.tip.setHidden(true);
    }

    /// A click on the block mark `handle`: the block's row goes two rows
    /// below the window's top — gliding, or at once where the scroll is not
    /// smooth (Reduce Motion, `snap`) — and the bar shows the move. `true` if
    /// the mark still pointed at a block.
    pub(crate) fn go_to_block(&self, handle: BlockHandle) -> bool {
        let Some(session) = self.session() else {
            return false;
        };
        let Some(info) = session.block_info(handle) else {
            return false;
        };
        let top = info.depth.saturating_sub(2) as f32;
        session.scroll_to(top, self.ivars().smooth_scroll.get());
        self.poke_scrollbar();
        true
    }

    /// Reads the lines of the commands that started into the pane's command memory
    /// ([`bt_core::Session::remember_commands`]); the main queue's turn after a start.
    pub(crate) fn remember_commands(&self) {
        if let Some(session) = self.session() {
            session.remember_commands();
        }
    }

    /// The window goes to command `id`'s row: it goes two rows below the window's top, as a click
    /// on its block mark does ([`Self::go_to_block`]). `false` once its row has left the history
    /// or while a program holds the screen.
    pub(crate) fn show_command(&self, id: CommandId) -> bool {
        let Some(session) = self.session() else {
            return false;
        };
        let Some(depth) = session.command_depth(id) else {
            return false;
        };
        session.scroll_to(
            depth.saturating_sub(2) as f32,
            self.ivars().smooth_scroll.get(),
        );
        self.poke_scrollbar();
        true
    }

    /// A start (Unix seconds) as the user's short time of day — `NSDateFormatter`'s
    /// short time style follows the 12/24-hour preference.
    fn time_of_day(&self, started: u32) -> String {
        let format = self.ivars().time_format.get_or_init(|| {
            let format = NSDateFormatter::new();
            format.setDateStyle(NSDateFormatterStyle::NoStyle);
            format.setTimeStyle(NSDateFormatterStyle::ShortStyle);
            format
        });
        let date = NSDate::dateWithTimeIntervalSince1970(f64::from(started));
        format.stringFromDate(&date).to_string()
    }

    /// The terminal view.
    pub(crate) fn view(&self) -> &BateriView {
        &self.ivars().view
    }

    pub fn session(&self) -> Option<&Arc<Session>> {
        self.ivars().session.get()
    }

    /// Gives the renderer the settings' font with this pane's point-size
    /// delta as a **request** — the launch path ([`TerminalPane::new`]): the
    /// atlas opens in `start`'s `sync_geometry` and that also writes the font
    /// slot. The return value (whether it changed) is not a question here: the
    /// geometry has not been built at all yet.
    fn request_font(&self, font: &FontOptions) {
        let _ = self
            .ivars()
            .renderer
            .set_font(&self.ivars().zoom.get().apply(font));
    }

    /// The font in the file changed: the point-size delta is updated by
    /// [`Zoom::after_reload`]'s rule. It does not apply the font; after the
    /// settings are written [`TerminalPane::apply_font`] applies it.
    pub(crate) fn zoom_after_reload(&self, old: &FontOptions, new: &FontOptions) {
        let zoom = self.ivars().zoom.get().after_reload(old, new);
        self.ivars().zoom.set(zoom);
    }

    /// Consumes the rest of the birth package: decides the dock reserve,
    /// builds the first geometry and opens the session.
    ///
    /// The integration was asked **once** on the owner's side and gave both
    /// answers together: the child's environment and the dock's existence
    /// (`AppDelegate::shell_integration`, [`PaneLaunch::integration`]).
    /// With two separate calls the two could diverge — a session that loses
    /// two rows from the window but has no dock (or the reverse), and the
    /// symptom would be silent. **Before** the geometry: the grid height must
    /// see the dock reserve, otherwise the shell is born at launch with one
    /// row too many and the first frame eats a `TIOCSWINSZ` for the correction.
    ///
    /// `working_directory` is the caller's decision (the active
    /// tab's directory, else home). The error returns to the caller: in the
    /// first window the process exits, in ⌘T/⌘N only that window closes —
    /// the other tabs' shells must not die because a new one could not be born.
    ///
    /// `launch.initial_input` is the shell's first input (⌘T in
    /// a remote tab, `AppDelegate::open_window`'s decision); `None` → an
    /// ordinary local shell.
    ///
    /// The pane must be attached to a window (`contentView`): the scale is
    /// read from it ([`TerminalPane::sync_geometry`]); if not, an error. A
    /// second call is an error too: the package is consumed once.
    pub fn start(&self, mtm: MainThreadMarker) -> std::io::Result<()> {
        let Some(birth) = self.ivars().birth.take() else {
            return Err(std::io::Error::other("pane started a second time"));
        };
        let rows = birth.integration.dock_rows;
        self.ivars().dock_rows_at_birth.set(rows);
        self.ivars().dock_rows.set(rows);
        // The grid size derives from the window; the session is born with its
        // first size so the shell sees the right `TIOCSWINSZ` at launch.
        let Some(grid) = self.sync_geometry() else {
            return Err(std::io::Error::other("pane is not attached to a window"));
        };
        self.start_session(mtm, grid, birth)
    }

    /// Opens the session and attaches the link that drives frames. The order
    /// is required: `Session` wants `Wake`, the link wants `Session`, `Waker`
    /// is born from the link.
    fn start_session(
        &self,
        mtm: MainThreadMarker,
        grid: Grid,
        birth: Birth,
    ) -> std::io::Result<()> {
        let Birth {
            stats,
            settings,
            theme,
            launch,
            integration:
                Integration {
                    env: integration,
                    shell_args,
                    dock_rows: _,
                },
        } = birth;
        let Launch {
            working_directory,
            initial_input,
            uuid: _,
            replay,
            adopt,
            env: extra_env,
            held,
        } = launch;
        // Whether the pipe is started or a carried-on one goes on, the pane is the program's it
        // was asked for by: the carried pipe has the flag on its own command line, and the
        // pane's next save says it again.
        if let Some(held) = &held {
            self.ivars().asked_for.replace(AskedFor {
                close_at_end: held.close_at_end,
                opener: held.opener.clone(),
            });
        }
        // In smoke and measurement runs the shell is fixed: the result must not
        // depend on the user's `$SHELL` and rc file. The owner of the scripts
        // is `bt-core`; that `smoke_shell` gives eight cells and six glyphs is
        // tested there — so the `cells=8` and `glyphs=6` expectations are not a
        // documentation sentence but a tested claim.
        //
        // The branch asks for the **load**, not the duration: the same `Run`
        // sets up both the deadline and the guard, and the load is independent
        // of them.
        //
        // An untimed session's command and the shell's parent come from
        // one call ([`child::shell_command`]): on macOS `login(1)` with
        // `-q`, so its `Last login:` banner never lands on the grid (an
        // unresolved user or shell falls back to `None`, alacritty's own
        // `login` path). The timed run's scripts are the shell itself, so
        // they are `Direct`; deciding the parent in the same branch as the
        // command is what keeps the two from disagreeing.
        let (command, shell_parent) = match self.ivars().run {
            None => {
                let shell = child::shell_command(&shell_args);
                // A terminal a server keeps shown first: the program in front is the
                // pipe to it, and `shell_parent` is the shell's, since that is what the
                // process is once the terminal ends and the pipe has taken its place.
                match held
                    .as_ref()
                    .filter(|_| shell.0.is_some())
                    .and_then(|held| crate::launch::held_command(held, &integration, &shell_args))
                {
                    Some(pipe) => (Some(pipe), shell.1),
                    None => shell,
                }
            }
            Some(run) => (
                Some(match run.workload {
                    Workload::Smoke => smoke_shell(),
                    // The load's duration is the same as the deadline: if it
                    // falls short the window idles at the tail of the run and
                    // the measurement samples idle frames. A load without a
                    // duration is now **not representable** — `Run` carries the
                    // duration next to the load, so the old `unwrap_or(0)` and
                    // the `debug_assert` defending it are gone.
                    Workload::Load => load_shell(run.seconds),
                }),
                ShellParent::Direct,
            ),
        };
        // Whether the wrapper was installed: if the integration's environment
        // is non-empty the shell will print our identity (the `blocks` tier has
        // no dock but has marks, i.e. it cannot be derived from `dock`). Before
        // the environment is moved into `env` below.
        let shell_marks = !integration.is_empty();
        let options = SessionOptions {
            command,
            // Directory and locale follow the same rule in **every**
            // session, timed run included: the decision has a single arm,
            // without exception, and neither of
            // the two fixed scripts depends on directory or locale —
            // `printf` with `sleep`, `date` with `printf`; paths absolute
            // or from `PATH`, output ASCII.
            //
            // The directory now comes from the caller: a new tab is in the
            // active tab's OSC 7 directory; in a timed run
            // and the first window `child::working_directory()`.
            working_directory,
            // The title's `~` rule; **the same resolution** as the directory (`child::home`).
            home: child::home(),
            // Shell integration sits beside the locale, in the same map:
            // both are environment **added** to the child and both go only
            // to the child. Their keys are disjoint (`LANG` ↔ `ZDOTDIR`),
            // so order does not matter.
            // The integration's environment comes **from the caller**: the
            // same answer also determines the dock's existence (`start`)
            // and if it were asked a second time here the two decisions
            // could diverge.
            env: extra_env
                .into_iter()
                .chain(child::locale_env(locale::system_locale()))
                .chain(integration)
                .collect(),
            cols: grid.cols,
            rows: grid.rows,
            cell_px: grid.cell.cell_px(),
            terminal: settings.terminal(),
            theme,
            // The dock's **existence**, not its reserve: `bt-core` hands
            // the caret over accordingly. Its source is the birth-reserve
            // slot (`start` wrote it a line earlier) and the alternate-screen
            // notifier's gate reads the same slot too, so they cannot diverge.
            dock: self.ivars().dock_rows_at_birth.get() > 0,
            // Clustering is on in all windows, timed run included.
            // Not a settings key: rolling back is this one line.
            cluster: true,
            // A timed run always gets `None` from `open_window` (single
            // window, no ⌘T), so its fixed scripts are unaffected by this.
            initial_input,
            shell_marks,
            // The identity is in every window, timed run included: the
            // variables read no file and do not move the tokens.
            pane_uuid: Some(self.ivars().uuid.clone()),
            // The machine's name: `file://$HOST/…` (GNU `ls --hyperlink`)
            // and OSC 7's named authority count as local. One `gethostname`
            // per pane; the timed run's tokens do not depend on it.
            hostname: crate::links::hostname(),
            // A restored pane's scrollback; `None` everywhere else.
            replay,
            journal: self.open_journal(settings.terminal().scrollback),
            // A program that writes pencere frames is answered, and its view is
            // shown, in every pane but a timed run's: there the program is the
            // gate's own script, the pane's reader hears no frame and nothing is
            // written to it.
            views: self.view_options(shell_parent),
        };
        let wake = Arc::clone(&self.ivars().wake) as Arc<dyn Wake>;
        // The update's handover: the running program is carried
        // on; if the session cannot be adopted after all (its `bt-core` blob
        // does not decode) the pane falls back to a new shell here, with the
        // carried history and the note.
        let adopting = adopt.is_some();
        let mut nudge = false;
        let mut carried_on = false;
        // The process this one took the pane from had a program's view over it.
        let mut view_was_open = false;
        let mut carried = None;
        let (session, shell_parent) = match adopt {
            Some(adopted) => {
                let taken_from = adopted.taken_from.clone();
                let note = adopted.note;
                nudge = adopted.nudge;
                let program = adopted.state.program;
                let viewed = adopted.state.view;
                match adopt_session(options.clone(), adopted, grid, &wake) {
                    Ok((session, parent)) => {
                        self.ivars().taken_from.replace(taken_from);
                        carried = program;
                        carried_on = true;
                        view_was_open = viewed;
                        (session, parent)
                    }
                    Err((error, history)) => {
                        eprintln!("bateri: could not carry a pane over: {error}");
                        nudge = false;
                        let options = SessionOptions {
                            replay: Some(crate::launch::fallen_back(history, note)),
                            ..options
                        };
                        (Session::spawn(options, wake)?, shell_parent)
                    }
                }
            }
            // A terminal window without a shell is an empty box; what to do is
            // the caller's call (first window: the process exits; later ones:
            // that window closes).
            None => (Session::spawn(options, wake)?, shell_parent),
        };
        let session = Arc::new(session);
        // The program kept running through the handover and its view did not: what the view
        // lived on (the token, the files, the messages) went with the old process, so the
        // program is offered a restart and opens the view again. Offered, not forced: the
        // conversation answers only where a program is in front of the pane (§6.2), and a pane
        // that had no view gets nothing written.
        if carried_on && view_was_open {
            self.restart_view_offered(&session);
        }
        if nudge {
            nudge_program(&session);
        } else if carried_on {
            // The text came across and the images did not (they are not
            // carried): a program that redraws on `SIGWINCH` draws them
            // again. The size is untouched; a nudged program is already
            // redrawing, through its size.
            session.winch();
        }
        // The closing sequence reaches the session from here, not through the
        // link, and the keyboard holds its own copy; all three live on the main
        // thread, so where the last reference drops is clear (see `shutdown`).
        let _ = self.ivars().session.set(Arc::clone(&session));
        let _ = self.ivars().shell_parent.set(shell_parent);
        // What the pane showed it ran before it was carried over, kept for the command that ran
        // then (`ProgramMemo::carried`) — before the registration below, which sends it on.
        if let Some(kind) = carried
            && let Some(command) = session
                .last_command()
                .filter(|command| command.phase == CommandPhase::Running)
        {
            let memo = self.ivars().program_memo.get();
            self.ivars().program_memo.set(ProgramMemo {
                carried: Some((command.id, kind)),
                ..memo
            });
        }
        // A pane carried over mid-command (an update, a crash) saw that command start in the
        // process it came from, and no start comes here to look its program up: it is looked up
        // now, or the tab would show the shell until the next command.
        if adopting
            && session.running_command().is_some()
            && self.ivars().wake.identity_probe.command_started()
        {
            self.ivars().wake.dispatch_identity_probe(None);
        }
        // The host marks' list at birth; its live change comes
        // from `AppDelegate::reload_settings` ([`Self::set_host_marks`]).
        session.set_host_marks(&settings.remote_hosts);
        // The text's contrast floor, before the first frame (this turn of the
        // main thread); its live change is [`Self::set_minimum_contrast`].
        session.set_minimum_contrast(settings.minimum_contrast);
        // A title notification that arrived before the session entered the slot
        // may have found an empty slot and dropped; the window's `start` closes
        // that (`TerminalWindow::start` → `refresh_title`), right after this call returns.
        let view = &self.ivars().view;
        view.attach(Arc::clone(&session));
        // The mouse translation must see the same grid as the session: the
        // size and count are the same ones that went to the `SessionOptions`
        // above. On the `resize` path the same triple is also written together
        // (`refresh_geometry`).
        view.set_metrics(grid, self.ivars().dock_rows.get());
        // The rhythm is the view's display link, as a timer; it gets the
        // frame loop to tick right below.
        let pacer = MacPacer::new(mtm, view);
        let link = DisplayLink::new(
            Arc::clone(&pacer) as Arc<dyn Pacer>,
            Rc::clone(&self.ivars().surface),
            Rc::clone(&self.ivars().renderer),
            session,
            Layout {
                dock_cols: grid.dock_cols,
                context: grid.context,
                dock_rows: self.ivars().dock_rows.get(),
                cell: grid.cell,
            },
            stats,
            // **Every pane watches**: a transition turns the tab's ring off or
            // on (`tabs::ring_running`), dock or not. In a dockless session it
            // still resizes nothing — `grid::dock_rows_for` gives a zero-row
            // birth zero on both screens, so the reserve never moves.
            Some(alt_screen_notifier(self.ivars().id, self.ivars().lookup)),
        );
        pacer.attach(mtm, link.ticker());
        // We do not want frames before the wake path is closed: a `Wakeup`
        // in between would be dropped silently.
        //
        // audit: `start_session` is called once per window, from `start`.
        // A silently swallowed `Err` here would produce the most insidious bug:
        // the old link's `Waker` stays, the window never wakes to shell output
        // again and not a single line of trace remains.
        assert!(
            self.ivars().wake.slot().replace(link.waker()).is_none(),
            "waker set a second time"
        );
        // The mouse mapping's vertical origin: like `set_metrics` it comes not
        // from the window but from the **frame** path, so once, after the link
        // is born. The mouse thus reads the drawn offset; a second computation
        // would mean "the click is off by a row" (`bt_gpu::Origin`).
        view.attach_origin(link.origin());
        // The cursor's style is the setting's too: the link is born with
        // `CursorMotion::default()` and the call here pulls it to the file's
        // value (in a hermetic run `Settings::default()`'s). `set_font`'s place
        // is `load_settings` but the style's cannot be: the link does not exist yet.
        // The dock's typing effects are here for the same reason.
        link.set_cursor_motion(settings.cursor_motion);
        link.set_glyph_fx(settings.keypress, settings.erase);
        link.set_dock_arrival(settings.dock_arrival);
        // The scroll bar's form, for the same reason; the grid above was
        // already sized with its reserve (`sync_geometry`).
        link.set_scrollbar_mode(self.ivars().scrollbar.get());
        // The top edge's mode, the same way: the link is born with `Fade` and
        // the grid above was cut with this mode's reserve — the two must agree
        // before the first frame.
        link.set_content_edge(self.ivars().content_edge.get());
        // The pointer's hand and tip over the block marks follow the drawn
        // marks, not only the pointer's moves.
        link.on_marks_published(marks_notifier(self.ivars().id, self.ivars().lookup));
        // Opening frame: `Session` is born dirty, we open the link once by hand.
        link.request_frame();
        let _ = self.ivars().link.set(link);
        // Reduce Motion **after** the link enters the slot: the first value
        // (the birth package's resolved value) lands on this pane's link from
        // here, the system's notification and the settings write later reach
        // all panes through `AppDelegate::apply_reduce_motion`. In a hermetic
        // run the resolved value is `false` and the link is born with that
        // value, so the call is a no-op (`DisplayLink::set_reduce_motion`).
        self.set_reduce_motion(self.ivars().reduce_motion.get());
        // The wheel's mode comes from the same resolved input (Reduce Motion is
        // the setting's third input) and the same later path (`apply_reduce_motion`).
        self.set_smooth_scroll(self.ivars().smooth_scroll.get());
        // The cursor's drawing numbers also land once at launch and are read
        // **from the slot**, not from the `link` in hand: the link was moved
        // into the slot in that call. `set_caret_style` is a no-op on the same
        // value, so it does not collide with the save-time path.
        self.apply_caret(&settings);
        // **The focus is seeded too** and the reason is the same ordering: the
        // window becomes key with `makeKeyAndOrderFront`, so
        // `windowDidBecomeKey:` fires **before** the link enters the slot and
        // that call is silently dropped. Without the seeding, in a window
        // opened in the background (`open -g`, a login item, a script-launched
        // open while another application is in front) no notification would
        // arrive and `focused` would stay `true`: an unfocused window would
        // draw a filled caret and set up the blink clock.
        self.apply_focus(self.is_active());
        // **And the keyboard**, for the same ordering: the view became first
        // responder, or another pane did, before the link entered the slot, and
        // the link is born believing the keyboard is its own. Unseeded, every
        // pane of a window opened with a split (a restored session, a tab torn
        // off with its panes) would draw a filled, blinking caret until the
        // keyboard came to it and left.
        if self.ivars().run.is_none()
            && let Some(link) = self.ivars().link.get()
        {
            link.set_keyboard_in_terminal(self.view().is_first_responder());
        }
        // **The dock's arrival**, for a shell born here and only that: a pane
        // that adopted a running session (the update's handover, a crash
        // recovery) has a shell at its prompt already, and the timed run
        // (`run`) has no dock. After the focus above: a tab born in a window
        // that is key is still hidden at this point, and the link asks for
        // focus only when the prompt comes. The link itself arms nothing for a
        // pane without a dock or one that is hidden.
        if !adopting
            && self.ivars().run.is_none()
            && let Some(link) = self.link()
        {
            link.arm_arrival(settings.dock_arrival);
        }
        // The block marks, for the same ordering: the always-up form wants
        // them from the first frame, and no change will say so.
        self.refresh_marks_wanted();
        // A carried-on session can be on the alternate screen already (vim
        // across the update): the link was born seeing it, so no transition
        // will ever take the dock away — the reserve is matched here, once.
        if adopting {
            self.alt_screen_did_change();
            // The remote target is not carried (the process table is its
            // source) and a carried `ssh` gives no new `C` edge:
            // the probe is armed once by hand, so `⇄ host`, the masters'
            // session and the lazy helper come back without a prompt.
            if self
                .session()
                .is_some_and(|session| session.running_command().is_some())
            {
                Wake::command_started(&*self.ivars().wake);
            }
        }
        // Last: the session is in its slot and the pane in its window, so the
        // layout the registration sends at once places this pane.
        self.register_with_holder(mtm);
        Ok(())
    }

    /// The session's journal at birth: the timed run's `BT_JOURNAL` in this
    /// process; a region in shared memory while a bound holder keeps the
    /// programs (it rebuilds the screen after a crash); none otherwise — under
    /// `"update"` the journal would cost the reader thread for nothing.
    fn open_journal(&self, scrollback: usize) -> Option<Arc<bt_core::Journal>> {
        if let Some(run) = self.ivars().run {
            return run.journal.then(bt_core::Journal::in_memory);
        }
        let keeper = self.ivars().keeper.as_deref()?;
        if !keeper.is_active() {
            return None;
        }
        match PaneJournal::open(scrollback) {
            Ok(journal) => {
                let shared = Arc::clone(journal.journal());
                self.ivars().journal.replace(Some(journal));
                Some(shared)
            }
            Err(error) => {
                eprintln!("bateri: no journal for a pane ({error}); a crash loses its screen");
                None
            }
        }
    }

    /// Breaks this pane's journal ([`PaneJournal::break_journal`]): no holder
    /// will confirm its bases any more — `"update"`, or a holder that cannot
    /// be replaced. A crash brings the screen back from its program's
    /// redraw.
    pub(crate) fn break_journal(&self) {
        if let Some(journal) = self.ivars().journal.borrow().as_ref() {
            journal.break_journal();
        }
    }

    /// Registers this pane with the bound holder ([`Keeper::add`]): a copy
    /// of the master (close-on-exec), the identity, the child and its start
    /// time, the shell's parent and `bt-core`'s state now — a crash right
    /// after still leaves a bundle. A carried-on pane goes unconfirmed, named
    /// by the holder it was taken from, until that holder is acknowledged.
    /// A journaled pane sends its region and its current base along
    /// ([`PaneJournal::register`]), and its later bases follow as the
    /// compaction makes them; a pane without a journal (born under
    /// `"update"`, or its journal broke) sends none — a stale screen must not
    /// come back as the whole one.
    ///
    /// A no-op without a bound holder, for a closed pane, or when the child's
    /// start time or the master's copy cannot be read (the program then ends
    /// with bateri, as before); the journal is then no holder's and frees
    /// itself ([`PaneJournal::disconnect`]) — a failure of the moment must not
    /// cost the screen for good, the next registration carries it again.
    /// Called again for every live pane when a holder is (re)spawned.
    pub(crate) fn register_with_holder(&self, mtm: MainThreadMarker) {
        if !self.try_register(mtm)
            && let Some(journal) = self.ivars().journal.borrow().as_ref()
        {
            journal.disconnect();
        }
    }

    /// [`TerminalPane::register_with_holder`]'s body; `false` if the pane
    /// was not registered.
    fn try_register(&self, mtm: MainThreadMarker) -> bool {
        let Some(keeper) = self.ivars().keeper.as_deref() else {
            return false;
        };
        if !keeper.is_active() || self.is_closed() {
            return false;
        }
        let Some(session) = self.session() else {
            return false;
        };
        let Some(&parent) = self.ivars().shell_parent.get() else {
            return false;
        };
        let pid = session.child_pid();
        let Some(start) = jobs::start_time(pid) else {
            return false;
        };
        let Some(master) = session.with_pty_fd(|fd| fd.try_clone_to_owned().ok()) else {
            return false;
        };
        let pane = |journal| crate::handover::BoundPane {
            tab: self.ivars().uuid.clone(),
            pid,
            start,
            parent,
            blob: session.state_blob(),
            program: self.carried_program(),
            master,
            taken_from: self.ivars().taken_from.take(),
            journal,
        };
        match self.ivars().journal.borrow().as_ref() {
            Some(journal) => journal.register(|registration| {
                let bound = registration.map(|registration| crate::handover::BoundJournal {
                    registration,
                    journal: Arc::downgrade(journal.journal()),
                });
                keeper.add(mtm, pane(bound))
            }),
            None => {
                keeper.add(mtm, pane(None));
            }
        }
        true
    }

    /// Sends this pane's current `bt-core` state and program to the bound
    /// holder — the shell's edges, a program that said what it is or was
    /// found to be, and, delayed, the mirror's changes ([`ShellWake`]'s
    /// sends). A no-op without a holder or once closed.
    pub(crate) fn push_state(&self) {
        let Some(keeper) = self.ivars().keeper.as_deref() else {
            return;
        };
        if self.is_closed() || !keeper.is_active() {
            return;
        }
        if let Some(session) = self.session() {
            keeper.state(
                &self.ivars().uuid,
                session.state_blob(),
                self.carried_program(),
            );
        }
    }

    /// The alternate screen changed: the tab's ring is read again and the
    /// dock goes away or comes back.
    ///
    /// The sender is the frame path's notifier ([`alt_screen_notifier`]) and
    /// this method runs **on the next main-queue turn** — so as not to pull
    /// the rug from under a drawn frame.
    ///
    /// **It re-reads the truth**, ignoring what the notification carried: if
    /// two transitions chase each other (vim open-close) both jobs waiting in
    /// the queue see the same, current answer. If the dock's share did not
    /// change it **does not resize** — this gate upholds the "one resize per
    /// transition" claim.
    pub(crate) fn alt_screen_did_change(&self) {
        let Some(session) = self.ivars().session.get() else {
            return;
        };
        // Before the dock's gate: a full-screen program gets no ring
        // (`tabs::ring_running`) whether or not the window has a dock to give
        // back. Reading again is idempotent — the ended commands' counts it
        // also reads only mark what is new.
        self.host().activity_changed(self.ivars().id);
        let wanted = grid::dock_rows_for(
            session.alt_screen(),
            session.remote_mark().is_some(),
            self.ivars().dock_rows_at_birth.get(),
        );
        if self.ivars().dock_rows.replace(wanted) == wanted {
            return;
        }
        // Reserve, grid and link **in a single block**: the frame path is on the
        // main thread too, so no frame can slip in between and no half state is drawn.
        self.refresh_geometry();
    }

    /// View ▸ Bigger (Cmd +): the point-size delta grows one step.
    pub(crate) fn zoom_in(&self) {
        self.change_zoom(Zoom::bigger);
    }

    /// View ▸ Smaller (Cmd −): the point-size delta shrinks one step.
    pub(crate) fn zoom_out(&self) {
        self.change_zoom(Zoom::smaller);
    }

    /// View ▸ Actual Size (Cmd 0): the delta is reset, the setting's point size.
    pub(crate) fn zoom_reset(&self) {
        self.change_zoom(|_, _| Zoom::default());
    }

    /// Bigger, Smaller, Actual Size: changes this pane's temporary point-size
    /// delta with `step` and applies the font. It does not touch the file and
    /// also works in a timed run — it reads nothing from the user's world.
    fn change_zoom(&self, step: impl FnOnce(Zoom, &FontOptions) -> Zoom) {
        let zoom = step(self.ivars().zoom.get(), &self.ivars().font.borrow());
        self.ivars().zoom.set(zoom);
        self.apply_font();
        // The point-size step is part of the layout the bound holder keeps.
        if let Some(keeper) = self.ivars().keeper.as_deref() {
            keeper.layout_changed();
        }
    }

    /// The setting's font changed (`AppDelegate::reload_settings`, after the
    /// delta was updated by [`TerminalPane::zoom_after_reload`]): it is
    /// stored and applied.
    pub(crate) fn set_font(&self, font: &FontOptions) {
        self.ivars().font.replace(font.clone());
        // The view is given the family too (its colours follow `set_theme`).
        if let Some(session) = self.session() {
            session.set_view_font(font.family.as_deref());
        }
        self.apply_font();
    }

    /// Gives the renderer the settings' font with this pane's temporary
    /// point-size delta; if the request changed the geometry is rebuilt
    /// ([`TerminalPane::refresh_geometry`]: atlas, grid, PTY size, font slot).
    ///
    /// Two gates, both needed: the caller's gate (delta, press) says something
    /// changed, `set_font` says whether the renderer already wants that font —
    /// a press at the limit or a save that writes the same point size as the
    /// delta does not rebuild the atlas.
    fn apply_font(&self) {
        let font = self.ivars().zoom.get().apply(&self.ivars().font.borrow());
        if self.ivars().renderer.set_font(&font) {
            self.refresh_geometry();
        }
    }

    /// `[remote]` changed — the `hosts` pattern list goes to the session; the
    /// active remote host's mark is re-resolved there. The tab's
    /// dot is the window's job (`TerminalWindow::set_host_marks`). The preview
    /// and download keys are kept here for the next download.
    pub(crate) fn set_host_marks(&self, settings: &Settings) {
        if let Some(session) = self.ivars().session.get() {
            session.set_host_marks(&settings.remote_hosts);
        }
        self.ivars()
            .remote_files
            .replace(settings.remote_files.clone());
    }

    /// `[appearance] minimum_contrast` changed — to the session, which draws
    /// its next frame with it (no-op on the same value,
    /// `Session::set_minimum_contrast`).
    pub(crate) fn set_minimum_contrast(&self, ratio: f64) {
        if let Some(session) = self.ivars().session.get() {
            session.set_minimum_contrast(ratio);
        }
    }

    /// Terminal options changed — to the session, **in full**.
    pub(crate) fn set_terminal_options(&self, settings: &Settings) {
        if let Some(session) = self.ivars().session.get() {
            session.set_terminal_options(settings.terminal());
        }
    }

    /// Swaps the theme into the session (no-op on the same theme,
    /// `Session::set_theme`) and paints the search panel with it; if the panel
    /// is not born yet it is painted with the session's theme on the first ⌘F.
    /// In bateri the window paints the chrome and the tab bar around it
    /// (`TerminalWindow::set_theme`); a host paints its own.
    pub fn set_theme(&self, theme: Theme) {
        if let Some(session) = self.ivars().session.get() {
            session.set_theme(theme);
        }
        if let Some(host) = self.view_host() {
            host.paint(&theme);
        }
        if let Some(bar) = self.ivars().search.get() {
            bar.paint(&theme, theme.is_dark());
        }
        self.ivars().dim.paint(&theme);
        self.ivars().link_veil.paint_with(&theme, LINK_VEIL_ALPHA);
        self.ivars().card_frame.paint(&theme);
        self.ivars().link_label.0.paint(&theme);
        self.ivars().block_tip.paint(&theme);
        self.ivars().jump.paint(&theme);
    }

    /// Whether the pane shows the focus (its card's light, its footing).
    pub(crate) fn shows_focus(&self) -> bool {
        self.ivars().card_frame.focused()
    }

    /// What tells this pane's focus: the focused card stands nearer (its
    /// light and its footing, [`crate::card`]), and `veiled` shows the dim
    /// veil over an unfocused one (`[appearance] dim_unfocused_splits`). The
    /// decision is the owner's (`TerminalTab::refresh_look`); it asks for no
    /// frame — both are AppKit's.
    pub(crate) fn set_focus_look(&self, focused: bool, veiled: bool) {
        self.ivars().card_frame.set_focused(focused);
        self.ivars().dim.setHidden(!veiled);
    }

    // ─── The connection of a terminal the server keeps ───────────────────
    //
    // A pane that shows a terminal kablo keeps on a server outlives the
    // connection to it: while the application's bridge is gone the pane shows
    // the terminal's last screen and waits. The word comes from the bridge's
    // state in the application (`hold_service`, `Backend::link_changed`) and
    // not from `ssh-hold`, which is never told — the holder lives on and the
    // pane's PTY with it.

    /// The bridge to the server of the terminal this pane shows went, or is
    /// back. A **passive** pane ([`PaneLink::is_passive`]) shows its last screen
    /// under a veil over the grid, with no caret, takes no keys but ⏎ and says
    /// in the dock's context row what is going on
    /// ([`bt_core::RemoteLink`]). All of it is CoreAnimation's or one change of
    /// the dock's text: the wait, the clock that tries again and the wake from
    /// sleep ask for no frame (zero frames while idle) — the change itself asks
    /// for one.
    pub(crate) fn set_connection(&self, link: PaneLink) {
        let before = self.ivars().connection.replace(link);
        if before == link {
            return;
        }
        self.ivars().link_veil.setHidden(!link.is_passive());
        self.place_link_veil();
        self.apply_connection();
        // The screen came back, not the messages a program's view lives on: a view that
        // is open is offered to the program again. Once per change, like everything above
        // (`PaneLink::comes_back`): a reconnection's failed attempts are no news.
        if before.comes_back(link)
            && self.shows_view()
            && let Some(session) = self.session().cloned()
        {
            self.restart_view_offered(&session);
        }
    }

    /// How the connection stands for this pane's terminal.
    pub(crate) fn connection(&self) -> PaneLink {
        self.ivars().connection.get()
    }

    /// Writes the connection's state where the dock and the caret read it. Also
    /// called when the remote state lands or changes
    /// ([`TerminalPane::remote_or_title_changed`]): the session drops the word
    /// with the remote session it belonged to, and a bridge's news may come
    /// before the probe that knows the session is remote.
    fn apply_connection(&self) {
        let Some(session) = self.session() else {
            return;
        };
        let link = self.ivars().connection.get();
        // Who took the terminal is the service's to say; the word and the name land together.
        let by = (link == PaneLink::Taken)
            .then(|| self.host().kept_terminal(self.uuid()))
            .flatten()
            .and_then(|shown| shown.taken_by);
        session.set_remote_link(
            match link {
                PaneLink::Reconnecting => Some(bt_core::RemoteLink::Reconnecting),
                PaneLink::NeedsLogin => Some(bt_core::RemoteLink::LoginNeeded),
                PaneLink::Taken => Some(bt_core::RemoteLink::Taken),
                PaneLink::Connecting | PaneLink::Live => None,
            },
            by.as_deref(),
        );
    }

    /// The veil's frame: the pane without the dock's band, in the pane's own
    /// (unflipped) points. The band is the grid's reserve for it
    /// ([`bt_gpu::dock_px`], what "Jump to latest" stands on): the context row
    /// at the bottom stays clear and keeps saying what is happening.
    fn place_link_veil(&self) {
        let ivars = self.ivars();
        if ivars.link_veil.isHidden() {
            return;
        }
        let Some(window) = self.window() else {
            return;
        };
        let scale = window.backingScaleFactor();
        let dock = f64::from(bt_gpu::dock_px(
            ivars.dock_rows.get(),
            self.cell_metrics(scale),
        )) / scale;
        let bounds = self.bounds();
        let dock = dock.min(bounds.size.height);
        ivars.link_veil.setFrame(NSRect::new(
            NSPoint::new(bounds.origin.x, bounds.origin.y + dock),
            NSSize::new(bounds.size.width, bounds.size.height - dock),
        ));
    }

    /// A key, a paste or a press reached a pane: `true` when the pane takes it
    /// because its terminal is not there to be typed to. Nothing goes to the
    /// terminal (a line typed into the wait and sent when the connection comes
    /// back would run where nobody looks) and the system beeps — except ⏎, which
    /// is the one key the wait has: try now, or log in when the server wants a
    /// password nobody saved.
    pub(crate) fn hold_input(&self, enter: bool) -> bool {
        match on_wait(self.connection(), enter) {
            Wait::Pass => return false,
            Wait::LogIn => self.log_in_to_reconnect(),
            Wait::TryNow => self.host().retry_kept_terminal(self.uuid()),
            Wait::BringBack => self.host().bring_back_kept_terminal(self.uuid()),
            Wait::Beep => crate::preview::beep(),
        }
        true
    }

    /// Makes the pane one of several cards, or the whole tab again: rounds and
    /// clips its layer and shows or hides the light on it ([`crate::card`]).
    /// The container calls it on every layout.
    pub(crate) fn set_card(&self, carded: bool) {
        self.ivars().card_frame.set_carded(carded);
        if let Some(layer) = self.layer() {
            card::round(&layer, carded);
        }
    }

    /// Tells the pane the radii, in points, of its bottom left and right
    /// corners — what the window's or the card's curve takes from the text:
    /// the grid, the dock and the pointer start further in beside a round
    /// corner (the rule is [`CellMetrics::with_corners`]'s). The container
    /// calls it on every layout, which the window repeats when full screen
    /// comes or goes; a value that is not a radius (NaN, negative, infinite)
    /// is a square corner and an absurdly large one is capped
    /// ([`CellMetrics::with_corners`]). The geometry is taken again when the
    /// value moves — and only then: the temporary look of the arrangement
    /// ([`crate::arrange`]) does not tell corners, since a column count that
    /// moved would send the shell a `SIGWINCH` for a look that lasts a moment.
    pub fn set_corners(&self, left: f64, right: f64) {
        if self.take_corners(left, right) {
            self.refresh_geometry();
        }
    }

    /// Sets the corners and the frame together, for a container that moves a
    /// pane and rounds it in one layout: the frame's own notification takes
    /// the geometry once, with the new corners already in it, so the shell
    /// never gets the size of a pane that was never drawn (the old frame with
    /// the new corners). When the frame does not move there is no
    /// notification and the corners alone take the geometry again.
    pub(crate) fn place(&self, frame: NSRect, left: f64, right: f64) {
        let moved = self.take_corners(left, right);
        let same_frame = self.frame() == frame;
        self.setFrame(frame);
        if moved && same_frame {
            self.refresh_geometry();
        }
    }

    /// Stores the corners (a value that is not a radius is square) and says
    /// whether they moved; the geometry is the caller's to take.
    fn take_corners(&self, left: f64, right: f64) -> bool {
        let radius = |value: f64| {
            if value.is_finite() {
                value.max(0.0)
            } else {
                0.0
            }
        };
        let corners = (radius(left), radius(right));
        self.ivars().corners.replace(corners) != corners
    }

    /// The renderer's metrics at `scale` with this pane's corners applied:
    /// the **one** way the pane asks for them, so the grid's columns, the
    /// minimum size, the keyboard step and the jump button all read the same
    /// margins the drawing and the mouse do.
    fn cell_metrics(&self, scale: f64) -> CellMetrics {
        let (left, right) = self.ivars().corners.get();
        self.ivars()
            .renderer
            .cell_metrics(scale)
            .with_corners(left, right)
    }

    /// The pane size, in points, nearest to `size` at which the grid leaves
    /// nothing over ([`grid::fitted_size`], with the arithmetic's own inputs:
    /// this pane's cell, scroll bar form, top edge and dock) — what a window
    /// resizing a whole cell at a time asks of the pane that fills it. `None`
    /// off a window.
    pub(crate) fn fitted_size(&self, size: NSSize) -> Option<NSSize> {
        let scale = self.window()?.backingScaleFactor();
        let ivars = self.ivars();
        let cell = self.cell_metrics(scale);
        let reserve = ivars.scrollbar.get().reserve_px(cell);
        let top = bt_gpu::edge_reserve_px(ivars.content_edge.get(), cell);
        let (width, height) = grid::fitted_size(
            size.width * scale,
            size.height * scale,
            cell,
            self.reserved_dock_rows(),
            reserve,
            top,
        );
        Some(NSSize::new(width / scale, height / scale))
    }

    /// The dock rows the grid's arithmetic reserves: before
    /// [`TerminalPane::start`] the birth package's (the one `start` will
    /// set), so a size asked for before the shell exists is the size it will
    /// have — session restore checks the saved tree against the smallest
    /// pane, and a new window is fitted to its cells, before any shell starts.
    fn reserved_dock_rows(&self) -> u16 {
        self.ivars()
            .birth
            .borrow()
            .as_ref()
            .map_or(self.ivars().dock_rows.get(), |birth| {
                birth.integration.dock_rows
            })
    }

    /// The pane is lifted for arranging ([`crate::arrange`]), or set down:
    /// its frame reads at the focused strength while lifted.
    pub(crate) fn set_raised(&self, raised: bool) {
        self.ivars().card_frame.set_raised(raised);
    }

    /// The search panel's frame while it is open, in the pane's own
    /// coordinates (the arrangement's capsule keeps clear of it).
    pub(crate) fn search_frame(&self) -> Option<NSRect> {
        let bar = self.ivars().search.get().filter(|bar| bar.is_shown())?;
        Some(bar.resting_frame())
    }

    /// Whether the pane holds a question — one up on the tab's owner for
    /// it, or parked for a place to open.
    pub(crate) fn asking(&self) -> bool {
        !self.ivars().opened.borrow().is_empty() || !self.ivars().parked.borrow().is_empty()
    }

    /// Whether the pane is a card now.
    pub(crate) fn is_card(&self) -> bool {
        self.ivars().card_frame.carded()
    }

    /// The card's frame, for the slide that fades it.
    pub(crate) fn frame_view(&self) -> &NSView {
        &self.ivars().card_frame
    }

    /// Shows the ⌘-hovered OSC 8 link's target in the bottom-left label, or
    /// hides it (`None`). The caller is `hyperlink`'s hover: only
    /// with ⌘ and only for an OSC 8 link — a plain-text link is its own target.
    /// The width is the text's, at most the pane's minus the margins; asks for
    /// no frame.
    /// The bottom-left label's text while it shows; `None` hidden.
    pub(crate) fn link_target_text(&self) -> Option<String> {
        let (label, text) = &self.ivars().link_label;
        (!label.isHidden()).then(|| text.stringValue().to_string())
    }

    pub(crate) fn set_link_target(&self, target: Option<&str>) {
        let (label, text) = &self.ivars().link_label;
        let Some(target) = target else {
            label.setHidden(true);
            return;
        };
        text.setStringValue(&NSString::from_str(target));
        let fit = text.fittingSize();
        let border = label.borderWidth();
        let room = self.bounds().size.width - 2.0 * (LINK_LABEL_MARGIN + LINK_LABEL_PAD_X + border);
        let width = fit.width.min(room).max(0.0);
        label.setFrameFromContentFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(width, fit.height),
        ));
        label.setFrameOrigin(NSPoint::new(LINK_LABEL_MARGIN, LINK_LABEL_MARGIN));
        label.setHidden(false);
    }

    /// The cursor's style and the dock's typing effects go to the link, not to
    /// the session: they change **how** we draw, not which frame we draw.
    /// The effects descend **raw**; the reduction for `snap` and Reduce Motion
    /// is in `bt-gpu` (`DisplayLink::set_glyph_fx`).
    pub(crate) fn set_cursor_motion(&self, settings: &Settings) {
        if let Some(link) = self.ivars().link.get() {
            link.set_cursor_motion(settings.cursor_motion);
            link.set_glyph_fx(settings.keypress, settings.erase);
            // A scene playing now is finished; the new kind plays at the next
            // shell's birth.
            link.set_dock_arrival(settings.dock_arrival);
        }
    }

    /// A key was pressed in this pane: the dock's arrival, if it is still
    /// waiting or playing, ends at the dock's last state. Silent before the
    /// link is born.
    pub(crate) fn skip_arrival(&self) {
        if let Some(link) = self.link() {
            link.skip_arrival();
        }
    }

    /// Gives the link the cursor's values that descend from the settings.
    ///
    /// **Launch and save time go through the same code** and the reason is a
    /// class of defect: had the two lists been written
    /// separately they could drift — a key seeded only at launch would not
    /// apply at save time, a key only reloaded would stay at the default at
    /// launch. Both are silent: a key that descends halfway shows up in no
    /// gate.
    ///
    /// A single `Changes::caret` field, two calls: the destinations are
    /// separate (drawing numbers to `Frame`, the period to `bt_gpu::blink`)
    /// but both are the result of the same save — the precedent is the two
    /// keys of `Changes::motion`.
    ///
    /// It returns silently if there is no link: the launch call will give the
    /// same value anyway.
    pub(crate) fn apply_caret(&self, settings: &Settings) {
        let Some(link) = self.ivars().link.get() else {
            return;
        };
        link.set_caret_style(settings.caret);
        link.set_blink_interval(settings.blink_interval);
    }

    /// Gives the link Reduce Motion's **resolved** value
    /// (`AppDelegate::reduce_motion`). A no-op if the value did not change
    /// (`bt_gpu::DisplayLink::set_reduce_motion`); returns silently if there is no link.
    pub(crate) fn set_reduce_motion(&self, reduce: bool) {
        self.ivars().reduce_motion.set(reduce);
        if let Some(link) = self.ivars().link.get() {
            link.set_reduce_motion(reduce);
        }
    }

    /// Gives the pane the scroll bar's **resolved** form
    /// (`AppDelegate::scrollbar_mode`). A no-op on the same form. A new one
    /// goes to the link, and when the always-up form comes or goes the grid
    /// is resized by the track through the one geometry path
    /// ([`TerminalPane::refresh_geometry`]: the PTY's size, the mouse's
    /// metrics and the link's, the dock's columns staying the window's).
    ///
    /// **The split's proportions are not touched**: no `equalize`, and the
    /// smallest pane's size ([`TerminalPane::min_size`]) does not grow by the
    /// track — a pane already at its narrowest gives the track's columns up
    /// from its text rather than having the user's layout reset when a mouse
    /// is plugged in.
    pub(crate) fn set_scrollbar_mode(&self, mode: ScrollbarMode) {
        let before = self.ivars().scrollbar.replace(mode);
        if before == mode {
            return;
        }
        if let Some(link) = self.ivars().link.get() {
            link.set_scrollbar_mode(mode);
        }
        if before.reserves() != mode.reserves() {
            self.refresh_geometry();
        }
        self.refresh_marks_wanted();
    }

    /// Gives the pane what the content does at its top edge (`[appearance]
    /// content_edge`; `TerminalTab::set_content_edge` is the one caller).
    /// A no-op on the same mode. A new one goes to the link, and when the
    /// fade comes or goes ([`bt_gpu::edge_fades`]) the grid is resized
    /// through the one geometry path ([`TerminalPane::refresh_geometry`]) —
    /// **always**, not only when the row count changes: at a height where
    /// both modes give the same rows the PTY sees no new size, yet the grid
    /// moves by the fade and the caret must snap with it, which only the
    /// geometry refresh does. Between `line` and `cut` nothing moves here;
    /// the line is the container's (`SplitView::set_content_edge`).
    ///
    /// The split's proportions are not touched, for
    /// [`TerminalPane::set_scrollbar_mode`]'s reason: the smallest pane's
    /// size does not grow by the reserve ([`TerminalPane::min_size`]).
    pub(crate) fn set_content_edge(&self, edge: ContentEdge) {
        let before = self.ivars().content_edge.replace(edge);
        if before == edge {
            return;
        }
        if let Some(link) = self.ivars().link.get() {
            link.set_content_edge(edge);
        }
        if bt_gpu::edge_fades(before) != bt_gpu::edge_fades(edge) {
            self.refresh_geometry();
        }
    }

    /// Gives the view the scrolling's **resolved** mode
    /// (`AppDelegate::smooth_scroll`). To the view, not the link: the decision
    /// is made in the event's classification, in `scrollWheel:`, and the
    /// `false` arm is the very same as today's line path.
    pub(crate) fn set_smooth_scroll(&self, smooth: bool) {
        self.ivars().smooth_scroll.set(smooth);
        self.ivars().view.set_smooth_scroll(smooth);
    }

    /// The keyboard came to the terminal (`here`) or went to the search field
    /// — `BateriView`'s first-responder hooks supply it. The focus's
    /// second bit; the combination of the two bits is in `bt-gpu`
    /// (`DisplayLink::set_keyboard_in_terminal`). In a timed run it stays
    /// silent through [`TerminalPane::apply_focus`]'s gate.
    ///
    /// The keyboard's arrival also goes to the owner as a focus event
    /// ([`PaneHost::focused`]); its departure does not, because a keyboard
    /// moving to the search field stays in the same pane.
    pub(crate) fn keyboard_moved(&self, here: bool) {
        if self.ivars().run.is_some() {
            return;
        }
        if let Some(link) = self.ivars().link.get() {
            link.set_keyboard_in_terminal(here);
        }
        self.report_focus(self.is_active(), here);
        if here {
            self.host().focused(self.ivars().id);
        }
    }

    /// Tells the program in the terminal whether it has the keyboard
    /// ([`Session::set_focus`], heard only by one that asked): the pane's
    /// window is the user's (`window`) **and** the keyboard is in its terminal
    /// (`keyboard`) — the two bits the caret fills on, so a program hears focus
    /// exactly when the caret is solid. The keyboard in the search field is
    /// away from the program, as it is in another pane of the split.
    fn report_focus(&self, window: bool, keyboard: bool) {
        if let Some(session) = self.session() {
            session.set_focus(window && keyboard);
        }
    }

    /// The smallest pane's size, in points: a pane whose grid
    /// is exactly [`MIN_PANE_COLS`] × [`MIN_PANE_ROWS`] — the inverse of
    /// [`split_into_grid`] (left gutter + columns, dock reserve + rows)
    /// **without the scroll bar's always-up reserve**, on purpose: with it,
    /// a mouse plugged in would make saved and current split layouts not fit
    /// and reset the user's proportions, and the dividers at the limit would
    /// freeze. A pane at its narrowest gives the track's columns up from its
    /// text instead — splits too, in that form. The
    /// measure is this pane's cell and dock reserve: the point-size delta is
    /// per pane. The split's gate ([`TerminalPane::grid_fits`]) and the
    /// resizing's limit (`SplitView::resize`) come from here. `None` if not
    /// attached to a window.
    ///
    /// Before [`TerminalPane::start`] the dock reserve is the birth package's
    /// (the one `start` will set): session restore checks the saved tree
    /// against this limit before any shell starts.
    ///
    /// **Without the top edge's reserve** too ([`bt_gpu::edge_reserve_px`]),
    /// for the same reason as the scroll bar's: a larger limit would not fit
    /// split layouts saved before it and would reset them. Where the content
    /// fades at the top, the smallest pane can therefore have a row less than
    /// [`MIN_PANE_ROWS`]. **And without the grid's right side and foot**
    /// ([`CellMetrics::grid_inset`]), the same reason again: the smallest pane
    /// can have a column or two and a row less than the two constants.
    pub fn min_size(&self) -> Option<NSSize> {
        let scale = self.window()?.backingScaleFactor();
        let cell = self.cell_metrics(scale);
        let (cell_w, cell_h) = cell.cell_px();
        let dock_rows = self.reserved_dock_rows();
        let width =
            f64::from(cell.grid_inset().left) + f64::from(MIN_PANE_COLS) * f64::from(cell_w);
        let height = f64::from(bt_gpu::dock_px(dock_rows, cell))
            + f64::from(MIN_PANE_ROWS) * f64::from(cell_h);
        Some(NSSize::new(width / scale, height / scale))
    }

    /// A cell's size, in points — the step of keyboard resizing
    /// (`TerminalTab::resize_split`). `None` if not attached to a window.
    pub(crate) fn cell_size(&self) -> Option<NSSize> {
        let scale = self.window()?.backingScaleFactor();
        let (cell_w, cell_h) = self.cell_metrics(scale).cell_px();
        Some(NSSize::new(
            f64::from(cell_w) / scale,
            f64::from(cell_h) / scale,
        ))
    }

    /// Whether a pane of `size` (points) has a grid that passes the smallest
    /// pane limit — the split's gate. The new split inherits
    /// the cell and the dock reserve from this pane ([`TerminalPane::min_size`]).
    /// `false` if not attached to a window.
    pub(crate) fn grid_fits(&self, size: NSSize) -> bool {
        self.min_size()
            .is_some_and(|min| size.width >= min.width && size.height >= min.height)
    }

    /// The user is closing this pane: the terminal a server keeps for it ends
    /// with it ([`PaneHost::end_kept_terminal`]). Called by the paths that close
    /// a pane because the user asked to — and not by [`Self::begin_close`], which
    /// a quit uses too, where the terminals' fate is `keep_running`'s.
    ///
    /// A pane the user chose to leave running ([`TerminalPane::leave_running`]) lets its
    /// terminal run on instead: this close is the one the choice was for, and spends it.
    pub(crate) fn end_kept_terminal(&self) {
        if self.ivars().leaving.take() {
            self.host().leave_kept_terminal(self.uuid());
        } else {
            self.host().end_kept_terminal(self.uuid());
        }
    }

    /// The user answered *Leave Running* to the question that closes this pane: the close
    /// that follows lets the terminal a server keeps for it run on. Only a pane that shows
    /// such a terminal takes the choice — a local shell closes as it does under *Close*.
    pub(crate) fn leave_running(&self) {
        self.ivars().leaving.set(self.shows_kept_terminal());
    }

    /// The job running in the foreground outside the shell.
    /// Idle if there is no session or the reader thread has finished: the
    /// shell is gone and `child_pid` may be stale, a stale pid is not asked.
    pub fn foreground(&self) -> Foreground {
        // A pane that shows a terminal a server keeps: the job in front is the
        // server's, which the process table here cannot see (it shows the pipe).
        if let Some(kept) = self.host().kept_terminal(self.uuid()) {
            return kept_foreground(&kept);
        }
        let (Some(session), Some(&parent)) =
            (self.ivars().session.get(), self.ivars().shell_parent.get())
        else {
            return Foreground::Idle;
        };
        if !session.reader_alive() {
            return Foreground::Idle;
        }
        jobs::foreground(parent, session.child_pid(), &SystemTable)
    }

    /// The remote-session probe: takes the running command's
    /// generation, probes the foreground group and reports to the session if
    /// it found ssh/mosh. The return is two bits ([`RemoteProbeOutcome`]):
    /// **whether undecided** — if so the arm stays set and the next output
    /// probes again ([`RemoteProbe`]) — and whether the remote state changed
    /// (so the window title refreshes).
    ///
    /// The generation **before** the probe: `Session::set_remote` rejects the
    /// answer of a command that ended in between. No probe if the reader has
    /// finished ([`Self::foreground`]'s rule: a stale pid is not asked).
    pub(crate) fn probe_remote(&self) -> RemoteProbeOutcome {
        let settled = RemoteProbeOutcome::default();
        let (Some(session), Some(&parent)) =
            (self.ivars().session.get(), self.ivars().shell_parent.get())
        else {
            return settled;
        };
        if !session.reader_alive() {
            return settled;
        }
        let Some(command) = session.running_command() else {
            return settled;
        };
        match jobs::remote(parent, session.child_pid(), &SystemTable) {
            Probe::Undecided => RemoteProbeOutcome {
                undecided: true,
                changed: false,
            },
            Probe::Local => settled,
            Probe::Remote(target) => {
                // A call bateri wrapped carries its nonce: kept for
                // the bootstrap's `up`, which may already be here.
                if let Some(nonce) = &target.nonce {
                    self.ivars().wrap_proof.borrow_mut().wrapped =
                        Some((command, nonce.clone(), target.argv.clone()));
                }
                let wrapped = target.nonce.is_some();
                // The line is per argument, with readable quoting;
                // `bt-core` does not write the rule a second time, it stores the string.
                let line = quote::command_line(&target.argv);
                let target = RemoteTarget {
                    host: target.host,
                    port: target.port,
                    kind: target.kind,
                    argv: target.argv,
                    line,
                };
                let changed = session.set_remote(command, Some(&target));
                if wrapped {
                    self.check_remote_up();
                }
                RemoteProbeOutcome {
                    undecided: false,
                    changed,
                }
            }
        }
    }

    /// Which program the pane runs now, as its tab's icon draws it: what the program said of
    /// itself (`app=` of `OSC 7501`), else — in a pane that shows a terminal a server keeps —
    /// what the server says is in front of it ([`Self::server_kind`]), else what the process
    /// table named, else what the bateri the pane was carried over from showed for the same
    /// command, else what the command's line said — the shell when no command runs, the server
    /// in a remote session none of them knows ([`identity::decide`]). Read live from the
    /// sources, so a program that speaks up later is heard on the tab's next refresh; the
    /// readings that cost a screen walk or a syscall are the probe's ([`Self::probe_identity`])
    /// and kept. Remembers what it saw while a command runs, for [`Self::ended_kind`]. A leaf
    /// lock or two; no `Term`.
    pub(crate) fn program_kind(&self) -> Kind {
        self.decided(self.server_kind(), true)
    }

    /// What the pane runs **here**: [`Self::program_kind`] without the server's word. What a
    /// program on a server cannot do is what this Mac has — the clipboard ⌘V pastes an image
    /// from, a program that is handed to the next bateri.
    pub(crate) fn local_program_kind(&self) -> Kind {
        // What the pane ran is [`Self::program_kind`]'s to remember: where the server has a
        // word the local reading is another kind than the pane's, and two writers would have
        // [`Self::ended_kind`] say one or the other by who looked last.
        self.decided(None, self.server_kind().is_none())
    }

    /// The icon of the program in front of the terminal a server keeps for this pane, by the
    /// name the server gave it ([`Shown::kind`]). `None` in a pane that is not a server pane
    /// (a plain `ssh` has no such word, and a pane that is not in a remote session does not
    /// listen to one: the gate is [`Self::server_target`]'s), while the server has not said,
    /// and for a name nothing here knows — the pane keeps the server's icon.
    fn server_kind(&self) -> Option<Kind> {
        self.server_target()?;
        self.host().kept_terminal(self.uuid())?.kind()
    }

    /// The program in front of the terminal the pane shows is another one as far as the icon
    /// tells (`Backend::program_changed`): the tab reads the pane's kind again. The server's
    /// word is the news, so there is no probe to run.
    pub(crate) fn server_program_changed(&self) {
        self.host().program_changed(self.ivars().id);
    }

    /// The decision over the sources with `server` as the server's word; `remember` writes it to
    /// the memo of what ran ([`Self::ended_kind`]).
    fn decided(&self, server: Option<Kind>, remember: bool) -> Kind {
        let Some(session) = self.session() else {
            return Kind::Shell;
        };
        let Some(running) = session.running_command() else {
            return Kind::Shell;
        };
        let memo = self.ivars().program_memo.get();
        let app = session
            .program_app()
            .as_deref()
            .and_then(identity::from_app);
        let command = session
            .last_command()
            .filter(|command| command.phase == CommandPhase::Running)
            .map(|command| command.id);
        let line = command
            .and_then(|id| memo.line.filter(|(seen, _)| *seen == id))
            .and_then(|(_, kind)| kind);
        // The old process's whole decision, line included: under what is live, over the line.
        let carried = carried_kind(memo.carried, command);
        let group = memo
            .group
            .filter(|(generation, _)| *generation == running)
            .map(|(_, kind)| kind);
        let remote = session.remote_line().is_some();
        let kind = identity::decide(app, server, group, carried.or(line), remote);
        if remember
            && let Some(id) = command
            && memo.ran != Some((id, kind))
        {
            self.ivars().program_memo.set(ProgramMemo {
                ran: Some((id, kind)),
                ..memo
            });
        }
        kind
    }

    /// The program of the command that just ended, for the tab to keep showing: by the time the
    /// end is noticed the pane is at its prompt and [`Self::program_kind`] says shell. What
    /// [`Self::program_kind`] last saw while a command ran, if that was during the newest command
    /// (the one that ended); the shell for one that came and went before anyone looked.
    pub(crate) fn ended_kind(&self) -> Kind {
        let Some(session) = self.session() else {
            return Kind::Shell;
        };
        let newest = session.last_command().map(|command| command.id);
        kind_of_end(self.ivars().program_memo.get().ran, newest)
    }

    /// What a handover carries of [`Self::program_kind`] to the next bateri: a program, not the
    /// shell or the server — those it tells by itself, and a pane carrying none is written in
    /// the format a rolled-back bateri still reads (`handover::PaneState`).
    fn carried_program(&self) -> Option<Kind> {
        Some(self.local_program_kind()).filter(|kind| !matches!(kind, Kind::Shell | Kind::Remote))
    }

    /// The identity probe: reads what [`Self::program_kind`] cannot cheaply — the newest command's
    /// line, off the screen, once per command, and the foreground group's processes — and keeps
    /// it. `undecided` while nothing names the program (the shell itself still holds the
    /// terminal, the command has not executed, the group is a helper nobody knows) and the line
    /// has not either: the next burst of output looks again; `changed` when the kind is another.
    /// A remote session has no table to read (the process table shows only the local `ssh`) and
    /// is settled by the line alone. Main thread, syscalls only.
    pub(crate) fn probe_identity(&self) -> RemoteProbeOutcome {
        let settled = RemoteProbeOutcome::default();
        let (Some(session), Some(&parent)) =
            (self.ivars().session.get(), self.ivars().shell_parent.get())
        else {
            return settled;
        };
        if !session.reader_alive() {
            return settled;
        }
        let Some(running) = session.running_command() else {
            return settled;
        };
        let before = self.program_kind();
        let mut memo = self.ivars().program_memo.get();
        if let Some(command) = session
            .last_command()
            .filter(|command| command.phase == CommandPhase::Running)
            && memo.line.map(|(id, _)| id) != Some(command.id)
        {
            let kind = session
                .command_line(command.id)
                .as_deref()
                .and_then(identity::from_line);
            memo.line = Some((command.id, kind));
        }
        let mut undecided = false;
        if session.remote_line().is_none()
            && memo.group.map(|(generation, _)| generation) != Some(running)
        {
            let newest = session.last_command().map(|command| command.id);
            let named = memo
                .line
                .and_then(|(_, kind)| kind)
                .or(carried_kind(memo.carried, newest));
            let (group, wait) = after_table(
                identity::find(parent, session.child_pid(), &SystemTable),
                named,
            );
            memo.group = group.map(|kind| (running, kind)).or(memo.group);
            let looks = memo
                .looks
                .filter(|(generation, _)| *generation == running)
                .map_or(0, |(_, looks)| looks)
                .saturating_add(1);
            memo.looks = Some((running, looks));
            undecided = keep_looking(wait, looks);
        }
        self.ivars().program_memo.set(memo);
        RemoteProbeOutcome {
            undecided,
            changed: self.program_kind() != before,
        }
    }

    /// The program probe: whether the running command's program reads the
    /// keyboard itself — `true` while there is no answer yet (the arm stays
    /// set, the next output edge looks again). Asked only while a job outside
    /// the shell's own group holds the terminal
    /// ([`jobs::job_in_foreground`]): the shell's own raw moments (ZLE, a
    /// builtin `read -k`, `exec fish`) are not a program. Raw modes mark the
    /// generation ([`bt_core::Session::note_raw`]) and settle the arm; no
    /// command, or a reader that has finished, settles it too — the next `C`
    /// arms again. Main thread, syscalls only: the group, `tcgetattr` and
    /// the group's process table ([`program::find`]).
    ///
    /// **Not on sudo's modes** ([`program::Found`]): while sudo holds the
    /// terminal its raw modes are its own — its password prompt with
    /// `pwfeedback`, its relay of the terminal it opened for the command
    /// with `use_pty` — and marking would keep the band down for a whole
    /// `sudo make install`. The command it runs answers instead: none yet is
    /// no answer (the prompt — at most an ask per keystroke), one no bar
    /// knows settles the arm **unmarked**, so the build's output asks
    /// nothing more.
    ///
    /// **Not on the alternate screen**: a full-screen program (vim from `git
    /// rebase -i`, `less`) is raw too, but the dock is already lifted for it,
    /// and marking would keep the band hidden for the rest of the command
    /// after it exits — over a build's output. No answer there; the arm
    /// stays and the next output after the program looks again.
    ///
    /// **Cheapest first**: an output-heavy command that never goes raw (a
    /// build, `tail -f`) is asked once per delay while it prints, so the
    /// atomic read and the one `tcgetattr` come before the process table.
    ///
    /// **The program is known before the mark** ([`Self::show_program`]): the
    /// arm drops with the mark, so a bar is written once per command.
    pub(crate) fn probe_program(&self) -> bool {
        let (Some(session), Some(&parent)) =
            (self.ivars().session.get(), self.ivars().shell_parent.get())
        else {
            return false;
        };
        if !session.reader_alive() || session.running_command().is_none() {
            return false;
        }
        if session.alt_screen()
            || !session
                .with_pty_fd(jobs::tty_modes)
                .is_some_and(TtyModes::raw)
            || !jobs::job_in_foreground(parent, session.child_pid(), &SystemTable)
        {
            return true;
        }
        let home = crate::child::home();
        let program =
            match program::find(parent, session.child_pid(), &SystemTable, home.as_deref()) {
                program::Found::Waiting => return true,
                program::Found::Elevated => return false,
                program::Found::Unknown => None,
                program::Found::Program(program) => Some(*program),
            };
        let Some(command) = session.note_raw(jobs::tty_modes) else {
            return true;
        };
        if let Some(program) = program {
            self.show_program(session, command, program);
        }
        false
    }

    /// The marked program's guide bar ([`program::find`]): written at once
    /// with what the process table says (main thread, system calls only —
    /// the group's members and their exec records in the probe, the
    /// program's start time here), then by a background job on a thread of
    /// its own: completed with the interpreter's `--version` (killed past
    /// its timeout), a
    /// venv's `pyvenv.cfg` and a kubectl session's context from its
    /// kubeconfig ([`program::details`]), and taken away when the
    /// program exits ([`program::wait_for_exit`]) — the command can go on
    /// without it (`python3; make`). Each answer comes back on the main
    /// queue, finds the pane by its id and is written for the **same
    /// generation**: a command that ended in between rejects it
    /// ([`bt_core::Session::set_program`]). Only a recognized program gets
    /// here; an unrecognized one writes nothing — no band. A timed run never
    /// gets here: its probe is never armed ([`ShellWake::command_started`]'s
    /// gate).
    fn show_program(&self, session: &Session, command: u64, found: program::Program) {
        session.set_program(command, Some(&found.bar(None)));
        let start = jobs::start_time(found.pid);
        let (id, lookup) = (self.ivars().id, self.ivars().lookup);
        let post = move |bar: Option<ProgramBar>| {
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(pane) = lookup(mtm, id)
                    && let Some(session) = pane.session()
                {
                    session.set_program(command, bar.as_ref());
                }
            });
        };
        // No thread, no details: the bar stays with what was known.
        let _ = std::thread::Builder::new()
            .name("program details".into())
            .spawn(move || {
                let details = program::details(&found);
                post(Some(found.bar(Some(&details))));
                program::wait_for_exit(found.pid, start);
                post(None);
            });
    }

    /// The bootstrap's proof. **First**, whatever the probe says,
    /// the `up`'s nonce is marked seen ([`bt_shell_common::ssh_wrap::mark_up`],
    /// a file created on a thread of its own, no `ssh -G`): the local `ssh`
    /// function's fallback asks for its own nonce before anything else, so a
    /// session that ends at once or a slow `ssh -G` cannot have a server with
    /// a shell branded `plain`. A forged `up`
    /// (a remote program printing one) can only name a nonce it cannot know —
    /// at worst a fallback that does not happen, the safe direction.
    ///
    /// Then, when the session's last `up`
    /// ([`bt_core::Session::remote_up`]) and the wrapped `ssh` the probe found
    /// ([`WrapProof`]) are of the same command generation and carry the same
    /// nonce, the server is recorded as `posix` — once per generation, on a
    /// thread of its own: `ssh -G` (the server's key, `ssh_wrap::record_posix`) and
    /// the state file's lock must not hold the main thread. Called from both
    /// ends ([`Wake::remote_up`], [`Self::probe_remote`]); a mismatch, a call
    /// bateri did not wrap or a timed run records nothing.
    pub(crate) fn check_remote_up(&self) {
        if self.ivars().run.is_some() {
            return;
        }
        let Some((generation, seen)) = self.session().and_then(|session| session.remote_up())
        else {
            return;
        };
        self.mark_up(generation, &seen);
        let argv = {
            let mut proof = self.ivars().wrap_proof.borrow_mut();
            let Some((command, nonce, argv)) = proof.wrapped.as_ref() else {
                return;
            };
            if *command != generation || *nonce != seen || proof.recorded == Some(generation) {
                return;
            }
            let argv = argv.clone();
            proof.recorded = Some(generation);
            argv
        };
        // A write that fails (the state file's lock past its patience, a
        // full disk) is tried again a few times on the thread: nothing else
        // asks again for this generation, and a missing `posix` row is the
        // wrong direction for the plain-ssh fallback.
        let spawned = std::thread::Builder::new()
            .name("remote posix".into())
            .spawn(move || {
                let Some(home) = crate::child::home() else {
                    return;
                };
                let path = crate::remote_hosts_path(&home);
                for _ in 0..POSIX_ATTEMPTS {
                    if bt_shell_common::ssh_wrap::record_posix(
                        &crate::ssh_route::SystemSsh,
                        &argv,
                        &path,
                    )
                    .is_ok()
                    {
                        return;
                    }
                }
            });
        // No thread, no write: the next `up` or probe of this generation may try.
        if spawned.is_err() {
            self.ivars().wrap_proof.borrow_mut().recorded = None;
        }
    }

    /// The user typed into the remote session after its login
    /// ([`Wake::remote_typed`]): when the session is the wrapped `ssh` the probe
    /// found ([`WrapProof`], same generation), its attempt is marked used
    /// ([`bt_shell_common::ssh_wrap::mark_used`], a file on a thread of its
    /// own) — the local `ssh` function's fallback then reruns nothing and
    /// brands nothing `plain`: a session the user worked in (a `ForceCommand`
    /// CLI that ignored our command) was theirs, and its `exit` must not
    /// connect them again. Once per generation; a timed run marks nothing.
    pub(crate) fn check_remote_typed(&self) {
        if self.ivars().run.is_some() {
            return;
        }
        let Some(generation) = self.session().and_then(|session| session.remote_typed()) else {
            return;
        };
        let nonce = {
            let mut proof = self.ivars().wrap_proof.borrow_mut();
            let Some((command, nonce, _)) = proof.wrapped.as_ref() else {
                return;
            };
            if *command != generation || proof.used == Some(generation) {
                return;
            }
            let nonce = nonce.clone();
            proof.used = Some(generation);
            nonce
        };
        let spawned = std::thread::Builder::new()
            .name("remote used".into())
            .spawn(move || {
                if let Some(home) = crate::child::home() {
                    let path = crate::remote_hosts_path(&home);
                    for _ in 0..POSIX_ATTEMPTS {
                        if bt_shell_common::ssh_wrap::mark_used(&path, &nonce).is_ok() {
                            return;
                        }
                    }
                }
            });
        if spawned.is_err() {
            self.ivars().wrap_proof.borrow_mut().used = None;
        }
    }

    /// [`Self::check_remote_up`]'s first half: marks `seen` once per arrival.
    fn mark_up(&self, generation: u64, seen: &str) {
        let arrival = Some((generation, seen.to_owned()));
        {
            let mut proof = self.ivars().wrap_proof.borrow_mut();
            if proof.marked == arrival {
                return;
            }
            proof.marked.clone_from(&arrival);
        }
        let nonce = seen.to_owned();
        let spawned = std::thread::Builder::new()
            .name("remote up".into())
            .spawn(move || {
                if let Some(home) = crate::child::home() {
                    let path = crate::remote_hosts_path(&home);
                    for _ in 0..POSIX_ATTEMPTS {
                        if bt_shell_common::ssh_wrap::mark_up(&path, &nonce).is_ok() {
                            return;
                        }
                    }
                }
            });
        if spawned.is_err() {
            self.ivars().wrap_proof.borrow_mut().marked = None;
        }
    }

    /// The pane leaves the screen (its tab is not the one shown): focus off,
    /// and what floats over it or follows the pointer goes, since nothing
    /// would close it while hidden — the popovers, the ⌘-hovered link, the
    /// scroll bar's hover and the upload buttons' hover. The tab's whole
    /// leaving ([`crate::tab::TerminalTab::leave_screen`]) and a pane that
    /// moves into a tab that is not shown both come here.
    pub fn leave_screen(&self) {
        self.apply_focus(false);
        self.close_server_card();
        self.close_upload_list();
        self.unhover_footer();
        self.view().clear_link();
        self.view().release_scrollbar_hover();
    }

    /// The focus changed — forwards it to `bt-gpu` and to the program in the
    /// terminal ([`Self::report_focus`]).
    ///
    /// **Never called in a hermetic run** and the gate is here, not in
    /// `bt-gpu`'s default: `DisplayLink`'s `focused` is born `true` anyway but
    /// that alone is not enough — a Spotlight opening during `make smoke`
    /// produces `windowDidResignKey:`, which asks for a frame, and the gate
    /// would be green on one machine and red on another. The precedent is
    /// `app::resolve_reduce_motion` looking at `Inputs`.
    ///
    /// It returns silently if there is no link: the key event can also fire
    /// before `start_session` and in that state the default (`true`) is
    /// already right.
    pub(crate) fn apply_focus(&self, focused: bool) {
        // The gate looks at the `run` **flag**, not `inputs()`: `inputs()`
        // resolves `child::home()` as an argument (may go as far as the passwd
        // record) and focus changes on every application switch. The flag's
        // copy is therefore in the pane itself: no need to reach the
        // application delegate either.
        if self.ivars().run.is_some() {
            return;
        }
        if focused {
            self.keep_keyboard_in_view();
        }
        if let Some(link) = self.ivars().link.get() {
            link.set_focused(focused);
        }
        self.report_focus(focused, self.keyboard_here());
    }

    /// The keyboard is in the terminal that a program's view covers: it goes to the view. The
    /// terminal under a view is nobody's to type in, and something other than [`Self::focus`]
    /// left it there — the window gave it to the pane's terminal before its view started (a
    /// restored window's first responder, a tab shown again) — which is what a view started again
    /// by a handover found: a pane that is the user's and has its keyboard in the covered
    /// terminal has it in the view from the moment it is the user's.
    fn keep_keyboard_in_view(&self) {
        if self.shows_view() && self.view().is_first_responder() {
            self.focus();
        }
    }

    /// The pane's share of the closing sequence — **starts, does not wait**.
    /// Its callers are the tab's `begin_close` — the window's closing
    /// (`windowWillClose:`, the handle drops) and the application's closing
    /// (`AppDelegate::shutdown`, all handles waited on until a single
    /// deadline) — and a single pane's closing (`TerminalTab::close_pane`,
    /// the handle drops; if the split could not be born, `add_pane`'s rollback).
    ///
    /// The order is required: first the bound holder lets its copy of the
    /// master go ([`Keeper::release`]), then the upload queue is released
    /// (processes are killed and the half file is deleted; there is no dock
    /// left to show the result), **then** the rhythm, the `Waker` and
    /// `SIGHUP` — in the reverse order the cancellation would go after the
    /// shell's `SIGHUP`.
    ///
    /// 0. Count the pane as closed ([`PaneIvars::closed`]) and remove the frame
    ///    observer: while its tab or window closes AppKit can re-lay-out the content
    ///    and if the observer stayed the dying session would receive a resize
    ///    (and a `Msg::Resize` that cannot be written to a dropped reader).
    /// 1. Cut the rhythm (`DisplayLink::stop`): the link stops, leaves the
    ///    run loop and the wake gate closes. No new frame is asked after this.
    /// 2. **Detach** the `Waker` from `ShellWake` and drop it here, on the
    ///    main thread ([`ShellWake::detach`]): `ShellWake`'s last copy can
    ///    drop on the reader or the `"PTY teardown"` thread and must not carry a `Waker` there.
    /// 3. Start the session's closing (`Session::begin_shutdown`: `SIGHUP` +
    ///    the reader thread's finish in the background).
    ///
    /// `DisplayLink` is now **droppable** and drops on the main thread with
    /// the pane object: after detaching, the last `Waker` copy is either in
    /// this object or in Metal's completion block; the second throws a
    /// synchronous job at the main queue but the main thread is not waiting
    /// at that time — the window's closing does not wait, and ⌘Q keeps the
    /// windows in the list until the wait ends.
    ///
    /// Idempotent: the second call returns [`Closing::AlreadyDone`] (`stop` is
    /// latched, `detach` is `take`, `begin_shutdown` is an `Option`, removing
    /// the observer is a no-op if unregistered). `None` if the session was
    /// never born — there is nothing to close.
    /// The pane closes: its shell is told to end and nothing waits for it —
    /// what a host does with a pane it no longer shows.
    pub fn close(&self) {
        drop(self.begin_close());
    }

    pub(crate) fn begin_close(&self) -> Option<Closing> {
        // The holder lets its copy go **first**: a program with unread
        // output cannot finish exiting while an unread copy of its master is
        // open, and the hang-up below waits for that exit.
        if let Some(keeper) = self.ivars().keeper.as_deref() {
            keeper.release(&self.ivars().uuid);
        }
        self.quiesce(true);
        let session = self.ivars().session.get()?;
        Some(match session.begin_shutdown() {
            Some(handle) => Closing::Started(handle),
            None => Closing::AlreadyDone,
        })
    }

    /// Freezes the pane for the update's handover and returns
    /// what the holder carries: the master, the child's pid and start time,
    /// the pane's state ([`PaneState`](crate::handover::PaneState): the
    /// VT, `bt-core`'s blob, the unsent input, the scrollback for a
    /// fallback, the program it shows) and the tail as the buffer's head.
    ///
    /// The second half of the return is the pane's session-restore history
    /// (`Session::frozen_history`, also inside the state) for the caller's
    /// save at this quit.
    ///
    /// The start time is read **before** the freeze, and so is the pane's
    /// own quiet-down ([`Self::quiesce`]: no frame may reach the `Term` the
    /// freeze probed destructively) — but not the remote session's end: the
    /// ssh master is not closed by a handover. `None` if there is
    /// nothing to carry (no session, already closed, the child's start time
    /// unreadable) or the freeze failed — the caller closes the pane today's
    /// way, which a failed freeze leaves intact.
    pub(crate) fn freeze_for_handover(&self) -> Option<(crate::handover::HeldPane, Vec<u8>)> {
        if self.is_closed() {
            return None;
        }
        let session = Arc::clone(self.session()?);
        let parent = *self.ivars().shell_parent.get()?;
        let pid = session.child_pid();
        let start = jobs::start_time(pid)?;
        let program = self.carried_program();
        // Before the quiet-down: what the pane shows is what the next process starts again.
        let view = self.shows_view();
        self.quiesce(false);
        let frozen = match session.freeze() {
            Ok(frozen) => frozen,
            Err(error) => {
                eprintln!("bateri: could not freeze a pane for the update: {error}");
                return None;
            }
        };
        // From the frozen VT: the freeze's snapshot drove the live `Term`
        // destructively (and a live read before it would have destroyed the
        // alternate screen the snapshot must carry).
        let history = session.frozen_history(&frozen);
        let state = crate::handover::PaneState {
            cols: frozen.cols,
            rows: frozen.rows,
            parent,
            vt: frozen.vt,
            core: frozen.blob,
            input: frozen.input,
            history: history.clone(),
            program,
            view,
        };
        let held = crate::handover::HeldPane::new(
            self.ivars().uuid.clone(),
            pid,
            start,
            state.encode(),
            frozen.tail,
            frozen.master,
        );
        Some((held, history))
    }

    /// The pane's side of closing, the session aside — shared by
    /// [`Self::begin_close`] and [`Self::freeze_for_handover`]; `end_remote`
    /// tells the ssh registry this pane's remote session ended (not in a
    /// handover: the master lives on). Idempotent.
    fn quiesce(&self, end_remote: bool) {
        // First: a job's thread waiting at the password sheet holds the helper's
        // worker too — dropping the sender answers it, then `close` is served.
        self.close_password();
        // The questions a background tab parked go the way an open sheet
        // goes: answered `Cancel`, each asker clearing its own gate — while
        // the pane can still be found by its blocks.
        crate::sheets::answer_parked(self);
        self.abandon_uploads();
        // Finder's pending promises fail now (cancelled), not with the last reference.
        self.finder_abandon();
        // The helper's ssh goes now, not when the last reference drops.
        self.remote_helper().borrow_mut().close();
        // A remote session closing with the pane is one less session to the
        // host: our master ends with the last one.
        if end_remote
            && self.ivars().ssh_session.take().is_some()
            && let Some(masters) = self.ivars().masters.as_deref()
        {
            masters.session_ended(self.id());
        }
        // The server card and its Esc monitor go with the pane (the upload
        // list's goes in `abandon_uploads`).
        self.close_server_card();
        self.stop_stats();
        // A forward tunnel of ours goes with the pane.
        self.end_remote_ports();
        // A program's view goes with the pane (no teardown to ask: its program
        // is hung up on next, or carried over, and meets a pane with no view).
        self.drop_view();
        self.ivars().closed.set(true);
        // SAFETY: the observer is this object, registered in `observe_frame`;
        // a no-op if it is not registered.
        unsafe { NSNotificationCenter::defaultCenter().removeObserver(self) };
        if let Some(link) = self.ivars().link.get() {
            link.stop();
        }
        drop(self.ivars().wake.detach());
    }

    /// Pane geometry or font moved: match the layer, update the grid, ask for
    /// a frame. A font change that lands on the same grid is redrawn too:
    /// `DisplayLink::resize` asks for the frame unconditionally and asking for
    /// a frame also sets the damage flag.
    ///
    /// The mouse inputs are refreshed here too: the view sits in
    /// `PaneIvars.view` as a `Retained<BateriView>` and goes with the pane
    /// object. If the pane is not attached to a window there is no scale and
    /// nothing is done ([`TerminalPane::sync_geometry`]).
    pub(crate) fn refresh_geometry(&self) {
        // The view's container is the pane, not the grid: told on its own.
        self.sync_view_size();
        let Some(grid) = self.sync_geometry() else {
            return;
        };
        self.ivars()
            .view
            .set_metrics(grid, self.ivars().dock_rows.get());
        if let Some(link) = self.ivars().link.get() {
            link.resize(
                grid.cols,
                grid.rows,
                grid.cell,
                self.ivars().dock_rows.get(),
                grid.dock_cols,
                grid.context,
            );
        }
        // "Jump to latest" stands on the reserves that just moved.
        self.refresh_jump();
        // So does the veil over a grid that waits for its connection.
        self.place_link_veil();
    }

    /// Matches the layer's drawable size to the view's backing geometry **and**
    /// returns the grid size — the name says both because the caller needs
    /// both and deriving the size without writing the dimensions would give a
    /// wrong result. The scale is read from a single source and the pixel size
    /// is multiplied from it; if `drawableSize` and `contentsScale` diverge
    /// there is blur.
    ///
    /// **The scale's two gates** (`Surface::set_size`, `Renderer::cell_metrics`;
    /// a long-standing debt) are not merged: this function is the only caller of the
    /// two, the scale is read once here and goes to both from the same local;
    /// the font setting does not touch the scale.
    ///
    /// The font slot is written here too, at the end: the only path that
    /// (re)builds the atlas is `cell_metrics` and the font notice is current
    /// only after it. Even if a screen change rebuilds the atlas the family is
    /// the same, the slot does not move.
    ///
    /// The scale is from the pane's **window** (`NSView::window`); `None` if the
    /// pane is not attached to a window — an invented scale would build the atlas wrong.
    fn sync_geometry(&self) -> Option<Grid> {
        let scale = self.window()?.backingScaleFactor();
        // The terminal view, not the pane: the two are the same size today but
        // the surface drawn is this view's layer and the size must be its.
        let view = &self.ivars().view;
        let bounds = view.bounds().size;
        let (width_px, height_px) = (bounds.width * scale, bounds.height * scale);
        // The scale is the layer owner's; the pixel size is the surface's
        // configuration.
        self.ivars().layer.setContentsScale(scale);
        self.ivars().surface.set_size(width_px, height_px);

        // The cell size comes from `bt-atlas`'s font metrics through `bt-gpu`
        // and the multiplication by the scale is there too. There is **no**
        // second rounding rule here: the old `.round()` block was deleted on
        // purpose. Had two rules sat side by side, which one wins would depend
        // on call order and the symptom, a one-pixel cell shift, would be silent.
        let renderer = &self.ivars().renderer;
        let cell = self.cell_metrics(scale);
        self.host().post_notices(
            self.ivars().id,
            Source::Font,
            font_messages(renderer.font_notice()),
        );
        // The scroll bar's reserve: a function of its form alone
        // ([`ScrollbarMode::reserve_px`]), so neither the alternate screen
        // nor the history moves the grid.
        let reserve = self.ivars().scrollbar.get().reserve_px(cell);
        // The top edge's reserve, the same kind of function: of the mode and
        // the cell alone. The mode is the one the link draws with
        // ([`TerminalPane::set_content_edge`] gives both), so the rows cut
        // here and the fade drawn over them agree.
        let top = bt_gpu::edge_reserve_px(self.ivars().content_edge.get(), cell);
        Some(split_into_grid(
            width_px,
            height_px,
            cell,
            self.ivars().dock_rows.get(),
            reserve,
            top,
        ))
    }
}

/// A program's view in the pane: the web view over the terminal, and what the pane
/// does while it is there.
///
/// The conversation with the program is in the session (`bt_core::pencere`, read
/// from the stream); what it asks of the window arrives as news
/// ([`ShellWake::view_changed`]) and is taken here on the main queue. While a view
/// is shown the terminal's frames stop, as in a background tab — the session and the
/// renderer stay, and what the screen holds is what comes back (§6.5). The
/// keyboard goes to the view if the terminal had it, and comes back to the terminal
/// if the view had it.
impl TerminalPane {
    /// What the session is opened with to host views: the application's name and
    /// version, the process's shelf of bundle files, the font family and the
    /// person's language and time zone, the pane's area in the unit a view is told it in
    /// (points: `0 × 0` where the pane has no layout yet), and how the shell is found
    /// from the child (`shell_parent`, recorded where the command was chosen: `login(1)`
    /// forks the shell on macOS, and the terminal's foreground group is the shell's, not
    /// the child's). `None` in a timed run, whose program is the gate's own script.
    fn view_options(&self, shell_parent: ShellParent) -> Option<ViewOptions> {
        let ivars = self.ivars();
        if ivars.run.is_some() {
            return None;
        }
        let mut options = ViewOptions::new(
            ivars.identity.app_name.clone(),
            bt_core::TERM_PROGRAM_VERSION,
        );
        options.store = pencere_view::shelf();
        options.font_mono = font_stack(ivars.font.borrow().family.as_deref());
        options.locale = pencere_view::locale_tag();
        options.time_zone = pencere_view::time_zone_name();
        options.size = pencere_view::content_size(self.bounds());
        options.shell_group = Some(ShellGroup::new(move |child| {
            jobs::shell_group(shell_parent, child, &SystemTable)
        }));
        Some(options)
    }

    /// The program's view, if one is over the terminal: a second hold on it, so
    /// nothing of the slot stays borrowed while AppKit runs (it calls back into the
    /// pane for the keyboard and the focus).
    fn view_host(&self) -> Option<ViewHost> {
        self.ivars().pencere.borrow().clone()
    }

    /// Whether a program's view is over the terminal.
    pub(crate) fn shows_view(&self) -> bool {
        self.ivars().pencere.borrow().is_some()
    }

    /// Whether the keyboard is in the pane: in the terminal, or in the view that
    /// covers it.
    fn keyboard_here(&self) -> bool {
        match self.view_host() {
            Some(host) => host.holds_keyboard(),
            None => self.view().is_first_responder(),
        }
    }

    /// Takes what the program's view has for the pane and does it, in order; then
    /// sets the deadline the conversation's state calls for. Main queue.
    ///
    /// **A question for the person and a link to open are asked** ([`pencere_view::ask`]),
    /// each on the pane's seat in the sheet gate, and the answer goes back to the
    /// conversation: what the person allowed is what the view starts with, and a link
    /// opens only if they agree. The log of the view is dropped.
    pub(crate) fn drain_views(&self) {
        if self.is_closed() {
            return;
        }
        let Some(session) = self.session().cloned() else {
            return;
        };
        for action in session.take_view_actions() {
            match action {
                Action::StartView(start) => self.start_view(&session, &start),
                Action::ToView(message) => {
                    if let Some(host) = self.view_host() {
                        host.deliver(&message);
                    }
                }
                Action::DestroyView => self.end_view(),
                Action::Consent(consent) => {
                    let app = self.ivars().identity.app_name.clone();
                    let origin = self.view_origin();
                    pencere_view::ask::consent(self, &session, consent, &origin, &app);
                }
                Action::WithdrawConsent(id) => pencere_view::ask::withdraw(self, id),
                Action::OpenLink { link, url } => {
                    let origin = self.view_origin();
                    pencere_view::ask::link(self, &session, link, &url, &origin);
                }
                _ => {}
            }
        }
        self.wait_for_view(&session);
    }

    /// The questions of the program's view that are up.
    pub(crate) fn view_asking(&self) -> &pencere_view::ask::Asking {
        &self.ivars().view_asking
    }

    /// Who a program's view is asking for: where its program runs and what it is called
    /// ([`TerminalPane::view_whereabouts`]), and what it calls itself once its view is up. The
    /// tool the view is of is the question's own ([`bt_core::pencere::Consent::tool`]), not read
    /// here.
    fn view_origin(&self) -> bt_shell_common::view_ask::Origin {
        let (place, program) = self.view_whereabouts();
        bt_shell_common::view_ask::Origin {
            place,
            program,
            identity: self.view_identity(),
            said: self
                .view_host()
                .and_then(|host| host.program().map(str::to_owned)),
        }
    }

    /// Puts the view over the terminal, with the application's strip above it: over the
    /// terminal's small surfaces (the link label, the block tip, the jump button) and under
    /// the veils, so a pane that is not the keyboard's dims over its view and its strip as it
    /// does over its terminal.
    fn start_view(&self, session: &Arc<Session>, start: &bt_core::pencere::ViewStart) {
        // The crate has one view at a time; a stale one goes first.
        self.end_view();
        self.sync_view_size();
        let ivars = self.ivars();
        let theme = session.theme();
        let text = self.view_strip(start);
        let Some(host) = ViewHost::start(self.mtm(), session, start, self.bounds(), &theme, &text)
        else {
            session.close_view();
            return;
        };
        host.aim(self);
        self.addSubview_positioned_relativeTo(
            host.web(),
            NSWindowOrderingMode::Below,
            Some(&ivars.link_veil),
        );
        self.addSubview_positioned_relativeTo(
            host.strip(),
            NSWindowOrderingMode::Below,
            Some(&ivars.link_veil),
        );
        // The terminal's frames stop while the view covers it: nothing of them
        // could be seen, and a program that streams under a view would draw them
        // all the same.
        if let Some(link) = ivars.link.get() {
            link.set_visible(false);
        }
        let keyboard = ivars.view.is_first_responder();
        ivars.pencere.replace(Some(host));
        if keyboard {
            self.focus();
        }
    }

    /// Takes the view down: the keyboard goes back to the terminal if the view
    /// had it (a first responder that leaves the window leaves the window with no
    /// keyboard), and the terminal's frames come back — it is drawn again, with
    /// the screen the program left.
    fn end_view(&self) {
        let Some(host) = self.ivars().pencere.take() else {
            return;
        };
        self.view_gone();
        if host.holds_keyboard()
            && let Some(window) = self.window()
        {
            window.makeFirstResponder(Some(self.view()));
        }
        host.close();
        if !self.is_closed()
            && let Some(link) = self.ivars().link.get()
        {
            link.set_visible(self.ivars().seen.get());
        }
    }

    /// The program's view goes with the pane, whatever it was doing: no
    /// teardown is asked of the view and no frame comes back.
    fn drop_view(&self) {
        if let Some(host) = self.ivars().pencere.take() {
            self.view_gone();
            host.close();
        }
    }

    /// What was the view's goes with it: the requests it had out are not waited for, and the
    /// question it asked about a link is taken down — its answer would open a link for a view
    /// that is not there and shut the next view's out.
    fn view_gone(&self) {
        self.ivars().view_waits.borrow_mut().requests_over();
        pencere_view::ask::view_ended(self);
    }

    /// Sets the deadline the conversation's state calls for: one delayed block on
    /// the main queue, which asks for no frame ([`pencere_view`]'s `wait`).
    fn wait_for_view(&self, session: &Arc<Session>) {
        // While the person is asked about access the open waits for them, however
        // long: no deadline of ours runs, and one that was set is cancelled.
        let state = self
            .ivars()
            .view_asking
            .deadline_state(session.view_state());
        let plan = self.ivars().view_waits.borrow_mut().observe(state);
        if let Some((number, delay)) = plan {
            self.after_on_main(number, delay, Self::view_wait_ended);
        }
    }

    /// One delayed block on the main queue, for the wait `number`: it finds the pane again by its
    /// id (the pane may be gone) and calls `ended` with the number, which says whether it is
    /// still the wait that is set.
    fn after_on_main(&self, number: u64, delay: Duration, ended: fn(&TerminalPane, u64)) {
        let Ok(when) = DispatchTime::try_from(delay) else {
            return;
        };
        let (id, lookup) = (self.id(), self.ivars().lookup);
        let _ = DispatchQueue::main().after(when, move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                ended(&pane, number);
            }
        });
    }

    /// A deadline ran out: if it is still the one that is set, the conversation is
    /// told the time is up and what follows is taken at once.
    fn view_wait_ended(&self, number: u64) {
        let Some(session) = self.session().cloned() else {
            return;
        };
        if !self.ivars().view_waits.borrow_mut().expired(number) {
            return;
        }
        session.view_wait_over();
        self.drain_views();
    }

    /// The view's message made the pane write to the program ([`Session::view_message`]): a
    /// request is out, and the program has until the time of the wait to answer it. The first
    /// of a burst sets the wait; the others are in it.
    pub(crate) fn view_requests_out(&self) {
        let plan = self.ivars().view_waits.borrow_mut().requests_out();
        if let Some((number, delay)) = plan {
            self.after_on_main(number, delay, Self::view_requests_ended);
        }
    }

    /// The time for the program's answers ran out: if it is still the wait that is set, the
    /// requests that are out fail the way MCP defines, and the view hears it at once.
    fn view_requests_ended(&self, number: u64) {
        let Some(session) = self.session().cloned() else {
            return;
        };
        if !self
            .ivars()
            .view_waits
            .borrow_mut()
            .requests_expired(number)
        {
            return;
        }
        session.view_requests_overdue();
        self.drain_views();
    }

    /// The program is offered a restart (an update's handover; the connection to its terminal
    /// came back): what the screen restored, the messages a view lives on did not. The offer
    /// is a token the program was handed unasked, so it has a time to be taken up
    /// ([`pencere_view`]'s `wait`), and this is the one way to offer it — a call that does not
    /// set the time leaves the token valid for as long as the pane lives.
    fn restart_view_offered(&self, session: &Arc<Session>) {
        session.restart_view();
        self.ivars().view_waits.borrow_mut().restart_offered();
        self.wait_for_view(session);
    }

    /// Tells the conversation how big the view's container is: the pane's area below
    /// the strip, in CSS pixels, which are points. The same size is not told twice.
    fn sync_view_size(&self) {
        let Some(session) = self.session() else {
            return;
        };
        if !session.hosts_views() {
            return;
        }
        // The view's area is the pane's less the application's strip.
        let size = pencere_view::content_size(self.bounds());
        let size_pair = (size.width, size.height);
        if size.width == 0 || size.height == 0 || size_pair == self.ivars().view_size.get() {
            return;
        }
        self.ivars().view_size.set(size_pair);
        session.set_view_size(size);
    }

    /// What the strip over the view of `start` says: where the program runs
    /// ([`TerminalPane::view_whereabouts`]) and what it is called — what the program calls
    /// itself, else what the pane read —, with the view's title: what the program calls the
    /// tool, else the tool's label (`view_strip::strip`). Both words are the crate's, cleaned;
    /// the tool's own name (`ViewStart::tool`) is what the view calls it by and is not drawn.
    fn view_strip(&self, start: &bt_core::pencere::ViewStart) -> view_strip::Strip {
        let (place, program) = self.view_whereabouts();
        view_strip::strip(
            start.title.as_deref(),
            start.tool_label.as_deref(),
            place.as_deref(),
            view_strip::program_name(pencere_view::said_program(start), program.as_deref()),
        )
    }

    /// What tells the program in front of this pane from another that is run the same way, for
    /// the answers the person gives about it (`bt_shell_common::view_ask::Origin::identity`):
    /// the process table's name for it, and for `node` and `python` the script — never what the
    /// program says of itself. `None` where it is not known: a pane that shows a terminal a
    /// server keeps or a plain `ssh` (the process table there is `ssh`'s), and a program that
    /// is an interpreter running no script. An answer is then not kept.
    fn view_identity(&self) -> Option<String> {
        if self.host().kept_terminal(self.uuid()).is_some()
            || self.session()?.remote_target().is_some()
        {
            return None;
        }
        let parent = *self.ivars().shell_parent.get()?;
        let child = self.session()?.child_pid();
        identity::kept_name(parent, child, &SystemTable)
    }

    /// Where the program that showed the view runs (the server's name; `None` on this Mac)
    /// and what it is called (`None` when nothing here knows). A pane that shows a terminal
    /// a server keeps has both from the server's own word; a plain `ssh` knows the place
    /// and not the program (the process table shows `ssh`); a local pane's program is the
    /// first of the jobs in front of its shell.
    fn view_whereabouts(&self) -> (Option<String>, Option<String>) {
        if let Some(kept) = self.host().kept_terminal(self.uuid()) {
            let program = kept
                .program
                .clone()
                .filter(|name| !name.is_empty() && !program::is_shell_name(name));
            return (Some(kept.host), program);
        }
        if let Some((_, target, _)) = self.session().and_then(|session| session.remote_target()) {
            return (Some(target.label().into_owned()), None);
        }
        match self.foreground() {
            Foreground::Running(names) => (None, names.into_iter().next()),
            Foreground::Idle => (None, None),
        }
    }

    /// ⌃C in the view: the interrupt reaches the program, under the same rule as the
    /// terminal's keys ([`TerminalPane::hold_input`]: a pane waiting for its
    /// connection holds it).
    pub(crate) fn interrupt_program(&self) {
        if self.hold_input(false) {
            return;
        }
        if let Some(session) = self.session() {
            session.write(&[pencere_view::INTERRUPT]);
        }
    }
}

/// Pane-level actions and scrollback search — the menu
/// selectors and the search panel's controls land here.
impl TerminalPane {
    /// Remote state or title changed: first the upload queue's connection edge
    /// ([`TerminalPane::check_upload_connection`]; if ssh closed the waiting
    /// ones are cancelled), then the owner
    /// re-reads the title and the tab's label ([`PaneHost::title_changed`]).
    pub(crate) fn remote_or_title_changed(&self) {
        self.remote_edge();
        self.apply_connection();
        // The remote edge moves the alternate screen's share too (a remote vim
        // keeps the status bar); a no-op unless the answer changed.
        self.alt_screen_did_change();
        self.host().title_changed(self.ivars().id);
    }

    /// The remote state's edge, from both of its paths — the probe
    /// ([`TerminalPane::remote_or_title_changed`]) and the title news that
    /// carries `C`/`D`/`A`'s deletion: the upload queue's connection, then the
    /// load indicator's generation.
    pub(crate) fn remote_edge(&self) {
        self.check_upload_connection();
        self.sync_server_door();
        self.sync_ssh_session();
        self.sync_stats_generation();
        // The server session ended, or the pane is not a server pane any more: its
        // helper ssh is not held open until idle.
        if self.server_target().is_none() {
            self.remote_helper().borrow_mut().close();
        }
    }

    /// The remote edge's half for the dock: `⇄ rasp` is the door to the server's card exactly
    /// while the pane is a server pane ([`Self::server_target`]) — written both ways, so a
    /// terminal that goes (the holder was refused after it was admitted and the pane runs the
    /// plain `ssh`, whose process-table target is the same) closes it again. The ssh's
    /// generation is only the session's own gate for the write, which is why this asks the
    /// session for its target: the *rule* is `server_target`'s.
    fn sync_server_door(&self) {
        let Some(session) = self.session() else {
            return;
        };
        let Some((command, ..)) = session.remote_target() else {
            return;
        };
        session.set_server(command, self.server_target().is_some());
    }

    /// The remote edge's half for the masters: an ended
    /// remote session is reported to the registry — our master to the host
    /// ends with the last pane's session — and a new one is registered and,
    /// while the user's ssh still asks (a host key, a password), the login
    /// probe is armed.
    fn sync_ssh_session(&self) {
        let current = self.server_target();
        let previous = self.ivars().ssh_session.get();
        if previous == current.as_ref().map(|(command, ..)| *command) {
            return;
        }
        let masters = self.ivars().masters.as_ref();
        if previous.is_some()
            && let Some(masters) = masters
        {
            masters.session_ended(self.id());
        }
        self.ivars()
            .ssh_session
            .set(current.as_ref().map(|(command, ..)| *command));
        let Some((_, target, _)) = current else {
            return;
        };
        if let Some(masters) = masters {
            masters.session_started(self.id(), &target);
        }
        match self
            .session()
            .and_then(|session| jobs::remote_login(session))
        {
            Some(command) => self.note_login(command),
            None => self.ivars().wake.login_probe.rearm(),
        }
    }

    /// The login probe's check: `true` while the user's ssh has not
    /// logged in — the probe re-arms. Logged in: the background jobs may
    /// connect now, the load indicator starts. Not remote any more: done.
    pub(crate) fn login_check(&self) -> bool {
        let Some(session) = self.session() else {
            return false;
        };
        let Some((command, ..)) = self.server_target() else {
            return false;
        };
        if jobs::remote_login(session) == Some(command) {
            self.sync_stats_generation();
            self.note_login(command);
            return false;
        }
        true
    }

    /// The remote session `command` got past its login: when it is
    /// the wrapped `ssh` the probe found ([`WrapProof`]), its attempt is marked
    /// logged in ([`bt_shell_common::ssh_wrap::mark_login`], a file on a
    /// thread of its own) — the fallback then reads a 255 as an endpoint that
    /// refused our command, not as ssh's own error. Once per generation; a timed run marks nothing.
    fn note_login(&self, command: u64) {
        if self.ivars().run.is_some() {
            return;
        }
        let nonce = {
            let mut proof = self.ivars().wrap_proof.borrow_mut();
            let Some((wrapped, nonce, _)) = proof.wrapped.as_ref() else {
                return;
            };
            if *wrapped != command || proof.login == Some(command) {
                return;
            }
            let nonce = nonce.clone();
            proof.login = Some(command);
            nonce
        };
        let spawned = std::thread::Builder::new()
            .name("remote login".into())
            .spawn(move || {
                if let Some(home) = crate::child::home() {
                    let path = crate::remote_hosts_path(&home);
                    for _ in 0..POSIX_ATTEMPTS {
                        if bt_shell_common::ssh_wrap::mark_login(&path, &nonce).is_ok() {
                            return;
                        }
                    }
                }
            });
        if spawned.is_err() {
            self.ivars().wrap_proof.borrow_mut().login = None;
        }
    }

    /// Edit ▸ Find ▸ Find… (⌘F): opens the panel, focuses the field and selects
    /// its text; if the panel is open, only focus and selection.
    /// If the pane has no query the field fills with the find pasteboard's text.
    pub(crate) fn find(&self) {
        self.open_search(true);
    }

    /// Opens the panel with `text` as the query ("Find … in This Pane" in the command palette):
    /// the field is focused with the text selected, and the matches are shown.
    pub(crate) fn find_text(&self, text: &str) {
        self.search_bar().set_text(text);
        self.open_search(true);
    }

    /// Edit ▸ Find ▸ Find Next (⌘G): the previous, **older** match.
    pub(crate) fn find_next(&self) {
        self.search_step(SearchDirection::Older);
    }

    /// Edit ▸ Find ▸ Find Previous (⇧⌘G): the newer match.
    pub(crate) fn find_previous(&self) {
        self.search_step(SearchDirection::Newer);
    }

    /// Edit ▸ Clear to Start (⌘K): deletes the screen and the
    /// scrollback, the current block stays — `Session::clear_to_start`. No
    /// byte goes to the shell; on the alternate screen the item is grey and
    /// the call is a no-op anyway.
    pub(crate) fn clear_to_start(&self) {
        if let Some(session) = self.session() {
            session.clear_to_start();
        }
    }

    /// Edit ▸ Clear Scrollback (⌥⌘K): scrollback only —
    /// `Session::clear_scrollback`.
    pub(crate) fn clear_scrollback(&self) {
        if let Some(session) = self.session() {
            session.clear_scrollback();
        }
    }

    /// View ▸ Scroll to Top (⌘Home): the start of the scrollback. There is no
    /// new scroll API in `bt-core`: `scroll_page`'s
    /// `saturating_mul` clamps `i32::MAX` pages to the end of the scrollback.
    pub(crate) fn scroll_to_top(&self) {
        self.scroll_pages(i32::MAX);
    }

    /// View ▸ Scroll to Bottom (⌘End): the bottom — `scroll_locked` goes down
    /// to the bottom by the fill-band rule.
    pub(crate) fn scroll_to_bottom(&self) {
        self.scroll_pages(-i32::MAX);
    }

    /// View ▸ Page Up (⌘PgUp): Shift+PgUp's path.
    pub(crate) fn page_up(&self) {
        self.scroll_pages(1);
    }

    /// View ▸ Page Down (⌘PgDn): Shift+PgDn's path.
    pub(crate) fn page_down(&self) {
        self.scroll_pages(-1);
    }

    /// Shell ▸ Cancel Upload (⌘.): this pane's whole upload queue, asking first
    /// if the flowing item has run long ([`TerminalPane::request_stop`]).
    pub(crate) fn cancel_uploads(&self) {
        self.request_stop(true);
    }

    /// View ▸'s four scrolls: `Session::scroll_page`'s path (the very same as
    /// Shift+PgUp/PgDn) — the fraction is reset, the glide generation goes up,
    /// the fill-band rule is in `scroll_locked`. `None` on the alternate screen
    /// and the items are grey anyway; the answer only gates the scroll bar's
    /// poke — a page scroll shows the bar, at either end too.
    fn scroll_pages(&self, pages: i32) {
        if let Some(session) = self.session()
            && session.scroll_page(pages).is_some()
        {
            self.poke_scrollbar();
        }
    }

    /// The search panel — built on the first call, painted with the theme.
    fn search_bar(&self) -> &SearchBar {
        self.ivars().search.get_or_init(|| {
            // The panel is inside the pane, a sibling of the view that carries
            // the Metal layer (the pane is that very container). The field's
            // delegate and the controls' target are
            // the pane — both weak, the pane holds the panel.
            let bar = SearchBar::new(
                self.mtm(),
                self,
                self.view(),
                self,
                ProtocolObject::from_ref(self),
            );
            if let Some(session) = self.session() {
                let theme = session.theme();
                bar.paint(&theme, theme.is_dark());
            }
            bar
        })
    }

    /// Opens the panel (leaves it in place if open) and applies the query; if
    /// `focus`, focuses the field and selects its text (⌘F).
    /// `true` if the query was given to the session in this call ([`TerminalPane::apply_search`]).
    fn open_search(&self, focus: bool) -> bool {
        let bar = self.search_bar();
        if bar.query().text.is_empty()
            && let Some(text) = find_pasteboard_text()
        {
            bar.set_text(&text);
        }
        bar.show(!self.ivars().reduce_motion.get());
        if focus && let Some(window) = self.window() {
            window.makeFirstResponder(Some(bar.field()));
            // SAFETY: the sender is optional; the field's own action.
            unsafe { bar.field().selectText(None) };
        }
        self.apply_search()
    }

    /// If the query of the field and switches changed, gives it to the
    /// session, reveals the current match and writes the label; `true` if it
    /// gave. The same query is a no-op.
    fn apply_search(&self) -> bool {
        let (Some(bar), Some(session)) = (self.ivars().search.get(), self.session()) else {
            return false;
        };
        if !bar.is_shown() {
            return false;
        }
        let query = bar.query();
        if !bar.take_change(&query) {
            return false;
        }
        let status = session.set_search(&query);
        self.ivars().search_status.set(status);
        let report = if status == SearchStatus::Ready {
            let report =
                session.search_reveal(self.search_cover(), self.ivars().smooth_scroll.get());
            // An applied query is search navigation: the bar shows where the
            // current match sits in the history, whether or not the reveal had
            // to move the window to reach it.
            self.poke_scrollbar();
            report
        } else {
            SearchReport::default()
        };
        bar.set_count(status, report);
        self.kick_search();
        true
    }

    /// Sets up the indexes' driver: one chunk on the next turn of the main
    /// queue. A no-op if already set up, or if neither index has work — the
    /// panel is closed or its query is not a pattern to count, and the block
    /// marks are not wanted.
    ///
    /// Its callers: a query change, navigation (a new match whose order is
    /// unknown may want another pass) and the scrollback news
    /// ([`Wake::search_changed`]) — the last one in a background tab too.
    ///
    /// **One latch, two indexes**: the same driver steps the scroll bar's
    /// block index while its marks are wanted
    /// ([`TerminalPane::refresh_marks_wanted`]) — its callers are the marks
    /// becoming wanted and the frame's news (`Wake::blocks_changed`). Each
    /// index says when it is done; the latch drops when both are.
    pub(crate) fn kick_search(&self) {
        if !(self.search_wants_steps() || self.ivars().marks_wanted.get())
            || self.ivars().driving.replace(true)
        {
            return;
        }
        self.schedule_chunk();
    }

    /// Whether the search's count has steps to take: the panel is shown and
    /// its query is a pattern to count.
    fn search_wants_steps(&self) -> bool {
        self.ivars().search.get().is_some_and(SearchBar::is_shown)
            && self.ivars().search_status.get() == SearchStatus::Ready
    }

    /// One turn of the driver onto the main queue: the pane is found by id
    /// (`ShellWake`'s pattern), the job drops for a pane that closed.
    fn schedule_chunk(&self) {
        let (id, lookup) = (self.ivars().id, self.ivars().lookup);
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.drive_chunk();
            }
        });
    }

    /// One turn of the driver: a chunk of each index that has work, then
    /// set up again for the next turn while either has more — key events
    /// slip in between turns. The next turn comes at once, but for an
    /// unfinished block index while output streams: then a display frame
    /// later ([`BLOCK_PACE`]), a step a frame.
    fn drive_chunk(&self) {
        let search_done = self.search_chunk();
        let (blocks_done, streaming) = self.blocks_chunk();
        if search_done && blocks_done {
            self.ivars().driving.set(false);
        } else if search_done && streaming {
            self.schedule_chunk_paced();
        } else {
            self.schedule_chunk();
        }
    }

    /// [`TerminalPane::schedule_chunk`] a display frame later; at once if the
    /// delay cannot be told.
    fn schedule_chunk_paced(&self) {
        let Ok(when) = DispatchTime::try_from(BLOCK_PACE) else {
            self.schedule_chunk();
            return;
        };
        let (id, lookup) = (self.ivars().id, self.ivars().lookup);
        let _ = DispatchQueue::main().after(when, move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.drive_chunk();
            }
        });
    }

    /// A chunk of the count and the label; `true` → done. The stop
    /// condition is the core's `complete` (the pass is done **and** no
    /// pending scrollback news), the panel closing or the search being dropped.
    fn search_chunk(&self) -> bool {
        if !self.search_wants_steps() {
            return true;
        }
        let (Some(bar), Some(session)) = (self.ivars().search.get(), self.session()) else {
            return true;
        };
        let Some(report) = session.search_step() else {
            return true;
        };
        // A pass reached the top with other rows: the scroll bar's marks of
        // the whole history changed, and the step itself asks for no frame.
        if report.marks_changed
            && let Some(link) = self.link()
        {
            link.marks_changed();
        }
        bar.set_count(self.ivars().search_status.get(), report);
        report.complete
    }

    /// A step of the block index; `true` → done (the marks are not wanted,
    /// or every row is looked at and no news waits), and whether output is
    /// streaming. A different picture asks for the frame that draws it
    /// through the link — only while a bar is up to show it
    /// (`bt_gpu::DisplayLink::marks_changed`).
    ///
    /// **The pace is the index's own**, not the driver's: while output
    /// streams an unfinished index steps at most once per [`BLOCK_PACE`]
    /// however often the search's count turns the driver — a turn inside
    /// the pace skips the step and answers "not done, streaming".
    fn blocks_chunk(&self) -> (bool, bool) {
        let ivars = self.ivars();
        if !ivars.marks_wanted.get() {
            return (true, false);
        }
        if ivars
            .block_paced
            .get()
            .is_some_and(|at| at.elapsed() < BLOCK_PACE)
        {
            return (false, true);
        }
        let Some(report) = self.session().and_then(|session| session.block_step()) else {
            return (true, false);
        };
        if report.marks_changed
            && let Some(link) = self.link()
        {
            link.marks_changed();
        }
        ivars
            .block_paced
            .set((report.streaming && !report.complete).then(Instant::now));
        (report.complete, report.streaming)
    }

    /// ⏎ / ⌘G / ⇧⏎ / ⇧⌘G: if the panel is closed it is opened first (focus
    /// stays in place), then the next match.
    ///
    /// If the opening gave the query **again** (Esc had closed the search) the
    /// step is that selection itself: `set_search` chose and revealed the
    /// nearest match and one more step on top would make ⇧⌘G wrap to the
    /// oldest.
    fn search_step(&self, direction: SearchDirection) {
        if self.open_search(false) {
            return;
        }
        let (Some(bar), Some(session)) = (self.ivars().search.get(), self.session()) else {
            return;
        };
        let status = self.ivars().search_status.get();
        if status != SearchStatus::Ready {
            return;
        }
        let report = session.search_next(
            direction,
            self.search_cover(),
            self.ivars().smooth_scroll.get(),
        );
        // A search jump is scrolling input: the bar shows where it landed.
        self.poke_scrollbar();
        bar.set_count(status, report);
        if !report.complete {
            self.kick_search();
        }
    }

    /// Esc and the close button: the panel goes away, **the
    /// window stays in place**, the current match becomes the grid's selection
    /// and the keyboard returns to the terminal. The query stays in the field.
    pub(crate) fn close_search(&self) {
        let Some(bar) = self.ivars().search.get() else {
            return;
        };
        bar.hide(!self.ivars().reduce_motion.get());
        bar.forget_applied();
        if let Some(session) = self.session() {
            session.select_search_match();
            session.clear_search();
        }
        self.ivars().search_status.set(SearchStatus::Empty);
        if let Some(window) = self.window() {
            window.makeFirstResponder(Some(self.view()));
        }
    }

    /// Edit ▸ Find ▸ Use Selection for Find (⌘E): the selection's
    /// first line (grid or dock) becomes the query — escaped in regex mode —,
    /// is written to the find pasteboard and the panel opens with the field focused.
    pub(crate) fn use_selection_for_find(&self) {
        let Some(text) = self.session().and_then(|session| session.selection_text()) else {
            return;
        };
        let bar = self.search_bar();
        let Some(query) = selection_query(&text, bar.regex()) else {
            return;
        };
        bar.set_text(&query);
        // The pasteboard carries **plain** text: other applications do not know regex mode.
        if let Some(plain) = selection_query(&text, false) {
            // SAFETY: a constant name AppKit exposes, lives for the whole process.
            let name = unsafe { NSPasteboardNameFind };
            clipboard::copy(&NSPasteboard::pasteboardWithName(name), Some(plain));
        }
        self.open_search(true);
    }

    /// The folder of this Mac the pane's shell is in; `None` in a remote session (its
    /// directory is the one the shell left) and before the shell says.
    fn local_folder(&self) -> Option<std::path::PathBuf> {
        let session = self.session()?;
        if session.remote_mark().is_some() {
            return None;
        }
        session.working_directory()
    }

    /// The folder as Copy Folder Path writes it: the path, or `host:path` in a remote session
    /// (the host alone when the server says no folder), the way the tab's place line says it.
    fn folder_text(&self) -> Option<String> {
        let session = self.session()?;
        match session.remote_mark() {
            Some((host, _)) => {
                let folder = session.remote_link_directory();
                Some(if folder.is_empty() {
                    host
                } else {
                    format!("{host}:{folder}")
                })
            }
            None => session
                .working_directory()
                .map(|folder| folder.to_string_lossy().into_owned()),
        }
    }

    /// Gate of Find Next/Previous: whether the pane has a query or the find
    /// pasteboard has text.
    fn has_query(&self) -> bool {
        self.ivars()
            .search
            .get()
            .is_some_and(|bar| !bar.query().text.is_empty())
            || find_pasteboard_text().is_some()
    }

    /// The cells the panel covers; none if the panel is closed. The panel's
    /// coordinates are the pane's (its container is that).
    fn search_cover(&self) -> SearchCover {
        let Some(bar) = self.ivars().search.get().filter(|bar| bar.is_shown()) else {
            return SearchCover::default();
        };
        let view = self.view();
        view.search_cover(view.convertRect_fromView(bar.resting_frame(), Some(self)))
    }

    /// The upload queue (half of `uploader`).
    pub(crate) fn uploads(&self) -> &RefCell<Transfers> {
        &self.ivars().uploads
    }

    /// The helper ssh session's handle.
    pub(crate) fn remote_helper(&self) -> &RefCell<RemoteHelper> {
        &self.ivars().remote_helper
    }

    /// The pane's seat in its server's load sampling ([`crate::stats`]).
    pub(crate) fn stats_seat(&self) -> &RefCell<StatsSeat> {
        &self.ivars().stats
    }

    /// The pane's last input ([`PaneIvars::last_input`]); its one writer is
    /// [`TerminalPane::note_interaction`].
    pub(crate) fn input_stamp(&self) -> &Cell<Moment> {
        &self.ivars().last_input
    }

    /// `[remote]`'s preview and download keys as last read.
    pub(crate) fn remote_files(&self) -> &RefCell<RemoteFiles> {
        &self.ivars().remote_files
    }

    /// The previews this pane downloaded, by landing path.
    pub(crate) fn previews(&self) -> &RefCell<HashMap<PathBuf, PreviewTicket>> {
        &self.ivars().previews
    }

    /// The file promises of ⌘-dragged remote links ([`crate::promise`]).
    pub(crate) fn finder_drops(&self) -> &RefCell<FinderDrops> {
        &self.ivars().finder
    }

    /// The open upload sheet's slot.
    pub(crate) fn upload_alert(&self) -> &RefCell<Option<Retained<NSAlert>>> {
        &self.ivars().upload_alert
    }

    /// The open stop question's slot.
    pub(crate) fn upload_stop(&self) -> &RefCell<Option<StopSheet>> {
        &self.ivars().upload_stop
    }

    /// The open password sheet's slot.
    pub(crate) fn password(&self) -> &RefCell<Option<PasswordSheet>> {
        &self.ivars().password
    }

    /// The questions waiting for this pane's tab to come on screen — the
    /// sheet gate's slot ([`crate::sheets`]).
    pub(crate) fn parked(&self) -> &RefCell<crate::sheets::ParkedQueue> {
        &self.ivars().parked
    }

    /// The questions this pane has up on its tab's owner — the sheet gate's
    /// slot ([`crate::sheets`]).
    pub(crate) fn opened(&self) -> &RefCell<Vec<crate::sheets::Opened>> {
        &self.ivars().opened
    }

    /// The application's ssh masters; `None` in a timed run.
    pub(crate) fn masters(&self) -> Option<Arc<Masters>> {
        self.ivars().masters.clone()
    }

    /// The open "Show transfers (N)" popover's slot.
    pub(crate) fn upload_list(&self) -> &RefCell<Option<UploadPopover>> {
        &self.ivars().upload_list
    }

    /// The time of the event that closed the popover.
    pub(crate) fn list_closed_at(&self) -> &Cell<Option<f64>> {
        &self.ivars().list_closed_at
    }

    /// The open server card.
    pub(crate) fn server_card(&self) -> &RefCell<Option<ServerCard>> {
        &self.ivars().server_card
    }

    pub(crate) fn listeners(&self) -> &RefCell<Vec<Listener>> {
        &self.ivars().listeners
    }

    pub(crate) fn environment_wants(&self) -> &Cell<Wants> {
        &self.ivars().environment_wants
    }

    pub(crate) fn watched(&self) -> &RefCell<HashSet<(u32, u64)>> {
        &self.ivars().watched
    }

    pub(crate) fn remote_ports(&self) -> &RefCell<RemotePorts> {
        &self.ivars().remote_ports
    }

    /// The listening ports' probe — on or off with `[shell] ports`.
    pub(crate) fn port_probe(&self) -> &PortProbe {
        &self.ivars().wake.port_probe
    }

    /// Schedules a scan of the listening ports, as an edge would
    /// ([`ShellWake::poke_ports`]).
    pub(crate) fn poke_ports(&self) {
        self.ivars().wake.poke_ports();
    }

    pub(crate) fn shell_parent(&self) -> Option<ShellParent> {
        self.ivars().shell_parent.get().copied()
    }

    /// The time of the event that closed the server card.
    pub(crate) fn card_closed_at(&self) -> &Cell<Option<f64>> {
        &self.ivars().card_closed_at
    }

    /// The queue's sent and total bytes; `None` if there is no queue — the
    /// input of the owner's Dock icon total ([`PaneHost::uploads_changed`]).
    pub(crate) fn upload_totals(&self) -> Option<(u64, u64)> {
        self.ivars().uploads.borrow().totals()
    }

    /// Whether the queue is running (Dock icon, `cancelUpload:`'s gate).
    pub(crate) fn upload_active(&self) -> bool {
        self.ivars().uploads.borrow().active()
    }

    /// The arrow and percentage of the title's `↑ N% · ` prefix (`↓` while
    /// only downloads flow); `None` if nothing is flowing (`upload::titled_as`).
    pub(crate) fn upload_title_prefix(&self) -> Option<(&'static str, u8)> {
        self.ivars().uploads.borrow().title_prefix()
    }

    /// The name of the item the transfer line speaks of; `None` while
    /// nothing streams — the tab bar's summary card.
    pub(crate) fn upload_flowing_name(&self) -> Option<String> {
        self.ivars()
            .uploads
            .borrow()
            .flowing_name()
            .map(str::to_owned)
    }
}

/// What made a pane a program's (the pane's `asked_for`); the default is a pane the user
/// opened.
#[derive(Clone, Debug, Default)]
struct AskedFor {
    /// The pane closes when its terminal ends.
    close_at_end: bool,
    /// Who asked for it, as its tab's card says.
    opener: Option<String>,
}

/// [`adopt_session`]'s failure: why, and the carried history for the
/// fallback's replay.
type NotAdopted = (std::io::Error, Option<Vec<u8>>);

/// Carries a frozen pane on in this process ([`Session::adopt`]):
/// born at the frozen grid size (the VT is laid out for it), then resized
/// to this pane's grid. `Err` gives back the carried history for the
/// fallback.
fn adopt_session(
    options: SessionOptions,
    adopted: Adopted,
    grid: Grid,
    wake: &Arc<dyn Wake>,
) -> Result<(Session, ShellParent), NotAdopted> {
    let Adopted {
        master,
        exit,
        pid,
        mut state,
        prefix,
        taken_from: _,
        mode,
        nudge: _,
        note: _,
    } = adopted;
    let history = Some(std::mem::take(&mut state.history)).filter(|bytes| !bytes.is_empty());
    let parent = state.parent;
    let options = SessionOptions {
        cols: state.cols,
        rows: state.rows,
        ..options
    };
    let session = Session::adopt(
        options,
        bt_core::Adoption {
            master,
            exit,
            pid,
            vt: state.vt,
            blob: state.core,
            prefix,
            input: state.input,
            ops: Arc::new(jobs::SystemPty),
            mode,
        },
        Arc::clone(wake),
    )
    .map_err(|error| (error, history))?;
    // `false` is "nothing to do" (the same size) or a refused one (zero):
    // the session keeps the frozen size, the next geometry change resizes.
    let _ = session.resize(grid.cols, grid.rows, grid.cell.cell_px());
    Ok((session, parent))
}

/// How long a nudged program sees the narrower size before the real one
/// comes back ([`nudge_program`]). A program behind ssh redraws only if ssh
/// **read** the narrower size — it forwards the size it reads, and the remote
/// kernel signals only a change — and ssh reads it on its next turn after
/// the signal, a scheduling round; a quarter of a second covers a loaded
/// machine and is too short to notice the narrower frame. A design constant.
const NUDGE_GAP: std::time::Duration = std::time::Duration::from_millis(250);

/// Nudges an adopted pane's program to redraw the screen that did not come
/// back: the PTY one column narrower now, its own size [`NUDGE_GAP`] later
/// (`Session::nudge_size`). The second half is skipped once the session's
/// reader is gone (the pane closed meanwhile).
fn nudge_program(session: &Arc<Session>) {
    session.nudge_size(true);
    let back = Arc::clone(session);
    let restore = move || {
        if back.reader_alive() {
            back.nudge_size(false);
        }
    };
    match DispatchTime::try_from(NUDGE_GAP) {
        Ok(when) => {
            let _ = DispatchQueue::main().after(when, restore);
        }
        Err(_) => restore(),
    }
}

/// What closing a pane that shows a terminal on a server would end: the program
/// in front there with the server's name (`npm on rasp`), or nothing when the
/// shell is waiting for a command (or the server has not said).
fn kept_foreground(kept: &Shown) -> Foreground {
    match kept.running() {
        Some(running) => Foreground::Running(vec![running]),
        None => Foreground::Idle,
    }
}

/// A new pane identity, from `NSUUID`.
fn new_pane_uuid() -> PaneUuid {
    // `UUIDString` gives the canonical 8-4-4-4-12 form; if `parse` rejects it
    // the defect is in `bt-core`'s contract, not on this line.
    PaneUuid::parse(&NSUUID::new().UUIDString().to_string())
        .expect("NSUUID's UUIDString must be a canonical UUID")
}

#[cfg(test)]
mod tests {
    /// The command memory is filled on the start of a command, while its rows are on the grid: a
    /// start edge that does not ask for the memory's turn loses the command from the palette's
    /// history without anything failing. A source scan: the edges are the reader thread's.
    #[test]
    fn both_start_edges_ask_for_the_command_memorys_turn() {
        let source = include_str!("pane.rs");
        let code = &source[..source.find("#[cfg(test)]").expect("the tests follow")];
        let body = |signature: &str| {
            let start = code.find(signature).expect("the method is in this file");
            let rest = &code[start + signature.len()..];
            &rest[..rest.find("\n    fn ").unwrap_or(rest.len())]
        };
        assert!(body("    fn command_started(&self) {").contains("self.remember_soon()"));
        assert!(body("    fn remote_command_edge(&self) {").contains("self.remember_soon()"));
        assert!(body("    fn remember_soon(&self) {").contains("pane.remember_commands()"));
    }

    /// The add-ons that work on a server are offered in a pane that shows a terminal a server
    /// keeps and in no other, and the rule is one predicate, `server_target`: an add-on that
    /// starts from `remote_target` (which only says an `ssh` is running) would be offered on a
    /// plain `ssh` and open a connection of bateri's own to the host, and nothing else would say
    /// so. A source scan, because the panes need a window server.
    #[test]
    fn every_add_on_on_a_server_asks_the_one_gate() {
        let add_ons = [
            ("uploader.rs", include_str!("uploader.rs")),
            ("preview.rs", include_str!("preview.rs")),
            ("promise.rs", include_str!("promise.rs")),
            ("stats.rs", include_str!("stats.rs")),
            ("server_card.rs", include_str!("server_card.rs")),
            ("hyperlink.rs", include_str!("hyperlink.rs")),
            ("remote_ports.rs", include_str!("remote_ports.rs")),
            ("footer.rs", include_str!("footer.rs")),
            ("password_sheet.rs", include_str!("password_sheet.rs")),
        ];
        for (name, source) in add_ons {
            let code = &source[..source.find("#[cfg(test)]").unwrap_or(source.len())];
            assert!(
                !code.contains("remote_target()"),
                "{name}: an add-on asks `server_target`, not `remote_target`"
            );
            assert!(code.contains("server_target()"), "{name} asks the gate");
        }
        // The entries each add-on is started from, one by one.
        let entry = |source: &str, signature: &str| {
            let at = source
                .find(signature)
                .unwrap_or_else(|| panic!("{signature}"));
            let rest = &source[at..];
            let end = rest.find("\n    }\n").expect("its end");
            rest[..end].to_owned()
        };
        let uploader = include_str!("uploader.rs");
        for signature in [
            "pub(crate) fn upload_drop(",
            "pub(crate) fn download_remote(",
            "pub(crate) fn check_upload_connection(",
        ] {
            assert!(
                entry(uploader, signature).contains("server_target()"),
                "{signature}"
            );
        }
        assert!(entry(uploader, "pub(crate) fn accepts_drop(").contains("is_plain_ssh()"));
        assert!(
            entry(include_str!("preview.rs"), "fn preview_remote(").contains("server_target()")
        );
        assert!(
            entry(
                include_str!("stats.rs"),
                "pub(crate) fn sync_stats_generation("
            )
            .contains("server_target()")
        );
        assert!(
            entry(
                include_str!("remote_ports.rs"),
                "pub(crate) fn request_remote_ports("
            )
            .contains("server_target()")
        );
        let hyperlink = include_str!("hyperlink.rs");
        for signature in ["fn check(&self, hit: &LinkHit)", "fn verify_remote("] {
            assert!(
                entry(hyperlink, signature).contains("server_target()"),
                "{signature}"
            );
        }
        let sheet = include_str!("password_sheet.rs");
        for signature in [
            "pub(crate) fn can_forget_password(",
            "pub(crate) fn forget_password(",
        ] {
            assert!(
                entry(sheet, signature).contains("server_target()"),
                "{signature}"
            );
        }
        // And the pane's half of the connection bateri keeps for them: it is registered
        // and its login is probed for a server pane only.
        let pane = include_str!("pane.rs");
        for signature in ["fn sync_ssh_session(", "pub(crate) fn login_check("] {
            let body = entry(pane, signature);
            assert!(body.contains("server_target()"), "{signature}");
            assert!(!body.contains("remote_target()"), "{signature}");
        }
        // ⌘T and ⌘D take a pane for a server's only through the same gate.
        assert!(
            entry(include_str!("app.rs"), "    pub(crate) fn pane_launch_as(")
                .contains("server_target"),
            "the new pane's first program is the source's terminal only if the source is a server pane"
        );
        // The terminal appearing or going runs the pane's remote edge again: the probe and
        // the holder race, and the second to arrive is the one that opens the gate.
        let changed = entry(include_str!("hold_service.rs"), "fn terminal_changed(");
        assert!(changed.contains("remote_edge()"));
    }

    /// The server's word of what is in front of its terminal is for the icon: what a program on
    /// this Mac can do is asked of [`TerminalPane::local_program_kind`] — a program on a server
    /// reads the server's clipboard, so ⌘V's image key (⌃V) is not sent to it, and the next
    /// bateri is carried what runs here. A source scan, because the panes need a window server.
    #[test]
    fn what_only_a_program_here_can_do_asks_the_kind_of_the_program_here() {
        let paste = include_str!("view.rs");
        let paste = &paste[..paste.find("#[cfg(test)]").unwrap_or(paste.len())];
        assert!(paste.contains("image_paste_key(pane.local_program_kind())"));
        assert!(!paste.contains("image_paste_key(pane.program_kind())"));
        let pane = include_str!("pane.rs");
        let at = pane.find("fn carried_program(").expect("carried_program");
        let body = &pane[at..at + pane[at..].find("\n    }\n").expect("its end")];
        assert!(body.contains("local_program_kind()"));
        assert!(!body.contains(" self.program_kind()"));
        // The server's word reaches the pane through its own entry and no other.
        let hold = include_str!("hold_service.rs");
        let at = hold.find("fn program_changed(").expect("program_changed");
        let body = &hold[at..at + hold[at..].find("\n}\n").expect("its end")];
        assert!(body.contains("server_program_changed()"));
    }

    /// A program's view is started again at the two places the screen is restored without the
    /// messages it lives on: the pane that took its program over from an earlier process, and the
    /// pane whose server terminal's connection came back. The handover's record of the view is
    /// the one the pane reads (`shows_view`, in front of the quiet-down), and the reconnection's
    /// one rule is [`PaneLink::comes_back`]. A source scan, because the panes need a window
    /// server and the first needs a process to hand over from.
    #[test]
    fn a_view_is_restarted_where_the_screen_comes_back_without_its_messages() {
        let pane = include_str!("pane.rs");
        let pane = &pane[..pane.find("#[cfg(test)]").unwrap_or(pane.len())];
        let at = pane.find("fn freeze_for_handover(").expect("the freeze");
        let body = &pane[at..at + pane[at..].find("\n    }\n").expect("its end")];
        let seen = body
            .find("self.shows_view()")
            .expect("the view is recorded");
        let quiet = body.find("self.quiesce(false)").expect("the quiet-down");
        assert!(seen < quiet, "recorded before the quiet-down");
        assert!(
            body.contains("            view,\n"),
            "and written into the state"
        );
        let at = pane
            .find("let viewed = adopted.state.view;")
            .expect("the adoption reads it");
        let body = &pane[at..at + pane[at..].find("nudge_program(&session);").expect("nudge")];
        assert!(body.contains("view_was_open = viewed;"));
        assert!(body.contains("carried_on && view_was_open"));
        assert!(body.contains("self.restart_view_offered(&session);"));
        let at = pane
            .find("pub(crate) fn set_connection(")
            .expect("set_connection");
        let body = &pane[at..at + pane[at..].find("\n    }\n").expect("its end")];
        assert!(body.contains("before.comes_back(link)"));
        assert!(body.contains("self.shows_view()"));
        assert!(body.contains("restart_view_offered("));
        // After the early return, so a retry that changes nothing restarts nothing.
        assert!(
            body.find("return;").expect("the return") < body.find("restart_view_offered(").unwrap()
        );
    }

    /// The token a restart offers the program is a token it was handed unasked, and a program
    /// that never opens its view again must not leave it valid for as long as the pane lives:
    /// the offer has a time to be taken up (`Waits`), set where the offer is made. A call of
    /// `restart_view` anywhere else would offer a token with no clock behind it.
    #[test]
    fn a_restart_is_offered_only_where_the_offer_has_its_time() {
        let pane = include_str!("pane.rs");
        let pane = &pane[..pane.find("#[cfg(test)]").unwrap_or(pane.len())];
        let calls = pane
            .lines()
            .filter(|line| !line.trim_start().starts_with("//") && line.contains(".restart_view()"))
            .count();
        assert_eq!(calls, 1, "one call, in the function that sets the time");
        let at = pane.find("fn restart_view_offered(").expect("the offer");
        let body = &pane[at..at + pane[at..].find("\n    }\n").expect("its end")];
        assert!(body.contains(".restart_view()"));
        assert!(body.contains("restart_offered()") && body.contains("wait_for_view("));
    }

    /// The requests a view has out are given up when their time runs out: the bridge says when a
    /// message of the view made the pane write to the program (`view_message` returns it), the
    /// pane sets the wait, and when it is up the conversation fails them and the view is told at
    /// once. `Session::view_requests_overdue` had no caller: the wait was never set. A source scan,
    /// because the panes need a window server and the time is a minute.
    #[test]
    fn the_requests_a_view_has_out_are_given_up_when_their_time_runs_out() {
        let bridge = include_str!("pencere_view.rs");
        let at = bridge.find("fn receive(").expect("the message handler");
        let body = &bridge[at..at + bridge[at..].find("\n        }\n").expect("its end")];
        assert!(
            body.contains(".view_message(") && body.contains("view_requests_out()"),
            "a message that went to the program sets the wait"
        );
        let pane = include_str!("pane.rs");
        let pane = &pane[..pane.find("#[cfg(test)]").unwrap_or(pane.len())];
        let at = pane
            .find("fn view_requests_ended(")
            .expect("the wait's end");
        let body = &pane[at..at + pane[at..].find("\n    }\n").expect("its end")];
        assert!(body.contains("requests_expired(number)"));
        assert!(body.contains("session.view_requests_overdue()") && body.contains("drain_views()"));
        let at = pane
            .find("fn view_requests_out(")
            .expect("the wait's start");
        let body = &pane[at..at + pane[at..].find("\n    }\n").expect("its end")];
        assert!(body.contains("requests_out()") && body.contains("Self::view_requests_ended"));
    }

    /// A session is opened to host views with how its shell is found from its child (`login(1)`
    /// forks it on macOS: the child's own pid is not the shell's group, and a pane that took it
    /// for one answered a `hello` into a line editor) and with the pane's area in points (the
    /// grid's device pixels are twice that on a Retina display).
    #[test]
    fn a_pane_gives_its_session_how_to_find_the_shell_and_the_area_in_points() {
        let pane = include_str!("pane.rs");
        let pane = &pane[..pane.find("#[cfg(test)]").unwrap_or(pane.len())];
        let at = pane.find("fn view_options(").expect("view_options");
        let body = &pane[at..at + pane[at..].find("\n    }\n").expect("its end")];
        assert!(body.contains("shell_parent: ShellParent"));
        assert!(body.contains("jobs::shell_group(shell_parent, child, &SystemTable)"));
        assert!(body.contains("pencere_view::content_size(self.bounds())"));
        // The parent is the one the command was chosen with, in the same branch.
        assert!(pane.contains("views: self.view_options(shell_parent),"));
    }

    /// The news that a view has something for the pane arrives on the reader's thread, possibly
    /// under the `Term` lock (a command's end that tore the view down): it takes nothing and posts
    /// one job, which the comment says as `Wake` does — not "no `Term` lock held".
    #[test]
    fn the_news_of_a_view_is_a_job_posted_with_nothing_taken() {
        let pane = include_str!("pane.rs");
        let pane = &pane[..pane.find("#[cfg(test)]").unwrap_or(pane.len())];
        let at = pane
            .find("    fn view_changed(&self) {")
            .expect("view_changed");
        let body = &pane[at..at + pane[at..].find("\n    }\n").expect("its end")];
        assert!(body.contains("`Term` lock may be held"), "{body}");
        assert!(!body.contains("no `Term` lock"), "{body}");
        for taken in ["session", "lock(", ".lock()", "borrow"] {
            assert!(
                !body.contains(taken),
                "nothing is taken in the news: {taken}"
            );
        }
        assert!(body.contains("pane.drain_views()"));
        let wake = include_str!("../../bt-core/src/wake.rs");
        assert!(
            wake.contains("possibly under it when a command's"),
            "the trait says the `Term` lock may be held"
        );
    }

    /// A pane that shows a program's view has the keyboard in the view, and the window's ways of
    /// giving the keyboard to a pane (a pane switched to, a tab shown, the name field giving it up)
    /// go through [`TerminalPane::focus`], which knows it: given to the terminal under the view
    /// instead, the keyboard is in a view nobody can see and the program's view takes no key — a
    /// restored view found that after a handover. The one place that gives it to a terminal
    /// directly is a window's birth, when no pane has a view yet; and a pane that is the user's
    /// with its keyboard in the covered terminal moves it ([`TerminalPane::apply_focus`]). A source
    /// scan, because what it guards needs a window server and a pane with a view in a window.
    #[test]
    fn the_keyboard_is_given_to_a_pane_through_the_pane_and_not_to_the_terminal_under_a_view() {
        let live =
            |source: &'static str| &source[..source.find("#[cfg(test)]").unwrap_or(source.len())];
        // The arguments of every `makeFirstResponder(` that name a terminal view.
        let to_a_terminal = |source: &str| {
            source
                .match_indices("makeFirstResponder(")
                .filter(|(at, _)| {
                    let call = &source[*at..(*at + 120).min(source.len())];
                    call[..call.find(");").unwrap_or(call.len())].contains(".view()")
                })
                .count()
        };
        assert_eq!(
            to_a_terminal(live(include_str!("window.rs"))),
            1,
            "only a window's birth gives the keyboard to a terminal itself"
        );
        assert_eq!(to_a_terminal(live(include_str!("tab.rs"))), 0);
        let pane = live(include_str!("pane.rs"));
        let at = pane
            .find("pub(crate) fn apply_focus(")
            .expect("apply_focus");
        let body = &pane[at..at + pane[at..].find("\n    }\n").expect("its end")];
        assert!(body.contains("self.keep_keyboard_in_view();"), "{body}");
        let at = pane.find("fn keep_keyboard_in_view(").expect("the rule");
        let body = &pane[at..at + pane[at..].find("\n    }\n").expect("its end")];
        assert!(body.contains("self.shows_view()") && body.contains("self.focus()"));
    }

    #[test]
    fn a_pane_that_waits_for_its_connection_takes_every_key_but_enter() {
        use super::{Wait, on_wait};
        use bt_shell_common::kablo::service::PaneLink;
        for link in [PaneLink::Connecting, PaneLink::Live] {
            // The first connection and a live one are the terminal's own.
            assert_eq!(on_wait(link, false), Wait::Pass, "{link:?}");
            assert_eq!(on_wait(link, true), Wait::Pass, "{link:?}");
        }
        assert_eq!(on_wait(PaneLink::Reconnecting, true), Wait::TryNow);
        assert_eq!(on_wait(PaneLink::NeedsLogin, true), Wait::LogIn);
        // A terminal another computer took is taken back by ⏎ — not dialed for, which would
        // try a connection that is there.
        assert_eq!(on_wait(PaneLink::Taken, true), Wait::BringBack);
        for link in [
            PaneLink::Reconnecting,
            PaneLink::NeedsLogin,
            PaneLink::Taken,
        ] {
            assert_eq!(on_wait(link, false), Wait::Beep, "{link:?}");
        }
    }

    #[test]
    fn closing_a_pane_on_a_server_asks_about_the_program_there() {
        use crate::jobs::Foreground;
        use bt_shell_common::kablo::service::{PaneLink, Shown};
        let shown = |program: Option<&str>| Shown {
            account: "u@rasp:22".into(),
            host: "rasp".into(),
            argv: vec!["ssh".into(), "rasp".into()],
            terminal: bt_shell_common::kablo::SessionId::new("b-1").expect("an id"),
            program: program.map(str::to_owned),
            link: PaneLink::Live,
            taken_by: None,
        };
        assert_eq!(
            super::kept_foreground(&shown(Some("npm"))),
            Foreground::Running(vec!["npm on rasp".to_owned()])
        );
        // What another computer took is not running here: closing the dimmed pane asks nothing.
        let taken = Shown {
            link: PaneLink::Taken,
            taken_by: Some("iMac".into()),
            ..shown(Some("npm"))
        };
        assert_eq!(super::kept_foreground(&taken), Foreground::Idle);
        // The shell waiting for a command is not a job, nor is a server that
        // has said nothing yet; a login shell is named with a dash.
        for program in [Some("zsh"), Some("-bash"), Some("fish"), Some(""), None] {
            assert_eq!(
                super::kept_foreground(&shown(program)),
                Foreground::Idle,
                "{program:?}"
            );
        }
    }

    #[test]
    fn pane_uuids_are_canonical_and_distinct() {
        let (a, b) = (super::new_pane_uuid(), super::new_pane_uuid());
        assert_ne!(a, b, "two NSUUID identities must differ");
        assert_eq!(bt_core::PaneUuid::from_url(&a.url()), Some(a));
    }

    /// Fake owner: records the events with their ids — no window, no
    /// pasteboard.
    #[derive(Default)]
    struct FakeHost(std::cell::RefCell<Vec<(u64, String)>>);

    impl super::PaneHost for FakeHost {
        fn title_changed(&self, pane: u64) {
            self.0.borrow_mut().push((pane, "title".into()));
        }
        fn focused(&self, pane: u64) {
            self.0.borrow_mut().push((pane, "focused".into()));
        }
        fn shell_exited(&self, pane: u64) {
            self.0.borrow_mut().push((pane, "exit".into()));
        }
        fn uploads_changed(&self, pane: u64) {
            self.0.borrow_mut().push((pane, "uploads".into()));
        }
        fn activity_changed(&self, pane: u64) {
            self.0.borrow_mut().push((pane, "activity".into()));
        }
        fn notify(&self, pane: u64, title: &str, _body: &str) {
            self.0.borrow_mut().push((pane, format!("notify {title}")));
        }
        fn post_notices(&self, pane: u64, _source: crate::notices::Source, _messages: Vec<String>) {
            self.0.borrow_mut().push((pane, "notices".into()));
        }
        fn copy_to_clipboard(&self, pane: u64, text: String) {
            self.0.borrow_mut().push((pane, format!("copy {text}")));
        }
        fn files_dragged(&self, pane: u64, over: bool) {
            self.0.borrow_mut().push((pane, format!("files {over}")));
        }
        fn carry_press(&self, pane: u64, _at: (f64, f64)) {
            self.0.borrow_mut().push((pane, "carry".into()));
        }
        fn questions_changed(&self, pane: u64) {
            self.0.borrow_mut().push((pane, "questions".into()));
        }
        fn ports_changed(&self, pane: u64) {
            self.0.borrow_mut().push((pane, "ports".into()));
        }
        fn bell(&self, pane: u64) {
            self.0.borrow_mut().push((pane, "bell".into()));
        }
        fn program_notification(&self, pane: u64, _title: Option<&str>, _body: &str) {
            self.0.borrow_mut().push((pane, "note".into()));
        }
        fn cover(
            &self,
            _pane: &super::TerminalPane,
        ) -> Option<std::rc::Rc<dyn crate::sheets::Cover>> {
            None
        }
    }

    #[test]
    fn an_owner_with_no_shortcuts_of_its_own_keeps_none_from_a_view() {
        use bt_core::{Chord, Key, Mods};
        // Its menu still gets what the view leaves unhandled; the view just is not stopped
        // from seeing ⌘T first.
        let host = FakeHost::default();
        let chord = Chord::plain(Mods::CMD, Key::Char('t'));
        assert!(!super::PaneHost::keeps_key(&host, &chord));
    }

    #[test]
    fn title_and_copy_events_reach_the_host_with_the_pane_id() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let host = FakeHost::default();
        // Title: the flag drops, the event goes to the owner with the pane's id.
        // The pane's edge runs after the flag dropped.
        let pending = AtomicBool::new(true);
        let edge_saw = std::cell::Cell::new(true);
        super::announce_title(
            &pending,
            || edge_saw.set(pending.load(Ordering::Acquire)),
            &host,
            7,
        );
        assert!(!pending.load(Ordering::Acquire));
        assert!(!edge_saw.get(), "the edge ran before the flag dropped");
        // Copy: the text in the slot goes to the owner, not to the general
        // pasteboard; an empty slot produces no event.
        let copy = crate::clipboard::PendingCopy::default();
        assert!(copy.put("osc52".into()));
        super::announce_copy(&copy, &host, 7);
        super::announce_copy(&copy, &host, 7);
        assert_eq!(
            *host.0.borrow(),
            vec![(7, "title".to_owned()), (7, "copy osc52".to_owned())]
        );
    }

    #[test]
    fn remote_probe_repeats_only_while_undecided() {
        use super::RemoteProbe;
        let probe = RemoteProbe::default();
        // When unarmed, output throws no probe.
        assert!(!probe.output());
        // The `C` edge sets the arm and throws a single job; output arriving
        // while a job waits does not throw a second.
        assert!(probe.command_started());
        assert!(!probe.output());
        // The job starts, the probe is undecided: the arm is set back, the
        // next output throws again.
        assert!(probe.begin());
        probe.rearm();
        assert!(probe.output());
        // The job starts, the answer is definitive: the arm stays down, output throws nothing.
        assert!(probe.begin());
        assert!(!probe.output());
        // A new `C` arriving while the definitive answer is being probed is not overwritten.
        assert!(probe.command_started());
        assert!(probe.begin());
        assert!(probe.command_started());
        assert!(probe.begin(), "the new command must be probed");
        // When the arm is down a job that fell into the queue does not probe.
        assert!(!probe.begin());
        // A job that could not be scheduled gives its slot back and keeps the
        // arm: the next edge schedules again.
        assert!(probe.command_started());
        probe.release();
        assert!(probe.output(), "the slot was given back");
    }

    /// The process table's answer settles the identity probe, an unknown one does not — unless the
    /// command's own line already named the program, which the table would only second.
    #[test]
    fn the_identity_probe_rests_on_a_name_and_looks_again_without_one() {
        use super::{Kind, Seen, after_table};
        use bt_shell_common::program::identity::Agent;
        let claude = Kind::Agent(Agent::ClaudeCode);
        assert_eq!(after_table(Seen::Kind(claude), None), (Some(claude), false));
        assert_eq!(
            after_table(Seen::Kind(Kind::Task), Some(Kind::Shell)),
            (Some(Kind::Task), false),
            "the table says what the program is, whatever the line guessed"
        );
        assert_eq!(after_table(Seen::Waiting, None), (None, true));
        assert_eq!(after_table(Seen::Nothing, None), (None, true));
        assert_eq!(
            after_table(Seen::Nothing, Some(Kind::Editor)),
            (None, false),
            "a line that named the program is enough"
        );
        assert_eq!(
            after_table(Seen::Waiting, Some(Kind::Editor)),
            (None, false)
        );
        assert_eq!(
            after_table(Seen::Kind(Kind::Shell), Some(Kind::Agent(Agent::Pi))),
            (None, true),
            "a launcher script before it execs is no answer, and does not hide the line"
        );
    }

    /// What the bateri before showed stands only for the command it was carried with.
    #[test]
    fn a_carried_program_stands_for_the_command_it_came_with_only() {
        use super::{Kind, carried_kind};
        use bt_shell_common::program::identity::Agent;
        let claude = Kind::Agent(Agent::ClaudeCode);
        assert_eq!(carried_kind(Some((7, claude)), Some(7)), Some(claude));
        assert_eq!(
            carried_kind(Some((7, claude)), Some(8)),
            None,
            "a command started after the handover is not the one it ran"
        );
        assert_eq!(carried_kind(Some((7, claude)), None), None);
        assert_eq!(carried_kind::<u32>(None, Some(7)), None);
    }

    /// What a pane ran is handed over for the command that ended only if it was seen during that
    /// one; a command that came and went since leaves the shell.
    #[test]
    fn an_ended_command_keeps_the_program_seen_while_it_ran() {
        use super::{Kind, kind_of_end};
        assert_eq!(kind_of_end(Some((7, Kind::Task)), Some(7)), Kind::Task);
        assert_eq!(
            kind_of_end(Some((6, Kind::Task)), Some(7)),
            Kind::Shell,
            "a command came and went since: the pane is at its prompt"
        );
        assert_eq!(kind_of_end(None, Some(7)), Kind::Shell);
        assert_eq!(kind_of_end(Some((7, Kind::Task)), None), Kind::Shell);
    }

    /// A program that says it is done and the command's own end are two ends of one command: the
    /// program it was seen running stays its, however many ends were noticed together.
    #[test]
    fn two_ends_of_one_command_keep_its_program() {
        use super::{Kind, kind_of_end};
        let ran = Some(("claude -p", Kind::Task));
        for _ends_noticed in [1, 2, 3] {
            assert_eq!(kind_of_end(ran, Some("claude -p")), Kind::Task);
        }
    }

    /// A command nobody names is looked at a bounded number of times, a decided one not at all.
    #[test]
    fn the_identity_probe_gives_up_on_a_program_nobody_names() {
        use super::{IDENTITY_LOOKS, keep_looking};
        assert!(keep_looking(true, 1));
        assert!(keep_looking(true, IDENTITY_LOOKS - 1));
        assert!(!keep_looking(true, IDENTITY_LOOKS));
        assert!(!keep_looking(false, 1), "named: nothing left to look for");
    }

    #[test]
    fn jump_to_latest_counts_its_lines_in_the_singular_and_the_plural() {
        assert_eq!(super::jump_count(1), "1 new line");
        assert_eq!(super::jump_count(2), "2 new lines");
        assert_eq!(super::jump_count(12_345), "12345 new lines");
    }

    #[test]
    fn jump_to_latest_stands_above_the_dock_clear_of_the_track() {
        use objc2_foundation::{NSPoint, NSSize};
        let (pane, size) = (NSSize::new(600.0, 400.0), NSSize::new(180.0, 24.0));
        // The self-hiding forms leave no track; the dock's reserve below.
        assert_eq!(
            super::jump_origin(pane, size, 0.0, 50.0),
            NSPoint::new(600.0 - 20.0 - 180.0, 50.0 + 12.0)
        );
        // "always": further in by the track's width; no dock: above the bottom.
        assert_eq!(
            super::jump_origin(pane, size, 16.0, 0.0),
            NSPoint::new(600.0 - 20.0 - 16.0 - 180.0, 12.0)
        );
        // A pane narrower than the button keeps it inside its left edge.
        let narrow = NSSize::new(150.0, 400.0);
        assert_eq!(
            super::jump_origin(narrow, size, 0.0, 0.0).x,
            super::TIP_MARGIN
        );
    }

    #[test]
    fn a_block_tip_says_only_what_the_ledger_knows() {
        use bt_core::BlockInfo;
        let clock = |started: u32| format!("@{started}");
        let info = BlockInfo {
            command: "make".into(),
            color: [0, 0, 0],
            running: false,
            exit: Some(2),
            duration: Some("3.4s".into()),
            started: Some(7),
            depth: 0,
        };
        assert_eq!(super::block_meta(&info, clock), "exit 2 · 3.4s · @7");
        // Over an update from an older build: no time of day.
        let older = BlockInfo {
            started: None,
            ..info.clone()
        };
        assert_eq!(super::block_meta(&older, clock), "exit 2 · 3.4s");
        let quick = BlockInfo {
            duration: None,
            ..info.clone()
        };
        assert_eq!(super::block_meta(&quick, clock), "exit 2 · @7");
        let running = BlockInfo {
            running: true,
            exit: None,
            duration: None,
            ..info.clone()
        };
        assert_eq!(super::block_meta(&running, clock), "running · since @7");
        let unstamped = BlockInfo {
            started: None,
            ..running
        };
        assert_eq!(super::block_meta(&unstamped, clock), "running");
        // A restored block: its text alone.
        let restored = BlockInfo {
            exit: None,
            duration: None,
            started: None,
            ..info
        };
        assert_eq!(super::block_meta(&restored, clock), "");
    }
}
