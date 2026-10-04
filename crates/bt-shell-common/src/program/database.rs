//! A database client's guide bar: what the client is connected to —
//! `postgres  app@db.prod:5432/main` — read from its command line and a few
//! listed environment variables, **never its password**.
//!
//! **What can reach the bar** is a closed list: the values of the options a
//! client's table names as its server, port, user or database; the
//! arguments that are not options; the parts of a connection URI or a libpq
//! connection string that are not the password; and the listed variables
//! (`super::ENV_KEYS`), none of them a password's. A password could only be shown
//! if an option that takes one were mistaken for a flag — its value would
//! then read as an argument — so every option that carries a secret is in
//! the tables as taking a value, and each spelling has a test.
//!
//! **An option a table does not know makes the target unknown**: the bar
//! keeps the client's name and how to leave, and nothing else. Such an
//! option may name the server itself (getopt takes `--hos=db` for
//! `--host`), take the next argument as its value (an argument after it is
//! then not what it seems) or stand in for a variable; a bar naming another
//! server than the one the client talks to is worse than one naming none.
//!
//! **Pure**: argv and the listed variables in, a [`Target`] out; the client
//! is not asked anything and no file is read (a libpq service file and
//! MySQL's option files are not, so their contents are not shown).

use std::path::Path;

use super::options::{self, Dialect, Takes, row, walk};
use crate::jobs::ProcArgs;

/// A database client with a guide bar — a row of [`Client::label`] and
/// [`Client::hint`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Client {
    /// `psql`.
    Postgres,
    /// `mysql`.
    Mysql,
    /// `mariadb`: the same options as `mysql`, its own name.
    Mariadb,
    /// `sqlite3`.
    Sqlite,
    /// `redis-cli`.
    Redis,
    /// `mongosh`, a binary or a node script.
    Mongo,
}

impl Client {
    /// Every client, for the tests that walk the bar's strings.
    pub const ALL: [Self; 6] = [
        Self::Postgres,
        Self::Mysql,
        Self::Mariadb,
        Self::Sqlite,
        Self::Redis,
        Self::Mongo,
    ];

    /// The client a process name is.
    pub(super) fn of_name(name: &str) -> Option<Self> {
        match name {
            "psql" => Some(Self::Postgres),
            "mysql" => Some(Self::Mysql),
            "mariadb" => Some(Self::Mariadb),
            "sqlite3" => Some(Self::Sqlite),
            "redis-cli" => Some(Self::Redis),
            "mongosh" => Some(Self::Mongo),
            _ => None,
        }
    }

    /// The client a node script is, by its file name: Homebrew's `mongosh`
    /// is a script, its process `node /opt/homebrew/bin/mongosh …`.
    pub(super) fn of_script(name: &str) -> Option<Self> {
        matches!(name, "mongosh" | "mongosh.js").then_some(Self::Mongo)
    }

    /// The bar's title: the database, not the client's binary — a UI
    /// string.
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
            Self::Mariadb => "mariadb",
            Self::Sqlite => "sqlite",
            Self::Redis => "redis",
            Self::Mongo => "mongodb",
        }
    }

    /// How to leave its prompt — the client's own command; a UI string.
    pub(super) fn hint(self) -> &'static str {
        match self {
            Self::Postgres => "\\q to leave",
            Self::Mysql | Self::Mariadb | Self::Mongo => "exit to leave",
            Self::Sqlite => ".quit to leave",
            Self::Redis => "quit to leave",
        }
    }

    /// What the client is connected to, from its arguments after argv[0]
    /// and the listed variables; `None` when the command line is not the
    /// client's prompt (it runs a command, a file or a script and exits —
    /// `psql -c`, `redis-cli get k`, `sqlite3 x.db 'select 1'`).
    pub(super) fn target(self, args: &[String], record: &ProcArgs) -> Option<Target> {
        match self {
            Self::Postgres => postgres(args, record),
            Self::Mysql | Self::Mariadb => mysql(args, record),
            Self::Sqlite => sqlite(args),
            Self::Redis => redis(args),
            Self::Mongo => mongo(args),
        }
    }
}

/// Where a client is connected: each part as the user gave it, `None` when
/// nothing said. **No field holds a password**, and nothing in this type
/// is ever read from where a password stands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Target {
    pub user: Option<String>,
    /// The server as given: a name, an address or a list (`a:5432,b`); a
    /// socket's path (`/tmp`, which libpq takes as a host) is never shown.
    pub host: Option<String>,
    pub port: Option<String>,
    pub database: Option<String>,
    /// libpq's service: its entry in the service file names the server.
    pub service: Option<String>,
    /// libpq's `hostaddr`: the address the client connects to whatever the
    /// host says — it becomes the shown host ([`postgres`]).
    pub hostaddr: Option<String>,
    /// SQLite's database file.
    pub file: Option<String>,
}

impl Target {
    /// The host when it is a server's, not a socket's.
    fn server(&self) -> Option<&str> {
        self.host
            .as_deref()
            .filter(|host| !host.is_empty() && !host.starts_with(['/', '@']))
    }

    /// The bar's text: `user@host:port/database` with every part that is
    /// not known left out (with its separator), or SQLite's file with the
    /// home directory as `~`. An IPv6 address is bracketed, as in a URI.
    pub(super) fn shown(&self, home: Option<&Path>) -> String {
        if let Some(file) = self.file.as_deref().filter(|file| !file.is_empty()) {
            return super::tilde(Path::new(file), home);
        }
        let mut text = String::new();
        if let Some(user) = self.user.as_deref().filter(|user| !user.is_empty()) {
            text.push_str(user);
            text.push('@');
        }
        if let Some(host) = self.server() {
            if host.contains(':') && !host.contains(',') && !host.starts_with('[') {
                text.push('[');
                text.push_str(host);
                text.push(']');
            } else {
                text.push_str(host);
            }
        }
        if let Some(port) = self.port.as_deref().filter(|port| !port.is_empty()) {
            text.push(':');
            text.push_str(port);
        }
        if let Some(database) = self.database.as_deref().filter(|db| !db.is_empty()) {
            text.push('/');
            text.push_str(database);
        }
        text
    }

    /// The host the `[remote] hosts` marks are resolved against: the
    /// server's name without a port or brackets, the first of a list; no
    /// user (an ssh pattern like `root@*` is not about a database user).
    pub(super) fn mark_host(&self) -> Option<String> {
        let first = self.server()?.split(',').next()?;
        let host = match first.strip_prefix('[') {
            Some(bracketed) => bracketed.split(']').next()?,
            None if first.matches(':').count() == 1 => first.split(':').next()?,
            None => first,
        };
        (!host.is_empty()).then(|| host.to_owned())
    }

    /// Every part `other` names replaces this one's.
    fn overlay(&mut self, other: Self) {
        let Self {
            user,
            host,
            port,
            database,
            service,
            hostaddr,
            file,
        } = other;
        for (slot, value) in [
            (&mut self.user, user),
            (&mut self.host, host),
            (&mut self.port, port),
            (&mut self.database, database),
            (&mut self.service, service),
            (&mut self.hostaddr, hostaddr),
            (&mut self.file, file),
        ] {
            if value.is_some() {
                *slot = value;
            }
        }
    }
}

/// What an option's value is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Host,
    Port,
    User,
    Database,
    /// A connection URI (`redis-cli -u`).
    Uri,
    /// A socket's path: the server is local, the host not used.
    Socket,
    /// MySQL's option files: they can name the server and beat the
    /// variables.
    OptionFile,
    /// MySQL's `--no-defaults`: no option file is read.
    NoDefaults,
    /// The client runs this (a command, a file) and exits: no prompt.
    Runs,
    /// mongosh's `--shell`: the prompt after `--eval` or a file.
    Shell,
    /// mongosh's `--nodb`: a prompt connected to nothing.
    NoDb,
    /// Anything else — a format, a file, a timeout, a password.
    Other,
}

/// One option of a client's table.
type Opt = options::Opt<Role>;

/// A given name as shown. One carrying a control character shows as
/// nothing — it would be drawn as a box, and it is no name anyone typed on
/// purpose — and so does one holding a URI (`://`) or a `key=value` string:
/// a connection string where a name belongs (`psql -d main
/// 'postgresql://u:secret@h'` makes it the user) may carry a password. Either
/// still counts as given: a variable does not stand in for it, the client
/// does not read the variable either.
fn scrub(value: &str) -> String {
    if value.chars().any(char::is_control) || value.contains("://") || value.contains('=') {
        String::new()
    } else {
        value.to_owned()
    }
}

/// A URI component with its `%XX` escapes decoded (`bt_core::decode_percent`),
/// [`scrub`]bed; nothing for a broken escape or text that is not UTF-8 (the
/// client refuses such a URI before its prompt).
fn decode(text: &str) -> String {
    let mut out = Vec::with_capacity(text.len());
    match bt_core::decode_percent(text.as_bytes(), &mut out) {
        Some(()) => String::from_utf8(out).map_or_else(|_| String::new(), |text| scrub(&text)),
        None => String::new(),
    }
}

/// A URI after its scheme, split so that **no part of a password can land
/// outside the userinfo**.
struct Uri<'a> {
    /// `user:password`, or redis's bare password; never shown whole.
    userinfo: Option<&'a str>,
    /// `host:port`, `[::1]:5432` or a list.
    authority: &'a str,
    path: Option<&'a str>,
    query: Option<&'a str>,
}

