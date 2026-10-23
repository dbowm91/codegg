//! C008 qualification: the mediation gate's *authority* is complete.
//!
//! C004 proved that CodeGG's non-broker checkout writers are mediated. It did
//! not prove that the gate is a real authority, and the 2026-10-23 review
//! found five ways it was not:
//!
//! * the reservation authority resolved the containing checkout through a
//!   **capped, oldest-first list**, so a catalog larger than the cap hid a
//!   newly created — and currently reserved — checkout, and the gate admitted
//!   the writer silently;
//! * the HTTP routes mediated only `if let Some(daemon)`, so a server with no
//!   daemon wrote into a reserved checkout with no gate at all;
//! * the gate resolved the workspace identifier by a **stricter** rule than the
//!   effect, so an identifier the effect could resolve left the gate with no
//!   target and the write went unmediated;
//! * the reservation authority resolved a relative path against the **process
//!   working directory**;
//! * the gate's answer was taken **before the repository lock was held**, so
//!   the check-to-write window was the lock wait.
//!
//! This suite covers the first, third, fourth and fifth. The HTTP half lives
//! in `tests/server_feature/checkout_mediation_authority_c008_http.inc`,
//! because the server module is behind the `server` feature.

use super::common::checkout_reservation::ReservedCheckout;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg::protocol::document::DocumentScope;
use codegg_core::agent_run::{AgentRunStatus, AgentRunTerminalOutcome};
use codegg_core::identity::{RepositoryId, WorktreeId};
use codegg_core::workspace::WorkspaceId;

/// How long the test waits for a spawned write to reach the repository lock.
///
/// The wait is bounded by the write's own progress: the spawned task has to
/// run the pre-dispatch gate and then block on the repository lock the test
/// holds, so the sleep only has to cover the gate's database round-trip. The
/// assertions after it — the task is still in flight, and no refusal row
/// exists yet — are what actually prove the write reached the lock.
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_millis(300);

// ---------------------------------------------------------------------------
// The capped list
// ---------------------------------------------------------------------------

/// A reserved checkout stays refused when the user's catalog is larger than
/// the list cap.
///
/// The catalog is user-scoped: one database holds every project the user ever
/// opened, and an archived row never leaves the non-removed set. So the cap
/// eventually drops the *newest* rows — and a freshly created, currently
/// reserved checkout is one of them. The refusal must still be typed, and it
/// must still be durable.
#[tokio::test]
async fn a_reserved_checkout_is_refused_past_the_list_cap() {
    let fixture = ReservedCheckout::build().await;

    let created_at: i64 =
        sqlx::query_scalar("SELECT created_at FROM managed_worktree WHERE worktree_id = ?")
            .bind(fixture.worktree_id.as_str())
            .fetch_one(&fixture.pool)
            .await
            .expect("read the reserved checkout's creation time");

    // Fill the catalog past the cap with *older* ordinary rows, so the
    // reserved checkout is the newest row in the set.
    let filler_repository = RepositoryId::new();
    let filler_count = codegg_core::worktree_service::MAX_WORKTREE_LIST + 88;
    for index in 0..filler_count {
        let path = format!("/codegg-c008-filler/repo-{index}/wt");
        sqlx::query(
            "INSERT INTO managed_worktree (worktree_id, project_id, repository_id, workspace_id, \
             repository_root, path, branch, base_commit, managed, state, health, lease_generation, \
             owner_run_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'codegg/worktree/filler', 'abc', 1, 'archived', 'clean', 0, \
             NULL, ?, ?)",
        )
        .bind(WorktreeId::new().as_str())
        .bind(fixture.project_id.clone())
        .bind(filler_repository.as_str())
        .bind(fixture.workspace_id.clone())
        .bind(format!("/codegg-c008-filler/repo-{index}"))
        .bind(path)
        .bind(created_at - 1 - index as i64)
        .bind(created_at - 1 - index as i64)
        .execute(&fixture.pool)
        .await
        .expect("seed an ordinary managed worktree row");
    }

    // The capped list is the defect this milestone closes: it returns the
    // oldest rows, so it cannot see the reserved checkout at all.
    let capped: Vec<String> = sqlx::query_scalar(
        "SELECT worktree_id FROM managed_worktree WHERE state <> 'removed' \
         ORDER BY created_at LIMIT ?",
    )
    .bind(codegg_core::worktree_service::MAX_WORKTREE_LIST as i64)
    .fetch_all(&fixture.pool)
    .await
    .expect("read the capped list");
    assert_eq!(
        capped.len(),
        codegg_core::worktree_service::MAX_WORKTREE_LIST
    );
    assert!(
        !capped.contains(&fixture.worktree_id.to_string()),
        "the capped list must be the shape the defect had: it cannot see the newest row"
    );

    // The write is still refused, and still durably.
    let target = fixture.checkout.path().join("past-the-cap.txt");
    let response = fixture
        .call(CoreRequest::DocumentCreate {
            project_id: fixture.project_id.clone(),
            workspace_id: fixture.workspace_id.clone(),
            relative_path: "past-the-cap.txt".to_owned(),
            initial_text: "hello".to_owned(),
            scope: DocumentScope::Workspace,
        })
        .await;

    fixture.assert_conflict(&response);
    assert!(
        !target.exists(),
        "a refused write must not leave the file behind"
    );

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
    assert_eq!(rows[0].0, "checkout_writer_refusal");
}

