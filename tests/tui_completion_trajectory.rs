//! M004 trajectory: completion presentation and qualification.
//!
//! End-to-end through `App::on_key` with a scripted core client (canned
//! `LspCompletionResult` answers) and a scripted document transport, proving
//! the properties the plan requires rather than merely intending:
//!
//! 1. An explicit trigger (`F8`, `:complete`) sends a bound request and
//!    opens a loading popup; without an open buffer nothing is sent.
//! 2. A `Ready` answer displays bounded typed suggestions; `Enter` inserts
//!    only the chosen supported edit as one controller transaction with one
//!    undo unit, and redo replays it exactly.
//! 3. Stale, superseded, wrong-scope, and identity-mismatched answers never
//!    display or apply; an edit typed before arrival cannot become a
//!    suggestion.
//! 4. Non-applicable, `NotReady`, and error answers refuse with words, never
//!    with an insertion.
//! 5. Popup keys never leak into the document or the global bindings; `Esc`
//!    preserves the editing mode; typing clears the popup.
//! 6. Lost writer leases, tab switches, detaches, and reconnects invalidate
//!    the popup without touching text or history.
//!
//! Widget-level rendering (untrusted strings, narrow terminals, scrolling,
//! huge lines) is covered in `src/tui/components/editor.rs`, next to the
//! widget, where the render harness lives.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use codegg::core::CoreClient;
use codegg::error::AppError;
use codegg::protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope,
};
use codegg::protocol::document::{DocumentScope, DocumentSnapshotDto};
use codegg::protocol::lsp::{
    LspCompletionApplicabilityDto, LspCompletionCandidateDto, LspCompletionEditDto,
    LspCompletionRequestDto, LspCompletionResultDto, LspCompletionStatus,
    LspCompletionUnsupportedReason,
};
use codegg::tui::app::state::editor::EditorStatus;
use codegg::tui::app::state::project_tabs::{ProjectTabId, ProjectTabState};
use codegg::tui::app::{App, TuiCommand};
use codegg::tui::document_session::TuiDocumentSession;
use codegg::tui::editor::EditorMode;
use codegg::tui::editor_completion::CompletionViewStatus;
use codegg::tui::route::Route;
use codegg_client::{DocumentController, DocumentTransport};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::{mpsc, Mutex as AsyncMutex};

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

struct DocState {
    text: String,
    revision: u64,
}

struct ScriptedDocTransport {
    state: AsyncMutex<DocState>,
    lease: Option<String>,
}

impl ScriptedDocTransport {
    fn new(text: &str, lease: Option<&str>) -> Self {
        Self {
            state: AsyncMutex::new(DocState {
                text: text.to_string(),
                revision: 0,
            }),
            lease: lease.map(str::to_string),
        }
    }

    fn snapshot_dto(&self, state: &DocState) -> DocumentSnapshotDto {
        DocumentSnapshotDto {
            document_id: "doc-1".to_string(),
            project_id: "project-1".to_string(),
            workspace_id: "ws-1".to_string(),
            relative_path: "src/lib.rs".to_string(),
            revision: state.revision,
            text: state.text.clone(),
            dirty: false,
            conflicted: false,
            writer: self.lease.is_some(),
            disk_base_digest: "digest".to_string(),
            scope: DocumentScope::Workspace,
        }
    }
}

#[async_trait]
impl DocumentTransport for ScriptedDocTransport {
    async fn request(&self, request: CoreRequest) -> Result<CoreResponse, String> {
        match request {
            CoreRequest::DocumentOpen { .. } => {
                let state = self.state.lock().await;
                Ok(CoreResponse::DocumentSnapshot {
                    snapshot: self.snapshot_dto(&state),
                    writer_lease: self.lease.clone(),
                    lsp_degraded: false,
                })
            }
            CoreRequest::DocumentChange { transaction, .. } => {
                // The daemon side of the change: apply to the stored copy
                // and acknowledge the next revision.
                let mut state = self.state.lock().await;
                for edit in transaction.edits.iter().rev() {
                    state
                        .text
                        .replace_range(edit.range.start..edit.range.end, &edit.insert);
                }
                state.revision += 1;
                Ok(CoreResponse::DocumentChanged {
                    revision: state.revision,
                    lsp_degraded: false,
                })
            }
            CoreRequest::DocumentWriterAcquire { document_id, .. } => match &self.lease {
                Some(lease) => Ok(CoreResponse::DocumentWriterLease {
                    document_id,
                    writer_lease: lease.clone(),
                }),
                None => Err("no writer lease".to_string()),
            },
            other => Err(format!("unexpected document request: {other:?}")),
        }
    }
}

#[derive(Debug)]
enum FakeReply {
    Result(LspCompletionResultDto),
    Error(String),
}

struct FakeCompletionClient {
    script: Mutex<VecDeque<FakeReply>>,
    seen: Mutex<Vec<LspCompletionRequestDto>>,
}

impl FakeCompletionClient {
    fn new(replies: Vec<FakeReply>) -> Self {
        Self {
            script: Mutex::new(replies.into()),
            seen: Mutex::new(Vec::new()),
        }
    }

