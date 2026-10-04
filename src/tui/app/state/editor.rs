//! M006-A: TUI editor state.
//!
//! Owns the editor's frontend state: the document session handle, the
//! requested path, the in-flight lifecycle operation, and the presentation
//! of controller states and errors.
//!
//! **Holds no document text.** Text reaches the render path only as a
//! scoped borrow of `DocumentController::try_snapshot()` inside a single
//! frame. `scripts/check_tui_editor_text_authority.py` enforces this.

use codegg_client::{DocumentControllerError, DocumentState};

use super::async_request::AsyncUiRequestState;
use crate::tui::document_session::TuiDocumentSession;
use crate::tui::editor::{EditorFocus, EditorMode};

/// Which lifecycle operation is in flight.
///
/// Drives the busy indicator and, on completion, the decision to clear
/// undo history. Every value except `None` corresponds to one
/// reconciliation transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorOperation {
    Open,
    Save,
    Reload,
    Resync,
    Close,
}

impl EditorOperation {
    /// Whether completing this operation invalidates frontend undo history.
    ///
    /// Reload, resync, and close can replace or drop the local replica, so
    /// inverse transactions recorded against the previous text no longer
    /// describe it. Open starts a fresh document. Save does not: it writes
    /// the same local text to disk, so undo stays meaningful.
    pub fn clears_undo_history(self) -> bool {
        !matches!(self, EditorOperation::Save)
    }
}

/// Presentation of one `DocumentState`.
///
/// Every `DocumentState` maps to a distinct, tested variant so a conflict
/// never renders like a transport failure and a read-only attachment never
/// renders like a writable one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorStatus {
    /// No document is attached.
    Closed,
    /// A document is being opened.
    Opening,
    /// Local replica matches the daemon.
    Synced,
    /// Local edits not yet accepted by the daemon.
    DirtyLocal,
    /// A local change is in flight.
    Flushing,
    /// The daemon reported something this replica cannot interpret; an
    /// explicit resync is required before editing.
    ResyncRequired,
    /// Disk changed under the document. The local draft is retained and
    /// surfaced; the editor never auto-resolves it.
    Conflict,
    /// Attached without a writer lease. Edits are refused.
    ReadOnly,
    /// Transport is down. The local draft is retained.
    Disconnected,
    /// The daemon no longer has this document. The local draft is retained.
    GoneWithDraft,
}

impl EditorStatus {
    /// Map a controller state onto its presentation.
    pub fn from_state(state: &DocumentState) -> Self {
        match state {
            DocumentState::Closed => EditorStatus::Closed,
            DocumentState::Opening => EditorStatus::Opening,
            DocumentState::Synced => EditorStatus::Synced,
            DocumentState::DirtyLocal => EditorStatus::DirtyLocal,
            DocumentState::Flushing => EditorStatus::Flushing,
            DocumentState::ResyncRequired => EditorStatus::ResyncRequired,
            DocumentState::Conflict => EditorStatus::Conflict,
            DocumentState::ReadOnly => EditorStatus::ReadOnly,
            DocumentState::Disconnected => EditorStatus::Disconnected,
            DocumentState::GoneWithLocalDraft => EditorStatus::GoneWithDraft,
        }
    }

    /// Status-bar label. Distinct wording per state so a user can tell a
    /// retained-draft condition from a clean one without reading the banner.
    pub fn label(self) -> &'static str {
        match self {
            EditorStatus::Closed => "closed",
            EditorStatus::Opening => "opening…",
            EditorStatus::Synced => "synced",
            EditorStatus::DirtyLocal => "modified",
            EditorStatus::Flushing => "syncing…",
            EditorStatus::ResyncRequired => "resync required",
            EditorStatus::Conflict => "disk conflict — draft kept",
            EditorStatus::ReadOnly => "read-only",
            EditorStatus::Disconnected => "disconnected — draft kept",
            EditorStatus::GoneWithDraft => "document gone — draft kept",
        }
    }

    /// Whether the editor must refuse local edits in this state.
    pub fn blocks_edits(self) -> bool {
        matches!(
            self,
            EditorStatus::Closed
                | EditorStatus::Opening
                | EditorStatus::ReadOnly
                | EditorStatus::Conflict
                | EditorStatus::Disconnected
                | EditorStatus::ResyncRequired
                | EditorStatus::GoneWithDraft
        )
    }
}

