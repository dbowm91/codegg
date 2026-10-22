//! C002 qualification: the daemon-side writer permit adapter.
//!
//! The store's own tests live in `crates/codegg-core/tests/writer_permit.rs`.
//! These cover what the adapter adds: authoritative scope resolution, the
//! bounded per-checkout waiter, the guard's release contract, and the operator
//! surface.
//!
//! # What these tests do not claim
//!
//! No writer is routed through the permit here, and none is anywhere yet. C002
//! builds the authority; C003 and C004 route writers through it. A green suite
//! here means the permit is *correct*, not that shared mutation exists.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use codegg::core::writer_permit::{
    WorktreePermitAuthority, WriterPermitAuthority, WriterPermitAuthorityError,
    WriterPermitRequest, WriterPermitService, WriterPermitWait,
};
use codegg_core::identity::{
    AgentRunId, PrincipalId, ProjectId, RepositoryId, WorkspaceId, WorktreeId,
};
use codegg_core::transport_auth::AuthenticatedPrincipal;
use codegg_core::writer_permit::{
    QuiescenceEvidence, WriterPermitError, WriterPermitOperation, WriterPermitQuarantineReason,
    WriterPermitScope, WriterPermitState, WriterPermitStore,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

async fn isolated_pool() -> SqlitePool {
    let name = format!("writer_permit_svc_{}", uuid::Uuid::new_v4().simple());
    let url = format!("file:{name}?mode=memory&cache=shared");
    let opts = SqliteConnectOptions::from_str(&url)
        .expect("valid sqlite options")
        .create_if_missing(true)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await
        .expect("connect in-memory sqlite");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate");
    pool
}

/// Insert a managed worktree row and lease it to `owner_run`, so the authority
/// sees a real, leased checkout.
async fn leased_worktree(
    pool: &SqlitePool,
    worktree_id: &WorktreeId,
    owner_run: &AgentRunId,
) -> (ProjectId, WorkspaceId, RepositoryId) {
    let project_id = ProjectId::new();
    let workspace_id = WorkspaceId::new();
    let repository_id = RepositoryId::new();
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT INTO managed_worktree (worktree_id, project_id, repository_id, workspace_id, \
         repository_root, path, branch, base_commit, managed, state, health, lease_generation, \
         owner_run_id, created_at, updated_at) \
         VALUES (?, ?, ?, ?, '/tmp/repo', '/tmp/repo/wt', 'codegg/worktree/test', 'abc', 1, \
         'ready', 'clean', 0, NULL, ?, ?)",
    )
    .bind(worktree_id.as_str())
    .bind(project_id.as_str())
    .bind(repository_id.as_str())
    .bind(workspace_id.as_str())
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("insert managed worktree");

    let worktrees = codegg_core::worktree_service::WorktreeService::sqlite(
        pool.clone(),
        std::path::PathBuf::from("/tmp/codegg-managed-worktrees"),
        Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
    );
    worktrees
        .acquire_lease(worktree_id, owner_run)
        .await
        .expect("acquire lease");
    (project_id, workspace_id, repository_id)
}

fn principal() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal::local_owner("writer-permit-test")
}

/// A stub authority that resolves a fixed scope. Used for the adapter's own
/// behaviour — waiting, the guard, the operator surface — where the point is
/// the adapter, not the worktree lookup.
struct StubAuthority {
    scope: WriterPermitScope,
}

#[async_trait::async_trait]
impl WriterPermitAuthority for StubAuthority {
    async fn resolve_scope(
        &self,
        _request: &codegg::core::writer_permit::WriterPermitScopeRequest,
    ) -> Result<WriterPermitScope, WriterPermitAuthorityError> {
        Ok(self.scope.clone())
    }
}

fn stub_scope(worktree_id: &WorktreeId) -> WriterPermitScope {
    WriterPermitScope {
        project_id: ProjectId::new(),
        repository_id: Some(RepositoryId::new()),
        workspace_id: WorkspaceId::new(),
        worktree_id: worktree_id.clone(),
        lease_generation: 1,
        group_id: None,
        principal_id: PrincipalId::new(),
        agent_run_id: Some(AgentRunId::new()),
        attempt_id: Some("attempt-1".to_owned()),
    }
}

