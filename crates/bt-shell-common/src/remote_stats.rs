//! The remote host's load for the ssh status bar (046): what the samples mean
//! and when they are taken — both pure, so `make linux` runs them too.
//!
//! - **[`Sampler`]** turns the helper's raw readings
//!   ([`crate::remote_files::LoadSample`]) into the value the context row
//!   draws ([`RemoteStats`]: rounded percentages, the sparkline's levels) and
//!   the popover's [`Detail`]. CPU is the difference of two readings, so the
//!   first sample after a (re)start has none.
//! - **[`Schedule`]** says when a sample is taken: events in (the remote
//!   generation, the form, visibility, interaction, a tick, a reply, the
//!   popover), actions out (arm a tick with a token, send a request, hide the
//!   indicator). The clock is an argument; the platform shell runs the actions
//!   (`dispatch`'s `after` cannot be cancelled, hence the token).
//!
//! The rationale is in `.tasks/046-uzak-yuk-gostergesi/discussion.md` → Karar 1
//! (the helper's request, the two kinds of failure), Karar 2 (what is measured
//! and how) and Karar 6 (when sampling runs and stops).

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use bt_core::{RemoteStats, STATS_HISTORY, StatsForm};

use crate::remote_files::{CpuCounters, LoadSample, Process, ProcessScan};

/// How many processes the popover lists.
pub const TOP_PROCESSES: usize = 3;

/// With no interaction for this long sampling pauses and the indicator keeps
/// its last value (046 Karar 6). A **design constant**, not a measurement:
/// long enough for reading a log, short enough that a forgotten window does
/// not sample a server all night.
pub const STATS_IDLE: Duration = Duration::from_secs(120);

/// After a sample without CPU (the first one: CPU is a difference) the next
/// comes this soon rather than a whole interval later (046 Karar 2). Design
/// constant — the reason it is not a `sleep 1` inside the script is that the
/// helper's worker is serial and a ⌘-hover would wait behind it.
pub const FIRST_FOLLOW: Duration = Duration::from_secs(1);

// ─── sampler ─────────────────────────────────────────────────────────────

/// The popover's content (046 Karar 7), from one sample. Sizes in bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Detail {
    /// `PRETTY_NAME`; only on a `detail` request.
    pub os: Option<String>,
    /// Only on a `detail` request.
    pub cores: Option<u32>,
    /// CPU %, rounded; `None` on the first sample.
    pub cpu: Option<u8>,
    /// The 1/5/15 minute load averages, in hundredths.
    pub load: Option<[u32; 3]>,
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    /// The root file system's use, %.
    pub disk: Option<u8>,
    /// Seconds since boot.
    pub uptime: Option<u64>,
    /// The top [`TOP_PROCESSES`] by CPU since the previous scan
    /// ([`Sampler`]); `None` while there is no difference yet — the first
    /// `detail` sample after the popover opened, and every plain one.
    /// `Some(empty)`: measured, and nothing used the CPU.
    pub processes: Option<Vec<Process>>,
}

/// What one sample gives: the context row's value and the popover's content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reading {
    pub stats: RemoteStats,
    pub detail: Detail,
}

/// The previous CPU counters, the previous process scan and the sparkline's
/// history (046 Karar 2, 4).
#[derive(Debug, Default)]
pub struct Sampler {
    previous: Option<CpuCounters>,
    /// The last `detail` sample's per-process ticks; a sample without a scan
    /// (the popover closed) drops it, so a reopened popover never shows a
    /// difference minutes wide.
    scan: Option<ScanBase>,
    /// The levels, oldest first; kept in every form, so switching to
    /// `sparkline` does not start empty — but only that form carries it out.
    history: [u8; STATS_HISTORY],
    len: usize,
}

impl Sampler {
    /// Forgets the counters and the history: sampling resumed after a pause,
    /// and a gapped time axis would read as a continuous graph (Karar 6).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// One sample → what is drawn, in `form`.
    pub fn take(&mut self, sample: &LoadSample, form: StatsForm) -> Reading {
        let processes = self.processes(sample);
        let cpu = self.cpu(sample.cpu);
        if let Some(cpu) = cpu {
            self.push(level(cpu));
        }
        let mem_used = sample.mem_total.saturating_sub(sample.mem_available);
        let mut stats = RemoteStats {
            form,
            cpu,
            mem: percent(mem_used, sample.mem_total),
            disk: sample.disk.unwrap_or(0),
            ..RemoteStats::default()
        };
        if form == StatsForm::Sparkline {
            stats.history = self.history;
            stats.len = self.len as u8;
        }
        let detail = Detail {
            os: sample.os.clone(),
            cores: sample.cores,
            cpu,
            load: sample.load,
            mem_used,
            mem_total: sample.mem_total,
            swap_used: sample.swap_total.saturating_sub(sample.swap_free),
            swap_total: sample.swap_total,
            disk: sample.disk,
            uptime: sample.uptime,
            processes,
        };
        Reading { stats, detail }
    }

