use std::sync::Arc;

use crate::config::schema::Config;
use crate::error::AppError;
use crate::provider::ChatRequest;

/// Everything needed to execute one agent turn.
///
/// This struct captures the raw inputs from a `TurnSubmit` request so the
/// runtime provider can build tool registries, permission checkers, system
/// prompts, and the agent loop without the daemon knowing about those types.
pub struct TurnRunInput {
    /// Session identifier.
    pub session_id: String,
    /// Raw agent DTOs from the protocol layer.
    pub agents_dto: Vec<codegg_protocol::dto::Agent>,
    /// Index into `agents_dto` for the active agent this turn.
    pub current_agent_idx: usize,
    /// Provider/model string, e.g. `"openai/gpt-4o"` or just `"gpt-4o"`.
    pub model: String,
    /// Raw message DTOs from the protocol layer (provider messages).
    pub messages_dto: Vec<codegg_protocol::dto::ProviderMessage>,
    /// Whether plan-mode is active for this turn.
    pub plan_mode: bool,
    /// Loaded configuration.
    pub config: Config,
    /// SQLite connection pool.
    pub pool: Option<sqlx::SqlitePool>,
    /// Sub-agent pool for task-tool registration.
    pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
    /// Memory store for user preferences / learned context.
    pub memory_store: Option<Arc<crate::memory::MemoryStore>>,
    /// Event log for publishing turn lifecycle events to remote clients.
    pub event_log: Arc<super::super::core::event_log::EventLog>,
    /// Unique identifier for this turn, used in event publishing.
    pub turn_id: String,
}

/// Minimal output from a turn execution.
///
/// Contains the control channels the daemon needs to store in the session
/// runtime's `TurnHandle` so external cancel/steer requests can be delivered.
pub struct TurnRunOutput {
    /// Sender to signal the agent loop to cancel.
    pub cancel_tx: tokio::sync::watch::Sender<bool>,
    /// Sender to deliver steering instructions to the agent loop.
    pub steer_tx: tokio::sync::mpsc::UnboundedSender<String>,
}

/// The turn runtime trait abstracts the full agent turn lifecycle.
///
/// Implementations build the tool registry, permission checker, agent loop,
/// and system prompt, then spawn the agent loop execution. The daemon owns
/// session-level concerns (active-turn bookkeeping, event publishing) while
/// the runtime owns everything needed to run the LLM turn.
#[async_trait::async_trait]
pub trait TurnRuntime: Send + Sync {
    /// Execute one agent turn.
    ///
    /// On success, returns [`TurnRunOutput`] containing the cancel/steer
    /// senders the caller must store in the session runtime.
    async fn run_turn(&self, input: TurnRunInput) -> Result<TurnRunOutput, AppError>;
}

/// Default implementation that delegates to the existing factory functions.
///
/// Builds the tool registry, permission checker, agent loop, assembles the
/// system prompt, bootstraps the search backend, and spawns the agent loop
/// in a background task.
pub struct DefaultTurnRuntime;

