//! The desktop editor's text authority, proved against a fake transport.
//!
//! The editor's substrate is shared: the frontend-neutral `DocumentController`
//! owns the canonical text replica, and the daemon owns the document. What
//! this file proves is the half the desktop owns:
//!
//! - the host serves a **bounded window** of the controller's own snapshot,
//!   never the document;
//! - the host's edits are byte-range transactions whose inverse the host
//!   records, so undo and redo restore the exact prior bytes;
//! - the host refuses a path that is not workspace-relative;
//! - the host's state and language-server presentations distinguish "no warm
//!   server" from an empty successful answer.
//!
//! The transport is a fake, so the daemon is never contacted: the controller
//! is the shared substrate, and the fake answers it the way the daemon does.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use codegg_client::{DocumentController, DocumentTransport};
use codegg_desktop::document::{
    completion_view, diagnostics_view, navigation_view, present_state, safe_relative_path,
    slice_lines, DesktopDocumentSession, EditorCompletionView, EditorDiagnosticsView,
    EditorNavigationView, MAX_VIEWPORT_LINES,
};
use codegg_document::DocumentBuffer;
use codegg_protocol::core::{CoreRequest, CoreResponse};
use codegg_protocol::document::{DocumentScope, DocumentSnapshotDto};
use codegg_protocol::lsp::{
    LspCompletionApplicabilityDto, LspCompletionCandidateDto, LspCompletionResultDto,
    LspCompletionStatus, LspDiagnosticDto, LspDiagnosticsResultDto, LspDiagnosticsStatus,
    LspFileDiagnosticsDto, LspRangeDto, LspReadOperation, LspReadPayloadDto, LspReadResultDto,
    LspReadStatus,
};

/// A transport that answers the document protocol from a fixed text.
struct FakeTransport {
    text: String,
    seen: Mutex<Vec<String>>,
}

