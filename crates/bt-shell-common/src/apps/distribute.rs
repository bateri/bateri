//! The programs on every machine: what [`super::check`] landed in the apps folder on this computer is
//! put on this computer and on each server a terminal connects to, the way kablo is.
//!
//! **The layout is the same everywhere**, below the home directory:
//!
//! ```text
//! .local/share/bateri/apps/
//!   bin/<name>               a symbolic link to ../<name>/<version>/<name>: the release in use
//!   <name>/<version>/        a release, whole
//!     <name>                 the program, for the machine's target
//!     app.json               the release as it was signed
//! ```
//!
//! `bin` is on the shells' `PATH` at the end (`assets/shell`), so a program is a command on the
//! machine and never stands in for one the machine had. It is `~/.local/share` whatever
//! `XDG_DATA_HOME` says, like kablo's own place and the shells' files: the shell that reads the
//! `PATH` and the call that writes the folder do not share an environment.
//!
//! **There is no record of what was sent: the machine's disk is the truth.** One call asks what
//! `bin/<name>` points to and what versions are there; the answer is compared here, with
//! [`Version`]s, and only a release newer than the one in use is sent. The link is switched in
//! one step (a temporary link and `mv -f`) and **never goes back**: two computers that follow the same
//! program leave each other's newer release alone, and a link that is not the one a computer would
//! write (a command of the person's own at `bin/<name>`) is not touched.
//!
//! **Cleanup is in the same visit** and only of what this computer follows or removed: the release in
//! use, the one before it, and every folder newer than 30 days stay; the rest of that program's
//! releases go (a second computer may use the one before). A program this computer took off the list in
//! the last 30 days ([`store::removed`]) is deleted from the machine, folder and link.
//! Programs it has never heard of are none of its business: another computer's, or another copy of
//! bateri's (`make dev` shares this folder with the installed one).
//!
//! **Nothing waits for it.** [`Distributor::sync_in_background`] runs on a thread of its own
//! after the connection is made; an error is a line in the log and a [`Report`] for the Apps page,
//! never a refusal of the terminal.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, SystemTime};

use bt_core::pencere::Digest;
use bt_core::pencere::apps::{Version, is_name};

use super::check::{Program, fetch_target};
use super::fetch::Fetch;
use super::store::{self, KEEP};
use crate::kablo::install::{Channel, Reply};

/// The apps folder on a machine, relative to its home directory (where every call starts).
const ROOT: &str = ".local/share/bateri/apps";

/// How long one call to a machine may take: the program of an app is some megabytes, and the
/// connection may be slow.
pub const LIMIT: Duration = Duration::from_secs(300);

/// The least time between two visits to a machine when nothing changed here: a person who opens
/// ten panes on a server makes one.
const SPACING: Duration = Duration::from_secs(10 * 60);

/// The exit codes of the scripts below that say what went wrong.
const EXIT_DAMAGED: i32 = 7;
const EXIT_INCOMPLETE: i32 = 8;
const EXIT_UNWRITABLE: i32 = 9;
/// The link was not what the computer saw a moment ago: someone else moved it, and it is left.
const EXIT_MOVED: i32 = 11;

/// A program of this computer's apps folder, as far as a machine needs to know: the release in use
/// here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Want {
    /// The program's name.
    pub name: String,
    /// The newest release that landed.
    pub version: Version,
    /// Its folder on this computer (`app.json` is in it).
    pub dir: PathBuf,
}

/// What one visit to a machine did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    /// The programs put on the machine now, with their versions.
    pub shipped: Vec<(String, Version)>,
    /// Releases deleted by the cleanup, as `name/version`.
    pub pruned: Vec<String>,
    /// The programs of the removed list that were found there and deleted.
    pub removed: Vec<String>,
    /// What could not be done, one line for each program: the rest of the visit went on.
    pub problems: Vec<String>,
}

/// The last visit to a machine, for the Apps page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The machine, as the service names it.
    pub machine: String,
    /// When the visit ended.
    pub at: SystemTime,
    /// What it did, or why it could not begin (the machine did not answer).
    pub result: Result<Summary, String>,
}

/// What this computer has, which tells whether a visit made a minute ago would be the same.
type Fingerprint = (Vec<(String, Version)>, Vec<String>);

#[derive(Default)]
struct Machine {
    running: bool,
    last: Option<(SystemTime, Fingerprint)>,
    report: Option<Report>,
}

