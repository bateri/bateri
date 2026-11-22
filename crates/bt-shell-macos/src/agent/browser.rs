//! `bateri browser`: the app finds the pane a request names, judges the level
//! for the browser's machine ([`browser::judge`]) and does what is asked of the pane's browsers,
//! which are the helper's, on this computer. The pane's page is where they are; the pane is the
//! one of the terminal the request names, so a program on a server asks for the browsers of its
//! own terminals the way `capture` does.
//!
//! What a verb does: `ls` is told from the page's rooms and answered at once; `open`, `back` and
//! `reload` are asks of the page's browser, as the page itself makes, and answered `ok`; `text`,
//! `snapshot`, `shot`, `console`, `click` and `type` are jobs of the helper, and so is `clear`
//! (the same the page's own ask for it is, which also tells a profile's home), which a thread of
//! the app waits for ([`WAIT`]) and answers, so that the main thread never does: an `ok` for a
//! `clear` is a cache or a site that was emptied, not asked to be. A text or a picture that does
//! not fit a reply is written as a file on the machine that asked ([`browser::file_on`]).

use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::time::Duration;

use browser_ipc::message::{AgentOp, AgentOutput, Target};
use bt_shell_common::agent::browser::{self, Action, Aim, Browser, Clearing, FileKind, Said};
use bt_shell_common::agent::message::{Failure, Kind, Reply};
use bt_shell_common::agent::serve::Server;
use bt_shell_common::browser::rooms::Clearing as Emptied;
use bt_shell_common::kablo::install::Channel;
use bt_shell_common::kablo::service::Letter;
use bt_shell_common::native_view::{BrowserAsk, go_refusal};

use super::reach::Machine;
use super::{AppDelegate, find_address};
use crate::native_browser::on_main;

/// How long the helper may take over a job before the request is answered as failed: a page may be
/// slow to give its tree, and a picture is large, but a browser that says nothing is not waited for.
const WAIT: Duration = Duration::from_secs(30);

fn failed(letter: Letter, failure: Failure) {
    letter.answer(Reply::Failed(failure).encode().into_bytes());
}

fn said(letter: Letter, reply: Reply) {
    letter.answer(reply.encode().into_bytes());
}

fn aim_of(text: &str) -> Result<Target, Failure> {
    Ok(match browser::aim(text)? {
        Aim::Node(id) => Target::Node { id },
        Aim::Point(x, y) => Target::Point { x, y },
    })
}

/// What the helper is asked, for a verb that is a job of its own; `None` for the others.
fn job_of(action: &Action) -> Result<Option<AgentOp>, Failure> {
    Ok(Some(match action {
        Action::Text { .. } => AgentOp::Text,
        Action::Snapshot { .. } => AgentOp::Snapshot,
        Action::Shot { .. } => AgentOp::Shot,
        Action::Console { .. } => AgentOp::Console,
        Action::Click { target } => AgentOp::Click {
            target: aim_of(target)?,
        },
        Action::Type { text, into } => {
            // A line break at the end is the key that ends the line.
            let (text, enter) = match text.strip_suffix('\n') {
                Some(text) => (text.to_owned(), true),
                None => (text.clone(), false),
            };
            AgentOp::Type {
                text,
                target: into.as_deref().map(aim_of).transpose()?,
                enter,
            }
        }
        Action::Ls | Action::Open { .. } | Action::Back | Action::Reload | Action::Clear { .. } => {
            return Ok(None);
        }
    }))
}

