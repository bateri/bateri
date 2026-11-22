//! The sync of one profile with its home, as a state machine: it is given what happens (the
//! home says something, the helper answers or reports, a browser opens or closes) and says
//! what it does to the home and to the helper. No threads and no clock of its own: the
//! driver ([`super::ProfileSync`]) runs it on one thread, and the tests feed it a home and a
//! helper that are made of a few lines.
//!
//! **A run, in the order of things** (the plan's Karar 3, 4 and 5):
//!
//! 1. *Join.* The home is opened with what the Mac last took (`since`) and answers with the
//!    records that bring it up to date. The blobs they point at that the Mac lacks are fetched,
//!    the profile's own IndexedDB is read ([`Command::IndexedDbRead`]: no browser needed) and
//!    [`plan`] says what is written, what is sent and what goes. The IndexedDB folders are put in
//!    before the engine opens the profile (`BeforeOpen` is released here). Then, with a browser to
//!    ask through, the same is done with the cookies and the Local Storage
//!    ([`Command::ProfileRead`], [`Command::ProfileWrite`]); `AfterOpen` is released when that is
//!    written, and the browser's first page can go.
//! 2. *Live.* The helper reports what the profile gains and loses ([`Event::ProfileChanged`]);
//!    each change is sent to the home, one at a time, on the number the Mac has of the record
//!    (compare and write: a home that has a newer one says no, and the newer comes from its
//!    stream). What the home streams is written into the profile as it comes. A folder of an
//!    IndexedDB cannot be put under an engine that runs: it waits for the next open.
//! 3. *Away.* A home that is lost keeps what the profile gains in the queue; when it is back the
//!    join is made again from `since`, and the queue goes out on the same rule.
//!
//! What the home keeps is bytes it does not read; what this module knows of them is
//! [`super::records`]'s.
//!
//! **`since`** is the number the Mac asks the home to go on from. It stays behind any record
//! that was seen and could not be put in the profile (an IndexedDB under a running engine, a
//! blob that did not come), so that the record is brought again at the next join.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use browser_ipc::message::{Command, Event, Record};
use kablo_proto::profile::{
    self as wire, Body, Client, Digest, Entry, MAX_PART, Mode, Open, Part, Put, PutResult, Server,
    code, kind,
};

use super::gate::{Gate, Stage};
use super::plan::{LocalRec, plan};
use super::records::{
    Folder, Id, digest_of, folder_value, id_of, record_digest, record_of, wire_of, without_value,
};
use super::store::{Cache, Known, Saved};

/// What the engine says to the helper.
pub trait Helper: Send {
    fn send(&mut self, command: Command);
}

/// What the engine says to the home.
pub trait Home: Send {
    fn send(&mut self, message: &Client) -> io::Result<()>;
}

pub struct Config {
    /// The profile's folder under the helper's root.
    pub dir: String,
    /// The names the home knows the profile by.
    pub app: String,
    pub name: String,
    /// The helper's root: its stages are below it (`.stage/…`).
    pub root: PathBuf,
    /// Where the state is kept between runs.
    pub state: PathBuf,
    pub cache: Cache,
}

/// What happens to the engine.
pub enum Input {
    /// The helper's news of the profile.
    Helper(Event),
    Home(Server),
    /// A browser of the profile is open: the engine asks the helper through the first.
    Attach(u32),
    Detach(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Down,
    Joining,
    Live,
}

/// Where a join stands.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// The home is still saying what it has.
    Collecting,
    /// The blobs the records point at are being fetched.
    Fetching,
    IdbRead,
    IdbWrite,
    /// The folders are done; the cookies and the storage wait for a browser to ask through.
    NeedBrowser,
    LocalRead,
    LocalWrite,
}

struct Join {
    mode: Mode,
    entries: Vec<Entry>,
    /// The newest number of the home when it said it had said everything.
    caught_up: Option<u64>,
    step: Step,
}

/// What a request to the helper is for. `round` is the connection it was made on: the answer to
/// one of an older connection is of no use to the join that is made now.
enum Pending {
    IdbRead {
        round: u32,
    },
    IdbWrite {
        round: u32,
        stage: String,
        entries: Vec<Entry>,
    },
    LocalRead {
        round: u32,
    },
    Write {
        round: u32,
        entries: Vec<Entry>,
        removed: Vec<Id>,
        join: bool,
    },
}

enum Source {
    File(PathBuf),
    Bytes(Vec<u8>),
}

struct Change {
    id: Id,
    body: Body,
    /// Where the bytes of the blobs the record points at are, by their sums.
    sources: HashMap<Digest, Source>,
    /// The helper's stage the files are in, kept until the record is dealt with.
    stage: Option<String>,
    /// The home said the record was not there: written again as new.
    as_new: bool,
    tries: u8,
}

enum Work {
    Put(Box<Change>),
    Fetch(Digest),
}

enum Job {
    /// Asked which blobs the home lacks.
    Have(Change),
    /// Sent one blob and waiting for the home to say it is whole.
    Uploading {
        change: Change,
        waiting: Digest,
        todo: Vec<Digest>,
    },
    Putting {
        change: Change,
        req: u32,
    },
    Fetching {
        digest: Digest,
        file: File,
        got: u64,
    },
}

