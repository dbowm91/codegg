//! Phase 4 durable schedule model.
//!
//! Schedules are typed records that describe recurring or one-shot
//! work. A firing schedule produces at most one job per scheduled
//! occurrence, identified by the `(schedule_id, scheduled_for)` pair.
//! The persistence layer enforces this uniqueness so duplicate ticks
//! after a restart cannot enqueue the same occurrence twice.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::error::StorageError;
use crate::jobs::{JobId, JobPayload, JobPriority, JobSource, ResourceRequest, RetryPolicy};
use crate::workspace::WorkspaceId;

use super::{IdempotencyClass, JobKind};

/// Schedule lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleState {
    Active,
    Paused,
    Completed,
    Archived,
}

impl ScheduleState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScheduleState::Active => "active",
            ScheduleState::Paused => "paused",
            ScheduleState::Completed => "completed",
            ScheduleState::Archived => "archived",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "paused" => ScheduleState::Paused,
            "completed" => ScheduleState::Completed,
            "archived" => ScheduleState::Archived,
            _ => ScheduleState::Active,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, ScheduleState::Completed | ScheduleState::Archived)
    }
}

/// Schedule kind. Calendar/cron syntax is deferred to a later phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ScheduleKind {
    OneShot {
        run_at: DateTime<Utc>,
    },
    Interval {
        every: Duration,
        anchor: DateTime<Utc>,
    },
}

impl ScheduleKind {
    pub fn tag(&self) -> &'static str {
        match self {
            ScheduleKind::OneShot { .. } => "one_shot",
            ScheduleKind::Interval { .. } => "interval",
        }
    }
}

/// Overlap policy applied when a firing occurs while a previous
/// occurrence is still queued or running.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlapPolicy {
    #[default]
    SkipIfRunning,
    QueueOne,
    Allow,
}

/// Policy applied when the daemon was offline through one or more
/// scheduled occurrences.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissedRunPolicy {
    Skip,
    #[default]
    RunOnceNow,
    CatchUpBounded {
        max_occurrences: u32,
    },
}

/// Template used by a schedule to materialize jobs at firing time.
/// Stores enough data to rerun safely without consulting stale client
/// state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobTemplate {
    pub kind: JobKind,
    pub priority: JobPriority,
    pub payload: JobPayload,
    pub resource_request: ResourceRequest,
    pub retry_policy: RetryPolicy,
    pub idempotency: IdempotencyClass,
    pub timeout: Option<Duration>,
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

/// Persisted schedule record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleRecord {
    pub schedule_id: crate::jobs::ScheduleId,
    pub workspace_id: WorkspaceId,
    pub session_id: Option<String>,
    pub kind: ScheduleKind,
    pub job_template: JobTemplate,
    pub state: ScheduleState,
    pub overlap_policy: OverlapPolicy,
    pub missed_run_policy: MissedRunPolicy,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_occurrence_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

/// Compact summary for list responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleSummary {
    pub schedule_id: String,
    pub workspace_id: String,
    pub kind: ScheduleKind,
    pub state: ScheduleState,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_occurrence_at: Option<DateTime<Utc>>,
}

/// Parameters for a new schedule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleTemplate {
    pub workspace_id: WorkspaceId,
    pub session_id: Option<String>,
    pub kind: ScheduleKind,
    pub job_template: JobTemplate,
    pub overlap_policy: OverlapPolicy,
    pub missed_run_policy: MissedRunPolicy,
    pub next_run_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

/// Query parameters for [`ScheduleStore::list`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScheduleQuery {
    pub workspace_id: Option<WorkspaceId>,
    pub state: Option<ScheduleState>,
    pub include_archived: bool,
}

/// Status of an occurrence against a schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceStatus {
    /// The occurrence is recorded but no job has been created yet.
    Pending,
    /// A job was created from this occurrence.
    Queued,
    /// The occurrence was skipped due to overlap policy.
    Skipped,
    /// The occurrence was suppressed due to missed-run policy.
    Suppressed,
}

