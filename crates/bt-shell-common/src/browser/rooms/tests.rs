//! The rooms against a helper that is a thread of the test (the helper's own tests' stand-in): the
//! commands that go to it and the events it says are the protocol's, and what the page and the
//! shell are told is read from the effects.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use browser_ipc::message::{DownloadState, MenuItem, Profile};
use pencere_view::{BrowserOptions, Rect};

use super::*;
use crate::browser::helper::tests::{Behaviour, World, world};
use crate::browser::route::Direct;

const WAIT: Duration = Duration::from_secs(5);

/// A popup's place in device pixels, or none when it went.
type Place = Option<(i32, i32)>;

struct Pictures {
    popups: Mutex<Vec<(Id, Place)>>,
    gone: Mutex<Vec<Id>>,
}

impl Frames for Pictures {
    fn frame(&self, _: Id, _: Frame, _: Surface, _: bool) {}

    fn popup(&self, room: Id, at: Place) {
        lock(&self.popups).push((room, at));
    }

    fn gone(&self, room: Id) {
        lock(&self.gone).push(room);
    }
}

struct Rig {
    world: World,
    rooms: Rooms,
    pictures: Arc<Pictures>,
    woken: Arc<AtomicUsize>,
    /// Everything drained so far.
    effects: Vec<Effect>,
    /// What the helper got, in order.
    seen: Vec<Command>,
}

fn rig(name: &str, installed: bool) -> Rig {
    rig_with(name, installed, true)
}

fn rig_with(name: &str, installed: bool, proven: bool) -> Rig {
    let world = world(
        name,
        Behaviour::default(),
        installed.then_some("0.1.0"),
        Duration::from_secs(60),
    );
    let pictures = Arc::new(Pictures {
        popups: Mutex::new(Vec::new()),
        gone: Mutex::new(Vec::new()),
    });
    let woken = Arc::new(AtomicUsize::new(0));
    let counter = woken.clone();
    let rooms = Rooms::new(
        world.manager.clone(),
        "bateri",
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
        pictures.clone(),
    );
    let mut rig = Rig {
        world,
        rooms,
        pictures,
        woken,
        effects: Vec::new(),
        seen: Vec::new(),
    };
    // The way is shown before the rooms are, as it is after a probe: the tests that are not
    // about the way begin there.
    if proven {
        rig.prove("mac");
    }
    rig
}

fn room(id: u64, profile: Option<&str>, (w, h): (f32, f32)) -> pencere_drive::Browser {
    pencere_drive::Browser {
        id: Id::new(id),
        options: BrowserOptions {
            profile: profile.map(str::to_owned),
            ..BrowserOptions::default()
        },
        rect: Rect::new(10.0, 20.0, w, h),
        hidden: false,
        label: None,
    }
}

impl Rig {
    fn prove(&mut self, machine: &str) {
        let mut told = Vec::new();
        self.rooms
            .route(Ok((machine.to_owned(), Arc::new(Direct))), &mut told);
        self.effects.extend(told);
    }

    fn apply(&mut self, rooms: &[pencere_drive::Browser], scale: f32) {
        let mut told = Vec::new();
        self.rooms.apply(rooms, scale, &mut told);
        self.effects.extend(told);
    }

    /// Drains until `done` is true of what was drained, or fails.
    fn until(&mut self, what: &str, done: impl Fn(&[Effect]) -> bool) {
        let deadline = Instant::now() + WAIT;
        loop {
            let mut told = Vec::new();
            self.rooms.drain(&mut told);
            self.effects.extend(told);
            if done(&self.effects) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The commands the helper has got, up to the first that `done` takes. The rooms are drained
    /// meanwhile, as the shell does when it is woken: what the helper says is done only then.
    fn command(&mut self, what: &str, done: impl Fn(&Command) -> bool) -> Command {
        let deadline = Instant::now() + WAIT;
        loop {
            let mut told = Vec::new();
            self.rooms.drain(&mut told);
            self.effects.extend(told);
            match self.world.commands.recv_timeout(Duration::from_millis(20)) {
                Ok(command) => {
                    self.seen.push(command.clone());
                    if done(&command) {
                        return command;
                    }
                }
                Err(_) => assert!(Instant::now() < deadline, "no command: {what}"),
            }
        }
    }

    /// The helper says `event`.
    fn say(&self, event: &Event) {
        let events = lock(&self.world.events);
        events
            .last()
            .expect("a helper is running")
            .send(event)
            .unwrap();
    }

    /// Opens a room and waits until the page is told the browser is ready.
    fn open(&mut self, id: u64) {
        self.apply(&[room(id, None, (400.0, 300.0))], 2.0);
        self.until("ready", |effects| ready(effects, id));
    }
}

fn ready(effects: &[Effect], id: u64) -> bool {
    effects.iter().any(|e| {
        matches!(e, Effect::State(room, state)
            if *room == Id::new(id) && state.status == BrowserState::READY)
    })
}

fn states(effects: &[Effect], id: u64) -> Vec<&BrowserState> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::State(room, state) if *room == Id::new(id) => Some(state),
            _ => None,
        })
        .collect()
}

