//! Production adapter that implements [`LspEvidenceProvider`] over
//! [`LspService`] and [`LspOperations`].
//!
//! # Purpose
//!
//! [`LspEvidenceProvider`] is intentionally a tuple-returning trait
//! so the collector can be exercised with lightweight mocks. The
//! production path, however, needs to surface real per-call
//! provenance — server_id, server_generation, capability decision,
//! freshness, post-restart flag — so downstream renderers can
//! distinguish stale evidence from fresh.
//!
//! `ServiceLspEvidenceProvider` bridges that gap:
//!
//! - It implements [`LspEvidenceProvider`] using the typed
//!   [`LspOperations`] and the [`DiagnosticsCollector`] for live
//!   data.
//! - It records per-call capability decisions and exposes them via
//!   [`ServiceLspEvidenceProvider::last_provenance`] so callers can
//!   enrich the items the collector produces.
//!
//! # Sequential side-channel contract (Pass 3 Phase 5)
//!
//! The per-call provenance slot (`last_provenance`) is a **single
//! shared `Option<LspEvidenceProvenance>` behind a `Mutex`**. It is
//! NOT thread-safe under concurrent calls on the same adapter
//! instance — the second concurrent call would clobber the first
//! caller's provenance before that caller could observe it.
//!
//! Therefore the adapter requires a **sequential call contract**:
//!
//! 1. Each `LspEvidenceProvider` trait method invocation must
//!    complete (return to the caller) before the next method on the
//!    same adapter instance is invoked.
//! 2. Callers that need to observe the provenance for a given call
//!    MUST consume it via
//!    [`ServiceLspEvidenceProvider::consume_provenance_for`] **before
//!    the next provider call**. This atomically takes the slot and
//!    validates that the recorded operation matches the caller's
//!    expectation (a `debug_assert!` in debug builds; a documented
//!    no-op match in release builds — the slot is still cleared).
//! 3. No `tokio::join!`, `futures::join!`, or `tokio::spawn` may
//!    invoke two provider methods on the same adapter concurrently.
//!    Each `collect_context` / `collect_hunk_context` invocation in
//!    [`crate::evidence_collector`] already obeys this contract —
//!    every provider call is `.await`ed sequentially.
//!
//! The contract is enforced by:
//!
//! - [`ServiceLspEvidenceProvider::consume_provenance_for`] — the
//!   guarded accessor that takes the slot AND validates the
//!   operation tag, panicking on mismatch in debug builds.
//! - The doc-pinned [`crate::evidence_collector::LspEvidenceProvider`]
//!   contract that mirrors the requirement.
//!
//! Future maintainers: if you need to parallelize provider calls,
//! add a typed DTO trait (Pass 3 Option A) that carries provenance
//! alongside the data — do NOT try to make the side-channel
//! thread-safe.
//!
//! # Capability gating
//!
//! Every read-only adapter method consults
//! `LspService::capability_decision` for the relevant
//! [`LspSemanticOperation`] before issuing the request:
//!
//! - **Supported** — issue the request normally.
//! - **Unsupported** — return `LspError::Unavailable` with a
//!   structured reason and stamp `last_provenance` with
//!   `capability_decision = "unsupported"`.
//! - **Unknown** (client still initializing) — issue the request
//!   for backward compatibility; stamp
//!   `capability_decision = "unknown"`.
//!
//! The adapter never invokes `workspace/executeCommand` or any
//! mutation-producing operation. `textDocument/rename`,
//! `textDocument/formatting`, and `textDocument/codeAction` are
//! not part of the trait surface.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use url::Url;

use crate::capability::{CapabilityDecision, LspSemanticOperation, LspUnavailable};
use crate::context::{LspEvidenceFreshness, LspEvidenceProvenance};
use crate::diagnostics::{DiagnosticsCollector, LspDiagnosticFreshness};
use crate::error::LspError;
use crate::evidence_collector::LspEvidenceProvider;
use crate::operations::LspOperations;
use crate::service::LspService;

/// The kind of LSP operation an evidence adapter call was attempting.
///
/// Persisted on [`LspEvidenceProvenance::operation`] so collectors
/// and renderers can attribute every line of context to a specific
/// LSP request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvidenceOperation {
    Diagnostics,
    DocumentSymbols,
    GoToDefinition,
    FindReferences,
    Implementations,
    Hover,
    DocumentHighlights,
    SignatureHelp,
    Completion,
    SemanticTokens,
    WorkspaceSymbols,
}

