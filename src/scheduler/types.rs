//! Scheduler queue and admission types.
//!
//! The fair queue is a three-level hierarchy:
//!
//! ```text
//! Priority class
//!   -> workspace lane
//!       -> ordered entries
//! ```
//!
//! Aging elevates an entry's effective priority without mutating the
//! persisted `JobPriority` on the durable record. Entries carry
//! `submitted_at` and an `enqueued_at` so the queue can compute age
//! deterministically.

use std::fmt;

use codegg_core::jobs::{JobId, JobPriority, JobRecord};
use codegg_core::workspace::WorkspaceId;

use crate::scheduler::config::ResolvedSchedulerConfig;
use crate::scheduler::fair_queue::PriorityClass;

/// One entry in the in-memory scheduler queue.
///
/// The queue stores metadata needed for selection; the executor fetches
/// the full `JobRecord` from `JobStore` on dispatch.
#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub job_id: JobId,
    pub workspace_id: WorkspaceId,
    pub priority: JobPriority,
    pub submitted_at: chrono::DateTime<chrono::Utc>,
    pub enqueued_at: chrono::DateTime<chrono::Utc>,
    pub effective_class: PriorityClass,
}

impl QueueEntry {
    pub fn from_job(job: &JobRecord) -> Self {
        let now = chrono::Utc::now();
        Self {
            job_id: job.job_id.clone(),
            workspace_id: job.workspace_id.clone(),
            priority: job.priority,
            submitted_at: job.created_at,
            enqueued_at: now,
            effective_class: PriorityClass::from_priority(job.priority),
        }
    }

    /// Recompute `effective_class` using the configured aging window.
    pub fn recompute_aging(
        &mut self,
        cfg: &ResolvedSchedulerConfig,
        now: chrono::DateTime<chrono::Utc>,
    ) {
        let age_secs = (now - self.submitted_at).num_seconds().max(0) as u64;
        self.effective_class =
            PriorityClass::with_aging(self.priority, age_secs, cfg.fairness.aging_secs);
    }
}

impl fmt::Display for QueueEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "QueueEntry(job={}, ws={}, prio={:?}, eff={:?}, age={}s)",
            self.job_id,
            self.workspace_id,
            self.priority,
            self.effective_class,
            (chrono::Utc::now() - self.submitted_at).num_seconds()
        )
    }
}

/// Identifier for a queue insertion operation. The scheduler accepts
/// this when an external caller wants to push a known `JobId` into the
/// queue (e.g. after a manual wake). Existing entries are silently
/// kept (deduplication by job id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneInsert {
    pub job_id: JobId,
    pub workspace_id: WorkspaceId,
    pub priority: JobPriority,
    pub submitted_at: chrono::DateTime<chrono::Utc>,
}

/// Reason a queue removal was triggered. Recorded for diagnostics; the
/// queue does not currently act on it but the snapshot surfaces
/// counters per reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueueRemovalReason {
    Admitted,
    Cancelled,
    Expired,
    Blocked,
    Dropped,
}

impl QueueRemovalReason {
    pub fn label(&self) -> &'static str {
        match self {
            QueueRemovalReason::Admitted => "admitted",
            QueueRemovalReason::Cancelled => "cancelled",
            QueueRemovalReason::Expired => "expired",
            QueueRemovalReason::Blocked => "blocked",
            QueueRemovalReason::Dropped => "dropped",
        }
    }
}

/// Errors raised by queue insertion when a request is structurally
/// invalid (e.g. zero-value). Bounded queue overflow returns
/// `Overflow` rather than panicking so the scheduler can map it to a
/// typed `AdmissionDecision::TemporarilyBlocked`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueInsertError {
    Overflow,
    Invalid(String),
}

impl fmt::Display for QueueInsertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueueInsertError::Overflow => f.write_str("scheduler queue is at capacity"),
            QueueInsertError::Invalid(msg) => write!(f, "invalid queue insert: {msg}"),
        }
    }
}

impl std::error::Error for QueueInsertError {}

/// Bounded per-class workspace lane. Stored under
/// `PriorityClass -> WorkspaceId -> VecDeque<QueueEntry>`.
pub type LaneEntries = std::collections::HashMap<WorkspaceId, Vec<QueueEntry>>;