#[test]
fn a_room_opens_a_browser_of_its_size_and_the_page_is_told_it_is_starting_then_ready() {
    let mut rig = rig("open", true);
    rig.open(1);
    let states = states(&rig.effects, 1);
    assert_eq!(states[0].status, BrowserState::STARTING);
    assert!(states[0].message.is_some());
    assert_eq!(states.last().unwrap().status, BrowserState::READY);
    let open = rig.command("open", |c| matches!(c, Command::Open { .. }));
    let Command::Open {
        browser,
        profile,
        width,
        height,
        scale,
    } = open
    else {
        unreachable!()
    };
    assert_eq!((browser, width, height), (1, 400, 300));
    assert!((scale - 2.0).abs() < f32::EPSILON);
    // No name: a profile of the browser's own, gone with it.
    assert_eq!(profile.dir, None);
    assert!(profile.context.starts_with("memory-"));
    assert!(rig.woken.load(Ordering::SeqCst) > 0, "the shell was woken");
}

#[test]
fn a_named_profile_is_the_same_for_every_room_of_it_and_another_name_is_another() {
    let mut rig = rig("profiles", true);
    rig.apply(
        &[
            room(1, Some("work"), (100.0, 100.0)),
            room(2, Some("work"), (100.0, 100.0)),
            room(3, Some("play"), (100.0, 100.0)),
        ],
        1.0,
    );
    let mut profiles = Vec::new();
    while profiles.len() < 3 {
        if let Command::Open {
            browser, profile, ..
        } = rig.command("opens", |c| matches!(c, Command::Open { .. }))
        {
            profiles.push((browser, profile));
        }
    }
    let of = |wanted: &[u32]| -> Vec<Profile> {
        profiles
            .iter()
            .filter(|(id, _)| wanted.contains(id))
            .map(|(_, p)| p.clone())
            .collect()
    };
    // Which room got which browser number is the threads' business; two of three share a profile
    // and the third does not.
    let all = of(&[1, 2, 3]);
    let same = all
        .iter()
        .filter(|p| all.iter().filter(|q| q.context == p.context).count() == 2)
        .count();
    assert_eq!(same, 2, "{all:?}");
    assert!(all.iter().all(|p| p.dir.is_some()));
    assert_ne!(all[0].pass, "");
}

#[test]
fn a_room_that_is_resized_or_moves_to_another_display_tells_its_engine() {
    let mut rig = rig("resize", true);
    rig.open(1);
    rig.apply(&[room(1, None, (500.0, 250.0))], 2.0);
    let Command::Resize { width, height, .. } = rig.command("resize", |c| {
        matches!(c, Command::Resize { width: 500, .. })
    }) else {
        unreachable!()
    };
    assert_eq!((width, height), (500, 250));
    rig.apply(&[room(1, None, (500.0, 250.0))], 1.0);
    let Command::Resize { scale, .. } = rig.command(
        "scale",
        |c| matches!(c, Command::Resize { scale, .. } if *scale < 1.5),
    ) else {
        unreachable!()
    };
    assert!((scale - 1.0).abs() < f32::EPSILON);
    // A room of nothing is a point, not nothing.
    rig.apply(&[room(1, None, (0.0, 0.0))], 1.0);
    assert!(matches!(
        rig.command("a point", |c| matches!(
            c,
            Command::Resize {
                width: 1,
                height: 1,
                ..
            }
        )),
        Command::Resize { .. }
    ));
}

#[test]
fn a_room_that_goes_closes_its_browser_and_its_pictures_go() {
    let mut rig = rig("close", true);
    rig.open(1);
    rig.apply(&[], 1.0);
    rig.command("close", |c| matches!(c, Command::Close { browser: 1 }));
    assert_eq!(*lock(&rig.pictures.gone), vec![Id::new(1)]);
}

#[test]
fn a_room_that_went_before_its_engine_opened_leaves_nothing_open() {
    let mut rig = rig("early", true);
    rig.apply(&[room(1, None, (100.0, 100.0))], 1.0);
    rig.apply(&[], 1.0);
    // The open that was on its way finds nobody and is closed.
    rig.command("open", |c| matches!(c, Command::Open { .. }));
    rig.command("close", |c| matches!(c, Command::Close { .. }));
    let mut told = Vec::new();
    rig.rooms.drain(&mut told);
    assert!(states(&told, 1).is_empty(), "{}", told.len());
}

#[test]
fn what_the_page_asked_before_the_engine_was_open_is_done_once_it_is() {
    let mut rig = rig("held", true);
    rig.apply(&[room(1, None, (100.0, 100.0))], 1.0);
    let mut told = Vec::new();
    rig.rooms.ask(
        Id::new(1),
        BrowserAsk::Go("https://example.com/".into()),
        &mut told,
    );
    rig.rooms.ask(Id::new(1), BrowserAsk::Back, &mut told);
    rig.until("ready", |e| ready(e, 1));
    rig.command(
        "go",
        |c| matches!(c, Command::Navigate { url, .. } if url == "https://example.com/"),
    );
    rig.command("back", |c| matches!(c, Command::Back { .. }));
}

