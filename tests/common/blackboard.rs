#![allow(dead_code)]

//! The shared blackboard fixture for the M008 comparison suites.
//!
//! Both `tests/blackboard_trajectory_m008.rs` (the functional arms) and
//! `tests/blackboard_economics_m008.rs` (the measured arms) need the same
//! three things: a real Git repository with real linked worktrees, a
//! migrated SQLite database with a live group and member runs, and the
//! durable claim store over that database. This module is that fixture, so
//! the two suites measure and assert against the **same** state — a
//! comparison whose arms used different fixtures would not be a comparison.
//!
//! The fixture models the real group hierarchy, because the group service
//! enforces it: the owner is the parent, every member is a child of the
//! owner one level deeper, and the whole group is rooted at the owner. A
//! top-level delegated run has depth 1, so the owner has depth 1 and its
//! members have depth 2.

use std::path::Path;
use std::process::Command;
use std::str::FromStr;
use std::sync::Arc;

use codegg_core::agent_run::{AgentRunBudget, AgentRunStore, NewAgentRun, NewAgentTask};
use codegg_core::identity::{AgentRunGroupId, AgentRunId, AgentTaskId, ProjectId, WorkspaceId};
use codegg_core::workstream_claim::{
    ClaimActor, ClaimTarget, NewWorkstreamClaim, WorkstreamClaimStore,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

// ---------------------------------------------------------------------------
// Git fixture
// ---------------------------------------------------------------------------

/// Run a git command in `cwd` and fail loudly if it does not succeed.
pub fn git(args: &[&str], cwd: &Path) -> std::process::Output {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run a git fixture command");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// The trimmed stdout of a git command.
pub fn git_stdout(args: &[&str], cwd: &Path) -> String {
    String::from_utf8_lossy(&git(args, cwd).stdout)
        .trim()
        .to_owned()
}

/// A real Git repository with one commit, so a checkout is a checkout and not
/// a directory that merely looks like one.
pub fn init_repository(root: &Path) {
    std::fs::create_dir_all(root).expect("create the repository directory");
    git(&["init", "--quiet", "--initial-branch=main"], root);
    git(&["config", "user.email", "test@example.com"], root);
    git(&["config", "user.name", "Test"], root);
    std::fs::write(root.join("README"), "base\n").expect("write the base file");
    git(&["add", "."], root);
    git(&["commit", "--quiet", "-m", "base"], root);
}

/// A linked worktree, so two children really do have two separate roots.
pub fn add_worktree(repository: &Path, path: &Path, branch: &str) {
    git(
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            branch,
            path.to_str().expect("a utf-8 worktree path"),
            "main",
        ],
        repository,
    );
}

/// Commit everything in a checkout and return the new commit id.
pub fn commit_all(root: &Path, message: &str) -> String {
    git(&["add", "-A"], root);
    git(&["commit", "--quiet", "-m", message], root);
    git_stdout(&["rev-parse", "HEAD"], root)
}

// ---------------------------------------------------------------------------
// The blackboard fixture
// ---------------------------------------------------------------------------

/// A live blackboard: one project, one workspace, one group with member runs
/// and their tasks, and the durable claim store over the same database.
pub struct Blackboard {
    pub pool: SqlitePool,
    pub project_id: ProjectId,
    pub workspace_id: WorkspaceId,
    pub group_id: AgentRunGroupId,
    pub members: Vec<(AgentRunId, AgentTaskId)>,
    pub claims: Arc<WorkstreamClaimStore>,
}

impl Blackboard {
    /// A live blackboard with a group.
    pub async fn new(member_count: usize) -> Self {
        Self::build(member_count, true).await
    }

    /// The S0 shape: one agent, **no group**. The blackboard tables exist
    /// and stay empty, which is exactly what S0 must be measured against.
    pub async fn solo() -> Self {
        Self::build(1, false).await
    }

    async fn build(member_count: usize, with_group: bool) -> Self {
        let dir = tempfile::tempdir().expect("the blackboard fixture directory");
        let db_path = dir.path().join("blackboard.db");
        let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path.display()))
            .expect("valid sqlite options")
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect the blackboard database");
        codegg_core::session::schema::migrate(&pool)
            .await
            .expect("migrate the blackboard database");
        // The temporary directory owns the database file; the pool holds the
        // open handle, so the directory has to outlive the pool. Leaking it
        // is what keeps the fixture's lifetime honest.
        std::mem::forget(dir);

        let project_id = ProjectId::new();
        let workspace_id = WorkspaceId::new();
        let group_id = AgentRunGroupId::new();
        let mut members = Vec::new();
        for ordinal in 0..member_count {
            let task_id = AgentTaskId::new();
            let run_id = AgentRunId::new();
            sqlx::query(
                "INSERT INTO agent_task (task_id, root_task_id, session_id, project_id, workspace_id, \
                 requested_agent, delegation_key, request_fingerprint, description, status, created_at, updated_at) \
                 VALUES (?, ?, 'session-m008', ?, ?, 'worker', ?, 'fingerprint', 'redacted', 'running', 1, 1)",
            )
            .bind(task_id.as_str())
            .bind(task_id.as_str())
            .bind(project_id.as_str())
            .bind(workspace_id.as_str())
            .bind(format!("delegate-{ordinal}"))
            .execute(&pool)
            .await
            .expect("insert the member task");
            sqlx::query(
                "INSERT INTO agent_run (run_id, task_id, root_run_id, workspace_id, agent_name, provider, model, \
                 authority_digest, budget_json, status, cancellation_requested, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, 'worker', 'test', 'test', 'test', '{}', 'running', 0, 1, 1)",
            )
            .bind(run_id.as_str())
            .bind(task_id.as_str())
            .bind(run_id.as_str())
            .bind(workspace_id.as_str())
            .execute(&pool)
            .await
            .expect("insert the member run");
            members.push((run_id, task_id));
        }

        if with_group && !members.is_empty() {
            let root_run = &members[0].0;
            sqlx::query(
                "INSERT INTO agent_run_group (group_id, root_run_id, owner_run_id, join_policy, cancel_remaining, \
                 status, created_at, idempotency_key, owner_kind) VALUES (?, ?, ?, 'all', 0, 'running', 1, 'group-key', 'run')",
            )
            .bind(group_id.as_str())
            .bind(root_run.as_str())
            .bind(root_run.as_str())
            .execute(&pool)
            .await
            .expect("insert the group");
            for (ordinal, (run_id, _)) in members.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO agent_run_group_member (group_id, ordinal, run_id) VALUES (?, ?, ?)",
                )
                .bind(group_id.as_str())
                .bind(ordinal as i64)
                .bind(run_id.as_str())
                .execute(&pool)
                .await
                .expect("insert the group member");
            }
        }

        let claims = WorkstreamClaimStore::new(pool.clone());
        Self {
            pool,
            project_id,
            workspace_id,
            group_id,
            members,
            claims,
        }
    }

    /// A claim input scoped to this blackboard's project and workspace.
    pub fn claim_input(&self, run_id: &AgentRunId, key: &str, target: &str) -> NewWorkstreamClaim {
        NewWorkstreamClaim {
            project_id: self.project_id.clone(),
            workspace_id: self.workspace_id.clone(),
            group_id: self.group_id.clone(),
            actor: ClaimActor::Run(run_id.clone()),
            target: ClaimTarget::Path(target.to_owned()),
            idempotency_key: key.to_owned(),
            lease_expires_at: Some(chrono::Utc::now().timestamp_millis() + 60_000),
        }
    }

    /// The count of rows in a blackboard table, used to measure the durable
    /// writes a configuration performs and to prove that the no-blackboard
    /// path writes nothing.
    pub async fn row_count(&self, table: &str) -> i64 {
        let query = format!("SELECT COUNT(*) FROM {table}");
        sqlx::query_scalar(&query)
            .fetch_one(&self.pool)
            .await
            .expect("count the blackboard rows")
    }
}

