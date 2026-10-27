//! The post-commit publication of a delegated run's transcript advance.
//!
//! A delegated run's recorded transcript is the durable truth. This module
//! publishes the **advisory** that says it grew, and it is deliberately the
//! smallest thing that can carry that fact:
//!
//! - **Body-free.** The advisory carries the run id, the committed
//!   high-water ordinal, and whether the run's own recording stopped. It
//!   carries no text, no tool arguments, no provider reasoning and no
//!   credentials: a client that wants the conversation asks for the
//!   authorized page, and the hint never stands in for it.
//! - **Post-commit.** The capture sink calls the publisher only after the
//!   store's append has returned, so a hint can never claim a row the
//!   store does not hold, and the write transaction is never held across
//!   the publication.
//! - **Coalesced.** A run that appends in a tight loop does not produce one
//!   advisory per row. Appends collapse under a bounded row cap and a
//!   bounded interval, and a run's recording-stopped state publishes
//!   immediately because it is that run's last word.
//! - **Injected, never global.** The delegated child's runtime holds an
//!   `Arc<dyn TranscriptAdvancePublisher>`: there is no process-wide
//!   registry, no TUI dependency, and no direct call into the projection
//!   replay handle. The publisher reaches the daemon's canonical
//!   [`EventLog`], which is the only producer.
//!
//! ## The quiet-run bound
//!
//! Coalescing has a cost the append-time policy cannot pay: a run that
//! commits one to seven rows and then goes quiet — a child blocked inside a
//! tool — has those rows coalesced away with nothing to release them. The
//! advisory would wait for the *next* append, the recording-stopped state
//! or the run's completion, and a follower that already read the run's
//! first high-water has no reason to read again.
//!
//! So the coalescer owns **one** flush worker. It is the publisher's single
//! timer: it waits for the earliest deadline at which a coalesced advance
//! becomes due, publishes what is due, and waits again. A quiet run's
//! committed watermark therefore reaches its subscribers within
//! [`ADVANCE_INTERVAL`] of the run's last published advisory, with no
//! further append and no run completion.
//!
//! The worker is deliberately small and bounded: it is **one** task per
//! publisher, started once and never per append; it holds **no** state of
//! its own, because the coalescing map is the state and the task ends when
//! the publisher is dropped; it never holds the coalescing lock across an
//! `await`; and it publishes only the highest committed ordinal, never one
//! below the watermark already published for that run.
//!
//! `architecture/projection.md` owns the advisory's contract end to end.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use codegg_core::agent_run::AgentRunStore;
use codegg_core::identity::AgentRunId;
use tokio::sync::watch;

use crate::core::event_log::EventLog;
use crate::protocol::core::CoreEvent;

/// The row cap under which one run's appends collapse into one advisory.
///
/// Eight rows is small enough that a client following a live child sees
/// progress promptly and large enough that a chatty child does not produce
/// one event per message.
pub const ADVANCE_ROW_CAP: i64 = 8;

/// The interval under which one run's appends collapse into one advisory.
///
/// This is also the **quiet-run bound**: a committed row that no further
/// append follows is published within one interval of the run's last
/// published advisory, because the coalescer's flush worker publishes it
/// when the interval elapses.
pub const ADVANCE_INTERVAL: Duration = Duration::from_millis(500);

/// The most runs the coalescer tracks at once.
///
/// The tracked map holds one entry per run the daemon has published for. A
/// long-lived daemon would otherwise accumulate an entry per delegated run
/// for as long as it lived, so the map is capped and the oldest tracked run
/// is dropped when the cap is reached. A dropped run's advisory is lost,
/// and a lost advisory is recoverable by a cursor read: the durable store
/// remains the truth.
pub const MAX_TRACKED_RUNS: usize = 256;

/// The body-free advisory that a delegated run's recorded transcript
/// advanced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptAdvance {
    /// The run whose transcript advanced.
    pub run_id: String,
    /// The run's committed high-water ordinal.
    ///
    /// This is the store's own ordinal, so it is monotonic per run even
    /// when two children write concurrently.
    pub ordinal: i64,
    /// The run's recording stopped at the store's per-run bound, so the
    /// recorded transcript is itself incomplete and will not grow.
    pub recording_stopped: bool,
}

