//! C003 qualification: the checkout writer fence at the final native effect.
//!
//! The store's own tests live in `crates/codegg-core/tests/writer_permit.rs`;
//! C002's adapter tests live in `tests/writer_permit_service.rs`. These cover
//! what C003 adds: the fence's *placement* in the writer path, the
//! containment rule, the fail-closed effect classes, and the mutual
//! exclusion two runs on one checkout actually produce.
//!
//! # What these tests do and do not claim
//!
//! They claim that a native writer consults the fence *before* its effect,
//! that a write outside the run's own checkout is refused with no
//! filesystem effect, that an unclassified effect is refused on a shared
//! checkout, and that two runs sharing one checkout never overlap their
//! mutating effects.
//!
//! They do **not** claim that shared mutation is available. Nothing in
//! production places two runs on one checkout yet: the work-order
//! `shared`/`serialized` placement is still refused, and the scheduler still
//! gives every delegated run its own isolated worktree. The shared case here
//! is constructed directly, which is the point — the fence is proved before
//! any product path is allowed to depend on it.

use std::str::FromStr;
use std::sync::Arc;

use codegg::core::writer_fence::DaemonWriterFence;
use codegg::core::writer_permit::{WorktreePermitAuthority, WriterPermitService};
use codegg::tool::writer_fence::{
    CheckoutWriterFence, CheckoutWriterScope, NativeEffectClass, WorkspaceWriterFence,
    WriterFenceError, WriterFenceOutcome, WriterFenceRequest,
};
use codegg_core::agent_run::{
    AgentRunBudget, AgentRunStore, InMemoryAgentRunStore, NewAgentRun, NewAgentTask,
};
use codegg_core::agent_run_group::{
    AgentRunGroupOwner, AgentRunGroupService, NewAgentRunGroup, RunJoinPolicy,
};
use codegg_core::identity::{
    AgentRunGroupId, AgentRunId, AgentTaskId, ProjectId, RepositoryId, WorkspaceId, WorktreeId,
};
use codegg_core::transport_auth::AuthenticatedPrincipal;
use codegg_core::writer_permit::WriterPermitStore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

async fn isolated_pool() -> SqlitePool {
    let name = format!("writer_fence_c003_{}", uuid::Uuid::new_v4().simple());
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

fn principal() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal::local_owner("writer-fence-c003")
}

/// A temporary directory that removes itself when the test ends.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "codegg-c003-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---------------------------------------------------------------------------
// Containment at the final native effect
// ---------------------------------------------------------------------------

/// A write outside the run's own checkout is refused, and nothing lands on
/// disk. This is the hole the `git` tool's model-supplied `workdir` used to
/// open: a writer could reach a checkout the run does not own.
#[tokio::test]
async fn a_write_outside_the_run_checkout_is_refused_before_any_effect() {
    let checkout = TempDir::new("checkout");
    let outside = TempDir::new("outside");
    let target = outside.path().join("stolen.txt");

    let fence = WorkspaceWriterFence::new(checkout.path().to_path_buf());
    let request = WriterFenceRequest {
        effect_class: NativeEffectClass::FileWrite,
        targets: vec![target.clone()],
        idempotency_key: "c003-outside".to_owned(),
        granted_roots: Vec::new(),
        principal: Some(principal()),
    };

    let outcome = fence.begin(request).await;
    assert!(
        matches!(outcome, Err(WriterFenceError::OutsideScope { .. })),
        "a target outside the checkout must be refused: {outcome:?}"
    );
    assert!(
        !target.exists(),
        "the refusal must happen before the filesystem effect"
    );
}

/// A write inside the run's own checkout is allowed.
#[tokio::test]
async fn a_write_inside_the_run_checkout_is_allowed() {
    let checkout = TempDir::new("checkout");
    let target = checkout.path().join("notes.txt");

    let fence = WorkspaceWriterFence::new(checkout.path().to_path_buf());
    let request = WriterFenceRequest {
        effect_class: NativeEffectClass::FileWrite,
        targets: vec![target.clone()],
        idempotency_key: "c003-inside".to_owned(),
        granted_roots: Vec::new(),
        principal: Some(principal()),
    };

    let permit = fence
        .begin(request)
        .await
        .expect("a target inside the checkout is allowed");
    assert!(
        permit.is_none(),
        "an isolated checkout takes no permit: there is no second writer to exclude"
    );
}

