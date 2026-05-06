use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::error::ToolError;
use crate::tool::util::{canonicalize_path, validate_path};
use crate::tool::Tool;

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

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'path' parameter".to_string()))?
            .to_string();

        let offset = input["offset"].as_u64().unwrap_or(1) as usize;
        let limit = input["limit"].as_u64().unwrap_or(2000) as usize;

        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;

        let result = tokio::task::spawn_blocking(move || {
            let original_path = Path::new(&path_str);
            let validated_path = if unrestricted {
                canonicalize_path(original_path)?
            } else {
                validate_path(original_path, &allowed_root)?
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
