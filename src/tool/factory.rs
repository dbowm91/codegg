use std::sync::Arc;

use sqlx::SqlitePool;

use crate::config::schema::Config;
use crate::context::{ContextArtifactStore, InMemoryArtifactStore};
use crate::model_profile::types::TaskStatePolicy;
use crate::tool::integrated_config;
use crate::tool::{ToolRegistry, ToolRegistryOptions};

/// Build a session-scoped [`ToolRegistry`] with default tools, goal tools,
/// and the optional task tool (when a task tool runtime is available).
///
/// Returns the registry and a shared artifact store. The artifact store
/// is used both by `context_read` (registered in the registry when
/// context projection is enabled) and by the agent loop for capturing
/// tool output artifacts.
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
) -> (ToolRegistry, Arc<dyn ContextArtifactStore>) {
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

    // Build the run store rooted at `.codegg/runs/` in the workspace.
    let run_store: Option<Arc<dyn codegg_core::run_store::RunStore>> =
        std::env::current_dir()
            .ok()
            .map(|cwd| cwd.join(".codegg").join("runs"))
            .map(|root| {
                let store = codegg_core::run_store::FsRunStore::new(root);
                Arc::new(store) as Arc<dyn codegg_core::run_store::RunStore>
            });

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