/// A request of `bateri browser` from a terminal of `server`.
pub(super) fn browse(app: &AppDelegate, letter: Letter, server: &Server, request: Browser) {
    let (pane, shown) = match find_address(app, server, &request.pane) {
        Ok(Some((_, pane, shown))) => (pane, shown),
        Ok(None) => {
            return failed(
                letter,
                Failure::new(
                    Kind::Unknown,
                    format!("{} is no pane on screen", request.pane),
                ),
            );
        }
        Err(failure) => return failed(letter, failure),
    };
    if let Err(failure) = browser::judge(
        server.level,
        &request.action,
        shown.account == server.account,
        server.host_name(),
        &request.pane.to_string(),
    ) {
        return failed(letter, failure);
    }
    let Some(browsers) = pane.page_browsers() else {
        // A pane whose program shows no page has no browsers: `ls` says none, the rest cannot.
        return if matches!(request.action, Action::Ls) {
            said(letter, Reply::Text(String::new()))
        } else {
            failed(
                letter,
                Failure::new(Kind::Unknown, format!("{} shows no browser", request.pane)),
            )
        };
    };
    if matches!(request.action, Action::Ls) {
        return said(letter, Reply::Text(browser::listing(&browsers.listed())));
    }
    let id = match browsers.pick(request.element.as_deref()) {
        Ok(id) => id,
        Err(failure) => return failed(letter, failure),
    };
    let ask = match &request.action {
        Action::Open { url } => match go_refusal(url) {
            None => Some(BrowserAsk::Go(url.clone())),
            Some(why) => {
                return failed(
                    letter,
                    Failure::new(
                        Kind::Usage,
                        format!("a browser does not go to `{url}` ({})", why.word()),
                    ),
                );
            }
        },
        Action::Back => Some(BrowserAsk::Back),
        Action::Reload => Some(BrowserAsk::Reload { cache: true }),
        _ => None,
    };
    if let Some(ask) = ask {
        browsers.ask_of(id, ask);
        return said(letter, Reply::Done);
    }
    // What is emptied is one of the site the browser is at: not a job of the engine's protocol, but
    // the page's own ask, which also tells a profile's home, and waits for the answer as a job does.
    let job = if let Action::Clear { what } = &request.action {
        browsers.clear(
            id,
            match what {
                Clearing::Cache => Emptied::Cache,
                Clearing::Site => Emptied::Site,
            },
        )
    } else {
        match job_of(&request.action) {
            Ok(Some(op)) => browsers.job(id, op),
            Ok(None) => return failed(letter, Failure::new(Kind::Failed, "nothing to do")),
            Err(failure) => return failed(letter, failure),
        }
    };
    let (req, wait) = match job {
        Ok(job) => job,
        Err(failure) => return failed(letter, failure),
    };
    let machine = Machine::of(server);
    let (pane_id, serial) = (pane.id(), browsers.serial());
    let action = request.action;
    let started = thread::Builder::new()
        .name("bateri browser".into())
        .spawn(move || {
            let reply = match wait.recv_timeout(WAIT) {
                Ok(output) => finish(&machine, &action, output),
                Err(RecvTimeoutError::Timeout) => {
                    on_main(pane_id, move |pane| {
                        if let Some(browsers) =
                            pane.page_browsers().filter(|b| b.serial() == serial)
                        {
                            browsers.forget_job(req);
                        }
                    });
                    Reply::Failed(Failure::new(
                        Kind::Failed,
                        "the browser did not answer in time",
                    ))
                }
                Err(RecvTimeoutError::Disconnected) => Reply::Failed(Failure::new(
                    Kind::Failed,
                    "the pane's page ended before the browser answered",
                )),
            };
            said(letter, reply);
        });
    if let Err(error) = started {
        eprintln!("bateri: no thread to wait for a browser: {error}");
    }
}

