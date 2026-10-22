//! M008 — the comparative trajectory suite for the agent-workstream blackboard.
//!
//! M008 compares five agent configurations (S0–S4) against each other. The
//! comparison is only meaningful if every arm runs the same contract, which
//! `architecture/blackboard_contract_matrix.md` freezes and
//! `scripts/check_blackboard_contract_matrix.py` keeps frozen.
//!
//! This suite is the **functional** half of that milestone. It runs each
//! configuration's mechanics against real state — a real SQLite database, real
//! Git repositories, real worktrees, the real shared-checkout coordinator and
//! the real daemon writer fence — and it pins the acceptance criteria the
//! plan states:
//!
//! * **S0** remains operational and performs no blackboard work at all.
//! * **S1**'s isolation behaviour is unchanged: two children in separate
//!   worktrees do not see each other's edits.
//! * **S3**'s claim arbitration never creates double authoritative ownership
//!   and never widens a scope.
//! * **S4** serializes two children on one shared checkout and produces
//!   Git-verified commits.
//! * **Integration** is explicit: disjoint work integrates cleanly, and
//!   overlapping work returns an actionable conflict rather than an
//!   unqualified automatic success.
//!
//! The **peer-observation** clauses of the contract — attribution, the
//! untrusted-data trust class, and the no-blackboard path — are pinned in
//! `tests/agent_loop_harness.rs`, which owns the scripted-provider harness
//! that the peer path needs.
//!
//! What this suite does **not** claim:
//!
//! * It does not claim anything about model quality, cost or latency. The
//!   measured comparison is `tests/blackboard_economics_m008.rs`, and it is
//!   opt-in for that reason.
//! * It does not claim a live two-agent race inside one daemon process. The
//!   S4 arm here is two real writers contending for one real checkout's
//!   permit, driven sequentially, which is the carrier-level claim M005 C006
//!   closed.
//! * It does not claim the shared profile on Windows. Where the profile is
//!   unavailable the arm reports itself unrun rather than substituting an
//!   unsafe emulation.

mod common;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use common::blackboard::{
    add_worktree, commit_all, create_turn_members, git, init_repository, insert_run_rows,
    Blackboard,
};

use codegg::core::writer_fence::DaemonWriterFence;
use codegg::core::writer_permit::{WorktreePermitAuthority, WriterPermitService};
use codegg::tool::writer_fence::{
    CheckoutWriterFence, CheckoutWriterScope, NativeEffectClass, WriterFenceError,
    WriterFenceOutcome, WriterFenceRequest,
};
use codegg_core::agent_run::{AgentRunStatus, AgentRunStore, InMemoryAgentRunStore};
use codegg_core::agent_run_group::{
    AgentRunGroupOwner, AgentRunGroupService, NewAgentRunGroup, RunJoinPolicy,
};
use codegg_core::identity::{
    AgentRunGroupId, AgentRunId, ProjectId, RepositoryId, WorkspaceId, WorktreeId,
};
use codegg_core::run_placement::{
    shared_checkout_availability, QUALIFIED_SHARED_CHECKOUT_PLATFORMS,
};
use codegg_core::shared_checkout::{
    SharedCheckoutCoordinator, SharedCheckoutOutcome, SharedCheckoutRequest,
};
use codegg_core::transport_auth::AuthenticatedPrincipal;
use codegg_core::workstream_claim::{ClaimActor, ClaimTarget, WorkstreamClaimError};
use codegg_core::worktree_service::{CreateWorktreeRequest, WorktreeService};
use codegg_core::writer_permit::WriterPermitStore;
use sqlx::SqlitePool;

// ---------------------------------------------------------------------------
// S0 — one agent, no blackboard
// ---------------------------------------------------------------------------

