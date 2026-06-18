//! LSP server capability discovery, normalization, and fallback responses.
//!
//! [`LspCapabilitySnapshot`] provides a normalized boolean view of what a
//! server supports, derived from the LSP `ServerCapabilities` returned
//! during `initialize`. Callers use [`LspSemanticOperation`] to query
//! whether a specific operation is supported and to obtain a structured
//! [`LspUnavailable`] reason when it is not.
//!
//! # Phase 4 changes
//!
//! The snapshot now exposes a broader set of normalized booleans
//! (declaration, implementation, document highlight, signature help,
//! rename + prepare rename, code actions, formatting, inlay hints,
//! folding ranges, selection ranges, document links, execute command)
//! plus a [`LspCapabilityDetails`] struct that preserves option
//! information that a single bool cannot capture (rename prepare
//! provider, code-action kinds, trigger characters, semantic-token
//! legend).
//!
//! Diagnostics support is split into push/pull advertised support
//! rather than the previous "every initialized server has diagnostics"
//! assumption. The old `supports_diagnostics: bool` field is kept as a
//! legacy alias for callers that only need a coarse signal — it is
//! true when either push or pull is advertised.
//!
//! All types here are plain data — no live LSP connection is required.

use lsp_types::ServerCapabilities;
use serde::{Deserialize, Serialize};

/// Normalized boolean view of an LSP server's capabilities.
///
/// Constructed via [`LspCapabilitySnapshot::from_capabilities`] from the
/// `ServerCapabilities` returned by a live server, or fabricated for
/// testing. Boolean fields default to `false` (conservative). The
/// `details` field carries option-level information that a bool cannot
/// represent.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspCapabilitySnapshot {
    pub language_id: Option<String>,
    pub server_name: Option<String>,
    // Phase 4: split diagnostics into advertised push/pull.
    pub supports_push_diagnostics: bool,
    pub supports_pull_diagnostics: bool,
    // Legacy alias kept for backward compatibility — true when either
    // push or pull is advertised. New code should query the
    // `supports_*_diagnostics` flags directly.
    pub supports_diagnostics: bool,
    pub supports_document_symbols: bool,
    pub supports_workspace_symbols: bool,
    pub supports_definition: bool,
    pub supports_declaration: bool,
    pub supports_implementation: bool,
    pub supports_references: bool,
    pub supports_hover: bool,
    pub supports_document_highlight: bool,
    pub supports_completion: bool,
    pub supports_signature_help: bool,
    pub supports_rename: bool,
    pub supports_prepare_rename: bool,
    pub supports_code_actions: bool,
    pub supports_document_formatting: bool,
    pub supports_range_formatting: bool,
    pub supports_inlay_hints: bool,
    pub supports_folding_ranges: bool,
    pub supports_selection_ranges: bool,
    pub supports_document_links: bool,
    pub supports_execute_command: bool,
    pub supports_call_hierarchy: bool,
    // Phase 4: type hierarchy is no longer inferred from call hierarchy.
    // `lsp-types` 0.97 only models type hierarchy as a CLIENT capability,
    // so the server-side advertised state defaults to `false` unless a
    // profile override flips it on (see `observed_capabilities`).
    pub supports_type_hierarchy: bool,
    pub supports_semantic_tokens: bool,
    /// Option-level details preserved where a bool is insufficient.
    #[serde(default)]
    pub details: LspCapabilityDetails,
}

/// Option-level information that a single bool cannot capture.
///
/// This is intentionally compact — full `ServerCapabilities` payloads
/// are never exposed to model-facing surfaces.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspCapabilityDetails {
    /// Server can compute prepare-rename for arbitrary positions
    /// (vs. only for symbol positions).
    pub rename_prepare_provider: bool,
    /// Code action kinds the server advertises. Empty when the server
    /// uses a boolean `code_action_provider` with no kinds.
    pub code_action_kinds: Vec<String>,
    /// Completion trigger characters advertised by the server.
    pub completion_trigger_characters: Vec<String>,
    /// Signature help trigger characters advertised by the server.
    pub signature_trigger_characters: Vec<String>,
    /// Semantic-token legend (token types + modifiers) when advertised.
    pub semantic_token_legend: Option<SemanticTokenLegendSnapshot>,
}

