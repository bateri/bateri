//! The ledger of the terminals bateri keeps on servers: what has to outlive the
//! process.
//!
//! A pane's `ssh-hold` asks the running bateri for a terminal **by the name of its
//! account** (`kablo::hub`, [`crate::hold::wire::Target::Resume`]); the account is
//! the server's key, and what it stands for — where the server is, how the user
//! reaches it, which kablo is on it — lives only in the bateri that made the
//! first terminal there. When bateri is another process (an update carried the
//! panes over, a crash left them with a holder, the Mac restarted and the
//! restored panes ask again) that bateri has to be told, or the terminal that is
//! still running on the server cannot be shown: the holder would end with a line.
//! This file is the telling. It holds three kinds of rows:
//!
//! - **an account** (`A`): the key, the server as the user typed it, the `ssh`
//!   arguments that reach it, and the platform and version of the kablo put there
//!   — [`Known`], enough to start a client without asking the user anything. It
//!   exists as long as one of its terminals is meant to live;
//! - **a terminal** (`T … live`): a terminal bateri started, with the pane that
//!   shows it; it is written when the request is made (a crash between the request
//!   and the server's answer must not leave a terminal nobody can name) and
//!   dropped when the terminal ends, when the server says it is not there, or
//!   when another computer takes it;
//! - **a terminal to end** (`T … ending`): one that was asked to end and has not
//!   been seen to. A connection that is down cannot carry the request, and a quit
//!   does not wait for a server that does not answer: the row waits for the next
//!   connection to the account, which ends the terminal before anything else
//!   ([`Ledger::ending`]). It is also what `keep_running = "update"` makes of every
//!   live terminal after a crash.
//!
//! **What is not here.** No secret: the arguments are the ones the session
//! record already keeps for a remote pane (`restore`'s `remote_line`), a password
//! lives in the Keychain, and a login is asked again when the connection needs it.
//! Not `ssh -G`'s dump either ([`KabloHost::config`]): nothing reads it after the
//! account was decided, so a [`Known`] server has it empty.
//!
//! **The file** is `{session directory}/kablo` ([`crate::restore::ledger_path`]):
//! the session directory is named by the bundle, so a development build and the
//! installed app never see each other's terminals — a ledger they shared would have
//! one end the other's — and the sweeps of the layout leave it alone. Its text is
//! line based like the layout's: the first line names the version ([`HEADER`] +
//! [`VERSION`]), then `A {key} {host} {port|-} {target} {version} {argument}…` and
//! `T {account} {terminal} {pane|-} {live|ending}`, text fields escaped as the
//! layout's are. Every version the writer still writes is read, and a version's
//! text stays frozen in the tests; a file that is not understood is an empty ledger
//! (a newer bateri's after a downgrade is dropped, not misread). Every change is
//! written through a temporary name and a `rename` (a half-written file is never
//! read), and the file — not a copy in memory — is the truth: each call reads it
//! again. A change is a read, an edit and a rewrite of the whole file, so two
//! processes of one bundle at the same moment can lose one another's change; the
//! second bateri of a bundle does not restore or save windows, and that is the
//! only way two meet, so the case is a known limit, not one guarded against.
//! Without a path ([`Ledger::memory`]: a timed run, a process with no bundle) the
//! same rows live in memory and nothing touches the disk.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bt_core::PaneUuid;
use kablo_proto::SessionId;
use kablo_proto::install::{self, Target};

use super::install::Installed;
use crate::restore::{escape, unescape, write_atomic};
use crate::ssh_wrap::KabloHost;

/// The first line's word; the version follows it after one space.
pub const HEADER: &str = "bateri-kablo";

/// The format's current version.
pub const VERSION: u32 = 1;

/// A server as the ledger remembers it: what [`Ledger::remember`] was told when the
/// account was made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Known {
    /// The account's key ([`crate::ssh_wrap::host_key`]).
    pub key: String,
    /// The server as the user typed it (`rasp`).
    pub host: String,
    /// The port the arguments name.
    pub port: Option<u16>,
    /// The `ssh` arguments that reach the server, the program's name first.
    pub argv: Vec<String>,
    /// The platform kablo was put on.
    pub target: Target,
    /// The version of the kablo there.
    pub version: String,
}

impl Known {
    /// The server, as the service starts a client for it. `config` is empty: it is
    /// decided once, when the account is made, and nothing reads it afterwards.
    pub fn server(&self) -> KabloHost {
        KabloHost {
            host: self.host.clone(),
            port: self.port,
            argv: self.argv.clone(),
            config: String::new(),
            key: self.key.clone(),
        }
    }

    /// The kablo that is on the server, as installing it left it.
    pub fn installed(&self) -> Installed {
        Installed {
            target: self.target,
            version: self.version.clone(),
            path: install::install_path(&self.version).unwrap_or_default(),
            fresh: false,
        }
    }
}