impl EvidenceOperation {
    /// Stable LSP method name for this operation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Diagnostics => "textDocument/diagnostic",
            Self::DocumentSymbols => "textDocument/documentSymbol",
            Self::GoToDefinition => "textDocument/definition",
            Self::FindReferences => "textDocument/references",
            Self::Implementations => "textDocument/implementation",
            Self::Hover => "textDocument/hover",
            Self::DocumentHighlights => "textDocument/documentHighlight",
            Self::SignatureHelp => "textDocument/signatureHelp",
            Self::Completion => "textDocument/completion",
            Self::SemanticTokens => "textDocument/semanticTokens/full",
            Self::WorkspaceSymbols => "workspace/symbol",
        }
    }
}

/// Production adapter that drives [`LspEvidenceProvider`] from a live
/// [`LspService`].
///
/// Cheap to construct — `Arc<LspService>` is the only external
/// state. The adapter records per-call provenance in
/// [`last_provenance`] so callers (typically the collector
/// orchestrator) can enrich collected items with capability
/// decisions and freshness metadata without modifying the existing
/// tuple-shaped trait.
#[derive(Clone)]
pub struct ServiceLspEvidenceProvider {
    service: Arc<LspService>,
    operations: Arc<LspOperations>,
    diagnostics: Arc<DiagnosticsCollector>,
    allowed_root: PathBuf,
    last_provenance: Arc<Mutex<Option<LspEvidenceProvenance>>>,
}

impl std::fmt::Debug for ServiceLspEvidenceProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceLspEvidenceProvider")
            .field("service", &Arc::as_ptr(&self.service))
            .field("allowed_root", &self.allowed_root)
            .field(
                "last_provenance",
                &self.last_provenance.lock().ok().map(|p| p.clone()),
            )
            .finish()
    }
}

impl ServiceLspEvidenceProvider {
    /// Build a new adapter bound to the given LSP service and root.
    pub fn new(service: Arc<LspService>, allowed_root: PathBuf) -> Self {
        let operations = Arc::new(LspOperations::new(Arc::clone(&service)));
        let diagnostics = Arc::new(DiagnosticsCollector::new(Arc::clone(&service)));
        Self {
            service,
            operations,
            diagnostics,
            allowed_root,
            last_provenance: Arc::new(Mutex::new(None)),
        }
    }

    /// Underlying [`LspService`] — exposed so callers can read
    /// server info, generation, and capability decisions directly.
    pub fn service(&self) -> &Arc<LspService> {
        &self.service
    }

    /// Allowed root for path validation. Read-only callers may
    /// apply stricter validation when consuming locations.
    pub fn allowed_root(&self) -> &Path {
        &self.allowed_root
    }

    /// Consume the per-call provenance metadata set by the most
    /// recent adapter call.
    ///
    /// Each trait method call overwrites this slot with the
    /// provenance it observed (server_id, generation, capability
    /// decision, freshness). After the collector dispatches a call
    /// it can call `take_provenance()` to learn what the adapter
    /// saw and stamp it onto the produced items.
    ///
    /// **Sequential contract**: see the module-level doc comment.
    /// This takes the slot without validating the operation tag;
    /// prefer [`consume_provenance_for`](Self::consume_provenance_for)
    /// in call-site code where the expected operation is known.
    pub fn take_provenance(&self) -> Option<LspEvidenceProvenance> {
        self.last_provenance.lock().ok().and_then(|mut g| g.take())
    }

    /// Borrow the per-call provenance without consuming it.
    pub fn last_provenance(&self) -> Option<LspEvidenceProvenance> {
        self.last_provenance.lock().ok().and_then(|g| g.clone())
    }

