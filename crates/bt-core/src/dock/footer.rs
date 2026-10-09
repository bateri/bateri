//! The dock's **context row** — the footer under the input block: its two
//! zones and the one plan the drawing and the mouse both read.
//!
//! **Left, the lead: where this row is.** The local path and branch, `⇄ host`
//! and the remote path, a program guide bar's title, an upload's host and
//! text. Each form builds only its own lead ([`Lead`]); none of them lays out
//! the right side.
//!
//! **Right, the trailing zone: what reports on that place or acts for it**,
//! ordered from the edge inward by how often it changes. Controls and hints
//! stand at the edge (they do not change while their form shows), the load
//! gauge next to them (its numbers are tabular, its width the same every
//! sample), the listening
//! ports innermost (they change only when a server starts or stops). A change
//! in an item then moves only what is to its left — the lead's budget — and
//! never a control out from under the mouse: the upload buttons were
//! right-aligned for this very reason before the zone had a name. The ports
//! are appended to **every** form here, so a form does not know they exist.
//!
//! **One reduction order for every form** ([`plan`]). With the whole lead
//! showing: the hint drops, the gauge steps down (sparkline → numbers), the
//! ports shorten (`↗ :3000  :6006` → `↗ :3000 +1`), the gauge steps down
//! again (→ the worst value). Then the lead gives way (the path from the left,
//! an upload's text from the right) to what is kept: the shortened ports, the
//! worst value only past its threshold, the controls whole; and when even the
//! lead's core cannot stand beside them, the ports go to a count and drop, the
//! gauge drops, the controls lose their `⌘.` hint, their list button and
//! their last button. Never shortened — a cut one reads as another: a host, a
//! branch, a title, a port number, a button's label.
//!
//! **One gap**: [`GAP`] columns between the lead's last drawn character and
//! the zone, and between two items of the zone.
//!
//! Drawing ([`render_context`]) and the mouse ([`footer_spans`],
//! [`footer_hit`], [`footer_span`]) read the same plan; had the two
//! arithmetics diverged a click would land next to what is drawn.

use super::*;
use crate::shell::FooterPort;

/// The gap between the lead and the trailing zone, and between two of its
/// items, in columns — the remote form's own gap (`⇄ prod  /srv`).
const GAP: usize = 2;

/// A clickable part of the context row — what [`footer_hit`] finds and
/// [`footer_span`] anchors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FooterControl {
    /// The listening ports.
    Ports,
    /// The remote load indicator.
    Stats,
    /// The ssh status bar's Sign In… button.
    SignIn,
    /// The upload row's list button ("Show transfers (N)").
    List,
    /// The upload row's cancel button.
    Cancel,
}

/// What lies under a column of the context row ([`footer_hit`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FooterHit {
    pub control: FooterControl,
    /// On [`FooterControl::Ports`], the port whose `:N` is under the column;
    /// `None` on the mark, a `+N` or a count.
    pub port: Option<u16>,
}

/// Where this row is: the left zone of each form.
#[derive(Clone, Copy)]
enum Lead<'a> {
    /// `{path} | {branch}`.
    Local { cwd: &'a str, branch: &'a str },
    /// `⇄ {host}  {remote path}`.
    Remote { host: &'a str, cwd: &'a str },
    /// `{title} · {detail}  {path}`.
    Program(&'a ProgramBar),
    /// `⇄ {host}  {text}`.
    Transfer(&'a Transfer),
}

impl Lead<'_> {
    /// The width with everything showing.
    fn full(self) -> usize {
        match self {
            Self::Local { cwd, branch } => {
                let (path, branch) = (cwd.chars().count(), branch.chars().count());
                let separator = if path > 0 && branch > 0 {
                    SEPARATOR.chars().count()
                } else {
                    0
                };
                path + separator + branch
            }
            Self::Remote { host, cwd } => remote_head(host) + tail(cwd),
            Self::Program(bar) => program_head(bar) + tail(&bar.path),
            Self::Transfer(transfer) => remote_head(&transfer.host) + tail(&transfer.body),
        }
    }

    /// The **core**: what must show for anything to stand beside the lead —
    /// the parts that are never shortened. The rest (a path, an upload's
    /// text) gives way to what the trailing zone keeps.
    fn core(self) -> usize {
        match self {
            Self::Local { branch, .. } => branch.chars().count(),
            Self::Remote { host, .. } => remote_head(host),
            Self::Program(bar) => program_head(bar),
            Self::Transfer(transfer) => remote_head(&transfer.host),
        }
    }
}

/// `⇄ host`'s width.
fn remote_head(host: &str) -> usize {
    2 + host.chars().count()
}

/// A guide bar's `{title} · {detail}` width.
fn program_head(bar: &ProgramBar) -> usize {
    let detail = bar.detail.chars().count();
    bar.title.chars().count()
        + if detail == 0 {
            0
        } else {
            PROGRAM_DETAIL.chars().count() + detail
        }
}

/// The width of a lead's shortenable tail after its head: the gap and the
/// text, nothing when there is no text.
fn tail(text: &str) -> usize {
    match text.chars().count() {
        0 => 0,
        chars => REMOTE_GAP.chars().count() + chars,
    }
}

