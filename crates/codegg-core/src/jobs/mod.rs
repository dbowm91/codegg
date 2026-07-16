//! Phase 4 of the single-daemon multi-project orchestration roadmap:
//! durable jobs, attempts, and schedules.
//!
//! This module is the authoritative source of truth for the daemon's
//! queue and lifecycle control plane. A [`JobRecord`] is created before
//! any execution begins and persists through retries, cancellation,
//! daemon restarts, and manual interventions. Execution attempts are
//! modelled separately from the logical job so history, retries, and
//! restart recovery can be reasoned about independently.
//!
//! Three boundaries are enforced here:
//!
//! 1. **Identity.** Every persisted record uses an opaque typed newtype
//!    (`JobId`, `AttemptId`, `ScheduleId`, `DependencyId`,
//!    `DaemonGeneration`). Numeric IDs are never parsed.
//! 2. **Lifecycle.** Transitions go through intent-named methods on
//!    [`JobStore`]; generic `set_state` operations are not exposed.
//! 3. **Authority.** This module owns queue state. [`RunStore`] owns
//!    execution artifacts. Attempts carry an optional `run_id` link but
//!    the queue and the run store are not a single atomic transaction.
//!
//! The module is UI-, server-, plugin-, and auth-free: it is the lowest
//! level at which the daemon reasons about queued and scheduled work.
//! Concrete dispatching to executors lives in the root crate, behind
//! the [`JobDispatcher`] compatibility trait.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;

use crate::error::StorageError;
use crate::run_store::RunId;
use crate::workspace::WorkspaceId;

pub mod schedule;
pub mod schedule_store;
pub mod store;

pub use schedule::{
    compute_next_run, missed_run_targets, ClaimedOccurrence, MissedRunPolicy,
    OccurrenceMaterializer, OccurrenceStatus, OverlapPolicy, ScheduleError, ScheduleKind,
    ScheduleQuery, ScheduleRecord, ScheduleState, ScheduleStore, ScheduleSummary, ScheduleTemplate,
};
pub use schedule_store::{InMemoryScheduleStore, SqliteScheduleStore};

pub use store::{
    attempt_state_to_str, attempt_state_transitions, job_state_to_str, job_state_transitions,
    validate_state_transition, InMemoryJobStore, JobStoreQuery, JobSummary, SqliteJobStore,
};

/// Opaque, stable identifier for a durable job.
///
/// Created at submit time by the daemon and never re-derived. Equality
/// is structural (string compare) and the type is `Hash` so it can be
/// used as a map key. The string content is a UUID v4 produced by the
/// store at creation time.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(String);