    /// Atomically consume the per-call provenance **and validate**
    /// that the recorded operation matches `expected_operation`.
    ///
    /// This is the guarded accessor that implements Pass 3 Phase 5's
    /// sequential side-channel contract:
    ///
    /// - On a match, the slot is cleared and the provenance is
    ///   returned.
    /// - On a mismatch, the slot is **still** cleared (so a stale
    ///   entry cannot leak to a subsequent caller) but the mismatch
    ///   is surfaced: in debug builds via `debug_assert!`, and
    ///   always via a `tracing::warn!` so release builds surface
    ///   the regression in logs.
    /// - On an empty slot (no provenance recorded yet, or already
    ///   consumed), `None` is returned and no assertion fires.
    ///
    /// Call-site code should prefer this over
    /// [`take_provenance`](Self::take_provenance) whenever the
    /// call site knows which LSP operation it just dispatched — i.e.
    /// almost always.
    pub fn consume_provenance_for(
        &self,
        expected_operation: &str,
    ) -> Option<LspEvidenceProvenance> {
        let taken = self.take_provenance();
        if let Some(ref prov) = taken {
            if prov.operation != expected_operation {
                debug_assert_eq!(
                    prov.operation,
                    expected_operation,
                    "ServiceLspEvidenceProvider provenance mismatch: slot recorded `{}` but call site expected `{}`. \
                     This usually indicates a concurrent provider call clobbered the slot — see the module-level \
                     sequential side-channel contract.",
                    prov.operation,
                    expected_operation
                );
                tracing::warn!(
                    recorded = %prov.operation,
                    expected = %expected_operation,
                    "ServiceLspEvidenceProvider provenance operation mismatch; sequential contract violated"
                );
            }
        }
        taken
    }

    /// Asynchronously look up `(server_id, generation)` for the
    /// first known client.
    async fn server_info_async(&self) -> (Option<String>, Option<u64>) {
        let keys = self.service.client_keys().await;
        for key in &keys {
            let gen = self.service.generation_for_key(key).await;
            if gen > 0 {
                return (Some(key.clone()), Some(gen));
            }
        }
        (None, None)
    }

    /// Asynchronously look up the operational state label for the
    /// first known client.
    async fn operational_state_label_async(&self) -> Option<String> {
        let keys = self.service.client_keys().await;
        for key in &keys {
            if let Some(state) = self.service.operational_state_for_key(key).await {
                return Some(state.label().to_string());
            }
        }
        None
    }

    /// Build an `LspUnavailable` describing why `op` is unavailable.
    async fn unavailable(&self, op: LspSemanticOperation, reason: &str) -> LspError {
        let (server, _) = self.server_info_async().await;
        let mut u = LspUnavailable::new(op, reason);
        if let Some(s) = server {
            u = u.with_server(s);
        }
        LspError::Unavailable(u)
    }

    /// Look up the capability decision for `file` and `op`.
    async fn capability_for(&self, file: &Path, op: LspSemanticOperation) -> CapabilityDecision {
        match self.service.get_or_create_client(file).await {
            Ok((key, _)) => self.service.capability_decision(&key, op).await,
            Err(_) => CapabilityDecision::Unknown {
                operation: op,
                reason: "no client".to_string(),
            },
        }
    }

    /// Build and store per-call provenance.
    async fn record_provenance_async(
        &self,
        op: EvidenceOperation,
        capability_decision: CapabilityDecision,
    ) {
        let (server_id, generation) = self.server_info_async().await;
        let state_label = self.operational_state_label_async().await;
        let freshness = match state_label.as_deref() {
            Some("ready") => LspEvidenceFreshness::Fresh,
            Some("indexing") | Some("degraded") => LspEvidenceFreshness::PossiblyStale,
            Some("initializing") | Some("starting") => LspEvidenceFreshness::Unknown,
            _ => LspEvidenceFreshness::Stale,
        };
        let capability_str = match &capability_decision {
            CapabilityDecision::Supported => Some("supported".to_string()),
            CapabilityDecision::Unsupported(_) => Some("unsupported".to_string()),
            CapabilityDecision::Unknown { .. } => Some("unknown".to_string()),
        };
        let prov = LspEvidenceProvenance {
            server_id: server_id.clone().unwrap_or_else(|| "unknown".to_string()),
            server_generation: generation,
            operation: op.as_str().to_string(),
            freshness,
            capability_decision: capability_str,
            document_version: None,
            age_ms: None,
            post_restart: generation.map_or(false, |g| g > 1),
        };
        if let Ok(mut slot) = self.last_provenance.lock() {
            *slot = Some(prov);
        }
    }

    /// Workspace-scoped capability decision — picks the first known
    /// client key.
    async fn workspace_capability_for(&self, op: LspSemanticOperation) -> CapabilityDecision {
        let keys = self.service.client_keys().await;
        if let Some(key) = keys.first() {
            self.service.capability_decision(key, op).await
        } else {
            CapabilityDecision::Unknown {
                operation: op,
                reason: "no client".to_string(),
            }
        }
    }
}

