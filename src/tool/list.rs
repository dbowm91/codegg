use crate::error::ToolError;
use crate::tool::contract::{
    IdempotencyClass, ToolCachePolicy, ToolCallerPolicy, ToolContract, ToolEffectClass,
};
use crate::tool::util::{canonicalize_path, tolerant_opt_usize, validate_path};
use crate::tool::{Tool, ToolCategory};
use async_trait::async_trait;
use ignore::WalkBuilder;
use serde::Deserialize;
use std::path::PathBuf;
use tokio::task;

#[derive(Debug, Deserialize)]
struct ListInput {
    path: Option<String>,
    #[serde(default, deserialize_with = "tolerant_opt_usize")]
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
                    "description": "Directory to list (default: the session workspace root). A relative path resolves against that root."
                },
                "max_files": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum number of files to list (default: 300)"
                }
            }
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
                ttl_secs: 30,
                max_entries: 50,
            },
            output_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "entries": {"type": "array", "items": {"type": "string"}},
                    "count": {"type": "integer"},
                    "truncated": {"type": "boolean"}
                },
                "required": ["entries"]
            })),
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let parsed: ListInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid list input: {e}")))?;

        // An omitted `path` lists the session's workspace. It used to default
        // to the daemon process's own current directory, which under the
        // daemon is whatever tree the daemon was launched from — so a session
        // listing "its" directory could be reading an unrelated repository.
        let root = self.allowed_root.clone();
        let dir = match parsed.path.as_ref() {
            Some(raw) => PathBuf::from(raw),
            None => root.clone().ok_or_else(|| {
                ToolError::Execution(
                    "no workspace root is bound to this tool; pass 'path' explicitly".to_string(),
                )
            })?,
        };

        // Walk the *validated* path. This used to validate `dir` and then
        // discard the result, walking the raw request instead — so the
        // containment check authorized one directory while the listing came
        // from another.
        let dir_for_task = if self.unrestricted {
            canonicalize_path(&dir)?
        } else {
            let root = root.ok_or_else(|| {
                ToolError::Execution(
                    "no workspace root is bound to this tool; pass 'path' explicitly".to_string(),
                )
            })?;
            validate_path(&dir, &root)?
        };

        if !dir_for_task.is_dir() {
            return Err(ToolError::Execution(format!(
                "not a directory: {}",
                dir_for_task.display()
            )));
        }

        let max_files = parsed.max_files.unwrap_or(300);

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
