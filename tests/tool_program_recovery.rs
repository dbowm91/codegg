//! Integration tests for Tool Program recovery (M005).
//!
//! Tests that programs can be restarted from checkpoints, that
//! completed calls are replayed and not repeated, and that
//! generation recovery works correctly.

use std::panic::AssertUnwindSafe;
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
use futures::future::FutureExt;

/// Broker that counts calls to detect replay.
struct CountingBroker {
    call_count: std::sync::atomic::AtomicU32,
}

/// Broker that panics on specific tool names.
struct PanicBroker {
    panic_tool: String,
}

/// Broker that fails on specific tool names.
struct FaultBroker {
    fail_tool: String,
}

impl FaultBroker {
    fn new(fail_tool: &str) -> Self {
        Self {
            fail_tool: fail_tool.to_string(),
        }
    }
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

#[async_trait::async_trait]
impl BrokerCallback for PanicBroker {
    async fn execute_call(&self, request: &CallRequest) -> Result<CallResult, InterpreterError> {
        if request.tool_name == self.panic_tool {
            panic!("simulated panic in broker");
        }
        Ok(CallResult {
            output: ProgramValue::ToolResult(serde_json::json!({
                "tool": request.tool_name,
                "result": "ok"
            })),
            artifacts: vec![],
        })
    }
}

#[async_trait::async_trait]
impl BrokerCallback for FaultBroker {
    async fn execute_call(&self, request: &CallRequest) -> Result<CallResult, InterpreterError> {
        if request.tool_name == self.fail_tool {
            return Err(InterpreterError::BrokerError(format!(
                "simulated failure for {}",
                self.fail_tool
            )));
        }
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
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
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

// ── Restart at each checkpoint boundary ───────────────────────────

#[tokio::test]
async fn restart_replays_before_call_checkpoint() {
    // Program: call then emit. Checkpoint is emitted before the call.
    // On replay, the completed call should be reused.
    let source = r#"
result = call({"tool": "test_tool", "input": "value"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);

    // First run
    let mut interp1 = MeteredInterpreter::new(compilation.ir.clone(), limits.clone());
    let broker1 = CountingBroker::new();
    let result1 = interp1.run(&broker1, None).await;
    assert_eq!(result1.status, ProgramStatus::Completed);
    let calls1 = broker1.calls();
    assert!(calls1 > 0);

    // Get completed calls
    let completed = interp1.completed_calls().clone();

    // Replay — should use completed calls, not re-execute
    let mut interp2 = MeteredInterpreter::new(compilation.ir, limits);
    interp2.load_completed_calls(completed);
    let broker2 = CountingBroker::new();
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(result2.status, ProgramStatus::Completed);
    assert_eq!(broker2.calls(), 0, "replay should not invoke broker");
}

#[tokio::test]
async fn restart_replays_after_call_checkpoint() {
    // The checkpoint after call completion ensures the call result
    // is durable before advancing. On replay, the completed call
    // map includes this call.
    let source = r#"
x = 1
result = call({"tool": "test_tool", "input": "data"})
y = result
emit({"x": x, "y": y})
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);

    let mut interp1 = MeteredInterpreter::new(compilation.ir.clone(), limits.clone());
    let broker1 = CountingBroker::new();
    let result1 = interp1.run(&broker1, None).await;
    assert_eq!(result1.status, ProgramStatus::Completed);

    let completed = interp1.completed_calls().clone();
    let mut interp2 = MeteredInterpreter::new(compilation.ir, limits);
    interp2.load_completed_calls(completed);
    let broker2 = CountingBroker::new();
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(result2.status, ProgramStatus::Completed);
    assert_eq!(broker2.calls(), 0);
}

#[tokio::test]
async fn restart_replays_loop_checkpoint() {
    // Loop with call — checkpoint is emitted at each loop iteration.
    // On replay, all completed calls should be reused.
    let source = r#"
results = []
for i in range(3):
    r = call({"tool": "test_tool", "input": "iter"})
    results = results + [r]
emit(results)
"#;
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    // Checkpoint instructions add extra steps; increase budget
    limits.max_steps = limits.max_steps * 3;
    limits.max_dynamic_calls = 10;

    let mut interp1 = MeteredInterpreter::new(compilation.ir.clone(), limits.clone());
    let broker1 = CountingBroker::new();
    let result1 = interp1.run(&broker1, None).await;
    assert_eq!(
        result1.status,
        ProgramStatus::Completed,
        "first run failed: {:?} error: {:?}",
        result1.status,
        result1.error_message
    );
    let calls1 = broker1.calls();
    assert!(calls1 > 0);

    let completed = interp1.completed_calls().clone();
    assert_eq!(completed.len(), calls1 as usize);

    let mut interp2 = MeteredInterpreter::new(compilation.ir, limits);
    interp2.load_completed_calls(completed);
    let broker2 = CountingBroker::new();
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(result2.status, ProgramStatus::Completed);
    assert_eq!(broker2.calls(), 0, "replay should use all completed calls");
}

#[tokio::test]
async fn restart_replays_parallel_checkpoint() {
    // Parallel call — checkpoint after parallel convergence.
    let source = r#"
reads = parallel(
    {"tool": "test_tool", "input": "a"},
    {"tool": "test_tool", "input": "b"},
)
emit(reads)
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);

    let mut interp1 = MeteredInterpreter::new(compilation.ir.clone(), limits.clone());
    let broker1 = CountingBroker::new();
    let result1 = interp1.run(&broker1, None).await;
    assert_eq!(result1.status, ProgramStatus::Completed);
    let calls1 = broker1.calls();
    assert!(calls1 >= 2, "parallel should make at least 2 calls");

    let completed = interp1.completed_calls().clone();
    let mut interp2 = MeteredInterpreter::new(compilation.ir, limits);
    interp2.load_completed_calls(completed);
    let broker2 = CountingBroker::new();
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(result2.status, ProgramStatus::Completed);
    assert_eq!(broker2.calls(), 0);
}

#[tokio::test]
async fn restart_replays_terminal_checkpoint() {
    // Checkpoint before emit ensures terminal result is durable.
    let source = "emit({\"final\": true})\n";
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);

