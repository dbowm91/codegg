use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use strsim::levenshtein;

use crate::bus::events::AppEvent;
use crate::bus::global::GlobalEventBus;
use crate::error::ToolError;
use crate::preflight::{PreflightDecision, PreflightService};
use crate::tool::backend::ToolExecutionContext;
use crate::tool::util::{canonicalize_path, validate_path};
use crate::tool::writer_fence::{
    acquire_writer_permit, settle_writer_fence, writer_fence_key, NativeEffectClass,
    WriterFenceOutcome,
};
use crate::tool::{Tool, ToolCategory};

const MAX_INPUT_SIZE: usize = 100_000;

pub struct EditTool {
    allowed_root: PathBuf,
    unrestricted: bool,
    preflight: Option<Arc<PreflightService>>,
}

impl EditTool {
    pub fn new() -> Self {
        Self {
            allowed_root: crate::tool::util::unresolved_workspace_root(),
            unrestricted: false,
            preflight: None,
        }
    }

    pub fn with_allowed_root(mut self, root: PathBuf) -> Self {
        self.allowed_root = root;
        self.unrestricted = false;
        self
    }

    pub fn with_preflight(mut self, service: PreflightService) -> Self {
        self.preflight = Some(Arc::new(service));
        self
    }

    pub fn set_allowed_root(&mut self, root: PathBuf) {
        self.allowed_root = root;
        self.unrestricted = false;
    }
}

impl Default for EditTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn causal_contract(&self) -> Option<crate::tool_advisor::causal_frontier::ToolCausalContract> {
        crate::tool_advisor::causal_frontier::native_causal_contract("edit")
    }

    fn description(&self) -> &str {
        "Replace the first occurrence of literal text in a file (replaces first occurrence only). See tool_search for the file-op chooser."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "old_string": {
                    "type": "string",
                    "description": "Text to search for"
                },
                "new_string": {
                    "type": "string",
                    "description": "Text to replace with"
                }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        self.execute_inner(input, None).await
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<crate::tool::StructuredToolResult, ToolError> {
        let output = self.execute_inner(input, ctx.as_ref()).await?;
        Ok(crate::tool::StructuredToolResult::legacy(
            self.name(),
            output,
        ))
    }
}

impl EditTool {
    /// The edit effect, with the checkout writer fence held across it.
    ///
    /// The fence is an *additional* refusal on top of the existing path
    /// boundary: it never widens what this tool may write. A context without
    /// a fence mints no permit, so the legacy path behaves exactly as
    /// before.
    async fn execute_inner(
        &self,
        input: serde_json::Value,
        ctx: Option<&ToolExecutionContext>,
    ) -> Result<String, ToolError> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'path' parameter".to_string()))?
            .to_string();

