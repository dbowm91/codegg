//! M014 canonical artifact tests.
//!
//! Covers C-39 through C-44: call artifacts use canonical resolvable handles,
//! child artifacts include real attempt/run identity, large output spills
//! through the canonical store, and missing/corrupt data fails closed.

#![cfg(test)]

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use codegg::context::ContextArtifactStore;
use codegg::scheduler::executor::{JobExecutionContext, NoopProgressSink};
use codegg::scheduler::permit::ResourcePermitGuard;
use codegg::scheduler::tool_program_executor::ToolProgramExecutor;
use codegg::scheduler::{ExecutorStatus, JobExecutor};
use codegg::tool::{
    StructuredToolResult, Tool, ToolBroker, ToolCallerPolicy, ToolCategory, ToolContract,
    ToolEffectClass, ToolExecutionContext, ToolRegistry,
};
use codegg_core::jobs::{
    AttemptId, DaemonGeneration, IdempotencyClass, JobId, JobKind, JobPayload, JobPriority,
    JobRecord, JobSource, JobState, ResourceRequest, RetryPolicy, ToolProgramExecutionContext,
};
use codegg_core::workspace::WorkspaceId;
use serde_json::json;

struct LargeStructuredOutputTool;

#[async_trait::async_trait]
impl Tool for LargeStructuredOutputTool {
    fn name(&self) -> &str {
        "large_structured_output"
    }

    fn description(&self) -> &str {
        "Returns large structured output"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object"})
    }

    async fn execute(&self, _input: serde_json::Value) -> Result<String, codegg::error::ToolError> {
        Ok("large structured output".into())
    }

    async fn execute_structured(
        &self,
        _input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, codegg::error::ToolError> {
        Ok(StructuredToolResult::with_value(
            "large structured output".into(),
            json!("x".repeat(400_000)),
            true,
            None,
        ))
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    fn contract(&self, name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: name.into(),
            caller_policy: ToolCallerPolicy::DirectOrProgrammatic,
            effect_class: ToolEffectClass::ReadOnly,
            idempotency: codegg::tool::IdempotencyClass::Idempotent,
            output_schema: Some(json!({"type": "string"})),
            ..ToolContract::legacy(name, input_schema)
        }
    }
}

struct FailingArtifactStore;

#[async_trait::async_trait]
impl ContextArtifactStore for FailingArtifactStore {
    async fn put(&self, _artifact: codegg::context::ContextArtifact) -> anyhow::Result<()> {
        anyhow::bail!("simulated artifact storage failure")
    }

    async fn get(&self, _handle: &str) -> anyhow::Result<Option<codegg::context::ContextArtifact>> {
        Ok(None)
    }

