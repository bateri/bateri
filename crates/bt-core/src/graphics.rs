//! The kitty graphics protocol: `ESC _ G <keys> ; <payload> ESC \`.
//!
//! Programs send pixels this way (`kitten icat`, `chafa --format=kitty`,
//! terminal-browser). This module is the protocol and the storage; it knows
//! nothing about the grid. The command arrives where it takes effect: the
//! vendored parser hands the body of `ESC _ … ESC \` to the parser's wrapper
//! (`handler::ClusterHandler`) in the order of the stream, so what the
//! command does to the screen, the cursor and the program's input is decided
//! at its place in the stream — after an `?1049h` before it, in a synchronized
//! update (`?2026`) at the update's end, before an `ESC [ c` that follows it.
//! The pieces:
//!
//! - **The command** ([`Commands`]) is put together from the body's slices
//!   ([`Collector`]) and received ([`Receiver`]): the chunks of a transmission
//!   are joined as base64 text, nothing is decoded, and what comes out is a
//!   [`Pending`] record or, for a query whose pixels are in the sequence, an
//!   answer. This is cheap on purpose — the parser runs under the terminal's
//!   lock. A query that carries its own pixels is decoded there, and may only
//!   be small: in its text ([`QUERY_LIMIT`]) and in the pixels the text comes
//!   to ([`QUERY_BYTES`]).
//! - **The decoding** ([`Hub::decode_pending`]) is the expensive part —
//!   base64, zlib, PNG, and the file or shared memory a command names. A
//!   command that needs it ends with the parser *halted* right behind it: the
//!   read loop lets go of the terminal's lock and lease, decodes, takes both
//!   back and goes on ([`Commands::halted`]). A frame waits for either, and
//!   an image can take long to inflate or a disk to answer.
//! - **The applying** ([`Hub::apply`]) puts the record into the store of the
//!   screen that is showing at that point of the stream and answers the
//!   program, with the terminal locked and the cursor where the stream left
//!   it.
//!
//! **Placing** is the parser's side too. A placement is anchored to the
//! cursor's grid line where its command ends; when the command moves the
//! cursor (`C=0`, the default) the movement is the command's ([`Motion`]): the
//! wrapper moves the cursor down, anchors the placement and moves it right.
//! A stream with no store to place into — a bare replay of the same bytes — makes
//! the same movement from the same numbers and nothing else. How placements keep their lines while the content
//! scrolls is [`follow`]'s, and what the frame draws is [`Pictures`].
//!
//! **What a program can do with it is what the protocol lets it.** A command
//! is the body of an application program command, nothing else is looked at
//! for it, and the cursor moves by numbers the command's own keys give — the
//! size of the image — never by what a file holds.
//!
//! **Where the pixels come from** is the command's `t=`. In the escape
//! sequence itself (`d`) they travel as the payload. A file (`f`), a
//! temporary file (`t`) or a shared memory object (`s`) is named by the payload
//! instead, and [`source`] reads it — where the decoding is, never in the
//! parser — under rules that treat the name as what it is: text any program,
//! and anything a program prints, can write, and the server's, not this
//! machine's, in a remote session (where it is refused). The cursor's
//! movement is never made from a file: a named image says how big it is in its
//! `s=` and `v=` keys (`kitten icat` sends them for a PNG too), and one that
//! does not leaves the cursor where it is.
//!
//! **A placement that no cell of its own holds** (`U=1`) is a box an image is
//! fitted to, and the character `U+10EEEE` in the grid's cells is what shows
//! it ([`placeholder`]): the frame reads the cells when such a box exists and
//! hands over a [`Picture`] for each stretch of them, so that text moves the
//! image.
//!
//! **A placement that hangs on another** (`P=`) has no line of its own: where
//! it is comes from its parent when the frame is drawn, and it ends with the
//! parent. **An image drawn from the palette** (`f=8`, `f=16`) keeps its
//! indices, and the pixels that are drawn are made again for each palette the
//! terminal has ([`palette`]).
//!
//! Nothing here draws. The store holds decoded pixels and placements per
//! screen ([`Images`]); the frame hands them over as [`Picture`]s.

mod apc;
mod canvas;
mod command;
mod edits;
mod features;
mod follow;
mod palette;
mod place;
mod placeholder;
mod receive;
mod source;
mod store;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use ::pencere_host::frame::{BodyDecoder, Direction, Event, Message};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Term;

use crate::handler::KeptModes;
use crate::pencere::Desk;
use crate::pointer::PointerShapes;
use crate::shell::BlockKey;

