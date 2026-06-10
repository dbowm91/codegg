use crate::context::artifact::ArtifactKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionStatus {
    Success,
    Failure,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ToolOutputProjection {
    pub model_text: String,
    pub summary: String,
    pub status: ProjectionStatus,
    pub detected_kind: ArtifactKind,
    pub touched_files: Vec<String>,
    pub commands_run: Vec<String>,
    pub test_results: Vec<String>,
    pub unresolved_errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ProjectionConfig {
    pub max_success_tokens: usize,
    pub max_failure_tokens: usize,
    pub enabled: bool,
    /// When `true`, the artifact store is active and `ctx://` handles
    /// may be included in projected output.
    pub artifact_store_enabled: bool,
    /// When `true`, bypass projection and append full redacted output
    /// to the model transcript (useful for debugging).
    pub lossless_debug: bool,
}

impl Default for ProjectionConfig {
    fn default() -> Self {
        Self {
            max_success_tokens: 800,
            max_failure_tokens: 2000,
            enabled: true,
            artifact_store_enabled: true,
            lossless_debug: false,
        }
    }
}

pub fn project_tool_output(
    tool_name: &str,
    tool_call_args: Option<&str>,
    output: &str,
    success: bool,
    handle: &str,
    config: &ProjectionConfig,
) -> ToolOutputProjection {
    // lossless_debug: bypass projection, return full output
    if config.lossless_debug {
        return passthrough(output, success, handle, tool_name, tool_call_args);
    }

    if !config.enabled {
        return passthrough(output, success, handle, tool_name, tool_call_args);
    }

    let detected_kind = detect_kind(tool_name);
    let commands_run = extract_commands(tool_name, tool_call_args);
    let touched_files = extract_touched_files(output);
    let test_results = extract_test_results(output);
    let unresolved_errors = extract_errors(output);

    let status = if success {
        ProjectionStatus::Success
    } else {
        ProjectionStatus::Failure
    };

    let tool_label = tool_name;
    let recoverable = !handle.is_empty();
    let header = if recoverable {
        format!(
            "[tool output captured]\nTool: {tool_label}\nHandle: {handle}\nFull output: use context_read with this handle."
        )
    } else {
        format!("[tool output captured]\nTool: {tool_label}")
    };

    let (model_text, summary) = match status {
        ProjectionStatus::Success => {
            let token_est = crate::context::artifact::estimate_tokens(output);
            if token_est <= config.max_success_tokens {
                let text = format!("{header}\n{output}");
                (text.clone(), text)
            } else {
                let truncated = truncate_to_lines(output, 20);
                let stats = format!(
                    "({token_est} tokens, {} lines total)",
                    output.lines().count()
                );
                let text = format!("{header}\n{truncated}\n{stats}");
                let summary = format!("{header}: {stats}");
                (text, summary)
            }
        }
        ProjectionStatus::Failure => {
            let high_priority = collect_high_priority_lines(output);
            let medium_priority = collect_medium_priority_lines(output);

            let mut sections = Vec::new();
            sections.push(header.clone());

            if !high_priority.is_empty() {
                sections.push(high_priority.join("\n"));
            }
            if !medium_priority.is_empty() {
                sections.push(medium_priority.join("\n"));
            }

            let full_token_est = crate::context::artifact::estimate_tokens(&sections.join("\n"));
            if full_token_est <= config.max_failure_tokens {
                if !high_priority.is_empty() || !medium_priority.is_empty() {
                    let full_text = format!("{header}\n{output}");
                    if crate::context::artifact::estimate_tokens(&full_text)
                        <= config.max_failure_tokens
                        && sections.len() <= 2
                    {
                        (full_text, format!("{header}: output includes errors"))
                    } else {
                        (sections.join("\n"), format!("{header}: errors detected"))
                    }
                } else {
                    let text = format!("{header}\n{output}");
                    (text.clone(), text)
                }
            } else {
                (sections.join("\n"), format!("{header}: errors detected"))
            }
        }
        ProjectionStatus::Unknown => {
            let text = format!("{header}\n{output}");
            (text.clone(), text)
        }
    };

    let summary_line = if unresolved_errors.is_empty() {
        summary
    } else {
        format!("{} [{} error(s)]", summary, unresolved_errors.len())
    };

    ToolOutputProjection {
        model_text,
        summary: summary_line,
        status,
        detected_kind,
        touched_files,
        commands_run,
        test_results,
        unresolved_errors,
    }
}

fn passthrough(
    output: &str,
    success: bool,
    handle: &str,
    tool_name: &str,
    tool_call_args: Option<&str>,
) -> ToolOutputProjection {
    let status = if success {
        ProjectionStatus::Success
    } else {
        ProjectionStatus::Failure
    };
    let detected_kind = detect_kind(tool_name);
    let commands_run = extract_commands(tool_name, tool_call_args);
    let touched_files = extract_touched_files(output);
    let test_results = extract_test_results(output);
    let unresolved_errors = extract_errors(output);
    let recoverable = !handle.is_empty();
    let header = if recoverable {
        format!(
            "[tool output captured]\nTool: {tool_name}\nHandle: {handle}\nFull output: use context_read with this handle."
        )
    } else {
        format!("[tool output captured]\nTool: {tool_name}")
    };
    let text = format!("{header}\n{output}");
    ToolOutputProjection {
        model_text: text.clone(),
        summary: text,
        status,
        detected_kind,
        touched_files,
        commands_run,
        test_results,
        unresolved_errors,
    }
}

fn detect_kind(tool_name: &str) -> ArtifactKind {
    match tool_name {
        "bash" | "exec" => ArtifactKind::ToolResult,
        "read" | "read_file" | "glob" => ArtifactKind::ReadResult,
        "diff" | "git_diff" | "git_diff_staged" => ArtifactKind::Diff,
        "webfetch" | "web_search" | "websearch" => ArtifactKind::WebFetch,
        "image" | "image_gen" | "screenshot" => ArtifactKind::Image,
        _ => ArtifactKind::ToolResult,
    }
}

fn extract_commands(tool_name: &str, tool_call_args: Option<&str>) -> Vec<String> {
    if tool_name != "bash" && tool_name != "exec" {
        return Vec::new();
    }
    let Some(args_str) = tool_call_args else {
        return Vec::new();
    };
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(args_str) {
        if let Some(cmd) = val.get("command").and_then(|v| v.as_str()) {
            return vec![cmd.to_string()];
        }
        if let Some(cmd) = val.get("cmd").and_then(|v| v.as_str()) {
            return vec![cmd.to_string()];
        }
    }
    Vec::new()
}

fn extract_touched_files(output: &str) -> Vec<String> {
    let extensions = [
        ".rs", ".py", ".js", ".ts", ".tsx", ".jsx", ".go", ".java", ".rb", ".c", ".cpp", ".h",
        ".hpp", ".cs", ".swift", ".kt", ".scala", ".sh", ".bash", ".zsh", ".toml", ".yaml", ".yml",
        ".json", ".md", ".txt", ".html", ".css", ".scss", ".sql",
    ];
    let mut files = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        for ext in &extensions {
            if trimmed.contains(ext) {
                if let Some(path) = extract_path_with_extension(trimmed, ext) {
                    if !files.contains(&path) {
                        files.push(path);
                    }
                }
                break;
            }
        }
    }
    files
}

fn extract_path_with_extension(line: &str, ext: &str) -> Option<String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    for word in &words {
        let clean = word.trim_matches(|c: char| {
            c == '`' || c == '\'' || c == '"' || c == '(' || c == ')' || c == ',' || c == ';'
        });
        if clean.ends_with(ext) && clean.len() > ext.len() && !clean.starts_with('/') {
            if clean.contains('/') {
                return Some(clean.to_string());
            }
            return Some(clean.to_string());
        }
    }
    if let Some(pos) = line.find(ext) {
        let before = &line[..pos];
        if let Some(slash_pos) = before.rfind('/') {
            let start = before[..slash_pos]
                .rfind(|c: char| c.is_whitespace() || c == '\'' || c == '"' || c == '`')
                .map(|p| p + 1)
                .unwrap_or(0);
            let path = &line[start..pos + ext.len()];
            let clean = path.trim_matches(|c: char| c == '`' || c == '\'' || c == '"');
            if clean.len() > 1 {
                return Some(clean.to_string());
            }
        }
    }
    None
}