#[test]
fn a_page_asks_become_the_engines_commands() {
    let mut find = None;
    let go = |ask: &BrowserAsk, find: &mut Option<(String, bool)>| command_of(7, ask, find);
    assert_eq!(
        go(&BrowserAsk::Reload { cache: true }, &mut find),
        Some(Command::Reload {
            browser: 7,
            ignore_cache: false
        })
    );
    assert_eq!(
        go(&BrowserAsk::Reload { cache: false }, &mut find),
        Some(Command::Reload {
            browser: 7,
            ignore_cache: true
        })
    );
    assert_eq!(
        go(&BrowserAsk::Zoom(1.5), &mut find),
        Some(Command::Zoom {
            browser: 7,
            ratio: 1.5
        })
    );
    assert_eq!(
        go(&BrowserAsk::Devtools, &mut find),
        Some(Command::DevTools { browser: 7 })
    );
    assert_eq!(go(&BrowserAsk::Unknown, &mut find), None);
    // The next and the previous go on with the search that was asked.
    assert_eq!(go(&BrowserAsk::FindNext, &mut find), None);
    assert_eq!(
        go(
            &BrowserAsk::Find {
                pattern: "cat".into(),
                case: false
            },
            &mut find
        ),
        Some(Command::Find {
            browser: 7,
            pattern: "cat".into(),
            case: false,
            forward: true,
            next: false
        })
    );
    assert_eq!(
        go(&BrowserAsk::FindPrevious, &mut find),
        Some(Command::Find {
            browser: 7,
            pattern: "cat".into(),
            case: false,
            forward: false,
            next: true
        })
    );
    // An empty pattern, and StopFinding, end it.
    assert_eq!(
        go(
            &BrowserAsk::Find {
                pattern: String::new(),
                case: true
            },
            &mut find
        ),
        Some(Command::StopFinding {
            browser: 7,
            keep_selection: false
        })
    );
    assert_eq!(find, None);
}

#[test]
fn the_engines_state_is_the_pages_and_a_search_with_no_match_has_no_current() {
    let mut rig = rig("state", true);
    rig.open(1);
    rig.say(&Event::State {
        browser: 1,
        url: "https://example.com/".into(),
        title: "Example".into(),
        loading: true,
        can_back: true,
        can_forward: false,
        status: "waiting".into(),
        zoom: 1.25,
        error: None,
        found: Some(browser_ipc::message::Found {
            count: 0,
            current: 0,
        }),
    });
    rig.until("the title", |e| {
        states(e, 1).iter().any(|s| s.title == "Example")
    });
    let all = states(&rig.effects, 1);
    let state = all.iter().find(|s| s.title == "Example").unwrap();
    assert_eq!(state.url, "https://example.com/");
    assert!(state.loading && state.back && !state.forward);
    // The engine's own status line is not the browser's status.
    assert_eq!(state.status, BrowserState::READY);
    assert!((state.zoom - 1.25).abs() < f32::EPSILON);
    assert_eq!(
        state.found,
        Some(BrowserFound {
            count: 0,
            current: None
        })
    );
}

#[test]
fn the_helpers_answer_to_a_scroll_question_comes_back_numbered_and_history_is_what_state_said() {
    let mut rig = rig("scroll", true);
    rig.open(1);
    assert_eq!(rig.rooms.history(Id::new(1)), (false, false));
    rig.say(&Event::State {
        browser: 1,
        url: "https://example.com/".into(),
        title: "Example".into(),
        loading: false,
        can_back: true,
        can_forward: false,
        status: String::new(),
        zoom: 1.0,
        error: None,
        found: None,
    });
    rig.until("the state", |e| {
        states(e, 1).iter().any(|s| s.title == "Example")
    });
    assert_eq!(rig.rooms.history(Id::new(1)), (true, false));
    assert_eq!(rig.rooms.history(Id::new(99)), (false, false));
    // The question is sent to the room's engine with its own number.
    assert!(rig.rooms.send(Id::new(1), |browser| Command::CanScroll {
        browser,
        req: 4,
        x: 10,
        y: 20,
        dx: 1,
    }));
    let asked = rig.command("the question", |c| matches!(c, Command::CanScroll { .. }));
    assert_eq!(
        asked,
        Command::CanScroll {
            browser: 1,
            req: 4,
            x: 10,
            y: 20,
            dx: 1
        }
    );
    rig.say(&Event::CanScroll {
        browser: 1,
        req: 4,
        can: false,
    });
    rig.until("the answer", |e| {
        e.iter()
            .any(|e| matches!(e, Effect::CanScroll(room, 4, false) if *room == Id::new(1)))
    });
}

