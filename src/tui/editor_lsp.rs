//! Bounded, document-scoped presentation state for the native LSP read API.
//!
//! This module stores only typed LSP results and presentation identity. It
//! never owns or caches editor text; ranges are resolved against a scoped
//! `DocumentSnapshot` borrow by the renderer.

use codegg_protocol::lsp::{
    LspDiagnosticDto, LspDiagnosticsResultDto, LspReadResultDto, LspSemanticTokenDto,
};

pub const MAX_EDITOR_DIAGNOSTICS: usize = 128;
pub const MAX_EDITOR_DIAGNOSTIC_MESSAGE_CHARS: usize = 512;
pub const MAX_EDITOR_READ_ITEMS: usize = 100;
pub const MAX_EDITOR_HOVER_CHARS: usize = 4_000;
pub const MAX_EDITOR_QUERY_ERROR_CHARS: usize = 512;

/// The complete authority binding for one editor presentation generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorLspScope {
    pub generation: u64,
    pub session_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub path: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DiagnosticsViewStatus {
    #[default]
    Idle,
    Loading,
    Ready,
    NotReady,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorLspPanel {
    Diagnostics,
    ReadResult,
}

/// Bounded LSP state attached to one editor generation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorLspState {
    pub scope: Option<EditorLspScope>,
    pub diagnostics: Vec<LspDiagnosticDto>,
    pub diagnostics_status: DiagnosticsViewStatus,
    pub diagnostics_stale: bool,
    pub diagnostics_loading: bool,
    pub diagnostics_request_id: u64,
    pub read_result: Option<LspReadResultDto>,
    pub read_error: Option<String>,
    pub read_loading: bool,
    pub read_request_id: u64,
    pub selected_index: usize,
    pub panel: Option<EditorLspPanel>,
    pub semantic_tokens: Vec<LspSemanticTokenDto>,
    pub semantic_revision: Option<u64>,
}

impl EditorLspState {
    pub fn reset_for_scope(&mut self, scope: EditorLspScope) {
        if self.scope.as_ref() == Some(&scope) {
            return;
        }
        *self = Self {
            scope: Some(scope),
            ..Self::default()
        };
    }

    pub fn begin_diagnostics(&mut self, scope: &EditorLspScope) -> Option<u64> {
        if self.scope.as_ref() != Some(scope) {
            self.reset_for_scope(scope.clone());
        }
        if self.diagnostics_loading {
            self.diagnostics_stale = !self.diagnostics.is_empty();
            return None;
        }
        self.diagnostics_request_id = self.diagnostics_request_id.wrapping_add(1);
        self.diagnostics_loading = true;
        self.diagnostics_stale = !self.diagnostics.is_empty();
        self.diagnostics_status = if self.diagnostics.is_empty() {
            DiagnosticsViewStatus::Loading
        } else {
            self.diagnostics_status
        };
        Some(self.diagnostics_request_id)
    }

    pub fn apply_diagnostics(
        &mut self,
        request_id: u64,
        scope: &EditorLspScope,
        result: Option<LspDiagnosticsResultDto>,
        error: Option<String>,
    ) -> bool {
        if self.scope.as_ref() != Some(scope)
            || self.diagnostics_request_id != request_id
            || !self.diagnostics_loading
        {
            return false;
        }
        self.diagnostics_loading = false;
        match (result, error) {
            (Some(result), None)
                if result.project_id == scope.project_id
                    && result.status == codegg_protocol::lsp::LspDiagnosticsStatus::Ready =>
            {
                let mut diagnostics = Vec::new();
                for file in result
                    .files
                    .into_iter()
                    .filter(|file| file.path == scope.path)
                {
                    for mut diagnostic in file.diagnostics {
                        if diagnostics.len() == MAX_EDITOR_DIAGNOSTICS {
                            break;
                        }
                        diagnostic.message = truncate_chars(
                            &diagnostic.message,
                            MAX_EDITOR_DIAGNOSTIC_MESSAGE_CHARS,
                        );
                        diagnostics.push(diagnostic);
                    }
                    if diagnostics.len() == MAX_EDITOR_DIAGNOSTICS {
                        break;
                    }
                }
                self.diagnostics = diagnostics;
                self.diagnostics_status = DiagnosticsViewStatus::Ready;
                self.diagnostics_stale = false;
            }
            (Some(result), None)
                if result.project_id == scope.project_id
                    && result.status == codegg_protocol::lsp::LspDiagnosticsStatus::NotReady =>
            {
                self.diagnostics_status = DiagnosticsViewStatus::NotReady;
                self.diagnostics_stale = !self.diagnostics.is_empty();
            }
            _ => {
                self.diagnostics_status = DiagnosticsViewStatus::Unavailable;
                self.diagnostics_stale = !self.diagnostics.is_empty();
            }
        }
        true
    }

