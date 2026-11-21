//! A program's web view as an agent reads and drives it, as far as that needs no web view.
//!
//! The view's own part is the app's (`bt-shell-macos`, `pencere_view/agent.rs`): a script that lives
//! in a content world of the web view's own, which the page's scripts cannot see, the calls into
//! it, and the picture. What the two ends say to each other is here: the **record** the script
//! answers a look with ([`parse`]), the **plan** an action is made into once the element it names is
//! found in a reading ([`plan`]), the keys as the page's events have them ([`js_keys`]), and the
//! mistakes the script says in codes ([`failure`]).
//!
//! A reading of a web view is the same [`Reading`] a native page has, so it is written by the same
//! writer ([`super::written`]): its elements are numbered by the script, a number is an [`Id`], and
//! the ref an agent types is derived from it as for a native page's. A web view has no commands, no
//! menu and no editor of the kit's: `command`, `menu`, `pick` and `text` say so; its text fields and
//! its editable boxes are fields, which `fill` and `type` reach.
//!
//! A web page is the program's, and what it says is untrusted words: they are cleaned where the
//! reading is written ([`pencere_drive::reading::clean`]), and a password field's value is never
//! in the record, whatever the script held.

use pencere_drive::reading::Focus;
use pencere_drive::reading::{FieldReading, Item, ScrollReading, reference};
use pencere_drive::{Reading, Rest};

use super::act::{describe, target};
use super::words;
use crate::agent::message::{Failure, Kind};
use crate::native_view::{FieldKind, Id, Key, KeyConvention, Rect, Shortcut, State};
use kablo_bateri::view::{Action, Way};

/// How long, in milliseconds, the page's document must have stood still for the page to be at rest.
pub const QUIET_MS: u32 = 300;

/// The most lines a record is read to: a page of a thousand elements is read as far as the script
/// listed it, and no further than this.
const LINES_MOST: usize = 4000;

/// The first line of a record: its version.
pub const RECORD: &str = "v1";

fn mistake(text: impl Into<String>) -> Failure {
    Failure::new(Kind::Failed, text)
}

// ─── The record ─────────────────────────────────────────────────────────────────────────────────

