//! LSP security context executor adapter.
//!
//! This adapter lives in `src/security/` rather than in `src/tool/` or the
//! TUI layer because:
//!
//! - It is part of the **security review workflow** boundary — callers
//!   come from `security::workflow::enrichment`, not from TUI commands.
//! - Placing it in `src/tool/` would couple the security workflow to
//!   tool internals (e.g. `ToolError`, `LspInput` deserialization).
//! - Keeping it in the security module makes the dependency direction
//!   clear: security depends on tool, not the other way around.
//!
//! The adapter wraps [`crate::tool::lsp::LspTool`] and implements
//! [`SecurityContextExecutor`](super::workflow::context::SecurityContextExecutor),
//! bridging the gap between the security workflow's JSON-in/JSON-out
//! trait and the tool's `execute()` method which returns a `String`.

use std::sync::Arc;

use crate::tool::lsp::LspTool;
use crate::tool::Tool;

use super::workflow::context::SecurityContextExecutor;

/// Maximum allowed `call_depth` for security context requests.
const MAX_CALL_DEPTH: u8 = 2;

/// Maximum allowed `max_call_nodes` for security context requests.
const MAX_CALL_NODES: usize = 64;

/// Fields that indicate a mutating action. Security context requests are
/// read-only — their presence is rejected.
const MUTATION_FIELDS: &[&str] = &[
    "apply", "write", "edit", "patch", "command", "execute", "shell",
];

/// Adapter that implements [`SecurityContextExecutor`] by delegating to
/// [`LspTool`].
///
/// Validates incoming requests before forwarding them to the tool,
/// ensuring the security workflow never accidentally triggers mutations
/// or passes out-of-range parameters.
pub struct LspSecurityContextExecutor {
    tool: Arc<LspTool>,
}

impl LspSecurityContextExecutor {
    /// Create a new executor wrapping the given [`LspTool`].
    pub fn new(tool: Arc<LspTool>) -> Self {
        Self { tool }
    }
}

#[async_trait::async_trait]
impl SecurityContextExecutor for LspSecurityContextExecutor {
    async fn security_context(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        validate_security_context_request(&request)?;

        let mut request = request;
        if request.get("operation").is_none() {
            request["operation"] = serde_json::Value::String("securityContext".to_string());
        }

        let result = self
            .tool
            .execute(request)
            .await
            .map_err(|e| format!("securityContext LSP execution failed: {e}"))?;

        serde_json::from_str(&result)
            .map_err(|e| format!("failed to parse securityContext response as JSON: {e}"))
    }
}

/// Validate a security context request for required fields, value
/// ranges, and absence of mutation fields.
pub fn validate_security_context_request(request: &serde_json::Value) -> Result<(), String> {
    // --- required: file_path (string) ---
    match request.get("file_path") {
        Some(serde_json::Value::String(s)) if !s.is_empty() => {}
        Some(_) => return Err("file_path must be a non-empty string".to_string()),
        None => return Err("file_path is required".to_string()),
    }

    // --- required: security_preset (string) ---
    match request.get("security_preset") {
        Some(serde_json::Value::String(s)) if !s.is_empty() => {}
        Some(_) => return Err("security_preset must be a non-empty string".to_string()),
        None => return Err("security_preset is required".to_string()),
    }

    // --- optional: call_depth 0..=MAX_CALL_DEPTH ---
    if let Some(depth) = request.get("call_depth") {
        let d = depth
            .as_u64()
            .ok_or_else(|| "call_depth must be a number".to_string())?;
        if d > MAX_CALL_DEPTH as u64 {
            return Err(format!("call_depth {d} exceeds maximum {MAX_CALL_DEPTH}"));
        }
    }

    // --- optional: max_call_nodes within cap ---
    if let Some(nodes) = request.get("max_call_nodes") {
        let n = nodes
            .as_u64()
            .ok_or_else(|| "max_call_nodes must be a number".to_string())?;
        if n > MAX_CALL_NODES as u64 {
            return Err(format!(
                "max_call_nodes {n} exceeds maximum {MAX_CALL_NODES}"
            ));
        }
    }

    // --- reject mutation fields ---
    for field in MUTATION_FIELDS {
        if request.get(*field).is_some() {
            return Err(format!(
                "mutation field '{field}' is not allowed in security context requests"
            ));
        }
    }

    Ok(())
}

/// Adapter that implements [`HunkSourceContextExecutor`] by delegating to
/// [`LspTool`].
pub struct LspHunkSourceContextExecutor {
    tool: Arc<LspTool>,
}

