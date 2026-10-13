//! Bounded, document-scoped presentation state for the M003 completion read.
//!
//! This module stores only typed completion results and presentation
//! identity. It never owns or caches editor text; edit ranges are resolved
//! against a scoped `DocumentSnapshot` borrow at apply time, and the
//! snapshot is dropped before the transaction is submitted.
//!
//! The popup is advisory until the user accepts: `selected_candidate` hands
//! out inert data, and only an explicit accept through the command layer
//! (after re-validation against the current snapshot) may build a
//! transaction.

use codegg_protocol::lsp::{
    LspCompletionCandidateDto, LspCompletionResultDto, LspCompletionStatus,
};

use super::editor_lsp::truncate_chars;
use super::editor_lsp::EditorLspScope;

/// Bound for completion error lines kept for display.
pub const MAX_EDITOR_COMPLETION_ERROR_CHARS: usize = 512;
/// Bound for one rendered candidate row's label.
pub const MAX_COMPLETION_LABEL_CHARS: usize = 64;
/// Bound for one rendered candidate row's detail text.
pub const MAX_COMPLETION_DETAIL_CHARS: usize = 64;
/// Maximum candidate rows the popup shows at once; the list scrolls.
pub const MAX_COMPLETION_POPUP_ROWS: usize = 10;
/// Maximum popup width in display columns, including borders.
pub const MAX_COMPLETION_POPUP_WIDTH: u16 = 60;

/// What the popup is showing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CompletionViewStatus {
    /// Nothing requested, or the popup was dismissed.
    #[default]
    Idle,
    /// A request is in flight; the popup shows a waiting row.
    Loading,
    /// A warm, synchronized answer arrived (possibly zero candidates).
    Ready,
    /// No warm server. Retryable later; never an error and never empty.
    NotReady,
    /// The request's revision was not current. Re-trigger to rebase.
    Stale,
    /// The daemon's revision never reached the server.
    Unsynced,
    /// The warm server has no completion provider.
    Unsupported,
    /// Transport or denial failure, with the message kept verbatim.
    Error,
}

/// The complete authority binding for one completion request.
///
/// Extends the read scope with the document identity, the revision the
/// request was issued against, and the UTF-16 cursor the server saw, so a
/// late result for a moved cursor or an old revision cannot become a
/// suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorCompletionScope {
    pub read_scope: EditorLspScope,
    pub document_id: String,
    pub revision: u64,
    pub line: u32,
    pub character: u32,
}

/// Bounded completion state attached to one editor generation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorCompletionState {
    pub scope: Option<EditorCompletionScope>,
    pub status: CompletionViewStatus,
    pub candidates: Vec<LspCompletionCandidateDto>,
    pub truncated: bool,
    pub incomplete: bool,
    pub selected: usize,
    pub error: Option<String>,
    pub loading: bool,
    pub request_id: u64,
    pub popup_open: bool,
}

impl EditorCompletionState {
    pub fn reset_for_scope(&mut self, scope: EditorCompletionScope) {
        if self.scope.as_ref() == Some(&scope) {
            return;
        }
        *self = Self {
            scope: Some(scope),
            ..Self::default()
        };
    }

    /// Begin one request, superseding any in flight.
    ///
    /// A re-trigger while loading starts a newer request rather than
    /// queueing behind the old one: completions carry request ids, so the
    /// superseded answer is dropped on arrival and can never paint over the
    /// newer one. Returns the new request id.
    pub fn begin(&mut self, scope: &EditorCompletionScope) -> u64 {
        if self.scope.as_ref() != Some(scope) {
            self.reset_for_scope(scope.clone());
        }
        self.request_id = self.request_id.wrapping_add(1);
        self.loading = true;
        self.status = CompletionViewStatus::Loading;
        self.candidates.clear();
        self.truncated = false;
        self.incomplete = false;
        self.selected = 0;
        self.error = None;
        self.popup_open = true;
        self.request_id
    }

