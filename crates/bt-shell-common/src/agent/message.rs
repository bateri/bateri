//! The text an agent's script and the Mac say to each other on kablo's app channel.
//!
//! **A request** is lines: `bateri 2`, then the verb (`place`, `ls`, `close`, `notify`, `focus`, `zoom`, `swap`, `move`, `resize`), then one
//! `key value` line per argument — the key up to the first space, the value the rest of the
//! line, verbatim. A value has no line break (nor any control character) because the line is
//! the frame: the script on the server is `sh`, which writes lines and reads none of them
//! apart. Who asks is not in it; it is the terminal kablo names on the conversation's ticket.
//!
//! ```text
//! bateri 2
//! place
//! terminal p9
//! anchor p4
//! side below
//! ratio 30
//! keep-open yes
//! focus yes
//! ```
//!
//! The `terminal` is the one just started, and an agent's number (with `fresh DIRECTORY`, one
//! that is not started yet: the Mac opens an ordinary pane of this Mac under that name, its shell
//! started in the directory). The `anchor` is a terminal as
//! kablo names it: `p4` when the agent wrote one, and the caller's own name, whatever it is, when
//! it did not (a terminal the Mac opened before the numbers existed is `b-` and 32 digits). The
//! `terminal` of a `close` is named the same way, because a `close` that names none closes the
//! caller's own.
//!
//! The verbs that move the keyboard or change the layout (`focus`, `zoom`, `swap`, `move`,
//! `resize`) name panes the same way and a tab, a group or a window by its id (`t3`, `g1`, `w1`);
//! a `focus` that names no id means the one the caller stands in. A pane of another machine is
//! named with the machine after an `@` (`p9@prod`, this Mac `p9@local`: [`Anchor::address`]),
//! in `close`, `focus`, `zoom`, `swap`, `move` and `resize`.
//!
//! A pane for a terminal that runs on another machine than the caller's is a `place` with the
//! `server` line (`server prod`: the name `ssh` takes), and the `ssh` verb asks how to reach that
//! server (`ssh`, then `server prod`); its reply is the text of one `arg` line for each word of the
//! `ssh` command, which the script on this Mac runs itself.
//!
//! **A reply** starts with `bateri 2` too and then says `ok` (`ok placed` and the lines that
//! tell how and where, `ok text` and the rest of the message as the text to print) or `error KIND TEXT`
//! on one line. The script turns the kind into an exit code; the Mac builds the text of an `ls`,
//! the script prints it.
//!
//! **Reading is strict and never panics**: a wrong greeting, an unknown verb or key, a key twice,
//! a key without a value, a missing argument, a number that is not written plainly, a message
//! over [`MAX_BYTES`] — each is an `error usage` that says which, not a guess. A *version* this
//! build does not read is its own error ([`Kind::Version`]), so a script can try an older one.
//!
//! **One version is read.** Nothing relied on the commands of version 1 when it was dropped
//! (087 Karar 12), so keeping its reader would have been dead weight: a request of any other
//! version is a [`Kind::Version`] error that names the version this build reads, and a reply of
//! another one is not read. The version that is read has its frozen text in the tests (`V2_*`):
//! changing the format adds a version and its fixtures, it does not rewrite these. (Version 1 had
//! no names for a window, a group or a tab, and went with `bateri new`.)

use super::place::{Landed, Side};
use super::{Anchor, Terminal};
use crate::ids::{Id, Kind as IdKind};

/// The version this build writes, and the only one it reads.
pub const VERSION: u32 = 2;

/// The first line's word; the version follows it after one space.
pub const GREETING: &str = "bateri";

/// The most a request may be. A notification's words and a terminal's number come nowhere near.
pub const MAX_BYTES: usize = 8 * 1024;

/// Why a request was not done. The script maps these to exit codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The message is not one this version reads: a mistake in it (64).
    Usage,
    /// The message is of a version this build does not read; an older one may be tried.
    Version,
    /// The terminal is none the Mac or the server knows.
    Unknown,
    /// The user was asked and said no, or the request is not the sender's to make (77).
    Refused,
    /// Anything else went wrong.
    Failed,
}

impl Kind {
    const ALL: [Kind; 5] = [
        Kind::Usage,
        Kind::Version,
        Kind::Unknown,
        Kind::Refused,
        Kind::Failed,
    ];

    /// The word a reply carries.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Usage => "usage",
            Kind::Version => "version",
            Kind::Unknown => "unknown",
            Kind::Refused => "refused",
            Kind::Failed => "failed",
        }
    }

    fn from_word(word: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.word() == word)
    }
}

/// What was wrong, in words a person reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub kind: Kind,
    pub text: String,
}

impl Failure {
    pub fn new(kind: Kind, text: impl Into<String>) -> Failure {
        Failure {
            kind,
            text: text.into(),
        }
    }

    fn usage(text: impl Into<String>) -> Failure {
        Failure::new(Kind::Usage, text)
    }
}

/// How `ls` is printed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    #[default]
    Text,
    Json,
}

impl Format {
    fn word(self) -> &'static str {
        match self {
            Format::Text => "text",
            Format::Json => "json",
        }
    }
}

/// A pane for a terminal the server has started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// The terminal to show.
    pub terminal: Terminal,
    /// The terminal whose pane it goes beside; none is the sender's own. Its name is kablo's, so
    /// the sender's own terminal can be named whatever it is called (a script writes it out, since a
    /// message that waits on the server is read when nobody names its sender).
    pub anchor: Option<Anchor>,
    /// The side, a tab of its own, or none for the palette's rule.
    pub side: Option<Side>,
    /// The share of the anchor the pane asks for, in percent, `1..=99`.
    pub ratio: Option<u8>,
    /// The pane stays when the program ends.
    pub keep_open: bool,
    /// The pane takes the keyboard.
    pub focus: bool,
    /// Where else than beside the anchor, and the label.
    pub home: Home,
    /// The terminal is not started: the Mac opens an ordinary pane of this Mac, its shell started
    /// in this directory, under the name `terminal` gives it (`bateri split` with no program on a
    /// machine that has no bootstrap — this Mac). A directory and a name are all the message has
    /// to say of it; no program is carried (073, Karar 1).
    pub fresh: Option<String>,
    /// The terminal runs on this server, not on the machine that asks: the name `ssh` takes for it
    /// (`prod`). The Mac makes the pane that shows it, which connects by its own path (`-S`).
    pub server: Option<String>,
}

impl Place {
    /// The share [`Place::ratio`] is, as [`super::place::Ask::share`] takes it.
    pub fn share(&self) -> Option<f64> {
        self.ratio.map(|percent| f64::from(percent) / 100.0)
    }
}

/// What an agent asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Put the pane of a terminal in a tab.
    Place(Place),
    /// Say what is open: the windows and tabs, then the terminals.
    Ls(Format),
    /// Close a terminal: an agent's number, or — the one a request that names none means, the
    /// caller's own — the name kablo has for it, whatever that is (a terminal the Mac opened
    /// before the numbers existed is `b-` and 32 digits).
    Close(Anchor),
    /// Tell the user something.
    Notify(String),
    /// Move the keyboard or the layout.
    Arrange(Arrange),
    /// How to reach the server a terminal is to be started on: the `ssh` arguments (`-S`).
    Ssh(String),
}

/// What `focus` is about: a pane (as kablo names its terminal), or a tab, a group or a window by
/// its id. `None` is the one the caller stands in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subject {
    Pane(Option<Anchor>),
    Tab(Option<Id>),
    Group(Option<Id>),
    Window(Option<Id>),
}

impl Subject {
    /// The id line it writes, if it names one.
    fn id(&self) -> Option<String> {
        match self {
            Subject::Pane(pane) => pane.as_ref().map(ToString::to_string),
            Subject::Tab(id) | Subject::Group(id) | Subject::Window(id) => {
                id.map(|id| id.to_string())
            }
        }
    }

    fn word(&self) -> &'static str {
        match self {
            Subject::Pane(_) => "pane",
            Subject::Tab(_) => "tab",
            Subject::Group(_) => "group",
            Subject::Window(_) => "window",
        }
    }
}

/// Where a tab goes when it moves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TabHome {
    /// Into this group (of whatever window it is in).
    Group(Id),
    /// Into this window, out of any group.
    Window(Id),
}

/// The longest a label is, in bytes (kablo's own limit for a terminal's).
pub const LABEL_MAX: usize = 64;

/// The window a new tab goes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowHome {
    /// This one.
    Existing(Id),
    /// A window of its own, with the pane as its only tab.
    New,
}

/// Where the pane or tab goes when it is not simply beside the anchor, and what it is called.
/// Every part is optional; some do not go together ([`Home::check`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Home {
    /// `split --tab t3`: the pane goes into this tab.
    pub tab: Option<Id>,
    /// `tab --group g1`: the tab goes into this group.
    pub group: Option<Id>,
    /// `tab --new-group api`: the tab goes into a group of its own made for it.
    pub new_group: Option<String>,
    /// `tab --window w1`, `window`: the tab goes to this window, or to a new one.
    pub window: Option<WindowHome>,
    /// `--label web`: the label of the new tab (`tab`, `window`) or pane (`split`).
    pub label: Option<String>,
}

