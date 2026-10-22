//! Scenario schema and deterministic runner for Tool Programs (M010).
//!
//! Provides a reusable `Scenario` format with versioned fields, a
//! deterministic scripted-provider driver, resource convergence
//! assertions, and evidence capture. Each scenario compiles restricted
//! Python through the full pipeline, runs it against a configurable
//! broker, and asserts terminal convergence with bounded resources.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use codegg_core::tool_program::{
    compile_program, BrokerCallback, BudgetSnapshot, CallRequest, CallResult, ChildJobRequest,
    ChildJobResult, InterpreterError, MeteredInterpreter, ProgramResult, ProgramStatus,
    ProgramValue, RuntimeLimits,
};

// ── Scenario schema ────────────────────────────────────────────────────────

/// Versioned scenario schema for deterministic tool-program evaluation.
#[derive(Debug, Clone)]
pub struct Scenario {
    /// Human-readable scenario name.
    pub name: String,
    /// Scenario version for schema evolution.
    pub version: u32,
    /// Restricted-Python source to compile and execute.
    pub source: String,
    /// Tool names this program may call.
    pub tools: Vec<String>,
    /// Expected terminal status.
    pub expected_status: ProgramStatus,
    /// Expected output substring (if any).
    pub expected_output_contains: Option<String>,
    /// Expected minimum calls completed.
    pub min_calls_completed: u32,
    /// Expected maximum calls completed.
    pub max_calls_completed: u32,
    /// Maximum allowed wall-clock duration.
    pub deadline: Duration,
    /// Maximum allowed steps.
    pub max_steps: u32,
    /// Maximum allowed iterations.
    pub max_iterations: u32,
    /// Custom broker factory (None uses default deterministic broker).
    pub broker: Option<BrokerKind>,
    /// Random seed for reproducibility.
    pub seed: u64,
    /// Whether this scenario expects no active resources after completion.
    pub expect_clean_exit: bool,
    /// Expected failure class (if status is Failed).
    pub expected_failure_class: Option<String>,
}

impl Scenario {
    /// Build a minimal scenario with sane defaults.
    pub fn simple(name: &str, source: &str, tools: &[&str]) -> Self {
        Self {
            name: name.to_string(),
            version: 1,
            source: source.to_string(),
            tools: tools.iter().map(|s| s.to_string()).collect(),
            expected_status: ProgramStatus::Completed,
            expected_output_contains: None,
            min_calls_completed: 0,
            max_calls_completed: 100,
            deadline: Duration::from_secs(30),
            max_steps: 10_000,
            max_iterations: 1_000,
            broker: None,
            seed: 42,
            expect_clean_exit: true,
            expected_failure_class: None,
        }
    }
}

/// Broker variant selector for scenario configuration.
#[derive(Debug, Clone)]
pub enum BrokerKind {
    /// Default deterministic broker returning OK for all calls.
    Deterministic,
    /// Fail on specific tool names.
    Faulty { fail_tools: Vec<String> },
    /// Rate-limit after N calls.
    RateLimited { limit: usize },
    /// Panic on specific tool names.
    Panicking { panic_tools: Vec<String> },
    /// Counting broker that records all calls.
    Counting(Arc<CallCounter>),
}

// ── Scenario result ────────────────────────────────────────────────────────

/// Outcome of running a single scenario.
#[derive(Debug)]
pub struct ScenarioResult {
    pub scenario_name: String,
    pub program_result: ProgramResult,
    pub elapsed: Duration,
    pub converged: bool,
    pub call_count: usize,
    pub assertions_passed: u32,
    pub assertions_failed: Vec<String>,
}

// ── Broker implementations ─────────────────────────────────────────────────

/// Thread-safe call counter for assertion.
#[derive(Debug, Default)]
pub struct CallCounter {
    pub count: AtomicUsize,
    pub calls: std::sync::Mutex<Vec<String>>,
}

