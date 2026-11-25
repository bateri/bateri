//! The program of a pane that is one of bateri's own tools (`bateri tool-hold`): it says nothing,
//! shows nothing and ends with its terminal.
//!
//! Every pane runs a process and a tool has nothing to run: its face is a page that bateri
//! draws over the pane. This is what stands in the process's place. It is not a shell, so a key
//! that reaches the pane's terminal has no command to run; the terminal is put to no echo, so
//! it is not shown either; and the process ends when its terminal does (the pane closing hangs
//! it up, or its input ends), so a tool's pane has no process that outlives it.

/// `bateri tool-hold`: the subcommand's word, the first argument.
pub const SUBCOMMAND: &str = "tool-hold";

/// The subcommand's body: the exit code.
///
/// Reads standard input until it ends and throws what it reads away. A terminal that is gone
/// ends the read (`EIO`, or `SIGHUP`, which ends the process as it ends any program of a closed
/// terminal).
pub fn hold_main() -> i32 {
    hold(0)
}

/// [`hold_main`] on the descriptor `fd`.
fn hold(fd: i32) -> i32 {
    quiet(fd);
    let mut sink = [0_u8; 256];
    loop {
        // SAFETY: `sink` is a live buffer of the length given, which `read` fills at most.
        let read = unsafe { libc::read(fd, sink.as_mut_ptr().cast(), sink.len()) };
        if read > 0 {
            continue;
        }
        if read < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return 0;
    }
}

/// The terminal `fd` is, put to raw input with no echo: what is typed to it is neither shown nor
/// held for a line to be made of it. Not a terminal (a test that runs the body), or one that
/// refuses: left as it is.
fn quiet(fd: i32) {
    // SAFETY: `termios` is plain data that `tcgetattr` fills before it is read, and each call is
    // given the descriptor and a live pointer to it.
    unsafe {
        let mut mode: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut mode) != 0 {
            return;
        }
        libc::cfmakeraw(&mut mode);
        libc::tcsetattr(fd, libc::TCSANOW, &mode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_is_not_one_of_the_other_subcommands() {
        assert_eq!(SUBCOMMAND, "tool-hold");
        assert_ne!(SUBCOMMAND, crate::hold::SUBCOMMAND);
        assert_ne!(SUBCOMMAND, crate::focus::SUBCOMMAND);
    }

    /// What reaches the process is thrown away, and it ends when its input does.
    #[test]
    fn it_reads_what_comes_and_ends_when_its_input_ends() {
        let mut fds = [0; 2];
        // SAFETY: `fds` has room for the two descriptors `pipe` writes.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let typed = b"keys that no command is waiting for\n";
        // SAFETY: `typed` is a live buffer of the length given; the descriptors are the pipe's.
        unsafe {
            assert_eq!(
                libc::write(fds[1], typed.as_ptr().cast(), typed.len()),
                typed.len() as isize
            );
            libc::close(fds[1]);
        }
        assert_eq!(hold(fds[0]), 0);
        // SAFETY: the read end is the pipe's and is closed once.
        unsafe { libc::close(fds[0]) };
    }

    /// A descriptor that is no terminal is left alone and nothing is raised.
    #[test]
    fn a_descriptor_that_is_no_terminal_is_left_as_it_is() {
        let mut fds = [0; 2];
        // SAFETY: `fds` has room for the two descriptors `pipe` writes.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        quiet(fds[0]);
        // SAFETY: both descriptors are the pipe's and are closed once.
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
    }
}
