//! M003 — Operation-scoped LSP read adapter for Tool Programs.
//!
//! The multiplexed `lsp` tool mixes bounded reads (`diagnostics`,
//! `documentSymbol`, `workspaceSymbol`, `hover`, `goToDefinition`,
//! `findReferences`) with preview/mutation-adjacent operations
//! (`renamePreview`, `formatPreview`, `sourceActionPreview`,
//! `codeActionPreview`, `semanticCheckPreview`, semantic/security
//! contexts, hierarchies, workflows, and any future `executeCommand`).
//! Marking it `DirectOrProgrammatic` wholesale would structurally
//! admit preview-capable inputs to programs, so this hidden
//! `ProgrammaticOnly` adapter exposes only the six read operations
//! with an input schema that cannot express a preview or mutation.
//!
//! Canonical delegation: each allowed read executes through
//! [`crate::tool::lsp::LspTool::execute_scoped_read`] — the exact
//! typed implementation behind the model-facing `lsp` arms — sharing
//! the registry's `LspService` and the program workspace root as
//! `allowed_root`. Path validation, per-operation bounds, `egglsp` /
//! `LocalUntrusted` provenance shaping, and error taxonomy are
//! therefore the canonical rules; this adapter implements no LSP
//! protocol logic of its own. (Delegation uses the typed method
//! rather than the `Tool` trait so the tool-broker boundary guard —
//! all production tool calls go through `ToolBroker::execute` —
//! stays green: the outer adapter call is already fully
//! broker-mediated.)
//!
//! Workspace authority: the broker-supplied
//! [`crate::tool::ToolExecutionContext::cwd`] (program workspace root)
//! becomes the inner tool's `allowed_root`; the workspace-symbol root
//! hint follows the same value. Calls without an execution context
//! fall back to the registry-installed root and still fail closed on
//! path escape.
//!
//! State semantics: LSP diagnostics/symbols/navigation reflect live
//! server state (restarts, indexing, edits), so the contract declares
//! cache DISABLED and documents that a fresh rerun may observe
//! different results. Ledger replay still serves the recorded
//! execution-time result (`Idempotent` in the replay sense, exactly
//! like the M002 `repo_search` seam). Provenance is inherited from
//! the canonical tool (`native` / `egglsp` / `LocalUntrusted`).

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use crate::error::ToolError;
use crate::tool::backend::{StructuredToolResult, ToolExecutionContext, ToolProvenance};
use crate::tool::contract::{
    IdempotencyClass, ToolCachePolicy, ToolCallerPolicy, ToolContract, ToolEffectClass,
};
use crate::tool::{Tool, ToolCategory};

/// Allowed read operations. Everything else on the multiplexed `lsp`
/// surface — previews, code actions, semantic/security contexts,
/// hierarchies, capabilities, hunk contexts, and all workflows — is
/// structurally unavailable (no schema field can name it).
pub const ALLOWED_OPERATIONS: &[&str] = &[
    "diagnostics",
    "documentSymbol",
    "workspaceSymbol",
    "hover",
    "goToDefinition",
    "findReferences",
];

/// Maximum accepted `workspaceSymbol` query length.
pub const MAX_SYMBOL_QUERY_LEN: usize = 200;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LspReadInput {
    operation: String,
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default)]
    line: Option<u32>,
    #[serde(default)]
    column: Option<u32>,
    #[serde(default)]
    symbol: Option<String>,
}

pub struct LspReadTool {
    service: Arc<crate::lsp::service::LspService>,
    allowed_root: PathBuf,
}