        let old_string = input["old_string"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'old_string' parameter".to_string()))?
            .to_string();

        if old_string.len() > MAX_INPUT_SIZE {
            return Err(ToolError::Execution(format!(
                "old_string exceeds maximum size of {} bytes",
                MAX_INPUT_SIZE
            )));
        }

        let new_string = input["new_string"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'new_string' parameter".to_string()))?
            .to_string();

        if new_string.len() > MAX_INPUT_SIZE {
            return Err(ToolError::Execution(format!(
                "new_string exceeds maximum size of {} bytes",
                MAX_INPUT_SIZE
            )));
        }

        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;
        let path_for_read = path_str.clone();

        // The read resolves the target and reads it in one step. The resolved
        // path is the one the fence names, and the write below re-resolves
        // the same request through the same resolution — so the fenced path
        // is the path the write lands on.
        let (canonical_path, content) = tokio::task::spawn_blocking(move || {
            let path = Path::new(&path_for_read);
            let canonical = if unrestricted {
                canonicalize_path(path)?
            } else {
                validate_path(path, &allowed_root)?
            };
            let content = std::fs::read_to_string(&canonical).map_err(|e| {
                ToolError::Execution(format!("read failed for '{}': {}", canonical.display(), e))
            })?;
            Ok::<_, ToolError>((canonical, content))
        })
        .await
        .map_err(|e| ToolError::Execution(format!("join error: {}", e)))??;

        let mut preflight_warnings = Vec::new();
        if let Some(ref svc) = self.preflight {
            // Text replacement check
            match svc
                .check_text_replace(&content, &old_string, &new_string)
                .await
            {
                PreflightDecision::Block { findings } => {
                    return Err(ToolError::Execution(format!(
                        "preflight blocked edit: {}",
                        PreflightDecision::Block { findings }.summary()
                    )));
                }
                w @ PreflightDecision::Warn { .. } => preflight_warnings.push(w.summary()),
                _ => {}
            }
            // Unicode safety check on new text
            match svc.check_text_security(&new_string).await {
                PreflightDecision::Block { findings } => {
                    return Err(ToolError::Execution(format!(
                        "preflight blocked edit: {}",
                        PreflightDecision::Block { findings }.summary()
                    )));
                }
                w @ PreflightDecision::Warn { .. } => preflight_warnings.push(w.summary()),
                _ => {}
            }
        }

        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;

        let path_str_for_closure = path_str.clone();
        let old_string_for_closure = old_string.clone();
        let new_string_for_closure = new_string.clone();

        // The permit is taken after the path boundary and the preflight
        // checks approved the edit, and immediately before the write; it is
        // held until the write has settled.
        let targets = vec![canonical_path.clone()];
        let permit = acquire_writer_permit(
            ctx,
            NativeEffectClass::FileWrite,
            targets.clone(),
            writer_fence_key(self.name(), &targets, ctx),
        )
        .await?;

        let effect = match tokio::task::spawn_blocking(move || {
            let path = Path::new(&path_str_for_closure);

            let canonical = if unrestricted {
                canonicalize_path(path)
            } else {
                validate_path(path, &allowed_root)
            }
            .map_err(|error| (error, WriterFenceOutcome::NoEffect))?;

            let outcome = try_edit(&content, &old_string_for_closure, &new_string_for_closure)
                .ok_or_else(|| {
                    let hint = find_similar_block(&content, &old_string_for_closure);
                    (
                        ToolError::Execution(format!(
                            "could not find exact match for old_string.{}",
                            hint
                        )),
                        WriterFenceOutcome::NoEffect,
                    )
                })?;

            // `fs::write` truncates before it writes, so a failure here can
            // leave a partially written file behind: the effect is unproven,
            // not absent.
            std::fs::write(&canonical, &outcome.result).map_err(|e| {
                let message = format!("write failed for '{}': {}", canonical.display(), e);
                (
                    ToolError::Execution(message.clone()),
                    WriterFenceOutcome::Unproven(message),
                )
            })?;

            Ok::<_, (ToolError, WriterFenceOutcome)>((
                canonical.display().to_string(),
                content,
                outcome.result,
                outcome.strategy,
                outcome.start_line,
                outcome.end_line,
            ))
        })
        .await
        {
            Ok(Ok(written)) => Ok(written),
            Ok(Err(failure)) => Err(failure),
            Err(join_error) => Err((
                ToolError::Execution(format!("join error: {}", join_error)),
                WriterFenceOutcome::Unproven(format!("join error: {}", join_error)),
            )),
        };

        let (path_display, old_content, new_content, strategy, start_line, end_line) = match effect
        {
            Ok(written) => {
                // The edit landed and was observed, so the permit is
                // released. A failure to release it is reported: the caller
                // has to know the checkout was not handed on.
                settle_writer_fence(permit, WriterFenceOutcome::Settled, false).await?;
                written
            }
            Err((error, outcome)) => {
                // Settle before the error propagates: a failed effect must
                // still release (or park) the permit it held. The tool's own
                // error is the one the caller sees.
                settle_writer_fence(permit, outcome, true).await?;
                return Err(error);
            }
        };

        GlobalEventBus::publish(AppEvent::FileChanged {
            path: path_str.clone(),
            action: "Modified".to_string(),
            old_content: Some(old_content.clone()),
        });

        // Config format validation after write
        if let Some(ref svc) = self.preflight {
            if is_config_file(&path_str) {
                let ext = std::path::Path::new(&path_str)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                let config_decision = match ext {
                    "json" | "jsonc" | "json5" => svc.check_json_valid(&new_content).await,
                    "toml" => svc.check_toml_valid(&new_content).await,
                    _ => svc.check_config(&new_content).await,
                };
                match config_decision {
                    PreflightDecision::Block { findings } => {
                        return Err(ToolError::Execution(format!(
                            "preflight blocked config write: {}",
                            PreflightDecision::Block { findings }.summary()
                        )));
                    }
                    w @ PreflightDecision::Warn { .. } => preflight_warnings.push(w.summary()),
                    _ => {}
                }
            }
        }

        let preview = grouped_preview(&old_content, &new_content, &path_str);
        let preview = truncate_preview(preview, 4000);
        let mut output = format!(
            "Edited {} via {} at lines {}-{} ({} -> {} lines)\n\n{}",
            path_display,
            strategy,
            start_line,
            end_line,
            old_string.lines().count(),
            new_string.lines().count(),
            preview
        );
        if !preflight_warnings.is_empty() {
            output = format!("{}\n\n{}", output, preflight_warnings.join("\n"));
        }
        Ok(output)
    }
}

struct EditOutcome {
    result: String,
    strategy: &'static str,
    start_line: usize,
    end_line: usize,
}

type EditStrategy = (&'static str, fn(&str, &str, &str) -> Option<EditOutcome>);

fn try_edit(content: &str, old_string: &str, new_string: &str) -> Option<EditOutcome> {
    let strategies: &[EditStrategy] = &[
        ("exact_match", exact_match),
        ("line_trimmed_match", line_trimmed_match),
        ("whitespace_normalized_match", whitespace_normalized_match),
        ("block_anchored_match", block_anchored_match),
        ("indentation_flexible_match", indentation_flexible_match),
        ("escape_normalized_match", escape_normalized_match),
        ("trimmed_boundary_match", trimmed_boundary_match),
        ("context_aware_match", context_aware_match),
    ];
    for (name, strategy) in strategies {
        if let Some(mut outcome) = strategy(content, old_string, new_string) {
            outcome.strategy = name;
            return Some(outcome);
        }
    }
    None
}

fn min_indent(lines: &[&str]) -> usize {
    lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0)
}
fn reindent_to_base(new_string: &str, base_indent: usize) -> String {
    let lines: Vec<&str> = new_string.lines().collect();
    let min_new = min_indent(&lines);
    let prefix = " ".repeat(base_indent);
    lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                (*l).to_string()
            } else {
                let stripped = if l.len() >= min_new {
                    &l[min_new..]
                } else {
                    l.trim_start()
                };
                format!("{prefix}{stripped}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn splice_lines(content_lines: &[&str], start: usize, len: usize, new_text: &str) -> String {
    let before = content_lines[..start].join("\n");
    let after = content_lines[start + len..].join("\n");
    let mut result = if before.is_empty() {
        String::new()
    } else {
        format!("{before}\n")
    };
    result.push_str(new_text);
    if !after.is_empty() {
        result.push_str(&format!("\n{after}"));
    }
    result
}
fn line_range_for_byte_idx(
    content: &str,
    byte_idx: usize,
    match_len_bytes: usize,
) -> (usize, usize) {
    let start_line = content[..byte_idx.min(content.len())]
        .chars()
        .filter(|&c| c == '\n')
        .count()
        + 1;
    let end_line = content[..(byte_idx + match_len_bytes).min(content.len())]
        .chars()
        .filter(|&c| c == '\n')
        .count()
        + 1;
    (start_line, end_line.max(start_line))
}
fn grouped_preview(old: &str, new: &str, path: &str) -> String {
    use similar::TextDiff;
    let diff = TextDiff::from_lines(old, new);
    let mut result = String::new();
    result.push_str(&format!("--- a/{path}\n"));
    result.push_str(&format!("+++ b/{path}\n"));
    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
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
                let line_num = change.old_index().or(change.new_index()).unwrap_or(0) + 1;
                result.push_str(&format!(
                    "{}{:>4} {}\n",
                    sign,
                    line_num,
                    change.value().trim_end_matches('\n')
                ));
            }
        }
    }
    if result.ends_with(&format!("+++ b/{path}\n")) {
        result.push_str("(no changes)\n");
    }
    result
}
fn truncate_preview(preview: String, cap: usize) -> String {
    if preview.len() <= cap {
        return preview;
    }
    let mut end = cap;
    while end > 0 && !preview.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n... [preview truncated]", &preview[..end])
}
fn exact_match(content: &str, old_string: &str, new_string: &str) -> Option<EditOutcome> {
    let idx = content.find(old_string)?;
    let (start_line, end_line) = line_range_for_byte_idx(content, idx, old_string.len());
    Some(EditOutcome {
        result: content.replacen(old_string, new_string, 1),
        strategy: "exact_match",
        start_line,
        end_line,
    })
}