impl JobId {
    /// Wrap an already-validated identifier. Prefer
    /// [`JobStore::create_job`] for new jobs.
    pub fn new_unchecked(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Opaque, stable identifier for a single execution attempt of a job.
///
/// The attempt id is paired with a monotonic `sequence` integer that
/// resets per job. New attempts for the same job always have strictly
/// greater sequences.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AttemptId(String);

impl AttemptId {
    pub fn new_unchecked(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AttemptId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Opaque, stable identifier for a durable schedule record.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScheduleId(String);

impl ScheduleId {
    pub fn new_unchecked(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ScheduleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Opaque, stable identifier for a job dependency edge.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DependencyId(String);

impl DependencyId {
    pub fn new_unchecked(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DependencyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Opaque, stable identifier for a daemon instance lifecycle. Every
/// daemon process generates a fresh `DaemonGeneration` at startup; a
/// running attempt is valid only while its stored generation matches
/// the active daemon generation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DaemonGeneration(String);

impl DaemonGeneration {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn new_unchecked(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for DaemonGeneration {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for DaemonGeneration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Top-level classification of a job. New variants are allowed in
/// persisted form; unknown future kinds deserialize into the
/// `Unsupported` catch-all so older daemons can still surface them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    AgentTurn,
    Subagent,
    Build,
    Test,
    Lint,
    Format,
    Shell,
    ManagedProcess,
    Python,
    GitRead,
    GitMutation,
    Research,
    Maintenance,
    /// Catch-all for forward compatibility. The daemon refuses to
    /// execute these but persists them so newer daemons can pick them
    /// up.
    #[serde(other)]
    Unsupported,
}

impl JobKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobKind::AgentTurn => "agent_turn",
            JobKind::Subagent => "subagent",
            JobKind::Build => "build",
            JobKind::Test => "test",
            JobKind::Lint => "lint",
            JobKind::Format => "format",
            JobKind::Shell => "shell",
            JobKind::ManagedProcess => "managed_process",
            JobKind::Python => "python",
            JobKind::GitRead => "git_read",
            JobKind::GitMutation => "git_mutation",
            JobKind::Research => "research",
            JobKind::Maintenance => "maintenance",
            JobKind::Unsupported => "unsupported",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "agent_turn" => JobKind::AgentTurn,
            "subagent" => JobKind::Subagent,
            "build" => JobKind::Build,
            "test" => JobKind::Test,
            "lint" => JobKind::Lint,
            "format" => JobKind::Format,
            "shell" => JobKind::Shell,
            "managed_process" => JobKind::ManagedProcess,
            "python" => JobKind::Python,
            "git_read" => JobKind::GitRead,
            "git_mutation" => JobKind::GitMutation,
            "research" => JobKind::Research,
            "maintenance" => JobKind::Maintenance,
            _ => JobKind::Unsupported,
        }
    }
}

/// Source attribution for a job. Persisted so audit and replay tools
/// can distinguish scheduled firings from interactive submissions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum JobSource {
    Interactive,
    Scheduled {
        schedule_id: ScheduleId,
        occurrence: DateTime<Utc>,
    },
    AgentDelegated,
    Retry {
        prior_attempt_id: AttemptId,
    },
    Maintenance,
    Api,
}

impl JobSource {
    pub fn tag(&self) -> &'static str {
        match self {
            JobSource::Interactive => "interactive",
            JobSource::Scheduled { .. } => "scheduled",
            JobSource::AgentDelegated => "agent_delegated",
            JobSource::Retry { .. } => "retry",
            JobSource::Maintenance => "maintenance",
            JobSource::Api => "api",
        }
    }
}

/// Persisted priority bucket. Affects Phase 5 admission ordering; in
/// Phase 4 priority is recorded and validated but not yet used by a
/// scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPriority {
    Urgent,
    Interactive,
    Normal,
    Background,
    Maintenance,
}

impl JobPriority {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobPriority::Urgent => "urgent",
            JobPriority::Interactive => "interactive",
            JobPriority::Normal => "normal",
            JobPriority::Background => "background",
            JobPriority::Maintenance => "maintenance",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "urgent" => JobPriority::Urgent,
            "interactive" => JobPriority::Interactive,
            "normal" => JobPriority::Normal,
            "background" => JobPriority::Background,
            "maintenance" => JobPriority::Maintenance,
            _ => JobPriority::Normal,
        }
    }
}

/// Resource request metadata persisted before Phase 5 admission exists.
/// Conservative defaults ensure that later scheduler changes do not
/// silently reinterpret old jobs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRequest {
    pub cpu_weight: u32,
    pub memory_mb_hint: u64,
    pub process_slots: u16,
    pub io_weight: u32,
    pub network_slots: u16,
    pub exclusivity_keys: Vec<String>,
}

impl Default for ResourceRequest {
    fn default() -> Self {
        Self {
            // Scheduler weights use the same units as the default
            // admission budget (single-digit values).  Older defaults
            // used percentages here, which made every default job
            // impossible to admit.
            cpu_weight: 1,
            memory_mb_hint: 256,
            process_slots: 1,
            io_weight: 1,
            network_slots: 0,
            exclusivity_keys: Vec::new(),
        }
    }
}

impl ResourceRequest {
    pub fn for_kind(kind: JobKind) -> Self {
        match kind {
            JobKind::AgentTurn | JobKind::Subagent | JobKind::Research => Self {
                cpu_weight: 1,
                memory_mb_hint: 512,
                process_slots: 1,
                io_weight: 1,
                network_slots: 1,
                exclusivity_keys: Vec::new(),
            },
            JobKind::Build => Self {
                cpu_weight: 3,
                memory_mb_hint: 2048,
                process_slots: 1,
                io_weight: 3,
                network_slots: 0,
                exclusivity_keys: vec!["exclusive:workspace-mutation".to_string()],
            },
            JobKind::Lint => Self {
                cpu_weight: 1,
                memory_mb_hint: 768,
                process_slots: 1,
                io_weight: 1,
                network_slots: 0,
                exclusivity_keys: Vec::new(),
            },
            JobKind::Format => Self {
                cpu_weight: 1,
                memory_mb_hint: 256,
                process_slots: 1,
                io_weight: 1,
                network_slots: 0,
                exclusivity_keys: vec!["exclusive:workspace-mutation".to_string()],
            },
            JobKind::Test => Self {
                cpu_weight: 2,
                memory_mb_hint: 1024,
                process_slots: 1,
                io_weight: 2,
                network_slots: 0,
                exclusivity_keys: Vec::new(),
            },
            JobKind::Shell | JobKind::ManagedProcess => Self {
                cpu_weight: 1,
                memory_mb_hint: 256,
                process_slots: 1,
                io_weight: 1,
                network_slots: 0,
                exclusivity_keys: Vec::new(),
            },
            JobKind::Python => Self {
                cpu_weight: 1,
                memory_mb_hint: 512,
                process_slots: 1,
                io_weight: 1,
                network_slots: 0,
                exclusivity_keys: Vec::new(),
            },
            JobKind::GitRead => Self {
                cpu_weight: 1,
                memory_mb_hint: 128,
                process_slots: 1,
                io_weight: 1,
                network_slots: 0,
                exclusivity_keys: Vec::new(),
            },
            JobKind::GitMutation => Self {
                cpu_weight: 1,
                memory_mb_hint: 256,
                process_slots: 1,
                io_weight: 1,
                network_slots: 0,
                exclusivity_keys: vec!["exclusive:worktree-mutation".to_string()],
            },
            JobKind::Maintenance => Self {
                cpu_weight: 1,
                memory_mb_hint: 128,
                process_slots: 1,
                io_weight: 1,
                network_slots: 0,
                exclusivity_keys: Vec::new(),
            },
            JobKind::Unsupported => Self::default(),
        }
    }
}

/// Retry policy. Persisted at creation time so restart recovery does not
/// reinterpret old jobs against current code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff: BackoffPolicy,
    pub retryable_failures: Vec<FailureClass>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 1,
            backoff: BackoffPolicy::None,
            retryable_failures: Vec::new(),
        }
    }
}

