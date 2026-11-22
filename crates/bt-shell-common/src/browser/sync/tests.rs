//! The sync against a home and a helper that are made for the tests: the home is a small server
//! that speaks the real wire over a socket pair (so what is sent is what the wire carries), the
//! helper is a table of cookies and folders that answers the commands the engine gives.
//!
//! Waiting is by event: every change of the fakes rings a bell, and a test waits until its
//! condition is true after a ring or fails after ten seconds. Nothing is slept.

use std::collections::HashMap;
use std::fs;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use browser_ipc::message::{Command, CookieValue, Event, IndexedDbFile, IndexedDbValue, Record};
use kablo_proto::profile::{
    self as wire, Client, Digest, Entry, Mode, Opened, Put, PutReply, PutResult, Server,
};

use super::ProfileSync;
use super::engine::{Config, Engine, Helper, Home, Input};
use super::gate::{Gate, Stage};
use super::records::{Id, id_of, record_digest, wire_of};
use super::store::Cache;
use crate::browser::depot::Depot;
use crate::browser::route::Link;

// ---- waiting ----

#[derive(Default)]
pub(crate) struct Bell {
    n: Mutex<u64>,
    cv: Condvar,
}

impl Bell {
    pub(crate) fn ring(&self) {
        *lock(&self.n) += 1;
        self.cv.notify_all();
    }

