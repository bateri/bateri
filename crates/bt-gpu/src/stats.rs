//! Measurement samples: the frame path's two CPU spans, the GPU delta and the startup
//! stamp. **Prints nothing** — it only accumulates; `bt-shell` writes the report.
//!
//! The gate is `BT_FRAME_STATS`: while it is off, nothing is born from this module (no
//! ring is allocated, no clock is read). While it is on, the cost **per sample** is one
//! atomic counter plus one `store` (three columns per frame, so three of each), on top
//! of three clock reads and one `OnceLock` lookup — no file, no lock, no allocation
//! (R4.3). A frame that could not be measured costs the same order: a single
//! `fetch_add` ([`Ring::reject`]).
//!
//! The statistics (p95 and worst) are computed **at shutdown** and in-process
//! ([`Samples::p95_and_worst`]); no file is written (R5).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The per-second multiplier of the ring capacity — an **allocation ceiling**, not a
/// measured refresh rate. It is the top end of ProMotion; a faster display fills the
/// ring, and the dropped samples are counted too (see [`Samples::dropped`]).
const MAX_REFRESH_HZ: u64 = 120;

/// The absolute ceiling of the capacity (~10 minutes @120 Hz). With three columns × 8
/// bytes it is ~1.7 MB: it bounds the hole a long run can open in memory. In a run that
/// exceeds it the ring fills, the oldest sample is dropped and that is **counted**.
const MAX_CAPACITY: u64 = MAX_REFRESH_HZ * 600;

/// The smallest sample count at which the p95 can be meaningful — **derived, not chosen**.
///
/// The p95 by the nearest-rank method is the `ceil(0.95 × n)`-th element of the sorted
/// array. For `n < 20` that element is **the last one**, so the p95 and the worst
/// collapse into the same number: the report prints two tokens, both carry the same
/// number, and one of them gets read as a "distribution". The floor is the first `n` at
/// which this collapse ends (`ceil(0.95 × 20) = 19 < 20`).
///
/// This is the gate of R5.6: below the floor the number is **not computed**
/// ([`Samples::p95_and_worst`] returns `None`). How that is stated in the report is
/// **not** this layer's job — the upper layer that prints the token knows; copying a
/// token's spelling here would both violate the layering direction and silently go stale
/// at the next rename (it went stale once).
pub const MIN_SAMPLES: usize = 20;

/// The state of one column at shutdown: the samples standing in the ring (oldest to
/// newest, nanoseconds), those dropped because they did not fit, and those rejected
/// without ever being written.
///
/// Neither counter is decoration: a report that computes the p95 over few samples looks
/// *good* (R5.2), and the number that **could not** be collected must be as readable as
/// the number that was. They are separate because they are separate failures:
///
/// - `dropped` — a sample was produced but did not fit in the ring (the run was faster
///   than the refresh ceiling). It says the capacity stayed small. **It is per
///   column**: a report that wants a single number takes the largest of the `dropped`
///   values of the three snapshots it holds — **not** a fresh read, because the cursors
///   may still move at shutdown (the poll of a frame still in flight) and two separate
///   reads could make the tokens contradict each other. Today the capacities of the
///   three columns are equal and the GPU cannot write more than the CPU, so the largest
///   is the CPU's; `max` stays correct even if that invariant breaks.
/// - `rejected` — no sample could be produced: [`Stats::record_gpu`] drops
///   a zero/NaN stamp and [`Stats::reject_gpu`] counts a timestamp that could
///   not be read back. Without this counter an empty GPU column could not be
///   **told apart** between "the hardware gives no stamps" and "no frame was drawn".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Samples {
    pub nanos: Vec<u64>,
    pub dropped: u64,
    pub rejected: u64,
}

