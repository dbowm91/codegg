use crate::shell::projector::{
    ProjectionExactness, ProjectionKind, ProjectionRawSemantics, ProjectionResult,
};
use crate::test_runner::types::{FailureClass, TestReport, TestStatus, TimeoutKind};

/// Convert a structured [`TestReport`] into a [`ProjectionResult`] without
/// reparsing raw logs. This is the Phase 03 test-runner adapter: the report
/// is already parsed, so we format it directly into the common projection
/// shape.
pub fn test_report_to_projection(report: &TestReport) -> ProjectionResult {
    let text = format_report_body(report);
    let input_bytes = report.stdout_log.as_ref().map(|_| 0).unwrap_or(0)
        + report.stderr_log.as_ref().map(|_| 0).unwrap_or(0);

    ProjectionResult {
        text,
        projector: "test-report".to_string(),
        kind: ProjectionKind::Structured,
        exactness: ProjectionExactness::Exact,
        redaction: crate::shell::projection::RedactionState::NotApplied,
        omitted: Vec::new(),
        expansion_handles: Vec::new(),
        input_bytes,
        output_bytes: 0, // filled below
        estimated_input_tokens: None,
        estimated_output_tokens: None,
        warnings: build_warnings(report),
        raw_semantics: ProjectionRawSemantics::Unknown,
    }
    .with_output_bytes()
}

impl ProjectionResult {
    fn with_output_bytes(mut self) -> Self {
        self.output_bytes = self.text.len();
        self
    }
}

/// Format the report body for model consumption. This is deliberately
/// similar to `format_test_report` but returns a `ProjectionResult`-ready
/// string without the log-path footer (those are available via expansion
/// handles in a future phase).
fn format_report_body(report: &TestReport) -> String {
    let mut out = String::new();

    let status_str = match report.status {
        TestStatus::Passed => "Test run passed.",
        TestStatus::Failed => "Test run failed.",
        TestStatus::TimedOut => "Test run timed out.",
        TestStatus::Cancelled => "Test run cancelled.",
        TestStatus::Error => "Test run could not be started.",
    };
    out.push_str(status_str);
    out.push('\n');

    out.push('\n');
    out.push_str("Command:\n");
    out.push_str(&report.argv.join(" "));
    out.push('\n');

    if let Some(ref label) = report.scope_label {
        if let Some(run_id) = label.strip_prefix("previous-failures:") {
            out.push('\n');
            out.push_str("Rerun source:\n");
            out.push_str(&format!("previous failed run {run_id}\n"));
        }
    }

    out.push('\n');
    out.push_str("Duration:\n");
    let secs = report.duration_ms as f64 / 1000.0;
    out.push_str(&format!("{secs:.2}s"));
    out.push('\n');

    out.push('\n');
    out.push_str("Exit code:\n");
    match report.exit_code {
        Some(code) => out.push_str(&code.to_string()),
        None => out.push_str("unavailable"),
    }
    out.push('\n');

    let failure_class = compute_failure_class(report);
    if failure_class != FailureClass::Passed {
        out.push('\n');
        out.push_str("Failure class:\n");
        out.push_str(failure_class.as_str());
        out.push('\n');
    }

    out.push('\n');
    out.push_str("Summary:\n");
    out.push_str(&report.summary);
    out.push('\n');

    if let Some(ref timeout) = report.timeout {
        out.push('\n');
        out.push_str("Timeout details:\n");
        let kind_str = match timeout.kind {
            TimeoutKind::WallClock => "wall_clock",
            TimeoutKind::NoOutput => "no_output",
            TimeoutKind::NoProgress => "no_progress",
        };
        out.push_str(&format!("kind: {kind_str}\n"));
        let elapsed_secs = timeout.elapsed_ms as f64 / 1000.0;
        out.push_str(&format!("elapsed: {elapsed_secs:.1}s\n"));
        if let Some(ref last) = timeout.last_output {
            let excerpt = truncate_utf8(last, 2000);
            out.push_str(&format!("last_output: {excerpt}\n"));
        }
    }

    if !report.failures.is_empty() {
        out.push('\n');
        let shown = report.failures.len().min(5);
        out.push_str(&format!("Primary failures ({shown}):\n"));
        for (i, f) in report.failures[..shown].iter().enumerate() {
            out.push_str(&format!("{}. ", i + 1));
            if let Some(ref name) = f.name {
                out.push_str(name);
                if f.file.is_some() || f.line.is_some() {
                    out.push_str(" (");
                    if let Some(ref file) = f.file {
                        out.push_str(file);
                        if let Some(line) = f.line {
                            out.push_str(&format!(":{line}"));
                        }
                    } else if let Some(line) = f.line {
                        out.push_str(&format!("line {line}"));
                    }
                    out.push(')');
                }
                out.push_str(": ");
            } else if let Some(ref file) = f.file {
                out.push_str(file);
                if let Some(line) = f.line {
                    out.push_str(&format!(":{line}"));
                }
                out.push_str(": ");
            }
            let msg = truncate_utf8(&f.message, 2000);
            out.push_str(&msg);
            out.push('\n');
        }
        if report.failures.len() > 5 {
            let remaining = report.failures.len() - 5;
            out.push_str(&format!(
                "... {remaining} additional failures omitted; see full log.\n"
            ));
        }
    }

    if report.output_truncated {
        out.push('\n');
        out.push_str("Note: output was truncated; see full logs for complete output.\n");
    }

    out
}