    pub(crate) fn until(&self, what: &str, condition: impl Fn() -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let seen = *lock(&self.n);
            if condition() {
                return;
            }
            let guard = lock(&self.n);
            if *guard != seen {
                continue;
            }
            let left = end
                .checked_duration_since(Instant::now())
                .unwrap_or_else(|| panic!("timed out waiting for {what}"));
            drop(
                self.cv
                    .wait_timeout(guard, left)
                    .unwrap_or_else(|e| e.into_inner()),
            );
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

// ---- records ----

pub(crate) fn cookie(name: &str, value: Option<&str>) -> Record {
    Record::Cookie {
        domain: "site.test".into(),
        path: "/".into(),
        name: name.into(),
        partition: None,
        value: value.map(|v| CookieValue {
            value: v.into(),
            expires: Some(4_000_000_000.0),
            secure: true,
            http_only: true,
            same_site: 0,
            priority: 1,
            source_scheme: 2,
            source_port: 443,
        }),
    }
}

fn storage(key: &str, value: Option<&str>) -> Record {
    Record::LocalStorage {
        origin: "https://site.test".into(),
        key: key.into(),
        value: value.map(str::to_owned),
    }
}

fn idb_record(origin: &str, files: &[(&str, &[u8])]) -> Record {
    Record::IndexedDb {
        origin: origin.into(),
        value: Some(IndexedDbValue {
            format: "chromium-idb-leveldb".into(),
            major: 156,
            files: files
                .iter()
                .map(|(path, bytes)| IndexedDbFile {
                    path: (*path).into(),
                    size: bytes.len() as u64,
                })
                .collect(),
        }),
    }
}

fn sums_of(files: &[(&str, &[u8])]) -> Vec<Digest> {
    files.iter().map(|(_, bytes)| wire::sum(bytes)).collect()
}

fn value_of(record: &Record) -> Option<String> {
    match record {
        Record::Cookie { value, .. } => value.as_ref().map(|v| v.value.clone()),
        Record::LocalStorage { value, .. } => value.clone(),
        Record::IndexedDb { value, .. } => value.as_ref().map(|v| v.format.clone()),
    }
}

// ---- the home ----

#[derive(Default)]
struct Store {
    depot: [u8; 16],
    head: u64,
    horizon: u64,
    entries: HashMap<Id, Entry>,
    blobs: HashMap<Digest, Vec<u8>>,
    parts: HashMap<Digest, Vec<u8>>,
    subs: Vec<Arc<Mutex<UnixStream>>>,
    conns: Vec<UnixStream>,
    puts: Vec<(Id, bool, u64)>,
    opens: Vec<wire::Open>,
    /// What the home answers an open with instead of the profile.
    refuse: Option<u16>,
    /// The home answers nothing until this is false.
    stall: bool,
}

#[derive(Clone)]
pub(crate) struct FakeHome {
    store: Arc<Mutex<Store>>,
    bell: Arc<Bell>,
}

fn say(out: &Arc<Mutex<UnixStream>>, message: &Server) -> bool {
    use std::io::Write;
    let mut stream = lock(out);
    wire::write_server(&mut *stream, message).is_ok() && stream.flush().is_ok()
}

impl FakeHome {
    /// The home answers nothing while it is stalled.
    pub(crate) fn stall(&self, stalled: bool) {
        lock(&self.store).stall = stalled;
        self.bell.ring();
    }

    /// How many times the home was opened.
    pub(crate) fn opens(&self) -> usize {
        lock(&self.store).opens.len()
    }

    pub(crate) fn new(bell: &Arc<Bell>) -> FakeHome {
        let store = Store {
            depot: [7; 16],
            ..Store::default()
        };
        FakeHome {
            store: Arc::new(Mutex::new(store)),
            bell: bell.clone(),
        }
    }

    /// Another computer wrote this (the home has it as that computer's).
    pub(crate) fn seed(&self, record: &Record, sums: &[Digest], blobs: &[(&str, &[u8])]) -> u64 {
        let wired = wire_of(record, sums);
        let mut store = lock(&self.store);
        for (_, bytes) in blobs {
            store.blobs.insert(wire::sum(bytes), bytes.to_vec());
        }
        if let (Some(bytes), Some(blob)) = (&wired.big_value, wired.body.value_blob) {
            store.blobs.insert(blob.sha256, bytes.clone());
        }
        store.head += 1;
        let entry = Entry {
            seq: store.head,
            body: wired.body,
            writer: [0xAA; 16],
            time: 0,
        };
        let id = (entry.body.kind, entry.body.key.clone());
        store.entries.insert(id, entry.clone());
        store
            .subs
            .retain(|out| say(out, &Server::Record(entry.clone())));
        let seq = entry.seq;
        drop(store);
        self.bell.ring();
        seq
    }

    /// An entry that points at files the home does not have.
    fn seed_without_blobs(&self, record: &Record, sums: &[Digest]) -> u64 {
        self.seed(record, sums, &[])
    }

    fn live(&self, record: &Record) -> bool {
        lock(&self.store)
            .entries
            .get(&id_of(record))
            .is_some_and(|e| !e.body.deleted)
    }

    fn deleted(&self, record: &Record) -> bool {
        lock(&self.store)
            .entries
            .get(&id_of(record))
            .is_some_and(|e| e.body.deleted)
    }

    fn cut(&self) {
        let conns: Vec<UnixStream> = std::mem::take(&mut lock(&self.store).conns);
        lock(&self.store).subs.clear();
        for conn in conns {
            let _ = conn.shutdown(std::net::Shutdown::Both);
        }
    }

    fn serve(&self, stream: UnixStream) {
        let store = self.store.clone();
        let bell = self.bell.clone();
        std::thread::spawn(move || {
            let Ok(mut reader) = stream.try_clone() else {
                return;
            };
            let out = Arc::new(Mutex::new(stream));
            {
                let mut stream = lock(&out);
                if wire::WIRE.server(&mut reader, &mut *stream).is_err() {
                    return;
                }
            }
            let mut client = [0u8; 16];
            while let Ok(Some(message)) = wire::read_client(&mut reader) {
                bell.until("the home to be let answer", || !lock(&store).stall);
                match message {
                    Client::Open(open) => {
                        let mut s = lock(&store);
                        s.opens.push(open.clone());
                        client = open.client;
                        if let Some(code) = s.refuse {
                            say(
                                &out,
                                &Server::Error(wire::Failure {
                                    code,
                                    text: "no".into(),
                                }),
                            );
                            continue;
                        }
                        let known = open.depot == Some(s.depot);
                        let since = if known { open.since } else { 0 };
                        let mode = if !known {
                            Mode::Full
                        } else if since > 0 && since < s.horizon {
                            Mode::FullReplace
                        } else {
                            Mode::Diff
                        };
                        say(
                            &out,
                            &Server::Opened(Opened {
                                head: s.head,
                                horizon: s.horizon,
                                mode,
                                depot: s.depot,
                            }),
                        );
                        let mut list: Vec<Entry> = s
                            .entries
                            .values()
                            .filter(|e| mode != Mode::Diff || e.seq > since)
                            .cloned()
                            .collect();
                        list.sort_by_key(|e| e.seq);
                        for entry in list {
                            say(&out, &Server::Record(entry));
                        }
                        say(&out, &Server::CaughtUp { head: s.head });
                        s.subs.push(out.clone());
                        if let Ok(conn) = lock(&out).try_clone() {
                            s.conns.push(conn);
                        }
                    }
                    Client::Put(put) => {
                        let mut s = lock(&store);
                        handle_put(&mut s, &out, client, put);
                    }
                    Client::Have(digests) => {
                        let s = lock(&store);
                        let lacking = digests
                            .into_iter()
                            .filter(|d| !s.blobs.contains_key(d))
                            .collect();
                        say(&out, &Server::HaveReply(lacking));
                    }
                    Client::Blob(part) => {
                        let mut s = lock(&store);
                        let have = s.parts.entry(part.sha256).or_default();
                        if have.len() as u64 == part.start {
                            have.extend_from_slice(&part.bytes);
                        }
                        if have.len() as u64 >= part.total {
                            let whole = s.parts.remove(&part.sha256).unwrap_or_default();
                            if wire::sum(&whole) == part.sha256 {
                                s.blobs.insert(part.sha256, whole);
                                say(&out, &Server::BlobOk(part.sha256));
                            }
                        }
                    }
                    Client::GetBlob(digest) => {
                        let s = lock(&store);
                        match s.blobs.get(&digest) {
                            Some(bytes) => {
                                say(
                                    &out,
                                    &Server::Blob(wire::Part {
                                        sha256: digest,
                                        total: bytes.len() as u64,
                                        start: 0,
                                        bytes: bytes.clone(),
                                    }),
                                );
                            }
                            None => {
                                say(&out, &Server::BlobMissing(digest));
                            }
                        }
                    }
                }
                bell.ring();
            }
        });
    }
}

fn handle_put(s: &mut Store, out: &Arc<Mutex<UnixStream>>, client: [u8; 16], put: Put) {
    let id: Id = (put.body.kind, put.body.key.clone());
    s.puts.push((id.clone(), put.body.deleted, put.base));
    let current = s.entries.get(&id).map_or(0, |e| e.seq);
    let reply = |result, seq| {
        Server::PutReply(PutReply {
            req: put.req,
            result,
            seq,
        })
    };
    if put.body.blobs().any(|b| !s.blobs.contains_key(&b.sha256)) {
        say(out, &reply(PutResult::MissingBlob, 0));
    } else if current != put.base {
        say(out, &reply(PutResult::Conflict, current));
    } else {
        s.head += 1;
        let entry = Entry {
            seq: s.head,
            body: put.body,
            writer: client,
            time: 0,
        };
        s.entries.insert(id, entry.clone());
        say(out, &reply(PutResult::Done, entry.seq));
        s.subs
            .retain(|sub| say(sub, &Server::Record(entry.clone())));
    }
}

pub(crate) struct FakeDepot {
    home: FakeHome,
    refuse: AtomicBool,
    /// The first this many openings fail.
    failures: AtomicU32,
}

impl Depot for FakeDepot {
    fn open(&self, _wait: Duration) -> Result<Link, String> {
        if self.refuse.load(Ordering::Relaxed) {
            return Err("no home".into());
        }
        if self
            .failures
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err("not yet".into());
        }
        let (client, server) = UnixStream::pair().map_err(|e| e.to_string())?;
        self.home.serve(server);
        let closing = client.try_clone().map_err(|e| e.to_string())?;
        Ok(Link::new(
            client.try_clone().map_err(|e| e.to_string())?,
            client,
            move || {
                let _ = closing.shutdown(std::net::Shutdown::Both);
            },
            None,
        ))
    }
}

pub(crate) fn depot(home: &FakeHome) -> Arc<FakeDepot> {
    Arc::new(FakeDepot {
        home: home.clone(),
        refuse: AtomicBool::new(false),
        failures: AtomicU32::new(0),
    })
}

// ---- the helper ----

#[derive(Default)]
struct Table {
    /// Cookies and values of Local Storage.
    records: HashMap<Id, Record>,
    /// The folders of IndexedDB: origin → files.
    idb: HashMap<String, Vec<(String, Vec<u8>)>>,
    /// Every batch the engine put in with `ProfileWrite`.
    writes: Vec<Vec<Record>>,
    idb_writes: Vec<Vec<Record>>,
    /// The engine of the profile runs: a folder is not put under it.
    running: bool,
    watching: Option<u32>,
    stages_dropped: Vec<String>,
    commands: Vec<String>,
    stage_seq: u32,
}

struct Fake {
    commands: Sender<Command>,
}

impl Helper for Fake {
    fn send(&mut self, command: Command) {
        let _ = self.commands.send(command);
    }
}

struct Mac {
    sync: Arc<ProfileSync>,
    table: Arc<Mutex<Table>>,
    root: PathBuf,
    state: PathBuf,
    bell: Arc<Bell>,
    /// The folder stays when this is dropped (a second run starts from it).
    keep: AtomicBool,
}

const BROWSER: u32 = 1;

fn config(root: &std::path::Path) -> Config {
    Config {
        dir: "bateri-main-0123456789abcdef".into(),
        app: "bateri".into(),
        name: "main".into(),
        root: root.join("profiles"),
        state: root.join("sync").join("main"),
        cache: Cache::new(root.join("blobs")),
    }
}

impl Mac {
    fn new(tag: &str, depot: Arc<dyn Depot>, bell: &Arc<Bell>) -> Mac {
        Mac::with(tag, depot, bell, Table::default())
    }

