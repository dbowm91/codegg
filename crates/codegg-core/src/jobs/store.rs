//! [`JobStore`] trait surface plus concrete SQLite and in-memory
//! implementations.
//!
//! The SQLite-backed implementation is authoritative in production. The
//! in-memory implementation provides a hermetic conformance surface for
//! state-machine tests and migration tests that do not need to
//! round-trip through SQL.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::Mutex as SyncMutex;
use sqlx::{Row, SqlitePool};
use tokio::sync::Mutex as AsyncMutex;

use crate::error::StorageError;
use crate::jobs::{
    AttemptCompletion, AttemptId, AttemptState, CancelOutcome, CancelReason, CancelResult,
    DaemonGeneration, IdempotencyClass, JobAttempt, JobErrorRecord, JobId, JobKind, JobPayload,
    JobPriority, JobRecord, JobSource, NewJob, RecoveryPolicy, RecoveryReport, ResourceRequest,
    RetryPolicy,
};
use crate::workspace::WorkspaceId;

use super::{JobState, JobStore, JobStoreError};

/// Query parameters for [`JobStore::list_jobs`].
#[derive(Debug, Clone, Default)]
pub struct JobStoreQuery {
    pub workspace_id: Option<WorkspaceId>,
    pub states: Vec<JobState>,
    pub kinds: Vec<JobKind>,
    pub session_id: Option<String>,
    pub limit: Option<u32>,
}

/// Compact summary returned by [`JobStore::list_jobs`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSummary {
    pub job_id: JobId,
    pub workspace_id: WorkspaceId,
    pub kind: JobKind,
    pub priority: JobPriority,
    pub state: JobState,
    pub attempt_count: u32,
    pub current_attempt_id: Option<AttemptId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub schedule_id: Option<crate::jobs::ScheduleId>,
    pub cancel_requested_at: Option<DateTime<Utc>>,
}

pub fn job_state_to_str(s: JobState) -> &'static str {
    s.as_str()
}

pub fn attempt_state_to_str(s: AttemptState) -> &'static str {
    s.as_str()
}

/// Validate a job-state transition. Returns `Ok(())` if the transition
/// is permitted by the documented state machine.
pub fn validate_state_transition(from: JobState, to: JobState) -> Result<(), JobStoreError> {
    let allowed = job_state_transitions(from);
    if allowed.contains(&to) {
        Ok(())
    } else {
        Err(JobStoreError::InvalidTransition {
            job: String::new(),
            from,
            to,
        })
    }
}

/// The full set of job states a given state may transition into.
/// Centralized here so tests can exhaustively assert against the
/// rules.
pub fn job_state_transitions(from: JobState) -> &'static [JobState] {
    use JobState::*;
    match from {
        Scheduled => &[Queued, Cancelled, Expired],
        Queued => &[Running, Cancelled, Expired, Blocked],
        Running => &[Completed, Failed, Cancelled, TimedOut, Interrupted],
        Completed => &[],
        Failed => &[Queued],
        Cancelled => &[],
        TimedOut => &[Queued],
        Interrupted => &[Queued],
        Blocked => &[Queued, Cancelled, Expired],
        Expired => &[],
    }
}

/// Attempt-state transition table. Used to validate attempt-side
/// transitions before they are persisted.
pub fn attempt_state_transitions(from: AttemptState) -> &'static [AttemptState] {
    use AttemptState::*;
    match from {
        Created => &[Admitted, Running, Failed, Cancelled, Interrupted],
        Admitted => &[Running, Failed, Cancelled, Interrupted],
        Running => &[Completed, Failed, Cancelled, TimedOut, Interrupted],
        Completed => &[],
        Failed => &[],
        Cancelled => &[],
        TimedOut => &[],
        Interrupted => &[],
    }
}

/// Validate an attempt-state transition.
pub fn validate_attempt_transition(
    from: AttemptState,
    to: AttemptState,
) -> Result<(), JobStoreError> {
    let allowed = attempt_state_transitions(from);
    if allowed.contains(&to) {
        Ok(())
    } else {
        Err(JobStoreError::InvalidAttemptTransition {
            attempt: String::new(),
            from,
            to,
        })
    }
}

fn serialize_payload(payload: &JobPayload) -> Result<String, JobStoreError> {
    serde_json::to_string(payload).map_err(|e| JobStoreError::Serialization(e.to_string()))
}

fn deserialize_payload(s: &str) -> Result<JobPayload, JobStoreError> {
    serde_json::from_str(s).map_err(|e| JobStoreError::Serialization(e.to_string()))
}

fn serialize_source(source: &JobSource) -> Result<String, JobStoreError> {
    serde_json::to_string(source).map_err(|e| JobStoreError::Serialization(e.to_string()))
}

fn deserialize_source(s: &str) -> Result<JobSource, JobStoreError> {
    serde_json::from_str(s).map_err(|e| JobStoreError::Serialization(e.to_string()))
}

fn serialize_retry(p: &RetryPolicy) -> Result<String, JobStoreError> {
    serde_json::to_string(p).map_err(|e| JobStoreError::Serialization(e.to_string()))
}

fn deserialize_retry(s: &str) -> Result<RetryPolicy, JobStoreError> {
    serde_json::from_str(s).map_err(|e| JobStoreError::Serialization(e.to_string()))
}

fn serialize_resources(r: &ResourceRequest) -> Result<String, JobStoreError> {
    serde_json::to_string(r).map_err(|e| JobStoreError::Serialization(e.to_string()))
}

fn deserialize_resources(s: &str) -> Result<ResourceRequest, JobStoreError> {
    serde_json::from_str(s).map_err(|e| JobStoreError::Serialization(e.to_string()))
}

fn serialize_error(e: &Option<JobErrorRecord>) -> Result<Option<String>, JobStoreError> {
    match e {
        Some(rec) => serde_json::to_string(rec)
            .map(Some)
            .map_err(|e| JobStoreError::Serialization(e.to_string())),
        None => Ok(None),
    }
}

fn deserialize_error(s: Option<String>) -> Result<Option<JobErrorRecord>, JobStoreError> {
    match s {
        Some(s) => serde_json::from_str(&s)
            .map(Some)
            .map_err(|e| JobStoreError::Serialization(e.to_string())),
        None => Ok(None),
    }
}

fn priority_to_str(p: JobPriority) -> &'static str {
    p.as_str()
}

fn priority_from_str(s: &str) -> JobPriority {
    JobPriority::from_str_lossy(s)
}

// ── In-memory implementation ──────────────────────────────────────────────

/// In-memory job store. Used for state-machine unit tests and as a
/// fast default for tests that don't need durability. Operations are
/// serialized through an `AsyncMutex` so concurrent attempts to begin
/// a new attempt are correctly single-flighted.
pub struct InMemoryJobStore {
    inner: AsyncMutex<Inner>,
}

