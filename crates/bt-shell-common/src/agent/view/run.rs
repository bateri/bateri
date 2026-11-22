//! One `view` request carried out: begun (the action done), then asked again, by the app's clock,
//! until the page is at rest, and answered.
//!
//! The wait never blocks: [`Run::step`] looks at what there is and says [`Step::Pending`], and the
//! app asks again at its next tick (each look takes in what the page said since the last, and
//! lays out a page that is not seen). Four things are waited for ([`Rest`]): the
//! module has done what it was told, the calls the page made to its program are answered, the
//! images are decoded and the page is laid out for what it last was. Motion is told and not
//! waited for. [`REST_LIMIT`] ends the wait of a page that never comes to rest, and the answer
//! says what was still going on; that is not a mistake.

use pencere_drive::{FrameAt, Reading, ReadingInput};

use super::act::{Done, perform};
use super::{Env, Pane, Shown, menu_lines, written};
use crate::agent::message::{Failure, Format, Kind, Reply};
use crate::native_view::PageHandle;
use kablo_bateri::view::{Action, View, WAIT_DEFAULT};

/// How long, in seconds, an answer waits for a page to come to rest.
pub const REST_LIMIT: f64 = 2.0;

/// What a request asked the answer to wait for, and what it has been told.
#[derive(Debug)]
pub struct Run {
    view: View,
    done: Done,
    began: f64,
    /// How long the wait lasts, in seconds.
    limit: f64,
}

/// Whether the request is answered.
#[derive(Debug)]
pub enum Step {
    /// Not yet: ask again at the next tick of the app's clock.
    Pending,
    /// Now.
    Answer(Reply),
    /// The page is at rest (or the wait has run out) and the request is for a picture of the pane:
    /// the app draws it ([`super::picture_size`]) and answers with it. `seen` says whether the pane
    /// is on the screen: a pane that is is pictured as its last frame drew it, one that is not as
    /// the page is laid out now.
    Shot { full: bool, seen: bool },
}

/// The page of a pane as a request has it: its handle and the room it has.
pub struct Ctx<'a> {
    pub handle: &'a PageHandle,
    seen: bool,
    size: (u32, u32),
    scale: f64,
    reduce_motion: bool,
    pub now: f64,
}

impl<'a> Ctx<'a> {
    /// The page `shown` holds, or why a request cannot be carried out on what the pane shows.
    pub fn of(pane: Pane<'_>, shown: &'a Shown) -> Result<Ctx<'a>, Failure> {
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
            return Err(match shown {
                Shown::Terminal => Failure::new(
                    Kind::Failed,
                    format!(
                        "{name} shows a terminal, not a view; `bateri capture {name}` reads it"
                    ),
                ),
                Shown::NotUp => Failure::new(
                    Kind::Failed,
                    format!("the view of {name} is not up yet: its program has not shown it"),
                ),
                Shown::Web => Failure::new(
                    Kind::Failed,
                    format!(
                        "the view of {name} is a web view, which the app reads and drives itself, not through a page of the kit"
                    ),
                ),
                Shown::Page { .. } => unreachable!("matched above"),
            });
        };
        let Some(size) = size.filter(|(width, height)| *width > 0 && *height > 0) else {
            return Err(Failure::new(
                Kind::Failed,
                format!(
                    "the view of {name} has no room yet: it is not placed; ask again in a moment"
                ),
            ));
        };
        Ok(Ctx {
            handle,
            seen: *seen,
            size,
            scale: *scale,
            reduce_motion: *reduce_motion,
            now: *now,
        })
    }

    /// A pane that is not seen has no frames, so the page it shows stays as the last frame left it:
    /// laid out once now, for what a request needs of it, and nothing drawn.
    fn lay_out_unseen(&self) {
        if !self.seen {
            self.handle.lay_out_unseen(FrameAt {
                size: self.size,
                scale: self.scale,
                now: self.now,
                reduce_motion: self.reduce_motion,
            });
        }
    }

    /// What a page that is not seen still owes a frame for is laid out once: a pane that is seen
    /// is laid out by its own frames.
    fn settle(&self) {
        if self.seen {
            return;
        }
        // A page that was never laid out has had no room told it, and the module makes none until it
        // is: that is the first layout a cold page needs, before there is anything unsettled.
        let owed = {
            let mut page = self.handle.page();
            page.laid().is_none() || page.rest().unsettled
        };
        if owed {
            self.lay_out_unseen();
        }
    }

    fn reading(&self, env: &Env<'_>) -> Reading {
        let text = |id| env.fields.text(id);
        let (menu, held_field) = {
            let session = env.session.borrow();
            (session.menu.as_ref().map(menu_lines_of), session.field)
        };
        let focus = if self.handle.page().focused_editor().is_some() {
            None
        } else {
            env.fields.keyboard_in().or(held_field)
        };
        let input = ReadingInput {
            field_text: Some(&text),
            field_focus: focus,
            menu,
        };
        self.handle.page().reading(&input)
    }
}

