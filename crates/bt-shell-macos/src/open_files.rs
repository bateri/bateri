//! How many files bateri may have open at once.
//!
//! A process started from Finder or the Dock gets launchd's default soft limit, 256 descriptors.
//! Every pane holds several (its terminal's stream and exit, the session's event queue, the
//! terminal device of an ordinary shell), so a window of some forty tabs reaches the limit: a tab
//! asked for then does not open, and at the next launch the tabs past the limit cannot be built
//! again ("Too many open files") and are left out of the windows. The soft limit is raised to the
//! hard one at start, up to [`TARGET`] — what macOS lets one process have without a change to the
//! system — and the shells inherit it, as they would from a terminal opened by hand after
//! `ulimit -n`.

/// The soft limit asked for: `OPEN_MAX`, past which `setrlimit` refuses on macOS.
const TARGET: libc::rlim_t = 10_240;

/// Raises this process's soft limit on open files towards [`TARGET`], never above the hard
/// limit and never below what it is. Silent when it cannot.
pub(crate) fn raise() {
    // SAFETY: `rlimit` is plain data; all zeroes is a valid value of it.
    let mut limit: libc::rlimit = unsafe { std::mem::zeroed() };
    // SAFETY: `getrlimit` writes only into `limit`, which this function owns.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return;
    }
    let wanted = TARGET.min(limit.rlim_max);
    if limit.rlim_cur >= wanted {
        return;
    }
    limit.rlim_cur = wanted;
    // SAFETY: `setrlimit` reads only `limit`.
    let _ = unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn soft() -> (libc::rlim_t, libc::rlim_t) {
        // SAFETY: as in `raise`.
        let mut limit: libc::rlimit = unsafe { std::mem::zeroed() };
        // SAFETY: as in `raise`.
        assert_eq!(
            unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
            0
        );
        (limit.rlim_cur, limit.rlim_max)
    }

    #[test]
    fn the_soft_limit_on_open_files_goes_up_to_the_target_and_not_past_the_hard_one() {
        raise();
        let (cur, max) = soft();
        assert!(cur >= TARGET.min(max), "{cur} of {max}");
        assert!(cur <= max);
        // Asked again, nothing changes.
        raise();
        assert_eq!(soft().0, cur);
    }
}
