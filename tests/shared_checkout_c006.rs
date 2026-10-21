//! M005 corrective C006 — the restricted shared checkout, qualified.
//!
//! The shared profile is the last piece of the M005 writer-containment
//! corrective: two bounded agents on **one** managed checkout, with
//! CodeGG-mediated serialized native writes and every process-capable
//! surface refused.
//!
//! What this suite proves:
//!
//! * a placement request puts two members of a live group on **one**
//!   checkout — the first allocates it, the second joins it, and two
//!   members that race for the same group still end up on one checkout;
//! * the shared checkout's writer permit serializes the two writers: the
//!   second is refused while the first holds it, and takes it only once the
//!   first has settled;
//! * a run outside the group may not write the shared checkout at all;
//! * the placement is refused for an unknown group and for a run that is
//!   not a member of the named group;
//! * a live persistent process refuses the placement — the C005 reservation
//!   preflight — and the placement succeeds once the process is drained;
//! * a process effect is refused on a shared checkout;
//! * the shared profile is **off** on a platform with no native containment
//!   evidence, and the refusal says why;
//! * the default placement is unchanged: a run that does not ask for the
//!   shared profile still gets an isolated checkout, and a durable payload
//!   written before the placement existed still decodes as the default.
//!
//! What this suite does **not** claim:
//!
//! * It does not claim the profile is qualified on every platform. The
//!   platform gate is part of the implementation, and only a platform with
//!   native containment evidence may enable it.
//! * It does not claim that two agents can *merge* their work. Integration
//!   remains an explicit, typed step that CodeGG never performs on its own.
//! * It does not claim that an external editor's writes to the same
//!   directory are mediated. The contract covers CodeGG's own writers.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use codegg::agent::worker::SubAgentPool;
use codegg::config::schema::Config;
use codegg::core::writer_fence::{DaemonWriterFence, DaemonWriterFenceFactory};
use codegg::core::writer_permit::{WorktreePermitAuthority, WriterPermitService};
use codegg::provider::ProviderRegistry;
use codegg::scheduler::executors::SubagentAuthority;
use codegg::scheduler::{JobScheduler, JobSubmissionService, ResolvedSchedulerConfig};
use codegg::session::SessionStore;
use codegg::tool::writer_fence::{
    CheckoutWriterFence, CheckoutWriterScope, NativeEffectClass, WriterFenceError,
    WriterFenceFactory, WriterFenceOutcome, WriterFenceRequest, WriterScopeRequest,
};
use codegg_core::agent_run::{
    AgentRunBudget, AgentRunStatus, AgentRunStore, InMemoryAgentRunStore, NewAgentRun, NewAgentTask,
};
use codegg_core::agent_run_group::{
    AgentRunGroupOwner, AgentRunGroupService, NewAgentRunGroup, RunJoinPolicy,
};
use codegg_core::identity::{
    AgentRunGroupId, AgentRunId, AgentTaskId, ProjectId, RepositoryId, WorkspaceId, WorktreeId,
};
use codegg_core::jobs::{
    DaemonGeneration, ExecutionTarget, IdempotencyClass, InMemoryJobStore, JobKind, JobPayload,
    JobPriority, JobSource, JobStore, NewJob, ResourceRequest, RetryPolicy,
};
use codegg_core::process_admission::{
    PersistentProcessEntry, PersistentProcessRegistry, ProcessAdmissionService, ProcessKind,
};
use codegg_core::run_placement::{
    shared_checkout_availability, RunPlacement, QUALIFIED_SHARED_CHECKOUT_PLATFORMS,
};
use codegg_core::shared_checkout::{
    SharedCheckoutCoordinator, SharedCheckoutError, SharedCheckoutOutcome, SharedCheckoutRequest,
};
use codegg_core::transport_auth::AuthenticatedPrincipal;
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};
use codegg_core::worktree_service::{CreateWorktreeRequest, WorktreeService};
use codegg_core::writer_permit::WriterPermitStore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

async fn isolated_pool() -> SqlitePool {
    let name = format!("shared_checkout_c006_{}", uuid::Uuid::new_v4().simple());
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
    AuthenticatedPrincipal::local_owner("shared-checkout-c006")
}

/// A temporary directory that removes itself when the test ends.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "codegg-c006-{label}-{}",
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

/// A real Git repository, so the fixture is a checkout and not a directory
/// that merely looks like one. A managed worktree cannot be created without
/// a commit to branch from.
fn init_git_repository(root: &std::path::Path) {
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git is available");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--quiet", "--initial-branch=main"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);
    std::fs::write(root.join("README"), "base\n").expect("write the base file");
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "base"]);
}