/// A delegated run, created in the durable run store with the real
/// hierarchy the group service enforces.
pub struct DelegatedRun {
    pub run_id: AgentRunId,
    pub task_id: AgentTaskId,
}

/// Create `count` top-level delegated runs under one turn.
///
/// A turn-owned group's members are top-level delegated runs of that turn:
/// no parent run, depth 1, and the task names the originating turn. That is
/// the shape the group service validates for a turn-owned group, and it is
/// the shape a real turn uses when it spawns children.
pub async fn create_turn_members(
    runs: &Arc<dyn AgentRunStore>,
    project_id: &ProjectId,
    workspace_id: &WorkspaceId,
    session_id: &str,
    turn_id: &str,
    label: &str,
    count: usize,
) -> Vec<DelegatedRun> {
    let mut created = Vec::with_capacity(count);
    for index in 0..count {
        let run_id = AgentRunId::new();
        let task_id = AgentTaskId::new();
        runs.create_or_get(
            NewAgentTask {
                task_id: task_id.clone(),
                parent_task_id: None,
                originating_session_id: session_id.to_owned(),
                originating_turn_id: Some(turn_id.to_owned()),
                project_id: project_id.clone(),
                repository_id: None,
                workspace_id: workspace_id.clone(),
                requested_agent: "worker".to_owned(),
                delegation_key: format!("delegation-{index}-{label}"),
                request_fingerprint: format!("fingerprint-{index}-{label}"),
                description: format!("m008 {label}"),
            },
            NewAgentRun {
                run_id: run_id.clone(),
                parent_run_id: None,
                depth: 1,
                workspace_id: workspace_id.clone(),
                agent_name: "worker".to_owned(),
                agent_digest: None,
                provider: "test".to_owned(),
                model: "test".to_owned(),
                authority_digest: format!("authority-{index}-{label}"),
                budget: AgentRunBudget::default(),
                shared_scratch: Vec::new(),
            },
        )
        .await
        .expect("create the delegated run");
        created.push(DelegatedRun { run_id, task_id });
    }
    created
}