impl Samples {
    /// The p95 and the worst value. **No mean**: a hitch does not move the mean but the
    /// user sees it (`/measure` → "distribution, not mean").
    ///
    /// `None` → the sample count is below [`MIN_SAMPLES`]; in that range both numbers
    /// would come from the same element (R5.6). An empty column is the extreme case of
    /// this.
    ///
    /// **It consumes itself:** the sort is done in place and at shutdown nobody else
    /// reads this vector. A caller that wants the counter half (`nanos.len()`,
    /// [`Samples::dropped`], [`Samples::rejected`]) reads them **before** this call; the
    /// type enforces the order that way.
    ///
    /// It runs once at shutdown, not on the frame path: that is why the `n log n` of the
    /// sort is legitimate.
    pub fn p95_and_worst(mut self) -> Option<(Duration, Duration)> {
        if self.nanos.len() < MIN_SAMPLES {
            return None;
        }
        // The ring yields oldest to newest; an implementation that does not sort would
        // print "the duration of the frame at 5% from the end" instead of the p95.
        self.nanos.sort_unstable();
        // `ceil(0.95 × n)` in integers: `(95 × n).div_ceil(100)`. Because of the floor,
        // `1 ≤ rank < n`, so the index neither overflows nor does the p95 collapse into
        // the worst. The product does not overflow either: `n` is at most
        // [`MAX_CAPACITY`].
        let rank = (95 * self.nanos.len()).div_ceil(100);
        let p95 = self.nanos[rank - 1];
        // The end of the sorted array; it exists because the length is ≥ [`MIN_SAMPLES`].
        let worst = self.nanos[self.nanos.len() - 1];
        Some((Duration::from_nanos(p95), Duration::from_nanos(worst)))
    }
}

/// A single column: a preallocated ring and an atomic cursor.
///
/// Lock-free, because the frame path writes it (the tick records the CPU
/// spans and, when it polls completions, the GPU delta) while `bt-shell`
/// holds the same book behind an `Arc` and reads it from its own thread: a
/// lock here would be the first lock on the frame's success path, which today
/// is atomics only.
///
/// `align(128)`: if the three columns' cursors fell on one cache line, a
/// writer and a reader on two threads would pull that line back and forth
/// (false sharing) — the measuring tool would disturb what it measures. The number was
/// **measured, not chosen**: on this machine `sysctl hw.cachelinesize` = **128** (Apple
/// Silicon), so the classic `64` could have left two neighbouring `Ring`s on the same
/// line. 128 also covers Intel's 64.
#[repr(align(128))]
struct Ring {
    /// Capacity ≥ 1 (the constructor clamps it); the cursor is taken modulo this.
    slots: Box<[AtomicU64]>,
    /// The total write count — the number **written**, not the number that fit in the
    /// ring. The dropped samples are the part of it that exceeds the capacity.
    pushed: AtomicU64,
    /// Samples never written because they could not be measured (see
    /// [`Samples::rejected`]).
    rejected: AtomicU64,
}

impl Ring {
    /// The capacity is allocated once; no allocation per frame (R4.3). The clamp lives
    /// **only here**: a value below `1` would turn the `%` into a division by zero, and
    /// a value above the ceiling would open a needless hole in memory.
    fn new(capacity: u64) -> Self {
        let capacity = capacity.clamp(1, MAX_CAPACITY);
        Self {
            slots: (0..capacity).map(|_| AtomicU64::new(0)).collect(),
            pushed: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
        }
    }

    /// The whole per-frame cost: one `fetch_add`, one `store`.
    ///
    /// Both are `Relaxed`, because there is nothing to order here: what makes the ticket
    /// unique is the **atomicity** of the RMW, not its ordering; the slot is read
    /// `Relaxed` as well. `AcqRel` would make us pay, three times per frame, for a
    /// barrier that pairs with nothing.
    fn push(&self, nanos: u64) {
        let ticket = self.pushed.fetch_add(1, Ordering::Relaxed);
        self.slot(ticket).store(nanos, Ordering::Relaxed);
    }

    /// The number of samples that did not fit in the ring and were dropped — from a read
    /// whose cursor is **given**.
    ///
    /// It does not read `pushed` itself, and that is not pedantry: [`Ring::snapshot`]
    /// uses the same cursor to derive both the dropped count and the retained range.
    /// With two separate `load`s a frame written in between (a frame still in flight
    /// being polled at shutdown) would pull the two apart and `dropped > pushed` could
    /// come out: the range would empty out and the report would say "no samples at all
    /// but some dropped".
    fn dropped_at(&self, pushed: u64) -> u64 {
        pushed.saturating_sub(self.slots.len() as u64)
    }

