//! M011 production-correctness contract tests.

use codegg::context::{ArtifactKind, ContextArtifact, ContextArtifactStore, FileArtifactStore};
use codegg::scheduler::tool_program_notifications::ToolProgramNotificationService;
use codegg::tool::tool_program_context::stable_digest;
use codegg::tool::tool_program_ledger::ToolProgramLedger;
use codegg::tool::tool_program_result::{result_to_json, ToolProgramResultStore};
use codegg_core::tool_program::{
    CallRequest, CallResult, ProgramResult, ProgramStatus, ProgramValue,
};
use tempfile::TempDir;

#[test]
fn invocation_identity_is_stable_and_source_independent() {
    let first = stable_digest("session:s1:turn:t1:call:c1");
    let retry = stable_digest("session:s1:turn:t1:call:c1");
    let different_call = stable_digest("session:s1:turn:t1:call:c2");
    assert_eq!(first, retry);
    assert_ne!(first, different_call);
}

#[test]
fn journal_reservation_and_completion_are_request_bound() {
    let workspace = TempDir::new().unwrap();
    let ledger = ToolProgramLedger::new(workspace.path());
    let request = CallRequest {
        tool_name: "read".into(),
        input: serde_json::json!({"path": "Cargo.toml"}),
        call_id: Some("pc:0".into()),
    };
    ledger.reserve_call("tp-m011", 0, &request).unwrap();
    assert!(ledger.load_completed_calls("tp-m011").unwrap().is_empty());

    let completed = codegg_core::tool_program::CompletedCall {
        sequence: 0,
        request: request.clone(),
        result: CallResult {
            output: ProgramValue::String("ok".into()),
            artifacts: vec![],
            success: true,
        },
        replay_fingerprint: None,
    };
    ledger
        .persist_call_completion("tp-m011", &completed)
        .unwrap();
    assert_eq!(ledger.load_completed_calls("tp-m011").unwrap().len(), 1);

    let divergent = CallRequest {
        input: serde_json::json!({"path": "other.txt"}),
        ..request
    };
    assert!(ledger.reserve_call("tp-m011", 0, &divergent).is_err());
}

#[test]
fn typed_result_projection_does_not_parse_executor_summary() {
    let workspace = TempDir::new().unwrap();
    let result = ProgramResult {
        status: ProgramStatus::Completed,
        output: Some(ProgramValue::String("result".into())),
        error_message: None,
        failure_class: None,
        steps_used: 7,
        bytes_used: 11,
        calls_completed: 2,
        calls_total: 2,
        iterations_used: 1,
    };
    let record = ToolProgramResultStore::new(workspace.path())
        .persist(
            "tp-result",
            "attempt-1",
            "native",
            result,
            vec![],
            vec![],
            None,
        )
        .unwrap();
    let projected = result_to_json(&record);
    assert_eq!(projected["steps_used"], 7);
    assert_eq!(projected["calls_completed"], 2);
    assert_eq!(projected["selected_backend"], "native");
}

#[tokio::test(flavor = "current_thread")]
async fn terminal_notification_is_created_once_from_typed_result() {
    let service = ToolProgramNotificationService::new();
    let result = ProgramResult {
        status: ProgramStatus::Completed,
        output: None,
        error_message: None,
        failure_class: None,
        steps_used: 1,
        bytes_used: 0,
        calls_completed: 0,
        calls_total: 0,
        iterations_used: 0,
    };
    let workspace = TempDir::new().unwrap();
    let record = ToolProgramResultStore::new(workspace.path())
        .persist(
            "tp-notify",
            "attempt-1",
            "native",
            result,
            vec![],
            vec![],
            None,
        )
        .unwrap();
    service
        .record_terminal_result(
            "tp-notify",
            "job-1",
            Some("session-1"),
            Some("agent-1"),
            Some("turn-1"),
            &record,
        )
        .await;
    service
        .record_terminal_result(
            "tp-notify",
            "job-1",
            Some("session-1"),
            Some("agent-1"),
            Some("turn-1"),
            &record,
        )
        .await;
    let pending = service.pending_for_session("session-1").await;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].program_id, "tp-notify");
    assert!(service.claim("tp-notify").await.unwrap());
    assert!(service.acknowledge("tp-notify").await.unwrap());
}

#[tokio::test(flavor = "current_thread")]
async fn artifact_handle_resolves_after_store_reopen() {
    let workspace = TempDir::new().unwrap();
    let handle = codegg::context::ContextHandle::build_tool("session-1", 0, "call-1").unwrap();
    let artifact = ContextArtifact {
        handle: handle.clone(),
        session_id: "session-1".into(),
        turn_index: 0,
        tool_call_id: Some("call-1".into()),
        tool_name: Some("read".into()),
        kind: ArtifactKind::ToolResult,
        created_at_ms: 1,
        content_hash: codegg::context::compute_content_hash("bounded output"),
        redacted_content: "bounded output".into(),
        raw_bytes_len: 14,
        estimated_tokens: 2,
    };
    FileArtifactStore::new(workspace.path())
        .put(artifact)
        .await
        .unwrap();
    let reopened = FileArtifactStore::new(workspace.path())
        .get(&handle)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened.redacted_content, "bounded output");
}
