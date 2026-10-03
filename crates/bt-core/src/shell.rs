//! The OSC marks the shell prints, the session state they hold, the **command
//! block ledger** and **ZLE's display mirror**.
//!
//! The scanner has **three OSC arms** and all three are fed from the same byte
//! stream: [`MARK_OSC`] carries the session's phase and block identities,
//! [`DOCK_OSC`] the line editor's (ZLE's) current display, [`CWD_OSC`] the working
//! directory. All three sit in one state machine, because the stream is one:
//! separate scanners would frame the same sequence three times and the framing
//! rule (the three points below) would exist in three copies.
//!
//! **The fourth arm is not OSC but CSI** and differs from the others in two
//! ways: the only sequences it recognizes are `CSI 2 J` and `CSI ? 2004 h`, and
//! it **has no payload**. What it gives is not a payload but how many times
//! "clear the screen on purpose" went by ([`Scanner::take_screen_clears`]) and
//! each time a line editor switched
//! bracketed paste on — the remote shell's login signal, an event in stream
//! order rather than a count (047 phase-4, [`ScanEvent::PasteOn`]). It sits in the same
//! state machine because the framing is still one: a scanner stuck in a
//! malformed CSI would swallow the `ESC ] 133;…` that follows, and blocks,
//! suppression and the dock would die **silently**. Until now `ESC [` fell to
//! `Ground`; that arm was harmless only because it did not see the sequence it
//! was looking for, otherwise a `]` inside a CSI could open a new OSC for us.
//!
//! Why the terminal **itself** watches this sequence: alacritty handles
//! `ClearMode::All` on the primary screen with `clear_viewport()`
//! (`term/mod.rs:1794`), that is, it **pushes the visible lines into
//! scrollback**. The screen empties but `history_size()` grows; if the "fill the
//! gap with scrollback" rule (017) cannot tell the two apart, it would undo
//! Ctrl-L.
//!
//! Five responsibilities, one module: extracting marks from bytes
//! ([`parse_mark`]), keeping the session phase from the marks ([`ShellState`]),
//! keeping a fate per block identity and deciding whether the stripe is drawn
//! ([`BlockLog`], [`ShellLog::stripe`]), reducing the mirror's five variables to
//! a decoded record ([`DockState`]) and keeping the dock's context line — the
//! directory and the branch ([`DockContext`]). All five in one place, because the
//! **same** mark stream feeds all five; if they were split, `D`'s exit code
//! would be handed from one module to the other by hand. The ledger's ceiling
//! derives from `scrollback` and the "an unknown identity is not drawn" decision
//! is here too — the only thing the renderer will see is a decoded color.
//!
//! **The mirror's and the context's lifetimes are separate** and this distinction
//! is written into the types: [`DockState`] arrives per keystroke and is reset at
//! `line-finish`, [`DockContext`] arrives per prompt and stays on screen while a
//! command runs.
//!
//! This module is **pure**: it holds no `Session`, no `Wake`, no lock. A byte
//! slice goes in, a mark comes out. That the scanner can request no frame is not
//! a rule but the shape of the type — the place that feeds it (`read()`, the
//! reader thread) runs **before** `advance()`; if it could request a frame, it
//! would draw the old grid and the new state in the same frame.
//!
//! **Why we have our own scanner:** `vte` does not recognize OSC 133 and the
//! `Handler` trait has no "unknown OSC" hook, so even a type wrapping `Term`
//! cannot see this sequence (`.tasks/009-shell-entegrasyonu/context.md` → Kanıt).
//! We scan the bytes on their way to the parser. The same reasoning holds for
//! [`DOCK_OSC`] and [`CWD_OSC`]: `vte` does **not recognize** either, it drops
//! the payload into the `_` arm of `osc_dispatch` and discards it
//! (`vte-0.15.0/src/ansi.rs`, `unhandled`; the numbers it interprets are 0, 2, 4,
//! 8, 10–12, 22, 50, 52, 104 and 110–112). The payload reaches there in full —
//! `osc_raw` is an unbounded `Vec` under `std` and the 1024 `MAX_OSC_RAW` applies
//! only in the `no_std` arm — but is not read where it arrives. For the
//! directory this is not an alacritty event being **dropped**: OSC 7 produces no
//! event, so the empty arm of `Event::Title` could not have stood in for this
//! arm.
//!
//! **The cost, by name:** before dropping the payload, that `_` arm builds a
//! diagnostic string with one `write!` per byte, and because it builds the
//! string **before** `debug!`, the log level does not short-circuit it. So each
//! keystroke's mirror causes one more allocation on the parser side,
//! proportional to the payload length.
//!
//! This is not our defect but alacritty's behavior toward **every** OSC it does
//! not recognize; we only introduced a sequence that often hits that path. There
//! are two escapes: **removing** the sequence from the stream (breaks the
//! scanner's "does not touch the bytes" promise and cannot be done in place
//! because a sequence can cross a chunk boundary) or **changing the carrier to
//! DCS** (`put` neither buffers nor formats the payload; in `discussion.md` →
//! Karar 5 an **unevaluated** alternative, not eliminated like 5b). Both are
//! post-measurement work: the cost is **not on the frame path** but on the reader
//! thread, and the shell-side base64 encoding of the same keystroke is already
//! the dominant term. It stands as a known limit; its measurement is in the debt
//! of R6.2 (per-keystroke cost).
//!
//! **Framing is at parity with `vte`** and this is mandatory: the sequence
//! boundary the scanner sees must be the same as the grid sees, otherwise the
//! two sides read two different stories from the same stream. Three rules from
//! the state table of `vte-0.15.0/src/lib.rs`:
//!
//! - A sequence begins with `ESC ]` (`advance_esc`, `0x5D`) — and bytes can
//!   enter between `ESC` and `]`: `advance_esc` `execute`s C0s other than
//!   0x18/0x1A and **does not change state**, and it does not recognize bytes
//!   above 0x7F at all. So `ESC \r ] 133;A BEL` is a valid mark on the grid.
//! - **Four** bytes end a sequence: `BEL` (0x07), `CAN` (0x18), `SUB` (0x1A) and
//!   a **bare `ESC`** (0x1B). The last is a surprise: `advance_osc_string`, on
//!   seeing ESC, dispatches the sequence **immediately** without waiting for the
//!   `\` of `ESC \`. So `ESC ] 133;A ESC [ 0 m` is a valid mark too, and because
//!   `ESC` takes every state to `Escape`, the "jump to the next ESC" scan is
//!   complete.
//! - C0 control bytes inside the sequence (0x00–0x06, 0x08–0x17, 0x19,
//!   0x1C–0x1F) **do not enter the payload**; `vte` silently drops them. We drop
//!   them too, otherwise a `D;0\r` payload with a pasted line ending would look
//!   corrupt to us.

use std::collections::VecDeque;
use std::fmt::{self, Write as _};
use std::path::Path;
use std::time::{Duration, Instant};

use unicode_width::UnicodeWidthChar;

use crate::dock::{self, DockPoint};
use crate::session::{CellHalf, SelectKind};
use crate::settings::{HostMark, HostRule, RemoteStatsMode};

/// A single OSC 133 mark the shell writes into the stream.
///
/// All four are independent of the shell: neither zsh, nor bash, nor fish
/// appears in the type (R2.4). Adding a new shell is only writing a script.
///
/// **Only two variants carry the identity** (`A` and `D`), because they open
/// and close the block; `B` and `C` sit *inside* the block and repeating the
/// identity would do nothing but add bytes to the stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    /// `A` — the prompt starts here; the block opens here too.
    PromptStart { id: Option<u32> },
    /// `B` — the prompt is done, what follows is the command the user typed.
    PromptEnd,
    /// `C` — the command started running, what follows is output.
    CommandStart,
    /// `D` — the command ended. The code is **optional**: the shell may print `D`
    /// bare, and an unreadable parameter does not refute the fact that the command
    /// ended — "finished but I don't know the code" is the right answer. The
    /// identity is optional too, for the same reason: an identity-less `D` still
    /// advances the state, it just has nowhere to write into the ledger.
    CommandEnd { exit: Option<i32>, id: Option<u32> },
}

/// Which remote shell a remote mark comes from: `P`, the local block of the
/// `ssh` command that opened the connection, and `S`, the remote shell's own
/// process id (048 phase-3).
///
/// **`P` alone is not enough**: one command line can open two connections
/// (`ssh a; ssh b`, a `for` loop) and both servers count from one — under one
/// `P` the second session's `rblock/P.1` would reopen the first's and repaint
/// its rows. The pid tells the two shells apart; `P` stays the gate of the
/// remote clock ([`ShellLog::running_blocks`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RemoteShell {
    pub(crate) parent: u32,
    pub(crate) pid: u32,
}

/// A mark of **our remote shell** (048 phase-3): any of the four letters
/// carrying `bt_remote=<P>.<S>.<n>` — [`RemoteShell`] and `n`, the remote
/// shell's own counter.
///
/// **A separate type, not a [`Mark`] with one more field**: the local gates
/// (`ours`, `command_open`, the held `line-finish`, the remote state, the
/// three [`ScanOutcome`] bits) must never see it, and a value the local
/// [`ShellLog::apply`] cannot receive is the guarantee — no condition in it to
/// forget. Its ledger is [`ShellLog::remote`].
///
/// **The field is on all four letters**, unlike `bt_block=` (only `A`/`D`): an
/// identity-less `B`/`C` from the server would drive the local phase in the
/// window before the remote probe has set the remote state (the local gate
/// fires only while it is set).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RemoteMark {
    pub(crate) shell: RemoteShell,
    /// `n`: the remote block.
    pub(crate) id: u32,
    /// The letter and, on `D`, the code; its own identities are `None`.
    pub(crate) mark: Mark,
}

/// Which ledger a block anchor points to: `bateri://block/<n>` (the local
/// shell, [`ShellLog::local`]) or `bateri://rblock/<P>.<S>.<n>` (our remote
/// shell, [`ShellLog::remote`]).
///
/// The namespaces are **separate** because the two counters are: the remote
/// shell counts from one like the local one, and `rblock/1` read as `block/1`
/// would paint a remote row with a local command's colour. The shell is part
/// of the key for the same reason one level up: two ssh sessions' `rblock/1`s
/// are two blocks.
///
/// A third namespace is no shell's: `bateri://sblock/<k>.<role>`, a block of
/// a **restored** history ([`crate::snapshot`]). Its colour travels in the
/// key, because no ledger of this session knows it — and the new shell
/// counts from one, so the old `block/N` would paint the wrong block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BlockKey {
    Local(u32),
    Remote {
        shell: RemoteShell,
        id: u32,
    },
    /// `id` is unique within one saved history only; `stripe` is never
    /// [`Stripe::Running`] (the snapshot writes only finished blocks).
    Saved {
        id: u32,
        stripe: Stripe,
    },
}

/// The running block of each ledger — the frame path's `running` argument
/// ([`ShellLog::running_blocks`]), read once per frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RunningBlocks {
    pub(crate) local: Option<u32>,
    /// The remote block running under the open local `ssh` block.
    pub(crate) remote: Option<(RemoteShell, u32)>,
}

impl RunningBlocks {
    /// Whether `key` is the running block of its ledger — the **single**
    /// answer [`ShellLog::stripe`], [`ShellLog::duration`] and the frame's
    /// clock read.
    pub(crate) fn is(self, key: BlockKey) -> bool {
        match key {
            BlockKey::Local(id) => self.local == Some(id),
            BlockKey::Remote { shell, id } => self.remote == Some((shell, id)),
            BlockKey::Saved { .. } => false,
        }
    }
}

/// Everything the session knows about the shell.
///
/// `Copy` and small: [`crate::Session::shell_state`] hands it out by copying
/// from under the lock (the precedent is [`crate::Session::theme`]).
///
/// **There is no level `enum`.** The product-language "level 0" (no integration)
/// is, in code, the **absence** of this type (`Option<ShellState>`); if a
/// separate record were kept, the two would contradict each other on the far
/// side of SSH — integration installed locally but no marks arriving from the
/// remote (`discussion.md` → Karar 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellState {
    /// What the shell is doing right now.
    pub phase: ShellPhase,
    /// The exit code of the most recently finished command; `None` if no command has
    /// finished yet or if the shell printed the code in an unreadable form.
    pub last_exit: Option<i32>,
}

/// The shell's current phase — one for each of the four marks.
///
/// `A` and `B` are **not merged**: the boundary between "the prompt is being
/// drawn" and "the user is typing" is the first question of the Input Dock (012).
/// Keeping the distinction here is free, winning it back later is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellPhase {
    /// `A` — the prompt is being drawn.
    Prompt,
    /// `B` — the prompt is done, the user is typing their command.
    Input,
    /// `C` — the command is running, its output is flowing onto the screen.
    Running,
    /// `D` — the command finished, the new prompt has not arrived yet. The code is in `last_exit`.
    Finished,
}

/// [`ShellLog::history_cut`]'s answer: where the quit-time snapshot stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HistoryCut {
    /// The shell is at `Input`: before the top row of this block's anchor.
    Anchor(u32),
    /// No shell state at all: before the cursor's row.
    BeforeCursor,
    /// A command is running or between commands: through the cursor's row.
    ThroughCursor,
}

/// A block's fate — the raw record the ledger keeps.
///
/// `Pending` does **not mean** "running": an Enter pressed on an empty prompt
/// also produces `A`, but since no command ran, `D` never arrives. The information
/// that tells the two apart is in [`ShellState::phase`] and the function that
/// makes the distinction is [`ShellLog::stripe`]; the ledger only records what it
/// sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// `A` arrived, `D` did not.
    Pending,
    /// `D` arrived; `exit` is `None` if the shell printed the code in an unreadable form.
    Finished {
        exit: Option<i32>,
        /// The time between `C` and `D`, in milliseconds.
        ///
        /// **Inside** the block, not in a side table: it has the same lifetime as the
        /// fate and the ring's eviction drops both together. The `u32` ceiling is
        /// 49 days; the counter of a command that runs longer saturates, it does not
        /// wrap.
        ///
        /// Zero for a block that never saw the duration: if `D` arrives without `C`
        /// (a `D` after an identity-less `A`, or half an integration), instead of
        /// writing a made-up duration it falls below the threshold, that is, the
        /// counter is not drawn.
        ///
        /// **Known limit: what is measured is not the command itself but the span
        /// between `C` and `D`.** Our hooks that print the two marks are appended
        /// **last** with `add-zsh-hook` (their reasons are in `bateri.zsh`: the anchor's
        /// closing in `preexec`, the `psvar` slot in `precmd`), so the user's own hooks
        /// run before both. The result goes both ways and partly cancels out: `C` is
        /// printed late (the duration shrinks), `D` is printed after the user's
        /// `precmd`s (the duration grows). The margin is the duration of the hooks —
        /// tens of milliseconds in a setup like starship that runs a binary per prompt.
        ///
        /// **Our own work is not inside the margin:** `D` is `precmd`'s first job —
        /// **before** the branch's `git` fork, OSC 7 and `psvar`.
        elapsed_ms: u32,
    },
}

/// The ledger's per-entry budget — the **verified** form of the number in
/// [`BlockLog`]'s doc.
///
/// Rust uses the niche in `Option<i32>`'s tag for [`Outcome`]'s discrimination,
/// so the size is not a number that can be added up by hand (by hand it comes to
/// 16 and that is how it was written the first time): adding a field to
/// `Finished` silently grows 12, and with it the memory paid per tab in a
/// 10,000-line scrollback. When the assert breaks, both this place and
/// `BlockLog`'s budget sentence are updated in the same commit.
const _: () = assert!(size_of::<Outcome>() == 12);

/// A block's **drawable** state; it descends to a color from the theme in
/// [`crate::Session::frame`].
///
/// Three values, because three colors are drawn today. The name of the undrawn
/// state is not in this enum but in the `None` of the function that produces it:
/// "not drawn in any unknown state" is not a color choice, it is a drawing
/// decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stripe {
    /// The command is running — its color is the theme's `accent`.
    Running,
    /// Finished with a zero exit code.
    Success,
    /// Finished with a nonzero exit code.
    Error,
}

/// ZLE's display mirror — everything the dock will draw, **decoded**.
///
/// Five display variables are carried ([`DOCK_OSC`]'s payload; alongside them
/// `KEYMAP` and, since 032, `PREBUFFER`) and here they descend to strings, a
/// column and a list of ranges. If only `BUFFER` were carried, suppression would
/// turn into information loss: `POSTDISPLAY` is the autosuggestions suggestion,
/// `region_highlight` is syntax highlighting's color — the two most common
/// plugins, and without them the dock would show the user **less** than they
/// see (`discussion.md` → Karar 8b).
///
/// **It is a reused buffer, not a record.** The scanner refreshes its own copy in
/// place on every keystroke, [`ShellLog`] takes it under the lock with
/// [`Clone::clone_from`] and [`crate::Session::dock_state`] hands it out again
/// with `clone_from`; in all three steps the strings keep their capacities with
/// `clear()` + `push_str`. In steady state there are **zero** allocations per
/// keystroke — the criterion is `CLAUDE.md`'s per-frame cost rule and this type
/// is read every frame.
///
/// `Clone` is written by hand: `derive` produces only `clone` and the default
/// `clone_from` is "`*self = source.clone()`", so it would silently lose this
/// type's one important property — reusing capacity.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DockState {
    /// Whether the dock is drawable and, if not, why.
    pub status: DockStatus,
    /// `PREDISPLAY` — the non-editable text ZLE puts **before** the line.
    pub predisplay: String,
    /// `BUFFER` — the text the user typed, editable.
    pub buffer: String,
    /// `POSTDISPLAY` — the non-editable text appended **after** the line;
    /// today's only producer is zsh-autosuggestions' suggestion.
    pub postdisplay: String,
    /// `PREBUFFER` — the earlier lines of a multi-line command that ZLE has
    /// **accepted** (`for`, heredoc, `\`-continuation); always ends with `\n` and
    /// is no longer editable. The mirror's seventh, **optional** body (032); empty
    /// with an old script.
    ///
    /// **Outside the display space:** [`Self::cursor`], [`Self::display_chars`],
    /// [`Self::last_ink`] and `region_highlight` do not count it — zsh's
    /// spaces do not count it either. The dock draws it **above** the editable
    /// lines, selectable and copyable but read-only (the flow of
    /// [`crate::dock::dock_layout`] is `PREBUFFER ++ display`); its being filled
    /// lowers suppression's upper floor to the anchor's row
    /// ([`SuppressedInput::from_anchor`]).
    pub prebuffer: String,
    /// The caret's **character** offset, **in the same space** as
    /// [`Highlight::start`]: counted from the start of the string
    /// `PREDISPLAY ++ BUFFER ++ POSTDISPLAY`.
    ///
    /// zsh's `$CURSOR` counts from the start of `BUFFER`; the shift is done on this
    /// side of the boundary so that the two offset fields stay in one space. If they
    /// stood in separate spaces, the moment the drawing side forgot to shift one by
    /// the `PREDISPLAY` length it would shift the caret by the prompt's length — and
    /// since `PREDISPLAY` is non-empty, this would happen on **every line**.
    ///
    /// Characters, not bytes: the dock's question is "which cell do I draw at".
    pub cursor: usize,
    /// `region_highlight` — the display's colored ranges, **normalized** into the
    /// single space in [`Highlight::start`]'s doc.
    pub highlights: Vec<Highlight>,
    /// The character count of `PREDISPLAY ++ BUFFER ++ POSTDISPLAY` — the length of
    /// the same space as [`Self::cursor`].
    ///
    /// **Stored because it was already counted:** the decoder computes all three
    /// lengths to clamp the offsets. Its consumer is [`ShellLog::suppressed_input`]
    /// and from there `Session::frame` — the suppressed range's **lower** end
    /// derives from this. Recounting per frame would add an O(n) walk to `frame()`
    /// before the `Term` lock.
    pub display_chars: usize,
    /// The last non-blank character of the display's **last line**; `None` if that
    /// line is blank (also after `echo a\n`).
    ///
    /// Suppression's **freshness gate** uses this: it is compared with the last
    /// inked cell of the **last** line of the grid's last input line, and on a
    /// mismatch the mirror is considered stale and suppression is dropped. Blanks
    /// are **excluded**, because a blank cell never crosses the boundary (`frame()`'s
    /// skip gate) and for a user typing `ls ` it would give a false alarm every
    /// frame.
    ///
    /// It is stored here because the decoder already has the text in hand;
    /// rescanning per frame would add O(n) before the `Term` lock.
    pub last_ink: Option<char>,
    /// Whether ZLE is in the **insert** keymap: whether a pressed printable key
    /// becomes text ([`INSERT_KEYMAPS`]).
    ///
    /// Its only consumer is the paste's narrow exception
    /// ([`crate::Session::can_be_typed`]) and it is mandatory there: the whole
    /// justification of the exception is "if the user typed this text by hand it
    /// would give the same result" and that sentence is true only in the insert
    /// keymap. In `vicmd` the same bytes are commands — `dd` on the clipboard
    /// deletes a line.
    ///
    /// **A `bool`, not a name:** it crosses the boundary decoded (the same rule as
    /// the rest of `DockState`) and storing the name would mean holding one more
    /// `String` per frame. The classification is at decode time, in one place.
    ///
    /// The default is `false` and this is the **safe direction**: a window running
    /// with an old script that never sends the field (`plan.md` → Göç) loses the
    /// exception, that is, goes back to the wrapped paste — the behavior before
    /// phase-5.
    pub insert_keymap: bool,
    /// The user input this mirror **answers**: the input generation read at the
    /// moment the mirror was decoded (`Session`'s `key_gen`).
    ///
    /// The freshness gate's temporal half: if the generation has not advanced since
    /// then, the mirror of the user's last input has arrived and there is no need to
    /// look at what the grid says. It sits **next to the content**, not in a free
    /// flag: the frame path reads it in the same leaf-lock turn as the text, so a
    /// stale read brings the stale stamp with it and the gate falls back to content
    /// comparison — the wrong direction is safe
    /// (`.tasks/025-tazelik-zamansal/discussion.md` → Muhakeme).
    ///
    /// The only writer is [`ShellLog::apply_scan_answering`]; in the copy the
    /// scanner stages it is meaningless and zero.
    ///
    /// **The `Idle` mirror is stamped too** (`End` arm, 030): the dock's typing
    /// animations ([`crate::DockEdit`]) bound the number of glyphs to animate by the
    /// difference of this stamp, and the base of the first keystroke after Enter is
    /// the `Idle` mirror. A base with a zero stamp would defeat that bound, and the
    /// first paste at the prompt would animate letter by letter. The freshness gate
    /// never reads `Idle` ([`ShellLog::suppressed_input`] requires `Live`), so it has
    /// no effect on it.
    pub answers: u64,
    /// Whether the mirror is read by cluster (035, `SessionOptions::cluster`): the
    /// dock's layout ([`crate::dock::layout_with`]) and [`Self::last_ink`] count an
    /// emoji sequence as one cluster.
    ///
    /// **Not the mirror's content but its reading**, and constant for the session: the
    /// scanner writes it into its own copy at startup ([`Scanner::cluster`]),
    /// `clone_from` carries it, [`Self::reset`] does not touch it. The reason it sits
    /// here is that all consumers already hold this record — the dock's drawing, hit
    /// testing, line count and the freshness gate; a separate argument would add a
    /// parameter to all of those signatures.
    pub cluster: bool,
}

impl Clone for DockState {
    fn clone(&self) -> Self {
        let mut fresh = Self::default();
        fresh.clone_from(self);
        fresh
    }

    fn clone_from(&mut self, source: &Self) {
        self.status = source.status;
        self.predisplay.clear();
        self.predisplay.push_str(&source.predisplay);
        self.buffer.clear();
        self.buffer.push_str(&source.buffer);
        self.postdisplay.clear();
        self.postdisplay.push_str(&source.postdisplay);
        self.prebuffer.clear();
        self.prebuffer.push_str(&source.prebuffer);
        self.cursor = source.cursor;
        self.display_chars = source.display_chars;
        self.last_ink = source.last_ink;
        self.insert_keymap = source.insert_keymap;
        self.answers = source.answers;
        self.cluster = source.cluster;
        self.highlights.clear();
        self.highlights.extend_from_slice(&source.highlights);
    }
}

impl DockState {
    /// Empties the text and the ranges; the capacities stay.
    ///
    /// The state is written by the **caller**: not leaving stale text is the shared
    /// job of both callers (`End`, `Unavailable`), which state to move to is not.
    fn reset(&mut self) {
        self.predisplay.clear();
        self.buffer.clear();
        self.postdisplay.clear();
        self.prebuffer.clear();
        self.cursor = 0;
        self.display_chars = 0;
        self.last_ink = None;
        // Safe direction: the keymap of a line we cannot show is unknown too, and
        // "I don't know" must send the paste to the wrapped path.
        self.insert_keymap = false;
        self.answers = 0;
        self.highlights.clear();
    }
}

/// The dock's **context line**: the working directory and the git branch.
///
/// A **separate type** from [`DockState`] and this separation is mandatory, not a
/// layout preference: the mirror arrives per keystroke and is reset at
/// `line-finish` ([`DockState::reset`]), while the context arrives **per prompt**
/// and has to stay on screen while a command runs. If they were in one type,
/// every reset of the mirror would erase the context too — the directory would
/// vanish the moment the user pressed Enter. Also, the mirror's record is
/// refreshed wholesale in the scanner with `clone_from` and the directory comes
/// from **another arm** (OSC 7): in one type every mirror update would overwrite
/// the directory.
///
/// The same buffer discipline as [`DockState`]: `clone_from` keeps capacities,
/// so there is no per-frame allocation.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DockContext {
    /// The shell's working directory, the **full path**; it comes from OSC 7 and is
    /// empty if it never arrived.
    pub cwd: String,
    /// The git branch; **empty** if not a repository or unreadable. On a detached
    /// HEAD the short SHA instead of the branch — the shell does not say which it
    /// is, it only sends the name to show.
    pub branch: String,
    /// The remote session's target (037 Karar 1: host, kind, argv to re-run and its
    /// line); `None` when there is no remote session (036).
    ///
    /// Its writer is `bt-shell`'s process-table probe
    /// ([`crate::Session::set_remote`]); it is cleared automatically on `C`, `D` and
    /// `A` ([`ShellLog::apply`]). It is inside the context because the frame path
    /// takes the context under the lock with `clone_from` and does the drawing
    /// after the lock: if it were `ShellLog`'s own field, it would take either a
    /// `String` allocation per frame or holding the lock through the drawing.
    pub remote: Option<RemoteTarget>,
    /// The **resolved** mark of the active remote host (037 Karar 2); meaningful only
    /// while [`Self::remote`] is filled, [`HostMark::None`] locally.
    ///
    /// The pattern is not here but in `ShellLog`, and resolution happens at two edges
    /// (a change of the remote state and of the list): the frame path sees no
    /// pattern, it only reads this.
    pub remote_mark: HostMark,
    /// The remote side's OSC 7 directory (036 Karar 4); empty if it did not arrive.
    /// **Read only while [`Self::remote`] is filled** — it is also written while
    /// inactive (an OSC 7 with a foreign authority), so that the probe can conclude
    /// after OSC 7.
    pub remote_cwd: String,
    /// The reconnect offer of a dropped ssh (037 Karar 8); `None` otherwise.
    ///
    /// Set up in a single arm: the remote session is active, the kind is ssh and
    /// **our** identified `D` carries 255 ([`ShellLog::apply`]) — the target moves
    /// here in the same place where the remote state is deleted. Three edges clear
    /// it: the next `Running` transition, a new remote target
    /// ([`ShellLog::set_remote`]) and any user input
    /// ([`crate::Session::send_input`]). `A` does not clear it: the offer is born
    /// exactly at that prompt.
    ///
    /// Inside the context, for the same reason as [`Self::remote`]: the dock's
    /// placeholder takes it in the frame path in the same lock turn as the context.
    pub reconnect: Option<Reconnect>,
    /// The status line of an upload to a remote directory (037 Karar 7 → Kullanıcı
    /// kararı 4); `None` if there is no upload.
    ///
    /// Its writer is `bt-shell`'s upload queue ([`crate::Session::set_transfer`]);
    /// neither `C`/`D`/`A` nor input clears it — the queue has its own lifetime and
    /// when it finishes the line shows the result for a while and goes away. It is
    /// **separate** from the remote state, because it has to be visible when ssh has
    /// closed too ("connection closed"): it carries the host and the mark itself.
    pub transfer: Option<Transfer>,
    /// The remote host's load indicator (046 Karar 5); `None` while there is no
    /// sample, on an error and with `stats = "off"`.
    ///
    /// Its writer is `bt-shell`'s sampler ([`crate::Session::set_remote_stats`],
    /// generation and equality gated). It belongs to the remote state and goes
    /// with it: `C`/`D`/`A` and a new remote target clear it
    /// ([`Self::clear_remote`], [`ShellLog::set_remote`]) — otherwise a new host
    /// would show the previous one's numbers until its first sample.
    pub stats: Option<RemoteStats>,
    /// The ssh status bar's **Sign In…** button (047 R7.2): a background job
    /// (the link check, the load indicator) could not log in by itself — no
    /// saved password, or the saved one was refused. `None` otherwise.
    ///
    /// Its writer is `bt-shell`'s pane ([`crate::Session::set_sign_in`]); it
    /// belongs to the remote state and goes with it, like [`Self::stats`]. While
    /// it is shown the load indicator is not (there is no sample without a
    /// login) and the upload row wins over both.
    pub sign_in: Option<SignIn>,
    /// Why bateri's remote bootstrap fell back to a plain login shell (048
    /// R3.2, R3.4): the shell integration did not start on the server, so the
    /// remote folder stays unknown and the pane's label says why
    /// ([`crate::Session::remote_setup_fault`]). `None` otherwise. It comes from
    /// the stream (`8133;f`, [`RemoteSetupFault::from_code`]) and belongs to the
    /// remote state like [`Self::remote_cwd`]: written whether or not the probe
    /// has landed, cleared with it.
    pub remote_setup: Option<RemoteSetupFault>,
}

/// Why the remote bootstrap (048, `assets/shell/remote/`) did not start the
/// shell integration — the bootstrap's fixed codes, never the server's text
/// (`ESC ] 8133 ; f ; {code} BEL`, [`parse_dock`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteSetupFault {
    /// `write`: the files could not be written under
    /// `~/.local/share/bateri/shell/` (a read-only home, a full disk).
    Write,
    /// `decode`: the server has no base64 decoder the bootstrap knows.
    Decode,
    /// `shell`: the login shell is not zsh, bash or fish.
    Shell,
}

impl RemoteSetupFault {
    /// The wire code; [`Self::from_code`]'s inverse.
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Decode => "decode",
            Self::Shell => "shell",
        }
    }

    /// The wire code → the fault; an unknown code (a newer bootstrap) is `None`.
    pub fn from_code(code: &[u8]) -> Option<Self> {
        match code {
            b"write" => Some(Self::Write),
            b"decode" => Some(Self::Decode),
            b"shell" => Some(Self::Shell),
            _ => None,
        }
    }
}

/// The Sign In… button's drawing state ([`DockContext::sign_in`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SignIn {
    /// Under the mouse: the fill darkens (the upload buttons' rule).
    pub hover: bool,
}

/// The form of the load indicator that is drawn (046 Karar 4) —
/// [`RemoteStatsMode`] without `Off`: with `Off` there is no value at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatsForm {
    /// `cpu ▂▃▅▇▅▃▂▁ 23%  mem 61%`.
    #[default]
    Sparkline,
    /// `cpu 23%  mem 61%`.
    Numbers,
    /// `●`, or only the values past their threshold.
    Alerts,
}

impl RemoteStatsMode {
    /// The drawn form; `None` for `Off`.
    pub fn form(self) -> Option<StatsForm> {
        match self {
            Self::Sparkline => Some(StatsForm::Sparkline),
            Self::Numbers => Some(StatsForm::Numbers),
            Self::Alerts => Some(StatsForm::Alerts),
            Self::Off => None,
        }
    }
}

/// The number of CPU samples the sparkline shows (046 Karar 4).
pub const STATS_HISTORY: usize = 8;

/// One sample of the remote host's load, as the context row draws it (046
/// Karar 5): `Copy` and fixed-size, so the frame path's context copy allocates
/// nothing for it.
///
/// The percentages are **rounded** by the writer (`bt-shell-common`'s sampler):
/// the shown value is the compared value, so a change below a percent asks for
/// no frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct RemoteStats {
    pub form: StatsForm,
    /// CPU %, `0..=100`; `None` on the first sample — CPU is the difference of
    /// two counter readings.
    pub cpu: Option<u8>,
    /// Memory in use, %.
    pub mem: u8,
    /// The root file system's use, %.
    pub disk: u8,
    /// The sparkline's levels (`0..=7`, U+2581 + level), **oldest first**; only
    /// the first [`Self::len`] are meaningful. Filled only in
    /// [`StatsForm::Sparkline`]: an invisible history change must not ask for
    /// a frame.
    pub history: [u8; STATS_HISTORY],
    /// How many of [`Self::history`] are filled.
    pub len: u8,
}

impl RemoteStats {
    /// The meaningful part of the history, oldest first.
    pub fn history(&self) -> &[u8] {
        &self.history[..usize::from(self.len).min(STATS_HISTORY)]
    }
}

/// Equality is over what is **drawn**: the history's slots past [`RemoteStats::len`]
/// do not take part — a derived comparison would let a stale byte there
/// request a frame that changes nothing (the equality gate, 046 R3.5).
impl PartialEq for RemoteStats {
    fn eq(&self, other: &Self) -> bool {
        self.form == other.form
            && self.cpu == other.cpu
            && self.mem == other.mem
            && self.disk == other.disk
            && self.history() == other.history()
    }
}

impl Eq for RemoteStats {}

/// The dock's status line for the upload queue (037 Karar 7 → Kullanıcı kararı
/// 4): in place of the context line `⇄ {host}  {body}{controls}` and progress on
/// the top hairline.
///
/// The body's text is formatted in `bt-shell` (bytes, speed, time, file count);
/// the buttons' labels, on the other hand, are born in this crate from the state
/// here ([`TransferControls`]), because the label's length is an input to the
/// layout. This crate draws the line and **clips** it — if `body` does not fit it
/// is shortened with `…`, the buttons are not shortened and if they do not fit
/// they drop in order (half a button cannot be clicked). Which column is which
/// button is told by [`crate::transfer_button_at`]; drawing and the mouse read
/// the same layout (037 phase-6).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Transfer {
    /// The target's host, as shown ([`RemoteTarget::host`]).
    pub host: String,
    /// The host's resolved mark: the color of `⇄ host` and of the progress bar.
    pub mark: HostMark,
    /// Durum metni (`↑ 1 of 2 · backup.tar.gz  18.2 / 44.6 MB · …`).
    pub body: String,
    /// The tone of the [`Self::lead`] characters at the start of the body; the rest is
    /// dim (037 phase-7): the result line carries the result's color — success
    /// `success`, error text `error`, cancel and progress dim.
    pub tone: TransferTone,
    /// The number of leading characters drawn in [`Self::tone`]: in a failure line
    /// `Failed — {reason}` is red, the ` · k of n uploaded` after it is dim.
    pub lead: usize,
    /// The state of the buttons at the right of the line; no buttons if the item
    /// count is zero (result line).
    pub controls: TransferControls,
    /// Progress by the bytes of the whole queue, in **ten-thousandths**
    /// (`0..=10_000`); `None` → no bar (result line). An integer, because the
    /// context is `Eq` and the frame path compares it; at ten-thousandths it is under
    /// half a pixel in a 4K window.
    pub progress: Option<u16>,
}

impl Clone for Transfer {
    fn clone(&self) -> Self {
        let mut fresh = Self::default();
        fresh.clone_from(self);
        fresh
    }

    /// The reason for [`RemoteTarget::clone_from`]: the frame path copies the context
    /// every frame, so the strings' capacity must be kept.
    fn clone_from(&mut self, source: &Self) {
        self.host.clone_from(&source.host);
        self.mark = source.mark;
        self.body.clone_from(&source.body);
        self.tone = source.tone;
        self.lead = source.lead;
        self.controls = source.controls;
        self.progress = source.progress;
    }
}

/// The leading tone of the upload line's body ([`Transfer::tone`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TransferTone {
    /// Dim (`dim`): progress and cancel.
    #[default]
    Quiet,
    /// The theme's `success`: the queue finished.
    Success,
    /// The theme's `error`: the queue finished with an error.
    Error,
}

/// The state of the upload line's buttons (037 phase-6): the labels and the
/// number of buttons are born from this ([`crate::dock`]'s layout).
///
/// The button under the mouse and the list's openness are **here**, with the
/// line: the line is rewritten on every refresh and if the mouse state lived on a
/// separate path, either that path or the refresh would overwrite the other.
/// Its change goes through [`crate::Session::set_transfer`]'s equality gate, so a
/// frame is requested only when the state changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransferControls {
    /// The number of items visible in the list — finished, flowing and waiting (037
    /// phase-7). `0` → no buttons, `1` → only `Cancel`, more → `Show transfers (N)` +
    /// `Cancel all`.
    pub items: u16,
    /// The list (popover) is open: the list button is in the pressed tone; its label does not change.
    pub list_open: bool,
    /// The button under the mouse.
    pub hover: Option<TransferAction>,
}

/// The job of one of the upload line's buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferAction {
    /// Open the queue's list.
    List,
    /// Cancel the whole queue.
    Cancel,
}

/// A button's state in drawing: the tone of the fill and the border is `bt-gpu`'s
/// decision (alpha is a drawing state, not the palette's).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonState {
    #[default]
    Idle,
    /// Under the mouse.
    Hover,
    /// Pressed — today only the open list's button.
    Pressed,
}

/// The reconnect offer (037 Karar 8): the placeholder's host and mark, the line
/// ⏎ will send.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reconnect {
    /// The dropped target's host, as shown ([`RemoteTarget::host`]).
    pub host: String,
    /// The host's **resolved** mark; re-resolved when the list changes
    /// ([`ShellLog::set_host_rules`]).
    pub mark: HostMark,
    /// The escaped line to re-run ([`RemoteTarget::line`]).
    pub line: String,
}

impl Clone for Reconnect {
    fn clone(&self) -> Self {
        let mut fresh = Self::default();
        fresh.clone_from(self);
        fresh
    }

    /// The reason for [`RemoteTarget::clone_from`]: the frame path copies the context
    /// every frame, so the strings' capacity must be kept.
    fn clone_from(&mut self, source: &Self) {
        self.host.clone_from(&source.host);
        self.mark = source.mark;
        self.line.clone_from(&source.line);
    }
}

impl Clone for DockContext {
    fn clone(&self) -> Self {
        let mut fresh = Self::default();
        fresh.clone_from(self);
        fresh
    }

    fn clone_from(&mut self, source: &Self) {
        self.cwd.clear();
        self.cwd.push_str(&source.cwd);
        self.branch.clear();
        self.branch.push_str(&source.branch);
        // Capacity is kept: allocation happens only at the **edge** of the remote session
        // (`Option::clone_from` descends to `RemoteTarget::clone_from` on `Some`/`Some`).
        self.remote.clone_from(&source.remote);
        self.remote_mark = source.remote_mark;
        self.remote_cwd.clear();
        self.remote_cwd.push_str(&source.remote_cwd);
        self.reconnect.clone_from(&source.reconnect);
        self.transfer.clone_from(&source.transfer);
        self.stats = source.stats;
        self.sign_in = source.sign_in;
        self.remote_setup = source.remote_setup;
    }
}

impl DockContext {
    /// Uzak oturumun host'u; yerelde `None`.
    pub fn remote_host(&self) -> Option<&str> {
        self.remote.as_ref().map(|target| target.host.as_str())
    }

    /// Deletes the remote state; `true` **if the title's input changed** (there was a
    /// host). The remote slot goes too: it is not the next session's directory.
    fn clear_remote(&mut self) -> bool {
        self.remote_cwd.clear();
        self.remote_setup = None;
        self.remote_mark = HostMark::None;
        // The load belongs to the host (046 Karar 5), and so does its login.
        self.stats = None;
        self.sign_in = None;
        self.remote.take().is_some()
    }
}

/// The two terminal modes of the PTY that say whether the remote session is
/// past its login (047 R9.1): read from the master with `tcgetattr` by the
/// platform shell (`bt-shell-common::jobs::tty_modes`; `bt-core` has no
/// `libc`), decided here.
///
/// Measured (macOS, the master's `tcgetattr` gives the slave's flags): ssh's
/// host key question is canonical with echo, its password prompt canonical
/// without echo, the logged-in session neither (ssh's raw mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TtyModes {
    /// `ICANON`.
    pub canonical: bool,
    /// `ECHO`.
    pub echo: bool,
}

impl TtyModes {
    /// Whether these modes are a logged-in session's: neither canonical nor
    /// echoing. A question (host key, password, a passphrase) is canonical.
    pub fn logged_in(self) -> bool {
        !self.canonical && !self.echo
    }
}

/// The remote session's kind (037 Karar 1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RemoteKind {
    #[default]
    Ssh,
    Mosh,
}

/// The remote session's target — what the probe found, as a whole (037 Karar 1).
///
/// ⌘T and reconnect **re-run** the same command: the host alone is not enough
/// (without port, `-i`, `-J` a second connection cannot be made). `bt-core` sees
/// no pid or `libc`, what is carried is a string; the escaping rule is
/// `bt-shell`'s too (`quote`), only its result ([`Self::line`]) sits here.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RemoteTarget {
    /// As the user typed it (`prod`, `deploy@10.0.0.5`); the `ssh://` scheme and the
    /// port are dropped (036 Karar 3).
    pub host: String,
    pub kind: RemoteKind,
    /// The argv to re-run — for ssh the local forwardings (`-L -R -D`), `-M` and `-f`
    /// are stripped; for mosh `mosh` + the script's arguments.
    pub argv: Vec<String>,
    /// [`Self::argv`] as a readable line, escaped for the shell.
    pub line: String,
}

#[cfg(test)]
impl RemoteTarget {
    /// The tests' target: `ssh {host}`.
    pub(crate) fn ssh(host: &str) -> Self {
        Self {
            host: host.to_owned(),
            kind: RemoteKind::Ssh,
            argv: vec!["ssh".to_owned(), host.to_owned()],
            line: format!("ssh {host}"),
        }
    }
}

impl Clone for RemoteTarget {
    fn clone(&self) -> Self {
        let mut fresh = Self::default();
        fresh.clone_from(self);
        fresh
    }

    /// The frame path takes the context every frame with `clone_from`: the default of
    /// a derived `Clone` (`*self = source.clone()`) would reallocate the argv and two
    /// strings every frame. `Vec<String>::clone_from` keeps the elements' capacity.
    fn clone_from(&mut self, source: &Self) {
        self.host.clone_from(&source.host);
        self.kind = source.kind;
        self.argv.clone_from(&source.argv);
        self.line.clone_from(&source.line);
    }
}

/// The remote shell's directory as its **title** says it, the fallback when it
/// sends no OSC 7 (045, user decision 2026-10-02): Debian's and Ubuntu's stock
/// `.bashrc` sets the title to `user@host: dir` at every prompt, so the title
/// follows `cd`; oh-my-zsh's `termsupport` sets `user@host:dir` (`%n@%m:%~`, no
/// space), so the space after the colon is optional. Only that shape counts and
/// the directory must be absolute or `~`-rooted; anything else (vim's title, a
/// free-form one) is `None` and the remote folder stays unknown.
pub(crate) fn title_directory(title: &str) -> Option<&str> {
    let (who, dir) = title.split_once(':')?;
    let dir = dir.strip_prefix(' ').unwrap_or(dir);
    let (user, host) = who.split_once('@')?;
    let plain = |part: &str| {
        !part.is_empty()
            && !part
                .chars()
                .any(|c| c.is_whitespace() || c == ':' || c == '@')
    };
    let dir = dir.trim_end();
    (plain(user) && plain(host) && (dir.starts_with('/') || dir == "~" || dir.starts_with("~/")))
        .then_some(dir)
}

/// The window's (and the native tab's) title — priority order
/// `.tasks/026-sekmeler/discussion.md` → Karar 7.
///
/// 1. **The application's OSC 0/2 title** (vim, ssh, Claude Code, oh-my-zsh's
///    `termsupport`). An empty title is ignored: `\e]2;\a` is not a title, it
///    would leave the tab nameless.
/// 2. **The last component of the working directory** (OSC 7); the home
///    directory itself is `~`, the root `/`. A subdirectory is `proj`, not
///    `~/proj` — the tab is narrow and the last component is the distinguishing
///    one.
/// 3. `bateri`.
///
/// **While a remote session is active** (036 Karar 5, `remote` = host) the title
/// carries the [`crate::dock::REMOTE_MARK`] prefix: `⇄ {OSC title}`, `⇄ {host}` if
/// there is no title; the local directory is never consulted. The prefix is
/// unconditional, because most remote shells print `user@host: dir` into the
/// title and what tells the remote apart among tabs is that; since the dock goes
/// away on the alternate screen (vim on the remote), only the title carries the
/// indicator.
///
/// Pure and fed from three slots; the reader is [`crate::Session::title`]. The
/// home directory is an argument, because this crate reads no environment — the
/// application supplies the value ([`crate::SessionOptions::home`]).
pub(crate) fn title_of(
    osc_title: Option<&str>,
    cwd: Option<&str>,
    home: Option<&Path>,
    remote: Option<&str>,
) -> String {
    let osc_title = osc_title.filter(|title| !title.trim().is_empty());
    if let Some(host) = remote {
        let mark = crate::dock::REMOTE_MARK;
        return format!("{mark} {}", osc_title.unwrap_or(host));
    }
    if let Some(title) = osc_title {
        return title.to_owned();
    }
    let Some(cwd) = cwd.filter(|cwd| !cwd.is_empty()) else {
        return "bateri".to_owned();
    };
    let path = Path::new(cwd);
    if home.is_some_and(|home| home == path) {
        return "~".to_owned();
    }
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        // An absolute path with no last component is only the root: `file_name` gives
        // `None` for `/`. The scanner passes only absolute paths, so a relative `..`
        // does not land here; even if it did, the path itself is an honest title.
        None => cwd.to_owned(),
    }
}

/// The mirror's current state — the single answer to whether the dock is drawn.
///
/// `Unavailable` is a separate variant, **not** inside `Idle`: the two do not
/// show the same thing. In `Idle` there is no line to draw (ZLE is not editing),
/// in `Unavailable` there **is one but we cannot show it** — and the difference
/// decides phase-4's suppression decision: a line we cannot show must stay on
/// the grid, otherwise the user sees what they typed nowhere. Today's `Skip` arm
/// not signaling the caller is exactly what produced this symptom (R1.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DockStatus {
    /// ZLE is not editing a line: no mirror ever arrived or `line-finish` arrived.
    #[default]
    Idle,
    /// The fields are fresh and valid.
    Live,
    /// A mirror arrived but could not be read; the fields are **empty**.
    Unavailable(DockFault),
    /// The mirror was **read and is valid**, but the display carries a control
    /// character the dock **does not draw** (the `\x01` of `Ctrl-V Ctrl-A`): the dock
    /// would leave that column blank, while ZLE prints a readable `^A` on the grid.
    ///
    /// The data is sound, the **surface is narrow**; a line we cannot show and its
    /// caret stay on the grid. (Until 032 it had a sibling, a display with line
    /// breaks, `Multiline`; it was removed when the dock learned to draw multiple
    /// lines, and `\n` is not counted as a control character by this arm.)
    /// Before this arm arrived, the fate of a control character was left to the
    /// freshness gate's **coincidence**: if `^A` was the last character of the line
    /// the two sides did not match and the line stayed on the grid, but if it was in
    /// the middle (`\x01foo`) both sides said `'o'`, the gate passed and the line went
    /// to the dock — `^A`'s column was blank, so the user saw what they typed
    /// **nowhere**. The arm makes the decision independent of position (025,
    /// `discussion.md` → Karar 1).
    ///
    /// **Tab is outside this arm** and the reason is information: a tab says nothing,
    /// the blank column in the dock is not a loss — it opens into blank on the grid
    /// too. Without the exception a Ctrl-V Tab line would fall from the dock to the
    /// grid.
    ///
    /// **The return rule is automatic:** the state is recomputed on every mirror
    /// payload, so when the control character is deleted the next mirror is `Live`.
    /// It has to depend on the line's shape, not on the keystroke — if it said
    /// "return on the next key" the caret would go back and forth between the grid and
    /// the dock. **The arm's lifetime is tied to a placeholder**: the day the dock
    /// draws a control character as `^X`, like zsh, this arm is deleted
    /// (`docs/YOL-HARITASI.md`).
    Control,
}

/// Why the mirror could not be read.
///
/// The two are separate, because they say two different things: `Overflow` that
/// the limit is narrow (and the limit is a design number derived in
/// [`DOCK_PAYLOAD_LIMIT`]'s doc), `Malformed` that the channel is corrupt. If
/// reduced to a single variant the answer to "do I need to raise the limit"
/// would be lost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockFault {
    /// The payload exceeded [`DOCK_PAYLOAD_LIMIT`].
    Overflow,
    /// The payload could not be decoded: field count, base64 or UTF-8.
    Malformed,
}

/// One record of `region_highlight`: a range of the display and its style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Highlight {
    /// The start of the range, as a **character** offset.
    ///
    /// The space is single and normalized: offsets are counted from the start of the
    /// string `PREDISPLAY ++ BUFFER ++ POSTDISPLAY`. zsh uses two spaces — if the
    /// record starts with `P` the offset is from the start of `PREDISPLAY`, otherwise
    /// from the start of `BUFFER` (`zshzle(1)`, `region_highlight`) — and merging
    /// them on **this** side of the boundary frees the drawing side from knowing
    /// `PREDISPLAY`'s length. This is R1.3's "decoded crosses".
    pub start: usize,
    /// The end of the range, exclusive.
    pub end: usize,
    pub style: HighlightStyle,
}

/// A range's style — the half of zsh's "character highlighting" specification
/// that we recognize.
///
/// An unrecognized component (`blink`, `dim`, an unknown name) **drops silently**,
/// it does not drop the record: the mirror's job is to carry what the user sees,
/// and to stumble on an attribute we do not recognize and leave the whole range
/// colorless would be to lose the information entirely.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HighlightStyle {
    pub fg: Option<HighlightColor>,
    pub bg: Option<HighlightColor>,
    pub bold: bool,
    pub underline: bool,
    /// `standout` — zsh's reverse video; the counterpart of SGR 7.
    pub standout: bool,
}

/// The color of a style component; it is **not** bound to the theme here.
///
/// Resolution is in `frame()`, when the [`crate::Theme`] is in hand: the color
/// space is linearized as it crosses the boundary (`CLAUDE.md` → Renk uzayı) and
/// this module has no theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HighlightColor {
    /// 0–255; the first 16 are the theme's [`crate::Theme::ansi`], above that the 256-color cube.
    Indexed(u8),
    /// `#rrggbb` → `0xRRGGBB`.
    Rgb(u32),
}

/// The minimum number of blocks the ledger will hold.
///
/// The ceiling derives from `scrollback` (below) and that **can be zero**: even
/// in a session that keeps no scrollback, the color of the blocks on screen is
/// needed. That is why the floor exists, and it is a **design constant**, not a
/// measurement (the precedent is [`PAYLOAD_LIMIT`]): far above a screenful of
/// prompts even in the largest reasonable window.
const BLOCK_LOG_FLOOR: usize = 256;

/// The `block identity → fate` ledger; a fixed ring.
///
/// **No eviction signal is awaited** and this is not a deficiency but an absence
/// of data: when the anchor's row is reset on the grid it drops silently and
/// alacritty does not publish that. The ring therefore solves eviction **by
/// design** — by overwriting the oldest.
///
/// Identities are **incremented one by one** by the shell, so the ring always
/// holds a contiguous range of identities and lookup is index arithmetic; a linear
/// scan would be multiplied by the number of visible blocks per frame.
///
/// **The ceiling derives from `scrollback`** and is not a fixed number: since at
/// least one row (the prompt) falls to each block, that is the upper bound on the
/// number of blocks that can be visible in scrollback. If a fixed ceiling were
/// chosen it would either fall below scrollback and leave still-on-screen blocks
/// colorless or hold space for nothing. 12 bytes per record: 120 KB at the
/// default 10,000 rows. (Until 013 it was 8 bytes; [`Outcome::Finished`] took the
/// elapsed time next to the exit code too. The number is tied to the `const`
/// assert next to [`Outcome`] — a budget that was written but not verified would
/// silently go stale exactly on this line.)
///
/// **Known limit:** the ceiling is set when the session is born; if `scrollback`
/// is enlarged live the ring does not grow and as many old blocks as the
/// difference lose their color — the stripe is **not drawn**, not drawn wrongly.
#[derive(Clone)]
pub(crate) struct BlockLog {
    /// `entries[i]` is the fate of the block whose identity is `first + i`.
    entries: VecDeque<Outcome>,
    /// The identity of `entries[0]`; meaningless while the ledger is empty.
    first: u32,
    capacity: usize,
}

impl BlockLog {
    pub(crate) fn new(scrollback: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            first: 0,
            capacity: Self::capacity_for(scrollback),
        }
    }

    /// Deriving the ceiling from `scrollback` — the **single** source of
    /// [`BlockLog::new`] and [`BlockLog::set_capacity`].
    ///
    /// If it were written separately in two places, the floor (`BLOCK_LOG_FLOOR`)
    /// could be forgotten in one and a live-shrunk `scrollback` could bring the
    /// ledger down to zero.
    fn capacity_for(scrollback: usize) -> usize {
        scrollback.max(BLOCK_LOG_FLOOR)
    }

    /// When `scrollback` changes at save time, it moves the ceiling too.
    ///
    /// The ceiling used to be set only when the session was born and `scrollback` is
    /// a **live-applied** setting: the excess of an enlarged history stayed colorless
    /// (`/code-review`, 010 gate). On shrinking, the excess is dropped from the
    /// oldest — the ring's own eviction rule, no second policy.
    fn set_capacity(&mut self, scrollback: usize) {
        self.capacity = Self::capacity_for(scrollback);
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
            self.first = self.first.wrapping_add(1);
        }
    }

    /// Empties the ledger; the ceiling stays.
    fn clear(&mut self) {
        self.entries.clear();
        self.first = 0;
    }

    /// Writes the block opened with `A` into the ledger.
    fn start(&mut self, id: u32) {
        // An identity inside the range being opened a second time: we reopen the block,
        // we do not delete the ledger. The ones after it are now invalid — those
        // identities are left over from a previous round.
        if let Some(at) = self.index_of(id) {
            self.entries.truncate(at + 1);
            self.entries[at] = Outcome::Pending;
            return;
        }
        // If it is not contiguous the ledger cannot interpret this identity and carrying
        // the old one would show the blocks of two separate counters in one range.
        //
        // **Both are defensive arms.** Our counter is monotonic across the shell
        // instance and `exec zsh` does not "reset" it: `.zshrc` calls `__bateri_restore`
        // before the first prompt, so the reborn shell inherits the user's `ZDOTDIR`,
        // never loads the wrapper and prints not a single mark. The way to land here
        // would be a `bt_block=` we did not print — that is the second reason the field
        // is ours alone.
        if self.entries.is_empty() || id != self.first.wrapping_add(self.entries.len() as u32) {
            self.entries.clear();
            self.first = id;
        }
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
            self.first = self.first.wrapping_add(1);
        }
        self.entries.push_back(Outcome::Pending);
    }

    /// Processes the code and duration of the block closed with `D`; an identity not
    /// in the ledger is ignored.
    fn finish(&mut self, id: u32, exit: Option<i32>, elapsed_ms: u32) {
        if let Some(at) = self.index_of(id) {
            self.entries[at] = Outcome::Finished { exit, elapsed_ms };
        }
    }

    /// A **not running** block's stripe — the one table of "unknown is not
    /// drawn" ([`ShellLog::stripe`]): `Pending`, an unreadable code and an id
    /// the ledger lost are `None`.
    fn finished_stripe(&self, id: u32) -> Option<Stripe> {
        match self.get(id)? {
            Outcome::Finished { exit: Some(0), .. } => Some(Stripe::Success),
            Outcome::Finished { exit: Some(_), .. } => Some(Stripe::Error),
            Outcome::Finished { exit: None, .. } | Outcome::Pending => None,
        }
    }

    /// The block's fate; `None` if not in the ledger, and in that state the stripe is not drawn.
    fn get(&self, id: u32) -> Option<Outcome> {
        self.index_of(id).map(|at| self.entries[at])
    }

    /// The block the ledger **opened most recently**; `None` while the ledger is empty.
    ///
    /// Since identities are contiguous and increasing, the last record is the last
    /// `A` — the single answer to "which block is running" ([`BlockTrack::running`]).
    fn last(&self) -> Option<(u32, Outcome)> {
        let at = self.entries.len().checked_sub(1)?;
        Some((self.first.wrapping_add(at as u32), self.entries[at]))
    }

    /// The identity's place in the ring; an identity outside the range is `None`.
    ///
    /// `checked_sub`: a block that dropped past the ceiling (identity smaller than
    /// `first`) must not wrap around to the end of the ring.
    fn index_of(&self, id: u32) -> Option<usize> {
        let at = id.checked_sub(self.first)? as usize;
        (at < self.entries.len()).then_some(at)
    }
}

/// The ledgers' copy the quit-time snapshot resolves anchors with
/// ([`ShellLog::saved_stripes`]).
pub(crate) struct SavedStripes {
    local: BlockLog,
    remote: Option<(RemoteShell, BlockLog)>,
    running: RunningBlocks,
}

impl SavedStripes {
    /// The colour a saved anchor carries; `None` → the row is saved without
    /// one. A block **running** at quit is `None` too: the command dies with
    /// the shell, `accent` would claim it still runs and there is no neutral
    /// role — "unknown is not drawn" (`.tasks/053-oturum-geri-yukleme/discussion.md`
    /// → Set sonrası düzeltmeler).
    pub(crate) fn stripe(&self, key: BlockKey) -> Option<Stripe> {
        if self.running.is(key) {
            return None;
        }
        match key {
            BlockKey::Local(id) => self.local.finished_stripe(id),
            BlockKey::Remote { shell, id } => self
                .remote
                .as_ref()
                .filter(|(saved, _)| *saved == shell)
                .and_then(|(_, blocks)| blocks.finished_stripe(id)),
            BlockKey::Saved { stripe, .. } => Some(stripe),
        }
    }
}

/// One shell's block trail: the phase, the running command's clock and the
/// ledger — the triple a stripe, a counter and "which block is running" are
/// read from (048 phase-3).
///
/// **Two instances** ([`ShellLog::local`], [`ShellLog::remote`]): our remote
/// shell's marks drive the same rules on their own trail, so the remote rows
/// get stripes and counters while the local phase (the dock's caret, the
/// clock, the remote session) stays the `ssh` command's. The rules are the
/// local ones, written once: the first `C` plants the clock, `A` resets it,
/// only an identified `D` consumes it.
pub(crate) struct BlockTrack {
    /// The shell's current state; `None` = no mark yet.
    pub(crate) state: Option<ShellState>,
    /// The start instant of the running command; `None` while no command is running.
    ///
    /// **A single field, not per block:** only one command runs at a time, because
    /// between `C` and `D` the shell does not print the next prompt. Putting an
    /// `Instant` in every ledger entry would be a dead cost paid per tab in a
    /// 10,000-line scrollback.
    ///
    /// `Instant`, not system time: even if the user changes the clock or daylight
    /// saving passes, the duration does not run backwards.
    pub(crate) running_since: Option<Instant>,
    pub(crate) blocks: BlockLog,
}

impl BlockTrack {
    fn new(scrollback: usize) -> Self {
        Self {
            state: None,
            running_since: None,
            blocks: BlockLog::new(scrollback),
        }
    }

    /// Forgets the trail (a new remote shell); the ledger keeps its ceiling.
    fn clear(&mut self) {
        self.state = None;
        self.running_since = None;
        self.blocks.clear();
    }

    /// The first mark **creates** the state.
    fn state(&mut self) -> &mut ShellState {
        self.state.get_or_insert(ShellState {
            phase: ShellPhase::Prompt,
            last_exit: None,
        })
    }

    /// `A`: the prompt; an identified one opens the block.
    ///
    /// **The clock's second reset point and a defensive arm.** If the prompt is
    /// being printed no command is running, so the clock here is stale by
    /// definition. If only `D` consumed it, a lost `D` (an OSC cut halfway, an
    /// identity-less close) would leave the clock standing and the **next**
    /// block's `D` would consume it: an instant command would look like it took
    /// "4m 12s" (`/code-review`, 013 gate). The reset's direction is safe — the
    /// worst case is the counter never appearing, not a made-up duration.
    fn prompt(&mut self, id: Option<u32>) {
        self.state().phase = ShellPhase::Prompt;
        self.running_since = None;
        if let Some(id) = id {
            self.blocks.start(id);
        }
    }

    /// `C`: the command runs. Where the clock is planted: `C` says the command
    /// **started running**; the printing of the prompt or the time the user
    /// spent typing must not enter the counter.
    ///
    /// **The first `C` wins, later ones do not overwrite.** The user's shell may
    /// have a second OSC 133 source (iTerm2's `~/.iterm2_shell_integration.zsh`,
    /// VS Code, Ghostty) and it prints `C` too — measured, on the user's machine
    /// **two** `C`s arrive per command. If we overwrote, the duration would start
    /// from the second mark; worse, a `C` arriving mid-command (the ^C arm of
    /// iTerm2's `precmd`) would reset the clock. The only place for resetting is
    /// the prompt (`A`).
    fn command(&mut self) {
        self.state().phase = ShellPhase::Running;
        self.running_since.get_or_insert_with(Instant::now);
    }

    /// `D`: the command ended.
    ///
    /// We refresh the code **in every state**: filling an unreadable code with
    /// the old one would be labeling the finished command with someone else's
    /// code.
    ///
    /// **The clock is consumed only by an identified `D`**: an identity-less `D`
    /// cannot write into the ledger, and if it consumed the clock anyway the
    /// duration we measured would **go to waste**. This is not hypothetical, it
    /// was measured on the user's machine: with iTerm2's shell integration
    /// installed every command produces two `D`s — first its identity-less one,
    /// then ours; the identity-less one took the clock, ours found it empty and
    /// the duration was written as **zero** ("the duration does not show when it
    /// finishes"). `take` is still mandatory but inside the identity: if it
    /// remained, between two commands (the `Finished` phase, which contains a
    /// `git` fork) a finished command would still be counted as running.
    fn end(&mut self, exit: Option<i32>, id: Option<u32>) {
        let state = self.state();
        state.phase = ShellPhase::Finished;
        state.last_exit = exit;
        if let Some(id) = id {
            let elapsed = self
                .running_since
                .take()
                .map_or(0, |since| millis(since.elapsed()));
            self.blocks.finish(id, exit, elapsed);
        }
    }

    /// The identity of the **running** block; `None` if no command is running.
    ///
    /// Two conditions together: the phase is `Running` **and** the ledger's last
    /// record is still open. Without the second, a `C` arriving after an
    /// identity-less `A` would put the phase in `Running`, while the ledger's last
    /// record would be the previous (finished) block and that block would be
    /// painted as if running.
    fn running(&self) -> Option<u32> {
        if self.state?.phase != ShellPhase::Running {
            return None;
        }
        match self.blocks.last()? {
            (id, Outcome::Pending) => Some(id),
            (_, Outcome::Finished { .. }) => None,
        }
    }

    /// [`ShellLog::stripe`] on this trail; `running`: `id` is this trail's
    /// running block.
    fn stripe(&self, id: u32, running: bool) -> Option<Stripe> {
        if running {
            return Some(Stripe::Running);
        }
        self.blocks.finished_stripe(id)
    }

    /// [`ShellLog::duration`] on this trail; `running` as in [`Self::stripe`].
    fn duration(&self, id: u32, running: bool) -> Option<Duration> {
        if running {
            // A running block but no clock: half of the integration arrived (`A` is
            // there, `C` is not). No counter instead of a made-up duration.
            return self.running_since.map(|since| since.elapsed());
        }
        match self.blocks.get(id)? {
            Outcome::Finished { elapsed_ms, .. } => Some(Duration::from_millis(elapsed_ms.into())),
            Outcome::Pending => None,
        }
    }
}

/// Mouse selection in the dock's input line (031 phase-4): the two ends, the step
/// and the resolved range — in **`BUFFER`'s character indices**.
///
/// **It lives next to the mirror, not in it** ([`ShellLog::dock_selection`]). If
/// it were inside [`DockState`], the frame path's diff (`dock::change` /
/// `diff`) would compare it too and every drag step would `Reset` 030's typing
/// effects; moreover the scanner refreshes the mirror wholesale with `clone_from`
/// and would overwrite the selection on every keystroke. The selection is still
/// **tied** to the mirror: it is dropped when `BUFFER` changes
/// ([`ShellLog::apply_dock`]), because the indices would now point at another
/// text.
///
/// The range is resolved once when the ends change (`dock::selection_range`) and
/// stored here: the frame path searches no word, it only reads two numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DockSelection {
    /// The press point; drag and Shift+click do not move it.
    anchor: DockPoint,
    /// The drag's end.
    head: DockPoint,
    kind: SelectKind,
    /// `[start, end)`; `start == end` for an empty selection.
    range: (usize, usize),
    /// The mirror's clustering flag ([`DockState::cluster`]): the ends and the
    /// ⇧←/⇧→ step are on cluster boundaries (035 R4.2). It travels with the
    /// selection, so extension does not ask for it again.
    cluster: bool,
}

impl DockSelection {
    /// `buffer` is the `BUFFER` the selection belongs to: the range is resolved against it.
    pub(crate) fn new(
        kind: SelectKind,
        anchor: DockPoint,
        head: DockPoint,
        buffer: &str,
        cluster: bool,
    ) -> Self {
        Self {
            anchor,
            head,
            kind,
            range: dock::selection_range(buffer, kind, anchor, head, cluster),
            cluster,
        }
    }

    /// Moves the end to `head`; anchor and step stay (drag, Shift+click).
    pub(crate) fn extended(self, head: DockPoint, buffer: &str) -> Self {
        Self::new(self.kind, self.anchor, head, buffer, self.cluster)
    }

    /// The selected range; `None` if empty — a click without a drag selects nothing.
    pub(crate) fn range(&self) -> Option<(usize, usize)> {
        (self.range.0 < self.range.1).then_some(self.range)
    }

    /// The boundary a **single click** without a drag falls on — the click-to-caret
    /// target (031 R4.1). Only for an empty `Simple` selection: double and triple
    /// click do not move the caret. A mouse dragged and brought back to where it
    /// started also leaves an empty `Simple` and moves the caret — the behavior of
    /// text fields.
    pub(crate) fn click(&self) -> Option<usize> {
        (self.kind == SelectKind::Simple && self.range.0 == self.range.1).then_some(self.range.0)
    }

    /// ⇧← / ⇧→ (031 Karar 8): moves the selection's **moving end** by one character;
    /// if there is no selection, it starts from `caret`. The result is always
    /// `Simple` — a selection started with a word or line step grows with a letter
    /// step on the keyboard (the behavior of text fields).
    ///
    /// The **moving end** is from the head's direction relative to the anchor: if it
    /// is to the left of the anchor, the range's start, otherwise its end. In an
    /// empty range (a selection narrowed by the keyboard) the two ends are the same
    /// point and the step is from there.
    ///
    /// The step is a **character** but does not fall between a combining mark and its
    /// base: the keyboard form of `dock::selection_range`'s `boundary` rule,
    /// otherwise `é`'s accent could be selected apart from its base. With clustering
    /// on (`cluster`, 035) the step is a **cluster**: half of `🇹🇷` cannot be selected.
    pub(crate) fn stepped(
        current: Option<Self>,
        caret: usize,
        forward: bool,
        buffer: &str,
        cluster: bool,
    ) -> Self {
        let chars: Vec<char> = buffer.chars().collect();
        let len = chars.len();
        let (fixed, active) = match current {
            Some(selection) => {
                let (start, end) = selection.range;
                // Halves are not ordered (`CellHalf` is not `Ord`): left < right.
                let order = |point: DockPoint| (point.index, point.half == CellHalf::Right);
                let backward = order(selection.head) < order(selection.anchor);
                if backward { (end, start) } else { (start, end) }
            }
            None => {
                let caret = caret.min(len);
                // If the caret is **inside** a cluster (ZLE can put it there) ⇧←'s fixed end is
                // the back of the cluster: otherwise `boundary` would lower it to the start of
                // the cluster and the first step would give an empty selection (`/code-review`).
                let fixed = if cluster && !forward {
                    dock::cluster_span(chars.iter().copied(), caret, true)
                        .filter(|&(start, _)| start < caret)
                        .map_or(caret, |(_, end)| end)
                } else {
                    caret
                };
                (fixed, caret)
            }
        };
        let zero_width = |index: usize| {
            chars
                .get(index)
                .is_some_and(|&ch| dock::column_width(ch) == 0)
        };
        let mut moved = active;
        let span = |index| dock::cluster_span(chars.iter().copied(), index, true);
        if cluster {
            // The moving end is on a cluster's boundary (range from [`boundary`]); the next
            // boundary is the back of the cluster, the previous one is the start of the
            // previous.
            moved = if forward {
                span(moved).map_or(moved, |(_, end)| end)
            } else {
                moved
                    .checked_sub(1)
                    .and_then(span)
                    .map_or(moved, |(start, _)| start)
            };
        } else if forward {
            if moved < len {
                moved += 1;
                while moved < len && zero_width(moved) {
                    moved += 1;
                }
            }
        } else if moved > 0 {
            moved -= 1;
            while moved > 0 && zero_width(moved) {
                moved -= 1;
            }
        }
        let point = |index| DockPoint {
            index,
            half: CellHalf::Left,
        };
        Self::new(
            SelectKind::Simple,
            point(fixed),
            point(moved),
            buffer,
            cluster,
        )
    }
}

/// The shell ledger the reader thread writes and the frame path reads.
///
/// Two records under a **single** leaf lock: the same mark stream feeds both and
/// the same frame reads both. Separate locks could catch the same frame between
/// the two halves of a mark.
/// The expected result of an editing command: `BUFFER` and the caret, stamped
/// with the **generation** at which the command was sent.
///
/// The repeat of a held ⌫ can arrive faster than the mirror; if the gate looked at
/// the stale mirror and closed, the repeat would go to ZLE as a code point and
/// in `🇹🇷🇺🇸` the second ⌫ would delete only the `🇷`. We define the command's
/// effect ourselves (`d;S;E;L`: `[S,E)` is deleted, the caret is `S`), so the
/// result is certain; the only ways it can come out wrong are the widget rejecting
/// the command or a write from outside the shell, and both change the length —
/// the next command's `L` does not match, the widget does nothing: a repeat is
/// lost, a cluster is not split
/// (`.tasks/035-grapheme-dizileri/phase-5.md` → Uygulama Notları).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DockPrediction {
    /// The generation born when the command was sent ([`crate::Session`]'s
    /// `key_gen`); in any other generation the prediction is invalid.
    pub(crate) generation: u64,
    pub(crate) buffer: String,
    /// Character index in `BUFFER`.
    pub(crate) caret: usize,
}

pub(crate) struct ShellLog {
    /// The local shell's trail: its phase (`None` = no integration), the
    /// running command's clock and the block ledger ([`BlockTrack`]).
    pub(crate) local: BlockTrack,
    /// Our **remote** shell's trail (048 phase-3, [`RemoteMark`]): the stripes
    /// and counters of the rows the server's prompt anchored with
    /// `bateri://rblock/<P>.<S>.<n>`.
    ///
    /// **Not tied to the remote state** ([`DockContext::remote`]): a remote `A`
    /// can arrive before the probe has set it, and the stripes stay in the
    /// history after ssh ends, like local ones (048 discussion → Karar). It is
    /// cleared when the remote shell changes ([`Self::remote_shell`]) — an old
    /// session's rows are then not drawn, rather than drawn with the new
    /// session's codes. Its clock is read only while the parent is the open
    /// local command ([`Self::running_blocks`]): a dropped connection leaves no
    /// remote `D`, and a running remote block must not tick an idle window.
    pub(crate) remote: BlockTrack,
    /// Whose the remote trail is; `None` before the first remote mark.
    pub(crate) remote_shell: Option<RemoteShell>,
    /// ZLE's display mirror. Under the same lock, because it is fed from the same
    /// stream and read by the same frame: a separate lock could catch the frame at a
    /// moment when the phase and the mirror contradict each other — like suppressing
    /// the grid in the `Input` phase and drawing the previous line in the dock.
    pub(crate) dock: DockState,
    /// The dock's context line: directory and branch. **Next to** the mirror, not in
    /// it ([`DockContext`]); same lock, separate lifetime.
    pub(crate) context: DockContext,
    /// The pattern list of `[remote] hosts` (037 Karar 2); the remote host's mark
    /// ([`DockContext::remote_mark`]) is resolved from it, at two edges.
    pub(crate) host_rules: Vec<HostRule>,
    /// The dock's mouse selection; `None` → no selection. **Next to** the mirror (see
    /// [`DockSelection`]'s doc) and under the same lock: the writer that deletes it
    /// when `BUFFER` changes (the reader thread) and the frame that reads the range
    /// see it in the same turn.
    pub(crate) dock_selection: Option<DockSelection>,
    /// The **wheel-selected** top of the dock's vertical window (032 phase-4); `None`
    /// → the window follows the caret ([`crate::dock::render_with`]).
    ///
    /// If in an input past the ceiling the window only followed the caret, the mouse
    /// could never reach the rows above. **Next to** the mirror, with the selection's
    /// reason ([`Self::dock_selection`]): the scanner refreshes the mirror wholesale.
    /// Its lifetime is tied to the caret staying in place — it is dropped when
    /// `BUFFER`, `PREBUFFER` or the caret changes (typing, an arrow key) and the
    /// window returns to the caret; a change of suggestion does not drop it.
    pub(crate) dock_scroll: Option<usize>,
    /// Whether the shell bound the editing widget **at this prompt** (`8133;w`,
    /// 031) — the editing gate's fourth condition
    /// ([`crate::Session::can_edit_dock`]).
    ///
    /// **Next to** the mirror, not in it: the scanner refreshes [`DockState`]
    /// wholesale with `clone_from` on every `u` payload and the capability arrives
    /// once per prompt, so if it were inside it would be erased at the first
    /// keystroke. Its lifetime is the prompt's: `line-finish` (`e`) and the start of
    /// the prompt (`A`) clear it. `A` is a second belt — so that an exit where
    /// `line-finish` did not run (an interrupted line) does not carry the capability
    /// to the next prompt's `w`; the wrong direction is "no editing".
    pub(crate) dock_editable: bool,
    /// The **expected** result of the last editing command (035 phase-5): until the
    /// mirror answers that command, the editing gate looks at this line
    /// ([`DockPrediction`]). Its lifetime is only one generation — any intervening
    /// input invalidates it; `e` and `A` clear it too.
    pub(crate) dock_pending: Option<DockPrediction>,
    /// Command generation: incremented on every **transition** of the phase to
    /// `Running` (036 Karar 2).
    ///
    /// The stale-answer gate for the remote session probe: the probe is on the main
    /// thread, `D` on the reader thread, and the answer of a command that finishes in
    /// between must not leak into the next one. The caller takes the generation
    /// before the probe ([`crate::Session::running_command`]) and hands it back with
    /// the answer ([`crate::Session::set_remote`]); if it does not match the answer is
    /// dropped.
    ///
    /// **A second `C` is not a transition** and does not move the generation — the
    /// same place as the clock's "first `C` wins" rule ([`Self::running_since`]):
    /// iTerm2's mid-command `C` must not invalidate the answer for a running ssh.
    pub(crate) command: u64,
    /// Whether a line editor switched bracketed paste on **since the remote
    /// state was set** (`CSI ? 2004 h`, in stream order; 047 phase-4) — the
    /// remote shell's prompt. Not since `C`: what the same command line ran
    /// before ssh (`ssh $(fzf)`) switches it on too. The `C` transition and a
    /// new remote target clear it; a remote prompt that came before the
    /// probe is the terminal modes' to see.
    pub(crate) paste_since_remote: bool,
    /// The command generation whose remote session is known to be logged in
    /// (047 R9.1, [`crate::Session::remote_login`]) — a cache: the answer does
    /// not change within a generation and a later question needs no syscall.
    /// Bound to the generation, so `C` invalidates it by itself.
    pub(crate) login: Option<u64>,
    /// The remote bootstrap's last `8133;i;up;{nonce}` (049 R2.2) and the
    /// command generation it arrived in ([`crate::Session::remote_up`]). Bound
    /// to the generation like [`Self::login`], so `C` invalidates it by itself
    /// — and neither `set_remote` nor `D` clears it: `up` is the bootstrap's
    /// first byte and routinely beats the probe, and the pane's main-queue
    /// check may run after `D`. Written only while a command runs.
    pub(crate) remote_up: Option<(u64, String)>,
    /// The command generation whose remote session the user typed into after
    /// its login was seen ([`Self::login`]; 049 R7,
    /// [`crate::Session::remote_typed`]): the session was the user's, so a
    /// wrapped `ssh` that ends without the bootstrap's `up` (a `ForceCommand`
    /// CLI) did not fall back. Bound to the generation like [`Self::login`];
    /// set at the first input after the login, once.
    pub(crate) typed: Option<u64>,
    /// Whether the shell printed a mark carrying **our** identity (an `A` or `D` with
    /// `bt_block=`) — sticky; the precondition of [`Self::apply`]'s foreign-mark gate.
    ///
    /// The gate cannot be built without this flag: in a shell with integration off
    /// but printing its own OSC 133 (iTerm2, kitty) **all** marks are identity-less
    /// and the remote state would never be cleared.
    ours: bool,
    /// The command opened by the last `Running` transition has not yet been closed
    /// by **our** `D`/`A` ([`Self::running_command`]'s second arm).
    ///
    /// The phase alone is not enough: the `A` of the integration on the far end of
    /// ssh can arrive in the same read **before** the probe and pull the phase to
    /// `Prompt` — while the command is still running the generation looks like
    /// `None`, the probe is dropped and the indicator would never appear.
    command_open: bool,
    /// The **raw** answer of the handover, as last observed.
    ///
    /// The stamp is kept in [`Self::apply_scan`] — the single entry point and under the
    /// leaf lock, so without touching `Term` at all. If it were kept in the frame path,
    /// in a window where no mark arrives between two frames the stamp would never
    /// move, and a mark that arrives between two frames would pass **without a
    /// trace**.
    caret_raw: CaretHome,
    /// When [`Self::caret_raw`] last **changed**.
    ///
    /// An unchanged observation does not move the stamp: every keystroke produces a
    /// mirror event and if the stamp were refreshed with them the hold would never
    /// expire.
    caret_since: Instant,
    /// `line-finish` (`8133;e`) is **held**: when it arrived (032 Karar 11).
    ///
    /// zsh runs `line-finish` on every `PS2` acceptance, with no `precmd` in between
    /// and the phase stays `Input` (measured, zpty); right after it comes
    /// `line-init`'s mirror (`u`, `PREBUFFER` filled). If `e` reset the mirror
    /// instantly, in the multi-line dock every ⏎ would shrink the band for a frame and
    /// grow it again, and the accepted line would appear on the grid for a moment.
    /// While held, the mirror's display, band and suppression stay **as they are**;
    /// if `u` arrives the new mirror passes, and if an OSC 133 mark (`C`: the command
    /// ran, `A`: a new prompt) arrives or [`HANDOVER_HOLD`] expires
    /// ([`Self::expire_end`]) today's reset happens. The duration is the caret hold's
    /// clock, no second number.
    ///
    /// **Next to the mirror, not in it:** every consumer asks `status == Live` and has
    /// to see `Live` throughout the hold; a new state variant would change all nine of
    /// nine consumers.
    end_since: Option<Instant>,
}

/// The caret's owner: the grid or the dock.
///
/// **One predicate, two consumers.** [`crate::dock::render`] asks it to draw the
/// caret, [`crate::Session::frame`] to hide the grid's cursor; if they were written
/// separately, the same frame would have two carets (or none) —
/// the observed defect was exactly that (012 phase-8).
///
/// **This is a separate question from the suppression of the input line.**
/// Suppression asks which **cells** will be skipped and its answer depends on the
/// anchor; what is asked here is the caret's **place** and it has nothing to do
/// with the anchor — since a zero-width prompt writes no cell, the caret is the
/// dock's even when there is no anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaretHome {
    /// The grid draws.
    Grid,
    /// The dock draws.
    Dock,
}

/// The **hold duration** of the Dock→Grid handover (hysteresis).
///
/// **Chosen, not measured.** Both ends have reasons: the lower bound is measured
/// (`context.md` → Kanıt: while `ls` runs the phase lasts 44 ms, so any hold
/// below 44 ms never catches `ls`) and the value here is more than three times
/// that — so that `git status`-class commands are covered too. The upper bound's
/// precedent is 013: the counter of a command that does not exceed one second is
/// **not shown**, so the threshold at which the user counts it as "running" is one
/// second already; the hold must stay well below it so that a genuinely running
/// command shows its caret on the grid.
///
/// **Its relation to the third number must be written:** the cursor animation
/// settles in ~230 ms (`bt_gpu::motion`, `OMEGA`'s doc) and this value is
/// **below** it. The consequence: every hold that expires releases the caret
/// while the animator is still on its way, so in commands that exceed the hold a
/// single clean targeting splits into two. A known cost, `.tasks/015-imlec-cilasi/phase-1.md`
/// → Bilinen sınırlar; whoever changes the value must take this relation into
/// account. The precedent is `bt_gpu::motion`'s
/// `const _: () = assert!(EASE_DURATION < TIME_CEILING)` — there, since the two
/// numbers are in the same crate, the condition can be written to the compiler;
/// here, since it crosses the crate boundary, there is only this sentence.
///
/// **Not** the subject of `docs/OLCUMLER.md`: this is a feel threshold, not a
/// measurement (the precedent is `FADE_DURATION`).
pub(crate) const HANDOVER_HOLD: Duration = Duration::from_millis(150);

/// The **nearer** of two deadlines; empty if both are empty.
///
/// Free and pure, **for testability**: `min` quietly turning into a write
/// (overwrite) would be an invisible defect in both directions — either the
/// running command's counter freezes or the handover never happens. The precedent
/// is `bt_gpu::link`'s `due_clock`, which was pulled out into a pure helper for
/// exactly this reason.
pub(crate) fn sooner(a: Option<Duration>, b: Option<Duration>) -> Option<Duration> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// The handover's answer at an instant: the caret's owner and the remainder of the
/// hold.
///
/// **One record, because one `now`.** If the two were asked separately they would
/// belong to two different instants; the same reason as [`SuppressedInput`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CaretDecision {
    /// Who draws the caret in this frame.
    pub(crate) home: CaretHome,
    /// The time left while the hold is **flipping the answer**; only this enters the
    /// clock. `None` → no hold, so no frame to request.
    pub(crate) hold_left: Option<Duration>,
}

/// The handover's **raw** answer: from the phase and the mirror's state, without
/// the hold applied.
///
/// A separate function, because this is what the stamp tracks
/// ([`ShellLog::observe_caret`]): the hold derives from the stamp and cannot be
/// fed back into it — if it were, the hold would extend itself indefinitely.
fn caret_home_raw(shell: Option<ShellState>, status: DockStatus) -> CaretHome {
    match (shell.map(|state| state.phase), status) {
        (Some(ShellPhase::Running), _)
        | (_, DockStatus::Unavailable(_) | DockStatus::Control)
        | (Some(ShellPhase::Input), DockStatus::Idle) => CaretHome::Grid,
        _ => CaretHome::Dock,
    }
}

/// Resolves the caret's owner with the phase, the mirror's state and the **hold**.
///
/// A free function and does not see the ledger; in production its only caller is
/// [`ShellLog::caret`], and it was left free so that tests can ask the predicate
/// without building a load.
///
/// `held` = whether the Dock→Grid handover is **being held** right now
/// (hysteresis, R1.1). The hold's duration and stamp are in the ledger
/// ([`HANDOVER_HOLD`], [`ShellLog::caret`]); only the decision comes here, because
/// this function sees no clock.
///
/// **Only the Dock→Grid direction is held.** If the reverse direction were
/// delayed, the caret would hang on the grid when the command finishes, and when
/// the user starts typing they would see a line in the dock with no caret — the
/// wrong direction is not safe.
///
/// **`Unavailable` and `Control` are outside the hold.** The reason for those two
/// arms is written below and is unconditional: a line we cannot show stays on the
/// grid, its caret must stay there too, *otherwise the user cannot see where they
/// are typing*. If the hold covered them too, after a paste carrying `^A` the
/// caret would stand beside the dock's prompt mark for 150 ms, that is, the fixed
/// symptom would come back in shortened form. Moreover the failure **does not
/// produce** a jump — the mirror does not return to `Live` until the user deletes
/// back — so the hold's gain there is zero, and its cost would be the caret
/// standing in an empty dock for 150 ms. The carve-out is **inside** the
/// predicate, because if it were outside `caret_home(_, Unavailable, true)` would
/// return `Dock` and the predicate would lie.
///
/// **The rule is one sentence: the caret follows where the line is drawn.** If the
/// input line is on the grid the caret is on the grid, if it is in the dock it is
/// in the dock. Four states are the grid's:
///
/// - `Running` — a command is running. It owns the line: the input `cat` waits
///   for, `ssh`'s password prompt and vim's own cursor live on the grid.
/// - `Unavailable` — there is a line we cannot show and it stays on the grid
///   (R1.2); its caret must stay there too, otherwise the user cannot see where
///   they are typing.
/// - `Control` — the display carries a control character the dock does not draw
///   ([`DockStatus::Control`]). The second application of the same sentence: since
///   the line stays on the grid, the caret is there too. (The line break has not
///   been in this list since 032: the dock draws multiple lines itself.)
/// - `Input` + `Idle` — the shell says "the user is typing" but ZLE has
///   **released** the line. Suppression lifts exactly here too (R3.3): `CORRECT`'s
///   `[nyae]` question, a `zle -M` message, between `line-finish` and Enter. Since
///   the line returns to the grid, the caret has to return too.
///
/// **Every remaining state is the dock's, and `state == None` is included.** At
/// startup (zsh's rc time), while the prompt is being drawn (`Prompt`) and between
/// the end of each command and the new prompt (`Finished`; it contains `precmd`'s
/// `git rev-parse` fork) there is **no** input line in the middle — the dock shows
/// an empty caret and that is where the user will start typing. Tying the gate to
/// "has the shell spoken at least once" would leave the caret on the grid in
/// those windows and would **make it jump** when the prompt arrived — that was the
/// fixed defect.
///
/// **Known window:** `B` is printed inside the prompt while the mirror is born in
/// ZLE's `line-init`; in between the phase is `Input` but the state is `Idle`, so
/// the caret is on the grid for a moment. The window is as long as zsh's own
/// startup — no fork, no I/O — and the way to close it is to keep a new state
/// that separates "the mirror never arrived" from "ZLE released". We do not add
/// that state without a measured symptom; the window that was removed (`Finished`,
/// one `git` fork) is many times above this.
/// *(015 phase-1: the window now **melts inside the hold** — zsh's `line-init` is
/// far below [`HANDOVER_HOLD`], so it is not reported at that moment at all. The
/// record above is dated and stays: the window itself did not close, it became
/// invisible.)*
///
/// **Second known limit:** if the integration is installed but the script dies
/// silently, the caret stays in the dock and does not move as you type.
/// Misleading but visible (the dock is empty, no block stripe), so it does not
/// fall into the "silently wrong" class this repo forbids.
pub(crate) fn caret_home(shell: Option<ShellState>, status: DockStatus, held: bool) -> CaretHome {
    match caret_home_raw(shell, status) {
        CaretHome::Grid
            if held && !matches!(status, DockStatus::Unavailable(_) | DockStatus::Control) =>
        {
            CaretHome::Dock
        }
        home => home,
    }
}

/// The two ends of the input line to suppress; `Copy`.
///
/// The **top** of the range is given by the identity (its anchor is on that
/// row), the **bottom** by the text left behind the cursor. The two are in one
/// record, because both come out of the same leaf-lock turn; if read separately
/// they could belong to different instants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SuppressedInput {
    /// The identity of the block being written; [`crate::Session::frame`] finds its
    /// **row** from the anchor — the shell does not know which row it is on.
    pub(crate) block: u32,
    /// The mirror has **no characters at all**: neither in the display
    /// (`PREDISPLAY ++ BUFFER ++ POSTDISPLAY`) nor in `PREBUFFER` — the freshness
    /// gate's empty-mirror question (`Session::frame`'s `blank_mirror`, 025).
    ///
    /// **Character, not column** (032): until 032 the criterion was "zero columns on
    /// both sides of the caret" and it was not line-aware — a lone `\n` pushes the
    /// cursor down a row, so the premise "the cursor must be on the anchor's row" is
    /// now true only for a genuinely empty mirror. If `PREBUFFER` is filled (a `for>`
    /// line) the cursor is legitimately below the anchor and the anchor question is
    /// never asked.
    pub(crate) blank: bool,
    /// Whether suppression's upper floor is **the anchor's row** (032 Karar 7):
    /// `PREBUFFER` is filled (ZLE accepted the earlier lines, all of them together
    /// with their `PS2`s are part of the input) or `line-finish` is being held
    /// ([`ShellLog::expire_end`]; the accepted line is about to move to `PREBUFFER`).
    /// In both the layout walk's top end does not know the grid — `PS2`'s width is
    /// not in the mirror — and the anchor is certain data.
    pub(crate) from_anchor: bool,
    /// The display's last ink ([`DockState::last_ink`]) — the mirror-side half of the
    /// freshness gate.
    pub(crate) last_ink: Option<char>,
    /// Whether ZLE is in the insert keymap ([`DockState::insert_keymap`]); the third
    /// condition of the paste's narrow exception.
    ///
    /// In the same record, because it comes out of the same leaf-lock turn: if read
    /// separately the keymap and the phase could belong to different instants and the
    /// exception could stay open on a line the user has long since moved to `vicmd`.
    pub(crate) insert_keymap: bool,
    /// The input generation the mirror answers ([`DockState::answers`]) — the
    /// freshness gate's temporal half. **In the same record** as `last_ink`, because
    /// the two must belong to the same mirror: if read separately, a new stamp could
    /// declare an old content "fresh".
    pub(crate) answers: u64,
}

impl ShellLog {
    pub(crate) fn new(scrollback: usize) -> Self {
        Self {
            local: BlockTrack::new(scrollback),
            remote: BlockTrack::new(scrollback),
            remote_shell: None,
            dock: DockState::default(),
            context: DockContext::default(),
            host_rules: Vec::new(),
            dock_selection: None,
            dock_scroll: None,
            dock_editable: false,
            dock_pending: None,
            command: 0,
            paste_since_remote: false,
            login: None,
            remote_up: None,
            typed: None,
            ours: false,
            command_open: false,
            // At startup the caret is the dock's (`caret_home_raw(None, Idle)`), so the first
            // handover is always in the Dock→Grid direction and the hold can apply to it.
            caret_raw: CaretHome::Dock,
            caret_since: Instant::now(),
            end_since: None,
        }
    }

    /// Moves the ledger's ceiling when `scrollback` changes at save time; does not
    /// touch the session state (`state`).
    pub(crate) fn set_scrollback(&mut self, scrollback: usize) {
        self.local.blocks.set_capacity(scrollback);
        self.remote.blocks.set_capacity(scrollback);
    }

    /// Applies the mark to both the state and the ledger; the first mark **creates**
    /// the state.
    ///
    /// Since the state slot is an `Option`, "we never saw a mark" and "we are at the
    /// prompt" do not get confused: while nothing feeds it the slot stays empty and
    /// tells the outside "no integration".
    ///
    /// The return value is the notifications the caller will give ([`ScanOutcome`]):
    /// the transition to `Running` and the deletion of the remote state (036).
    pub(crate) fn apply(&mut self, mark: Mark) -> ScanOutcome {
        // **The remote session is ended only by OUR mark** (036 phase-3): if the shell
        // has printed our identity once and the remote session is active, an
        // identity-less mark (an identity-less `A`/`D`, every `B` and `C`) touches
        // nothing. The source is the far end of ssh: fish 4 or the kitty/iTerm2
        // integration prints 133 into the same PTY and the remote `A` would erase the
        // indicator, the remote `C` would open a new command generation. The local shell
        // is with blocks behind ssh, so an identity-less mark arriving then cannot be
        // ours. The gate is **limited to the remote session**, not to `Running`: on a
        // switch to a shell that will never print our identity again, like `exec fish`,
        // `Running` would never end, the clock would not stop and the counter would
        // request frames while idle. The race before the probe is in
        // [`Self::command_open`].
        let identified = match mark {
            Mark::PromptStart { id } | Mark::CommandEnd { id, .. } => id.is_some(),
            Mark::PromptEnd | Mark::CommandStart => false,
        };
        if identified {
            self.ours = true;
        } else if self.ours && self.context.remote.is_some() {
            return ScanOutcome::default();
        }
        // What closes the command: our identified `A`/`D`; any `A`/`D` in a shell that
        // has never shown our identity.
        if matches!(mark, Mark::PromptStart { .. } | Mark::CommandEnd { .. })
            && (identified || !self.ours)
        {
            self.command_open = false;
        }
        // **A held `line-finish` ends on every mark** (Karar 11): `C` says the command
        // ran, `A` says a new prompt — in both the accepted line is now the grid's
        // permanent content.
        if self.end_since.take().is_some() {
            self.end_line();
        }
        let mut outcome = ScanOutcome::default();
        match mark {
            Mark::PromptStart { id } => {
                // The remote state goes **by itself** (036 Karar 2): no round trip to `bt-shell`
                // to end it. `A` is `D`'s defensive arm — like the clock's, so that a lost `D`
                // does not carry the remote indicator into the next prompt. The remote shell's
                // identity-less `A` never reaches here (the gate above).
                outcome.title = self.context.clear_remote();
                self.dock_editable = false;
                self.dock_pending = None;
                // The phase, the clock's reset and the block ([`BlockTrack::prompt`]).
                self.local.prompt(id);
                outcome.prompt = id.is_some();
            }
            Mark::PromptEnd => self.local.state().phase = ShellPhase::Input,
            Mark::CommandStart => {
                // A **transition** only when the phase is not `Running`: a second `C` (iTerm2
                // integration) moves neither the generation nor the remote state
                // ([`Self::command`]). The probe is locked until `D`, so if that `C` deleted the
                // host the indicator would not come back.
                if self.local.state().phase != ShellPhase::Running {
                    self.command += 1;
                    self.command_open = true;
                    self.paste_since_remote = false;
                    outcome.started = true;
                    outcome.title = self.context.clear_remote();
                    // The offer's lifetime is until the next command (Karar 8).
                    self.context.reconnect = None;
                }
                // The phase and the clock: the first `C` wins ([`BlockTrack::command`]).
                self.local.command();
            }
            Mark::CommandEnd { exit, id } => {
                // **The offer comes before the remote state is deleted** (037 Karar 8): the
                // target and mark go with `clear_remote`. The identity is required — in a shell
                // that has never shown our identity an identity-less `D` reaches here, and a
                // `D;255` from the far end of ssh must not produce an offer. 255 is ssh's own
                // error (a drop or failure to connect); mosh does not exit on a drop, so its 255
                // does not carry this meaning.
                if id.is_some()
                    && exit == Some(255)
                    && let Some(target) = &self.context.remote
                    && target.kind == RemoteKind::Ssh
                {
                    let offer = self
                        .context
                        .reconnect
                        .get_or_insert_with(Reconnect::default);
                    offer.host.clone_from(&target.host);
                    offer.line.clone_from(&target.line);
                    offer.mark = self.context.remote_mark;
                }
                outcome.title = self.context.clear_remote();
                // The code, and the clock consumed **only by OUR `D`** — the one carrying the
                // identity ([`BlockTrack::end`]).
                self.local.end(exit, id);
            }
        }
        outcome
    }

    /// Applies our remote shell's mark to the remote trail (048 phase-3) — and
    /// **only** there: no local field, no notification ([`RemoteMark`]'s doc).
    ///
    /// A new remote shell starts a new trail: the old session's ledger cannot
    /// be read under the new one's numbers (two sessions' `rblock/1`s).
    fn apply_remote(&mut self, remote: RemoteMark) {
        if self.remote_shell != Some(remote.shell) {
            self.remote.clear();
            self.remote_shell = Some(remote.shell);
        }
        match remote.mark {
            Mark::PromptStart { .. } => self.remote.prompt(Some(remote.id)),
            Mark::PromptEnd => self.remote.state().phase = ShellPhase::Input,
            Mark::CommandStart => self.remote.command(),
            Mark::CommandEnd { exit, .. } => self.remote.end(exit, Some(remote.id)),
        }
    }

    /// Applies the event the scanner extracted to the right arm.
    ///
    /// A single entry point, because the reader thread takes the lock **per event**
    /// and two separate calls would mean two separate lock turns.
    #[cfg(test)]
    pub(crate) fn apply_scan(&mut self, event: ScanEvent<'_>) {
        self.apply_scan_answering(event, 0);
    }

    /// [`Self::apply_scan`] as it is in production: the mirror event writes `answers`
    /// into [`DockState::answers`] **in the same turn as the content**.
    ///
    /// The stamp is an argument, because the ledger does not see `Session` and must
    /// not; the reading side (`session`'s `TappedPty`) brings the generation with one
    /// atomic read per event.
    ///
    /// The return value is two notifications for the caller ([`ScanOutcome`]); it
    /// gives both after releasing the lock. The title's notification only on a
    /// **different** local OSC 7 directory or on the deletion of the remote state: if
    /// every `precmd` printing the same directory produced a notification, every
    /// prompt would post a needless job to the main queue.
    pub(crate) fn apply_scan_answering(
        &mut self,
        event: ScanEvent<'_>,
        answers: u64,
    ) -> ScanOutcome {
        let mut outcome = ScanOutcome::default();
        match event {
            ScanEvent::Mark(mark) => outcome = self.apply(mark),
            // **Our remote shell's mark is separated before anything local**
            // (048 phase-3): `apply`'s identity, the held `line-finish`, the
            // command's closing and the three notifications never see it — the
            // remote `A` must not hand ⌘T's first input to the remote shell
            // (`outcome.prompt`) nor end the remote session.
            ScanEvent::RemoteMark(remote) => self.apply_remote(remote),
            // **While a remote session is active OSC 8133 is ignored** (048 R4): the
            // local shell is behind ssh and the only 8133 that can arrive is a remote
            // one — a remote mirror would draw a foreign line in the local dock, and
            // a remote `8133;w` would make the local dock send `CSI 8133 ~` editing
            // commands to the remote shell. The defense is here, not in a script's
            // care. Our `D` clears the remote state, so the next local 8133 applies.
            ScanEvent::Dock(_) if self.context.remote.is_some() => {}
            ScanEvent::Dock(event) => self.apply_dock(event, answers),
            // The directory arrives **resolved**: the scanner did the scheme, the path and
            // the percent-decoding, and only a drawable path and whether the authority is
            // local reach here. A rejected OSC 7 produces no event at all, so the old path
            // stays in place — a stale path rather than showing a wrong one.
            //
            // **Which slot** (036 Karar 4): while a remote session is active **every** OSC 7
            // goes to the remote side's — the local shell is behind ssh with blocks, so a
            // remote shell printing `file:///…` must not overwrite the local directory. While
            // inactive a foreign authority goes to the remote slot: OSC 7 can arrive before
            // the probe and must not change the result. The remote slot does not enter the
            // title, so there is no notification.
            ScanEvent::PasteOn => self.note_paste_on(),
            // **Not gated by the remote session** (048): it is the remote side's
            // own report about itself, it touches no dock state, and it can arrive
            // before the probe has set the remote state — the same lifecycle as
            // the remote slot below (cleared on `C`/`D`/`A`).
            ScanEvent::RemoteSetup(fault) => self.context.remote_setup = Some(fault),
            // **The remote 8133 defense's second narrow exception** (049 R2.2,
            // `f`'s precedent): the bootstrap's proof that the wrapped command
            // ran. Not gated by the remote state — it is the bootstrap's first
            // byte and beats the probe — but by a running command: at a local
            // prompt there is no wrapped ssh to vouch for. Its content is a
            // nonce and the pane matches it against the wrapped argv's; a
            // foreign `up` (a `cat`ed file, a server that prints one) carries
            // no nonce of ours and teaches nothing.
            ScanEvent::RemoteUp(nonce) => {
                if self.running_command().is_some() {
                    self.remote_up = Some((self.command, nonce));
                    outcome.up = true;
                }
            }
            ScanEvent::Cwd { path, local } => {
                if self.context.remote.is_some() || !local {
                    self.context.remote_cwd.clear();
                    self.context.remote_cwd.push_str(path);
                } else if self.context.cwd != path {
                    self.context.cwd.clear();
                    self.context.cwd.push_str(path);
                    outcome.title = true;
                }
            }
        }
        self.observe_caret();
        outcome
    }

    /// The running command's generation ([`Self::command`]); `None` if no command is
    /// running. The **single** definition of the two halves of the stale-answer gate
    /// ([`crate::Session::running_command`], [`crate::Session::set_remote`]): if they
    /// diverged the probe could take a generation that `set_remote` would reject.
    ///
    /// The second arm is [`Self::command_open`]: in a shell that prints our identity
    /// the command runs until our `D`, even if a foreign `A` has moved the phase.
    pub(crate) fn running_command(&self) -> Option<u64> {
        let running = self
            .local
            .state
            .is_some_and(|state| state.phase == ShellPhase::Running);
        (running || (self.ours && self.command_open)).then_some(self.command)
    }

    /// The user sent input ([`crate::Session::send_input`], the single funnel):
    /// `true` once per command generation, at the first input after the
    /// remote session's login was seen (049 R7, [`Self::typed`]) — the edge
    /// of [`crate::Wake::remote_typed`]. A login seen later than the keys
    /// leaves them uncounted (the login probe's lag): the wrong direction is
    /// a plain rerun, today's behaviour.
    pub(crate) fn note_typed(&mut self) -> bool {
        let Some(command) = self.login else {
            return false;
        };
        if self.running_command() != Some(command) || self.typed == Some(command) {
            return false;
        }
        self.typed = Some(command);
        true
    }

    /// The scanner saw `CSI ? 2004 h` ([`ScanEvent::PasteOn`], in stream
    /// order): recorded only while the remote state is set — the local
    /// prompt's and anything before ssh are not the remote shell's (047 phase-4).
    pub(crate) fn note_paste_on(&mut self) {
        if self.context.remote.is_some() && self.running_command().is_some() {
            self.paste_since_remote = true;
        }
    }

    /// The remote session's login signals that arrive with the output (047
    /// R9.1): a remote OSC 7, bracketed paste switched on since the remote
    /// state was set, or a title of the `user@host: dir` shape written since
    /// then (`titled`, the caller's — the title is another leaf lock). `false`
    /// without a remote session.
    pub(crate) fn login_signalled(&self, titled: bool) -> bool {
        self.context.remote.is_some()
            && (!self.context.remote_cwd.is_empty() || self.paste_since_remote || titled)
    }

    /// Writes the remote session's host (036); `true` **if the title's input
    /// changed**.
    ///
    /// The gate is in the caller ([`crate::Session::set_remote`]: generation and
    /// phase); only the write is here. An empty host means "not remote" — there is no
    /// name to show.
    ///
    /// **A host carrying a control character is also ignored**: the name comes from
    /// the process table, that is, from the argv the user typed, and a line break or
    /// ESC would go (as a box) into the window title and the context line. The wrong
    /// direction is safe: the indicator does not appear, a wrong name is not drawn.
    ///
    /// The target is written as a whole (037 Karar 1) and the mark is resolved here,
    /// from the pattern list ([`Self::host_rules`]); the return is still only the
    /// change of the **host** — that is the title's input.
    pub(crate) fn set_remote(&mut self, target: Option<&RemoteTarget>) -> bool {
        let target = target
            .filter(|target| !target.host.is_empty() && !target.host.chars().any(char::is_control));
        let changed = self.context.remote_host() != target.map(|target| target.host.as_str());
        // Another host's load is not this one's (046 Karar 5); the same host
        // re-reported keeps its indicator.
        if changed {
            self.context.stats = None;
            self.context.sign_in = None;
            // Another host's prompt is not this one's login.
            self.paste_since_remote = false;
        }
        match target {
            Some(target) => {
                match &mut self.context.remote {
                    Some(slot) => slot.clone_from(target),
                    slot => *slot = Some(target.clone()),
                }
                self.context.remote_mark =
                    crate::settings::host_mark(&self.host_rules, &target.host);
                // A new remote session invalidates the old one's offer.
                self.context.reconnect = None;
            }
            // The remote slot stays: if the probe said "local" it is not read anyway and
            // `C`/`D`/`A` deletes it.
            None => {
                self.context.remote = None;
                self.context.remote_mark = HostMark::None;
            }
        }
        changed
    }

    /// Writes the host marks' pattern list and re-resolves the active remote host's
    /// mark (037 Karar 2); `true` **if the mark changed**. The same list is a no-op.
    pub(crate) fn set_host_rules(&mut self, rules: &[HostRule]) -> bool {
        if self.host_rules == rules {
            return false;
        }
        self.host_rules = rules.to_vec();
        // The offer's color is the mark's too: the placeholder's host is painted with it.
        let mut changed = false;
        if let Some(offer) = &mut self.context.reconnect {
            let mark = crate::settings::host_mark(&self.host_rules, &offer.host);
            changed |= mark != offer.mark;
            offer.mark = mark;
        }
        let Some(host) = self.context.remote_host() else {
            return changed;
        };
        let mark = crate::settings::host_mark(&self.host_rules, host);
        changed |= mark != self.context.remote_mark;
        self.context.remote_mark = mark;
        changed
    }

    /// Stamps the handover's raw answer; **if unchanged the stamp does not move**.
    fn observe_caret(&mut self) {
        let raw = caret_home(self.local.state, self.caret_status(), false);
        if raw != self.caret_raw {
            self.caret_raw = raw;
            // **The clock is read only on change.** `apply_scan` is the reader thread's
            // per-event entry point: every keystroke produces a mirror event, every prompt an
            // OSC 7 and a branch event, and the overwhelming majority of them do not change
            // the raw answer. If the read were outside, all of them would pay an
            // `Instant::now()` assumed to be free.
            self.caret_since = Instant::now();
        }
    }

    /// Applies the mirror event to [`Self::dock`].
    ///
    /// In the two states that cannot be drawn (`End`, `Unavailable`) the text is
    /// **emptied**: leaving a stale line would mean that in phase-4, while the grid is
    /// suppressed, the dock shows the previous command — the user's typing and what
    /// they see silently diverging, the symptom class this repo forbids.
    ///
    /// **A mirror whose `BUFFER` changed deletes the dock selection** (031 R3.4): the
    /// indices would now point at the characters of another text. Only `BUFFER` —
    /// the prompt being redrawn (`PREDISPLAY`) or a change of suggestion does not move
    /// the selected text. `End` and `Unavailable` empty the text, the selection too.
    fn apply_dock(&mut self, event: DockEvent<'_>, answers: u64) {
        match event {
            DockEvent::Update(staged) => {
                self.end_since = None;
                if self.dock.buffer != staged.buffer || self.dock.prebuffer != staged.prebuffer {
                    self.dock_selection = None;
                    self.dock_scroll = None;
                }
                if self.dock.cursor != staged.cursor {
                    self.dock_scroll = None;
                }
                self.dock.clone_from(staged);
                self.dock.answers = answers;
            }
            DockEvent::End => {
                self.dock_selection = None;
                self.dock_scroll = None;
                self.dock_editable = false;
                self.dock_pending = None;
                // An empty line is an answer too: see [`DockState::answers`]. A held line too —
                // `e` is ⏎'s answer and the freshness gate asks it throughout the hold (the
                // cursor may have descended to `PS2`'s line).
                self.dock.answers = answers;
                // **Held if the phase is `Input` and the mirror is live** (Karar 11,
                // [`Self::end_since`]). The clock is read only here, that is, once per ⏎ —
                // [`Self::observe_caret`]'s rule.
                let typing = self
                    .local
                    .state
                    .is_some_and(|state| state.phase == ShellPhase::Input);
                if typing && self.dock.status == DockStatus::Live {
                    self.end_since = Some(Instant::now());
                } else {
                    self.end_since = None;
                    self.end_line();
                }
            }
            DockEvent::Unavailable(fault) => {
                self.end_since = None;
                self.dock_selection = None;
                self.dock_scroll = None;
                self.dock.reset();
                self.dock.status = DockStatus::Unavailable(fault);
            }
            // The branch comes from the mirror's **channel** but is not the mirror's state:
            // it does not touch `status` or the text. An empty body means "not a repository"
            // and **deletes** the branch — if the previous repository's branch hung on in the
            // new directory the user would think they were on the wrong branch.
            DockEvent::Branch(branch) => {
                self.context.branch.clear();
                self.context.branch.push_str(branch);
            }
            DockEvent::Editable => self.dock_editable = true,
        }
    }

    /// `line-finish`'s reset: the text empties, the state is `Idle`; the stamp
    /// (`answers`) stays where `e` wrote it — [`DockState::answers`]'s "the `Idle`
    /// mirror is stamped too" rule.
    fn end_line(&mut self) {
        let answers = self.dock.answers;
        self.dock.reset();
        self.dock.status = DockStatus::Idle;
        self.dock.answers = answers;
    }

    /// Turns a held `line-finish` ([`Self::end_since`]) into a reset if its time is
    /// up; if the hold continues returns the remainder.
    ///
    /// Its only caller is [`crate::Session::frame`], at the **start** of the lock turn
    /// in which the mirror is read and with `caret`'s `now`: in the frame where the
    /// hold ends, suppression, band, caret and dock see the same reset mirror. The
    /// remaining time enters the clock (`Cursor::next_tick`) — one-shot, with a named
    /// stop condition (the hold expired or a `u`/mark ended it), so zero frames while
    /// idle is preserved. `Session::dock` does not call it: so that it does not
    /// diverge from `frame()`'s decision in the same frame.
    pub(crate) fn expire_end(&mut self, now: Instant) -> Option<Duration> {
        let since = self.end_since?;
        let left = HANDOVER_HOLD
            .checked_sub(now.saturating_duration_since(since))
            .filter(|left| !left.is_zero());
        if left.is_none() {
            self.end_since = None;
            self.end_line();
        }
        left
    }

    /// Whether a `line-finish` is being held ([`Self::end_since`]). The hold is
    /// resolved only in the frame path ([`Self::expire_end`]); in a window that draws
    /// no frames (a covered tab) an expired hold can remain, and the other paths that
    /// decide (the paste's wrapping decision) must count it as closed.
    pub(crate) fn holding_end(&self) -> bool {
        self.end_since.is_some()
    }

    /// The mirror's state the handover asks about: a held `line-finish` is counted as
    /// **already arrived** (`Idle`).
    ///
    /// The raw answer returns to `Grid` at the instant of `e` as before the hold, and
    /// the caret hold ([`HANDOVER_HOLD`]) counts from the same instant — the two holds
    /// end at the same clock, `line-finish` timing is the same as before 032. The
    /// mirror itself stays `Live` throughout the hold: drawing, band and suppression
    /// read it.
    fn caret_status(&self) -> DockStatus {
        if self.end_since.is_some() {
            DockStatus::Idle
        } else {
            self.dock.status
        }
    }

    /// The running block of each trail — the frame's single reading for
    /// [`Self::stripe`] and [`Self::duration`].
    ///
    /// **The remote one only while its parent is the open local command**:
    /// the `ssh` block `P` is the local ledger's last, still open record and
    /// the command runs ([`Self::running_command`], so a foreign `A` from the
    /// server before the probe does not cut it). A connection that drops
    /// leaves no remote `D` and the remote trail would say "running" forever —
    /// the stripe accent and the counter ticking an idle window. Our local `D`
    /// closing `P` ends it with no second path; the block stays `Pending`, i.e.
    /// not drawn (the local `exit` command's fate).
    pub(crate) fn running_blocks(&self) -> RunningBlocks {
        let remote = self.remote_shell.and_then(|shell| {
            let open = self.running_command().is_some()
                && self.local.blocks.last() == Some((shell.parent, Outcome::Pending));
            if !open {
                return None;
            }
            self.remote.running().map(|id| (shell, id))
        });
        RunningBlocks {
            local: self.local.running(),
            remote,
        }
    }

    /// The trail and the identity a key points to; `None` for a remote key
    /// of a shell the remote trail no longer holds.
    fn track(&self, key: BlockKey) -> Option<(&BlockTrack, u32)> {
        match key {
            BlockKey::Local(id) => Some((&self.local, id)),
            BlockKey::Remote { shell, id } => {
                (self.remote_shell == Some(shell)).then_some((&self.remote, id))
            }
            BlockKey::Saved { .. } => None,
        }
    }

    /// An owned copy of what [`Self::stripe`] reads, for the quit-time
    /// snapshot ([`crate::Session::final_history`]): the snapshot walks the
    /// grid under `Term` and the leaf lock does not go there. A copy of the
    /// two ledgers (12 bytes a block), once per pane at quit.
    pub(crate) fn saved_stripes(&self) -> SavedStripes {
        SavedStripes {
            local: self.local.blocks.clone(),
            remote: self
                .remote_shell
                .map(|shell| (shell, self.remote.blocks.clone())),
            running: self.running_blocks(),
        }
    }

    /// The identity of the block being written while the phase is `Input` —
    /// **without looking** at the mirror's state (the unconditional-mirror form of
    /// [`Self::suppressed_input`]).
    ///
    /// Its consumer is the first protected row when clearing the screen
    /// ([`crate::Session::clear_to_start`]): if the cursor's row is anchorless (an
    /// empty row of a multi-line input) the block is found from the anchor, and there
    /// the question is not "where is the line drawn" but "which rows are the input's"
    /// — whether the mirror is live does not change the answer. The second condition
    /// has the same reason as [`BlockTrack::running`]'s.
    pub(crate) fn input_block(&self) -> Option<u32> {
        if self.local.state?.phase != ShellPhase::Input {
            return None;
        }
        match self.local.blocks.last()? {
            (id, Outcome::Pending) => Some(id),
            (_, Outcome::Finished { .. }) => None,
        }
    }

    /// Where the quit-time snapshot of the scrollback stops
    /// ([`crate::Session::final_history`], 053 R1.2).
    ///
    /// The anchor arm is [`Self::input_block`]'s answer, i.e. the `blocks`
    /// tier too (its prompt is the user's but carries our anchor); the
    /// "no state" arm is a shell that never marked — integration off or a
    /// shell we have no wrapper for — whose cursor row is the prompt.
    pub(crate) fn history_cut(&self) -> HistoryCut {
        if self.local.state.is_none() {
            return HistoryCut::BeforeCursor;
        }
        self.input_block()
            .map_or(HistoryCut::ThroughCursor, HistoryCut::Anchor)
    }

    /// The identity of the block the user is **typing right now** — `Some` if the
    /// input line will be suppressed from the grid, `None` otherwise.
    ///
    /// [`crate::Session::frame`] reads this **before** the `Term` lock (the same
    /// pattern as [`crate::Theme`]) and finds the anchor row with the returned
    /// identity: the range to suppress is from that row to the cursor's row.
    ///
    /// **Three conditions together and all three are mandatory:**
    ///
    /// - The phase is `Input` — the user is typing. At `Prompt` ZLE has not yet taken
    ///   the line, at `Running`/`Finished` what they typed has long since become the
    ///   grid's permanent content.
    /// - The mirror is `Live` — we can show the line **somewhere else**. `Idle` and
    ///   `Unavailable` are separately correct answers: in the first ZLE is not editing
    ///   a line (`line-finish` arrived), in the second there is a line we cannot show
    ///   and it **has** to stay on the grid, otherwise the user sees what they typed
    ///   nowhere (R1.2). This tier of the gate is the reason [`DockStatus`] exists.
    /// - The ledger's last record is still open — the same as [`BlockTrack::running`]'s
    ///   second condition and for the same reason: a `B` arriving after an
    ///   identity-less `A` puts the phase in `Input`, while the ledger's last record
    ///   is the previous (finished) block and suppression would start from the
    ///   **wrong** row.
    pub(crate) fn suppressed_input(&self) -> Option<SuppressedInput> {
        if self.local.state?.phase != ShellPhase::Input || self.dock.status != DockStatus::Live {
            return None;
        }
        match self.local.blocks.last()? {
            (block, Outcome::Pending) => Some(SuppressedInput {
                block,
                blank: self.dock.display_chars == 0 && self.dock.prebuffer.is_empty(),
                from_anchor: !self.dock.prebuffer.is_empty() || self.end_since.is_some(),
                last_ink: self.dock.last_ink,
                insert_keymap: self.dock.insert_keymap,
                answers: self.dock.answers,
            }),
            (_, Outcome::Finished { .. }) => None,
        }
    }

    /// Writes the mirror's display (`PREDISPLAY ++ BUFFER ++ POSTDISPLAY`) into
    /// `into`, keeping its capacity; returns the caret's character index
    /// ([`DockState::cursor`], the same space).
    ///
    /// Its consumer is suppression's row arithmetic ([`crate::dock::grid_span`]) and
    /// it is called in the **same lock turn** as [`Self::suppressed_input`].
    /// `PREBUFFER` does not enter: on the grid those rows have long been printed with
    /// zsh's `PS2`, and they are not the subject of the layout-walking calculation
    /// (032 Karar 7).
    pub(crate) fn display_into(&self, into: &mut String) -> usize {
        into.clear();
        into.push_str(&self.dock.predisplay);
        into.push_str(&self.dock.buffer);
        into.push_str(&self.dock.postdisplay);
        self.dock.cursor
    }

    /// Who owns the caret at this moment and the remainder of the hold — the ledger-
    /// side face of [`caret_home`].
    ///
    /// [`crate::Session::frame`] reads this in the **same lock turn** as
    /// [`Self::suppressed_input`]: if taken in separate turns the two would belong to
    /// separate instants. The same reason gathers `home` and `hold_left` into one
    /// record — both come out of a single `now`.
    ///
    /// **The remainder is filled only while the hold is flipping the answer.** If the
    /// raw answer is already `Dock` there is nothing to wait for, and requesting a
    /// frame for an idle window would break the zero-frames-while-idle contract. All
    /// three of the clock's conditions are met here: the content genuinely changes
    /// (the caret moves **and** the fill count moves), one-shot, and the stop
    /// condition is named — the hold expired or the predicate returned to `Dock`.
    ///
    /// **The remote session comes before the hold** (036 Karar 8): on the remote
    /// there is no dock input line (`Cursor::input_rows == 0`), so there is no surface
    /// to take over either — the caret is on the grid, no hold. If the hold were
    /// applied afterward, a `set_remote` arriving right after `C` would seat the caret
    /// on the context line for 150 ms; the hold's reason (a caret coming back
    /// halfway) is moot here, because the remote state is lifted only by
    /// `D`/`A`/a new `C`. The remainder is `None` too: no frame is requested for an
    /// answer that is not flipped. The raw answer's stamp ([`Self::observe_caret`])
    /// does not look at this — the remote state does not change the phase.
    pub(crate) fn caret(&self, now: Instant) -> CaretDecision {
        if self.context.remote.is_some() {
            return CaretDecision {
                home: CaretHome::Grid,
                hold_left: None,
            };
        }
        let held = HANDOVER_HOLD
            .checked_sub(now.saturating_duration_since(self.caret_since))
            .filter(|left| !left.is_zero());
        let status = self.caret_status();
        let home = caret_home(self.local.state, status, held.is_some());
        // The two answers being compared pass through the **same gate** and differ only
        // in `held`: deriving the raw answer through a second path (calling
        // `caret_home_raw` directly) would make it possible for the two to diverge. If
        // they diverged, `home != raw` would hold in an arm where the hold was never
        // applied too, and the window would request a frame every 150 ms that changed
        // nothing — the silent-leak class for which the `quiet=` token is the last line
        // of defense.
        let unheld = caret_home(self.local.state, status, false);
        CaretDecision {
            home,
            // The remainder derives from **the answer having been flipped**, not from a
            // separate condition: if the two were written separately they could diverge and a
            // frame would be requested for nothing in an arm where the hold did not apply
            // (`Unavailable`). One sentence: if the hold flipped the answer, it has a
            // remainder.
            hold_left: held.filter(|_| home != unheld),
        }
    }

    /// A block's stripe; `None` → **not drawn**.
    ///
    /// The one place where the ledger and the phase meet, and the two are already
    /// under the same leaf lock — if they were separate, a frame could read an
    /// inconsistent pair between the two halves of a mark.
    ///
    /// The running block's color comes **from the phase, not the ledger** (that is why
    /// the [`Self::running_blocks`] parameter is in the signature, the one rule-bound
    /// exception): since `D` has not arrived its record in the ledger is `Pending`
    /// and `Pending` itself does not mean "running".
    ///
    /// The four states not drawn are in a single `match`, because all four are part of
    /// the same thesis — not drawing the unknown wrongly:
    ///
    /// - the identity is not in the ledger (the ring wrapped around or it was never
    ///   seen),
    /// - `Pending` but not running (an Enter pressed on an empty prompt, a pending
    ///   prompt),
    /// - `Finished(None)`: the command finished but the code could not be read —
    ///   there is no neutral role for "finished" and imitating a nonexistent role
    ///   with `accent` would be showing a non-running block as running.
    ///
    /// A restored block ([`BlockKey::Saved`]) carries its colour in the key.
    pub(crate) fn stripe(&self, key: BlockKey, running: RunningBlocks) -> Option<Stripe> {
        if let BlockKey::Saved { stripe, .. } = key {
            return Some(stripe);
        }
        let (track, id) = self.track(key)?;
        track.stripe(id, running.is(key))
    }

    /// The block's **drawable** duration; `None` in every state that produces no
    /// counter.
    ///
    /// Two sources, one question: the clock's age for a running block, the ledger's
    /// record for a finished one. If asked separately the caller would have to ask the
    /// "is this block running" question a second time and could diverge from
    /// [`Self::stripe`] — both take `running` **from outside**, so in the same frame
    /// they look at the same answer.
    ///
    /// The threshold is **not applied** here: "did it exceed one second" is a drawing
    /// decision and the drawing side ([`crate::Session::frame`]) supplies it. If it
    /// were applied here the path computing the clock's next tick would have to know
    /// the threshold a second time.
    pub(crate) fn duration(&self, key: BlockKey, running: RunningBlocks) -> Option<Duration> {
        let (track, id) = self.track(key)?;
        track.duration(id, running.is(key))
    }
}

/// Reduces a [`Duration`] to milliseconds, saturating.
///
/// Narrowing with `as` would wrap: a command that runs longer than 49 days (a
/// nohup'd build, a forgotten `tail -f`) would restart the counter from zero.
/// Saturation is wrong but **monotonic**; wrapping is wrong and surprising.
fn millis(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

/// The counter's threshold — a command shorter than this never produces a
/// counter.
///
/// **A design constant, not a measurement** (does not go into
/// `docs/OLCUMLER.md`): `0.01s` next to every `ls` would be noise, while a command
/// that exceeds one second raises two questions — while running "is it hung", when
/// finished "how long did it take" — and the answer to both is the same number.
/// The reference product makes the same threshold a setting
/// (`docs/ARASTIRMA.md` → `command_duration_threshold`); ours is constant today.
pub(crate) const COUNTER_FLOOR: Duration = Duration::from_secs(1);

/// The counter's resolution — separate for a **running** and a **finished**
/// command, and the reason for the distinction is both reading and battery.
///
/// A running counter requests a frame at every change (013 phase-2, clock). If it
/// showed tenths it would be **ten frames per second**, yet the question asked
/// while running is "is it hung" and the decimal is just noise. A finished value
/// is **frozen**: it costs no frames, so there the decimal's cost is zero and its
/// information is real — for someone comparing two runs `2.1s` and `2.9s` differ.
///
/// The visible result: the counter advances as `1s, 2s, 3s` and when the command
/// ends it **settles** as `3.4s`. Not a jump, a firming up.
///
/// **The decimal's boundary is the seconds tier, not ten seconds** (user
/// decision): a finished `45s` also settles as `45.3s`, because the "real number"
/// question holds after ten seconds too and costs nothing. From the minutes tier
/// the decimal drops — `1m 05.3s` is both long and unreadable; what is sought
/// there is already the rough magnitude.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Precision {
    /// A running command: whole seconds.
    Whole,
    /// A finished command: tenths below a minute.
    Tenths,
}

/// The time remaining until the running counter's next **visible** change.
///
/// The clock's only input (013 phase-2) and a direct consequence of the format:
/// since a running counter shows whole seconds the boundary is the next whole
/// second. If the format changes this place has to change too and the two sit side
/// by side — `bt-gpu` never asks the "when" question, it only waits for the given
/// duration.
///
/// Below the threshold the next change is the counter's **appearance**: if the
/// first frame of `sleep 5` is drawn before the threshold the clock is set to 1
/// second, not 16 ms.
pub(crate) fn next_tick(elapsed: Duration) -> Duration {
    if elapsed < COUNTER_FLOOR {
        return COUNTER_FLOOR - elapsed;
    }
    // **A separate resolution per tier.** In the hour tier the text (`1h 07m`)
    // changes once a minute; waking every second would have an hour draw 3540
    // **identical** frames (`/code-review`, 013 gate) and we would be the first to
    // violate the "content must genuinely change" condition we just wrote into the
    // module header.
    let period = if elapsed.as_secs() < 3600 {
        Duration::from_secs(1)
    } else {
        Duration::from_secs(60)
    };
    // The time remaining to the next whole boundary. If the remainder is zero the
    // full period returns: a zero-duration clock would put the callback in a loop.
    let since =
        Duration::from_nanos(u64::try_from(elapsed.as_nanos() % period.as_nanos()).unwrap_or(0));
    period - since
}

/// The counter's text — **on the stack**, no per-frame allocation.
///
/// With `String` it would be one allocation per frame for every running block: the
/// text changes once a second but is regenerated every frame.
///
/// The ceiling comes from the longest representable text and is not a fixed
/// guess: the duration is `u32` milliseconds, that is at most ~1193 hours, so the
/// longest text is `"1193h 03m"` — nine bytes. The buffer is tied by a test
/// ([`the_longest_counter_fits_the_buffer`]).
pub(crate) struct Counter {
    text: [u8; Counter::CAPACITY],
    len: usize,
}

impl Counter {
    const CAPACITY: usize = 12;

    /// Converts the duration to text.
    ///
    /// Four tiers and all from the reading question: tenths, seconds, minutes, hours.
    /// Truncation is deliberate **in place of rounding**: while writing `1.9s` there
    /// must not be a command that has passed 2.0 seconds; the counter is honest
    /// backwards, not forwards.
    pub(crate) fn new(duration: Duration, precision: Precision) -> Self {
        let mut counter = Self {
            text: [0; Self::CAPACITY],
            len: 0,
        };
        let secs = duration.as_secs();
        // `write!` returns a `fmt::Result` and the only error arm is the buffer filling
        // — which cannot be represented given the ceiling above, its guard is
        // `the_longest_counter_fits_the_buffer`. Instead of swallowing the result it is
        // tied with `debug_assert`: no panic on the PTY path.
        let written = if precision == Precision::Tenths && secs < 60 {
            let tenths = duration.as_millis() / 100;
            write!(counter, "{}.{}s", tenths / 10, tenths % 10)
        } else if secs < 60 {
            write!(counter, "{secs}s")
        } else if secs < 3600 {
            write!(counter, "{}m {:02}s", secs / 60, secs % 60)
        } else {
            write!(counter, "{}h {:02}m", secs / 3600, (secs / 60) % 60)
        };
        debug_assert!(written.is_ok(), "counter buffer is full: {duration:?}");
        counter
    }

    pub(crate) fn as_str(&self) -> &str {
        // The only writing path is `write_str` and it takes `&str`, so the buffer is
        // always valid UTF-8. The corrupt arm falls to an empty string: panicking for a
        // counter would violate the no-panic-on-the-PTY-path ban.
        std::str::from_utf8(&self.text[..self.len]).unwrap_or("")
    }
}

impl fmt::Write for Counter {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        let slot = self.text.get_mut(self.len..end).ok_or(fmt::Error)?;
        slot.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// The mark arm's OSC number — FinalTerm's "semantic prompt".
///
/// Not our choice but a contract we comply with: iTerm2, kitty, WezTerm, VS Code
/// and Ghostty read the same number, so our script produces blocks under them too.
const MARK_OSC: u32 = 133;

/// The mirror arm's OSC number — **our** sequence.
///
/// The number itself is a decision and has two criteria:
///
/// **It must not collide.** The exclusion list was not recalled by hand, it was
/// grepped: the numbers our parser (`vte-0.15.0/src/ansi.rs`, `osc_dispatch`)
/// interprets are 0, 2, 4, 8, 10–12, 22, 50, 52, 104 and 110–112; everything else
/// falls to the `unhandled` arm. On top of that the owned numbers of common
/// integrations: 7 (cwd), 9 (ConEmu/Windows Terminal), 133, 633 (VS Code), 777
/// (urxvt), 1337 (iTerm2/WezTerm), 9278 (Warp), 30001–30002 (kitty). 8133 is in
/// none of them.
///
/// **It must be short.** The number enters the stream **per** keystroke (R6.2); a
/// six-digit number means two extra bytes on every stroke. Four digits, with a
/// prefix saying it is `133`'s second arm: `8133`.
///
/// **What happens in another terminal:** practically nothing, because the wrapper
/// is loaded only under bateri's `ZDOTDIR` — a foreign terminal never sees this
/// sequence. The only exception is `tmux`/`screen` running **inside** bateri, and
/// both drop an OSC they do not recognize.
const DOCK_OSC: u32 = 8133;

/// The upper bound of the `ESC ] 133 ;` payload, in bytes.
///
/// **A design constant, not a measurement.** Standard payloads consist of a single
/// letter and a few key-values (`A;aid=12345`, `D;0;aid=12345`); 256 bytes is an
/// order of magnitude above them. The bound's job is not to hit a performance
/// threshold but to keep a corrupt or malicious stream from growing memory
/// without sending a terminator — a sequence exceeding the bound is dropped and
/// the scanner returns to idle.
const PAYLOAD_LIMIT: usize = 256;

/// The upper bound of the `ESC ] 8133 ;` payload, in bytes.
///
/// [`PAYLOAD_LIMIT`] (256) is **wrong** for this arm: a command line alone exceeds
/// it. The number was derived, not chosen:
///
/// - A 4096-character input — twenty lines in a 200-column window, orders of
///   magnitude above a hand-typed command line.
/// - At worst 4 bytes of UTF-8 per character → 16 KiB.
/// - base64's 4/3 inflation → ~21 KiB.
/// - `region_highlight` is of the same order: syntax highlighting leaves one
///   record per token and ~30 bytes per record.
/// - Rounded ceiling: **64 KiB**.
///
/// **What it governs:** not correctness but the dock's *usability*. A line that
/// exceeds it becomes visible via [`DockFault::Overflow`] and the input stays on
/// the grid — the user still sees what they type, just not in the dock.
const DOCK_PAYLOAD_LIMIT: usize = 64 * 1024;

/// The directory arm's OSC number — not our choice but a contract we comply with.
///
/// `7` is the de facto standard for "working directory": iTerm2, kitty, WezTerm,
/// GNOME Terminal and VS Code read the same number, and oh-my-zsh's
/// `termsupport.zsh` prints the same number. Had we chosen our own number we would
/// see only what our own script prints.
///
/// **`vte` does not recognize it** and this is the same situation as
/// [`DOCK_OSC`]: the payload falls into `osc_dispatch`'s `unhandled` arm and is
/// discarded (`vte-0.15.0/src/ansi.rs`; the interpreted numbers are 0, 2, 4, 8,
/// 10–12, 22, 50, 52, 104 and 110–112). So it is not "alacritty's `Title` event
/// is silently dropped" — **no event is born at all**; the directory can be seen
/// only through this arm.
const CWD_OSC: u32 = 7;

/// The upper bound of the `ESC ] 7 ;` payload, in bytes.
///
/// The number was derived, not chosen (the precedent is [`DOCK_PAYLOAD_LIMIT`]):
///
/// - On macOS a path's ceiling is `PATH_MAX`, that is 1024 bytes.
/// - At worst every byte is percent-encoded → 3072.
/// - On top of that the `file://` scheme and the authority section.
/// - Rounded ceiling: **4 KiB**.
///
/// **The consequence of exceeding it is silent** and this is deliberately
/// separate from the mirror's visible overflow (`DockFault`): an input line we
/// cannot show makes the user lose what they typed, while a directory we cannot
/// show only leaves the previous value on screen and the next prompt refreshes it.
const CWD_PAYLOAD_LIMIT: usize = 4 * 1024;

/// The reasonable upper bound of the number prefix; a sequence exceeding it is not
/// ours.
///
/// It exists so that the counter does not overflow in a stream that prints digits
/// after `ESC ]` without a terminator; there is no OSC number six digits away from
/// `133`.
const MAX_OSC_NUMBER: u32 = 999_999;

/// The reasonable upper bound of a CSI parameter; a sequence exceeding it is not
/// ours.
///
/// The precedent is [`MAX_OSC_NUMBER`] and it does the same job: so that the
/// counter does not overflow in a stream with no terminator. The penalty for
/// exceeding it goes the same way — the sequence is **not dropped**, it just goes
/// unrecognized; there is no ED parameter six digits away from `2`.
const MAX_CSI_PARAM: u32 = 999_999;

/// ED's "clear the whole screen" parameter: `CSI 2 J`.
///
/// There is **no arm** for `3J` (`ClearMode::Saved`) and RIS and the reason is the
/// same for both: both call `clear_history()` (`term/mod.rs:1806`, for RIS
/// `grid/mod.rs:341`), so `history_size()` drops to zero and the "fill the gap with
/// scrollback" rule closes **by itself**. Adding a third arm would be closing a
/// path a second time that is already closed by a flag.
const ERASE_ALL: u32 = 2;

/// DEC private mode 2004, bracketed paste: `CSI ? 2004 h` switches it on. A
/// line editor turns it on at its prompt (zsh's ZLE, bash's readline, fish) —
/// after the running command's `C`, i.e. inside an ssh, the remote shell's
/// prompt: the user is logged in (047 phase-4). The local zsh turns it off
/// before `C` (`zle_bracketed_paste`), so its own never counts.
const BRACKETED_PASTE: u32 = 2004;

/// Where the scanner is. What has to survive a chunk boundary is **not** the
/// payload itself but this whole state: "I saw ESC" and "I'm in the middle of the
/// digits" are also carried between two `read()`s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScanState {
    /// Outside a sequence; searching for the next `ESC`.
    Ground,
    /// `ESC` seen, `]` (OSC) or `[` (CSI) expected.
    Escape,
    /// `ESC ]` seen, the OSC number is being collected.
    Number,
    /// One of our numbers and `;` seen; the payload is being collected into that arm's buffer.
    Payload(Arm),
    /// Not our sequence (or it exceeded the bound): skipping until the terminator.
    Skip,
    /// `ESC [` seen; tracked until the CSI terminator.
    Csi(CsiScan),
}

/// As much of a CSI sequence as concerns us.
///
/// **There is no buffer and there will not be:** the two sequences we recognize
/// have a single number as their parameter, so there is no payload to collect
/// either. `vte`'s four CSI states (`advance_csi_entry`, `_param`,
/// `_intermediate`, `_ignore`) descend to a **single** state for us, because the
/// question we ask is narrow: "is this sequence `CSI 2 J` or `CSI ? 2004 h`".
/// The answer for any sequence with an intermediate byte, a marker other than a
/// leading `?` or a second parameter is the same — no — and [`Self::simple`]
/// carries that answer.
///
/// `Default` is **not derived**: if it were derived it would be `simple: false`,
/// that is "no sequence is recognized" — a silently wrong start. The one correct
/// start is named [`CsiScan::new`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CsiScan {
    /// The single collected parameter.
    param: u32,
    /// Whether any digit was seen. A parameterless `CSI J` means **ED 0** (from the
    /// cursor down), that is, not our sequence; without a separate flag `param`'s zero
    /// would be confused with "never written".
    has_digit: bool,
    /// Whether the sequence is still "single-parameter, no intermediate bytes, at
    /// most a leading `?`".
    simple: bool,
    /// Whether the sequence opened with the private marker `?` (DEC private mode,
    /// 047 phase-4: `CSI ? 2004 h`). Only a **leading** `?` counts; any other
    /// marker or a `?` after a digit clears [`Self::simple`].
    private: bool,
}

impl CsiScan {
    /// The state right after `ESC [`.
    fn new() -> Self {
        Self {
            param: 0,
            has_digit: false,
            simple: true,
            private: false,
        }
    }

    /// Whether the sequence is `CSI 2 J` — terminator included.
    fn is_erase_all(&self, final_byte: u8) -> bool {
        self.simple
            && !self.private
            && self.has_digit
            && self.param == ERASE_ALL
            && final_byte == b'J'
    }

    /// Whether the sequence is `CSI ? 2004 h` — bracketed paste switched on, the
    /// remote shell's prompt (047 phase-4, a login signal).
    fn is_paste_on(&self, final_byte: u8) -> bool {
        self.simple
            && self.private
            && self.has_digit
            && self.param == BRACKETED_PASTE
            && final_byte == b'h'
    }
}

/// The scanner's two arms; which buffer and which parser the payload goes to.
///
/// Carried inside the state, not in a separate field: that the arm is **always**
/// known while the payload is being collected is guaranteed by the shape of the
/// type — a separate field would look meaningful in `Ground` too and the "which
/// arm are we in" question could be answered from two places.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    /// [`MARK_OSC`] — the mark arm.
    Mark,
    /// [`DOCK_OSC`] — the mirror arm.
    Dock,
    /// [`CWD_OSC`] — dizin kolu.
    Cwd,
}

/// The answer of [`ShellLog::apply_scan_answering`]: the notifications the reader
/// thread will give after releasing the lock (036).
///
/// One return path, two notifications: opening a second path (a flag in the
/// ledger, a separate query) would tie the notification to a second turn of the
/// lock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScanOutcome {
    /// The title's input changed — a different local directory or the deletion of the
    /// remote state → [`crate::Wake::title_changed`].
    pub(crate) title: bool,
    /// The phase **moved** to `Running` → [`crate::Wake::command_started`].
    pub(crate) started: bool,
    /// Our identified `A` arrived: the shell reached the prompt (037 Karar 6). Its
    /// consumer is the session's first input (`SessionOptions::initial_input`); an
    /// identity-less `A` does not count — the `A` of the far end of ssh or of another
    /// tool does not say our shell reached the prompt.
    pub(crate) prompt: bool,
    /// The remote bootstrap's `up` was recorded (049, [`ShellLog::remote_up`])
    /// → [`crate::Wake::remote_up`].
    pub(crate) up: bool,
}

/// The event the scanner hands out.
///
/// Two arms in a single `enum`, because one call: the reader thread takes the lock
/// per event and two separate callbacks would produce two separate lock turns.
///
/// `Update` **lends**, it does not take ownership: the mirror's decoded state sits
/// in the scanner's own buffer and the consumer takes it under the lock with
/// `clone_from`. If it took ownership it would produce three `String`s and a `Vec`
/// per keystroke ([`DockState`]'s doc).
pub(crate) enum ScanEvent<'a> {
    Mark(Mark),
    /// A mark carrying `bt_remote=` ([`RemoteMark`]): its own arm, so the local
    /// [`ShellLog::apply`] cannot receive it.
    RemoteMark(RemoteMark),
    Dock(DockEvent<'a>),
    /// The working directory, the **resolved** full path, and whether the authority is
    /// this machine ([`LOCAL_AUTHORITIES`]). A rejected URI produces no event at all:
    /// there is no "directory could not be read" state, because the right answer is to
    /// leave the old one. A foreign authority has not been rejected since 036, it goes
    /// to the remote slot ([`ShellLog::apply_scan_answering`]).
    Cwd {
        path: &'a str,
        local: bool,
    },
    /// `CSI ? 2004 h`: a line editor switched bracketed paste on (047 phase-4).
    /// An **event**, not a counter like `CSI 2 J`'s: its meaning depends on
    /// where it falls between the marks of the same read (a local prompt's
    /// before `C`, the remote shell's after the remote state is set), so it
    /// is applied in stream order ([`ShellLog::note_paste_on`]).
    PasteOn,
    /// `8133;f;{code}`: the remote bootstrap fell back to a plain login shell
    /// (048, [`DockContext::remote_setup`]). On the mirror's number but **not**
    /// a [`DockEvent`]: the remote session's gate on the dock (048 R4) must not
    /// swallow it, and it never touches the mirror's state.
    RemoteSetup(RemoteSetupFault),
    /// `8133;i;up;{nonce}`: the remote bootstrap started (049 R2.2,
    /// [`ShellLog::remote_up`]). `f`'s twin: on the mirror's number, not a
    /// [`DockEvent`]. Owned: it arrives once per connection.
    RemoteUp(String),
}

/// The mirror arm's events.
pub(crate) enum DockEvent<'a> {
    /// The line was refreshed; its decoded state is on loan.
    Update(&'a DockState),
    /// `line-finish`: ZLE released the line.
    End,
    /// A mirror arrived but could not be read. **The signal is here**: today's `Skip`
    /// arm told the caller nothing (R1.2).
    Unavailable(DockFault),
    /// The git branch; an empty body means "not a repository". It comes from the
    /// mirror's channel (`precmd` prints it) but does not touch the mirror's
    /// **state**.
    Branch(&'a str),
    /// The editing widget is bound at this prompt (`line-init`, 031); does not touch
    /// the mirror's state ([`ShellLog::dock_editable`]).
    Editable,
}

/// The state machine that pulls two OSC numbers out of the stream.
///
/// **The number decision is made early:** every sequence that is not ours falls to
/// `Skip` without touching the buffer. Otherwise every legitimate OSC 52 copy
/// (kilobytes, megabytes) would enter the "sequence exceeding the bound" path and
/// what the bound distinguishes would be lost.
///
/// **The buffers of the two arms are separate.** If a single buffer were shared,
/// either 133's narrow bound would truncate the mirror or the mirror's wide bound
/// would give up what 133 protects (a stream with no terminator growing memory) —
/// one buffer cannot fit two bounds at once.
pub(crate) struct Scanner {
    state: ScanState,
    /// The payload after `133;`; filled only for our sequence and reused with
    /// `clear()` on every sequence — no per-sequence allocation.
    payload: Vec<u8>,
    /// The payload after `8133;`. A separate buffer, a separate bound (above).
    dock: Vec<u8>,
    /// The payload after `7;`. A third buffer, a third bound — the same reason: one
    /// buffer cannot fit three bounds at once.
    cwd: Vec<u8>,
    /// The intermediate buffer base64 output lands in; reused for every field.
    decoded: Vec<u8>,
    /// The mirror's decoded state — the buffer [`DockEvent::Update`] lends.
    line: DockState,
    /// The decoded directory — the buffer [`ScanEvent::Cwd`] lends.
    path: String,
    /// The decoded branch — the buffer [`DockEvent::Branch`] lends.
    branch: String,
    /// The collected OSC number and whether any digit was seen.
    number: u32,
    has_digit: bool,
    /// The number of `CSI 2 J` seen since the last drain.
    ///
    /// **A counter, not an event**, and the distinction is deliberate: [`ScanEvent`]
    /// exists for "a single lock turn per event" (its own doc), while the CSI arm's
    /// consumer asks for no lock at all — what it will increment is an atomic.
    /// Adding an arm to the enum would open a free but meaningless branch on every
    /// event path.
    screen_clears: u32,
    /// This machine's name, if the application supplied it
    /// (`SessionOptions::hostname`): an OSC 7 authority equal to it counts as
    /// local ([`is_local_authority`]).
    hostname: Option<String>,
}

impl Scanner {
    pub(crate) fn new() -> Self {
        Self {
            state: ScanState::Ground,
            payload: Vec::with_capacity(PAYLOAD_LIMIT),
            // The mirror buffer is allocated **up front**, like 133's: in steady state there
            // should be no per-keystroke allocation and a buffer that arrives by growing would
            // do exactly that in the first lines. 64 KiB per session, too small to measure
            // next to the grid.
            dock: Vec::with_capacity(DOCK_PAYLOAD_LIMIT),
            // The directory arm runs **per prompt**, not per keystroke; the buffer is still
            // allocated up front, because its size is 4 KiB and a buffer that arrives by
            // growing would allocate in the first prompts.
            cwd: Vec::with_capacity(CWD_PAYLOAD_LIMIT),
            decoded: Vec::new(),
            line: DockState::default(),
            path: String::new(),
            branch: String::new(),
            number: 0,
            has_digit: false,
            screen_clears: 0,
            hostname: None,
        }
    }

    /// The scanner that counts an OSC 7 authority equal to this machine's name as
    /// local ([`is_local_authority`]); once at startup, from
    /// `SessionOptions::hostname`.
    pub(crate) fn hostname(mut self, name: Option<String>) -> Self {
        self.hostname = name;
        self
    }

    /// The scanner that reads the mirror by cluster ([`DockState::cluster`]); once at
    /// startup, from `SessionOptions::cluster`.
    pub(crate) fn cluster(mut self, on: bool) -> Self {
        self.line.cluster = on;
        self
    }

    /// The number of `CSI 2 J` seen since the last call; **drains** the counter.
    ///
    /// A number, because its consumer adds it to a **generation counter**
    /// (`Session::screen_clears`): the question the frame path asks is not "was the
    /// screen cleared" but "have I accounted for this clear". If it were a flag, a
    /// frame falling between two clears would mistake the second for the first.
    pub(crate) fn take_screen_clears(&mut self) -> u32 {
        std::mem::take(&mut self.screen_clears)
    }

    /// Scans the slice and hands every event it finds to `on_event`.
    ///
    /// It **does not touch** the bytes: the slice is `&[u8]` and on return the caller
    /// passes it to the parser as it is.
    pub(crate) fn feed(&mut self, bytes: &[u8], mut on_event: impl FnMut(ScanEvent<'_>)) {
        let mut rest = bytes;
        while !rest.is_empty() {
            // The idle fast path: while outside a sequence we do not walk the buffer byte by
            // byte, we jump to the next `ESC`. Almost all of the ordinary stream is this
            // branch.
            if self.state == ScanState::Ground {
                match rest.iter().position(|&b| b == 0x1b) {
                    Some(at) => {
                        self.state = ScanState::Escape;
                        rest = &rest[at + 1..];
                    }
                    None => return,
                }
                continue;
            }
            let byte = rest[0];
            rest = &rest[1..];
            self.step(byte, &mut on_event);
        }
    }

    fn step(&mut self, byte: u8, on_event: &mut impl FnMut(ScanEvent<'_>)) {
        match self.state {
            // Same as `advance_ground`: only if the fast path falls through does it get here.
            ScanState::Ground => {
                if byte == 0x1b {
                    self.state = ScanState::Escape;
                }
            }
            // `vte::advance_esc`: `]` opens an OSC sequence, `[` a CSI; everything else
            // (DCS, single-letter escapes) does not concern us.
            ScanState::Escape => match byte {
                b']' => {
                    self.state = ScanState::Number;
                    self.number = 0;
                    self.has_digit = false;
                }
                // **The fourth arm.** Until now it fell to `Ground` through the `_ =>` below; it
                // is now tracked, because `CSI 2 J` is being looked for and a `]` inside an
                // unframed CSI could open a false OSC for us.
                b'[' => self.state = ScanState::Csi(CsiScan::new()),
                // The bytes that `advance_esc` **leaves** in `Escape`: `ESC` itself, C0s other
                // than 0x18/0x1A (they are `execute`d, the state does not change) and everything
                // above 0x7F (the last `_ => ()` arm). If we fell to Ground, a `]` arriving after
                // these would not open a new sequence for us and the `ESC \r ] 133;A BEL` mark
                // the grid counts as valid would be lost on our side.
                0x00..=0x17 | 0x19 | 0x1b | 0x1c..=0x1f | 0x7f.. => {}
                _ => self.state = ScanState::Ground,
            },
            ScanState::Number => match byte {
                b'0'..=b'9' => {
                    self.number = self
                        .number
                        .saturating_mul(10)
                        .saturating_add(u32::from(byte - b'0'));
                    self.has_digit = true;
                    if self.number > MAX_OSC_NUMBER {
                        self.state = ScanState::Skip;
                    }
                }
                b';' => {
                    self.state = match (self.has_digit, self.number) {
                        (true, MARK_OSC) => {
                            self.payload.clear();
                            ScanState::Payload(Arm::Mark)
                        }
                        (true, DOCK_OSC) => {
                            self.dock.clear();
                            ScanState::Payload(Arm::Dock)
                        }
                        (true, CWD_OSC) => {
                            self.cwd.clear();
                            ScanState::Payload(Arm::Cwd)
                        }
                        _ => ScanState::Skip,
                    };
                }
                _ if is_terminator(byte) => self.close(byte),
                // `vte` drops these bytes without taking them into the payload; so do we, for parity.
                _ if is_ignored(byte) => {}
                _ => self.state = ScanState::Skip,
            },
            ScanState::Payload(Arm::Mark) => {
                if is_terminator(byte) {
                    let mark = parse_mark(&self.payload);
                    self.close(byte);
                    if let Some(event) = mark {
                        on_event(event);
                    }
                } else if is_ignored(byte) {
                } else if self.payload.len() == PAYLOAD_LIMIT {
                    // A sequence exceeding the bound is dropped; it is skipped up to the terminator so
                    // that a sound sequence following it is still seen.
                    self.state = ScanState::Skip;
                } else {
                    self.payload.push(byte);
                }
            }
            ScanState::Payload(Arm::Dock) => {
                if is_terminator(byte) {
                    // Decoding happens **before** `close`: `close` empties the buffer.
                    let outcome = parse_dock(
                        &self.dock,
                        &mut self.decoded,
                        &mut self.line,
                        &mut self.branch,
                    );
                    self.close(byte);
                    let event = match outcome {
                        DockOutcome::Update => ScanEvent::Dock(DockEvent::Update(&self.line)),
                        DockOutcome::End => ScanEvent::Dock(DockEvent::End),
                        DockOutcome::Unavailable(fault) => {
                            ScanEvent::Dock(DockEvent::Unavailable(fault))
                        }
                        DockOutcome::Branch => ScanEvent::Dock(DockEvent::Branch(&self.branch)),
                        DockOutcome::Editable => ScanEvent::Dock(DockEvent::Editable),
                        DockOutcome::Setup(Some(fault)) => ScanEvent::RemoteSetup(fault),
                        DockOutcome::Info(Some(nonce)) => ScanEvent::RemoteUp(nonce),
                        // An unknown code is a newer bootstrap's: nothing to say.
                        DockOutcome::Setup(None) | DockOutcome::Info(None) => return,
                    };
                    on_event(event);
                } else if is_ignored(byte) {
                } else if self.dock.len() == DOCK_PAYLOAD_LIMIT {
                    // Unlike 133's silent drop, the overflow is reported **immediately**: so that the
                    // consumer can say "I cannot show it" (R1.2). The rest of the sequence is still
                    // skipped so that we see what follows it.
                    self.state = ScanState::Skip;
                    on_event(ScanEvent::Dock(DockEvent::Unavailable(DockFault::Overflow)));
                } else {
                    self.dock.push(byte);
                }
            }
            ScanState::Payload(Arm::Cwd) => {
                if is_terminator(byte) {
                    // Decoding happens **before** `close`: `close` empties the buffer.
                    let read = parse_cwd(
                        &self.cwd,
                        self.hostname.as_deref(),
                        &mut self.decoded,
                        &mut self.path,
                    );
                    self.close(byte);
                    if let Some(local) = read {
                        on_event(ScanEvent::Cwd {
                            path: &self.path,
                            local,
                        });
                    }
                } else if is_ignored(byte) {
                } else if self.cwd.len() == CWD_PAYLOAD_LIMIT {
                    // The overflow is **silent**, unlike the mirror's: a directory we cannot show
                    // leaves the old value on screen and the next prompt refreshes it
                    // (`CWD_PAYLOAD_LIMIT`'s doc).
                    self.state = ScanState::Skip;
                } else {
                    self.cwd.push(byte);
                }
            }
            ScanState::Skip => {
                if is_terminator(byte) {
                    self.close(byte);
                }
            }
            // **OSC's framing rules do not apply here and this alone was a source of
            // defects:** `is_terminator` counts `BEL` (0x07) as the end of the sequence,
            // while `vte`'s CSI states `execute` it in place and **do not change** the state —
            // so `ESC [ 2 BEL J` is still an ED 2. Sharing the two sets would split the
            // sequence boundary the grid sees from ours; the two sides would read two
            // different stories from the same stream (module header).
            ScanState::Csi(mut csi) => match byte {
                // **The cancel rules are verbatim from `vte::anywhere`** (R1.3). If they were not
                // carried over, a corrupt CSI would get us stuck and the `ESC ] 133;…` that
                // follows would be swallowed — blocks, suppression and the dock would die
                // **silently**.
                0x18 | 0x1a => self.state = ScanState::Ground,
                0x1b => self.state = ScanState::Escape,
                // C0s inside the sequence are `execute`d in place; the state stays, the
                // parameter is not affected.
                0x00..=0x17 | 0x19 | 0x1c..=0x1f => {}
                b'0'..=b'9' => {
                    csi.param = csi
                        .param
                        .saturating_mul(10)
                        .saturating_add(u32::from(byte - b'0'));
                    csi.has_digit = true;
                    if csi.param > MAX_CSI_PARAM {
                        csi.simple = false;
                    }
                    self.state = ScanState::Csi(csi);
                }
                // Intermediate bytes (0x20–0x2F), parameter separators (`;`, `:`) and private
                // markers (`<=>?`) — the answer for all three is the same: the sequence is not
                // ours, but **the framing continues**. `ESC [ ? 1049 h` neither sets a flag nor
                // gets us stuck.
                0x20..=0x3f => {
                    if byte == b'?' && csi.simple && !csi.has_digit && !csi.private {
                        csi.private = true;
                    } else {
                        csi.simple = false;
                    }
                    self.state = ScanState::Csi(csi);
                }
                0x40..=0x7e => {
                    if csi.is_paste_on(byte) {
                        on_event(ScanEvent::PasteOn);
                    }
                    if csi.is_erase_all(byte) {
                        // **Saturating collection, not wrapping.** The consumer drains the counter on
                        // every read round, so the ceiling is reached only with four billion clears in a
                        // single `read()`; if it wrapped that read would return `0` and say "no clear
                        // happened" — the wrong direction of loss.
                        self.screen_clears = self.screen_clears.saturating_add(1);
                    }
                    self.state = ScanState::Ground;
                }
                // `0x7F` and everything above 0x7F is `anywhere`'s `_ => ()` arm: ignored, the
                // state stays.
                _ => {}
            },
        }
    }

    /// Closes the sequence and moves to the next state according to the terminator
    /// itself: a bare `ESC` ends the sequence **and** opens a new escape
    /// (`vte::advance_osc_string`, the `0x1B` arm).
    fn close(&mut self, terminator: u8) {
        self.payload.clear();
        self.dock.clear();
        self.cwd.clear();
        self.state = if terminator == 0x1b {
            ScanState::Escape
        } else {
            ScanState::Ground
        };
    }
}

/// Diziyi bitiren baytlar (`vte::advance_osc_string`).
fn is_terminator(byte: u8) -> bool {
    matches!(byte, 0x07 | 0x18 | 0x1a | 0x1b)
}

/// C0 bytes ignored inside the sequence (`vte::advance_osc_string`).
fn is_ignored(byte: u8) -> bool {
    matches!(byte, 0x00..=0x06 | 0x08..=0x17 | 0x19 | 0x1c..=0x1f)
}

/// Converts the payload after `133;` into a mark; **ignores** what it does not
/// recognize.
///
/// The first field is the mark itself and must match **exactly**: `A;aid=12` is a
/// `PromptStart`, `AB` is nothing. Shells can attach key-values next to the mark
/// and not knowing them must not mean losing the mark.
fn parse_mark(payload: &[u8]) -> Option<ScanEvent<'static>> {
    let mut fields = payload.split(|&b| b == b';');
    let letter = fields.next()?;
    if !matches!(letter, b"A" | b"B" | b"C" | b"D") {
        return None;
    }
    let mut exit = None;
    let mut id = None;
    let mut remote = None;
    // The code depends on **position** (the first field), the identities on **name** —
    // `BLOCK_ID_FIELD` (`bt_block=`) and `REMOTE_ID_FIELD` (`bt_remote=`); **not** `aid=`
    // (the reason is below, in `block_id`'s doc: a foreign `aid` must not be confused with
    // our counter). The position question is therefore asked after the arms that look at
    // the name: `D;bt_block=7` is a valid payload with an identity but no code, and if we
    // blindly counted the first field as the code it would swallow the identity.
    for (index, field) in fields.enumerate() {
        if let Some(value) = block_id(field) {
            id = Some(value);
        } else if let Some(value) = remote_id(field) {
            remote = Some(value);
        } else if index == 0 && letter == b"D" {
            exit = number(field);
        }
    }
    // **The remote field wins** over a `bt_block=` beside it: such a payload is not
    // our local shell's (it never prints `bt_remote=`), and the remote trail is the
    // side where a wrong mark touches nothing local.
    if remote.is_some() {
        id = None;
    }
    let mark = match letter {
        b"A" => Mark::PromptStart { id },
        b"B" => Mark::PromptEnd,
        b"C" => Mark::CommandStart,
        _ => Mark::CommandEnd { exit, id },
    };
    Some(match remote {
        Some((shell, id)) => ScanEvent::RemoteMark(RemoteMark { shell, id, mark }),
        None => ScanEvent::Mark(mark),
    })
}

/// The authority values that point to this machine — the answer to the "is it
/// local" question, **not an accept list** (036): an OSC 7 with a foreign
/// authority also produces an event and goes to the remote slot
/// ([`ShellLog::apply_scan_answering`]), it just does not write to the local
/// directory.
///
/// **Every named host is counted as foreign** and this was chosen deliberately
/// instead of "compare with our own name": comparison means `gethostname`, which
/// means a new dependency edge for `bt-core` (`proje.md` → Yayın etkisi: a new
/// dependency is an architectural decision). Our own script therefore prints with
/// an **empty authority** (`file:///…`), so the gate is never tied to a name
/// match — it does not silently close when the machine is renamed.
///
/// **The machine's name is the second arm** (044): `bt-shell` (which has `libc`)
/// reads the name and passes it via `SessionOptions::hostname` — the precedent is
/// `decide_locale` — so third-party hooks that print `file://$HOST$PWD` (like
/// oh-my-zsh's `termsupport.zsh`) and GNU `ls --hyperlink`'s `file://$HOSTNAME/…`
/// links count as local. Without a name (`None`) every named host is foreign as
/// before. The single answer is [`is_local_authority`]; OSC 7 and the link hit
/// test (`Session::link_at`) both ask it.
const LOCAL_AUTHORITIES: [&str; 2] = ["", "localhost"];

/// Whether a `file://` authority points to this machine: empty, `localhost`
/// ([`LOCAL_AUTHORITIES`]) or — if the application supplied it — the machine's
/// name, case-insensitively. An empty name counts as no name.
pub(crate) fn is_local_authority(authority: &str, hostname: Option<&str>) -> bool {
    LOCAL_AUTHORITIES
        .iter()
        .any(|local| authority.eq_ignore_ascii_case(local))
        || hostname.is_some_and(|name| !name.is_empty() && authority.eq_ignore_ascii_case(name))
}

/// Converts the URI after `7;` into a drawable path and returns whether the
/// authority is local; **ignores** what it does not recognize (`None`).
///
/// **The payload is not split into fields** (unlike [`parse_mark`] and
/// [`parse_dock`]): `;` is a valid character in a file name and if we split the
/// payload we would read the path `/tmp/a;b` as `/tmp/a`.
///
/// The result of every rejected state is the same and **ignoring, not panicking**
/// (`CLAUDE.md` → PTY yolunda panik yok): the scheme is not `file:`, the authority
/// is not UTF-8, the path does not start with `/`, a percent escape is corrupt or
/// the result is not UTF-8. A foreign authority is not a rejection (036): the
/// answer is `Some(false)`.
fn parse_cwd(
    payload: &[u8],
    hostname: Option<&str>,
    decoded: &mut Vec<u8>,
    into: &mut String,
) -> Option<bool> {
    // The scheme is case-insensitive (RFC 3986 §3.1); `file:` is five bytes.
    let rest = payload
        .get(..5)
        .filter(|head| head.eq_ignore_ascii_case(b"file:"))?;
    let rest = &payload[rest.len()..];
    // The authority section is **mandatory**: the `file:/tmp` form is a valid URI but
    // accepting it would collapse "no authority" and "empty authority" into one arm,
    // and in practice no shell prints it.
    let rest = rest.strip_prefix(b"//".as_slice())?;
    let at = rest.iter().position(|&b| b == b'/')?;
    let (authority, path) = rest.split_at(at);
    let authority = std::str::from_utf8(authority).ok()?;
    let local = is_local_authority(authority, hostname);

    decoded.clear();
    decode_percent(path, decoded)?;
    let text = std::str::from_utf8(decoded).ok()?;
    into.clear();
    into.push_str(text);
    Some(local)
}

/// Decodes percent escapes; `%` **must come with two hex digits**.
///
/// Passing a corrupt escape through as it is was an option too and was rejected: a
/// path carrying `%zz` says either the encoder is broken or the payload is not
/// ours; in both the right answer is not to show the path at all.
fn decode_percent(input: &[u8], out: &mut Vec<u8>) -> Option<()> {
    let mut rest = input;
    while let Some((&byte, tail)) = rest.split_first() {
        if byte != b'%' {
            out.push(byte);
            rest = tail;
            continue;
        }
        let digits = tail.get(..2)?;
        let high = char::from(digits[0]).to_digit(16)?;
        let low = char::from(digits[1]).to_digit(16)?;
        // audit: two hex digits are at most 0xff; fits in `u8`.
        out.push((high * 16 + low) as u8);
        rest = &tail[2..];
    }
    Some(())
}

/// The field name that carries the block identity, **ours alone**.
///
/// **NOT `aid`** and this distinction is critical: `aid` is defined in the
/// semantic-prompts specification, means "application id" and generally carries a
/// **pid** — that is, *constant* for the whole session, not a counter that
/// increases per prompt like ours. If we read it as an identity, any
/// specification-compliant integration (the user's own rc, a nested REPL, the far
/// side of SSH) would print the same value at every prompt, the ledger would see it
/// as non-contiguous and **delete itself**; and a `D;code;aid=pid` falling into the
/// range would overwrite our block's color with someone else's code — exactly the
/// "wrong color" that was the reason A′ was rejected. A foreign `aid` is therefore
/// **ignored** as before.
const BLOCK_ID_FIELD: &[u8] = b"bt_block=";

/// The value of the `bt_block={number}` field; any other field is `None`.
fn block_id(field: &[u8]) -> Option<u32> {
    number(field.strip_prefix(BLOCK_ID_FIELD)?)
}

/// Our remote shell's identity field (048 phase-3): `bt_remote=<P>.<S>.<n>`
/// ([`RemoteShell`] and the remote counter). A separate name, not `bt_block=`:
/// the local gates read that one as the local shell's.
const REMOTE_ID_FIELD: &[u8] = b"bt_remote=";

/// The shell and block of a `bt_remote=<P>.<S>.<n>` field; any other field is
/// `None`. The same three numbers as the anchor's path ([`remote_key`]).
fn remote_id(field: &[u8]) -> Option<(RemoteShell, u32)> {
    let value = std::str::from_utf8(field.strip_prefix(REMOTE_ID_FIELD)?).ok()?;
    remote_key(value)
}

/// `<P>.<S>.<n>` → the remote shell and its block: the one reading of the mark's
/// field and of the anchor's path (`bateri://rblock/<P>.<S>.<n>`, the session's
/// `block_key`) — if the two diverged no anchor would find its mark.
pub(crate) fn remote_key(value: &str) -> Option<(RemoteShell, u32)> {
    let mut parts = value.split('.');
    let parent = parts.next()?.parse().ok()?;
    let pid = parts.next()?.parse().ok()?;
    let id = parts.next()?.parse().ok()?;
    parts
        .next()
        .is_none()
        .then_some((RemoteShell { parent, pid }, id))
}

fn number<T: std::str::FromStr>(field: &[u8]) -> Option<T> {
    std::str::from_utf8(field).ok()?.parse().ok()
}

/// [`parse_dock`]'s three results; [`Scanner::step`] converts them to an event.
///
/// A separate type, because `parse_dock` **cannot produce** a `DockEvent`: the
/// `Update` variant lends `line` and the function holds it `&mut`.
enum DockOutcome {
    Update,
    End,
    Unavailable(DockFault),
    Branch,
    Editable,
    /// `f`: the remote bootstrap's fault code (048); `None` for an unknown code.
    Setup(Option<RemoteSetupFault>),
    /// `i`: the remote bootstrap's information (049); `Some(nonce)` for a
    /// well-formed `up`, `None` for anything else (a newer bootstrap's word, a
    /// malformed nonce) — nothing to say, and no fault of the mirror's.
    Info(Option<String>),
}

/// Decodes the mirror payload and writes into `line`.
///
/// **Wire format** (fields separated by `;`, bodies base64):
///
/// ```text
/// ESC ] 8133 ; u ; {CURSOR} ; {PREDISPLAY} ; {BUFFER} ; {POSTDISPLAY} ; {region_highlight} BEL
/// ESC ] 8133 ; e BEL
/// ESC ] 8133 ; o BEL
/// ESC ] 8133 ; b ; {branch} BEL
/// ESC ] 8133 ; w BEL
/// ESC ] 8133 ; f ; {code} BEL
/// ESC ] 8133 ; i ; up ; {nonce} BEL
/// ```
///
/// `u` refreshes the line, `e` (`line-finish`) closes it, `o` is the shell saying
/// "this display does not fit the mirror", and `b` carries the branch in the dock's
/// context line. `w` (031) says "the editing widget is bound at this prompt": the
/// precondition of the only sequence the terminal sends to the shell
/// (`CSI 8133 ~`); it has no payload and does not touch the mirror's state, like
/// `b`. `f` (048) is printed by the **remote** bootstrap, not the local wrapper:
/// the integration did not start on the server and `{code}` (plain ASCII, one of
/// [`RemoteSetupFault::from_code`]'s) says why. It touches neither the mirror nor
/// the dock — and an unknown code is no fault of the mirror's either. `i`
/// (049) is the remote bootstrap too: `up` is its first output and `{nonce}`
/// the attempt's (lowercase hex, at most [`NONCE_LIMIT`] digits — the form is
/// checked, the content is the pane's to match); any other `i` is silent.
///
/// **`b` is on the mirror's channel but is not part of the mirror:** it arrives
/// **per prompt** (`precmd`), not per keystroke, and does not touch the line's
/// state. It does not deserve its own OSC number — unlike the directory
/// (`CWD_OSC`) there is no contract for the branch, so a new number would only be a
/// second channel printed by our script alone. **Extra fields are ignored** — the
/// same reason as [`parse_mark`] tolerating unknown key-values: phase-4's special
/// mode signal should be addable without reopening this parser.
///
/// **Why base64:** the bodies are text the user typed, so they can contain `;`,
/// `ESC` and C0 bytes — all three break the sequence's framing. base64's alphabet
/// has none of the three, so the framing rules (the three points above) never
/// touch the body. The encoding side is pure zsh; no fork.
///
/// `region_highlight` records are separated by line breaks inside the body.
///
/// On a corrupt payload `line` is **emptied**: a half-written record is spread
/// nowhere (`Malformed` → [`ShellLog::apply_dock`] already resets), but leaving the
/// buffer dirty would turn the next read into reasoning debt.
fn parse_dock(
    payload: &[u8],
    decoded: &mut Vec<u8>,
    line: &mut DockState,
    branch: &mut String,
) -> DockOutcome {
    let mut fields = payload.split(|&b| b == b';');
    let Some(op) = fields.next() else {
        return unavailable(line, DockFault::Malformed);
    };
    match op {
        b"e" => DockOutcome::End,
        // The branch's corruption does not drop the mirror: `Unavailable` means "I cannot
        // show the input line" and brings the grid into play, while an unreadable branch
        // is just half of the context line. A corrupt body **empties** the branch — a line
        // without a branch rather than showing a wrong branch.
        //
        // **The field's absence goes through the same gate** (the difference between `b`
        // and `b;` is an encoder detail and both mean "no branch"). This arm once returned
        // `Malformed` and that was exactly what the sentence above forbids: a truncated
        // `b` sequence dropped the input line from the dock and sent it back to the grid
        // (`/code-review`, 012 phase-6).
        b"b" => {
            branch.clear();
            if let Some(field) = fields.next() {
                decoded.clear();
                if decode_base64(field, decoded).is_some()
                    && let Ok(text) = std::str::from_utf8(decoded)
                {
                    branch.push_str(text);
                }
            }
            DockOutcome::Branch
        }
        // **The shell-side end of the overflow.** While [`DOCK_PAYLOAD_LIMIT`] cuts the
        // payload here, the shell has already **encoded** it; `o` says it measured before
        // encoding. The two are the two sides of the same budget and each is necessary
        // separately: this end protects the time the shell spends per keystroke, the other
        // end our memory. Their result **must** be the same, otherwise which side the
        // bound is held on would show up to the user as different behavior.
        b"o" => unavailable(line, DockFault::Overflow),
        b"w" => DockOutcome::Editable,
        b"f" => DockOutcome::Setup(fields.next().and_then(RemoteSetupFault::from_code)),
        b"i" => DockOutcome::Info(match (fields.next(), fields.next(), fields.next()) {
            (Some(b"up"), Some(nonce), None) => remote_nonce(nonce),
            _ => None,
        }),
        b"u" => match decode_line(&mut fields, decoded, line) {
            Some(()) => DockOutcome::Update,
            // The state is written too: `decode_line` says `Live` on its very first line, and
            // if a half-finished decode leaves it as is, the scanner's buffer would carry
            // empty text under the name "live".
            None => unavailable(line, DockFault::Malformed),
        },
        _ => unavailable(line, DockFault::Malformed),
    }
}

/// The longest nonce `8133;i;up` carries (049): the bootstrap's is 16 hex
/// digits (`bt-shell-common::ssh_wrap::NONCE_LEN`); the bound only keeps a
/// foreign sequence from making a long string.
pub(crate) const NONCE_LIMIT: usize = 64;

/// `up`'s nonce field → the nonce: lowercase hex, `1..=`[`NONCE_LIMIT`]
/// digits; anything else is `None`.
fn remote_nonce(field: &[u8]) -> Option<String> {
    (!field.is_empty()
        && field.len() <= NONCE_LIMIT
        && field.iter().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
    .then(|| String::from_utf8_lossy(field).into_owned())
}

/// Empties the buffer, writes the state and returns the result.
///
/// All three callers have to do the same thing: if the text of a line we cannot
/// show stays in the buffer, the next read has to answer the "is this text fresh"
/// question by reasoning.
fn unavailable(line: &mut DockState, fault: DockFault) -> DockOutcome {
    line.reset();
    line.status = DockStatus::Unavailable(fault);
    DockOutcome::Unavailable(fault)
}

/// The clustered form of [`DockState::last_ink`]: the first character of the last
/// inked **cluster** of the display's last line. Clusters can only be walked
/// forward ([`crate::cluster::Walk`]), so what comes after the last `\n` is
/// scanned from the start — the answer is reset at the line end.
fn last_cluster_ink(line: &DockState) -> Option<char> {
    let display = line
        .predisplay
        .chars()
        .chain(line.buffer.chars())
        .chain(line.postdisplay.chars());
    let mut ink = None;
    crate::cluster::Walk::new().run(display, |cluster| {
        if cluster.head == '\n' {
            ink = None;
        } else if cluster.head != ' ' && cluster.head != '\t' && cluster.width > 0 {
            ink = Some(cluster.head);
        }
    });
    ink
}

/// Decodes the fields of the `u` payload into `line`; `None` if one of the five
/// mandatory ones is missing or any text body is corrupt. The last two (`KEYMAP`,
/// `PREBUFFER`) are optional.
fn decode_line<'a>(
    fields: &mut impl Iterator<Item = &'a [u8]>,
    decoded: &mut Vec<u8>,
    line: &mut DockState,
) -> Option<()> {
    line.status = DockStatus::Live;
    // The cursor field comes first in wire order but its normalization waits until
    // `PREDISPLAY` is decoded.
    let cursor_in_buffer: usize = number(fields.next()?)?;
    decode_text(fields.next()?, decoded, &mut line.predisplay)?;
    decode_text(fields.next()?, decoded, &mut line.buffer)?;
    decode_text(fields.next()?, decoded, &mut line.postdisplay)?;

    // All three lengths are here: folding the offsets into a single space
    // ([`Highlight::start`]) and clamping an offset that overflows the display need
    // them.
    let predisplay_chars = line.predisplay.chars().count();
    let text_chars = predisplay_chars + line.buffer.chars().count();
    let display_chars = text_chars + line.postdisplay.chars().count();
    // `$CURSOR` is at most `$#BUFFER`; the clamp is so as not to trust the shell's word.
    line.cursor = predisplay_chars
        .checked_add(cursor_in_buffer)?
        .min(text_chars);
    line.display_chars = display_chars;
    // The last non-blank character of the display's **last line**, from the end; the
    // three bodies in display order.
    //
    // **The last line, not the last character** (032): the other half of the gate
    // scans **one** row of the grid — suppression's lower end, that is the display's
    // last line. In a paste of `echo a\necho b\n`, zsh keeps the final line break in
    // the buffer and that line is **empty**; if the mirror said `'b'` (or `'\n'` —
    // which used to pass the old filter because its width is `None`) it would never
    // match the grid's empty line and in every unanswered frame the line would be
    // counted stale. If what comes after the last `\n` is empty, `None`, the same
    // answer as the grid's empty line. Wrapping does not break this: the last
    // character of a wrapped line is on the last visual line.
    //
    // **Known limit, safe direction** (032 phase-4): on a `PS2` line the grid carries
    // the user's `for> ` ink, the mirror does not (`PS2` is not touched and its width
    // is not in the mirror). When `BUFFER` is empty the two sides diverge and in an
    // unanswered frame the content gate says "stale"; the temporal gate (`line-init`'s
    // mirror is ⏎'s answer) rescues as it does today, and at the moment it cannot
    // rescue (a key without redisplay) the line is visible in two places.
    //
    // **With clustering on (035) the criterion is the cluster's first character**:
    // the grid keeps `👍🏽` in a single cell and the cell's `c` is `👍`; if the mirror
    // said `🏽` the gate would bring back 024's symptom — the line leaps to the grid on
    // every keystroke. The three criteria below as they are: a combining mark joining
    // a cluster is already not counted separately, a headless combining mark (column
    // zero) is skipped.
    line.last_ink = if line.cluster {
        last_cluster_ink(line)
    } else {
        line.predisplay
            .chars()
            .chain(line.buffer.chars())
            .chain(line.postdisplay.chars())
            .rev()
            .take_while(|&ch| ch != '\n')
            // **The criterion is `' '` and `'\t'`; not `is_whitespace()`** and this is
            // deliberately narrow: the other half of the gate scans the grid
            // (`Session::last_ink_in_row`) and that is pinned to `frame()`'s skip gate —
            // there inklessness is only blank, spacer and hidden cell. If we said
            // `is_whitespace()`, in a buffer ending with NBSP (U+00A0, U+2007, U+3000) the
            // mirror would say the previous letter and the grid the NBSP, the two would never
            // match and the line would be counted permanently **stale**: drawn both on the
            // grid and in the dock.
            //
            // **Tab is the reverse and was measured** (user, 2026-09-18): the mirror carries
            // the **raw** buffer while the grid holds the **drawn** state. Since the terminal
            // expands the tab into blanks, that character never reaches the cell — in a real
            // zsh, Tab on an empty line makes `BUFFER='\t'`, so the mirror says `Some('\t')`,
            // the grid says `None` and the gate dropped. The symptom was visible: suppression
            // lifted and the caret leapt from the dock to the grid. Counting the tab as
            // inkless as well equalizes the two halves again — `"ls\t"` is `'s'` in both,
            // `"\t"` is `None` in both.
            //
            // **A raw control character never comes to this comparison** (025): ZLE draws
            // `\x01` as `^A` on the grid and the dock does not draw it at all, so that line
            // stays on the grid with [`DockStatus::Control`] and is not suppressed. It was once
            // written here as a "remaining limit" and was worse than written: only when `^A`
            // was the **last** character did the gate drop, when it was in the middle the line
            // went to the dock and vanished.
            //
            // **The third criterion is zero width and it came in 024** (the user reported,
            // measured): combining code points (VS16, ZWJ, skin tone) **never enter** the grid
            // cell — alacritty keeps them in `CellExtra` and `cell.c` carries the base
            // character. So in a buffer with `❤️` (U+2764 + U+FE0F) the mirror says `U+FE0F`,
            // the grid `U+2764` and the two **never** match: the gate says "stale" permanently,
            // suppression lifts on every keystroke and the input line leaps from the dock to
            // the grid. The same sentence as the tab's reason above — the mirror carries the
            // **raw** buffer, the grid the **drawn** state — and the remedy is the same too:
            // the mirror does not count a character that does not reach the grid either.
            //
            // The criterion is `unicode-width`'s `Some(0)`, that is the very source
            // `dock::column_width` is fed from. Control characters return `None` and **do not
            // enter** this filter: the line carrying them is already `Control`.
            .find(|ch| *ch != ' ' && *ch != '\t' && UnicodeWidthChar::width(*ch) != Some(0))
    };

    decoded.clear();
    decode_base64(fields.next()?, decoded)?;
    let entries = std::str::from_utf8(decoded).ok()?;
    line.highlights.clear();
    line.highlights.extend(
        entries
            .lines()
            .filter_map(|entry| parse_highlight(entry, predisplay_chars, display_chars)),
    );

    // KEYMAP is an **optional field** and this is the reverse direction of the wire's
    // "extra fields are ignored" rule: the field was added at phase-6's gate and an
    // open window may still be running with the old script (`plan.md` → Göç). Its
    // absence does not corrupt the payload, it just leaves `false` — that is, the
    // paste returns to the wrapped path. The direction is safe: missing information
    // **closes** the exception, it does not open it.
    line.insert_keymap = fields.next().is_some_and(|field| {
        decoded.clear();
        decode_base64(field, decoded).is_some()
            && std::str::from_utf8(decoded).is_ok_and(|name| INSERT_KEYMAPS.contains(&name))
    });
    // PREBUFFER is the **seventh, optional body** (032) and sits behind `KEYMAP`,
    // because the wire can only grow at the end: a window running with an old script
    // never sends it and its absence does not corrupt the payload, it leaves it empty.
    // A corrupt body is under the same rule as the other text bodies — the payload is
    // corrupt.
    //
    // It **does not enter** the display space: the three lengths above and the last
    // ink do not see it (zsh's `CURSOR` and `region_highlight` do not see it either).
    line.prebuffer.clear();
    if let Some(field) = fields.next() {
        decode_text(field, decoded, &mut line.prebuffer)?;
    }

    // **A control character the dock does not draw** ([`DockStatus::Control`]). Tab is
    // an exception and its reason is in the arm's doc: it carries no information. **A
    // line break is an exception too** (032): the dock breaks lines, so it can show it
    // — until 032 a display with line breaks stayed on the grid in its own arm
    // (`Multiline`). `PREBUFFER` is also asked, because the dock draws it too.
    if line
        .prebuffer
        .chars()
        .chain(line.predisplay.chars())
        .chain(line.buffer.chars())
        .chain(line.postdisplay.chars())
        .any(|ch| ch.is_control() && ch != '\t' && ch != '\n')
    {
        line.status = DockStatus::Control;
    }
    Some(())
}

/// The zsh keymaps in which a pressed key **turns into text**.
///
/// The list is an **allow list** and has to be: a keymap we do not recognize (one
/// the user created with `bindkey -N`, or one zsh adds in the future) is **not
/// counted** as an insert keymap and the paste goes the wrapped way. If it were a
/// deny list every new keymap name would silently enter the exception.
///
/// All three say the same thing but by three different routes: `main` is an alias
/// for zsh's active binding (the value reported both in emacs mode and in vi's
/// **insert** mode), `emacs` and `viins` are the directly named states. The ones
/// left out: `vicmd` (the keys are commands), `visual`, `viopp`, `isearch` and
/// `command` — in none of them does a pressed byte turn into text.
const INSERT_KEYMAPS: [&str; 3] = ["main", "emacs", "viins"];

/// Decodes a base64 field and writes into `into`, **keeping its capacity**.
fn decode_text(field: &[u8], decoded: &mut Vec<u8>, into: &mut String) -> Option<()> {
    decoded.clear();
    decode_base64(field, decoded)?;
    let text = std::str::from_utf8(decoded).ok()?;
    into.clear();
    into.push_str(text);
    Some(())
}

/// One record of `region_highlight`: `[P]{start} {end} {spec} [memo=…]`.
///
/// `memo=` and unrecognized tail fields are ignored (`zshzle(1)` leaves them free).
///
/// **The range is clamped to `display_chars` and what ends up empty is dropped.**
/// Carrying an offset that points outside the text to the drawing side would create
/// a `panic` (or silent clamping) debt there, and an overflowing offset is not
/// hypothetical: any plugin that builds `region_highlight` from a stale `BUFFER`
/// snapshot produces it. A reversed range drops through the same gate.
fn parse_highlight(
    entry: &str,
    predisplay_chars: usize,
    display_chars: usize,
) -> Option<Highlight> {
    let mut parts = entry.split_whitespace();
    let first = parts.next()?;
    // The `P` prefix ties the offset to the start of `PREDISPLAY`; the unprefixed one to `BUFFER`'s.
    let (start_text, shift) = match first.strip_prefix('P') {
        Some(rest) => (rest, 0),
        None => (first, predisplay_chars),
    };
    let start = start_text
        .parse::<usize>()
        .ok()?
        .checked_add(shift)?
        .min(display_chars);
    let end = parts
        .next()?
        .parse::<usize>()
        .ok()?
        .checked_add(shift)?
        .min(display_chars);
    let style = parse_style(parts.next()?);
    (start < end).then_some(Highlight { start, end, style })
}

/// zsh's named colors, in `HighlightColor::Indexed` order.
const HIGHLIGHT_COLOR_NAMES: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

/// Converts a spec like `fg=red,bold` into a style; an unrecognized component drops.
fn parse_style(spec: &str) -> HighlightStyle {
    let mut style = HighlightStyle::default();
    for part in spec.split(',') {
        match part {
            "bold" => style.bold = true,
            "underline" => style.underline = true,
            "standout" => style.standout = true,
            _ => {
                if let Some(value) = part.strip_prefix("fg=") {
                    style.fg = parse_highlight_color(value);
                } else if let Some(value) = part.strip_prefix("bg=") {
                    style.bg = parse_highlight_color(value);
                }
            }
        }
    }
    style
}

/// `#rrggbb`, `0`–`255` or a named color; `default` and unrecognized → `None`.
fn parse_highlight_color(value: &str) -> Option<HighlightColor> {
    if let Some(hex) = value.strip_prefix('#') {
        return (hex.len() == 6)
            .then(|| u32::from_str_radix(hex, 16).ok())
            .flatten()
            .map(HighlightColor::Rgb);
    }
    if let Ok(index) = value.parse::<u8>() {
        return Some(HighlightColor::Indexed(index));
    }
    let at = HIGHLIGHT_COLOR_NAMES
        .iter()
        .position(|&name| name == value)?;
    Some(HighlightColor::Indexed(at as u8))
}

/// The table's counterpart of a byte that is not in the base64 alphabet.
const B64_INVALID: u8 = 0xff;

/// The `byte → 6 bits` decode table; every byte outside the alphabet is [`B64_INVALID`].
const B64_DECODE: [u8; 256] = {
    let mut table = [B64_INVALID; 256];
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut at = 0;
    while at < alphabet.len() {
        table[alphabet[at] as usize] = at as u8;
        at += 1;
    }
    table
};

/// Decodes base64 into `out`; on corrupt input `None` and `out` may be left
/// half-written (the caller does not use it).
///
/// **Written by hand:** a base64 crate is an architectural decision (`proje.md` →
/// Yayın etkisi) and this phase does not open it; a table + `chunks_exact` is thirty
/// lines.
///
/// **Padding is optional.** The encoding side is pure zsh and an implementation that
/// prints no padding also produces valid base64; to require padding would tie the
/// channel to an implementation detail of the encoder. After padding the body
/// length must be divisible by 4 or leave a remainder of 2/3 — a remainder of 1 is
/// not base64.
fn decode_base64(input: &[u8], out: &mut Vec<u8>) -> Option<()> {
    let body = match input {
        [rest @ .., b'=', b'='] => rest,
        [rest @ .., b'='] => rest,
        rest => rest,
    };
    let mut chunks = body.chunks_exact(4);
    for chunk in chunks.by_ref() {
        let a = b64_value(chunk[0])?;
        let b = b64_value(chunk[1])?;
        let c = b64_value(chunk[2])?;
        let d = b64_value(chunk[3])?;
        // The masks are **mandatory**, not decoration: shifting a six-bit value without a
        // mask overflows a `u8` and panics in debug — there is no unjustified panic in
        // `bt-core` (`CLAUDE.md`).
        out.push((a << 2) | (b >> 4));
        out.push(((b & 0x0f) << 4) | (c >> 2));
        out.push(((c & 0x03) << 6) | d);
    }
    match chunks.remainder() {
        [] => Some(()),
        [a, b] => {
            let (a, b) = (b64_value(*a)?, b64_value(*b)?);
            out.push((a << 2) | (b >> 4));
            Some(())
        }
        [a, b, c] => {
            let (a, b, c) = (b64_value(*a)?, b64_value(*b)?, b64_value(*c)?);
            out.push((a << 2) | (b >> 4));
            out.push(((b & 0x0f) << 4) | (c >> 2));
            Some(())
        }
        // A single leftover is not base64: six bits do not make a byte.
        _ => None,
    }
}

fn b64_value(byte: u8) -> Option<u8> {
    match B64_DECODE[byte as usize] {
        B64_INVALID => None,
        value => Some(value),
    }
}

// ─── the handover's state blob (055) ─────────────────────────────────────

/// The state blob's first word; the version follows it after one space.
const STATE_HEADER: &str = "bateri-state";

/// The state blob's version (055 R1.4): the old bateri writes it, the new one
/// reads it across an update. A change of the format increments it.
const STATE_VERSION: u32 = 1;

/// The oldest version the reader still takes: the current one and the one
/// before it (`.tasks/055-guncellemede-canli-devir/discussion.md` → Karar 8).
/// While the format has one version the two are the same.
const STATE_OLDEST: u32 = if STATE_VERSION > 1 {
    STATE_VERSION - 1
} else {
    STATE_VERSION
};

/// One block trail as the blob carries it ([`BlockTrack`]).
#[derive(Clone, Debug, PartialEq, Eq)]
struct CarriedTrack {
    state: Option<ShellState>,
    /// How long the running command has run, in milliseconds — an `Instant`
    /// does not cross processes; the new side subtracts it from its own now.
    running_ms: Option<u64>,
    /// The identity of `entries[0]`.
    first: u32,
    entries: Vec<Outcome>,
}

impl CarriedTrack {
    fn of(track: &BlockTrack) -> Self {
        Self {
            state: track.state,
            running_ms: track
                .running_since
                .map(|since| u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)),
            first: track.blocks.first,
            entries: track.blocks.entries.iter().copied().collect(),
        }
    }

    /// Into `track`, whose ledger keeps its own ceiling: entries over it are
    /// dropped from the oldest, the ring's own rule.
    fn restore(self, track: &mut BlockTrack) {
        track.state = self.state;
        track.running_since = self
            .running_ms
            .and_then(|ms| Instant::now().checked_sub(Duration::from_millis(ms)));
        track.blocks.entries = self.entries.into();
        track.blocks.first = self.first;
        while track.blocks.entries.len() > track.blocks.capacity {
            track.blocks.entries.pop_front();
            track.blocks.first = track.blocks.first.wrapping_add(1);
        }
    }

    fn render(&self) -> String {
        let (phase, exit) = match self.state {
            None => ("-", "-".to_owned()),
            Some(state) => (
                match state.phase {
                    ShellPhase::Prompt => "prompt",
                    ShellPhase::Input => "input",
                    ShellPhase::Running => "running",
                    ShellPhase::Finished => "finished",
                },
                render_number(state.last_exit),
            ),
        };
        let entries = if self.entries.is_empty() {
            "-".to_owned()
        } else {
            let mut out = String::new();
            for (index, entry) in self.entries.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                match entry {
                    Outcome::Pending => out.push('p'),
                    Outcome::Finished { exit, elapsed_ms } => {
                        let _ = write!(out, "{}/{elapsed_ms}", render_number(*exit));
                    }
                }
            }
            out
        };
        format!(
            "{phase} {exit} {} {} {entries}",
            render_number(self.running_ms),
            self.first
        )
    }

    fn parse(fields: &[&str]) -> Option<Self> {
        let [phase, exit, running, first, entries] = fields else {
            return None;
        };
        let phase = match *phase {
            "-" => None,
            "prompt" => Some(ShellPhase::Prompt),
            "input" => Some(ShellPhase::Input),
            "running" => Some(ShellPhase::Running),
            "finished" => Some(ShellPhase::Finished),
            _ => return None,
        };
        let last_exit = parse_number(exit)?;
        let state = match phase {
            Some(phase) => Some(ShellState { phase, last_exit }),
            None if last_exit.is_none() => None,
            None => return None,
        };
        let entries = if *entries == "-" {
            Vec::new()
        } else {
            entries
                .split(',')
                .map(|entry| {
                    if entry == "p" {
                        return Some(Outcome::Pending);
                    }
                    let (exit, elapsed) = entry.split_once('/')?;
                    Some(Outcome::Finished {
                        exit: parse_number(exit)?,
                        elapsed_ms: elapsed.parse().ok()?,
                    })
                })
                .collect::<Option<Vec<_>>>()?
        };
        Some(Self {
            state,
            running_ms: parse_number(running)?,
            first: first.parse().ok()?,
            entries,
        })
    }
}

/// What the handover carries of `bt-core`'s own state (055,
/// `.tasks/055-guncellemede-canli-devir/discussion.md` → Karar 4): the
/// ledgers, the context, the last mirror and the generations the remote
/// gates read. **Not carried:** the transient interface state (the dock's
/// selection, vertical window and prediction; search; the mouse selection;
/// the scroll fraction), the remote target (the process table finds it
/// again — a carried copy would be a second source of truth), the remote
/// mark (resolved again from the host rules) and the title (the VT
/// snapshot carries it, through the listener).
///
/// Beyond Karar 4's list, [`ShellLog::ours`] and [`ShellLog::command_open`]:
/// without them the foreign-mark gate is off until our next mark, and a
/// remote fish's or kitty's `A` would clear `⇄ host` under a running ssh.
#[derive(Debug, PartialEq)]
pub(crate) struct Carried {
    local: CarriedTrack,
    remote: CarriedTrack,
    remote_shell: Option<RemoteShell>,
    cwd: String,
    branch: String,
    remote_cwd: String,
    remote_setup: Option<RemoteSetupFault>,
    /// The reconnect offer's host and line; its mark is resolved again.
    reconnect: Option<(String, String)>,
    /// `answers` and `cluster` are not carried: the new session stamps the
    /// mirror with its own input generation ([`Self::dock_fresh`]) and reads
    /// it with its own cluster setting.
    dock: DockState,
    /// Whether the mirror answered the last input before the handover — the
    /// freshness gate's temporal half ([`DockState::answers`]). A freeze
    /// between a key and its mirror (the `bracketed-paste-magic` arm sends
    /// none until the next key) must cross as stale, not as fresh.
    dock_fresh: bool,
    dock_editable: bool,
    command: u64,
    login: Option<u64>,
    remote_up: Option<(u64, String)>,
    typed: Option<u64>,
    ours: bool,
    command_open: bool,
    /// The deliberate clear's flag with its stamp (`Session`'s
    /// `screen_cleared` and `screen_clear_history`): `Some` while set. The
    /// stamp is a scrollback length, and the replay rebuilds the same one.
    pub(crate) cleared: Option<usize>,
}

impl Carried {
    /// The blob: the header line, then one `key fields…` line per field, one
    /// `hl` line per highlight and `end`. Text is escaped so that a field never
    /// holds a space or a line break (`+{escaped}`, `-` for none — the
    /// `restore` format's rule, 053).
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut out = format!("{STATE_HEADER} {STATE_VERSION}\n");
        let dock = &self.dock;
        let status = match dock.status {
            DockStatus::Idle => "idle",
            DockStatus::Live => "live",
            DockStatus::Control => "control",
            DockStatus::Unavailable(DockFault::Overflow) => "overflow",
            DockStatus::Unavailable(DockFault::Malformed) => "malformed",
        };
        let mut ink = [0; 4];
        let lines = [
            format!("local {}", self.local.render()),
            format!("remote {}", self.remote.render()),
            format!(
                "remote-shell {}",
                self.remote_shell.map_or("-".to_owned(), |shell| format!(
                    "{}.{}",
                    shell.parent, shell.pid
                ))
            ),
            format!("cwd {}", render_text(Some(&self.cwd))),
            format!("branch {}", render_text(Some(&self.branch))),
            format!("remote-cwd {}", render_text(Some(&self.remote_cwd))),
            format!(
                "remote-setup {}",
                self.remote_setup.map_or("-", RemoteSetupFault::code)
            ),
            match &self.reconnect {
                None => "reconnect -".to_owned(),
                Some((host, line)) => format!(
                    "reconnect {} {}",
                    render_text(Some(host)),
                    render_text(Some(line))
                ),
            },
            format!(
                "dock {status} {} {} {} {}",
                dock.cursor,
                dock.display_chars,
                render_text(dock.last_ink.map(|ch| &*ch.encode_utf8(&mut ink))),
                u8::from(dock.insert_keymap)
            ),
            format!("predisplay {}", render_text(Some(&dock.predisplay))),
            format!("buffer {}", render_text(Some(&dock.buffer))),
            format!("postdisplay {}", render_text(Some(&dock.postdisplay))),
            format!("prebuffer {}", render_text(Some(&dock.prebuffer))),
            format!("dock-fresh {}", u8::from(self.dock_fresh)),
            format!("editable {}", u8::from(self.dock_editable)),
            format!("command {}", self.command),
            format!("login {}", render_number(self.login)),
            match &self.remote_up {
                None => "remote-up -".to_owned(),
                Some((generation, nonce)) => {
                    format!("remote-up {generation} {}", render_text(Some(nonce)))
                }
            },
            format!("typed {}", render_number(self.typed)),
            format!("ours {}", u8::from(self.ours)),
            format!("command-open {}", u8::from(self.command_open)),
            format!("cleared {}", render_number(self.cleared)),
        ];
        for line in lines {
            out.push_str(&line);
            out.push('\n');
        }
        for highlight in &dock.highlights {
            let style = highlight.style;
            let flags: String = [
                (style.bold, 'b'),
                (style.underline, 'u'),
                (style.standout, 's'),
            ]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, letter)| *letter)
            .collect();
            let _ = writeln!(
                out,
                "hl {} {} {} {} {}",
                highlight.start,
                highlight.end,
                render_color(style.fg),
                render_color(style.bg),
                if flags.is_empty() { "-" } else { &flags }
            );
        }
        // A cut blob must not read as a shorter one (the highlights are a
        // list): the last line says the blob is whole.
        out.push_str("end\n");
        out.into_bytes()
    }

    /// [`Self::encode`]'s inverse for this version and the one before it.
    /// Strict: an unknown version or key, a missing or repeated key, a
    /// malformed field — `None`, never a panic. A half-read state would put a
    /// wrong caret or a wrong colour on screen; the caller's fallback is a
    /// fresh state.
    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?;
        let mut lines = text.lines();
        let mut header = lines.next()?.split(' ');
        if header.next()? != STATE_HEADER {
            return None;
        }
        let version: u32 = header.next()?.parse().ok()?;
        if !(STATE_OLDEST..=STATE_VERSION).contains(&version) || header.next().is_some() {
            return None;
        }
        let mut fields: Vec<(&str, Vec<&str>)> = Vec::new();
        let mut highlights = Vec::new();
        let mut whole = false;
        for line in lines {
            if whole {
                return None;
            }
            if line == "end" {
                whole = true;
                continue;
            }
            let mut words = line.split(' ');
            let key = words.next()?;
            let rest: Vec<&str> = words.collect();
            if key == "hl" {
                highlights.push(parse_carried_highlight(&rest)?);
            } else if fields.iter().any(|(seen, _)| *seen == key) {
                return None;
            } else {
                fields.push((key, rest));
            }
        }
        if !whole || !text.ends_with('\n') {
            return None;
        }
        let mut take = |key: &str| -> Option<Vec<&str>> {
            let at = fields.iter().position(|(seen, _)| *seen == key)?;
            Some(fields.remove(at).1)
        };
        let one = |values: Vec<&str>| -> Option<String> {
            let [value] = values.as_slice() else {
                return None;
            };
            Some((*value).to_owned())
        };
        let text_of = |values: Vec<&str>| -> Option<String> { parse_text(&one(values)?)? };
        let flag = |values: Vec<&str>| -> Option<bool> {
            match one(values)?.as_str() {
                "0" => Some(false),
                "1" => Some(true),
                _ => None,
            }
        };
        let number = |values: Vec<&str>| -> Option<Option<u64>> { parse_number(&one(values)?) };

        let local = CarriedTrack::parse(&take("local")?)?;
        let remote = CarriedTrack::parse(&take("remote")?)?;
        let remote_shell = match one(take("remote-shell")?)?.as_str() {
            "-" => None,
            shell => {
                let (parent, pid) = shell.split_once('.')?;
                Some(RemoteShell {
                    parent: parent.parse().ok()?,
                    pid: pid.parse().ok()?,
                })
            }
        };
        let cwd = text_of(take("cwd")?)?;
        let branch = text_of(take("branch")?)?;
        let remote_cwd = text_of(take("remote-cwd")?)?;
        let remote_setup = match one(take("remote-setup")?)?.as_str() {
            "-" => None,
            code => Some(RemoteSetupFault::from_code(code.as_bytes())?),
        };
        let reconnect = match take("reconnect")?.as_slice() {
            ["-"] => None,
            [host, line] => Some((parse_text(host)??, parse_text(line)??)),
            _ => return None,
        };
        let dock_fields = take("dock")?;
        let [status, cursor, display_chars, last_ink, insert] = dock_fields.as_slice() else {
            return None;
        };
        let status = match *status {
            "idle" => DockStatus::Idle,
            "live" => DockStatus::Live,
            "control" => DockStatus::Control,
            "overflow" => DockStatus::Unavailable(DockFault::Overflow),
            "malformed" => DockStatus::Unavailable(DockFault::Malformed),
            _ => return None,
        };
        let last_ink = match parse_text(last_ink)? {
            None => None,
            Some(ink) => {
                let mut chars = ink.chars();
                let ch = chars.next()?;
                chars.next().is_none().then_some(ch)?;
                Some(ch)
            }
        };
        let insert_keymap = match *insert {
            "0" => false,
            "1" => true,
            _ => return None,
        };
        let dock = DockState {
            status,
            predisplay: text_of(take("predisplay")?)?,
            buffer: text_of(take("buffer")?)?,
            postdisplay: text_of(take("postdisplay")?)?,
            prebuffer: text_of(take("prebuffer")?)?,
            cursor: cursor.parse().ok()?,
            highlights,
            display_chars: display_chars.parse().ok()?,
            last_ink,
            insert_keymap,
            answers: 0,
            cluster: false,
        };
        let dock_fresh = flag(take("dock-fresh")?)?;
        let dock_editable = flag(take("editable")?)?;
        let command = one(take("command")?)?.parse().ok()?;
        let login = number(take("login")?)?;
        let remote_up = match take("remote-up")?.as_slice() {
            ["-"] => None,
            [generation, nonce] => Some((generation.parse().ok()?, parse_text(nonce)??)),
            _ => return None,
        };
        let typed = number(take("typed")?)?;
        let ours = flag(take("ours")?)?;
        let command_open = flag(take("command-open")?)?;
        let cleared = number(take("cleared")?)?
            .map(usize::try_from)
            .transpose()
            .ok()?;
        if !fields.is_empty() {
            return None;
        }
        Some(Self {
            local,
            remote,
            remote_shell,
            cwd,
            branch,
            remote_cwd,
            remote_setup,
            reconnect,
            dock,
            dock_fresh,
            dock_editable,
            command,
            login,
            remote_up,
            typed,
            ours,
            command_open,
            cleared,
        })
    }
}

impl ShellLog {
    /// What the handover carries ([`Carried`]); `cleared` is `Session`'s,
    /// `answered` its input generation now.
    pub(crate) fn carried(&self, cleared: Option<usize>, answered: u64) -> Carried {
        Carried {
            local: CarriedTrack::of(&self.local),
            remote: CarriedTrack::of(&self.remote),
            remote_shell: self.remote_shell,
            cwd: self.context.cwd.clone(),
            branch: self.context.branch.clone(),
            remote_cwd: self.context.remote_cwd.clone(),
            remote_setup: self.context.remote_setup,
            reconnect: self
                .context
                .reconnect
                .as_ref()
                .map(|offer| (offer.host.clone(), offer.line.clone())),
            dock: self.dock.clone(),
            dock_fresh: self.dock.answers == answered,
            dock_editable: self.dock_editable,
            command: self.command,
            login: self.login,
            remote_up: self.remote_up.clone(),
            typed: self.typed,
            ours: self.ours,
            command_open: self.command_open,
            cleared,
        }
    }

    /// Takes the carried state over a fresh log; `answers` is this
    /// session's input generation — a mirror that answered the last input
    /// before the handover answers it (nothing was typed since), a stale one
    /// is stamped one generation behind and the gate falls back to the
    /// content comparison. The mirror
    /// keeps this session's cluster setting; the reconnect offer's mark is
    /// resolved from this session's host rules.
    pub(crate) fn restore(&mut self, carried: Carried, answers: u64) {
        carried.local.restore(&mut self.local);
        carried.remote.restore(&mut self.remote);
        self.remote_shell = carried.remote_shell;
        self.context.cwd = carried.cwd;
        self.context.branch = carried.branch;
        self.context.remote_cwd = carried.remote_cwd;
        self.context.remote_setup = carried.remote_setup;
        self.context.reconnect = carried.reconnect.map(|(host, line)| Reconnect {
            mark: crate::settings::host_mark(&self.host_rules, &host),
            host,
            line,
        });
        let cluster = self.dock.cluster;
        self.dock = carried.dock;
        self.dock.answers = if carried.dock_fresh {
            answers
        } else {
            answers.wrapping_sub(1)
        };
        self.dock.cluster = cluster;
        self.dock_editable = carried.dock_editable;
        self.command = carried.command;
        self.login = carried.login;
        self.remote_up = carried.remote_up;
        self.typed = carried.typed;
        self.ours = carried.ours;
        self.command_open = carried.command_open;
    }
}

/// A number field: `-` for none.
fn render_number<N: fmt::Display>(value: Option<N>) -> String {
    value.map_or("-".to_owned(), |value| value.to_string())
}

/// [`render_number`]'s inverse; the outer `None` is a malformed field.
fn parse_number<N: std::str::FromStr>(field: &str) -> Option<Option<N>> {
    if field == "-" {
        return Some(None);
    }
    field.parse().ok().map(Some)
}

/// A text field: `-` for none, `+` and the escaped text — `\` → `\\`,
/// space → `\s`, tab → `\t`, line feed → `\n`, carriage return → `\r`.
fn render_text(text: Option<&str>) -> String {
    let Some(text) = text else {
        return "-".to_owned();
    };
    let mut out = String::with_capacity(text.len() + 1);
    out.push('+');
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            ' ' => out.push_str("\\s"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

/// [`render_text`]'s inverse; the outer `None` is a malformed field.
fn parse_text(field: &str) -> Option<Option<String>> {
    if field == "-" {
        return Some(None);
    }
    let mut chars = field.strip_prefix('+')?.chars();
    let mut out = String::new();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        out.push(match chars.next()? {
            '\\' => '\\',
            's' => ' ',
            't' => '\t',
            'n' => '\n',
            'r' => '\r',
            _ => return None,
        });
    }
    Some(Some(out))
}

fn render_color(color: Option<HighlightColor>) -> String {
    match color {
        None => "-".to_owned(),
        Some(HighlightColor::Indexed(index)) => format!("i{index}"),
        Some(HighlightColor::Rgb(rgb)) => format!("r{rgb:06x}"),
    }
}

fn parse_carried_color(field: &str) -> Option<Option<HighlightColor>> {
    if field == "-" {
        return Some(None);
    }
    if let Some(index) = field.strip_prefix('i') {
        return Some(Some(HighlightColor::Indexed(index.parse().ok()?)));
    }
    let rgb = field.strip_prefix('r')?;
    (rgb.len() == 6).then_some(())?;
    Some(Some(HighlightColor::Rgb(
        u32::from_str_radix(rgb, 16).ok()?,
    )))
}

fn parse_carried_highlight(fields: &[&str]) -> Option<Highlight> {
    let [start, end, fg, bg, flags] = fields else {
        return None;
    };
    let mut style = HighlightStyle {
        fg: parse_carried_color(fg)?,
        bg: parse_carried_color(bg)?,
        ..HighlightStyle::default()
    };
    if *flags != "-" {
        for letter in flags.chars() {
            let slot = match letter {
                'b' => &mut style.bold,
                'u' => &mut style.underline,
                's' => &mut style.standout,
                _ => return None,
            };
            *slot = true;
        }
    }
    Some(Highlight {
        start: start.parse().ok()?,
        end: end.parse().ok()?,
        style,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::CellHalf;

    /// Feeds the sequence in the given chunks; the scanner has to carry its state
    /// between chunks.
    fn marks_of_chunks(chunks: &[&[u8]]) -> Vec<Mark> {
        let mut scanner = Scanner::new();
        let mut seen = Vec::new();
        for chunk in chunks {
            scanner.feed(chunk, |event| {
                if let ScanEvent::Mark(mark) = event {
                    seen.push(mark);
                }
            });
        }
        seen
    }

    fn marks(bytes: &[u8]) -> Vec<Mark> {
        marks_of_chunks(&[bytes])
    }

    #[test]
    fn four_marks_are_recognised() {
        assert_eq!(
            marks(b"\x1b]133;A\x07"),
            vec![Mark::PromptStart { id: None }]
        );
        assert_eq!(marks(b"\x1b]133;B\x07"), vec![Mark::PromptEnd]);
        assert_eq!(marks(b"\x1b]133;C\x07"), vec![Mark::CommandStart]);
        assert_eq!(
            marks(b"\x1b]133;D\x07"),
            vec![Mark::CommandEnd {
                exit: None,
                id: None
            }]
        );
    }

    #[test]
    fn command_end_carries_the_exit_code() {
        assert_eq!(
            marks(b"\x1b]133;D;0\x07"),
            vec![Mark::CommandEnd {
                exit: Some(0),
                id: None
            }]
        );
        assert_eq!(
            marks(b"\x1b]133;D;130\x07"),
            vec![Mark::CommandEnd {
                exit: Some(130),
                id: None
            }]
        );
    }

    #[test]
    fn unreadable_exit_code_still_ends_the_command() {
        // That the command ended is worth more than its code: even if the payload's
        // parameter is corrupt the mark is not dropped, only the code is unknown.
        assert_eq!(
            marks(b"\x1b]133;D;abc\x07"),
            vec![Mark::CommandEnd {
                exit: None,
                id: None
            }]
        );
    }

    #[test]
    fn the_block_id_is_read_from_the_payload() {
        assert_eq!(
            marks(b"\x1b]133;A;bt_block=42\x07"),
            vec![Mark::PromptStart { id: Some(42) }]
        );
        assert_eq!(
            marks(b"\x1b]133;D;0;bt_block=42\x07"),
            vec![Mark::CommandEnd {
                exit: Some(0),
                id: Some(42)
            }]
        );
    }

    #[test]
    fn unknown_attributes_beside_the_mark_are_still_tolerated() {
        // Shells can attach key-values we do not recognize next to the mark; not knowing
        // them must lose neither the mark nor the identity.
        assert_eq!(
            marks(b"\x1b]133;A;cl=m;bt_block=9\x07"),
            vec![Mark::PromptStart { id: Some(9) }]
        );
        assert_eq!(
            marks(b"\x1b]133;A;bt_block=\x07"),
            vec![Mark::PromptStart { id: None }]
        );
    }

    #[test]
    fn a_code_less_command_end_keeps_its_id() {
        // If the first field were blindly counted as the code it would swallow
        // `bt_block=7` and the block would never close in the ledger.
        assert_eq!(
            marks(b"\x1b]133;D;bt_block=7\x07"),
            vec![Mark::CommandEnd {
                exit: None,
                id: Some(7)
            }]
        );
    }

    #[test]
    fn both_terminators_end_the_sequence() {
        assert_eq!(
            marks(b"\x1b]133;A\x07"),
            vec![Mark::PromptStart { id: None }]
        );
        assert_eq!(
            marks(b"\x1b]133;A\x1b\\"),
            vec![Mark::PromptStart { id: None }]
        );
    }

    #[test]
    fn bare_escape_dispatches_like_vte() {
        // `vte` dispatches the sequence on seeing ESC, without waiting for `\`: two
        // back-to-back sequences are read without a terminator between them.
        assert_eq!(
            marks(b"\x1b]133;A\x1b]133;B\x07"),
            vec![Mark::PromptStart { id: None }, Mark::PromptEnd]
        );
        // And a CSI following the ESC does not corrupt the sequence.
        assert_eq!(marks(b"\x1b]133;C\x1b[0m"), vec![Mark::CommandStart]);
    }

    #[test]
    fn sequence_split_at_every_byte_survives() {
        let seq: &[u8] = b"\x1b]133;D;7\x07";
        for at in 0..=seq.len() {
            let (head, tail) = seq.split_at(at);
            assert_eq!(
                marks_of_chunks(&[head, tail]),
                vec![Mark::CommandEnd {
                    exit: Some(7),
                    id: None
                }],
                "split point {at}"
            );
        }
    }

    #[test]
    fn escape_survives_the_bytes_vte_executes_in_place() {
        // On these bytes `advance_esc` stays in `Escape`, so the `]` that follows really
        // does open a sequence. A scanner that fell to Ground would silently lose the mark
        // and the state would split from the grid.
        assert_eq!(
            marks(b"\x1b\r]133;A\x07"),
            vec![Mark::PromptStart { id: None }]
        );
        assert_eq!(marks(b"\x1b\x07]133;B\x07"), vec![Mark::PromptEnd]);
        assert_eq!(marks(b"\x1b\x80]133;C\x07"), vec![Mark::CommandStart]);

        // And on the two C0s that really **end** `Escape` (0x18, 0x1A) a sequence is not
        // opened — `advance_esc` takes them to Ground.
        assert_eq!(marks(b"\x1b\x18]133;A\x07"), vec![]);
        assert_eq!(marks(b"\x1b\x1a]133;A\x07"), vec![]);
    }

    /// Feeds the sequence in chunks; returns the counter and the marks seen after it
    /// together — so that both claims of the CSI arm ("did it set the flag", "did it
    /// swallow what comes after") can be asked in a single call.
    fn clears_and_marks_of_chunks(chunks: &[&[u8]]) -> (u32, Vec<Mark>) {
        let mut scanner = Scanner::new();
        let mut seen = Vec::new();
        let mut clears = 0;
        for chunk in chunks {
            scanner.feed(chunk, |event| {
                if let ScanEvent::Mark(mark) = event {
                    seen.push(mark);
                }
            });
            clears += scanner.take_screen_clears();
        }
        (clears, seen)
    }

    fn clears(bytes: &[u8]) -> u32 {
        clears_and_marks_of_chunks(&[bytes]).0
    }

    fn paste_ons_of_chunks(chunks: &[&[u8]]) -> u32 {
        let mut scanner = Scanner::new();
        let mut seen = 0;
        for chunk in chunks {
            scanner.feed(chunk, |event| {
                seen += u32::from(matches!(event, ScanEvent::PasteOn))
            });
            assert_eq!(scanner.take_screen_clears(), 0, "{chunk:?} is not a clear");
        }
        seen
    }

    fn paste_ons(bytes: &[u8]) -> u32 {
        paste_ons_of_chunks(&[bytes])
    }

    #[test]
    fn only_bracketed_paste_on_is_the_login_signal() {
        // 047 phase-4: `CSI ? 2004 h` — the leading `?` only, the one parameter,
        // the `h`.
        assert_eq!(paste_ons(b"\x1b[?2004h"), 1);
        assert_eq!(paste_ons(b"a\x1b[?2004hb\x1b[?2004h"), 2);
        for other in [
            &b"\x1b[?2004l"[..],
            b"\x1b[2004h",
            b"\x1b[?2004;1h",
            b"\x1b[?1049h",
            b"\x1b[??2004h",
            b"\x1b[2?004h",
            b"\x1b[>2004h",
        ] {
            assert_eq!(paste_ons(other), 0, "{other:?}");
        }
        // Split across two reads, like any CSI.
        assert_eq!(paste_ons_of_chunks(&[b"\x1b[?20", b"04h"]), 1);
        // A private marker does not make an erase: `CSI ? 2 J` is DECSED.
        assert_eq!(clears(b"\x1b[?2J"), 0);
        assert_eq!(clears(b"\x1b[2J"), 1);
    }

    #[test]
    fn only_erase_all_sets_the_screen_clear() {
        // The **only** recognized sequence is `CSI 2 J`. `CSI J` is a parameterless ED,
        // that is ED 0 (from the cursor down) and `CSI 3 J` deletes the history — neither
        // is deliberately clearing the screen.
        assert_eq!(clears(b"\x1b[2J"), 1);
        assert_eq!(
            clears(b"\x1b[02J"),
            1,
            "a leading zero must not break the sequence"
        );
        assert_eq!(clears(b"\x1b[J"), 0);
        assert_eq!(clears(b"\x1b[0J"), 0);
        assert_eq!(clears(b"\x1b[1J"), 0);
        assert_eq!(clears(b"\x1b[3J"), 0);
        assert_eq!(clears(b"\x1b[22J"), 0);
        assert_eq!(clears(b"\x1b[2K"), 0, "the terminator must match too");
        // A private marker (`?`, DECSED), a second parameter and an intermediate byte:
        // all three make the sequence unrecognized.
        assert_eq!(clears(b"\x1b[?2J"), 0);
        assert_eq!(clears(b"\x1b[2;2J"), 0);
        assert_eq!(clears(b"\x1b[2 J"), 0);
        // The parameter ceiling: the sequence becomes unrecognized before the counter overflows.
        assert_eq!(clears(b"\x1b[99999999999999999999J"), 0);
        // Two clears are counted twice: the consumer is a generation counter, not a flag.
        assert_eq!(clears(b"\x1b[2J\x1b[2J"), 2);
    }

    #[test]
    fn a_csi_never_swallows_the_mark_behind_it() {
        // **The defect this test closes is silent:** a scanner stuck in a corrupt CSI
        // swallows the `ESC ] 133;…` that follows, and blocks, suppression of the input
        // line and the dock die together.
        let mark = vec![Mark::PromptStart { id: None }];

        // (1) After completed CSIs: both recognized and unrecognized.
        assert_eq!(
            clears_and_marks_of_chunks(&[b"\x1b[2J\x1b]133;A\x07"]),
            (1, mark.clone())
        );
        assert_eq!(
            clears_and_marks_of_chunks(&[b"\x1b[?1049h\x1b]133;A\x07"]),
            (0, mark.clone())
        );
        // (2) `ESC` cancels a half-finished CSI (`vte::anywhere`), so our sequence opens
        // again.
        assert_eq!(
            clears_and_marks_of_chunks(&[b"\x1b[2;3\x1b]133;A\x07"]),
            (0, mark.clone())
        );
        assert_eq!(
            clears_and_marks_of_chunks(&[b"\x1b[\x1b]133;A\x07"]),
            (0, mark.clone())
        );
        // (3) `CAN`/`SUB` take the sequence to `Ground`; from there a new sequence can
        // open only with `ESC`.
        assert_eq!(
            clears_and_marks_of_chunks(&[b"\x1b[2\x18", b"\x1b]133;A\x07"]),
            (0, mark.clone())
        );
        assert_eq!(
            clears_and_marks_of_chunks(&[b"\x1b[2\x18]133;A\x07"]),
            (0, vec![])
        );
        // (4) `ESC` also rescues a CSI whose terminator never arrives: the ceiling only
        // makes the parameter unrecognized, it does not leave the state.
        let long = b"\x1b["
            .iter()
            .copied()
            .chain(std::iter::repeat_n(b'9', 10_000));
        let stream: Vec<u8> = long.chain(b"\x1b]133;A\x07".iter().copied()).collect();
        assert_eq!(clears_and_marks_of_chunks(&[&stream]), (0, mark));
    }

    #[test]
    fn a_bel_inside_a_csi_is_not_a_terminator() {
        // OSC's terminator set is **not** valid in CSI: `vte`'s CSI states `execute` C0s
        // in place and do not change the state, so `ESC [ 2 BEL J` is still an ED 2. An
        // arrangement that shared the two sets would split the sequence boundary the grid
        // sees from ours.
        assert_eq!(clears(b"\x1b[2\x07J"), 1);
        assert_eq!(clears(b"\x1b[\r2J"), 1);
        // 0x7F and bytes above 0x7F are ignored too (`anywhere`'s last arm), they do not
        // leave the state.
        assert_eq!(clears(b"\x1b[2\x7fJ"), 1);
        assert_eq!(clears(b"\x1b[2\x80J"), 1);
    }

    #[test]
    fn the_screen_clear_survives_a_split_at_every_byte() {
        // The whole state has to be carried across a chunk boundary: "I'm in a CSI",
        // "parameter 2" and "the sequence is still plain" live between two `read()`s too.
        let seq: &[u8] = b"\x1b[2J";
        for at in 0..=seq.len() {
            let (head, tail) = seq.split_at(at);
            assert_eq!(
                clears_and_marks_of_chunks(&[head, tail]).0,
                1,
                "split point {at}"
            );
        }
    }

    #[test]
    fn unknown_submark_and_broken_payload_are_ignored() {
        assert_eq!(marks(b"\x1b]133;Z\x07"), vec![]);
        assert_eq!(marks(b"\x1b]133;AB\x07"), vec![]);
        assert_eq!(marks(b"\x1b]133;\x07"), vec![]);
        assert_eq!(marks(b"\x1b]133\x07"), vec![]);
    }

    #[test]
    fn other_osc_numbers_never_touch_the_buffer() {
        // OSC 52's payload can legitimately be megabytes; so that what the bound
        // distinguishes is preserved that path never touches the buffer.
        let mut stream = b"\x1b]52;c;".to_vec();
        stream.extend(std::iter::repeat_n(b'Z', 100_000));
        stream.push(0x07);
        stream.extend_from_slice(b"\x1b]133;B\x07");

        let mut scanner = Scanner::new();
        let mut seen = Vec::new();
        scanner.feed(&stream, |event| {
            if let ScanEvent::Mark(mark) = event {
                seen.push(mark);
            }
        });

        assert_eq!(seen, vec![Mark::PromptEnd]);
        assert_eq!(scanner.payload.capacity(), PAYLOAD_LIMIT);
    }

    #[test]
    fn oversized_payload_is_dropped_and_the_next_sequence_survives() {
        let mut stream = b"\x1b]133;D;".to_vec();
        stream.extend(std::iter::repeat_n(b'9', PAYLOAD_LIMIT + 1));
        stream.push(0x07);
        stream.extend_from_slice(b"\x1b]133;A\x07");

        let mut scanner = Scanner::new();
        let mut seen = Vec::new();
        scanner.feed(&stream, |event| {
            if let ScanEvent::Mark(mark) = event {
                seen.push(mark);
            }
        });

        assert_eq!(seen, vec![Mark::PromptStart { id: None }]);
        assert_eq!(scanner.payload.capacity(), PAYLOAD_LIMIT);
    }

    #[test]
    fn control_bytes_inside_the_payload_are_dropped_like_vte() {
        // `vte` does not take it into the payload; if we had, a code with a line ending
        // pasted on would look corrupt.
        assert_eq!(
            marks(b"\x1b]133;D;0\r\x07"),
            vec![Mark::CommandEnd {
                exit: Some(0),
                id: None
            }]
        );
    }

    #[test]
    fn plain_text_around_the_marks_is_ignored() {
        assert_eq!(
            marks(b"merhaba\x1b]133;A\x07dunya\x1b]133;B\x07$ ls\r\n"),
            vec![Mark::PromptStart { id: None }, Mark::PromptEnd]
        );
    }

    #[test]
    fn marks_walk_the_state_through_a_whole_command() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::PromptStart { id: None });
        assert_eq!(log.local.state.map(|s| s.phase), Some(ShellPhase::Prompt));

        log.apply(Mark::PromptEnd);
        assert_eq!(log.local.state.map(|s| s.phase), Some(ShellPhase::Input));

        log.apply(Mark::CommandStart);
        assert_eq!(log.local.state.map(|s| s.phase), Some(ShellPhase::Running));

        log.apply(Mark::CommandEnd {
            exit: Some(2),
            id: None,
        });
        assert_eq!(
            log.local.state,
            Some(ShellState {
                phase: ShellPhase::Finished,
                last_exit: Some(2),
            })
        );
    }

    #[test]
    fn an_unreadable_code_does_not_inherit_the_previous_one() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::CommandEnd {
            exit: Some(2),
            id: None,
        });
        log.apply(Mark::CommandEnd {
            exit: None,
            id: None,
        });
        assert_eq!(log.local.state.and_then(|s| s.last_exit), None);
    }

    /// Opens and closes a block; the ledger's ordinary flow.
    fn run_block(log: &mut ShellLog, id: u32, exit: Option<i32>) {
        log.apply(Mark::PromptStart { id: Some(id) });
        log.apply(Mark::CommandStart);
        log.apply(Mark::CommandEnd { exit, id: Some(id) });
    }

    /// The exit code the block recorded; `None` if it is not in the ledger or is
    /// still open.
    ///
    /// The tests below ask for the **code**, not the duration: the elapsed time comes
    /// from the real clock and cannot be made equal. Comparing with the whole
    /// `Outcome` would make them clock-dependent and brittle.
    fn exit_of(log: &ShellLog, id: u32) -> Option<Option<i32>> {
        match log.local.blocks.get(id)? {
            Outcome::Finished { exit, .. } => Some(exit),
            Outcome::Pending => None,
        }
    }

    #[test]
    fn the_log_remembers_each_block_by_its_id() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        run_block(&mut log, 1, Some(0));
        run_block(&mut log, 2, Some(130));
        log.apply(Mark::PromptStart { id: Some(3) });

        assert_eq!(exit_of(&log, 1), Some(Some(0)));
        assert_eq!(exit_of(&log, 2), Some(Some(130)));
        // Open but not closed: running or an empty prompt.
        assert_eq!(log.local.blocks.get(3), Some(Outcome::Pending));
        assert_eq!(log.local.blocks.get(4), None);
    }

    #[test]
    fn a_command_end_without_a_start_is_ignored() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::CommandEnd {
            exit: Some(1),
            id: Some(77),
        });
        assert_eq!(log.local.blocks.get(77), None);
    }

    #[test]
    fn the_oldest_block_falls_out_of_the_full_log() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        for id in 1..=(BLOCK_LOG_FLOOR as u32 + 2) {
            run_block(&mut log, id, Some(0));
        }
        // The dropped block has no color; the frame path **does not draw** it, does not draw it wrongly.
        assert_eq!(log.local.blocks.get(1), None);
        assert_eq!(log.local.blocks.get(2), None);
        assert_eq!(exit_of(&log, 3), Some(Some(0)));
        assert_eq!(exit_of(&log, BLOCK_LOG_FLOOR as u32 + 2), Some(Some(0)));
    }

    #[test]
    fn reopening_an_id_drops_only_what_followed_it() {
        // Defensive arm: our counter is monotonic across the shell instance, so this
        // happens only with a `bt_block=` we did not print. When it does, not the whole
        // ledger but what comes AFTER that identity is dropped.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        run_block(&mut log, 1, Some(0));
        run_block(&mut log, 2, Some(0));
        run_block(&mut log, 1, Some(3));

        assert_eq!(exit_of(&log, 1), Some(Some(3)));
        assert_eq!(log.local.blocks.get(2), None);
    }

    #[test]
    fn a_foreign_aid_is_ignored() {
        // `aid` in the specification is the "application id" and generally carries a pid,
        // that is, it is CONSTANT across the session. If we read it as an identity, an
        // integration that complies with the specification (the user's rc, a nested REPL,
        // the far side of SSH) would print the same value at every prompt and make the
        // ledger non-contiguous every time; a `D` falling into the range would overwrite
        // our block's color with someone else's code.
        assert_eq!(
            marks(b"\x1b]133;A;aid=4711\x07"),
            vec![Mark::PromptStart { id: None }]
        );
        assert_eq!(
            marks(b"\x1b]133;D;1;aid=4711\x07"),
            vec![Mark::CommandEnd {
                exit: Some(1),
                id: None
            }]
        );

        // And foreign marks cannot touch our ledger.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        run_block(&mut log, 1, Some(0));
        log.apply(Mark::PromptStart { id: None });
        log.apply(Mark::CommandEnd {
            exit: Some(1),
            id: None,
        });
        assert_eq!(exit_of(&log, 1), Some(Some(0)));
    }

    /// The tests' encoder — in production its counterpart is the shell's pure-zsh arm.
    fn b64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let (a, b, c) = (
                u32::from(chunk[0]),
                chunk.get(1).map_or(0, |&b| u32::from(b)),
                chunk.get(2).map_or(0, |&b| u32::from(b)),
            );
            let word = a << 16 | b << 8 | c;
            for slot in 0..4 {
                if slot <= chunk.len() {
                    out.push(ALPHABET[(word >> (18 - 6 * slot) & 0x3f) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    /// The mirror sequence; `highlights` is joined with line breaks and base64'd.
    fn dock_update(
        cursor: usize,
        pre: &str,
        buffer: &str,
        post: &str,
        highlights: &[&str],
    ) -> Vec<u8> {
        format!(
            "\x1b]8133;u;{cursor};{};{};{};{}\x07",
            b64(pre.as_bytes()),
            b64(buffer.as_bytes()),
            b64(post.as_bytes()),
            b64(highlights.join("\n").as_bytes()),
        )
        .into_bytes()
    }

    /// Since `DockEvent` lends, the tests keep an owned copy.
    #[derive(Debug, PartialEq, Eq)]
    enum DockSnapshot {
        Update(DockState),
        End,
        Unavailable(DockFault),
        Branch(String),
        Editable,
    }

    fn dock_events_of_chunks(chunks: &[&[u8]]) -> Vec<DockSnapshot> {
        let mut scanner = Scanner::new();
        let mut seen = Vec::new();
        for chunk in chunks {
            scanner.feed(chunk, |event| {
                if let ScanEvent::Dock(event) = event {
                    seen.push(match event {
                        DockEvent::Update(line) => DockSnapshot::Update(line.clone()),
                        DockEvent::End => DockSnapshot::End,
                        DockEvent::Unavailable(fault) => DockSnapshot::Unavailable(fault),
                        DockEvent::Branch(branch) => DockSnapshot::Branch(branch.to_owned()),
                        DockEvent::Editable => DockSnapshot::Editable,
                    });
                }
            });
        }
        seen
    }

    fn dock_events(bytes: &[u8]) -> Vec<DockSnapshot> {
        dock_events_of_chunks(&[bytes])
    }

    /// The single `DockState` of an update; if anything else came, it drops.
    fn dock_line(bytes: &[u8]) -> DockState {
        match dock_events(bytes).pop() {
            Some(DockSnapshot::Update(line)) => line,
            other => panic!("an update was expected, got: {other:?}"),
        }
    }

    #[test]
    fn the_dock_arm_decodes_the_five_variables() {
        let line = dock_line(&dock_update(
            3,
            "❯ ",
            "git sta",
            "tus",
            &["0 3 fg=green,bold"],
        ));
        assert_eq!(line.status, DockStatus::Live);
        assert_eq!(line.predisplay, "❯ ");
        assert_eq!(line.buffer, "git sta");
        assert_eq!(line.postdisplay, "tus");
        // `$CURSOR` 3, `PREDISPLAY` two characters: 5 in the display space.
        assert_eq!(line.cursor, 5);
        assert_eq!(
            line.highlights,
            vec![Highlight {
                // `PREDISPLAY` is two characters: the unprefixed offset is counted after it.
                start: 2,
                end: 5,
                style: HighlightStyle {
                    fg: Some(HighlightColor::Indexed(2)),
                    bold: true,
                    ..HighlightStyle::default()
                },
            }]
        );
    }

    #[test]
    fn highlight_offsets_collapse_into_one_space() {
        // The `P` prefix ties the offset to the start of PREDISPLAY, the unprefixed one
        // to BUFFER's: both descend into the single space counted from the start of the
        // display (R1.3).
        let line = dock_line(&dock_update(
            0,
            "ab",
            "cd",
            "",
            &["P0 2 fg=red", "0 2 bg=4"],
        ));
        assert_eq!(line.highlights[0].start, 0);
        assert_eq!(line.highlights[0].end, 2);
        assert_eq!(
            line.highlights[0].style.fg,
            Some(HighlightColor::Indexed(1))
        );
        assert_eq!(line.highlights[1].start, 2);
        assert_eq!(line.highlights[1].end, 4);
        assert_eq!(
            line.highlights[1].style.bg,
            Some(HighlightColor::Indexed(4))
        );
    }

    #[test]
    fn a_highlight_keeps_what_it_understands_and_drops_the_rest() {
        // `memo=` is a free tail field, `blink` an attribute we do not recognize; neither
        // must drop the record — if it did the whole range would be left colorless.
        let line = dock_line(&dock_update(
            0,
            "",
            "xy",
            "",
            &["0 2 fg=#ff8800,underline,blink memo=zsh-syntax-highlighting"],
        ));
        assert_eq!(
            line.highlights,
            vec![Highlight {
                start: 0,
                end: 2,
                style: HighlightStyle {
                    fg: Some(HighlightColor::Rgb(0xff8800)),
                    underline: true,
                    ..HighlightStyle::default()
                },
            }]
        );

        // A reversed range and an unreadable offset drop the record, not the sequence.
        let line = dock_line(&dock_update(0, "", "xy", "", &["5 1 fg=red", "a b fg=red"]));
        assert_eq!(line.highlights, vec![]);
    }

    #[test]
    fn offsets_never_point_past_the_mirrored_text() {
        // A `region_highlight` built from a stale `BUFFER` snapshot can point outside the
        // text; carrying it to the drawing side would create a clamping or panic debt
        // there.
        let line = dock_line(&dock_update(
            0,
            "ab",
            "cd",
            "",
            &["0 99 fg=red", "50 60 fg=red"],
        ));
        assert_eq!(line.highlights.len(), 1);
        assert_eq!(line.highlights[0].start, 2);
        assert_eq!(line.highlights[0].end, 4);

        // The cursor is not left to the shell's word either: at most the end of `BUFFER`.
        assert_eq!(dock_line(&dock_update(99, "ab", "cd", "ef", &[])).cursor, 4);
    }

    #[test]
    fn the_dock_end_closes_the_mirror() {
        assert_eq!(dock_events(b"\x1b]8133;e\x07"), vec![DockSnapshot::End]);
    }

    #[test]
    fn extra_trailing_fields_are_tolerated() {
        // A forward-looking field: phase-4's special mode signal should be addable
        // without reopening this parser.
        let mut sequence = dock_update(1, "", "ab", "", &[]);
        sequence.pop();
        sequence.extend_from_slice(b";mode=isearch\x07");
        assert_eq!(dock_line(&sequence).buffer, "ab");
    }

    #[test]
    fn a_tab_in_the_buffer_carries_no_ink() {
        // **A measured defect** (user, 2026-09-18): pressing Tab with an empty dock made
        // the caret leap from the dock to the grid. The chain was measured in a real zsh —
        // Tab makes `BUFFER='\t'`, the mirror said `Some('\t')`, the grid said `None`
        // because it expands the tab into blanks; when the freshness gate
        // (`Session::frame`) did not match, suppression lifted and the caret's owner
        // changed.
        assert_eq!(dock_line(&dock_update(1, "", "\t", "", &[])).last_ink, None);
        // It must drop the previous letter when the tab is **at the end** too: on the grid
        // the line's last ink is still `s`.
        assert_eq!(
            dock_line(&dock_update(3, "", "ls\t", "", &[])).last_ink,
            Some('s')
        );
        // The text before the tab is not affected.
        assert_eq!(
            dock_line(&dock_update(3, "", "\tls", "", &[])).last_ink,
            Some('s')
        );
        // The blank's rule did not change and ink is still ink.
        assert_eq!(
            dock_line(&dock_update(3, "", "ls ", "", &[])).last_ink,
            Some('s')
        );
    }

    #[test]
    fn padding_is_optional() {
        // The encoding side is pure zsh; to require padding would tie the channel to an
        // implementation detail of it.
        let padded = dock_line(&dock_update(0, "", "abcd", "", &[]));
        let bare = dock_line(b"\x1b]8133;u;0;;YWJjZA;;\x07");
        assert_eq!(padded.buffer, "abcd");
        assert_eq!(bare.buffer, "abcd");
    }

    #[test]
    fn a_broken_payload_is_reported_not_panicked() {
        // Three corruptions, one answer: we cannot show it.
        for sequence in [
            &b"\x1b]8133;u;0;;!!!!;;\x07"[..], // not in the base64 alphabet
            &b"\x1b]8133;u;0;;YQ;\x07"[..],    // alan eksik
            &b"\x1b]8133;u;abc;;;;\x07"[..],   // the cursor is not a number
            &b"\x1b]8133;u;0;;gA;;\x07"[..],   // invalid UTF-8
            &b"\x1b]8133;z\x07"[..],           // unrecognized operation
            &b"\x1b]8133;\x07"[..],            // empty payload
        ] {
            assert_eq!(
                dock_events(sequence),
                vec![DockSnapshot::Unavailable(DockFault::Malformed)],
                "dizi: {:?}",
                String::from_utf8_lossy(sequence)
            );
        }
    }

    #[test]
    fn an_oversized_dock_payload_is_visible_and_the_next_sequence_survives() {
        // Unlike 133's silent drop, the overflow **returns a result** to the caller
        // (R1.2); a sound sequence that follows is still seen.
        let mut stream = b"\x1b]8133;u;0;;".to_vec();
        stream.extend(std::iter::repeat_n(b'A', DOCK_PAYLOAD_LIMIT + 1));
        stream.push(0x07);
        stream.extend_from_slice(&dock_update(1, "", "ok", "", &[]));

        let seen = dock_events(&stream);
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0], DockSnapshot::Unavailable(DockFault::Overflow));
        assert!(matches!(&seen[1], DockSnapshot::Update(line) if line.buffer == "ok"));
    }

    /// The **local** paths the directory arm decodes.
    fn cwd_events(bytes: &[u8]) -> Vec<String> {
        cwd_events_of(bytes, true)
    }

    /// The paths the directory arm decodes, whose authority is local (`local`) or
    /// not.
    fn cwd_events_of(bytes: &[u8], local: bool) -> Vec<String> {
        let mut scanner = Scanner::new();
        let mut seen = Vec::new();
        scanner.feed(bytes, |event| {
            if let ScanEvent::Cwd { path, local: is } = event
                && is == local
            {
                seen.push(path.to_owned());
            }
        });
        seen
    }

    #[test]
    fn the_cwd_arm_decodes_a_percent_encoded_path() {
        // Percent decoding: a blank and a multi-byte character.
        assert_eq!(
            cwd_events(b"\x1b]7;file:///Users/a%20b/%C3%A7\x07"),
            ["/Users/a b/ç"]
        );
        // Both authorities are this machine: empty and `localhost`.
        assert_eq!(cwd_events(b"\x1b]7;file://localhost/tmp\x07"), ["/tmp"]);
        // The scheme is case-insensitive (RFC 3986).
        assert_eq!(cwd_events(b"\x1b]7;FILE:///tmp\x07"), ["/tmp"]);
        // The payload is **not split** into fields: a path carrying `;` is valid.
        assert_eq!(cwd_events(b"\x1b]7;file:///tmp/a;b\x07"), ["/tmp/a;b"]);
        // An unencoded path is read too: percent encoding is not mandatory.
        assert_eq!(cwd_events(b"\x1b]7;file:///tmp/plain\x07"), ["/tmp/plain"]);
    }

    #[test]
    fn a_named_host_is_foreign_and_a_broken_uri_is_ignored() {
        // A named host is **foreign** (036): it produces an event but is not local, so it
        // never writes to the local directory — it goes to the remote slot.
        let named = b"\x1b]7;file://remote.example/tmp\x07";
        assert_eq!(cwd_events(named), Vec::<String>::new());
        assert_eq!(cwd_events_of(named, false), ["/tmp"]);
        // The only answer for the rest: no event at all. The direction is safe — the old
        // path stays on screen.
        for sequence in [
            &b"\x1b]7;/tmp\x07"[..],          // no scheme
            b"\x1b]7;http://host/tmp\x07",    // foreign scheme
            b"\x1b]7;file:/tmp\x07",          // no authority section
            b"\x1b]7;file://localhost\x07",   // no path
            b"\x1b]7;file:///tmp/%zz\x07",    // corrupt percent escape
            b"\x1b]7;file:///tmp/%e0%80\x07", // not UTF-8
            b"\x1b]7;\x07",                   // empty payload
        ] {
            assert!(
                cwd_events(sequence).is_empty() && cwd_events_of(sequence, false).is_empty(),
                "the sequence passed: {}",
                String::from_utf8_lossy(sequence)
            );
        }
    }

    #[test]
    fn the_machines_own_name_is_local_when_supplied() {
        // 044: with the name supplied (`SessionOptions::hostname`) `file://$HOST/…`
        // writes the local directory; without it the same bytes stay foreign.
        let named = b"\x1b]7;file://MyMac/tmp\x07";
        let mut scanner = Scanner::new().hostname(Some("mymac".to_owned()));
        let mut seen = Vec::new();
        scanner.feed(named, |event| {
            if let ScanEvent::Cwd { path, local } = event {
                seen.push((path.to_owned(), local));
            }
        });
        assert_eq!(seen, [("/tmp".to_owned(), true)]);
        assert_eq!(cwd_events_of(named, false), ["/tmp"]);
        assert!(is_local_authority("", None));
        assert!(is_local_authority("LOCALHOST", None));
        assert!(!is_local_authority("mymac", None));
        assert!(!is_local_authority("other", Some("mymac")));
        assert!(!is_local_authority("x", Some("")));
    }

    #[test]
    fn an_oversized_cwd_payload_is_dropped_and_the_next_sequence_survives() {
        // The overflow is **silent** (unlike the mirror arm): the directory is not an
        // invisible detail, its old value stays on screen and the next prompt refreshes it.
        let mut stream = b"\x1b]7;file:///".to_vec();
        stream.extend(std::iter::repeat_n(b'a', CWD_PAYLOAD_LIMIT + 1));
        stream.push(0x07);
        stream.extend_from_slice(b"\x1b]7;file:///tmp\x07");

        assert_eq!(cwd_events(&stream), ["/tmp"]);
    }

    #[test]
    fn the_branch_op_touches_only_the_branch() {
        // The branch comes from the mirror's channel but is not the mirror's **state**:
        // `b` must leave both the text and the `Live`/`Idle` distinction as they are,
        // otherwise the per-prompt branch would darken every line for a frame.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        scanner.feed(&dock_update(2, "% ", "ls", "", &[]), |event| {
            log.apply_scan(event);
        });
        scanner.feed(
            format!("\x1b]8133;b;{}\x07", b64(b"main")).as_bytes(),
            |event| log.apply_scan(event),
        );

        assert_eq!(log.context.branch, "main");
        assert_eq!(log.dock.status, DockStatus::Live);
        assert_eq!(log.dock.buffer, "ls");

        // If it is not a repository the body is empty and the branch is deleted — the
        // previous repository's branch must not hang on in the new directory.
        scanner.feed(b"\x1b]8133;b;\x07", |event| log.apply_scan(event));
        assert_eq!(log.context.branch, "");
        assert_eq!(log.dock.status, DockStatus::Live);
    }

    /// **A fast command produces no handover** — the set's core claim.
    ///
    /// The measured symptom (`context.md` → Kanıt): while `ls` runs the phase lasts 44
    /// ms, the cursor animation settles in 230 ms; the caret leaves the dock and comes
    /// back halfway and the eye reads that as a jump.
    ///
    /// That the handover starts **at `line-finish`** is pinned here too: without `C`
    /// arriving, as soon as the mirror drops to `Idle` the raw answer becomes `Grid`.
    /// A threshold tied to `running_since` would not see this first transition.
    #[test]
    fn a_fast_command_never_hands_the_caret_over() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        scanner.feed(b"\x1b]133;A\x07\x1b]133;B\x07", |event| {
            log.apply_scan(event)
        });
        scanner.feed(&dock_update(2, "% ", "ls", "", &[]), |event| {
            log.apply_scan(event)
        });

        // **`now` is taken after the events.** If it were taken first `caret_since` would
        // be ahead of it, `saturating_duration_since` would clamp to zero and the test
        // would stay green at every value with `HANDOVER_HOLD > 0` — even though a 1 ms
        // hold never catches `ls`'s 44 ms.
        let at_prompt = log.caret(Instant::now());
        assert_eq!(
            at_prompt.home,
            CaretHome::Dock,
            "at the prompt the caret is the dock's"
        );
        assert_eq!(
            at_prompt.hold_left, None,
            "no clock must be set while not holding: zero frames while idle"
        );

        // Enter → `line-finish`: the mirror released the line, the phase is still `Input`.
        scanner.feed(b"\x1b]8133;e\x07", |event| log.apply_scan(event));
        let now = Instant::now();
        // The mirror is held (032 Karar 11) but the state the handover asks about is `Idle`.
        assert_eq!(
            caret_home(log.local.state, log.caret_status(), false),
            CaretHome::Grid,
            "the raw answer is already `Grid` at `line-finish`"
        );
        let handing_over = log.caret(now);
        assert_eq!(
            handing_over.home,
            CaretHome::Dock,
            "the hold must hide the handover"
        );
        assert!(
            handing_over.hold_left.is_some(),
            "the hold's remainder must request a frame, otherwise the handover would wait for the next damage"
        );

        // The command ran and finished — all inside the hold.
        scanner.feed(b"\x1b]133;C\x07", |event| log.apply_scan(event));
        assert_eq!(
            log.caret(now).home,
            CaretHome::Dock,
            "hidden while running too"
        );
        scanner.feed(b"\x1b]133;D;0\x07\x1b]133;A\x07", |event| {
            log.apply_scan(event)
        });
        let after = log.caret(now);
        assert_eq!(after.home, CaretHome::Dock);
        assert_eq!(
            after.hold_left, None,
            "the raw answer returned to `Dock`: the clock must go out (named stop condition)"
        );
    }

    /// **A slow command's handover does happen**, delayed by the hold duration; and
    /// **the reverse direction is never held**.
    ///
    /// Both in a single test, because the second is the first's acceptance: if the
    /// hold applied in both directions the caret would hang on the grid when the
    /// command finishes, and when the user started typing they would see a line in the
    /// dock with no caret.
    #[test]
    fn a_slow_command_hands_over_after_the_hold_but_comes_back_at_once() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        scanner.feed(b"\x1b]133;A\x07\x1b]133;B\x07", |event| {
            log.apply_scan(event)
        });
        scanner.feed(&dock_update(2, "% ", "sleep 2", "", &[]), |event| {
            log.apply_scan(event)
        });
        scanner.feed(b"\x1b]8133;e\x07\x1b]133;C\x07", |event| {
            log.apply_scan(event)
        });

        let now = Instant::now();
        assert_eq!(log.caret(now).home, CaretHome::Dock, "the hold continues");
        // The instant the hold **exactly** expires: the remainder is zero, so the handover becomes visible.
        let expired = now + HANDOVER_HOLD;
        let handed = log.caret(expired);
        assert_eq!(
            handed.home,
            CaretHome::Grid,
            "a slow command must hand over"
        );
        assert_eq!(handed.hold_left, None, "an expired hold requests no frame");

        // The command finished: the reverse direction is **instant**, no hold.
        scanner.feed(b"\x1b]133;D;0\x07", |event| log.apply_scan(event));
        let back = log.caret(expired);
        assert_eq!(back.home, CaretHome::Dock, "Grid→Dock must not be delayed");
        assert_eq!(back.hold_left, None);
    }

    /// **The remote session comes before the hold** (036 Karar 8): a `set_remote`
    /// arriving right after `C`, while the hold is still going, takes the caret to the
    /// grid and sets no clock — on a band with no input line the caret would seat on
    /// the context line. When `D` deletes the remote state the predicate returns to
    /// today's answer.
    #[test]
    fn a_remote_session_takes_the_caret_before_the_hold() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        scanner.feed(b"\x1b]133;A\x07\x1b]133;B\x07", |event| {
            log.apply_scan(event)
        });
        scanner.feed(&dock_update(8, "% ", "ssh prod", "", &[]), |event| {
            log.apply_scan(event)
        });
        scanner.feed(b"\x1b]8133;e\x07\x1b]133;C\x07", |event| {
            log.apply_scan(event)
        });
        let now = Instant::now();
        assert_eq!(log.caret(now).home, CaretHome::Dock, "the hold continues");

        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        let remote = log.caret(now);
        assert_eq!(
            remote.home,
            CaretHome::Grid,
            "on the remote the caret is on the grid"
        );
        assert_eq!(
            remote.hold_left, None,
            "an answer that is not flipped requests no frame"
        );

        scanner.feed(b"\x1b]133;D;0\x07", |event| log.apply_scan(event));
        assert_eq!(log.context.remote, None);
        assert_eq!(log.caret(now + HANDOVER_HOLD).home, CaretHome::Dock);
    }

    /// **A remote session's OSC 8133 does not reach the local dock** (048 R4): the
    /// mirror, the branch and the editing widget's capability stay as the local shell
    /// left them; our `D` ends the remote session and the next local 8133 applies.
    #[test]
    fn a_remote_sessions_8133_is_ignored_until_the_local_d() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        let mut feed = |log: &mut ShellLog, bytes: &[u8]| {
            scanner.feed(bytes, |event| log.apply_scan(event));
        };
        feed(&mut log, b"\x1b]133;A\x07\x1b]133;B\x07");
        feed(
            &mut log,
            format!("\x1b]8133;b;{}\x07", b64(b"main")).as_bytes(),
        );
        feed(&mut log, &dock_update(8, "% ", "ssh prod", "", &[]));
        feed(&mut log, b"\x1b]8133;e\x07\x1b]133;C\x07");
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        let dock = log.dock.clone();
        let editable = log.dock_editable;

        feed(&mut log, &dock_update(2, "$ ", "rm", "", &[]));
        feed(
            &mut log,
            format!("\x1b]8133;b;{}\x07", b64(b"remote")).as_bytes(),
        );
        feed(&mut log, b"\x1b]8133;w\x07\x1b]8133;e\x07");
        assert_eq!(log.dock, dock, "the remote mirror is ignored");
        assert_eq!(log.context.branch, "main", "the remote branch is ignored");
        assert_eq!(log.dock_editable, editable, "the remote `w` is ignored");

        feed(&mut log, b"\x1b]133;D;0\x07");
        assert_eq!(log.context.remote, None);
        feed(&mut log, b"\x1b]133;A\x07\x1b]133;B\x07");
        feed(&mut log, &dock_update(2, "% ", "ls", "", &[]));
        feed(
            &mut log,
            format!("\x1b]8133;b;{}\x07", b64(b"dev")).as_bytes(),
        );
        feed(&mut log, b"\x1b]8133;w\x07");
        assert_eq!(log.dock.buffer, "ls", "the local mirror applies again");
        assert_eq!(log.context.branch, "dev");
        assert!(log.dock_editable);
    }

    /// The first input after a remote login (049 R7): one edge per command
    /// generation, none before the login is seen or once the command ended,
    /// and a new command starts over.
    #[test]
    fn the_first_input_after_a_login_is_one_edge_per_generation() {
        let mut scanner = Scanner::new();
        let mut feed = |log: &mut ShellLog, bytes: &[u8]| {
            scanner.feed(bytes, |event| {
                log.apply_scan_answering(event, 0);
            });
        };
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        feed(&mut log, b"\x1b]133;A\x07\x1b]133;B\x07\x1b]133;C\x07");
        let first = log.command;
        assert!(!log.note_typed(), "the password: no login yet");
        log.login = Some(first);
        assert!(log.note_typed());
        assert_eq!(log.typed, Some(first));
        assert!(!log.note_typed(), "once per generation");
        // The command ended: its login vouches for nothing.
        feed(&mut log, b"\x1b]133;D;0\x07\x1b]133;A\x07\x1b]133;B\x07");
        log.typed = None;
        assert!(!log.note_typed());
        // The next command's login starts over.
        feed(&mut log, b"\x1b]133;C\x07");
        assert_ne!(log.command, first);
        assert!(!log.note_typed(), "an older generation's login");
        log.login = Some(log.command);
        assert!(log.note_typed());
        assert_eq!(log.typed, Some(log.command));
    }

    /// The remote bootstrap's `up` (049 R2.2, `8133;i;up;{nonce}`): recorded
    /// with the command's generation while a command runs — before the probe
    /// and under a remote session alike, past `set_remote` and `D` — and
    /// ignored at a local prompt; a malformed nonce or another `i` word says
    /// nothing; neither touches the mirror or the dock.
    #[test]
    fn the_bootstraps_up_is_kept_with_its_generation_only_while_a_command_runs() {
        let mut scanner = Scanner::new();
        let mut feed = |log: &mut ShellLog, bytes: &[u8]| {
            let mut up = false;
            scanner.feed(bytes, |event| up |= log.apply_scan_answering(event, 0).up);
            up
        };
        // At a local prompt: no command, nothing recorded.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        feed(&mut log, b"\x1b]133;A\x07\x1b]133;B\x07");
        let dock = log.dock.clone();
        assert!(!feed(&mut log, b"\x1b]8133;i;up;00c0ffee00c0ffee\x07"));
        assert_eq!(log.remote_up, None, "a local prompt vouches for nothing");
        assert_eq!(log.dock, dock, "the mirror is untouched");

        // A command runs, the probe has not landed: kept with the generation.
        feed(&mut log, b"\x1b]133;C\x07");
        let generation = log.command;
        assert!(feed(&mut log, b"\x1b]8133;i;up;00c0ffee00c0ffee\x07"));
        assert_eq!(
            log.remote_up,
            Some((generation, "00c0ffee00c0ffee".to_owned()))
        );
        assert_eq!(log.dock, dock, "the mirror is untouched");
        // The probe's answer and the remote 8133 defense keep it; a second `up`
        // replaces it.
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        assert!(feed(&mut log, b"\x1b]8133;i;up;0123456789abcdef\x07"));
        assert_eq!(
            log.remote_up,
            Some((generation, "0123456789abcdef".to_owned())),
            "under the remote session too"
        );
        // Malformed or foreign: nothing, the last one stays.
        for bytes in [
            &b"\x1b]8133;i;up\x07"[..],
            b"\x1b]8133;i;up;\x07",
            b"\x1b]8133;i;up;00C0FFEE\x07",
            b"\x1b]8133;i;up;00c0ffee;x\x07",
            b"\x1b]8133;i;up;0x12\x07",
            b"\x1b]8133;i;ready;00c0ffee\x07",
            b"\x1b]8133;i\x07",
        ] {
            assert!(!feed(&mut log, bytes), "{bytes:?}");
            assert_eq!(
                log.remote_up,
                Some((generation, "0123456789abcdef".to_owned()))
            );
        }
        let long = format!("\x1b]8133;i;up;{}\x07", "a".repeat(NONCE_LIMIT + 1));
        assert!(!feed(&mut log, long.as_bytes()), "past the bound");
        // Our `D` ends the remote session, not the record: the pane's check may
        // come after it. The next `C` is a new generation.
        feed(&mut log, b"\x1b]133;D;0\x07");
        assert_eq!(log.context.remote, None);
        assert_eq!(
            log.remote_up.as_ref().map(|(generation, _)| *generation),
            Some(generation)
        );
        feed(&mut log, b"\x1b]133;A\x07\x1b]133;B\x07\x1b]133;C\x07");
        assert_ne!(
            log.command, generation,
            "a stale record is the caller's to see"
        );

        // Locally, without the remote defense, an `i` never makes the dock
        // unavailable (an unknown op would).
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        feed(&mut log, b"\x1b]133;A\x07\x1b]133;B\x07");
        feed(&mut log, &dock_update(2, "% ", "ls", "", &[]));
        feed(&mut log, b"\x1b]8133;i;whatever\x07");
        assert_eq!(log.dock.buffer, "ls");
        assert_eq!(log.dock.status, DockStatus::Live);
    }

    /// The remote bootstrap's fault (048, `8133;f`) reaches the remote state
    /// whether or not the probe has landed, never touches the mirror, and goes
    /// with the remote state; an unknown code says nothing.
    #[test]
    fn the_remote_bootstraps_fault_lives_with_the_remote_state() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        let mut feed = |log: &mut ShellLog, bytes: &[u8]| {
            scanner.feed(bytes, |event| log.apply_scan(event));
        };
        feed(&mut log, b"\x1b]133;A\x07\x1b]133;B\x07");
        feed(&mut log, &dock_update(8, "% ", "ssh prod", "", &[]));
        feed(&mut log, b"\x1b]8133;e\x07\x1b]133;C\x07");
        let dock = log.dock.clone();
        // Before the probe: still recorded.
        feed(&mut log, b"\x1b]8133;f;write\x07");
        assert_eq!(log.context.remote_setup, Some(RemoteSetupFault::Write));
        assert_eq!(log.dock, dock, "the fault is not a mirror event");
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        assert_eq!(
            log.context.remote_setup,
            Some(RemoteSetupFault::Write),
            "the probe keeps it"
        );
        // While remote: recorded too (the 8133 gate is the dock's, not this).
        feed(&mut log, b"\x1b]8133;f;shell\x07");
        assert_eq!(log.context.remote_setup, Some(RemoteSetupFault::Shell));
        feed(&mut log, b"\x1b]8133;f;newer\x07\x1b]8133;f\x07");
        assert_eq!(
            log.context.remote_setup,
            Some(RemoteSetupFault::Shell),
            "unknown codes say nothing"
        );
        assert_eq!(log.dock, dock);
        feed(&mut log, b"\x1b]133;D;0\x07");
        assert_eq!(log.context.remote_setup, None, "our `D` clears it");
    }

    /// The stamp moves **on change**, not on every event.
    ///
    /// Every keystroke produces a mirror event; if the stamp were refreshed with them
    /// the hold would never expire and in a slow command the handover would **never**
    /// happen.
    #[test]
    fn the_stamp_moves_on_change_not_on_every_event() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        scanner.feed(b"\x1b]133;A\x07\x1b]133;B\x07", |event| {
            log.apply_scan(event)
        });
        scanner.feed(&dock_update(0, "% ", "", "", &[]), |event| {
            log.apply_scan(event)
        });
        scanner.feed(b"\x1b]8133;e\x07", |event| log.apply_scan(event));
        let stamped = log.caret_since;

        // Events arriving after the handover do not change the raw answer (`Running` is
        // `Grid` too), so the stamp must stay in place.
        scanner.feed(b"\x1b]133;C\x07", |event| log.apply_scan(event));
        assert_eq!(
            log.caret_since, stamped,
            "an unchanged answer must not move the stamp"
        );
    }

    /// **A display carrying a line break is `Live`** (032): the dock breaks lines, so
    /// it can show it. Until 032 this mirror was `Multiline` and both the line and its
    /// caret stayed on the grid.
    #[test]
    fn a_newline_anywhere_in_the_display_keeps_the_mirror_live() {
        for (pre, buffer, post) in [
            ("", "echo a\necho b\n", ""),
            ("", "for x in 1 2 3; do\n  echo $x", ""),
            ("% \n", "ls", ""),
            ("", "ls", " a\nb"),
        ] {
            let line = dock_line(&dock_update(0, pre, buffer, post, &[]));
            assert_eq!(
                line.status,
                DockStatus::Live,
                "display carrying a line break: {pre:?} {buffer:?} {post:?}"
            );
            assert_eq!(line.buffer, buffer);
        }
    }

    /// **The last ink is from the display's last line** (032): the other half of the
    /// gate scans the grid's last input line. In a paste of `echo a\necho b\n` zsh
    /// keeps the final line break in the buffer and the cursor is on an empty line —
    /// the mirror must say `None` too. `\n` itself is not ink (the old filter passed it
    /// because its width was `None`).
    #[test]
    fn the_last_ink_comes_from_the_last_row_of_the_display() {
        let ink = |buffer: &str| dock_line(&dock_update(0, "", buffer, "", &[])).last_ink;
        assert_eq!(ink("echo a\necho b\n"), None);
        assert_eq!(ink("echo a\necho b"), Some('b'));
        assert_eq!(ink("echo a\n  "), None);
        assert_eq!(ink("\n"), None);
        // The suggestion is counted on the last line too, if it carries no line break.
        assert_eq!(
            dock_line(&dock_update(0, "", "ls\ngi", "t", &[])).last_ink,
            Some('t')
        );
    }

    /// **With clustered reading the last ink is the last cluster's head** (035): the
    /// grid's cell keeps `👍🏽` with `c = 👍`; if the mirror said `🏽` the gate would
    /// permanently say "stale" (the clustered sibling of 024's guard). The closed
    /// reading is as today: a skin tone is ink on its own.
    #[test]
    fn the_clustered_last_ink_is_the_head_of_the_last_cluster() {
        let ink = |cluster: bool, buffer: &str| {
            let mut scanner = Scanner::new().cluster(cluster);
            let mut seen = None;
            scanner.feed(&dock_update(0, "", buffer, "", &[]), |event| {
                if let ScanEvent::Dock(DockEvent::Update(line)) = event {
                    assert_eq!(
                        line.cluster, cluster,
                        "the reading was not carried to the mirror"
                    );
                    seen = Some(line.last_ink);
                }
            });
            seen.expect("an update was expected")
        };
        assert_eq!(ink(true, "ls 👍🏽"), Some('👍'));
        assert_eq!(ink(true, "🇹🇷"), Some('🇹'));
        assert_eq!(ink(true, "a 👨\u{200D}👩\u{200D}👧"), Some('👨'));
        assert_eq!(ink(true, "❤\u{FE0F} "), Some('❤'));
        assert_eq!(ink(true, "a\n🇹🇷\n"), None, "the last line is empty");
        assert_eq!(ink(true, "ls\t"), Some('s'), "a tab is not ink");
        // The closed reading: code point by code point, zero width is skipped.
        assert_eq!(ink(false, "ls 👍🏽"), Some('🏽'));
        assert_eq!(ink(false, "🇹🇷"), Some('🇷'));
        assert_eq!(ink(false, "❤\u{FE0F}"), Some('❤'));
    }

    /// **A control character the dock does not draw lowers the line to `Control`**
    /// (025) — independent of position and body; tab excepted.
    #[test]
    fn a_control_char_anywhere_in_the_display_marks_the_mirror_control() {
        for (pre, buffer, post) in [
            ("", "\x01foo", ""),
            ("", "foo\x01", ""),
            ("", "echo \x1b[0m", ""),
            ("% \x7f", "ls", ""),
            ("", "ls", " \x02"),
        ] {
            let line = dock_line(&dock_update(0, pre, buffer, post, &[]));
            assert_eq!(
                line.status,
                DockStatus::Control,
                "a display carrying a control character stayed `Live`: {pre:?} {buffer:?} {post:?}"
            );
            assert_eq!(line.buffer, buffer, "the fields stay");
        }
        // **Tab is an exception**: it carries no information and a Ctrl-V Tab line must
        // stay in the dock. Emoji, an empty line and plain text are `Live` too.
        for buffer in ["ls\t", "\t", "🥰", "", "echo a"] {
            let line = dock_line(&dock_update(0, "% ", buffer, "", &[]));
            assert_eq!(line.status, DockStatus::Live, "{buffer:?}");
        }
        // A line break is not counted as a control character (032), another control
        // character is `Control` in a display with line breaks too.
        let both = dock_line(&dock_update(0, "", "a\x01\nb", "", &[]));
        assert_eq!(both.status, DockStatus::Control);
        // A control character in `PREBUFFER` too: the dock draws that too.
        let prebuffer = format!(
            "\x1b]8133;u;0;;{};;;{};{}\x07",
            b64(b"b"),
            b64(b"main"),
            b64(b"a\x01\n")
        );
        assert_eq!(dock_line(prebuffer.as_bytes()).status, DockStatus::Control);
        // The return is automatic: when the control character is deleted the next mirror is `Live`.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        scanner.feed(&dock_update(4, "", "\x01foo", "", &[]), |event| {
            log.apply_scan(event)
        });
        assert_eq!(log.dock.status, DockStatus::Control);
        scanner.feed(&dock_update(3, "", "foo", "", &[]), |event| {
            log.apply_scan(event)
        });
        assert_eq!(log.dock.status, DockStatus::Live);
    }

    /// **`Control` is not held either** — `Unavailable`'s twin and the same reason: the
    /// caret of a line we cannot show is on the grid and must not stand in the dock,
    /// even for 150 ms.
    #[test]
    fn a_control_mirror_is_never_held() {
        let typing = Some(ShellState {
            phase: ShellPhase::Input,
            last_exit: None,
        });
        for held in [false, true] {
            assert_eq!(
                caret_home(typing, DockStatus::Control, held),
                CaretHome::Grid,
                "held={held}"
            );
        }
    }

    /// **The mirror takes its stamp in the same turn as the content** (025):
    /// `answers` is written at the mirror event, does not move on other events and
    /// `End` re-stamps it with the current generation (030).
    #[test]
    fn the_mirror_carries_the_generation_it_answers() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        scanner.feed(&dock_update(2, "", "ls", "", &[]), |event| {
            log.apply_scan_answering(event, 7);
        });
        assert_eq!(log.dock.answers, 7);
        // A mark or branch event does not touch the stamp.
        scanner.feed(b"\x1b]133;B\x07", |event| {
            log.apply_scan_answering(event, 9);
        });
        assert_eq!(log.dock.answers, 7);
        scanner.feed(b"\x1b]8133;e\x07", |event| {
            log.apply_scan_answering(event, 9);
        });
        // In the `Input` phase `e` is held (032 Karar 11) but the stamp is current at
        // once: the held line is ⏎'s answer.
        assert_eq!(
            log.dock.answers, 9,
            "the held mirror must carry the current stamp"
        );
        // A closed mirror does not carry the **old** stamp, it carries the current one:
        // the `Idle` base enters the dock's typing animations' input bound too.
        let _ = log.expire_end(Instant::now() + HANDOVER_HOLD);
        assert_eq!(log.dock.status, DockStatus::Idle);
        assert_eq!(
            log.dock.answers, 9,
            "the closed mirror must carry the current stamp"
        );
    }

    /// **A `line-finish` between `PS2` lines is held** (032 Karar 11).
    ///
    /// zsh prints `e` at every `PS2` acceptance and right after it comes the new
    /// line's mirror (`u`, `PREBUFFER` filled); in between the phase is `Input`.
    /// Without the hold every ⏎ would shrink the band for a frame and the accepted
    /// line would appear on the grid for a moment. `Multiline`'s no-hold guard was
    /// replaced by this: that arm was removed, the hold's rule arrived.
    #[test]
    fn a_line_finish_while_typing_is_held_until_the_next_mirror() {
        let typing = |log: &mut ShellLog, scanner: &mut Scanner| {
            scanner.feed(b"\x1b]133;A;bt_block=1\x07\x1b]133;B\x07", |event| {
                log.apply_scan(event)
            });
            scanner.feed(&dock_update(16, "", "for i in 1 2; do", "", &[]), |event| {
                log.apply_scan_answering(event, 3);
            });
        };
        let end = |log: &mut ShellLog, scanner: &mut Scanner| {
            scanner.feed(b"\x1b]8133;e\x07", |event| {
                log.apply_scan_answering(event, 4);
            });
        };

        // `e` → display, band and suppression stay in place; the floor is the anchor's row.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        typing(&mut log, &mut scanner);
        end(&mut log, &mut scanner);
        let now = Instant::now();
        assert_eq!(log.dock.status, DockStatus::Live);
        assert_eq!(log.dock.buffer, "for i in 1 2; do");
        let input = log
            .suppressed_input()
            .expect("the held line must be suppressed");
        assert!(input.from_anchor, "the held line's floor is the anchor");
        assert_eq!(input.answers, 4, "`e` is ⏎'s answer");
        // The caret is in the dock and the clock is set: a frame is needed when the hold expires.
        let caret = log.caret(now);
        assert_eq!(caret.home, CaretHome::Dock);
        assert!(caret.hold_left.is_some());
        assert!(log.expire_end(now).is_some());

        // If `u` arrives the new mirror passes and the hold ends.
        let next = format!(
            "\x1b]8133;u;0;;;;;{};{}\x07",
            b64(b"main"),
            b64(b"for i in 1 2; do\n")
        );
        scanner.feed(next.as_bytes(), |event| {
            log.apply_scan_answering(event, 4);
        });
        assert_eq!(log.expire_end(now), None);
        assert_eq!(log.dock.prebuffer, "for i in 1 2; do\n");
        assert!(
            log.suppressed_input()
                .is_some_and(|input| input.from_anchor)
        );
        assert_eq!(log.caret(now).home, CaretHome::Dock);

        // `C` (the command ran) ends the hold instantly.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        typing(&mut log, &mut scanner);
        end(&mut log, &mut scanner);
        scanner.feed(b"\x1b]133;C\x07", |event| log.apply_scan(event));
        assert_eq!(log.dock.status, DockStatus::Idle);
        assert_eq!(log.expire_end(Instant::now()), None);

        // When the time is up, today's reset; the stamp stays in place.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        typing(&mut log, &mut scanner);
        end(&mut log, &mut scanner);
        let later = Instant::now() + HANDOVER_HOLD;
        assert_eq!(log.expire_end(later), None);
        assert_eq!(log.dock.status, DockStatus::Idle);
        assert!(log.dock.buffer.is_empty());
        assert_eq!(log.dock.answers, 4);
        assert_eq!(log.caret(later).home, CaretHome::Grid);

        // If the phase is not `Input` there is no hold (`e` does not arrive while a
        // command runs, but if it did it would be as today): instantly `Idle`.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        typing(&mut log, &mut scanner);
        scanner.feed(b"\x1b]133;C\x07", |event| log.apply_scan(event));
        end(&mut log, &mut scanner);
        assert_eq!(log.dock.status, DockStatus::Idle);
    }

    /// **The wheel's window is tied to the caret's place** (032 phase-4): a change of
    /// suggestion leaves it, a change of the caret or the text removes it — a user
    /// typing or pressing an arrow key must see their caret.
    #[test]
    fn the_dock_scroll_ends_when_the_caret_moves() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        let mut feed = |log: &mut ShellLog, bytes: &[u8]| {
            scanner.feed(bytes, |event| log.apply_scan(event));
        };
        feed(&mut log, &dock_update(2, "", "ls", "", &[]));
        log.dock_scroll = Some(0);
        feed(&mut log, &dock_update(2, "", "ls", " -la", &[]));
        assert_eq!(log.dock_scroll, Some(0), "a suggestion leaves the window");
        feed(&mut log, &dock_update(1, "", "ls", " -la", &[]));
        assert_eq!(log.dock_scroll, None, "the caret moved");
        log.dock_scroll = Some(0);
        feed(&mut log, &dock_update(1, "", "lxs", "", &[]));
        assert_eq!(log.dock_scroll, None, "the text changed");
    }

    /// **An empty mirror is measured by character** (032): a lone `\n` pushes the
    /// cursor down, so it is not empty; if `PREBUFFER` is filled it is not empty either
    /// (on a `for>` line the cursor is legitimately below the anchor) and the floor is
    /// the anchor.
    #[test]
    fn a_blank_mirror_has_no_character_at_all() {
        let input = |update: &[u8]| {
            let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
            let mut scanner = Scanner::new();
            scanner.feed(b"\x1b]133;A;bt_block=1\x07\x1b]133;B\x07", |event| {
                log.apply_scan(event)
            });
            scanner.feed(update, |event| log.apply_scan(event));
            log.suppressed_input().expect("the suppressed line")
        };
        let empty = input(&dock_update(0, "", "", "", &[]));
        assert!(empty.blank && !empty.from_anchor);
        // The shape of 025's paste: the caret is behind the trailing line break.
        let pasted = input(&dock_update(14, "", "echo a\necho b\n", "", &[]));
        assert!(!pasted.blank);
        assert_eq!(pasted.last_ink, None);
        assert!(!input(&dock_update(1, "", "\n", "", &[])).blank);
        let ps2 = format!(
            "\x1b]8133;u;0;;;;;{};{}\x07",
            b64(b"main"),
            b64(b"for i in 1 2; do\n")
        );
        let ps2 = input(ps2.as_bytes());
        assert!(!ps2.blank && ps2.from_anchor);
    }

    /// **The mirror's failure is not held** — the carve-out's deterministic guard.
    ///
    /// The integration test (`the_grid_keeps_the_input_line_when_the_mirror_
    /// cannot_show_it`) sees this only if `frame()` runs within 150 ms, so on a loaded
    /// machine it could stay green even if the carve-out were deleted — a guard that
    /// **leaks**. The query here is independent of the clock.
    #[test]
    fn a_faulty_mirror_is_never_held() {
        let typing = Some(ShellState {
            phase: ShellPhase::Input,
            last_exit: None,
        });
        for fault in [DockFault::Overflow, DockFault::Malformed] {
            assert_eq!(
                caret_home(typing, DockStatus::Unavailable(fault), true),
                CaretHome::Grid,
                "the caret of a line we cannot show cannot be held: {fault:?}"
            );
        }
        // The opposite end, with the same `held`: the arm where the hold is really
        // applied. Without the two together the test would stay green in the "the hold
        // does not work at all" state too.
        assert_eq!(
            caret_home(typing, DockStatus::Idle, true),
            CaretHome::Dock,
            "`Input`+`Idle` is the arm that can be held"
        );
    }

    /// The two deadlines **combine**, neither overwrites the other.
    ///
    /// Today `resolve_blocks` writes `next_tick` directly and that path depends on the
    /// running block's anchor being visible; the handover cannot be tied to it.
    /// Overwriting is silent in both directions: either the counter freezes or the
    /// handover never happens.
    #[test]
    fn two_deadlines_merge_into_the_sooner_one() {
        let tick = Duration::from_millis(600);
        let hold = Duration::from_millis(150);
        assert_eq!(sooner(Some(tick), Some(hold)), Some(hold));
        assert_eq!(
            sooner(Some(hold), Some(tick)),
            Some(hold),
            "order does not matter"
        );
        // One-sided states: the one that exists wins, the one that does not exist loses nothing.
        assert_eq!(sooner(Some(tick), None), Some(tick));
        assert_eq!(sooner(None, Some(hold)), Some(hold));
        assert_eq!(
            sooner(None, None),
            None,
            "if both sides are empty no clock is set"
        );
    }

    #[test]
    fn the_keymap_field_opens_the_gate_only_for_insert_keymaps() {
        // Allow list: the three names we recognize pass, **everything else** (a command
        // keymap, a name the user created with `bindkey -N`, a field that never arrived)
        // is closed. The direction is safe — not knowing closes the exception
        // (`Session::can_be_typed`).
        let with = |keymap: &str| {
            let sequence = format!(
                "\x1b]8133;u;0;;{};;;{}\x07",
                b64(b"ls"),
                b64(keymap.as_bytes())
            );
            dock_line(sequence.as_bytes()).insert_keymap
        };
        for keymap in ["main", "emacs", "viins"] {
            assert!(with(keymap), "{keymap} was not counted as an insert keymap");
        }
        for keymap in ["vicmd", "visual", "viopp", "isearch", "command", "mine", ""] {
            assert!(!with(keymap), "{keymap} was counted as an insert keymap");
        }
        // If the field is absent altogether (old script) the gate is closed, but the
        // payload is **not corrupt**: the line is still drawn.
        let line = dock_line(&dock_update(0, "", "ls", "", &[]));
        assert_eq!(line.status, DockStatus::Live);
        assert_eq!(line.buffer, "ls");
        assert!(!line.insert_keymap);
    }

    #[test]
    fn a_six_body_mirror_from_an_old_script_still_decodes() {
        // **A window running with an old script** (032 phase-1): the seventh body
        // (`PREBUFFER`) is absent altogether. Its absence does not corrupt the payload,
        // `PREBUFFER` is counted as empty — `KEYMAP`'s precedent.
        let sequence = format!("\x1b]8133;u;2;;{};;;{}\x07", b64(b"ls"), b64(b"main"));
        let line = dock_line(sequence.as_bytes());
        assert_eq!(line.status, DockStatus::Live);
        assert_eq!(line.buffer, "ls");
        assert!(line.insert_keymap);
        assert_eq!(line.prebuffer, "");
    }

    #[test]
    fn the_seventh_body_carries_the_prebuffer_and_stays_out_of_the_line() {
        // `for i in 1 2` + Enter: ZLE takes the previous line into `PREBUFFER` and
        // `BUFFER` starts with the new line. `PREBUFFER` **always** ends with `\n` and
        // does not drop the mirror from `Live`. It does not enter the display space: the
        // caret, length and last ink come only from `PREDISPLAY ++ BUFFER ++ POSTDISPLAY`.
        let sequence = format!(
            "\x1b]8133;u;2;;{};;;{};{}\x07",
            b64(b"do"),
            b64(b"main"),
            b64(b"for i in 1 2\n")
        );
        let line = dock_line(sequence.as_bytes());
        assert_eq!(line.status, DockStatus::Live);
        assert_eq!(line.prebuffer, "for i in 1 2\n");
        assert_eq!(line.buffer, "do");
        assert_eq!(line.cursor, 2);
        assert_eq!(line.display_chars, 2);
        assert_eq!(line.last_ink, Some('o'));

        // A corrupt seventh body is under the same rule as the other text bodies: the
        // payload is corrupt, the line is on the grid.
        let broken = format!("\x1b]8133;u;0;;{};;;{};!!!!\x07", b64(b"ls"), b64(b"main"));
        assert_eq!(
            dock_events(broken.as_bytes()),
            vec![DockSnapshot::Unavailable(DockFault::Malformed)]
        );
    }

    #[test]
    fn a_broken_branch_never_drops_the_mirror() {
        // Both corruption forms of the branch must drop only the branch: `Unavailable`
        // means "I cannot show the input line" and would bring the grid into play — so
        // because of a truncated branch sequence the user would see what they typed on the
        // grid, not in the dock (`/code-review`, 012 phase-6).
        for sequence in [
            &b"\x1b]8133;b\x07"[..], // no field at all
            b"\x1b]8133;b;!!!!\x07", // not in the base64 alphabet
            b"\x1b]8133;b;gA\x07",   // valid base64, invalid UTF-8
        ] {
            let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
            let mut scanner = Scanner::new();
            scanner.feed(&dock_update(2, "% ", "ls", "", &[]), |event| {
                log.apply_scan(event);
            });
            scanner.feed(sequence, |event| log.apply_scan(event));

            assert_eq!(
                log.dock.status,
                DockStatus::Live,
                "the sequence dropped the mirror: {}",
                String::from_utf8_lossy(sequence)
            );
            assert_eq!(log.dock.buffer, "ls");
            assert_eq!(log.context.branch, "");
        }
    }

    #[test]
    fn the_cwd_survives_a_finished_line() {
        // The context line is not tied to the mirror's lifetime: `line-finish` deletes the
        // text but the directory and branch stay until the next prompt.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        let mut stream = b"\x1b]7;file:///tmp\x07".to_vec();
        stream.extend_from_slice(format!("\x1b]8133;b;{}\x07", b64(b"main")).as_bytes());
        stream.extend_from_slice(&dock_update(0, "", "ls", "", &[]));
        stream.extend_from_slice(b"\x1b]8133;e\x07");
        scanner.feed(&stream, |event| log.apply_scan(event));

        assert_eq!(log.dock.status, DockStatus::Idle);
        assert_eq!(log.dock.buffer, "");
        assert_eq!(log.context.cwd, "/tmp");
        assert_eq!(log.context.branch, "main");
    }

    /// The path of the shell script (`assets/shell/zsh/bateri.zsh`).
    ///
    /// The test runs it from its **source**, not from the bundle: `make bundle` checks
    /// the copy with `cmp`, so the gate for the two being identical is there.
    fn script_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/shell/zsh")
    }

    /// The bytes the script prints for the given ZLE state.
    ///
    /// **The encoder really is zsh.** What these tests hold is not `parse_dock`'s
    /// correctness — it has its own tests — but that the **two ends** of the wire speak
    /// the same format: the encoder is in the shell, the decoder here, and the two are
    /// written in separate languages.
    ///
    /// `zsh -f`: none of the user's startup files is read, so the result is independent
    /// of the plugins installed on the machine.
    ///
    /// **The values pass through the environment**, not embedded in the script: what is
    /// carried is exactly bytes like `;`, `ESC`, backslash and quote, and embedding them
    /// in a zsh string would turn the test into a test of quoting rules.
    ///
    /// If the script cannot run (no zsh) the test **fails**, it is not skipped: bt-core
    /// is *compiled* for the Linux target, its tests run on macOS and `/bin/zsh` is
    /// always there.
    fn script_output(
        cursor: usize,
        pre: &str,
        buffer: &str,
        post: &str,
        highlights: &[&str],
    ) -> Vec<u8> {
        script_output_after(cursor, "", pre, buffer, post, highlights)
    }

    /// [`script_output`], `PREBUFFER` doluyken (032).
    fn script_output_after(
        cursor: usize,
        prebuffer: &str,
        pre: &str,
        buffer: &str,
        post: &str,
        highlights: &[&str],
    ) -> Vec<u8> {
        run_script(
            "source $ZDOTDIR/bateri.zsh
             PREDISPLAY=$T_PRE BUFFER=$T_BUF POSTDISPLAY=$T_POST CURSOR=$T_CURSOR
             region_highlight=( ${(f)T_HL} ) KEYMAP=$T_KEYMAP PREBUFFER=$T_PREBUF
             __bateri_dock_redraw",
            &[
                ("T_CURSOR", &cursor.to_string()),
                ("T_PRE", pre),
                ("T_BUF", buffer),
                ("T_POST", post),
                ("T_HL", &highlights.join("\n")),
                // `$KEYMAP` is a ZLE parameter and is empty outside a hook; the test sets it by
                // hand so that the wire's sixth body runs too.
                ("T_KEYMAP", "main"),
                // `$PREBUFFER` likewise; the seventh body.
                ("T_PREBUF", prebuffer),
            ],
        )
    }

    fn run_script(body: &str, env: &[(&str, &str)]) -> Vec<u8> {
        let mut command = std::process::Command::new("zsh");
        command
            .args(["-f", "-c", body])
            .env("ZDOTDIR", script_path());
        for (key, value) in env {
            command.env(key, value);
        }
        let output = command.output().expect("zsh did not run");
        assert!(
            output.status.success() && output.stderr.is_empty(),
            "the script did not run cleanly: {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    #[test]
    fn the_script_encodes_what_the_scanner_decodes() {
        // The bodies are base64 exactly for these bytes: a `;` would be a field, `ESC`
        // would end the sequence. Backslash is here too — the encoder's first draft read it
        // with the arithmetic `##` form and saw 32 instead of 92.
        let buffer = "echo 'a;b' \\ \u{1b}[0m çığır";
        let line = dock_line(&script_output(
            5,
            "❯ ",
            buffer,
            " --dry-run",
            &[
                "P0 2 fg=blue",
                "0 4 fg=green,bold memo=zsh-syntax-highlighting",
            ],
        ));

        // The wire is sound but the line is not the dock's: `ESC` is a control character,
        // the dock does not draw it and zsh prints `^[` on the grid
        // ([`DockStatus::Control`], 025). The text is still decoded in full — the claims
        // below test the wire itself.
        assert_eq!(line.status, DockStatus::Control);
        assert_eq!(line.predisplay, "❯ ");
        assert_eq!(line.buffer, buffer);
        assert_eq!(line.postdisplay, " --dry-run");
        // `$CURSOR` 5, `PREDISPLAY` two characters: 7 in the display space.
        assert_eq!(line.cursor, 7);
        assert_eq!(line.highlights.len(), 2);
        assert_eq!(line.highlights[0].start, 0);
        assert_eq!(line.highlights[0].end, 2);
        assert_eq!(line.highlights[1].start, 2);
        assert_eq!(line.highlights[1].end, 6);
        assert!(line.highlights[1].style.bold);
        // The sixth body passes through the wire too: the shell prints `$KEYMAP` and the
        // decoder turns it into the insert gate.
        assert!(line.insert_keymap, "the keymap body was lost on the wire");
    }

    #[test]
    fn the_script_sends_the_prebuffer_as_the_seventh_body() {
        // The `for` loop's second line: the shell prints `PREBUFFER`, the decoder keeps it
        // separate and the line is `Live` with a single-line `BUFFER`.
        let line = dock_line(&script_output_after(2, "for i in 1 2\n", "", "do", "", &[]));
        assert_eq!(line.status, DockStatus::Live);
        assert_eq!(line.prebuffer, "for i in 1 2\n");
        assert_eq!(line.buffer, "do");
        assert_eq!(line.cursor, 2);
        // An empty `PREBUFFER` (the ordinary single-line command) is a field too: an empty body.
        let line = dock_line(&script_output(0, "", "ls", "", &[]));
        assert_eq!(line.prebuffer, "");
        assert_eq!(line.status, DockStatus::Live);
    }

    #[test]
    fn the_script_encodes_every_padding_remainder() {
        // base64 grinds three bytes at a time; the three lengths with remainders 0, 1 and 2
        // are tested too. UTF-8 is multiple bytes per character, so "character count" and
        // "byte count" diverge here.
        for text in ["abc", "abcd", "abcde", "ç", "çi", "çığ", "😀"] {
            let line = dock_line(&script_output(0, "", text, "", &[]));
            assert_eq!(line.buffer, text, "metin: {text}");
        }
        // An empty display is valid too: the first mirror arriving as soon as the prompt is drawn is this.
        let line = dock_line(&script_output(0, "", "", "", &[]));
        assert_eq!(line.buffer, "");
        assert_eq!(line.status, DockStatus::Live);
    }

    #[test]
    fn the_script_closes_the_mirror_when_the_line_is_finished() {
        let bytes = run_script("source $ZDOTDIR/bateri.zsh; __bateri_dock_finish", &[]);
        assert_eq!(dock_events(&bytes), vec![DockSnapshot::End]);
    }

    #[test]
    fn a_line_too_long_to_mirror_is_refused_before_it_is_encoded() {
        // The shell-side gate: encoding runs per keystroke and its cost is linear in the
        // length, so encoding a payload the terminal will reject anyway is wasted time. The
        // result of the two ends **must** be the same (`DockFault::Overflow`), otherwise
        // which side the bound is held on would show up to the user as different behavior.
        let long = "x".repeat(4097);
        assert_eq!(
            dock_events(&script_output(0, "", &long, "", &[])),
            vec![DockSnapshot::Unavailable(DockFault::Overflow)]
        );

        // A line below the bound is in the mirror; the gate does not quietly narrow.
        let fits = "x".repeat(4096);
        assert_eq!(
            dock_line(&script_output(0, "", &fits, "", &[])).buffer,
            fits
        );

        // **`PREBUFFER` enters the sum too** (032): the earlier lines of a pasted loop are
        // part of the display and can exceed the bound together with `BUFFER`.
        let before = format!("{}\n", "x".repeat(4095));
        assert_eq!(
            dock_events(&script_output_after(0, &before, "", "ls", "", &[])),
            vec![DockSnapshot::Unavailable(DockFault::Overflow)]
        );

        // **The fourth body is subject to the gate too.** Syntax highlighting leaves one
        // record per token, so next to a short text `region_highlight` alone can exceed the
        // bound; if the gate measured only the text, the comment would claim more than it
        // really does.
        let many: Vec<String> = (0..200)
            .map(|at| format!("{at} {at} fg=green memo=zsh-syntax-highlighting"))
            .collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert_eq!(
            dock_events(&script_output(0, "", "ls", "", &many)),
            vec![DockSnapshot::Unavailable(DockFault::Overflow)]
        );
    }

    #[test]
    fn the_script_prints_a_cwd_the_scanner_decodes() {
        // The encoder is really zsh, the decoder is here: percent encoding is written in
        // two separate languages at the two ends and the bytes it carries are exactly the
        // ones that can break a URI (blank, `%`, `;`, multi-byte character).
        //
        // The directories are **really created**: assigning `PWD` by hand would stop this
        // from testing the shell's own value — zsh sets it itself at startup.
        let root = std::env::temp_dir().join(format!("bateri-cwd-{}", std::process::id()));
        let names = ["plain", "a b", "a%b", "a;b", "çığır", "😀"];
        for name in names {
            std::fs::create_dir_all(root.join(name)).expect("the directory could not be created");
        }

        for name in names {
            let path = root.join(name);
            let path = path.to_string_lossy().into_owned();
            let bytes = run_script(
                "source $ZDOTDIR/bateri.zsh; cd -q -- $T_DIR; __bateri_cwd",
                &[("T_DIR", &path)],
            );
            assert_eq!(cwd_events(&bytes), [path.clone()], "yol: {path}");
        }

        // The root directory: the edge where the path is a single character.
        let bytes = run_script("source $ZDOTDIR/bateri.zsh; cd -q -- /; __bateri_cwd", &[]);
        assert_eq!(cwd_events(&bytes), ["/"]);

        std::fs::remove_dir_all(&root).expect("the temporary directory could not be deleted");
    }

    #[test]
    fn the_script_prints_the_branch_from_the_repository() {
        // While there is no repository the branch is empty; the separator drops with it
        // (`dock::render`). The temporary directory is **not a repository**, so this arm
        // witnesses the repository's absence, not its presence.
        let outside = std::env::temp_dir();
        let bytes = run_script(
            "source $ZDOTDIR/bateri.zsh; cd -q -- $T_DIR; __bateri_branch_print",
            &[("T_DIR", &outside.to_string_lossy())],
        );
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        scanner.feed(&bytes, |event| log.apply_scan(event));
        assert_eq!(log.context.branch, "");

        // Inside the repository the branch name arrives. Our own repository: if `git` is
        // absent the test is not skipped, the branch stays empty and the claim says so.
        let inside = script_path();
        let bytes = run_script(
            "source $ZDOTDIR/bateri.zsh; cd -q -- $T_DIR; __bateri_branch_print",
            &[("T_DIR", &inside.to_string_lossy())],
        );
        scanner.feed(&bytes, |event| log.apply_scan(event));
        assert!(
            !log.context.branch.is_empty(),
            "the branch stayed empty inside the repository: {:?}",
            log.context.branch
        );
    }

    #[test]
    fn the_three_arms_do_not_touch_each_others_buffers() {
        // The mirror's wide bound must not loosen 133's narrow bound; 133's narrow bound
        // must not cut the mirror either. The directory arm's bound is a third budget too.
        // The proof that the buffers are separate.
        let mut scanner = Scanner::new();
        let mut marks = Vec::new();
        let mut lines = Vec::new();
        let mut paths = Vec::new();
        let mut stream = dock_update(2, "", "ls", "", &[]);
        stream.extend_from_slice(b"\x1b]133;B\x07");
        stream.extend_from_slice(b"\x1b]7;file:///tmp\x07");
        stream.extend_from_slice(&dock_update(3, "", "lsx", "", &[]));
        scanner.feed(&stream, |event| match event {
            ScanEvent::Mark(mark) => marks.push(mark),
            ScanEvent::Dock(DockEvent::Update(line)) => lines.push(line.buffer.clone()),
            ScanEvent::Dock(_) => {}
            ScanEvent::Cwd { path, .. } => paths.push(path.to_owned()),
            ScanEvent::PasteOn
            | ScanEvent::RemoteSetup(_)
            | ScanEvent::RemoteUp(_)
            | ScanEvent::RemoteMark(_) => {}
        });

        assert_eq!(marks, vec![Mark::PromptEnd]);
        assert_eq!(lines, vec!["ls".to_string(), "lsx".to_string()]);
        assert_eq!(paths, vec!["/tmp".to_string()]);
        assert_eq!(scanner.payload.capacity(), PAYLOAD_LIMIT);
        assert_eq!(scanner.dock.capacity(), DOCK_PAYLOAD_LIMIT);
        assert_eq!(scanner.cwd.capacity(), CWD_PAYLOAD_LIMIT);
    }

    #[test]
    fn a_dock_sequence_split_at_every_byte_survives() {
        let sequence = dock_update(1, "p", "ab", "c", &["0 1 fg=red"]);
        let expected = dock_line(&sequence);
        for at in 0..=sequence.len() {
            let (head, tail) = sequence.split_at(at);
            let seen = dock_events_of_chunks(&[head, tail]);
            assert_eq!(
                seen,
                vec![DockSnapshot::Update(expected.clone())],
                "split point {at}"
            );
        }
    }

    #[test]
    fn the_log_clears_the_mirror_when_it_cannot_be_drawn() {
        // Leaving stale text would mean the dock showing the previous command while the
        // grid is suppressed.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let staged = DockState {
            status: DockStatus::Live,
            buffer: "git status".to_string(),
            cursor: 10,
            ..DockState::default()
        };
        log.apply_scan(ScanEvent::Dock(DockEvent::Update(&staged)));
        assert_eq!(log.dock.buffer, "git status");

        log.apply_scan(ScanEvent::Dock(DockEvent::Unavailable(DockFault::Overflow)));
        assert_eq!(
            log.dock.status,
            DockStatus::Unavailable(DockFault::Overflow)
        );
        assert_eq!(log.dock.buffer, "");
        assert_eq!(log.dock.cursor, 0);

        log.apply_scan(ScanEvent::Dock(DockEvent::Update(&staged)));
        log.apply_scan(ScanEvent::Dock(DockEvent::End));
        assert_eq!(log.dock.status, DockStatus::Idle);
        assert_eq!(log.dock.buffer, "");
    }

    #[test]
    fn a_new_buffer_clears_the_dock_selection_and_a_new_prompt_does_not() {
        // 031 R3.4: the selection's indices are `BUFFER`'s characters; when `BUFFER`
        // changes they would point at another text. The prompt being redrawn or a change
        // of suggestion does not move the selected text.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut staged = DockState {
            status: DockStatus::Live,
            predisplay: "% ".to_string(),
            buffer: "git status".to_string(),
            cursor: 12,
            ..DockState::default()
        };
        log.apply_scan(ScanEvent::Dock(DockEvent::Update(&staged)));
        let word = DockPoint {
            index: 0,
            half: CellHalf::Left,
        };
        let select = |log: &mut ShellLog| {
            log.dock_selection = Some(DockSelection::new(
                SelectKind::Word,
                word,
                word,
                &log.dock.buffer,
                false,
            ));
        };
        select(&mut log);
        assert_eq!(log.dock_selection.and_then(|s| s.range()), Some((0, 3)));

        staged.predisplay = "%% ".to_string();
        staged.postdisplay = " -s".to_string();
        log.apply_scan(ScanEvent::Dock(DockEvent::Update(&staged)));
        assert!(
            log.dock_selection.is_some(),
            "a prompt change deleted the selection"
        );

        staged.buffer = "git statu".to_string();
        log.apply_scan(ScanEvent::Dock(DockEvent::Update(&staged)));
        assert_eq!(
            log.dock_selection, None,
            "a new BUFFER did not delete the selection"
        );

        for (name, event) in [
            ("End", DockEvent::End),
            ("Unavailable", DockEvent::Unavailable(DockFault::Overflow)),
        ] {
            log.apply_scan(ScanEvent::Dock(DockEvent::Update(&staged)));
            select(&mut log);
            log.apply_scan(ScanEvent::Dock(event));
            assert_eq!(
                log.dock_selection, None,
                "{name} did not delete the selection"
            );
        }
    }

    #[test]
    fn shift_arrows_step_the_moving_end_of_the_selection() {
        let range = |selection: DockSelection| selection.range;
        // If there is no selection it starts from the caret.
        let one = DockSelection::stepped(None, 2, true, "abcd", false);
        assert_eq!(range(one), (2, 3));
        let two = DockSelection::stepped(Some(one), 2, true, "abcd", false);
        assert_eq!(range(two), (2, 4));
        // It stays at the end of the line.
        assert_eq!(
            range(DockSelection::stepped(Some(two), 2, true, "abcd", false)),
            (2, 4)
        );
        let back = DockSelection::stepped(Some(two), 2, false, "abcd", false);
        assert_eq!(range(back), (2, 3));
        // A selection that descends to empty does not lose its end: the next step is from there.
        let empty = DockSelection::stepped(Some(back), 2, false, "abcd", false);
        assert_eq!((empty.range(), range(empty)), (None, (2, 2)));
        assert_eq!(
            range(DockSelection::stepped(Some(empty), 0, false, "abcd", false)),
            (1, 2)
        );

        // If the head is to the left of the anchor the moving end is the range's start (a
        // mouse selection dragged left).
        let point = |index| DockPoint {
            index,
            half: CellHalf::Left,
        };
        let leftward = DockSelection::new(SelectKind::Simple, point(3), point(1), "abcd", false);
        assert_eq!(
            range(DockSelection::stepped(
                Some(leftward),
                0,
                false,
                "abcd",
                false
            )),
            (0, 3)
        );
        assert_eq!(
            range(DockSelection::stepped(
                Some(leftward),
                0,
                true,
                "abcd",
                false
            )),
            (2, 3)
        );

        // A word selection grows with a letter step, its end is the moving one.
        let word = DockSelection::new(SelectKind::Word, point(1), point(1), "ab cd", false);
        assert_eq!(range(word), (0, 2));
        assert_eq!(
            range(DockSelection::stepped(Some(word), 0, true, "ab cd", false)),
            (0, 3)
        );

        // The combining mark does not separate from its base: `é` = `e` + U+0301.
        let text = "e\u{301}x";
        assert_eq!(
            range(DockSelection::stepped(None, 0, true, text, false)),
            (0, 2)
        );
        assert_eq!(
            range(DockSelection::stepped(None, 3, false, text, false)),
            (2, 3)
        );
        assert_eq!(
            range(DockSelection::stepped(None, 2, false, text, false)),
            (0, 2)
        );
    }

    #[test]
    fn shift_arrows_step_over_a_cluster_whole() {
        // 035 R4.2: in `a🇹🇷b`, ⇧← takes the flag whole from the end, ⇧→ takes the flag
        // after `a` from the start. In the closed reading the step is a code point.
        let range = |selection: DockSelection| selection.range;
        let text = "a🇹🇷b";
        let back = DockSelection::stepped(None, 3, false, text, true);
        assert_eq!(range(back), (1, 3));
        // The caret is between two RIs: both directions take the flag whole.
        assert_eq!(
            range(DockSelection::stepped(None, 2, false, text, true)),
            (1, 3)
        );
        assert_eq!(
            range(DockSelection::stepped(None, 2, true, text, true)),
            (1, 3)
        );
        let forward = DockSelection::stepped(None, 1, true, text, true);
        assert_eq!(range(forward), (1, 3));
        assert_eq!(
            range(DockSelection::stepped(Some(forward), 1, true, text, true)),
            (1, 4)
        );
        assert_eq!(
            range(DockSelection::stepped(None, 3, false, text, false)),
            (2, 3)
        );
    }

    #[test]
    fn the_edit_capability_lives_for_one_prompt() {
        // `w` is the editing gate's fourth condition (031 phase-5): it has no payload,
        // does not touch the mirror's state and goes with the prompt's lifetime —
        // `line-finish` (`e`) and the start of the prompt (`A`) both delete it.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        let mut stream = dock_update(0, "", "ls", "", &[]);
        stream.extend_from_slice(b"\x1b]8133;w\x07");
        scanner.feed(&stream, |event| log.apply_scan(event));
        assert!(log.dock_editable, "w did not set the capability");
        assert_eq!(log.dock.status, DockStatus::Live, "w moved the mirror");
        assert_eq!(log.dock.buffer, "ls");

        // Not every keystroke of the mirror deletes the capability: the scanner's
        // `clone_from` only refreshes the mirror.
        scanner.feed(&dock_update(0, "", "ls -l", "", &[]), |event| {
            log.apply_scan(event)
        });
        assert!(log.dock_editable, "the mirror deleted the capability");

        scanner.feed(b"\x1b]8133;e\x07", |event| log.apply_scan(event));
        assert!(
            !log.dock_editable,
            "line-finish did not delete the capability"
        );

        scanner.feed(b"\x1b]8133;w\x07", |event| log.apply_scan(event));
        assert!(log.dock_editable);
        scanner.feed(b"\x1b]133;A;bt_block=3\x07", |event| log.apply_scan(event));
        assert!(
            !log.dock_editable,
            "the start of the prompt did not delete the capability"
        );
    }

    #[test]
    fn the_script_arms_the_widget_before_it_announces_it() {
        // The script's `line-init` hook: binding to three keymaps, then `w`. It runs
        // without ZLE (`zsh -f -c`), so what is tested is the two ends of the wire
        // speaking the same letter; the binding in a real ZLE is seen by `session.rs`'s
        // end-to-end tests.
        let bytes = run_script(
            "source $ZDOTDIR/bateri.zsh
             zmodload zsh/zle
             __bateri_dock_arm
             for map in main emacs viins; do bindkey -M $map $'\\e[8133~'; done",
            &[],
        );
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.starts_with("\x1b]8133;w\x07"),
            "the capability was not printed: {text:?}"
        );
        assert_eq!(
            text.matches("__bateri_dock_edit").count(),
            3,
            "the widget was not bound to three keymaps: {text:?}"
        );
        assert_eq!(dock_events(&bytes[..9]), vec![DockSnapshot::Editable]);
    }

    #[test]
    fn the_mirror_reuses_its_buffers() {
        // R1.3's criterion: no per-keystroke allocation in steady state. The capacity not
        // growing in the second round is the observable face of this.
        let mut scanner = Scanner::new();
        let long = "x".repeat(200);
        let sequence = dock_update(0, "", &long, "", &[]);
        scanner.feed(&sequence, |_| {});
        let capacity = scanner.line.buffer.capacity();
        for _ in 0..10 {
            scanner.feed(&sequence, |_| {});
        }
        assert_eq!(scanner.line.buffer.capacity(), capacity);
    }

    #[test]
    fn the_capacity_follows_the_scrollback() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR * 4);
        for id in 1..=(BLOCK_LOG_FLOOR as u32 * 4) {
            run_block(&mut log, id, Some(0));
        }
        // If it had been the floor this block would have long since dropped.
        assert_eq!(exit_of(&log, BLOCK_LOG_FLOOR as u32 + 1), Some(Some(0)));
    }

    /// The counter's four tiers; **both sides** of the boundaries are tested.
    ///
    /// Writing a `<` instead of `<=` at a tier boundary would be a defect with no
    /// symptom: a counter that writes `10.0s` instead of `10s` is not wrong, just
    /// outside the design — and no compiler sees it.
    #[test]
    fn the_counter_reads_its_four_tiers() {
        let text = |ms| {
            Counter::new(Duration::from_millis(ms), Precision::Tenths)
                .as_str()
                .to_owned()
        };

        // **The decimal in a finished value, across the whole seconds tier.** From just
        // above the threshold to just below a minute: the boundary is **not** ten seconds
        // (user decision) — a frozen value pays nothing for the decimal and the "real
        // number" question holds after ten seconds too.
        assert_eq!(text(1_000), "1.0s");
        assert_eq!(text(1_449), "1.4s");
        assert_eq!(text(9_999), "9.9s");
        assert_eq!(text(10_000), "10.0s");
        assert_eq!(text(45_300), "45.3s");
        assert_eq!(text(59_999), "59.9s");
        // Truncation, not rounding: 1.49 seconds is "1.4s", not "1.5s". The counter
        // should be honest backwards, not forwards.
        assert_eq!(text(1_499), "1.4s");

        // From the minute the decimal drops: `1m 05.3s` is both long and unreadable, what
        // is sought there is the rough magnitude. The seconds are two digits, otherwise
        // "1m 5s" and "1m 50s" get confused.
        assert_eq!(text(60_000), "1m 00s");
        assert_eq!(text(65_000), "1m 05s");
        assert_eq!(text(3_599_999), "59m 59s");

        // Saat.
        assert_eq!(text(3_600_000), "1h 00m");
        assert_eq!(text(3_720_000), "1h 02m");
    }

    /// **A running counter shows no decimal**, a finished one does.
    ///
    /// The reason for the distinction is both reading and battery: every change of a
    /// running counter requests a frame, so tenths would be **ten frames per second**.
    /// If the distinction is removed this goes red and the clock would quietly climb
    /// to 10 Hz.
    ///
    /// Tested at both ends, because the decimal now extends up to the minute: a
    /// running `45s` and a finished `45.3s` are born from the same duration.
    #[test]
    fn a_running_counter_costs_one_frame_a_second() {
        let elapsed = Duration::from_millis(3_400);
        assert_eq!(
            Counter::new(elapsed, Precision::Whole).as_str(),
            "3s",
            "the running counter shows a decimal"
        );
        assert_eq!(Counter::new(elapsed, Precision::Tenths).as_str(), "3.4s");

        // Above the old ceiling of tenths (10 s): a running one is still whole seconds.
        let long = Duration::from_millis(45_300);
        assert_eq!(
            Counter::new(long, Precision::Whole).as_str(),
            "45s",
            "the running counter must stay whole seconds after ten seconds too"
        );
        assert_eq!(Counter::new(long, Precision::Tenths).as_str(), "45.3s");

        // The tick is set to the whole second: at 3.4 seconds 600 ms remain.
        assert_eq!(next_tick(elapsed), Duration::from_millis(600));
        // On the whole second not zero but **one** second: a zero-duration clock would
        // put the callback in a loop.
        assert_eq!(next_tick(Duration::from_secs(3)), Duration::from_secs(1));
        // Below the threshold the next change is the counter's **appearance**.
        assert_eq!(
            next_tick(Duration::from_millis(200)),
            Duration::from_millis(800)
        );
    }

    /// In the hour tier the tick is **once a minute**, not once a second.
    ///
    /// The text (`1h 07m`) changes once a minute; waking every second would have an
    /// hour draw 3540 **identical** frames and we would be the first to violate the
    /// "content must genuinely change" condition we wrote into the module header
    /// (`/code-review`, 013 gate).
    #[test]
    fn the_hour_tier_ticks_once_a_minute() {
        // 1 hour 7 minutes 20 seconds: 40 seconds to the next minute.
        let elapsed = Duration::from_secs(3600 + 7 * 60 + 20);
        assert_eq!(Counter::new(elapsed, Precision::Whole).as_str(), "1h 07m");
        assert_eq!(next_tick(elapsed), Duration::from_secs(40));

        // On the whole minute not zero but **one minute**: a zero-duration clock would put
        // the callback in a loop.
        assert_eq!(
            next_tick(Duration::from_secs(3600)),
            Duration::from_secs(60)
        );

        // Below the boundary it is still once a second: `59m 59s` changes every second.
        assert_eq!(next_tick(Duration::from_secs(3599)), Duration::from_secs(1));
    }

    /// A lost `D` is not written to the **next** block.
    ///
    /// If the clock were consumed only at `D`, after an OSC cut halfway a stale
    /// `Instant` would stay standing and the next block's `D` would consume it: an
    /// instant command would look like it took minutes (`/code-review`, 013 gate). `A`
    /// is the second reset point.
    #[test]
    fn a_lost_command_end_does_not_charge_the_next_block() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        // The 1st block is running but its `D` never arrives.
        log.apply(Mark::PromptStart { id: Some(1) });
        log.apply(Mark::CommandStart);
        assert!(log.local.running_since.is_some());

        // Block 2's prompt: the clock must be reset here.
        log.apply(Mark::PromptStart { id: Some(2) });
        assert!(
            log.local.running_since.is_none(),
            "`A` should have cleared the stale clock"
        );

        // Block 2 closes without seeing `C` (the shell prints `D` anyway).
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: Some(2),
        });
        assert_eq!(
            log.duration(BlockKey::Local(2), RunningBlocks::default()),
            Some(Duration::ZERO),
            "a lost `D` wrote a duration to the next block"
        );
    }

    /// **A second OSC 133 source does not steal the clock.**
    ///
    /// If iTerm2's integration is installed in the user's shell
    /// (`~/.iterm2_shell_integration.zsh`) every command produces **two** `C`s and
    /// **two** `D`s; its own is identity-less, ours carries `bt_block=`. The sequence
    /// was measured on a real machine and is exactly this.
    ///
    /// The identity-less `D` used to consume the clock: ours found it empty, the
    /// duration was written as **zero** and the counter stayed below the threshold and
    /// was never drawn. That was the defect the user saw and no test could see it,
    /// because all of them assumed a single-source stream.
    #[test]
    fn a_foreign_integration_does_not_steal_the_clock() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::PromptStart { id: Some(1) });
        log.apply(Mark::PromptEnd);
        // Two `C`s: foreign + ours. The first must win.
        log.apply(Mark::CommandStart);
        log.apply(Mark::CommandStart);
        std::thread::sleep(Duration::from_millis(60));
        // Two `D`s: first the foreign identity-less one, then ours.
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: None,
        });
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: Some(1),
        });
        log.apply(Mark::PromptStart { id: Some(2) });

        let measured = log
            .duration(BlockKey::Local(1), RunningBlocks::default())
            .expect("a finished block must have a duration");
        assert!(
            measured >= Duration::from_millis(50),
            "a foreign `D` stole the clock, the duration fell to zero: {measured:?}"
        );
    }

    /// The buffer's ceiling takes the longest representable text.
    ///
    /// [`Counter::CAPACITY`] is a derivation, not a guess: the duration is `u32`
    /// milliseconds, that is at most ~1193 hours. If the ceiling is reduced,
    /// `Counter::new`'s `debug_assert` blows up here — in a release build the text
    /// would be silently truncated.
    #[test]
    fn the_longest_counter_fits_the_buffer() {
        let longest = Counter::new(
            Duration::from_millis(u64::from(u32::MAX)),
            Precision::Tenths,
        );
        assert_eq!(longest.as_str(), "1193h 02m");
        assert!(
            longest.as_str().len() <= Counter::CAPACITY,
            "the counter buffer does not take the longest text: {}",
            longest.as_str()
        );
    }

    /// A block whose `D` arrives without `C` records a **zero** duration, not a
    /// made-up one.
    ///
    /// The path is real: a `D` arriving after an identity-less `A`, or a shell that
    /// prints half the integration. Zero stays below the threshold, so the counter is
    /// not drawn — the duration arm of the "the unknown is not drawn" rule.
    #[test]
    fn a_command_that_never_started_records_no_time() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::PromptStart { id: Some(1) });
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: Some(1),
        });

        let duration = log
            .duration(BlockKey::Local(1), RunningBlocks::default())
            .expect("a finished block must have a duration");
        assert_eq!(duration, Duration::ZERO);
        assert!(
            duration < COUNTER_FLOOR,
            "a zero duration must not pass the threshold"
        );
    }

    /// The clock is **consumed** at `D`: between two commands there is no running
    /// command.
    ///
    /// If it were a read instead of `take`, in the `Finished` phase (which contains a
    /// `git` fork) a finished command would still look like it was counting and in
    /// phase-2 the clock would never stop.
    #[test]
    fn the_clock_is_spent_when_the_command_ends() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::PromptStart { id: Some(1) });
        log.apply(Mark::CommandStart);
        assert!(
            log.local.running_since.is_some(),
            "`C` should have planted the clock"
        );

        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: Some(1),
        });
        assert!(
            log.local.running_since.is_none(),
            "`D` should have consumed the clock"
        );
    }

    #[test]
    fn the_title_prefers_the_application_then_the_directory() {
        let home = Path::new("/Users/someone");
        // OSC 0/2 wins, whatever the directory.
        assert_eq!(title_of(Some("vim"), Some("/tmp"), Some(home), None), "vim");
        // An empty (or whitespace-only) OSC title is ignored and falls to the directory.
        assert_eq!(title_of(Some(""), Some("/tmp"), Some(home), None), "tmp");
        assert_eq!(title_of(Some("  "), Some("/tmp"), Some(home), None), "tmp");
        // `ResetTitle` deletes the slot: the title returns to the directory.
        assert_eq!(
            title_of(None, Some("/usr/local/bin"), Some(home), None),
            "bin"
        );
        // The home directory itself is `~`, its subdirectory the last component.
        assert_eq!(
            title_of(None, Some("/Users/someone"), Some(home), None),
            "~"
        );
        assert_eq!(
            title_of(None, Some("/Users/someone/"), Some(home), None),
            "~"
        );
        assert_eq!(
            title_of(None, Some("/Users/someone/proj"), Some(home), None),
            "proj"
        );
        // If the home directory is unknown, home is an ordinary directory too.
        assert_eq!(
            title_of(None, Some("/Users/someone"), None, None),
            "someone"
        );
        // Root.
        assert_eq!(title_of(None, Some("/"), Some(home), None), "/");
        // None: the application's name.
        assert_eq!(title_of(None, None, Some(home), None), "bateri");
        assert_eq!(title_of(None, Some(""), Some(home), None), "bateri");
    }

    #[test]
    fn a_remote_session_marks_the_title() {
        // 036 Karar 5: while remote is active the directory is never consulted; the OSC
        // title with the prefix, otherwise the host.
        let home = Path::new("/Users/someone");
        let remote = Some("prod");
        assert_eq!(
            title_of(Some("deploy@prod: ~"), Some("/tmp"), Some(home), remote),
            "⇄ deploy@prod: ~"
        );
        assert_eq!(title_of(None, Some("/tmp"), Some(home), remote), "⇄ prod");
        // An empty OSC title is ignored: it falls to the host, not to the directory.
        assert_eq!(
            title_of(Some(" "), Some("/tmp"), Some(home), remote),
            "⇄ prod"
        );
        // The host even when there is no directory.
        assert_eq!(title_of(None, None, None, remote), "⇄ prod");
    }

    #[test]
    fn only_a_different_directory_changes_the_title_input() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let title = |log: &mut ShellLog, event| log.apply_scan_answering(event, 0).title;
        assert!(title(&mut log, local_cwd("/tmp")));
        // A second `precmd` printing the same directory produces no notification.
        assert!(!title(&mut log, local_cwd("/tmp")));
        assert!(title(&mut log, local_cwd("/")));
        // Non-directory events never touch the title's input.
        assert!(!title(&mut log, ScanEvent::Mark(Mark::PromptEnd)));
        assert!(!title(&mut log, ScanEvent::Dock(DockEvent::End)));
        // While there is no remote state `A` and `D` do not touch it either.
        assert!(!title(
            &mut log,
            ScanEvent::Mark(Mark::PromptStart { id: None })
        ));
        assert_eq!(log.context.cwd, "/");
    }

    fn local_cwd(path: &str) -> ScanEvent<'_> {
        ScanEvent::Cwd { path, local: true }
    }

    fn foreign_cwd(path: &str) -> ScanEvent<'_> {
        ScanEvent::Cwd { path, local: false }
    }

    /// A ledger with a command running (`C`); its generation and phase are ready.
    fn running_log() -> ShellLog {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::PromptStart { id: Some(1) });
        log.apply(Mark::PromptEnd);
        log.apply(Mark::CommandStart);
        log
    }

    #[test]
    fn only_the_transition_into_running_starts_a_command() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        assert_eq!(log.command, 0);
        let first = log.apply(Mark::CommandStart);
        assert!(first.started, "the first `C` is a transition");
        assert_eq!(log.command, 1);
        // A second `C` (iTerm2) is not a transition: the generation does not move, no notification.
        assert!(!log.apply(Mark::CommandStart).started);
        assert_eq!(log.command, 1);
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: None,
        });
        log.apply(Mark::PromptStart { id: None });
        assert!(log.apply(Mark::CommandStart).started);
        assert_eq!(log.command, 2);
    }

    #[test]
    fn the_second_command_start_keeps_the_remote_host() {
        let mut log = running_log();
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        // A second `C` in the middle of a command does not delete the remote state: the
        // probe is locked until `D` and the indicator would not come back.
        let outcome = log.apply(Mark::CommandStart);
        assert_eq!(outcome, ScanOutcome::default());
        assert_eq!(log.context.remote_host(), Some("prod"));
    }

    #[test]
    fn end_and_prompt_clear_the_remote_state() {
        for mark in [
            Mark::CommandEnd {
                exit: Some(0),
                id: Some(1),
            },
            Mark::PromptStart { id: Some(2) },
        ] {
            let mut log = running_log();
            assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
            log.apply_scan_answering(foreign_cwd("/srv"), 0);
            assert_eq!(log.context.remote_cwd, "/srv");
            let outcome = log.apply(mark);
            assert!(outcome.title, "{mark:?}: deletion is the title's input");
            assert_eq!(log.context.remote, None, "{mark:?}");
            assert_eq!(log.context.remote_cwd, "", "{mark:?}");
            // Deleting an already deleted state is not a notification.
            assert!(!log.apply(mark).title, "{mark:?}");
        }
    }

    #[test]
    fn only_our_identified_prompt_announces_the_prompt() {
        // The trigger of the session's first input (037 Karar 6): only an identified `A`.
        // An identity-less `A` (another tool's integration) and the other marks do not say
        // our shell reached the prompt.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        for mark in [
            Mark::PromptStart { id: None },
            Mark::PromptEnd,
            Mark::CommandStart,
            Mark::CommandEnd {
                exit: Some(0),
                id: Some(1),
            },
        ] {
            assert!(!log.apply(mark).prompt, "{mark:?}");
        }
        assert!(log.apply(Mark::PromptStart { id: Some(2) }).prompt);
        // While a remote session is going too: an identified `A` passes, deleting the remote state.
        let mut log = running_log();
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        assert!(!log.apply(Mark::PromptStart { id: None }).prompt);
        assert!(log.apply(Mark::PromptStart { id: Some(3) }).prompt);
    }

    #[test]
    fn foreign_marks_leave_a_remote_session_alone() {
        // The fish 4 / kitty integration at the far end of ssh: the remote `A`, `B`, `C`,
        // `D` do not carry our identity and neither delete the remote state, nor advance
        // the generation, nor take the phase out of `Running`.
        let mut log = running_log();
        let command = log.running_command();
        assert!(command.is_some());
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        for mark in [
            Mark::PromptStart { id: None },
            Mark::PromptEnd,
            Mark::CommandStart,
            Mark::CommandEnd {
                exit: Some(1),
                id: None,
            },
        ] {
            assert_eq!(log.apply(mark), ScanOutcome::default(), "{mark:?}");
            assert_eq!(log.context.remote_host(), Some("prod"), "{mark:?}");
            assert_eq!(log.running_command(), command, "{mark:?}");
        }
        // Our `D` ends the command and deletes the remote state.
        let outcome = log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: Some(1),
        });
        assert!(outcome.title);
        assert_eq!(log.context.remote, None);
        assert_eq!(log.running_command(), None);
    }

    #[test]
    fn a_foreign_prompt_before_the_probe_keeps_the_command_running() {
        // The remote `A` arrived in the same read before the probe: the phase went back to
        // `Prompt` but our `D` did not arrive, so the command is running and the probe's
        // answer is accepted.
        let mut log = running_log();
        let command = log.running_command();
        log.apply(Mark::PromptStart { id: None });
        log.apply(Mark::PromptEnd);
        assert_eq!(log.running_command(), command);
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: Some(1),
        });
        assert_eq!(log.running_command(), None);
    }

    #[test]
    fn a_foreign_shell_without_a_remote_session_drives_the_phase() {
        // `exec fish`: our identity never arrives again. While there is no remote session
        // fish's marks drive the phase, otherwise `Running` would never end and the clock
        // would request frames while idle.
        let mut log = running_log();
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: None,
        });
        log.apply(Mark::PromptStart { id: None });
        assert_ne!(
            log.local.state.map(|state| state.phase),
            Some(ShellPhase::Running)
        );
        assert_eq!(log.local.running_since, None, "the clock stopped");
    }

    #[test]
    fn without_our_marks_foreign_marks_still_drive_the_phase() {
        // A shell with integration off + its own 133: our identity never arrived, so an
        // identity-less `D` has to end the command — otherwise `Running` would go on
        // forever.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::PromptStart { id: None });
        log.apply(Mark::CommandStart);
        assert!(log.running_command().is_some());
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: None,
        });
        assert_eq!(log.running_command(), None);
    }

    #[test]
    fn a_new_command_clears_a_foreign_directory() {
        // A foreign OSC 7 that arrives before the probe waits in the remote slot; but the
        // leftover from the previous command is not carried over to the next.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply_scan_answering(foreign_cwd("/srv"), 0);
        assert_eq!(log.context.remote_cwd, "/srv");
        log.apply(Mark::CommandStart);
        assert_eq!(log.context.remote_cwd, "");
    }

    #[test]
    fn osc7_routes_by_authority_and_remote_state() {
        let mut log = running_log();
        // A local authority goes to the local directory as today.
        assert!(log.apply_scan_answering(local_cwd("/Users/me"), 0).title);
        // A foreign authority to the remote slot; the local directory and the title do not move.
        let outcome = log.apply_scan_answering(foreign_cwd("/var/www"), 0);
        assert!(!outcome.title);
        assert_eq!(
            (log.context.cwd.as_str(), log.context.remote_cwd.as_str()),
            ("/Users/me", "/var/www")
        );
        // While remote is active **an empty authority too** goes to the remote slot: the
        // local shell is behind ssh with blocks.
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        let outcome = log.apply_scan_answering(local_cwd("/home/deploy"), 0);
        assert!(!outcome.title);
        assert_eq!(
            (log.context.cwd.as_str(), log.context.remote_cwd.as_str()),
            ("/Users/me", "/home/deploy")
        );
    }

    #[test]
    fn set_remote_reports_only_a_change() {
        let mut log = running_log();
        assert!(!log.set_remote(None), "none → none is not a change");
        assert!(
            !log.set_remote(Some(&RemoteTarget::ssh(""))),
            "an empty host is not remote"
        );
        assert!(
            !log.set_remote(Some(&RemoteTarget::ssh("prod\n"))),
            "control character"
        );
        assert!(
            !log.set_remote(Some(&RemoteTarget::ssh("\u{1b}[31mprod"))),
            "ESC"
        );
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        assert!(!log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        assert!(log.set_remote(Some(&RemoteTarget::ssh("deploy@10.0.0.5"))));
        assert_eq!(log.context.remote_host(), Some("deploy@10.0.0.5"));
        assert!(log.set_remote(None));
        assert_eq!(log.context.remote, None);
    }

    #[test]
    fn the_login_signals_belong_to_the_running_command() {
        // 047 R9.1: termios decides the modes, the output's signals only while
        // remote and only since `C`.
        assert!(
            !TtyModes {
                canonical: true,
                echo: true
            }
            .logged_in(),
            "host key question"
        );
        assert!(
            !TtyModes {
                canonical: true,
                echo: false
            }
            .logged_in(),
            "password prompt"
        );
        assert!(
            !TtyModes {
                canonical: false,
                echo: true
            }
            .logged_in()
        );
        assert!(
            TtyModes {
                canonical: false,
                echo: false
            }
            .logged_in(),
            "logged in"
        );

        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        // The local prompt's own `?2004h`, and one from the command line before
        // ssh (`ssh $(fzf)`), are not recorded — in stream order, as the scanner
        // hands them over within one read.
        let mut scanner = Scanner::new();
        scanner.feed(
            b"\x1b]133;A;bt_block=1\x07\x1b[?2004h\x1b]133;B\x07\x1b[?2004l\x1b]133;C\x07\x1b[?2004h",
            |event| {
                log.apply_scan_answering(event, 0);
            },
        );
        assert_eq!(log.running_command(), Some(1));
        assert!(!log.paste_since_remote);
        assert!(!log.login_signalled(false), "not remote");
        log.set_remote(Some(&RemoteTarget::ssh("prod")));
        assert!(!log.paste_since_remote, "before the remote state");
        assert!(!log.login_signalled(false), "nothing came yet");
        assert!(log.login_signalled(true), "a title of the shell's shape");
        log.note_paste_on();
        assert!(log.login_signalled(false));
        // A new command clears it, and the remote state with it.
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: Some(1),
        });
        log.apply(Mark::PromptStart { id: Some(2) });
        log.apply(Mark::PromptEnd);
        log.apply(Mark::CommandStart);
        log.set_remote(Some(&RemoteTarget::ssh("prod")));
        assert!(!log.login_signalled(false));
        log.context.remote_cwd.push_str("/srv");
        assert!(log.login_signalled(false), "a remote OSC 7");
    }

    /// A load sample for the clearing tests: the value itself is not asked.
    fn sample(mem: u8) -> RemoteStats {
        RemoteStats {
            cpu: Some(20),
            mem,
            ..RemoteStats::default()
        }
    }

    #[test]
    fn the_load_goes_with_the_remote_state() {
        // 046 Karar 5: `C`/`D`/`A` and a new target clear the indicator — a new
        // host must not show the previous one's numbers.
        let remote = |log: &mut ShellLog| {
            assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
            log.context.stats = Some(sample(40));
        };
        for mark in [
            Mark::CommandEnd {
                exit: Some(0),
                id: Some(1),
            },
            Mark::PromptStart { id: Some(2) },
        ] {
            let mut log = running_log();
            remote(&mut log);
            log.apply(mark);
            assert_eq!(log.context.stats, None, "{mark:?}");
        }
        // `C`'s transition into `Running`: in a shell that never showed our
        // identity (with it, an identity-less `C` is the far end's and ignored).
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::PromptStart { id: None });
        log.apply(Mark::PromptEnd);
        remote(&mut log);
        log.apply(Mark::CommandStart);
        assert_eq!(log.context.stats, None, "C");

        // The same host re-reported keeps it; another host or none clears it.
        let mut log = running_log();
        remote(&mut log);
        assert!(!log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        assert_eq!(log.context.stats, Some(sample(40)), "same host");
        assert!(log.set_remote(Some(&RemoteTarget::ssh("stage"))));
        assert_eq!(log.context.stats, None, "another host");
        log.context.stats = Some(sample(40));
        assert!(log.set_remote(None));
        assert_eq!(log.context.stats, None, "local");
    }

    #[test]
    fn the_load_compares_only_what_is_drawn() {
        // The history's slots past `len` do not take part (the equality gate).
        let mut a = sample(40);
        a.history = [1, 2, 0, 0, 0, 0, 0, 0];
        a.len = 2;
        let mut b = a;
        b.history[5] = 7;
        assert_eq!(a, b);
        assert_eq!(a.history(), [1, 2]);
        b.history[1] = 3;
        assert_ne!(a, b);
        assert_ne!(sample(40), sample(41));
        assert_eq!(RemoteStatsMode::Off.form(), None);
        assert_eq!(
            RemoteStatsMode::Sparkline.form(),
            Some(StatsForm::Sparkline)
        );
    }

    fn rule(pattern: &str, mark: HostMark) -> HostRule {
        HostRule {
            pattern: pattern.to_owned(),
            mark: Some(mark),
            integration: None,
        }
    }

    #[test]
    fn the_remote_target_is_kept_whole_and_its_mark_resolved() {
        // 037 Karar 1, 2: the target stands as a whole; the mark is resolved in
        // `set_remote` and at a change of the list, and `C`/`D`/`A` delete it too.
        let mut log = running_log();
        assert!(!log.set_host_rules(&[rule("prod-*", HostMark::Production)]));
        let target = RemoteTarget {
            host: "deploy@prod-web-1".to_owned(),
            kind: RemoteKind::Ssh,
            argv: ["ssh", "-p", "2222", "deploy@prod-web-1"]
                .map(str::to_owned)
                .to_vec(),
            line: "ssh -p 2222 deploy@prod-web-1".to_owned(),
        };
        assert!(log.set_remote(Some(&target)));
        assert_eq!(log.context.remote.as_ref(), Some(&target));
        assert_eq!(log.context.remote_mark, HostMark::Production);

        // The same host, another argv: the target is written but the title's input is the same.
        let other = RemoteTarget {
            argv: ["ssh", "deploy@prod-web-1"].map(str::to_owned).to_vec(),
            line: "ssh deploy@prod-web-1".to_owned(),
            ..target.clone()
        };
        assert!(!log.set_remote(Some(&other)));
        assert_eq!(log.context.remote.as_ref(), Some(&other));

        // A change of list re-resolves the mark; the same list is a no-op, a list that
        // does not move the mark is `false`.
        let staging = [rule("*", HostMark::Staging)];
        assert!(log.set_host_rules(&staging));
        assert_eq!(log.context.remote_mark, HostMark::Staging);
        assert!(!log.set_host_rules(&staging));
        assert!(!log.set_host_rules(&[rule("prod-web-?", HostMark::Staging)]));
        assert!(log.set_host_rules(&[]));
        assert_eq!(log.context.remote_mark, HostMark::None);

        // Our `D` deletes the remote state together with its mark.
        assert!(log.set_host_rules(&staging));
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: Some(1),
        });
        assert_eq!(log.context.remote, None);
        assert_eq!(log.context.remote_mark, HostMark::None);
    }

    /// `D;{exit}` with our identity.
    fn our_end(exit: i32) -> Mark {
        Mark::CommandEnd {
            exit: Some(exit),
            id: Some(1),
        }
    }

    #[test]
    fn our_ssh_255_leaves_a_reconnect_offer() {
        // 037 Karar 8: remote ssh + our `D;255` → offer (host, resolved mark, line); `A`
        // does not delete it, the next `C` does.
        let mut log = running_log();
        log.set_host_rules(&[rule("prod", HostMark::Production)]);
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        log.apply(our_end(255));
        assert_eq!(
            log.context.remote, None,
            "the remote state is deleted again"
        );
        let offer = Reconnect {
            host: "prod".to_owned(),
            mark: HostMark::Production,
            line: "ssh prod".to_owned(),
        };
        assert_eq!(log.context.reconnect.as_ref(), Some(&offer));
        log.apply(Mark::PromptStart { id: Some(2) });
        log.apply(Mark::PromptEnd);
        assert_eq!(
            log.context.reconnect.as_ref(),
            Some(&offer),
            "`A`/`B` silmiyor"
        );
        // A change of mark refreshes the offer's color and reports it.
        assert!(log.set_host_rules(&[]));
        assert_eq!(
            log.context.reconnect.as_ref().map(|offer| offer.mark),
            Some(HostMark::None)
        );
        log.apply(Mark::CommandStart);
        assert_eq!(log.context.reconnect, None, "sonraki `C` siliyor");
    }

    #[test]
    fn only_our_ssh_255_makes_an_offer() {
        let mosh = RemoteTarget {
            kind: RemoteKind::Mosh,
            argv: vec!["mosh".to_owned(), "prod".to_owned()],
            line: "mosh prod".to_owned(),
            ..RemoteTarget::ssh("prod")
        };
        for (target, end) in [
            (RemoteTarget::ssh("prod"), our_end(0)),
            (RemoteTarget::ssh("prod"), our_end(1)),
            // mosh does not exit on a drop; its 255 does not carry this meaning.
            (mosh, our_end(255)),
        ] {
            let mut log = running_log();
            assert!(log.set_remote(Some(&target)));
            log.apply(end);
            assert_eq!(log.context.reconnect, None, "{end:?} {:?}", target.kind);
        }
        // While there is no remote session 255 is the code of a local command.
        let mut log = running_log();
        log.apply(our_end(255));
        assert_eq!(log.context.reconnect, None);
        // An identity-less `D;255`: in a session that has seen our identity it is the
        // remote shell's mark (touches nothing), and in one that has not it does not
        // produce an offer either.
        let foreign = Mark::CommandEnd {
            exit: Some(255),
            id: None,
        };
        let mut log = running_log();
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        log.apply(foreign);
        assert_eq!(log.context.reconnect, None);
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::CommandStart);
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        log.apply(foreign);
        assert_eq!(log.context.remote, None, "in a foreign shell `D` ends it");
        assert_eq!(log.context.reconnect, None);
    }

    #[test]
    fn a_new_remote_target_drops_the_offer() {
        let mut log = running_log();
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        log.apply(our_end(255));
        assert!(log.context.reconnect.is_some());
        log.apply(Mark::PromptStart { id: Some(2) });
        // A new remote session (the probe after `C`) does not leave the old one's offer —
        // `C` already deleted it, `set_remote` is the second belt.
        log.context.reconnect = Some(Reconnect::default());
        assert!(log.set_remote(Some(&RemoteTarget::ssh("staging"))));
        assert_eq!(log.context.reconnect, None);
    }

    #[test]
    fn the_title_gives_the_directory_only_in_the_user_at_host_shape() {
        assert_eq!(title_directory("root@kararla-production: ~"), Some("~"));
        assert_eq!(
            title_directory("deploy@web-01: /var/www/app"),
            Some("/var/www/app")
        );
        assert_eq!(
            title_directory("deploy@web-01: ~/My Drive"),
            Some("~/My Drive")
        );
        // oh-my-zsh's `termsupport`: `%n@%m:%~`, no space after the colon.
        assert_eq!(title_directory("tdgunes@tdg-fw13:~"), Some("~"));
        assert_eq!(
            title_directory("tdgunes@tdg-fw13:~/Projects"),
            Some("~/Projects")
        );
        assert_eq!(title_directory("vim - notes.txt"), None);
        assert_eq!(title_directory("deploy@web-01: relative"), None);
        assert_eq!(title_directory("deploy@web-01:relative"), None);
        assert_eq!(title_directory("host:/x"), None);
        assert_eq!(title_directory("a b@host: /x"), None);
        assert_eq!(title_directory("deploy@: /x"), None);
    }

    // ─── our remote shell's blocks (048 phase-3) ─────────────────────────

    fn events(bytes: &[u8]) -> Vec<RemoteMark> {
        let mut scanner = Scanner::new();
        let mut out = Vec::new();
        scanner.feed(bytes, |event| {
            if let ScanEvent::RemoteMark(mark) = event {
                out.push(mark);
            }
        });
        out
    }

    /// The remote shell `9` under the local block `parent`.
    fn shell(parent: u32) -> RemoteShell {
        RemoteShell { parent, pid: 9 }
    }

    fn remote(parent: u32, id: u32, mark: Mark) -> RemoteMark {
        RemoteMark {
            shell: shell(parent),
            id,
            mark,
        }
    }

    #[test]
    fn the_remote_field_routes_all_four_letters_to_their_own_arm() {
        assert_eq!(
            events(
                b"\x1b]133;A;bt_remote=7.9.1\x07\x1b]133;B;bt_remote=7.9.1\x07\
                  \x1b]133;C;bt_remote=7.9.1\x07\x1b]133;D;2;bt_remote=7.9.1\x07"
            ),
            vec![
                remote(7, 1, Mark::PromptStart { id: None }),
                remote(7, 1, Mark::PromptEnd),
                remote(7, 1, Mark::CommandStart),
                remote(
                    7,
                    1,
                    Mark::CommandEnd {
                        exit: Some(2),
                        id: None
                    }
                ),
            ]
        );
        // None of them is a local mark.
        assert!(
            marks(b"\x1b]133;A;bt_remote=7.9.1\x07\x1b]133;D;0;bt_remote=7.9.1\x07").is_empty()
        );
        // The field wins over a `bt_block=` beside it, and a code-less `D` keeps its identity.
        assert_eq!(
            events(b"\x1b]133;D;bt_block=3;bt_remote=7.9.2\x07"),
            vec![remote(
                7,
                2,
                Mark::CommandEnd {
                    exit: None,
                    id: None
                }
            )]
        );
        // A broken field is not ours: the letter stays a local, identity-less mark.
        for broken in [&b"7"[..], b"7.1", b"7.9.", b".9.1", b"x.9.1", b"7.9.1.2"] {
            let mut payload = b"\x1b]133;A;bt_remote=".to_vec();
            payload.extend_from_slice(broken);
            payload.push(0x07);
            assert!(events(&payload).is_empty(), "{broken:?}");
            assert_eq!(marks(&payload), vec![Mark::PromptStart { id: None }]);
        }
    }

    /// The local ssh command `P = 1` running under a remote session.
    fn ssh_log() -> ShellLog {
        let mut log = running_log();
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        log
    }

    fn feed(log: &mut ShellLog, bytes: &[u8]) -> ScanOutcome {
        let mut scanner = Scanner::new();
        let mut outcome = ScanOutcome::default();
        scanner.feed(bytes, |event| {
            let one = log.apply_scan_answering(event, 0);
            outcome.title |= one.title;
            outcome.started |= one.started;
            outcome.prompt |= one.prompt;
            outcome.up |= one.up;
        });
        outcome
    }

    #[test]
    fn remote_marks_touch_nothing_local() {
        let mut log = ssh_log();
        log.end_since = Some(Instant::now());
        let since = log.local.running_since;
        let (command, open, ours) = (log.command, log.command_open, log.ours);
        let outcome = feed(
            &mut log,
            b"\x1b]133;D;0;bt_remote=1.9.1\x07\x1b]133;A;bt_remote=1.9.2\x07\
              \x1b]133;B;bt_remote=1.9.2\x07\x1b]133;C;bt_remote=1.9.2\x07",
        );
        assert_eq!(
            outcome,
            ScanOutcome::default(),
            "no title, no command start, no prompt (⌘T's first input)"
        );
        assert_eq!(log.context.remote_host(), Some("prod"), "the session stays");
        assert_eq!(
            log.local.state.map(|state| state.phase),
            Some(ShellPhase::Running)
        );
        assert_eq!(log.local.running_since, since, "the local clock stays");
        assert_eq!(
            (log.command, log.command_open, log.ours),
            (command, open, ours)
        );
        assert!(log.end_since.is_some(), "a held line-finish is not ended");
        assert_eq!(log.local.blocks.last(), Some((1, Outcome::Pending)));
        assert_eq!(
            log.running_blocks(),
            RunningBlocks {
                local: Some(1),
                remote: Some((shell(1), 2)),
            }
        );
    }

    #[test]
    fn remote_blocks_get_stripes_and_counters_of_their_own() {
        let mut log = ssh_log();
        feed(
            &mut log,
            b"\x1b]133;A;bt_remote=1.9.1\x07\x1b]133;C;bt_remote=1.9.1\x07\
              \x1b]133;D;0;bt_remote=1.9.1\x07\x1b]133;A;bt_remote=1.9.2\x07\
              \x1b]133;C;bt_remote=1.9.2\x07\x1b]133;D;1;bt_remote=1.9.2\x07\
              \x1b]133;A;bt_remote=1.9.3\x07\x1b]133;C;bt_remote=1.9.3\x07",
        );
        let running = log.running_blocks();
        let key = |id| BlockKey::Remote {
            shell: shell(1),
            id,
        };
        assert_eq!(log.stripe(key(1), running), Some(Stripe::Success));
        assert_eq!(log.stripe(key(2), running), Some(Stripe::Error));
        assert_eq!(log.stripe(key(3), running), Some(Stripe::Running));
        assert!(log.duration(key(1), running).is_some());
        assert!(log.duration(key(3), running).is_some(), "the remote clock");
        // The local `rblock/1` and `block/1` are two blocks.
        assert_eq!(
            log.stripe(BlockKey::Local(1), running),
            Some(Stripe::Running),
            "the ssh command itself"
        );
    }

    #[test]
    fn our_local_d_ends_the_remote_clock_and_keeps_the_stripes() {
        let mut log = ssh_log();
        feed(
            &mut log,
            b"\x1b]133;A;bt_remote=1.9.1\x07\x1b]133;C;bt_remote=1.9.1\x07\
              \x1b]133;D;0;bt_remote=1.9.1\x07\x1b]133;A;bt_remote=1.9.2\x07\
              \x1b]133;C;bt_remote=1.9.2\x07",
        );
        // The connection dropped: no remote `D`, ours closes `P`.
        log.apply(our_end(255));
        assert_eq!(log.context.remote, None);
        let running = log.running_blocks();
        assert_eq!(running.remote, None, "no remote block runs any more");
        let key = |id| BlockKey::Remote {
            shell: shell(1),
            id,
        };
        assert_eq!(log.stripe(key(2), running), None, "unknown is not drawn");
        assert_eq!(log.duration(key(2), running), None, "no counter ticks");
        assert_eq!(
            log.stripe(key(1), running),
            Some(Stripe::Success),
            "the history keeps its stripes"
        );
        // The next local prompt does not erase them either.
        log.apply(Mark::PromptStart { id: Some(2) });
        assert_eq!(
            log.stripe(key(1), log.running_blocks()),
            Some(Stripe::Success)
        );
    }

    #[test]
    fn two_ssh_sessions_never_share_a_remote_block() {
        let mut log = ssh_log();
        feed(
            &mut log,
            b"\x1b]133;A;bt_remote=1.9.1\x07\x1b]133;C;bt_remote=1.9.1\x07\
              \x1b]133;D;0;bt_remote=1.9.1\x07",
        );
        log.apply(our_end(0));
        // The second ssh is local block 2; its server counts from 1 again.
        log.apply(Mark::PromptStart { id: Some(2) });
        log.apply(Mark::CommandStart);
        feed(
            &mut log,
            b"\x1b]133;A;bt_remote=2.9.1\x07\x1b]133;C;bt_remote=2.9.1\x07\
              \x1b]133;D;1;bt_remote=2.9.1\x07",
        );
        let running = log.running_blocks();
        assert_eq!(
            log.stripe(
                BlockKey::Remote {
                    shell: shell(2),
                    id: 1
                },
                running
            ),
            Some(Stripe::Error)
        );
        assert_eq!(
            log.stripe(
                BlockKey::Remote {
                    shell: shell(1),
                    id: 1
                },
                running
            ),
            None,
            "the old session's row is not painted with the new one's code"
        );
    }

    #[test]
    fn two_shells_under_one_ssh_command_are_two_trails() {
        // `ssh a; ssh b`: one local block, two servers counting from one. The
        // second shell's `rblock/1.9.1` must not reopen the first's `1.8.1`.
        let mut log = ssh_log();
        feed(
            &mut log,
            b"\x1b]133;A;bt_remote=1.8.1\x07\x1b]133;C;bt_remote=1.8.1\x07\
              \x1b]133;D;0;bt_remote=1.8.1\x07\x1b]133;A;bt_remote=1.9.1\x07\
              \x1b]133;C;bt_remote=1.9.1\x07\x1b]133;D;1;bt_remote=1.9.1\x07",
        );
        let running = log.running_blocks();
        let first = BlockKey::Remote {
            shell: RemoteShell { parent: 1, pid: 8 },
            id: 1,
        };
        assert_eq!(log.stripe(first, running), None, "not repainted");
        assert_eq!(
            log.stripe(
                BlockKey::Remote {
                    shell: shell(1),
                    id: 1
                },
                running
            ),
            Some(Stripe::Error)
        );
    }

    #[test]
    fn a_remote_trail_runs_only_under_its_open_parent() {
        // A remote mark whose parent is not the open local command (a stale or
        // foreign `P`) never runs: no accent, no clock.
        let mut log = ssh_log();
        feed(
            &mut log,
            b"\x1b]133;A;bt_remote=9.9.1\x07\x1b]133;C;bt_remote=9.9.1\x07",
        );
        let running = log.running_blocks();
        assert_eq!(running.remote, None);
        let key = BlockKey::Remote {
            shell: shell(9),
            id: 1,
        };
        assert_eq!(log.stripe(key, running), None);
        assert_eq!(log.duration(key, running), None);
    }

    #[test]
    fn the_remote_trail_follows_the_scrollback() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.set_scrollback(BLOCK_LOG_FLOOR * 2);
        assert_eq!(log.remote.blocks.capacity, BLOCK_LOG_FLOOR * 2);
    }

    // ─── the handover's state blob (055) ────────────────────────────────

    /// A log with every carried field away from its default.
    fn carried_log() -> ShellLog {
        let mut log = ShellLog::new(1000);
        log.local.blocks.start(7);
        log.local.blocks.finish(7, Some(0), 120);
        log.local.blocks.start(8);
        log.local.blocks.finish(8, None, 0);
        log.local.blocks.start(9);
        log.local.state = Some(ShellState {
            phase: ShellPhase::Running,
            last_exit: Some(-2),
        });
        log.local.running_since = Some(Instant::now() - Duration::from_secs(3));
        log.remote.blocks.start(1);
        log.remote.blocks.finish(1, Some(130), 4_000);
        log.remote.state = Some(ShellState {
            phase: ShellPhase::Finished,
            last_exit: Some(130),
        });
        log.remote_shell = Some(RemoteShell {
            parent: 9,
            pid: 4242,
        });
        log.context.cwd = "/Users/me/My Drive\\x".to_owned();
        log.context.branch = "feat/ş".to_owned();
        log.context.remote_cwd = "/srv/app".to_owned();
        log.context.remote_setup = Some(RemoteSetupFault::Decode);
        log.context.reconnect = Some(Reconnect {
            host: "prod".to_owned(),
            mark: HostMark::None,
            line: "ssh -p 2222 prod".to_owned(),
        });
        log.dock = DockState {
            status: DockStatus::Live,
            predisplay: String::new(),
            buffer: "echo 'a b'\n\tz".to_owned(),
            postdisplay: " # suggestion".to_owned(),
            prebuffer: "for x in 1\n".to_owned(),
            cursor: 4,
            highlights: vec![
                Highlight {
                    start: 0,
                    end: 4,
                    style: HighlightStyle {
                        fg: Some(HighlightColor::Indexed(2)),
                        bg: None,
                        bold: true,
                        underline: false,
                        standout: true,
                    },
                },
                Highlight {
                    start: 5,
                    end: 10,
                    style: HighlightStyle {
                        fg: None,
                        bg: Some(HighlightColor::Rgb(0x00ab_cdef)),
                        bold: false,
                        underline: true,
                        standout: false,
                    },
                },
            ],
            display_chars: 26,
            last_ink: Some('z'),
            insert_keymap: true,
            answers: 0,
            cluster: false,
        };
        log.dock_editable = true;
        log.command = 41;
        log.login = Some(41);
        log.remote_up = Some((41, "n0nce".to_owned()));
        log.typed = Some(40);
        log.ours = true;
        log.command_open = true;
        log
    }

    #[test]
    fn the_state_blob_round_trips_every_carried_field() {
        let log = carried_log();
        let carried = log.carried(Some(321), 0);
        let blob = carried.encode();
        assert_eq!(Carried::decode(&blob).as_ref(), Some(&carried));

        let mut fresh = ShellLog::new(1000);
        fresh.dock.cluster = true;
        fresh.host_rules = vec![HostRule {
            pattern: "prod".to_owned(),
            mark: Some(HostMark::Production),
            integration: None,
        }];
        fresh.restore(Carried::decode(&blob).unwrap(), 17);
        assert_eq!(fresh.local.state, log.local.state);
        assert_eq!(fresh.local.blocks.first, 7);
        assert_eq!(
            fresh.local.blocks.entries,
            [
                Outcome::Finished {
                    exit: Some(0),
                    elapsed_ms: 120
                },
                Outcome::Finished {
                    exit: None,
                    elapsed_ms: 0
                },
                Outcome::Pending
            ]
        );
        assert_eq!(fresh.running_blocks().local, Some(9));
        let ran = fresh.local.running_since.unwrap().elapsed();
        assert!(
            ran >= Duration::from_secs(3) && ran < Duration::from_secs(60),
            "{ran:?}"
        );
        assert_eq!(fresh.remote.blocks.entries, log.remote.blocks.entries);
        assert_eq!(fresh.remote_shell, log.remote_shell);
        assert_eq!(fresh.context.cwd, log.context.cwd);
        assert_eq!(fresh.context.branch, log.context.branch);
        // The mark comes from this session's rules, not the blob.
        assert_eq!(
            fresh.context.reconnect.as_ref().map(|offer| offer.mark),
            Some(HostMark::Production)
        );
        assert_eq!(fresh.dock.buffer, log.dock.buffer);
        assert_eq!(fresh.dock.highlights, log.dock.highlights);
        assert_eq!(fresh.dock.answers, 17);
        assert!(fresh.dock.cluster);
        assert!(fresh.dock_editable && fresh.ours && fresh.command_open);
        assert_eq!(
            (fresh.command, fresh.login, fresh.typed),
            (41, Some(41), Some(40))
        );
        assert_eq!(fresh.remote_up, log.remote_up);
        // The remote target is not carried: the probe finds it again.
        assert!(fresh.context.remote.is_none());
    }

    #[test]
    fn a_mirror_stale_at_the_handover_stays_stale() {
        let mut log = carried_log();
        log.dock.answers = 4;
        // A key went after the mirror (generation 5): stale.
        let stale = Carried::decode(&log.carried(None, 5).encode()).unwrap();
        let mut fresh = ShellLog::new(1000);
        fresh.restore(stale, 0);
        assert_ne!(fresh.dock.answers, 0);
        let answered = Carried::decode(&log.carried(None, 4).encode()).unwrap();
        fresh.restore(answered, 0);
        assert_eq!(fresh.dock.answers, 0);
    }

    #[test]
    fn a_fresh_logs_blob_round_trips_too() {
        let carried = ShellLog::new(10).carried(None, 0);
        assert_eq!(Carried::decode(&carried.encode()), Some(carried));
    }

    #[test]
    fn a_ledger_longer_than_the_new_ceiling_keeps_its_newest_entries() {
        let mut log = ShellLog::new(0);
        for id in 0..300 {
            log.local.blocks.start(id);
        }
        let mut carried = log.carried(None, 0);
        carried.local.first = 0;
        carried.local.entries = vec![Outcome::Pending; 300];
        let mut fresh = ShellLog::new(0);
        fresh.restore(carried, 0);
        assert_eq!(fresh.local.blocks.entries.len(), BLOCK_LOG_FLOOR);
        assert_eq!(fresh.local.blocks.first, 300 - BLOCK_LOG_FLOOR as u32);
    }

    #[test]
    fn a_corrupt_or_cut_blob_is_none() {
        let blob = String::from_utf8(carried_log().carried(Some(5), 0).encode()).unwrap();
        // Every cut short of the whole: a missing line, or half of one.
        for cut in 0..blob.len() - 1 {
            if blob.is_char_boundary(cut) {
                assert!(
                    Carried::decode(&blob.as_bytes()[..cut]).is_none(),
                    "cut at {cut}"
                );
            }
        }
        let swap = |from: &str, to: &str| Carried::decode(blob.replacen(from, to, 1).as_bytes());
        assert!(swap("bateri-state 1", "bateri-state 2").is_none());
        assert!(swap("bateri-state 1", "bateri-state 0").is_none());
        assert!(swap("bateri-state 1", "bateri-session 1").is_none());
        assert!(swap("ours 1", "ours 2").is_none());
        assert!(swap("ours 1", "ours 1\nours 1").is_none());
        assert!(swap("ours 1", "ours 1\nnew-key 1").is_none());
        assert!(swap("command 41", "command x").is_none());
        assert!(swap("cwd +", "cwd ").is_none());
        assert!(swap("remote-setup decode", "remote-setup gone").is_none());
        assert!(swap("end\n", "end\nhl 0 1 - - -\n").is_none());
        assert!(Carried::decode(b"\xff\xfe").is_none());
        assert!(Carried::decode(b"").is_none());
    }

    /// **Wire (a), the shell → the terminal** (055 Karar 8): after an update
    /// the carried shell keeps running the **previous** version's script, so
    /// this bateri must read what that script printed. The stream below is
    /// frozen as the scripts in `assets/shell/` print it today — the local
    /// zsh's prompt, mirror, branch, capability, command and directory and
    /// the remote scripts' marks, bootstrap proof and fault. **The rule:**
    /// when a format changes, this fixture stays (it is the version before)
    /// and a new one is added next to it; it goes two versions later.
    #[test]
    fn the_previous_scripts_stream_still_reads() {
        let stream: &[u8] = b"\x1b]133;A;bt_block=5\x07\
            \x1b]8;;bateri://block/5\x07  \x1b]133;B\x07\
            \x1b]7;file:///Users/me/a%20b\x07\
            \x1b]8133;b;ZmVhdHVyZS94\x07\
            \x1b]8133;w\x07\
            \x1b]8133;u;6;;bHMgLWxh;;MCAyIGZnPWdyZWVu;bWFpbg==;ZWNobyBhCg==\x07\
            \x1b]8133;e\x07\
            \x1b]8;;\x07\x1b]133;C\x07\
            \x1b]8133;i;up;0123456789abcdef\x07\
            \x1b]133;A;bt_remote=5.4242.1\x07\x1b]8;;bateri://rblock/5.4242.1\x07\
            \x1b]8;;\x07\x1b]133;C;bt_remote=5.4242.1\x07\
            \x1b]133;D;3;bt_remote=5.4242.1\x07\
            \x1b]8133;f;shell\x07\
            \x1b]133;D;0;bt_block=5\x07";
        let mut scanner = Scanner::new();
        let mut seen = Vec::new();
        scanner.feed(stream, |event| {
            seen.push(match event {
                ScanEvent::Mark(mark) => format!("mark {mark:?}"),
                ScanEvent::RemoteMark(remote) => format!(
                    "remote {}.{}.{} {:?}",
                    remote.shell.parent, remote.shell.pid, remote.id, remote.mark
                ),
                ScanEvent::Cwd { path, local } => format!("cwd {path} {local}"),
                ScanEvent::Dock(DockEvent::Update(state)) => {
                    assert_eq!(state.status, DockStatus::Live);
                    assert_eq!(
                        (
                            state.buffer.as_str(),
                            state.prebuffer.as_str(),
                            state.cursor
                        ),
                        ("ls -la", "echo a\n", 6)
                    );
                    assert!(state.insert_keymap, "`main` is an insert keymap");
                    let highlight = state.highlights.first().expect("a highlight");
                    assert_eq!((highlight.start, highlight.end), (0, 2));
                    assert!(highlight.style.fg.is_some());
                    "mirror".to_owned()
                }
                ScanEvent::Dock(DockEvent::End) => "end".to_owned(),
                ScanEvent::Dock(DockEvent::Branch(branch)) => format!("branch {branch}"),
                ScanEvent::Dock(DockEvent::Editable) => "editable".to_owned(),
                ScanEvent::Dock(DockEvent::Unavailable(fault)) => format!("fault {fault:?}"),
                ScanEvent::PasteOn => "paste".to_owned(),
                ScanEvent::RemoteSetup(fault) => format!("setup {}", fault.code()),
                ScanEvent::RemoteUp(nonce) => format!("up {nonce}"),
            });
        });
        assert_eq!(
            seen,
            [
                "mark PromptStart { id: Some(5) }",
                "mark PromptEnd",
                "cwd /Users/me/a b true",
                "branch feature/x",
                "editable",
                "mirror",
                "end",
                "mark CommandStart",
                "up 0123456789abcdef",
                "remote 5.4242.1 PromptStart { id: None }",
                "remote 5.4242.1 CommandStart",
                "remote 5.4242.1 CommandEnd { exit: Some(3), id: None }",
                "setup shell",
                "mark CommandEnd { exit: Some(0), id: Some(5) }",
            ]
        );
        // The anchors the prompt cells carry (read by the session's loops).
        assert_eq!(
            crate::session::block_key_for_tests("bateri://block/5"),
            Some((None, 5))
        );
        assert_eq!(
            crate::session::block_key_for_tests("bateri://rblock/5.4242.1"),
            Some((Some((5, 4242)), 1))
        );
    }

    #[test]
    fn the_version_1_fixture_still_reads() {
        // Written by version 1; the reader takes the current version and
        // the one before it, so this fixture stays until two bumps later.
        let fixture = "bateri-state 1\n\
                       local input 0 - 3 0/12,p\n\
                       remote - - - 0 -\n\
                       remote-shell -\n\
                       cwd +/tmp/a\\sb\n\
                       branch +main\n\
                       remote-cwd +\n\
                       remote-setup -\n\
                       reconnect -\n\
                       dock live 2 2 +s 1\n\
                       predisplay +\n\
                       buffer +ls\n\
                       postdisplay +\n\
                       prebuffer +\n\
                       dock-fresh 1\n\
                       editable 1\n\
                       command 3\n\
                       login -\n\
                       remote-up -\n\
                       typed -\n\
                       ours 1\n\
                       command-open 0\n\
                       cleared -\n\
                       hl 0 2 i2 - b\n\
                       end\n";
        let carried = Carried::decode(fixture.as_bytes()).unwrap();
        assert_eq!(
            carried.local.state,
            Some(ShellState {
                phase: ShellPhase::Input,
                last_exit: Some(0)
            })
        );
        assert_eq!(carried.local.first, 3);
        assert_eq!(carried.cwd, "/tmp/a b");
        assert_eq!(carried.dock.buffer, "ls");
        assert_eq!(carried.dock.last_ink, Some('s'));
        assert_eq!(carried.dock.highlights.len(), 1);
        assert_eq!(carried.encode(), fixture.as_bytes());
    }
}
