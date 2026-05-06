use async_trait::async_trait;
use globset::GlobBuilder;
use ignore::WalkBuilder;
use serde_json::json;
use std::path::PathBuf;

use crate::error::ToolError;
use crate::tool::Tool;

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
        let search_path = PathBuf::from(search_path_str);

        {
            let canonical_search = search_path
                .canonicalize()
                .map_err(|_| ToolError::Execution("cannot canonicalize search path".to_string()))?;

            let root_canonical = self
                .allowed_root
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from("."));

            if !canonical_search.starts_with(&root_canonical) {
                if self.unrestricted {
                    tracing::warn!(
                        "GlobTool path '{}' outside allowed_root, unrestricted=true - bypassing",
                        search_path.display()
                    );
                } else {
                    return Err(ToolError::Permission(
                        "search path outside allowed directory".to_string(),
                    ));
                }
            }
        }

        let root_canonical = self
            .allowed_root
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from("."));

        let unrestricted = self.unrestricted;

        let glob = GlobBuilder::new(pattern)
            .case_insensitive(false)
            .build()
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        let matcher = glob.compile_matcher();

        let walk = WalkBuilder::new(&search_path)
            .hidden(false)
            .git_ignore(true)
            .follow_links(false)
            .build();

        let root_canonical = root_canonical.clone();

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

                    if !unrestricted && !canonical.starts_with(&root_canonical) {
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
