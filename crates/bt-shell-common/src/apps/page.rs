//! The Apps page of the Settings window, without the window: what each row says, what a sheet asks,
//! and the lists the page writes. The window (`bt-shell-macos`) draws it and sends what the person
//! does; everything it decides is here, where a test can reach it with a temporary folder.
//!
//! **A row's state is a sign and a sentence.** The sign is all the row shows ([`Sign`]): a green dot
//! when all is well, a grey one when there is nothing to do here, a turning ring while the shell
//! looks, a triangle (red when a key changed) for what asks for the person. The sentence is a
//! [`Tip`]: its first line says what is, its second what will happen.
//!
//! **Nothing is written before the person says so.** A source typed in the Add field is only looked
//! at ([`super::look`]); a program whose key nobody approved is written to `[apps] sources` after
//! [`approve`] kept the key, and not before; saying no writes nothing.

use std::io;
use std::path::Path;

use bt_core::app_source_address;

use super::{Failure, Look, State, Status, store};

/// What a row shows of its state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    /// A green dot: the release is in use.
    Good,
    /// A grey dot: there is nothing to run here.
    Quiet,
    /// A turning ring: the source has not been read yet.
    Busy,
    /// An orange triangle: something asks for the person, or could not be done.
    Warn,
    /// A red triangle: the key changed.
    Bad,
}

/// The words behind a sign: what is, and what will happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tip {
    /// What is.
    pub title: String,
    /// What will happen, when there is something to say.
    pub more: Option<String>,
}

impl Tip {
    fn new(title: impl Into<String>, more: &str) -> Self {
        Self {
            title: title.into(),
            more: (!more.is_empty()).then(|| more.to_owned()),
        }
    }

    /// The two lines AppKit's `toolTip` shows.
    pub fn lines(&self) -> String {
        match &self.more {
            Some(more) => format!("{}\n{more}", self.title),
            None => self.title.clone(),
        }
    }

    /// The same words for a screen reader: one sentence after the other.
    pub fn spoken(&self) -> String {
        match &self.more {
            Some(more) => format!("{}. {more}", self.title),
            None => self.title.clone(),
        }
    }
}

/// What a sheet asks about a release: a key to approve, or to trust in place of the one approved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    /// The entry of `[apps] sources`, as the list has it (or will).
    pub source: String,
    /// The program's name.
    pub name: String,
    /// The release.
    pub version: String,
    /// The key the release is signed with (`ed25519:` and base64).
    pub key: String,
    /// The key that was approved before, when the release is signed with another one.
    pub was: Option<String>,
}

impl Review {
    /// What a [`Look`] at `source` asks, when it is not trusted already.
    pub fn from_look(source: &str, look: &Look) -> Self {
        Self {
            source: source.to_owned(),
            name: look.name.clone(),
            version: look.version.to_string(),
            key: look.key.clone(),
            was: look
                .approved
                .clone()
                .filter(|approved| *approved != look.key),
        }
    }
}

/// One row of the page: a source of the list and how it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The entry of `[apps] sources`, as written: what the row's second line says and what a
    /// removal takes out of the list.
    pub source: String,
    /// The program's name; before its `app.json` has been read, what the source says it is.
    pub name: String,
    /// The release the row speaks of, when there is one.
    pub version: Option<String>,
    /// The row's sign.
    pub sign: Sign,
    /// The sign's words.
    pub tip: Tip,
    /// What the Review button opens, when the row asks for a decision.
    pub review: Option<Review>,
}

