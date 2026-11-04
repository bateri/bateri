//! The names a window, a group and a tab are known by from outside: `w1`, `g1`, `t3`.
//!
//! A number is the app's to give and each kind counts on its own, across every window. It does not
//! depend on where the thing stands: moving a tab, putting it in a group or taking it to another
//! window leaves it as it was. A number that was given is never given again, whatever became of
//! the thing, and a restart keeps it: the counters live in a file of the session directory and a
//! number is written **before** it is used (`next.sh`'s rule for a pane's number: a crash between
//! the two loses a number, which is allowed, and never repeats one). The layout is saved a moment
//! after it changes, so the counters could not live in it — a crash would give a number twice.
//!
//! The counters never fall below what a session that was read holds ([`Counters::observe`]): a
//! counter file that is gone, or older than the layout, must not hand out a number a restored tab
//! already has.
//!
//! The file is best effort. When it cannot be written the thing is still made; its number only
//! lives in memory for that run.

use std::fmt;
use std::path::{Path, PathBuf};

/// The file inside the session directory.
pub(crate) const FILE: &str = "ids";

/// What a number names. The prefix says which kind a name is, so a command that means a tab and
/// is given a group's name can say so instead of closing the wrong thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Window,
    Group,
    Tab,
}

impl Kind {
    /// All kinds, in the order the file lists them.
    pub const ALL: [Kind; 3] = [Kind::Window, Kind::Group, Kind::Tab];

    /// The letter a name starts with.
    pub const fn prefix(self) -> char {
        match self {
            Kind::Window => 'w',
            Kind::Group => 'g',
            Kind::Tab => 't',
        }
    }

    /// The word for it in a message.
    pub const fn word(self) -> &'static str {
        match self {
            Kind::Window => "window",
            Kind::Group => "group",
            Kind::Tab => "tab",
        }
    }

    fn of_prefix(prefix: char) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.prefix() == prefix)
    }

    const fn slot(self) -> usize {
        match self {
            Kind::Window => 0,
            Kind::Group => 1,
            Kind::Tab => 2,
        }
    }
}

/// A window's, a group's or a tab's name: its kind's letter and its number, from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Id {
    kind: Kind,
    number: u64,
}

impl Id {
    /// The name of number `number` of `kind`; there is no number 0.
    pub fn new(kind: Kind, number: u64) -> Option<Id> {
        (number > 0).then_some(Id { kind, number })
    }

    /// The thing a name stands for: a letter and the number written plainly (`t3`, never `t03`,
    /// `T3` or `t+3`), and not `t0`.
    pub fn parse(name: &str) -> Option<Id> {
        let mut chars = name.chars();
        let kind = Kind::of_prefix(chars.next()?)?;
        let digits = chars.as_str();
        let number: u64 = digits.parse().ok()?;
        // `u64::from_str` takes a `+` and leading zeros; the one spelling is the plain one.
        if number.to_string() != digits {
            return None;
        }
        Id::new(kind, number)
    }

    pub fn kind(self) -> Kind {
        self.kind
    }

    pub fn number(self) -> u64 {
        self.number
    }
}

impl fmt::Display for Id {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "{}{}", self.kind.prefix(), self.number)
    }
}

/// The last number given of each kind, and where it is kept.
#[derive(Clone, Debug, Default)]
pub struct Counters {
    given: [u64; 3],
    file: Option<PathBuf>,
}

impl Counters {
    /// Counters that keep nothing on disk (a run with no session directory, a test).
    pub fn in_memory() -> Counters {
        Counters::default()
    }

    /// The counters kept in `dir`, as that file left them; a file that is missing or that no one
    /// can read counts from the start (the session read afterwards raises them,
    /// [`Counters::observe`]).
    pub fn open(dir: &Path) -> Counters {
        let file = dir.join(FILE);
        let given = std::fs::read_to_string(&file)
            .map(|text| parse(&text))
            .unwrap_or_default();
        Counters {
            given,
            file: Some(file),
        }
    }

    /// Notes that `number` of `kind` is in use (a window, a group or a tab that was read from a
    /// saved session), so the next one given is above it.
    pub fn observe(&mut self, kind: Kind, number: u64) {
        let slot = &mut self.given[kind.slot()];
        *slot = (*slot).max(number);
    }

    /// The last number given of `kind`.
    pub fn last(&self, kind: Kind) -> u64 {
        self.given[kind.slot()]
    }

    /// The next name of `kind`. The number is on disk before this returns.
    pub fn next(&mut self, kind: Kind) -> Id {
        let slot = &mut self.given[kind.slot()];
        // A counter at the top of its range stays there and answers with the last number; the
        // range is not one a person gets near.
        *slot = slot.saturating_add(1);
        let number = *slot;
        self.write();
        Id { kind, number }
    }

    fn write(&self) {
        let Some(file) = &self.file else {
            return;
        };
        let text = Kind::ALL
            .iter()
            .map(|kind| format!("{} {}\n", kind.prefix(), self.given[kind.slot()]))
            .collect::<String>();
        // Through a temporary name and a rename, as the layout is: the file is never half
        // written, and a number is on disk when this returns.
        let _ = crate::restore::write_atomic(file, text.as_bytes());
    }
}

