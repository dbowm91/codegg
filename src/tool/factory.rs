use std::sync::Arc;

use sqlx::SqlitePool;

use crate::config::schema::Config;
use crate::model_profile::types::TaskStatePolicy;
use crate::tool::{ToolRegistry, ToolRegistryOptions};

/// Build a session-scoped [`ToolRegistry`] with default tools, goal tools,
/// and the optional task tool (when a task tool runtime is available).
///
/// This function consolidates tool construction that was previously inline
/// in `CoreDaemon`. It serves as a seam so that `core/daemon.rs` does not
/// need to import individual tool types directly.
pub fn build_session_tool_registry(
    config: &Config,
    pool: Option<SqlitePool>,
    session_id: &str,
    task_tool_runtime: Option<&crate::agent::task_tool_runtime::TaskToolRuntime>,
    task_state_policy: TaskStatePolicy,
) -> ToolRegistry {
    let todo_state = Arc::new(tokio::sync::Mutex::new(
        crate::task_state::TodoState::new(),
    ));
    let mut tool_registry = ToolRegistry::with_options(ToolRegistryOptions {
        todo_state: Some(todo_state),
        todo_policy: Some(task_state_policy),
        pool: pool.clone(),
        session_id: Some(session_id.to_string()),
        lsp_service: None,
        tool_backends: crate::tool::ToolBackendConfig::from_config(config),
    });

    // Register the task/subagent tool when a runtime is available.
    if let Some(runtime) = task_tool_runtime {
        let task_tool = crate::tool::task::TaskTool::new(
            runtime.store(),
            runtime.spawner(),
            Some(session_id.to_string()),
            Vec::new(),
        );
        tool_registry.register(task_tool);
    }

    // Register goal tools.
    if let Some(p) = pool {
        tool_registry.register(crate::tool::goal::GoalGetTool::new(
            p.clone(),
            session_id.to_string(),
        ));
        tool_registry.register(crate::tool::goal::GoalUpdateProgressTool::new(
            p.clone(),
            session_id.to_string(),
        ));
        tool_registry.register(crate::tool::goal::GoalRequestCompletionTool::new(
            p,
            session_id.to_string(),
        ));
    }

    tool_registry
}
