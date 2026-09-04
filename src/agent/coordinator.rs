//! Coordinator-owned runtime seams for the agent turn.
//!
//! `AgentLoop` is deliberately small in authority: it owns the turn identity,
//! lifecycle sequencing, and live control channels.  The concrete provider,
//! context, tool, recovery, persistence, and projection capabilities are
//! retained in this bundle so they have one explicit construction boundary.
//! The bundle is not a second policy engine; each member remains the handle to
//! the canonical owner documented by `architecture/agent.md`.

use crate::agent::progress_recovery::RecoveryController;
use crate::agent::router::ModelRouter;
use crate::agent::Agent;
use crate::bus::events::AppEvent;
use crate::config::schema::{Config, ContextPackerConfig, ContextPolicyConfig, ToolDeferralConfig};
use crate::context::compaction::ContextTracker;
use crate::context::policy::ContextPolicyRuntimeState;
use crate::context::{ContextArtifactStore, ContextCacheStats, ProjectionConfig};
use crate::permission::PermissionChecker;
use crate::provider::{Provider, ToolDefinition};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// All long-lived capabilities used by one loop.
///
/// Keeping these handles together makes the ownership boundary visible at the
/// type level.  In particular, the loop no longer presents provider, tool,
/// context, recovery, snapshot, goal, and projection policy as independent
/// fields that can be initialized or replaced piecemeal.
pub(super) struct AgentLoopServices {
    pub(super) provider: Box<dyn Provider>,
    pub(super) permission_checker: PermissionChecker,
    pub(super) tool_registry: crate::tool::ToolRegistry,
    pub(super) hook_registry: Option<Arc<crate::hooks::HookRegistry>>,
    pub(super) context_tracker: ContextTracker,
    pub(super) progress_recovery: RecoveryController,
    pub(super) recovery_parallel_limit: Option<usize>,
    pub(super) mcp_service: Option<Arc<RwLock<crate::mcp::McpService>>>,
    pub(super) tool_def_cache: Option<(
        Option<String>,
        bool,
        bool,
        String,
        u64,
        bool,
        Option<ToolDeferralConfig>,
        Vec<ToolDefinition>,
        Vec<ToolDefinition>,
    )>,
    pub(super) deferred_tool_definitions: Vec<ToolDefinition>,
    pub(super) model_router: ModelRouter,
    pub(super) snapshot_manager: Option<crate::snapshot::SnapshotManager>,
    pub(super) checkpoint_manager: Option<crate::snapshot::checkpoint::EditCheckpointManager>,
    pub(super) file_change_rx: broadcast::Receiver<AppEvent>,
    pub(super) usage_store: Option<Arc<crate::session::UsageStore>>,
    pub(super) security_service: crate::security::service::SecurityService,
    pub(super) todo_state: Arc<tokio::sync::Mutex<crate::task_state::TodoState>>,
    pub(super) task_state_policy: crate::model_profile::types::TaskStatePolicy,
    pub(super) todo_pool: Option<sqlx::SqlitePool>,
    pub(super) event_store: Option<Arc<crate::session::EventStore>>,
    pub(super) execution_policy: Option<crate::agent::policy::ExecutionPolicy>,
    pub(super) artifact_store: Arc<dyn ContextArtifactStore>,
    pub(super) projection_config: ProjectionConfig,
    pub(super) context_packer_config: ContextPackerConfig,
    pub(super) context_policy_config: ContextPolicyConfig,
    pub(super) context_cache_stats: ContextCacheStats,
    pub(super) context_plan_cache_key: Option<String>,
    pub(super) prompt_compiler_fingerprint: Option<String>,
    pub(super) base_request_tools: Vec<ToolDefinition>,
    pub(super) context_policy_runtime: ContextPolicyRuntimeState,
    pub(super) runtime_asset_pin:
        Option<Arc<std::sync::Mutex<crate::agent::asset_snapshot::RuntimeAssetPin>>>,
    pub(super) tool_broker: Arc<crate::tool::ToolBroker>,
    pub(super) notification_service:
        Option<Arc<crate::scheduler::tool_program_notifications::ToolProgramNotificationService>>,
    pub(super) run_control: Option<Arc<crate::agent::run_control::RunControlService>>,
    pub(super) goal_store: Option<Arc<crate::goal::GoalStore>>,
    pub(super) habit_store: Option<Arc<codegg_core::memory::habit::HabitStore>>,
    pub(super) agents: HashMap<String, Agent>,
    pub(super) config: Config,
}

/// Explicit lifecycle states used by the coordinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TurnPhase {
    Admission,
    ContextPreparation,
    ProviderInvocation,
    ToolExecution,
    Recovery,
    Completion,
}

/// Bounded transient sequencing state. Durable run state remains owned by the
/// run-control and RunStore services.
#[derive(Debug, Clone, Copy)]
pub(super) struct TurnLifecycle {
    phase: TurnPhase,
    turn_index: usize,
}

impl TurnLifecycle {
    pub(super) fn new() -> Self {
        Self {
            phase: TurnPhase::Admission,
            turn_index: 0,
        }
    }

    pub(super) fn begin_turn(&mut self, turn_index: usize) {
        self.turn_index = turn_index;
        self.phase = TurnPhase::ContextPreparation;
    }

    pub(super) fn set_phase(&mut self, phase: TurnPhase) {
        self.phase = phase;
    }

    #[cfg(test)]
    pub(super) fn phase(&self) -> TurnPhase {
        self.phase
    }

    #[cfg(test)]
    pub(super) fn turn_index(&self) -> usize {
        self.turn_index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_exposes_typed_phase_transitions() {
        let mut lifecycle = TurnLifecycle::new();
        assert_eq!(lifecycle.phase(), TurnPhase::Admission);
        lifecycle.begin_turn(3);
        assert_eq!(lifecycle.phase(), TurnPhase::ContextPreparation);
        assert_eq!(lifecycle.turn_index(), 3);
        lifecycle.set_phase(TurnPhase::ProviderInvocation);
        assert_eq!(lifecycle.phase(), TurnPhase::ProviderInvocation);
        lifecycle.set_phase(TurnPhase::ToolExecution);
        lifecycle.set_phase(TurnPhase::Recovery);
        lifecycle.set_phase(TurnPhase::Completion);
        assert_eq!(lifecycle.phase(), TurnPhase::Completion);
    }
}