// ---------------------------------------------------------------------------
// One identifier rule
// ---------------------------------------------------------------------------

/// An identifier the workspace registry cannot resolve is refused, not
/// skipped.
///
/// The gate used to resolve the workspace with a stricter rule than the
/// effect, so an identifier the effect could resolve left the gate with no
/// target: the write went *unmediated*. The gate now refuses instead, and it
/// refuses on both the checkpoint-restore and the LSP-apply path.
#[tokio::test]
async fn an_unresolvable_workspace_identifier_is_refused_not_unmediated() {
    let fixture = ReservedCheckout::build().await;
    let unknown = format!("unknown-workspace-{}", uuid::Uuid::new_v4().simple());

    let restore = fixture
        .call(CoreRequest::EditCheckpointUndoLatest {
            workspace_id: unknown.clone(),
            session_id: fixture.session_id.clone(),
        })
        .await;
    fixture.assert_conflict(&restore);

    let target = fixture.checkout.path().join("unmediated.rs");
    std::fs::write(&target, "fn main() {}\n").expect("seed the file");

    let apply = fixture
        .call(CoreRequest::LspPreviewApply {
            request: codegg::protocol::lsp::LspPreviewApplyRequestDto {
                preview_id: "preview-c008".to_owned(),
                preview_revision: 1,
                preview_digest: "digest".to_owned(),
                kind: "formatting".to_owned(),
                title: "format".to_owned(),
                provenance: "c008".to_owned(),
                workspace_id: unknown,
                session_id: fixture.session_id.clone(),
                turn_id: Some(fixture.turn_id.clone()),
                patches: Vec::new(),
            },
        })
        .await;
    fixture.assert_conflict(&apply);
    assert_eq!(
        std::fs::read_to_string(&target).expect("read the file"),
        "fn main() {}\n",
        "a refused LSP apply must not change the bytes on disk"
    );
}

// ---------------------------------------------------------------------------
// The process working directory
// ---------------------------------------------------------------------------

/// The reservation authority refuses a relative path.
///
/// Resolving a relative path against the process working directory would
/// target an unrelated tree — a user-scoped daemon's working directory is
/// wherever the user launched it. The authority refuses instead.
#[tokio::test]
async fn a_relative_path_is_refused_by_the_reservation_authority() {
    let fixture = ReservedCheckout::build().await;

    let error = fixture
        .daemon
        .checkout_mediator
        .reservation_for_root(std::path::Path::new("relative/checkout"))
        .await
        .expect_err("a relative path must be refused");
    assert!(
        matches!(
            error,
            codegg_core::checkout_reservation::CheckoutReservationError::RelativePath(_)
        ),
        "the refusal must be typed, got {error:?}"
    );
}

// ---------------------------------------------------------------------------
// The lock wait
// ---------------------------------------------------------------------------

/// Free the checkout's lease so the pre-dispatch gate sees an unreserved
/// checkout and the write reaches the repository lock.
///
/// The group stays live; only the lease is released, so the checkout can be
/// reserved again while a writer is queued on the lock.
async fn free_the_checkout(fixture: &ReservedCheckout) {
    let record = fixture
        .daemon
        .worktree_service
        .get(&fixture.worktree_id)
        .await
        .expect("read the managed checkout");
    fixture
        .daemon
        .worktree_service
        .release_lease(
            &fixture.worktree_id,
            &fixture.owner_run,
            record.lease_generation,
        )
        .await
        .expect("release the checkout's lease");
}