    /// A Mac whose profile has this from before.
    fn with(tag: &str, depot: Arc<dyn Depot>, bell: &Arc<Bell>, table: Table) -> Mac {
        let root = std::env::temp_dir().join(format!("bt-sync-mac-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("profiles")).unwrap();
        Mac::start(root, depot, bell, table)
    }

    fn start(root: PathBuf, depot: Arc<dyn Depot>, bell: &Arc<Bell>, table: Table) -> Mac {
        let table = Arc::new(Mutex::new(table));
        let (commands, received) = channel();
        let reqs = Arc::new(AtomicU32::new(1));
        let sync =
            ProfileSync::start(config(&root), Box::new(Fake { commands }), depot, reqs).unwrap();
        let (hand, handed) = channel::<Arc<ProfileSync>>();
        let (state, bell_for_thread, table_for_thread, root_for_thread) = (
            root.join("sync").join("main"),
            bell.clone(),
            table.clone(),
            root.clone(),
        );
        std::thread::spawn(move || {
            let Ok(sync) = handed.recv() else { return };
            for command in received {
                answer(&sync, &table_for_thread, &root_for_thread, command);
                bell_for_thread.ring();
            }
        });
        hand.send(sync.clone()).unwrap();
        Mac {
            sync,
            table,
            root,
            state,
            bell: bell.clone(),
            keep: AtomicBool::new(false),
        }
    }

    /// Ends the sync and waits until what it keeps is written.
    fn end(&self) {
        self.sync.stop();
        assert!(self.sync.ended(Duration::from_secs(5)), "the sync ends");
    }

    fn attach(&self) {
        self.sync.attach(BROWSER);
    }

    /// The page changed this while the profile is watched.
    fn changes(&self, record: Record) {
        {
            let mut table = lock(&self.table);
            if matches!(
                record,
                Record::Cookie { value: None, .. } | Record::LocalStorage { value: None, .. }
            ) {
                table.records.remove(&id_of(&record));
            } else {
                table.records.insert(id_of(&record), record.clone());
            }
        }
        self.sync.event(Event::ProfileChanged {
            browser: BROWSER,
            records: vec![record],
            stage: None,
        });
        self.bell.ring();
    }

    fn value(&self, record: &Record) -> Option<String> {
        lock(&self.table)
            .records
            .get(&id_of(record))
            .and_then(value_of)
    }

    fn folder(&self, origin: &str) -> Option<Vec<(String, Vec<u8>)>> {
        lock(&self.table).idb.get(origin).cloned()
    }
}

impl Drop for Mac {
    fn drop(&mut self) {
        // It saves what it keeps as it ends: the folder goes after that, or it would be made again.
        self.sync.stop();
        let _ = self.sync.ended(Duration::from_secs(5));
        if !self.keep.load(Ordering::Relaxed) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

/// What the helper does with a command.
fn answer(
    sync: &Arc<ProfileSync>,
    table: &Arc<Mutex<Table>>,
    root: &std::path::Path,
    command: Command,
) {
    let mut t = lock(table);
    t.commands
        .push(format!("{command:?}").chars().take(24).collect());
    match command {
        Command::IndexedDbRead { req, .. } => {
            t.stage_seq += 1;
            let stage = format!(".stage/fake-{}", t.stage_seq);
            let mut records = Vec::new();
            for (origin, files) in &t.idb {
                let at = root.join("profiles").join(&stage).join(origin);
                for (path, bytes) in files {
                    let to = at.join(path);
                    fs::create_dir_all(to.parent().unwrap()).unwrap();
                    fs::write(to, bytes).unwrap();
                }
                records.push(Record::IndexedDb {
                    origin: origin.clone(),
                    value: Some(IndexedDbValue {
                        format: "chromium-idb-leveldb".into(),
                        major: 156,
                        files: files
                            .iter()
                            .map(|(p, b)| IndexedDbFile {
                                path: p.clone(),
                                size: b.len() as u64,
                            })
                            .collect(),
                    }),
                });
            }
            fs::create_dir_all(root.join("profiles").join(&stage)).unwrap();
            drop(t);
            sync.event(Event::IndexedDbRecords {
                req,
                records,
                stage,
                error: None,
            });
        }
        Command::IndexedDbWrite {
            req,
            stage,
            records,
            ..
        } => {
            let mut deferred = 0;
            let mut written = 0;
            for record in &records {
                let Record::IndexedDb { origin, value } = record else {
                    continue;
                };
                if t.running {
                    deferred += 1;
                    continue;
                }
                match value {
                    None => {
                        t.idb.remove(origin);
                    }
                    Some(value) => {
                        let files = value
                            .files
                            .iter()
                            .map(|f| {
                                let from = root
                                    .join("profiles")
                                    .join(&stage)
                                    .join(origin)
                                    .join(&f.path);
                                (f.path.clone(), fs::read(from).unwrap())
                            })
                            .collect();
                        t.idb.insert(origin.clone(), files);
                    }
                }
                written += 1;
            }
            t.idb_writes.push(records);
            drop(t);
            sync.event(Event::IndexedDbWritten {
                req,
                written,
                deferred,
                error: None,
            });
        }
        Command::ProfileRead { browser, req } => {
            let records = t.records.values().cloned().collect();
            drop(t);
            sync.event(Event::ProfileRecords {
                browser,
                req,
                records,
                error: None,
            });
        }
        Command::ProfileWrite {
            browser,
            req,
            records,
        } => {
            for record in &records {
                match record {
                    Record::Cookie { value: None, .. }
                    | Record::LocalStorage { value: None, .. } => {
                        t.records.remove(&id_of(record));
                    }
                    other => {
                        t.records.insert(id_of(other), other.clone());
                    }
                }
            }
            t.writes.push(records);
            drop(t);
            sync.event(Event::ProfileWritten {
                browser,
                req,
                deferred: 0,
                error: None,
            });
        }
        Command::ProfileWatch { browser, on } => {
            t.watching = on.then_some(browser);
        }
        Command::StageDrop { stage } => {
            let _ = fs::remove_dir_all(root.join("profiles").join(&stage));
            t.stages_dropped.push(stage);
        }
        _ => {}
    }
}

type Files<'a> = &'a [(&'a str, &'a [u8])];

fn table(records: &[Record], folders: &[(&str, Files)]) -> Table {
    Table {
        records: records.iter().map(|r| (id_of(r), r.clone())).collect(),
        idb: folders
            .iter()
            .map(|(origin, files)| {
                (
                    (*origin).to_owned(),
                    files
                        .iter()
                        .map(|(p, b)| ((*p).to_owned(), b.to_vec()))
                        .collect(),
                )
            })
            .collect(),
        ..Table::default()
    }
}

fn started(bell: &Arc<Bell>) -> (FakeHome, Arc<FakeDepot>) {
    let home = FakeHome::new(bell);
    let depot = depot(&home);
    (home, depot)
}

fn joined(mac: &Mac) -> bool {
    mac.sync.gate().released(Stage::AfterOpen)
}

// ---- the tests ----

#[test]
fn an_empty_profile_takes_what_the_home_has_and_deletes_nothing() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    home.seed(&cookie("sid", Some("home-1")), &[], &[]);
    home.seed(&storage("theme", Some("dark")), &[], &[]);
    let mac = Mac::new("empty", depot, &bell);
    mac.attach();
    bell.until("the join", || joined(&mac));
    assert_eq!(mac.value(&cookie("sid", None)).as_deref(), Some("home-1"));
    assert_eq!(mac.value(&storage("theme", None)).as_deref(), Some("dark"));
    // Nothing of the home was said gone, and nothing was said at all.
    assert!(lock(&home.store).puts.is_empty());
    assert!(home.live(&cookie("sid", None)) && home.live(&storage("theme", None)));
}

#[test]
fn a_profile_the_home_has_never_heard_of_is_given_to_it() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    let mac = Mac::with(
        "give",
        depot,
        &bell,
        table(&[cookie("sid", Some("mac-1"))], &[]),
    );
    mac.attach();
    bell.until("the home has it", || home.live(&cookie("sid", None)));
    // New to the home: written on base 0.
    assert_eq!(lock(&home.store).puts[0].2, 0);
}

#[test]
fn what_one_machine_does_the_other_sees_and_a_deletion_too() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    let a = Mac::with(
        "two-a",
        depot.clone(),
        &bell,
        table(&[cookie("sid", Some("one"))], &[]),
    );
    let b = Mac::new("two-b", depot, &bell);
    a.attach();
    bell.until("a joined", || joined(&a));
    bell.until("the home has it", || home.live(&cookie("sid", None)));
    b.attach();
    bell.until("b joined", || joined(&b));
    bell.until("b has the cookie", || {
        b.value(&cookie("sid", None)).as_deref() == Some("one")
    });

    // A rotating token: one change, seen live at the other.
    a.changes(cookie("sid", Some("two")));
    bell.until("b sees the new token", || {
        b.value(&cookie("sid", None)).as_deref() == Some("two")
    });
    // And back, from the other side, on the number it took.
    b.changes(storage("seen", Some("by b")));
    bell.until("a sees b's storage", || {
        a.value(&storage("seen", None)).as_deref() == Some("by b")
    });
    // A deletion goes the same way.
    a.changes(cookie("sid", None));
    bell.until("b loses it", || b.value(&cookie("sid", None)).is_none());
    assert!(home.deleted(&cookie("sid", None)));
}

#[test]
fn a_machine_that_was_away_catches_up_and_says_what_it_did_meanwhile() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    let a = Mac::with(
        "away",
        depot,
        &bell,
        table(&[cookie("sid", Some("one"))], &[]),
    );
    a.attach();
    bell.until("joined", || joined(&a));
    bell.until("the home has it", || home.live(&cookie("sid", None)));
    let opens = lock(&home.store).opens.len();