#[async_trait]
impl LspEvidenceProvider for ServiceLspEvidenceProvider {
    async fn diagnostics_for_file(
        &self,
        file: &Path,
    ) -> Result<Vec<(String, String, String)>, LspError> {
        let result = self
            .diagnostics
            .get_diagnostic_snapshot_for_file(file)
            .await;
        match &result {
            Ok(snap) => {
                let decision = match snap.freshness {
                    LspDiagnosticFreshness::Unavailable => CapabilityDecision::Unknown {
                        operation: LspSemanticOperation::Diagnostics,
                        reason: "diagnostics unavailable".to_string(),
                    },
                    _ => CapabilityDecision::Supported,
                };
                self.record_provenance_async(EvidenceOperation::Diagnostics, decision)
                    .await;
            }
            Err(_) => {
                self.record_provenance_async(
                    EvidenceOperation::Diagnostics,
                    CapabilityDecision::Unknown {
                        operation: LspSemanticOperation::Diagnostics,
                        reason: "diagnostics fetch failed".to_string(),
                    },
                )
                .await;
            }
        }
        let snap = result?;
        Ok(snap
            .diagnostics
            .into_iter()
            .map(|d| {
                let severity = format!("{:?}", d.severity).to_lowercase();
                let range = format!(
                    "({}:{})-({}:{})",
                    d.line + 1,
                    d.column + 1,
                    d.line + 1,
                    d.column + 1
                );
                (severity, d.message, range)
            })
            .collect())
    }

    async fn document_symbols(
        &self,
        file: &Path,
    ) -> Result<Vec<(String, String, String)>, LspError> {
        let decision = self
            .capability_for(file, LspSemanticOperation::DocumentSymbols)
            .await;
        self.record_provenance_async(EvidenceOperation::DocumentSymbols, decision.clone())
            .await;
        if matches!(decision, CapabilityDecision::Unsupported(_)) {
            return Err(self
                .unavailable(
                    LspSemanticOperation::DocumentSymbols,
                    "server does not advertise documentSymbolProvider",
                )
                .await);
        }
        let symbols = self.operations.document_symbols(file).await?;
        Ok(symbols
            .into_iter()
            .map(|s| {
                (
                    s.name,
                    format!("{:?}", s.kind),
                    format!(
                        "({}:{})-({}:{})",
                        s.range.start.line + 1,
                        s.range.start.character + 1,
                        s.range.end.line + 1,
                        s.range.end.character + 1
                    ),
                )
            })
            .collect())
    }

    async fn go_to_definition(
        &self,
        file: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<(String, String)>, LspError> {
        let decision = self
            .capability_for(file, LspSemanticOperation::Definition)
            .await;
        self.record_provenance_async(EvidenceOperation::GoToDefinition, decision.clone())
            .await;
        if matches!(decision, CapabilityDecision::Unsupported(_)) {
            return Err(self
                .unavailable(
                    LspSemanticOperation::Definition,
                    "server does not advertise definitionProvider",
                )
                .await);
        }
        let links = self.operations.go_to_definition(file, line, column).await?;
        Ok(links
            .into_iter()
            .map(|l| {
                let path = Url::parse(&l.target_uri.to_string())
                    .ok()
                    .and_then(|u| u.to_file_path().ok())
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| l.target_uri.to_string());
                (
                    path,
                    format!(
                        "({}:{})-({}:{})",
                        l.target_range.start.line + 1,
                        l.target_range.start.character + 1,
                        l.target_range.end.line + 1,
                        l.target_range.end.character + 1
                    ),
                )
            })
            .collect())
    }

    async fn find_references(
        &self,
        file: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<(String, String)>, LspError> {
        let decision = self
            .capability_for(file, LspSemanticOperation::References)
            .await;
        self.record_provenance_async(EvidenceOperation::FindReferences, decision.clone())
            .await;
        if matches!(decision, CapabilityDecision::Unsupported(_)) {
            return Err(self
                .unavailable(
                    LspSemanticOperation::References,
                    "server does not advertise referencesProvider",
                )
                .await);
        }
        let refs = self.operations.find_references(file, line, column).await?;
        Ok(refs
            .into_iter()
            .map(|l| {
                let path = Url::parse(&l.uri.to_string())
                    .ok()
                    .and_then(|u| u.to_file_path().ok())
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| l.uri.to_string());
                (
                    path,
                    format!(
                        "({}:{})-({}:{})",
                        l.range.start.line + 1,
                        l.range.start.character + 1,
                        l.range.end.line + 1,
                        l.range.end.character + 1
                    ),
                )
            })
            .collect())
    }

