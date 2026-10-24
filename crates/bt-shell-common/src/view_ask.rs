//! What a program's view asks of the person, and what the application remembers of the
//! answer.
//!
//! A view is a web page the program wrote, running on this Mac. It asks two things the
//! person decides, and both come from the program and not from the application:
//!
//! * **Access** ([`consent`]): the addresses it declared (the `access` of its `open`) and the
//!   browser features it wants (camera, microphone, location, the clipboard). The view starts
//!   with only what the person allowed.
//! * **A link** ([`link`]): the page's `link` message, which the page may send and the
//!   person's browser, on this Mac, opens.
//!
//! The words of both questions are made here, from the facts, so that they are tested and
//! no toolkit writes its own: **where the program runs** (the server's name, or this Mac)
//! is part of every question, because a view that a program on a server opened still runs
//! here — `localhost` in its list is this Mac's, not the server's, and the question says so
//! ([`Locality`]).
//!
//! Every name in a question is **cleaned and bounded** the way the strip's are
//! ([`view_strip`]): the program's name, the view's title, the server's and every address come
//! from the program or from the machine it runs on, and a question must not be made to read
//! as the application's own words by a line break, a direction override or a quotation mark.
//! A link is checked harder, because what the person sees must be what opens
//! ([`web_target`]).
//!
//! **What is remembered** ([`Memory`]): an answer to the first question, by *where* the
//! program runs (this Mac or the server), *which* program and *which* set — the addresses
//! and features, in any order — and until the application quits; it is not written anywhere.
//! Another program, another server or another set asks again: what a person allows one
//! program is not allowed to another (§12.2 of the protocol). Both answers are remembered: a
//! program that opens its view often does not ask each time, and a person who said no is not
//! asked again either. An answer for a program that cannot be told from another is not
//! remembered.
//!
//! **Two panes that ask the same at the same time are asked once** ([`Waiting`]): a session
//! restored with several panes of one program puts the same question up in each, and the
//! answer to the first is the answer to the others, which go with it. Memory alone does not
//! do this: it is read when a question is about to be put up, and the second was up by then.
//! Which program is asking is the application's own reading of the pane ([`Origin::identity`]);
//! the words the program gives for itself and for its view ([`Consent::program`],
//! [`Consent::title`]) are only what the question quotes, since a program can say anything it
//! likes about itself.
//!
//! Platform-free: the toolkit shows a [`Question`] and says what the person pressed.

use bt_core::pencere::{Consent, Declared, Directive, Locality, Permission};

use crate::view_strip::{NAME_LIMIT, TITLE_LIMIT, clean};

/// What the question calls the person's own computer.
pub const THIS_MAC: &str = "this Mac";

/// The most addresses listed under one heading before the rest is counted.
const LISTED: usize = 8;

/// The longest address shown, in characters: more than a valid origin has.
const ADDRESS_LIMIT: usize = 260;

/// The longest link the person is asked about, in bytes; a longer one is refused.
pub const LINK_LIMIT: usize = 2048;

/// How much of a link is shown; the rest is counted.
const LINK_SHOWN: usize = 300;

/// The most answers remembered. The oldest goes first.
pub const MEMORY_LIMIT: usize = 256;

/// Who is asking, as far as anything here knows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Origin {
    /// The server the program runs on (its name as the pane knows it); `None` on this Mac.
    pub place: Option<String>,
    /// The program's name as the application reads it from the pane (the process in front,
    /// the server's word for a terminal it keeps); `None` when nothing here knows it (a
    /// plain `ssh`, a server that has not said). The words a question quotes, no more: an
    /// interpreter's name is every script's, so an answer is not kept by it ([`Self::identity`]).
    pub program: Option<String>,
    /// What tells this program from another that is run the same way, which the program cannot
    /// pick for itself: the file it was executed from, and for `node` and `python` the script
    /// they run (`program::identity::kept_name`). **This is what an answer is kept by**; `None`
    /// when nothing tells it from the next program the same interpreter runs (a REPL, a plain
    /// `ssh`, a terminal a server keeps) and an answer is then not kept: the question is
    /// asked again.
    pub identity: Option<String>,
    /// Where it was run from, as full paths from the process table: the file it executed and,
    /// for an interpreter, the script it runs (`program::identity::kept_path`). **The name
    /// alone is not who a program is** — two programs of one name run from two places are two
    /// — so an answer is kept by [`Self::identity`] and this together, and a program whose
    /// place is not known has no answer kept.
    pub path: Option<String>,
    /// What the program calls itself (`program` of its `open`), when it said: the words
    /// a question quotes in the program's place. The program wrote it and it is the one
    /// thing here that says nothing about which program it is, so no answer is kept by it.
    pub said: Option<String>,
}

