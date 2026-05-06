use crate::error::ToolError;
use crate::tool::util::{canonicalize_path, validate_path};
use crate::tool::Tool;
use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct EditOp {
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MultiEditInput {
    path: String,
    edits: Vec<EditOp>,
}

pub struct MultiEditTool {
    allowed_root: PathBuf,
    unrestricted: bool,
}

impl MultiEditTool {
    pub fn new() -> Self {
        Self {
            allowed_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            unrestricted: false,
        }
    }

    pub fn with_allowed_root(mut self, root: PathBuf) -> Self {
        self.allowed_root = root;
        self.unrestricted = false;
        self
    }
}

impl Default for MultiEditTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for MultiEditTool {
    fn name(&self) -> &str {
        "multiedit"
    }

    fn description(&self) -> &str {
        "Apply multiple edit operations to a single file sequentially."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "edits": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_string": {
                                "type": "string",
                                "description": "Text to find"
                            },
                            "new_string": {
                                "type": "string",
                                "description": "Text to replace with"
                            },
                            "replace_all": {
                                "type": "boolean",
                                "description": "Replace all occurrences (default: false)"
                            }
                        },
                        "required": ["old_string", "new_string"]
                    },
                    "description": "List of edit operations to apply sequentially"
                }
            },
            "required": ["path", "edits"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let parsed: MultiEditInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid multiedit input: {e}")))?;

        let path = if parsed.path.starts_with('/') {
            std::path::PathBuf::from(&parsed.path)
        } else {
            std::env::current_dir()
                .unwrap_or_default()
                .join(&parsed.path)
        };

        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;

        let result = tokio::task::spawn_blocking(move || {
            let canonical = if unrestricted {
                canonicalize_path(&path)?
            } else {
                validate_path(&path, &allowed_root)?
            };

            let mut content = std::fs::read_to_string(&canonical)
                .map_err(|e| ToolError::Execution(format!("failed to read file: {e}")))?;

            for (i, edit) in parsed.edits.iter().enumerate() {
                let replace_all = edit.replace_all.unwrap_or(false);
                if replace_all {
                    content = content.replace(&edit.old_string, &edit.new_string);
                } else {
                    if let Some(pos) = content.find(&edit.old_string) {
                        content.replace_range(pos..pos + edit.old_string.len(), &edit.new_string);
                    } else {
                        return Err(ToolError::Execution(format!(
                            "edit {i}: '{}' not found in file",
                            edit.old_string
                        )));
                    }
                }
            }

            std::fs::write(&canonical, &content)
                .map_err(|e| ToolError::Execution(format!("failed to write file: {e}")))?;

            Ok::<_, ToolError>(format!(
                "Applied {} edits to {}",
                parsed.edits.len(),
                parsed.path
            ))
        })
        .await
        .map_err(|e| ToolError::Execution(format!("join error: {}", e)))??;

        Ok(result)
    }
}