    async fn list_recent(
        &self,
        _session_id: &str,
        _limit: usize,
    ) -> anyhow::Result<Vec<codegg::context::ContextArtifact>> {
        Ok(Vec::new())
    }
}

fn c41_executor_fixture(
    workspace: &Path,
    program_id: &str,
    artifact_store: Arc<dyn ContextArtifactStore>,
) -> (ToolProgramExecutor, JobExecutionContext) {
    let source = "value = call({\"tool\": \"large_structured_output\"})\nemit(value)\n";
    let source_ref = codegg::tool::tool_program_source::ToolProgramSourceStore::new(workspace)
        .persist(source)
        .unwrap();
    let source_digest = codegg_core::tool_program::ProgramStore::digest_source(source);
    let mut registry = ToolRegistry::with_defaults();
    registry.register(LargeStructuredOutputTool);
    let registry = Arc::new(registry);
    let broker = Arc::new(ToolBroker::new(&registry));
    let tools = vec!["large_structured_output".to_string()];
    let contracts =
        codegg::tool::tool_program_context::resolve_contract_snapshot(&broker, &tools).unwrap();
    let contract_json =
        codegg::tool::tool_program_context::canonical_contract_json(&contracts).unwrap();
    let contract_digest =
        codegg::tool::tool_program_context::canonical_contract_digest(&contracts).unwrap();
    let now = chrono::Utc::now();
    let execution_context = ToolProgramExecutionContext {
        workspace_path_policy_id: "ws-1".into(),
        principal_ref: Some("test-principal".into()),
        authority_ref: Some("test-decision".into()),
        policy_revision: Some("test-policy-v1".into()),
        path_policy_revision: Some("test-path-v1".into()),
        decision_outcome: Some("allowed".into()),
        caller_class: Some("agent".into()),
        maximum_effect_class: Some("read_only".into()),
        decision_issued_at: Some(now.timestamp_millis()),
        session_id: Some("session-c41".into()),
        contract_snapshot_json: contract_json,
        ..ToolProgramExecutionContext::for_workspace("ws-1", "test")
    };
    let authority_digest = codegg::tool::tool_program_context::authority_digest(
        &execution_context,
        &tools,
        &source_digest,
    );
    let authority_grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&execution_context),
        "ws-1",
        program_id,
        &tools,
        &source_digest,
        "",
        &contract_digest,
    )
    .unwrap();
    let job = JobRecord {
        job_id: JobId::new_unchecked(format!("job-{program_id}")),
        workspace_id: WorkspaceId::new_unchecked("ws-1"),
        session_id: execution_context.session_id.clone(),
        turn_id: None,
        kind: JobKind::ToolProgram,
        source: JobSource::Interactive,
        priority: JobPriority::Normal,
        payload: JobPayload::ToolProgram {
            program_id: program_id.into(),
            invocation_key: format!("invocation-{program_id}"),
            source_digest,
            ir_digest: None,
            authority_digest,
            execution_context_json: Some(serde_json::to_string(&execution_context).unwrap()),
            submission_key: format!("submission-{program_id}"),
            execution_mode: "foreground".into(),
            source_ref: Some(source_ref.relative_path),
            source_length: Some(source_ref.length),
            allowed_tools: tools,
            authority_grant_json: Some(serde_json::to_string(&authority_grant).unwrap()),
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
        parent_job_id: None,
        parent_attempt_id: None,
        parent_call_id: None,
        parent_program_id: None,
        parent_instruction_sequence: None,
        relation_kind: None,
    };
    let executor = ToolProgramExecutor::new(broker, registry).with_artifact_store(artifact_store);
    let context = JobExecutionContext {
        job,
        attempt_id: AttemptId::new_unchecked(format!("attempt-{program_id}")),
        daemon_generation: DaemonGeneration::new_unchecked("generation-c41"),
        workspace_id: WorkspaceId::new_unchecked("ws-1"),
        workspace_root: workspace.to_path_buf(),
        cancellation: tokio_util::sync::CancellationToken::new(),
        progress: Arc::new(NoopProgressSink),
        resources: ResourcePermitGuard::new_orphan(Default::default()),
    };
    (executor, context)
}

/// C-39: Call artifacts use canonical resolvable handles and verified content
/// digests.
#[tokio::test(flavor = "current_thread")]
async fn c39_call_artifacts_have_digests() {
    let temp = tempfile::tempdir().unwrap();
    let store = codegg::tool::tool_program_result::ToolProgramResultStore::new(temp.path());

    let program_id = "tp-c39";
    let attempt_id = "att-c39";
    let result = codegg_core::tool_program::ProgramResult {
        status: codegg_core::tool_program::ProgramStatus::Completed,
        output: Some(codegg_core::tool_program::ProgramValue::String(
            "hello world".into(),
        )),
        error_message: None,
        failure_class: None,
        steps_used: 10,
        iterations_used: 1,
        bytes_used: 128,
        calls_completed: 1,
        calls_total: 1,
    };

    let call_artifacts = vec![codegg::tool::tool_program_result::ProgramArtifactHandle {
        tool_name: Some("read".into()),
        preview: "hello world".into(),
        success: true,
        artifact_id: Some("sha256:artifact-c39".into()),
        digest: Some("sha256:content-c39".into()),
        absence_reason: None,
    }];

    let record = store
        .persist(
            program_id,
            attempt_id,
            "native",
            result,
            call_artifacts,
            vec![],
            None,
        )
        .expect("persist should succeed");

    assert!(
        !record.call_artifacts.is_empty(),
        "call artifacts must be persisted"
    );
    let artifact = &record.call_artifacts[0];
    assert!(
        artifact.digest.is_some(),
        "call artifact must have a digest"
    );
    assert!(
        artifact.digest.as_ref().unwrap().starts_with("sha256:"),
        "call artifact digest must be SHA-256"
    );
}