/// One live group of two runs, and the authority that places them on a
/// shared checkout.
struct Shared {
    /// Held so the pool outlives the fixture. It is never read.
    #[allow(dead_code)]
    pool: SqlitePool,
    repository_root: TempDir,
    /// Held so the managed-worktree root outlives the fixture. It is never
    /// read.
    #[allow(dead_code)]
    managed_root: TempDir,
    worktrees: Arc<WorktreeService>,
    runs: Arc<dyn AgentRunStore>,
    groups: Arc<AgentRunGroupService>,
    permits: Arc<WriterPermitService>,
    coordinator: Arc<SharedCheckoutCoordinator>,
    processes: Arc<PersistentProcessRegistry>,
    admission: Arc<ProcessAdmissionService>,
    group_id: AgentRunGroupId,
    worktree_id: WorktreeId,
    owner_run: AgentRunId,
    member_run: AgentRunId,
    outsider_run: AgentRunId,
    owner_task: AgentTaskId,
    member_task: AgentTaskId,
    project_id: ProjectId,
    repository_id: RepositoryId,
    workspace_id: WorkspaceId,
    session_id: String,
    turn_id: String,
}

impl Shared {
    async fn new(label: &str) -> Self {
        let pool = isolated_pool().await;
        let repository_root = TempDir::new(&format!("{label}-repository"));
        init_git_repository(repository_root.path());
        let managed_root = TempDir::new(&format!("{label}-managed"));

        let project_id = ProjectId::new();
        let repository_id = RepositoryId::new();
        let workspace_id = WorkspaceId::new();
        let owner_run = AgentRunId::new();
        let member_run = AgentRunId::new();
        let outsider_run = AgentRunId::new();
        let group_id = AgentRunGroupId::new();
        let session_id = format!("session-{}", uuid::Uuid::new_v4().simple());
        let turn_id = format!("turn-{}", uuid::Uuid::new_v4().simple());

        let worktrees = WorktreeService::sqlite(
            pool.clone(),
            managed_root.path().to_path_buf(),
            Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
        );
        let runs: Arc<dyn AgentRunStore> = Arc::new(InMemoryAgentRunStore::new());
        let groups = AgentRunGroupService::in_memory(runs.clone());
        let permits = WriterPermitService::new(
            WriterPermitStore::new(pool.clone()),
            WorktreePermitAuthority::new(worktrees.clone()).with_groups(groups.clone()),
        );

        // The group's managed checkout.
        //
        // The record is written directly rather than allocated through
        // `WorktreeService::create`, because **managed worktree creation is
        // unavailable in this tree by design**: `git worktree add` needs a
        // contained Git backend that does not exist here, and the tree's own
        // tests assert that refusal. The checkout therefore arrives already
        // built and leased, which is the shape C006's placement joins.
        let worktree_id = WorktreeId::new();
        let now = chrono::Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT INTO managed_worktree (worktree_id, project_id, repository_id, workspace_id, \
             repository_root, path, branch, base_commit, managed, state, health, lease_generation, \
             owner_run_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'codegg/worktree/c006', 'abc', 1, 'ready', 'clean', 0, NULL, ?, ?)",
        )
        .bind(worktree_id.as_str())
        .bind(project_id.as_str())
        .bind(repository_id.as_str())
        .bind(workspace_id.as_str())
        .bind(repository_root.path().to_string_lossy().to_string())
        .bind(repository_root.path().to_string_lossy().to_string())
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert the group's managed checkout");

        // The admission authority, wired to the same reservation state the
        // daemon uses, so the C005 preflight is the real one.
        let reservations = codegg_core::checkout_reservation::CheckoutReservationService::new(
            worktrees.clone(),
            Some(groups.clone()),
        );
        let processes = PersistentProcessRegistry::new();
        let admission = ProcessAdmissionService::new(Some(reservations), processes.clone());

        // Three real delegated runs under one turn, so a turn-owned group can
        // cover two of them and leave the third outside it.
        let mut task_ids: Vec<AgentTaskId> = Vec::new();
        for (index, run_id) in [owner_run.clone(), member_run.clone(), outsider_run.clone()]
            .into_iter()
            .enumerate()
        {
            let task_id = AgentTaskId::new();
            let task = NewAgentTask {
                task_id: task_id.clone(),
                parent_task_id: None,
                originating_session_id: session_id.clone(),
                originating_turn_id: Some(turn_id.clone()),
                project_id: project_id.clone(),
                repository_id: Some(repository_id.clone()),
                workspace_id: workspace_id.clone(),
                requested_agent: "build".to_owned(),
                delegation_key: format!("delegation-{index}-{}", uuid::Uuid::new_v4().simple()),
                request_fingerprint: format!("fingerprint-{index}"),
                description: "c006 shared checkout qualification".to_owned(),
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
            task_ids.push(task_id);
        }

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

        // The lease owner holds the group's checkout. It is the run whose
        // durable record names the checkout, which is how the coordinator
        // finds it when a member joins.
        runs.attach_worktree(&owner_run, worktree_id.clone())
            .await
            .expect("place the lease owner on the group's checkout");
        worktrees
            .acquire_lease(&worktree_id, &owner_run)
            .await
            .expect("acquire the checkout's lease");

        // A second checkout, held by a run that is in no group. It gives the
        // outsider a checkout of its own, so the outsider's fence resolves
        // against a lease it does not hold rather than against no checkout
        // at all.
        let other_root = TempDir::new(&format!("{label}-other"));
        init_git_repository(other_root.path());
        let other_worktree_id = WorktreeId::new();
        sqlx::query(
            "INSERT INTO managed_worktree (worktree_id, project_id, repository_id, workspace_id, \
             repository_root, path, branch, base_commit, managed, state, health, lease_generation, \
             owner_run_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'codegg/worktree/c006-other', 'abc', 1, 'ready', 'clean', 0, NULL, ?, ?)",
        )
        .bind(other_worktree_id.as_str())
        .bind(project_id.as_str())
        .bind(repository_id.as_str())
        .bind(workspace_id.as_str())
        .bind(other_root.path().to_string_lossy().to_string())
        .bind(other_root.path().to_string_lossy().to_string())
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert the second managed checkout");
        let other_owner = AgentRunId::new();
        worktrees
            .acquire_lease(&other_worktree_id, &other_owner)
            .await
            .expect("acquire the second checkout's lease");
        runs.attach_worktree(&outsider_run, other_worktree_id.clone())
            .await
            .expect("place the outsider on its own checkout");

        let coordinator = SharedCheckoutCoordinator::new(
            Some(groups.clone()),
            Some(runs.clone()),
            Some(worktrees.clone()),
            Some(admission.clone()),
        );

        Self {
            pool,
            repository_root,
            managed_root,
            worktrees,
            runs,
            groups,
            permits,
            coordinator,
            processes,
            admission,
            group_id,
            worktree_id,
            owner_run,
            member_run,
            outsider_run,
            owner_task: task_ids[0].clone(),
            member_task: task_ids[1].clone(),
            project_id,
            repository_id,
            workspace_id,
            session_id,
            turn_id,
        }
    }

