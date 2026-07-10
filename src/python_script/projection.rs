use crate::shell::projection::{CommandOutputStore, CommandRunId};
use crate::shell::projector::{
    ArtifactSpanRef, CommandOutputProjector, ExpansionHandle, ProjectionBudget, ProjectionError,
    ProjectionExactness, ProjectionId, ProjectionKind, ProjectionRawSemantics, ProjectionRequest,
    ProjectionResult, ProjectionSupport, RtkResultMetadata, SpanRole,
};

use super::types::PythonRunResult;

const PYTHON_PROJECTOR_NAME: &str = "python";
const PYTHON_COMMAND_PREFIXES: &[&str] = &["python3", "python", "pip", "pip3", "conda"];

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

    // Enforcement evidence
    if !result.denied_capabilities.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "**Denied capabilities:** {}",
            result.denied_capabilities.join(", ")
        ));
    }
    if let Some(ref decision) = result.policy_decision {
        let sandbox_label = match decision.enforcement_backend {
            super::types::SandboxBackend::Landlock => "Landlock (OS-level)",
            super::types::SandboxBackend::PortableFallback => "Portable fallback",
            super::types::SandboxBackend::None => "None",
        };
        lines.push(format!("**Sandbox:** {sandbox_label}"));
        if !decision.warnings.is_empty() {
            lines.push(format!(
                "**Policy warnings:** {}",
                decision.warnings.join("; ")
            ));
        }
    }
    if !result.enforcement_warnings.is_empty() {
        lines.push(format!(
            "**Enforcement warnings:** {}",
            result.enforcement_warnings.join("; ")
        ));
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

pub struct PythonProjector;

impl PythonProjector {
    pub const NAME: &'static str = PYTHON_PROJECTOR_NAME;

    fn is_python_command(argv: &[String]) -> bool {
        argv.first().is_some_and(|cmd| {
            let base = cmd.rsplit('/').next().unwrap_or(cmd);
            PYTHON_COMMAND_PREFIXES
                .iter()
                .any(|prefix| base == *prefix || base.starts_with(&format!("{prefix}-")))
        })
    }
}

impl CommandOutputProjector for PythonProjector {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn supports(&self, request: &ProjectionRequest<'_>) -> ProjectionSupport {
        if request
            .run
            .argv
            .as_deref()
            .is_some_and(Self::is_python_command)
        {
            ProjectionSupport::Preferred
        } else {
            ProjectionSupport::Fallback
        }
    }

    fn project(
        &self,
        request: ProjectionRequest<'_>,
        store: &CommandOutputStore,
    ) -> Result<ProjectionResult, ProjectionError> {
        let run = request.run;
        let budget = request.budget.max_output_bytes;
        let mut text = String::new();
        let mut input_bytes: u64 = 0;
        let mut warnings = Vec::new();

        let mut expansion_handles = Vec::new();

        let omitted = Vec::new();

        if !run.is_failure() {
            text.push_str("## Python Output\n");
        } else {
            let code = match &run.exit {
                crate::shell::projection::CommandExit::Code(c) => format!(" (exit {c})"),
                _ => String::new(),
            };
            text.push_str(&format!("## Python Output — failed{code}\n"));
        }

        if let Some(handle) = run.stdout_handle() {
            input_bytes += run.stdout.retained_bytes;
            match store.get_stream(handle) {
                Some(bytes) => {
                    let stdout = String::from_utf8_lossy(bytes);
                    let truncated = truncate_output(&stdout, budget / 2);
                    if truncated != stdout.as_ref() {
                        warnings.push(format!(
                            "stdout truncated to {budget} bytes (total {} bytes)",
                            stdout.len()
                        ));
                    }
                    if !truncated.is_empty() {
                        text.push_str("\n### stdout\n");
                        text.push_str(&truncated);
                        if !truncated.ends_with('\n') {
                            text.push('\n');
                        }
                    }
                    expansion_handles.push(ExpansionHandle::full(run.id, handle.stream));
                }
                None => {
                    text.push_str("\n### stdout: <unavailable>\n");
                }
            }
        }

        if let Some(handle) = run.stderr_handle() {
            input_bytes += run.stderr.retained_bytes;
            match store.get_stream(handle) {
                Some(bytes) => {
                    let stderr = String::from_utf8_lossy(bytes);
                    let truncated = truncate_output(&stderr, budget / 4);
                    if truncated != stderr.as_ref() {
                        warnings.push(format!(
                            "stderr truncated to {} bytes (total {} bytes)",
                            budget / 4,
                            stderr.len()
                        ));
                    }
                    if !truncated.is_empty() {
                        text.push_str("\n### stderr\n");
                        text.push_str(&truncated);
                        if !truncated.ends_with('\n') {
                            text.push('\n');
                        }
                    }
                    expansion_handles.push(ExpansionHandle::full(run.id, handle.stream));
                }
                None => {
                    text.push_str("\n### stderr: <unavailable>\n");
                }
            }
        }

        let source_spans = extract_python_diagnostic_spans(&text, &run.id);
        let output_bytes = text.len();
        let estimated_tokens = ProjectionBudget::approx_tokens_from_bytes(output_bytes);

        Ok(ProjectionResult {
            projection_id: ProjectionId::new(),
            text,
            projector: Self::NAME.to_string(),
            kind: ProjectionKind::Structured,
            exactness: ProjectionExactness::Parsed,
            redaction: crate::shell::projection::RedactionState::NotApplied,
            omitted,
            expansion_handles,
            input_bytes,
            output_bytes,
            estimated_input_tokens: Some(estimated_tokens),
            estimated_output_tokens: Some(estimated_tokens),
            warnings,
            raw_semantics: ProjectionRawSemantics::OriginalCommandRaw,
            source_spans,
            redaction_records: Vec::new(),
            rtk_metadata: RtkResultMetadata::default(),
        })
    }
}

pub fn project_python_result(result: &PythonRunResult) -> ProjectionResult {
    let text = project_python_run(result);
    let output_bytes = text.len();
    let estimated_tokens = ProjectionBudget::approx_tokens_from_bytes(output_bytes);

    ProjectionResult {
        projection_id: ProjectionId::new(),
        text,
        projector: PYTHON_PROJECTOR_NAME.to_string(),
        kind: ProjectionKind::Structured,
        exactness: ProjectionExactness::Parsed,
        redaction: crate::shell::projection::RedactionState::NotApplied,
        omitted: Vec::new(),
        expansion_handles: Vec::new(),
        input_bytes: 0,
        output_bytes,
        estimated_input_tokens: Some(estimated_tokens),
        estimated_output_tokens: Some(estimated_tokens),
        warnings: Vec::new(),
        raw_semantics: ProjectionRawSemantics::OriginalCommandRaw,
        source_spans: Vec::new(),
        redaction_records: Vec::new(),
        rtk_metadata: RtkResultMetadata::default(),
    }
}

fn extract_python_diagnostic_spans(text: &str, run_id: &CommandRunId) -> Vec<ArtifactSpanRef> {
    let mut spans = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("Traceback (most recent call last):")
            || trimmed.starts_with("Error:")
            || trimmed.starts_with("ImportError:")
            || trimmed.starts_with("ModuleNotFoundError:")
            || trimmed.starts_with("ValueError:")
            || trimmed.starts_with("TypeError:")
            || trimmed.starts_with("RuntimeError:")
            || trimmed.starts_with("File \"")
        {
            let byte_start = text
                .lines()
                .take(line_idx)
                .map(|l| l.len() + 1)
                .sum::<usize>();
            let byte_end = byte_start + line.len();
            spans.push(ArtifactSpanRef {
                artifact_id: format!("cmd://{}/stderr", run_id.0),
                byte_start: byte_start as u64,
                byte_end: byte_end as u64,
                line_start: Some((line_idx + 1) as u64),
                line_end: Some((line_idx + 1) as u64),
                role: SpanRole::SupportingDiagnostic,
            });
        }
    }
    spans
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
            policy_decision: None,
            denied_capabilities: vec![],
            os_filesystem_isolation: false,
            os_network_isolation: false,
            effective_read_roots: vec![],
            effective_write_roots: vec![],
            allowed_subprocesses: vec![],
            enforcement_warnings: vec![],
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
    fn projection_shows_denied_capabilities() {
        let mut result = make_result(PythonRunStatus::Failed(2), PythonExecutionMode::Analyze);
        result.denied_capabilities = vec!["network".to_string(), "subprocess".to_string()];
        let text = project_python_run(&result);
        assert!(text.contains("Denied capabilities"));
        assert!(text.contains("network"));
        assert!(text.contains("subprocess"));
    }

    #[test]
    fn projection_shows_sandbox_landlock() {
        let mut result = make_result(PythonRunStatus::Success, PythonExecutionMode::Analyze);
        result.policy_decision = Some(super::super::types::PythonPolicyDecision {
            profile: super::super::types::PythonCapabilityProfile::analyze(
                &std::path::PathBuf::from("/tmp"),
            ),
            denied: vec![],
            warnings: vec![],
            enforcement_backend: super::super::types::SandboxBackend::Landlock,
            os_filesystem_isolation: true,
            os_network_isolation: false,
        });
        let text = project_python_run(&result);
        assert!(text.contains("Sandbox"));
        assert!(text.contains("Landlock"));
    }

    #[test]
    fn projection_shows_sandbox_portable_fallback() {
        let mut result = make_result(PythonRunStatus::Success, PythonExecutionMode::Transform);
        result.policy_decision = Some(super::super::types::PythonPolicyDecision {
            profile: super::super::types::PythonCapabilityProfile::transform(
                &std::path::PathBuf::from("/tmp"),
            ),
            denied: vec![],
            warnings: vec!["Landlock unavailable on this OS".to_string()],
            enforcement_backend: super::super::types::SandboxBackend::PortableFallback,
            os_filesystem_isolation: false,
            os_network_isolation: false,
        });
        let text = project_python_run(&result);
        assert!(text.contains("Portable fallback"));
        assert!(text.contains("Policy warnings"));
    }

    #[test]
    fn projection_shows_enforcement_warnings() {
        let mut result = make_result(PythonRunStatus::Success, PythonExecutionMode::Analyze);
        result.enforcement_warnings = vec!["cwd not inside workspace".to_string()];
        let text = project_python_run(&result);
        assert!(text.contains("Enforcement warnings"));
        assert!(text.contains("cwd not inside workspace"));
    }

    #[test]
    fn projection_no_enforcement_section_when_empty() {
        let result = make_result(PythonRunStatus::Success, PythonExecutionMode::Analyze);
        let text = project_python_run(&result);
        assert!(!text.contains("Denied capabilities"));
        assert!(!text.contains("Sandbox"));
        assert!(!text.contains("Enforcement warnings"));
    }

    #[test]
    fn projection_truncates_long_diff() {
        let mut result = make_result(PythonRunStatus::Success, PythonExecutionMode::Transform);
        result.diff = Some("x".repeat(10000));
        let text = project_python_run(&result);
        assert!(text.contains("truncated"));
    }
}
