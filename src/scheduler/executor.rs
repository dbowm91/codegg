//! Typed `JobExecutor` trait and registry.
//!
//! The scheduler is executor-agnostic: it walks the fair queue, asks
//! the admission controller for permits, then calls into a typed
//! executor to actually run the work. The trait intentionally does
//! not expose shell; executors are responsible for invoking existing
//! subsystems (TestRunner, managed argv, subagent pool) and must not
//! reconstruct shell from the typed payload.

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use async_trait::async_trait;
use codegg_core::jobs::{AttemptId, DaemonGeneration, ExecutionTarget, JobId, JobKind, JobRecord};
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
    Python,
    ToolProgram,
    AgentTurn,
    Synthetic,
    /// Fixed-target remote execution on one named Eggwork node. Reached
    /// only for `ExecutionTarget::EggworkNode` jobs; never for `Local`.
    Eggwork,
}

impl ExecutorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecutorKind::Test => "test",
            ExecutorKind::ManagedArgv => "managed_argv",
            ExecutorKind::Subagent => "subagent",
            ExecutorKind::BashDispatch => "bash_dispatch",
            ExecutorKind::Python => "python",
            ExecutorKind::ToolProgram => "tool_program",
            ExecutorKind::AgentTurn => "agent_turn",
            ExecutorKind::Synthetic => "synthetic",
            ExecutorKind::Eggwork => "eggwork",
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
    pub attempt_id: AttemptId,
    pub daemon_generation: DaemonGeneration,
    pub workspace_id: WorkspaceId,
    /// Canonical workspace root captured from the scheduler-owned lease.
    /// Executors must use this instead of consulting process-global CWD.
    pub workspace_root: std::path::PathBuf,
    /// Workspace-scoped artifact store captured by the scheduler lease.
    /// `None` is retained for lightweight executor unit tests; production
    /// scheduler dispatch always supplies it.
    pub run_store: Option<Arc<dyn codegg_core::run_store::RunStore>>,
    /// Durable attempt store and scheduler-captured source identity. Executors
    /// may seal only at a backend-specific immutable-input boundary.
    pub subject_store: Option<Arc<dyn codegg_core::jobs::JobStore>>,
    pub source_subject_started: Option<codegg_core::jobs::ExecutionSubjectProvenance>,
    pub cancellation: CancellationToken,
    pub progress: Arc<dyn JobProgressSink>,
    pub resources: ResourcePermitGuard,
}

impl JobExecutionContext {
    pub fn job_id(&self) -> &JobId {
        &self.job.job_id
    }

    /// Validate the provenance that must be present before a canonical
    /// executor may launch a process or agent. This is intentionally a
    /// release-mode check; debug assertions alone would make production
    /// bypasses invisible.
    pub fn validate_runtime(&self) -> Result<(), ExecutorValidationError> {
        if self.attempt_id.as_str().is_empty() {
            return Err(ExecutorValidationError::InvalidPayload(
                "missing attempt id".into(),
            ));
        }
        if self.daemon_generation.as_str().is_empty() {
            return Err(ExecutorValidationError::InvalidPayload(
                "missing daemon generation".into(),
            ));
        }
        if self.workspace_id.as_str().is_empty() {
            return Err(ExecutorValidationError::InvalidPayload(
                "missing workspace id".into(),
            ));
        }
        if !self.resources.is_controller_bound() {
            return Err(ExecutorValidationError::InvalidPayload(
                "execution does not own a live admission permit".into(),
            ));
        }
        Ok(())
    }