pub(crate) use apc::Collector;
pub use edits::{Dirty, Edits};
pub(crate) use place::Motion;
pub(crate) use place::Screen;
pub use place::{Picture, PictureLayer, Pictures};
pub(crate) use placeholder::has_glyph;
pub(crate) use receive::{Env, Outcome, Receiver};
pub(crate) use store::{Window, screen_of};

use command::{Action, Command, Medium};
use placeholder::Marks;
use receive::{Decoded, Room};
use store::{Images, View};

/// The most stored pixel bytes one screen keeps: kitty's own quota, so a
/// program that fits kitty fits here. Past it the images with no placement
/// go first, then the oldest.
pub(crate) const QUOTA_BYTES: usize = 320 * 1024 * 1024;

/// The longest side of an image, kitty's limit.
pub(crate) const MAX_DIMENSION: u32 = 10_000;

/// The most base64 text one command may carry. Chunked transmission (4 KiB a
/// command, as the protocol asks) never gets near it; a single giant command
/// is held whole in memory, so it is bounded well under the quota.
pub(crate) const PAYLOAD_LIMIT: usize = 64 * 1024 * 1024;

/// The most base64 text a transmission may add up to over its chunks. The
/// text is kept whole until the image is decoded, so this is what a
/// transmission costs in memory before the decoded image does; past it a
/// program compresses (`o=z`) or sends a PNG.
pub(crate) const TRANSMISSION_LIMIT: usize = 192 * 1024 * 1024;

/// The most bytes read from a file or shared memory object, where the image
/// is not raw pixels of a known size: the decoded size of
/// [`TRANSMISSION_LIMIT`] worth of base64, so the three media cost the same
/// memory.
pub(crate) const FILE_LIMIT: usize = TRANSMISSION_LIMIT / 4 * 3;

/// The longest file or shared memory name, kitty's limit.
pub(crate) const NAME_LIMIT: usize = 2048;

/// The most base64 text a query (`a=q`) may carry. A query is answered the
/// moment it is read, so it is decoded there and then, in the read path; real
/// ones are a pixel. A query that names a file reads at most the same.
pub(crate) const QUERY_LIMIT: usize = 64 * 1024;

/// The most pixel bytes (RGBA) the image a query carries in its sequence may
/// come to. [`QUERY_LIMIT`] bounds the text, but the text is not what the
/// decoding costs: a PNG or a zlib stream of a few kilobytes can say it holds
/// the quota's worth of pixels, and the query is decoded where it ends, under
/// the terminal's lock, with every frame waiting for it. **The size is judged
/// before anything is inflated or allocated** — from the keys of raw pixels,
/// from the PNG's header — and a compressed payload may inflate to no more
/// than this either. A 512 × 512 image: real queries are a pixel, and no raw
/// payload [`QUERY_LIMIT`] lets through comes near it.
pub(crate) const QUERY_BYTES: usize = 1024 * 1024;

/// The most images one screen keeps. The quota counts bytes, so on its own it
/// lets in millions of one-pixel images, and storing one looks through all
/// of them; past this an image goes the way it goes past the quota (those no
/// placement shows first, then the oldest). Far more than a screen and its
/// history can show.
pub(crate) const IMAGE_LIMIT: usize = 4096;

/// The most placements one screen keeps. A put with no placement id adds one
/// every time (some 17 bytes of output), and every frame looks through them
/// all, sorts those it shows and hands each over; past this the oldest
/// placement goes, and its image stays for the program to show again.
pub(crate) const PLACEMENT_LIMIT: usize = 16 * 1024;

/// How long a chunked transmission may wait for its next chunk before it is
/// dropped. Checked when the next command arrives, not by a timer.
const OPEN_TIMEOUT: Duration = Duration::from_secs(30);

/// The keys that address a command's answer: which image, which placement,
/// and how much of the answer the program wants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Echo {
    /// `i=`: the image id; 0 when absent.
    pub(crate) id: u32,
    /// `I=`: the image number; 0 when absent.
    pub(crate) number: u32,
    /// `p=`: the placement id; 0 when absent.
    pub(crate) placement: u32,
    /// `q=`: 1 hides the success answer, 2 hides every answer.
    pub(crate) quiet: u8,
}

impl Echo {
    /// Reads the addressing keys of control data that may be otherwise
    /// unreadable: a key that does not parse is left at 0, so the answer to
    /// a broken command still reaches the program that sent it.
    pub(crate) fn lenient(control: &[u8]) -> Self {
        let mut echo = Self::default();
        for (key, value) in command::pairs(control) {
            let read = command::number(value).unwrap_or(0);
            match key {
                b'i' => echo.id = read,
                b'I' => echo.number = read,
                b'p' => echo.placement = read,
                b'q' => echo.quiet = u8::try_from(read).unwrap_or(0),
                _ => {}
            }
        }
        echo
    }
}