/// Puts the programs of this computer's apps folder on machines. One for the process: it remembers
/// which machine is being visited and when it was last.
pub struct Distributor {
    root: PathBuf,
    fetch: Arc<dyn Fetch>,
    machines: Mutex<HashMap<String, Machine>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Distributor {
    /// A distributor of the programs in `root` (the apps folder), which fetches the programs of
    /// other targets with `fetch` when a machine of another kind needs them.
    pub fn new(root: PathBuf, fetch: Arc<dyn Fetch>) -> Self {
        Self {
            root,
            fetch,
            machines: Mutex::new(HashMap::new()),
        }
    }

    /// The last visit to each machine, by machine.
    pub fn reports(&self) -> Vec<Report> {
        let mut reports: Vec<Report> = lock(&self.machines)
            .values()
            .filter_map(|machine| machine.report.clone())
            .collect();
        reports.sort_by(|a, b| a.machine.cmp(&b.machine));
        reports
    }

    /// The programs that landed here, each with its newest release.
    fn wanted(&self) -> Vec<Want> {
        store::followed_names(&self.root)
            .into_iter()
            .filter(|(name, _)| is_name(name))
            .filter_map(|(name, _)| {
                let version = store::newest(&self.root, &name)?;
                let dir = store::release_dir(&self.root, &name, version);
                Some(Want { name, version, dir })
            })
            .collect()
    }

    /// The names taken off the list in the last 30 days that are not on it again.
    fn gone(&self, wanted: &[Want], now: SystemTime) -> Vec<String> {
        store::removed(&self.root, now)
            .into_iter()
            .map(|entry| entry.name)
            .filter(|name| is_name(name) && !wanted.iter().any(|want| want.name == *name))
            .filter(|name| store::followed(&self.root, name).is_none())
            .collect()
    }

    /// A visit to `machine` (whose programs are made for `target`) on a thread of its own, unless
    /// one is under way or nothing changed since the last. Never blocks and never fails the caller.
    pub fn sync_in_background<C: Channel + Send + 'static>(
        self: &Arc<Self>,
        machine: String,
        target: &'static str,
        channel: C,
    ) {
        let me = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name("bateri apps".into())
            .spawn(move || me.sync_unless_recent(&machine, target, &channel, SystemTime::now()));
        if let Err(e) = spawned {
            eprintln!("bateri: could not start putting programs on a machine: {e}");
        }
    }

    /// A visit to `machine`, here and now, unless one is under way or nothing changed since the
    /// last. What it did goes to the machine's [`Report`] and, if it failed, to the log.
    pub fn sync_unless_recent(
        &self,
        machine: &str,
        target: &str,
        channel: &dyn Channel,
        now: SystemTime,
    ) {
        let wanted = self.wanted();
        let gone = self.gone(&wanted, now);
        let fingerprint: Fingerprint = (
            wanted.iter().map(|w| (w.name.clone(), w.version)).collect(),
            gone.clone(),
        );
        {
            let mut machines = lock(&self.machines);
            let state = machines.entry(machine.to_owned()).or_default();
            let recent = state.last.as_ref().is_some_and(|(at, before)| {
                *before == fingerprint
                    && now
                        .duration_since(*at)
                        .is_ok_and(|elapsed| elapsed < SPACING)
            });
            if state.running || recent {
                return;
            }
            state.running = true;
        }
        let result = self.visit(channel, target, &wanted, &gone);
        if let Err(why) = &result {
            eprintln!("bateri: programs on {machine}: {why}");
        } else if let Ok(summary) = &result {
            for problem in &summary.problems {
                eprintln!("bateri: programs on {machine}: {problem}");
            }
        }
        let mut machines = lock(&self.machines);
        let state = machines.entry(machine.to_owned()).or_default();
        state.running = false;
        state.last = Some((now, fingerprint));
        state.report = Some(Report {
            machine: machine.to_owned(),
            at: SystemTime::now(),
            result,
        });
    }

    /// One visit: the look, the programs that are newer than the machine's, the cleanup.
    fn visit(
        &self,
        channel: &dyn Channel,
        target: &str,
        wanted: &[Want],
        gone: &[String],
    ) -> Result<Summary, String> {
        let resolve = |want: &Want| {
            fetch_target(
                &self.root,
                &want.name,
                want.version,
                target,
                self.fetch.as_ref(),
            )
            .map_err(|failure| failure.to_string())
        };
        visit(channel, wanted, gone, &resolve)
    }
}