/// The constrained, typed handle the delegated child's runtime holds.
///
/// It is injected into the child's runtime; it is not a global singleton,
/// and it exposes no way to publish an arbitrary event.
#[async_trait::async_trait]
// Stable >=1.99 flags the #[must_use] injected by the async-trait expansion
// (the desugared future is already must_use). Suppressed at the macro site;
// no behavior change.
#[allow(clippy::double_must_use)]
pub trait TranscriptAdvancePublisher: Send + Sync {
    /// Note that the run's committed transcript advanced.
    ///
    /// The call never fails the delegated work: the durable store is the
    /// truth, and a lost advisory is recovered by a page read.
    async fn publish_advance(&self, advance: TranscriptAdvance);

    /// Publish whatever is still coalesced for a run.
    ///
    /// Called when the delegated run finishes, so the run's final
    /// watermark is not left waiting for an append that will never come.
    async fn flush(&self, run_id: &str);
}

/// Where a coalesced advisory is published.
#[async_trait::async_trait]
// Stable >=1.99 flags the #[must_use] injected by the async-trait expansion
// (the desugared future is already must_use). Suppressed at the macro site;
// no behavior change.
#[allow(clippy::double_must_use)]
pub trait TranscriptAdvanceSink: Send + Sync {
    /// Publish one advisory.
    async fn publish(&self, advance: &TranscriptAdvance);
}

/// The clock the coalescer reads.
///
/// The production clock is the system clock. Injecting it keeps the
/// *policy* — when a coalesced advance is due — decidable without a wall
/// clock, so the quiet-run bound is asserted rather than measured.
trait AdvanceClock: Send + Sync {
    /// The current instant.
    fn now(&self) -> Instant;
}

/// The system clock.
struct SystemClock;

impl AdvanceClock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// The per-run coalescing state.
#[derive(Debug, Default)]
struct RunState {
    /// The ordinal of the last advisory published for this run.
    last_published_ordinal: Option<i64>,
    /// The highest ordinal seen since the last published advisory.
    ///
    /// `None` means "nothing pending". The distinction from `Some(0)`
    /// matters: a run's first row has ordinal zero, and a sentinel zero
    /// would make a pending first row indistinguishable from an empty
    /// pending set.
    pending_ordinal: Option<i64>,
    /// Whether the run's recording stopped since the last published
    /// advisory.
    pending_recording_stopped: bool,
    /// When the last advisory was published for this run.
    last_published_at: Option<Instant>,
}

impl RunState {
    /// The instant at which this run's coalesced advance becomes due.
    ///
    /// `None` means the run holds nothing pending, so it has no deadline
    /// and the flush worker does not wait for it.
    fn deadline(&self, interval: Duration, now: Instant) -> Option<Instant> {
        self.pending_ordinal?;
        Some(match self.last_published_at {
            Some(at) => at + interval,
            None => now,
        })
    }

    /// Whether this run's coalesced advance is due at `now`.
    fn is_due(&self, now: Instant, interval: Duration) -> bool {
        if self.pending_ordinal.is_none() {
            return false;
        }
        match self.last_published_at {
            Some(at) => now.saturating_duration_since(at) >= interval,
            None => true,
        }
    }

    /// Whether the pending advance carries anything the last published
    /// advisory did not.
    ///
    /// An out-of-order advisory carries nothing new: the canonical storage
    /// ordinal already published for this run is at least as high. Such a
    /// pending advance is dropped rather than published a second time.
    fn has_something_new(&self) -> bool {
        if self.pending_recording_stopped {
            return true;
        }
        match (self.pending_ordinal, self.last_published_ordinal) {
            (Some(pending), Some(published)) => pending > published,
            (Some(_), None) => true,
            _ => false,
        }
    }
}

/// The coalescer's tracked state.
#[derive(Debug, Default)]
struct CoalescerState {
    /// The tracked runs, by run id.
    runs: HashMap<String, RunState>,
    /// The tracked run ids, oldest first.
    order: VecDeque<String>,
}

impl CoalescerState {
    /// The state for `run_id`, tracking the run when it is new.
    ///
    /// Tracking is capped: the oldest tracked run is dropped once the map
    /// holds [`MAX_TRACKED_RUNS`] runs, so the map cannot grow with the
    /// daemon's lifetime.
    fn entry(&mut self, run_id: &str) -> &mut RunState {
        if !self.runs.contains_key(run_id) {
            self.order.push_back(run_id.to_string());
            while self.runs.len() >= MAX_TRACKED_RUNS {
                let Some(oldest) = self.order.pop_front() else {
                    break;
                };
                self.runs.remove(&oldest);
            }
        }
        self.runs.entry(run_id.to_string()).or_default()
    }

    /// The earliest deadline at which a coalesced advance becomes due.
    fn earliest_deadline(&self, interval: Duration, now: Instant) -> Option<Instant> {
        self.runs
            .values()
            .filter_map(|state| state.deadline(interval, now))
            .min()
    }