fn menu_lines_of(menu: &crate::native_view::Menu) -> Vec<pencere_drive::reading::MenuLine> {
    menu_lines(menu)
        .into_iter()
        .map(|(_, title, enabled)| pencere_drive::reading::MenuLine { title, enabled })
        .collect()
}

impl Run {
    /// Does what `view` asks to the page the pane shows, and returns what is to be waited for.
    ///
    /// # Errors
    ///
    /// The pane shows no page to act on, or the action cannot be done (no such element, one that
    /// is covered, no key): what the agent is told.
    pub fn begin(
        pane: Pane<'_>,
        shown: &Shown,
        env: &Env<'_>,
        view: &View,
    ) -> Result<Run, Failure> {
        let ctx = Ctx::of(pane, shown)?;
        {
            let mut session = env.session.borrow_mut();
            // Only an action that drives the page has what the page does held for it: a read, a
            // wait and a picture put nothing in the page, so what the person does meanwhile is
            // theirs and goes where it goes.
            session.active = super::drives(&view.action);
            session.effects = super::Effects::default();
            // A menu that was held is let go of by any action but the pick from it.
            if !matches!(view.action, Action::Pick(_)) {
                session.menu = None;
            }
        }
        // What the page said since the pane last looked is taken first, and a page that is not seen
        // is laid out for what it says now: the reading the element is named by is of this page.
        (env.drain)();
        ctx.settle();
        let reading = ctx.reading(env);
        let done = match perform(&ctx, env, &reading, &view.action) {
            Ok(done) => done,
            Err(failure) => {
                env.session.borrow_mut().active = false;
                return Err(failure);
            }
        };
        let limit = match &view.action {
            Action::Wait { seconds, .. } => f64::from(seconds.unwrap_or(WAIT_DEFAULT)),
            _ => REST_LIMIT,
        };
        Ok(Run {
            view: view.clone(),
            done,
            began: ctx.now,
            limit,
        })
    }

    /// Looks at the page once more: the answer if the request is over, else [`Step::Pending`].
    pub fn step(&mut self, pane: Pane<'_>, shown: &Shown, env: &Env<'_>) -> Step {
        let ctx = match Ctx::of(pane, shown) {
            Ok(ctx) => ctx,
            Err(failure) => {
                env.session.borrow_mut().active = false;
                // A view that was there when the request began and is not now has ended.
                let text = if matches!(shown, Shown::Terminal) {
                    format!(
                        "the view of {} ended while the request was carried out",
                        pane.name
                    )
                } else {
                    failure.text
                };
                return Step::Answer(Reply::Failed(Failure::new(Kind::Failed, text)));
            }
        };
        if self.done.text.is_some() {
            return self.answer(pane, &ctx, env);
        }
        (env.drain)();
        ctx.settle();
        let reading = ctx.reading(env);
        let elapsed = ctx.now - self.began;
        match &self.view.action {
            Action::Wait { text, gone, .. } => {
                let shows = reading.text().contains(text.as_str());
                if shows != *gone {
                    return self.answer_with(pane, &reading, env);
                }
                if elapsed >= self.limit {
                    env.session.borrow_mut().active = false;
                    let what = if *gone { "to go" } else { "to show" };
                    return Step::Answer(Reply::Failed(Failure::new(
                        Kind::Failed,
                        format!(
                            "timed out after {} seconds waiting for \"{}\" {what} in the view of {}",
                            self.limit,
                            super::words(text, 60),
                            pane.name
                        ),
                    )));
                }
                Step::Pending
            }
            _ => {
                if reading.rest.resting() || elapsed >= self.limit {
                    if let Action::Shot { full } = &self.view.action {
                        let mut session = env.session.borrow_mut();
                        session.active = false;
                        session.effects = super::Effects::default();
                        return Step::Shot {
                            full: *full,
                            seen: ctx.seen,
                        };
                    }
                    self.answer_with(pane, &reading, env)
                } else {
                    Step::Pending
                }
            }
        }
    }

    fn answer(&mut self, pane: Pane<'_>, ctx: &Ctx<'_>, env: &Env<'_>) -> Step {
        let reading = ctx.reading(env);
        self.answer_with(pane, &reading, env)
    }

    fn answer_with(&mut self, pane: Pane<'_>, reading: &Reading, env: &Env<'_>) -> Step {
        let mut session = env.session.borrow_mut();
        session.active = false;
        if let Some(text) = &self.done.text {
            let mut text = text.clone();
            if !text.ends_with('\n') {
                text.push('\n');
            }
            return Step::Answer(Reply::Text(text));
        }
        let mut notes = std::mem::take(&mut session.effects).sentences();
        if self.done.asked_for_menu && session.menu.is_none() {
            notes.push("the page showed no menu there".to_owned());
        }
        let said = self.done.said.as_deref();
        let format: Format = self.view.format;
        Step::Answer(Reply::Text(written(pane, reading, format, said, &notes)))
    }
}