impl<'a> Uri<'a> {
    /// The userinfo ends at the **last** `@`: a password with an unescaped
    /// `@` (`p@ss`) then stays inside it, where taking the first would show
    /// its tail as the host. A userinfo that then holds a `/`, `?` or `#` is
    /// ambiguous — a password with those characters, or an `@` in the path
    /// or the query — and the URI is not read at all (`None`): either
    /// guess could show a password or a wrong server.
    fn split(rest: &'a str) -> Option<Self> {
        let (userinfo, tail) = match rest.rfind('@') {
            Some(at) => (Some(&rest[..at]), &rest[at + 1..]),
            None => (None, rest),
        };
        if userinfo.is_some_and(|info| info.contains(['/', '?', '#'])) {
            return None;
        }
        let tail = tail.split('#').next().unwrap_or_default();
        let (tail, query) = match tail.split_once('?') {
            Some((tail, query)) => (tail, Some(query)),
            None => (tail, None),
        };
        let (authority, path) = match tail.split_once('/') {
            Some((authority, path)) => (authority, Some(path)),
            None => (tail, None),
        };
        Some(Self {
            userinfo,
            authority,
            path,
            query,
        })
    }

    /// The user of a `user[:password]` userinfo; the password is not
    /// looked at. **A userinfo holding an `@` names no user**: the URI had
    /// two, and the clients split at the first (`p@ss:word@db` is user `p`
    /// to libpq, password `p` to redis-cli) — the part before the `:` may be
    /// a password's head.
    fn user(&self) -> Option<String> {
        let info = self.userinfo.filter(|info| !info.contains('@'))?;
        let user = info.split(':').next()?;
        (!user.is_empty()).then(|| decode(user))
    }

    /// The path: a database's name or number.
    fn database(&self) -> Option<String> {
        self.path.filter(|path| !path.is_empty()).map(decode)
    }

    /// The host and port of the authority. A list (`a:5432,b:5433`) is the
    /// host as written, its ports inside it; a port that is not a number
    /// is not shown.
    fn server(&self) -> (Option<String>, Option<String>) {
        let authority = self.authority;
        if authority.is_empty() {
            return (None, None);
        }
        if authority.contains(',') {
            return (Some(decode(authority)), None);
        }
        let (host, port) = match authority.strip_prefix('[') {
            Some(bracketed) => match bracketed.split_once(']') {
                Some((host, after)) => (host, after.strip_prefix(':')),
                None => return (None, None),
            },
            None => match authority.split_once(':') {
                Some((host, port)) if !port.contains(':') => (host, Some(port)),
                Some(_) => (authority, None),
                None => (authority, None),
            },
        };
        let port = port
            .filter(|port| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
            .map(str::to_owned);
        ((!host.is_empty()).then(|| decode(host)), port)
    }
}

/// `text` without the scheme `scheme` (`postgres://`), ignoring case when
/// `any_case`.
fn strip_scheme<'a>(text: &'a str, scheme: &str, any_case: bool) -> Option<&'a str> {
    let head = text.get(..scheme.len())?;
    let same = if any_case {
        head.eq_ignore_ascii_case(scheme)
    } else {
        head == scheme
    };
    same.then(|| &text[scheme.len()..])
}

// --- psql ---------------------------------------------------------------

/// psql's options (`psql --help`; getopt `aAbc:d:eEf:F:h:HlL:no:p:P:qR:sStT:U:v:VwWxXz?01`).
/// `-W`/`--password` only asks for the password — a flag.
const PSQL: &[Opt] = &[
    ("-c", Takes::Value, Role::Runs),
    ("--command", Takes::Value, Role::Runs),
    ("-d", Takes::Value, Role::Database),
    ("--dbname", Takes::Value, Role::Database),
    ("-f", Takes::Value, Role::Runs),
    ("--file", Takes::Value, Role::Runs),
    ("-l", Takes::Nothing, Role::Runs),
    ("--list", Takes::Nothing, Role::Runs),
    ("-v", Takes::Value, Role::Other),
    ("--set", Takes::Value, Role::Other),
    ("--variable", Takes::Value, Role::Other),
    ("-V", Takes::Nothing, Role::Runs),
    ("--version", Takes::Nothing, Role::Runs),
    ("-X", Takes::Nothing, Role::Other),
    ("--no-psqlrc", Takes::Nothing, Role::Other),
    ("-1", Takes::Nothing, Role::Other),
    ("--single-transaction", Takes::Nothing, Role::Other),
    ("-?", Takes::Nothing, Role::Runs),
    ("--help", Takes::Attached, Role::Runs),
    ("-a", Takes::Nothing, Role::Other),
    ("--echo-all", Takes::Nothing, Role::Other),
    ("-b", Takes::Nothing, Role::Other),
    ("--echo-errors", Takes::Nothing, Role::Other),
    ("-e", Takes::Nothing, Role::Other),
    ("--echo-queries", Takes::Nothing, Role::Other),
    ("-E", Takes::Nothing, Role::Other),
    ("--echo-hidden", Takes::Nothing, Role::Other),
    ("-L", Takes::Value, Role::Other),
    ("--log-file", Takes::Value, Role::Other),
    ("-n", Takes::Nothing, Role::Other),
    ("--no-readline", Takes::Nothing, Role::Other),
    ("-o", Takes::Value, Role::Other),
    ("--output", Takes::Value, Role::Other),
    ("-q", Takes::Nothing, Role::Other),
    ("--quiet", Takes::Nothing, Role::Other),
    ("-s", Takes::Nothing, Role::Other),
    ("--single-step", Takes::Nothing, Role::Other),
    ("-S", Takes::Nothing, Role::Other),
    ("--single-line", Takes::Nothing, Role::Other),
    ("-A", Takes::Nothing, Role::Other),
    ("--no-align", Takes::Nothing, Role::Other),
    ("--csv", Takes::Nothing, Role::Other),
    ("-F", Takes::Value, Role::Other),
    ("--field-separator", Takes::Value, Role::Other),
    ("-H", Takes::Nothing, Role::Other),
    ("--html", Takes::Nothing, Role::Other),
    ("-P", Takes::Value, Role::Other),
    ("--pset", Takes::Value, Role::Other),
    ("-R", Takes::Value, Role::Other),
    ("--record-separator", Takes::Value, Role::Other),
    ("-t", Takes::Nothing, Role::Other),
    ("--tuples-only", Takes::Nothing, Role::Other),
    ("-T", Takes::Value, Role::Other),
    ("--table-attr", Takes::Value, Role::Other),
    ("-x", Takes::Nothing, Role::Other),
    ("--expanded", Takes::Nothing, Role::Other),
    ("-z", Takes::Nothing, Role::Other),
    ("--field-separator-zero", Takes::Nothing, Role::Other),
    ("-0", Takes::Nothing, Role::Other),
    ("--record-separator-zero", Takes::Nothing, Role::Other),
    ("-h", Takes::Value, Role::Host),
    ("--host", Takes::Value, Role::Host),
    ("-p", Takes::Value, Role::Port),
    ("--port", Takes::Value, Role::Port),
    ("-U", Takes::Value, Role::User),
    ("--username", Takes::Value, Role::User),
];

/// psql's flags that ask for the password or not — a flag each.
const PSQL_PASSWORD: &[Opt] = &[
    ("-w", Takes::Nothing, Role::Other),
    ("--no-password", Takes::Nothing, Role::Other),
    ("-W", Takes::Nothing, Role::Other),
    ("--password", Takes::Nothing, Role::Other),
];

fn psql_option(name: &str) -> Option<(Takes, Role)> {
    row(PSQL, name).or_else(|| row(PSQL_PASSWORD, name))
}

/// psql's target, libpq's order: the options, then a connection string in
/// the database's place **over** them (libpq reads it after the host, port
/// and user), then — unless a service names the server from its file —
/// the `PG*` variables for what is still unset. An empty value is unset, as
/// libpq takes it (`psql -h "$EMPTY"` reads `PGHOST`). A `hostaddr` (in the
/// string or `PGHOSTADDR`) is where libpq connects whatever the host says:
/// it is the host shown and marked.
fn postgres(args: &[String], record: &ProcArgs) -> Option<Target> {
    let walk = walk(args, Dialect::Getopt, psql_option);
    if walk.has(Role::Runs) {
        return None;
    }
    if walk.unknown {
        return Some(Target::default());
    }
    let (mut host, mut port, mut user, mut dbname) = (None, None, None, None);
    for &(role, value) in &walk.options {
        match role {
            Role::Host => host = value,
            Role::Port => port = value,
            Role::User => user = value,
            Role::Database => dbname = value,
            _ => {}
        }
    }
    // `psql [DBNAME [USERNAME]]`, each only where the options left it
    // unset (an empty one is set for psql); a third is ignored (psql warns).
    let mut positionals = walk.positionals.iter().copied();
    if dbname.is_none() {
        dbname = positionals.next();
    }
    if user.is_none() {
        user = positionals.next();
    }
    let given = |value: Option<&str>| value.filter(|value| !value.is_empty()).map(scrub);
    let mut target = Target {
        host: given(host),
        port: given(port),
        user: given(user),
        ..Target::default()
    };
    if let Some(dbname) = dbname.filter(|dbname| !dbname.is_empty()) {
        if is_connection_string(dbname) {
            // A string libpq cannot parse fails the connection: psql exits
            // before its prompt. Nothing from it is shown either way.
            let Some(connection) = postgres_uri(dbname).or_else(|| conninfo(dbname)) else {
                return Some(Target::default());
            };
            target.overlay(connection);
        } else {
            target.database = Some(scrub(dbname));
        }
    }
    let service = target
        .service
        .take()
        .or_else(|| given(record.var("PGSERVICE")));
    if service.is_none() {
        for (slot, key) in [
            (&mut target.host, "PGHOST"),
            (&mut target.hostaddr, "PGHOSTADDR"),
            (&mut target.port, "PGPORT"),
            (&mut target.user, "PGUSER"),
            (&mut target.database, "PGDATABASE"),
        ] {
            if slot.is_none() {
                *slot = given(record.var(key));
            }
        }
    }
    if let Some(address) = target.hostaddr.take() {
        target.host = Some(address);
    }
    target.service = service.filter(|service| !service.is_empty());
    Some(target)
}

