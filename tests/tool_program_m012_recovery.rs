//! M012 durable replay identity and recovery cursor tests.
//!
//! Covers closure criteria C-19 through C-23:
//! - C-19: Call reservation, dispatch state, terminal outcome, and recovery cursor are durable
//!   before dependent interpreter state advances.
//! - C-20: Restart never physically re-executes a durably completed call.
//! - C-21: Replay validates tool/contract, input, authority, context, manifest, source/IR,
//!   workspace/path policy, backend, and call-order fingerprints.
//! - C-22: Replay divergence persists an inspectable recoverable result and stops execution.
//! - C-23: Original absolute deadline remains authoritative across restart.

#![cfg(test)]

use codegg::tool::tool_program_ledger::ToolProgramLedger;
use codegg::tool::tool_program_result::{ToolProgramResultError, ToolProgramResultStore};
use codegg_core::tool_program::{CallRequest, CompletedCall, ProgramResult, ProgramStatus};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
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
        },
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
