//! The remote load indicator's **detail popover**: a
//! click on the indicator opens it, a second click, a click outside or Esc
//! closes it.
//!
//! - **The upload list's pattern** (`uploader`'s "Show transfers (N)"):
//!   `transient`, anchored to the indicator's column range
//!   (`Session::footer_span` → `BateriView::context_span_rect`, the drawing's
//!   own layout), Esc swallowed by a local key monitor in the pane's window —
//!   the terminal window stays key, so Esc would otherwise reach the remote
//!   shell. The pane is the delegate of both popovers and tells them apart by
//!   the notification's object; each has its own close-time slot, so a second
//!   press on the indicator does not reopen it.
//! - **Refreshed in place**: the rows are fixed (title, CPU, load, memory,
//!   swap, disk, uptime, three processes — "Measuring…" until the second
//!   process scan), so the views are built once and
//!   every sample only rewrites their text, bar width and bar colour.
//! - **While open the samples carry the details** (OS, cores, processes):
//!   `Schedule::set_detail`, and opening asks at once rather than at the next
//!   tick.
//! - **It goes with the indicator**: when the indicator is no longer drawn
//!   (an upload row took its place, the remote session ended, `off`, a hide)
//!   the popover closes ([`TerminalPane::stats_gauge_changed`]).
//!
//! The bars' colour is the context row's: `bt_core`'s threshold class
//! ([`StatsMetric::level`]) → the theme's `info`/`warning`/`error`; swap has
//! no threshold and stays `info`.

use bt_core::{FooterControl, StatsLevel, StatsMetric, Theme};
use bt_shell_common::remote_files::Process;
use bt_shell_common::remote_stats::{Detail, TOP_PROCESSES};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBox, NSBoxType, NSColor, NSFont, NSLineBreakMode, NSPopover,
    NSPopoverBehavior, NSTextAlignment, NSTextField, NSTitlePosition, NSView, NSViewController,
};
use objc2_foundation::{NSNotification, NSPoint, NSRect, NSRectEdge, NSSize, NSString};

use crate::pane::TerminalPane;
use crate::uploader::{ESCAPE, add_key_monitor, label, remove_monitor};

/// The popover's width, inner padding, the gap between rows, a line's height
/// and the bar's height — **design constants** (pt), the upload list's measures.
const WIDTH: f64 = 300.0;
const PAD: f64 = 12.0;
const ROW_GAP: f64 = 8.0;
const LINE: f64 = 16.0;
const BAR: f64 = 4.0;
/// The value column's width (right-aligned).
const VALUE_WIDTH: f64 = 150.0;
/// How many processes the popover lists.
const PROCESSES: usize = TOP_PROCESSES;

/// What an unknown value shows: an empty row would read as "nothing", not "not yet".
const UNKNOWN: &str = "—";
/// The process list before its second scan: CPU is a difference.
const MEASURING: &str = "Measuring…";
/// The swap row of a server without swap (`SwapTotal` 0).
const NO_SWAP: &str = "none";

/// A metric row's bar: the full-width track and the fill whose width and
/// colour change.
struct Meter {
    value: Retained<NSTextField>,
    track: Retained<NSBox>,
    fill: Retained<NSBox>,
}

/// The open popover's refreshed views.
struct Rows {
    title: Retained<NSTextField>,
    cpu_label: Retained<NSTextField>,
    cpu: Meter,
    load: Retained<NSTextField>,
    mem: Meter,
    swap: Meter,
    disk: Meter,
    uptime: Retained<NSTextField>,
    processes: [(Retained<NSTextField>, Retained<NSTextField>); PROCESSES],
}

/// The open detail popover and what it holds for refreshing.
pub(crate) struct StatsPopover {
    popover: Retained<NSPopover>,
    rows: Rows,
    /// The Esc monitor (removed on close).
    monitor: Option<Retained<AnyObject>>,
}

