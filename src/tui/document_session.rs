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

use super::editor::{
    EditorCommand, EditorMode, EditorPoint, EditorPrefix, EditorUndoEntry, FindMotion, FindNote,
    ParagraphNote, SelectionMotion,
};
use super::helix_selection::Selection;

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
    /// The last repeatable **selection** motion, for `Alt-.`.
    ///
    /// A second slot beside `last_find`, not a widening of it: `f x` records a
    /// find with a `char` to scan for, and `C` records "put a cursor on the
    /// next line". Widening `last_find` would mean a motion that is not a
    /// find wearing a find's type, and `find_note` — which reports on the
    /// *attempt* — would have to learn about motions it cannot describe.
    ///
    /// One slot, every motion overwrites it, exactly as Helix's single repeat
    /// slot does. `note_motion` clears it, so `j` between two `C` presses
    /// disarms the repeat and the gap is predictable rather than surprising.
    pub last_selection_motion: Option<SelectionMotion>,
    /// The last repeatable **unimpaired** motion, for `Alt-.`.
    ///
    /// Helix's keymap calls `Alt-.` "repeat last motion (`f`, `t`, `m`, `[` or
    /// `]`)", so the unimpaired paragraph motions belong in the repeat path and
    /// not only on their own keys.
    ///
    /// A third slot for the same reason [`last_selection_motion`](Self) is a
    /// second one: `FindMotion` carries a `char` to scan for and
    /// [`SelectionMotion`](Self) names one of M008's verbs, and neither can
    /// describe "move to the next paragraph" without becoming a union of three
    /// unrelated things. One slot, every motion overwrites it, exactly as
    /// Helix's does — `note_motion` clears it, so `j` between two `]p` presses
    /// disarms the repeat.
    ///
    /// The three slots are **mutually exclusive by construction**: whichever
    /// recorder runs last clears the other two. `Alt-.` may therefore consult
    /// them in any order without inventing a precedence between two motions
    /// that were never both the last one.
    pub last_unimpaired: Option<EditorCommand>,
    /// Why the last unimpaired paragraph motion did not land, if it did not.
    ///
    /// The paragraph analogue of [`find_note`](Self::find_note), and it exists
    /// for the same reason: `]p` at the last paragraph and `]p` on a buffer
    /// whose scan hit its byte budget both leave the cursor exactly where it
    /// was, and only one of them is a true "there is none".
    ///
    /// Written by [`editor::apply_motion`](super::editor::apply_motion) next to
    /// the resolution that produced it, and lifted into the notice by the
    /// command layer immediately after — so it describes exactly one keystroke
    /// and cannot survive to be re-read as the answer to the next one.
    pub paragraph_note: ParagraphNote,
    /// The held Helix selections, in document order, or empty when nothing is
    /// selected.
    ///
    /// Points are byte offsets paired with their line — see
    /// [`EditorPoint`](super::editor::EditorPoint). Stored as the shared
    /// [`Selection`] rather than a bare range so `v` stays a mode and `x` an
    /// action here exactly as they are in `/terminal`; resolving them to
    /// ranges is [`editor::selection_ranges`](super::editor::selection_ranges)'s
    /// job, and only the effects consume a range.
    ///
    /// This is a *set*, not an optional one, because Helix selection is:
    /// `d` over several selections must delete all of them in one gesture, and
    /// that is undefined rather than merely unbound while there is room for
    /// exactly one. [`primary_index`](Self::primary_index) names the member
    /// every single-selection accessor resolves to.
    pub selections: Vec<Selection<EditorPoint>>,
    /// Index into [`selections`](Self::selections) of the distinguished member.
    ///
    /// Meaningless while the set is empty; the single-selection accessors on
    /// the editor resolve the primary, which is why they keep answering for
    /// "the" selection after this milestone replaced the optional one.
    pub primary_index: usize,
    /// First visible line, zero-based. The hard-wrap viewport is one
    /// screen row per logical line.
    pub viewport_line: usize,
    /// Horizontal scroll in display columns.
    pub viewport_column: usize,
    /// Buffer edit mode.
    pub mode: EditorMode,
    /// The multi-key grammar the editor is waiting on.
    ///
    /// Idle, or one armed [`EditorPrefix`] consumed by the next key. See that
    /// type for the grammar rule — one pending prefix, one terminal key, and
    /// a disarm whenever the thing it was aimed at changes.
    pub pending_prefix: EditorPrefix,
    /// Byte column a vertical run is aiming for, or `None` when there is no
    /// run in progress.
    ///
    /// Without it, `j` onto a short line and back up returns to the short
    /// line's end column instead of where the run started — which is how a
    /// reader scrolling a ragged file loses their place every time the file
    /// narrows. It is a **byte** column, matching the editor's byte offsets,
    /// and is clamped into the target line at use time so a column recorded
    /// against older text can never place the cursor outside a line or inside
    /// a multi-byte character.
    ///
    /// Not a flag on a grammar, and not a second source of truth: it is
    /// rewritten by every motion through [`apply_motion`](super::editor::apply_motion)
    /// in one step, so it cannot disagree with the cursor it describes.
    pub desired_column: Option<usize>,
    /// The find-char motion `Alt-.` repeats, or `None` when there is none.
    ///
    /// Helix keeps one *motion* slot that every motion overwrites, so this
    /// is cleared by any other motion rather than only by another find — the
    /// gap between `f x`, `j`, and `Alt-.` is exactly what makes `Alt-.`
    /// predictable. A reconciliation clears it for the same reason
    /// `disarm_prefix` exists: the character it was aimed at may not be in
    /// the text any more.
    pub last_find: Option<FindMotion>,
    /// Why the last find motion did not land, if it did not.
    ///
    /// Disclosed in the status row. A find that stops because the character
    /// is absent and one that stops at [`MAX_EDITOR_FIND_BYTES`] otherwise
    /// look identical — like a dropped keypress — and they call for
    /// different answers, so the bound is stated rather than hidden.
    pub find_note: FindNote,
    /// The `:` command line while command mode is open, without the leading
    /// `:`. Empty when command mode is closed.
    ///
    /// Bounded to [`MAX_EDITOR_COMMAND_LINE`](super::editor::MAX_EDITOR_COMMAND_LINE).
    /// This is a command *name*, never document text, so it is exempt from
    /// the text-authority guard for the same reason
    /// [`pending_prefix`](Self::pending_prefix) is.
    pub command_line: String,
    /// Whether the `:` command line is open.
    pub command_active: bool,
    /// Index into [`complete_editor_command`](super::editor::complete_editor_command)
    /// for the highlighted palette row.
    pub command_selection: usize,
    /// The `/` search line while search mode is open, without the leading
    /// `/`. Empty when search mode is closed.
    ///
    /// A *needle*, never document text: it is bounded by
    /// [`MAX_EDITOR_SEARCH_LINE`](super::editor::MAX_EDITOR_SEARCH_LINE) and
    /// capped at the keystroke, so it cannot grow into a second buffer.
    pub search_line: String,
    /// Whether the `/` search line is open and owns the keyboard.
    pub search_active: bool,
    /// Absolute byte offsets of every retained match for
    /// [`search_line`](Self::search_line), ascending.
    ///
    /// Positions, not text — the renderer resolves each against the row it is
    /// painting. Bounded by
    /// [`MAX_EDITOR_SEARCH_MATCHES`](super::editor::MAX_EDITOR_SEARCH_MATCHES).
    pub search_matches: Vec<usize>,
    /// Index into [`search_matches`](Self::search_matches) the cursor is
    /// parked on. Only meaningful when there is at least one match.
    pub search_index: usize,
    /// Whether a search dropped matches past its bound or stopped scanning
    /// before the end of the document. Disclosed in the status row: a
    /// partial match list that reads as complete is the same defect as
    /// unvalidated LSP coverage.
    pub search_partial: bool,
    /// Frontend-local undo history of inverse transactions.
    pub undo: Vec<EditorUndoEntry>,
    /// Frontend-local redo history of inverse transactions.
    pub redo: Vec<EditorUndoEntry>,
    /// Retained undo payload bytes, for the undo memory bound.
    pub undo_bytes: usize,
    /// Retained redo payload bytes, for the redo memory bound.
    pub redo_bytes: usize,
    /// The open insert session, if insert mode has typed anything.
    ///
    /// Held beside the history because it *is* history that has not been
    /// written yet: the edits of one typing burst become one undo entry
    /// when the burst ends. The name deliberately does not contain
    /// `prefix` — the prefix-state guard allows exactly one such field on
    /// this struct, and the pending prefix already holds it.
    pub insert_session: Option<crate::tui::editor::InsertSession>,
}

