use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

use crate::agent::run_control::{ControlActor, ControlOutcome, RunControlService};
use crate::agent::worker::{SubAgentRequest, SubAgentSpawner};
use crate::error::ToolError;
use crate::tool::{Tool, ToolCategory};
use codegg_core::agent_run::{AgentRunBudget, AgentRunStore, NewAgentRun, NewAgentTask};
use codegg_core::agent_run_control::AgentRunControlKind;
use codegg_core::agent_run_group::{
    AgentRunGroupOwner, AgentRunGroupService, AgentRunGroupSummary, GroupActor, NewAgentRunGroup,
    RunJoinPolicy, MAX_GROUP_MEMBERS,
};
use codegg_core::identity::{AgentRunGroupId, AgentRunId, AgentTaskId, ProjectId, RepositoryId};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentTask {
    pub id: u64,
    pub description: String,
    pub prompt: String,
    pub agent: String,
    pub status: TaskStatus,
    pub result: Option<String>,
    pub parent_id: Option<String>,
    pub denied_tools: Vec<String>,
    pub allowed_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Interrupted,
}

pub struct TaskStore {
    tasks: Mutex<HashMap<u64, SubAgentTask>>,
    next_id: AtomicU64,
    pool: Option<SqlitePool>,
}

impl TaskStore {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            pool: None,
        }
    }

    pub fn set_pool(&mut self, pool: SqlitePool) {
        self.pool = Some(pool);
    }

    pub async fn save_task(&self, task: &SubAgentTask) -> Result<(), sqlx::Error> {
        if let Some(ref pool) = self.pool {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            let denied_tools = serde_json::to_string(&task.denied_tools).unwrap_or_default();
            let allowed_paths = serde_json::to_string(&task.allowed_paths).unwrap_or_default();
            let status_str = match task.status {
                TaskStatus::Pending => "pending",
                TaskStatus::Running => "running",
                TaskStatus::Completed => "completed",
                TaskStatus::Failed => "failed",
                TaskStatus::Interrupted => "interrupted",
            };

            sqlx::query(
                r#"
                INSERT INTO task (parent_id, session_id, description, prompt, agent, status,
                                 result, denied_tools, allowed_paths, time_created, time_updated)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(id) DO UPDATE SET
                    status = excluded.status,
                    result = excluded.result,
                    time_updated = excluded.time_updated
                "#,
            )
            .bind(&task.parent_id)
            .bind(task.parent_id.clone().unwrap_or_default())
            .bind(&task.description)
            .bind(&task.prompt)
            .bind(&task.agent)
            .bind(status_str)
            .bind(&task.result)
            .bind(&denied_tools)
            .bind(&allowed_paths)
            .bind(now)
            .bind(now)
            .execute(pool)
            .await?;

            Ok(())
        } else {
            Ok(())
        }
    }

    pub async fn load_tasks(&self) -> Result<Vec<SubAgentTask>, sqlx::Error> {
        if let Some(ref pool) = self.pool {
            let rows = sqlx::query(
                r#"
                SELECT id, parent_id, session_id, description, prompt, agent, status,
                       result, denied_tools, allowed_paths, time_created, time_updated
                FROM task
                WHERE status IN ('pending', 'running')
                "#,
            )
            .fetch_all(pool)
            .await?;

            let mut tasks = Vec::new();
            for row in rows {
                let id: u64 = row.get("id");
                let parent_id: Option<String> = row.get("parent_id");
                let _session_id: String = row.get("session_id");
                let description: String = row.get("description");
                let prompt: String = row.get("prompt");
                let agent: String = row.get("agent");
                let status_str: String = row.get("status");
                let result: Option<String> = row.get("result");
                let denied_tools_str: Option<String> = row.get("denied_tools");
                let allowed_paths_str: Option<String> = row.get("allowed_paths");

                let status = match status_str.as_str() {
                    "running" => TaskStatus::Running,
                    "completed" => TaskStatus::Completed,
                    "failed" => TaskStatus::Failed,
                    "interrupted" => TaskStatus::Interrupted,
                    _ => TaskStatus::Pending,
                };

                let denied_tools: Vec<String> = denied_tools_str
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();

                let allowed_paths: Vec<String> = allowed_paths_str
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();

                tasks.push(SubAgentTask {
                    id,
                    description,
                    prompt,
                    agent,
                    status,
                    result,
                    parent_id,
                    denied_tools,
                    allowed_paths,
                });
            }

            Ok(tasks)
        } else {
            Ok(Vec::new())
        }
    }

    pub async fn update_status_in_db(
        &self,
        id: u64,
        status: &TaskStatus,
        result: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        if let Some(ref pool) = self.pool {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            let status_str = match status {
                TaskStatus::Pending => "pending",
                TaskStatus::Running => "running",
                TaskStatus::Completed => "completed",
                TaskStatus::Failed => "failed",
                TaskStatus::Interrupted => "interrupted",
            };

            sqlx::query("UPDATE task SET status = ?, result = ?, time_updated = ? WHERE id = ?")
                .bind(status_str)
                .bind(result)
                .bind(now)
                .bind(id as i64)
                .execute(pool)
                .await?;

            Ok(())
        } else {
            Ok(())
        }
    }

    pub async fn create_task(
        &self,
        description: String,
        prompt: String,
        agent: String,
        parent_id: Option<String>,
        denied_tools: Vec<String>,
        allowed_paths: Vec<String>,
    ) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.create_task_with_id(
            id,
            description,
            prompt,
            agent,
            parent_id,
            denied_tools,
            allowed_paths,
        )
        .await;
        id
    }

    pub async fn create_task_with_id(
        &self,
        id: u64,
        description: String,
        prompt: String,
        agent: String,
        parent_id: Option<String>,
        denied_tools: Vec<String>,
        allowed_paths: Vec<String>,
    ) {
        let task = SubAgentTask {
            id,
            description,
            prompt,
            agent,
            status: TaskStatus::Pending,
            result: None,
            parent_id,
            denied_tools,
            allowed_paths,
        };
        self.tasks.lock().await.insert(id, task);
    }

    pub async fn update_status(&self, id: u64, status: TaskStatus) {
        if let Some(task) = self.tasks.lock().await.get_mut(&id) {
            task.status = status;
        }
    }

    pub async fn set_result(&self, id: u64, result: String) {
        let db_update = {
            let mut tasks = self.tasks.lock().await;
            if let Some(task) = tasks.get_mut(&id) {
                task.result = Some(result.clone());
                task.status = TaskStatus::Completed;
                Some((TaskStatus::Completed, result))
            } else {
                None
            }
        };
        if let Some((status, result)) = db_update {
            if let Err(e) = self.update_status_in_db(id, &status, Some(&result)).await {
                tracing::warn!(error = %e, id = %id, "failed to update task status");
            }
        }
    }

    pub async fn set_failed(&self, id: u64, error: String) {
        let db_update = {
            let mut tasks = self.tasks.lock().await;
            if let Some(task) = tasks.get_mut(&id) {
                task.result = Some(error.clone());
                task.status = TaskStatus::Failed;
                Some((TaskStatus::Failed, error))
            } else {
                None
            }
        };
        if let Some((status, error)) = db_update {
            if let Err(e) = self.update_status_in_db(id, &status, Some(&error)).await {
                tracing::warn!(error = %e, id = %id, "failed to update task status");
            }
        }
    }

    /// Set the task as interrupted with a custom message.
    /// This preserves the Interrupted status (unlike set_failed which sets Failed).
    pub async fn set_interrupted(&self, id: u64, msg: String) {
        let db_update = {
            let mut tasks = self.tasks.lock().await;
            if let Some(task) = tasks.get_mut(&id) {
                task.result = Some(msg.clone());
                task.status = TaskStatus::Interrupted;
                Some((TaskStatus::Interrupted, msg))
            } else {
                None
            }
        };
        if let Some((status, msg)) = db_update {
            if let Err(e) = self.update_status_in_db(id, &status, Some(&msg)).await {
                tracing::warn!(error = %e, id = %id, "failed to update task status");
            }
        }
    }

    /// Set the task as failed, but only if it's not already Interrupted.
    /// Returns true if the status was changed.
    pub async fn set_failed_if_not_interrupted(&self, id: u64, error: String) -> bool {
        let db_update = {
            let mut tasks = self.tasks.lock().await;
            if let Some(task) = tasks.get_mut(&id) {
                if task.status == TaskStatus::Interrupted {
                    return false;
                }
                task.result = Some(error.clone());
                task.status = TaskStatus::Failed;
                Some((TaskStatus::Failed, error))
            } else {
                None
            }
        };
        if let Some((status, error)) = db_update {
            if let Err(e) = self.update_status_in_db(id, &status, Some(&error)).await {
                tracing::warn!(error = %e, id = %id, "failed to update task status");
            }
            true
        } else {
            false
        }
    }

    pub async fn get_task(&self, id: u64) -> Option<SubAgentTask> {
        self.tasks.lock().await.get(&id).cloned()
    }

    pub fn format_task(&self, task: &SubAgentTask) -> String {
        let status = match task.status {
            TaskStatus::Pending => "pending",
            TaskStatus::Running => "running",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Interrupted => "interrupted",
        };
        let result = task
            .result
            .as_ref()
            .map(|r| format!("\nResult: {}", r))
            .unwrap_or_default();
        format!(
            "Task #{}: {}\nStatus: {}{}",
            task.id, task.description, status, result
        )
    }

    pub async fn get_and_format_task(&self, task_id: u64) -> Option<String> {
        let guard = self.tasks.lock().await;
        guard.get(&task_id).map(|task| self.format_task(task))
    }
}