/// libpq's test: a URI or a `key=value` string, not a database's name.
fn is_connection_string(text: &str) -> bool {
    text.starts_with("postgresql://") || text.starts_with("postgres://") || text.contains('=')
}

/// The server's keys of libpq's `key = value` connection string — `host`,
/// `hostaddr`, `port`, `user`, `dbname`, `service`, nothing else (an empty
/// value is unset, as libpq takes it); `None` when libpq
/// would refuse the string. Parsed as libpq does: a value is quoted (`'…'`)
/// or runs to the next space, a backslash escapes the next character — so a
/// quoted password (`password='x host=evil'`) is one value, skipped whole,
/// and never yields a host. The other keys' values are walked over without
/// being kept.
fn conninfo(text: &str) -> Option<Target> {
    let mut target = Target::default();
    let mut chars = text.chars().peekable();
    loop {
        while chars.next_if(char::is_ascii_whitespace).is_some() {}
        if chars.peek().is_none() {
            return Some(target);
        }
        let mut key = String::new();
        while let Some(ch) = chars.next_if(|ch| *ch != '=' && !ch.is_ascii_whitespace()) {
            key.push(ch);
        }
        while chars.next_if(char::is_ascii_whitespace).is_some() {}
        if chars.next() != Some('=') {
            return None;
        }
        while chars.next_if(char::is_ascii_whitespace).is_some() {}
        let slot = match key.as_str() {
            "host" => Some(&mut target.host),
            "hostaddr" => Some(&mut target.hostaddr),
            "port" => Some(&mut target.port),
            "user" => Some(&mut target.user),
            "dbname" => Some(&mut target.database),
            "service" => Some(&mut target.service),
            _ => None,
        };
        // Only a kept key's value is collected.
        let mut value = slot.is_some().then(String::new);
        let mut keep = |ch: char| {
            if let Some(value) = value.as_mut() {
                value.push(ch);
            }
        };
        if chars.next_if_eq(&'\'').is_some() {
            loop {
                match chars.next()? {
                    '\\' => keep(chars.next()?),
                    '\'' => break,
                    ch => keep(ch),
                }
            }
        } else {
            while let Some(ch) = chars.next() {
                if ch.is_ascii_whitespace() {
                    break;
                }
                if ch == '\\' {
                    if let Some(escaped) = chars.next() {
                        keep(escaped);
                    }
                } else {
                    keep(ch);
                }
            }
        }
        if let Some(slot) = slot {
            *slot = value
                .filter(|value| !value.is_empty())
                .as_deref()
                .map(scrub);
        }
    }
}

/// A libpq URI (`postgresql://user:password@host:port/dbname?key=value`):
/// the user, the server, the database and the query's `host`, `hostaddr`,
/// `port`, `user`, `dbname` and `service` (over the rest); the password is never
/// read. `None` for a URI that is ambiguous ([`Uri::split`]) or that libpq
/// would refuse.
fn postgres_uri(text: &str) -> Option<Target> {
    let rest = strip_scheme(text, "postgresql://", false)
        .or_else(|| strip_scheme(text, "postgres://", false))?;
    let uri = Uri::split(rest)?;
    let (host, port) = uri.server();
    let mut target = Target {
        user: uri.user(),
        host,
        port,
        database: uri.database(),
        ..Target::default()
    };
    // An empty pair (`?`, a trailing `&`) is nothing, as libpq takes it.
    let pairs = uri.query.into_iter().flat_map(|query| query.split('&'));
    for pair in pairs.filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=')?;
        let slot = match key {
            "host" => &mut target.host,
            "hostaddr" => &mut target.hostaddr,
            "port" => &mut target.port,
            "user" => &mut target.user,
            "dbname" => &mut target.database,
            "service" => &mut target.service,
            _ => continue,
        };
        *slot = (!value.is_empty()).then(|| decode(value));
    }
    Some(target)
}

// --- mysql and mariadb --------------------------------------------------

/// mysql's and mariadb's options (`mysql --help`). The password's
/// argument is **optional**: `-pSECRET` and `--password=SECRET` carry it,
/// a bare `-p` asks for it and leaves the next argument to be the database
/// — mysql's own reading, so a password typed after `-p ` would be a
/// database named so, refused before the prompt. A long boolean also takes
/// `=0`, so it is `Attached`; a short letter stays a flag, or the cluster's
/// next letters would be read as its value.
const MYSQL: &[Opt] = &[
    ("-p", Takes::Attached, Role::Other),
    ("--password", Takes::Attached, Role::Other),
    ("--password1", Takes::Attached, Role::Other),
    ("--password2", Takes::Attached, Role::Other),
    ("--password3", Takes::Attached, Role::Other),
    ("-h", Takes::Value, Role::Host),
    ("--host", Takes::Value, Role::Host),
    ("-P", Takes::Value, Role::Port),
    ("--port", Takes::Value, Role::Port),
    ("-u", Takes::Value, Role::User),
    ("--user", Takes::Value, Role::User),
    ("-D", Takes::Value, Role::Database),
    ("--database", Takes::Value, Role::Database),
    ("-S", Takes::Value, Role::Socket),
    ("--socket", Takes::Value, Role::Socket),
    ("-e", Takes::Value, Role::Runs),
    ("--execute", Takes::Value, Role::Runs),
    ("-?", Takes::Nothing, Role::Runs),
    ("-I", Takes::Nothing, Role::Runs),
    ("--help", Takes::Nothing, Role::Runs),
    ("-V", Takes::Nothing, Role::Runs),
    ("--version", Takes::Nothing, Role::Runs),
    ("--print-defaults", Takes::Nothing, Role::Runs),
    ("--defaults-file", Takes::Value, Role::OptionFile),
    ("--defaults-extra-file", Takes::Value, Role::OptionFile),
    ("--defaults-group-suffix", Takes::Value, Role::OptionFile),
    ("--login-path", Takes::Value, Role::OptionFile),
    ("--no-defaults", Takes::Nothing, Role::NoDefaults),
    ("--no-login-paths", Takes::Nothing, Role::Other),
    ("-#", Takes::Attached, Role::Other),
    ("--debug", Takes::Attached, Role::Other),
    ("--pager", Takes::Attached, Role::Other),
    ("--local-infile", Takes::Attached, Role::Other),
    ("-A", Takes::Nothing, Role::Other),
    ("--no-auto-rehash", Takes::Nothing, Role::Other),
    ("--auto-rehash", Takes::Attached, Role::Other),
    ("--auto-vertical-output", Takes::Attached, Role::Other),
    ("-B", Takes::Nothing, Role::Other),
    ("--batch", Takes::Attached, Role::Other),
    ("-b", Takes::Nothing, Role::Other),
    ("--no-beep", Takes::Attached, Role::Other),
    ("--binary-as-hex", Takes::Attached, Role::Other),
    ("--binary-mode", Takes::Attached, Role::Other),
    ("--bind-address", Takes::Value, Role::Other),
    ("--character-sets-dir", Takes::Value, Role::Other),
    ("--column-names", Takes::Attached, Role::Other),
    ("--column-type-info", Takes::Attached, Role::Other),
    ("-c", Takes::Nothing, Role::Other),
    ("--comments", Takes::Attached, Role::Other),
    ("-C", Takes::Nothing, Role::Other),
    ("--compress", Takes::Attached, Role::Other),
    ("--compression-algorithms", Takes::Value, Role::Other),
    ("--zstd-compression-level", Takes::Value, Role::Other),
    ("--connect-expired-password", Takes::Attached, Role::Other),
    ("--connect-timeout", Takes::Value, Role::Other),
    ("--debug-check", Takes::Attached, Role::Other),
    ("-T", Takes::Nothing, Role::Other),
    ("--debug-info", Takes::Attached, Role::Other),
    ("--default-auth", Takes::Value, Role::Other),
    ("--default-character-set", Takes::Value, Role::Other),
    ("--delimiter", Takes::Value, Role::Other),
    ("--dns-srv-name", Takes::Value, Role::Host),
    ("--enable-cleartext-plugin", Takes::Attached, Role::Other),
    ("-E", Takes::Nothing, Role::Other),
    ("--vertical", Takes::Attached, Role::Other),
    ("-f", Takes::Nothing, Role::Other),
    ("--force", Takes::Attached, Role::Other),
    ("-G", Takes::Nothing, Role::Other),
    ("--named-commands", Takes::Attached, Role::Other),
    ("-g", Takes::Nothing, Role::Other),
    ("--no-named-commands", Takes::Attached, Role::Other),
    ("--histignore", Takes::Value, Role::Other),
    ("-H", Takes::Nothing, Role::Other),
    ("--html", Takes::Attached, Role::Other),
    ("-i", Takes::Nothing, Role::Other),
    ("--ignore-spaces", Takes::Attached, Role::Other),
    ("--init-command", Takes::Value, Role::Other),
    ("--init-command-add", Takes::Value, Role::Other),
    ("--load-data-local-dir", Takes::Value, Role::Other),
    ("--line-numbers", Takes::Attached, Role::Other),
    ("-L", Takes::Nothing, Role::Other),
    ("-j", Takes::Nothing, Role::Other),
    ("--syslog", Takes::Attached, Role::Other),
    ("--max-allowed-packet", Takes::Value, Role::Other),
    ("--max-join-size", Takes::Value, Role::Other),
    ("--max-statement-time", Takes::Value, Role::Other),
    ("--net-buffer-length", Takes::Value, Role::Other),
    ("--network-namespace", Takes::Value, Role::Other),
    ("-n", Takes::Nothing, Role::Other),
    ("--unbuffered", Takes::Attached, Role::Other),
    ("-N", Takes::Nothing, Role::Other),
    ("-o", Takes::Nothing, Role::Other),
    ("--one-database", Takes::Attached, Role::Other),
    ("--plugin-dir", Takes::Value, Role::Other),
    ("--prompt", Takes::Value, Role::Other),
    ("--protocol", Takes::Value, Role::Other),
    ("-q", Takes::Nothing, Role::Other),
    ("--quick", Takes::Attached, Role::Other),
    ("-r", Takes::Nothing, Role::Other),
    ("--raw", Takes::Attached, Role::Other),
    ("--reconnect", Takes::Attached, Role::Other),
    ("-U", Takes::Nothing, Role::Other),
    ("--safe-updates", Takes::Attached, Role::Other),
    ("--i-am-a-dummy", Takes::Attached, Role::Other),
    ("--sandbox", Takes::Attached, Role::Other),
    ("--select-limit", Takes::Value, Role::Other),
    ("--server-public-key-path", Takes::Value, Role::Other),
    ("--get-server-public-key", Takes::Attached, Role::Other),
    ("--shared-memory-base-name", Takes::Value, Role::Other),
    ("--show-warnings", Takes::Attached, Role::Other),
    ("--sigint-ignore", Takes::Attached, Role::Other),
    ("-s", Takes::Nothing, Role::Other),
    ("--silent", Takes::Attached, Role::Other),
    ("--ssl", Takes::Attached, Role::Other),
    ("--ssl-mode", Takes::Value, Role::Other),
    ("--ssl-ca", Takes::Value, Role::Other),
    ("--ssl-capath", Takes::Value, Role::Other),
    ("--ssl-cert", Takes::Value, Role::Other),
    ("--ssl-cipher", Takes::Value, Role::Other),
    ("--ssl-key", Takes::Value, Role::Other),
    ("--ssl-crl", Takes::Value, Role::Other),
    ("--ssl-crlpath", Takes::Value, Role::Other),
    ("--ssl-fips-mode", Takes::Value, Role::Other),
    ("--ssl-session-data", Takes::Value, Role::Other),
    (
        "--ssl-session-data-continue-on-failed-reuse",
        Takes::Attached,
        Role::Other,
    ),
    ("--ssl-verify-server-cert", Takes::Attached, Role::Other),
    ("--tls-version", Takes::Value, Role::Other),
    ("--tls-ciphersuites", Takes::Value, Role::Other),
    ("--tls-sni-servername", Takes::Value, Role::Other),
    ("-t", Takes::Nothing, Role::Other),
    ("--table", Takes::Attached, Role::Other),
    ("--tee", Takes::Value, Role::Other),
    ("--no-tee", Takes::Attached, Role::Other),
    ("-v", Takes::Nothing, Role::Other),
    ("--verbose", Takes::Attached, Role::Other),
    ("-w", Takes::Nothing, Role::Other),
    ("--wait", Takes::Attached, Role::Other),
    ("-W", Takes::Nothing, Role::Other),
    ("--pipe", Takes::Attached, Role::Other),
    ("-X", Takes::Nothing, Role::Other),
    ("--xml", Takes::Attached, Role::Other),
    ("--abort-source-on-error", Takes::Attached, Role::Other),
    ("--progress-reports", Takes::Attached, Role::Other),
    ("--commands", Takes::Attached, Role::Other),
    ("--system-command", Takes::Attached, Role::Other),
    ("--telemetry-client", Takes::Attached, Role::Other),
    ("--oci-config-file", Takes::Value, Role::Other),
    (
        "--authentication-oci-client-config-profile",
        Takes::Value,
        Role::Other,
    ),
    ("--fido-register-factor", Takes::Value, Role::Other),
    ("--register-factor", Takes::Value, Role::Other),
    (
        "--plugin-authentication-kerberos-client-mode",
        Takes::Value,
        Role::Other,
    ),
    (
        "--plugin-authentication-webauthn-client-preserve-privacy",
        Takes::Attached,
        Role::Other,
    ),
];

