//! Integration tests for Tool Program child-job composition (M007).
//!
//! Tests that programs can submit and await scheduler-owned build, test,
//! lint, and format child jobs through the Tool Broker, with correct
//! correlation, status distinction, cancellation propagation, and
//! security invariants.

use std::sync::atomic::{AtomicU32, Ordering};

use codegg_core::tool_program::{
    child_job::{ChildJobConfig, ChildJobDetails, ChildJobOp, ChildJobRequest, ChildJobResult},
    compile_program, BrokerCallback, CallRequest, CallResult, InterpreterError, MeteredInterpreter,
    ProgramResult, ProgramStatus, ProgramValue, RuntimeLimits,
};

// ── Test brokers ───────────────────────────────────────────────────

/// Broker that returns configurable child-job results and tracks submissions.
struct ChildJobBroker {
    results: std::sync::Mutex<Vec<ChildJobResult>>,
    submission_count: AtomicU32,
    /// If set, the next child job returns this error instead of a result.
    fail_next: std::sync::Mutex<Option<String>>,
}

impl ChildJobBroker {
    fn new(results: Vec<ChildJobResult>) -> Self {
        Self {
            results: std::sync::Mutex::new(results),
            submission_count: AtomicU32::new(0),
            fail_next: std::sync::Mutex::new(None),
        }
    }

    fn with_fail_next(message: &str) -> Self {
        Self {
            results: std::sync::Mutex::new(vec![]),
            submission_count: AtomicU32::new(0),
            fail_next: std::sync::Mutex::new(Some(message.to_string())),
        }
    }

    fn submission_count(&self) -> u32 {
        self.submission_count.load(Ordering::Relaxed)
    }
}

#[async_trait::async_trait]
impl BrokerCallback for ChildJobBroker {
    async fn execute_call(&self, _request: &CallRequest) -> Result<CallResult, InterpreterError> {
        Ok(CallResult {
            output: ProgramValue::ToolResult(serde_json::json!({"status": "ok"})),
            artifacts: vec![],
            success: true,
        })
    }

    async fn submit_child_job(
        &self,
        _request: &ChildJobRequest,
    ) -> Result<ChildJobResult, InterpreterError> {
        self.submission_count.fetch_add(1, Ordering::Relaxed);

        // Check if we should fail
        {
            let mut fail_next = self.fail_next.lock().unwrap();
            if let Some(msg) = fail_next.take() {
                return Err(InterpreterError::BrokerError(msg));
            }
        }

        let mut results = self.results.lock().unwrap();
        if results.is_empty() {
            Err(InterpreterError::BrokerError("no more mock results".into()))
        } else {
            Ok(results.remove(0))
        }
    }
}

/// Broker that always rejects child jobs — used to test error propagation.
struct NoopChildBroker;

#[async_trait::async_trait]
impl BrokerCallback for NoopChildBroker {
    async fn execute_call(&self, _request: &CallRequest) -> Result<CallResult, InterpreterError> {
        Ok(CallResult {
            output: ProgramValue::ToolResult(serde_json::json!({"status": "ok"})),
            artifacts: vec![],
            success: true,
        })
    }

    async fn submit_child_job(
        &self,
        _request: &ChildJobRequest,
    ) -> Result<ChildJobResult, InterpreterError> {
        Err(InterpreterError::BrokerError(
            "noop broker does not support child jobs".into(),
        ))
    }
}

// ── Helpers ────────────────────────────────────────────────────────

