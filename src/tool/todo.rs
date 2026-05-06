use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::error::ToolError;
use crate::tool::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
    pub priority: TodoPriority,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoPriority {
    Low,
    Medium,
    High,
}

pub struct TodoStore {
    items: Vec<TodoItem>,
}

impl TodoStore {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn replace(&mut self, items: Vec<TodoItem>) {
        self.items = items;
    }

    pub fn items(&self) -> &[TodoItem] {
        &self.items
    }

    pub fn format(&self) -> String {
        if self.items.is_empty() {
            return "No todos.".to_string();
        }

        let mut result = String::from("Todos:\n");
        for (i, item) in self.items.iter().enumerate() {
            let status = match item.status {
                TodoStatus::Pending => "pending",
                TodoStatus::InProgress => "in_progress",
                TodoStatus::Completed => "completed",
            };
            let priority = match item.priority {
                TodoPriority::Low => "low",
                TodoPriority::Medium => "medium",
                TodoPriority::High => "high",
            };
            result.push_str(&format!(
                "{}. [{}] {} (priority: {})\n",
                i + 1,
                status,
                item.content,
                priority
            ));
        }
        result
    }
}

impl Default for TodoStore {
    fn default() -> Self {
        Self::new()
    }
}

pub struct TodoTool {
    store: Arc<Mutex<TodoStore>>,
}

impl TodoTool {
    pub fn new(store: Arc<Mutex<TodoStore>>) -> Self {
        Self { store }
    }

    pub fn store(&self) -> Arc<Mutex<TodoStore>> {
        self.store.clone()
    }
}

impl Default for TodoTool {
    fn default() -> Self {
        Self::new(Arc::new(Mutex::new(TodoStore::new())))
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
                            "content": { "type": "string" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "completed"] },
                            "priority": { "type": "string", "enum": ["low", "medium", "high"] }
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

            items.push(TodoItem {
                content,
                status,
                priority,
            });
        }

        let mut store = self.store.lock().await;
        store.replace(items);
        let output = store.format();

        Ok(output)
    }
}
