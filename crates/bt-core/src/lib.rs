//! bt-core — the platformless core of the terminal model.
//!
//! The VT state machine, grid, scrollback, PTY and reader thread live here;
//! `alacritty_terminal` is **encapsulated**: no alacritty type appears in the
//! `pub` API, the outside sees only `Session`, `Cell`, `UnderlineStyle`, `Cursor`, `Block`,
//! `Blocks`,
//! `SelectionPoint`, `SelectKind`, `CellHalf`, `Arrow`, `Wheel`, `ScrollIntent`,
//! search's `SearchQuery`, `SearchStatus`, `SearchRun`, `SearchRuns`,
//! the scroll bar's `ScrollPosition`, `TrackMark`, `TrackBlock`, `TrackMarks`,
//! the block marks' `BlockHandle`, `BlockInfo` and `BlockReport`,
//! `ScrollGlide`, `LinearRgba`, `Theme`,
//! `ShellState`, `ShellPhase`, the mirror's `DockState`, `DockStatus`, `DockFault`,
//! `Highlight`, `HighlightStyle`, `HighlightColor`, the context line's
//! `DockContext`, the dock surface's `Dock`, the typing animations'
//! `DockEdit`, `EditCells` and the text's column (`DOCK_TEXT_COL`),
//! `Wake` and the settings model's `Settings`, `SettingsEdit`, `Parsed`, `Diagnostic`,
//! `CursorMotion`, `ReduceMotion`, `SmoothScroll`, `ConfirmClose`, `RestoreWindows`,
//! `KeepRunning`, `Scrollbar` and the tables
//! of valid values (`NAMES`, `*_RANGE`), the journal's `Journal`,
//! `JournalStore`, `JournalStream`, `JournalRecords` and `JournalCut` (the full
//! list is the `pub use` block below). `Osc52` is the counterpart of
//! alacritty's type of the same name, not that type itself. The move to our
//! own grid (00X) is done behind this boundary and does not know the renderer.
//! `toml_edit` likewise stays inside: there is no TOML type on the `pub`
//! surface of the settings model and the theme file; the theme's colors are
//! `0xRRGGBB`, not alacritty's `Rgb`.
//!
//! What the boundary carries is a **decision**, not pixels: `Cursor` gives the
//! cursor's position and the color of the text left under its block ("the text
//! under the cursor must stay readable" is a terminal semantic), and the
//! drawing side knows which pixels will take that color — **the decision is
//! here, the painting is there**. The criterion for the split is the cell's
//! divisibility: when the cursor block is between two cells the boundary
//! passes through the middle of a cell and a cell decision made here cannot
//! see it. The command block (`Block`) is the second example of the same rule:
//! line spacing and color cross the boundary, the exit code does not — a
//! branch in the renderer that recognizes escape sequences or exit codes is in
//! the wrong place.
//!
//! Contract: this crate sees no macOS-specific library — no `objc2*`,
//! `core-text`, `metal` — and stays buildable on Linux; the Vulkan gate is
//! built on top of this separation. The Unix PTY (`libc`, `rustix`) is allowed
//! and does not close that gate.
//!
//! The audit is `make audit` (`Makefile`, runs in every `make check`) and is a
//! proxy at the dependency level; the real gate is building with
//! `--target x86_64-unknown-linux-gnu`, closed until `rustup` arrives.

mod block_index;
mod cluster;
mod color;
mod dock;
mod handler;
mod identity;
mod input;
mod journal;
mod link;
mod reader;
mod search;
mod session;
mod settings;
mod shell;
mod snapshot;
mod theme;
mod wake;