    /// Seal subject provenance immediately after a remote/materialized input
    /// has been assembled, before submission. Returns false on source drift.
    pub async fn seal_materialized_source_subject(
        &self,
        manifest_digest: String,
        skipped_non_regular: usize,
        skipped_oversize: usize,
    ) -> Result<bool, String> {
        use codegg_core::jobs::{
            ExecutionSubjectDisposition as D, ExecutionSubjectKind as K,
            ExecutionSubjectMaterialization as M, ExecutionSubjectProvenance as P,
            ExecutionSubjectRevision as R, ExecutionSubjectSealKind as S,
            ExecutionSubjectState as T,
        };
        let Some(started) = self.source_subject_started.as_ref() else {
            return Err("missing started subject provenance".into());
        };
        let end = egggit::capture_git_source_subject(&self.workspace_root)
            .await
            .ok()
            .map(|subject| R {
                schema_version: R::SCHEMA_VERSION,
                subject_kind: K::Git,
                repository_identity: format!("codegg-workspace:{}", self.workspace_id.as_str()),
                revision: subject.revision,
                state: if subject.dirty_digest.is_some() {
                    T::Dirty
                } else {
                    T::Clean
                },
                dirty_digest: subject.dirty_digest,
            });
        let equal = started
            .captured
            .as_ref()
            .zip(end.as_ref())
            .is_some_and(|(a, b)| a == b);
        let capture_failed = end.is_none();
        let skipped_non_regular = skipped_non_regular.min(u32::MAX as usize) as u32;
        let skipped_oversize = skipped_oversize.min(u32::MAX as usize) as u32;
        let complete = skipped_non_regular == 0 && skipped_oversize == 0;
        let sealed = P {
            schema_version: 1,
            captured: started.captured.clone(),
            sealed: end,
            disposition: if equal && complete {
                D::Stable
            } else if started.captured.is_some() {
                if equal {
                    D::Unavailable
                } else {
                    D::Drifted
                }
            } else {
                D::Unavailable
            },
            seal_kind: S::SnapshotMaterialized,
            unavailable_reason: if equal && complete {
                None
            } else if equal {
                Some(
                    codegg_core::jobs::ExecutionSubjectUnavailableReason::MaterializationIncomplete,
                )
            } else if started.captured.is_none() {
                started.unavailable_reason
            } else if capture_failed {
                Some(codegg_core::jobs::ExecutionSubjectUnavailableReason::CaptureFailed)
            } else {
                None
            },
            materialization: Some(M {
                manifest_digest,
                complete,
                skipped_non_regular,
                skipped_oversize,
            }),
        };
        let store = self
            .subject_store
            .as_ref()
            .ok_or_else(|| "attempt store unavailable for source-subject seal".to_string())?;
        store
            .seal_attempt_source_subject(&self.attempt_id, &sealed)
            .await
            .map_err(|e| e.to_string())?;
        Ok(equal)
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
    executors: HashMap<ExecutorKind, ExecutorRecord>,
    health: HashMap<ExecutorKind, ExecutorHealth>,
}

#[derive(Clone)]
pub(crate) struct ExecutorStats {
    pub(crate) total_invocations: Arc<AtomicU64>,
    pub(crate) total_failures: Arc<AtomicU64>,
}

struct ExecutorRecord {
    executor: Arc<dyn JobExecutor>,
    stats: ExecutorStats,
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
        self.executors.insert(
            kind,
            ExecutorRecord {
                executor: exec,
                stats: ExecutorStats {
                    total_invocations: Arc::new(AtomicU64::new(0)),
                    total_failures: Arc::new(AtomicU64::new(0)),
                },
            },
        );
        Ok(())
    }

    pub fn get(&self, kind: ExecutorKind) -> Option<Arc<dyn JobExecutor>> {
        self.executors
            .get(&kind)
            .map(|record| Arc::clone(&record.executor))
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
            .map(|(k, record)| (*k, record.executor.health()))
            .collect();
        v.sort_by_key(|(k, _)| k.as_str());
        v
    }

    pub(crate) fn stats(&self, kind: ExecutorKind) -> Option<ExecutorStats> {
        self.executors.get(&kind).map(|record| record.stats.clone())
    }

    pub(crate) fn health_snapshot_with_stats(
        &self,
    ) -> Vec<(ExecutorKind, ExecutorHealth, ExecutorStats)> {
        let mut v: Vec<_> = self
            .executors
            .iter()
            .map(|(kind, record)| (*kind, record.executor.health(), record.stats.clone()))
            .collect();
        v.sort_by_key(|(kind, _, _)| kind.as_str());
        v
    }
}