/// The detail popover's half of the pane.
impl TerminalPane {
    /// Opens the popover, or closes it if it is open. A `transient` popover
    /// closes itself on a press outside and that same press reaches here too:
    /// the close time is kept (`popoverWillClose:`) and nothing is done for
    /// that event (the upload list's rule).
    pub(crate) fn toggle_stats_popover(&self, (start, end): (u16, u16)) {
        let shown = self
            .stats_popover()
            .borrow()
            .as_ref()
            .map(|open| open.popover.clone());
        if let Some(popover) = shown {
            let was_shown = popover.isShown();
            self.close_stats_popover();
            if was_shown {
                return;
            }
        }
        if self.stats_closed_by_current_event() {
            return;
        }
        let Some(rect) = self.view().context_span_rect(start, end) else {
            return;
        };
        let Some(window) = self.window() else {
            return;
        };
        let mtm = self.mtm();
        let popover = NSPopover::new(mtm);
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setDelegate(Some(ProtocolObject::from_ref(self)));
        let controller = NSViewController::new(mtm);
        let content = NSView::new(mtm);
        let (size, rows) = build(mtm, &content);
        controller.setView(&content);
        popover.setContentViewController(Some(&controller));
        popover.setContentSize(size);
        let (id, lookup) = (self.id(), self.lookup());
        let number = window.windowNumber();
        let monitor = add_key_monitor(move |event| {
            if event.keyCode() != ESCAPE || event.windowNumber() != number {
                return false;
            }
            // audit: the local event monitor runs on the main thread.
            let mtm = MainThreadMarker::new().expect("the event monitor is on the main thread");
            let Some(pane) = lookup(mtm, id) else {
                return false;
            };
            if pane.stats_popover().borrow().is_some() {
                pane.close_stats_popover();
                return true;
            }
            // Even if the popover closed Esc itself, the key must not go to the shell.
            pane.stats_closed_by_current_event()
        });
        self.stats_popover().replace(Some(StatsPopover {
            popover: popover.clone(),
            rows,
            monitor,
        }));
        self.refresh_stats_popover();
        let view: &NSView = self.view();
        popover.showRelativeToRect_ofView_preferredEdge(rect, view, NSRectEdge::MinY);
        self.set_stats_detail(true);
    }

    /// Whether the event that closed the popover is the current event.
    fn stats_closed_by_current_event(&self) -> bool {
        let now = current_event_time(self.mtm());
        now.is_some() && self.stats_closed_at().get() == now
    }

    /// Whether `notification` is about the detail popover (the pane is the
    /// delegate of the upload list too).
    pub(crate) fn is_stats_popover(&self, notification: &NSNotification) -> bool {
        let Some(object) = notification.object() else {
            return false;
        };
        self.stats_popover().borrow().as_ref().is_some_and(|open| {
            std::ptr::eq(
                Retained::as_ptr(&object).cast::<()>(),
                Retained::as_ptr(&open.popover).cast::<()>(),
            )
        })
    }

    /// `popoverWillClose:` of the detail popover: the time of the event that closed it.
    pub(crate) fn stats_popover_will_close(&self) {
        self.stats_closed_at().set(current_event_time(self.mtm()));
    }

    /// Closes the popover and stops asking for the details; no-op if closed.
    /// `popoverDidClose:` lands here too (AppKit closed it itself).
    pub(crate) fn close_stats_popover(&self) {
        let Some(open) = self.stats_popover().borrow_mut().take() else {
            return;
        };
        remove_monitor(open.monitor);
        if open.popover.isShown() {
            open.popover.close();
        }
        self.set_stats_detail(false);
    }

    /// Rewrites the open popover from the last sample, in place.
    pub(crate) fn refresh_stats_popover(&self) {
        let open = self.stats_popover().borrow();
        let Some(open) = open.as_ref() else {
            return;
        };
        let Some(session) = self.session() else {
            return;
        };
        let theme = session.theme();
        let host = session.remote_target().map(|(_, target, _)| target.host);
        let driver = self.stats_driver().borrow();
        fill(&open.rows, host.as_deref(), driver.detail(), &theme);
    }

