use crate::test_runner::types::{FailureClass, TestReport, TestStatus, TimeoutKind};

const MAX_DISPLAY_FAILURES: usize = 5;
const MAX_FAILURE_MESSAGE_BYTES: usize = 2000;
const MAX_TIMEOUT_EXCERPT_BYTES: usize = 2000;
/// Default ceiling when the caller does not specify `max_report_bytes`.
/// Matches the runner's `DEFAULT_MAX_REPORT_BYTES`.
pub const DEFAULT_MAX_REPORT_BYTES: usize = 20_000;

pub fn format_test_report(report: &TestReport) -> String {
    format_test_report_with_cap(report, DEFAULT_MAX_REPORT_BYTES)
}

/// Format a report and enforce a hard total byte ceiling. When the formatted
/// output exceeds `max_report_bytes`, the body is head-truncated at a UTF-8
/// character boundary, a truncation marker is inserted, and the log-path
/// footer is reattached so callers always see where to read the full output.
pub fn format_test_report_with_cap(report: &TestReport, max_report_bytes: usize) -> String {
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
            let excerpt = truncate_utf8(last, MAX_TIMEOUT_EXCERPT_BYTES);
            out.push_str(&format!("last_output: {excerpt}\n"));
        }
    }

    if !report.failures.is_empty() {
        out.push('\n');
        let shown = report.failures.len().min(MAX_DISPLAY_FAILURES);
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
            let msg = truncate_utf8(&f.message, MAX_FAILURE_MESSAGE_BYTES);
            out.push_str(&msg);
            out.push('\n');
        }
        if report.failures.len() > MAX_DISPLAY_FAILURES {
            let remaining = report.failures.len() - MAX_DISPLAY_FAILURES;
            out.push_str(&format!(
                "... {remaining} additional failures omitted; see full log.\n"
            ));
        }
    }

    let log_footer = build_log_footer(report);

    if report.output_truncated {
        out.push('\n');
        out.push_str("Note: output was truncated; see full logs for complete output.\n");
    }

    let total_with_footer = out.len() + log_footer.len();
    if total_with_footer > max_report_bytes {
        // Reserve space for the footer + truncation marker so the model
        // always sees where to read the full output.
        let suffix = "\n... report body truncated to fit max_report_bytes; see full logs.\n";
        let footer_budget = log_footer.len() + suffix.len();
        let body_budget = max_report_bytes.saturating_sub(footer_budget);
        if body_budget == 0 {
            // Cap is too small to fit anything besides the footer; just
            // emit the footer and let the caller know.
            let mut minimal = String::with_capacity(log_footer.len() + suffix.len());
            minimal.push_str(suffix);
            minimal.push_str(&log_footer);
            return minimal;
        }
        let mut end = body_budget;
        while end > 0 && !out.is_char_boundary(end) {
            end -= 1;
        }
        let mut trimmed = String::with_capacity(end + suffix.len() + log_footer.len());
        trimmed.push_str(&out[..end]);
        trimmed.push_str(suffix);
        trimmed.push_str(&log_footer);
        return trimmed;
    }

    out.push_str(&log_footer);
    out
}