impl Default for TaskStore {
    fn default() -> Self {
        Self::new()
    }
}

pub struct TaskTool {
    store: Arc<Mutex<TaskStore>>,
    spawner: Option<SubAgentSpawner>,
    parent_session_id: Option<String>,
    denied_tools: Vec<String>,
    depth: usize,
    parent_model: Option<String>,
    submission: Option<Arc<crate::scheduler::JobSubmissionService>>,
    workspace_root: Option<std::path::PathBuf>,
    parent_allowed_paths: Vec<String>,
    agent_runs: Option<Arc<dyn AgentRunStore>>,
    project_id: Option<ProjectId>,
    repository_id: Option<RepositoryId>,
    parent_turn_id: Option<String>,
    run_control: Option<Arc<RunControlService>>,
    parent_run_id: Option<AgentRunId>,
    run_groups: Option<Arc<AgentRunGroupService>>,
    orchestration_owner: Option<AgentOrchestrationOwner>,
    max_depth: u32,
}

/// Explicit owner of orchestration operations. A root turn owns its direct
/// fan-out; a durable run owns only its direct descendants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentOrchestrationOwner {
    Turn { session_id: String, turn_id: String },
    Run { run_id: AgentRunId },
}

fn next_durable_depth(parent_depth: Option<u32>, max_depth: u32) -> Result<u32, String> {
    let depth = parent_depth.map_or(1, |depth| depth.saturating_add(1));
    if depth > max_depth {
        return Err(format!(
            "subagent max depth {} exceeded (requested depth: {})",
            max_depth, depth
        ));
    }
    Ok(depth)
}