impl TuiDocumentPresentation {
    /// Reset to a post-open presentation: cursor at the start of the
    /// document, no selection, no history. Undo never survives opening a
    /// different document.
    ///
    /// Keyboard ownership is deliberately *not* recorded here. It used to be
    /// (`focus`), and that second axis is exactly why opening a file rendered
    /// a buffer no key could reach: focus defaulted to the composer, so the
    /// editor claimed nothing and the bare-text gate swallowed everything else.
    /// The one axis is `App::ui_state.input_mode` — Insert puts the keyboard
    /// in the document, Normal leaves it with the Helix verbs and the bottom
    /// input box.
    pub fn reset_for_open(&mut self) {
        self.cursor_byte = 0;
        self.clear_selections();
        self.viewport_line = 0;
        self.viewport_column = 0;
        self.mode = EditorMode::Normal;
        self.desired_column = None;
        self.disarm_prefix();
        super::editor::clear_history(self);
        self.close_command_line();
        self.cancel_search();
    }

    /// Arm `prefix`, waiting for its next key.
    ///
    /// Replacing rather than stacking is the grammar: one pending prefix at a
    /// time, so no gesture can ever need a third key.
    pub fn arm_prefix(&mut self, prefix: EditorPrefix) {
        self.pending_prefix = prefix;
    }