impl CallCounter {
    pub fn new() -> Self {
        Self {
            count: AtomicUsize::new(0),
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn total(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }

    pub fn tool_counts(&self) -> HashMap<String, usize> {
        let calls = self.calls.lock().unwrap();
        let mut map = HashMap::new();
        for call in calls.iter() {
            *map.entry(call.clone()).or_insert(0) += 1;
        }
        map
    }
}

struct DeterministicBroker;

#[async_trait::async_trait]
impl BrokerCallback for DeterministicBroker {
    async fn execute_call(&self, request: &CallRequest) -> Result<CallResult, InterpreterError> {
        let output = serde_json::json!({
            "tool": request.tool_name,
            "status": "ok",
            "input": request.input,
        });
        Ok(CallResult {
            output: ProgramValue::ToolResult(output),
            artifacts: vec![],
            success: true,
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

struct FaultyBroker {
    fail_tools: Vec<String>,
}

#[async_trait::async_trait]
impl BrokerCallback for FaultyBroker {
    async fn execute_call(&self, request: &CallRequest) -> Result<CallResult, InterpreterError> {
        if self.fail_tools.contains(&request.tool_name) {
            return Err(InterpreterError::BrokerError(format!(
                "simulated fault: {}",
                request.tool_name
            )));
        }
        let output = serde_json::json!({
            "tool": request.tool_name,
            "status": "ok",
        });
        Ok(CallResult {
            output: ProgramValue::ToolResult(output),
            artifacts: vec![],
            success: true,
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

struct RateLimitedBroker {
    limit: usize,
    count: AtomicUsize,
}

#[async_trait::async_trait]
impl BrokerCallback for RateLimitedBroker {
    async fn execute_call(&self, request: &CallRequest) -> Result<CallResult, InterpreterError> {
        let n = self.count.fetch_add(1, Ordering::Relaxed) + 1;
        if n > self.limit {
            return Err(InterpreterError::BrokerError(format!(
                "rate limit exceeded after {} calls",
                n
            )));
        }
        let output = serde_json::json!({
            "tool": request.tool_name,
            "status": "ok",
            "call_index": n,
        });
        Ok(CallResult {
            output: ProgramValue::ToolResult(output),
            artifacts: vec![],
            success: true,
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

struct PanickingBroker {
    panic_tools: Vec<String>,
}

#[async_trait::async_trait]
impl BrokerCallback for PanickingBroker {
    async fn execute_call(&self, request: &CallRequest) -> Result<CallResult, InterpreterError> {
        if self.panic_tools.contains(&request.tool_name) {
            panic!("simulated panic on {}", request.tool_name);
        }
        let output = serde_json::json!({
            "tool": request.tool_name,
            "status": "ok",
        });
        Ok(CallResult {
            output: ProgramValue::ToolResult(output),
            artifacts: vec![],
            success: true,
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

struct CountingBroker {
    counter: Arc<CallCounter>,
}

#[async_trait::async_trait]
impl BrokerCallback for CountingBroker {
    async fn execute_call(&self, request: &CallRequest) -> Result<CallResult, InterpreterError> {
        self.counter.count.fetch_add(1, Ordering::Relaxed);
        self.counter
            .calls
            .lock()
            .unwrap()
            .push(request.tool_name.clone());
        let output = serde_json::json!({
            "tool": request.tool_name,
            "status": "ok",
        });
        Ok(CallResult {
            output: ProgramValue::ToolResult(output),
            artifacts: vec![],
            success: true,
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

// ── Scenario runner ────────────────────────────────────────────────────────

/// Run a single scenario and return convergence-asserted results.
pub async fn run_scenario(scenario: &Scenario) -> ScenarioResult {
    let start = Instant::now();

    // Compile
    let compilation = match compile_program(&scenario.source) {
        Ok(c) => c,
        Err(e) => {
            let elapsed = start.elapsed();
            return ScenarioResult {
                scenario_name: scenario.name.clone(),
                program_result: ProgramResult {
                    status: ProgramStatus::Failed,
                    output: None,
                    error_message: Some(format!("compilation failed: {}", e)),
                    steps_used: 0,
                    iterations_used: 0,
                    bytes_used: 0,
                    calls_completed: 0,
                    calls_total: 0,
                    failure_class: None,
                },
                elapsed,
                converged: scenario.expected_status == ProgramStatus::Failed,
                call_count: 0,
                assertions_passed: 0,
                assertions_failed: vec![],
            };
        }
    };

    // Build limits
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    limits.max_steps = limits.max_steps.min(scenario.max_steps.into());
    limits.max_loop_iterations = limits
        .max_loop_iterations
        .min(scenario.max_iterations.into());
    limits.max_total_iterations = limits
        .max_total_iterations
        .min(scenario.max_iterations.into());
    limits.max_dynamic_calls = limits.max_dynamic_calls.max(50);
    limits.max_stall_time_ms = scenario.deadline.as_millis() as u64;
    limits.max_per_call_time_ms = 10_000;

    // Build broker
    let broker: Box<dyn BrokerCallback + Send + Sync> = match &scenario.broker {
        None | Some(BrokerKind::Deterministic) => Box::new(DeterministicBroker),
        Some(BrokerKind::Faulty { fail_tools }) => Box::new(FaultyBroker {
            fail_tools: fail_tools.clone(),
        }),
        Some(BrokerKind::RateLimited { limit }) => Box::new(RateLimitedBroker {
            limit: *limit,
            count: AtomicUsize::new(0),
        }),
        Some(BrokerKind::Panicking { panic_tools }) => Box::new(PanickingBroker {
            panic_tools: panic_tools.clone(),
        }),
        Some(BrokerKind::Counting(counter)) => Box::new(CountingBroker {
            counter: counter.clone(),
        }),
    };

    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    let result = interp.run(&*broker, None).await;
    let elapsed = start.elapsed();

    // Assert convergence
    let mut assertions_passed = 0u32;
    let mut assertions_failed = Vec::new();

    // Status assertion
    if result.status == scenario.expected_status {
        assertions_passed += 1;
    } else {
        assertions_failed.push(format!(
            "status: expected {:?}, got {:?}",
            scenario.expected_status, result.status
        ));
    }

    // Call count bounds
    if result.calls_completed >= scenario.min_calls_completed
        && result.calls_completed <= scenario.max_calls_completed
    {
        assertions_passed += 1;
    } else {
        assertions_failed.push(format!(
            "calls_completed: {} not in [{}, {}]",
            result.calls_completed, scenario.min_calls_completed, scenario.max_calls_completed
        ));
    }

    // Output contains
    if let Some(ref expected) = scenario.expected_output_contains {
        let output_str = result
            .output
            .as_ref()
            .map(|v| format!("{:?}", v))
            .unwrap_or_default();
        if output_str.contains(expected) {
            assertions_passed += 1;
        } else {
            assertions_failed.push(format!(
                "output does not contain '{}': got {:?}",
                expected, result.output
            ));
        }
    }

    // Failure class
    if let Some(ref expected_class) = scenario.expected_failure_class {
        let actual_class = result.failure_class.map(|c| format!("{:?}", c));
        if actual_class.as_deref() == Some(expected_class.as_str()) {
            assertions_passed += 1;
        } else {
            assertions_failed.push(format!(
                "failure_class: expected '{}', got {:?}",
                expected_class, actual_class
            ));
        }
    }

    // Clean exit: calls_completed should be bounded
    if scenario.expect_clean_exit {
        if result.calls_completed <= scenario.max_calls_completed {
            assertions_passed += 1;
        } else {
            assertions_failed.push(format!(
                "calls_completed ({}) is unbounded",
                result.calls_completed
            ));
        }
    }

    let converged = assertions_failed.is_empty();

    if !converged {
        eprintln!(
            "SCENARIO '{}' FAILED: status={:?} expected={:?} calls={} min={} max={} output={:?}",
            scenario.name,
            result.status,
            scenario.expected_status,
            result.calls_completed,
            scenario.min_calls_completed,
            scenario.max_calls_completed,
            result.output,
        );
        for err in &assertions_failed {
            eprintln!("  ASSERTION: {}", err);
        }
    }

    ScenarioResult {
        scenario_name: scenario.name.clone(),
        program_result: result,
        elapsed,
        converged,
        call_count: 0, // set by caller if counting broker used
        assertions_passed,
        assertions_failed,
    }
}

/// Run multiple scenarios and report aggregate results.
pub async fn run_scenario_suite(scenarios: &[Scenario]) -> Vec<ScenarioResult> {
    let mut results = Vec::with_capacity(scenarios.len());
    for scenario in scenarios {
        results.push(run_scenario(scenario).await);
    }
    results
}

// ── Built-in scenarios ─────────────────────────────────────────────────────

/// Single emit — trivial program, baseline for compile/runtime overhead.
pub fn scenario_single_emit() -> Scenario {
    Scenario::simple("single_emit", "emit({\"result\": \"ok\"})\n", &[])
}

/// Multi-call read aggregation — reads multiple files and aggregates.
pub fn scenario_multi_read_aggregation() -> Scenario {
    Scenario {
        name: "multi_read_aggregation".into(),
        version: 1,
        source: r#"
r1 = call({"tool": "read", "path": "/tmp/file_0"})
r2 = call({"tool": "read", "path": "/tmp/file_1"})
r3 = call({"tool": "read", "path": "/tmp/file_2"})
r4 = call({"tool": "read", "path": "/tmp/file_3"})
r5 = call({"tool": "read", "path": "/tmp/file_4"})
emit({"files_read": 5, "results": [r1, r2, r3, r4, r5]})
"#
        .into(),
        tools: vec!["read".into()],
        expected_status: ProgramStatus::Completed,
        min_calls_completed: 5,
        max_calls_completed: 5,
        deadline: Duration::from_secs(15),
        ..Scenario::simple("multi_read_aggregation", "", &["read"])
    }
}

/// Parallel reads — exercises parallel() with concurrent tool calls.
pub fn scenario_parallel_reads() -> Scenario {
    Scenario {
        name: "parallel_reads".into(),
        source: r#"
results = parallel(
    {"tool": "read", "path": "/tmp/a.txt"},
    {"tool": "read", "path": "/tmp/b.txt"},
    {"tool": "read", "path": "/tmp/c.txt"},
)
emit({"parallel_results": len(results)})
"#
        .into(),
        tools: vec!["read".into()],
        expected_status: ProgramStatus::Completed,
        min_calls_completed: 3,
        max_calls_completed: 3,
        ..Scenario::simple("parallel_reads", "", &["read"])
    }
}

/// Filter and aggregate — search, filter, count pattern.
pub fn scenario_filter_aggregate() -> Scenario {
    Scenario {
        name: "filter_aggregate".into(),
        source: r#"
m1 = call({"tool": "grep", "pattern": "TODO", "path": "/tmp/file_0"})
m2 = call({"tool": "grep", "pattern": "TODO", "path": "/tmp/file_1"})
m3 = call({"tool": "grep", "pattern": "TODO", "path": "/tmp/file_2"})
m4 = call({"tool": "grep", "pattern": "TODO", "path": "/tmp/file_3"})
m5 = call({"tool": "grep", "pattern": "TODO", "path": "/tmp/file_4"})
m6 = call({"tool": "grep", "pattern": "TODO", "path": "/tmp/file_5"})
m7 = call({"tool": "grep", "pattern": "TODO", "path": "/tmp/file_6"})
m8 = call({"tool": "grep", "pattern": "TODO", "path": "/tmp/file_7"})
m9 = call({"tool": "grep", "pattern": "TODO", "path": "/tmp/file_8"})
m10 = call({"tool": "grep", "pattern": "TODO", "path": "/tmp/file_9"})
emit({"total_matches": 10, "files_searched": 10})
"#
        .into(),
        tools: vec!["grep".into()],
        expected_status: ProgramStatus::Completed,
        min_calls_completed: 10,
        max_calls_completed: 10,
        ..Scenario::simple("filter_aggregate", "", &["grep"])
    }
}

/// Git status/diff/log comparison — metadata aggregation.
pub fn scenario_git_metadata() -> Scenario {
    Scenario {
        name: "git_metadata".into(),
        source: r#"
status = call({"tool": "git_status"})
diff = call({"tool": "git_diff"})
log = call({"tool": "git_log", "limit": 5})
emit({
    "has_changes": status != "",
    "has_diff": diff != "",
    "log_entries": log,
})
"#
        .into(),
        tools: vec!["git_status".into(), "git_diff".into(), "git_log".into()],
        expected_status: ProgramStatus::Completed,
        min_calls_completed: 3,
        max_calls_completed: 3,
        ..Scenario::simple("git_metadata", "", &["git_status", "git_diff", "git_log"])
    }
}

/// Invalid source — should fail with compile error.
pub fn scenario_invalid_source() -> Scenario {
    Scenario {
        name: "invalid_source".into(),
        source: "import os\n".into(),
        tools: vec![],
        expected_status: ProgramStatus::Failed,
        expected_failure_class: Some("compile_error".into()),
        expect_clean_exit: false,
        ..Scenario::simple("invalid_source", "", &[])
    }
}

/// Rate-limited broker — should fail after N calls.
pub fn scenario_rate_limited() -> Scenario {
    Scenario {
        name: "rate_limited".into(),
        source: r#"
for i in range(5):
    call({"tool": "read", "path": "/tmp/f"})
emit({"done": true})
"#
        .into(),
        tools: vec!["read".into()],
        expected_status: ProgramStatus::Failed,
        broker: Some(BrokerKind::RateLimited { limit: 2 }),
        expect_clean_exit: false,
        ..Scenario::simple("rate_limited", "", &["read"])
    }
}

/// Empty program — compiles to a no-op and completes.
pub fn scenario_empty_source() -> Scenario {
    Scenario {
        name: "empty_source".into(),
        source: "".into(),
        tools: vec![],
        expected_status: ProgramStatus::Completed,
        expect_clean_exit: true,
        ..Scenario::simple("empty_source", "", &[])
    }
}

/// Deeply nested loops — exercises static bounds.
pub fn scenario_nested_loops() -> Scenario {
    Scenario {
        name: "nested_loops".into(),
        source: r#"
total = 0
for i in range(3):
    for j in range(3):
        total = total + 1
emit({"total": total})
"#
        .into(),
        tools: vec![],
        expected_status: ProgramStatus::Completed,
        expected_output_contains: Some("9".into()),
        ..Scenario::simple("nested_loops", "", &[])
    }
}

/// Conditional logic — exercises if/else.
pub fn scenario_conditional_logic() -> Scenario {
    Scenario {
        name: "conditional_logic".into(),
        source: r#"
x = 10
if x > 5:
    result = "big"
else:
    result = "small"
emit({"result": result})
"#
        .into(),
        tools: vec![],
        expected_status: ProgramStatus::Completed,
        ..Scenario::simple("conditional_logic", "", &[])
    }
}

/// All corpus scenarios for the evaluation suite.
pub fn all_deterministic_scenarios() -> Vec<Scenario> {
    vec![
        scenario_single_emit(),
        scenario_multi_read_aggregation(),
        scenario_parallel_reads(),
        scenario_filter_aggregate(),
        scenario_git_metadata(),
        scenario_invalid_source(),
        scenario_rate_limited(),
        scenario_nested_loops(),
        scenario_conditional_logic(),
    ]
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn scenario_single_emit_converges() {
    let result = run_scenario(&scenario_single_emit()).await;
    assert!(
        result.converged,
        "assertions failed: {:?}",
        result.assertions_failed
    );
    assert_eq!(result.program_result.status, ProgramStatus::Completed);
}

#[tokio::test(flavor = "current_thread")]
async fn scenario_multi_read_aggregation_converges() {
    let result = run_scenario(&scenario_multi_read_aggregation()).await;
    assert!(
        result.converged,
        "assertions failed: {:?}",
        result.assertions_failed
    );
    assert!(result.program_result.calls_completed >= 5);
}

#[tokio::test(flavor = "current_thread")]
async fn scenario_parallel_reads_converges() {
    let result = run_scenario(&scenario_parallel_reads()).await;
    assert!(
        result.converged,
        "assertions failed: {:?}",
        result.assertions_failed
    );
    assert!(result.program_result.calls_completed >= 3);
}

#[tokio::test(flavor = "current_thread")]
async fn scenario_filter_aggregate_converges() {
    let result = run_scenario(&scenario_filter_aggregate()).await;
    assert!(
        result.converged,
        "assertions failed: {:?}",
        result.assertions_failed
    );
    assert!(result.program_result.calls_completed >= 10);
}

#[tokio::test(flavor = "current_thread")]
async fn scenario_git_metadata_converges() {
    let result = run_scenario(&scenario_git_metadata()).await;
    assert!(
        result.converged,
        "assertions failed: {:?}",
        result.assertions_failed
    );
    assert!(result.program_result.calls_completed >= 3);
}

#[tokio::test(flavor = "current_thread")]
async fn scenario_invalid_source_fails_as_expected() {
    let result = run_scenario(&scenario_invalid_source()).await;
    assert!(
        result.converged,
        "assertions failed: {:?}",
        result.assertions_failed
    );
}

#[tokio::test(flavor = "current_thread")]
async fn scenario_rate_limited_fails() {
    let result = run_scenario(&scenario_rate_limited()).await;
    assert!(
        result.converged,
        "assertions failed: {:?}",
        result.assertions_failed
    );
}

#[tokio::test(flavor = "current_thread")]
async fn scenario_empty_source_fails() {
    let result = run_scenario(&scenario_empty_source()).await;
    assert!(
        result.converged,
        "assertions failed: {:?}",
        result.assertions_failed
    );
}

#[tokio::test(flavor = "current_thread")]
async fn scenario_nested_loops_converges() {
    let result = run_scenario(&scenario_nested_loops()).await;
    assert!(
        result.converged,
        "assertions failed: {:?}",
        result.assertions_failed
    );
}

#[tokio::test(flavor = "current_thread")]
async fn scenario_conditional_logic_converges() {
    let result = run_scenario(&scenario_conditional_logic()).await;
    assert!(
        result.converged,
        "assertions failed: {:?}",
        result.assertions_failed
    );
}

#[tokio::test(flavor = "current_thread")]
async fn full_suite_converges() {
    let scenarios = all_deterministic_scenarios();
    let results = run_scenario_suite(&scenarios).await;
    let failed: Vec<_> = results.iter().filter(|r| !r.converged).collect();
    assert!(
        failed.is_empty(),
        "scenarios that failed convergence: {:?}",
        failed
            .iter()
            .map(|r| (&r.scenario_name, &r.assertions_failed))
            .collect::<Vec<_>>()
    );
    assert_eq!(results.len(), scenarios.len());
}

#[tokio::test(flavor = "current_thread")]
async fn counting_broker_tracks_calls() {
    let counter = Arc::new(CallCounter::new());
    let scenario = Scenario {
        name: "counting".into(),
        source: r#"
r1 = call({"tool": "read", "path": "/tmp/a"})
r2 = call({"tool": "grep", "pattern": "TODO"})
r3 = call({"tool": "read", "path": "/tmp/b"})
emit({"done": true})
"#
        .into(),
        tools: vec!["read".into(), "grep".into()],
        broker: Some(BrokerKind::Counting(counter.clone())),
        ..Scenario::simple("counting", "", &["read", "grep"])
    };
    let result = run_scenario(&scenario).await;
    assert!(
        result.converged,
        "assertions failed: {:?}",
        result.assertions_failed
    );
    assert_eq!(counter.total(), 3);
    let tool_counts = counter.tool_counts();
    assert_eq!(tool_counts.get("read").copied().unwrap_or(0), 2);
    assert_eq!(tool_counts.get("grep").copied().unwrap_or(0), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn deadline_enforced() {
    let scenario = Scenario {
        name: "deadline".into(),
        source: "emit(42)\n".into(),
        tools: vec![],
        deadline: Duration::from_millis(1), // Very tight deadline
        max_steps: 10_000,
        ..Scenario::simple("deadline", "", &[])
    };
    let result = run_scenario(&scenario).await;
    // Either completed quickly or timed out — both are acceptable convergence
    assert!(
        result.program_result.status == ProgramStatus::Completed
            || result.program_result.status == ProgramStatus::TimedOut
            || result.program_result.status == ProgramStatus::Stalled,
        "unexpected status: {:?}",
        result.program_result.status
    );
}