/// Severity of a surfaced controller error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EditorNoticeSeverity {
    /// A transient condition. The user may simply try again.
    Transient,
    /// The request was refused. The document state is unchanged.
    Refused,
    /// The document needs a decision the editor will not make for the user.
    Actionable,
}

/// Presentation of one `DocumentControllerError`.
///
/// `LifecycleBusy` and the queue bounds are expected conditions under rapid
/// editing, not failures, and get their own wording so the user is not told
/// an edit "failed" when it was merely deferred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorNotice {
    /// A destructive phase (save, reload, resync, close) is in flight.
    Busy,
    /// The pending transaction queue is at its entry bound.
    QueueFull,
    /// The pending transaction queue is at its byte bound.
    QueueBytes,
    /// The attachment holds no writer lease.
    ReadOnly,
    /// No document is attached.
    NotOpen,
    /// Another attachment already owns this document.
    AlreadyOpen,
    /// The daemon reported a disk conflict.
    Conflict,
    /// The replica cannot proceed without an explicit resync.
    ResyncRequired,
    /// The edit was rejected by the document engine.
    Rejected(String),
    /// The daemon answered with something unexpected.
    UnexpectedResponse(String),
    /// The transport failed.
    Transport(String),
}

impl EditorNotice {
    /// Map a controller error onto its presentation.
    pub fn from_error(error: &DocumentControllerError) -> Self {
        match error {
            DocumentControllerError::LifecycleBusy => EditorNotice::Busy,
            DocumentControllerError::QueueFull => EditorNotice::QueueFull,
            DocumentControllerError::QueueBytes => EditorNotice::QueueBytes,
            DocumentControllerError::ReadOnly => EditorNotice::ReadOnly,
            DocumentControllerError::NotOpen => EditorNotice::NotOpen,
            DocumentControllerError::AlreadyOpen => EditorNotice::AlreadyOpen,
            DocumentControllerError::Conflict => EditorNotice::Conflict,
            DocumentControllerError::ResyncRequired => EditorNotice::ResyncRequired,
            DocumentControllerError::Edit(inner) => EditorNotice::Rejected(inner.to_string()),
            DocumentControllerError::Response(message) => {
                EditorNotice::UnexpectedResponse(message.clone())
            }
            DocumentControllerError::Transport(message) => EditorNotice::Transport(message.clone()),
        }
    }

    /// Map a bare transport string, for failures raised before the
    /// controller produced a typed error.
    pub fn from_transport(message: String) -> Self {
        EditorNotice::Transport(message)
    }

    pub fn severity(&self) -> EditorNoticeSeverity {
        match self {
            EditorNotice::Busy
            | EditorNotice::QueueFull
            | EditorNotice::QueueBytes
            | EditorNotice::Transport(_)
            | EditorNotice::UnexpectedResponse(_) => EditorNoticeSeverity::Transient,
            EditorNotice::ReadOnly
            | EditorNotice::NotOpen
            | EditorNotice::AlreadyOpen
            | EditorNotice::Rejected(_) => EditorNoticeSeverity::Refused,
            EditorNotice::Conflict | EditorNotice::ResyncRequired => {
                EditorNoticeSeverity::Actionable
            }
        }
    }

    /// Single-line banner text.
    pub fn message(&self) -> String {
        match self {
            EditorNotice::Busy => {
                "Busy: another document operation is running — try again when it finishes"
                    .to_string()
            }
            EditorNotice::QueueFull => {
                "Edit queue is full — wait for pending changes to reach the daemon".to_string()
            }
            EditorNotice::QueueBytes => {
                "Edit queue byte limit reached — wait for pending changes to reach the daemon"
                    .to_string()
            }
            EditorNotice::ReadOnly => {
                "Read-only: this document is not attached as the writer".to_string()
            }
            EditorNotice::NotOpen => "No document is open".to_string(),
            EditorNotice::AlreadyOpen => {
                "This document is already open in another attachment".to_string()
            }
            EditorNotice::Conflict => {
                "Disk conflict: the file changed underneath this document. Your draft is kept — \
                 reload to take the disk version, which discards the draft"
                    .to_string()
            }
            EditorNotice::ResyncRequired => {
                "This replica is out of step with the daemon — run a resync before editing"
                    .to_string()
            }
            EditorNotice::Rejected(detail) => format!("Edit rejected: {detail}"),
            EditorNotice::UnexpectedResponse(detail) => {
                format!("Unexpected daemon response: {detail}")
            }
            EditorNotice::Transport(detail) => {
                format!("Daemon transport failed: {detail} — your draft is kept")
            }
        }
    }
}

