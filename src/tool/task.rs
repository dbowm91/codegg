use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

use crate::agent::worker::{SubAgentRequest, SubAgentSpawner};
use crate::error::ToolError;
use crate::tool::{Tool, ToolCategory};

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
            let _ = self.update_status_in_db(id, &status, Some(&result)).await;
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
            let _ = self.update_status_in_db(id, &status, Some(&error)).await;
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
            let _ = self.update_status_in_db(id, &status, Some(&msg)).await;
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
            let _ = self.update_status_in_db(id, &status, Some(&error)).await;
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
}

impl TaskTool {
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
        "Spawn a subagent to handle a task independently"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action to perform: 'spawn' (default) to spawn a subagent, 'get' to retrieve task results",
                    "enum": ["spawn", "get"]
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
                    "description": "List of directories the subagent is allowed to access (action=spawn)"
                },
                "task_id": {
                    "type": "integer",
                    "description": "Task ID to retrieve (action=get)"
                }
            },
            "required": ["action"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let action = input["action"].as_str().unwrap_or("spawn");

        if action == "get" {
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

            let mut denied_tools = self.denied_tools.clone();
            if !denied_tools.contains(&"task".to_string()) {
                denied_tools.push("task".to_string());
            }

            let allowed_paths: Vec<String> = input["allowed_paths"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            if let (Some(submission), Some(workspace_root)) =
                (self.submission.clone(), self.workspace_root.clone())
            {
                let workspace_id = submission
                    .workspace_id_for_root(&workspace_root)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                let submitted = submission
                    .submit(
                        None,
                        codegg_core::jobs::NewJob {
                            workspace_id,
                            session_id: self.parent_session_id.clone(),
                            turn_id: None,
                            kind: codegg_core::jobs::JobKind::Subagent,
                            source: codegg_core::jobs::JobSource::AgentDelegated,
                            priority: codegg_core::jobs::JobPriority::Interactive,
                            payload: codegg_core::jobs::JobPayload::Subagent {
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
                        },
                    )
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
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
                    description: description.clone(),
                    prompt,
                    agent,
                    parent_id: self.parent_session_id.clone(),
                    denied_tools,
                    allowed_paths,
                    depth: self.depth + 1,
                    max_tool_calls: None,
                    parent_model: self.parent_model.clone(),
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
