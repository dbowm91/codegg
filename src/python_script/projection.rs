use super::types::PythonRunResult;

/// Project a Python run result into model-facing text.
///
/// This is the Python-specific projector that formats run results,
/// risk assessments, and changed files into a compact summary.
pub fn project_python_run(result: &PythonRunResult) -> String {
    let mut lines = Vec::new();

    // Header
    lines.push(format!("## Python {} Run", result.mode));

    // Status line
    let status_str = match &result.status {
        super::types::PythonRunStatus::Success => "✅ Success".to_string(),
        super::types::PythonRunStatus::Failed(code) => format!("❌ Failed (exit {code})"),
        super::types::PythonRunStatus::TimedOut => "⏰ Timed Out".to_string(),
        super::types::PythonRunStatus::SpawnError => "💥 Spawn Error".to_string(),
    };
    lines.push(format!("**Status:** {status_str}"));
    lines.push(format!("**Duration:** {:.2?}", result.duration));
    lines.push(format!("**Interpreter:** {}", result.interpreter));

    // Risk assessment
    if !result.risk.reasons.is_empty() {
        lines.push(format!("**Risk:** {}", result.risk.reasons.join("; ")));
    }

    // Changed files
    if !result.changed_files.is_empty() {
        lines.push(format!(
            "**Changed files ({}):**",
            result.changed_files.len()
        ));
        for f in &result.changed_files {
            lines.push(format!("  - `{}`", f.display()));
        }
    }

    // Stdout (bounded)
    if !result.stdout.is_empty() {
        lines.push(String::new());
        lines.push("### stdout".to_string());
        let stdout = truncate_output(&result.stdout, 4000);
        lines.push(stdout);
    }

    // Stderr (bounded)
    if !result.stderr.is_empty() {
        lines.push(String::new());
        lines.push("### stderr".to_string());
        let stderr = truncate_output(&result.stderr, 2000);
        lines.push(stderr);
    }

    lines.join("\n")
}

/// Truncate output to max_chars, adding a notice if truncated.
fn truncate_output(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_chars).collect();
        format!(
            "{truncated}\n\n[truncated at {max_chars} chars, total {} chars]",
            text.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::*;
    use super::*;
    use std::time::Duration;

    fn make_result(status: PythonRunStatus, mode: PythonExecutionMode) -> PythonRunResult {
        PythonRunResult {
            status,
            stdout: "output".to_string(),
            stderr: String::new(),
            duration: Duration::from_millis(100),
            mode,
            script_length: 50,
            risk: PythonRiskAssessment::safe(),
            capabilities: PythonCapabilityEnvelope::analyze(),
            changed_files: vec![],
            interpreter: "python3".to_string(),
        }
    }

    #[test]
    fn projection_includes_mode() {
        let result = make_result(PythonRunStatus::Success, PythonExecutionMode::Analyze);
        let text = project_python_run(&result);
        assert!(text.contains("analyze"));
    }

    #[test]
    fn projection_includes_status() {
        let result = make_result(PythonRunStatus::Success, PythonExecutionMode::Analyze);
        let text = project_python_run(&result);
        assert!(text.contains("Success"));
    }

    #[test]
    fn projection_shows_failure() {
        let result = make_result(PythonRunStatus::Failed(1), PythonExecutionMode::Verify);
        let text = project_python_run(&result);
        assert!(text.contains("Failed"));
        assert!(text.contains("exit 1"));
    }

    #[test]
    fn projection_shows_changed_files() {
        let mut result = make_result(PythonRunStatus::Success, PythonExecutionMode::Transform);
        result.changed_files = vec![
            std::path::PathBuf::from("src/foo.rs"),
            std::path::PathBuf::from("src/bar.rs"),
        ];
        let text = project_python_run(&result);
        assert!(text.contains("Changed files (2)"));
        assert!(text.contains("foo.rs"));
    }

    #[test]
    fn projection_truncates_long_output() {
        let mut result = make_result(PythonRunStatus::Success, PythonExecutionMode::Analyze);
        result.stdout = "x".repeat(10000);
        let text = project_python_run(&result);
        assert!(text.contains("truncated"));
    }
}