    /// Take the pending advances that are due at `now`.
    ///
    /// The caller publishes what this returns **outside** the lock: a slow
    /// subscriber must not hold the coalescing state, and the state must
    /// not be held across an await.
    fn take_due(&mut self, now: Instant, interval: Duration) -> Vec<TranscriptAdvance> {
        let mut due = Vec::new();
        for (run_id, state) in self.runs.iter_mut() {
            if !state.is_due(now, interval) {
                continue;
            }
            if !state.has_something_new() {
                // The pending advance carries nothing the last published
                // advisory did not. It is dropped, so the run holds no
                // pending advance and the flush worker stops waiting for it.
                state.pending_ordinal = None;
                state.pending_recording_stopped = false;
                continue;
            }
            let Some(ordinal) = state.pending_ordinal else {
                continue;
            };
            let recording_stopped = state.pending_recording_stopped;
            state.pending_ordinal = None;
            state.pending_recording_stopped = false;
            state.last_published_ordinal = Some(ordinal);
            state.last_published_at = Some(now);
            due.push(TranscriptAdvance {
                run_id: run_id.clone(),
                ordinal,
                recording_stopped,
            });
        }
        due
    }
}

/// Whether the coalesced state should publish now.
///
/// The rule is deliberately small and deterministic:
///
/// - the **first** advisory for a run publishes immediately, so an
///   in-flight entry is signalled promptly;
/// - an advisory publishes once the run has committed
///   [`ADVANCE_ROW_CAP`] rows beyond the last published one;
/// - an advisory publishes once [`ADVANCE_INTERVAL`] has elapsed since
///   the last one for that run;
/// - a **recording-stopped** advisory publishes immediately: the run will
///   never produce another one, so holding it back would lose it.
///
/// The function is pure so the policy can be tested without a clock.
fn should_publish(
    state: &RunState,
    advance: &TranscriptAdvance,
    now: Instant,
    cap: i64,
    interval: Duration,
) -> bool {
    if advance.recording_stopped {
        return true;
    }
    let Some(last_ordinal) = state.last_published_ordinal else {
        return true;
    };
    if advance.ordinal - last_ordinal >= cap {
        return true;
    }
    state.is_due(now, interval)
}

/// The publisher the delegated child's runtime holds.
///
/// It collapses a run's appends into a bounded number of advisories and
/// forwards them to the injected [`TranscriptAdvanceSink`].
pub struct CoalescingTranscriptPublisher {
    sink: Arc<dyn TranscriptAdvanceSink>,
    cap: i64,
    interval: Duration,
    clock: Arc<dyn AdvanceClock>,
    state: Arc<Mutex<CoalescerState>>,
    /// Signals the flush worker that the tracked state changed.
    wake: watch::Sender<u64>,
    /// Whether the flush worker has been started.
    worker_started: AtomicBool,
    /// Whether the flush worker is still running.
    worker_alive: Arc<AtomicBool>,
}

impl CoalescingTranscriptPublisher {
    /// Build a publisher with the default bounds.
    pub fn new(sink: Arc<dyn TranscriptAdvanceSink>) -> Self {
        Self::with_bounds(sink, ADVANCE_ROW_CAP, ADVANCE_INTERVAL)
    }

    /// Build a publisher with explicit bounds.
    pub fn with_bounds(sink: Arc<dyn TranscriptAdvanceSink>, cap: i64, interval: Duration) -> Self {
        Self::with_clock(sink, cap, interval, Arc::new(SystemClock))
    }

