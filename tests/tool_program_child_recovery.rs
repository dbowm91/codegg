//! Integration tests for Tool Program child-job recovery (M007).
//!
//! Tests that child jobs survive restarts, completed calls are replayed
//! without duplicate execution, and idempotent submission prevents
//! double-dispatch.

use std::sync::atomic::{AtomicU32, Ordering};

use codegg_core::tool_program::{
    child_job::{ChildJobDetails, ChildJobRequest, ChildJobResult},
    compile_program, BrokerCallback, CallRequest, CallResult, InterpreterError, MeteredInterpreter,
    ProgramStatus, ProgramValue, RuntimeLimits,
};

// ── Test broker ────────────────────────────────────────────────────

/// Broker that tracks total submissions across multiple runs
/// (simulating restart) and returns configurable results.
struct RecoveryBroker {
    total_submissions: AtomicU32,
    results: std::sync::Mutex<Vec<ChildJobResult>>,
}

impl RecoveryBroker {
    fn new(results: Vec<ChildJobResult>) -> Self {
        Self {
            total_submissions: AtomicU32::new(0),
            results: std::sync::Mutex::new(results),
        }
    }

    fn total_submissions(&self) -> u32 {
        self.total_submissions.load(Ordering::Relaxed)
    }
}

#[async_trait::async_trait]
impl BrokerCallback for RecoveryBroker {
    async fn execute_call(&self, _request: &CallRequest) -> Result<CallResult, InterpreterError> {
        Ok(CallResult {
            output: ProgramValue::ToolResult(serde_json::json!({"status": "ok"})),
            artifacts: vec![],
        })
    }

