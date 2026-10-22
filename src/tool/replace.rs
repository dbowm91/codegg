use crate::bus::events::AppEvent;
use crate::bus::global::GlobalEventBus;
use crate::error::ToolError;
use crate::preflight::{PreflightDecision, PreflightService};
use crate::tool::backend::ToolExecutionContext;
use crate::tool::util::{canonicalize_path, validate_path};
use crate::tool::writer_fence::{
    acquire_writer_permit, settle_writer_fence, writer_fence_key, NativeEffectClass,
    WriterFenceOutcome,
};
use crate::tool::{Tool, ToolCategory};
use async_trait::async_trait;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_FILE_SIZE: usize = 10 * 1024 * 1024;
const MAX_PATTERN_SIZE: usize = 4096;
const MAX_PATTERN_GROUPS: usize = 32;

pub struct ReplaceTool {
    allowed_root: PathBuf,
    unrestricted: bool,
    preflight: Option<Arc<PreflightService>>,
}

impl ReplaceTool {
    pub fn new() -> Self {
        Self {
            allowed_root: crate::tool::util::unresolved_workspace_root(),
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

    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }

    /// M002: regex replace overlaps `edit`/`apply_patch`; the single-edit
    /// and batch primitives stay core while this variant is discoverable.
    fn defer_loading(&self) -> bool {
        crate::tool::disclosure::is_deferred_by_default(self.name())
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        self.execute_inner(input, None).await
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<crate::tool::StructuredToolResult, ToolError> {
        let output = self.execute_inner(input, ctx.as_ref()).await?;
        Ok(crate::tool::StructuredToolResult::legacy(
            self.name(),
            output,
        ))
    }
}

impl ReplaceTool {
    /// The replace effect, with the checkout writer fence held across it.
    ///
    /// The fence is an *additional* refusal on top of the existing path
    /// boundary: it never widens what this tool may write. A context without
    /// a fence mints no permit, so the legacy path behaves exactly as
    /// before.
    async fn execute_inner(
        &self,
        input: serde_json::Value,
        ctx: Option<&ToolExecutionContext>,
    ) -> Result<String, ToolError> {
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
        let path_for_read = path_str.clone();

        // The read resolves the target and reads it in one step. The resolved
        // path is the one the fence names, and the write below re-resolves
        // the same request through the same resolution — so the fenced path
        // is the path the write lands on.
        let (canonical_path, content) = tokio::task::spawn_blocking(move || {
            let path = Path::new(&path_for_read);
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
            let content = std::fs::read_to_string(&canonical).map_err(|e| {
                ToolError::Execution(format!("read failed for '{}': {}", canonical.display(), e))
            })?;
            Ok::<_, ToolError>((canonical, content))
        })
        .await
        .map_err(|e| ToolError::Execution(format!("join error: {}", e)))??;

        let mut preflight_warnings = Vec::new();
        if let Some(ref svc) = self.preflight {
            // Text replacement check
            match svc
                .check_text_replace(&content, &pattern, &replacement)
                .await
            {
                PreflightDecision::Block { findings } => {
                    return Err(ToolError::Execution(format!(
                        "preflight blocked replace: {}",
                        PreflightDecision::Block { findings }.summary()
                    )));
                }
                w @ PreflightDecision::Warn { .. } => preflight_warnings.push(w.summary()),
                _ => {}
            }
            // Unicode safety check on replacement text
            match svc.check_text_security(&replacement).await {
                PreflightDecision::Block { findings } => {
                    return Err(ToolError::Execution(format!(
                        "preflight blocked replace: {}",
                        PreflightDecision::Block { findings }.summary()
                    )));
                }
                w @ PreflightDecision::Warn { .. } => preflight_warnings.push(w.summary()),
                _ => {}
            }
        }

        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;

        let path_str_clone = path_str.clone();
        let pattern_clone = pattern.clone();
        let replacement_clone = replacement.clone();
        let expected_content = content.clone();

        // The permit is taken after the path boundary and the preflight
        // checks approved the replacement, and immediately before the write;
        // it is held until the write has settled.
        let targets = vec![canonical_path.clone()];
        let permit = acquire_writer_permit(
            ctx,
            NativeEffectClass::FileWrite,
            targets.clone(),
            writer_fence_key(self.name(), &targets, ctx),
        )
        .await?;

        let effect = match tokio::task::spawn_blocking(move || {
            // Every failure below left the file untouched, so it is a
            // `NoEffect`; only the write itself can land partially.
            let no_effect = |error: ToolError| (error, WriterFenceOutcome::NoEffect);

            let path = Path::new(&path_str_clone);

            let canonical = if unrestricted {
                canonicalize_path(path)
            } else if !allowed_root.to_string_lossy().is_empty() {
                validate_path(path, &allowed_root)
            } else {
                Ok(path.to_path_buf())
            }
            .map_err(no_effect)?;

            if !global {
                return Err(no_effect(ToolError::Execution(
                    "non-global replacement not supported. Use 'global: true' or the 'edit' tool for single replacement.".to_string()
                )));
            }

            if pattern_clone.len() > MAX_PATTERN_SIZE {
                return Err(no_effect(ToolError::Execution(format!(
                    "pattern exceeds {} bytes",
                    MAX_PATTERN_SIZE
                ))));
            }

            // Compile once with the `whole` wrapper and validate group count
            // against what actually executes (previously the bare pattern was
            // compiled separately, paying 2x compile cost and validating a
            // different pattern than the one executed).
            let regex_pattern = if case_sensitive {
                format!("(?P<whole>{})", pattern_clone)
            } else {
                format!("(?i)(?P<whole>{})", pattern_clone)
            };

            let re = Regex::new(&regex_pattern)
                .map_err(|e| no_effect(ToolError::Execution(format!(
                    "invalid regex pattern: {}",
                    e
                ))))?;
            // Subtract the synthetic `whole` wrapper group.
            let group_count = re.capture_names().flatten().count().saturating_sub(1);
            if group_count > MAX_PATTERN_GROUPS {
                return Err(no_effect(ToolError::Execution(format!(
                    "too many capture groups (max {})",
                    MAX_PATTERN_GROUPS
                ))));
            }

            let metadata = std::fs::metadata(&canonical)
                .map_err(|e| no_effect(ToolError::Execution(format!(
                    "failed to read file metadata: {}",
                    e
                ))))?;
            if metadata.len() > MAX_FILE_SIZE as u64 {
                return Err(no_effect(ToolError::Execution(format!(
                    "file too large (max {} bytes): {}",
                    MAX_FILE_SIZE,
                    canonical.display()
                ))));
            }

            let content = std::fs::read_to_string(&canonical).map_err(|e| {
                no_effect(ToolError::Execution(format!(
                    "read failed for '{}': {}",
                    canonical.display(),
                    e
                )))
            })?;
            if content != expected_content {
                return Err(no_effect(ToolError::Execution(
                    "file changed while replace preflight was running; retry".to_string(),
                )));
            }

            let matches: Vec<_> = re.find_iter(&content).collect();
            if matches.is_empty() {
                return Err(no_effect(ToolError::Execution(
                    "no matches found for pattern".to_string()
                )));
            }

            let new_content = re.replace_all(&content, replacement_clone.as_str()).to_string();

            // Narrow the check-then-write window (BUG-005): re-verify the
            // file is unchanged immediately before write. True concurrent
            // writers remain last-writer-wins by design; this turns a
            // stale-preflight overwrite into an explicit retry error in
            // the common interleaving.
            let current = std::fs::read_to_string(&canonical).map_err(|e| {
                no_effect(ToolError::Execution(format!(
                    "read failed for '{}': {}",
                    canonical.display(),
                    e
                )))
            })?;
            if current != content {
                return Err(no_effect(ToolError::Execution(
                    "file changed while replace preflight was running; retry".to_string(),
                )));
            }

            // `fs::write` truncates before it writes, so a failure here can
            // leave a partially written file behind: the effect is unproven,
            // not absent.
            std::fs::write(&canonical, new_content.as_bytes()).map_err(|e| {
                let message = format!("write failed for '{}': {}", canonical.display(), e);
                (
                    ToolError::Execution(message.clone()),
                    WriterFenceOutcome::Unproven(message),
                )
            })?;

            Ok::<_, (ToolError, WriterFenceOutcome)>((
                canonical.display().to_string(),
                matches.len(),
                content,
            ))
        })
        .await
        {
            Ok(Ok(written)) => Ok(written),
            Ok(Err(failure)) => Err(failure),
            Err(join_error) => Err((
                ToolError::Execution(format!("join error: {}", join_error)),
                WriterFenceOutcome::Unproven(format!("join error: {}", join_error)),
            )),
        };

        let (path_str_out, matches_len, old_content) = match effect {
            Ok(written) => {
                // The replacement landed and was observed, so the permit is
                // released. A failure to release it is reported: the caller
                // has to know the checkout was not handed on.
                settle_writer_fence(permit, WriterFenceOutcome::Settled, false).await?;
                written
            }
            Err((error, outcome)) => {
                // Settle before the error propagates: a failed effect must
                // still release (or park) the permit it held. The tool's own
                // error is the one the caller sees.
                settle_writer_fence(permit, outcome, true).await?;
                return Err(error);
            }
        };

        GlobalEventBus::publish(AppEvent::FileChanged {
            path: path_str_out.clone(),
            action: "Modified".to_string(),
            old_content: Some(old_content),
        });

        // Config format validation after write
        if let Some(ref svc) = self.preflight {
            if is_config_file(&path_str_out) {
                let content_after = tokio::fs::read_to_string(&path_str_out)
                    .await
                    .unwrap_or_default();
                let ext = std::path::Path::new(&path_str_out)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                let config_decision = match ext {
                    "json" | "jsonc" | "json5" => svc.check_json_valid(&content_after).await,
                    "toml" => svc.check_toml_valid(&content_after).await,
                    _ => svc.check_config(&content_after).await,
                };
                match config_decision {
                    PreflightDecision::Block { findings } => {
                        return Err(ToolError::Execution(format!(
                            "preflight blocked config write: {}",
                            PreflightDecision::Block { findings }.summary()
                        )));
                    }
                    w @ PreflightDecision::Warn { .. } => preflight_warnings.push(w.summary()),
                    _ => {}
                }
            }
        }

        let mut output = format!(
            "Replaced {} occurrence(s) in {} with pattern '{}'",
            matches_len, path_str, pattern
        );
        if !preflight_warnings.is_empty() {
            output = format!("{}\n\n{}", preflight_warnings.join("\n"), output);
        }
        Ok(output)
    }
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
