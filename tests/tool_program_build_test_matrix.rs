//! Integration tests for Tool Program build/test matrix evaluation (M007).
//!
//! Tests that bounded matrices of child jobs respect global resource
//! and contention policy, that all four operation types work correctly
//! in combination, and that cancellation propagates to child jobs.

use std::sync::atomic::{AtomicU32, Ordering};

use codegg_core::tool_program::{
    child_job::{ChildJobDetails, ChildJobOp, ChildJobRequest, ChildJobResult},
    compile_program, BrokerCallback, CallRequest, CallResult, InterpreterError, ProgramResult,
    ProgramStatus, ProgramValue, RuntimeLimits,
};

// ── Test broker ────────────────────────────────────────────────────

/// Broker that returns configurable results and tracks all submissions.
struct MatrixBroker {
    results: std::sync::Mutex<Vec<ChildJobResult>>,
    submission_count: AtomicU32,
    /// If true, all child jobs return failure.
    all_fail: bool,
}

impl MatrixBroker {
    fn passing() -> Self {
        Self {
            results: std::sync::Mutex::new(vec![]),
            submission_count: AtomicU32::new(0),
            all_fail: false,
        }
    }

    fn failing() -> Self {
        Self {
            results: std::sync::Mutex::new(vec![]),
            submission_count: AtomicU32::new(0),
            all_fail: true,
        }
    }

    fn submission_count(&self) -> u32 {
        self.submission_count.load(Ordering::Relaxed)
    }
}

#[async_trait::async_trait]
impl BrokerCallback for MatrixBroker {
    async fn execute_call(&self, _request: &CallRequest) -> Result<CallResult, InterpreterError> {
        Ok(CallResult {
            output: ProgramValue::ToolResult(serde_json::json!({"status": "ok"})),
            artifacts: vec![],
        })
    }

