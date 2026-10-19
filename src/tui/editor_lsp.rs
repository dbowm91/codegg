//! Bounded, document-scoped presentation state for the native LSP read API.
//!
//! This module stores only typed LSP results and presentation identity. It
//! never owns or caches editor text; ranges are resolved against a scoped
//! `DocumentSnapshot` borrow by the renderer.

use codegg_protocol::lsp::{
    LspDiagnosticDto, LspDiagnosticsResultDto, LspPreviewCandidateDto, LspReadPayloadDto,
    LspReadResultDto, LspSemanticTokenDto,
};

pub const MAX_EDITOR_DIAGNOSTICS: usize = 128;
pub const MAX_EDITOR_DIAGNOSTIC_MESSAGE_CHARS: usize = 512;
/// Bound for read-panel item lists (hover/locations/symbols panels).
/// This is deliberately distinct from the semantic-token cap below: panel
/// lists are truncated for display, while semantic tokens are validated
/// against the protocol maximum.
pub const MAX_EDITOR_READ_ITEMS: usize = 100;
/// Frontend received-token cap for semantic highlighting. Mirrors the
/// protocol maximum `MAX_LSP_SEMANTIC_TOKENS` so an ordinary valid daemon
/// response (up to 1000 tokens) is never discarded by a smaller local cap.
pub const MAX_EDITOR_SEMANTIC_TOKENS: usize = codegg_protocol::lsp::MAX_LSP_SEMANTIC_TOKENS;
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

/// Which way an unimpaired diagnostic leaf walks the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorDiagnosticStep {
    /// `]d` — forward. `]` is forward throughout this layer.
    Forward,
    /// `[d` — backward.
    Backward,
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
    /// The reviewable candidate the last preview read carried, if any.
    ///
    /// It is scoped like everything else here: `reset_for_scope` wipes it, so
    /// a candidate can never be offered against a document it was not
    /// computed for. A preview that could not become a candidate carries
    /// none — and says why it could not.
    pub preview_candidate: Option<LspPreviewCandidateDto>,
    pub read_error: Option<String>,
    pub read_loading: bool,
    pub read_request_id: u64,
    pub selected_index: usize,
    pub panel: Option<EditorLspPanel>,
    pub semantic_tokens: Vec<LspSemanticTokenDto>,
    pub semantic_revision: Option<u64>,
    /// The daemon reported `truncated=true` for the stored token set.
    /// When true the highlight is explicitly incomplete coverage, never a
    /// complete file highlight.
    pub semantic_truncated: bool,
    /// At least one token was dropped during structural validation, the
    /// whole response was rejected for exceeding the protocol cap, or the
    /// daemon reported truncation. Drives the visible incomplete-coverage
    /// signal. A `Ready` with zero tokens and both flags false is a valid
    /// empty highlight, never `NotReady`.
    pub semantic_incomplete: bool,
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
        // A new read invalidates the previous candidate: it was computed
        // against the previous answer's content.
        self.preview_candidate = None;
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
        self.preview_candidate = result.as_ref().and_then(candidate_of);
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

    /// Drop all semantic presentation state.
    ///
    /// Called on local edit, reload, resync, attachment switch (via
    /// `reset`/`detach`), reconnect, and stale-revision refusal. Never
    /// touches diagnostics or read panels.
    pub fn clear_semantic(&mut self) {
        self.semantic_tokens.clear();
        self.semantic_revision = None;
        self.semantic_truncated = false;
        self.semantic_incomplete = false;
    }

    /// Store one validated semantic result bound to `revision`.
    ///
    /// `truncated` is the daemon's own flag and is preserved verbatim so a
    /// truncated result is never represented as complete. `incomplete` also
    /// covers locally dropped spans and whole-response rejection.
    pub fn store_semantic(
        &mut self,
        revision: u64,
        validated: ValidatedSemanticTokens,
        truncated: bool,
    ) {
        self.semantic_tokens = validated.accepted;
        self.semantic_revision = Some(revision);
        self.semantic_truncated = truncated;
        self.semantic_incomplete = truncated || validated.dropped > 0 || validated.rejected_all;
    }

    /// Human-readable coverage note for the banner, if any.
    pub fn semantic_coverage_note(&self) -> Option<&'static str> {
        self.semantic_revision?;
        if self.semantic_truncated {
            return Some("highlighting truncated");
        }
        if self.semantic_incomplete {
            return Some("highlighting partial");
        }
        None
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

