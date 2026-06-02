use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::bus::events::AppEvent;
use crate::bus::global::GlobalEventBus;
use crate::error::ToolError;
use crate::model_profile::types::{TaskStatePolicy, TodoMode};
use crate::task_state::{TodoItem, TodoPriority, TodoState, TodoStatus};
use crate::tool::Tool;

pub struct TodoWriteTool {
    state: Arc<Mutex<TodoState>>,
    policy: TaskStatePolicy,
    pool: Option<sqlx::SqlitePool>,
    session_id: Option<String>,
}

impl TodoWriteTool {
    pub fn new(state: Arc<Mutex<TodoState>>, policy: TaskStatePolicy) -> Self {
        Self {
            state,
            policy,
            pool: None,
            session_id: None,
        }
    }

    pub fn with_persistence(
        state: Arc<Mutex<TodoState>>,
        policy: TaskStatePolicy,
        pool: sqlx::SqlitePool,
        session_id: String,
    ) -> Self {
        Self {
            state,
            policy,
            pool: Some(pool),
            session_id: Some(session_id),
        }
    }
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "todowrite"
    }

    fn description(&self) -> &str {
        "Create, update, and manage todo items with persistent state"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "Optional unique identifier" },
                            "content": { "type": "string" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "blocked", "cancelled"] },
                            "priority": { "type": "string", "enum": ["low", "medium", "high"] },
                            "blocker": { "type": "string", "description": "Required when status is blocked" }
                        },
                        "required": ["content", "status"]
                    },
                    "description": "List of todos to set (replaces all existing todos)"
                }
            },
            "required": ["todos"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        if self.policy.mode == TodoMode::Disabled {
            return Err(ToolError::Execution(
                "Todo management is disabled for this model profile.".to_string(),
            ));
        }
        if self.policy.mode == TodoMode::GuidedCurrentTask {
            return Err(ToolError::Execution(
                "This model profile is configured for harness-guided todos. Use todoread and follow the active task.".to_string(),
            ));
        }
        if !self.policy.allow_model_todo_write {
            return Err(ToolError::Execution(
                "Model profile does not allow todo writes.".to_string(),
            ));
        }

        let todos = input["todos"]
            .as_array()
            .ok_or_else(|| ToolError::Execution("missing 'todos' parameter".to_string()))?;

        let mut items = Vec::new();
        for todo in todos {
            let id = todo["id"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

            let content = todo["content"]
                .as_str()
                .ok_or_else(|| ToolError::Execution("todo missing 'content'".to_string()))?
                .to_string();

            let status_str = todo["status"]
                .as_str()
                .ok_or_else(|| ToolError::Execution("todo missing 'status'".to_string()))?;

            let status = match status_str {
                "pending" => TodoStatus::Pending,
                "in_progress" => TodoStatus::InProgress,
                "completed" => TodoStatus::Completed,
                "blocked" => TodoStatus::Blocked,
                "cancelled" => TodoStatus::Cancelled,
                _ => {
                    return Err(ToolError::Execution(format!(
                        "invalid status: {}",
                        status_str
                    )))
                }
            };

            let priority = match todo["priority"].as_str().unwrap_or("medium") {
                "low" => TodoPriority::Low,
                "medium" => TodoPriority::Medium,
                "high" => TodoPriority::High,
                _ => TodoPriority::Medium,
            };

            let blocker = todo["blocker"].as_str().map(|s| s.to_string());

            items.push(TodoItem {
                id,
                content,
                status,
                priority,
                blocker,
            });
        }

        let mut state = self.state.lock().await;
        state
            .replace_from_model(items, &self.policy)
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        let projection = state.full_projection_for_user();
        let session_items: Option<Vec<crate::session::models::TodoItemInput>> =
            if self.pool.is_some() {
                Some(
                    state
                        .items
                        .iter()
                        .enumerate()
                        .map(|(i, item)| item.to_session_input(i as i64))
                        .collect(),
                )
            } else {
                None
            };
        drop(state);

        // Persist to session store if available
        if let (Some(pool), Some(session_id), Some(items)) =
            (&self.pool, &self.session_id, session_items)
        {
            let store = crate::session::store::TodoStore::new(pool.clone());
            if let Err(e) = store.set(session_id, items).await {
                tracing::warn!("Failed to persist todos: {}", e);
            }
        }

        if let Some(session_id) = &self.session_id {
            GlobalEventBus::publish(AppEvent::TodoUpdated {
                session_id: session_id.clone(),
            });
        }

        Ok(projection)
    }
}

