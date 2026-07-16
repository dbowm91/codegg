//! Scheduler events.
//!
//! Phase 5 emits bounded-delta events for state changes that
//! downstream consumers (TUI, server, audit) need to surface.
//! Full queue snapshots are NOT emitted on the event stream; clients
//! request them via `SnapshotScheduler` or
//! `SnapshotWorkspace/Scheduler`.

use serde::{Deserialize, Serialize};

use codegg_core::jobs::AttemptId;
use codegg_core::run_store::RunId;

use crate::scheduler::admission::BlockReason;
use crate::scheduler::executor::ExecutorKind;

/// A scheduler-related event. Constructed by the scheduler and
/// surfaced via the daemon's event bus. The variants are kept
/// narrow and bounded; a `SchedulerSnapshot` is the only way to get
/// full state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SchedulerEvent {
    /// A job was rejected by the admission controller.
    AdmissionBlocked { job_id: String, reason: BlockReason },
    /// A job was admitted and an attempt is being dispatched.
    JobAdmitted {
        job_id: String,
        attempt_id: AttemptId,
        run_id: Option<RunId>,
    },
    /// Resource permits released.
    JobResourceReleased {
        job_id: String,
        attempt_id: AttemptId,
    },
    /// The queue is at capacity and at least one submission was
    /// rejected.
    SchedulerOverloaded { queued: usize, cap: usize },
    /// The queue grew or shrank by a meaningful delta. The
    /// threshold is set by the scheduler main loop.
    SchedulerQueueChanged {
        ready_window: usize,
        durable_queued: usize,
    },
    /// An executor reported Unavailable or Degraded.
    ExecutorUnavailable {
        executor: ExecutorKind,
        reason: String,
    },
    /// A scheduled sweep of the in-memory queue completed.
    SchedulerQueueReconciled {
        durable_queued: usize,
        ready_window: usize,
    },
    /// A wake arrived (job enqueued, executor finished, schedule
    /// tick). Internal: emitted only in debug builds via
    /// `tracing::debug!`. The struct variant exists for symmetry and
    /// future forwarding.
    SchedulerWoke { reason: WokeReason },
    /// Executor-reported progress for a running job. Routed through
    /// the configured event sink.
    Progress { job_id: String, message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WokeReason {
    JobEnqueued,
    ExecutorCompleted,
    CancellationRequested,
    ScheduleTick,
    ScheduleClaimed,
    Manual,
    RetryRequested,
}

impl WokeReason {
    pub fn label(&self) -> &'static str {
        match self {
            WokeReason::JobEnqueued => "job_enqueued",
            WokeReason::ExecutorCompleted => "executor_completed",
            WokeReason::CancellationRequested => "cancellation_requested",
            WokeReason::ScheduleTick => "schedule_tick",
            WokeReason::ScheduleClaimed => "schedule_claimed",
            WokeReason::Manual => "manual",
            WokeReason::RetryRequested => "retry_requested",
        }
    }
}