/// An item of the trailing zone.
#[derive(Clone, Copy)]
enum Item<'a> {
    /// The listening ports, ascending ([`DockContext::ports`]).
    Ports(&'a [FooterPort]),
    /// The remote load indicator.
    Gauge(&'a RemoteStats),
    /// The ssh status bar's Sign In… button.
    SignIn,
    /// The upload row's buttons.
    Buttons(&'a Transfer),
    /// A guide bar's hint (`⌃D exit`).
    Hint(&'a str),
}

/// How much of the ports shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PortsRung {
    /// `↗ :3000  :6006`.
    All,
    /// `↗ :3000 +1` — the first and how many more.
    Short,
    /// `↗ 2` — how many.
    Count,
}

/// An item's form at a rung of the reduction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rung {
    Ports(PortsRung),
    Gauge(GaugeStep),
    SignIn,
    /// The upload buttons: with the list button, and with the `⌘.` hint.
    Buttons {
        list: bool,
        hint: bool,
    },
    Hint,
}

impl Item<'_> {
    /// The item's first rung — everything showing.
    fn widest(self) -> Rung {
        match self {
            Self::Ports(_) => Rung::Ports(PortsRung::All),
            Self::Gauge(stats) => Rung::Gauge(ladder(stats.form)[0]),
            Self::SignIn => Rung::SignIn,
            Self::Buttons(transfer) => Rung::Buttons {
                list: transfer.controls.items > 1,
                hint: true,
            },
            Self::Hint(_) => Rung::Hint,
        }
    }

    /// The rung kept while the lead gives way; `None` → the item drops
    /// before the lead shortens.
    fn kept(self) -> Option<Rung> {
        match self {
            Self::Ports(_) => Some(Rung::Ports(PortsRung::Short)),
            // "disk 96%" matters more than the path; a calm value does not.
            Self::Gauge(stats) => {
                let (metric, value) = worst(stats);
                (metric.level(value) > StatsLevel::Normal).then_some(Rung::Gauge(GaugeStep::Worst))
            }
            Self::SignIn | Self::Buttons(_) => Some(self.widest()),
            Self::Hint(_) => None,
        }
    }

    /// The width of `rung`, in columns.
    fn width(self, rung: Rung) -> usize {
        match (self, rung) {
            (Self::Ports(ports), Rung::Ports(rung)) => ports_width(ports, rung),
            (Self::Gauge(stats), Rung::Gauge(step)) => gauge(stats, step).width(),
            (Self::SignIn, _) => button_width(ButtonLabel::SignIn, false),
            (Self::Buttons(transfer), Rung::Buttons { list, hint }) => {
                let list = if list {
                    button_width(list_label(transfer), false) + BUTTON_GAP
                } else {
                    0
                };
                list + button_width(cancel_label(transfer), hint)
            }
            (Self::Hint(hint), _) => hint.chars().count(),
            // An item is only ever asked about its own rungs.
            _ => 0,
        }
    }
}

/// The cancel button's label: "Cancel all" over a queue of several.
fn cancel_label(transfer: &Transfer) -> ButtonLabel {
    if transfer.controls.items > 1 {
        ButtonLabel::CancelAll
    } else {
        ButtonLabel::Cancel
    }
}

/// The list button's label, with the queue's length.
fn list_label(transfer: &Transfer) -> ButtonLabel {
    ButtonLabel::ShowFiles(transfer.controls.items)
}

/// The number of trailing items a form has: the ports and the form's own.
const ITEMS: usize = 2;

/// The plan of the context row: the lead, its budget and the placed items.
#[derive(Clone, Copy)]
struct Plan<'a> {
    lead: Lead<'a>,
    /// The columns the lead may draw in, from the row's start.
    lead_budget: usize,
    /// The trailing items, left to right; `None` for an absent or dropped one.
    items: [Option<Placed<'a>>; ITEMS],
}

/// A placed item: its rung and its context-local range `[start, end)`.
#[derive(Clone, Copy)]
struct Placed<'a> {
    item: Item<'a>,
    rung: Rung,
    start: usize,
    end: usize,
}

/// The row's parts from `context`: the lead and the trailing items, left to
/// right (the ports innermost, the form's own item at the edge).
fn parts(context: &DockContext) -> (Lead<'_>, [Option<Item<'_>>; ITEMS]) {
    let ports = (!context.ports.is_empty()).then_some(Item::Ports(&context.ports));
    // The upload row **before** the remote form: it carries its own host and
    // must show its result after ssh has closed too.
    if let Some(transfer) = &context.transfer {
        let buttons = (transfer.controls.items > 0).then_some(Item::Buttons(transfer));
        return (Lead::Transfer(transfer), [ports, buttons]);
    }
    if let Some(host) = context.remote_host() {
        // Sign In… takes the indicator's place: there is no sample without a login.
        let own = if context.sign_in.is_some() {
            Some(Item::SignIn)
        } else {
            context.stats.as_ref().map(Item::Gauge)
        };
        let lead = Lead::Remote {
            host,
            cwd: &context.remote_cwd,
        };
        return (lead, [ports, own]);
    }
    // A recognized program's guide bar after both: the upload's result and
    // the remote session say more about where the keys go.
    if let Some(bar) = &context.program {
        let hint = (!bar.hint.is_empty()).then_some(Item::Hint(&bar.hint));
        return (Lead::Program(bar), [ports, hint]);
    }
    let lead = Lead::Local {
        cwd: &context.cwd,
        branch: &context.branch,
    };
    (lead, [ports, None])
}

/// The plan of `context` on an `available`-column row — the reduction order
/// of the module's doc.
fn plan(context: &DockContext, available: usize) -> Plan<'_> {
    let (lead, items) = parts(context);
    // With the whole lead: each step is tried in order, the first that fits wins.
    let mut rungs = items.map(|item| item.map(Item::widest));
    let full = lead.full();
    for step in 0..=WHOLE_LEAD_STEPS.len() {
        if step > 0 {
            WHOLE_LEAD_STEPS[step - 1](&items, &mut rungs);
        }
        if let Some(plan) = place(lead, full, &items, &rungs, available) {
            return plan;
        }
    }
    // The lead gives way to what is kept; the kept items reduce in turn.
    let mut rungs = items.map(|item| item.and_then(Item::kept));
    loop {
        if let Some(plan) = place(lead, lead.core(), &items, &rungs, available) {
            return plan;
        }
        if !reduce_kept(&items, &mut rungs) {
            break;
        }
    }
    Plan {
        lead,
        lead_budget: available,
        items: [None; ITEMS],
    }
}

