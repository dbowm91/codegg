use std::sync::Arc;

use crate::config::schema::Config;
use crate::error::AppError;
use crate::provider::ChatRequest;

/// Task-aware metadata for assembling LSP context for a single turn.
///
/// Pass-through of workflow metadata that the LSP context pipeline
/// can use to collect *task-specific* evidence (rather than the
/// generic status-only section the runtime injects when no
/// metadata is supplied).
///
/// All fields are optional. The runtime behaves as follows:
///
/// - **All fields empty / `None`** — emit a generic LSP status
///   section (current Phase 5 behavior).
/// - **Some `changed_files` or `hunks`** — collect a real
///   [`egglsp::LspContextPacket`] via the production evidence
///   adapter, then render it through
///   [`egglsp::render_lsp_context_for_agent`] using the supplied
///   model tier.
/// - **`review_mode = true`** — also tag collected evidence with
///   [`egglsp::AgentContextSource::SecurityContext`] for security
///   review workflows (the security-context path consumes this in
///   Pass 5).
/// - **`security_review_mode = true`** — escalates the request and
///   surfaces security-relevant diagnostics + symbols first.
///
/// All other fields are passed through unchanged.
#[derive(Debug, Default, Clone)]
pub struct LspAgentContextInput {
    /// Files changed in this turn (from a diff or pending edits).
    pub changed_files: Vec<std::path::PathBuf>,
    /// Hunk descriptors (old_start, new_start, etc.) for each
    /// `changed_files` entry. Optional — when present, hunk-local
    /// evidence is boosted in the context packet.
    pub hunks: Vec<egglsp::hunk_context::HunkDescriptor>,
    /// The file the agent is currently focused on, if any.
    pub active_file: Option<std::path::PathBuf>,
    /// Cursor position in the active file (0-indexed line/col).
    pub cursor_position: Option<egglsp::lsp_types::Position>,
    /// Whether this turn is a generic review workflow.
    pub review_mode: bool,
    /// Whether this turn is the `/security-review` flow.
    pub security_review_mode: bool,
    /// Optional explicit model tier override. When `None`, the
    /// runtime derives a tier from the resolved model profile.
    pub model_tier: Option<egglsp::ModelTier>,
}

impl LspAgentContextInput {
    /// `true` when no task-specific metadata is set — the runtime
    /// should fall back to status-only.
    pub fn is_empty(&self) -> bool {
        self.changed_files.is_empty()
            && self.hunks.is_empty()
            && self.active_file.is_none()
            && self.cursor_position.is_none()
    }

    /// `true` when this input has enough metadata to drive a
    /// task-specific context collection (changed files, hunks, or
    /// an active file).
    ///
    /// Mode flags (`review_mode`, `security_review_mode`) are
    /// signals for downstream consumers (security review workflow,
    /// hunk/source navigation) — they do **not** by themselves
    /// trigger task-specific LSP context collection. Use the
    /// presence of `changed_files`/`hunks`/`active_file` to decide
    /// whether to emit a richer LSP section.
    pub fn has_workflow_metadata(&self) -> bool {
        !self.changed_files.is_empty() || !self.hunks.is_empty() || self.active_file.is_some()
    }
}

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
    /// Shared LSP service for injecting LSP context into the system prompt.
    /// `None` when LSP is not available (e.g. socket mode).
    pub lsp_service: Option<Arc<crate::lsp::service::LspService>>,
    /// Optional task-aware metadata for assembling LSP context.
    /// When absent, the runtime injects a generic status section.
    /// When present, the runtime collects an `LspContextPacket`
    /// using the production evidence adapter and renders it.
    pub lsp_context_input: Option<LspAgentContextInput>,
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
        use crate::agent::agent_loop_factory::{AgentLoopBuildInput, AgentLoopFactory};

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
            lsp_service,
            lsp_context_input,
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
        let (tool_registry, artifact_store) = crate::tool::factory::build_session_tool_registry(
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
                    crate::goal::render::render_goal_context(&goal, checkpoint_excerpt.as_deref())
                }
                _ => String::new(),
            }
        } else {
            String::new()
        };
        system.push_str(&goal_context);

        // ── LSP context ──────────────────────────────────────────────
        if let Some(ref svc) = lsp_service {
            use std::path::PathBuf;
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let lsp_ctx =
                assemble_lsp_context_for_turn(svc, lsp_context_input, &model_profile.family, root)
                    .await;
            if let Some(lsp_ctx) = lsp_ctx {
                system.push_str(&lsp_ctx);
            }
        }

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
            artifact_store,
        };
        let runtime_provider = crate::agent::agent_loop_factory::DefaultAgentLoopFactory;
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
                crate::bus::global::GlobalEventBus::publish(crate::bus::events::AppEvent::Error {
                    message: format!("Agent error: {}", e),
                });
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

        Ok(TurnRunOutput {
            cancel_tx,
            steer_tx,
        })
    }
}