/// Errors raised by the registry.
#[derive(Debug, Error)]
pub enum ExecutorRegistryError {
    #[error("executor already registered: {0:?}")]
    Duplicate(ExecutorKind),
    #[error("executor registry is busy during synchronous registration")]
    Busy,
    #[error("no executor for job kind '{kind}'", kind = .0.as_str())]
    Unsupported(JobKind),
}

/// Map a `JobRecord` to its canonical `ExecutorKind`.
///
/// This is the central place where `JobKind` -> executor dispatch is
/// decided. ManagedArgv is also the bounded process adapter for shell and
/// generic managed-process jobs; it does not make those jobs unscheduled.
///
/// The execution target is evaluated before the kind: any job explicitly
/// targeted at an Eggwork node routes to the Eggwork executor (which
/// validates eligibility itself and never falls back to local execution).
/// `Local` jobs keep the pre-existing mapping unchanged.
pub fn executor_kind_for_job(job: &JobRecord) -> Option<ExecutorKind> {
    if matches!(job.target, ExecutionTarget::EggworkNode { .. }) {
        return Some(ExecutorKind::Eggwork);
    }
    match (job.kind, executor_variant(&job.payload)) {
        (JobKind::Test, _) => Some(ExecutorKind::Test),
        (JobKind::AgentTurn, _) => Some(ExecutorKind::AgentTurn),
        (JobKind::Build, _) | (JobKind::Lint, _) | (JobKind::Format, _) => {
            Some(ExecutorKind::ManagedArgv)
        }
        (JobKind::ManagedProcess, _) | (JobKind::Shell, _) => Some(ExecutorKind::ManagedArgv),
        (JobKind::Subagent, _) => Some(ExecutorKind::Subagent),
        (JobKind::Python, _) => Some(ExecutorKind::Python),
        (JobKind::ToolProgram, _) => Some(ExecutorKind::ToolProgram),
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
    Python,
    Other,
}

pub(crate) fn executor_variant(payload: &codegg_core::jobs::JobPayload) -> PayloadVariant {
    use codegg_core::jobs::JobPayload;
    match payload {
        JobPayload::Test { .. } => PayloadVariant::Test,
        JobPayload::ManagedArgv { .. } => PayloadVariant::ManagedArgv,
        JobPayload::Subagent { .. } => PayloadVariant::Subagent,
        JobPayload::Python { .. } => PayloadVariant::Python,
        _ => PayloadVariant::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::jobs::{
        ExecutionTarget, IdempotencyClass, JobId, JobKind, JobPayload, JobPriority, JobSource,
        JobState, ResourceRequest, RetryPolicy,
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
    fn runtime_validation_rejects_empty_workspace_id() {
        let context = JobExecutionContext {
            job: sample_job(
                JobKind::Build,
                JobPayload::ManagedArgv {
                    argv: vec!["cargo".into(), "check".into()],
                    cwd: None,
                },
            ),
            attempt_id: codegg_core::jobs::AttemptId::new_unchecked("attempt"),
            daemon_generation: codegg_core::jobs::DaemonGeneration::new_unchecked("generation"),
            workspace_id: WorkspaceId::new_unchecked(""),
            workspace_root: std::path::PathBuf::from("/tmp"),
            run_store: None,
            subject_store: None,
            source_subject_started: None,
            cancellation: tokio_util::sync::CancellationToken::new(),
            progress: Arc::new(NoopProgressSink),
            resources: crate::scheduler::permit::ResourcePermitGuard::new_orphan(
                crate::scheduler::permit::PermitDimensions::default(),
            ),
        };

        assert!(matches!(
            context.validate_runtime(),
            Err(ExecutorValidationError::InvalidPayload(message))
                if message == "missing workspace id"
        ));
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
                parent_run_id: None,
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
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            target: ExecutionTarget::default(),
        }
    }
}
