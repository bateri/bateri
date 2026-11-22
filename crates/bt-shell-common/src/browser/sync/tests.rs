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
use crate::browser::depot::{Depot, Mark, Turn};
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
    /// The person's switch for the machine (a depot of a machine that keeps none is not opened).
    pub(crate) keep: AtomicBool,
    /// The first this many tellings of the switch fail.
    pub(crate) switch_failures: AtomicU32,
    /// The far end ends at once and says no hello (a server with no kablo that keeps profiles).
    pub(crate) mute: AtomicBool,
    /// What the machine was told, in order: the switch, and how many times the home had been
    /// opened and had a live record when it was.
    pub(crate) told: Mutex<Vec<Told>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Told {
    pub(crate) on: bool,
    pub(crate) opens: usize,
}

impl Depot for FakeDepot {
    fn kept(&self) -> bool {
        self.keep.load(Ordering::Relaxed)
    }

    /// What the server's kablo does: off deletes what it keeps and leaves the mark (the home
    /// turns every open away), on takes the mark away.
    fn switch(&self, on: bool, _wait: Duration) -> Result<(), String> {
        let mut told = lock(&self.told);
        if self
            .switch_failures
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err("not yet".into());
        }
        {
            let mut store = lock(&self.home.store);
            told.push(Told {
                on,
                opens: store.opens.len(),
            });
            if on {
                store.refuse = None;
            } else {
                store.entries.clear();
                store.refuse = Some(wire::code::OFF);
            }
        }
        drop(told);
        self.home.bell.ring();
        Ok(())
    }

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
        if self.mute.load(Ordering::Relaxed) {
            drop(server);
        } else {
            self.home.serve(server);
        }
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
        keep: AtomicBool::new(true),
        switch_failures: AtomicU32::new(0),
        mute: AtomicBool::new(false),
        told: Mutex::new(Vec::new()),
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
    /// The hosts whose data the helper was told to delete.
    site_clears: Vec<String>,
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
        Mac::turned(tag, depot, bell, table, |_| None)
    }

    /// A Mac whose person has turned the switch of the machine: `turn` is told where the Mac's
    /// note of the switch is.
    fn turned(
        tag: &str,
        depot: Arc<dyn Depot>,
        bell: &Arc<Bell>,
        table: Table,
        turn: impl FnOnce(&Mark) -> Option<Turn>,
    ) -> Mac {
        let root = std::env::temp_dir().join(format!("bt-sync-mac-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("profiles")).unwrap();
        let turn = turn(&Mark::new(&root, "server:test"));
        Mac::start(root, depot, bell, table, turn)
    }

    /// The note of the switch this Mac keeps for its machine.
    fn mark(&self) -> Mark {
        Mark::new(&self.root, "server:test")
    }

    fn start(
        root: PathBuf,
        depot: Arc<dyn Depot>,
        bell: &Arc<Bell>,
        table: Table,
        turn: Option<Turn>,
    ) -> Mac {
        let table = Arc::new(Mutex::new(table));
        let (commands, received) = channel();
        let reqs = Arc::new(AtomicU32::new(1));
        let sync = ProfileSync::start(
            config(&root),
            Box::new(Fake { commands }),
            depot,
            reqs,
            turn,
        )
        .unwrap();
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

    /// The person cleared the data of `host` in the browser.
    fn clears(&self, host: &str) {
        self.sync.clear_site(BROWSER, 77, host.to_owned());
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
        Command::ClearSiteData { browser, req, host } => {
            // The engine's own: it deletes the cookies and the storage it has of the host and says
            // what it found, each as a record with no value.
            let gone: Vec<Record> = t
                .records
                .values()
                .filter(|record| super::site::is_the_hosts(record, &host))
                .map(super::records::without_value)
                .collect();
            for record in &gone {
                t.records.remove(&id_of(record));
            }
            t.site_clears.push(host);
            drop(t);
            sync.event(Event::SiteCleared {
                browser,
                req,
                records: gone,
                error: None,
            });
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

fn cookie_of(domain: &str, name: &str, value: &str) -> Record {
    let Record::Cookie {
        path,
        partition,
        value: kept,
        ..
    } = cookie(name, Some(value))
    else {
        unreachable!()
    };
    Record::Cookie {
        domain: domain.into(),
        path,
        name: name.into(),
        partition,
        value: kept,
    }
}

#[test]
fn a_site_that_is_cleared_is_deleted_at_the_home_and_at_the_other_mac_and_the_others_stay() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    let a = Mac::with(
        "site-a",
        depot.clone(),
        &bell,
        table(
            &[
                cookie("sid", Some("one")),
                storage("seen", Some("here")),
                cookie_of("other.test", "keep", "me"),
            ],
            &[],
        ),
    );
    let b = Mac::new("site-b", depot, &bell);
    a.attach();
    bell.until("a joined", || joined(&a));
    bell.until("the home has them", || {
        home.live(&cookie("sid", None))
            && home.live(&storage("seen", None))
            && home.live(&cookie_of("other.test", "keep", "x"))
    });
    // A port of the site that the helper never saw: the home has it and the Mac took it, and the
    // engine of this Mac does not list it.
    a.changes(storage("ghost", Some("on a port nobody opened")));
    bell.until("the home has the ghost", || {
        home.live(&storage("ghost", None))
    });
    lock(&a.table)
        .records
        .remove(&id_of(&storage("ghost", None)));
    b.attach();
    bell.until("b has all of it", || {
        b.value(&cookie("sid", None)).is_some()
            && b.value(&storage("ghost", None)).is_some()
            && b.value(&cookie_of("other.test", "keep", "x")).is_some()
    });

    a.clears("site.test");
    bell.until("the home has them deleted", || {
        home.deleted(&cookie("sid", None))
            && home.deleted(&storage("seen", None))
            && home.deleted(&storage("ghost", None))
    });
    // The other Mac loses the site (it need not have had it open) and keeps the rest.
    bell.until("b loses the site", || {
        b.value(&cookie("sid", None)).is_none()
            && b.value(&storage("seen", None)).is_none()
            && b.value(&storage("ghost", None)).is_none()
    });
    assert_eq!(
        b.value(&cookie_of("other.test", "keep", "x")).as_deref(),
        Some("me")
    );
    assert!(home.live(&cookie_of("other.test", "keep", "x")));
    assert!(!home.deleted(&cookie_of("other.test", "keep", "x")));
    // The helper was asked once, after the home was told: it said what it deleted, and that
    // did not make a second deletion of it at the home.
    assert_eq!(lock(&a.table).site_clears, vec!["site.test".to_owned()]);
    assert_eq!(a.value(&cookie("sid", None)), None);
}

#[test]
fn a_site_that_was_cleared_does_not_come_back_when_the_mac_starts_again() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    let root;
    {
        let a = Mac::with(
            "site-again",
            depot.clone(),
            &bell,
            table(&[cookie("sid", Some("one"))], &[]),
        );
        a.attach();
        bell.until("a joined", || joined(&a));
        bell.until("the home has it", || home.live(&cookie("sid", None)));
        a.clears("site.test");
        bell.until("the home has it deleted", || {
            home.deleted(&cookie("sid", None))
        });
        a.end();
        a.keep.store(true, Ordering::Relaxed);
        root = a.root.clone();
    }
    // Started again with a profile that has nothing of the site (what the engine deleted).
    let again = Mac::start(root, depot, &bell, Table::default(), None);
    again.attach();
    bell.until("the second join", || joined(&again));
    assert_eq!(again.value(&cookie("sid", None)), None);
    assert!(home.deleted(&cookie("sid", None)));
    assert!(lock(&again.table).writes.iter().all(|batch| {
        batch.is_empty()
            || batch
                .iter()
                .all(|record| !super::site::is_the_hosts(record, "site.test"))
    }));
}

#[test]
fn a_profile_with_no_sync_is_cleared_by_the_helper_alone() {
    // The shell's `Syncs` has no sync for a browser of a profile that is kept on this Mac alone:
    // `false` says so and the command goes to the helper directly.
    let syncs = super::Syncs::new();
    assert!(!syncs.clear_site(5, 1, "site.test"));
    // The answer of a clearing nobody's sync asked for is given back to whoever asked.
    let said = Event::SiteCleared {
        browser: 5,
        req: 1,
        records: Vec::new(),
        error: None,
    };
    assert_eq!(syncs.route(said.clone()), Some(said));
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
fn a_switch_turned_on_again_takes_the_mark_away_before_the_first_connection_and_then_shares() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    // The mark is there: another Mac turned the switch off, and this one has just turned it on.
    lock(&home.store).refuse = Some(wire::code::OFF);
    let mac = Mac::turned(
        "lift",
        depot.clone(),
        &bell,
        table(&[cookie("sid", Some("mine"))], &[]),
        |mark| {
            mark.remember(false);
            Some(Turn::Lift(mark.clone()))
        },
    );
    mac.attach();
    bell.until("the mark is taken away and the profile shared", || {
        home.live(&cookie("sid", None))
    });
    // The mark went before the home was asked, or it would have turned the Mac away for good.
    assert_eq!(*lock(&depot.told), vec![Told { on: true, opens: 0 }]);
    assert_eq!(mac.mark().recall(), Some(true));
}

#[test]
fn a_mark_that_cannot_be_taken_away_is_tried_again_and_the_home_is_not_asked_meanwhile() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    lock(&home.store).refuse = Some(wire::code::OFF);
    depot.switch_failures.store(2, Ordering::Relaxed);
    let mac = Mac::turned(
        "lift-retry",
        depot.clone(),
        &bell,
        table(&[cookie("sid", Some("mine"))], &[]),
        |mark| {
            mark.remember(false);
            Some(Turn::Lift(mark.clone()))
        },
    );
    mac.attach();
    // A browser does not wait for it.
    assert!(
        mac.sync
            .gate()
            .wait(Stage::AfterOpen, Instant::now() + Duration::from_secs(5)),
        "the browser goes on with what the Mac has"
    );
    bell.until("shared after the third try", || {
        home.live(&cookie("sid", None))
    });
    assert!(
        lock(&home.store).opens.len() == 1,
        "asked once, after the mark was gone"
    );
    assert_eq!(lock(&depot.told).len(), 1);
}

#[test]
fn a_switch_turned_off_takes_what_the_home_has_first_and_then_tells_the_machine() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    home.seed(&cookie("sid", Some("home")), &[], &[]);
    let mac = Mac::turned("close", depot.clone(), &bell, Table::default(), |mark| {
        mark.remember(true);
        Some(Turn::Close(mark.clone()))
    });
    mac.attach();
    bell.until("the machine is told", || !lock(&depot.told).is_empty());
    // The Mac had the home's state before the home was deleted, and the home was joined to be so.
    assert_eq!(
        mac.value(&cookie("sid", None)).as_deref(),
        Some("home"),
        "taken before it was deleted"
    );
    let told = lock(&depot.told).clone();
    assert_eq!(
        told,
        vec![Told {
            on: false,
            opens: 1
        }]
    );
    // The sync is over once the note is written: nothing keeps asking.
    assert!(mac.sync.ended(Duration::from_secs(5)), "the sync ended");
    assert_eq!(mac.mark().recall(), Some(false));
    assert!(
        lock(&home.store).entries.is_empty(),
        "the server's copy is gone"
    );
}

#[test]
fn a_close_that_fails_is_tried_again_and_one_the_home_already_made_is_not_told() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    home.seed(&cookie("sid", Some("home")), &[], &[]);
    depot.switch_failures.store(2, Ordering::Relaxed);
    let mac = Mac::turned(
        "close-retry",
        depot.clone(),
        &bell,
        Table::default(),
        |mark| {
            mark.remember(true);
            Some(Turn::Close(mark.clone()))
        },
    );
    mac.attach();
    bell.until("told at last", || !lock(&depot.told).is_empty());
    assert!(mac.sync.ended(Duration::from_secs(5)), "the sync ended");
    assert_eq!(mac.mark().recall(), Some(false));
    assert_eq!(lock(&depot.told).len(), 1);
    // Another Mac turned it off first: the home says no, and there is nothing to tell it.
    let (home2, depot2) = started(&bell);
    lock(&home2.store).refuse = Some(wire::code::OFF);
    let other = Mac::turned(
        "close-off",
        depot2.clone(),
        &bell,
        Table::default(),
        |mark| {
            mark.remember(true);
            Some(Turn::Close(mark.clone()))
        },
    );
    other.attach();
    assert!(
        other
            .sync
            .gate()
            .wait(Stage::AfterOpen, Instant::now() + Duration::from_secs(5)),
        "the home said no"
    );
    other.end();
    assert_eq!(other.mark().recall(), Some(false));
    assert!(lock(&depot2.told).is_empty());
}