    /// The indicator may have appeared, gone or moved (a sample, a hide, the
    /// remote edge, the settings, an upload row): the popover closes if the
    /// indicator is no longer drawn, otherwise follows its range; the hand
    /// cursor's rectangle is refreshed.
    pub(crate) fn stats_gauge_changed(&self) {
        let shown = self
            .stats_popover()
            .borrow()
            .as_ref()
            .map(|open| open.popover.clone());
        if let Some(popover) = shown {
            let span = self
                .view()
                .context_budget()
                .zip(self.session())
                .and_then(|(budget, session)| session.footer_span(budget, FooterControl::Stats));
            match span.and_then(|(start, end)| self.view().context_span_rect(start, end)) {
                Some(rect) => popover.setPositioningRect(rect),
                None => self.close_stats_popover(),
            }
        }
        self.view().sync_cursor_rects();
    }
}

/// The current event's time stamp (the close-time slots' unit).
fn current_event_time(mtm: MainThreadMarker) -> Option<f64> {
    NSApplication::sharedApplication(mtm)
        .currentEvent()
        .map(|event| event.timestamp())
}

/// Lays the fixed rows into `content`, top-down (the view is not flipped; y is
/// computed from the bottom at the end, the upload list's way). Returns the
/// size and the views to refresh.
fn build(mtm: MainThreadMarker, content: &NSView) -> (NSSize, Rows) {
    let mut placed: Vec<(Retained<NSView>, NSRect)> = Vec::new();
    let mut top = PAD;
    let inner = WIDTH - 2.0 * PAD;
    let mut place = |view: Retained<NSView>, x: f64, top: f64, w: f64, h: f64| {
        placed.push((view, NSRect::new(NSPoint::new(x, top), NSSize::new(w, h))));
    };
    let secondary = NSColor::secondaryLabelColor();
    let primary = NSColor::labelColor();
    let value_label = |mtm: MainThreadMarker| {
        let value = label(mtm, "", 12.0, &primary);
        value.setAlignment(NSTextAlignment::Right);
        value
    };

    let title = label(mtm, "", 12.0, &secondary);
    title.setFont(Some(&NSFont::boldSystemFontOfSize(12.0)));
    title.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
    place(view_of(&title), PAD, top, inner, LINE);
    top += LINE + ROW_GAP;

    // A label + value line; `meter` adds a bar below it.
    let mut line = |name: &str, meter: bool, top: &mut f64| {
        let name = label(mtm, name, 12.0, &secondary);
        place(view_of(&name), PAD, *top, inner - VALUE_WIDTH, LINE);
        let value = value_label(mtm);
        place(
            view_of(&value),
            WIDTH - PAD - VALUE_WIDTH,
            *top,
            VALUE_WIDTH,
            LINE,
        );
        *top += LINE;
        let fill = meter.then(|| {
            *top += 2.0;
            let track = bar(mtm, &NSColor::quaternaryLabelColor());
            place(Retained::into_super(track.clone()), PAD, *top, inner, BAR);
            let fill = bar(mtm, &NSColor::clearColor());
            place(Retained::into_super(fill.clone()), PAD, *top, 0.0, BAR);
            *top += BAR;
            (track, fill)
        });
        *top += ROW_GAP;
        (name, value, fill)
    };
    let meter = |(_, value, bar): (Retained<NSTextField>, _, Option<_>)| {
        let (track, fill) = bar.expect("a meter line has a bar");
        Meter { value, track, fill }
    };
    let (cpu_label, cpu_value, cpu_bar) = line("CPU", true, &mut top);
    let cpu = meter((cpu_label.clone(), cpu_value, cpu_bar));
    let (_, load, _) = line("Load 1/5/15", false, &mut top);
    let mem = meter(line("Memory", true, &mut top));
    let swap = meter(line("Swap", true, &mut top));
    let disk = meter(line("Disk /", true, &mut top));
    let (_, uptime, _) = line("Uptime", false, &mut top);

    let rule = NSBox::new(mtm);
    rule.setBoxType(NSBoxType::Separator);
    place(Retained::into_super(rule), PAD, top, inner, 1.0);
    top += 1.0 + ROW_GAP;
    let header = label(mtm, "Top processes", 11.0, &secondary);
    place(view_of(&header), PAD, top, inner, LINE);
    top += LINE;
    let processes = std::array::from_fn(|_| {
        let name = label(mtm, "", 12.0, &primary);
        name.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        place(view_of(&name), PAD, top, inner - VALUE_WIDTH, LINE);
        let value = value_label(mtm);
        place(
            view_of(&value),
            WIDTH - PAD - VALUE_WIDTH,
            top,
            VALUE_WIDTH,
            LINE,
        );
        top += LINE;
        (name, value)
    });
    top += PAD - 4.0;

    let height = top;
    for (view, frame) in placed {
        view.setFrame(NSRect::new(
            NSPoint::new(frame.origin.x, height - frame.origin.y - frame.size.height),
            frame.size,
        ));
        content.addSubview(&view);
    }
    let size = NSSize::new(WIDTH, height);
    content.setFrameSize(size);
    let rows = Rows {
        title,
        cpu_label,
        cpu,
        load,
        mem,
        swap,
        disk,
        uptime,
        processes,
    };
    (size, rows)
}