/// Compact representation of the semantic-token legend advertised by
/// a server. We never expose the full `SemanticTokensLegend` type to
/// the agent surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticTokenLegendSnapshot {
    pub token_types: Vec<String>,
    pub token_modifiers: Vec<String>,
}

/// Semantic operation that a caller wants to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LspSemanticOperation {
    Diagnostics,
    DocumentSymbols,
    WorkspaceSymbols,
    Definition,
    Declaration,
    Implementation,
    References,
    Hover,
    DocumentHighlight,
    Completion,
    SignatureHelp,
    Rename,
    PrepareRename,
    CodeAction,
    DocumentFormatting,
    RangeFormatting,
    InlayHints,
    FoldingRanges,
    SelectionRanges,
    DocumentLinks,
    ExecuteCommand,
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
            Self::Declaration => "declaration",
            Self::Implementation => "implementation",
            Self::References => "references",
            Self::Hover => "hover",
            Self::DocumentHighlight => "documentHighlight",
            Self::Completion => "completion",
            Self::SignatureHelp => "signatureHelp",
            Self::Rename => "rename",
            Self::PrepareRename => "prepareRename",
            Self::CodeAction => "codeAction",
            Self::DocumentFormatting => "formatting",
            Self::RangeFormatting => "rangeFormatting",
            Self::InlayHints => "inlayHint",
            Self::FoldingRanges => "foldingRange",
            Self::SelectionRanges => "selectionRange",
            Self::DocumentLinks => "documentLink",
            Self::ExecuteCommand => "executeCommand",
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

impl std::fmt::Display for LspUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.server, &self.language_id) {
            (Some(s), Some(l)) => write!(f, "{} ({}/{}) — {}", self.operation, s, l, self.reason),
            (Some(s), None) => write!(f, "{} ({}) — {}", self.operation, s, self.reason),
            (None, Some(l)) => write!(f, "{} ({}) — {}", self.operation, l, self.reason),
            (None, None) => write!(f, "{} — {}", self.operation, self.reason),
        }
    }
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

/// Profile-level override for capabilities that cannot be discovered
/// from `ServerCapabilities` alone.
///
/// Phase 4 introduces this so the tier-2 profiles (gopls, typescript-
/// language-server, clangd) can declare observed type-hierarchy
/// support without relying on the (removed) call-hierarchy heuristic.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedCapabilitiesOverride {
    /// If `Some`, override `supports_type_hierarchy`.
    pub type_hierarchy: Option<bool>,
}

impl LspCapabilitySnapshot {
    /// Derive a snapshot from live `ServerCapabilities`.
    ///
    /// `server_name` and `language_id` are caller-supplied metadata
    /// not present in the LSP protocol itself. The
    /// `override_caps` argument lets profile data flip capabilities
    /// that cannot be inferred from the protocol alone (notably
    /// type-hierarchy on `lsp-types` 0.97).
    pub fn from_capabilities(
        caps: &ServerCapabilities,
        server_name: Option<&str>,
        language_id: Option<&str>,
    ) -> Self {
        Self::from_capabilities_with_override(
            caps,
            server_name,
            language_id,
            &ObservedCapabilitiesOverride::default(),
        )
    }

