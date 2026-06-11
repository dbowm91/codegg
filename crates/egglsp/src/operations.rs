use std::path::Path;

use lsp_types::*;
use tracing::trace;
use url::Url;

use crate::client::url_to_uri;
use crate::edit::{
    preview_text_edits_for_file, preview_workspace_edit, validate_path_against_root,
    WorkspaceEditPreview,
};
use crate::error::LspError;
use crate::overlay::{
    diagnostic_to_file_diagnostic, flatten_symbols, OverlaySession, SemanticCheckPreview,
};
use crate::service::LspService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceActionPreviewKind {
    OrganizeImports,
}

impl SourceActionPreviewKind {
    pub fn parse(input: &str) -> Result<Self, LspError> {
        match input {
            "source.organizeImports" | "organizeImports" | "organize_imports" => {
                Ok(Self::OrganizeImports)
            }
            other => Err(LspError::UnsupportedSourceAction(other.to_string())),
        }
    }

    pub fn lsp_kind(self) -> CodeActionKind {
        match self {
            Self::OrganizeImports => CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::OrganizeImports => "organize imports",
        }
    }
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

/// Pure helper: given a requested action kind and the raw LSP code action
/// responses, select the single best edit-bearing `WorkspaceEdit`.
///
/// Rules:
/// - Raw `Command` variants are rejected.
/// - `CodeAction` with `command: Some(_)` but `edit: None` is command-only
///   (command execution is disabled).
/// - `CodeAction` values without `edit` or `command` are rejected.
/// - Actions whose kind is not hierarchically compatible with the
///   requested kind are rejected.
/// - Exactly one edit-bearing match is returned.
/// - Zero matches → `NoEditForSourceAction` or `CommandOnlySourceAction`.
/// - Multiple matches → `AmbiguousSourceAction`.
pub fn select_source_action_edit(
    requested: SourceActionPreviewKind,
    actions: Vec<CodeActionOrCommand>,
) -> Result<WorkspaceEdit, LspError> {
    let requested_kind = requested.lsp_kind();
    let title = requested.title();

    let mut edit_bearing: Vec<(&CodeAction, &str)> = Vec::new();
    let mut matching_command_only = 0usize;

    for action in &actions {
        match action {
            CodeActionOrCommand::Command(cmd) => {
                trace!(
                    "source action: rejecting raw Command variant: {}",
                    cmd.command
                );
            }
            CodeActionOrCommand::CodeAction(ca) => {
                let kind_matches = match &ca.kind {
                    Some(kind) => {
                        kind == &requested_kind
                            || kind
                                .as_str()
                                .starts_with(&format!("{}.", requested_kind.as_str()))
                    }
                    None => false,
                };
                if !kind_matches {
                    trace!(
                        "source action: rejecting action '{}' with kind {:?}",
                        ca.title,
                        ca.kind
                    );
                    continue;
                }
                if let Some(_edit) = &ca.edit {
                    edit_bearing.push((ca, title));
                } else if ca.command.is_some() {
                    trace!(
                        "source action: rejecting action '{}' (command-only, no edit)",
                        ca.title
                    );
                    matching_command_only += 1;
                } else {
                    trace!(
                        "source action: rejecting action '{}' (no edit, no command)",
                        ca.title
                    );
                }
            }
        }
    }

    match edit_bearing.len() {
        0 => {
            let has_raw_command = actions
                .iter()
                .any(|a| matches!(a, CodeActionOrCommand::Command(_)));
            if has_raw_command || matching_command_only > 0 {
                Err(LspError::CommandOnlySourceAction(title.to_string()))
            } else {
                Err(LspError::NoEditForSourceAction(title.to_string()))
            }
        }
        1 => {
            let (ca, _title) = edit_bearing.remove(0);
            Ok(ca.edit.clone().expect("checked above"))
        }
        _ => {
            let titles: Vec<&str> = edit_bearing
                .iter()
                .map(|(ca, _)| ca.title.as_str())
                .collect();
            Err(LspError::AmbiguousSourceAction(
                title.to_string(),
                titles.join(", "),
            ))
        }
    }
}

/// Compute the LSP `Position` at the end of a document, using UTF-16 code
/// units for the character offset (as required by the LSP specification).
///
/// - empty string → `(0, 0)`
/// - one-line ASCII text → `(0, len)`
/// - text ending in newline → final line is the empty line after the
///   newline, character `0`
/// - unicode text counts UTF-16 code units, not bytes or chars
pub fn document_end_position_utf16(text: &str) -> Position {
    if text.is_empty() {
        return Position {
            line: 0,
            character: 0,
        };
    }
    let mut line: u32 = 0;
    let mut character: u32 = 0;
    for c in text.chars() {
        if c == '\n' {
            line += 1;
            character = 0;
        } else {
            character += c.len_utf16() as u32;
        }
    }
    // If the text ends with a newline, the cursor is at the start of the
    // next (empty) line — which is already correct from the loop.
    // If it does not end with a newline, character points to the end of
    // the last line.
    Position { line, character }
}

pub struct LspOperations {
    service: std::sync::Arc<LspService>,
}

impl LspOperations {
    pub fn new(service: std::sync::Arc<LspService>) -> Self {
        Self { service }
    }