fn compute_failure_class(report: &TestReport) -> FailureClass {
    if report.status == TestStatus::Passed {
        return FailureClass::Passed;
    }
    if let Some(ref timeout) = report.timeout {
        return match timeout.kind {
            TimeoutKind::WallClock => FailureClass::TimeoutWallClock,
            TimeoutKind::NoOutput => FailureClass::TimeoutNoOutput,
            TimeoutKind::NoProgress => FailureClass::TimeoutNoOutput,
        };
    }
    if !report.failures.is_empty() {
        return report.failures[0].failure_class;
    }
    match report.status {
        TestStatus::Failed => FailureClass::NonzeroExit,
        TestStatus::Error => FailureClass::SpawnError,
        _ => FailureClass::UnknownFailure,
    }
}

fn build_warnings(report: &TestReport) -> Vec<String> {
    let mut warnings = Vec::new();
    if report.output_truncated {
        warnings.push("output was truncated; see full logs".to_string());
    }
    if report.failures.len() > 5 {
        let extra = report.failures.len() - 5;
        warnings.push(format!("{extra} additional failures omitted"));
    }
    warnings
}

fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        let mut end = max_bytes;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_runner::types::{TestFailure, TestTimeout, TimeoutKind};
    use std::path::PathBuf;

    fn base_report(status: TestStatus) -> TestReport {
        TestReport {
            status,
            argv: vec!["cargo".into(), "test".into()],
            cwd: PathBuf::from("/workspace"),
            duration_ms: 12345,
            exit_code: Some(1),
            summary: "2 passed, 1 failed".into(),
            failures: vec![],
            timeout: None,
            log_dir: Some(PathBuf::from("/workspace/.codegg/test-runs/run1")),
            stdout_log: Some(PathBuf::from(
                "/workspace/.codegg/test-runs/run1/stdout.log",
            )),
            stderr_log: Some(PathBuf::from(
                "/workspace/.codegg/test-runs/run1/stderr.log",
            )),
            output_truncated: false,
            scope_label: Some("auto-rust".to_string()),
            previous_run_id: None,
        }
    }

    #[test]
    fn passed_report_produces_valid_projection() {
        let report = base_report(TestStatus::Passed);
        let result = test_report_to_projection(&report);
        assert_eq!(result.projector, "test-report");
        assert_eq!(result.kind, ProjectionKind::Structured);
        assert_eq!(result.exactness, ProjectionExactness::Exact);
        assert!(result.text.starts_with("Test run passed."));
        assert!(result.text.contains("Summary:\n2 passed, 1 failed"));
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn failed_report_produces_valid_projection() {
        let mut report = base_report(TestStatus::Failed);
        report.failures.push(TestFailure {
            name: Some("test_foo".into()),
            file: Some("src/lib.rs".into()),
            line: Some(42),
            message: "assertion failed".into(),
            failure_class: FailureClass::RustTestFailure,
        });
        let result = test_report_to_projection(&report);
        assert!(result.text.starts_with("Test run failed."));
        assert!(result.text.contains("Failure class:\nrust_test_failure"));
        assert!(result
            .text
            .contains("test_foo (src/lib.rs:42): assertion failed"));
    }

    #[test]
    fn timeout_report_produces_valid_projection() {
        let mut report = base_report(TestStatus::TimedOut);
        report.exit_code = None;
        report.timeout = Some(TestTimeout {
            kind: TimeoutKind::NoOutput,
            elapsed_ms: 30000,
            last_output: Some("waiting...".into()),
        });
        let result = test_report_to_projection(&report);
        assert!(result.text.starts_with("Test run timed out."));
        assert!(result.text.contains("kind: no_output"));
        assert!(result.text.contains("elapsed: 30.0s"));
    }

    #[test]
    fn output_bytes_matches_text_length() {
        let report = base_report(TestStatus::Passed);
        let result = test_report_to_projection(&report);
        assert_eq!(result.output_bytes, result.text.len());
    }

    #[test]
    fn truncation_warning_added_when_output_truncated() {
        let mut report = base_report(TestStatus::Failed);
        report.output_truncated = true;
        let result = test_report_to_projection(&report);
        assert!(result.warnings.iter().any(|w| w.contains("truncated")));
        assert!(result.text.contains("Note: output was truncated"));
    }

    #[test]
    fn many_failures_get_warning() {
        let mut report = base_report(TestStatus::Failed);
        report.failures = (0..8)
            .map(|i| TestFailure {
                name: Some(format!("test_{i}")),
                file: None,
                line: None,
                message: format!("fail {i}"),
                failure_class: FailureClass::RustTestFailure,
            })
            .collect();
        let result = test_report_to_projection(&report);
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("3 additional failures")));
        assert!(result.text.contains("Primary failures (5):"));
    }

    #[test]
    fn error_report_shows_could_not_start() {
        let report = base_report(TestStatus::Error);
        let result = test_report_to_projection(&report);
        assert!(result.text.starts_with("Test run could not be started."));
    }

    #[test]
    fn previous_failures_rerun_source_shown() {
        let mut report = base_report(TestStatus::Failed);
        report.scope_label = Some("previous-failures:run-42".to_string());
        let result = test_report_to_projection(&report);
        assert!(result.text.contains("Rerun source:"));
        assert!(result.text.contains("previous failed run run-42"));
    }

    #[test]
    fn redaction_state_is_not_applied() {
        let report = base_report(TestStatus::Passed);
        let result = test_report_to_projection(&report);
        assert!(matches!(
            result.redaction,
            crate::shell::projection::RedactionState::NotApplied
        ));
    }

    #[test]
    fn no_omitted_ranges_or_expansion_handles() {
        let report = base_report(TestStatus::Passed);
        let result = test_report_to_projection(&report);
        assert!(result.omitted.is_empty());
        assert!(result.expansion_handles.is_empty());
    }

    #[test]
    fn compile_error_failure_class_preserved() {
        let mut report = base_report(TestStatus::Failed);
        report.failures.push(TestFailure {
            name: Some("E0432".into()),
            file: Some("src/main.rs".into()),
            line: Some(5),
            message: "unresolved import `foo`".into(),
            failure_class: FailureClass::RustCompileError,
        });
        let result = test_report_to_projection(&report);
        assert!(result.text.contains("Failure class:\nrust_compile_error"));
        assert!(result
            .text
            .contains("E0432 (src/main.rs:5): unresolved import `foo`"));
    }
}
