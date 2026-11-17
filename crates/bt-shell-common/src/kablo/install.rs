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
//! **The shells' files go with the look** ([`probe_and_files_script`]): the files a terminal's
//! shell is set up from (the dock's functions, the wrappers, the agent's command) are written to
//! the server in the call that looks for kablo, every time, whether or not kablo is already
//! there — a file that is the same is left as it is. They are the Mac's own bytes, so a server
//! never runs a wrapper older than the application that opens its terminal.
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
//! ([`upgrade_sessions`]): the terminals that run from an earlier release are renewed with the
//! one that is there (`kablo upgrade <id>`, one per terminal — never `--all`), without ending a
//! shell or dropping a client. Which terminals is the rule of [`Census::renewable`]: the ledger's,
//! and the ones nobody has open. A *running bridge* (`kablo attach`) is not replaced by it, so a
//! bridge is made only after the renewal; one that was made before it stays what it was and
//! speaks to the new relay under the rule that a release reads the one before it. The version
//! the renewal left behind goes when no terminal of the machine runs from it
//! ([`prune_versions`]). **Both happen at every connection** ([`tidy`]), not only when the
//! binary was just written: the machine is looked at by the call that looks for the binary, and
//! a machine that has nothing to do is asked for nothing more.
//!
//! **This Mac is a place kablo is put too** ([`ensure_here`], [`super::local`]): the same
//! steps through a channel that is `sh` in the home directory, without the shells' files.
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
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use kablo_proto::SessionId;
use kablo_proto::install::{self, Target};
use kablo_proto::v1::Exit;

use super::census::{self, Census};
use super::remote::RemoteCommand;
use crate::ssh_wrap::Wants;
use crate::upload::sq;

/// How long a step may take before the channel is given up on. The upload is
/// the long one (a binary of a few megabytes over a slow link).
pub const STEP_LIMIT: Duration = Duration::from_secs(120);

/// How long the counter may take ([`next_number`]): a step of the pane's own start, which waits
/// for it, so a server that does not answer is given up on soon (the terminal then keeps the
/// name its holder made).
pub const NUMBER_LIMIT: Duration = Duration::from_secs(15);

/// The counter run on this Mac ([`next_number`] over a local process): no connection to wait
/// for, and shorter than what a local pane waits for its terminal, so the pane is not given up
/// on while the counter is still counting.
pub const LOCAL_NUMBER_LIMIT: Duration = Duration::from_secs(5);

/// How long reading what the server's command left for the application may take
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
    /// directory (`.local/share/bateri/kablo/0.2.0/kablo`): also the command to run it
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

    /// [`Channel::run`] for an input that takes a while to cross, told how much of it has: a
    /// channel that cannot say, or stop, runs it whole ([`Watch`]).
    fn put(&self, script: &str, input: &[u8], _watch: &dyn Watch) -> io::Result<Reply> {
        self.run(script, input)
    }
}

/// Whoever waits for a long input to cross: told how much has, and asked whether to go on. The
/// first connection to a server is one, in the dock's row (a number of bytes and a *Cancel*).
pub trait Watch: Send + Sync {
    /// `done` of `total` bytes have been handed to the connection (not that the server has them:
    /// what the connection holds in its buffers is counted). Called from the thread that waits for
    /// the step, when the count has moved, a few times a second at most.
    fn sent(&self, done: u64, total: u64);

    /// Whether the person said to stop: the write ends, and what the server was given is not a
    /// file ([`upload_script`] leaves nothing behind that is not whole).
    fn stopped(&self) -> bool;
}

/// A [`Watch`] that watches nothing: what is not shown to anyone.
pub struct Unwatched;

impl Watch for Unwatched {
    fn sent(&self, _done: u64, _total: u64) {}

    fn stopped(&self) -> bool {
        false
    }
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
    /// What the call that looked for the binary also saw of the machine: the versions that are
    /// installed and the terminals that run ([`Census`]). Only when the binary was there already
    /// and the answer was a whole one; the one who asked about the machine keeps none of it
    /// ([`tidy`] asks again when there is none, and nothing is kept from one look to the next).
    pub census: Option<Census>,
}

impl Installed {
    /// Which root the binary is in, from its path.
    pub fn root(&self) -> Root {
        Root::of(&self.path)
    }
}

/// The two places a version of kablo is kept below the home directory: where it is put now, and
/// where it was put before it moved into bateri's directory. A terminal started before the move
/// runs from the former one until it is renewed, so a machine can have both, and what is known of
/// a machine's kablo has to say which.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Root {
    /// [`kablo_proto::install::ROOT`]: where a version is installed.
    Current,
    /// [`kablo_proto::install::FORMER_ROOT`]: where one was installed before.
    Former,
}

