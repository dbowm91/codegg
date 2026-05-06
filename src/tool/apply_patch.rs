use crate::error::ToolError;
use crate::tool::util::{canonicalize_path, validate_path};
use crate::tool::Tool;
use async_trait::async_trait;
use serde::Deserialize;
use similar::{ChangeTag, TextDiff};
use std::path::{Path, PathBuf};

const MAX_PATCH_SIZE: usize = 100_000;

#[derive(Debug, Deserialize)]
struct ApplyPatchInput {
    path: String,
    patch: String,
    #[serde(default)]
    mode: Option<String>,
}

pub struct ApplyPatchTool {
    allowed_root: PathBuf,
    unrestricted: bool,
}

impl ApplyPatchTool {
    pub fn new() -> Self {
        Self {
            allowed_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            unrestricted: false,
        }
    }

    pub fn with_allowed_root(mut self, root: PathBuf) -> Self {
        self.allowed_root = root;
        self
    }

    pub fn set_allowed_root(&mut self, root: PathBuf) {
        self.allowed_root = root;
    }
}

impl Default for ApplyPatchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Apply a unified diff patch to a file. Supports add, update, delete, and move operations."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to patch"
                },
                "patch": {
                    "type": "string",
                    "description": "Unified diff patch content"
                },
                "mode": {
                    "type": "string",
                    "enum": ["update", "create", "delete", "move"],
                    "description": "Operation mode (default: update)"
                }
            },
            "required": ["path", "patch"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let parsed: ApplyPatchInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid patch input: {e}")))?;

        if parsed.patch.len() > MAX_PATCH_SIZE {
            return Err(ToolError::Execution(format!(
                "patch exceeds maximum size of {} bytes",
                MAX_PATCH_SIZE
            )));
        }

        let mode = parsed.mode.as_deref().unwrap_or("update");

        match mode {
            "delete" => self.apply_delete(&parsed.path).await,
            "create" => self.apply_create(&parsed.path, &parsed.patch).await,
            "move" => self.apply_move(&parsed.patch).await,
            "update" => self.apply_update(&parsed.path, &parsed.patch).await,
            _ => Err(ToolError::Execution(format!("unknown mode: {mode}"))),
        }
    }
}

impl ApplyPatchTool {
    async fn apply_delete(&self, path: &str) -> Result<String, ToolError> {
        let path_owned = path.to_string();
        let path_for_error = path.to_string();
        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;

        tokio::task::spawn_blocking(move || {
            let original_path = Path::new(&path_owned);
            let validated_path = if unrestricted {
                canonicalize_path(original_path)?
            } else {
                validate_path(original_path, &allowed_root)?
            };

            if !validated_path.exists() {
                return Err(ToolError::Execution(format!(
                    "file not found: {path_owned}"
                )));
            }

            std::fs::remove_file(&validated_path)
                .map_err(|e| ToolError::Execution(format!("failed to delete file: {e}")))
        })
        .await
        .map_err(|e| ToolError::Execution(format!("join error: {}", e)))??;

        Ok(format!("Deleted: {path_for_error}"))
    }

    async fn apply_create(&self, path: &str, content: &str) -> Result<String, ToolError> {
        let path_owned = path.to_string();
        let content_owned = content.to_string();
        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;

        tokio::task::spawn_blocking(move || {
            let original_path = Path::new(&path_owned);
            let validated_path = if unrestricted {
                canonicalize_path(original_path)?
            } else {
                validate_path(original_path, &allowed_root)?
            };

            if let Some(parent) = validated_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    ToolError::Execution(format!("failed to create directory: {e}"))
                })?;
            }