    pub fn begin_read(&mut self, scope: &EditorLspScope) -> Option<u64> {
        if self.scope.as_ref() != Some(scope) {
            self.reset_for_scope(scope.clone());
        }
        if self.read_loading {
            return None;
        }
        self.read_request_id = self.read_request_id.wrapping_add(1);
        self.read_loading = true;
        self.read_result = None;
        self.read_error = None;
        self.selected_index = 0;
        Some(self.read_request_id)
    }

    pub fn apply_read(
        &mut self,
        request_id: u64,
        scope: &EditorLspScope,
        result: Option<LspReadResultDto>,
        error: Option<String>,
    ) -> bool {
        if self.scope.as_ref() != Some(scope)
            || self.read_request_id != request_id
            || !self.read_loading
        {
            return false;
        }
        self.read_loading = false;
        self.read_error =
            error.map(|message| truncate_chars(&message, MAX_EDITOR_QUERY_ERROR_CHARS));
        self.read_result = result;
        self.selected_index = 0;
        self.panel = Some(EditorLspPanel::ReadResult);
        true
    }

    pub fn open_diagnostics_panel(&mut self) {
        self.panel = Some(EditorLspPanel::Diagnostics);
        self.selected_index = 0;
    }

    pub fn close_panel(&mut self) {
        self.panel = None;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

pub fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

/// Convert an LSP UTF-16 column into a UTF-8 byte offset in a line.
///
/// A column that lands in the middle of a surrogate pair is rejected rather
/// than rounded into a different token/range.
pub fn utf16_column_to_byte(line: &str, column: u32) -> Option<usize> {
    let target = usize::try_from(column).ok()?;
    let mut units = 0usize;
    for (byte, ch) in line.char_indices() {
        if units == target {
            return Some(byte);
        }
        units = units.checked_add(ch.len_utf16())?;
        if units > target {
            return None;
        }
    }
    (units == target).then_some(line.len())
}

pub fn validate_semantic_tokens(
    snapshot: &codegg_document::DocumentSnapshot,
    mut tokens: Vec<LspSemanticTokenDto>,
) -> Vec<LspSemanticTokenDto> {
    if tokens.len() > MAX_EDITOR_READ_ITEMS {
        return Vec::new();
    }
    tokens.sort_by_key(|token| (token.line, token.character));
    let mut accepted = Vec::with_capacity(tokens.len());
    let mut previous: Option<(u32, usize)> = None;
    for token in tokens {
        if token.length == 0
            || token.token_type.len() > 64
            || token.modifiers.len() > 16
            || !matches!(
                token.token_type.as_str(),
                "keyword"
                    | "string"
                    | "number"
                    | "comment"
                    | "function"
                    | "method"
                    | "type"
                    | "class"
                    | "enum"
                    | "struct"
            )
        {
            return Vec::new();
        }
        let line_number = token.line as usize;
        let Some(start) = crate::tui::editor::line_start(snapshot, line_number) else {
            return Vec::new();
        };
        let Some(end) = crate::tui::editor::line_content_end(snapshot, line_number) else {
            return Vec::new();
        };
        if end.saturating_sub(start) > 16 * 1024 {
            return Vec::new();
        }
        let Ok(line) = snapshot.read_bytes(start, end) else {
            return Vec::new();
        };
        let Some(byte_start) = utf16_column_to_byte(&line, token.character) else {
            return Vec::new();
        };
        let Some(end_utf16) = token.character.checked_add(token.length) else {
            return Vec::new();
        };
        let Some(byte_end) = utf16_column_to_byte(&line, end_utf16) else {
            return Vec::new();
        };
        if byte_end <= byte_start
            || previous.is_some_and(|(prev_line, prev_end)| {
                token.line < prev_line || (token.line == prev_line && byte_start < prev_end)
            })
        {
            return Vec::new();
        }
        previous = Some((token.line, byte_end));
        accepted.push(token);
    }
    accepted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_preserve_baseline_on_not_ready_and_failure() {
        let scope = scope();
        let mut state = EditorLspState::default();
        let request = state.begin_diagnostics(&scope).unwrap();
        assert!(state.apply_diagnostics(request, &scope, Some(ready(&scope, 1)), None));
        assert_eq!(state.diagnostics.len(), 1);

        let request = state.begin_diagnostics(&scope).unwrap();
        assert!(state.diagnostics_stale);
        assert!(state.apply_diagnostics(request, &scope, Some(not_ready(&scope)), None));
        assert_eq!(state.diagnostics.len(), 1);
        assert_eq!(state.diagnostics_status, DiagnosticsViewStatus::NotReady);

        let request = state.begin_diagnostics(&scope).unwrap();
        assert!(state.apply_diagnostics(request, &scope, None, Some("offline".into())));
        assert_eq!(state.diagnostics.len(), 1);
        assert_eq!(state.diagnostics_status, DiagnosticsViewStatus::Unavailable);
    }

    #[test]
    fn stale_request_and_wrong_project_cannot_replace_current_diagnostics() {
        let scope = scope();
        let mut state = EditorLspState::default();
        let old = state.begin_diagnostics(&scope).unwrap();
        let current = state.begin_diagnostics(&scope);
        assert!(current.is_none(), "an in-flight pull is coalesced");
        let other_scope = EditorLspScope {
            generation: scope.generation + 1,
            ..scope.clone()
        };
        state.reset_for_scope(other_scope.clone());
        assert!(!state.apply_diagnostics(old, &scope, Some(ready(&scope, 1)), None));
        let request = state.begin_diagnostics(&other_scope).unwrap();
        let mut wrong_project = ready(&other_scope, 1);
        wrong_project.project_id = "other-project".into();
        assert!(state.apply_diagnostics(request, &other_scope, Some(wrong_project), None));
        assert!(state.diagnostics.is_empty());
        assert_eq!(state.diagnostics_status, DiagnosticsViewStatus::Unavailable);
    }

    #[test]
    fn read_completion_is_bound_to_scope_and_request() {
        use codegg_protocol::lsp::{
            LspReadOperation, LspReadPayloadDto, LspReadResultDto, LspReadStatus,
        };
        let scope = scope();
        let mut state = EditorLspState::default();
        let request = state.begin_read(&scope).unwrap();
        let result = LspReadResultDto {
            operation: LspReadOperation::Hover,
            status: LspReadStatus::Ready,
            payload: Some(LspReadPayloadDto::Hover {
                text: "symbol".into(),
                truncated: false,
            }),
        };
        assert!(!state.apply_read(request + 1, &scope, Some(result.clone()), None));
        assert!(state.apply_read(request, &scope, Some(result), None));
        assert_eq!(
            state.read_result.as_ref().unwrap().operation,
            LspReadOperation::Hover
        );

        let next_scope = EditorLspScope {
            generation: scope.generation + 1,
            ..scope.clone()
        };
        state.reset_for_scope(next_scope.clone());
        let next_request = state.begin_read(&next_scope).unwrap();
        let stale = LspReadResultDto {
            operation: LspReadOperation::Hover,
            status: LspReadStatus::Ready,
            payload: None,
        };
        assert!(!state.apply_read(next_request, &scope, Some(stale), None));
        assert!(state.read_loading);
    }

    #[test]
    fn utf16_ranges_require_valid_utf8_boundaries() {
        assert_eq!(utf16_column_to_byte("a😀b", 0), Some(0));
        assert_eq!(utf16_column_to_byte("a😀b", 1), Some(1));
        assert_eq!(utf16_column_to_byte("a😀b", 2), None);
        assert_eq!(utf16_column_to_byte("a😀b", 3), Some(5));
        assert_eq!(utf16_column_to_byte("a😀b", 4), Some(6));
        assert_eq!(utf16_column_to_byte("a😀b", 5), None);
    }

    #[test]
    fn semantic_overlay_rejects_unknown_invalid_and_overlapping_spans() {
        let snapshot = codegg_document::DocumentBuffer::new("a😀bc").snapshot();
        let token = |character, length, token_type: &str| LspSemanticTokenDto {
            line: 0,
            character,
            length,
            token_type: token_type.to_string(),
            modifiers: vec![],
        };
        assert_eq!(
            validate_semantic_tokens(&snapshot, vec![token(1, 2, "keyword")]).len(),
            1
        );
        assert!(validate_semantic_tokens(&snapshot, vec![token(2, 1, "keyword")]).is_empty());
        assert!(validate_semantic_tokens(&snapshot, vec![token(0, 1, "vendor_custom")]).is_empty());
        assert!(validate_semantic_tokens(
            &snapshot,
            vec![token(0, 1, "keyword"), token(0, 1, "string")]
        )
        .is_empty());
    }

    fn scope() -> EditorLspScope {
        EditorLspScope {
            generation: 1,
            session_id: "session".into(),
            project_id: "project".into(),
            workspace_id: "workspace".into(),
            path: "src/lib.rs".into(),
        }
    }

    fn ready(scope: &EditorLspScope, line: u32) -> LspDiagnosticsResultDto {
        use codegg_protocol::lsp::{
            LspDiagnosticDto, LspDiagnosticsStatus, LspFileDiagnosticsDto, LspRangeDto,
        };
        LspDiagnosticsResultDto {
            project_id: scope.project_id.clone(),
            status: LspDiagnosticsStatus::Ready,
            files: vec![LspFileDiagnosticsDto {
                path: scope.path.clone(),
                sequence: 1,
                digest: "digest".into(),
                diagnostics: vec![LspDiagnosticDto {
                    range: LspRangeDto {
                        path: scope.path.clone(),
                        workspace_relative_path: None,
                        start_line: line,
                        start_column: 0,
                        end_line: line,
                        end_column: 1,
                    },
                    severity: 1,
                    tag: 0,
                    code: None,
                    message: "broken".into(),
                    source: Some("test".into()),
                }],
                post_restart: false,
                truncated: false,
            }],
            truncated: false,
        }
    }

    fn not_ready(scope: &EditorLspScope) -> LspDiagnosticsResultDto {
        let mut result = ready(scope, 0);
        result.status = codegg_protocol::lsp::LspDiagnosticsStatus::NotReady;
        result.files.clear();
        result
    }
}
