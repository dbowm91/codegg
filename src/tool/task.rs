use async_trait::async_trait;
use codegg_core::jobs::ExecutionTarget;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

const MAX_REQUESTED_CHILD_TOOL_CALLS: u32 = 128;

async fn create_agent_task_with_busy_retry(
    store: &dyn AgentRunStore,
    task: NewAgentTask,
    run: NewAgentRun,
) -> Result<AgentRunSubmission, AgentRunStoreError> {
    const RETRIES: usize = 8;
    for attempt in 0..RETRIES {
        match store.create_or_get(task.clone(), run.clone()).await {
            Ok(submission) => return Ok(submission),
            Err(error) if attempt + 1 < RETRIES && is_transient_sqlite_lock(&error.to_string()) => {
                let delay_ms = 25_u64.saturating_mul(1_u64 << attempt.min(6));
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("the final retry either returns or exits with its error")
}

fn is_transient_sqlite_lock(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();
    normalized.contains("database is locked")
        || normalized.contains("database table is locked")
        || normalized.contains("sqlite_busy")
        || normalized.contains("sqlite busy")
}

use crate::agent::run_control::{ControlActor, ControlOutcome, RunControlService};
use crate::agent::worker::{SubAgentRequest, SubAgentSpawner};
use crate::bus::global::GlobalEventBus;
use crate::error::ToolError;
use crate::tool::{StructuredToolResult, Tool, ToolCategory, ToolExecutionContext};
use codegg_core::agent_convergence::ConvergenceStore;
use codegg_core::agent_run::{
    AgentRunBudget, AgentRunStore, AgentRunStoreError, AgentRunSubmission, NewAgentRun,
    NewAgentTask,
};
use codegg_core::agent_run_control::AgentRunControlKind;
use codegg_core::agent_run_group::{
    AgentRunGroupOwner, AgentRunGroupService, AgentRunGroupSummary, GroupActor, NewAgentRunGroup,
    RunJoinPolicy, MAX_GROUP_MEMBERS,
};
use codegg_core::identity::{
    AgentRunGroupId, AgentRunId, AgentTaskId, ProjectId, RepositoryId, WorkspaceId,
    WorkstreamClaimId,
};
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

#[derive(Clone)]
pub struct TaskTool {
    store: Arc<Mutex<TaskStore>>,
    spawner: Option<SubAgentSpawner>,
    parent_session_id: Option<String>,
    denied_tools: Vec<String>,
    depth: usize,
    parent_model: Option<String>,
    submission: Option<Arc<crate::scheduler::JobSubmissionService>>,
    workspace_root: Option<std::path::PathBuf>,
    workspace_locks: Option<Arc<codegg_core::workspace_services::WorkspaceLockTable>>,
    parent_allowed_paths: Vec<String>,
    agent_runs: Option<Arc<dyn AgentRunStore>>,
    project_id: Option<ProjectId>,
    repository_id: Option<RepositoryId>,
    parent_turn_id: Option<String>,
    run_control: Option<Arc<RunControlService>>,
    parent_run_id: Option<AgentRunId>,
    run_groups: Option<Arc<AgentRunGroupService>>,
    workstream_claims: Option<Arc<codegg_core::workstream_claim::WorkstreamClaimStore>>,
    workspace_id: Option<WorkspaceId>,
    orchestration_owner: Option<AgentOrchestrationOwner>,
    max_depth: u32,
    convergence_store: Option<Arc<dyn ConvergenceStore>>,
    orchestration_config: crate::config::schema::OrchestrationConfig,
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
    fn workstream_actor(
        &self,
        ctx: Option<&ToolExecutionContext>,
    ) -> Result<codegg_core::workstream_claim::ClaimActor, ToolError> {
        if let Some(run_id) = &self.parent_run_id {
            return Ok(codegg_core::workstream_claim::ClaimActor::Run(
                run_id.clone(),
            ));
        }
        let principal = ctx
            .and_then(|ctx| ctx.origin_principal.as_deref())
            .and_then(|value| codegg_core::identity::PrincipalId::parse(value).ok())
            .ok_or_else(|| {
                ToolError::Execution("workstream action requires a bound group actor".into())
            })?;
        Ok(codegg_core::workstream_claim::ClaimActor::Principal(
            principal,
        ))
    }

    async fn execute_workstream_action(
        &self,
        input: &serde_json::Value,
        ctx: Option<&ToolExecutionContext>,
    ) -> Result<String, ToolError> {
        let groups = self
            .run_groups
            .as_ref()
            .ok_or_else(|| ToolError::Execution("durable run groups are unavailable".into()))?;
        let claims = self.workstream_claims.as_ref().ok_or_else(|| {
            ToolError::Execution("durable workstream claims are unavailable".into())
        })?;
        let owner = self.orchestration_owner.as_ref().ok_or_else(|| {
            ToolError::Execution("workstream actions require a bound group actor".into())
        })?;
        let group_actor = match owner {
            AgentOrchestrationOwner::Run { run_id } => GroupActor {
                run_id: Some(run_id.clone()),
                session_id: None,
                turn_id: None,
            },
            AgentOrchestrationOwner::Turn {
                session_id,
                turn_id,
            } => GroupActor {
                run_id: None,
                session_id: Some(session_id.clone()),
                turn_id: Some(turn_id.clone()),
            },
        };
        let actor = self.workstream_actor(ctx)?;
        let action = input["action"].as_str().unwrap_or_default();
        let group_id =
            AgentRunGroupId::parse(input["group_id"].as_str().ok_or_else(|| {
                ToolError::Execution("workstream action requires group_id".into())
            })?)
            .map_err(|error| ToolError::Execution(format!("invalid group_id: {error}")))?;

        match action {
            "board" => {
                let summary = groups
                    .status(&group_actor, group_id.clone())
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                let run_store = self.agent_runs.as_ref().ok_or_else(|| {
                    ToolError::Execution("durable agent runs are unavailable".into())
                })?;
                let mut scope: Option<(ProjectId, WorkspaceId)> = None;
                let mut snapshot = None;
                let mut summaries = Vec::with_capacity(summary.members.len());
                for member in &summary.members {
                    let run = run_store
                        .get_run(&member.run_id)
                        .await
                        .map_err(|error| ToolError::Execution(error.to_string()))?
                        .ok_or_else(|| {
                            ToolError::Execution("group member run is unavailable".into())
                        })?;
                    let task = run_store
                        .get_task(&run.task_id)
                        .await
                        .map_err(|error| ToolError::Execution(error.to_string()))?
                        .ok_or_else(|| {
                            ToolError::Execution("group member task is unavailable".into())
                        })?;
                    if let Some((project_id, workspace_id)) = &scope {
                        if project_id != &task.project_id || workspace_id != &task.workspace_id {
                            return Err(ToolError::Execution(
                                "group members span multiple project/workspace scopes".into(),
                            ));
                        }
                    } else {
                        scope = Some((task.project_id.clone(), task.workspace_id.clone()));
                    }
                    summaries.push(codegg_core::projection_replay::agent_run_summary(
                        &task,
                        &run,
                        None,
                        None,
                        Some(group_id.as_str()),
                    ));
                    if snapshot.is_none() {
                        snapshot = Some(
                            codegg_protocol::projection::SessionProjectionSnapshot::empty(
                                task.originating_session_id.as_str(),
                                task.project_id.as_str(),
                                task.workspace_id.as_str(),
                            ),
                        );
                    }
                    if let (Some(job_id), Some(submission)) =
                        (run.job_id.as_ref(), self.submission.as_ref())
                    {
                        let job = submission
                            .get_job(job_id)
                            .await
                            .map_err(|error| ToolError::Execution(error.to_string()))?;
                        if let Some(job) = job.filter(|job| {
                            job.workspace_id == task.workspace_id
                                && snapshot.as_ref().is_some_and(|snapshot| {
                                    job.workspace_id.as_str() == snapshot.workspace_id
                                })
                        }) {
                            let snapshot = snapshot.as_mut().expect("member initialized snapshot");
                            if snapshot.jobs.len()
                                < codegg_protocol::projection::MAX_PROJECTION_JOBS
                            {
                                snapshot.jobs.push_back(
                                    codegg_protocol::projection::dto::JobProjection {
                                        job_id: job.job_id.to_string(),
                                        workspace_id: job.workspace_id.to_string(),
                                        kind: job.kind.as_str().to_owned(),
                                        state: job.state.as_str().to_owned(),
                                        // Group members receive scheduler-owned
                                        // phase only; never command, summary,
                                        // environment, log or artifact data.
                                        summary: String::new(),
                                        session_id: None,
                                        turn_id: None,
                                        active_attempt_id: job
                                            .current_attempt_id
                                            .as_ref()
                                            .map(ToString::to_string),
                                        error_class: None,
                                        updated_at: job.updated_at.timestamp_millis(),
                                    },
                                );
                            }
                        }
                    }
                }
                let mut snapshot = snapshot.ok_or_else(|| {
                    ToolError::Execution("group has no observable member runs".into())
                })?;
                snapshot
                    .run_groups
                    .push_back(codegg_core::projection_replay::run_group_summary(
                        &summary,
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|duration| duration.as_millis() as i64)
                            .unwrap_or_default(),
                    ));
                snapshot.agent_runs.extend(summaries);
                let board = claims
                    .project_group(&snapshot, &group_id, &actor)
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                let (project_id, workspace_id) = self.workstream_scope(&group_id).await?;
                let events = claims
                    .list_events(&group_id, &project_id, &workspace_id, &actor)
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                let handoff_requests = events
                    .iter()
                    .filter(|event| event.action == "handoff_requested")
                    .rev()
                    .take(16)
                    .map(|event| {
                        let (requester_kind, requester_id) = match &event.actor {
                            codegg_core::workstream_claim::ClaimActor::Principal(id) => {
                                ("principal", id.to_string())
                            }
                            codegg_core::workstream_claim::ClaimActor::Run(id) => {
                                ("run", id.to_string())
                            }
                        };
                        serde_json::json!({
                            "claim_id": event.claim_id,
                            "requester_kind": requester_kind,
                            "requester_id": requester_id,
                            "claim_version": event.version,
                            "created_at": event.created_at
                        })
                    })
                    .collect::<Vec<_>>();
                let mut board = serde_json::to_value(&board)
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                if let Some(object) = board.as_object_mut() {
                    object.remove("revision");
                    object.insert(
                        "continuity".into(),
                        serde_json::Value::String("snapshot_only".into()),
                    );
                    object.insert(
                        "source_revision_available".into(),
                        serde_json::Value::Bool(false),
                    );
                    object.insert(
                        "handoff_requests".into(),
                        serde_json::Value::Array(handoff_requests),
                    );
                }
                serde_json::to_string(&board)
                    .map_err(|error| ToolError::Execution(error.to_string()))
            }
            "claim" => {
                let (project_id, workspace_id) = self.workstream_scope(&group_id).await?;
                let target_value = input["target"]
                    .as_str()
                    .ok_or_else(|| ToolError::Execution("claim requires a target".into()))?;
                let target = match input["target_kind"].as_str() {
                    Some("task") => codegg_core::workstream_claim::ClaimTarget::Task(
                        AgentTaskId::parse(target_value).map_err(|error| {
                            ToolError::Execution(format!("invalid task target: {error}"))
                        })?,
                    ),
                    Some("path") => {
                        codegg_core::workstream_claim::ClaimTarget::Path(target_value.into())
                    }
                    Some("interface") => {
                        codegg_core::workstream_claim::ClaimTarget::Interface(target_value.into())
                    }
                    _ => {
                        return Err(ToolError::Execution(
                            "target_kind must be task, path, or interface".into(),
                        ))
                    }
                };
                let idempotency_key = input["claim_idempotency_key"]
                    .as_str()
                    .filter(|value| !value.is_empty() && value.len() <= 128)
                    .ok_or_else(|| {
                        ToolError::Execution(
                            "claim requires a bounded claim_idempotency_key".into(),
                        )
                    })?;
                let claim_actor = actor.clone();
                let claim = claims
                    .claim(codegg_core::workstream_claim::NewWorkstreamClaim {
                        project_id,
                        workspace_id,
                        group_id,
                        actor,
                        target,
                        idempotency_key: idempotency_key.into(),
                        lease_expires_at: None,
                    })
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                let group = groups
                    .status(&group_actor, claim.group_id.clone())
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                self.publish_workstream_board_projection(&group, &claim_actor)
                    .await;
                let projection = claim
                    .to_projection()
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                self.publish_workstream_claim_update(&group, projection.clone(), "claimed");
                serde_json::to_string(&projection)
                    .map_err(|error| ToolError::Execution(error.to_string()))
            }
            "release_claim" => {
                let claim_id =
                    WorkstreamClaimId::parse(input["claim_id"].as_str().unwrap_or_default())
                        .map_err(|error| {
                            ToolError::Execution(format!("invalid claim_id: {error}"))
                        })?;
                let expected_version = input["expected_version"].as_u64().ok_or_else(|| {
                    ToolError::Execution("release_claim requires expected_version".into())
                })?;
                let expected_lease_generation =
                    input["expected_lease_generation"].as_u64().ok_or_else(|| {
                        ToolError::Execution(
                            "release_claim requires expected_lease_generation".into(),
                        )
                    })?;
                let claim_actor = actor.clone();
                let claim = claims
                    .transition(codegg_core::workstream_claim::ClaimCas {
                        claim_id,
                        actor,
                        expected_version,
                        expected_lease_generation,
                        transition: codegg_core::workstream_claim::ClaimTransition::Release,
                        new_owner: None,
                        lease_expires_at: None,
                    })
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                let group = groups
                    .status(&group_actor, claim.group_id.clone())
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                self.publish_workstream_board_projection(&group, &claim_actor)
                    .await;
                let projection = claim
                    .to_projection()
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                self.publish_workstream_claim_update(&group, projection.clone(), "released");
                serde_json::to_string(&projection)
                    .map_err(|error| ToolError::Execution(error.to_string()))
            }
            "request_handoff" => {
                let claim_id =
                    WorkstreamClaimId::parse(input["claim_id"].as_str().unwrap_or_default())
                        .map_err(|error| {
                            ToolError::Execution(format!("invalid claim_id: {error}"))
                        })?;
                claims
                    .request_handoff(&claim_id, &actor)
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                let (project_id, workspace_id) = self.workstream_scope(&group_id).await?;
                let claim = claims
                    .list_group(&group_id, &project_id, &workspace_id, &actor)
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?
                    .into_iter()
                    .find(|claim| claim.claim_id == claim_id)
                    .ok_or_else(|| ToolError::Execution("claim no longer exists".into()))?;
                let group = groups
                    .status(&group_actor, group_id)
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                self.publish_workstream_board_projection(&group, &actor)
                    .await;
                let projection = claim
                    .to_projection()
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                self.publish_workstream_claim_update(&group, projection, "handoff_requested");
                Ok(format!(
                    "Handoff requested for claim {claim_id}; ownership is unchanged."
                ))
            }
            _ => Err(ToolError::Execution("unsupported workstream action".into())),
        }
    }

    async fn workstream_scope(
        &self,
        group_id: &AgentRunGroupId,
    ) -> Result<(ProjectId, WorkspaceId), ToolError> {
        let groups = self
            .run_groups
            .as_ref()
            .ok_or_else(|| ToolError::Execution("durable run groups are unavailable".into()))?;
        let owner = self.orchestration_owner.as_ref().ok_or_else(|| {
            ToolError::Execution("workstream actions require a bound group actor".into())
        })?;
        let group_actor = match owner {
            AgentOrchestrationOwner::Run { run_id } => GroupActor {
                run_id: Some(run_id.clone()),
                session_id: None,
                turn_id: None,
            },
            AgentOrchestrationOwner::Turn {
                session_id,
                turn_id,
            } => GroupActor {
                run_id: None,
                session_id: Some(session_id.clone()),
                turn_id: Some(turn_id.clone()),
            },
        };
        let summary = groups
            .status(&group_actor, group_id.clone())
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?;
        let run_id = summary
            .members
            .first()
            .ok_or_else(|| ToolError::Execution("group has no members".into()))?
            .run_id
            .clone();
        let run_store = self
            .agent_runs
            .as_ref()
            .ok_or_else(|| ToolError::Execution("durable agent runs are unavailable".into()))?;
        let run = run_store
            .get_run(&run_id)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?
            .ok_or_else(|| ToolError::Execution("group member run is unavailable".into()))?;
        let task = run_store
            .get_task(&run.task_id)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?
            .ok_or_else(|| ToolError::Execution("group member task is unavailable".into()))?;
        Ok((task.project_id, task.workspace_id))
    }

    fn publish_workstream_claim_update(
        &self,
        group: &AgentRunGroupSummary,
        claim: codegg_protocol::projection::WorkstreamClaimProjection,
        action: &str,
    ) {
        let Some(session_id) = self.parent_session_id.clone() else {
            return;
        };
        crate::bus::global::GlobalEventBus::publish(
            crate::bus::events::AppEvent::WorkstreamClaimUpdated {
                session_id,
                group_id: group.group.group_id.to_string(),
                member_run_ids: group
                    .group
                    .member_run_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                claim,
                action: action.to_owned(),
            },
        );
    }

    async fn publish_workstream_board_projection(
        &self,
        group: &AgentRunGroupSummary,
        actor: &codegg_core::workstream_claim::ClaimActor,
    ) {
        let (Some(session_id), Some(claim_store)) = (
            self.parent_session_id.clone(),
            self.workstream_claims.as_ref(),
        ) else {
            return;
        };
        let group_id = &group.group.group_id;
        let Ok((project_id, workspace_id)) = self.workstream_scope(group_id).await else {
            return;
        };
        let Ok(claims) = claim_store
            .list_group(group_id, &project_id, &workspace_id, actor)
            .await
        else {
            return;
        };
        let Ok(dependencies) = claim_store
            .list_dependencies(group_id, &project_id, &workspace_id, actor)
            .await
        else {
            return;
        };
        let mut projection = codegg_core::projection_replay::run_group_summary(
            group,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_millis() as i64)
                .unwrap_or_default(),
        );
        projection.workstream_claims = claims
            .iter()
            .filter_map(|claim| claim.to_projection().ok())
            .take(codegg_protocol::projection::MAX_WORKSTREAM_CLAIMS)
            .collect();
        projection.workstream_dependencies = dependencies
            .iter()
            .map(codegg_core::workstream_claim::WorkstreamClaimDependency::to_projection)
            .take(codegg_protocol::projection::MAX_WORKSTREAM_DEPENDENCIES)
            .collect();
        projection.workstream_claims_updated_at = Some(projection.updated_at);
        crate::bus::global::GlobalEventBus::publish(
            crate::bus::events::AppEvent::AgentRunGroupUpdated {
                session_id,
                group: projection,
            },
        );
    }

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

    pub(crate) fn publish_convergence_projection(
        &self,
        summary: &codegg_core::agent_convergence::ConvergenceSummary,
    ) {
        let Some(session_id) = self.parent_session_id.clone() else {
            return;
        };
        let counts = &summary.producer_status_counts;
        let projection = codegg_protocol::projection::dto::ConvergenceSummaryProjection {
            convergence_id: summary.convergence_id.clone(),
            owner_summary: summary.owner_summary.clone(),
            status: summary.status.as_str().to_string(),
            cycle_ordinal: summary.cycle_ordinal,
            max_cycles: summary.max_cycles,
            remaining_cycles: summary.remaining_cycles,
            producer_run_ids: summary.producer_run_ids.clone(),
            producer_completed: counts.completed,
            producer_failed: counts.failed,
            producer_cancelled: counts.cancelled,
            producer_active: counts.active,
            verifier_run_id: summary.verifier_run_id.clone(),
            verdict_kind: summary.verdict_kind.clone(),
            verdict_summary: summary.verdict_summary.clone(),
            awaiting_decision: summary.awaiting_decision,
            terminal_reason_class: summary.terminal_reason_class.clone(),
            selected_run_id: summary.selected_run_id.clone(),
            selected_result_commit: summary.selected_result_commit.clone(),
            last_finding_count: summary.last_finding_count,
        };
        GlobalEventBus::publish(crate::bus::events::AppEvent::ConvergenceUpdated {
            session_id,
            convergence: projection,
        });
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
            workspace_locks: None,
            parent_allowed_paths: Vec::new(),
            agent_runs: None,
            project_id: None,
            repository_id: None,
            parent_turn_id: None,
            run_control: None,
            parent_run_id: None,
            run_groups: None,
            workstream_claims: None,
            workspace_id: None,
            orchestration_owner: None,
            max_depth: 3,
            convergence_store: None,
            orchestration_config: Default::default(),
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
            workspace_locks: None,
            parent_allowed_paths: Vec::new(),
            agent_runs: None,
            project_id: None,
            repository_id: None,
            parent_turn_id: None,
            run_control: None,
            parent_run_id: None,
            run_groups: None,
            workstream_claims: None,
            workspace_id: None,
            orchestration_owner: None,
            max_depth: 3,
            convergence_store: None,
            orchestration_config: Default::default(),
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

    pub fn with_workstream_claim_store(
        mut self,
        store: Option<Arc<codegg_core::workstream_claim::WorkstreamClaimStore>>,
    ) -> Self {
        self.workstream_claims = store;
        self
    }

    pub fn with_workspace_id(mut self, workspace_id: Option<WorkspaceId>) -> Self {
        self.workspace_id = workspace_id;
        self
    }

    pub fn with_convergence_store(mut self, store: Option<Arc<dyn ConvergenceStore>>) -> Self {
        self.convergence_store = store;
        self
    }

    pub fn with_orchestration_config(
        mut self,
        config: Option<crate::config::schema::OrchestrationConfig>,
    ) -> Self {
        self.orchestration_config = config.unwrap_or_default().bounded();
        self
    }

    pub(crate) fn with_additional_denied_tools(mut self, tools: &[&str]) -> Self {
        self.denied_tools
            .extend(tools.iter().map(|tool| (*tool).to_string()));
        self.denied_tools.sort();
        self.denied_tools.dedup();
        self
    }

    pub(crate) fn control_actor(&self) -> ControlActor {
        ControlActor {
            session_id: self.parent_session_id.clone(),
            run_id: self.parent_run_id.clone(),
            turn_id: self.parent_turn_id.clone(),
        }
    }

    pub(crate) fn agent_run_store(&self) -> Option<Arc<dyn AgentRunStore>> {
        self.agent_runs.clone()
    }

    pub(crate) fn run_control(&self) -> Option<Arc<RunControlService>> {
        self.run_control.clone()
    }

    pub(crate) fn convergence_coordinator(
        &self,
    ) -> Option<crate::agent::convergence::ConvergenceCoordinator> {
        self.convergence_store.as_ref().map(|store| {
            crate::agent::convergence::ConvergenceCoordinator::new(store.clone(), self.clone())
        })
    }

    pub(crate) async fn execute_convergence_child(
        &self,
        input: serde_json::Value,
        call_identity: String,
    ) -> Result<String, ToolError> {
        Box::pin(self.execute_impl(input, None, call_identity, None)).await
    }

    pub(crate) async fn execute_convergence_child_from_base(
        &self,
        input: serde_json::Value,
        call_identity: String,
        base_commit: String,
    ) -> Result<String, ToolError> {
        Box::pin(self.execute_impl(input, None, call_identity, Some(base_commit))).await
    }

    pub(crate) fn orchestration_owner(&self) -> Option<AgentOrchestrationOwner> {
        self.orchestration_owner.clone()
    }

    pub(crate) fn workspace_root(&self) -> Option<std::path::PathBuf> {
        self.workspace_root.clone()
    }

    pub(crate) fn repository_id(&self) -> Option<RepositoryId> {
        self.repository_id.clone()
    }

    pub(crate) fn orchestration_config(&self) -> &crate::config::schema::OrchestrationConfig {
        &self.orchestration_config
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

    pub fn with_workspace_locks(
        mut self,
        locks: Option<Arc<codegg_core::workspace_services::WorkspaceLockTable>>,
    ) -> Self {
        self.workspace_locks = locks;
        self
    }

    pub fn store(&self) -> Arc<Mutex<TaskStore>> {
        self.store.clone()
    }

    pub fn depth(&self) -> usize {
        self.depth
    }

    async fn execute_with_context(
        &self,
        input: serde_json::Value,
        ctx: Option<&ToolExecutionContext>,
    ) -> Result<String, ToolError> {
        let call_identity = resolve_call_identity(&input, ctx);
        self.execute_impl(input, ctx, call_identity, None).await
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

    fn causal_contract(&self) -> Option<crate::tool_advisor::causal_frontier::ToolCausalContract> {
        crate::tool_advisor::causal_frontier::native_causal_contract("task")
    }

    fn description(&self) -> &str {
        "Spawn a subagent to handle a task independently. Use spawn_many for a bounded group of independent children, then wait_group/status_group/cancel_group; prefer wait or push notifications over polling get/status. Mutating durable runs receive a managed isolated worktree before model execution; read-only runs inherit the parent workspace. Child completion never merges into the parent automatically—inspect the structured result and request explicit typed integration."
    }

    fn parameters(&self) -> serde_json::Value {
        let spawn_many_item = json!({
            "type": "object",
            "properties": {
                "description": { "type": "string" },
                "prompt": { "type": "string" },
                "agent": { "type": "string" },
                "model": { "type": "string" },
                "allowed_paths": { "type": "array", "items": { "type": "string" } },
                "max_tool_calls": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_REQUESTED_CHILD_TOOL_CALLS
                },
                "shared_scratch": {
                    "type": "array",
                    "maxItems": 8,
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "maxLength": 128 },
                            "alias": { "type": "string", "maxLength": 32 }
                        },
                        "required": ["id", "alias"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["description", "prompt"]
        });
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action: create/revoke parent-owned shared scratch, spawn children, converge, manage groups/runs, or manage workstream claims",
                    "enum": ["scratch_create", "scratch_revoke", "spawn", "converge", "convergence_status", "convergence_decide", "convergence_cancel", "spawn_many", "create_group", "status", "get", "message", "interrupt", "wait", "cancel", "status_group", "wait_group", "cancel_group", "board", "claim", "release_claim", "request_handoff"]
                },
                "scratch_alias": { "type": "string", "maxLength": 32, "description": "Bounded env-safe name for a parent-owned shared scratch resource (action=scratch_create)" },
                "scratch_id": { "type": "string", "description": "Opaque ScratchId, never a path (action=scratch_revoke or spawn shared_scratch)" },
                "shared_scratch": {
                    "type": "array",
                    "maxItems": 8,
                    "description": "Explicit parent-owned ScratchId grants for this child; each entry requires id and alias",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "maxLength": 128 },
                            "alias": { "type": "string", "maxLength": 32 }
                        },
                        "required": ["id", "alias"],
                        "additionalProperties": false
                    }
                },
                "objective": { "type": "string", "description": "Bounded convergence objective (action=converge)" },
                "criteria": { "type": "array", "maxItems": 32, "items": { "type": "string" }, "description": "Bounded acceptance criteria (action=converge)" },
                "producer": { "type": "object", "description": "Exactly one producer request (action=converge)" },
                "verifier_agent": { "type": "string", "description": "Read-only verifier agent (action=converge)" },
                "verifier_model": { "type": "string", "description": "Optional verifier model override" },
                "max_cycles": { "type": "integer", "minimum": 1, "maximum": 4, "description": "Bounded convergence cycles (default: configured 2)" },
                "max_producers_per_cycle": { "type": "integer", "minimum": 1, "maximum": 3, "description": "Host-enforced producer width; single strategy currently uses one" },
                "strategy": { "type": "string", "enum": ["single"], "description": "Bounded convergence producer strategy" },
                "replan_base": { "type": "string", "enum": ["original", "last_clean_result"], "description": "Owner-selected validated replan base" },
                "convergence_id": { "type": "string", "description": "Durable convergence id" },
                "decision": { "type": "string", "enum": ["accept", "stop", "escalate", "repair", "replan"] },
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
                "model": {
                    "type": "string",
                    "description": "Optional model override for the child"
                },
                "max_tool_calls": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_REQUESTED_CHILD_TOOL_CALLS,
                    "description": "Optional host-enforced tool-call cap for this child (action=spawn)"
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
                    "description": "Bounded child requests for action=spawn_many. Each item has description, prompt, optional agent, model, allowed_paths, max_tool_calls, and explicitly granted shared_scratch.",
                    "items": spawn_many_item
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
                },
                "claim_id": {
                    "type": "string",
                    "description": "Durable WorkstreamClaimId (action=release_claim or request_handoff)"
                },
                "expected_version": { "type": "integer", "minimum": 1 },
                "expected_lease_generation": { "type": "integer", "minimum": 1 },
                "target_kind": { "type": "string", "enum": ["task", "path", "interface"] },
                "target": { "type": "string", "maxLength": 256 },
                "claim_idempotency_key": { "type": "string", "maxLength": 128 }
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
        self.execute_with_context(input, None).await
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let output = self.execute_with_context(input, ctx.as_ref()).await?;
        Ok(StructuredToolResult::legacy(self.name(), output))
    }
}