fn extract_test_results(output: &str) -> Vec<String> {
    let mut results = Vec::new();
    let test_patterns = [
        "test result:",
        "running ",
        "tests passed",
        "tests failed",
        "assertions:",
        "assertion failed",
        "FAILED ", // pytest-style
        "failures:",
    ];
    for line in output.lines() {
        let lower = line.to_lowercase();
        for &pattern in &test_patterns {
            if lower.contains(pattern) {
                let trimmed = line.trim().to_string();
                if !results.contains(&trimmed) {
                    results.push(trimmed);
                }
                break;
            }
        }
    }
    results
}

fn extract_errors(output: &str) -> Vec<String> {
    let mut errors = Vec::new();
    let error_patterns = [
        "error[",
        "error:",
        "error ",
        "failed",
        "failures:",
        "panicked at",
        "traceback",
        "assertionerror",
        "e   ",
        "fatal:",
        "critical:",
    ];
    for line in output.lines() {
        let lower = line.to_lowercase();
        for &pattern in &error_patterns {
            if lower.contains(pattern) {
                let trimmed = line.trim().to_string();
                if !errors.contains(&trimmed) {
                    errors.push(trimmed);
                }
                break;
            }
        }
    }
    errors
}

fn collect_high_priority_lines(output: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let patterns = [
        "error[",
        "error:",
        "failed",
        "failures:",
        "panicked at",
        "traceback",
        "assertionerror",
        "e   ",
        "fatal:",
        "critical:",
    ];
    for line in output.lines() {
        let lower = line.to_lowercase();
        for &pattern in &patterns {
            if lower.contains(pattern) {
                let trimmed = line.trim().to_string();
                if !lines.contains(&trimmed) {
                    lines.push(trimmed);
                }
                break;
            }
        }
        if lines.len() >= 30 {
            break;
        }
    }
    lines
}