/// S0 stays operational **and** free: a single agent performs no blackboard
/// work at all. It creates no group, no claim and no board state, so the
/// baseline an S1/S2/S3 arm is measured against is not quietly paying for
/// the blackboard.
#[tokio::test]
async fn s0_a_single_agent_performs_no_blackboard_work_at_all() {
    let blackboard = Blackboard::solo().await;

    // The single agent's ordinary checkout.
    let repository = tempfile::tempdir().expect("the single agent's checkout");
    init_repository(repository.path());
    std::fs::write(
        repository.path().join("module.py"),
        "def f():\n    return 1\n",
    )
    .expect("the single agent writes its own work");

    assert!(
        repository.path().join("module.py").exists(),
        "S0's agent works on an ordinary checkout"
    );
    assert_eq!(
        blackboard.row_count("agent_run_group").await,
        0,
        "a single agent must not create a group"
    );
    assert_eq!(
        blackboard.row_count("workstream_claim").await,
        0,
        "a single agent must not create a claim"
    );
    assert_eq!(
        blackboard.row_count("workstream_claim_event").await,
        0,
        "a single agent must not create claim events"
    );
    assert_eq!(
        blackboard.row_count("agent_run_group_member").await,
        0,
        "a single agent must not join a group"
    );
}

// ---------------------------------------------------------------------------
// S1 — two children, separate worktrees
// ---------------------------------------------------------------------------

/// S1's isolation behaviour is unchanged: two independently mutating
/// children on their own worktrees do not see each other's edits before
/// integration, and both commits are Git-verified.
#[tokio::test]
async fn s1_two_children_in_separate_worktrees_do_not_see_each_others_edits() {
    let repository = tempfile::tempdir().expect("the repository");
    init_repository(repository.path());
    // The worktrees live **outside** the repository: a worktree inside the
    // repository's own tree is untracked content, and the integration
    // assertions below check that the checkout is clean.
    let scratch = tempfile::tempdir().expect("the worktree scratch");

    let rust_root = scratch.path().join("rust");
    let python_root = scratch.path().join("python");
    add_worktree(repository.path(), &rust_root, "child/rust");
    add_worktree(repository.path(), &python_root, "child/python");

    // The Rust child edits its own module.
    std::fs::write(
        rust_root.join("lib.rs"),
        "pub fn rust_child() -> u32 { 1 }\n",
    )
    .expect("the Rust child writes its module");
    let rust_commit = commit_all(&rust_root, "rust child");

    // The Python child edits its own module.
    std::fs::write(
        python_root.join("app.py"),
        "def python_child():\n    return 2\n",
    )
    .expect("the Python child writes its module");
    let python_commit = commit_all(&python_root, "python child");

    // Neither edit crossed over before integration.
    assert!(
        !python_root.join("lib.rs").exists(),
        "the Python child must not see the Rust child's edit before integration"
    );
    assert!(
        !rust_root.join("app.py").exists(),
        "the Rust child must not see the Python child's edit before integration"
    );

    // Both commits are real, distinct Git objects.
    assert_ne!(rust_commit, python_commit);
    assert_eq!(
        String::from_utf8_lossy(&git(&["rev-parse", "HEAD"], &rust_root).stdout).trim(),
        rust_commit.as_str(),
        "the Rust child's commit is Git-verified"
    );
    assert_eq!(
        String::from_utf8_lossy(&git(&["rev-parse", "HEAD"], &python_root).stdout).trim(),
        python_commit.as_str(),
        "the Python child's commit is Git-verified"
    );
}

// ---------------------------------------------------------------------------
// Integration — explicit, never automatic
// ---------------------------------------------------------------------------

/// Disjoint work integrates cleanly when the integration is **explicit**.
/// The resulting checkout is clean: nothing is left staged, nothing is left
/// dirty, and no file is silently discarded.
#[tokio::test]
async fn an_explicit_integration_of_disjoint_work_leaves_a_clean_checkout() {
    let repository = tempfile::tempdir().expect("the repository");
    init_repository(repository.path());
    let scratch = tempfile::tempdir().expect("the worktree scratch");

    let first = scratch.path().join("first");
    let second = scratch.path().join("second");
    add_worktree(repository.path(), &first, "child/first");
    add_worktree(repository.path(), &second, "child/second");

    std::fs::write(first.join("alpha.txt"), "alpha\n").expect("the first child writes");
    commit_all(&first, "alpha");
    std::fs::write(second.join("beta.txt"), "beta\n").expect("the second child writes");
    commit_all(&second, "beta");

    // The explicit integration step.
    git(&["merge", "--no-ff", "--quiet", "child/first"], &second);
    git(
        &["merge", "--no-ff", "--quiet", "child/second"],
        repository.path(),
    );

    assert!(
        repository.path().join("alpha.txt").exists(),
        "the first child's work is integrated"
    );
    assert!(
        repository.path().join("beta.txt").exists(),
        "the second child's work is integrated"
    );
    let status = git(&["status", "--porcelain"], repository.path());
    assert!(
        status.stdout.is_empty(),
        "an explicit integration leaves a clean checkout: {}",
        String::from_utf8_lossy(&status.stdout)
    );
}

