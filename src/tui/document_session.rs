//! Minimal TUI ownership adapter for the shared document controller.
//!
//! This is an M005 seam only: presentation position is separate from text,
//! and no widget or second mutable text buffer lives here.
//!
//! M006-A makes this the real ownership seam for the TUI editor. The
//! controller remains the sole owner of canonical text; the presentation
//! record below holds cursor, selection, viewport, buffer mode, and a
//! bounded frontend undo history — and **no text**. See
//! `crate::tui::editor` for the model and the static guard in
//! `scripts/check_tui_editor_text_authority.py`.
use std::sync::Arc;

use async_trait::async_trait;
use codegg_client::{
    DocumentAttachmentInfo, DocumentController, DocumentControllerError, DocumentState,
    DocumentTransport,
};
use codegg_document::DocumentSnapshot;
use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION};
use codegg_protocol::document::DocumentScope;

use crate::core::CoreClient;

use super::editor::{EditorFocus, EditorMode, EditorUndoEntry};

struct CoreClientTransport(Arc<dyn CoreClient>);

#[async_trait]
impl DocumentTransport for CoreClientTransport {
    async fn request(&self, request: CoreRequest) -> Result<CoreResponse, String> {
        self.0
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload: request,
            })
            .await
            .map_err(|error| error.to_string())
    }
}

/// Presentation-neutral document handle used by TUI editor flows.
pub struct TuiDocumentSession {
    controller: Arc<DocumentController>,
    presentation: TuiDocumentPresentation,
}

/// Editor presentation state owned by the frontend.
///
/// Holds no document text. Every offset is a position into the replica
/// owned by [`DocumentController`]; text is read on demand through
/// [`DocumentController::try_snapshot`] and dropped at the end of the call
/// that needed it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TuiDocumentPresentation {
    /// Cursor position as a UTF-8 byte offset into the local replica.
    pub cursor_byte: usize,
    /// Active selection, as a byte range into the local replica.
    pub selection: Option<std::ops::Range<usize>>,
    /// First visible line, zero-based. The hard-wrap viewport is one
    /// screen row per logical line.
    pub viewport_line: usize,
    /// Horizontal scroll in display columns.
    pub viewport_column: usize,
    /// Buffer edit mode.
    pub mode: EditorMode,
    /// Multi-key normal-mode command prefix (`d`, `g`). Bounded to
    /// [`MAX_EDITOR_PENDING_COMMAND`](super::editor::MAX_EDITOR_PENDING_COMMAND).
    pub pending_command: String,
    /// The `:` command line while command mode is open, without the leading
    /// `:`. Empty when command mode is closed.
    ///
    /// Bounded to [`MAX_EDITOR_COMMAND_LINE`](super::editor::MAX_EDITOR_COMMAND_LINE).
    /// This is a command *name*, never document text, so it is exempt from
    /// the text-authority guard for the same reason `pending_command` is.
    pub command_line: String,
    /// Whether the `:` command line is open.
    pub command_active: bool,
    /// Index into [`complete_editor_command`](super::editor::complete_editor_command)
    /// for the highlighted palette row.
    pub command_selection: usize,
    /// Frontend-local undo history of inverse transactions.
    pub undo: Vec<EditorUndoEntry>,
    /// Frontend-local redo history of inverse transactions.
    pub redo: Vec<EditorUndoEntry>,
    /// Retained undo payload bytes, for the undo memory bound.
    pub undo_bytes: usize,
    /// Retained redo payload bytes, for the redo memory bound.
    pub redo_bytes: usize,
    /// Which region of the editor route owns keyboard input.
    pub focus: EditorFocus,
}

impl TuiDocumentPresentation {
    /// Reset to a post-open presentation: cursor at the start of the
    /// document, no selection, no history. Undo never survives opening a
    /// different document.
    pub fn reset_for_open(&mut self) {
        self.cursor_byte = 0;
        self.selection = None;
        self.viewport_line = 0;
        self.viewport_column = 0;
        self.mode = EditorMode::Normal;
        super::editor::clear_history(self);
        self.focus = EditorFocus::Composer;
        self.close_command_line();
    }

