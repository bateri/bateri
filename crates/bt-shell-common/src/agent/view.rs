//! `bateri view` in the bateri app: an agent reads the view a pane shows and drives it. The kablo
//! command reads the words and prints what the app answers; the request is `kablo-bateri`'s
//! ([`kablo_bateri::view`]). What is decided here is what needs no window:
//!
//! - **May it?** ([`allowed`]) The pane of the machine that asked is read and driven at every
//!   level but `off`; a pane of another machine is reached ([`Act::Reach`]), so only `full` does. The
//!   level table is [`super::level`]'s and is not changed for the view.
//! - **What does the pane show?** ([`Shown`]) A page of the kit the pane built, a web view, a
//!   terminal, a view not up yet. Only a page is read here.
//! - **What does the answer say?** ([`written`]) What was done, what the host did not do, then a
//!   line that says which pane and which program, and the drive's own writing of the reading
//!   ([`pencere_drive::Reading`]), as text or as JSON.
//! - **How is a request carried out?** ([`Run`]) The action is done through the page's own inputs
//!   ([`act`]), what the page asks of its host meanwhile is held back ([`Held`]), and the answer
//!   waits for the page to come to rest without blocking.
//!
//! The reading is of a [`PageHandle`], the pane's handle on its page, and sees nothing of the
//! screen: the window's part is to find the pane, to give the texts of the host's fields, and to
//! lay the page out once for a pane that no frame is built for (it is on another tab, in a capsule
//! or in a window that is hidden).

use std::fmt::Write as _;

pub use kablo_bateri::view::{Action, View, WAIT_DEFAULT, Way};
use pencere_drive::Reading;

mod act;
mod held;
mod run;
pub mod web;

pub use held::{Effects, Held, Session, Shared};
pub use run::{Ctx, REST_LIMIT, Run, Step};

use super::level::{self, Act, Panes, Verdict};
use super::listing::{json_optional, json_string};
use super::message::{Failure, Format, Kind};
use super::serve::{Server, is_own_machine};
use crate::native_view::{Id, Menu, MenuEntry, PageHandle};

/// Whether the machine that asked may do `view` to its pane: a pane of its own machine is seen
/// (read) or changed (driven), whatever the level but `off`, and a pane of another is reached, which
/// only `full` is let do. What is refused says the level and the setting that changes it.
///
/// # Errors
///
/// A refusal ([`Kind::Refused`]) that names the level.
pub fn allowed(server: &Server, view: &View) -> Result<(), Failure> {
    let own = view
        .terminal
        .as_ref()
        .is_none_or(|address| is_own_machine(server, address));
    let (act, panes) = match (own, drives(&view.action)) {
        (true, false) => (Act::See, Panes::own(1)),
        (true, true) => (Act::Change, Panes::own(1)),
        (false, _) => (Act::Reach, Panes::other(1)),
    };
    match level::judge(server.level, act, panes) {
        Verdict::Allowed => Ok(()),
        Verdict::Refused(why) => Err(Failure::new(
            Kind::Refused,
            level::refusal(server.level, why, server.host_name()),
        )),
        Verdict::Unknown => Err(Failure::new(Kind::Unknown, "there is no such pane")),
    }
}

/// The longest edge, in pixels, of the picture `shot` gives unless `--full` asks for the pane's own
/// size: the size a card of the pane is shrunk to, and what an agent that reads pictures takes in
/// whole.
pub const SHOT_LONG_EDGE: u32 = 1280;

/// The size of the picture of a pane that is `source` pixels across: its own with `full`, else
/// brought down, in the pane's shape, until its longer edge is [`SHOT_LONG_EDGE`] at most. Never
/// larger than the pane, never an edge of no pixels.
#[must_use]
pub fn picture_size(source: (u32, u32), full: bool) -> (u32, u32) {
    let (width, height) = (source.0.max(1), source.1.max(1));
    let long = width.max(height);
    if full || long <= SHOT_LONG_EDGE {
        return (width, height);
    }
    let shrink = |edge: u32| {
        let scaled = u64::from(edge) * u64::from(SHOT_LONG_EDGE) + u64::from(long) / 2;
        u32::try_from(scaled / u64::from(long)).map_or(1, |edge| edge.max(1))
    };
    (shrink(width), shrink(height))
}

/// Whether the action does something to the view, and is not only a way of reading it: what the
/// level judges as a change.
pub fn drives(action: &Action) -> bool {
    !matches!(
        action,
        Action::Read | Action::Text(_) | Action::Wait { .. } | Action::Shot { .. }
    )
}

