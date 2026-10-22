//! Putting kablo on a server, over the ssh connection the user just made.
//!
//! The first `ssh` to a server that has no kablo of this version puts the
//! binary there; every later one finds it and goes on. The rules are kablo's
//! own (`kablo_proto::install`: which file a server needs, where it goes, never
//! over a version that is there) and the steps are these: ask the server what it
//! is (`uname`), look at the place the version belongs, and if it is empty write
//! the binary beside it under a name of its own, check it, make it runnable
//! and rename it into place — so a file at the place is always a whole one and
//! a binary that terminals run from is never written over.
//!
//! **The binary comes from the package** ([`Bundle`]): a directory with kablo's
//! release files — one binary per target and its `SHA256SUMS`. The version is
//! read from the names in that list, never from the version of
//! `kablo-proto` this build links: the server's path, the client and the
//! package must say the same version, and the package is the only one of the
//! three that is a binary on disk. A file whose sum is not the list's is not
//! sent.
//!
//! **A new binary replaces the old one under running terminals**
//! ([`upgrade_sessions`]): terminals the server runs from an earlier release are
//! renewed with the one just written (`kablo upgrade --all`), without ending a
//! shell or dropping a client. A *running bridge* (`kablo attach`) is not
//! replaced by it, so a bridge is made only after the renewal; one that was
//! made before it stays what it was and speaks to the new relay under the
//! rule that a release reads the one before it.
//!
//! **What goes wrong is of two kinds** ([`Failure`]). A server that kablo
//! *cannot* be kept on — a platform it is not built for, a home that is not
//! writable, a file that will not run there — is remembered
//! (`ssh_wrap::Fact::NoKablo`) and its `ssh` is a plain one from then on.
//! Anything else (the connection broke, the sum came out wrong, the server is
//! slow) may be gone next time: the `ssh` is plain this once and nothing is
//! written down.
//!
//! Every step is a command on one ssh channel ([`Channel`]); the scripts are
//! `sh` and nothing else, quoted as one word, because the login shell on the
//! other end may be anything (fish, csh) and only the shell the script is
//! handed to is known.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use kablo_proto::install::{self, Target};

use crate::upload::sq;

/// How long a step may take before the channel is given up on. The upload is
/// the long one (a binary of a few megabytes over a slow link).
pub const STEP_LIMIT: Duration = Duration::from_secs(120);

/// How long the counter may take ([`next_number`]): a step of the pane's own start, which waits
/// for it, so a server that does not answer is given up on soon (the terminal then keeps the
/// name its holder made).
pub const NUMBER_LIMIT: Duration = Duration::from_secs(15);

/// How long reading what the server's script left for the application may take
/// ([`crate::agent::notes::READ`]): a few small files, on a connection that is up.
pub const NOTES_LIMIT: Duration = Duration::from_secs(15);

/// How long reading who opened which terminal may take ([`crate::agent::notes::RECORDS`]): a
/// few small files, for a program that waits for `bateri ls`.
pub const RECORDS_LIMIT: Duration = Duration::from_secs(8);

/// How long the renewal of the server's terminals ([`upgrade_sessions`]) may
/// take. kablo gives each terminal 20 seconds; a server that is slower than a
/// few terminals' worth is not waited for, because what waits is a pane that
/// shows a connection being made.
pub const UPGRADE_LIMIT: Duration = Duration::from_secs(60);

/// A file of the package and the sum it must have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    /// The file's name (`kablo-<version>-<triple>`).
    pub name: String,
    /// Where it is.
    pub path: PathBuf,
    /// Its SHA-256, lower-case hex, from `SHA256SUMS`.
    pub sha256: String,
}

/// kablo's release files as the package carries them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bundle {
    dir: PathBuf,
    version: String,
    sums: BTreeMap<String, String>,
}

impl Bundle {
    /// The package directory: `SHA256SUMS` and the binaries it lists. The
    /// version is the one the listed binaries' names carry, and they must all
    /// carry the same.
    pub fn open(dir: &Path) -> io::Result<Bundle> {
        let text = std::fs::read_to_string(dir.join(install::CHECKSUMS))?;
        Bundle::from_sums(dir, &text)
    }