fn line_trimmed_match(content: &str, old_string: &str, new_string: &str) -> Option<EditOutcome> {
    let old_trimmed: Vec<&str> = old_string.lines().map(|l| l.trim()).collect();
    let content_lines: Vec<&str> = content.lines().collect();
    for window_start in 0..=content_lines.len().saturating_sub(old_trimmed.len()) {
        let window = &content_lines[window_start..window_start + old_trimmed.len()];
        if window.iter().zip(&old_trimmed).all(|(a, b)| a.trim() == *b) {
            let base = min_indent(window);
            let reindented = reindent_to_base(new_string, base);
            let result = splice_lines(&content_lines, window_start, old_trimmed.len(), &reindented);
            return Some(EditOutcome {
                result,
                strategy: "line_trimmed_match",
                start_line: window_start + 1,
                end_line: window_start + old_trimmed.len(),
            });
        }
    }
    None
}

fn whitespace_normalized_match(
    content: &str,
    old_string: &str,
    new_string: &str,
) -> Option<EditOutcome> {
    let normalize = |s: &str| -> String {
        s.lines()
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let old_normalized = normalize(old_string);
    let content_normalized = normalize(content);

    if content_normalized.contains(&old_normalized) {
        let idx = content_normalized.find(&old_normalized)?;
        let match_chars = old_normalized.chars().count();

        let content_chars: Vec<char> = content.chars().collect();
        let mut norm_idx = 0;
        let mut start_char = 0;
        let mut end_char = 0;

        for (i, &c) in content_chars.iter().enumerate() {
            if !c.is_whitespace() || (i > 0 && !content_chars[i - 1].is_whitespace()) {
                if norm_idx == idx {
                    start_char = i;
                }
                if norm_idx == idx + match_chars {
                    end_char = i;
                    break;
                }
                norm_idx += 1;
            }
        }

        if end_char == 0 {
            end_char = content_chars.len();
        }

        let mut result = content_chars[..start_char].iter().collect::<String>();
        result.push_str(new_string);
        result.push_str(&content_chars[end_char..].iter().collect::<String>());
        let start_line = content_chars[..start_char]
            .iter()
            .filter(|&&c| c == '\n')
            .count()
            + 1;
        let end_line = start_line + old_string.lines().count().saturating_sub(1);
        Some(EditOutcome {
            result,
            strategy: "whitespace_normalized_match",
            start_line,
            end_line,
        })
    } else {
        None
    }
}

fn block_anchored_match(content: &str, old_string: &str, new_string: &str) -> Option<EditOutcome> {
    let old_lines: Vec<&str> = old_string.lines().collect();
    if old_lines.len() < 3 {
        return None;
    }

    let anchor_start = old_lines[0].trim();
    let anchor_end = old_lines.last()?.trim();
    let middle_lines: Vec<&str> = old_lines[1..old_lines.len() - 1]
        .iter()
        .map(|l| l.trim())
        .collect();

    let content_lines: Vec<&str> = content.lines().collect();

    for i in 0..content_lines.len() {
        if content_lines[i].trim() != anchor_start {
            continue;
        }

        for j in (i + 2)..content_lines.len() {
            if content_lines[j].trim() != anchor_end {
                continue;
            }

            let block_len = j - i + 1;
            if block_len < old_lines.len() {
                continue;
            }

            let middle_ok = middle_lines
                .iter()
                .zip(&content_lines[i + 1..j])
                .all(|(expected, actual)| expected == &actual.trim());

            if middle_ok {
                let window = &content_lines[i..=j];
                let base = min_indent(window);
                let reindented = reindent_to_base(new_string, base);
                let result = splice_lines(&content_lines, i, j - i + 1, &reindented);
                return Some(EditOutcome {
                    result,
                    strategy: "block_anchored_match",
                    start_line: i + 1,
                    end_line: j + 1,
                });
            }
        }
    }

    None
}

fn indentation_flexible_match(
    content: &str,
    old_string: &str,
    new_string: &str,
) -> Option<EditOutcome> {
    let old_lines: Vec<&str> = old_string.lines().collect();
    let content_lines: Vec<&str> = content.lines().collect();

    let old_indent: Vec<usize> = old_lines
        .iter()
        .map(|l| l.len() - l.trim_start().len())
        .collect();

    let min_old_indent = old_indent.iter().copied().min().unwrap_or(0);
    let old_stripped: Vec<String> = old_lines
        .iter()
        .map(|l| l[min_old_indent..].to_string())
        .collect();

    for window_start in 0..=content_lines.len().saturating_sub(old_lines.len()) {
        let window = &content_lines[window_start..window_start + old_lines.len()];
        let window_indent: Vec<usize> = window
            .iter()
            .map(|l| l.len() - l.trim_start().len())
            .collect();

        let min_window_indent = window_indent.iter().copied().min().unwrap_or(0);

        let window_stripped: Vec<String> = window
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let relative = window_indent[i] - min_window_indent;
                let expected = min_old_indent + relative;
                if l.len() >= expected {
                    l[expected..].to_string()
                } else {
                    l.trim_start().to_string()
                }
            })
            .collect();

        if window_stripped
            .iter()
            .zip(&old_stripped)
            .all(|(a, b)| a == b)
        {
            let indent_prefix = " ".repeat(min_window_indent);
            let new_indented: String = new_string
                .lines()
                .map(|l| format!("{}{}", indent_prefix, l))
                .collect::<Vec<_>>()
                .join("\n");
            let result = splice_lines(&content_lines, window_start, old_lines.len(), &new_indented);
            return Some(EditOutcome {
                result,
                strategy: "indentation_flexible_match",
                start_line: window_start + 1,
                end_line: window_start + old_lines.len(),
            });
        }
    }

    None
}