impl Home {
    /// What does not go together: a pane into a tab and a tab into a group, a group and a new
    /// group, a group and another window (the group has its window). `side` is what the request
    /// asked for.
    pub fn check(&self, side: Option<Side>) -> Result<(), Failure> {
        let tab_asked = self.group.is_some() || self.new_group.is_some() || self.window.is_some();
        if self.tab.is_some() && (tab_asked || side == Some(Side::Tab)) {
            return Err(Failure::usage(
                "`tab` is the tab a pane goes into; it goes with no group, window or side `tab`",
            ));
        }
        if tab_asked && side != Some(Side::Tab) {
            return Err(Failure::usage(
                "`group`, `new-group` and `window` are for a tab: the side is `tab`",
            ));
        }
        if self.group.is_some() && (self.new_group.is_some() || self.window.is_some()) {
            return Err(Failure::usage(
                "`group` is a group that is in its window already: it goes with no new group and no window",
            ));
        }
        if matches!(self.window, Some(WindowHome::New))
            && (self.group.is_some() || self.new_group.is_some())
        {
            return Err(Failure::usage(
                "a new window has no group yet: `window new` goes with no group",
            ));
        }
        if let Some(label) = self.label.as_deref() {
            if side != Some(Side::Tab) {
                return Err(Failure::usage(
                    "`label` is the label of a tab; a pane's is kablo's own (`kablo new --label`)",
                ));
            }
            check_label("label", label)?;
        }
        if let Some(name) = self.new_group.as_deref() {
            check_label("new-group", name)?;
        }
        Ok(())
    }
}

/// A label a person reads: not empty, one line, short.
fn check_label(key: &str, label: &str) -> Result<(), Failure> {
    if label.trim().is_empty() || label.len() > LABEL_MAX {
        return Err(Failure::usage(format!("`{key}` is 1 to {LABEL_MAX} bytes")));
    }
    Ok(())
}

/// The most steps a `resize` may take at once.
pub const MAX_STEPS: u8 = 50;

/// What moves the keyboard or the layout of what is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arrange {
    /// Take the keyboard (and the screen) to this.
    Focus(Subject),
    /// Fill the tab with a pane, or give the tab back: the pane named, or the caller's own.
    Zoom(Option<Anchor>),
    /// Two panes of one tab trade places.
    Swap { pane: Anchor, other: Anchor },
    /// Close a tab, a group or a window, without a question: the panes in it end.
    Close(Subject),
    /// Give a tab or a group a label; `None` takes the label away. (A pane's label is kablo's.)
    Rename {
        subject: Subject,
        label: Option<String>,
    },
    /// A tab goes into a group or a window; into a window it leaves its group.
    MoveTab { tab: Id, to: TabHome },
    /// A pane goes beside another, on its side, in its tab (which may be another window's).
    Move {
        pane: Anchor,
        beside: Anchor,
        side: Side,
    },
    /// The divider of a pane on `side` moves by `steps` cells.
    Resize {
        pane: Option<Anchor>,
        side: Side,
        steps: u8,
    },
}

/// How a pane landed, as the reply to a [`Place`] says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    pub landed: Landed,
    /// The terminal the pane stands beside.
    pub next_to: Option<Anchor>,
    pub squeezed: bool,
    pub hidden: bool,
    /// Where the pane stands, by the names a command takes: its tab, the tab's group and the
    /// window. `None` where there is none: the pane waits for a window, or the tab is in no group.
    pub tab: Option<Id>,
    pub group: Option<Id>,
    pub window: Option<Id>,
    /// The pane was asked to take the keyboard and did not: the cursor is in another pane of the
    /// window being typed in ([`super::arrange`]).
    pub keyboard_stayed: bool,
}

/// What the Mac answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// Done, and nothing to tell.
    Done,
    Placed(Placed),
    /// Done, and this is to be printed (an `ls`), byte for byte.
    Text(String),
    Failed(Failure),
}

// ─── Reading a request ───────────────────────────────────────────────────

impl Request {
    /// The request `text` says.
    pub fn parse(text: &str) -> Result<Request, Failure> {
        if text.len() > MAX_BYTES {
            return Err(Failure::usage(format!(
                "the message is over {MAX_BYTES} bytes"
            )));
        }
        let body = text.strip_suffix('\n').unwrap_or(text);
        let mut lines = body.split('\n');
        let version = version_of(lines.next().unwrap_or(""))?;
        let verb = lines
            .next()
            .ok_or_else(|| Failure::usage("the message names no verb"))?;
        if version != VERSION {
            return Err(not_read(version));
        }
        read_request(verb, lines)
    }

    /// The request as text, in the order the format lists the keys. A request that could not be
    /// read back (a text with a line break, a share outside `1..=99`) is the mistake it would be.
    pub fn encode(&self) -> Result<String, Failure> {
        let mut out = format!("{GREETING} {VERSION}\n");
        match self {
            Request::Place(place) => {
                out.push_str("place\n");
                push(&mut out, "terminal", &place.terminal.to_string())?;
                if let Some(anchor) = &place.anchor {
                    if anchor.machine().is_some() {
                        return Err(Failure::usage("`anchor` is a terminal of the sender"));
                    }
                    push(&mut out, "anchor", anchor.name())?;
                }
                if let Some(side) = place.side {
                    push(&mut out, "side", side.word())?;
                }
                if let Some(ratio) = place.ratio {
                    if !(1..=99).contains(&ratio) {
                        return Err(Failure::usage("a ratio is 1 to 99 percent"));
                    }
                    push(&mut out, "ratio", &ratio.to_string())?;
                }
                if place.keep_open {
                    push(&mut out, "keep-open", "yes")?;
                }
                if place.focus {
                    push(&mut out, "focus", "yes")?;
                }
                if let Some(directory) = &place.fresh {
                    if !directory.starts_with('/') {
                        return Err(Failure::usage(
                            "`fresh` is the absolute path of a directory",
                        ));
                    }
                    push(&mut out, "fresh", directory)?;
                }
                if let Some(server) = &place.server {
                    if place.fresh.is_some() {
                        return Err(Failure::usage(
                            "`server` and `fresh` do not go together: a terminal on a server is started",
                        ));
                    }
                    check_server("server", server)?;
                    push(&mut out, "server", server)?;
                }
                place.home.check(place.side)?;
                if let Some(tab) = place.home.tab {
                    push(&mut out, "tab", &tab.to_string())?;
                }
                if let Some(group) = place.home.group {
                    push(&mut out, "group", &group.to_string())?;
                }
                if let Some(name) = &place.home.new_group {
                    push(&mut out, "new-group", name)?;
                }
                match &place.home.window {
                    Some(WindowHome::Existing(id)) => push(&mut out, "window", &id.to_string())?,
                    Some(WindowHome::New) => push(&mut out, "window", "new")?,
                    None => {}
                }
                if let Some(label) = &place.home.label {
                    push(&mut out, "label", label)?;
                }
            }
            Request::Ls(format) => {
                out.push_str("ls\n");
                if *format != Format::default() {
                    push(&mut out, "format", format.word())?;
                }
            }
            Request::Close(terminal) => {
                out.push_str("close\n");
                push(&mut out, "terminal", &terminal.to_string())?;
            }
            Request::Ssh(server) => {
                out.push_str("ssh\n");
                check_server("server", server)?;
                push(&mut out, "server", server)?;
            }
            Request::Notify(text) => {
                out.push_str("notify\n");
                push(&mut out, "text", text)?;
            }
            Request::Arrange(arrange) => encode_arrange(&mut out, arrange)?,
        }
        if out.len() > MAX_BYTES {
            return Err(Failure::usage(format!(
                "the message is over {MAX_BYTES} bytes"
            )));
        }
        Ok(out)
    }
}

fn encode_arrange(out: &mut String, arrange: &Arrange) -> Result<(), Failure> {
    match arrange {
        Arrange::Focus(subject) => {
            out.push_str("focus\n");
            push(out, "subject", subject.word())?;
            if let Some(id) = subject.id() {
                push(out, "id", &id)?;
            }
        }
        Arrange::Close(subject) => {
            if matches!(subject, Subject::Pane(_)) {
                return Err(Failure::usage("a pane is closed by its terminal"));
            }
            out.push_str("close\n");
            push(out, "subject", subject.word())?;
            if let Some(id) = subject.id() {
                push(out, "id", &id)?;
            }
        }
        Arrange::Rename { subject, label } => {
            out.push_str("rename\n");
            push(out, "subject", subject.word())?;
            let Some(id) = subject.id() else {
                return Err(Failure::usage("`rename` needs the `id` of what it renames"));
            };
            push(out, "id", &id)?;
            if let Some(label) = label {
                check_label("label", label)?;
                push(out, "label", label)?;
            }
        }
        Arrange::MoveTab { tab, to } => {
            out.push_str("move\n");
            push(out, "subject", "tab")?;
            push(out, "id", &tab.to_string())?;
            match to {
                TabHome::Group(group) => push(out, "group", &group.to_string())?,
                TabHome::Window(window) => push(out, "window", &window.to_string())?,
            }
        }
        Arrange::Zoom(pane) => {
            out.push_str("zoom\n");
            if let Some(pane) = pane {
                push(out, "terminal", &pane.to_string())?;
            }
        }
        Arrange::Swap { pane, other } => {
            out.push_str("swap\n");
            push(out, "terminal", &pane.to_string())?;
            push(out, "other", &other.to_string())?;
        }
        Arrange::Move { pane, beside, side } => {
            out.push_str("move\n");
            push(out, "terminal", &pane.to_string())?;
            push(out, "beside", &beside.to_string())?;
            push(out, "side", pane_side(*side)?.word())?;
        }
        Arrange::Resize { pane, side, steps } => {
            if !(1..=MAX_STEPS).contains(steps) {
                return Err(Failure::usage(format!(
                    "`steps` is 1 to {MAX_STEPS}, not {steps}"
                )));
            }
            out.push_str("resize\n");
            if let Some(pane) = pane {
                push(out, "terminal", &pane.to_string())?;
            }
            push(out, "side", pane_side(*side)?.word())?;
            push(out, "steps", &steps.to_string())?;
        }
    }
    Ok(())
}

