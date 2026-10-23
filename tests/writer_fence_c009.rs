//! M005 corrective C009 — the agent-loop writer fence, qualified.
//!
//! C009 closed three ways a write could reach a shared checkout without the
//! authority the fence exists to enforce:
//!
//! * the `commit` tool was registered without the fail-closed `unrooted(...)`
//!   wrapper every other workspace tool takes, so with no resolved workspace
//!   root it kept the sentinel workdir `\0codegg:no-workspace-root` — a path
//!   that resolves *under* the checkout root, which passes the fence's
//!   containment check and then parks the checkout in quarantine;
//! * the daemon asserted its local-principal fallback as a literal `true`, so
//!   the "a daemon with a team policy refuses an unattributed shared-checkout
//!   write" rule was unreachable;
//! * the `task` tool's in-process nested dispatch dropped the fence, and the
//!   worker substituted a containment-only fence that can never take the
//!   writer permit.
//!
//! What this suite proves:
//!
//! * a registry built with no workspace root refuses **every** mutating
//!   tool, `commit` included, with the typed missing-root error;
//! * a daemon with a team policy refuses an unattributed shared-checkout
//!   write instead of writing it anonymously;
//! * a daemon with no team policy still attributes its own writes — the
//!   single-user path must not regress into refusing itself;
//! * two writers on one shared checkout produce **distinguishable** permit
//!   rows, so the permit can tell them apart.
//!
//! What this suite does **not** claim:
//!
//! * It does not claim that a shared checkout is available to a team daemon.
//!   The refusal above is the point: an unattributed write is refused, not
//!   attributed to a synthetic owner.
//! * It does not claim the nested dispatch's fence is carried end to end. The
//!   static guard `scripts/check_writer_fence_completeness.py` owns that
//!   wiring; this suite owns the authority it is wired into.

use std::sync::Arc;

use codegg::core::writer_fence::DaemonWriterFenceFactory;
use codegg::core::writer_permit::{WorktreePermitAuthority, WriterPermitService};
use codegg::tool::writer_fence::{
    CheckoutWriterFence, NativeEffectClass, WriterFenceError, WriterFenceFactory,
    WriterFenceOutcome, WriterFenceRequest, WriterScopeRequest,
};
use codegg::tool::{Tool, ToolRegistry, ToolRegistryOptions};
use codegg_core::agent_run::{
    AgentRunBudget, AgentRunStore, InMemoryAgentRunStore, NewAgentRun, NewAgentTask,
};
use codegg_core::agent_run_group::{
    AgentRunGroupOwner, AgentRunGroupService, NewAgentRunGroup, RunJoinPolicy,
};
use codegg_core::identity::{
    AgentRunGroupId, AgentRunId, AgentTaskId, PrincipalId, ProjectId, RepositoryId, WorkspaceId,
    WorktreeId,
};
use codegg_core::team::{PrincipalKind, TeamStore};
use codegg_core::transport_auth::{AuthMethod, AuthenticatedPrincipal, TransportClass};
use codegg_core::writer_permit::WriterPermitStore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