    home.cut();
    // The home changes one thing and the Mac another while apart.
    home.seed(&storage("theirs", Some("yes")), &[], &[]);
    a.changes(cookie("mine", Some("later")));
    bell.until("it connects again", || {
        lock(&home.store).opens.len() > opens
    });
    bell.until("the Mac has theirs", || {
        a.value(&storage("theirs", None)).as_deref() == Some("yes")
    });
    bell.until("the home has mine", || home.live(&cookie("mine", None)));
    // The second open asked from what the first had taken, not from the start.
    let store = lock(&home.store);
    let second = &store.opens[opens];
    assert!(second.since > 0, "since {}", second.since);
    assert_eq!(second.depot, Some([7; 16]));
}

#[test]
fn a_home_that_is_not_there_lets_the_browser_go_on_and_the_profile_joins_when_it_is() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    home.seed(&cookie("sid", Some("home")), &[], &[]);
    depot.failures.store(1, Ordering::Relaxed);
    let mac = Mac::new("late", depot, &bell);
    mac.attach();
    // The first attempt fails: the browser is not held for it.
    assert!(
        mac.sync
            .gate()
            .wait(Stage::AfterOpen, Instant::now() + Duration::from_secs(5))
    );
    // The later attempt works, and the profile gets what the home has.
    bell.until("the profile joined", || {
        mac.value(&cookie("sid", None)).as_deref() == Some("home")
    });
}