    async fn implementations(
        &self,
        file: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<(String, String)>, LspError> {
        let decision = self
            .capability_for(file, LspSemanticOperation::Implementation)
            .await;
        self.record_provenance_async(EvidenceOperation::Implementations, decision.clone())
            .await;
        if matches!(decision, CapabilityDecision::Unsupported(_)) {
            return Err(self
                .unavailable(
                    LspSemanticOperation::Implementation,
                    "server does not advertise implementationProvider",
                )
                .await);
        }
        let links = self.operations.implementation(file, line, column).await?;
        Ok(links
            .into_iter()
            .map(|l| {
                let path = Url::parse(&l.target_uri.to_string())
                    .ok()
                    .and_then(|u| u.to_file_path().ok())
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| l.target_uri.to_string());
                (
                    path,
                    format!(
                        "({}:{})-({}:{})",
                        l.target_range.start.line + 1,
                        l.target_range.start.character + 1,
                        l.target_range.end.line + 1,
                        l.target_range.end.character + 1
                    ),
                )
            })
            .collect())
    }

    async fn hover(&self, file: &Path, line: u32, column: u32) -> Result<Option<String>, LspError> {
        let decision = self.capability_for(file, LspSemanticOperation::Hover).await;
        self.record_provenance_async(EvidenceOperation::Hover, decision.clone())
            .await;
        if matches!(decision, CapabilityDecision::Unsupported(_)) {
            return Err(self
                .unavailable(
                    LspSemanticOperation::Hover,
                    "server does not advertise hoverProvider",
                )
                .await);
        }
        self.operations.hover(file, line, column).await
    }

