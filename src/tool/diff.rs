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
            allowed_root: crate::tool::util::unresolved_workspace_root(),
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
                "current_bytes": {"type": "integer"},
                "newline_changed": {"type": "boolean"}
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
                "original content required for diff tool. Use \"original\" parameter; \
                 for pre-checks use text_replace_check, for working-tree state use \
                 git_query diff."
                    .to_string(),
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
        let newline_changed = trailing_newline_only_change(&original, &current);
        let (display, truncated) = truncate_diff_display(unified);
        let value = serde_json::json!({
            "path": parsed.path,
            "has_changes": has_changes,
            "diff": display,
            "truncated": truncated,
            "original_bytes": original.len(),
            "current_bytes": current.len(),
            "newline_changed": newline_changed,
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
        "For pre-checks use text_replace_check; for working-tree state use git_query diff. Show the unified diff between the given original text and the current file at \
         path. The original parameter is required: pass the full prior file contents; \
         the tool compares it against the file on disk and truncates long output. \
         Reserve diff for small files. \
         For large files prefer git_query diff (working tree against HEAD, no original needed) or text_replace_check. \
         See tool_search for the file-op chooser."
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
                    "description": "Required: full prior file contents to compare against the file at path"
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
            "required": ["path", "original"]
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

/// Repo-relative display path for diff `--- a/...` / `+++ b/...` headers.
///
/// Strips `[CWD]` / `[HOME]` placeholders and, when `workspace_root` is
/// known, strips that prefix so headers never contain an absolute path or a
/// placeholder. Falls back to stripping a leading `/` so the header stays
/// relative even when the root is unknown.
pub fn relative_diff_path(path: &str, workspace_root: Option<&Path>) -> String {
    let mut display = path.replace("[CWD]/", "").replace("[CWD]", "");
    display = display.replace("[HOME]/", "").replace("[HOME]", "");
    if let Some(root) = workspace_root {
        let root_str = root.to_string_lossy();
        let root_prefix = root_str.trim_end_matches('/');
        if display.starts_with(root_prefix)
            && display.len() > root_prefix.len()
            && display[root_prefix.len()..].starts_with('/')
        {
            display = display[root_prefix.len() + 1..].to_string();
        } else if let Ok(rel) = Path::new(&display).strip_prefix(root) {
            display = rel.to_string_lossy().to_string();
        }
    }
    while display.starts_with('/') {
        display.remove(0);
    }
    if display.is_empty() {
        display = "file".to_string();
    }
    display
}

/// Shared grouped-diff formatter for mutator previews.
///
/// Uses repo-relative headers, numbers every row from the new side
/// (`new_index` first), groups with `grouped_ops(3)`, emits `...` between
/// groups and when context omits the file head/tail, and reports
/// `(no changes)` when identical.
pub fn format_grouped_diff(
    old: &str,
    new: &str,
    path: &str,
    workspace_root: Option<&Path>,
) -> String {
    use similar::TextDiff;
    let display = relative_diff_path(path, workspace_root);
    let diff = TextDiff::from_lines(old, new);
    let mut result = String::new();
    result.push_str(&format!("--- a/{display}\n"));
    result.push_str(&format!("+++ b/{display}\n"));
    if old == new {
        result.push_str("(no changes)\n");
        return result;
    }
    let groups = diff.grouped_ops(3);
    if groups.is_empty() {
        result.push_str("(no changes)\n");
        return result;
    }
    // Head omission: first shown row does not start at line 1.
    if let Some(first_group) = groups.first() {
        if let Some(first_op) = first_group.first() {
            if let Some(change) = diff.iter_changes(first_op).next() {
                let first_idx = change.new_index().or(change.old_index()).unwrap_or(0);
                if first_idx > 0 {
                    result.push_str("...\n");
                }
            }
        }
    }
    for (idx, group) in groups.iter().enumerate() {
        if idx > 0 {
            result.push_str("...\n");
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    similar::ChangeTag::Delete => "-",
                    similar::ChangeTag::Insert => "+",
                    similar::ChangeTag::Equal => " ",
                };
                let line_num = change.new_index().or(change.old_index()).unwrap_or(0) + 1;
                result.push_str(&format!(
                    "{}{:>4} {}\n",
                    sign,
                    line_num,
                    change.value().trim_end_matches('\n')
                ));
            }
        }
    }
    // Tail omission (N8): last shown new-side line does not reach EOF.
    let new_total = new.lines().count();
    let mut last_new: Option<usize> = None;
    if let Some(last_group) = groups.last() {
        for op in last_group.iter().rev() {
            let mut found = None;
            for change in diff.iter_changes(op) {
                if change.new_index().is_some() {
                    found = change.new_index();
                }
            }
            if found.is_some() {
                last_new = found;
                break;
            }
        }
        // Fall back to scanning the whole tail when the last op is
        // delete-only (no new side).
        if last_new.is_none() {
            for group in groups.iter().rev() {
                for op in group.iter().rev() {
                    for change in diff.iter_changes(op).collect::<Vec<_>>().iter().rev() {
                        if let Some(n) = change.new_index() {
                            last_new = Some(n);
                            break;
                        }
                    }
                    if last_new.is_some() {
                        break;
                    }
                }
                if last_new.is_some() {
                    break;
                }
            }
        }
    }
    if new_total > 0 {
        if let Some(last) = last_new {
            if last + 1 < new_total {
                result.push_str("...\n");
            }
        }
    }
    if result.ends_with(&format!("+++ b/{display}\n")) {
        result.push_str("(no changes)\n");
    }
    result
}

