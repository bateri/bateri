//! What an agent's doing to a page leaves in the pane, and what the pane does not do while it is
//! done.
//!
//! An agent's request is a person's act for the page, but the person is not there: a copy is not
//! written to the pasteboard that is theirs, a link is not opened, a panel is not shown (the page
//! hears the person turned it away), and a menu is not popped up (a pop-up holds the main thread
//! until somebody picks from it): it is held, and the agent reads it and picks from it. The
//! keyboard of the system stays where it is: the page's own idea of where the keyboard is moves,
//! and the pane's reading says it. [`Held`] is that: the host the page's replies are told to while
//! a request is carried out, around the pane's own host, which it passes to what an agent does not
//! hold back.
//!
//! A vibration and a sound are not played for an agent either: the person is not there to feel or
//! hear them, and an agent that presses button after button would make the speaker buzz. The
//! agent is told what the page asked for instead ([`Effects::notes`]), so that it sees the sound
//! and the vibration come at the right moment.

use std::cell::RefCell;
use std::rc::Rc;

use crate::native_view::haptic::{self, Haptic, Pattern};
use crate::native_view::sound::{Ask as SoundAsk, Source};
use crate::native_view::{
    CarryOut, Cursor, Fault, FileSource, Fill, Host, Id, Keyboard, Menu, PageHandle, Panel, Point,
};

/// What the page asked of its host during a request that the host did not do, to be told to the
/// agent with the answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    /// Text the page copied: not written to the pasteboard.
    pub copies: Vec<String>,
    /// Links the page asked to open: not opened.
    pub links: Vec<String>,
    /// How many panels it asked for: each was turned away.
    pub panels: usize,
    /// Anything else the request did not do, a sentence each.
    pub notes: Vec<String>,
}

impl Effects {
    /// The sentences that tell the agent what was not done.
    pub fn sentences(&self) -> Vec<String> {
        let mut said = Vec::new();
        for text in &self.copies {
            said.push(format!(
                "copied, not written to the pasteboard: \"{}\"",
                super::words(text, 200)
            ));
        }
        for url in &self.links {
            said.push(format!("link not opened: {}", super::words(url, 300)));
        }
        if self.panels > 0 {
            said.push("a file panel was asked for and turned away".to_owned());
        }
        said.extend(self.notes.iter().cloned());
        said
    }
}

/// What an agent has with a pane's page between its requests and while one is carried out.
#[derive(Debug, Default)]
pub struct Session {
    /// A request is carried out: the page's replies are told through [`Held`].
    pub active: bool,
    /// What the request's replies held back.
    pub effects: Effects,
    /// The menu the page asked for, held for the agent to pick from.
    pub menu: Option<Menu>,
    /// Which menu that is: a number that grows with each, so that the clock that lets one go
    /// lets go of that one.
    pub menu_number: u64,
    /// The field of the host that the page's keyboard is in, as the page last said it: the
    /// system's keyboard is not moved for an agent, so the page's own word is what a reading
    /// goes by.
    pub field: Option<Id>,
}

impl Session {
    /// The agent is not holding a menu any more.
    pub fn let_go_of_menu(&mut self) {
        self.menu = None;
    }
}

/// A session shared by the pane's hosts and the request.
pub type Shared = Rc<RefCell<Session>>;

/// The host the page's replies are told to while an agent's request is carried out.
pub struct Held<'a> {
    base: &'a mut dyn Host,
    session: &'a Shared,
    handle: &'a PageHandle,
}

impl<'a> Held<'a> {
    pub fn new(base: &'a mut dyn Host, session: &'a Shared, handle: &'a PageHandle) -> Self {
        Self {
            base,
            session,
            handle,
        }
    }
}

/// What a pattern is called in what an agent is told: the kit's name for it where it is one the kit
/// has ready as it is, else how long it is.
fn pattern_words(pattern: &Pattern) -> String {
    haptic::NAMES
        .iter()
        .find(|name| haptic::preset(name).as_ref() == Some(pattern))
        .map_or_else(
            || {
                format!(
                    "a pattern of its own ({} events, {:.1} s)",
                    pattern.events().len(),
                    pattern.seconds()
                )
            },
            |name| (*name).to_owned(),
        )
}

impl Host for Held<'_> {
    fn send(&mut self, message: String) {
        self.base.send(message);
    }

    fn close(&mut self) {
        self.base.close();
    }

    fn redraw(&mut self) {
        self.base.redraw();
    }

    /// The pointer is not the agent's: the shape is the person's.
    fn cursor(&mut self, _cursor: Cursor) {}

    fn fill(&mut self, field: Id, fill: Fill) {
        self.base.fill(field, fill);
    }

    /// The page's own idea of where the keyboard is moves; the system's does not, whoever has it.
    fn keyboard(&mut self, to: Keyboard) {
        self.session.borrow_mut().field = match to {
            Keyboard::Field(id) => Some(id),
            Keyboard::Page => None,
        };
    }

    fn text_size(&mut self, points: i32) {
        self.base.text_size(points);
    }

    /// Not felt: the person's trackpad is not the agent's. Said in the answer.
    fn haptic(&mut self, haptic: Haptic) {
        let said = match haptic {
            Haptic::Play(pattern) => {
                format!("the page asked to vibrate: {}", pattern_words(&pattern))
            }
            Haptic::Stop => "the page ended its vibration".to_owned(),
        };
        self.session.borrow_mut().effects.notes.push(said);
    }

    /// Not played: the person's speaker is not the agent's. Said in the answer.
    fn sound(&mut self, ask: SoundAsk) {
        let said = match ask {
            SoundAsk::Play(play) => match play.source {
                Source::Kit(name) => {
                    format!("the page played the sound {}", super::words(&name, 60))
                }
                Source::File(path) => {
                    format!("the page played a sound from {}", super::words(&path, 200))
                }
            },
            SoundAsk::Stop => "the page ended its sounds".to_owned(),
        };
        self.session.borrow_mut().effects.notes.push(said);
    }

    fn copy(&mut self, text: String, _by_person: bool) {
        self.session.borrow_mut().effects.copies.push(text);
    }

    /// The pasteboard is the person's: a paste line of a menu pastes nothing for an agent.
    fn paste(&mut self) -> Option<String> {
        None
    }

    fn link(&mut self, url: String) {
        self.session.borrow_mut().effects.links.push(url);
    }

    /// Turned away as the person's Cancel is: the page hears no files, or no place.
    fn panel(&mut self, panel: Panel) {
        self.session.borrow_mut().effects.panels += 1;
        let mut page = self.handle.page();
        match panel {
            Panel::Pick { .. } => page.give(Point::new(0.0, 0.0), FileSource::Pick, Vec::new()),
            Panel::Save { .. } => page.saving(None),
        }
    }

    /// Held, not shown: a menu that is popped up holds the thread until a person picks from it.
    fn menu(&mut self, menu: Menu) {
        let mut session = self.session.borrow_mut();
        session.menu_number += 1;
        session.menu = Some(menu);
    }

    fn carry(&mut self, carry: CarryOut) {
        self.base.carry(carry);
    }

    fn diagnostic(&mut self, words: String) {
        self.base.diagnostic(words);
    }

    fn ended(&mut self, why: Fault) {
        self.base.ended(why);
    }

    fn rearm(&mut self) {
        self.base.rearm();
    }
}
