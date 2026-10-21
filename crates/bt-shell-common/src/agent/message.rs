//! The text an agent's script and the Mac say to each other on kablo's app channel.
//!
//! **A request** is lines: `bateri 1`, then the verb (`place`, `ls`, `close`, `notify`), then one
//! `key value` line per argument — the key up to the first space, the value the rest of the
//! line, verbatim. A value has no line break (nor any control character) because the line is
//! the frame: the script on the server is `sh`, which writes lines and reads none of them
//! apart. Who asks is not in it; it is the terminal kablo names on the conversation's ticket.
//!
//! ```text
//! bateri 1
//! place
//! terminal t9
//! anchor t4
//! side below
//! ratio 30
//! keep-open yes
//! focus yes
//! ```
//!
//! The `terminal` is the one just started, and an agent's number. The `anchor` is a terminal as
//! kablo names it: `t4` when the agent wrote one, and the caller's own name, whatever it is, when
//! it did not (a terminal the Mac opened before the numbers existed is `b-` and 32 digits).
//!
//! **A reply** starts with `bateri 1` too and then says `ok` (`ok placed` and the lines that
//! tell how, `ok text` and the rest of the message as the text to print) or `error KIND TEXT`
//! on one line. The script turns the kind into an exit code; the Mac builds the text of an `ls`,
//! the script prints it.
//!
//! **Reading is strict and never panics**: a wrong greeting, an unknown verb or key, a key twice,
//! a key without a value, a missing argument, a number that is not written plainly, a message
//! over [`MAX_BYTES`] — each is an `error usage` that says which, not a guess. A *version* this
//! build does not read is its own error ([`Kind::Version`]), so a script can try an older one.
//!
//! **A version is read forever.** A script on a server is as old as the last time its terminal
//! was started, and keeps asking in its own version for as long as it lives, so this reads every
//! version up to its own and each version's frozen text stays in the tests (`V1_*`): changing the
//! format adds a version and its fixtures, it never rewrites one.

use super::place::{Landed, Side};
use super::{Anchor, Terminal};

/// The version this build writes. Every version from 1 up to it is read.
pub const VERSION: u32 = 1;

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
    /// Close a terminal.
    Close(Terminal),
    /// Tell the user something.
    Notify(String),
}

/// How a pane landed, as the reply to a [`Place`] says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    pub landed: Landed,
    /// The terminal the pane stands beside.
    pub next_to: Option<Anchor>,
    pub squeezed: bool,
    pub hidden: bool,
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
        match version {
            1 => read_v1(verb, lines),
            other => Err(Failure::new(
                Kind::Version,
                format!("version {other} is not read; this reads up to {VERSION}"),
            )),
        }
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
            Request::Notify(text) => {
                out.push_str("notify\n");
                push(&mut out, "text", text)?;
            }
        }
        if out.len() > MAX_BYTES {
            return Err(Failure::usage(format!(
                "the message is over {MAX_BYTES} bytes"
            )));
        }
        Ok(out)
    }
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

fn anchor_of(key: &str, value: &str) -> Result<Anchor, Failure> {
    Anchor::parse(value).ok_or_else(|| {
        Failure::usage(format!(
            "`{key}` names a terminal as kablo does (letters, digits, `.`, `_`, `-`; at most 64), not `{value}`"
        ))
    })
}