fn collect_medium_priority_lines(output: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let patterns = [
        "warning:",
        "warn:",
        "warning ",
        "warn ",
        "summary",
        "test result:",
    ];
    for line in output.lines() {
        let lower = line.to_lowercase();
        for &pattern in &patterns {
            if lower.contains(pattern) {
                let trimmed = line.trim().to_string();
                if !lines.contains(&trimmed) {
                    lines.push(trimmed);
                }
                break;
            }
        }
        if lines.len() >= 20 {
            break;
        }
    }
    lines
}

fn truncate_to_lines(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        text.to_string()
    } else {
        let truncated: Vec<&str> = lines[..max_lines].to_vec();
        format!(
            "{}\n... ({} more lines)",
            truncated.join("\n"),
            lines.len() - max_lines
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ProjectionConfig {
        ProjectionConfig::default()
    }

    #[test]
    fn test_detect_kind_bash() {
        assert_eq!(detect_kind("bash"), ArtifactKind::ToolResult);
    }

    #[test]
    fn test_detect_kind_read() {
        assert_eq!(detect_kind("read"), ArtifactKind::ReadResult);
    }

    #[test]
    fn test_detect_kind_diff() {
        assert_eq!(detect_kind("diff"), ArtifactKind::Diff);
    }

    #[test]
    fn test_detect_kind_webfetch() {
        assert_eq!(detect_kind("webfetch"), ArtifactKind::WebFetch);
    }

    #[test]
    fn test_detect_kind_image() {
        assert_eq!(detect_kind("image"), ArtifactKind::Image);
    }

    #[test]
    fn test_detect_kind_unknown_falls_back_to_tool_result() {
        assert_eq!(detect_kind("edit"), ArtifactKind::ToolResult);
    }

    #[test]
    fn test_extract_commands_bash() {
        let args = r#"{"command": "ls -la"}"#;
        let cmds = extract_commands("bash", Some(args));
        assert_eq!(cmds, vec!["ls -la"]);
    }

    #[test]
    fn test_extract_commands_non_bash() {
        let cmds = extract_commands("read", Some(r#"{"path": "/foo"}"#));
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_extract_commands_no_args() {
        let cmds = extract_commands("bash", None);
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_extract_commands_invalid_json() {
        let cmds = extract_commands("bash", Some("not json"));
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_extract_commands_cmd_key() {
        let args = r#"{"cmd": "echo hello"}"#;
        let cmds = extract_commands("exec", Some(args));
        assert_eq!(cmds, vec!["echo hello"]);
    }

    #[test]
    fn test_extract_touched_files_rs() {
        let output = "Compiling my_project v0.1.0\nsrc/main.rs:10:5: warning unused variable";
        let files = extract_touched_files(output);
        assert!(files.iter().any(|f| f.contains("src/main.rs")));
    }

    #[test]
    fn test_extract_touched_files_py() {
        let output = "File \"app.py\", line 42";
        let files = extract_touched_files(output);
        assert!(files.iter().any(|f| f.contains("app.py")));
    }

    #[test]
    fn test_extract_touched_files_dedup() {
        let output = "src/lib.rs:10 error\nsrc/lib.rs:20 warning";
        let files = extract_touched_files(output);
        let lib_rs_count = files.iter().filter(|f| f.contains("lib.rs")).count();
        assert_eq!(lib_rs_count, 1);
    }

    #[test]
    fn test_extract_test_results() {
        let output = "test result: ok. 5 passed; 0 failed; 0 ignored";
        let results = extract_test_results(output);
        assert!(!results.is_empty());
        assert!(results[0].contains("test result"));
    }

    #[test]
    fn test_extract_test_results_failed() {
        let output = "test result: FAILED. 3 passed; 2 failed";
        let results = extract_test_results(output);
        assert!(results.iter().any(|r| r.contains("FAILED")));
    }

    #[test]
    fn test_extract_errors() {
        let output = "error[E0308]: mismatched types\n  --> src/main.rs:5:10";
        let errors = extract_errors(output);
        assert!(!errors.is_empty());
        assert!(errors[0].contains("error[E0308]"));
    }

    #[test]
    fn test_extract_errors_panicked() {
        let output = "panicked at 'index out of bounds', src/lib.rs:42:5";
        let errors = extract_errors(output);
        assert!(errors.iter().any(|e| e.contains("panicked")));
    }

    #[test]
    fn test_collect_high_priority_lines() {
        let output = "normal line\nerror: something failed\nanother line\nFAILED test_foo";
        let lines = collect_high_priority_lines(output);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_collect_medium_priority_lines() {
        let output = "warning: deprecated function\nclean output\ntest result: ok";
        let lines = collect_medium_priority_lines(output);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_truncate_to_lines_no_truncation() {
        let text = "line1\nline2\nline3";
        assert_eq!(truncate_to_lines(text, 5), text);
    }

    #[test]
    fn test_truncate_to_lines_with_truncation() {
        let text = "line1\nline2\nline3\nline4\nline5";
        let result = truncate_to_lines(text, 3);
        assert!(result.contains("2 more lines"));
        assert!(result.lines().count() <= 5);
    }

    #[test]
    fn test_project_tool_output_passthrough_disabled() {
        let config = ProjectionConfig {
            enabled: false,
            ..default_config()
        };
        let proj = project_tool_output("bash", None, "hello world", true, "ctx://t", &config);
        assert!(proj.model_text.contains("hello world"));
        assert_eq!(proj.status, ProjectionStatus::Success);
    }

    #[test]
    fn test_project_tool_output_success_short() {
        let config = default_config();
        let proj = project_tool_output(
            "bash",
            Some(r#"{"command":"ls"}"#),
            "file1.txt\nfile2.txt",
            true,
            "ctx://t",
            &config,
        );
        assert_eq!(proj.status, ProjectionStatus::Success);
        assert!(proj.model_text.contains("ctx://t"));
        assert!(proj.model_text.contains("file1.txt"));
        assert_eq!(proj.commands_run, vec!["ls"]);
    }

    #[test]
    fn test_project_tool_output_failure() {
        let config = default_config();
        let output =
            "error[E0308]: mismatched types\n  --> src/main.rs:5:10\nexpected `i32`, found `&str`";
        let proj = project_tool_output("bash", None, output, false, "ctx://t", &config);
        assert_eq!(proj.status, ProjectionStatus::Failure);
        assert!(!proj.unresolved_errors.is_empty());
    }

    #[test]
    fn test_project_tool_output_failure_high_priority_preserved() {
        let config = default_config();
        let output = "Compiling foo v0.1.0\nerror: cannot find value `x`\n  --> src/main.rs:10:5";
        let proj = project_tool_output("bash", None, output, false, "ctx://t", &config);
        assert!(proj.model_text.contains("error: cannot find value"));
    }

    #[test]
    fn test_project_tool_output_read_tool() {
        let config = default_config();
        let proj = project_tool_output("read", None, "fn main() {}", true, "ctx://t", &config);
        assert_eq!(proj.detected_kind, ArtifactKind::ReadResult);
    }

    #[test]
    fn test_project_tool_output_diff_tool() {
        let config = default_config();
        let proj = project_tool_output(
            "diff",
            None,
            "+added line\n-removed line",
            true,
            "ctx://t",
            &config,
        );
        assert_eq!(proj.detected_kind, ArtifactKind::Diff);
    }

    #[test]
    fn test_project_tool_output_webfetch_tool() {
        let config = default_config();
        let proj = project_tool_output(
            "webfetch",
            None,
            "<html>hello</html>",
            true,
            "ctx://t",
            &config,
        );
        assert_eq!(proj.detected_kind, ArtifactKind::WebFetch);
    }

    #[test]
    fn test_project_tool_output_image_tool() {
        let config = default_config();
        let proj = project_tool_output("image", None, "base64...", true, "ctx://t", &config);
        assert_eq!(proj.detected_kind, ArtifactKind::Image);
    }

    #[test]
    fn test_project_tool_output_test_results_extracted() {
        let config = default_config();
        let output = "running 5 tests\ntest result: ok. 5 passed; 0 failed";
        let proj = project_tool_output("bash", None, output, true, "ctx://t", &config);
        assert!(!proj.test_results.is_empty());
    }

    #[test]
    fn test_project_tool_output_commands_extracted() {
        let config = default_config();
        let args = r#"{"command": "cargo build"}"#;
        let proj =
            project_tool_output("bash", Some(args), "Compiling...", true, "ctx://t", &config);
        assert_eq!(proj.commands_run, vec!["cargo build"]);
    }

    #[test]
    fn test_projection_config_defaults() {
        let config = ProjectionConfig::default();
        assert_eq!(config.max_success_tokens, 800);
        assert_eq!(config.max_failure_tokens, 2000);
        assert!(config.enabled);
    }

    #[test]
    fn test_project_tool_output_success_verbose_truncates() {
        let config = ProjectionConfig {
            max_success_tokens: 10,
            ..default_config()
        };
        let output: String = (0..50)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let proj = project_tool_output("bash", None, &output, true, "ctx://t", &config);
        assert!(proj.model_text.contains("more lines"));
    }

    #[test]
    fn test_project_tool_output_summary_includes_error_count() {
        let config = default_config();
        let output = "error: fail1\nerror: fail2\nerror: fail3";
        let proj = project_tool_output("bash", None, output, false, "ctx://t", &config);
        assert!(proj.summary.contains("3 error(s)"));
    }

    #[test]
    fn test_project_tool_output_no_tool_call_args() {
        let config = default_config();
        let proj = project_tool_output("bash", None, "output", true, "ctx://t", &config);
        assert!(proj.commands_run.is_empty());
    }

    #[test]
    fn test_project_tool_output_exec_tool() {
        let config = default_config();
        let args = r#"{"cmd": "echo hi"}"#;
        let proj = project_tool_output("exec", Some(args), "hi", true, "ctx://t", &config);
        assert_eq!(proj.commands_run, vec!["echo hi"]);
        assert_eq!(proj.detected_kind, ArtifactKind::ToolResult);
    }

    #[test]
    fn test_project_tool_output_empty_output() {
        let config = default_config();
        let proj = project_tool_output("bash", None, "", true, "ctx://t", &config);
        assert_eq!(proj.status, ProjectionStatus::Success);
        assert!(proj.touched_files.is_empty());
    }

    #[test]
    fn test_project_tool_output_failure_long_output() {
        let config = ProjectionConfig {
            max_failure_tokens: 20,
            ..default_config()
        };
        let output: String = (0..100)
            .map(|i| format!("error: issue at step {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let proj = project_tool_output("bash", None, &output, false, "ctx://t", &config);
        assert_eq!(proj.status, ProjectionStatus::Failure);
        assert!(!proj.unresolved_errors.is_empty());
    }

    #[test]
    fn test_project_tool_output_touched_files_from_error_output() {
        let config = default_config();
        let output = "error in src/lib.rs:10:5\nalso src/main.rs:3:1";
        let proj = project_tool_output("bash", None, output, false, "ctx://t", &config);
        assert!(proj.touched_files.iter().any(|f| f.contains("lib.rs")));
    }

    #[test]
    fn test_project_tool_output_python_traceback() {
        let config = default_config();
        let output = "Traceback (most recent call last):\n  File \"app.py\", line 10\n    foo()\nAssertionError: x != y";
        let proj = project_tool_output("python", None, output, false, "ctx://t", &config);
        assert!(!proj.unresolved_errors.is_empty());
    }
}