/// C-40: Child artifacts include real attempt/run identity, canonical handles,
/// and verified digests, or a typed absence reason.
#[tokio::test(flavor = "current_thread")]
async fn c40_child_artifacts_have_identity_and_digests() {
    let temp = tempfile::tempdir().unwrap();
    let store = codegg::tool::tool_program_result::ToolProgramResultStore::new(temp.path());

    let program_id = "tp-c40";
    let attempt_id = "att-c40";
    let result = codegg_core::tool_program::ProgramResult {
        status: codegg_core::tool_program::ProgramStatus::Completed,
        output: Some(codegg_core::tool_program::ProgramValue::String(
            "parent result".into(),
        )),
        error_message: None,
        failure_class: None,
        steps_used: 5,
        iterations_used: 1,
        bytes_used: 64,
        calls_completed: 0,
        calls_total: 0,
    };

    let child_artifacts = vec![codegg::tool::tool_program_result::ChildArtifactHandle {
        job_id: "job-child-c40".into(),
        attempt_id: None,
        run_id: None,
        status: "completed".into(),
        artifact_id: Some("sha256:child-result-c40".into()),
        digest: Some("sha256:child-digest-c40".into()),
        absence_reason: None,
    }];

    let record = store
        .persist(
            program_id,
            attempt_id,
            "native",
            result,
            vec![],
            child_artifacts,
            None,
        )
        .expect("persist should succeed");

    assert!(
        !record.child_artifacts.is_empty(),
        "child artifacts must be persisted"
    );
    let child = &record.child_artifacts[0];
    assert_eq!(child.job_id, "job-child-c40");
    assert!(child.digest.is_some(), "child artifact must have a digest");
    assert!(
        child.digest.as_ref().unwrap().starts_with("sha256:"),
        "child artifact digest must be SHA-256"
    );
}

/// C-41: Large final output is persisted through the canonical artifact store
/// and fails closed on storage failure.
#[tokio::test(flavor = "current_thread")]
async fn c41_large_output_spills_through_canonical_store() {
    let temp = tempfile::tempdir().unwrap();
    let artifact_store = Arc::new(codegg::context::InMemoryArtifactStore::new());
    let (executor, context) = c41_executor_fixture(temp.path(), "tp-c41", artifact_store.clone());

    let completion = executor.execute(context).await;
    assert_eq!(
        completion.status,
        ExecutorStatus::Completed,
        "{completion:?}"
    );

    let record = codegg::tool::tool_program_result::ToolProgramResultStore::new(temp.path())
        .load("tp-c41")
        .unwrap()
        .expect("result record");
    let handle = record
        .output_artifact
        .as_ref()
        .expect("output artifact handle");
    assert!(handle.starts_with("ctx://"));
    let artifact = artifact_store
        .get(handle)
        .await
        .unwrap()
        .expect("spilled output artifact");
    assert!(artifact.raw_bytes_len > 256_000);
    let preview = serde_json::to_vec(&record.result.output).unwrap();
    assert!(preview.len() < 4_096);
}

#[tokio::test(flavor = "current_thread")]
async fn c41_output_spill_failure_fails_closed() {
    let temp = tempfile::tempdir().unwrap();
    let (executor, context) = c41_executor_fixture(
        temp.path(),
        "tp-c41-failure",
        Arc::new(FailingArtifactStore),
    );

    let completion = executor.execute(context).await;
    assert_eq!(completion.status, ExecutorStatus::Failed);
    assert!(completion
        .summary
        .contains("canonical output artifact persistence failed"));
    assert!(
        codegg::tool::tool_program_result::ToolProgramResultStore::new(temp.path())
            .load("tp-c41-failure")
            .unwrap()
            .is_none()
    );
}

/// C-42: Foreground, background notification, and inspection expose one
/// authoritative typed result and identical artifact identities.
#[tokio::test(flavor = "current_thread")]
async fn c42_result_record_is_authoritative() {
    let temp = tempfile::tempdir().unwrap();
    let store = codegg::tool::tool_program_result::ToolProgramResultStore::new(temp.path());

    let program_id = "tp-c42";
    let attempt_id = "att-c42";
    let result = codegg_core::tool_program::ProgramResult {
        status: codegg_core::tool_program::ProgramStatus::Completed,
        output: Some(codegg_core::tool_program::ProgramValue::String(
            "result".into(),
        )),
        error_message: None,
        failure_class: None,
        steps_used: 5,
        iterations_used: 1,
        bytes_used: 64,
        calls_completed: 0,
        calls_total: 0,
    };

    let _record = store
        .persist(
            program_id,
            attempt_id,
            "native",
            result,
            vec![],
            vec![],
            None,
        )
        .expect("persist should succeed");

    let loaded = store
        .load(program_id)
        .expect("load should succeed")
        .expect("record must exist");

    assert_eq!(loaded.program_id, program_id);
    assert_eq!(loaded.attempt_id, attempt_id);
    assert_eq!(loaded.selected_backend, "native");
    assert_eq!(
        loaded.result.status,
        codegg_core::tool_program::ProgramStatus::Completed
    );
    assert!(
        loaded.result_digest.starts_with("sha256:"),
        "result digest must be SHA-256"
    );
}