/// The editor's frontend state for the current attachment.
pub struct EditorState {
    /// The attached document session, or `None` when nothing is open.
    ///
    /// One controller owns one attachment, so switching documents closes the
    /// previous controller before opening the next.
    pub session: Option<TuiDocumentSession>,
    /// Workspace-relative path currently open or being opened.
    pub path: Option<String>,
    /// Request tracking for async lifecycle completions.
    pub request: AsyncUiRequestState,
    /// Bumped on close and on every document switch, so a completion for a
    /// previous attachment is discarded.
    pub generation: u64,
    /// Lifecycle operation in flight, if any.
    pub operation: Option<EditorOperation>,
    /// Last surfaced controller error, if any.
    pub notice: Option<EditorNotice>,
    /// Last observed controller state.
    pub status: EditorStatus,
    /// Whether the attachment was opened requesting the writer lease.
    pub wants_writer: bool,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            session: None,
            path: None,
            request: AsyncUiRequestState::new(),
            generation: 0,
            operation: None,
            notice: None,
            status: EditorStatus::Closed,
            wants_writer: false,
        }
    }
}

impl EditorState {
    /// Whether a document is attached.
    pub fn is_open(&self) -> bool {
        self.session.is_some()
    }

    /// Whether a lifecycle operation is in flight.
    pub fn is_busy(&self) -> bool {
        self.operation.is_some()
    }

