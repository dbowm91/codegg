use std::path::Path;

use lsp_types::*;
use tracing::trace;
use url::Url;

use crate::capability::{CapabilityDecision, LspCapabilitySnapshot, LspSemanticOperation};
use crate::client::url_to_uri;
use crate::error::LspError;
use crate::language::detect_language;

use super::signature::format_hover_contents;
use super::LspOperations;

/// Normalize a `GotoDefinitionResponse`-shaped payload (used by
/// definition, declaration, implementation, typeDefinition) into a
/// uniform `Vec<LocationLink>`. `origin_selection_range` is set to
/// `None` because the upstream response variants do not carry it.
pub fn normalize_goto_response(response: GotoDefinitionResponse) -> Vec<LocationLink> {
    match response {
        GotoDefinitionResponse::Link(links) => links,
        GotoDefinitionResponse::Scalar(loc) => vec![LocationLink {
            origin_selection_range: None,
            target_uri: loc.uri,
            target_range: loc.range,
            target_selection_range: loc.range,
        }],
        GotoDefinitionResponse::Array(locs) => locs
            .into_iter()
            .map(|loc| LocationLink {
                origin_selection_range: None,
                target_uri: loc.uri,
                target_range: loc.range,
                target_selection_range: loc.range,
            })
            .collect(),
    }
}

/// Normalize a `WorkspaceSymbolResponse` (either `Flat(Vec<SymbolInformation>)`
/// or `Nested(Vec<WorkspaceSymbol>)`) into a uniform `Vec<SymbolInformation>`.
///
/// For `WorkspaceSymbol` entries (the 3.17+ shape), the location can be
/// either a full `Location` or a `WorkspaceLocation` (URI only).
/// Workspace-only entries are surfaced with an empty range.
pub fn normalize_workspace_symbol_response(
    response: WorkspaceSymbolResponse,
) -> Vec<SymbolInformation> {
    match response {
        WorkspaceSymbolResponse::Flat(symbols) => symbols,
        WorkspaceSymbolResponse::Nested(symbols) => symbols
            .into_iter()
            .map(|sym| {
                let (uri, range) = match sym.location {
                    lsp_types::OneOf::Left(loc) => (loc.uri, loc.range),
                    lsp_types::OneOf::Right(wl) => {
                        // URI-only — no range available; emit an empty range.
                        (wl.uri, Range::default())
                    }
                };
                SymbolInformation {
                    name: sym.name,
                    kind: sym.kind,
                    tags: sym.tags,
                    location: Location { uri, range },
                    container_name: sym.container_name,
                    #[allow(deprecated)]
                    deprecated: None,
                }
            })
            .collect(),
    }
}

fn uri_to_file_path(uri: &Uri) -> Result<std::path::PathBuf, LspError> {
    let url = Url::parse(uri.as_str())
        .map_err(|e| LspError::RequestFailed(format!("invalid LSP URI: {e}")))?;
    url.to_file_path()
        .map_err(|_| LspError::RequestFailed(format!("URI is not a file path: {}", uri.as_str())))
}

impl LspOperations {
    /// Look up the [`LspCapabilitySnapshot`] for the client that
    /// services `file_path`. Returns `None` when the client has not
    /// published capabilities yet (i.e. still initializing).
    pub(crate) async fn capability_snapshot_for_file_impl(
        &self,
        file_path: &Path,
    ) -> Option<LspCapabilitySnapshot> {
        let (key, _) = self.service.get_or_create_client(file_path).await.ok()?;
        // Prefer the stored override-aware snapshot.
        if let Some(mut snap) = self.service.normalized_capabilities_for_key(&key).await {
            // Augment with observation state: if the client has
            // received a publishDiagnostics notification, mark the
            // snapshot accordingly so supports_diagnostics reflects
            // observed behavior.
            if self.service.has_observed_push_diagnostics_for_key(&key).await
                && !snap.observed_push_diagnostics
            {
                snap.observed_push_diagnostics = true;
                snap.supports_diagnostics = snap.supports_pull_diagnostics
                    || snap.observed_push_diagnostics
                    || snap.supports_push_diagnostics;
            }
            return Some(snap);
        }
        // Fallback: rebuild from raw capabilities (should not happen in
        // normal operation after initialization completes).
        let caps = self.service.get_capabilities_for_key(&key).await?;
        let lang = detect_language(file_path.to_str().unwrap_or(""));
        let server_name = key.split(':').next_back().map(String::from);
        Some(LspCapabilitySnapshot::from_capabilities(
            &caps,
            server_name.as_deref(),
            lang,
        ))
    }