fn format_control_outcome(outcome: ControlOutcome) -> String {
    match outcome {
        ControlOutcome::Queued(message) => format!(
            "Control queued\nRun: {}\nKind: {}\nSequence: {}\nState: {}",
            message.run_id,
            message.kind.as_str(),
            message.sequence,
            match message.state {
                codegg_core::agent_run_control::MailboxState::Queued => "queued",
                codegg_core::agent_run_control::MailboxState::Delivered => "delivered",
                codegg_core::agent_run_control::MailboxState::Acknowledged => "acknowledged",
                codegg_core::agent_run_control::MailboxState::Superseded => "superseded",
            }
        ),
        ControlOutcome::Terminal(status) => format!("Run is terminal: {}", status.as_str()),
        ControlOutcome::Status(run) => format!(
            "Run: {}\nStatus: {}\nTask: {}\nResult: {}",
            run.run_id,
            run.status.as_str(),
            run.task_id,
            run.result_ref.unwrap_or_default()
        ),
        ControlOutcome::Wait { run, timed_out } => format!(
            "Run: {}\nStatus: {}\nTimed out: {}\nResult: {}",
            run.run_id,
            run.status.as_str(),
            timed_out,
            run.result_ref.unwrap_or_default()
        ),
    }
}

impl TaskTool {
    fn publish_group_projection(&self, summary: &AgentRunGroupSummary) {
        let Some(session_id) = self.parent_session_id.clone() else {
            return;
        };
        let updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or_default();
        crate::bus::global::GlobalEventBus::publish(
            crate::bus::events::AppEvent::AgentRunGroupUpdated {
                session_id,
                group: codegg_core::projection_replay::run_group_summary(summary, updated_at),
            },
        );
    }

    pub fn new(
        store: Arc<Mutex<TaskStore>>,
        spawner: Option<SubAgentSpawner>,
        parent_session_id: Option<String>,
        denied_tools: Vec<String>,
    ) -> Self {
        Self {
            store,
            spawner,
            parent_session_id,
            denied_tools,
            depth: 0,
            parent_model: None,
            submission: None,
            workspace_root: None,
            parent_allowed_paths: Vec::new(),
            agent_runs: None,
            project_id: None,
            repository_id: None,
            parent_turn_id: None,
            run_control: None,
            parent_run_id: None,
            run_groups: None,
            orchestration_owner: None,
            max_depth: 3,
        }
    }

    pub fn new_with_pool(
        pool: Arc<crate::agent::worker::SubAgentPool>,
        parent_session_id: Option<String>,
        denied_tools: Vec<String>,
    ) -> Self {
        Self {
            store: pool.task_store(),
            spawner: Some(pool.spawner()),
            parent_session_id,
            denied_tools,
            depth: 0,
            parent_model: None,
            submission: None,
            workspace_root: None,
            parent_allowed_paths: Vec::new(),
            agent_runs: None,
            project_id: None,
            repository_id: None,
            parent_turn_id: None,
            run_control: None,
            parent_run_id: None,
            run_groups: None,
            orchestration_owner: None,
            max_depth: 3,
        }
    }

    pub fn with_parent_model(mut self, model: Option<String>) -> Self {
        self.parent_model = model;
        self
    }

    pub fn with_depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    pub fn with_submission(
        mut self,
        submission: Arc<crate::scheduler::JobSubmissionService>,
        workspace_root: std::path::PathBuf,
    ) -> Self {
        self.submission = Some(submission);
        self.workspace_root = Some(workspace_root);
        self
    }

    pub fn with_agent_run_store(mut self, store: Arc<dyn AgentRunStore>) -> Self {
        self.agent_runs = Some(store);
        self
    }

    pub fn with_agent_run_store_opt(mut self, store: Option<Arc<dyn AgentRunStore>>) -> Self {
        self.agent_runs = store;
        self
    }

    pub fn with_run_control_opt(mut self, service: Option<Arc<RunControlService>>) -> Self {
        self.run_control = service;
        self
    }

    pub fn with_parent_run_id(mut self, run_id: Option<AgentRunId>) -> Self {
        self.orchestration_owner = run_id
            .clone()
            .map(|run_id| AgentOrchestrationOwner::Run { run_id });
        self.parent_run_id = run_id;
        self
    }

    pub fn with_turn_owner(mut self, session_id: String, turn_id: String) -> Self {
        self.orchestration_owner = Some(AgentOrchestrationOwner::Turn {
            session_id,
            turn_id,
        });
        self
    }

    pub fn with_run_owner(mut self, run_id: AgentRunId) -> Self {
        self.orchestration_owner = Some(AgentOrchestrationOwner::Run {
            run_id: run_id.clone(),
        });
        self.parent_run_id = Some(run_id);
        self
    }

    pub fn with_max_depth(mut self, max_depth: u32) -> Self {
        self.max_depth = max_depth.max(1);
        self
    }

    pub fn with_run_group_service(mut self, service: Option<Arc<AgentRunGroupService>>) -> Self {
        self.run_groups = service;
        self
    }

    pub fn with_project_context(
        mut self,
        project_id: Option<ProjectId>,
        repository_id: Option<RepositoryId>,
        turn_id: Option<String>,
    ) -> Self {
        self.project_id = project_id;
        self.repository_id = repository_id;
        self.parent_turn_id = turn_id;
        self
    }

