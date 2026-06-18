use std::path::Path;

use lsp_types::*;
use url::Url;

use crate::client::url_to_uri;
use crate::edit::validate_path_against_root;
use crate::error::LspError;
use crate::overlay::{
    diagnostic_to_file_diagnostic, flatten_symbols, OverlaySession, SemanticCheckPreview,
};

use super::LspOperations;

fn uri_to_file_path(uri: &Uri) -> Result<std::path::PathBuf, LspError> {
    let url = Url::parse(uri.as_str())
        .map_err(|e| LspError::RequestFailed(format!("invalid LSP URI: {e}")))?;
    url.to_file_path()
        .map_err(|_| LspError::RequestFailed(format!("URI is not a file path: {}", uri.as_str())))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HierarchyDirection {
    Incoming,
    Outgoing,
    Both,
}

impl HierarchyDirection {
    pub fn parse(input: Option<&str>) -> Result<Self, crate::error::LspError> {
        match input.unwrap_or("both") {
            "incoming" => Ok(Self::Incoming),
            "outgoing" => Ok(Self::Outgoing),
            "both" => Ok(Self::Both),
            other => Err(crate::error::LspError::RequestFailed(format!(
                "unsupported hierarchy direction: {other}"
            ))),
        }
    }
}

impl LspOperations {
    pub async fn semantic_check_preview(
        &self,
        file_path: &Path,
        proposed_text: String,
        allowed_root: Option<&Path>,
    ) -> Result<SemanticCheckPreview, LspError> {
        const OVERLAY_DIAGNOSTIC_WAIT_MS: u64 = 250;
        const MAX_OVERLAY_DIAGNOSTICS: usize = 100;
        const MAX_OVERLAY_SYMBOLS: usize = 200;

        validate_path_against_root(file_path, allowed_root)?;

        let overlay = OverlaySession::new(self.service.clone());
        let token = overlay.apply_overlay(file_path, &proposed_text).await?;

        tokio::time::sleep(tokio::time::Duration::from_millis(
            OVERLAY_DIAGNOSTIC_WAIT_MS,
        ))
        .await;

        let warming = self
            .service
            .diagnostics_may_still_be_warming(&token.key, &token.uri)
            .await;

        let diag_result = self
            .service
            .get_diagnostics_for_key(&token.key, &token.uri)
            .await;

        let sym_result = self.document_symbols(file_path).await;

        let restore_result = overlay.restore(&token).await;

        let (diagnostics, diagnostics_error) = match diag_result {
            Ok(raw) => (
                raw.into_iter()
                    .take(MAX_OVERLAY_DIAGNOSTICS)
                    .map(|d| diagnostic_to_file_diagnostic(&token.uri, d))
                    .collect(),
                None,
            ),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };

        let (symbols, symbols_error) = match sym_result {
            Ok(raw) => {
                let mut symbols = Vec::new();
                let mut remaining = MAX_OVERLAY_SYMBOLS;
                flatten_symbols(&raw, &mut symbols, &mut remaining);
                (symbols, None)
            }
            Err(e) => (Vec::new(), Some(e.to_string())),
        };

        let (restored_disk_view, restore_error) = match restore_result {
            Ok(()) => (true, None),
            Err(e) => {
                tracing::warn!(file = %file_path.display(), error = %e, "overlay restore failed");
                (false, Some(e.to_string()))
            }
        };

        Ok(SemanticCheckPreview {
            file: token.uri,
            diagnostics_may_still_be_warming: warming,
            diagnostics,
            diagnostics_error,
            symbols,
            symbols_error,
            restored_disk_view,
            restore_error,
        })
    }

    pub async fn prepare_call_hierarchy(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<CallHierarchyItem>, LspError> {
        let (key, _root) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri = url_to_uri(&Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        let params = serde_json::to_value(CallHierarchyPrepareParams {
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
            .send_request(&key, "textDocument/prepareCallHierarchy", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let items: Vec<CallHierarchyItem> = serde_json::from_value(resp)?;
        Ok(items)
    }

    pub async fn incoming_calls(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyIncomingCall>, LspError> {
        let file_path = uri_to_file_path(&item.uri)?;
        let (key, _root) = self.service.get_or_create_client(&file_path).await?;

        let params = serde_json::to_value(CallHierarchyIncomingCallsParams {
            item,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "callHierarchy/incomingCalls", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let calls: Vec<CallHierarchyIncomingCall> = serde_json::from_value(resp)?;
        Ok(calls)
    }

    pub async fn outgoing_calls(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyOutgoingCall>, LspError> {
        let file_path = uri_to_file_path(&item.uri)?;
        let (key, _root) = self.service.get_or_create_client(&file_path).await?;

        let params = serde_json::to_value(CallHierarchyOutgoingCallsParams {
            item,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "callHierarchy/outgoingCalls", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let calls: Vec<CallHierarchyOutgoingCall> = serde_json::from_value(resp)?;
        Ok(calls)
    }

    pub async fn prepare_type_hierarchy(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<TypeHierarchyItem>, LspError> {
        let (key, _root) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri = url_to_uri(&Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        let params = serde_json::to_value(TypeHierarchyPrepareParams {
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
            .send_request(&key, "textDocument/prepareTypeHierarchy", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let items: Vec<TypeHierarchyItem> = serde_json::from_value(resp)?;
        Ok(items)
    }

    pub async fn supertypes(
        &self,
        item: TypeHierarchyItem,
    ) -> Result<Vec<TypeHierarchyItem>, LspError> {
        let file_path = uri_to_file_path(&item.uri)?;
        let (key, _root) = self.service.get_or_create_client(&file_path).await?;

        let params = serde_json::to_value(TypeHierarchySupertypesParams {
            item,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "typeHierarchy/supertypes", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let items: Vec<TypeHierarchyItem> = serde_json::from_value(resp)?;
        Ok(items)
    }

    pub async fn subtypes(
        &self,
        item: TypeHierarchyItem,
    ) -> Result<Vec<TypeHierarchyItem>, LspError> {
        let file_path = uri_to_file_path(&item.uri)?;
        let (key, _root) = self.service.get_or_create_client(&file_path).await?;

        let params = serde_json::to_value(TypeHierarchySubtypesParams {
            item,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "typeHierarchy/subtypes", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let items: Vec<TypeHierarchyItem> = serde_json::from_value(resp)?;
        Ok(items)
    }
}