    /// Fail fast with a structured [`LspUnavailable`] when the server
    /// does not advertise the requested operation.
    ///
    /// When the snapshot is unavailable (client still initializing, or
    /// no caps published yet), the state is classified as
    /// [`CapabilityDecision::Unknown`] and logged explicitly. The
    /// request is allowed to proceed for backward compatibility.
    pub(crate) async fn require_capability(
        &self,
        file_path: &Path,
        op: LspSemanticOperation,
    ) -> Result<(), LspError> {
        let (key, _) = self.service.get_or_create_client(file_path).await?;
        match self.service.capability_decision(&key, op).await {
            CapabilityDecision::Supported => Ok(()),
            CapabilityDecision::Unsupported(u) => Err(LspError::Unavailable(u)),
            CapabilityDecision::Unknown { operation, reason } => {
                tracing::debug!(?operation, reason, "capability unknown; allowing request");
                Ok(())
            }
        }
    }

    /// Return the first known client key, used as the routing target
    /// for workspace-scoped requests.
    async fn first_client_key(&self) -> Option<String> {
        self.service.client_keys().await.into_iter().next()
    }

    pub async fn go_to_definition(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<LocationLink>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = url_to_uri(&Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        let params = serde_json::to_value(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/definition", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let gdr: GotoDefinitionResponse = serde_json::from_value(resp)?;
        Ok(match gdr {
            GotoDefinitionResponse::Link(links) => links,
            GotoDefinitionResponse::Scalar(loc) => vec![LocationLink {
                origin_selection_range: None,
                target_uri: loc.uri,
                target_range: loc.range,
                target_selection_range: loc.range,
            }],
            GotoDefinitionResponse::Array(locs) => locs
                .into_iter()
                .map(|loc| LocationLink {
                    origin_selection_range: None,
                    target_uri: loc.uri,
                    target_range: loc.range,
                    target_selection_range: loc.range,
                })
                .collect(),
        })
    }

    /// Read-only `textDocument/declaration`. Capability-gated: returns
    /// [`LspError::Unavailable`] with a structured reason when the
    /// server does not advertise `declarationProvider`.
    pub async fn declaration(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<LocationLink>, LspError> {
        self.require_capability(file_path, LspSemanticOperation::Declaration)
            .await?;
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = url_to_uri(&Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        let params = serde_json::to_value(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/declaration", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let gdr: GotoDefinitionResponse = serde_json::from_value(resp)?;
        Ok(normalize_goto_response(gdr))
    }

    /// Read-only `textDocument/implementation`. Capability-gated.
    pub async fn implementation(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<LocationLink>, LspError> {
        self.require_capability(file_path, LspSemanticOperation::Implementation)
            .await?;
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = url_to_uri(&Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        // GotoImplementationParams == GotoTypeDefinitionParams == GotoDefinitionParams
        let params = serde_json::to_value(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/implementation", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let gdr: GotoDefinitionResponse = serde_json::from_value(resp)?;
        Ok(normalize_goto_response(gdr))
    }

    /// Read-only `textDocument/documentHighlight`. Capability-gated.
    /// Returns the raw `DocumentHighlight` entries (preserving the
    /// optional `kind` of Text / Read / Write).
    pub async fn document_highlights(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<DocumentHighlight>, LspError> {
        self.require_capability(file_path, LspSemanticOperation::DocumentHighlight)
            .await?;
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = url_to_uri(&Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        let params = serde_json::to_value(DocumentHighlightParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/documentHighlight", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let highlights: Vec<DocumentHighlight> = serde_json::from_value(resp)?;
        Ok(highlights)
    }

    /// Read-only `workspace/symbol` query. Capability-gated.
    pub async fn workspace_symbols(&self, query: &str) -> Result<Vec<SymbolInformation>, LspError> {
        let key = self.first_client_key().await.ok_or_else(|| {
            LspError::NotInitialized("no LSP client available for workspace_symbols".to_string())
        })?;

        match self.service.capability_decision(&key, LspSemanticOperation::WorkspaceSymbols).await {
            CapabilityDecision::Supported => {}
            CapabilityDecision::Unsupported(u) => return Err(LspError::Unavailable(u)),
            CapabilityDecision::Unknown { operation, reason } => {
                tracing::debug!(?operation, reason, "capability unknown; allowing workspace_symbols request");
            }
        }

        let params = serde_json::to_value(WorkspaceSymbolParams {
            query: query.to_string(),
            partial_result_params: Default::default(),
            work_done_progress_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "workspace/symbol", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let response: WorkspaceSymbolResponse = serde_json::from_value(resp)?;
        Ok(normalize_workspace_symbol_response(response))
    }

    pub async fn find_references(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<Location>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = url_to_uri(&Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        let params = serde_json::to_value(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line,
                    character: column,
                },
            },
            context: ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/references", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let refs: Vec<Location> = serde_json::from_value(resp)?;
        Ok(refs)
    }

    pub async fn hover(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Option<String>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = url_to_uri(&Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        let params = serde_json::to_value(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/hover", params)
            .await?;

        if resp.is_null() {
            return Ok(None);
        }

        let hover: Hover = match serde_json::from_value(resp) {
            Ok(h) => h,
            Err(e) => {
                trace!("failed to parse hover response: {}", e);
                return Ok(None);
            }
        };
        Ok(Some(format_hover_contents(&hover.contents)))
    }

    pub async fn document_symbols(
        &self,
        file_path: &Path,
    ) -> Result<Vec<DocumentSymbol>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = url_to_uri(&Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        let params = serde_json::to_value(DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/documentSymbol", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let symbols: Vec<DocumentSymbol> = serde_json::from_value(resp)?;
        Ok(symbols)
    }

    pub async fn code_lens(&self, file_path: &Path) -> Result<Vec<CodeLens>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = url_to_uri(&Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        let params = serde_json::to_value(CodeLensParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/codeLens", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let lenses: Vec<CodeLens> = serde_json::from_value(resp)?;
        Ok(lenses)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{LspCapabilitySnapshot, LspSemanticOperation};
    use lsp_types::{Uri, ServerCapabilities, OneOf};
    use std::str::FromStr;

    fn uri(s: &str) -> Uri {
        Uri::from_str(s).expect("valid uri")
    }

    // ---- normalize_goto_response ----

    #[test]
    fn normalize_goto_response_link_passthrough() {
        let loc = Location {
            uri: uri("file:///a.rs"),
            range: Range {
                start: Position {
                    line: 1,
                    character: 2,
                },
                end: Position {
                    line: 3,
                    character: 4,
                },
            },
        };
        let resp = GotoDefinitionResponse::Link(vec![LocationLink {
            origin_selection_range: None,
            target_uri: loc.uri.clone(),
            target_range: loc.range,
            target_selection_range: loc.range,
        }]);
        let out = normalize_goto_response(resp);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].target_uri, loc.uri);
        assert_eq!(out[0].target_range, loc.range);
        assert_eq!(out[0].origin_selection_range, None);
    }

    #[test]
    fn normalize_goto_response_scalar_promotes_to_link() {
        let resp = GotoDefinitionResponse::Scalar(Location {
            uri: uri("file:///b.rs"),
            range: Range {
                start: Position {
                    line: 10,
                    character: 0,
                },
                end: Position {
                    line: 10,
                    character: 5,
                },
            },
        });
        let out = normalize_goto_response(resp);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].target_uri, uri("file:///b.rs"));
        assert_eq!(out[0].origin_selection_range, None);
        assert_eq!(out[0].target_selection_range, out[0].target_range);
    }

    #[test]
    fn normalize_goto_response_array_promotes_each_to_link() {
        let resp = GotoDefinitionResponse::Array(vec![
            Location {
                uri: uri("file:///c.rs"),
                range: Range::default(),
            },
            Location {
                uri: uri("file:///d.rs"),
                range: Range::default(),
            },
        ]);
        let out = normalize_goto_response(resp);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].target_uri, uri("file:///c.rs"));
        assert_eq!(out[1].target_uri, uri("file:///d.rs"));
        for link in &out {
            assert!(link.origin_selection_range.is_none());
        }
    }

    // ---- normalize_workspace_symbol_response ----

    #[test]
    fn normalize_workspace_symbol_response_flat_passthrough() {
        let resp = WorkspaceSymbolResponse::Flat(vec![SymbolInformation {
            name: "foo".to_string(),
            kind: SymbolKind::FUNCTION,
            tags: None,
            location: Location {
                uri: uri("file:///a.rs"),
                range: Range::default(),
            },
            container_name: None,
            #[allow(deprecated)]
            deprecated: None,
        }]);
        let out = normalize_workspace_symbol_response(resp);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "foo");
        assert_eq!(out[0].kind, SymbolKind::FUNCTION);
    }

    #[test]
    fn normalize_workspace_symbol_response_nested_full_location() {
        let resp = WorkspaceSymbolResponse::Nested(vec![WorkspaceSymbol {
            name: "bar".to_string(),
            kind: SymbolKind::STRUCT,
            tags: None,
            container_name: Some("mod".to_string()),
            location: lsp_types::OneOf::Left(Location {
                uri: uri("file:///b.rs"),
                range: Range {
                    start: Position {
                        line: 5,
                        character: 0,
                    },
                    end: Position {
                        line: 5,
                        character: 3,
                    },
                },
            }),
            data: None,
        }]);
        let out = normalize_workspace_symbol_response(resp);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "bar");
        assert_eq!(out[0].kind, SymbolKind::STRUCT);
        assert_eq!(out[0].container_name.as_deref(), Some("mod"));
        assert_eq!(out[0].location.uri, uri("file:///b.rs"));
    }

    #[test]
    fn normalize_workspace_symbol_response_nested_workspace_location() {
        let resp = WorkspaceSymbolResponse::Nested(vec![WorkspaceSymbol {
            name: "baz".to_string(),
            kind: SymbolKind::VARIABLE,
            tags: None,
            container_name: None,
            location: lsp_types::OneOf::Right(WorkspaceLocation {
                uri: uri("file:///c.rs"),
            }),
            data: None,
        }]);
        let out = normalize_workspace_symbol_response(resp);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "baz");
        assert_eq!(out[0].location.uri, uri("file:///c.rs"));
        assert_eq!(out[0].location.range, Range::default());
    }

    // ---- DocumentHighlight (kind preservation via JSON round-trip) ----

    #[test]
    fn document_highlight_kind_round_trips() {
        let original = vec![
            DocumentHighlight {
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 3,
                    },
                },
                kind: Some(DocumentHighlightKind::TEXT),
            },
            DocumentHighlight {
                range: Range {
                    start: Position {
                        line: 1,
                        character: 0,
                    },
                    end: Position {
                        line: 1,
                        character: 4,
                    },
                },
                kind: Some(DocumentHighlightKind::READ),
            },
            DocumentHighlight {
                range: Range {
                    start: Position {
                        line: 2,
                        character: 0,
                    },
                    end: Position {
                        line: 2,
                        character: 5,
                    },
                },
                kind: Some(DocumentHighlightKind::WRITE),
            },
            DocumentHighlight {
                range: Range {
                    start: Position {
                        line: 3,
                        character: 0,
                    },
                    end: Position {
                        line: 3,
                        character: 1,
                    },
                },
                kind: None,
            },
        ];
        let v = serde_json::to_value(&original).expect("serialize");
        let decoded: Vec<DocumentHighlight> = serde_json::from_value(v).expect("deserialize");
        assert_eq!(decoded, original);
        assert_eq!(decoded[0].kind, Some(DocumentHighlightKind::TEXT));
        assert_eq!(decoded[1].kind, Some(DocumentHighlightKind::READ));
        assert_eq!(decoded[2].kind, Some(DocumentHighlightKind::WRITE));
        assert_eq!(decoded[3].kind, None);
    }

    // ---- capability snapshot decision ----

    #[test]
    fn capability_snapshot_reports_declaration_as_unavailable_when_unset() {
        let caps = ServerCapabilities::default();
        let snap =
            LspCapabilitySnapshot::from_capabilities(&caps, Some("rust-analyzer"), Some("rust"));
        assert!(!snap.supports(LspSemanticOperation::Declaration));
        let u = snap.unavailable(LspSemanticOperation::Declaration).unwrap();
        assert_eq!(u.operation, "declaration");
        assert!(u.reason.contains("rust-analyzer"));
        assert_eq!(u.server.as_deref(), Some("rust-analyzer"));
        assert_eq!(u.language_id.as_deref(), Some("rust"));
    }

    #[test]
    fn capability_snapshot_reports_implementation_as_available_when_set() {
        let mut caps = ServerCapabilities::default();
        caps.implementation_provider = Some(ImplementationProviderCapability::Simple(true));
        let snap =
            LspCapabilitySnapshot::from_capabilities(&caps, Some("rust-analyzer"), Some("rust"));
        assert!(snap.supports(LspSemanticOperation::Implementation));
        assert!(snap
            .unavailable(LspSemanticOperation::Implementation)
            .is_none());
    }

    #[test]
    fn capability_snapshot_reports_document_highlight_as_available_when_set() {
        let mut caps = ServerCapabilities::default();
        caps.document_highlight_provider = Some(OneOf::Left(true));
        let snap =
            LspCapabilitySnapshot::from_capabilities(&caps, Some("tsls"), Some("typescript"));
        assert!(snap.supports(LspSemanticOperation::DocumentHighlight));
    }

    #[test]
    fn capability_snapshot_reports_workspace_symbols_unavailable_when_unset() {
        let caps = ServerCapabilities::default();
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("pylsp"), Some("python"));
        assert!(!snap.supports(LspSemanticOperation::WorkspaceSymbols));
        let u = snap
            .unavailable(LspSemanticOperation::WorkspaceSymbols)
            .unwrap();
        assert_eq!(u.operation, "workspaceSymbol");
        assert!(u.reason.contains("pylsp"));
    }

    // ---- capability gating: completion ----

    #[test]
    fn capability_snapshot_reports_completion_as_unavailable_when_unset() {
        let caps = ServerCapabilities::default();
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("pylsp"), Some("python"));
        assert!(!snap.supports(LspSemanticOperation::Completion));
        let u = snap
            .unavailable(LspSemanticOperation::Completion)
            .expect("unavailable");
        assert_eq!(u.operation, "completion");
        assert!(u.reason.contains("pylsp"));
    }
}