/// A mysql option by name: a long name's `_` is `-` and its `--loose-`
/// prefix drops (MySQL's spellings), and `--skip-x`, `--enable-x` and
/// `--disable-x` are the other spellings of a boolean — a flag whatever
/// the boolean is.
fn mysql_option(name: &str) -> Option<(Takes, Role)> {
    let Some(long) = name.strip_prefix("--") else {
        return row(MYSQL, name);
    };
    let long = long.replace('_', "-");
    let long = long.strip_prefix("loose-").unwrap_or(&long);
    row(MYSQL, &format!("--{long}")).or_else(|| {
        ["skip-", "enable-", "disable-"]
            .iter()
            .any(|prefix| long.starts_with(prefix))
            .then_some((Takes::Attached, Role::Other))
    })
}

/// mysql's target: the options, then the one database argument over
/// `-D`. **`MYSQL_HOST` and `MYSQL_TCP_PORT` only with `--no-defaults`**:
/// otherwise mysql reads its option files (`/etc/my.cnf`, `~/.my.cnf`,
/// `~/.mylogin.cnf`'s `[client]`), whose host beats the variables, and they
/// are not read here — the variable could name another server than the one
/// mysql talks to. More than one argument is mysql's usage error: no
/// prompt.
fn mysql(args: &[String], record: &ProcArgs) -> Option<Target> {
    let walk = walk(args, Dialect::Getopt, mysql_option);
    if walk.has(Role::Runs) || walk.positionals.len() > 1 {
        return None;
    }
    if walk.unknown {
        return Some(Target::default());
    }
    let mut target = Target::default();
    for &(role, value) in &walk.options {
        let value = value.map(scrub);
        match role {
            Role::Host => target.host = value,
            Role::Port => target.port = value,
            Role::User => target.user = value,
            Role::Database => target.database = value,
            _ => {}
        }
    }
    if let Some(database) = walk.positionals.first() {
        target.database = Some(scrub(database));
    }
    if walk.has(Role::NoDefaults) && !walk.has(Role::OptionFile) {
        for (slot, key) in [
            (&mut target.host, "MYSQL_HOST"),
            (&mut target.port, "MYSQL_TCP_PORT"),
        ] {
            if slot.is_none() {
                *slot = record.var(key).map(scrub);
            }
        }
    }
    Some(target)
}

// --- sqlite3 ------------------------------------------------------------

/// sqlite3's options (`sqlite3 -help`); `--x` is `-x`.
const SQLITE: &[Opt] = &[
    ("-A", Takes::Rest, Role::Runs),
    ("-help", Takes::Nothing, Role::Runs),
    ("-version", Takes::Nothing, Role::Runs),
    ("-cmd", Takes::Value, Role::Other),
    ("-escape", Takes::Value, Role::Other),
    ("-heap", Takes::Value, Role::Other),
    ("-init", Takes::Value, Role::Other),
    ("-lookaside", Takes::Values(2), Role::Other),
    ("-maxsize", Takes::Value, Role::Other),
    ("-mmap", Takes::Value, Role::Other),
    ("-newline", Takes::Value, Role::Other),
    ("-nonce", Takes::Value, Role::Other),
    ("-nullvalue", Takes::Value, Role::Other),
    ("-pagecache", Takes::Values(2), Role::Other),
    ("-screenwidth", Takes::Value, Role::Other),
    ("-separator", Takes::Value, Role::Other),
    ("-sorterref", Takes::Value, Role::Other),
    ("-vfs", Takes::Value, Role::Other),
    ("-append", Takes::Nothing, Role::Other),
    ("-ascii", Takes::Nothing, Role::Other),
    ("-bail", Takes::Nothing, Role::Other),
    ("-batch", Takes::Nothing, Role::Other),
    ("-box", Takes::Nothing, Role::Other),
    ("-column", Takes::Nothing, Role::Other),
    ("-csv", Takes::Nothing, Role::Other),
    ("-deserialize", Takes::Nothing, Role::Other),
    ("-echo", Takes::Nothing, Role::Other),
    ("-header", Takes::Nothing, Role::Other),
    ("-noheader", Takes::Nothing, Role::Other),
    ("-html", Takes::Nothing, Role::Other),
    ("-ifexists", Takes::Nothing, Role::Other),
    ("-interactive", Takes::Nothing, Role::Other),
    ("-json", Takes::Nothing, Role::Other),
    ("-line", Takes::Nothing, Role::Other),
    ("-list", Takes::Nothing, Role::Other),
    ("-markdown", Takes::Nothing, Role::Other),
    ("-memtrace", Takes::Nothing, Role::Other),
    ("-multiplex", Takes::Nothing, Role::Other),
    ("-nofollow", Takes::Nothing, Role::Other),
    ("-no-rowid-in-view", Takes::Nothing, Role::Other),
    ("-pcachetrace", Takes::Nothing, Role::Other),
    ("-quote", Takes::Nothing, Role::Other),
    ("-readonly", Takes::Nothing, Role::Other),
    ("-safe", Takes::Nothing, Role::Other),
    ("-stats", Takes::Nothing, Role::Other),
    ("-table", Takes::Nothing, Role::Other),
    ("-tabs", Takes::Nothing, Role::Other),
    ("-unsafe-testing", Takes::Nothing, Role::Other),
    ("-utf8", Takes::Nothing, Role::Other),
    ("-no-utf8", Takes::Nothing, Role::Other),
    ("-vfstrace", Takes::Nothing, Role::Other),
    ("-zip", Takes::Nothing, Role::Other),
];

fn sqlite_option(name: &str) -> Option<(Takes, Role)> {
    let name = name
        .strip_prefix('-')
        .filter(|rest| rest.starts_with('-'))
        .unwrap_or(name);
    row(SQLITE, name)
}