/// The reduction's steps while the whole lead shows, in order.
type Step = fn(&[Option<Item<'_>>; ITEMS], &mut [Option<Rung>; ITEMS]);
const WHOLE_LEAD_STEPS: [Step; 4] = [drop_hint, lower_gauge, shorten_ports, lower_gauge];

fn drop_hint(items: &[Option<Item<'_>>; ITEMS], rungs: &mut [Option<Rung>; ITEMS]) {
    for (item, rung) in items.iter().zip(rungs.iter_mut()) {
        if matches!(item, Some(Item::Hint(_))) {
            *rung = None;
        }
    }
}

/// The gauge's next rung of its form's ladder; at the last one it stays.
fn lower_gauge(items: &[Option<Item<'_>>; ITEMS], rungs: &mut [Option<Rung>; ITEMS]) {
    for (item, rung) in items.iter().zip(rungs.iter_mut()) {
        if let (Some(Item::Gauge(stats)), Some(Rung::Gauge(step))) = (item, &rung) {
            let ladder = ladder(stats.form);
            if let Some(at) = ladder.iter().position(|candidate| candidate == step) {
                if let Some(&next) = ladder.get(at + 1) {
                    *rung = Some(Rung::Gauge(next));
                }
            }
        }
    }
}

fn shorten_ports(items: &[Option<Item<'_>>; ITEMS], rungs: &mut [Option<Rung>; ITEMS]) {
    for (item, rung) in items.iter().zip(rungs.iter_mut()) {
        if matches!(item, Some(Item::Ports(_))) && rung.is_some() {
            *rung = Some(Rung::Ports(PortsRung::Short));
        }
    }
}

/// One step of the kept items' reduction, in their order of least need: the
/// ports (to a count, then off), the gauge, the controls (the `⌘.` hint, the
/// list button, the last button). `false` when nothing is left to reduce.
fn reduce_kept(items: &[Option<Item<'_>>; ITEMS], rungs: &mut [Option<Rung>; ITEMS]) -> bool {
    let at = |kind: fn(&Item<'_>) -> bool| {
        items
            .iter()
            .zip(rungs.iter())
            .position(|(item, rung)| rung.is_some() && item.as_ref().is_some_and(kind))
    };
    if let Some(at) = at(|item| matches!(item, Item::Ports(_))) {
        rungs[at] = match rungs[at] {
            Some(Rung::Ports(PortsRung::All | PortsRung::Short)) => {
                Some(Rung::Ports(PortsRung::Count))
            }
            _ => None,
        };
        return true;
    }
    if let Some(at) = at(|item| matches!(item, Item::Gauge(_))) {
        rungs[at] = None;
        return true;
    }
    if let Some(at) = at(|item| matches!(item, Item::Buttons(_) | Item::SignIn)) {
        rungs[at] = match rungs[at] {
            Some(Rung::Buttons { list, hint: true }) => Some(Rung::Buttons { list, hint: false }),
            Some(Rung::Buttons {
                list: true,
                hint: false,
            }) => Some(Rung::Buttons {
                list: false,
                hint: false,
            }),
            _ => None,
        };
        return true;
    }
    false
}

/// The plan with `rungs` if the lead's `lead_width` columns, the gap and the
/// items fit in `available`; `None` if they do not. The items are
/// right-aligned: the last ends at the row's edge.
fn place<'a>(
    lead: Lead<'a>,
    lead_width: usize,
    items: &[Option<Item<'a>>; ITEMS],
    rungs: &[Option<Rung>; ITEMS],
    available: usize,
) -> Option<Plan<'a>> {
    let shown = items
        .iter()
        .zip(rungs.iter())
        .filter_map(|(item, rung)| Some((item.as_ref()?, (*rung)?)));
    let (count, items_width) = shown.fold((0_usize, 0_usize), |(count, width), (item, rung)| {
        (count + 1, width + item.width(rung))
    });
    let zone = items_width + GAP * count.saturating_sub(1);
    // The gap only between two drawn things.
    let gap = if count > 0 && lead_width > 0 { GAP } else { 0 };
    if lead_width + gap + zone > available {
        return None;
    }
    let mut placed = [None; ITEMS];
    let mut end = available;
    for (slot, (item, rung)) in placed.iter_mut().zip(items.iter().zip(rungs.iter())).rev() {
        let (Some(item), Some(rung)) = (item, *rung) else {
            continue;
        };
        let start = end - item.width(rung);
        *slot = Some(Placed {
            item: *item,
            rung,
            start,
            end,
        });
        end = start.saturating_sub(GAP);
    }
    let lead_budget = if count > 0 {
        available - zone - GAP.min(available - zone)
    } else {
        available
    };
    Some(Plan {
        lead,
        lead_budget,
        items: placed,
    })
}

/// The dock's **bottom** row ([`plan`]); the return is the buttons' fills
/// ([`Dock::buttons`]): the upload row's list and cancel, or Sign In… in the
/// second place.
///
/// **On overflow the path is shortened from the left, the branch never.**
/// Two separate reasons: the path's information is in its tail (which folder
/// you are in), so cutting from the front would throw away the most
/// informative half; while **no** half of the branch can be thrown away — a
/// shortened branch name (`mai…`) can make the user think they are on another
/// branch, and that is the "silently wrong" class this repository forbids.
///
/// The shortening is in **character** units and does not lean on component
/// boundaries: leaning on a boundary would leave some of the available columns
/// empty, and its gain would be taste, its loss information. **This row stays
/// in character units** and its reason differs from the input row's: the
/// context row is drawn in the **small size class**, the column pitch is the
/// small face's advance and the wide path is closed there (the precedent of the
/// procedural characters). So a path with CJK still shifts columns here — a
/// known limit, guarded by `the_context_line_keeps_character_columns`.
pub(super) fn render_context(
    context: &DockContext,
    theme: &Theme,
    cols: u16,
    row: u16,
    sink: &mut impl FnMut(Cell),
) -> [Option<DockButton>; 2] {
    let available = usize::from(cols.saturating_sub(CONTEXT_COL));
    if available == 0 {
        return [None; 2];
    }
    let plan = plan(context, available);
    emit_lead(context, plan.lead, plan.lead_budget, theme, row, sink);
    let mut buttons = [None; 2];
    for placed in plan.items.into_iter().flatten() {
        emit_item(context, placed, theme, available, row, sink, &mut buttons);
    }
    buttons
}

/// Draws the lead within `budget` columns.
fn emit_lead(
    context: &DockContext,
    lead: Lead<'_>,
    budget: usize,
    theme: &Theme,
    row: u16,
    sink: &mut impl FnMut(Cell),
) {
    let (dim, quiet) = (theme.dim_linear(), theme.quiet_linear());
    match lead {
        Lead::Local { cwd, branch } => {
            let branch_chars = branch.chars().count();
            // The budget is set aside **for the branch first**; the path gets
            // the rest. The separator is counted on the path's side, because
            // if the path drops the separator drops too.
            //
            // **A branch that does not fit is not clipped, it drops.** This is
            // the degenerate-width counterpart of the branch's "no half can be
            // thrown away" rule: showing the branch `release/2.1` as `release`
            // in twelve columns would tell the user they are on **a branch that
            // does not exist**, and putting a marker (`rele…`) would not fix
            // that either. Not showing it at all is a loss of information but
            // not wrong information; a window that narrow is unreadable anyway.
            let shows_branch = branch_chars > 0 && branch_chars <= budget;
            let path_budget = if shows_branch {
                budget
                    .saturating_sub(branch_chars)
                    .saturating_sub(SEPARATOR.chars().count())
            } else {
                // If the branch is not drawn the whole width is the path's: its
                // shortening is **marked** (`…`), so it cannot be misread.
                budget
            };
            let (shows_path, path) = path_cells(cwd, path_budget, dim, quiet);
            // **The separator is drawn if both sides are filled.** A dangling
            // `|` in a directory that is not a repo would say "the branch could
            // not be read"; there is no branch to read.
            let separator = if shows_path && shows_branch {
                SEPARATOR
            } else {
                ""
            };
            let line = path
                // The separator is a division mark, not content: in the quietest tone.
                .chain(separator.chars().map(|ch| (ch, quiet)))
                .chain(
                    shows_branch
                        .then(|| branch.chars().map(|ch| (ch, dim)))
                        .into_iter()
                        .flatten(),
                );
            emit_context(line, budget, row, sink);
        }
        // `⇄ {host}` in the mark's color (the theme's `info` when unmarked),
        // two spaces, then the remote path in the two tiers of the local path;
        // no branch and no `|` — the branch belongs to the local repo, the
        // remote side's is unknown. **The host is not shortened**, for the
        // branch's reason: a shortened host name (`prod-we…`) can be read as
        // another machine. If it does not fit only `⇄` remains — saying we are
        // remote is still correct information. If the remote shell prints no
        // OSC 7 there is no path at all.
        Lead::Remote { host, cwd } => {
            let color = theme.mark_linear(context.remote_mark);
            emit_remote_head(host, color, budget, row, sink, |rest| {
                path_cells(cwd, rest, dim, quiet).1
            });
        }
        // The title in the bar's tone ([`program_color`]: its host's mark,
        // `info` unmarked — the remote host's colors, both bars are the same
        // kind of guide), the separator quiet, the detail dim, the path quiet
        // (a location, the remote path's quieter tier). **The title is never
        // shortened** — a cut version (`Python 3.1…`) reads as another one; a
        // title that does not fit leaves the row empty (the hairline still
        // says the band is a program's), a detail that does not fit with it
        // drops with the path.
        Lead::Program(bar) => {
            let title = bar.title.chars().count();
            if title == 0 || title > budget {
                return;
            }
            let tone = program_color(bar.tone, context.program_mark, theme);
            let head = program_head(bar);
            let detail = head <= budget && !bar.detail.is_empty();
            let path_budget = if head <= budget && !bar.path.is_empty() {
                budget.saturating_sub(head + REMOTE_GAP.chars().count())
            } else {
                0
            };
            let (shows_path, path) = path_cells(&bar.path, path_budget, quiet, quiet);
            let line = bar
                .title
                .chars()
                .map(|ch| (ch, tone))
                .chain(
                    detail
                        .then(|| {
                            PROGRAM_DETAIL
                                .chars()
                                .map(|ch| (ch, quiet))
                                .chain(bar.detail.chars().map(|ch| (ch, dim)))
                        })
                        .into_iter()
                        .flatten(),
                )
                .chain(
                    shows_path
                        .then(|| REMOTE_GAP.chars().map(|ch| (ch, quiet)))
                        .into_iter()
                        .flatten(),
                )
                .chain(path);
            emit_context(line, budget, row, sink);
        }
        // `⇄ {host}` in the mark's color (the remote form's prefix and color
        // are kept), the text dim and **shortened from the right** with `…`:
        // its information is at the start (which file, which number). The
        // result's tone at the start of the text: success green, error red.
        Lead::Transfer(transfer) => {
            let accent = theme.mark_linear(transfer.mark);
            let toned = match transfer.tone {
                TransferTone::Quiet => dim,
                TransferTone::Success => theme.success_linear(),
                TransferTone::Error => theme.error_linear(),
            };
            let lead = transfer.lead;
            emit_remote_head(&transfer.host, accent, budget, row, sink, |rest| {
                let chars = transfer.body.chars().count();
                let (shown, clipped) = if chars <= rest {
                    (chars, false)
                } else {
                    // The mark itself is a column too.
                    (rest.saturating_sub(1), rest > 0)
                };
                transfer
                    .body
                    .chars()
                    .take(shown)
                    .enumerate()
                    .map(move |(i, ch)| (ch, if i < lead { toned } else { dim }))
                    .chain(clipped.then_some((ELLIPSIS, dim)))
            });
        }
    }
}

/// `⇄ {host}  {tail}` within `budget`: the tail gets what is left after the
/// head and the gap (`tail(rest)`); `⇄` alone if the host does not fit.
fn emit_remote_head<I: Iterator<Item = (char, LinearRgba)>>(
    host: &str,
    color: LinearRgba,
    budget: usize,
    row: u16,
    sink: &mut impl FnMut(Cell),
    tail: impl FnOnce(usize) -> I,
) {
    let mark = std::iter::once((REMOTE_MARK, color));
    let head = remote_head(host);
    if head > budget {
        emit_context(mark, budget, row, sink);
        return;
    }
    let rest = budget.saturating_sub(head + REMOTE_GAP.chars().count());
    let line = mark
        .chain(std::iter::once((' ', color)))
        .chain(host.chars().map(|ch| (ch, color)))
        .chain(REMOTE_GAP.chars().map(|ch| (ch, color)))
        .chain(tail(rest));
    emit_context(line, budget, row, sink);
}

/// Draws a placed item; a button's fill goes to `buttons`.
fn emit_item(
    context: &DockContext,
    placed: Placed<'_>,
    theme: &Theme,
    available: usize,
    row: u16,
    sink: &mut impl FnMut(Cell),
    buttons: &mut [Option<DockButton>; 2],
) {
    let dim = theme.dim_linear();
    match (placed.item, placed.rung) {
        (Item::Ports(ports), Rung::Ports(rung)) => {
            let cells = ports_cells(ports, rung).map(|(ch, tone)| {
                let color = match tone {
                    PortTone::Mark => theme.quiet_linear(),
                    PortTone::Port => theme.success_linear(),
                    PortTone::Closed | PortTone::More => dim,
                };
                (ch, color)
            });
            emit_context_at(placed.start, cells, available, row, sink);
        }
        (Item::Gauge(stats), Rung::Gauge(step)) => {
            let gauge = gauge(stats, step);
            let start = placed.start;
            let cells = gauge.cells().iter().map(|&(ch, tone)| {
                let color = match tone {
                    Tone::Quiet | Tone::Level(StatsLevel::Normal) => dim,
                    Tone::Level(StatsLevel::Warning) => theme.warning_linear(),
                    Tone::Level(StatsLevel::Critical) => theme.error_linear(),
                    Tone::Calm => theme.success_linear(),
                };
                (ch, color)
            });
            emit_context_at(start, cells, available, row, sink);
        }
        // The label in the foreground, the fill and border in the mark's color.
        (Item::SignIn, _) => {
            let color = theme.mark_linear(context.remote_mark);
            let label = ButtonLabel::SignIn
                .chars()
                .map(|ch| (ch, theme.foreground_linear()));
            emit_context_at(placed.start + BUTTON_PAD, label, available, row, sink);
            let hover = context.footer_hover == Some(FooterControl::SignIn);
            buttons[1] = Some(DockButton {
                // audit: `end ≤ available ≤ cols` and `cols` is `u16`.
                start: CONTEXT_COL + placed.start as u16,
                end: CONTEXT_COL + placed.end as u16,
                color,
                state: if hover {
                    ButtonState::Hover
                } else {
                    ButtonState::Idle
                },
            });
        }
        // The label is in the **foreground** — the row's only foreground
        // text, so what is to be clicked stands apart from what is to be read;
        // the `⌘.` hint is dim, in the foreground when the mouse is over.
        (Item::Buttons(transfer), Rung::Buttons { .. }) => {
            let accent = theme.mark_linear(transfer.mark);
            let controls = transfer.controls;
            for (slot, button) in buttons.iter_mut().zip(button_spans(transfer, placed)) {
                let Some(button) = button else {
                    continue;
                };
                let state = match button.control {
                    FooterControl::List if controls.list_open => ButtonState::Pressed,
                    control if context.footer_hover == Some(control) => ButtonState::Hover,
                    _ => ButtonState::Idle,
                };
                let hint_color = if state == ButtonState::Idle {
                    dim
                } else {
                    theme.foreground_linear()
                };
                let label = button
                    .label
                    .chars()
                    .map(|ch| (ch, theme.foreground_linear()))
                    .chain(
                        button
                            .hint
                            .then(|| {
                                std::iter::once((' ', dim))
                                    .chain(CANCEL_HINT.chars().map(|ch| (ch, hint_color)))
                            })
                            .into_iter()
                            .flatten(),
                    );
                emit_context_at(button.start + BUTTON_PAD, label, available, row, sink);
                *slot = Some(DockButton {
                    // audit: `end ≤ available ≤ cols` and `cols` is `u16`.
                    start: CONTEXT_COL + button.start as u16,
                    end: CONTEXT_COL + button.end as u16,
                    color: accent,
                    state,
                });
            }
        }
        (Item::Hint(hint), _) => {
            emit_context_at(
                placed.start,
                hint.chars().map(|ch| (ch, dim)),
                available,
                row,
                sink,
            );
        }
        // An item is only ever placed with its own rungs.
        _ => {}
    }
}

/// An upload button of a placed [`Item::Buttons`]: its control, label, hint
/// and context-local range — inner padding included, i.e. the whole of the
/// fill and the hit area.
#[derive(Clone, Copy)]
struct ButtonSpan {
    control: FooterControl,
    label: ButtonLabel,
    hint: bool,
    start: usize,
    end: usize,
}

/// The placed buttons, left to right: the list (if shown) and the cancel —
/// right-aligned, the cancel at the item's end.
fn button_spans(transfer: &Transfer, placed: Placed<'_>) -> [Option<ButtonSpan>; 2] {
    let Rung::Buttons { list, hint } = placed.rung else {
        return [None; 2];
    };
    let cancel = cancel_label(transfer);
    let cancel_start = placed.end - button_width(cancel, hint);
    let cancel = ButtonSpan {
        control: FooterControl::Cancel,
        label: cancel,
        hint,
        start: cancel_start,
        end: placed.end,
    };
    let list = list.then(|| {
        let label = list_label(transfer);
        let end = cancel_start - BUTTON_GAP;
        ButtonSpan {
            control: FooterControl::List,
            label,
            hint: false,
            start: end - button_width(label, false),
            end,
        }
    });
    [list, Some(cancel)]
}

/// A port cell's tone; the color is resolved at drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PortTone {
    /// The mark: quiet.
    Mark,
    /// A port number that opens from this Mac: the theme's `success` — a
    /// live server.
    Port,
    /// A server's port that opens only through the ssh connection: dim.
    Closed,
    /// `+N` and a count: dim.
    More,
}

/// The ports' cells at `rung`: `↗ :3000  :6006`, `↗ :3000 +1`, `↗ 2`.
fn ports_cells(
    ports: &[FooterPort],
    rung: PortsRung,
) -> impl Iterator<Item = (char, PortTone)> + '_ {
    let shown = match rung {
        PortsRung::All => ports.len(),
        PortsRung::Short => ports.len().min(1),
        PortsRung::Count => 0,
    };
    let more = ports.len() - shown;
    let count = (rung == PortsRung::Count).then_some(ports.len());
    let plus = (rung == PortsRung::Short && more > 0).then_some(more);
    [(PORTS_MARK, PortTone::Mark), (' ', PortTone::Mark)]
        .into_iter()
        .chain(ports[..shown].iter().enumerate().flat_map(|(i, port)| {
            let gap = if i > 0 { GAP } else { 0 };
            let tone = if port.open {
                PortTone::Port
            } else {
                PortTone::Closed
            };
            std::iter::repeat_n((' ', PortTone::Mark), gap)
                .chain(std::iter::once((':', tone)))
                .chain(decimal(port.port).map(move |digit| (digit, tone)))
        }))
        .chain(plus.into_iter().flat_map(|more| {
            [(' ', PortTone::More), ('+', PortTone::More)]
                .into_iter()
                .chain(
                    decimal(u16::try_from(more).unwrap_or(u16::MAX)).map(|d| (d, PortTone::More)),
                )
        }))
        .chain(count.into_iter().flat_map(|count| {
            decimal(u16::try_from(count).unwrap_or(u16::MAX)).map(|d| (d, PortTone::More))
        }))
}

/// The ports' width at `rung` — [`ports_cells`]'s count.
fn ports_width(ports: &[FooterPort], rung: PortsRung) -> usize {
    ports_cells(ports, rung).count()
}

/// The port whose `:N` covers context-local column `col` of a placed ports
/// item; `None` on the mark, a `+N` or a count.
fn port_at(ports: &[FooterPort], placed: Placed<'_>, col: usize) -> Option<u16> {
    let Rung::Ports(rung) = placed.rung else {
        return None;
    };
    let shown = match rung {
        PortsRung::All => ports.len(),
        PortsRung::Short => ports.len().min(1),
        PortsRung::Count => 0,
    };
    // After the mark and its space.
    let mut start = placed.start + 2;
    for (i, port) in ports[..shown].iter().enumerate() {
        if i > 0 {
            start += GAP;
        }
        let end = start + 1 + decimal(port.port).count();
        if (start..end).contains(&col) {
            return Some(port.port);
        }
        start = end;
    }
    None
}

/// The clickable parts of `context`'s context row on a `budget`-column row,
/// with their **dock-local** column ranges `[start, end)` — the hand cursor's
/// rectangles. From the drawing's plan.
pub fn footer_spans(
    context: &DockContext,
    budget: u16,
) -> impl Iterator<Item = (FooterControl, u16, u16)> {
    let available = usize::from(budget.saturating_sub(CONTEXT_COL));
    let mut spans = [None; 4];
    if available > 0 {
        let plan = plan(context, available);
        let mut at = 0;
        for placed in plan.items.into_iter().flatten() {
            let mut push = |control, start: usize, end: usize| {
                if let Some(slot) = spans.get_mut(at) {
                    // audit: `end ≤ available ≤ budget` and `budget` is `u16`.
                    *slot = Some((
                        control,
                        CONTEXT_COL + start as u16,
                        CONTEXT_COL + end as u16,
                    ));
                    at += 1;
                }
            };
            match placed.item {
                Item::Ports(_) => push(FooterControl::Ports, placed.start, placed.end),
                Item::Gauge(_) => push(FooterControl::Stats, placed.start, placed.end),
                Item::SignIn => push(FooterControl::SignIn, placed.start, placed.end),
                Item::Buttons(transfer) => {
                    for button in button_spans(transfer, placed).into_iter().flatten() {
                        push(button.control, button.start, button.end);
                    }
                }
                Item::Hint(_) => {}
            }
        }
    }
    spans.into_iter().flatten()
}

/// What lies under dock-local column `col` of the context row (`budget` is the
/// row's budget, [`DockCols::context`]); `None` → nothing clickable. The
/// mouse's only input, from the drawing's plan; a range is the whole of the
/// part — a button's inner padding, the gauge's blank columns.
pub fn footer_hit(context: &DockContext, budget: u16, col: u16) -> Option<FooterHit> {
    let (control, start, _) =
        footer_spans(context, budget).find(|&(_, start, end)| (start..end).contains(&col))?;
    let port = if control == FooterControl::Ports {
        let available = usize::from(budget.saturating_sub(CONTEXT_COL));
        plan(context, available)
            .items
            .into_iter()
            .flatten()
            .find(|placed| matches!(placed.item, Item::Ports(_)))
            .and_then(|placed| {
                let Item::Ports(ports) = placed.item else {
                    return None;
                };
                debug_assert_eq!(usize::from(start - CONTEXT_COL), placed.start);
                port_at(ports, placed, usize::from(col - CONTEXT_COL))
            })
    } else {
        None
    };
    Some(FooterHit { control, port })
}

/// `control`'s dock-local column range `[start, end)` on the context row;
/// `None` if it is not drawn. A popover's anchor — tied to the part, not to
/// the clicked point. The inverse of [`footer_hit`], from the same plan.
pub fn footer_span(
    context: &DockContext,
    budget: u16,
    control: FooterControl,
) -> Option<(u16, u16)> {
    footer_spans(context, budget)
        .find(|&(found, _, _)| found == control)
        .map(|(_, start, end)| (start, end))
}

/// A guide bar's tone as a color: the title and the top hairline — its
/// host's mark ([`DockContext::program_mark`]), `info` when unmarked, the
/// remote status bar's mapping; a root shell's `error` whatever the mark.
pub(super) fn program_color(tone: ProgramTone, mark: HostMark, theme: &Theme) -> LinearRgba {
    match tone {
        ProgramTone::Info => theme.mark_linear(mark),
        ProgramTone::Error => theme.error_linear(),
    }
}

/// One of the load indicator's three values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatsMetric {
    Cpu,
    Mem,
    /// The root file system.
    Disk,
}

/// A value's two thresholds, in percent: at `warning` the number takes the
/// theme's `warning`, at `critical` its `error` and a `▲`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatsThreshold {
    pub warning: u8,
    pub critical: u8,
}

/// The thresholds of [`StatsMetric::Cpu`], `Mem` and `Disk`, in that order — a
/// **design constant**, not a measurement (the approved design's numbers). The
/// context row's colors and the popover's bars read this single table. Disk's
/// warning is also the line below which disk is not shown at all: a full disk
/// is news, a half-full one is not.
pub const STATS_THRESHOLDS: [StatsThreshold; 3] = [
    StatsThreshold {
        warning: 70,
        critical: 90,
    },
    StatsThreshold {
        warning: 80,
        critical: 92,
    },
    StatsThreshold {
        warning: 85,
        critical: 95,
    },
];

/// How severe a value is ([`StatsMetric::level`]); ordered, the worst is the
/// largest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum StatsLevel {
    #[default]
    Normal,
    Warning,
    Critical,
}