fn run_program(source: &str, broker: &dyn BrokerCallback) -> ProgramResult {
    let compilation = compile_program(source).expect("compilation failed");
    let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
    // Ensure enough call budget for programs with multiple submit_job calls
    if limits.max_dynamic_calls < 20 {
        limits.max_dynamic_calls = 20;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut interp = MeteredInterpreter::new(compilation.ir, limits);
    rt.block_on(interp.run(broker, None))
}

fn child_job_result_from_emitted(value: &ProgramValue) -> ChildJobResult {
    match value {
        ProgramValue::ToolResult(json) => {
            serde_json::from_value(json.clone()).expect("failed to deserialize ChildJobResult")
        }
        other => panic!("expected ToolResult, got {:?}", other),
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[test]
fn submit_passing_test_child_job() {
    let broker = ChildJobBroker::new(vec![ChildJobResult {
        success: true,
        exit_code: Some(0),
        duration_ms: 1500,
        details: ChildJobDetails::Test(codegg_core::tool_program::child_job::TestJobResult {
            status: "passed".into(),
            framework: Some("cargo".into()),
            total: Some(42),
            passed: Some(42),
            failed: Some(0),
            skipped: Some(0),
            failed_tests: vec![],
            failure_evidence: vec![],
            cancelled: false,
            timed_out: false,
        }),
        artifacts: vec!["ctx://logs/test-run-1".into()],
        error: None,
    }]);

    let source = r#"
result = submit_job("test", {"scope": "workspace", "timeout_secs": 120})
emit(result)
"#;

    let prog_result = run_program(source, &broker);
    assert_eq!(prog_result.status, ProgramStatus::Completed);
    assert_eq!(broker.submission_count(), 1);

    let child = child_job_result_from_emitted(prog_result.output.as_ref().unwrap());
    assert!(child.success);
    assert_eq!(child.exit_code, Some(0));
    assert_eq!(child.duration_ms, 1500);
    assert_eq!(child.artifacts, vec!["ctx://logs/test-run-1"]);

    match &child.details {
        ChildJobDetails::Test(t) => {
            assert_eq!(t.status, "passed");
            assert_eq!(t.framework.as_deref(), Some("cargo"));
            assert_eq!(t.total, Some(42));
            assert_eq!(t.passed, Some(42));
            assert_eq!(t.failed, Some(0));
        }
        other => panic!("expected Test details, got {:?}", other),
    }
}

#[test]
fn submit_failing_test_child_job_returns_success_true() {
    // A failing test is NOT an infrastructure error — it returns
    // success=false in the typed result, not a BrokerError.
    let broker = ChildJobBroker::new(vec![ChildJobResult {
        success: false,
        exit_code: Some(1),
        duration_ms: 800,
        details: ChildJobDetails::Test(codegg_core::tool_program::child_job::TestJobResult {
            status: "failed".into(),
            framework: Some("cargo".into()),
            total: Some(10),
            passed: Some(8),
            failed: Some(2),
            skipped: Some(0),
            failed_tests: vec!["test_parse_json".into(), "test_serialize".into()],
            failure_evidence: vec!["assertion failed: left == right".into()],
            cancelled: false,
            timed_out: false,
        }),
        artifacts: vec![],
        error: None,
    }]);

    let source = r#"
result = submit_job("test", {"scope": "package"})
emit(result)
"#;

    let prog_result = run_program(source, &broker);
    // Program completes successfully — the test failure is in the result, not the program status
    assert_eq!(prog_result.status, ProgramStatus::Completed);

    let child = child_job_result_from_emitted(prog_result.output.as_ref().unwrap());
    assert!(!child.success);
    assert_eq!(child.exit_code, Some(1));

    match &child.details {
        ChildJobDetails::Test(t) => {
            assert_eq!(t.status, "failed");
            assert_eq!(t.failed, Some(2));
            assert!(t.failed_tests.contains(&"test_parse_json".to_string()));
            assert!(!t.failure_evidence.is_empty());
        }
        other => panic!("expected Test details, got {:?}", other),
    }
}

#[test]
fn submit_build_child_job() {
    let broker = ChildJobBroker::new(vec![ChildJobResult {
        success: true,
        exit_code: Some(0),
        duration_ms: 30000,
        details: ChildJobDetails::Build(codegg_core::tool_program::child_job::BuildJobResult {
            status: "success".into(),
            command: Some("cargo build --release".into()),
            diagnostics_errors: Some(0),
            diagnostics_warnings: Some(3),
            changed_files: vec![],
        }),
        artifacts: vec!["ctx://logs/build-1".into()],
        error: None,
    }]);

    let source = r#"
result = submit_job("build", {"argv": ["cargo", "build", "--release"], "timeout_secs": 300})
emit(result)
"#;

    let prog_result = run_program(source, &broker);
    assert_eq!(prog_result.status, ProgramStatus::Completed);

    let child = child_job_result_from_emitted(prog_result.output.as_ref().unwrap());
    assert!(child.success);
    match &child.details {
        ChildJobDetails::Build(b) => {
            assert_eq!(b.status, "success");
            assert_eq!(b.diagnostics_warnings, Some(3));
        }
        other => panic!("expected Build details, got {:?}", other),
    }
}

#[test]
fn submit_lint_child_job() {
    let broker = ChildJobBroker::new(vec![ChildJobResult {
        success: true,
        exit_code: Some(0),
        duration_ms: 5000,
        details: ChildJobDetails::Lint(codegg_core::tool_program::child_job::LintJobResult {
            status: "warnings".into(),
            command: Some("cargo clippy".into()),
            diagnostics_errors: Some(0),
            diagnostics_warnings: Some(2),
        }),
        artifacts: vec![],
        error: None,
    }]);

    let source = r#"
result = submit_job("lint", {})
emit(result)
"#;

    let prog_result = run_program(source, &broker);
    assert_eq!(prog_result.status, ProgramStatus::Completed);

    let child = child_job_result_from_emitted(prog_result.output.as_ref().unwrap());
    assert!(child.success);
    match &child.details {
        ChildJobDetails::Lint(l) => {
            assert_eq!(l.status, "warnings");
            assert_eq!(l.diagnostics_warnings, Some(2));
        }
        other => panic!("expected Lint details, got {:?}", other),
    }
}

#[test]
fn submit_format_child_job() {
    let broker = ChildJobBroker::new(vec![ChildJobResult {
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

    let source = r#"
result = submit_job("format", {"argv": ["cargo", "fmt", "--check"]})
emit(result)
"#;

    let prog_result = run_program(source, &broker);
    assert_eq!(prog_result.status, ProgramStatus::Completed);

    let child = child_job_result_from_emitted(prog_result.output.as_ref().unwrap());
    assert!(child.success);
    match &child.details {
        ChildJobDetails::Format(f) => {
            assert_eq!(f.status, "clean");
            assert!(!f.would_change);
        }
        other => panic!("expected Format details, got {:?}", other),
    }
}

#[test]
fn submit_format_needs_formatting() {
    let broker = ChildJobBroker::new(vec![ChildJobResult {
        success: false,
        exit_code: Some(1),
        duration_ms: 1000,
        details: ChildJobDetails::Format(codegg_core::tool_program::child_job::FormatJobResult {
            status: "needs_formatting".into(),
            command: Some("cargo fmt --check".into()),
            would_change: true,
        }),
        artifacts: vec![],
        error: None,
    }]);

    let source = r#"
result = submit_job("format", {})
emit(result)
"#;

    let prog_result = run_program(source, &broker);
    assert_eq!(prog_result.status, ProgramStatus::Completed);

    let child = child_job_result_from_emitted(prog_result.output.as_ref().unwrap());
    assert!(!child.success);
    match &child.details {
        ChildJobDetails::Format(f) => {
            assert_eq!(f.status, "needs_formatting");
            assert!(f.would_change);
        }
        other => panic!("expected Format details, got {:?}", other),
    }
}

#[test]
fn infrastructure_failure_is_broker_error_not_child_result() {
    // When the broker itself fails (e.g. scheduler unavailable),
    // it should return a BrokerError, not a ChildJobResult with success=false.
    let broker = ChildJobBroker::with_fail_next("scheduler unavailable");

    let source = r#"
result = submit_job("test", {})
emit(result)
"#;

    let prog_result = run_program(source, &broker);
    assert_eq!(prog_result.status, ProgramStatus::Failed);
    assert!(prog_result.error_message.is_some());
    assert!(prog_result
        .error_message
        .as_ref()
        .unwrap()
        .contains("scheduler unavailable"));
}

#[test]
fn invalid_operation_is_broker_error() {
    // An unknown operation string should fail at parse time.
    let broker = ChildJobBroker::new(vec![]);

    let source = r#"
result = submit_job("deploy", {"target": "prod"})
emit(result)
"#;

    let prog_result = run_program(source, &broker);
    assert_eq!(prog_result.status, ProgramStatus::Failed);
    assert!(prog_result.error_message.is_some());
    assert!(prog_result
        .error_message
        .as_ref()
        .unwrap()
        .contains("unknown child job operation"));
}

#[test]
fn submit_job_expression_form() {
    // submit_job() as a bare expression (no assignment) should work
    let broker = ChildJobBroker::new(vec![ChildJobResult {
        success: true,
        exit_code: Some(0),
        duration_ms: 100,
        details: ChildJobDetails::Test(
            codegg_core::tool_program::child_job::TestJobResult::default(),
        ),
        artifacts: vec![],
        error: None,
    }]);

    let source = r#"
submit_job("test", {})
emit({"done": true})
"#;

    let prog_result = run_program(source, &broker);
    assert_eq!(prog_result.status, ProgramStatus::Completed);
    assert_eq!(broker.submission_count(), 1);
}

#[test]
fn multiple_child_jobs_in_sequence() {
    let broker = ChildJobBroker::new(vec![
        ChildJobResult {
            success: true,
            exit_code: Some(0),
            duration_ms: 30000,
            details: ChildJobDetails::Build(codegg_core::tool_program::child_job::BuildJobResult {
                status: "success".into(),
                ..Default::default()
            }),
            artifacts: vec![],
            error: None,
        },
        ChildJobResult {
            success: true,
            exit_code: Some(0),
            duration_ms: 5000,
            details: ChildJobDetails::Test(codegg_core::tool_program::child_job::TestJobResult {
                status: "passed".into(),
                total: Some(5),
                passed: Some(5),
                ..Default::default()
            }),
            artifacts: vec![],
            error: None,
        },
        ChildJobResult {
            success: true,
            exit_code: Some(0),
            duration_ms: 1000,
            details: ChildJobDetails::Lint(codegg_core::tool_program::child_job::LintJobResult {
                status: "clean".into(),
                ..Default::default()
            }),
            artifacts: vec![],
            error: None,
        },
    ]);

    let source = r#"
build_result = submit_job("build", {"argv": ["cargo", "build"]})
test_result = submit_job("test", {"scope": "workspace"})
lint_result = submit_job("lint", {})
emit({"build": build_result, "test": test_result, "lint": lint_result})
"#;

    let prog_result = run_program(source, &broker);
    assert_eq!(prog_result.status, ProgramStatus::Completed);
    assert_eq!(broker.submission_count(), 3);

    match &prog_result.output {
        Some(ProgramValue::Dict(pairs)) => {
            // Each value in the dict is a ToolResult containing a ChildJobResult
            for (key, val) in pairs {
                if let ProgramValue::String(k) = key {
                    let child = child_job_result_from_emitted(val);
                    assert!(child.success, "child job '{}' should succeed", k);
                }
            }
        }
        other => panic!("expected Dict, got {:?}", other),
    }
}

#[test]
fn child_job_rejected_by_broker_propagates_error() {
    let broker = NoopChildBroker;

    let source = r#"
result = submit_job("test", {})
emit(result)
"#;

    let prog_result = run_program(source, &broker);
    assert_eq!(prog_result.status, ProgramStatus::Failed);
    assert!(prog_result
        .error_message
        .as_ref()
        .unwrap()
        .contains("does not support child jobs"));
}

#[test]
fn child_job_config_passed_through_broker() {
    // Verify that the broker receives the correct typed config
    use std::sync::Mutex;

    struct ConfigCaptureBroker {
        captured: Mutex<Vec<ChildJobRequest>>,
    }

    impl ConfigCaptureBroker {
        fn new() -> Self {
            Self {
                captured: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait::async_trait]
    impl BrokerCallback for ConfigCaptureBroker {
        async fn execute_call(&self, _: &CallRequest) -> Result<CallResult, InterpreterError> {
            Ok(CallResult {
                output: ProgramValue::ToolResult(serde_json::json!({})),
                artifacts: vec![],
                success: true,
            })
        }

        async fn submit_child_job(
            &self,
            request: &ChildJobRequest,
        ) -> Result<ChildJobResult, InterpreterError> {
            self.captured.lock().unwrap().push(request.clone());
            Ok(ChildJobResult {
                success: true,
                exit_code: Some(0),
                duration_ms: 100,
                details: ChildJobDetails::Test(
                    codegg_core::tool_program::child_job::TestJobResult::default(),
                ),
                artifacts: vec![],
                error: None,
            })
        }
    }

    let broker = ConfigCaptureBroker::new();
    let source = r#"
result = submit_job("test", {"scope": "package", "timeout_secs": 60, "stall_timeout_secs": 30})
emit(result)
"#;

    let prog_result = run_program(source, &broker);
    assert_eq!(prog_result.status, ProgramStatus::Completed);

    let captured = broker.captured.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].op, ChildJobOp::Test);
    match &captured[0].config {
        ChildJobConfig::Test(cfg) => {
            assert_eq!(cfg.scope.as_deref(), Some("package"));
            assert_eq!(cfg.timeout_secs, Some(60));
            assert_eq!(cfg.stall_timeout_secs, Some(30));
        }
        other => panic!("expected Test config, got {:?}", other),
    }
}

#[test]
fn no_model_visible_polling() {
    // Programs must not poll — submit_job blocks until completion.
    // Verify the call count matches exactly the number of submit_job calls.
    let broker = ChildJobBroker::new(vec![
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
    ]);

    let source = r#"
r1 = submit_job("test", {})
r2 = submit_job("build", {})
emit({"r1": r1, "r2": r2})
"#;

    let prog_result = run_program(source, &broker);
    assert_eq!(prog_result.status, ProgramStatus::Completed);
    // Exactly 2 submit_job calls, no polling overhead
    assert_eq!(broker.submission_count(), 2);
    assert_eq!(prog_result.calls_completed, 2);
}