impl OccurrenceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            OccurrenceStatus::Pending => "pending",
            OccurrenceStatus::Queued => "queued",
            OccurrenceStatus::Skipped => "skipped",
            OccurrenceStatus::Suppressed => "suppressed",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "queued" => OccurrenceStatus::Queued,
            "skipped" => OccurrenceStatus::Skipped,
            "suppressed" => OccurrenceStatus::Suppressed,
            _ => OccurrenceStatus::Pending,
        }
    }
}

/// Errors emitted by `ScheduleStore` implementations.
#[derive(Debug, Error)]
pub enum ScheduleError {
    #[error("storage failure: {0}")]
    Storage(#[from] StorageError),

    #[error("schedule '{0}' not found")]
    ScheduleNotFound(String),

    #[error("occurrence for schedule '{0}' at {1} already exists")]
    DuplicateOccurrence(String, i64),

    #[error("serialization failure: {0}")]
    Serialization(String),

    #[error("schedule '{0}' is in terminal state {1:?}")]
    Terminal(String, ScheduleState),

    #[error("invalid schedule kind: {0}")]
    InvalidKind(String),
}

/// Storage trait for durable schedule records.
#[async_trait]
pub trait ScheduleStore: Send + Sync {
    /// Persist a new schedule.
    async fn create(&self, template: ScheduleTemplate) -> Result<ScheduleRecord, ScheduleError>;

    /// Pause or resume a schedule.
    async fn set_state(
        &self,
        id: &crate::jobs::ScheduleId,
        state: ScheduleState,
    ) -> Result<ScheduleRecord, ScheduleError>;

    /// Delete a schedule.
    async fn delete(&self, id: &crate::jobs::ScheduleId) -> Result<(), ScheduleError>;

    /// Fetch a schedule by id.
    async fn get(
        &self,
        id: &crate::jobs::ScheduleId,
    ) -> Result<Option<ScheduleRecord>, ScheduleError>;

    /// List schedules matching `query`.
    async fn list(&self, query: ScheduleQuery) -> Result<Vec<ScheduleSummary>, ScheduleError>;

    /// Claim any due occurrences (scheduled_for <= `now`). For each
    /// claim, atomically insert a `schedule_occurrence` row with status
    /// `Queued`, attach the produced `JobId`, and update the
    /// schedule's `next_run_at`.
    async fn claim_due(
        &self,
        now: DateTime<Utc>,
        materialize: &dyn OccurrenceMaterializer,
    ) -> Result<Vec<ClaimedOccurrence>, ScheduleError>;
}

/// Result of a successful occurrence claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedOccurrence {
    pub schedule_id: crate::jobs::ScheduleId,
    pub scheduled_for: DateTime<Utc>,
    pub job_id: JobId,
    pub status: OccurrenceStatus,
}

/// Materializes a job for a single fired occurrence. The schedule
/// store does not own the executor; this callback lets the daemon
/// translate a `JobTemplate` into a `JobId` exactly once per
/// occurrence.
#[async_trait]
pub trait OccurrenceMaterializer: Send + Sync {
    async fn materialize(
        &self,
        schedule_id: &crate::jobs::ScheduleId,
        template: &JobTemplate,
        scheduled_for: DateTime<Utc>,
    ) -> Result<JobId, MaterializerError>;
}

