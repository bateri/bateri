use super::*;
use crate::remote_files::Process;

const ACCOUNT: &str = "u@rasp:22";
const DAY: u64 = 86_400;

fn terminal(name: &str, program: &str, output_at: u64, running: bool) -> Background {
    Background {
        account: ACCOUNT.to_owned(),
        host: "rasp".to_owned(),
        argv: Vec::new(),
        terminal: crate::kablo::SessionId::new(name).expect("an id"),
        program: Some(program.to_owned()),
        cwd: Some("/srv/app".to_owned()),
        label: None,
        started: Some(100),
        front_started: Some(output_at.saturating_sub(720)),
        front_is_own: Some(!running),
        output_at: Some(output_at),
        clients: 0,
        clients_named: Vec::new(),
    }
}

/// Seconds since the epoch → a day running number and a time; a clock at UTC.
fn utc(seconds: u64) -> Local {
    Local {
        day: (seconds / DAY) as i64,
        hour: (seconds % DAY / 3_600) as u8,
        minute: (seconds % 3_600 / 60) as u8,
    }
}

const NOON: u64 = 100 * DAY + 12 * 3_600;

#[test]
fn a_program_in_front_says_for_how_long_and_a_waiting_shell_since_when() {
    let running = terminal("b-1", "claude", NOON, true);
    let found = listing(&[running], NOON, &utc);
    assert_eq!(found.entries[0].title, "claude");
    assert_eq!(found.entries[0].place.as_deref(), Some("/srv/app"));
    assert_eq!(
        found.entries[0].activity.as_deref(),
        Some("running 12m 00s")
    );
    assert!(found.entries[0].running);

    // The shell waiting: the last output, as a time of the day.
    let waiting = terminal("b-2", "zsh", NOON - 3 * 3_600 - 20 * 60, false);
    let found = listing(&[waiting], NOON, &utc);
    assert_eq!(
        found.entries[0].activity.as_deref(),
        Some("idle since 08:40")
    );
    assert!(!found.entries[0].running);
}

#[test]
fn an_old_wait_names_the_day_and_what_the_server_did_not_say_names_nothing() {
    let local = |seconds: u64| utc(seconds);
    let idle = |since| Activity::Idle { since };
    assert_eq!(
        activity_text(idle(Some(NOON - DAY)), NOON, &local).as_deref(),
        Some("idle since yesterday 12:00")
    );
    assert_eq!(
        activity_text(idle(Some(NOON - 3 * DAY)), NOON, &local).as_deref(),
        Some("idle for 3 days")
    );
    // A server whose clock is ahead: not a negative number of days.
    assert_eq!(
        activity_text(idle(Some(NOON + 600)), NOON, &local).as_deref(),
        Some("idle since 12:10")
    );
    assert_eq!(activity_text(idle(None), NOON, &local), None);
    assert_eq!(activity_text(Activity::Unknown, NOON, &local), None);
    // A program in front whose start is not known, or too short to count.
    assert_eq!(
        activity_text(Activity::Running(None), NOON, &local).as_deref(),
        Some("running")
    );
    assert_eq!(
        activity_text(
            Activity::Running(Some(Duration::from_millis(400))),
            NOON,
            &local
        )
        .as_deref(),
        Some("running")
    );
}

#[test]
fn the_card_lists_six_and_says_how_many_more() {
    let many: Vec<Background> = (0..9)
        .map(|n| terminal(&format!("b-{n}"), "claude", NOON - n * 60, true))
        .collect();
    let found = listing(&many, NOON, &utc);
    assert_eq!((found.total, found.entries.len(), found.more), (9, 6, 3));
    assert_eq!(more_text(found.more), "and 3 more");
    assert_eq!(heading(found.total), "Running in the background · 9");
    // The order is the given one: the last to write first.
    assert_eq!(found.entries[0].terminal, "b-0");
    assert_eq!(found.entries[5].terminal, "b-5");
    // Exactly six is not "and 0 more".
    let six = listing(&many[..6], NOON, &utc);
    assert_eq!((six.total, six.entries.len(), six.more), (6, 6, 0));
    assert_eq!(listing(&[], NOON, &utc), Listing::default());
}

#[test]
fn the_card_and_the_palette_name_the_same_things_the_same() {
    let background = terminal("b-1", "claude", NOON, true);
    let row = crate::palette::background_row(&BackgroundFacts::of(&background, NOON));
    assert_eq!(row.hints[0].1, OPEN);
    assert_eq!(
        crate::palette::open_all_row("rasp", 1).title,
        OPEN_ALL,
        "one text for one action"
    );
    assert_eq!(HEADING, crate::palette::BACKGROUND_HEADING);
    // A heading is a noun, never a verb.
    assert_eq!(heading(2), "Running in the background · 2");
}

fn busy(name: &str, tenths: u32) -> Process {
    Process {
        name: name.to_owned(),
        cpu: tenths,
    }
}