/// A text field as a plain view.
fn view_of(field: &Retained<NSTextField>) -> Retained<NSView> {
    Retained::into_super(Retained::into_super(field.clone()))
}

/// A flat, rounded bar (track or fill): an `NSBox`, because a progress
/// indicator cannot take the theme's colour.
fn bar(mtm: MainThreadMarker, color: &NSColor) -> Retained<NSBox> {
    let bar = NSBox::new(mtm);
    bar.setBoxType(NSBoxType::Custom);
    bar.setTitlePosition(NSTitlePosition::NoTitle);
    bar.setBorderWidth(0.0);
    bar.setCornerRadius(BAR / 2.0);
    bar.setFillColor(color);
    bar
}

/// Writes one sample into the rows; `None` values show [`UNKNOWN`].
fn fill(rows: &Rows, host: Option<&str>, detail: Option<&Detail>, theme: &Theme) {
    let empty = Detail::default();
    let sampled = detail.is_some();
    let detail = detail.unwrap_or(&empty);
    set_text(
        &rows.title,
        &format!(
            "{} · {}",
            host.unwrap_or(UNKNOWN),
            detail.os.as_deref().unwrap_or(UNKNOWN)
        ),
    );
    set_text(&rows.cpu_label, &cpu_label(detail.cores));
    let cpu = detail.cpu;
    set_meter(
        &rows.cpu,
        &cpu.map_or_else(|| UNKNOWN.to_owned(), |cpu| format!("{cpu}%")),
        cpu.map(|cpu| (f64::from(cpu) / 100.0, StatsMetric::Cpu.level(cpu))),
        theme,
    );
    set_text(
        &rows.load,
        &detail.load.map_or_else(
            || UNKNOWN.to_owned(),
            |[one, five, fifteen]| {
                format!(
                    "{}  {}  {}",
                    hundredths(one),
                    hundredths(five),
                    hundredths(fifteen)
                )
            },
        ),
    );
    let mem = percent(detail.mem_used, detail.mem_total);
    set_meter(
        &rows.mem,
        &used_of(detail.mem_used, detail.mem_total),
        mem.map(|pct| {
            (
                fraction(detail.mem_used, detail.mem_total),
                StatsMetric::Mem.level(pct),
            )
        }),
        theme,
    );
    // Swap has no threshold: its bar stays `info`. A server
    // without swap says so and draws no bar — an empty track read as "0 of
    // something"; before the first sample the row is unknown, not "none".
    let (swap, no_swap) = swap_value(sampled, detail.swap_used, detail.swap_total);
    set_meter(
        &rows.swap,
        &swap,
        percent(detail.swap_used, detail.swap_total).map(|_| {
            (
                fraction(detail.swap_used, detail.swap_total),
                StatsLevel::Normal,
            )
        }),
        theme,
    );
    rows.swap.set_bar_hidden(no_swap);
    set_meter(
        &rows.disk,
        &detail
            .disk
            .map_or_else(|| UNKNOWN.to_owned(), |disk| format!("{disk}%")),
        detail
            .disk
            .map(|disk| (f64::from(disk) / 100.0, StatsMetric::Disk.level(disk))),
        theme,
    );
    set_text(
        &rows.uptime,
        &detail.uptime.map_or_else(|| UNKNOWN.to_owned(), uptime),
    );
    for (index, (name, value)) in rows.processes.iter().enumerate() {
        let (left, right) =
            process_row(sampled, detail.processes.as_deref(), index).unwrap_or_default();
        set_text(name, &left);
        set_text(value, &right);
    }
}