    /// A frame that could not be measured. It does not touch the ring, it is only
    /// counted: its cost is half of [`Ring::push`]'s (a single `fetch_add`) and the
    /// reason for it being `Relaxed` is the same — there is nothing to order here.
    fn reject(&self) {
        self.rejected.fetch_add(1, Ordering::Relaxed);
    }

    /// Ticket → slot. The result of the modulo is below `slots.len()` by definition, so
    /// there is no narrowing and no "cannot happen" fallback.
    fn slot(&self, ticket: u64) -> &AtomicU64 {
        &self.slots[(ticket % self.slots.len() as u64) as usize]
    }

    /// Read at shutdown, not on the frame path — that is why the `Vec` allocation is
    /// legitimate.
    ///
    /// **It can be read while writing continues**, and there are two windows in between:
    /// a writer that has taken its ticket but not yet written its value (between the
    /// `fetch_add` and the `store`), and, if the ring is full, the oldest slot being
    /// overwritten. Both give the same symptom — the slot is either **never written**
    /// (zero) or stale. Zero is filtered out: a real sample cannot be zero
    /// ([`Stats::record_gpu`] already rejects zero, and a frame does not take zero
    /// nanoseconds). Adding a lock would create the first lock of a write path that is
    /// atomic only; at shutdown (after `link.stop()`) there are only a frame or two in
    /// flight anyway.
    fn snapshot(&self) -> Samples {
        // The cursor is read **once** and both derivatives come from it; the reason is
        // at [`Ring::dropped_at`].
        let pushed = self.pushed.load(Ordering::Relaxed);
        // The dropped count is the same number as the ticket of the oldest **retained**
        // sample: those before it were overwritten.
        let dropped = self.dropped_at(pushed);
        Samples {
            nanos: (dropped..pushed)
                .map(|ticket| self.slot(ticket).load(Ordering::Relaxed))
                .filter(|&nanos| nanos != 0)
                .collect(),
            dropped,
            rejected: self.rejected.load(Ordering::Relaxed),
        }
    }
}

/// The run's measurement book. When the gate is on, `bt-shell` builds it, `bt-gpu`
/// fills it and at shutdown `bt-shell` reads it again.
pub struct Stats {
    /// The startup stamp: taken on the **first line of `main()`** and **carried** here.
    /// Had the constructor read it itself, the measurement would start after
    /// `Renderer::system_default()`, so device and pipeline set-up would be missed
    /// (R3.3).
    ///
    /// **Not the process start:** dyld and the Rust runtime setup finish before this
    /// stamp. The first line of `main()` is the earliest point we have, and whoever
    /// places the stamp there explains next to it why it stands there.
    since: Instant,
    /// Time to the first finished frame. `OnceLock`: written once, by the
    /// frame path's completion poll — the same pattern as `ShellWake.waker`
    /// and `Session`'s sender. A hand-rolled zero sentinel plus CAS would do the same
    /// job, but it would invent a "zero = not yet" contract and then miss a measured
    /// zero with `max(1)`.
    ///
    /// `Stats` must be `Send + Sync`: the frame path writes it and `bt-shell`
    /// reads it, the two sharing one `Arc<Stats>`.
    startup: OnceLock<Duration>,
    cpu_frame: Ring,
    cpu_encode: Ring,
    gpu: Ring,
}

