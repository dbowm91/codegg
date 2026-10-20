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
            allowed_root: crate::tool::util::unresolved_workspace_root(),
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

    fn causal_contract(&self) -> Option<crate::tool_advisor::causal_frontier::ToolCausalContract> {
        crate::tool_advisor::causal_frontier::native_causal_contract("glob")
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
                    "description": "Directory to search in (default: the session workspace root). A relative path resolves against that root."
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

        // Walk the validated root, never the raw request path. `path` defaults
        // to "." and a relative `path` resolves against the workspace root,
        // but this walk started from the process cwd — so it traversed a
        // directory the session does not own and the containment filter below
        // then discarded every match, surfacing as "No files matching ...".
        // The old validate-against-the-process-cwd behavior had been rejecting
        // that walk outright, which masked the split.
        let canonical_search = canonical_search.clone();
        let display_search = canonical_search.clone();
        let walk_root = canonical_search.clone();

        let walk = WalkBuilder::new(&walk_root)
            .hidden(false)
            .git_ignore(true)
            .follow_links(false)
            .build();

        let (matches, truncated, walk_error) = tokio::task::spawn_blocking(move || {
            let mut matches = Vec::new();
            let mut truncated = false;
            let mut walk_error = None;
            for (visited, entry) in walk.enumerate() {
                if visited >= MAX_WALK_ENTRIES {
                    truncated = true;
                    break;
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        walk_error.get_or_insert_with(|| error.to_string());
                        continue;
                    }
                };
                if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
                    continue;
                }
                let path = entry.into_path();
                if !unrestricted && !path.starts_with(&walk_root) {
                    continue;
                }
                // Glob patterns are expressed relative to the requested
                // search directory, while WalkBuilder yields absolute
                // paths. Match against the root-relative subject so a
                // pattern such as `scripts/verify*.sh` behaves the same
                // regardless of the daemon's process cwd.
                let Ok(relative) = path.strip_prefix(&walk_root) else {
                    continue;
                };
                if !matcher.is_match(relative) {
                    continue;
                }
                let canonical = match path.canonicalize() {
                    Ok(c) => c,
                    Err(error) => {
                        walk_error.get_or_insert_with(|| error.to_string());
                        continue;
                    }
                };

                if !unrestricted && !canonical.starts_with(&canonical_search) {
                    continue;
                }

                matches.push(canonical.display().to_string());
            }
            (matches, truncated, walk_error)
        })
        .await
        .map_err(|e| ToolError::Execution(format!("task join error: {}", e)))?;

        let mut sorted_matches = matches;
        sorted_matches.sort();

        if sorted_matches.is_empty() {
            if let Some(error) = walk_error {
                Err(ToolError::Execution(format!(
                    "glob scan under '{}' failed before finding matches: {error}",
                    display_search.display()
                )))
            } else if truncated {
                Ok(format!(
                    "No files matching '{}' found under '{}' before the {}-entry scan limit (results may be incomplete)",
                    pattern,
                    display_search.display(),
                    MAX_WALK_ENTRIES
                ))
            } else {
                Ok(format!(
                    "No files matching '{}' under '{}'",
                    pattern,
                    display_search.display()
                ))
            }
        } else {
            let mut result = format!("[{} files", sorted_matches.len());
            if truncated {
                result.push_str(&format!(" (truncated at {} entries)", MAX_WALK_ENTRIES));
            }
            if let Some(error) = walk_error {
                result.push_str(&format!(" (scan warning: {error})"));
            }
            result.push_str("]\n\n");
            result.push_str(&sorted_matches.join("\n"));
            Ok(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;

    #[tokio::test]
    async fn relative_pattern_matches_under_an_alternate_search_root() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let search_root = workspace.join("scripts");
        std::fs::create_dir_all(search_root.join("nested")).expect("create dirs");
        std::fs::write(search_root.join("verify.sh"), "#!/bin/sh\n").expect("fixture");
        std::fs::write(search_root.join("nested/verify-extra.sh"), "#!/bin/sh\n")
            .expect("nested fixture");

        let tool = GlobTool::new().with_allowed_root(workspace);
        let result = tool
            .execute(json!({"pattern": "verify*.sh", "path": "scripts"}))
            .await
            .expect("glob should match relative to path");
        assert!(result.contains("verify.sh"), "{result}");
        assert!(!result.contains("verify-extra.sh"), "{result}");
    }

    #[tokio::test]
    async fn nested_relative_pattern_matches_workspace_script() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(temp.path().join("scripts")).expect("create scripts");
        std::fs::write(temp.path().join("scripts/verify.sh"), "#!/bin/sh\n").expect("fixture");
        let tool = GlobTool::new().with_allowed_root(temp.path().to_path_buf());
        let result = tool
            .execute(json!({"pattern": "scripts/verify*.sh"}))
            .await
            .expect("glob should match workspace-relative path");
        assert!(result.contains("verify.sh"), "{result}");
    }
}