    /// Build a publisher with explicit bounds and an explicit clock.
    fn with_clock(
        sink: Arc<dyn TranscriptAdvanceSink>,
        cap: i64,
        interval: Duration,
        clock: Arc<dyn AdvanceClock>,
    ) -> Self {
        let (wake, _rx) = watch::channel(0u64);
        Self {
            sink,
            cap,
            interval,
            clock,
            state: Arc::new(Mutex::new(CoalescerState::default())),
            wake,
            worker_started: AtomicBool::new(false),
            worker_alive: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the flush worker, once.
    ///
    /// The worker is the publisher's **single** timer. It is started when
    /// a run first holds a coalesced advance, never per append, and it
    /// ends when the publisher is dropped: the wake channel closes with
    /// the publisher, and the worker observes that and stops.
    fn ensure_worker(&self) {
        if self.worker_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            // There is no runtime to schedule the wake on. The flag is
            // released so a later append can try again.
            self.worker_started.store(false, Ordering::SeqCst);
            return;
        };
        let wake = self.wake.subscribe();
        let state = self.state.clone();
        let clock = self.clock.clone();
        let sink = self.sink.clone();
        let interval = self.interval;
        let alive = self.worker_alive.clone();
        self.worker_alive.store(true, Ordering::SeqCst);
        handle.spawn(async move {
            flush_worker(state, clock, sink, interval, wake, alive).await;
        });
    }

    /// Signal the flush worker that the tracked state changed.
    ///
    /// The counter is what the worker watches. A worker that is gone
    /// ignores the signal, which is the same thing as having nothing to
    /// wake.
    fn signal_wake(&self) {
        self.wake
            .send_modify(|counter| *counter = counter.wrapping_add(1));
    }

    /// Whether a run still holds a coalesced advance.
    fn has_pending(&self, run_id: &str) -> bool {
        let guard = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .runs
            .get(run_id)
            .is_some_and(|state| state.pending_ordinal.is_some())
    }
}

/// The publisher's single flush worker.
///
/// It waits for the earliest deadline at which a coalesced advance becomes
/// due, publishes what is due, and waits again. It holds no state of its
/// own, and it ends when the publisher is dropped.
async fn flush_worker(
    state: Arc<Mutex<CoalescerState>>,
    clock: Arc<dyn AdvanceClock>,
    sink: Arc<dyn TranscriptAdvanceSink>,
    interval: Duration,
    mut wake: watch::Receiver<u64>,
    alive: Arc<AtomicBool>,
) {
    loop {
        // The next deadline at which a coalesced advance becomes due.
        let deadline = {
            let guard = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.earliest_deadline(interval, clock.now())
        };
        let woke = match deadline {
            Some(at) => {
                let wait = at.saturating_duration_since(clock.now());
                tokio::select! {
                    _ = tokio::time::sleep(wait) => true,
                    changed = wake.changed() => changed.is_ok(),
                }
            }
            None => wake.changed().await.is_ok(),
        };
        if !woke {
            // The publisher is gone: the daemon shut down, or the pool
            // dropped it. The worker ends here rather than outliving the
            // publisher that owns it.
            break;
        }
        // Publish what the clock says is due. The coalescing lock is not
        // held across the publication.
        let due = {
            let mut guard = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.take_due(clock.now(), interval)
        };
        for advance in due {
            sink.publish(&advance).await;
        }
    }
    alive.store(false, Ordering::SeqCst);
}

#[async_trait::async_trait]
impl TranscriptAdvancePublisher for CoalescingTranscriptPublisher {
    async fn publish_advance(&self, advance: TranscriptAdvance) {
        let run_id = advance.run_id.clone();
        let publish = {
            let mut guard = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let state = guard.entry(&run_id);
            // The canonical storage ordinal dominates: the merged
            // watermark is never below the one already published for this
            // run, so an out-of-order advisory cannot move it backwards —
            // not when it is published, and not when it is flushed later.
            let floor = state.last_published_ordinal.unwrap_or(advance.ordinal);
            state.pending_ordinal = Some(
                state
                    .pending_ordinal
                    .unwrap_or(advance.ordinal)
                    .max(advance.ordinal)
                    .max(floor),
            );
            state.pending_recording_stopped |= advance.recording_stopped;
            if !should_publish(state, &advance, self.clock.now(), self.cap, self.interval) {
                None
            } else {
                let ordinal = state.pending_ordinal.unwrap_or(advance.ordinal);
                let recording_stopped = state.pending_recording_stopped;
                state.pending_ordinal = None;
                state.pending_recording_stopped = false;
                state.last_published_ordinal = Some(ordinal);
                state.last_published_at = Some(self.clock.now());
                Some(TranscriptAdvance {
                    run_id: run_id.clone(),
                    ordinal,
                    recording_stopped,
                })
            }
        };
        // The publication happens outside the lock: a slow subscriber must
        // not hold the coalescing state, and the state must not be held
        // across an await.
        if let Some(advance) = publish {
            self.sink.publish(&advance).await;
        }
        if self.has_pending(&run_id) {
            // The run holds a coalesced advance. The flush worker is armed
            // so a quiet run's watermark is published within the interval,
            // without waiting for another append.
            self.ensure_worker();
            self.signal_wake();
        }
    }

