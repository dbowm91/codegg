use crate::error::ToolError;
use crate::tool::backend::{StructuredToolResult, ToolExecutionContext, ToolProvenance, ToolTrust};
use crate::tool::contract::{
    IdempotencyClass, ToolCachePolicy, ToolCallerPolicy, ToolContract, ToolEffectClass,
};
use crate::tool::util::check_path_for_symlinks;
use crate::tool::{Tool, ToolCategory};
use async_trait::async_trait;
use serde::Deserialize;
use similar::{ChangeTag, TextDiff};
use std::path::{Path, PathBuf};

const MAX_FILE_SIZE: usize = 10 * 1024 * 1024;

/// Maximum accepted `original` input in bytes. Matches the file-size bound so
/// neither side of the comparison can push unbounded data through the broker.
const MAX_ORIGINAL_BYTES: usize = 10 * 1024 * 1024;

/// Maximum unified-diff display in bytes before tool-level truncation. Keeps
/// program call results bounded; the broker artifact boundary applies above.
const MAX_DIFF_DISPLAY_BYTES: usize = 256 * 1024;

#[derive(Debug, Deserialize)]
struct DiffInput {
    path: String,
    #[serde(default)]
    original: Option<String>,
    #[serde(default)]
    line_range: Option<LineRange>,
}

#[derive(Debug, Deserialize)]
struct LineRange {
    start: Option<u32>,
    end: Option<u32>,
}

pub struct DiffTool {
    allowed_root: PathBuf,
}

impl DiffTool {
    pub fn new() -> Self {
        Self {
            allowed_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    pub fn with_allowed_root(mut self, root: PathBuf) -> Self {
        self.allowed_root = root;
        self
    }
}

impl Default for DiffTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolved outcome shared by the string (`execute`) and structured
/// (`execute_structured`) paths so direct and programmatic callers observe
/// identical diff semantics.
struct DiffOutcome {
    display: String,
    value: serde_json::Value,
    truncated: bool,
}

impl DiffTool {
    /// Effective workspace root for this call. When the broker supplies an
    /// execution context (always for broker-mediated direct and programmatic
    /// calls), its `cwd` — set from the workspace root by `BrokerAdapter` and
    /// the agent loop — is authoritative. The tool-default root is retained
    /// only for legacy direct callers that invoke `execute` without a context.
    fn effective_root(&self, ctx: Option<&ToolExecutionContext>) -> PathBuf {
        if let Some(context) = ctx {
            if context.cwd.is_dir() {
                return context.cwd.clone();
            }
        }
        self.allowed_root.clone()
    }

    fn contract_output_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "has_changes": {"type": "boolean"},
                "diff": {"type": "string"},
                "truncated": {"type": "boolean"},
                "original_bytes": {"type": "integer"},
                "current_bytes": {"type": "integer"}
            },
            "required": ["path", "has_changes", "diff", "truncated"]
        })
    }

    async fn execute_with_context(
        &self,
        input: serde_json::Value,
        ctx: Option<&ToolExecutionContext>,
    ) -> Result<DiffOutcome, ToolError> {
        let parsed: DiffInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid diff input: {e}")))?;

        let original = parsed.original.clone().ok_or_else(|| {
            ToolError::Execution(
                "original content required for diff tool. Use \"original\" parameter.".to_string(),
            )
        })?;
        if original.len() > MAX_ORIGINAL_BYTES {
            return Err(ToolError::Execution(format!(
                "original content exceeds maximum size of {} bytes",
                MAX_ORIGINAL_BYTES
            )));
        }

        let effective_root = self.effective_root(ctx);
        let path_str = parsed.path.clone();

        let current = tokio::task::spawn_blocking(move || {
            // Relative paths resolve against the execution-context workspace
            // root, never against whatever directory the daemon process was
            // launched from.
            let candidate = if Path::new(&path_str).is_absolute() {
                PathBuf::from(&path_str)
            } else {
                effective_root.join(&path_str)
            };
            check_path_for_symlinks(&candidate)?;
            let canonical = std::fs::canonicalize(&candidate).map_err(|_| {
                ToolError::Execution(format!("invalid path: {}", Path::new(&path_str).display()))
            })?;
            let root_canonical = std::fs::canonicalize(&effective_root)
                .map_err(|_| ToolError::Execution("invalid allowed root".to_string()))?;
            if !canonical.starts_with(&root_canonical) {
                return Err(ToolError::Permission(format!(
                    "path '{}' is outside allowed directory",
                    Path::new(&path_str).display()
                )));
            }

            if !candidate.exists() {
                return Err(ToolError::Execution(format!(
                    "file not found: {}",
                    Path::new(&path_str).display()
                )));
            }

            let metadata = std::fs::metadata(&candidate)
                .map_err(|e| ToolError::Execution(format!("failed to read file metadata: {e}")))?;
            if metadata.len() as usize > MAX_FILE_SIZE {
                return Err(ToolError::Execution(format!(
                    "file too large (max {} bytes): {}",
                    MAX_FILE_SIZE,
                    Path::new(&path_str).display()
                )));
            }

            std::fs::read_to_string(&candidate)
                .map_err(|e| ToolError::Execution(format!("failed to read file: {e}")))
        })
        .await
        .map_err(|e| ToolError::Execution(format!("join error: {e}")))??;

        let unified = generate_unified_diff(
            &original,
            &current,
            &parsed.path,
            parsed.line_range.as_ref(),
        );
        let has_changes = unified != "(no changes)";
        let (display, truncated) = truncate_diff_display(unified);
        let value = serde_json::json!({
            "path": parsed.path,
            "has_changes": has_changes,
            "diff": display,
            "truncated": truncated,
            "original_bytes": original.len(),
            "current_bytes": current.len(),
        });
        Ok(DiffOutcome {
            display: value["diff"].as_str().unwrap_or_default().to_string(),
            value,
            truncated,
        })
    }
}