/// A terminal of the ledger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The account that holds it.
    pub account: String,
    /// Its name on the server.
    pub terminal: SessionId,
    /// The pane that showed it; none for a terminal that is only to be ended.
    pub pane: Option<PaneUuid>,
    /// Asked to end and not seen to have: the next connection ends it.
    pub ending: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Rows {
    accounts: BTreeMap<String, Known>,
    terminals: Vec<Row>,
}

impl Rows {
    fn position(&self, account: &str, terminal: &SessionId) -> Option<usize> {
        self.terminals
            .iter()
            .position(|row| row.account == account && row.terminal == *terminal)
    }

    /// An account with no terminal meant to live is forgotten: it is only there to
    /// show them again. Called where a terminal stops being one, never where an account
    /// is made (its first terminal comes a moment after it).
    fn forget_idle_accounts(&mut self) {
        let terminals = &self.terminals;
        self.accounts.retain(|key, _| {
            terminals
                .iter()
                .any(|row| !row.ending && row.account == *key)
        });
    }

    fn render(&self) -> String {
        let mut out = format!("{HEADER} {VERSION}\n");
        for known in self.accounts.values() {
            out.push_str(&format!(
                "A {} {} {} {} {}",
                escape(&known.key),
                escape(&known.host),
                known.port.map_or("-".to_owned(), |port| port.to_string()),
                known.target.triple(),
                escape(&known.version),
            ));
            for arg in &known.argv {
                out.push(' ');
                out.push_str(&escape(arg));
            }
            out.push('\n');
        }
        for row in &self.terminals {
            out.push_str(&format!(
                "T {} {} {} {}\n",
                escape(&row.account),
                row.terminal.as_str(),
                row.pane.as_ref().map_or("-", PaneUuid::as_str),
                if row.ending { "ending" } else { "live" },
            ));
        }
        out
    }

    /// `None` for text that is not exactly understood.
    fn parse(text: &str) -> Option<Rows> {
        let mut lines = text.split('\n');
        let mut header = lines.next()?.split(' ');
        if header.next()? != HEADER
            || header.next()?.parse::<u32>().ok()? != VERSION
            || header.next().is_some()
        {
            return None;
        }
        let mut rows = Rows::default();
        let mut ended = false;
        for line in lines {
            // Only the final line break may leave an empty line.
            if ended {
                return None;
            }
            if line.is_empty() {
                ended = true;
                continue;
            }
            let mut tokens = line.split(' ');
            match tokens.next()? {
                "A" => {
                    let known = parse_account(&mut tokens)?;
                    if rows.accounts.insert(known.key.clone(), known).is_some() {
                        return None;
                    }
                }
                "T" => {
                    let row = parse_terminal(&mut tokens)?;
                    if rows.position(&row.account, &row.terminal).is_some() {
                        return None;
                    }
                    rows.terminals.push(row);
                }
                _ => return None,
            }
            if tokens.next().is_some() {
                return None;
            }
        }
        ended.then_some(rows)
    }
}

/// An `A` line's fields after the letter; the arguments run to the end of the line.
fn parse_account<'a>(tokens: &mut impl Iterator<Item = &'a str>) -> Option<Known> {
    let key = unescape(tokens.next()?)?;
    let host = unescape(tokens.next()?)?;
    let port = match tokens.next()? {
        "-" => None,
        port => Some(port.parse::<u16>().ok()?),
    };
    let triple = tokens.next()?;
    let target = Target::ALL
        .into_iter()
        .find(|target| target.triple() == triple)?;
    let version = unescape(tokens.next()?)?;
    install::install_path(&version).ok()?;
    let argv = tokens.map(unescape).collect::<Option<Vec<String>>>()?;
    (!key.is_empty() && !host.is_empty() && !argv.is_empty()).then_some(Known {
        key,
        host,
        port,
        argv,
        target,
        version,
    })
}

/// A `T` line's fields after the letter.
fn parse_terminal<'a>(tokens: &mut impl Iterator<Item = &'a str>) -> Option<Row> {
    let account = unescape(tokens.next()?)?;
    let terminal = SessionId::new(tokens.next()?).ok()?;
    let pane = match tokens.next()? {
        "-" => None,
        pane => Some(PaneUuid::parse(pane)?),
    };
    let ending = match tokens.next()? {
        "live" => false,
        "ending" => true,
        _ => return None,
    };
    (!account.is_empty()).then_some(Row {
        account,
        terminal,
        pane,
        ending,
    })
}

/// The ledger. See the module documentation.
pub struct Ledger {
    path: Option<PathBuf>,
    /// Serializes the calls of this process and, without a path, is the ledger.
    rows: Mutex<Rows>,
}

impl Ledger {
    /// A ledger kept in `path`; the file need not exist yet.
    pub fn open(path: PathBuf) -> Ledger {
        Ledger {
            path: Some(path),
            rows: Mutex::new(Rows::default()),
        }
    }

    /// A ledger that lives in memory only.
    pub fn memory() -> Ledger {
        Ledger {
            path: None,
            rows: Mutex::new(Rows::default()),
        }
    }

