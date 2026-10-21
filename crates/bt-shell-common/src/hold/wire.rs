//! The wire between `bateri ssh-hold` and the application: what a pane's
//! program says to the running bateri, and what bateri says back.
//!
//! The connection is a local unix socket; the bytes on it are **frames**:
//! a kind (one byte), a payload length (four bytes, big endian) and the
//! payload. Every number inside a payload is big endian. A frame whose kind this
//! build does not know is skipped, so a later version can say more to a reader
//! that has not learned it yet; a payload longer than [`MAX_PAYLOAD`] or one
//! that does not fit its kind is a broken peer.
//!
//! **Versions.** The first frame is the holder's [`ToApp::Hello`] with the
//! newest wire version it speaks and the oldest; the application answers
//! [`ToHold::Welcome`] with the newest version both speak ([`agree`]) and
//! speaks that version from then on, or refuses. A running pane keeps the
//! `ssh-hold` it was started with across a bateri update, so the application
//! reads **the version it was shipped with and the one before it**
//! ([`OLDEST`]..=[`VERSION`]); a version leaves that range only when the
//! one after it has been shipped for a release. Every version's bytes are
//! frozen in the tests (`V1_FIXTURE`, `V2_FIXTURE`, `V3_FIXTURE`), so a change to the wire
//! adds a version and keeps the fixtures reading.
//!
//! **Version 2** lets the application ask the holder to do something on the
//! pane's terminal before the terminal is shown ([`ToHold::Run`], answered by
//! [`ToApp::Ran`]: the first connection to a server asks for a password, or
//! whether to trust its key, and only the program in front of the pane can ask
//! it there) and to tell the user a line ([`ToHold::Note`]). A version 1 holder
//! is never sent either: it is a pane that outlived an update and only ever
//! resumes a terminal.
//!
//! **Version 3** lets the application name the terminal: the holder makes a name for the terminal
//! it asks for (so a request repeated after a lost connection cannot make a second one), and the
//! application, which numbers the terminals of a server (`t7`: what an agent on the server is
//! told and says), may start the terminal under that number instead. [`ToHold::Bound`] then
//! carries the name, and the holder asks for the terminal by it from then on — the pipe that
//! resumes a terminal after an update or a crash must ask for the name the server knows. A
//! holder of version 2 or 1 is never sent a name: it keeps its own, and the terminal it asked for
//! is started under it.
//!
//! **What the offsets mean.** A terminal's output has a position, the
//! number of bytes the program wrote before it ([`ToHold::Output::end`] is the position
//! after the bytes it carries). The holder keeps only the last `end` it was
//! given and says it back when it connects to a bateri that was not there
//! before (an update, a crash): [`Target::Resume::offset`]. It never parses
//! the bytes and does not count them — a picture of the screen, which stands
//! in for bytes the server no longer holds, moves the position without being
//! that many bytes.

use kablo_proto::SessionId;
use kablo_proto::v1::{Exit, Size};

/// The newest wire version this build speaks.
pub const VERSION: u8 = 3;

/// The first wire version whose holder is told the name its terminal was started under
/// ([`ToHold::Bound::terminal`]); a holder of an earlier one keeps the name it made.
pub const NAMED: u8 = 3;

/// The oldest wire version this build still reads (see the module docs).
pub const OLDEST: u8 = 1;

/// The longest payload a frame may carry; a longer one is a broken peer.
pub const MAX_PAYLOAD: usize = 1 << 20;

/// The most bytes of terminal data one [`ToHold::Output`] or [`ToApp::Input`]
/// carries; more are sent as several frames in order.
pub const CHUNK: usize = 64 * 1024;

/// The longest account name the wire carries.
const ACCOUNT_LIMIT: usize = 255;

/// The most arguments one request carries (a command line is far fewer).
const ARGS_LIMIT: usize = 4096;

mod kind {
    pub const HELLO: u8 = 0x01;
    pub const OPEN: u8 = 0x02;
    pub const INPUT: u8 = 0x03;
    pub const RESIZE: u8 = 0x04;
    pub const RAN: u8 = 0x05;

    pub const WELCOME: u8 = 0x81;
    pub const REFUSED: u8 = 0x82;
    pub const BOUND: u8 = 0x83;
    pub const OUTPUT: u8 = 0x84;
    pub const END: u8 = 0x85;
    pub const BELL: u8 = 0x86;
    pub const RUN: u8 = 0x87;
    pub const NOTE: u8 = 0x88;
}