#[async_trait]
impl Tool for DiffTool {
    fn name(&self) -> &str {
        "diff"
    }

    fn description(&self) -> &str {
        "Show differences between two versions of a file. Supports unified diff format."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to diff"
                },
                "original": {
                    "type": "string",
                    "description": "Original content (if comparing against a different version)"
                },
                "line_range": {
                    "type": "object",
                    "description": "Only show diff for a specific line range",
                    "properties": {
                        "start": {
                            "type": "number",
                            "description": "Start line (1-indexed)"
                        },
                        "end": {
                            "type": "number",
                            "description": "End line (1-indexed)"
                        }
                    }
                }
            },
            "required": ["path"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::DirectOrProgrammatic,
            effect_class: ToolEffectClass::ReadOnly,
            idempotency: IdempotencyClass::Idempotent,
            cache_policy: ToolCachePolicy {
                enabled: true,
                ttl_secs: 60,
                max_entries: 50,
            },
            output_schema: Some(Self::contract_output_schema()),
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        Ok(self.execute_with_context(input, None).await?.display)
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let outcome = self.execute_with_context(input, ctx.as_ref()).await?;
        let provenance = ToolProvenance {
            backend: "native".to_string(),
            implementation: "codegg/diff".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            elapsed_ms: None,
            truncated: outcome.truncated,
            trust: ToolTrust::LocalTrusted,
        };
        Ok(StructuredToolResult::with_value(
            outcome.display,
            outcome.value,
            true,
            Some(provenance),
        ))
    }
}

/// Bound the unified-diff display so program call results carry explicit
/// truncation metadata instead of unbounded text.
fn truncate_diff_display(diff: String) -> (String, bool) {
    if diff.len() <= MAX_DIFF_DISPLAY_BYTES {
        return (diff, false);
    }
    let marker = format!(
        "\n... [diff truncated at {} bytes; rerun with a narrower line_range to see more]\n",
        MAX_DIFF_DISPLAY_BYTES
    );
    let budget = MAX_DIFF_DISPLAY_BYTES.saturating_sub(marker.len());
    let mut end = budget.min(diff.len());
    while end > 0 && !diff.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = diff[..end].to_string();
    truncated.push_str(&marker);
    (truncated, true)
}

