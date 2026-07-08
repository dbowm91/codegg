use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use strsim::levenshtein;

use crate::bus::events::AppEvent;
use crate::bus::global::GlobalEventBus;
use crate::error::ToolError;
use crate::preflight::{PreflightDecision, PreflightService};
use crate::tool::util::{canonicalize_path, validate_path};
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
            allowed_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
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

    fn description(&self) -> &str {
        "Surgically search and replace text in a file"
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

        let content = tokio::task::spawn_blocking(move || {
            let path = Path::new(&path_for_read);
            let canonical = if unrestricted {
                canonicalize_path(path)?
            } else {
                validate_path(path, &allowed_root)?
            };
            std::fs::read_to_string(&canonical).map_err(|e| {
                ToolError::Execution(format!("read failed for '{}': {}", canonical.display(), e))
            })
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

        let (path_display, old_content, _result) = tokio::task::spawn_blocking(move || {
            let path = Path::new(&path_str_for_closure);

            let canonical = if unrestricted {
                canonicalize_path(path)?
            } else {
                validate_path(path, &allowed_root)?
            };

            let result = try_edit(&content, &old_string_for_closure, &new_string_for_closure)
                .ok_or_else(|| {
                    let hint = find_similar_block(&content, &old_string_for_closure);
                    ToolError::Execution(format!(
                        "could not find exact match for old_string.{}",
                        hint
                    ))
                })?;

            std::fs::write(&canonical, &result).map_err(|e| {
                ToolError::Execution(format!("write failed for '{}': {}", canonical.display(), e))
            })?;

            Ok::<_, ToolError>((canonical.display().to_string(), content, result))
        })
        .await
        .map_err(|e| ToolError::Execution(format!("join error: {}", e)))??;

        GlobalEventBus::publish(AppEvent::FileChanged {
            path: path_str.clone(),
            action: "Modified".to_string(),
            old_content: Some(old_content),
        });

        // Config format validation after write
        if let Some(ref svc) = self.preflight {
            if is_config_file(&path_str) {
                let ext = std::path::Path::new(&path_str)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                let config_decision = match ext {
                    "json" | "jsonc" | "json5" => svc.check_json_valid(&_result).await,
                    "toml" => svc.check_toml_valid(&_result).await,
                    _ => svc.check_config(&_result).await,
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

        let mut output = format!(
            "Edited {}\n\n- {}\n+ {}",
            path_display,
            old_string.lines().count(),
            new_string.lines().count()
        );
        if !preflight_warnings.is_empty() {
            output = format!("{}\n\n{}", preflight_warnings.join("\n"), output);
        }
        Ok(output)
    }
}

fn try_edit(content: &str, old_string: &str, new_string: &str) -> Option<String> {
    let strategies = [
        exact_match,
        line_trimmed_match,
        whitespace_normalized_match,
        block_anchored_match,
        indentation_flexible_match,
        escape_normalized_match,
        trimmed_boundary_match,
        context_aware_match,
    ];

    for strategy in strategies {
        if let Some(result) = strategy(content, old_string, new_string) {
            return Some(result);
        }
    }

    None
}

fn exact_match(content: &str, old_string: &str, new_string: &str) -> Option<String> {
    if content.contains(old_string) {
        Some(content.replace(old_string, new_string))
    } else {
        None
    }
}

fn line_trimmed_match(content: &str, old_string: &str, new_string: &str) -> Option<String> {
    let old_trimmed: Vec<&str> = old_string.lines().map(|l| l.trim()).collect();
    let content_lines: Vec<&str> = content.lines().collect();

    for window_start in 0..=content_lines.len().saturating_sub(old_trimmed.len()) {
        let window = &content_lines[window_start..window_start + old_trimmed.len()];
        if window.iter().zip(&old_trimmed).all(|(a, b)| a.trim() == *b) {
            let before = content_lines[..window_start].join("\n");
            let after = content_lines[window_start + old_trimmed.len()..].join("\n");
            let mut result = if before.is_empty() {
                String::new()
            } else {
                format!("{}\n", before)
            };
            result.push_str(new_string);
            if !after.is_empty() {
                result.push_str(&format!("\n{}", after));
            }
            return Some(result);
        }
    }

    None
}

fn whitespace_normalized_match(
    content: &str,
    old_string: &str,
    new_string: &str,
) -> Option<String> {
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
        Some(result)
    } else {
        None
    }
}

fn block_anchored_match(content: &str, old_string: &str, new_string: &str) -> Option<String> {
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
                let before = content_lines[..i].join("\n");
                let after = content_lines[j + 1..].join("\n");
                let mut result = if before.is_empty() {
                    String::new()
                } else {
                    format!("{}\n", before)
                };
                result.push_str(new_string);
                if !after.is_empty() {
                    result.push_str(&format!("\n{}", after));
                }
                return Some(result);
            }
        }
    }

    None
}

fn indentation_flexible_match(content: &str, old_string: &str, new_string: &str) -> Option<String> {
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
            let before = content_lines[..window_start].join("\n");
            let after = content_lines[window_start + old_lines.len()..].join("\n");
            let indent_prefix = " ".repeat(min_window_indent);
            let new_indented: String = new_string
                .lines()
                .map(|l| format!("{}{}", indent_prefix, l))
                .collect::<Vec<_>>()
                .join("\n");

            let mut result = if before.is_empty() {
                String::new()
            } else {
                format!("{}\n", before)
            };
            result.push_str(&new_indented);
            if !after.is_empty() {
                result.push_str(&format!("\n{}", after));
            }
            return Some(result);
        }
    }

    None
}

fn escape_normalized_match(content: &str, old_string: &str, new_string: &str) -> Option<String> {
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
        Some(result)
    } else {
        None
    }
}

fn trimmed_boundary_match(content: &str, old_string: &str, new_string: &str) -> Option<String> {
    let old_trimmed = old_string.trim();
    let content_trimmed = content.trim();

    if content_trimmed == old_trimmed {
        let leading_ws_len = content.len() - content.trim_start().len();
        let trailing_ws_len = content.len() - content.trim_end().len();
        let prefix = &content[..leading_ws_len];
        let suffix = &content[content.len() - trailing_ws_len..];
        return Some(format!("{}{}{}", prefix, new_string, suffix));
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

            let before = orig_lines[..actual_start].join("\n");
            let after = orig_lines[actual_end..].join("\n");
            let mut result = if before.is_empty() {
                String::new()
            } else {
                format!("{}\n", before)
            };
            result.push_str(new_string);
            if !after.is_empty() {
                result.push_str(&format!("\n{}", after));
            }
            return Some(result);
        }
    }

    None
}

fn context_aware_match(content: &str, old_string: &str, new_string: &str) -> Option<String> {
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
                let before = content_lines[..i].join("\n");
                let after = content_lines[j + 1..].join("\n");
                let mut result = if before.is_empty() {
                    String::new()
                } else {
                    format!("{}\n", before)
                };
                result.push_str(new_string);
                if !after.is_empty() {
                    result.push_str(&format!("\n{}", after));
                }
                return Some(result);
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
    format!("\n\nDid you mean lines {}-{}?", start_line, end_line)
}