/// The reply to a request, from what the helper said of its job. A text or a picture that is a file
/// is written on the machine of the one who asked.
fn finish(machine: &dyn Channel, action: &Action, output: AgentOutput) -> Reply {
    let failure = |text: &str| Reply::Failed(Failure::new(Kind::Failed, text));
    let filed = |part: &str, kind: FileKind, bytes: &[u8]| {
        browser::file_on(machine, part, kind, bytes).map_or_else(Reply::Failed, Reply::Filed)
    };
    match (action, output) {
        (_, AgentOutput::Failed { message }) => Reply::Failed(Failure::new(Kind::Failed, message)),
        (Action::Click { .. } | Action::Type { .. } | Action::Clear { .. }, AgentOutput::Done) => {
            Reply::Done
        }
        (
            Action::Text { part } | Action::Snapshot { part } | Action::Console { part },
            AgentOutput::Text { text, truncated },
        ) => match browser::said(text, truncated) {
            Said::Reply(text) => Reply::Text(text),
            Said::File(bytes) => filed(part, FileKind::Text, &bytes),
        },
        (Action::Shot { part }, AgentOutput::Png { data }) => match browser::unbase64(&data) {
            Some(bytes) if !bytes.is_empty() => filed(part, FileKind::Png, &bytes),
            _ => failure("the page gave no picture"),
        },
        _ => failure("the browser answered with something else than was asked"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_shell_common::kablo::install::Reply as Ran;
    use std::cell::RefCell;
    use std::io;

    /// A machine that keeps what it was given and says how the script ended.
    struct Machine {
        status: i32,
        kept: RefCell<Vec<(String, Vec<u8>)>>,
    }

    impl Machine {
        fn ending(status: i32) -> Machine {
            Machine {
                status,
                kept: RefCell::new(Vec::new()),
            }
        }
    }

    impl Channel for Machine {
        fn run(&self, script: &str, input: &[u8]) -> io::Result<Ran> {
            self.kept
                .borrow_mut()
                .push((script.to_owned(), input.to_vec()));
            Ok(Ran {
                status: Some(self.status),
                out: String::new(),
                err: String::new(),
            })
        }
    }

    fn action(word: &str) -> Action {
        let part = "p1".to_owned();
        match word {
            "text" => Action::Text { part },
            "snapshot" => Action::Snapshot { part },
            "shot" => Action::Shot { part },
            "click" => Action::Click {
                target: "[7]".to_owned(),
            },
            "console" => Action::Console { part },
            "clear" => Action::Clear {
                what: Clearing::Site,
            },
            _ => panic!("{word}"),
        }
    }

    #[test]
    fn a_verb_is_the_job_the_helper_is_given_and_a_point_or_a_node_is_the_target() {
        assert_eq!(job_of(&action("text")).unwrap(), Some(AgentOp::Text));
        assert_eq!(
            job_of(&action("snapshot")).unwrap(),
            Some(AgentOp::Snapshot)
        );
        assert_eq!(job_of(&action("shot")).unwrap(), Some(AgentOp::Shot));
        assert_eq!(job_of(&action("console")).unwrap(), Some(AgentOp::Console));
        assert_eq!(
            job_of(&action("click")).unwrap(),
            Some(AgentOp::Click {
                target: Target::Node { id: 7 }
            })
        );
        let point = Action::Click {
            target: "412,88".to_owned(),
        };
        assert_eq!(
            job_of(&point).unwrap(),
            Some(AgentOp::Click {
                target: Target::Point { x: 412.0, y: 88.0 }
            })
        );
        // What is no job of the helper's is the page's own ask, or the page's list, or the clearing
        // that the rooms make of the site the browser is at (`PageBrowsers::clear`).
        for none in [
            Action::Ls,
            Action::Back,
            Action::Reload,
            action("clear"),
            Action::Open {
                url: "https://a/".to_owned(),
            },
        ] {
            assert_eq!(job_of(&none).unwrap(), None, "{}", none.word());
        }
        let wrong = Action::Click {
            target: "the button".to_owned(),
        };
        assert_eq!(job_of(&wrong).unwrap_err().kind, Kind::Usage);
    }

    #[test]
    fn what_is_typed_ends_with_the_key_when_it_ends_with_a_line_break() {
        let typed = |text: &str, into: Option<&str>| {
            job_of(&Action::Type {
                text: text.to_owned(),
                into: into.map(str::to_owned),
            })
            .unwrap()
            .unwrap()
        };
        assert_eq!(
            typed("hi there\n", Some("[12]")),
            AgentOp::Type {
                text: "hi there".to_owned(),
                target: Some(Target::Node { id: 12 }),
                enter: true
            }
        );
        assert_eq!(
            typed("a\nb", None),
            AgentOp::Type {
                text: "a\nb".to_owned(),
                target: None,
                enter: false
            }
        );
    }

    #[test]
    fn the_helpers_word_is_the_reply_and_a_long_text_or_a_picture_is_a_file() {
        let machine = Machine::ending(0);
        let text = |text: &str| AgentOutput::Text {
            text: text.to_owned(),
            truncated: false,
        };
        assert_eq!(
            finish(&machine, &action("text"), text("hello\n")),
            Reply::Text("hello\n".to_owned())
        );
        assert_eq!(
            finish(&machine, &action("click"), AgentOutput::Done),
            Reply::Done
        );
        // A clearing that was done is `ok`; one that was not says why, as every job does.
        assert_eq!(
            finish(&machine, &action("clear"), AgentOutput::Done),
            Reply::Done
        );
        assert_eq!(
            finish(
                &machine,
                &action("clear"),
                AgentOutput::Failed {
                    message: "the cache is in use".to_owned()
                }
            ),
            Reply::Failed(Failure::new(Kind::Failed, "the cache is in use"))
        );
        // The console is a text, as a page's is.
        assert_eq!(
            finish(&machine, &action("console"), text("[error] boom\n")),
            Reply::Text("[error] boom\n".to_owned())
        );
        assert!(
            machine.kept.borrow().is_empty(),
            "nothing written for these"
        );
        // A text over a reply is a file, written as the part says.
        let long = "x".repeat(browser::TEXT_MAX + 1);
        let Reply::Filed(filed) = finish(&machine, &action("snapshot"), text(&long)) else {
            panic!("a file");
        };
        assert_eq!(filed.kind, FileKind::Text);
        assert_eq!(filed.bytes as usize, long.len());
        // A picture is always one.
        let Reply::Filed(filed) = finish(
            &machine,
            &action("shot"),
            AgentOutput::Png {
                data: "iVBORw0KGgo=".to_owned(),
            },
        ) else {
            panic!("a file");
        };
        assert_eq!((filed.kind, filed.bytes), (FileKind::Png, 8));
        let kept = machine.kept.borrow();
        assert_eq!(kept.len(), 2);
        assert!(kept[0].0.contains("p1.txt"), "{}", kept[0].0);
        assert!(kept[1].0.contains("p1.png"), "{}", kept[1].0);
        assert_eq!(&kept[1].1[1..4], b"PNG");
    }

    #[test]
    fn a_failure_of_the_helper_or_a_wrong_kind_of_answer_or_a_file_that_cannot_be_written_is_failed()
     {
        let machine = Machine::ending(0);
        let failed_with = |reply: Reply| match reply {
            Reply::Failed(failure) => {
                assert_eq!(failure.kind, Kind::Failed);
                failure.text
            }
            other => panic!("not failed: {other:?}"),
        };
        assert_eq!(
            failed_with(finish(
                &machine,
                &action("click"),
                AgentOutput::Failed {
                    message: "the node has no box on the page".to_owned()
                }
            )),
            "the node has no box on the page"
        );
        assert!(
            failed_with(finish(&machine, &action("text"), AgentOutput::Done))
                .contains("something else")
        );
        assert!(
            failed_with(finish(
                &machine,
                &action("shot"),
                AgentOutput::Png {
                    data: String::new()
                }
            ))
            .contains("no picture")
        );
        assert!(
            failed_with(finish(
                &machine,
                &action("shot"),
                AgentOutput::Png {
                    data: "not base64!".to_owned()
                }
            ))
            .contains("no picture")
        );
        let unwritable = Machine::ending(9);
        let long = AgentOutput::Text {
            text: "x".repeat(browser::TEXT_MAX + 1),
            truncated: false,
        };
        assert!(failed_with(finish(&unwritable, &action("text"), long)).contains("folder"));
    }
}
