//! Clipboard bridge: carries the text `bt-core` knows to the AppKit pasteboard.
//!
//! Only `bt-shell-macos` (AppKit) touches the pasteboard; `bt-core` sees bytes, not
//! the pasteboard (`CLAUDE.md` → layer layout). The copied text comes from
//! phase-1's single text path (`Session::selection_text`); a second text path
//! would mean a second wrapping bug.
//!
//! Both directions go through `NSPasteboard`; the board comes in as a **parameter**
//! on every call. In production both are the general pasteboard (`copy` is the
//! user's Cmd-C, `read` their Cmd-V); in tests each direction gets its own unique
//! board: the general pasteboard cannot be touched in a headless environment and
//! two tests must not see each other's content. Making the board a parameter also
//! makes the liveness anchor testable (`board()`).
//!
//! The names `copy`/`read` are neither AppKit's nor `NSPasteboard`'s, they are this
//! bridge's own vocabulary: `copy` is the write direction, `read` the read
//! direction.
//!
//! The third path is remote copy (OSC 52): the text is born on the reader thread
//! and reaches the main queue through the [`PendingCopy`] slot; it is still `copy`
//! that writes to the pasteboard.

use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;

/// Writes the selected text to the pasteboard. If there is no text to write the
/// pasteboard is left **untouched** and `false` is returned.
///
/// An empty write clears the general pasteboard and would erase what the user
/// copied from another app. The gate therefore rejects both cases:
///
/// - `None` → no selection.
/// - `Some("")` → a selection **exists** but yields empty text: dragging over a
///   line made of blanks. On that line alacritty's `line_length()` is zero, so
///   `selection_to_string()` returns `Some("")`. Had this case passed the gate,
///   selecting an empty line under the prompt and hitting Cmd-C would empty the
///   user's pasteboard — the very loss the `None` gate prevents.
///   (A click without a drag does not land here: a selection with equal ends is
///   empty and gives `None`.)
///
/// **Whitespace only** (`Some("   ")`) is not rejected: selecting and copying the
/// spaces inside a line is legitimate and that text is not empty.
pub(crate) fn copy(board: &NSPasteboard, text: Option<String>) -> bool {
    let Some(text) = text.filter(|t| !t.is_empty()) else {
        return false;
    };
    // `clearContents` + `setString`: `setString` alone also writes, but then the old
    // types (RTF, TIFF) stay on the pasteboard and the pasting side may pick them up
    // instead of the text.
    board.clearContents();
    let string = NSString::from_str(&text);
    // SAFETY: the `unsafe` block is for the **static** access to
    // `NSPasteboardTypeString`; `setString:forType:` itself is not `unsafe` (`objc2`
    // wraps it safely). The static is a real `NSPasteboardType` record and does not
    // resolve to `None`.
    //
    // The context is not `MainThreadMarker`: `NSPasteboard` is `AnyThread` in `objc2`,
    // so the type does not force the main thread. But `NSPasteboard` is **not**
    // resilient to concurrent use — even separate unique boards share a process-wide
    // type cache (`+[NSPasteboard(NSTypeConversion) …]`) and two threads touching it
    // at once produce a SIGSEGV in `_updateTypeCacheIfNeeded` or one board reading
    // another's text. In production all callers are on the main thread, i.e.
    // sequential; the tests set up the same order with
    // [`tests::pasteboard_lock`].
    unsafe { board.setString_forType(&string, NSPasteboardTypeString) }
}

/// Reads the text on the pasteboard. If there is none (`None`) pasting is silent.
pub(crate) fn read(board: &NSPasteboard) -> Option<String> {
    // SAFETY: the same static access as above.
    unsafe { board.stringForType(NSPasteboardTypeString) }.map(|s| s.to_string())
}

/// The text OSC 52 sends to the pasteboard: the reader thread puts it, the main
/// queue takes it.
///
/// **One slot, last write wins.** If an app that prints OSC 52 nonstop (a `printf`
/// in a loop) caused a job on the main queue for every sequence, the queue would
/// grow unbounded and the pasteboard would be written a hundred times in the same
/// second; the only one the user will see is the last anyway. The call that fills
/// an empty slot asks for **one** job ([`PendingCopy::put`] `true`), the job empties
/// the slot and writes ([`PendingCopy::take`]). Invariant: if the slot is full
/// there is a job in the queue that will take it and has not yet; the empty→full
/// transition always asks for a job, the job's own swap empties the slot. So the
/// queue holds at most one waiting and one running job and the last text is never
/// lost.
///
/// **Lock-free**, because `put` is called on the reader thread while the `Term` lock
/// is held and whoever implements `Wake` does not take a lock (`bt-core`'s
/// `wake.rs`; `discussion.md` → Karar 5). `AtomicPtr` + `Box`: std has no other
/// type that atomically swaps an owned value.
///
/// Separate from AppKit: the slot logic is tested without a pasteboard, the side
/// that receives the board chooses it.
#[derive(Default)]
pub(crate) struct PendingCopy(AtomicPtr<String>);