/// A daemon-granted scratch root is part of the scope. A scratch write is a
/// legitimate write that is not a checkout write, and the model never names
/// the root — the daemon does.
#[tokio::test]
async fn a_granted_scratch_root_is_inside_the_scope() {
    let checkout = TempDir::new("checkout");
    let scratch = TempDir::new("scratch");
    let target = scratch.path().join("scratch.txt");

    let fence = WorkspaceWriterFence::new(checkout.path().to_path_buf());
    let request = WriterFenceRequest {
        effect_class: NativeEffectClass::FileWrite,
        targets: vec![target.clone()],
        idempotency_key: "c003-scratch".to_owned(),
        granted_roots: vec![scratch.path().to_path_buf()],
        principal: Some(principal()),
    };

    let outcome = fence.begin(request).await;
    assert!(
        outcome.is_ok(),
        "a granted scratch root must be inside the scope: {outcome:?}"
    );
}

/// The fence is consulted at the *final* effect, through the real tool. A
/// `write` whose target is outside the run's checkout fails, and the file is
/// never created.
#[tokio::test]
async fn the_write_tool_refuses_an_out_of_scope_target() {
    use codegg::tool::backend::{ToolBackendKind, ToolExecutionContext};
    use codegg::tool::write::WriteTool;
    use codegg::tool::Tool;

    let checkout = TempDir::new("checkout");
    let outside = TempDir::new("outside");
    let target = outside.path().join("written.txt");

    let mut ctx = ToolExecutionContext::with_backend(ToolBackendKind::Native);
    ctx.writer_fence = Some(Arc::new(WorkspaceWriterFence::new(
        checkout.path().to_path_buf(),
    )));

    let tool = WriteTool::new();
    let input = serde_json::json!({
        "path": target.to_string_lossy(),
        "content": "should never land",
    });

    let outcome = tool.execute(input).await;
    assert!(
        outcome.is_err(),
        "a write outside the run's checkout must be refused"
    );
    assert!(
        !target.exists(),
        "the write tool must not create the file after the fence refused it"
    );
}

// ---------------------------------------------------------------------------
// The shared checkout
// ---------------------------------------------------------------------------

/// The shared fixture: one managed checkout, leased to one run, with a live
/// group that covers a second run on the same checkout.
struct Shared {
    worktrees: Arc<codegg_core::worktree_service::WorktreeService>,
    runs: Arc<dyn AgentRunStore>,
    groups: Arc<AgentRunGroupService>,
    permits: Arc<WriterPermitService>,
    checkout: TempDir,
    worktree_id: WorktreeId,
    owner_run: AgentRunId,
    member_run: AgentRunId,
    group_id: AgentRunGroupId,
}