pub struct TodoReadTool {
    state: Arc<Mutex<TodoState>>,
    policy: TaskStatePolicy,
}

impl TodoReadTool {
    pub fn new(state: Arc<Mutex<TodoState>>, policy: TaskStatePolicy) -> Self {
        Self { state, policy }
    }
}

#[async_trait]
impl Tool for TodoReadTool {
    fn name(&self) -> &str {
        "todoread"
    }

    fn description(&self) -> &str {
        "Read the current todo list and active task state"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        if !self.policy.allow_model_todo_read {
            return Err(ToolError::Execution(
                "Model profile does not allow todo reads.".to_string(),
            ));
        }

        let state = self.state.lock().await;
        match state.compact_projection(&self.policy) {
            Some(proj) => Ok(proj),
            None => Ok("No active todos.".to_string()),
        }
    }
}

/// Backward-compatible default: creates a TodoWriteTool with explicit_todo policy.
/// Used by ToolRegistry::with_defaults() for tests and non-session contexts.
pub struct TodoTool {
    state: Arc<Mutex<TodoState>>,
    policy: TaskStatePolicy,
}

impl TodoTool {
    pub fn new(state: Arc<Mutex<TodoState>>, policy: TaskStatePolicy) -> Self {
        Self { state, policy }
    }

    pub fn state(&self) -> Arc<Mutex<TodoState>> {
        self.state.clone()
    }

    pub fn policy(&self) -> &TaskStatePolicy {
        &self.policy
    }
}

impl Default for TodoTool {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(TodoState::new())),
            policy: TaskStatePolicy::explicit_todo(),
        }
    }
}

#[async_trait]
impl Tool for TodoTool {
    fn name(&self) -> &str {
        "todowrite"
    }

    fn description(&self) -> &str {
        "Create, update, and manage todo items with persistent state"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "Optional unique identifier" },
                            "content": { "type": "string" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "blocked", "cancelled"] },
                            "priority": { "type": "string", "enum": ["low", "medium", "high"] },
                            "blocker": { "type": "string", "description": "Required when status is blocked" }
                        },
                        "required": ["content", "status"]
                    },
                    "description": "List of todos to set (replaces all existing todos)"
                }
            },
            "required": ["todos"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let todos = input["todos"]
            .as_array()
            .ok_or_else(|| ToolError::Execution("missing 'todos' parameter".to_string()))?;

        let mut items = Vec::new();
        for todo in todos {
            let id = todo["id"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

            let content = todo["content"]
                .as_str()
                .ok_or_else(|| ToolError::Execution("todo missing 'content'".to_string()))?
                .to_string();

            let status_str = todo["status"]
                .as_str()
                .ok_or_else(|| ToolError::Execution("todo missing 'status'".to_string()))?;

            let status = match status_str {
                "pending" => TodoStatus::Pending,
                "in_progress" => TodoStatus::InProgress,
                "completed" => TodoStatus::Completed,
                "blocked" => TodoStatus::Blocked,
                "cancelled" => TodoStatus::Cancelled,
                _ => {
                    return Err(ToolError::Execution(format!(
                        "invalid status: {}",
                        status_str
                    )))
                }
            };

            let priority = match todo["priority"].as_str().unwrap_or("medium") {
                "low" => TodoPriority::Low,
                "medium" => TodoPriority::Medium,
                "high" => TodoPriority::High,
                _ => TodoPriority::Medium,
            };

            let blocker = todo["blocker"].as_str().map(|s| s.to_string());

            items.push(TodoItem {
                id,
                content,
                status,
                priority,
                blocker,
            });
        }

        let mut state = self.state.lock().await;
        state
            .replace_from_model(items, &self.policy)
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        Ok(state.full_projection_for_user())
    }
}
