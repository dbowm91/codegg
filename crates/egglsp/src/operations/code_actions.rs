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

/// One code action the server offered, with the preview of its edit when it
/// has one.
///
/// The action's own facts — its title, its kind, whether it is preferred,
/// whether it is disabled and why, whether it carried an edit, whether it
/// carried a command — are carried through verbatim. Only the *edit* is
/// transformed, and only into a preview: this surface never applies one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodeActionEntry {
    /// The action's title, as the server wrote it.
    pub title: String,
    /// The LSP `CodeActionKind` (e.g. `"quickfix"`), when the server gave
    /// one. `None` for a raw `Command` action.
    pub kind: Option<String>,
    /// Whether the server marked this action preferred.
    pub preferred: bool,
    /// `disabled.reason` from the LSP `CodeAction.disabled` field.
    pub disabled_reason: Option<String>,
    /// True when the action carried a `WorkspaceEdit` payload.
    pub has_edit: bool,
    /// True when the action carried a `Command` payload.
    pub has_command: bool,
    /// The preview of the action's edit.
    ///
    /// `None` for a command-bearing action, a disabled action, an action
    /// with no edit, and an action whose edit could not be previewed
    /// completely.
    pub preview: Option<CodeActionPreview>,
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
/// - Any `CodeAction` with `command: Some(_)` is rejected, including mixed
///   edit-and-command actions (command execution is disabled).
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
                if ca.command.is_some() {
                    trace!(
                        "source action: rejecting action '{}' (opaque command payload)",
                        ca.title
                    );
                    matching_command_only += 1;
                } else if let Some(_edit) = &ca.edit {
                    edit_bearing.push((ca, title));
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
            let (ca, _title) = edit_bearing
                .pop()
                .expect("edit_bearing has exactly one element");
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

/// Convert one raw action into an entry, previewing its edit locally.
///
/// Pure with respect to the network: the only I/O is the local file read
/// inside [`preview_workspace_edit`].
///
/// A **command-bearing** action is never previewed, even one that also
/// carries an edit. The LSP specification has the client apply an action's
/// edit *and then* its command; this surface never runs commands, so
/// previewing the edit of a mixed action would be previewing half of what
/// the action means. The same rule the single-action
/// [`LspOperations::preview_code_action`] already applies, kept here so the
/// list and the single agree.
fn code_action_entry(
    action: &CodeActionOrCommand,
    allowed_root: Option<&Path>,
    server_generation: u64,
) -> CodeActionEntry {
    match action {
        CodeActionOrCommand::Command(cmd) => CodeActionEntry {
            title: cmd.title.clone(),
            kind: None,
            preferred: false,
            disabled_reason: None,
            has_edit: false,
            has_command: true,
            preview: None,
        },
        CodeActionOrCommand::CodeAction(ca) => {
            let has_command = ca.command.is_some();
            let has_edit = ca.edit.is_some();
            let disabled = ca.disabled.is_some();
            let preview = match (has_command, disabled, ca.edit.as_ref()) {
                (false, false, Some(edit)) => {
                    preview_workspace_edit(ca.title.clone(), edit.clone(), allowed_root)
                        .ok()
                        .map(|preview| CodeActionPreview {
                            title: ca.title.clone(),
                            kind: ca.kind.as_ref().map(|k| k.as_str().to_string()),
                            affected_files: preview.files,
                            edit_count: preview.total_edits,
                            warnings: Vec::new(),
                            truncated: preview.truncated,
                            server_generation,
                        })
                }
                _ => None,
            };
            CodeActionEntry {
                title: ca.title.clone(),
                kind: ca.kind.as_ref().map(|k| k.as_str().to_string()),
                preferred: ca.is_preferred.unwrap_or(false),
                disabled_reason: ca.disabled.as_ref().map(|d| d.reason.clone()),
                has_edit,
                has_command,
                preview,
            }
        }
    }
}

impl LspOperations {
    /// Low-level `textDocument/codeAction` protocol wrapper.
    ///
    /// **No capability gating, no command-only filtering, no
    /// summary normalization.** Callers outside the typed
    /// [`Self::code_action_summaries`] /
    /// [`Self::preview_code_action`] helpers should generally
    /// prefer the typed API; this method exists for the typed
    /// surface to use internally and for the real-server smoke
    /// harness to drive raw protocol behavior.
    pub async fn code_actions_unchecked(
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
    #[allow(clippy::too_many_arguments)]
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
        // Fail-closed capability gate.
        self.require_capability(file_path, LspSemanticOperation::CodeAction)
            .await?;

        let actions = self
            .code_actions_unchecked(
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
    /// resolved action is a raw `Command` or has an opaque command
    /// payload (the surface never executes commands). The on-disk
    /// file is never mutated.
    #[allow(clippy::too_many_arguments)]
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

        // Fail-closed capability gate.
        self.require_capability(file_path, LspSemanticOperation::CodeAction)
            .await?;

        let actions = self
            .code_actions_unchecked(
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
                if ca.command.is_some() {
                    // Mixed and command-only CodeActions are both
                    // rejected: the command may have side effects
                    // that cannot be represented by a checked edit.
                    return Err(LspError::CommandOnlyCodeAction(ca.title.clone()));
                }
                if ca.edit.is_none() {
                    // Empty CodeAction.
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

    /// The code actions a warm server would offer at a position, each with
    /// the preview of its own edit.
    ///
    /// **One request.** The actions are fetched with a single
    /// `textDocument/codeAction` and then previewed **locally**, one
    /// `preview_workspace_edit` per action that carries an editable edit.
    ///
    /// The alternative — summaries first, then a second request naming the
    /// chosen action — carries a hazard this method does not: an action's
    /// *index* is only meaningful for the list the server returned in that
    /// moment, and a second request may return a different list. The index
    /// would then name a different action than the user picked. One request
    /// removes the index entirely, and the candidate is computed from
    /// exactly the action that will be shown.
    ///
    /// **Warm-only by construction:** the caller resolved `key` itself, and
    /// every step below — the capability gate, the document sync, the
    /// generation stamp — is taken against that same key. A file-path form
    /// would resolve a client of its own, and `get_or_create_client` is the
    /// one step that is allowed to start a language server.
    ///
    /// An action whose edit cannot be previewed completely yields an entry
    /// with no preview rather than failing the whole read: one unusable
    /// action must not hide the usable ones beside it.
    #[allow(clippy::too_many_arguments)]
    pub async fn code_action_previews_for_warm_client(
        &self,
        key: &str,
        file_path: &Path,
        range: Range,
        only: Option<Vec<CodeActionKind>>,
        max_actions: usize,
        allowed_root: Option<&Path>,
    ) -> Result<Vec<CodeActionEntry>, LspError> {
        use crate::capability::LspSemanticOperation;

        if max_actions == 0 {
            return Ok(Vec::new());
        }
        // Fail-closed capability gate, against the caller's own key.
        self.require_capability_for_key(key, LspSemanticOperation::CodeAction)
            .await?;

        let actions = self
            .code_actions_for_key(key, file_path, range, only)
            .await?;
        let server_generation = self.service.generation_for_key(key).await;

        Ok(actions
            .iter()
            .take(max_actions)
            .map(|action| code_action_entry(action, allowed_root, server_generation))
            .collect())
    }

    /// The raw `textDocument/codeAction` answer for an already-warm key.
    ///
    /// The file-path form of this request resolves its own client; this one
    /// takes the key it is given, so the whole code-action flow stays warm.
    async fn code_actions_for_key(
        &self,
        key: &str,
        file_path: &Path,
        range: Range,
        only: Option<Vec<CodeActionKind>>,
    ) -> Result<Vec<CodeActionOrCommand>, LspError> {
        let _uri_str = self
            .service
            .ensure_file_open_from_disk_for_key(key, file_path)
            .await?;
        let uri = url_to_uri(&url::Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        let params = serde_json::to_value(CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
            range,
            context: CodeActionContext {
                // The editor's code-action read asks without a diagnostics
                // context. A server that keys its quick fixes to the
                // diagnostics it was told about will offer fewer of them
                // here than it would with the context attached.
                diagnostics: Vec::new(),
                only,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(key, "textDocument/codeAction", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        Ok(serde_json::from_value(resp)?)
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
#[allow(clippy::field_reassign_with_default, dead_code)]
mod tests {
    use super::*;
    use crate::capability::{LspCapabilitySnapshot, LspSemanticOperation};
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
    fn mixed_edit_and_command_source_action_is_rejected() {
        let mut action = code_action_with_edit(
            "Organize imports",
            Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
            false,
            None,
        );
        if let CodeActionOrCommand::CodeAction(code_action) = &mut action {
            code_action.command = Some(Command {
                title: "also run a command".to_string(),
                command: "server.side_effect".to_string(),
                arguments: None,
            });
        }

        let error =
            select_source_action_edit(SourceActionPreviewKind::OrganizeImports, vec![action])
                .expect_err("opaque mixed command must not reach the edit path");
        assert!(matches!(error, LspError::CommandOnlySourceAction(_)));
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
    #[allow(clippy::useless_vec)]
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