fn escape_normalized_match(
    content: &str,
    old_string: &str,
    new_string: &str,
) -> Option<EditOutcome> {
    let normalize_escapes = |s: &str| -> String {
        s.replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\r", "\r")
            .replace("\\\\", "\\")
    };

    let old_normalized = normalize_escapes(old_string);
    let content_normalized = normalize_escapes(content);

    if content_normalized.contains(&old_normalized) {
        let idx = content_normalized.find(&old_normalized)?;
        let before_len = content_normalized[..idx].chars().count();
        let match_len = old_normalized.chars().count();

        let content_chars: Vec<char> = content.chars().collect();
        let mut norm_count = 0;
        let mut start = 0;
        let mut end = 0;

        for (i, _) in content_chars.iter().enumerate() {
            if norm_count == before_len {
                start = i;
            }
            norm_count += 1;
            if norm_count == before_len + match_len {
                end = i + 1;
                break;
            }
        }

        if end == 0 {
            end = content_chars.len();
        }

        let mut result = content_chars[..start].iter().collect::<String>();
        result.push_str(new_string);
        result.push_str(&content_chars[end..].iter().collect::<String>());
        let start_line = content_chars[..start]
            .iter()
            .filter(|&&c| c == '\n')
            .count()
            + 1;
        let end_line = start_line + old_string.lines().count().saturating_sub(1);
        Some(EditOutcome {
            result,
            strategy: "escape_normalized_match",
            start_line,
            end_line,
        })
    } else {
        None
    }
}