/// [`Distributor::visit`] with the way to a program's file for the machine's target given (a
/// test has no network).
fn visit(
    channel: &dyn Channel,
    wanted: &[Want],
    gone: &[String],
    resolve: &dyn Fn(&Want) -> Result<Option<Program>, String>,
) -> Result<Summary, String> {
    let mut summary = Summary::default();
    if wanted.is_empty() && gone.is_empty() {
        return Ok(summary);
    }
    let names: Vec<&str> = wanted.iter().map(|want| want.name.as_str()).collect();
    let reply = run(channel, &probe_script(&names, gone), b"")?;
    let probe = Probe::read(&reply.out).ok_or_else(|| {
        "the machine's answer to the look at the programs was cut short".to_owned()
    })?;

    let mut prune: Vec<(String, Version)> = Vec::new();
    for want in wanted {
        let seen = probe.apps.get(&want.name).cloned().unwrap_or_default();
        match one(channel, want, &seen, resolve) {
            Ok(Done { shipped, current }) => {
                if shipped {
                    summary.shipped.push((want.name.clone(), want.version));
                }
                if let Some(current) = current {
                    prune.extend(
                        seen.to_prune(current)
                            .into_iter()
                            .map(|version| (want.name.clone(), version)),
                    );
                }
            }
            Err(why) => summary.problems.push(format!("{}: {why}", want.name)),
        }
    }
    let stale: Vec<&String> = gone.iter().filter(|n| probe.stale.contains(*n)).collect();
    if !prune.is_empty() || !stale.is_empty() {
        let reply = run(channel, &remove_script(&prune, &stale), b"")?;
        if reply.status == Some(0) {
            summary.pruned = prune.iter().map(|(n, v)| format!("{n}/{v}")).collect();
            summary.removed = stale.into_iter().cloned().collect();
        } else {
            summary
                .problems
                .push(format!("cleanup: {}", failure_of(&reply)));
        }
    }
    Ok(summary)
}

/// What came of one program.
struct Done {
    /// The release was put on the machine now.
    shipped: bool,
    /// The release the link names now, if it names one of ours.
    current: Option<Version>,
}

/// One program: send its release if the machine's is older (or none), then switch the link.
fn one(
    channel: &dyn Channel,
    want: &Want,
    seen: &Seen,
    resolve: &dyn Fn(&Want) -> Result<Option<Program>, String>,
) -> Result<Done, String> {
    let current = seen.current(&want.name);
    if (seen.link.is_some() && current.is_none()) || seen.foreign {
        return Err(format!(
            "bin/{} is not a link bateri made; left as it is",
            want.name
        ));
    }
    // The link only ever goes forward, and a release that is there is not sent again.
    if current.is_some_and(|current| current >= want.version) {
        return Ok(Done {
            shipped: false,
            current,
        });
    }
    // A release with no file for this machine's kind gives it nothing.
    let Some(program) = resolve(want)? else {
        return Ok(Done {
            shipped: false,
            current,
        });
    };
    // This visit's own temporary place on the machine: another visit (another alias of the same
    // server, another computer) is never in the same folder.
    let part = store::unique();
    if !seen.dirs.contains(&want.version) {
        let bytes = fs::read(&program.path)
            .map_err(|e| format!("reading {}: {e}", program.path.display()))?;
        // What is sent is the file the author signed, not whatever the disk holds now.
        if Digest::of(&bytes).to_string() != program.sha256 {
            return Err("the program on this computer is not the one that was signed".to_owned());
        }
        let reply = run(
            channel,
            &upload_script(&want.name, want.version, &program.sha256, &part),
            &bytes,
        )?;
        if reply.status != Some(0) {
            return Err(format!("sending the program: {}", failure_of(&reply)));
        }
    }
    let manifest = fs::read(want.dir.join("app.json"))
        .map_err(|e| format!("reading app.json of {}: {e}", want.name))?;
    let reply = run(
        channel,
        &switch_script(
            &want.name,
            want.version,
            &program.sha256,
            &part,
            seen.link.as_deref(),
        ),
        &manifest,
    )?;
    match reply.status {
        Some(0) => Ok(Done {
            shipped: true,
            current: Some(want.version),
        }),
        Some(EXIT_MOVED) => Ok(Done {
            shipped: false,
            current,
        }),
        _ => Err(format!(
            "switching to {}: {}",
            want.version,
            failure_of(&reply)
        )),
    }
}