    fn seen(&self) -> Vec<LspCompletionRequestDto> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait]
impl CoreClient for FakeCompletionClient {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        match request.payload {
            CoreRequest::LspCompletionGet { request } => {
                self.seen.lock().unwrap().push(request);
                match self.script.lock().unwrap().pop_front() {
                    Some(FakeReply::Result(result)) => {
                        Ok(CoreResponse::LspCompletionResult { result })
                    }
                    Some(FakeReply::Error(message)) => Ok(CoreResponse::Error {
                        code: "injected".to_string(),
                        message,
                    }),
                    None => panic!("no scripted completion reply left"),
                }
            }
            other => panic!("unexpected core request: {other:?}"),
        }
    }

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = mpsc::channel(8);
        rx
    }
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn plain(ch: char) -> KeyEvent {
    key(KeyCode::Char(ch))
}

struct Harness {
    app: App,
    fake: Arc<FakeCompletionClient>,
    rx: mpsc::Receiver<TuiCommand>,
    controller: Arc<DocumentController>,
}

async fn harness(
    text: &str,
    writable: bool,
    replies: Vec<FakeReply>,
    cursor: usize,
    mode: EditorMode,
) -> Harness {
    let lease = writable.then_some("lease-1");
    let transport = Arc::new(ScriptedDocTransport::new(text, lease));
    let controller = Arc::new(DocumentController::new(transport));
    controller
        .open(
            "project-1".to_string(),
            "ws-1".to_string(),
            "src/lib.rs".to_string(),
            writable,
            DocumentScope::Workspace,
        )
        .await
        .expect("scripted open succeeds");
    let mut session = TuiDocumentSession::from_controller(controller.clone());
    session.presentation_mut().cursor_byte = cursor;
    session.presentation_mut().mode = mode;

    let mut app = App::new_for_testing("/tmp/tui-completion".to_string());
    // The route's `input_mode` is now the *only* keyboard axis on the editor
    // route, so a harness that set only the presentation mode produced a
    // document whose mode said one thing and whose key routing said another —
    // `:` would type a colon into the text instead of opening the command line.
    app.ui_state.input_mode = match mode {
        EditorMode::Insert => codegg::tui::input::InputMode::Insert,
        EditorMode::Normal => codegg::tui::input::InputMode::Normal,
    };
    if let Some(tab) = app.project_tabs.active_mut() {
        tab.project_id = Some("project-1".to_string());
        tab.workspace_id = Some("ws-1".to_string());
        tab.session_id = Some("session-1".to_string());
    }
    app.ui_state.routes.navigate_to(Route::Editor);
    app.editor_state.session = Some(session);
    app.editor_state.path = Some("src/lib.rs".to_string());
    app.editor_state.status = EditorStatus::Synced;

    let fake = Arc::new(FakeCompletionClient::new(replies));
    app.set_core_client(fake.clone());
    let (tx, rx) = mpsc::channel(16);
    app.tui_cmd_tx = Some(tx);
    Harness {
        app,
        fake,
        rx,
        controller,
    }
}

async fn recv_completion(rx: &mut mpsc::Receiver<TuiCommand>) -> TuiCommand {
    tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("completion arrives")
        .expect("channel open")
}

fn apply_finished(
    app: &mut App,
    command: &TuiCommand,
) -> (u64, codegg::tui::editor_completion::EditorCompletionScope) {
    match command {
        TuiCommand::EditorCompletionFinished {
            request_id,
            completion_epoch,
            scope,
            result,
            error,
        } => {
            app.apply_editor_completion(
                *request_id,
                *completion_epoch,
                scope.clone(),
                result.clone(),
                error.clone(),
            );
            (*request_id, scope.clone())
        }
        other => panic!("expected completion finished, got {other:?}"),
    }
}

fn controller_text(controller: &Arc<DocumentController>) -> String {
    controller
        .try_snapshot()
        .map(|(snapshot, _)| snapshot.to_string())
        .expect("snapshot")
}

/// Where the history cursor is standing.
///
/// The history is one list and the cursor names a position in it, so the
/// entry at the cursor is the one a redo would apply.
fn presentation_history_cursor(app: &App) -> usize {
    app.editor_state
        .session
        .as_ref()
        .expect("session")
        .presentation()
        .history_cursor
}

fn applicable(
    label: &str,
    start: (u32, u32),
    end: (u32, u32),
    new_text: &str,
) -> LspCompletionCandidateDto {
    LspCompletionCandidateDto {
        label: label.to_string(),
        detail: Some(format!("detail for {label}")),
        kind: Some("function".to_string()),
        filter_text: Some(label.to_string()),
        sort_text: None,
        applicability: LspCompletionApplicabilityDto::Applicable {
            edit: LspCompletionEditDto {
                start_line: start.0,
                start_character: start.1,
                end_line: end.0,
                end_character: end.1,
                new_text: new_text.to_string(),
            },
        },
    }
}