impl RetryPolicy {
    pub fn no_retry() -> Self {
        Self::default()
    }

    pub fn bounded(
        max_attempts: u32,
        backoff: BackoffPolicy,
        retryable: Vec<FailureClass>,
    ) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            backoff,
            retryable_failures: retryable,
        }
    }
}

/// Backoff strategy for retry attempts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackoffPolicy {
    None,
    Fixed { seconds: u32 },
    Linear { base_seconds: u32 },
    Exponential { base_seconds: u32, max_seconds: u32 },
}

/// Classification of failures used to drive retry eligibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    Transient,
    Timeout,
    Cancelled,
    Permission,
    Validation,
    Execution,
    Unknown,
}

/// Persisted idempotency classification. The daemon refuses to
/// auto-retry jobs whose persisted idempotency is `NonIdempotent` or
/// `Destructive` regardless of current code defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdempotencyClass {
    ReadOnly,
    SafeRepeat,
    Conditional,
    NonIdempotent,
    Destructive,
}

impl IdempotencyClass {
    pub fn is_retry_eligible(&self) -> bool {
        matches!(
            self,
            IdempotencyClass::ReadOnly | IdempotencyClass::SafeRepeat
        )
    }
}

/// Logical job state. Terminal states never transition except by
/// creating a new job or attempt; transitions go through the
/// `JobStore` API which validates them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Scheduled,
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Interrupted,
    Blocked,
    Expired,
}

