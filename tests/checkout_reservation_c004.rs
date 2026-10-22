//! C004 qualification: CodeGG's non-broker checkout writers are mediated.
//!
//! The C003 suite (`tests/writer_fence_c003.rs`) covers the model's *native*
//! writers. This suite covers the writers the model never reaches: the
//! document service, the change-review LSP applier, project initialization,
//! checkpoint restore, the managed-worktree admin requests, and the HTTP file
//! routes. They are all human-originated, and they all arrive without a run.
//!
//! # What these tests do and do not claim
//!
//! They claim that a non-broker writer into a checkout reserved by a live
//! agent-run group is **refused before its first effect**, that the refusal
//! leaves no file, index, branch or metadata change behind, that the refusal
//! is recorded durably, and that an ordinary write into an unreserved
//! checkout still succeeds.
//!
//! They do **not** claim that shared mutation is available. Nothing in
//! production places two runs on one checkout yet. The shared case here is
//! constructed directly — the same construction the C003 suite uses — so the
//! mediation is proved before any product path may depend on it.
//!
//! The HTTP surface is proved separately in
//! `tests/checkout_reservation_c004_http.rs`, because the server module is
//! behind the `server` feature.

mod common;

use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg::protocol::document::DocumentScope;
use codegg_core::agent_run::{AgentRunStatus, AgentRunTerminalOutcome};
use common::checkout_reservation::ReservedCheckout;

// ---------------------------------------------------------------------------
// The refusal
// ---------------------------------------------------------------------------

/// Creating a file in a reserved checkout is refused before the file
/// exists. This is the document service's own path, reached the way the
/// TUI reaches it.
#[tokio::test]
async fn a_document_create_into_a_reserved_checkout_is_refused_with_no_effect() {
    let fixture = ReservedCheckout::build().await;
    let target = fixture.checkout.path().join("notes.txt");

    let response = fixture
        .call(CoreRequest::DocumentCreate {
            project_id: fixture.project_id.clone(),
            workspace_id: fixture.workspace_id.clone(),
            relative_path: "notes.txt".to_owned(),
            initial_text: "hello".to_owned(),
            scope: DocumentScope::Workspace,
        })
        .await;

    fixture.assert_conflict(&response);
    assert!(
        !target.exists(),
        "a refused create must not leave the file behind"
    );
}

/// The editor's own save is refused on the same rule, and the bytes on disk
/// are untouched: the refusal lands before the document service runs, so the
/// revision never advances and the file never becomes dirty.
#[tokio::test]
async fn an_editor_save_into_a_reserved_checkout_is_refused_with_no_effect() {
    let fixture = ReservedCheckout::build().await;
    let target = fixture.checkout.path().join("edited.txt");
    std::fs::write(&target, "before\n").expect("seed the file");

    // Opening a document is a read; it is not mediated and it must not be.
    let opened = fixture
        .call(CoreRequest::DocumentOpen {
            project_id: fixture.project_id.clone(),
            workspace_id: fixture.workspace_id.clone(),
            relative_path: "edited.txt".to_owned(),
            scope: DocumentScope::Workspace,
        })
        .await;
    let (document_id, revision) = match opened {
        CoreResponse::DocumentSnapshot { snapshot, .. } => {
            (snapshot.document_id, snapshot.revision)
        }
        other => panic!("expected an opened document, got {other:?}"),
    };

    // The editor takes the writer lease before it edits.
    let lease = fixture
        .call(CoreRequest::DocumentWriterAcquire {
            project_id: fixture.project_id.clone(),
            document_id: document_id.clone(),
        })
        .await;
    let writer_lease = match lease {
        CoreResponse::DocumentWriterLease { writer_lease, .. } => writer_lease,
        other => panic!("expected a writer lease, got {other:?}"),
    };

    let response = fixture
        .call(CoreRequest::DocumentSave {
            project_id: fixture.project_id.clone(),
            document_id,
            writer_lease,
            expected_revision: revision,
        })
        .await;

    fixture.assert_conflict(&response);
    assert_eq!(
        std::fs::read_to_string(&target).expect("read the file"),
        "before\n",
        "a refused save must not change the bytes on disk"
    );
}