// ---------------------------------------------------------------------------
// LSP context assembly helpers
// ---------------------------------------------------------------------------

/// Resolve the renderer tier for a turn's LSP context input.
///
/// Precedence chain:
/// 1. Explicit per-request tier override (`input.model_tier` already set)
/// 2. Model family heuristic (via [`egglsp::model_tier_for_profile`])
/// 3. Default to Workhorse
///
/// Exposed `pub(crate)` so turn-runtime tests can verify the wiring
/// without spinning up an agent loop.
pub(crate) fn resolve_lsp_context_tier(
    input: Option<&mut LspAgentContextInput>,
    model_family: &str,
) -> egglsp::context_policy::TierResolution {
    use egglsp::context_policy::{TierResolution, TierSource};

    let mut notes = Vec::new();

    if let Some(inp) = input {
        if let Some(tier) = inp.model_tier {
            notes.push(format!("using explicit tier override: {tier}"));
            return TierResolution {
                tier,
                source: TierSource::ExplicitOverride,
                notes,
            };
        }
        let tier = egglsp::model_tier_for_profile(model_family);
        notes.push(format!(
            "resolved from model family '{model_family}': {tier}"
        ));
        TierResolution {
            tier,
            source: TierSource::ModelFamily,
            notes,
        }
    } else {
        notes.push("no LSP context input; defaulting to Workhorse".to_string());
        TierResolution {
            tier: egglsp::ModelTier::Workhorse,
            source: TierSource::Default,
            notes,
        }
    }
}

fn infer_workflow_from_input(
    input: &LspAgentContextInput,
) -> egglsp::workflow_recipes::LspWorkflowRecipe {
    if input.security_review_mode {
        egglsp::workflow_recipes::LspWorkflowRecipe::SecurityReviewEnriched
    } else if input.review_mode {
        egglsp::workflow_recipes::LspWorkflowRecipe::ReviewDiff
    } else if !input.hunks.is_empty() {
        egglsp::workflow_recipes::LspWorkflowRecipe::RepairHunk
    } else if input.active_file.is_some() {
        egglsp::workflow_recipes::LspWorkflowRecipe::RepairLocal
    } else {
        egglsp::workflow_recipes::LspWorkflowRecipe::ReviewDiff
    }
}

/// Build the LSP context section that gets appended to the system
/// prompt for a turn.
///
/// This is the production assembly path used by
/// [`DefaultTurnRuntime::run_turn`]. It:
///
/// 1. Resolves the renderer tier from the model profile family
///    (unless the caller set an explicit override).
/// 2. Builds an [`egglsp::LspContextPolicy`] from the resolved
///    tier, inferred workflow, and task risk.
/// 3. Routes to the task-aware collection path when the input has
///    workflow metadata, otherwise to the status-only path.
/// 4. Returns the rendered section or `None` when the LSP service
///    has no clients and the empty-packet fallback path produces
///    nothing.
///
/// `pub(crate)` so turn-runtime tests can exercise the path
/// directly. The `model_family` argument is the resolved
/// [`crate::model_profile::ResolvedModelProfile::family`] string.
pub(crate) async fn assemble_lsp_context_for_turn(
    lsp_service: &Arc<crate::lsp::service::LspService>,
    lsp_context_input: Option<LspAgentContextInput>,
    model_family: &str,
    allowed_root: std::path::PathBuf,
) -> Option<String> {
    use crate::tool::lsp::LspTool;

    let tool = LspTool::new(Arc::clone(lsp_service)).with_allowed_root(allowed_root);

    let mut input = lsp_context_input;
    let resolution = resolve_lsp_context_tier(input.as_mut(), model_family);

    let tier = resolution.tier;
    let workflow = input
        .as_ref()
        .map(infer_workflow_from_input)
        .unwrap_or(egglsp::workflow_recipes::LspWorkflowRecipe::ReviewDiff);

    let task_risk = if input.as_ref().is_some_and(|i| i.security_review_mode) {
        egglsp::context_policy::LspTaskRisk::SecuritySensitive
    } else {
        egglsp::context_policy::LspTaskRisk::Normal
    };

    let mut policy = egglsp::context_policy::LspContextPolicy::resolve(
        tier,
        workflow,
        task_risk,
        None,
        None,
        None,
        None,
        None,
    );
    policy.tier_source = resolution.source;

    tracing::debug!("{}", policy.policy_summary());

    if input.as_ref().is_some_and(|i| i.has_workflow_metadata()) {
        tool.lsp_context_for_agent_with_input(input.as_ref()).await
    } else {
        tool.lsp_context_for_agent().await
    }
}

