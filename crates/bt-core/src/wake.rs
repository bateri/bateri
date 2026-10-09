//! One-way signal from the core to the outside world.

/// The reader thread's way of notifying the outside world.
///
/// Calls arrive **on the reader thread** and may arrive WHILE alacritty's
/// `Term` lock is HELD (`pty_read` sends `Wakeup` after parsing the buffer,
/// while it still holds the lock). The implementor therefore does three things
/// it must not do: re-enter `Session`, block, take a lock — it only tells
/// another thread "something happened". `bt-gpu`'s `Waker` is the counterpart:
/// it posts a single job to the main queue. The one exception is a **leaf**
/// lock: a slot taken and released immediately, with no other lock taken under
/// it (the precedent is `Theme`'s leaf lock; in production `ShellWake`'s
/// detachable `Waker` slot).
///
/// **Ownership:** `Session` holds this object through an `Arc`. If the
/// implementor also holds an `Arc<Session>` the cycle closes: `Drop for
/// Session` never runs, the reader thread is never `join`ed, and one PTY and
/// one thread leak per tab. If `Session` must be looked at, use a `Weak` — and
/// do not open a **synchronous** path from inside `wake()` to `shutdown()`.
/// The prohibition stands, but its cost changed: because `shutdown()` now moves
/// the `join` to a separate thread and waits for a bounded time, even if the
/// last strong reference drops on the reader thread there is no `EDEADLK`
/// panic — instead that thread stalls for half a second, one "left behind"
/// line is printed, and the shutdown never finishes.
///
/// **The thread `Drop` runs on is part of the contract.** When the limit
/// expires, the `(EventLoop, State)` pair stays on the `"PTY teardown"` thread,
/// and that pair carries an `Arc` copy of this object through `Adapter`: if the
/// last copy drops there, **`Wake::drop` runs on that thread**. So the
/// implementor's `Drop` must not block either — in particular it must not post
/// synchronous work to the main queue. The production implementor is
/// `bt-shell`'s `ShellWake`, and the `bt-gpu` `Waker` it carries has exactly
/// such a field (`MainThreadBound<Retained<CAMetalDisplayLink>>`); that is why
/// the `Waker` is **detached** on the main thread when the window closes and
/// `ShellWake` does not carry it, whichever thread it drops on.
///
/// None of the calls has a default body: when a new call is added, the
/// implementor cannot forget it — the compiler says so.
pub trait Wake: Send + Sync + 'static {
    /// The grid changed; a frame may be needed.
    fn wake(&self);

    /// The shell child ended. `code` is filled only on a normal exit; it is
    /// `None` for a child killed by a signal.
    fn child_exit(&self, code: Option<i32>);

    /// The application in the terminal asked, via OSC 52, to write `text` to
    /// the clipboard (the copy of vim over ssh). It arrives only in
    /// `Osc52::Copy` mode; `text` is not empty. The sequence's target (`c`,
    /// `p`, `s`) is not carried: on a platform with a single clipboard there is
    /// no distinction (the arm of `Adapter`).
    ///
    /// The three prohibitions above apply here too, and this is where they are
    /// tested hardest: writing to the clipboard can be too slow to do under the
    /// `Term` lock (the text is unbounded), so the implementor puts the text
    /// into a lock-free slot and leaves the writing to another thread. An
    /// application that prints OSC 52 nonstop can make this call hundreds of
    /// times per second; not piling unbounded work onto its queue is the
    /// implementor's job.
    ///
    /// A write that arrives during shutdown failing to reach the clipboard is
    /// harmless.
    fn copy_to_clipboard(&self, text: String);

    /// The session's title ([`crate::Session::title`]) may have changed: the
    /// application's OSC 0/2 title changed, or the shell's OSC 7 directory
    /// **changed** (a `precmd` printing the same directory produces no
    /// notification).
    ///
    /// The two sources have two thread situations: OSC 0/2 arrives **under**
    /// the `Term` lock (on the reader thread or on the thread calling
    /// `Session::set_terminal_options` — `Term::set_options` re-sends the title
    /// event, and there is no notification if nothing changed), OSC 7 arrives
    /// on the reader thread without the lock. The three prohibitions above
    /// apply to both.
    ///
    /// **Carries no payload:** the receiver reads the title from
    /// `Session::title` itself, so two changes chasing each other cannot act on
    /// a stale value. The implementor posts **at most one** job to its queue —
    /// a shell printing its title on every command, or a `printf` in a loop,
    /// can make the call frequent, and the title to be seen is the latest one
    /// anyway.
    fn title_changed(&self);

    /// While search in scrollback is open, **the ledger changed**: PTY
    /// output arrived or the window was re-wrapped. The receiver drives the
    /// count index ([`crate::Session::search_step`]); the index restarts its
    /// next pass from the beginning.
    ///
    /// **Edge-triggered and payload-free** (the precedent is
    /// [`Wake::title_changed`]): a pending notification does not produce a
    /// second one until the index consumes it, so even while `yes` streams the
    /// number of calls is bounded by the number of passes. It arrives on the
    /// reader thread **while the `Term` lock is held** or on the main thread
    /// (`resize`); the three prohibitions above apply. It never arrives while
    /// search is closed.
    fn search_changed(&self);

    /// The shell's phase **moved** to `Running` (OSC 133 `C`): a
    /// command started. The receiver probes the foreground program and reports
    /// the remote session via [`crate::Session::set_remote`].
    ///
    /// **Edge-triggered and payload-free** (the precedent is
    /// [`Wake::title_changed`]): a second `C` in the same command (iTerm2
    /// integration) is not a transition and produces no notification. No
    /// payload, because the receiver reads the command's generation itself
    /// ([`crate::Session::running_command`]) and hands the probe's answer back
    /// with it — if a `D` intervenes, the stale answer is dropped. It arrives
    /// on the reader thread; after the ledger's leaf lock has been released,
    /// but the contract assumes the `Term` lock may be held and the three
    /// prohibitions above apply. The implementor posts **at most one** job to
    /// its queue.
    fn command_started(&self);

    /// The shell's phase moved at the other two edges: our identified `A`
    /// (a prompt) or a `D` (a command ended) — the edges where the state
    /// [`crate::Session::state_blob`] carries (the block ledger, the
    /// command's clock) changes. With [`Wake::command_started`] the receiver
    /// sees all three, and a copy of the state it keeps elsewhere is never a
    /// command behind.
    ///
    /// **Edge-triggered and payload-free** (the precedent is
    /// [`Wake::command_started`]): the receiver reads the state itself. It
    /// arrives on the reader thread after the ledger's leaf lock has been
    /// released; the contract assumes the `Term` lock may be held and the
    /// three prohibitions above apply. The implementor posts **at most one**
    /// job to its queue.
    fn phase_edge(&self);

    /// Our **remote** shell's command started (its `C` moved the remote trail
    /// to `Running`) or ended (its `D`) — the edges of a command run on the
    /// far end of ssh, which the local phase does not see: there the local
    /// command is `ssh` itself, running throughout. The receiver reads what
    /// it wants with [`crate::Session::activity`].
    ///
    /// **Separate from [`Wake::phase_edge`]**, whose receiver also sends the
    /// state to a bound holder: every remote command would add that traffic.
    ///
    /// **Edge-triggered and payload-free** (the precedent is
    /// [`Wake::command_started`]): a second remote `C` in the same command is
    /// not a transition. It arrives on the reader thread after the ledger's
    /// leaf lock has been released; the contract assumes the `Term` lock may
    /// be held and the three prohibitions above apply. The implementor posts
    /// **at most one** job to its queue.
    fn remote_command_edge(&self);

    /// The dock's mirror or its context changed (OSC 8133: the line, its
    /// end, a mirror that could not be read, the branch) — the part of
    /// [`crate::Session::state_blob`] that moves with every keystroke.
    ///
    /// **Payload-free** (the precedent is [`Wake::title_changed`]): once per
    /// event, i.e. as often as the shell redraws its line; the receiver that
    /// copies the state elsewhere coalesces — one copy per interval, not per
    /// key. It arrives on the reader thread after the ledger's leaf lock has
    /// been released; the contract assumes the `Term` lock may be held and
    /// the three prohibitions above apply.
    fn mirror_changed(&self);

    /// The remote bootstrap said it runs (`8133;i;up;{nonce}`)
    /// while a command ran: the receiver reads it with
    /// [`crate::Session::remote_up`] and, if the nonce is the wrapped `ssh`'s,
    /// learns the server as one with a shell.
    ///
    /// **Edge-triggered and payload-free** (the precedent is
    /// [`Wake::command_started`]): once per mark, which the bootstrap prints
    /// once per connection. It arrives on the reader thread after the ledger's
    /// leaf lock has been released; the contract assumes the `Term` lock may be
    /// held and the three prohibitions above apply. The implementor posts **at
    /// most one** job to its queue.
    fn remote_up(&self);

    /// The user typed into a remote session after its login was seen: the
    /// receiver reads the generation with
    /// [`crate::Session::remote_typed`] and, if it is a wrapped `ssh`'s, marks
    /// the attempt used — its end then reruns nothing.
    ///
    /// **Edge-triggered and payload-free** (the precedent is
    /// [`Wake::remote_up`]): once per command generation. It arrives on the
    /// thread that sent the input (the main thread), inside the input call
    /// and after the ledger's leaf lock has been released; the contract
    /// assumes the `Term` lock may be held and the three prohibitions above
    /// apply. The implementor posts **at most one** job to its queue.
    fn remote_typed(&self);

    /// The link hover ([`crate::Session::set_link_hover`]) went **stale** and
    /// was dropped: its stamp no longer held — output came, the window
    /// scrolled, the screen was cleared, the link's cell changed — so the frame
    /// did not draw it. The receiver re-runs the hit test if ⌘ is still held, so
    /// the highlight lands on the right text again.
    ///
    /// **Edge-triggered and payload-free** (the precedent is
    /// [`Wake::title_changed`]): once per dropped hover; a hover the receiver set
    /// in between is not dropped. It arrives on the **frame path's** thread,
    /// after the `Term` lock is released and the hover's leaf lock too; the three
    /// prohibitions above apply. The implementor posts **at most one** job to its
    /// queue.
    fn link_hover_lost(&self);

    /// The scroll bar's block marks are wanted
    /// ([`crate::Session::set_block_marks`]) and the history moved since the
    /// block index last looked — output, a clear, a resize. The receiver
    /// drives the index ([`crate::Session::block_step`]).
    ///
    /// **Edge-triggered and payload-free** (the precedent is
    /// [`Wake::search_changed`]): a pending notification does not produce a
    /// second one until a step consumes it, so while output streams it comes
    /// at most once per drawn frame. It arrives on the **frame path's**
    /// thread, after the `Term` lock and the index's leaf lock are released;
    /// the three prohibitions above apply. It never arrives while the marks
    /// are not wanted. The implementor posts **at most one** job to its
    /// queue.
    fn blocks_changed(&self);

    /// The count of rows output pushed below a window scrolled up the
    /// history changed ([`crate::Session::unseen_rows`]): it grew, or the
    /// window went back to the bottom and it dropped to zero. The receiver
    /// shows, relabels or hides its "Jump to latest".
    ///
    /// **Edge-triggered and payload-free** (the precedent is
    /// [`Wake::title_changed`]): once per change of the number, which the
    /// frame takes at most once per drawn frame; the receiver reads the
    /// count itself, so two changes chasing each other cannot act on a
    /// stale value. It arrives on the **frame path's** thread, after the
    /// `Term` lock and the count's leaf lock are released; the three
    /// prohibitions above apply. The implementor posts **at most one** job
    /// to its queue.
    fn unseen_changed(&self);

    /// A program's status record changed or went (`OSC 7501`, or the prompt
    /// that ended what it reported): the receiver reads
    /// [`crate::Session::activity`] again — the tab's ring, its "waiting" mark
    /// and its "finished" tick.
    ///
    /// **Edge-triggered and payload-free** (the precedent is
    /// [`Wake::remote_command_edge`]): once per report that changed something,
    /// which a program makes when its state moves, not per frame. It arrives on
    /// the reader thread after the ledger's leaf lock has been released; the
    /// contract assumes the `Term` lock may be held and the three prohibitions
    /// above apply. The implementor posts **at most one** job to its queue.
    fn program_status_changed(&self);
}