    async fn flush(&self, run_id: &str) {
        let publish = {
            let mut guard = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            // The run is over, so its tracked state is dropped: a
            // long-lived daemon must not accumulate an entry per
            // completed run.
            let Some(state) = guard.runs.remove(run_id) else {
                return;
            };
            guard.order.retain(|tracked| tracked != run_id);
            if !state.has_something_new() {
                return;
            }
            let Some(ordinal) = state.pending_ordinal else {
                return;
            };
            Some(TranscriptAdvance {
                run_id: run_id.to_string(),
                ordinal,
                recording_stopped: state.pending_recording_stopped,
            })
        };
        if let Some(advance) = publish {
            self.sink.publish(&advance).await;
        }
    }
}

/// The daemon-side sink.
///
/// It resolves the run's **owning session** from the durable run/task
/// origin and publishes the body-free advisory through the daemon's
/// canonical [`EventLog`]. The EventLog is the only producer: nothing here
/// touches the projection replay handle directly.
pub struct EventLogTranscriptAdvanceSink {
    event_log: Arc<EventLog>,
    runs: Arc<dyn AgentRunStore>,
}

impl EventLogTranscriptAdvanceSink {
    /// Bind the sink to a daemon's event log and durable run store.
    pub fn new(event_log: Arc<EventLog>, runs: Arc<dyn AgentRunStore>) -> Self {
        Self { event_log, runs }
    }

    /// The session that owns a run, from the run's durable task origin.
    ///
    /// This is the same authority the page read uses: the run's task
    /// records the session that originated it. A run that cannot be
    /// attributed has no session to publish under, so no advisory is
    /// published for it.
    async fn owning_session(&self, run_id: &AgentRunId) -> Option<String> {
        let run = self.runs.get_run(run_id).await.ok()??;
        let task = self.runs.get_task(&run.task_id).await.ok()??;
        if task.originating_session_id.is_empty() {
            return None;
        }
        Some(task.originating_session_id)
    }
}

#[async_trait::async_trait]
impl TranscriptAdvanceSink for EventLogTranscriptAdvanceSink {
    async fn publish(&self, advance: &TranscriptAdvance) {
        let Ok(run_id) = AgentRunId::parse(&advance.run_id) else {
            tracing::warn!(
                run_id = %advance.run_id,
                "a transcript advance named a run id that is not a run identity; the advisory is dropped"
            );
            return;
        };
        let Some(session_id) = self.owning_session(&run_id).await else {
            tracing::warn!(
                %run_id,
                "a transcript advance named a run with no resolvable owning session; the advisory is dropped"
            );
            return;
        };
        let event = CoreEvent::AgentRunTranscriptAdvanced {
            session_id: session_id.clone(),
            run_id: advance.run_id.clone(),
            ordinal: advance.ordinal,
            recording_stopped: advance.recording_stopped,
        };
        self.event_log.publish(Some(session_id), None, event).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::Mutex as AsyncMutex;

    /// A sink that records what it was asked to publish.
    #[derive(Default)]
    struct RecordingSink {
        published: AsyncMutex<Vec<TranscriptAdvance>>,
        count: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl TranscriptAdvanceSink for RecordingSink {
        async fn publish(&self, advance: &TranscriptAdvance) {
            self.count.fetch_add(1, Ordering::SeqCst);
            self.published.lock().await.push(advance.clone());
        }
    }

    /// A clock the test moves by hand.
    ///
    /// The quiet-run bound is a policy about *elapsed* time, so a test
    /// that waits on a wall clock would be measuring the machine rather
    /// than the rule. This clock makes "the interval elapsed" an action
    /// the test takes.
    #[derive(Debug)]
    struct ManualClock {
        now: Mutex<Instant>,
    }

    impl ManualClock {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                now: Mutex::new(Instant::now()),
            })
        }

        /// Move the clock forward.
        fn advance(&self, by: Duration) {
            let mut now = self
                .now
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *now += by;
        }
    }

    impl AdvanceClock for ManualClock {
        fn now(&self) -> Instant {
            *self
                .now
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        }
    }

    fn advance(run_id: &str, ordinal: i64) -> TranscriptAdvance {
        TranscriptAdvance {
            run_id: run_id.to_string(),
            ordinal,
            recording_stopped: false,
        }
    }

    #[tokio::test]
    async fn a_burst_of_appends_collapses_into_one_advisory() {
        let sink = Arc::new(RecordingSink::default());
        let publisher =
            CoalescingTranscriptPublisher::with_bounds(sink.clone(), 8, Duration::from_secs(3600));
        for ordinal in 0..4 {
            publisher.publish_advance(advance("run-1", ordinal)).await;
        }
        let published = sink.published.lock().await;
        assert_eq!(
            published.len(),
            1,
            "a burst under the row cap must collapse into one advisory, got {published:?}"
        );
        assert_eq!(
            published[0].ordinal, 0,
            "the first advisory carries the first ordinal"
        );
    }