/// Why a command failed, in the protocol's words: `EINVAL:message`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Error {
    code: &'static str,
    message: String,
}

impl Error {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// A command or payload that cannot be understood.
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::new("EINVAL", message)
    }

    /// A payload shorter than the dimensions say.
    pub(crate) fn short(message: impl Into<String>) -> Self {
        Self::new("ENODATA", message)
    }

    /// A payload or image over a limit.
    pub(crate) fn too_big(message: impl Into<String>) -> Self {
        Self::new("EFBIG", message)
    }

    /// A command that names an image there is none of.
    pub(crate) fn missing(message: impl Into<String>) -> Self {
        Self::new("ENOENT", message)
    }

    /// A relative placement whose parent there is none of.
    pub(crate) fn no_parent(message: impl Into<String>) -> Self {
        Self::new("ENOPARENT", message)
    }

    /// A relative placement that would hang on itself, through its parents.
    pub(crate) fn cycle(message: impl Into<String>) -> Self {
        Self::new("ECYCLE", message)
    }

    /// A relative placement with more parents above it than are followed.
    pub(crate) fn too_deep(message: impl Into<String>) -> Self {
        Self::new("ETOODEEP", message)
    }

    /// An image that cannot be kept.
    pub(crate) fn no_space(message: impl Into<String>) -> Self {
        Self::new("ENOSPC", message)
    }

    /// A file or shared memory object that could not be read, for whatever
    /// reason: kitty's own words. One answer for all of them, and none of it
    /// the program's: a refusal that says why would tell a program that
    /// only prints text which files this machine has and how big they are,
    /// and an answer that repeats a name would write the program's text
    /// into the PTY's input.
    pub(crate) fn unreadable() -> Self {
        Self::new("EBADF", "Failed to read image file")
    }

    #[cfg(test)]
    pub(crate) fn code(&self) -> &'static str {
        self.code
    }

    #[cfg(test)]
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

/// The answer to one command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Reply {
    echo: Echo,
    result: Result<(), Error>,
    /// Keys of the answer after the addressing ones, each as `k=v`: the frame
    /// a frame command named, what a feature query understood.
    keys: Vec<String>,
}

impl Reply {
    fn ok(echo: Echo) -> Self {
        Self {
            echo,
            result: Ok(()),
            keys: Vec::new(),
        }
    }

    fn failed(echo: Echo, error: Error) -> Self {
        Self {
            echo,
            result: Err(error),
            keys: Vec::new(),
        }
    }

    /// This answer with `keys` after the addressing ones.
    fn with_keys(mut self, keys: Vec<String>) -> Self {
        self.keys = keys;
        self
    }

    /// An answer to a frame command, which says which frame it asked for.
    fn of_frame(self, command: &Command) -> Self {
        match command.rows {
            0 => self,
            frame => self.with_keys(vec![format!("r={frame}")]),
        }
    }

    /// The bytes to write to the program, or `None` when the program asked
    /// not to hear (`q=`) or gave no way to address an answer (no `i=` and
    /// no `I=`).
    pub(crate) fn encode(&self) -> Option<Vec<u8>> {
        let echo = self.echo;
        if echo.id == 0 && echo.number == 0 {
            return None;
        }
        let hidden = match &self.result {
            Ok(()) => echo.quiet >= 1,
            Err(_) => echo.quiet >= 2,
        };
        if hidden {
            return None;
        }
        let mut keys = Vec::new();
        if echo.id != 0 {
            keys.push(format!("i={}", echo.id));
        }
        if echo.number != 0 {
            keys.push(format!("I={}", echo.number));
        }
        if echo.placement != 0 {
            keys.push(format!("p={}", echo.placement));
        }
        keys.extend(self.keys.iter().cloned());
        let body = match &self.result {
            Ok(()) => "OK".to_owned(),
            Err(error) => format!("{}:{}", error.code, error.message),
        };
        Some(format!("\x1b_G{};{body}\x1b\\", keys.join(",")).into_bytes())
    }
}

