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
    pub provider_registry: Arc<crate::provider::ProviderRegistry>,
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
    /// Lease retaining the daemon-owned workspace service (and therefore its
    /// canonical lock table) for the lifetime of the spawned turn.
    pub workspace_service_lease: Option<codegg_core::workspace_services::WorkspaceServicesLease>,
    pub notification_service:
        Option<Arc<crate::scheduler::tool_program_notifications::ToolProgramNotificationService>>,
    pub job_notification_service:
        Option<Arc<crate::scheduler::job_notifications::JobNotificationService>>,
    pub ci_watch_service: Option<Arc<crate::scheduler::ci_watch::CiWatchService>>,
    /// M003: resolved effective approval mode for this turn. `None` means
    /// the `Interactive` default; daemon turns resolve the persisted
    /// principal preference before construction.
    pub approval_mode: Option<codegg_core::approval::ApprovalMode>,
    /// M003: sandbox profile preference, separate from approval mode.
    pub sandbox_profile: Option<codegg_core::approval::SandboxProfile>,
    /// M010: optional inference feedback bound to the session's selected
    /// provider connection revision.
    pub credential_observer:
        Option<crate::agent::provider_qualification::SharedProviderCredentialObserver>,
}

/// Permission rules declared by the agent this loop will treat as current.
///
/// `AgentLoop::new` seeds `state.current_agent` with the built-in `build`
/// agent (`src/agent/loop.rs:156`), and the checker is built once, so the
/// rules come from that same agent identity the rest of the loop already
/// reports — the `agent_id` on tool calls, todo policy, and snapshot
/// capture all read `state.current_agent`. Anything else would be a guess
/// about which agent "should" be active.
pub fn active_agent_rules(agents: &[crate::agent::Agent]) -> crate::permission::PermissionRuleset {
    agents
        .iter()
        .find(|agent| agent.name == "build")
        .map(crate::agent::Agent::permission_ruleset)
        .unwrap_or_default()
}

/// Build a fully initialized loop from the daemon-resolved turn identity.
pub fn build_agent_loop(input: AgentLoopBuildInput) -> crate::agent::r#loop::AgentLoop {
    let permission_checker = crate::permission::PermissionChecker::new(
        Some(&input.config),
        crate::permission::approval::canonical_permission_store_path(),
    )
    // An agent's own declaration is authoritative while the operator has not
    // spoken for that tool. Wired before `with_active_mode` so an active
    // built-in mode still narrows a tool the agent allowed
    // (`merge_rulesets` lets the later ruleset win a tie).
    .with_agent_rules(active_agent_rules(&input.agents))
    .with_workspace_root(input.execution.workspace_root.clone())
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
    agent_loop.set_provider_registry(input.provider_registry);
    agent_loop.set_workspace_id(input.execution.workspace_id.clone());
    if let Some(lease) = input.workspace_service_lease {
        agent_loop.set_workspace_services_lease(lease);
    }
    if let Some(spool) = input.subagent_pool {
        agent_loop.set_subagent_pool(spool);
    }
    if let Some(observer) = input.credential_observer {
        agent_loop.set_credential_observer(observer);
    }
    if let Some(submission) = input.submission {
        agent_loop.set_submission(submission);
    }
    agent_loop.set_task_state_policy(input.task_state_policy);
    if let Some(svc) = input.notification_service {
        agent_loop.set_notification_service(svc);
    }
    if let Some(svc) = input.job_notification_service {
        agent_loop.set_job_notification_service(svc);
    }
    if let Some(svc) = input.ci_watch_service {
        agent_loop.set_ci_watch_service(svc);
    }
    if let Some(mode) = input.approval_mode {
        agent_loop.set_approval_mode(mode);
    }
    if let Some(profile) = input.sandbox_profile {
        agent_loop.set_sandbox_profile(profile);
    }
    agent_loop
}