fn request(worktree_id: &WorktreeId, key: &str) -> WriterPermitRequest {
    WriterPermitRequest {
        worktree_id: worktree_id.clone(),
        principal: principal(),
        agent_run_id: None,
        group_id: None,
        attempt_id: Some("attempt-1".to_owned()),
        operation: WriterPermitOperation::NativeFileWrite,
        idempotency_key: key.to_owned(),
        wait: None,
    }
}

// ---------------------------------------------------------------------------
// Authority
// ---------------------------------------------------------------------------

/// The run that holds the checkout's lease may write to it.
#[tokio::test]
async fn the_lease_owner_may_acquire() {
    let pool = isolated_pool().await;
    let worktree_id = WorktreeId::new();
    let owner_run = AgentRunId::new();
    leased_worktree(&pool, &worktree_id, &owner_run).await;

    let worktrees = codegg_core::worktree_service::WorktreeService::sqlite(
        pool.clone(),
        std::path::PathBuf::from("/tmp/codegg-managed-worktrees"),
        Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
    );
    let service = WriterPermitService::new(
        WriterPermitStore::new(pool.clone()),
        WorktreePermitAuthority::new(worktrees),
    );

    let mut req = request(&worktree_id, "key-1");
    req.agent_run_id = Some(owner_run);
    let guard = service
        .acquire(req)
        .await
        .expect("the lease owner acquires");
    assert_eq!(guard.record().state, WriterPermitState::Held);
}

/// A run that does not hold the lease is refused. This is the cross-tenant
/// denial: a run in another project cannot own this checkout's lease, so it
/// never reaches the store.
#[tokio::test]
async fn a_run_without_the_lease_is_refused() {
    let pool = isolated_pool().await;
    let worktree_id = WorktreeId::new();
    let owner_run = AgentRunId::new();
    leased_worktree(&pool, &worktree_id, &owner_run).await;

    let worktrees = codegg_core::worktree_service::WorktreeService::sqlite(
        pool.clone(),
        std::path::PathBuf::from("/tmp/codegg-managed-worktrees"),
        Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
    );
    let service = WriterPermitService::new(
        WriterPermitStore::new(pool.clone()),
        WorktreePermitAuthority::new(worktrees),
    );

    let mut req = request(&worktree_id, "key-1");
    req.agent_run_id = Some(AgentRunId::new());
    let outcome = service.acquire(req).await;
    assert!(
        matches!(outcome, Err(WriterPermitError::NotOwner)),
        "a run that does not hold the lease must be refused: {outcome:?}"
    );

    // And a request with no run at all is refused too.
    let outcome = service.acquire(request(&worktree_id, "key-2")).await;
    assert!(matches!(outcome, Err(WriterPermitError::NotOwner)));
}

// ---------------------------------------------------------------------------
// The waiter
// ---------------------------------------------------------------------------

/// A waiter waits for a busy permit and takes it when it is released.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_waiter_takes_the_permit_when_it_is_released() {
    let pool = isolated_pool().await;
    let worktree_id = WorktreeId::new();
    let service = WriterPermitService::new(
        WriterPermitStore::new(pool.clone()),
        Arc::new(StubAuthority {
            scope: stub_scope(&worktree_id),
        }),
    );

    let holder = service
        .acquire(request(&worktree_id, "key-holder"))
        .await
        .expect("the first caller holds the permit");

    let waiter_service = service.clone();
    let waiter_worktree = worktree_id.clone();
    let waiter = tokio::spawn(async move {
        let mut req = request(&waiter_worktree, "key-waiter");
        req.wait = Some(WriterPermitWait {
            timeout: Duration::from_secs(10),
            cancellation: CancellationToken::new(),
        });
        waiter_service.acquire(req).await
    });

    // The waiter is parked, and it holds nothing while it waits.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let record = service
        .store()
        .load(&worktree_id)
        .await
        .expect("load")
        .expect("row");
    assert_eq!(record.state, WriterPermitState::Held);
    assert_eq!(
        record.idempotency_key.as_deref(),
        Some("key-holder"),
        "a waiter must not take the permit while it waits"
    );

    holder
        .release(QuiescenceEvidence::Settled)
        .await
        .expect("release");

    let acquired = waiter.await.expect("joins").expect("the waiter acquires");
    assert_eq!(acquired.record().state, WriterPermitState::Held);
    assert_eq!(
        acquired.record().idempotency_key.as_deref(),
        Some("key-waiter")
    );
}