/// A command received, and what is left to do with it.
pub(crate) enum Pending {
    /// A transmission that has not been decoded yet: [`Hub::decode_pending`]
    /// turns it into a `Store` or a `Reply`. `text` is the payload as it
    /// came — base64 pixels — or, when the command names a file or shared
    /// memory object (`t=f/t/s`), the name, which is read then. The `motion`
    /// is how the cursor moves around it if the command shows the image
    /// (`a=T`).
    Raw {
        command: Command,
        text: Vec<u8>,
        motion: Motion,
    },
    /// Keep a decoded image (and show it, for `a=T`).
    Store {
        command: Command,
        decoded: Decoded,
        motion: Motion,
    },
    /// Lay a decoded rectangle on a frame of a stored image, or make a frame of
    /// it (`a=f`). It moves nothing: a frame is not shown by itself.
    Frame { command: Command, decoded: Decoded },
    /// Lay a rectangle of one frame on another (`a=c`).
    Compose(Command),
    /// Control how an image plays (`a=a`), at the instant the command came.
    Animate { command: Command, now: Instant },
    /// Show a stored image (`a=p`).
    Put { command: Command, motion: Motion },
    /// Remove placements and, for the upper-case forms, image data.
    Delete(Command),
    /// An answer that goes where the command was: it failed before it could
    /// be applied. The cursor still moves by `motion`, as it does for the
    /// command that was meant (a replay with no store, which reads no file,
    /// moves it that way too).
    Reply { reply: Reply, motion: Motion },
}

impl Pending {
    /// How the cursor moves around this record.
    pub(crate) fn motion(&self) -> Motion {
        match self {
            Self::Raw { motion, .. }
            | Self::Store { motion, .. }
            | Self::Put { motion, .. }
            | Self::Reply { motion, .. } => *motion,
            Self::Delete(_) | Self::Frame { .. } | Self::Compose(_) | Self::Animate { .. } => {
                Motion::NONE
            }
        }
    }

    /// Whether the record names a file or shared memory object that is still
    /// to be read.
    fn names_something(&self) -> bool {
        matches!(self, Self::Raw { command, .. } if command.medium != Medium::Direct)
    }
}

/// The graphics commands of one stream, and what the stream's parser has of
/// them between its calls.
///
/// The parser's wrapper is made anew for every slice of bytes it is given,
/// but a command crosses slices, so the state is the owner's of the parser (the
/// reader loop's) and is lent to each wrapper. One per
/// stream, never shared between two.
pub(crate) struct Commands {
    collector: Collector,
    receiver: Receiver,
    /// A record waits outside the parser for something that cannot be done
    /// under the terminal's lock (the decoding): the parser stops right behind
    /// the command and takes no byte until this is taken back.
    halt: bool,
    /// Whether the pane was in a remote session at some point from the first
    /// byte of the command being received to now: the names in it are the
    /// server's. See [`Hub::note_remote`].
    remote: bool,
    /// A cell's size in pixels, for a stream that has no session to ask.
    cell: (u16, u16),
    /// The frames of the view protocol (an application program command that
    /// begins `pencere;`), put together as the kitty commands are: the body
    /// comes in slices and a message is several frames, so what is half-read
    /// belongs to the stream and not to the parser's wrapper. Idle — taking
    /// nothing — unless [`Commands::start_frames`] was called for the command
    /// being received.
    frames: BodyDecoder,
    /// What `frames` delivered and nobody has taken, reused so that a frame
    /// costs no allocation.
    events: Vec<Event>,
    /// The message the last frame ended, waiting to be taken
    /// ([`Commands::take_message`]).
    message: Option<Message>,
}

impl Default for Commands {
    fn default() -> Self {
        Self::with_limit(PAYLOAD_LIMIT)
    }
}

impl Commands {
    /// `payload_limit`: the most base64 bytes of one command that are kept.
    pub(crate) fn with_limit(payload_limit: usize) -> Self {
        Self {
            collector: Collector::new(payload_limit),
            receiver: Receiver::new(),
            halt: false,
            remote: false,
            cell: (0, 0),
            frames: BodyDecoder::new(Direction::ToHost),
            events: Vec::new(),
            message: None,
        }
    }

    /// The cell size a stream without a session works with.
    #[cfg(test)]
    pub(crate) fn set_cell(&mut self, cell: (u16, u16)) {
        self.cell = cell;
    }

    pub(crate) fn cell(&self) -> (u16, u16) {
        self.cell
    }

    /// `ESC _`: a command begins. `remote`: the pane was seen in a remote
    /// session since the stream was last caught up.
    pub(crate) fn start(&mut self, remote: bool) {
        self.collector.start();
        self.remote = remote;
    }

    /// The command being received is also read as a frame of the view
    /// protocol: only a pane that has a desk to hear it asks. A body that does
    /// not begin `pencere;` is let go at its first byte that differs, so the
    /// kitty commands pay nothing for it.
    pub(crate) fn start_frames(&mut self) {
        self.frames.start();
    }