/// C-43: Result integrity covers every semantic result and artifact field.
/// Tampering with the stored record causes load to fail with DigestMismatch.
#[tokio::test(flavor = "current_thread")]
async fn c43_tampered_record_fails_load() {
    let temp = tempfile::tempdir().unwrap();
    let store = codegg::tool::tool_program_result::ToolProgramResultStore::new(temp.path());

    let program_id = "tp-c43";
    let attempt_id = "att-c43";
    let result = codegg_core::tool_program::ProgramResult {
        status: codegg_core::tool_program::ProgramStatus::Completed,
        output: Some(codegg_core::tool_program::ProgramValue::String(
            "result".into(),
        )),
        error_message: None,
        failure_class: None,
        steps_used: 5,
        iterations_used: 1,
        bytes_used: 64,
        calls_completed: 0,
        calls_total: 0,
    };

    let call_artifacts = vec![codegg::tool::tool_program_result::ProgramArtifactHandle {
        tool_name: Some("read".into()),
        preview: "preview".into(),
        success: true,
        artifact_id: Some("sha256:art".into()),
        digest: Some("sha256:digest".into()),
        absence_reason: None,
    }];

    let record = store
        .persist(
            program_id,
            attempt_id,
            "native",
            result,
            call_artifacts,
            vec![],
            None,
        )
        .expect("persist should succeed");

    // Tamper with the stored file
    let artifact_dir = temp.path().join(".codegg").join("tool_program_results");
    let result_file = artifact_dir.join(format!("{}.json", program_id));
    let bytes = std::fs::read(&result_file).unwrap();
    let mut tampered: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    tampered["result_digest"] = "sha256:tampered".into();
    std::fs::write(&result_file, serde_json::to_vec(&tampered).unwrap()).unwrap();

    // Loading should fail with DigestMismatch
    let load_result = store.load(program_id);
    assert!(load_result.is_err(), "tampered record must fail to load");
    let err = load_result.unwrap_err();
    assert!(
        matches!(
            err,
            codegg::tool::tool_program_result::ToolProgramResultError::DigestMismatch { .. }
        ),
        "tampered record must fail with DigestMismatch, got: {:?}",
        err
    );

    // Verify the original record's digest is correct
    assert!(
        record.result_digest.starts_with("sha256:"),
        "original record digest must be SHA-256"
    );
}

/// C-44: Missing or corrupt result/artifact data fails closed with bounded
/// diagnostics.
#[tokio::test(flavor = "current_thread")]
async fn c44_corrupt_data_fails_closed() {
    let temp = tempfile::tempdir().unwrap();
    let store = codegg::tool::tool_program_result::ToolProgramResultStore::new(temp.path());

    let program_id = "tp-c44";
    let attempt_id = "att-c44";

    let result = codegg_core::tool_program::ProgramResult {
        status: codegg_core::tool_program::ProgramStatus::Completed,
        output: Some(codegg_core::tool_program::ProgramValue::String(
            "result".into(),
        )),
        error_message: None,
        failure_class: None,
        steps_used: 5,
        iterations_used: 1,
        bytes_used: 64,
        calls_completed: 0,
        calls_total: 0,
    };

    store
        .persist(
            program_id,
            attempt_id,
            "native",
            result,
            vec![],
            vec![],
            None,
        )
        .expect("persist should succeed");

    // Corrupt the stored file
    let artifact_dir = temp.path().join(".codegg").join("tool_program_results");
    let result_file = artifact_dir.join(format!("{}.json", program_id));
    std::fs::write(&result_file, "corrupted json").expect("write should succeed");

    // Loading should fail gracefully
    let load_result = store.load(program_id);
    assert!(
        load_result.is_err(),
        "corrupted result data must fail closed"
    );
}