#[derive(Default)]
struct Inner {
    jobs: HashMap<JobId, JobRecord>,
    attempts: HashMap<AttemptId, JobAttempt>,
    attempts_by_job: HashMap<JobId, Vec<AttemptId>>,
}

impl InMemoryJobStore {
    pub fn new() -> Self {
        Self {
            inner: AsyncMutex::new(Inner::default()),
        }
    }
}

impl Default for InMemoryJobStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl JobStore for InMemoryJobStore {
    async fn create_job(&self, spec: NewJob) -> Result<JobRecord, JobStoreError> {
        let mut guard = self.inner.lock().await;
        let now = Utc::now();
        let job_id = JobId::new_unchecked(uuid::Uuid::new_v4().to_string());
        let record = JobRecord {
            job_id: job_id.clone(),
            workspace_id: spec.workspace_id,
            session_id: spec.session_id,
            turn_id: spec.turn_id,
            kind: spec.kind,
            source: spec.source,
            priority: spec.priority,
            payload: spec.payload,
            resource_request: spec.resource_request,
            timeout: spec.timeout,
            retry_policy: spec.retry_policy,
            idempotency: spec.idempotency,
            state: JobState::Queued,
            current_attempt_id: None,
            attempt_count: 0,
            not_before: spec.not_before,
            deadline: spec.deadline,
            schedule_id: spec.schedule_id,
            created_at: now,
            updated_at: now,
            terminal_at: None,
            cancel_requested_at: None,
            cancel_reason: None,
            depends_on: spec.depends_on,
            labels: HashMap::new(),
        };
        guard.jobs.insert(job_id.clone(), record.clone());
        Ok(record)
    }

    async fn get_job(&self, id: &JobId) -> Result<Option<JobRecord>, JobStoreError> {
        let guard = self.inner.lock().await;
        Ok(guard.jobs.get(id).cloned())
    }

    async fn list_jobs(&self, query: JobStoreQuery) -> Result<Vec<JobSummary>, JobStoreError> {
        let guard = self.inner.lock().await;
        let mut out: Vec<JobSummary> = guard
            .jobs
            .values()
            .filter(|r| match &query.workspace_id {
                Some(w) => r.workspace_id == *w,
                None => true,
            })
            .filter(|r| query.states.is_empty() || query.states.contains(&r.state))
            .filter(|r| query.kinds.is_empty() || query.kinds.contains(&r.kind))
            .filter(|r| match &query.session_id {
                Some(s) => r.session_id.as_deref() == Some(s.as_str()),
                None => true,
            })
            .map(|r| JobSummary {
                job_id: r.job_id.clone(),
                workspace_id: r.workspace_id.clone(),
                kind: r.kind,
                priority: r.priority,
                state: r.state,
                attempt_count: r.attempt_count,
                current_attempt_id: r.current_attempt_id.clone(),
                created_at: r.created_at,
                updated_at: r.updated_at,
                schedule_id: r.schedule_id.clone(),
                cancel_requested_at: r.cancel_requested_at,
            })
            .collect();
        out.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        if let Some(limit) = query.limit {
            out.truncate(limit as usize);
        }
        Ok(out)
    }

    async fn list_attempts(&self, job_id: &JobId) -> Result<Vec<JobAttempt>, JobStoreError> {
        let guard = self.inner.lock().await;
        let ids = match guard.attempts_by_job.get(job_id) {
            Some(ids) => ids.clone(),
            None => return Ok(Vec::new()),
        };
        let mut attempts: Vec<JobAttempt> = ids
            .iter()
            .filter_map(|aid| guard.attempts.get(aid).cloned())
            .collect();
        attempts.sort_by_key(|a| a.sequence);
        Ok(attempts)
    }

    async fn enqueue(&self, id: &JobId) -> Result<JobRecord, JobStoreError> {
        let mut guard = self.inner.lock().await;
        let job = guard
            .jobs
            .get(id)
            .cloned()
            .ok_or_else(|| JobStoreError::JobNotFound(id.to_string()))?;
        validate_state_transition(job.state, JobState::Queued).map_err(|mut e| {
            if let JobStoreError::InvalidTransition { job: ref mut s, .. } = e {
                *s = id.to_string();
            }
            e
        })?;
        let now = Utc::now();
        let updated = JobRecord {
            state: JobState::Queued,
            updated_at: now,
            ..job
        };
        guard.jobs.insert(id.clone(), updated.clone());
        Ok(updated)
    }

    async fn begin_attempt(
        &self,
        id: &JobId,
        generation: &DaemonGeneration,
    ) -> Result<JobAttempt, JobStoreError> {
        let mut guard = self.inner.lock().await;
        let job = guard
            .jobs
            .get(id)
            .cloned()
            .ok_or_else(|| JobStoreError::JobNotFound(id.to_string()))?;
        if matches!(job.state, JobState::Running) {
            return Err(JobStoreError::JobAlreadyRunning(
                id.to_string(),
                job.current_attempt_id
                    .map(|a| a.to_string())
                    .unwrap_or_default(),
            ));
        }
        if job.state.is_terminal() {
            return Err(JobStoreError::AlreadyTerminal(id.to_string(), job.state));
        }
        validate_state_transition(job.state, JobState::Running).map_err(|mut e| {
            if let JobStoreError::InvalidTransition { job: ref mut s, .. } = e {
                *s = id.to_string();
            }
            e
        })?;
        let now = Utc::now();
        let attempt_id = AttemptId::new_unchecked(uuid::Uuid::new_v4().to_string());
        let next_seq = job.attempt_count + 1;
        let attempt = JobAttempt {
            attempt_id: attempt_id.clone(),
            job_id: id.clone(),
            sequence: next_seq,
            state: AttemptState::Created,
            daemon_generation: generation.clone(),
            executor: None,
            run_id: None,
            heartbeat_at: None,
            started_at: None,
            completed_at: None,
            error: None,
            created_at: now,
            updated_at: now,
        };
        let updated_job = JobRecord {
            state: JobState::Running,
            attempt_count: next_seq,
            current_attempt_id: Some(attempt_id.clone()),
            updated_at: now,
            ..job
        };
        guard.attempts.insert(attempt_id.clone(), attempt.clone());
        guard
            .attempts_by_job
            .entry(id.clone())
            .or_insert_with(Vec::new)
            .push(attempt_id);
        guard.jobs.insert(id.clone(), updated_job);
        Ok(attempt)
    }

    async fn mark_attempt_running(&self, attempt_id: &AttemptId) -> Result<(), JobStoreError> {
        let mut guard = self.inner.lock().await;
        let attempt = guard
            .attempts
            .get(attempt_id)
            .cloned()
            .ok_or_else(|| JobStoreError::AttemptNotFound(attempt_id.to_string()))?;
        if attempt.state == AttemptState::Running {
            return Ok(());
        }
        validate_attempt_transition(attempt.state, AttemptState::Running).map_err(|mut e| {
            if let JobStoreError::InvalidAttemptTransition {
                attempt: ref mut s, ..
            } = e
            {
                *s = attempt_id.to_string();
            }
            e
        })?;
        let now = Utc::now();
        let updated = JobAttempt {
            state: AttemptState::Running,
            started_at: Some(now),
            heartbeat_at: Some(now),
            updated_at: now,
            ..attempt
        };
        guard.attempts.insert(attempt_id.clone(), updated);
        Ok(())
    }