    /// A slice of the body.
    pub(crate) fn put(&mut self, data: &[u8]) {
        self.collector.put(data);
        self.frames.put(data, &mut self.events);
        self.keep_message();
    }

    /// The message a frame ended, once: the parser parks it with the desk and
    /// halts behind it ([`Desk::park`]).
    pub(crate) fn take_message(&mut self) -> Option<Message> {
        self.message.take()
    }

    /// Keeps the message in `events`, if the last frame ended one. What the
    /// decoder dropped (a malformed frame, one too long) is nobody's: the
    /// stream is anyone's to write.
    fn keep_message(&mut self) {
        for event in self.events.drain(..) {
            if let Event::Message(message) = event {
                self.message = Some(message);
            }
        }
    }

    /// The body ended. What it comes to if it is a whole graphics command, in
    /// order: `seen`: the pane was seen in a remote session since the stream
    /// was last caught up; `remote`: whether it is in one now (a command
    /// that names a file is refused if either is so — the scanner has seen the
    /// whole read by the time the parser gets here, and a session that ended in
    /// it would no longer be remote while the name is still the server's);
    /// `stored`: the size of an image the store holds.
    pub(crate) fn end(
        &mut self,
        cancelled: bool,
        now: Instant,
        env: Env,
        stored: &dyn Fn(Echo) -> Option<(u32, u32)>,
        (seen, remote): (bool, &dyn Fn() -> bool),
    ) -> Vec<Outcome> {
        self.frames.end(cancelled, &mut self.events);
        self.keep_message();
        let Some(apc) = self.collector.end(cancelled) else {
            return Vec::new();
        };
        let tainted = self.remote || seen;
        let mut outcomes = Vec::new();
        self.receiver
            .receive(apc, now, env, stored, &|| tainted || remote(), |outcome| {
                outcomes.push(outcome)
            });
        outcomes
    }

    /// Whether a command is half-received: bytes the parser took that no
    /// record carries yet.
    pub(crate) fn open(&self) -> bool {
        self.collector.inside() || self.receiver.is_open()
    }

    /// Whether the parser is to stop where it is ([`Commands::halt`]).
    pub(crate) fn halted(&self) -> bool {
        self.halt
    }

    /// Asks for the parser to stop right behind the command it just ended.
    pub(crate) fn halt(&mut self) {
        self.halt = true;
    }

    /// Takes the request back: the parser goes on.
    pub(crate) fn resume(&mut self) {
        self.halt = false;
    }

    /// Whether a command is being received (the body is open), whatever it
    /// is.
    pub(crate) fn receiving(&self) -> bool {
        self.collector.inside()
    }
}