/// What the script said of the page, as an agent's reading. The number of a view's element is its
/// [`Id`] (`Id::new(number)`).
///
/// The record is lines of tab-separated words; a word that is text is percent-encoded
/// (`encodeURIComponent`), so no tab, newline or control character of the page can end one early:
///
/// ```text
/// v1
/// rest <quiet milliseconds> <images 0|1> <loading 0|1>
/// focus <number or 0>
/// note <words>
/// item <number> <role> <words> <flags> <value> <x> <y> <width> <height>
/// field <number> <line|lines|secret> <label> <hint> <value> <covered 0|1> <x> <y> <width> <height>
/// scroll <number> <percent> <above 0|1> <below 0|1> <x> <y> <width> <height>
/// ```
///
/// An item's flags are a comma list of `checked`, `unchecked`, `selected`, `expanded`, `collapsed`
/// and `disabled`; its value and a field's are `-` for none and `+` and the text otherwise.
///
/// # Errors
///
/// The text is not a record: the script answered something else.
pub fn parse(record: &str) -> Result<Reading, String> {
    let mut lines = record.lines();
    if lines.next() != Some(RECORD) {
        return Err("the view's script gave an answer that is not a record".to_owned());
    }
    let mut reading = Reading::default();
    let mut focus = 0u64;
    let (mut quiet, mut images, mut loading) = (0u32, false, false);
    for line in lines.take(LINES_MOST) {
        let at: Vec<&str> = line.split('\t').collect();
        match at.as_slice() {
            ["rest", q, i, l] => {
                quiet = q.parse().unwrap_or(0);
                images = *i == "1";
                loading = *l == "1";
            }
            ["focus", n] => focus = n.parse().unwrap_or(0),
            ["note", text] => reading.notes.push(decode(text)),
            ["item", n, role, words, flags, value, x, y, w, h] => {
                let Ok(n) = n.parse::<u64>() else { continue };
                reading.items.push(Item {
                    role: decode(role),
                    id: Some(Id::new(n)),
                    words: decode(words),
                    state: state_of(flags, optional(value)),
                    rect: rect_of(x, y, w, h),
                });
            }
            ["field", n, kind, label, hint, value, covered, x, y, w, h] => {
                let Ok(n) = n.parse::<u64>() else { continue };
                let kind = match *kind {
                    "secret" => FieldKind::Secret,
                    "lines" => FieldKind::Lines,
                    _ => FieldKind::Line,
                };
                let text = |word: &str| Some(decode(word)).filter(|text| !text.is_empty());
                reading.fields.push(FieldReading {
                    id: Id::new(n),
                    kind,
                    label: text(label),
                    hint: text(hint),
                    // A password's value is in no record, and so in no reading.
                    value: optional(value).filter(|_| kind != FieldKind::Secret),
                    covered: *covered == "1",
                    rect: rect_of(x, y, w, h),
                });
            }
            ["scroll", n, percent, above, below, x, y, w, h] => {
                let Ok(n) = n.parse::<u64>() else { continue };
                reading.scrolls.push(ScrollReading {
                    id: Id::new(n),
                    percent: percent.parse::<u8>().unwrap_or(0).min(100),
                    above: *above == "1",
                    below: *below == "1",
                    rect: rect_of(x, y, w, h),
                });
            }
            _ => {}
        }
    }
    reading.focus = if reading
        .fields
        .iter()
        .any(|field| field.id == Id::new(focus))
    {
        Focus::Field(Id::new(focus))
    } else {
        Focus::Page
    };
    reading.rest = Rest {
        // A document that changed a moment ago is still saying something.
        unprocessed: quiet < QUIET_MS,
        calls: 0,
        images,
        unsettled: loading,
        moving: false,
    };
    reading.notes.push(
        "this is a web view: it has no commands or menu, and `text` is not read; its fields are the page's own"
            .to_owned(),
    );
    Ok(reading)
}

/// `-` is none; `+` and the text is that text.
fn optional(word: &str) -> Option<String> {
    word.strip_prefix('+').map(decode)
}

fn state_of(flags: &str, value: Option<String>) -> State {
    let has = |flag: &str| flags.split(',').any(|word| word == flag);
    State {
        checked: if has("checked") {
            Some(true)
        } else if has("unchecked") {
            Some(false)
        } else {
            None
        },
        selected: has("selected"),
        expanded: if has("expanded") {
            Some(true)
        } else if has("collapsed") {
            Some(false)
        } else {
            None
        },
        disabled: has("disabled"),
        value,
    }
}

fn rect_of(x: &str, y: &str, w: &str, h: &str) -> Rect {
    let number = |word: &str| {
        word.parse::<f32>()
            .ok()
            .filter(|n| n.is_finite())
            .unwrap_or(0.0)
    };
    Rect::new(number(x), number(y), number(w), number(h))
}

/// `text` as `encodeURIComponent` wrote it: `%XX` bytes of UTF-8. What is not a whole escape is as
/// it is, and what is not UTF-8 is replaced.
#[must_use]
pub fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && let (Some(high), Some(low)) = (
                bytes.get(at + 1).copied().and_then(hex),
                bytes.get(at + 2).copied().and_then(hex),
            )
        {
            out.push(high * 16 + low);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

// ─── Keys ───────────────────────────────────────────────────────────────────────────────────────

/// A key as the page's keyboard events have it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsKey {
    /// `KeyboardEvent.key`: `Enter`, `ArrowDown`, `F5`, `a`.
    pub key: String,
    /// `KeyboardEvent.code`: `Enter`, `KeyA`, `Digit1`; empty where the key is none of those.
    pub code: String,
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}

fn convention() -> KeyConvention {
    if cfg!(target_os = "macos") {
        KeyConvention::Mac
    } else {
        KeyConvention::Linux
    }
}

