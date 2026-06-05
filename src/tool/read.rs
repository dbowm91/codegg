use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::error::ToolError;
use crate::tool::util::{canonicalize_path, validate_path};
use crate::tool::{Tool, ToolCategory};

const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "tiff",
];
const PDF_EXTENSIONS: &[&str] = &["pdf"];

fn is_image_extension(ext: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&ext)
}

fn is_pdf_extension(ext: &str) -> bool {
    PDF_EXTENSIONS.contains(&ext)
}

fn path_for_display(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn missing_path_hint(requested: &Path, allowed_root: &Path, unrestricted: bool) -> Option<String> {
    let root = allowed_root.canonicalize().ok()?;
    let candidate = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        root.join(requested)
    };

    let parent = candidate.parent()?;
    let parent_canonical = parent.canonicalize().ok()?;
    if !unrestricted && !parent_canonical.starts_with(&root) {
        return None;
    }

    let requested_name = candidate
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let requested_stem = candidate
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_lowercase();

    let mut suggestions = Vec::new();
    collect_matching_entries(
        &parent_canonical,
        &root,
        &requested_name,
        &requested_stem,
        0,
        &mut suggestions,
    );

    suggestions.sort();
    suggestions.dedup();
    suggestions.truncate(8);

    let mut message = format!("read failed: file not found: {}", candidate.display());
    if parent_canonical.is_dir() {
        message.push_str(&format!(
            "\nParent directory exists: {}",
            path_for_display(&parent_canonical, &root)
        ));
    }
    if !suggestions.is_empty() {
        message.push_str("\nDid you mean one of these paths?\n");
        for suggestion in suggestions {
            message.push_str("- ");
            message.push_str(&suggestion);
            message.push('\n');
        }
    }
    Some(message)
}

fn collect_matching_entries(
    dir: &Path,
    root: &Path,
    requested_name: &str,
    requested_stem: &str,
    depth: usize,
    suggestions: &mut Vec<String>,
) {
    if depth > 4 || suggestions.len() >= 16 {
        return;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_lowercase();
        let name_matches = !requested_name.is_empty() && name.contains(requested_name);
        let stem_matches = !requested_stem.is_empty() && name.starts_with(requested_stem);

        if (name_matches || stem_matches) && file_type.is_file() {
            suggestions.push(path_for_display(&path, root));
            if suggestions.len() >= 16 {
                return;
            }
        }

        if file_type.is_dir() {
            collect_matching_entries(
                &path,
                root,
                requested_name,
                requested_stem,
                depth + 1,
                suggestions,
            );
            if suggestions.len() >= 16 {
                return;
            }
        }
    }
}

pub struct ReadTool {
    allowed_root: PathBuf,
    unrestricted: bool,
}

impl ReadTool {
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

    pub fn set_allowed_root(&mut self, root: PathBuf) {
        self.allowed_root = root;
        self.unrestricted = false;
    }
}

impl Default for ReadTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file with line numbers. Images and PDFs are returned as base64."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                },
                "offset": {
                    "type": "number",
                    "description": "Starting line number (1-indexed)"
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of lines to read"
                }
            },
            "required": ["path"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'path' parameter".to_string()))?
            .to_string();

        let offset = input["offset"]
            .as_f64()
            .map(|v| v.max(1.0) as usize)
            .unwrap_or(1);
        let limit = input["limit"]
            .as_f64()
            .map(|v| v.max(1.0) as usize)
            .unwrap_or(2000);

        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;

        let result = tokio::task::spawn_blocking(move || {
            let original_path = Path::new(&path_str);
            let validated_path = match if unrestricted {
                canonicalize_path(original_path)
            } else {
                validate_path(original_path, &allowed_root)
            } {
                Ok(path) => path,
                Err(err) => {
                    if let Some(hint) =
                        missing_path_hint(original_path, &allowed_root, unrestricted)
                    {
                        return Err(ToolError::Execution(hint));
                    }
                    return Err(err);
                }
            };

            if !validated_path.exists() {
                return Err(ToolError::Execution(format!(
                    "read failed: file not found: {}",
                    validated_path.display()
                )));
            }

            if validated_path.is_dir() {
                let entries = std::fs::read_dir(&validated_path)
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                let mut result = String::new();
                for entry in entries {
                    let entry = entry.map_err(|e| ToolError::Execution(e.to_string()))?;
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        result.push_str(&format!("{}/\n", name));
                    } else {
                        result.push_str(&format!("{}\n", name));
                    }
                }
                return Ok(result);
            }

            let ext = validated_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            if is_image_extension(&ext) || is_pdf_extension(&ext) {
                let data = std::fs::read(&validated_path)
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                let encoded =
                    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);
                let mime = match ext.as_str() {
                    "png" => "image/png",
                    "jpg" | "jpeg" => "image/jpeg",
                    "gif" => "image/gif",
                    "webp" => "image/webp",
                    "svg" => "image/svg+xml",
                    "bmp" => "image/bmp",
                    "ico" => "image/x-icon",
                    "tiff" => "image/tiff",
                    "pdf" => "application/pdf",
                    _ => "application/octet-stream",
                };
                return Ok(format!("[{mime} base64 attachment]\n{encoded}"));
            }

            let content = std::fs::read_to_string(&validated_path)
                .map_err(|e| ToolError::Execution(e.to_string()))?;

            let lines: Vec<&str> = content.lines().collect();
            let start = offset.saturating_sub(1);
            if start >= lines.len() {
                if lines.is_empty() {
                    return Ok("(empty file)".to_string());
                }
                return Ok(format!(
                    "(offset {} is beyond end of file which has {} lines)",
                    offset,
                    lines.len()
                ));
            }
            let end = (start + limit).min(lines.len());
            let selected = &lines[start..end];

            let mut result = String::new();
            for (i, line) in selected.iter().enumerate() {
                result.push_str(&format!("{:>6}: {}\n", start + i + 1, line));
            }

            if lines.len() > end {
                result.push_str(&format!("\n... [{} more lines] ...\n", lines.len() - end));
            }

            Ok(result)
        })
        .await
        .map_err(|e| ToolError::Execution(format!("join error: {}", e)))??;

        Ok(result)
    }
}
