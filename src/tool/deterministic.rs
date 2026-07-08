use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

use crate::eggsact::adapter::{to_structured_result, EggsactRuntime};
use crate::error::ToolError;
use crate::tool::backend::{StructuredToolResult, ToolExecutionContext};
use crate::tool::{Tool, ToolCategory};

/// Generic wrapper around any eggsact tool, exposed to the model as a
/// Codegg-native deterministic tool.
///
/// Each instance maps a Codegg tool name to an eggsact tool name with a
/// fixed JSON Schema, description, and visibility flag. The wrapper
/// delegates both `execute()` and `execute_structured()` to the shared
/// `EggsactRuntime` and attaches eggsact provenance.
pub struct EggsactTool {
    codegg_name: &'static str,
    eggsact_name: &'static str,
    description: &'static str,
    parameters: serde_json::Value,
    category: ToolCategory,
    defer: bool,
    expose: bool,
    runtime: Arc<EggsactRuntime>,
}

impl EggsactTool {
    pub fn new(
        codegg_name: &'static str,
        eggsact_name: &'static str,
        description: &'static str,
        parameters: serde_json::Value,
        category: ToolCategory,
        defer: bool,
        expose: bool,
        runtime: Arc<EggsactRuntime>,
    ) -> Self {
        Self {
            codegg_name,
            eggsact_name,
            description,
            parameters,
            category,
            defer,
            expose,
            runtime,
        }
    }
}

#[async_trait]
impl Tool for EggsactTool {
    fn name(&self) -> &str {
        self.codegg_name
    }

    fn description(&self) -> &str {
        self.description
    }

    fn parameters(&self) -> serde_json::Value {
        self.parameters.clone()
    }

    fn category(&self) -> ToolCategory {
        self.category
    }

    fn defer_loading(&self) -> bool {
        self.defer
    }

    fn expose_in_definitions(&self) -> bool {
        self.expose
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let result = self.runtime.call_json(self.eggsact_name, input)?;
        Ok(result.output)
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let result = self.runtime.call_json(self.eggsact_name, input)?;
        Ok(to_structured_result(self.eggsact_name, result))
    }
}

/// Helper to build a simple `{ "type": "object", "properties": ..., "required": ... }` schema.
fn simple_schema(props: serde_json::Value, required: &[&str]) -> serde_json::Value {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
    })
}

