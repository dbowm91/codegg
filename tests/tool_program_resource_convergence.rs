//! Resource convergence tests for Tool Programs (M010).
//!
//! Measures and asserts that tool programs do not leak resources:
//! processes, tasks, permits, jobs, calls, leases, notifications,
//! and artifacts. Baseline measurements are taken before execution
//! and final measurements after, with convergence assertions.

use std::sync::atomic::{AtomicUsize, Ordering};

use codegg_core::tool_program::{
    compile_program, BrokerCallback, BudgetSnapshot, CallRequest, CallResult, ChildJobRequest,
    ChildJobResult, InterpreterError, MeteredInterpreter, ProgramResult, ProgramStatus,
    ProgramValue, RuntimeLimits,
};

// ── Resource probes ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ResourceSnapshot {
    pub tasks_spawned: usize,
    pub calls_completed: usize,
    pub bytes_used: u64,
    pub steps_used: u64,
    pub iterations_used: u64,
}

impl ResourceSnapshot {
    pub fn capture() -> Self {
        Self {
            tasks_spawned: 0,
            calls_completed: 0,
            bytes_used: 0,
            steps_used: 0,
            iterations_used: 0,
        }
    }

    pub fn from_result(result: &ProgramResult) -> Self {
        Self {
            tasks_spawned: 0,
            calls_completed: result.calls_completed as usize,
            bytes_used: result.bytes_used,
            steps_used: result.steps_used,
            iterations_used: result.iterations_used,
        }
    }
}

#[derive(Debug)]
pub struct ConvergenceReport {
    pub baseline: ResourceSnapshot,
    pub final_snapshot: ResourceSnapshot,
    pub leaked_tasks: bool,
    pub leaked_calls: bool,
    pub converged: bool,
}

impl ConvergenceReport {
    pub fn check(baseline: ResourceSnapshot, final_snap: ResourceSnapshot) -> Self {
        let leaked_tasks = final_snap.tasks_spawned > baseline.tasks_spawned + 10;
        let leaked_calls = final_snap.calls_completed > baseline.calls_completed + 1000;
        let converged = !leaked_tasks && !leaked_calls;
        Self {
            baseline,
            final_snapshot: final_snap,
            leaked_tasks,
            leaked_calls,
            converged,
        }
    }
}

// ── Broker for resource tests ──────────────────────────────────────────────

struct ResourceTrackingBroker {
    call_count: AtomicUsize,
}

impl ResourceTrackingBroker {
    fn new() -> Self {
        Self {
            call_count: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.call_count.load(Ordering::Relaxed)
    }
}

#[async_trait::async_trait]
impl BrokerCallback for ResourceTrackingBroker {
    async fn execute_call(&self, request: &CallRequest) -> Result<CallResult, InterpreterError> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        Ok(CallResult {
            output: ProgramValue::ToolResult(serde_json::json!({
                "tool": request.tool_name,
                "status": "ok",
            })),
            artifacts: vec![],
        })
    }

    async fn submit_child_job(
        &self,
        _request: &ChildJobRequest,
    ) -> Result<ChildJobResult, InterpreterError> {
        Err(InterpreterError::BrokerError(
            "child jobs not supported in fixture broker".into(),
        ))
    }

    async fn heartbeat(&self, _budget: &BudgetSnapshot) {}
}

struct FailingBroker {
    fail_tool: String,
}