    pub async fn go_to_definition(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<LocationLink>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
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

    pub async fn find_references(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<Location>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
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
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
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
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(DocumentSymbolParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(&uri)?,
            },
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

    pub async fn code_actions(
        &self,
        file_path: &Path,
        start_line: u32,
        start_col: u32,
        end_line: u32,
        end_col: u32,
        diagnostics: Vec<Diagnostic>,
        only: Option<Vec<CodeActionKind>>,
    ) -> Result<Vec<CodeActionOrCommand>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let range = Range {
            start: Position {
                line: start_line,
                character: start_col,
            },
            end: Position {
                line: end_line,
                character: end_col,
            },
        };

        let params = serde_json::to_value(CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(&uri)?,
            },
            range,
            context: CodeActionContext {
                diagnostics,
                only,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/codeAction", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let actions: Vec<CodeActionOrCommand> = serde_json::from_value(resp)?;
        Ok(actions)
    }

    pub async fn completion(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
        trigger_kind: Option<CompletionTriggerKind>,
        trigger_char: Option<String>,
    ) -> Result<Vec<CompletionItem>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: trigger_kind.map(|kind| CompletionContext {
                trigger_kind: kind,
                trigger_character: trigger_char,
            }),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/completion", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let items: Vec<CompletionItem> =
            match serde_json::from_value::<CompletionList>(resp.clone()) {
                Ok(list) => list.items,
                Err(_) => serde_json::from_value(resp).unwrap_or_default(),
            };
        Ok(items)
    }

    pub async fn signature_help(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Option<String>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(SignatureHelpParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
            context: None,
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/signatureHelp", params)
            .await?;

        if resp.is_null() {
            return Ok(None);
        }

        let help: SignatureHelp = match serde_json::from_value(resp) {
            Ok(h) => h,
            Err(e) => {
                trace!("failed to parse signature help response: {}", e);
                return Ok(None);
            }
        };
        Ok(Some(format_signature_help(&help)))
    }

    pub async fn code_lens(&self, file_path: &Path) -> Result<Vec<CodeLens>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(CodeLensParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(&uri)?,
            },
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

    pub async fn prepare_rename(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Option<PrepareRenameResponse>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(&uri)?,
            },
            position: Position {
                line,
                character: column,
            },
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/prepareRename", params)
            .await?;

        if resp.is_null() {
            return Ok(None);
        }

        let pr: Option<PrepareRenameResponse> = serde_json::from_value(resp)?;
        Ok(pr)
    }

    pub async fn rename_preview(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
        new_name: &str,
        allowed_root: Option<&Path>,
    ) -> Result<WorkspaceEditPreview, LspError> {
        let (key, _uri_str) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        // Optionally attempt prepareRename; ignore unsupported errors and proceed.
        let _ = self
            .service
            .send_request(
                &key,
                "textDocument/prepareRename",
                serde_json::to_value(TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: url_to_uri(&uri)?,
                    },
                    position: Position {
                        line,
                        character: column,
                    },
                })?,
            )
            .await;

        let params = serde_json::to_value(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
                position: Position {
                    line,
                    character: column,
                },
            },
            new_name: new_name.to_string(),
            work_done_progress_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/rename", params)
            .await?;

        if resp.is_null() {
            return Err(LspError::RequestFailed(
                "rename returned no result (no edits or unsupported at location)".to_string(),
            ));
        }

        let ws_edit: WorkspaceEdit = serde_json::from_value(resp)?;
        preview_workspace_edit("rename symbol", ws_edit, allowed_root)
    }

