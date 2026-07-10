use std::collections::HashSet;

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
        projection_id: crate::shell::projector::ProjectionId::new(),
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
        source_spans: Vec::new(),
        redaction_records: Vec::new(),
        rtk_metadata: crate::shell::projector::RtkResultMetadata::default(),
    }
    .with_output_bytes()
}

/// Delta between two test reports, showing new, resolved, and unchanged failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestDelta {
    pub new_failures: Vec<String>,
    pub resolved_failures: Vec<String>,
    pub unchanged_count: usize,
}

/// Compute the delta between two test reports.
///
/// Failure identity is based on `name` (when present), falling back
/// to a composite of `file:line`.
pub fn compute_test_delta(current: &TestReport, previous: &TestReport) -> TestDelta {
    let prev_keys: HashSet<String> = previous.failures.iter().map(failure_key).collect();
    let curr_keys: HashSet<String> = current.failures.iter().map(failure_key).collect();

    let new_failures: Vec<String> = current
        .failures
        .iter()
        .filter(|f| !prev_keys.contains(&failure_key(f)))
        .map(failure_label)
        .collect();

    let resolved_failures: Vec<String> = previous
        .failures
        .iter()
        .filter(|f| !curr_keys.contains(&failure_key(f)))
        .map(failure_label)
        .collect();

    let unchanged_count = curr_keys.intersection(&prev_keys).count();

    TestDelta {
        new_failures,
        resolved_failures,
        unchanged_count,
    }
}

fn failure_key(f: &crate::test_runner::types::TestFailure) -> String {
    if let Some(ref name) = f.name {
        name.clone()
    } else if let Some(ref file) = f.file {
        match f.line {
            Some(line) => format!("{file}:{line}"),
            None => file.clone(),
        }
    } else {
        f.message.clone()
    }
}

fn failure_label(f: &crate::test_runner::types::TestFailure) -> String {
    if let Some(ref name) = f.name {
        if let Some(ref file) = f.file {
            match f.line {
                Some(line) => format!("{name} ({file}:{line})"),
                None => format!("{name} ({file})"),
            }
        } else {
            name.clone()
        }
    } else if let Some(ref file) = f.file {
        match f.line {
            Some(line) => format!("{file}:{line}"),
            None => file.clone(),
        }
    } else {
        truncate_utf8(&f.message, 80)
    }
}

/// Convert a structured [`TestReport`] into a [`ProjectionResult`], optionally
/// including a delta section when a previous report is provided.
pub fn test_report_to_projection_with_delta(
    report: &TestReport,
    previous_report: Option<&TestReport>,
) -> ProjectionResult {
    let mut result = test_report_to_projection(report);

    if let Some(prev) = previous_report {
        let delta = compute_test_delta(report, prev);
        let delta_text = format_delta(&delta);

        if !delta_text.is_empty() {
            result.text.push_str(&delta_text);
            result.output_bytes = result.text.len();

            if delta.new_failures.len() > 10 {
                result.warnings.push(format!(
                    "{} new failures since last run",
                    delta.new_failures.len()
                ));
            }
            if delta.resolved_failures.len() > 10 {
                result.warnings.push(format!(
                    "{} failures resolved since last run",
                    delta.resolved_failures.len()
                ));
            }
        }
    }

    result
}

