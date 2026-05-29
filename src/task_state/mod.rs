use crate::model_profile::types::{CompletedTodoExposure, TaskStatePolicy, TodoMode};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
    pub priority: TodoPriority,
    pub blocker: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoPriority {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TodoState {
    pub items: Vec<TodoItem>,
    pub revision: u64,
    pub reminder_pending: bool,
    pub tool_calls_since_injection: usize,
}

#[derive(Debug, Error)]
pub enum TodoStateError {
    #[error("Todo mode is disabled for this model profile")]
    ModeDisabled,
    #[error("Model profile does not allow todo writes")]
    WriteNotAllowed,
    #[error("Todo list exceeds maximum of {max} items (got {got})")]
    TooManyItems { max: usize, got: usize },
    #[error("Multiple items marked as in_progress (require_single_in_progress is enabled)")]
    MultipleInProgress,
    #[error("Blocked item missing blocker reason (require_blocker_reason is enabled)")]
    MissingBlockerReason,
    #[error("Invalid status: {0}")]
    InvalidStatus(String),
}

impl TodoItem {
    pub fn to_session_input(&self, _position: i64) -> crate::session::models::TodoItemInput {
        let status = match self.status {
            TodoStatus::Pending => "pending".to_string(),
            TodoStatus::InProgress => "in_progress".to_string(),
            TodoStatus::Completed => "completed".to_string(),
            TodoStatus::Blocked => "blocked".to_string(),
            TodoStatus::Cancelled => "cancelled".to_string(),
        };
        let priority = match self.priority {
            TodoPriority::Low => "low".to_string(),
            TodoPriority::Medium => "medium".to_string(),
            TodoPriority::High => "high".to_string(),
        };
        crate::session::models::TodoItemInput {
            content: self.content.clone(),
            status,
            priority,
        }
    }

    pub fn from_session_model(model: &crate::session::models::TodoItem) -> Self {
        let status = match model.status.as_str() {
            "pending" => TodoStatus::Pending,
            "in_progress" => TodoStatus::InProgress,
            "completed" => TodoStatus::Completed,
            "blocked" => TodoStatus::Blocked,
            "cancelled" => TodoStatus::Cancelled,
            _ => TodoStatus::Pending,
        };
        let priority = match model.priority.as_str() {
            "low" => TodoPriority::Low,
            "medium" => TodoPriority::Medium,
            "high" => TodoPriority::High,
            _ => TodoPriority::Medium,
        };
        let id = format!("pos-{}", model.position);
        TodoItem {
            id,
            content: model.content.clone(),
            status,
            priority,
            blocker: None,
        }
    }
}

impl TodoState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load_from_session(&mut self, session_items: Vec<crate::session::models::TodoItem>) {
        self.items = session_items
            .iter()
            .map(TodoItem::from_session_model)
            .collect();
        self.revision = 0;
        let all_done = self.is_all_done();
        self.reminder_pending = !all_done && !self.items.is_empty();
        self.tool_calls_since_injection = 0;
    }

    pub fn replace_from_model(
        &mut self,
        items: Vec<TodoItem>,
        policy: &TaskStatePolicy,
    ) -> Result<(), TodoStateError> {
        if policy.mode == TodoMode::Disabled {
            return Err(TodoStateError::ModeDisabled);
        }
        if !policy.allow_model_todo_write {
            return Err(TodoStateError::WriteNotAllowed);
        }
        if items.len() > policy.max_total_items {
            return Err(TodoStateError::TooManyItems {
                max: policy.max_total_items,
                got: items.len(),
            });
        }
        if policy.require_single_in_progress {
            let in_progress_count = items
                .iter()
                .filter(|i| i.status == TodoStatus::InProgress)
                .count();
            if in_progress_count > 1 {
                return Err(TodoStateError::MultipleInProgress);
            }
        }
        if policy.require_blocker_reason {
            for item in &items {
                if item.status == TodoStatus::Blocked
                    && item.blocker.as_ref().map_or(true, |b| b.trim().is_empty())
                {
                    return Err(TodoStateError::MissingBlockerReason);
                }
            }
        }
        self.items = items;
        self.revision += 1;
        let all_done = self.items.iter().all(|i| {
            matches!(
                i.status,
                TodoStatus::Completed | TodoStatus::Cancelled
            )
        });
        self.reminder_pending = !all_done;
        Ok(())
    }

    pub fn active_item(&self) -> Option<&TodoItem> {
        self.items
            .iter()
            .find(|i| i.status == TodoStatus::InProgress)
    }

    pub fn unfinished_items(&self) -> impl Iterator<Item = &TodoItem> {
        self.items
            .iter()
            .filter(|i| !matches!(i.status, TodoStatus::Completed | TodoStatus::Cancelled))
    }

    pub fn is_all_done(&self) -> bool {
        self.items.iter().all(|i| {
            matches!(
                i.status,
                TodoStatus::Completed | TodoStatus::Cancelled
            )
        })
    }

    pub fn compact_projection(&self, policy: &TaskStatePolicy) -> Option<String> {
        use crate::model_profile::types::TodoMode;

        if policy.mode == TodoMode::Disabled {
            return None;
        }
        if self.items.is_empty() {
            return None;
        }

        let unfinished: Vec<&TodoItem> = self.unfinished_items().collect();
        if unfinished.is_empty() {
            return None;
        }

        let expose_completed = match policy.expose_completed_items {
            CompletedTodoExposure::Full => true,
            CompletedTodoExposure::SummaryOnly => false,
            CompletedTodoExposure::NoneUnlessAsked => false,
        };

        let display_items: Vec<&TodoItem> = if expose_completed {
            self.items.iter().collect()
        } else {
            unfinished
        };

        let mut result = String::new();
        match policy.mode {
            TodoMode::SparsePlan => {
                let in_progress: Vec<&TodoItem> = display_items
                    .iter()
                    .filter(|i| i.status == TodoStatus::InProgress)
                    .copied()
                    .collect();
                let pending: Vec<&TodoItem> = display_items
                    .iter()
                    .filter(|i| i.status == TodoStatus::Pending)
                    .copied()
                    .collect();
                let blocked: Vec<&TodoItem> = display_items
                    .iter()
                    .filter(|i| i.status == TodoStatus::Blocked)
                    .copied()
                    .collect();

                result.push_str("Active task state: ");
                let mut parts = Vec::new();
                for item in &in_progress {
                    parts.push(format!("in_progress: {}", item.content));
                }
                for item in &pending {
                    parts.push(format!("pending: {}", item.content));
                }
                for item in &blocked {
                    parts.push(format!("blocked: {}", item.content));
                }
                result.push_str(&parts.join("; "));
                result.push_str(". Continue from the active item unless the user changes direction.");
            }
            TodoMode::ExplicitTodo => {
                result.push_str("Active todo state:\n");
                for item in &display_items {
                    let status = match item.status {
                        TodoStatus::Pending => "pending",
                        TodoStatus::InProgress => "in_progress",
                        TodoStatus::Completed => "completed",
                        TodoStatus::Blocked => "blocked",
                        TodoStatus::Cancelled => "cancelled",
                    };
                    result.push_str(&format!("- {}: {}\n", status, item.content));
                }
                result.push_str("Continue from the in-progress item unless the user changes direction.");
            }
            TodoMode::GuidedCurrentTask => {
                let unfinished_display: Vec<&TodoItem> = if expose_completed {
                    self.items.iter().collect()
                } else {
                    self.unfinished_items().collect()
                };
                if let Some(active) = self.active_item() {
                    result.push_str(&format!(
                        "Current task: {}. Do this task only. Report a blocker if unable to continue.",
                        active.content
                    ));
                    if let Some(next) = unfinished_display.iter().find(|i| i.status == TodoStatus::Pending) {
                        result.push_str(&format!("\nNext task: {}.", next.content));
                    }
                } else if let Some(first) = unfinished_display.first() {
                    result.push_str(&format!(
                        "Next task: {}. Do this task only. Report a blocker if unable to continue.",
                        first.content
                    ));
                }
            }
            TodoMode::Disabled => return None,
        }

        Some(result)
    }

    pub fn full_projection_for_user(&self) -> String {
        if self.items.is_empty() {
            return "No todos.".to_string();
        }
        let mut result = String::from("Todos:\n");
        for (i, item) in self.items.iter().enumerate() {
            let status = match item.status {
                TodoStatus::Pending => "pending",
                TodoStatus::InProgress => "in_progress",
                TodoStatus::Completed => "completed",
                TodoStatus::Blocked => "blocked",
                TodoStatus::Cancelled => "cancelled",
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

/// Build a compact reminder string for injection into the agent loop.
pub fn build_todo_reminder(todo: &TodoState, policy: &TaskStatePolicy) -> Option<String> {
    if policy.mode == TodoMode::Disabled {
        return None;
    }
    if todo.items.is_empty() {
        return None;
    }
    if !todo.reminder_pending && todo.tool_calls_since_injection < policy.inject_after_tool_calls.unwrap_or(usize::MAX) {
        return None;
    }
    todo.compact_projection(policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_policy() -> TaskStatePolicy {
        TaskStatePolicy::explicit_todo()
    }

    fn make_item(id: &str, content: &str, status: TodoStatus) -> TodoItem {
        TodoItem {
            id: id.to_string(),
            content: content.to_string(),
            status,
            priority: TodoPriority::Medium,
            blocker: None,
        }
    }

    #[test]
    fn test_replace_basic() {
        let mut state = TodoState::new();
        let policy = make_policy();
        let items = vec![
            make_item("1", "Task A", TodoStatus::InProgress),
            make_item("2", "Task B", TodoStatus::Pending),
        ];
        state.replace_from_model(items, &policy).unwrap();
        assert_eq!(state.revision, 1);
        assert!(state.reminder_pending);
        assert_eq!(state.items.len(), 2);
    }

    #[test]
    fn test_active_item() {
        let mut state = TodoState::new();
        let policy = make_policy();
        let items = vec![
            make_item("1", "Task A", TodoStatus::Pending),
            make_item("2", "Task B", TodoStatus::InProgress),
        ];
        state.replace_from_model(items, &policy).unwrap();
        let active = state.active_item().unwrap();
        assert_eq!(active.content, "Task B");
    }

    #[test]
    fn test_is_all_done() {
        let mut state = TodoState::new();
        let policy = make_policy();
        let items = vec![
            make_item("1", "Task A", TodoStatus::Completed),
            make_item("2", "Task B", TodoStatus::Cancelled),
        ];
        state.replace_from_model(items, &policy).unwrap();
        assert!(state.is_all_done());
        assert!(!state.reminder_pending);
    }

    #[test]
    fn test_disabled_rejects() {
        let mut state = TodoState::new();
        let policy = TaskStatePolicy::disabled();
        let items = vec![make_item("1", "Task A", TodoStatus::Pending)];
        let err = state.replace_from_model(items, &policy).unwrap_err();
        assert!(matches!(err, TodoStateError::ModeDisabled));
    }

    #[test]
    fn test_write_not_allowed() {
        let mut state = TodoState::new();
        let mut policy = make_policy();
        policy.allow_model_todo_write = false;
        let items = vec![make_item("1", "Task A", TodoStatus::Pending)];
        let err = state.replace_from_model(items, &policy).unwrap_err();
        assert!(matches!(err, TodoStateError::WriteNotAllowed));
    }

    #[test]
    fn test_too_many_items() {
        let mut state = TodoState::new();
        let mut policy = make_policy();
        policy.max_total_items = 2;
        let items = vec![
            make_item("1", "A", TodoStatus::Pending),
            make_item("2", "B", TodoStatus::Pending),
            make_item("3", "C", TodoStatus::Pending),
        ];
        let err = state.replace_from_model(items, &policy).unwrap_err();
        assert!(matches!(err, TodoStateError::TooManyItems { .. }));
    }

    #[test]
    fn test_multiple_in_progress_rejected() {
        let mut state = TodoState::new();
        let policy = make_policy();
        let items = vec![
            make_item("1", "A", TodoStatus::InProgress),
            make_item("2", "B", TodoStatus::InProgress),
        ];
        let err = state.replace_from_model(items, &policy).unwrap_err();
        assert!(matches!(err, TodoStateError::MultipleInProgress));
    }

    #[test]
    fn test_missing_blocker_reason() {
        let mut state = TodoState::new();
        let mut policy = make_policy();
        policy.require_blocker_reason = true;
        let items = vec![TodoItem {
            id: "1".to_string(),
            content: "A".to_string(),
            status: TodoStatus::Blocked,
            priority: TodoPriority::Medium,
            blocker: None,
        }];
        let err = state.replace_from_model(items, &policy).unwrap_err();
        assert!(matches!(err, TodoStateError::MissingBlockerReason));
    }

    #[test]
    fn test_compact_projection_explicit_todo() {
        let mut state = TodoState::new();
        let policy = make_policy();
        let items = vec![
            make_item("1", "Task A", TodoStatus::InProgress),
            make_item("2", "Task B", TodoStatus::Pending),
        ];
        state.replace_from_model(items, &policy).unwrap();
        let proj = state.compact_projection(&policy).unwrap();
        assert!(proj.contains("in_progress: Task A"));
        assert!(proj.contains("pending: Task B"));
    }

    #[test]
    fn test_compact_projection_sparse_plan() {
        let mut state = TodoState::new();
        let policy = TaskStatePolicy::sparse_plan();
        let items = vec![
            make_item("1", "Task A", TodoStatus::InProgress),
            make_item("2", "Task B", TodoStatus::Pending),
        ];
        state.replace_from_model(items, &policy).unwrap();
        let proj = state.compact_projection(&policy).unwrap();
        assert!(proj.contains("Active task state:"));
        assert!(proj.contains("in_progress: Task A"));
    }

    #[test]
    fn test_compact_projection_guided() {
        let mut state = TodoState::new();
        let mut policy = TaskStatePolicy::guided_current_task();
        // guided_current_task has allow_model_todo_write=false, so we bypass replace_from_model
        // and test compact_projection directly with manually set items.
        policy.allow_model_todo_write = true;
        let items = vec![
            make_item("1", "Task A", TodoStatus::InProgress),
            make_item("2", "Task B", TodoStatus::Pending),
        ];
        state.replace_from_model(items, &policy).unwrap();
        let proj = state.compact_projection(&policy).unwrap();
        assert!(proj.contains("Current task: Task A"));
        assert!(proj.contains("Next task: Task B"));
    }

    #[test]
    fn test_empty_list_clears_state() {
        let mut state = TodoState::new();
        let policy = make_policy();
        let items = vec![make_item("1", "Task A", TodoStatus::InProgress)];
        state.replace_from_model(items, &policy).unwrap();
        assert_eq!(state.items.len(), 1);
        
        state.replace_from_model(vec![], &policy).unwrap();
        assert!(state.items.is_empty());
        assert_eq!(state.revision, 2);
    }

    #[test]
    fn test_build_todo_reminder_disabled() {
        let state = TodoState::new();
        let policy = TaskStatePolicy::disabled();
        assert!(build_todo_reminder(&state, &policy).is_none());
    }

    #[test]
    fn test_build_todo_reminder_empty() {
        let state = TodoState::new();
        let policy = make_policy();
        assert!(build_todo_reminder(&state, &policy).is_none());
    }
}