/// The rows of the page: one for each entry of `sources`, in the list's order, each with the
/// status the last check gave it (`root` is the apps folder, which says what key was approved).
/// An entry no check has seen yet is a row that is still looking. `statuses` is `None` when no
/// check could be started at all: no row would ever stop looking, so each says it was not checked.
pub fn rows(sources: &[String], statuses: Option<&[Status]>, root: &Path) -> Vec<Row> {
    sources
        .iter()
        .map(|source| {
            let unread = |sign, tip| Row {
                source: source.clone(),
                name: name_of(source),
                version: None,
                sign,
                tip,
                review: None,
            };
            let Some(statuses) = statuses else {
                return unread(
                    Sign::Warn,
                    Tip::new(
                        "Not checked",
                        "bateri could not start checking. It tries again at the next launch.",
                    ),
                );
            };
            match statuses.iter().find(|status| status.source == *source) {
                Some(status) => row(status, root),
                None => unread(Sign::Busy, Tip::new("Checking", "bateri is looking at it.")),
            }
        })
        .collect()
}

/// When a source that failed is tried again, in the words of a row's second line: soon for what
/// the clock may mend ([`Failure::mendable`], the worker's short wait), else at the next day's check.
fn again(failure: &Failure) -> &'static str {
    if failure.mendable() {
        "bateri tries again soon."
    } else {
        "bateri tries again tomorrow."
    }
}

fn row(status: &Status, root: &Path) -> Row {
    let name = status
        .name
        .clone()
        .unwrap_or_else(|| name_of(&status.source));
    let (version, sign, tip, review) = match &status.state {
        State::Ready { version, newer } => {
            let (sign, tip) = match (&status.problem, newer) {
                (Some(problem), _) => (
                    Sign::Warn,
                    Tip::new(plain(&problem.to_string()), again(problem)),
                ),
                (None, Some(newer)) => (
                    Sign::Warn,
                    Tip::new(
                        format!("{newer} needs a newer bateri"),
                        &format!("{version} stays until bateri is updated."),
                    ),
                ),
                (None, None) => (Sign::Good, Tip::new("Up to date", "")),
            };
            (Some(version.to_string()), sign, tip, None)
        }
        State::Awaiting {
            version,
            key,
            changed,
        } => {
            let review = Review {
                source: status.source.clone(),
                name: name.clone(),
                version: version.to_string(),
                key: key.clone(),
                was: changed
                    .then(|| store::approved_key(root, &name))
                    .flatten()
                    .filter(|was| was != key),
            };
            let (sign, tip) = if *changed {
                (
                    Sign::Bad,
                    Tip::new(
                        "Signed with another key",
                        "Updates stopped. Review to decide.",
                    ),
                )
            } else {
                (
                    Sign::Warn,
                    Tip::new("Waiting for approval", "Review shows who signed it."),
                )
            };
            (Some(version.to_string()), sign, tip, Some(review))
        }
        State::WaitsForUpdate { version } => (
            None,
            Sign::Warn,
            Tip::new(
                format!("{version} needs a newer bateri"),
                "It comes when bateri is updated.",
            ),
            None,
        ),
        State::NotHere { version } => (
            Some(version.to_string()),
            Sign::Quiet,
            Tip::new(
                "No file for this computer",
                "It runs on the servers it has a file for.",
            ),
            None,
        ),
        State::Error(failure) => (
            None,
            Sign::Warn,
            Tip::new(plain(&failure.to_string()), again(failure)),
            None,
        ),
    };
    Row {
        source: status.source.clone(),
        name,
        version,
        sign,
        tip,
        review,
    }
}

/// What a source is called before its `app.json` says: the project of `user/repo`; for an
/// address the folder the file is in, or the host.
pub fn name_of(source: &str) -> String {
    let source = source.trim();
    let Some((_, rest)) = source.split_once("://") else {
        return source.rsplit('/').next().unwrap_or(source).to_owned();
    };
    let mut parts = rest.split('/').filter(|part| !part.is_empty());
    let host = parts.next().unwrap_or(rest);
    let folders: Vec<&str> = parts.filter(|part| *part != "app.json").collect();
    folders.last().map_or(host, |last| *last).to_owned()
}