/// The store of images and the record of the command the parser is halted
/// for.
///
/// One per session, shared by the reader and the frame. The slot is a leaf
/// lock; the store comes after the terminal's own lock in the lock order
/// (`term` → `images`), because the parser applies commands while it holds
/// `Term`.
pub(crate) struct Hub {
    images: Mutex<Images>,
    /// The record the halted parser waits for (at most one: nothing is
    /// received while it is there).
    parked: Mutex<Option<Pending>>,
    /// Called before each image is decoded (a test sees what is locked then).
    #[cfg(test)]
    decoding: Mutex<Option<Box<dyn Fn() + Send>>>,
    /// Whether a command is half-received ([`Commands::open`]): the next read may
    /// carry its rest.
    open: AtomicBool,
    /// Whether the pane is in a remote session, asked when a file is about
    /// to be read: the name then belongs to another machine.
    remote: OnceLock<Box<dyn Fn() -> bool + Send + Sync>>,
    /// Whether the pane was in a remote session at a read that may have
    /// carried a command, since the parser last caught up with the reads
    /// ([`Hub::note_remote`]).
    remote_seen: AtomicBool,
    /// Whether the images that have a gap play by themselves. Off while the
    /// system asks for less motion: a program's own change of frame still shows,
    /// the terminal's does not come.
    playing: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Hub {
    pub(crate) fn new() -> Self {
        Self::with_quota(QUOTA_BYTES)
    }

    /// `quota`: the pixel bytes one screen keeps.
    fn with_quota(quota: usize) -> Self {
        Self {
            images: Mutex::new(Images::with_quota(quota)),
            parked: Mutex::new(None),
            #[cfg(test)]
            decoding: Mutex::new(None),
            open: AtomicBool::new(false),
            remote: OnceLock::new(),
            remote_seen: AtomicBool::new(false),
            playing: AtomicBool::new(true),
        }
    }

    /// Whether the animations the terminal runs go on (they do, but where
    /// motion is to be reduced).
    pub(crate) fn set_playing(&self, playing: bool) {
        self.playing.store(playing, Ordering::Relaxed);
    }

    /// Holds the record of the command the parser has just halted behind.
    pub(crate) fn park(&self, pending: Pending) {
        *lock(&self.parked) = Some(pending);
    }

    /// Takes the record the parser was halted for, decoded by
    /// [`Hub::decode_pending`] if it needed it.
    pub(crate) fn take_parked(&self) -> Option<Pending> {
        lock(&self.parked).take()
    }

    /// Decodes the parked transmission, if it is still in base64 — the
    /// expensive half of receiving an image. The read loop calls it with
    /// the parser halted and neither the terminal's lock nor its lease held,
    /// so a frame does not wait for an image to inflate. Only the reader
    /// takes the slot's lock, so holding it meanwhile blocks no one.
    pub(crate) fn decode_pending(&self) {
        // Asked once, before the slot is locked (the answer comes from the
        // shell's state, a lock of its own), and only if the record names
        // something.
        let names = lock(&self.parked)
            .as_ref()
            .is_some_and(Pending::names_something);
        let remote = names && self.in_remote_session();
        let mut slot = lock(&self.parked);
        if !matches!(*slot, Some(Pending::Raw { .. })) {
            return;
        }
        #[cfg(test)]
        if let Some(hook) = lock(&self.decoding).as_ref() {
            hook();
        }
        let Some(Pending::Raw {
            command,
            text,
            motion,
        }) = slot.take()
        else {
            return;
        };
        *slot = Some(self.decoded(command, text, motion, remote));
    }

    /// Installs the question "is this pane in a remote session", asked when
    /// a record that names a file or shared memory object is decoded. The
    /// parser asks it too, when it ends the command ([`Commands::end`]); a
    /// command is refused if either answer is yes, since a session can be
    /// found to be remote between the two, and can end between them (the
    /// scanner has seen the whole read by the time the parser gets to it).
    pub(crate) fn set_remote_gate(&self, gate: Box<dyn Fn() -> bool + Send + Sync>) {
        let _ = self.remote.set(gate);
    }

    /// Whether the pane is in a remote session now.
    pub(crate) fn in_remote_session(&self) -> bool {
        self.remote.get().is_some_and(|gate| gate())
    }

    /// The pane is in a remote session as a read is taken in that may carry a
    /// command: before the scanner has looked at it, so a session that ends in
    /// the read is still counted. Kept until the parser has caught up with the
    /// reads ([`Hub::forget_remote`]); a command that begins in the meantime
    /// remembers it for itself.
    pub(crate) fn note_remote(&self) {
        self.remote_seen.store(true, Ordering::Release);
    }

    /// Whether [`Hub::note_remote`] has been called since the parser last
    /// caught up.
    pub(crate) fn remote_seen(&self) -> bool {
        self.remote_seen.load(Ordering::Acquire)
    }

    /// The parser has caught up with every read and is in no command: what
    /// was noted belongs to bytes it has dealt with.
    pub(crate) fn forget_remote(&self) {
        self.remote_seen.store(false, Ordering::Release);
    }

    /// The record a transmission comes to once its text is decoded.
    /// `motion`: how the cursor moves around it.
    fn decoded(&self, command: Command, text: Vec<u8>, motion: Motion, remote: bool) -> Pending {
        let echo = command.echo;
        // A name is the machine's of whoever wrote it. In a remote session
        // that is not this one: refuse it, with nothing touched.
        let decoded = if remote && command.medium != Medium::Direct {
            Err(Error::unreadable())
        } else {
            receive::decode_payload(&command, text, Room::Store)
        };
        match decoded {
            // A query that names a file is a transmission that is not kept:
            // it only waits here, for its place in the stream, to be answered
            // in order.
            Ok(_) if command.action == Action::Query => Pending::Reply {
                reply: Reply::ok(echo),
                motion,
            },
            Ok(decoded) if command.action == Action::Frame => Pending::Frame { command, decoded },
            Ok(decoded) => Pending::Store {
                command,
                decoded,
                motion,
            },
            Err(error) => Pending::Reply {
                reply: if command.action == Action::Frame {
                    Reply::failed(echo, error).of_frame(&command)
                } else {
                    Reply::failed(echo, error)
                },
                motion,
            },
        }
    }

    /// Applies a record to the screen that is showing now and returns the
    /// bytes to answer with, if any.
    pub(crate) fn apply(&self, pending: Pending, screen: &Screen) -> Option<Vec<u8>> {
        let pending = match pending {
            // The read loop decodes before it goes on, so this is a net
            // under it, not a path: the command is not lost if something else
            // drove the parser. Asked here and only for a name: the shell's
            // state is a lock of its own, and this is under the terminal's.
            Pending::Raw {
                command,
                text,
                motion,
            } => {
                let remote = command.medium != Medium::Direct && self.in_remote_session();
                self.decoded(command, text, motion, remote)
            }
            other => other,
        };
        match pending {
            Pending::Store {
                command,
                decoded,
                motion,
            } => lock(&self.images)
                .store(screen, &command, decoded, motion)
                .encode(),
            Pending::Frame { command, decoded } => {
                lock(&self.images).frame(screen, &command, decoded).encode()
            }
            Pending::Compose(command) => lock(&self.images).compose(screen, &command).encode(),
            Pending::Animate { command, now } => lock(&self.images)
                .animate(screen, &command, now)
                .and_then(|reply| reply.encode()),
            Pending::Put { command, motion } => {
                lock(&self.images).put(screen, &command, motion).encode()
            }
            Pending::Delete(command) => {
                lock(&self.images).delete(screen, &command);
                None
            }
            Pending::Reply { reply, .. } => reply.encode(),
            Pending::Raw { .. } => None,
        }
    }

    /// Brings the placements of the screen showing to its grid as it is now.
    /// Called before anything reads or changes them, with the `Term` lock
    /// held.
    pub(crate) fn look<T>(&self, term: &Term<T>, cell: (u16, u16)) {
        lock(&self.images).look(term, cell);
    }

    /// Takes the look of the screen showing as it is now, after something
    /// that knew where the placements went changed them.
    pub(crate) fn settle<T>(&self, term: &Term<T>) {
        lock(&self.images).settle(term);
    }

    /// `CSI 2 J` (`everything`: `CSI 3 J`): the placements on the screen
    /// showing go.
    pub(crate) fn clear_screen(&self, screen: &Screen, everything: bool) {
        lock(&self.images).clear_screen(screen, everything);
    }

    /// The terminal was reset (`RIS`): nothing is kept on either screen.
    pub(crate) fn reset(&self) {
        lock(&self.images).reset();
    }

    /// The main screen's top is grid line `top` now and the history above it
    /// is gone (⌘K and ⌥⌘K). `term` is the main screen's: those cannot run on
    /// the alternate one.
    pub(crate) fn wiped<T>(&self, term: &Term<T>, top: i32, cell: (u16, u16)) {
        let mut images = lock(&self.images);
        images.wiped(top, cell);
        images.settle_main(term);
    }

    /// The grid was resized. `moved`: how many lines up the content went
    /// with the cursor (down when negative); `reflowed`: the width changed or
    /// the main screen is not the one showing, and no line says where a
    /// placement went.
    pub(crate) fn resized<T>(&self, term: &Term<T>, moved: i32, reflowed: bool, cell: (u16, u16)) {
        let mut images = lock(&self.images);
        images.resized(moved, reflowed, cell);
        images.settle_main(term);
    }

    /// The size of a stored image on either screen.
    pub(crate) fn stored_size(&self, echo: Echo) -> Option<(u32, u32)> {
        lock(&self.images).size_of(echo)
    }

    /// Makes the images that are drawn from the palette (`f=8`, `f=16`) in the
    /// colours the terminal has now: what a program set (`OSC 4`) or else the
    /// theme's. A frame does it before it asks for the pictures; nothing is
    /// looked at while there is no such image.
    pub(crate) fn follow_palette<T>(&self, term: &Term<T>, theme: &crate::Theme) {
        let mut images = lock(&self.images);
        if images.holds_indexed() {
            images.repaint(palette::Palette::of(term.colors(), theme));
        }
    }

    /// Fills `out` with the pictures of the placements the frame draws: those
    /// that reach into the grid's `grid` lines and the fill band's `band`.
    pub(crate) fn pictures<T>(
        &self,
        term: &Term<T>,
        cell: (u16, u16),
        grid: Window,
        band: Option<Window>,
        now: Instant,
        out: &mut Pictures,
    ) -> Option<Duration> {
        let mut images = lock(&self.images);
        let alt = term
            .mode()
            .contains(alacritty_terminal::term::TermMode::ALT_SCREEN);
        if !images.shows(alt) {
            return None;
        }
        images.look(term, cell);
        let columns = u16::try_from(term.columns()).unwrap_or(u16::MAX);
        // The cells that show an image are read only when a virtual placement
        // could be what they show.
        let marks = if images.holds_virtual(alt) {
            Marks::read(term, grid, band, columns)
        } else {
            Marks::default()
        };
        let view = View {
            grid,
            band,
            marks: &marks,
        };
        // What the terminal plays comes before the pictures are made, so that a
        // frame that is due is the one drawn. It is the frames on screen that
        // are played, and the time to the next is for the clock.
        let due = if self.playing.load(Ordering::Relaxed) {
            images.advance(alt, cell, columns, &view, now)
        } else {
            None
        };
        images.collect(alt, cell, columns, &view, out);
        due.map(|due| due.saturating_duration_since(now))
    }

    /// The alternate screen was entered or left: what it showed goes.
    pub(crate) fn drop_alt(&self) {
        lock(&self.images).drop_alt();
    }

    /// Records whether a command is half-received ([`Hub::open`]).
    pub(crate) fn set_open(&self, open: bool) {
        self.open.store(open, Ordering::Release);
    }

    /// Whether a command is half-received.
    pub(crate) fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }

