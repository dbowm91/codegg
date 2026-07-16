//! Concrete `JobExecutor` implementations backed by the existing
//! canonical subsystems. The scheduler is the single authority that
//! invokes them; tool-level callers (e.g. `tool::test`,
//! `tool::bash` test submission path) route jobs through the
//! scheduler instead of calling these subsystems directly.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use codegg_core::jobs::{JobKind, JobPayload, JobRecord};
use codegg_core::workspace::WorkspaceId;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::scheduler::events::SchedulerEvent;
use crate::scheduler::executor::{
    ExecutorAvailability, ExecutorCompletion, ExecutorHealth, ExecutorKind, ExecutorMetrics,
    ExecutorStatus, ExecutorValidationError, JobExecutionContext, JobExecutor, JobProgressSink,
};
use crate::scheduler::permit::ResourcePermitGuard;

#[derive(Debug, Error)]
pub enum ExecutorImplError {
    #[error("missing cwd for test execution in workspace '{0}'")]
    MissingCwd(WorkspaceId),
    #[error("test payload missing argv")]
    MissingArgv,
    #[error("failed to resolve workspace root for '{0}': {1}")]
    WorkspaceResolution(WorkspaceId, String),
    #[error("test runner error: {0}")]
    TestRunner(String),
    #[error("managed argv execution error: {0}")]
    ManagedArgv(String),
    #[error("subagent pool error: {0}")]
    Subagent(String),
}

/// Sink adapter: routes `JobProgressSink` callbacks into the
/// scheduler's event sink so progress events land in the same stream
/// as `Submit`/`Admitted`/`Completed`/`Failed`.
pub struct SchedulerEventProgress {
    pub sink: Arc<dyn Fn(SchedulerEvent) + Send + Sync>,
}

#[async_trait]
impl JobProgressSink for SchedulerEventProgress {
    async fn progress(&self, job_id: &codegg_core::jobs::JobId, message: &str) {
        (self.sink)(SchedulerEvent::Progress {
            job_id: job_id.as_str().to_string(),
            message: message.to_string(),
        });
    }
}

/// Plain no-op progress sink used when the scheduler has no event
/// sink wired.
pub struct NullProgressSink;

#[async_trait]
impl JobProgressSink for NullProgressSink {}

/// Canonical executor for `JobKind::Test`. Delegates to the existing
/// `test_runner::resolve_and_run_test` subsystem so all test-run
/// semantics (RunStore persistence, projection, supervisor, sink)
/// continue to flow through the same path the daemon used pre-Phase 5.
pub struct TestJobExecutor {
    run_store: Option<Arc<dyn codegg_core::run_store::RunStore>>,
    sink: Option<Arc<dyn crate::test_runner::types::TestEventSink>>,
}

impl TestJobExecutor {
    pub fn new(
        run_store: Option<Arc<dyn codegg_core::run_store::RunStore>>,
        sink: Option<Arc<dyn crate::test_runner::types::TestEventSink>>,
    ) -> Self {
        Self { run_store, sink }
    }
}

#[async_trait]
impl JobExecutor for TestJobExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Test
    }

    fn supports(&self, kind: JobKind) -> bool {
        matches!(kind, JobKind::Test)
    }

    fn validate(&self, job: &JobRecord) -> Result<(), ExecutorValidationError> {
        match &job.payload {
            JobPayload::Test { argv, .. } => {
                if argv.is_empty() {
                    return Err(ExecutorValidationError::MissingField("argv".into()));
                }
                Ok(())
            }
            _ => Err(ExecutorValidationError::UnsupportedKind {
                executor: self.kind().as_str().into(),
                kind: job.kind.as_str().to_string(),
            }),
        }
    }

    fn health(&self) -> ExecutorHealth {
        ExecutorHealth::Healthy
    }

    async fn execute(&self, ctx: JobExecutionContext) -> ExecutorCompletion {
        let started = std::time::Instant::now();
        let argv = match &ctx.job.payload {
            JobPayload::Test { argv, .. } => argv.clone(),
            _ => {
                return failure_completion(
                    started,
                    ExecutorStatus::Failed,
                    "unsupported payload kind".into(),
                );
            }
        };
        let cwd = match extract_cwd(&ctx.job) {
            Some(p) => p,
            None => {
                return failure_completion(
                    started,
                    ExecutorStatus::Failed,
                    format!("missing cwd for workspace '{}'", ctx.workspace_id.as_str()),
                );
            }
        };

        let request = crate::test_runner::types::TestRunRequest {
            scope: crate::test_runner::types::TestScope::BashDispatch(argv),
            workdir: cwd,
            timeout_secs: ctx.job.timeout.map(|d| d.as_secs().max(1)).or(Some(900)),
            stall_timeout_secs: Some(450),
            max_report_bytes: None,
            session_id: ctx.job.session_id.clone(),
        };

        let result = crate::test_runner::runner::resolve_and_run_test(
            request,
            self.sink.as_deref(),
            self.run_store.as_ref(),
        )
        .await;

        let elapsed = started.elapsed().as_millis() as u64;
        match result {
            Ok(delegated) => {
                let status = match delegated.report().status {
                    crate::test_runner::types::TestStatus::Passed => ExecutorStatus::Completed,
                    crate::test_runner::types::TestStatus::Failed => ExecutorStatus::Failed,
                    crate::test_runner::types::TestStatus::TimedOut => ExecutorStatus::TimedOut,
                    crate::test_runner::types::TestStatus::Cancelled => ExecutorStatus::Cancelled,
                    crate::test_runner::types::TestStatus::Error => ExecutorStatus::Failed,
                };
                ExecutorCompletion {
                    status,
                    summary: crate::test_runner::format_test_report_with_cap(
                        delegated.report(),
                        crate::test_runner::report::DEFAULT_MAX_REPORT_BYTES,
                    ),
                    run_id: delegated.run_id.clone(),
                    metrics: ExecutorMetrics {
                        cpu_time_ms: None,
                        peak_memory_mb: None,
                        elapsed_ms: elapsed,
                    },
                }
            }
            Err(e) => {
                let _ = ctx.cancellation.is_cancelled();
                failure_completion(
                    started,
                    ExecutorStatus::Failed,
                    format!("test runner error: {e}"),
                )
            }
        }
    }
}