/// Errors from `OccurrenceMaterializer::materialize`.
#[derive(Debug, Error)]
pub enum MaterializerError {
    #[error("job store error: {0}")]
    JobStore(#[from] crate::jobs::JobStoreError),

    #[error("invalid template: {0}")]
    InvalidTemplate(String),
}

impl JobTemplate {
    /// Convenience: build a template for a recurring prompt-driven
    /// subagent. Used by the legacy `BackgroundScheduler` adapter to
    /// migrate to the durable model without changing callers.
    pub fn for_subagent(
        kind: JobKind,
        prompt: String,
        agent: String,
        session_id: Option<String>,
    ) -> Self {
        Self {
            kind,
            priority: JobPriority::Background,
            payload: JobPayload::Subagent {
                prompt,
                agent,
                parent_id: session_id,
                denied_tools: Vec::new(),
                allowed_paths: Vec::new(),
                max_tool_calls: None,
            },
            resource_request: ResourceRequest::for_kind(kind),
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            timeout: None,
            labels: HashMap::new(),
        }
    }

    /// Build a job source for a firing occurrence.
    pub fn job_source(
        schedule_id: &crate::jobs::ScheduleId,
        scheduled_for: DateTime<Utc>,
    ) -> JobSource {
        JobSource::Scheduled {
            schedule_id: schedule_id.clone(),
            occurrence: scheduled_for,
        }
    }
}

/// Compute the next run time for a schedule kind. Returns `None` when
/// the schedule is exhausted (e.g. one-shot whose `run_at` is past).
pub fn compute_next_run(
    kind: &ScheduleKind,
    now: DateTime<Utc>,
    last: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    match kind {
        ScheduleKind::OneShot { run_at } => {
            if last.is_some() {
                None
            } else {
                Some(*run_at)
            }
        }
        ScheduleKind::Interval { every, anchor } => {
            if every.as_secs() == 0 {
                return None;
            }
            let every_secs = every.as_secs() as i64;
            let anchor_ts = anchor.timestamp();
            let now_ts = now.timestamp();
            let base = last.map(|d| d.timestamp()).unwrap_or(anchor_ts);
            let next = if now_ts <= base {
                base
            } else {
                base + ((now_ts - base + every_secs - 1) / every_secs) * every_secs
            };
            chrono::DateTime::<Utc>::from_timestamp(next, 0)
        }
    }
}

/// Apply the missed-run policy against the gap between `last_fired`
/// and `now`, given `kind`. Returns the list of scheduled_for
/// timestamps that should actually run.
pub fn missed_run_targets(
    kind: &ScheduleKind,
    last_fired: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    policy: &MissedRunPolicy,
) -> Vec<DateTime<Utc>> {
    let candidates: Vec<DateTime<Utc>> = match kind {
        ScheduleKind::OneShot { run_at } => {
            if last_fired.is_some() {
                Vec::new()
            } else if *run_at <= now {
                vec![*run_at]
            } else {
                Vec::new()
            }
        }
        ScheduleKind::Interval { every, anchor } => {
            let every_secs = every.as_secs() as i64;
            if every_secs == 0 {
                return Vec::new();
            }
            let anchor_ts = anchor.timestamp();
            let now_ts = now.timestamp();
            let start = last_fired
                .map(|d| d.timestamp() + every_secs)
                .unwrap_or(anchor_ts);
            if start > now_ts {
                return Vec::new();
            }
            let count = ((now_ts - start) / every_secs) + 1;
            let mut out: Vec<DateTime<Utc>> = (0..count)
                .filter_map(|i| chrono::DateTime::<Utc>::from_timestamp(start + i * every_secs, 0))
                .collect();
            if let MissedRunPolicy::CatchUpBounded { max_occurrences } = policy {
                let max = *max_occurrences as usize;
                if out.len() > max {
                    let drop_count = out.len() - max;
                    out.drain(0..drop_count);
                }
            }
            out
        }
    };
    match policy {
        MissedRunPolicy::Skip => Vec::new(),
        MissedRunPolicy::RunOnceNow => candidates.into_iter().last().into_iter().collect(),
        MissedRunPolicy::CatchUpBounded { .. } => candidates,
    }
}

#[allow(dead_code)]
fn _ensure_pathbuf_used(_p: &PathBuf) {}
#[allow(dead_code)]
fn _ensure_arc_used<T>(_a: &Arc<T>) {}