    pub async fn format_preview(
        &self,
        file_path: &Path,
        allowed_root: Option<&Path>,
    ) -> Result<WorkspaceEditPreview, LspError> {
        let (key, _uri_str) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(DocumentFormattingParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(&uri)?,
            },
            options: FormattingOptions {
                tab_size: 4,
                insert_spaces: true,
                properties: Default::default(),
                trim_trailing_whitespace: Some(true),
                insert_final_newline: Some(true),
                trim_final_newlines: Some(true),
            },
            work_done_progress_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/formatting", params)
            .await?;

        if resp.is_null() {
            return Ok(WorkspaceEditPreview {
                title: "format".to_string(),
                files: vec![],
                total_files: 0,
                total_edits: 0,
                truncated: false,
            });
        }

        let edits: Vec<TextEdit> = serde_json::from_value(resp)?;
        if edits.is_empty() {
            return Ok(WorkspaceEditPreview {
                title: "format".to_string(),
                files: vec![],
                total_files: 0,
                total_edits: 0,
                truncated: false,
            });
        }

        preview_text_edits_for_file("format", file_path, edits, allowed_root)
    }

    pub async fn source_action_preview(
        &self,
        file_path: &Path,
        action: SourceActionPreviewKind,
        allowed_root: Option<&Path>,
    ) -> Result<WorkspaceEditPreview, LspError> {
        let (key, _uri_str) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let text = tokio::fs::read_to_string(file_path).await.map_err(|e| {
            LspError::RequestFailed(format!(
                "failed to read file {}: {}",
                file_path.display(),
                e
            ))
        })?;
        let end = document_end_position_utf16(&text);

        let params = serde_json::to_value(CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(&uri)?,
            },
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end,
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: Some(vec![action.lsp_kind()]),
                trigger_kind: Some(CodeActionTriggerKind::INVOKED),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/codeAction", params)
            .await?;

        if resp.is_null() {
            return Err(LspError::NoEditForSourceAction(action.title().to_string()));
        }

        let actions: Vec<CodeActionOrCommand> = serde_json::from_value(resp)?;
        let ws_edit = crate::operations::select_source_action_edit(action, actions)?;
        preview_workspace_edit(action.title(), ws_edit, allowed_root)
    }

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
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(CallHierarchyPrepareParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
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
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(TypeHierarchyPrepareParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
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

fn uri_to_file_path(uri: &Uri) -> Result<std::path::PathBuf, LspError> {
    let url = Url::parse(uri.as_str())
        .map_err(|e| LspError::RequestFailed(format!("invalid LSP URI: {e}")))?;
    url.to_file_path()
        .map_err(|_| LspError::RequestFailed(format!("URI is not a file path: {}", uri.as_str())))
}

fn format_hover_contents(contents: &HoverContents) -> String {
    match contents {
        HoverContents::Scalar(s) => match s {
            MarkedString::String(s) => s.clone(),
            MarkedString::LanguageString(ls) => {
                format!("```{}\n{}\n```", ls.language, ls.value)
            }
        },
        HoverContents::Array(arr) => arr
            .iter()
            .map(|s| match s {
                MarkedString::String(s) => s.clone(),
                MarkedString::LanguageString(ls) => {
                    format!("```{}\n{}\n```", ls.language, ls.value)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        HoverContents::Markup(mc) => mc.value.clone(),
    }
}

fn format_documentation(doc: &Documentation) -> String {
    match doc {
        Documentation::String(s) => s.clone(),
        Documentation::MarkupContent(mc) => mc.value.clone(),
    }
}

fn format_signature_help(help: &SignatureHelp) -> String {
    let mut result = String::new();
    for (i, sig) in help.signatures.iter().enumerate() {
        if i > 0 {
            result.push_str("\n---\n");
        }
        result.push_str(&sig.label);
        if let Some(doc) = &sig.documentation {
            result.push_str("\n\n");
            result.push_str(&format_documentation(doc));
        }
        if let Some(params) = &help.signatures[i].parameters {
            for (j, param) in params.iter().enumerate() {
                let label_str = match &param.label {
                    ParameterLabel::Simple(s) => s.clone(),
                    ParameterLabel::LabelOffsets([start, end]) => {
                        sig.label[*start as usize..*end as usize].to_string()
                    }
                };
                let doc_str = param
                    .documentation
                    .as_ref()
                    .map(format_documentation)
                    .unwrap_or_default();
                result.push_str(&format!("\n  {}. {}: {}", j + 1, label_str, doc_str));
            }
        }
    }
    result
}