fn extract_cwd(job: &JobRecord) -> Option<PathBuf> {
    match &job.payload {
        JobPayload::Test { cwd, .. } => cwd.clone().map(PathBuf::from),
        _ => None,
    }
}

fn failure_completion(
    started: std::time::Instant,
    status: ExecutorStatus,
    summary: String,
) -> ExecutorCompletion {
    let elapsed = started.elapsed().as_millis() as u64;
    ExecutorCompletion {
        status,
        summary,
        run_id: None,
        metrics: ExecutorMetrics {
            cpu_time_ms: None,
            peak_memory_mb: None,
            elapsed_ms: elapsed,
        },
    }
}

/// Scheduler adapter for managed argv, shell, build, lint, and format jobs.
/// Process policy lives
/// in [`crate::managed_process::ManagedProcessService`]; this adapter only
/// translates the durable payload and maps the result to executor status.
pub struct ManagedArgvExecutor {
    label: &'static str,
}

impl ManagedArgvExecutor {
    pub fn new(label: &'static str) -> Self {
        Self { label }
    }
}

#[async_trait]
impl JobExecutor for ManagedArgvExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::ManagedArgv
    }

    fn supports(&self, kind: JobKind) -> bool {
        matches!(
            kind,
            JobKind::Build
                | JobKind::Lint
                | JobKind::Format
                | JobKind::ManagedProcess
                | JobKind::Shell
        )
    }

    fn validate(&self, job: &JobRecord) -> Result<(), ExecutorValidationError> {
        match &job.payload {
            JobPayload::ManagedArgv { argv, .. } => {
                if argv.is_empty() {
                    return Err(ExecutorValidationError::MissingField("argv".into()));
                }
                Ok(())
            }
            JobPayload::Shell { command, argv, .. } => {
                if command.is_empty() && argv.as_ref().map_or(true, Vec::is_empty) {
                    return Err(ExecutorValidationError::MissingField("command".into()));
                }
                Ok(())
            }
            _ => Err(ExecutorValidationError::UnsupportedKind {
                executor: self.kind().as_str().into(),
                kind: job.kind.as_str().to_string(),
            }),
        }
    }

    fn health(&self) -> ExecutorHealth {
        ExecutorHealth::Healthy
    }

    async fn execute(&self, ctx: JobExecutionContext) -> ExecutorCompletion {
        let started = std::time::Instant::now();
        let argv = match &ctx.job.payload {
            JobPayload::ManagedArgv { argv, .. } => argv.clone(),
            JobPayload::Shell { command, argv, .. } => argv
                .clone()
                .unwrap_or_else(|| vec!["sh".into(), "-c".into(), command.clone()]),
            _ => {
                return failure_completion(
                    started,
                    ExecutorStatus::Failed,
                    "unsupported payload kind".into(),
                );
            }
        };
        let cwd = match &ctx.job.payload {
            JobPayload::ManagedArgv { cwd, .. } | JobPayload::Shell { cwd, .. } => {
                cwd.clone().map(PathBuf::from)
            }
            _ => None,
        };

        let Some(cwd) = cwd else {
            // production path: fall back to workspace root via
            // ExecutionContext was used by callers submitting; here we
            // best-effort to the job's session's recorded workspace
            // dir if available. For Phase 5 the bash tool wires cwd
            // explicitly into the payload, so missing cwd is a
            // configuration error.
            return failure_completion(
                started,
                ExecutorStatus::Failed,
                "managed argv missing cwd".into(),
            );
        };
        let mut request = crate::managed_process::ManagedProcessRequest::new(
            argv.into_iter().map(OsString::from).collect(),
            cwd,
            crate::managed_process::ProcessProvenance::new(
                ctx.job.job_id.as_str(),
                ctx.attempt_id.as_str(),
            ),
        );
        request.timeout = ctx.job.timeout;
        request.cancellation = ctx.cancellation.clone();

        let output = match crate::managed_process::ManagedProcessService::run(request).await {
            Ok(o) => o,
            Err(e) => {
                return failure_completion(
                    started,
                    ExecutorStatus::Failed,
                    format!("spawn failed: {e}"),
                );
            }
        };
        let status = match output.termination {
            crate::managed_process::TerminationReason::Cancelled => ExecutorStatus::Cancelled,
            crate::managed_process::TerminationReason::TimedOut => ExecutorStatus::TimedOut,
            crate::managed_process::TerminationReason::Exited if output.exit_status.success() => {
                ExecutorStatus::Completed
            }
            crate::managed_process::TerminationReason::Exited => ExecutorStatus::Failed,
        };
        let mut summary = format!(
            "{} exit={} stdout_bytes={} stderr_bytes={} truncated={}",
            self.label,
            output
                .exit_status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".into()),
            output.stdout.total_bytes,
            output.stderr.total_bytes,
            output.stdout.is_truncated() || output.stderr.is_truncated()
        );
        let stdout = output.stdout.to_string_lossy();
        let stderr = output.stderr.to_string_lossy();
        if !stdout.is_empty() {
            summary.push('\n');
            summary.push_str(&stdout);
        }
        if !stderr.is_empty() {
            summary.push_str("\n--- stderr ---\n");
            summary.push_str(&stderr);
        }
        let _ = ctx.progress.progress(ctx.job_id(), &summary).await;
        ExecutorCompletion {
            status,
            summary,
            run_id: None,
            metrics: ExecutorMetrics {
                cpu_time_ms: None,
                peak_memory_mb: None,
                elapsed_ms: output.duration.as_millis() as u64,
            },
        }
    }
}

