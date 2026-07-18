use std::sync::Arc;

use sqlx::SqlitePool;

use crate::config::schema::Config;
use crate::context::{ContextArtifactStore, InMemoryArtifactStore};
use crate::model_profile::types::TaskStatePolicy;
use crate::tool::integrated_config;
use crate::tool::{ToolRegistry, ToolRegistryOptions};

use codegg_core::workspace::ExecutionContext;

/// Immutable runtime-asset context captured for one session/turn tool
/// registry. The snapshot is fixed for the turn; the pin records only
/// path-free activation digests.
#[derive(Default)]
pub struct RuntimeAssetContext {
    pub snapshot: Option<Arc<crate::agent::asset_snapshot::ProjectAssetSnapshot>>,
    pub pin: Option<Arc<std::sync::Mutex<crate::agent::asset_snapshot::RuntimeAssetPin>>>,
}

/// Session-scoped inputs shared by the scheduler-backed task tool and the
/// immutable runtime-asset surface.
#[derive(Default)]
pub struct SessionToolContext {
    pub submission: Option<Arc<crate::scheduler::JobSubmissionService>>,
    pub runtime_assets: RuntimeAssetContext,
}

/// Build a session-scoped [`ToolRegistry`] with default tools, goal tools,
/// and the optional task tool (when a task tool runtime is available).
///
/// Returns the registry and a shared artifact store. The artifact store
/// is used both by `context_read` (registered in the registry when
/// context projection is enabled) and by the agent loop for capturing
/// tool output artifacts.
///
/// `execution` is the immutable daemon-resolved workspace context. The
/// factory anchors the per-session `RunStore` at
/// `<workspace_root>/.codegg/runs` instead of inferring it from process
/// cwd. Pre-Phase-2 callers without an execution context should use the
/// `_legacy_no_workspace_root` variant, which is reserved for tests that
/// have not yet been migrated and explicitly opt out.
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
    parent_model: Option<String>,
    execution: Arc<ExecutionContext>,
    session_context: SessionToolContext,
) -> (ToolRegistry, Arc<dyn ContextArtifactStore>) {
    let SessionToolContext {
        submission,
        runtime_assets: asset_context,
    } = session_context;
    let todo_state = Arc::new(tokio::sync::Mutex::new(crate::task_state::TodoState::new()));

    // Determine whether context_read should be registered.
    let ctx_config = config.context.as_ref();
    let artifact_store_enabled = ctx_config.and_then(|c| c.artifact_store).unwrap_or(true);
    let _project_enabled = ctx_config
        .and_then(|c| c.project_tool_outputs)
        .unwrap_or(true);
    let context_read_enabled = artifact_store_enabled;

    let artifact_store: Arc<dyn ContextArtifactStore> = Arc::new(InMemoryArtifactStore::new());

    let integrated = integrated_config::resolve_integrated_config(config);

    // Build the run store rooted at `<workspace_root>/.codegg/runs`.
    // Phase 2: this is propagated from the execution context, never from
    // process-global cwd. Tools that need `.codegg/runs` reads/writes
    // should expect this layout.
    let run_store: Option<Arc<dyn codegg_core::run_store::RunStore>> = {
        let root = execution.workspace_root.join(".codegg").join("runs");
        let store = codegg_core::run_store::FsRunStore::new(root);
        Some(Arc::new(store) as Arc<dyn codegg_core::run_store::RunStore>)
    };

    let mut tool_registry = ToolRegistry::with_options(ToolRegistryOptions {
        todo_state: Some(todo_state),
        todo_policy: Some(task_state_policy),
        pool: pool.clone(),
        session_id: Some(session_id.to_string()),
        lsp_service: None,
        tool_backends: crate::tool::ToolBackendConfig::from_config(config),
        context_artifact_store: if context_read_enabled {
            Some(artifact_store.clone())
        } else {
            None
        },
        context_session_id: if context_read_enabled {
            Some(session_id.to_string())
        } else {
            None
        },
        context_read_enabled,
        lsp_cache_config: crate::tool::convert_lsp_cache_config(&config.lsp_semantic_cache),
        evidence_config: integrated.evidence,
        deterministic_config: integrated.deterministic,
        preflight_config: integrated.preflight,
        run_store,
        submission: submission.clone(),
        command_intent: config.command_intent.clone(),
        workspace_root: Some(execution.workspace_root.clone()),
        asset_snapshot: asset_context.snapshot,
        asset_pin: asset_context.pin,
    });

    // Register the task/subagent tool when a runtime is available.
    if let Some(runtime) = task_tool_runtime {
        let task_tool = crate::tool::task::TaskTool::new(
            runtime.store(),
            runtime.spawner(),
            Some(session_id.to_string()),
            Vec::new(),
        )
        .with_parent_model(parent_model);
        let task_tool = if let Some(submission) = submission {
            task_tool.with_submission(submission, execution.workspace_root.clone())
        } else {
            task_tool
        };
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

    (tool_registry, artifact_store)
}

/// Build a `ToolRegistry` for tests/integration paths that have not yet
/// been migrated to propagate an [`ExecutionContext`]. The RunStore is
/// anchored at `<cwd>/.codegg/runs`. New production code MUST NOT call
/// this — use [`build_session_tool_registry`] with a real
/// `ExecutionContext` instead.
#[doc(hidden)]
pub fn build_session_tool_registry_legacy(
    config: &Config,
    pool: Option<SqlitePool>,
    session_id: &str,
    task_tool_runtime: Option<&crate::agent::task_tool_runtime::TaskToolRuntime>,
    task_state_policy: TaskStatePolicy,
    parent_model: Option<String>,
) -> (ToolRegistry, Arc<dyn ContextArtifactStore>) {
    // Construct a synthetic execution context rooted at the current
    // process cwd. This is only valid for tests where the workspace
    // identity is not observed.
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let synthetic_id = codegg_core::workspace::WorkspaceId::new_unchecked(format!(
        "legacy-test-ws-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let synthetic_record = Arc::new(codegg_core::workspace::WorkspaceRecord {
        id: synthetic_id,
        canonical_root: cwd.clone(),
        display_name: "legacy-test".to_string(),
        created_at: chrono::Utc::now(),
        last_opened_at: chrono::Utc::now(),
        archived_at: None,
    });
    let execution = codegg_core::workspace::ExecutionContext::new(
        synthetic_record,
        Some(session_id.to_string()),
        Default::default(),
    );
    build_session_tool_registry(
        config,
        pool,
        session_id,
        task_tool_runtime,
        task_state_policy,
        parent_model,
        execution,
        SessionToolContext::default(),
    )
}
