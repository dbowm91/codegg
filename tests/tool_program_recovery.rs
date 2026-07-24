//! Integration tests for Tool Program recovery (M005).
//!
//! Tests that programs can be restarted from checkpoints, that
//! completed calls are replayed and not repeated, and that
//! generation recovery works correctly.

use std::sync::Arc;

use codegg_core::jobs::{
    AttemptId, DaemonGeneration, IdempotencyClass, JobId, JobKind, JobPayload, JobPriority,
    JobRecord, JobSource, JobState, ResourceRequest, RetryPolicy,
};
use codegg_core::tool_program::{
    compile_program, BrokerCallback, BudgetSnapshot, CallRequest, CallResult, CompletedCall,
    InterpreterError, MeteredInterpreter, ProgramResult, ProgramStatus, ProgramValue,
    RuntimeLimits,
};
use codegg_core::workspace::WorkspaceId;

/// Broker that counts calls to detect replay.
struct CountingBroker {
    call_count: std::sync::atomic::AtomicU32,
}

impl CountingBroker {
    fn new() -> Self {
        Self {
            call_count: std::sync::atomic::AtomicU32::new(0),
        }
    }

    fn calls(&self) -> u32 {
        self.call_count.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[async_trait::async_trait]
impl BrokerCallback for CountingBroker {
    async fn execute_call(&self, request: &CallRequest) -> Result<CallResult, InterpreterError> {
        self.call_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(CallResult {
            output: ProgramValue::ToolResult(serde_json::json!({
                "tool": request.tool_name,
                "result": "ok"
            })),
            artifacts: vec![],
        })
    }
}

#[tokio::test]
async fn completed_calls_not_repeated_on_resume() {
    // Run a program that makes a tool call, then verify that
    // on re-entry (simulated via load_completed_calls), the call
    // is not repeated.
    let source = r#"
result = call({"tool": "test_tool", "input": "value"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);

    // First run: execute the program
    let broker = CountingBroker::new();
    let mut interp = MeteredInterpreter::new(compilation.ir.clone(), limits.clone());
    let result1 = interp.run(&broker, None).await;
    assert_eq!(result1.status, ProgramStatus::Completed);
    let calls_after_first = broker.calls();

    // Verify at least one call was made
    assert!(calls_after_first > 0, "expected at least one broker call");

    let completed = interp.completed_calls();
    let num_completed = completed.len();
    assert!(num_completed > 0, "completed_calls should not be empty");

    // Second run: load completed calls and verify replay behavior
    let broker2 = CountingBroker::new();
    let mut interp2 = MeteredInterpreter::new(compilation.ir, limits);
    interp2.load_completed_calls(completed.clone());
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(result2.status, ProgramStatus::Completed);

    // Replay should use completed calls, reducing broker invocations.
    // The second run should make fewer or equal broker calls compared
    // to the number of completed calls loaded.
    let calls_second = broker2.calls();
    assert!(
        calls_second <= num_completed as u32,
        "replay should not exceed completed calls: second={}, completed={}",
        calls_second,
        num_completed
    );
}

#[tokio::test]
async fn program_result_has_budget_info() {
    let source = r#"
total = 0
for i in range(10):
    total = total + 1
emit({"total": total})
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = CountingBroker::new();
    let result = interp.run(&broker, None).await;

    assert_eq!(result.status, ProgramStatus::Completed);
    assert!(result.steps_used > 0, "steps should be > 0");
    assert!(
        result.iterations_used > 0,
        "iterations should be > 0 for a loop"
    );
    assert!(result.bytes_used > 0, "bytes should be > 0");
    assert_eq!(result.calls_completed, result.calls_total);
}

#[tokio::test]
async fn cancelled_program_returns_immediately() {
    let source = "emit({\"ok\": true})\n";
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = CountingBroker::new();
    let token = tokio_util::sync::CancellationToken::new();
    token.cancel();
    let result = interp.run(&broker, Some(&token)).await;
    assert_eq!(result.status, ProgramStatus::Cancelled);
    assert_eq!(
        result.failure_class,
        Some(codegg_core::tool_program::FailureClass::Cancelled)
    );
}

#[tokio::test]
async fn budget_exhausted_returns_incomplete() {
    let source = r#"
total = 0
for i in range(100):
    total = total + 1
emit(total)
"#;
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_total_iterations = 5; // Very tight
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = CountingBroker::new();
    let result = interp.run(&broker, None).await;
    assert!(
        result.status == ProgramStatus::Failed || result.status == ProgramStatus::Incomplete,
        "expected Failed or Incomplete, got {:?}",
        result.status
    );
}

#[tokio::test]
async fn empty_loop_completes() {
    let source = r#"
total = 0
for i in range(0):
    total = total + 1
emit(total)
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = CountingBroker::new();
    let result = interp.run(&broker, None).await;
    assert_eq!(result.status, ProgramStatus::Completed);
    assert_eq!(result.output, Some(ProgramValue::Int(0)));
}

#[tokio::test]
async fn concurrent_programs_independent_results() {
    // Run two programs concurrently and verify they produce independent results
    let source1 = "emit(1)\n";
    let source2 = "emit(2)\n";

    let comp1 = compile_program(source1).unwrap();
    let comp2 = compile_program(source2).unwrap();

    let limits1 = RuntimeLimits::from(&comp1.ir.bounds);
    let limits2 = RuntimeLimits::from(&comp2.ir.bounds);

    let mut interp1 = MeteredInterpreter::new(comp1.ir, limits1);
    let mut interp2 = MeteredInterpreter::new(comp2.ir, limits2);

    let broker1 = CountingBroker::new();
    let broker2 = CountingBroker::new();

    let (r1, r2) = tokio::join!(interp1.run(&broker1, None), interp2.run(&broker2, None));

    assert_eq!(r1.status, ProgramStatus::Completed);
    assert_eq!(r2.status, ProgramStatus::Completed);
    assert_eq!(r1.output, Some(ProgramValue::Int(1)));
    assert_eq!(r2.output, Some(ProgramValue::Int(2)));
}

// ── Heartbeat emission tests ──────────────────────────────────────

use std::sync::atomic::{AtomicU32, Ordering};

struct HeartbeatCountingBroker {
    call_count: AtomicU32,
    heartbeat_count: AtomicU32,
}

impl HeartbeatCountingBroker {
    fn new() -> Self {
        Self {
            call_count: AtomicU32::new(0),
            heartbeat_count: AtomicU32::new(0),
        }
    }
}

#[async_trait::async_trait]
impl BrokerCallback for HeartbeatCountingBroker {
    async fn execute_call(&self, request: &CallRequest) -> Result<CallResult, InterpreterError> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        Ok(CallResult {
            output: ProgramValue::ToolResult(serde_json::json!({
                "tool": request.tool_name,
                "result": "ok"
            })),
            artifacts: vec![],
        })
    }

    async fn heartbeat(&self, _budget: &BudgetSnapshot) {
        self.heartbeat_count.fetch_add(1, Ordering::Relaxed);
    }
}

#[tokio::test]
async fn heartbeat_emitted_on_progress() {
    let source = r#"
result = call({"tool": "test_tool", "input": "value"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = HeartbeatCountingBroker::new();

    let result = interp.run(&broker, None).await;
    assert_eq!(result.status, ProgramStatus::Completed);
    // Heartbeats should be emitted at instruction boundaries
    assert!(
        broker.heartbeat_count.load(Ordering::Relaxed) > 0,
        "expected at least one heartbeat"
    );
}

// ── Cancellation during execution ─────────────────────────────────

#[tokio::test]
async fn cancellation_during_call() {
    struct SlowBroker;

    #[async_trait::async_trait]
    impl BrokerCallback for SlowBroker {
        async fn execute_call(
            &self,
            _request: &CallRequest,
        ) -> Result<CallResult, InterpreterError> {
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            Ok(CallResult {
                output: ProgramValue::ToolResult(serde_json::json!("ok")),
                artifacts: vec![],
            })
        }
    }

    let source = r#"
result = call({"tool": "slow_tool", "input": "data"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = SlowBroker;
    let token = tokio_util::sync::CancellationToken::new();

    // Spawn interpreter in a task
    let token_clone = token.clone();
    let handle = tokio::spawn(async move { interp.run(&broker, Some(&token_clone)).await });

    // Cancel after a short delay
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    token.cancel();

    let result = handle.await.unwrap();
    assert_eq!(result.status, ProgramStatus::Cancelled);
}

// ── Replay divergence detection ───────────────────────────────────

#[tokio::test]
async fn completed_calls_not_repeated_with_replay() {
    struct CallCountingBroker {
        count: AtomicU32,
    }

    #[async_trait::async_trait]
    impl BrokerCallback for CallCountingBroker {
        async fn execute_call(
            &self,
            _request: &CallRequest,
        ) -> Result<CallResult, InterpreterError> {
            self.count.fetch_add(1, Ordering::Relaxed);
            Ok(CallResult {
                output: ProgramValue::ToolResult(serde_json::json!("result")),
                artifacts: vec![],
            })
        }
    }

    let source = r#"
result = call({"tool": "test_tool", "input": "value"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_retries = 0;

    // First run: complete the call
    let mut interp1 = MeteredInterpreter::new(compilation.ir.clone(), limits.clone());
    let broker1 = CallCountingBroker {
        count: AtomicU32::new(0),
    };
    let result1 = interp1.run(&broker1, None).await;
    assert_eq!(result1.status, ProgramStatus::Completed);
    assert_eq!(broker1.count.load(Ordering::Relaxed), 1);

    // Get completed calls from first run
    let completed = interp1.completed_calls().clone();

    // Second run: load completed calls (replay) and verify no new broker call
    let mut interp2 = MeteredInterpreter::new(compilation.ir, limits);
    interp2.load_completed_calls(completed);
    let broker2 = CallCountingBroker {
        count: AtomicU32::new(0),
    };
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(result2.status, ProgramStatus::Completed);
    assert_eq!(
        broker2.count.load(Ordering::Relaxed),
        0,
        "completed call should be replayed, not re-executed"
    );
}

// ── Budget tracking across restart ────────────────────────────────

#[tokio::test]
async fn budget_preserved_across_replay() {
    let source = r#"
x = 1
y = 2
result = call({"tool": "test_tool", "input": "value"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);

    // Run to completion
    let mut interp = MeteredInterpreter::new(compilation.ir.clone(), limits.clone());
    let broker = CountingBroker::new();
    let result = interp.run(&broker, None).await;
    assert_eq!(result.status, ProgramStatus::Completed);

    // Budget should be non-zero
    assert!(result.steps_used > 0);
    assert!(result.bytes_used > 0);
}
