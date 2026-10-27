//! The post-commit publication of a delegated run's transcript advance.
//!
//! A delegated run's recorded transcript is the durable truth. This module
//! publishes the **advisory** that says it grew, and it is deliberately the
//! smallest thing that can carry that fact:
//!
//! - **Body-free.** The advisory carries the run id, the committed
//!   high-water ordinal, and whether the run's own recording stopped. It
//!   carries no text, no tool arguments, no provider reasoning, and no
//!   credentials. A client that wants the conversation asks for the
//!   authorized page; the hint never stands in for it.
//! - **Post-commit.** The capture sink calls the publisher only after the
//!   store's append has returned, so a hint can never claim a row the
//!   store does not hold. The transcript's write transaction is never held
//!   across the publication.
//! - **Coalesced.** A run that appends in a tight loop does not produce one
//!   advisory per row. Appends collapse under a bounded row cap and a
//!   bounded interval, and a run's recording-stopped state publishes
//!   immediately because it is that run's last word.
//! - **Injected, never global.** The delegated child's runtime holds an
//!   `Arc<dyn TranscriptAdvancePublisher>`. There is no process-wide
//!   registry, no TUI dependency, and no direct call into the projection
//!   replay handle: the publisher reaches the daemon's canonical
//!   [`EventLog`], which is the only producer.
//!
//! ## Monotonicity
//!
//! The watermark is **per run**, not per child task: a continuation that
//! appends to the same run id advances the same watermark rather than
//! starting a new one. The canonical storage ordinal dominates, so a
//! duplicate or out-of-order advisory never moves the watermark backwards.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use codegg_core::agent_run::AgentRunStore;
use codegg_core::identity::AgentRunId;

use crate::core::event_log::EventLog;
use crate::protocol::core::CoreEvent;

/// The row cap under which one run's appends collapse into one advisory.
///
/// Eight rows is small enough that a client following a live child sees
/// progress promptly and large enough that a chatty child does not produce
/// one event per message.
pub const ADVANCE_ROW_CAP: i64 = 8;

/// The interval under which one run's appends collapse into one advisory.
pub const ADVANCE_INTERVAL: Duration = Duration::from_millis(500);

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
    match state.last_published_at {
        Some(at) => now.duration_since(at) >= interval,
        None => true,
    }
}

/// The publisher the delegated child's runtime holds.
///
/// It collapses a run's appends into a bounded number of advisories and
/// forwards them to the injected [`TranscriptAdvanceSink`].
pub struct CoalescingTranscriptPublisher {
    sink: Arc<dyn TranscriptAdvanceSink>,
    cap: i64,
    interval: Duration,
    state: Mutex<HashMap<String, RunState>>,
}

impl CoalescingTranscriptPublisher {
    /// Build a publisher with the default bounds.
    pub fn new(sink: Arc<dyn TranscriptAdvanceSink>) -> Self {
        Self::with_bounds(sink, ADVANCE_ROW_CAP, ADVANCE_INTERVAL)
    }

    /// Build a publisher with explicit bounds.
    pub fn with_bounds(sink: Arc<dyn TranscriptAdvanceSink>, cap: i64, interval: Duration) -> Self {
        Self {
            sink,
            cap,
            interval,
            state: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl TranscriptAdvancePublisher for CoalescingTranscriptPublisher {
    async fn publish_advance(&self, advance: TranscriptAdvance) {
        let publish = {
            let mut states = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let state = states.entry(advance.run_id.clone()).or_default();
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
            if !should_publish(state, &advance, Instant::now(), self.cap, self.interval) {
                None
            } else {
                let ordinal = state.pending_ordinal.unwrap_or(advance.ordinal);
                let recording_stopped = state.pending_recording_stopped;
                state.pending_ordinal = None;
                state.pending_recording_stopped = false;
                state.last_published_ordinal = Some(ordinal);
                state.last_published_at = Some(Instant::now());
                Some(TranscriptAdvance {
                    run_id: advance.run_id.clone(),
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
    }

    async fn flush(&self, run_id: &str) {
        let publish = {
            let mut states = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(state) = states.get_mut(run_id) else {
                return;
            };
            let Some(ordinal) = state.pending_ordinal else {
                // Nothing is pending for this run. The recording-stopped
                // flag is only ever set alongside a pending ordinal, so
                // there is nothing to publish here.
                return;
            };
            let recording_stopped = state.pending_recording_stopped;
            state.pending_ordinal = None;
            state.pending_recording_stopped = false;
            state.last_published_ordinal = Some(ordinal);
            state.last_published_at = Some(Instant::now());
            Some(TranscriptAdvance {
                run_id: run_id.to_string(),
                ordinal,
                recording_stopped,
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
    use std::sync::atomic::{AtomicUsize, Ordering};
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
}