/// A question for a toolkit's alert: a title, a text and the two buttons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    /// The alert's title.
    pub title: String,
    /// The alert's text.
    pub text: String,
    /// The button that refuses.
    pub no: &'static str,
    /// The button that agrees.
    pub yes: &'static str,
}

/// A name from the program or the machine, as a question may quote it: cleaned and bounded as
/// the strip's are, and without the quotation marks that would end the quote around it.
fn quotable(text: &str, limit: usize) -> Option<String> {
    let text: String = clean(text, limit)
        .chars()
        .filter(|c| !matches!(c, '"' | '“' | '”'))
        .collect();
    let text = text.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// Who is asking, as a sentence's subject: `“docker-ui” on this Mac`, `A program on the
/// server “prod”`. A server is always named as one, so a server called "this Mac" does not
/// read as this Mac. The program is named by what it says it is, else by what the
/// application reads in the pane ([`Origin::said`], [`Origin::program`]); `said` is the
/// program's own word for it, which the question quotes like any other name.
fn subject(origin: &Origin, said: Option<&str>) -> String {
    let place = match origin
        .place
        .as_deref()
        .and_then(|p| quotable(p, NAME_LIMIT))
    {
        Some(server) => format!("the server “{server}”"),
        None => THIS_MAC.to_owned(),
    };
    match said.and_then(|p| quotable(p, NAME_LIMIT)).or_else(|| {
        origin
            .program
            .as_deref()
            .and_then(|p| quotable(p, NAME_LIMIT))
    }) {
        Some(program) => format!("“{program}” on {place}"),
        None => format!("A program on {place}"),
    }
}

/// `a`, `a and b`, `a, b and c`.
fn joined(parts: &[&str]) -> String {
    match parts {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// The heading of a list of addresses: what the view does with them.
const fn heading(directive: Directive) -> &'static str {
    match directive {
        Directive::Connect => "Connect to",
        Directive::Resource => "Load scripts, styles, images and fonts from",
        Directive::Frame => "Embed pages from",
        Directive::BaseUri => "Take its base address from",
    }
}

/// What a browser feature is, as a line of the list, and as part of the title.
const fn feature(permission: Permission) -> (&'static str, &'static str) {
    match permission {
        Permission::Camera => ("the camera", "use the camera"),
        Permission::Microphone => ("the microphone", "use the microphone"),
        Permission::Geolocation => ("your location", "see your location"),
        Permission::ClipboardWrite => ("the clipboard, to write to it", "write to the clipboard"),
    }
}

const DIRECTIVES: [Directive; 4] = [
    Directive::Connect,
    Directive::Resource,
    Directive::Frame,
    Directive::BaseUri,
];

/// The question for the access a view declared: which address and which features, and where
/// the program that opened it runs. `app` is the application's name, which says how long the
/// answer is kept.
pub fn consent(origin: &Origin, consent: &Consent, app: &str) -> Question {
    let mut wants: Vec<&str> = Vec::new();
    if !consent.domains.is_empty() {
        wants.push("reach other sites");
    }
    wants.extend(
        consent
            .permissions
            .iter()
            .map(|permission| feature(*permission).1),
    );
    let title = if wants.is_empty() {
        "Allow this view more access?".to_owned()
    } else {
        format!("Allow this view to {}?", joined(&wants))
    };

    // The view as the person would call it: the title the program gave it, if it gave one.
    let opened = match consent
        .title
        .as_deref()
        .and_then(|t| quotable(t, TITLE_LIMIT))
    {
        Some(title) => format!("opened the view “{title}”"),
        None => "opened a view".to_owned(),
    };
    let mut text = format!(
        "{} {opened}, and the view asks to:",
        subject(origin, consent.program.as_deref())
    );

    let mut outside = false;
    for directive in DIRECTIVES {
        let declared: Vec<&Declared> = consent
            .domains
            .iter()
            .filter(|declared| declared.directive == directive)
            .collect();
        if declared.is_empty() {
            continue;
        }
        text.push_str(&format!("\n\n{}", heading(directive)));
        for declared in declared.iter().take(LISTED) {
            let address = clean(declared.domain.as_str(), ADDRESS_LIMIT);
            let note = match declared.domain.locality() {
                Locality::Public => "",
                Locality::Loopback => {
                    outside = true;
                    " (this Mac)"
                }
                Locality::Private => {
                    outside = true;
                    " (a private network)"
                }
            };
            text.push_str(&format!("\n• {address}{note}"));
        }
        if declared.len() > LISTED {
            // What is not listed is still counted for the locality, so the warning
            // below does not depend on where in the list an address stands.
            outside |= declared.iter().skip(LISTED).any(|declared| {
                matches!(
                    declared.domain.locality(),
                    Locality::Loopback | Locality::Private
                )
            });
            text.push_str(&format!("\n• and {} more", declared.len() - LISTED));
        }
    }
    if !consent.permissions.is_empty() {
        text.push_str("\n\nUse");
        for permission in &consent.permissions {
            text.push_str(&format!("\n• {}", feature(*permission).0));
        }
    }

    if outside {
        let place = origin
            .place
            .as_deref()
            .and_then(|p| quotable(p, NAME_LIMIT));
        match place {
            Some(server) => text.push_str(&format!(
                "\n\nThe view runs on this Mac, so a place marked above is on this Mac or a \
network it is on, not on the server “{server}”."
            )),
            None => text.push_str(
                "\n\nA place marked above is on this Mac or a network it is on, and the view \
could reach what runs there.",
            ),
        }
    }
    text.push_str(&format!(
        "\n\n{} keeps your answer until it quits.",
        quotable(app, NAME_LIMIT).unwrap_or_else(|| "The application".to_owned())
    ));
    Question {
        title,
        text,
        no: "Don't Allow",
        yes: "Allow",
    }
}

/// The question about a link a view asks to open, and the link to open when the person agrees;
/// `None` for one that is not asked about ([`web_target`]).
pub fn link(origin: &Origin, url: &str) -> Option<Link> {
    let target = web_target(url)?;
    let shown = if target.len() > LINK_SHOWN {
        format!(
            "{}…\n(and {} more characters)",
            &target[..LINK_SHOWN],
            target.len() - LINK_SHOWN
        )
    } else {
        target.to_owned()
    };
    Some(Link {
        question: Question {
            title: "Open this link?".to_owned(),
            text: format!(
                "{} opened a view that asks to open this link in your browser on this Mac:\n\n{shown}",
                subject(origin, origin.said.as_deref())
            ),
            no: "Cancel",
            yes: "Open",
        },
        target: target.to_owned(),
    })
}

/// A link a view asked to open, asked about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// What the person is asked.
    pub question: Question,
    /// The link, as it is opened if the person agrees: the text the question shows.
    pub target: String,
}

/// The link, when it is one a person may be asked to open: `http` or `https`, no longer than
/// [`LINK_LIMIT`], and plain. "Plain" is the part that keeps what is shown the same as what
/// opens: only visible ASCII (a page names a non-ASCII address in its escaped form, and one
/// that does not could look like another), no backslash (browsers read it as a slash) and no
/// credentials before the host (`https://shop.example@elsewhere.test/` reads as the first and
/// goes to the second). A link that is not plain is not asked about and is refused; so is
/// every other scheme — `file:`, `javascript:`, `data:` and the rest are the program's to
/// leave in its own text.
pub fn web_target(url: &str) -> Option<&str> {
    if url.len() > LINK_LIMIT || !url.bytes().all(|byte| byte.is_ascii_graphic()) {
        return None;
    }
    if url.contains('\\') {
        return None;
    }
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // A host is there, and it is a host: no credentials, and not a bare port.
    if authority.is_empty() || authority.contains('@') || authority.starts_with(':') {
        return None;
    }
    Some(url)
}

/// What an answer is remembered by ([`key`]): the same program in the same place with the
/// same set, whatever the order the set was declared in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    place: Option<String>,
    identity: String,
    path: String,
    domains: Vec<(&'static str, String)>,
    permissions: Vec<&'static str>,
}

/// The key of `consent` as `origin` asks it; `None` when the program cannot be told from
/// another ([`Origin::identity`]), and an answer is then not remembered: asking again is the
/// way to be wrong that costs a question. What the program says about itself or its view
/// ([`Consent::program`], [`Consent::title`]) names nothing; the identity is the application's.
pub fn key(origin: &Origin, consent: &Consent) -> Option<Key> {
    let identity = origin.identity.clone().filter(|name| !name.is_empty())?;
    let path = origin.path.clone().filter(|path| !path.is_empty())?;
    let mut domains: Vec<(&'static str, String)> = consent
        .domains
        .iter()
        .map(|declared| {
            (
                declared.directive.key(),
                declared.domain.as_str().to_owned(),
            )
        })
        .collect();
    domains.sort();
    domains.dedup();
    let mut permissions: Vec<&'static str> = consent
        .permissions
        .iter()
        .map(|permission| permission.key())
        .collect();
    permissions.sort_unstable();
    permissions.dedup();
    Some(Key {
        place: origin.place.clone(),
        identity,
        path,
        domains,
        permissions,
    })
}

/// The answers given so far, kept until the application quits. Owned by one thread (the
/// shell's main one) and written nowhere.
#[derive(Debug, Default)]
pub struct Memory {
    answers: Vec<(Key, bool)>,
}

impl Memory {
    /// No answer yet.
    pub const fn new() -> Self {
        Self {
            answers: Vec::new(),
        }
    }

    /// What the person said to this, if they have: `true` is allowed.
    pub fn recall(&self, key: &Key) -> Option<bool> {
        self.answers
            .iter()
            .find(|(known, _)| known == key)
            .map(|(_, allowed)| *allowed)
    }

    /// Keeps an answer, in place of an earlier one to the same; at [`MEMORY_LIMIT`] the
    /// oldest goes.
    pub fn remember(&mut self, key: Key, allowed: bool) {
        self.answers.retain(|(known, _)| *known != key);
        if self.answers.len() >= MEMORY_LIMIT {
            self.answers.remove(0);
        }
        self.answers.push((key, allowed));
    }
}

/// The questions about access that are up, by what each asks, so that an answer to one settles
/// the others that ask the same ([`Key`]). `T` is whatever the toolkit finds a question by (a
/// pane and the number of its question).
///
/// Only a question that has a key is kept here: one about a program that cannot be told from
/// another is not the same as any other, and is asked every time.
#[derive(Debug)]
pub struct Waiting<T> {
    questions: Vec<(Key, T)>,
}

impl<T> Default for Waiting<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Waiting<T> {
    /// No question up.
    pub const fn new() -> Self {
        Self {
            questions: Vec::new(),
        }
    }

    /// A question that asks `key` is up, found again as `who`.
    pub fn up(&mut self, key: Key, who: T) {
        self.questions.push((key, who));
    }

    /// The questions for which `is` holds are no longer up: answered, taken away, or their pane
    /// is gone.
    pub fn gone(&mut self, mut is: impl FnMut(&T) -> bool) {
        self.questions.retain(|(_, who)| !is(who));
    }

    /// The person answered a question that asks `key` (which is [`gone`](Self::gone) by now):
    /// every other that asks the same is no longer up either, and is returned, to be settled
    /// with the same answer. A question that asks something else stays.
    pub fn answered(&mut self, key: &Key) -> Vec<T> {
        let mut same = Vec::new();
        let mut kept = Vec::with_capacity(self.questions.len());
        for (asked, who) in std::mem::take(&mut self.questions) {
            if &asked == key {
                same.push(who);
            } else {
                kept.push((asked, who));
            }
        }
        self.questions = kept;
        same
    }
}

#[cfg(test)]
mod tests {
    use bt_core::pencere::Domain;

    use super::*;

    fn declared(directive: Directive, origin: &str) -> Declared {
        Declared {
            directive,
            domain: Domain::parse(origin).expect("an origin"),
        }
    }

    fn consent_of(domains: &[(Directive, &str)], permissions: &[Permission]) -> Consent {
        Consent {
            id: 0,
            title: None,
            program: None,
            domains: domains
                .iter()
                .map(|(directive, origin)| declared(*directive, origin))
                .collect(),
            permissions: permissions.to_vec(),
        }
    }

    /// `consent`, for the view the program called `title`.
    fn titled(mut consent: Consent, title: &str) -> Consent {
        consent.title = Some(title.to_owned());
        consent
    }

    /// `consent`, with the words the program gave for its view and for itself.
    fn said(mut consent: Consent, title: Option<&str>, program: Option<&str>) -> Consent {
        consent.title = title.map(str::to_owned);
        consent.program = program.map(str::to_owned);
        consent
    }

    /// An origin whose program is told from others by the name it is called by and the file
    /// it was run from (`/usr/bin/<name>`).
    fn origin(place: Option<&str>, program: Option<&str>) -> Origin {
        Origin {
            place: place.map(str::to_owned),
            program: program.map(str::to_owned),
            identity: program.map(str::to_owned),
            path: program.map(|name| format!("/usr/bin/{name}")),
            said: None,
        }
    }

    #[test]
    fn a_program_that_cannot_be_told_from_another_has_no_answer_kept() {
        let set = consent_of(&[(Directive::Connect, "https://api.example.com")], &[]);
        // `node` is every script's name: the pane read the interpreter, and no script.
        let interpreter = Origin {
            identity: None,
            ..origin(None, Some("node"))
        };
        assert_eq!(key(&interpreter, &set), None);
        // A plain `ssh`: the place is known, the program is not.
        assert_eq!(key(&origin(Some("prod"), None), &set), None);
        // Two scripts one interpreter runs are two programs, whatever the pane called them.
        let script = |name: &str| Origin {
            identity: Some(name.to_owned()),
            ..origin(None, Some("node"))
        };
        assert_ne!(
            key(&script("containers.js"), &set),
            key(&script("evil.js"), &set)
        );
        assert!(key(&script("containers.js"), &set).is_some());
    }

    /// A name is not who a program is: two programs of one name run from two places are two
    /// programs, and the answer to the first is not the second's. The same program in the same
    /// place keeps its answer, and a program whose place is not known is asked every time.
    #[test]
    fn two_programs_of_one_name_in_two_places_are_asked_separately() {
        let set = consent_of(&[(Directive::Connect, "https://api.example.com")], &[]);
        let at = |path: &str| Origin {
            path: Some(path.to_owned()),
            ..origin(None, Some("index.js"))
        };
        let (mine, other) = (at("/home/me/a/index.js"), at("/tmp/evil/index.js"));
        assert_ne!(key(&mine, &set), key(&other, &set));
        assert_eq!(key(&mine, &set), key(&at("/home/me/a/index.js"), &set));
        assert!(key(&mine, &set).is_some());

        let mut memory = Memory::new();
        memory.remember(key(&mine, &set).expect("a key"), true);
        assert_eq!(
            memory.recall(&key(&at("/home/me/a/index.js"), &set).unwrap()),
            Some(true)
        );
        assert_eq!(memory.recall(&key(&other, &set).unwrap()), None);

        for path in [None, Some(String::new())] {
            let unplaced = Origin {
                path,
                ..origin(None, Some("index.js"))
            };
            assert_eq!(key(&unplaced, &set), None, "no place, no kept answer");
        }
    }

    #[test]
    fn one_address_on_this_mac() {
        let ask = consent(
            &origin(None, Some("docker-ui")),
            &titled(
                consent_of(&[(Directive::Connect, "https://api.example.com")], &[]),
                "list_containers",
            ),
            "bateri",
        );
        assert_eq!(ask.title, "Allow this view to reach other sites?");
        assert_eq!(
            ask.text,
            "“docker-ui” on this Mac opened the view “list_containers”, and the view \
asks to:\n\nConnect to\n• https://api.example.com\n\nbateri keeps your answer until it quits."
        );
        assert_eq!((ask.no, ask.yes), ("Don't Allow", "Allow"));
    }

    #[test]
    fn several_addresses_and_features_are_listed_under_what_the_view_does_with_them() {
        let ask = consent(
            &origin(Some("prod"), Some("maps")),
            &titled(
                consent_of(
                    &[
                        (Directive::Resource, "https://tiles.example.org"),
                        (Directive::Connect, "https://api.example.com"),
                        (Directive::Connect, "wss://live.example.com"),
                        (Directive::Frame, "https://*.example.net"),
                        (Directive::BaseUri, "https://example.com"),
                    ],
                    &[
                        Permission::Camera,
                        Permission::Geolocation,
                        Permission::ClipboardWrite,
                    ],
                ),
                "show",
            ),
            "bateri",
        );
        assert_eq!(
            ask.title,
            "Allow this view to reach other sites, use the camera, see your location and \
write to the clipboard?"
        );
        assert_eq!(
            ask.text,
            "“maps” on the server “prod” opened the view “show”, and the view asks to:\
\n\nConnect to\n• https://api.example.com\n• wss://live.example.com\
\n\nLoad scripts, styles, images and fonts from\n• https://tiles.example.org\
\n\nEmbed pages from\n• https://*.example.net\
\n\nTake its base address from\n• https://example.com\
\n\nUse\n• the camera\n• your location\n• the clipboard, to write to it\
\n\nbateri keeps your answer until it quits."
        );
    }

    #[test]
    fn a_feature_alone_is_asked_for_by_what_it_is() {
        let ask = consent(
            &origin(None, None),
            &consent_of(&[], &[Permission::Microphone]),
            "bateri",
        );
        assert_eq!(ask.title, "Allow this view to use the microphone?");
        assert_eq!(
            ask.text,
            "A program on this Mac opened a view, and the view asks to:\n\nUse\n• the microphone\
\n\nbateri keeps your answer until it quits."
        );
    }

    #[test]
    fn a_place_on_this_mac_or_a_private_network_is_marked_and_warned_about() {
        let domains = consent_of(
            &[
                (Directive::Connect, "http://localhost:8080"),
                (Directive::Connect, "http://192.168.1.5"),
                (Directive::Connect, "https://api.example.com"),
            ],
            &[],
        );
        let local = consent(&origin(None, Some("tool")), &domains, "bateri");
        assert!(
            local.text.contains(
                "• http://localhost:8080 (this Mac)\n• http://192.168.1.5 (a private network)\
\n• https://api.example.com\n\nA place marked above is on this Mac or a network it is on, and \
the view could reach what runs there."
            ),
            "{}",
            local.text
        );
        // A program on a server opens a view that still runs here: `localhost` is not the
        // server's.
        let remote = consent(&origin(Some("prod"), Some("tool")), &domains, "bateri");
        assert!(
            remote.text.contains(
                "\n\nThe view runs on this Mac, so a place marked above is on this Mac or a \
network it is on, not on the server “prod”."
            ),
            "{}",
            remote.text
        );
        // No warning where nothing is marked.
        let public = consent(
            &origin(None, None),
            &consent_of(&[(Directive::Connect, "https://api.example.com")], &[]),
            "bateri",
        );
        assert!(!public.text.contains("marked above"), "{}", public.text);
    }

    #[test]
    fn a_long_list_is_cut_and_a_place_in_the_cut_part_still_warns() {
        let mut domains: Vec<(Directive, String)> = (0..LISTED)
            .map(|at| (Directive::Connect, format!("https://site{at}.example.com")))
            .collect();
        domains.push((Directive::Connect, "http://localhost:9".to_owned()));
        domains.push((Directive::Connect, "https://last.example.com".to_owned()));
        let list: Vec<(Directive, &str)> = domains
            .iter()
            .map(|(directive, text)| (*directive, text.as_str()))
            .collect();
        let ask = consent(&origin(None, None), &consent_of(&list, &[]), "bateri");
        assert!(
            ask.text
                .contains("• https://site7.example.com\n• and 2 more")
        );
        assert!(!ask.text.contains("localhost"), "{}", ask.text);
        assert!(ask.text.contains("A place marked above"), "{}", ask.text);
    }

    #[test]
    fn what_the_program_and_the_machine_call_themselves_is_cleaned_before_it_is_quoted() {
        let ask = consent(
            &origin(
                Some("this Mac”.\nAllow everything.\u{202e}“"),
                Some("tool\u{200b}\"x\"\nline"),
            ),
            &titled(
                consent_of(&[(Directive::Connect, "https://api.example.com")], &[]),
                "a\tb\u{202e}c",
            ),
            "bate\nri",
        );
        assert!(
            ask.text.starts_with(
                "“toolx line” on the server “this Mac. Allow everything.” opened the view \
“a bc”, and the view asks to:"
            ),
            "{}",
            ask.text
        );
        assert!(
            ask.text
                .ends_with("bate ri keeps your answer until it quits.")
        );
        assert!(!ask.text.contains('\u{202e}'));
        // A name that is nothing after cleaning is as if the program had none.
        let nameless = consent(
            &origin(Some(" \u{200b} "), Some("\n")),
            &consent_of(&[], &[Permission::Camera]),
            "",
        );
        assert!(
            nameless
                .text
                .starts_with("A program on this Mac opened a view,"),
            "{}",
            nameless.text
        );
        assert!(
            nameless
                .text
                .ends_with("The application keeps your answer until it quits.")
        );
    }

    #[test]
    fn a_long_name_is_cut_and_a_name_cannot_make_the_question_long() {
        let ask = consent(
            &origin(None, Some(&"p".repeat(500))),
            &titled(
                consent_of(&[(Directive::Connect, "https://api.example.com")], &[]),
                &"t".repeat(500),
            ),
            "bateri",
        );
        let first = ask.text.lines().next().expect("a line");
        assert!(first.chars().count() < 200, "{first}");
        assert!(first.contains('…'));
    }

    #[test]
    fn the_same_set_in_any_order_is_the_same_key() {
        let there = origin(Some("prod"), Some("maps"));
        let one = consent_of(
            &[
                (Directive::Connect, "https://a.example.com"),
                (Directive::Resource, "https://b.example.com"),
            ],
            &[Permission::Camera, Permission::Microphone],
        );
        let other = consent_of(
            &[
                (Directive::Resource, "https://b.example.com"),
                (Directive::Connect, "https://a.example.com"),
                (Directive::Connect, "https://a.example.com"),
            ],
            &[
                Permission::Microphone,
                Permission::Camera,
                Permission::Camera,
            ],
        );
        assert_eq!(key(&there, &one), key(&there, &other));
        assert!(key(&there, &one).is_some());
    }

    #[test]
    fn another_program_server_or_set_is_another_key() {
        let base = origin(Some("prod"), Some("maps"));
        let set = consent_of(&[(Directive::Connect, "https://a.example.com")], &[]);
        let reference = key(&base, &set);
        for changed in [
            origin(Some("staging"), Some("maps")),
            origin(None, Some("maps")),
            origin(Some("prod"), Some("other")),
            origin(Some("prod"), None),
        ] {
            assert_ne!(key(&changed, &set), reference, "{changed:?}");
        }
        for other in [
            consent_of(&[(Directive::Connect, "https://b.example.com")], &[]),
            consent_of(&[(Directive::Resource, "https://a.example.com")], &[]),
            consent_of(
                &[(Directive::Connect, "https://a.example.com")],
                &[Permission::Camera],
            ),
            consent_of(&[], &[]),
        ] {
            assert_ne!(key(&base, &other), reference);
        }
        // This Mac and a server that is named like it are not the same place.
        assert_ne!(
            key(&origin(Some("this Mac"), Some("maps")), &set),
            key(&origin(None, Some("maps")), &set)
        );
    }

    #[test]
    fn the_title_a_program_gives_its_view_is_not_part_of_a_key() {
        let set = consent_of(&[(Directive::Connect, "https://a.example.com")], &[]);
        let at = origin(None, Some("maps"));
        assert_eq!(
            key(&at, &titled(set.clone(), "Containers")),
            key(&at, &titled(set, "Something else"))
        );
    }

    #[test]
    fn memory_gives_back_both_answers_and_the_latest_for_a_key() {
        let at = origin(None, Some("maps"));
        let yes = key(
            &at,
            &consent_of(&[(Directive::Connect, "https://a.example.com")], &[]),
        )
        .expect("a key");
        let no = key(
            &at,
            &consent_of(&[(Directive::Connect, "https://b.example.com")], &[]),
        )
        .expect("a key");
        let mut memory = Memory::new();
        assert_eq!(memory.recall(&yes), None);
        memory.remember(yes.clone(), true);
        memory.remember(no.clone(), false);
        assert_eq!(memory.recall(&yes), Some(true));
        assert_eq!(memory.recall(&no), Some(false));
        memory.remember(yes.clone(), false);
        assert_eq!(memory.recall(&yes), Some(false));
    }

    #[test]
    fn memory_forgets_the_oldest_at_its_limit() {
        let keys: Vec<Key> = (0..=MEMORY_LIMIT)
            .map(|at| {
                key(
                    &origin(None, Some("maps")),
                    &consent_of(
                        &[(Directive::Connect, &format!("https://site{at}.example.com"))],
                        &[],
                    ),
                )
            })
            .collect::<Option<_>>()
            .expect("keys");
        let mut memory = Memory::new();
        for key in &keys {
            memory.remember(key.clone(), true);
        }
        assert_eq!(memory.recall(&keys[0]), None, "the oldest went");
        assert_eq!(memory.recall(&keys[1]), Some(true));
        assert_eq!(memory.recall(&keys[MEMORY_LIMIT]), Some(true));
    }

    #[test]
    fn the_question_quotes_the_views_title_and_the_programs_own_name_before_what_the_pane_read() {
        let set = consent_of(&[(Directive::Connect, "https://api.example.com")], &[]);
        let at = origin(None, Some("node"));
        // The program's own words are what the person is shown.
        let titled = consent(
            &at,
            &said(set.clone(), Some("Containers"), Some("Docker UI")),
            "bateri",
        );
        assert!(
            titled.text.starts_with(
                "“Docker UI” on this Mac opened the view “Containers”, and the view asks to:"
            ),
            "{}",
            titled.text
        );
        // No title is just a view; no name of its own is what the pane read.
        let plain = consent(&at, &said(set.clone(), None, None), "bateri");
        assert!(
            plain
                .text
                .starts_with("“node” on this Mac opened a view, and the view asks to:"),
            "{}",
            plain.text
        );
        // Both are quoted like any name: cleaned, and cut to the question's width.
        let hostile = consent(
            &at,
            &said(set, Some("a\u{202e}b\"c\"\nd"), Some(&"p".repeat(500))),
            "bateri",
        );
        let first = hostile.text.lines().next().expect("a line");
        assert!(first.contains("the view “abc d”"), "{first}");
        assert!(first.chars().count() < 200, "{first}");
        assert!(!hostile.text.contains('\u{202e}'));
    }

    #[test]
    fn what_a_program_says_about_itself_does_not_make_an_answer_another_one() {
        // A program can call itself anything: the key is by what the application read in the
        // pane, and not by these.
        let at = origin(None, Some("node"));
        let set = consent_of(&[(Directive::Connect, "https://a.example.com")], &[]);
        assert_eq!(
            key(&at, &said(set.clone(), Some("A"), Some("B"))),
            key(&at, &said(set.clone(), None, None))
        );
        assert_ne!(
            key(&origin(None, Some("python")), &set),
            key(&origin(None, Some("node")), &set)
        );
        // And what the program says is not what the pane read, whatever it says.
        let spoken = Origin {
            said: Some("node".to_owned()),
            ..origin(None, Some("python"))
        };
        assert_ne!(key(&spoken, &set), key(&at, &set));
    }

    #[test]
    fn two_panes_that_ask_the_same_are_asked_once_and_a_different_question_is_still_asked() {
        let set = |domain: &str| consent_of(&[(Directive::Connect, domain)], &[]);
        let at = origin(None, Some("maps"));
        let maps = key(&at, &set("https://a.example.com")).expect("a key");
        let other_set = key(&at, &set("https://b.example.com")).expect("a key");
        let other_program =
            key(&origin(None, Some("tiles")), &set("https://a.example.com")).expect("a key");

        // Four panes asked at once, as a restored session does: two the same, two not.
        let mut waiting = Waiting::new();
        waiting.up(maps.clone(), 1);
        waiting.up(other_set.clone(), 2);
        waiting.up(maps.clone(), 3);
        waiting.up(other_program.clone(), 4);

        // The person answers the first (which is up no longer): the other that asks the same
        // goes with it, and the two that ask something else stay up for their own answers.
        waiting.gone(|who| *who == 1);
        assert_eq!(waiting.answered(&maps), [3]);
        waiting.gone(|who| *who == 2);
        assert_eq!(waiting.answered(&other_set), Vec::<i32>::new());
        waiting.gone(|who| *who == 4);
        assert_eq!(waiting.answered(&other_program), Vec::<i32>::new());
        // Nothing is left to settle.
        assert_eq!(waiting.answered(&maps), Vec::<i32>::new());
    }

    #[test]
    fn a_question_that_went_before_the_answer_is_not_settled_by_it() {
        let set = consent_of(&[(Directive::Connect, "https://a.example.com")], &[]);
        let maps = key(&origin(None, Some("maps")), &set).expect("a key");
        let mut waiting = Waiting::new();
        for who in [1, 2, 3] {
            waiting.up(maps.clone(), who);
        }
        // The second one's program gave up, or its pane closed under its sheet: it is gone. The
        // person answers the first: only the third is settled with it.
        waiting.gone(|who| *who == 2);
        waiting.gone(|who| *who == 1);
        assert_eq!(waiting.answered(&maps), [3]);
        assert_eq!(waiting.answered(&maps), Vec::<i32>::new());
    }

    #[test]
    fn a_link_is_asked_about_in_the_words_the_program_has_for_itself() {
        let at = Origin {
            said: Some("Docker UI".to_owned()),
            ..origin(None, Some("node"))
        };
        let ask = link(&at, "https://example.com/").expect("a link");
        assert!(
            ask.question
                .text
                .starts_with("“Docker UI” on this Mac opened a view"),
            "{}",
            ask.question.text
        );
    }

    #[test]
    fn a_web_link_is_asked_about_with_where_the_program_runs() {
        let ask = link(
            &origin(Some("prod"), Some("docker-ui")),
            "https://example.com/docs?q=1#top",
        )
        .expect("a link");
        assert_eq!(ask.target, "https://example.com/docs?q=1#top");
        assert_eq!(ask.question.title, "Open this link?");
        assert_eq!(
            ask.question.text,
            "“docker-ui” on the server “prod” opened a view that asks to open this link in \
your browser on this Mac:\n\nhttps://example.com/docs?q=1#top"
        );
        assert_eq!((ask.question.no, ask.question.yes), ("Cancel", "Open"));
        for url in [
            "HTTP://example.com",
            "hTTps://example.com:8443/",
            "http://[::1]:3000/x",
        ] {
            assert!(web_target(url).is_some(), "{url}");
        }
    }

    #[test]
    fn what_is_not_a_plain_web_link_is_not_asked_about() {
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,hi",
            "ftp://example.com/",
            "mailto:a@example.com",
            "bateri://tab/1",
            "example.com",
            "https://",
            "https:///path",
            "https://:80/",
            "https://shop.example@elsewhere.test/",
            "https://user:pass@example.com/",
            "https://example.com\\@evil.test/",
            "https://example.com/a b",
            "https://example.com/\n",
            "https://exаmple.com/",
            "https://example.com/é",
            "",
        ] {
            assert_eq!(web_target(url), None, "{url:?}");
            assert_eq!(link(&Origin::default(), url), None, "{url:?}");
        }
        // The `@` after the host is the path's.
        assert!(web_target("https://example.com/@user").is_some());
        let long = format!("https://example.com/{}", "a".repeat(LINK_LIMIT));
        assert_eq!(web_target(&long), None);
    }

    #[test]
    fn a_long_link_is_shown_cut_and_counted_and_opens_whole() {
        let url = format!("https://example.com/{}", "a".repeat(400));
        let ask = link(&Origin::default(), &url).expect("a link");
        assert_eq!(ask.target, url);
        let shown = ask
            .question
            .text
            .split("\n\n")
            .nth(1)
            .expect("the link part");
        assert!(shown.starts_with("https://example.com/aaaa"), "{shown}");
        assert!(
            shown.ends_with(&format!(
                "…\n(and {} more characters)",
                url.len() - LINK_SHOWN
            )),
            "{shown}"
        );
    }
}