    /// The allocation request the coordinator uses when the group has no
    /// checkout yet.
    fn allocation(&self, owner_run_id: &AgentRunId) -> CreateWorktreeRequest {
        CreateWorktreeRequest {
            project_id: self.project_id.clone(),
            repository_id: self.repository_id.clone(),
            workspace_id: self.workspace_id.clone(),
            node_id: None,
            repository_root: self.repository_root.path().to_path_buf(),
            base_commit: None,
            base_ref: None,
            target_branch: None,
            base_path: Some(self.repository_root.path().to_path_buf()),
            owner_run_id: owner_run_id.clone(),
        }
    }

    /// Place one run on the group's checkout.
    /// Place one run on the group's checkout, the way the executor does.
    ///
    /// The coordinator resolves the group's checkout; the run's durable
    /// record is attached to it afterwards, and it is that record the fence
    /// reads. Both halves are exercised here, because a fence that resolved
    /// anything else would be advisory rather than authoritative.
    async fn place(&self, run_id: &AgentRunId) -> SharedCheckoutOutcome {
        let outcome = self
            .coordinator
            .acquire(SharedCheckoutRequest {
                group_id: self.group_id.clone(),
                member_run_id: run_id.clone(),
                allocation: self.allocation(run_id),
            })
            .await
            .expect("the placement succeeds for a member of a live group");
        self.runs
            .attach_worktree(run_id, outcome.checkout().worktree_id.clone())
            .await
            .expect("attach the run to the group's checkout");
        outcome
    }

    /// The fence for one run, resolved the way the daemon resolves it.
    fn fence(
        &self,
        run_id: &AgentRunId,
        worktree_id: &WorktreeId,
        root: &std::path::Path,
    ) -> Arc<DaemonWriterFence> {
        Arc::new(
            DaemonWriterFence::new(
                CheckoutWriterScope {
                    worktree_id: worktree_id.as_str().to_owned(),
                    checkout_root: root.to_path_buf(),
                    agent_run_id: Some(run_id.as_str().to_owned()),
                    group_id: Some(self.group_id.as_str().to_owned()),
                },
                self.permits.clone(),
                self.worktrees.clone(),
                self.runs.clone(),
                Some(self.groups.clone()),
                Some(run_id.clone()),
                Some("attempt-c006".to_owned()),
                None,
            )
            .with_wait_timeout(std::time::Duration::from_millis(250)),
        )
    }

    fn request(
        &self,
        root: &std::path::Path,
        key: &str,
        class: NativeEffectClass,
    ) -> WriterFenceRequest {
        WriterFenceRequest {
            effect_class: class,
            targets: vec![root.join("README")],
            idempotency_key: key.to_owned(),
            granted_roots: Vec::new(),
            principal: Some(principal()),
        }
    }
}

// ---------------------------------------------------------------------------
// The placement
// ---------------------------------------------------------------------------

/// A member of a live group is placed on **the group's checkout** — the one
/// the lease owner already holds. The placement is what attaches the
/// member's durable record to that checkout, and the placement is the only
/// thing that does.
#[tokio::test]
async fn a_group_member_joins_the_groups_checkout() {
    let shared = Shared::new("placement").await;

    // Before the placement, the member holds no checkout at all.
    let before = shared
        .runs
        .get_run(&shared.member_run)
        .await
        .expect("read the member")
        .expect("the member exists");
    assert!(
        before.worktree_id.is_none(),
        "the member starts with no checkout"
    );

    let placed = shared.place(&shared.member_run).await;
    assert!(
        !placed.is_creator(),
        "the member joins the checkout the group already holds"
    );
    assert_eq!(
        placed.checkout().worktree_id,
        shared.worktree_id,
        "the member joins the group's own checkout"
    );

    // The member's durable record now names that checkout, so the fence —
    // which reads the run's own record — resolves the same scope for it.
    let after = shared
        .runs
        .get_run(&shared.member_run)
        .await
        .expect("read the member")
        .expect("the member exists");
    assert_eq!(
        after.worktree_id.as_ref(),
        Some(&shared.worktree_id),
        "the member's durable record names the shared checkout"
    );
}