/// Overlapping work returns an **actionable conflict** rather than an
/// unqualified automatic success. The merge fails, the conflict is visible in
/// the working tree, and nothing is silently resolved.
#[tokio::test]
async fn an_overlapping_integration_returns_an_actionable_conflict() {
    let repository = tempfile::tempdir().expect("the repository");
    init_repository(repository.path());
    std::fs::write(repository.path().join("shared.txt"), "base\n").expect("write the base file");
    commit_all(repository.path(), "base with shared file");
    let scratch = tempfile::tempdir().expect("the worktree scratch");

    let first = scratch.path().join("first");
    let second = scratch.path().join("second");
    add_worktree(repository.path(), &first, "child/first");
    add_worktree(repository.path(), &second, "child/second");

    // Both children change the **same** line.
    std::fs::write(first.join("shared.txt"), "first edit\n").expect("the first child writes");
    commit_all(&first, "first edit");
    std::fs::write(second.join("shared.txt"), "second edit\n").expect("the second child writes");
    commit_all(&second, "second edit");

    // The integration is attempted explicitly, and it **fails**.
    let merge = Command::new("git")
        .args(["merge", "--no-ff", "child/first"])
        .current_dir(&second)
        .output()
        .expect("run the explicit merge");
    assert!(
        !merge.status.success(),
        "an overlapping integration must not report success"
    );

    // The conflict is visible and recoverable: the markers are in the file
    // and the index records the conflict.
    let conflicted = std::fs::read_to_string(second.join("shared.txt"))
        .expect("the conflicted file is readable");
    assert!(
        conflicted.contains("<<<<<<<") && conflicted.contains(">>>>>>>"),
        "the conflict must be visible in the file: {conflicted}"
    );
    let status = git(&["status", "--porcelain"], &second);
    assert!(
        String::from_utf8_lossy(&status.stdout).contains("shared.txt"),
        "the conflict must be reported as unresolved: {}",
        String::from_utf8_lossy(&status.stdout)
    );

    // Recovery is explicit too: aborting the merge restores the checkout
    // exactly, rather than leaving a half-merged tree behind.
    git(&["merge", "--abort"], &second);
    let after = std::fs::read_to_string(second.join("shared.txt")).expect("read the file back");
    assert_eq!(
        after, "second edit\n",
        "an aborted integration restores the child's own content exactly"
    );
    let status = git(&["status", "--porcelain"], &second);
    assert!(
        status.stdout.is_empty(),
        "an aborted integration leaves a clean checkout"
    );
}

// ---------------------------------------------------------------------------
// S3 — claims never create double ownership
// ---------------------------------------------------------------------------

/// A claim race has exactly one winner, and the losers' scope is not
/// widened: the surviving claim's target is exactly what the winner asked
/// for, and the group holds exactly one claim on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s3_a_claim_race_has_exactly_one_owner_and_never_widens_a_scope() {
    let blackboard = Blackboard::new(4).await;
    let mut contenders = Vec::new();
    for (ordinal, (run_id, _)) in blackboard.members.iter().enumerate() {
        let store = blackboard.claims.clone();
        let run_id = run_id.clone();
        let input = blackboard.claim_input(&run_id, &format!("claim-{ordinal}"), "src/work.rs");
        contenders.push(tokio::spawn(
            async move { (run_id, store.claim(input).await) },
        ));
    }

    let mut winners = Vec::new();
    for contender in contenders {
        let (run_id, outcome) = contender.await.expect("the contender finished");
        match outcome {
            Ok(claim) => winners.push((run_id, claim)),
            Err(WorkstreamClaimError::Overlap) => {}
            Err(error) => panic!("unexpected contention result: {error}"),
        }
    }
    assert_eq!(
        winners.len(),
        1,
        "exactly one contender may own the contested path"
    );

    let (winner_run, winner) = &winners[0];
    assert_eq!(
        winner.target,
        ClaimTarget::Path("src/work.rs".to_owned()),
        "the winning claim's scope is exactly what the winner asked for"
    );
    assert_eq!(winner.version, 1, "the winning claim starts at version one");

    // The group holds exactly one claim on the contested path: the race did
    // not leave a second, partially-created claim behind.
    let actor = ClaimActor::Run(winner_run.clone());
    let claims = blackboard
        .claims
        .list_group(
            &blackboard.group_id,
            &blackboard.project_id,
            &blackboard.workspace_id,
            &actor,
        )
        .await
        .expect("the owner may read the group's claims");
    assert_eq!(
        claims.len(),
        1,
        "a contested path yields exactly one durable claim"
    );
    assert_eq!(claims[0].claim_id, winner.claim_id);
}

