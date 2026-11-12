//! How the bytes of a program reach a machine: written, on the machine, to the folder kablo made
//! ready for them, and checked there against the signed sum. Nothing else is done on the machine
//! by bateri: the program is put in its place, and in use, by kablo, which judges the release again
//! (`kablo bateri package install`; `kablo-package`).
//!
//! The folder is `.<version>.part-<part>` below the program's own, named by kablo, and it holds
//! the program under the program's name. A visit that ended badly leaves a folder of this kind
//! behind; the ones more than a day old are swept away by the next, as kablo sweeps them too.
//!
//! **No byte a program or a server chose is read by a shell.** The script holds fixed text and
//! four words that are checked first: the program's name (an `app.json` signed by its author
//! names it, in the characters [`is_name`] allows), the version (three numbers), the sum (64
//! hex digits) and the part (letters, digits and `-`).

use std::io;
use std::time::Duration;

use bt_core::pencere::apps::{Version, is_name};

use crate::kablo::install::{Channel, Reply, reply_of};
use crate::kablo::served::Served;
use crate::ssh_route::Ask;

/// The apps folder on a machine, relative to its home directory.
pub(crate) const ROOT: &str = ".local/share/bateri/apps";

/// How long the connection and the machine's answer may take, for the step that writes a program.
pub const WAIT: Duration = Duration::from_secs(60);

/// The most of what a step says that is kept: it says almost nothing.
const SAID: u64 = 4 * 1024;

/// The exit codes of the script that say what went wrong.
const EXIT_DAMAGED: i32 = 7;
const EXIT_INCOMPLETE: i32 = 8;
const EXIT_UNWRITABLE: i32 = 9;

/// Whether `part` is what kablo names a folder by: letters, digits and `-`, and not long.
pub fn is_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 64
        && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Whether `sum` is a SHA-256 as a release signs it: 64 lowercase hex digits.