/// Two members that reach the placement at the same time still end up on one
/// checkout. The group's lock is what makes that true: without it both
/// could resolve the group's checkout and attach independently.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_members_racing_for_one_group_end_up_on_one_checkout() {
    let shared = Shared::new("race").await;

    let owner = shared.coordinator.clone();
    let member = shared.coordinator.clone();
    let owner_run = shared.owner_run.clone();
    let member_run = shared.member_run.clone();
    let allocation_owner = shared.allocation(&shared.owner_run);
    let allocation_member = shared.allocation(&shared.member_run);
    let group_id = shared.group_id.clone();

    let (first, second) = tokio::join!(
        owner.acquire(SharedCheckoutRequest {
            group_id: group_id.clone(),
            member_run_id: owner_run,
            allocation: allocation_owner,
        }),
        member.acquire(SharedCheckoutRequest {
            group_id,
            member_run_id: member_run,
            allocation: allocation_member,
        }),
    );
    let first = first.expect("the owner's placement succeeds");
    let second = second.expect("the member's placement succeeds");

    assert_eq!(
        first.checkout().worktree_id,
        second.checkout().worktree_id,
        "two members racing for one group must land on one checkout"
    );
    assert_eq!(
        first.checkout().worktree_id,
        shared.worktree_id,
        "and it is the group's own checkout"
    );
}

/// A group that holds no checkout yet cannot be placed on one in this tree:
/// managed worktree creation needs a contained Git backend that does not
/// exist here. The refusal is **typed** and it does not fall back to an
/// isolated checkout — a caller that asked to share is told no, rather than
/// being silently given something else.
#[tokio::test]
async fn a_group_without_a_checkout_refuses_the_placement_with_a_typed_error() {
    let shared = Shared::new("no-checkout").await;

    // A second group, with no checkout of its own.
    let group_id = AgentRunGroupId::new();
    let second_run = AgentRunId::new();
    let task = NewAgentTask {
        task_id: AgentTaskId::new(),
        parent_task_id: None,
        originating_session_id: shared.session_id.clone(),
        originating_turn_id: Some(shared.turn_id.clone()),
        project_id: shared.project_id.clone(),
        repository_id: Some(shared.repository_id.clone()),
        workspace_id: shared.workspace_id.clone(),
        requested_agent: "build".to_owned(),
        delegation_key: format!("delegation-{}", uuid::Uuid::new_v4().simple()),
        request_fingerprint: format!("fingerprint-{}", uuid::Uuid::new_v4().simple()),
        description: "c006 group without a checkout".to_owned(),
    };
    let run = NewAgentRun {
        run_id: second_run.clone(),
        parent_run_id: None,
        depth: 1,
        workspace_id: shared.workspace_id.clone(),
        agent_name: "build".to_owned(),
        agent_digest: None,
        provider: "test".to_owned(),
        model: "test".to_owned(),
        authority_digest: format!("authority-{}", uuid::Uuid::new_v4().simple()),
        budget: AgentRunBudget::default(),
        shared_scratch: Vec::new(),
    };
    shared
        .runs
        .create_or_get(task, run)
        .await
        .expect("create the group's run");
    shared
        .groups
        .create(NewAgentRunGroup {
            group_id: group_id.clone(),
            root_run_id: second_run.clone(),
            owner_run_id: second_run.clone(),
            owner: AgentRunGroupOwner::Turn {
                session_id: shared.session_id.clone(),
                turn_id: shared.turn_id.clone(),
            },
            member_run_ids: vec![second_run.clone()],
            join_policy: RunJoinPolicy::All,
            cancel_remaining_on_satisfaction: false,
            idempotency_key: format!("group-{}", uuid::Uuid::new_v4().simple()),
        })
        .await
        .expect("create the second live group");

    let outcome = shared
        .coordinator
        .acquire(SharedCheckoutRequest {
            group_id,
            member_run_id: second_run.clone(),
            allocation: shared.allocation(&second_run),
        })
        .await;
    match outcome {
        Err(SharedCheckoutError::Allocation(reason)) => {
            assert!(
                reason.contains("Git backend unavailable"),
                "the refusal must name the missing backend: {reason}"
            );
        }
        other => panic!("expected a typed allocation refusal, got {other:?}"),
    }

    // The run was not silently given an isolated checkout.
    let run = shared
        .runs
        .get_run(&second_run)
        .await
        .expect("read the run")
        .expect("the run exists");
    assert!(
        run.worktree_id.is_none(),
        "a refused placement must not attach a checkout of its own"
    );
}