/// Why bytes are not a conversation this build can have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A frame announced more payload than [`MAX_PAYLOAD`].
    Oversize,
    /// A payload that does not fit its kind (too short, too long for what it
    /// names, a name that is not a name).
    Malformed,
}

/// Where a pane's terminal comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A terminal that does not exist yet: the `ssh` arguments as they were
    /// typed (or as the application composed them for a new pane on the same
    /// server). The application decides the account from them and what runs.
    New {
        /// The arguments, each as the bytes the shell gave.
        args: Vec<Vec<u8>>,
    },
    /// A terminal that exists: the account that holds it and the position of
    /// the next byte the pane is owed, or none when the pane has never seen
    /// the terminal (it is shown the screen as it is).
    Resume {
        /// The account the application named the terminal's server by
        /// ([`ToHold::Bound::account`]).
        account: String,
        /// The next byte owed.
        offset: Option<u64>,
    },
}

/// What the holder asks for once the versions are agreed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    /// The terminal's name, made by the holder, so a request repeated after a
    /// lost connection names the same terminal and cannot make a second. The
    /// application may start a new terminal under another name (version 3: it
    /// numbers them); a request for a terminal that exists says the name it has.
    pub terminal: SessionId,
    /// The pane's size now.
    pub size: Size,
    /// New or existing.
    pub target: Target,
}

/// What the holder says to the application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToApp {
    /// The first frame: the newest and the oldest version it speaks.
    Hello {
        /// The newest.
        newest: u8,
        /// The oldest.
        oldest: u8,
    },
    /// The request, sent once the application has agreed to a version.
    Open(Open),
    /// What the user typed, exactly.
    Input(Vec<u8>),
    /// The pane changed size.
    Resize(Size),
    /// The program asked for by [`ToHold::Run`] has ended (version 2): its
    /// exit status, or 128 plus the signal that ended it, or 255 when it
    /// could not be started.
    Ran {
        /// The status.
        status: u8,
    },
}

/// What the application says to the holder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToHold {
    /// The version both speak.
    Welcome {
        /// The version.
        version: u8,
    },
    /// No terminal will be shown. `fall_back`: the holder runs the plain
    /// `ssh` it was asked to replace (it was never shown a terminal);
    /// otherwise `text` is printed and it ends.
    Refused {
        /// Run plain `ssh` instead.
        fall_back: bool,
        /// What to tell the user.
        text: String,
    },
    /// The terminal exists on the server named `account`; the holder keeps the
    /// name to find it again.
    Bound {
        /// The server's account.
        account: String,
        /// The name the terminal was started under when it is not the one the holder
        /// asked with (version 3; a holder of an older version is sent none). The holder
        /// asks for the terminal by it from now on.
        terminal: Option<SessionId>,
    },
    /// Program output, exactly as it was written, in order.
    Output {
        /// The position after these bytes.
        end: u64,
        /// The bytes.
        data: Vec<u8>,
    },
    /// The conversation is over: the holder ends with `code`, and says `note`
    /// first if there is one.
    End {
        /// The process exit status.
        code: u8,
        /// A line for the user.
        note: Option<String>,
    },
    /// What was typed could not be sent (the connection to the server is
    /// down): ring the bell.
    Bell,
    /// Run this program on the pane's terminal, which is given back as it was
    /// for the time (version 2); the answer is [`ToApp::Ran`]. The holder
    /// takes standard input, output and error as they are, so what the program
    /// asks the user it asks on the pane.
    Run {
        /// The program and its arguments, each as the bytes to run.
        argv: Vec<Vec<u8>>,
    },
    /// A line for the user, shown before the terminal is (version 2).
    Note {
        /// The text, one line, without a line break.
        text: String,
    },
}

/// The version both sides speak, or none.
pub fn agree(newest: u8, oldest: u8) -> Option<u8> {
    let version = newest.min(VERSION);
    (version >= oldest && version >= OLDEST).then_some(version)
}