impl PendingCopy {
    /// Puts the text in the slot; if the previous text has not been taken yet it is
    /// dropped.
    ///
    /// `true` → the slot was empty, the caller must post **one** job to the main queue.
    /// `false` → the job that will take the slot is already queued, no new one is
    /// needed. Does not block; releasing the dropped text is not a lock.
    pub(crate) fn put(&self, text: String) -> bool {
        let new = Box::into_raw(Box::new(text));
        let old = self.0.swap(new, Ordering::AcqRel);
        if old.is_null() {
            return true;
        }
        // SAFETY: every non-null pointer in the slot comes from the `Box::into_raw`
        // above and the swap removed it from the slot **atomically**: no other party
        // (`take`, `Drop`, a second `put`) can see the same pointer, so ownership is
        // single and reclaimed exactly once.
        drop(unsafe { Box::from_raw(old) });
        false
    }

    /// The main queue's job: takes the text in the slot and empties the slot; `None` if
    /// empty (another job may have taken it in a race). The pane's owner writes the
    /// text to the pasteboard (`pane::PaneHost::copy_to_clipboard`; the default arm to
    /// the general pasteboard via [`copy`]).
    pub(crate) fn take(&self) -> Option<String> {
        let old = self.0.swap(ptr::null_mut(), Ordering::AcqRel);
        // SAFETY: the reasoning in `put` — the pointer comes from `Box::into_raw` and the
        // swap removed it from the slot alone.
        (!old.is_null()).then(|| *unsafe { Box::from_raw(old) })
    }
}

impl Drop for PendingCopy {
    /// Releases an untaken text — if the app closes before the job runs.
    ///
    /// Per `bt-core`'s `Wake` contract this `Drop` may run on the `"PTY teardown"`
    /// thread; it is nothing more than freeing memory, it does not block.
    fn drop(&mut self) {
        let _ = self.take();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, PoisonError};

    /// The **first** line of every test that touches the pasteboard: only one thread
    /// enters `NSPasteboard` at a time (reasoning inside [`copy`]).
    ///
    /// What is needed is not the main thread but **ordering**: `--test-threads=1` still
    /// runs each test on a worker thread and the pasteboard tests passed 300/300 there,
    /// while in a parallel run 5/300 failed (2026-09-27). The lock only orders the
    /// pasteboard tests, the remaining tests keep running in parallel. Poisoning is
    /// swallowed: one test's assertion must not fail the other tests.
    pub(crate) fn pasteboard_lock() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A live pasteboard; `None` in a headless environment (CI, `cargo test` over ssh).
    ///
    /// A unique board is **born** in a headless setting too, but writes do not stick.
    /// The anchor therefore depends not on existence but on **being able to write and
    /// read back**: a write that sticks means a real pasteboard server. Building this
    /// through `copy`/`read` also takes the liveness measure from the bridge's own
    /// body — better than building a separate write path and keeping the needle in two
    /// places.
    fn board() -> Option<objc2::rc::Retained<NSPasteboard>> {
        // `pasteboardWithUniqueName` gives a fresh board on every call — tests do not see
        // each other's content.
        let board = NSPasteboard::pasteboardWithUniqueName();
        const SENTINEL: &str = "bateri pano çapası";
        let alive =
            copy(&board, Some(SENTINEL.to_owned())) && read(&board).as_deref() == Some(SENTINEL);
        alive.then_some(board)
    }

    /// Shared reasoning for the headless branch. Both tests take the same path: if
    /// there is no pasteboard they are **skipped silently**. Asserting against an empty
    /// pasteboard would be a path where the gate gives a false green — a fresh board is
    /// empty anyway, `read` returns `None` even if the bridge does not work.
    const HEADLESS: &str = "no pasteboard in a headless environment, test skipped";

    #[test]
    fn copy_writes_selection_text_to_clipboard() {
        let _pasteboard = pasteboard_lock();
        let Some(board) = board() else {
            eprintln!("{HEADLESS}");
            return;
        };
        assert!(copy(&board, Some("hello".to_owned())));
        assert_eq!(read(&board).as_deref(), Some("hello"));
    }

    #[test]
    fn copy_without_selection_leaves_board_untouched() {
        let _pasteboard = pasteboard_lock();
        let Some(board) = board() else {
            eprintln!("{HEADLESS}");
            return;
        };
        assert!(copy(&board, Some("önce".to_owned())));
        // A copy without a selection returns `false` and does not erase what is on the pasteboard.
        assert!(!copy(&board, None));
        assert_eq!(read(&board).as_deref(), Some("önce"));
    }

