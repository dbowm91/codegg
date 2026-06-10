use std::sync::Arc;

use sqlx::SqlitePool;

use crate::config::schema::Config;
use crate::context::ContextArtifactStore;
use crate::model_profile::types::TaskStatePolicy;
use crate::tool::ToolRegistry;

/// Build a fully configured [`AgentLoop`] for a session turn.
///
/// This function consolidates agent construction that was previously inline
/// in `CoreDaemon`. It creates the permission checker, constructs the
/// `AgentLoop`, and applies session/subagent/task-state configuration.
///
/// System prompt assembly (memory context, goal context, plan_mode) stays
/// in the caller because it depends on daemon-local state.
#[allow(clippy::too_many_arguments)]
pub fn build_agent_loop(
    agents: Vec<crate::agent::Agent>,
    provider: Box<dyn crate::provider::Provider>,
    config: Config,
    tool_registry: ToolRegistry,
    pool: Option<SqlitePool>,
    session_id: &str,
    subagent_pool: Option<&Arc<crate::agent::worker::SubAgentPool>>,
    task_state_policy: TaskStatePolicy,
    mcp_service: Option<Arc<tokio::sync::RwLock<crate::mcp::McpService>>>,
    artifact_store: Arc<dyn ContextArtifactStore>,
) -> crate::agent::r#loop::AgentLoop {
    let permission_checker =
        crate::permission::PermissionChecker::new(Some(&config), None).with_active_mode(&config);

    let mut agent_loop = crate::agent::r#loop::AgentLoop::new(
        agents,
        provider,
        permission_checker,
        tool_registry,
        config,
        mcp_service,
        pool,
        artifact_store,
    );
    agent_loop.set_session_id(session_id);
    if let Some(spool) = subagent_pool {
        agent_loop.set_subagent_pool(Arc::clone(spool));
    }
    agent_loop.set_task_state_policy(task_state_policy);
    agent_loop
}
