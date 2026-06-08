//! Stub tool returned when a domain is configured as `disabled` or
//! when an MCP backend is configured but not implemented.
//!
//! `DisabledTool` deliberately *replaces* the real tool in the
//! registry so the model gets a clear, actionable error if it
//! attempts to call the disabled domain, instead of the call
//! silently routing to a different backend. Use
//! `ToolRegistry::backend_report` (and `/tool-backends`) to
//! diagnose which tools are disabled.
//!
//! See `plans/native_tool_crates_hardening.md` Phase 4 for context.

use crate::error::ToolError;
use crate::tool::{Tool, ToolCategory};
use async_trait::async_trait;
use serde_json::json;

/// A `Tool` whose every call returns a clear, actionable error
/// explaining the configured reason.
pub struct DisabledTool {
    name: &'static str,
    description: String,
    reason: String,
}

impl DisabledTool {
    /// Build a disabled wrapper for a specific tool name and reason.
    /// The model-facing `name()` and `description()` are kept stable
    /// so disabled domains look the same as the real wrapper for
    /// tool-catalog purposes, but the JSON Schema advertises that
    /// the call will fail.
    pub fn new(
        name: &'static str,
        description: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            name,
            description: description.into(),
            reason: reason.into(),
        }
    }
}

#[async_trait]
impl Tool for DisabledTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false,
            "description": format!("Disabled: {}", self.reason),
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Err(ToolError::Execution(self.reason.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn execute_returns_configured_reason() {
        let tool = DisabledTool::new("lsp", "stub", "lsp backend = disabled in [tool_backends]");
        let err = tool.execute(json!({})).await.unwrap_err();
        match err {
            ToolError::Execution(s) => assert!(s.contains("lsp backend = disabled")),
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    #[test]
    fn name_and_description_are_stable() {
        let tool = DisabledTool::new("security", "Deterministic security scanning tool", "off");
        assert_eq!(tool.name(), "security");
        assert_eq!(tool.description(), "Deterministic security scanning tool");
    }
}