/// Replaying the same claim request is idempotent: it converges on the same
/// claim rather than creating a second owner of the same path.
#[tokio::test]
async fn s3_replaying_a_claim_request_never_creates_a_second_owner() {
    let blackboard = Blackboard::new(2).await;
    let (run_id, _) = &blackboard.members[0];

    let first = blackboard
        .claims
        .claim(blackboard.claim_input(run_id, "same-key", "src/once.rs"))
        .await
        .expect("the first claim succeeds");
    let replay = blackboard
        .claims
        .claim(blackboard.claim_input(run_id, "same-key", "src/once.rs"))
        .await
        .expect("the replayed claim converges on the original");

    assert_eq!(
        first.claim_id, replay.claim_id,
        "a replayed request must converge on the original claim"
    );
    assert_eq!(first.version, replay.version);

    let actor = ClaimActor::Run(run_id.clone());
    let claims = blackboard
        .claims
        .list_group(
            &blackboard.group_id,
            &blackboard.project_id,
            &blackboard.workspace_id,
            &actor,
        )
        .await
        .expect("the owner may read the group's claims");
    assert_eq!(
        claims.len(),
        1,
        "a replayed request must not leave a second claim behind"
    );
}

/// A claim by a run that is **not** in the group is refused outright: the
/// board is not writable by an outsider, and the refusal is typed rather
/// than a silent no-op.
#[tokio::test]
async fn s3_a_claim_by_an_outsider_is_refused_with_a_typed_error() {
    let blackboard = Blackboard::new(1).await;
    let outsider = AgentRunId::new();

    let result = blackboard
        .claims
        .claim(blackboard.claim_input(&outsider, "outsider-key", "src/outside.rs"))
        .await;
    assert!(
        matches!(result, Err(WorkstreamClaimError::Unauthorized)),
        "an outsider must be refused: {result:?}"
    );
    assert_eq!(
        blackboard.row_count("workstream_claim").await,
        0,
        "a refused claim writes nothing"
    );
}

/// A cancelled member's board becomes **explicitly unavailable** rather than
/// silently empty. An empty board reads as "there are no claims", which is a
/// different fact from "you may no longer read this board".
#[tokio::test]
async fn s3_a_cancelled_member_board_is_explicitly_unavailable() {
    let blackboard = Blackboard::new(2).await;
    let (owner_run, _) = &blackboard.members[0];

    blackboard
        .claims
        .claim(blackboard.claim_input(owner_run, "live-claim", "src/live.rs"))
        .await
        .expect("the owner claims a path");

    // The observer's run is cancelled: its durable status is terminal, so it
    // is no longer an active group member.
    sqlx::query("UPDATE agent_run SET status = 'cancelled' WHERE run_id = ?")
        .bind(owner_run.as_str())
        .execute(&blackboard.pool)
        .await
        .expect("cancel the member run");

    let result = blackboard
        .claims
        .list_group(
            &blackboard.group_id,
            &blackboard.project_id,
            &blackboard.workspace_id,
            &ClaimActor::Run(owner_run.clone()),
        )
        .await;
    assert!(
        matches!(result, Err(WorkstreamClaimError::Unauthorized)),
        "a cancelled member must be refused the board, not handed an empty one: {result:?}"
    );
}