/// A wait that is cancelled gives up without taking the permit.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cancelled_wait_gives_up_without_taking_the_permit() {
    let pool = isolated_pool().await;
    let worktree_id = WorktreeId::new();
    let service = WriterPermitService::new(
        WriterPermitStore::new(pool.clone()),
        Arc::new(StubAuthority {
            scope: stub_scope(&worktree_id),
        }),
    );

    let _holder = service
        .acquire(request(&worktree_id, "key-holder"))
        .await
        .expect("the first caller holds the permit");

    let cancellation = CancellationToken::new();
    let waiter_service = service.clone();
    let waiter_worktree = worktree_id.clone();
    let waiter_cancellation = cancellation.clone();
    let waiter = tokio::spawn(async move {
        let mut req = request(&waiter_worktree, "key-waiter");
        req.wait = Some(WriterPermitWait {
            timeout: Duration::from_secs(30),
            cancellation: waiter_cancellation,
        });
        waiter_service.acquire(req).await
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    cancellation.cancel();

    let outcome = waiter.await.expect("joins");
    assert!(matches!(outcome, Err(WriterPermitError::Cancelled)));

    // The holder still holds it: a cancelled wait leaks nothing.
    let record = service
        .store()
        .load(&worktree_id)
        .await
        .expect("load")
        .expect("row");
    assert_eq!(record.state, WriterPermitState::Held);
    assert_eq!(record.idempotency_key.as_deref(), Some("key-holder"));
}

/// A wait that runs out of time gives up without taking the permit.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_timed_out_wait_gives_up_without_taking_the_permit() {
    let pool = isolated_pool().await;
    let worktree_id = WorktreeId::new();
    let service = WriterPermitService::new(
        WriterPermitStore::new(pool.clone()),
        Arc::new(StubAuthority {
            scope: stub_scope(&worktree_id),
        }),
    );

    let _holder = service
        .acquire(request(&worktree_id, "key-holder"))
        .await
        .expect("the first caller holds the permit");

    let mut req = request(&worktree_id, "key-waiter");
    req.wait = Some(WriterPermitWait {
        timeout: Duration::from_millis(150),
        cancellation: CancellationToken::new(),
    });
    let outcome = service.acquire(req).await;
    assert!(matches!(outcome, Err(WriterPermitError::WaitTimeout)));

    let record = service
        .store()
        .load(&worktree_id)
        .await
        .expect("load")
        .expect("row");
    assert_eq!(record.state, WriterPermitState::Held);
    assert_eq!(record.idempotency_key.as_deref(), Some("key-holder"));
}

/// Two checkouts do not wait on each other: a busy first checkout must not
/// delay a second one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn distinct_checkouts_do_not_wait_on_each_other() {
    let pool = isolated_pool().await;
    let first = WorktreeId::new();
    let second = WorktreeId::new();
    let service = WriterPermitService::new(
        WriterPermitStore::new(pool.clone()),
        Arc::new(StubAuthority {
            scope: stub_scope(&first),
        }),
    );

    let _holder = service
        .acquire(request(&first, "key-holder"))
        .await
        .expect("the first caller holds the permit");

    // The stub authority resolves to the first checkout's scope, so point the
    // second request at a different scope by using a second service.
    let second_service = WriterPermitService::new(
        WriterPermitStore::new(pool.clone()),
        Arc::new(StubAuthority {
            scope: stub_scope(&second),
        }),
    );
    let mut req = request(&second, "key-second");
    req.wait = Some(WriterPermitWait {
        timeout: Duration::from_millis(500),
        cancellation: CancellationToken::new(),
    });
    let guard = second_service
        .acquire(req)
        .await
        .expect("the second checkout is not blocked by the first");
    assert_eq!(guard.record().worktree_id, second);
}

