//! Per-turn request preparation: policy, routing, and tool definitions.
//!
//! Physical decomposition of [`super::r#loop`] (M003): agent/model-profile
//! application, automatic model routing, research-hint injection, context
//! frame assembly, persisted-todo loading, and model-facing tool-definition
//! assembly. Broker, permission, and disclosure authority are unchanged.

use super::r#loop::AgentLoop;
use super::tool_inspect::{compute_model_flags, filter_tools_for_model, mcp_tool_surface_revision};
use crate::bus::events::AppEvent;
use crate::provider::{ChatRequest, ContentPart, Message};

fn project_initial_tool_palette(
    mode: crate::agent::policy::ToolExposureMode,
    contextual_immediate: &std::collections::BTreeSet<String>,
    definitions: Vec<crate::provider::ToolDefinition>,
) -> Vec<crate::provider::ToolDefinition> {
    definitions
        .into_iter()
        .map(|mut definition| {
            let initially_advertised = contextual_immediate.contains(&definition.name)
                || match mode {
                    crate::agent::policy::ToolExposureMode::Full => true,
                    crate::agent::policy::ToolExposureMode::Curated => {
                        crate::tool::disclosure::CURATED_PALETTE.contains(&definition.name.as_str())
                    }
                    crate::agent::policy::ToolExposureMode::MinimalWithDiscovery => {
                        crate::tool::disclosure::MINIMAL_PALETTE.contains(&definition.name.as_str())
                    }
                };
            if !initially_advertised {
                definition.defer_loading = Some(true);
            }
            definition
        })
        .collect()
}

fn contextual_tool_names(
    context_read_available: bool,
    active_goal: bool,
    active_work_plan: bool,
) -> std::collections::BTreeSet<String> {
    let mut tools = std::collections::BTreeSet::new();
    if context_read_available {
        tools.insert("context_read".to_string());
    }
    if active_goal {
        tools.extend([
            "goal_get".to_string(),
            "goal_update_progress".to_string(),
            "goal_request_completion".to_string(),
        ]);
    }
    if active_work_plan {
        tools.extend([
            "work_plan_get".to_string(),
            "work_plan_update_item".to_string(),
        ]);
    }
    tools
}

struct PreturnDecisionConfig<'a> {
    engine: &'a std::sync::Arc<dyn codegg_core::decision::DecisionEngine>,
    timeout: std::time::Duration,
    mode: crate::tool_advisor::AdvisorMode,
    threshold: f64,
    max_promotions: usize,
    schema_budget: usize,
    max_candidates: usize,
}

#[cfg(test)]
struct PreturnDisclosureConfig<'a> {
    advisor: &'a dyn crate::tool_advisor::ToolAdvisor,
    mode: crate::tool_advisor::AdvisorMode,
    threshold: f64,
    max_promotions: usize,
    schema_budget: usize,
    max_candidates: usize,
}

/// Project proactive disclosure over the final resolved surface. The return
/// value is canonical-name-only; the caller revalidates it against each wire
/// definition immediately before provider palette construction.
async fn project_preturn_decision(
    surface: &crate::agent::tool_surface::ResolvedToolSurface,
    deferred: &[crate::provider::ToolDefinition],
    context: &str,
    config: PreturnDecisionConfig<'_>,
) -> std::collections::BTreeSet<String> {
    if matches!(
        config.mode,
        crate::tool_advisor::AdvisorMode::Off | crate::tool_advisor::AdvisorMode::Rerank
    ) || context.is_empty()
        || (config.mode == crate::tool_advisor::AdvisorMode::Promote && config.max_promotions == 0)
    {
        return std::collections::BTreeSet::new();
    }
    if !matches!(
        config.engine.state(),
        codegg_core::decision::BackendState::Ready
    ) || !config.engine.capabilities().rank
    {
        tracing::debug!(
            scope = "preturn_disclosure",
            backend_state = ?config.engine.state(),
            "decision backend cannot rank; palette unchanged"
        );
        return std::collections::BTreeSet::new();
    }
    let deferred_names: std::collections::BTreeSet<String> = deferred
        .iter()
        .map(|definition| definition.name.clone())
        .collect();
    let eligible = crate::tool_advisor::candidates_from_deferred_surface(surface, &deferred_names);
    let backend_limit = config.engine.capabilities().max_candidates.max(1);
    let (candidates, preselection) = crate::tool_advisor::preselect_candidates(
        eligible,
        context,
        config
            .max_candidates
            .min(crate::tool_advisor::MAX_CANDIDATES)
            .min(backend_limit),
    );
    tracing::debug!(
        scope = "preturn_disclosure",
        eligible_deferred = preselection.eligible_deferred,
        shortlisted = preselection.shortlisted,
        truncated = preselection.truncated,
        preselect_millis = preselection.elapsed_millis,
        "projected deferred-first decision shortlist"
    );
    if candidates.is_empty() {
        return std::collections::BTreeSet::new();
    }
    let input = crate::tool_advisor::ToolAdvisorInput {
        case_id: "preturn-disclosure".into(),
        context: context.into(),
        candidates,
        surface_fingerprint: surface.fingerprint.clone(),
    };
    use futures_util::FutureExt;
    let result =
        std::panic::AssertUnwindSafe(crate::tool_advisor::decision_adapter::rank_with_engine(
            config.engine,
            &input,
            config.timeout,
        ))
        .catch_unwind()
        .await;
    let result = match result {
        Ok(Ok(result)) => result,
        Ok(Err(_)) | Err(_) => {
            tracing::debug!(
                scope = "preturn_disclosure",
                "decision scoring failed; palette unchanged"
            );
            return std::collections::BTreeSet::new();
        }
    };
    tracing::debug!(
        scope = "preturn_disclosure",
        backend = %result.provenance.backend,
        model = ?result.provenance.model,
        runtime_version = ?result.provenance.runtime_version,
        latency_micros = result.provenance.latency_micros,
        "decision backend returned advisor observation"
    );
    apply_preturn_prediction(
        surface,
        deferred,
        &result.prediction,
        config.mode,
        config.threshold,
        config.max_promotions,
        config.schema_budget,
    )
}

#[cfg(test)]
fn project_preturn_promotions(
    surface: &crate::agent::tool_surface::ResolvedToolSurface,
    deferred: &[crate::provider::ToolDefinition],
    context: &str,
    config: PreturnDisclosureConfig<'_>,
) -> std::collections::BTreeSet<String> {
    if matches!(
        config.mode,
        crate::tool_advisor::AdvisorMode::Off | crate::tool_advisor::AdvisorMode::Rerank
    ) || context.is_empty()
        || (config.mode == crate::tool_advisor::AdvisorMode::Promote && config.max_promotions == 0)
    {
        return std::collections::BTreeSet::new();
    }
    let deferred_names: std::collections::BTreeSet<String> = deferred
        .iter()
        .map(|definition| definition.name.clone())
        .collect();
    // Deferred-first shortlisting: build the full eligible deferred universe
    // from the resolved surface before applying any candidate limit, so a
    // relevant deferred tool past the first-N surface window still reaches
    // the advisor. Authority filtering stays upstream in `ResolvedToolSurface`;
    // required/never-reduce entries are excluded here and revalidated again
    // at promotion time.
    let eligible = crate::tool_advisor::candidates_from_deferred_surface(surface, &deferred_names);
    let (candidates, preselection) = crate::tool_advisor::preselect_candidates(
        eligible,
        context,
        config
            .max_candidates
            .min(crate::tool_advisor::MAX_CANDIDATES),
    );
    tracing::debug!(
        scope = "preturn_disclosure",
        eligible_deferred = preselection.eligible_deferred,
        shortlisted = preselection.shortlisted,
        truncated = preselection.truncated,
        preselect_millis = preselection.elapsed_millis,
        "projected deferred-first advisor shortlist"
    );
    if candidates.is_empty() {
        return std::collections::BTreeSet::new();
    }
    let input = crate::tool_advisor::ToolAdvisorInput {
        case_id: "preturn-disclosure".into(),
        context: context.into(),
        candidates,
        surface_fingerprint: surface.fingerprint.clone(),
    };
    let prediction = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        config.advisor.score(&input)
    })) {
        Ok(Ok(prediction)) => prediction,
        Ok(Err(_)) | Err(_) => {
            tracing::debug!(
                scope = "preturn_disclosure",
                "advisor scoring failed; palette unchanged"
            );
            return std::collections::BTreeSet::new();
        }
    };
    if prediction.abstain_probability.unwrap_or(0.0) >= 0.5 {
        tracing::debug!(scope = "preturn_disclosure", "advisor abstained");
        return std::collections::BTreeSet::new();
    }
    if config.mode == crate::tool_advisor::AdvisorMode::Observe {
        tracing::debug!(
            scope = "preturn_disclosure",
            predictions = prediction.ranked.len(),
            "recorded pre-turn advisor observation"
        );
        return std::collections::BTreeSet::new();
    }
    apply_preturn_prediction(
        surface,
        deferred,
        &prediction,
        config.mode,
        config.threshold,
        config.max_promotions,
        config.schema_budget,
    )
}

