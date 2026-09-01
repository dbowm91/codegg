//! Durable ownership records for delegated agent execution.
//!
//! `JobRecord`/`JobAttempt` remain the scheduler's queue authority.  This
//! module owns the durable identity and provenance of the delegated agent
//! itself.  The two records are deliberately related by typed job/attempt
//! references rather than by display IDs or hashes.

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::identity::{AgentRunId, AgentTaskId, NodeId, ProjectId, RepositoryId, WorktreeId};
use crate::jobs::{AttemptId, JobId};
use crate::workspace::WorkspaceId;

pub const MAX_RUN_RESULT_BYTES: usize = 16 * 1024;
pub const MAX_FAILURE_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskStatus {
    Created,
    Queued,
    Running,
    Completed,
    Failed,
    Interrupted,
    Cancelled,
}

impl AgentTaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Interrupted | Self::Cancelled
        )
    }

    fn from_str(value: &str) -> Self {
        match value {
            "created" => Self::Created,
            "queued" => Self::Queued,
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "interrupted" => Self::Interrupted,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Created,
    Queued,
    Preparing,
    Running,
    Waiting,
    Cancelling,
    Completed,
    Failed,
    Interrupted,
    Cancelled,
}

impl AgentRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Queued => "queued",
            Self::Preparing => "preparing",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Cancelling => "cancelling",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Interrupted | Self::Cancelled
        )
    }

    fn from_str(value: &str) -> Self {
        match value {
            "created" => Self::Created,
            "queued" => Self::Queued,
            "preparing" => Self::Preparing,
            "running" => Self::Running,
            "waiting" => Self::Waiting,
            "cancelling" => Self::Cancelling,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "interrupted" => Self::Interrupted,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunTerminal {
    Completed,
    Failed,
    Interrupted,
    Cancelled,
}

impl AgentRunTerminal {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunBudget {
    pub max_tool_calls: Option<u32>,
    pub max_wall_clock_secs: Option<u64>,
    pub max_depth: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTaskRecord {
    pub task_id: AgentTaskId,
    pub root_task_id: AgentTaskId,
    pub parent_task_id: Option<AgentTaskId>,
    pub originating_session_id: String,
    pub originating_turn_id: Option<String>,
    pub project_id: ProjectId,
    pub repository_id: Option<RepositoryId>,
    pub workspace_id: WorkspaceId,
    pub requested_agent: String,
    pub delegation_key: String,
    pub description: String,
    pub status: AgentTaskStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunRecord {
    pub run_id: AgentRunId,
    pub task_id: AgentTaskId,
    pub root_run_id: AgentRunId,
    pub parent_run_id: Option<AgentRunId>,
    pub workspace_id: WorkspaceId,
    pub worktree_id: Option<WorktreeId>,
    pub node_id: Option<NodeId>,
    pub job_id: Option<JobId>,
    pub attempt_id: Option<AttemptId>,
    pub agent_name: String,
    pub agent_digest: Option<String>,
    pub provider: String,
    pub model: String,
    pub authority_digest: String,
    pub budget: AgentRunBudget,
    pub status: AgentRunStatus,
    pub terminal: Option<AgentRunTerminal>,
    pub result_ref: Option<String>,
    pub failure_class: Option<String>,
    pub failure_message: Option<String>,
    pub cancellation_requested: bool,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewAgentTask {
    pub task_id: AgentTaskId,
    pub parent_task_id: Option<AgentTaskId>,
    pub originating_session_id: String,
    pub originating_turn_id: Option<String>,
    pub project_id: ProjectId,
    pub repository_id: Option<RepositoryId>,
    pub workspace_id: WorkspaceId,
    pub requested_agent: String,
    pub delegation_key: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct NewAgentRun {
    pub run_id: AgentRunId,
    pub parent_run_id: Option<AgentRunId>,
    pub workspace_id: WorkspaceId,
    pub agent_name: String,
    pub agent_digest: Option<String>,
    pub provider: String,
    pub model: String,
    pub authority_digest: String,
    pub budget: AgentRunBudget,
}

#[derive(Debug, Clone)]
pub struct AgentRunSubmission {
    pub task: AgentTaskRecord,
    pub run: AgentRunRecord,
    pub created: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRunTerminalOutcome {
    Completed,
    Failed,
    Interrupted,
    Cancelled,
}

impl AgentRunTerminalOutcome {
    fn status(self) -> AgentRunStatus {
        match self {
            Self::Completed => AgentRunStatus::Completed,
            Self::Failed => AgentRunStatus::Failed,
            Self::Interrupted => AgentRunStatus::Interrupted,
            Self::Cancelled => AgentRunStatus::Cancelled,
        }
    }

    fn task_status(self) -> AgentTaskStatus {
        match self {
            Self::Completed => AgentTaskStatus::Completed,
            Self::Failed => AgentTaskStatus::Failed,
            Self::Interrupted => AgentTaskStatus::Interrupted,
            Self::Cancelled => AgentTaskStatus::Cancelled,
        }
    }

    fn terminal(self) -> AgentRunTerminal {
        match self {
            Self::Completed => AgentRunTerminal::Completed,
            Self::Failed => AgentRunTerminal::Failed,
            Self::Interrupted => AgentRunTerminal::Interrupted,
            Self::Cancelled => AgentRunTerminal::Cancelled,
        }
    }
}

#[derive(Debug, Error)]
pub enum AgentRunStoreError {
    #[error("storage failure: {0}")]
    Storage(String),
    #[error("agent task '{0}' not found")]
    TaskNotFound(String),
    #[error("agent run '{0}' not found")]
    RunNotFound(String),
    #[error("delegation key already belongs to task '{0}'")]
    DelegationConflict(String),
    #[error("agent run relation is invalid: {0}")]
    InvalidRelation(String),
    #[error("invalid agent task transition: {from:?} -> {to:?}")]
    InvalidTaskTransition {
        from: AgentTaskStatus,
        to: AgentTaskStatus,
    },
    #[error("invalid agent run transition: {from:?} -> {to:?}")]
    InvalidRunTransition {
        from: AgentRunStatus,
        to: AgentRunStatus,
    },
    #[error("agent run is already terminal")]
    AlreadyTerminal,
    #[error("serialization failure: {0}")]
    Serialization(String),
}

fn bound(value: impl AsRef<str>, max: usize) -> String {
    let value = value.as_ref();
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = 0;
    for ch in value.chars() {
        if end + ch.len_utf8() > max {
            break;
        }
        end += ch.len_utf8();
    }
    value[..end].to_owned()
}

fn task_transition_allowed(from: AgentTaskStatus, to: AgentTaskStatus) -> bool {
    matches!(
        (from, to),
        (
            AgentTaskStatus::Created,
            AgentTaskStatus::Queued | AgentTaskStatus::Failed | AgentTaskStatus::Cancelled
        ) | (
            AgentTaskStatus::Queued,
            AgentTaskStatus::Running
                | AgentTaskStatus::Cancelled
                | AgentTaskStatus::Failed
                | AgentTaskStatus::Interrupted
        ) | (
            AgentTaskStatus::Running,
            AgentTaskStatus::Completed
                | AgentTaskStatus::Failed
                | AgentTaskStatus::Interrupted
                | AgentTaskStatus::Cancelled
        )
    )
}

fn run_transition_allowed(from: AgentRunStatus, to: AgentRunStatus) -> bool {
    matches!(
        (from, to),
        (
            AgentRunStatus::Created,
            AgentRunStatus::Queued
                | AgentRunStatus::Failed
                | AgentRunStatus::Cancelling
                | AgentRunStatus::Cancelled
        ) | (
            AgentRunStatus::Queued,
            AgentRunStatus::Preparing
                | AgentRunStatus::Cancelling
                | AgentRunStatus::Cancelled
                | AgentRunStatus::Failed
                | AgentRunStatus::Interrupted
        ) | (
            AgentRunStatus::Preparing,
            AgentRunStatus::Running
                | AgentRunStatus::Cancelling
                | AgentRunStatus::Failed
                | AgentRunStatus::Interrupted
                | AgentRunStatus::Cancelled
        ) | (
            AgentRunStatus::Running,
            AgentRunStatus::Waiting
                | AgentRunStatus::Cancelling
                | AgentRunStatus::Completed
                | AgentRunStatus::Failed
                | AgentRunStatus::Interrupted
                | AgentRunStatus::Cancelled
        ) | (
            AgentRunStatus::Waiting,
            AgentRunStatus::Running
                | AgentRunStatus::Cancelling
                | AgentRunStatus::Completed
                | AgentRunStatus::Failed
                | AgentRunStatus::Interrupted
                | AgentRunStatus::Cancelled
        ) | (
            AgentRunStatus::Cancelling,
            AgentRunStatus::Cancelled
                | AgentRunStatus::Failed
                | AgentRunStatus::Interrupted
                | AgentRunStatus::Completed
        )
    )
}

#[async_trait]
pub trait AgentRunStore: Send + Sync {
    async fn create_or_get(
        &self,
        task: NewAgentTask,
        run: NewAgentRun,
    ) -> Result<AgentRunSubmission, AgentRunStoreError>;
    async fn get_task(
        &self,
        id: &AgentTaskId,
    ) -> Result<Option<AgentTaskRecord>, AgentRunStoreError>;
    async fn get_run(&self, id: &AgentRunId) -> Result<Option<AgentRunRecord>, AgentRunStoreError>;
    async fn get_run_for_task(
        &self,
        id: &AgentTaskId,
    ) -> Result<Option<AgentRunRecord>, AgentRunStoreError>;
    async fn list_by_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<AgentRunRecord>, AgentRunStoreError>;
    async fn list_by_root(
        &self,
        root_id: &AgentRunId,
    ) -> Result<Vec<AgentRunRecord>, AgentRunStoreError>;
    async fn attach_job(
        &self,
        id: &AgentRunId,
        job_id: JobId,
    ) -> Result<AgentRunRecord, AgentRunStoreError>;
    async fn attach_attempt(
        &self,
        id: &AgentRunId,
        attempt_id: AttemptId,
    ) -> Result<AgentRunRecord, AgentRunStoreError>;
    async fn transition_task(
        &self,
        id: &AgentTaskId,
        status: AgentTaskStatus,
    ) -> Result<AgentTaskRecord, AgentRunStoreError>;
    async fn transition(
        &self,
        id: &AgentRunId,
        status: AgentRunStatus,
    ) -> Result<AgentRunRecord, AgentRunStoreError>;
    async fn finish(
        &self,
        id: &AgentRunId,
        outcome: AgentRunTerminalOutcome,
        result_ref: Option<String>,
        failure_class: Option<String>,
        failure_message: Option<String>,
    ) -> Result<AgentRunRecord, AgentRunStoreError>;
    async fn request_cancel(&self, id: &AgentRunId) -> Result<AgentRunRecord, AgentRunStoreError>;
}

#[derive(Default)]
struct MemoryState {
    tasks: HashMap<AgentTaskId, AgentTaskRecord>,
    runs: HashMap<AgentRunId, AgentRunRecord>,
    delegation: HashMap<String, AgentTaskId>,
}

#[derive(Default)]
pub struct InMemoryAgentRunStore {
    state: Mutex<MemoryState>,
}

impl InMemoryAgentRunStore {
    pub fn new() -> Self {
        Self::default()
    }
}

fn build_records(
    task: NewAgentTask,
    run: NewAgentRun,
    now: i64,
) -> (AgentTaskRecord, AgentRunRecord) {
    let root_task_id = task
        .parent_task_id
        .clone()
        .unwrap_or_else(|| task.task_id.clone());
    let root_run_id = run
        .parent_run_id
        .clone()
        .unwrap_or_else(|| run.run_id.clone());
    let task_record = AgentTaskRecord {
        task_id: task.task_id,
        root_task_id,
        parent_task_id: task.parent_task_id,
        originating_session_id: bound(task.originating_session_id, 256),
        originating_turn_id: task.originating_turn_id.map(|v| bound(v, 128)),
        project_id: task.project_id,
        repository_id: task.repository_id,
        workspace_id: task.workspace_id.clone(),
        requested_agent: bound(task.requested_agent, 128),
        delegation_key: bound(task.delegation_key, 512),
        description: bound(task.description, 1024),
        status: AgentTaskStatus::Created,
        created_at: now,
        updated_at: now,
    };
    let run_record = AgentRunRecord {
        run_id: run.run_id,
        task_id: task_record.task_id.clone(),
        root_run_id,
        parent_run_id: run.parent_run_id,
        workspace_id: run.workspace_id,
        worktree_id: None,
        node_id: None,
        job_id: None,
        attempt_id: None,
        agent_name: bound(run.agent_name, 128),
        agent_digest: run.agent_digest.map(|v| bound(v, 256)),
        provider: bound(run.provider, 128),
        model: bound(run.model, 256),
        authority_digest: bound(run.authority_digest, 256),
        budget: run.budget,
        status: AgentRunStatus::Created,
        terminal: None,
        result_ref: None,
        failure_class: None,
        failure_message: None,
        cancellation_requested: false,
        created_at: now,
        started_at: None,
        finished_at: None,
        updated_at: now,
    };
    (task_record, run_record)
}

fn transition_task(
    record: &mut AgentTaskRecord,
    to: AgentTaskStatus,
) -> Result<(), AgentRunStoreError> {
    if record.status == to {
        return Ok(());
    }
    if record.status.is_terminal() {
        return Err(AgentRunStoreError::AlreadyTerminal);
    }
    if !task_transition_allowed(record.status, to) {
        return Err(AgentRunStoreError::InvalidTaskTransition {
            from: record.status,
            to,
        });
    }
    record.status = to;
    record.updated_at = Utc::now().timestamp_millis();
    Ok(())
}

fn transition_run(
    record: &mut AgentRunRecord,
    to: AgentRunStatus,
) -> Result<(), AgentRunStoreError> {
    if record.status == to {
        return Ok(());
    }
    if record.status.is_terminal() {
        return Err(AgentRunStoreError::AlreadyTerminal);
    }
    if !run_transition_allowed(record.status, to) {
        return Err(AgentRunStoreError::InvalidRunTransition {
            from: record.status,
            to,
        });
    }
    record.status = to;
    let now = Utc::now().timestamp_millis();
    if to == AgentRunStatus::Running && record.started_at.is_none() {
        record.started_at = Some(now);
    }
    record.updated_at = now;
    Ok(())
}

#[async_trait]
impl AgentRunStore for InMemoryAgentRunStore {
    async fn create_or_get(
        &self,
        task: NewAgentTask,
        run: NewAgentRun,
    ) -> Result<AgentRunSubmission, AgentRunStoreError> {
        let mut state = self.state.lock().await;
        if let Some(id) = state.delegation.get(&task.delegation_key) {
            let task_record = state
                .tasks
                .get(id)
                .cloned()
                .ok_or_else(|| AgentRunStoreError::TaskNotFound(id.to_string()))?;
            let run_record = state
                .runs
                .values()
                .find(|r| r.task_id == *id)
                .cloned()
                .ok_or_else(|| AgentRunStoreError::RunNotFound(id.to_string()))?;
            if task_record.workspace_id != task.workspace_id
                || task_record.originating_session_id != task.originating_session_id
            {
                return Err(AgentRunStoreError::DelegationConflict(
                    task_record.task_id.to_string(),
                ));
            }
            return Ok(AgentRunSubmission {
                task: task_record,
                run: run_record,
                created: false,
            });
        }
        if let Some(parent_id) = task.parent_task_id.as_ref() {
            let parent = state.tasks.get(parent_id).ok_or_else(|| {
                AgentRunStoreError::InvalidRelation("parent task does not exist".into())
            })?;
            if parent.workspace_id != task.workspace_id
                || parent.originating_session_id != task.originating_session_id
            {
                return Err(AgentRunStoreError::InvalidRelation(
                    "parent task is outside the session/workspace scope".into(),
                ));
            }
        }
        if let Some(parent_run_id) = run.parent_run_id.as_ref() {
            let parent = state.runs.get(parent_run_id).ok_or_else(|| {
                AgentRunStoreError::InvalidRelation("parent run does not exist".into())
            })?;
            if parent.workspace_id != run.workspace_id
                || task.parent_task_id.as_ref() != Some(&parent.task_id)
            {
                return Err(AgentRunStoreError::InvalidRelation(
                    "parent run/task relation is outside the declared scope".into(),
                ));
            }
        }
        let now = Utc::now().timestamp_millis();
        let (mut task_record, mut run_record) = build_records(task, run, now);
        if let Some(parent_id) = task_record.parent_task_id.as_ref() {
            task_record.root_task_id = state
                .tasks
                .get(parent_id)
                .map(|parent| parent.root_task_id.clone())
                .ok_or_else(|| AgentRunStoreError::TaskNotFound(parent_id.to_string()))?;
        }
        if let Some(parent_id) = run_record.parent_run_id.as_ref() {
            run_record.root_run_id = state
                .runs
                .get(parent_id)
                .map(|parent| parent.root_run_id.clone())
                .ok_or_else(|| AgentRunStoreError::RunNotFound(parent_id.to_string()))?;
        }
        state.delegation.insert(
            task_record.delegation_key.clone(),
            task_record.task_id.clone(),
        );
        state
            .tasks
            .insert(task_record.task_id.clone(), task_record.clone());
        state
            .runs
            .insert(run_record.run_id.clone(), run_record.clone());
        Ok(AgentRunSubmission {
            task: task_record,
            run: run_record,
            created: true,
        })
    }

    async fn get_task(
        &self,
        id: &AgentTaskId,
    ) -> Result<Option<AgentTaskRecord>, AgentRunStoreError> {
        Ok(self.state.lock().await.tasks.get(id).cloned())
    }
    async fn get_run(&self, id: &AgentRunId) -> Result<Option<AgentRunRecord>, AgentRunStoreError> {
        Ok(self.state.lock().await.runs.get(id).cloned())
    }
    async fn get_run_for_task(
        &self,
        id: &AgentTaskId,
    ) -> Result<Option<AgentRunRecord>, AgentRunStoreError> {
        Ok(self
            .state
            .lock()
            .await
            .runs
            .values()
            .find(|run| &run.task_id == id)
            .cloned())
    }
    async fn list_by_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<AgentRunRecord>, AgentRunStoreError> {
        let state = self.state.lock().await;
        let task_ids: std::collections::HashSet<_> = state
            .tasks
            .values()
            .filter(|t| t.originating_session_id == session_id)
            .map(|t| t.task_id.clone())
            .collect();
        Ok(state
            .runs
            .values()
            .filter(|r| task_ids.contains(&r.task_id))
            .cloned()
            .collect())
    }
    async fn list_by_root(
        &self,
        root_id: &AgentRunId,
    ) -> Result<Vec<AgentRunRecord>, AgentRunStoreError> {
        Ok(self
            .state
            .lock()
            .await
            .runs
            .values()
            .filter(|r| &r.root_run_id == root_id)
            .cloned()
            .collect())
    }
    async fn attach_job(
        &self,
        id: &AgentRunId,
        job_id: JobId,
    ) -> Result<AgentRunRecord, AgentRunStoreError> {
        let mut state = self.state.lock().await;
        let run = state
            .runs
            .get_mut(id)
            .ok_or_else(|| AgentRunStoreError::RunNotFound(id.to_string()))?;
        if run
            .job_id
            .as_ref()
            .is_some_and(|current| current != &job_id)
        {
            return Err(AgentRunStoreError::InvalidRelation(
                "run is already attached to another job".into(),
            ));
        }
        run.job_id = Some(job_id);
        run.updated_at = Utc::now().timestamp_millis();
        Ok(run.clone())
    }
    async fn attach_attempt(
        &self,
        id: &AgentRunId,
        attempt_id: AttemptId,
    ) -> Result<AgentRunRecord, AgentRunStoreError> {
        let mut state = self.state.lock().await;
        let run = state
            .runs
            .get_mut(id)
            .ok_or_else(|| AgentRunStoreError::RunNotFound(id.to_string()))?;
        if run
            .attempt_id
            .as_ref()
            .is_some_and(|current| current != &attempt_id)
        {
            return Err(AgentRunStoreError::InvalidRelation(
                "run is already attached to another attempt".into(),
            ));
        }
        run.attempt_id = Some(attempt_id);
        run.updated_at = Utc::now().timestamp_millis();
        Ok(run.clone())
    }
    async fn transition_task(
        &self,
        id: &AgentTaskId,
        status: AgentTaskStatus,
    ) -> Result<AgentTaskRecord, AgentRunStoreError> {
        let mut state = self.state.lock().await;
        let task = state
            .tasks
            .get_mut(id)
            .ok_or_else(|| AgentRunStoreError::TaskNotFound(id.to_string()))?;
        transition_task(task, status)?;
        Ok(task.clone())
    }
    async fn transition(
        &self,
        id: &AgentRunId,
        status: AgentRunStatus,
    ) -> Result<AgentRunRecord, AgentRunStoreError> {
        let mut state = self.state.lock().await;
        let run = state
            .runs
            .get_mut(id)
            .ok_or_else(|| AgentRunStoreError::RunNotFound(id.to_string()))?;
        transition_run(run, status)?;
        let task_id = run.task_id.clone();
        let result = run.clone();
        let task_status = match status {
            AgentRunStatus::Queued => Some(AgentTaskStatus::Queued),
            AgentRunStatus::Running => Some(AgentTaskStatus::Running),
            _ => None,
        };
        if let (Some(task_status), Some(task)) = (task_status, state.tasks.get_mut(&task_id)) {
            if !task.status.is_terminal() {
                transition_task(task, task_status)?;
            }
        }
        Ok(result)
    }
    async fn finish(
        &self,
        id: &AgentRunId,
        outcome: AgentRunTerminalOutcome,
        result_ref: Option<String>,
        failure_class: Option<String>,
        failure_message: Option<String>,
    ) -> Result<AgentRunRecord, AgentRunStoreError> {
        let mut state = self.state.lock().await;
        let run = state
            .runs
            .get_mut(id)
            .ok_or_else(|| AgentRunStoreError::RunNotFound(id.to_string()))?;
        if run.status.is_terminal() {
            return Ok(run.clone());
        }
        transition_run(run, outcome.status())?;
        let now = Utc::now().timestamp_millis();
        run.terminal = Some(outcome.terminal());
        run.result_ref = result_ref.map(|v| bound(v, MAX_RUN_RESULT_BYTES));
        run.failure_class = failure_class.map(|v| bound(v, 128));
        run.failure_message = failure_message.map(|v| bound(v, MAX_FAILURE_BYTES));
        run.finished_at = Some(now);
        run.updated_at = now;
        let task_id = run.task_id.clone();
        let finished_run = run.clone();
        if let Some(task) = state.tasks.get_mut(&task_id) {
            if !task.status.is_terminal() {
                transition_task(task, outcome.task_status())?;
            }
        }
        Ok(finished_run)
    }
    async fn request_cancel(&self, id: &AgentRunId) -> Result<AgentRunRecord, AgentRunStoreError> {
        let mut state = self.state.lock().await;
        let run = state
            .runs
            .get_mut(id)
            .ok_or_else(|| AgentRunStoreError::RunNotFound(id.to_string()))?;
        if run.status.is_terminal() {
            return Ok(run.clone());
        }
        run.cancellation_requested = true;
        if matches!(
            run.status,
            AgentRunStatus::Created
                | AgentRunStatus::Queued
                | AgentRunStatus::Preparing
                | AgentRunStatus::Running
                | AgentRunStatus::Waiting
        ) {
            transition_run(run, AgentRunStatus::Cancelling)?;
        }
        Ok(run.clone())
    }
}

pub struct SqliteAgentRunStore {
    pool: SqlitePool,
}

impl SqliteAgentRunStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn storage_error(error: impl std::fmt::Display) -> AgentRunStoreError {
    AgentRunStoreError::Storage(error.to_string())
}
fn parse_task(value: String) -> Result<AgentTaskId, AgentRunStoreError> {
    AgentTaskId::parse(&value).map_err(storage_error)
}
fn parse_run(value: String) -> Result<AgentRunId, AgentRunStoreError> {
    AgentRunId::parse(&value).map_err(storage_error)
}
fn parse_job(value: Option<String>) -> Result<Option<JobId>, AgentRunStoreError> {
    Ok(value.map(JobId::new_unchecked))
}
fn parse_attempt(value: Option<String>) -> Result<Option<AttemptId>, AgentRunStoreError> {
    Ok(value.map(AttemptId::new_unchecked))
}

async fn load_task(
    pool: &SqlitePool,
    id: &AgentTaskId,
) -> Result<Option<AgentTaskRecord>, AgentRunStoreError> {
    let row = sqlx::query("SELECT task_id, root_task_id, parent_task_id, session_id, turn_id, project_id, repository_id, workspace_id, requested_agent, delegation_key, description, status, created_at, updated_at FROM agent_task WHERE task_id = ?").bind(id.as_str()).fetch_optional(pool).await.map_err(storage_error)?;
    row.map(|r| {
        Ok(AgentTaskRecord {
            task_id: parse_task(r.get("task_id"))?,
            root_task_id: parse_task(r.get("root_task_id"))?,
            parent_task_id: r
                .get::<Option<String>, _>("parent_task_id")
                .map(parse_task)
                .transpose()?,
            originating_session_id: r.get("session_id"),
            originating_turn_id: r.get("turn_id"),
            project_id: ProjectId::parse(r.get::<String, _>("project_id").as_str())
                .map_err(storage_error)?,
            repository_id: r
                .get::<Option<String>, _>("repository_id")
                .map(|v| RepositoryId::parse(&v))
                .transpose()
                .map_err(storage_error)?,
            workspace_id: WorkspaceId::new_unchecked(r.get::<String, _>("workspace_id")),
            requested_agent: r.get("requested_agent"),
            delegation_key: r.get("delegation_key"),
            description: r.get("description"),
            status: AgentTaskStatus::from_str(r.get::<String, _>("status").as_str()),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        })
    })
    .transpose()
}

async fn load_run(
    pool: &SqlitePool,
    id: &AgentRunId,
) -> Result<Option<AgentRunRecord>, AgentRunStoreError> {
    let row = sqlx::query("SELECT run_id, task_id, root_run_id, parent_run_id, workspace_id, worktree_id, node_id, job_id, attempt_id, agent_name, agent_digest, provider, model, authority_digest, budget_json, status, terminal, result_ref, failure_class, failure_message, cancellation_requested, created_at, started_at, finished_at, updated_at FROM agent_run WHERE run_id = ?").bind(id.as_str()).fetch_optional(pool).await.map_err(storage_error)?;
    row.map(|r| {
        Ok(AgentRunRecord {
            run_id: parse_run(r.get("run_id"))?,
            task_id: parse_task(r.get("task_id"))?,
            root_run_id: parse_run(r.get("root_run_id"))?,
            parent_run_id: r
                .get::<Option<String>, _>("parent_run_id")
                .map(parse_run)
                .transpose()?,
            workspace_id: WorkspaceId::new_unchecked(r.get::<String, _>("workspace_id")),
            worktree_id: r
                .get::<Option<String>, _>("worktree_id")
                .map(|v| WorktreeId::parse(&v))
                .transpose()
                .map_err(storage_error)?,
            node_id: r
                .get::<Option<String>, _>("node_id")
                .map(|v| NodeId::parse(&v))
                .transpose()
                .map_err(storage_error)?,
            job_id: parse_job(r.get("job_id"))?,
            attempt_id: parse_attempt(r.get("attempt_id"))?,
            agent_name: r.get("agent_name"),
            agent_digest: r.get("agent_digest"),
            provider: r.get("provider"),
            model: r.get("model"),
            authority_digest: r.get("authority_digest"),
            budget: serde_json::from_str(r.get::<String, _>("budget_json").as_str())
                .map_err(|e| AgentRunStoreError::Serialization(e.to_string()))?,
            status: AgentRunStatus::from_str(r.get::<String, _>("status").as_str()),
            terminal: r
                .get::<Option<String>, _>("terminal")
                .map(|v| match v.as_str() {
                    "completed" => AgentRunTerminal::Completed,
                    "failed" => AgentRunTerminal::Failed,
                    "interrupted" => AgentRunTerminal::Interrupted,
                    _ => AgentRunTerminal::Cancelled,
                }),
            result_ref: r.get("result_ref"),
            failure_class: r.get("failure_class"),
            failure_message: r.get("failure_message"),
            cancellation_requested: r.get::<i64, _>("cancellation_requested") != 0,
            created_at: r.get("created_at"),
            started_at: r.get("started_at"),
            finished_at: r.get("finished_at"),
            updated_at: r.get("updated_at"),
        })
    })
    .transpose()
}

async fn update_run(pool: &SqlitePool, run: &AgentRunRecord) -> Result<(), AgentRunStoreError> {
    sqlx::query("UPDATE agent_run SET job_id = ?, attempt_id = ?, status = ?, terminal = ?, result_ref = ?, failure_class = ?, failure_message = ?, cancellation_requested = ?, started_at = ?, finished_at = ?, updated_at = ? WHERE run_id = ?")
        .bind(run.job_id.as_ref().map(JobId::as_str)).bind(run.attempt_id.as_ref().map(AttemptId::as_str)).bind(run.status.as_str()).bind(run.terminal.as_ref().map(AgentRunTerminal::as_str)).bind(run.result_ref.as_deref()).bind(run.failure_class.as_deref()).bind(run.failure_message.as_deref()).bind(i64::from(run.cancellation_requested)).bind(run.started_at).bind(run.finished_at).bind(run.updated_at).bind(run.run_id.as_str()).execute(pool).await.map_err(storage_error)?;
    Ok(())
}

#[async_trait]
impl AgentRunStore for SqliteAgentRunStore {
    async fn create_or_get(
        &self,
        task: NewAgentTask,
        run: NewAgentRun,
    ) -> Result<AgentRunSubmission, AgentRunStoreError> {
        let mut tx = self.pool.begin().await.map_err(storage_error)?;
        if let Some(row) = sqlx::query("SELECT task_id FROM agent_task WHERE delegation_key = ?")
            .bind(&task.delegation_key)
            .fetch_optional(&mut *tx)
            .await
            .map_err(storage_error)?
        {
            let existing = parse_task(row.get("task_id"))?;
            let run_record = sqlx::query("SELECT run_id FROM agent_run WHERE task_id = ?")
                .bind(existing.as_str())
                .fetch_optional(&mut *tx)
                .await
                .map_err(storage_error)?
                .ok_or_else(|| AgentRunStoreError::RunNotFound(existing.to_string()))?;
            let run_id = parse_run(run_record.get("run_id"))?;
            tx.rollback().await.map_err(storage_error)?;
            let task_record = load_task(&self.pool, &existing)
                .await?
                .ok_or_else(|| AgentRunStoreError::TaskNotFound(existing.to_string()))?;
            let existing_run = load_run(&self.pool, &run_id)
                .await?
                .ok_or_else(|| AgentRunStoreError::RunNotFound(run_id.to_string()))?;
            if task_record.workspace_id != task.workspace_id
                || task_record.originating_session_id != task.originating_session_id
            {
                return Err(AgentRunStoreError::DelegationConflict(existing.to_string()));
            }
            return Ok(AgentRunSubmission {
                task: task_record,
                run: existing_run,
                created: false,
            });
        }
        if let Some(parent) = task.parent_task_id.as_ref() {
            let exists = sqlx::query("SELECT 1 FROM agent_task WHERE task_id = ?")
                .bind(parent.as_str())
                .fetch_optional(&mut *tx)
                .await
                .map_err(storage_error)?
                .is_some();
            if !exists {
                return Err(AgentRunStoreError::InvalidRelation(
                    "parent task does not exist".into(),
                ));
            }
        }
        let task_input = task.clone();
        let run_input = run.clone();
        let now = Utc::now().timestamp_millis();
        let (task_record, run_record) = build_records(task, run, now);
        let budget = serde_json::to_string(&run_record.budget)
            .map_err(|e| AgentRunStoreError::Serialization(e.to_string()))?;
        let task_insert = sqlx::query("INSERT INTO agent_task (task_id, root_task_id, parent_task_id, session_id, turn_id, project_id, repository_id, workspace_id, requested_agent, delegation_key, description, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(task_record.task_id.as_str()).bind(task_record.root_task_id.as_str()).bind(task_record.parent_task_id.as_ref().map(AgentTaskId::as_str)).bind(&task_record.originating_session_id).bind(task_record.originating_turn_id.as_deref()).bind(task_record.project_id.as_str()).bind(task_record.repository_id.as_ref().map(RepositoryId::as_str)).bind(task_record.workspace_id.as_str()).bind(&task_record.requested_agent).bind(&task_record.delegation_key).bind(&task_record.description).bind(task_record.status.as_str()).bind(now).bind(now).execute(&mut *tx).await;
        if let Err(error) = task_insert {
            tx.rollback().await.map_err(storage_error)?;
            if error.to_string().contains("agent_task.delegation_key") {
                return self.create_or_get(task_input, run_input).await;
            }
            return Err(storage_error(error));
        }
        sqlx::query("INSERT INTO agent_run (run_id, task_id, root_run_id, parent_run_id, workspace_id, agent_name, agent_digest, provider, model, authority_digest, budget_json, status, cancellation_requested, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)")
            .bind(run_record.run_id.as_str()).bind(run_record.task_id.as_str()).bind(run_record.root_run_id.as_str()).bind(run_record.parent_run_id.as_ref().map(AgentRunId::as_str)).bind(run_record.workspace_id.as_str()).bind(&run_record.agent_name).bind(run_record.agent_digest.as_deref()).bind(&run_record.provider).bind(&run_record.model).bind(&run_record.authority_digest).bind(budget).bind(run_record.status.as_str()).bind(now).bind(now).execute(&mut *tx).await.map_err(storage_error)?;
        tx.commit().await.map_err(storage_error)?;
        Ok(AgentRunSubmission {
            task: task_record,
            run: run_record,
            created: true,
        })
    }
    async fn get_task(
        &self,
        id: &AgentTaskId,
    ) -> Result<Option<AgentTaskRecord>, AgentRunStoreError> {
        load_task(&self.pool, id).await
    }
    async fn get_run(&self, id: &AgentRunId) -> Result<Option<AgentRunRecord>, AgentRunStoreError> {
        load_run(&self.pool, id).await
    }
    async fn get_run_for_task(
        &self,
        id: &AgentTaskId,
    ) -> Result<Option<AgentRunRecord>, AgentRunStoreError> {
        let row = sqlx::query("SELECT run_id FROM agent_run WHERE task_id = ?")
            .bind(id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(storage_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let run_id = parse_run(row.get("run_id"))?;
        load_run(&self.pool, &run_id).await
    }
    async fn list_by_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<AgentRunRecord>, AgentRunStoreError> {
        let ids = sqlx::query("SELECT r.run_id FROM agent_run r JOIN agent_task t ON t.task_id = r.task_id WHERE t.session_id = ? ORDER BY r.created_at").bind(session_id).fetch_all(&self.pool).await.map_err(storage_error)?;
        let mut out = Vec::with_capacity(ids.len());
        for row in ids {
            if let Some(run) = self.get_run(&parse_run(row.get("run_id"))?).await? {
                out.push(run);
            }
        }
        Ok(out)
    }
    async fn list_by_root(
        &self,
        root_id: &AgentRunId,
    ) -> Result<Vec<AgentRunRecord>, AgentRunStoreError> {
        let ids =
            sqlx::query("SELECT run_id FROM agent_run WHERE root_run_id = ? ORDER BY created_at")
                .bind(root_id.as_str())
                .fetch_all(&self.pool)
                .await
                .map_err(storage_error)?;
        let mut out = Vec::with_capacity(ids.len());
        for row in ids {
            if let Some(run) = self.get_run(&parse_run(row.get("run_id"))?).await? {
                out.push(run);
            }
        }
        Ok(out)
    }
    async fn attach_job(
        &self,
        id: &AgentRunId,
        job_id: JobId,
    ) -> Result<AgentRunRecord, AgentRunStoreError> {
        let mut run = self
            .get_run(id)
            .await?
            .ok_or_else(|| AgentRunStoreError::RunNotFound(id.to_string()))?;
        if run.job_id.as_ref().is_some_and(|v| v != &job_id) {
            return Err(AgentRunStoreError::InvalidRelation(
                "run is already attached to another job".into(),
            ));
        }
        run.job_id = Some(job_id);
        run.updated_at = Utc::now().timestamp_millis();
        update_run(&self.pool, &run).await?;
        Ok(run)
    }
    async fn attach_attempt(
        &self,
        id: &AgentRunId,
        attempt_id: AttemptId,
    ) -> Result<AgentRunRecord, AgentRunStoreError> {
        let mut run = self
            .get_run(id)
            .await?
            .ok_or_else(|| AgentRunStoreError::RunNotFound(id.to_string()))?;
        if run.attempt_id.as_ref().is_some_and(|v| v != &attempt_id) {
            return Err(AgentRunStoreError::InvalidRelation(
                "run is already attached to another attempt".into(),
            ));
        }
        run.attempt_id = Some(attempt_id);
        run.updated_at = Utc::now().timestamp_millis();
        update_run(&self.pool, &run).await?;
        Ok(run)
    }
    async fn transition_task(
        &self,
        id: &AgentTaskId,
        status: AgentTaskStatus,
    ) -> Result<AgentTaskRecord, AgentRunStoreError> {
        let mut task = self
            .get_task(id)
            .await?
            .ok_or_else(|| AgentRunStoreError::TaskNotFound(id.to_string()))?;
        transition_task(&mut task, status)?;
        sqlx::query("UPDATE agent_task SET status = ?, updated_at = ? WHERE task_id = ?")
            .bind(task.status.as_str())
            .bind(task.updated_at)
            .bind(id.as_str())
            .execute(&self.pool)
            .await
            .map_err(storage_error)?;
        Ok(task)
    }
    async fn transition(
        &self,
        id: &AgentRunId,
        status: AgentRunStatus,
    ) -> Result<AgentRunRecord, AgentRunStoreError> {
        let mut run = self
            .get_run(id)
            .await?
            .ok_or_else(|| AgentRunStoreError::RunNotFound(id.to_string()))?;
        transition_run(&mut run, status)?;
        update_run(&self.pool, &run).await?;
        if matches!(status, AgentRunStatus::Queued | AgentRunStatus::Running) {
            let task_status = if status == AgentRunStatus::Queued {
                "queued"
            } else {
                "running"
            };
            sqlx::query("UPDATE agent_task SET status = ?, updated_at = ? WHERE task_id = ? AND status IN ('created', 'queued')")
                .bind(task_status)
                .bind(run.updated_at)
                .bind(run.task_id.as_str())
                .execute(&self.pool)
                .await
                .map_err(storage_error)?;
        }
        Ok(run)
    }
    async fn finish(
        &self,
        id: &AgentRunId,
        outcome: AgentRunTerminalOutcome,
        result_ref: Option<String>,
        failure_class: Option<String>,
        failure_message: Option<String>,
    ) -> Result<AgentRunRecord, AgentRunStoreError> {
        let mut run = self
            .get_run(id)
            .await?
            .ok_or_else(|| AgentRunStoreError::RunNotFound(id.to_string()))?;
        if run.status.is_terminal() {
            return Ok(run);
        }
        transition_run(&mut run, outcome.status())?;
        let now = Utc::now().timestamp_millis();
        run.terminal = Some(outcome.terminal());
        run.result_ref = result_ref.map(|v| bound(v, MAX_RUN_RESULT_BYTES));
        run.failure_class = failure_class.map(|v| bound(v, 128));
        run.failure_message = failure_message.map(|v| bound(v, MAX_FAILURE_BYTES));
        run.finished_at = Some(now);
        run.updated_at = now;
        let mut tx = self.pool.begin().await.map_err(storage_error)?;
        sqlx::query("UPDATE agent_run SET status = ?, terminal = ?, result_ref = ?, failure_class = ?, failure_message = ?, finished_at = ?, updated_at = ? WHERE run_id = ? AND status NOT IN ('completed','failed','interrupted','cancelled')").bind(run.status.as_str()).bind(run.terminal.as_ref().map(AgentRunTerminal::as_str)).bind(run.result_ref.as_deref()).bind(run.failure_class.as_deref()).bind(run.failure_message.as_deref()).bind(now).bind(now).bind(id.as_str()).execute(&mut *tx).await.map_err(storage_error)?;
        sqlx::query("UPDATE agent_task SET status = ?, updated_at = ? WHERE task_id = ? AND status NOT IN ('completed','failed','interrupted','cancelled')").bind(outcome.task_status().as_str()).bind(now).bind(run.task_id.as_str()).execute(&mut *tx).await.map_err(storage_error)?;
        tx.commit().await.map_err(storage_error)?;
        Ok(self.get_run(id).await?.unwrap_or(run))
    }
    async fn request_cancel(&self, id: &AgentRunId) -> Result<AgentRunRecord, AgentRunStoreError> {
        let mut run = self
            .get_run(id)
            .await?
            .ok_or_else(|| AgentRunStoreError::RunNotFound(id.to_string()))?;
        if run.status.is_terminal() {
            return Ok(run);
        }
        run.cancellation_requested = true;
        if !matches!(run.status, AgentRunStatus::Cancelling) {
            transition_run(&mut run, AgentRunStatus::Cancelling)?;
        }
        update_run(&self.pool, &run).await?;
        Ok(run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(key: &str, parent: Option<AgentTaskId>) -> NewAgentTask {
        NewAgentTask {
            task_id: AgentTaskId::new(),
            parent_task_id: parent,
            originating_session_id: "session".into(),
            originating_turn_id: Some("turn".into()),
            project_id: ProjectId::new(),
            repository_id: None,
            workspace_id: WorkspaceId::new_unchecked("workspace"),
            requested_agent: "general".into(),
            delegation_key: key.into(),
            description: "bounded task".into(),
        }
    }
    fn run(parent: Option<AgentRunId>) -> NewAgentRun {
        NewAgentRun {
            run_id: AgentRunId::new(),
            parent_run_id: parent,
            workspace_id: WorkspaceId::new_unchecked("workspace"),
            agent_name: "general".into(),
            agent_digest: Some("digest".into()),
            provider: "openai".into(),
            model: "model".into(),
            authority_digest: "authority".into(),
            budget: AgentRunBudget {
                max_tool_calls: Some(10),
                ..Default::default()
            },
        }
    }

    #[tokio::test]
    async fn duplicate_key_returns_one_identity_and_terminal_is_first_wins() {
        let store = InMemoryAgentRunStore::new();
        let first = store
            .create_or_get(task("same", None), run(None))
            .await
            .unwrap();
        let second = store
            .create_or_get(task("same", None), run(None))
            .await
            .unwrap();
        assert!(first.created);
        assert!(!second.created);
        assert_eq!(first.task.task_id, second.task.task_id);
        store
            .transition(&first.run.run_id, AgentRunStatus::Queued)
            .await
            .unwrap();
        store
            .transition(&first.run.run_id, AgentRunStatus::Preparing)
            .await
            .unwrap();
        store
            .transition(&first.run.run_id, AgentRunStatus::Running)
            .await
            .unwrap();
        store
            .finish(
                &first.run.run_id,
                AgentRunTerminalOutcome::Completed,
                Some("ok".into()),
                None,
                None,
            )
            .await
            .unwrap();
        let late = store
            .finish(
                &first.run.run_id,
                AgentRunTerminalOutcome::Failed,
                None,
                Some("late".into()),
                Some("late".into()),
            )
            .await
            .unwrap();
        assert_eq!(late.status, AgentRunStatus::Completed);
    }

    #[tokio::test]
    async fn forged_parent_is_rejected() {
        let store = InMemoryAgentRunStore::new();
        let err = store
            .create_or_get(task("child", Some(AgentTaskId::new())), run(None))
            .await
            .unwrap_err();
        assert!(matches!(err, AgentRunStoreError::InvalidRelation(_)));
    }

    #[tokio::test]
    async fn cancellation_before_admission_is_terminal_and_idempotent() {
        let store = InMemoryAgentRunStore::new();
        let submission = store
            .create_or_get(task("cancel-before-admission", None), run(None))
            .await
            .unwrap();
        let requested = store.request_cancel(&submission.run.run_id).await.unwrap();
        assert!(requested.cancellation_requested);
        assert_eq!(requested.status, AgentRunStatus::Cancelling);
        let cancelled = store
            .finish(
                &submission.run.run_id,
                AgentRunTerminalOutcome::Cancelled,
                None,
                Some("cancelled_before_admission".into()),
                None,
            )
            .await
            .unwrap();
        assert_eq!(cancelled.status, AgentRunStatus::Cancelled);
        assert_eq!(cancelled.terminal, Some(AgentRunTerminal::Cancelled));
        let late = store
            .finish(
                &submission.run.run_id,
                AgentRunTerminalOutcome::Completed,
                Some("late".into()),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(late.status, AgentRunStatus::Cancelled);
    }

    #[tokio::test]
    async fn sqlite_records_round_trip_and_duplicate_key_is_stable() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::session::schema::migrate(&pool).await.unwrap();
        let store = SqliteAgentRunStore::new(pool);
        let first = store
            .create_or_get(task("sqlite-key", None), run(None))
            .await
            .unwrap();
        let second = store
            .create_or_get(task("sqlite-key", None), run(None))
            .await
            .unwrap();
        assert!(first.created);
        assert!(!second.created);
        assert_eq!(first.run.run_id, second.run.run_id);
        store
            .transition(&first.run.run_id, AgentRunStatus::Queued)
            .await
            .unwrap();
        store
            .transition(&first.run.run_id, AgentRunStatus::Preparing)
            .await
            .unwrap();
        store
            .transition(&first.run.run_id, AgentRunStatus::Running)
            .await
            .unwrap();
        store
            .attach_job(&first.run.run_id, JobId::new_unchecked("job-1"))
            .await
            .unwrap();
        store
            .attach_attempt(&first.run.run_id, AttemptId::new_unchecked("attempt-1"))
            .await
            .unwrap();
        let loaded = store.get_run(&first.run.run_id).await.unwrap().unwrap();
        assert_eq!(loaded.job_id.unwrap().as_str(), "job-1");
        assert_eq!(loaded.attempt_id.unwrap().as_str(), "attempt-1");
    }
}