    /// Whether the buffer region owns keyboard input.
    pub fn buffer_focused(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.presentation().focus == EditorFocus::Buffer)
    }

    /// Whether the attachment currently holds the writer lease.
    pub fn is_writer(&self) -> bool {
        self.session
            .as_ref()
            .and_then(|session| session.attachment_info())
            .is_some_and(|info| info.is_writer)
    }

    /// Current buffer mode, or `Normal` when no document is open.
    pub fn mode(&self) -> EditorMode {
        self.session
            .as_ref()
            .map(|session| session.presentation().mode)
            .unwrap_or(EditorMode::Normal)
    }

    /// Record a surfaced controller error.
    pub fn set_notice(&mut self, notice: EditorNotice) {
        self.notice = Some(notice);
    }

    /// Clear the surfaced error, for example after a successful retry.
    pub fn clear_notice(&mut self) {
        self.notice = None;
    }

    /// Invalidate every in-flight completion and detach the session.
    ///
    /// Called when the editor closes or switches documents. Bumping the
    /// generation means a completion that arrives afterwards is discarded
    /// rather than applied to the new attachment.
    pub fn detach(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.request.cancel();
        self.operation = None;
        self.session = None;
        self.path = None;
        self.status = EditorStatus::Closed;
        self.notice = None;
        self.wants_writer = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_client::DocumentControllerError;

    #[test]
    fn every_controller_state_maps_to_a_distinct_status() {
        let states = [
            DocumentState::Closed,
            DocumentState::Opening,
            DocumentState::Synced,
            DocumentState::DirtyLocal,
            DocumentState::Flushing,
            DocumentState::ResyncRequired,
            DocumentState::Conflict,
            DocumentState::ReadOnly,
            DocumentState::Disconnected,
            DocumentState::GoneWithLocalDraft,
        ];
        let mut mapped: Vec<EditorStatus> = states.iter().map(EditorStatus::from_state).collect();
        let before = mapped.len();
        mapped.sort_by_key(|status| status.label());
        mapped.dedup();
        assert_eq!(mapped.len(), before, "each state needs distinct wording");
        assert!(states.iter().all(|state| {
            let label = EditorStatus::from_state(state).label();
            !label.is_empty()
        }));
    }

    #[test]
    fn only_writable_ready_states_allow_edits() {
        assert!(!EditorStatus::Synced.blocks_edits());
        assert!(!EditorStatus::DirtyLocal.blocks_edits());
        assert!(!EditorStatus::Flushing.blocks_edits());
        for blocked in [
            EditorStatus::Closed,
            EditorStatus::Opening,
            EditorStatus::ResyncRequired,
            EditorStatus::Conflict,
            EditorStatus::ReadOnly,
            EditorStatus::Disconnected,
            EditorStatus::GoneWithDraft,
        ] {
            assert!(blocked.blocks_edits(), "{blocked:?} must block edits");
        }
    }

    #[test]
    fn every_controller_error_maps_to_a_distinct_notice_message() {
        let errors = vec![
            DocumentControllerError::LifecycleBusy,
            DocumentControllerError::ReadOnly,
            DocumentControllerError::NotOpen,
            DocumentControllerError::AlreadyOpen,
            DocumentControllerError::QueueFull,
            DocumentControllerError::QueueBytes,
            DocumentControllerError::ResyncRequired,
            DocumentControllerError::Conflict,
            DocumentControllerError::Edit(codegg_document::DocumentError::OutOfBounds),
            DocumentControllerError::Response("weird".into()),
            DocumentControllerError::Transport("pipe closed".into()),
        ];
        let mut messages: Vec<String> = errors
            .iter()
            .map(|error| EditorNotice::from_error(error).message())
            .collect();
        let before = messages.len();
        messages.sort();
        messages.dedup();
        assert_eq!(
            messages.len(),
            before,
            "each error needs distinct presentation, got {messages:?}"
        );
    }

    #[test]
    fn queue_and_busy_are_transient_while_conflict_is_actionable() {
        assert_eq!(
            EditorNotice::from_error(&DocumentControllerError::LifecycleBusy).severity(),
            EditorNoticeSeverity::Transient
        );
        assert_eq!(
            EditorNotice::from_error(&DocumentControllerError::QueueFull).severity(),
            EditorNoticeSeverity::Transient
        );
        assert_eq!(
            EditorNotice::from_error(&DocumentControllerError::Conflict).severity(),
            EditorNoticeSeverity::Actionable
        );
        assert_eq!(
            EditorNotice::from_error(&DocumentControllerError::ResyncRequired).severity(),
            EditorNoticeSeverity::Actionable
        );
    }

    #[test]
    fn conflict_notice_states_the_draft_is_kept_and_is_never_auto_resolved() {
        let message = EditorNotice::from_error(&DocumentControllerError::Conflict).message();
        assert!(message.contains("draft is kept"), "{message}");
        assert!(!message.contains("automatically"), "{message}");
    }

    #[test]
    fn reconciliation_operations_clear_undo_history_but_save_does_not() {
        assert!(EditorOperation::Open.clears_undo_history());
        assert!(EditorOperation::Reload.clears_undo_history());
        assert!(EditorOperation::Resync.clears_undo_history());
        assert!(EditorOperation::Close.clears_undo_history());
        assert!(!EditorOperation::Save.clears_undo_history());
    }

    #[test]
    fn detach_invalidates_in_flight_completions() {
        let mut state = EditorState::default();
        let generation = state.generation;
        state.request.begin();
        state.operation = Some(EditorOperation::Open);
        state.path = Some("src/lib.rs".into());
        state.detach();
        assert_ne!(state.generation, generation);
        assert!(state.request.is_cancelled());
        assert!(state.operation.is_none());
        assert!(state.session.is_none());
        assert!(state.path.is_none());
        assert_eq!(state.status, EditorStatus::Closed);
    }

    #[test]
    fn default_state_reports_no_document_and_no_focus() {
        let state = EditorState::default();
        assert!(!state.is_open());
        assert!(!state.is_busy());
        assert!(!state.buffer_focused());
        assert!(!state.is_writer());
        assert_eq!(state.mode(), EditorMode::Normal);
    }
}