    /// Store one finished request.
    ///
    /// Returns `false` — dropping the result — when the scope moved on or
    /// the request id is not the latest. A stale response may be counted
    /// diagnostically by the caller but is never shown as actionable.
    pub fn apply(
        &mut self,
        request_id: u64,
        scope: &EditorCompletionScope,
        result: Option<LspCompletionResultDto>,
        error: Option<String>,
    ) -> bool {
        if self.scope.as_ref() != Some(scope) || self.request_id != request_id || !self.loading {
            return false;
        }
        self.loading = false;
        match (result, error) {
            (Some(result), _) => {
                // Identity is checked before anything is shown: a result for
                // another document or revision is discarded, not displayed.
                if result.document_id != scope.document_id || result.revision != scope.revision {
                    self.status = CompletionViewStatus::Error;
                    self.error = Some("completion answer is for another document revision".into());
                    self.candidates.clear();
                    return true;
                }
                self.status = match result.status {
                    LspCompletionStatus::Ready => CompletionViewStatus::Ready,
                    LspCompletionStatus::NotReady => CompletionViewStatus::NotReady,
                    LspCompletionStatus::Stale => CompletionViewStatus::Stale,
                    LspCompletionStatus::Unsynced => CompletionViewStatus::Unsynced,
                    LspCompletionStatus::Unsupported => CompletionViewStatus::Unsupported,
                };
                self.truncated = result.truncated;
                self.incomplete = result.incomplete;
                // Defense in depth: the protocol caps at 100 candidates, but
                // a misbehaving daemon must not grow frontend state without
                // bound. Dropping here sets `truncated` so the popup stays
                // honest about what it shows.
                let mut candidates = result.candidates;
                if candidates.len() > codegg_protocol::lsp::MAX_LSP_COMPLETION_CANDIDATES {
                    candidates.truncate(codegg_protocol::lsp::MAX_LSP_COMPLETION_CANDIDATES);
                    self.truncated = true;
                }
                self.candidates = candidates;
                self.selected = 0;
                self.error = None;
            }
            (None, Some(message)) => {
                self.status = CompletionViewStatus::Error;
                self.error = Some(truncate_chars(&message, MAX_EDITOR_COMPLETION_ERROR_CHARS));
                self.candidates.clear();
            }
            (None, None) => {
                self.status = CompletionViewStatus::Error;
                self.error = Some("empty completion response".to_string());
                self.candidates.clear();
            }
        }
        true
    }

    pub fn select_next(&mut self) {
        self.selected = self
            .selected
            .saturating_add(1)
            .min(self.candidates.len().saturating_sub(1));
    }

    pub fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn selected_candidate(&self) -> Option<&LspCompletionCandidateDto> {
        self.candidates.get(self.selected)
    }

    pub fn is_open(&self) -> bool {
        self.popup_open
    }

    /// Dismiss the popup without side effects.
    ///
    /// Cancelling is pure presentation: no transaction is built, no request
    /// is replayed, and the revision-tracked result (if any) is dropped so a
    /// later re-trigger re-validates from scratch.
    pub fn dismiss(&mut self) {
        self.popup_open = false;
        self.loading = false;
        self.status = CompletionViewStatus::Idle;
        self.candidates.clear();
        self.error = None;
        self.selected = 0;
        self.truncated = false;
        self.incomplete = false;
    }

    /// Drop all completion state.
    ///
    /// Called on local edit, undo/redo, save, reload, resync, close,
    /// attachment switch, session change, and disconnect: anything that
    /// moves the text or the binding invalidates the popup and any pending
    /// request id with it.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// One-line status for the popup header, if the popup is open.
    pub fn status_line(&self) -> Option<&'static str> {
        if !self.popup_open {
            return None;
        }
        Some(match self.status {
            CompletionViewStatus::Idle => "completion",
            CompletionViewStatus::Loading => "completion…",
            CompletionViewStatus::Ready if self.candidates.is_empty() => "no suggestions",
            CompletionViewStatus::Ready => "suggestions",
            CompletionViewStatus::NotReady => "language server warming…",
            CompletionViewStatus::Stale => "document changed — re-trigger",
            CompletionViewStatus::Unsynced => "server not synchronized",
            CompletionViewStatus::Unsupported => "no completion provider",
            CompletionViewStatus::Error => "completion failed",
        })
    }
}