fn is_sum(sum: &str) -> bool {
    sum.len() == 64
        && sum
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The program: its bytes on the standard input, written to the folder `.<version>.part-<part>`
/// beside the release's own place, checked against `sum`, made executable. The folder is below the
/// `HOME` of the account the step runs for (not the directory it starts in, which a bridge takes
/// from the password file), as kablo's is.
fn script(name: &str, version: Version, sum: &str, part: &str) -> String {
    format!(
        "r=\"$HOME/{ROOT}\"; p=\"$r/{name}/.{version}.part-{part}\"; f=\"$p/{name}\"; \
         find \"$r/{name}/.\" ! -name . -prune -type d -name '.*.part-*' -mtime +0 \
           -exec rm -rf {{}} \\; 2>/dev/null; \
         mkdir -p \"$p\" || exit {EXIT_UNWRITABLE}; \
         cat >\"$f\" && chmod 755 \"$f\" || {{ rm -rf \"$p\"; exit {EXIT_INCOMPLETE}; }}; \
         s=$(sha256sum \"$f\" 2>/dev/null || shasum -a 256 \"$f\" 2>/dev/null); \
         case $s in {sum}*) ;; ?*) rm -rf \"$p\"; exit {EXIT_DAMAGED};; esac"
    )
}

/// Writes `bytes` as the program `name` of `version` into the part folder `part` on the machine
/// `channel` reaches. `Err` is a line for the person.
pub fn put(
    channel: &dyn Channel,
    name: &str,
    version: Version,
    sum: &str,
    part: &str,
    bytes: &[u8],
) -> Result<(), String> {
    if !is_name(name) || !is_sum(sum) || !is_part(part) {
        return Err("the program cannot be named to the machine".to_owned());
    }
    let reply = channel
        .run(&script(name, version, sum, part), bytes)
        .map_err(|error| format!("the machine could not be written to: {error}"))?;
    match reply.status {
        Some(0) => Ok(()),
        Some(EXIT_DAMAGED) => Err("the file arrived damaged".to_owned()),
        Some(EXIT_INCOMPLETE) => Err("the file did not arrive whole".to_owned()),
        Some(EXIT_UNWRITABLE) => Err("the machine's apps folder cannot be written".to_owned()),
        _ => Err(format!(
            "writing the file failed: {}",
            reply.err.lines().next().unwrap_or("no reason given")
        )),
    }
}

/// A machine that is reached over the connection bateri keeps to it, as a way to run a script
/// there ([`Served`]): the same for this computer's own account and for a server.
pub struct Through {
    pub served: Served,
    pub wait: Duration,
}

impl Channel for Through {
    fn run(&self, script: &str, input: &[u8]) -> io::Result<Reply> {
        let command = self
            .served
            .sh(script, Ask::Never, self.wait)
            .map_err(io::Error::other)?;
        reply_of(command, input, SAID, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kablo::local::LocalChannel;
    use crate::settings::TempRoot;
    use bt_core::pencere::Digest;
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;

    fn version() -> Version {
        Version::parse("1.2.3").expect("a version")
    }

    fn home(label: &str) -> (TempRoot, LocalChannel) {
        let root = TempRoot::new(label);
        let home = root.0.join("home");
        fs::create_dir_all(&home).expect("a home");
        (root, LocalChannel::new(home))
    }

    #[test]
    fn the_program_is_written_to_the_part_folder_whole_and_executable() {
        let (_root, channel) = home("upload-whole");
        let bytes = b"#!/bin/sh\necho hi\n";
        let sum = Digest::of(bytes).to_string();
        put(&channel, "textpen", version(), &sum, "ab-12", bytes).expect("written");
        let file = channel
            .home
            .join(ROOT)
            .join("textpen/.1.2.3.part-ab-12/textpen");
        assert_eq!(fs::read(&file).expect("the file"), bytes);
        assert_eq!(
            fs::metadata(&file).expect("metadata").permissions().mode() & 0o777,
            0o755
        );
        // Nothing is put in use: that is kablo's.
        assert!(!channel.home.join(ROOT).join("bin").exists());
        assert!(!channel.home.join(ROOT).join("textpen/1.2.3").exists());
    }

    #[test]
    fn bytes_that_are_not_the_signed_sum_are_not_left_behind() {
        let (_root, channel) = home("upload-damaged");
        let sum = Digest::of(b"the signed one").to_string();
        let error =
            put(&channel, "textpen", version(), &sum, "p1", b"another").expect_err("damaged");
        assert!(error.contains("damaged"), "{error}");
        assert!(
            !channel
                .home
                .join(ROOT)
                .join("textpen/.1.2.3.part-p1")
                .exists()
        );
    }

    #[test]
    fn a_folder_a_visit_left_more_than_a_day_ago_is_swept_and_a_young_one_stays() {
        let (_root, channel) = home("upload-sweep");
        let old = channel.home.join(ROOT).join("textpen/.0.9.0.part-old");
        let young = channel.home.join(ROOT).join("textpen/.0.9.1.part-young");
        for folder in [&old, &young] {
            fs::create_dir_all(folder).expect("a folder");
        }
        let status = std::process::Command::new("touch")
            .args(["-t", "200001010000"])
            .arg(&old)
            .status()
            .expect("touch");
        assert!(status.success());
        let bytes = b"x";
        let sum = Digest::of(bytes).to_string();
        put(&channel, "textpen", version(), &sum, "p2", bytes).expect("written");
        assert!(!old.exists());
        assert!(young.exists());
    }

    #[test]
    fn what_cannot_be_a_word_of_the_script_is_not_put_in_it() {
        let (_root, channel) = home("upload-words");
        let sum = Digest::of(b"x").to_string();
        for (name, part) in [
            ("a b", "p"),
            ("a;b", "p"),
            ("textpen", "p; rm -rf /"),
            ("textpen", ""),
        ] {
            assert!(put(&channel, name, version(), &sum, part, b"x").is_err());
        }
        assert!(put(&channel, "textpen", version(), "not a sum", "p", b"x").is_err());
        assert!(!channel.home.join(ROOT).exists());
    }
}
