use std::path::Path;

use lsp_types::*;
use tracing::trace;
use url::Url;

use crate::client::url_to_uri;
use crate::edit::{preview_text_edits_for_file, preview_workspace_edit, WorkspaceEditPreview};
use crate::error::LspError;
use crate::service::LspService;

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
        _allowed_root: Option<&Path>,
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

        preview_text_edits_for_file("format", file_path, edits)
    }
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