#[test]
fn a_home_that_is_slow_does_not_hold_the_first_page_longer_than_it_is_waited_for() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    home.seed(&cookie("sid", Some("home")), &[], &[]);
    lock(&home.store).stall = true;
    let mac = Mac::new("slow", depot, &bell);
    mac.attach();
    let waited = Instant::now();
    assert!(
        !mac.sync.gate().wait(
            Stage::BeforeOpen,
            Instant::now() + Duration::from_millis(150)
        ),
        "nothing has come"
    );
    assert!(waited.elapsed() < Duration::from_secs(2));
    // The browser went on; when the home answers the profile joins all the same.
    lock(&home.store).stall = false;
    bell.ring();
    bell.until("the profile joined", || {
        mac.value(&cookie("sid", None)).as_deref() == Some("home")
    });
    bell.until("the join is over", || joined(&mac));
}

#[test]
fn a_home_that_turns_the_machine_away_is_not_asked_again() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    lock(&home.store).refuse = Some(wire::code::OFF);
    let mac = Mac::new("off", depot, &bell);
    mac.attach();
    assert!(
        mac.sync
            .gate()
            .wait(Stage::AfterOpen, Instant::now() + Duration::from_secs(5)),
        "the browser goes on alone"
    );
    bell.until("the open", || !lock(&home.store).opens.is_empty());
    // No other try follows.
    assert_eq!(lock(&home.store).opens.len(), 1);
}