    async fn set_attempt_executor(
        &self,
        attempt_id: &AttemptId,
        executor: &str,
    ) -> Result<(), JobStoreError> {
        let mut guard = self.inner.lock().await;
        let attempt = guard
            .attempts
            .get(attempt_id)
            .cloned()
            .ok_or_else(|| JobStoreError::AttemptNotFound(attempt_id.to_string()))?;
        guard.attempts.insert(
            attempt_id.clone(),
            JobAttempt {
                executor: Some(executor.to_string()),
                updated_at: Utc::now(),
                ..attempt
            },
        );
        Ok(())
    }

    async fn record_heartbeat(
        &self,
        attempt_id: &AttemptId,
        at: DateTime<Utc>,
    ) -> Result<(), JobStoreError> {
        let mut guard = self.inner.lock().await;
        let attempt = guard
            .attempts
            .get(attempt_id)
            .cloned()
            .ok_or_else(|| JobStoreError::AttemptNotFound(attempt_id.to_string()))?;
        let updated = JobAttempt {
            heartbeat_at: Some(at),
            updated_at: at,
            ..attempt
        };
        guard.attempts.insert(attempt_id.clone(), updated);
        Ok(())
    }

    async fn finish_attempt(
        &self,
        completion: AttemptCompletion,
    ) -> Result<JobRecord, JobStoreError> {
        let mut guard = self.inner.lock().await;
        let attempt = guard
            .attempts
            .get(&completion.attempt_id)
            .cloned()
            .ok_or_else(|| JobStoreError::AttemptNotFound(completion.attempt_id.to_string()))?;
        validate_attempt_transition(attempt.state, completion.state).map_err(|mut e| {
            if let JobStoreError::InvalidAttemptTransition {
                attempt: ref mut s, ..
            } = e
            {
                *s = completion.attempt_id.to_string();
            }
            e
        })?;
        let job = guard
            .jobs
            .get(&attempt.job_id)
            .cloned()
            .ok_or_else(|| JobStoreError::JobNotFound(attempt.job_id.to_string()))?;
        let now = Utc::now();
        let new_attempt = JobAttempt {
            state: completion.state,
            run_id: completion.run_id.or(attempt.run_id.clone()),
            completed_at: Some(now),
            heartbeat_at: Some(now),
            error: completion.error.clone().or(attempt.error.clone()),
            updated_at: now,
            ..attempt.clone()
        };
        guard
            .attempts
            .insert(completion.attempt_id.clone(), new_attempt);

        let new_job_state = match completion.state {
            AttemptState::Completed => JobState::Completed,
            AttemptState::Failed => JobState::Failed,
            AttemptState::Cancelled => JobState::Cancelled,
            AttemptState::TimedOut => JobState::TimedOut,
            AttemptState::Interrupted => JobState::Interrupted,
            // Non-terminal completion states do not advance the job.
            AttemptState::Created | AttemptState::Admitted | AttemptState::Running => job.state,
        };
        validate_state_transition(job.state, new_job_state).map_err(|mut e| {
            if let JobStoreError::InvalidTransition { job: ref mut s, .. } = e {
                *s = job.job_id.to_string();
            }
            e
        })?;
        let terminal = new_job_state.is_terminal();
        let updated_job = JobRecord {
            state: new_job_state,
            current_attempt_id: if terminal {
                None
            } else {
                job.current_attempt_id.clone()
            },
            updated_at: now,
            terminal_at: if terminal { Some(now) } else { job.terminal_at },
            ..job.clone()
        };
        guard
            .jobs
            .insert(updated_job.job_id.clone(), updated_job.clone());
        Ok(updated_job)
    }

    async fn request_cancel(
        &self,
        id: &JobId,
        reason: CancelReason,
    ) -> Result<CancelResult, JobStoreError> {
        let mut guard = self.inner.lock().await;
        let job = guard
            .jobs
            .get(id)
            .cloned()
            .ok_or_else(|| JobStoreError::JobNotFound(id.to_string()))?;
        let now = Utc::now();
        match job.state {
            JobState::Scheduled | JobState::Queued | JobState::Blocked => {
                validate_state_transition(job.state, JobState::Cancelled).map_err(|mut e| {
                    if let JobStoreError::InvalidTransition { job: ref mut s, .. } = e {
                        *s = id.to_string();
                    }
                    e
                })?;
                let updated = JobRecord {
                    state: JobState::Cancelled,
                    cancel_requested_at: Some(now),
                    cancel_reason: Some(reason.reason.clone()),
                    terminal_at: Some(now),
                    updated_at: now,
                    ..job
                };
                guard.jobs.insert(id.clone(), updated);
                Ok(CancelResult {
                    job_id: id.clone(),
                    state: CancelOutcome::Cancelled,
                    terminal: true,
                })
            }
            JobState::Running => {
                let updated = JobRecord {
                    cancel_requested_at: Some(now),
                    cancel_reason: Some(reason.reason.clone()),
                    updated_at: now,
                    ..job
                };
                guard.jobs.insert(id.clone(), updated);
                Ok(CancelResult {
                    job_id: id.clone(),
                    state: CancelOutcome::Requested,
                    terminal: false,
                })
            }
            JobState::Completed
            | JobState::Failed
            | JobState::Cancelled
            | JobState::TimedOut
            | JobState::Expired
            | JobState::Interrupted => Ok(CancelResult {
                job_id: id.clone(),
                state: CancelOutcome::AlreadyTerminal,
                terminal: true,
            }),
        }
    }

    async fn retry_job(
        &self,
        id: &JobId,
        generation: &DaemonGeneration,
        _prior_attempt_id: &AttemptId,
    ) -> Result<JobAttempt, JobStoreError> {
        // `retry_job` is a thin convenience wrapper: re-enqueue (if
        // currently Failed/TimedOut/Interrupted) and start a new
        // attempt. Eligibility is gated by the persisted retry policy.
        let guard = self.inner.lock().await;
        let job = guard
            .jobs
            .get(id)
            .cloned()
            .ok_or_else(|| JobStoreError::JobNotFound(id.to_string()))?;
        let prior_attempt = guard
            .attempts
            .get(_prior_attempt_id)
            .cloned()
            .ok_or_else(|| JobStoreError::AttemptNotFound(_prior_attempt_id.to_string()))?;
        if prior_attempt.job_id != *id {
            return Err(JobStoreError::Conflict(id.to_string()));
        }
        let eligible_states = [JobState::Failed, JobState::TimedOut, JobState::Interrupted];
        if !eligible_states.contains(&job.state) {
            return Err(JobStoreError::InvalidTransition {
                job: id.to_string(),
                from: job.state,
                to: JobState::Queued,
            });
        }
        if job.attempt_count >= job.retry_policy.max_attempts {
            return Err(JobStoreError::InvalidPayload(format!(
                "max attempts {} exhausted",
                job.retry_policy.max_attempts
            )));
        }
        if !job.idempotency.is_retry_eligible()
            && !matches!(job.idempotency, IdempotencyClass::Conditional)
        {
            return Err(JobStoreError::InvalidPayload(format!(
                "idempotency {:?} is not auto-retry eligible",
                job.idempotency
            )));
        }
        // Fall through to begin_attempt by releasing the lock and
        // delegating to the trait method.
        drop(guard);
        self.enqueue(id).await?;
        self.begin_attempt(id, generation).await
    }