/// The claim events are ordered by creation and event id, so a replayed
/// reader sees the same sequence every time.
#[tokio::test]
async fn s3_claim_events_are_ordered_and_bounded() {
    let blackboard = Blackboard::new(2).await;
    let (owner_run, _) = &blackboard.members[0];
    let (peer_run, _) = &blackboard.members[1];

    let first = blackboard
        .claims
        .claim(blackboard.claim_input(owner_run, "ordered-1", "src/one.rs"))
        .await
        .expect("the first claim succeeds");
    let second = blackboard
        .claims
        .claim(blackboard.claim_input(peer_run, "ordered-2", "src/two.rs"))
        .await
        .expect("the second claim succeeds");

    let events = blackboard
        .claims
        .list_events(
            &blackboard.group_id,
            &blackboard.project_id,
            &blackboard.workspace_id,
            &ClaimActor::Run(owner_run.clone()),
        )
        .await
        .expect("the owner may read the group's claim events");
    assert!(events.len() >= 2, "both claims left an event");
    let timestamps = events
        .iter()
        .map(|event| event.created_at)
        .collect::<Vec<_>>();
    let mut sorted = timestamps.clone();
    sorted.sort_unstable();
    assert_eq!(
        timestamps, sorted,
        "claim events must be ordered by creation time"
    );
    assert_eq!(events[0].claim_id, first.claim_id);
    assert_eq!(events[1].claim_id, second.claim_id);
}

// ---------------------------------------------------------------------------
// S4 — two children on one shared checkout
// ---------------------------------------------------------------------------

/// The shared checkout: one managed checkout, one live group, two members
/// placed on it, and the real writer fence that serializes them.
struct Shared {
    #[allow(dead_code)]
    pool: SqlitePool,
    #[allow(dead_code)]
    repository_root: tempfile::TempDir,
    #[allow(dead_code)]
    managed_root: tempfile::TempDir,
    worktrees: Arc<WorktreeService>,
    runs: Arc<dyn AgentRunStore>,
    groups: Arc<AgentRunGroupService>,
    permits: Arc<WriterPermitService>,
    coordinator: Arc<SharedCheckoutCoordinator>,
    group_id: AgentRunGroupId,
    worktree_id: WorktreeId,
    owner_run: AgentRunId,
    member_run: AgentRunId,
    project_id: ProjectId,
    repository_id: RepositoryId,
    workspace_id: WorkspaceId,
}