impl Root {
    /// The directory, relative to the home directory, as it is written in a script and in the
    /// ledger.
    pub fn dir(self) -> &'static str {
        match self {
            Root::Current => install::ROOT,
            Root::Former => install::FORMER_ROOT,
        }
    }

    /// The binary of `version` in this root, relative to the home directory.
    pub fn path(self, version: &str) -> Result<String, kablo_proto::Error> {
        match self {
            Root::Current => install::install_path(version),
            Root::Former => install::former_install_path(version),
        }
    }

    /// The binary of `version` in this root under a known home directory (an absolute path).
    pub fn path_in(self, home: &str, version: &str) -> Result<String, kablo_proto::Error> {
        match self {
            Root::Current => install::install_path_in(home, version),
            Root::Former => install::former_install_path_in(home, version),
        }
    }

    /// The root a binary's path is in: the former one if it is under it, the current one for
    /// any other.
    pub fn of(path: &str) -> Root {
        match path
            .strip_prefix(install::FORMER_ROOT)
            .is_some_and(|rest| rest.starts_with('/'))
        {
            true => Root::Former,
            false => Root::Current,
        }
    }
}

/// Makes sure the server has the package's kablo: asks what the server is,
/// looks for the binary (writing the shells' files in the same call) and writes
/// it if it is not there. See the module documentation for what a [`Failure`]
/// means.
pub fn ensure(channel: &dyn Channel, bundle: &Bundle) -> Result<Installed, Failure> {
    ensure_for(channel, bundle, Wants::COMMAND)
}

/// [`ensure`] for a server whose `bateri` command is wanted (`wants.agent`) or not
/// (`commands = "off"`, [`crate::ssh_wrap::files_script_for`]), with the hooks its agents' settings
/// ask for ([`Wants`]; a `bool` is the command alone).
pub fn ensure_for(
    channel: &dyn Channel,
    bundle: &Bundle,
    wants: impl Into<Wants>,
) -> Result<Installed, Failure> {
    ensure_watched(channel, bundle, wants, &Unwatched)
}

/// [`ensure_for`] for a person who waits for the binary to cross ([`Watch`]): told how far it
/// is, and able to stop it ([`Failure::Transient`] then, and no binary is left on the server).
pub fn ensure_watched(
    channel: &dyn Channel,
    bundle: &Bundle,
    wants: impl Into<Wants>,
    watch: &dyn Watch,
) -> Result<Installed, Failure> {
    place(channel, bundle, Files::Server(wants.into()), watch)
}

/// [`ensure`] for this Mac ([`super::local`]): the same look and the same write, with the one
/// files of the shells' that this Mac has a reader for — the agent's command and the agents'
/// hooks ([`crate::ssh_wrap::files_script_here`]; without the command, `[commands] level = "off"`,
/// neither is written and both are removed). The rest of a server's files have none: a terminal
/// here starts from the application's own shells.
pub fn ensure_here(
    channel: &dyn Channel,
    bundle: &Bundle,
    wants: impl Into<Wants>,
) -> Result<Installed, Failure> {
    place(channel, bundle, Files::Here(wants.into()), &Unwatched)
}

/// Which of the shells' files go with the look at kablo ([`place`]).
#[derive(Clone, Copy)]
enum Files {
    /// A server's whole set, with the `bateri` command or without it, and the hooks asked for.
    Server(Wants),
    /// This Mac's ([`crate::ssh_wrap::files_script_here`]).
    Here(Wants),
}