    /// Where the ledger is written, if it is.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn read<T>(&self, ask: impl FnOnce(&Rows) -> T) -> T {
        let mut rows = self.rows.lock().unwrap_or_else(|e| e.into_inner());
        self.load(&mut rows);
        ask(&rows)
    }

    /// Applies `change` to what the file says and writes the result when it
    /// changed anything (`true`). A write that fails is not an error the caller can
    /// act on: the next change writes the whole file again.
    fn change<T>(&self, change: impl FnOnce(&mut Rows) -> (bool, T)) -> T {
        let mut rows = self.rows.lock().unwrap_or_else(|e| e.into_inner());
        self.load(&mut rows);
        let (changed, out) = change(&mut rows);
        if changed {
            if let Some(path) = &self.path {
                let _ = save(path, &rows);
            }
        }
        out
    }

    fn load(&self, rows: &mut Rows) {
        if let Some(path) = &self.path {
            *rows = std::fs::read_to_string(path)
                .ok()
                .and_then(|text| Rows::parse(&text))
                .unwrap_or_default();
        }
    }

    /// The server of an account, if the ledger remembers it.
    pub fn known(&self, account: &str) -> Option<Known> {
        self.read(|rows| rows.accounts.get(account).cloned())
    }

    /// Remembers the server an account was made for and the kablo that is on it
    /// (again, when the kablo was renewed).
    pub fn remember(&self, server: &KabloHost, installed: &Installed) {
        let known = Known {
            key: server.key.clone(),
            host: server.host.clone(),
            port: server.port,
            argv: server.argv.clone(),
            target: installed.target,
            version: installed.version.clone(),
        };
        self.change(|rows| {
            let same = rows.accounts.get(&known.key) == Some(&known);
            if !same {
                rows.accounts.insert(known.key.clone(), known);
            }
            (!same, ())
        });
    }

    /// A terminal bateri started (or took up again) for `pane`. One already marked
    /// to end stays so.
    pub fn shown(&self, account: &str, terminal: &SessionId, pane: Option<&PaneUuid>) {
        self.change(|rows| {
            match rows.position(account, terminal) {
                Some(at) => {
                    let row = &mut rows.terminals[at];
                    if row.pane.as_ref() == pane || pane.is_none() {
                        return (false, ());
                    }
                    row.pane = pane.cloned();
                }
                None => rows.terminals.push(Row {
                    account: account.to_owned(),
                    terminal: terminal.clone(),
                    pane: pane.cloned(),
                    ending: false,
                }),
            }
            (true, ())
        });
    }

    /// The terminal was asked to end: the row stays until it is seen to have.
    pub fn end(&self, account: &str, terminal: &SessionId) {
        self.change(|rows| {
            match rows.position(account, terminal) {
                Some(at) if rows.terminals[at].ending => return (false, ()),
                Some(at) => rows.terminals[at].ending = true,
                None => rows.terminals.push(Row {
                    account: account.to_owned(),
                    terminal: terminal.clone(),
                    pane: None,
                    ending: true,
                }),
            }
            rows.forget_idle_accounts();
            (true, ())
        });
    }

    /// The terminal is not there any more (it ended, the server does not have it, another
    /// computer took it): its row goes.
    pub fn gone(&self, account: &str, terminal: &SessionId) {
        self.change(|rows| {
            let Some(at) = rows.position(account, terminal) else {
                return (false, ());
            };
            rows.terminals.remove(at);
            rows.forget_idle_accounts();
            (true, ())
        });
    }

    /// Whether a terminal is meant to live and its account is known: a pane that
    /// shows it can be started again.
    pub fn keeps(&self, account: &str, terminal: &SessionId) -> bool {
        self.read(|rows| {
            rows.accounts.contains_key(account)
                && rows
                    .position(account, terminal)
                    .is_some_and(|at| !rows.terminals[at].ending)
        })
    }

    /// The terminals of an account that are to be ended.
    pub fn ending(&self, account: &str) -> Vec<SessionId> {
        self.read(|rows| {
            rows.terminals
                .iter()
                .filter(|row| row.ending && row.account == account)
                .map(|row| row.terminal.clone())
                .collect()
        })
    }

    /// Every terminal meant to live is marked to end (`keep_running = "update"`
    /// after a launch that carried nothing over): how many there were.
    pub fn end_all(&self) -> usize {
        self.change(|rows| {
            let mut count = 0;
            for row in rows.terminals.iter_mut().filter(|row| !row.ending) {
                row.ending = true;
                count += 1;
            }
            rows.forget_idle_accounts();
            (count > 0, count)
        })
    }

    /// The terminals the ledger has, in the order they were started.
    pub fn terminals(&self) -> Vec<Row> {
        self.read(|rows| rows.terminals.clone())
    }
}

/// Writes the ledger, creating its directory (private) if it is not there yet.
fn save(path: &Path, rows: &Rows) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    write_atomic(path, rows.render().as_bytes())
}

#[cfg(test)]
mod tests;