/// The placement is refused for a group that does not exist, and for a run
/// that is not a member of the named group. Neither falls back to an
/// isolated checkout: a caller cannot turn "you may not share" into "you
/// may share something else".
#[tokio::test]
async fn the_placement_is_refused_for_an_unknown_group() {
    let shared = Shared::new("unknown-group").await;
    let outcome = shared
        .coordinator
        .acquire(SharedCheckoutRequest {
            group_id: AgentRunGroupId::new(),
            member_run_id: shared.owner_run.clone(),
            allocation: shared.allocation(&shared.owner_run),
        })
        .await;
    assert!(
        matches!(outcome, Err(SharedCheckoutError::UnknownGroup)),
        "an unknown group must be refused: {outcome:?}"
    );
}

#[tokio::test]
async fn the_placement_is_refused_for_a_run_outside_the_group() {
    let shared = Shared::new("non-member").await;
    let outcome = shared
        .coordinator
        .acquire(SharedCheckoutRequest {
            group_id: shared.group_id.clone(),
            member_run_id: shared.outsider_run.clone(),
            allocation: shared.allocation(&shared.outsider_run),
        })
        .await;
    assert!(
        matches!(outcome, Err(SharedCheckoutError::NotAMember)),
        "a run that is not a member must be refused: {outcome:?}"
    );
}

// ---------------------------------------------------------------------------
// The permit: one writer at a time
// ---------------------------------------------------------------------------

/// Two runs on one shared checkout never overlap their mutating effects.
/// The second writer is refused while the first holds the checkout, and
/// takes it only once the first has settled.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_writers_on_the_shared_checkout_never_overlap() {
    let shared = Shared::new("writers").await;
    let placed = shared.place(&shared.owner_run).await;
    shared.place(&shared.member_run).await;
    let checkout = placed.checkout().clone();

    let first = shared.fence(&shared.owner_run, &checkout.worktree_id, &checkout.path);
    let first_permit = first
        .begin(shared.request(&checkout.path, "c006-first", NativeEffectClass::FileWrite))
        .await
        .expect("the lease owner acquires the shared checkout");
    assert!(
        first_permit.is_some(),
        "a shared checkout must take a permit: there is a second writer to exclude"
    );

    // The second writer is refused while the first holds the checkout.
    let second = shared.fence(&shared.member_run, &checkout.worktree_id, &checkout.path);
    let refused = second
        .begin(shared.request(&checkout.path, "c006-second", NativeEffectClass::FileWrite))
        .await;
    assert!(
        matches!(refused, Err(WriterFenceError::Held)),
        "the second writer must be refused while the checkout is held: {refused:?}"
    );

    // Once the first writer settles, the second one takes the checkout.
    first_permit
        .expect("held permit")
        .settle(WriterFenceOutcome::Settled)
        .await
        .expect("the first writer settles its permit");
    let permit = second
        .begin(shared.request(&checkout.path, "c006-third", NativeEffectClass::FileWrite))
        .await
        .expect("the checkout is free once the first writer settled");
    assert!(permit.is_some(), "the second writer now takes the checkout");
    permit
        .expect("held permit")
        .settle(WriterFenceOutcome::Settled)
        .await
        .expect("the second writer settles its permit");
}

/// A run outside the group may not write the shared checkout. The group is
/// the bounded reservation; without it the lease owner is the only writer.
///
/// The outsider holds a checkout of its own, so its fence resolves against a
/// lease it does not hold. The scope it was handed names the shared
/// checkout, and the run's own durable record wins over that scope — a
/// caller cannot borrow another checkout's authority by naming it.
#[tokio::test]
async fn a_run_outside_the_group_may_not_write_the_shared_checkout() {
    let shared = Shared::new("outsider").await;
    let placed = shared.place(&shared.owner_run).await;
    let checkout = placed.checkout().clone();

    let outsider = shared.fence(&shared.outsider_run, &checkout.worktree_id, &checkout.path);
    let outcome = outsider
        .begin(shared.request(
            &checkout.path,
            "c006-outsider",
            NativeEffectClass::FileWrite,
        ))
        .await;
    assert!(
        matches!(outcome, Err(WriterFenceError::NotAuthorized { .. })),
        "a run that shares no live group must be refused: {outcome:?}"
    );
}

/// A process effect is refused on a shared checkout. The shared profile
/// admits no unqualified process, and the fence is one of the places that
/// says so.
#[tokio::test]
async fn a_process_effect_is_refused_on_a_shared_checkout() {
    let shared = Shared::new("process").await;
    let placed = shared.place(&shared.owner_run).await;
    let checkout = placed.checkout().clone();

    let fence = shared.fence(&shared.owner_run, &checkout.worktree_id, &checkout.path);
    let outcome = fence
        .begin(shared.request(
            &checkout.path,
            "c006-process",
            NativeEffectClass::ProcessSpawn,
        ))
        .await;
    assert!(
        matches!(outcome, Err(WriterFenceError::EffectClassDenied { .. })),
        "a process effect must be refused on a shared checkout: {outcome:?}"
    );
}

// ---------------------------------------------------------------------------
// The C005 preflight
// ---------------------------------------------------------------------------