/// sqlite3's file: `sqlite3 [OPTIONS] FILENAME [SQL…]` — SQL after the
/// file runs and exits. A `file:` URI's query is not shown: SQLite's
/// encryption extensions take the key there (`?key=…`).
fn sqlite(args: &[String]) -> Option<Target> {
    let walk = walk(args, Dialect::Whole, sqlite_option);
    if walk.has(Role::Runs) || walk.positionals.len() > 1 {
        return None;
    }
    if walk.unknown {
        return Some(Target::default());
    }
    // A path, not a name: `file:///x.db` is SQLite's URI. Only a control
    // character hides it.
    let file = walk.positionals.first().map(|file| {
        let file = match file.strip_prefix("file:") {
            Some(_) => file.split(['?', '#']).next().unwrap_or_default(),
            None => file,
        };
        if file.chars().any(char::is_control) {
            String::new()
        } else {
            (*file).to_owned()
        }
    });
    Some(Target {
        file,
        ..Target::default()
    })
}

// --- redis-cli ----------------------------------------------------------

/// redis-cli's options (`redis-cli --help`); a mode that is not the prompt
/// (`--stat`, `--scan`, `--pipe`…) runs and exits, and so does an argument
/// — the command to send.
const REDIS: &[Opt] = &[
    ("-a", Takes::Value, Role::Other),
    ("--pass", Takes::Value, Role::Other),
    ("--askpass", Takes::Nothing, Role::Other),
    ("-u", Takes::Value, Role::Uri),
    ("-h", Takes::Value, Role::Host),
    ("-p", Takes::Value, Role::Port),
    ("-s", Takes::Value, Role::Socket),
    ("-n", Takes::Value, Role::Database),
    ("--user", Takes::Value, Role::User),
    ("-r", Takes::Value, Role::Other),
    ("-i", Takes::Value, Role::Other),
    ("-d", Takes::Value, Role::Other),
    ("-D", Takes::Value, Role::Other),
    ("-t", Takes::Value, Role::Other),
    ("-X", Takes::Value, Role::Other),
    ("-x", Takes::Nothing, Role::Other),
    ("-2", Takes::Nothing, Role::Other),
    ("-3", Takes::Nothing, Role::Other),
    ("-c", Takes::Nothing, Role::Other),
    ("-e", Takes::Nothing, Role::Other),
    ("-4", Takes::Nothing, Role::Other),
    ("-6", Takes::Nothing, Role::Other),
    ("--tls", Takes::Nothing, Role::Other),
    ("--sni", Takes::Value, Role::Other),
    ("--cacert", Takes::Value, Role::Other),
    ("--cacertdir", Takes::Value, Role::Other),
    ("--insecure", Takes::Nothing, Role::Other),
    ("--tls-ciphers", Takes::Value, Role::Other),
    ("--tls-ciphersuites", Takes::Value, Role::Other),
    ("--cert", Takes::Value, Role::Other),
    ("--key", Takes::Value, Role::Other),
    ("--raw", Takes::Nothing, Role::Other),
    ("--no-raw", Takes::Nothing, Role::Other),
    ("--quoted-input", Takes::Nothing, Role::Other),
    ("--csv", Takes::Nothing, Role::Other),
    ("--json", Takes::Nothing, Role::Other),
    ("--quoted-json", Takes::Nothing, Role::Other),
    ("--show-pushes", Takes::Value, Role::Other),
    ("--verbose", Takes::Nothing, Role::Other),
    ("--no-auth-warning", Takes::Nothing, Role::Other),
    ("--ldb", Takes::Nothing, Role::Other),
    ("--ldb-sync-mode", Takes::Nothing, Role::Other),
    ("--pattern", Takes::Value, Role::Other),
    ("--quoted-pattern", Takes::Value, Role::Other),
    ("--count", Takes::Value, Role::Other),
    ("--cursor", Takes::Value, Role::Other),
    ("--top", Takes::Value, Role::Other),
    ("--pipe-timeout", Takes::Value, Role::Other),
    ("--memkeys-samples", Takes::Value, Role::Other),
    ("--keystats-samples", Takes::Value, Role::Other),
    ("--eval", Takes::Value, Role::Runs),
    ("--rdb", Takes::Value, Role::Runs),
    ("--functions-rdb", Takes::Value, Role::Runs),
    ("--lru-test", Takes::Value, Role::Runs),
    ("--intrinsic-latency", Takes::Value, Role::Runs),
    ("--cluster", Takes::Rest, Role::Runs),
    ("--stat", Takes::Nothing, Role::Runs),
    ("--latency", Takes::Nothing, Role::Runs),
    ("--latency-history", Takes::Nothing, Role::Runs),
    ("--latency-dist", Takes::Nothing, Role::Runs),
    ("--replica", Takes::Nothing, Role::Runs),
    ("--slave", Takes::Nothing, Role::Runs),
    ("--pipe", Takes::Nothing, Role::Runs),
    ("--bigkeys", Takes::Nothing, Role::Runs),
    ("--memkeys", Takes::Nothing, Role::Runs),
    ("--keystats", Takes::Nothing, Role::Runs),
    ("--hotkeys", Takes::Nothing, Role::Runs),
    ("--scan", Takes::Nothing, Role::Runs),
    ("--help", Takes::Nothing, Role::Runs),
    ("--version", Takes::Nothing, Role::Runs),
    ("-v", Takes::Nothing, Role::Runs),
    ("--no-ignore-host", Takes::Nothing, Role::Other),
    ("--latency-history-interval", Takes::Value, Role::Other),
    ("--keystats-top", Takes::Value, Role::Other),
    ("--version-check", Takes::Nothing, Role::Other),
];

fn redis_option(name: &str) -> Option<(Takes, Role)> {
    row(REDIS, name)
}

/// redis-cli's target, its options in order (a later `-h` beats `-u`'s
/// host); a socket (`-s`) makes the host and port unused.
fn redis(args: &[String]) -> Option<Target> {
    let walk = walk(args, Dialect::Whole, redis_option);
    if walk.has(Role::Runs) || !walk.positionals.is_empty() {
        return None;
    }
    if walk.unknown {
        return Some(Target::default());
    }
    let mut target = Target::default();
    let mut socket = false;
    for &(role, value) in &walk.options {
        match role {
            Role::Uri => match value.and_then(redis_uri) {
                Some(uri) => target.overlay(uri),
                // redis-cli refuses a URI with another scheme and goes on
                // with one it reads its own way; an ambiguous one is not
                // read here.
                None => return Some(Target::default()),
            },
            Role::Host => target.host = value.map(scrub),
            Role::Port => target.port = value.map(scrub),
            Role::User => target.user = value.map(scrub),
            Role::Database => target.database = value.map(scrub),
            Role::Socket => socket = true,
            _ => {}
        }
    }
    if socket {
        target.host = None;
        target.port = None;
    }
    Some(target)
}

/// `redis://[[user]:password@]host[:port][/db]` (or `rediss://`). **A
/// userinfo without `:` is the password**, not a user: redis-cli reads
/// `redis://secret@host` as `AUTH secret`.
fn redis_uri(text: &str) -> Option<Target> {
    let rest =
        strip_scheme(text, "redis://", true).or_else(|| strip_scheme(text, "rediss://", true))?;
    let uri = Uri::split(rest)?;
    let (host, port) = uri.server();
    let user = uri
        .userinfo
        .filter(|info| info.contains(':'))
        .and_then(|_| uri.user());
    let database = uri
        .path
        .filter(|db| !db.is_empty() && db.bytes().all(|b| b.is_ascii_digit()))
        .map(str::to_owned);
    Some(Target {
        user,
        host,
        port,
        database,
        ..Target::default()
    })
}

// --- mongosh ------------------------------------------------------------

/// mongosh's options (`mongosh --help`). Each value-taking one takes the
/// next argument only when that is not an option (yargs): `-p` alone asks
/// for the password. The secrets — the password, the AWS keys, the
/// certificate key's password — are values here like any other.
const MONGO: &[Opt] = &[
    ("-p", Takes::Value, Role::Other),
    ("--password", Takes::Value, Role::Other),
    ("--awsAccessKeyId", Takes::Value, Role::Other),
    ("--awsSecretAccessKey", Takes::Value, Role::Other),
    ("--awsSessionToken", Takes::Value, Role::Other),
    ("--awsIamSessionToken", Takes::Value, Role::Other),
    ("--tlsCertificateKeyFilePassword", Takes::Value, Role::Other),
    ("--host", Takes::Value, Role::Host),
    ("--port", Takes::Value, Role::Port),
    ("-u", Takes::Value, Role::User),
    ("--username", Takes::Value, Role::User),
    ("--eval", Takes::Value, Role::Runs),
    ("-f", Takes::Value, Role::Runs),
    ("--file", Takes::Value, Role::Runs),
    ("--shell", Takes::Nothing, Role::Shell),
    ("--nodb", Takes::Nothing, Role::NoDb),
    ("-h", Takes::Nothing, Role::Runs),
    ("--help", Takes::Nothing, Role::Runs),
    ("--version", Takes::Nothing, Role::Runs),
    ("--build-info", Takes::Nothing, Role::Runs),
    ("--smokeTests", Takes::Nothing, Role::Runs),
    ("--authenticationDatabase", Takes::Value, Role::Other),
    ("--authenticationMechanism", Takes::Value, Role::Other),
    ("--gssapiServiceName", Takes::Value, Role::Other),
    ("--sspiHostnameCanonicalization", Takes::Value, Role::Other),
    ("--sspiRealmOverride", Takes::Value, Role::Other),
    ("--tlsCAFile", Takes::Value, Role::Other),
    ("--tlsCertificateKeyFile", Takes::Value, Role::Other),
    ("--tlsCRLFile", Takes::Value, Role::Other),
    ("--tlsCertificateSelector", Takes::Value, Role::Other),
    ("--tlsDisabledProtocols", Takes::Value, Role::Other),
    ("--apiVersion", Takes::Value, Role::Other),
    ("--keyVaultNamespace", Takes::Value, Role::Other),
    ("--kmsURL", Takes::Value, Role::Other),
    ("--cryptSharedLibPath", Takes::Value, Role::Other),
    ("--csfleLibraryPath", Takes::Value, Role::Other),
    ("--browser", Takes::Value, Role::Other),
    ("--oidcFlows", Takes::Value, Role::Other),
    ("--oidcRedirectUri", Takes::Value, Role::Other),
    ("--json", Takes::Attached, Role::Other),
    ("--retryWrites", Takes::Attached, Role::Other),
    ("--quiet", Takes::Nothing, Role::Other),
    ("--verbose", Takes::Nothing, Role::Other),
    ("--norc", Takes::Nothing, Role::Other),
    ("--ipv6", Takes::Nothing, Role::Other),
    ("--tls", Takes::Nothing, Role::Other),
    ("--tlsAllowInvalidCertificates", Takes::Nothing, Role::Other),
    ("--tlsAllowInvalidHostnames", Takes::Nothing, Role::Other),
    ("--tlsFIPSMode", Takes::Nothing, Role::Other),
    ("--tlsUseSystemCA", Takes::Nothing, Role::Other),
    ("--apiStrict", Takes::Nothing, Role::Other),
    ("--apiDeprecationErrors", Takes::Nothing, Role::Other),
    ("--skipStartupWarnings", Takes::Nothing, Role::Other),
    ("--deepInspect", Takes::Attached, Role::Other),
    ("--exposeAsyncRewriter", Takes::Nothing, Role::Other),
    ("--oidcTrustedEndpoint", Takes::Nothing, Role::Other),
    ("--oidcIdTokenAsAccessToken", Takes::Nothing, Role::Other),
    ("--oidcDumpTokens", Takes::Attached, Role::Other),
    ("--oidcNoNonce", Takes::Nothing, Role::Other),
    ("--redactInfo", Takes::Nothing, Role::Other),
    ("--sslPEMKeyPassword", Takes::Value, Role::Other),
    ("--sslPEMKeyFile", Takes::Value, Role::Other),
    ("--sslCAFile", Takes::Value, Role::Other),
    ("--ssl", Takes::Nothing, Role::Other),
    ("--sslAllowInvalidCertificates", Takes::Nothing, Role::Other),
    ("--sslAllowInvalidHostnames", Takes::Nothing, Role::Other),
];