impl JobState {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobState::Scheduled => "scheduled",
            JobState::Queued => "queued",
            JobState::Running => "running",
            JobState::Completed => "completed",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
            JobState::TimedOut => "timed_out",
            JobState::Interrupted => "interrupted",
            JobState::Blocked => "blocked",
            JobState::Expired => "expired",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "scheduled" => JobState::Scheduled,
            "queued" => JobState::Queued,
            "running" => JobState::Running,
            "completed" => JobState::Completed,
            "failed" => JobState::Failed,
            "cancelled" => JobState::Cancelled,
            "timed_out" => JobState::TimedOut,
            "interrupted" => JobState::Interrupted,
            "blocked" => JobState::Blocked,
            "expired" => JobState::Expired,
            _ => JobState::Failed,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobState::Completed
                | JobState::Failed
                | JobState::Cancelled
                | JobState::TimedOut
                | JobState::Expired
        )
    }
}

/// Per-attempt state. Mirrors job state but is namespaced for execution
/// attempts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptState {
    Created,
    Admitted,
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Interrupted,
}

impl AttemptState {
    pub fn as_str(&self) -> &'static str {
        match self {
            AttemptState::Created => "created",
            AttemptState::Admitted => "admitted",
            AttemptState::Running => "running",
            AttemptState::Completed => "completed",
            AttemptState::Failed => "failed",
            AttemptState::Cancelled => "cancelled",
            AttemptState::TimedOut => "timed_out",
            AttemptState::Interrupted => "interrupted",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "created" => AttemptState::Created,
            "admitted" => AttemptState::Admitted,
            "running" => AttemptState::Running,
            "completed" => AttemptState::Completed,
            "failed" => AttemptState::Failed,
            "cancelled" => AttemptState::Cancelled,
            "timed_out" => AttemptState::TimedOut,
            "interrupted" => AttemptState::Interrupted,
            _ => AttemptState::Failed,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            AttemptState::Completed
                | AttemptState::Failed
                | AttemptState::Cancelled
                | AttemptState::TimedOut
                | AttemptState::Interrupted
        )
    }
}

/// Typed job payload variants. Persisted as JSON for forward
/// compatibility; secret material is never embedded (use credential
/// references).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum JobPayload {
    AgentTurn {
        prompt: String,
        agent: String,
        model: Option<String>,
    },
    Subagent {
        prompt: String,
        agent: String,
        parent_id: Option<String>,
        denied_tools: Vec<String>,
        allowed_paths: Vec<String>,
        max_tool_calls: Option<u32>,
    },
    Test {
        command: String,
        argv: Vec<String>,
        cwd: Option<String>,
        scope: Option<String>,
    },
    ManagedArgv {
        argv: Vec<String>,
        cwd: Option<String>,
    },
    Shell {
        command: String,
        argv: Option<Vec<String>>,
        cwd: Option<String>,
    },
    Python {
        script_path: String,
        args: Vec<String>,
        mode: String,
    },
    Git {
        argv: Vec<String>,
        cwd: Option<String>,
    },
    Research {
        query: String,
        max_depth: Option<u32>,
    },
    Maintenance {
        task: String,
    },
}

/// Payload for a new job submission. Converted into a [`JobRecord`] by
/// [`JobStore::create_job`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewJob {
    pub workspace_id: WorkspaceId,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub kind: JobKind,
    pub source: JobSource,
    pub priority: JobPriority,
    pub payload: JobPayload,
    pub resource_request: ResourceRequest,
    pub timeout: Option<Duration>,
    pub retry_policy: RetryPolicy,
    pub idempotency: IdempotencyClass,
    pub not_before: Option<DateTime<Utc>>,
    pub deadline: Option<DateTime<Utc>>,
    pub schedule_id: Option<ScheduleId>,
    pub depends_on: Vec<JobId>,
}

impl NewJob {
    pub fn default_resource_for_kind(kind: JobKind) -> ResourceRequest {
        ResourceRequest::for_kind(kind)
    }
}

/// Optional structured error recorded on an attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobErrorRecord {
    pub class: FailureClass,
    pub message: String,
    pub transient: bool,
}

