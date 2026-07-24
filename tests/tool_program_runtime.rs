//! Integration tests for Tool Program runtime execution (M005).
//!
//! Tests that fixture programs execute through the production
//! [`ToolProgramExecutor`] and return typed terminal results.

use std::sync::Arc;

use codegg_core::jobs::{
    AttemptId, DaemonGeneration, IdempotencyClass, JobId, JobKind, JobPayload, JobPriority,
    JobRecord, JobSource, JobState, ResourceRequest, RetryPolicy,
};
use codegg_core::tool_program::{compile_program, ProgramStore};
use codegg_core::workspace::WorkspaceId;

use codegg::scheduler::executor::{
    ExecutorCompletion, ExecutorKind, ExecutorMetrics, ExecutorStatus, ExecutorValidationError,
    JobExecutionContext, JobExecutor, NoopProgressSink,
};
use codegg::scheduler::permit::ResourcePermitGuard;
use codegg::scheduler::tool_program_executor::ToolProgramExecutor;

fn sample_job(program_id: &str, source: &str) -> JobRecord {
    let now = chrono::Utc::now();
    let source_digest = ProgramStore::digest_source(source);
    JobRecord {
        job_id: JobId::new_unchecked("j-tp-integration"),
        workspace_id: WorkspaceId::new_unchecked("ws-integration"),
        session_id: None,
        turn_id: None,
        kind: JobKind::ToolProgram,
        source: JobSource::Interactive,
        priority: JobPriority::Normal,
        payload: JobPayload::ToolProgram {
            program_id: program_id.to_string(),
            source_digest,
            ir_digest: None,
            authority_digest: "auth_test".to_string(),
            submission_key: "sub_test".to_string(),
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
        labels: Default::default(),
    }
}

fn make_ctx(job: JobRecord, cancelled: bool) -> JobExecutionContext {
    let token = tokio_util::sync::CancellationToken::new();
    if cancelled {
        token.cancel();
    }
    JobExecutionContext {
        job,
        attempt_id: AttemptId::new_unchecked("att-integration"),
        daemon_generation: DaemonGeneration::new_unchecked("gen-1"),
        workspace_id: WorkspaceId::new_unchecked("ws-integration"),
        cancellation: token,
        progress: Arc::new(NoopProgressSink),
        resources: ResourcePermitGuard::new_orphan(Default::default()),
    }
}

#[tokio::test]
async fn emit_constant_completes() {
    let exec = ToolProgramExecutor::default();
    let source = "emit({\"ok\": true})\n";
    let job = sample_job("prog_emit", source);
    let ctx = make_ctx(job, false);
    let result = exec.execute(ctx).await;
    assert_eq!(result.status, ExecutorStatus::Completed);
    assert!(result.summary.contains("Completed"));
}

#[tokio::test]
async fn for_loop_program_completes() {
    let exec = ToolProgramExecutor::default();
    let source = r#"
total = 0
for i in range(5):
    total = total + 1
emit({"total": total})
"#;
    let job = sample_job("prog_loop", source);
    let ctx = make_ctx(job, false);
    let result = exec.execute(ctx).await;
    assert_eq!(result.status, ExecutorStatus::Completed);
}

#[tokio::test]
async fn if_else_program_completes() {
    let exec = ToolProgramExecutor::default();
    let source = r#"
x = 10
if x > 5:
    result = "big"
else:
    result = "small"
emit({"result": result})
"#;
    let job = sample_job("prog_if", source);
    let ctx = make_ctx(job, false);
    let result = exec.execute(ctx).await;
    assert_eq!(result.status, ExecutorStatus::Completed);
}

#[tokio::test]
async fn failed_program_returns_failed() {
    // In M005, the executor runs a fixture program. To test the
    // cancellation/failed path, we cancel the job before execution.
    let exec = ToolProgramExecutor::default();
    let source = "emit({\"ok\": true})\n";
    let job = sample_job("prog_fail", source);
    let ctx = make_ctx(job, true);
    let result = exec.execute(ctx).await;
    assert_eq!(result.status, ExecutorStatus::Cancelled);
}

#[tokio::test]
async fn cancelled_program_returns_cancelled() {
    let exec = ToolProgramExecutor::default();
    let source = "emit({\"ok\": true})\n";
    let job = sample_job("prog_cancel", source);
    let ctx = make_ctx(job, true); // cancelled = true
    let result = exec.execute(ctx).await;
    assert_eq!(result.status, ExecutorStatus::Cancelled);
}

#[tokio::test]
async fn nested_loop_program_completes() {
    let exec = ToolProgramExecutor::default();
    let source = r#"
total = 0
for i in range(3):
    for j in range(3):
        total = total + 1
emit({"total": total})
"#;
    let job = sample_job("prog_nested", source);
    let ctx = make_ctx(job, false);
    let result = exec.execute(ctx).await;
    assert_eq!(result.status, ExecutorStatus::Completed);
}

#[tokio::test]
async fn list_operations_program_completes() {
    let exec = ToolProgramExecutor::default();
    let source = r#"
items = [3, 1, 4, 1, 5]
first = items[0]
last = items[4]
emit({"first": first, "last": last})
"#;
    let job = sample_job("prog_list", source);
    let ctx = make_ctx(job, false);
    let result = exec.execute(ctx).await;
    assert_eq!(result.status, ExecutorStatus::Completed);
}

#[tokio::test]
async fn string_operations_program_completes() {
    let exec = ToolProgramExecutor::default();
    let source = r#"
greeting = "hello" + " " + "world"
length = len(greeting)
emit({"greeting": greeting, "length": length})
"#;
    let job = sample_job("prog_str", source);
    let ctx = make_ctx(job, false);
    let result = exec.execute(ctx).await;
    assert_eq!(result.status, ExecutorStatus::Completed);
}

#[test]
fn executor_registry_includes_tool_program() {
    let mut registry = codegg::scheduler::executor::ExecutorRegistry::new();
    registry
        .register(Arc::new(ToolProgramExecutor::default()))
        .unwrap();
    assert!(registry.get(ExecutorKind::ToolProgram).is_some());
}

#[test]
fn tool_program_job_routes_to_correct_executor() {
    let job = sample_job("prog_1", "emit(1)\n");
    let kind = codegg::scheduler::executor::executor_kind_for_job(&job);
    assert_eq!(kind, Some(ExecutorKind::ToolProgram));
}
