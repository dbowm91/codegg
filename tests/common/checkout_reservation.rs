#![allow(dead_code)]

//! The C004 shared-checkout fixture.
//!
//! One reserved checkout: a managed worktree leased by a run that belongs
//! to a live agent-run group with a second member. The construction is
//! deliberately direct — nothing in production places two runs on one
//! checkout yet — so the mediation can be proved before any product path
//! is allowed to depend on it.

use std::str::FromStr;
use std::sync::Arc;

use codegg::core::daemon::CoreDaemon;
use codegg::core::runtime_deps::CoreRuntimeDeps;
use codegg_core::agent_run::{
    AgentRunBudget, AgentRunStore, InMemoryAgentRunStore, NewAgentRun, NewAgentTask,
};
use codegg_core::agent_run_group::{
    AgentRunGroupOwner, AgentRunGroupService, NewAgentRunGroup, RunJoinPolicy,
};
use codegg_core::identity::{AgentRunGroupId, AgentRunId, AgentTaskId, RepositoryId, WorktreeId};
use codegg_core::project_catalog::{ProjectCatalog, RegisterLocalProject};
use codegg_core::workspace_services::WorkspaceLockTable;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

/// A temporary directory that removes itself when the test ends.
pub struct TempDir(std::path::PathBuf);