    /// [`Bundle::open`] with the list's text given.
    pub fn from_sums(dir: &Path, text: &str) -> io::Result<Bundle> {
        let sums = parse_sums(text);
        let mut versions: Vec<String> = Vec::new();
        for name in sums.keys() {
            for target in Target::ALL {
                let suffix = format!("-{}", target.triple());
                if let Some(version) = name
                    .strip_prefix("kablo-")
                    .and_then(|rest| rest.strip_suffix(&suffix))
                    && install::install_path(version).is_ok()
                    && !versions.iter().any(|known| known == version)
                {
                    versions.push(version.to_owned());
                }
            }
        }
        match versions.as_slice() {
            [version] => Ok(Bundle {
                dir: dir.to_owned(),
                version: version.clone(),
                sums,
            }),
            [] => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "no kablo binary in the list of sums",
            )),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the kablo binaries of the package are of different versions",
            )),
        }
    }

    /// The version of the package's binaries (`0.2.0`).
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The binary for a target, if the package has one.
    pub fn artifact(&self, target: Target) -> Option<Artifact> {
        let name = install::artifact_name(&self.version, target).ok()?;
        let sha256 = self.sums.get(&name)?.clone();
        Some(Artifact {
            path: self.dir.join(&name),
            name,
            sha256,
        })
    }

    /// The file's bytes, once they are the list's: a file that is not (damaged,
    /// replaced) is never sent to a server.
    pub fn read(&self, artifact: &Artifact) -> io::Result<Vec<u8>> {
        let bytes = std::fs::read(&artifact.path)?;
        if sha256_hex(&bytes) != artifact.sha256 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is not the file SHA256SUMS lists", artifact.name),
            ));
        }
        Ok(bytes)
    }

    /// Where the binary of this version is on a server, relative to the home
    /// directory (`.local/share/kablo/0.2.0/kablo`): also the command to run it
    /// over ssh, which starts in the home directory.
    pub fn remote_path(&self) -> String {
        install::install_path(&self.version).unwrap_or_default()
    }
}

/// `SHA256SUMS`' lines (`<sha>  <name>`, a `*` before the name for a binary
/// file as `sha256sum -b` writes it) as name → sum. A line that is not one is
/// skipped.
fn parse_sums(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let (sum, name) = (words.next()?, words.next()?);
            let valid = sum.len() == 64
                && sum
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            (valid && words.next().is_none())
                .then(|| (name.trim_start_matches('*').to_owned(), sum.to_owned()))
        })
        .collect()
}

// ─── the steps ───────────────────────────────────────────────────────────

/// What a command on the server said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    /// Its exit status; `None` when a signal ended it.
    pub status: Option<i32>,
    /// Standard output.
    pub out: String,
    /// Standard error.
    pub err: String,
}

/// A way to run a command on the server: one channel of the ssh connection
/// the user made. A test answers from a table.
pub trait Channel {
    /// Runs the script (`sh`) on the server with `input` on its standard
    /// input.
    fn run(&self, script: &str, input: &[u8]) -> io::Result<Reply>;
}

/// Why kablo cannot be kept on a server at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    /// A platform kablo is not built for (`uname`'s words).
    Platform(String),
    /// The home directory cannot be written to.
    Unwritable,
    /// The file is there but will not run (a mount that forbids it, a system
    /// the static binary does not suit).
    WontRun,
}

/// Why the server has no kablo now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Never: the server is recorded as one kablo cannot be kept on.
    Impossible(Why),
    /// Not this time: the text says what happened, for the log.
    Transient(String),
}

impl Failure {
    fn transient(text: impl Into<String>) -> Failure {
        Failure::Transient(text.into())
    }
}

/// What is on the server after [`ensure`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    /// The server's platform.
    pub target: Target,
    /// The binary's version.
    pub version: String,
    /// The binary's place relative to the home directory.
    pub path: String,
    /// The binary was written now (it was not there): the user is told once.
    pub fresh: bool,
}

/// Makes sure the server has the package's kablo: asks what the server is,
/// looks for the binary and writes it if it is not there. See the module
/// documentation for what a [`Failure`] means.
pub fn ensure(channel: &dyn Channel, bundle: &Bundle) -> Result<Installed, Failure> {
    let target = server_target(channel)?;
    let artifact = bundle
        .artifact(target)
        .ok_or_else(|| Failure::transient(format!("the package has no kablo for {target}")))?;
    let installed = Installed {
        target,
        version: bundle.version().to_owned(),
        path: bundle.remote_path(),
        fresh: false,
    };
    let probe = channel
        .run(&probe_script(bundle.version()), &[])
        .map_err(|error| Failure::transient(format!("looking for kablo: {error}")))?;
    // A macOS binary is code-signed when the package is made, and a signature
    // is part of the file: two packages of the same kablo have two sums. What
    // such a file says its version is has to do.
    let resigned = matches!(target, Target::MacosAarch64 | Target::MacosX86_64);
    match look(&probe, &artifact.sha256, bundle.version(), resigned) {
        Look::Absent => {}
        Look::Matches => return Ok(installed),
        Look::Differs => {
            // A binary is there that is not the package's: terminals may run
            // from it, so it is left alone, and so is the connection's
            // fate — plain this time.
            return Err(Failure::transient(
                "the kablo on the server is not the package's",
            ));
        }
    }
    let bytes = bundle
        .read(&artifact)
        .map_err(|error| Failure::transient(format!("reading the package's kablo: {error}")))?;
    let reply = channel
        .run(&upload_script(bundle.version(), &artifact.sha256), &bytes)
        .map_err(|error| Failure::transient(format!("writing kablo: {error}")))?;
    match reply.status {
        Some(0) => Ok(Installed {
            fresh: true,
            ..installed
        }),
        Some(EXIT_UNWRITABLE) => Err(Failure::Impossible(Why::Unwritable)),
        Some(EXIT_WONT_RUN) => Err(Failure::Impossible(Why::WontRun)),
        Some(EXIT_DAMAGED) => Err(Failure::transient("kablo arrived damaged")),
        Some(EXIT_INCOMPLETE) => Err(Failure::transient("kablo could not be written in full")),
        other => Err(Failure::transient(format!(
            "writing kablo ended with {other:?}: {}",
            reply.err.trim()
        ))),
    }
}