    /// Disarm any armed prefix, leaving the key that armed it unaffected.
    ///
    /// Called wherever the pending key's target stops being the thing on
    /// screen: opening a document, leaving the editor route, moving focus to
    /// the tree pane, and every reconciliation that rewrites the text.
    pub fn disarm_prefix(&mut self) {
        self.pending_prefix = EditorPrefix::Idle;
    }

    /// Drop every held selection, leaving the cursor where it is.
    ///
    /// The reset point for every place the set must not survive: opening a
    /// document, leaving Insert, and every accepted edit. Kept as one
    /// operation rather than two field writes because a set that kept its
    /// members while losing `primary_index` would silently resolve every
    /// primary-scoped accessor to a different member than the one it resolved
    /// to a moment earlier.
    pub fn clear_selections(&mut self) {
        self.selections.clear();
        self.primary_index = 0;
    }

    /// Put back a set recorded before an edit — undo and redo only.
    ///
    /// The recorded members address the **pre-edit** bytes, and undo/redo
    /// restore exactly those bytes, so they need no re-clamping: the offsets
    /// are correct again by the time this runs. Ordering and the primary are
    /// re-derived through the shared normalizer so a restored set obeys the
    /// same "document order, primary names a member" rule a live one does.
    pub fn restore_selections(&mut self, selections: &[Selection<EditorPoint>]) {
        let (selections, primary) =
            super::helix_selection::normalize_set(selections.to_vec(), self.primary_index);
        self.selections = selections;
        self.primary_index = primary;
    }

    /// Forget the repeatable find and clear its disclosure.
    ///
    /// Called whenever something else becomes the last motion, which is how
    /// Helix's single repeat slot behaves: `f x`, `j`, `Alt-.` finds nothing
    /// to repeat, because `j` overwrote the motion `f x` recorded. Also
    /// called on a reconciliation, where the recorded character may no longer
    /// exist in the text at all.
    ///
    /// Kept as one operation rather than two assignments because the two
    /// halves have no meaning apart from each other: a disclosure about a
    /// find that cannot be repeated, or a repeatable find whose disclosure is
    /// missing, are both a status row lying about the last keystroke.
    pub fn forget_find(&mut self) {
        self.last_find = None;
        self.find_note = FindNote::None;
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

    // ── `/` search ──────────────────────────────────────────────────────────
    //
    // Mirrors the `:` command line's shape on purpose: both are modal input
    // lines owned by the editor that appear in the status row, so a reader
    // who learned where to look for `:` already knows where to look for `/`.

    /// Open the `/` search line, seeded with `seed`.
    ///
    /// Seeding is why `host_status` on the cursor and `/` are the same
    /// gesture: the common case is refining a word you are already looking
    /// at, and starting from an empty line makes the first keystrokes
    /// redundant.
    pub fn open_search_line(&mut self, seed: &str) {
        self.search_active = true;
        self.search_line = seed
            .chars()
            .take(super::editor::MAX_EDITOR_SEARCH_LINE)
            .collect();
        self.search_index = 0;
    }

    /// Close the `/` line and keep the highlight.
    ///
    /// What `Enter` does: the search is a thing you did, not a thing you
    /// cancelled, so the matches stay painted and `n`/`N` keep working.
    pub fn close_search_line(&mut self) {
        self.search_active = false;
    }

    /// Close the `/` line and drop the highlight entirely.
    ///
    /// What `Esc` does. The match set is derived state — leaving it behind
    /// would highlight text the user has explicitly disowned.
    pub fn cancel_search(&mut self) {
        self.search_active = false;
        self.search_line.clear();
        self.search_matches.clear();
        self.search_index = 0;
        self.search_partial = false;
    }

    /// Append one character to the search line, bounded by
    /// [`MAX_EDITOR_SEARCH_LINE`](super::editor::MAX_EDITOR_SEARCH_LINE).
    ///
    /// Returns `false` at the bound so the caller can leave the line
    /// untouched rather than dropping the keystroke after mutating it.
    pub fn push_search_char(&mut self, ch: char) -> bool {
        if self.search_line.chars().count() >= super::editor::MAX_EDITOR_SEARCH_LINE {
            return false;
        }
        self.search_line.push(ch);
        true
    }

    /// Delete the last character of the search line.
    pub fn pop_search_char(&mut self) {
        self.search_line.pop();
    }

    /// Adopt a resolved search and park the cursor on its first match.
    ///
    /// Returns the offset the cursor should move to, or `None` when nothing
    /// matched — a search that finds nothing must leave the cursor alone
    /// rather than yanking it to the top of the file.
    pub fn adopt_search(
        &mut self,
        outcome: super::editor::SearchOutcome,
        cursor: usize,
    ) -> Option<usize> {
        self.search_matches = outcome.matches;
        self.search_partial = outcome.truncated || outcome.bounded;
        // `first_match_index`, not `next_match_index`: a cursor already
        // sitting on a match must stay on it while the needle is being
        // refined, or every keystroke walks the search further away.
        self.search_index =
            super::editor::first_match_index(&self.search_matches, cursor).unwrap_or(0);
        self.search_matches.get(self.search_index).copied()
    }

    /// Move to the next/previous match, wrapping at either end.
    ///
    /// Returns the new offset so the caller can move the cursor and scroll
    /// the viewport; `None` means there is nothing to move to.
    pub fn step_search(&mut self, forward: bool) -> Option<usize> {
        let index = if forward {
            super::editor::next_match_index(&self.search_matches, self.cursor_byte)
        } else {
            super::editor::previous_match_index(&self.search_matches, self.cursor_byte)
        }?;
        self.search_index = index;
        self.search_matches.get(index).copied()
    }

    /// Whether a search highlight is currently painted.
    pub fn has_search(&self) -> bool {
        !self.search_matches.is_empty()
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
            viewport_line: 4,
            ..TuiDocumentPresentation::default()
        };
        assert_eq!(session.presentation().cursor_byte, 2);
        assert_eq!(
            session.controller().snapshot().await.unwrap().0.to_string(),
            "text"
        );
    }