/// Logical job record persisted by [`JobStore`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRecord {
    pub job_id: JobId,
    pub workspace_id: WorkspaceId,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub kind: JobKind,
    pub source: JobSource,
    pub priority: JobPriority,
    pub payload: JobPayload,
    pub resource_request: ResourceRequest,
    pub timeout: Option<Duration>,
    pub retry_policy: RetryPolicy,
    pub idempotency: IdempotencyClass,
    pub state: JobState,
    pub current_attempt_id: Option<AttemptId>,
    pub attempt_count: u32,
    pub not_before: Option<DateTime<Utc>>,
    pub deadline: Option<DateTime<Utc>>,
    pub schedule_id: Option<ScheduleId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub terminal_at: Option<DateTime<Utc>>,
    pub cancel_requested_at: Option<DateTime<Utc>>,
    pub cancel_reason: Option<String>,
    pub depends_on: Vec<JobId>,
    /// Free-form metadata persisted alongside the job (tool name, run
    /// identifier, etc.). Not used by the queue state machine.
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

/// Per-attempt record persisted by [`JobStore`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobAttempt {
    pub attempt_id: AttemptId,
    pub job_id: JobId,
    pub sequence: u32,
    pub state: AttemptState,
    pub daemon_generation: DaemonGeneration,
    pub executor: Option<String>,
    pub run_id: Option<RunId>,
    pub heartbeat_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<JobErrorRecord>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Reason attached to a cancellation request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelReason {
    pub requested_by: String,
    pub reason: String,
}

impl CancelReason {
    pub fn new(requested_by: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            requested_by: requested_by.into(),
            reason: reason.into(),
        }
    }
}

/// Result of [`JobStore::request_cancel`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelResult {
    pub job_id: JobId,
    pub state: CancelOutcome,
    pub terminal: bool,
}

/// Outcome of a cancellation request, reflecting the deterministic
/// precedence rules in the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelOutcome {
    /// Cancellation was applied immediately because no attempt had
    /// started yet.
    Cancelled,
    /// Cancellation was marked as requested; the executor will be
    /// notified.
    Requested,
    /// The job was already terminal; cancellation was rejected.
    AlreadyTerminal,
}

/// Policy applied during daemon generation recovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryPolicy {
    pub requeue_read_only: bool,
    pub requeue_safe_repeat: bool,
    pub requeue_conditional: bool,
    pub requeue_non_idempotent: bool,
    pub requeue_destructive: bool,
}

impl Default for RecoveryPolicy {
    fn default() -> Self {
        Self {
            requeue_read_only: true,
            requeue_safe_repeat: true,
            requeue_conditional: false,
            requeue_non_idempotent: false,
            requeue_destructive: false,
        }
    }
}

/// Report summarizing a recovery pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryReport {
    pub interrupted_attempts: u32,
    pub requeued_jobs: u32,
    pub terminal_jobs: u32,
    pub schedules_reconciled: u32,
}

/// Parameters for finishing an attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptCompletion {
    pub attempt_id: AttemptId,
    pub state: AttemptState,
    pub error: Option<JobErrorRecord>,
    pub run_id: Option<RunId>,
}

/// Storage trait for the durable job control plane. Implementations are
/// responsible for persisting every lifecycle transition before or
/// atomically with externally visible state changes.
#[async_trait]
pub trait JobStore: Send + Sync {
    /// Persist a new job. Returns the assigned record including its
    /// generated `JobId`.
    async fn create_job(&self, spec: NewJob) -> Result<JobRecord, JobStoreError>;

    /// Fetch a job by id.
    async fn get_job(&self, id: &JobId) -> Result<Option<JobRecord>, JobStoreError>;

    /// Filter jobs according to `query`.
    async fn list_jobs(&self, query: JobStoreQuery) -> Result<Vec<JobSummary>, JobStoreError>;

    /// List all attempts for a job, ordered by sequence ascending.
    async fn list_attempts(&self, job_id: &JobId) -> Result<Vec<JobAttempt>, JobStoreError>;

    /// Move a job from `Scheduled` or `Blocked` to `Queued`. Validates
    /// the transition.
    async fn enqueue(&self, id: &JobId) -> Result<JobRecord, JobStoreError>;