/// What the pane a `view` is about shows.
pub enum Shown {
    /// A page of the kit: `handle` is the pane's handle on it, `seen` whether the pane is on the
    /// screen (it has frames of its own then) and `size` the room it has, in pixels, at `scale`
    /// pixels to a point; none while the pane has not been given any.
    Page {
        handle: PageHandle,
        seen: bool,
        size: Option<(u32, u32)>,
        scale: f64,
        reduce_motion: bool,
        /// The frames' clock, now.
        now: f64,
    },
    /// A web view: answered by a later part of the verb.
    Web,
    /// A terminal, with no view: `bateri capture` reads it.
    Terminal,
    /// The program that makes the view has not shown it yet.
    NotUp,
}

/// The pane a reading is of: the name `ls` gives it and the program that runs in it.
#[derive(Clone, Copy, Debug)]
pub struct Pane<'a> {
    pub name: &'a str,
    pub program: Option<&'a str>,
}

/// What an agent's request needs of the host's own fields: the texts they hold, where the keyboard
/// is in them, and putting text in them. The window's (the page's fields are the system's controls
/// laid over it), so a trait: the pane implements it, and a test of the page does.
pub trait Fields {
    /// What the field `id` holds. Never asked of a secret field.
    fn text(&self, id: Id) -> Option<String>;
    /// Puts `text` in the field `id`, in place of what it holds.
    fn put(&self, id: Id, text: &str);
    /// Puts `text` after what the field `id` holds, and gives what it holds now, for the page to be
    /// told: the field may be a secret's, so the answer never says it. `None` for a field that is
    /// not on the screen.
    fn append(&self, id: Id, text: &str) -> Option<String>;
    /// The field the system's keyboard is in, where it is in one.
    fn keyboard_in(&self) -> Option<Id>;
}

/// What a request is carried out with: the host's fields, what an agent holds of the pane
/// ([`Session`]) and the pane's taking in of what the page said (`drain`: told through [`Held`]
/// while a request is carried out).
pub struct Env<'a> {
    pub fields: &'a dyn Fields,
    pub session: &'a Shared,
    pub drain: &'a dyn Fn(),
}

