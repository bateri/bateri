//! `bateri view` in the bateri app: an agent reads the view a pane shows and drives it. The kablo
//! command reads the words and prints what the app answers; the request is `kablo-bateri`'s
//! ([`kablo_bateri::view`]). What is decided here is what needs no window:
//!
//! - **May it?** ([`allowed`]) The pane of the machine that asked is read and driven at every
//!   level but `off`; a pane of another machine is reached ([`Act::Reach`]), so only `full` does. The
//!   level table is [`super::level`]'s and is not changed for the view.
//! - **What does the pane show?** ([`Shown`]) A page of the kit the pane built, a web view, a
//!   terminal, a view not up yet. Only a page is read here.
//! - **What does the reading say?** ([`read`]) A line that says which pane and which program, then
//!   the drive's own writing of the reading ([`pencere_drive::Reading`]), as text or as JSON.
//!
//! The reading is of a [`PageHandle`], the pane's handle on its page, and sees nothing of the
//! screen: the window's part is to find the pane, to give the texts of the host's fields, and to
//! lay the page out once for a pane that no frame is built for (it is on another tab, in a capsule
//! or in a window that is hidden).

use std::fmt::Write as _;

pub use kablo_bateri::view::{Action, View};
use pencere_drive::{FrameAt, Reading, ReadingInput};

use super::level::{self, Act, Panes, Verdict};
use super::listing::{json_optional, json_string};
use super::message::{Failure, Format, Kind, Reply};
use super::serve::{Server, is_own_machine};
use crate::native_view::PageHandle;

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

/// Whether the action does something to the view, and is not only a way of reading it: what the
/// level judges as a change.
pub fn drives(action: &Action) -> bool {
    !matches!(action, Action::Read | Action::Text(_) | Action::Shot { .. })
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

/// The answer to a `view` of `pane`, which shows `shown`: `input` is what only the window has, the
/// texts of the host's fields and where its keyboard is ([`ReadingInput`]).
pub fn answer(pane: Pane<'_>, shown: &Shown, view: &View, input: &ReadingInput<'_>) -> Reply {
    let name = pane.name;
    let Shown::Page {
        handle,
        seen,
        size,
        scale,
        reduce_motion,
        now,
    } = shown
    else {
        return Reply::Failed(match shown {
            Shown::Terminal => Failure::new(
                Kind::Failed,
                format!("{name} shows a terminal, not a view; `bateri capture {name}` reads it"),
            ),
            Shown::NotUp => Failure::new(
                Kind::Failed,
                format!("the view of {name} is not up yet: its program has not shown it"),
            ),
            Shown::Web => Failure::new(
                Kind::Failed,
                format!("the view of {name} is a web view, which `bateri view` does not read yet"),
            ),
            Shown::Page { .. } => unreachable!("matched above"),
        });
    };
    if !matches!(view.action, Action::Read) {
        return Reply::Failed(Failure::new(
            Kind::Failed,
            "this bateri reads a view and does not drive it yet",
        ));
    }
    let Some(size) = size.filter(|(width, height)| *width > 0 && *height > 0) else {
        return Reply::Failed(Failure::new(
            Kind::Failed,
            format!("the view of {name} has no room yet: it is not placed; ask again in a moment"),
        ));
    };
    // A pane that is seen has its frames; one that is not is laid out once, here, at its size.
    if !seen {
        handle.lay_out_unseen(FrameAt {
            size,
            scale: *scale,
            now: *now,
            reduce_motion: *reduce_motion,
        });
    }
    let reading = handle.page().reading(input);
    Reply::Text(written(pane, &reading, view.format))
}

/// The reading as it is written to the agent: the head line (or, as JSON, the pane and the program
/// in front of what the reading says), then the drive's writing of it.
pub fn written(pane: Pane<'_>, reading: &Reading, format: Format) -> String {
    let program = pane.program.map(printable);
    let mut out = String::new();
    match format {
        Format::Text => {
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
        assert!(drives(&Action::Click("k3f9qa".into())));
        assert!(drives(&Action::Menu("k3f9qa".into())));
        assert!(drives(&Action::Type("a".into())));
    }

    #[test]
    fn the_head_names_the_pane_and_the_program_and_the_json_leads_with_them() {
        let pane = Pane {
            name: "p9",
            program: Some("docker\u{1b}pen"),
        };
        let reading = Reading::default();
        let text = written(pane, &reading, Format::Text);
        assert!(
            text.starts_with("pane p9 (docker?pen)\nfocus: page\n"),
            "{text:?}"
        );
        assert!(text.ends_with('\n'));
        let json = written(pane, &reading, Format::Json);
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
        );
        assert!(
            none.starts_with("{\"pane\":\"p9\",\"program\":null,"),
            "{none:?}"
        );
    }
}