    #[test]
    fn copy_of_empty_text_leaves_board_untouched() {
        let _pasteboard = pasteboard_lock();
        // `Some("")` = a selection exists but the text is empty: dragging over an empty
        // line. The gate rejects this too, otherwise Cmd-C would empty the user's
        // pasteboard. The part that needs no pasteboard is measured in every environment
        // (`Some("")` never writes), the rest needs a live pasteboard.
        let fresh = NSPasteboard::pasteboardWithUniqueName();
        assert!(!copy(&fresh, Some(String::new())));

        let Some(board) = board() else {
            eprintln!("{HEADLESS}");
            return;
        };
        assert!(copy(&board, Some("önce".to_owned())));
        assert!(!copy(&board, Some(String::new())));
        assert_eq!(read(&board).as_deref(), Some("önce"));
    }

    #[test]
    fn pending_copy_asks_for_one_job_and_keeps_the_last_text() {
        // An app printing OSC 52 nonstop: a hundred texts, one job, the last text.
        let slot = PendingCopy::default();
        let jobs = (0..100).filter(|i| slot.put(format!("metin {i}"))).count();
        assert_eq!(jobs, 1, "consecutive texts must ask for a single job");
        assert_eq!(slot.take().as_deref(), Some("metin 99"));
        assert_eq!(slot.take(), None);
        // A text after the job has emptied the slot asks for a job again: otherwise the
        // second copy would never reach the pasteboard.
        assert!(slot.put("sonra".to_owned()));
        assert_eq!(slot.take().as_deref(), Some("sonra"));
        // A slot dropped with an untaken text releases it (`Drop` closes the leak; here we
        // only see that it drops without panicking).
        assert!(slot.put("alınmadı".to_owned()));
        drop(slot);
    }

    #[test]
    fn pending_copy_delivers_to_the_given_board() {
        let _pasteboard = pasteboard_lock();
        // The board is passed from outside, not the general pasteboard: the test does not
        // touch the user's pasteboard.
        let slot = PendingCopy::default();
        let fresh = NSPasteboard::pasteboardWithUniqueName();
        // An empty slot does not write to the pasteboard — the half that needs no
        // pasteboard. The job's two steps as in production: take from the slot (`take`),
        // write to the pasteboard (`copy`, the owner's default arm).
        let deliver = |board: &NSPasteboard| copy(board, slot.take());
        assert!(!deliver(&fresh));

        let Some(board) = board() else {
            eprintln!("{HEADLESS}");
            return;
        };
        assert!(slot.put("ilk".to_owned()));
        assert!(!slot.put("son".to_owned()));
        assert!(deliver(&board));
        assert_eq!(read(&board).as_deref(), Some("son"));
        // The slot emptied: the second job leaves the pasteboard untouched.
        assert!(!deliver(&board));
        assert_eq!(read(&board).as_deref(), Some("son"));
    }

    #[test]
    #[ignore = "runs with make test-race"]
    fn race_pending_copy_put_and_take() {
        // The production shape: a single reader thread puts, the "main queue" thread takes
        // once for every job `put` asks for. Two invariants:
        // - **At most one waiting job in the queue.** `waiting` counts the span between a
        //   job's dispatch and the main thread taking it; it must be zero when `put` asks
        //   for a job. A `put` that asked for a job on every call would grow the queue
        //   unbounded and this would fail here.
        // - **The last text is not lost:** a full slot always has a waiting job; if this
        //   breaks the last text is left in a slot without a job.
        use std::sync::atomic::AtomicUsize;
        use std::sync::{Arc, mpsc};

        const TEXTS: usize = 20_000;
        let slot = Arc::new(PendingCopy::default());
        let waiting = Arc::new(AtomicUsize::new(0));
        let (jobs, queue) = mpsc::channel::<()>();
        let main = {
            let slot = Arc::clone(&slot);
            let waiting = Arc::clone(&waiting);
            std::thread::spawn(move || {
                let mut last = None;
                for () in queue {
                    // The order as in production: the job starts first, then it takes the slot.
                    waiting.fetch_sub(1, Ordering::SeqCst);
                    if let Some(text) = slot.take() {
                        last = Some(text);
                    }
                }
                last
            })
        };
        let mut sent = 0usize;
        for i in 0..TEXTS {
            if slot.put(i.to_string()) {
                let before = waiting.fetch_add(1, Ordering::SeqCst);
                assert_eq!(before, 0, "text {i} asked for a second waiting job");
                jobs.send(()).expect("main thread is alive");
                sent += 1;
            }
        }
        drop(jobs);
        let last = main.join().expect("ana thread paniklemedi");
        assert_eq!(last.as_deref(), Some((TEXTS - 1).to_string().as_str()));
        assert_eq!(
            slot.take(),
            None,
            "the last text was left in a slot without a job"
        );
        assert!(sent > 0);
    }
}