fn trimmed_boundary_match(
    content: &str,
    old_string: &str,
    new_string: &str,
) -> Option<EditOutcome> {
    let old_trimmed = old_string.trim();
    let content_trimmed = content.trim();

    if content_trimmed == old_trimmed {
        let leading_ws_len = content.len() - content.trim_start().len();
        let trailing_ws_len = content.len() - content.trim_end().len();
        let prefix = &content[..leading_ws_len];
        let suffix = &content[content.len() - trailing_ws_len..];
        let total_lines = content.lines().count().max(1);
        return Some(EditOutcome {
            result: format!("{}{}{}", prefix, new_string, suffix),
            strategy: "trimmed_boundary_match",
            start_line: 1,
            end_line: total_lines,
        });
    }

    let old_lines: Vec<&str> = old_trimmed.lines().collect();
    let content_lines: Vec<&str> = content_trimmed.lines().collect();

    for window_start in 0..=content_lines.len().saturating_sub(old_lines.len()) {
        let window: String = content_lines[window_start..window_start + old_lines.len()]
            .iter()
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join("\n");

        if window == old_trimmed {
            let orig_lines: Vec<&str> = content.lines().collect();
            let mut first_content_line = 0;
            let mut count = 0;
            for (i, line) in orig_lines.iter().enumerate() {
                if !line.trim().is_empty() {
                    if count == 0 {
                        first_content_line = i;
                    }
                    count += 1;
                    if count > window_start {
                        break;
                    }
                }
            }

            let actual_start = first_content_line + window_start;
            let actual_end = actual_start + old_lines.len();
            let window_slice = &orig_lines[actual_start..actual_end.min(orig_lines.len())];
            let base = min_indent(window_slice);
            let reindented = reindent_to_base(new_string, base);
            let result = splice_lines(&orig_lines, actual_start, old_lines.len(), &reindented);
            return Some(EditOutcome {
                result,
                strategy: "trimmed_boundary_match",
                start_line: actual_start + 1,
                end_line: actual_end,
            });
        }
    }

    None
}