/// Hold the repository lock the write's effect handler will contend on.
async fn hold_the_repository_lock(
    fixture: &ReservedCheckout,
) -> (
    codegg_core::workspace_services::WorkspaceServicesLease,
    codegg_core::workspace_services::WorkspaceRepositoryGuard,
    std::path::PathBuf,
) {
    let workspace_id = WorkspaceId::new_unchecked(fixture.workspace_id.clone());
    let services = fixture
        .daemon
        .workspace_services
        .acquire(&workspace_id)
        .await
        .expect("acquire the workspace services");
    let canonical_root = fixture
        .daemon
        .workspaces
        .resolve(&workspace_id)
        .await
        .expect("resolve the workspace")
        .canonical_root
        .clone();
    let guard = services.locks().acquire_repository(&canonical_root).await;
    (services, guard, canonical_root)
}

/// Spawn a document create that will contend for the repository lock.
fn spawn_a_write(
    fixture: &ReservedCheckout,
    relative_path: &str,
) -> tokio::task::JoinHandle<Result<CoreResponse, codegg::error::AppError>> {
    let daemon = fixture.daemon.clone();
    let project_id = fixture.project_id.clone();
    let workspace_id = fixture.workspace_id.clone();
    let relative_path = relative_path.to_owned();
    tokio::spawn(async move {
        let request_id = format!("req-{}", uuid::Uuid::new_v4().simple());
        daemon
            .handle_request(codegg::core::new_request(
                request_id,
                CoreRequest::DocumentCreate {
                    project_id,
                    workspace_id,
                    relative_path,
                    initial_text: "hello".to_owned(),
                    scope: DocumentScope::Workspace,
                },
            ))
            .await
    })
}

/// A write that waits on the repository lock re-checks the reservation.
///
/// The gate's answer is a fast pre-filter; the answer that counts is the one
/// taken with the repository lock held. The window between the gate and the
/// write is the lock wait, so a checkout that becomes reserved while the
/// writer is queued must still refuse the write.
#[tokio::test]
async fn a_write_that_waits_on_the_repository_lock_re_checks_the_reservation() {
    let fixture = ReservedCheckout::build().await;
    free_the_checkout(&fixture).await;
    let (_services, guard, _root) = hold_the_repository_lock(&fixture).await;

    let write = spawn_a_write(&fixture, "waited.txt");
    tokio::time::sleep(LOCK_WAIT).await;

    assert!(
        !write.is_finished(),
        "the write must be queued on the repository lock, not already answered"
    );
    let refusals: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_event WHERE action = 'checkout_writer_refusal'",
    )
    .fetch_all(&fixture.pool)
    .await
    .expect("read the audit trail");
    assert!(
        refusals.is_empty(),
        "the pre-dispatch gate saw an unreserved checkout, so it must not have refused: {refusals:?}"
    );

    // The checkout becomes reserved while the writer is queued.
    fixture
        .daemon
        .worktree_service
        .acquire_lease(&fixture.worktree_id, &fixture.owner_run)
        .await
        .expect("reserve the checkout while the writer waits");

    drop(guard);

    let response = write
        .await
        .expect("the write task completes")
        .expect("the daemon request succeeds at the transport layer");
    fixture.assert_conflict(&response);
    assert!(
        !fixture.checkout.path().join("waited.txt").exists(),
        "a refused write must not leave the file behind"
    );

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT action, metadata_json FROM audit_event WHERE action = 'checkout_writer_refusal'",
    )
    .fetch_all(&fixture.pool)
    .await
    .expect("read the audit trail");
    assert_eq!(
        rows.len(),
        1,
        "the under-lock re-check must record exactly one durable refusal: {rows:?}"
    );
}

/// The re-check is not a permanent refusal: a checkout whose group finishes
/// while the writer waits is written.
///
/// The group reaches a terminal state while the writer is queued on the
/// repository lock. The under-lock re-check sees the terminal group and does
/// not refuse — the write lands.
#[tokio::test]
async fn a_write_proceeds_when_the_group_finishes_while_it_waits() {
    let fixture = ReservedCheckout::build().await;
    free_the_checkout(&fixture).await;
    let (_services, guard, _root) = hold_the_repository_lock(&fixture).await;

    let write = spawn_a_write(&fixture, "waited.txt");
    tokio::time::sleep(LOCK_WAIT).await;
    assert!(
        !write.is_finished(),
        "the write must be queued on the repository lock, not already answered"
    );

    // The group finishes while the writer is queued. The group status is
    // recomputed from its members, so every member has to reach a terminal
    // state.
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

    drop(guard);

    let response = write
        .await
        .expect("the write task completes")
        .expect("the daemon request succeeds at the transport layer");
    assert!(
        matches!(response, CoreResponse::DocumentSnapshot { .. }),
        "a checkout whose group finished must accept the write, got {response:?}"
    );
    assert!(
        fixture.checkout.path().join("waited.txt").exists(),
        "the write must land once the group has finished"
    );
}
