//! The folder bateri keeps on a server for what is pasted or dropped **for the program in
//! front**: an image ⌘V pastes (a terminal's paste is text, and a program on the server reads the
//! server's clipboard, which holds nothing of this Mac's), and the files a person drops on, or
//! pastes into, a coding agent there. What goes into it is uploaded by the transfer queue
//! ([`crate::upload`]) and its path is pasted once it has arrived, as a drop pastes a path on this
//! Mac. A coding agent that takes a path (Claude Code's `[Image #1]`) takes it as it takes a file
//! dropped on it; a shell has the path on its line and the file is there.
//!
//! The folder is bateri's own ([`ROOT`], in the server account's home, never the project's
//! directory). Each paste or drop gets a folder of its own in it, named by the server's clock,
//! so what is sent keeps its name and never meets another of the same name. The run that makes
//! it sweeps what earlier ones left there more than [`KEEP_DAYS`] ago; nothing outside the
//! folder is touched.
//!
//! A shell in front keeps a drop's way: the files go to the shell's directory, after the sheet
//! that says what goes where ([`for_program`]).

use crate::program::identity::Kind;

/// The folder, relative to the server account's home directory.
pub const ROOT: &str = ".cache/bateri/paste";

/// How old a paste may grow before a later one removes it — a **design constant**: long enough
/// for a conversation with an agent to come back to it, short enough that a folder nobody looks
/// into does not grow without end.
pub const KEEP_DAYS: u32 = 7;

/// The exit code of the script when the folder cannot be made or written.
pub(crate) const EXIT_UNWRITABLE: i32 = 9;

/// How many folders one second may make before the script gives up: a guard on the loop, not a
/// limit anyone meets.
const SAME_SECOND: u32 = 99;

/// Whether what is dropped on, or pasted into, a pane a server keeps goes to the paste folder
/// with its path pasted (`true`), or to the shell's directory after the sheet (`false`): the
/// program in front is a coding agent, which reads a path it is given and has no directory of
/// the person's to put a file in.
#[must_use]
pub fn for_program(kind: Kind) -> bool {
    matches!(kind, Kind::Agent(_))
}

/// The start of the upload's first question ([`crate::upload::probe_paste`]): the folder is
/// made (closed to other users: a screenshot may hold anything), what is older than
/// [`KEEP_DAYS`] is removed from it, a new folder named by the server's local time is made in
/// it — `-2` and on for a second one in the same second; `mkdir` both tests and takes the name,
/// so two pastes at once never share one — and the shell goes into it.
pub(crate) fn fresh_folder_script() -> String {
    let older = KEEP_DAYS.saturating_sub(1);
    format!(
        "umask 077; d=\"$HOME/{ROOT}\"; mkdir -p \"$d\" || exit {EXIT_UNWRITABLE}; \
         find \"$d/.\" ! -name . -prune -mtime +{older} -exec rm -rf {{}} \\; 2>/dev/null; \
         n=$(date +%Y-%m-%d-%H%M%S); f=\"$d/$n\"; i=1; \
         while ! mkdir \"$f\" 2>/dev/null; do \
         i=$((i+1)); [ \"$i\" -gt {SAME_SECOND} ] && exit {EXIT_UNWRITABLE}; f=\"$d/$n-$i\"; \
         done; cd \"$f\" || exit {EXIT_UNWRITABLE}; "
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program::identity::Agent;
    use crate::settings::TempRoot;
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// Runs the script in a home of its own and says the folder it went into.
    fn run(home: &Path) -> Result<PathBuf, i32> {
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("{} pwd", fresh_folder_script()))
            .env("HOME", home)
            .output()
            .expect("sh");
        match output.status.code() {
            Some(0) => Ok(PathBuf::from(
                String::from_utf8_lossy(&output.stdout).trim(),
            )),
            code => Err(code.unwrap_or(-1)),
        }
    }

    #[test]
    fn each_paste_gets_a_folder_of_its_own_that_only_its_account_opens() {
        let root = TempRoot::new("paste-folder-fresh");
        let first = run(&root.0).expect("first");
        let second = run(&root.0).expect("second");
        assert_ne!(first, second, "a second paste in the same second");
        let folder = root.0.join(ROOT);
        for made in [&first, &second] {
            assert_eq!(made.parent(), Some(folder.as_path()), "{}", made.display());
            assert!(made.is_dir());
        }
        // A screenshot may hold anything: the folder is the account's alone.
        let mode = fs::metadata(&folder).expect("folder").permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn old_pastes_are_swept_and_young_ones_and_everything_outside_the_folder_stay() {
        let root = TempRoot::new("paste-folder-sweep");
        let folder = root.0.join(ROOT);
        let (old_folder, young_folder) = (folder.join("2000-01-01-000000"), folder.join("young"));
        fs::create_dir_all(&old_folder).expect("old folder");
        fs::create_dir_all(&young_folder).expect("young folder");
        let old_inside = old_folder.join("image.png");
        let old_file = folder.join("2000-01-01-000000.png");
        let young_inside = young_folder.join("old-but-in-a-young-folder.png");
        let outside = root.0.join(".cache/bateri/2000-01-01-000000.png");
        for file in [&old_inside, &old_file, &young_inside, &outside] {
            fs::write(file, b"x").expect("a file");
        }
        for path in [&old_inside, &old_file, &young_inside, &outside, &old_folder] {
            let status = Command::new("touch")
                .args(["-t", "200001010000"])
                .arg(path)
                .status()
                .expect("touch");
            assert!(status.success());
        }
        run(&root.0).expect("pasted");
        assert!(
            !old_folder.exists(),
            "older than {KEEP_DAYS} days, with what is in it"
        );
        assert!(!old_file.exists(), "a file of the folder's own");
        assert!(young_inside.exists(), "a young folder is kept whole");
        assert!(outside.exists(), "nothing outside the folder is touched");
    }

    #[test]
    fn a_folder_that_cannot_be_made_says_so() {
        let root = TempRoot::new("paste-folder-unwritable");
        // `.cache` is a file: the folder cannot be made below it.
        fs::write(root.0.join(".cache"), b"").expect("a file in the way");
        assert_eq!(run(&root.0), Err(EXIT_UNWRITABLE));
    }

    #[test]
    fn an_agent_in_front_takes_the_paste_folder_and_a_shell_its_directory() {
        assert!(for_program(Kind::Agent(Agent::ClaudeCode)));
        assert!(for_program(Kind::Agent(Agent::Other)));
        assert!(!for_program(Kind::Shell));
    }
}
