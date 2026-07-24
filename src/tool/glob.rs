use async_trait::async_trait;
use globset::GlobBuilder;
use ignore::WalkBuilder;
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::error::ToolError;
use crate::tool::contract::{
    IdempotencyClass, ToolCachePolicy, ToolCallerPolicy, ToolContract, ToolEffectClass,
};
use crate::tool::util::{canonicalize_path, validate_path};
use crate::tool::{Tool, ToolCategory};

const MAX_PATTERN_SIZE: usize = 4096;
const MAX_WALK_ENTRIES: usize = 100_000;

pub struct GlobTool {
    allowed_root: PathBuf,
    unrestricted: bool,
}

impl GlobTool {
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

impl Default for GlobTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in (default: current directory)"
                }
            },
            "required": ["pattern"]
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
                ttl_secs: 60,
                max_entries: 50,
            },
            output_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string"},
                    "files": {"type": "array", "items": {"type": "string"}},
                    "count": {"type": "integer"},
                    "truncated": {"type": "boolean"}
                },
                "required": ["files"]
            })),
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        if self.unrestricted {
            tracing::warn!("GlobTool executing with unrestricted=true - no path validation");
        }

        let pattern = input["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'pattern' parameter".to_string()))?;

        if pattern.len() > MAX_PATTERN_SIZE {
            return Err(ToolError::Execution(format!(
                "pattern exceeds maximum size of {} bytes",
                MAX_PATTERN_SIZE
            )));
        }

        let search_path_str = input["path"].as_str().unwrap_or(".");
        let search_path = Path::new(search_path_str);

        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;

        let canonical_search = if unrestricted {
            canonicalize_path(search_path)?
        } else {
            validate_path(search_path, &allowed_root)?
        };

        let glob = GlobBuilder::new(pattern)
            .case_insensitive(false)
            .build()
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        let matcher = glob.compile_matcher();

        let walk = WalkBuilder::new(search_path)
            .hidden(false)
            .git_ignore(true)
            .follow_links(false)
            .build();

        let canonical_search = canonical_search.clone();

        let (matches, truncated) = tokio::task::spawn_blocking(move || {
            let mut matches = Vec::new();
            let mut truncated = false;
            for entry in walk {
                if matches.len() >= MAX_WALK_ENTRIES {
                    truncated = true;
                    break;
                }
                if let Ok(entry) = entry {
                    if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
                        continue;
                    }
                    let path = entry.into_path();
                    let canonical = match path.canonicalize() {
                        Ok(c) => c,
                        Err(_) => continue,
                    };

                    if !unrestricted && !canonical.starts_with(&canonical_search) {
                        continue;
                    }

                    if matcher.is_match(&path) {
                        matches.push(canonical.display().to_string());
                    }
                }
            }
            (matches, truncated)
        })
        .await
        .map_err(|e| ToolError::Execution(format!("task join error: {}", e)))?;

        let mut sorted_matches = matches;
        sorted_matches.sort();

        if sorted_matches.is_empty() {
            Ok(format!("No files matching '{}'", pattern))
        } else {
            let mut result = format!("[{} files", sorted_matches.len());
            if truncated {
                result.push_str(&format!(" (truncated at {} entries)", MAX_WALK_ENTRIES));
            }
            result.push_str("]\n\n");
            result.push_str(&sorted_matches.join("\n"));
            Ok(result)
        }
    }
}