    /// Transition the job to `Running` and create a fresh attempt
    /// under `generation`. Returns the attempt record. The job must
    /// currently be `Queued` or `Running` (for a retry), and must not
    /// already have a non-terminal attempt.
    async fn begin_attempt(
        &self,
        id: &JobId,
        generation: &DaemonGeneration,
    ) -> Result<JobAttempt, JobStoreError>;

    /// Transition an attempt from `Created`/`Admitted` to `Running`.
    async fn mark_attempt_running(&self, attempt_id: &AttemptId) -> Result<(), JobStoreError>;

    /// Persist the selected executor before process/agent dispatch. The
    /// default implementation keeps custom test stores source-compatible;
    /// the built-in stores persist it for recovery and auditability.
    async fn set_attempt_executor(
        &self,
        _attempt_id: &AttemptId,
        _executor: &str,
    ) -> Result<(), JobStoreError> {
        Ok(())
    }

    /// Persist attempt heart-beat at `at`.
    async fn record_heartbeat(
        &self,
        attempt_id: &AttemptId,
        at: DateTime<Utc>,
    ) -> Result<(), JobStoreError>;

    /// Persist attempt+job completion atomically. The attempt state is
    /// propagated to the job (e.g. Completed → Completed).
    async fn finish_attempt(
        &self,
        completion: AttemptCompletion,
    ) -> Result<JobRecord, JobStoreError>;

    /// Persist a cancellation request. Honours the deterministic
    /// precedence rules: queued → cancelled; running → request recorded
    /// and reported to caller; terminal → rejected.
    async fn request_cancel(
        &self,
        id: &JobId,
        reason: CancelReason,
    ) -> Result<CancelResult, JobStoreError>;

    /// Persist a retry decision for a non-terminal job. Creates a new
    /// attempt under the supplied generation. Returns the new attempt.
    async fn retry_job(
        &self,
        id: &JobId,
        generation: &DaemonGeneration,
        prior_attempt_id: &AttemptId,
    ) -> Result<JobAttempt, JobStoreError>;

    /// Persist a `Blocked` job whose dependencies are not yet satisfied.
    async fn block_job(&self, id: &JobId) -> Result<JobRecord, JobStoreError>;

    /// Reconcile a daemon-generation restart: any attempt whose stored
    /// `daemon_generation` does not match `stale` and is in a
    /// non-terminal state is marked `Interrupted` and the parent job
    /// either requeues or transitions to `Failed` according to policy.
    async fn recover_generation(
        &self,
        stale: &DaemonGeneration,
        policy: &RecoveryPolicy,
    ) -> Result<RecoveryReport, JobStoreError>;
}