    /// The top processes since the previous scan — `top`'s method: each
    /// process's `utime + stime` difference over the elapsed time, one core =
    /// 100 % (`top`'s default **Irix mode**, so a busy multi-threaded process
    /// can pass 100 %). The elapsed time is the aggregate `cpu` line's
    /// difference divided by the cores (it sums every core); with the core
    /// count unknown it is not divided, i.e. the whole machine = 100 %
    /// (Solaris mode). `ps`'s `pcpu` is not used: it is the average over the
    /// process's **lifetime** (a short-lived `ps` showed 2200 %).
    ///
    /// Left out: our own measuring (the helper's `sh` and its descendants —
    /// by process tree, not by name: our `awk` and the user's look alike),
    /// a process with no base (started since, or its PID reused — the
    /// identity is PID + `starttime`), a counter that went back and a zero
    /// difference. Zombies never arrive (`PROC_AWK`).
    fn processes(&mut self, sample: &LoadSample) -> Option<Vec<Process>> {
        let Some(scan) = &sample.scan else {
            self.scan = None;
            return None;
        };
        let next = ScanBase {
            total: sample.cpu.total,
            ticks: scan
                .tasks
                .iter()
                .map(|task| ((task.pid, task.start), task.ticks))
                .collect(),
        };
        let base = self.scan.replace(next)?;
        let elapsed = sample
            .cpu
            .total
            .checked_sub(base.total)
            .filter(|&n| n > 0)?;
        Some(top_processes(
            scan,
            &base.ticks,
            elapsed,
            sample.cores.unwrap_or(1),
        ))
    }

    /// CPU % since the previous reading; `None` on the first one and when a
    /// counter went back (a rebooted server, an overflow) — that difference
    /// is thrown away and the new reading is the next one's base.
    fn cpu(&mut self, now: CpuCounters) -> Option<u8> {
        let before = self.previous.replace(now)?;
        let total = now.total.checked_sub(before.total)?;
        let idle = now.idle.checked_sub(before.idle)?;
        if total == 0 {
            return None;
        }
        Some(percent(total.saturating_sub(idle), total))
    }

    fn push(&mut self, level: u8) {
        if self.len == STATS_HISTORY {
            self.history.copy_within(1.., 0);
            self.history[STATS_HISTORY - 1] = level;
        } else {
            self.history[self.len] = level;
            self.len += 1;
        }
    }
}

/// One process scan's ticks by identity (PID, `starttime`) and the aggregate
/// CPU total at that moment.
#[derive(Debug)]
struct ScanBase {
    total: u64,
    ticks: HashMap<(u32, u64), u64>,
}

/// The [`TOP_PROCESSES`] largest differences against `before`, in tenths of a
/// percent of one core; `elapsed` is the aggregate CPU difference (all cores'
/// jiffies). See [`Sampler::processes`] for what is left out.
fn top_processes(
    scan: &ProcessScan,
    before: &HashMap<(u32, u64), u64>,
    elapsed: u64,
    cores: u32,
) -> Vec<Process> {
    let ours = own_tree(scan);
    let mut top: Vec<Process> = scan
        .tasks
        .iter()
        .filter(|task| !ours.contains(&task.pid) && !task.name.is_empty())
        .filter_map(|task| {
            let delta = task
                .ticks
                .checked_sub(*before.get(&(task.pid, task.start))?)?;
            let elapsed = u128::from(elapsed);
            let tenths =
                (u128::from(delta) * 1000 * u128::from(cores.max(1)) + elapsed / 2) / elapsed;
            let cpu = u32::try_from(tenths).unwrap_or(u32::MAX);
            (cpu > 0).then(|| Process {
                name: task.name.clone(),
                cpu,
            })
        })
        .collect();
    top.sort_by(|a, b| b.cpu.cmp(&a.cpu).then_with(|| a.name.cmp(&b.name)));
    top.truncate(TOP_PROCESSES);
    top
}

/// The helper's `sh` and every descendant (the scan's `awk`; any pipeline the
/// script grows later). Empty without a `self` line.
fn own_tree(scan: &ProcessScan) -> HashSet<u32> {
    let mut ours = HashSet::new();
    let Some(root) = scan.self_pid else {
        return ours;
    };
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for task in &scan.tasks {
        children.entry(task.ppid).or_default().push(task.pid);
    }
    let mut stack = vec![root];
    while let Some(pid) = stack.pop() {
        if ours.insert(pid)
            && let Some(kids) = children.get(&pid)
        {
            stack.extend(kids);
        }
    }
    ours
}