impl TempDir {
    pub fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "codegg-c004-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A fresh in-memory database with the full migration set applied.
pub async fn isolated_pool() -> SqlitePool {
    let name = format!(
        "checkout_reservation_c004_{}",
        uuid::Uuid::new_v4().simple()
    );
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

/// A real Git checkout, so the fixture is a checkout and not a directory
/// that merely looks like one.
pub fn init_git_checkout(root: &std::path::Path) {
    let output = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(root)
        .output()
        .expect("git is available");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// One reserved checkout, plus the daemon that mediates writes into it.
pub struct ReservedCheckout {
    pub daemon: Arc<CoreDaemon>,
    pub pool: SqlitePool,
    pub checkout: TempDir,
    pub project_id: String,
    pub workspace_id: String,
    pub worktree_id: WorktreeId,
    pub group_id: AgentRunGroupId,
    pub groups: Arc<AgentRunGroupService>,
    pub runs: Arc<dyn AgentRunStore>,
    pub owner_run: AgentRunId,
    pub member_run: AgentRunId,
    pub session_id: String,
    pub turn_id: String,
}

impl ReservedCheckout {
    /// Build the fixture.
    pub async fn build() -> Self {
        let pool = isolated_pool().await;
        let checkout = TempDir::new("checkout");
        init_git_checkout(checkout.path());

        let repository_id = RepositoryId::new();
        let worktree_id = WorktreeId::new();
        let owner_run = AgentRunId::new();
        let member_run = AgentRunId::new();
        let group_id = AgentRunGroupId::new();
        let session_id = format!("session-{}", uuid::Uuid::new_v4().simple());
        let turn_id = format!("turn-{}", uuid::Uuid::new_v4().simple());

        let worktrees = codegg_core::worktree_service::WorktreeService::sqlite(
            pool.clone(),
            std::path::PathBuf::from("/tmp/codegg-c004-managed-worktrees"),
            Arc::new(WorkspaceLockTable::new()),
        );
        let runs: Arc<dyn AgentRunStore> = Arc::new(InMemoryAgentRunStore::new());
        let groups = AgentRunGroupService::in_memory(runs.clone());

        let mut deps = CoreRuntimeDeps::new(Some(pool.clone()), None, None);
        deps.agent_run_store = runs.clone();
        deps.run_group_service = groups.clone();
        deps.worktree_service = Some(worktrees.clone());
        deps.run_control.set_group_service_sync(groups.clone());
        let daemon = Arc::new(CoreDaemon::with_deps(deps));

        // The workspace is registered first: the registry mints the
        // workspace identity, and the managed worktree row has to name it.
        let workspace = daemon
            .workspaces
            .get_or_register(checkout.path())
            .await
            .expect("register the checkout as a workspace");
        let workspace_id = workspace.id.clone();

        // The project the request names has to resolve to the reserved
        // checkout, or the mediation would never look at it. The catalog
        // mints the project identity, so it is registered before the
        // managed worktree row that has to name it.
        let project = ProjectCatalog::new(pool.clone())
            .register_local_project(
                RegisterLocalProject {
                    display_name: "c004 reserved checkout".to_owned(),
                    description: None,
                    tags: Vec::new(),
                    primary_repository_id: None,
                },
                &workspace.id,
                "c004-qualification",
            )
            .await
            .expect("register the project");
        let project_id = project.project_id.clone();

        // The managed checkout record. It is a *managed* worktree rooted at
        // the temporary checkout, which is what makes the checkout a shared
        // reservation candidate at all.
        let now = chrono::Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT INTO managed_worktree (worktree_id, project_id, repository_id, workspace_id, \
             repository_root, path, branch, base_commit, managed, state, health, lease_generation, \
             owner_run_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'codegg/worktree/c004', 'abc', 1, 'ready', 'clean', 0, NULL, ?, ?)",
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

        // Two real delegated runs under one turn, so a turn-owned group can
        // cover both of them.
        for (index, run_id) in [owner_run.clone(), member_run.clone()]
            .into_iter()
            .enumerate()
        {
            let task = NewAgentTask {
                task_id: AgentTaskId::new(),
                parent_task_id: None,
                originating_session_id: session_id.clone(),
                originating_turn_id: Some(turn_id.clone()),
                project_id: project_id.clone(),
                repository_id: Some(repository_id.clone()),
                workspace_id: workspace_id.clone(),
                requested_agent: "build".to_owned(),
                delegation_key: format!("delegation-{index}-{}", uuid::Uuid::new_v4().simple()),
                request_fingerprint: format!("fingerprint-{index}"),
                description: "c004 reserved checkout qualification".to_owned(),
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

        worktrees
            .acquire_lease(&worktree_id, &owner_run)
            .await
            .expect("acquire the checkout's lease");

        Self {
            daemon,
            pool,
            checkout,
            project_id: project_id.to_string(),
            workspace_id: workspace_id.to_string(),
            worktree_id,
            group_id,
            groups,
            runs,
            owner_run,
            member_run,
            session_id,
            turn_id,
        }
    }

    /// Register an ordinary, unreserved workspace beside the reserved one.
    pub async fn ordinary_workspace(&self) -> (TempDir, String, String) {
        let ordinary = TempDir::new("ordinary");
        // A Git checkout too, so the ordinary case differs from the
        // reserved one in exactly one respect: nothing reserves it.
        init_git_checkout(ordinary.path());
        let workspace = self
            .daemon
            .workspaces
            .get_or_register(ordinary.path())
            .await
            .expect("register the ordinary workspace");
        let project = ProjectCatalog::new(self.pool.clone())
            .register_local_project(
                RegisterLocalProject {
                    display_name: "c004 ordinary checkout".to_owned(),
                    description: None,
                    tags: Vec::new(),
                    primary_repository_id: None,
                },
                &workspace.id,
                "c004-qualification",
            )
            .await
            .expect("register the ordinary project");
        (
            ordinary,
            project.project_id.to_string(),
            workspace.id.to_string(),
        )
    }

    /// Send one request through the daemon's dispatch.
    pub async fn call(
        &self,
        request: codegg::protocol::core::CoreRequest,
    ) -> codegg::protocol::core::CoreResponse {
        let request_id = format!("req-{}", uuid::Uuid::new_v4().simple());
        Box::pin(
            self.daemon
                .handle_request(codegg::core::new_request(request_id, request)),
        )
        .await
        .expect("daemon request succeeds at the transport layer")
    }

    /// Assert the response is the typed checkout conflict.
    pub fn assert_conflict(&self, response: &codegg::protocol::core::CoreResponse) {
        use codegg::protocol::core::CoreResponse;
        match response {
            CoreResponse::Error { code, message } => {
                assert_eq!(
                    code, "checkout_reserved",
                    "expected the typed checkout conflict, got {response:?}"
                );
                assert!(
                    message.contains("reserved"),
                    "the conflict must name the reservation: {message}"
                );
                assert!(
                    message.contains("retry"),
                    "the conflict must offer a recovery path: {message}"
                );
            }
            other => panic!("expected the checkout conflict, got {other:?}"),
        }
    }
}