#[test]
fn the_browsers_of_a_page_are_listed_by_id_with_what_they_are_and_where() {
    let mut rig = rig("seen", true);
    assert!(rig.rooms.seen().is_empty());
    rig.apply(
        &[
            room(0xb2, None, (400.0, 300.0)),
            room(0xa1, None, (400.0, 300.0)),
        ],
        2.0,
    );
    // Before the engines say anything they are starting.
    let before = rig.rooms.seen();
    assert_eq!(
        before.iter().map(|b| b.id).collect::<Vec<_>>(),
        vec![Id::new(0xa1), Id::new(0xb2)]
    );
    assert!(before.iter().all(|b| b.status == BrowserState::STARTING));
    rig.until("both ready", |e| ready(e, 0xa1) && ready(e, 0xb2));
    // The engines are numbered as their threads come to the helper, so which room is engine 1 is
    // not said: the one that is says what it is, and the other still does not.
    rig.say(&Event::State {
        browser: 1,
        url: "https://example.com/".into(),
        title: "Example".into(),
        loading: false,
        can_back: false,
        can_forward: false,
        status: String::new(),
        zoom: 1.0,
        error: None,
        found: None,
    });
    rig.until("the title", |e| {
        [0xa1, 0xb2]
            .iter()
            .any(|id| states(e, *id).iter().any(|s| s.title == "Example"))
    });
    let seen = rig.rooms.seen();
    let (named, unnamed): (Vec<_>, Vec<_>) = seen.iter().partition(|b| b.title == "Example");
    assert_eq!((named.len(), unnamed.len()), (1, 1));
    assert_eq!(named[0].url, "https://example.com/");
    assert_eq!(unnamed[0].url, "");
    assert!(seen.iter().all(|b| b.status == BrowserState::READY));
}

/// An agent's job is a command with a number the answer comes back with, to the room that has
/// the browser.
#[test]
fn an_agents_job_is_sent_to_the_browser_and_its_answer_comes_back_for_the_room() {
    let mut rig = rig("agent-job", true);
    rig.open(1);
    assert!(rig.rooms.send(Id::new(1), |browser| Command::Agent {
        browser,
        req: 41,
        op: browser_ipc::message::AgentOp::Text,
    }));
    let Command::Agent { browser, req, op } =
        rig.command("the job", |c| matches!(c, Command::Agent { .. }))
    else {
        unreachable!()
    };
    assert_eq!((browser, req), (1, 41));
    assert_eq!(op, browser_ipc::message::AgentOp::Text);
    rig.say(&Event::AgentResult {
        browser: 1,
        req: 41,
        output: browser_ipc::message::AgentOutput::Text {
            text: "hello".into(),
            truncated: false,
        },
    });
    rig.until("the answer", |e| {
        e.iter().any(|e| {
            matches!(e, Effect::Agent(room, 41, browser_ipc::message::AgentOutput::Text { text, .. })
                if *room == Id::new(1) && text == "hello")
        })
    });
    // A browser that is not open cannot be given one.
    assert!(!rig.rooms.send(Id::new(9), |browser| Command::Agent {
        browser,
        req: 42,
        op: browser_ipc::message::AgentOp::Shot,
    }));
}

#[test]
fn a_link_that_opens_a_window_goes_here_or_is_the_pages_to_decide() {
    let mut rig = rig("window", true);
    rig.open(1);
    // Here: the browser goes there itself.
    rig.say(&Event::Window {
        browser: 1,
        url: "https://a.test/".into(),
        background: false,
    });
    rig.command(
        "here",
        |c| matches!(c, Command::Navigate { url, .. } if url == "https://a.test/"),
    );
    // The page decides: it is told, with the way the person asked for it.
    let mut page = room(1, None, (400.0, 300.0));
    page.options.windows = BrowserWindows::Page;
    rig.apply(&[page], 2.0);
    rig.say(&Event::Window {
        browser: 1,
        url: "https://b.test/".into(),
        background: true,
    });
    rig.until("the window", |e| {
        e.iter()
            .any(|e| matches!(e, Effect::Window(_, url, true) if url == "https://b.test/"))
    });
}

#[test]
fn the_helper_is_told_where_windows_go_as_it_opens_and_when_that_changes() {
    use browser_ipc::message::NewWindows;
    let mut rig = rig("winmode", true);
    // As the engine opens: the room's own (it goes here, in these rooms), before the first size.
    rig.open(1);
    let first = rig.command("the mode", |c| matches!(c, Command::Windows { .. }));
    assert_eq!(
        first,
        Command::Windows {
            browser: 1,
            mode: NewWindows::Here
        }
    );
    // A change is said once, and what is said again is not.
    let mut page = room(1, None, (400.0, 300.0));
    page.options.windows = BrowserWindows::Page;
    rig.apply(&[page.clone()], 2.0);
    rig.apply(&[page.clone()], 2.0);
    let second = rig.command("the mode", |c| matches!(c, Command::Windows { .. }));
    assert_eq!(
        second,
        Command::Windows {
            browser: 1,
            mode: NewWindows::Page
        }
    );
    page.options.windows = BrowserWindows::Here;
    rig.apply(&[page], 2.0);
    rig.command("the mode back", |c| {
        matches!(
            c,
            Command::Windows {
                mode: NewWindows::Here,
                ..
            }
        )
    });
    let said = rig
        .seen
        .iter()
        .filter(|c| matches!(c, Command::Windows { .. }))
        .count();
    assert_eq!(said, 3, "{:?}", rig.seen);
}