pub use cluster::{ClusterId, Clusters};
pub use color::{LinearRgba, Theme, contrast_ratio};
pub use dock::{
    Dock, DockBudget, DockButton, DockCaret, DockCols, DockEdit, EDIT_MAX, EditCells,
    PROGRAM_GLYPHS, STATS_GLYPHS, STATS_THRESHOLDS, StatsLevel, StatsMetric, StatsThreshold,
    TEXT_COL as DOCK_TEXT_COL, UPLOAD_GLYPHS, sign_in_span, stats_at, stats_span,
    transfer_button_at, transfer_button_span,
};
pub use identity::{LC_TERMINAL, TERM_PROGRAM, TERM_PROGRAM_VERSION, TabId};
pub use input::{Arrow, MouseButton, MouseModifiers};
pub use journal::{
    HEADROOM as JOURNAL_HEADROOM, JOURNAL_FORMAT, Journal, JournalCut, JournalRecords,
    JournalStore, JournalStream, Rebuilt, STALL as JOURNAL_STALL, due as journal_due,
    rebuild as journal_rebuild,
};
pub use search::{
    SearchCover, SearchDirection, SearchQuery, SearchReport, SearchRun, SearchRuns, SearchStatus,
    escape as escape_search,
};
pub use session::{
    Activity, AdoptMode, Adoption, Block, BlockHandle, BlockInfo, BlockReport, Blocks, Cell,
    CellHalf, Click, Cursor, DirtyFlag, DockKey, Frozen, InitialInput, LinkHit, LinkHover,
    LinkKind, LinkPoint, LinkSpan, LinkStamp, Osc52, PathCandidate, PtyOps, PtySize,
    SHUTDOWN_GRACE, ScrollGlide, ScrollIntent, ScrollPosition, SelectKind, SelectionPoint,
    SelectionRun, SelectionRuns, Session, SessionOptions, ShutdownHandle, Teardown,
    TerminalOptions, TrackBlock, TrackMark, TrackMarks, UnderlineStyle, Wheel, load_shell,
    smoke_shell,
};
pub use settings::{
    CURSOR_BLINK_INTERVAL, CURSOR_BLINK_RANGE, CURSOR_GLOW, CURSOR_GLOW_RANGE, CURSOR_RADIUS,
    CURSOR_RADIUS_RANGE, CaretShape, CaretStyle, Changes, ConfirmClose, ContentEdge, CursorBlink,
    CursorMotion, Diagnostic, DownloadConflict, Erase, FontOptions, HostMark, HostRule,
    KeepRunning, Keypress, LETTER_SPACING_RANGE, LINE_HEIGHT_RANGE, MAX_LETTER_SPACING,
    MAX_LINE_HEIGHT, MIN_SPACING, MarkSubject, Parsed, PreviewKeep, ReduceMotion, RemoteFiles,
    RemoteStatsMode, RemoteStatsSettings, RestoreWindows, SCROLLBACK_MAX, SIZE_UNITS,
    STATS_INTERVAL_RANGE, SYSTEM_THEME, Scrollbar, Settings, SettingsEdit, ShellIntegration,
    SmoothScroll, UnfocusedCaret, bare_host, expand_home, format_size, parse_size,
};
pub use shell::{
    ButtonState, DockContext, DockFault, DockState, DockStatus, Highlight, HighlightColor,
    HighlightStyle, ProgramBar, ProgramTone, Reconnect, RemoteKind, RemoteSetupFault, RemoteStats,
    RemoteTarget, STATS_HISTORY, ShellPhase, ShellState, SignIn, StatsForm, Transfer,
    TransferAction, TransferControls, TransferTone, TtyModes, counter_period, decode_percent,
    next_tick, running_counter,
};
pub use wake::Wake;

/// The cell is fixed-size and the size is pinned here: **alacritty's** cell
/// (not our [`Cell`] — that one is a frame output, this one is a grid record) =
/// `c` 4 + `fg` 4 + `bg` 4 + `flags` 2 + padding + `Option<Arc<CellExtra>>` 8
/// = 24 bytes.
/// Sparse data (grapheme cluster, underline color, hyperlink) is already in a
/// side table — `CellExtra`. This number is what grows a 10,000-line
/// scrollback per tab.
///
/// Honestly on scope: the measured type is not ours, and this assert blocks no
/// move in this repo — it is a **version canary**: if `cargo update` silently
/// changes the per-cell memory, it breaks the build and hands the decision to a
/// human. When our own cell arrives (00X) the assert moves to it.
const _: () = assert!(size_of::<alacritty_terminal::term::cell::Cell>() == 24);
