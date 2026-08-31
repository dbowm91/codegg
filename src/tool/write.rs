use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::bus::events::AppEvent;
use crate::bus::global::GlobalEventBus;
use crate::error::ToolError;
use crate::tool::{Tool, ToolCategory};

pub struct WriteTool {
    allowed_root: PathBuf,
    unrestricted: bool,
}

impl WriteTool {
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

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Create or overwrite a file with the given content. Runs auto-formatting after write if configured."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
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

        let content = input["content"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'content' parameter".to_string()))?
            .to_string();

        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;
        let content_for_write = content.clone();

        let (existed, old_content, final_path, formatted) =
            tokio::task::spawn_blocking(move || {
                let original_path = Path::new(&path_str);
                let validated_path = if !unrestricted {
                    crate::tool::util::validate_target_path(original_path, &allowed_root)?
                } else {
                    let parent_to_validate = original_path
                        .parent()
                        .ok_or_else(|| ToolError::Execution("invalid path".to_string()))?;
                    crate::tool::util::check_path_for_symlinks(parent_to_validate)?;
                    let parent_canonical = parent_to_validate.canonicalize().map_err(|_| {
                        ToolError::Execution(format!(
                            "invalid path: {}",
                            parent_to_validate.display()
                        ))
                    })?;
                    let file_name = original_path
                        .file_name()
                        .ok_or_else(|| ToolError::Execution("invalid path".to_string()))?;
                    parent_canonical.join(file_name)
                };

                if let Some(parent) = validated_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| ToolError::Execution(e.to_string()))?;
                }

                let existed = std::path::Path::new(&validated_path).exists();
                let old_content = if existed {
                    std::fs::read_to_string(&validated_path).unwrap_or_default()
                } else {
                    String::new()
                };

                std::fs::write(&validated_path, content_for_write.as_bytes())
                    .map_err(|e| ToolError::Execution(e.to_string()))?;

                let formatted = std::fs::read_to_string(&validated_path).unwrap_or_default();

                Ok::<_, ToolError>((existed, old_content, validated_path, formatted))
            })
            .await
            .map_err(|e| ToolError::Execution(format!("join error: {}", e)))??;

        let action_str = if existed { "Modified" } else { "Created" };
        let path_display = final_path.to_string_lossy().to_string();
        GlobalEventBus::publish(AppEvent::FileChanged {
            path: path_display.clone(),
            action: action_str.to_string(),
            old_content: if old_content.is_empty() {
                None
            } else {
                Some(old_content.clone())
            },
        });

        let mut format_note = String::new();
        if formatted != content {
            format_note = "\n\n[auto-formatted]".to_string();
        }

        let action = if existed { "Updated" } else { "Created" };
        let diff = generate_diff(&old_content, &content, &path_display);

        Ok(format!(
            "{} {}{}{}",
            action,
            path_display,
            format_note,
            if diff.is_empty() {
                String::new()
            } else {
                format!("\n\n{diff}")
            }
        ))
    }
}

impl Default for WriteTool {
    fn default() -> Self {
        Self {
            allowed_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            unrestricted: false,
        }
    }
}

fn generate_diff(old: &str, new: &str, path: &str) -> String {
    use similar::TextDiff;
    let diff = TextDiff::from_lines(old, new);
    let mut result = String::new();
    result.push_str(&format!("--- a/{}\n", path));
    result.push_str(&format!("+++ b/{}\n", path));
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
                    change.value().trim_end_matches('\n'),
                ));
            }
        }
    }
    result
}