/// A live persistent process refuses the placement, and the placement
/// succeeds once the process is drained. The process's lifetime belongs to
/// its own owner: the refusal never kills it.
#[tokio::test]
async fn a_live_persistent_process_refuses_the_shared_placement() {
    let shared = Shared::new("preflight").await;

    // The group's checkout, allocated by the first member, is where the
    // persistent process is live.
    let placed = shared.place(&shared.owner_run).await;
    let checkout = placed.checkout().clone();

    shared.processes.register(PersistentProcessEntry {
        process_id: "lsp-1".to_owned(),
        kind: ProcessKind::LanguageServer,
        checkout_root: checkout.path.clone(),
    });

    // A second member cannot join while the process is live in the checkout.
    let refused = shared
        .coordinator
        .acquire(SharedCheckoutRequest {
            group_id: shared.group_id.clone(),
            member_run_id: shared.member_run.clone(),
            allocation: shared.allocation(&shared.member_run),
        })
        .await;
    assert!(
        matches!(
            refused,
            Err(SharedCheckoutError::PersistentProcessPresent(_))
        ),
        "a live persistent process must refuse the placement: {refused:?}"
    );

    // Draining the process releases the refusal.
    assert!(shared.processes.release("lsp-1"), "the process is drained");
    let admitted = shared
        .coordinator
        .acquire(SharedCheckoutRequest {
            group_id: shared.group_id.clone(),
            member_run_id: shared.member_run.clone(),
            allocation: shared.allocation(&shared.member_run),
        })
        .await;
    let admitted = admitted.expect("the placement succeeds once the process is drained");
    assert_eq!(
        admitted.checkout().worktree_id,
        checkout.worktree_id,
        "the member joins the checkout the group already holds"
    );
}

// ---------------------------------------------------------------------------
// The production seam
// ---------------------------------------------------------------------------

/// The daemon's writer-fence factory resolves the fence from the run's own
/// durable record. This is the seam that makes the shared profile
/// authoritative rather than advisory: the fence a child runs under is the
/// one its own run record names.
#[tokio::test]
async fn the_fence_factory_resolves_the_fence_from_the_runs_own_record() {
    let shared = Shared::new("factory").await;
    let placed = shared.place(&shared.owner_run).await;
    shared.place(&shared.member_run).await;
    let checkout = placed.checkout().clone();

    let factory = DaemonWriterFenceFactory::new(
        shared.permits.clone(),
        shared.worktrees.clone(),
        shared.runs.clone(),
        Some(shared.groups.clone()),
        true,
    );

    let fence = factory
        .fence_for(WriterScopeRequest {
            agent_run_id: Some(shared.member_run.as_str().to_owned()),
            workspace_root: checkout.path.clone(),
            attempt_id: Some("attempt-factory".to_owned()),
            cancellation: None,
        })
        .await
        .expect("the factory resolves the fence")
        .expect("a run that holds a managed checkout has a fence");
    assert_eq!(
        fence.scope().checkout_root,
        checkout.path,
        "the fence's scope is the checkout the run actually holds"
    );
    assert_eq!(
        fence.scope().worktree_id,
        checkout.worktree_id.as_str(),
        "the fence's scope names the run's own checkout"
    );
}

/// A run that holds no managed checkout gets no fence at all: it has no
/// lease, so there is nothing to fence.
#[tokio::test]
async fn a_run_without_a_managed_checkout_has_no_fence() {
    let shared = Shared::new("no-fence").await;
    let factory = DaemonWriterFenceFactory::new(
        shared.permits.clone(),
        shared.worktrees.clone(),
        shared.runs.clone(),
        Some(shared.groups.clone()),
        true,
    );
    // A run with no durable record at all.
    let fence = factory
        .fence_for(WriterScopeRequest {
            agent_run_id: Some(AgentRunId::new().as_str().to_owned()),
            workspace_root: shared.repository_root.path().to_path_buf(),
            attempt_id: None,
            cancellation: None,
        })
        .await
        .expect("the factory resolves the fence");
    assert!(
        fence.is_none(),
        "a run with no managed checkout has no fence"
    );
}

// ---------------------------------------------------------------------------
// The profile and the platform
// ---------------------------------------------------------------------------

/// The shared profile denies every tool that starts a process in the
/// checkout and every tool that uses a persistent process the host owns.
#[test]
fn the_shared_profile_denies_every_process_capable_tool() {
    let denied = codegg::tool::process_capability::shared_profile_denied_tools(&[]);
    for tool in [
        "bash",
        "terminal",
        "python_script",
        "test",
        "verify",
        "ci_watch",
        "formatter",
        "tool_program",
    ] {
        assert!(
            denied.iter().any(|entry| entry == tool),
            "the shared profile must deny {tool}: {denied:?}"
        );
    }
    for tool in ["lsp", "lsp_read"] {
        assert!(
            denied.iter().any(|entry| entry == tool),
            "the shared profile must deny the persistent-process tool {tool}: {denied:?}"
        );
    }
    // The delegating and submitting tools are deliberately **not** denied:
    // a shared group is made of delegating children, and a child runs in its
    // own scope.
    for tool in ["task", "job"] {
        assert!(
            !denied.iter().any(|entry| entry == tool),
            "the shared profile must not deny {tool}: {denied:?}"
        );
    }
    // The base list is preserved, not replaced.
    let base = vec!["write".to_owned()];
    let denied = codegg::tool::process_capability::shared_profile_denied_tools(&base);
    assert!(
        denied.iter().any(|entry| entry == "write"),
        "the shared profile narrows the base list, it does not replace it"
    );
}