fn unhandled(kind: KeyKind, vk: i32, character: char, mods: u32) -> Event {
    Event::UnhandledKey {
        browser: 1,
        kind,
        key: KeyData {
            vk,
            native: 0,
            character: character as u16,
            unmodified: character as u16,
            mods,
            system: false,
        },
    }
}

#[test]
fn a_key_nobody_used_is_the_pages_on_its_press_and_its_release_follows_only_a_press() {
    let mut rig = rig("keys", true);
    rig.open(1);
    // A release the page never saw the press of (the page's own script took the press) is nothing.
    rig.say(&unhandled(KeyKind::Up, 76, 'l', modifier::COMMAND));
    rig.say(&unhandled(KeyKind::Down, 37, '\u{f702}', modifier::COMMAND));
    rig.say(&unhandled(KeyKind::Up, 37, '\u{f702}', modifier::COMMAND));
    rig.say(&unhandled(
        KeyKind::Down,
        75,
        'k',
        modifier::COMMAND | modifier::SHIFT,
    ));
    rig.until("the keys", |e| {
        e.iter()
            .filter(|e| matches!(e, Effect::UnusedKey(..)))
            .count()
            >= 3
    });
    let keys: Vec<(Key, Mods, bool)> = rig
        .effects
        .iter()
        .filter_map(|e| match e {
            Effect::UnusedKey(_, key, mods, down) => Some((*key, *mods, *down)),
            _ => None,
        })
        .collect();
    let command = Mods {
        command: true,
        ..Mods::default()
    };
    assert_eq!(
        keys,
        vec![
            (Key::Left, command, true),
            (Key::Left, command, false),
            (
                Key::Character('k'),
                Mods {
                    shift: true,
                    ..command
                },
                true
            ),
        ]
    );
}

#[test]
fn an_edit_key_the_engine_gives_back_is_the_menus_command_and_is_not_the_pages() {
    use browser_ipc::message::EditAction;
    let mut rig = rig("editkeys", true);
    rig.open(1);
    for (vk, character, mods, action) in [
        (65, 'a', modifier::COMMAND, EditAction::SelectAll),
        (67, 'c', modifier::COMMAND, EditAction::Copy),
        (88, 'x', modifier::COMMAND, EditAction::Cut),
        (86, 'v', modifier::COMMAND, EditAction::Paste),
        (90, 'z', modifier::COMMAND, EditAction::Undo),
        (
            90,
            'z',
            modifier::COMMAND | modifier::SHIFT,
            EditAction::Redo,
        ),
    ] {
        rig.say(&unhandled(KeyKind::Down, vk, character, mods));
        rig.say(&unhandled(KeyKind::Up, vk, character, mods));
        let command = rig.command("the menu's command", |c| matches!(c, Command::Edit { .. }));
        assert!(
            matches!(&command, Command::Edit { action: a, .. } if *a == action),
            "{vk} {mods}: {command:?}"
        );
    }
    // What is not the menu's key (other modifiers, ⌘K) is the page's as before, and the page is
    // given none of the edit keys: the press and the release above made no effect for it.
    rig.say(&unhandled(
        KeyKind::Down,
        67,
        'c',
        modifier::COMMAND | modifier::ALT,
    ));
    rig.say(&unhandled(KeyKind::Down, 67, 'c', modifier::CONTROL));
    rig.say(&unhandled(KeyKind::Down, 75, 'k', modifier::COMMAND));
    rig.until("the keys", |e| {
        e.iter()
            .filter(|e| matches!(e, Effect::UnusedKey(..)))
            .count()
            >= 3
    });
    let keys: Vec<Key> = rig
        .effects
        .iter()
        .filter_map(|e| match e {
            Effect::UnusedKey(_, key, _, true) => Some(*key),
            _ => None,
        })
        .collect();
    assert_eq!(
        keys,
        vec![
            Key::Character('c'),
            Key::Character('c'),
            Key::Character('k')
        ]
    );
    assert!(
        !rig.effects
            .iter()
            .any(|e| matches!(e, Effect::UnusedKey(_, _, _, false))),
        "the release of an edit key is the page's"
    );
}

#[test]
fn a_key_is_the_pages_before_the_browsers_where_the_page_asked_for_it() {
    let mut rig = rig("pagekeys", true);
    let mut page = room(1, None, (100.0, 100.0));
    page.options.keys = vec![Shortcut::new(
        Key::Left,
        Mods {
            command: true,
            ..Mods::default()
        },
    )];
    rig.apply(&[page], 1.0);
    let command = Mods {
        command: true,
        ..Mods::default()
    };
    assert!(rig.rooms.page_key(Id::new(1), Key::Left, command));
    assert!(!rig.rooms.page_key(Id::new(1), Key::Right, command));
    assert!(!rig.rooms.page_key(Id::new(1), Key::Left, Mods::default()));
    assert!(!rig.rooms.page_key(Id::new(2), Key::Left, command));
}

