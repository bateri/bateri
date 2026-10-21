//! What the server's card says: the card that opens from `⇄ rasp` in the dock of a pane that
//! shows a terminal the server keeps. Only the words and the numbers, so the platform shells
//! draw one card and `make linux` runs its tests.
//!
//! The card has two parts, and what is in them is **the server's, not the pane's**: the load (one
//! measuring for the server however many panes show it, [`super::remote_stats::Account`]) and the
//! terminals the server keeps that no pane of this Mac shows ([`super::kablo::roster`]). Terminals a
//! tab of this Mac has open are not listed — they are on screen already.
//!
//! - **The load is small**: the CPU and the memory as a bar each and a line naming the busiest
//!   processes ([`load`]). The context row's own thresholds colour the bars
//!   ([`bt_core::StatsMetric::level`]); swap, disk, the load averages and the uptime are not on
//!   the card (disk shows in the dock the moment it is past its threshold).
//! - **The list** ([`listing`]) is *Running in the background · N*, the last to write first, at most
//!   [`MOST_ROWS`] rows and "and 3 more" for the rest. A heading is a noun and the status words
//!   carry no verb; the one verb a row has is the one a click does, written at its right while the
//!   pointer is on it ([`OPEN`]). The row's time is the palette's own text: how long a program has
//!   been in front is [`bt_core::running_counter`], and a shell waiting says since when.
//! - **Ending a terminal** asks in the words closing a pane does ([`end_text`]).

use std::time::Duration;

use bt_core::{StatsLevel, StatsMetric, running_counter};

use crate::kablo::roster::{Activity, Background};
use crate::palette::BackgroundFacts;
use crate::remote_stats::Detail;

/// The heading over the list; the number follows it ([`heading`]). The palette's cluster of the
/// same terminals has the same name.
pub const HEADING: &str = crate::palette::BACKGROUND_HEADING;
/// The list's header button: every terminal in the list, together, in a new tab.
pub const OPEN_ALL: &str = "Open All in New Tab";
/// What a click on a row does, written at its right while the pointer is on it.
pub const OPEN: &str = "Open in New Tab";
/// The row menu's one item.
pub const END: &str = "End Terminal";
/// The list when nothing runs in the background.
pub const EMPTY: &str = "Nothing is running in the background.";
/// What the load shows for a value not known yet: an empty one would read as "nothing", not
/// "not yet".
pub const UNKNOWN: &str = "—";
/// The busiest line before the second process scan: CPU is a difference.
pub const MEASURING: &str = "Measuring…";
/// How many terminals the card lists before it says "and 3 more".
pub const MOST_ROWS: usize = 6;
/// How many processes the busiest line names.
const BUSIEST: usize = 2;

/// A moment of the server's clock as a person here reads it: which day (a running number, only
/// compared) and the time of the day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Local {
    pub day: i64,
    pub hour: u8,
    pub minute: u8,
}

impl Local {
    /// The moment of a calendar date and a time of the day, as the local clock reads it (the
    /// platform shell splits a second into these): the day is counted from the civil calendar,
    /// so that a month's and a year's turn is one day, not a jump.
    pub fn civil(year: i32, month: u32, day: u32, hour: u8, minute: u8) -> Local {
        // Days from 1970-01-01 (Howard Hinnant's `days_from_civil`).
        let y = i64::from(year) - i64::from(month <= 2);
        let era = y.div_euclid(400);
        let year_of_era = y.rem_euclid(400);
        let shifted_month = i64::from((month + 9) % 12);
        let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        Local {
            day: era * 146_097 + day_of_era - 719_468,
            hour,
            minute,
        }
    }
}

/// A terminal on the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The server's account and the terminal's name on it: what the row acts on.
    pub account: String,
    pub terminal: String,
    /// What the row is called: the terminal's label, else the program in front.
    pub title: String,
    /// The directory of the program in front, on the server.
    pub place: Option<String>,
    /// `running 12m 00s`, `idle since 09:40`; `None` when the server did not say.
    pub activity: Option<String>,
    /// A program other than the shell is in front.
    pub running: bool,
}

/// The list as the card shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// How many there are in all.
    pub total: usize,
    /// The first [`MOST_ROWS`], in the order given.
    pub entries: Vec<Entry>,
    /// How many more there are than rows.
    pub more: usize,
}

/// The list of `backgrounds` (one account's, the last to write first) at `now` (seconds since the
/// Unix epoch); `local` says when a second of the server's clock is, here.
pub fn listing(backgrounds: &[Background], now: u64, local: &dyn Fn(u64) -> Local) -> Listing {
    let entries: Vec<Entry> = backgrounds
        .iter()
        .take(MOST_ROWS)
        .map(|background| {
            let facts = BackgroundFacts::of(background, now);
            let activity = background.activity(now);
            Entry {
                account: facts.account,
                terminal: facts.terminal,
                title: facts.title,
                place: facts.place,
                activity: activity_text(activity, now, local),
                running: matches!(activity, Activity::Running(_)),
            }
        })
        .collect();
    Listing {
        total: backgrounds.len(),
        more: backgrounds.len().saturating_sub(entries.len()),
        entries,
    }
}

/// `Running in the background · 3`.
pub fn heading(total: usize) -> String {
    format!("{HEADING} · {total}")
}

/// `and 3 more`.
pub fn more_text(more: usize) -> String {
    format!("and {more} more")
}