/// The counters a file says; a line that is not `letter number` is left out.
fn parse(text: &str) -> [u64; 3] {
    let mut given = [0; 3];
    for line in text.lines() {
        let Some((letter, number)) = line.split_once(' ') else {
            continue;
        };
        let mut letters = letter.chars();
        let (Some(letter), None) = (letters.next(), letters.next()) else {
            continue;
        };
        let (Some(kind), Ok(number)) = (Kind::of_prefix(letter), number.parse::<u64>()) else {
            continue;
        };
        given[kind.slot()] = number;
    }
    given
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::TempRoot;

    #[test]
    fn a_name_is_a_letter_and_the_number_written_plainly() {
        let tab = Id::new(Kind::Tab, 3).expect("an id");
        assert_eq!(tab.to_string(), "t3");
        assert_eq!(Id::parse("t3"), Some(tab));
        assert_eq!(Id::parse("w12").map(|id| id.kind()), Some(Kind::Window));
        assert_eq!(Id::parse("g1").map(|id| id.number()), Some(1));
        for name in [
            "",
            "t",
            "3",
            "T3",
            "t03",
            "t+3",
            "t-3",
            "t0",
            "t3 ",
            " t3",
            "tt3",
            "t3a",
            "p3",
            "t3.1",
            "t99999999999999999999999",
        ] {
            assert_eq!(Id::parse(name), None, "{name:?}");
        }
        assert_eq!(Id::new(Kind::Group, 0), None);
    }

    #[test]
    fn each_kind_counts_on_its_own_from_one() {
        let mut counters = Counters::in_memory();
        let names: Vec<String> = [
            Kind::Tab,
            Kind::Tab,
            Kind::Window,
            Kind::Group,
            Kind::Tab,
            Kind::Window,
        ]
        .into_iter()
        .map(|kind| counters.next(kind).to_string())
        .collect();
        assert_eq!(names, ["t1", "t2", "w1", "g1", "t3", "w2"]);
    }

    #[test]
    fn a_number_is_on_disk_before_it_is_given_and_a_restart_goes_on_from_it() {
        let root = TempRoot::new("ids-restart");
        let mut counters = Counters::open(&root.0);
        assert_eq!(counters.next(Kind::Tab).to_string(), "t1");
        assert_eq!(counters.next(Kind::Tab).to_string(), "t2");
        assert_eq!(counters.next(Kind::Group).to_string(), "g1");
        // Read by someone else at that moment: the file already holds what was given.
        let text = std::fs::read_to_string(root.0.join(FILE)).expect("the counters");
        assert_eq!(text, "w 0\ng 1\nt 2\n");
        // A crash after the last number was given: the restart does not give it again.
        let mut again = Counters::open(&root.0);
        assert_eq!(again.next(Kind::Tab).to_string(), "t3");
        assert_eq!(again.next(Kind::Group).to_string(), "g2");
        assert_eq!(again.next(Kind::Window).to_string(), "w1");
    }

    #[test]
    fn counters_never_fall_below_what_a_session_that_was_read_holds() {
        let root = TempRoot::new("ids-observed");
        // The file is gone: the session says what is in use.
        let mut counters = Counters::open(&root.0);
        counters.observe(Kind::Tab, 9);
        counters.observe(Kind::Tab, 4);
        counters.observe(Kind::Window, 2);
        assert_eq!(counters.next(Kind::Tab).to_string(), "t10");
        assert_eq!(counters.next(Kind::Window).to_string(), "w3");
        assert_eq!(counters.next(Kind::Group).to_string(), "g1");
        // The file is older than the layout: the layout wins, and the file follows.
        std::fs::write(root.0.join(FILE), "w 1\ng 1\nt 2\n").expect("a counter file");
        let mut counters = Counters::open(&root.0);
        counters.observe(Kind::Tab, 7);
        assert_eq!(counters.next(Kind::Tab).to_string(), "t8");
        assert_eq!(counters.last(Kind::Window), 1);
    }

    #[test]
    fn a_file_nobody_can_read_or_write_counts_in_memory() {
        let root = TempRoot::new("ids-broken");
        std::fs::write(
            root.0.join(FILE),
            "not a counter\nt\nt x\nx 4\nt 5 6\ng 2\n",
        )
        .expect("a file");
        let mut counters = Counters::open(&root.0);
        assert_eq!(counters.next(Kind::Group).to_string(), "g3");
        assert_eq!(counters.next(Kind::Tab).to_string(), "t1");
        // The directory is gone: the tab is still made, with a number that lives in memory.
        let gone = Counters::open(&root.0.join("nothing/here"));
        let mut gone = gone;
        assert_eq!(gone.next(Kind::Tab).to_string(), "t1");
        assert_eq!(gone.next(Kind::Tab).to_string(), "t2");
    }
}