/// The shared profile is available only on a platform with native
/// containment evidence, and the refusal says which platform is missing it.
#[test]
fn the_shared_profile_is_platform_gated() {
    let availability = shared_checkout_availability();
    if QUALIFIED_SHARED_CHECKOUT_PLATFORMS.contains(&std::env::consts::OS) {
        assert!(availability.is_available());
        assert!(availability.reason().is_none());
    } else {
        assert!(!availability.is_available());
        let reason = availability
            .reason()
            .expect("an unavailable platform says why");
        assert!(
            reason.contains(std::env::consts::OS),
            "the refusal must name the platform: {reason}"
        );
    }
}

// ---------------------------------------------------------------------------
// The default is unchanged
// ---------------------------------------------------------------------------

/// The default placement is the isolated one, and a durable payload written
/// before the placement existed still decodes as the default.
#[test]
fn the_default_placement_is_the_isolated_one() {
    assert_eq!(RunPlacement::default(), RunPlacement::Auto);
    assert!(!RunPlacement::default().is_shared());
    assert!(RunPlacement::default().shared_group_id().is_none());
    assert!(!RunPlacement::IsolatedNew.is_shared());

    // The shape a durable job payload had before the placement existed.
    let legacy = r#"{"kind":"subagent_run","prompt":"p","agent":"build","model":null,"parent_id":null,"denied_tools":[],"allowed_paths":[],"max_tool_calls":null,"task_id":"t","run_id":"r","delegation_key":"k"}"#;
    let decoded: codegg_core::jobs::JobPayload =
        serde_json::from_str(legacy).expect("a legacy payload still decodes");
    match decoded {
        codegg_core::jobs::JobPayload::SubagentRun { placement, .. } => {
            assert_eq!(placement, RunPlacement::Auto);
        }
        other => panic!("expected a subagent-run payload, got {other:?}"),
    }
}

/// The shared profile names the group it shares, and the group is named by
/// the placement rather than inferred.
#[test]
fn a_shared_placement_names_its_group() {
    let group_id = AgentRunGroupId::new();
    let placement = RunPlacement::SerializedShared {
        group_id: group_id.clone(),
    };
    assert!(placement.is_shared());
    assert_eq!(placement.shared_group_id(), Some(&group_id));
}

// ---------------------------------------------------------------------------
// The composed production carrier
// ---------------------------------------------------------------------------