    #[tokio::test]
    async fn the_row_cap_releases_a_coalesced_burst() {
        let sink = Arc::new(RecordingSink::default());
        let publisher =
            CoalescingTranscriptPublisher::with_bounds(sink.clone(), 8, Duration::from_secs(3600));
        for ordinal in 0..9 {
            publisher.publish_advance(advance("run-1", ordinal)).await;
        }
        let published = sink.published.lock().await;
        assert_eq!(
            published.len(),
            2,
            "the ninth row must release the coalesced advisory, got {published:?}"
        );
        assert_eq!(
            published[1].ordinal, 8,
            "the released advisory carries the highest committed ordinal"
        );
    }

    #[tokio::test]
    async fn a_recording_stopped_advance_publishes_immediately() {
        let sink = Arc::new(RecordingSink::default());
        let publisher =
            CoalescingTranscriptPublisher::with_bounds(sink.clone(), 8, Duration::from_secs(3600));
        publisher.publish_advance(advance("run-1", 0)).await;
        publisher
            .publish_advance(TranscriptAdvance {
                run_id: "run-1".into(),
                ordinal: 1,
                recording_stopped: true,
            })
            .await;
        let published = sink.published.lock().await;
        assert_eq!(
            published.len(),
            2,
            "the run's last advisory must not be held back, got {published:?}"
        );
        assert!(published[1].recording_stopped);
    }

    #[tokio::test]
    async fn two_runs_do_not_coalesce_into_one_advisory() {
        let sink = Arc::new(RecordingSink::default());
        let publisher =
            CoalescingTranscriptPublisher::with_bounds(sink.clone(), 8, Duration::from_secs(3600));
        publisher.publish_advance(advance("run-1", 0)).await;
        publisher.publish_advance(advance("run-2", 0)).await;
        let published = sink.published.lock().await;
        assert_eq!(
            published.len(),
            2,
            "two runs are two watermarks, got {published:?}"
        );
    }

    #[tokio::test]
    async fn an_out_of_order_advance_never_moves_the_watermark_backwards() {
        let sink = Arc::new(RecordingSink::default());
        let publisher =
            CoalescingTranscriptPublisher::with_bounds(sink.clone(), 1, Duration::from_secs(3600));
        publisher.publish_advance(advance("run-1", 5)).await;
        publisher.publish_advance(advance("run-1", 2)).await;
        {
            let published = sink.published.lock().await;
            assert_eq!(
                published.len(),
                1,
                "an out-of-order advisory carries no new information, so it is not published, got {published:?}"
            );
            assert_eq!(
                published[0].ordinal, 5,
                "the canonical storage ordinal dominates an out-of-order advisory"
            );
        }

        // And the flush must not move it backwards either. The sink's lock
        // is released first: the flush publishes through the same sink.
        publisher.flush("run-1").await;
        let published = sink.published.lock().await;
        assert!(
            published.iter().all(|adv| adv.ordinal >= 5),
            "no advisory may carry an ordinal below the one already published, got {published:?}"
        );
    }

    #[tokio::test]
    async fn a_flush_publishes_what_is_still_coalesced() {
        let sink = Arc::new(RecordingSink::default());
        let publisher =
            CoalescingTranscriptPublisher::with_bounds(sink.clone(), 8, Duration::from_secs(3600));
        publisher.publish_advance(advance("run-1", 0)).await;
        publisher.publish_advance(advance("run-1", 1)).await;
        publisher.flush("run-1").await;
        let published = sink.published.lock().await;
        assert_eq!(
            published.len(),
            2,
            "the flush must release the coalesced ordinal, got {published:?}"
        );
        assert_eq!(published[1].ordinal, 1);
    }

    #[test]
    fn the_elapsed_interval_releases_a_coalesced_advance() {
        let state = RunState {
            last_published_ordinal: Some(0),
            pending_ordinal: Some(1),
            pending_recording_stopped: false,
            last_published_at: Some(Instant::now()),
        };
        let now = Instant::now();
        assert!(
            !should_publish(
                &state,
                &advance("run-1", 1),
                now,
                8,
                Duration::from_secs(3600)
            ),
            "an advance inside the interval must stay coalesced"
        );
        let later = state.last_published_at.expect("a published time") + Duration::from_secs(1);
        assert!(
            should_publish(
                &state,
                &advance("run-1", 1),
                later,
                8,
                Duration::from_secs(1)
            ),
            "an advance past the interval must be released"
        );
    }

