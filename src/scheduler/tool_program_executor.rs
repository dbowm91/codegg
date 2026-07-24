//! Scheduler executor for Tool Programs (M005).
//!
//! Loads verified IR, creates a [`MeteredInterpreter`], and runs it
//! through the scheduler's admission-controlled execution path with
//! cancellation, heartbeat, and typed terminal results.

use std::sync::Arc;

use async_trait::async_trait;
use codegg_core::jobs::{JobKind, JobPayload, JobRecord};
use codegg_core::tool_program::{
    BrokerCallback, CallRequest, CallResult, InterpreterError, MeteredInterpreter, ProgramResult,
    ProgramStatus, ProgramValue, RunConfig, RuntimeLimits,
};

use crate::scheduler::executor::{
    ExecutorCompletion, ExecutorKind, ExecutorMetrics, ExecutorStatus, ExecutorValidationError,
    JobExecutionContext, JobExecutor,
};

/// In-process fixture broker for M005 testing.
///
/// Returns a deterministic `ToolResult` for any tool call. Real broker
/// integration through [`codegg::tool::broker::ToolBroker`] begins in M006.
pub struct FixtureBroker;

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

    async fn heartbeat(&self, _budget: &codegg_core::tool_program::BudgetSnapshot) {
        // Fixture broker heartbeat is a no-op
    }
}

/// Scheduler executor for `JobKind::ToolProgram`.
///
/// Validates the program payload, loads and verifies IR, creates a
/// [`MeteredInterpreter`], and runs it with cancellation support.
pub struct ToolProgramExecutor;

impl ToolProgramExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ToolProgramExecutor {
    fn default() -> Self {
        Self::new()
    }
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

        // Verify source digest matches
        // NOTE: In M005, the executor compiles a fixture program, so we
        // skip source/IR digest validation. In M006+, the IR would be
        // loaded from the content-addressed store and these checks would
        // be performed against the stored IR.
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

        // Create fixture broker
        let broker = FixtureBroker;

        // Build run configuration
        let run_config = RunConfig {
            wall_deadline,
            per_call_timeout_ms: Some(per_call_timeout_ms),
            result_schema: None, // No schema for M005 fixture programs
        };

        // Run the interpreter with cancellation support and config
        let result = interpreter
            .run_with_config(&broker, Some(&ctx.cancellation), &run_config)
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
        let exec = ToolProgramExecutor::new();
        assert_eq!(exec.kind(), ExecutorKind::ToolProgram);
    }

    #[test]
    fn supports_tool_program_kind() {
        let exec = ToolProgramExecutor::new();
        assert!(exec.supports(JobKind::ToolProgram));
        assert!(!exec.supports(JobKind::Python));
        assert!(!exec.supports(JobKind::Test));
    }

    #[test]
    fn validate_rejects_empty_program_id() {
        let exec = ToolProgramExecutor::new();
        let job = sample_tool_program_job("", "digest");
        assert!(exec.validate(&job).is_err());
    }

    #[test]
    fn validate_rejects_empty_source_digest() {
        let exec = ToolProgramExecutor::new();
        let job = sample_tool_program_job("prog_1", "");
        assert!(exec.validate(&job).is_err());
    }

    #[test]
    fn validate_rejects_empty_authority_digest() {
        let exec = ToolProgramExecutor::new();
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
        let exec = ToolProgramExecutor::new();
        let job = sample_tool_program_job("prog_1", "digest_abc");
        assert!(exec.validate(&job).is_ok());
    }

    #[test]
    fn validate_rejects_wrong_payload() {
        let exec = ToolProgramExecutor::new();
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

        let exec = ToolProgramExecutor::new();
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

        let exec = ToolProgramExecutor::new();
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
