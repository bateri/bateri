//! What an agent asks of a page's browsers (`bateri browser`): which browser it
//! means, the job the helper is given for it, and the answer that comes back to whoever waits.
//!
//! The helper does a job with the engine's DevTools protocol and says how it ended with the
//! request's number ([`browser_ipc::message::Event::AgentResult`]); the number is this page's
//! (a counter, from 1), and what waits for it is a thread of the app's that holds the conversation
//! ([`crate::agent`]), so that nothing here blocks the main thread.

use std::sync::mpsc::{self, Receiver};

use browser_ipc::message::{AgentOp, AgentOutput, Command};
use bt_shell_common::agent::browser::{Listed, element_name, element_number};
use bt_shell_common::agent::message::{Failure, Kind};

use super::{Id, PageBrowsers};

impl PageBrowsers {
    /// The page's browsers, as `ls` tells them: the one the person has the keyboard in is marked.
    pub(crate) fn listed(&self) -> Vec<Listed> {
        let keyboard = self.focused.get();
        self.rooms
            .borrow()
            .seen()
            .into_iter()
            .map(|seen| Listed {
                id: seen.id.number(),
                status: seen.status,
                url: seen.url,
                title: seen.title,
                keyboard: keyboard == Some(seen.id),
            })
            .collect()
    }

    /// The browser a request means: the one `element` names, or with none the page's only one, or
    /// of several the one that has the keyboard.
    pub(crate) fn pick(&self, element: Option<&str>) -> Result<Id, Failure> {
        let ids: Vec<Id> = self.rooms.borrow().ids().collect();
        if let Some(word) = element {
            let number = element_number(word)?;
            return ids
                .into_iter()
                .find(|id| id.number() == number)
                .ok_or_else(|| {
                    Failure::new(
                        Kind::Unknown,
                        format!("the pane has no browser {word}: `bateri browser ls` lists them"),
                    )
                });
        }
        match ids.as_slice() {
            [] => Err(Failure::new(Kind::Unknown, "the pane shows no browser")),
            [only] => Ok(*only),
            several => self
                .focused
                .get()
                .filter(|id| several.contains(id))
                .ok_or_else(|| {
                    let mut names: Vec<String> =
                        several.iter().map(|id| element_name(id.number())).collect();
                    names.sort();
                    Failure::new(
                        Kind::Usage,
                        format!(
                            "the pane shows {} browsers and the keyboard is in none: name one with --element ({})",
                            several.len(),
                            names.join(", ")
                        ),
                    )
                }),
        }
    }

    /// Gives the browser `id` the job `op`. The number the job has, and where its answer comes: a
    /// browser that is not open (starting, with no route, ended) cannot do one.
    pub(crate) fn job(&self, id: Id, op: AgentOp) -> Result<(u32, Receiver<AgentOutput>), Failure> {
        let req = self.next_job.get().wrapping_add(1).max(1);
        let sent = self
            .rooms
            .borrow()
            .send(id, |browser| Command::Agent { browser, req, op });
        if !sent {
            let why = self
                .rooms
                .borrow()
                .seen()
                .into_iter()
                .find(|seen| seen.id == id)
                .map(|seen| seen.status)
                .unwrap_or_default();
            return Err(Failure::new(
                Kind::Failed,
                format!(
                    "the browser is not open ({}): a reload makes it again",
                    if why.is_empty() { "gone" } else { &why }
                ),
            ));
        }
        self.next_job.set(req);
        let (answer, wait) = mpsc::channel();
        self.jobs.borrow_mut().insert(req, answer);
        Ok((req, wait))
    }

    /// The helper says how job `req` ended. A job nobody waits for any more (it timed out) is let go.
    pub(super) fn job_done(&self, req: u32, output: AgentOutput) {
        if let Some(answer) = self.jobs.borrow_mut().remove(&req) {
            let _ = answer.send(output);
        }
    }

    /// Nobody waits for job `req` any more.
    pub(crate) fn forget_job(&self, req: u32) {
        self.jobs.borrow_mut().remove(&req);
    }
}