/// How the sending of a blob went.
enum Sent {
    Whole,
    /// The file is not there (a stage that was cleared, a folder that changed).
    Gone,
    /// The home could not be written to.
    Home,
}

pub struct Engine {
    cfg: Config,
    helper: Box<dyn Helper>,
    home: Option<Box<dyn Home>>,
    gate: Arc<Gate>,
    reqs: Arc<AtomicU32>,
    saved: Saved,
    dirty: bool,
    state: State,
    ever_live: bool,
    round: u32,
    /// The newest number taken from the home.
    head: u64,
    /// The numbers of records seen and not put in the profile.
    unapplied: BTreeSet<u64>,
    browsers: Vec<u32>,
    /// The browser the helper reports the profile's changes through.
    watching: Option<u32>,
    join: Option<Join>,
    /// What came from the home while the join was written.
    backlog: Vec<Entry>,
    queue: VecDeque<Work>,
    job: Option<Job>,
    pending: HashMap<u32, Pending>,
    /// Records of the home that wait for a blob to come.
    waiting: Vec<Entry>,
    /// Blobs the home said it does not have.
    missing: HashSet<Digest>,
    stages: HashMap<String, usize>,
    stage_seq: u32,
    /// The code the home turned the Mac away with: it is not asked again.
    refused: Option<u16>,
}

fn hash_file(path: &Path) -> io::Result<Digest> {
    let mut hasher = wire::Sha256::new();
    let mut file = File::open(path)?;
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut chunk)?;
        if n == 0 {
            return Ok(hasher.finish());
        }
        hasher.update(chunk.get(..n).unwrap_or_default());
    }
}

fn is_gone(record: &Record) -> bool {
    matches!(
        record,
        Record::Cookie { value: None, .. }
            | Record::LocalStorage { value: None, .. }
            | Record::IndexedDb { value: None, .. }
    )
}

impl Engine {
    pub fn new(
        cfg: Config,
        helper: Box<dyn Helper>,
        gate: Arc<Gate>,
        reqs: Arc<AtomicU32>,
    ) -> Engine {
        let saved = Saved::load(&cfg.state);
        Engine {
            cfg,
            helper,
            home: None,
            gate,
            reqs,
            saved,
            dirty: false,
            state: State::Down,
            ever_live: false,
            round: 0,
            head: 0,
            unapplied: BTreeSet::new(),
            browsers: Vec::new(),
            watching: None,
            join: None,
            backlog: Vec::new(),
            queue: VecDeque::new(),
            job: None,
            pending: HashMap::new(),
            waiting: Vec::new(),
            missing: HashSet::new(),
            stages: HashMap::new(),
            stage_seq: 0,
            refused: None,
        }
    }

    /// The code the home turned the Mac away with, if it did.
    pub fn refused(&self) -> Option<u16> {
        self.refused
    }

    /// The home could not be reached: a browser that waits for it is let go.
    pub fn unreachable(&mut self) {
        if !self.ever_live {
            self.gate.release_all();
        }
    }

    pub fn is_down(&self) -> bool {
        self.state == State::Down
    }

    pub fn is_live(&self) -> bool {
        self.state == State::Live
    }

    pub fn known(&self, id: &Id) -> Option<Known> {
        self.saved.known.get(id).copied()
    }

    pub fn since(&self) -> u64 {
        self.saved.since
    }

    fn req(&self) -> u32 {
        self.reqs.fetch_add(1, Ordering::Relaxed)
    }

    fn carrier(&self) -> Option<u32> {
        self.browsers.first().copied()
    }

    fn say(&mut self, message: &Client) {
        let Some(home) = self.home.as_mut() else {
            return;
        };
        if home.send(message).is_err() {
            self.lost();
        }
    }

    fn ask(&mut self, command: Command) {
        self.helper.send(command);
    }

    // ---- the connection ----

    /// A connection to the home is made: it is opened.
    pub fn connected(&mut self, home: Box<dyn Home>) {
        self.home = Some(home);
        self.state = State::Joining;
        self.round += 1;
        self.head = self.saved.since;
        self.join = None;
        self.backlog.clear();
        if self.ever_live {
            // The Mac was away: its requests wait for what happened meanwhile.
            self.gate.hold(true);
        }
        let open = Open {
            app: self.cfg.app.clone(),
            name: self.cfg.name.clone(),
            client: self.saved.client,
            since: self.saved.since,
            depot: self.saved.depot,
        };
        self.say(&Client::Open(open));
    }

    /// The connection to the home is gone (or the home could not be reached).
    pub fn lost(&mut self) {
        self.home = None;
        if self.state == State::Down {
            return;
        }
        self.state = State::Down;
        // What was in flight goes back to be done again; the join is made again from `since`.
        match self.job.take() {
            Some(
                Job::Have(change) | Job::Uploading { change, .. } | Job::Putting { change, .. },
            ) => self.queue.push_front(Work::Put(Box::new(change))),
            Some(Job::Fetching { .. }) | None => {}
        }
        self.queue.retain(|w| matches!(w, Work::Put(_)));
        self.join = None;
        self.backlog.clear();
        if !self.ever_live {
            // A home that never answered: the browser works with what the Mac has.
            self.gate.release_all();
        }
        self.gate.hold(false);
        self.save();
    }

