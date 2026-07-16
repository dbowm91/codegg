//! Typed `JobExecutor` trait and registry.
//!
//! The scheduler is executor-agnostic: it walks the fair queue, asks
//! the admission controller for permits, then calls into a typed
//! executor to actually run the work. The trait intentionally does
//! not expose shell; executors are responsible for invoking existing
//! subsystems (TestRunner, managed argv, subagent pool) and must not
//! reconstruct shell from the typed payload.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use codegg_core::jobs::{JobId, JobKind, JobRecord};
use codegg_core::workspace::WorkspaceId;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::scheduler::permit::ResourcePermitGuard;
use codegg_core::run_store::RunId;

/// Why a particular `JobKind` failed the executor's pre-flight
/// validation. The scheduler uses this to mark the job `Failed` with
/// a structured reason and emit a diagnostic event.
#[derive(Debug, Clone, Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorValidationError {
    #[error("executor '{executor}' does not support job kind '{kind}'")]
    UnsupportedKind { executor: String, kind: String },
    #[error("job payload missing required field: {0}")]
    MissingField(String),
    #[error("job payload invalid: {0}")]
    InvalidPayload(String),
    #[error("workspace '{0}' is not registered")]
    UnknownWorkspace(String),
}

/// Per-execution metrics. Summary fields only; large output remains
/// in RunStore.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutorMetrics {
    pub cpu_time_ms: Option<u64>,
    pub peak_memory_mb: Option<u64>,
    pub elapsed_ms: u64,
}

/// Coarse executor health. Reported by the registry and surfaced via
/// `ExecutorHealthSnapshot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorHealth {
    Healthy,
    Degraded,
    Unavailable,
}

/// Whether a particular executor variant is currently accepting
/// dispatches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorAvailability {
    Available,
    Unavailable,
}

/// Executor identifier. Distinct from `JobKind` so multiple executor
/// implementations can be registered for the same `JobKind` (e.g.
/// `test` with `default` and `bash_dispatch` variants).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorKind {
    Test,
    ManagedArgv,
    Subagent,
    BashDispatch,
    Synthetic,
}

impl ExecutorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecutorKind::Test => "test",
            ExecutorKind::ManagedArgv => "managed_argv",
            ExecutorKind::Subagent => "subagent",
            ExecutorKind::BashDispatch => "bash_dispatch",
            ExecutorKind::Synthetic => "synthetic",
        }
    }
}

/// Final result of an executor invocation. The scheduler persists this
/// as the attempt + job terminal state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorCompletion {
    pub status: ExecutorStatus,
    pub summary: String,
    pub run_id: Option<RunId>,
    pub metrics: ExecutorMetrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorStatus {
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Interrupted,
}

/// Context passed to the executor's `execute` method. Includes the
/// full `JobRecord`, the freshly created attempt id, the workspace
/// services lease, the admission permit guard (drops on completion),
/// and a cancellation token. The guard is intentionally passed
/// through so the executor is the single owner of permit release
/// timing (it must drop the guard only after the executor has fully
/// stopped, e.g. process-group cleanup finished).
pub struct JobExecutionContext {
    pub job: JobRecord,
    pub attempt_id: String,
    pub workspace_id: WorkspaceId,
    pub cancellation: CancellationToken,
    pub progress: Arc<dyn JobProgressSink>,
    pub resources: ResourcePermitGuard,
}

impl JobExecutionContext {
    pub fn job_id(&self) -> &JobId {
        &self.job.job_id
    }
}

/// Progress sink that the executor publishes to. The default
/// `NoopProgressSink` discards; production wiring plugs a bus sink
/// into the TUI / event log.
#[async_trait]
pub trait JobProgressSink: Send + Sync {
    async fn progress(&self, _job_id: &JobId, _message: &str) {}
}

pub struct NoopProgressSink;

#[async_trait]
impl JobProgressSink for NoopProgressSink {}

/// Typed executor contract. One implementation per `ExecutorKind`
/// (or per `JobKind` family). Validation is synchronous and cheap;
/// `execute` does the real work.
#[async_trait]
pub trait JobExecutor: Send + Sync {
    fn kind(&self) -> ExecutorKind;
    fn supports(&self, kind: JobKind) -> bool;
    fn validate(&self, _job: &JobRecord) -> Result<(), ExecutorValidationError> {
        Ok(())
    }
    async fn execute(&self, ctx: JobExecutionContext) -> ExecutorCompletion;
    fn health(&self) -> ExecutorHealth {
        ExecutorHealth::Healthy
    }
}

/// Registry of typed executors. The scheduler consults
/// `dispatch(&job)` to find the executor responsible for a given
/// `JobRecord`.
pub struct ExecutorRegistry {
    executors: HashMap<ExecutorKind, Arc<dyn JobExecutor>>,
    health: HashMap<ExecutorKind, ExecutorHealth>,
}

impl Default for ExecutorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutorRegistry {
    pub fn new() -> Self {
        Self {
            executors: HashMap::new(),
            health: HashMap::new(),
        }
    }

    /// Register an executor. Duplicate kinds are rejected so a
    /// misconfiguration surfaces immediately rather than silently
    /// overriding an existing executor.
    pub fn register(&mut self, exec: Arc<dyn JobExecutor>) -> Result<(), ExecutorRegistryError> {
        let kind = exec.kind();
        if self.executors.contains_key(&kind) {
            return Err(ExecutorRegistryError::Duplicate(kind));
        }
        self.health.insert(kind, exec.health());
        self.executors.insert(kind, exec);
        Ok(())
    }