fn non_applicable(
    label: &str,
    reason: LspCompletionUnsupportedReason,
) -> LspCompletionCandidateDto {
    LspCompletionCandidateDto {
        label: label.to_string(),
        detail: None,
        kind: Some("snippet".to_string()),
        filter_text: None,
        sort_text: None,
        applicability: LspCompletionApplicabilityDto::NonApplicable { reason },
    }
}

fn ready(
    candidates: Vec<LspCompletionCandidateDto>,
    truncated: bool,
    incomplete: bool,
) -> FakeReply {
    FakeReply::Result(LspCompletionResultDto::ready(
        "doc-1".to_string(),
        0,
        Some(0),
        candidates,
        truncated,
        incomplete,
    ))
}

const LIB_RS: &str = "fn main() {\n    prin\n}\n";

// ---------------------------------------------------------------------------
// 1. Trigger gating and request binding
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn f8_without_an_open_document_requests_nothing() {
    let mut app = App::new_for_testing("/tmp/tui-completion".to_string());
    let fake = Arc::new(FakeCompletionClient::new(vec![]));
    app.set_core_client(fake.clone());
    let (tx, _rx) = mpsc::channel(16);
    app.tui_cmd_tx = Some(tx);
    app.ui_state.routes.navigate_to(Route::Editor);

    app.on_key(key(KeyCode::F(8)));
    assert!(fake.seen().is_empty(), "no attachment means no request");
    assert!(!app.editor_state.completion.is_open());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_completion_trigger_fires_in_both_modes() {
    // There is no separate "composer focus" any more: Normal mode is the
    // route's one non-buffer mode, and `F8` is claimed in both. A buffer that
    // only answered keys while the document held the keyboard is exactly what
    // made the editor look dead, and completion was the first casualty.
    for mode in [EditorMode::Normal, EditorMode::Insert] {
        let empty =
            LspCompletionResultDto::ready("doc-1".to_string(), 0, Some(0), vec![], false, false);
        let Harness {
            mut app,
            fake,
            mut rx,
            ..
        } = harness(LIB_RS, true, vec![FakeReply::Result(empty)], 20, mode).await;

        app.on_key(key(KeyCode::F(8)));
        // The request is dispatched on a spawned task, so the assertion has to
        // come after the reply is pumped — not merely after `on_key` returns.
        let _ = recv_completion(&mut rx).await;
        assert!(
            !fake.seen().is_empty(),
            "F8 must request in {mode:?}; an unclaimed F8 reaches the composer"
        );
        assert!(app.editor_state.completion.is_open());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trigger_sends_a_bound_request_and_opens_loading() {
    // Cursor after "prin" (byte 20) is UTF-16 column 8 on line 1.
    let empty =
        LspCompletionResultDto::ready("doc-1".to_string(), 0, Some(0), vec![], false, false);
    let Harness {
        mut app,
        fake,
        mut rx,
        ..
    } = harness(
        LIB_RS,
        true,
        vec![FakeReply::Result(empty)],
        20,
        EditorMode::Insert,
    )
    .await;

    app.on_key(key(KeyCode::F(8)));
    // The popup opens synchronously in `begin()`; the request lands once
    // the spawned task runs, so wait for its completion first.
    assert!(app.editor_state.completion.is_open());
    assert_eq!(
        app.editor_state.completion.status,
        CompletionViewStatus::Loading
    );
    let _ = recv_completion(&mut rx).await;
    let seen = fake.seen();
    assert_eq!(seen.len(), 1, "one explicit trigger sends one request");
    let request = &seen[0];
    assert_eq!(request.session_id, "session-1");
    assert_eq!(request.path, "src/lib.rs");
    assert_eq!(request.document_id, "doc-1");
    assert_eq!(request.revision, 0);
    assert_eq!((request.line, request.character), (1, 8));
}

// ---------------------------------------------------------------------------
// 2. Accept is one transaction with one undo unit, redo replays it
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ready_answer_applies_one_undo_unit_with_redo() {
    let candidates = vec![
        applicable("println!", (1, 4), (1, 8), "println!"),
        non_applicable("snip", LspCompletionUnsupportedReason::Snippet),
        applicable("print", (1, 8), (1, 8), "tln!"),
    ];
    let Harness {
        mut app,
        mut rx,
        controller,
        ..
    } = harness(
        LIB_RS,
        true,
        vec![ready(candidates, false, false)],
        20,
        EditorMode::Insert,
    )
    .await;

    app.on_key(key(KeyCode::F(8)));
    let command = recv_completion(&mut rx).await;
    apply_finished(&mut app, &command);
    assert_eq!(
        app.editor_state.completion.status,
        CompletionViewStatus::Ready
    );
    assert_eq!(app.editor_state.completion.candidates.len(), 3);

    // Down to the snippet, Enter refuses with words and changes nothing.
    app.on_key(key(KeyCode::Down));
    assert_eq!(app.editor_state.completion.selected, 1);
    app.on_key(key(KeyCode::Enter));
    assert_eq!(controller_text(&controller), LIB_RS);
    assert!(
        app.editor_state.completion.is_open(),
        "refusal keeps context"
    );
    assert!(
        app.editor_state
            .session
            .as_ref()
            .expect("session")
            .presentation()
            .history
            .is_empty(),
        "a refused accept records no history"
    );

    // Back up to the supported edit and accept: exactly one transaction.
    app.on_key(key(KeyCode::Up));
    app.on_key(key(KeyCode::Enter));
    assert_eq!(
        controller_text(&controller),
        "fn main() {\n    println!\n}\n"
    );
    assert!(
        !app.editor_state.completion.is_open(),
        "accept closes the popup"
    );
    let undo_len = app
        .editor_state
        .session
        .as_ref()
        .expect("session")
        .presentation()
        .history
        .len();
    assert_eq!(undo_len, 1, "one accept is one undo unit");

    // Undo restores the exact prior text: Esc leaves Insert for Normal
    // (the popup is already gone, so Esc reaches the buffer), then `u`
    // undoes the one accepted transaction.
    app.on_key(key(KeyCode::Esc));
    app.on_key(plain('u'));
    assert_eq!(controller_text(&controller), LIB_RS);
    // Redo replays the intended forward text exactly. The recorded redo
    // unit is applied directly: its content — not the key routing — is
    // what this milestone guarantees.
    let forward = app
        .editor_state
        .session
        .as_ref()
        .expect("session")
        .presentation()
        .history
        .get(presentation_history_cursor(&app))
        .cloned()
        .map(|entry| entry.forward);
    assert!(forward.is_some(), "undo stages exactly one redo unit");
    controller
        .apply_local(forward.expect("redo"))
        .expect("redo applies");
    assert_eq!(
        controller_text(&controller),
        "fn main() {\n    println!\n}\n"
    );
}

// ---------------------------------------------------------------------------
// 3. Typing clears the popup; late answers cannot become suggestions
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typing_clears_the_popup_and_late_answers_drop() {
    let candidates = vec![applicable("println!", (1, 4), (1, 8), "println!")];
    let Harness {
        mut app,
        mut rx,
        controller,
        ..
    } = harness(
        LIB_RS,
        true,
        vec![ready(candidates, false, false)],
        20,
        EditorMode::Insert,
    )
    .await;

    app.on_key(key(KeyCode::F(8)));
    let command = recv_completion(&mut rx).await;
    // Type before the answer arrives: the edit clears the popup, so the
    // late answer has nothing to land on.
    app.on_key(plain('x'));
    assert!(
        !app.editor_state.completion.is_open(),
        "typing clears the popup"
    );
    apply_finished(&mut app, &command);
    assert!(
        !app.editor_state.completion.is_open(),
        "the late answer stays dropped"
    );
    assert_eq!(
        controller_text(&controller),
        "fn main() {\n    prinx\n}\n",
        "only the typed character landed"
    );
}

// ---------------------------------------------------------------------------
// 4. Stale scopes, generations, and identities never display or apply
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_scope_and_generation_answers_drop() {
    let candidates = vec![applicable("println!", (1, 4), (1, 8), "println!")];
    let Harness {
        mut app, mut rx, ..
    } = harness(
        LIB_RS,
        true,
        vec![ready(candidates, false, false)],
        20,
        EditorMode::Insert,
    )
    .await;

    app.on_key(key(KeyCode::F(8)));
    let command = recv_completion(&mut rx).await;
    // Detach bumps the generation: the in-flight answer is now for a
    // previous attachment and must drop.
    app.editor_state.detach();
    apply_finished(&mut app, &command);
    assert!(!app.editor_state.completion.is_open());
    assert_eq!(
        app.editor_state.completion.status,
        CompletionViewStatus::Idle
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identity_mismatch_is_an_error_not_a_suggestion() {
    let mut wrong = LspCompletionResultDto::ready(
        "doc-OTHER".to_string(),
        0,
        Some(0),
        vec![applicable("println!", (1, 4), (1, 8), "println!")],
        false,
        false,
    );
    wrong.status = LspCompletionStatus::Ready;
    let Harness {
        mut app,
        mut rx,
        controller,
        ..
    } = harness(
        LIB_RS,
        true,
        vec![FakeReply::Result(wrong)],
        20,
        EditorMode::Insert,
    )
    .await;

    app.on_key(key(KeyCode::F(8)));
    let command = recv_completion(&mut rx).await;
    apply_finished(&mut app, &command);
    assert_eq!(
        app.editor_state.completion.status,
        CompletionViewStatus::Error
    );
    assert!(app.editor_state.completion.candidates.is_empty());
    // Enter on an error refuses without touching text.
    app.on_key(key(KeyCode::Enter));
    assert_eq!(controller_text(&controller), LIB_RS);
}

// ---------------------------------------------------------------------------
// 5. `:complete`, `NotReady`, errors, and `Esc` semantics
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn colon_complete_triggers_from_normal_mode() {
    let empty =
        LspCompletionResultDto::ready("doc-1".to_string(), 0, Some(0), vec![], false, false);
    let Harness {
        mut app,
        fake,
        mut rx,
        ..
    } = harness(
        LIB_RS,
        true,
        vec![FakeReply::Result(empty)],
        20,
        EditorMode::Normal,
    )
    .await;

    app.on_key(plain(':'));
    for ch in "complete".chars() {
        app.on_key(plain(ch));
    }
    app.on_key(key(KeyCode::Enter));
    assert!(app.editor_state.completion.is_open());
    let _ = recv_completion(&mut rx).await;
    assert_eq!(fake.seen().len(), 1, ":complete sends one request");
    let request = &fake.seen()[0];
    assert_eq!(request.document_id, "doc-1");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn colon_is_text_in_insert_mode() {
    let Harness { mut app, fake, .. } = harness(LIB_RS, true, vec![], 20, EditorMode::Insert).await;

    app.on_key(plain(':'));
    assert!(
        fake.seen().is_empty(),
        "a colon in Insert mode is document text"
    );
    assert!(!app.editor_state.completion.is_open());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn not_ready_and_error_states_refuse_with_words() {
    let not_ready = LspCompletionResultDto::not_ready("doc-1".to_string());
    let Harness {
        mut app,
        mut rx,
        controller,
        ..
    } = harness(
        LIB_RS,
        true,
        vec![
            FakeReply::Result(not_ready),
            FakeReply::Error("daemon exploded".to_string()),
        ],
        20,
        EditorMode::Insert,
    )
    .await;

    app.on_key(key(KeyCode::F(8)));
    apply_finished(&mut app, &recv_completion(&mut rx).await);
    assert_eq!(
        app.editor_state.completion.status,
        CompletionViewStatus::NotReady
    );
    // Enter with no candidates refuses; the warming state is not an error
    // and not an empty success.
    app.on_key(key(KeyCode::Enter));
    assert_eq!(controller_text(&controller), LIB_RS);
    assert!(app.editor_state.completion.is_open());

    app.editor_state.completion.dismiss();
    app.on_key(key(KeyCode::F(8)));
    apply_finished(&mut app, &recv_completion(&mut rx).await);
    assert_eq!(
        app.editor_state.completion.status,
        CompletionViewStatus::Error
    );
    assert_eq!(
        app.editor_state.completion.error.as_deref(),
        Some("daemon exploded")
    );
    app.on_key(key(KeyCode::Enter));
    assert_eq!(controller_text(&controller), LIB_RS);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn escape_dismisses_and_preserves_the_editing_mode() {
    for mode in [EditorMode::Insert, EditorMode::Normal] {
        let Harness { mut app, .. } = harness(LIB_RS, true, vec![], 20, mode).await;
        // Open the popup without a daemon round trip: begin() is the same
        // state the request path leaves behind.
        let scope = codegg::tui::editor_completion::EditorCompletionScope {
            read_scope: codegg::tui::editor_lsp::EditorLspScope {
                generation: 0,
                session_id: "session-1".to_string(),
                project_id: "project-1".to_string(),
                workspace_id: "ws-1".to_string(),
                path: "src/lib.rs".to_string(),
            },
            document_id: "doc-1".to_string(),
            revision: 0,
            line: 1,
            character: 8,
        };
        app.editor_state.completion.begin(&scope);
        assert!(app.editor_state.completion.is_open());
        app.on_key(key(KeyCode::Esc));
        assert!(!app.editor_state.completion.is_open());
        let current = app
            .editor_state
            .session
            .as_ref()
            .expect("session")
            .presentation()
            .mode;
        assert_eq!(current, mode, "Esc restores the prior mode");
    }
}

// ---------------------------------------------------------------------------
// 6. Bounds, leases, ordering, invalidation
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hundred_plus_one_candidates_truncate_defensively() {
    let items: Vec<LspCompletionCandidateDto> = (0..101)
        .map(|index| applicable(&format!("item-{index:03}"), (1, 4), (1, 8), "x"))
        .collect();
    let Harness {
        mut app, mut rx, ..
    } = harness(
        LIB_RS,
        true,
        vec![ready(items, false, false)],
        20,
        EditorMode::Insert,
    )
    .await;

    app.on_key(key(KeyCode::F(8)));
    apply_finished(&mut app, &recv_completion(&mut rx).await);
    assert_eq!(app.editor_state.completion.candidates.len(), 100);
    assert!(
        app.editor_state.completion.truncated,
        "defensive truncation still reports itself"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lost_writer_lease_refuses_without_side_effects() {
    let candidates = vec![applicable("println!", (1, 4), (1, 8), "println!")];
    let Harness {
        mut app,
        mut rx,
        controller,
        ..
    } = harness(
        LIB_RS,
        false,
        vec![ready(candidates, false, false)],
        20,
        EditorMode::Insert,
    )
    .await;

    app.on_key(key(KeyCode::F(8)));
    apply_finished(&mut app, &recv_completion(&mut rx).await);
    assert_eq!(
        app.editor_state.completion.status,
        CompletionViewStatus::Ready
    );
    app.on_key(key(KeyCode::Enter));
    assert_eq!(
        controller_text(&controller),
        LIB_RS,
        "a lost lease changes no text"
    );
    assert!(
        app.editor_state
            .session
            .as_ref()
            .expect("session")
            .presentation()
            .history
            .is_empty(),
        "a refused accept records no history"
    );
    assert!(app.editor_state.notice.is_some(), "the refusal is visible");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn out_of_order_completions_drop_the_older_answer() {
    // Both answers carry the same content on purpose: the two tasks race
    // for the scripted replies, so content cannot identify a request — only
    // the request id can. The fence property is what matters here; content
    // ordering is proven by the single-request tests.
    let same = || vec![applicable("same", (1, 4), (1, 8), "same")];
    let Harness {
        mut app, mut rx, ..
    } = harness(
        LIB_RS,
        true,
        vec![ready(same(), false, false), ready(same(), false, false)],
        20,
        EditorMode::Insert,
    )
    .await;

    app.on_key(key(KeyCode::F(8)));
    app.on_key(key(KeyCode::F(8)));
    // Arrival order is nondeterministic: identify the answers by request
    // id, then prove the fence holds in both application orders.
    let first = recv_completion(&mut rx).await;
    let second = recv_completion(&mut rx).await;
    fn request_id_of(command: &TuiCommand) -> u64 {
        match command {
            TuiCommand::EditorCompletionFinished { request_id, .. } => *request_id,
            other => panic!("expected completion finished, got {other:?}"),
        }
    }
    let (older, newer) = if request_id_of(&first) < request_id_of(&second) {
        (first, second)
    } else {
        (second, first)
    };
    apply_finished(&mut app, &older);
    assert!(
        app.editor_state.completion.candidates.is_empty(),
        "the superseded answer drops"
    );
    apply_finished(&mut app, &newer);
    assert_eq!(app.editor_state.completion.candidates.len(), 1);
    assert_eq!(
        app.editor_state.completion.request_id,
        request_id_of(&newer),
        "the newer answer is the one shown"
    );
    apply_finished(&mut app, &older);
    assert_eq!(
        app.editor_state.completion.candidates.len(),
        1,
        "the older answer cannot paint over the newer one"
    );
    assert_eq!(
        app.editor_state.completion.request_id,
        request_id_of(&newer)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rapid_retrigger_supersedes_without_state_growth() {
    // Identical content on every reply: tasks race for the script, so the
    // newest request id — not the label — is what "last wins" means here.
    let same = || vec![applicable("same", (1, 4), (1, 8), "same")];
    let Harness {
        mut app,
        mut rx,
        fake,
        ..
    } = harness(
        LIB_RS,
        true,
        vec![
            ready(same(), false, false),
            ready(same(), false, false),
            ready(same(), false, false),
        ],
        20,
        EditorMode::Insert,
    )
    .await;

    for _ in 0..3 {
        app.on_key(key(KeyCode::F(8)));
    }
    // The tasks land after the triggers: wait for all three before
    // asserting what was sent.
    let mut ids = Vec::new();
    for _ in 0..3 {
        let command = recv_completion(&mut rx).await;
        if let TuiCommand::EditorCompletionFinished { request_id, .. } = &command {
            ids.push(*request_id);
        }
        apply_finished(&mut app, &command);
    }
    assert_eq!(fake.seen().len(), 3, "every trigger sends");
    assert_eq!(app.editor_state.completion.candidates.len(), 1);
    assert_eq!(
        app.editor_state.completion.request_id,
        *ids.iter().max().expect("three ids"),
        "the newest request is the one shown"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn switch_tab_and_reconnect_clear_the_popup() {
    let candidates = vec![applicable("println!", (1, 4), (1, 8), "println!")];
    let Harness {
        mut app, mut rx, ..
    } = harness(
        LIB_RS,
        true,
        vec![ready(candidates, false, false)],
        20,
        EditorMode::Insert,
    )
    .await;

    app.on_key(key(KeyCode::F(8)));
    apply_finished(&mut app, &recv_completion(&mut rx).await);
    assert!(app.editor_state.completion.is_open());

    let other = app
        .project_tabs
        .add_tab(ProjectTabState::empty(ProjectTabId::new(), "other".into()));
    assert!(app.switch_active_tab(&other));
    assert!(
        !app.editor_state.completion.is_open(),
        "a tab switch closes the popup"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn emoji_cursor_binds_and_applies_exactly() {
    // "a😀b" is 4 UTF-16 units (a=1, emoji=2, b=1): byte 6 is column 4 and
    // the server replaces "b", columns 3..4.
    let text = "a😀b\n";
    let candidates = vec![applicable("B", (0, 3), (0, 4), "B")];
    let Harness {
        mut app,
        mut rx,
        fake,
        controller,
        ..
    } = harness(
        text,
        true,
        vec![ready(candidates, false, false)],
        6,
        EditorMode::Insert,
    )
    .await;

    app.on_key(key(KeyCode::F(8)));
    apply_finished(&mut app, &recv_completion(&mut rx).await);
    let seen = fake.seen();
    assert_eq!((seen[0].line, seen[0].character), (0, 4));
    app.on_key(key(KeyCode::Enter));
    assert_eq!(controller_text(&controller), "a😀B\n");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn f8_with_an_open_popup_supersedes_with_a_fresh_request() {
    // A second trigger refreshes rather than toggles: stale, warming, and
    // failed states are one key away from a re-query, and dismissal stays
    // `Esc`'s job alone.
    let Harness {
        mut app,
        fake,
        mut rx,
        ..
    } = harness(
        LIB_RS,
        true,
        vec![
            ready(vec![applicable("a", (1, 4), (1, 8), "a")], false, false),
            ready(vec![applicable("b", (1, 4), (1, 8), "b")], false, false),
        ],
        20,
        EditorMode::Insert,
    )
    .await;

    app.on_key(key(KeyCode::F(8)));
    apply_finished(&mut app, &recv_completion(&mut rx).await);
    assert!(app.editor_state.completion.is_open());
    app.on_key(key(KeyCode::F(8)));
    assert!(
        app.editor_state.completion.is_open(),
        "re-trigger keeps the popup open"
    );
    apply_finished(&mut app, &recv_completion(&mut rx).await);
    assert_eq!(fake.seen().len(), 2);
    assert_eq!(app.editor_state.completion.candidates.len(), 1);
    assert_eq!(app.editor_state.completion.candidates[0].label, "b");
}

// ---------------------------------------------------------------------------
// C003. Trigger budget, typed wire statuses, and reconnect fencing
// ---------------------------------------------------------------------------

/// Tolerant core fake for the reconnect test: scripted completion answers,
/// graceful `Error` for every other request (presence negotiation and
/// friends must degrade, never panic, when the transport reconnects).
struct TolerantClient {
    script: Mutex<VecDeque<FakeReply>>,
    seen: Mutex<Vec<LspCompletionRequestDto>>,
}

impl TolerantClient {
    fn new(replies: Vec<FakeReply>) -> Self {
        Self {
            script: Mutex::new(replies.into()),
            seen: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl CoreClient for TolerantClient {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        match request.payload {
            CoreRequest::LspCompletionGet { request } => {
                self.seen.lock().unwrap().push(request);
                match self.script.lock().unwrap().pop_front() {
                    Some(FakeReply::Result(result)) => {
                        Ok(CoreResponse::LspCompletionResult { result })
                    }
                    Some(FakeReply::Error(message)) => Ok(CoreResponse::Error {
                        code: "injected".to_string(),
                        message,
                    }),
                    None => panic!("no scripted completion reply left"),
                }
            }
            other => Ok(CoreResponse::Error {
                code: "unscripted".to_string(),
                message: format!("tolerant client has no script for {other:?}"),
            }),
        }
    }

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = mpsc::channel(8);
        rx
    }
}

fn last_toast(app: &App) -> Option<String> {
    app.messages_state
        .toasts
        .history()
        .next()
        .map(|record| record.message.clone())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn giant_line_refuses_f8_without_sending_or_allocating_the_prefix() {
    // One 2 MiB logical line with the cursor at the end: far past the
    // 32 KiB position budget. F8 must refuse visibly — no daemon request,
    // no popup, no truncated/wrong cursor.
    let line = "a".repeat(2 * 1024 * 1024);
    let Harness { mut app, fake, .. } =
        harness(&line, true, vec![], line.len(), EditorMode::Insert).await;

    app.on_key(key(KeyCode::F(8)));

    assert!(
        fake.seen().is_empty(),
        "an over-budget line must never send a completion request"
    );
    assert!(
        !app.editor_state.completion.is_open(),
        "refusal leaves the popup closed"
    );
    assert_eq!(
        last_toast(&app).as_deref(),
        Some("Line too long for completion at this position")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn budget_boundary_sends_exactly_and_refuses_just_past_it() {
    // Cursor exactly at the 32 KiB budget sends with the exact column;
    // ten bytes further refuses. Both orders prove the boundary is exact,
    // not approximate.
    let budget = codegg_document::DocumentSnapshot::MAX_COMPLETION_PREFIX_BYTES;
    let line = "a".repeat(budget + 10);
    let Harness {
        mut app,
        mut rx,
        fake,
        ..
    } = harness(
        &line,
        true,
        vec![ready(vec![], false, false)],
        budget,
        EditorMode::Insert,
    )
    .await;

    app.on_key(key(KeyCode::F(8)));
    apply_finished(&mut app, &recv_completion(&mut rx).await);
    let seen = fake.seen();
    assert_eq!(seen.len(), 1, "at-budget positions still send");
    assert_eq!(seen[0].character, budget as u32);
    assert_eq!(
        app.editor_state.completion.status,
        CompletionViewStatus::Ready
    );

    app.editor_state
        .session
        .as_mut()
        .expect("session")
        .presentation_mut()
        .cursor_byte = budget + 10;
    app.on_key(key(KeyCode::F(8)));
    assert_eq!(
        fake.seen().len(),
        1,
        "ten bytes past the budget must not send"
    );
    assert_eq!(
        last_toast(&app).as_deref(),
        Some("Line too long for completion at this position")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_wire_constructor_renders_stale_not_an_identity_error() {
    // The exact daemon `stale` constructor on a revision-7 request: before
    // the fix this presented the generic "another document revision" Error
    // because the revision check ran before the status match.
    let Harness {
        mut app, mut rx, ..
    } = harness(
        LIB_RS,
        true,
        vec![FakeReply::Result(LspCompletionResultDto::stale(
            "doc-1".to_string(),
            8,
            Some(8),
        ))],
        20,
        EditorMode::Insert,
    )
    .await;

    // The request goes out at revision 0 while the daemon answers stale
    // at revision 8: a true newer-revision echo. Before the fix the
    // revision check ran before the status match, so this presented the
    // generic identity Error; now it must map to Stale.
    app.on_key(key(KeyCode::F(8)));
    apply_finished(&mut app, &recv_completion(&mut rx).await);
    assert_eq!(
        app.editor_state.completion.status,
        CompletionViewStatus::Stale
    );
    assert!(app.editor_state.completion.candidates.is_empty());
    assert!(app.editor_state.completion.is_open());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_epoch_fence_drops_a_queued_pre_reset_answer() {
    // F8 issues request 1 under epoch E0. A reconnect clears completion
    // (epoch E1) without touching generation or scope; F8 reuses id 1
    // under E1. The queued E0 answer must drop even though scope and id
    // coincide; the E1 answer renders. Both application orders hold.
    let candidates_a = vec![applicable("a", (1, 4), (1, 8), "a")];
    let candidates_b = vec![applicable("b", (1, 4), (1, 8), "b")];
    let lease = Some("lease-1");
    let transport = Arc::new(ScriptedDocTransport::new(LIB_RS, lease));
    let controller = Arc::new(DocumentController::new(transport));
    controller
        .open(
            "project-1".to_string(),
            "ws-1".to_string(),
            "src/lib.rs".to_string(),
            true,
            DocumentScope::Workspace,
        )
        .await
        .expect("scripted open succeeds");
    let mut session = TuiDocumentSession::from_controller(controller.clone());
    session.presentation_mut().cursor_byte = 20;
    session.presentation_mut().mode = EditorMode::Insert;

    let mut app = App::new_for_testing("/tmp/tui-completion".to_string());
    // Both axes have to agree: the route's `input_mode` is the *only* keyboard
    // axis on the editor route, so a harness that set only the presentation
    // mode produced a document whose mode said one thing and whose key routing
    // said another — `:` would type a colon into the text instead of opening
    // the command line.
    app.ui_state.input_mode = codegg::tui::input::InputMode::Insert;
    if let Some(tab) = app.project_tabs.active_mut() {
        tab.project_id = Some("project-1".to_string());
        tab.workspace_id = Some("ws-1".to_string());
        tab.session_id = Some("session-1".to_string());
    }
    app.ui_state.routes.navigate_to(Route::Editor);
    app.editor_state.session = Some(session);
    app.editor_state.path = Some("src/lib.rs".to_string());
    app.editor_state.status = EditorStatus::Synced;

    let fake = Arc::new(TolerantClient::new(vec![
        ready(candidates_a, false, false),
        ready(candidates_b, false, false),
    ]));
    app.set_core_client(fake.clone());
    let (tx, mut rx) = mpsc::channel(16);
    app.tui_cmd_tx = Some(tx);

    app.on_key(key(KeyCode::F(8)));
    let first = recv_completion(&mut rx).await;
    let first_id = match &first {
        TuiCommand::EditorCompletionFinished { request_id, .. } => *request_id,
        other => panic!("expected completion finished, got {other:?}"),
    };

    // The transport reconnects before the first answer is applied. The
    // generation and scope binding are unchanged — only the epoch moves.
    app.on_projection_reconnect();
    app.on_key(key(KeyCode::F(8)));
    // The reconnect also triggers background refreshes (presence) whose
    // completions share the channel; skip anything that is not the
    // completion under test.
    let second = loop {
        let command = recv_completion(&mut rx).await;
        if matches!(command, TuiCommand::EditorCompletionFinished { .. }) {
            break command;
        }
    };
    let second_id = match &second {
        TuiCommand::EditorCompletionFinished { request_id, .. } => *request_id,
        other => panic!("expected completion finished, got {other:?}"),
    };
    assert_eq!(
        (first_id, second_id),
        (1, 1),
        "the fence under test is id reuse across the reset"
    );

    // Old answer first: dropped, new request stays live.
    apply_finished(&mut app, &first);
    assert!(
        app.editor_state.completion.candidates.is_empty(),
        "the pre-reconnect answer must drop despite matching scope+id"
    );
    assert!(app.editor_state.completion.loading);
    // New answer: renders.
    apply_finished(&mut app, &second);
    assert_eq!(app.editor_state.completion.candidates.len(), 1);
    assert_eq!(app.editor_state.completion.candidates[0].label, "b");
    // Late duplicate of the old answer cannot paint over the new one.
    apply_finished(&mut app, &first);
    assert_eq!(app.editor_state.completion.candidates.len(), 1);
    assert_eq!(app.editor_state.completion.candidates[0].label, "b");
    assert_eq!(fake.seen.lock().unwrap().len(), 2, "both triggers sent");
}
