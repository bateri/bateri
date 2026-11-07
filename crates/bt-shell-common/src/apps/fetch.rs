//! Getting bytes from an address: `curl`, because bateri has no HTTP client of its own (the
//! `pencere` command fetches the same way) and one more crate for a daily check of a few small
//! files is not worth its weight.
//!
//! [`Fetch`] is the seam the checks are written on: the production body is [`Curl`], and a test
//! puts a map of addresses to bytes where the network was.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

/// What a check needs of the network: the bytes at an address, and a file downloaded to a path.
/// An error is the text to show for it, one line.
pub trait Fetch: Send + Sync {
    /// The bytes at `address`, at most `limit` of them.
    fn get(&self, address: &str, limit: u64) -> Result<Vec<u8>, String>;

    /// The file at `address` written to `path` (made, or replaced), at most `limit` bytes.
    fn download(&self, address: &str, limit: u64, path: &Path) -> Result<(), String>;
}

/// `curl` as the system has it (`/usr/bin/curl`, on a Mac and on the Linux machines bateri runs
/// on: the program is not a setting, so nothing can be made to run in its place). `-q` first, so `~/.curlrc` cannot add an option; `-f` so an HTTP
/// error is an error and not the body of an error page; `-L` with the redirects kept to `https:`
/// (GitHub's download address redirects to its storage); and a time and a size limit on each
/// call. An address of the `file:` kind is read only when it was named as such: the program is
/// told the one protocol the address has.
#[derive(Debug, Clone, Copy, Default)]
pub struct Curl;

/// How long one call may take, by what it fetches: a manifest is small, a program is some
/// megabytes and the connection may be slow.
const MANIFEST_SECONDS: u32 = 30;
const FILE_SECONDS: u32 = 600;
const CONNECT_SECONDS: u32 = 15;

impl Curl {
    fn command(&self, address: &str, limit: u64) -> Command {
        let seconds = if limit > 1 << 20 {
            FILE_SECONDS
        } else {
            MANIFEST_SECONDS
        };
        let protocol = if address.starts_with("file:") {
            "=file"
        } else {
            "=https"
        };
        let mut command = Command::new("/usr/bin/curl");
        command
            .args(["-q", "-f", "-s", "-S", "-L"])
            .args(["--proto", protocol, "--proto-redir", "=https"])
            .args(["--max-redirs", "8"])
            .args(["--connect-timeout", &CONNECT_SECONDS.to_string()])
            .args(["--max-time", &seconds.to_string()])
            .args(["--max-filesize", &limit.to_string()])
            .args(["--user-agent", "bateri"])
            .stdin(Stdio::null())
            .stderr(Stdio::piped());
        command
    }
}

impl Fetch for Curl {
    fn get(&self, address: &str, limit: u64) -> Result<Vec<u8>, String> {
        let mut child = self
            .command(address, limit)
            .arg("--")
            .arg(address)
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not start curl: {e}"))?;
        // One byte more than the limit says it was too long; the rest is never read.
        let mut body = Vec::new();
        let read = child.stdout.take().map_or(Ok(0), |out| {
            out.take(limit.saturating_add(1)).read_to_end(&mut body)
        });
        if body.len() as u64 > limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("more than {limit} bytes"));
        }
        let output = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
        if !output.status.success() {
            return Err(failure(&output.stderr, output.status));
        }
        read.map_err(|e| format!("curl: {e}"))?;
        Ok(body)
    }

    fn download(&self, address: &str, limit: u64, path: &Path) -> Result<(), String> {
        let output = self
            .command(address, limit)
            .arg("-o")
            .arg(path)
            .arg("--")
            .arg(address)
            .stdout(Stdio::null())
            .output()
            .map_err(|e| format!("could not start curl: {e}"))?;
        if !output.status.success() {
            let _ = fs::remove_file(path);
            return Err(failure(&output.stderr, output.status));
        }
        // `--max-filesize` only holds for a size the server announced.
        match fs::metadata(path) {
            Ok(meta) if meta.len() <= limit => Ok(()),
            Ok(_) => {
                let _ = fs::remove_file(path);
                Err(format!("more than {limit} bytes"))
            }
            Err(e) => Err(format!("curl wrote nothing: {e}")),
        }
    }
}

/// The first line curl said, else its exit status.
fn failure(stderr: &[u8], status: std::process::ExitStatus) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map_or_else(|| format!("curl {status}"), str::to_owned)
}