impl Shared {
    async fn new(label: &str) -> Self {
        let pool = isolated_pool().await;
        let checkout = TempDir::new(label);
        let worktree_id = WorktreeId::new();
        let owner_run = AgentRunId::new();
        let member_run = AgentRunId::new();
        let project_id = ProjectId::new();
        let repository_id = RepositoryId::new();
        let workspace_id = WorkspaceId::new();
        let now = chrono::Utc::now().timestamp_millis();

        sqlx::query(
            "INSERT INTO managed_worktree (worktree_id, project_id, repository_id, workspace_id, \
             repository_root, path, branch, base_commit, managed, state, health, lease_generation, \
             owner_run_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'codegg/worktree/c003', 'abc', 1, 'ready', 'clean', 0, NULL, ?, ?)",
        )
        .bind(worktree_id.as_str())
        .bind(project_id.as_str())
        .bind(repository_id.as_str())
        .bind(workspace_id.as_str())
        .bind(checkout.path().to_string_lossy().to_string())
        .bind(checkout.path().to_string_lossy().to_string())
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert managed worktree");

        let worktrees = codegg_core::worktree_service::WorktreeService::sqlite(
            pool.clone(),
            std::path::PathBuf::from("/tmp/codegg-managed-worktrees"),
            Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
        );
        worktrees
            .acquire_lease(&worktree_id, &owner_run)
            .await
            .expect("acquire the checkout's lease");

        // Two real delegated runs under one turn, so a turn-owned group can
        // cover both of them.
        let runs: Arc<dyn AgentRunStore> = Arc::new(InMemoryAgentRunStore::new());
        let session_id = format!("session-{}", uuid::Uuid::new_v4().simple());
        let turn_id = format!("turn-{}", uuid::Uuid::new_v4().simple());
        for (index, run_id) in [owner_run.clone(), member_run.clone()]
            .into_iter()
            .enumerate()
        {
            let task_id = AgentTaskId::new();
            let task = NewAgentTask {
                task_id,
                parent_task_id: None,
                originating_session_id: session_id.clone(),
                originating_turn_id: Some(turn_id.clone()),
                project_id: project_id.clone(),
                repository_id: Some(repository_id.clone()),
                workspace_id: workspace_id.clone(),
                requested_agent: "build".to_owned(),
                delegation_key: format!("delegation-{index}-{}", uuid::Uuid::new_v4().simple()),
                request_fingerprint: format!("fingerprint-{index}"),
                description: "c003 shared checkout qualification".to_owned(),
            };
            let run = NewAgentRun {
                run_id,
                parent_run_id: None,
                depth: 1,
                workspace_id: workspace_id.clone(),
                agent_name: "build".to_owned(),
                agent_digest: None,
                provider: "test".to_owned(),
                model: "test".to_owned(),
                authority_digest: format!("authority-{index}"),
                budget: AgentRunBudget::default(),
                shared_scratch: Vec::new(),
            };
            runs.create_or_get(task, run)
                .await
                .expect("create the delegated run");
        }

        // Both runs are placed on the same checkout: the lease owner is on
        // it because it holds the lease, the second because the group
        // reservation covers it.
        runs.attach_worktree(&owner_run, worktree_id.clone())
            .await
            .expect("place the lease owner on the checkout");
        runs.attach_worktree(&member_run, worktree_id.clone())
            .await
            .expect("place the second run on the shared checkout");

        let groups = AgentRunGroupService::in_memory(runs.clone());
        let group_id = AgentRunGroupId::new();
        groups
            .create(NewAgentRunGroup {
                group_id: group_id.clone(),
                root_run_id: owner_run.clone(),
                owner_run_id: owner_run.clone(),
                owner: AgentRunGroupOwner::Turn {
                    session_id: session_id.clone(),
                    turn_id: turn_id.clone(),
                },
                member_run_ids: vec![owner_run.clone(), member_run.clone()],
                join_policy: RunJoinPolicy::All,
                cancel_remaining_on_satisfaction: false,
                idempotency_key: format!("group-{}", uuid::Uuid::new_v4().simple()),
            })
            .await
            .expect("create the live group");

        let permits = WriterPermitService::new(
            WriterPermitStore::new(pool.clone()),
            WorktreePermitAuthority::new(worktrees.clone()).with_groups(groups.clone()),
        );

        Self {
            worktrees,
            runs,
            groups,
            permits,
            checkout,
            worktree_id,
            owner_run,
            member_run,
            group_id,
        }
    }

    /// The fence for one run on the shared checkout.
    fn fence(&self, run_id: &AgentRunId) -> Arc<DaemonWriterFence> {
        // The second writer must not wait forever behind the first; a short
        // bound turns "the checkout is busy" into a typed refusal quickly.
        Arc::new(
            DaemonWriterFence::new(
                CheckoutWriterScope {
                    worktree_id: self.worktree_id.as_str().to_owned(),
                    checkout_root: self.checkout.path().to_path_buf(),
                    agent_run_id: Some(run_id.as_str().to_owned()),
                    group_id: None,
                },
                self.permits.clone(),
                self.worktrees.clone(),
                self.runs.clone(),
                Some(self.groups.clone()),
                Some(run_id.clone()),
                Some("attempt-c003".to_owned()),
                None,
            )
            .with_wait_timeout(std::time::Duration::from_millis(250)),
        )
    }

