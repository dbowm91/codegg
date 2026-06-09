use std::sync::Arc;

use crate::config::schema::Config;
use crate::model_profile::types::TaskStatePolicy;
use crate::tool::ToolRegistry;

/// Input for building an agent loop. Localizes all the concrete types
/// needed to construct an `AgentLoop` so callers don't need to know them.
pub struct AgentLoopBuildInput {
    pub agents: Vec<crate::agent::Agent>,
    pub provider: Box<dyn crate::provider::Provider>,
    pub config: Config,
    pub tool_registry: ToolRegistry,
    pub pool: Option<sqlx::SqlitePool>,
    pub session_id: String,
    pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
    pub task_state_policy: TaskStatePolicy,
    pub mcp_service: Option<Arc<tokio::sync::RwLock<crate::mcp::McpService>>>,
}

/// Trait for building agent loops, abstracting daemon→agent construction.
///
/// The default implementation delegates to `runtime_factory::build_agent_loop`.
pub trait AgentRuntimeProvider: Send + Sync {
    fn build_agent_loop(&self, input: AgentLoopBuildInput) -> crate::agent::r#loop::AgentLoop;
}

/// Default implementation that delegates to the existing factory function.
pub struct DefaultAgentRuntimeProvider;

impl AgentRuntimeProvider for DefaultAgentRuntimeProvider {
    fn build_agent_loop(&self, input: AgentLoopBuildInput) -> crate::agent::r#loop::AgentLoop {
        crate::agent::runtime_factory::build_agent_loop(
            input.agents,
            input.provider,
            input.config,
            input.tool_registry,
            input.pool,
            &input.session_id,
            input.subagent_pool.as_ref(),
            input.task_state_policy,
            input.mcp_service,
        )
    }
}