fn generate_unified_diff(
    old: &str,
    new: &str,
    path: &str,
    line_range: Option<&LineRange>,
) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut result = String::new();

    result.push_str(&format!("--- a/{}\n", path));
    result.push_str(&format!("+++ b/{}\n", path));

    let changes: Vec<_> = diff.iter_all_changes().collect();
    let total_changes = changes.len();

    let (start_idx, end_idx) = if let Some(range) = line_range {
        let start = (range.start.unwrap_or(1).saturating_sub(1)) as usize;
        let end = (range.end.unwrap_or(u32::MAX)) as usize;
        (start, end.min(start + 1000).min(total_changes))
    } else {
        (0, total_changes)
    };

    // A window that starts past the end of the change list (or is otherwise
    // empty) means there is nothing to show, not a subtraction underflow.
    if start_idx >= total_changes || end_idx <= start_idx {
        return String::from("(no changes)");
    }

    let mut old_line = 0;

    for change in changes.iter().skip(start_idx).take(end_idx - start_idx) {
        match change.tag() {
            ChangeTag::Delete => {
                old_line += 1;
                let _line_num = change.old_index().unwrap_or(old_line);
                result.push_str(&format!("-{}\n", change.value().trim_end_matches('\n')));
            }
            ChangeTag::Insert => {
                result.push_str(&format!("+{}\n", change.value().trim_end_matches('\n')));
            }
            ChangeTag::Equal => {
                old_line += 1;
                result.push_str(&format!(" {}\n", change.value().trim_end_matches('\n')));
            }
        }
    }

    let has_changes = result
        .lines()
        .skip(2)
        .any(|line| line.starts_with('+') || line.starts_with('-'));

    if !has_changes {
        return String::from("(no changes)");
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::backend::ToolBackendKind;

    #[test]
    fn test_diff_no_changes() {
        let old = "hello\nworld\n";
        let new = "hello\nworld\n";
        let result = generate_unified_diff(old, new, "test.txt", None);
        assert_eq!(result, "(no changes)");
    }

    #[test]
    fn test_diff_with_changes() {
        let old = "hello\nworld\n";
        let new = "hello\nrust\n";
        let result = generate_unified_diff(old, new, "test.txt", None);
        assert!(result.contains("-world"));
        assert!(result.contains("+rust"));
    }

    #[test]
    fn line_range_past_end_reports_no_changes_without_panic() {
        let old = "a\nb\n";
        let new = "a\nc\n";
        let range = LineRange {
            start: Some(10_000),
            end: Some(20_000),
        };
        let result = generate_unified_diff(old, new, "test.txt", Some(&range));
        assert_eq!(result, "(no changes)");
    }

    #[test]
    fn diff_display_truncation_is_bounded_and_flagged() {
        let big = "x".repeat(MAX_DIFF_DISPLAY_BYTES + 1024);
        let (display, truncated) = truncate_diff_display(big);
        assert!(truncated);
        assert!(display.contains("diff truncated"));
        assert!(display.len() <= MAX_DIFF_DISPLAY_BYTES + 512);
    }

    #[test]
    fn diff_contract_is_programmatic_read_only_with_schema() {
        let tool = DiffTool::new();
        let contract = tool.contract("diff", tool.parameters());
        assert_eq!(
            contract.caller_policy,
            ToolCallerPolicy::DirectOrProgrammatic
        );
        assert_eq!(contract.effect_class, ToolEffectClass::ReadOnly);
        assert_eq!(contract.idempotency, IdempotencyClass::Idempotent);
        assert!(contract.cache_policy.enabled);
        assert_eq!(contract.retry_policy.max_retries, 0);
        assert!(contract.validate().is_ok());
        let schema = contract.output_schema.expect("diff output schema");
        for key in ["path", "has_changes", "diff", "truncated"] {
            assert!(
                schema["properties"].get(key).is_some(),
                "output schema missing {key}"
            );
        }
    }

    #[test]
    fn effective_root_prefers_execution_context_cwd() {
        let workspace = tempfile::tempdir().unwrap();
        let tool = DiffTool::new();
        let mut ctx = ToolExecutionContext::with_backend(ToolBackendKind::Native);
        ctx.cwd = workspace.path().to_path_buf();
        assert_eq!(tool.effective_root(Some(&ctx)), workspace.path());
        // A non-directory cwd never overrides the tool root.
        ctx.cwd = workspace.path().join("missing-dir");
        assert_eq!(tool.effective_root(Some(&ctx)), tool.allowed_root);
        assert_eq!(tool.effective_root(None), tool.allowed_root);
    }
}