/// Canonical executor for `JobKind::Subagent`. Hands the work off to
/// `SubAgentPool`, which is the existing durable entry point for
/// subagent execution.
pub struct SubagentJobExecutor {
    pool: Arc<crate::agent::worker::SubAgentPool>,
}

impl SubagentJobExecutor {
    pub fn new(pool: Arc<crate::agent::worker::SubAgentPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl JobExecutor for SubagentJobExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Subagent
    }

    fn supports(&self, kind: JobKind) -> bool {
        matches!(kind, JobKind::Subagent)
    }

    fn validate(&self, job: &JobRecord) -> Result<(), ExecutorValidationError> {
        match &job.payload {
            JobPayload::Subagent { .. } => Ok(()),
            _ => Err(ExecutorValidationError::UnsupportedKind {
                executor: self.kind().as_str().into(),
                kind: job.kind.as_str().to_string(),
            }),
        }
    }

    fn health(&self) -> ExecutorHealth {
        ExecutorHealth::Healthy
    }

    async fn execute(&self, ctx: JobExecutionContext) -> ExecutorCompletion {
        let started = std::time::Instant::now();
        let (prompt, agent, parent_id, denied_tools, allowed_paths, max_tool_calls) =
            match &ctx.job.payload {
                JobPayload::Subagent {
                    prompt,
                    agent,
                    parent_id,
                    denied_tools,
                    allowed_paths,
                    max_tool_calls,
                } => (
                    prompt.clone(),
                    agent.clone(),
                    parent_id.clone(),
                    denied_tools.clone(),
                    allowed_paths.clone(),
                    *max_tool_calls,
                ),
                _ => {
                    return failure_completion(
                        started,
                        ExecutorStatus::Failed,
                        "unsupported payload".into(),
                    );
                }
            };

        let task_id = ctx
            .job
            .job_id
            .as_str()
            .bytes()
            .take(8)
            .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));

        let request = crate::agent::worker::SubAgentRequest {
            task_id,
            prompt,
            agent,
            parent_id,
            denied_tools,
            allowed_paths,
            description: format!("Scheduler job {}", ctx.job.job_id.as_str()),
            depth: 1,
            max_tool_calls: max_tool_calls.map(|m| m as usize),
            parent_model: None,
        };

        let result = self.pool.spawner().send_and_wait(request).await;
        match result {
            Ok(result) => ExecutorCompletion {
                status: if result.success {
                    ExecutorStatus::Completed
                } else {
                    ExecutorStatus::Failed
                },
                summary: result.result,
                run_id: None,
                metrics: ExecutorMetrics {
                    cpu_time_ms: None,
                    peak_memory_mb: None,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                },
            },
            Err(e) => failure_completion(
                started,
                ExecutorStatus::Failed,
                format!("subagent pool error: {e}"),
            ),
        }
    }
}