    /// Parks a transmission as the parser would: undecoded.
    #[cfg(test)]
    pub(crate) fn park_raw(&self, control: &str, text: &[u8]) {
        let command = Command::parse(control.as_bytes()).expect("a command");
        self.park(Pending::Raw {
            command,
            text: text.to_vec(),
            motion: Motion::NONE,
        });
    }

    /// `hook` runs before each queued image is decoded.
    #[cfg(test)]
    pub(crate) fn on_decode(&self, hook: Box<dyn Fn() + Send>) {
        *lock(&self.decoding) = Some(hook);
    }

    /// Whether a record waits for the parser to be let go.
    #[cfg(test)]
    pub(crate) fn is_parked(&self) -> bool {
        lock(&self.parked).is_some()
    }

    #[cfg(test)]
    pub(crate) fn with_images<R>(&self, read: impl FnOnce(&Images) -> R) -> R {
        read(&lock(&self.images))
    }

    #[cfg(test)]
    pub(crate) fn with_images_mut<R>(&self, change: impl FnOnce(&mut Images) -> R) -> R {
        change(&mut lock(&self.images))
    }
}

/// What the parser's wrapper needs from the session it runs in.
pub(crate) trait Sink: Send + Sync {
    fn hub(&self) -> &Hub;

    /// A cell's size in pixels.
    fn cell(&self) -> (u16, u16);