/// A side of a pane: `tab` is a place for a new pane, not a side of one that is there.
fn pane_side(side: Side) -> Result<Side, Failure> {
    if side == Side::Tab {
        return Err(Failure::usage("`side` is left, right, above or below"));
    }
    Ok(side)
}

/// `bateri N` → N.
fn version_of(line: &str) -> Result<u32, Failure> {
    let digits = line
        .strip_prefix(GREETING)
        .and_then(|rest| rest.strip_prefix(' '))
        .ok_or_else(|| Failure::usage(format!("the first line is `{GREETING} VERSION`")))?;
    let version: u32 = digits
        .parse()
        .ok()
        .filter(|version: &u32| version.to_string() == digits && *version >= 1)
        .ok_or_else(|| Failure::usage("the version is a plain number from 1"))?;
    Ok(version)
}

/// A value a line can carry.
fn check_value(key: &str, value: &str) -> Result<(), Failure> {
    if value.is_empty() {
        return Err(Failure::usage(format!("`{key}` has no value")));
    }
    if value.chars().any(char::is_control) {
        return Err(Failure::usage(format!(
            "the value of `{key}` has a line break or another control character"
        )));
    }
    Ok(())
}

fn push(out: &mut String, key: &str, value: &str) -> Result<(), Failure> {
    check_value(key, value)?;
    out.push_str(key);
    out.push(' ');
    out.push_str(value);
    out.push('\n');
    Ok(())
}

/// The `key value` lines of one request, each key at most once and only the ones the verb takes.
struct Fields<'a> {
    verb: &'a str,
    taken: Vec<(&'a str, &'a str)>,
}

impl<'a> Fields<'a> {
    fn read(
        verb: &'a str,
        lines: impl Iterator<Item = &'a str>,
        allowed: &[&str],
    ) -> Result<Fields<'a>, Failure> {
        let mut taken: Vec<(&'a str, &'a str)> = Vec::new();
        for line in lines {
            let (key, value) = line.split_once(' ').ok_or_else(|| {
                if line.is_empty() {
                    Failure::usage("a line is empty")
                } else {
                    Failure::usage(format!("`{line}` has no value"))
                }
            })?;
            if !allowed.contains(&key) {
                return Err(Failure::usage(format!("`{verb}` takes no `{key}`")));
            }
            if taken.iter().any(|(seen, _)| *seen == key) {
                return Err(Failure::usage(format!("`{key}` is given twice")));
            }
            check_value(key, value)?;
            taken.push((key, value));
        }
        Ok(Fields { verb, taken })
    }

    fn get(&self, key: &str) -> Option<&'a str> {
        self.taken
            .iter()
            .find(|(seen, _)| *seen == key)
            .map(|(_, value)| *value)
    }

    fn need(&self, key: &str) -> Result<&'a str, Failure> {
        self.get(key)
            .ok_or_else(|| Failure::usage(format!("`{}` needs a `{key}`", self.verb)))
    }

    fn terminal(&self, key: &str) -> Result<Terminal, Failure> {
        terminal_of(key, self.need(key)?)
    }

    fn flag(&self, key: &str) -> Result<bool, Failure> {
        match self.get(key) {
            None | Some("no") => Ok(false),
            Some("yes") => Ok(true),
            Some(other) => Err(Failure::usage(format!(
                "`{key}` is yes or no, not `{other}`"
            ))),
        }
    }
}

fn terminal_of(key: &str, value: &str) -> Result<Terminal, Failure> {
    Terminal::parse(value).ok_or_else(|| {
        Failure::usage(format!(
            "`{key}` names a terminal as t and its number, not `{value}`"
        ))
    })
}

/// A terminal of the sender's machine, as kablo names it.
fn anchor_of(key: &str, value: &str) -> Result<Anchor, Failure> {
    Anchor::parse(value).ok_or_else(|| {
        Failure::usage(format!(
            "`{key}` names a terminal as kablo does (letters, digits, `.`, `_`, `-`; at most 64), not `{value}`"
        ))
    })
}

/// A terminal of the sender's machine or of another, `p9` or `p9@prod`.
fn address_of(key: &str, value: &str) -> Result<Anchor, Failure> {
    Anchor::address(value).ok_or_else(|| {
        Failure::usage(format!(
            "`{key}` names a terminal as kablo does, and a machine after an `@` for one of another (`p9@prod`), not `{value}`"
        ))
    })
}

/// The most a server's name may be: a host name's own limit.
const SERVER_MAX: usize = 253;

/// A server as `ssh` is given it: a host or an alias, `user@host` too — letters, digits and
/// `. _ - : @`, starting with a letter or a digit, so that it can never be read as an option.
fn check_server(key: &str, name: &str) -> Result<(), Failure> {
    let starts_well = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric());
    if name.len() > SERVER_MAX
        || name.ends_with('@')
        || name.matches('@').count() > 1
        || !starts_well
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':' | b'@'))
    {
        return Err(Failure::usage(format!(
            "`{key}` is a host as `ssh` takes it (letters, digits, `. _ - : @`; it starts with a letter or a digit), not `{name}`"
        )));
    }
    Ok(())
}

/// The error for a message of a version that is not this build's.
fn not_read(version: u32) -> Failure {
    Failure::new(
        Kind::Version,
        format!("version {version} is not read; this reads version {VERSION}"),
    )
}

fn read_request<'a>(
    verb: &'a str,
    lines: impl Iterator<Item = &'a str>,
) -> Result<Request, Failure> {
    match verb {
        "place" => {
            let fields = Fields::read(
                verb,
                lines,
                &[
                    "terminal",
                    "anchor",
                    "side",
                    "ratio",
                    "keep-open",
                    "focus",
                    "fresh",
                    "server",
                    "tab",
                    "group",
                    "new-group",
                    "window",
                    "label",
                ],
            )?;
            let ratio = fields
                .get("ratio")
                .map(|value| {
                    value
                        .parse::<u8>()
                        .ok()
                        .filter(|percent| {
                            (1..=99).contains(percent) && percent.to_string() == value
                        })
                        .ok_or_else(|| Failure::usage("`ratio` is a percent from 1 to 99"))
                })
                .transpose()?;
            let side = fields
                .get("side")
                .map(|word| {
                    Side::from_word(word).ok_or_else(|| {
                        Failure::usage(format!(
                            "`side` is left, right, above, below or tab, not `{word}`"
                        ))
                    })
                })
                .transpose()?;
            let id = |key: &str, kind: IdKind| id_field(&fields, key, kind);
            let home = Home {
                tab: id("tab", IdKind::Tab)?,
                group: id("group", IdKind::Group)?,
                new_group: fields.get("new-group").map(str::to_owned),
                window: match fields.get("window") {
                    None => None,
                    Some("new") => Some(WindowHome::New),
                    Some(_) => id("window", IdKind::Window)?.map(WindowHome::Existing),
                },
                label: fields.get("label").map(str::to_owned),
            };
            home.check(side)?;
            if fields.get("fresh").is_some() && fields.get("server").is_some() {
                return Err(Failure::usage(
                    "`server` and `fresh` do not go together: a terminal on a server is started",
                ));
            }
            Ok(Request::Place(Place {
                terminal: fields.terminal("terminal")?,
                anchor: fields
                    .get("anchor")
                    .map(|value| anchor_of("anchor", value))
                    .transpose()?,
                side,
                ratio,
                keep_open: fields.flag("keep-open")?,
                focus: fields.flag("focus")?,
                home,
                fresh: fields
                    .get("fresh")
                    .map(|directory| {
                        if directory.starts_with('/') {
                            Ok(directory.to_owned())
                        } else {
                            Err(Failure::usage(
                                "`fresh` is the absolute path of a directory",
                            ))
                        }
                    })
                    .transpose()?,
                server: fields
                    .get("server")
                    .map(|name| check_server("server", name).map(|()| name.to_owned()))
                    .transpose()?,
            }))
        }
        "ssh" => {
            let fields = Fields::read(verb, lines, &["server"])?;
            let name = fields.need("server")?;
            check_server("server", name)?;
            Ok(Request::Ssh(name.to_owned()))
        }
        "ls" => {
            let fields = Fields::read(verb, lines, &["format"])?;
            match fields.get("format") {
                None | Some("text") => Ok(Request::Ls(Format::Text)),
                Some("json") => Ok(Request::Ls(Format::Json)),
                Some(other) => Err(Failure::usage(format!(
                    "`format` is text or json, not `{other}`"
                ))),
            }
        }
        "close" => {
            let fields = Fields::read(verb, lines, &["terminal", "subject", "id"])?;
            match (fields.get("terminal"), fields.get("subject")) {
                (Some(terminal), None) if fields.get("id").is_none() => {
                    Ok(Request::Close(address_of("terminal", terminal)?))
                }
                (None, Some(_)) => match subject_of(&fields)? {
                    Subject::Pane(_) => Err(Failure::usage(
                        "a pane is closed by its `terminal`, not by a `subject`",
                    )),
                    subject => Ok(Request::Arrange(Arrange::Close(subject))),
                },
                (None, None) => Err(Failure::usage("`close` needs a `terminal` or a `subject`")),
                _ => Err(Failure::usage(
                    "`close` takes a `terminal` or a `subject` and its `id`, not both",
                )),
            }
        }
        "notify" => {
            let fields = Fields::read(verb, lines, &["text"])?;
            Ok(Request::Notify(fields.need("text")?.to_owned()))
        }
        "focus" | "zoom" | "swap" | "move" | "resize" | "rename" => {
            read_arrange(verb, lines).map(Request::Arrange)
        }
        other => Err(Failure::usage(format!(
            "`{other}` is no verb: place, ls, close, notify, ssh, focus, zoom, swap, move, resize or rename"
        ))),
    }
}

