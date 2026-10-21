use kablo_proto::v1::{State, TerminalState};

use super::*;

fn id(name: &str) -> SessionId {
    SessionId::new(name).expect("an id")
}

/// A running terminal that wrote at `output_at`.
fn status(name: &str, output_at: Option<u64>) -> Status {
    let mut status = Status::new(id(name));
    status.output_at = output_at;
    status
}

fn names(found: &[Background]) -> Vec<&str> {
    found.iter().map(|b| b.terminal.as_str()).collect()
}

fn listed(statuses: Vec<Status>) -> Roster {
    let mut roster = Roster::default();
    roster.apply(&Event::Listed(statuses));
    roster
}

fn shown_nowhere(_: &SessionId) -> bool {
    false
}

#[test]
fn what_the_server_lists_is_what_runs_in_the_background_the_last_to_write_first() {
    let roster = listed(vec![
        status("quiet", None),
        status("old", Some(100)),
        status("recent", Some(900)),
        status("middle", Some(500)),
    ]);
    let found = roster.background("u@rasp:22", "rasp", shown_nowhere);
    assert_eq!(names(&found), ["recent", "middle", "old", "quiet"]);
    assert_eq!(found[0].account, "u@rasp:22");
    assert_eq!(found[0].host, "rasp");
}

#[test]
fn a_terminal_that_a_pane_shows_or_that_was_asked_to_end_is_not_in_the_background() {
    let roster = listed(vec![
        status("here", Some(3)),
        status("ending", Some(2)),
        status("away", Some(1)),
    ]);
    let hidden = |terminal: &SessionId| matches!(terminal.as_str(), "here" | "ending");
    let found = roster.background("a", "rasp", hidden);
    assert_eq!(names(&found), ["away"]);
}

#[test]
fn only_a_running_terminal_is_in_the_background() {
    let mut ended = status("ended", Some(9));
    ended.state = State::ENDED;
    let mut broken = status("broken", Some(8));
    broken.state = State::BROKEN;
    let roster = listed(vec![ended, broken, status("running", Some(1))]);
    let found = roster.background("a", "rasp", shown_nowhere);
    assert_eq!(names(&found), ["running"]);
}

#[test]
fn the_times_the_server_says_are_read_as_they_are() {
    let mut full = status("busy", Some(1_000));
    full.started = Some(400);
    full.front_started = Some(700);
    full.front_is_own = Some(false);
    full.clients = 2;
    let roster = listed(vec![full]);
    let [found] = &roster.background("a", "rasp", shown_nowhere)[..] else {
        panic!("one terminal");
    };
    assert_eq!(
        (
            found.started,
            found.front_started,
            found.front_is_own,
            found.output_at,
            found.clients
        ),
        (Some(400), Some(700), Some(false), Some(1_000), 2)
    );
    assert_eq!(
        found.activity(1_420),
        Activity::Running(Some(Duration::from_secs(720))),
        "a program in front since 700 has run twelve minutes at 1420"
    );
}

#[test]
fn a_shell_waiting_is_idle_since_its_last_output_and_an_unknown_one_says_nothing() {
    let mut waiting = status("shell", Some(1_000));
    waiting.started = Some(400);
    waiting.front_is_own = Some(true);
    let mut silent = status("silent", None);
    silent.started = Some(400);
    silent.front_is_own = Some(true);
    let roster = listed(vec![waiting, silent, status("old-relay", Some(5))]);
    let found = roster.background("a", "rasp", shown_nowhere);
    let by_name = |name: &str| {
        found
            .iter()
            .find(|b| b.terminal.as_str() == name)
            .expect("it")
    };
    assert_eq!(
        by_name("shell").activity(2_000),
        Activity::Idle { since: Some(1_000) }
    );
    assert_eq!(
        by_name("silent").activity(2_000),
        Activity::Idle { since: Some(400) },
        "no output yet: since its start"
    );
    assert_eq!(by_name("old-relay").activity(2_000), Activity::Unknown);
}