    /// C001 L2: a run that commits rows and then goes quiet still has its
    /// watermark published.
    ///
    /// The child here is the quiet one: no further append, no
    /// recording-stopped state, and no completion. Before the flush worker
    /// existed, the coalesced rows were held until one of those three
    /// things happened, so a follower that had already read the run's
    /// first high-water never learned that the run had grown.
    ///
    /// The bound is the publisher's own interval. The test uses a short
    /// one so the wait is short, and a generous deadline so a loaded
    /// machine cannot turn the bound into a flake: the assertion is about
    /// *what* is published, not about how fast.
    #[tokio::test]
    async fn a_quiet_run_publishes_its_coalesced_advance_without_a_further_append() {
        const QUIET_INTERVAL: Duration = Duration::from_millis(25);
        let sink = Arc::new(RecordingSink::default());
        let publisher = CoalescingTranscriptPublisher::with_bounds(sink.clone(), 8, QUIET_INTERVAL);
        publisher.publish_advance(advance("run-1", 0)).await;
        publisher.publish_advance(advance("run-1", 1)).await;
        publisher.publish_advance(advance("run-1", 2)).await;
        {
            let published = sink.published.lock().await;
            assert_eq!(
                published.len(),
                1,
                "the burst must coalesce into the first advisory, got {published:?}"
            );
        }

        // The run goes quiet. The interval elapses, and the flush worker
        // is what releases the coalesced rows.
        let published = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let published = sink.published.lock().await;
                if published.len() >= 2 {
                    break published.clone();
                }
                drop(published);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the quiet run's coalesced advance must be published within the bound");
        assert_eq!(
            published[1].ordinal, 2,
            "the released advisory carries the highest committed ordinal, got {published:?}"
        );
    }

