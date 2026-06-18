use std::path::Path;

use lsp_types::*;
use tracing::trace;

use crate::client::url_to_uri;
use crate::edit::preview_workspace_edit;
use crate::error::LspError;

use super::LspOperations;

/// Default cap on the number of [`CodeActionSummary`] entries
/// returned by [`LspOperations::code_action_summaries`].
pub const CODE_ACTION_SUMMARY_DEFAULT_MAX: usize = 50;

/// Bounded, preview-only code-action summary DTO. The raw
/// `WorkspaceEdit` and `Command` payloads are intentionally not
/// exposed — this surface is read-only and never applies edits.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodeActionSummary {
    pub title: String,
    /// LSP `CodeActionKind` (e.g. `"quickfix"`,
    /// `"refactor.extract"`, `"source.organizeImports"`). `None`
    /// for raw `Command` actions and for `CodeAction` values
    /// without a kind.
    pub kind: Option<String>,
    pub preferred: bool,
    /// `disabled.reason` from the LSP `CodeAction.disabled` field.
    pub disabled_reason: Option<String>,
    /// True when the action carries a `WorkspaceEdit` payload.
    pub has_edit: bool,
    /// True when the action carries a `Command` payload. The
    /// surface never executes commands; `has_command == true`
    /// indicates the action is command-only and cannot be
    /// previewed.
    pub has_command: bool,
    /// Bounded diagnostic descriptions (code + message) this
    /// action is reported to address. Server order is preserved.
    pub diagnostics: Vec<String>,
}

impl CodeActionSummary {
    /// Build a summary from a single raw `CodeActionOrCommand`.
    /// Pure conversion — does not touch the network.
    pub fn from_action(action: &CodeActionOrCommand) -> Self {
        match action {
            CodeActionOrCommand::Command(cmd) => Self {
                title: cmd.title.clone(),
                kind: None,
                preferred: false,
                disabled_reason: None,
                has_edit: false,
                has_command: true,
                diagnostics: Vec::new(),
            },
            CodeActionOrCommand::CodeAction(ca) => Self {
                title: ca.title.clone(),
                kind: ca.kind.as_ref().map(|k| k.as_str().to_string()),
                preferred: ca.is_preferred.unwrap_or(false),
                disabled_reason: ca.disabled.as_ref().map(|d| d.reason.clone()),
                has_edit: ca.edit.is_some(),
                has_command: ca.command.is_some(),
                diagnostics: ca
                    .diagnostics
                    .as_ref()
                    .map(|ds| {
                        ds.iter()
                            .map(|d| format!("{:?}: {}", d.code, d.message))
                            .collect()
                    })
                    .unwrap_or_default(),
            },
        }
    }
}

/// Bounded, preview-only code-action DTO. Wraps a
/// [`WorkspaceEditPreview`] (built from the resolved action's
/// `WorkspaceEdit`) with structured warnings. Commands are
/// rejected up-front (they cannot be previewed) and surface as
/// `LspError::CommandOnlyCodeAction`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodeActionPreview {
    pub title: String,
    pub kind: Option<String>,
    /// Per-file preview entries. Out-of-root files produced
    /// errors during the underlying `preview_workspace_edit` call
    /// and are not present here.
    pub affected_files: Vec<crate::edit::FileEditPreview>,
    pub edit_count: usize,
    /// Structured warnings (e.g. resource operations present in
    /// the raw edit that the preview pipeline could not surface).
    pub warnings: Vec<String>,
    /// True when the underlying edit count or file count
    /// exceeded the preview caps.
    pub truncated: bool,
    pub server_generation: u64,
}

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

