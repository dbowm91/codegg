use crate::error::ToolError;
use crate::tool::util::validate_path;
use crate::tool::{Tool, ToolCategory};
use async_trait::async_trait;
use ignore::WalkBuilder;
use serde::Deserialize;
use std::path::PathBuf;
use tokio::task;

#[derive(Debug, Deserialize)]
struct ListInput {
    path: Option<String>,
    #[serde(default)]
    max_files: Option<usize>,
}

pub struct ListTool {
    allowed_root: Option<PathBuf>,
    unrestricted: bool,
}

impl ListTool {
    pub fn new() -> Self {
        Self {
            allowed_root: None,
            unrestricted: false,
        }
    }

    pub fn with_allowed_root(mut self, root: PathBuf) -> Self {
        self.allowed_root = Some(root);
        self
    }

    pub fn with_unrestricted(mut self) -> Self {
        self.unrestricted = true;
        self
    }
}

impl Default for ListTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ListTool {
    fn name(&self) -> &str {
        "list"
    }

    fn description(&self) -> &str {
        "List directory tree with ignore patterns (node_modules, .git, etc.). Limited to 300 files by default."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory to list (default: current directory)"
                },
                "max_files": {
                    "type": "number",
                    "description": "Maximum number of files to list (default: 300)"
                }
            }
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let parsed: ListInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid list input: {e}")))?;

        let dir = parsed
            .path
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        if !self.unrestricted {
            let default_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let root = self.allowed_root.as_deref().unwrap_or(&default_root);
            validate_path(&dir, root)?;
        }

        if !dir.is_dir() {
            return Err(ToolError::Execution(format!(
                "not a directory: {}",
                dir.display()
            )));
        }

        let max_files = parsed.max_files.unwrap_or(300);
        let dir_for_task = dir.clone();

        let (entries, truncated) = task::spawn_blocking(move || {
            let canonical_dir = match std::fs::canonicalize(&dir_for_task) {
                Ok(c) => c,
                Err(e) => {
                    return Err(ToolError::Execution(format!(
                        "cannot access {}: {}",
                        dir_for_task.display(),
                        e
                    )));
                }
            };

            let walk = WalkBuilder::new(&dir_for_task)
                .hidden(false)
                .git_ignore(true)
                .follow_links(false)
                .max_depth(Some(3))
                .build();

            let mut entries = Vec::new();
            let mut truncated = false;

            for entry in walk {
                if entries.len() >= max_files {
                    truncated = true;
                    break;
                }
                if let Ok(entry) = entry {
                    if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
                        continue;
                    }
                    let path = entry.path();
                    if let Ok(canonical) = std::fs::canonicalize(path) {
                        if !canonical.starts_with(&canonical_dir) {
                            tracing::warn!(
                                path = %path.display(),
                                "skipping path outside allowed directory"
                            );
                            continue;
                        }
                    } else {
                        tracing::warn!(path = %path.display(), "skipping path that cannot be canonicalized");
                        continue;
                    }
                    if let Ok(rel) = path.strip_prefix(&dir_for_task) {
                        let rel_str = rel.to_string_lossy();
                        if !rel_str.is_empty() {
                            let entry_text = if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                                format!("{}/", rel_str)
                            } else {
                                rel_str.to_string()
                            };
                            entries.push(entry_text);
                        }
                    }
                }
            }

            Ok((entries, truncated))
        })
        .await
        .map_err(|e| ToolError::Execution(format!("spawn_blocking failed: {}", e)))??;

        let mut result = entries.join("\n");
        if truncated {
            result.push_str(&format!(
                "\n\n... [truncated, showing first {} entries; rerun with a larger max_files value to see more]",
                max_files,
            ));
        }

        Ok(result)
    }
}