    /// The Mac is not to ask the home again (it said no for good): the browser goes on alone.
    fn give_up(&mut self, why: u16) {
        self.refused = Some(why);
        self.lost();
        self.gate.release_all();
    }

    pub fn handle(&mut self, input: Input) {
        match input {
            Input::Home(message) => self.on_home(message),
            Input::Helper(event) => self.on_helper(event),
            Input::Attach(browser) => self.attach(browser),
            Input::Detach(browser) => self.detach(browser),
        }
    }

    /// Saves what is to be kept if it changed.
    pub fn tick(&mut self) {
        if self.dirty {
            self.save();
        }
    }

    /// The profile is no longer used: what is kept is kept, the helper's stages are given back,
    /// the home is hung up on.
    pub fn stop(&mut self) {
        // The watch ends with the browser it is through; nothing is asked of a browser that is
        // going.
        self.watching = None;
        for stage in self.stages.keys().cloned().collect::<Vec<_>>() {
            self.ask(Command::StageDrop { stage });
        }
        self.stages.clear();
        self.home = None;
        self.state = State::Down;
        self.save();
        self.gate.hold(false);
        self.gate.release_all();
    }

    fn save(&mut self) {
        // A profile that cannot be saved is joined again from the beginning next time.
        let _ = self.saved.save(&self.cfg.state);
        self.dirty = false;
    }

    /// `since` goes as far as the records that are all in the profile.
    fn advance(&mut self) {
        let since = match self.unapplied.iter().next() {
            Some(&first) => first.saturating_sub(1).min(self.head),
            None => self.head,
        };
        if since > self.saved.since {
            self.saved.since = since;
            self.dirty = true;
        }
    }

    fn note(&mut self, id: Id, applied: u64, digest: Digest) {
        self.saved.known.insert(id, Known { applied, digest });
        self.dirty = true;
    }

    fn note_entry(&mut self, entry: &Entry) {
        self.note(
            (entry.body.kind, entry.body.key.clone()),
            entry.seq,
            digest_of(&entry.body),
        );
    }

    // ---- the browsers ----

    fn attach(&mut self, browser: u32) {
        if self.browsers.contains(&browser) {
            return;
        }
        self.browsers.push(browser);
        if self
            .join
            .as_ref()
            .is_some_and(|j| j.step == Step::NeedBrowser)
        {
            self.begin_local();
        } else if self.state == State::Live && self.watching.is_none() {
            self.watch();
        }
    }

    fn detach(&mut self, browser: u32) {
        self.browsers.retain(|b| *b != browser);
        if self.watching == Some(browser) {
            self.watching = None;
        }
        let local_step = self
            .join
            .as_ref()
            .is_some_and(|j| matches!(j.step, Step::LocalRead | Step::LocalWrite));
        if local_step {
            // The answer of the helper goes to a browser that is gone: asked again of the next.
            if let Some(join) = self.join.as_mut() {
                join.step = Step::NeedBrowser;
            }
            self.round += 1;
            if self.carrier().is_some() {
                self.begin_local();
            }
        } else if self.state == State::Live && self.watching.is_none() && self.carrier().is_some() {
            self.watch();
        }
    }

    fn watch(&mut self) {
        if let Some(browser) = self.carrier() {
            self.ask(Command::ProfileWatch { browser, on: true });
            self.watching = Some(browser);
        }
    }

    // ---- from the home ----

    fn on_home(&mut self, message: Server) {
        match message {
            Server::Opened(opened) => {
                if self.saved.depot != Some(opened.depot) {
                    // Another home than the one the Mac knew: nothing of what it knew holds.
                    self.saved.known.clear();
                    self.saved.since = 0;
                    self.head = 0;
                    self.unapplied.clear();
                    self.saved.depot = Some(opened.depot);
                    self.dirty = true;
                }
                self.join = Some(Join {
                    mode: opened.mode,
                    entries: Vec::new(),
                    caught_up: None,
                    step: Step::Collecting,
                });
            }
            Server::Record(entry) => match self.join.as_mut() {
                Some(join) if join.step == Step::Collecting => join.entries.push(entry),
                Some(_) => self.backlog.push(entry),
                None => self.live(entry),
            },
            Server::CaughtUp { head } => {
                if let Some(join) = self.join.as_mut()
                    && join.step == Step::Collecting
                {
                    join.caught_up = Some(head);
                    join.step = Step::Fetching;
                    self.head = self.head.max(head);
                    self.begin_fetching();
                }
            }
            Server::Error(failure) => match failure.code {
                code::INTERNAL => self.lost(),
                other => self.give_up(other),
            },
            reply @ (Server::PutReply(_)
            | Server::HaveReply(_)
            | Server::Blob(_)
            | Server::BlobOk(_)
            | Server::BlobMissing(_)) => self.on_job(reply),
        }
    }