impl StatsMetric {
    /// The label drawn before the number — a UI string.
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Mem => "mem",
            Self::Disk => "disk",
        }
    }

    /// This value's thresholds, from [`STATS_THRESHOLDS`].
    pub fn threshold(self) -> StatsThreshold {
        STATS_THRESHOLDS[self as usize]
    }

    /// The severity of `percent`; a threshold is reached **at** its value.
    pub fn level(self, percent: u8) -> StatsLevel {
        let threshold = self.threshold();
        if percent >= threshold.critical {
            StatsLevel::Critical
        } else if percent >= threshold.warning {
            StatsLevel::Warning
        } else {
            StatsLevel::Normal
        }
    }
}

/// A rung of the indicator's ladder, widest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GaugeStep {
    /// `cpu ▂▃▅▇▅▃▂▁ 23%  mem 61%`.
    Spark,
    /// `cpu 23%  mem 61%`.
    Numbers,
    /// `●`, or only the values past their threshold.
    Alerts,
    /// The worst single value: severity first, then the number.
    Worst,
}

/// The ladder of each form; the last rung is always [`GaugeStep::Worst`].
fn ladder(form: StatsForm) -> &'static [GaugeStep] {
    match form {
        StatsForm::Sparkline => &[GaugeStep::Spark, GaugeStep::Numbers, GaugeStep::Worst],
        StatsForm::Numbers => &[GaugeStep::Numbers, GaugeStep::Worst],
        StatsForm::Alerts => &[GaugeStep::Alerts, GaugeStep::Worst],
    }
}