    pub fn get(&self, kind: ExecutorKind) -> Option<Arc<dyn JobExecutor>> {
        self.executors.get(&kind).map(Arc::clone)
    }

    pub fn for_job(&self, job: &JobRecord) -> Option<Arc<dyn JobExecutor>> {
        let kind = executor_kind_for_job(job)?;
        self.get(kind)
    }

    pub fn kinds(&self) -> Vec<ExecutorKind> {
        let mut v: Vec<ExecutorKind> = self.executors.keys().copied().collect();
        v.sort_by_key(|k| k.as_str());
        v
    }

    pub fn health_snapshot(&self) -> Vec<(ExecutorKind, ExecutorHealth)> {
        let mut v: Vec<(ExecutorKind, ExecutorHealth)> = self
            .executors
            .iter()
            .map(|(k, e)| (*k, e.health()))
            .collect();
        v.sort_by_key(|(k, _)| k.as_str());
        v
    }
}

/// Errors raised by the registry.
#[derive(Debug, Error)]
pub enum ExecutorRegistryError {
    #[error("executor already registered: {0:?}")]
    Duplicate(ExecutorKind),
    #[error("no executor for job kind '{kind}'", kind = .0.as_str())]
    Unsupported(JobKind),
}

/// Map a `JobRecord` to its canonical `ExecutorKind`.
///
/// This is the central place where `JobKind` -> executor dispatch is
/// decided. The migration plan in §8-§10 of the Phase 5 roadmap
/// brings Test, ManagedArgv (Build/Lint/Format), and Subagent online
/// first.
pub fn executor_kind_for_job(job: &JobRecord) -> Option<ExecutorKind> {
    match (job.kind, executor_variant(&job.payload)) {
        (JobKind::Test, _) => Some(ExecutorKind::Test),
        (JobKind::Build, _) | (JobKind::Lint, _) | (JobKind::Format, _) => {
            Some(ExecutorKind::ManagedArgv)
        }
        (JobKind::Subagent, _) => Some(ExecutorKind::Subagent),
        // The bash-dispatch path uses TestRunner but with a
        // BashDispatch payload, so the executor is distinct.
        (_, PayloadVariant::BashDispatch) => Some(ExecutorKind::BashDispatch),
        _ => None,
    }
}

/// Discriminator for a `JobPayload` variant. Kept internal so the
/// dispatcher can treat `Test` and `BashDispatch` distinctly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum PayloadVariant {
    Test,
    BashDispatch,
    ManagedArgv,
    Subagent,
    Other,
}

pub(crate) fn executor_variant(payload: &codegg_core::jobs::JobPayload) -> PayloadVariant {
    use codegg_core::jobs::JobPayload;
    match payload {
        JobPayload::Test { .. } => PayloadVariant::Test,
        JobPayload::ManagedArgv { .. } => PayloadVariant::ManagedArgv,
        JobPayload::Subagent { .. } => PayloadVariant::Subagent,
        _ => PayloadVariant::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::jobs::{
        IdempotencyClass, JobId, JobKind, JobPayload, JobPriority, JobSource, JobState,
        ResourceRequest, RetryPolicy,
    };
    use codegg_core::workspace::WorkspaceId;
    use std::collections::HashMap;
    use std::sync::Arc;

    struct AlwaysAvailable;

    #[async_trait]
    impl JobExecutor for AlwaysAvailable {
        fn kind(&self) -> ExecutorKind {
            ExecutorKind::Synthetic
        }
        fn supports(&self, _kind: JobKind) -> bool {
            true
        }
        async fn execute(&self, _ctx: JobExecutionContext) -> ExecutorCompletion {
            ExecutorCompletion {
                status: ExecutorStatus::Completed,
                summary: "ok".into(),
                run_id: None,
                metrics: ExecutorMetrics::default(),
            }
        }
    }

    #[test]
    fn register_and_lookup() {
        let mut r = ExecutorRegistry::new();
        r.register(Arc::new(AlwaysAvailable)).unwrap();
        assert!(r.get(ExecutorKind::Synthetic).is_some());
    }

    #[test]
    fn duplicate_registration_rejected() {
        let mut r = ExecutorRegistry::new();
        r.register(Arc::new(AlwaysAvailable)).unwrap();
        let err = r.register(Arc::new(AlwaysAvailable)).unwrap_err();
        assert!(matches!(err, ExecutorRegistryError::Duplicate(_)));
    }

    #[test]
    fn for_job_routes_test() {
        let job = sample_job(
            JobKind::Test,
            JobPayload::Test {
                command: "cargo test".into(),
                argv: vec!["cargo".into(), "test".into()],
                cwd: None,
                scope: None,
            },
        );
        let r = ExecutorRegistry::new();
        let exec = r.for_job(&job);
        assert!(exec.is_none(), "empty registry returns None");
    }

    fn sample_job(kind: JobKind, payload: JobPayload) -> JobRecord {
        let now = chrono::Utc::now();
        JobRecord {
            job_id: JobId::new_unchecked("j1"),
            workspace_id: WorkspaceId::new_unchecked("ws1"),
            session_id: None,
            turn_id: None,
            kind,
            source: JobSource::Interactive,
            priority: JobPriority::Normal,
            payload,
            resource_request: ResourceRequest::default(),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            state: JobState::Queued,
            current_attempt_id: None,
            attempt_count: 0,
            not_before: None,
            deadline: None,
            schedule_id: None,
            created_at: now,
            updated_at: now,
            terminal_at: None,
            cancel_requested_at: None,
            cancel_reason: None,
            depends_on: vec![],
            labels: HashMap::new(),
        }
    }
}
