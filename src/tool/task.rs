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
use crate::tool::Tool;

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
                                 result, denied_tools, time_created, time_updated)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
                       result, denied_tools, time_created, time_updated
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

                let status = match status_str.as_str() {
                    "running" => TaskStatus::Running,
                    "completed" => TaskStatus::Completed,
                    "failed" => TaskStatus::Failed,
                    _ => TaskStatus::Pending,
                };

                let denied_tools: Vec<String> = denied_tools_str
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
    ) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let task = SubAgentTask {
            id,
            description,
            prompt,
            agent,
            status: TaskStatus::Pending,
            result: None,
            parent_id,
            denied_tools,
        };
        self.tasks.lock().await.insert(id, task);
        id
    }

    pub async fn update_status(&self, id: u64, status: TaskStatus) {
        if let Some(task) = self.tasks.lock().await.get_mut(&id) {
            task.status = status;
        }
    }

    pub async fn set_result(&self, id: u64, result: String) {
        if let Some(task) = self.tasks.lock().await.get_mut(&id) {
            task.result = Some(result.clone());
            task.status = TaskStatus::Completed;
            let _ = self
                .update_status_in_db(id, &TaskStatus::Completed, Some(&result))
                .await;
        }
    }

    pub async fn set_failed(&self, id: u64, error: String) {
        if let Some(task) = self.tasks.lock().await.get_mut(&id) {
            task.result = Some(error.clone());
            task.status = TaskStatus::Failed;
            let _ = self
                .update_status_in_db(id, &TaskStatus::Failed, Some(&error))
                .await;
        }
    }

    /// Set the task as interrupted with a custom message.
    /// This preserves the Interrupted status (unlike set_failed which sets Failed).
    pub async fn set_interrupted(&self, id: u64, msg: String) {
        if let Some(task) = self.tasks.lock().await.get_mut(&id) {
            task.result = Some(msg.clone());
            task.status = TaskStatus::Interrupted;
            let _ = self
                .update_status_in_db(id, &TaskStatus::Interrupted, Some(&msg))
                .await;
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
        }
    }

    pub fn new_with_pool(
        pool: Arc<crate::agent::worker::SubAgentPool>,
        parent_session_id: Option<String>,
        denied_tools: Vec<String>,
    ) -> Self {
        Self {
            store: Arc::new(Mutex::new(TaskStore::new())),
            spawner: Some(pool.spawner()),
            parent_session_id,
            denied_tools,
        }
    }

    pub fn store(&self) -> Arc<Mutex<TaskStore>> {
        self.store.clone()
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
                "task_id": {
                    "type": "integer",
                    "description": "Task ID to retrieve (action=get)"
                }
            },
            "required": ["action"]
        })
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

            let task_id = self
                .store
                .lock()
                .await
                .create_task(
                    description.clone(),
                    prompt.clone(),
                    agent.clone(),
                    self.parent_session_id.clone(),
                    self.denied_tools.clone(),
                )
                .await;

            if let Some(ref spawner) = self.spawner {
                let req = SubAgentRequest {
                    task_id,
                    description: description.clone(),
                    prompt,
                    agent,
                    parent_id: self.parent_session_id.clone(),
                    denied_tools: self.denied_tools.clone(),
                    depth: 0,
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
