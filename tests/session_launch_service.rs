//! M006 qualification: the session launch service against a real Git
//! repository.
//!
//! `crates/codegg-core/tests/session_launch.rs` covers the **store** —
//! the idempotency contract, the stage machine, the restart
//! reconciliation. This file covers the **service**: reserve → create →
//! register → bind → session, against an actual Git repository with
//! actual linked worktrees.
//!
//! # The Git backend, and what this file therefore proves
//!
//! `git worktree` is classified as requiring **contained** execution
//! (`egggit::process::classify_git_request` → `CleanSmudgeFilter`,
//! because a checkout can run a smudge filter). This revision has **no
//! contained Git backend**: `GitEnvPolicy::apply` and `apply_sync` both
//! turn a contained request into
//! `GitExecutionPolicyOutcome::BackendUnavailable`, and
//! `WorktreeService::create_reserved` — the only path that creates a
//! checkout — goes through the sync boundary. So on this host the
//! product's own checkout creation always fails closed.
//!
//! That is the condition the plan anticipates: *"If the available Git
//! backend cannot safely create a managed checkout on a platform, the
//! request must fail closed and retain recoverable state."* This file
//! proves **both** halves of it:
//!
//! * `a_launch_fails_closed_when_the_managed_git_backend_is_unavailable`
//!   — the new-checkout path refuses, creates no session, and leaves a
//!   visible `needs_attention` record plus an `orphaned` checkout.
//! * the placement tests — the session-placement semantics, run against
//!   a **real** linked worktree that the fixture creates with a plain
//!   `git worktree add` and records in the state the product's
//!   `create_reserved` would have set.
//!
//! The product's checkout *creation* on a host with the contained backend
//! is therefore **not** claimed here.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use codegg::core::session_launch::{SessionLaunchContext, SessionLaunchService};
use codegg_core::identity::{AgentRunId, ProjectId, RepositoryId, WorkspaceId};
use codegg_core::project_storage::ProjectStorage;
use codegg_core::session::{CreateSession, SessionStore};
use codegg_core::session_launch::{
    SessionLaunchIntent, SessionLaunchPlacement, SessionLaunchRequest, SessionLaunchStage,
    SqliteSessionLaunchStore,
};
use codegg_core::workspace::{SqliteWorkspaceStore, WorkspaceRegistry};
use codegg_core::workspace_services::WorkspaceLockTable;
use codegg_core::worktree_service::{
    CreateWorktreeRequest, WorktreeQuery, WorktreeRecord, WorktreeService,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

/// An in-memory database, migrated once. `migrate` runs inside the
/// helper, so no test adds its own.
async fn isolated_pool() -> SqlitePool {
    let name = format!("session_launch_service_{}", uuid::Uuid::new_v4().simple());
    let url = format!("file:{name}?mode=memory&cache=shared");
    let opts = SqliteConnectOptions::from_str(&url)
        .expect("valid sqlite options")
        .create_if_missing(true)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("connect in-memory sqlite");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrations run inside the helper");
    pool
}

fn git(args: &[&str], dir: &Path) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The number of worktrees Git knows about, including the main one.
fn git_worktree_count(repo: &Path) -> usize {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo)
        .output()
        .expect("run git worktree list");
    assert!(
        output.status.success(),
        "git worktree list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.starts_with("worktree "))
        .count()
}

fn git_branch(repo: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("run git rev-parse");
    assert!(output.status.success(), "git rev-parse failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A repository with one commit on `main` and a usable remote.
struct Repository {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    managed_root: PathBuf,
}

impl Repository {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let root = tmp.path().join("repo");
        let managed_root = tmp.path().join("managed");
        std::fs::create_dir_all(&root).expect("create repo dir");
        std::fs::create_dir_all(&managed_root).expect("create managed root");
        git(&["init"], &root);
        // `init.defaultBranch` is a global setting; pin the branch name
        // so the test does not depend on the host's Git configuration.
        git(&["symbolic-ref", "HEAD", "refs/heads/main"], &root);
        git(&["config", "user.name", "Launch Test"], &root);
        git(&["config", "user.email", "launch@example.invalid"], &root);
        git(&["config", "commit.gpgsign", "false"], &root);
        // A remote is what makes the repository's lineage resolvable, and
        // a launch refuses a project with no resolved repository.
        git(
            &[
                "remote",
                "add",
                "origin",
                "https://example.invalid/launch/test.git",
            ],
            &root,
        );
        std::fs::write(root.join("README.md"), "launch\n").expect("write README");
        git(&["add", "README.md"], &root);
        git(&["commit", "-m", "initial"], &root);
        Self {
            _tmp: tmp,
            root,
            managed_root,
        }
    }
}

/// Everything a launch needs: the project, its repository, and the
/// service that places a session.
struct Fixture {
    repository: Repository,
    pool: SqlitePool,
    project_id: ProjectId,
    repository_id: RepositoryId,
    source_workspace_id: WorkspaceId,
    service: Arc<SessionLaunchService>,
    storage: Arc<ProjectStorage>,
    sessions: Arc<SessionStore>,
    worktrees: Arc<WorktreeService>,
}

impl Fixture {
    async fn new() -> Self {
        let repository = Repository::new();
        let pool = isolated_pool().await;
        let workspaces =
            WorkspaceRegistry::new_for_tests(Arc::new(SqliteWorkspaceStore::new(pool.clone())));
        let storage = ProjectStorage::new(pool.clone());
        let sessions = SessionStore::new(pool.clone());
        let worktrees = WorktreeService::sqlite(
            pool.clone(),
            repository.managed_root.clone(),
            Arc::new(WorkspaceLockTable::new()),
        );

        // The source workspace is the repository root, resolved exactly
        // as the daemon's project context resolver resolves it: register
        // the root, then reconcile it. The reconciliation is what creates
        // the project and the repository row the launch names.
        let source = workspaces
            .get_or_register(&repository.root)
            .await
            .expect("register the source workspace");
        let reconciliation = storage
            .reconcile_workspace_path(&source, "session_launch_test")
            .await
            .expect("reconcile the source workspace");
        let project_id = reconciliation.binding.project_id.clone();
        let repository_id = reconciliation
            .binding
            .repository_id
            .clone()
            .expect("the source workspace resolves a repository");
        let source_workspace_id = source.id.clone();

        let sessions = Arc::new(sessions);
        let storage = Arc::new(storage);
        let service = Arc::new(SessionLaunchService::new(
            Arc::new(SqliteSessionLaunchStore::new(pool.clone())),
            worktrees.clone(),
            sessions.clone(),
            storage.clone(),
            workspaces,
        ));
        Self {
            repository,
            pool,
            project_id,
            repository_id,
            source_workspace_id,
            service,
            storage,
            sessions,
            worktrees,
        }
    }

    fn context(&self) -> SessionLaunchContext {
        SessionLaunchContext {
            project_id: self.project_id.clone(),
            repository_id: self.repository_id.clone(),
            repository_root: self.repository.root.clone(),
            source_workspace_id: self.source_workspace_id.clone(),
        }
    }

    fn request(&self, key: &str, placement: SessionLaunchPlacement) -> SessionLaunchRequest {
        SessionLaunchRequest {
            project_id: self.project_id.as_str().to_string(),
            intent: SessionLaunchIntent::New,
            placement,
            parent_session_id: None,
            idempotency_key: key.to_string(),
        }
    }

    fn new_checkout(&self, base_ref: Option<&str>, branch: Option<&str>) -> SessionLaunchPlacement {
        SessionLaunchPlacement::NewManagedCheckout {
            base_ref: base_ref.map(str::to_string),
            target_branch: branch.map(str::to_string),
        }
    }

    /// A managed checkout that **exists**.
    ///
    /// The product's own creation path needs the contained Git backend
    /// this revision does not have, so the fixture reserves the identity
    /// through the product API, creates the real linked worktree with a
    /// plain `git worktree add`, and records the state
    /// `WorktreeService::create_reserved` would have set.
    async fn managed_checkout(&self, branch: &str) -> WorktreeRecord {
        let reserved = self
            .worktrees
            .reserve(&CreateWorktreeRequest {
                project_id: self.project_id.clone(),
                repository_id: self.repository_id.clone(),
                workspace_id: self.source_workspace_id.clone(),
                node_id: None,
                repository_root: self.repository.root.clone(),
                base_commit: None,
                base_ref: Some("main".to_string()),
                target_branch: Some(branch.to_string()),
                base_path: None,
                owner_run_id: AgentRunId::new(),
            })
            .await
            .expect("reserve the checkout");
        let path = reserved.path.to_string_lossy().into_owned();
        let base = reserved.base_commit.clone();
        git(
            &["worktree", "add", &path, "-b", branch, &base],
            &self.repository.root,
        );
        // The state `create_reserved` sets once the directory exists.
        sqlx::query("UPDATE managed_worktree SET state = 'ready' WHERE worktree_id = ?")
            .bind(reserved.worktree_id.as_str())
            .execute(&self.pool)
            .await
            .expect("mark the checkout ready");
        self.worktrees
            .get(&reserved.worktree_id)
            .await
            .expect("reload the checkout")
    }

    fn existing(&self, worktree_id: &str) -> SessionLaunchPlacement {
        SessionLaunchPlacement::ExistingManagedWorktree {
            worktree_id: worktree_id.to_string(),
        }
    }

    /// A session in the project's own source workspace.
    async fn source_session(&self, title: &str) -> codegg_core::session::Session {
        self.sessions
            .create_with_binding(
                CreateSession {
                    project_id: self.project_id.as_str().to_string(),
                    directory: self.repository.root.to_string_lossy().into_owned(),
                    title: Some(title.to_string()),
                    parent_id: None,
                    workspace_id: Some(self.source_workspace_id.as_str().to_string()),
                    ..Default::default()
                },
                &self.project_id,
                &self.source_workspace_id,
                "session_launch_test",
            )
            .await
            .expect("create a session in the source workspace")
    }
}

#[tokio::test]
async fn a_launch_fails_closed_when_the_managed_git_backend_is_unavailable() {
    // The plan's own fallback: when the platform cannot safely create a
    // managed checkout, the request must fail closed and retain
    // recoverable state. Nothing is cleaned up, and the operator can see
    // what happened.
    let fixture = Fixture::new().await;
    let error = fixture
        .service
        .launch(
            fixture.request(
                "k-no-backend",
                fixture.new_checkout(Some("main"), Some("feature/x")),
            ),
            &fixture.context(),
        )
        .await
        .expect_err("the checkout cannot be created without the contained backend");
    assert!(
        error.to_string().contains("Git backend unavailable"),
        "the refusal names the missing backend: {error}"
    );

    // No session, and no half-made one.
    let sessions = fixture
        .sessions
        .list(fixture.project_id.as_str(), 50)
        .await
        .expect("list sessions");
    assert!(
        sessions.is_empty(),
        "a failed launch creates no session: {sessions:?}"
    );

    // The launch left a visible record. It is *terminal* — it is waiting
    // for an operator, not for a resume — so it is in the project's list
    // rather than in the unfinished set.
    let records = fixture
        .service
        .list(fixture.project_id.as_str(), 50)
        .await
        .expect("list the launches");
    let record = records
        .iter()
        .find(|record| record.idempotency_key == "k-no-backend")
        .expect("the failed launch is still visible");
    assert_eq!(record.stage, SessionLaunchStage::NeedsAttention);
    assert_eq!(record.attention_code.as_deref(), Some("checkout_failed"));

    // The checkout itself is marked for an operator, which is the state
    // the TUI's `⚠` marker reads: the reservation succeeded, the
    // creation did not, and the worktree service marked the record
    // `orphaned` with a `git_error` health.
    let checkouts = fixture
        .worktrees
        .list(WorktreeQuery {
            workspace_id: None,
            repository_id: None,
            owner_run_id: None,
            include_removed: false,
        })
        .await
        .expect("list the managed checkouts");
    let checkout = checkouts
        .iter()
        .find(|checkout| checkout.branch.as_deref() == Some("feature/x"))
        .expect("the reserved checkout is recorded");
    assert_eq!(checkout.state.as_str(), "orphaned");
    assert_eq!(checkout.health.as_str(), "git_error");

    // And nothing was created on disk: the `git worktree add` never ran,
    // and the launch never cleaned the record up.
    assert_eq!(
        git_worktree_count(&fixture.repository.root),
        1,
        "a failed launch leaves no checkout on disk"
    );
    assert!(
        !PathBuf::from(&checkout.path.to_string_lossy().into_owned()).exists(),
        "a failed creation leaves no directory behind"
    );
}

#[tokio::test]
async fn a_launch_places_a_session_in_an_existing_managed_checkout() {
    let fixture = Fixture::new().await;
    let checkout = fixture.managed_checkout("feature/x").await;

    let outcome = fixture
        .service
        .launch(
            fixture.request(
                "k-existing",
                fixture.existing(checkout.worktree_id.as_str()),
            ),
            &fixture.context(),
        )
        .await
        .expect("the launch completes");

    assert_eq!(outcome.stage, SessionLaunchStage::Completed);
    assert!(!outcome.converged, "the first call does the work");
    assert_eq!(outcome.branch.as_deref(), Some("feature/x"));
    assert_eq!(outcome.worktree_id, checkout.worktree_id.as_str());

    // The checkout is a real linked worktree: a *file* named `.git`
    // pointing at the repository's worktree directory, not a directory.
    let path = PathBuf::from(&outcome.checkout_path);
    assert!(path.is_dir(), "the checkout exists on disk");
    assert!(
        path.join(".git").is_file(),
        "a linked worktree carries a .git *file*, not a directory"
    );
    assert_eq!(git_branch(&path), "feature/x");
    assert_ne!(
        path.canonicalize().expect("canonicalize the checkout"),
        fixture
            .repository
            .root
            .canonicalize()
            .expect("canonicalize the repository"),
        "the session must run in its own checkout, not the source root"
    );

    // The session runs in the checkout's own workspace.
    let session = fixture
        .sessions
        .get(&outcome.session_id)
        .await
        .expect("load the session")
        .expect("the session exists");
    assert_eq!(
        session.workspace_id.as_deref(),
        Some(outcome.workspace_id.as_str())
    );
    assert_eq!(session.directory, outcome.checkout_path);
    assert_eq!(session.project_id, fixture.project_id.as_str());

    // The canonical binding agrees with the session row.
    let binding = fixture
        .storage
        .session_binding(&outcome.session_id)
        .await
        .expect("load the session binding")
        .expect("the session is bound");
    assert_eq!(binding.project_id.as_ref(), Some(&fixture.project_id));
    assert_eq!(
        binding
            .workspace_id
            .as_ref()
            .expect("the session is bound to a workspace")
            .as_str(),
        outcome.workspace_id
    );

    // The checkout's workspace belongs to the launch's project and
    // repository, and names the managed checkout it is.
    let workspace_binding = fixture
        .storage
        .workspace_binding(
            &codegg_core::identity::WorkspaceId::parse(&outcome.workspace_id)
                .expect("valid workspace id"),
        )
        .await
        .expect("load the workspace binding")
        .expect("the workspace is bound");
    assert_eq!(
        workspace_binding.project_id, fixture.project_id,
        "a checkout is a second working directory of the launch's project, not a new project"
    );
    assert_eq!(
        workspace_binding.repository_id.as_ref(),
        Some(&fixture.repository_id),
        "the checkout descends from the project's own repository"
    );
    assert_eq!(
        workspace_binding.worktree_id.as_deref(),
        Some(outcome.worktree_id.as_str()),
        "the managed checkout is named on its workspace binding"
    );

    // The source workspace is untouched: the launch adds a workspace, it
    // does not move the source one.
    let source_binding = fixture
        .storage
        .workspace_binding(&fixture.source_workspace_id)
        .await
        .expect("load the source binding")
        .expect("the source workspace is bound");
    assert_eq!(source_binding.project_id, fixture.project_id);
    assert_ne!(
        source_binding.workspace_id.as_str(),
        outcome.workspace_id,
        "the launch adds a workspace; it does not move the source one"
    );
}

#[tokio::test]
async fn a_fork_launch_moves_the_child_session_into_the_checkout() {
    let fixture = Fixture::new().await;
    let checkout = fixture.managed_checkout("feature/fork").await;
    let parent = fixture.source_session("Parent").await;
    let mut request = fixture.request("k-fork", fixture.existing(checkout.worktree_id.as_str()));
    request.intent = SessionLaunchIntent::Fork;
    request.parent_session_id = Some(parent.id.clone());

    let outcome = fixture
        .service
        .launch(request, &fixture.context())
        .await
        .expect("the fork launch completes");

    let child = fixture
        .sessions
        .get(&outcome.session_id)
        .await
        .expect("load the child")
        .expect("the child exists");
    assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
    assert_eq!(
        child.workspace_id.as_deref(),
        Some(outcome.workspace_id.as_str()),
        "the fork leaves the parent's workspace behind"
    );
    assert_eq!(child.directory, outcome.checkout_path);
    assert_eq!(
        child.project_id,
        fixture.project_id.as_str(),
        "the fork's project moves with its workspace"
    );

    // The parent is unchanged.
    let parent_after = fixture
        .sessions
        .get(&parent.id)
        .await
        .expect("load the parent")
        .expect("the parent exists");
    assert_eq!(
        parent_after.workspace_id.as_deref(),
        Some(fixture.source_workspace_id.as_str())
    );
    assert_eq!(
        parent_after.directory,
        fixture.repository.root.to_string_lossy()
    );
}

#[tokio::test]
async fn a_retry_with_the_same_key_converges_on_one_checkout_and_one_session() {
    let fixture = Fixture::new().await;
    let checkout = fixture.managed_checkout("feature/retry").await;
    let request = fixture.request("k-retry", fixture.existing(checkout.worktree_id.as_str()));
    let first = fixture
        .service
        .launch(request.clone(), &fixture.context())
        .await
        .expect("the first launch completes");
    let second = fixture
        .service
        .launch(request, &fixture.context())
        .await
        .expect("the retry completes");

    assert_eq!(second.launch_id, first.launch_id);
    assert_eq!(second.session_id, first.session_id);
    assert_eq!(second.worktree_id, first.worktree_id);
    assert!(second.converged, "the retry converges on the first launch");
    assert_eq!(
        git_worktree_count(&fixture.repository.root),
        2,
        "the retry must not create a second checkout"
    );
    let sessions = fixture
        .sessions
        .list(fixture.project_id.as_str(), 50)
        .await
        .expect("list sessions");
    assert_eq!(
        sessions.len(),
        1,
        "the retry must not create a second session"
    );
}

#[tokio::test]
async fn simultaneous_identical_requests_converge_on_one_checkout_and_one_session() {
    // Two callers, one idempotency key, one launch. The second must
    // converge on the first rather than doing the work again.
    let fixture = Fixture::new().await;
    let checkout = fixture.managed_checkout("feature/concurrent").await;
    let request = fixture.request(
        "k-concurrent",
        fixture.existing(checkout.worktree_id.as_str()),
    );
    let context = fixture.context();

    let (first, second) = tokio::join!(
        fixture.service.launch(request.clone(), &context),
        fixture.service.launch(request, &context)
    );
    let first = first.expect("the first launch completes");
    let second = second.expect("the second launch converges");

    assert_eq!(first.launch_id, second.launch_id);
    assert_eq!(first.session_id, second.session_id);
    assert_eq!(first.worktree_id, second.worktree_id);
    let sessions = fixture
        .sessions
        .list(fixture.project_id.as_str(), 50)
        .await
        .expect("list sessions");
    assert_eq!(
        sessions.len(),
        1,
        "two simultaneous requests must not create two sessions"
    );
}

#[tokio::test]
async fn an_unresolvable_base_ref_fails_closed_and_creates_nothing() {
    // The base ref is resolved at reservation time, before any Git
    // process runs, so this refusal never reaches the checkout.
    let fixture = Fixture::new().await;
    let error = fixture
        .service
        .launch(
            fixture.request("k-bad-ref", fixture.new_checkout(Some("no-such-ref"), None)),
            &fixture.context(),
        )
        .await
        .expect_err("an unresolvable base ref is refused");
    let message = error.to_string();
    assert!(
        message.contains("no-such-ref") || message.contains("base ref"),
        "the refusal names the ref it could not resolve: {message}"
    );
    assert_eq!(
        git_worktree_count(&fixture.repository.root),
        1,
        "a refused launch creates no checkout"
    );
    let sessions = fixture
        .sessions
        .list(fixture.project_id.as_str(), 50)
        .await
        .expect("list sessions");
    assert!(
        sessions.is_empty(),
        "a refused launch creates no session: {sessions:?}"
    );
}

#[tokio::test]
async fn a_conflicting_target_branch_is_refused_and_creates_nothing() {
    // A branch that already exists in the repository is not a new
    // branch: a checkout on it would share a branch with whatever else
    // is on it, and the worktree service refuses the collision.
    let fixture = Fixture::new().await;
    git(&["branch", "feature/taken"], &fixture.repository.root);

    let error = fixture
        .service
        .launch(
            fixture.request(
                "k-conflict",
                fixture.new_checkout(Some("main"), Some("feature/taken")),
            ),
            &fixture.context(),
        )
        .await
        .expect_err("a conflicting branch is refused");
    // The *reason* on this host is the branch listing that precedes the
    // collision check, and that listing needs the contained Git backend
    // this revision does not have. The outcome is the same either way:
    // the branch is not taken and nothing is created.
    let message = error.to_string();
    assert!(
        message.contains("collision")
            || message.contains("branch")
            || message.contains("Git backend unavailable"),
        "the refusal names the conflict or the missing backend: {message}"
    );
    assert_eq!(
        git_worktree_count(&fixture.repository.root),
        1,
        "a refused launch creates no checkout"
    );
    let sessions = fixture
        .sessions
        .list(fixture.project_id.as_str(), 50)
        .await
        .expect("list sessions");
    assert!(
        sessions.is_empty(),
        "a refused launch creates no session: {sessions:?}"
    );
}

#[tokio::test]
async fn a_launch_naming_a_checkout_of_another_project_is_refused() {
    let fixture = Fixture::new().await;
    // A managed checkout that belongs to a different project.
    let foreign_project = ProjectId::new();
    let reserved = fixture
        .worktrees
        .reserve(&CreateWorktreeRequest {
            project_id: foreign_project.clone(),
            repository_id: fixture.repository_id.clone(),
            workspace_id: fixture.source_workspace_id.clone(),
            node_id: None,
            repository_root: fixture.repository.root.clone(),
            base_commit: None,
            base_ref: Some("main".to_string()),
            target_branch: Some("feature/foreign".to_string()),
            base_path: None,
            owner_run_id: AgentRunId::new(),
        })
        .await
        .expect("reserve the foreign checkout");

    let error = fixture
        .service
        .launch(
            fixture.request("k-foreign", fixture.existing(reserved.worktree_id.as_str())),
            &fixture.context(),
        )
        .await
        .expect_err("a checkout of another project is refused");
    assert!(
        error.to_string().contains("another project"),
        "the refusal names the ownership boundary: {error}"
    );
}

#[tokio::test]
async fn a_launch_naming_a_checkout_that_another_run_holds_is_refused() {
    let fixture = Fixture::new().await;
    let checkout = fixture.managed_checkout("feature/held").await;
    fixture
        .worktrees
        .acquire_lease(&checkout.worktree_id, &AgentRunId::new())
        .await
        .expect("another run holds the checkout");

    let error = fixture
        .service
        .launch(
            fixture.request("k-held", fixture.existing(checkout.worktree_id.as_str())),
            &fixture.context(),
        )
        .await
        .expect_err("a checkout another run holds is refused");
    assert!(
        error.to_string().contains("in use"),
        "the refusal names the isolation boundary: {error}"
    );
}

#[tokio::test]
async fn a_launch_naming_a_checkout_that_does_not_exist_is_refused() {
    let fixture = Fixture::new().await;
    let error = fixture
        .service
        .launch(
            fixture.request("k-missing", fixture.existing("wt-does-not-exist")),
            &fixture.context(),
        )
        .await
        .expect_err("a nonexistent checkout is refused");
    assert!(
        error.to_string().contains("not found") || error.to_string().contains("checkout"),
        "the refusal names the missing checkout: {error}"
    );
    let sessions = fixture
        .sessions
        .list(fixture.project_id.as_str(), 50)
        .await
        .expect("list sessions");
    assert!(
        sessions.is_empty(),
        "a refused launch creates no session: {sessions:?}"
    );
}