/// A gauge character's tone; the color is resolved at drawing (the theme is
/// not the layout's input).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tone {
    /// Labels, the sparkline and the gaps: dim.
    Quiet,
    /// A number and its `▲`: dim below the threshold, then `warning`/`error`.
    Level(StatsLevel),
    /// The alerts form's `●`: `success`.
    Calm,
}

/// The widest gauge: `cpu ▁▁▁▁▁▁▁▁ ▲100%  mem ▲100%  disk ▲100%` is 41
/// characters; a fixed capacity keeps per-frame allocation at zero.
pub(super) const GAUGE_MAX: usize = 48;

/// A rung's characters, in a fixed buffer. The context row counts characters
/// (`render_context`'s doc), and every character here is one column.
#[derive(Clone, Copy)]
pub(super) struct Gauge {
    cells: [(char, Tone); GAUGE_MAX],
    len: usize,
}

/// A number's column, by its level: below the critical threshold `NN%` —
/// every threshold is under 100, so the number has at most two digits —
/// and at it `▲100%`.
fn value_field(level: StatsLevel) -> usize {
    if level == StatsLevel::Critical { 5 } else { 3 }
}

impl Gauge {
    fn new() -> Self {
        Self {
            cells: [(' ', Tone::Quiet); GAUGE_MAX],
            len: 0,
        }
    }