/// `part / whole` as a rounded percentage, `0..=100`; 0 for an empty whole.
fn percent(part: u64, whole: u64) -> u8 {
    if whole == 0 {
        return 0;
    }
    let part = u128::from(part.min(whole));
    let whole = u128::from(whole);
    ((part * 100 + whole / 2) / whole) as u8
}

/// A CPU % → the sparkline's level, `⌊v / 12.5⌋` clipped to 7 (U+2581 + level).
fn level(cpu: u8) -> u8 {
    (u16::from(cpu) * 2 / 25).min(7) as u8
}

// ─── schedule ────────────────────────────────────────────────────────────

/// What the platform shell does next ([`Schedule`]'s output).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Call [`Schedule::tick`] with `token` after `after`. An earlier token
    /// is stale by then — `after` cannot be cancelled.
    Arm { token: u64, after: Duration },
    /// Send a [`crate::remote_helper::Query::Load`] now. `restart`: reset the
    /// [`Sampler`] first (sampling resumed after a pause).
    Request { detail: bool, restart: bool },
    /// Take the indicator away (`set_remote_stats(…, None)`).
    Hide,
}

/// How a load request ended, as the schedule needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A sample; `cpu` — whether it carried a CPU % (the first does not).
    Sample { cpu: bool },
    /// `BT-NOPROC`: no Linux `/proc` on this server.
    NoProc,
    /// The helper session could not be opened (Karar 1: no retry in this
    /// generation — a password prompt, an unreachable host).
    Unreachable,
    /// The open session failed (closed, timed out, unreadable answer).
    Failed,
}

/// When samples are taken (046 Karar 6). Sampling runs while a remote
/// generation is on, the form is not `off`, the pane is visible, the last
/// interaction is younger than [`STATS_IDLE`] and the generation has not
/// ended sampling (a failed open, `BT-NOPROC`, a second failure in a row).
/// At most one request is in flight; one wanted meanwhile goes out at the
/// reply.
///
/// A pause the tick discovers (idle) just stops arming; an event that stops
/// sampling (generation, form, visibility) also makes the armed token stale.
/// Resuming takes the first sample at once and restarts the history.
#[derive(Debug)]
pub struct Schedule {
    generation: Option<u64>,
    on: bool,
    interval: Duration,
    visible: bool,
    interaction: Option<Instant>,
    detail: bool,
    /// Sampling ended for this generation.
    ended: bool,
    /// The open session's failure was already retried once.
    retried: bool,
    in_flight: bool,
    /// A request is wanted as soon as the one in flight answers.
    wanted: Option<bool>,
    token: u64,
    armed: bool,
}

impl Schedule {
    /// A schedule with the settings' form (`on` — not `off`) and interval;
    /// nothing runs until a generation starts.
    pub fn new(on: bool, interval: Duration) -> Self {
        Self {
            generation: None,
            on,
            interval,
            visible: true,
            interaction: None,
            detail: false,
            ended: false,
            retried: false,
            in_flight: false,
            wanted: None,
            token: 0,
            armed: false,
        }
    }

    /// Whether sampling runs at `now`.
    pub fn running(&self, now: Instant) -> bool {
        self.generation.is_some()
            && self.on
            && self.visible
            && !self.ended
            && self
                .interaction
                .is_some_and(|at| now.saturating_duration_since(at) < STATS_IDLE)
    }

    /// The remote generation the samples belong to; `None` while no remote
    /// session runs.
    pub fn generation(&self) -> Option<u64> {
        self.generation
    }

    /// A remote session started (`Some`) or ended (`None`). A new generation
    /// starts clean and counts as an interaction: the user just connected.
    pub fn set_generation(&mut self, now: Instant, generation: Option<u64>) -> Vec<Action> {
        if generation == self.generation {
            return Vec::new();
        }
        // Fresh: running on into another generation still takes its first
        // sample at once, and the old generation's tick goes stale.
        self.change(now, true, |schedule| {
            schedule.generation = generation;
            schedule.ended = false;
            schedule.retried = false;
            if generation.is_some() {
                schedule.interaction = Some(now);
            }
        })
    }

    /// The settings changed: `on` is "not `off`". Turning off hides the
    /// indicator; a new interval holds from the next tick.
    pub fn set_form(&mut self, now: Instant, on: bool, interval: Duration) -> Vec<Action> {
        self.interval = interval;
        let mut actions = self.change(now, false, |schedule| schedule.on = on);
        if !on && self.generation.is_some() {
            actions.push(Action::Hide);
        }
        actions
    }

    /// The pane became visible or covered (a background tab, a minimised
    /// window, behind a zoomed split).
    pub fn set_visible(&mut self, now: Instant, visible: bool) -> Vec<Action> {
        self.change(now, false, |schedule| schedule.visible = visible)
    }