    async fn block_job(&self, id: &JobId) -> Result<JobRecord, JobStoreError> {
        let mut guard = self.inner.lock().await;
        let job = guard
            .jobs
            .get(id)
            .cloned()
            .ok_or_else(|| JobStoreError::JobNotFound(id.to_string()))?;
        validate_state_transition(job.state, JobState::Blocked).map_err(|mut e| {
            if let JobStoreError::InvalidTransition { job: ref mut s, .. } = e {
                *s = id.to_string();
            }
            e
        })?;
        let now = Utc::now();
        let updated = JobRecord {
            state: JobState::Blocked,
            updated_at: now,
            ..job
        };
        guard.jobs.insert(id.clone(), updated.clone());
        Ok(updated)
    }

    async fn recover_generation(
        &self,
        stale: &DaemonGeneration,
        policy: &RecoveryPolicy,
    ) -> Result<RecoveryReport, JobStoreError> {
        let mut guard = self.inner.lock().await;
        let mut interrupted: u32 = 0;
        let mut requeued: u32 = 0;
        let mut terminals: u32 = 0;
        let now = Utc::now();
        let attempt_ids: Vec<AttemptId> = guard
            .attempts
            .iter()
            .filter_map(|(aid, a)| {
                if a.daemon_generation != *stale || a.state.is_terminal() {
                    None
                } else {
                    Some(aid.clone())
                }
            })
            .collect();
        let mut touched: HashSet<JobId> = HashSet::new();
        for aid in attempt_ids {
            let attempt = guard.attempts.get(&aid).cloned().unwrap();
            let updated_attempt = JobAttempt {
                state: AttemptState::Interrupted,
                completed_at: Some(now),
                updated_at: now,
                ..attempt.clone()
            };
            guard.attempts.insert(aid.clone(), updated_attempt);
            interrupted += 1;
            touched.insert(attempt.job_id.clone());
        }
        for job_id in &touched {
            let job = guard.jobs.get(job_id).cloned().unwrap();
            let eligible = match job.idempotency {
                IdempotencyClass::ReadOnly => policy.requeue_read_only,
                IdempotencyClass::SafeRepeat => policy.requeue_safe_repeat,
                IdempotencyClass::Conditional => policy.requeue_conditional,
                IdempotencyClass::NonIdempotent => policy.requeue_non_idempotent,
                IdempotencyClass::Destructive => policy.requeue_destructive,
            };
            if eligible && job.attempt_count < job.retry_policy.max_attempts {
                let updated = JobRecord {
                    state: JobState::Queued,
                    current_attempt_id: None,
                    cancel_requested_at: None,
                    cancel_reason: None,
                    updated_at: now,
                    ..job
                };
                guard.jobs.insert(job_id.clone(), updated);
                requeued += 1;
            } else {
                let updated = JobRecord {
                    state: JobState::Failed,
                    current_attempt_id: None,
                    terminal_at: Some(now),
                    updated_at: now,
                    ..job
                };
                guard.jobs.insert(job_id.clone(), updated);
                terminals += 1;
            }
        }
        Ok(RecoveryReport {
            interrupted_attempts: interrupted,
            requeued_jobs: requeued,
            terminal_jobs: terminals,
            schedules_reconciled: 0,
        })
    }
}

// ── SQLite implementation ─────────────────────────────────────────────────

/// SQLite-backed implementation of [`JobStore`].
pub struct SqliteJobStore {
    pool: SqlitePool,
}