/// A curl line without its `curl: (22) ` lead: the person reads the rest.
fn plain(line: &str) -> String {
    let rest = line.strip_prefix("curl: (").and_then(|rest| {
        let (code, after) = rest.split_once(") ")?;
        code.chars().all(|c| c.is_ascii_digit()).then_some(after)
    });
    rest.unwrap_or(line).to_owned()
}

/// Why a source typed into the Add field cannot be followed, in the page's words (the line under
/// the field).
pub fn refusal(source: &str, failure: &Failure) -> String {
    match failure {
        Failure::Source => "That is not a GitHub project (user/repo) or an address.".to_owned(),
        // An address that answers 404 has no release; any other word of curl's is its own.
        Failure::Fetch(why) if why.trim_end().ends_with("404") => {
            format!("No release of {source} has an app.json.")
        }
        other => {
            let mut text = plain(&other.to_string());
            text.push('.');
            text
        }
    }
}

/// The list with `source` added at its end, or why it cannot be: it names no `app.json`
/// ([`Failure::Source`]), or it is in the list already (two spellings of one address count).
pub fn added(list: &[String], source: &str) -> Result<Vec<String>, AddRefused> {
    let source = source.trim();
    let address = app_source_address(source).ok_or(AddRefused::NotASource)?;
    if list
        .iter()
        .any(|listed| app_source_address(listed).as_deref() == Some(address.as_str()))
    {
        return Err(AddRefused::Listed);
    }
    let mut list = list.to_vec();
    list.push(source.to_owned());
    Ok(list)
}

/// Whether `name` is held by a source of the list other than `source`: a program that landed, one
/// that waits for a yes, or one that was refused for the name itself. The list's own order decides
/// who holds a name ([`super::check`]), so a second source of the name would never be followed.
pub fn name_claimed(statuses: &[Status], name: &str, source: &str) -> bool {
    let address = app_source_address(source);
    statuses.iter().any(|status| {
        status.name.as_deref() == Some(name) && app_source_address(&status.source) != address
    })
}

/// Why [`added`] said no.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddRefused {
    /// Neither `user/repo` nor an address.
    NotASource,
    /// The list has it.
    Listed,
}

impl AddRefused {
    /// The line under the Add field.
    pub fn text(self) -> &'static str {
        match self {
            Self::NotASource => "That is not a GitHub project (user/repo) or an address.",
            Self::Listed => "It is in the list already.",
        }
    }
}

/// The list without `source`.
pub fn without(list: &[String], source: &str) -> Vec<String> {
    list.iter()
        .filter(|listed| listed.as_str() != source)
        .cloned()
        .collect()
}

/// The person said yes to a release: its key is kept as the one approved for the name. The next
/// check lands the release; for a source that is not in the list yet the caller writes the list.
pub fn approve(root: &Path, review: &Review) -> io::Result<()> {
    store::approve(root, &review.name, &review.key)
}

