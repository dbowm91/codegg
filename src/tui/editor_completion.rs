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
    /// Lifetime-unique fencing epoch (C003/F2).
    ///
    /// Monotonic for the process lifetime and bumped on every path that
    /// resets `request_id` (`clear`, `reset_for_scope`), so a request id
    /// is never reused within one epoch. Completions carry the epoch they
    /// were issued under; an old completion arriving after a clear,
    /// dismiss-then-clear, reconnect, tab switch, or scope ping-pong
    /// mismatches and is dropped even when scope and request id coincide.
    /// `u64` rollover is unreachable in practice; arithmetic still uses
    /// `wrapping_add` so exhaustion degrades to a documented fence rather
    /// than a panic.
    pub epoch: u64,
    pub popup_open: bool,
}

impl EditorCompletionState {
    pub fn reset_for_scope(&mut self, scope: EditorCompletionScope) {
        if self.scope.as_ref() == Some(&scope) {
            return;
        }
        let epoch = self.epoch.wrapping_add(1);
        *self = Self {
            scope: Some(scope),
            epoch,
            ..Self::default()
        };
    }

    /// The current fencing epoch. Captured at request time and echoed on
    /// the completion so `apply` can reject pre-reset answers.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Begin one request, superseding any in flight.
    ///
    /// A re-trigger while loading starts a newer request rather than
    /// queueing behind the old one: completions carry request ids, so the
    /// superseded answer is dropped on arrival and can never paint over the
    /// newer one. Returns the new request id; pair it with [`Self::epoch`]
    /// for the full fence.
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
    /// Returns `false` — dropping the result — when the epoch moved on
    /// (clear/reset/reconnect since issue), the scope moved on, or the
    /// request id is not the latest. A dropped response is never shown as
    /// actionable, no matter how well its scope matches.
    pub fn apply(
        &mut self,
        request_id: u64,
        epoch: u64,
        scope: &EditorCompletionScope,
        result: Option<LspCompletionResultDto>,
        error: Option<String>,
    ) -> bool {
        if self.epoch != epoch
            || self.scope.as_ref() != Some(scope)
            || self.request_id != request_id
            || !self.loading
        {
            return false;
        }
        self.loading = false;
        match (result, error) {
            (Some(result), _) => {
                // Document identity is checked before anything is shown,
                // for every status: a result for another document is
                // discarded, not displayed.
                if result.document_id != scope.document_id {
                    self.status = CompletionViewStatus::Error;
                    self.error = Some("completion answer is for another document".into());
                    self.candidates.clear();
                    return true;
                }
                match result.status {
                    LspCompletionStatus::Ready => {
                        // Ready is bound to the exact requested revision,
                        // including the sync witness when the daemon sent
                        // one. Anything else is malformed, not actionable.
                        let synced_ok = result
                            .synced_revision
                            .is_none_or(|synced| synced == scope.revision);
                        if result.revision != scope.revision || !synced_ok {
                            self.status = CompletionViewStatus::Error;
                            self.error =
                                Some("completion answer is for another document revision".into());
                            self.candidates.clear();
                            return true;
                        }
                        self.status = CompletionViewStatus::Ready;
                        self.truncated = result.truncated;
                        self.incomplete = result.incomplete;
                        // Defense in depth: the protocol caps at 100
                        // candidates, but a misbehaving daemon must not grow
                        // frontend state without bound. Dropping here sets
                        // `truncated` so the popup stays honest about what
                        // it shows.
                        let mut candidates = result.candidates;
                        if candidates.len() > codegg_protocol::lsp::MAX_LSP_COMPLETION_CANDIDATES {
                            candidates
                                .truncate(codegg_protocol::lsp::MAX_LSP_COMPLETION_CANDIDATES);
                            self.truncated = true;
                        }
                        self.candidates = candidates;
                        self.selected = 0;
                        self.error = None;
                    }
                    // Non-ready statuses keep their typed meaning regardless
                    // of revision: `NotReady` carries the constructor's
                    // placeholder revision by design, and `Stale` legitimately
                    // echoes the daemon-current (newer) revision. Candidates
                    // are always disarmed: a non-ready answer with payload —
                    // whether a hostile peer or a malformed daemon — is
                    // displayed as its status, never as suggestions.
                    LspCompletionStatus::NotReady => {
                        self.status = CompletionViewStatus::NotReady;
                        self.disarm_non_ready(&result);
                    }
                    LspCompletionStatus::Stale => {
                        self.status = CompletionViewStatus::Stale;
                        self.disarm_non_ready(&result);
                    }
                    LspCompletionStatus::Unsynced => {
                        self.status = CompletionViewStatus::Unsynced;
                        self.disarm_non_ready(&result);
                    }
                    LspCompletionStatus::Unsupported => {
                        self.status = CompletionViewStatus::Unsupported;
                        self.disarm_non_ready(&result);
                    }
                }
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

    /// Display a non-ready answer as its typed status with no actionable
    /// candidates. Copies only the presentation flags; payload bodies are
    /// never trusted from a non-ready result.
    fn disarm_non_ready(&mut self, result: &LspCompletionResultDto) {
        self.truncated = result.truncated;
        self.incomplete = result.incomplete;
        self.candidates.clear();
        self.selected = 0;
        self.error = None;
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
    /// request id with it. Bumps the fencing epoch so a request id issued
    /// afterwards can never collide with a pre-clear completion still in
    /// flight — even across reconnects that leave generation and scope
    /// unchanged.
    pub fn clear(&mut self) {
        let epoch = self.epoch.wrapping_add(1);
        *self = Self {
            epoch,
            ..Self::default()
        };
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
        assert!(!state.apply(
            first,
            state.epoch(),
            &scope,
            Some(ready_result("doc-1", 7)),
            None
        ));
        assert!(state.loading);
        assert!(state.apply(
            second,
            state.epoch(),
            &scope,
            Some(ready_result("doc-1", 7)),
            None
        ));
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
        assert!(!state.apply(
            request,
            state.epoch(),
            &other,
            Some(ready_result("doc-1", 8)),
            None
        ));
        assert!(state.loading, "a fenced apply must not clear loading");
        assert!(!state.apply(
            request + 1,
            state.epoch(),
            &scope,
            Some(ready_result("doc-1", 7)),
            None
        ));
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
            assert!(state.apply(request, state.epoch(), &scope, Some(result), None));
            assert_eq!(state.status, view, "{status:?}");
            assert!(state.candidates.is_empty());
        }
    }

    #[test]
    fn a_result_for_another_identity_is_an_error_not_a_suggestion() {
        let scope = scope();
        let mut state = EditorCompletionState::default();
        let request = state.begin(&scope);
        assert!(state.apply(
            request,
            state.epoch(),
            &scope,
            Some(ready_result("doc-2", 7)),
            None
        ));
        assert_eq!(state.status, CompletionViewStatus::Error);
        assert!(state.candidates.is_empty());
        let mut state = EditorCompletionState::default();
        let request = state.begin(&scope);
        assert!(state.apply(
            request,
            state.epoch(),
            &scope,
            Some(ready_result("doc-1", 9)),
            None
        ));
        assert_eq!(state.status, CompletionViewStatus::Error);
    }

    /// F1: the exact wire constructors keep their typed statuses on a
    /// nonzero-revision request. Before the fix both presented the generic
    /// "another document revision" Error.
    #[test]
    fn real_not_ready_and_stale_constructors_keep_typed_status() {
        let scope = scope();
        // Cold server: the constructor's placeholder revision 0 is normal,
        // not an identity error, on a revision-7 request.
        let mut state = EditorCompletionState::default();
        let request = state.begin(&scope);
        let epoch = state.epoch();
        assert!(state.apply(
            request,
            epoch,
            &scope,
            Some(LspCompletionResultDto::not_ready("doc-1".into())),
            None
        ));
        assert_eq!(state.status, CompletionViewStatus::NotReady);
        assert!(state.candidates.is_empty());
        assert_eq!(state.status_line(), Some("language server warming…"));
        // True stale: the daemon-current revision 8 legitimately differs
        // from the requested 7 and must not become a generic Error.
        let mut state = EditorCompletionState::default();
        let request = state.begin(&scope);
        let epoch = state.epoch();
        assert!(state.apply(
            request,
            epoch,
            &scope,
            Some(LspCompletionResultDto::stale("doc-1".into(), 8, Some(8))),
            None
        ));
        assert_eq!(state.status, CompletionViewStatus::Stale);
        assert!(state.candidates.is_empty());
        assert_eq!(state.status_line(), Some("document changed — re-trigger"));
        // Unsynced/unsupported with mismatched revisions are equally typed.
        for (result, view) in [
            (
                LspCompletionResultDto::unsynced("doc-1".into(), 9, Some(5)),
                CompletionViewStatus::Unsynced,
            ),
            (
                LspCompletionResultDto::unsupported("doc-1".into(), 0, None),
                CompletionViewStatus::Unsupported,
            ),
        ] {
            let mut state = EditorCompletionState::default();
            let request = state.begin(&scope);
            assert!(state.apply(request, state.epoch(), &scope, Some(result), None));
            assert_eq!(state.status, view);
            assert!(state.candidates.is_empty());
        }
    }

    /// F1 defensive: a non-ready answer carrying candidates — hostile peer
    /// or malformed daemon — is displayed as its status with the payload
    /// disarmed, never as suggestions. A malformed Ready (wrong revision,
    /// inconsistent sync witness) is an Error, never actionable.
    #[test]
    fn hostile_and_malformed_payloads_are_never_actionable() {
        let scope = scope();
        let mut hostile = LspCompletionResultDto::not_ready("doc-1".into());
        hostile.candidates = vec![candidate("evil")];
        let mut state = EditorCompletionState::default();
        let request = state.begin(&scope);
        assert!(state.apply(request, state.epoch(), &scope, Some(hostile), None));
        assert_eq!(state.status, CompletionViewStatus::NotReady);
        assert!(state.candidates.is_empty());
        assert!(state.selected_candidate().is_none());
        // Ready with a stale revision.
        let mut state = EditorCompletionState::default();
        let request = state.begin(&scope);
        assert!(state.apply(
            request,
            state.epoch(),
            &scope,
            Some(ready_result("doc-1", 8)),
            None
        ));
        assert_eq!(state.status, CompletionViewStatus::Error);
        assert!(state.candidates.is_empty());
        // Ready with the right revision but an inconsistent sync witness.
        let mut state = EditorCompletionState::default();
        let request = state.begin(&scope);
        let mut inconsistent = ready_result("doc-1", 7);
        inconsistent.candidates = vec![candidate("x")];
        inconsistent.synced_revision = Some(9);
        assert!(state.apply(request, state.epoch(), &scope, Some(inconsistent), None));
        assert_eq!(state.status, CompletionViewStatus::Error);
        assert!(state.candidates.is_empty());
        assert!(state.selected_candidate().is_none());
    }

    /// F2: a request id reused after `clear` cannot admit a pre-clear
    /// answer, even with an identical scope. Captures the old epoch before
    /// the fix point (both requests would be id 1, epoch 0).
    #[test]
    fn reused_request_id_after_clear_needs_a_new_epoch() {
        let scope = scope();
        let mut state = EditorCompletionState::default();
        let first = state.begin(&scope);
        let first_epoch = state.epoch();
        assert_eq!(first, 1);
        // Reconnect/edit/switch drops everything without touching the
        // generation or scope binding.
        state.clear();
        let second = state.begin(&scope);
        let second_epoch = state.epoch();
        assert_eq!(second, 1, "ids restart after clear");
        assert_ne!(first_epoch, second_epoch);
        // The old answer arrives late with a matching scope and id — and
        // is dropped on the epoch fence while the new request stays live.
        assert!(!state.apply(
            first,
            first_epoch,
            &scope,
            Some(ready_result("doc-1", 7)),
            None
        ));
        assert!(state.loading);
        assert!(state.apply(
            second,
            second_epoch,
            &scope,
            Some(ready_result("doc-1", 7)),
            None
        ));
        assert_eq!(state.status, CompletionViewStatus::Ready);
    }

    /// F2: scope ping-pong (A → B → A) reuses ids with an identical scope
    /// and must still fence the late A answer.
    #[test]
    fn scope_ping_pong_does_not_resurrect_an_old_answer() {
        let scope_a = scope();
        let mut scope_b = scope();
        scope_b.revision = 8;
        let mut state = EditorCompletionState::default();
        let a_first = state.begin(&scope_a);
        let a_first_epoch = state.epoch();
        let _b = state.begin(&scope_b);
        let a_second = state.begin(&scope_a);
        let a_second_epoch = state.epoch();
        assert_eq!((a_first, a_second), (1, 1));
        assert_ne!(a_first_epoch, a_second_epoch);
        assert!(!state.apply(
            a_first,
            a_first_epoch,
            &scope_a,
            Some(ready_result("doc-1", 7)),
            None
        ));
        assert!(state.loading);
        assert!(state.apply(
            a_second,
            a_second_epoch,
            &scope_a,
            Some(ready_result("doc-1", 7)),
            None
        ));
        assert_eq!(state.status, CompletionViewStatus::Ready);
    }

    /// F2: dismiss-then-retrigger without an intervening clear keeps
    /// incrementing ids (no reset, no reuse), so the fence holds without
    /// an epoch change.
    #[test]
    fn dismiss_then_retrigger_does_not_reuse_ids() {
        let scope = scope();
        let mut state = EditorCompletionState::default();
        let first = state.begin(&scope);
        state.dismiss();
        let second = state.begin(&scope);
        assert_eq!((first, second), (1, 2));
        assert!(!state.apply(
            first,
            state.epoch(),
            &scope,
            Some(ready_result("doc-1", 7)),
            None
        ));
        assert!(state.apply(
            second,
            state.epoch(),
            &scope,
            Some(ready_result("doc-1", 7)),
            None
        ));
        assert_eq!(state.status, CompletionViewStatus::Ready);
    }

    #[test]
    fn transport_errors_keep_their_message_and_show_the_popup() {
        let scope = scope();
        let mut state = EditorCompletionState::default();
        let request = state.begin(&scope);
        assert!(state.apply(request, state.epoch(), &scope, None, Some("boom".into())));
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
        assert!(state.apply(
            request,
            state.epoch(),
            &scope,
            Some(ready_result("doc-1", 7)),
            None
        ));
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
        assert!(state.apply(request, state.epoch(), &scope, Some(result), None));
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
        assert!(state.apply(request, state.epoch(), &scope, Some(result), None));
        state.clear();
        // Everything resets except the fencing epoch, which must survive
        // so the next request id cannot collide with a pre-clear answer.
        let epoch = state.epoch;
        assert_ne!(epoch, 0, "resets must advance the fence");
        assert_eq!(
            state,
            EditorCompletionState {
                epoch,
                ..EditorCompletionState::default()
            }
        );
    }

    #[test]
    fn status_lines_distinguish_empty_ready_from_cold_and_broken() {
        let scope = scope();
        let mut state = EditorCompletionState::default();
        assert_eq!(state.status_line(), None, "closed popup has no line");
        let request = state.begin(&scope);
        assert_eq!(state.status_line(), Some("completion…"));
        assert!(state.apply(
            request,
            state.epoch(),
            &scope,
            Some(ready_result("doc-1", 7)),
            None
        ));
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