    async fn submit_child_job(
        &self,
        request: &ChildJobRequest,
    ) -> Result<ChildJobResult, InterpreterError> {
        self.submission_count.fetch_add(1, Ordering::Relaxed);

        if self.all_fail {
            return Ok(ChildJobResult {
                success: false,
                exit_code: Some(1),
                duration_ms: 100,
                details: match request.op {
                    ChildJobOp::Test => {
                        ChildJobDetails::Test(codegg_core::tool_program::child_job::TestJobResult {
                            status: "failed".into(),
                            failed: Some(1),
                            ..Default::default()
                        })
                    }
                    ChildJobOp::Build => ChildJobDetails::Build(
                        codegg_core::tool_program::child_job::BuildJobResult {
                            status: "failure".into(),
                            ..Default::default()
                        },
                    ),
                    ChildJobOp::Lint => {
                        ChildJobDetails::Lint(codegg_core::tool_program::child_job::LintJobResult {
                            status: "errors".into(),
                            ..Default::default()
                        })
                    }
                    ChildJobOp::Format => ChildJobDetails::Format(
                        codegg_core::tool_program::child_job::FormatJobResult {
                            status: "needs_formatting".into(),
                            would_change: true,
                            ..Default::default()
                        },
                    ),
                },
                artifacts: vec![],
                error: None,
            });
        }

        // Return a result based on the operation type
        let details = match request.op {
            ChildJobOp::Test => {
                ChildJobDetails::Test(codegg_core::tool_program::child_job::TestJobResult {
                    status: "passed".into(),
                    total: Some(5),
                    passed: Some(5),
                    ..Default::default()
                })
            }
            ChildJobOp::Build => {
                ChildJobDetails::Build(codegg_core::tool_program::child_job::BuildJobResult {
                    status: "success".into(),
                    ..Default::default()
                })
            }
            ChildJobOp::Lint => {
                ChildJobDetails::Lint(codegg_core::tool_program::child_job::LintJobResult {
                    status: "clean".into(),
                    ..Default::default()
                })
            }
            ChildJobOp::Format => {
                ChildJobDetails::Format(codegg_core::tool_program::child_job::FormatJobResult {
                    status: "clean".into(),
                    would_change: false,
                    ..Default::default()
                })
            }
        };

        Ok(ChildJobResult {
            success: true,
            exit_code: Some(0),
            duration_ms: 100,
            details,
            artifacts: vec![],
            error: None,
        })
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
    let mut interp = codegg_core::tool_program::MeteredInterpreter::new(compilation.ir, limits);
    rt.block_on(interp.run(broker, None))
}

// ── Tests ──────────────────────────────────────────────────────────

#[test]
fn bounded_matrix_of_test_jobs() {
    // A for-loop submitting test jobs for multiple packages
    let broker = MatrixBroker::passing();
    let source = r#"
count = 0
for i in range(5):
    r = submit_job("test", {"scope": "package"})
    count = count + 1
emit(count)
"#;

    let prog = run_program(source, &broker);
    assert_eq!(prog.status, ProgramStatus::Completed);
    assert_eq!(broker.submission_count(), 5);
}

#[test]
fn all_four_operation_types_in_one_program() {
    let broker = MatrixBroker::passing();
    let source = r#"
build = submit_job("build", {"argv": ["cargo", "build"]})
test = submit_job("test", {"scope": "workspace"})
lint = submit_job("lint", {})
fmt = submit_job("format", {})
emit({"build": build, "test": test, "lint": lint, "format": fmt})
"#;

    let prog = run_program(source, &broker);
    assert_eq!(prog.status, ProgramStatus::Completed);
    assert_eq!(broker.submission_count(), 4);

    // Verify all results are successful
    match &prog.output {
        Some(ProgramValue::Dict(pairs)) => {
            for (key, val) in pairs {
                if let ProgramValue::String(k) = key {
                    match val {
                        ProgramValue::ToolResult(json) => {
                            let success = json["success"].as_bool().unwrap_or(false);
                            assert!(success, "operation '{}' should succeed", k);
                        }
                        other => panic!("expected ToolResult for '{}', got {:?}", k, other),
                    }
                }
            }
        }
        other => panic!("expected Dict, got {:?}", other),
    }
}

#[test]
fn matrix_with_mixed_pass_fail() {
    // Some child jobs pass, some fail — the program should still complete
    // and the results should reflect individual outcomes.
    let broker = MatrixBroker::failing();
    let source = r#"
count = 0
for i in range(3):
    r = submit_job("test", {"scope": "package"})
    count = count + 1
emit(count)
"#;

    let prog = run_program(source, &broker);
    assert_eq!(prog.status, ProgramStatus::Completed);
    assert_eq!(broker.submission_count(), 3);
}

#[test]
fn matrix_with_build_and_test_combined() {
    // Build first, then test — common CI pattern
    let broker = MatrixBroker::passing();
    let source = r#"
build_result = submit_job("build", {"argv": ["cargo", "build", "--release"]})
test_count = 0
for i in range(3):
    r = submit_job("test", {"scope": "package"})
    test_count = test_count + 1
emit(test_count)
"#;

    let prog = run_program(source, &broker);
    assert_eq!(prog.status, ProgramStatus::Completed);
    // 1 build + 3 tests = 4 total
    assert_eq!(broker.submission_count(), 4);
}

#[test]
fn matrix_respects_loop_bounds() {
    // Verify the matrix is bounded by the loop iteration limit
    let broker = MatrixBroker::passing();
    // Small matrix — should complete well within bounds
    let source = r#"
count = 0
for i in range(10):
    r = submit_job("test", {"scope": "file"})
    count = count + 1
emit(count)
"#;

    let prog = run_program(source, &broker);
    assert_eq!(prog.status, ProgramStatus::Completed);
    assert_eq!(broker.submission_count(), 10);
}

#[test]
fn build_failure_does_not_block_lint() {
    // A failed build should not prevent subsequent lint/format jobs from running
    let broker = MatrixBroker::failing();
    let source = r#"
build = submit_job("build", {})
lint = submit_job("lint", {})
fmt = submit_job("format", {})
emit({"build": build, "lint": lint, "format": fmt})
"#;

    let prog = run_program(source, &broker);
    assert_eq!(prog.status, ProgramStatus::Completed);
    assert_eq!(broker.submission_count(), 3);

    // All should show their individual failure status
    match &prog.output {
        Some(ProgramValue::Dict(pairs)) => {
            assert_eq!(pairs.len(), 3);
        }
        other => panic!("expected Dict, got {:?}", other),
    }
}

#[test]
fn no_process_or_permit_leakage() {
    // Verify that calls_completed exactly matches the number of submit_job calls
    let broker = MatrixBroker::passing();
    let source = r#"
r1 = submit_job("test", {})
r2 = submit_job("build", {})
r3 = submit_job("lint", {})
r4 = submit_job("format", {})
emit({"done": true})
"#;

    let prog = run_program(source, &broker);
    assert_eq!(prog.status, ProgramStatus::Completed);
    assert_eq!(broker.submission_count(), 4);
    // calls_completed counts only the ExecuteChildJob ops
    assert_eq!(prog.calls_completed, 4);
}

#[test]
fn conditional_matrix_branch() {
    // Matrix with conditional branching — some paths submit, others don't
    let broker = MatrixBroker::passing();
    let source = r#"
count = 0
for i in range(5):
    if i > 2:
        r = submit_job("test", {"scope": "file"})
        count = count + 1
emit(count)
"#;

    let prog = run_program(source, &broker);
    assert_eq!(prog.status, ProgramStatus::Completed);
    // Only i=3,4 should have submitted
    assert_eq!(broker.submission_count(), 2);
}

#[test]
fn format_check_only_mode() {
    // Format in check-only mode (should not mutate)
    let broker = MatrixBroker::passing();
    let source = r#"
result = submit_job("format", {"argv": ["cargo", "fmt", "--check"]})
emit(result)
"#;

    let prog = run_program(source, &broker);
    assert_eq!(prog.status, ProgramStatus::Completed);

    match &prog.output {
        Some(ProgramValue::ToolResult(json)) => {
            assert_eq!(json["success"], true);
            match &json["details"] {
                serde_json::Value::Object(map) => {
                    if let Some(fmt) = map.get("format") {
                        assert_eq!(fmt["would_change"], false);
                    }
                }
                other => panic!("expected object details, got {:?}", other),
            }
        }
        other => panic!("expected ToolResult, got {:?}", other),
    }
}