#[test]
fn an_indexeddb_goes_both_ways_by_its_files_and_a_large_value_as_a_blob() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    let files: &[(&str, &[u8])] = &[
        ("leveldb/000003.log", b"log bytes"),
        ("blob/1/00/1", b"a blob"),
        ("leveldb/EMPTY", b""),
    ];
    let big = "x".repeat(70_000);
    let a = Mac::with(
        "idb-a",
        depot.clone(),
        &bell,
        table(
            &[storage("big", Some(&big))],
            &[("https_site.test_0", files)],
        ),
    );
    a.attach();
    bell.until("a joined", || joined(&a));
    bell.until("the home has the folder", || {
        home.live(&idb_record("https_site.test_0", files))
    });
    bell.until("the home has the value", || {
        home.live(&storage("big", None))
    });
    assert!(
        lock(&home.store).blobs.len() >= 4,
        "three files and one value"
    );

    // A second Mac that opens now finds the folder before it opens the profile.
    let b = Mac::new("idb-b", depot, &bell);
    b.attach();
    bell.until("b joined", || joined(&b));
    let got = b.folder("https_site.test_0").expect("the folder is there");
    let mut got: Vec<_> = got.into_iter().collect();
    got.sort();
    let mut want: Vec<_> = files
        .iter()
        .map(|(p, bytes)| ((*p).to_owned(), bytes.to_vec()))
        .collect();
    want.sort();
    assert_eq!(got, want);
    assert_eq!(
        b.value(&storage("big", None)).as_deref(),
        Some(big.as_str())
    );
    // Both stages were given back.
    assert!(!lock(&b.table).stages_dropped.is_empty());
}

#[test]
fn an_indexeddb_whose_files_the_home_lacks_is_left_for_later_and_the_open_goes_on() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    let files: &[(&str, &[u8])] = &[("leveldb/000003.log", b"never uploaded")];
    let seq = home.seed_without_blobs(&idb_record("https_site.test_0", files), &sums_of(files));
    home.seed(&cookie("sid", Some("home")), &[], &[]);
    let mac = Mac::new("missing", depot, &bell);
    mac.attach();
    // The page is not held for it: the first stage and the second go.
    bell.until("the join", || joined(&mac));
    assert!(mac.folder("https_site.test_0").is_none());
    assert!(lock(&mac.table).idb_writes.is_empty());
    assert_eq!(mac.value(&cookie("sid", None)).as_deref(), Some("home"));
    // `since` does not pass the record that is not in: it comes again at the next join.
    mac.end();
    assert!(super::store::Saved::load(&mac.state).since < seq);
}

#[test]
fn a_folder_cannot_go_under_a_running_engine_and_is_brought_again() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    let files: &[(&str, &[u8])] = &[("leveldb/000003.log", b"from home")];
    let seq = home.seed(
        &idb_record("https_site.test_0", files),
        &sums_of(files),
        files,
    );
    let mac = Mac::new("deferred", depot, &bell);
    lock(&mac.table).running = true;
    mac.attach();
    bell.until("the join", || joined(&mac));
    assert!(mac.folder("https_site.test_0").is_none());
    mac.end();
    assert!(super::store::Saved::load(&mac.state).since < seq);
    // The record is not taken for known either: it is not said back to the home as the Mac's.
    assert!(lock(&home.store).puts.is_empty());
}