/// Build the full set of eggsact-backed deterministic tools.
///
/// Returns `(always_visible, deferred)` where each entry is an `EggsactTool`.
/// Caller decides registration order in `ToolRegistry::with_options`.
pub fn build_eggsact_tools(runtime: Arc<EggsactRuntime>) -> (Vec<EggsactTool>, Vec<EggsactTool>) {
    let mut always_visible: Vec<EggsactTool> = Vec::new();
    let mut deferred: Vec<EggsactTool> = Vec::new();

    // ── Always-visible (expose_in_definitions = true) ────────────────

    always_visible.push(EggsactTool::new(
        "text_equal",
        "text_equal",
        "Compare two strings for equality under various modes (raw, normalized, casefolded, trimmed). Use when verifying text identity; prefer over regex for equality checks.",
        simple_schema(json!({
            "a": { "type": "string", "description": "First text string" },
            "b": { "type": "string", "description": "Second text string" },
        }), &["a", "b"]),
        ToolCategory::ReadOnly,
        false,
        true,
        runtime.clone(),
    ));

    always_visible.push(EggsactTool::new(
        "text_diff_explain",
        "text_diff_explain",
        "Explain why two strings differ with Unicode-aware span analysis, codepoints, confusables, and normalization equivalence. Use when diagnosing subtle text mismatches.",
        simple_schema(json!({
            "a": { "type": "string", "description": "First text string" },
            "b": { "type": "string", "description": "Second text string" },
        }), &["a", "b"]),
        ToolCategory::ReadOnly,
        false,
        true,
        runtime.clone(),
    ));

    always_visible.push(EggsactTool::new(
        "text_replace_check",
        "text_replace_check",
        "Check whether a text replacement would apply cleanly before editing. Reports match count, positions, ambiguity, and before/after preview. Use before calling edit/apply_patch.",
        simple_schema(json!({
            "text": { "type": "string", "description": "Full source text" },
            "old": { "type": "string", "description": "Text to find" },
            "new": { "type": "string", "description": "Replacement text" },
        }), &["text", "old", "new"]),
        ToolCategory::ReadOnly,
        false,
        true,
        runtime.clone(),
    ));

    always_visible.push(EggsactTool::new(
        "validate_json",
        "validate_json",
        "Validate JSON syntax and report precise parse errors or structure info. Use before writing or editing JSON files.",
        simple_schema(json!({
            "text": { "type": "string", "description": "JSON text to validate" },
        }), &["text"]),
        ToolCategory::ReadOnly,
        false,
        true,
        runtime.clone(),
    ));

    always_visible.push(EggsactTool::new(
        "validate_toml",
        "validate_toml",
        "Validate TOML files (Cargo.toml, pyproject.toml, etc.) and report parse errors with line/column. Use before writing or editing TOML.",
        simple_schema(json!({
            "text": { "type": "string", "description": "TOML text to validate" },
        }), &["text"]),
        ToolCategory::ReadOnly,
        false,
        true,
        runtime.clone(),
    ));

    always_visible.push(EggsactTool::new(
        "command_preflight",
        "command_preflight",
        "Analyze a shell command before execution: parse argv, detect features (network, filesystem, env, process), find risk patterns, and return a verdict. Use before running bash commands.",
        simple_schema(json!({
            "command": { "type": "string", "description": "Shell command to analyze" },
        }), &["command"]),
        ToolCategory::ReadOnly,
        false,
        true,
        runtime.clone(),
    ));

    always_visible.push(EggsactTool::new(
        "path_normalize",
        "path_normalize",
        "Normalize a filesystem path: collapse dot segments, resolve components. Use when comparing or validating file paths.",
        simple_schema(json!({
            "path": { "type": "string", "description": "Path to normalize" },
        }), &["path"]),
        ToolCategory::ReadOnly,
        false,
        true,
        runtime.clone(),
    ));

    always_visible.push(EggsactTool::new(
        "text_security_inspect",
        "text_security_inspect",
        "Security-oriented text hygiene pass: detect hidden chars, confusables, mixed scripts, normalization issues, and prompt injection patterns. Returns allow/review/block verdict. Use when handling untrusted text.",
        simple_schema(json!({
            "text": { "type": "string", "description": "Text to inspect for security issues" },
        }), &["text"]),
        ToolCategory::ReadOnly,
        false,
        true,
        runtime.clone(),
    ));

    // ── Deferred / contextual (discoverable via tool_search only) ─────

    deferred.push(EggsactTool::new(
        "text_inspect",
        "text_inspect",
        "Inspect a string for hidden characters, Unicode confusables, mixed scripts, normalization state, and display-safe representation.",
        simple_schema(json!({
            "text": { "type": "string", "description": "Text to inspect" },
        }), &["text"]),
        ToolCategory::ReadOnly,
        true,
        true,
        runtime.clone(),
    ));

    deferred.push(EggsactTool::new(
        "config_preflight",
        "config_preflight",
        "Validate generated config text. Auto-detects format (JSON, TOML, YAML) and runs the appropriate validator. Returns valid/invalid with parse error details.",
        simple_schema(json!({
            "text": { "type": "string", "description": "Config text to validate" },
        }), &["text"]),
        ToolCategory::ReadOnly,
        true,
        true,
        runtime.clone(),
    ));

    deferred.push(EggsactTool::new(
        "identifier_inspect",
        "identifier_inspect",
        "Inspect identifiers for validity and collisions. Detects confusables, mixed scripts, normalization issues, and casefold collisions across a list of identifiers.",
        simple_schema(json!({
            "identifiers": { "type": "array", "items": { "type": "string" }, "description": "List of identifiers to inspect" },
        }), &["identifiers"]),
        ToolCategory::ReadOnly,
        true,
        true,
        runtime.clone(),
    ));

    deferred.push(EggsactTool::new(
        "structured_data_compare",
        "structured_data_compare",
        "Compare structured config/data output (JSON). Calls canonicalization and shape comparison. Returns equal/not-equal verdict with structured diffs.",
        simple_schema(json!({
            "a": { "type": "string", "description": "First data string (JSON)" },
            "b": { "type": "string", "description": "Second data string (JSON)" },
        }), &["a", "b"]),
        ToolCategory::ReadOnly,
        true,
        true,
        runtime.clone(),
    ));

    deferred.push(EggsactTool::new(
        "text_fingerprint",
        "text_fingerprint",
        "Compute a deterministic SHA-256 fingerprint of text with canonicalization options. Use for deduplication or change detection.",
        simple_schema(json!({
            "text": { "type": "string", "description": "Text to fingerprint" },
        }), &["text"]),
        ToolCategory::ReadOnly,
        true,
        true,
        runtime.clone(),
    ));

    (always_visible, deferred)
}