#[test]
fn the_load_is_a_bar_for_the_cpu_a_bar_for_the_memory_and_the_busiest() {
    const GIB: u64 = 1 << 30;
    let detail = Detail {
        os: Some("Ubuntu 24.04 LTS".to_owned()),
        cores: Some(4),
        cpu: Some(23),
        mem_used: 3 * GIB + GIB / 10,
        mem_total: 8 * GIB,
        processes: Some(vec![
            busy("node", 412),
            busy("python", 118),
            busy("sshd", 51),
        ]),
        ..Detail::default()
    };
    let card = load("rasp", Some(&detail));
    assert_eq!(card.title, "rasp · Ubuntu 24.04 LTS");
    assert_eq!(card.cpu_label, "CPU · 4 cores");
    assert_eq!(card.cpu.value, "23%");
    assert_eq!(card.cpu.level, StatsLevel::Normal);
    assert!((card.cpu.fill - 0.23).abs() < 1e-9);
    assert_eq!(card.mem.value, "3.1 / 8.0 GB");
    assert_eq!(card.mem.level, StatsLevel::Normal);
    assert_eq!(
        card.busiest.as_deref(),
        Some("Busiest: node 41% · python 12%"),
        "two of them, in whole percents"
    );
}

#[test]
fn the_bars_take_the_threshold_classes_of_the_context_row() {
    const GIB: u64 = 1 << 30;
    let detail = Detail {
        cpu: Some(95),
        mem_used: 7 * GIB,
        mem_total: 8 * GIB,
        ..Detail::default()
    };
    let card = load("rasp", Some(&detail));
    assert_eq!(card.cpu.level, StatsLevel::Critical);
    assert_eq!(card.mem.level, StatsLevel::Warning, "7 of 8 is 88 %");
    assert!((card.mem.fill - 0.875).abs() < 1e-9);
}

#[test]
fn what_is_not_known_yet_says_so() {
    let card = load("rasp", None);
    assert_eq!(card.title, "rasp · —");
    assert_eq!(card.cpu_label, "CPU");
    assert_eq!((card.cpu.value.as_str(), card.cpu.fill), ("—", 0.0));
    assert_eq!(card.mem.value, "—");
    assert_eq!(card.busiest, None, "no sample at all: nothing to measure");
    // A sample without the processes' difference: they are still being measured.
    let first = Detail {
        mem_total: 1 << 30,
        ..Detail::default()
    };
    assert_eq!(
        load("rasp", Some(&first)).busiest.as_deref(),
        Some(MEASURING)
    );
    // Measured, and nothing used the CPU: no line.
    let quiet = Detail {
        processes: Some(Vec::new()),
        ..Detail::default()
    };
    assert_eq!(load("rasp", Some(&quiet)).busiest, None);
}

#[test]
fn memory_is_in_the_totals_unit_and_never_over_a_full_bar() {
    assert_eq!(used_of(100 << 20, 512 << 20), "100 / 512 MB");
    assert_eq!(used_of(0, 0), UNKNOWN);
    assert_eq!(percent(1, 3), Some(33));
    assert_eq!(percent(5, 4), Some(100));
    assert_eq!(percent(1, 0), None);
    assert_eq!(fraction(5, 4), 1.0);
    assert_eq!(fraction(1, 0), 0.0);
    assert_eq!(cpu_label(Some(1)), "CPU · 1 core");
}

#[test]
fn ending_asks_in_the_words_closing_a_pane_does() {
    let running = Entry {
        account: ACCOUNT.to_owned(),
        terminal: "b-1".to_owned(),
        title: "claude".to_owned(),
        place: None,
        activity: Some("running 12m 00s".to_owned()),
        running: true,
    };
    let (title, message) = end_text("rasp", &running);
    assert_eq!(title, "End this terminal?");
    assert_eq!(
        message,
        "\u{201c}claude\u{201d} on rasp is still running. Ending it stops it."
    );
    let waiting = Entry {
        title: "zsh".to_owned(),
        activity: Some("idle since 09:40".to_owned()),
        running: false,
        ..running.clone()
    };
    assert_eq!(
        end_text("rasp", &waiting).1,
        "The terminal on rasp is waiting for a command. Ending it closes it."
    );
    // The server did not say what is in front: the safe words, a program may be running.
    let unknown = Entry {
        activity: None,
        running: false,
        ..running
    };
    assert!(end_text("rasp", &unknown).1.contains("still running"));
}

#[test]
fn a_day_is_counted_on_the_calendar_so_a_new_year_is_one_day_on() {
    let day = |y, m, d| Local::civil(y, m, d, 12, 0).day;
    assert_eq!(day(1970, 1, 1), 0);
    assert_eq!(day(2026, 10, 21) - day(2026, 10, 20), 1);
    assert_eq!(day(2027, 1, 1) - day(2026, 12, 31), 1, "the year turns");
    assert_eq!(
        day(2026, 3, 1) - day(2026, 2, 28),
        1,
        "February of a common year"
    );
    assert_eq!(
        day(2028, 3, 1) - day(2028, 2, 28),
        2,
        "February of a leap year"
    );
    // The same second, read by `utc`, agrees with the civil date.
    let seconds = 20_382 * DAY + 5 * 3_600 + 7 * 60;
    assert_eq!(utc(seconds), Local::civil(2025, 10, 21, 5, 7));
}