/// Render one untrusted server string as literal popup text.
///
/// Control characters (including newlines and escape sequences) become
/// spaces so a malicious label cannot break the popup layout or smuggle
/// terminal escapes into the frame; the result is truncated to `max_chars`.
/// Ratatui spans are literal, but layout is computed from the string, so
/// sanitizing here is what keeps one row exactly one row.
pub fn sanitize_completion_text(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (count, ch) in value.chars().enumerate() {
        if count >= max_chars {
            break;
        }
        out.push(if ch.is_control() { ' ' } else { ch });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::editor_lsp::EditorLspScope;
    use codegg_protocol::lsp::{
        LspCompletionApplicabilityDto, LspCompletionEditDto, LspCompletionResultDto,
        LspCompletionStatus,
    };

    fn read_scope() -> EditorLspScope {
        EditorLspScope {
            generation: 1,
            session_id: "session".into(),
            project_id: "project".into(),
            workspace_id: "workspace".into(),
            path: "src/lib.rs".into(),
        }
    }

    fn scope() -> EditorCompletionScope {
        EditorCompletionScope {
            read_scope: read_scope(),
            document_id: "doc-1".into(),
            revision: 7,
            line: 3,
            character: 5,
        }
    }

    fn ready_result(document_id: &str, revision: u64) -> LspCompletionResultDto {
        LspCompletionResultDto::ready(
            document_id.into(),
            revision,
            Some(revision),
            Vec::new(),
            false,
            false,
        )
    }

    #[test]
    fn begin_supersedes_an_in_flight_request() {
        let scope = scope();
        let mut state = EditorCompletionState::default();
        let first = state.begin(&scope);
        let second = state.begin(&scope);
        assert_ne!(first, second);
        assert!(state.loading);
        assert!(state.popup_open);
        // The superseded answer is dropped on arrival.
        assert!(!state.apply(first, &scope, Some(ready_result("doc-1", 7)), None));
        assert!(state.loading);
        assert!(state.apply(second, &scope, Some(ready_result("doc-1", 7)), None));
        assert!(!state.loading);
        assert_eq!(state.status, CompletionViewStatus::Ready);
    }

    #[test]
    fn apply_is_fenced_by_scope_and_request() {
        let scope = scope();
        let mut state = EditorCompletionState::default();
        let request = state.begin(&scope);
        let mut other = scope.clone();
        other.revision = 8;
        assert!(!state.apply(request, &other, Some(ready_result("doc-1", 8)), None));
        assert!(state.loading, "a fenced apply must not clear loading");
        assert!(!state.apply(request + 1, &scope, Some(ready_result("doc-1", 7)), None));
        assert!(state.loading);
    }

    #[test]
    fn every_daemon_status_maps_to_a_visible_state() {
        let scope = scope();
        for (status, view) in [
            (LspCompletionStatus::Ready, CompletionViewStatus::Ready),
            (
                LspCompletionStatus::NotReady,
                CompletionViewStatus::NotReady,
            ),
            (LspCompletionStatus::Stale, CompletionViewStatus::Stale),
            (
                LspCompletionStatus::Unsynced,
                CompletionViewStatus::Unsynced,
            ),
            (
                LspCompletionStatus::Unsupported,
                CompletionViewStatus::Unsupported,
            ),
        ] {
            let mut state = EditorCompletionState::default();
            let request = state.begin(&scope);
            let mut result = ready_result("doc-1", 7);
            result.status = status;
            assert!(state.apply(request, &scope, Some(result), None));
            assert_eq!(state.status, view, "{status:?}");
            assert!(state.candidates.is_empty());
        }
    }

    #[test]
    fn a_result_for_another_identity_is_an_error_not_a_suggestion() {
        let scope = scope();
        let mut state = EditorCompletionState::default();
        let request = state.begin(&scope);
        assert!(state.apply(request, &scope, Some(ready_result("doc-2", 7)), None));
        assert_eq!(state.status, CompletionViewStatus::Error);
        assert!(state.candidates.is_empty());
        let mut state = EditorCompletionState::default();
        let request = state.begin(&scope);
        assert!(state.apply(request, &scope, Some(ready_result("doc-1", 9)), None));
        assert_eq!(state.status, CompletionViewStatus::Error);
    }

    #[test]
    fn transport_errors_keep_their_message_and_show_the_popup() {
        let scope = scope();
        let mut state = EditorCompletionState::default();
        let request = state.begin(&scope);
        assert!(state.apply(request, &scope, None, Some("boom".into())));
        assert_eq!(state.status, CompletionViewStatus::Error);
        assert_eq!(state.error.as_deref(), Some("boom"));
        assert!(state.popup_open);
        assert_eq!(state.status_line(), Some("completion failed"));
    }

    #[test]
    fn selection_clamps_and_survives_an_empty_list() {
        let scope = scope();
        let mut state = EditorCompletionState::default();
        let request = state.begin(&scope);
        assert!(state.apply(request, &scope, Some(ready_result("doc-1", 7)), None));
        state.select_next();
        state.select_previous();
        assert_eq!(state.selected, 0);
        assert!(state.selected_candidate().is_none());
        assert_eq!(state.status_line(), Some("no suggestions"));
    }

    #[test]
    fn selection_moves_within_a_ready_list() {
        let scope = scope();
        let mut state = EditorCompletionState::default();
        let request = state.begin(&scope);
        let mut result = ready_result("doc-1", 7);
        result.candidates = vec![candidate("b"), candidate("a")];
        assert!(state.apply(request, &scope, Some(result), None));
        assert_eq!(
            state.selected_candidate().map(|c| c.label.as_str()),
            Some("b")
        );
        state.select_next();
        assert_eq!(
            state.selected_candidate().map(|c| c.label.as_str()),
            Some("a")
        );
        state.select_next();
        assert_eq!(state.selected, 1, "selection clamps at the end");
        state.select_previous();
        assert_eq!(state.selected, 0);
        assert_eq!(state.status_line(), Some("suggestions"));
    }

    #[test]
    fn dismiss_and_clear_drop_everything_without_side_effects() {
        let scope = scope();
        let mut state = EditorCompletionState::default();
        state.begin(&scope);
        state.dismiss();
        assert!(!state.is_open());
        assert_eq!(state.status, CompletionViewStatus::Idle);
        assert!(!state.loading);
        let mut state = EditorCompletionState::default();
        let request = state.begin(&scope);
        let mut result = ready_result("doc-1", 7);
        result.candidates = vec![candidate("b")];
        assert!(state.apply(request, &scope, Some(result), None));
        state.clear();
        assert_eq!(state, EditorCompletionState::default());
    }

    #[test]
    fn status_lines_distinguish_empty_ready_from_cold_and_broken() {
        let scope = scope();
        let mut state = EditorCompletionState::default();
        assert_eq!(state.status_line(), None, "closed popup has no line");
        let request = state.begin(&scope);
        assert_eq!(state.status_line(), Some("completion…"));
        assert!(state.apply(request, &scope, Some(ready_result("doc-1", 7)), None));
        assert_eq!(state.status_line(), Some("no suggestions"));
    }

    #[test]
    fn sanitizing_neutralizes_escapes_newlines_and_length() {
        assert_eq!(sanitize_completion_text("plain", 64), "plain");
        assert_eq!(
            sanitize_completion_text("a\x1b[31mb\nc\td", 64),
            "a [31mb c d"
        );
        assert_eq!(sanitize_completion_text("abcdef", 4), "abcd");
        assert_eq!(sanitize_completion_text("", 64), "");
    }

    fn candidate(label: &str) -> LspCompletionCandidateDto {
        LspCompletionCandidateDto {
            label: label.into(),
            detail: None,
            kind: None,
            filter_text: None,
            sort_text: None,
            applicability: LspCompletionApplicabilityDto::Applicable {
                edit: LspCompletionEditDto {
                    start_line: 0,
                    start_character: 0,
                    end_line: 0,
                    end_character: 0,
                    new_text: label.into(),
                },
            },
        }
    }
}
