use super::types::PythonRunResult;

/// Threshold (in chars) for stdout/stderr/diff beyond which RTK eligibility is noted.
const RTK_ELIGIBLE_THRESHOLD: usize = 2000;

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

    // Script body hash for reproducibility
    if let Some(ref hash) = result.script_body_hash {
        lines.push(format!("**Script hash:** `{hash}`"));
    }

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

    // Diff (Transform mode)
    if let Some(ref diff) = result.diff {
        lines.push(String::new());
        lines.push("### diff".to_string());
        let diff_text = truncate_output(diff, 4000);
        lines.push(diff_text);
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

    // RTK eligibility note
    let rtk_eligible = result.stdout.len() > RTK_ELIGIBLE_THRESHOLD
        || result.stderr.len() > RTK_ELIGIBLE_THRESHOLD
        || result
            .diff
            .as_ref()
            .is_some_and(|d| d.len() > RTK_ELIGIBLE_THRESHOLD);
    if rtk_eligible {
        lines.push(String::new());
        lines.push("[RTK eligible: output exceeds threshold]".to_string());
    }

    // Artifact handle availability note
    if result.stdout_label.is_some() || result.stderr_label.is_some() {
        lines.push(String::new());
        lines.push("[Run labels: not expandable artifacts]".to_string());
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
            diff: None,
            script_body_hash: None,
            stdout_label: None,
            stderr_label: None,
            diff_label: None,
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

    #[test]
    fn projection_includes_script_hash() {
        let mut result = make_result(PythonRunStatus::Success, PythonExecutionMode::Analyze);
        result.script_body_hash = Some("abc123".to_string());
        let text = project_python_run(&result);
        assert!(text.contains("Script hash"));
        assert!(text.contains("abc123"));
    }

    #[test]
    fn projection_includes_diff() {
        let mut result = make_result(PythonRunStatus::Success, PythonExecutionMode::Transform);
        result.diff = Some("--- a/foo.txt\n+++ b/foo.txt\n-old\n+new".to_string());
        let text = project_python_run(&result);
        assert!(text.contains("### diff"));
        assert!(text.contains("--- a/foo.txt"));
    }

    #[test]
    fn projection_shows_rtk_eligible_for_large_stdout() {
        let mut result = make_result(PythonRunStatus::Success, PythonExecutionMode::Analyze);
        result.stdout = "x".repeat(3000);
        result.stdout_label = Some("python_run://1/stdout".to_string());
        let text = project_python_run(&result);
        assert!(text.contains("RTK eligible"));
    }

    #[test]
    fn projection_shows_rtk_eligible_for_large_diff() {
        let mut result = make_result(PythonRunStatus::Success, PythonExecutionMode::Transform);
        result.diff = Some("x".repeat(3000));
        result.diff_label = Some("python_run://1/diff".to_string());
        let text = project_python_run(&result);
        assert!(text.contains("RTK eligible"));
    }

    #[test]
    fn projection_no_rtk_note_for_small_output() {
        let result = make_result(PythonRunStatus::Success, PythonExecutionMode::Analyze);
        let text = project_python_run(&result);
        assert!(!text.contains("RTK eligible"));
    }

    #[test]
    fn projection_shows_artifact_handles() {
        let mut result = make_result(PythonRunStatus::Success, PythonExecutionMode::Transform);
        result.stdout_label = Some("python_run://1/stdout".to_string());
        result.stderr_label = Some("python_run://1/stderr".to_string());
        let text = project_python_run(&result);
        assert!(text.contains("Run labels"));
    }

    #[test]
    fn projection_no_artifact_handles_when_none() {
        let result = make_result(PythonRunStatus::Success, PythonExecutionMode::Analyze);
        let text = project_python_run(&result);
        assert!(!text.contains("Run labels"));
    }

    #[test]
    fn projection_no_script_hash_when_none() {
        let result = make_result(PythonRunStatus::Success, PythonExecutionMode::Analyze);
        let text = project_python_run(&result);
        assert!(!text.contains("Script hash"));
    }

    #[test]
    fn projection_no_diff_when_none() {
        let result = make_result(PythonRunStatus::Success, PythonExecutionMode::Transform);
        let text = project_python_run(&result);
        assert!(!text.contains("### diff"));
    }

    #[test]
    fn projection_truncates_long_diff() {
        let mut result = make_result(PythonRunStatus::Success, PythonExecutionMode::Transform);
        result.diff = Some("x".repeat(10000));
        let text = project_python_run(&result);
        assert!(text.contains("truncated"));
    }
}