/// The keys of an agent's `press` (`cmd+enter`, `f5`, `escape`, a letter), as the events the page
/// is sent: all of them are read before any is pressed.
///
/// # Errors
///
/// A word that is no key.
pub fn js_keys(keys: &[String]) -> Result<Vec<JsKey>, Failure> {
    keys.iter()
        .map(|spec| {
            let Shortcut { key, mods } =
                Shortcut::parse_for(spec, convention()).ok_or_else(|| {
                    mistake(format!(
                        "{} is not a key: write them as cmd+enter, f5, escape or a letter",
                        words(spec, 40)
                    ))
                })?;
            let (name, code) = match key {
                Key::Up => ("ArrowUp".to_owned(), "ArrowUp".to_owned()),
                Key::Down => ("ArrowDown".to_owned(), "ArrowDown".to_owned()),
                Key::Left => ("ArrowLeft".to_owned(), "ArrowLeft".to_owned()),
                Key::Right => ("ArrowRight".to_owned(), "ArrowRight".to_owned()),
                Key::PageUp => ("PageUp".to_owned(), "PageUp".to_owned()),
                Key::PageDown => ("PageDown".to_owned(), "PageDown".to_owned()),
                Key::Home => ("Home".to_owned(), "Home".to_owned()),
                Key::End => ("End".to_owned(), "End".to_owned()),
                Key::Enter => ("Enter".to_owned(), "Enter".to_owned()),
                Key::Escape => ("Escape".to_owned(), "Escape".to_owned()),
                Key::Tab => ("Tab".to_owned(), "Tab".to_owned()),
                Key::Backspace => ("Backspace".to_owned(), "Backspace".to_owned()),
                Key::Delete => ("Delete".to_owned(), "Delete".to_owned()),
                Key::Function(n) => (format!("F{n}"), format!("F{n}")),
                Key::Character(' ') => (" ".to_owned(), "Space".to_owned()),
                Key::Character(c) => {
                    let code = if c.is_ascii_alphabetic() {
                        format!("Key{}", c.to_ascii_uppercase())
                    } else if c.is_ascii_digit() {
                        format!("Digit{c}")
                    } else {
                        String::new()
                    };
                    // A letter with shift is the capital the page's key event has.
                    let name = if mods.shift && c.is_ascii_alphabetic() {
                        c.to_ascii_uppercase().to_string()
                    } else {
                        c.to_string()
                    };
                    (name, code)
                }
            };
            Ok(JsKey {
                key: name,
                code,
                shift: mods.shift,
                control: mods.control,
                alt: mods.alt,
                meta: mods.command,
            })
        })
        .collect()
}

// ─── The plan ───────────────────────────────────────────────────────────────────────────────────

/// What is done to the page, once the element an action names is found: the numbers are the
/// script's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Nothing: a reading, a wait, a picture.
    Nothing,
    Click {
        number: u64,
        times: u32,
    },
    Fill {
        number: u64,
        text: String,
    },
    Type {
        text: String,
    },
    Press(Vec<JsKey>),
    Scroll {
        number: u64,
        way: Way,
        screens: u32,
    },
}

/// An action made into what the script is asked and what the answer says was done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    pub act: Act,
    /// The line that leads the answer: what was done, with the element's role and words.
    pub said: Option<String>,
}

/// What a web view cannot do, said once.
fn not_there(verb: &str) -> Failure {
    mistake(format!(
        "a web view has no {verb}: read it, then click, fill, type, press, scroll or shot"
    ))
}

/// The failure of an action a web view does not have (`command`, `menu`, `pick`, `text`), so that
/// a request that is one is refused before the page is looked at.
#[must_use]
pub fn unsupported(action: &Action) -> Option<Failure> {
    match action {
        Action::Command(_) => Some(not_there("commands")),
        Action::Menu(_) | Action::Pick(_) => Some(not_there("menu")),
        Action::Text(_) => Some(not_there("`text`: the reading has the value of each field")),
        _ => None,
    }
}

