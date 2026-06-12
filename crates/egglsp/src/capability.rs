//! LSP server capability discovery, normalization, and fallback responses.
//!
//! [`LspCapabilitySnapshot`] provides a normalized boolean view of what a
//! server supports, derived from the LSP `ServerCapabilities` returned
//! during `initialize`. Callers use [`LspSemanticOperation`] to query
//! whether a specific operation is supported and to obtain a structured
//! [`LspUnavailable`] reason when it is not.
//!
//! All types here are plain data — no live LSP connection is required.

use lsp_types::ServerCapabilities;
use serde::{Deserialize, Serialize};

/// Normalized boolean view of an LSP server's capabilities.
///
/// Constructed via [`LspCapabilitySnapshot::from_capabilities`] from the
/// `ServerCapabilities` returned by a live server, or fabricated for
/// testing. Boolean fields default to `false` (conservative).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspCapabilitySnapshot {
    pub language_id: Option<String>,
    pub server_name: Option<String>,
    pub supports_diagnostics: bool,
    pub supports_document_symbols: bool,
    pub supports_workspace_symbols: bool,
    pub supports_definition: bool,
    pub supports_references: bool,
    pub supports_hover: bool,
    pub supports_completion: bool,
    pub supports_call_hierarchy: bool,
    pub supports_type_hierarchy: bool,
    pub supports_semantic_tokens: bool,
}

/// Semantic operation that a caller wants to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LspSemanticOperation {
    Diagnostics,
    DocumentSymbols,
    WorkspaceSymbols,
    Definition,
    References,
    Hover,
    Completion,
    CallHierarchy,
    TypeHierarchy,
    SemanticTokens,
    SecurityContext,
}

impl LspSemanticOperation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Diagnostics => "diagnostics",
            Self::DocumentSymbols => "documentSymbol",
            Self::WorkspaceSymbols => "workspaceSymbol",
            Self::Definition => "definition",
            Self::References => "references",
            Self::Hover => "hover",
            Self::Completion => "completion",
            Self::CallHierarchy => "callHierarchy",
            Self::TypeHierarchy => "typeHierarchy",
            Self::SemanticTokens => "semanticTokens",
            Self::SecurityContext => "securityContext",
        }
    }
}

/// Structured reason why an LSP operation is unavailable.
///
/// This is **not** an error — it is a model-safe, concise explanation
/// that the tool surface can emit when a server lacks the requested
/// capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspUnavailable {
    pub operation: String,
    pub reason: String,
    pub server: Option<String>,
    pub language_id: Option<String>,
}

impl LspUnavailable {
    pub fn new(op: LspSemanticOperation, reason: impl Into<String>) -> Self {
        Self {
            operation: op.as_str().to_string(),
            reason: reason.into(),
            server: None,
            language_id: None,
        }
    }

    pub fn with_server(mut self, server: impl Into<String>) -> Self {
        self.server = Some(server.into());
        self
    }

    pub fn with_language_id(mut self, lang: impl Into<String>) -> Self {
        self.language_id = Some(lang.into());
        self
    }
}

impl LspCapabilitySnapshot {
    /// Derive a snapshot from live `ServerCapabilities`.
    ///
    /// `server_name` and `language_id` are caller-supplied metadata
    /// not present in the LSP protocol itself.
    pub fn from_capabilities(
        caps: &ServerCapabilities,
        server_name: Option<&str>,
        language_id: Option<&str>,
    ) -> Self {
        Self {
            language_id: language_id.map(String::from),
            server_name: server_name.map(String::from),
            supports_diagnostics: true,
            supports_document_symbols: caps.document_symbol_provider.is_some(),
            supports_workspace_symbols: caps.workspace_symbol_provider.is_some(),
            supports_definition: caps.definition_provider.is_some(),
            supports_references: caps.references_provider.is_some(),
            supports_hover: caps.hover_provider.is_some(),
            supports_completion: caps.completion_provider.is_some(),
            supports_call_hierarchy: caps.call_hierarchy_provider.is_some(),
            // type_hierarchy is only a client capability in lsp-types 0.97;
            // heuristic: if call_hierarchy is supported, assume type_hierarchy
            // may also be available (many servers advertise both).
            supports_type_hierarchy: caps.call_hierarchy_provider.is_some(),
            supports_semantic_tokens: caps.semantic_tokens_provider.is_some(),
        }
    }

    /// Returns `true` when the snapshot indicates the server supports `op`.
    pub fn supports(&self, op: LspSemanticOperation) -> bool {
        match op {
            LspSemanticOperation::Diagnostics => self.supports_diagnostics,
            LspSemanticOperation::DocumentSymbols => self.supports_document_symbols,
            LspSemanticOperation::WorkspaceSymbols => self.supports_workspace_symbols,
            LspSemanticOperation::Definition => self.supports_definition,
            LspSemanticOperation::References => self.supports_references,
            LspSemanticOperation::Hover => self.supports_hover,
            LspSemanticOperation::Completion => self.supports_completion,
            LspSemanticOperation::CallHierarchy => self.supports_call_hierarchy,
            LspSemanticOperation::TypeHierarchy => self.supports_type_hierarchy,
            LspSemanticOperation::SemanticTokens => self.supports_semantic_tokens,
            // SecurityContext is a composite operation that does not map 1:1
            // to a single server capability; always treat as "available" and
            // let individual sub-operations degrade independently.
            LspSemanticOperation::SecurityContext => true,
        }
    }

