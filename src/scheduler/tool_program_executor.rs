//! Scheduler executor for Tool Programs (M005).
//!
//! Loads verified IR, creates a [`MeteredInterpreter`], and runs it
//! through the scheduler's admission-controlled execution path with
//! cancellation, heartbeat, and typed terminal results.

use std::sync::Arc;

use async_trait::async_trait;
use codegg_core::jobs::{JobKind, JobPayload, JobRecord};
use codegg_core::tool_program::{
    BrokerCallback, BudgetSnapshot, CallRequest, CallResult, InterpreterError, MeteredInterpreter,
    ProgramStatus, ProgramValue, RunConfig, RuntimeLimits,
};

use crate::scheduler::executor::{
    ExecutorCompletion, ExecutorKind, ExecutorMetrics, ExecutorStatus, ExecutorValidationError,
    JobExecutionContext, JobExecutor,
};
use crate::tool::broker::{BrokerInvocationContext, ToolBroker};
use crate::tool::ToolRegistry;

/// In-process fixture broker — retained for backward-compat tests only.
///
/// Returns a deterministic `ToolResult` for any tool call. Production
/// code should use [`BrokerAdapter`] with the real [`ToolBroker`].
#[cfg(test)]
pub struct FixtureBroker {
    heartbeat_count: std::sync::atomic::AtomicU32,
}

#[cfg(test)]
impl FixtureBroker {
    pub fn new() -> Self {
        Self {
            heartbeat_count: std::sync::atomic::AtomicU32::new(0),
        }
    }