/// `action`, as it is done to the page `reading` is of.
///
/// # Errors
///
/// The element is not named, is named by more than one, or is not what the action needs (a field
/// to fill, a box that scrolls), the keys are not keys, the keyboard is in no field to type into,
/// or the action is one a web view does not have.
pub fn plan(action: &Action, reading: &Reading) -> Result<Planned, Failure> {
    let nothing = || Planned {
        act: Act::Nothing,
        said: None,
    };
    let number = |id: Id| id.number();
    match action {
        Action::Read | Action::Wait { .. } | Action::Shot { .. } => Ok(nothing()),
        Action::Command(_) | Action::Menu(_) | Action::Pick(_) | Action::Text(_) => {
            Err(unsupported(action).unwrap_or_else(|| not_there("action")))
        }
        Action::Click(word) | Action::DoubleClick(word) => {
            let id = target(reading, word)?;
            let times = if matches!(action, Action::DoubleClick(_)) {
                2
            } else {
                1
            };
            let what = describe(reading, id);
            let verb = if times == 2 {
                "double-clicked"
            } else {
                "clicked"
            };
            Ok(Planned {
                act: Act::Click {
                    number: number(id),
                    times,
                },
                said: Some(format!("{verb} {what}")),
            })
        }
        Action::Fill { target: word, text } => {
            let id = target(reading, word)?;
            let what = describe(reading, id);
            if !reading.fields.iter().any(|field| field.id == id) {
                return Err(mistake(format!("{what} is not a field of the page")));
            }
            Ok(Planned {
                act: Act::Fill {
                    number: number(id),
                    text: text.clone(),
                },
                // The count and never the text: a field may be a password's.
                said: Some(format!(
                    "filled {what} with {} characters",
                    text.chars().count()
                )),
            })
        }
        Action::Type(text) => {
            let Focus::Field(id) = reading.focus else {
                return Err(mistake(
                    "the keyboard is in no field of the page: click or fill one first",
                ));
            };
            Ok(Planned {
                act: Act::Type { text: text.clone() },
                said: Some(format!(
                    "typed {} characters into the field {}",
                    text.chars().count(),
                    reference(id)
                )),
            })
        }
        Action::Press(keys) => Ok(Planned {
            act: Act::Press(js_keys(keys)?),
            said: Some(format!(
                "pressed {}",
                keys.iter()
                    .map(|key| words(key, 30))
                    .collect::<Vec<_>>()
                    .join(" ")
            )),
        }),
        Action::Scroll {
            target: word,
            way,
            count,
        } => {
            let id = target(reading, word)?;
            let what = describe(reading, id);
            if !reading.scrolls.iter().any(|scroll| scroll.id == id) {
                return Err(mistake(format!(
                    "{what} does not scroll: name one of the boxes the reading lists under scrolls"
                )));
            }
            let screens = count.unwrap_or(1).max(1);
            let said = match way {
                Way::Down => format!("scrolled {what} down {screens} screens"),
                Way::Up => format!("scrolled {what} up {screens} screens"),
                Way::Bottom => format!("scrolled {what} to the bottom"),
                Way::Top => format!("scrolled {what} to the top"),
            };
            Ok(Planned {
                act: Act::Scroll {
                    number: number(id),
                    way: *way,
                    screens,
                },
                said: Some(said),
            })
        }
    }
}

/// What the script says when it cannot do what it was asked: a code and, for some, a detail (the
/// result of a call is `ok` or `err`, a tab, the code, a tab, the detail).
///
/// # Errors
///
/// Always: the failure an agent is told. `what` is the element, as the answer names it.
pub fn failure(code: &str, detail: &str, what: &str) -> Failure {
    let detail = decode(detail);
    mistake(match code {
        "gone" => {
            format!("{what} is not on the page any more: read the view again for the names it has")
        }
        "hidden" => format!("{what} is not shown on the page now"),
        "covered" => format!(
            "{what} is covered by {}: close or move what covers it first",
            words(&detail, 60)
        ),
        "notfield" => format!("{what} is neither a text field nor an editable box"),
        "nooption" => format!("{what} has no such option; it has: {}", words(&detail, 200)),
        "nofocus" => "the keyboard is in no field of the page: click or fill one first".to_owned(),
        other => format!("the view's script could not do it ({})", words(other, 40)),
    })
}