    /// Same as [`Self::from_capabilities`] but accepts profile-level
    /// overrides for capabilities the protocol does not advertise on
    /// the server side.
    pub fn from_capabilities_with_override(
        caps: &ServerCapabilities,
        server_name: Option<&str>,
        language_id: Option<&str>,
        override_caps: &ObservedCapabilitiesOverride,
    ) -> Self {
        let details = extract_details(caps);

        // Diagnostics support — split into push and pull.
        // Pull is advertised via `caps.diagnostic_provider`.
        let supports_pull_diagnostics = caps.diagnostic_provider.is_some();
        // Push is implicit in the LSP spec but not advertised in
        // ServerCapabilities on lsp-types 0.97 — many servers publish
        // diagnostics without advertising. We treat push as supported
        // when a text-document-sync provider is present, which is the
        // conservative protocol-level signal.
        let supports_push_diagnostics = caps.text_document_sync.is_some();
        let supports_diagnostics = supports_push_diagnostics || supports_pull_diagnostics;

        // Declaration / implementation.
        let supports_declaration = caps.declaration_provider.is_some();
        let supports_implementation = caps.implementation_provider.is_some();

        // Document highlight — `Option<OneOf<bool, _>>`.
        let supports_document_highlight = caps.document_highlight_provider.is_some();

        // Signature help — `Option<SignatureHelpOptions>` (direct struct).
        let supports_signature_help = caps.signature_help_provider.is_some();

        // Rename + prepare rename — `Option<OneOf<bool, RenameOptions>>`.
        let supports_rename = caps.rename_provider.is_some();
        let supports_prepare_rename = caps.rename_provider.as_ref().is_some_and(|p| match p {
            lsp_types::OneOf::Left(_) => false,
            lsp_types::OneOf::Right(opts) => opts.prepare_provider.unwrap_or(false),
        });

        // Code action — `Option<CodeActionProviderCapability>` enum.
        let supports_code_actions = caps.code_action_provider.is_some();

        // Formatting — `Option<OneOf<bool, _>>`.
        let supports_document_formatting = caps.document_formatting_provider.is_some();
        let supports_range_formatting = caps.document_range_formatting_provider.is_some();

        // Type hierarchy — never inferred from call hierarchy. The
        // override is the only way to flip this on.
        let supports_type_hierarchy = override_caps.type_hierarchy.unwrap_or(false);

        Self {
            language_id: language_id.map(String::from),
            server_name: server_name.map(String::from),
            supports_push_diagnostics,
            supports_pull_diagnostics,
            supports_diagnostics,
            supports_document_symbols: caps.document_symbol_provider.is_some(),
            supports_workspace_symbols: caps.workspace_symbol_provider.is_some(),
            supports_definition: caps.definition_provider.is_some(),
            supports_declaration,
            supports_implementation,
            supports_references: caps.references_provider.is_some(),
            supports_hover: caps.hover_provider.is_some(),
            supports_document_highlight,
            supports_completion: caps.completion_provider.is_some(),
            supports_signature_help,
            supports_rename,
            supports_prepare_rename,
            supports_code_actions,
            supports_document_formatting,
            supports_range_formatting,
            supports_inlay_hints: caps.inlay_hint_provider.is_some(),
            supports_folding_ranges: caps.folding_range_provider.is_some(),
            supports_selection_ranges: caps.selection_range_provider.is_some(),
            supports_document_links: caps.document_link_provider.is_some(),
            supports_execute_command: caps.execute_command_provider.is_some(),
            supports_call_hierarchy: caps.call_hierarchy_provider.is_some(),
            supports_type_hierarchy,
            supports_semantic_tokens: caps.semantic_tokens_provider.is_some(),
            details,
        }
    }