#[cfg(test)]
mod tests {
    use bt_core::pencere::apps::Version;

    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).expect("a version")
    }

    fn status(source: &str, name: Option<&str>, state: State) -> Status {
        Status {
            source: source.to_owned(),
            name: name.map(str::to_owned),
            state,
            problem: None,
        }
    }

    fn one(status: Status) -> Row {
        let list = vec![status.source.clone()];
        rows(&list, Some(&[status]), Path::new("/nonexistent")).remove(0)
    }

    #[test]
    fn a_row_that_is_well_is_a_green_dot_and_says_so_in_one_line() {
        let row = one(status(
            "acme/notes",
            Some("notes"),
            State::Ready {
                version: v("0.3.1"),
                newer: None,
            },
        ));
        assert_eq!(row.name, "notes");
        assert_eq!(row.version.as_deref(), Some("0.3.1"));
        assert_eq!(row.sign, Sign::Good);
        assert_eq!(row.tip.lines(), "Up to date");
        assert_eq!(row.tip.spoken(), "Up to date");
        assert!(row.review.is_none());
    }

    #[test]
    fn a_row_says_what_is_and_then_what_will_happen() {
        let newer = one(status(
            "mina/sketch",
            Some("sketch"),
            State::Ready {
                version: v("2.1.0"),
                newer: Some(v("3.0.0")),
            },
        ));
        assert_eq!(newer.sign, Sign::Warn);
        assert_eq!(
            newer.tip.lines(),
            "3.0.0 needs a newer bateri\n2.1.0 stays until bateri is updated."
        );
        assert_eq!(
            newer.tip.spoken(),
            "3.0.0 needs a newer bateri. 2.1.0 stays until bateri is updated."
        );

        let waits = one(status(
            "a/b",
            Some("b"),
            State::WaitsForUpdate {
                version: v("1.0.0"),
            },
        ));
        assert_eq!(waits.version, None);
        assert_eq!(
            waits.tip.lines(),
            "1.0.0 needs a newer bateri\nIt comes when bateri is updated."
        );

        let nothere = one(status(
            "ozge/mono",
            Some("mono"),
            State::NotHere {
                version: v("1.0.0"),
            },
        ));
        assert_eq!(nothere.sign, Sign::Quiet);
        assert_eq!(
            nothere.tip.lines(),
            "No file for this computer\nIt runs on the servers it has a file for."
        );
    }

    #[test]
    fn a_name_held_by_another_source_of_the_list_is_claimed_waiting_or_not() {
        let waiting = status(
            "acme/notes",
            Some("notes"),
            State::Awaiting {
                version: v("1.0.0"),
                key: "ed25519:x".to_owned(),
                changed: false,
            },
        );
        let nameless = status("acme/broken", None, State::Error(Failure::Source));
        let list = [waiting, nameless];
        // Another source with the name: claimed, though nothing has landed.
        assert!(name_claimed(&list, "notes", "mina/notes"));
        // The source that holds it is not in its own way, spelled either way.
        assert!(!name_claimed(&list, "notes", "acme/notes"));
        assert!(!name_claimed(
            &list,
            "notes",
            "https://github.com/acme/notes/releases/latest/download/app.json"
        ));
        // A name nobody holds.
        assert!(!name_claimed(&list, "sketch", "mina/sketch"));
    }

    #[test]
    fn what_asks_for_a_decision_has_review_and_a_changed_key_is_red() {
        let waiting = one(status(
            "https://lab.example.org/app.json",
            Some("lab"),
            State::Awaiting {
                version: v("0.9.0"),
                key: "ed25519:K".to_owned(),
                changed: false,
            },
        ));
        assert_eq!(waiting.sign, Sign::Warn);
        assert_eq!(
            waiting.tip.lines(),
            "Waiting for approval\nReview shows who signed it."
        );
        let review = waiting.review.expect("review");
        assert_eq!(review.name, "lab");
        assert_eq!(review.version, "0.9.0");
        assert_eq!(review.key, "ed25519:K");
        assert_eq!(review.was, None);

        let changed = one(status(
            "kerem/draw",
            Some("draw"),
            State::Awaiting {
                version: v("1.4.2"),
                key: "ed25519:N".to_owned(),
                changed: true,
            },
        ));
        assert_eq!(changed.sign, Sign::Bad);
        assert_eq!(
            changed.tip.lines(),
            "Signed with another key\nUpdates stopped. Review to decide."
        );
        assert!(changed.review.is_some());
    }

    #[test]
    fn a_failure_is_said_in_curls_words_without_the_curl_and_tried_again_soon() {
        let row = one(status(
            "dev/flaky",
            None,
            State::Error(Failure::Fetch(
                "curl: (6) Could not resolve host: github.com".to_owned(),
            )),
        ));
        // The name is the project's until the `app.json` says another.
        assert_eq!(row.name, "flaky");
        assert_eq!(row.sign, Sign::Warn);
        assert_eq!(
            row.tip.lines(),
            "Could not resolve host: github.com\nbateri tries again soon."
        );
        // Something usable that could not be refreshed stays shown, with the reason.
        let mut kept = status(
            "acme/notes",
            Some("notes"),
            State::Ready {
                version: v("0.3.1"),
                newer: None,
            },
        );
        kept.problem = Some(Failure::Digest);
        let row = one(kept);
        assert_eq!(row.sign, Sign::Warn);
        assert_eq!(row.version.as_deref(), Some("0.3.1"));
        assert!(row.tip.title.contains("does not match"));
        assert_eq!(row.tip.more.as_deref(), Some("bateri tries again soon."));
        // What the clock does not mend waits for the next day's check.
        let row = one(status("dev/taken", None, State::Error(Failure::NameTaken)));
        assert_eq!(
            row.tip.lines(),
            "another program already has this name\nbateri tries again tomorrow."
        );
    }

    #[test]
    fn a_source_nobody_has_checked_yet_is_a_ring() {
        let list = vec!["acme/fresh".to_owned()];
        let rows = rows(&list, Some(&[]), Path::new("/nonexistent"));
        assert_eq!(rows[0].sign, Sign::Busy);
        assert_eq!(rows[0].name, "fresh");
        assert_eq!(rows[0].tip.title, "Checking");
        assert!(rows[0].review.is_none());
    }

    #[test]
    fn a_list_no_check_could_start_for_is_not_checked_rather_than_looking_for_ever() {
        let list = vec!["acme/fresh".to_owned()];
        let rows = rows(&list, None, Path::new("/nonexistent"));
        assert_eq!(rows[0].sign, Sign::Warn);
        assert_eq!(rows[0].name, "fresh");
        assert_eq!(
            rows[0].tip.lines(),
            "Not checked\nbateri could not start checking. It tries again at the next launch."
        );
        assert!(rows[0].review.is_none());
    }

    #[test]
    fn a_source_is_named_by_its_project_or_its_folder_or_its_host() {
        assert_eq!(name_of("bateri/textpen"), "textpen");
        assert_eq!(
            name_of("https://downloads.example.org/releases/lab/app.json"),
            "lab"
        );
        assert_eq!(
            name_of("https://lab.example.org/app.json"),
            "lab.example.org"
        );
        assert_eq!(name_of("file:///tmp/tool/app.json"), "tool");
    }

    #[test]
    fn a_refusal_is_in_the_pages_words() {
        assert_eq!(
            refusal("nothing", &Failure::Source),
            "That is not a GitHub project (user/repo) or an address."
        );
        assert_eq!(
            refusal(
                "acme/missing",
                &Failure::Fetch("curl: (22) The requested URL returned error: 404".to_owned())
            ),
            "No release of acme/missing has an app.json."
        );
        assert_eq!(
            refusal(
                "acme/x",
                &Failure::Fetch("curl: (6) Could not resolve host: github.com".to_owned())
            ),
            "Could not resolve host: github.com."
        );
        assert_eq!(
            refusal("pencere/x", &Failure::Reserved),
            "bateri, kablo and pencere are not names a program may have."
        );
    }

    #[test]
    fn the_list_is_added_to_and_taken_from_without_touching_the_rest() {
        let list = vec!["a/one".to_owned(), "b/two".to_owned()];
        assert_eq!(without(&list, "a/one"), ["b/two"]);
        assert_eq!(without(&list, "c/none"), ["a/one", "b/two"]);
        assert_eq!(
            added(&list, " c/three ").expect("new"),
            ["a/one", "b/two", "c/three"]
        );
        assert_eq!(added(&list, "a/one"), Err(AddRefused::Listed));
        // Two spellings of one address are one entry.
        let spelled = vec!["acme/notes".to_owned()];
        assert_eq!(
            added(
                &spelled,
                "https://github.com/acme/notes/releases/latest/download/app.json"
            ),
            Err(AddRefused::Listed)
        );
        assert_eq!(added(&list, ""), Err(AddRefused::NotASource));
        assert_eq!(AddRefused::Listed.text(), "It is in the list already.");
    }
}