    async fn document_highlights(
        &self,
        file: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<String>, LspError> {
        let decision = self
            .capability_for(file, LspSemanticOperation::DocumentHighlight)
            .await;
        self.record_provenance_async(EvidenceOperation::DocumentHighlights, decision.clone())
            .await;
        if matches!(decision, CapabilityDecision::Unsupported(_)) {
            return Err(self
                .unavailable(
                    LspSemanticOperation::DocumentHighlight,
                    "server does not advertise documentHighlightProvider",
                )
                .await);
        }
        let items = self
            .operations
            .document_highlights(file, line, column)
            .await?;
        Ok(items
            .into_iter()
            .map(|h| {
                format!(
                    "({}:{})-({}:{})",
                    h.range.start.line + 1,
                    h.range.start.character + 1,
                    h.range.end.line + 1,
                    h.range.end.character + 1
                )
            })
            .collect())
    }

    async fn signature_help(
        &self,
        file: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<(String, String)>, LspError> {
        let decision = self
            .capability_for(file, LspSemanticOperation::SignatureHelp)
            .await;
        self.record_provenance_async(EvidenceOperation::SignatureHelp, decision.clone())
            .await;
        if matches!(decision, CapabilityDecision::Unsupported(_)) {
            return Err(self
                .unavailable(
                    LspSemanticOperation::SignatureHelp,
                    "server does not advertise signatureHelpProvider",
                )
                .await);
        }
        let summary = self
            .operations
            .signature_help_typed(file, line, column)
            .await?;
        let Some(summary) = summary else {
            return Ok(Vec::new());
        };
        Ok(summary
            .signatures
            .into_iter()
            .map(|sig| {
                let params = sig
                    .parameters
                    .iter()
                    .map(|p| {
                        let label = match (p.label.is_empty(), p.documentation.as_deref()) {
                            (false, Some(d)) => format!("{} — {}", p.label, d),
                            (false, None) => p.label.clone(),
                            (true, Some(d)) => d.to_string(),
                            (true, None) => String::new(),
                        };
                        if label.is_empty() {
                            "?".to_string()
                        } else {
                            label
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                (sig.label, params)
            })
            .collect())
    }

    async fn completion(
        &self,
        file: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<(String, String, String)>, LspError> {
        let decision = self
            .capability_for(file, LspSemanticOperation::Completion)
            .await;
        self.record_provenance_async(EvidenceOperation::Completion, decision.clone())
            .await;
        if matches!(decision, CapabilityDecision::Unsupported(_)) {
            return Err(self
                .unavailable(
                    LspSemanticOperation::Completion,
                    "server does not advertise completionProvider",
                )
                .await);
        }
        let candidates = self
            .operations
            .completion_bounded(file, line, column, None, None, 64)
            .await?;
        Ok(candidates
            .into_iter()
            .map(|c| {
                (
                    c.label,
                    c.kind.unwrap_or_default(),
                    c.detail.unwrap_or_default(),
                )
            })
            .collect())
    }

    async fn semantic_tokens(
        &self,
        file: &Path,
        start_line: u32,
        end_line: u32,
    ) -> Result<Vec<(u32, u32, u32, String)>, LspError> {
        let decision = self
            .capability_for(file, LspSemanticOperation::SemanticTokens)
            .await;
        self.record_provenance_async(EvidenceOperation::SemanticTokens, decision.clone())
            .await;
        if matches!(decision, CapabilityDecision::Unsupported(_)) {
            return Err(self
                .unavailable(
                    LspSemanticOperation::SemanticTokens,
                    "server does not advertise semanticTokensProvider",
                )
                .await);
        }
        let tokens = self.operations.semantic_tokens(file, 200).await?;
        Ok(tokens
            .into_iter()
            .filter(|t| t.line >= start_line && t.line <= end_line)
            .map(|t| (t.line, t.start, t.length, t.token_type))
            .collect())
    }

    async fn workspace_symbols(
        &self,
        query: &str,
    ) -> Result<Vec<(String, String, String, String)>, LspError> {
        let decision = self
            .workspace_capability_for(LspSemanticOperation::WorkspaceSymbols)
            .await;
        self.record_provenance_async(EvidenceOperation::WorkspaceSymbols, decision.clone())
            .await;
        if matches!(
            decision,
            CapabilityDecision::Unsupported(_) | CapabilityDecision::Unknown { .. }
        ) {
            return Err(self
                .unavailable(
                    LspSemanticOperation::WorkspaceSymbols,
                    "server does not advertise workspaceSymbolProvider or no client",
                )
                .await);
        }
        let symbols = self.operations.workspace_symbols(query).await?;
        Ok(symbols
            .into_iter()
            .map(|s| {
                let path = Url::parse(&s.location.uri.to_string())
                    .ok()
                    .and_then(|u| u.to_file_path().ok())
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| s.location.uri.to_string());
                (
                    s.name,
                    format!("{:?}", s.kind),
                    path,
                    format!(
                        "({}:{})-({}:{})",
                        s.location.range.start.line + 1,
                        s.location.range.start.character + 1,
                        s.location.range.end.line + 1,
                        s.location.range.end.character + 1
                    ),
                )
            })
            .collect())
    }

    async fn operational_state(&self) -> String {
        match self.operational_state_label_async().await {
            Some(label) => label,
            None => "unknown".to_string(),
        }
    }

    async fn server_info(&self) -> (Option<String>, Option<u64>) {
        self.server_info_async().await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_operation_strings_match_lsp_methods() {
        assert_eq!(
            EvidenceOperation::Diagnostics.as_str(),
            "textDocument/diagnostic"
        );
        assert_eq!(
            EvidenceOperation::DocumentSymbols.as_str(),
            "textDocument/documentSymbol"
        );
        assert_eq!(
            EvidenceOperation::GoToDefinition.as_str(),
            "textDocument/definition"
        );
        assert_eq!(
            EvidenceOperation::FindReferences.as_str(),
            "textDocument/references"
        );
        assert_eq!(
            EvidenceOperation::Implementations.as_str(),
            "textDocument/implementation"
        );
        assert_eq!(EvidenceOperation::Hover.as_str(), "textDocument/hover");
        assert_eq!(
            EvidenceOperation::DocumentHighlights.as_str(),
            "textDocument/documentHighlight"
        );
        assert_eq!(
            EvidenceOperation::SignatureHelp.as_str(),
            "textDocument/signatureHelp"
        );
        assert_eq!(
            EvidenceOperation::Completion.as_str(),
            "textDocument/completion"
        );
        assert_eq!(
            EvidenceOperation::SemanticTokens.as_str(),
            "textDocument/semanticTokens/full"
        );
        assert_eq!(
            EvidenceOperation::WorkspaceSymbols.as_str(),
            "workspace/symbol"
        );
    }

    #[test]
    fn evidence_operation_as_str_is_static() {
        let s: &'static str = EvidenceOperation::Diagnostics.as_str();
        assert!(!s.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn take_provenance_returns_none_initially() {
        let service = LspService::new_arc(crate::config::LspConfig::default());
        let provider = ServiceLspEvidenceProvider::new(service, PathBuf::from("/tmp"));
        assert!(provider.take_provenance().is_none());
        assert!(provider.last_provenance().is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn operational_state_without_clients_is_unknown() {
        let service = LspService::new_arc(crate::config::LspConfig::default());
        let provider = ServiceLspEvidenceProvider::new(service, PathBuf::from("/tmp"));
        let state = provider.operational_state().await;
        assert_eq!(state, "unknown");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn server_info_without_clients_is_none() {
        let service = LspService::new_arc(crate::config::LspConfig::default());
        let provider = ServiceLspEvidenceProvider::new(service, PathBuf::from("/tmp"));
        let (sid, gen) = provider.server_info().await;
        assert!(sid.is_none());
        assert!(gen.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn diagnostics_with_no_clients_returns_error() {
        let service = LspService::new_arc(crate::config::LspConfig::default());
        let provider = ServiceLspEvidenceProvider::new(service, PathBuf::from("/tmp"));
        let result = provider
            .diagnostics_for_file(std::path::Path::new("/nonexistent.rs"))
            .await;
        assert!(result.is_err(), "expected error without clients");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn workspace_symbols_without_clients_returns_error() {
        let service = LspService::new_arc(crate::config::LspConfig::default());
        let provider = ServiceLspEvidenceProvider::new(service, PathBuf::from("/tmp"));
        let result = provider.workspace_symbols("foo").await;
        assert!(result.is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn adapter_records_provenance_after_diagnostics_failure() {
        let service = LspService::new_arc(crate::config::LspConfig::default());
        let provider = ServiceLspEvidenceProvider::new(service, PathBuf::from("/tmp"));
        let _ = provider
            .diagnostics_for_file(std::path::Path::new("/nonexistent.rs"))
            .await;
        // Provenance is recorded even on error paths.
        let prov = provider.last_provenance();
        assert!(prov.is_some(), "provenance should be recorded");
        let prov = prov.unwrap();
        assert_eq!(prov.operation, "textDocument/diagnostic");
        assert_eq!(prov.server_id, "unknown");
        assert!(prov.capability_decision.is_some());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn adapter_capability_unsupported_for_unconfigured_server() {
        let service = LspService::new_arc(crate::config::LspConfig::default());
        let provider = ServiceLspEvidenceProvider::new(service, PathBuf::from("/tmp"));
        // With no clients, the workspace capability decision is Unknown.
        // workspace_symbols must return Unavailable, not a panic.
        let result = provider.workspace_symbols("test").await;
        assert!(matches!(result, Err(LspError::Unavailable(_))));
        let prov = provider.last_provenance();
        assert!(prov.is_some());
        let prov = prov.unwrap();
        assert_eq!(prov.operation, "workspace/symbol");
        assert_eq!(
            prov.capability_decision.as_deref(),
            Some("unknown"),
            "unknown capability decision should be recorded when no client is configured"
        );
    }

    // -----------------------------------------------------------------------
    // Pass 3 sequential side-channel contract tests
    // -----------------------------------------------------------------------

    /// Pass 3: After each trait method call, the caller MUST immediately
    /// consume the provenance via `consume_provenance_for` before the
    /// next call. This test verifies the normal happy path:
    /// `diagnostics_for_file` records provenance, and `consume_provenance_for`
    /// atomically takes it AND validates the operation matches.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn collector_consumes_provenance_immediately_after_call() {
        let service = LspService::new_arc(crate::config::LspConfig::default());
        let provider = ServiceLspEvidenceProvider::new(service, PathBuf::from("/tmp"));

        assert!(provider.last_provenance().is_none());

        let _ = provider
            .diagnostics_for_file(std::path::Path::new("/tmp/x.rs"))
            .await;
        let prov = provider
            .consume_provenance_for("textDocument/diagnostic")
            .expect("provenance should be recorded after diagnostics call");
        assert_eq!(prov.operation, "textDocument/diagnostic");

        // Slot is now empty.
        assert!(provider.last_provenance().is_none());
        assert!(provider
            .consume_provenance_for("textDocument/diagnostic")
            .is_none());
    }

    /// Pass 3: When the call site passes the wrong `expected_operation`
    /// to `consume_provenance_for`, the slot is still cleared (so stale
    /// data doesn't leak) but a `debug_assert!` fires in debug builds and
    /// a `tracing::warn!` is emitted in release builds. This test
    /// verifies the detection mechanism: in debug builds the assert
    /// fires and the test panics (which we explicitly assert via
    /// `should_panic`); the slot is still cleared before the panic
    /// propagates so no stale state survives.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[should_panic(expected = "provenance mismatch")]
    async fn provenance_operation_mismatch_is_detected() {
        let service = LspService::new_arc(crate::config::LspConfig::default());
        let provider = ServiceLspEvidenceProvider::new(service, PathBuf::from("/tmp"));

        let _ = provider
            .diagnostics_for_file(std::path::Path::new("/tmp/x.rs"))
            .await;

        // Try to consume it under the wrong operation tag — the
        // debug_assert! fires because the recorded operation
        // (`textDocument/diagnostic`) does not match the call site's
        // expectation (`textDocument/definition`).
        let _ = provider.consume_provenance_for("textDocument/definition");
    }

    /// Pass 3: `collect_context` and `collect_hunk_context` dispatch
    /// provider calls sequentially — there is no `join!`, `tokio::spawn`,
    /// or `futures::join_all` over the provider in the canonical
    /// collector code. This test exercises the same sequential pattern
    /// against the adapter to verify the contract is observable: each
    /// call overwrites the previous provenance slot exactly once.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn collector_does_not_parallelize_provider_calls() {
        let service = LspService::new_arc(crate::config::LspConfig::default());
        let provider = ServiceLspEvidenceProvider::new(service, PathBuf::from("/tmp"));

        let _ = provider
            .diagnostics_for_file(std::path::Path::new("/tmp/a.rs"))
            .await;
        let first = provider
            .consume_provenance_for("textDocument/diagnostic")
            .expect("first provenance present");
        assert_eq!(first.operation, "textDocument/diagnostic");

        let _ = provider
            .diagnostics_for_file(std::path::Path::new("/tmp/b.rs"))
            .await;
        let second = provider
            .consume_provenance_for("textDocument/diagnostic")
            .expect("second provenance present");
        assert_eq!(second.operation, "textDocument/diagnostic");

        // After consuming the second, the slot is empty.
        assert!(provider
            .consume_provenance_for("textDocument/diagnostic")
            .is_none());
    }

    /// Pass 3: `take_provenance` clears the slot — it does not leave
    /// stale data that could be observed by the next caller. This is
    /// the foundational invariant the sequential contract relies on.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn adapter_take_provenance_clears_slot() {
        let service = LspService::new_arc(crate::config::LspConfig::default());
        let provider = ServiceLspEvidenceProvider::new(service, PathBuf::from("/tmp"));

        let _ = provider
            .diagnostics_for_file(std::path::Path::new("/tmp/x.rs"))
            .await;
        let first = provider.take_provenance();
        assert!(first.is_some(), "first take must return provenance");

        let second = provider.take_provenance();
        assert!(second.is_none(), "second take must return None");
        assert!(provider.last_provenance().is_none());
    }

    /// Pass 3: Provenance is recorded even when the underlying LSP call
    /// errors out. The `capability_decision` and `operation` fields must
    /// still be stamped so downstream consumers can attribute the failure
    /// to a specific LSP request, not just see "no provenance".
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn adapter_records_provenance_on_error() {
        let service = LspService::new_arc(crate::config::LspConfig::default());
        let provider = ServiceLspEvidenceProvider::new(service, PathBuf::from("/tmp"));

        // Trigger an error path: document_symbols on an unsupported config
        // returns Unavailable, but provenance must still be recorded.
        let result = provider
            .document_symbols(std::path::Path::new("/tmp/x.rs"))
            .await;
        assert!(result.is_err(), "expected Unavailable error");

        let prov = provider
            .consume_provenance_for("textDocument/documentSymbol")
            .expect("provenance must be recorded even on error");
        assert_eq!(prov.operation, "textDocument/documentSymbol");
        assert!(
            prov.capability_decision.is_some(),
            "capability_decision must be stamped on error path"
        );
    }
}