#[test]
fn a_server_that_ends_the_command_without_a_hello_lets_the_browser_go_on_and_is_tried_slowly() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    depot.mute.store(true, Ordering::Relaxed);
    let mac = Mac::new("mute", depot, &bell);
    mac.attach();
    assert!(
        mac.sync
            .gate()
            .wait(Stage::AfterOpen, Instant::now() + Duration::from_secs(5)),
        "the browser goes on with what the Mac has"
    );
    assert_eq!(home.opens(), 0);
}

#[test]
fn a_home_that_says_hello_and_then_goes_slow_holds_the_pages_for_the_wait_and_not_longer() {
    let bell = Arc::new(Bell::default());
    let (home, depot) = started(&bell);
    home.seed(&cookie("sid", Some("home")), &[], &[]);
    let mac = Mac::new("slowhold", depot, &bell);
    mac.attach();
    bell.until("joined", || joined(&mac));
    // The home goes away and comes back, and says hello and nothing more.
    home.stall(true);
    home.cut();
    let end = Instant::now() + Duration::from_secs(10);
    while mac.sync.gate().wait_released(Instant::now()) {
        assert!(Instant::now() < end, "the requests were held");
        std::thread::sleep(Duration::from_millis(5));
    }
    let held = Instant::now();
    assert!(
        mac.sync
            .gate()
            .wait_released(held + super::SYNC_WAIT + Duration::from_secs(3)),
        "let go with the join not over"
    );
    assert!(held.elapsed() < super::SYNC_WAIT + Duration::from_secs(2));
    home.stall(false);
}