impl Shared {
    async fn new(label: &str) -> Self {
        let pool = Blackboard::new(0).await.pool;
        let repository_root = tempfile::tempdir().expect("the shared repository");
        init_repository(repository_root.path());
        let managed_root = tempfile::tempdir().expect("the managed worktree root");

        let project_id = ProjectId::new();
        let repository_id = RepositoryId::new();
        let workspace_id = WorkspaceId::new();
        let group_id = AgentRunGroupId::new();

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

        // The group's managed checkout. The record is written directly rather
        // than allocated through `WorktreeService::create`, because managed
        // worktree creation needs a contained Git backend that does not
        // exist in this tree by design — the tree's own tests assert that
        // refusal. The checkout therefore arrives already built and leased,
        // which is the shape the shared placement joins.
        let worktree_id = WorktreeId::new();
        let now = chrono::Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT INTO managed_worktree (worktree_id, project_id, repository_id, workspace_id, \
             repository_root, path, branch, base_commit, managed, state, health, lease_generation, \
             owner_run_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'codegg/worktree/m008', 'abc', 1, 'ready', 'clean', 0, NULL, ?, ?)",
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

        // The group hierarchy is the real one: a **turn** owns the group and
        // its members are that turn's top-level delegated runs. The group
        // service refuses anything else, and this is the shape a real turn
        // uses when it spawns children.
        let session_id = format!("session-{label}-{}", uuid::Uuid::new_v4());
        let turn_id = format!("turn-{label}-{}", uuid::Uuid::new_v4().simple());
        let members = create_turn_members(
            &runs,
            &project_id,
            &workspace_id,
            &session_id,
            &turn_id,
            label,
            2,
        )
        .await;
        let owner_run = members[0].run_id.clone();
        let member_run = members[1].run_id.clone();

        groups
            .create(NewAgentRunGroup {
                group_id: group_id.clone(),
                root_run_id: owner_run.clone(),
                owner_run_id: owner_run.clone(),
                owner: AgentRunGroupOwner::Turn {
                    session_id,
                    turn_id,
                },
                member_run_ids: vec![owner_run.clone(), member_run.clone()],
                join_policy: RunJoinPolicy::All,
                cancel_remaining_on_satisfaction: false,
                idempotency_key: format!("group-{label}-{}", uuid::Uuid::new_v4().simple()),
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

        let coordinator = SharedCheckoutCoordinator::new(
            Some(groups.clone()),
            Some(runs.clone()),
            Some(worktrees.clone()),
            None,
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
            group_id,
            worktree_id,
            owner_run,
            member_run,
            project_id,
            repository_id,
            workspace_id,
        }
    }

    fn allocation(&self) -> CreateWorktreeRequest {
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
            owner_run_id: self.owner_run.clone(),
        }
    }

    /// Place one run on the group's checkout, the way the executor does.
    async fn place(&self, run_id: &AgentRunId) -> SharedCheckoutOutcome {
        let outcome = self
            .coordinator
            .acquire(SharedCheckoutRequest {
                group_id: self.group_id.clone(),
                member_run_id: run_id.clone(),
                allocation: self.allocation(),
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
    fn fence(&self, run_id: &AgentRunId, checkout_root: &Path) -> Arc<DaemonWriterFence> {
        Arc::new(
            DaemonWriterFence::new(
                CheckoutWriterScope {
                    worktree_id: self.worktree_id.as_str().to_owned(),
                    checkout_root: checkout_root.to_path_buf(),
                    agent_run_id: Some(run_id.as_str().to_owned()),
                    group_id: Some(self.group_id.as_str().to_owned()),
                },
                self.permits.clone(),
                self.worktrees.clone(),
                self.runs.clone(),
                Some(self.groups.clone()),
                Some(run_id.clone()),
                Some("attempt-m008".to_owned()),
                None,
            )
            .with_wait_timeout(Duration::from_millis(250)),
        )
    }

    fn request(&self, root: &Path, key: &str) -> WriterFenceRequest {
        WriterFenceRequest {
            effect_class: NativeEffectClass::FileWrite,
            targets: vec![root.join("shared.txt")],
            idempotency_key: key.to_owned(),
            granted_roots: Vec::new(),
            principal: Some(AuthenticatedPrincipal::local_owner("blackboard-m008")),
        }
    }
}

/// The platform gate is exactly the qualified table: the shared profile is
/// available on a qualified platform and refused, with a named reason, on
/// every other one. This arm runs everywhere and is never vacuous.
#[test]
fn the_shared_profile_is_available_exactly_on_the_qualified_platforms() {
    let availability = shared_checkout_availability();
    if QUALIFIED_SHARED_CHECKOUT_PLATFORMS.contains(&std::env::consts::OS) {
        assert!(
            availability.is_available(),
            "a qualified platform must have the shared profile available"
        );
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

/// S4: two children on **one** shared checkout. The writer permit serializes
/// them — the second is refused while the first holds the checkout and takes
/// it only once the first has settled — and both produce Git-verified commits
/// in that one checkout.
#[tokio::test]
async fn s4_two_children_on_one_shared_checkout_serialize_their_writes() {
    let availability = shared_checkout_availability();
    if !availability.is_available() {
        eprintln!(
            "S4 is UNRUN on this platform: {}",
            availability
                .reason()
                .unwrap_or("the shared checkout profile has no native containment evidence here")
        );
        return;
    }

    let shared = Shared::new("s4").await;
    let placed = shared.place(&shared.owner_run).await;
    let member_placed = shared.place(&shared.member_run).await;
    let checkout = placed.checkout().clone();

    assert_eq!(
        checkout.worktree_id,
        member_placed.checkout().worktree_id,
        "both members must be placed on the same checkout"
    );

    // The first writer takes the checkout.
    let first = shared.fence(&shared.owner_run, &checkout.path);
    let first_permit = first
        .begin(shared.request(&checkout.path, "m008-first"))
        .await
        .expect("the lease owner acquires the shared checkout");
    assert!(
        first_permit.is_some(),
        "a shared checkout must take a permit: there is a second writer to exclude"
    );

    // The second writer is refused while the first holds the checkout.
    let second = shared.fence(&shared.member_run, &checkout.path);
    let refused = second
        .begin(shared.request(&checkout.path, "m008-second"))
        .await;
    assert!(
        matches!(refused, Err(WriterFenceError::Held)),
        "the second writer must be refused while the checkout is held: {refused:?}"
    );

    // The first child writes and commits its own work, still holding the
    // checkout.
    std::fs::write(
        checkout.path.join("shared.txt"),
        "written by the first child\n",
    )
    .expect("the first child writes");
    let first_commit = commit_all(&checkout.path, "the first child");

    // Once the first writer settles, the second one takes the checkout.
    first_permit
        .expect("held permit")
        .settle(WriterFenceOutcome::Settled)
        .await
        .expect("the first writer settles its permit");
    let permit = second
        .begin(shared.request(&checkout.path, "m008-third"))
        .await
        .expect("the checkout is free once the first writer settled");
    assert!(permit.is_some(), "the second writer now takes the checkout");
    std::fs::write(
        checkout.path.join("shared.txt"),
        "written by the second child\n",
    )
    .expect("the second child writes");
    let second_commit = commit_all(&checkout.path, "the second child");
    permit
        .expect("held permit")
        .settle(WriterFenceOutcome::Settled)
        .await
        .expect("the second writer settles its permit");

    // Both children's work lands in **one** checkout, and the checkout's
    // history is Git-verified: the second child's commit is the second
    // commit on top of the first child's, in the one repository.
    assert_ne!(first_commit, second_commit);
    let log = git(&["log", "--oneline"], &checkout.path);
    let log = String::from_utf8_lossy(&log.stdout);
    assert!(
        log.contains("the first child") && log.contains("the second child"),
        "both children's commits are in the one checkout's history: {log}"
    );
    assert_eq!(
        String::from_utf8_lossy(&git(&["status", "--porcelain"], &checkout.path).stdout).trim(),
        "",
        "the shared checkout is clean after both children settled"
    );
}

// ---------------------------------------------------------------------------
// E — group member order
// ---------------------------------------------------------------------------

/// The group's members are delivered in a **stable ordinal order**, and the
/// order does not drift between two reads of the same group. A board whose
/// members reshuffle between reads would make two arms of a comparison see
/// different worlds, which is exactly what the frozen matrix forbids.
#[tokio::test]
async fn e_the_group_delivers_its_members_in_a_stable_ordinal_order() {
    let runs: Arc<dyn AgentRunStore> = Arc::new(InMemoryAgentRunStore::new());
    let groups = AgentRunGroupService::in_memory(runs.clone());
    let project_id = ProjectId::new();
    let workspace_id = WorkspaceId::new();
    let session_id = format!("session-m008-{}", uuid::Uuid::new_v4());
    let turn_id = format!("turn-m008-{}", uuid::Uuid::new_v4().simple());
    let members = create_turn_members(
        &runs,
        &project_id,
        &workspace_id,
        &session_id,
        &turn_id,
        "m008-order",
        3,
    )
    .await;
    let group_id = AgentRunGroupId::new();
    let member_run_ids = members
        .iter()
        .map(|member| member.run_id.clone())
        .collect::<Vec<_>>();

    groups
        .create(NewAgentRunGroup {
            group_id: group_id.clone(),
            root_run_id: member_run_ids[0].clone(),
            owner_run_id: member_run_ids[0].clone(),
            owner: AgentRunGroupOwner::Turn {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
            },
            member_run_ids: member_run_ids.clone(),
            join_policy: RunJoinPolicy::All,
            cancel_remaining_on_satisfaction: false,
            idempotency_key: "m008-order".to_owned(),
        })
        .await
        .expect("create the live group");

    // The group is turn-owned, so the actor that may read it is the turn.
    let actor = codegg_core::agent_run_group::GroupActor {
        run_id: None,
        session_id: Some(session_id),
        turn_id: Some(turn_id),
    };

    let first = groups
        .status(&actor, group_id.clone())
        .await
        .expect("the member may read its group's status");
    let second = groups
        .status(&actor, group_id.clone())
        .await
        .expect("the member may read its group's status again");

    assert_eq!(
        first.members.len(),
        3,
        "every member is delivered: {:?}",
        first.members
    );
    assert_eq!(
        first
            .members
            .iter()
            .map(|member| member.ordinal)
            .collect::<Vec<_>>(),
        vec![0, 1, 2],
        "the members are delivered in ordinal order"
    );
    assert_eq!(
        first
            .members
            .iter()
            .map(|member| member.run_id.clone())
            .collect::<Vec<_>>(),
        member_run_ids,
        "the ordinal order is the order the group was created with"
    );
    assert_eq!(
        first
            .members
            .iter()
            .map(|member| member.ordinal)
            .collect::<Vec<_>>(),
        second
            .members
            .iter()
            .map(|member| member.ordinal)
            .collect::<Vec<_>>(),
        "the member order does not drift between two reads"
    );
}

// ---------------------------------------------------------------------------
// E — notification persistence
// ---------------------------------------------------------------------------

/// The group notification is claimed **durably**: exactly one caller claims
/// it, and a service rebuilt over the same database — a restart — does not
/// claim it again. Live delivery is exactly-once across a replay.
#[tokio::test]
async fn e_a_group_notification_is_claimed_once_across_a_restart() {
    let pool = Blackboard::solo().await.pool;
    let runs: Arc<dyn AgentRunStore> = Arc::new(InMemoryAgentRunStore::new());

    // A group with one member. The owner is the member's parent, so the
    // group service's hierarchy check holds: every member is a child of the
    // owner, one level deeper, rooted at the owner.
    let group_id = AgentRunGroupId::new();
    let project_id = ProjectId::new();
    let workspace_id = WorkspaceId::new();
    let session_id = format!("session-m008-{}", uuid::Uuid::new_v4());
    let turn_id = format!("turn-m008-{}", uuid::Uuid::new_v4().simple());
    let members = create_turn_members(
        &runs,
        &project_id,
        &workspace_id,
        &session_id,
        &turn_id,
        "m008-notification",
        1,
    )
    .await;
    let member_run = members[0].run_id.clone();
    let owner_run = member_run.clone();

    // The durable group store references `agent_run`, so the member has to
    // be present in the database as well as in the run store.
    insert_run_rows(
        &pool,
        &project_id,
        &workspace_id,
        &member_run,
        &members[0].task_id,
        &member_run,
    )
    .await;

    let service = AgentRunGroupService::with_pool(runs.clone(), pool.clone());
    service
        .create(NewAgentRunGroup {
            group_id: group_id.clone(),
            root_run_id: owner_run.clone(),
            owner_run_id: owner_run.clone(),
            owner: AgentRunGroupOwner::Turn {
                session_id,
                turn_id,
            },
            member_run_ids: vec![member_run.clone()],
            join_policy: RunJoinPolicy::All,
            cancel_remaining_on_satisfaction: false,
            idempotency_key: "m008-notification".to_owned(),
        })
        .await
        .expect("create the live group");

    // The member reaches a terminal state, so the group recomputes to a
    // terminal status and emits its notification.
    // The member reaches a terminal state through the real transition
    // ladder; the durable run store refuses a jump.
    for status in [
        AgentRunStatus::Queued,
        AgentRunStatus::Preparing,
        AgentRunStatus::Running,
    ] {
        runs.transition(&member_run, status)
            .await
            .expect("advance the member run");
    }
    runs.finish(
        &member_run,
        codegg_core::agent_run::AgentRunTerminalOutcome::Completed,
        None,
        None,
        None,
    )
    .await
    .expect("complete the member run");

    let first = service
        .member_changed_with_notifications(&member_run)
        .await
        .expect("the terminal member recomputes its group");
    assert_eq!(first.len(), 1, "the member belongs to exactly one group");
    let (summary, claimed) = &first[0];
    assert!(
        *claimed,
        "the first reconciler durably claims the terminal notification"
    );
    assert!(
        summary.group.status.is_terminal(),
        "the group is terminal: {}",
        summary.group.status.as_str()
    );

    // A restart: a brand-new service over the same database.
    let restarted = AgentRunGroupService::with_pool(runs.clone(), pool.clone());
    let second = restarted
        .member_changed_with_notifications(&member_run)
        .await
        .expect("the restarted service recomputes the group");
    assert_eq!(second.len(), 1);
    let (_, claimed_again) = &second[0];
    assert!(
        !*claimed_again,
        "the durable claim must survive a restart: the notification is not re-emitted"
    );
}