    /// Open the `:` command line, optionally seeded with `prefix`.
    ///
    /// Opening resets the selection to the best match so `Enter` on a fresh
    /// `:` runs the first palette row rather than nothing.
    pub fn open_command_line(&mut self, prefix: &str) {
        self.command_active = true;
        self.command_line = prefix
            .chars()
            .take(super::editor::MAX_EDITOR_COMMAND_LINE)
            .collect();
        self.command_selection = 0;
    }

    /// Close the `:` command line and discard whatever was typed.
    pub fn close_command_line(&mut self) {
        self.command_active = false;
        self.command_line.clear();
        self.command_selection = 0;
    }

    /// Current palette rows for the command line.
    pub fn command_matches(&self) -> Vec<super::editor::EditorCommandMatch> {
        super::editor::complete_editor_command(&self.command_line)
    }

    /// Append one character to the command line, bounded by
    /// [`MAX_EDITOR_COMMAND_LINE`](super::editor::MAX_EDITOR_COMMAND_LINE).
    ///
    /// Returns `false` when the bound is already reached, so the caller can
    /// leave the line untouched rather than silently dropping the keystroke
    /// after mutating the cursor.
    pub fn push_command_char(&mut self, ch: char) -> bool {
        if self.command_line.chars().count() >= super::editor::MAX_EDITOR_COMMAND_LINE {
            return false;
        }
        self.command_line.push(ch);
        self.command_selection = 0;
        true
    }

    /// Delete the last character of the command line.
    pub fn pop_command_char(&mut self) {
        self.command_line.pop();
        self.command_selection = 0;
    }

    /// Move the palette selection by `delta`, clamped to the current rows.
    pub fn move_command_selection(&mut self, delta: isize) {
        let count = self.command_matches().len();
        if count == 0 {
            self.command_selection = 0;
            return;
        }
        let next = if delta.is_negative() {
            self.command_selection.saturating_sub(delta.unsigned_abs())
        } else {
            self.command_selection
                .saturating_add(delta.unsigned_abs())
                .min(count - 1)
        };
        self.command_selection = next;
    }

    /// Replace the command line with `value`, as palette selection does.
    pub fn accept_command_completion(&mut self, value: &str) {
        self.command_line = value.to_string();
        self.command_selection = 0;
    }
}

impl TuiDocumentSession {
    pub fn new(client: Arc<dyn CoreClient>) -> Self {
        Self {
            controller: Arc::new(DocumentController::new(Arc::new(CoreClientTransport(
                client,
            )))),
            presentation: TuiDocumentPresentation::default(),
        }
    }

    /// Adopt an existing controller.
    ///
    /// Used to qualify this seam against a scripted transport, and by
    /// reconnect flows that keep one controller across a transport
    /// replacement. Ownership and semantics are unchanged: the adopted
    /// controller is still the sole owner of the text.
    pub fn from_controller(controller: Arc<DocumentController>) -> Self {
        Self {
            controller,
            presentation: TuiDocumentPresentation::default(),
        }
    }

    pub fn controller(&self) -> Arc<DocumentController> {
        Arc::clone(&self.controller)
    }

    pub fn presentation(&self) -> &TuiDocumentPresentation {
        &self.presentation
    }

    pub fn presentation_mut(&mut self) -> &mut TuiDocumentPresentation {
        &mut self.presentation
    }

    /// Read the current replica for rendering, without retaining it.
    ///
    /// The returned snapshot is a handle to the controller's own buffer and
    /// is dropped at the end of the caller's scope, so the TUI never holds a
    /// second text buffer.
    pub fn try_snapshot(&self) -> Option<DocumentSnapshot> {
        self.controller.try_snapshot().map(|(snapshot, _)| snapshot)
    }

    /// Non-textual attachment metadata, safe to hold across frames.
    pub fn attachment_info(&self) -> Option<DocumentAttachmentInfo> {
        self.controller.try_attachment_info()
    }