/// Two members of one group, placed through the **real scheduler and the real
/// executor**, land on one checkout — and the fences the daemon resolves for
/// those two runs queue behind each other.
///
/// The tests above prove the *resolution*: the coordinator joins a member to
/// the group's checkout, and the daemon's factory resolves that checkout for
/// the run's own durable record. This one proves the **production carrier**
/// uses them. The executor's shared branch runs, the durable run record is
/// attached to the group's checkout, and the fence the daemon resolves for
/// that run then names the same checkout.
///
/// The child cannot actually start here — the pool is built with no agents, so
/// the run stops after the placement. That is deliberate: the placement is
/// what is under test, and it happens before the agent is looked up. An
/// unknown agent is treated as isolation-requiring, which is what routes the
/// run into the placement branch at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_members_placed_through_the_real_scheduler_share_one_checkout() {
    let shared = Shared::new("composed").await;

    // The scheduler's world: a real workspace, so the scheduler can lease it.
    let root = tempfile::tempdir().expect("workspace root");
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    let workspace = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register the workspace");
    let services = WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );

    // A pool with no agents. An unknown agent is isolation-requiring, which
    // is what routes the run into the placement branch.
    let pool = Arc::new(
        SubAgentPool::new(
            &Config::default(),
            Vec::new(),
            ProviderRegistry::new(),
            Arc::new(SessionStore::new(shared.pool.clone())),
            Some(shared.pool.clone()),
        )
        .await,
    );

    // The daemon's authority: the same coordinator and fence factory the
    // daemon builds, not test doubles.
    let factory = DaemonWriterFenceFactory::new(
        shared.permits.clone(),
        shared.worktrees.clone(),
        shared.runs.clone(),
        Some(shared.groups.clone()),
        true,
    );
    let authority = SubagentAuthority {
        process_admission: Some(shared.admission.clone()),
        shared_checkout: Some(shared.coordinator.clone()),
        writer_fences: Some(factory.clone() as Arc<dyn WriterFenceFactory>),
    };

    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let scheduler = JobScheduler::new(
        store.clone(),
        services.clone(),
        ResolvedSchedulerConfig::default(),
        DaemonGeneration::new_unchecked("gen-c006-composed"),
    );
    scheduler
        .register_default_executors_sync_with_agent_runs_and_control_and_worktree(
            Some(pool),
            Some(shared.runs.clone()),
            None,
            Some(shared.worktrees.clone()),
            Some(authority),
        )
        .expect("register the default executors");

    let submission = JobSubmissionService::new(
        store.clone(),
        scheduler.clone(),
        services.clone(),
        DaemonGeneration::new_unchecked("gen-c006-composed"),
    );

    // Two members of the same group, each asking for the shared placement.
    //
    // The durable path queues a run before the executor prepares it, so the
    // fixture moves each run to `Queued` first: the executor's transition to
    // `Preparing` is only legal from there.
    let mut placed = Vec::new();
    for (index, (run_id, task_id)) in [
        (shared.owner_run.clone(), shared.owner_task.clone()),
        (shared.member_run.clone(), shared.member_task.clone()),
    ]
    .into_iter()
    .enumerate()
    {
        shared
            .runs
            .transition(&run_id, AgentRunStatus::Queued)
            .await
            .expect("queue the delegated run");
        let job = submission
            .submit(
                None,
                NewJob {
                    workspace_id: workspace.id.clone(),
                    session_id: Some(shared.session_id.clone()),
                    turn_id: Some(shared.turn_id.clone()),
                    kind: JobKind::Subagent,
                    source: JobSource::Interactive,
                    priority: JobPriority::Interactive,
                    payload: JobPayload::SubagentRun {
                        prompt: "composed shared checkout placement".to_owned(),
                        agent: "build".to_owned(),
                        model: None,
                        parent_id: None,
                        denied_tools: Vec::new(),
                        allowed_paths: Vec::new(),
                        max_tool_calls: None,
                        task_id,
                        run_id: run_id.clone(),
                        delegation_key: format!(
                            "composed-{index}-{}",
                            uuid::Uuid::new_v4().simple()
                        ),
                        base_commit: None,
                        placement: RunPlacement::SerializedShared {
                            group_id: shared.group_id.clone(),
                        },
                    },
                    resource_request: ResourceRequest::default(),
                    timeout: None,
                    retry_policy: RetryPolicy::no_retry(),
                    idempotency: IdempotencyClass::SafeRepeat,
                    not_before: None,
                    deadline: None,
                    schedule_id: None,
                    depends_on: vec![],
                    parent_job_id: None,
                    parent_attempt_id: None,
                    parent_call_id: None,
                    parent_program_id: None,
                    parent_instruction_sequence: None,
                    relation_kind: None,
                    target: ExecutionTarget::default(),
                },
            )
            .await
            .expect("submit the shared placement");
        placed.push((run_id, job.job_id));
    }

    scheduler
        .reconcile()
        .await
        .expect("reconcile the submitted jobs");
    scheduler.clone().admit_and_dispatch_batch().await;
    for (_, job_id) in &placed {
        scheduler
            .wait_for_completion(job_id, Duration::from_secs(30))
            .await
            .expect("the placed job completes");
    }

    // Both runs are attached to the group's checkout: the executor's shared
    // branch placed them, through the real coordinator.
    for (run_id, _) in &placed {
        let run = shared
            .runs
            .get_run(run_id)
            .await
            .expect("the run record")
            .expect("the placed run has a durable record");
        assert_eq!(
            run.worktree_id.as_ref().map(|id| id.as_str()),
            Some(shared.worktree_id.as_str()),
            "the placed run must hold the group's checkout"
        );
    }

    // And the daemon resolves that same checkout for both runs, through the
    // production factory rather than a directly constructed fence.
    let scope_for = |run_id: &AgentRunId, attempt: &str| WriterScopeRequest {
        agent_run_id: Some(run_id.as_str().to_owned()),
        workspace_root: shared.repository_root.path().to_path_buf(),
        attempt_id: Some(attempt.to_owned()),
        cancellation: None,
    };
    let first = factory
        .fence_for(scope_for(&shared.owner_run, "composed-first"))
        .await
        .expect("the factory resolves the first fence")
        .expect("a placed run has a fence");
    let second = factory
        .fence_for(scope_for(&shared.member_run, "composed-second"))
        .await
        .expect("the factory resolves the second fence")
        .expect("a placed run has a fence");
    assert_eq!(
        first.scope().worktree_id,
        second.scope().worktree_id,
        "both runs resolve the same checkout"
    );

    // The first writer takes the checkout.
    let held = first
        .begin(shared.request(
            shared.repository_root.path(),
            "composed-first",
            NativeEffectClass::FileWrite,
        ))
        .await
        .expect("the first writer takes the shared checkout");
    assert!(
        held.is_some(),
        "a shared checkout must take a permit: there is a second writer to exclude"
    );

    // The second writer is queued behind it. It cannot take the checkout
    // while the first holds it, so it proceeds only once the first settles.
    //
    // The first settles on a timer rather than on demand: the production
    // fence's wait bound is thirty seconds, and a proof that waits for it
    // would make this test a thirty-second test.
    let settler = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(250)).await;
        held.expect("held permit")
            .settle(WriterFenceOutcome::Settled)
            .await
            .expect("the first writer settles its permit");
    });
    let queued = second
        .begin(shared.request(
            shared.repository_root.path(),
            "composed-second",
            NativeEffectClass::FileWrite,
        ))
        .await
        .expect("the second writer takes the checkout once the first settles");
    assert!(
        queued.is_some(),
        "the second writer takes the checkout after the first settles"
    );
    settler.await.expect("the settling writer finished");
    queued
        .expect("held permit")
        .settle(WriterFenceOutcome::Settled)
        .await
        .expect("the second writer settles its permit");
}
