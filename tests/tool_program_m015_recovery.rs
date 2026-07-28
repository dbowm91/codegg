//! M015 monotonic replay merge and active-child recovery invariants.

use codegg_core::tool_program::{
    compile_program, CallRequest, CallResult, CompletedCall, InterpreterCheckpoint,
    MeteredInterpreter, PendingChildWait, ProgramValue, RuntimeLimits,
};
use std::collections::HashMap;

fn completed(sequence: u32, value: &str) -> CompletedCall {
    CompletedCall {
        sequence,
        request: CallRequest {
            tool_name: "read".into(),
            input: serde_json::json!({"path": format!("{sequence}.txt")}),
            call_id: Some(format!("call-{sequence}")),
        },
        result: CallResult {
            output: ProgramValue::String(value.into()),
            artifacts: vec![],
            success: true,
        },
        replay_fingerprint: None,
    }
}

fn checkpoint(calls: Vec<CompletedCall>) -> InterpreterCheckpoint {
    InterpreterCheckpoint {
        pc: 0,
        steps: 0,
        iterations: 0,
        calls_completed: calls.len() as u32,
        bytes_used: 0,
        parallel_groups: 0,
        locals: vec![],
        stack: vec![],
        pending_child_wait: None,
        original_deadline_millis: Some(4_102_444_800_000),
        checkpoint_sequence: 1,
        created_at_millis: chrono::Utc::now().timestamp_millis(),
        semantic_digest: String::new(),
        completed_calls: calls,
        locals_hash: String::new(),
    }
}

fn interpreter() -> MeteredInterpreter {
    let compiled = compile_program("emit('done')").unwrap();
    MeteredInterpreter::new(compiled.ir, RuntimeLimits::default())
}

#[test]
fn newer_durable_completion_survives_older_checkpoint() {
    let mut interpreter = interpreter();
    interpreter.load_completed_calls(HashMap::from([
        (0, completed(0, "zero")),
        (2, completed(2, "newer")),
    ]));
    interpreter
        .restore_checkpoint(checkpoint(vec![completed(0, "zero")]))
        .unwrap();

    assert_eq!(interpreter.completed_calls().len(), 2);
    assert_eq!(interpreter.next_call_seq(), 3);
}

#[test]
fn conflicting_completion_fails_closed() {
    let mut interpreter = interpreter();
    interpreter.load_completed_calls(HashMap::from([(0, completed(0, "durable"))]));
    let error = interpreter
        .restore_checkpoint(checkpoint(vec![completed(0, "checkpoint")]))
        .unwrap_err();
    assert!(error.to_string().contains("replay divergence"));
}

#[test]
fn sparse_valid_merge_advances_sequence_monotonically() {
    let mut interpreter = interpreter();
    interpreter.load_completed_calls(HashMap::from([(4, completed(4, "four"))]));
    interpreter
        .restore_checkpoint(checkpoint(vec![completed(1, "one")]))
        .unwrap();
    assert_eq!(interpreter.next_call_seq(), 5);
}

#[test]
fn pending_child_checkpoint_carries_reattachment_identity_and_deadline() {
    let mut checkpoint = checkpoint(vec![]);
    checkpoint.pending_child_wait = Some(PendingChildWait {
        child_job_id: "job-child".into(),
        expected_result_slot: 7,
        child_op: "test".into(),
        parent_program_id: "program-parent".into(),
        parent_job_id: "job-parent".into(),
        parent_attempt_id: "attempt-parent".into(),
        canonical_call_id: "call:program-parent:7".into(),
        instruction_sequence: 7,
        operation_config_digest: "sha256:config".into(),
    });
    let encoded = serde_json::to_string(&checkpoint).unwrap();
    let restored: InterpreterCheckpoint = serde_json::from_str(&encoded).unwrap();
    let pending = restored.pending_child_wait.unwrap();
    assert_eq!(pending.child_job_id, "job-child");
    assert_eq!(pending.instruction_sequence, 7);
    assert_eq!(restored.original_deadline_millis, Some(4_102_444_800_000));
}

#[test]
fn overlapping_ledger_writers_preserve_all_completed_calls() {
    let temp = tempfile::tempdir().unwrap();
    let ledger = codegg::tool::tool_program_ledger::ToolProgramLedger::new(temp.path());
    let program = "tp-m015-overlap";
    std::thread::scope(|scope| {
        for sequence in 0..8 {
            let ledger = ledger.clone();
            scope.spawn(move || {
                ledger
                    .record_completed_call(program, &completed(sequence, "ok"))
                    .unwrap();
            });
        }
    });
    assert_eq!(ledger.load_completed_calls(program).unwrap().len(), 8);
}