impl Meter {
    /// Hides or shows the bar (track and fill) under the value.
    fn set_bar_hidden(&self, hidden: bool) {
        if self.track.isHidden() != hidden {
            self.track.setHidden(hidden);
            self.fill.setHidden(hidden);
        }
    }
}

/// The process list's `index`th line: a process and its CPU; before the
/// second scan one "Measuring…" (no sample at all yet: one "—"); measured but
/// nothing used the CPU: one "—". `None` for an empty line.
fn process_row(
    sampled: bool,
    processes: Option<&[Process]>,
    index: usize,
) -> Option<(String, String)> {
    match processes {
        Some(list) => match list.get(index) {
            Some(process) => Some((process.name.clone(), tenths(process.cpu))),
            None => (index == 0 && list.is_empty()).then(|| (UNKNOWN.to_owned(), String::new())),
        },
        None => (index == 0).then(|| {
            let text = if sampled { MEASURING } else { UNKNOWN };
            (text.to_owned(), String::new())
        }),
    }
}

/// Sets a label's text only if it changed (no relayout for the same text).
fn set_text(field: &NSTextField, text: &str) {
    if field.stringValue().to_string() != text {
        field.setStringValue(&NSString::from_str(text));
    }
}

/// A meter's value and bar: `bar` is the fill fraction and the threshold class
/// (`None` → an empty bar).
fn set_meter(meter: &Meter, value: &str, bar: Option<(f64, StatsLevel)>, theme: &Theme) {
    set_text(&meter.value, value);
    let (fraction, level) = bar.unwrap_or((0.0, StatsLevel::Normal));
    let width = (WIDTH - 2.0 * PAD) * fraction.clamp(0.0, 1.0);
    meter.fill.setFrameSize(NSSize::new(width, BAR));
    meter.fill.setFillColor(&srgb(level_color(level, theme)));
}

/// The theme role of a threshold class — the context row's colours.
fn level_color(level: StatsLevel, theme: &Theme) -> u32 {
    match level {
        StatsLevel::Normal => theme.info,
        StatsLevel::Warning => theme.warning,
        StatsLevel::Critical => theme.error,
    }
}

/// A `0xRRGGBB` theme colour as an sRGB `NSColor`.
fn srgb(color: u32) -> Retained<NSColor> {
    let byte = |shift: u32| f64::from((color >> shift) & 0xff) / 255.0;
    NSColor::colorWithSRGBRed_green_blue_alpha(byte(16), byte(8), byte(0), 1.0)
}

/// `CPU · N cores`; the cores come only with a `detail` sample.
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
    let (unit, name) = unit_of(total);
    format!("{} / {} {name}", scaled(used, unit), scaled(total, unit))
}

/// The swap row's value and whether its bar is hidden: `none` without a bar
/// on a sampled server without swap, [`used_of`] otherwise (unknown before the
/// first sample).
fn swap_value(sampled: bool, used: u64, total: u64) -> (String, bool) {
    if sampled && total == 0 {
        (NO_SWAP.to_owned(), true)
    } else {
        (used_of(used, total), false)
    }
}

/// The unit a size is shown in: GB from one GiB up, MB below.
fn unit_of(bytes: u64) -> (u64, &'static str) {
    const MIB: u64 = 1 << 20;
    const GIB: u64 = 1 << 30;
    if bytes >= GIB {
        (GIB, "GB")
    } else {
        (MIB, "MB")
    }
}

/// `bytes` in `unit` with one decimal for GB, none for MB.
fn scaled(bytes: u64, unit: u64) -> String {
    // audit: the sizes are a server's memory; `f64`'s 53 bits are exact up to 8 PiB.
    let value = bytes as f64 / unit as f64;
    if unit >= 1 << 30 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    }
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
        used as f64 / total as f64
    }
}

/// A load average in hundredths → `0.42`.
fn hundredths(value: u32) -> String {
    format!("{}.{:02}", value / 100, value % 100)
}