#[test]
fn a_number_a_live_sync_has_is_not_given_to_another() {
    let bell = Arc::new(Bell::default());
    let (_home, depot) = started(&bell);
    let (a, b) = (
        Mac::new("ids-a", depot.clone(), &bell),
        Mac::new("ids-b", depot, &bell),
    );
    let syncs = super::Syncs::new();
    syncs.join("a", 1, |_| Ok(a.sync.clone())).unwrap();
    // The counter comes round to the number the first one has.
    lock(&syncs.slots).next = super::MAX_SYNCS;
    syncs.join("b", 2, |_| Ok(b.sync.clone())).unwrap();
    let slots = lock(&syncs.slots);
    assert_eq!(slots.by_id.len(), 2, "two numbers, two syncs");
    assert_eq!(slots.by_id.get(&1).map(String::as_str), Some("a"));
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
    let again = Mac::start(root, depot, &bell, table, None);
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

/// The `Put`s the engine has said so far: their numbers and the numbers they are written on.
fn puts_of(said: &Said) -> Vec<(u32, u64, bool)> {
    lock(&said.0)
        .iter()
        .filter_map(|m| match m {
            Client::Put(p) => Some((p.req, p.base, p.body.deleted)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_deletion_the_person_asked_for_is_written_over_a_newer_record_and_that_one_is_not_put_in_the_profile()
 {
    let (mut engine, said, asked, _gate, root) = rig("forced");
    engine.handle(Input::Attach(BROWSER));
    engine.connected(Box::new(said.clone()));
    engine.handle(Input::Home(opened(Mode::Full, 0)));
    engine.handle(Input::Home(Server::CaughtUp { head: 0 }));
    let mut seen = reply(&mut engine, &asked, 0);
    seen = reply(&mut engine, &asked, seen);
    // The Mac took the record at number 4, and the site is cleared.
    let record = cookie("sid", Some("zero"));
    engine.handle(Input::Home(Server::Record(entry_of(4, &record))));
    seen = reply(&mut engine, &asked, seen);
    engine.handle(Input::ClearSite {
        browser: BROWSER,
        req: 500,
        host: "site.test".into(),
    });
    // The deletion is said to the home before anything else is, on the number the Mac took, and the
    // helper is asked to delete it too.
    let first = puts_of(&said);
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!((first[0].1, first[0].2), (4, true));
    assert!(lock(&asked.0).iter().any(|c| matches!(
        c,
        Command::ClearSiteData { req: 500, host, .. } if host == "site.test"
    )));
    // Before the home answers, another computer's newer value comes down its stream: it is not put
    // in the profile (the deletion is the last word).
    let writes = |asked: &Asked| {
        lock(&asked.0)
            .iter()
            .filter(|c| matches!(c, Command::ProfileWrite { .. }))
            .count()
    };
    let before = writes(&asked);
    engine.handle(Input::Home(Server::Record(entry_of(
        6,
        &cookie("sid", Some("theirs")),
    ))));
    let _ = reply(&mut engine, &asked, seen);
    assert_eq!(
        writes(&asked),
        before,
        "nothing was written into the profile"
    );
    // The home says the record has changed (it is at 6): the deletion is written on that number.
    engine.handle(Input::Home(Server::PutReply(PutReply {
        req: first[0].0,
        result: PutResult::Conflict,
        seq: 6,
    })));
    let all = puts_of(&said);
    assert_eq!(all.len(), 2, "{all:?}");
    assert_eq!((all[1].1, all[1].2), (6, true));
    engine.handle(Input::Home(Server::PutReply(PutReply {
        req: all[1].0,
        result: PutResult::Done,
        seq: 7,
    })));
    assert_eq!(engine.known(&id_of(&record)).map(|k| k.applied), Some(7));
    assert_eq!(
        engine.known(&id_of(&record)).map(|k| k.digest),
        Some(record_digest(&cookie("sid", None), &[]))
    );
    // Its own deletion comes back from the stream as the Mac's own word and changes nothing.
    // Once it is said, a peer's record is the profile's again.
    engine.handle(Input::Home(Server::Record(entry_of(
        8,
        &cookie("sid", Some("later")),
    ))));
    let _ = reply(&mut engine, &asked, seen);
    assert_eq!(writes(&asked), before + 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn what_the_helper_deleted_and_the_mac_did_not_know_is_said_to_the_home_once_and_other_sites_are_left()
 {
    let (mut engine, said, asked, _gate, root) = rig("helper-list");
    engine.handle(Input::Attach(BROWSER));
    engine.connected(Box::new(said.clone()));
    engine.handle(Input::Home(opened(Mode::Full, 0)));
    engine.handle(Input::Home(Server::CaughtUp { head: 0 }));
    let seen = reply(&mut engine, &asked, 0);
    let seen = reply(&mut engine, &asked, seen);
    // Two cookies the Mac took, of the site and of another.
    engine.handle(Input::Home(Server::Record(entry_of(
        4,
        &cookie("old", Some("v")),
    ))));
    engine.handle(Input::Home(Server::Record(entry_of(
        5,
        &cookie_of("other.test", "keep", "x"),
    ))));
    let _ = reply(&mut engine, &asked, seen);
    engine.handle(Input::ClearSite {
        browser: BROWSER,
        req: 501,
        host: "site.test".into(),
    });
    let first = puts_of(&said);
    assert_eq!(
        first.len(),
        1,
        "only the site's cookie the Mac knew: {first:?}"
    );
    assert_eq!((first[0].1, first[0].2), (4, true));
    // The helper says what it deleted: the cookie that is on its way already, and one the Mac did
    // not know (made a moment ago). The first is not said again.
    engine.handle(Input::Helper(Event::SiteCleared {
        browser: BROWSER,
        req: 501,
        records: vec![cookie("old", None), cookie("new", None)],
        error: None,
    }));
    engine.handle(Input::Home(Server::PutReply(PutReply {
        req: first[0].0,
        result: PutResult::Done,
        seq: 6,
    })));
    let all = puts_of(&said);
    assert_eq!(all.len(), 2, "{all:?}");
    assert_eq!(
        (all[1].1, all[1].2),
        (0, true),
        "the new one, written as new"
    );
    engine.handle(Input::Home(Server::PutReply(PutReply {
        req: all[1].0,
        result: PutResult::Done,
        seq: 7,
    })));
    assert_eq!(puts_of(&said).len(), 2, "nothing more is said");
    // An answer nobody asked for (another request's number) is nothing of the profile's.
    engine.handle(Input::Helper(Event::SiteCleared {
        browser: BROWSER,
        req: 999,
        records: vec![cookie("stranger", None)],
        error: None,
    }));
    assert_eq!(puts_of(&said).len(), 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn two_panes_that_number_their_clearings_alike_are_each_told_what_the_helper_deleted() {
    // Every pane numbers the clearings of its page from the same start, and a profile is shared by
    // panes: the request alone is not one clearing, the browser with it is.
    let (mut engine, said, asked, _gate, root) = rig("two-panes");
    engine.handle(Input::Attach(BROWSER));
    engine.connected(Box::new(said.clone()));
    engine.handle(Input::Home(opened(Mode::Full, 0)));
    engine.handle(Input::Home(Server::CaughtUp { head: 0 }));
    let seen = reply(&mut engine, &asked, 0);
    let _ = reply(&mut engine, &asked, seen);
    for browser in [BROWSER, BROWSER + 1] {
        engine.handle(Input::ClearSite {
            browser,
            req: 501,
            host: "site.test".into(),
        });
    }
    for (browser, name) in [(BROWSER + 1, "second"), (BROWSER, "first")] {
        engine.handle(Input::Helper(Event::SiteCleared {
            browser,
            req: 501,
            records: vec![cookie(name, None)],
            error: None,
        }));
    }
    let first = puts_of(&said);
    assert_eq!(first.len(), 1, "one is in flight: {first:?}");
    engine.handle(Input::Home(Server::PutReply(PutReply {
        req: first[0].0,
        result: PutResult::Done,
        seq: 6,
    })));
    let all = puts_of(&said);
    assert_eq!(
        all.len(),
        2,
        "both panes' deletions are told to the home: {all:?}"
    );
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
