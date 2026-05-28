use crate::bus::events::AppEvent;
use crate::bus::global::GlobalEventBus;
use crate::error::ToolError;
use crate::tool::util::{canonicalize_path, validate_path};
use crate::tool::Tool;
use async_trait::async_trait;
use regex::Regex;
use std::path::{Path, PathBuf};

const MAX_FILE_SIZE: usize = 10 * 1024 * 1024;
const MAX_PATTERN_SIZE: usize = 4096;
const MAX_PATTERN_GROUPS: usize = 32;

pub struct ReplaceTool {
    allowed_root: PathBuf,
    unrestricted: bool,
}

impl ReplaceTool {
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

impl Default for ReplaceTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ReplaceTool {
    fn name(&self) -> &str {
        "replace"
    }

    fn description(&self) -> &str {
        "Find and replace text in a file using regular expressions. Replaces all occurrences by default."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "pattern": {
                    "type": "string",
                    "description": "Regular expression pattern to search for"
                },
                "replacement": {
                    "type": "string",
                    "description": "Text to replace matched patterns with. Use $1, $2, etc. for capture groups."
                },
                "global": {
                    "type": "boolean",
                    "description": "Replace all occurrences (default: true)",
                    "default": true
                },
                "case_sensitive": {
                    "type": "boolean",
                    "description": "Case sensitive matching (default: true)",
                    "default": true
                }
            },
            "required": ["path", "pattern", "replacement"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'path' parameter".to_string()))?
            .to_string();

        let pattern = input["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'pattern' parameter".to_string()))?
            .to_string();

        let replacement = input["replacement"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'replacement' parameter".to_string()))?
            .to_string();

        let global = input["global"].as_bool().unwrap_or(true);

        let case_sensitive = input["case_sensitive"].as_bool().unwrap_or(true);

        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;

        let path_str_clone = path_str.clone();
        let pattern_clone = pattern.clone();
        let replacement_clone = replacement.clone();

        let (path_str_out, matches_len, old_content) = tokio::task::spawn_blocking(move || {
            let path = Path::new(&path_str_clone);

            let canonical = if unrestricted {
                canonicalize_path(path)?
            } else if !allowed_root.to_string_lossy().is_empty() {
                validate_path(path, &allowed_root)?
            } else {
                path.to_path_buf()
            };

            if !canonical.exists() {
                return Err(ToolError::Execution(format!(
                    "file not found: {}",
                    canonical.display()
                )));
            }

            if !global {
                return Err(ToolError::Execution(
                    "non-global replacement not supported. Use 'global: true' or the 'edit' tool for single replacement.".to_string()
                ));
            }

            if pattern_clone.len() > MAX_PATTERN_SIZE {
                return Err(ToolError::Execution(format!(
                    "pattern exceeds {} bytes",
                    MAX_PATTERN_SIZE
                )));
            }

            let group_count = {
                let test_re = Regex::new(&pattern_clone)
                    .map_err(|e| ToolError::Execution(format!("invalid regex pattern: {}", e)))?;
                test_re.capture_names().flatten().count()
            };
            if group_count > MAX_PATTERN_GROUPS {
                return Err(ToolError::Execution(format!(
                    "too many capture groups (max {})",
                    MAX_PATTERN_GROUPS
                )));
            }

            let regex_pattern = if case_sensitive {
                format!("(?P<whole>{})", pattern_clone)
            } else {
                format!("(?i)(?P<whole>{})", pattern_clone)
            };

            let re = Regex::new(&regex_pattern)
                .map_err(|e| ToolError::Execution(format!("invalid regex pattern: {}", e)))?;

            let metadata = std::fs::metadata(&canonical)
                .map_err(|e| ToolError::Execution(format!("failed to read file metadata: {}", e)))?;
            if metadata.len() as usize > MAX_FILE_SIZE {
                return Err(ToolError::Execution(format!(
                    "file too large (max {} bytes): {}",
                    MAX_FILE_SIZE,
                    canonical.display()
                )));
            }

            let content = std::fs::read_to_string(&canonical)
                .map_err(|e| ToolError::Execution(format!("read failed for '{}': {}", canonical.display(), e)))?;

            let matches: Vec<_> = re.find_iter(&content).collect();
            if matches.is_empty() {
                return Err(ToolError::Execution(
                    "no matches found for pattern".to_string()
                ));
            }

            let new_content = re.replace_all(&content, replacement_clone.as_str()).to_string();

            std::fs::write(&canonical, new_content.as_bytes())
                .map_err(|e| ToolError::Execution(format!("write failed for '{}': {}", canonical.display(), e)))?;

            Ok::<_, ToolError>((canonical.display().to_string(), matches.len(), content))
        })
        .await
        .map_err(|e| ToolError::Execution(format!("join error: {}", e)))??;

        GlobalEventBus::publish(AppEvent::FileChanged {
            path: path_str_out.clone(),
            action: "Modified".to_string(),
            old_content: Some(old_content),
        });

        Ok(format!(
            "Replaced {} occurrence(s) in {} with pattern '{}'",
            matches_len, path_str, pattern
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_basic() {
        let content = "hello world hello world";
        let pattern = "hello";
        let replacement = "goodbye";

        let regex_pattern = format!("(?P<whole>{})", pattern);
        let re = Regex::new(&regex_pattern).unwrap();
        let new_content = re.replace_all(content, replacement);

        assert_eq!(new_content, "goodbye world goodbye world");
    }

    #[test]
    fn test_replace_no_matches() {
        let content = "hello world";
        let pattern = "goodbye";

        let regex_pattern = format!("(?P<whole>{})", pattern);
        let re = Regex::new(&regex_pattern).unwrap();
        let matches: Vec<_> = re.find_iter(content).collect();

        assert!(matches.is_empty());
    }

    #[test]
    fn test_replace_capture_groups() {
        let content = "hello123world456";
        let pattern = r"(\d+)";
        let replacement = "[$1]";

        let regex_pattern = format!("(?P<whole>{})", pattern);
        let re = Regex::new(&regex_pattern).unwrap();
        let new_content = re.replace_all(content, replacement);

        assert_eq!(new_content, "hello[123]world[456]");
    }
}