/// Errors emitted by `JobStore` implementations.
#[derive(Debug, Error)]
pub enum JobStoreError {
    #[error("storage failure: {0}")]
    Storage(#[from] StorageError),

    #[error("job '{0}' not found")]
    JobNotFound(String),

    #[error("attempt '{0}' not found")]
    AttemptNotFound(String),

    #[error("invalid transition for job '{job}': {from:?} -> {to:?}")]
    InvalidTransition {
        job: String,
        from: JobState,
        to: JobState,
    },

    #[error("invalid transition for attempt '{attempt}': {from:?} -> {to:?}")]
    InvalidAttemptTransition {
        attempt: String,
        from: AttemptState,
        to: AttemptState,
    },

    #[error("job '{0}' already has an active attempt '{1}'")]
    JobAlreadyRunning(String, String),

    #[error("job '{0}' is already terminal in state {1:?}")]
    AlreadyTerminal(String, JobState),

    #[error("job '{0}' is not yet eligible (not_before in the future)")]
    NotYetEligible(String),

    #[error("job '{0}' past deadline")]
    PastDeadline(String),

    #[error("serialization failure: {0}")]
    Serialization(String),

    #[error("invalid payload: {0}")]
    InvalidPayload(String),

    #[error("concurrent modification of job '{0}'")]
    Conflict(String),
}

/// Recovery entry point. Performs a single recovery pass using the
/// provided job store. Returns the [`RecoveryReport`] summarizing the
/// pass. Intended to be invoked once at daemon startup.
pub async fn recover_at_startup(
    store: &Arc<dyn JobStore>,
    stale_generation: &DaemonGeneration,
    policy: &RecoveryPolicy,
) -> Result<RecoveryReport, JobStoreError> {
    store.recover_generation(stale_generation, policy).await
}

/// Re-export for backward compat with newtypes used elsewhere.
pub type JobError = JobStoreError;

// `Arc<Mutex<()>>` is used as an in-process fairness primitive in the
// in-memory store; reference it here to silence dead-code warnings on
// minimal build configurations where the in-memory variant is unused.
#[allow(dead_code)]
fn _ensure_async_mutex_used(_m: &AsyncMutex<()>) {}
#[allow(dead_code)]
fn _ensure_pathbuf_used(_p: &PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn typed_ids_roundtrip_through_strings() {
        let job = JobId::new_unchecked("job-1");
        let attempt = AttemptId::new_unchecked("att-1");
        let sched = ScheduleId::new_unchecked("sched-1");
        let dep = DependencyId::new_unchecked("dep-1");
        let gen = DaemonGeneration::new();
        assert_eq!(job.as_str(), "job-1");
        assert_eq!(attempt.as_str(), "att-1");
        assert_eq!(sched.as_str(), "sched-1");
        assert_eq!(dep.as_str(), "dep-1");
        assert!(!gen.as_str().is_empty());
    }

    #[test]
    fn job_kind_string_roundtrip() {
        for kind in [
            JobKind::AgentTurn,
            JobKind::Subagent,
            JobKind::Build,
            JobKind::Test,
            JobKind::Lint,
            JobKind::Format,
            JobKind::Shell,
            JobKind::ManagedProcess,
            JobKind::Python,
            JobKind::GitRead,
            JobKind::GitMutation,
            JobKind::Research,
            JobKind::Maintenance,
        ] {
            assert_eq!(JobKind::from_str_lossy(kind.as_str()), kind);
        }
        // Forward-compatible unknown kinds deserialize to Unsupported.
        assert_eq!(JobKind::from_str_lossy("future_kind"), JobKind::Unsupported);
    }

    #[test]
    fn job_state_string_roundtrip() {
        for state in [
            JobState::Scheduled,
            JobState::Queued,
            JobState::Running,
            JobState::Completed,
            JobState::Failed,
            JobState::Cancelled,
            JobState::TimedOut,
            JobState::Interrupted,
            JobState::Blocked,
            JobState::Expired,
        ] {
            assert_eq!(job_state_to_str(state), state.as_str());
            assert_eq!(JobState::from_str_lossy(state.as_str()), state);
        }
    }

    #[test]
    fn attempt_state_string_roundtrip() {
        for state in [
            AttemptState::Created,
            AttemptState::Admitted,
            AttemptState::Running,
            AttemptState::Completed,
            AttemptState::Failed,
            AttemptState::Cancelled,
            AttemptState::TimedOut,
            AttemptState::Interrupted,
        ] {
            assert_eq!(attempt_state_to_str(state), state.as_str());
            assert_eq!(AttemptState::from_str_lossy(state.as_str()), state);
        }
    }

    #[test]
    fn terminal_states_are_terminal() {
        assert!(JobState::Completed.is_terminal());
        assert!(JobState::Failed.is_terminal());
        assert!(JobState::Cancelled.is_terminal());
        assert!(JobState::TimedOut.is_terminal());
        assert!(JobState::Expired.is_terminal());
        assert!(!JobState::Queued.is_terminal());
        assert!(!JobState::Running.is_terminal());
        assert!(!JobState::Scheduled.is_terminal());
        assert!(!JobState::Blocked.is_terminal());
        assert!(!JobState::Interrupted.is_terminal());

        assert!(AttemptState::Completed.is_terminal());
        assert!(AttemptState::Failed.is_terminal());
        assert!(AttemptState::Cancelled.is_terminal());
        assert!(AttemptState::TimedOut.is_terminal());
        assert!(AttemptState::Interrupted.is_terminal());
        assert!(!AttemptState::Running.is_terminal());
        assert!(!AttemptState::Created.is_terminal());
        assert!(!AttemptState::Admitted.is_terminal());
    }

    #[test]
    fn idempotency_retry_eligibility() {
        assert!(IdempotencyClass::ReadOnly.is_retry_eligible());
        assert!(IdempotencyClass::SafeRepeat.is_retry_eligible());
        assert!(!IdempotencyClass::Conditional.is_retry_eligible());
        assert!(!IdempotencyClass::NonIdempotent.is_retry_eligible());
        assert!(!IdempotencyClass::Destructive.is_retry_eligible());
    }

    #[test]
    fn resource_request_defaults_are_safe() {
        let req = ResourceRequest::default();
        assert_eq!(req.cpu_weight, 1);
        assert_eq!(req.memory_mb_hint, 256);
        assert_eq!(req.process_slots, 1);
        assert_eq!(req.network_slots, 0);
    }

    #[test]
    fn resource_request_for_kind_is_conservative() {
        let agent = ResourceRequest::for_kind(JobKind::AgentTurn);
        assert!(agent.network_slots >= 1);
        let format = ResourceRequest::for_kind(JobKind::Format);
        assert!(format
            .exclusivity_keys
            .iter()
            .any(|k| k == "exclusive:workspace-mutation"));
        let git = ResourceRequest::for_kind(JobKind::GitMutation);
        assert!(git
            .exclusivity_keys
            .iter()
            .any(|k| k == "exclusive:worktree-mutation"));
    }

    #[test]
    fn retry_policy_no_retry_helper() {
        let p = RetryPolicy::no_retry();
        assert_eq!(p.max_attempts, 1);
        assert!(p.retryable_failures.is_empty());
    }

    #[test]
    fn retry_policy_bounded_helper_clamps_max_attempts() {
        let p = RetryPolicy::bounded(0, BackoffPolicy::None, vec![FailureClass::Transient]);
        assert_eq!(p.max_attempts, 1);
        let p2 = RetryPolicy::bounded(3, BackoffPolicy::None, vec![]);
        assert_eq!(p2.max_attempts, 3);
    }

    #[test]
    fn job_source_tags() {
        assert_eq!(JobSource::Interactive.tag(), "interactive");
        assert_eq!(JobSource::Maintenance.tag(), "maintenance");
        assert_eq!(JobSource::Api.tag(), "api");
        assert_eq!(
            JobSource::Scheduled {
                schedule_id: ScheduleId::new_unchecked("s"),
                occurrence: chrono::Utc::now(),
            }
            .tag(),
            "scheduled"
        );
        assert_eq!(
            JobSource::Retry {
                prior_attempt_id: AttemptId::new_unchecked("a")
            }
            .tag(),
            "retry"
        );
        assert_eq!(JobSource::AgentDelegated.tag(), "agent_delegated");
    }

    #[test]
    fn cancel_reason_helper() {
        let r = CancelReason::new("user", "abort");
        assert_eq!(r.requested_by, "user");
        assert_eq!(r.reason, "abort");
    }

    #[test]
    fn daemon_generation_unique() {
        let a = DaemonGeneration::new();
        let b = DaemonGeneration::new();
        assert_ne!(a, b);
        assert!(!a.as_str().is_empty());
    }

    #[test]
    fn resource_keys_dedupe() {
        let mut keys: HashSet<String> = HashSet::new();
        keys.insert("a".to_string());
        keys.insert("a".to_string());
        keys.insert("b".to_string());
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn payload_serializes_as_tagged_enum() {
        let payload = JobPayload::AgentTurn {
            prompt: "hi".to_string(),
            agent: "build".to_string(),
            model: None,
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["kind"], "agent_turn");
        assert_eq!(json["prompt"], "hi");
        assert_eq!(json["agent"], "build");
    }

    #[test]
    fn new_job_default_resource_for_kind() {
        let req = NewJob::default_resource_for_kind(JobKind::Build);
        assert_eq!(req.cpu_weight, 3);
    }

    #[test]
    fn attempt_completion_clone_eq() {
        let comp = AttemptCompletion {
            attempt_id: AttemptId::new_unchecked("a"),
            state: AttemptState::Completed,
            error: None,
            run_id: None,
        };
        assert_eq!(comp.clone(), comp);
    }
}