/// What the server is: `uname`'s answer as a kablo target.
fn server_target(channel: &dyn Channel) -> Result<Target, Failure> {
    let reply = channel
        .run(install::UNAME_COMMAND, &[])
        .map_err(|error| Failure::transient(format!("asking the server what it is: {error}")))?;
    if reply.status != Some(0) {
        return Err(Failure::transient(format!(
            "`{}` ended with {:?}",
            install::UNAME_COMMAND,
            reply.status
        )));
    }
    Target::from_uname_output(&reply.out).ok_or_else(|| {
        let words = reply
            .out
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or_default()
            .trim()
            .to_owned();
        Failure::Impossible(Why::Platform(words))
    })
}

/// The exit statuses of [`upload_script`].
const EXIT_DAMAGED: i32 = 7;
/// The file could be made but not filled or made runnable (a disk that filled
/// up, a pipe that broke): a bad moment, not a server that cannot keep kablo.
const EXIT_INCOMPLETE: i32 = 8;
const EXIT_UNWRITABLE: i32 = 9;
const EXIT_WONT_RUN: i32 = 10;

/// The script that looks at the place of a version: `present`, the sum of the
/// file (whichever tool the server has) and what the file says its version is.
/// One line of `sh`: [`crate::ssh_wrap::is_inline`]'s alphabet and no more.
pub fn probe_script(version: &str) -> String {
    format!(
        "f=.local/share/kablo/{version}/kablo; if [ -x $f ]; then echo present; \
         (sha256sum $f || shasum -a 256 $f) 2>/dev/null; $f version 2>/dev/null; fi"
    )
}

/// What [`probe_script`] found.
#[derive(Debug, PartialEq, Eq)]
enum Look {
    Absent,
    Matches,
    Differs,
}

/// `resigned`: the file's sum says nothing against it when it names the
/// package's version (it may be the same kablo, signed by another package).
fn look(reply: &Reply, sha256: &str, version: &str, resigned: bool) -> Look {
    if reply.status != Some(0) || !reply.out.lines().any(|line| line.trim() == "present") {
        return Look::Absent;
    }
    let says = format!("version={version}");
    let named = || {
        reply
            .out
            .lines()
            .any(|line| line.split_whitespace().any(|word| word == says))
    };
    let sum = reply.out.lines().find_map(|line| {
        let first = line.split_whitespace().next()?;
        (first.len() == 64 && first.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| first.to_ascii_lowercase())
    });
    match sum {
        Some(sum) if sum == sha256 => Look::Matches,
        Some(_) if resigned && named() => Look::Matches,
        Some(_) => Look::Differs,
        // The server has no tool to sum with: the file's own word has to do.
        None if named() => Look::Matches,
        None => Look::Differs,
    }
}

/// The script that writes the binary (on its standard input) to the place of
/// the version: beside it under a name of its own, made runnable, checked
/// against the sum (when the server has a tool to sum with), run once to see
/// that it runs there, and only then renamed into place — and not over a file
/// that runs, which came to be meanwhile ([`probe_script`] looks for the same:
/// a file that cannot run is replaced). The exit statuses say what went wrong:
/// [`EXIT_UNWRITABLE`] (the directory or the file cannot be made),
/// [`EXIT_INCOMPLETE`], [`EXIT_DAMAGED`], [`EXIT_WONT_RUN`].
pub fn upload_script(version: &str, sha256: &str) -> String {
    format!(
        "d=.local/share/kablo/{version}; f=$d/kablo; t=$d/.new-$$; \
         mkdir -p $d && : >$t || {{ rm -f $t; exit {EXIT_UNWRITABLE}; }}; \
         cat >$t && chmod 755 $t || {{ rm -f $t; exit {EXIT_INCOMPLETE}; }}; \
         s=$(sha256sum $t 2>/dev/null || shasum -a 256 $t 2>/dev/null); \
         case $s in {sha256}*) ;; ?*) rm -f $t; exit {EXIT_DAMAGED};; esac; \
         $t version >/dev/null 2>&1 || {{ rm -f $t; exit {EXIT_WONT_RUN}; }}; \
         if [ -x $f ]; then rm -f $t; else mv $t $f || {{ rm -f $t; exit {EXIT_UNWRITABLE}; }}; fi"
    )
}