    pub(super) fn cells(&self) -> &[(char, Tone)] {
        &self.cells[..self.len]
    }

    pub(super) fn width(&self) -> usize {
        self.len
    }

    /// The capacity is a guard, not a policy: [`GAUGE_MAX`] holds the widest rung.
    fn push(&mut self, ch: char, tone: Tone) {
        if let Some(slot) = self.cells.get_mut(self.len) {
            *slot = (ch, tone);
            self.len += 1;
        }
    }

    fn text(&mut self, text: &str, tone: Tone) {
        for ch in text.chars() {
            self.push(ch, tone);
        }
    }

    /// The gap between two values: two columns, the remote form's own gap;
    /// nothing before the first.
    fn gap(&mut self) {
        if self.len > 0 {
            self.text(REMOTE_GAP, Tone::Quiet);
        }
    }

    /// `[label ][▲]{n}%`: the label dim, the number in its severity; `▲` glued
    /// to the number when critical.
    ///
    /// **The number is right-aligned in a column of its level's width**
    /// ([`value_field`]; `cpu  9%`, `cpu 23%`), `top`'s tabular way: the
    /// digits change with every sample and the gauge is right-aligned, so a
    /// number sized by its digits would move the gauge's start — and the
    /// ports left of it — under a still pointer (measured: 56, 55, 53 at 80
    /// columns for 9, 23 and 100). The column is the level's, not the widest
    /// number's: a number reaches three digits and `▲` only at the critical
    /// threshold, where the colour and the mark change anyway — a change of
    /// state, like the disk joining past its threshold — and a column sized
    /// for `▲100%` at every sample left a wide hole before a calm gauge.
    fn value(&mut self, metric: StatsMetric, percent: u8, label: bool) {
        if label {
            self.text(metric.label(), Tone::Quiet);
            self.push(' ', Tone::Quiet);
        }
        let level = metric.level(percent);
        let critical = level == StatsLevel::Critical;
        let drawn = usize::from(critical) + decimal(u16::from(percent)).count() + 1;
        for _ in drawn..value_field(level) {
            self.push(' ', Tone::Quiet);
        }
        if critical {
            self.push(STATS_CRITICAL, Tone::Level(level));
        }
        for digit in decimal(u16::from(percent)) {
            self.push(digit, Tone::Level(level));
        }
        self.push('%', Tone::Level(level));
    }