impl Stats {
    /// `since` is the startup stamp, `run_seconds` the run's budget.
    ///
    /// This constructor does **not** take the stamp itself, it receives it from outside;
    /// both the reason and what it is not are in the doc of the `since` field.
    ///
    /// The capacity is derived from the run duration: `run_seconds × MAX_REFRESH_HZ`.
    /// A zero-second run falls to `Ring::new`'s lower bound (one slot) — the clamp lives
    /// in a single place and is visible there because it stands next to the `%`.
    ///
    /// **The allocation is inside the startup time:** the three rings are born here,
    /// after the stamp is taken and before the first frame finishes, so
    /// [`Stats::startup`] also counts the set-up of its own measuring tool. The cost is
    /// `run_seconds × MAX_REFRESH_HZ × 3 × 8` bytes — a derivation, not a measurement:
    /// ~8.6 KB for a three-second run, ~1.7 MB at the `MAX_CAPACITY` ceiling.
    pub fn new(since: Instant, run_seconds: u64) -> Self {
        let capacity = run_seconds.saturating_mul(MAX_REFRESH_HZ);
        Self {
            since,
            startup: OnceLock::new(),
            cpu_frame: Ring::new(capacity),
            cpu_encode: Ring::new(capacity),
            gpu: Ring::new(capacity),
        }
    }

    /// The frame's two CPU spans: `session.frame` and `draw`.
    ///
    /// Both are `Duration`, not `Option`: a frame with a missing stamp
    /// (`session.frame` returned `None` or `draw` dropped out) **never reaches**
    /// here — the shape of the call path ties this to the type. A signature taking
    /// `Option` would push that rule down to run time, and a fake "encode = 0 ns" sample
    /// would drag the p95 down.
    pub(crate) fn record_cpu(&self, frame: Duration, encode: Duration) {
        // `as_nanos` returns `u128`. An overflow would mean a 584-year frame, so it
        // cannot happen; and if it does, we **do not saturate**: a sample of `u64::MAX`
        // would own the p95 and the worst all by itself. A frame that could not be
        // measured is not a frame with an extreme value, it is a frame that does **not
        // exist** — the same principle as `record_gpu`'s zero rule. The two columns are
        // written together so their alignment is not broken.
        // Zero is dropped too, like the overflow: [`Ring::snapshot`] recognises an
        // unwritten slot by its zero, so a real zero entering the ring would vanish
        // **silently** — it would show up neither in `ornek=` nor in `gpu_elenen=`, and
        // the lengths of the two CPU columns would drift apart. The rejects are counted.
        let (Ok(frame @ 1..), Ok(encode @ 1..)) = (
            u64::try_from(frame.as_nanos()),
            u64::try_from(encode.as_nanos()),
        ) else {
            // The two columns are dropped together: they are written together so their
            // alignment is not broken, so they must be counted together as well.
            self.cpu_frame.reject();
            self.cpu_encode.reject();
            return;
        };
        self.cpu_frame.push(frame);
        self.cpu_encode.push(encode);
    }

    /// The frame's GPU delta; `start`/`end` come from the GPU's own clock
    /// (seconds, the timestamp query's).
    ///
    /// A zero stamp means nothing was measured for that frame, and a 0 ns
    /// sample would pull the p95 down. Not writing a zero sample means the GPU
    /// column can end up shorter than the CPU column — that is exactly why `Samples` is
    /// read per column.
    pub(crate) fn record_gpu(&self, start: f64, end: f64) {
        // The gate is written **positively** and `is_finite` is required: the negated
        // form of the comparison would let NaN through (`NaN <= 0.0` and `end <= NaN`
        // are both `false`), and because the `f64 → u64` conversion turns NaN into zero,
        // exactly the **fake 0 ns sample** we are trying to drop would come out.
        // Infinity is the same: it saturates to `u64::MAX` and owns the p95 by itself.
        if !(start.is_finite() && end.is_finite() && start > 0.0 && end > start) {
            // The rejection is **counted**: if it were not, an empty GPU column would
            // fold "the hardware gave no stamp" and "no frame was drawn" into the same
            // appearance and the report could not tell them apart.
            self.gpu.reject();
            return;
        }
        // The gate was on the `f64` side; the actual sample is born **after the
        // conversion**, and two ends were still open there: a delta below one nanosecond
        // is clamped to zero (the snapshot would take it for an unwritten slot), and a
        // finite but absurdly large delta **saturates** to `u64::MAX` with `as u64` and
        // owned the p95 and the worst by itself — exactly the outcome the `is_finite`
        // gate was written to prevent. A frame that could not be measured is treated as
        // **non-existent**, not as an extreme value; both are dropped and counted.
        let nanos = (end - start) * 1e9;
        if !(nanos >= 1.0 && nanos < u64::MAX as f64) {
            self.gpu.reject();
            return;
        }
        self.gpu.push(nanos as u64);
    }