impl SqliteJobStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[async_trait]
impl JobStore for SqliteJobStore {
    async fn create_job(&self, spec: NewJob) -> Result<JobRecord, JobStoreError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        let now = Utc::now();
        let job_id = JobId::new_unchecked(uuid::Uuid::new_v4().to_string());
        let payload_json = serialize_payload(&spec.payload)?;
        let source_json = serialize_source(&spec.source)?;
        let retry_json = serialize_retry(&spec.retry_policy)?;
        let resource_json = serialize_resources(&spec.resource_request)?;
        let kind_str = spec.kind.as_str();
        let priority_str = priority_to_str(spec.priority);
        let idempotency_str = match spec.idempotency {
            IdempotencyClass::ReadOnly => "read_only",
            IdempotencyClass::SafeRepeat => "safe_repeat",
            IdempotencyClass::Conditional => "conditional",
            IdempotencyClass::NonIdempotent => "non_idempotent",
            IdempotencyClass::Destructive => "destructive",
        };
        sqlx::query(
            r#"
            INSERT INTO job (
                id, workspace_id, session_id, turn_id, kind, source_json,
                priority, payload_json, resource_json, retry_json,
                idempotency, state, current_attempt_id, attempt_count,
                not_before, deadline, schedule_id,
                time_created, time_updated, time_terminal,
                cancel_requested_at, cancel_reason, labels_json
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'queued', NULL, 0,
                      ?, ?, ?, ?, ?, NULL, NULL, NULL, '{}')
            "#,
        )
        .bind(job_id.as_str())
        .bind(spec.workspace_id.as_str())
        .bind(spec.session_id.as_deref())
        .bind(spec.turn_id.as_deref())
        .bind(kind_str)
        .bind(&source_json)
        .bind(priority_str)
        .bind(&payload_json)
        .bind(&resource_json)
        .bind(&retry_json)
        .bind(idempotency_str)
        .bind(spec.not_before.map(|d| d.timestamp_millis()))
        .bind(spec.deadline.map(|d| d.timestamp_millis()))
        .bind(spec.schedule_id.as_ref().map(|s| s.as_str()))
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .execute(&mut *tx)
        .await
        .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        for dep in &spec.depends_on {
            let dep_id = uuid::Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT OR IGNORE INTO job_dependency (id, job_id, depends_on_job_id, condition) VALUES (?, ?, ?, 'completed')"
            )
            .bind(&dep_id)
            .bind(job_id.as_str())
            .bind(dep.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        }
        tx.commit()
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        let record = JobRecord {
            job_id,
            workspace_id: spec.workspace_id,
            session_id: spec.session_id,
            turn_id: spec.turn_id,
            kind: spec.kind,
            source: spec.source,
            priority: spec.priority,
            payload: spec.payload,
            resource_request: spec.resource_request,
            timeout: spec.timeout,
            retry_policy: spec.retry_policy,
            idempotency: spec.idempotency,
            state: JobState::Queued,
            current_attempt_id: None,
            attempt_count: 0,
            not_before: spec.not_before,
            deadline: spec.deadline,
            schedule_id: spec.schedule_id,
            created_at: now,
            updated_at: now,
            terminal_at: None,
            cancel_requested_at: None,
            cancel_reason: None,
            depends_on: spec.depends_on,
            labels: HashMap::new(),
        };
        Ok(record)
    }

    async fn get_job(&self, id: &JobId) -> Result<Option<JobRecord>, JobStoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, workspace_id, session_id, turn_id, kind, source_json,
                   priority, payload_json, resource_json, retry_json,
                   idempotency, state, current_attempt_id, attempt_count,
                   not_before, deadline, schedule_id,
                   time_created, time_updated, time_terminal,
                   cancel_requested_at, cancel_reason, labels_json
            FROM job WHERE id = ?
            "#,
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        let Some(row) = row else { return Ok(None) };
        Ok(Some(row_to_job(&row)?))
    }

    async fn list_jobs(&self, query: JobStoreQuery) -> Result<Vec<JobSummary>, JobStoreError> {
        let mut sql = String::from(
            r#"
            SELECT id, workspace_id, kind, priority, state, attempt_count,
                   current_attempt_id, time_created, time_updated,
                   schedule_id, cancel_requested_at
            FROM job WHERE 1=1
            "#,
        );
        if query.workspace_id.is_some() {
            sql.push_str(" AND workspace_id = ?");
        }
        if !query.states.is_empty() {
            sql.push_str(&format!(
                " AND state IN ({})",
                query
                    .states
                    .iter()
                    .map(|_| "?")
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if !query.kinds.is_empty() {
            sql.push_str(&format!(
                " AND kind IN ({})",
                query
                    .kinds
                    .iter()
                    .map(|_| "?")
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if query.session_id.is_some() {
            sql.push_str(" AND session_id = ?");
        }
        sql.push_str(" ORDER BY time_updated DESC");
        if let Some(limit) = query.limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        }
        let mut q = sqlx::query(&sql);
        if let Some(w) = &query.workspace_id {
            q = q.bind(w.as_str());
        }
        for s in &query.states {
            q = q.bind(s.as_str());
        }
        for k in &query.kinds {
            q = q.bind(k.as_str());
        }
        if let Some(s) = &query.session_id {
            q = q.bind(s);
        }
        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        let mut out: Vec<JobSummary> = rows
            .into_iter()
            .map(|row| {
                let job_id: String = row.get("id");
                let workspace_id: String = row.get("workspace_id");
                let kind: String = row.get("kind");
                let priority: String = row.get("priority");
                let state: String = row.get("state");
                let attempt_count: i64 = row.get("attempt_count");
                let current_attempt_id: Option<String> = row.get("current_attempt_id");
                let time_created: i64 = row.get("time_created");
                let time_updated: i64 = row.get("time_updated");
                let schedule_id: Option<String> = row.get("schedule_id");
                let cancel_requested_at: Option<i64> = row.get("cancel_requested_at");
                JobSummary {
                    job_id: JobId::new_unchecked(job_id),
                    workspace_id: WorkspaceId::new_unchecked(workspace_id),
                    kind: JobKind::from_str_lossy(&kind),
                    priority: priority_from_str(&priority),
                    state: JobState::from_str_lossy(&state),
                    attempt_count: attempt_count as u32,
                    current_attempt_id: current_attempt_id.map(AttemptId::new_unchecked),
                    created_at: chrono::DateTime::<Utc>::from_timestamp_millis(time_created)
                        .unwrap_or_else(Utc::now),
                    updated_at: chrono::DateTime::<Utc>::from_timestamp_millis(time_updated)
                        .unwrap_or_else(Utc::now),
                    schedule_id: schedule_id.map(crate::jobs::ScheduleId::new_unchecked),
                    cancel_requested_at: cancel_requested_at
                        .and_then(chrono::DateTime::<Utc>::from_timestamp_millis),
                }
            })
            .collect();
        if let Some(limit) = query.limit {
            out.truncate(limit as usize);
        }
        Ok(out)
    }

    async fn list_attempts(&self, job_id: &JobId) -> Result<Vec<JobAttempt>, JobStoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, job_id, sequence, state, daemon_generation, executor,
                   run_id, heartbeat_at, time_started, time_completed,
                   error_json, time_created, time_updated
            FROM job_attempt
            WHERE job_id = ?
            ORDER BY sequence ASC
            "#,
        )
        .bind(job_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        let mut attempts = Vec::with_capacity(rows.len());
        for row in rows {
            attempts.push(row_to_attempt(&row)?);
        }
        Ok(attempts)
    }

    async fn enqueue(&self, id: &JobId) -> Result<JobRecord, JobStoreError> {
        let existing = self
            .get_job(id)
            .await?
            .ok_or_else(|| JobStoreError::JobNotFound(id.to_string()))?;
        validate_state_transition(existing.state, JobState::Queued).map_err(|mut e| {
            if let JobStoreError::InvalidTransition { job: ref mut s, .. } = e {
                *s = id.to_string();
            }
            e
        })?;
        let now = Utc::now();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        let result = sqlx::query(
            r#"
            UPDATE job SET state = 'queued', time_updated = ?
            WHERE id = ? AND state IN ('scheduled', 'blocked', 'failed',
                                       'timed_out', 'interrupted')
            "#,
        )
        .bind(now.timestamp_millis())
        .bind(id.as_str())
        .execute(&mut *tx)
        .await
        .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        if result.rows_affected() == 0 {
            return Err(JobStoreError::InvalidTransition {
                job: id.to_string(),
                from: existing.state,
                to: JobState::Queued,
            });
        }
        tx.commit()
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        self.get_job(id)
            .await?
            .ok_or_else(|| JobStoreError::JobNotFound(id.to_string()))
    }

    async fn begin_attempt(
        &self,
        id: &JobId,
        generation: &DaemonGeneration,
    ) -> Result<JobAttempt, JobStoreError> {
        let job = self
            .get_job(id)
            .await?
            .ok_or_else(|| JobStoreError::JobNotFound(id.to_string()))?;
        if matches!(job.state, JobState::Running) {
            return Err(JobStoreError::JobAlreadyRunning(
                id.to_string(),
                job.current_attempt_id
                    .map(|a| a.to_string())
                    .unwrap_or_default(),
            ));
        }
        if job.state.is_terminal() {
            return Err(JobStoreError::AlreadyTerminal(id.to_string(), job.state));
        }
        validate_state_transition(job.state, JobState::Running).map_err(|mut e| {
            if let JobStoreError::InvalidTransition { job: ref mut s, .. } = e {
                *s = id.to_string();
            }
            e
        })?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        let now = Utc::now();
        let attempt_id = AttemptId::new_unchecked(uuid::Uuid::new_v4().to_string());
        let next_seq = job.attempt_count + 1;
        sqlx::query(
            r#"
            INSERT INTO job_attempt (
                id, job_id, sequence, state, daemon_generation, executor,
                run_id, heartbeat_at, time_started, time_completed,
                error_json, time_created, time_updated
            ) VALUES (?, ?, ?, 'created', ?, NULL, NULL, ?, NULL, NULL,
                      NULL, ?, ?)
            "#,
        )
        .bind(attempt_id.as_str())
        .bind(id.as_str())
        .bind(next_seq as i64)
        .bind(generation.as_str())
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .execute(&mut *tx)
        .await
        .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        let updated = sqlx::query(
            r#"
            UPDATE job SET state = 'running', attempt_count = ?,
                           current_attempt_id = ?, time_updated = ?
            WHERE id = ? AND state NOT IN ('running', 'completed', 'failed',
                                           'cancelled', 'timed_out', 'expired')
            "#,
        )
        .bind(next_seq as i64)
        .bind(attempt_id.as_str())
        .bind(now.timestamp_millis())
        .bind(id.as_str())
        .execute(&mut *tx)
        .await
        .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        if updated.rows_affected() == 0 {
            return Err(JobStoreError::Conflict(id.to_string()));
        }
        tx.commit()
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        Ok(JobAttempt {
            attempt_id,
            job_id: id.clone(),
            sequence: next_seq,
            state: AttemptState::Created,
            daemon_generation: generation.clone(),
            executor: None,
            run_id: None,
            heartbeat_at: Some(now),
            started_at: None,
            completed_at: None,
            error: None,
            created_at: now,
            updated_at: now,
        })
    }

    async fn mark_attempt_running(&self, attempt_id: &AttemptId) -> Result<(), JobStoreError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        let now = Utc::now();
        let result = sqlx::query(
            r#"
            UPDATE job_attempt
            SET state = 'running', time_started = ?, heartbeat_at = ?,
                time_updated = ?
            WHERE id = ? AND state IN ('created', 'admitted')
            "#,
        )
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .bind(attempt_id.as_str())
        .execute(&mut *tx)
        .await
        .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        if result.rows_affected() == 0 {
            return Err(JobStoreError::InvalidAttemptTransition {
                attempt: attempt_id.to_string(),
                from: AttemptState::Created,
                to: AttemptState::Running,
            });
        }
        tx.commit()
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        Ok(())
    }

    async fn set_attempt_executor(
        &self,
        attempt_id: &AttemptId,
        executor: &str,
    ) -> Result<(), JobStoreError> {
        let result =
            sqlx::query("UPDATE job_attempt SET executor = ?, time_updated = ? WHERE id = ?")
                .bind(executor)
                .bind(Utc::now().timestamp_millis())
                .bind(attempt_id.as_str())
                .execute(&self.pool)
                .await
                .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        if result.rows_affected() == 0 {
            return Err(JobStoreError::AttemptNotFound(attempt_id.to_string()));
        }
        Ok(())
    }

    async fn record_heartbeat(
        &self,
        attempt_id: &AttemptId,
        at: DateTime<Utc>,
    ) -> Result<(), JobStoreError> {
        sqlx::query("UPDATE job_attempt SET heartbeat_at = ?, time_updated = ? WHERE id = ?")
            .bind(at.timestamp_millis())
            .bind(at.timestamp_millis())
            .bind(attempt_id.as_str())
            .execute(&self.pool)
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        Ok(())
    }

    async fn finish_attempt(
        &self,
        completion: AttemptCompletion,
    ) -> Result<JobRecord, JobStoreError> {
        let now = Utc::now();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        let attempt_row = sqlx::query("SELECT job_id, state FROM job_attempt WHERE id = ?")
            .bind(completion.attempt_id.as_str())
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        let attempt_row = attempt_row
            .ok_or_else(|| JobStoreError::AttemptNotFound(completion.attempt_id.to_string()))?;
        let job_id_str: String = attempt_row.get("job_id");
        let attempt_state_str: String = attempt_row.get("state");
        let attempt_state = AttemptState::from_str_lossy(&attempt_state_str);
        validate_attempt_transition(attempt_state, completion.state).map_err(|mut e| {
            if let JobStoreError::InvalidAttemptTransition {
                attempt: ref mut s, ..
            } = e
            {
                *s = completion.attempt_id.to_string();
            }
            e
        })?;
        let error_json = serialize_error(&completion.error)?;
        sqlx::query(
            r#"
            UPDATE job_attempt SET state = ?, run_id = COALESCE(?, run_id),
                                   time_completed = ?, time_updated = ?,
                                   error_json = ?, heartbeat_at = ?
            WHERE id = ?
            "#,
        )
        .bind(completion.state.as_str())
        .bind(completion.run_id.as_ref().map(|r| r.as_str()))
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .bind(error_json.as_deref())
        .bind(now.timestamp_millis())
        .bind(completion.attempt_id.as_str())
        .execute(&mut *tx)
        .await
        .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        let new_job_state = match completion.state {
            AttemptState::Completed => JobState::Completed,
            AttemptState::Failed => JobState::Failed,
            AttemptState::Cancelled => JobState::Cancelled,
            AttemptState::TimedOut => JobState::TimedOut,
            AttemptState::Interrupted => JobState::Interrupted,
            _ => {
                return Err(JobStoreError::InvalidAttemptTransition {
                    attempt: completion.attempt_id.to_string(),
                    from: attempt_state,
                    to: completion.state,
                })
            }
        };
        let terminal = new_job_state.is_terminal();
        let query = if terminal {
            sqlx::query(
                r#"
                UPDATE job SET state = ?, current_attempt_id = NULL,
                               time_updated = ?, time_terminal = ?
                WHERE id = ?
                "#,
            )
        } else {
            sqlx::query(
                r#"
                UPDATE job SET state = ?, time_updated = ?
                WHERE id = ?
                "#,
            )
        };
        let q = query
            .bind(new_job_state.as_str())
            .bind(now.timestamp_millis());
        let q = if terminal {
            q.bind(now.timestamp_millis()).bind(job_id_str.clone())
        } else {
            q.bind(job_id_str.clone())
        };
        let result = q
            .execute(&mut *tx)
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        if result.rows_affected() == 0 {
            return Err(JobStoreError::Conflict(job_id_str.clone()));
        }
        tx.commit()
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        self.get_job(&JobId::new_unchecked(job_id_str))
            .await?
            .ok_or_else(|| JobStoreError::JobNotFound("<unknown>".to_string()))
    }

    async fn request_cancel(
        &self,
        id: &JobId,
        reason: CancelReason,
    ) -> Result<CancelResult, JobStoreError> {
        let existing = self
            .get_job(id)
            .await?
            .ok_or_else(|| JobStoreError::JobNotFound(id.to_string()))?;
        let now = Utc::now();
        match existing.state {
            JobState::Scheduled | JobState::Queued | JobState::Blocked => {
                validate_state_transition(existing.state, JobState::Cancelled).map_err(
                    |mut e| {
                        if let JobStoreError::InvalidTransition { job: ref mut s, .. } = e {
                            *s = id.to_string();
                        }
                        e
                    },
                )?;
                sqlx::query(
                    r#"
                    UPDATE job SET state = 'cancelled', cancel_requested_at = ?,
                                   cancel_reason = ?, time_updated = ?,
                                   time_terminal = ?
                    WHERE id = ?
                    "#,
                )
                .bind(now.timestamp_millis())
                .bind(&reason.reason)
                .bind(now.timestamp_millis())
                .bind(now.timestamp_millis())
                .bind(id.as_str())
                .execute(&self.pool)
                .await
                .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
                Ok(CancelResult {
                    job_id: id.clone(),
                    state: CancelOutcome::Cancelled,
                    terminal: true,
                })
            }
            JobState::Running => {
                sqlx::query(
                    r#"
                    UPDATE job SET cancel_requested_at = ?, cancel_reason = ?,
                                   time_updated = ?
                    WHERE id = ?
                    "#,
                )
                .bind(now.timestamp_millis())
                .bind(&reason.reason)
                .bind(now.timestamp_millis())
                .bind(id.as_str())
                .execute(&self.pool)
                .await
                .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
                Ok(CancelResult {
                    job_id: id.clone(),
                    state: CancelOutcome::Requested,
                    terminal: false,
                })
            }
            _ => Ok(CancelResult {
                job_id: id.clone(),
                state: CancelOutcome::AlreadyTerminal,
                terminal: true,
            }),
        }
    }

    async fn retry_job(
        &self,
        id: &JobId,
        generation: &DaemonGeneration,
        prior_attempt_id: &AttemptId,
    ) -> Result<JobAttempt, JobStoreError> {
        let existing = self
            .get_job(id)
            .await?
            .ok_or_else(|| JobStoreError::JobNotFound(id.to_string()))?;
        let prior = sqlx::query("SELECT job_id FROM job_attempt WHERE id = ?")
            .bind(prior_attempt_id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        let prior =
            prior.ok_or_else(|| JobStoreError::AttemptNotFound(prior_attempt_id.to_string()))?;
        let prior_job: String = prior.get("job_id");
        if prior_job != id.as_str() {
            return Err(JobStoreError::Conflict(id.to_string()));
        }
        let eligible_states = [JobState::Failed, JobState::TimedOut, JobState::Interrupted];
        if !eligible_states.contains(&existing.state) {
            return Err(JobStoreError::InvalidTransition {
                job: id.to_string(),
                from: existing.state,
                to: JobState::Queued,
            });
        }
        if existing.attempt_count >= existing.retry_policy.max_attempts {
            return Err(JobStoreError::InvalidPayload(format!(
                "max attempts {} exhausted",
                existing.retry_policy.max_attempts
            )));
        }
        if !existing.idempotency.is_retry_eligible()
            && !matches!(existing.idempotency, IdempotencyClass::Conditional)
        {
            return Err(JobStoreError::InvalidPayload(format!(
                "idempotency {:?} is not auto-retry eligible",
                existing.idempotency
            )));
        }
        self.enqueue(id).await?;
        self.begin_attempt(id, generation).await
    }

    async fn block_job(&self, id: &JobId) -> Result<JobRecord, JobStoreError> {
        let existing = self
            .get_job(id)
            .await?
            .ok_or_else(|| JobStoreError::JobNotFound(id.to_string()))?;
        validate_state_transition(existing.state, JobState::Blocked).map_err(|mut e| {
            if let JobStoreError::InvalidTransition { job: ref mut s, .. } = e {
                *s = id.to_string();
            }
            e
        })?;
        let now = Utc::now();
        sqlx::query("UPDATE job SET state = 'blocked', time_updated = ? WHERE id = ?")
            .bind(now.timestamp_millis())
            .bind(id.as_str())
            .execute(&self.pool)
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        self.get_job(id)
            .await?
            .ok_or_else(|| JobStoreError::JobNotFound(id.to_string()))
    }

    async fn recover_generation(
        &self,
        stale: &DaemonGeneration,
        policy: &RecoveryPolicy,
    ) -> Result<RecoveryReport, JobStoreError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        let now = Utc::now();
        let attempt_rows = sqlx::query(
            r#"
            SELECT id, job_id FROM job_attempt
            WHERE daemon_generation != ? AND state IN ('created', 'admitted', 'running')
            "#,
        )
        .bind(stale.as_str())
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        let mut interrupted: u32 = 0;
        let mut requeued: u32 = 0;
        let mut terminals: u32 = 0;
        let mut touched: std::collections::HashSet<String> = std::collections::HashSet::new();
        for row in attempt_rows {
            let attempt_id: String = row.get("id");
            let job_id: String = row.get("job_id");
            sqlx::query(
                r#"
                UPDATE job_attempt SET state = 'interrupted', time_completed = ?,
                                       time_updated = ?, heartbeat_at = ?
                WHERE id = ?
                "#,
            )
            .bind(now.timestamp_millis())
            .bind(now.timestamp_millis())
            .bind(now.timestamp_millis())
            .bind(&attempt_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
            interrupted += 1;
            touched.insert(job_id);
        }
        for job_id in &touched {
            let job_row = sqlx::query(
                r#"
                SELECT state, attempt_count, retry_json, idempotency, current_attempt_id
                FROM job WHERE id = ?
                "#,
            )
            .bind(job_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
            let current_state: String = job_row.get("state");
            let attempt_count: i64 = job_row.get("attempt_count");
            let retry_json: String = job_row.get("retry_json");
            let idempotency_str: String = job_row.get("idempotency");
            let current_attempt_id: Option<String> = job_row.get("current_attempt_id");
            let retry_policy: RetryPolicy = deserialize_retry(&retry_json)?;
            let idempotency = match idempotency_str.as_str() {
                "read_only" => IdempotencyClass::ReadOnly,
                "safe_repeat" => IdempotencyClass::SafeRepeat,
                "conditional" => IdempotencyClass::Conditional,
                "non_idempotent" => IdempotencyClass::NonIdempotent,
                "destructive" => IdempotencyClass::Destructive,
                _ => IdempotencyClass::NonIdempotent,
            };
            let eligible = match idempotency {
                IdempotencyClass::ReadOnly => policy.requeue_read_only,
                IdempotencyClass::SafeRepeat => policy.requeue_safe_repeat,
                IdempotencyClass::Conditional => policy.requeue_conditional,
                IdempotencyClass::NonIdempotent => policy.requeue_non_idempotent,
                IdempotencyClass::Destructive => policy.requeue_destructive,
            };
            if JobState::from_str_lossy(&current_state).is_terminal() {
                continue;
            }
            if eligible && (attempt_count as u32) < retry_policy.max_attempts {
                sqlx::query(
                    r#"
                    UPDATE job SET state = 'queued', current_attempt_id = NULL,
                                   cancel_requested_at = NULL, cancel_reason = NULL,
                                   time_updated = ?
                    WHERE id = ?
                    "#,
                )
                .bind(now.timestamp_millis())
                .bind(job_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
                requeued += 1;
            } else {
                sqlx::query(
                    r#"
                    UPDATE job SET state = 'failed', current_attempt_id = NULL,
                                   time_updated = ?, time_terminal = ?
                    WHERE id = ?
                    "#,
                )
                .bind(now.timestamp_millis())
                .bind(now.timestamp_millis())
                .bind(job_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
                terminals += 1;
            }
            let _ = current_attempt_id; // suppress unused
        }
        tx.commit()
            .await
            .map_err(|e| JobStoreError::Storage(StorageError::Database(e.to_string())))?;
        Ok(RecoveryReport {
            interrupted_attempts: interrupted,
            requeued_jobs: requeued,
            terminal_jobs: terminals,
            schedules_reconciled: 0,
        })
    }
}

// ── Row helpers ───────────────────────────────────────────────────────────

fn row_to_job(row: &sqlx::sqlite::SqliteRow) -> Result<JobRecord, JobStoreError> {
    let id: String = row.get("id");
    let workspace_id: String = row.get("workspace_id");
    let session_id: Option<String> = row.get("session_id");
    let turn_id: Option<String> = row.get("turn_id");
    let kind_str: String = row.get("kind");
    let source_json: String = row.get("source_json");
    let priority_str: String = row.get("priority");
    let payload_json: String = row.get("payload_json");
    let resource_json: String = row.get("resource_json");
    let retry_json: String = row.get("retry_json");
    let idempotency_str: String = row.get("idempotency");
    let state_str: String = row.get("state");
    let current_attempt_id: Option<String> = row.get("current_attempt_id");
    let attempt_count: i64 = row.get("attempt_count");
    let not_before_ms: Option<i64> = row.get("not_before");
    let deadline_ms: Option<i64> = row.get("deadline");
    let schedule_id: Option<String> = row.get("schedule_id");
    let time_created: i64 = row.get("time_created");
    let time_updated: i64 = row.get("time_updated");
    let time_terminal: Option<i64> = row.get("time_terminal");
    let cancel_requested_at: Option<i64> = row.get("cancel_requested_at");
    let cancel_reason: Option<String> = row.get("cancel_reason");
    let labels_json: String = row
        .try_get::<String, _>("labels_json")
        .unwrap_or_else(|_| "{}".to_string());
    let source: JobSource = deserialize_source(&source_json)?;
    let payload: JobPayload = deserialize_payload(&payload_json)?;
    let resource_request: ResourceRequest = deserialize_resources(&resource_json)?;
    let retry_policy: RetryPolicy = deserialize_retry(&retry_json)?;
    let idempotency = match idempotency_str.as_str() {
        "read_only" => IdempotencyClass::ReadOnly,
        "safe_repeat" => IdempotencyClass::SafeRepeat,
        "conditional" => IdempotencyClass::Conditional,
        "non_idempotent" => IdempotencyClass::NonIdempotent,
        "destructive" => IdempotencyClass::Destructive,
        _ => IdempotencyClass::NonIdempotent,
    };
    let labels: HashMap<String, String> = serde_json::from_str(&labels_json)
        .map_err(|e| JobStoreError::Serialization(e.to_string()))?;
    Ok(JobRecord {
        job_id: JobId::new_unchecked(id),
        workspace_id: WorkspaceId::new_unchecked(workspace_id),
        session_id,
        turn_id,
        kind: JobKind::from_str_lossy(&kind_str),
        source,
        priority: priority_from_str(&priority_str),
        payload,
        resource_request,
        timeout: None,
        retry_policy,
        idempotency,
        state: JobState::from_str_lossy(&state_str),
        current_attempt_id: current_attempt_id.map(AttemptId::new_unchecked),
        attempt_count: attempt_count as u32,
        not_before: not_before_ms.and_then(chrono::DateTime::<Utc>::from_timestamp_millis),
        deadline: deadline_ms.and_then(chrono::DateTime::<Utc>::from_timestamp_millis),
        schedule_id: schedule_id.map(crate::jobs::ScheduleId::new_unchecked),
        created_at: chrono::DateTime::<Utc>::from_timestamp_millis(time_created)
            .unwrap_or_else(Utc::now),
        updated_at: chrono::DateTime::<Utc>::from_timestamp_millis(time_updated)
            .unwrap_or_else(Utc::now),
        terminal_at: time_terminal.and_then(chrono::DateTime::<Utc>::from_timestamp_millis),
        cancel_requested_at: cancel_requested_at
            .and_then(chrono::DateTime::<Utc>::from_timestamp_millis),
        cancel_reason,
        depends_on: Vec::new(),
        labels,
    })
}

fn row_to_attempt(row: &sqlx::sqlite::SqliteRow) -> Result<JobAttempt, JobStoreError> {
    let id: String = row.get("id");
    let job_id: String = row.get("job_id");
    let sequence: i64 = row.get("sequence");
    let state_str: String = row.get("state");
    let daemon_generation: String = row.get("daemon_generation");
    let executor: Option<String> = row.get("executor");
    let run_id: Option<String> = row.get("run_id");
    let heartbeat_at: Option<i64> = row.get("heartbeat_at");
    let time_started: Option<i64> = row.get("time_started");
    let time_completed: Option<i64> = row.get("time_completed");
    let error_json: Option<String> = row.get("error_json");
    let time_created: i64 = row.get("time_created");
    let time_updated: i64 = row.get("time_updated");
    Ok(JobAttempt {
        attempt_id: AttemptId::new_unchecked(id),
        job_id: JobId::new_unchecked(job_id),
        sequence: sequence as u32,
        state: AttemptState::from_str_lossy(&state_str),
        daemon_generation: DaemonGeneration::new_unchecked(daemon_generation),
        executor,
        run_id: run_id.map(crate::run_store::RunId),
        heartbeat_at: heartbeat_at.and_then(chrono::DateTime::<Utc>::from_timestamp_millis),
        started_at: time_started.and_then(chrono::DateTime::<Utc>::from_timestamp_millis),
        completed_at: time_completed.and_then(chrono::DateTime::<Utc>::from_timestamp_millis),
        error: deserialize_error(error_json)?,
        created_at: chrono::DateTime::<Utc>::from_timestamp_millis(time_created)
            .unwrap_or_else(Utc::now),
        updated_at: chrono::DateTime::<Utc>::from_timestamp_millis(time_updated)
            .unwrap_or_else(Utc::now),
    })
}

#[allow(dead_code)]
fn _ensure_sync_mutex_used<T>(_m: &SyncMutex<T>) {}
#[allow(dead_code)]
fn _ensure_arc_used<T>(_a: &Arc<T>) {}