    #[test]
    fn a_reset_presentation_opens_in_normal_and_the_two_axes_agree() {
        let mut presentation = TuiDocumentPresentation {
            cursor_byte: 9,
            mode: EditorMode::Insert,
            ..TuiDocumentPresentation::default()
        };
        presentation.reset_for_open();

        // A file opens in Normal, like every other reader surface. The
        // danger is not the value itself but that there are two axes: the
        // route's `input_mode` and this presentation's `mode`. A reset that
        // sets one and not the other is exactly the drift that once let a
        // document render with the keyboard nowhere in it.
        assert_eq!(presentation.mode, EditorMode::Normal);
        assert_eq!(presentation.cursor_byte, 0);
        assert_eq!(presentation.viewport_line, 0);
        assert!(presentation.selections.is_empty());
        assert_eq!(presentation.primary_index, 0);
    }

    #[test]
    fn a_reset_presentation_drops_a_carried_over_selection_set() {
        use crate::tui::editor::EditorPoint;
        use crate::tui::helix_selection::{Selection, SelectionMode};
        let mut presentation = TuiDocumentPresentation {
            selections: vec![
                Selection {
                    anchor: EditorPoint { byte: 0, line: 0 },
                    head: EditorPoint { byte: 3, line: 0 },
                    mode: SelectionMode::Char,
                    growing: false,
                },
                Selection {
                    anchor: EditorPoint { byte: 9, line: 1 },
                    head: EditorPoint { byte: 12, line: 1 },
                    mode: SelectionMode::Line,
                    growing: true,
                },
            ],
            primary_index: 1,
            ..TuiDocumentPresentation::default()
        };
        presentation.reset_for_open();
        // Not just empty: the primary index must reset with it. A set that
        // kept `primary_index` pointing past the end would make every
        // primary-scoped accessor answer about a member that no longer exists.
        assert!(presentation.selections.is_empty());
        assert_eq!(presentation.primary_index, 0);
    }

    #[test]
    fn a_reset_presentation_drops_a_carried_over_search() {
        let mut presentation = TuiDocumentPresentation::default();
        presentation.open_search_line("needle");
        presentation.search_matches = vec![0, 4];
        presentation.search_index = 1;
        presentation.search_partial = true;
        presentation.reset_for_open();
        assert!(!presentation.has_search());
        assert!(!presentation.search_active);
        assert!(presentation.search_line.is_empty());
        assert!(!presentation.search_partial);
    }

    #[test]
    fn the_search_line_is_a_needle_and_never_grows_into_a_buffer() {
        let mut presentation = TuiDocumentPresentation::default();
        presentation.open_search_line("");
        for _ in 0..(super::super::editor::MAX_EDITOR_SEARCH_LINE * 2) {
            presentation.push_search_char('x');
        }
        assert_eq!(
            presentation.search_line.chars().count(),
            super::super::editor::MAX_EDITOR_SEARCH_LINE
        );
        presentation.pop_search_char();
        assert_eq!(
            presentation.search_line.chars().count(),
            super::super::editor::MAX_EDITOR_SEARCH_LINE - 1
        );
    }
}