/// Insert the durable `agent_task` and `agent_run` rows for a run.
///
/// The SQLite group store references `agent_run`, so a group whose store is
/// the durable one needs its members present in the database as well as in
/// the run store.
pub async fn insert_run_rows(
    pool: &SqlitePool,
    project_id: &ProjectId,
    workspace_id: &WorkspaceId,
    run_id: &AgentRunId,
    task_id: &AgentTaskId,
    root_run_id: &AgentRunId,
) {
    sqlx::query(
        "INSERT INTO agent_task (task_id, root_task_id, session_id, project_id, workspace_id, \
         requested_agent, delegation_key, request_fingerprint, description, status, created_at, updated_at) \
         VALUES (?, ?, 'session-m008', ?, ?, 'worker', 'delegate', 'fingerprint', 'redacted', 'running', 1, 1)",
    )
    .bind(task_id.as_str())
    .bind(task_id.as_str())
    .bind(project_id.as_str())
    .bind(workspace_id.as_str())
    .execute(pool)
    .await
    .expect("insert the task row");
    sqlx::query(
        "INSERT INTO agent_run (run_id, task_id, root_run_id, workspace_id, agent_name, provider, model, \
         authority_digest, budget_json, status, cancellation_requested, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 'worker', 'test', 'test', 'test', '{}', 'running', 0, 1, 1)",
    )
    .bind(run_id.as_str())
    .bind(task_id.as_str())
    .bind(root_run_id.as_str())
    .bind(workspace_id.as_str())
    .execute(pool)
    .await
    .expect("insert the run row");
}