    /// A frame measured without a usable GPU span (the timestamp readback
    /// failed): counted as rejected, like `record_gpu`'s zero stamps, so an
    /// empty GPU column is not mistaken for "no frame drawn".
    pub(crate) fn reject_gpu(&self) {
        self.gpu.reject();
    }

    /// The first completed frame: the startup time closes here, later ones do not touch
    /// it. The per-frame cost is the `OnceLock`'s fast path: one atomic read.
    pub(crate) fn mark_startup(&self) {
        self.startup.get_or_init(|| self.since.elapsed());
    }

    /// From the `since` stamp to the first **completed** frame. `None` → no frame has
    /// finished.
    ///
    /// Both ends are narrow and both must be named: the start is **not the process
    /// start** (see `since`), and the end is not a **presented** frame either —
    /// the completion poll says the GPU finished the submission, not that it
    /// reached the screen.
    pub fn startup(&self) -> Option<Duration> {
        self.startup.get().copied()
    }

    /// The `session.frame` span: lock wait + parsing + grid + sink.
    pub fn cpu_frame(&self) -> Samples {
        self.cpu_frame.snapshot()
    }

    /// The `draw` span: encode + commit.
    pub fn cpu_encode(&self) -> Samples {
        self.cpu_encode.snapshot()
    }