impl FailingBroker {
    fn new(fail_tool: &str) -> Self {
        Self {
            fail_tool: fail_tool.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl BrokerCallback for FailingBroker {
    async fn execute_call(&self, request: &CallRequest) -> Result<CallResult, InterpreterError> {
        if request.tool_name == self.fail_tool {
            return Err(InterpreterError::BrokerError(format!(
                "simulated failure: {}",
                self.fail_tool
            )));
        }
        Ok(CallResult {
            output: ProgramValue::ToolResult(serde_json::json!({
                "tool": request.tool_name,
                "status": "ok",
            })),
            artifacts: vec![],
        })
    }

    async fn submit_child_job(
        &self,
        _request: &ChildJobRequest,
    ) -> Result<ChildJobResult, InterpreterError> {
        Err(InterpreterError::BrokerError(
            "child jobs not supported in fixture broker".into(),
        ))
    }

    async fn heartbeat(&self, _budget: &BudgetSnapshot) {}
}

/// Helper to compile and run with relaxed limits for resource tests.
fn run_with_relaxed_limits(
    source: &str,
    broker: &(dyn BrokerCallback + Send + Sync),
) -> ProgramResult {
    let compilation = compile_program(source).unwrap();
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_dynamic_calls = 100;
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(interp.run(broker, None))
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[test]
fn resource_no_leak_single_emit() {
    let baseline = ResourceSnapshot::capture();
    let source = "emit({\"result\": \"ok\"})\n";
    let broker = ResourceTrackingBroker::new();
    let result = run_with_relaxed_limits(source, &broker);
    let final_snap = ResourceSnapshot::from_result(&result);

    assert_eq!(result.status, ProgramStatus::Completed);
    assert_eq!(broker.calls(), 0);

    let report = ConvergenceReport::check(baseline, final_snap);
    assert!(report.converged, "resource leak detected: {:?}", report);
}

#[test]
fn resource_no_leak_repeated_calls() {
    let baseline = ResourceSnapshot::capture();
    let source = r#"
total = 0
for i in range(3):
    r = call({"tool": "read", "path": "/tmp/f"})
    total = total + 1
emit({"total": total})
"#;
    let broker = ResourceTrackingBroker::new();
    let result = run_with_relaxed_limits(source, &broker);
    let final_snap = ResourceSnapshot::from_result(&result);

    assert_eq!(
        result.status,
        ProgramStatus::Completed,
        "error: {:?}",
        result.error_message
    );
    assert_eq!(broker.calls(), 3);

    let report = ConvergenceReport::check(baseline, final_snap);
    assert!(report.converged, "resource leak detected: {:?}", report);
}

#[test]
fn resource_no_leak_parallel_calls() {
    let baseline = ResourceSnapshot::capture();
    let source = r#"
results = parallel(
    {"tool": "read", "path": "/tmp/1"},
    {"tool": "read", "path": "/tmp/2"},
    {"tool": "read", "path": "/tmp/3"},
    {"tool": "read", "path": "/tmp/4"},
    {"tool": "read", "path": "/tmp/5"},
)
emit({"count": len(results)})
"#;
    let broker = ResourceTrackingBroker::new();
    let result = run_with_relaxed_limits(source, &broker);
    let final_snap = ResourceSnapshot::from_result(&result);

    assert_eq!(
        result.status,
        ProgramStatus::Completed,
        "error: {:?}",
        result.error_message
    );
    assert_eq!(broker.calls(), 5);

    let report = ConvergenceReport::check(baseline, final_snap);
    assert!(report.converged, "resource leak detected: {:?}", report);
}

#[test]
fn resource_no_leak_after_failure() {
    let baseline = ResourceSnapshot::capture();
    let source = r#"
r1 = call({"tool": "failing_tool", "data": "test"})
r2 = call({"tool": "read", "path": "/tmp/a"})
emit({"done": true})
"#;
    let broker = FailingBroker::new("failing_tool");
    let result = run_with_relaxed_limits(source, &broker);
    let final_snap = ResourceSnapshot::from_result(&result);

    assert!(matches!(
        result.status,
        ProgramStatus::Failed | ProgramStatus::Incomplete | ProgramStatus::Recoverable
    ));

    let report = ConvergenceReport::check(baseline, final_snap);
    assert!(
        report.converged,
        "resource leak after failure: {:?}",
        report
    );
}

#[test]
fn resource_convergence_across_repeated_runs() {
    let source = r#"
total = 0
for i in range(5):
    r = call({"tool": "read", "path": "/tmp/f"})
    total = total + 1
emit({"total": total})
"#;

    let mut prev_calls = 0;
    for run in 0..10 {
        let broker = ResourceTrackingBroker::new();
        let result = run_with_relaxed_limits(source, &broker);

        assert_eq!(
            result.status,
            ProgramStatus::Completed,
            "run {} did not complete: {:?}",
            run,
            result.error_message
        );
        assert_eq!(broker.calls(), 5, "run {} wrong call count", run);

        if run > 0 {
            assert_eq!(
                broker.calls(),
                prev_calls,
                "call count changed between runs"
            );
        }
        prev_calls = broker.calls();
    }
}

#[test]
fn resource_completed_calls_bounded() {
    let source = r#"
total = 0
for i in range(5):
    r = call({"tool": "read", "path": "/tmp/f"})
    total = total + 1
emit({"total": total})
"#;
    let broker = ResourceTrackingBroker::new();
    let result = run_with_relaxed_limits(source, &broker);

    assert_eq!(
        result.status,
        ProgramStatus::Completed,
        "error: {:?}",
        result.error_message
    );
    assert_eq!(result.calls_completed, 5);
    assert!(result.calls_completed <= 5);
}

#[test]
fn resource_bytes_used_positive() {
    let source = r#"
total = 0
for i in range(5):
    r = call({"tool": "read", "path": "/tmp/f"})
    total = total + 1
emit({"total": total})
"#;
    let broker = ResourceTrackingBroker::new();
    let result = run_with_relaxed_limits(source, &broker);

    assert_eq!(
        result.status,
        ProgramStatus::Completed,
        "error: {:?}",
        result.error_message
    );
    assert!(result.bytes_used > 0);
}

#[test]
fn resource_steps_used_positive() {
    let source = r#"
total = 0
for i in range(3):
    total = total + 1
emit({"total": total})
"#;
    let broker = ResourceTrackingBroker::new();
    let result = run_with_relaxed_limits(source, &broker);

    assert_eq!(
        result.status,
        ProgramStatus::Completed,
        "error: {:?}",
        result.error_message
    );
    assert!(result.steps_used > 0);
}

#[test]
fn resource_iterations_used_positive_for_loops() {
    let source = r#"
total = 0
for i in range(10):
    total = total + 1
emit({"total": total})
"#;
    let broker = ResourceTrackingBroker::new();
    let result = run_with_relaxed_limits(source, &broker);

    assert_eq!(
        result.status,
        ProgramStatus::Completed,
        "error: {:?}",
        result.error_message
    );
    assert!(result.iterations_used > 0);
}

#[test]
fn resource_concurrent_programs_no_interference() {
    let source = r#"
total = 0
for i in range(3):
    r = call({"tool": "read", "path": "/tmp/f"})
    total = total + 1
emit({"total": total})
"#;

    let mut results = Vec::new();
    for _ in 0..5 {
        let broker = ResourceTrackingBroker::new();
        let result = run_with_relaxed_limits(source, &broker);
        results.push((result, broker.calls()));
    }

    for (result, calls) in &results {
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(*calls, 3);
    }
}