    /// Returns `true` when the snapshot indicates the server supports `op`.
    pub fn supports(&self, op: LspSemanticOperation) -> bool {
        match op {
            LspSemanticOperation::Diagnostics => self.supports_diagnostics,
            LspSemanticOperation::DocumentSymbols => self.supports_document_symbols,
            LspSemanticOperation::WorkspaceSymbols => self.supports_workspace_symbols,
            LspSemanticOperation::Definition => self.supports_definition,
            LspSemanticOperation::Declaration => self.supports_declaration,
            LspSemanticOperation::Implementation => self.supports_implementation,
            LspSemanticOperation::References => self.supports_references,
            LspSemanticOperation::Hover => self.supports_hover,
            LspSemanticOperation::DocumentHighlight => self.supports_document_highlight,
            LspSemanticOperation::Completion => self.supports_completion,
            LspSemanticOperation::SignatureHelp => self.supports_signature_help,
            LspSemanticOperation::Rename => self.supports_rename,
            LspSemanticOperation::PrepareRename => self.supports_prepare_rename,
            LspSemanticOperation::CodeAction => self.supports_code_actions,
            LspSemanticOperation::DocumentFormatting => self.supports_document_formatting,
            LspSemanticOperation::RangeFormatting => self.supports_range_formatting,
            LspSemanticOperation::InlayHints => self.supports_inlay_hints,
            LspSemanticOperation::FoldingRanges => self.supports_folding_ranges,
            LspSemanticOperation::SelectionRanges => self.supports_selection_ranges,
            LspSemanticOperation::DocumentLinks => self.supports_document_links,
            LspSemanticOperation::ExecuteCommand => self.supports_execute_command,
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

fn extract_details(caps: &ServerCapabilities) -> LspCapabilityDetails {
    let rename_prepare_provider = caps.rename_provider.as_ref().is_some_and(|p| match p {
        lsp_types::OneOf::Left(_) => false,
        lsp_types::OneOf::Right(opts) => opts.prepare_provider.unwrap_or(false),
    });

    let code_action_kinds: Vec<String> = match caps.code_action_provider.as_ref() {
        Some(lsp_types::CodeActionProviderCapability::Options(opts)) => opts
            .code_action_kinds
            .as_ref()
            .map(|k| k.iter().map(|k| k.as_str().to_string()).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    let completion_trigger_characters: Vec<String> = caps
        .completion_provider
        .as_ref()
        .and_then(|opts| opts.trigger_characters.clone())
        .unwrap_or_default();

    let signature_trigger_characters: Vec<String> = caps
        .signature_help_provider
        .as_ref()
        .and_then(|opts| opts.trigger_characters.clone())
        .unwrap_or_default();

    let semantic_token_legend = caps.semantic_tokens_provider.as_ref().map(|p| {
        let legend = match p {
            lsp_types::SemanticTokensServerCapabilities::SemanticTokensOptions(opts) => {
                &opts.legend
            }
            lsp_types::SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(
                opts,
            ) => &opts.semantic_tokens_options.legend,
        };
        SemanticTokenLegendSnapshot {
            token_types: legend
                .token_types
                .iter()
                .map(|t| t.as_str().to_string())
                .collect(),
            token_modifiers: legend
                .token_modifiers
                .iter()
                .map(|m| m.as_str().to_string())
                .collect(),
        }
    });

    LspCapabilityDetails {
        rename_prepare_provider,
        code_action_kinds,
        completion_trigger_characters,
        signature_trigger_characters,
        semantic_token_legend,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{
        CodeActionOptions, CompletionOptions, RenameOptions, SemanticTokensLegend,
        SignatureHelpOptions,
    };

    fn sample_snapshot() -> LspCapabilitySnapshot {
        LspCapabilitySnapshot {
            language_id: Some("rust".into()),
            server_name: Some("rust-analyzer".into()),
            supports_push_diagnostics: true,
            supports_pull_diagnostics: false,
            supports_diagnostics: true,
            supports_document_symbols: true,
            supports_workspace_symbols: true,
            supports_definition: true,
            supports_declaration: false,
            supports_implementation: false,
            supports_references: true,
            supports_hover: true,
            supports_document_highlight: true,
            supports_completion: true,
            supports_signature_help: true,
            supports_rename: true,
            supports_prepare_rename: false,
            supports_code_actions: true,
            supports_document_formatting: true,
            supports_range_formatting: true,
            supports_inlay_hints: true,
            supports_folding_ranges: true,
            supports_selection_ranges: true,
            supports_document_links: true,
            supports_execute_command: true,
            supports_call_hierarchy: true,
            supports_type_hierarchy: false,
            supports_semantic_tokens: true,
            details: LspCapabilityDetails::default(),
        }
    }

    fn minimal_snapshot() -> LspCapabilitySnapshot {
        LspCapabilitySnapshot {
            language_id: Some("python".into()),
            server_name: Some("pylsp".into()),
            supports_push_diagnostics: true,
            supports_pull_diagnostics: false,
            supports_diagnostics: true,
            supports_document_symbols: true,
            supports_workspace_symbols: false,
            supports_definition: true,
            supports_declaration: false,
            supports_implementation: false,
            supports_references: true,
            supports_hover: true,
            supports_document_highlight: false,
            supports_completion: false,
            supports_signature_help: false,
            supports_rename: false,
            supports_prepare_rename: false,
            supports_code_actions: false,
            supports_document_formatting: false,
            supports_range_formatting: false,
            supports_inlay_hints: false,
            supports_folding_ranges: false,
            supports_selection_ranges: false,
            supports_document_links: false,
            supports_execute_command: false,
            supports_call_hierarchy: false,
            supports_type_hierarchy: false,
            supports_semantic_tokens: false,
            details: LspCapabilityDetails::default(),
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
        assert!(s.supports(LspSemanticOperation::DocumentHighlight));
        assert!(s.supports(LspSemanticOperation::Completion));
        assert!(s.supports(LspSemanticOperation::SignatureHelp));
        assert!(s.supports(LspSemanticOperation::Rename));
        assert!(s.supports(LspSemanticOperation::CodeAction));
        assert!(s.supports(LspSemanticOperation::DocumentFormatting));
        assert!(s.supports(LspSemanticOperation::RangeFormatting));
        assert!(s.supports(LspSemanticOperation::CallHierarchy));
        assert!(!s.supports(LspSemanticOperation::TypeHierarchy));
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
        assert!(!snap.supports(LspSemanticOperation::DocumentHighlight));
        assert!(!snap.supports(LspSemanticOperation::Implementation));
        assert!(!snap.supports(LspSemanticOperation::Declaration));
        // Type hierarchy must NOT be inferred from call hierarchy.
        assert!(!snap.supports(LspSemanticOperation::TypeHierarchy));
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

    // ── Phase 4 normalization tests ─────────────────────────────────

    #[test]
    fn type_hierarchy_not_inferred_from_call_hierarchy() {
        // ServerCapabilities with call_hierarchy_provider but no
        // observed override → supports_type_hierarchy must be false.
        let mut caps = ServerCapabilities::default();
        caps.call_hierarchy_provider = Some(lsp_types::CallHierarchyServerCapability::Simple(true));
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("s"), Some("rust"));
        assert!(snap.supports_call_hierarchy);
        assert!(
            !snap.supports_type_hierarchy,
            "type hierarchy must NOT be inferred from call hierarchy"
        );
    }

    #[test]
    fn type_hierarchy_override_flips_default() {
        let mut caps = ServerCapabilities::default();
        caps.call_hierarchy_provider = Some(lsp_types::CallHierarchyServerCapability::Simple(true));
        let override_caps = ObservedCapabilitiesOverride {
            type_hierarchy: Some(true),
        };
        let snap = LspCapabilitySnapshot::from_capabilities_with_override(
            &caps,
            Some("s"),
            Some("go"),
            &override_caps,
        );
        assert!(snap.supports_type_hierarchy);
    }

    #[test]
    fn rename_with_prepare_provider_records_capability() {
        let mut caps = ServerCapabilities::default();
        caps.rename_provider = Some(lsp_types::OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        }));
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("s"), Some("rust"));
        assert!(snap.supports_rename);
        assert!(snap.supports_prepare_rename);
        assert!(snap.details.rename_prepare_provider);
    }

    #[test]
    fn rename_without_prepare_provider_leaves_flag_false() {
        let mut caps = ServerCapabilities::default();
        caps.rename_provider = Some(lsp_types::OneOf::Right(RenameOptions {
            prepare_provider: Some(false),
            work_done_progress_options: Default::default(),
        }));
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("s"), Some("rust"));
        assert!(snap.supports_rename);
        assert!(!snap.supports_prepare_rename);
    }

    #[test]
    fn code_action_kinds_preserved_in_details() {
        let mut caps = ServerCapabilities::default();
        caps.code_action_provider = Some(lsp_types::CodeActionProviderCapability::Options(
            CodeActionOptions {
                code_action_kinds: Some(vec![
                    lsp_types::CodeActionKind::QUICKFIX,
                    lsp_types::CodeActionKind::REFACTOR_EXTRACT,
                ]),
                work_done_progress_options: Default::default(),
                resolve_provider: Some(true),
            },
        ));
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("s"), Some("ts"));
        assert!(snap.supports_code_actions);
        let kinds = &snap.details.code_action_kinds;
        assert!(kinds.contains(&"quickfix".to_string()));
        assert!(kinds.contains(&"refactor.extract".to_string()));
    }

    #[test]
    fn completion_trigger_characters_preserved() {
        let mut caps = ServerCapabilities::default();
        caps.completion_provider = Some(lsp_types::CompletionOptions {
            trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
            ..CompletionOptions::default()
        });
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("s"), Some("rust"));
        assert!(snap.supports_completion);
        assert_eq!(
            snap.details.completion_trigger_characters,
            vec![".".to_string(), ":".to_string()]
        );
    }

    #[test]
    fn signature_help_trigger_characters_preserved() {
        let mut caps = ServerCapabilities::default();
        caps.signature_help_provider = Some(lsp_types::SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
            retrigger_characters: None,
            work_done_progress_options: Default::default(),
        });
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("s"), Some("ts"));
        assert!(snap.supports_signature_help);
        assert_eq!(
            snap.details.signature_trigger_characters,
            vec!["(".to_string(), ",".to_string()]
        );
    }

    #[test]
    fn semantic_token_legend_extracted() {
        let mut caps = ServerCapabilities::default();
        caps.semantic_tokens_provider = Some(
            lsp_types::SemanticTokensServerCapabilities::SemanticTokensOptions(
                lsp_types::SemanticTokensOptions {
                    legend: SemanticTokensLegend {
                        token_types: vec![lsp_types::SemanticTokenType::FUNCTION],
                        token_modifiers: vec![lsp_types::SemanticTokenModifier::DECLARATION],
                    },
                    range: Some(false),
                    full: Some(lsp_types::SemanticTokensFullOptions::Bool(true)),
                    work_done_progress_options: Default::default(),
                },
            ),
        );
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("s"), Some("rust"));
        assert!(snap.supports_semantic_tokens);
        let legend = snap.details.semantic_token_legend.expect("legend present");
        assert!(legend.token_types.contains(&"function".to_string()));
        assert!(legend.token_modifiers.contains(&"declaration".to_string()));
    }

    #[test]
    fn document_and_range_formatting_are_distinct() {
        let mut caps = ServerCapabilities::default();
        caps.document_formatting_provider = Some(lsp_types::OneOf::Left(true));
        // range formatting NOT set
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("s"), Some("rust"));
        assert!(snap.supports_document_formatting);
        assert!(!snap.supports_range_formatting);

        caps.document_range_formatting_provider = Some(lsp_types::OneOf::Left(true));
        let snap2 = LspCapabilitySnapshot::from_capabilities(&caps, Some("s"), Some("rust"));
        assert!(snap2.supports_document_formatting);
        assert!(snap2.supports_range_formatting);
    }

    #[test]
    fn diagnostics_push_and_pull_are_distinct() {
        // No providers → both false.
        let caps = ServerCapabilities::default();
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("s"), Some("rust"));
        assert!(!snap.supports_push_diagnostics);
        assert!(!snap.supports_pull_diagnostics);
        assert!(!snap.supports_diagnostics);

        // diagnostic_provider → pull true.
        let mut caps = ServerCapabilities::default();
        caps.diagnostic_provider = Some(lsp_types::DiagnosticServerCapabilities::Options(
            lsp_types::DiagnosticOptions {
                identifier: None,
                inter_file_dependencies: false,
                workspace_diagnostics: false,
                work_done_progress_options: Default::default(),
            },
        ));
        let snap2 = LspCapabilitySnapshot::from_capabilities(&caps, Some("s"), Some("rust"));
        assert!(!snap2.supports_push_diagnostics);
        assert!(snap2.supports_pull_diagnostics);
        assert!(snap2.supports_diagnostics);
    }

    #[test]
    fn absent_providers_default_to_false() {
        // All-None ServerCapabilities → every normalized boolean is
        // false except SecurityContext (composite) and the legacy
        // supports_diagnostics alias (false when nothing is advertised).
        let caps = ServerCapabilities::default();
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, None, None);
        for op in [
            LspSemanticOperation::Diagnostics,
            LspSemanticOperation::DocumentSymbols,
            LspSemanticOperation::WorkspaceSymbols,
            LspSemanticOperation::Definition,
            LspSemanticOperation::Declaration,
            LspSemanticOperation::Implementation,
            LspSemanticOperation::References,
            LspSemanticOperation::Hover,
            LspSemanticOperation::DocumentHighlight,
            LspSemanticOperation::Completion,
            LspSemanticOperation::SignatureHelp,
            LspSemanticOperation::Rename,
            LspSemanticOperation::PrepareRename,
            LspSemanticOperation::CodeAction,
            LspSemanticOperation::DocumentFormatting,
            LspSemanticOperation::RangeFormatting,
            LspSemanticOperation::InlayHints,
            LspSemanticOperation::FoldingRanges,
            LspSemanticOperation::SelectionRanges,
            LspSemanticOperation::DocumentLinks,
            LspSemanticOperation::ExecuteCommand,
            LspSemanticOperation::CallHierarchy,
            LspSemanticOperation::TypeHierarchy,
            LspSemanticOperation::SemanticTokens,
        ] {
            assert!(!snap.supports(op), "operation {op:?} should be false");
        }
        // Composite SecurityContext stays available.
        assert!(snap.supports(LspSemanticOperation::SecurityContext));
    }

    #[test]
    fn new_operations_advertised_from_capabilities() {
        // ServerCapabilities with declaration + implementation + highlight.
        let mut caps = ServerCapabilities::default();
        caps.declaration_provider = Some(lsp_types::DeclarationCapability::Simple(true));
        caps.implementation_provider =
            Some(lsp_types::ImplementationProviderCapability::Simple(true));
        caps.document_highlight_provider = Some(lsp_types::OneOf::Left(true));
        caps.inlay_hint_provider = Some(lsp_types::OneOf::Left(true));
        caps.folding_range_provider = Some(lsp_types::FoldingRangeProviderCapability::Simple(true));
        caps.selection_range_provider =
            Some(lsp_types::SelectionRangeProviderCapability::Simple(true));
        caps.document_link_provider = Some(lsp_types::DocumentLinkOptions {
            resolve_provider: None,
            work_done_progress_options: Default::default(),
        });
        caps.execute_command_provider = Some(lsp_types::ExecuteCommandOptions::default());
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("s"), Some("rust"));
        assert!(snap.supports_declaration);
        assert!(snap.supports_implementation);
        assert!(snap.supports_document_highlight);
        assert!(snap.supports_inlay_hints);
        assert!(snap.supports_folding_ranges);
        assert!(snap.supports_selection_ranges);
        assert!(snap.supports_document_links);
        assert!(snap.supports_execute_command);
    }
}