/// The process exit status a program's end stands for: its own status, or
/// 128 plus the signal as a shell reports it.
pub fn exit_status(exit: Exit) -> u8 {
    match exit {
        Exit::Code(code) => u8::try_from(code).unwrap_or(1),
        Exit::Signal(signal) => u8::try_from(128i32.saturating_add(signal)).unwrap_or(255),
    }
}

// Encoding

fn frame(out: &mut Vec<u8>, kind: u8, payload: &[u8]) {
    out.push(kind);
    // A payload past the limit is never built here: data is cut at `CHUNK`,
    // and the rest is a name or a command line.
    let length = u32::try_from(payload.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(payload);
}

fn put_size(out: &mut Vec<u8>, size: Size) {
    for value in [size.cols, size.rows, size.xpixel, size.ypixel] {
        out.extend_from_slice(&value.to_be_bytes());
    }
}

fn put_short(out: &mut Vec<u8>, bytes: &[u8]) {
    out.push(u8::try_from(bytes.len()).unwrap_or(u8::MAX));
    out.extend_from_slice(&bytes[..bytes.len().min(usize::from(u8::MAX))]);
}

impl ToApp {
    /// Appends the frame.
    pub fn encode(&self, out: &mut Vec<u8>) {
        let mut payload = Vec::new();
        match self {
            ToApp::Hello { newest, oldest } => {
                payload.extend_from_slice(&[*newest, *oldest]);
                frame(out, kind::HELLO, &payload);
            }
            ToApp::Open(open) => {
                put_short(&mut payload, open.terminal.as_str().as_bytes());
                put_size(&mut payload, open.size);
                match &open.target {
                    Target::New { args } => {
                        payload.push(0);
                        let count = u16::try_from(args.len()).unwrap_or(u16::MAX);
                        payload.extend_from_slice(&count.to_be_bytes());
                        for arg in args.iter().take(usize::from(count)) {
                            let length = u32::try_from(arg.len()).unwrap_or(u32::MAX);
                            payload.extend_from_slice(&length.to_be_bytes());
                            payload.extend_from_slice(arg);
                        }
                    }
                    Target::Resume { account, offset } => {
                        payload.push(1);
                        put_short(&mut payload, account.as_bytes());
                        match offset {
                            Some(offset) => {
                                payload.push(1);
                                payload.extend_from_slice(&offset.to_be_bytes());
                            }
                            None => payload.push(0),
                        }
                    }
                }
                frame(out, kind::OPEN, &payload);
            }
            ToApp::Input(bytes) => {
                for chunk in bytes.chunks(CHUNK) {
                    frame(out, kind::INPUT, chunk);
                }
            }
            ToApp::Resize(size) => {
                put_size(&mut payload, *size);
                frame(out, kind::RESIZE, &payload);
            }
            ToApp::Ran { status } => frame(out, kind::RAN, &[*status]),
        }
    }

    /// Reads one frame's payload. `Ok(None)`: a kind this build does not know.
    pub fn parse(kind: u8, payload: &[u8]) -> Result<Option<ToApp>, Error> {
        let mut at = Reader(payload);
        let message = match kind {
            kind::HELLO => ToApp::Hello {
                newest: at.u8()?,
                oldest: at.u8()?,
            },
            kind::OPEN => {
                let terminal = SessionId::from_bytes(at.short()?).map_err(|_| Error::Malformed)?;
                let size = at.size()?;
                let target = match at.u8()? {
                    0 => {
                        let count = usize::from(at.u16()?);
                        if count > ARGS_LIMIT {
                            return Err(Error::Malformed);
                        }
                        let mut args = Vec::with_capacity(count);
                        for _ in 0..count {
                            let length = at.u32()? as usize;
                            args.push(at.take(length)?.to_vec());
                        }
                        Target::New { args }
                    }
                    1 => {
                        let account = at.text()?;
                        let offset = match at.u8()? {
                            0 => None,
                            1 => Some(at.u64()?),
                            _ => return Err(Error::Malformed),
                        };
                        Target::Resume { account, offset }
                    }
                    _ => return Err(Error::Malformed),
                };
                at.done()?;
                ToApp::Open(Open {
                    terminal,
                    size,
                    target,
                })
            }
            kind::INPUT => ToApp::Input(payload.to_vec()),
            kind::RESIZE => {
                let size = at.size()?;
                at.done()?;
                ToApp::Resize(size)
            }
            kind::RAN => {
                let status = at.u8()?;
                at.done()?;
                ToApp::Ran { status }
            }
            _ => return Ok(None),
        };
        Ok(Some(message))
    }
}

impl ToHold {
    /// Appends the frame.
    pub fn encode(&self, out: &mut Vec<u8>) {
        let mut payload = Vec::new();
        match self {
            ToHold::Welcome { version } => frame(out, kind::WELCOME, &[*version]),
            ToHold::Refused { fall_back, text } => {
                payload.push(u8::from(*fall_back));
                payload.extend_from_slice(clip(text).as_bytes());
                frame(out, kind::REFUSED, &payload);
            }
            ToHold::Bound { account, terminal } => {
                put_short(&mut payload, account.as_bytes());
                // After the account, so the bytes of a bound without a name are the ones
                // version 1 wrote.
                if let Some(terminal) = terminal {
                    put_short(&mut payload, terminal.as_str().as_bytes());
                }
                frame(out, kind::BOUND, &payload);
            }
            ToHold::Output { end, data } => {
                // Each frame says where the stream stands after its own bytes.
                // A write that is not cut says `end` as it is: a piece of a
                // picture carries more bytes than the position it stands at, and
                // working the position back from the length would move it.
                let whole = data.len() <= CHUNK;
                let mut stands = end.saturating_sub(data.len() as u64);
                for chunk in data.chunks(CHUNK) {
                    stands += chunk.len() as u64;
                    payload.clear();
                    let at = if whole { *end } else { stands };
                    payload.extend_from_slice(&at.to_be_bytes());
                    payload.extend_from_slice(chunk);
                    frame(out, kind::OUTPUT, &payload);
                }
            }
            ToHold::End { code, note } => {
                payload.push(*code);
                if let Some(note) = note {
                    payload.extend_from_slice(clip(note).as_bytes());
                }
                frame(out, kind::END, &payload);
            }
            ToHold::Bell => frame(out, kind::BELL, &[]),
            ToHold::Run { argv } => {
                let count = u16::try_from(argv.len()).unwrap_or(u16::MAX);
                payload.extend_from_slice(&count.to_be_bytes());
                for arg in argv.iter().take(usize::from(count)) {
                    let length = u32::try_from(arg.len()).unwrap_or(u32::MAX);
                    payload.extend_from_slice(&length.to_be_bytes());
                    payload.extend_from_slice(arg);
                }
                frame(out, kind::RUN, &payload);
            }
            ToHold::Note { text } => frame(out, kind::NOTE, clip(text).as_bytes()),
        }
    }

    /// Reads one frame's payload. `Ok(None)`: a kind this build does not know.
    pub fn parse(kind: u8, payload: &[u8]) -> Result<Option<ToHold>, Error> {
        let mut at = Reader(payload);
        let message = match kind {
            kind::WELCOME => {
                let version = at.u8()?;
                at.done()?;
                ToHold::Welcome { version }
            }
            kind::REFUSED => ToHold::Refused {
                fall_back: at.u8()? != 0,
                text: at.rest_text(),
            },
            kind::BOUND => {
                let account = at.text()?;
                let terminal = if at.is_empty() {
                    None
                } else {
                    Some(SessionId::new(&at.text()?).map_err(|_| Error::Malformed)?)
                };
                at.done()?;
                ToHold::Bound { account, terminal }
            }
            kind::OUTPUT => ToHold::Output {
                end: at.u64()?,
                data: at.rest().to_vec(),
            },
            kind::END => {
                let code = at.u8()?;
                let note = at.rest_text();
                ToHold::End {
                    code,
                    note: (!note.is_empty()).then_some(note),
                }
            }
            kind::BELL => {
                at.done()?;
                ToHold::Bell
            }
            kind::RUN => {
                let count = usize::from(at.u16()?);
                if count > ARGS_LIMIT {
                    return Err(Error::Malformed);
                }
                let mut argv = Vec::with_capacity(count);
                for _ in 0..count {
                    let length = at.u32()? as usize;
                    argv.push(at.take(length)?.to_vec());
                }
                at.done()?;
                ToHold::Run { argv }
            }
            kind::NOTE => ToHold::Note {
                text: at.rest_text(),
            },
            _ => return Ok(None),
        };
        Ok(Some(message))
    }
}

/// A line of text for a frame: cut to what a frame is meant to carry (1 KiB),
/// at a character boundary.
fn clip(text: &str) -> &str {
    const LIMIT: usize = 1024;
    if text.len() <= LIMIT {
        return text;
    }
    let mut end = LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// A payload being read.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.0.len() < n {
            return Err(Error::Malformed);
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, Error> {
        let bytes: [u8; 2] = self.take(2)?.try_into().map_err(|_| Error::Malformed)?;
        Ok(u16::from_be_bytes(bytes))
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let bytes: [u8; 4] = self.take(4)?.try_into().map_err(|_| Error::Malformed)?;
        Ok(u32::from_be_bytes(bytes))
    }

    fn u64(&mut self) -> Result<u64, Error> {
        let bytes: [u8; 8] = self.take(8)?.try_into().map_err(|_| Error::Malformed)?;
        Ok(u64::from_be_bytes(bytes))
    }

    fn size(&mut self) -> Result<Size, Error> {
        Ok(Size {
            cols: self.u16()?,
            rows: self.u16()?,
            xpixel: self.u16()?,
            ypixel: self.u16()?,
        })
    }

    /// A length byte and that many bytes.
    fn short(&mut self) -> Result<&'a [u8], Error> {
        let length = usize::from(self.u8()?);
        self.take(length)
    }

    /// A name: short and text.
    fn text(&mut self) -> Result<String, Error> {
        let bytes = self.short()?;
        if bytes.len() > ACCOUNT_LIMIT {
            return Err(Error::Malformed);
        }
        String::from_utf8(bytes.to_vec()).map_err(|_| Error::Malformed)
    }

    fn rest(&mut self) -> &'a [u8] {
        std::mem::take(&mut self.0)
    }

    fn rest_text(&mut self) -> String {
        String::from_utf8_lossy(self.rest()).into_owned()
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn done(&self) -> Result<(), Error> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(Error::Malformed)
        }
    }
}