    /// The sparkline's eight columns, right-aligned: missing samples on the
    /// left are blank — a group whose width changed with every sample would
    /// move the path's budget too.
    fn spark(&mut self, history: &[u8]) {
        let shown = &history[history.len().saturating_sub(STATS_HISTORY)..];
        for _ in shown.len()..STATS_HISTORY {
            self.push(' ', Tone::Quiet);
        }
        for &level in shown {
            let block = char::from_u32(SPARK_BASE + u32::from(level.min(7))).unwrap_or(' ');
            self.push(block, Tone::Quiet);
        }
    }
}

/// The values shown at all: CPU once it has a value (the first sample has
/// none), memory always, disk only past its warning.
fn shown_values(stats: &RemoteStats) -> impl Iterator<Item = (StatsMetric, u8)> {
    let disk = (StatsMetric::Disk.level(stats.disk) > StatsLevel::Normal).then_some(stats.disk);
    stats
        .cpu
        .map(|cpu| (StatsMetric::Cpu, cpu))
        .into_iter()
        .chain(std::iter::once((StatsMetric::Mem, stats.mem)))
        .chain(disk.map(|disk| (StatsMetric::Disk, disk)))
}

/// The worst shown value: severity first, then the number; on a tie the
/// first in `cpu, mem, disk` order.
fn worst(stats: &RemoteStats) -> (StatsMetric, u8) {
    let rank = |(metric, value): (StatsMetric, u8)| (metric.level(value), value);
    let mut shown = shown_values(stats);
    // Memory is always shown, so the first value always exists.
    let first = shown.next().unwrap_or((StatsMetric::Mem, stats.mem));
    shown.fold(
        first,
        |best, next| if rank(next) > rank(best) { next } else { best },
    )
}

/// A rung's characters. **CPU without a value is left out** rather than
/// guessed: the first sample carries only counters and the second follows a
/// second later.
pub(super) fn gauge(stats: &RemoteStats, step: GaugeStep) -> Gauge {
    let mut gauge = Gauge::new();
    match step {
        GaugeStep::Spark => {
            for (metric, value) in shown_values(stats) {
                if metric == StatsMetric::Cpu {
                    gauge.text(metric.label(), Tone::Quiet);
                    gauge.push(' ', Tone::Quiet);
                    gauge.spark(stats.history());
                    gauge.push(' ', Tone::Quiet);
                    gauge.value(metric, value, false);
                } else {
                    gauge.gap();
                    gauge.value(metric, value, true);
                }
            }
        }
        GaugeStep::Numbers => {
            for (metric, value) in shown_values(stats) {
                gauge.gap();
                gauge.value(metric, value, true);
            }
        }
        GaugeStep::Alerts => {
            for (metric, value) in shown_values(stats) {
                if metric.level(value) > StatsLevel::Normal {
                    gauge.gap();
                    gauge.value(metric, value, true);
                }
            }
            if gauge.width() == 0 {
                gauge.push(STATS_CALM, Tone::Calm);
            }
        }
        GaugeStep::Worst => {
            let (metric, value) = worst(stats);
            gauge.value(metric, value, true);
        }
    }
    gauge
}