#[async_trait::async_trait]
impl TurnRuntime for DefaultTurnRuntime {
    async fn run_turn(&self, input: TurnRunInput) -> Result<TurnRunOutput, AppError> {
        use crate::agent::runtime_provider::{AgentLoopBuildInput, AgentRuntimeProvider};

        let TurnRunInput {
            session_id,
            agents_dto,
            current_agent_idx,
            model,
            messages_dto,
            plan_mode,
            config,
            pool,
            subagent_pool,
            memory_store,
            event_log,
            turn_id,
        } = input;

        // ── Provider resolution ──────────────────────────────────────
        let mut registry = crate::provider::ProviderRegistry::new();
        crate::provider::register_builtin_with_config(&mut registry, &config);

        let provider_name = model.split('/').next().unwrap_or("openai").to_string();
        let model_name = model.split('/').next_back().unwrap_or(&model).to_string();

        let base_provider = registry.get(&provider_name).ok_or_else(|| {
            AppError::Provider(crate::error::ProviderError::NotFound(format!(
                "Provider '{}' not found",
                provider_name
            )))
        })?;
        let provider = base_provider.clone_box();

        // ── Model profile / task-state policy ────────────────────────
        let model_profile =
            crate::model_profile::ModelProfileResolver::new(&config).resolve(&model_name);
        let task_state_policy = model_profile.task_state_policy;

        // ── Tool registry ────────────────────────────────────────────
        let task_tool_runtime = subagent_pool
            .as_ref()
            .map(crate::agent::task_tool_runtime::TaskToolRuntime::from_subagent_pool);
        let tool_registry = crate::tool::factory::build_session_tool_registry(
            &config,
            pool.clone(),
            &session_id,
            task_tool_runtime.as_ref(),
            task_state_policy.clone(),
        );

        // ── Memory context ───────────────────────────────────────────
        let memory_context = memory_store
            .as_ref()
            .map(|store| {
                let all_memories = store.list("user/preferences");
                if all_memories.is_empty() {
                    String::new()
                } else {
                    let summary: String = all_memories
                        .iter()
                        .take(10)
                        .map(|m| {
                            format!(
                                "- [{}] {}",
                                m.id,
                                m.title.as_deref().unwrap_or("(untitled)")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!("\n\n## Learned Preferences\n{}\n", summary)
                }
            })
            .unwrap_or_default();

        // ── System prompt assembly ───────────────────────────────────
        let agents = crate::protocol_conversions::dtos_to_agents(agents_dto.clone());

        let mut system = crate::agent::prompt::load_agent_prompt(
            &crate::protocol_conversions::dto_to_agent(agents_dto[current_agent_idx].clone()),
            &config,
            &model_name,
        );
        system.push_str(&memory_context);

        // Goal context
        let goal_context = if let Some(ref p) = pool {
            let goal_store = crate::goal::GoalStore::new(p.clone());
            match goal_store.active_for_session(&session_id).await {
                Ok(Some(goal)) if goal.status == crate::goal::GoalStatus::Active => {
                    let checkpoint_excerpt = if let Some(ref path) = goal.checkpoint_path {
                        crate::goal::checkpoint::read_checkpoint_excerpt(path, 4000)
                            .await
                            .ok()
                            .flatten()
                    } else {
                        None
                    };
                    crate::goal::render::render_goal_context(
                        &goal,
                        checkpoint_excerpt.as_deref(),
                    )
                }
                _ => String::new(),
            }
        } else {
            String::new()
        };
        system.push_str(&goal_context);

        if plan_mode {
            system.push_str("\n\n");
            system.push_str(crate::agent::prompt::plan_mode_contract());
        }

        // ── Search backend bootstrap ─────────────────────────────────
        let (mcp_service, _report) =
            crate::search_backend::bootstrap::bootstrap_search_backend(&config).await;

        // ── Agent loop construction ──────────────────────────────────
        let agent_loop_input = AgentLoopBuildInput {
            agents,
            provider,
            config,
            tool_registry,
            pool,
            session_id: session_id.clone(),
            subagent_pool,
            task_state_policy,
            mcp_service,
        };
        let runtime_provider = crate::agent::runtime_provider::DefaultAgentRuntimeProvider;
        let mut agent_loop = runtime_provider.build_agent_loop(agent_loop_input);
        agent_loop.load_persisted_todos().await;

        // ── Chat request ─────────────────────────────────────────────
        let request = ChatRequest {
            messages: crate::protocol_conversions::dtos_to_provider_messages(messages_dto),
            model: model_name,
            tools: None,
            system: Some(system),
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
        };

        // ── Cancel / steer channels ──────────────────────────────────
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        agent_loop.set_cancel_receiver(cancel_rx);

        let (steer_tx, steer_rx) = tokio::sync::mpsc::unbounded_channel();
        agent_loop.set_steer_receiver(steer_rx);

        // ── Spawn agent loop ─────────────────────────────────────────
        let session_id_for_spawn = session_id.clone();
        let turn_id_for_spawn = turn_id.clone();
        let event_log_for_spawn = event_log;
        tokio::spawn(async move {
            let result = agent_loop.run(request).await;
            if let Err(e) = result {
                tracing::error!("Agent loop error: {}", e);
                event_log_for_spawn
                    .publish(
                        Some(session_id_for_spawn.clone()),
                        Some(turn_id_for_spawn.clone()),
                        crate::protocol::core::CoreEvent::TurnFailed {
                            session_id: session_id_for_spawn.clone(),
                            turn_id: Some(turn_id_for_spawn.clone()),
                            message: format!("Agent error: {}", e),
                        },
                    )
                    .await;
                crate::bus::global::GlobalEventBus::publish(
                    crate::bus::events::AppEvent::Error {
                        message: format!("Agent error: {}", e),
                    },
                );
                crate::bus::global::GlobalEventBus::publish(
                    crate::bus::events::AppEvent::AgentFinished {
                        session_id: session_id_for_spawn,
                        stop_reason: "error".to_string(),
                        input_tokens: None,
                        output_tokens: None,
                        cached_tokens: None,
                        reasoning_tokens: None,
                    },
                );
            } else {
                event_log_for_spawn
                    .publish(
                        Some(session_id_for_spawn.clone()),
                        Some(turn_id_for_spawn.clone()),
                        crate::protocol::core::CoreEvent::TurnCompleted {
                            session_id: session_id_for_spawn.clone(),
                            turn_id: turn_id_for_spawn.clone(),
                            stop_reason: "completed".to_string(),
                        },
                    )
                    .await;
                crate::bus::global::GlobalEventBus::publish(
                    crate::bus::events::AppEvent::AgentFinished {
                        session_id: session_id_for_spawn,
                        stop_reason: "completed".to_string(),
                        input_tokens: None,
                        output_tokens: None,
                        cached_tokens: None,
                        reasoning_tokens: None,
                    },
                );
            }
        });

        Ok(TurnRunOutput { cancel_tx, steer_tx })
    }
}