    // ---- join: the blobs ----

    /// The blobs of what the home sent that the Mac lacks are fetched first.
    fn begin_fetching(&mut self) {
        let wanted: Vec<Digest> = self
            .join
            .as_ref()
            .map(|j| {
                j.entries
                    .iter()
                    .filter(|e| !e.body.deleted)
                    .flat_map(|e| e.body.blobs().map(|b| b.sha256))
                    .collect()
            })
            .unwrap_or_default();
        let mut seen = HashSet::new();
        for digest in wanted.into_iter().rev() {
            if !self.cfg.cache.has(&digest) && seen.insert(digest) {
                self.queue.push_front(Work::Fetch(digest));
            }
        }
        self.pump();
    }

    fn fetches_done(&self) -> bool {
        !matches!(self.job, Some(Job::Fetching { .. }))
            && !self.queue.iter().any(|w| matches!(w, Work::Fetch(_)))
    }

    fn after_fetch(&mut self) {
        if self.join.as_ref().is_some_and(|j| j.step == Step::Fetching) && self.fetches_done() {
            self.begin_idb_read();
        }
        self.apply_waiting();
    }

    fn begin_idb_read(&mut self) {
        let req = self.req();
        if let Some(join) = self.join.as_mut() {
            join.step = Step::IdbRead;
        }
        self.pending
            .insert(req, Pending::IdbRead { round: self.round });
        self.ask(Command::IndexedDbRead {
            req,
            dir: self.cfg.dir.clone(),
        });
    }

    // ---- from the helper ----

    fn on_helper(&mut self, event: Event) {
        match event {
            Event::IndexedDbRecords {
                req,
                records,
                stage,
                error,
            } => match self.pending.remove(&req) {
                Some(Pending::IdbRead { round }) if round == self.round => {
                    self.idb_read(records, stage, error.is_some());
                }
                _ => self.ask(Command::StageDrop { stage }),
            },
            Event::IndexedDbWritten {
                req,
                deferred,
                error,
                ..
            } => {
                if let Some(Pending::IdbWrite {
                    round,
                    stage,
                    entries,
                }) = self.pending.remove(&req)
                {
                    self.ask(Command::StageDrop { stage });
                    if round == self.round {
                        self.idb_written(&entries, deferred == 0 && error.is_none());
                    }
                }
            }
            Event::ProfileRecords {
                req,
                records,
                error,
                ..
            } => {
                if let Some(Pending::LocalRead { round }) = self.pending.remove(&req)
                    && round == self.round
                {
                    self.local_read(records, error.is_some());
                }
            }
            Event::ProfileWritten { req, error, .. } => {
                if let Some(Pending::Write {
                    round,
                    entries,
                    removed,
                    join,
                }) = self.pending.remove(&req)
                {
                    self.written(&entries, &removed, error.is_none());
                    if join && round == self.round {
                        self.complete_join();
                    }
                }
            }
            Event::ProfileChanged {
                browser,
                records,
                stage,
            } => {
                if self.browsers.contains(&browser) {
                    for record in records {
                        self.changed(record, stage.as_deref());
                    }
                }
                if let Some(stage) = stage
                    && !self.stages.contains_key(&stage)
                {
                    self.ask(Command::StageDrop { stage });
                }
                self.pump();
            }
            _ => {}
        }
    }

    // ---- join: the folders of IndexedDB ----

    fn stage_name(&mut self) -> String {
        self.stage_seq += 1;
        format!(".stage/b{}-{}", std::process::id(), self.stage_seq)
    }

    fn stage_dir(&self, stage: &str) -> PathBuf {
        self.cfg.root.join(stage)
    }

    /// A user of a stage is done with it; the last one frees it.
    fn release_stage(&mut self, stage: Option<String>) {
        let Some(stage) = stage else { return };
        match self.stages.get(&stage).copied() {
            Some(count) if count > 1 => {
                self.stages.insert(stage, count - 1);
            }
            Some(_) => {
                self.stages.remove(&stage);
                self.ask(Command::StageDrop { stage });
            }
            None => {}
        }
    }

    fn hash_folder(&self, stage: &str, record: &Record) -> io::Result<Vec<Digest>> {
        let Record::IndexedDb {
            origin,
            value: Some(value),
        } = record
        else {
            return Ok(Vec::new());
        };
        value
            .files
            .iter()
            .map(|f| hash_file(&self.stage_dir(stage).join(origin).join(&f.path)))
            .collect()
    }