    /// The time the GPU took to process the command buffer.
    pub fn gpu(&self) -> Samples {
        self.gpu.snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> Stats {
        Stats::new(Instant::now(), 1)
    }

    fn column(nanos: Vec<u64>) -> Samples {
        // The zeros come from `Default`: when a field is added to `Samples`, this
        // fixture (and the three assertions below) should not have to be told about it.
        Samples {
            nanos,
            ..Default::default()
        }
    }

    #[test]
    fn p95_returns_none_on_empty_ring() {
        // With no samples at all there is no distribution to compute. It is the extreme
        // case of the floor but tested separately: an empty column is the only way to
        // say "the measurement never ran", and handling it with an `unwrap` would have
        // panicked on the report path.
        let stats = stats();
        assert_eq!(stats.gpu().p95_and_worst(), None);
        assert_eq!(stats.cpu_frame().p95_and_worst(), None);
        assert_eq!(column(Vec::new()).p95_and_worst(), None);
    }

    #[test]
    fn few_samples_suppress_p95() {
        // The floor is **derived, not chosen**: the index of the nearest-rank p95 is
        // `ceil(0.95 × n) − 1` and for `n < 20` that index is the **last** element, so
        // the p95 and the worst become the same number. A p95 printed below the floor
        // would present a copy of the worst as a "distribution" (R5.6).
        let below = column((1..=MIN_SAMPLES as u64 - 1).collect());
        assert_eq!(below.p95_and_worst(), None, "no number below the floor");

        // **Exactly above** the floor the two separate; if they did not, the floor
        // would do nothing but name one number twice.
        let (p95, worst) = column((1..=MIN_SAMPLES as u64).collect())
            .p95_and_worst()
            .expect("at the floor the number is computed");
        assert_eq!(worst, Duration::from_nanos(MIN_SAMPLES as u64));
        assert_eq!(p95, Duration::from_nanos(MIN_SAMPLES as u64 - 1));
        assert!(p95 < worst, "the p95 is not a copy of the worst");

        // The ring yields oldest to newest, not sorted: an implementation that skips
        // the sort goes red here.
        let reversed = column((1..=MIN_SAMPLES as u64).rev().collect());
        assert_eq!(reversed.p95_and_worst(), Some((p95, worst)));
    }

    #[test]
    fn rejected_gpu_samples_are_counted() {
        // `/code-review` finding: when a rejected frame is not counted, an empty GPU
        // column cannot be told apart between "the hardware returns zero" and "no frame
        // was drawn" — the very blindness R5.2 wants to close.
        let stats = stats();
        stats.record_gpu(0.0, 0.0);
        stats.record_gpu(f64::NAN, 1.0);
        stats.record_gpu(1.0, 1.002);
        let gpu = stats.gpu();
        assert_eq!(gpu.nanos, vec![2_000_000]);
        assert_eq!(gpu.rejected, 2, "a rejected frame is counted");
        // The CPU column was never written in this run, so nothing was rejected either.
        // The CPU's **own** rejection path is not unreachable — a zero-length span fires
        // it and `cpu_rejects_zero_spans` pins that; only the overflow arm is
        // unreachable (it would mean a 584-year frame).
        assert_eq!(stats.cpu_frame().rejected, 0);
    }

    #[test]
    fn cpu_sample_fills_both_columns() {
        // The two columns fill separately and **their order does not get mixed up**: an
        // edit that pushes both into the same ring or swaps the arguments goes red here.
        // The distinction itself is R3.1: a single span cannot separate 002 #1 (the lock
        // wait) from the encode.
        let stats = stats();
        stats.record_cpu(Duration::from_millis(3), Duration::from_micros(400));
        assert_eq!(stats.cpu_frame().nanos, vec![3_000_000]);
        assert_eq!(stats.cpu_encode().nanos, vec![400_000]);
        // An empty frame writes no sample and that is in the **type**: a frame with a
        // missing stamp never reaches `record_cpu` (the early return in `link.rs`), so
        // there is no branch to test — a test would not have been able to go red under
        // any condition.
        assert!(
            stats.gpu().nanos.is_empty(),
            "a CPU sample does not write to the GPU column"
        );
    }

    #[test]
    fn gpu_zero_timestamps_record_nothing() {
        // A zero stamp is "nothing measured" (a timestamp that never
        // started or never came back). A 0 ns sample derived from zero pulls the p95
        // down and the report looks **good** — the very blindness R5.2 warns about.
        let stats = stats();
        stats.record_gpu(0.0, 0.0);
        stats.record_gpu(0.0, 1.0);
        stats.record_gpu(2.0, 2.0);
        stats.record_gpu(3.0, 2.0);
        assert!(
            stats.gpu().nanos.is_empty(),
            "an unmeasured frame writes no sample"
        );

        stats.record_gpu(1.0, 1.002);
        assert_eq!(stats.gpu().nanos, vec![2_000_000]);
    }

    #[test]
    fn gpu_rejects_zero_and_saturating_deltas() {
        // `/code-review` finding: the gate was on the `f64` side, while the sample is
        // born **after** the conversion. A delta below one nanosecond is clamped to zero
        // and caught by the snapshot's "unwritten slot" filter (vanishing without
        // appearing in any token); a finite but absurdly large delta saturates to
        // `u64::MAX` and owns the p95 by itself.
        let stats = stats();
        // Half a nanosecond: positive, finite, `end > start` — it would have passed the
        // old gate.
        stats.record_gpu(1.0, 1.0 + 0.5e-9);
        // Finite but does not fit in `u64`: above ~1.8e10 seconds.
        stats.record_gpu(1.0, 1.0 + 2.0e10);
        assert!(
            stats.gpu().nanos.is_empty(),
            "a delta clamped to zero or saturated writes no sample"
        );
        assert_eq!(stats.gpu().rejected, 2, "both are counted");

        // Exactly one nanosecond is the smallest **legitimate** sample.
        stats.record_gpu(1.0, 1.0 + 1e-9);
        assert_eq!(stats.gpu().nanos, vec![1]);
    }

    #[test]
    fn cpu_rejects_zero_spans() {
        // The CPU side of the same blindness: had a zero-length span entered the ring,
        // the snapshot would have taken it for an unwritten slot and dropped it, and the
        // lengths of the two CPU columns would have drifted apart.
        let stats = stats();
        stats.record_cpu(Duration::ZERO, Duration::from_millis(1));
        stats.record_cpu(Duration::from_millis(1), Duration::ZERO);
        assert!(stats.cpu_frame().nanos.is_empty());
        assert!(stats.cpu_encode().nanos.is_empty());
        assert_eq!(stats.cpu_frame().rejected, 2);
        assert_eq!(
            stats.cpu_encode().rejected,
            2,
            "the two columns are written together, so they are rejected together"
        );
    }

    #[test]
    fn gpu_rejects_nan_and_infinity() {
        // `/code-review` finding: when the gate was written negatively (`start <= 0.0 ||
        // end <= start`) NaN **passed** — both comparisons return `false` — and because
        // `f64 → u64` turns NaN into zero, exactly the 0 ns sample we were trying to
        // drop entered the ring. Infinity likewise saturated to `u64::MAX` and owned the
        // p95 by itself.
        let stats = stats();
        for (start, end) in [
            (f64::NAN, 1.0),
            (1.0, f64::NAN),
            (f64::NAN, f64::NAN),
            (1.0, f64::INFINITY),
            (f64::NEG_INFINITY, 1.0),
        ] {
            stats.record_gpu(start, end);
        }
        assert!(
            stats.gpu().nanos.is_empty(),
            "a frame that could not be measured is treated as NON-EXISTENT, not extreme"
        );
    }

    #[test]
    fn full_ring_drops_oldest_and_counts() {
        // Exceeding the capacity is **not silent**: the oldest is dropped and the
        // dropped one is counted. Because the capacity is `run_seconds ×
        // MAX_REFRESH_HZ`, we set up the overflow by hand here.
        let ring = Ring::new(2);
        ring.push(1);
        ring.push(2);
        assert_eq!(
            ring.snapshot(),
            Samples {
                nanos: vec![1, 2],
                ..Default::default()
            }
        );

        ring.push(3);
        assert_eq!(
            ring.snapshot(),
            Samples {
                nanos: vec![2, 3],
                dropped: 1,
                ..Default::default()
            },
            "the oldest sample is dropped and counted"
        );

        ring.push(4);
        ring.push(5);
        assert_eq!(
            ring.snapshot(),
            Samples {
                nanos: vec![4, 5],
                dropped: 3,
                ..Default::default()
            }
        );
    }

    #[test]
    fn zero_second_run_still_has_a_ring() {
        // A zero-second book is **no longer born from the binary** (`main.rs` ties
        // `BT_FRAME_STATS` to a `BT_RUN_SECONDS` greater than zero, otherwise exit 1),
        // but the constructor is `pub` and can be called from the library API: a ring
        // with no capacity cannot take the cursor modulo — not a division by zero but a
        // panic (`slots[i % 0]`). That is why the lower bound stands.
        let stats = Stats::new(Instant::now(), 0);
        stats.record_gpu(1.0, 1.001);
        assert_eq!(stats.gpu().nanos.len(), 1);
    }

    #[test]
    fn startup_stamp_precedes_renderer_setup() {
        // The real ordering is **structural**: the stamp is taken on the first line of
        // `main()` and carried with `Options`, so the `Renderer::system_default()`
        // inside `bt_shell::run` runs after it. This test pins the half the type
        // is responsible for: `Stats` **receives** the stamp, it does not read it
        // itself — had it read it, the most expensive part of startup would have stayed
        // outside the measurement and the number would have looked better than it is.
        let since = Instant::now();
        // "Renderer set-up": had the constructor taken the stamp itself, this time would
        // have stayed outside the measurement.
        std::thread::sleep(Duration::from_millis(2));
        let stats = Stats::new(since, 1);
        assert!(
            stats.startup().is_none(),
            "no startup time before a frame finishes"
        );

        stats.mark_startup();
        let first = stats
            .startup()
            .expect("the first frame closes the startup time");
        assert!(
            first >= Duration::from_millis(2),
            "the startup time must cover the set-up: {first:?}"
        );

        std::thread::sleep(Duration::from_millis(2));
        stats.mark_startup();
        assert_eq!(
            stats.startup(),
            Some(first),
            "the first frame wins, not the later ones"
        );
    }
}