    /// A key, a click, the wheel, a mouse move or the window becoming key.
    /// Called at mouse-move rate: while sampling runs it only stamps the time.
    /// So does an interaction after [`STATS_IDLE`] that the tick has not yet
    /// found (a tick armed or a request in flight): there was no gap in the
    /// samples, so the history is not restarted (`/code-review`).
    pub fn interaction(&mut self, now: Instant) -> Vec<Action> {
        if self.running(now) || self.armed || self.in_flight {
            self.interaction = Some(now);
            return Vec::new();
        }
        self.change(now, false, |schedule| schedule.interaction = Some(now))
    }

    /// The popover opened or closed. Opening asks for the details at once
    /// rather than at the next tick (after the request in flight, if any).
    pub fn set_detail(&mut self, now: Instant, open: bool) -> Vec<Action> {
        self.detail = open;
        if !open || !self.running(now) {
            return Vec::new();
        }
        self.disarm();
        self.request(false)
    }

    /// An armed tick fired. A stale token does nothing; a paused schedule
    /// arms nothing more.
    pub fn tick(&mut self, now: Instant, token: u64) -> Vec<Action> {
        if !self.armed || token != self.token {
            return Vec::new();
        }
        self.armed = false;
        if !self.running(now) {
            return Vec::new();
        }
        self.request(false)
    }

    /// A request of `generation` answered. Another generation's reply only
    /// frees the worker.
    pub fn answered(&mut self, now: Instant, generation: u64, outcome: Outcome) -> Vec<Action> {
        self.in_flight = false;
        let current = self.generation == Some(generation);
        let mut actions = Vec::new();
        if current {
            match outcome {
                Outcome::Sample { cpu } => {
                    self.retried = false;
                    if self.running(now) && self.wanted.is_none() {
                        let after = if cpu { self.interval } else { FIRST_FOLLOW };
                        actions.push(self.arm(after));
                    }
                }
                Outcome::NoProc | Outcome::Unreachable => {
                    self.end();
                    actions.push(Action::Hide);
                }
                Outcome::Failed if self.retried => {
                    self.end();
                    actions.push(Action::Hide);
                }
                Outcome::Failed => {
                    self.retried = true;
                    actions.push(Action::Hide);
                    if self.running(now) && self.wanted.is_none() {
                        actions.push(self.arm(self.interval));
                    }
                }
            }
        }
        if let Some(restart) = self.wanted.take()
            && self.running(now)
        {
            actions.extend(self.request(restart));
        }
        actions
    }