    async fn submit_child_job(
        &self,
        _request: &ChildJobRequest,
    ) -> Result<ChildJobResult, InterpreterError> {
        self.total_submissions.fetch_add(1, Ordering::Relaxed);

        let mut results = self.results.lock().unwrap();
        if results.is_empty() {
            Err(InterpreterError::BrokerError("no more mock results".into()))
        } else {
            Ok(results.remove(0))
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────

fn make_limits() -> RuntimeLimits {
    RuntimeLimits {
        max_steps: 100_000,
        max_loop_iterations: 1000,
        max_total_iterations: 10_000,
        max_dynamic_calls: 100,
        max_parallel_width: 10,
        max_parallel_depth: 2,
        max_value_growth: 100_000,
        max_bytes: 400_000,
        max_inflight_calls: 10,
        max_wall_time_ms: 0,
        max_stall_time_ms: 0,
        max_per_call_time_ms: 0,
        max_retries: 0,
        retry_base_delay_ms: 100,
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn replay_completed_child_job_on_restart() {
    // Program has 2 child jobs. After first run completes both,
    // restart should replay both from completed_calls without
    // calling the broker again.
    let source = r#"
r1 = submit_job("test", {"scope": "workspace"})
r2 = submit_job("build", {"argv": ["cargo", "build"]})
emit({"r1": r1, "r2": r2})
"#;

    let compilation = compile_program(source).expect("compilation failed");
    let limits = make_limits();

    let broker = RecoveryBroker::new(vec![
        ChildJobResult {
            success: true,
            exit_code: Some(0),
            duration_ms: 100,
            details: ChildJobDetails::Test(
                codegg_core::tool_program::child_job::TestJobResult::default(),
            ),
            artifacts: vec![],
            error: None,
        },
        ChildJobResult {
            success: true,
            exit_code: Some(0),
            duration_ms: 200,
            details: ChildJobDetails::Build(
                codegg_core::tool_program::child_job::BuildJobResult::default(),
            ),
            artifacts: vec![],
            error: None,
        },
    ]);

    // First run: both child jobs execute
    let mut interp1 = MeteredInterpreter::new(compilation.ir.clone(), limits.clone());
    let result1 = interp1.run(&broker, None).await;
    assert_eq!(result1.status, ProgramStatus::Completed);
    assert_eq!(broker.total_submissions(), 2);

    // Get completed calls for replay
    let completed = interp1.completed_calls().clone();
    assert_eq!(completed.len(), 2);

    // Second run (restart): both should be replayed, no new submissions
    let submissions_before = broker.total_submissions();
    let mut interp2 = MeteredInterpreter::new(compilation.ir, limits);
    interp2.load_completed_calls(completed);
    let result2 = interp2.run(&broker, None).await;
    assert_eq!(result2.status, ProgramStatus::Completed);
    assert_eq!(broker.total_submissions(), submissions_before);
}

#[tokio::test]
async fn replay_preserves_child_job_result_values() {
    // The replayed results should have the same values as the originals
    let source = r#"
r = submit_job("test", {"scope": "package"})
emit(r)
"#;

    let compilation = compile_program(source).expect("compilation failed");
    let limits = make_limits();

    let broker = RecoveryBroker::new(vec![ChildJobResult {
        success: false,
        exit_code: Some(1),
        duration_ms: 5000,
        details: ChildJobDetails::Test(codegg_core::tool_program::child_job::TestJobResult {
            status: "failed".into(),
            framework: Some("cargo".into()),
            total: Some(10),
            passed: Some(7),
            failed: Some(3),
            skipped: Some(0),
            failed_tests: vec!["test_parse".into()],
            failure_evidence: vec!["assertion failed".into()],
            cancelled: false,
            timed_out: false,
        }),
        artifacts: vec!["ctx://logs/test-1".into()],
        error: None,
    }]);

    let mut interp1 = MeteredInterpreter::new(compilation.ir.clone(), limits.clone());
    let result1 = interp1.run(&broker, None).await;
    assert_eq!(result1.status, ProgramStatus::Completed);

    // Verify the original result
    match &result1.output {
        Some(ProgramValue::ToolResult(json)) => {
            assert_eq!(json["success"], false);
            assert_eq!(json["exit_code"], 1);
            assert_eq!(json["duration_ms"], 5000);
        }
        other => panic!("expected ToolResult, got {:?}", other),
    }

    // Restart and verify replay produces identical output
    let completed = interp1.completed_calls().clone();
    let mut interp2 = MeteredInterpreter::new(compilation.ir, limits);
    interp2.load_completed_calls(completed);
    let result2 = interp2.run(&broker, None).await;
    assert_eq!(result2.status, ProgramStatus::Completed);
    assert_eq!(result1.output, result2.output);
}

#[tokio::test]
async fn no_duplicate_execution_on_replay() {
    // A broker that counts invocations — on restart, the count
    // should not increase because completed calls are replayed.
    struct CountingBroker {
        call_count: AtomicU32,
        child_count: AtomicU32,
    }

    impl CountingBroker {
        fn new() -> Self {
            Self {
                call_count: AtomicU32::new(0),
                child_count: AtomicU32::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl BrokerCallback for CountingBroker {
        async fn execute_call(
            &self,
            _request: &CallRequest,
        ) -> Result<CallResult, InterpreterError> {
            self.call_count.fetch_add(1, Ordering::Relaxed);
            Ok(CallResult {
                output: ProgramValue::ToolResult(serde_json::json!({"ok": true})),
                artifacts: vec![],
            })
        }

        async fn submit_child_job(
            &self,
            _request: &ChildJobRequest,
        ) -> Result<ChildJobResult, InterpreterError> {
            self.child_count.fetch_add(1, Ordering::Relaxed);
            Ok(ChildJobResult {
                success: true,
                exit_code: Some(0),
                duration_ms: 50,
                details: ChildJobDetails::Test(
                    codegg_core::tool_program::child_job::TestJobResult::default(),
                ),
                artifacts: vec![],
                error: None,
            })
        }
    }

    let source = r#"
x = call({"tool": "read", "path": "/dev/null"})
r = submit_job("test", {})
emit(r)
"#;

    let compilation = compile_program(source).expect("compile");
    let limits = make_limits();

    let broker = CountingBroker::new();

    // First run
    let mut interp1 = MeteredInterpreter::new(compilation.ir.clone(), limits.clone());
    let _ = interp1.run(&broker, None).await;
    assert_eq!(broker.call_count.load(Ordering::Relaxed), 1);
    assert_eq!(broker.child_count.load(Ordering::Relaxed), 1);

    let completed = interp1.completed_calls().clone();

    // Second run (restart) — should replay, not re-execute
    let compilation2 = compile_program(source).expect("compile");
    let mut interp2 = MeteredInterpreter::new(compilation2.ir, limits);
    interp2.load_completed_calls(completed);
    let _ = interp2.run(&broker, None).await;

    // Counts should not have increased — replay doesn't touch broker
    assert_eq!(broker.call_count.load(Ordering::Relaxed), 1);
    assert_eq!(broker.child_count.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn completed_call_sequence_numbers_are_stable() {
    // Verify that completed call sequence numbers are deterministic
    // across runs with the same source
    let source = r#"
r1 = submit_job("test", {})
r2 = submit_job("build", {})
r3 = submit_job("lint", {})
emit(r3)
"#;

    let compilation = compile_program(source).expect("compile");
    let limits = make_limits();

    let broker = RecoveryBroker::new(vec![
        ChildJobResult {
            success: true,
            exit_code: Some(0),
            duration_ms: 50,
            details: ChildJobDetails::Test(
                codegg_core::tool_program::child_job::TestJobResult::default(),
            ),
            artifacts: vec![],
            error: None,
        },
        ChildJobResult {
            success: true,
            exit_code: Some(0),
            duration_ms: 50,
            details: ChildJobDetails::Build(
                codegg_core::tool_program::child_job::BuildJobResult::default(),
            ),
            artifacts: vec![],
            error: None,
        },
        ChildJobResult {
            success: true,
            exit_code: Some(0),
            duration_ms: 50,
            details: ChildJobDetails::Lint(
                codegg_core::tool_program::child_job::LintJobResult::default(),
            ),
            artifacts: vec![],
            error: None,
        },
    ]);

    let mut interp1 = MeteredInterpreter::new(compilation.ir.clone(), limits.clone());
    let result1 = interp1.run(&broker, None).await;
    assert_eq!(result1.status, ProgramStatus::Completed);

    let completed = interp1.completed_calls().clone();
    // Sequence numbers should be 0, 1, 2
    let mut seqs: Vec<u32> = completed.keys().copied().collect();
    seqs.sort();
    assert_eq!(seqs, vec![0, 1, 2]);

    // Replay with new broker should produce same result
    let broker2 = RecoveryBroker::new(vec![]);
    let mut interp2 = MeteredInterpreter::new(compilation.ir, limits);
    interp2.load_completed_calls(completed);
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(result2.status, ProgramStatus::Completed);
    assert_eq!(broker2.total_submissions(), 0);
}

#[tokio::test]
async fn single_child_job_restart() {
    // Simplest case: one child job, restart replays it
    let source = r#"
r = submit_job("format", {"argv": ["cargo", "fmt", "--check"]})
emit(r)
"#;

    let compilation = compile_program(source).expect("compile");
    let limits = make_limits();

    let broker = RecoveryBroker::new(vec![ChildJobResult {
        success: true,
        exit_code: Some(0),
        duration_ms: 1000,
        details: ChildJobDetails::Format(codegg_core::tool_program::child_job::FormatJobResult {
            status: "clean".into(),
            command: Some("cargo fmt --check".into()),
            would_change: false,
        }),
        artifacts: vec![],
        error: None,
    }]);

    let mut interp1 = MeteredInterpreter::new(compilation.ir.clone(), limits.clone());
    let result1 = interp1.run(&broker, None).await;
    assert_eq!(result1.status, ProgramStatus::Completed);

    let completed = interp1.completed_calls().clone();
    assert_eq!(completed.len(), 1);

    let broker2 = RecoveryBroker::new(vec![]);
    let mut interp2 = MeteredInterpreter::new(compilation.ir, limits);
    interp2.load_completed_calls(completed);
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(result2.status, ProgramStatus::Completed);
    assert_eq!(result1.output, result2.output);
    assert_eq!(broker2.total_submissions(), 0);
}