    fn change_of(&mut self, record: &Record, files: &[Digest], stage: Option<&str>) -> Change {
        let wired = wire_of(record, files);
        let mut sources = HashMap::new();
        let holds =
            matches!(record, Record::IndexedDb { value: Some(v), .. } if !v.files.is_empty());
        let stage = stage.filter(|_| holds);
        if let (
            Some(stage),
            Record::IndexedDb {
                origin,
                value: Some(value),
            },
        ) = (stage, record)
        {
            for (file, sum) in value.files.iter().zip(files) {
                sources.insert(
                    *sum,
                    Source::File(self.stage_dir(stage).join(origin).join(&file.path)),
                );
            }
        }
        if let (Some(bytes), Some(blob)) = (wired.big_value.clone(), wired.body.value_blob) {
            sources.insert(blob.sha256, Source::Bytes(bytes));
        }
        if let Some(stage) = stage {
            *self.stages.entry(stage.to_owned()).or_insert(0) += 1;
        }
        Change {
            id: wired.id(),
            body: wired.body,
            sources,
            stage: stage.map(str::to_owned),
            as_new: false,
            tries: 0,
        }
    }

    fn enqueue(&mut self, change: Change) {
        // The newest of a record is the one to say.
        let mut stale = Vec::new();
        self.queue.retain_mut(|w| match w {
            Work::Put(old) if old.id == change.id => {
                stale.push(old.stage.take());
                false
            }
            _ => true,
        });
        for stage in stale {
            self.release_stage(stage);
        }
        self.queue.push_back(Work::Put(Box::new(change)));
    }

    fn idb_read(&mut self, records: Vec<Record>, stage: String, failed: bool) {
        let Some(join) = self.join.as_ref() else {
            self.ask(Command::StageDrop { stage });
            return;
        };
        let mode = join.mode;
        let entries: Vec<Entry> = join
            .entries
            .iter()
            .filter(|e| e.body.kind == kind::INDEXED_DB)
            .cloned()
            .collect();
        if failed {
            // A folder that could not be read is not put under: what is in it may be all there is.
            for entry in &entries {
                self.unapplied.insert(entry.seq);
            }
            self.ask(Command::StageDrop { stage });
            self.after_idb();
            return;
        }
        // The profile's own folders, with the sums of their files.
        let mut local: Vec<LocalRec> = Vec::new();
        let mut sums: HashMap<Id, Vec<Digest>> = HashMap::new();
        for record in records {
            if !matches!(record, Record::IndexedDb { value: Some(_), .. }) {
                continue;
            }
            if let Ok(files) = self.hash_folder(&stage, &record) {
                let id = id_of(&record);
                local.push(LocalRec {
                    digest: record_digest(&record, &files),
                    id: id.clone(),
                    record,
                });
                sums.insert(id, files);
            }
        }
        let plan = plan(&entries, mode, &self.saved.known, &local);
        for entry in &plan.same {
            self.note_entry(entry);
        }

        // What the profile has that the home lacks (or has an older of) goes to it.
        for (rec, _base) in plan.send {
            let files = sums.remove(&rec.id).unwrap_or_default();
            let change = self.change_of(&rec.record, &files, Some(&stage));
            self.enqueue(change);
        }
        if !self.stages.contains_key(&stage) {
            self.ask(Command::StageDrop {
                stage: stage.clone(),
            });
        }

        // What the home has is put in the profile's folders, if its files are here.
        let mut batch: Vec<(Record, Option<Folder>)> = Vec::new();
        let mut applied: Vec<Entry> = Vec::new();
        for entry in plan.apply {
            match self.folder_of(&entry) {
                Ok(Some(item)) => {
                    batch.push(item);
                    applied.push(entry);
                }
                Ok(None) => {}
                Err(()) => {
                    self.unapplied.insert(entry.seq);
                }
            }
        }
        for rec in plan.remove {
            batch.push((without_value(&rec.record), None));
            self.saved.known.remove(&rec.id);
        }
        if batch.is_empty() {
            self.after_idb();
            return;
        }
        match self.build_stage(&batch) {
            Ok(stage) => {
                let req = self.req();
                if let Some(join) = self.join.as_mut() {
                    join.step = Step::IdbWrite;
                }
                self.pending.insert(
                    req,
                    Pending::IdbWrite {
                        round: self.round,
                        stage: stage.clone(),
                        entries: applied,
                    },
                );
                self.ask(Command::IndexedDbWrite {
                    req,
                    dir: self.cfg.dir.clone(),
                    stage,
                    records: batch.into_iter().map(|(r, _)| r).collect(),
                });
            }
            Err(_) => {
                for entry in applied {
                    self.unapplied.insert(entry.seq);
                }
                self.after_idb();
            }
        }
    }

    fn idb_written(&mut self, entries: &[Entry], whole: bool) {
        if whole {
            for entry in entries {
                self.note_entry(entry);
            }
        } else {
            // Which of them did not go in is not said: all of them are brought again.
            for entry in entries {
                self.unapplied.insert(entry.seq);
            }
        }
        self.after_idb();
    }

    /// The record that puts an IndexedDB entry of the home in the folders, when all its files are
    /// in the cache; `Ok(None)` for one that is of no use here, `Err` for one that waits for them.
    fn folder_of(&self, entry: &Entry) -> Result<Option<(Record, Option<Folder>)>, ()> {
        if entry.body.deleted {
            return Ok(record_of(&entry.body, None).ok().map(|r| (r, None)));
        }
        if entry.body.blobs().any(|b| !self.cfg.cache.has(&b.sha256)) {
            return Err(());
        }
        let Ok(folder) = folder_value(&entry.body.value) else {
            return Ok(None);
        };
        Ok(record_of(&entry.body, None).ok().map(|r| (r, Some(folder))))
    }