    /// Writes an answer to the program, the way the terminal's other
    /// answers go.
    fn reply(&self, bytes: Vec<u8>);

    /// The session's DECSET 1016 and 2048 ([`KeptModes`]), which the wrapper
    /// sets and clears as the stream says. They live with the session rather
    /// than in `Term` because alacritty does not know the modes; the pointer
    /// reports read 1016, the resize reads 2048.
    fn kept_modes(&self) -> &KeptModes;

    /// The session's pointer shapes (OSC 22, [`crate::pointer`]), which the
    /// wrapper applies the stream's requests, screen moves and resets to.
    /// Kept with the session for 1016's reason, but no replay carries them:
    /// a stream without a sink drops the requests (the module says why).
    fn pointer(&self) -> &PointerShapes;

    /// The shown pointer shape changed: tells the shell, which reads it
    /// ([`crate::Session::pointer_shape`]). No frame — the pointer is not
    /// drawn by the renderer.
    fn pointer_changed(&self);

    /// A program is about to take the alternate screen: the newest block on the screen it
    /// leaves, with its command's text (`None` when nothing anchored is there). The command
    /// memory ([`crate::history`]) keeps it, because once the screen is taken the rows are out of
    /// reach. A stream with no memory to keep it in lets it go.
    fn leaving_primary_screen(&self, _newest: Option<(BlockKey, String)>) {}

    /// The pane's desk for the view protocol ([`crate::pencere`]), if it has
    /// one and may answer now. A stream without one — a pane opened with no
    /// [`crate::pencere::ViewOptions`], an adopted pane's carried prefix whose
    /// answers are dropped — lets the frames go.
    fn desk(&self) -> Option<&Desk> {
        None
    }

    /// The parser is halted behind a frame the desk parked: hears it, with
    /// the terminal's lock and lease let go ([`Desk::settle`]). The bytes the
    /// protocol answers with go where the terminal's other answers go.
    fn settle_views(&self) {
        if let Some(desk) = self.desk() {
            desk.settle(|bytes| self.reply(bytes));
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