#[test]
fn keys_the_engine_returns_are_named_for_the_page() {
    let key = |vk: i32, unmodified: char| {
        key_of(&KeyData {
            vk,
            unmodified: unmodified as u16,
            ..KeyData::default()
        })
    };
    assert_eq!(key(13, '\r'), Some(Key::Enter));
    assert_eq!(key(27, '\u{1b}'), Some(Key::Escape));
    assert_eq!(key(36, '\u{f729}'), Some(Key::Home));
    assert_eq!(key(112, '\u{f704}'), Some(Key::Function(1)));
    assert_eq!(key(123, '\u{f70f}'), Some(Key::Function(12)));
    assert_eq!(key(75, 'k'), Some(Key::Character('k')));
    assert_eq!(key(75, 'K'), Some(Key::Character('k')));
    assert_eq!(key(188, ','), Some(Key::Character(',')));
    // No character but an alphanumeric virtual key: the letter.
    assert_eq!(key(65, '\0'), Some(Key::Character('a')));
    assert_eq!(key(0, '\0'), None);
    assert!(mods_of(modifier::SHIFT | modifier::ALT).alt);
    assert_eq!(mods_of(modifier::CAPS_LOCK), Mods::default());
}

#[test]
fn every_question_is_answered_once_whatever_becomes_of_it() {
    let mut rig = rig("questions", true);
    rig.open(1);
    let line = |command: i32| MenuItem {
        command,
        label: format!("line {command}"),
        enabled: true,
        checked: false,
        separator: false,
        shortcut: String::new(),
    };
    let menu = |req: u32| Event::Menu {
        browser: 1,
        req,
        x: 5,
        y: 6,
        items: vec![line(1), line(2)],
        link: Some("https://x.test/".into()),
        source: None,
        selection: None,
        editable: false,
    };
    // A menu the person turned away (dropped) says none; one they picked from says the line.
    rig.say(&menu(11));
    rig.until("a menu", |e| {
        e.iter().any(|e| matches!(e, Effect::Menu(..)))
    });
    let Some(Effect::Menu(_, asked)) = take(&mut rig.effects, |e| matches!(e, Effect::Menu(..)))
    else {
        unreachable!()
    };
    assert_eq!((asked.x, asked.y, asked.items.len()), (5, 6, 2));
    assert_eq!(asked.link.as_deref(), Some("https://x.test/"));
    drop(asked);
    rig.command("none", |c| {
        matches!(
            c,
            Command::MenuResult {
                req: 11,
                command: None
            }
        )
    });
    rig.say(&menu(12));
    rig.until("a second menu", |e| {
        e.iter().any(|e| matches!(e, Effect::Menu(..)))
    });
    let Some(Effect::Menu(_, asked)) = take(&mut rig.effects, |e| matches!(e, Effect::Menu(..)))
    else {
        unreachable!()
    };
    asked.pick(Some(2));
    rig.command("picked", |c| {
        matches!(
            c,
            Command::MenuResult {
                req: 12,
                command: Some(2)
            }
        )
    });
    // A dialog turned away says no, with no text.
    rig.say(&Event::Dialog {
        browser: 1,
        req: 21,
        kind: DialogKind::Prompt,
        origin: "https://x.test".into(),
        message: "name?".into(),
        default_text: "me".into(),
    });
    rig.until("a dialog", |e| {
        e.iter().any(|e| matches!(e, Effect::Dialog(..)))
    });
    let Some(Effect::Dialog(_, dialog)) =
        take(&mut rig.effects, |e| matches!(e, Effect::Dialog(..)))
    else {
        unreachable!()
    };
    assert_eq!(
        (dialog.kind, dialog.default_text.as_str()),
        (DialogKind::Prompt, "me")
    );
    drop(dialog);
    rig.command(
        "no",
        |c| matches!(c, Command::DialogResult { req: 21, ok: false, text } if text.is_empty()),
    );
    // A dialog the page left behind (it moved on) is void: it is named for the shell to take down
    // and, taken down, says nothing, where one merely dropped says no.
    rig.say(&Event::Dialog {
        browser: 1,
        req: 22,
        kind: DialogKind::Confirm,
        origin: "https://x.test".into(),
        message: "leave?".into(),
        default_text: String::new(),
    });
    rig.say(&Event::DialogVoid {
        browser: 1,
        req: 22,
    });
    rig.until("a void dialog", |e| {
        e.iter().any(|e| matches!(e, Effect::DialogVoid(..)))
    });
    let Some(Effect::Dialog(_, dialog)) =
        take(&mut rig.effects, |e| matches!(e, Effect::Dialog(..)))
    else {
        unreachable!()
    };
    assert!(
        rig.effects
            .iter()
            .any(|e| matches!(e, Effect::DialogVoid(_, 22))),
        "the void dialog is named by its number"
    );
    assert_eq!(dialog.req(), 22);
    dialog.void();
    // Nothing comes of it: the next thing the helper hears is of the question after it.
    rig.say(&Event::Dialog {
        browser: 1,
        req: 23,
        kind: DialogKind::Alert,
        origin: "https://x.test".into(),
        message: "hi".into(),
        default_text: String::new(),
    });
    rig.until("a dialog after it", |e| {
        e.iter().any(|e| matches!(e, Effect::Dialog(..)))
    });
    let Some(Effect::Dialog(_, after)) =
        take(&mut rig.effects, |e| matches!(e, Effect::Dialog(..)))
    else {
        unreachable!()
    };
    drop(after);
    let answered = rig.command("the dialog after it", |c| {
        matches!(c, Command::DialogResult { .. })
    });
    assert!(
        matches!(
            answered,
            Command::DialogResult {
                req: 23,
                ok: false,
                ..
            }
        ),
        "a void dialog answered: {answered:?}"
    );
    // Files turned away say none.
    rig.say(&Event::FileDialog {
        browser: 1,
        req: 31,
        mode: FileMode::OpenMultiple,
        title: "Pick".into(),
        default_path: String::new(),
        accept: vec![".png".into()],
    });
    rig.until("files", |e| {
        e.iter().any(|e| matches!(e, Effect::Files(..)))
    });
    let Some(Effect::Files(_, files)) = take(&mut rig.effects, |e| matches!(e, Effect::Files(..)))
    else {
        unreachable!()
    };
    assert_eq!(files.mode, FileMode::OpenMultiple);
    files.choose(vec![
        PathBuf::from("/tmp/a.png"),
        PathBuf::from("/tmp/b.png"),
    ]);
    rig.command("files", |c| {
        matches!(c, Command::FileResult { req: 31, paths } if paths == &["/tmp/a.png", "/tmp/b.png"])
    });
}