    /// Makes the stage the helper reads the folders from: below its own, the files by name.
    fn build_stage(&mut self, batch: &[(Record, Option<Folder>)]) -> io::Result<String> {
        let stage = self.stage_name();
        let dir = self.stage_dir(&stage);
        let made = (|| {
            fs::create_dir_all(&dir)?;
            for (record, folder) in batch {
                let (Record::IndexedDb { origin, .. }, Some(folder)) = (record, folder) else {
                    continue;
                };
                for spec in &folder.files {
                    let to = dir.join(origin).join(&spec.path);
                    if let Some(parent) = to.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    let from = self.cfg.cache.path(&spec.sha256);
                    if fs::hard_link(&from, &to).is_err() {
                        fs::copy(&from, &to)?;
                    }
                }
            }
            Ok(())
        })();
        match made {
            Ok(()) => Ok(stage),
            Err(e) => {
                let _ = fs::remove_dir_all(&dir);
                Err(e)
            }
        }
    }

    fn after_idb(&mut self) {
        self.gate.release(Stage::BeforeOpen);
        if let Some(join) = self.join.as_mut() {
            join.step = Step::NeedBrowser;
        }
        if self.carrier().is_some() {
            self.begin_local();
        }
    }

    // ---- join: the cookies and the Local Storage ----

    fn begin_local(&mut self) {
        let Some(browser) = self.carrier() else {
            return;
        };
        let req = self.req();
        if let Some(join) = self.join.as_mut() {
            join.step = Step::LocalRead;
        }
        self.pending
            .insert(req, Pending::LocalRead { round: self.round });
        self.ask(Command::ProfileRead { browser, req });
    }

    fn local_read(&mut self, records: Vec<Record>, failed: bool) {
        let Some(join) = self.join.as_ref() else {
            return;
        };
        let mode = join.mode;
        let mut entries: Vec<Entry> = join
            .entries
            .iter()
            .filter(|e| e.body.kind == kind::COOKIE || e.body.kind == kind::LOCAL_STORAGE)
            .cloned()
            .collect();
        // What the home said since it said it had said everything is part of the same write, so
        // that the first page does not go ahead of it (a folder of IndexedDB waits for the next
        // open as it does when live).
        let (late, rest): (Vec<Entry>, Vec<Entry>) = std::mem::take(&mut self.backlog)
            .into_iter()
            .partition(|e| e.body.kind != kind::INDEXED_DB);
        self.backlog = rest;
        entries.extend(late);
        let local: Vec<LocalRec> = if failed {
            Vec::new()
        } else {
            records
                .into_iter()
                .filter(|r| matches!(r, Record::Cookie { .. } | Record::LocalStorage { .. }))
                .map(|record| LocalRec {
                    id: id_of(&record),
                    digest: record_digest(&record, &[]),
                    record,
                })
                .collect()
        };
        // What could not be read is not taken for empty: nothing is removed or sent from it.
        let mode = if failed { Mode::Diff } else { mode };
        let plan = plan(&entries, mode, &self.saved.known, &local);
        for entry in &plan.same {
            self.note_entry(entry);
        }
        for (rec, _base) in plan.send {
            let change = self.change_of(&rec.record, &[], None);
            self.enqueue(change);
        }
        let mut batch: Vec<Record> = Vec::new();
        let mut applied: Vec<Entry> = Vec::new();
        for entry in plan.apply {
            match self.record_of_entry(&entry) {
                Ok(Some(record)) => {
                    batch.push(record);
                    applied.push(entry);
                }
                Ok(None) => {}
                Err(()) => {
                    self.unapplied.insert(entry.seq);
                }
            }
        }
        let mut removed = Vec::new();
        for rec in plan.remove {
            removed.push(rec.id.clone());
            batch.push(without_value(&rec.record));
        }
        let Some(browser) = self.carrier() else {
            if let Some(join) = self.join.as_mut() {
                join.step = Step::NeedBrowser;
            }
            return;
        };
        if batch.is_empty() {
            self.complete_join();
            return;
        }
        let req = self.req();
        if let Some(join) = self.join.as_mut() {
            join.step = Step::LocalWrite;
        }
        self.pending.insert(
            req,
            Pending::Write {
                round: self.round,
                entries: applied,
                removed,
                join: true,
            },
        );
        self.ask(Command::ProfileWrite {
            browser,
            req,
            records: batch,
        });
    }

    /// The record that puts a cookie or a value of Local Storage of the home in the profile;
    /// `Err` when the blob its value is in has not come.
    fn record_of_entry(&self, entry: &Entry) -> Result<Option<Record>, ()> {
        let blob = match entry.body.value_blob.filter(|_| !entry.body.deleted) {
            Some(blob) => Some(self.cfg.cache.read(&blob.sha256).map_err(|_| ())?),
            None => None,
        };
        Ok(record_of(&entry.body, blob.as_deref()).ok())
    }