fn run(channel: &dyn Channel, script: &str, input: &[u8]) -> Result<Reply, String> {
    channel
        .run(script, input)
        .map_err(|error| error.to_string())
}

/// What a script that did not end well said.
fn failure_of(reply: &Reply) -> String {
    match reply.status {
        Some(EXIT_DAMAGED) => "the file arrived damaged".to_owned(),
        Some(EXIT_INCOMPLETE) => "the file did not arrive whole".to_owned(),
        Some(EXIT_UNWRITABLE) => "the machine cannot be written to there".to_owned(),
        Some(code) => {
            let said = reply.err.lines().find(|line| !line.trim().is_empty());
            match said {
                Some(line) => format!("exit {code}: {}", line.trim()),
                None => format!("exit {code}"),
            }
        }
        None => "it did not end in time".to_owned(),
    }
}

// ---- what a machine says ---------------------------------------------------------------------

/// What the look found of one program.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Seen {
    /// What `bin/<name>` is, when it is a link.
    link: Option<String>,
    /// `bin/<name>` is something that is not a link.
    foreign: bool,
    /// The releases in the program's folder.
    dirs: Vec<Version>,
    /// Those not touched in the last 30 days.
    old: Vec<Version>,
}

impl Seen {
    /// The release the link names, if it is the link a computer writes for this program.
    fn current(&self, name: &str) -> Option<Version> {
        let link = self.link.as_deref()?;
        let rest = link
            .strip_prefix("../")?
            .strip_prefix(name)?
            .strip_prefix('/')?;
        let (version, program) = rest.split_once('/')?;
        (program == name).then(|| Version::parse(version)).flatten()
    }

    /// The releases to delete once `current` is the one in use: the old ones, but not `current`
    /// and not the greatest before it.
    fn to_prune(&self, current: Version) -> Vec<Version> {
        let before = self
            .dirs
            .iter()
            .copied()
            .filter(|version| *version < current)
            .max();
        let keep: BTreeSet<Version> = [Some(current), before].into_iter().flatten().collect();
        let mut prune: Vec<Version> = self
            .old
            .iter()
            .copied()
            .filter(|version| !keep.contains(version))
            .collect();
        prune.sort();
        prune
    }
}

/// The machine's answer to [`probe_script`].
#[derive(Debug, Default, PartialEq, Eq)]
struct Probe {
    apps: HashMap<String, Seen>,
    /// The names of the removed list that are on the machine.
    stale: Vec<String>,
}

impl Probe {
    /// `None` when the answer does not end with `end`: the call was cut short, and what it
    /// said so far is not the machine's state. A line that is none of ours is a shell's noise.
    fn read(output: &str) -> Option<Self> {
        let mut probe = Self::default();
        let mut name: Option<String> = None;
        let mut ended = false;
        for line in output.lines() {
            let line = line.trim_end();
            let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
            match word {
                "app" => {
                    name = Some(rest.to_owned());
                    probe.apps.entry(rest.to_owned()).or_default();
                }
                "link" | "foreign" | "dir" | "old" => {
                    let Some(seen) = name.as_ref().and_then(|n| probe.apps.get_mut(n)) else {
                        continue;
                    };
                    match word {
                        "link" => seen.link = Some(rest.to_owned()),
                        "foreign" => seen.foreign = true,
                        "dir" => seen.dirs.extend(Version::parse(rest)),
                        _ => seen.old.extend(Version::parse(rest)),
                    }
                }
                "stale" => probe.stale.push(rest.to_owned()),
                "end" => ended = true,
                _ => {}
            }
        }
        ended.then_some(probe)
    }
}

// ---- what a machine is told ------------------------------------------------------------------
//
// The scripts are plain `sh` (a server's `sh` may be dash or BusyBox's ash), start in the home
// directory, and are given only names and versions this module has checked: a name is
// `is_name`'s (lowercase letters, digits, `-`), a version three numbers.