fn take(effects: &mut Vec<Effect>, which: impl Fn(&Effect) -> bool) -> Option<Effect> {
    let at = effects.iter().position(which)?;
    Some(effects.remove(at))
}

#[test]
fn nothing_of_a_download_is_written_before_the_person_says_where_and_its_end_is_told() {
    let mut rig = rig("download", true);
    rig.open(1);
    let offer = |req: u32| Event::Download {
        browser: 1,
        req,
        url: "https://x.test/f.zip".into(),
        name: "../f.zip".into(),
        mime: "application/zip".into(),
        size: Some(10),
    };
    // Not wanted (the question dropped): the engine is told so, and no path was ever given.
    rig.say(&offer(41));
    rig.until("a download", |e| {
        e.iter().any(|e| matches!(e, Effect::Download(..)))
    });
    let Some(Effect::Download(_, asked)) =
        take(&mut rig.effects, |e| matches!(e, Effect::Download(..)))
    else {
        unreachable!()
    };
    assert_eq!(asked.name, "../f.zip", "the name as the page gave it");
    drop(asked);
    rig.command("not wanted", |c| {
        matches!(
            c,
            Command::DownloadResult {
                req: 41,
                path: None
            }
        )
    });
    assert!(
        !rig.seen
            .iter()
            .any(|c| matches!(c, Command::DownloadResult { path: Some(_), .. })),
        "{:?}",
        rig.seen
    );
    // Wanted: the engine is given the place, and the end of the download is the shell's to hear.
    rig.say(&offer(42));
    rig.until("a second download", |e| {
        e.iter().any(|e| matches!(e, Effect::Download(..)))
    });
    let Some(Effect::Download(_, asked)) =
        take(&mut rig.effects, |e| matches!(e, Effect::Download(..)))
    else {
        unreachable!()
    };
    asked.save(PathBuf::from("/tmp/saved/f.zip"));
    rig.command("saved", |c| {
        matches!(c, Command::DownloadResult { req: 42, path: Some(p) } if p == "/tmp/saved/f.zip")
    });
    rig.say(&Event::DownloadProgress {
        req: 42,
        state: DownloadState::Running,
        received: 5,
        total: Some(10),
    });
    rig.say(&Event::DownloadProgress {
        req: 42,
        state: DownloadState::Done,
        received: 10,
        total: Some(10),
    });
    rig.until("its end", |e| {
        e.iter().any(|e| matches!(e, Effect::Downloaded(..)))
    });
    let ends: Vec<_> = rig
        .effects
        .iter()
        .filter_map(|e| match e {
            Effect::Downloaded(_, path, done) => Some((path.clone(), *done)),
            _ => None,
        })
        .collect();
    assert_eq!(ends, vec![(PathBuf::from("/tmp/saved/f.zip"), true)]);
}

#[test]
fn an_engine_that_is_not_installed_asks_the_shell_and_asks_again_on_a_reload() {
    let mut rig = rig("missing", false);
    rig.apply(&[room(1, None, (100.0, 100.0))], 1.0);
    rig.until("the question", |e| {
        e.iter().any(|e| matches!(e, Effect::NotInstalled(_)))
    });
    let before = rig.effects.len();
    let mut told = Vec::new();
    rig.rooms
        .ask(Id::new(1), BrowserAsk::Reload { cache: true }, &mut told);
    assert!(
        matches!(told.as_slice(), [Effect::NotInstalled(_)]),
        "{}",
        told.len()
    );
    // Anything else waits, and is done when the engine is there.
    rig.rooms.ask(
        Id::new(1),
        BrowserAsk::Go("https://a.test/".into()),
        &mut told,
    );
    assert_eq!(told.len(), 1);
    let _ = before;
}