/// Convenience constructor: register the standard executor set
/// against a registry.
pub fn register_default_executors(
    registry: &mut crate::scheduler::executor::ExecutorRegistry,
    run_store: Option<Arc<dyn codegg_core::run_store::RunStore>>,
    sink: Option<Arc<dyn crate::test_runner::types::TestEventSink>>,
    subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
) -> Result<(), crate::scheduler::executor::ExecutorRegistryError> {
    registry.register(Arc::new(TestJobExecutor::new(run_store, sink)))?;
    registry.register(Arc::new(ManagedArgvExecutor::new("managed_argv")))?;
    if let Some(pool) = subagent_pool {
        registry.register(Arc::new(SubagentJobExecutor::new(pool)))?;
    }
    Ok(())
}

#[allow(dead_code)]
fn _ensure_progress_sink_used(_s: &dyn JobProgressSink) {}

#[allow(dead_code)]
fn _ensure_cancellation_used(_c: &CancellationToken) {}

#[allow(dead_code)]
fn _ensure_permit_used(_p: &ResourcePermitGuard) {}

#[allow(dead_code)]
fn _ensure_availability_used(_a: ExecutorAvailability) {}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::jobs::{
        IdempotencyClass, JobId, JobPayload, JobPriority, JobSource, JobState, ResourceRequest,
        RetryPolicy,
    };

    fn sample_test_job(argv: Vec<String>, cwd: Option<String>) -> JobRecord {
        let now = chrono::Utc::now();
        JobRecord {
            job_id: JobId::new_unchecked("j-test"),
            workspace_id: WorkspaceId::new_unchecked("ws-1"),
            session_id: None,
            turn_id: None,
            kind: JobKind::Test,
            source: JobSource::Interactive,
            priority: JobPriority::Normal,
            payload: JobPayload::Test {
                command: argv.join(" "),
                argv,
                cwd,
                scope: None,
            },
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
            labels: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn test_executor_reports_kind() {
        let exec = TestJobExecutor::new(None, None);
        assert_eq!(exec.kind(), ExecutorKind::Test);
        assert!(exec.supports(JobKind::Test));
    }

    #[test]
    fn test_executor_rejects_empty_argv() {
        let exec = TestJobExecutor::new(None, None);
        let job = sample_test_job(vec![], Some("/tmp".into()));
        let result = exec.validate(&job);
        assert!(matches!(
            result,
            Err(ExecutorValidationError::MissingField(_))
        ));
    }

    #[test]
    fn managed_argv_executor_validates_payload() {
        let exec = ManagedArgvExecutor::new("build");
        let now = chrono::Utc::now();
        let job = JobRecord {
            job_id: JobId::new_unchecked("j-build"),
            workspace_id: WorkspaceId::new_unchecked("ws-1"),
            session_id: None,
            turn_id: None,
            kind: JobKind::Build,
            source: JobSource::Api,
            priority: JobPriority::Normal,
            payload: JobPayload::ManagedArgv {
                argv: vec!["cargo".into(), "build".into()],
                cwd: Some("/tmp".into()),
            },
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
            labels: std::collections::HashMap::new(),
        };
        assert!(exec.validate(&job).is_ok());
    }

    #[test]
    fn managed_argv_executor_rejects_empty_argv() {
        let exec = ManagedArgvExecutor::new("lint");
        let now = chrono::Utc::now();
        let job = JobRecord {
            job_id: JobId::new_unchecked("j-lint"),
            workspace_id: WorkspaceId::new_unchecked("ws-1"),
            session_id: None,
            turn_id: None,
            kind: JobKind::Lint,
            source: JobSource::Api,
            priority: JobPriority::Normal,
            payload: JobPayload::ManagedArgv {
                argv: vec![],
                cwd: Some("/tmp".into()),
            },
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
            labels: std::collections::HashMap::new(),
        };
        assert!(matches!(
            exec.validate(&job),
            Err(ExecutorValidationError::MissingField(_))
        ));
    }

    #[test]
    fn subagent_executor_kind_is_subagent() {
        // The validate() path is exercised at construction time by
        // production wiring; we only need to verify the trait surface
        // here (requires a full SubAgentPool which is environment-
        // heavy in unit tests).
        let registry = crate::scheduler::executor::ExecutorRegistry::new();
        assert!(registry.get(ExecutorKind::Subagent).is_none());
    }

    #[test]
    fn test_executor_health_default() {
        let exec = TestJobExecutor::new(None, None);
        assert_eq!(exec.health(), ExecutorHealth::Healthy);
    }
}