/// An in-memory pool with the full session schema applied.
async fn isolated_pool() -> SqlitePool {
    let name = format!("writer_fence_c009_{}", uuid::Uuid::new_v4().simple());
    let url = format!("file:{name}?mode=memory&cache=shared");
    let opts = url
        .parse::<SqliteConnectOptions>()
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

/// A temporary directory that removes itself when the test ends.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "codegg-c009-{label}-{}",
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

/// A named principal, so two writers on one checkout are distinguishable.
fn named_principal(id: &str) -> AuthenticatedPrincipal {
    AuthenticatedPrincipal::reconstructed(
        PrincipalId::parse(id).expect("a bounded principal identity"),
        PrincipalKind::Human,
        AuthMethod::PersonalToken,
        TransportClass::AuthenticatedRemote,
        format!("client-{id}"),
    )
}

// ---------------------------------------------------------------------------
// The registry refuses every mutating tool without a workspace root
// ---------------------------------------------------------------------------

/// A registry with no resolved workspace root refuses every mutating tool.
///
/// `commit` used to be the exception: it kept the sentinel workdir
/// `\0codegg:no-workspace-root`, which resolves *under* the checkout root, so
/// the fence's containment check passed and the tool took the checkout's
/// writer permit before failing with a raw Git error and parking the checkout
/// in quarantine. A missing root is now a typed refusal for every mutating
/// tool, and the tool still carries its own name so the model gets an
/// actionable error instead of a missing tool.
#[tokio::test]
async fn a_registry_without_a_workspace_root_refuses_every_mutating_tool() {
    let registry = ToolRegistry::with_options(ToolRegistryOptions {
        workspace_root: None,
        ..Default::default()
    });

    for name in [
        "write",
        "edit",
        "replace",
        "apply_patch",
        "git",
        "commit",
        "terminal",
    ] {
        let tool: &dyn Tool = registry
            .get(name)
            .unwrap_or_else(|| panic!("the '{name}' tool must still be registered"));
        assert_eq!(tool.name(), name, "the wrapper must keep the tool's name");

        let outcome = tool.execute(serde_json::json!({})).await;
        let error = match outcome {
            Ok(output) => panic!("'{name}' must refuse without a workspace root: {output}"),
            Err(error) => error,
        };
        assert!(
            matches!(error, codegg::error::ToolError::MissingWorkspaceRoot { .. }),
            "'{name}' must refuse with the typed missing-root error, got: {error:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// The shared checkout fixture
// ---------------------------------------------------------------------------

/// One managed checkout, leased to one run, with a live group that covers a
/// second run on the same checkout. Copied from the C003 qualification
/// fixture: the shape is what makes the checkout *shared*, and a shared
/// checkout is where the writer permit and the principal both matter.
struct Shared {
    pool: SqlitePool,
    worktrees: Arc<codegg_core::worktree_service::WorktreeService>,
    runs: Arc<dyn AgentRunStore>,
    groups: Arc<AgentRunGroupService>,
    permits: Arc<WriterPermitService>,
    checkout: TempDir,
    worktree_id: WorktreeId,
    owner_run: AgentRunId,
    member_run: AgentRunId,
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
             VALUES (?, ?, ?, ?, ?, ?, 'codegg/worktree/c009', 'abc', 1, 'ready', 'clean', 0, NULL, ?, ?)",
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
                description: "c009 shared checkout qualification".to_owned(),
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
            pool,
            worktrees,
            runs,
            groups,
            permits,
            checkout,
            worktree_id,
            owner_run,
            member_run,
        }
    }

    /// The factory a daemon would build for this checkout, bound to the
    /// given durable team state.
    fn factory(&self, team_store: Option<Arc<TeamStore>>) -> Arc<dyn WriterFenceFactory> {
        DaemonWriterFenceFactory::with_team_state(
            self.permits.clone(),
            self.worktrees.clone(),
            self.runs.clone(),
            Some(self.groups.clone()),
            team_store,
        )
    }

    /// The fence the factory resolves for one run on this checkout.
    async fn fence(
        &self,
        factory: &Arc<dyn WriterFenceFactory>,
        run_id: &AgentRunId,
    ) -> Arc<dyn CheckoutWriterFence> {
        factory
            .fence_for(WriterScopeRequest {
                agent_run_id: Some(run_id.as_str().to_owned()),
                workspace_root: self.checkout.path().to_path_buf(),
                attempt_id: Some("attempt-c009".to_owned()),
                cancellation: None,
            })
            .await
            .expect("resolve the run's fence")
            .expect("a run on a managed checkout has a fence")
    }

    fn request(&self, key: &str, principal: Option<AuthenticatedPrincipal>) -> WriterFenceRequest {
        WriterFenceRequest {
            effect_class: NativeEffectClass::FileWrite,
            targets: vec![self.checkout.path().join("file.txt")],
            idempotency_key: key.to_owned(),
            granted_roots: Vec::new(),
            principal,
        }
    }
}

/// A daemon with a team policy refuses an unattributed shared-checkout write.
///
/// The daemon used to assert its local-principal fallback as a literal
/// `true`, so this refusal was dead code and every shared write was
/// attributed to the synthetic `codegg-writer-fence` owner. The fallback is
/// now derived from the daemon's durable team state, and a daemon whose team
/// store names anyone other than the local owner has a team policy.
#[tokio::test]
async fn a_team_daemon_refuses_an_unattributed_shared_checkout_write() {
    let shared = Shared::new("team").await;
    let team_store = Arc::new(TeamStore::new(shared.pool.clone()));
    team_store
        .ensure_local_owner()
        .await
        .expect("bootstrap the local owner");
    team_store
        .create_principal(PrincipalKind::Human, "team member")
        .await
        .expect("add a team member");

    let factory = shared.factory(Some(team_store));
    let fence = shared.fence(&factory, &shared.owner_run).await;

    let outcome = fence.begin(shared.request("c009-unattributed", None)).await;
    match outcome {
        Err(WriterFenceError::EffectClassDenied { reason, .. }) => {
            assert!(
                reason.contains("no bound actor"),
                "the refusal must name the missing actor: {reason}"
            );
        }
        other => panic!("a team daemon must refuse an unattributed write: {other:?}"),
    }
}

/// A daemon with no team policy still attributes its own writes.
///
/// This is the non-regression: removing the fallback outright would refuse a
/// single-user daemon's own writes, which is a regression and not a fix. A
/// daemon whose team store names only the local owner keeps the fallback, and
/// its unattributed write is attributed and permitted.
#[tokio::test]
async fn a_single_user_daemon_still_attributes_its_own_writes() {
    let shared = Shared::new("single-user").await;
    let team_store = Arc::new(TeamStore::new(shared.pool.clone()));
    team_store
        .ensure_local_owner()
        .await
        .expect("bootstrap the local owner");

    let factory = shared.factory(Some(team_store));
    let fence = shared.fence(&factory, &shared.owner_run).await;

    let permit = fence
        .begin(shared.request("c009-single-user", None))
        .await
        .expect("a single-user daemon's own write is not refused");
    assert!(
        permit.is_some(),
        "a shared checkout takes the writer permit even for the local owner"
    );
    permit
        .expect("held permit")
        .settle(WriterFenceOutcome::Settled)
        .await
        .expect("the writer settles its permit");
}

/// Two writers on one shared checkout produce distinguishable permit rows.
///
/// The permit row records the principal that owns it, so a checkout's
/// history can say *who* wrote. Two named principals must land in two
/// different rows — with the synthetic fallback in place, every write would
/// have carried the same `codegg-writer-fence` owner and the row could not
/// tell the writers apart.
#[tokio::test]
async fn two_writers_on_one_shared_checkout_have_distinguishable_principals() {
    let shared = Shared::new("distinguishable").await;
    let factory = shared.factory(None);

    let first_principal = named_principal("writer-a");
    let second_principal = named_principal("writer-b");

    let first_fence = shared.fence(&factory, &shared.owner_run).await;
    let first_permit = first_fence
        .begin(shared.request("c009-writer-a", Some(first_principal.clone())))
        .await
        .expect("the first writer takes the shared checkout");
    assert!(first_permit.is_some(), "a shared checkout takes a permit");

    let store = WriterPermitStore::new(shared.pool.clone());
    let first_row = store
        .load(&shared.worktree_id)
        .await
        .expect("read the permit row")
        .expect("the shared checkout has a permit row");
    let first_owner = first_row
        .owner
        .as_ref()
        .expect("the permit row names its owner")
        .principal_id
        .as_str()
        .to_owned();

    first_permit
        .expect("held permit")
        .settle(WriterFenceOutcome::Settled)
        .await
        .expect("the first writer settles its permit");

    let second_fence = shared.fence(&factory, &shared.member_run).await;
    let second_permit = second_fence
        .begin(shared.request("c009-writer-b", Some(second_principal.clone())))
        .await
        .expect("the second writer takes the checkout once it is free");
    assert!(second_permit.is_some(), "a shared checkout takes a permit");

    let second_row = store
        .load(&shared.worktree_id)
        .await
        .expect("read the permit row")
        .expect("the shared checkout has a permit row");
    let second_owner = second_row
        .owner
        .as_ref()
        .expect("the permit row names its owner")
        .principal_id
        .as_str()
        .to_owned();

    second_permit
        .expect("held permit")
        .settle(WriterFenceOutcome::Settled)
        .await
        .expect("the second writer settles its permit");

    assert_ne!(
        first_owner, second_owner,
        "two writers must be distinguishable in the permit row"
    );
    assert_eq!(
        first_owner,
        first_principal.principal_id().as_str(),
        "the permit row must name the principal that actually wrote"
    );
    assert_eq!(
        second_owner,
        second_principal.principal_id().as_str(),
        "the permit row must name the principal that actually wrote"
    );
}

/// M005 corrective C009 (implementation review): an unreadable run store is
/// an **unresolved** lease, not an absent one.
///
/// The fence factory used to discard the run-store error (`.ok().flatten()`),
/// so a store failure produced "this run holds no checkout" and the caller
/// received **no fence at all**. The write then proceeded with no mediation:
/// the exact anonymous-writer hole the milestone exists to close.
#[tokio::test]
async fn the_fence_factory_refuses_when_the_run_store_cannot_be_read() {
    let pool = isolated_pool().await;
    let runs: Arc<dyn AgentRunStore> = Arc::new(codegg_core::agent_run::SqliteAgentRunStore::new(
        pool.clone(),
    ));

    // A real run, in a real store, so the read succeeds until the pool closes.
    let run_id = AgentRunId::new();
    let workspace_id = WorkspaceId::new();
    runs.create_or_get(
        NewAgentTask {
            task_id: AgentTaskId::new(),
            parent_task_id: None,
            originating_session_id: "session-unreadable".to_owned(),
            originating_turn_id: Some("turn-unreadable".to_owned()),
            project_id: ProjectId::new(),
            repository_id: Some(RepositoryId::new()),
            workspace_id: workspace_id.clone(),
            requested_agent: "build".to_owned(),
            delegation_key: "delegation-unreadable".to_owned(),
            request_fingerprint: "fingerprint-unreadable".to_owned(),
            description: "c009 unreadable run store".to_owned(),
        },
        NewAgentRun {
            run_id: run_id.clone(),
            parent_run_id: None,
            depth: 1,
            workspace_id,
            agent_name: "build".to_owned(),
            agent_digest: None,
            provider: "test".to_owned(),
            model: "test".to_owned(),
            authority_digest: "authority-unreadable".to_owned(),
            budget: AgentRunBudget::default(),
            shared_scratch: Vec::new(),
        },
    )
    .await
    .expect("create the delegated run");

    // The store is now unreadable: its pool is closed.
    pool.close().await;

    let worktrees = codegg_core::worktree_service::WorktreeService::sqlite(
        isolated_pool().await,
        std::path::PathBuf::from("/tmp/codegg-managed-worktrees"),
        Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
    );
    let permits = WriterPermitService::new(
        WriterPermitStore::new(isolated_pool().await),
        WorktreePermitAuthority::new(worktrees.clone()),
    );
    let factory = DaemonWriterFenceFactory::new(permits, worktrees, runs, None, false);

    let outcome = factory
        .fence_for(WriterScopeRequest {
            agent_run_id: Some(run_id.as_str().to_owned()),
            workspace_root: std::path::PathBuf::from("/tmp"),
            attempt_id: Some("attempt-unreadable".to_owned()),
            cancellation: None,
        })
        .await;

    match outcome {
        Err(WriterFenceError::Unavailable(reason)) => {
            assert!(
                !reason.is_empty(),
                "the refusal must say why the store could not be read"
            );
        }
        Err(other) => panic!("expected the store to be unavailable, got {other:?}"),
        Ok(_) => {
            panic!("an unreadable run store must not resolve a fence: it must refuse")
        }
    }
}
