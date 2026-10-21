//! What a program's view asks of the person, and what the application remembers of the
//! answer.
//!
//! A view is a web page the program wrote, running on this Mac. It asks two things the
//! person decides, and both come from the program and not from the application:
//!
//! * **Access** ([`consent`]): the addresses it declared (`connectDomains` and the other
//!   lists) and the browser features it wants (camera, microphone, location, the clipboard).
//!   The view starts with only what the person allowed.
//! * **A link** ([`link`]): `ui/open-link`, which the view may ask for and the person's
//!   browser, on this Mac, opens.
//!
//! The words of both questions are made here, from the facts, so that they are tested and
//! no toolkit writes its own: **where the program runs** (the server's name, or this Mac)
//! is part of every question, because a view that a program on a server opened still runs
//! here — `localhost` in its list is this Mac's, not the server's, and the question says so
//! ([`Locality`]).
//!
//! Every name in a question is **cleaned and bounded** the way the strip's are
//! ([`view_strip`]): the program's name, the tool's, the server's and every address come
//! from the program or from the machine it runs on, and a question must not be made to read
//! as the application's own words by a line break, a direction override or a quotation mark.
//! A link is checked harder, because what the person sees must be what opens
//! ([`web_target`]).
//!
//! **What is remembered** ([`Memory`]): an answer to the first question, by *where* the
//! program runs (this Mac or the server), *which* program, *which* tool and *which* set —
//! the addresses and features, in any order — and until the application quits; it is not
//! written anywhere. Another program, another server or another set asks again: what a
//! person allows one program is not allowed to another (§12.2 of the protocol). Both answers
//! are remembered: a tool that is opened often does not ask each time, and a person who
//! said no is not asked again either. An answer for a tool whose name could not be read
//! is not remembered.
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
    /// The program's own name; `None` when nothing here knows it (a plain `ssh`, a server
    /// that has not said).
    pub program: Option<String>,
    /// The tool the program opened the view for, as it wrote the name; `None` when it
    /// could not be read.
    pub tool: Option<String>,
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
/// read as this Mac.
fn subject(origin: &Origin) -> String {
    let place = match origin
        .place
        .as_deref()
        .and_then(|p| quotable(p, NAME_LIMIT))
    {
        Some(server) => format!("the server “{server}”"),
        None => THIS_MAC.to_owned(),
    };
    match origin
        .program
        .as_deref()
        .and_then(|p| quotable(p, NAME_LIMIT))
    {
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

    let opened = match origin
        .tool
        .as_deref()
        .and_then(|t| quotable(t, TITLE_LIMIT))
    {
        Some(tool) => format!("opened a view of its tool “{tool}”"),
        None => "opened a view".to_owned(),
    };
    let mut text = format!("{} {opened}, and the view asks to:", subject(origin));

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
                subject(origin)
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
/// same tool and the same set, whatever the order the set was declared in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    place: Option<String>,
    program: Option<String>,
    tool: String,
    domains: Vec<(&'static str, String)>,
    permissions: Vec<&'static str>,
}

/// The key of `consent` as `origin` asks it; `None` when the tool is not known, and an answer
/// is then not remembered: asking again is the way to be wrong that costs a question.
pub fn key(origin: &Origin, consent: &Consent) -> Option<Key> {
    let tool = origin.tool.clone().filter(|tool| !tool.is_empty())?;
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
        program: origin.program.clone(),
        tool,
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
            domains: domains
                .iter()
                .map(|(directive, origin)| declared(*directive, origin))
                .collect(),
            permissions: permissions.to_vec(),
        }
    }

    fn origin(place: Option<&str>, program: Option<&str>, tool: Option<&str>) -> Origin {
        Origin {
            place: place.map(str::to_owned),
            program: program.map(str::to_owned),
            tool: tool.map(str::to_owned),
        }
    }

    #[test]
    fn one_address_on_this_mac() {
        let ask = consent(
            &origin(None, Some("docker-ui"), Some("list_containers")),
            &consent_of(&[(Directive::Connect, "https://api.example.com")], &[]),
            "bateri",
        );
        assert_eq!(ask.title, "Allow this view to reach other sites?");
        assert_eq!(
            ask.text,
            "“docker-ui” on this Mac opened a view of its tool “list_containers”, and the view \
asks to:\n\nConnect to\n• https://api.example.com\n\nbateri keeps your answer until it quits."
        );
        assert_eq!((ask.no, ask.yes), ("Don't Allow", "Allow"));
    }

    #[test]
    fn several_addresses_and_features_are_listed_under_what_the_view_does_with_them() {
        let ask = consent(
            &origin(Some("prod"), Some("maps"), Some("show")),
            &consent_of(
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
            "bateri",
        );
        assert_eq!(
            ask.title,
            "Allow this view to reach other sites, use the camera, see your location and \
write to the clipboard?"
        );
        assert_eq!(
            ask.text,
            "“maps” on the server “prod” opened a view of its tool “show”, and the view asks to:\
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
            &origin(None, None, None),
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
        let local = consent(&origin(None, Some("tool"), None), &domains, "bateri");
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
        let remote = consent(
            &origin(Some("prod"), Some("tool"), None),
            &domains,
            "bateri",
        );
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
            &origin(None, None, None),
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
        let ask = consent(&origin(None, None, None), &consent_of(&list, &[]), "bateri");
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
                Some("a\tb\u{202e}c"),
            ),
            &consent_of(&[(Directive::Connect, "https://api.example.com")], &[]),
            "bate\nri",
        );
        assert!(
            ask.text.starts_with(
                "“toolx line” on the server “this Mac. Allow everything.” opened a view of its \
tool “a bc”, and the view asks to:"
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
            &origin(Some(" \u{200b} "), Some("\n"), None),
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
            &origin(None, Some(&"p".repeat(500)), Some(&"t".repeat(500))),
            &consent_of(&[(Directive::Connect, "https://api.example.com")], &[]),
            "bateri",
        );
        let first = ask.text.lines().next().expect("a line");
        assert!(first.chars().count() < 200, "{first}");
        assert!(first.contains('…'));
    }

    #[test]
    fn the_same_set_in_any_order_is_the_same_key() {
        let there = origin(Some("prod"), Some("maps"), Some("show"));
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
    fn another_program_server_tool_or_set_is_another_key() {
        let base = origin(Some("prod"), Some("maps"), Some("show"));
        let set = consent_of(&[(Directive::Connect, "https://a.example.com")], &[]);
        let reference = key(&base, &set);
        for (changed, set) in [
            (origin(Some("staging"), Some("maps"), Some("show")), &set),
            (origin(None, Some("maps"), Some("show")), &set),
            (origin(Some("prod"), Some("other"), Some("show")), &set),
            (origin(Some("prod"), None, Some("show")), &set),
            (origin(Some("prod"), Some("maps"), Some("list")), &set),
        ] {
            assert_ne!(key(&changed, set), reference, "{changed:?}");
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
            key(&origin(Some("this Mac"), None, Some("show")), &set),
            key(&origin(None, None, Some("show")), &set)
        );
    }

    #[test]
    fn a_tool_that_could_not_be_read_has_no_key() {
        let set = consent_of(&[(Directive::Connect, "https://a.example.com")], &[]);
        assert_eq!(key(&origin(None, Some("maps"), None), &set), None);
        assert_eq!(key(&origin(None, Some("maps"), Some("")), &set), None);
    }

    #[test]
    fn memory_gives_back_both_answers_and_the_latest_for_a_key() {
        let set = consent_of(&[(Directive::Connect, "https://a.example.com")], &[]);
        let yes = key(&origin(None, Some("maps"), Some("show")), &set).expect("a key");
        let no = key(&origin(None, Some("maps"), Some("list")), &set).expect("a key");
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
        let set = consent_of(&[(Directive::Connect, "https://a.example.com")], &[]);
        let keys: Vec<Key> = (0..=MEMORY_LIMIT)
            .map(|at| {
                key(
                    &origin(None, Some("maps"), Some(&format!("tool{at}"))),
                    &set,
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
    fn a_web_link_is_asked_about_with_where_the_program_runs() {
        let ask = link(
            &origin(Some("prod"), Some("docker-ui"), Some("show")),
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