fn context_aware_match(content: &str, old_string: &str, new_string: &str) -> Option<EditOutcome> {
    let old_lines: Vec<&str> = old_string.lines().collect();
    let content_lines: Vec<&str> = content.lines().collect();

    if old_lines.len() < 2 {
        return None;
    }

    let context_before = old_lines[0].trim().to_string();
    let context_after = old_lines.last()?.trim().to_string();
    let target_lines: Vec<&str> = old_lines[1..old_lines.len() - 1]
        .iter()
        .map(|l| l.trim())
        .collect();

    for i in 0..content_lines.len().saturating_sub(1) {
        if content_lines[i].trim() != context_before {
            continue;
        }

        for j in (i + 1 + target_lines.len())..content_lines.len() {
            if content_lines[j].trim() != context_after {
                continue;
            }

            let between_len = j - i - 1;
            if between_len < target_lines.len() {
                continue;
            }

            let between_trimmed: Vec<&str> =
                content_lines[i + 1..j].iter().map(|l| l.trim()).collect();

            if between_trimmed
                .iter()
                .zip(&target_lines)
                .all(|(a, b)| *a == *b)
            {
                let window = &content_lines[i..=j];
                let base = min_indent(window);
                let reindented = reindent_to_base(new_string, base);
                let result = splice_lines(&content_lines, i, j - i + 1, &reindented);
                return Some(EditOutcome {
                    result,
                    strategy: "context_aware_match",
                    start_line: i + 1,
                    end_line: j + 1,
                });
            }
        }
    }

    None
}

/// Returns true if the path looks like a structured config file.
fn is_config_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".json")
        || lower.ends_with(".jsonc")
        || lower.ends_with(".json5")
        || lower.ends_with(".toml")
        || lower.ends_with(".yaml")
        || lower.ends_with(".yml")
        || lower.ends_with(".env")
        || lower.ends_with("cargo.toml")
        || lower.ends_with("package.json")
}