#[test]
fn an_answer_to_a_list_replaces_the_cache_and_an_answer_to_a_status_sets_one_terminal() {
    let mut roster = listed(vec![status("a", Some(1)), status("b", Some(2))]);
    roster.apply(&Event::Listed(vec![
        status("b", Some(2)),
        status("c", Some(3)),
    ]));
    assert_eq!(
        names(&roster.background("x", "h", shown_nowhere)),
        ["c", "b"],
        "a is gone from the server"
    );
    roster.apply(&Event::Status(status("b", Some(10))));
    roster.apply(&Event::Created(status("d", Some(4))));
    assert_eq!(
        names(&roster.background("x", "h", shown_nowhere)),
        ["b", "d", "c"]
    );
}

#[test]
fn a_terminal_that_ended_or_is_not_there_is_taken_away() {
    let mut roster = listed(vec![
        status("a", Some(1)),
        status("b", Some(2)),
        status("c", Some(3)),
    ]);
    roster.apply(&Event::Ended {
        terminal: id("a"),
        exit: kablo_proto::v1::Exit::Code(0),
    });
    roster.apply(&Event::Failed {
        terminal: Some(id("b")),
        code: ErrorCode::UNKNOWN_SESSION,
        text: String::new(),
    });
    // Any other failure says nothing of whether the terminal is there.
    roster.apply(&Event::Failed {
        terminal: Some(id("c")),
        code: ErrorCode::UNAVAILABLE,
        text: String::new(),
    });
    assert_eq!(names(&roster.background("x", "h", shown_nowhere)), ["c"]);
}

#[test]
fn the_program_in_front_is_renewed_and_what_was_said_of_the_old_one_is_forgotten() {
    let mut busy = status("t", Some(1));
    busy.program = Some(b"zsh".to_vec());
    busy.front_started = Some(10);
    busy.front_is_own = Some(true);
    let mut roster = listed(vec![busy]);
    let mut state = TerminalState::default();
    state.program = Some(b"claude".to_vec());
    state.cwd = Some(b"/srv/app".to_vec());
    roster.apply(&Event::State {
        terminal: id("t"),
        state,
    });
    let [found] = &roster.background("a", "h", shown_nowhere)[..] else {
        panic!("one terminal");
    };
    assert_eq!(found.program.as_deref(), Some("claude"));
    assert_eq!(found.cwd.as_deref(), Some("/srv/app"));
    assert_eq!((found.front_started, found.front_is_own), (None, None));
    // A terminal the cache does not know is not made up from its state.
    roster.apply(&Event::State {
        terminal: id("unknown"),
        state: TerminalState::default(),
    });
    assert_eq!(roster.background("a", "h", shown_nowhere).len(), 1);
}

#[test]
fn what_a_person_reads_has_no_control_character_in_it() {
    let mut odd = status("odd", Some(1));
    odd.program = Some(b"cla\x1b[31mude\x07".to_vec());
    odd.cwd = Some(b"/srv/\napp".to_vec());
    odd.label = Some("night\r\nbuild".to_owned());
    let found = listed(vec![odd]).background("a", "h", shown_nowhere);
    assert_eq!(found[0].program.as_deref(), Some("cla[31mude"));
    assert_eq!(found[0].cwd.as_deref(), Some("/srv/app"));
    assert_eq!(found[0].label.as_deref(), Some("nightbuild"));
}

#[test]
fn events_that_say_nothing_of_the_terminals_change_nothing() {
    let mut roster = listed(vec![status("a", Some(1))]);
    for event in [
        Event::Linked,
        Event::Output {
            terminal: id("a"),
            data: b"x".to_vec(),
        },
        Event::Taken {
            terminal: id("a"),
            by: None,
        },
    ] {
        roster.apply(&event);
    }
    assert_eq!(names(&roster.background("x", "h", shown_nowhere)), ["a"]);
}