/// Frames out of whatever pieces the socket delivers.
#[derive(Default)]
pub struct Frames {
    buffer: Vec<u8>,
}

impl Frames {
    /// An empty reader.
    pub fn new() -> Frames {
        Frames::default()
    }

    /// Bytes arrived.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// The next whole frame as its kind and payload, or none until more bytes
    /// arrive. An announced length past [`MAX_PAYLOAD`] is an error before
    /// the payload is waited for.
    pub fn frame(&mut self) -> Result<Option<(u8, Vec<u8>)>, Error> {
        let Some(head) = self.buffer.get(..5) else {
            return Ok(None);
        };
        let length = u32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize;
        if length > MAX_PAYLOAD {
            return Err(Error::Oversize);
        }
        let Some(payload) = self.buffer.get(5..5 + length) else {
            return Ok(None);
        };
        let frame = (self.buffer[0], payload.to_vec());
        self.buffer.drain(..5 + length);
        Ok(Some(frame))
    }
}

/// Frames the holder sent, as messages: the unknown kinds are skipped.
#[derive(Default)]
pub struct AppReader {
    frames: Frames,
}

impl AppReader {
    /// An empty reader.
    pub fn new() -> AppReader {
        AppReader::default()
    }

    /// Bytes arrived.
    pub fn push(&mut self, bytes: &[u8]) {
        self.frames.push(bytes);
    }

    /// The next message, or none until more bytes arrive.
    pub fn read(&mut self) -> Result<Option<ToApp>, Error> {
        while let Some((kind, payload)) = self.frames.frame()? {
            if let Some(message) = ToApp::parse(kind, &payload)? {
                return Ok(Some(message));
            }
        }
        Ok(None)
    }
}

/// Frames the application sent, as messages: the unknown kinds are skipped.
#[derive(Default)]
pub struct HoldReader {
    frames: Frames,
}

impl HoldReader {
    /// An empty reader.
    pub fn new() -> HoldReader {
        HoldReader::default()
    }

    /// Bytes arrived.
    pub fn push(&mut self, bytes: &[u8]) {
        self.frames.push(bytes);
    }

    /// The next message, or none until more bytes arrive.
    pub fn read(&mut self) -> Result<Option<ToHold>, Error> {
        while let Some((kind, payload)) = self.frames.frame()? {
            if let Some(message) = ToHold::parse(kind, &payload)? {
                return Ok(Some(message));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