/// The look: for each program, `bin/<name>` and the folders it has, and which of the folders have
/// not been touched in 30 days; and which of the removed names are still there. Ends with `end`.
fn probe_script(names: &[&str], gone: &[String]) -> String {
    let apps = names.join(" ");
    let gone = gone.join(" ");
    let days = KEEP.as_secs() / (24 * 60 * 60) - 1;
    format!(
        "r={ROOT}; \
         for a in {apps}; do \
           echo \"app $a\"; \
           if [ -L $r/bin/$a ]; then echo \"link $(readlink $r/bin/$a)\"; \
           elif [ -e $r/bin/$a ]; then echo foreign; fi; \
           for d in $r/$a/[0-9]*; do [ -d \"$d\" ] && echo \"dir ${{d##*/}}\"; done; \
           find $r/$a/. ! -name . -prune -type d -mtime +{days} 2>/dev/null \
             | while read -r o; do echo \"old ${{o##*/}}\"; done; \
         done; \
         for n in {gone}; do \
           if [ -e $r/$n ] || [ -L $r/bin/$n ]; then echo \"stale $n\"; fi; \
         done; \
         echo end"
    )
}

/// The program: its bytes on the standard input, written to a temporary folder beside the
/// release's own place (`.<version>.part-<part>`, the visit's own), checked against `sum`, made
/// executable. The release is not in its place until [`switch_script`] puts it there. Folders of
/// this kind that a visit that ended badly left more than a day ago are swept away.
fn upload_script(name: &str, version: Version, sum: &str, part: &str) -> String {
    format!(
        "r={ROOT}; p=$r/{name}/.{version}.part-{part}; f=$p/{name}; \
         find $r/{name}/. ! -name . -prune -type d -name '.*.part-*' -mtime +0 \
           -exec rm -rf {{}} \\; 2>/dev/null; \
         mkdir -p $p || exit {EXIT_UNWRITABLE}; \
         cat >$f && chmod 755 $f || {{ rm -rf $p; exit {EXIT_INCOMPLETE}; }}; \
         s=$(sha256sum $f 2>/dev/null || shasum -a 256 $f 2>/dev/null); \
         case $s in {sum}*) ;; ?*) rm -rf $p; exit {EXIT_DAMAGED};; esac"
    )
}

/// The release's `app.json` on the standard input, then the release in its place and the link to
/// it, in one step and only if the link is still what the computer saw (`was`: its text, or none).
/// The program the visit put in its folder is checked against `sum` once more before the folder
/// is moved: it is what the author signed, or the release is not put in its place.
fn switch_script(name: &str, version: Version, sum: &str, part: &str, was: Option<&str>) -> String {
    let expected = was.unwrap_or("");
    // The text of a link is a path a machine made; it is compared, never run.
    let expected = expected.replace('\'', "'\\''");
    format!(
        "r={ROOT}; a={name}; v={version}; d=$r/$a/$v; p=$r/$a/.$v.part-{part}; l=$r/bin/.$a.new-$$; \
         mkdir -p $r/bin $r/$a || exit {EXIT_UNWRITABLE}; \
         if [ -d $d ]; then rm -rf $p; cat >$d/app.json || exit {EXIT_INCOMPLETE}; \
         else [ -x $p/$a ] || exit {EXIT_INCOMPLETE}; \
           s=$(sha256sum $p/$a 2>/dev/null || shasum -a 256 $p/$a 2>/dev/null); \
           case $s in {sum}*) ;; ?*) rm -rf $p; exit {EXIT_DAMAGED};; esac; \
           cat >$p/app.json || {{ rm -rf $p; exit {EXIT_INCOMPLETE}; }}; \
           mv $p $d || {{ rm -rf $p; exit {EXIT_UNWRITABLE}; }}; fi; \
         now=$(readlink $r/bin/$a 2>/dev/null); \
         if [ \"$now\" != '{expected}' ]; then exit {EXIT_MOVED}; fi; \
         if [ -z \"$now\" ] && [ -e $r/bin/$a ]; then exit {EXIT_MOVED}; fi; \
         ln -s ../$a/$v/$a $l && mv -f $l $r/bin/$a || {{ rm -f $l; exit {EXIT_UNWRITABLE}; }}"
    )
}

/// The cleanup: the releases named, and the programs of the removed list (folder and link, the
/// link only if it is one a computer wrote).
fn remove_script(releases: &[(String, Version)], gone: &[&String]) -> String {
    let mut script = format!("r={ROOT}; cd $r 2>/dev/null || exit 0; ");
    for (name, version) in releases {
        script.push_str(&format!("rm -rf {name}/{version}; "));
    }
    for name in gone {
        script.push_str(&format!(
            "case $(readlink bin/{name} 2>/dev/null) in ../{name}/*) rm -f bin/{name};; esac; \
             rm -rf {name}; "
        ));
    }
    script.push_str("exit 0");
    script
}

#[cfg(test)]
mod tests;