impl LspReadTool {
    pub fn new(service: Arc<crate::lsp::service::LspService>) -> Self {
        Self {
            service,
            allowed_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    pub fn with_allowed_root(mut self, root: PathBuf) -> Self {
        self.allowed_root = root;
        self
    }

    fn contract_output_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "description": "Bounded LSP read result in the canonical LspToolOutput shape (operation, result_count, truncated, results). Server-state dependent: a fresh rerun may observe different diagnostics/symbols; ledger replay serves the recorded execution-time result.",
            "properties": {
                "operation": {"type": "string"},
                "result_count": {"type": "number"},
                "truncated": {"type": "boolean"},
                "results": {}
            },
            "required": ["operation", "results"]
        })
    }

    /// Effective workspace root: the broker execution-context cwd
    /// (program workspace root) when it names a directory, else the
    /// registry-installed root.
    fn effective_root(&self, ctx: Option<&ToolExecutionContext>) -> PathBuf {
        if let Some(context) = ctx {
            if context.cwd.is_dir() {
                return context.cwd.clone();
            }
        }
        self.allowed_root.clone()
    }

    /// Validate per-operation required fields with the same bounds
    /// the canonical tool enforces (1-indexed line/column, bounded
    /// symbol query). Unknown operations and preview-only fields
    /// (`new_name`, `action`, `content`, `patch`, …) cannot be
    /// expressed — strict parsing rejects them before dispatch.
    fn validate_input(parsed: &LspReadInput) -> Result<(), ToolError> {
        if !ALLOWED_OPERATIONS.contains(&parsed.operation.as_str()) {
            return Err(ToolError::Execution(format!(
                "unsupported lsp_read operation '{}'; allowed: diagnostics, documentSymbol, workspaceSymbol, hover, goToDefinition, findReferences",
                parsed.operation
            )));
        }
        let require_file = || {
            parsed
                .file_path
                .as_ref()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| {
                    ToolError::Execution(format!("file_path required for {}", parsed.operation))
                })
        };
        let require_position = || match (parsed.line, parsed.column) {
            (Some(l), Some(c)) if l >= 1 && c >= 1 => Ok(()),
            _ => Err(ToolError::Execution(format!(
                "{} requires 1-indexed line and column",
                parsed.operation
            ))),
        };
        match parsed.operation.as_str() {
            "diagnostics" | "documentSymbol" => {
                require_file()?;
            }
            "hover" | "goToDefinition" | "findReferences" => {
                require_file()?;
                require_position()?;
            }
            "workspaceSymbol" => {
                let q = parsed
                    .symbol
                    .as_ref()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| {
                        ToolError::Execution("symbol required for workspaceSymbol".to_string())
                    })?;
                if q.len() > MAX_SYMBOL_QUERY_LEN {
                    return Err(ToolError::Execution(format!(
                        "symbol query exceeds maximum length of {MAX_SYMBOL_QUERY_LEN}"
                    )));
                }
            }
            _ => {
                return Err(ToolError::Execution(format!(
                    "unsupported lsp_read operation '{}'",
                    parsed.operation
                )));
            }
        }
        Ok(())
    }

    async fn execute_with_context(
        &self,
        input: serde_json::Value,
        ctx: Option<&ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        // Strict parse: preview/mutation fields (`new_name`,
        // `action`, `content`, `patch`, `operation_state`, …) fail
        // closed here.
        let parsed: LspReadInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid lsp_read input: {e}")))?;
        Self::validate_input(&parsed)?;

        let effective_root = self.effective_root(ctx);
        let inner = crate::tool::lsp::LspTool::with_cache_config(self.service.clone(), None)
            .with_allowed_root(effective_root.clone());
        let req = crate::tool::lsp::ScopedLspRead {
            operation: parsed.operation.clone(),
            file_path: parsed.file_path.clone(),
            line: parsed.line,
            column: parsed.column,
            symbol: parsed.symbol.clone(),
            execution_root: Some(effective_root),
        };

        // Canonical delegation through the typed scoped-read entry
        // point (the same implementation behind the model-facing
        // `lsp` arms) — never the `Tool` trait, so the broker
        // boundary stays intact.
        let start = std::time::Instant::now();
        let display = inner.execute_scoped_read(&req).await?;
        // The canonical display is the bounded `LspToolOutput` JSON;
        // parse it into a typed value so programs (and broker
        // output-schema validation) observe the same structure
        // direct callers see as text.
        let value: serde_json::Value = serde_json::from_str(&display)
            .unwrap_or_else(|_| serde_json::json!({"operation": parsed.operation, "results": {}}));
        let truncated = value
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        // Provenance mirrors the canonical structured path
        // (`native` / `egglsp` / `LocalUntrusted`).
        let provenance = ToolProvenance {
            backend: crate::tool::ToolBackendKind::Native.label().to_lowercase(),
            implementation: "egglsp".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            elapsed_ms: Some(start.elapsed().as_millis() as u64),
            truncated,
            trust: crate::tool::ToolTrust::LocalUntrusted,
        };
        Ok(StructuredToolResult::with_value(
            display,
            value,
            true,
            Some(provenance),
        ))
    }
}

#[async_trait]
impl Tool for LspReadTool {
    fn name(&self) -> &str {
        "lsp_read"
    }

