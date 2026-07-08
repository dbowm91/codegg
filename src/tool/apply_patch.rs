use crate::error::ToolError;
use crate::preflight::{PreflightDecision, PreflightService};
use crate::tool::patch_util::apply_unified_diff;
use crate::tool::util::{canonicalize_path, check_path_for_symlinks, validate_path};
use crate::tool::{Tool, ToolCategory};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    preflight: Option<Arc<PreflightService>>,
}

impl ApplyPatchTool {
    pub fn new() -> Self {
        Self {
            allowed_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            unrestricted: false,
            preflight: None,
        }
    }

    pub fn with_allowed_root(mut self, root: PathBuf) -> Self {
        self.allowed_root = root;
        self
    }

    pub fn with_preflight(mut self, service: PreflightService) -> Self {
        self.preflight = Some(Arc::new(service));
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

    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
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
        // Config format validation for create mode
        if let Some(ref svc) = self.preflight {
            if is_config_file(path) {
                let ext = Path::new(path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                let config_decision = match ext {
                    "json" | "jsonc" | "json5" => svc.check_json_valid(content).await,
                    "toml" => svc.check_toml_valid(content).await,
                    _ => svc.check_config(content).await,
                };
                if let PreflightDecision::Block { findings } = config_decision {
                    return Err(ToolError::Execution(format!(
                        "preflight blocked config create: {}",
                        PreflightDecision::Block { findings }.summary()
                    )));
                }
            }
            // Unicode safety check on new content
            let unicode_decision = svc.check_text_security(content).await;
            if let PreflightDecision::Block { findings } = unicode_decision {
                return Err(ToolError::Execution(format!(
                    "preflight blocked create: {}",
                    PreflightDecision::Block { findings }.summary()
                )));
            }
        }

        let path_owned = path.to_string();
        let content_owned = content.to_string();
        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;

        tokio::task::spawn_blocking(move || {
            let original_path = Path::new(&path_owned);
            let validated_path = if unrestricted {
                validate_target_path_unrestricted(original_path)?
            } else {
                validate_target_path(original_path, &allowed_root)?
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
                validate_target_path_unrestricted(new_original)?
            } else {
                validate_target_path(new_original, &allowed_root)?
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

        let (result, preview) = tokio::task::spawn_blocking(move || {
            let original_path = Path::new(&path_owned);
            let validated_path = if unrestricted {
                canonicalize_path(original_path)?
            } else {
                validate_path(original_path, &allowed_root)?
            };

            let original = std::fs::read_to_string(&validated_path)
                .map_err(|e| ToolError::Execution(format!("failed to read file: {e}")))?;

            let result =
                apply_unified_diff(&original, &patch_owned).map_err(ToolError::Execution)?;

            let preview = generate_diff_preview(&original, &result, &path_owned);

            std::fs::write(&validated_path, &result)
                .map_err(|e| ToolError::Execution(format!("failed to write file: {e}")))?;

            Ok::<_, ToolError>((result, preview))
        })
        .await
        .map_err(|e| ToolError::Execution(format!("join error: {}", e)))??;

        // Post-patch validation on the result
        let mut warnings = Vec::new();
        if let Some(ref svc) = self.preflight {
            // Config format validation for config files
            if is_config_file(path) {
                let ext = Path::new(path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                let config_decision = match ext {
                    "json" | "jsonc" | "json5" => svc.check_json_valid(&result).await,
                    "toml" => svc.check_toml_valid(&result).await,
                    _ => svc.check_config(&result).await,
                };
                match config_decision {
                    PreflightDecision::Block { findings } => {
                        return Err(ToolError::Execution(format!(
                            "preflight blocked config write: {}",
                            PreflightDecision::Block { findings }.summary()
                        )));
                    }
                    w @ PreflightDecision::Warn { .. } => warnings.push(w.summary()),
                    _ => {}
                }
            }
            // Unicode safety check on result
            let unicode_decision = svc.check_text_security(&result).await;
            match unicode_decision {
                PreflightDecision::Block { findings } => {
                    return Err(ToolError::Execution(format!(
                        "preflight blocked patch result: {}",
                        PreflightDecision::Block { findings }.summary()
                    )));
                }
                w @ PreflightDecision::Warn { .. } => warnings.push(w.summary()),
                _ => {}
            }
        }

        let mut output = format!("Applied patch to: {path}\n\n{preview}");
        if !warnings.is_empty() {
            output = format!("{}\n\n{}", warnings.join("\n"), output);
        }
        Ok(output)
    }
}

fn generate_diff_preview(original: &str, modified: &str, _path: &str) -> String {
    let diff = similar::TextDiff::from_lines(original, modified);
    let mut preview = String::from("Preview:\n");

    for change in diff.iter_all_changes() {
        let prefix = match change.tag() {
            similar::ChangeTag::Delete => "-",
            similar::ChangeTag::Insert => "+",
            similar::ChangeTag::Equal => " ",
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

fn validate_target_path(path: &Path, allowed_root: &Path) -> Result<PathBuf, ToolError> {
    check_path_for_symlinks(path)?;
    let root_canonical = allowed_root
        .canonicalize()
        .map_err(|_| ToolError::Execution("invalid allowed root".to_string()))?;

    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root_canonical.join(path)
    };

    let parent = candidate
        .parent()
        .ok_or_else(|| ToolError::Execution("invalid path".to_string()))?;
    check_path_for_symlinks(parent)?;
    let parent_canonical = parent
        .canonicalize()
        .map_err(|_| ToolError::Execution(format!("invalid path: {}", parent.display())))?;
    if !parent_canonical.starts_with(&root_canonical) {
        return Err(ToolError::Permission(format!(
            "path '{}' is outside allowed directory",
            path.display()
        )));
    }

    let file_name = candidate
        .file_name()
        .ok_or_else(|| ToolError::Execution("invalid path".to_string()))?;
    Ok(parent_canonical.join(file_name))
}

fn validate_target_path_unrestricted(path: &Path) -> Result<PathBuf, ToolError> {
    check_path_for_symlinks(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| ToolError::Execution("invalid path".to_string()))?;
    check_path_for_symlinks(parent)?;
    let parent_canonical = parent
        .canonicalize()
        .map_err(|_| ToolError::Execution(format!("invalid path: {}", parent.display())))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| ToolError::Execution("invalid path".to_string()))?;
    Ok(parent_canonical.join(file_name))
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn validate_target_path_allows_nonexistent_file_within_root() {
        let temp = TempDir::new().expect("temp dir");
        let root = temp.path();
        std::fs::create_dir_all(root.join("nested")).expect("create nested");

        let validated =
            validate_target_path(Path::new("nested/new.txt"), root).expect("path should validate");
        let expected_parent = root
            .join("nested")
            .canonicalize()
            .expect("canonical nested parent");
        assert_eq!(validated, expected_parent.join("new.txt"));
    }

    #[test]
    fn validate_target_path_rejects_outside_root() {
        let temp = TempDir::new().expect("temp dir");
        let root = temp.path().join("root");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(&outside).expect("create outside");

        let path = Path::new("../outside/new.txt");
        let err = validate_target_path(path, &root).expect_err("must reject");
        match err {
            ToolError::Permission(msg) => {
                assert!(
                    msg.contains("outside allowed directory"),
                    "unexpected permission message: {msg}"
                );
            }
            other => panic!("unexpected error type: {other:?}"),
        }
    }

    #[test]
    fn generated_lsp_patch_applies_with_codegg_patch_parser() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("main.rs");
        let original = "fn main() {\n    old_name();\n}\n";
        std::fs::write(&file_path, original).unwrap();

        let edits = vec![egglsp::lsp_types::TextEdit {
            range: egglsp::lsp_types::Range {
                start: egglsp::lsp_types::Position {
                    line: 1,
                    character: 4,
                },
                end: egglsp::lsp_types::Position {
                    line: 1,
                    character: 14,
                },
            },
            new_text: "new_name()".to_string(),
        }];
        let preview = egglsp::edit::preview_text_edits_for_file(
            "rename",
            &file_path,
            edits,
            Some(dir.path()),
        )
        .unwrap();
        assert!(!preview.files[0].patch_omitted);
        let patch = &preview.files[0].patch;
        let updated = crate::tool::patch_util::apply_unified_diff(original, patch)
            .expect("patch should apply");
        assert_eq!(updated, "fn main() {\n    new_name();\n}");
    }
}