fn read_v1<'a>(verb: &'a str, lines: impl Iterator<Item = &'a str>) -> Result<Request, Failure> {
    match verb {
        "place" => {
            let fields = Fields::read(
                verb,
                lines,
                &["terminal", "anchor", "side", "ratio", "keep-open", "focus"],
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
            }))
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
            let fields = Fields::read(verb, lines, &["terminal"])?;
            Ok(Request::Close(fields.terminal("terminal")?))
        }
        "notify" => {
            let fields = Fields::read(verb, lines, &["text"])?;
            Ok(Request::Notify(fields.need("text")?.to_owned()))
        }
        other => Err(Failure::usage(format!(
            "`{other}` is no verb: place, ls, close or notify"
        ))),
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
        if version != 1 {
            return Err(Failure::new(
                Kind::Version,
                format!("version {version} is not read; this reads up to {VERSION}"),
            ));
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
        &["landed", "next-to", "squeezed", "hidden"],
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
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Version 1, frozen: a script on a server may be speaking it for as long as its terminal
    // lives. These are never edited; a later version gets its own constants.
    const V1_PLACE: &str =
        "bateri 1\nplace\nterminal t9\nanchor t4\nside below\nratio 30\nkeep-open yes\nfocus yes\n";
    const V1_PLACE_BARE: &str = "bateri 1\nplace\nterminal t9\n";
    const V1_LS: &str = "bateri 1\nls\n";
    const V1_LS_JSON: &str = "bateri 1\nls\nformat json\n";
    const V1_CLOSE: &str = "bateri 1\nclose\nterminal t7\n";
    const V1_NOTIFY: &str = "bateri 1\nnotify\ntext The build is done: 3 tests failed\n";
    // Added to version 1 after it was first written: an anchor that is no agent's number, which
    // the first spelling of `anchor` did not allow. Reading it changes nothing of the above.
    const V1_PLACE_MADE: &str =
        "bateri 1\nplace\nterminal t9\nanchor b-0123456789abcdef0123456789abcdef\nside right\n";
    const V1_REPLY_PLACED_MADE: &str = "bateri 1\nok placed\nlanded as_asked\nnext-to b-0123456789abcdef0123456789abcdef\nsqueezed no\nhidden no\n";
    const V1_REPLY_DONE: &str = "bateri 1\nok\n";
    const V1_REPLY_PLACED: &str =
        "bateri 1\nok placed\nlanded beside_group\nnext-to t4\nsqueezed no\nhidden yes\n";
    const V1_REPLY_TEXT: &str = "bateri 1\nok text\nWindow 1\n  Tab 1.1\n";
    const V1_REPLY_ERROR: &str = "bateri 1\nerror refused the user said no\n";

    fn place() -> Place {
        Place {
            terminal: Terminal(9),
            anchor: Some(Anchor::of(Terminal(4))),
            side: Some(Side::Below),
            ratio: Some(30),
            keep_open: true,
            focus: true,
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
        }
    }

    fn usage(text: &str) -> Failure {
        Request::parse(text).expect_err(text)
    }

    #[test]
    fn version_one_is_read_byte_for_byte() {
        let cases = [
            (V1_PLACE, Request::Place(place())),
            (V1_PLACE_BARE, Request::Place(bare())),
            (V1_LS, Request::Ls(Format::Text)),
            (V1_LS_JSON, Request::Ls(Format::Json)),
            (V1_CLOSE, Request::Close(Terminal(7))),
            (
                V1_NOTIFY,
                Request::Notify("The build is done: 3 tests failed".to_owned()),
            ),
        ];
        for (text, request) in cases {
            assert_eq!(Request::parse(text).as_ref(), Ok(&request), "{text:?}");
            // And what this writes is what version 1 wrote.
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
            Request::parse(V1_PLACE_MADE),
            Ok(Request::Place(wanted.clone()))
        );
        assert_eq!(
            Request::Place(wanted).encode().as_deref(),
            Ok(V1_PLACE_MADE)
        );
        // A name a person gave a terminal with kablo is as good, and so is an agent's number.
        for name in ["mine", "build.2", "t04", "t4"] {
            let text = format!("bateri 1\nplace\nterminal t9\nanchor {name}\n");
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
        });
        assert_eq!(Reply::parse(V1_REPLY_PLACED_MADE).as_ref(), Ok(&placed));
        assert_eq!(placed.encode(), V1_REPLY_PLACED_MADE);
        assert_eq!(
            usage("bateri 1\nplace\nterminal b-0123456789abcdef0123456789abcdef\n").kind,
            Kind::Usage
        );
        assert_eq!(usage("bateri 1\nclose\nterminal mine\n").kind, Kind::Usage);
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
            Request::Close(Terminal(120)),
            Request::Notify("two  spaces, and a trailing one ".to_owned()),
        ];
        for request in requests {
            let text = request.encode().expect("a request that can be written");
            assert_eq!(Request::parse(&text), Ok(request), "{text:?}");
        }
    }

    #[test]
    fn a_reader_takes_the_last_newline_or_leaves_it() {
        for text in [V1_PLACE, V1_PLACE.trim_end_matches('\n')] {
            assert_eq!(Request::parse(text), Ok(Request::Place(place())));
        }
        // The flags may be said `no`, and the keys come in any order.
        let text = "bateri 1\nplace\nfocus no\nside left\nterminal t2\nkeep-open no\n";
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
        let failure = usage("bateri 1\nnotify\ntext two\nlines\n");
        assert_eq!(failure.kind, Kind::Usage);
        assert!(failure.text.contains("`lines`"), "{failure:?}");
        // Another control character is no better than the line break.
        for text in [
            "bateri 1\nnotify\ntext a\rb\n",
            "bateri 1\nnotify\ntext a\tb\n",
            "bateri 1\nnotify\ntext a\u{1b}[31mb\n",
            "bateri 1\nnotify\ntext a\0b\n",
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
            ("bateri 1\n", "no verb"),
            ("bateri 1\nlist\n", "no verb"),
            ("bateri 1\nPLACE\nterminal t1\n", "no verb"),
            ("bateri 1\nplace\n", "needs a `terminal`"),
            ("bateri 1\nclose\n", "needs a `terminal`"),
            ("bateri 1\nnotify\n", "needs a `text`"),
            ("bateri 1\nplace\nterminal t1\nterminal t2\n", "twice"),
            (
                "bateri 1\nplace\nterminal t1\nlabel work\n",
                "takes no `label`",
            ),
            (
                "bateri 1\nclose\nterminal t1\nside left\n",
                "takes no `side`",
            ),
            ("bateri 1\nplace\nterminal\n", "no value"),
            ("bateri 1\nplace\nterminal \n", "no value"),
            ("bateri 1\nplace\nterminal t1\n\n", "empty"),
            ("bateri 1\nplace\n\nterminal t1\n", "empty"),
            ("bateri 1\nplace\nterminal b-0a1b\n", "names a terminal"),
            ("bateri 1\nplace\nterminal t09\n", "names a terminal"),
            (
                "bateri 1\nplace\nterminal t1\nanchor a/b\n",
                "names a terminal as kablo does",
            ),
            (
                "bateri 1\nplace\nterminal t1\nanchor ../t4\n",
                "names a terminal as kablo does",
            ),
            (
                "bateri 1\nplace\nterminal t1\nanchor not a name\n",
                "names a terminal as kablo does",
            ),
            ("bateri 1\nplace\nterminal t1\nanchor\n", "no value"),
            ("bateri 1\nplace\nterminal t1\nside top\n", "left, right"),
            ("bateri 1\nplace\nterminal t1\nside Left\n", "left, right"),
            ("bateri 1\nplace\nterminal t1\nratio 0\n", "1 to 99"),
            ("bateri 1\nplace\nterminal t1\nratio 100\n", "1 to 99"),
            ("bateri 1\nplace\nterminal t1\nratio 030\n", "1 to 99"),
            ("bateri 1\nplace\nterminal t1\nratio 30%\n", "1 to 99"),
            ("bateri 1\nplace\nterminal t1\nratio -3\n", "1 to 99"),
            (
                "bateri 1\nplace\nterminal t1\nkeep-open true\n",
                "yes or no",
            ),
            ("bateri 1\nls\nformat yaml\n", "text or json"),
            ("bateri 1\nls\nterminal t1\n", "takes no `terminal`"),
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
        let long = format!("bateri 1\nnotify\ntext {}\n", "x".repeat(MAX_BYTES));
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
    fn a_newer_version_is_its_own_error_so_a_script_can_try_an_older_one() {
        let failure = usage("bateri 2\nplace\nterminal t1\nfuture-key x\n");
        assert_eq!(failure.kind, Kind::Version);
        assert!(failure.text.contains("version 2"), "{failure:?}");
        assert_eq!(usage("bateri 4000000000\nls\n").kind, Kind::Version);
        // A number that does not fit is not a version.
        assert_eq!(usage("bateri 99999999999\nls\n").kind, Kind::Usage);
    }

    #[test]
    fn no_text_makes_the_reader_panic() {
        let seeds = [
            V1_PLACE,
            V1_LS,
            V1_CLOSE,
            V1_NOTIFY,
            V1_REPLY_PLACED,
            V1_REPLY_TEXT,
            V1_REPLY_ERROR,
            "bateri 1\nnotify\ntext héllo wörld ✓\n",
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
            "bateri 1\n\n",
            "\u{0}",
            "bateri 1\nls\n \n",
        ] {
            let _ = Request::parse(text);
            let _ = Reply::parse(text);
        }
    }

    #[test]
    fn the_reply_of_version_one_is_written_and_read_byte_for_byte() {
        let cases = [
            (V1_REPLY_DONE, Reply::Done),
            (
                V1_REPLY_PLACED,
                Reply::Placed(Placed {
                    landed: Landed::BesideGroup,
                    next_to: Some(Anchor::of(Terminal(4))),
                    squeezed: false,
                    hidden: true,
                }),
            ),
            (
                V1_REPLY_TEXT,
                Reply::Text("Window 1\n  Tab 1.1\n".to_owned()),
            ),
            (
                V1_REPLY_ERROR,
                Reply::Failed(Failure::new(Kind::Refused, "the user said no")),
            ),
        ];
        for (text, reply) in cases {
            assert_eq!(Reply::parse(text).as_ref(), Ok(&reply), "{text:?}");
            assert_eq!(reply.encode(), text, "{reply:?}");
        }
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
        });
        assert_eq!(Reply::parse(&reply.encode()), Ok(reply));
    }

    #[test]
    fn an_error_is_one_line_whatever_its_words_held() {
        let reply = Reply::Failed(Failure::new(Kind::Failed, "two\nlines\tand a tab"));
        let text = reply.encode();
        assert_eq!(text, "bateri 1\nerror failed two lines and a tab\n");
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
        assert!(Reply::parse("bateri 1\nerror bad x\n").is_err());
        assert!(Reply::parse("bateri 1\nmaybe\n").is_err());
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
}