/// Build the log-path footer block that should always survive report
/// truncation. Lives as a separate helper so the cap-aware formatter can
/// reserve space for it before head-truncating the report body.
fn build_log_footer(report: &TestReport) -> String {
    let mut footer = String::new();
    footer.push('\n');
    footer.push_str("Logs:\n");
    if let Some(ref path) = report.stdout_log {
        footer.push_str(&format!("stdout: {}\n", path.display()));
    }
    if let Some(ref path) = report.stderr_log {
        footer.push_str(&format!("stderr: {}\n", path.display()));
    }
    if let Some(ref path) = report.log_dir {
        let report_path = path.join("report.json");
        footer.push_str(&format!("report: {}\n", report_path.display()));
    }
    footer
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
    fn failed_report_has_stable_sections() {
        let mut report = base_report(TestStatus::Failed);
        report.failures.push(TestFailure {
            name: Some("test_foo".into()),
            file: Some("src/lib.rs".into()),
            line: Some(42),
            message: "assertion failed".into(),
            failure_class: FailureClass::RustTestFailure,
        });
        let text = format_test_report(&report);
        assert!(text.starts_with("Test run failed."));
        assert!(text.contains("Command:\ncargo test"));
        assert!(text.contains("Duration:\n12.35s"));
        assert!(text.contains("Exit code:\n1"));
        assert!(text.contains("Failure class:\nrust_test_failure"));
        assert!(text.contains("Summary:\n2 passed, 1 failed"));
        assert!(text.contains("Primary failures (1):"));
        assert!(text.contains("test_foo (src/lib.rs:42): assertion failed"));
        assert!(text.contains("Logs:\n"));
        assert!(text.contains("stdout:"));
        assert!(text.contains("stderr:"));
        assert!(text.contains("report:"));
    }

    #[test]
    fn passed_report_suppresses_failure_sections() {
        let report = base_report(TestStatus::Passed);
        let text = format_test_report(&report);
        assert!(text.starts_with("Test run passed."));
        assert!(!text.contains("Failure class:"));
        assert!(!text.contains("Primary failures"));
        assert!(text.contains("Summary:\n2 passed, 1 failed"));
    }

    #[test]
    fn timeout_report_includes_timeout_kind_and_last_output() {
        let mut report = base_report(TestStatus::TimedOut);
        report.exit_code = None;
        report.timeout = Some(TestTimeout {
            kind: TimeoutKind::NoOutput,
            elapsed_ms: 30000,
            last_output: Some("waiting...".into()),
        });
        let text = format_test_report(&report);
        assert!(text.starts_with("Test run timed out."));
        assert!(text.contains("kind: no_output"));
        assert!(text.contains("elapsed: 30.0s"));
        assert!(text.contains("last_output: waiting..."));
    }

    #[test]
    fn report_omits_extra_failures_after_limit() {
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
        let text = format_test_report(&report);
        assert!(text.contains("Primary failures (5):"));
        assert!(text.contains("3 additional failures omitted; see full log."));
    }

    #[test]
    fn report_respects_max_report_bytes() {
        let mut report = base_report(TestStatus::Failed);
        report.failures.push(TestFailure {
            name: Some("test_long".into()),
            file: None,
            line: None,
            message: "x".repeat(3000),
            failure_class: FailureClass::RustTestFailure,
        });
        let text = format_test_report(&report);
        let msg_line = text.lines().find(|l| l.contains("test_long")).unwrap();
        assert!(msg_line.len() < 2200);
        assert!(msg_line.ends_with("..."));
    }

    #[test]
    fn report_caps_total_bytes_when_max_report_bytes_is_low() {
        let mut report = base_report(TestStatus::Failed);
        for i in 0..20 {
            report.failures.push(TestFailure {
                name: Some(format!("test_{i}")),
                file: Some(format!("src/test_{i}.rs")),
                line: Some(i as u32 + 1),
                message: "x".repeat(1500),
                failure_class: FailureClass::RustTestFailure,
            });
        }
        let cap = 4000usize;
        let text = format_test_report_with_cap(&report, cap);
        assert!(
            text.len() <= cap,
            "report len {} exceeded cap {}",
            text.len(),
            cap
        );
        assert!(
            text.contains("report body truncated to fit max_report_bytes"),
            "expected truncation marker in: {text}"
        );
    }

    #[test]
    fn report_caps_total_bytes_preserves_log_paths() {
        let mut report = base_report(TestStatus::Failed);
        for i in 0..30 {
            report.failures.push(TestFailure {
                name: Some(format!("test_{i}")),
                file: None,
                line: None,
                message: "y".repeat(2000),
                failure_class: FailureClass::RustTestFailure,
            });
        }
        // Cap is set high enough that the failure section is truncated but
        // the log-path footer still survives.
        let cap = 8000usize;
        let text = format_test_report_with_cap(&report, cap);
        assert!(text.len() <= cap);
        // Log paths and primary status must remain even after truncation.
        assert!(text.starts_with("Test run failed."));
        assert!(text.contains("stdout:"));
        assert!(text.contains("stderr:"));
        assert!(text.contains("report:"));
    }

    #[test]
    fn report_caps_under_total_byte_boundary_does_not_truncate() {
        let report = base_report(TestStatus::Passed);
        let text = format_test_report_with_cap(&report, 1_000_000);
        assert!(
            !text.contains("report body truncated to fit max_report_bytes"),
            "small report should not trigger truncation"
        );
    }

    #[test]
    fn report_includes_full_log_paths() {
        let mut report = base_report(TestStatus::Failed);
        report.stdout_log = Some(PathBuf::from("/tmp/stdout.log"));
        report.stderr_log = Some(PathBuf::from("/tmp/stderr.log"));
        report.log_dir = Some(PathBuf::from("/tmp/run1"));
        let text = format_test_report(&report);
        assert!(text.contains("stdout: /tmp/stdout.log"));
        assert!(text.contains("stderr: /tmp/stderr.log"));
        assert!(text.contains("report: /tmp/run1/report.json"));
    }

    #[test]
    fn report_shows_truncation_note() {
        let mut report = base_report(TestStatus::Failed);
        report.output_truncated = true;
        let text = format_test_report(&report);
        assert!(text.contains("Note: output was truncated"));
    }

    #[test]
    fn error_report_shows_could_not_start() {
        let report = base_report(TestStatus::Error);
        let text = format_test_report(&report);
        assert!(text.starts_with("Test run could not be started."));
    }

    #[test]
    fn compile_error_failure_class_displayed() {
        let mut report = base_report(TestStatus::Failed);
        report.failures.push(TestFailure {
            name: Some("E0432".into()),
            file: Some("src/main.rs".into()),
            line: Some(5),
            message: "unresolved import `foo`".into(),
            failure_class: FailureClass::RustCompileError,
        });
        let text = format_test_report(&report);
        assert!(text.contains("Failure class:\nrust_compile_error"));
        assert!(text.contains("E0432 (src/main.rs:5): unresolved import `foo`"));
    }
}