fn mongo_option(name: &str) -> Option<(Takes, Role)> {
    row(MONGO, name)
}

/// mongosh's target: `mongosh [options] [db address] [files…]` — a file
/// (`.js`, `.mongodb`), `--eval` or `-f` runs and exits unless `--shell`
/// keeps the prompt; `--nodb` is a prompt connected to nothing. The
/// options beat the address's parts.
fn mongo(args: &[String]) -> Option<Target> {
    let walk = walk(args, Dialect::Yargs, mongo_option);
    let (files, addresses): (Vec<&str>, Vec<&str>) = walk.positionals.iter().partition(|arg| {
        let lower = arg.to_ascii_lowercase();
        lower.ends_with(".js") || lower.ends_with(".mongodb")
    });
    if (walk.has(Role::Runs) || !files.is_empty()) && !walk.has(Role::Shell) {
        return None;
    }
    if walk.unknown || walk.has(Role::NoDb) {
        return Some(Target::default());
    }
    let mut target = match addresses.first() {
        Some(address) => match mongo_address(address) {
            Some(target) => target,
            None => return Some(Target::default()),
        },
        None => Target::default(),
    };
    for &(role, value) in &walk.options {
        match role {
            Role::Host => match value.map(mongo_host) {
                Some(Some(host)) => target.overlay(host),
                Some(None) => return Some(Target::default()),
                None => {}
            },
            Role::Port => target.port = value.map(scrub),
            Role::User => target.user = value.map(scrub),
            _ => {}
        }
    }
    Some(target)
}

/// mongosh's address argument: a URI, `host[:port]/db`, `host:port`, a
/// host (it has a dot — a database name cannot) or a database's name.
fn mongo_address(text: &str) -> Option<Target> {
    if let Some(uri) = mongo_uri(text) {
        return uri;
    }
    if let Some((server, database)) = text.split_once('/') {
        let mut target = mongo_server(server)?;
        target.database = (!database.is_empty()).then(|| scrub(database));
        return Some(target);
    }
    if text.contains([':', '.']) {
        return mongo_server(text);
    }
    Some(Target {
        database: Some(scrub(text)),
        ..Target::default()
    })
}

/// mongosh's `--host`: a URI, `replicaSet/host1,host2` or `host[:port]`.
fn mongo_host(text: &str) -> Option<Target> {
    if let Some(uri) = mongo_uri(text) {
        return uri;
    }
    let hosts = text.split_once('/').map_or(text, |(_, hosts)| hosts);
    mongo_server(hosts)
}

/// `host[:port]` or a list, as a URI's authority.
fn mongo_server(text: &str) -> Option<Target> {
    let (host, port) = Uri {
        userinfo: None,
        authority: text,
        path: None,
        query: None,
    }
    .server();
    Some(Target {
        host,
        port,
        ..Target::default()
    })
}