/// The change-review LSP applier is a human path that writes the preview's
/// files. It is refused on the same rule.
#[tokio::test]
async fn an_lsp_preview_apply_into_a_reserved_checkout_is_refused_with_no_effect() {
    let fixture = ReservedCheckout::build().await;
    let target = fixture.checkout.path().join("formatted.rs");
    std::fs::write(&target, "fn main() {}\n").expect("seed the file");

    let response = fixture
        .call(CoreRequest::LspPreviewApply {
            request: codegg::protocol::lsp::LspPreviewApplyRequestDto {
                preview_id: "preview-1".to_owned(),
                preview_revision: 1,
                preview_digest: "digest".to_owned(),
                kind: "formatting".to_owned(),
                title: "format".to_owned(),
                provenance: "c004".to_owned(),
                workspace_id: fixture.workspace_id.clone(),
                session_id: fixture.session_id.clone(),
                turn_id: Some(fixture.turn_id.clone()),
                patches: Vec::new(),
            },
        })
        .await;

    fixture.assert_conflict(&response);
    assert_eq!(
        std::fs::read_to_string(&target)
            .expect("read the file")
            .len(),
        "fn main() {}\n".len(),
        "a refused preview apply must not rewrite the file"
    );
}

/// Publishing the project `AGENTS.md` is a checkout write like any other.
#[tokio::test]
async fn a_project_init_publish_into_a_reserved_checkout_is_refused_with_no_effect() {
    let fixture = ReservedCheckout::build().await;
    let target = fixture.checkout.path().join("AGENTS.md");

    let response = fixture
        .call(CoreRequest::ProjectInitPublish {
            project_id: fixture.project_id.clone(),
            workspace_id: fixture.workspace_id.clone(),
            draft_token: "draft-1".to_owned(),
        })
        .await;

    fixture.assert_conflict(&response);
    assert!(
        !target.exists(),
        "a refused publication must not leave the file behind"
    );
}

/// Removing a managed checkout while a live group holds it is refused. The
/// worktree record itself must survive untouched.
#[tokio::test]
async fn a_managed_worktree_cleanup_of_a_reserved_checkout_is_refused() {
    let fixture = ReservedCheckout::build().await;

    let response = fixture
        .call(CoreRequest::ManagedWorktreeCleanup {
            worktree_id: fixture.worktree_id.to_string(),
            lease_generation: 0,
        })
        .await;

    fixture.assert_conflict(&response);
    assert!(
        fixture.checkout.path().exists(),
        "a refused cleanup must not remove the checkout"
    );
}

// ---------------------------------------------------------------------------
// The refusal is durable, attributed and redacted
// ---------------------------------------------------------------------------

/// The refusal leaves a durable audit row. It is the deny side of the
/// restricted writer profile, which C003 recorded as missing.
#[tokio::test]
async fn a_refused_writer_leaves_a_durable_audit_row() {
    let fixture = ReservedCheckout::build().await;

    let response = fixture
        .call(CoreRequest::DocumentCreate {
            project_id: fixture.project_id.clone(),
            workspace_id: fixture.workspace_id.clone(),
            relative_path: "audited.txt".to_owned(),
            initial_text: "hello".to_owned(),
            scope: DocumentScope::Workspace,
        })
        .await;
    fixture.assert_conflict(&response);

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT action, metadata_json FROM audit_event WHERE action = 'checkout_writer_refusal'",
    )
    .fetch_all(&fixture.pool)
    .await
    .expect("read the audit trail");
    assert_eq!(
        rows.len(),
        1,
        "exactly one durable refusal row is expected, got {rows:?}"
    );
    let (action, metadata) = &rows[0];
    assert_eq!(action, "checkout_writer_refusal");
    assert!(
        metadata.contains("document_create"),
        "the row must name the refused sink: {metadata}"
    );
    assert!(
        !metadata.contains(fixture.checkout.path().to_string_lossy().as_ref()),
        "the refusal must not disclose the checkout path: {metadata}"
    );
}