    fn request(&self, key: &str, class: NativeEffectClass) -> WriterFenceRequest {
        WriterFenceRequest {
            effect_class: class,
            targets: vec![self.checkout.path().join("file.txt")],
            idempotency_key: key.to_owned(),
            granted_roots: Vec::new(),
            principal: Some(principal()),
        }
    }
}

/// Two runs on one checkout never overlap their mutating effects: the second
/// writer waits for the first, and takes the checkout only once the first
/// has settled.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_runs_on_one_checkout_never_overlap_their_writes() {
    let shared = Shared::new("shared").await;

    // The lease owner takes the checkout.
    assert!(
        !shared.group_id.as_str().is_empty(),
        "the shared case runs under a real live group"
    );
    let first = shared.fence(&shared.owner_run);
    let first_permit = first
        .begin(shared.request("c003-first", NativeEffectClass::FileWrite))
        .await
        .expect("the lease owner acquires the shared checkout");
    assert!(
        first_permit.is_some(),
        "a shared checkout must take a permit: there is a second writer to exclude"
    );

    // The second run shares the checkout through the live group. It must not
    // be able to write while the first holds it.
    let second = shared.fence(&shared.member_run);
    let second_request = shared.request("c003-second", NativeEffectClass::FileWrite);
    let second_permit = second.begin(second_request).await;
    assert!(
        matches!(second_permit, Err(WriterFenceError::Held)),
        "the second writer must be refused while the checkout is held: {second_permit:?}"
    );

    // The first writer settles; the checkout is free again.
    first_permit
        .expect("held permit")
        .settle(WriterFenceOutcome::Settled)
        .await
        .expect("the first writer settles its permit");

    let third = shared.fence(&shared.member_run);
    let permit = third
        .begin(shared.request("c003-third", NativeEffectClass::FileWrite))
        .await
        .expect("the checkout is free once the first writer settled");
    assert!(permit.is_some(), "the second writer now takes the checkout");
    permit
        .expect("held permit")
        .settle(WriterFenceOutcome::Settled)
        .await
        .expect("the second writer settles its permit");
}

/// A run that is not a member of any live group may not write a checkout it
/// does not hold the lease for. The group is the bounded reservation; without
/// it the lease owner is the only writer.
#[tokio::test]
async fn a_run_outside_the_group_may_not_write_the_shared_checkout() {
    let shared = Shared::new("outsider").await;
    let outsider = AgentRunId::new();

    let fence = shared.fence(&outsider);
    let outcome = fence
        .begin(shared.request("c003-outsider", NativeEffectClass::FileWrite))
        .await;
    assert!(
        outcome.is_err(),
        "a run that does not hold the lease and shares no live group must be refused: {outcome:?}"
    );
}

/// An effect class with no qualified profile is refused on a shared
/// checkout. There is no raw fallback and no best effort: an unclassified
/// effect does not get to write.
#[tokio::test]
async fn an_unclassified_effect_is_refused_on_a_shared_checkout() {
    let shared = Shared::new("unclassified").await;
    let fence = shared.fence(&shared.owner_run);

    let outcome = fence
        .begin(shared.request("c003-unknown", NativeEffectClass::Unknown))
        .await;
    assert!(
        matches!(outcome, Err(WriterFenceError::EffectClassDenied { .. })),
        "an unclassified effect must be refused on a shared checkout: {outcome:?}"
    );

    let outcome = fence
        .begin(shared.request("c003-spawn", NativeEffectClass::ProcessSpawn))
        .await;
    assert!(
        matches!(outcome, Err(WriterFenceError::EffectClassDenied { .. })),
        "a process writer must be refused on a shared checkout: {outcome:?}"
    );
}