    /// Restrict descendant path requests to the scope inherited by this task.
    pub fn with_parent_allowed_paths(mut self, paths: Vec<String>) -> Self {
        self.parent_allowed_paths = paths;
        self
    }

    pub fn with_workspace_root(mut self, root: Option<std::path::PathBuf>) -> Self {
        self.workspace_root = root;
        self
    }

    pub fn store(&self) -> Arc<Mutex<TaskStore>> {
        self.store.clone()
    }

    pub fn depth(&self) -> usize {
        self.depth
    }
}

impl Default for TaskTool {
    fn default() -> Self {
        Self::new(
            Arc::new(Mutex::new(TaskStore::new())),
            None,
            None,
            Vec::new(),
        )
    }
}

#[async_trait]
impl Tool for TaskTool {
    fn name(&self) -> &str {
        "task"
    }

    fn description(&self) -> &str {
        "Spawn a subagent to handle a task independently. Use spawn_many for a bounded group of independent children, then wait_group/status_group/cancel_group; prefer wait or push notifications over polling get/status. Mutating durable runs receive a managed isolated worktree before model execution; read-only runs inherit the parent workspace. Child completion never merges into the parent automatically—inspect the structured result and request explicit typed integration."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action: spawn, spawn_many, create_group, status (get alias), message, interrupt, wait, cancel, status_group, wait_group, or cancel_group",
                    "enum": ["spawn", "spawn_many", "create_group", "status", "get", "message", "interrupt", "wait", "cancel", "status_group", "wait_group", "cancel_group"]
                },
                "description": {
                    "type": "string",
                    "description": "Description of the task for the subagent (action=spawn)"
                },
                "prompt": {
                    "type": "string",
                    "description": "Detailed prompt for the subagent (action=spawn)"
                },
                "agent": {
                    "type": "string",
                    "description": "Agent to use (default: general, action=spawn)"
                },
                "allowed_paths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "List of directories the subagent is allowed to access (action=spawn). Mutating durable runs are narrowed to their managed worktree."
                },
                "task_id": {
                    "description": "Durable AgentTaskId or legacy numeric task ID"
                },
                "run_id": {
                    "type": "string",
                    "description": "Durable AgentRunId for control/status/wait"
                },
                "message": {
                    "type": "string",
                    "description": "Bounded control message (action=message)"
                },
                "idempotency_key": {
                    "type": "string",
                    "description": "Stable retry key for a control operation"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Bounded wait timeout in milliseconds"
                },
                "requests": {
                    "type": "array",
                    "maxItems": 16,
                    "description": "Bounded child requests for action=spawn_many. Each item has description, prompt, optional agent, and optional allowed_paths.",
                    "items": { "type": "object" }
                },
                "member_run_ids": {
                    "type": "array",
                    "maxItems": 16,
                    "items": { "type": "string" },
                    "description": "Accepted direct child AgentRunIds for action=create_group or spawn_many"
                },
                "group_id": {
                    "type": "string",
                    "description": "Durable AgentRunGroupId for group actions"
                },
                "join_policy": {
                    "type": "string",
                    "enum": ["all", "any_successful", "first_completed", "detached"],
                    "description": "Deterministic group join policy"
                },
                "cancel_remaining_on_satisfaction": {
                    "type": "boolean",
                    "description": "For any_successful or first_completed, persist whether active siblings are cancelled after satisfaction"
                }
            },
            "required": ["action"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }

    fn has_functional_backend(&self) -> bool {
        self.spawner.is_some() || self.submission.is_some()
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let action = input["action"].as_str().unwrap_or("spawn");

        if matches!(
            action,
            "spawn_many" | "create_group" | "status_group" | "wait_group" | "cancel_group"
        ) {
            let Some(groups) = self.run_groups.clone() else {
                return Err(ToolError::Execution(
                    "durable run groups are unavailable".into(),
                ));
            };
            let owner = self.orchestration_owner.clone().ok_or_else(|| {
                ToolError::Execution("group actions require an orchestration owner".into())
            })?;
            let (owner_run_id, actor, group_owner) = match owner {
                AgentOrchestrationOwner::Run { run_id } => (
                    run_id.clone(),
                    GroupActor {
                        run_id: Some(run_id.clone()),
                        session_id: None,
                        turn_id: None,
                    },
                    AgentRunGroupOwner::Run { run_id },
                ),
                AgentOrchestrationOwner::Turn {
                    session_id,
                    turn_id,
                } => (
                    AgentRunId::new(),
                    GroupActor {
                        run_id: None,
                        session_id: Some(session_id.clone()),
                        turn_id: Some(turn_id.clone()),
                    },
                    AgentRunGroupOwner::Turn {
                        session_id,
                        turn_id,
                    },
                ),
            };
            if matches!(action, "spawn_many" | "create_group") {
                let (accepted, rejected) = if action == "spawn_many" {
                    let requests = input["requests"].as_array().ok_or_else(|| {
                        ToolError::Execution("spawn_many requires a requests array".into())
                    })?;
                    if requests.is_empty() || requests.len() > MAX_GROUP_MEMBERS {
                        return Err(ToolError::Execution(format!(
                            "spawn_many requires 1 to {MAX_GROUP_MEMBERS} requests"
                        )));
                    }
                    let mut accepted = Vec::with_capacity(requests.len());
                    let mut rejected = Vec::new();
                    for request in requests {
                        let mut child = request.clone();
                        let object = child.as_object_mut().ok_or_else(|| {
                            ToolError::Execution("each spawn_many request must be an object".into())
                        })?;
                        object.insert("action".into(), serde_json::Value::String("spawn".into()));
                        match self.execute(child).await {
                            Ok(output) => {
                                if let Some(run_id) = output.lines().find_map(|line| {
                                    line.strip_prefix("Run: ")
                                        .and_then(|value| AgentRunId::parse(value.trim()).ok())
                                }) {
                                    accepted.push(run_id);
                                } else {
                                    rejected.push(
                                        "child did not return a durable run handle".to_string(),
                                    );
                                }
                            }
                            Err(error) => rejected.push(error.to_string()),
                        }
                    }
                    (accepted, rejected)
                } else {
                    let values = input["member_run_ids"].as_array().ok_or_else(|| {
                        ToolError::Execution("create_group requires a member_run_ids array".into())
                    })?;
                    if values.is_empty() || values.len() > MAX_GROUP_MEMBERS {
                        return Err(ToolError::Execution(format!(
                            "create_group requires 1 to {MAX_GROUP_MEMBERS} member_run_ids"
                        )));
                    }
                    let mut accepted = Vec::with_capacity(values.len());
                    for value in values {
                        let raw = value.as_str().ok_or_else(|| {
                            ToolError::Execution("member_run_ids must contain strings".into())
                        })?;
                        accepted.push(AgentRunId::parse(raw).map_err(|error| {
                            ToolError::Execution(format!("invalid member run_id: {error}"))
                        })?);
                    }
                    (accepted, Vec::new())
                };
                if accepted.is_empty() {
                    return Err(ToolError::Execution(format!(
                        "spawn_many accepted no children: {}",
                        rejected.join("; ")
                    )));
                }
                let policy = parse_join_policy(input["join_policy"].as_str().unwrap_or("all"))?;
                let group_id = input["group_id"]
                    .as_str()
                    .map(AgentRunGroupId::parse)
                    .transpose()
                    .map_err(|e| ToolError::Execution(format!("invalid group_id: {e}")))?
                    .unwrap_or_else(AgentRunGroupId::new);
                let key = input["idempotency_key"]
                    .as_str()
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| {
                        let canonical = serde_json::to_string(&input).unwrap_or_default();
                        let owner_key = match &self.orchestration_owner {
                            Some(AgentOrchestrationOwner::Turn {
                                session_id,
                                turn_id,
                            }) => format!("turn:{session_id}:{turn_id}"),
                            Some(AgentOrchestrationOwner::Run { run_id }) => run_id.to_string(),
                            None => owner_run_id.to_string(),
                        };
                        format!(
                            "group:{}:{:x}",
                            owner_key,
                            Sha256::digest(canonical.as_bytes())
                        )
                    });
                let root_run_id = if matches!(&group_owner, AgentRunGroupOwner::Run { .. }) {
                    self.agent_runs
                        .as_ref()
                        .ok_or_else(|| {
                            ToolError::Execution("group requires a durable run store".into())
                        })?
                        .get_run(&owner_run_id)
                        .await
                        .map_err(|error| ToolError::Execution(error.to_string()))?
                        .ok_or_else(|| ToolError::Execution("owner run no longer exists".into()))?
                        .root_run_id
                } else {
                    accepted.first().cloned().ok_or_else(|| {
                        ToolError::Execution("group has no accepted members".into())
                    })?
                };
                let summary = groups
                    .create(NewAgentRunGroup {
                        group_id,
                        root_run_id,
                        owner_run_id: accepted.first().cloned().unwrap_or(owner_run_id.clone()),
                        owner: group_owner,
                        member_run_ids: accepted,
                        join_policy: policy,
                        cancel_remaining_on_satisfaction: input["cancel_remaining_on_satisfaction"]
                            .as_bool()
                            .unwrap_or(false),
                        idempotency_key: key,
                    })
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                self.publish_group_projection(&summary);
                return Ok(format_group_summary(
                    &summary,
                    !rejected.is_empty(),
                    &rejected,
                ));
            }
            let raw_group_id = input["group_id"]
                .as_str()
                .ok_or_else(|| ToolError::Execution("group action requires a group_id".into()))?;
            let group_id = AgentRunGroupId::parse(raw_group_id)
                .map_err(|e| ToolError::Execution(format!("invalid group_id: {e}")))?;
            let result = match action {
                "status_group" => groups.status(&actor, group_id).await,
                "wait_group" => {
                    let timeout = input["timeout_ms"].as_u64().unwrap_or(1000).min(30_000);
                    groups
                        .wait(&actor, group_id, std::time::Duration::from_millis(timeout))
                        .await
                }
                "cancel_group" => groups.cancel(&actor, group_id).await,
                _ => unreachable!(),
            };
            return match result {
                Ok(summary) => {
                    self.publish_group_projection(&summary);
                    Ok(format_group_summary(&summary, false, &[]))
                }
                Err(codegg_core::agent_run_group::AgentRunGroupError::WaitTimedOut(summary)) => {
                    self.publish_group_projection(&summary);
                    Ok(format_group_summary(&summary, true, &[]))
                }
                Err(error) => Err(ToolError::Execution(error.to_string())),
            };
        }

        if matches!(
            action,
            "status" | "message" | "interrupt" | "wait" | "cancel"
        ) {
            let Some(control) = self.run_control.clone() else {
                return Err(ToolError::Execution(
                    "durable run control is unavailable".into(),
                ));
            };
            let raw_run_id = input["run_id"]
                .as_str()
                .ok_or_else(|| ToolError::Execution("missing 'run_id' parameter".into()))?;
            let run_id = AgentRunId::parse(raw_run_id)
                .map_err(|e| ToolError::Execution(format!("invalid run_id: {e}")))?;
            let actor = ControlActor {
                session_id: self.parent_session_id.clone(),
                run_id: self.parent_run_id.clone(),
                turn_id: self.parent_turn_id.clone(),
            };
            let outcome = match action {
                "status" => control.status(&actor, run_id).await,
                "wait" => {
                    let timeout = input["timeout_ms"].as_u64().unwrap_or(1000).min(30_000);
                    control
                        .wait(&actor, run_id, std::time::Duration::from_millis(timeout))
                        .await
                }
                "message" | "interrupt" | "cancel" => {
                    let kind = match action {
                        "message" => AgentRunControlKind::Message,
                        "interrupt" => AgentRunControlKind::Interrupt,
                        _ => AgentRunControlKind::Cancel,
                    };
                    let payload = input["message"].as_str().unwrap_or_default().to_string();
                    let key = input["idempotency_key"]
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(|| format!("{}:{}", action, raw_run_id));
                    control.send(&actor, run_id, kind, payload, key).await
                }
                _ => unreachable!(),
            }
            .map_err(|e| ToolError::Execution(e.to_string()))?;
            return Ok(format_control_outcome(outcome));
        }

        if action == "get" {
            if let (Some(raw_id), Some(agent_runs)) =
                (input["task_id"].as_str(), self.agent_runs.clone())
            {
                let task_id = AgentTaskId::parse(raw_id)
                    .map_err(|e| ToolError::Execution(format!("invalid durable task_id: {e}")))?;
                let task = agent_runs
                    .get_task(&task_id)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                let Some(task) = task else {
                    return Ok(format!("Task {} not found", task_id));
                };
                if self
                    .parent_session_id
                    .as_deref()
                    .is_some_and(|session| task.originating_session_id != session)
                {
                    return Ok(format!("Task {} not found", task_id));
                }
                let run = agent_runs
                    .get_run_for_task(&task_id)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                return Ok(match run {
                    Some(run) => format!(
                        "Task: {}\nRun: {}\nStatus: {}\nResult: {}",
                        task.task_id,
                        run.run_id,
                        run.status.as_str(),
                        run.result_ref.unwrap_or_default()
                    ),
                    None => format!("Task: {}\nStatus: {}", task.task_id, task.status.as_str()),
                });
            }
            let task_id = input["task_id"]
                .as_u64()
                .ok_or_else(|| ToolError::Execution("missing 'task_id' parameter".to_string()))?;

            if let Some(formatted) = self.store.lock().await.get_and_format_task(task_id).await {
                Ok(formatted)
            } else {
                Ok(format!("Task #{} not found", task_id))
            }
        } else {
            let description = input["description"]
                .as_str()
                .ok_or_else(|| ToolError::Execution("missing 'description' parameter".to_string()))?
                .to_string();

            let prompt = input["prompt"]
                .as_str()
                .ok_or_else(|| ToolError::Execution("missing 'prompt' parameter".to_string()))?
                .to_string();

            let agent = input["agent"].as_str().unwrap_or("general").to_string();

            let denied_tools = self.denied_tools.clone();

            let allowed_paths: Vec<String> = input["allowed_paths"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            let allowed_paths = if self.parent_allowed_paths.is_empty() {
                allowed_paths
            } else if allowed_paths.is_empty() {
                self.parent_allowed_paths.clone()
            } else {
                if allowed_paths.iter().any(|requested| {
                    !self.parent_allowed_paths.iter().any(|parent| {
                        requested == parent || requested.starts_with(&format!("{parent}/"))
                    })
                }) {
                    return Err(ToolError::Execution(
                        "child allowed_paths exceed the parent path scope".to_string(),
                    ));
                }
                allowed_paths
            };

            if let (Some(submission), Some(workspace_root)) =
                (self.submission.clone(), self.workspace_root.clone())
            {
                let workspace_id = submission
                    .workspace_id_for_root(&workspace_root)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                let durable = if let Some(agent_runs) = self.agent_runs.clone() {
                    let delegation_key = delegation_key(
                        self.parent_session_id.as_deref(),
                        self.parent_turn_id.as_deref(),
                        &agent,
                        &prompt,
                        &allowed_paths,
                    );
                    let project_id = self.project_id.clone().unwrap_or_default();
                    let provider = self
                        .parent_model
                        .as_deref()
                        .and_then(|model| model.split('/').next())
                        .unwrap_or("unknown")
                        .to_string();
                    let authority_digest = authority_digest(&denied_tools, &allowed_paths);
                    let parent_run = match &self.orchestration_owner {
                        Some(AgentOrchestrationOwner::Run { run_id }) => agent_runs
                            .get_run(run_id)
                            .await
                            .map_err(|e| ToolError::Execution(e.to_string()))?,
                        _ => None,
                    };
                    let parent_run_id = parent_run.as_ref().map(|run| run.run_id.clone());
                    let depth = next_durable_depth(
                        parent_run.as_ref().map(|run| run.depth),
                        self.max_depth,
                    )
                    .map_err(ToolError::Execution)?;
                    let created = agent_runs
                        .create_or_get(
                            NewAgentTask {
                                task_id: AgentTaskId::new(),
                                parent_task_id: if let Some(parent_run_id) = parent_run_id.as_ref()
                                {
                                    agent_runs
                                        .get_run(parent_run_id)
                                        .await
                                        .ok()
                                        .flatten()
                                        .map(|run| run.task_id)
                                } else {
                                    None
                                },
                                originating_session_id: self
                                    .parent_session_id
                                    .clone()
                                    .unwrap_or_default(),
                                originating_turn_id: self.parent_turn_id.clone(),
                                project_id,
                                repository_id: self.repository_id.clone(),
                                workspace_id: workspace_id.clone(),
                                requested_agent: agent.clone(),
                                delegation_key: delegation_key.clone(),
                                description: description.clone(),
                            },
                            NewAgentRun {
                                run_id: AgentRunId::new(),
                                parent_run_id,
                                workspace_id: workspace_id.clone(),
                                agent_name: agent.clone(),
                                agent_digest: None,
                                provider,
                                model: self.parent_model.clone().unwrap_or_default(),
                                authority_digest,
                                budget: AgentRunBudget {
                                    max_depth: Some(self.max_depth),
                                    ..Default::default()
                                },
                                depth,
                            },
                        )
                        .await
                        .map_err(|e| ToolError::Execution(e.to_string()))?;
                    if created.created {
                        if let Some(control) = self.run_control.as_ref() {
                            control
                                .append(
                                    created.run.run_id.clone(),
                                    codegg_core::agent_run_control::AgentRunJournalEventKind::RunCreated,
                                    None,
                                    None,
                                    [("task_id".into(), created.task.task_id.to_string())],
                                )
                                .await
                                .map_err(|e| ToolError::Execution(e.to_string()))?;
                        }
                    }
                    if created.run.job_id.is_some() || created.run.status.is_terminal() {
                        return Ok(format!(
                            "Task status\nTask: {}\nRun: {}\nStatus: {}",
                            created.task.task_id,
                            created.run.run_id,
                            created.run.status.as_str()
                        ));
                    }
                    agent_runs
                        .transition_task(
                            &created.task.task_id,
                            codegg_core::agent_run::AgentTaskStatus::Queued,
                        )
                        .await
                        .map_err(|e| ToolError::Execution(e.to_string()))?;
                    if let Some(control) = self.run_control.as_ref() {
                        control
                            .append(
                                created.run.run_id.clone(),
                                codegg_core::agent_run_control::AgentRunJournalEventKind::RunQueued,
                                None,
                                None,
                                [],
                            )
                            .await
                            .map_err(|e| ToolError::Execution(e.to_string()))?;
                    }
                    agent_runs
                        .transition(
                            &created.run.run_id,
                            codegg_core::agent_run::AgentRunStatus::Queued,
                        )
                        .await
                        .map_err(|e| ToolError::Execution(e.to_string()))?;
                    Some((created.task.task_id, created.run.run_id, delegation_key))
                } else {
                    None
                };
                let submitted = match submission
                    .submit(
                        durable.as_ref().and_then(|(_, _, key)| {
                            crate::scheduler::SubmissionKey::new(key.clone()).ok()
                        }),
                        codegg_core::jobs::NewJob {
                            workspace_id,
                            session_id: self.parent_session_id.clone(),
                            turn_id: self.parent_turn_id.clone(),
                            kind: codegg_core::jobs::JobKind::Subagent,
                            source: codegg_core::jobs::JobSource::AgentDelegated,
                            priority: codegg_core::jobs::JobPriority::Interactive,
                            payload: if let Some((task_id, run_id, delegation_key)) =
                                durable.clone()
                            {
                                codegg_core::jobs::JobPayload::SubagentRun {
                                    prompt,
                                    agent,
                                    parent_id: self.parent_session_id.clone(),
                                    denied_tools: denied_tools.clone(),
                                    allowed_paths: if allowed_paths.is_empty() {
                                        vec![workspace_root.to_string_lossy().into_owned()]
                                    } else {
                                        allowed_paths.clone()
                                    },
                                    max_tool_calls: None,
                                    task_id,
                                    run_id,
                                    delegation_key,
                                }
                            } else {
                                codegg_core::jobs::JobPayload::Subagent {
                                    prompt,
                                    agent,
                                    parent_id: self.parent_session_id.clone(),
                                    denied_tools: denied_tools.clone(),
                                    allowed_paths: if allowed_paths.is_empty() {
                                        vec![workspace_root.to_string_lossy().into_owned()]
                                    } else {
                                        allowed_paths.clone()
                                    },
                                    max_tool_calls: None,
                                }
                            },
                            resource_request: codegg_core::jobs::ResourceRequest::for_kind(
                                codegg_core::jobs::JobKind::Subagent,
                            ),
                            timeout: None,
                            retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
                            idempotency: codegg_core::jobs::IdempotencyClass::NonIdempotent,
                            not_before: None,
                            deadline: None,
                            schedule_id: None,
                            depends_on: Vec::new(),
                            parent_job_id: None,

                            parent_attempt_id: None,

                            parent_call_id: None,
                            parent_program_id: None,
                            parent_instruction_sequence: None,
                            relation_kind: None,
                        },
                    )
                    .await
                {
                    Ok(submitted) => submitted,
                    Err(error) => {
                        if let Some((_, run_id, _)) = durable.as_ref() {
                            if let Some(agent_runs) = self.agent_runs.clone() {
                                let _ = agent_runs
                                    .finish(
                                        run_id,
                                        codegg_core::agent_run::AgentRunTerminalOutcome::Failed,
                                        None,
                                        Some("submission".into()),
                                        Some(error.to_string()),
                                    )
                                    .await;
                            }
                        }
                        return Err(ToolError::Execution(error.to_string()));
                    }
                };
                let durable_ids = durable
                    .as_ref()
                    .map(|(task_id, run_id, _)| (task_id.clone(), run_id.clone()));
                if let Some((_, run_id, _)) = durable {
                    if let Some(agent_runs) = self.agent_runs.clone() {
                        agent_runs
                            .attach_job(&run_id, submitted.job_id.clone())
                            .await
                            .map_err(|e| ToolError::Execution(e.to_string()))?;
                    }
                }
                let task_id = submitted
                    .job_id
                    .as_str()
                    .bytes()
                    .take(8)
                    .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
                self.store
                    .lock()
                    .await
                    .create_task_with_id(
                        task_id,
                        description,
                        input["prompt"].as_str().unwrap_or_default().to_string(),
                        input["agent"].as_str().unwrap_or("general").to_string(),
                        self.parent_session_id.clone(),
                        input["allowed_paths"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        Vec::new(),
                    )
                    .await;
                if let Some((durable_task_id, durable_run_id)) = durable_ids {
                    return Ok(format!(
                        "Task queued\nTask: {}\nRun: {}\nStatus: queued\nCompatibility task: #{}\nScheduler job: {}",
                        durable_task_id, durable_run_id, task_id, submitted.job_id
                    ));
                }
                return Ok(format!(
                    "Task #{} queued as scheduler job {}",
                    task_id, submitted.job_id
                ));
            }

            let task_id = self
                .store
                .lock()
                .await
                .create_task(
                    description.clone(),
                    prompt.clone(),
                    agent.clone(),
                    self.parent_session_id.clone(),
                    denied_tools.clone(),
                    allowed_paths.clone(),
                )
                .await;

            if let Some(ref spawner) = self.spawner {
                let req = SubAgentRequest {
                    task_id,
                    run_id: None,
                    description: description.clone(),
                    prompt,
                    agent,
                    parent_id: self.parent_session_id.clone(),
                    parent_run_id: self.parent_run_id.clone(),
                    denied_tools,
                    allowed_paths,
                    depth: self.depth + 1,
                    max_tool_calls: None,
                    parent_model: self.parent_model.clone(),
                    workspace_root: self.workspace_root.clone(),
                };
                spawner.send_async(req).await.map_err(|e| {
                    ToolError::Execution(format!("failed to queue subagent: {}", e))
                })?;

                self.store
                    .lock()
                    .await
                    .update_status(task_id, TaskStatus::Running)
                    .await;

                Ok(format!(
                    "<task_result>\nSubagent spawned for task: {}\nTask ID: {}\nStatus: running\nParent session: {}\n</task_result>",
                    description,
                    task_id,
                    self.parent_session_id.as_deref().unwrap_or("none")
                ))
            } else {
                self.store
                    .lock()
                    .await
                    .set_result(
                        task_id,
                        "Subagent spawner not configured - task queued but not executed"
                            .to_string(),
                    )
                    .await;

                Ok(format!(
                    "<task_result>\nSubagent queued for task: {}\nTask ID: {}\nStatus: pending (no spawner configured)\nParent session: {}\n</task_result>",
                    description,
                    task_id,
                    self.parent_session_id.as_deref().unwrap_or("none")
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::next_durable_depth;

    #[test]
    fn durable_depth_enforces_last_allowed_level() {
        assert_eq!(next_durable_depth(None, 1), Ok(1));
        assert_eq!(next_durable_depth(Some(1), 2), Ok(2));
        assert!(next_durable_depth(Some(2), 2).is_err());
    }
}

fn parse_join_policy(value: &str) -> Result<RunJoinPolicy, ToolError> {
    match value {
        "all" => Ok(RunJoinPolicy::All),
        "any_successful" => Ok(RunJoinPolicy::AnySuccessful),
        "first_completed" => Ok(RunJoinPolicy::FirstCompleted),
        "detached" => Ok(RunJoinPolicy::Detached),
        other => Err(ToolError::Execution(format!(
            "unknown join_policy '{other}'"
        ))),
    }
}

fn format_group_summary(
    summary: &AgentRunGroupSummary,
    timed_out: bool,
    rejected: &[String],
) -> String {
    let mut lines = vec![
        format!("Group: {}", summary.group.group_id),
        format!("Status: {}", summary.group.status.as_str()),
        format!("Policy: {}", summary.group.join_policy.as_str()),
        format!(
            "Members: {} (successful {}, failed {}, active {})",
            summary.members.len(),
            summary.successful,
            summary.failed,
            summary.active
        ),
    ];
    if timed_out {
        lines.push("Timed out: true (group remains active)".into());
    }
    if let Some(winner) = &summary.group.winner_run_id {
        lines.push(format!("Winner: {winner}"));
    }
    for member in &summary.members {
        lines.push(format!(
            "- {}: {}{}",
            member.run_id,
            member.status.as_str(),
            member
                .result_ref
                .as_deref()
                .map(|value| format!(", result {value}"))
                .unwrap_or_default()
        ));
    }
    if !rejected.is_empty() {
        lines.push(format!("Rejected children: {}", rejected.len()));
    }
    lines.join("\n")
}

fn delegation_key(
    session_id: Option<&str>,
    turn_id: Option<&str>,
    agent: &str,
    prompt: &str,
    allowed_paths: &[String],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session_id.unwrap_or_default().as_bytes());
    hasher.update([0]);
    hasher.update(turn_id.unwrap_or_default().as_bytes());
    hasher.update([0]);
    hasher.update(agent.as_bytes());
    hasher.update([0]);
    hasher.update(prompt.as_bytes());
    for path in allowed_paths {
        hasher.update([0]);
        hasher.update(path.as_bytes());
    }
    format!("agent-delegation-{:x}", hasher.finalize())
}

fn authority_digest(denied_tools: &[String], allowed_paths: &[String]) -> String {
    let mut hasher = Sha256::new();
    for value in denied_tools.iter().chain(allowed_paths.iter()) {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}
