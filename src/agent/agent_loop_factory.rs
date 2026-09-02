use std::sync::Arc;

use crate::config::schema::Config;
use crate::context::ContextArtifactStore;
use crate::model_profile::types::TaskStatePolicy;
use crate::tool::ToolRegistry;
use codegg_core::workspace::ExecutionContext;

/// Input for building an agent loop. Localizes all the concrete types
/// needed to construct an `AgentLoop` so callers don't need to know them.
pub struct AgentLoopBuildInput {
    pub agents: Vec<crate::agent::Agent>,
    pub provider: Box<dyn crate::provider::Provider>,
    pub config: Config,
    pub tool_registry: ToolRegistry,
    pub pool: Option<sqlx::SqlitePool>,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
    pub task_state_policy: TaskStatePolicy,
    pub mcp_service: Option<Arc<tokio::sync::RwLock<crate::mcp::McpService>>>,
    /// Shared artifact store for context projection. The agent loop
    /// stores tool output artifacts here and `context_read` expands them.
    pub artifact_store: Arc<dyn ContextArtifactStore>,
    pub submission: Option<Arc<crate::scheduler::JobSubmissionService>>,
    pub execution: Arc<ExecutionContext>,
    pub notification_service:
        Option<Arc<crate::scheduler::tool_program_notifications::ToolProgramNotificationService>>,
}

/// Build a fully initialized loop from the daemon-resolved turn identity.
pub fn build_agent_loop(input: AgentLoopBuildInput) -> crate::agent::r#loop::AgentLoop {
    let permission_checker = crate::permission::PermissionChecker::new(Some(&input.config), None)
        .with_active_mode(&input.config);
    let mut agent_loop = crate::agent::r#loop::AgentLoop::new(
        input.agents,
        input.provider,
        permission_checker,
        input.tool_registry,
        input.config,
        input.mcp_service,
        input.pool,
        input.artifact_store,
        input.execution.workspace_root.clone(),
        input.session_id,
    );
    agent_loop.set_turn_id(input.turn_id);
    if let Some(spool) = input.subagent_pool {
        agent_loop.set_subagent_pool(spool);
    }
    if let Some(submission) = input.submission {
        agent_loop.set_submission(submission);
    }
    agent_loop.set_task_state_policy(input.task_state_policy);
    if let Some(svc) = input.notification_service {
        agent_loop.set_notification_service(svc);
    }
    agent_loop
}