    /// Attach one document under `scope`.
    ///
    /// `scope` decides which authority root the daemon confines the document
    /// to; the TUI forwards it rather than choosing, so the daemon remains
    /// the only place a root is resolved.
    pub async fn open(
        &self,
        project_id: String,
        workspace_id: String,
        relative_path: String,
        writable: bool,
        scope: DocumentScope,
    ) -> Result<(), DocumentControllerError> {
        self.controller
            .open(project_id, workspace_id, relative_path, writable, scope)
            .await
    }

    /// Create a new file holding `initial_text` under `scope` and attach it.
    pub async fn create(
        &self,
        project_id: String,
        workspace_id: String,
        relative_path: String,
        initial_text: String,
        scope: DocumentScope,
    ) -> Result<(), DocumentControllerError> {
        self.controller
            .create(project_id, workspace_id, relative_path, initial_text, scope)
            .await
    }

    /// Delete one file under `scope` through a fresh controller over the same
    /// transport.
    ///
    /// A delete is not an attachment operation, so it needs no replica; it
    /// only needs the same daemon and the same containment checks.
    pub async fn delete_file(
        client: Arc<dyn CoreClient>,
        project_id: String,
        workspace_id: String,
        relative_path: String,
        scope: DocumentScope,
    ) -> Result<bool, DocumentControllerError> {
        let controller = DocumentController::new(Arc::new(CoreClientTransport(client)));
        controller
            .delete_file(project_id, workspace_id, relative_path, scope)
            .await
    }

    /// The scope the attached document was opened under, if any.
    ///
    /// Drives the global/project badge in the editor chrome and decides
    /// whether a successful save should refresh the agent registry.
    pub fn scope(&self) -> Option<DocumentScope> {
        self.controller.try_attachment_info().map(|info| info.scope)
    }

    pub async fn state(&self) -> DocumentState {
        self.controller.state().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use tokio::sync::Mutex;

    struct FakeCoreClient(Mutex<VecDeque<CoreResponse>>);

    #[async_trait]
    impl CoreClient for FakeCoreClient {
        async fn request(
            &self,
            _request: RequestEnvelope<CoreRequest>,
        ) -> Result<CoreResponse, crate::error::AppError> {
            self.0.lock().await.pop_front().ok_or_else(|| {
                crate::error::AppError::Other(anyhow::anyhow!("no scripted response"))
            })
        }

        fn subscribe(
            &self,
        ) -> tokio::sync::mpsc::Receiver<
            codegg_protocol::core::EventEnvelope<codegg_protocol::core::CoreEvent>,
        > {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            rx
        }
    }

    #[tokio::test]
    async fn tui_session_opens_through_the_shared_replica() {
        let client = Arc::new(FakeCoreClient(Mutex::new(VecDeque::from([
            CoreResponse::DocumentSnapshot {
                snapshot: codegg_protocol::document::DocumentSnapshotDto {
                    document_id: "doc".into(),
                    project_id: "project".into(),
                    workspace_id: "workspace".into(),
                    relative_path: "src/lib.rs".into(),
                    revision: 2,
                    text: "text".into(),
                    dirty: false,
                    conflicted: false,
                    writer: true,
                    disk_base_digest: "digest".into(),
                    scope: DocumentScope::Workspace,
                },
                writer_lease: Some("lease".into()),
                lsp_degraded: false,
            },
        ]))));
        let session = TuiDocumentSession::new(client);
        let controller = session.controller();
        session
            .open(
                "project".into(),
                "workspace".into(),
                "src/lib.rs".into(),
                true,
                DocumentScope::Workspace,
            )
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&controller, &session.controller()));
        assert_eq!(session.state().await, DocumentState::Synced);
        assert_eq!(controller.snapshot().await.unwrap().0.to_string(), "text");

        let mut session = session;
        *session.presentation_mut() = TuiDocumentPresentation {
            cursor_byte: 2,
            selection: Some(1..3),
            viewport_line: 4,
            ..TuiDocumentPresentation::default()
        };
        assert_eq!(session.presentation().cursor_byte, 2);
        assert_eq!(
            session.controller().snapshot().await.unwrap().0.to_string(),
            "text"
        );
    }
}