/// The element an action names, as the answer says it, for a failure of the script's: `None` for an
/// action that names none.
#[must_use]
pub fn named(action: &Action, reading: &Reading) -> Option<String> {
    let word = match action {
        Action::Click(word) | Action::DoubleClick(word) => word,
        Action::Fill { target, .. } | Action::Scroll { target, .. } => target,
        _ => return None,
    };
    target(reading, word).ok().map(|id| describe(reading, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kablo_bateri::view::Way;

    fn enc(text: &str) -> String {
        text.bytes()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
                    (byte as char).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect()
    }

    fn record() -> String {
        let t = enc;
        [
            RECORD.to_owned(),
            "rest\t450\t0\t0".to_owned(),
            "focus\t7".to_owned(),
            format!(
                "item\t3\tbutton\t{}\tdisabled\t-\t10\t20\t80\t24",
                t("Save it")
            ),
            format!(
                "item\t4\ttext\t{}\t\t-\t0\t0\t100\t12",
                t("Line one\nLine\ttwo")
            ),
            format!(
                "item\t5\tcheckbox\t{}\tchecked\t-\t0\t0\t10\t10",
                t("Agree")
            ),
            format!(
                "field\t7\tline\t{}\t{}\t+{}\t0\t0\t40\t200\t24",
                t("Search"),
                t("type here"),
                t("api")
            ),
            format!(
                "field\t8\tsecret\t{}\t\t+{}\t1\t0\t70\t200\t24",
                t("Password"),
                t("hunter2")
            ),
            "scroll\t9\t40\t1\t1\t0\t100\t300\t200".to_owned(),
            "bogus\tline".to_owned(),
            "item\tnot-a-number\tx".to_owned(),
        ]
        .join("\n")
    }

    #[test]
    fn a_record_is_a_reading_with_ids_from_the_scripts_numbers_and_no_password() {
        let reading = parse(&record()).expect("a record");
        assert_eq!(reading.items.len(), 3);
        assert_eq!(reading.items[0].id, Some(Id::new(3)));
        assert_eq!(reading.items[0].role, "button");
        assert_eq!(reading.items[0].words, "Save it");
        assert!(reading.items[0].state.disabled);
        assert_eq!(reading.items[1].words, "Line one\nLine\ttwo");
        assert_eq!(reading.items[2].state.checked, Some(true));
        assert_eq!(reading.fields.len(), 2);
        assert_eq!(reading.fields[0].value.as_deref(), Some("api"));
        assert_eq!(reading.fields[0].label.as_deref(), Some("Search"));
        assert_eq!(reading.fields[0].hint.as_deref(), Some("type here"));
        assert_eq!(reading.fields[1].kind, FieldKind::Secret);
        assert_eq!(
            reading.fields[1].value, None,
            "a password has no value in a reading"
        );
        assert!(reading.fields[1].covered);
        assert_eq!(reading.scrolls.len(), 1);
        assert_eq!(reading.scrolls[0].percent, 40);
        assert_eq!(reading.focus, Focus::Field(Id::new(7)));
        let written = reading.text();
        assert!(!written.contains("hunter2"), "{written}");
        // The page's tab and newline are cleaned where the reading is written.
        assert!(written.contains("Line one"), "{written}");
        assert!(written.contains("note: this is a web view"), "{written}");
        // The ref an agent types names the script's number, as for a native page.
        assert_eq!(
            reading.find(&reference(Id::new(3))),
            pencere_drive::reading::Found::One(Id::new(3))
        );
    }

    #[test]
    fn a_page_is_at_rest_when_its_document_has_been_quiet_for_the_time_and_nothing_loads() {
        let rest = |quiet: u32, images: u32, loading: u32| {
            parse(&format!("{RECORD}\nrest\t{quiet}\t{images}\t{loading}"))
                .expect("a record")
                .rest
                .resting()
        };
        assert!(rest(QUIET_MS, 0, 0));
        assert!(rest(5000, 0, 0));
        assert!(
            !rest(QUIET_MS - 1, 0, 0),
            "the document changed a moment ago"
        );
        assert!(!rest(5000, 1, 0), "an image is loading");
        assert!(!rest(5000, 0, 1), "the document is loading");
    }

    #[test]
    fn what_is_not_a_record_is_refused_and_a_focus_that_is_no_field_is_the_page() {
        assert!(parse("").is_err());
        assert!(parse("v2\nrest\t1\t0\t0").is_err());
        assert!(parse("<html>").is_err());
        let reading = parse(&format!("{RECORD}\nfocus\t99")).expect("a record");
        assert_eq!(reading.focus, Focus::Page);
    }

    #[test]
    fn percent_escapes_are_utf8_and_what_is_not_an_escape_stays() {
        assert_eq!(decode("plain"), "plain");
        assert_eq!(decode("a%20b%0Ac"), "a b\nc");
        assert_eq!(decode(&enc("çay – 日本語 😀")), "çay – 日本語 😀");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz%4"), "%zz%4");
        assert_eq!(decode("%FF"), "\u{fffd}");
    }

    #[test]
    fn keys_are_the_events_the_page_has_for_them() {
        let keys = js_keys(&[
            "cmd+enter".to_owned(),
            "f5".to_owned(),
            "escape".to_owned(),
            "a".to_owned(),
            "shift+tab".to_owned(),
            "ctrl+shift+a".to_owned(),
            "1".to_owned(),
            "space".to_owned(),
        ])
        .expect("keys");
        assert_eq!(
            keys[0],
            JsKey {
                key: "Enter".into(),
                code: "Enter".into(),
                shift: false,
                control: false,
                alt: false,
                meta: true
            }
        );
        assert_eq!(keys[1].key, "F5");
        assert_eq!(keys[2].key, "Escape");
        assert_eq!((keys[3].key.as_str(), keys[3].code.as_str()), ("a", "KeyA"));
        assert!(keys[4].shift && keys[4].key == "Tab");
        assert!(keys[5].control && keys[5].shift && keys[5].key == "A");
        assert_eq!(
            (keys[6].key.as_str(), keys[6].code.as_str()),
            ("1", "Digit1")
        );
        assert_eq!(
            (keys[7].key.as_str(), keys[7].code.as_str()),
            (" ", "Space")
        );
        let bad = js_keys(&["a".to_owned(), "cmd+nonsense+".to_owned()]).expect_err("not a key");
        assert!(bad.text.contains("is not a key"), "{}", bad.text);
    }

    fn reading() -> Reading {
        parse(&record()).expect("a record")
    }

    #[test]
    fn an_action_is_planned_against_the_reading_and_says_what_it_did_in_the_native_words() {
        let reading = reading();
        let name = |n: u64| reference(Id::new(n));
        let click = plan(&Action::Click(name(3)), &reading).expect("planned");
        assert_eq!(
            click.act,
            Act::Click {
                number: 3,
                times: 1
            }
        );
        assert!(
            click
                .said
                .as_deref()
                .is_some_and(|said| said.starts_with("clicked button \"Save it\" ("))
        );
        let double = plan(&Action::DoubleClick(name(3)), &reading).expect("planned");
        assert_eq!(
            double.act,
            Act::Click {
                number: 3,
                times: 2
            }
        );
        let fill = plan(
            &Action::Fill {
                target: name(8),
                text: "s3cret!".into(),
            },
            &reading,
        )
        .expect("planned");
        assert_eq!(
            fill.act,
            Act::Fill {
                number: 8,
                text: "s3cret!".into()
            }
        );
        let said = fill.said.expect("said");
        assert!(
            said.contains("with 7 characters") && !said.contains("s3cret"),
            "{said}"
        );
        let typed = plan(&Action::Type("er".into()), &reading).expect("planned");
        assert_eq!(typed.act, Act::Type { text: "er".into() });
        assert!(
            typed
                .said
                .as_deref()
                .is_some_and(|said| said.starts_with("typed 2 characters into the field "))
        );
        let scroll = plan(
            &Action::Scroll {
                target: name(9),
                way: Way::Down,
                count: Some(2),
            },
            &reading,
        )
        .expect("planned");
        assert_eq!(
            scroll.act,
            Act::Scroll {
                number: 9,
                way: Way::Down,
                screens: 2
            }
        );
        assert!(
            scroll
                .said
                .as_deref()
                .is_some_and(|said| said.contains("down 2 screens"))
        );
        let press = plan(&Action::Press(vec!["cmd+enter".into()]), &reading).expect("planned");
        assert_eq!(press.said.as_deref(), Some("pressed cmd+enter"));
        for none in [
            Action::Read,
            Action::Shot { full: false },
            Action::Wait {
                text: "x".into(),
                gone: false,
                seconds: None,
            },
        ] {
            assert_eq!(plan(&none, &reading).expect("planned").act, Act::Nothing);
        }
    }

    #[test]
    fn what_a_web_view_does_not_have_and_what_does_not_fit_the_action_is_told() {
        let reading = reading();
        let name = |n: u64| reference(Id::new(n));
        for action in [
            Action::Command("x".into()),
            Action::Menu(name(3)),
            Action::Pick("1".into()),
            Action::Text(name(7)),
        ] {
            let failure = plan(&action, &reading).expect_err("not in a web view");
            assert!(
                failure.text.contains("a web view has no"),
                "{}",
                failure.text
            );
        }
        let not_field = plan(
            &Action::Fill {
                target: name(3),
                text: "x".into(),
            },
            &reading,
        )
        .expect_err("a button");
        assert!(
            not_field.text.contains("is not a field"),
            "{}",
            not_field.text
        );
        let not_box = plan(
            &Action::Scroll {
                target: name(3),
                way: Way::Up,
                count: None,
            },
            &reading,
        )
        .expect_err("a button");
        assert!(not_box.text.contains("does not scroll"), "{}", not_box.text);
        let none = plan(&Action::Click("zzzzzz".into()), &reading).expect_err("no such element");
        assert!(none.text.contains("no element"), "{}", none.text);
        // No keyboard in a field: nothing to type into.
        let mut page = reading.clone();
        page.focus = Focus::Page;
        let nowhere = plan(&Action::Type("x".into()), &page).expect_err("no field");
        assert!(
            nowhere.text.contains("no field of the page"),
            "{}",
            nowhere.text
        );
    }

    #[test]
    fn the_scripts_codes_are_the_mistakes_an_agent_is_told() {
        let what = "button \"Save\" (abc123)";
        assert!(
            failure("gone", "", what)
                .text
                .contains("not on the page any more")
        );
        assert!(failure("hidden", "", what).text.contains("not shown"));
        let covered = failure("covered", &enc("dialog \"Sure?\""), what);
        assert!(
            covered.text.contains("is covered by dialog \"Sure?\""),
            "{}",
            covered.text
        );
        assert!(
            failure("notfield", "", what)
                .text
                .contains("neither a text field")
        );
        let option = failure("nooption", &enc("Red|Green"), what);
        assert!(option.text.contains("it has: Red|Green"), "{}", option.text);
        assert!(
            failure("nofocus", "", what)
                .text
                .contains("no field of the page")
        );
        assert!(
            failure("strange", "", what)
                .text
                .contains("could not do it (strange)")
        );
    }
}