/// The script that has the terminals the server is running (this user's, all of
/// them) renewed with the binary of `version`, which is there: the relay of each
/// is replaced by the one in that binary and the warden follows, with the shell,
/// its programs and the clients attached to it staying as they are. The path is
/// the one relative to the home directory the commands start in; kablo makes it
/// absolute itself. One line of `sh`, like the others.
pub fn upgrade_script(version: &str) -> String {
    format!("f=.local/share/kablo/{version}/kablo; $f upgrade --all $f")
}

/// Renews the server's running terminals with the kablo [`ensure`] found or
/// wrote ([`upgrade_script`]) — **best effort**: a server with no terminals has
/// no directory for them and the command says so; a terminal that cannot be
/// renewed keeps its relay and is served by the bridge's "this release and the
/// one before" rule. Nothing it says changes what happens next, so the answer
/// (every terminal renewed, or there were none) is for a test and a log.
pub fn upgrade_sessions(channel: &dyn Channel, installed: &Installed) -> bool {
    channel
        .run(&upgrade_script(&installed.version), &[])
        .is_ok_and(|reply| reply.status == Some(0))
}

// ─── the number of a terminal ────────────────────────────────────────────

/// The number of the next terminal on the server (`7`: the `t7` an agent there is told), from
/// the counter the agent's command uses ([`crate::ssh_wrap::COUNTER`], the same bytes): a number
/// is never given twice, whoever asks, and one that is lost with a caller that died is not given
/// again. `None` when the server could not count — no home, a directory it cannot write, a
/// counter that is not a number, a connection that broke — and the terminal then keeps the name
/// its holder made.
pub fn next_number(channel: &dyn Channel) -> Option<u64> {
    let reply = channel.run(crate::ssh_wrap::COUNTER, &[]).ok()?;
    if reply.status != Some(0) {
        return None;
    }
    // The last line: a login shell that says something of its own comes before it.
    let line = reply
        .out
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())?;
    let digits = line.trim();
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

// ─── the channel ─────────────────────────────────────────────────────────

/// A [`Channel`] over an ssh argv: `ssh` and the options that make it use the
/// connection the user made ([`crate::upload::ssh_argv_for`]), then the script
/// as one word for `sh -c`.
pub struct SshChannel {
    /// The ssh program, its options and the destination.
    pub argv: Vec<String>,
    /// How long one step may take.
    pub limit: Duration,
}

impl Channel for SshChannel {
    fn run(&self, script: &str, input: &[u8]) -> io::Result<Reply> {
        let mut argv = self.argv.clone();
        argv.push(format!("sh -c {}", sq(script)));
        run_command(&argv, input, self.limit)
    }
}

/// Runs `argv`, writes `input` to it, collects what it says; ends it when it
/// takes longer than `limit`.
pub fn run_command(argv: &[String], input: &[u8], limit: Duration) -> io::Result<Reply> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty argv"))?;
    let mut child = crate::ssh_route::command(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let data = input.to_vec();
    let writer = std::thread::spawn(move || {
        if let Some(stdin) = stdin.as_mut() {
            let _ = stdin.write_all(&data);
        }
        drop(stdin);
    });
    let out = std::thread::spawn(move || {
        let mut text = Vec::new();
        if let Some(stdout) = stdout.as_mut() {
            let _ = stdout.read_to_end(&mut text);
        }
        text
    });
    let err = std::thread::spawn(move || {
        let mut text = Vec::new();
        if let Some(stderr) = stderr.as_mut() {
            let _ = stderr.read_to_end(&mut text);
        }
        text
    });
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            crate::program::end_group(&mut child);
            break None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    // A step that ran out of time is not waited for further: something it
    // started may hold its pipes, and the readers end when that does.
    let Some(status) = status else {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "the step took too long",
        ));
    };
    let _ = writer.join();
    let out = out.join().unwrap_or_default();
    let err = err.join().unwrap_or_default();
    Ok(Reply {
        status: status.code(),
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
    })
}

// ─── SHA-256 ─────────────────────────────────────────────────────────────

/// The SHA-256 of `data`, as lower-case hex. Written here and not taken from a
/// crate: the package's files are checked against a list of these before they
/// are sent anywhere, and nothing else needs the function.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_be_bytes());
    for block in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(value);
        }
    }
    h.iter().map(|word| format!("{word:08x}")).collect()
}

#[cfg(test)]
mod tests;