#[test]
fn what_is_known_is_kept_for_the_next_run() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    home.seed(&cookie("sid", Some("home")), &[], &[]);
    let root;
    {
        let mac = Mac::new("again", depot.clone(), &bell);
        mac.attach();
        bell.until("the join", || joined(&mac));
        // Saved once a second at most: the end of the sync saves too.
        mac.end();
        mac.keep.store(true, Ordering::Relaxed);
        root = mac.root.clone();
        let saved = super::store::Saved::load(&mac.state);
        assert_eq!(saved.depot, Some([7; 16]));
        assert_eq!(saved.known.len(), 1);
        // The record's sum is what the Mac has of it.
        assert_eq!(
            saved.known.values().next().unwrap().digest,
            record_digest(&cookie("sid", Some("home")), &[])
        );
    }
    let table = Table {
        records: [(id_of(&cookie("sid", None)), cookie("sid", Some("home")))].into(),
        ..Table::default()
    };
    let again = Mac::start(root, depot, &bell, table);
    again.attach();
    bell.until("the second join", || joined(&again));
    // Asked from where the first run stopped, and nothing written or said.
    let store = lock(&home.store);
    assert_eq!(store.opens.last().unwrap().since, 1);
    assert!(store.puts.is_empty());
    assert!(lock(&again.table).writes.is_empty());
}

// ---- the engine alone: what must be in order ----

#[derive(Clone, Default)]
struct Said(Arc<Mutex<Vec<Client>>>);

impl Home for Said {
    fn send(&mut self, message: &Client) -> std::io::Result<()> {
        lock(&self.0).push(message.clone());
        Ok(())
    }
}

#[derive(Clone, Default)]
struct Asked(Arc<Mutex<Vec<Command>>>);

impl Helper for Asked {
    fn send(&mut self, command: Command) {
        lock(&self.0).push(command);
    }
}