    fn written(&mut self, entries: &[Entry], removed: &[Id], ok: bool) {
        for entry in entries {
            if ok {
                self.note_entry(entry);
                self.unapplied.remove(&entry.seq);
            } else {
                self.unapplied.insert(entry.seq);
            }
        }
        for id in removed {
            self.saved.known.remove(id);
        }
        self.advance();
    }

    fn complete_join(&mut self) {
        let Some(join) = self.join.take() else {
            return;
        };
        self.head = self.head.max(join.caught_up.unwrap_or(0));
        self.state = State::Live;
        self.ever_live = true;
        self.advance();
        self.dirty = true;
        // The watch is on before the page may go on: what it changes first is not missed.
        if self.watching.is_none() {
            self.watch();
        }
        self.gate.hold(false);
        self.gate.release(Stage::AfterOpen);
        for entry in std::mem::take(&mut self.backlog) {
            self.live(entry);
        }
        self.pump();
    }

    // ---- live ----

    /// A record of the home that comes while the Mac is joined.
    fn live(&mut self, entry: Entry) {
        self.head = self.head.max(entry.seq);
        let id: Id = (entry.body.kind, entry.body.key.clone());
        if self
            .saved
            .known
            .get(&id)
            .is_some_and(|k| k.applied >= entry.seq)
        {
            self.advance();
            return;
        }
        if entry.writer == self.saved.client {
            // This Mac's own word, come back (also the one whose answer was lost): the profile
            // has it.
            self.note_entry(&entry);
            self.advance();
            return;
        }
        let lacking: Vec<Digest> = if entry.body.deleted {
            Vec::new()
        } else {
            entry
                .body
                .blobs()
                .map(|b| b.sha256)
                .filter(|d| !self.cfg.cache.has(d) && !self.missing.contains(d))
                .collect()
        };
        self.unapplied.insert(entry.seq);
        if entry.body.kind == kind::INDEXED_DB {
            // A folder cannot be put under a running engine: its files are fetched now and it is
            // put in at the next open.
            for digest in lacking {
                self.queue.push_back(Work::Fetch(digest));
            }
        } else if lacking.is_empty() {
            self.apply_live(entry);
        } else {
            for digest in lacking {
                self.queue.push_front(Work::Fetch(digest));
            }
            self.waiting.push(entry);
        }
        self.advance();
        self.pump();
    }

    fn apply_live(&mut self, entry: Entry) {
        let Some(browser) = self.carrier() else {
            return;
        };
        match self.record_of_entry(&entry) {
            Ok(Some(record)) => {
                let req = self.req();
                self.pending.insert(
                    req,
                    Pending::Write {
                        round: self.round,
                        entries: vec![entry],
                        removed: Vec::new(),
                        join: false,
                    },
                );
                self.ask(Command::ProfileWrite {
                    browser,
                    req,
                    records: vec![record],
                });
            }
            Ok(None) => {
                self.unapplied.remove(&entry.seq);
            }
            Err(()) => {}
        }
    }

    fn apply_waiting(&mut self) {
        for entry in std::mem::take(&mut self.waiting) {
            if entry.body.blobs().all(|b| self.cfg.cache.has(&b.sha256)) {
                self.apply_live(entry);
            } else if !entry.body.blobs().any(|b| self.missing.contains(&b.sha256)) {
                self.waiting.push(entry);
            }
            // A record whose blob is not at the home stays among those not put in.
        }
    }

    /// The profile gained, changed or lost a record.
    fn changed(&mut self, record: Record, stage: Option<&str>) {
        let id = id_of(&record);
        let files = match stage {
            Some(stage) => match self.hash_folder(stage, &record) {
                Ok(files) => files,
                Err(_) => return,
            },
            None => Vec::new(),
        };
        let digest = record_digest(&record, &files);
        match self.saved.known.get(&id) {
            // What the Mac took from the home itself.
            Some(k) if k.digest == digest => return,
            // Nothing there to say gone.
            None if is_gone(&record) => return,
            _ => {}
        }
        let change = self.change_of(&record, &files, stage);
        self.enqueue(change);
    }

    // ---- the serial work to and from the home ----

    fn pump(&mut self) {
        while self.job.is_none() && self.home.is_some() && self.state != State::Down {
            let Some(work) = self.queue.pop_front() else {
                break;
            };
            match work {
                Work::Put(change) => self.start_put(*change),
                Work::Fetch(digest) => self.start_fetch(digest),
            }
        }
        if !matches!(self.job, Some(Job::Fetching { .. })) {
            self.after_fetch();
        }
    }

    fn start_fetch(&mut self, digest: Digest) {
        if self.cfg.cache.has(&digest) {
            return;
        }
        match self.cfg.cache.begin(&digest) {
            Ok(file) => {
                self.job = Some(Job::Fetching {
                    digest,
                    file,
                    got: 0,
                });
                self.say(&Client::GetBlob(digest));
            }
            Err(_) => {
                self.missing.insert(digest);
            }
        }
    }