fn format_delta(delta: &TestDelta) -> String {
    if delta.new_failures.is_empty() && delta.resolved_failures.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push('\n');
    out.push_str("--- Delta vs previous run ---\n");

    if !delta.new_failures.is_empty() {
        out.push('\n');
        out.push_str(&format!("New failures ({}):\n", delta.new_failures.len()));
        for name in &delta.new_failures {
            out.push_str(&format!("  + {name}\n"));
        }
    }

    if !delta.resolved_failures.is_empty() {
        out.push('\n');
        out.push_str(&format!(
            "Resolved failures ({}):\n",
            delta.resolved_failures.len()
        ));
        for name in &delta.resolved_failures {
            out.push_str(&format!("  - {name}\n"));
        }
    }

    if delta.unchanged_count > 0 {
        out.push('\n');
        out.push_str(&format!("Unchanged failures: {}\n", delta.unchanged_count));
    }

    out
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

    fn report_with_failures(names: &[&str]) -> TestReport {
        let mut report = base_report(TestStatus::Failed);
        for name in names {
            report.failures.push(TestFailure {
                name: Some((*name).into()),
                file: None,
                line: None,
                message: format!("fail {name}"),
                failure_class: FailureClass::RustTestFailure,
            });
        }
        report
    }

    #[test]
    fn delta_no_previous_report() {
        let report = report_with_failures(&["test_a"]);
        let result = test_report_to_projection_with_delta(&report, None);
        assert!(!result.text.contains("Delta vs previous run"));
    }

    #[test]
    fn delta_identical_failures() {
        let current = report_with_failures(&["test_a", "test_b"]);
        let previous = report_with_failures(&["test_a", "test_b"]);
        let result = test_report_to_projection_with_delta(&current, Some(&previous));
        assert!(!result.text.contains("Delta vs previous run"));
    }

    #[test]
    fn delta_new_failures() {
        let current = report_with_failures(&["test_a", "test_b", "test_c"]);
        let previous = report_with_failures(&["test_a"]);
        let result = test_report_to_projection_with_delta(&current, Some(&previous));
        assert!(result.text.contains("Delta vs previous run"));
        assert!(result.text.contains("New failures (2):"));
        assert!(result.text.contains("+ test_b"));
        assert!(result.text.contains("+ test_c"));
    }

    #[test]
    fn delta_resolved_failures() {
        let current = report_with_failures(&["test_a"]);
        let previous = report_with_failures(&["test_a", "test_b", "test_c"]);
        let result = test_report_to_projection_with_delta(&current, Some(&previous));
        assert!(result.text.contains("Resolved failures (2):"));
        assert!(result.text.contains("- test_b"));
        assert!(result.text.contains("- test_c"));
        assert!(result.text.contains("Unchanged failures: 1"));
    }

    #[test]
    fn delta_mixed_new_and_resolved() {
        let current = report_with_failures(&["test_a", "test_c", "test_d"]);
        let previous = report_with_failures(&["test_a", "test_b", "test_c"]);
        let result = test_report_to_projection_with_delta(&current, Some(&previous));
        assert!(result.text.contains("+ test_d"));
        assert!(result.text.contains("- test_b"));
        assert!(result.text.contains("Unchanged failures: 2"));
    }

    #[test]
    fn delta_no_failures_either_side() {
        let current = base_report(TestStatus::Passed);
        let previous = base_report(TestStatus::Passed);
        let result = test_report_to_projection_with_delta(&current, Some(&previous));
        assert!(!result.text.contains("Delta vs previous run"));
    }

    #[test]
    fn delta_output_bytes_updated() {
        let current = report_with_failures(&["test_x"]);
        let previous = report_with_failures(&["test_y"]);
        let result = test_report_to_projection_with_delta(&current, Some(&previous));
        assert_eq!(result.output_bytes, result.text.len());
    }

    #[test]
    fn compute_test_delta_empty_reports() {
        let current = base_report(TestStatus::Passed);
        let previous = base_report(TestStatus::Passed);
        let delta = compute_test_delta(&current, &previous);
        assert!(delta.new_failures.is_empty());
        assert!(delta.resolved_failures.is_empty());
        assert_eq!(delta.unchanged_count, 0);
    }

    #[test]
    fn compute_test_delta_failure_key_fallback_to_file_line() {
        let current = TestReport {
            status: TestStatus::Failed,
            argv: vec!["cargo".into(), "test".into()],
            cwd: std::path::PathBuf::from("/workspace"),
            duration_ms: 0,
            exit_code: Some(1),
            summary: "1 failed".into(),
            failures: vec![TestFailure {
                name: None,
                file: Some("src/lib.rs".into()),
                line: Some(10),
                message: "assertion failed".into(),
                failure_class: FailureClass::RustTestFailure,
            }],
            timeout: None,
            log_dir: None,
            stdout_log: None,
            stderr_log: None,
            output_truncated: false,
            scope_label: None,
            previous_run_id: None,
        };
        let mut previous = base_report(TestStatus::Failed);
        previous.failures.push(TestFailure {
            name: None,
            file: Some("src/lib.rs".into()),
            line: Some(10),
            message: "assertion failed".into(),
            failure_class: FailureClass::RustTestFailure,
        });
        let delta = compute_test_delta(&current, &previous);
        assert!(delta.new_failures.is_empty());
        assert_eq!(delta.unchanged_count, 1);
    }

    #[test]
    fn delta_large_delta_adds_warnings() {
        let mut current = base_report(TestStatus::Failed);
        current.failures = (0..15)
            .map(|i| TestFailure {
                name: Some(format!("new_test_{i}")),
                file: None,
                line: None,
                message: format!("fail {i}"),
                failure_class: FailureClass::RustTestFailure,
            })
            .collect();
        let previous = base_report(TestStatus::Failed);
        let result = test_report_to_projection_with_delta(&current, Some(&previous));
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("15 new failures")));
    }
}