impl FakeTransport {
    fn new(text: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            text: text.into(),
            seen: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl DocumentTransport for FakeTransport {
    async fn request(&self, request: CoreRequest) -> Result<CoreResponse, String> {
        self.seen.lock().expect("request log mutex poisoned").push(
            format!("{request:?}")
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_owned(),
        );
        match request {
            CoreRequest::DocumentOpen { .. } => Ok(CoreResponse::DocumentSnapshot {
                snapshot: DocumentSnapshotDto {
                    document_id: "doc-1".to_owned(),
                    project_id: "project-1".to_owned(),
                    workspace_id: "workspace-1".to_owned(),
                    relative_path: "src/lib.rs".to_owned(),
                    revision: 1,
                    text: self.text.clone(),
                    dirty: false,
                    conflicted: false,
                    writer: true,
                    disk_base_digest: "digest-1".to_owned(),
                    scope: DocumentScope::Workspace,
                },
                writer_lease: Some("lease-1".to_owned()),
                lsp_degraded: false,
            }),
            CoreRequest::DocumentChange { .. } => Ok(CoreResponse::DocumentChanged {
                revision: 2,
                lsp_degraded: false,
            }),
            CoreRequest::DocumentStatusGet { .. } => Ok(CoreResponse::DocumentStatus {
                document_id: "doc-1".to_owned(),
                revision: 1,
                dirty: false,
                conflicted: false,
                writer: true,
            }),
            other => Err(format!("unexpected request: {other:?}")),
        }
    }
}

/// A session over a fake transport, with `text` as the document.
async fn session(text: &str) -> DesktopDocumentSession {
    let transport = FakeTransport::new(text);
    let controller = Arc::new(DocumentController::new(transport));
    controller
        .open(
            "project-1".to_owned(),
            "workspace-1".to_owned(),
            "src/lib.rs".to_owned(),
            true,
            DocumentScope::Workspace,
        )
        .await
        .expect("the fake transport answers the open");
    DesktopDocumentSession::new(controller, "src/lib.rs".to_owned())
}

/// A document of `count` numbered lines.
///
/// The last line has no terminator, so the document has exactly `count`
/// lines: a trailing newline would add the empty line every editor shows
/// after it, and the line count would be `count + 1`.
fn numbered_lines(count: usize) -> String {
    (0..count)
        .map(|index| format!("line {index}"))
        .collect::<Vec<String>>()
        .join("\n")
}

#[tokio::test]
async fn the_viewport_serves_a_bounded_window_of_the_controllers_own_snapshot() {
    let session = session(&numbered_lines(500)).await;
    let view = session
        .viewport(Some(100), Some(10))
        .await
        .expect("the viewport is readable");
    assert_eq!(view.first_line, 100);
    assert_eq!(view.line_count, 10);
    // The substrate counts the position after the last line, so a
    // 500-line document has 501 line positions. The host reports the
    // substrate's own count rather than inventing one.
    assert_eq!(view.total_lines, 501);
    assert_eq!(view.visible_lines[0], "line 100");
    assert_eq!(view.visible_lines[9], "line 109");
}

#[tokio::test]
async fn the_viewport_never_serves_more_than_the_bound() {
    let session = session(&numbered_lines(500)).await;
    let view = session
        .viewport(Some(0), Some(100_000))
        .await
        .expect("the viewport is readable");
    assert_eq!(view.line_count, MAX_VIEWPORT_LINES);
    assert!(
        view.line_count < view.total_lines,
        "the whole document must not be served"
    );
}

#[tokio::test]
async fn the_viewport_clamps_to_the_documents_own_lines() {
    let session = session(&numbered_lines(3)).await;
    let view = session
        .viewport(Some(2_000), Some(10))
        .await
        .expect("the viewport is readable");
    assert!(view.first_line < view.total_lines, "the window is clamped");
    assert_eq!(view.line_count, 1);
    assert!(
        view.line_count < view.total_lines,
        "the whole document must not be served"
    );
}

#[tokio::test]
async fn the_slice_never_materialises_the_whole_document() {
    let snapshot = DocumentBuffer::new(numbered_lines(1_000)).snapshot();
    let lines = slice_lines(&snapshot, 0, 5);
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[0], "line 0");
    assert_eq!(snapshot.len_lines(), 1_001);
}

#[tokio::test]
async fn an_edit_is_recorded_with_its_inverse_and_undo_restores_the_bytes() {
    let session = session("hello world\n").await;
    session
        .edit(5, 5, " brave")
        .await
        .expect("the edit applies");
    let view = session
        .viewport(None, None)
        .await
        .expect("the viewport is readable");
    assert_eq!(view.visible_lines[0], "hello brave world");
    assert_eq!(view.caret, 11);

    session.undo().await.expect("the undo applies");
    let view = session
        .viewport(None, None)
        .await
        .expect("the viewport is readable");
    assert_eq!(view.visible_lines[0], "hello world");
    assert_eq!(view.caret, 5);

    session.redo().await.expect("the redo applies");
    let view = session
        .viewport(None, None)
        .await
        .expect("the viewport is readable");
    assert_eq!(view.visible_lines[0], "hello brave world");
}

#[tokio::test]
async fn an_undo_after_the_history_was_cleared_is_refused_rather_than_applied() {
    let session = session("hello world\n").await;
    session.edit(5, 5, "!").await.expect("the edit applies");
    session.forget_history().await;
    let failure = session.undo().await.expect_err("the undo is refused");
    assert!(
        failure.contains("nothing to undo"),
        "unexpected failure: {failure}"
    );
}

#[tokio::test]
async fn the_caret_is_clamped_to_a_utf8_byte_boundary() {
    // "é" is two bytes, so offset 1 is inside the character.
    let session = session("éclair\n").await;
    session
        .move_cursor("right", false)
        .await
        .expect("the caret moves");
    let view = session
        .viewport(None, None)
        .await
        .expect("the viewport is readable");
    assert_eq!(view.caret, 2, "the caret lands on the character's end");
}

#[tokio::test]
async fn a_path_that_is_not_workspace_relative_is_refused() {
    for refused in [
        "",
        "/etc/passwd",
        "../escape",
        "a/../../escape",
        "a//b",
        "a/./b",
        ".",
        "..",
        "a\\b",
    ] {
        assert!(
            safe_relative_path(refused).is_err(),
            "expected {refused:?} to be refused"
        );
    }
    assert_eq!(
        safe_relative_path("src/lib.rs").as_deref(),
        Ok("src/lib.rs")
    );
}

#[test]
fn every_controller_state_has_a_distinct_presentation_key() {
    let states = [
        codegg_client::DocumentState::Closed,
        codegg_client::DocumentState::Opening,
        codegg_client::DocumentState::Synced,
        codegg_client::DocumentState::DirtyLocal,
        codegg_client::DocumentState::Flushing,
        codegg_client::DocumentState::ResyncRequired,
        codegg_client::DocumentState::Conflict,
        codegg_client::DocumentState::ReadOnly,
        codegg_client::DocumentState::Disconnected,
        codegg_client::DocumentState::GoneWithLocalDraft,
    ];
    let mut keys: Vec<&str> = states.iter().map(|state| present_state(state).0).collect();
    keys.sort_unstable();
    let count = keys.len();
    keys.dedup();
    assert_eq!(keys.len(), count, "two states share a presentation key");
    assert!(!present_state(&codegg_client::DocumentState::Synced)
        .1
        .is_empty());
}

#[test]
fn no_warm_server_is_not_an_empty_successful_answer() {
    let not_ready: EditorDiagnosticsView = diagnostics_view(LspDiagnosticsResultDto {
        project_id: "project-1".to_owned(),
        status: LspDiagnosticsStatus::NotReady,
        files: Vec::new(),
        truncated: false,
    });
    assert_eq!(not_ready.status, "no_warm_server");
    assert!(not_ready.diagnostics.is_empty());

    let ready: EditorDiagnosticsView = diagnostics_view(LspDiagnosticsResultDto {
        project_id: "project-1".to_owned(),
        status: LspDiagnosticsStatus::Ready,
        files: Vec::new(),
        truncated: false,
    });
    assert_eq!(ready.status, "ready");
    assert_ne!(ready.status, not_ready.status);
}

#[test]
fn a_ready_diagnostic_set_carries_its_entries_and_its_own_path() {
    let view = diagnostics_view(LspDiagnosticsResultDto {
        project_id: "project-1".to_owned(),
        status: LspDiagnosticsStatus::Ready,
        files: vec![LspFileDiagnosticsDto {
            path: "src/lib.rs".to_owned(),
            sequence: 1,
            digest: "digest".to_owned(),
            diagnostics: vec![LspDiagnosticDto {
                range: LspRangeDto {
                    path: "src/lib.rs".to_owned(),
                    workspace_relative_path: Some("src/lib.rs".to_owned()),
                    start_line: 3,
                    start_column: 5,
                    end_line: 3,
                    end_column: 9,
                },
                severity: 1,
                tag: 0,
                code: None,
                message: "unused".to_owned(),
                source: Some("rust-analyzer".to_owned()),
            }],
            post_restart: false,
            truncated: false,
        }],
        truncated: false,
    });
    assert_eq!(view.status, "ready");
    assert_eq!(view.diagnostics.len(), 1);
    assert_eq!(view.diagnostics[0].relative_path, "src/lib.rs");
    assert_eq!(view.diagnostics[0].line, 3);
    assert_eq!(view.diagnostics[0].server.as_deref(), Some("rust-analyzer"));
}

#[test]
fn a_completion_that_is_not_ready_says_so_and_carries_no_candidates() {
    let view: EditorCompletionView = completion_view(LspCompletionResultDto {
        document_id: "doc-1".to_owned(),
        revision: 4,
        synced_revision: None,
        status: LspCompletionStatus::NotReady,
        candidates: Vec::new(),
        truncated: false,
        incomplete: false,
    });
    assert_eq!(view.status, "no_warm_server");
    assert!(view.candidates.is_empty());
}

#[test]
fn an_inapplicable_completion_keeps_its_reason() {
    let view = completion_view(LspCompletionResultDto {
        document_id: "doc-1".to_owned(),
        revision: 4,
        synced_revision: Some(4),
        status: LspCompletionStatus::Ready,
        candidates: vec![LspCompletionCandidateDto {
            label: "snippet".to_owned(),
            detail: None,
            kind: Some("snippet".to_owned()),
            filter_text: None,
            sort_text: None,
            applicability: LspCompletionApplicabilityDto::NonApplicable {
                reason: codegg_protocol::lsp::LspCompletionUnsupportedReason::Snippet,
            },
        }],
        truncated: false,
        incomplete: false,
    });
    assert_eq!(view.status, "ready");
    assert_eq!(view.candidates.len(), 1);
    assert!(!view.candidates[0].applicable);
    assert!(view.candidates[0].reason.is_some());
}

#[test]
fn a_navigation_read_distinguishes_no_warm_server_from_an_answer() {
    let not_ready: EditorNavigationView = navigation_view(LspReadResultDto {
        operation: LspReadOperation::Definition,
        status: LspReadStatus::NotReady,
        payload: None,
    });
    assert_eq!(not_ready.status, "no_warm_server");
    assert!(not_ready.locations.is_empty());

    let ready = navigation_view(LspReadResultDto {
        operation: LspReadOperation::Definition,
        status: LspReadStatus::Ready,
        payload: Some(LspReadPayloadDto::Locations {
            locations: vec![LspRangeDto {
                path: "src/lib.rs".to_owned(),
                workspace_relative_path: Some("src/lib.rs".to_owned()),
                start_line: 7,
                start_column: 2,
                end_line: 7,
                end_column: 8,
            }],
            truncated: false,
        }),
    });
    assert_eq!(ready.status, "ready");
    assert_eq!(ready.locations.len(), 1);
    assert_eq!(ready.locations[0].relative_path, "src/lib.rs");
    assert_eq!(ready.locations[0].line, 7);
}
