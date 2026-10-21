//! The requests of a view's web view that wait for a file of its bundle.
//!
//! A file of the bundle is read from the shelf off the main thread (the file service waits for
//! the conversation, which may be busy with a file of 8 MiB, and reads and checks one), so the
//! web view's task for it is **held** here until the answer comes back to the main queue. The
//! web view may stop a task in the meantime (the page was left, the view closed), and a task that
//! was stopped must not be answered — AppKit raises when it is — so a held task is answered
//! only if it is still held when its file is ready.
//!
//! Generic over the task, which is the web view's and cannot be made in a test: what the rules
//! are is tested with plain values.

/// One task that waits for its file.
struct Held<T> {
    serial: u64,
    /// Whose it is: the bridge of one view.
    owner: usize,
    task: T,
}

/// The tasks that wait for a file.
pub(super) struct Pending<T> {
    next: u64,
    held: Vec<Held<T>>,
}

impl<T> Pending<T> {
    pub(super) const fn new() -> Self {
        Self {
            next: 0,
            held: Vec::new(),
        }
    }

    /// Holds `task` for `owner`; the number it is called by when its file is ready.
    pub(super) fn hold(&mut self, owner: usize, task: T) -> u64 {
        self.next += 1;
        self.held.push(Held {
            serial: self.next,
            owner,
            task,
        });
        self.next
    }

    /// The task `serial` is ready to be answered: the task, if it is still held — it was not
    /// stopped and its view did not close — and held no more.
    pub(super) fn finish(&mut self, serial: u64) -> Option<T> {
        let at = self.held.iter().position(|held| held.serial == serial)?;
        Some(self.held.remove(at).task)
    }

    /// The web view stopped a task: it is not answered. Whether it was held.
    pub(super) fn stop(&mut self, is: impl Fn(&T) -> bool) -> bool {
        let before = self.held.len();
        self.held.retain(|held| !is(&held.task));
        self.held.len() != before
    }

    /// The view of `owner` closed: none of its tasks is answered.
    pub(super) fn forget(&mut self, owner: usize) {
        self.held.retain(|held| held.owner != owner);
    }

    /// How many are held.
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.held.len()
    }
}