/// The reading as it is written to the agent: the head line (or, as JSON, the pane and the program
/// in front of what the reading says), then the drive's writing of it. What was done leads (`said`),
/// and what the host did not do (`notes`) follows it.
pub fn written(
    pane: Pane<'_>,
    reading: &Reading,
    format: Format,
    said: Option<&str>,
    notes: &[String],
) -> String {
    let program = pane.program.map(printable);
    let mut out = String::new();
    match format {
        Format::Text => {
            if let Some(said) = said {
                let _ = writeln!(out, "{}", words(said, 400));
            }
            for note in notes {
                let _ = writeln!(out, "{}", words(note, 400));
            }
            let _ = write!(out, "pane {}", printable(pane.name));
            if let Some(program) = &program {
                let _ = write!(out, " ({program})");
            }
            out.push('\n');
            out.push_str(&reading.text());
        }
        Format::Json => {
            let json = reading.json();
            let rest = json.strip_prefix('{').unwrap_or(&json);
            out.push_str("{\"pane\":");
            json_string(&mut out, &printable(pane.name));
            out.push_str(",\"program\":");
            json_optional(&mut out, program.as_deref());
            if let Some(said) = said {
                out.push_str(",\"did\":");
                json_string(&mut out, &words(said, 400));
            }
            if !notes.is_empty() {
                out.push_str(",\"notes\":[");
                for (at, note) in notes.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    json_string(&mut out, &words(note, 400));
                }
                out.push(']');
            }
            if rest.trim_start() != "}" {
                out.push(',');
            }
            out.push_str(rest);
        }
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// The lines of a menu as an agent counts them: where each is among the menu's entries (a separator
/// is counted there and is no line), its title and whether it is on.
pub fn menu_lines(menu: &Menu) -> Vec<(usize, String, bool)> {
    menu.entries
        .iter()
        .enumerate()
        .filter_map(|(at, entry)| match entry {
            MenuEntry::Item { title, enabled, .. } | MenuEntry::Paste { title, enabled, .. } => {
                Some((at, title.clone(), *enabled))
            }
            MenuEntry::Separator => None,
        })
        .collect()
}

/// Words the page or the program wrote, as one line of the answer: what is not printable is a
/// space, and what is past `limit` characters is cut.
fn words(text: &str, limit: usize) -> String {
    let mut line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(limit)
        .collect();
    if text.chars().count() > limit {
        line.push('…');
    }
    line
}

/// A name or a program as one line of plain text: what is not printable is `?`.
fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_core::CommandLevel;
    use kablo_bateri::Anchor;

    fn view(terminal: Option<&str>, action: Action) -> View {
        View {
            terminal: terminal.and_then(Anchor::address),
            format: Format::Text,
            action,
        }
    }

    fn here(level: CommandLevel) -> Server {
        Server {
            account: "ada@rasp:22".into(),
            host: "rasp".into(),
            argv: vec!["ssh".into(), "rasp".into()],
            level,
        }
    }

    fn this_computer(level: CommandLevel) -> Server {
        Server {
            account: crate::kablo::local::ACCOUNT.into(),
            host: String::new(),
            argv: Vec::new(),
            level,
        }
    }

    #[test]
    fn a_pane_of_the_machine_that_asked_is_read_and_driven_at_every_level_but_off() {
        for level in [CommandLevel::Own, CommandLevel::Screen, CommandLevel::Full] {
            for server in [here(level), this_computer(level)] {
                for (terminal, action) in [
                    (None, Action::Read),
                    (Some("p4"), Action::Read),
                    (Some("p4"), Action::Click("k3f9qa".into())),
                    (Some("p4"), Action::Shot { full: false }),
                ] {
                    assert_eq!(
                        allowed(&server, &view(terminal, action.clone())),
                        Ok(()),
                        "{level:?} {terminal:?} {action:?}"
                    );
                }
            }
        }
        // The machine's own name, written out, is no other machine.
        assert_eq!(
            allowed(
                &here(CommandLevel::Own),
                &view(Some("p4@rasp"), Action::Read)
            ),
            Ok(())
        );
        assert_eq!(
            allowed(
                &this_computer(CommandLevel::Own),
                &view(Some("p4@local"), Action::Read)
            ),
            Ok(())
        );
        assert!(allowed(&here(CommandLevel::Off), &view(None, Action::Read)).is_err());
    }

    #[test]
    fn a_pane_of_another_machine_is_reached_and_only_full_may() {
        for terminal in ["p9@prod", "p9@local"] {
            for action in [Action::Read, Action::Click("k3f9qa".into())] {
                for level in [CommandLevel::Own, CommandLevel::Screen] {
                    let refused = allowed(&here(level), &view(Some(terminal), action.clone()))
                        .expect_err("refused");
                    assert_eq!(refused.kind, Kind::Refused, "{level:?} {terminal}");
                    assert!(refused.text.contains("full"), "{}", refused.text);
                }
                assert_eq!(
                    allowed(
                        &here(CommandLevel::Full),
                        &view(Some(terminal), action.clone())
                    ),
                    Ok(())
                );
            }
        }
        // For a program on this computer, another machine is a server.
        let refused = allowed(
            &this_computer(CommandLevel::Screen),
            &view(Some("p9@prod"), Action::Read),
        )
        .expect_err("refused");
        assert_eq!(refused.kind, Kind::Refused);
        assert_eq!(
            allowed(
                &this_computer(CommandLevel::Full),
                &view(Some("p9@prod"), Action::Read)
            ),
            Ok(())
        );
    }

    #[test]
    fn only_what_does_something_to_the_view_is_a_change() {
        assert!(!drives(&Action::Read));
        assert!(!drives(&Action::Text("k3f9qa".into())));
        assert!(!drives(&Action::Shot { full: true }));
        assert!(!drives(&Action::Wait {
            text: "Done".into(),
            gone: false,
            seconds: None
        }));
        assert!(drives(&Action::Click("k3f9qa".into())));
        assert!(drives(&Action::Menu("k3f9qa".into())));
        assert!(drives(&Action::Type("a".into())));
        // A command an element lists is judged as a click is; bringing a row into view is a scroll.
        assert!(drives(&Action::CommandOn {
            target: "k3f9qa".into(),
            title: "Kill".into()
        }));
        assert!(drives(&Action::Reveal {
            target: "k3f9qa".into(),
            index: Some(3)
        }));
    }

    #[test]
    fn the_head_names_the_pane_and_the_program_and_the_json_leads_with_them() {
        let pane = Pane {
            name: "p9",
            program: Some("docker\u{1b}pen"),
        };
        let reading = Reading::default();
        let text = written(pane, &reading, Format::Text, None, &[]);
        assert!(
            text.starts_with("pane p9 (docker?pen)\nfocus: page\n"),
            "{text:?}"
        );
        assert!(text.ends_with('\n'));
        let json = written(pane, &reading, Format::Json, None, &[]);
        assert!(
            json.starts_with("{\"pane\":\"p9\",\"program\":\"docker?pen\","),
            "{json:?}"
        );
        assert!(json.contains("\"focus\":{\"kind\":\"page\"}"), "{json:?}");
        assert!(json.ends_with("}\n"));
        let none = written(
            Pane {
                name: "p9",
                program: None,
            },
            &reading,
            Format::Json,
            None,
            &[],
        );
        assert!(
            none.starts_with("{\"pane\":\"p9\",\"program\":null,"),
            "{none:?}"
        );
    }

    #[test]
    fn what_was_done_and_what_the_host_did_not_do_lead_the_answer() {
        let pane = Pane {
            name: "p9",
            program: None,
        };
        let reading = Reading::default();
        let notes = vec!["link not opened: https://example.com".to_owned()];
        let text = written(
            pane,
            &reading,
            Format::Text,
            Some("clicked button \"Save\" (m29gqw)"),
            &notes,
        );
        assert!(
            text.starts_with(
                "clicked button \"Save\" (m29gqw)\nlink not opened: https://example.com\npane p9\n"
            ),
            "{text:?}"
        );
        let json = written(pane, &reading, Format::Json, Some("clicked"), &notes);
        assert!(
            json.starts_with(
                "{\"pane\":\"p9\",\"program\":null,\"did\":\"clicked\",\"notes\":[\"link not opened: https://example.com\"],"
            ),
            "{json:?}"
        );
        // What the page wrote is one line: nothing in it ends the line or moves the cursor.
        let hostile = written(
            pane,
            &reading,
            Format::Text,
            Some("clicked\nbutton \u{1b}[2J"),
            &[],
        );
        assert!(
            hostile.starts_with("clicked button  [2J\npane p9"),
            "{hostile:?}"
        );
    }

    /// What an agent does reaches the page through the page's own inputs and what the page asks of
    /// its host is held back: nothing in the actions or the held host names the window's, the
    /// pasteboard, a panel or a terminal, and the held host passes nothing the person's to the
    /// pane's own host.
    #[test]
    fn what_an_agent_does_reaches_neither_the_window_nor_the_pasteboard_nor_a_terminal() {
        let sources = [
            include_str!("view/act.rs"),
            include_str!("view/held.rs"),
            include_str!("view/run.rs"),
        ];
        for source in sources {
            let source = &source[..source.find("#[cfg(test)]").unwrap_or(source.len())];
            // The words of the comments are left out: they say what is not done.
            let code: String = source
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            for name in [
                "makeFirstResponder",
                "NSPasteboard",
                "clipboard",
                "show_page_menu",
                "ask_panel",
                "module_link",
                "module_message",
                "Pty",
                "write_to_pty",
                "send_input",
            ] {
                assert!(!code.contains(name), "{name} in\n{code}");
            }
        }
        let held = include_str!("view/held.rs");
        let held = &held[..held.find("#[cfg(test)]").unwrap_or(held.len())];
        for method in [
            "fn keyboard",
            "fn copy",
            "fn link",
            "fn panel",
            "fn menu",
            "fn paste",
            "fn cursor",
        ] {
            let start = held.find(method).expect(method);
            let rest = &held[start + method.len()..];
            let end = [rest.find("\n    }\n"), rest.find("\n    fn ")]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(rest.len());
            let body = &held[start..start + method.len() + end];
            assert!(
                !body.contains("self.base"),
                "{method} reaches the pane: {body}"
            );
        }
    }

    /// A picture is the pane's own size with `--full`, else brought down in the pane's shape to a
    /// long edge of 1280 pixels; it is never larger than the pane and never has an edge of none.
    #[test]
    fn a_picture_is_the_panes_size_or_brought_down_in_its_shape_to_a_long_edge() {
        assert_eq!(picture_size((2880, 1800), true), (2880, 1800));
        assert_eq!(picture_size((2880, 1800), false), (1280, 800));
        assert_eq!(picture_size((1800, 2880), false), (800, 1280));
        assert_eq!(picture_size((1280, 720), false), (1280, 720));
        assert_eq!(
            picture_size((640, 400), false),
            (640, 400),
            "never larger than the pane"
        );
        assert_eq!(
            picture_size((10_000, 3), false),
            (1280, 1),
            "never an edge of none"
        );
        assert_eq!(picture_size((0, 0), false), (1, 1));
        assert_eq!(picture_size((u32::MAX, u32::MAX), false), (1280, 1280));
    }
}