/// What a row says at its right: how long a program has been in front, or since when the shell has
/// been waiting.
pub fn activity_text(activity: Activity, now: u64, local: &dyn Fn(u64) -> Local) -> Option<String> {
    match activity {
        Activity::Running(time) => Some(
            match time
                .filter(|time| *time > Duration::ZERO)
                .and_then(running_counter)
            {
                Some(time) => format!("running {time}"),
                None => "running".to_owned(),
            },
        ),
        Activity::Idle { since: Some(since) } => {
            let (today, then) = (local(now), local(since));
            Some(match today.day - then.day {
                ..=0 => format!("idle since {:02}:{:02}", then.hour, then.minute),
                1 => format!("idle since yesterday {:02}:{:02}", then.hour, then.minute),
                days => format!("idle for {days} days"),
            })
        }
        Activity::Idle { since: None } | Activity::Unknown => None,
    }
}

/// A bar and the number beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Meter {
    pub value: String,
    /// How full the bar is, `0..=1`.
    pub fill: f64,
    /// The context row's threshold class: the bar's colour.
    pub level: StatsLevel,
}

impl Meter {
    fn unknown() -> Meter {
        Meter {
            value: UNKNOWN.to_owned(),
            fill: 0.0,
            level: StatsLevel::Normal,
        }
    }
}

/// The card's load, from the last sample.
#[derive(Clone, Debug, PartialEq)]
pub struct Load {
    /// `rasp · Ubuntu 24.04 LTS`.
    pub title: String,
    /// `CPU · 4 cores`; the cores come only with a `detail` sample.
    pub cpu_label: String,
    pub cpu: Meter,
    pub mem: Meter,
    /// `Busiest: node 41% · python 12%`; `Measuring…` before the second scan, nothing when nothing
    /// used the CPU.
    pub busiest: Option<String>,
}

/// The load of the server `host` from `detail` (`None` before the first sample).
pub fn load(host: &str, detail: Option<&Detail>) -> Load {
    let sampled = detail.is_some();
    let empty = Detail::default();
    let detail = detail.unwrap_or(&empty);
    let cpu = detail.cpu.map_or_else(Meter::unknown, |cpu| Meter {
        value: format!("{cpu}%"),
        fill: f64::from(cpu) / 100.0,
        level: StatsMetric::Cpu.level(cpu),
    });
    let mem = percent(detail.mem_used, detail.mem_total).map_or_else(Meter::unknown, |pct| Meter {
        value: used_of(detail.mem_used, detail.mem_total),
        fill: fraction(detail.mem_used, detail.mem_total),
        level: StatsMetric::Mem.level(pct),
    });
    Load {
        title: format!("{host} · {}", detail.os.as_deref().unwrap_or(UNKNOWN)),
        cpu_label: cpu_label(detail.cores),
        cpu,
        mem,
        busiest: busiest(sampled, detail.processes.as_deref()),
    }
}

/// `Busiest: node 41% · python 12%`: the processes that used the CPU since the previous scan.
/// Before that scan there is none to compare: "Measuring…", and without any sample nothing.
fn busiest(sampled: bool, processes: Option<&[crate::remote_files::Process]>) -> Option<String> {
    match processes {
        None => sampled.then(|| MEASURING.to_owned()),
        Some([]) => None,
        Some(list) => Some(format!(
            "Busiest: {}",
            list.iter()
                .take(BUSIEST)
                .map(|process| format!("{} {}%", process.name, (process.cpu + 5) / 10))
                .collect::<Vec<_>>()
                .join(" · ")
        )),
    }
}

/// `CPU · N cores`.
fn cpu_label(cores: Option<u32>) -> String {
    match cores {
        Some(1) => "CPU · 1 core".to_owned(),
        Some(cores) => format!("CPU · {cores} cores"),
        None => "CPU".to_owned(),
    }
}

/// `used / total` in the total's unit (`3.1 / 7.8 GB`); [`UNKNOWN`] without a total.
fn used_of(used: u64, total: u64) -> String {
    if total == 0 {
        return UNKNOWN.to_owned();
    }
    const MIB: u64 = 1 << 20;
    const GIB: u64 = 1 << 30;
    let (unit, name) = if total >= GIB {
        (GIB, "GB")
    } else {
        (MIB, "MB")
    };
    let scaled = |bytes: u64| {
        // audit: the sizes are a server's memory; `f64`'s 53 bits are exact up to 8 PiB.
        let value = bytes as f64 / unit as f64;
        if unit >= GIB {
            format!("{value:.1}")
        } else {
            format!("{value:.0}")
        }
    };
    format!("{} / {} {name}", scaled(used), scaled(total))
}

/// `used / total` as a rounded percentage; `None` without a total.
fn percent(used: u64, total: u64) -> Option<u8> {
    (total > 0).then(|| {
        let pct = (u128::from(used) * 100 + u128::from(total) / 2) / u128::from(total);
        // audit: clamped to 100 before the narrowing.
        pct.min(100) as u8
    })
}

/// `used / total` as a bar fraction.
fn fraction(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        // audit: the sizes are a server's memory; the ratio's precision is plenty.
        (used as f64 / total as f64).clamp(0.0, 1.0)
    }
}

/// The question before a terminal is ended from the card, in the words of the one that closing a
/// pane asks: `(title, message)`. The confirm button is [`END`].
pub fn end_text(host: &str, entry: &Entry) -> (String, String) {
    let message = if entry.running || entry.activity.is_none() {
        format!(
            "\u{201c}{}\u{201d} on {host} is still running. Ending it stops it.",
            entry.title
        )
    } else {
        format!("The terminal on {host} is waiting for a command. Ending it closes it.")
    };
    ("End this terminal?".to_owned(), message)
}

#[cfg(test)]
mod tests;