    fn base_of(&self, change: &Change) -> u64 {
        if change.as_new {
            0
        } else {
            self.saved.known.get(&change.id).map_or(0, |k| k.applied)
        }
    }

    fn start_put(&mut self, change: Change) {
        let blobs: Vec<Digest> = change.body.blobs().map(|b| b.sha256).collect();
        if blobs.is_empty() {
            self.send_put(change);
        } else {
            self.job = Some(Job::Have(change));
            self.say(&Client::Have(blobs));
        }
    }

    fn send_put(&mut self, change: Change) {
        let req = self.req();
        let put = Put {
            req,
            body: change.body.clone(),
            base: self.base_of(&change),
        };
        self.job = Some(Job::Putting { change, req });
        self.say(&Client::Put(put));
    }

    /// Sends the parts of a blob, one at a time: a file of many megabytes is not held in memory.
    fn send_blob(&mut self, change: &Change, digest: &Digest) -> Sent {
        let Some(source) = change.sources.get(digest) else {
            return Sent::Gone;
        };
        let total = change
            .body
            .blobs()
            .find(|b| &b.sha256 == digest)
            .map_or(0, |b| b.size);
        let mut reader: Box<dyn Read + '_> = match source {
            Source::Bytes(bytes) => Box::new(io::Cursor::new(bytes.as_slice())),
            Source::File(path) => match File::open(path) {
                Ok(file) => Box::new(file),
                Err(_) => return Sent::Gone,
            },
        };
        let mut start = 0u64;
        let mut chunk = vec![0u8; MAX_PART];
        loop {
            let Ok(n) = reader.read(&mut chunk) else {
                return Sent::Gone;
            };
            if n == 0 && start > 0 {
                return Sent::Whole;
            }
            let part = Part {
                sha256: *digest,
                total,
                start,
                bytes: chunk.get(..n).unwrap_or_default().to_vec(),
            };
            let Some(home) = self.home.as_mut() else {
                return Sent::Home;
            };
            if home.send(&Client::Blob(part)).is_err() {
                return Sent::Home;
            }
            start += n as u64;
            if n == 0 {
                return Sent::Whole;
            }
        }
    }

    fn upload(&mut self, change: Change, mut todo: Vec<Digest>) {
        let Some(next) = todo.pop() else {
            self.send_put(change);
            return;
        };
        match self.send_blob(&change, &next) {
            Sent::Whole => {
                self.job = Some(Job::Uploading {
                    change,
                    waiting: next,
                    todo,
                });
            }
            Sent::Gone => {
                // A file that is gone: the record is not said, the next time it changes it is.
                let stage = change.stage;
                self.release_stage(stage);
            }
            Sent::Home => {
                self.queue.push_front(Work::Put(Box::new(change)));
                self.lost();
            }
        }
    }

    fn on_job(&mut self, reply: Server) {
        let Some(job) = self.job.take() else {
            return;
        };
        match (job, reply) {
            (Job::Have(change), Server::HaveReply(missing)) => self.upload(change, missing),
            (
                Job::Uploading {
                    change,
                    waiting,
                    todo,
                },
                Server::BlobOk(digest),
            ) if digest == waiting => self.upload(change, todo),
            (Job::Putting { change, req }, Server::PutReply(reply)) if reply.req == req => {
                self.put_done(change, reply.result, reply.seq);
            }
            (
                Job::Fetching {
                    digest,
                    mut file,
                    got,
                },
                Server::Blob(part),
            ) if part.sha256 == digest => {
                if part.start != got || file.write_all(&part.bytes).is_err() {
                    self.missing.insert(digest);
                } else {
                    let got = got + part.bytes.len() as u64;
                    if got < part.total {
                        self.job = Some(Job::Fetching { digest, file, got });
                        return;
                    }
                    drop(file);
                    if !matches!(self.cfg.cache.finish(&digest), Ok(true)) {
                        self.missing.insert(digest);
                    }
                }
            }
            (Job::Fetching { digest, .. }, Server::BlobMissing(d)) if d == digest => {
                self.missing.insert(digest);
            }
            (job, _) => {
                // Not what the job waits for: it waits on.
                self.job = Some(job);
                return;
            }
        }
        self.pump();
    }

    fn put_done(&mut self, mut change: Change, result: PutResult, seq: u64) {
        match result {
            PutResult::Done => {
                self.note(change.id.clone(), seq, digest_of(&change.body));
                self.release_stage(change.stage.take());
            }
            PutResult::Conflict if seq == 0 && change.tries < 3 => {
                // "No such record": the Mac had a number the home does not know. Written as new.
                change.as_new = true;
                change.tries += 1;
                self.queue.push_front(Work::Put(Box::new(change)));
            }
            PutResult::MissingBlob if change.tries < 3 => {
                change.tries += 1;
                self.queue.push_front(Work::Put(Box::new(change)));
            }
            // A newer one is at the home and comes from its stream; the rest cannot be said.
            _ => self.release_stage(change.stage.take()),
        }
    }
}