impl LspOperations {
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
        let uri = url_to_uri(&url::Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

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
            text_document: TextDocumentIdentifier { uri },
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

    // ── Phase 4 Pass 7: typed code-action surface ───────────────────

    /// Bounded, read-only `textDocument/codeAction` summary DTOs.
    /// Capability-gated: returns `LspUnavailable` when the server
    /// does not advertise a code-action provider.
    ///
    /// The output is truncated to at most `max_actions` items
    /// preserving server order. Raw `Command` payloads are
    /// surfaced as summaries with `has_command = true` and
    /// `has_edit = false`; the surface never executes commands.
    pub async fn code_action_summaries(
        &self,
        file_path: &Path,
        start_line: u32,
        start_col: u32,
        end_line: u32,
        end_col: u32,
        diagnostics: Vec<Diagnostic>,
        only: Option<Vec<CodeActionKind>>,
        max_actions: usize,
    ) -> Result<Vec<CodeActionSummary>, LspError> {
        use crate::capability::LspSemanticOperation;

        if max_actions == 0 {
            return Ok(Vec::new());
        }
        if let Some(snapshot) = self.capability_snapshot_for_file(file_path).await {
            if !snapshot.supports(LspSemanticOperation::CodeAction) {
                if let Some(u) = snapshot.unavailable(LspSemanticOperation::CodeAction) {
                    return Err(LspError::Unavailable(u));
                }
            }
        }

        let actions = self
            .code_actions(
                file_path,
                start_line,
                start_col,
                end_line,
                end_col,
                diagnostics,
                only,
            )
            .await?;

        Ok(actions
            .iter()
            .take(max_actions)
            .map(CodeActionSummary::from_action)
            .collect())
    }

    /// Preview a single code action identified by `action_index`
    /// (the index into the same ordering produced by
    /// `code_action_summaries`). Capability-gated via
    /// `code_action_summaries`.
    ///
    /// Returns [`LspError::CommandOnlyCodeAction`] when the
    /// resolved action is a raw `Command` (the surface never
    /// executes commands) or when the resolved `CodeAction` has
    /// `command: Some(_)` but no `edit` payload. The on-disk
    /// file is never mutated.
    pub async fn preview_code_action(
        &self,
        file_path: &Path,
        start_line: u32,
        start_col: u32,
        end_line: u32,
        end_col: u32,
        diagnostics: Vec<Diagnostic>,
        only: Option<Vec<CodeActionKind>>,
        action_index: usize,
        allowed_root: Option<&Path>,
    ) -> Result<CodeActionPreview, LspError> {
        use crate::capability::LspSemanticOperation;

        // Capability-gate by running the same check as summaries
        // (without consuming a max_actions budget).
        if let Some(snapshot) = self.capability_snapshot_for_file(file_path).await {
            if !snapshot.supports(LspSemanticOperation::CodeAction) {
                if let Some(u) = snapshot.unavailable(LspSemanticOperation::CodeAction) {
                    return Err(LspError::Unavailable(u));
                }
            }
        }

        let actions = self
            .code_actions(
                file_path,
                start_line,
                start_col,
                end_line,
                end_col,
                diagnostics,
                only,
            )
            .await?;

        let action = actions.get(action_index).ok_or_else(|| {
            LspError::RequestFailed(format!(
                "action_index {} out of range (server returned {} actions)",
                action_index,
                actions.len()
            ))
        })?;

        let (title, kind, ws_edit) = match action {
            CodeActionOrCommand::Command(cmd) => {
                return Err(LspError::CommandOnlyCodeAction(cmd.title.clone()));
            }
            CodeActionOrCommand::CodeAction(ca) => {
                if ca.edit.is_none() {
                    // Command-only (or empty) CodeAction. The
                    // surface never executes commands; reject
                    // with the same error type.
                    return Err(LspError::CommandOnlyCodeAction(ca.title.clone()));
                }
                (
                    ca.title.clone(),
                    ca.kind.as_ref().map(|k| k.as_str().to_string()),
                    ca.edit.clone().expect("edit checked above"),
                )
            }
        };

        let preview = preview_workspace_edit("code action", ws_edit, allowed_root)?;

        // preview_workspace_edit already errors on resource ops
        // via UnsupportedEdit; we only surface edits-only shapes
        // here, so the warning bucket stays empty.
        let warnings: Vec<String> = Vec::new();

        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let server_generation = self.service.generation_for_key(&key).await;

        Ok(CodeActionPreview {
            title,
            kind,
            affected_files: preview.files,
            edit_count: preview.total_edits,
            warnings,
            truncated: preview.truncated,
            server_generation,
        })
    }

    pub async fn source_action_preview(
        &self,
        file_path: &Path,
        action: SourceActionPreviewKind,
        allowed_root: Option<&Path>,
    ) -> Result<crate::edit::WorkspaceEditPreview, LspError> {
        use super::formatting::document_end_position_utf16;

        let (key, _uri_str) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri = url_to_uri(&url::Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        let text = tokio::fs::read_to_string(file_path).await.map_err(|e| {
            LspError::RequestFailed(format!(
                "failed to read file {}: {}",
                file_path.display(),
                e
            ))
        })?;
        let end = document_end_position_utf16(&text);

        let params = serde_json::to_value(CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
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
        let ws_edit = super::code_actions::select_source_action_edit(action, actions)?;
        preview_workspace_edit(action.title(), ws_edit, allowed_root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{LspCapabilitySnapshot, LspSemanticOperation, LspUnavailable};
    use lsp_types::{ServerCapabilities, Uri};
    use std::str::FromStr;

    fn uri(s: &str) -> Uri {
        Uri::from_str(s).expect("valid uri")
    }

    // ---- CodeActionSummary ----

    fn command_action(title: &str) -> CodeActionOrCommand {
        CodeActionOrCommand::Command(Command {
            title: title.to_string(),
            command: "rust-analyzer.run".to_string(),
            arguments: None,
        })
    }

    fn code_action_with_edit(
        title: &str,
        kind: Option<CodeActionKind>,
        preferred: bool,
        disabled_reason: Option<&str>,
    ) -> CodeActionOrCommand {
        CodeActionOrCommand::CodeAction(CodeAction {
            title: title.to_string(),
            kind,
            diagnostics: Some(vec![Diagnostic::new_simple(
                Range::default(),
                "unused variable".to_string(),
            )]),
            edit: Some(WorkspaceEdit {
                changes: None,
                document_changes: None,
                change_annotations: None,
            }),
            command: None,
            is_preferred: Some(preferred),
            disabled: disabled_reason.map(|r| CodeActionDisabled {
                reason: r.to_string(),
            }),
            data: None,
        })
    }

    #[test]
    fn code_action_summary_for_command_variant() {
        let s = CodeActionSummary::from_action(&command_action("Run cargo build"));
        assert_eq!(s.title, "Run cargo build");
        assert!(s.kind.is_none());
        assert!(!s.preferred);
        assert!(s.disabled_reason.is_none());
        assert!(!s.has_edit);
        assert!(s.has_command);
        assert!(s.diagnostics.is_empty());
    }

    #[test]
    fn code_action_summary_for_code_action_with_edit() {
        let s = CodeActionSummary::from_action(&code_action_with_edit(
            "Remove unused",
            Some(CodeActionKind::QUICKFIX),
            true,
            None,
        ));
        assert_eq!(s.title, "Remove unused");
        assert_eq!(s.kind.as_deref(), Some("quickfix"));
        assert!(s.preferred);
        assert!(s.disabled_reason.is_none());
        assert!(s.has_edit);
        assert!(!s.has_command);
        assert_eq!(s.diagnostics.len(), 1);
        assert!(s.diagnostics[0].contains("unused variable"));
    }

    #[test]
    fn code_action_summary_for_disabled_code_action() {
        let s = CodeActionSummary::from_action(&code_action_with_edit(
            "Extract fn",
            Some(CodeActionKind::REFACTOR_EXTRACT),
            false,
            Some("cursor not on a function"),
        ));
        assert_eq!(s.kind.as_deref(), Some("refactor.extract"));
        assert!(!s.preferred);
        assert_eq!(
            s.disabled_reason.as_deref(),
            Some("cursor not on a function")
        );
    }

    #[test]
    fn code_action_summary_truncation_at_max_actions_preserves_order() {
        let actions: Vec<CodeActionOrCommand> = (0..5)
            .map(|i| {
                code_action_with_edit(
                    &format!("act-{i}"),
                    Some(CodeActionKind::REFACTOR_REWRITE),
                    false,
                    None,
                )
            })
            .collect();
        let max = 3usize;
        let bounded: Vec<CodeActionSummary> = actions
            .iter()
            .take(max)
            .map(CodeActionSummary::from_action)
            .collect();
        assert_eq!(bounded.len(), 3);
        assert_eq!(bounded[0].title, "act-0");
        assert_eq!(bounded[1].title, "act-1");
        assert_eq!(bounded[2].title, "act-2");
    }

    #[test]
    fn code_action_summary_max_actions_zero_yields_empty() {
        let actions = vec![code_action_with_edit(
            "a",
            Some(CodeActionKind::QUICKFIX),
            false,
            None,
        )];
        let bounded: Vec<CodeActionSummary> = actions
            .iter()
            .take(0)
            .map(CodeActionSummary::from_action)
            .collect();
        assert!(bounded.is_empty());
    }

    #[test]
    fn code_action_summaries_returns_unavailable_when_capability_missing() {
        let caps = ServerCapabilities::default();
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("pylsp"), Some("python"));
        assert!(!snap.supports(LspSemanticOperation::CodeAction));
        let u = snap
            .unavailable(LspSemanticOperation::CodeAction)
            .expect("unavailable");
        assert_eq!(u.operation, "codeAction");
        assert!(u.reason.contains("pylsp"));
        assert_eq!(u.server.as_deref(), Some("pylsp"));
        assert_eq!(u.language_id.as_deref(), Some("python"));
    }

    // ---- LspError::CommandOnlyCodeAction ----

    #[test]
    fn lsp_error_command_only_code_action_displays_title() {
        let err = LspError::CommandOnlyCodeAction("Run cargo build".to_string());
        let s = err.to_string();
        assert!(s.contains("command-only"));
        assert!(s.contains("Run cargo build"));
        assert!(!err.is_retryable());
    }
}