    /// The bound is a rule about elapsed time, not a race with the wall
    /// clock: an advance that has not waited out the interval stays
    /// coalesced.
    #[tokio::test]
    async fn a_coalesced_advance_short_of_the_interval_stays_coalesced() {
        let sink = Arc::new(RecordingSink::default());
        let clock = ManualClock::new();
        let publisher = CoalescingTranscriptPublisher::with_clock(
            sink.clone(),
            8,
            ADVANCE_INTERVAL,
            clock.clone(),
        );
        publisher.publish_advance(advance("run-1", 0)).await;
        publisher.publish_advance(advance("run-1", 1)).await;

        // Short of the interval: nothing is due, so the worker publishes
        // nothing.
        clock.advance(ADVANCE_INTERVAL - Duration::from_millis(1));
        publisher.signal_wake();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            sink.published.lock().await.len(),
            1,
            "an advance short of the interval must stay coalesced"
        );
    }

    /// A dropped publisher takes its flush worker with it: the worker does
    /// not outlive the publisher that owns the timer.
    #[tokio::test]
    async fn dropping_the_publisher_ends_its_flush_worker() {
        let sink = Arc::new(RecordingSink::default());
        let clock = ManualClock::new();
        let publisher = CoalescingTranscriptPublisher::with_clock(
            sink.clone(),
            8,
            ADVANCE_INTERVAL,
            clock.clone(),
        );
        publisher.publish_advance(advance("run-1", 0)).await;
        publisher.publish_advance(advance("run-1", 1)).await;
        let alive = publisher.worker_alive.clone();
        assert!(
            alive.load(Ordering::SeqCst),
            "the flush worker must be running while the publisher holds a pending advance"
        );

        drop(publisher);
        tokio::time::timeout(Duration::from_secs(5), async {
            while alive.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the flush worker must end when the publisher is dropped");
    }

    /// Sixteen concurrent runs stay bounded: the tracked map is capped, and
    /// the published advisories collapse per run rather than per append.
    #[tokio::test]
    async fn sixteen_concurrent_runs_stay_within_the_publication_bounds() {
        let sink = Arc::new(RecordingSink::default());
        let publisher =
            CoalescingTranscriptPublisher::with_bounds(sink.clone(), 8, Duration::from_secs(3600));
        for run in 0..16 {
            for ordinal in 0..4 {
                publisher
                    .publish_advance(advance(&format!("run-{run}"), ordinal))
                    .await;
            }
        }
        {
            let published = sink.published.lock().await;
            assert_eq!(
                published.len(),
                16,
                "each of the sixteen runs publishes its first advisory once, got {published:?}"
            );
        }
        let tracked = publisher
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .runs
            .len();
        assert_eq!(
            tracked, 16,
            "the coalescer tracks one entry per live run, got {tracked}"
        );
    }

    /// A completed run's entry is dropped, so a long-lived daemon does not
    /// accumulate one entry per delegated run it ever published for.
    #[tokio::test]
    async fn a_finished_run_is_forgotten() {
        let sink = Arc::new(RecordingSink::default());
        let publisher =
            CoalescingTranscriptPublisher::with_bounds(sink.clone(), 8, Duration::from_secs(3600));
        publisher.publish_advance(advance("run-1", 0)).await;
        publisher.flush("run-1").await;
        let tracked = publisher
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .runs
            .len();
        assert_eq!(
            tracked, 0,
            "a finished run must not stay in the coalescer's tracked map"
        );
    }

    /// C001-D: the advisory crosses the daemon's canonical event log, from a
    /// **real** durable run.
    ///
    /// The run is a real row in a real SQLite store, the sink resolves the
    /// run's own owning session from the durable task origin, and the
    /// advisory arrives on the event log as a body-free event. The
    /// coalesced advance follows within the interval, with no further
    /// append and no completion — the quiet run's bound, end to end.
    #[tokio::test]
    async fn the_advisory_crosses_the_event_log_from_a_real_durable_run() {
        use codegg_core::agent_run::{
            AgentRunBudget, AgentRunStore, NewAgentRun, NewAgentTask, SqliteAgentRunStore,
        };
        use codegg_core::identity::{AgentRunId, AgentTaskId, ProjectId};
        use codegg_core::workspace::WorkspaceId;

        const SESSION: &str = "session-quiet";

        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        codegg_core::session::schema::migrate(&pool)
            .await
            .expect("migrate the test pool");

        let run_id = AgentRunId::new();
        let task = NewAgentTask {
            task_id: AgentTaskId::new(),
            parent_task_id: None,
            originating_session_id: SESSION.to_string(),
            originating_turn_id: Some("turn-quiet".to_string()),
            project_id: ProjectId::new(),
            repository_id: None,
            workspace_id: WorkspaceId::new(),
            requested_agent: "explore".into(),
            delegation_key: format!("delegation-{run_id}"),
            request_fingerprint: format!("fingerprint-{run_id}"),
            description: "a delegated run".into(),
        };
        let run = NewAgentRun {
            run_id: run_id.clone(),
            parent_run_id: None,
            depth: 1,
            workspace_id: WorkspaceId::new(),
            agent_name: "explore".into(),
            agent_digest: None,
            provider: "test".into(),
            model: "test/model".into(),
            authority_digest: "authority".into(),
            budget: AgentRunBudget::default(),
            shared_scratch: Vec::new(),
        };
        let runs: Arc<dyn AgentRunStore> = Arc::new(SqliteAgentRunStore::new(pool.clone()));
        runs.create_or_get(task, run)
            .await
            .expect("start the delegated run");

        let event_log = Arc::new(crate::core::event_log::EventLog::new(16));
        let mut events = event_log.subscribe();
        let sink = Arc::new(EventLogTranscriptAdvanceSink::new(
            event_log.clone(),
            runs.clone(),
        ));
        let publisher =
            CoalescingTranscriptPublisher::with_bounds(sink, 8, Duration::from_millis(25));
        publisher.publish_advance(advance(run_id.as_ref(), 0)).await;
        publisher.publish_advance(advance(run_id.as_ref(), 3)).await;

        // The first advisory names the run's own owning session.
        let envelope = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("the advisory must reach the event log")
            .expect("the event log must stay open");
        match envelope.payload {
            CoreEvent::AgentRunTranscriptAdvanced {
                session_id,
                run_id: advised_run,
                ordinal,
                recording_stopped,
            } => {
                assert_eq!(
                    session_id, SESSION,
                    "the advisory must name the run's own owning session"
                );
                assert_eq!(
                    advised_run,
                    run_id.as_ref(),
                    "the advisory must name its run"
                );
                assert_eq!(ordinal, 0, "the first advisory carries the first ordinal");
                assert!(!recording_stopped, "the run is still recording");
            }
            other => panic!("the advisory must be the transcript-advanced event, got {other:?}"),
        }

        // The coalesced advance follows within the interval, with no
        // further append and no completion.
        let envelope = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("the quiet run's coalesced advance must reach the event log")
            .expect("the event log must stay open");
        match envelope.payload {
            CoreEvent::AgentRunTranscriptAdvanced { ordinal, .. } => {
                assert_eq!(
                    ordinal, 3,
                    "the released advisory carries the highest committed ordinal"
                );
            }
            other => panic!("the advisory must be the transcript-advanced event, got {other:?}"),
        }
    }
}