/// What [`ensure`] and [`ensure_here`] are: the look at kablo, and the files that ride it.
fn place(
    channel: &dyn Channel,
    bundle: &Bundle,
    files: Files,
    watch: &dyn Watch,
) -> Result<Installed, Failure> {
    let target = server_target(channel)?;
    let artifact = bundle
        .artifact(target)
        .ok_or_else(|| Failure::transient(format!("the package has no bateri for {target}")))?;
    let installed = Installed {
        target,
        version: bundle.version().to_owned(),
        path: bundle.remote_path(),
        fresh: false,
        census: None,
    };
    // The shells' files ride the call that looks for kablo: no step of their own, on a connection
    // that has no limit on how many bytes one call carries ([`probe_and_files_script`]).
    let script = match files {
        Files::Server(wants) => crate::ssh_wrap::files_script_for(Some(&installed.path), wants),
        Files::Here(wants) => crate::ssh_wrap::files_script_here(Some(&installed.path), wants),
    };
    let probe = channel
        .run(&probe_and_files_script(bundle.version()), script.as_bytes())
        .map_err(|error| Failure::transient(format!("looking for bateri: {error}")))?;
    // A macOS binary is code-signed when the package is made, and a signature
    // is part of the file: two packages of the same kablo have two sums. What
    // such a file says its version is has to do.
    let resigned = matches!(target, Target::MacosAarch64 | Target::MacosX86_64);
    let (found, census) = split_look(&probe);
    match look(&found, &artifact.sha256, bundle.version(), resigned) {
        Look::Absent => {}
        Look::Matches => {
            return Ok(Installed {
                census,
                ..installed
            });
        }
        Look::Differs => {
            // A binary is there that is not the package's: terminals may run
            // from it, so it is left alone, and so is the connection's
            // fate — plain this time.
            return Err(Failure::transient(
                "the bateri on the server is not the package's",
            ));
        }
    }
    let bytes = bundle
        .read(&artifact)
        .map_err(|error| Failure::transient(format!("reading the package's bateri: {error}")))?;
    let reply = channel
        .put(
            &upload_script(bundle.version(), &artifact.sha256),
            &bytes,
            watch,
        )
        .map_err(|error| Failure::transient(format!("writing bateri: {error}")))?;
    match reply.status {
        Some(0) => Ok(Installed {
            fresh: true,
            ..installed
        }),
        Some(EXIT_UNWRITABLE) => Err(Failure::Impossible(Why::Unwritable)),
        Some(EXIT_WONT_RUN) => Err(Failure::Impossible(Why::WontRun)),
        Some(EXIT_DAMAGED) => Err(Failure::transient("bateri arrived damaged")),
        Some(EXIT_INCOMPLETE) => Err(Failure::transient("bateri could not be written in full")),
        other => Err(Failure::transient(format!(
            "writing bateri ended with {other:?}: {}",
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
    let root = install::ROOT;
    format!(
        "f={root}/{version}/kablo; if [ -x $f ]; then echo present; \
         (sha256sum $f || shasum -a 256 $f) 2>/dev/null; $f version 2>/dev/null; fi"
    )
}

/// [`probe_script`], and then the server's `sh` reads from the call's standard input the script
/// that writes the shells' files ([`crate::ssh_wrap::files_script`]): the files go to the server
/// in the call that looks for kablo, not in one of their own — and not in the terminal's command
/// either, whose one argument could not hold them. What that script says is not the call's: its
/// output is dropped and the call ends well, so [`look`] reads the probe alone; a file that could
/// not be written is found missing by the bootstrap. The agent's command in them names the place
/// kablo has when the package's is the one the server ends up with; where [`look`] finds another
/// binary there, [`ensure`] fails and the `ssh` is the plain one, which runs none of them.
///
/// What the machine has of kablo — the versions that are installed and the terminals that run
/// ([`census_script`]) — is said by the same call, after [`PROBE_END`]: the cleanup that goes
/// with every connection ([`tidy`]) chooses by it and asks nothing of the server in a call of
/// its own when there is nothing to do. One line of `sh`, like the others.
pub fn probe_and_files_script(version: &str) -> String {
    format!(
        "{}; sh -s >/dev/null 2>&1; echo {PROBE_END}; {}; :",
        probe_script(version),
        census_script(version)
    )
}

/// The line between what the call that looks for kablo says of the place of the version and what
/// it says of the machine ([`probe_and_files_script`]).
const PROBE_END: &str = "===";

/// The look of [`probe_and_files_script`]'s answer, and the census after it: the census is
/// `None` where it is not there or not a whole one, and an answer that has no [`PROBE_END`] (a
/// stand-in's, a server that ended the call early) is the look alone.
fn split_look(reply: &Reply) -> (Reply, Option<Census>) {
    match reply.out.split_once(&format!("{PROBE_END}\n")) {
        Some((found, rest)) => (
            Reply {
                out: found.to_owned(),
                ..reply.clone()
            },
            Census::parse(rest),
        ),
        None => (reply.clone(), None),
    }
}

/// Writes the shells' files alone, to a server whose kablo is where `installed` says (the
/// application's ledger knew it, so there was no look): the same script, the same bytes, as a call
/// of their own (the command file in them is the one line that starts kablo). **Best effort**: what could not be written is found missing by the bootstrap,
/// which says so.
pub fn put_files(channel: &dyn Channel, installed: &Installed, wants: impl Into<Wants>) {
    let files = crate::ssh_wrap::files_script_for(Some(&installed.path), wants);
    let _ = channel.run("sh -s >/dev/null 2>&1; :", files.as_bytes());
}

/// [`put_files`] for this Mac ([`crate::ssh_wrap::files_script_here`]): the agent's command and
/// the hooks asked for, or their removal when they are not wanted.
pub fn put_files_here(channel: &dyn Channel, installed: &Installed, wants: impl Into<Wants>) {
    let files = crate::ssh_wrap::files_script_here(Some(&installed.path), wants);
    let _ = channel.run("sh -s >/dev/null 2>&1; :", files.as_bytes());
}

// ─── the folder for a program's files ────────────────────────────────────

/// What the folder script prints before the folder's path (the login shell may say things of
/// its own before it, and nothing else begins with this).
const STAGE_MARK: &str = "pencere-stage";

/// What a pipe's script prints when the pipe is there, and when the program has opened its end.
pub const PIPE_READY: &str = "pencere-ready";
pub const PIPE_OPEN: &str = "pencere-open";

/// The exit status of a pipe's script that could not make the pipe.
pub const PIPE_NOT_MADE: i32 = 20;

/// How long a pipe's script waits for the program to open its end on its own, whatever the Mac
/// does: the Mac that asked for the pipe has gone when its connection has, and the script is not
/// to wait for it for ever. Longer than the Mac's own wait.
const PIPE_WAIT_SECS: u32 = 12;

/// The script that makes the folder a program on the server may be offered for the bytes of a
/// file ([`crate::stage`]): `~/.cache/pencere-stage/<instance>`, closed to everyone but the user,
/// with a marker holding `token`. It checks that the folder above is the user's own and not a link,
/// removes what an earlier run of the same instance left, removes the folders of instances that
/// nobody has touched for a month, and says where the folder really is (`pwd -P`: the program
/// opens the folder without following a link, and a home is often reached through one).
///
/// `None` unless `instance` and `token` are lowercase hexadecimal: they are part of the script.
/// The script is read by `sh` from standard input.
#[must_use]
pub fn stage_folder_script(instance: &str, token: &str) -> Option<String> {
    let hex = |text: &str| {
        !text.is_empty()
            && text.len() <= 64
            && text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };
    if !hex(instance) || !hex(token) {
        return None;
    }
    Some(format!(
        r#"umask 077
case $HOME in /*) ;; *) exit 2 ;; esac
mkdir -p "$HOME/.cache" || exit 3
r=$HOME/{root}
if [ -L "$r" ]; then exit 4; fi
mkdir -p "$r" || exit 5
chmod 700 "$r" || exit 5
set -- $(ls -dn "$r")
[ "$3" = "$(id -u)" ] || exit 6
d=$r/{instance}
rm -rf "$d"
mkdir "$d" || exit 7
chmod 700 "$d" || exit 7
printf %s {token} > "$d/.marker-new" || exit 8
mv -f "$d/.marker-new" "$d/{marker}" || exit 8
for e in "$r"/*; do
  [ -d "$e" ] && [ ! -L "$e" ] || continue
  [ "$e" = "$d" ] && continue
  [ -n "$(find "$e" -prune -mtime +{stale} 2>/dev/null)" ] && rm -rf "$e"
done
cd "$d" || exit 9
printf '{mark} %s\n' "$(pwd -P)"
"#,
        root = crate::stage::ROOT,
        marker = crate::stage::MARKER,
        stale = crate::stage::STALE_AFTER.as_secs() / 86_400,
        mark = STAGE_MARK,
    ))
}

/// The folder's path in what [`stage_folder_script`] said, if it said one a script may be trusted
/// with ([`crate::stage::is_plain_path`]).
fn stage_folder_path(reply: &Reply) -> Option<String> {
    if reply.status != Some(0) {
        return None;
    }
    let path = reply
        .out
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix(STAGE_MARK)?.strip_prefix(' '))?
        .trim();
    crate::stage::is_plain_path(path).then(|| path.to_owned())
}

/// Makes the folder `name` ([`crate::stage::folder_name`]) on the server `channel` runs on, with a
/// token of its own, and returns it. `None` when the server would not (no `$HOME`, a
/// folder that is not the user's own) or said a path that cannot be offered.
#[must_use]
pub fn prepare_stage(channel: &dyn Channel, name: &str) -> Option<crate::stage::Folder> {
    let token = crate::stage::hex(&bt_core::pencere::system_random()?);
    let script = stage_folder_script(name, &token)?;
    let reply = channel.run("sh -s", script.as_bytes()).ok()?;
    let path = stage_folder_path(&reply)?;
    crate::stage::Folder::new(std::path::PathBuf::from(path), token)
}

/// Which way a file crosses a pipe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cross {
    /// The Mac's file goes in, and the program reads it out (`stage`).
    ToProgram,
    /// The program writes it in, and the Mac reads it out (`fetch`).
    FromProgram,
}

/// The script that makes the pipe `name` in the folder `dir`, says [`PIPE_READY`], waits for the
/// program to open its end, says [`PIPE_OPEN`], and carries the bytes: from its standard input to
/// the pipe ([`Cross::ToProgram`]) or from the pipe to its standard output
/// ([`Cross::FromProgram`]; the two words come first, and the file's bytes follow the second).
/// The pipe goes when the script does, and a script whose program never comes ends itself
/// ([`PIPE_WAIT_SECS`]) and takes the pipe with it. Its exit status is `cat`'s, or
/// [`PIPE_NOT_MADE`].
///
/// One line of `sh` in the alphabet of [`crate::ssh_wrap::is_inline`] (no single quote, backslash
/// or `!`, and double quotes are fine): it is a command that the user's login shell reads, inside
/// the single quotes of the remote command, whichever shell that is. `None` unless `dir` is a plain
/// path and `name` is the name of a pipe.
#[must_use]
pub fn pipe_script(dir: &str, name: &str, cross: Cross) -> Option<String> {
    pipe_script_waiting(dir, name, cross, PIPE_WAIT_SECS)
}

/// [`pipe_script`] that waits `wait` seconds for the program.
pub(crate) fn pipe_script_waiting(
    dir: &str,
    name: &str,
    cross: Cross,
    wait: u32,
) -> Option<String> {
    if !crate::stage::is_plain_path(dir) || !crate::stage::is_pipe_name(name) {
        return None;
    }
    let (open, carry) = match cross {
        Cross::ToProgram => ("exec 3>$f", "cat >&3"),
        Cross::FromProgram => ("exec 3<$f", "cat <&3"),
    };
    // `$f.a` is there until the program has opened its end: what tells the timer that nobody
    // came. The timer is a subshell of its own, with no output of its own, so that the
    // connection is not held open by it after the script has ended.
    Some(format!(
        "f={dir}/{name}; mkfifo -m 600 $f || exit {PIPE_NOT_MADE}; : > $f.a; \
         trap \"rm -f $f $f.a\" EXIT; trap \"exit 1\" HUP INT TERM; \
         (sleep {wait}; [ -e $f.a ] && {{ rm -f $f $f.a; kill -9 $$; }}) >/dev/null 2>&1 & \
         echo {PIPE_READY}; {open}; rm -f $f.a; echo {PIPE_OPEN}; {carry}"
    ))
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
    let root = install::ROOT;
    format!(
        "d={root}/{version}; f=$d/kablo; t=$d/.new-$$; \
         mkdir -p $d && : >$t || {{ rm -f $t; exit {EXIT_UNWRITABLE}; }}; \
         cat >$t && chmod 755 $t || {{ rm -f $t; exit {EXIT_INCOMPLETE}; }}; \
         s=$(sha256sum $t 2>/dev/null || shasum -a 256 $t 2>/dev/null); \
         case $s in {sha256}*) ;; ?*) rm -f $t; exit {EXIT_DAMAGED};; esac; \
         $t version >/dev/null 2>&1 || {{ rm -f $t; exit {EXIT_WONT_RUN}; }}; \
         if [ -x $f ]; then rm -f $t; else mv $t $f || {{ rm -f $t; exit {EXIT_UNWRITABLE}; }}; fi"
    )
}

/// The script that has the terminals `ids` renewed with the binary of `version`, which is
/// there: the relay of each is replaced by the one in that binary and the warden follows, with
/// the shell, its programs and the clients attached to it staying as they are. Only the
/// terminals [`Census::renewable`] names, never `--all`: what it says of a terminal that is not
/// the ledger's, and of one that runs a build for development, is its rule. The path is
/// the one relative to the home directory the commands start in; kablo makes it absolute
/// itself. One line of `sh`, like the others, and a failure of one terminal (it ended meanwhile)
/// is not the others': the status is 1 if any was not renewed. An id that could be read as an
/// option (`-x`) is left out. `None` when there is no terminal to renew.
pub fn upgrade_script(version: &str, ids: &[SessionId]) -> Option<String> {
    let ids: Vec<&str> = ids
        .iter()
        .map(SessionId::as_str)
        .filter(|id| !id.starts_with('-'))
        .collect();
    (!ids.is_empty()).then(|| {
        let root = install::ROOT;
        format!(
            "f={root}/{version}/kablo; s=0; for i in {}; do $f upgrade $i $f || s=1; done; exit $s",
            ids.join(" ")
        )
    })
}

/// Renews the terminals `ids` with the kablo [`ensure`] found or wrote ([`upgrade_script`]) —
/// **best effort**: a terminal that cannot be renewed keeps its relay and is served by the
/// bridge's "this release and the one before" rule. Nothing it says changes what happens next,
/// so the answer (every terminal renewed, or there were none) is for a test and a log.
pub fn upgrade_sessions(channel: &dyn Channel, installed: &Installed, ids: &[SessionId]) -> bool {
    match upgrade_script(&installed.version, ids) {
        Some(script) => channel
            .run(&script, &[])
            .is_ok_and(|reply| reply.status == Some(0)),
        None => true,
    }
}

// ─── the versions nobody runs ────────────────────────────────────────────

/// How long a version's directory has to be untouched before it may go. A directory that was
/// written a moment ago may belong to a terminal about to be made — by another build of
/// bateri on the same machine that has installed its kablo and not yet started a terminal from
/// it — and the machine's terminals say nothing of it yet.
const PRUNE_AGE_MINUTES: u32 = 10;

/// The script that says which versions of kablo are installed and which terminals run from
/// which: the directories of [`kablo_proto::install::ROOT`] and of
/// [`kablo_proto::install::FORMER_ROOT`] that hold a binary (each as its path under the home
/// directory), [`census::MARK`], then what the binary of `version` (the one that stays, in
/// `ROOT`) lists as the machine's terminals — **all** of them, this user's, whoever started
/// them. Its status is that of the listing: a listing that failed is not an empty one, and
/// [`Census::parse`] is not given one. One line of `sh`, like the others.
pub fn census_script(version: &str) -> String {
    census::script(&format!("{}/{version}/kablo list --json", install::ROOT))
}

/// The installed versions nothing runs from, given what [`census_script`] said, each as its
/// directory under the home directory (`.local/share/bateri/kablo/0.2.0`, or the same under
/// [`kablo_proto::install::FORMER_ROOT`]; one version can be in both): a version that is not
/// `keep` in `ROOT`, that no terminal on the machine runs its relay or its warden from
/// (`"version"` and `"warden_version"` of the listing), and that is a name kablo would have made
/// ([`kablo_proto::install::install_dir`]). A terminal is told apart by the version it says, not
/// by the file it runs from, so a copy in the former root stays while a terminal says its
/// version. Empty for an answer that is not a whole one — the script failed, or the listing is
/// not one — because nothing is known to be unused then.
pub fn unused_versions(reply: &Reply, keep: &str) -> Vec<String> {
    if reply.status != Some(0) {
        return Vec::new();
    }
    Census::parse(&reply.out).map_or_else(Vec::new, |census| unused(&census, keep))
}

/// [`unused_versions`] of a census that is whole.
pub fn unused(census: &Census, keep: &str) -> Vec<String> {
    census
        .dirs
        .iter()
        .filter_map(|line| {
            let (root, name) = version_dir(line)?;
            (!(root == install::ROOT && name == keep) && !census.runs(name)).then(|| line.clone())
        })
        .collect()
}

/// The root and the version of a directory under the home directory, if it is the directory of a
/// version in one of the two roots: what [`census_script`] says is what the roots hold, and this
/// reads nothing that is not a name kablo would have made.
fn version_dir(dir: &str) -> Option<(&'static str, &str)> {
    [install::ROOT, install::FORMER_ROOT]
        .into_iter()
        .find_map(|root| {
            let name = dir.strip_prefix(root)?.strip_prefix('/')?;
            install::install_dir(name).ok()?;
            Some((root, name))
        })
}

/// The script that removes the version directories `dirs` (each one [`unused_versions`] gave, a
/// directory that can only be one version's directly under a root) that were not written in the
/// last [`PRUNE_AGE_MINUTES`] minutes, and the former root itself when that leaves it empty
/// (`rmdir` leaves one that holds anything). One line of `sh`, like the others.
pub fn prune_script(dirs: &[String]) -> Option<String> {
    let dirs: Vec<&str> = dirs
        .iter()
        .map(String::as_str)
        .filter(|dir| version_dir(dir).is_some())
        .collect();
    let former = install::FORMER_ROOT;
    let in_former = dirs
        .iter()
        .any(|dir| version_dir(dir).is_some_and(|(root, _)| root == former));
    let tidy = if in_former {
        format!(" rmdir {former} 2>/dev/null;")
    } else {
        String::new()
    };
    (!dirs.is_empty()).then(|| {
        format!(
            "for v in {}; do [ -z \"$(find $v -maxdepth 0 -mmin -{PRUNE_AGE_MINUTES})\" ] && rm -rf $v; done;{tidy} :",
            dirs.join(" ")
        )
    })
}

/// Removes the versions of kablo nothing runs from, once the machine has the one in `installed`
/// and its terminals were renewed with it: after an update the previous release's directory
/// would otherwise stay for good, and so would the former root's. **Best effort**, and it removes
/// nothing it has not been told is unused by a whole answer. The same rule on this Mac and on a
/// server; the answer is the directories that were asked to go, for a test and a log.
pub fn prune_versions(channel: &dyn Channel, installed: &Installed) -> Vec<String> {
    match look_at(channel, &installed.version) {
        Some(census) => prune_unused(channel, &census, &installed.version),
        None => Vec::new(),
    }
}

/// The machine as it is now ([`census_script`]), when the answer is a whole one.
fn look_at(channel: &dyn Channel, version: &str) -> Option<Census> {
    census::look(channel, &census_script(version))
}

/// Asks the machine to remove [`unused`] of `census`, and says which.
fn prune_unused(channel: &dyn Channel, census: &Census, keep: &str) -> Vec<String> {
    let unused = unused(census, keep);
    if let Some(script) = prune_script(&unused) {
        let _ = channel.run(&script, &[]);
    }
    unused
}

/// What [`tidy`] asked of the machine and what it saw of it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tidy {
    /// The terminals it had renewed.
    pub renewed: Vec<SessionId>,
    /// The version directories it had removed.
    pub pruned: Vec<String>,
    /// The last whole look it had at the machine; none when it had none.
    pub census: Option<Census>,
}

/// What every connection to a machine does for its kablo, whether or not the binary was just
/// written: the terminals that run from an earlier release are renewed with the one in `installed`
/// ([`Census::renewable`]; `ledger` is the terminals this application keeps there), and then the
/// versions nothing runs from any more are removed ([`prune_unused`]).
///
/// The machine is looked at once, by the call that looked for the binary (`installed.census`); a
/// call of its own is made only when that did not (the binary was just written), and again when
/// terminals were renewed, since what removal goes by is what runs now. A machine that has
/// nothing to renew and nothing to remove is asked for nothing at all. **Best effort**: an answer
/// that is not a whole one renews and removes nothing, and nothing it does is told to the user.
/// Terminals are only renewed, never ended.
pub fn tidy(channel: &dyn Channel, installed: &Installed, ledger: &[SessionId]) -> Tidy {
    let mut census = installed
        .census
        .clone()
        .or_else(|| look_at(channel, &installed.version));
    let mut renewed = Vec::new();
    if let Some(seen) = &census {
        renewed = seen.renewable(ledger, &installed.version);
        if !renewed.is_empty() {
            upgrade_sessions(channel, installed, &renewed);
            if let Some(again) = look_at(channel, &installed.version) {
                census = Some(again);
            }
        }
    }
    let pruned = census.as_ref().map_or_else(Vec::new, |seen| {
        prune_unused(channel, seen, &installed.version)
    });
    Tidy {
        renewed,
        pruned,
        census,
    }
}

// ─── the number of a terminal ────────────────────────────────────────────

/// The number of the next terminal on the server (`7`: the `p7` an agent there is told), from
/// kablo's own counter (`kablo bateri --next-number`, the one the agent's command numbers with;
/// `kablo` is where kablo is, relative to the home: [`Installed::path`]): a number is never given
/// twice, whoever asks, and one that is lost with a caller that died is not given again. `None`
/// when the server could not count — no home, a directory it cannot write, a counter that is not
/// a number, a kablo that is not there, a connection that broke — and the terminal then keeps the
/// name its holder made.
pub fn next_number(channel: &dyn Channel, kablo: &str) -> Option<u64> {
    if !crate::ssh_wrap::is_kablo_path(kablo) {
        return None;
    }
    let reply = channel
        .run(&format!("exec \"$HOME/{kablo}\" bateri --next-number"), &[])
        .ok()?;
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

    fn put(&self, script: &str, input: &[u8], watch: &dyn Watch) -> io::Result<Reply> {
        let mut argv = self.argv.clone();
        argv.push(format!("sh -c {}", sq(script)));
        run_command_watched(&argv, input, self.limit, watch)
    }
}

/// Runs `argv`, writes `input` to it, collects what it says; ends it when it
/// takes longer than `limit`.
pub fn run_command(argv: &[String], input: &[u8], limit: Duration) -> io::Result<Reply> {
    run_command_in(argv, input, limit, None)
}

/// [`run_command`] in the directory `dir` (this process's own when `None`), which is also the
/// `HOME` the step sees.
pub fn run_command_in(
    argv: &[String],
    input: &[u8],
    limit: Duration,
    dir: Option<&Path>,
) -> io::Result<Reply> {
    run_command_with(argv, input, limit, dir, &Unwatched)
}

/// [`run_command`] for an input a person waits for ([`Watch`]): written in chunks, each told,
/// and ended — the whole group, the proxy too ([`crate::program::end_group`]) — as soon as the
/// watch says stop.
pub fn run_command_watched(
    argv: &[String],
    input: &[u8],
    limit: Duration,
    watch: &dyn Watch,
) -> io::Result<Reply> {
    run_command_with(argv, input, limit, None, watch)
}

/// How much of the input is handed to the connection at a time, and so how often a [`Watch`] is
/// told. Small enough for a bar to move, large enough not to be a call per kilobyte.
const PUT_CHUNK: usize = 32 * 1024;

fn run_command_with(
    argv: &[String],
    input: &[u8],
    limit: Duration,
    dir: Option<&Path>,
    watch: &dyn Watch,
) -> io::Result<Reply> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty argv"))?;
    let mut command = crate::ssh_route::command(program);
    command.args(args);
    if let Some(dir) = dir {
        // The directory is the home of whoever the step is for: what it writes below `$HOME`
        // (the counter's state) goes where its relative paths do.
        command.current_dir(dir).env("HOME", dir);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
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
    // The writer is not waited for when a step is given up on (something the step started may hold
    // its pipe, and the write then never ends): it counts what it has handed over, and the loop
    // below, which waits anyway, is the one that tells the watch.
    let data = input.to_vec();
    let total = data.len() as u64;
    let handed = Arc::new(AtomicU64::new(0));
    let writer = std::thread::spawn({
        let handed = Arc::clone(&handed);
        move || {
            if let Some(stdin) = stdin.as_mut() {
                for chunk in data.chunks(PUT_CHUNK) {
                    if stdin.write_all(chunk).is_err() {
                        break;
                    }
                    handed.fetch_add(chunk.len() as u64, Ordering::SeqCst);
                }
            }
            drop(stdin);
        }
    });
    let mut told = 0u64;
    let tell = |told: &mut u64| {
        let now = handed.load(Ordering::SeqCst);
        if now != *told {
            *told = now;
            watch.sent(now, total);
        }
    };
    let deadline = Instant::now() + limit;
    let mut stopped = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(error) => {
                crate::program::end_group(&mut child);
                return Err(error);
            }
        }
        tell(&mut told);
        stopped = stopped || watch.stopped();
        if stopped || Instant::now() >= deadline {
            crate::program::end_group(&mut child);
            break None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    // A step that ran out of time, or was stopped, is not waited for further: something it
    // started may hold its pipes, and the readers end when that does.
    let Some(status) = status else {
        let (kind, text) = if stopped {
            (io::ErrorKind::Interrupted, "stopped")
        } else {
            (io::ErrorKind::TimedOut, "the step took too long")
        };
        return Err(io::Error::new(kind, text));
    };
    let _ = writer.join();
    tell(&mut told);
    let out = out.join().unwrap_or_default();
    let err = err.join().unwrap_or_default();
    Ok(Reply {
        status: status.code(),
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
    })
}

/// What a program the server ran over the connection kept to it ([`RemoteCommand`]) came to, as
/// a step's [`Reply`]: `input` is written to its standard input, the first `said` bytes of what
/// it writes to each of its outputs are kept (the rest is read and dropped, so that the program
/// is not held back), and its end is waited for — for `limit` at most when there is one, after
/// which the program is ended and the step is an error, as an `ssh` that took too long is
/// ([`run_command`]).
pub fn reply_of(
    command: RemoteCommand,
    input: &[u8],
    said: u64,
    limit: Option<Duration>,
) -> io::Result<Reply> {
    let RemoteCommand {
        stdin,
        stdout,
        stderr,
        end,
    } = command;
    let data = input.to_vec();
    let writer = std::thread::spawn(move || {
        if let Some(mut stdin) = stdin {
            let _ = stdin.write_all(&data);
        }
    });
    let kept = |mut from: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let _ = from.by_ref().take(said).read_to_end(&mut kept);
            let _ = io::copy(&mut from, &mut io::sink());
            String::from_utf8_lossy(&kept).into_owned()
        })
    };
    let out = kept(Box::new(stdout));
    let err = kept(Box::new(stderr));
    // A step that ran out of time is not waited for further: its readers end with the stream,
    // which ending the program closes.
    let exit = match limit {
        Some(limit) => end.wait_within(limit),
        None => end.wait(),
    }
    .map_err(io::Error::other)?;
    let _ = writer.join();
    let status = match exit {
        Exit::Code(code) => i32::from(u8::try_from(code & 0xff).unwrap_or(255)),
        // Ended by a signal: not an exit status of the script.
        Exit::Signal(_) => return Err(io::Error::other("the step was ended by a signal")),
    };
    Ok(Reply {
        status: Some(status),
        out: out.join().unwrap_or_default(),
        err: err.join().unwrap_or_default(),
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
    for block in message.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
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

pub mod remove;

#[cfg(test)]
mod tests;