    fn description(&self) -> &str {
        "Program-only bounded LSP reads (diagnostics, documentSymbol, workspaceSymbol, hover, goToDefinition, findReferences) delegating to the canonical LSP tool. Hidden from ordinary model turns."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["diagnostics", "documentSymbol", "workspaceSymbol", "hover", "goToDefinition", "findReferences"],
                    "description": "Bounded LSP read. diagnostics/documentSymbol need file_path; hover/goToDefinition/findReferences need file_path+line+column (1-indexed); workspaceSymbol needs symbol."
                },
                "file_path": {
                    "type": "string",
                    "description": "Workspace-relative file path (must stay inside the program workspace root)"
                },
                "line": {
                    "type": "number",
                    "description": "1-indexed line number"
                },
                "column": {
                    "type": "number",
                    "description": "1-indexed column number"
                },
                "symbol": {
                    "type": "string",
                    "description": "Symbol query for workspaceSymbol (max 200 chars)"
                }
            },
            "required": ["operation"],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    /// Hidden program-only adapter: never advertised to the model.
    fn expose_in_definitions(&self) -> bool {
        false
    }

    /// M003 programmatic read contract.
    ///
    /// Admission rationale (M001 matrix + M003 operation scoping):
    /// explicit `ProgrammaticOnly` caller policy (the multiplexed
    /// `lsp` tool stays `DirectOnly`); read-side effect (`ReadOnly`);
    /// declared output schema; bounded I/O (canonical per-operation
    /// caps, 1-indexed positions, 200-char symbol query, broker
    /// artifact boundary above); execution-context workspace root
    /// (inner tool validates every path); explicitly
    /// server-state-dependent semantics with cache DISABLED so reruns
    /// re-observe server state while ledger replay serves the
    /// recorded result; no retry; no LSP protocol logic of its own.
    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::ProgrammaticOnly,
            effect_class: ToolEffectClass::ReadOnly,
            idempotency: IdempotencyClass::Idempotent,
            cache_policy: ToolCachePolicy {
                enabled: false,
                ttl_secs: 0,
                max_entries: 0,
            },
            output_schema: Some(Self::contract_output_schema()),
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        Ok(self.execute_with_context(input, None).await?.output)
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        self.execute_with_context(input, ctx.as_ref()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::contract::ToolCallerPolicy;

    fn test_service() -> Arc<crate::lsp::service::LspService> {
        crate::lsp::service::LspService::new_arc(crate::lsp::config_lsp_to_egglsp(
            crate::config::schema::LspConfig::default(),
        ))
    }

    #[test]
    fn lsp_read_contract_is_programmatic_only_read_with_schema() {
        let tool = LspReadTool::new(test_service());
        let contract = tool.contract("lsp_read", tool.parameters());
        assert_eq!(contract.caller_policy, ToolCallerPolicy::ProgrammaticOnly);
        assert_eq!(contract.effect_class, ToolEffectClass::ReadOnly);
        assert_eq!(contract.idempotency, IdempotencyClass::Idempotent);
        assert!(
            !contract.cache_policy.enabled,
            "lsp_read program-call cache must be disabled (server-state dependent)"
        );
        assert_eq!(contract.retry_policy.max_retries, 0);
        assert!(contract.validate().is_ok());
        let schema = contract.output_schema.expect("lsp_read output schema");
        for key in ["operation", "results"] {
            assert!(schema["properties"].get(key).is_some(), "missing {key}");
        }
    }

    #[test]
    fn lsp_read_is_hidden_from_definitions() {
        assert!(!LspReadTool::new(test_service()).expose_in_definitions());
        assert_eq!(
            crate::tool::disclosure::disclosure_for("lsp_read"),
            crate::tool::disclosure::ToolDisclosure::Hidden
        );
    }

    #[test]
    fn preview_operations_are_structurally_unavailable() {
        for op in [
            "renamePreview",
            "formatPreview",
            "sourceActionPreview",
            "codeActionPreview",
            "semanticCheckPreview",
            "semanticContext",
            "securityContext",
            "callHierarchy",
            "executeCommand",
        ] {
            let parsed = LspReadInput {
                operation: op.to_string(),
                file_path: Some("src/main.rs".to_string()),
                line: Some(1),
                column: Some(1),
                symbol: None,
            };
            assert!(
                LspReadTool::validate_input(&parsed).is_err(),
                "op '{op}' must be denied"
            );
        }
    }

    #[test]
    fn preview_fields_fail_closed_at_parse() {
        for extra in [
            "new_name", "action", "content", "patch", "mutation", "recover",
        ] {
            let mut input = serde_json::json!({"operation": "hover", "file_path": "a.rs", "line": 1, "column": 1});
            input[extra] = serde_json::json!("x");
            let parsed: Result<LspReadInput, _> = serde_json::from_value(input);
            assert!(parsed.is_err(), "field '{extra}' must be rejected");
        }
    }

    #[test]
    fn position_and_symbol_bounds_hold() {
        let missing_pos = LspReadInput {
            operation: "hover".to_string(),
            file_path: Some("a.rs".to_string()),
            line: None,
            column: None,
            symbol: None,
        };
        assert!(LspReadTool::validate_input(&missing_pos).is_err());
        let zero_pos = LspReadInput {
            operation: "goToDefinition".to_string(),
            file_path: Some("a.rs".to_string()),
            line: Some(0),
            column: Some(1),
            symbol: None,
        };
        assert!(LspReadTool::validate_input(&zero_pos).is_err());
        let long_symbol = LspReadInput {
            operation: "workspaceSymbol".to_string(),
            file_path: None,
            line: None,
            column: None,
            symbol: Some("x".repeat(201)),
        };
        assert!(LspReadTool::validate_input(&long_symbol).is_err());
    }
}