/// The button's label — a UI string. **A verb, not an icon** (the user,
/// visual check: `✕` also read as "close", `▴` could not be told from text at
/// the small point size).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ButtonLabel {
    Cancel,
    CancelAll,
    /// With the number of items in the list; the same label while the list is
    /// open (`Hide files` was dropped, the button is in the pressed tone).
    /// "Transfers", not "files": the list carries both directions.
    ShowFiles(u16),
    /// The ssh status bar's login: the ellipsis says a sheet opens.
    SignIn,
}

impl ButtonLabel {
    /// The label's characters — no per-frame allocation, the number is printed in place.
    fn chars(self) -> impl Iterator<Item = char> + Clone {
        let (head, count, tail) = match self {
            Self::Cancel => ("Cancel", None, ""),
            Self::CancelAll => ("Cancel all", None, ""),
            Self::ShowFiles(items) => ("Show transfers (", Some(items), ")"),
            Self::SignIn => ("Sign In\u{2026}", None, ""),
        };
        head.chars()
            .chain(count.into_iter().flat_map(decimal))
            .chain(tail.chars())
    }

    fn len(self) -> usize {
        self.chars().count()
    }
}

/// The decimal digits of `n`, without separators.
fn decimal(n: u16) -> impl Iterator<Item = char> + Clone {
    let n = u32::from(n);
    let digits = n.checked_ilog10().unwrap_or(0) + 1;
    (0..digits)
        .rev()
        // audit: `n / 10^p % 10` 0..=9, `from_digit` hep `Some`.
        .map(move |p| char::from_digit(n / 10u32.pow(p) % 10, 10).unwrap_or('0'))
}

/// The button's inner padding, in columns — one empty column on each side of
/// the label and the fill covers them too. A design constant: one column of
/// the small class is roughly half a large cell, the approved design's inner
/// padding.
const BUTTON_PAD: usize = 1;

/// The gap between the two buttons, in columns: so the fills do not touch.
const BUTTON_GAP: usize = 1;

/// The cancel's keyboard hint — a UI string; the menu's Cancel Upload (⌘.)
/// key. Inside the button and dim: it teaches cancelling from the keyboard,
/// it does not compete with the label.
pub(super) const CANCEL_HINT: &str = "⌘.";

/// A button's width, in columns: `pad + label [+ space + ⌘.] + pad`.
fn button_width(label: ButtonLabel, hint: bool) -> usize {
    let hint = if hint {
        1 + CANCEL_HINT.chars().count()
    } else {
        0
    };
    BUTTON_PAD + label.len() + hint + BUTTON_PAD
}

/// A path's cells on the context row, shortened **from the left** to `budget`
/// characters and in two tiers; the first value is whether the path shows.
///
/// The part common to the local and remote forms: the rule is the same in
/// both, because both are the answer to the "which folder are you in"
/// question.
fn path_cells(
    path: &str,
    budget: usize,
    normal: LinearRgba,
    quiet: LinearRgba,
) -> (bool, impl Iterator<Item = (char, LinearRgba)> + '_) {
    let path_chars = path.chars().count();
    // `skip` is the number of characters dropped from the **start** of the
    // path; `mark` is the shortening's visible mark. If the path is not drawn
    // at all both are silent from the start.
    let (mark, skip) = if budget == 0 || path_chars == 0 {
        (None, path_chars)
    } else if path_chars <= budget {
        (None, 0)
    } else {
        // The mark itself is a column too: `budget - 1` characters from the tail.
        (Some(ELLIPSIS), path_chars - (budget - 1))
    };
    let shows = mark.is_some() || skip < path_chars;

    // **The path's last component stands out, what precedes it recedes.** The
    // information the user looks for is "which folder am I in"; the parent
    // directories are the context that places it. With both in the same tone
    // the eye had to search for the last component.
    //
    // The dim one is **not a new color**: the dim of the dim
    // (`Theme::quiet_linear`), i.e. the second application of the same rule
    // (`dim_toward`). The hairline is one step further out and there is a
    // reason it stops there: it is **not ink**, this is still a path that
    // needs to be read.
    //
    // The **character** index of the last component in the path: what is
    // after the last `/`. No splitting, `enumerate` not `char_indices`: the
    // `skip` above also counts characters and the two must be in the same unit.
    let head_end = path
        .chars()
        .enumerate()
        .filter(|(_, ch)| *ch == '/')
        .map(|(index, _)| index + 1)
        .last()
        .unwrap_or(0);
    // If the last component is empty (`/`, or a trailing slash) no distinction
    // is made: the whole path stands out. The wrong side is the safe side —
    // over-emphasizing hides no information, dimming everything would.
    let head_end = if head_end >= path_chars { 0 } else { head_end };

    let cells = mark
        // The shortening mark stands in the place of the dropped **parent**
        // directories, i.e. in the same tone as them.
        .map(|ch| (ch, quiet))
        .into_iter()
        .chain(
            path.chars()
                .skip(skip)
                .enumerate()
                .map(move |(offset, ch)| {
                    (
                        ch,
                        if skip + offset < head_end {
                            quiet
                        } else {
                            normal
                        },
                    )
                }),
        );
    (shows, cells)
}

/// Prints the context row's cells to the sink within `available` columns.
fn emit_context(
    line: impl Iterator<Item = (char, LinearRgba)>,
    available: usize,
    row: u16,
    sink: &mut impl FnMut(Cell),
) {
    emit_context_at(0, line, available, row, sink);
}

/// [`emit_context`], starting at the context-local column `start` (the upload
/// row's right-aligned buttons).
fn emit_context_at(
    start: usize,
    line: impl Iterator<Item = (char, LinearRgba)>,
    available: usize,
    row: u16,
    sink: &mut impl FnMut(Cell),
) {
    // `take` is a guard, not a policy: the caller's budget already does not
    // exceed `available` columns. A cell overflowing on the right would write
    // outside the grid and that arithmetic error stops silently here.
    for (offset, (ch, fg)) in line.take(available.saturating_sub(start)).enumerate() {
        let offset = start + offset;
        // A space produces no glyph (`cell`'s rule); both sides of the
        // separator are eliminated here.
        if ch == ' ' {
            continue;
        }
        sink(Cell {
            // audit: `offset < available ≤ cols` and `cols` is `u16`; the sum cannot overflow.
            col: CONTEXT_COL + offset as u16,
            row,
            ch: Some(ch),
            // The whole row stays dim — the context is readable but does not
            // compete with the input row — and there is a second tier **inside**
            // it (above). The remote form's host is the one exception: distance
            // is this row's actual news.
            fg,
            ..Cell::default()
        });
    }
}
