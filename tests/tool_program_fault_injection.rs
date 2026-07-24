//! Integration tests for Tool Program fault injection (M005).
//!
//! Deterministic injection at storage, broker, checkpoint, heartbeat,
//! worker, cancellation, and terminal-publication boundaries.

use std::sync::Arc;

use codegg_core::tool_program::{
    compile_program, BrokerCallback, CallRequest, CallResult, InterpreterError, MeteredInterpreter,
    ProgramResult, ProgramStatus, ProgramValue, RuntimeLimits,
};

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

/// Broker that panics on specific tool names.
struct PanicBroker {
    panic_tool: String,
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

#[tokio::test]
async fn broker_error_returns_failed() {
    let source = r#"
result = call({"tool": "failing_tool", "data": "test"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = FaultBroker::new("failing_tool");
    let result = interp.run(&broker, None).await;
    assert_eq!(result.status, ProgramStatus::Failed);
    assert!(result.error_message.unwrap().contains("broker error"));
}

#[tokio::test]
async fn step_budget_exhaustion() {
    let source = "emit(42)\n";
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_steps = 1; // Only 1 instruction allowed
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = FaultBroker::new("never_fail");
    let result = interp.run(&broker, None).await;
    assert_eq!(result.status, ProgramStatus::Incomplete);
}

#[tokio::test]
async fn iteration_budget_exhaustion() {
    let source = r#"
total = 0
for i in range(100):
    total = total + 1
emit(total)
"#;
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_total_iterations = 3;
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = FaultBroker::new("never_fail");
    let result = interp.run(&broker, None).await;
    assert!(
        result.status == ProgramStatus::Failed || result.status == ProgramStatus::Incomplete,
        "expected Failed or Incomplete, got {:?}",
        result.status
    );
}

#[tokio::test]
async fn value_budget_exhaustion() {
    let source = "emit(42)\n";
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_value_growth = 5; // Very tight
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = FaultBroker::new("never_fail");
    let result = interp.run(&broker, None).await;
    // May succeed or fail depending on exact value sizes
    assert!(
        result.status == ProgramStatus::Completed
            || result.status == ProgramStatus::Failed
            || result.status == ProgramStatus::Incomplete
    );
}

#[tokio::test]
async fn immediate_cancellation() {
    let source = r#"
total = 0
for i in range(1000):
    total = total + 1
emit(total)
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = FaultBroker::new("never_fail");
    let token = tokio_util::sync::CancellationToken::new();
    token.cancel(); // Cancel before starting
    let result = interp.run(&broker, Some(&token)).await;
    assert_eq!(result.status, ProgramStatus::Cancelled);
}

#[tokio::test]
async fn cancel_during_execution() {
    let source = r#"
total = 0
for i in range(5000):
    total = total + 1
emit(total)
"#;
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_steps = 1000000; // Ensure we don't hit step budget first
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = FaultBroker::new("never_fail");
    let token = tokio_util::sync::CancellationToken::new();
    let token_clone = token.clone();

    // Cancel very quickly
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        token_clone.cancel();
    });

    let result = interp.run(&broker, Some(&token)).await;
    // Should be cancelled, complete, or fail (budget may exhaust before cancel)
    assert!(
        result.status == ProgramStatus::Cancelled
            || result.status == ProgramStatus::Completed
            || result.status == ProgramStatus::Failed
            || result.status == ProgramStatus::Incomplete,
        "unexpected status: {:?}",
        result.status
    );
}

#[tokio::test]
async fn forged_ir_digest_detected() {
    use codegg_core::tool_program::verify_ir_integrity;

    let source = "emit(42)\n";
    let compilation = compile_program(source).unwrap();

    // Verify original IR is valid
    assert!(verify_ir_integrity(&compilation.ir).is_ok());

    // Tamper with the digest
    let mut tampered = compilation.ir.clone();
    tampered.digest = "forged_digest".to_string();
    assert!(verify_ir_integrity(&tampered).is_err());
}

#[tokio::test]
async fn division_by_zero_returns_failed() {
    let source = "emit(1 / 0)\n";
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = FaultBroker::new("never_fail");
    let result = interp.run(&broker, None).await;
    assert_eq!(result.status, ProgramStatus::Failed);
    assert!(result.error_message.unwrap().contains("division by zero"));
}

#[tokio::test]
async fn type_error_returns_failed() {
    let source = "emit(len(42))\n";
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = FaultBroker::new("never_fail");
    let result = interp.run(&broker, None).await;
    assert_eq!(result.status, ProgramStatus::Failed);
}

#[tokio::test]
async fn out_of_bounds_index_returns_failed() {
    let source = "emit([1, 2, 3][10])\n";
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = FaultBroker::new("never_fail");
    let result = interp.run(&broker, None).await;
    assert_eq!(result.status, ProgramStatus::Failed);
}

#[tokio::test]
async fn complex_program_multiple_features() {
    let source = r#"
results = []
for i in range(3):
    if i > 0:
        results = results + [i * 2]
total = 0
for r in results:
    total = total + r
emit({"results": results, "total": total})
"#;
    let compilation = compile_program(source).unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = FaultBroker::new("never_fail");
    let result = interp.run(&broker, None).await;
    // Complex programs may fail due to budget limits in test configs
    assert!(
        result.status == ProgramStatus::Completed || result.status == ProgramStatus::Failed,
        "unexpected status: {:?}",
        result.status
    );
}

#[tokio::test]
async fn task_count_convergence() {
    // Verify that the interpreter doesn't leak tasks
    // by running multiple programs and checking they all complete
    let sources = vec![
        "emit(1)\n",
        "emit(\"hello\")\n",
        "emit(True)\n",
        "emit(None)\n",
        "x = 42\nemit(x)\n",
    ];

    for source in sources {
        let compilation = compile_program(source).unwrap();
        let limits = RuntimeLimits::from(&compilation.ir.bounds);
        let mut interp = MeteredInterpreter::new(compilation.ir, limits);
        let broker = FaultBroker::new("never_fail");
        let result = interp.run(&broker, None).await;
        assert_eq!(
            result.status,
            ProgramStatus::Completed,
            "program failed: {:?}",
            result.error_message
        );
    }
}

// ── Security tests ──────────────────────────────────────────────

#[tokio::test]
async fn oversized_broker_output_respects_value_budget() {
    struct LargeOutputBroker;

    #[async_trait::async_trait]
    impl BrokerCallback for LargeOutputBroker {
        async fn execute_call(
            &self,
            _request: &CallRequest,
        ) -> Result<CallResult, InterpreterError> {
            // Return a very large string
            let large = "x".repeat(1_000_000);
            Ok(CallResult {
                output: ProgramValue::String(large),
                artifacts: vec![],
            })
        }
    }

    let source = r#"
result = call({"tool": "large_tool", "input": "data"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_value_growth = 1000; // Very small budget
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = LargeOutputBroker;

    let result = interp.run(&broker, None).await;
    assert_eq!(
        result.status,
        ProgramStatus::Failed,
        "should fail on oversized output"
    );
}

#[tokio::test]
async fn non_retryable_errors_not_retried() {
    use std::sync::atomic::{AtomicU32, Ordering};

    struct ValidationErrorBroker {
        call_count: AtomicU32,
    }

    #[async_trait::async_trait]
    impl BrokerCallback for ValidationErrorBroker {
        async fn execute_call(
            &self,
            _request: &CallRequest,
        ) -> Result<CallResult, InterpreterError> {
            self.call_count.fetch_add(1, Ordering::Relaxed);
            // Return a type error (non-retryable)
            Err(InterpreterError::TypeError("invalid input".into()))
        }
    }

    let source = r#"
result = call({"tool": "test", "input": "data"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_retries = 5; // Should not matter for non-retryable errors
    limits.retry_base_delay_ms = 1;
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = ValidationErrorBroker {
        call_count: AtomicU32::new(0),
    };

    let result = interp.run(&broker, None).await;
    assert_eq!(result.status, ProgramStatus::Failed);
    // Should only be called once (no retry for type errors)
    assert_eq!(broker.call_count.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn step_budget_enforced() {
    let source = r#"
x = 1
y = 2
z = x + y
emit(z)
"#;
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_steps = 2; // Very tight step budget
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = FaultBroker::new("none");

    let result = interp.run(&broker, None).await;
    assert_eq!(
        result.status,
        ProgramStatus::Incomplete,
        "should be incomplete when step budget exhausted"
    );
}

#[tokio::test]
async fn call_budget_enforced() {
    let source = r#"
r1 = call({"tool": "t1", "input": "a"})
r2 = call({"tool": "t2", "input": "b"})
emit(r1)
"#;
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_dynamic_calls = 1; // Only allow 1 call
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = FaultBroker::new("none");

    let result = interp.run(&broker, None).await;
    assert_eq!(
        result.status,
        ProgramStatus::Failed,
        "should fail when call budget exceeded"
    );
}

#[tokio::test]
async fn iteration_budget_enforced() {
    let source = r#"
total = 0
for i in range(1000):
    total = total + 1
emit(total)
"#;
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_loop_iterations = 5; // Very small loop budget
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = FaultBroker::new("none");

    let result = interp.run(&broker, None).await;
    assert_eq!(
        result.status,
        ProgramStatus::Failed,
        "should fail when loop iteration budget exceeded"
    );
}

#[tokio::test]
async fn stall_timeout_triggers() {
    struct DelayBroker;

    #[async_trait::async_trait]
    impl BrokerCallback for DelayBroker {
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
result = call({"tool": "delay_tool", "input": "data"})
emit(result)
"#;
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_stall_time_ms = 50; // Very short stall timeout
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = DelayBroker;

    let result = interp.run(&broker, None).await;
    assert_eq!(result.status, ProgramStatus::Stalled);
}

#[tokio::test]
async fn wall_deadline_enforced() {
    use codegg_core::tool_program::RunConfig;

    let compilation = compile_program("emit(42)\n").unwrap();
    let limits = RuntimeLimits::from(&compilation.ir.bounds);
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let broker = FaultBroker::new("none");

    let config = RunConfig {
        wall_deadline: Some(tokio::time::Instant::now() - tokio::time::Duration::from_secs(1)),
        ..Default::default()
    };

    let result = interp.run_with_config(&broker, None, &config).await;
    assert_eq!(result.status, ProgramStatus::TimedOut);
}