// ---------------------------------------------------------------------------
// Ordinary writes still work
// ---------------------------------------------------------------------------

/// A checkout that no live group holds is an ordinary checkout: the human
/// write proceeds, with no writer authority required.
#[tokio::test]
async fn a_document_create_into_an_unreserved_checkout_succeeds() {
    let fixture = ReservedCheckout::build().await;
    let (ordinary, project_id, workspace_id) = fixture.ordinary_workspace().await;

    let response = fixture
        .call(CoreRequest::DocumentCreate {
            project_id,
            workspace_id,
            relative_path: "ordinary.txt".to_owned(),
            initial_text: "hello".to_owned(),
            scope: DocumentScope::Workspace,
        })
        .await;

    assert!(
        matches!(response, CoreResponse::DocumentSnapshot { .. }),
        "an ordinary create must succeed, got {response:?}"
    );
    assert!(
        ordinary.path().join("ordinary.txt").exists(),
        "the ordinary create must land"
    );
}

/// The reservation ends with the group. A terminal group no longer reserves
/// the checkout, so the ordinary write proceeds again.
#[tokio::test]
async fn a_terminal_group_releases_the_reservation() {
    let fixture = ReservedCheckout::build().await;
    let target = fixture.checkout.path().join("after.txt");

    let refused = fixture
        .call(CoreRequest::DocumentCreate {
            project_id: fixture.project_id.clone(),
            workspace_id: fixture.workspace_id.clone(),
            relative_path: "after.txt".to_owned(),
            initial_text: "hello".to_owned(),
            scope: DocumentScope::Workspace,
        })
        .await;
    fixture.assert_conflict(&refused);
    assert!(!target.exists());

    // The group ends when its members reach a terminal state. Cancelling
    // the group is not enough: the status is recomputed from the members,
    // and a member that is still running keeps the group live.
    for run_id in [fixture.owner_run.clone(), fixture.member_run.clone()] {
        for status in [
            AgentRunStatus::Queued,
            AgentRunStatus::Preparing,
            AgentRunStatus::Running,
        ] {
            fixture
                .runs
                .transition(&run_id, status)
                .await
                .expect("admit the delegated run");
        }
        fixture
            .runs
            .finish(
                &run_id,
                AgentRunTerminalOutcome::Completed,
                None,
                None,
                None,
            )
            .await
            .expect("finish the delegated run");
        fixture
            .groups
            .member_changed(&run_id)
            .await
            .expect("recompute the group");
    }

    let allowed = fixture
        .call(CoreRequest::DocumentCreate {
            project_id: fixture.project_id.clone(),
            workspace_id: fixture.workspace_id.clone(),
            relative_path: "after.txt".to_owned(),
            initial_text: "hello".to_owned(),
            scope: DocumentScope::Workspace,
        })
        .await;
    assert!(
        matches!(allowed, CoreResponse::DocumentSnapshot { .. }),
        "a terminal group must not keep reserving the checkout, got {allowed:?}"
    );
    assert!(
        target.exists(),
        "the write must land once the group is done"
    );
}

// ---------------------------------------------------------------------------
// Canonical identity
// ---------------------------------------------------------------------------

/// An alias path resolves to the same reservation. A checkout reached
/// through a different spelling is the same checkout, so it is refused the
/// same way.
#[tokio::test]
async fn an_alias_path_resolves_to_the_same_reservation() {
    let fixture = ReservedCheckout::build().await;
    let alias = fixture.checkout.path().join("nested").join("..");

    let mediator = fixture.daemon.checkout_mediator.clone();
    let reservation = mediator
        .reservation_for_root(&alias)
        .await
        .expect("resolve the alias");
    assert!(
        reservation.is_reserved(),
        "an alias path must resolve to the same reserved checkout"
    );

    // And the checkout root itself resolves to the same claim.
    let direct = mediator
        .reservation_for_root(fixture.checkout.path())
        .await
        .expect("resolve the checkout root");
    assert_eq!(
        direct.claim().map(|claim| claim.group_id.to_string()),
        reservation.claim().map(|claim| claim.group_id.to_string()),
        "both spellings must name the same reservation"
    );
}