fn apply_preturn_prediction(
    surface: &crate::agent::tool_surface::ResolvedToolSurface,
    deferred: &[crate::provider::ToolDefinition],
    prediction: &crate::tool_advisor::ToolAdvisorPrediction,
    mode: crate::tool_advisor::AdvisorMode,
    threshold: f64,
    max_promotions: usize,
    schema_budget: usize,
) -> std::collections::BTreeSet<String> {
    if prediction.abstain_probability.unwrap_or(0.0) >= 0.5 {
        tracing::debug!(scope = "preturn_disclosure", "advisor abstained");
        return std::collections::BTreeSet::new();
    }
    if mode == crate::tool_advisor::AdvisorMode::Observe {
        tracing::debug!(
            scope = "preturn_disclosure",
            predictions = prediction.ranked.len(),
            "recorded pre-turn advisor observation"
        );
        return std::collections::BTreeSet::new();
    }
    let scores: std::collections::BTreeMap<_, _> = prediction
        .ranked
        .iter()
        .map(|candidate| (candidate.name.as_str(), candidate.score))
        .collect();
    let mut selected = std::collections::BTreeSet::new();
    let mut bytes = 0usize;
    let mut ranked = deferred
        .iter()
        .filter_map(|definition| {
            let canonical = surface
                .wire_to_canonical
                .get(&definition.name)
                .map(String::as_str)
                .unwrap_or(&definition.name);
            let eligible = surface.tools.iter().any(|tool| {
                tool.canonical_name == canonical && !tool.required && !tool.never_reduce
            });
            if !eligible {
                return None;
            }
            scores
                .get(canonical)
                .copied()
                .filter(|score| *score >= threshold)
                .map(|score| (canonical.to_string(), score, definition))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    for (canonical, _, definition) in ranked {
        let definition_bytes = serde_json::to_vec(definition)
            .map(|bytes| bytes.len())
            .unwrap_or(0);
        if bytes.saturating_add(definition_bytes) > schema_budget {
            continue;
        }
        bytes = bytes.saturating_add(definition_bytes);
        selected.insert(canonical);
        if selected.len() >= max_promotions {
            break;
        }
    }
    tracing::debug!(
        scope = "preturn_disclosure",
        promoted = selected.len(),
        schema_bytes = bytes,
        "projected proactive advisor disclosure"
    );
    selected
}

impl AgentLoop {
    async fn current_advisor_context_v2(&self) -> String {
        let frame = self.build_context_frame().await;
        let mut work_plan_task: Option<String> = None;
        let mut work_plan_next_steps = Vec::new();
        if let Some(pool) = self.services.todo_pool.clone() {
            let store = codegg_core::work_plan::WorkPlanStore::new(pool);
            if let Ok(Some(plan)) = store.active_for_session(&self.session_id).await {
                if let Ok(items) = store.list_items(&plan.id).await {
                    work_plan_task = plan
                        .current_item_id
                        .as_ref()
                        .and_then(|id| items.iter().find(|item| &item.id == id))
                        .or_else(|| {
                            items.iter().find(|item| {
                                item.status == codegg_core::work_plan::WorkItemStatus::InProgress
                            })
                        })
                        .map(|item| item.description.clone());
                    work_plan_next_steps = items
                        .iter()
                        .filter(|item| {
                            matches!(
                                item.status,
                                codegg_core::work_plan::WorkItemStatus::Pending
                                    | codegg_core::work_plan::WorkItemStatus::Actionable
                            )
                        })
                        .take(2)
                        .map(|item| item.description.clone())
                        .collect();
                }
            }
        }
        crate::tool_advisor::context_v2::AdvisorContextV2::from_context_frame(
            self.current_user_prompt.as_deref(),
            self.original_user_prompt.as_deref(),
            &frame,
            work_plan_task.as_deref(),
            &work_plan_next_steps,
            None,
        )
        .serialize()
    }

    /// M004 observe-mode switch: default-off experimental config surface
    /// (`[tool_advisor.causal_frontier] mode = "off" | "observe"`).
    /// Config omission is behaviorally identical to current main. There is
    /// no active/promote mode in M004.
    fn causal_frontier_mode(&self) -> crate::tool_advisor::causal_observe::CausalFrontierMode {
        crate::tool_advisor::causal_observe::CausalFrontierMode::parse(
            self.services
                .config
                .tool_advisor
                .as_ref()
                .and_then(|advisor| advisor.causal_frontier.as_ref())
                .and_then(|frontier| frontier.mode.as_deref()),
        )
    }

    /// Build the bounded host-owned causal state inputs for observe mode.
    ///
    /// Every fact comes from its canonical host source (see
    /// `crate::tool_advisor::causal_frontier` module docs). Two pilot facts
    /// are unavailable at preparation time and stay absent rather than
    /// guessed: failed-test evidence (no structured per-turn execution
    /// status here — ledger result strings are never parsed) and LSP
    /// preview availability (the turn-local `PreviewArtifactRegistry` is
    /// not held by the loop at preparation time). Absent facts can only
    /// abstain the frontier, never widen it.
    async fn causal_observe_inputs(
        &self,
    ) -> crate::tool_advisor::causal_frontier::CausalStateInputs {
        use crate::tool_advisor::causal_frontier::CausalStateInputs;
        let mut inputs = CausalStateInputs::default();
        let goal = if let Some(store) = self.services.goal_store.clone() {
            store
                .active_for_session(&self.session_id)
                .await
                .ok()
                .flatten()
        } else {
            None
        };
        inputs.apply_goal(goal.as_ref());
        if let Some(pool) = self.services.todo_pool.clone() {
            let store = codegg_core::work_plan::WorkPlanStore::new(pool);
            match store.active_for_session(&self.session_id).await {
                Ok(Some(plan)) => {
                    let items = store.list_items(&plan.id).await.unwrap_or_default();
                    inputs.apply_work_plan(Some(&plan), &items);
                }
                _ => inputs.apply_work_plan(None, &[]),
            }
        }
        inputs.apply_ledger(&self.context_ledger);
        inputs.set_failed_tests(false);
        inputs.set_security_findings(self.recent_findings.len());
        inputs.set_context_read_available(self.services.tool_registry.contains("context_read"));
        inputs
    }

    async fn contextual_immediate_tools(&self) -> std::collections::BTreeSet<String> {
        let context_read_available = self.services.tool_registry.contains("context_read")
            && !self.context_ledger.artifact_handles.is_empty();
        let mut active_goal = false;

        if let Some(goal_store) = self.services.goal_store.clone() {
            active_goal = matches!(
                goal_store.active_for_session(&self.session_id).await,
                Ok(Some(goal)) if goal.status == crate::goal::model::GoalStatus::Active
            );
        }

        let mut active_work_plan = false;
        if let Some(pool) = self.services.todo_pool.clone() {
            let store = codegg_core::work_plan::WorkPlanStore::new(pool);
            active_work_plan = matches!(
                store.active_for_session(&self.session_id).await,
                Ok(Some(_))
            );
        }

        contextual_tool_names(context_read_available, active_goal, active_work_plan)
    }

    /// Project the policy's initial palette without deleting the allowed
    /// discovery universe.  Palette omission is represented as provider
    /// deferral; deny/disable/callability filtering happens before this
    /// projection and remains authoritative for both prompt and search.
    fn apply_tool_exposure_filter(
        &self,
        contextual_immediate: &std::collections::BTreeSet<String>,
        definitions: Vec<crate::provider::ToolDefinition>,
    ) -> Vec<crate::provider::ToolDefinition> {
        let Some(ref policy) = self.services.execution_policy else {
            return definitions;
        };

        let filtered = project_initial_tool_palette(
            policy.initial_tool_mode,
            contextual_immediate,
            definitions,
        );

        // Then apply model profile disabled_tools filter
        if let Some(ref disabled) = policy.disabled_tools {
            if !disabled.is_empty() {
                return filtered
                    .into_iter()
                    .filter(|t| !disabled.contains(&t.name))
                    .collect();
            }
        }

        filtered
    }

    // Keep this compatibility constructor available to embedded/test callers;
    // daemon production construction goes through `build_agent_loop`, whose
    // typed input binds the execution context before initialization.

    /// Evaluate the research trigger heuristic against a user prompt
    /// and, if it fires, prepend a hint to the next user message that
    /// tells the model about the `research` subagent. Returns
    /// `Some(hint)` when the hint was generated (caller can prepend
    /// it to the user-visible message), `None` otherwise.
    ///
    /// The trigger config lives at `config.research.auto_trigger`.
    /// When `enabled` is `false` or the confidence is below
    /// `min_confidence`, the hint is suppressed. Plan mode always
    /// suppresses the hint (research is not part of the plan-mode
    /// surface).
    pub fn maybe_inject_research_hint(&self, user_prompt: &str) -> Option<String> {
        if self.state.plan_mode {
            return None;
        }
        let trigger_cfg = self
            .services
            .config
            .research
            .as_ref()
            .and_then(|r| r.auto_trigger.clone())
            .unwrap_or_default();
        if !trigger_cfg.enabled {
            return None;
        }
        // Build a fresh TriggerConfig from the resolved profile (the
        // keyword lists live in the research module and are not part
        // of the user-facing schema).
        let trigger = crate::research::triggers::TriggerConfig {
            enabled: true,
            min_confidence: f64::from(trigger_cfg.min_confidence),
            ..Default::default()
        };
        let analysis = crate::research::triggers::analyze_trigger(user_prompt, &[], &[], &trigger);
        if !analysis.should_invoke {
            return None;
        }
        Some(format!(
            "[Hint: this task looks like a `{:?}` question (confidence: {:.2}). \
             Consider spawning a `research` subagent via \
             `task({{action: 'spawn', agent: 'research', prompt: '…'}})` for a structured, \
             multi-source answer with citations. You can also just use `websearch` for a quick lookup.]",
            analysis.suggested_mode,
            analysis.confidence,
        ))
    }

    pub fn execution_policy(&self) -> Option<&crate::agent::policy::ExecutionPolicy> {
        self.services.execution_policy.as_ref()
    }

    pub async fn build_context_frame(&self) -> crate::agent::context_frame::ContextFrame {
        let todo = self.services.todo_state.lock().await;
        let current_task = todo
            .items
            .iter()
            .find(|item| item.status == crate::task_state::TodoStatus::InProgress)
            .map(|item| item.content.clone());
        let next_steps: Vec<String> = todo
            .items
            .iter()
            .filter(|item| item.status == crate::task_state::TodoStatus::Pending)
            .take(3)
            .map(|item| item.content.clone())
            .collect();
        let security_findings: Vec<String> = self
            .recent_findings
            .iter()
            .map(|f| {
                let cat = format!("{:?}", f.category);
                format!("[{}] {}", cat, f.evidence)
            })
            .take(5)
            .collect();
        drop(todo);

        let mut frame = crate::agent::context_frame::ContextFrame {
            user_goal: self.original_user_prompt.clone(),
            current_task,
            constraints: Vec::new(),
            decisions: Vec::new(),
            touched_files: Vec::new(),
            commands_run: Vec::new(),
            test_results: Vec::new(),
            unresolved_errors: Vec::new(),
            security_findings,
            next_steps,
            artifact_handles: crate::agent::context_frame::bounded_artifact_handles(
                &self.context_ledger.artifact_handles,
            ),
        };

        let ledger_frame = self.context_ledger.to_context_frame();
        if !ledger_frame.touched_files.is_empty() {
            frame.touched_files = ledger_frame.touched_files;
        }
        if !ledger_frame.commands_run.is_empty() {
            frame.commands_run = ledger_frame.commands_run;
        }
        if !ledger_frame.test_results.is_empty() {
            frame.test_results = ledger_frame.test_results;
        }
        if !ledger_frame.unresolved_errors.is_empty() {
            frame.unresolved_errors = ledger_frame.unresolved_errors;
        }
        if !ledger_frame.artifact_handles.is_empty() {
            frame.artifact_handles = ledger_frame.artifact_handles;
        }

        // Authoritative goal precedence (M002 §6.2): an active durable Goal
        // outranks the immutable session-origin prompt for the current
        // objective. Origin provenance is retained separately by the
        // continuation assembler; this compatibility frame keeps the
        // resolved objective visible. Lookup failure falls back to origin
        // provenance and never produces a fake goal.
        if let Some(goal_store) = self.services.goal_store.clone() {
            match goal_store.active_for_session(&self.session_id).await {
                Ok(Some(goal)) if goal.status == crate::goal::model::GoalStatus::Active => {
                    if !goal.objective.trim().is_empty() {
                        frame.user_goal = Some(goal.objective.clone());
                    }
                    if frame.current_task.is_none() {
                        frame.current_task = goal
                            .next_action
                            .clone()
                            .filter(|action| !action.trim().is_empty())
                            .or_else(|| {
                                goal.current_phase
                                    .clone()
                                    .filter(|phase| !phase.trim().is_empty())
                            });
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::debug!(error = %error, "goal lookup failed; using origin provenance");
                }
            }
        }

        frame
    }

    pub fn todo_state(&self) -> std::sync::Arc<tokio::sync::Mutex<crate::task_state::TodoState>> {
        self.services.todo_state.clone()
    }

    pub async fn load_persisted_todos(&self) {
        if let Some(pool) = &self.services.todo_pool {
            if !self.session_id.is_empty() {
                let store = crate::session::store::TodoStore::new(pool.clone());
                match store.list(&self.session_id).await {
                    Ok(session_items) => {
                        let mut todo = self.services.todo_state.lock().await;
                        todo.load_from_session(session_items);
                    }
                    Err(e) => {
                        tracing::debug!("No persisted todos for session: {}", e);
                    }
                }
            }
        }
    }

    pub(super) fn apply_agent_config(&self, request: &mut ChatRequest) {
        if let Some(agent) = self.services.agents.get(&self.state.current_agent) {
            if let Some(ref model) = agent.model {
                request.model = model.clone();
            }
            if let Some(temp) = agent.temperature {
                request.temperature = Some(temp);
            }
            if let Some(top_p) = agent.top_p {
                request.top_p = Some(top_p);
            }
            if let Some(budget) = agent.thinking_budget {
                request.thinking_budget = Some(budget);
            }
            if let Some(effort) = agent.reasoning_effort.clone() {
                request.reasoning_effort = Some(effort);
            }
        }
    }

    pub(super) async fn apply_semantic_routing(
        &mut self,
        request: &mut ChatRequest,
    ) -> Result<bool, crate::error::AppError> {
        if !self
            .services
            .semantic_router
            .is_virtual_model(&request.model)
        {
            return Ok(false);
        }

        let decision = self
            .services
            .semantic_router
            .resolve(
                request,
                &self.services.provider_registry,
                self.cancel_rx.as_mut(),
            )
            .await?
            .ok_or_else(|| {
                crate::error::AppError::Agent(crate::error::AgentError::Invalid(
                    "semantic model router returned no decision".to_string(),
                ))
            })?;
        let concrete_model =
            super::semantic_router::validate_semantic_route_against_selected_connection(
                self.services.provider.id(),
                &decision.resolved_model,
            )
            .map_err(|error| {
                crate::error::AppError::Config(crate::error::ConfigError::Invalid(
                    error.to_string(),
                ))
            })?;
        request.model = concrete_model.to_string();
        tracing::info!(
            requested_model = %decision.requested_model,
            resolved_model = %decision.resolved_model,
            route_id = %decision.route_id,
            route_label = %decision.route_label,
            source = decision.source.as_str(),
            attempts = decision.attempts,
            fallback_reason = ?decision.fallback_reason,
            "semantic model route resolved"
        );
        crate::bus::global::GlobalEventBus::publish(AppEvent::ModelChanged {
            model: decision.resolved_model,
            complexity: format!("semantic:{}", decision.route_label),
        });
        Ok(true)
    }

    pub(super) fn apply_model_profile_defaults(
        &self,
        request: &mut ChatRequest,
        profile: &crate::model_profile::types::ResolvedModelProfile,
    ) {
        if request.reasoning_effort.is_none() {
            request.reasoning_effort = profile.default_reasoning_effort.clone();
        }
        if request.thinking_budget.is_none() {
            request.thinking_budget = profile.default_thinking_budget;
        }
    }

    /// Apply the frontend's explicit extended-thinking choice for this turn.
    ///
    /// Runs *after* the agent config and model-profile defaults, so an
    /// explicit user selection always wins over configuration. `Inherit`
    /// leaves whatever the earlier steps decided untouched.
    ///
    /// `Off` clears both fields: the wire encoder treats "neither field set"
    /// as thinking not requested (`wire.rs`), which is the only way to
    /// express an explicit disable — an agent that sets `thinking_budget`
    /// otherwise keeps thinking no matter what the user chose.
    pub(super) fn apply_reasoning_override(&self, request: &mut ChatRequest) {
        let Some(chosen) = self.state.reasoning_override else {
            return;
        };
        Self::apply_reasoning_choice(request, chosen);
    }

    /// Apply an explicit extended-thinking choice to a prepared request.
    ///
    /// Pure so the precedence rules are directly testable: an explicit user
    /// choice must win over agent config and model-profile defaults, and effort
    /// and budget must never both be set (the wire encoder picks its mode from
    /// whichever is present, so leaving both would silently prefer budget).
    fn apply_reasoning_choice(
        request: &mut ChatRequest,
        chosen: crate::protocol::core::ReasoningOverride,
    ) {
        use crate::protocol::core::ReasoningOverride;
        match chosen {
            ReasoningOverride::Effort { level } => {
                request.thinking_budget = None;
                request.reasoning_effort = Some(level.as_str().to_string());
            }
            ReasoningOverride::Budget { tokens } => {
                request.reasoning_effort = None;
                request.thinking_budget = Some(tokens);
            }
            ReasoningOverride::Off => {
                // Both cleared: the wire encoder treats "neither set" as
                // thinking-not-requested, which is the only way to disable.
                request.reasoning_effort = None;
                request.thinking_budget = None;
            }
        }
    }

    pub(super) fn apply_auto_routing(&self, request: &mut ChatRequest) {
        if !self.services.model_router.is_enabled() {
            return;
        }

        let (prompt, tool_name) = self.extract_current_prompt_and_tool(request);
        if prompt.is_empty() {
            return;
        }

        let complexity = self.services.model_router.classify(&prompt, tool_name);
        if let Some(model) = self.services.model_router.route_model(complexity) {
            tracing::info!(
                "Auto-routing task to {} (complexity: {:?}, prompt: {:.50}...)",
                model,
                complexity,
                prompt
            );
            crate::bus::global::GlobalEventBus::publish(AppEvent::ModelChanged {
                model: model.clone(),
                complexity: complexity.as_str().to_string(),
            });
            request.model = model;
        }
    }

    fn infer_tool_from_prompt(prompt: &str) -> &'static str {
        let p = prompt.to_lowercase();
        if p.contains("debug")
            || p.contains("analyze")
            || p.contains("review")
            || p.contains("architect")
            || p.contains("investigate")
        {
            return "debug";
        }
        if p.contains("edit")
            || p.contains("rewrite")
            || p.contains("refactor")
            || p.contains("patch")
            || p.contains("modify")
            || p.contains("update")
            || p.contains("change")
        {
            return "edit";
        }
        if p.contains("write")
            || p.contains("create")
            || p.contains("implement")
            || p.contains("add")
            || p.contains("build")
        {
            return "write";
        }
        if p.contains("search") || p.contains("find") || p.contains("grep") {
            return "search";
        }
        if p.contains("list") || p.contains("show") || p.contains("read") || p.contains("view") {
            return "read";
        }
        "read"
    }

    pub(super) fn latest_user_prompt(request: &ChatRequest) -> String {
        request
            .messages
            .iter()
            .rev()
            .find_map(|msg| match msg {
                Message::User { content } => {
                    let prompt = content
                        .iter()
                        .filter_map(|part| match part {
                            ContentPart::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    (!prompt.trim().is_empty()).then_some(prompt)
                }
                _ => None,
            })
            .unwrap_or_default()
    }

    fn extract_current_prompt_and_tool(&self, request: &ChatRequest) -> (String, &'static str) {
        let prompt = Self::latest_user_prompt(request);
        let tool = Self::infer_tool_from_prompt(&prompt);
        (prompt, tool)
    }

    pub(super) async fn build_tool_definitions(&mut self) -> Vec<crate::provider::ToolDefinition> {
        let model = self
            .services
            .agents
            .get(&self.state.current_agent)
            .and_then(|a| a.model.as_ref());

        let lsp_enabled = self
            .services
            .config
            .experimental
            .as_ref()
            .and_then(|e| e.lsp_tool)
            .unwrap_or(false);

        // M001 (agent-tool-runtime-reconciliation): the registry is the
        // single canonical origin for raw MCP definitions. Adapters were
        // installed before broker construction (turn/worker async pass plus
        // the `AgentLoop::new` sync safety net), so `tool_registry.list()`
        // already contains exactly the policy-visible `mcp__*` set. No
        // second `list_filtered_tools` append path exists; that bypass is
        // retired. The exposure policy construction stays canonical in
        // `mcp_adapter::mcp_exposure_policy_for_turn` (same inputs as
        // registration) so cache identity agrees with admission.
        //
        // M002: the same inputs feed an immutable `ToolDescriptorSnapshot`
        // (generation, visible revision, server identities, permitted set,
        // policy, plugin activation, agent, hidden set). Its fingerprint is
        // the cache identity for the descriptor universe; the resolved
        // surface fingerprint is diagnostics-only and never part of the key.
        let policy = crate::tool::mcp_adapter::mcp_exposure_policy_for_turn(
            &self.services.search_runtime,
            self.services.tool_registry.tool_backends(),
        );
        let expose_raw_search = self.services.search_runtime.config().expose_raw_mcp_tools();
        // Cache identity is based on the registry's visible MCP surface (the
        // registered adapters), not its cardinality and not a second service
        // listing. Sorting makes this stable across the HashMap-backed
        // service and the digest contains no credentials or transport
        // configuration.
        let registry_mcp_tools: Vec<crate::provider::ToolDefinition> = self
            .services
            .tool_registry
            .list()
            .iter()
            .filter(|tool| tool.name().starts_with("mcp__"))
            .filter(|tool| tool.expose_in_definitions())
            .map(|tool| crate::provider::ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters(),
                defer_loading: if tool.defer_loading() {
                    Some(true)
                } else {
                    None
                },
            })
            .collect();
        let mcp_tool_revision = mcp_tool_surface_revision(&registry_mcp_tools);
        // M002 snapshot inputs (no locks held across awaits beyond the
        // bounded `try_read` inside the helper).
        let (mcp_generation, mcp_servers) =
            super::tool_descriptor_snapshot::snapshot_mcp_identities(
                self.services.mcp_service.as_ref(),
            );
        let plugin_activation_revision =
            super::tool_descriptor_snapshot::plugin_activation_revision(
                self.plugin_service.as_ref(),
            );
        let current_agent_name = self.state.current_agent.clone();
        let permitted_pre_surface: std::collections::BTreeSet<String> = self
            .services
            .tool_registry
            .list()
            .iter()
            .filter(|tool| tool.expose_in_definitions())
            .map(|tool| tool.name().to_string())
            .collect();
        let permission_version_for_snapshot = self.permission_version().to_string();
        let snapshot = super::tool_descriptor_snapshot::ToolDescriptorSnapshot::builder(
            mcp_generation,
            mcp_tool_revision.clone(),
            mcp_servers,
            permitted_pre_surface,
            permission_version_for_snapshot,
            plugin_activation_revision.clone(),
            current_agent_name.clone(),
            policy.hidden_servers.clone(),
        );
        let snapshot_fingerprint = snapshot.fingerprint();
        let contextual_immediate = self.contextual_immediate_tools().await;
        let contextual_revision = contextual_immediate
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(",");

        let permission_version = self.permission_version();

        // M004: qualify the disclosure wire mode before cache lookup so
        // every turn records its mode — cache hits reuse cached defs but
        // re-report mode + reason (no sticky state; model switch and
        // failover always re-evaluate).
        let provider_id = self.services.provider.id();
        let model_name: &str = model.map(String::as_str).unwrap_or_default();
        let disclosure = crate::provider::qualify_tool_disclosure(provider_id, model_name);

        if let Some((
            ref cache_model,
            cache_plan,
            cache_lsp,
            ref cache_mcp_count,
            cache_perm_ver,
            ref cache_contextual_revision,
            cache_expose_raw,
            ref cache_tool_deferral,
            ref cached_defs,
            ref cached_deferred,
            ref cache_agent,
            ref cache_plugin_activation,
            ref cache_snapshot_fp,
        )) = self.services.tool_def_cache
        {
            if cache_model.as_ref().map(|s| s.as_str()) == model.map(|s| s.as_str())
                && cache_plan == self.state.plan_mode
                && cache_lsp == lsp_enabled
                && cache_mcp_count == &mcp_tool_revision
                && cache_perm_ver == permission_version
                && cache_contextual_revision == &contextual_revision
                && cache_expose_raw == expose_raw_search
                && cache_tool_deferral == &self.services.config.tool_deferral
                && cache_agent == &current_agent_name
                && cache_plugin_activation == &plugin_activation_revision
                && cache_snapshot_fp == &snapshot_fingerprint
                && self.services.tool_advisor_mode != crate::tool_advisor::AdvisorMode::Promote
            {
                let mut definitions = cached_defs.clone();
                self.services.deferred_tool_definitions = cached_deferred.clone();
                // M004: per-turn mode truth on the cache-hit path (counts
                // from the reused defs; byte accounting lives on the
                // uncached build below).
                tracing::debug!(
                    provider = %provider_id,
                    model = %model_name,
                    disclosure_mode = %disclosure.mode.as_str(),
                    disclosure_reason = %disclosure.reason,
                    cache_hit = true,
                    immediate_tools = definitions.len(),
                    deferred_tools = self.services.deferred_tool_definitions.len(),
                    "selected tool disclosure wire mode"
                );

                // M002 §5D: confine hook output to the cached authorized
                // surface. Additions, renames, invalid schemas and
                // permission-expanding parameter changes are rejected;
                // bounded description rewordings survive with canonical
                // parameters and deferral preserved.
                if let Some(ref plugin_svc) = self.plugin_service {
                    let input = serde_json::json!({
                        "tools": definitions,
                        "model": model,
                    });
                    let hook_result = plugin_svc.dispatch_tool_definition(input).await;
                    if hook_result.output.get("tools").is_some() {
                        let canonical: std::collections::BTreeMap<
                            String,
                            crate::provider::ToolDefinition,
                        > = definitions
                            .iter()
                            .cloned()
                            .map(|d| (d.name.clone(), d))
                            .collect();
                        let confined = super::tool_descriptor_snapshot::confine_hook_definitions(
                            &hook_result.output,
                            &canonical,
                        );
                        if !confined.is_empty() || hook_result.output.get("tools").is_some() {
                            // An empty confined set with a present hook array
                            // means every entry was rejected: advertise
                            // nothing from the hook rather than falling back
                            // to the unconfined cache.
                            let mut confined_plus_deferred = confined;
                            confined_plus_deferred
                                .extend(self.services.deferred_tool_definitions.iter().cloned());
                            // Re-validate deferred against the same canonical
                            // set so the hook cannot smuggle a deferred-only
                            // addition.
                            let canonical_deferred: std::collections::BTreeSet<String> =
                                cached_deferred.iter().map(|d| d.name.clone()).collect();
                            confined_plus_deferred.retain(|d| {
                                canonical.contains_key(&d.name)
                                    || canonical_deferred.contains(&d.name)
                            });
                            return confined_plus_deferred;
                        }
                    }
                }

                definitions.extend(self.services.deferred_tool_definitions.iter().cloned());
                return definitions;
            }
        }

        let tools = self.services.tool_registry.list();
        let flags = compute_model_flags(model, self.services.search_runtime.backend());
        let native_categories = tools
            .iter()
            .map(|tool| (tool.name().to_string(), tool.category()))
            .collect::<std::collections::BTreeMap<_, _>>();
        // Hide tools that the registry marks as non-exposed
        // (e.g. `DisabledTool` stubs) so the model never sees a
        // tool whose every call is a guaranteed failure. This is
        // the model-facing half of the same predicate the
        // registry uses in `definitions()`.
        let tools: Vec<&dyn crate::tool::Tool> = tools
            .into_iter()
            .filter(|t| t.expose_in_definitions())
            .collect();
        let filtered =
            filter_tools_for_model(model, &tools, self.state.plan_mode, lsp_enabled, &flags);
        let all_definitions: Vec<_> = filtered
            .iter()
            .map(|t| crate::provider::ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters(),
                defer_loading: if t.defer_loading() { Some(true) } else { None },
            })
            .collect();

        let all_definitions =
            self.apply_tool_exposure_filter(&contextual_immediate, all_definitions);

        // M001: no second MCP append. `all_definitions` already contains the
        // registry's `mcp__*` adapters (the single canonical origin), so
        // extending with a separately listed raw set would advertise every
        // raw tool twice and reintroduce the pre-broker bypass.
        let all_definitions = all_definitions;

        // Resolve the complete model-facing surface once.  Prompt assembly,
        // provider schemas, palette reduction, and diagnostics all consume
        // this deterministic snapshot; the broker remains the execution and
        // permission authority.
        let has_functional_spawner = self
            .services
            .tool_registry
            .list()
            .iter()
            .find(|tool| tool.name() == "task")
            .is_some_and(|tool| tool.has_functional_backend());
        let surface = match crate::agent::tool_surface::ResolvedToolSurface::resolve_with_categories(
            all_definitions,
            &self
                .services
                .agents
                .get(&self.state.current_agent)
                .map(|agent| {
                    agent
                        .permissions
                        .iter()
                        .filter(|(_, level)| level.eq_ignore_ascii_case("deny"))
                        .map(|(name, _)| name.clone())
                        .collect()
                })
                .unwrap_or_default(),
            &std::collections::BTreeSet::new(),
            self.state.plan_mode,
            has_functional_spawner,
            None,
            &std::collections::BTreeMap::new(),
            &native_categories,
        ) {
            Ok(surface) => surface,
            Err(error) => {
                tracing::error!(error = ?error, "invalid resolved tool surface");
                return Vec::new();
            }
        };
        tracing::debug!(
            surface_fingerprint = %surface.fingerprint,
            selected_tool_count = surface.tools.len(),
            omitted_tool_count = surface.omissions.len(),
            capabilities = ?surface.capabilities.capabilities(),
            "resolved agent tool surface"
        );
        let all_definitions = surface.definitions();

        // M004 observe-only causal frontier: inspect the immutable resolved
        // surface plus bounded host-owned state and record diagnostics. This
        // takes `&surface` and returns no definitions: provider definitions
        // and `defer_loading` bits below are byte-for-byte identical with
        // observe disabled. Cached preparations return before any surface
        // exists, so they reuse the last recorded outcome for the same
        // cache identity rather than evaluating again.
        if self.causal_frontier_mode()
            == crate::tool_advisor::causal_observe::CausalFrontierMode::Observe
        {
            let inputs = self.causal_observe_inputs().await;
            let outcome = crate::tool_advisor::causal_observe::evaluate_observe(&surface, &inputs);
            tracing::debug!(
                scope = "causal_frontier_observe",
                surface_fingerprint = %outcome.surface_fingerprint,
                state_fingerprint = %outcome.state_fingerprint,
                contract_catalog_fingerprint = %outcome.contract_catalog_fingerprint,
                admissible = outcome.admissible_count,
                inadmissible = outcome.inadmissible_count,
                uncontracted = outcome.uncontracted_count,
                required = outcome.required_count,
                evaluation_millis = outcome.evaluation_millis,
                fallback_reason = ?outcome.fallback_reason,
                deferred_promotion = ?outcome.deferred_promotion(),
                "evaluated causal frontier in observe mode"
            );
            crate::tool_advisor::causal_observe::record_observe_outcome(&self.session_id, outcome);
        }

        let candidate_deferred: Vec<_> = all_definitions
            .iter()
            .filter(|definition| definition.defer_loading == Some(true))
            .cloned()
            .collect();
        let advisor_context = self.current_advisor_context_v2().await;
        let decision_timeout = std::time::Duration::from_millis(
            self.services
                .config
                .tool_advisor
                .as_ref()
                .and_then(|advisor| advisor.timeout_ms)
                .unwrap_or(25)
                .clamp(1, 1_000),
        );
        let mut promoted_names = project_preturn_decision(
            &surface,
            &candidate_deferred,
            &advisor_context,
            PreturnDecisionConfig {
                engine: &self.services.decision_engine,
                timeout: decision_timeout,
                mode: self.services.tool_advisor_mode,
                threshold: self.services.tool_advisor_threshold,
                max_promotions: self.services.tool_advisor_max_promotions,
                schema_budget: self.services.tool_advisor_schema_budget,
                max_candidates: self
                    .services
                    .config
                    .tool_advisor
                    .as_ref()
                    .and_then(|advisor| advisor.max_candidates)
                    .unwrap_or(16),
            },
        )
        .await;

        // M005 bounded active causal disclosure (opt-in): union at most two
        // causally admissible deferred tools, within a 16 KiB promoted
        // schema budget, into the advisor promotion set. Required, core, and
        // contextual immediacy are untouched: causal names only exempt
        // definitions from deferral below, exactly like advisor promotions.
        // Uncontracted tools are never promoted and stay discoverable via
        // `tool_search`. When the outcome carries no promotion, definitions
        // are byte-identical to active-disabled preparation.
        if self.causal_frontier_mode()
            == crate::tool_advisor::causal_observe::CausalFrontierMode::Active
        {
            let mut deferred_schema_bytes = std::collections::BTreeMap::new();
            for definition in &candidate_deferred {
                let canonical_name = surface
                    .wire_to_canonical
                    .get(&definition.name)
                    .map(String::as_str)
                    .unwrap_or(&definition.name);
                let bytes = definition.parameters.to_string().len();
                deferred_schema_bytes
                    .entry(canonical_name.to_string())
                    .and_modify(|entry| {
                        if bytes > *entry {
                            *entry = bytes;
                        }
                    })
                    .or_insert(bytes);
            }
            let inputs = self.causal_observe_inputs().await;
            let outcome = crate::tool_advisor::causal_active::evaluate_active(
                &surface,
                &inputs,
                &deferred_schema_bytes,
            );
            tracing::debug!(
                scope = "causal_frontier_active",
                surface_fingerprint = %outcome.observe.surface_fingerprint,
                state_fingerprint = %outcome.observe.state_fingerprint,
                contract_catalog_fingerprint = %outcome.observe.contract_catalog_fingerprint,
                promotion_applied = outcome.promotion_applied,
                promoted = ?outcome.promoted,
                promoted_schema_bytes = outcome.promoted_schema_bytes,
                no_change_reason = ?outcome.no_change_reason,
                active_evaluation_millis = outcome.active_evaluation_millis,
                "evaluated bounded causal active disclosure"
            );
            if outcome.promotion_applied {
                promoted_names.extend(outcome.promoted.iter().cloned());
            }
            crate::tool_advisor::causal_observe::record_observe_outcome(
                &self.session_id,
                outcome.observe,
            );
        }

        // Partition tools into immediate vs deferred (host-side palette
        // reduction for discovery/counts). The partition is palette
        // truth; only a qualified native `disclosure.mode` changes wire
        // emission, and no combination is qualified without on-wire proof
        // — so FullCompatibility sends the complete authorized array
        // exactly as before.
        let caps = crate::provider::ProviderCapabilities::for_provider(provider_id);
        let deferral_enabled = self
            .services
            .config
            .tool_deferral
            .as_ref()
            .and_then(|td| td.defer_loading)
            .unwrap_or(true);

        let always_loaded: Vec<String> = self
            .services
            .config
            .tool_deferral
            .as_ref()
            .and_then(|td| td.always_loaded.clone())
            .unwrap_or_default();

        let max_initial = self
            .services
            .config
            .tool_deferral
            .as_ref()
            .and_then(|td| td.max_initial_tools);

        // M004: the host partition is preserved whenever the provider
        // family is deferral-capable (palette/discovery/cache behavior
        // unchanged); the wire mode is qualified independently. A generic
        // ID match never enables native emission — see
        // `qualify_tool_disclosure`.
        let (definitions, deferred) = if deferral_enabled && caps.supports_defer_loading {
            let mut immediate = Vec::new();
            let mut deferred_tools = Vec::new();

            // Specialist roles receive their role-appropriate deferred tools
            // immediately (M002 profile-specific disclosure). This never
            // widens authority: deny/plan/disable/backend/ceiling filtering
            // already ran in the resolved surface above.
            let agent_name = self.state.current_agent.clone();
            for def in all_definitions {
                let is_always_loaded = always_loaded.iter().any(|n| n == &def.name)
                    || crate::tool::disclosure::immediate_for_agent(&def.name, &agent_name);
                let canonical_name = surface
                    .wire_to_canonical
                    .get(&def.name)
                    .map(String::as_str)
                    .unwrap_or(&def.name);
                let is_advisor_promoted = promoted_names.contains(canonical_name);
                let should_defer =
                    !is_always_loaded && !is_advisor_promoted && def.defer_loading == Some(true);

                if should_defer {
                    deferred_tools.push(def);
                } else {
                    immediate.push(def);
                }
            }

            if !promoted_names.is_empty() {
                immediate.sort_by_key(|definition| {
                    let canonical_name = surface
                        .wire_to_canonical
                        .get(&definition.name)
                        .map(String::as_str)
                        .unwrap_or(&definition.name);
                    (
                        !promoted_names.contains(canonical_name),
                        definition.name.clone(),
                    )
                });
            }

            // Apply max_initial_tools cap if configured
            let immediate = if let Some(max) = max_initial {
                if immediate.len() > max {
                    // Move excess tools to deferred
                    let (kept, excess) = immediate.split_at(max);
                    let mut deferred_tools = deferred_tools;
                    deferred_tools.extend(excess.iter().cloned());
                    self.services.deferred_tool_definitions = deferred_tools;
                    kept.to_vec()
                } else {
                    self.services.deferred_tool_definitions = deferred_tools;
                    immediate
                }
            } else {
                self.services.deferred_tool_definitions = deferred_tools;
                immediate
            };

            (immediate, self.services.deferred_tool_definitions.clone())
        } else {
            // Provider doesn't support defer_loading or deferral is disabled: all tools immediate.
            // Providers like deepseek, qwen, cerebras, groq, etc. go through OpenAiCompatibleProvider
            // with provider_ids not matching "openai" or "anthropic", so they get default capabilities
            // (supports_defer_loading: false). All tools are sent in the single `tools` array.
            self.services.deferred_tool_definitions.clear();
            (all_definitions, Vec::new())
        };

        // Update tool_search with available tool names so search results
        // only include tools the LLM can actually call
        let mut available_names: Vec<String> = definitions.iter().map(|t| t.name.clone()).collect();
        // Also include deferred tool names so they can be found via search
        available_names.extend(deferred.iter().map(|t| t.name.clone()));
        self.services
            .tool_registry
            .set_search_tool_available_tools(available_names);

        // M002: cache identity includes the acting agent, the deterministic
        // plugin activation revision, and the full descriptor snapshot
        // fingerprint (MCP generation, visible revision, origins, permitted
        // set, policy, hidden set). The resolved surface fingerprint is
        // diagnostics-only and never part of the key.
        self.services.tool_def_cache = Some((
            model.map(|s| s.to_string()),
            self.state.plan_mode,
            lsp_enabled,
            mcp_tool_revision,
            permission_version,
            contextual_revision,
            expose_raw_search,
            self.services.config.tool_deferral.clone(),
            definitions.clone(),
            deferred.clone(),
            current_agent_name.clone(),
            plugin_activation_revision.clone(),
            snapshot_fingerprint.clone(),
        ));

        // M002 observability: bounded generation/omission telemetry (no
        // credentials, endpoints, or full schemas). The surface fingerprint
        // plus counts let operators distinguish "no tools because hidden"
        // from "no tools because disconnected".
        {
            let snapshotted = snapshot.with_surface(surface.fingerprint.clone());
            tracing::debug!(
                mcp_generation = %snapshotted.mcp_generation,
                mcp_visible_revision = %snapshotted.mcp_visible_revision,
                permitted = snapshotted.permitted_names.len(),
                advertised = surface.tools.len(),
                omitted = surface.omissions.len(),
                policy_revision = %snapshotted.policy_revision,
                plugin_activation = %snapshotted.plugin_activation_revision,
                agent = %snapshotted.agent_name,
                surface_fingerprint = %surface.fingerprint,
                "resolved tool descriptor snapshot"
            );
        }
        // M004 observability: per-turn disclosure mode truth. Only a
        // qualified native mode changes wire emission; everything else
        // logs full compatibility (never native deferral) with the
        // fallback reason plus exact definition bytes for tradeoff
        // analysis. No credentials, endpoints, or schemas.
        {
            let immediate_bytes: usize = definitions
                .iter()
                .map(|def| {
                    serde_json::to_string(&def.parameters)
                        .map(|s| s.len())
                        .unwrap_or(0)
                })
                .sum();
            let deferred_bytes: usize = deferred
                .iter()
                .map(|def| {
                    serde_json::to_string(&def.parameters)
                        .map(|s| s.len())
                        .unwrap_or(0)
                })
                .sum();
            tracing::debug!(
                provider = %provider_id,
                model = %model_name,
                disclosure_mode = %disclosure.mode.as_str(),
                disclosure_reason = %disclosure.reason,
                cache_hit = false,
                immediate_tools = definitions.len(),
                deferred_tools = deferred.len(),
                immediate_schema_bytes = immediate_bytes,
                deferred_schema_bytes = deferred_bytes,
                "selected tool disclosure wire mode"
            );
        }

        let mut result = definitions;
        result.extend(self.services.deferred_tool_definitions.iter().cloned());

        // M002 §5D: confine hook output to the resolved authorized surface.
        // The canonical map is the pre-hook result (immediate + deferred);
        // surviving entries keep canonical parameters/deferral with only a
        // bounded description rewording. Additions, renames, invalid schemas
        // and widening parameter edits are rejected.
        if let Some(ref plugin_svc) = self.plugin_service {
            let input = serde_json::json!({
                "tools": result,
                "model": model,
            });
            let hook_result = plugin_svc.dispatch_tool_definition(input).await;
            if hook_result.output.get("tools").is_some() {
                let canonical: std::collections::BTreeMap<String, crate::provider::ToolDefinition> =
                    result
                        .iter()
                        .cloned()
                        .map(|d| (d.name.clone(), d))
                        .collect();
                return super::tool_descriptor_snapshot::confine_hook_definitions(
                    &hook_result.output,
                    &canonical,
                );
            }
        }

        result
    }
}

/// Test-only: expose `build_tool_definitions` so integration tests can
/// assert the actual tool set the agent sends to the model.
///
/// **Not** intended for production use.
#[doc(hidden)]
impl AgentLoop {
    #[doc(hidden)]
    pub async fn test_build_tool_definitions(&mut self) -> Vec<crate::provider::ToolDefinition> {
        self.build_tool_definitions().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{Config, ResearchAutoTriggerConfig, ResearchConfig};
    use std::sync::Arc;

    fn config_with_trigger(enabled: bool, min_confidence: f32) -> Config {
        Config {
            research: Some(ResearchConfig {
                search_provider: None,
                auto_trigger: Some(ResearchAutoTriggerConfig {
                    enabled,
                    min_confidence,
                }),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn research_trigger_fires_on_comparison_query() {
        let trigger = crate::research::triggers::TriggerConfig {
            enabled: true,
            min_confidence: 0.5,
            ..Default::default()
        };
        let analysis = crate::research::triggers::analyze_trigger(
            "Compare React and Vue for our frontend",
            &[],
            &[],
            &trigger,
        );
        assert!(analysis.should_invoke);
        assert_eq!(
            analysis.suggested_mode,
            crate::research::types::ResearchMode::LibraryEvaluation
        );
    }

    #[test]
    fn research_trigger_config_resolves_enabled_flag() {
        let cfg = config_with_trigger(false, 0.5);
        let resolved = cfg
            .research
            .as_ref()
            .and_then(|r| r.auto_trigger.as_ref())
            .cloned()
            .unwrap_or_default();
        assert!(!resolved.enabled);
        assert!((resolved.min_confidence - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn current_turn_prompt_uses_latest_user_message() {
        let request = ChatRequest {
            messages: vec![
                Message::User {
                    content: vec![ContentPart::Text {
                        text: "Compare the old libraries".to_string().into(),
                    }],
                },
                Message::Assistant {
                    content: vec![ContentPart::Text {
                        text: "Historical answer".to_string().into(),
                    }],
                    tool_calls: Vec::new(),
                },
                Message::User {
                    content: vec![ContentPart::Text {
                        text: "Read src/main.rs".to_string().into(),
                    }],
                },
            ],
            model: "test/model".to_string(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: Default::default(),
        };

        assert_eq!(AgentLoop::latest_user_prompt(&request), "Read src/main.rs");
    }

    #[test]
    fn palette_projection_retains_deferred_discovery() {
        let definitions = vec![
            crate::provider::ToolDefinition {
                name: "read".into(),
                description: "read files".into(),
                parameters: serde_json::json!({"type": "object"}),
                defer_loading: None,
            },
            crate::provider::ToolDefinition {
                name: "security".into(),
                description: "security analysis".into(),
                parameters: serde_json::json!({"type": "object"}),
                defer_loading: None,
            },
        ];
        let projected = project_initial_tool_palette(
            crate::agent::policy::ToolExposureMode::Curated,
            &std::collections::BTreeSet::new(),
            definitions,
        );
        assert_eq!(projected.len(), 2);
        assert_eq!(projected[0].defer_loading, None);
        assert_eq!(projected[1].defer_loading, Some(true));
    }

    #[test]
    fn contextual_tools_are_host_state_driven_and_bounded() {
        let inactive = contextual_tool_names(false, false, false);
        assert!(inactive.is_empty());

        let active = contextual_tool_names(true, true, true);
        assert_eq!(active.len(), 6);
        assert!(active.contains("context_read"));
        assert!(active.contains("goal_update_progress"));
        assert!(active.contains("work_plan_update_item"));
        assert!(!active.contains("work_order"));
    }

    struct RecordingAdvisor {
        seen: std::sync::Mutex<Vec<String>>,
        calls: std::sync::atomic::AtomicUsize,
        promote_first: Vec<String>,
        abstain: f64,
        fail: bool,
    }

    impl RecordingAdvisor {
        fn new(promote_first: Vec<String>) -> Self {
            Self {
                seen: std::sync::Mutex::new(Vec::new()),
                calls: std::sync::atomic::AtomicUsize::new(0),
                promote_first,
                abstain: 0.0,
                fail: false,
            }
        }

        fn seen_names(&self) -> Vec<String> {
            self.seen.lock().expect("seen names").clone()
        }

        fn call_count(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    impl crate::tool_advisor::ToolAdvisor for RecordingAdvisor {
        fn score(
            &self,
            input: &crate::tool_advisor::ToolAdvisorInput,
        ) -> anyhow::Result<crate::tool_advisor::ToolAdvisorPrediction> {
            self.calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            *self.seen.lock().expect("record input") = input
                .candidates
                .iter()
                .map(|candidate| candidate.name.clone())
                .collect();
            if self.fail {
                return Err(anyhow::anyhow!("injected advisor failure"));
            }
            let mut ranked = Vec::new();
            for name in &self.promote_first {
                if input
                    .candidates
                    .iter()
                    .any(|candidate| &candidate.name == name)
                {
                    ranked.push(crate::tool_advisor::RankedCandidate {
                        name: name.clone(),
                        score: 1.0 - ranked.len() as f64 * 0.1,
                    });
                }
            }
            for candidate in &input.candidates {
                if !ranked.iter().any(|item| item.name == candidate.name) {
                    ranked.push(crate::tool_advisor::RankedCandidate {
                        name: candidate.name.clone(),
                        score: 0.05,
                    });
                }
            }
            Ok(crate::tool_advisor::ToolAdvisorPrediction {
                schema_version: crate::tool_advisor::PREDICTION_SCHEMA_VERSION,
                case_id: input.case_id.clone(),
                ranked,
                abstain_probability: Some(self.abstain),
                mode: "test".into(),
            })
        }
    }

    fn tool_definition(
        name: &str,
        description: &str,
        deferred: bool,
    ) -> crate::provider::ToolDefinition {
        crate::provider::ToolDefinition {
            name: name.into(),
            description: description.into(),
            parameters: serde_json::json!({"type": "object"}),
            defer_loading: deferred.then_some(true),
        }
    }

    fn deferred_from_surface(
        surface: &crate::agent::tool_surface::ResolvedToolSurface,
    ) -> Vec<crate::provider::ToolDefinition> {
        surface
            .definitions()
            .into_iter()
            .filter(|definition| definition.defer_loading == Some(true))
            .collect()
    }

    fn promote_config<'a>(
        advisor: &'a dyn crate::tool_advisor::ToolAdvisor,
        max_candidates: usize,
    ) -> PreturnDisclosureConfig<'a> {
        PreturnDisclosureConfig {
            advisor,
            mode: crate::tool_advisor::AdvisorMode::Promote,
            threshold: 0.5,
            max_promotions: 2,
            schema_budget: 16 * 1024,
            max_candidates,
        }
    }

    struct FixedDecisionEngine {
        target: String,
        calls: std::sync::atomic::AtomicUsize,
        rank: bool,
    }

    #[async_trait::async_trait]
    impl codegg_core::decision::DecisionEngine for FixedDecisionEngine {
        fn state(&self) -> codegg_core::decision::BackendState {
            codegg_core::decision::BackendState::Ready
        }

        fn capabilities(&self) -> codegg_core::decision::BackendCapabilities {
            codegg_core::decision::BackendCapabilities {
                binary: false,
                choice: false,
                score: false,
                rank: self.rank,
                max_options: 0,
                max_candidates: 16,
            }
        }

        async fn decide(
            &self,
            request: codegg_core::decision::DecisionRequest,
            _deadline: std::time::Instant,
        ) -> Result<codegg_core::decision::DecisionResponse, codegg_core::decision::DecisionError>
        {
            use codegg_core::decision::{
                DecisionAnswer, DecisionCandidate, DecisionProvenance, DecisionResponse,
                DecisionSpec, DecisionStatus, RankedDecisionCandidate,
            };
            self.calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let DecisionSpec::Rank { candidates, .. } = request.spec else {
                panic!("expected Rank request")
            };
            let candidates = candidates
                .into_iter()
                .map(|candidate: DecisionCandidate| RankedDecisionCandidate {
                    relevance: if candidate.id == self.target {
                        0.95
                    } else {
                        0.05
                    },
                    ranking_score: if candidate.id == self.target {
                        2.0
                    } else {
                        0.1
                    },
                    candidate_id: candidate.id,
                })
                .collect();
            Ok(DecisionResponse {
                request_id: request.request_id,
                schema_version: request.schema_version,
                status: DecisionStatus::Answered,
                answer: Some(DecisionAnswer::Rank { candidates }),
                provenance: DecisionProvenance {
                    backend: "request-preparation-test".into(),
                    model: None,
                    runtime_version: Some("test".into()),
                    latency_micros: 0,
                },
            })
        }
    }

    #[tokio::test]
    async fn request_preparation_uses_decision_rank_and_preserves_authority() {
        let surface = crate::agent::tool_surface::ResolvedToolSurface::resolve(
            vec![
                tool_definition("lsp_definition", "Find a symbol definition", true),
                tool_definition("secret_admin", "Delete every workspace", true),
            ],
            &std::collections::BTreeSet::new(),
            &std::collections::BTreeSet::from(["secret_admin".into()]),
            false,
            true,
            None,
        )
        .expect("resolved surface");
        let deferred = deferred_from_surface(&surface);
        let engine = Arc::new(FixedDecisionEngine {
            target: "lsp_definition".into(),
            calls: std::sync::atomic::AtomicUsize::new(0),
            rank: true,
        });
        let engine_trait: Arc<dyn codegg_core::decision::DecisionEngine> = engine.clone();
        let promoted = project_preturn_decision(
            &surface,
            &deferred,
            "find the definition of a symbol",
            PreturnDecisionConfig {
                engine: &engine_trait,
                timeout: std::time::Duration::from_millis(25),
                mode: crate::tool_advisor::AdvisorMode::Promote,
                threshold: 0.5,
                max_promotions: 2,
                schema_budget: 16 * 1024,
                max_candidates: 16,
            },
        )
        .await;
        assert_eq!(
            promoted,
            std::collections::BTreeSet::from(["lsp_definition".into()])
        );
        assert_eq!(engine.calls.load(std::sync::atomic::Ordering::Relaxed), 1);

        let unsupported = Arc::new(FixedDecisionEngine {
            target: "lsp_definition".into(),
            calls: std::sync::atomic::AtomicUsize::new(0),
            rank: false,
        });
        let unsupported_trait: Arc<dyn codegg_core::decision::DecisionEngine> = unsupported.clone();
        let none = project_preturn_decision(
            &surface,
            &deferred,
            "find the definition of a symbol",
            PreturnDecisionConfig {
                engine: &unsupported_trait,
                timeout: std::time::Duration::from_millis(25),
                mode: crate::tool_advisor::AdvisorMode::Promote,
                threshold: 0.5,
                max_promotions: 2,
                schema_budget: 16 * 1024,
                max_candidates: 16,
            },
        )
        .await;
        assert!(none.is_empty());
        assert_eq!(
            unsupported.calls.load(std::sync::atomic::Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn late_position_deferred_tool_reaches_the_advisor() {
        // Nine immediate tools sort before the deferred target; the old
        // first-N truncation would have hidden it from the advisor.
        let mut definitions = Vec::new();
        for index in 0..9 {
            definitions.push(tool_definition(
                &format!("core_{index:02}"),
                "Unrelated scaffold maintenance",
                false,
            ));
        }
        definitions.push(tool_definition(
            "zzz_lsp_definition",
            "Jump to a symbol definition using language server semantics",
            true,
        ));
        let surface = crate::agent::tool_surface::ResolvedToolSurface::resolve(
            definitions,
            &std::collections::BTreeSet::new(),
            &std::collections::BTreeSet::new(),
            false,
            true,
            None,
        )
        .expect("resolved surface");
        let deferred = deferred_from_surface(&surface);
        let advisor = RecordingAdvisor::new(vec!["zzz_lsp_definition".into()]);
        let promoted = project_preturn_promotions(
            &surface,
            &deferred,
            "find the symbol definition",
            promote_config(&advisor, 4),
        );
        assert_eq!(
            promoted,
            std::collections::BTreeSet::from(["zzz_lsp_definition".into()])
        );
        assert_eq!(advisor.seen_names(), vec!["zzz_lsp_definition".to_string()]);
    }

    #[test]
    fn immediate_tools_do_not_consume_neural_candidate_slots() {
        let mut definitions = Vec::new();
        for index in 0..8 {
            definitions.push(tool_definition(
                &format!("core_{index:02}"),
                "Unrelated scaffold maintenance",
                false,
            ));
        }
        definitions.push(tool_definition(
            "deferred_a",
            "Unrelated scaffold maintenance",
            true,
        ));
        definitions.push(tool_definition(
            "deferred_b",
            "Unrelated scaffold maintenance",
            true,
        ));
        definitions.push(tool_definition(
            "zzz_relevant",
            "Jump to a symbol definition using language server semantics",
            true,
        ));
        let surface = crate::agent::tool_surface::ResolvedToolSurface::resolve(
            definitions,
            &std::collections::BTreeSet::new(),
            &std::collections::BTreeSet::new(),
            false,
            true,
            None,
        )
        .expect("resolved surface");
        let deferred = deferred_from_surface(&surface);
        let advisor = RecordingAdvisor::new(vec!["zzz_relevant".into()]);
        let promoted = project_preturn_promotions(
            &surface,
            &deferred,
            "find the symbol definition",
            promote_config(&advisor, 2),
        );
        assert_eq!(
            promoted,
            std::collections::BTreeSet::from(["zzz_relevant".into()])
        );
        let seen = advisor.seen_names();
        assert!(seen.contains(&"zzz_relevant".to_string()));
        assert!(seen.iter().all(|name| !name.starts_with("core_")));
        assert!(seen.len() <= 2);
    }

    #[test]
    fn large_deferred_catalog_preselects_the_relevant_late_candidate() {
        let mut definitions = vec![
            tool_definition("read", "Read files", false),
            tool_definition("grep", "Search literal text", false),
        ];
        for index in 0..10 {
            definitions.push(tool_definition(
                &format!("deferred_{index:02}"),
                "Unrelated scaffold maintenance",
                true,
            ));
        }
        definitions.push(tool_definition(
            "zzz_target",
            "Jump to a symbol definition using language server semantics",
            true,
        ));
        let surface = crate::agent::tool_surface::ResolvedToolSurface::resolve(
            definitions,
            &std::collections::BTreeSet::new(),
            &std::collections::BTreeSet::new(),
            false,
            true,
            None,
        )
        .expect("resolved surface");
        let deferred = deferred_from_surface(&surface);
        let advisor = RecordingAdvisor::new(vec!["zzz_target".into()]);
        let promoted = project_preturn_promotions(
            &surface,
            &deferred,
            "find the symbol definition",
            promote_config(&advisor, 3),
        );
        assert_eq!(
            promoted,
            std::collections::BTreeSet::from(["zzz_target".into()])
        );
        let seen = advisor.seen_names();
        assert!(seen.contains(&"zzz_target".to_string()));
        assert!(seen.len() <= 3);
    }

    #[test]
    fn preselection_tie_ordering_is_stable() {
        let definitions = vec![
            tool_definition("aaa_dup", "Jump to a symbol definition", true),
            tool_definition("zzz_dup", "Jump to a symbol definition", true),
        ];
        let surface = crate::agent::tool_surface::ResolvedToolSurface::resolve(
            definitions,
            &std::collections::BTreeSet::new(),
            &std::collections::BTreeSet::new(),
            false,
            true,
            None,
        )
        .expect("resolved surface");
        let deferred = deferred_from_surface(&surface);
        let first_advisor = RecordingAdvisor::new(vec!["aaa_dup".into(), "zzz_dup".into()]);
        project_preturn_promotions(
            &surface,
            &deferred,
            "find the symbol definition",
            promote_config(&first_advisor, 1),
        );
        let second_advisor = RecordingAdvisor::new(vec!["aaa_dup".into(), "zzz_dup".into()]);
        project_preturn_promotions(
            &surface,
            &deferred,
            "find the symbol definition",
            promote_config(&second_advisor, 1),
        );
        assert_eq!(first_advisor.seen_names(), second_advisor.seen_names());
        assert_eq!(first_advisor.seen_names().len(), 1);
    }

    #[test]
    fn denied_and_disabled_tools_never_enter_the_eligible_set() {
        for (denied, disabled) in [
            (
                std::collections::BTreeSet::from(["lsp_definition".to_string()]),
                std::collections::BTreeSet::new(),
            ),
            (
                std::collections::BTreeSet::new(),
                std::collections::BTreeSet::from(["lsp_definition".to_string()]),
            ),
        ] {
            let surface = crate::agent::tool_surface::ResolvedToolSurface::resolve(
                vec![
                    tool_definition("read", "Read files", false),
                    tool_definition("lsp_definition", "Jump to a symbol definition", true),
                ],
                &denied,
                &disabled,
                false,
                true,
                None,
            )
            .expect("resolved surface");
            assert!(surface
                .omissions
                .iter()
                .any(|omission| omission.canonical_name == "lsp_definition"));
            let deferred = deferred_from_surface(&surface);
            assert!(deferred
                .iter()
                .all(|definition| definition.name != "lsp_definition"));
            // Even an adversarial advisor ranking the denied tool first
            // cannot promote it: it never reaches the scorer.
            let advisor = RecordingAdvisor::new(vec!["lsp_definition".into()]);
            let promoted = project_preturn_promotions(
                &surface,
                &deferred,
                "find the symbol definition",
                promote_config(&advisor, 16),
            );
            assert!(promoted.is_empty());
            assert!(
                !advisor.seen_names().contains(&"lsp_definition".to_string()),
                "denied/disabled tool reached the advisor input"
            );
        }
    }

    #[test]
    fn required_tools_are_excluded_from_learned_promotion_candidates() {
        // `read` is required/never-reduce by surface construction.
        let surface = crate::agent::tool_surface::ResolvedToolSurface::resolve(
            vec![
                tool_definition("read", "Read files", true),
                tool_definition("lsp_definition", "Jump to a symbol definition", true),
            ],
            &std::collections::BTreeSet::new(),
            &std::collections::BTreeSet::new(),
            false,
            true,
            None,
        )
        .expect("resolved surface");
        assert!(surface
            .tools
            .iter()
            .find(|tool| tool.canonical_name == "read")
            .is_some_and(|tool| tool.required && tool.never_reduce));
        let deferred = deferred_from_surface(&surface);
        let advisor = RecordingAdvisor::new(vec!["read".into(), "lsp_definition".into()]);
        let promoted = project_preturn_promotions(
            &surface,
            &deferred,
            "find the symbol definition",
            promote_config(&advisor, 16),
        );
        assert_eq!(
            promoted,
            std::collections::BTreeSet::from(["lsp_definition".into()])
        );
        assert!(!advisor.seen_names().contains(&"read".to_string()));
    }

    #[test]
    fn plugin_wire_alias_mapping_survives_preselection() {
        let surface = crate::agent::tool_surface::ResolvedToolSurface::resolve_with_aliases(
            vec![
                tool_definition("read", "Read files", false),
                tool_definition("mcp__docs_search", "Search documentation pages", true),
            ],
            &std::collections::BTreeSet::new(),
            &std::collections::BTreeSet::new(),
            false,
            true,
            None,
            &std::collections::BTreeMap::from([(
                "mcp__docs_search".to_string(),
                "docs_search".to_string(),
            )]),
        )
        .expect("resolved surface");
        let deferred = deferred_from_surface(&surface);
        assert_eq!(deferred.len(), 1);
        assert_eq!(deferred[0].name, "mcp__docs_search");
        let advisor = RecordingAdvisor::new(vec!["docs_search".into()]);
        let promoted = project_preturn_promotions(
            &surface,
            &deferred,
            "search the documentation pages",
            promote_config(&advisor, 16),
        );
        assert_eq!(
            promoted,
            std::collections::BTreeSet::from(["docs_search".into()])
        );
        assert_eq!(advisor.seen_names(), vec!["docs_search".to_string()]);
    }

    #[test]
    fn unknown_textual_tool_enters_via_descriptor_relevance() {
        let mut definitions = Vec::new();
        for index in 0..6 {
            definitions.push(tool_definition(
                &format!("deferred_{index:02}"),
                "Unrelated scaffold maintenance",
                true,
            ));
        }
        definitions.push(tool_definition(
            "zorg_lattice",
            "Jump to a symbol definition using language server semantics",
            true,
        ));
        let surface = crate::agent::tool_surface::ResolvedToolSurface::resolve(
            definitions,
            &std::collections::BTreeSet::new(),
            &std::collections::BTreeSet::new(),
            false,
            true,
            None,
        )
        .expect("resolved surface");
        let deferred = deferred_from_surface(&surface);
        let advisor = RecordingAdvisor::new(vec!["zorg_lattice".into()]);
        let promoted = project_preturn_promotions(
            &surface,
            &deferred,
            "find the symbol definition",
            promote_config(&advisor, 3),
        );
        assert_eq!(
            promoted,
            std::collections::BTreeSet::from(["zorg_lattice".into()])
        );
    }

    #[test]
    fn empty_and_failed_preselection_leave_the_palette_unchanged() {
        let surface = crate::agent::tool_surface::ResolvedToolSurface::resolve(
            vec![
                tool_definition("read", "Read files", false),
                tool_definition("lsp_definition", "Jump to a symbol definition", true),
            ],
            &std::collections::BTreeSet::new(),
            &std::collections::BTreeSet::new(),
            false,
            true,
            None,
        )
        .expect("resolved surface");
        let deferred = deferred_from_surface(&surface);
        // Empty context short-circuits before the advisor is consulted.
        let advisor = RecordingAdvisor::new(vec!["lsp_definition".into()]);
        assert!(
            project_preturn_promotions(&surface, &deferred, "", promote_config(&advisor, 16),)
                .is_empty()
        );
        assert_eq!(advisor.call_count(), 0);
        // Advisor failure also leaves the palette unchanged.
        let failing = RecordingAdvisor {
            fail: true,
            ..RecordingAdvisor::new(vec!["lsp_definition".into()])
        };
        assert!(project_preturn_promotions(
            &surface,
            &deferred,
            "find the symbol definition",
            promote_config(&failing, 16),
        )
        .is_empty());
    }

    #[test]
    fn candidate_and_schema_budgets_apply_independently() {
        let definitions = vec![
            tool_definition(
                "aaa_first",
                "Jump to a symbol definition using language server semantics",
                true,
            ),
            tool_definition(
                "zzz_second",
                "Jump to a symbol definition using language server semantics",
                true,
            ),
        ];
        let surface = crate::agent::tool_surface::ResolvedToolSurface::resolve(
            definitions,
            &std::collections::BTreeSet::new(),
            &std::collections::BTreeSet::new(),
            false,
            true,
            None,
        )
        .expect("resolved surface");
        let deferred = deferred_from_surface(&surface);
        // The neural budget admits one candidate even though promotion would
        // allow two: at most one promotion is possible.
        let advisor = RecordingAdvisor::new(vec!["aaa_first".into(), "zzz_second".into()]);
        let promoted = project_preturn_promotions(
            &surface,
            &deferred,
            "find the symbol definition",
            promote_config(&advisor, 1),
        );
        assert_eq!(promoted.len(), 1);
        // A starved schema budget independently blocks promotion.
        let starved = PreturnDisclosureConfig {
            schema_budget: 1,
            ..promote_config(&advisor, 16)
        };
        assert!(project_preturn_promotions(
            &surface,
            &deferred,
            "find the symbol definition",
            starved,
        )
        .is_empty());
    }

    #[test]
    fn proactive_disclosure_promotes_only_final_deferred_surface_without_search() {
        let surface = crate::agent::tool_surface::ResolvedToolSurface::resolve(
            vec![
                crate::provider::ToolDefinition {
                    name: "read".into(),
                    description: "Read files".into(),
                    parameters: serde_json::json!({"type": "object"}),
                    defer_loading: None,
                },
                crate::provider::ToolDefinition {
                    name: "lsp_definition".into(),
                    description: "Jump to a symbol definition".into(),
                    parameters: serde_json::json!({"type": "object"}),
                    defer_loading: Some(true),
                },
            ],
            &std::collections::BTreeSet::new(),
            &std::collections::BTreeSet::new(),
            false,
            true,
            None,
        )
        .expect("resolved surface");
        let deferred = surface
            .definitions()
            .into_iter()
            .filter(|definition| definition.defer_loading == Some(true))
            .collect::<Vec<_>>();
        let advisor = RecordingAdvisor::new(vec!["lsp_definition".into()]);
        assert!(project_preturn_promotions(
            &surface,
            &deferred,
            "find the symbol definition",
            PreturnDisclosureConfig {
                advisor: &advisor,
                mode: crate::tool_advisor::AdvisorMode::Off,
                threshold: 0.5,
                max_promotions: 2,
                schema_budget: 16 * 1024,
                max_candidates: 16,
            },
        )
        .is_empty());
        let promoted = project_preturn_promotions(
            &surface,
            &deferred,
            "find the symbol definition",
            PreturnDisclosureConfig {
                advisor: &advisor,
                mode: crate::tool_advisor::AdvisorMode::Promote,
                threshold: 0.5,
                max_promotions: 2,
                schema_budget: 16 * 1024,
                max_candidates: 16,
            },
        );
        assert_eq!(
            promoted,
            std::collections::BTreeSet::from(["lsp_definition".into()])
        );
        assert!(!promoted.contains("not_on_surface"));

        assert!(project_preturn_promotions(
            &surface,
            &deferred,
            "find the symbol definition",
            PreturnDisclosureConfig {
                advisor: &advisor,
                mode: crate::tool_advisor::AdvisorMode::Observe,
                threshold: 0.5,
                max_promotions: 2,
                schema_budget: 16 * 1024,
                max_candidates: 16,
            },
        )
        .is_empty());
        assert!(project_preturn_promotions(
            &surface,
            &deferred,
            "find the symbol definition",
            PreturnDisclosureConfig {
                advisor: &advisor,
                mode: crate::tool_advisor::AdvisorMode::Promote,
                threshold: 0.5,
                max_promotions: 2,
                schema_budget: 1,
                max_candidates: 16,
            },
        )
        .is_empty());
        let mut abstaining_advisor = RecordingAdvisor::new(vec!["lsp_definition".into()]);
        abstaining_advisor.abstain = 0.9;
        assert!(project_preturn_promotions(
            &surface,
            &deferred,
            "find the symbol definition",
            PreturnDisclosureConfig {
                advisor: &abstaining_advisor,
                mode: crate::tool_advisor::AdvisorMode::Promote,
                threshold: 0.5,
                max_promotions: 2,
                schema_budget: 16 * 1024,
                max_candidates: 16,
            },
        )
        .is_empty());
    }

    /// A request pre-seeded as if agent config / model profile already
    /// configured thinking, so precedence is observable.
    fn request_with_thinking_configured() -> ChatRequest {
        ChatRequest {
            messages: Vec::new(),
            model: "test/model".to_string(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            // Both set, as if agent config and model profile both configured
            // thinking: the explicit choice must still win over these.
            reasoning_effort: Some("high".to_string()),
            thinking_budget: Some(8192),
            context: Default::default(),
        }
    }

    #[test]
    fn reasoning_effort_choice_overrides_a_configured_budget() {
        use crate::protocol::core::{ReasoningEffort, ReasoningOverride};
        let mut request = request_with_thinking_configured();
        AgentLoop::apply_reasoning_choice(
            &mut request,
            ReasoningOverride::Effort {
                level: ReasoningEffort::Low,
            },
        );
        assert_eq!(request.reasoning_effort.as_deref(), Some("low"));
        assert_eq!(
            request.thinking_budget, None,
            "effort and budget are mutually exclusive; leaving both would make \
             the wire encoder pick budget over the user's choice"
        );
    }

    #[test]
    fn reasoning_budget_choice_overrides_a_configured_effort() {
        use crate::protocol::core::ReasoningOverride;
        let mut request = request_with_thinking_configured();
        AgentLoop::apply_reasoning_choice(&mut request, ReasoningOverride::Budget { tokens: 2048 });
        assert_eq!(request.thinking_budget, Some(2048));
        assert_eq!(request.reasoning_effort, None);
    }

    #[test]
    fn reasoning_off_clears_both_configured_fields() {
        use crate::protocol::core::ReasoningOverride;
        let mut request = request_with_thinking_configured();
        AgentLoop::apply_reasoning_choice(&mut request, ReasoningOverride::Off);
        assert_eq!(
            (request.reasoning_effort, request.thinking_budget),
            (None, None),
            "Off must clear both, or the model keeps thinking anyway"
        );
    }
}
