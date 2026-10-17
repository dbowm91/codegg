//! Stub tools returned when a tool cannot be given real authority.
//!
//! [`DisabledTool`] replaces a tool whose *domain* is configured as disabled,
//! or whose MCP backend is configured but not implemented.
//!
//! [`UnrootedTool`] replaces a workspace-scoped tool that was reached with no
//! resolved workspace root. It is a different condition with a different
//! remedy: `DisabledTool` says "an operator turned this off", `UnrootedTool`
//! says "there is no directory this call is allowed to act in", and the fix
//! for the second is to resolve a root, never to widen the search.
//!
//! Both deliberately *replace* the real tool in the registry so the model
//! gets a clear, actionable error if it attempts to call it, instead of the
//! call silently routing somewhere else. Use `ToolRegistry::backend_report`
//! (and `/tool-backends`) to diagnose disabled domains.
//!
//! See `plans/native_tool_crates_hardening.md` Phase 4 for context.

use crate::error::ToolError;
use crate::tool::{StructuredToolResult, Tool, ToolCategory, ToolExecutionContext};
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

    /// Disabled tools are not exposed to the model: a disabled
    /// domain is intentionally hidden from the catalog so the model
    /// does not pick a tool whose every call is a guaranteed
    /// failure. The stub still exists in the registry so that
    /// `/tool-backends` and tests can introspect the disabled
    /// reason.
    fn expose_in_definitions(&self) -> bool {
        false
    }
}

/// A workspace-scoped tool that was reached with no resolved workspace root.
///
/// This is a *wrapper*, not a parallel stub: it delegates the wrapped tool's
/// name, description, schema, effect class, and `ToolContract`, and replaces
/// only the ability to execute. Delegating the contract matters — contracts
/// are a property of the tool *type*, and the Tool Program validator resolves
/// them from a registry to decide program callability. A stub with the
/// conservative default contract made every workspace tool look
/// non-program-callable, which would have rejected legitimate Tool Programs.
///
/// Constructing the wrapped tool is free of process state: since W001 its
/// `allowed_root` seeds [`crate::tool::util::unresolved_workspace_root`]
/// rather than `std::env::current_dir()`, so the inner tool can never observe
/// the daemon's launch directory — and even if it somehow executed, the shared
/// path validators would refuse it with the same typed error.
pub struct UnrootedTool {
    inner: Box<dyn Tool>,
}

impl UnrootedTool {
    /// Wrap `inner`, discarding its ability to execute.
    pub fn wrapping(inner: Box<dyn Tool>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Tool for UnrootedTool {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters(&self) -> serde_json::Value {
        self.inner.parameters()
    }

    fn category(&self) -> ToolCategory {
        self.inner.category()
    }

    fn defer_loading(&self) -> bool {
        self.inner.defer_loading()
    }

    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Err(ToolError::MissingWorkspaceRoot {
            context: format!(
                "'{}' has no workspace root, so it has no directory it is allowed to act in",
                self.inner.name()
            ),
        })
    }

    async fn execute_structured(
        &self,
        _input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        // Deliberately not delegated: the inner tool must never run, on either
        // execution path.
        Err(ToolError::MissingWorkspaceRoot {
            context: format!(
                "'{}' has no workspace root, so it has no directory it is allowed to act in",
                self.inner.name()
            ),
        })
    }

    /// Delegates so the tool's program-callability, effect class, cache
    /// policy, and retry policy stay identical to the real tool's.
    fn contract(
        &self,
        tool_name: &str,
        input_schema: serde_json::Value,
    ) -> crate::tool::contract::ToolContract {
        self.inner.contract(tool_name, input_schema)
    }

    fn causal_contract(&self) -> Option<crate::tool_advisor::causal_frontier::ToolCausalContract> {
        self.inner.causal_contract()
    }

    /// Hidden from the model's definitions, like [`DisabledTool`]: a tool
    /// whose every call is a guaranteed failure should not be offered.
    /// The stub stays in the registry so the refusal reason is introspectable.
    fn expose_in_definitions(&self) -> bool {
        false
    }

    /// There is no backend: every call is refused before reaching one.
    fn has_functional_backend(&self) -> bool {
        false
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

    /// A minimal inner tool standing in for a workspace-scoped one.
    struct Probe;

    #[async_trait]
    impl Tool for Probe {
        fn name(&self) -> &str {
            "read"
        }

        fn description(&self) -> &str {
            "Read a file from the workspace"
        }

        fn parameters(&self) -> serde_json::Value {
            json!({ "type": "object", "properties": { "path": { "type": "string" } } })
        }

        async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
            Ok("inner ran".to_string())
        }
    }

    #[tokio::test]
    async fn unrooted_tool_reports_a_missing_root_rather_than_an_execution_failure() {
        let tool = UnrootedTool::wrapping(Box::new(Probe));
        let err = tool
            .execute(json!({ "path": "README.md" }))
            .await
            .unwrap_err();
        match err {
            ToolError::MissingWorkspaceRoot { context } => {
                assert!(context.contains("'read'"), "{context}");
            }
            other => panic!(
                "a missing workspace root must not be reported as {other:?}; it is a \
                 distinct condition with a distinct remedy"
            ),
        }
        // The inner tool must not have run, and the stub must not be offered.
        assert!(!tool.expose_in_definitions());
        assert!(!tool.has_functional_backend());
    }

    #[tokio::test]
    async fn unrooted_tool_delegates_metadata_and_contract() {
        let tool = UnrootedTool::wrapping(Box::new(Probe));
        assert_eq!(tool.name(), "read");
        assert_eq!(tool.description(), "Read a file from the workspace");
        // The schema is the real tool's, not a placeholder: a stub schema
        // would change what a model sees between rooted and unrooted builds.
        assert_eq!(tool.parameters()["properties"]["path"]["type"], "string");
        // Contracts are a property of the tool type; the Tool Program
        // validator resolves them from a registry, so a stub with the
        // conservative default contract would reject legitimate programs.
        let contract = tool.contract("read", tool.parameters());
        assert_eq!(contract.name, "read");
        assert!(!tool.expose_in_definitions());
    }
}