/// Bound a grouped preview so a capped diff always carries its marker.
pub fn truncate_grouped_preview(preview: String, cap: usize) -> String {
    if preview.len() <= cap {
        return preview;
    }
    let mut end = cap;
    while end > 0 && !preview.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n... [preview truncated]", &preview[..end])
}

/// True when the only difference is the trailing newline.
///
/// `TextDiff::from_lines` compares line bodies and reports a trailing-newline
/// flip as "no changes", which renders as an identical `-`/`+` pair (or as
/// "(no changes)" while the bytes differ). The bytes differ but every line
/// body matches, so callers must surface the `\ No newline` marker instead
/// of claiming the files are identical.
pub fn trailing_newline_only_change(old: &str, new: &str) -> bool {
    if old == new {
        return false;
    }
    if old.lines().collect::<Vec<_>>() != new.lines().collect::<Vec<_>>() {
        return false;
    }
    old.ends_with('\n') != new.ends_with('\n')
}

fn generate_unified_diff(
    old: &str,
    new: &str,
    path: &str,
    line_range: Option<&LineRange>,
) -> String {
    // A trailing-newline-only change must never render as an identical
    // `-`/`+` pair (or as "(no changes)" while the bytes differ). Emit the
    // conventional marker up front, regardless of how `similar` tokenizes
    // the newline boundary.
    if trailing_newline_only_change(old, new) && line_range.is_none() {
        let mut marked = String::new();
        marked.push_str(&format!("--- a/{}\n", relative_diff_path(path, None)));
        marked.push_str(&format!("+++ b/{}\n", relative_diff_path(path, None)));
        let last = old.lines().last().unwrap_or_default();
        if !old.ends_with('\n') {
            marked.push_str(&format!("-{last}\n"));
            marked.push_str("\\ No newline at end of file\n");
            marked.push_str(&format!("+{last}\n"));
        } else {
            marked.push_str(&format!("-{last}\n"));
            marked.push_str(&format!("+{last}\n"));
            marked.push_str("\\ No newline at end of file\n");
        }
        return marked;
    }
    let diff = TextDiff::from_lines(old, new);
    let mut result = String::new();

    result.push_str(&format!("--- a/{}\n", relative_diff_path(path, None)));
    result.push_str(&format!("+++ b/{}\n", relative_diff_path(path, None)));

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
        if trailing_newline_only_change(old, new) && line_range.is_none() {
            let mut marked = String::new();
            marked.push_str(&format!("--- a/{}\n", relative_diff_path(path, None)));
            marked.push_str(&format!("+++ b/{}\n", relative_diff_path(path, None)));
            let last = old.lines().last().unwrap_or_default();
            if !old.ends_with('\n') {
                marked.push_str(&format!("-{last}\n"));
                marked.push_str("\\ No newline at end of file\n");
                marked.push_str(&format!("+{last}\n"));
            } else {
                marked.push_str(&format!("-{last}\n"));
                marked.push_str(&format!("+{last}\n"));
                marked.push_str("\\ No newline at end of file\n");
            }
            return marked;
        }
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

    #[test]
    fn diff_description_routes_prechecks_to_replace_check() {
        let tool = DiffTool::new();
        let description = tool.description();
        assert!(description.contains("text_replace_check"));
        assert!(description.contains("small files"));
        assert!(
            description.starts_with("For pre-checks use text_replace_check;"),
            "cross-link must lead the description, got: {description}"
        );
        assert!(
            description.contains("for working-tree state use git_query diff"),
            "must route working-tree state to git_query diff, got: {description}"
        );
    }

    #[test]
    fn grouped_diff_uses_relative_headers_and_new_side_numbering() {
        let preview = format_grouped_diff("A\nB\nC\n", "A\nINSERTED\nB\nC\n", "d.txt", None);
        assert!(preview.contains("--- a/d.txt"), "got: {preview}");
        assert!(preview.contains("+++ b/d.txt"), "got: {preview}");
        assert!(!preview.contains("[CWD]"), "got: {preview}");
        assert!(preview.contains("   2 INSERTED"), "got: {preview}");
        assert!(preview.contains("   3 B"), "got: {preview}");
    }

    #[test]
    fn relative_diff_path_strips_root_and_placeholders() {
        let root = Path::new("/repo");
        assert_eq!(relative_diff_path("/repo/a/b.txt", Some(root)), "a/b.txt");
        assert_eq!(relative_diff_path("[CWD]/tmp/a.txt", None), "tmp/a.txt");
        assert_eq!(relative_diff_path("/abs/a.txt", None), "abs/a.txt");
        assert!(!relative_diff_path("[CWD]/x", None).contains("[CWD]"));
    }

    #[test]
    fn grouped_diff_tail_omission_carries_marker() {
        let old = (1..=10)
            .map(|i| format!("l{i}"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let new = format!("NEW\n{old}");
        let preview = format_grouped_diff(&old, &new, "t.txt", None);
        assert!(
            preview.contains("..."),
            "tail cap must be marked, got: {preview}"
        );
        assert!(preview.contains("--- a/t.txt"), "got: {preview}");
    }

    #[test]
    fn trailing_newline_only_change_emits_no_newline_marker() {
        let old = "PROBE line 3 via patch";
        let new = "PROBE line 3 via patch\n";
        assert!(trailing_newline_only_change(old, new));
        assert!(trailing_newline_only_change(new, old));
        assert!(!trailing_newline_only_change(new, new));
        let result = generate_unified_diff(old, new, "probe.txt", None);
        assert_ne!(result, "(no changes)", "newline-only must not be silent");
        assert!(
            result.contains("\\ No newline at end of file"),
            "must carry the marker, got: {result}"
        );
        assert!(result.contains("-PROBE"), "got: {result}");
        assert!(result.contains("+PROBE"), "got: {result}");
        let reverse = generate_unified_diff(new, old, "probe.txt", None);
        assert!(
            reverse.contains("\\ No newline at end of file"),
            "reverse direction must also mark, got: {reverse}"
        );
    }

    #[test]
    fn identical_content_is_not_a_newline_change() {
        assert!(!trailing_newline_only_change("a\n", "a\n"));
        assert!(!trailing_newline_only_change("a\nb\n", "a\nc\n"));
        assert_eq!(
            generate_unified_diff("a\n", "a\n", "t.txt", None),
            "(no changes)"
        );
    }
}