    /// Applies `edit`; if sampling started — or, `fresh`, runs on into a new
    /// start — the first request goes out now (with a fresh history); if it
    /// stopped, the armed tick goes stale.
    fn change(&mut self, now: Instant, fresh: bool, edit: impl FnOnce(&mut Self)) -> Vec<Action> {
        let before = self.running(now);
        edit(self);
        let after = self.running(now);
        match (before, after) {
            (false, true) => {
                self.disarm();
                self.request(true)
            }
            (true, true) if fresh => {
                self.disarm();
                self.request(true)
            }
            (true, false) => {
                self.disarm();
                self.wanted = None;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// A request now, or as soon as the one in flight answers.
    fn request(&mut self, restart: bool) -> Vec<Action> {
        if self.in_flight {
            self.wanted = Some(self.wanted.unwrap_or(false) || restart);
            return Vec::new();
        }
        self.in_flight = true;
        vec![Action::Request {
            detail: self.detail,
            restart,
        }]
    }

    fn arm(&mut self, after: Duration) -> Action {
        self.token += 1;
        self.armed = true;
        Action::Arm {
            token: self.token,
            after,
        }
    }

    fn disarm(&mut self) {
        self.token += 1;
        self.armed = false;
    }

    /// Sampling ends for this generation (Karar 1, R4.2).
    fn end(&mut self) {
        self.ended = true;
        self.wanted = None;
        self.disarm();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_files::Task;

    fn sample(total: u64, idle: u64) -> LoadSample {
        LoadSample {
            cpu: CpuCounters { total, idle },
            mem_total: 1000,
            mem_available: 390,
            swap_total: 100,
            swap_free: 75,
            disk: Some(54),
            ..LoadSample::default()
        }
    }

    #[test]
    fn cpu_is_the_difference_of_two_readings_rounded() {
        let mut sampler = Sampler::default();
        let first = sampler.take(&sample(1000, 800), StatsForm::Sparkline);
        assert_eq!(first.stats.cpu, None);
        assert_eq!(first.stats.history(), &[] as &[u8]);
        assert_eq!(first.stats.mem, 61);
        assert_eq!(first.stats.disk, 54);
        assert_eq!(first.detail.mem_used, 610);
        assert_eq!(first.detail.swap_used, 25);
        // 200 jiffies, 47 idle → 153 busy → 76.5 % → 77.
        let second = sampler.take(&sample(1200, 847), StatsForm::Sparkline);
        assert_eq!(second.stats.cpu, Some(77));
        assert_eq!(second.detail.cpu, Some(77));
        assert_eq!(second.stats.history(), &[6]);
        // 0.4 % rounds down.
        let third = sampler.take(&sample(2200, 1843), StatsForm::Sparkline);
        assert_eq!(third.stats.cpu, Some(0));
    }

    #[test]
    fn the_level_is_an_eighth_of_the_range() {
        let levels: Vec<u8> = [0, 12, 13, 24, 25, 37, 38, 50, 62, 63, 75, 87, 88, 99, 100]
            .into_iter()
            .map(level)
            .collect();
        assert_eq!(levels, [0, 0, 1, 1, 2, 2, 3, 4, 4, 5, 6, 6, 7, 7, 7]);
    }

    #[test]
    fn the_history_keeps_the_last_eight_oldest_first() {
        let mut sampler = Sampler::default();
        let mut total = 0;
        let mut idle = 0;
        sampler.take(&sample(total, idle), StatsForm::Sparkline);
        let mut last = None;
        // CPU 0, 10, …, 90 %: ten samples.
        for busy in (0..10).map(|n| n * 10) {
            total += 100;
            idle += 100 - busy;
            last = Some(sampler.take(&sample(total, idle), StatsForm::Sparkline));
        }
        let stats = last.expect("ten samples").stats;
        assert_eq!(usize::from(stats.len), STATS_HISTORY);
        // 20 % … 90 %.
        assert_eq!(stats.history(), &[1, 2, 3, 4, 4, 5, 6, 7]);
        // Another form carries no history, but it is kept.
        total += 100;
        let numbers = sampler.take(&sample(total, idle), StatsForm::Numbers).stats;
        assert_eq!(numbers.len, 0);
        assert_eq!(numbers.cpu, Some(100));
        total += 100;
        let back = sampler
            .take(&sample(total, idle), StatsForm::Sparkline)
            .stats;
        assert_eq!(back.history(), &[3, 4, 4, 5, 6, 7, 7, 7]);
        sampler.reset();
        let fresh = sampler
            .take(&sample(total, idle), StatsForm::Sparkline)
            .stats;
        assert_eq!((fresh.cpu, fresh.len), (None, 0));
    }

    #[test]
    fn a_counter_that_goes_back_gives_no_cpu_once() {
        let mut sampler = Sampler::default();
        sampler.take(&sample(5000, 4000), StatsForm::Sparkline);
        // A rebooted server: the counters start over.
        assert_eq!(
            sampler
                .take(&sample(300, 200), StatsForm::Sparkline)
                .stats
                .cpu,
            None
        );
        // Idle went back alone (an iowait quirk): no CPU either.
        assert_eq!(
            sampler
                .take(&sample(400, 100), StatsForm::Sparkline)
                .stats
                .cpu,
            None
        );
        // No time passed: no difference to divide.
        assert_eq!(
            sampler
                .take(&sample(400, 100), StatsForm::Sparkline)
                .stats
                .cpu,
            None
        );
        assert_eq!(
            sampler
                .take(&sample(500, 150), StatsForm::Sparkline)
                .stats
                .cpu,
            Some(50)
        );
    }

    fn task(pid: u32, ppid: u32, start: u64, ticks: u64, name: &str) -> Task {
        Task {
            pid,
            ppid,
            start,
            ticks,
            name: name.to_owned(),
        }
    }

    /// A `detail` sample at CPU total `total` on four cores, `self` = 50.
    fn scanned(total: u64, tasks: Vec<Task>) -> LoadSample {
        LoadSample {
            cores: Some(4),
            scan: Some(ProcessScan {
                self_pid: Some(50),
                tasks,
            }),
            ..sample(total, 0)
        }
    }

    fn top(reading: &Reading) -> Vec<(&str, u32)> {
        reading
            .detail
            .processes
            .as_deref()
            .expect("measured")
            .iter()
            .map(|process| (process.name.as_str(), process.cpu))
            .collect()
    }

    #[test]
    fn process_cpu_is_the_difference_of_two_scans_one_core_a_hundred() {
        let mut sampler = Sampler::default();
        let first = sampler.take(
            &scanned(
                1000,
                vec![
                    task(10, 1, 7, 500, "postgres"),
                    task(11, 1, 7, 100, "btop"),
                    task(12, 1, 7, 900, "idle one"),
                    task(13, 1, 7, 40, "nginx"),
                    task(14, 1, 7, 0, "java"),
                    task(15, 1, 7, 0, "new pid"),
                ],
            ),
            StatsForm::Sparkline,
        );
        assert_eq!(first.detail.processes, None, "one scan: measuring");
        // 400 jiffies over four cores = 100 wall ticks.
        let second = sampler.take(
            &scanned(
                1400,
                vec![
                    task(10, 1, 7, 650, "postgres"), // 150 → 150 %
                    task(11, 1, 7, 105, "btop"),     // 5 → 5 %
                    task(12, 1, 7, 900, "idle one"), // 0 → left out
                    task(13, 1, 7, 41, "nginx"),     // 1 → 1 %
                    task(14, 1, 7, 2, "java"),       // 2 → 2 %
                    task(15, 1, 9, 80, "new pid"),   // another start: no base
                    task(16, 1, 9, 80, "born"),      // no base
                ],
            ),
            StatsForm::Sparkline,
        );
        assert_eq!(
            top(&second),
            [("postgres", 1500), ("btop", 50), ("java", 20)]
        );
        // A plain sample (the popover closed) drops the base: no difference
        // minutes wide on the next open.
        let plain = sampler.take(&sample(1800, 0), StatsForm::Sparkline);
        assert_eq!(plain.detail.processes, None);
        let reopened = sampler.take(
            &scanned(9000, vec![task(10, 1, 7, 9000, "postgres")]),
            StatsForm::Sparkline,
        );
        assert_eq!(reopened.detail.processes, None, "measuring again");
    }

    #[test]
    fn our_own_measuring_is_not_the_servers_load() {
        let mut sampler = Sampler::default();
        let tasks = |ticks: u64| {
            vec![
                task(50, 40, 1, ticks, "sh"),       // the helper (`self`)
                task(51, 50, 1, ticks, "awk"),      // its scan
                task(52, 51, 1, ticks, "cat"),      // a grandchild
                task(40, 1, 1, ticks / 10, "sshd"), // its parent stays
                task(60, 1, 1, ticks, "awk"),       // the user's awk stays
            ]
        };
        sampler.take(&scanned(0, tasks(0)), StatsForm::Sparkline);
        let reading = sampler.take(&scanned(400, tasks(30)), StatsForm::Sparkline);
        assert_eq!(top(&reading), [("awk", 300), ("sshd", 30)]);
    }

    #[test]
    fn a_scan_without_elapsed_time_or_cores_is_still_safe() {
        let mut sampler = Sampler::default();
        let busy = |ticks| vec![task(10, 1, 7, ticks, "postgres")];
        sampler.take(&scanned(1000, busy(0)), StatsForm::Sparkline);
        // No time passed (or the counters went back): no difference yet.
        let still = sampler.take(&scanned(1000, busy(50)), StatsForm::Sparkline);
        assert_eq!(still.detail.processes, None);
        // A counter that went back is left out, not a wrap.
        let back = sampler.take(&scanned(1400, busy(10)), StatsForm::Sparkline);
        assert_eq!(top(&back), []);
        // Unknown cores: the whole machine = 100 % (400 jiffies, 100 used → 25 %).
        let mut unknown = scanned(1800, busy(110));
        unknown.cores = None;
        assert_eq!(
            top(&sampler.take(&unknown, StatsForm::Sparkline)),
            [("postgres", 250)]
        );
    }

    #[test]
    fn an_empty_memory_reading_is_zero_not_a_panic() {
        let mut sampler = Sampler::default();
        let reading = sampler.take(&LoadSample::default(), StatsForm::Alerts);
        assert_eq!((reading.stats.mem, reading.stats.disk), (0, 0));
        assert_eq!(reading.detail.disk, None);
        // Available above total (a kernel's rounding) is 0 %, not a wrap.
        let odd = LoadSample {
            mem_total: 100,
            mem_available: 120,
            ..LoadSample::default()
        };
        assert_eq!(sampler.take(&odd, StatsForm::Numbers).stats.mem, 0);
    }

    // ─── schedule ───

    const INTERVAL: Duration = Duration::from_secs(3);

    fn request(restart: bool) -> Action {
        Action::Request {
            detail: false,
            restart,
        }
    }

    /// A schedule with generation 7 started at `t0`: the first request is out.
    fn started(t0: Instant) -> Schedule {
        let mut schedule = Schedule::new(true, INTERVAL);
        assert_eq!(schedule.set_generation(t0, Some(7)), [request(true)]);
        schedule
    }

    fn armed(actions: &[Action]) -> (u64, Duration) {
        match actions {
            [Action::Arm { token, after }] => (*token, *after),
            other => panic!("expected one Arm, got {other:?}"),
        }
    }

    #[test]
    fn the_first_sample_is_at_once_and_the_cpu_follows_soon() {
        let t0 = Instant::now();
        let mut schedule = started(t0);
        // The first reply has no CPU: the second comes after FIRST_FOLLOW.
        let (token, after) = armed(&schedule.answered(t0, 7, Outcome::Sample { cpu: false }));
        assert_eq!(after, FIRST_FOLLOW);
        let t1 = t0 + after;
        assert_eq!(schedule.tick(t1, token), [request(false)]);
        // With CPU, the interval.
        let (token, after) = armed(&schedule.answered(t1, 7, Outcome::Sample { cpu: true }));
        assert_eq!(after, INTERVAL);
        assert_eq!(schedule.tick(t1 + after, token), [request(false)]);
    }

    #[test]
    fn nothing_runs_without_a_remote_generation_or_with_off() {
        let t0 = Instant::now();
        let mut schedule = Schedule::new(false, INTERVAL);
        assert!(schedule.interaction(t0).is_empty());
        assert!(schedule.set_generation(t0, Some(1)).is_empty());
        assert!(!schedule.running(t0));
        // Turning the form on starts it, at once.
        assert_eq!(schedule.set_form(t0, true, INTERVAL), [request(true)]);
        // Off: hidden and stopped; the armed tick is stale.
        let (token, _) = armed(&schedule.answered(t0, 1, Outcome::Sample { cpu: true }));
        assert_eq!(schedule.set_form(t0, false, INTERVAL), [Action::Hide]);
        assert!(schedule.tick(t0 + INTERVAL, token).is_empty());
        // The generation ends: no Hide (the session clears the value), no tick.
        let mut schedule = started(t0);
        let (token, _) = armed(&schedule.answered(t0, 7, Outcome::Sample { cpu: true }));
        assert!(schedule.set_generation(t0, None).is_empty());
        assert!(schedule.tick(t0 + INTERVAL, token).is_empty());
        assert!(!schedule.running(t0));
    }

    #[test]
    fn a_stale_token_does_nothing() {
        let t0 = Instant::now();
        let mut schedule = started(t0);
        let (first, _) = armed(&schedule.answered(t0, 7, Outcome::Sample { cpu: true }));
        // Hidden and shown again: a new request, a new token later.
        assert!(schedule.set_visible(t0, false).is_empty());
        assert_eq!(schedule.set_visible(t0, true), [request(true)]);
        let (second, _) = armed(&schedule.answered(t0, 7, Outcome::Sample { cpu: true }));
        assert_ne!(first, second);
        assert!(schedule.tick(t0 + INTERVAL, first).is_empty());
        assert_eq!(schedule.tick(t0 + INTERVAL, second), [request(false)]);
        // A token fires once.
        assert!(schedule.tick(t0 + INTERVAL, second).is_empty());
    }

    #[test]
    fn idleness_pauses_at_the_tick_and_an_interaction_resumes_at_once() {
        let t0 = Instant::now();
        let mut schedule = started(t0);
        let (token, _) = armed(&schedule.answered(t0, 7, Outcome::Sample { cpu: true }));
        // An interaction while running only stamps the time.
        let t1 = t0 + Duration::from_secs(60);
        assert!(schedule.interaction(t1).is_empty());
        assert_eq!(schedule.tick(t1, token), [request(false)]);
        let (token, _) = armed(&schedule.answered(t1, 7, Outcome::Sample { cpu: true }));
        // STATS_IDLE after the last interaction the tick arms nothing more.
        let idle = t1 + STATS_IDLE;
        assert!(!schedule.running(idle));
        assert!(schedule.tick(idle, token).is_empty());
        // The next interaction: a sample at once, the history restarted.
        let t2 = idle + Duration::from_secs(30);
        assert_eq!(schedule.interaction(t2), [request(true)]);
    }

    #[test]
    fn an_interaction_before_the_tick_found_the_pause_keeps_the_history() {
        let t0 = Instant::now();
        let mut schedule = started(t0);
        let (token, _) = armed(&schedule.answered(t0, 7, Outcome::Sample { cpu: true }));
        // Past STATS_IDLE, but the armed tick has not fired yet.
        let late = t0 + STATS_IDLE + Duration::from_secs(1);
        assert!(!schedule.running(late));
        assert!(schedule.interaction(late).is_empty(), "no restart");
        assert_eq!(schedule.tick(late, token), [request(false)]);
        // The same with a request in flight: its reply arms the next tick.
        let later = late + STATS_IDLE + Duration::from_secs(1);
        assert!(schedule.interaction(later).is_empty());
        let (_, after) = armed(&schedule.answered(later, 7, Outcome::Sample { cpu: true }));
        assert_eq!(after, INTERVAL);
    }

    #[test]
    fn one_request_in_flight_at_most() {
        let t0 = Instant::now();
        let mut schedule = started(t0);
        // Covered and shown while the first request is out: no second one.
        assert!(schedule.set_visible(t0, false).is_empty());
        assert!(schedule.set_visible(t0, true).is_empty());
        // The popover opens meanwhile: still none.
        assert!(schedule.set_detail(t0, true).is_empty());
        // The reply sends the wanted one, with the details and a restart.
        assert_eq!(
            schedule.answered(t0, 7, Outcome::Sample { cpu: false }),
            [Action::Request {
                detail: true,
                restart: true
            }]
        );
        // Its reply arms the tick again.
        let (_, after) = armed(&schedule.answered(t0, 7, Outcome::Sample { cpu: true }));
        assert_eq!(after, INTERVAL);
    }

    #[test]
    fn opening_the_popover_asks_for_details_at_once() {
        let t0 = Instant::now();
        let mut schedule = started(t0);
        let (token, _) = armed(&schedule.answered(t0, 7, Outcome::Sample { cpu: true }));
        assert_eq!(
            schedule.set_detail(t0, true),
            [Action::Request {
                detail: true,
                restart: false
            }]
        );
        // The tick armed before is stale.
        assert!(schedule.tick(t0 + INTERVAL, token).is_empty());
        let (token, _) = armed(&schedule.answered(t0, 7, Outcome::Sample { cpu: true }));
        assert_eq!(
            schedule.tick(t0 + INTERVAL, token),
            [Action::Request {
                detail: true,
                restart: false
            }]
        );
        // Closing it: the next request is a plain one.
        assert!(schedule.set_detail(t0, false).is_empty());
        let (token, _) = armed(&schedule.answered(t0, 7, Outcome::Sample { cpu: true }));
        assert_eq!(schedule.tick(t0 + INTERVAL, token), [request(false)]);
    }

    #[test]
    fn a_failed_open_or_no_proc_ends_the_generation() {
        let t0 = Instant::now();
        for outcome in [Outcome::Unreachable, Outcome::NoProc] {
            let mut schedule = started(t0);
            assert_eq!(schedule.answered(t0, 7, outcome), [Action::Hide]);
            assert!(!schedule.running(t0));
            // Nothing brings it back in this generation…
            assert!(schedule.interaction(t0 + STATS_IDLE * 2).is_empty());
            assert!(schedule.set_visible(t0, false).is_empty());
            assert!(schedule.set_visible(t0, true).is_empty());
            assert!(schedule.set_detail(t0, true).is_empty());
            // …a new one starts clean.
            assert_eq!(
                schedule.set_generation(t0, Some(8)),
                [Action::Request {
                    detail: true,
                    restart: true
                }]
            );
        }
    }

    #[test]
    fn a_session_failure_hides_and_retries_once() {
        let t0 = Instant::now();
        let mut schedule = started(t0);
        let actions = schedule.answered(t0, 7, Outcome::Failed);
        assert_eq!(actions[0], Action::Hide);
        let (token, after) = armed(&actions[1..]);
        assert_eq!(after, INTERVAL);
        assert_eq!(schedule.tick(t0 + after, token), [request(false)]);
        // The retry failed too: the generation is done.
        assert_eq!(schedule.answered(t0, 7, Outcome::Failed), [Action::Hide]);
        assert!(!schedule.running(t0));
        // A success between two failures restores the retry.
        let mut schedule = started(t0);
        let actions = schedule.answered(t0, 7, Outcome::Failed);
        let (token, _) = armed(&actions[1..]);
        assert_eq!(schedule.tick(t0, token), [request(false)]);
        let (token, _) = armed(&schedule.answered(t0, 7, Outcome::Sample { cpu: true }));
        assert_eq!(schedule.tick(t0, token), [request(false)]);
        let actions = schedule.answered(t0, 7, Outcome::Failed);
        assert_eq!(actions.len(), 2, "{actions:?}");
    }

    #[test]
    fn an_old_generations_reply_only_frees_the_worker() {
        let t0 = Instant::now();
        let mut schedule = started(t0);
        // ssh to another host while the first request is out: the new
        // generation's first request waits for the worker.
        assert!(schedule.set_generation(t0, Some(8)).is_empty());
        // The old reply — even a failure — does not end the new generation;
        // it sends the wanted request.
        assert_eq!(
            schedule.answered(t0, 7, Outcome::Unreachable),
            [request(true)]
        );
        assert!(schedule.running(t0));
        let (_, after) = armed(&schedule.answered(t0, 8, Outcome::Sample { cpu: false }));
        assert_eq!(after, FIRST_FOLLOW);
        // A reply after the session ended arms nothing.
        assert!(schedule.set_generation(t0, None).is_empty());
        let mut schedule = started(t0);
        assert!(schedule.set_generation(t0, None).is_empty());
        assert!(
            schedule
                .answered(t0, 7, Outcome::Sample { cpu: true })
                .is_empty()
        );
    }

    #[test]
    fn a_new_interval_holds_from_the_next_tick() {
        let t0 = Instant::now();
        let mut schedule = started(t0);
        let (token, _) = armed(&schedule.answered(t0, 7, Outcome::Sample { cpu: true }));
        assert!(
            schedule
                .set_form(t0, true, Duration::from_secs(10))
                .is_empty()
        );
        assert_eq!(schedule.tick(t0 + INTERVAL, token), [request(false)]);
        let (_, after) = armed(&schedule.answered(t0, 7, Outcome::Sample { cpu: true }));
        assert_eq!(after, Duration::from_secs(10));
    }
}