/// The id a request's `key` line names, which must be of `kind`.
fn id_field(fields: &Fields<'_>, key: &str, kind: IdKind) -> Result<Option<Id>, Failure> {
    id_of(fields, key, kind)
}

/// The `subject` and `id` lines: what a verb is about.
fn subject_of(fields: &Fields<'_>) -> Result<Subject, Failure> {
    let given = fields.get("id");
    let id = |kind: IdKind| -> Result<Option<Id>, Failure> {
        given
            .map(|value| {
                Id::parse(value)
                    .filter(|id| id.kind() == kind)
                    .ok_or_else(|| {
                        Failure::usage(format!(
                            "`id` is a {} id like `{}1`, not `{value}`",
                            kind.word(),
                            kind.prefix()
                        ))
                    })
            })
            .transpose()
    };
    Ok(match fields.need("subject")? {
        "pane" => Subject::Pane(given.map(|value| address_of("id", value)).transpose()?),
        "tab" => Subject::Tab(id(IdKind::Tab)?),
        "group" => Subject::Group(id(IdKind::Group)?),
        "window" => Subject::Window(id(IdKind::Window)?),
        other => {
            return Err(Failure::usage(format!(
                "`subject` is pane, tab, group or window, not `{other}`"
            )));
        }
    })
}

fn read_arrange<'a>(
    verb: &'a str,
    lines: impl Iterator<Item = &'a str>,
) -> Result<Arrange, Failure> {
    let side = |fields: &Fields<'_>| -> Result<Side, Failure> {
        let word = fields.need("side")?;
        Side::from_word(word)
            .filter(|side| *side != Side::Tab)
            .ok_or_else(|| {
                Failure::usage(format!(
                    "`side` is left, right, above or below, not `{word}`"
                ))
            })
    };
    let pane = |fields: &Fields<'_>, key: &str| -> Result<Option<Anchor>, Failure> {
        fields
            .get(key)
            .map(|value| address_of(key, value))
            .transpose()
    };
    match verb {
        "focus" => {
            let fields = Fields::read(verb, lines, &["subject", "id"])?;
            Ok(Arrange::Focus(subject_of(&fields)?))
        }
        "rename" => {
            let fields = Fields::read(verb, lines, &["subject", "id", "label"])?;
            let subject = subject_of(&fields)?;
            let named = match &subject {
                Subject::Tab(id) | Subject::Group(id) => id.is_some(),
                Subject::Pane(_) => {
                    return Err(Failure::usage(
                        "a pane's label is kablo's own: `kablo label` on the server",
                    ));
                }
                Subject::Window(_) => {
                    return Err(Failure::usage("a window has no label to change"));
                }
            };
            if !named {
                return Err(Failure::usage("`rename` needs the `id` of what it renames"));
            }
            let label = fields.get("label").map(str::to_owned);
            if let Some(label) = &label {
                check_label("label", label)?;
            }
            Ok(Arrange::Rename { subject, label })
        }
        "zoom" => {
            let fields = Fields::read(verb, lines, &["terminal"])?;
            Ok(Arrange::Zoom(pane(&fields, "terminal")?))
        }
        "swap" => {
            let fields = Fields::read(verb, lines, &["terminal", "other"])?;
            Ok(Arrange::Swap {
                pane: address_of("terminal", fields.need("terminal")?)?,
                other: address_of("other", fields.need("other")?)?,
            })
        }
        "move" => {
            let fields = Fields::read(
                verb,
                lines,
                &[
                    "terminal", "beside", "side", "subject", "id", "group", "window",
                ],
            )?;
            if fields.get("subject").is_some() {
                let Subject::Tab(Some(tab)) = subject_of(&fields)? else {
                    return Err(Failure::usage(
                        "`move` moves a pane or a tab, and a tab by its `id`",
                    ));
                };
                let to = match (
                    id_field(&fields, "group", IdKind::Group)?,
                    id_field(&fields, "window", IdKind::Window)?,
                ) {
                    (Some(group), None) => TabHome::Group(group),
                    (None, Some(window)) => TabHome::Window(window),
                    _ => {
                        return Err(Failure::usage("a tab moves to one `group` or one `window`"));
                    }
                };
                return Ok(Arrange::MoveTab { tab, to });
            }
            Ok(Arrange::Move {
                pane: address_of("terminal", fields.need("terminal")?)?,
                beside: address_of("beside", fields.need("beside")?)?,
                side: side(&fields)?,
            })
        }
        _ => {
            let fields = Fields::read(verb, lines, &["terminal", "side", "steps"])?;
            let steps = match fields.get("steps") {
                None => 1,
                Some(value) => value
                    .parse::<u8>()
                    .ok()
                    .filter(|steps| (1..=MAX_STEPS).contains(steps) && steps.to_string() == value)
                    .ok_or_else(|| {
                        Failure::usage(format!("`steps` is a number from 1 to {MAX_STEPS}"))
                    })?,
            };
            Ok(Arrange::Resize {
                pane: pane(&fields, "terminal")?,
                side: side(&fields)?,
                steps,
            })
        }
    }
}

// ─── The reply ───────────────────────────────────────────────────────────

fn yes_no(flag: bool) -> &'static str {
    if flag { "yes" } else { "no" }
}

impl Reply {
    /// The reply as text.
    pub fn encode(&self) -> String {
        let mut out = format!("{GREETING} {VERSION}\n");
        match self {
            Reply::Done => out.push_str("ok\n"),
            Reply::Placed(placed) => {
                out.push_str("ok placed\n");
                out.push_str(&format!("landed {}\n", placed.landed.word()));
                if let Some(next_to) = &placed.next_to {
                    out.push_str(&format!("next-to {next_to}\n"));
                }
                out.push_str(&format!("squeezed {}\n", yes_no(placed.squeezed)));
                out.push_str(&format!("hidden {}\n", yes_no(placed.hidden)));
                for (key, id) in [
                    ("tab", placed.tab),
                    ("group", placed.group),
                    ("window", placed.window),
                ] {
                    if let Some(id) = id {
                        out.push_str(&format!("{key} {id}\n"));
                    }
                }
                if placed.keyboard_stayed {
                    out.push_str("keyboard stayed\n");
                }
            }
            Reply::Text(text) => {
                out.push_str("ok text\n");
                out.push_str(text);
            }
            Reply::Failed(failure) => {
                // One line, whatever the words held.
                let text: String = failure
                    .text
                    .chars()
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect();
                out.push_str(&format!("error {} {text}\n", failure.kind.word()));
            }
        }
        out
    }

    /// The reply `text` says.
    pub fn parse(text: &str) -> Result<Reply, Failure> {
        let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
        let version = version_of(first)?;
        if version != VERSION {
            return Err(not_read(version));
        }
        let (status, body) = rest.split_once('\n').unwrap_or((rest, ""));
        if let Some(failure) = status.strip_prefix("error ") {
            let (word, words) = failure.split_once(' ').unwrap_or((failure, ""));
            let kind = Kind::from_word(word)
                .ok_or_else(|| Failure::usage(format!("`{word}` is no kind of error")))?;
            return Ok(Reply::Failed(Failure::new(kind, words)));
        }
        match status {
            "ok" => Ok(Reply::Done),
            "ok text" => Ok(Reply::Text(body.to_owned())),
            "ok placed" => read_placed(body),
            other => Err(Failure::usage(format!("`{other}` is no status"))),
        }
    }
}

fn read_placed(body: &str) -> Result<Reply, Failure> {
    let lines = body.strip_suffix('\n').unwrap_or(body).split('\n');
    let fields = Fields::read(
        "placed",
        lines,
        &[
            "landed", "next-to", "squeezed", "hidden", "tab", "group", "window", "keyboard",
        ],
    )?;
    let landed = fields.need("landed")?;
    Ok(Reply::Placed(Placed {
        landed: Landed::from_word(landed)
            .ok_or_else(|| Failure::usage(format!("`{landed}` is no way to land")))?,
        next_to: fields
            .get("next-to")
            .map(|value| anchor_of("next-to", value))
            .transpose()?,
        squeezed: fields.flag("squeezed")?,
        hidden: fields.flag("hidden")?,
        tab: id_of(&fields, "tab", IdKind::Tab)?,
        group: id_of(&fields, "group", IdKind::Group)?,
        window: id_of(&fields, "window", IdKind::Window)?,
        keyboard_stayed: match fields.get("keyboard") {
            None => false,
            Some("stayed") => true,
            Some(other) => {
                return Err(Failure::usage(format!(
                    "`keyboard` is stayed, not `{other}`"
                )));
            }
        },
    }))
}