impl LspHunkSourceContextExecutor {
    pub fn new(tool: Arc<LspTool>) -> Self {
        Self { tool }
    }
}

#[async_trait::async_trait]
impl crate::security::workflow::context::HunkSourceContextExecutor for LspHunkSourceContextExecutor {
    async fn execute_hunk_source_context(
        &self,
        request: egglsp::hunk_context::HunkSourceNavigationRequest,
    ) -> Result<egglsp::hunk_context::HunkSourceNavigationResponse, String> {
        let mut value = serde_json::to_value(&request)
            .map_err(|e| format!("failed to serialize hunkSourceContext request: {e}"))?;
        value["operation"] = serde_json::Value::String("hunkSourceContext".to_string());

        let result = self
            .tool
            .execute(value)
            .await
            .map_err(|e| format!("hunkSourceContext LSP execution failed: {e}"))?;

        let parsed: serde_json::Value = serde_json::from_str(&result)
            .map_err(|e| format!("failed to parse hunkSourceContext response: {e}"))?;

        serde_json::from_value(parsed["results"].clone())
            .map_err(|e| format!("failed to extract HunkSourceNavigationResponse from results: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn security_context_executor_validates_request() {
        let req = json!({
            "file_path": "/tmp/test.rs",
            "security_preset": "rust_server"
        });
        assert!(
            validate_security_context_request(&req).is_ok(),
            "valid request should pass"
        );
    }

    #[test]
    fn security_context_executor_rejects_bad_call_depth() {
        let req = json!({
            "file_path": "/tmp/test.rs",
            "security_preset": "rust_server",
            "call_depth": 3
        });
        let err = validate_security_context_request(&req).unwrap_err();
        assert!(
            err.contains("call_depth"),
            "error should mention call_depth: {err}"
        );
        assert!(err.contains("3"), "error should mention the value: {err}");
    }

    #[test]
    fn security_context_executor_rejects_missing_file_path() {
        let req = json!({
            "security_preset": "rust_server"
        });
        let err = validate_security_context_request(&req).unwrap_err();
        assert!(
            err.contains("file_path"),
            "error should mention file_path: {err}"
        );
    }

    #[test]
    fn security_context_executor_rejects_missing_preset() {
        let req = json!({
            "file_path": "/tmp/test.rs"
        });
        let err = validate_security_context_request(&req).unwrap_err();
        assert!(
            err.contains("security_preset"),
            "error should mention security_preset: {err}"
        );
    }

    #[test]
    fn security_context_executor_rejects_mutation_fields() {
        let mutation_fields = [
            "apply", "write", "edit", "patch", "command", "execute", "shell",
        ];
        for field in mutation_fields {
            let mut req = json!({
                "file_path": "/tmp/test.rs",
                "security_preset": "rust_server"
            });
            req[field] = json!("some value");
            let err = validate_security_context_request(&req).unwrap_err();
            assert!(
                err.contains(field),
                "error for '{field}' should mention the field name: {err}"
            );
        }
    }

    #[test]
    fn security_context_executor_preserves_caps() {
        // max_call_nodes within cap passes
        let req = json!({
            "file_path": "/tmp/test.rs",
            "security_preset": "rust_server",
            "max_call_nodes": 64
        });
        assert!(
            validate_security_context_request(&req).is_ok(),
            "max_call_nodes=64 should be allowed"
        );

        // max_call_nodes exceeds cap fails
        let req = json!({
            "file_path": "/tmp/test.rs",
            "security_preset": "rust_server",
            "max_call_nodes": 65
        });
        let err = validate_security_context_request(&req).unwrap_err();
        assert!(
            err.contains("max_call_nodes"),
            "error should mention max_call_nodes: {err}"
        );
    }

    #[test]
    fn security_context_executor_rejects_empty_file_path() {
        let req = json!({
            "file_path": "",
            "security_preset": "rust_server"
        });
        let err = validate_security_context_request(&req).unwrap_err();
        assert!(
            err.contains("file_path"),
            "error should mention file_path: {err}"
        );
    }

    #[test]
    fn security_context_executor_allows_optional_fields() {
        let req = json!({
            "file_path": "/tmp/test.rs",
            "security_preset": "rust_server",
            "call_depth": 0,
            "max_call_nodes": 32,
            "max_risk_markers": 80,
            "line": 10,
            "column": 5
        });
        assert!(
            validate_security_context_request(&req).is_ok(),
            "request with all valid optional fields should pass"
        );
    }
}