    /// Returns a human-readable reason when `op` is **not** supported,
    /// or `None` when it is.
    pub fn fallback_reason(&self, op: LspSemanticOperation) -> Option<String> {
        if self.supports(op) {
            return None;
        }
        let name = match self.server_name.as_deref() {
            Some(n) => n.to_string(),
            None => "unknown server".to_string(),
        };
        Some(format!(
            "{} does not advertise {} support for {}",
            name,
            op.as_str(),
            self.language_id.as_deref().unwrap_or("unknown language")
        ))
    }

    /// Build an [`LspUnavailable`] for the given operation.
    pub fn unavailable(&self, op: LspSemanticOperation) -> Option<LspUnavailable> {
        let reason = self.fallback_reason(op)?;
        let mut u = LspUnavailable::new(op, reason);
        if let Some(ref s) = self.server_name {
            u = u.with_server(s.clone());
        }
        if let Some(ref l) = self.language_id {
            u = u.with_language_id(l.clone());
        }
        Some(u)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_snapshot() -> LspCapabilitySnapshot {
        LspCapabilitySnapshot {
            language_id: Some("rust".into()),
            server_name: Some("rust-analyzer".into()),
            supports_diagnostics: true,
            supports_document_symbols: true,
            supports_workspace_symbols: true,
            supports_definition: true,
            supports_references: true,
            supports_hover: true,
            supports_completion: true,
            supports_call_hierarchy: true,
            supports_type_hierarchy: true,
            supports_semantic_tokens: true,
        }
    }

    fn minimal_snapshot() -> LspCapabilitySnapshot {
        LspCapabilitySnapshot {
            language_id: Some("python".into()),
            server_name: Some("pylsp".into()),
            supports_diagnostics: true,
            supports_document_symbols: true,
            supports_workspace_symbols: false,
            supports_definition: true,
            supports_references: true,
            supports_hover: true,
            supports_completion: false,
            supports_call_hierarchy: false,
            supports_type_hierarchy: false,
            supports_semantic_tokens: false,
        }
    }

    #[test]
    fn lsp_capability_snapshot_supports_known_operations() {
        let s = sample_snapshot();
        assert!(s.supports(LspSemanticOperation::Diagnostics));
        assert!(s.supports(LspSemanticOperation::DocumentSymbols));
        assert!(s.supports(LspSemanticOperation::WorkspaceSymbols));
        assert!(s.supports(LspSemanticOperation::Definition));
        assert!(s.supports(LspSemanticOperation::References));
        assert!(s.supports(LspSemanticOperation::Hover));
        assert!(s.supports(LspSemanticOperation::Completion));
        assert!(s.supports(LspSemanticOperation::CallHierarchy));
        assert!(s.supports(LspSemanticOperation::TypeHierarchy));
        assert!(s.supports(LspSemanticOperation::SemanticTokens));
        assert!(s.supports(LspSemanticOperation::SecurityContext));
    }

    #[test]
    fn lsp_capability_snapshot_reports_unavailable_reason() {
        let s = minimal_snapshot();
        assert!(s.supports(LspSemanticOperation::Definition));
        assert!(!s.supports(LspSemanticOperation::WorkspaceSymbols));
        let reason = s.fallback_reason(LspSemanticOperation::WorkspaceSymbols);
        assert!(reason.is_some());
        let reason = reason.unwrap();
        assert!(reason.contains("pylsp"));
        assert!(reason.contains("workspaceSymbol"));
    }

    #[test]
    fn lsp_capability_snapshot_from_live_capabilities() {
        let caps = ServerCapabilities::default();
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("test"), Some("txt"));
        // Default capabilities have no providers set → most booleans false
        assert!(!snap.supports(LspSemanticOperation::Definition));
        assert!(!snap.supports(LspSemanticOperation::Hover));
        assert!(snap.supports(LspSemanticOperation::SecurityContext));
        assert_eq!(snap.server_name.as_deref(), Some("test"));
        assert_eq!(snap.language_id.as_deref(), Some("txt"));
    }

    #[test]
    fn lsp_capability_snapshot_unavailable_struct() {
        let s = minimal_snapshot();
        let u = s.unavailable(LspSemanticOperation::CallHierarchy).unwrap();
        assert_eq!(u.operation, "callHierarchy");
        assert!(u.reason.contains("pylsp"));
        assert_eq!(u.server.as_deref(), Some("pylsp"));
        assert_eq!(u.language_id.as_deref(), Some("python"));
    }

    #[test]
    fn lsp_capability_snapshot_no_unavailable_for_supported() {
        let s = minimal_snapshot();
        assert!(s.unavailable(LspSemanticOperation::Definition).is_none());
    }
}