// ---------------------------------------------------------------------------
// The guard
// ---------------------------------------------------------------------------

/// A guard that is dropped without being released quarantines the permit. It
/// never frees it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dropped_guard_quarantines_rather_than_frees() {
    let pool = isolated_pool().await;
    let worktree_id = WorktreeId::new();
    let service = WriterPermitService::new(
        WriterPermitStore::new(pool.clone()),
        Arc::new(StubAuthority {
            scope: stub_scope(&worktree_id),
        }),
    );

    {
        let _guard = service
            .acquire(request(&worktree_id, "key-1"))
            .await
            .expect("acquire");
        // Dropped here, unsettled.
    }

    // The Drop-spawned quarantine is asynchronous; give it a moment.
    let mut state = WriterPermitState::Held;
    for _ in 0..50 {
        let record = service
            .store()
            .load(&worktree_id)
            .await
            .expect("load")
            .expect("row");
        state = record.state;
        if state == WriterPermitState::Quarantined {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        state,
        WriterPermitState::Quarantined,
        "a dropped guard must park the permit, not free it"
    );
}

/// Unproven quiescence parks the permit.
#[tokio::test]
async fn unproven_quiescence_parks_the_permit() {
    let pool = isolated_pool().await;
    let worktree_id = WorktreeId::new();
    let service = WriterPermitService::new(
        WriterPermitStore::new(pool.clone()),
        Arc::new(StubAuthority {
            scope: stub_scope(&worktree_id),
        }),
    );

    let guard = service
        .acquire(request(&worktree_id, "key-1"))
        .await
        .expect("acquire");
    let released = guard
        .release(QuiescenceEvidence::Unproven(
            WriterPermitQuarantineReason::ProcessTreeUncertain,
        ))
        .await
        .expect("release parks the permit");
    assert_eq!(released.state, WriterPermitState::Quarantined);
}

// ---------------------------------------------------------------------------
// Operator surface
// ---------------------------------------------------------------------------

/// The operator can see a parked permit and free it, and the view carries no
/// free text.
#[tokio::test]
async fn the_operator_can_inspect_and_release_a_parked_permit() {
    let pool = isolated_pool().await;
    let worktree_id = WorktreeId::new();
    let service = WriterPermitService::new(
        WriterPermitStore::new(pool.clone()),
        Arc::new(StubAuthority {
            scope: stub_scope(&worktree_id),
        }),
    );

    let guard = service
        .acquire(request(&worktree_id, "key-1"))
        .await
        .expect("acquire");
    let quarantined = guard
        .quarantine(
            WriterPermitQuarantineReason::OperatorHold,
            "operator parked this checkout".to_owned(),
        )
        .await
        .expect("quarantine");

    let view = service
        .inspect(&worktree_id)
        .await
        .expect("inspect")
        .expect("a view");
    assert_eq!(view.state, "quarantined");
    assert_eq!(view.quarantine_reason.as_deref(), Some("operator_hold"));

    let released = service
        .operator_release(
            &worktree_id,
            quarantined.revision,
            "the checkout was inspected".to_owned(),
            QuiescenceEvidence::Settled,
        )
        .await
        .expect("operator release");
    assert_eq!(released.state, "available");
}