fn rig(tag: &str) -> (Engine, Said, Asked, Arc<Gate>, PathBuf) {
    let root = std::env::temp_dir().join(format!("bt-sync-rig-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("profiles")).unwrap();
    let (said, asked, gate) = (Said::default(), Asked::default(), Arc::new(Gate::new()));
    let engine = Engine::new(
        config(&root),
        Box::new(asked.clone()),
        gate.clone(),
        Arc::new(AtomicU32::new(1)),
    );
    (engine, said, asked, gate, root)
}

fn opened(mode: Mode, head: u64) -> Server {
    Server::Opened(Opened {
        head,
        horizon: 0,
        mode,
        depot: [7; 16],
    })
}

fn entry_of(seq: u64, record: &Record) -> Entry {
    Entry {
        seq,
        body: wire_of(record, &[]).body,
        writer: [0xAA; 16],
        time: 0,
    }
}

/// The helper's answers to the commands the engine asks that it waits for, until it asks no more.
fn reply(engine: &mut Engine, asked: &Asked, from: usize) -> usize {
    let mut from = from;
    loop {
        let commands: Vec<Command> = lock(&asked.0).iter().skip(from).cloned().collect();
        if commands.is_empty() {
            return from;
        }
        from += commands.len();
        for command in commands {
            match command {
                Command::IndexedDbRead { req, .. } => {
                    engine.handle(Input::Helper(Event::IndexedDbRecords {
                        req,
                        records: Vec::new(),
                        stage: ".stage/rig".into(),
                        error: None,
                    }));
                }
                Command::ProfileRead { browser, req } => {
                    engine.handle(Input::Helper(Event::ProfileRecords {
                        browser,
                        req,
                        records: Vec::new(),
                        error: None,
                    }));
                }
                Command::ProfileWrite { browser, req, .. } => {
                    engine.handle(Input::Helper(Event::ProfileWritten {
                        browser,
                        req,
                        deferred: 0,
                        error: None,
                    }));
                }
                _ => {}
            }
        }
    }
}

#[test]
fn after_a_loss_the_profile_asks_nothing_of_the_pages_until_the_difference_is_in() {
    let (mut engine, said, asked, gate, root) = rig("hold");
    engine.handle(Input::Attach(BROWSER));
    engine.connected(Box::new(said.clone()));
    engine.handle(Input::Home(opened(Mode::Full, 0)));
    engine.handle(Input::Home(Server::CaughtUp { head: 0 }));
    let mut seen = reply(&mut engine, &asked, 0);
    seen = reply(&mut engine, &asked, seen);
    assert!(engine.is_live());
    assert!(
        gate.wait_released(Instant::now()),
        "nothing held the first time"
    );

    // The home is lost and found again: the requests wait for what happened meanwhile.
    engine.lost();
    engine.connected(Box::new(said.clone()));
    assert!(
        !gate.wait_released(Instant::now()),
        "held from the moment the Mac is back"
    );
    engine.handle(Input::Home(opened(Mode::Diff, 5)));
    engine.handle(Input::Home(Server::Record(entry_of(
        5,
        &storage("k", Some("v")),
    ))));
    assert!(
        !gate.wait_released(Instant::now()),
        "still held: the diff is not in"
    );
    engine.handle(Input::Home(Server::CaughtUp { head: 5 }));
    assert!(
        !gate.wait_released(Instant::now()),
        "still held: the diff is not written"
    );
    seen = reply(&mut engine, &asked, seen);
    seen = reply(&mut engine, &asked, seen);
    let _ = seen;
    assert!(engine.is_live());
    assert!(gate.wait_released(Instant::now()), "let go when it is in");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_connection_that_ends_before_the_first_answer_lets_everything_go() {
    let (mut engine, said, _asked, gate, root) = rig("never");
    engine.handle(Input::Attach(BROWSER));
    engine.connected(Box::new(said));
    assert!(!gate.released(Stage::BeforeOpen));
    engine.lost();
    assert!(gate.released(Stage::BeforeOpen) && gate.released(Stage::AfterOpen));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_record_that_comes_with_the_peers_number_is_not_said_back() {
    let (mut engine, said, asked, _gate, root) = rig("echo");
    engine.handle(Input::Attach(BROWSER));
    engine.connected(Box::new(said.clone()));
    engine.handle(Input::Home(opened(Mode::Full, 0)));
    engine.handle(Input::Home(Server::CaughtUp { head: 0 }));
    let mut seen = reply(&mut engine, &asked, 0);
    seen = reply(&mut engine, &asked, seen);
    assert!(engine.is_live());
    // The home streams a record: it is written into the profile and then known.
    let record = cookie("sid", Some("theirs"));
    engine.handle(Input::Home(Server::Record(entry_of(3, &record))));
    seen = reply(&mut engine, &asked, seen);
    let _ = seen;
    assert_eq!(engine.known(&id_of(&record)).map(|k| k.applied), Some(3));
    // The helper does not say back what was written; but if it were to say the same value, the
    // Mac would not send it either.
    engine.handle(Input::Helper(Event::ProfileChanged {
        browser: BROWSER,
        records: vec![record],
        stage: None,
    }));
    assert!(
        !lock(&said.0).iter().any(|m| matches!(m, Client::Put(_))),
        "nothing was put"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_conflict_that_says_there_was_no_such_record_is_written_again_as_new() {
    let (mut engine, said, asked, _gate, root) = rig("conflict");
    engine.handle(Input::Attach(BROWSER));
    engine.connected(Box::new(said.clone()));
    engine.handle(Input::Home(opened(Mode::Full, 0)));
    engine.handle(Input::Home(Server::CaughtUp { head: 0 }));
    let seen = reply(&mut engine, &asked, 0);
    // The home streams the record (number 4); the Mac then changes it.
    engine.handle(Input::Home(Server::Record(entry_of(
        4,
        &cookie("sid", Some("zero")),
    ))));
    let _ = reply(&mut engine, &asked, seen);
    engine.handle(Input::Helper(Event::ProfileChanged {
        browser: BROWSER,
        records: vec![cookie("sid", Some("one"))],
        stage: None,
    }));
    let put = |said: &Said| {
        lock(&said.0)
            .iter()
            .filter_map(|m| match m {
                Client::Put(p) => Some((p.req, p.base)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let first = put(&said);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].1, 4, "on the number it was taken at");
    // "No such record" (seq 0): again, on base 0.
    engine.handle(Input::Home(Server::PutReply(PutReply {
        req: first[0].0,
        result: PutResult::Conflict,
        seq: 0,
    })));
    let all = put(&said);
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].1, 0);
    // A conflict with a number is the home's word: not said again.
    engine.handle(Input::Home(Server::PutReply(PutReply {
        req: all[1].0,
        result: PutResult::Conflict,
        seq: 9,
    })));
    assert_eq!(put(&said).len(), 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_requests_of_a_held_profile_wait_in_the_proxy_and_the_others_go() {
    use crate::browser::profile::ProfileKey;
    use crate::browser::proxy::Holds;
    let bell = Arc::new(Bell::default());
    let (_home, depot) = started(&bell);
    let mac = Mac::new("hold", depot, &bell);
    let key = ProfileKey::new("server:x", "bateri", Some("main")).unwrap();
    let other = ProfileKey::new("server:x", "bateri", Some("other")).unwrap();
    let syncs = super::Syncs::new();
    syncs
        .join(&key.dir().unwrap(), BROWSER, |_| Ok(mac.sync.clone()))
        .unwrap();
    assert_eq!(syncs.len(), 1);
    let gate = mac.sync.gate().clone();
    gate.hold(true);
    let (done, heard) = channel();
    let (held, syncs_for_thread) = (key.clone(), syncs.clone());
    std::thread::spawn(move || {
        syncs_for_thread.hold(&held);
        let _ = done.send(());
    });
    // A profile with no sync is not held.
    syncs.hold(&other);
    assert!(heard.recv_timeout(Duration::from_millis(150)).is_err());
    gate.hold(false);
    heard.recv_timeout(Duration::from_secs(5)).unwrap();
    // The last browser to leave ends the sync.
    syncs.leave(BROWSER);
    assert_eq!(syncs.len(), 0);
}