impl TaskTool {
    async fn execute_impl(
        &self,
        input: serde_json::Value,
        ctx: Option<&ToolExecutionContext>,
        call_identity: String,
        explicit_base_commit: Option<String>,
    ) -> Result<String, ToolError> {
        let action = input["action"].as_str().unwrap_or("spawn");

        if matches!(action, "scratch_create" | "scratch_revoke") {
            let agent_runs = self.agent_runs.as_ref().ok_or_else(|| {
                ToolError::Execution("durable scratch ownership is unavailable".into())
            })?;
            let (session_id, owner_run_id) = match self.orchestration_owner.as_ref() {
                Some(AgentOrchestrationOwner::Turn { session_id, .. }) => {
                    (session_id.clone(), None)
                }
                Some(AgentOrchestrationOwner::Run { run_id }) => (
                    self.parent_session_id.clone().unwrap_or_default(),
                    Some(run_id.clone()),
                ),
                None => {
                    return Err(ToolError::Execution(
                        "shared scratch requires a host-owned session or run parent".into(),
                    ));
                }
            };
            return if action == "scratch_create" {
                let alias = input["scratch_alias"].as_str().ok_or_else(|| {
                    ToolError::Execution("scratch_create requires scratch_alias".into())
                })?;
                let resource = agent_runs
                    .create_scratch_resource(
                        &session_id,
                        owner_run_id.as_ref(),
                        alias,
                        &call_identity,
                    )
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                Ok(format!(
                    "Shared scratch created\nScratchId: {}\nAlias: {}\nShare only with direct child runs by including this id and a child-local alias in shared_scratch.",
                    resource.scratch_id, resource.alias
                ))
            } else {
                let id = input["scratch_id"]
                    .as_str()
                    .ok_or_else(|| {
                        ToolError::Execution("scratch_revoke requires scratch_id".into())
                    })
                    .and_then(|value| {
                        codegg_core::identity::ScratchId::parse(value)
                            .map_err(|error| ToolError::Execution(error.to_string()))
                    })?;
                let resource = agent_runs
                    .revoke_scratch_resource(&id, &session_id, owner_run_id.as_ref())
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                if !agent_runs
                    .has_scratch_leases(&resource.scratch_id)
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?
                {
                    crate::scratch::reclaim_shared_scratch_dir(&session_id, &resource.scratch_id)
                        .map_err(|error| ToolError::Execution(error.to_string()))?;
                }
                Ok(format!("Shared scratch revoked\nScratchId: {id}"))
            };
        }

        if matches!(
            action,
            "board" | "claim" | "release_claim" | "request_handoff"
        ) {
            return self.execute_workstream_action(&input, ctx).await;
        }

        if matches!(
            action,
            "converge" | "convergence_status" | "convergence_decide" | "convergence_cancel"
        ) {
            let coordinator = self
                .convergence_coordinator()
                .ok_or_else(|| ToolError::Execution("durable convergence is unavailable".into()))?;
            return match action {
                "converge" => coordinator.start(input, call_identity).await,
                "convergence_status" => coordinator.status(&input).await,
                "convergence_decide" => coordinator.decide(&input).await,
                "convergence_cancel" => coordinator.cancel(&input).await,
                _ => {
                    return Err(ToolError::Execution(format!(
                        "unsupported convergence action '{action}'"
                    )));
                }
            };
        }

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
                let owner_key = match &group_owner {
                    AgentRunGroupOwner::Run { run_id } => run_id.to_string(),
                    AgentRunGroupOwner::Turn {
                        session_id,
                        turn_id,
                    } => format!("{session_id}/{turn_id}"),
                };
                let group_id = match input["group_id"].as_str() {
                    Some(value) => AgentRunGroupId::parse(value)
                        .map_err(|e| ToolError::Execution(format!("invalid group_id: {e}")))?,
                    None => {
                        deterministic_group_id(&format!("{call_identity}/{action}/{owner_key}"))
                    }
                };
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
                    for (index, request) in requests.iter().enumerate() {
                        let mut child = request.clone();
                        let object = child.as_object_mut().ok_or_else(|| {
                            ToolError::Execution("each spawn_many request must be an object".into())
                        })?;
                        object.insert("action".into(), serde_json::Value::String("spawn".into()));
                        object.insert(
                            "_fanout_group_id".into(),
                            serde_json::Value::String(group_id.to_string()),
                        );
                        let member_identity = format!("{call_identity}/member/{index}");
                        match Box::pin(self.execute_impl(child, ctx, member_identity, None)).await {
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
                let key = input["idempotency_key"]
                    .as_str()
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| {
                        let owner_key = match &self.orchestration_owner {
                            Some(AgentOrchestrationOwner::Turn {
                                session_id,
                                turn_id,
                            }) => format!("turn:{session_id}:{turn_id}"),
                            Some(AgentOrchestrationOwner::Run { run_id }) => run_id.to_string(),
                            None => owner_run_id.to_string(),
                        };
                        format!("group:{call_identity}:{action}:{owner_key}")
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
                other => {
                    return Err(ToolError::Execution(format!(
                        "unknown group action: {other}"
                    )))
                }
            };
            return match result {
                Ok(summary) => {
                    if let Ok(claim_actor) = self.workstream_actor(ctx) {
                        self.publish_workstream_board_projection(&summary, &claim_actor)
                            .await;
                    } else {
                        self.publish_group_projection(&summary);
                    }
                    Ok(format_group_summary(&summary, false, &[]))
                }
                Err(codegg_core::agent_run_group::AgentRunGroupError::WaitTimedOut(summary)) => {
                    if let Ok(claim_actor) = self.workstream_actor(ctx) {
                        self.publish_workstream_board_projection(&summary, &claim_actor)
                            .await;
                    } else {
                        self.publish_group_projection(&summary);
                    }
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
            let result_run_id = run_id.clone();
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
                        "cancel" => AgentRunControlKind::Cancel,
                        other => {
                            return Err(ToolError::Execution(format!(
                                "unknown run control action: {other}"
                            )))
                        }
                    };
                    let payload = input["message"].as_str().unwrap_or_default().to_string();
                    let key = input["idempotency_key"]
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(|| {
                            format!("control:{call_identity}:{action}:{raw_run_id}")
                        });
                    control.send(&actor, run_id, kind, payload, key).await
                }
                other => {
                    return Err(ToolError::Execution(format!(
                        "unknown run control action: {other}"
                    )))
                }
            }
            .map_err(|e| ToolError::Execution(e.to_string()))?;
            let mut output = format_control_outcome(outcome);
            if matches!(action, "status" | "wait") {
                if let Some(agent_runs) = self.agent_runs.as_ref() {
                    if let Some(result) = agent_runs
                        .get_result(&result_run_id)
                        .await
                        .map_err(|e| ToolError::Execution(e.to_string()))?
                    {
                        output.push_str("\n\nStructured child result\n");
                        output.push_str(&format_agent_run_result(&result));
                    }
                }
            }
            return Ok(output);
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
                    Some(run) => {
                        let Some(parent_session) = self.parent_session_id.as_deref() else {
                            return Ok(format!("Task {} not found", task_id));
                        };
                        let authorized = task_result_visible_to(
                            parent_session,
                            self.parent_turn_id.as_deref(),
                            self.parent_run_id.as_ref(),
                            &task,
                            &run,
                        );
                        if !authorized {
                            return Ok(format!("Task {} not found", task_id));
                        }
                        let result = agent_runs
                            .get_result(&run.run_id)
                            .await
                            .map_err(|e| ToolError::Execution(e.to_string()))?;
                        format_agent_run_task(&task.task_id, &run, result.as_ref())
                    }
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
            let child_model = input["model"]
                .as_str()
                .map(ToOwned::to_owned)
                .or_else(|| self.parent_model.clone());
            let max_tool_calls = requested_max_tool_calls(&input)?;

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
            let shared_scratch = parse_shared_scratch_grants(&input["shared_scratch"])?;
            if !shared_scratch.is_empty() && self.agent_runs.is_none() {
                return Err(ToolError::Execution(
                    "shared scratch grants require durable AgentRun ownership".into(),
                ));
            }

            if let (Some(submission), Some(workspace_root)) =
                (self.submission.clone(), self.workspace_root.clone())
            {
                let workspace_id = submission
                    .workspace_id_for_root(&workspace_root)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                let durable = if let Some(agent_runs) = self.agent_runs.clone() {
                    let delegation_key = delegation_key(&call_identity);
                    let project_id = self.project_id.clone().unwrap_or_default();
                    let provider = child_model
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
                    let request_fingerprint = spawn_request_fingerprint(
                        &agent,
                        &description,
                        &prompt,
                        &allowed_paths,
                        parent_run_id.as_ref(),
                        max_tool_calls,
                        &shared_scratch,
                    );
                    let depth = next_durable_depth(
                        parent_run.as_ref().map(|run| run.depth),
                        self.max_depth,
                    )
                    .map_err(ToolError::Execution)?;
                    let new_task = NewAgentTask {
                        task_id: AgentTaskId::new(),
                        parent_task_id: if let Some(parent_run_id) = parent_run_id.as_ref() {
                            agent_runs
                                .get_run(parent_run_id)
                                .await
                                .ok()
                                .flatten()
                                .map(|run| run.task_id)
                        } else {
                            None
                        },
                        originating_session_id: self.parent_session_id.clone().unwrap_or_default(),
                        originating_turn_id: self.parent_turn_id.clone(),
                        project_id,
                        repository_id: self.repository_id.clone(),
                        workspace_id: workspace_id.clone(),
                        requested_agent: agent.clone(),
                        delegation_key: delegation_key.clone(),
                        request_fingerprint,
                        description: description.clone(),
                    };
                    let new_run = NewAgentRun {
                        run_id: AgentRunId::new(),
                        parent_run_id,
                        workspace_id: workspace_id.clone(),
                        agent_name: agent.clone(),
                        agent_digest: None,
                        provider,
                        model: child_model.clone().unwrap_or_default(),
                        authority_digest,
                        budget: AgentRunBudget {
                            max_tool_calls,
                            max_depth: Some(self.max_depth),
                            ..Default::default()
                        },
                        shared_scratch: shared_scratch.clone(),
                        depth,
                    };
                    let created =
                        create_agent_task_with_busy_retry(agent_runs.as_ref(), new_task, new_run)
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
                                    model: child_model.clone(),
                                    parent_id: self.parent_session_id.clone(),
                                    denied_tools: denied_tools.clone(),
                                    allowed_paths: if allowed_paths.is_empty() {
                                        vec![workspace_root.to_string_lossy().into_owned()]
                                    } else {
                                        allowed_paths.clone()
                                    },
                                    max_tool_calls,
                                    task_id,
                                    run_id,
                                    group_id: input["_fanout_group_id"]
                                        .as_str()
                                        .and_then(|value| AgentRunGroupId::parse(value).ok()),
                                    delegation_key,
                                    approval_mode: ctx
                                        .and_then(|ctx| ctx.permission_mode.as_deref())
                                        .and_then(codegg_core::approval::ApprovalMode::parse),
                                    base_commit: explicit_base_commit.clone(),
                                }
                            } else {
                                codegg_core::jobs::JobPayload::Subagent {
                                    prompt,
                                    agent,
                                    model: child_model.clone(),
                                    parent_id: self.parent_session_id.clone(),
                                    denied_tools: denied_tools.clone(),
                                    allowed_paths: if allowed_paths.is_empty() {
                                        vec![workspace_root.to_string_lossy().into_owned()]
                                    } else {
                                        allowed_paths.clone()
                                    },
                                    max_tool_calls,
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
                            target: ExecutionTarget::default(),
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
                // M005: parent sandbox ceiling comes from the captured batch
                // snapshot threaded through ToolExecutionContext. Approval
                // mode never mutates it; unknown/missing defaults to
                // WorkspaceWrite (never FullHost).
                let parent_sandbox = ctx
                    .and_then(|c| c.sandbox_profile.as_deref())
                    .and_then(codegg_core::approval::SandboxProfile::parse)
                    .unwrap_or(codegg_core::approval::SandboxProfile::WorkspaceWrite);
                let req = SubAgentRequest {
                    task_id,
                    run_id: None,
                    group_id: None,
                    description: description.clone(),
                    prompt,
                    agent,
                    parent_id: self.parent_session_id.clone(),
                    parent_run_id: self.parent_run_id.clone(),
                    denied_tools,
                    allowed_paths,
                    depth: self.depth + 1,
                    max_tool_calls: max_tool_calls.map(|limit| limit as usize),
                    parent_model: child_model,
                    workspace_root: self.workspace_root.clone(),
                    workspace_locks: self.workspace_locks.clone(),
                    parent_sandbox_profile: Some(parent_sandbox),
                    // Inherit parent by default; worker narrows for
                    // read-only/shared paths and enforces the ceiling.
                    sandbox_profile: None,
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
mod identity_tests {
    use super::{deterministic_group_id, format_agent_run_result, next_durable_depth};

    #[test]
    fn durable_depth_enforces_last_allowed_level() {
        assert_eq!(next_durable_depth(None, 1), Ok(1));
        assert_eq!(next_durable_depth(Some(1), 2), Ok(2));
        assert!(next_durable_depth(Some(2), 2).is_err());
    }

    #[test]
    fn fanout_group_identity_is_stable_for_idempotent_dispatch() {
        let first = deterministic_group_id("call-1/spawn_many/session/turn");
        let replay = deterministic_group_id("call-1/spawn_many/session/turn");
        let next_call = deterministic_group_id("call-2/spawn_many/session/turn");
        assert_eq!(first, replay);
        assert_ne!(first, next_call);
    }

    #[test]
    fn structured_run_result_exposes_machine_derived_git_subject() {
        let result = codegg_core::run_result::AgentRunResult {
            run_id: codegg_core::identity::AgentRunId::new(),
            status: codegg_core::run_result::AgentRunResultStatus::Succeeded,
            summary: "producer complete".into(),
            worktree_id: Some(codegg_core::identity::WorktreeId::new()),
            base_commit: Some("base123".into()),
            result_commit: Some("result456".into()),
            changed_paths: vec!["rust/src/lib.rs".into()],
            validation: vec![codegg_core::run_result::ValidationEvidence {
                kind: "cargo test".into(),
                status: codegg_core::run_result::ValidationStatus::Passed,
                summary: "7 passed".into(),
            }],
            findings: Vec::new(),
            artifacts: Vec::new(),
            repository_state: codegg_core::run_result::RepositoryState::Clean,
            retryability: codegg_core::run_result::Retryability::NotRetryable,
            recovery_hint: None,
        };
        let formatted = format_agent_run_result(&result);
        assert!(formatted.contains("Result commit: result456"));
        assert!(formatted.contains("Base commit: base123"));
        assert!(formatted.contains("- rust/src/lib.rs"));
        assert!(formatted.contains("Validation cargo test (Passed): 7 passed"));
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
        lines.push(format!("Rejected child requests: {}", rejected.len()));
        for (index, error) in rejected.iter().enumerate() {
            let mut detail = error.chars().take(512).collect::<String>();
            if error.chars().count() > 512 {
                detail.push('…');
            }
            lines.push(format!("- rejected request {}: {detail}", index + 1));
        }
    }
    lines.join("\n")
}

fn format_agent_run_task(
    task_id: &AgentTaskId,
    run: &codegg_core::agent_run::AgentRunRecord,
    result: Option<&codegg_core::run_result::AgentRunResult>,
) -> String {
    use codegg_core::run_result::AgentRunResult;

    let Some(result) = result else {
        return format!(
            "Task: {task_id}\nRun: {}\nStatus: {}\nResult: {}",
            run.run_id,
            run.status.as_str(),
            run.result_ref.as_deref().unwrap_or("pending")
        );
    };
    let result: AgentRunResult = result.clone().bounded();
    let mut lines = vec![
        format!("Task: {task_id}"),
        format!("Run: {}", run.run_id),
        format!("Status: {}", run.status.as_str()),
    ];
    lines.push(format_agent_run_result(&result));
    lines.join("\n")
}

fn format_agent_run_result(result: &codegg_core::run_result::AgentRunResult) -> String {
    let result = result.clone().bounded();
    let mut lines = vec![
        format!("Outcome: {:?}", result.status).to_lowercase(),
        format!("Summary: {}", result.summary),
        format!("Repository: {:?}", result.repository_state).to_lowercase(),
    ];
    if let Some(worktree_id) = result.worktree_id {
        lines.push(format!("Worktree: {worktree_id}"));
    }
    if let Some(base) = result.base_commit {
        lines.push(format!("Base commit: {base}"));
    }
    if let Some(commit) = result.result_commit {
        lines.push(format!("Result commit: {commit}"));
    }
    if !result.changed_paths.is_empty() {
        lines.push(format!("Changed paths ({}):", result.changed_paths.len()));
        lines.extend(
            result
                .changed_paths
                .into_iter()
                .map(|path| format!("- {path}")),
        );
    }
    for validation in result.validation {
        lines.push(format!(
            "Validation {} ({:?}): {}",
            validation.kind, validation.status, validation.summary
        ));
    }
    if let Some(hint) = result.recovery_hint {
        lines.push(format!("Recovery: {hint}"));
    }
    lines.join("\n")
}

fn task_result_visible_to(
    parent_session: &str,
    parent_turn: Option<&str>,
    parent_run: Option<&AgentRunId>,
    task: &codegg_core::agent_run::AgentTaskRecord,
    run: &codegg_core::agent_run::AgentRunRecord,
) -> bool {
    task.originating_session_id == parent_session
        && if let Some(parent_run) = parent_run {
            run.parent_run_id.as_ref() == Some(parent_run)
        } else {
            run.parent_run_id.is_none()
                && parent_turn.is_none_or(|turn| task.originating_turn_id.as_deref() == Some(turn))
        }
}

fn delegation_key(call_identity: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"agent-delegation-v3");
    hasher.update([0]);
    hasher.update(call_identity.as_bytes());
    hasher.update([0]);
    hasher.update(b"spawn");
    format!("agent-delegation-{:x}", hasher.finalize())
}

fn deterministic_group_id(key: &str) -> AgentRunGroupId {
    let mut hasher = Sha256::new();
    hasher.update(b"agent-run-group-v1");
    hasher.update([0]);
    hasher.update(key.as_bytes());
    let identity = format!("agent-group-{:x}", hasher.finalize());
    AgentRunGroupId::parse(&identity)
        .expect("SHA-256 group identity satisfies the identity contract")
}

fn requested_max_tool_calls(input: &serde_json::Value) -> Result<Option<u32>, ToolError> {
    let Some(value) = input.get("max_tool_calls") else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let Some(limit) = value.as_u64().and_then(|value| u32::try_from(value).ok()) else {
        return Err(ToolError::Execution(
            "max_tool_calls must be an integer between 1 and 128".into(),
        ));
    };
    if !(1..=MAX_REQUESTED_CHILD_TOOL_CALLS).contains(&limit) {
        return Err(ToolError::Execution(
            "max_tool_calls must be an integer between 1 and 128".into(),
        ));
    }
    Ok(Some(limit))
}

fn spawn_request_fingerprint(
    agent: &str,
    description: &str,
    prompt: &str,
    allowed_paths: &[String],
    parent_run_id: Option<&AgentRunId>,
    max_tool_calls: Option<u32>,
    shared_scratch: &[codegg_core::agent_run::SharedScratchGrant],
) -> String {
    let mut normalized_paths = allowed_paths.to_vec();
    normalized_paths.sort();
    let mut hasher = Sha256::new();
    hasher.update(b"spawn-request-v2");
    for value in [agent, description, prompt] {
        hasher.update((value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }
    if let Some(parent_run_id) = parent_run_id {
        hasher.update(parent_run_id.to_string().as_bytes());
    }
    hasher.update(max_tool_calls.unwrap_or_default().to_le_bytes());
    for path in normalized_paths {
        hasher.update((path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
    }
    let mut grants = shared_scratch.to_vec();
    grants.sort_by(|left, right| {
        left.alias
            .to_ascii_lowercase()
            .cmp(&right.alias.to_ascii_lowercase())
            .then_with(|| left.scratch_id.cmp(&right.scratch_id))
    });
    for grant in grants {
        hasher.update(grant.scratch_id.as_str().as_bytes());
        hasher.update([0]);
        hasher.update(grant.alias.to_ascii_lowercase().as_bytes());
        hasher.update([0]);
    }
    format!("spawn-request-{:x}", hasher.finalize())
}

fn parse_shared_scratch_grants(
    value: &serde_json::Value,
) -> Result<Vec<codegg_core::agent_run::SharedScratchGrant>, ToolError> {
    let Some(values) = value.as_array() else {
        if value.is_null() {
            return Ok(Vec::new());
        }
        return Err(ToolError::Execution(
            "shared_scratch must be an array".into(),
        ));
    };
    if values.len() > 8 {
        return Err(ToolError::Execution(
            "shared_scratch exceeds the 8-resource limit".into(),
        ));
    }
    let mut grants = Vec::with_capacity(values.len());
    let mut aliases = std::collections::HashSet::new();
    let mut env_keys = std::collections::HashSet::new();
    let mut ids = std::collections::HashSet::new();
    for value in values {
        let id = value["id"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("shared_scratch entries require an id".into()))?;
        let scratch_id = codegg_core::identity::ScratchId::parse(id)
            .map_err(|error| ToolError::Execution(error.to_string()))?;
        let alias = value["alias"].as_str().ok_or_else(|| {
            ToolError::Execution("shared_scratch entries require an alias".into())
        })?;
        if alias.is_empty()
            || alias.len() > 32
            || !alias
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            || !aliases.insert(alias.to_ascii_lowercase())
            || !env_keys.insert(alias.to_ascii_uppercase().replace('-', "_"))
            || !ids.insert(scratch_id.clone())
        {
            return Err(ToolError::Execution(
                "shared_scratch aliases must be unique bounded ASCII names and ids cannot repeat"
                    .into(),
            ));
        }
        grants.push(codegg_core::agent_run::SharedScratchGrant {
            scratch_id,
            alias: alias.to_owned(),
        });
    }
    Ok(grants)
}

fn resolve_call_identity(input: &serde_json::Value, ctx: Option<&ToolExecutionContext>) -> String {
    let candidate = input["idempotency_key"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            ctx.and_then(|ctx| ctx.invocation_key.as_deref())
                .filter(|value| !value.is_empty())
        });
    let value = candidate
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("legacy-{}", uuid::Uuid::new_v4()));
    if value.len() <= 256 {
        value
    } else {
        format!("identity-digest-{:x}", Sha256::digest(value.as_bytes()))
    }
}

fn authority_digest(denied_tools: &[String], allowed_paths: &[String]) -> String {
    let mut hasher = Sha256::new();
    for value in denied_tools.iter().chain(allowed_paths.iter()) {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_transient_sqlite_lock_errors_are_retried() {
        assert!(is_transient_sqlite_lock(
            "storage failure: database is locked"
        ));
        assert!(is_transient_sqlite_lock(
            "SQLITE_BUSY: database table is locked"
        ));
        assert!(!is_transient_sqlite_lock(
            "delegation key already belongs to another task"
        ));
        assert!(!is_transient_sqlite_lock("permission denied"));
    }

    #[test]
    fn call_identity_prefers_explicit_key_then_context_then_fresh_legacy_key() {
        let mut context = ToolExecutionContext::with_backend(crate::tool::ToolBackendKind::Native);
        context.invocation_key = Some("provider-call-1".into());
        let input = serde_json::json!({
            "action": "message",
            "idempotency_key": "explicit-key"
        });
        assert_eq!(
            resolve_call_identity(&input, Some(&context)),
            "explicit-key"
        );

        let without_explicit = serde_json::json!({"action": "message"});
        assert_eq!(
            resolve_call_identity(&without_explicit, Some(&context)),
            "provider-call-1"
        );
        let first = resolve_call_identity(&without_explicit, None);
        let second = resolve_call_identity(&without_explicit, None);
        assert_ne!(first, second);
    }

    #[test]
    fn task_tool_exposes_bounded_coordination_actions_without_handoff_control() {
        let parameters = TaskTool::default().parameters();
        let actions = parameters["properties"]["action"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        for action in ["board", "claim", "release_claim", "request_handoff"] {
            assert!(actions.contains(&action));
        }
        assert!(!actions.contains(&"handoff"));
        assert_eq!(parameters["properties"]["max_tool_calls"]["minimum"], 1);
        assert_eq!(parameters["properties"]["max_tool_calls"]["maximum"], 128);
        assert_eq!(
            parameters["properties"]["requests"]["items"]["properties"]["max_tool_calls"]
                ["maximum"],
            128
        );
    }

    #[test]
    fn requested_child_tool_call_limit_is_optional_and_bounded() {
        assert_eq!(
            requested_max_tool_calls(&serde_json::json!({})).unwrap(),
            None
        );
        assert_eq!(
            requested_max_tool_calls(&serde_json::json!({"max_tool_calls": 1})).unwrap(),
            Some(1)
        );
        assert_eq!(
            requested_max_tool_calls(&serde_json::json!({"max_tool_calls": 128})).unwrap(),
            Some(128)
        );
        for invalid in [0, 129, -1] {
            assert!(
                requested_max_tool_calls(&serde_json::json!({"max_tool_calls": invalid})).is_err()
            );
        }
        assert!(requested_max_tool_calls(&serde_json::json!({"max_tool_calls": 1.5})).is_err());
    }

    #[test]
    fn workstream_claim_actor_requires_authenticated_origin_context() {
        let tool = TaskTool::default();
        let mut context = ToolExecutionContext::with_backend(crate::tool::ToolBackendKind::Native);
        assert!(tool.workstream_actor(Some(&context)).is_err());

        context.origin_principal = Some("principal-fixture".into());
        assert_eq!(
            tool.workstream_actor(Some(&context)).unwrap(),
            codegg_core::workstream_claim::ClaimActor::Principal(
                codegg_core::identity::PrincipalId::parse("principal-fixture").unwrap()
            )
        );
        // An invalid or untrusted string must never be guessed into an actor.
        context.origin_principal = Some("/tmp/workspace".into());
        assert!(tool.workstream_actor(Some(&context)).is_err());
    }

    #[test]
    fn group_summary_reports_bounded_child_rejection_details() {
        let run_id = AgentRunId::new();
        let group_id = AgentRunGroupId::new();
        let summary = AgentRunGroupSummary {
            group: codegg_core::agent_run_group::AgentRunGroupRecord {
                group_id,
                root_run_id: run_id.clone(),
                owner_run_id: run_id,
                owner: AgentRunGroupOwner::Turn {
                    session_id: "session-fixture".into(),
                    turn_id: "turn-fixture".into(),
                },
                member_run_ids: Vec::new(),
                join_policy: RunJoinPolicy::All,
                cancel_remaining_on_satisfaction: false,
                status: codegg_core::agent_run_group::RunGroupStatus::Running,
                created_at: 0,
                completed_at: None,
                winner_run_id: None,
                idempotency_key: "group-fixture".into(),
            },
            members: Vec::new(),
            successful: 0,
            failed: 0,
            active: 0,
            timed_out: false,
        };
        let detail = format!("{}tail", "x".repeat(512));
        let output = format_group_summary(&summary, false, &[detail]);
        assert!(output.contains("Rejected child requests: 1"));
        assert!(output.contains(&format!("- rejected request 1: {}…", "x".repeat(512))));
        assert!(!output.contains("tail"));
    }

    #[test]
    fn identical_spawn_payloads_are_distinguished_by_call_identity() {
        let first = delegation_key("call-a");
        let second = delegation_key("call-b");
        assert_ne!(first, second);
        assert_eq!(delegation_key("call-a"), delegation_key("call-a"));
        assert_eq!(
            spawn_request_fingerprint(
                "general",
                "same description",
                "same prompt",
                &["src".into()],
                None,
                Some(32),
                &[],
            ),
            spawn_request_fingerprint(
                "general",
                "same description",
                "same prompt",
                &["src".into()],
                None,
                Some(32),
                &[],
            )
        );
    }

    #[test]
    fn delegation_identity_excludes_request_body() {
        assert_eq!(
            delegation_key("accepted-call"),
            delegation_key("accepted-call")
        );
        assert_ne!(
            delegation_key("accepted-call"),
            delegation_key("other-call")
        );
        assert_ne!(
            spawn_request_fingerprint("general", "description-a", "prompt", &[], None, None, &[]),
            spawn_request_fingerprint("general", "description-b", "prompt", &[], None, None, &[])
        );
        assert_ne!(
            spawn_request_fingerprint("general", "description", "prompt", &[], None, Some(32), &[]),
            spawn_request_fingerprint("general", "description", "prompt", &[], None, Some(48), &[])
        );
        assert_ne!(
            spawn_request_fingerprint("general", "description", "prompt", &[], None, None, &[]),
            spawn_request_fingerprint(
                "general",
                "description",
                "prompt",
                &[],
                None,
                None,
                &[codegg_core::agent_run::SharedScratchGrant {
                    scratch_id: codegg_core::identity::ScratchId::new(),
                    alias: "notes".into()
                }]
            )
        );
    }
}
