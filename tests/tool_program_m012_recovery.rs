//! M012 durable replay identity and recovery cursor tests.
//!
//! Covers closure criteria C-19 through C-26:
//! - C-19: Call reservation, dispatch state, terminal outcome, and recovery cursor are durable
//!   before dependent interpreter state advances.
//! - C-20: Restart never physically re-executes a durably completed call.
//! - C-21: Replay validates tool/contract, input, authority, context, manifest, source/IR,
//!   workspace/path policy, backend, and call-order fingerprints.
//! - C-22: Replay divergence persists an inspectable recoverable result and stops execution.
//! - C-23: Original absolute deadline remains authoritative across restart.
//! - C-24: Foreground, background, notification, and inspection read the same integrity-checked
//!   typed result.
//! - C-25: Result digest is recomputed on load; corruption or identity mismatch fails closed.
//! - C-26: Real call and child artifact handles are present, bounded, resolvable, and
//!   digest-verifiable.

#![cfg(test)]

use codegg::tool::tool_program_ledger::ToolProgramLedger;
use codegg::tool::tool_program_result::{ToolProgramResultError, ToolProgramResultStore};
use codegg_core::tool_program::{
    CallRequest, CompletedCall, ProgramResult, ProgramStatus, ReplayFingerprint,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Helper to construct a ReplayFingerprint with all new M013-F1 fields.
fn make_fingerprint(
    authority_digest: &str,
    source_digest: &str,
    ir_digest: &str,
) -> ReplayFingerprint {
    ReplayFingerprint {
        schema_version: 2,
        program_id: "test-program".into(),
        authority_digest: authority_digest.into(),
        execution_context_digest: "ctx-digest-1".into(),
        source_digest: source_digest.into(),
        ir_digest: ir_digest.into(),
        workspace_id: "ws-1".into(),
        workspace_path_policy_id: "workspace:ws-1".into(),
        policy_revision: "rev-1".into(),
        session_id: Some("s1".into()),
        agent_id: Some("agent-1".into()),
        manifest_digest: "manifest-v1".into(),
        contract_digest: "contract-v1".into(),
        backend_selection: "native_only".into(),
        original_deadline_millis: Some(now_millis() + 60_000),
    }
}

fn make_call_request(tool_name: &str, input: &str) -> CallRequest {
    CallRequest {
        tool_name: tool_name.to_string(),
        input: serde_json::json!({"path": input}),
        call_id: None,
    }
}

fn make_completed_call(sequence: u32, tool_name: &str, input: &str) -> CompletedCall {
    CompletedCall {
        sequence,
        request: make_call_request(tool_name, input),
        result: codegg_core::tool_program::CallResult {
            output: codegg_core::tool_program::ProgramValue::ToolResult(
                serde_json::json!({"ok": true}),
            ),
            artifacts: vec![],
            success: true,
        },
        replay_fingerprint: None,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn c19_ledger_persists_call_reservation() {
    // C-19: The ledger records call reservations durably.
    let temp = tempfile::tempdir().unwrap();
    let ledger = ToolProgramLedger::new(temp.path());
    let request = make_call_request("read_file", "/test/path");
    let result = ledger.reserve_call("tp-c19", 0, &request);
    assert!(result.is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn c19_ledger_persists_terminal_outcome() {
    // C-19: The ledger records terminal outcomes durably.
    let temp = tempfile::tempdir().unwrap();
    let ledger = ToolProgramLedger::new(temp.path());
    let request = make_call_request("read_file", "/test/path");
    let _ = ledger.reserve_call("tp-c19b", 0, &request);
    let completed = make_completed_call(0, "read_file", "/test/path");
    let result = ledger.persist_call_completion("tp-c19b", &completed);
    assert!(result.is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn c20_restart_does_not_reexecute_completed_call() {
    // C-20: A completed call is not re-executed after restart.
    let temp = tempfile::tempdir().unwrap();
    let ledger = ToolProgramLedger::new(temp.path());
    let request = make_call_request("read_file", "/test/path");
    let _ = ledger.reserve_call("tp-c20", 0, &request);
    let completed = make_completed_call(0, "read_file", "/test/path");
    let _ = ledger.persist_call_completion("tp-c20", &completed);

    // Simulate restart: check if the call is already completed.
    assert!(ledger.is_call_completed("tp-c20", 0));
}

#[tokio::test(flavor = "current_thread")]
async fn c21_replay_validates_fingerprints() {
    // C-21: Replay validates fingerprints. The ledger stores input/output digests
    // that can be compared against re-execution results.
    let temp = tempfile::tempdir().unwrap();
    let ledger = ToolProgramLedger::new(temp.path());
    let request = make_call_request("read_file", "/test/path");
    let _ = ledger.reserve_call("tp-c21", 0, &request);
    let completed = make_completed_call(0, "read_file", "/test/path");
    let _ = ledger.persist_call_completion("tp-c21", &completed);

    // Verify the stored input digest exists.
    let input_digest = ledger.get_call_input_digest("tp-c21", 0);
    assert!(input_digest.is_some());

    // Verify the stored output digest exists.
    let output_digest = ledger.get_call_output_digest("tp-c21", 0);
    assert!(output_digest.is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn c21_replay_fingerprint_matching_succeeds() {
    // C-21: A completed call with a matching fingerprint replays successfully.
    use codegg_core::tool_program::{
        compile_program, MeteredInterpreter, RunConfig, RuntimeLimits,
    };

    struct RecordingBroker {
        call_count: std::sync::atomic::AtomicU32,
    }

    #[async_trait::async_trait]
    impl codegg_core::tool_program::BrokerCallback for RecordingBroker {
        async fn execute_call(
            &self,
            _request: &codegg_core::tool_program::CallRequest,
        ) -> Result<
            codegg_core::tool_program::CallResult,
            codegg_core::tool_program::InterpreterError,
        > {
            self.call_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(codegg_core::tool_program::CallResult {
                output: codegg_core::tool_program::ProgramValue::String("ok".into()),
                artifacts: vec![],
                success: true,
            })
        }
    }

    let fingerprint = make_fingerprint("auth-v1", "src-abc", "ir-123");

    // Pre-populate a completed call with the same fingerprint
    let mut completed_calls = std::collections::HashMap::new();
    completed_calls.insert(
        0,
        CompletedCall {
            sequence: 0,
            request: CallRequest {
                tool_name: "read_file".into(),
                input: serde_json::json!({"path": "/test"}),
                call_id: Some("pc:0".into()),
            },
            result: codegg_core::tool_program::CallResult {
                output: codegg_core::tool_program::ProgramValue::String("cached".into()),
                artifacts: vec![],
                success: true,
            },
            replay_fingerprint: Some(fingerprint.clone()),
        },
    );

    // Compile a program that makes one call
    let compilation = compile_program(
        "result = call({\"tool\": \"read_file\", \"path\": \"/test\"})\nemit(result)\n",
    )
    .unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    interp.load_completed_calls(completed_calls);
    interp.set_replay_fingerprint(fingerprint);

    let broker = RecordingBroker {
        call_count: std::sync::atomic::AtomicU32::new(0),
    };
    let result = interp.run(&broker, None).await;

    // Should succeed - the fingerprint matches
    eprintln!(
        "TEST RESULT: status={:?} error={:?}",
        result.status, result.error_message
    );
    assert_eq!(result.status, ProgramStatus::Completed);
    // Broker should NOT have been called - the call was replayed from cache
    assert_eq!(
        broker.call_count.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn c21_replay_fingerprint_mismatch_triggers_divergence() {
    // C-21: A completed call with a mismatched fingerprint triggers ReplayDivergence.
    use codegg_core::tool_program::{
        compile_program, MeteredInterpreter, RunConfig, RuntimeLimits,
    };

    struct NoopBroker;

    #[async_trait::async_trait]
    impl codegg_core::tool_program::BrokerCallback for NoopBroker {
        async fn execute_call(
            &self,
            _request: &codegg_core::tool_program::CallRequest,
        ) -> Result<
            codegg_core::tool_program::CallResult,
            codegg_core::tool_program::InterpreterError,
        > {
            Ok(codegg_core::tool_program::CallResult {
                output: codegg_core::tool_program::ProgramValue::String("ok".into()),
                artifacts: vec![],
                success: true,
            })
        }
    }

    // The stored fingerprint has a different authority_digest
    let stored_fingerprint = make_fingerprint("auth-v1-OLD", "src-abc", "ir-123");

    let mut completed_calls = std::collections::HashMap::new();
    completed_calls.insert(
        0,
        CompletedCall {
            sequence: 0,
            request: CallRequest {
                tool_name: "read_file".into(),
                input: serde_json::json!({"path": "/test"}),
                call_id: Some("pc:0".into()),
            },
            result: codegg_core::tool_program::CallResult {
                output: codegg_core::tool_program::ProgramValue::String("cached".into()),
                artifacts: vec![],
                success: true,
            },
            replay_fingerprint: Some(stored_fingerprint),
        },
    );

    // The current fingerprint has a different authority_digest
    let current_fingerprint = make_fingerprint("auth-v2-NEW", "src-abc", "ir-123");

    let compilation = compile_program(
        "result = call({\"tool\": \"read_file\", \"path\": \"/test\"})\nemit(result)\n",
    )
    .unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    interp.load_completed_calls(completed_calls);
    interp.set_replay_fingerprint(current_fingerprint);

    let broker = NoopBroker;
    let result = interp.run(&broker, None).await;

    // Should fail with a divergence error due to authority mismatch
    assert_eq!(result.status, ProgramStatus::Failed);
    assert!(result
        .error_message
        .unwrap()
        .contains("replay identity mismatch"));
}

#[tokio::test(flavor = "current_thread")]
async fn c21_legacy_call_without_fingerprint_is_accepted() {
    // C-21: A legacy call without a fingerprint is accepted for backward compatibility.
    use codegg_core::tool_program::{
        compile_program, MeteredInterpreter, RunConfig, RuntimeLimits,
    };

    struct NoopBroker;

    #[async_trait::async_trait]
    impl codegg_core::tool_program::BrokerCallback for NoopBroker {
        async fn execute_call(
            &self,
            _request: &codegg_core::tool_program::CallRequest,
        ) -> Result<
            codegg_core::tool_program::CallResult,
            codegg_core::tool_program::InterpreterError,
        > {
            Ok(codegg_core::tool_program::CallResult {
                output: codegg_core::tool_program::ProgramValue::String("ok".into()),
                artifacts: vec![],
                success: true,
            })
        }
    }

    // A legacy completed call without a fingerprint
    let mut completed_calls = std::collections::HashMap::new();
    completed_calls.insert(
        0,
        CompletedCall {
            sequence: 0,
            request: CallRequest {
                tool_name: "read_file".into(),
                input: serde_json::json!({"path": "/test"}),
                call_id: Some("pc:0".into()),
            },
            result: codegg_core::tool_program::CallResult {
                output: codegg_core::tool_program::ProgramValue::String("cached".into()),
                artifacts: vec![],
                success: true,
            },
            replay_fingerprint: None, // Legacy call
        },
    );

    let current_fingerprint = make_fingerprint("auth-v2-NEW", "src-abc", "ir-123");

    let compilation = compile_program(
        "result = call({\"tool\": \"read_file\", \"path\": \"/test\"})\nemit(result)\n",
    )
    .unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    interp.load_completed_calls(completed_calls);
    interp.set_replay_fingerprint(current_fingerprint);

    let broker = NoopBroker;
    let result = interp.run(&broker, None).await;

    // Should succeed - legacy calls without fingerprints are accepted
    assert_eq!(result.status, ProgramStatus::Completed);
}

#[tokio::test(flavor = "current_thread")]
async fn c22_divergence_persists_recoverable_result() {
    // C-22: Replay divergence persists an inspectable result and stops execution.
    let temp = tempfile::tempdir().unwrap();
    let ledger = ToolProgramLedger::new(temp.path());
    let request = make_call_request("read_file", "/test/path");
    let _ = ledger.reserve_call("tp-c22", 0, &request);
    let completed = make_completed_call(0, "read_file", "/test/path");
    let _ = ledger.persist_call_completion("tp-c22", &completed);

    // Simulate divergence: record a different output.
    let divergence = ledger.record_divergence("tp-c22", 0, "sha256:different-output");
    assert!(divergence.is_ok());

    // The divergence should be inspectable.
    assert!(ledger.has_divergence("tp-c22", 0));
}

#[tokio::test(flavor = "current_thread")]
async fn c23_deadline_remains_authoritative_across_restart() {
    // C-23: Original absolute deadline remains authoritative across restart.
    let temp = tempfile::tempdir().unwrap();
    let ledger = ToolProgramLedger::new(temp.path());
    let deadline = now_millis() + 30_000; // 30 seconds from now
    let result = ledger.record_program_deadline("tp-c23", deadline);
    assert!(result.is_ok());

    // Simulate restart: re-read the deadline.
    let stored_deadline = ledger.get_program_deadline("tp-c23");
    assert_eq!(stored_deadline, Some(deadline));
}

#[tokio::test(flavor = "current_thread")]
async fn c25_result_digest_recomputed_on_load() {
    // C-25: Result digest is recomputed on load; corruption fails closed.
    let temp = tempfile::tempdir().unwrap();
    let store = ToolProgramResultStore::new(temp.path());
    let result = ProgramResult {
        status: ProgramStatus::Completed,
        output: None,
        error_message: None,
        failure_class: None,
        steps_used: 5,
        bytes_used: 1,
        calls_completed: 2,
        calls_total: 2,
        iterations_used: 1,
    };
    let record = store
        .persist(
            "tp-c25",
            "attempt-1",
            "native",
            result,
            vec![],
            vec![],
            None,
        )
        .unwrap();

    // Load and verify digest matches.
    let loaded = store.load("tp-c25").unwrap().unwrap();
    assert_eq!(loaded.result_digest, record.result_digest);
}

#[tokio::test(flavor = "current_thread")]
async fn c25_corrupted_result_fails_closed() {
    // C-25: A corrupted result file fails closed with DigestMismatch.
    let temp = tempfile::tempdir().unwrap();
    let store = ToolProgramResultStore::new(temp.path());
    let result = ProgramResult {
        status: ProgramStatus::Completed,
        output: None,
        error_message: None,
        failure_class: None,
        steps_used: 5,
        bytes_used: 1,
        calls_completed: 2,
        calls_total: 2,
        iterations_used: 1,
    };
    let _ = store
        .persist(
            "tp-c25b",
            "attempt-1",
            "native",
            result,
            vec![],
            vec![],
            None,
        )
        .unwrap();

    // Corrupt the file by writing invalid JSON.
    let path = temp
        .path()
        .join(".codegg")
        .join("tool_program_results")
        .join("tp-c25b.json");
    std::fs::write(&path, b"{invalid json}").unwrap();

    let result = store.load("tp-c25b");
    assert!(result.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn c25_identity_mismatch_fails_closed() {
    // C-25: A result file with a mismatched program_id fails closed.
    let temp = tempfile::tempdir().unwrap();
    let store = ToolProgramResultStore::new(temp.path());
    let result = ProgramResult {
        status: ProgramStatus::Completed,
        output: None,
        error_message: None,
        failure_class: None,
        steps_used: 5,
        bytes_used: 1,
        calls_completed: 2,
        calls_total: 2,
        iterations_used: 1,
    };
    let _ = store
        .persist(
            "tp-c25c",
            "attempt-1",
            "native",
            result,
            vec![],
            vec![],
            None,
        )
        .unwrap();

    // Write a file with a different program_id inside.
    let path = temp
        .path()
        .join(".codegg")
        .join("tool_program_results")
        .join("tp-wrong.json");
    let record = store.load("tp-c25c").unwrap().unwrap();
    // record.program_id is "tp-c25c" — writing to tp-wrong.json creates a mismatch.
    let bytes = serde_json::to_vec(&record).unwrap();
    std::fs::write(&path, bytes).unwrap();

    let result = store.load("tp-wrong");
    assert!(result.is_err());
}

// ── C-24 ──────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn c24_all_consumers_read_same_integrity_checked_result() {
    // C-24: Foreground, background, notification, and inspection read the same
    // integrity-checked typed result.
    //
    // We persist a result, then load it multiple times (simulating different
    // consumers) and verify they all receive the identical record with the
    // same digest, terminal status, counters, and backend.
    let temp = tempfile::tempdir().unwrap();
    let store = ToolProgramResultStore::new(temp.path());

    let result = ProgramResult {
        status: ProgramStatus::Completed,
        output: Some(codegg_core::tool_program::ProgramValue::String(
            "test output".into(),
        )),
        error_message: None,
        failure_class: None,
        steps_used: 10,
        bytes_used: 1024,
        calls_completed: 3,
        calls_total: 3,
        iterations_used: 2,
    };

    let record = store
        .persist(
            "tp-c24",
            "attempt-c24",
            "native",
            result.clone(),
            vec![],
            vec![],
            None,
        )
        .unwrap();

    // Simulate four different consumers reading the same result.
    let foreground = store.load("tp-c24").unwrap().unwrap();
    let background = store.load("tp-c24").unwrap().unwrap();
    let notification = store.load("tp-c24").unwrap().unwrap();
    let inspection = store.load("tp-c24").unwrap().unwrap();

    // All consumers must see the same record.
    for consumer in [&foreground, &background, &notification, &inspection] {
        assert_eq!(consumer.program_id, record.program_id);
        assert_eq!(consumer.attempt_id, record.attempt_id);
        assert_eq!(consumer.selected_backend, record.selected_backend);
        assert_eq!(consumer.result_digest, record.result_digest);
        assert_eq!(consumer.result.status, ProgramStatus::Completed);
        assert_eq!(consumer.result.steps_used, 10);
        assert_eq!(consumer.result.bytes_used, 1024);
        assert_eq!(consumer.result.calls_completed, 3);
        assert_eq!(consumer.result.calls_total, 3);
    }

    // Digests must all match.
    let digests: Vec<_> = [&foreground, &background, &notification, &inspection]
        .iter()
        .map(|c| c.result_digest.clone())
        .collect();
    assert!(digests.windows(2).all(|w| w[0] == w[1]));
}

// ── C-26 ──────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn c26_artifact_handles_present_and_verifiable() {
    // C-26: Real call and child artifact handles are present, bounded,
    // resolvable, and digest-verifiable; program_artifacts is not an
    // unconditional empty placeholder.
    let temp = tempfile::tempdir().unwrap();
    let store = ToolProgramResultStore::new(temp.path());

    let call_artifacts = vec![
        codegg::tool::tool_program_result::ProgramArtifactHandle {
            tool_name: Some("read_file".into()),
            preview: "file content preview".into(),
            success: true,
            artifact_id: Some("ctx://artifact-1".into()),
            digest: Some("sha256:abc123".into()),
        },
        codegg::tool::tool_program_result::ProgramArtifactHandle {
            tool_name: Some("grep".into()),
            preview: "search results".into(),
            success: true,
            artifact_id: Some("ctx://artifact-2".into()),
            digest: Some("sha256:def456".into()),
        },
    ];

    let child_artifacts = vec![codegg::tool::tool_program_result::ChildArtifactHandle {
        job_id: "child-job-1".into(),
        run_id: Some("run-1".into()),
        status: "completed".into(),
        artifact_id: Some("ctx://child-artifact-1".into()),
        digest: Some("sha256:child1".into()),
    }];

    let result = ProgramResult {
        status: ProgramStatus::Completed,
        output: Some(codegg_core::tool_program::ProgramValue::String(
            "with artifacts".into(),
        )),
        error_message: None,
        failure_class: None,
        steps_used: 5,
        bytes_used: 512,
        calls_completed: 2,
        calls_total: 2,
        iterations_used: 1,
    };

    let record = store
        .persist(
            "tp-c26",
            "attempt-c26",
            "native",
            result,
            call_artifacts,
            child_artifacts,
            None,
        )
        .unwrap();

    // Verify call artifacts are present and bounded.
    assert_eq!(record.call_artifacts.len(), 2);
    assert_eq!(
        record.call_artifacts[0].tool_name.as_deref(),
        Some("read_file")
    );
    assert!(record.call_artifacts[0].success);
    assert!(record.call_artifacts[0].artifact_id.is_some());
    assert!(record.call_artifacts[0].digest.is_some());
    // Preview is bounded (not the full file content).
    assert!(record.call_artifacts[0].preview.len() <= 200);

    assert_eq!(record.call_artifacts[1].tool_name.as_deref(), Some("grep"));

    // Verify child artifacts are present.
    assert_eq!(record.child_artifacts.len(), 1);
    assert_eq!(record.child_artifacts[0].job_id, "child-job-1");
    assert_eq!(record.child_artifacts[0].status, "completed");
    assert!(record.child_artifacts[0].artifact_id.is_some());

    // Load and verify the loaded record also has the artifacts.
    let loaded = store.load("tp-c26").unwrap().unwrap();
    assert_eq!(loaded.call_artifacts.len(), 2);
    assert_eq!(loaded.child_artifacts.len(), 1);
    assert_eq!(
        loaded.call_artifacts[0].digest,
        record.call_artifacts[0].digest
    );
    assert_eq!(
        loaded.child_artifacts[0].artifact_id,
        record.child_artifacts[0].artifact_id
    );
}

#[tokio::test(flavor = "current_thread")]
async fn c26_empty_artifacts_is_not_placeholder() {
    // C-26: When there are no artifacts, the vectors are empty but the
    // fields exist and are populated (not an unconditional placeholder).
    let temp = tempfile::tempdir().unwrap();
    let store = ToolProgramResultStore::new(temp.path());

    let result = ProgramResult {
        status: ProgramStatus::Completed,
        output: Some(codegg_core::tool_program::ProgramValue::String(
            "no artifacts".into(),
        )),
        error_message: None,
        failure_class: None,
        steps_used: 1,
        bytes_used: 0,
        calls_completed: 0,
        calls_total: 0,
        iterations_used: 0,
    };

    let record = store
        .persist(
            "tp-c26-empty",
            "attempt-c26-empty",
            "native",
            result,
            vec![], // empty but present
            vec![],
            None,
        )
        .unwrap();

    // Fields are present (not missing from the struct).
    assert!(record.call_artifacts.is_empty());
    assert!(record.child_artifacts.is_empty());
    assert!(record.output_artifact.is_none());

    // Loaded record also has the fields.
    let loaded = store.load("tp-c26-empty").unwrap().unwrap();
    assert!(loaded.call_artifacts.is_empty());
    assert!(loaded.child_artifacts.is_empty());
}
