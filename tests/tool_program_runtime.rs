//! Integration tests for Tool Program runtime execution (M005).
//!
//! Tests that fixture programs execute through the production
//! [`ToolProgramExecutor`] and return typed terminal results.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use async_trait::async_trait;
use codegg_core::jobs::{
    AttemptId, DaemonGeneration, IdempotencyClass, JobId, JobKind, JobPayload, JobPriority,
    JobRecord, JobSource, JobState, ResourceRequest, RetryPolicy,
};
use codegg_core::tool_program::ProgramStore;
use codegg_core::workspace::WorkspaceId;
use serde_json::json;

use codegg::error::ToolError;
use codegg::scheduler::executor::{
    ExecutorKind, ExecutorStatus, JobExecutionContext, JobExecutor, NoopProgressSink,
};
use codegg::scheduler::permit::ResourcePermitGuard;
use codegg::scheduler::tool_program_executor::ToolProgramExecutor;
use codegg::tool::broker::ToolBroker;
use codegg::tool::contract::{ToolCallerPolicy, ToolContract, ToolEffectClass};
use codegg::tool::{Tool, ToolCategory, ToolRegistry};

const FIXTURE_TOOL_NAME: &str = "runtime_fixture_read";

struct RuntimeFixtureTool {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for RuntimeFixtureTool {
    fn name(&self) -> &str {
        FIXTURE_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Deterministic read-only Tool Programs runtime fixture"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(r#"{"value":"runtime-fixture"}"#.into())
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.into(),
            caller_policy: ToolCallerPolicy::DirectOrProgrammatic,
            effect_class: ToolEffectClass::ReadOnly,
            output_schema: Some(json!({
                "type": "object",
                "properties": {"value": {"type": "string"}},
                "required": ["value"]
            })),
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }
}

struct RuntimeFixture {
    executor: ToolProgramExecutor,
    allowed_tools: Vec<String>,
    contract_snapshot_json: String,
    contract_digest: String,
    calls: Arc<AtomicUsize>,
}

impl RuntimeFixture {
    fn new() -> Self {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::with_defaults();
        registry.register(RuntimeFixtureTool {
            calls: calls.clone(),
        });
        let registry = Arc::new(registry);
        let broker = Arc::new(ToolBroker::new(&registry));
        let allowed_tools = vec![FIXTURE_TOOL_NAME.to_string()];
        let contracts =
            codegg::tool::tool_program_context::resolve_contract_snapshot(&broker, &allowed_tools)
                .unwrap();
        let contract_snapshot_json =
            codegg::tool::tool_program_context::canonical_contract_json(&contracts).unwrap();
        let contract_digest =
            codegg::tool::tool_program_context::canonical_contract_digest(&contracts).unwrap();

        Self {
            executor: ToolProgramExecutor::new(broker, registry),
            allowed_tools,
            contract_snapshot_json,
            contract_digest,
            calls,
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

fn sample_job(fixture: &RuntimeFixture, program_id: &str, source: &str) -> JobRecord {
    let now = chrono::Utc::now();
    let source_digest = ProgramStore::digest_source(source);
    // Durable Tool Program records are keyed by program ID. Include the
    // source digest so repeated integration-test runs never replay stale
    // records left by an earlier test process.
    let program_id = format!("{program_id}-{}", &source_digest[..12]);
    let execution_context = codegg_core::jobs::ToolProgramExecutionContext {
        workspace_path_policy_id: "ws-integration".into(),
        principal_ref: Some("test-principal".into()),
        authority_ref: Some("test-decision".into()),
        policy_revision: Some("test-policy-v1".into()),
        path_policy_revision: Some("test-path-v1".into()),
        decision_outcome: Some("allowed".into()),
        caller_class: Some("agent".into()),
        maximum_effect_class: Some("read_only".into()),
        decision_issued_at: Some(now.timestamp_millis()),
        contract_snapshot_json: fixture.contract_snapshot_json.clone(),
        ..codegg_core::jobs::ToolProgramExecutionContext::for_workspace(
            "ws-integration",
            "test-correlation",
        )
    };
    let authority_digest = codegg::tool::tool_program_context::authority_digest(
        &execution_context,
        &fixture.allowed_tools,
        &source_digest,
    );
    let source_ref = codegg::tool::tool_program_source::ToolProgramSourceStore::new(
        &std::env::current_dir().unwrap(),
    )
    .persist(source)
    .unwrap();
    let authority_grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&execution_context),
        "ws-integration",
        &program_id,
        &fixture.allowed_tools,
        &source_digest,
        "",
        &fixture.contract_digest,
    )
    .unwrap();
    let authority_grant_json = serde_json::to_string(&authority_grant).unwrap();
    JobRecord {
        job_id: JobId::new_unchecked(format!("j-tp-integration-{program_id}")),
        workspace_id: WorkspaceId::new_unchecked("ws-integration"),
        session_id: None,
        turn_id: None,
        kind: JobKind::ToolProgram,
        source: JobSource::Interactive,
        priority: JobPriority::Normal,
        payload: JobPayload::ToolProgram {
            program_id: program_id.to_string(),
            invocation_key: format!("test-invocation-{program_id}"),
            source_digest,
            ir_digest: None,
            authority_digest,
            execution_context_json: Some(serde_json::to_string(&execution_context).unwrap()),
            submission_key: format!("sub_test-{program_id}"),
            execution_mode: "foreground".to_string(),
            source_ref: Some(source_ref.relative_path),
            source_length: Some(source_ref.length),
            allowed_tools: fixture.allowed_tools.clone(),
            authority_grant_json: Some(authority_grant_json),
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
        parent_job_id: None,
        parent_attempt_id: None,
        parent_call_id: None,
        parent_program_id: None,
        parent_instruction_sequence: None,
        relation_kind: None,
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
        workspace_root: std::env::current_dir().unwrap(),
        run_store: None,
        cancellation: token,
        progress: Arc::new(NoopProgressSink),
        resources: ResourcePermitGuard::new_orphan(Default::default()),
    }
}

fn assert_failed(result: &codegg::scheduler::executor::ExecutorCompletion, fragment: &str) {
    assert_eq!(result.status, ExecutorStatus::Failed);
    assert!(
        result.summary.contains(fragment),
        "summary={}",
        result.summary
    );
}

#[tokio::test(flavor = "current_thread")]
async fn emit_constant_completes() {
    let fixture = RuntimeFixture::new();
    let result = fixture
        .executor
        .execute(make_ctx(
            sample_job(&fixture, "prog_emit", "emit({\"ok\": true})\n"),
            false,
        ))
        .await;
    assert_eq!(
        result.status,
        ExecutorStatus::Completed,
        "summary={}",
        result.summary
    );
    assert_eq!(fixture.call_count(), 0);
}

#[test]
fn runtime_fixture_contract_bundle_is_canonical_and_consistent() {
    let fixture = RuntimeFixture::new();
    assert_eq!(fixture.allowed_tools, vec![FIXTURE_TOOL_NAME]);
    let snapshot: serde_json::Value =
        serde_json::from_str(&fixture.contract_snapshot_json).unwrap();
    assert_eq!(snapshot["contracts"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot["contracts"][0]["tool_name"], FIXTURE_TOOL_NAME);
    assert_eq!(
        snapshot["contracts"][0]["caller_policy"],
        "direct_or_programmatic"
    );
    assert_eq!(snapshot["contracts"][0]["effect_class"], "read_only");
    assert!(fixture.contract_digest.starts_with("sha256:"));

    let job = sample_job(&fixture, "prog_contract_consistency", "emit(1)\n");
    let JobPayload::ToolProgram {
        allowed_tools,
        execution_context_json,
        authority_grant_json,
        ..
    } = job.payload
    else {
        panic!("expected Tool Program payload");
    };
    assert_eq!(allowed_tools, fixture.allowed_tools);
    let context: codegg_core::jobs::ToolProgramExecutionContext =
        serde_json::from_str(&execution_context_json.unwrap()).unwrap();
    assert_eq!(
        context.contract_snapshot_json,
        fixture.contract_snapshot_json
    );
    let grant: codegg_core::jobs::ToolAuthorityGrant =
        serde_json::from_str(&authority_grant_json.unwrap()).unwrap();
    assert_eq!(grant.contract_snapshot_json, fixture.contract_snapshot_json);
    assert_eq!(grant.contract_digest, fixture.contract_digest);
    assert!(grant.verify_integrity());
}

#[tokio::test(flavor = "current_thread")]
async fn for_loop_program_completes() {
    let fixture = RuntimeFixture::new();
    let source =
        "\ntotal = 0\nfor i in range(5):\n    total = total + 1\nemit({\"total\": total})\n";
    let result = fixture
        .executor
        .execute(make_ctx(sample_job(&fixture, "prog_loop", source), false))
        .await;
    assert_eq!(result.status, ExecutorStatus::Completed);
    assert_eq!(fixture.call_count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn if_else_program_completes() {
    let fixture = RuntimeFixture::new();
    let source = "\nx = 10\nif x > 5:\n    result = \"big\"\nelse:\n    result = \"small\"\nemit({\"result\": result})\n";
    let result = fixture
        .executor
        .execute(make_ctx(sample_job(&fixture, "prog_if", source), false))
        .await;
    assert_eq!(result.status, ExecutorStatus::Completed);
    assert_eq!(fixture.call_count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn pre_cancelled_program_returns_cancelled() {
    let fixture = RuntimeFixture::new();
    let job = sample_job(&fixture, "prog_cancelled", "emit({\"ok\": true})\n");
    let result = fixture.executor.execute(make_ctx(job, true)).await;
    assert_eq!(result.status, ExecutorStatus::Cancelled);
    assert_eq!(fixture.call_count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_program_returns_cancelled() {
    let fixture = RuntimeFixture::new();
    let job = sample_job(&fixture, "prog_cancel", "emit({\"ok\": true})\n");
    let result = fixture.executor.execute(make_ctx(job, true)).await;
    assert_eq!(result.status, ExecutorStatus::Cancelled);
    assert_eq!(fixture.call_count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn nested_loop_program_completes() {
    let fixture = RuntimeFixture::new();
    let source = "\ntotal = 0\nfor i in range(3):\n    for j in range(3):\n        total = total + 1\nemit({\"total\": total})\n";
    let result = fixture
        .executor
        .execute(make_ctx(sample_job(&fixture, "prog_nested", source), false))
        .await;
    assert_eq!(result.status, ExecutorStatus::Completed);
    assert_eq!(fixture.call_count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn list_operations_program_completes() {
    let fixture = RuntimeFixture::new();
    let source = "\nitems = [3, 1, 4, 1, 5]\nfirst = items[0]\nlast = items[4]\nemit({\"first\": first, \"last\": last})\n";
    let result = fixture
        .executor
        .execute(make_ctx(sample_job(&fixture, "prog_list", source), false))
        .await;
    assert_eq!(result.status, ExecutorStatus::Completed);
    assert_eq!(fixture.call_count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn string_operations_program_completes() {
    let fixture = RuntimeFixture::new();
    let source = "\ngreeting = \"hello\" + \" \" + \"world\"\nlength = len(greeting)\nemit({\"greeting\": greeting, \"length\": length})\n";
    let result = fixture
        .executor
        .execute(make_ctx(sample_job(&fixture, "prog_str", source), false))
        .await;
    assert_eq!(result.status, ExecutorStatus::Completed);
    assert_eq!(fixture.call_count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn empty_frozen_contract_is_rejected() {
    let fixture = RuntimeFixture::new();
    let mut job = sample_job(&fixture, "prog_empty_contract", "emit(1)\n");
    if let JobPayload::ToolProgram {
        allowed_tools,
        execution_context_json,
        authority_digest,
        authority_grant_json,
        source_digest,
        ..
    } = &mut job.payload
    {
        allowed_tools.clear();
        let mut context: codegg_core::jobs::ToolProgramExecutionContext =
            serde_json::from_str(execution_context_json.as_ref().unwrap()).unwrap();
        context.contract_snapshot_json = r#"{"contracts":[]}"#.into();
        *authority_digest = codegg::tool::tool_program_context::authority_digest(
            &context,
            allowed_tools,
            source_digest,
        );
        *execution_context_json = Some(serde_json::to_string(&context).unwrap());
        let grant = codegg::tool::tool_program_context::build_authority_grant(
            Some(&context),
            "ws-integration",
            "prog_empty_contract",
            allowed_tools,
            source_digest,
            "",
            "",
        )
        .unwrap();
        *authority_grant_json = Some(serde_json::to_string(&grant).unwrap());
    }
    let result = fixture.executor.execute(make_ctx(job, false)).await;
    assert_failed(&result, "at least one frozen runtime contract");
    assert_eq!(fixture.call_count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn allowed_tools_snapshot_mismatch_is_rejected() {
    let fixture = RuntimeFixture::new();
    let mut job = sample_job(&fixture, "prog_mismatch", "emit(1)\n");
    if let JobPayload::ToolProgram {
        allowed_tools,
        authority_digest,
        execution_context_json,
        source_digest,
        ..
    } = &mut job.payload
    {
        allowed_tools[0] = "runtime_fixture_missing".into();
        let context: codegg_core::jobs::ToolProgramExecutionContext =
            serde_json::from_str(execution_context_json.as_ref().unwrap()).unwrap();
        *authority_digest = codegg::tool::tool_program_context::authority_digest(
            &context,
            allowed_tools,
            source_digest,
        );
    }
    let result = fixture.executor.execute(make_ctx(job, false)).await;
    assert_failed(&result, "runtime contract resolution failed");
    assert_eq!(fixture.call_count(), 0);
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
    let fixture = RuntimeFixture::new();
    let job = sample_job(&fixture, "prog_1", "emit(1)\n");
    let kind = codegg::scheduler::executor::executor_kind_for_job(&job);
    assert_eq!(kind, Some(ExecutorKind::ToolProgram));
}