            std::fs::write(&validated_path, content_owned)
                .map_err(|e| ToolError::Execution(format!("failed to create file: {e}")))
        })
        .await
        .map_err(|e| ToolError::Execution(format!("join error: {}", e)))??;

        Ok(format!("Created: {path}"))
    }

    async fn apply_move(&self, patch: &str) -> Result<String, ToolError> {
        let (old_path, new_path) = parse_rename(patch).ok_or_else(|| {
            ToolError::Execution("move mode requires rename in patch header".to_string())
        })?;

        let old_path_owned = old_path.to_string();
        let new_path_owned = new_path.to_string();
        let old_path_for_error = old_path.to_string();
        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;

        tokio::task::spawn_blocking(move || {
            let old_original = Path::new(&old_path_owned);
            let new_original = Path::new(&new_path_owned);

            let old_full = if unrestricted {
                canonicalize_path(old_original)?
            } else {
                validate_path(old_original, &allowed_root)?
            };

            let new_full = if unrestricted {
                canonicalize_path(new_original)?
            } else {
                validate_path(new_original, &allowed_root)?
            };

            if !old_full.exists() {
                return Err(ToolError::Execution(format!(
                    "source file not found: {old_path_for_error}"
                )));
            }

            if let Some(parent) = new_full.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    ToolError::Execution(format!("failed to create directory: {e}"))
                })?;
            }

            std::fs::rename(&old_full, &new_full)
                .map_err(|e| ToolError::Execution(format!("failed to rename file: {e}")))
        })
        .await
        .map_err(|e| ToolError::Execution(format!("join error: {}", e)))??;

        let old_str = old_path.to_string();
        let new_str = new_path.to_string();
        Ok(format!("Renamed: {old_str} -> {new_str}"))
    }

    async fn apply_update(&self, path: &str, patch: &str) -> Result<String, ToolError> {
        let path_owned = path.to_string();
        let patch_owned = patch.to_string();
        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;

        let (_result, preview) = tokio::task::spawn_blocking(move || {
            let original_path = Path::new(&path_owned);
            let validated_path = if unrestricted {
                canonicalize_path(original_path)?
            } else {
                validate_path(original_path, &allowed_root)?
            };

            let original = std::fs::read_to_string(&validated_path)
                .map_err(|e| ToolError::Execution(format!("failed to read file: {e}")))?;

            let result = apply_unified_diff(&original, &patch_owned).ok_or_else(|| {
                ToolError::Execution("failed to apply patch: invalid diff format".to_string())
            })?;

            let preview = generate_diff_preview(&original, &result, &path_owned);

            std::fs::write(&validated_path, &result)
                .map_err(|e| ToolError::Execution(format!("failed to write file: {e}")))?;

            Ok::<_, ToolError>((result, preview))
        })
        .await
        .map_err(|e| ToolError::Execution(format!("join error: {}", e)))??;

        Ok(format!("Applied patch to: {path}\n\n{preview}"))
    }
}

fn apply_unified_diff(original: &str, patch: &str) -> Option<String> {
    let diff = TextDiff::from_lines(original, patch);

    let mut result: Vec<&str> = Vec::new();
    let mut orig_lines: Vec<&str> = original.lines().collect();

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Delete => {
                let line_num = change.old_index()?;
                if line_num > 0 && line_num <= orig_lines.len() {
                    orig_lines.remove(line_num - 1);
                }
            }
            ChangeTag::Insert => {
                let content = change.value().trim_start_matches('+');
                orig_lines.insert(change.new_index().unwrap_or(orig_lines.len()), content);
            }
            ChangeTag::Equal => {
                result.push(change.value().trim_end());
            }
        }
    }

    Some(orig_lines.join("\n"))
}

fn generate_diff_preview(original: &str, modified: &str, _path: &str) -> String {
    let diff = TextDiff::from_lines(original, modified);
    let mut preview = String::from("Preview:\n");

    for change in diff.iter_all_changes() {
        let prefix = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        preview.push_str(&format!("{}{}", prefix, change.value()));
    }

    if preview == "Preview:\n" {
        preview.push_str("(no changes)\n");
    }

    preview
}

fn parse_rename(patch: &str) -> Option<(String, String)> {
    for line in patch.lines() {
        if line.starts_with("rename from ") {
            let old = line.strip_prefix("rename from ")?;
            for line2 in patch.lines() {
                if line2.starts_with("rename to ") {
                    let new = line2.strip_prefix("rename to ")?;
                    return Some((old.to_string(), new.to_string()));
                }
            }
        }
    }

    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("--- a/") {
            for line2 in patch.lines() {
                if let Some(new) = line2.strip_prefix("+++ b/") {
                    return Some((rest.to_string(), new.to_string()));
                }
            }
        }
    }

    None
}