/// A MongoDB URI (`mongodb://` or `mongodb+srv://`): its user, server and
/// database — **never its query**, where `authMechanismProperties` can
/// carry a session token. `None` when `text` is no MongoDB URI;
/// `Some(None)` when it is an ambiguous one ([`Uri::split`]).
fn mongo_uri(text: &str) -> Option<Option<Target>> {
    let rest = strip_scheme(text, "mongodb://", true)
        .or_else(|| strip_scheme(text, "mongodb+srv://", true))?;
    Some(Uri::split(rest).map(|uri| {
        let (host, port) = uri.server();
        Target {
            user: uri.user(),
            host,
            port,
            database: uri.database(),
            ..Target::default()
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &[&str]) -> Vec<String> {
        line.iter().map(|&arg| arg.to_owned()).collect()
    }

    fn record(env: &[(&str, &str)]) -> ProcArgs {
        ProcArgs {
            exec: String::new(),
            args: Vec::new(),
            env: env
                .iter()
                .map(|&(key, value)| (key.to_owned(), value.to_owned()))
                .collect(),
        }
    }

    fn target(client: Client, line: &[&str]) -> Option<Target> {
        client.target(&args(line), &record(&[]))
    }

    /// The target's text as the bar shows it.
    fn shown(client: Client, line: &[&str]) -> Option<String> {
        target(client, line).map(|target| target.shown(None))
    }

    /// No piece of the password in `line` reaches what the client's line
    /// gives — the target, its `Debug`, its text, its mark host: none of
    /// the password's alphanumeric runs, nor any four characters of one
    /// (a tail cut off at an `@` is a leak too). The failure message names
    /// the case, never the value.
    fn assert_no_secret(client: Client, line: &[&str], env: &[(&str, &str)], case: &str) {
        // The password of every case is spelled around `hun`…`ter2`.
        let secret = line
            .iter()
            .chain(env.iter().map(|(_, value)| value))
            .find(|arg| arg.contains("hun"))
            .expect("the case carries its password");
        let found = client.target(&args(line), &record(env));
        let printed = format!(
            "{found:?} {:?} {:?}",
            found.as_ref().map(|target| target.shown(None)),
            found.as_ref().and_then(Target::mark_host),
        );
        let runs: Vec<&str> = secret
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .filter(|run| run.contains("hun") || run.contains("ter"))
            .collect();
        for run in runs {
            let pieces: Vec<&str> = if run.len() < 4 {
                vec![run]
            } else {
                (0..=run.len() - 4).map(|at| &run[at..at + 4]).collect()
            };
            for piece in pieces {
                assert!(
                    !printed.contains(piece),
                    "{client:?}, {case}: the password reached the target"
                );
            }
        }
    }

    #[test]
    fn psql_reads_its_options_its_arguments_and_the_variables() {
        let pg = Client::Postgres;
        assert_eq!(
            shown(pg, &["-h", "db.prod", "-p", "5432", "-U", "app", "main"]).as_deref(),
            Some("app@db.prod:5432/main")
        );
        // Clustered and attached, long with `=` and apart; permuted.
        assert_eq!(
            shown(
                pg,
                &["main", "-Xqhdb.prod", "--port=6432", "--username", "app"]
            )
            .as_deref(),
            Some("app@db.prod:6432/main")
        );
        // `psql DBNAME USERNAME`.
        assert_eq!(shown(pg, &["main", "app"]).as_deref(), Some("app@/main"));
        // Nothing given: nothing shown, the client's name alone.
        assert_eq!(shown(pg, &[]).as_deref(), Some(""));
        // A socket directory is local: no host.
        let local = target(pg, &["-h", "/var/run/postgresql", "main"]).expect("psql");
        assert_eq!(local.shown(None), "/main");
        assert_eq!(local.mark_host(), None);
        // The variables fill what the line does not say, never over it.
        let env = record(&[
            ("PGHOST", "db.stage"),
            ("PGPORT", "5433"),
            ("PGUSER", "bob"),
            ("PGDATABASE", "shop"),
        ]);
        let filled = pg.target(&args(&["-U", "app"]), &env).expect("psql");
        assert_eq!(filled.shown(None), "app@db.stage:5433/shop");
        assert_eq!(filled.mark_host().as_deref(), Some("db.stage"));
    }

    #[test]
    fn the_measured_records_read_as_expected() {
        // Measured (Linux, `/proc/{pid}/cmdline` at the prompt, ICANON and
        // ECHO both off): psql and redis-cli keep their argv as typed, the
        // password included — it is the tables that keep it off the bar.
        let psql = [
            "-U",
            "postgres",
            "host=127.0.0.1 dbname=postgres password='hunter2 x'",
        ];
        assert_eq!(
            shown(Client::Postgres, &psql).as_deref(),
            Some("postgres@127.0.0.1/postgres")
        );
        assert_no_secret(Client::Postgres, &psql, &[], "measured psql");
        let redis = [
            "-h",
            "127.0.0.1",
            "-n",
            "2",
            "--user",
            "default",
            "-a",
            "hunter2",
            "--no-auth-warning",
        ];
        assert_eq!(
            shown(Client::Redis, &redis).as_deref(),
            Some("default@127.0.0.1/2")
        );
        assert_no_secret(Client::Redis, &redis, &[], "measured redis-cli");
    }

    #[test]
    fn psql_running_something_is_no_prompt() {
        for line in [
            &["-c", "select 1"][..],
            &["--command=select 1"],
            &["-f", "x.sql"],
            &["-l"],
            &["-Xc", "select 1"],
            &["--version"],
        ] {
            assert_eq!(target(Client::Postgres, line), None, "{line:?}");
        }
    }

    #[test]
    fn a_connection_string_is_read_as_libpq_reads_it() {
        let pg = Client::Postgres;
        assert_eq!(
            shown(pg, &["host=db.prod port=5432 user=app dbname=main"]).as_deref(),
            Some("app@db.prod:5432/main")
        );
        assert_eq!(
            shown(
                pg,
                &["-d", "postgresql://app@db.prod:5432/main?sslmode=require"]
            )
            .as_deref(),
            Some("app@db.prod:5432/main")
        );
        // Over the options: libpq reads the string after them.
        assert_eq!(
            shown(pg, &["-h", "other", "host = 'db.prod' dbname=main"]).as_deref(),
            Some("db.prod/main")
        );
        // The query's keys; a percent-encoded name; an IPv6 address.
        assert_eq!(
            shown(
                pg,
                &["postgres:///main?host=db.prod&port=6432&user=app%40corp"]
            )
            .as_deref(),
            Some("app@corp@db.prod:6432/main")
        );
        let v6 = target(pg, &["postgresql://[::1]:5432/main"]).expect("psql");
        assert_eq!(v6.shown(None), "[::1]:5432/main");
        assert_eq!(v6.mark_host().as_deref(), Some("::1"));
        // A list: as written, the first host marked.
        let list = target(pg, &["postgresql://a.prod:5432,b.prod:5433/main"]).expect("psql");
        assert_eq!(list.shown(None), "a.prod:5432,b.prod:5433/main");
        assert_eq!(list.mark_host().as_deref(), Some("a.prod"));
        // A string libpq refuses names nothing.
        for broken in ["host=db.prod port", "host='db.prod"] {
            assert_eq!(shown(pg, &[broken]).as_deref(), Some(""), "{broken}");
        }
    }

    #[test]
    fn hostaddr_is_where_libpq_connects() {
        let pg = Client::Postgres;
        // The string's, the URI query's and the variable's, over the host.
        assert_eq!(
            shown(pg, &["host=db.stage hostaddr=10.0.0.5 dbname=main"]).as_deref(),
            Some("10.0.0.5/main")
        );
        assert_eq!(
            shown(pg, &["postgresql://db.stage/main?hostaddr=10.0.0.5"]).as_deref(),
            Some("10.0.0.5/main")
        );
        let env = record(&[("PGHOSTADDR", "10.0.0.5"), ("PGHOST", "db.stage")]);
        let target = pg
            .target(&args(&["-h", "db.stage", "main"]), &env)
            .expect("psql");
        assert_eq!(target.shown(None), "10.0.0.5/main");
        assert_eq!(target.mark_host().as_deref(), Some("10.0.0.5"));
        assert_eq!(target.hostaddr, None, "taken into the host");
    }

    #[test]
    fn an_empty_value_is_unset_as_libpq_takes_it() {
        let pg = Client::Postgres;
        let env = record(&[("PGHOST", "db.prod"), ("PGDATABASE", "shop")]);
        // `psql -h "$EMPTY" ""`: the variables decide.
        let target = pg.target(&args(&["-h", "", ""]), &env).expect("psql");
        assert_eq!(target.shown(None), "db.prod/shop");
        assert_eq!(
            pg.target(&args(&["host='' dbname=main"]), &env)
                .map(|target| target.shown(None))
                .as_deref(),
            Some("db.prod/main")
        );
        // An empty query pair is nothing: a trailing `&`, a bare `?`.
        assert_eq!(
            shown(pg, &["postgresql://app@db.prod/main?sslmode=require&"]).as_deref(),
            Some("app@db.prod/main")
        );
        assert_eq!(
            shown(pg, &["postgresql://db.prod/main?"]).as_deref(),
            Some("db.prod/main")
        );
    }

    #[test]
    fn a_service_names_the_server_from_its_file() {
        let pg = Client::Postgres;
        let env = record(&[("PGSERVICE", "prod"), ("PGHOST", "db.stage")]);
        let service = pg.target(&args(&["main"]), &env).expect("psql");
        assert_eq!(service.service.as_deref(), Some("prod"));
        assert_eq!(
            service.host, None,
            "the service file comes before the variables: PGHOST may not be it"
        );
        assert_eq!(service.shown(None), "/main");
        let inline = target(pg, &["service=prod"]).expect("psql");
        assert_eq!(inline.service.as_deref(), Some("prod"));
    }

    #[test]
    fn psql_never_shows_a_password() {
        let pg = Client::Postgres;
        let cases: [(&str, &[&str]); 11] = [
            ("conninfo", &["host=db password=hunter2 dbname=main"]),
            ("conninfo quoted", &["password='hunter2' host=db"]),
            (
                "conninfo quoted with a space",
                &["password='a hunter2 b' host=db"],
            ),
            (
                "conninfo quoted key=value",
                &["password='x host=hunter2' dbname=main"],
            ),
            (
                "conninfo escaped quote",
                &["password='x\\' host=hunter2' dbname=main"],
            ),
            (
                "conninfo escaped space",
                &["password=x\\ host=hunter2 dbname=main"],
            ),
            ("uri", &["postgresql://app:hunter2@db/main"]),
            (
                "uri with @ in the password",
                &["postgresql://app:hun@ter2@db/main"],
            ),
            (
                "uri with / in the password",
                &["postgres://app:hun/nter2@db/main"],
            ),
            ("uri query", &["postgresql://db/main?password=hunter2"]),
            ("-d", &["-d", "postgresql://app:hunter2@db/main"]),
        ];
        for (case, line) in cases {
            assert_no_secret(pg, line, &[], case);
        }
        // A connection string where a name belongs: with `-d` taken, the
        // next argument is the user; psql would send it as one.
        assert_no_secret(
            pg,
            &["-d", "main", "postgresql://admin:hunter2@db/x"],
            &[],
            "a URI as the user",
        );
        assert_no_secret(
            pg,
            &["-d", "main", "password=hunter2"],
            &[],
            "a string as the user",
        );
        assert_no_secret(pg, &["-U", "postgres://a:hunter2@b"], &[], "-U URI");
        // Two `@`: the user part may be a password's head.
        assert_no_secret(
            pg,
            &["postgresql://hun@ter:hunter2@db/main"],
            &[],
            "two @ in the userinfo",
        );
        // `--set`'s value is any text; the variables carry no password.
        assert_no_secret(pg, &["-v", "pw=hunter2"], &[], "--set");
        assert_no_secret(
            pg,
            &[],
            &[("PGPASSWORD", "hunter2"), ("PGHOST", "db")],
            "PGPASSWORD",
        );
        // `-W` asks for the password: a flag, the next argument a database.
        assert_eq!(shown(pg, &["-W", "main"]).as_deref(), Some("/main"));
        // An `@` inside the password stays inside the userinfo, which then
        // names no user (the part before `:` may be a password's head).
        assert_eq!(
            shown(pg, &["postgresql://app:hun@ter2@db/main"]).as_deref(),
            Some("db/main")
        );
        // An ambiguous URI is not read at all.
        assert_eq!(
            shown(pg, &["postgres://app:hun/nter2@db/main"]).as_deref(),
            Some("")
        );
    }

    #[test]
    fn an_unknown_option_leaves_the_target_unknown() {
        // `--hos=` is getopt's abbreviation of `--host`; a newer option may
        // take the next argument. Neither guess is made, and the variables
        // are not used in its place either.
        let env = record(&[("PGUSER", "bob")]);
        for line in [
            &["--hos=db.prod", "main"][..],
            &["--user=app"],
            &["--new-option", "hunter2", "main"],
            &["-Z", "main"],
            &["-qZhdb", "main"],
        ] {
            assert_eq!(
                Client::Postgres.target(&args(line), &env),
                Some(Target::default()),
                "{line:?}"
            );
        }
        assert_eq!(
            shown(Client::Mysql, &["--ssl-moded", "x", "shop"]).as_deref(),
            Some("")
        );
        assert_eq!(
            shown(Client::Redis, &["--auth", "hunter2"]).as_deref(),
            Some("")
        );
        assert_eq!(
            shown(Client::Sqlite, &["-key", "hunter2", "x.db"]).as_deref(),
            Some("")
        );
        assert_eq!(
            shown(Client::Mongo, &["--apiKey", "hunter2", "shop"]).as_deref(),
            Some("")
        );
    }

    #[test]
    fn mysql_reads_its_options_and_one_database() {
        let my = Client::Mysql;
        assert_eq!(
            shown(my, &["-h", "db.prod", "-P", "3306", "-u", "app", "shop"]).as_deref(),
            Some("app@db.prod:3306/shop")
        );
        assert_eq!(
            shown(my, &["-uapp", "-hdb.prod", "--database=shop"]).as_deref(),
            Some("app@db.prod/shop")
        );
        // MySQL's spellings: `_` for `-`, `--loose-`, a boolean's `=0`,
        // `--skip-…`.
        assert_eq!(
            shown(
                my,
                &[
                    "--ssl_mode=REQUIRED",
                    "--loose-host=db.prod",
                    "--reconnect=0",
                    "--skip-column-names",
                    "shop"
                ]
            )
            .as_deref(),
            Some("db.prod/shop")
        );
        // The argument beats `-D`.
        assert_eq!(shown(my, &["-D", "a", "b"]).as_deref(), Some("/b"));
        // mariadb is the same client.
        assert_eq!(
            shown(Client::Mariadb, &["-h", "db.prod", "shop"]).as_deref(),
            Some("db.prod/shop")
        );
        // The variables fill the host and port only when no option file is
        // read: mysql's default files (`~/.my.cnf`) beat them, and are not
        // read here.
        let env = record(&[("MYSQL_HOST", "db.stage"), ("MYSQL_TCP_PORT", "3307")]);
        let with = |line: &[&str]| my.target(&args(line), &env).map(|t| t.shown(None));
        assert_eq!(with(&["shop"]).as_deref(), Some("/shop"));
        assert_eq!(
            with(&["--no-defaults", "shop"]).as_deref(),
            Some("db.stage:3307/shop")
        );
        assert_eq!(
            with(&["--no-defaults", "--login-path=prod", "shop"]).as_deref(),
            Some("/shop")
        );
        // Running a statement, or a usage error, is no prompt.
        for line in [&["-e", "select 1"][..], &["a", "b"], &["--version"]] {
            assert_eq!(target(my, line), None, "{line:?}");
        }
    }

    #[test]
    fn mysql_never_shows_a_password() {
        let my = Client::Mysql;
        let cases: [(&str, &[&str]); 9] = [
            ("-p attached", &["-u", "app", "-phunter2", "shop"]),
            ("-p in a cluster", &["-uapp", "-Bphunter2", "shop"]),
            ("--password=", &["--password=hunter2", "shop"]),
            ("--loose-password=", &["--loose-password=hunter2", "shop"]),
            ("--loose_password=", &["--loose_password=hunter2"]),
            ("--password1=", &["--password1=hunter2", "shop"]),
            ("--password2=", &["--password2=hunter2"]),
            ("--password3=", &["--password3=hunter2"]),
            (
                "--init-command",
                &["--init-command", "set @x='hunter2'", "shop"],
            ),
        ];
        for (case, line) in cases {
            assert_no_secret(my, line, &[], case);
        }
        assert_no_secret(my, &["shop"], &[("MYSQL_PWD", "hunter2")], "MYSQL_PWD");
        // A bare `-p` asks for the password: the next argument is the
        // database, as mysql reads it.
        assert_eq!(
            shown(my, &["-u", "root", "-p", "shop"]).as_deref(),
            Some("root@/shop")
        );
        assert_eq!(shown(my, &["--password", "shop"]).as_deref(), Some("/shop"));
    }

    #[test]
    fn sqlite_shows_its_file() {
        let home = Path::new("/Users/me");
        let file = target(Client::Sqlite, &["/Users/me/x.db"]).expect("sqlite3");
        assert_eq!(file.shown(Some(home)), "~/x.db");
        assert_eq!(file.mark_host(), None, "a file is no host");
        assert_eq!(
            shown(Client::Sqlite, &["-readonly", "--box", "data/app.db"]).as_deref(),
            Some("data/app.db")
        );
        assert_eq!(
            shown(
                Client::Sqlite,
                &["-cmd", ".mode box", "-lookaside", "64", "128", "x.db"]
            )
            .as_deref(),
            Some("x.db")
        );
        assert_eq!(shown(Client::Sqlite, &[]).as_deref(), Some(""), "in memory");
        // A `file:` URI's query is not shown: the encryption key is there.
        assert_no_secret(
            Client::Sqlite,
            &["file:x.db?key=hunter2"],
            &[],
            "file: ?key",
        );
        assert_eq!(
            shown(Client::Sqlite, &["file:x.db?mode=ro"]).as_deref(),
            Some("file:x.db")
        );
        // SQL after the file runs and exits; so does an archive.
        for line in [
            &["x.db", "select 1"][..],
            &["-A", "-t", "x.db"],
            &["-version"],
        ] {
            assert_eq!(target(Client::Sqlite, line), None, "{line:?}");
        }
    }

    #[test]
    fn redis_reads_its_options_and_its_uri() {
        let redis = Client::Redis;
        assert_eq!(
            shown(redis, &["-h", "127.0.0.1", "-p", "6380", "-n", "2"]).as_deref(),
            Some("127.0.0.1:6380/2")
        );
        assert_eq!(
            shown(redis, &["-u", "redis://app:hunter2@cache.prod:6379/3"]).as_deref(),
            Some("app@cache.prod:6379/3")
        );
        // In order: a later `-h` beats the URI's host.
        assert_eq!(
            shown(
                redis,
                &[
                    "-u",
                    "rediss://cache.prod",
                    "-h",
                    "cache.stage",
                    "--user",
                    "app"
                ]
            )
            .as_deref(),
            Some("app@cache.stage")
        );
        // A socket: the host is not used.
        assert_eq!(
            shown(redis, &["-h", "x", "-s", "/tmp/redis.sock"]).as_deref(),
            Some("")
        );
        // A command, or a mode that is not the prompt, runs and exits.
        for line in [
            &["get", "k"][..],
            &["-h", "x", "ping"],
            &["--stat"],
            &["--cluster", "info", "x:1"],
        ] {
            assert_eq!(target(redis, line), None, "{line:?}");
        }
    }

    #[test]
    fn redis_never_shows_a_password() {
        let redis = Client::Redis;
        let cases: [(&str, &[&str]); 8] = [
            ("-a", &["-a", "hunter2", "-h", "db"]),
            ("--pass", &["--pass", "hunter2"]),
            ("-u with user", &["-u", "redis://app:hunter2@db:6379"]),
            ("-u password only", &["-u", "redis://:hunter2@db"]),
            (
                "-u bare userinfo is a password",
                &["-u", "redis://hunter2@db"],
            ),
            ("-u @ in the password", &["-u", "redis://:hun@ter2@db"]),
            ("-u / in the password", &["-u", "redis://:hun/ter2@db/0"]),
            (
                "-u @ and : in the password",
                &["-u", "redis://hun@ter:hunter2@db.prod"],
            ),
        ];
        for (case, line) in cases {
            assert_no_secret(redis, line, &[], case);
        }
        assert_no_secret(redis, &[], &[("REDISCLI_AUTH", "hunter2")], "REDISCLI_AUTH");
        assert_eq!(
            shown(redis, &["-u", "redis://hunter2@db"]).as_deref(),
            Some("db"),
            "a userinfo without `:` is the password, not a user"
        );
    }

    #[test]
    fn mongosh_reads_its_address_and_options() {
        let mongo = Client::Mongo;
        assert_eq!(
            shown(
                mongo,
                &["mongodb://app@db.prod:27017/shop?authSource=admin"]
            )
            .as_deref(),
            Some("app@db.prod:27017/shop")
        );
        assert_eq!(
            shown(mongo, &["mongodb+srv://cluster0.example.net/shop"]).as_deref(),
            Some("cluster0.example.net/shop")
        );
        assert_eq!(
            shown(mongo, &["db.prod:27018/shop"]).as_deref(),
            Some("db.prod:27018/shop")
        );
        assert_eq!(shown(mongo, &["shop"]).as_deref(), Some("/shop"));
        assert_eq!(shown(mongo, &["db.prod"]).as_deref(), Some("db.prod"));
        assert_eq!(
            shown(
                mongo,
                &[
                    "--host",
                    "rs0/a.prod:27017,b.prod",
                    "--port=27019",
                    "-u",
                    "app",
                    "--quiet"
                ]
            )
            .as_deref(),
            Some("app@a.prod:27017,b.prod:27019")
        );
        assert_eq!(shown(mongo, &["--nodb"]).as_deref(), Some(""));
        // Code or a file runs and exits — unless `--shell` keeps the prompt.
        for line in [
            &["--eval", "db.x()"][..],
            &["shop", "seed.js"],
            &["-f", "a.mongodb"],
        ] {
            assert_eq!(target(mongo, line), None, "{line:?}");
        }
        assert_eq!(
            shown(mongo, &["shop", "seed.js", "--shell"]).as_deref(),
            Some("/shop")
        );
    }

    #[test]
    fn mongosh_never_shows_a_password() {
        let mongo = Client::Mongo;
        let cases: [(&str, &[&str]); 9] = [
            ("-p", &["-u", "app", "-p", "hunter2", "shop"]),
            ("-p attached", &["-phunter2", "shop"]),
            ("--password", &["--password", "hunter2", "shop"]),
            ("--password=", &["--password=hunter2"]),
            ("uri", &["mongodb://app:hunter2@db/shop"]),
            (
                "uri query token",
                &["mongodb://db/shop?authMechanismProperties=AWS_SESSION_TOKEN:hunter2"],
            ),
            ("--host uri", &["--host", "mongodb://app:hunter2@db"]),
            (
                "--awsSecretAccessKey",
                &["--awsSecretAccessKey", "hunter2", "shop"],
            ),
            (
                "--tlsCertificateKeyFilePassword",
                &["--tlsCertificateKeyFilePassword", "hunter2"],
            ),
        ];
        for (case, line) in cases {
            assert_no_secret(mongo, line, &[], case);
        }
        // `-p` before an option asks for the password: it takes nothing.
        assert_eq!(
            shown(mongo, &["-u", "app", "-p", "--host", "db.prod"]).as_deref(),
            Some("app@db.prod")
        );
    }

    #[test]
    fn the_variables_hold_no_password() {
        for key in super::super::ENV_KEYS {
            for word in ["PASS", "PWD", "AUTH", "SECRET", "TOKEN", "KEY"] {
                assert!(!key.contains(word), "{key}");
            }
        }
    }

    #[test]
    fn a_control_character_drops_its_part_only() {
        assert_eq!(
            shown(Client::Postgres, &["-h", "db\u{1b}[31m", "main"]).as_deref(),
            Some("/main")
        );
        assert_eq!(
            shown(Client::Postgres, &["postgresql://db%0A.prod/main"]).as_deref(),
            Some("/main")
        );
    }
}