/// A process's CPU in tenths → `12.3%`.
fn tenths(value: u32) -> String {
    format!("{}.{}%", value / 10, value % 10)
}

/// Seconds since boot → `3d 4h`, `4h 12m`, `12m`.
fn uptime(seconds: u64) -> String {
    let (days, hours, minutes) = (
        seconds / 86_400,
        seconds % 86_400 / 3_600,
        seconds % 3_600 / 60,
    );
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_shown_in_the_totals_unit() {
        const GIB: u64 = 1 << 30;
        assert_eq!(used_of(3 * GIB + GIB / 10, 8 * GIB), "3.1 / 8.0 GB");
        assert_eq!(used_of(0, 2 * GIB), "0.0 / 2.0 GB");
        assert_eq!(used_of(100 << 20, 512 << 20), "100 / 512 MB");
        assert_eq!(used_of(0, 0), UNKNOWN, "no swap");
    }

    #[test]
    fn a_server_without_swap_says_none_and_draws_no_bar() {
        assert_eq!(swap_value(true, 0, 0), (NO_SWAP.to_owned(), true));
        assert_eq!(
            swap_value(false, 0, 0),
            (UNKNOWN.to_owned(), false),
            "before the first sample it is unknown, not none"
        );
        assert_eq!(
            swap_value(true, 0, 2 << 30),
            ("0.0 / 2.0 GB".to_owned(), false)
        );
    }

    #[test]
    fn the_process_list_says_when_it_is_still_measuring() {
        let row = |sampled, list: Option<&[Process]>, index| process_row(sampled, list, index);
        assert_eq!(
            row(false, None, 0),
            Some((UNKNOWN.to_owned(), String::new()))
        );
        assert_eq!(
            row(true, None, 0),
            Some((MEASURING.to_owned(), String::new()))
        );
        assert_eq!(row(true, None, 1), None);
        assert_eq!(
            row(true, Some(&[]), 0),
            Some((UNKNOWN.to_owned(), String::new()))
        );
        let list = [Process {
            name: "postgres".to_owned(),
            cpu: 1234,
        }];
        assert_eq!(
            row(true, Some(&list), 0),
            Some(("postgres".to_owned(), "123.4%".to_owned()))
        );
        assert_eq!(row(true, Some(&list), 1), None);
    }

    #[test]
    fn percentages_round_and_never_exceed_a_hundred() {
        assert_eq!(percent(1, 3), Some(33));
        assert_eq!(percent(2, 3), Some(67));
        assert_eq!(percent(5, 4), Some(100));
        assert_eq!(percent(1, 0), None);
        assert_eq!(fraction(1, 0), 0.0);
    }

    #[test]
    fn small_numbers_keep_their_fixed_point() {
        assert_eq!(hundredths(42), "0.42");
        assert_eq!(hundredths(1205), "12.05");
        assert_eq!(tenths(123), "12.3%");
        assert_eq!(tenths(5), "0.5%");
    }

    #[test]
    fn uptime_names_its_two_largest_units() {
        assert_eq!(uptime(59), "0m");
        assert_eq!(uptime(12 * 60 + 5), "12m");
        assert_eq!(uptime(4 * 3_600 + 12 * 60), "4h 12m");
        assert_eq!(uptime(3 * 86_400 + 4 * 3_600 + 59), "3d 4h");
    }

    #[test]
    fn the_cpu_label_names_the_cores_when_known() {
        assert_eq!(cpu_label(None), "CPU");
        assert_eq!(cpu_label(Some(1)), "CPU · 1 core");
        assert_eq!(cpu_label(Some(8)), "CPU · 8 cores");
    }

    #[test]
    fn a_bar_takes_the_threshold_class_colour() {
        let theme = Theme::BATERI;
        assert_eq!(level_color(StatsMetric::Cpu.level(10), &theme), theme.info);
        assert_eq!(
            level_color(StatsMetric::Mem.level(85), &theme),
            theme.warning
        );
        assert_eq!(
            level_color(StatsMetric::Disk.level(99), &theme),
            theme.error
        );
    }
}