/// A shared checkout with no bound actor is refused. Mutual exclusion
/// without an attributed writer is an anonymous writer.
#[tokio::test]
async fn a_shared_write_without_a_bound_actor_is_refused() {
    let shared = Shared::new("anonymous").await;
    let fence = shared.fence(&shared.owner_run);

    let mut request = shared.request("c003-anonymous", NativeEffectClass::FileWrite);
    request.principal = None;
    let outcome = fence.begin(request).await;
    assert!(
        matches!(outcome, Err(WriterFenceError::EffectClassDenied { .. })),
        "a shared write with no actor must be refused: {outcome:?}"
    );
}

/// A write outside the shared checkout is refused before the permit is even
/// taken, so a refused write never holds the checkout.
#[tokio::test]
async fn an_out_of_scope_write_is_refused_before_the_permit_is_taken() {
    let shared = Shared::new("scope").await;
    let outside = TempDir::new("outside-scope");
    let fence = shared.fence(&shared.owner_run);

    let mut request = shared.request("c003-out-of-scope", NativeEffectClass::FileWrite);
    request.targets = vec![outside.path().join("nope.txt")];
    let outcome = fence.begin(request).await;
    assert!(
        matches!(outcome, Err(WriterFenceError::OutsideScope { .. })),
        "an out-of-scope target must be refused: {outcome:?}"
    );

    // The checkout was never taken, so the other writer can still write.
    let other = shared.fence(&shared.member_run);
    let permit = other
        .begin(shared.request("c003-after-refusal", NativeEffectClass::FileWrite))
        .await
        .expect("the refused write never held the checkout");
    assert!(permit.is_some());
    permit
        .expect("held permit")
        .settle(WriterFenceOutcome::Settled)
        .await
        .expect("settle");
}

/// An isolated checkout — one with no live group covering it — takes no
/// permit at all. The default placement pays no durable write.
#[tokio::test]
async fn an_isolated_checkout_takes_no_permit() {
    let pool = isolated_pool().await;
    let checkout = TempDir::new("isolated");
    let worktree_id = WorktreeId::new();
    let run_id = AgentRunId::new();
    let now = chrono::Utc::now().timestamp_millis();

    sqlx::query(
        "INSERT INTO managed_worktree (worktree_id, project_id, repository_id, workspace_id, \
         repository_root, path, branch, base_commit, managed, state, health, lease_generation, \
         owner_run_id, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, 'codegg/worktree/isolated', 'abc', 1, 'ready', 'clean', 0, NULL, ?, ?)",
    )
    .bind(worktree_id.as_str())
    .bind(ProjectId::new().as_str())
    .bind(RepositoryId::new().as_str())
    .bind(WorkspaceId::new().as_str())
    .bind(checkout.path().to_string_lossy().to_string())
    .bind(checkout.path().to_string_lossy().to_string())
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .expect("insert managed worktree");

    let worktrees = codegg_core::worktree_service::WorktreeService::sqlite(
        pool.clone(),
        std::path::PathBuf::from("/tmp/codegg-managed-worktrees"),
        Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
    );
    worktrees
        .acquire_lease(&worktree_id, &run_id)
        .await
        .expect("acquire the lease");

    let runs: Arc<dyn AgentRunStore> = Arc::new(InMemoryAgentRunStore::new());
    let permits = WriterPermitService::new(
        WriterPermitStore::new(pool.clone()),
        WorktreePermitAuthority::new(worktrees.clone()),
    );

    let fence = DaemonWriterFence::new(
        CheckoutWriterScope {
            worktree_id: worktree_id.as_str().to_owned(),
            checkout_root: checkout.path().to_path_buf(),
            agent_run_id: Some(run_id.as_str().to_owned()),
            group_id: None,
        },
        permits,
        worktrees,
        runs,
        None,
        Some(run_id),
        Some("attempt-c003".to_owned()),
        None,
    );

    let request = WriterFenceRequest {
        effect_class: NativeEffectClass::FileWrite,
        targets: vec![checkout.path().join("file.txt")],
        idempotency_key: "c003-isolated".to_owned(),
        granted_roots: Vec::new(),
        principal: Some(principal()),
    };
    let permit = fence
        .begin(request)
        .await
        .expect("an isolated checkout is allowed");
    assert!(
        permit.is_none(),
        "an isolated checkout must not take a permit: no second writer can reach it"
    );
}