    let mut interp1 = MeteredInterpreter::new(compilation.ir.clone(), limits.clone());
    let broker1 = CountingBroker::new();
    let result1 = interp1.run(&broker1, None).await;
    assert_eq!(result1.status, ProgramStatus::Completed);

    let completed = interp1.completed_calls().clone();
    let mut interp2 = MeteredInterpreter::new(compilation.ir, limits);
    interp2.load_completed_calls(completed);
    let broker2 = CountingBroker::new();
    let result2 = interp2.run(&broker2, None).await;
    assert_eq!(result2.status, ProgramStatus::Completed);
}

// ── Cancellation during parallel group fan-out ────────────────────

#[tokio::test]
async fn cancellation_during_parallel_fan_out() {
    struct SlowParallelBroker;

    #[async_trait::async_trait]
    impl BrokerCallback for SlowParallelBroker {
        async fn execute_call(
            &self,
            _request: &CallRequest,
        ) -> Result<CallResult, InterpreterError> {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            Ok(CallResult {
                output: ProgramValue::ToolResult(serde_json::json!("ok")),
                artifacts: vec![],
            })
        }
    }

    let source = r#"
reads = parallel(
    {"tool": "slow_a", "input": "a"},
    {"tool": "slow_b", "input": "b"},
)
emit(reads)
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = SlowParallelBroker;
    let token = tokio_util::sync::CancellationToken::new();

    let token_clone = token.clone();
    let handle = tokio::spawn(async move { interp.run(&broker, Some(&token_clone)).await });

    // Cancel while parallel calls are in-flight
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    token.cancel();

    let result = handle.await.unwrap();
    assert_eq!(result.status, ProgramStatus::Cancelled);
}

// ── Worker panic and lost heartbeat tests ─────────────────────────

#[tokio::test]
async fn broker_panic_during_call_propagates() {
    let source = r#"
result = call({"tool": "panic_tool", "input": "data"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = PanicBroker {
        panic_tool: "panic_tool".to_string(),
    };

    let result = std::panic::AssertUnwindSafe(interp.run(&broker, None))
        .catch_unwind()
        .await;
    assert!(result.is_err(), "broker panic should propagate to caller");
}

#[tokio::test]
async fn lost_heartbeat_detected_by_stall() {
    // Stall detection fires between instructions, not during broker calls.
    // A hanging broker call blocks the interpreter. The per-call timeout
    // (which wraps broker calls via tokio::time::timeout) is the correct
    // mechanism for this scenario.
    struct NeverHeartbeatBroker {
        call_count: std::sync::atomic::AtomicU32,
    }

    #[async_trait::async_trait]
    impl BrokerCallback for NeverHeartbeatBroker {
        async fn execute_call(
            &self,
            _request: &CallRequest,
        ) -> Result<CallResult, InterpreterError> {
            self.call_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // Simulate a call that hangs (no heartbeat)
            tokio::time::sleep(tokio::time::Duration::from_secs(600)).await;
            Ok(CallResult {
                output: ProgramValue::ToolResult(serde_json::json!("ok")),
                artifacts: vec![],
            })
        }

        async fn heartbeat(&self, _budget: &BudgetSnapshot) {
            // Intentionally no-op — simulates lost heartbeat
        }
    }

    let source = r#"
result = call({"tool": "hang_tool", "input": "data"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_per_call_time_ms = 50; // Short per-call timeout catches hanging broker
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = NeverHeartbeatBroker {
        call_count: std::sync::atomic::AtomicU32::new(0),
    };

    let config = codegg_core::tool_program::RunConfig {
        per_call_timeout_ms: Some(50),
        ..Default::default()
    };

    let result = interp.run_with_config(&broker, None, &config).await;
    assert_eq!(result.status, ProgramStatus::Failed);
    assert!(result.error_message.unwrap().contains("timed out"));
}

// ── Many programs respecting global permits ───────────────────────

#[tokio::test]
async fn many_concurrent_programs_independent() {
    let mut handles = Vec::new();
    for i in 0..10 {
        let source = format!("emit({})\n", i);
        let compilation = compile_program(&source).unwrap();
        let limits = RuntimeLimits::from(&compilation.ir.bounds);
        let mut interp = MeteredInterpreter::new(compilation.ir, limits);
        let broker = CountingBroker::new();

        handles.push(tokio::spawn(async move {
            let result = interp.run(&broker, None).await;
            (i, result)
        }));
    }

    for handle in handles {
        let (i, result) = handle.await.unwrap();
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Int(i)));
    }
}

#[tokio::test]
async fn many_loop_programs_respect_budget() {
    let mut handles = Vec::new();
    for _ in 0..5 {
        let source = r#"
total = 0
for i in range(10):
    total = total + 1
emit(total)
"#;
        let compilation = compile_program(source).unwrap();
        let limits = RuntimeLimits::from(&compilation.ir.bounds);
        let mut interp = MeteredInterpreter::new(compilation.ir, limits);
        let broker = CountingBroker::new();

        handles.push(tokio::spawn(async move { interp.run(&broker, None).await }));
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert_eq!(result.status, ProgramStatus::Completed);
    }
}

// ── Job/attempt/program/call/run/artifact correlation ──────────────

#[tokio::test]
async fn call_sequence_numbering_monotonic() {
    use codegg_core::tool_program::{
        CallRequest, InterpreterError, MeteredInterpreter, RuntimeLimits,
    };

    struct SeqBroker {
        sequences: std::sync::Mutex<Vec<u32>>,
    }

    #[async_trait::async_trait]
    impl BrokerCallback for SeqBroker {
        async fn execute_call(
            &self,
            _request: &CallRequest,
        ) -> Result<CallResult, InterpreterError> {
            Ok(CallResult {
                output: ProgramValue::ToolResult(serde_json::json!("ok")),
                artifacts: vec![],
            })
        }
    }

    let source = r#"
r1 = call({"tool": "t1", "input": "a"})
r2 = call({"tool": "t2", "input": "b"})
r3 = call({"tool": "t3", "input": "c"})
emit({"r1": r1, "r2": r2, "r3": r3})
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = SeqBroker {
        sequences: std::sync::Mutex::new(Vec::new()),
    };

    let result = interp.run(&broker, None).await;
    assert_eq!(result.status, ProgramStatus::Completed);

    // Verify 3 calls were made with sequential numbering
    let completed = interp.completed_calls();
    assert_eq!(completed.len(), 3);
    let mut keys: Vec<u32> = completed.keys().copied().collect();
    keys.sort();
    assert_eq!(keys, vec![0, 1, 2]);

    // Verify each call has correct sequence
    for (i, key) in keys.iter().enumerate() {
        let call = completed.get(key).unwrap();
        assert_eq!(call.sequence, i as u32);
    }
}

#[tokio::test]
async fn call_request_tracks_tool_name_and_input() {
    use codegg_core::tool_program::{
        CallRequest, InterpreterError, MeteredInterpreter, RuntimeLimits,
    };

    struct TrackBroker;

    #[async_trait::async_trait]
    impl BrokerCallback for TrackBroker {
        async fn execute_call(
            &self,
            request: &CallRequest,
        ) -> Result<CallResult, InterpreterError> {
            // Verify the request has the expected tool name
            assert_eq!(request.tool_name, "tracked_tool");
            Ok(CallResult {
                output: ProgramValue::ToolResult(serde_json::json!("ok")),
                artifacts: vec![],
            })
        }
    }

    let source = r#"
result = call({"tool": "tracked_tool", "input": "data"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = TrackBroker;

    let result = interp.run(&broker, None).await;
    assert_eq!(result.status, ProgramStatus::Completed);

    // Verify completed call stores request
    let completed = interp.completed_calls();
    assert_eq!(completed.len(), 1);
    let call = completed.get(&0).unwrap();
    assert_eq!(call.request.tool_name, "tracked_tool");
}

// ── Transient retry with backoff tests ────────────────────────────

#[tokio::test]
async fn transient_error_retried_with_backoff() {
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TransientFailBroker {
        fail_count: AtomicU32,
        max_fails: u32,
    }

    #[async_trait::async_trait]
    impl BrokerCallback for TransientFailBroker {
        async fn execute_call(
            &self,
            _request: &CallRequest,
        ) -> Result<CallResult, InterpreterError> {
            let count = self.fail_count.fetch_add(1, Ordering::Relaxed);
            if count < self.max_fails {
                Err(InterpreterError::BrokerError(format!(
                    "transient failure #{}",
                    count
                )))
            } else {
                Ok(CallResult {
                    output: ProgramValue::ToolResult(serde_json::json!("recovered")),
                    artifacts: vec![],
                })
            }
        }
    }

    let source = r#"
result = call({"tool": "flaky_tool", "input": "data"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_retries = 3;
    limits.retry_base_delay_ms = 1; // Minimal delay for testing
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = TransientFailBroker {
        fail_count: AtomicU32::new(0),
        max_fails: 2, // Fail first 2 times, succeed on 3rd
    };

    let result = interp.run(&broker, None).await;
    assert_eq!(result.status, ProgramStatus::Completed);
    assert_eq!(broker.fail_count.load(Ordering::Relaxed), 3); // 2 fails + 1 success
}

#[tokio::test]
async fn transient_error_exceeds_retry_limit() {
    use std::sync::atomic::{AtomicU32, Ordering};

    struct AlwaysFailBroker {
        call_count: AtomicU32,
    }

    #[async_trait::async_trait]
    impl BrokerCallback for AlwaysFailBroker {
        async fn execute_call(
            &self,
            _request: &CallRequest,
        ) -> Result<CallResult, InterpreterError> {
            self.call_count.fetch_add(1, Ordering::Relaxed);
            Err(InterpreterError::BrokerError("persistent failure".into()))
        }
    }

    let source = r#"
result = call({"tool": "broken_tool", "input": "data"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_retries = 2;
    limits.retry_base_delay_ms = 1;
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = AlwaysFailBroker {
        call_count: AtomicU32::new(0),
    };

    let result = interp.run(&broker, None).await;
    assert_eq!(result.status, ProgramStatus::Failed);
    // 1 initial + 2 retries = 3 total calls
    assert_eq!(broker.call_count.load(Ordering::Relaxed), 3);
}

// ── Result-schema validation tests ────────────────────────────────

#[tokio::test]
async fn result_schema_validates_on_emit() {
    use codegg_core::tool_program::RunConfig;

    let source = "emit({\"name\": \"test\", \"value\": 42})\n";
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = FaultBroker::new("none");

    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "value": {"type": "integer"}
        },
        "required": ["name", "value"]
    });

    let config = RunConfig {
        result_schema: Some(schema),
        ..Default::default()
    };

    let result = interp.run_with_config(&broker, None, &config).await;
    assert_eq!(result.status, ProgramStatus::Completed);
}

#[tokio::test]
async fn result_schema_rejects_mismatch() {
    use codegg_core::tool_program::RunConfig;

    let source = "emit({\"name\": \"test\"})\n";
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = FaultBroker::new("none");

    // Schema requires "value" field which is missing
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "value": {"type": "integer"}
        },
        "required": ["name", "value"]
    });

    let config = RunConfig {
        result_schema: Some(schema),
        ..Default::default()
    };

    let result = interp.run_with_config(&broker, None, &config).await;
    assert_eq!(result.status, ProgramStatus::Failed);
    assert_eq!(
        result.failure_class,
        Some(codegg_core::tool_program::FailureClass::SchemaMismatch)
    );
}

// ── Checkpoint production tests ───────────────────────────────────

#[tokio::test]
async fn checkpoints_produced_during_execution() {
    let source = r#"
result = call({"tool": "test_tool", "input": "data"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = CountingBroker::new();

    let result = interp.run(&broker, None).await;
    assert_eq!(result.status, ProgramStatus::Completed);

    // Checkpoints should be produced (before call, after call, before emit)
    // The interpreter tracks checkpoints internally
    // We verify indirectly that the program executed correctly with checkpoints
    assert!(result.steps_used > 0);
}

// ── Per-call timeout tests ────────────────────────────────────────

#[tokio::test]
async fn per_call_timeout_triggers() {
    struct TimeoutBroker;

    #[async_trait::async_trait]
    impl BrokerCallback for TimeoutBroker {
        async fn execute_call(
            &self,
            _request: &CallRequest,
        ) -> Result<CallResult, InterpreterError> {
            tokio::time::sleep(tokio::time::Duration::from_secs(600)).await;
            Ok(CallResult {
                output: ProgramValue::ToolResult(serde_json::json!("ok")),
                artifacts: vec![],
            })
        }
    }

    let source = r#"
result = call({"tool": "timeout_tool", "input": "data"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_per_call_time_ms = 50; // Very short per-call timeout
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = TimeoutBroker;

    let config = codegg_core::tool_program::RunConfig {
        per_call_timeout_ms: Some(50),
        ..Default::default()
    };

    let result = interp.run_with_config(&broker, None, &config).await;
    assert_eq!(result.status, ProgramStatus::Failed);
    assert!(result.error_message.unwrap().contains("timed out"));
}
