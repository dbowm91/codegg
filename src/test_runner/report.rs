use crate::test_runner::types::{TestReport, TestStatus, TimeoutKind};

const MAX_DISPLAY_FAILURES: usize = 5;

pub fn format_test_report(report: &TestReport) -> String {
    let mut out = String::new();

    let status_str = match report.status {
        TestStatus::Passed => "PASSED",
        TestStatus::Failed => "FAILED",
        TestStatus::TimedOut => "TIMED_OUT",
        TestStatus::Cancelled => "CANCELLED",
        TestStatus::Error => "ERROR",
    };
    out.push_str(&format!("Status: {status_str}\n"));

    out.push_str(&format!("Command: {}\n", report.argv.join(" ")));
    out.push_str(&format!("Cwd: {}\n", report.cwd.display()));

    let secs = report.duration_ms as f64 / 1000.0;
    out.push_str(&format!("Duration: {secs:.2}s\n"));

    if let Some(code) = report.exit_code {
        out.push_str(&format!("Exit code: {code}\n"));
    }

    if let Some(ref timeout) = report.timeout {
        let kind_str = match timeout.kind {
            TimeoutKind::WallClock => "wall_clock",
            TimeoutKind::NoOutput => "no_output",
            TimeoutKind::NoProgress => "no_progress",
        };
        out.push_str(&format!(
            "Timeout: {} ({:.1}s)\n",
            kind_str,
            timeout.elapsed_ms as f64 / 1000.0
        ));
        if let Some(ref last) = timeout.last_output {
            out.push_str(&format!("Last output: {last}\n"));
        }
    }

    if !report.failures.is_empty() {
        let shown = report.failures.len().min(MAX_DISPLAY_FAILURES);
        out.push_str(&format!("Failures ({shown}):\n"));
        for f in &report.failures[..shown] {
            let mut line = format!("  - [{}] ", f.failure_class);
            if let Some(ref name) = f.name {
                line.push_str(name);
                line.push_str(": ");
            }
            line.push_str(&f.message);
            out.push_str(&line);
            out.push('\n');
        }
        if report.failures.len() > MAX_DISPLAY_FAILURES {
            let remaining = report.failures.len() - MAX_DISPLAY_FAILURES;
            out.push_str(&format!("  ... and {remaining} more\n"));
        }
    }

    if let Some(ref path) = report.stdout_log {
        out.push_str(&format!("Stdout log: {}\n", path.display()));
    }
    if let Some(ref path) = report.stderr_log {
        out.push_str(&format!("Stderr log: {}\n", path.display()));
    }

    if report.output_truncated {
        out.push_str("Note: output was truncated\n");
    }

    out
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
            log_dir: None,
            stdout_log: None,
            stderr_log: None,
            output_truncated: false,
        }
    }

    #[test]
    fn format_report_includes_status_command_duration() {
        let report = base_report(TestStatus::Passed);
        let text = format_test_report(&report);
        assert!(text.contains("Status: PASSED"));
        assert!(text.contains("Command: cargo test"));
        assert!(text.contains("12.35s"));
    }

    #[test]
    fn format_report_limits_failure_count() {
        let mut report = base_report(TestStatus::Failed);
        report.failures = (0..8)
            .map(|i| TestFailure {
                name: Some(format!("test_{i}")),
                file: None,
                line: None,
                message: format!("fail {i}"),
                failure_class: "test_failed".into(),
            })
            .collect();
        let text = format_test_report(&report);
        assert!(text.contains("Failures (5):"));
        assert!(text.contains("... and 3 more"));
    }

    #[test]
    fn format_report_includes_timeout_class() {
        let mut report = base_report(TestStatus::TimedOut);
        report.timeout = Some(TestTimeout {
            kind: TimeoutKind::NoOutput,
            elapsed_ms: 30000,
            last_output: Some("waiting...".into()),
        });
        let text = format_test_report(&report);
        assert!(text.contains("Timeout: no_output (30.0s)"));
        assert!(text.contains("Last output: waiting..."));
    }

    #[test]
    fn format_report_includes_log_path() {
        let mut report = base_report(TestStatus::Failed);
        report.stdout_log = Some(PathBuf::from("/tmp/stdout.log"));
        report.stderr_log = Some(PathBuf::from("/tmp/stderr.log"));
        report.output_truncated = true;
        let text = format_test_report(&report);
        assert!(text.contains("Stdout log: /tmp/stdout.log"));
        assert!(text.contains("Stderr log: /tmp/stderr.log"));
        assert!(text.contains("Note: output was truncated"));
    }
}