    pub fn heartbeat_count(&self) -> u32 {
        self.heartbeat_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
impl Default for FixtureBroker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[async_trait]
impl BrokerCallback for FixtureBroker {
    async fn execute_call(&self, request: &CallRequest) -> Result<CallResult, InterpreterError> {
        // Return a deterministic result based on the tool name
        let output = serde_json::json!({
            "tool": request.tool_name,
            "status": "ok",
            "input": request.input,
        });
        Ok(CallResult {
            output: ProgramValue::ToolResult(output),
            artifacts: vec![],
        })
    }

    async fn submit_child_job(
        &self,
        request: &codegg_core::tool_program::child_job::ChildJobRequest,
    ) -> Result<codegg_core::tool_program::child_job::ChildJobResult, InterpreterError> {
        use codegg_core::tool_program::child_job::*;
        let details = match request.op {
            ChildJobOp::Test => ChildJobDetails::Test(TestJobResult {
                status: "passed".into(),
                framework: Some("fixture".into()),
                total: Some(1),
                passed: Some(1),
                failed: Some(0),
                skipped: Some(0),
                ..Default::default()
            }),
            ChildJobOp::Build => ChildJobDetails::Build(BuildJobResult {
                status: "success".into(),
                ..Default::default()
            }),
            ChildJobOp::Lint => ChildJobDetails::Lint(LintJobResult {
                status: "clean".into(),
                ..Default::default()
            }),
            ChildJobOp::Format => ChildJobDetails::Format(FormatJobResult {
                status: "clean".into(),
                ..Default::default()
            }),
        };
        Ok(ChildJobResult {
            success: true,
            exit_code: Some(0),
            duration_ms: 100,
            details,
            artifacts: vec![],
            error: None,
        })
    }

    async fn heartbeat(&self, _budget: &BudgetSnapshot) {
        self.heartbeat_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Scheduler executor for `JobKind::ToolProgram`.
///
/// Validates the program payload, loads and verifies IR, creates a
/// [`MeteredInterpreter`], and runs it with cancellation support.
pub struct ToolProgramExecutor {
    broker: Arc<ToolBroker>,
    registry: Arc<ToolRegistry>,
    submission: Option<Arc<crate::scheduler::submission::JobSubmissionService>>,
}

impl ToolProgramExecutor {
    pub fn new(broker: Arc<ToolBroker>, registry: Arc<ToolRegistry>) -> Self {
        Self {
            broker,
            registry,
            submission: None,
        }
    }

    pub fn with_submission(
        mut self,
        submission: Arc<crate::scheduler::submission::JobSubmissionService>,
    ) -> Self {
        self.submission = Some(submission);
        self
    }
}

impl Default for ToolProgramExecutor {
    fn default() -> Self {
        // Default creates with a minimal setup - only used in tests
        let registry = Arc::new(ToolRegistry::with_defaults());
        let broker = Arc::new(ToolBroker::new(&registry));
        Self {
            broker,
            registry,
            submission: None,
        }
    }
}

/// Adapter that bridges the interpreter's `BrokerCallback` to the
/// real `ToolBroker` pipeline for programmatic tool calls.
pub struct BrokerAdapter {
    broker: Arc<ToolBroker>,
    registry: Arc<ToolRegistry>,
    program_id: String,
    submission: Option<Arc<crate::scheduler::submission::JobSubmissionService>>,
    workspace_id: Option<codegg_core::workspace::WorkspaceId>,
    cwd: std::path::PathBuf,
}

impl BrokerAdapter {
    pub fn new(broker: Arc<ToolBroker>, registry: Arc<ToolRegistry>, program_id: String) -> Self {
        Self {
            broker,
            registry,
            program_id,
            submission: None,
            workspace_id: None,
            cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        }
    }

    pub fn with_submission(
        mut self,
        submission: Arc<crate::scheduler::submission::JobSubmissionService>,
    ) -> Self {
        self.submission = Some(submission);
        self
    }

    pub fn with_workspace_id(mut self, workspace_id: codegg_core::workspace::WorkspaceId) -> Self {
        self.workspace_id = Some(workspace_id);
        self
    }

    pub fn with_cwd(mut self, cwd: std::path::PathBuf) -> Self {
        self.cwd = cwd;
        self
    }
}

#[async_trait]
impl BrokerCallback for BrokerAdapter {
    async fn execute_call(&self, request: &CallRequest) -> Result<CallResult, InterpreterError> {
        let ctx = BrokerInvocationContext {
            caller: crate::tool::contract::ToolCaller::Program {
                program_id: self.program_id.clone(),
            },
            cwd: self.cwd.clone(),
            session_id: None,
            workspace_id: self.workspace_id.as_ref().map(|w| w.to_string()),
            agent_id: None,
            turn_id: None,
            job_id: None,
            attempt_id: None,
            permission_mode: None,
            timeout_ms: Some(30_000),
            submission_key: None,
            caller_authorized: true,
        };

        match self
            .broker
            .execute(
                &self.registry,
                &request.tool_name,
                request.input.clone(),
                ctx,
            )
            .await
        {
            Ok(result) => {
                let program_value = match result.value.value {
                    Some(v) => ProgramValue::ToolResult(v),
                    None => ProgramValue::ToolResult(
                        serde_json::json!({"display": result.value.display}),
                    ),
                };
                Ok(CallResult {
                    output: program_value,
                    artifacts: result
                        .value
                        .artifacts
                        .into_iter()
                        .map(|a| a.artifact_id)
                        .collect(),
                })
            }
            Err(e) => Err(InterpreterError::BrokerError(e.to_string())),
        }
    }

    async fn submit_child_job(
        &self,
        request: &codegg_core::tool_program::child_job::ChildJobRequest,
    ) -> Result<codegg_core::tool_program::child_job::ChildJobResult, InterpreterError> {
        use crate::scheduler::submission::SubmissionKey;
        use codegg_core::tool_program::child_job::*;

        let submission = self.submission.as_ref().ok_or_else(|| {
            InterpreterError::BrokerError("child job submission requires scheduler service".into())
        })?;

        let workspace_id = self.workspace_id.as_ref().ok_or_else(|| {
            InterpreterError::BrokerError("child job requires workspace_id".into())
        })?;

        // Build the NewJob based on operation type
        let (kind, payload, timeout) = match &request.config {
            ChildJobConfig::Test(cfg) => {
                let argv = vec!["cargo".into(), "test".into()];
                let timeout = cfg.timeout_secs.map(std::time::Duration::from_secs);
                (
                    codegg_core::jobs::JobKind::Test,
                    codegg_core::jobs::JobPayload::Test {
                        command: "cargo test".into(),
                        argv,
                        cwd: cfg.cwd.clone(),
                        scope: cfg.scope.clone(),
                    },
                    timeout,
                )
            }
            ChildJobConfig::Build(cfg) => {
                let argv = cfg
                    .argv
                    .clone()
                    .unwrap_or_else(|| vec!["cargo".into(), "build".into()]);
                let timeout = cfg.timeout_secs.map(std::time::Duration::from_secs);
                (
                    codegg_core::jobs::JobKind::Build,
                    codegg_core::jobs::JobPayload::ManagedArgv {
                        argv,
                        cwd: cfg.cwd.clone(),
                    },
                    timeout,
                )
            }
            ChildJobConfig::Lint(cfg) => {
                let argv = cfg.argv.clone().unwrap_or_else(|| {
                    vec![
                        "cargo".into(),
                        "clippy".into(),
                        "--".into(),
                        "-D".into(),
                        "warnings".into(),
                    ]
                });
                let timeout = cfg.timeout_secs.map(std::time::Duration::from_secs);
                (
                    codegg_core::jobs::JobKind::Lint,
                    codegg_core::jobs::JobPayload::ManagedArgv {
                        argv,
                        cwd: cfg.cwd.clone(),
                    },
                    timeout,
                )
            }
            ChildJobConfig::Format(cfg) => {
                let argv = cfg.argv.clone().unwrap_or_else(|| {
                    vec!["cargo".into(), "fmt".into(), "--".into(), "--check".into()]
                });
                let timeout = cfg.timeout_secs.map(std::time::Duration::from_secs);
                (
                    codegg_core::jobs::JobKind::Format,
                    codegg_core::jobs::JobPayload::ManagedArgv {
                        argv,
                        cwd: cfg.cwd.clone(),
                    },
                    timeout,
                )
            }
        };

        // Create submission key for idempotency
        let config_hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(format!("{:?}", request.op).as_bytes());
            hasher.update(format!("{:?}", request.config).as_bytes());
            format!("{:x}", hasher.finalize())
        };
        let submission_key =
            SubmissionKey::new(format!("child-job:{}:{}", self.program_id, config_hash)).map_err(
                |e| InterpreterError::BrokerError(format!("invalid submission key: {}", e)),
            )?;

        let new_job = codegg_core::jobs::NewJob {
            workspace_id: workspace_id.clone(),
            session_id: None,
            turn_id: None,
            kind,
            source: codegg_core::jobs::JobSource::Interactive,
            priority: codegg_core::jobs::JobPriority::Normal,
            payload,
            resource_request: codegg_core::jobs::ResourceRequest::for_kind(kind),
            timeout,
            retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
            idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: vec![],
        };

        // Submit and wait
        let submitted = submission
            .submit(Some(submission_key), new_job)
            .await
            .map_err(|e| {
                InterpreterError::BrokerError(format!("child job submission failed: {}", e))
            })?;

        let wait_timeout = timeout
            .map(|d| d + std::time::Duration::from_secs(30))
            .unwrap_or_else(|| std::time::Duration::from_secs(300));

        let completion = submission
            .scheduler()
            .wait_for_completion(&submitted.job_id, wait_timeout)
            .await
            .map_err(|e| InterpreterError::BrokerError(format!("child job wait failed: {}", e)))?;

        // Map ExecutorStatus to ChildJobResult
        let success = matches!(
            completion.status,
            crate::scheduler::executor::ExecutorStatus::Completed
        );
        let exit_code = match completion.status {
            crate::scheduler::executor::ExecutorStatus::Completed => Some(0),
            crate::scheduler::executor::ExecutorStatus::Failed => Some(1),
            crate::scheduler::executor::ExecutorStatus::TimedOut => Some(124),
            crate::scheduler::executor::ExecutorStatus::Cancelled => Some(130),
            crate::scheduler::executor::ExecutorStatus::Interrupted => Some(1),
        };

        let is_cancelled = matches!(
            completion.status,
            crate::scheduler::executor::ExecutorStatus::Cancelled
        );
        let is_timed_out = matches!(
            completion.status,
            crate::scheduler::executor::ExecutorStatus::TimedOut
        );

        // Extract command string from config for result metadata
        let command_str = match &request.config {
            ChildJobConfig::Test(_) => Some("cargo test".into()),
            ChildJobConfig::Build(cfg) => cfg
                .argv
                .as_ref()
                .map(|a| a.join(" "))
                .or_else(|| Some("cargo build".into())),
            ChildJobConfig::Lint(cfg) => cfg
                .argv
                .as_ref()
                .map(|a| a.join(" "))
                .or_else(|| Some("cargo clippy".into())),
            ChildJobConfig::Format(cfg) => cfg
                .argv
                .as_ref()
                .map(|a| a.join(" "))
                .or_else(|| Some("cargo fmt".into())),
        };

        // Infer framework from command for test operations
        let framework = command_str.as_ref().and_then(|cmd| {
            if cmd.starts_with("cargo") {
                Some("cargo".into())
            } else if cmd.starts_with("pytest") || cmd.starts_with("python") {
                Some("pytest".into())
            } else if cmd.starts_with("npm") || cmd.starts_with("npx") {
                Some("npm".into())
            } else if cmd.starts_with("make") {
                Some("make".into())
            } else {
                None
            }
        });

        let details = match &request.config {
            ChildJobConfig::Test(_) => ChildJobDetails::Test(TestJobResult {
                status: if success {
                    "passed"
                } else if is_cancelled {
                    "cancelled"
                } else if is_timed_out {
                    "timed_out"
                } else {
                    "failed"
                }
                .into(),
                framework,
                cancelled: is_cancelled,
                timed_out: is_timed_out,
                ..Default::default()
            }),
            ChildJobConfig::Build(_) => ChildJobDetails::Build(BuildJobResult {
                status: if success {
                    "success"
                } else if is_cancelled {
                    "cancelled"
                } else if is_timed_out {
                    "timed_out"
                } else {
                    "failure"
                }
                .into(),
                command: command_str,
                ..Default::default()
            }),
            ChildJobConfig::Lint(_) => ChildJobDetails::Lint(LintJobResult {
                status: if success {
                    "clean"
                } else if is_cancelled {
                    "cancelled"
                } else if is_timed_out {
                    "timed_out"
                } else {
                    "warnings"
                }
                .into(),
                command: command_str,
                ..Default::default()
            }),
            ChildJobConfig::Format(_) => ChildJobDetails::Format(FormatJobResult {
                status: if success {
                    "clean"
                } else if is_cancelled {
                    "cancelled"
                } else if is_timed_out {
                    "timed_out"
                } else {
                    "needs_formatting"
                }
                .into(),
                command: command_str,
                would_change: !success && !is_cancelled && !is_timed_out,
                ..Default::default()
            }),
        };

        Ok(ChildJobResult {
            success,
            exit_code,
            duration_ms: completion.metrics.elapsed_ms,
            details,
            artifacts: vec![],
            error: if !success {
                Some(completion.summary)
            } else {
                None
            },
        })
    }

    async fn heartbeat(&self, _budget: &BudgetSnapshot) {}
}

#[async_trait]
impl JobExecutor for ToolProgramExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::ToolProgram
    }

    fn supports(&self, kind: JobKind) -> bool {
        matches!(kind, JobKind::ToolProgram)
    }

    fn validate(&self, job: &JobRecord) -> Result<(), ExecutorValidationError> {
        match &job.payload {
            JobPayload::ToolProgram {
                program_id,
                source_digest,
                authority_digest,
                ..
            } => {
                if program_id.is_empty() {
                    return Err(ExecutorValidationError::MissingField("program_id".into()));
                }
                if source_digest.is_empty() {
                    return Err(ExecutorValidationError::MissingField(
                        "source_digest".into(),
                    ));
                }
                if authority_digest.is_empty() {
                    return Err(ExecutorValidationError::MissingField(
                        "authority_digest".into(),
                    ));
                }
                Ok(())
            }
            _ => Err(ExecutorValidationError::UnsupportedKind {
                executor: "tool_program".into(),
                kind: format!("{:?}", job.kind),
            }),
        }
    }

    async fn execute(&self, ctx: JobExecutionContext) -> ExecutorCompletion {
        let started = std::time::Instant::now();

        // Emit progress: starting
        let _ = ctx
            .progress
            .progress(ctx.job_id(), "tool_program: starting")
            .await;

        // Extract payload
        let (program_id, source_digest, ir_digest, authority_digest) = match &ctx.job.payload {
            JobPayload::ToolProgram {
                program_id,
                source_digest,
                ir_digest,
                authority_digest,
                ..
            } => (
                program_id.clone(),
                source_digest.clone(),
                ir_digest.clone(),
                authority_digest.clone(),
            ),
            _ => {
                return ExecutorCompletion {
                    status: ExecutorStatus::Failed,
                    summary: "invalid payload".into(),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
        };

        // Emit progress: validating
        let _ = ctx
            .progress
            .progress(ctx.job_id(), "tool_program: validating")
            .await;

        // For M005, we use a fixture program. The IR should be provided
        // via the job payload or loaded from the program store.
        // Since we don't have full store integration yet, we compile
        // from source if available, or use a fixture.
        //
        // In production (M006+), the IR would be loaded from the
        // content-addressed store using source_digest and ir_digest.

        // Validate authority digest is not empty (already checked in validate)
        let _ = authority_digest;

        // Emit progress: loading IR
        let _ = ctx
            .progress
            .progress(ctx.job_id(), "tool_program: loading IR")
            .await;

        // For M005, compile a fixture program based on the source digest.
        // In production, this would load from the content-addressed store.
        let fixture_source =
            "emit({\"status\": \"ok\", \"program_id\": \"".to_string() + &program_id + "\"})\n";

        let compilation = match codegg_core::tool_program::compile_program(&fixture_source) {
            Ok(c) => c,
            Err(e) => {
                return ExecutorCompletion {
                    status: ExecutorStatus::Failed,
                    summary: format!("compilation failed: {}", e),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
        };

        // Verify IR integrity
        if let Err(e) = codegg_core::tool_program::verify_ir_integrity(&compilation.ir) {
            return ExecutorCompletion {
                status: ExecutorStatus::Failed,
                summary: format!("IR verification failed: {}", e),
                run_id: None,
                metrics: ExecutorMetrics {
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    ..Default::default()
                },
            };
        }

        // Source/IR digest validation against the content-addressed store
        // is deferred to M006 when ProgramStore integration is available.
        // For M005, we verify IR integrity (digest matches instructions)
        // and non-empty digests at admission time (validate() above).
        //
        // In production (M006+), the executor would:
        // 1. Load source from ContentAddressedStore by source_digest
        // 2. Recompile and verify ir_digest matches
        // 3. Load manifest and verify authority_digest
        let _ = &source_digest;
        let _ = &ir_digest;

        // Emit progress: executing
        let _ = ctx
            .progress
            .progress(ctx.job_id(), "tool_program: executing")
            .await;

        // Create runtime limits from IR bounds
        let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
        // Set sensible defaults for M005
        limits.max_stall_time_ms = 60_000; // 60s stall timeout
        limits.max_per_call_time_ms = 30_000; // 30s per-call timeout
        limits.max_retries = 2; // Up to 2 retries for transient errors

        // Save per-call timeout before moving limits
        let per_call_timeout_ms = limits.max_per_call_time_ms;

        // Compute wall deadline from job timeout or program limits
        let wall_deadline = ctx
            .job
            .deadline
            .map(|d| {
                let dur = d.signed_duration_since(chrono::Utc::now());
                if dur.num_milliseconds() > 0 {
                    Some(
                        tokio::time::Instant::now()
                            + tokio::time::Duration::from_millis(dur.num_milliseconds() as u64),
                    )
                } else {
                    None
                }
            })
            .flatten()
            .or_else(|| {
                if limits.max_wall_time_ms > 0 {
                    Some(
                        tokio::time::Instant::now()
                            + tokio::time::Duration::from_millis(limits.max_wall_time_ms),
                    )
                } else {
                    None
                }
            });

        // Create interpreter
        let mut interpreter = MeteredInterpreter::new(compilation.ir, limits);

        // Create real broker adapter
        let mut broker_adapter = BrokerAdapter::new(
            self.broker.clone(),
            self.registry.clone(),
            program_id.clone(),
        );
        if let Some(ref submission) = self.submission {
            broker_adapter = broker_adapter.with_submission(submission.clone());
        }
        broker_adapter = broker_adapter.with_workspace_id(ctx.workspace_id.clone());
        broker_adapter = broker_adapter
            .with_cwd(std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")));

        // Build run configuration
        let run_config = RunConfig {
            wall_deadline,
            per_call_timeout_ms: Some(per_call_timeout_ms),
            result_schema: None, // No schema for M005 fixture programs
        };

        // Run the interpreter with cancellation support and config
        let result = interpreter
            .run_with_config(&broker_adapter, Some(&ctx.cancellation), &run_config)
            .await;

        // Emit progress: completed
        let _ = ctx
            .progress
            .progress(ctx.job_id(), &format!("tool_program: {:?}", result.status))
            .await;

        // Map program result to executor completion
        let status = match result.status {
            ProgramStatus::Completed => ExecutorStatus::Completed,
            ProgramStatus::Failed => ExecutorStatus::Failed,
            ProgramStatus::Cancelled => ExecutorStatus::Cancelled,
            ProgramStatus::TimedOut => ExecutorStatus::TimedOut,
            ProgramStatus::Stalled => ExecutorStatus::TimedOut,
            ProgramStatus::Incomplete => ExecutorStatus::Failed,
            ProgramStatus::Recoverable => ExecutorStatus::Failed,
        };

        // Build summary
        let mut summary = format!(
            "status={:?} steps={} iterations={} bytes={} calls={}",
            result.status,
            result.steps_used,
            result.iterations_used,
            result.bytes_used,
            result.calls_completed
        );
        if let Some(ref output) = result.output {
            summary.push_str(&format!("\noutput: {}", output));
        }
        if let Some(ref err) = result.error_message {
            summary.push_str(&format!("\nerror: {}", err));
        }
        if let Some(ref class) = result.failure_class {
            summary.push_str(&format!("\nfailure_class: {}", class));
        }

        ExecutorCompletion {
            status,
            summary,
            run_id: None,
            metrics: ExecutorMetrics {
                cpu_time_ms: None,
                peak_memory_mb: None,
                elapsed_ms: started.elapsed().as_millis() as u64,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::jobs::{
        IdempotencyClass, JobId, JobPayload, JobPriority, JobSource, JobState, ResourceRequest,
        RetryPolicy,
    };
    use codegg_core::workspace::WorkspaceId;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn sample_tool_program_job(program_id: &str, source_digest: &str) -> JobRecord {
        let now = chrono::Utc::now();
        JobRecord {
            job_id: JobId::new_unchecked("j-tp"),
            workspace_id: WorkspaceId::new_unchecked("ws-1"),
            session_id: None,
            turn_id: None,
            kind: JobKind::ToolProgram,
            source: JobSource::Interactive,
            priority: JobPriority::Normal,
            payload: JobPayload::ToolProgram {
                program_id: program_id.to_string(),
                source_digest: source_digest.to_string(),
                ir_digest: None,
                authority_digest: "auth_digest_abc".to_string(),
                submission_key: "key_123".to_string(),
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
            labels: HashMap::new(),
        }
    }

    #[test]
    fn executor_kind_is_tool_program() {
        let exec = ToolProgramExecutor::default();
        assert_eq!(exec.kind(), ExecutorKind::ToolProgram);
    }

    #[test]
    fn supports_tool_program_kind() {
        let exec = ToolProgramExecutor::default();
        assert!(exec.supports(JobKind::ToolProgram));
        assert!(!exec.supports(JobKind::Python));
        assert!(!exec.supports(JobKind::Test));
    }

    #[test]
    fn validate_rejects_empty_program_id() {
        let exec = ToolProgramExecutor::default();
        let job = sample_tool_program_job("", "digest");
        assert!(exec.validate(&job).is_err());
    }

    #[test]
    fn validate_rejects_empty_source_digest() {
        let exec = ToolProgramExecutor::default();
        let job = sample_tool_program_job("prog_1", "");
        assert!(exec.validate(&job).is_err());
    }

    #[test]
    fn validate_rejects_empty_authority_digest() {
        let exec = ToolProgramExecutor::default();
        let mut job = sample_tool_program_job("prog_1", "digest");
        if let JobPayload::ToolProgram {
            ref mut authority_digest,
            ..
        } = job.payload
        {
            authority_digest.clear();
        }
        assert!(exec.validate(&job).is_err());
    }

    #[test]
    fn validate_accepts_valid_job() {
        let exec = ToolProgramExecutor::default();
        let job = sample_tool_program_job("prog_1", "digest_abc");
        assert!(exec.validate(&job).is_ok());
    }

    #[test]
    fn validate_rejects_wrong_payload() {
        let exec = ToolProgramExecutor::default();
        let now = chrono::Utc::now();
        let job = JobRecord {
            job_id: JobId::new_unchecked("j-test"),
            workspace_id: WorkspaceId::new_unchecked("ws-1"),
            session_id: None,
            turn_id: None,
            kind: JobKind::Test,
            source: JobSource::Interactive,
            priority: JobPriority::Normal,
            payload: JobPayload::Test {
                command: "echo ok".into(),
                argv: vec!["echo".into(), "ok".into()],
                cwd: None,
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
            labels: HashMap::new(),
        };
        assert!(exec.validate(&job).is_err());
    }

    #[tokio::test]
    async fn execute_fixture_program() {
        use crate::scheduler::executor::{JobExecutionContext, NoopProgressSink};
        use crate::scheduler::permit::ResourcePermitGuard;
        use codegg_core::jobs::AttemptId;
        use codegg_core::workspace::WorkspaceId;

        let exec = ToolProgramExecutor::default();
        let source = codegg_core::tool_program::ProgramStore::digest_source(
            "emit({\"status\": \"ok\", \"program_id\": \"test_prog\"})\n",
        );

        let job = sample_tool_program_job("test_prog", &source);

        let ctx = JobExecutionContext {
            job,
            attempt_id: AttemptId::new_unchecked("att-1"),
            daemon_generation: codegg_core::jobs::DaemonGeneration::new_unchecked("gen-1"),
            workspace_id: WorkspaceId::new_unchecked("ws-1"),
            cancellation: tokio_util::sync::CancellationToken::new(),
            progress: Arc::new(NoopProgressSink),
            resources: ResourcePermitGuard::new_orphan(Default::default()),
        };

        let result = exec.execute(ctx).await;
        assert_eq!(result.status, ExecutorStatus::Completed);
        assert!(result.summary.contains("Completed"));
    }

    #[tokio::test]
    async fn execute_cancelled_program() {
        use crate::scheduler::executor::{JobExecutionContext, NoopProgressSink};
        use crate::scheduler::permit::ResourcePermitGuard;
        use codegg_core::jobs::AttemptId;
        use codegg_core::workspace::WorkspaceId;

        let exec = ToolProgramExecutor::default();
        let source = codegg_core::tool_program::ProgramStore::digest_source(
            "emit({\"status\": \"ok\", \"program_id\": \"test_prog\"})\n",
        );

        let job = sample_tool_program_job("test_prog", &source);

        let token = tokio_util::sync::CancellationToken::new();
        token.cancel();

        let ctx = JobExecutionContext {
            job,
            attempt_id: AttemptId::new_unchecked("att-2"),
            daemon_generation: codegg_core::jobs::DaemonGeneration::new_unchecked("gen-1"),
            workspace_id: WorkspaceId::new_unchecked("ws-1"),
            cancellation: token,
            progress: Arc::new(NoopProgressSink),
            resources: ResourcePermitGuard::new_orphan(Default::default()),
        };

        let result = exec.execute(ctx).await;
        assert_eq!(result.status, ExecutorStatus::Cancelled);
    }
}