#[test]
fn an_engine_that_ended_tells_the_page_and_a_reload_starts_it_again() {
    let mut rig = rig("ended", true);
    rig.open(1);
    // The helper's end of the conversation closes.
    lock(&rig.world.events).last().unwrap().shutdown();
    rig.until("the end", |e| {
        states(e, 1)
            .iter()
            .any(|s| s.status == BrowserState::CRASHED)
    });
    let crashed = states(&rig.effects, 1)
        .into_iter()
        .find(|s| s.status == BrowserState::CRASHED)
        .unwrap()
        .clone();
    assert!(crashed.message.unwrap().contains("stopped"));
    // Asks that mean nothing to an engine that is not there are let go; a reload begins again.
    let mut told = Vec::new();
    rig.rooms.ask(Id::new(1), BrowserAsk::Back, &mut told);
    assert!(told.is_empty());
    rig.rooms
        .ask(Id::new(1), BrowserAsk::Reload { cache: true }, &mut told);
    rig.effects.extend(told);
    rig.until("started again", |e| {
        states(e, 1)
            .iter()
            .filter(|s| s.status == BrowserState::STARTING)
            .count()
            >= 2
    });
    assert!(rig.world.spawned.load(Ordering::SeqCst) >= 1);
}

#[test]
fn a_popup_of_the_engine_goes_with_the_frames_and_not_the_main_thread() {
    let mut rig = rig("popup", true);
    rig.open(1);
    rig.say(&Event::Popup {
        browser: 1,
        show: true,
        x: 10,
        y: 20,
        width: 100,
        height: 80,
    });
    rig.say(&Event::Popup {
        browser: 1,
        show: false,
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    });
    let deadline = Instant::now() + WAIT;
    while lock(&rig.pictures.popups).len() < 2 {
        assert!(Instant::now() < deadline, "no popups");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        *lock(&rig.pictures.popups),
        vec![(Id::new(1), Some((10, 20))), (Id::new(1), None)]
    );
}

#[test]
fn a_refused_address_is_an_error_the_page_can_read() {
    let mut rig = rig("refused", true);
    rig.open(1);
    rig.say(&Event::Refused {
        browser: 1,
        url: "file:///etc/passwd".into(),
        reason: "scheme".into(),
    });
    rig.until("the refusal", |e| {
        states(e, 1).iter().any(|s| s.error.is_some())
    });
    let all = states(&rig.effects, 1);
    let error = all.iter().find_map(|s| s.error.as_ref()).unwrap();
    assert_eq!(error.url, "file:///etc/passwd");
    assert!(error.message.contains("not opened here"));
}

fn routes_asked(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::Route))
        .count()
}

#[test]
fn no_engine_starts_before_the_way_is_shown_and_the_way_is_asked_for_once() {
    let mut rig = rig_with("routing", true, false);
    rig.apply(
        &[
            room(1, None, (100.0, 100.0)),
            room(2, Some("work"), (100.0, 100.0)),
        ],
        1.0,
    );
    assert_eq!(routes_asked(&rig.effects), 1);
    for id in [1, 2] {
        let all = states(&rig.effects, id);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].status, BrowserState::STARTING);
        assert_eq!(all[0].message.as_deref(), Some(FINDING));
    }
    // Nothing was started: the helper has not been asked for a browser.
    assert!(
        rig.world
            .commands
            .recv_timeout(Duration::from_millis(300))
            .is_err()
    );
    assert!(rig.world.routes_empty());
    // Shown: the rooms start and the proxy has the way of the machine.
    rig.prove("mac");
    rig.until("ready", |e| ready(e, 1) && ready(e, 2));
    assert!(rig.world.manager.routes().get("mac").is_some());
    assert_eq!(routes_asked(&rig.effects), 1);
}

#[test]
fn a_way_that_was_not_shown_says_why_and_a_reload_looks_again() {
    let mut rig = rig_with("noroute", true, false);
    rig.apply(&[room(1, None, (100.0, 100.0))], 1.0);
    let mut told = Vec::new();
    rig.rooms
        .route(Err("no way, said the proof".into()), &mut told);
    rig.effects.extend(told);
    let all = states(&rig.effects, 1);
    let last = all.last().unwrap();
    assert_eq!(last.status, BrowserState::NO_ROUTE);
    assert_eq!(last.message.as_deref(), Some("no way, said the proof"));
    // A room made after it is told so at once, and the way is not asked for again.
    rig.apply(
        &[room(1, None, (100.0, 100.0)), room(2, None, (100.0, 100.0))],
        1.0,
    );
    let second = states(&rig.effects, 2);
    assert_eq!(second[0].status, BrowserState::NO_ROUTE);
    assert_eq!(routes_asked(&rig.effects), 1);
    // The person says reload: the way is looked for again, and if it is found the room starts.
    let mut told = Vec::new();
    rig.rooms
        .ask(Id::new(1), BrowserAsk::Reload { cache: true }, &mut told);
    assert_eq!(routes_asked(&told), 1, "asked again");
    rig.effects.extend(told);
    rig.prove("mac");
    rig.until("ready", |e| ready(e, 1));
    // Both rooms start: the other waited for the same answer.
    rig.until("the other", |e| ready(e, 2));
}