/// The id a reply's `key` line names, which must be of `kind`: a `tab` line with a group's id is
/// a mistake, not a tab.
fn id_of(fields: &Fields<'_>, key: &str, kind: IdKind) -> Result<Option<Id>, Failure> {
    fields
        .get(key)
        .map(|value| {
            Id::parse(value)
                .filter(|id| id.kind() == kind)
                .ok_or_else(|| {
                    Failure::usage(format!(
                        "`{key}` is a {} id like `{}1`, not `{value}`",
                        kind.word(),
                        kind.prefix()
                    ))
                })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Version 2, frozen: the text a script and the Mac speak. These are never edited; a later
    // version gets its own constants.
    const V2_PLACE: &str =
        "bateri 2\nplace\nterminal p9\nanchor p4\nside below\nratio 30\nkeep-open yes\nfocus yes\n";
    const V2_PLACE_BARE: &str = "bateri 2\nplace\nterminal p9\n";
    const V2_LS: &str = "bateri 2\nls\n";
    const V2_LS_JSON: &str = "bateri 2\nls\nformat json\n";
    const V2_CLOSE: &str = "bateri 2\nclose\nterminal p7\n";
    const V2_NOTIFY: &str = "bateri 2\nnotify\ntext The build is done: 3 tests failed\n";
    // An anchor that is no agent's number: the name of a terminal kablo has, whatever it is.
    const V2_PLACE_MADE: &str =
        "bateri 2\nplace\nterminal p9\nanchor b-0123456789abcdef0123456789abcdef\nside right\n";
    const V2_REPLY_PLACED_MADE: &str = "bateri 2\nok placed\nlanded as_asked\nnext-to b-0123456789abcdef0123456789abcdef\nsqueezed no\nhidden no\n";
    // The terminal `close` names may be one that is no agent's number, because a request that
    // names none closes the one it comes from.
    const V2_CLOSE_MADE: &str = "bateri 2\nclose\nterminal b-0123456789abcdef0123456789abcdef\n";
    const V2_REPLY_DONE: &str = "bateri 2\nok\n";
    const V2_REPLY_PLACED: &str =
        "bateri 2\nok placed\nlanded beside_group\nnext-to p4\nsqueezed no\nhidden yes\n";
    // Where the pane stands: its tab, the tab's group and the window.
    const V2_REPLY_PLACED_AT: &str = "bateri 2\nok placed\nlanded as_asked\nsqueezed no\nhidden no\ntab t3\ngroup g1\nwindow w1\n";
    const V2_REPLY_PLACED_LOOSE: &str =
        "bateri 2\nok placed\nlanded as_asked\nsqueezed no\nhidden no\ntab t3\nwindow w1\n";
    const V2_REPLY_TEXT: &str = "bateri 2\nok text\nwindow id:w1\n";
    const V2_REPLY_ERROR: &str = "bateri 2\nerror refused the user said no\n";

    fn place() -> Place {
        Place {
            terminal: Terminal(9),
            anchor: Some(Anchor::of(Terminal(4))),
            side: Some(Side::Below),
            ratio: Some(30),
            keep_open: true,
            focus: true,
            fresh: None,
            server: None,
            home: Home::default(),
        }
    }

    #[test]
    fn a_pane_with_no_terminal_yet_says_the_directory_it_starts_in() {
        const FRESH: &str = "bateri 2\nplace\nterminal p9\nanchor p4\nfresh /Users/ada/api\n";
        let wanted = Place {
            fresh: Some("/Users/ada/api".to_owned()),
            server: None,
            ..bare_with_anchor()
        };
        assert_eq!(Request::parse(FRESH), Ok(Request::Place(wanted.clone())));
        assert_eq!(Request::Place(wanted).encode().as_deref(), Ok(FRESH));
        // Only a directory that can be told apart from a word: absolute.
        let relative = FRESH.replace("/Users/ada/api", "api");
        let failure = Request::parse(&relative).expect_err("a relative directory");
        assert_eq!(failure.kind, Kind::Usage);
        assert!(failure.text.contains("absolute"), "{}", failure.text);
        let wanted = Place {
            fresh: Some("api".to_owned()),
            server: None,
            ..bare_with_anchor()
        };
        assert!(Request::Place(wanted).encode().is_err());
        // A message without the key is the message it was.
        assert_eq!(
            Request::parse(&FRESH.replace("fresh /Users/ada/api\n", "")),
            Ok(Request::Place(bare_with_anchor()))
        );
    }

    fn bare_with_anchor() -> Place {
        Place {
            anchor: Some(Anchor::of(Terminal(4))),
            ..bare()
        }
    }

    fn bare() -> Place {
        Place {
            terminal: Terminal(9),
            anchor: None,
            side: None,
            ratio: None,
            keep_open: false,
            focus: false,
            fresh: None,
            server: None,
            home: Home::default(),
        }
    }

    fn usage(text: &str) -> Failure {
        Request::parse(text).expect_err(text)
    }

    // The verbs that move the keyboard or the layout, frozen the same way.
    const V2_FOCUS_TAB: &str = "bateri 2\nfocus\nsubject tab\nid t3\n";
    const V2_FOCUS_OWN: &str = "bateri 2\nfocus\nsubject group\n";
    const V2_FOCUS_PANE: &str = "bateri 2\nfocus\nsubject pane\nid p9\n";
    const V2_ZOOM: &str = "bateri 2\nzoom\nterminal p9\n";
    const V2_ZOOM_OWN: &str = "bateri 2\nzoom\n";
    const V2_SWAP: &str = "bateri 2\nswap\nterminal p9\nother p3\n";
    const V2_MOVE: &str = "bateri 2\nmove\nterminal p9\nbeside p3\nside right\n";
    const V2_RESIZE: &str = "bateri 2\nresize\nterminal p9\nside left\nsteps 3\n";
    const V2_RESIZE_OWN: &str = "bateri 2\nresize\nside below\nsteps 1\n";

    fn pane(number: u64) -> Anchor {
        Anchor::of(Terminal(number))
    }

    #[test]
    fn the_layout_verbs_are_read_and_written_byte_for_byte() {
        let cases = [
            (
                V2_FOCUS_TAB,
                Arrange::Focus(Subject::Tab(Id::new(IdKind::Tab, 3))),
            ),
            (V2_FOCUS_OWN, Arrange::Focus(Subject::Group(None))),
            (V2_FOCUS_PANE, Arrange::Focus(Subject::Pane(Some(pane(9))))),
            (V2_ZOOM, Arrange::Zoom(Some(pane(9)))),
            (V2_ZOOM_OWN, Arrange::Zoom(None)),
            (
                V2_SWAP,
                Arrange::Swap {
                    pane: pane(9),
                    other: pane(3),
                },
            ),
            (
                V2_MOVE,
                Arrange::Move {
                    pane: pane(9),
                    beside: pane(3),
                    side: Side::Right,
                },
            ),
            (
                V2_RESIZE,
                Arrange::Resize {
                    pane: Some(pane(9)),
                    side: Side::Left,
                    steps: 3,
                },
            ),
            (
                V2_RESIZE_OWN,
                Arrange::Resize {
                    pane: None,
                    side: Side::Below,
                    steps: 1,
                },
            ),
        ];
        for (text, arrange) in cases {
            let request = Request::Arrange(arrange);
            assert_eq!(Request::parse(text).as_ref(), Ok(&request), "{text:?}");
            assert_eq!(request.encode().as_deref(), Ok(text), "{request:?}");
        }
        // A resize that names no steps takes one.
        assert_eq!(
            Request::parse("bateri 2\nresize\nside above\n"),
            Ok(Request::Arrange(Arrange::Resize {
                pane: None,
                side: Side::Above,
                steps: 1
            }))
        );
    }

    const V2_CLOSE_TAB: &str = "bateri 2\nclose\nsubject tab\nid t3\n";
    const V2_CLOSE_OWN_WINDOW: &str = "bateri 2\nclose\nsubject window\n";
    const V2_RENAME: &str = "bateri 2\nrename\nsubject tab\nid t3\nlabel web\n";
    const V2_RENAME_CLEAR: &str = "bateri 2\nrename\nsubject group\nid g1\n";
    const V2_MOVE_TAB_GROUP: &str = "bateri 2\nmove\nsubject tab\nid t3\ngroup g1\n";
    const V2_MOVE_TAB_WINDOW: &str = "bateri 2\nmove\nsubject tab\nid t3\nwindow w2\n";
    const V2_PLACE_IN_TAB: &str = "bateri 2\nplace\nterminal p9\nanchor p4\ntab t3\n";
    const V2_PLACE_IN_GROUP: &str =
        "bateri 2\nplace\nterminal p9\nanchor p4\nside tab\ngroup g1\nlabel web\n";
    const V2_PLACE_NEW_GROUP: &str =
        "bateri 2\nplace\nterminal p9\nanchor p4\nside tab\nnew-group api\nwindow w2\n";
    const V2_PLACE_NEW_WINDOW: &str =
        "bateri 2\nplace\nterminal p9\nanchor p4\nside tab\nwindow new\n";

    #[test]
    fn closing_renaming_and_opening_in_a_place_are_read_and_written_byte_for_byte() {
        let tab = |number| Id::new(IdKind::Tab, number);
        let place_with = |side: Option<Side>, home: Home| {
            Request::Place(Place {
                terminal: Terminal(9),
                anchor: Some(pane(4)),
                side,
                ratio: None,
                keep_open: false,
                focus: false,
                fresh: None,
                server: None,
                home,
            })
        };
        let cases = [
            (
                V2_CLOSE_TAB,
                Request::Arrange(Arrange::Close(Subject::Tab(tab(3)))),
            ),
            (
                V2_CLOSE_OWN_WINDOW,
                Request::Arrange(Arrange::Close(Subject::Window(None))),
            ),
            (
                V2_RENAME,
                Request::Arrange(Arrange::Rename {
                    subject: Subject::Tab(tab(3)),
                    label: Some("web".to_owned()),
                }),
            ),
            (
                V2_RENAME_CLEAR,
                Request::Arrange(Arrange::Rename {
                    subject: Subject::Group(Id::new(IdKind::Group, 1)),
                    label: None,
                }),
            ),
            (
                V2_MOVE_TAB_GROUP,
                Request::Arrange(Arrange::MoveTab {
                    tab: tab(3).expect("an id"),
                    to: TabHome::Group(Id::new(IdKind::Group, 1).expect("an id")),
                }),
            ),
            (
                V2_MOVE_TAB_WINDOW,
                Request::Arrange(Arrange::MoveTab {
                    tab: tab(3).expect("an id"),
                    to: TabHome::Window(Id::new(IdKind::Window, 2).expect("an id")),
                }),
            ),
            (
                V2_PLACE_IN_TAB,
                place_with(
                    None,
                    Home {
                        tab: tab(3),
                        ..Home::default()
                    },
                ),
            ),
            (
                V2_PLACE_IN_GROUP,
                place_with(
                    Some(Side::Tab),
                    Home {
                        group: Id::new(IdKind::Group, 1),
                        label: Some("web".to_owned()),
                        ..Home::default()
                    },
                ),
            ),
            (
                V2_PLACE_NEW_GROUP,
                place_with(
                    Some(Side::Tab),
                    Home {
                        new_group: Some("api".to_owned()),
                        window: Some(WindowHome::Existing(
                            Id::new(IdKind::Window, 2).expect("an id"),
                        )),
                        ..Home::default()
                    },
                ),
            ),
            (
                V2_PLACE_NEW_WINDOW,
                place_with(
                    Some(Side::Tab),
                    Home {
                        window: Some(WindowHome::New),
                        ..Home::default()
                    },
                ),
            ),
        ];
        for (text, request) in cases {
            assert_eq!(Request::parse(text).as_ref(), Ok(&request), "{text:?}");
            assert_eq!(request.encode().as_deref(), Ok(text), "{request:?}");
        }
    }

    #[test]
    fn what_does_not_go_together_in_a_place_or_a_tab_verb_is_refused_both_ways() {
        let base = "bateri 2\nplace\nterminal p9\nanchor p4\n";
        for (extra, word) in [
            ("tab t3\ngroup g1\nside tab\n", "goes with no group"),
            ("tab t3\nside tab\n", "goes with no group"),
            ("group g1\n", "the side is `tab`"),
            ("side left\nnew-group api\n", "the side is `tab`"),
            ("side tab\ngroup g1\nnew-group api\n", "no new group"),
            (
                "side tab\ngroup g1\nwindow w2\n",
                "no new group and no window",
            ),
            (
                "side tab\nnew-group api\nwindow new\n",
                "goes with no group",
            ),
            ("side tab\ngroup t3\n", "group id"),
            ("tab g1\n", "tab id"),
            ("side tab\nwindow x1\n", "window id"),
            ("side left\nlabel web\n", "label of a tab"),
            ("side tab\nlabel   \n", "1 to 64 bytes"),
        ] {
            let failure = usage(&format!("{base}{extra}"));
            assert_eq!(failure.kind, Kind::Usage, "{extra:?}");
            assert!(failure.text.contains(word), "{extra:?}: {}", failure.text);
        }
        let long = "x".repeat(65);
        assert!(
            usage(&format!("{base}side tab\nlabel {long}\n"))
                .text
                .contains("1 to 64")
        );
        for (text, word) in [
            (
                "bateri 2\nclose\nsubject pane\nid p9\n",
                "by its `terminal`",
            ),
            ("bateri 2\nclose\nterminal p9\nsubject tab\n", "not both"),
            ("bateri 2\nclose\nterminal p9\nid t3\n", "not both"),
            ("bateri 2\nclose\n", "needs a `terminal` or a `subject`"),
            (
                "bateri 2\nrename\nsubject tab\nlabel web\n",
                "needs the `id`",
            ),
            ("bateri 2\nrename\nsubject window\nid w1\n", "no label"),
            (
                "bateri 2\nrename\nsubject pane\nid p9\nlabel x\n",
                "kablo's own",
            ),
            (
                "bateri 2\nmove\nsubject tab\nid t3\n",
                "one `group` or one `window`",
            ),
            (
                "bateri 2\nmove\nsubject tab\nid t3\ngroup g1\nwindow w1\n",
                "one `group` or one `window`",
            ),
            (
                "bateri 2\nmove\nsubject group\nid g1\ngroup g2\n",
                "moves a pane or a tab",
            ),
        ] {
            let failure = usage(text);
            assert_eq!(failure.kind, Kind::Usage, "{text:?}");
            assert!(failure.text.contains(word), "{text:?}: {}", failure.text);
        }
        // What this writes is read back, or it is not written.
        let place = Place {
            terminal: Terminal(9),
            anchor: None,
            side: Some(Side::Left),
            ratio: None,
            keep_open: false,
            focus: false,
            fresh: None,
            server: None,
            home: Home {
                new_group: Some("api".to_owned()),
                ..Home::default()
            },
        };
        assert!(Request::Place(place).encode().is_err());
        assert!(
            Request::Arrange(Arrange::Close(Subject::Pane(Some(pane(9)))))
                .encode()
                .is_err()
        );
    }

    #[test]
    fn a_layout_request_that_is_wrong_says_which_part() {
        // An id of another kind than the subject, not a plain number, a missing part.
        for (text, word) in [
            ("bateri 2\nfocus\nsubject group\nid t3\n", "group id"),
            ("bateri 2\nfocus\nsubject tab\nid t03\n", "tab id"),
            ("bateri 2\nfocus\nsubject tab\nid 3\n", "tab id"),
            (
                "bateri 2\nfocus\nsubject frame\n",
                "pane, tab, group or window",
            ),
            ("bateri 2\nfocus\n", "needs a `subject`"),
            ("bateri 2\nswap\nterminal p9\n", "needs a `other`"),
            ("bateri 2\nmove\nterminal p9\nbeside p3\n", "needs a `side`"),
            (
                "bateri 2\nmove\nterminal p9\nbeside p3\nside tab\n",
                "left, right, above or below",
            ),
            ("bateri 2\nresize\nside left\nsteps 0\n", "from 1 to 50"),
            ("bateri 2\nresize\nside left\nsteps 51\n", "from 1 to 50"),
            ("bateri 2\nresize\nside left\nsteps 03\n", "from 1 to 50"),
            ("bateri 2\nzoom\nside left\n", "takes no `side`"),
        ] {
            let failure = usage(text);
            assert_eq!(failure.kind, Kind::Usage, "{text:?}");
            assert!(failure.text.contains(word), "{text:?}: {}", failure.text);
        }
        // A request this writes that it could not read back is the mistake it would be.
        assert!(
            Request::Arrange(Arrange::Resize {
                pane: None,
                side: Side::Tab,
                steps: 1
            })
            .encode()
            .is_err()
        );
        assert!(
            Request::Arrange(Arrange::Resize {
                pane: None,
                side: Side::Left,
                steps: 0
            })
            .encode()
            .is_err()
        );
    }

    #[test]
    fn version_two_is_read_byte_for_byte() {
        let cases = [
            (V2_PLACE, Request::Place(place())),
            (V2_PLACE_BARE, Request::Place(bare())),
            (V2_LS, Request::Ls(Format::Text)),
            (V2_LS_JSON, Request::Ls(Format::Json)),
            (V2_CLOSE, Request::Close(Anchor::of(Terminal(7)))),
            (
                V2_CLOSE_MADE,
                Request::Close(
                    Anchor::parse("b-0123456789abcdef0123456789abcdef").expect("a name"),
                ),
            ),
            (
                V2_NOTIFY,
                Request::Notify("The build is done: 3 tests failed".to_owned()),
            ),
        ];
        for (text, request) in cases {
            assert_eq!(Request::parse(text).as_ref(), Ok(&request), "{text:?}");
            // And what this writes is what version 2 wrote.
            assert_eq!(request.encode().as_deref(), Ok(text), "{request:?}");
        }
    }

    #[test]
    fn an_anchor_may_be_any_name_kablo_has_and_is_written_as_it_was_read() {
        let made = Anchor::parse("b-0123456789abcdef0123456789abcdef").expect("a name");
        let wanted = Place {
            side: Some(Side::Right),
            anchor: Some(made.clone()),
            ..bare()
        };
        assert_eq!(
            Request::parse(V2_PLACE_MADE),
            Ok(Request::Place(wanted.clone()))
        );
        assert_eq!(
            Request::Place(wanted).encode().as_deref(),
            Ok(V2_PLACE_MADE)
        );
        // A name a person gave a terminal with kablo is as good, and so is an agent's number.
        for name in ["mine", "build.2", "p04", "p4"] {
            let text = format!("bateri 2\nplace\nterminal p9\nanchor {name}\n");
            assert_eq!(
                Request::parse(&text),
                Ok(Request::Place(Place {
                    anchor: Anchor::parse(name),
                    ..bare()
                })),
                "{name}"
            );
        }
        // The reply says the same name back; the terminal being placed stays a plain number.
        let placed = Reply::Placed(Placed {
            landed: Landed::AsAsked,
            next_to: Some(made),
            squeezed: false,
            hidden: false,
            tab: None,
            group: None,
            window: None,
            keyboard_stayed: false,
        });
        assert_eq!(Reply::parse(V2_REPLY_PLACED_MADE).as_ref(), Ok(&placed));
        assert_eq!(placed.encode(), V2_REPLY_PLACED_MADE);
        assert_eq!(
            usage("bateri 2\nplace\nterminal b-0123456789abcdef0123456789abcdef\n").kind,
            Kind::Usage
        );
        // `close` names the terminal the same way, since it may close the one it comes from.
        assert_eq!(
            Request::parse(V2_CLOSE_MADE),
            Ok(Request::Close(
                Anchor::parse("b-0123456789abcdef0123456789abcdef").expect("a name")
            ))
        );
        assert_eq!(
            Request::parse("bateri 2\nclose\nterminal mine\n"),
            Ok(Request::Close(Anchor::parse("mine").expect("a name")))
        );
        assert_eq!(usage("bateri 2\nclose\nterminal a b\n").kind, Kind::Usage);
        assert_eq!(usage("bateri 2\nclose\nterminal ../x\n").kind, Kind::Usage);
    }

    #[test]
    fn a_request_comes_back_the_way_it_went() {
        let requests = [
            Request::Place(place()),
            Request::Place(bare()),
            Request::Place(Place {
                side: Some(Side::Tab),
                ..bare()
            }),
            Request::Ls(Format::Json),
            Request::Close(Anchor::of(Terminal(120))),
            Request::Close(Anchor::parse("b-0123456789abcdef0123456789abcdef").expect("a name")),
            Request::Notify("two  spaces, and a trailing one ".to_owned()),
        ];
        for request in requests {
            let text = request.encode().expect("a request that can be written");
            assert_eq!(Request::parse(&text), Ok(request), "{text:?}");
        }
    }

    #[test]
    fn a_reader_takes_the_last_newline_or_leaves_it() {
        for text in [V2_PLACE, V2_PLACE.trim_end_matches('\n')] {
            assert_eq!(Request::parse(text), Ok(Request::Place(place())));
        }
        // The flags may be said `no`, and the keys come in any order.
        let text = "bateri 2\nplace\nfocus no\nside left\nterminal p2\nkeep-open no\n";
        assert_eq!(
            Request::parse(text),
            Ok(Request::Place(Place {
                terminal: Terminal(2),
                side: Some(Side::Left),
                ..bare()
            }))
        );
    }

    #[test]
    fn a_value_with_a_line_break_is_refused_both_ways() {
        let wrote = Request::Notify("two\nlines".to_owned()).encode();
        assert_eq!(wrote.expect_err("no line break").kind, Kind::Usage);
        // Written by hand, the second line is a key of its own.
        let failure = usage("bateri 2\nnotify\ntext two\nlines\n");
        assert_eq!(failure.kind, Kind::Usage);
        assert!(failure.text.contains("`lines`"), "{failure:?}");
        // Another control character is no better than the line break.
        for text in [
            "bateri 2\nnotify\ntext a\rb\n",
            "bateri 2\nnotify\ntext a\tb\n",
            "bateri 2\nnotify\ntext a\u{1b}[31mb\n",
            "bateri 2\nnotify\ntext a\0b\n",
        ] {
            assert_eq!(usage(text).kind, Kind::Usage, "{text:?}");
        }
        assert_eq!(
            Request::Notify(String::new())
                .encode()
                .expect_err("empty")
                .kind,
            Kind::Usage
        );
    }

    #[test]
    fn what_the_format_does_not_say_is_an_error_that_says_which() {
        let cases = [
            ("", "first line"),
            ("hello 1\nls\n", "first line"),
            ("bateri\nls\n", "first line"),
            ("bateri  1\nls\n", "plain number"),
            ("bateri 01\nls\n", "plain number"),
            ("bateri 0\nls\n", "plain number"),
            ("bateri x\nls\n", "plain number"),
            ("bateri 2\n", "no verb"),
            ("bateri 2\nlist\n", "no verb"),
            ("bateri 2\nPLACE\nterminal p1\n", "no verb"),
            ("bateri 2\nplace\n", "needs a `terminal`"),
            ("bateri 2\nclose\n", "needs a `terminal`"),
            ("bateri 2\nnotify\n", "needs a `text`"),
            ("bateri 2\nplace\nterminal p1\nterminal p2\n", "twice"),
            (
                "bateri 2\nplace\nterminal p1\ncolour work\n",
                "takes no `colour`",
            ),
            (
                "bateri 2\nclose\nterminal p1\nside left\n",
                "takes no `side`",
            ),
            ("bateri 2\nplace\nterminal\n", "no value"),
            ("bateri 2\nplace\nterminal \n", "no value"),
            ("bateri 2\nplace\nterminal p1\n\n", "empty"),
            ("bateri 2\nplace\n\nterminal p1\n", "empty"),
            ("bateri 2\nplace\nterminal b-0a1b\n", "names a terminal"),
            ("bateri 2\nplace\nterminal p09\n", "names a terminal"),
            (
                "bateri 2\nplace\nterminal p1\nanchor a/b\n",
                "names a terminal as kablo does",
            ),
            (
                "bateri 2\nplace\nterminal p1\nanchor ../p4\n",
                "names a terminal as kablo does",
            ),
            (
                "bateri 2\nplace\nterminal p1\nanchor not a name\n",
                "names a terminal as kablo does",
            ),
            ("bateri 2\nplace\nterminal p1\nanchor\n", "no value"),
            ("bateri 2\nplace\nterminal p1\nside top\n", "left, right"),
            ("bateri 2\nplace\nterminal p1\nside Left\n", "left, right"),
            ("bateri 2\nplace\nterminal p1\nratio 0\n", "1 to 99"),
            ("bateri 2\nplace\nterminal p1\nratio 100\n", "1 to 99"),
            ("bateri 2\nplace\nterminal p1\nratio 030\n", "1 to 99"),
            ("bateri 2\nplace\nterminal p1\nratio 30%\n", "1 to 99"),
            ("bateri 2\nplace\nterminal p1\nratio -3\n", "1 to 99"),
            (
                "bateri 2\nplace\nterminal p1\nkeep-open true\n",
                "yes or no",
            ),
            ("bateri 2\nls\nformat yaml\n", "text or json"),
            ("bateri 2\nls\nterminal p1\n", "takes no `terminal`"),
        ];
        for (text, mentions) in cases {
            let failure = usage(text);
            assert_eq!(failure.kind, Kind::Usage, "{text:?}");
            assert!(
                failure.text.contains(mentions),
                "{text:?} → {failure:?} should mention {mentions:?}"
            );
        }
        // A message that is too long is refused before it is read.
        let long = format!("bateri 2\nnotify\ntext {}\n", "x".repeat(MAX_BYTES));
        assert!(usage(&long).text.contains("bytes"));
        let long = Request::Notify("x".repeat(MAX_BYTES));
        assert_eq!(long.encode().expect_err("too long").kind, Kind::Usage);
        assert_eq!(
            Request::Place(Place {
                ratio: Some(100),
                ..bare()
            })
            .encode()
            .expect_err("1 to 99")
            .kind,
            Kind::Usage
        );
    }

    #[test]
    fn a_version_that_is_not_read_is_its_own_error_that_names_the_one_that_is() {
        // Version 1, the one before this, is not read: nobody used its commands when it went.
        for text in [
            "bateri 1\nplace\nterminal p1\n",
            "bateri 1\nls\n",
            "bateri 3\nplace\nterminal p1\nfuture-key x\n",
            "bateri 4000000000\nls\n",
        ] {
            let failure = usage(text);
            assert_eq!(failure.kind, Kind::Version, "{text:?}");
            assert!(
                failure.text.contains("reads version 2"),
                "{text:?} -> {failure:?}"
            );
        }
        for text in [
            "bateri 1\nok\n",
            "bateri 3\nok\n",
            "bateri 1\nok placed\nlanded as_asked\n",
        ] {
            assert_eq!(
                Reply::parse(text).expect_err(text).kind,
                Kind::Version,
                "{text:?}"
            );
        }
        // A number that does not fit is not a version.
        assert_eq!(usage("bateri 99999999999\nls\n").kind, Kind::Usage);
    }

    #[test]
    fn a_placed_reply_names_the_tab_group_and_window_by_their_kind() {
        // Each line holds an id of its own kind, written plainly.
        for line in [
            "tab g1",
            "tab w1",
            "tab p1",
            "tab 3",
            "tab t03",
            "tab t0",
            "tab T3",
            "group t1",
            "window t1",
            "window g1",
            "tab ",
        ] {
            let text =
                format!("bateri 2\nok placed\nlanded as_asked\nsqueezed no\nhidden no\n{line}\n");
            let failure = Reply::parse(&text).expect_err(&text);
            assert_eq!(failure.kind, Kind::Usage, "{line:?}");
        }
        // A line twice is refused as any key is.
        let twice =
            "bateri 2\nok placed\nlanded as_asked\nsqueezed no\nhidden no\ntab t1\ntab t2\n";
        assert!(Reply::parse(twice).is_err());
        // What the Mac writes for a pane no window has yet: no line of the three.
        let waiting = Reply::Placed(Placed {
            landed: Landed::Waiting,
            next_to: None,
            squeezed: false,
            hidden: false,
            tab: None,
            group: None,
            window: None,
            keyboard_stayed: false,
        });
        assert_eq!(
            waiting.encode(),
            "bateri 2\nok placed\nlanded waiting\nsqueezed no\nhidden no\n"
        );
    }

    #[test]
    fn no_text_makes_the_reader_panic() {
        let seeds = [
            V2_PLACE,
            V2_LS,
            V2_CLOSE,
            V2_NOTIFY,
            V2_FOCUS_TAB,
            V2_MOVE,
            V2_RESIZE,
            V2_CLOSE_TAB,
            V2_RENAME,
            V2_MOVE_TAB_GROUP,
            V2_PLACE_IN_GROUP,
            V2_PLACE_NEW_GROUP,
            V2_REPLY_PLACED,
            V2_REPLY_TEXT,
            V2_REPLY_ERROR,
            "bateri 2\nnotify\ntext héllo wörld ✓\n",
        ];
        for seed in seeds {
            // Every prefix on a character boundary, and every character left out.
            for end in (0..=seed.len()).filter(|&end| seed.is_char_boundary(end)) {
                let cut = &seed[..end];
                let _ = Request::parse(cut);
                let _ = Reply::parse(cut);
            }
            for (at, c) in seed.char_indices() {
                let mut without = String::from(&seed[..at]);
                without.push_str(&seed[at + c.len_utf8()..]);
                let _ = Request::parse(&without);
                let _ = Reply::parse(&without);
            }
        }
        for text in [
            "\n",
            "\n\n\n",
            " ",
            "bateri ",
            "bateri 2\n\n",
            "\u{0}",
            "bateri 2\nls\n \n",
        ] {
            let _ = Request::parse(text);
            let _ = Reply::parse(text);
        }
    }

    #[test]
    fn the_reply_of_version_two_is_written_and_read_byte_for_byte() {
        let cases = [
            (V2_REPLY_DONE, Reply::Done),
            (
                V2_REPLY_PLACED,
                Reply::Placed(Placed {
                    landed: Landed::BesideGroup,
                    next_to: Some(Anchor::of(Terminal(4))),
                    squeezed: false,
                    hidden: true,
                    tab: None,
                    group: None,
                    window: None,
                    keyboard_stayed: false,
                }),
            ),
            (
                V2_REPLY_PLACED_AT,
                Reply::Placed(Placed {
                    landed: Landed::AsAsked,
                    next_to: None,
                    squeezed: false,
                    hidden: false,
                    tab: Id::new(IdKind::Tab, 3),
                    group: Id::new(IdKind::Group, 1),
                    window: Id::new(IdKind::Window, 1),
                    keyboard_stayed: false,
                }),
            ),
            (
                V2_REPLY_PLACED_LOOSE,
                Reply::Placed(Placed {
                    landed: Landed::AsAsked,
                    next_to: None,
                    squeezed: false,
                    hidden: false,
                    tab: Id::new(IdKind::Tab, 3),
                    group: None,
                    window: Id::new(IdKind::Window, 1),
                    keyboard_stayed: false,
                }),
            ),
            (V2_REPLY_TEXT, Reply::Text("window id:w1\n".to_owned())),
            (
                V2_REPLY_ERROR,
                Reply::Failed(Failure::new(Kind::Refused, "the user said no")),
            ),
        ];
        for (text, reply) in cases {
            assert_eq!(Reply::parse(text).as_ref(), Ok(&reply), "{text:?}");
            assert_eq!(reply.encode(), text, "{reply:?}");
        }
    }

    #[test]
    fn a_pane_whose_keyboard_stayed_says_so_and_nothing_else_changes() {
        let placed = Placed {
            landed: Landed::AsAsked,
            next_to: Some(Anchor::of(Terminal(4))),
            squeezed: false,
            hidden: false,
            tab: Id::new(IdKind::Tab, 3),
            group: None,
            window: Id::new(IdKind::Window, 1),
            keyboard_stayed: true,
        };
        let text = Reply::Placed(placed.clone()).encode();
        assert!(text.ends_with("window w1\nkeyboard stayed\n"), "{text}");
        assert_eq!(Reply::parse(&text), Ok(Reply::Placed(placed.clone())));
        // Without the word the keyboard went where it was asked.
        let moved = Reply::Placed(Placed {
            keyboard_stayed: false,
            ..placed
        })
        .encode();
        assert!(!moved.contains("keyboard"), "{moved}");
        assert!(
            Reply::parse(&moved.replace("hidden no\n", "hidden no\nkeyboard moved\n")).is_err()
        );
    }

    #[test]
    fn a_text_reply_is_the_rest_of_the_message_exactly() {
        for body in [
            "",
            "no newline at the end",
            "a\n\nb\n\n",
            "  leading spaces\n",
        ] {
            let reply = Reply::Text(body.to_owned());
            assert_eq!(Reply::parse(&reply.encode()), Ok(reply), "{body:?}");
        }
        // A reply with nobody placed beside, and one that waits.
        let reply = Reply::Placed(Placed {
            landed: Landed::Waiting,
            next_to: None,
            squeezed: false,
            hidden: false,
            tab: None,
            group: None,
            window: None,
            keyboard_stayed: false,
        });
        assert_eq!(Reply::parse(&reply.encode()), Ok(reply));
    }

    #[test]
    fn an_error_is_one_line_whatever_its_words_held() {
        let reply = Reply::Failed(Failure::new(Kind::Failed, "two\nlines\tand a tab"));
        let text = reply.encode();
        assert_eq!(text, "bateri 2\nerror failed two lines and a tab\n");
        assert_eq!(
            Reply::parse(&text),
            Ok(Reply::Failed(Failure::new(
                Kind::Failed,
                "two lines and a tab"
            )))
        );
        for kind in Kind::ALL {
            let reply = Reply::Failed(Failure::new(kind, "x"));
            assert_eq!(Reply::parse(&reply.encode()), Ok(reply));
        }
        assert!(Reply::parse("bateri 2\nerror bad x\n").is_err());
        assert!(Reply::parse("bateri 2\nmaybe\n").is_err());
        assert_eq!(
            Reply::parse("bateri 3\nok\n").expect_err("version").kind,
            Kind::Version
        );
    }

    #[test]
    fn the_share_of_a_ratio_is_its_percent() {
        assert_eq!(place().share(), Some(0.3));
        assert_eq!(bare().share(), None);
    }
    const V2_SSH: &str = "bateri 2\nssh\nserver prod\n";
    const V2_PLACE_ON: &str = "bateri 2\nplace\nterminal p9\nanchor p4\nserver deploy@prod\n";

    /// The words that reach a server and the pane for a terminal that runs on one are written and
    /// read byte for byte, and what `ssh` could take for an option, or for two hosts, is no name.
    #[test]
    fn a_server_is_asked_for_by_the_name_ssh_takes_and_a_pane_for_its_terminal_names_it() {
        assert_eq!(Request::parse(V2_SSH), Ok(Request::Ssh("prod".to_owned())));
        assert_eq!(
            Request::Ssh("prod".to_owned()).encode().as_deref(),
            Ok(V2_SSH)
        );
        let Ok(Request::Place(on)) = Request::parse(V2_PLACE_ON) else {
            panic!("a place");
        };
        assert_eq!(on.server.as_deref(), Some("deploy@prod"));
        assert_eq!(Request::Place(on).encode().as_deref(), Ok(V2_PLACE_ON));

        for bad in [
            "",
            "-oProxyCommand=x",
            "-prod",
            "pr od",
            "prod\u{7}",
            "a;b",
            "$(x)",
            "prod@",
            "a@b@c",
            "@prod",
            "ünï",
            &"a".repeat(254),
        ] {
            let ask = format!("bateri 2\nssh\nserver {bad}\n");
            assert_eq!(
                Request::parse(&ask).map_err(|failure| failure.kind),
                Err(Kind::Usage),
                "{bad:?}"
            );
            assert!(Request::Ssh(bad.to_owned()).encode().is_err(), "{bad:?}");
        }
        assert_eq!(
            Request::parse("bateri 2\nssh\n").map_err(|failure| failure.kind),
            Err(Kind::Usage)
        );
        assert_eq!(
            Request::parse("bateri 2\nssh\nserver prod\nextra x\n").map_err(|f| f.kind),
            Err(Kind::Usage)
        );
        // A terminal that is not started and one that runs on a server do not go together.
        assert_eq!(
            Request::parse("bateri 2\nplace\nterminal p9\nanchor p4\nfresh /tmp\nserver prod\n")
                .map_err(|failure| failure.kind),
            Err(Kind::Usage)
        );
        let both = Place {
            fresh: Some("/tmp".to_owned()),
            server: Some("prod".to_owned()),
            ..place()
        };
        assert!(Request::Place(both).encode().is_err());
    }

    /// A pane of another machine is written after an `@` and read back the same, in every verb
    /// that names a pane to work on; the `anchor` of a `place` is the sender's own and takes none.
    #[test]
    fn a_pane_of_another_machine_is_an_address_in_the_verbs_that_work_on_panes() {
        let address = |text: &str| Anchor::address(text).expect("an address");
        assert_eq!(address("p9@prod").name(), "p9");
        assert_eq!(address("p9@prod").machine(), Some("prod"));
        assert_eq!(address("p9@prod").to_string(), "p9@prod");
        assert_eq!(address("p9").machine(), None);
        assert_eq!(address("p9@local").machine(), Some("local"));
        assert_eq!(address("b-0a1b@10.0.0.5").machine(), Some("10.0.0.5"));
        assert_eq!(address("p9@fe80::1").machine(), Some("fe80::1"));
        for bad in [
            "p9@", "@prod", "p9@-x", "p9@a b", "p9@a@b", "p9@pr/od", "p 9@prod", "p9@é",
        ] {
            assert_eq!(Anchor::address(bad), None, "{bad:?}");
        }
        // A name is not an address and an address is not a name: kablo's alphabet has no `@`.
        assert_eq!(Anchor::parse("p9@prod"), None);

        let verbs = [
            "bateri 2\nclose\nterminal p9@prod\n",
            "bateri 2\nfocus\nsubject pane\nid p9@prod\n",
            "bateri 2\nzoom\nterminal p9@prod\n",
            "bateri 2\nswap\nterminal p9@prod\nother p3\n",
            "bateri 2\nmove\nterminal p9@prod\nbeside p3@local\nside right\n",
            "bateri 2\nresize\nterminal p9@prod\nside left\nsteps 2\n",
        ];
        for text in verbs {
            let request = Request::parse(text).expect(text);
            assert_eq!(request.encode().as_deref(), Ok(text), "{text}");
        }
        assert_eq!(
            Request::parse(verbs[0]),
            Ok(Request::Close(address("p9@prod")))
        );
        // Not in a place, where the pane goes beside a terminal of the sender's.
        assert_eq!(
            Request::parse("bateri 2\nplace\nterminal p9\nanchor p4@prod\n")
                .map_err(|failure| failure.kind),
            Err(Kind::Usage)
        );
        let there = Place {
            anchor: Some(address("p4@prod")),
            ..place()
        };
        assert!(Request::Place(there).encode().is_err());
    }
}
