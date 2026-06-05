use crate::error::ToolError;
use crate::tool::util::{canonicalize_path, check_path_for_symlinks, validate_path};
use crate::tool::{Tool, ToolCategory};
use async_trait::async_trait;
use serde::Deserialize;
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

        let (_result, preview) = tokio::task::spawn_blocking(move || {
            let original_path = Path::new(&path_owned);
            let validated_path = if unrestricted {
                canonicalize_path(original_path)?
            } else {
                validate_path(original_path, &allowed_root)?
            };

            let original = std::fs::read_to_string(&validated_path)
                .map_err(|e| ToolError::Execution(format!("failed to read file: {e}")))?;

            let result = apply_unified_diff_result(&original, &patch_owned)
                .map_err(ToolError::Execution)?;

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

fn apply_unified_diff_result(original: &str, patch: &str) -> Result<String, String> {
    let original_lines: Vec<&str> = original.lines().collect();
    let patch_lines: Vec<&str> = patch.lines().collect();

    let mut output: Vec<String> = Vec::new();
    let mut orig_idx: usize = 0;
    let mut patch_idx: usize = 0;
    let mut saw_hunk = false;

    while patch_idx < patch_lines.len() {
        let line = patch_lines[patch_idx];

        if !line.starts_with("@@") {
            patch_idx += 1;
            continue;
        }

        saw_hunk = true;
        let old_start = parse_hunk_old_start(line)
            .ok_or_else(|| format!("invalid hunk header: {}", line))?;

        let target_idx = old_start.saturating_sub(1);
        if target_idx < orig_idx {
            return Err(format!(
                "overlapping hunk at original line {}",
                old_start
            ));
        }
        while orig_idx < target_idx && orig_idx < original_lines.len() {
            output.push(original_lines[orig_idx].to_string());
            orig_idx += 1;
        }

        patch_idx += 1;
        while patch_idx < patch_lines.len() {
            let hline = patch_lines[patch_idx];
            if hline.starts_with("@@") {
                break;
            }
            if hline.starts_with("--- ") || hline.starts_with("+++ ") {
                patch_idx += 1;
                continue;
            }
            if hline.starts_with("\\ No newline at end of file") {
                patch_idx += 1;
                continue;
            }

            if hline.is_empty() {
                return Err("invalid empty hunk line".to_string());
            }
            let tag = &hline[..1];
            let content = &hline[1..];
            match tag {
                " " => {
                    if orig_idx >= original_lines.len() || original_lines[orig_idx] != content {
                        return Err(format!(
                            "context mismatch at original line {}",
                            orig_idx + 1
                        ));
                    }
                    output.push(content.to_string());
                    orig_idx += 1;
                }
                "-" => {
                    if orig_idx >= original_lines.len() || original_lines[orig_idx] != content {
                        return Err(format!(
                            "delete mismatch at original line {}",
                            orig_idx + 1
                        ));
                    }
                    orig_idx += 1;
                }
                "+" => output.push(content.to_string()),
                _ => return Err(format!("invalid hunk prefix '{}'", tag)),
            }
            patch_idx += 1;
        }
    }

    if !saw_hunk {
        return Err("patch does not contain any hunks".to_string());
    }

    while orig_idx < original_lines.len() {
        output.push(original_lines[orig_idx].to_string());
        orig_idx += 1;
    }

    Ok(output.join("\n"))
}

fn parse_hunk_old_start(header: &str) -> Option<usize> {
    // @@ -old_start,old_count +new_start,new_count @@
    let mut parts = header.split_whitespace();
    let _at1 = parts.next()?;
    let old_part = parts.next()?;
    if !old_part.starts_with('-') {
        return None;
    }
    let old_nums = &old_part[1..];
    let old_start = old_nums.split(',').next()?.parse::<usize>().ok()?;
    Some(old_start)
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
    let parent_canonical = parent.canonicalize().map_err(|_| {
        ToolError::Execution(format!("invalid path: {}", parent.display()))
    })?;
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
    let parent_canonical = parent.canonicalize().map_err(|_| {
        ToolError::Execution(format!("invalid path: {}", parent.display()))
    })?;
    let file_name = path
        .file_name()
        .ok_or_else(|| ToolError::Execution("invalid path".to_string()))?;
    Ok(parent_canonical.join(file_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn apply_unified_diff_applies_single_hunk() {
        let original = "a\nb\nc";
        let patch = "\
@@ -1,3 +1,3 @@
 a
-b
+B
 c";

        let updated = apply_unified_diff_result(original, patch).expect("patch should apply");
        assert_eq!(updated, "a\nB\nc");
    }

    #[test]
    fn apply_unified_diff_applies_multiple_hunks() {
        let original = "l1\nl2\nl3\nl4\nl5";
        let patch = "\
@@ -1,2 +1,2 @@
 l1
-l2
+L2
@@ -4,2 +4,2 @@
 l4
-l5
+L5";

        let updated = apply_unified_diff_result(original, patch).expect("patch should apply");
        assert_eq!(updated, "l1\nL2\nl3\nl4\nL5");
    }

    #[test]
    fn apply_unified_diff_fails_on_context_mismatch() {
        let original = "a\nb\nc";
        let patch = "\
@@ -1,3 +1,3 @@
 a
 x
 c";

        let err = apply_unified_diff_result(original, patch).expect_err("must fail");
        assert!(
            err.contains("context mismatch"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn apply_unified_diff_fails_on_delete_mismatch() {
        let original = "a\nb\nc";
        let patch = "\
@@ -1,3 +1,2 @@
 a
-x
 c";

        let err = apply_unified_diff_result(original, patch).expect_err("must fail");
        assert!(err.contains("delete mismatch"), "unexpected error: {err}");
    }

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
}