/// The operator's release needs proven quiescence, like every other release.
#[tokio::test]
async fn the_operator_release_needs_proven_quiescence() {
    let pool = isolated_pool().await;
    let worktree_id = WorktreeId::new();
    let service = WriterPermitService::new(
        WriterPermitStore::new(pool.clone()),
        Arc::new(StubAuthority {
            scope: stub_scope(&worktree_id),
        }),
    );

    let guard = service
        .acquire(request(&worktree_id, "key-1"))
        .await
        .expect("acquire");
    let quarantined = guard
        .quarantine(
            WriterPermitQuarantineReason::OperatorHold,
            "operator parked this checkout".to_owned(),
        )
        .await
        .expect("quarantine");

    let outcome = service
        .operator_release(
            &worktree_id,
            quarantined.revision,
            "the checkout was inspected".to_owned(),
            QuiescenceEvidence::Unproven(WriterPermitQuarantineReason::IncompleteEffects),
        )
        .await;
    assert!(matches!(
        outcome,
        Err(WriterPermitError::UnprovenQuiescence(_))
    ));
}

// ---------------------------------------------------------------------------
// Restart
// ---------------------------------------------------------------------------

/// A permit left live by a previous daemon is quarantined at startup.
#[tokio::test]
async fn startup_reconciliation_parks_a_permit_left_live() {
    let pool = isolated_pool().await;
    let worktree_id = WorktreeId::new();
    let service = WriterPermitService::new(
        WriterPermitStore::new(pool.clone()),
        Arc::new(StubAuthority {
            scope: stub_scope(&worktree_id),
        }),
    );

    let _guard = service
        .acquire(request(&worktree_id, "key-1"))
        .await
        .expect("acquire");

    let reconciled = service.reconcile_after_restart().await.expect("reconcile");
    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0].state, "quarantined");
}

// ---------------------------------------------------------------------------
// Cross-principal denial
// ---------------------------------------------------------------------------

/// A principal that is not a member of the worktree's project is refused,
/// even when it is the run that holds the checkout's lease.
///
/// The lease says *which run* may write; the team policy says *which principal*
/// may. Both are required.
#[tokio::test]
async fn a_principal_outside_the_project_is_refused() {
    let pool = isolated_pool().await;
    let worktree_id = WorktreeId::new();
    let owner_run = AgentRunId::new();
    let (project_id, _workspace_id, _repository_id) =
        leased_worktree(&pool, &worktree_id, &owner_run).await;

    let team = codegg_core::team::TeamStore::new(pool.clone());
    // A principal that exists, is active, and is a *viewer* in the worktree's
    // project. A viewer does not hold `file.modify`.
    let principal_record = team
        .create_principal(codegg_core::team::PrincipalKind::Human, "Viewer Person")
        .await
        .expect("create principal");
    team.create_membership(
        &project_id,
        &principal_record.id,
        codegg_core::team::ProjectRole::Viewer,
    )
    .await
    .expect("create membership");
    // A principal with no membership at all.
    let outsider = team
        .create_principal(codegg_core::team::PrincipalKind::Human, "Outsider")
        .await
        .expect("create outsider");

    let worktrees = codegg_core::worktree_service::WorktreeService::sqlite(
        pool.clone(),
        std::path::PathBuf::from("/tmp/codegg-managed-worktrees"),
        Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
    );
    let service = WriterPermitService::new(
        WriterPermitStore::new(pool.clone()),
        WorktreePermitAuthority::new(worktrees).with_authorization(Arc::new(
            codegg_core::authorization::AuthorizationService::new(team),
        )),
    );

    let viewer = AuthenticatedPrincipal::personal_token(&principal_record, "viewer-client");
    let mut req = request(&worktree_id, "key-viewer");
    req.principal = viewer;
    req.agent_run_id = Some(owner_run.clone());
    let outcome = service.acquire(req).await;
    assert!(
        matches!(outcome, Err(WriterPermitError::ScopeMismatch)),
        "a principal without `file.modify` in the project must be refused: {outcome:?}"
    );

    let outsider_principal = AuthenticatedPrincipal::personal_token(&outsider, "outsider-client");
    let mut req = request(&worktree_id, "key-outsider");
    req.principal = outsider_principal;
    req.agent_run_id = Some(owner_run);
    let outcome = service.acquire(req).await;
    assert!(
        matches!(outcome, Err(WriterPermitError::ScopeMismatch)),
        "a principal with no membership in the project must be refused: {outcome:?}"
    );
}