fn find_similar_block(content: &str, old_string: &str) -> String {
    let old_lines: Vec<&str> = old_string.lines().collect();
    let content_lines: Vec<&str> = content.lines().collect();
    if old_lines.is_empty() || content_lines.is_empty() {
        return String::new();
    }
    let mut best_dist = usize::MAX;
    let mut best_start = 0;
    let window_size = old_lines.len().min(5);
    for i in 0..=content_lines.len().saturating_sub(window_size) {
        let window: String = content_lines[i..i + window_size].join("\n");
        let dist = levenshtein(
            &window,
            &old_lines[..window_size.min(old_lines.len())].join("\n"),
        );
        if dist < best_dist {
            best_dist = dist;
            best_start = i;
        }
    }
    let start_line = best_start + 1;
    let end_line = best_start + window_size;
    let closest = if best_dist < old_string.len().max(8) / 2 {
        "line_trimmed_match"
    } else {
        "exact_match"
    };
    let snippet_raw =
        content_lines[best_start..(best_start + window_size).min(content_lines.len())].join("\n");
    let snippet_redacted = crate::git_network_policy::redact_url_credentials_in_text(&snippet_raw);
    let mut snippet = snippet_redacted.chars().take(300).collect::<String>();
    if snippet_redacted.chars().count() > 300 {
        snippet.push_str("... [truncated]");
    }
    let before = best_start.saturating_sub(3);
    let after = (best_start + window_size + 3).min(content_lines.len());
    let window = content_lines[before..after].join("\n");
    let mut window_bounded = window.chars().take(600).collect::<String>();
    if window.chars().count() > 600 {
        window_bounded.push_str("\n... [window truncated]");
    }
    format!(
        "\n\nDid you mean lines {start_line}-{end_line}? closest strategy: {closest}\nSnippet (redacted, bounded):\n{snippet}\nBefore/after window lines {}-{}:\n{window_bounded}",
        before + 1, after,
    )
}

#[cfg(test)]
mod tests {
    use super::{grouped_preview, try_edit};

    #[test]
    fn exact_match_replaces_only_the_first_duplicate() {
        let outcome = try_edit("same\nsame", "same", "new").expect("match should be found");
        assert_eq!(outcome.result, "new\nsame");
        assert_eq!(outcome.strategy, "exact_match");
        assert_eq!((outcome.start_line, outcome.end_line), (1, 1));
    }

    #[test]
    fn success_reports_strategy_and_lines_with_grouped_preview() {
        let outcome = try_edit("a\nb\nc\n", "b", "B").expect("match should be found");
        assert_eq!(outcome.strategy, "exact_match");
        assert_eq!((outcome.start_line, outcome.end_line), (2, 2));
        let preview = grouped_preview("a\nb\nc\n", &outcome.result, "f.txt");
        assert!(preview.contains("--- a/f.txt"));
        assert!(preview.contains("-"));
        assert!(preview.contains("+"));
    }

    #[test]
    fn trim_hit_reindents_to_window_base_indent() {
        let content = "fn f() {\n    old\n}\n";
        let outcome = try_edit(content, "  old  ", "new_line\nsecond")
            .expect("trim hit should match with indent");
        assert!(outcome.result.contains("    new_line"));
        assert!(outcome.result.contains("    second"));
        assert_eq!(outcome.strategy, "line_trimmed_match");
        assert_eq!((outcome.start_line, outcome.end_line), (2, 2));
    }

    #[test]
    fn failure_hint_includes_snippet_strategy_and_window() {
        let hint = super::find_similar_block("a\nb\nc\nd\ne\nf\n", "zzz");
        assert!(hint.contains("Did you mean lines"));
        assert!(hint.contains("closest strategy:"));
        assert!(hint.contains("Snippet (redacted, bounded):"));
        assert!(hint.contains("Before/after window lines"));
    }
}
