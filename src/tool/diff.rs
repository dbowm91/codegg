use crate::error::ToolError;
use crate::tool::Tool;
use async_trait::async_trait;
use serde::Deserialize;
use similar::{ChangeTag, TextDiff};
use std::path::{Path, PathBuf};

const MAX_FILE_SIZE: usize = 10 * 1024 * 1024;

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

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let parsed: DiffInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid diff input: {e}")))?;

        let allowed_root = self.allowed_root.clone();
        let path_str = parsed.path.clone();

        let current = tokio::task::spawn_blocking(move || {
            let path = Path::new(&path_str);
            let canonical = std::fs::canonicalize(path)
                .map_err(|_| ToolError::Execution(format!("invalid path: {}", path.display())))?;
            let root_canonical = std::fs::canonicalize(&allowed_root)
                .map_err(|_| ToolError::Execution("invalid allowed root".to_string()))?;
            if !canonical.starts_with(&root_canonical) {
                return Err(ToolError::Permission(format!(
                    "path '{}' is outside allowed directory",
                    path.display()
                )));
            }

            if !path.exists() {
                return Err(ToolError::Execution(format!(
                    "file not found: {}",
                    path.display()
                )));
            }

            let metadata = std::fs::metadata(path)
                .map_err(|e| ToolError::Execution(format!("failed to read file metadata: {e}")))?;
            if metadata.len() as usize > MAX_FILE_SIZE {
                return Err(ToolError::Execution(format!(
                    "file too large (max {} bytes): {}",
                    MAX_FILE_SIZE,
                    path.display()
                )));
            }

            std::fs::read_to_string(path)
                .map_err(|e| ToolError::Execution(format!("failed to read file: {e}")))
        })
        .await
        .map_err(|e| ToolError::Execution(format!("join error: {e}")))??;

        match parsed.original {
            Some(original) => {
                let unified = generate_unified_diff(
                    &original,
                    &current,
                    &parsed.path,
                    parsed.line_range.as_ref(),
                );
                Ok(unified)
            }
            None => Err(ToolError::Execution(
                "original content required for diff tool. Use \"original\" parameter.".to_string(),
            )),
        }
    }
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
}