// ---------------------------------------------------------------------------
// Tests — Pass 5: tier-resolution wiring
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tier_resolution_tests {
    use super::*;
    use egglsp::context_policy::TierSource;

    fn make_input_with_metadata() -> LspAgentContextInput {
        LspAgentContextInput {
            changed_files: vec![std::path::PathBuf::from("src/lib.rs")],
            ..Default::default()
        }
    }

    #[test]
    fn turn_runtime_small_model_uses_small_lsp_render_tier() {
        for family in ["tool_fragile", "local_strict", "fast_executor"] {
            let mut input = make_input_with_metadata();
            let resolution = resolve_lsp_context_tier(Some(&mut input), family);
            assert_eq!(
                resolution.source,
                TierSource::ModelFamily,
                "family={family} should resolve from family"
            );
            assert_eq!(
                resolution.tier,
                egglsp::ModelTier::Small,
                "family={family} should produce Small tier"
            );
        }
    }

    #[test]
    fn turn_runtime_frontier_model_uses_frontier_lsp_render_tier() {
        for family in [
            "frontierreasoning",
            "frontier_executor",
            "longcontextplanner",
            "default",
        ] {
            let mut input = make_input_with_metadata();
            let resolution = resolve_lsp_context_tier(Some(&mut input), family);
            assert_eq!(
                resolution.source,
                TierSource::ModelFamily,
                "family={family} should resolve from family"
            );
            assert_eq!(
                resolution.tier,
                egglsp::ModelTier::Frontier,
                "family={family} should produce Frontier tier"
            );
        }
    }

    #[test]
    fn turn_runtime_unknown_family_defaults_workhorse() {
        for family in ["some-new-vendor/some-model", "anthropic-unknown", ""] {
            let mut input = make_input_with_metadata();
            let resolution = resolve_lsp_context_tier(Some(&mut input), family);
            assert_eq!(
                resolution.source,
                TierSource::ModelFamily,
                "family={family:?} should resolve from family"
            );
            assert_eq!(
                resolution.tier,
                egglsp::ModelTier::Workhorse,
                "family={family:?} should produce Workhorse tier"
            );
        }

        let resolution = resolve_lsp_context_tier(None, "any");
        assert_eq!(resolution.tier, egglsp::ModelTier::Workhorse);
        assert_eq!(resolution.source, TierSource::Default);
    }

    #[test]
    fn turn_runtime_explicit_tier_override_is_preserved() {
        for tier in [
            egglsp::ModelTier::Small,
            egglsp::ModelTier::Workhorse,
            egglsp::ModelTier::Frontier,
        ] {
            let mut input = LspAgentContextInput {
                changed_files: vec![std::path::PathBuf::from("src/lib.rs")],
                model_tier: Some(tier),
                ..Default::default()
            };
            let resolution = resolve_lsp_context_tier(Some(&mut input), "frontierreasoning");
            assert_eq!(resolution.source, TierSource::ExplicitOverride);
            assert_eq!(resolution.tier, tier);
        }
    }

    #[test]
    fn turn_runtime_infer_workflow_from_input_security_review() {
        let input = LspAgentContextInput {
            security_review_mode: true,
            ..Default::default()
        };
        assert_eq!(
            infer_workflow_from_input(&input),
            egglsp::workflow_recipes::LspWorkflowRecipe::SecurityReviewEnriched
        );
    }

    #[test]
    fn turn_runtime_infer_workflow_from_input_review_mode() {
        let input = LspAgentContextInput {
            review_mode: true,
            ..Default::default()
        };
        assert_eq!(
            infer_workflow_from_input(&input),
            egglsp::workflow_recipes::LspWorkflowRecipe::ReviewDiff
        );
    }

    #[test]
    fn turn_runtime_infer_workflow_from_input_with_hunks() {
        let input = LspAgentContextInput {
            hunks: vec![egglsp::HunkDescriptor {
                id: "h1".to_string(),
                file_path: "src/lib.rs".to_string(),
                old_range: Some(egglsp::hunk_context::HunkLineRange {
                    start_line: 1,
                    end_line: 5,
                }),
                new_range: Some(egglsp::hunk_context::HunkLineRange {
                    start_line: 1,
                    end_line: 10,
                }),
                header: None,
                added_lines: 5,
                removed_lines: 0,
                context_lines: 3,
            }],
            ..Default::default()
        };
        assert_eq!(
            infer_workflow_from_input(&input),
            egglsp::workflow_recipes::LspWorkflowRecipe::RepairHunk
        );
    }

    #[test]
    fn turn_runtime_infer_workflow_from_input_active_file() {
        let input = LspAgentContextInput {
            active_file: Some(std::path::PathBuf::from("src/main.rs")),
            ..Default::default()
        };
        assert_eq!(
            infer_workflow_from_input(&input),
            egglsp::workflow_recipes::LspWorkflowRecipe::RepairLocal
        );
    }

    #[test]
    fn turn_runtime_infer_workflow_from_input_default() {
        let input = LspAgentContextInput {
            changed_files: vec![std::path::PathBuf::from("src/lib.rs")],
            ..Default::default()
        };
        assert_eq!(
            infer_workflow_from_input(&input),
            egglsp::workflow_recipes::LspWorkflowRecipe::ReviewDiff
        );
    }

    #[test]
    fn turn_runtime_truncation_notes_visible_for_small_model() {
        // Small tier must keep truncation notes visible — Small is a
        // content-breadth filter, not a verbosity filter. We assert
        // this at the renderer level (the wiring that actually
        // exercises Small in the production path) by constructing an
        // `LspContextRenderConfig` with `model_tier: Small` (the same
        // shape the runtime produces when `model_tier_for_profile`
        // returns Small for a tool-fragile model) and rendering a
        // packet that carries truncation notes.
        use egglsp::context::{
            LspContextItem, LspContextItemKind, LspContextPacket, LspContextPacketMode,
            LspContextRequest, LspContextScore, LspContextTruncation, LspEvidenceFreshness,
            LspEvidenceProvenance, LspRiskMode,
        };
        use std::path::PathBuf;

        let file = PathBuf::from("src/lib.rs");
        let hunk_def = LspContextItem {
            kind: LspContextItemKind::Definition,
            file: file.clone(),
            range: None,
            line: Some(10),
            column: None,
            message: "hunk-local def".to_string(),
            symbol: None,
            source: None,
            provenance: LspEvidenceProvenance {
                server_id: "test".to_string(),
                server_generation: Some(1),
                operation: "definition".to_string(),
                freshness: LspEvidenceFreshness::Fresh,
                capability_decision: None,
                document_version: None,
                age_ms: None,
                post_restart: false,
            },
            score: LspContextScore {
                priority: 10,
                is_hunk_local: true,
                is_error: false,
                is_same_file: true,
                freshness_rank: 0,
            },
            payload: None,
        };
        // A cross-file reference that Small tier must drop.
        let cross_ref = LspContextItem {
            kind: LspContextItemKind::Reference,
            file: PathBuf::from("other.rs"),
            range: None,
            line: Some(2),
            column: None,
            message: "cross-file ref".to_string(),
            symbol: None,
            source: None,
            provenance: LspEvidenceProvenance {
                server_id: "test".to_string(),
                server_generation: Some(1),
                operation: "findReferences".to_string(),
                freshness: LspEvidenceFreshness::Fresh,
                capability_decision: None,
                document_version: None,
                age_ms: None,
                post_restart: false,
            },
            score: LspContextScore {
                priority: 10,
                is_hunk_local: false,
                is_error: false,
                is_same_file: false,
                freshness_rank: 0,
            },
            payload: None,
        };
        let truncation = LspContextTruncation {
            references_truncated: true,
            notes: vec!["references truncated at 5".to_string()],
            ..LspContextTruncation::default()
        };
        let packet = LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: vec![file],
                hunks: vec![],
                risk_mode: LspRiskMode::Standard,
            },
            items: vec![hunk_def, cross_ref],
            previews: vec![],
            preview_ids: vec![],
            mode: LspContextPacketMode::Opportunistic,
            workspace_root: None,
            generated_at: None,
            server_id: Some("test".to_string()),
            server_generation: Some(1),
            operational_state: None,
            budget: None,
            notes: vec![],
            truncation,
        };

        // Simulate the runtime-produced Small-tier config.
        let small_config = egglsp::LspContextRenderConfig {
            model_tier: egglsp::ModelTier::Small,
            ..Default::default()
        };
        let rendered = egglsp::render_lsp_context_for_agent(&packet, &small_config);

        // Truncation notes are visible.
        assert!(
            rendered.contains("references truncated"),
            "Small tier must keep truncation notes visible: {rendered}"
        );
        // Hunk-local diagnostics/definitions are present.
        assert!(
            rendered.contains("## Definitions (hunk-local)"),
            "Small tier must show hunk-local definitions section: {rendered}"
        );
        // Cross-file broad references are absent.
        assert!(
            !rendered.contains("## References"),
            "Small tier must omit cross-file references: {rendered}"
        );
        assert!(
            !rendered.contains("cross-file ref"),
            "Small tier must not include cross-file reference content: {rendered}"
        );
    }

    #[test]
    fn turn_runtime_frontier_tier_keeps_references_visible() {
        // Frontier tier keeps the references section when the
        // packet contains reference items. Mirrors the runtime
        // wiring for frontier_reasoning family.
        use egglsp::context::{
            LspContextItem, LspContextItemKind, LspContextPacket, LspContextPacketMode,
            LspContextRequest, LspContextScore, LspEvidenceFreshness, LspEvidenceProvenance,
            LspRiskMode,
        };
        use std::path::PathBuf;

        let file = PathBuf::from("src/lib.rs");
        let reference = LspContextItem {
            kind: LspContextItemKind::Reference,
            file: file.clone(),
            range: None,
            line: Some(5),
            column: None,
            message: "ref".to_string(),
            symbol: None,
            source: None,
            provenance: LspEvidenceProvenance {
                server_id: "test".to_string(),
                server_generation: Some(1),
                operation: "findReferences".to_string(),
                freshness: LspEvidenceFreshness::Fresh,
                capability_decision: None,
                document_version: None,
                age_ms: None,
                post_restart: false,
            },
            score: LspContextScore {
                priority: 10,
                is_hunk_local: false,
                is_error: false,
                is_same_file: true,
                freshness_rank: 0,
            },
            payload: None,
        };
        let packet = LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: vec![file],
                hunks: vec![],
                risk_mode: LspRiskMode::Standard,
            },
            items: vec![reference],
            previews: vec![],
            preview_ids: vec![],
            mode: LspContextPacketMode::Opportunistic,
            workspace_root: None,
            generated_at: None,
            server_id: Some("test".to_string()),
            server_generation: Some(1),
            operational_state: None,
            budget: None,
            notes: vec![],
            truncation: Default::default(),
        };

        let frontier_config = egglsp::LspContextRenderConfig {
            model_tier: egglsp::ModelTier::Frontier,
            ..Default::default()
        };
        let rendered = egglsp::render_lsp_context_for_agent(&packet, &frontier_config);
        assert!(
            rendered.contains("## References"),
            "Frontier tier must keep references section: {rendered}"
        );
        assert!(rendered.contains("ref"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn turn_runtime_assemble_helper_returns_none_without_clients() {
        // End-to-end smoke for `assemble_lsp_context_for_turn` with
        // an empty LspService: with no clients and no metadata the
        // status-only path returns None, matching the pre-refactor
        // behavior. The key invariant: the helper compiles and
        // resolves the tier without panicking, and the no-metadata
        // path still routes to status-only.
        use std::path::PathBuf;
        let service = crate::lsp::service::LspService::new_arc(crate::lsp::config_lsp_to_egglsp(
            crate::config::schema::LspConfig::default(),
        ));
        let none =
            assemble_lsp_context_for_turn(&service, None, "tool_fragile", PathBuf::from("/tmp"))
                .await;
        assert!(
            none.is_none(),
            "status-only path with no clients must return None, got {none:?}"
        );
    }
}