/// The reviewable candidate a read result carried, if any.
///
/// Only the preview reads carry one: a hover or a definition has nothing to
/// apply. A preview that could not become a candidate — a truncated rename, a
/// preview with no edits — carries none, and the reader is told why instead.
fn candidate_of(result: &LspReadResultDto) -> Option<LspPreviewCandidateDto> {
    match result.payload.as_ref()? {
        LspReadPayloadDto::FormatPreview(preview) => preview.candidate.clone(),
        LspReadPayloadDto::RenamePreview(preview) => preview.candidate.clone(),
        _ => None,
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

/// Outcome of structural semantic-token validation.
///
/// Only two shapes exist: per-token drops (safe because token offsets are
/// absolute, so dropping one span never shifts a later one) and one
/// whole-response rejection when the response exceeds the protocol cap.
/// An unknown *style* is never a malformation: well-formed unknown token
/// types are accepted here and render as plain text by the widget's
/// style-class mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedSemanticTokens {
    pub accepted: Vec<LspSemanticTokenDto>,
    /// Malformed spans dropped individually.
    pub dropped: usize,
    /// True only when the whole response exceeded
    /// `MAX_EDITOR_SEMANTIC_TOKENS` and nothing was accepted.
    pub rejected_all: bool,
}

impl ValidatedSemanticTokens {
    pub fn incomplete(&self) -> bool {
        self.dropped > 0 || self.rejected_all
    }
}

/// Validate semantic tokens against a borrowed snapshot.
///
/// Borrows `snapshot` for structural checks only; never mutates text and
/// never retains it. Tokens are sorted by `(line, character)` so a
/// server-ordered response and a shuffled one validate identically.
/// Overlapping spans are dropped individually (the later span is skipped,
/// the earlier end is retained) without changing later offsets.
pub fn validate_semantic_tokens(
    snapshot: &codegg_document::DocumentSnapshot,
    mut tokens: Vec<LspSemanticTokenDto>,
) -> ValidatedSemanticTokens {
    if tokens.len() > MAX_EDITOR_SEMANTIC_TOKENS {
        return ValidatedSemanticTokens {
            dropped: tokens.len(),
            accepted: Vec::new(),
            rejected_all: true,
        };
    }
    tokens.sort_by_key(|token| (token.line, token.character));
    let mut accepted = Vec::with_capacity(tokens.len());
    let mut dropped = 0usize;
    let mut previous: Option<(u32, usize)> = None;
    for token in tokens {
        // Structural bounds only. Unknown `token_type` names are accepted:
        // style mapping decides plain vs themed, not validity.
        if token.length == 0 || token.token_type.len() > 64 || token.modifiers.len() > 16 {
            dropped += 1;
            continue;
        }
        let line_number = token.line as usize;
        let (Some(start), Some(end)) = (
            crate::tui::editor::line_start(snapshot, line_number),
            crate::tui::editor::line_content_end(snapshot, line_number),
        ) else {
            dropped += 1;
            continue;
        };
        if end.saturating_sub(start) > 16 * 1024 {
            dropped += 1;
            continue;
        }
        let Ok(line) = snapshot.read_bytes(start, end) else {
            dropped += 1;
            continue;
        };
        let Some(byte_start) = utf16_column_to_byte(&line, token.character) else {
            dropped += 1;
            continue;
        };
        let Some(end_utf16) = token.character.checked_add(token.length) else {
            dropped += 1;
            continue;
        };
        let Some(byte_end) = utf16_column_to_byte(&line, end_utf16) else {
            dropped += 1;
            continue;
        };
        if byte_end <= byte_start {
            dropped += 1;
            continue;
        }
        if previous.is_some_and(|(prev_line, prev_end)| {
            token.line < prev_line || (token.line == prev_line && byte_start < prev_end)
        }) {
            dropped += 1;
            continue;
        }
        previous = Some((token.line, byte_end));
        accepted.push(token);
    }
    ValidatedSemanticTokens {
        accepted,
        dropped,
        rejected_all: false,
    }
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
    fn semantic_overlay_drops_malformed_spans_individually_and_keeps_unknown_plain() {
        let snapshot = codegg_document::DocumentBuffer::new("a😀bc").snapshot();
        let token = |character, length, token_type: &str| LspSemanticTokenDto {
            line: 0,
            character,
            length,
            token_type: token_type.to_string(),
            modifiers: vec![],
        };
        // Well-formed styled token is accepted.
        let valid = validate_semantic_tokens(&snapshot, vec![token(1, 2, "keyword")]);
        assert_eq!(valid.accepted.len(), 1);
        assert_eq!(valid.dropped, 0);
        assert!(!valid.rejected_all);
        // Split surrogate is dropped individually, not as a whole-response loss.
        let split = validate_semantic_tokens(&snapshot, vec![token(2, 1, "keyword")]);
        assert!(split.accepted.is_empty());
        assert_eq!(split.dropped, 1);
        assert!(!split.rejected_all);
        // Well-formed unknown style is accepted here; the widget renders it
        // plain while keeping neighbors.
        let unknown = validate_semantic_tokens(&snapshot, vec![token(0, 1, "vendor_custom")]);
        assert_eq!(unknown.accepted.len(), 1);
        assert_eq!(unknown.dropped, 0);
        // Overlapping later span is dropped; the earlier one survives.
        let overlap = validate_semantic_tokens(
            &snapshot,
            vec![token(0, 1, "keyword"), token(0, 1, "string")],
        );
        assert_eq!(overlap.accepted.len(), 1);
        assert_eq!(overlap.dropped, 1);
        assert!(!overlap.rejected_all);
    }

    fn semantic_token(line: u32, character: u32, length: u32, kind: &str) -> LspSemanticTokenDto {
        LspSemanticTokenDto {
            line,
            character,
            length,
            token_type: kind.to_string(),
            modifiers: vec![],
        }
    }

    /// Realistic ASCII Rust source for mixed-kind coverage. A separate
    /// supplementary-plane fixture below proves UTF-16 safety.
    fn rust_snapshot() -> codegg_document::DocumentSnapshot {
        let text =
            "fn main() {\n    let value = 42; // hello\n    println!(\"value {}\", value);\n}\n";
        codegg_document::DocumentBuffer::new(text).snapshot()
    }

    fn emoji_snapshot() -> codegg_document::DocumentSnapshot {
        codegg_document::DocumentBuffer::new("a😀bc\nlet x = 1;\n").snapshot()
    }

    #[test]
    fn mixed_standard_kinds_including_variable_parameter_property_namespace_are_kept() {
        let snapshot = rust_snapshot();
        // Line 1: `    let value = 42; // hello`
        // col 4 `let` (keyword), col 8 `value` (variable), col 16 `42`
        // (number), col 20 `// hello` (comment).
        // Line 2: `    println!("value {}", value);`
        // col 4 `println` (function), col 13 `"value {}"` (string),
        // col 25 `value` (property).
        let tokens = vec![
            semantic_token(1, 4, 3, "keyword"),
            semantic_token(1, 8, 5, "variable"),
            semantic_token(1, 16, 2, "number"),
            semantic_token(1, 20, 8, "comment"),
            semantic_token(2, 4, 7, "function"),
            semantic_token(2, 13, 10, "string"),
            semantic_token(0, 0, 2, "keyword"),
            semantic_token(0, 3, 4, "parameter"),
            semantic_token(2, 25, 5, "property"),
            semantic_token(3, 0, 1, "namespace"),
        ];
        let validated = validate_semantic_tokens(&snapshot, tokens);
        assert!(!validated.rejected_all, "mixed kinds must not fail closed");
        assert_eq!(validated.dropped, 0, "all spans are well-formed");
        assert_eq!(validated.accepted.len(), 10);
    }

    #[test]
    fn supplementary_character_spans_are_unicode_safe() {
        // Line 0 `a😀bc`: UTF-16 columns 0:a, 1:emoji-high, 3:b, 4:c.
        // Column 2 splits the surrogate and must be rejected; columns
        // straddling the emoji on valid boundaries must be accepted, with
        // styled tokens on each side of an unstyled one.
        let snapshot = emoji_snapshot();
        let tokens = vec![
            semantic_token(0, 0, 1, "keyword"),
            semantic_token(0, 1, 2, "variable"),
            semantic_token(0, 3, 1, "string"),
        ];
        let validated = validate_semantic_tokens(&snapshot, tokens);
        assert_eq!(validated.accepted.len(), 3);
        assert_eq!(validated.dropped, 0);
        let split = validate_semantic_tokens(&snapshot, vec![semantic_token(0, 2, 1, "keyword")]);
        assert!(split.accepted.is_empty());
        assert_eq!(split.dropped, 1);
    }

    #[test]
    fn counts_101_500_and_1000_are_accepted_but_over_cap_fails_closed() {
        // One token per line at columns 0..3 (`let`); distinct lines never
        // overlap, so the count alone decides acceptance.
        let wide = (0..(MAX_EDITOR_SEMANTIC_TOKENS + 1))
            .map(|i| format!("let v{i} = 0;\n"))
            .collect::<String>();
        let snapshot = codegg_document::DocumentBuffer::new(&wide).snapshot();
        for count in [101usize, 500, 1000] {
            let tokens: Vec<_> = (0..count)
                .map(|i| semantic_token(i as u32, 0, 3, "keyword"))
                .collect();
            let validated = validate_semantic_tokens(&snapshot, tokens);
            assert!(
                !validated.rejected_all,
                "{count} tokens must not fail closed"
            );
            assert_eq!(validated.dropped, 0);
            assert_eq!(validated.accepted.len(), count, "{count} tokens kept");
        }
        let over_tokens: Vec<_> = (0..(MAX_EDITOR_SEMANTIC_TOKENS + 1))
            .map(|i| semantic_token(i as u32, 0, 3, "keyword"))
            .collect();
        let rejected = validate_semantic_tokens(&snapshot, over_tokens);
        assert!(rejected.rejected_all);
        assert!(rejected.accepted.is_empty());
        assert_eq!(rejected.dropped, MAX_EDITOR_SEMANTIC_TOKENS + 1);
    }

    #[test]
    fn empty_ready_is_valid_and_truncation_is_preserved_as_incomplete() {
        let mut state = EditorLspState::default();
        // Ready with zero tokens: valid empty highlight, not NotReady.
        let empty = validate_semantic_tokens(&rust_snapshot(), Vec::new());
        assert!(!empty.rejected_all);
        assert_eq!(empty.dropped, 0);
        assert!(!empty.incomplete());
        state.store_semantic(7, empty, false);
        assert_eq!(state.semantic_revision, Some(7));
        assert!(state.semantic_tokens.is_empty());
        assert!(!state.semantic_truncated);
        assert!(!state.semantic_incomplete);
        assert_eq!(state.semantic_coverage_note(), None);
        // Truncated daemon result is stored with the flag and reports
        // incomplete coverage explicitly.
        let tokens = vec![semantic_token(0, 0, 2, "keyword")];
        let validated = validate_semantic_tokens(&rust_snapshot(), tokens);
        state.store_semantic(8, validated, true);
        assert!(state.semantic_truncated);
        assert!(state.semantic_incomplete);
        assert_eq!(
            state.semantic_coverage_note(),
            Some("highlighting truncated")
        );
        // Dropped spans without daemon truncation still report partial.
        let bad = vec![
            semantic_token(0, 0, 2, "keyword"),
            semantic_token(99_999, 0, 2, "keyword"),
        ];
        let partial = validate_semantic_tokens(&rust_snapshot(), bad);
        assert_eq!(partial.accepted.len(), 1);
        assert_eq!(partial.dropped, 1);
        state.store_semantic(9, partial, false);
        assert!(!state.semantic_truncated);
        assert!(state.semantic_incomplete);
        assert_eq!(state.semantic_coverage_note(), Some("highlighting partial"));
        // Clearing returns to no-data, which renders plain with no signal.
        state.clear_semantic();
        assert_eq!(state.semantic_revision, None);
        assert_eq!(state.semantic_coverage_note(), None);
    }

    #[test]
    fn malformed_line_offset_and_zero_length_are_dropped_individually() {
        let snapshot = rust_snapshot();
        let good = semantic_token(0, 0, 2, "keyword");
        let bad_line = semantic_token(99_999, 0, 2, "keyword");
        let bad_offset = semantic_token(0, 9999, 2, "keyword");
        let zero = semantic_token(0, 5, 0, "keyword");
        let validated = validate_semantic_tokens(
            &snapshot,
            vec![good.clone(), bad_line, bad_offset, zero, good.clone()],
        );
        // Two well-formed tokens survive (one on line 0 col 0, one duplicate
        // would overlap, so only one of the duplicates survives).
        assert!(!validated.rejected_all);
        assert_eq!(validated.dropped, 4, "bad line/offset/zero + overlap");
        assert_eq!(validated.accepted.len(), 1);
    }

    #[test]
    fn semantic_cap_matches_the_protocol_maximum() {
        // Both caps are `const`; compare via a non-const indirection so the
        // assertion survives `clippy::assertions_on_constants`.
        let semantic = MAX_EDITOR_SEMANTIC_TOKENS;
        let protocol = codegg_protocol::lsp::MAX_LSP_SEMANTIC_TOKENS;
        let panel = MAX_EDITOR_READ_ITEMS;
        assert_eq!(semantic, protocol);
        assert!(
            panel < semantic,
            "panel-item cap must stay distinct from the semantic cap"
        );
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

    /// A reviewable candidate, as the daemon mints one.
    fn candidate() -> LspPreviewCandidateDto {
        use codegg_protocol::lsp::LspPreviewPatchDto;
        LspPreviewCandidateDto {
            preview_id: "editor-preview-1".into(),
            preview_revision: 1,
            preview_digest: "c".repeat(64),
            kind: "formatting".into(),
            title: "Format src/lib.rs".into(),
            provenance: "editor:format:src/lib.rs".into(),
            patches: vec![LspPreviewPatchDto {
                path: "src/lib.rs".into(),
                patch: "@@ -1 +1 @@\n-old\n+new\n".into(),
                original_hash: "a".repeat(64),
            }],
        }
    }

    /// The candidate is scoped like everything else here.
    ///
    /// A candidate is the material the apply path consumes, so offering one
    /// against the wrong document would be offering a change that belongs to
    /// another file. A new read invalidates it, and a new scope wipes it.
    #[test]
    fn a_candidate_is_scoped_to_the_read_that_produced_it() {
        use codegg_protocol::lsp::{
            LspFormatPreviewDto, LspReadOperation, LspReadPayloadDto, LspReadResultDto,
            LspReadStatus,
        };
        let scope = scope();
        let mut state = EditorLspState::default();
        let request = state.begin_read(&scope).unwrap();
        let result = LspReadResultDto {
            operation: LspReadOperation::FormatPreview,
            status: LspReadStatus::Ready,
            payload: Some(LspReadPayloadDto::FormatPreview(LspFormatPreviewDto {
                path: "src/lib.rs".into(),
                edit_count: 1,
                before_hash: "a".into(),
                after_hash: "b".into(),
                diff: "-a\n+b\n".into(),
                truncated: false,
                base_stale: false,
                candidate: Some(candidate()),
            })),
        };
        assert!(state.apply_read(request, &scope, Some(result), None));
        assert!(
            state.preview_candidate.is_some(),
            "the candidate the read carried must be held"
        );

        // A new read invalidates it: it was computed against the old answer.
        let request = state.begin_read(&scope).unwrap();
        assert!(
            state.preview_candidate.is_none(),
            "a new read must not inherit the previous candidate"
        );

        // And a new scope wipes it entirely.
        let result = LspReadResultDto {
            operation: LspReadOperation::FormatPreview,
            status: LspReadStatus::Ready,
            payload: Some(LspReadPayloadDto::FormatPreview(LspFormatPreviewDto {
                path: "src/lib.rs".into(),
                edit_count: 1,
                before_hash: "a".into(),
                after_hash: "b".into(),
                diff: "-a\n+b\n".into(),
                truncated: false,
                base_stale: false,
                candidate: Some(candidate()),
            })),
        };
        assert!(state.apply_read(request, &scope, Some(result), None));
        let other_scope = EditorLspScope {
            generation: scope.generation + 1,
            ..scope.clone()
        };
        state.reset_for_scope(other_scope);
        assert!(
            state.preview_candidate.is_none(),
            "a candidate must not survive the scope it was read under"
        );
    }
}
