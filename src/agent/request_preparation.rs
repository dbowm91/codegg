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
                .filter(|score| *score >= config.threshold)
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
        if bytes.saturating_add(definition_bytes) > config.schema_budget {
            continue;
        }
        bytes = bytes.saturating_add(definition_bytes);
        selected.insert(canonical);
        if selected.len() >= config.max_promotions {
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

        // Build an MCP exposure policy from the resolved
        // `[search]` and `[tool_backends.*]` config so raw
        // Codegg-managed backends (eggsearch today, future
        // egglsp/eggsentry MCP adapters) are hidden by default while
        // user-configured third-party MCP servers stay visible.
        // The search config comes from the loop-owned explicit runtime
        // context (M005), never a process-global slot.
        let search_cfg = self.services.search_runtime.config().clone();
        let tool_backends = self.services.tool_registry.tool_backends();
        let expose_raw_search = search_cfg.expose_raw_mcp_tools();
        let eggsearch_server = search_cfg
            .eggsearch
            .as_ref()
            .and_then(|e| e.server_name.clone())
            .unwrap_or_else(|| "eggsearch".to_string());
        let mut hidden_servers: Vec<String> = Vec::new();
        // Always hide eggsearch raw tools unless explicitly opted
        // in via `[search].expose_raw_mcp_tools = true`.
        if !expose_raw_search {
            hidden_servers.push(eggsearch_server.clone());
        }
        // Per-domain backend config: when the user has set
        // `expose_raw_mcp_tools = true` for a managed backend,
        // unhide that server. This is the forward-compatible hook
        // for the future `egglsp` and `eggsentry` MCP adapters.
        for domain_cfg in [
            tool_backends.lsp.as_ref(),
            tool_backends.security.as_ref(),
            tool_backends.context.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(server) = domain_cfg.server_name.as_ref() {
                if domain_cfg.expose_raw_mcp_tools() {
                    hidden_servers.retain(|s| s != server);
                } else {
                    if !hidden_servers.iter().any(|s| s == server) {
                        hidden_servers.push(server.clone());
                    }
                }
            }
        }
        let policy = crate::mcp::McpExposurePolicy {
            show_raw: true,
            hidden_servers,
        };

        let mcp_tools = if let Some(ref mcp_arc) = self.services.mcp_service {
            match mcp_arc.try_read() {
                Ok(mcp) => mcp.list_filtered_tools(&policy),
                Err(_) => {
                    tracing::debug!("MCP service write-locked during tool def building, retrying");
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    mcp_arc
                        .try_read()
                        .map(|mcp| mcp.list_filtered_tools(&policy))
                        .unwrap_or_default()
                }
            }
        } else {
            Vec::new()
        };
        // Set defer_loading on MCP tools based on the catalog
        let catalog = self.services.tool_registry.catalog();
        let mcp_tools: Vec<_> = mcp_tools
            .into_iter()
            .map(|mut t| {
                if catalog.is_deferred(&t.name) {
                    t.defer_loading = Some(true);
                }
                t
            })
            .collect();

        // Cache identity is based on the complete provider-visible MCP
        // surface, not its cardinality. Sorting makes this stable across the
        // HashMap-backed service and the digest contains no credentials or
        // transport configuration.
        let mcp_tool_revision = mcp_tool_surface_revision(&mcp_tools);
        let contextual_immediate = self.contextual_immediate_tools().await;
        let contextual_revision = contextual_immediate
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(",");

        let permission_version = self.permission_version();

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
                && self.services.tool_advisor_mode != crate::tool_advisor::AdvisorMode::Promote
            {
                let mut definitions = cached_defs.clone();
                self.services.deferred_tool_definitions = cached_deferred.clone();

                if let Some(ref plugin_svc) = self.plugin_service {
                    let input = serde_json::json!({
                        "tools": definitions,
                        "model": model,
                    });
                    let hook_result = plugin_svc.dispatch_tool_definition(input).await;
                    if let Some(tools) = hook_result.output.get("tools").and_then(|v| v.as_array())
                    {
                        return tools
                            .iter()
                            .filter_map(|t| {
                                Some(crate::provider::ToolDefinition {
                                    name: t.get("name")?.as_str()?.to_string(),
                                    description: t.get("description")?.as_str()?.to_string(),
                                    parameters: t.get("parameters")?.clone(),
                                    defer_loading: None,
                                })
                            })
                            .collect();
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

        // Include MCP tools in the definitions for deferral partitioning
        let mut all_definitions = all_definitions;
        all_definitions.extend(mcp_tools);

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

        let candidate_deferred: Vec<_> = all_definitions
            .iter()
            .filter(|definition| definition.defer_loading == Some(true))
            .cloned()
            .collect();
        let advisor_context = self.current_advisor_context_v2().await;
        let promoted_names = project_preturn_promotions(
            &surface,
            &candidate_deferred,
            &advisor_context,
            PreturnDisclosureConfig {
                advisor: self.services.tool_advisor.as_ref(),
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
        );

        // Partition tools into immediate vs deferred based on provider capabilities
        let provider_id = self.services.provider.id();
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
            deferred,
        ));

        let mut result = definitions;
        result.extend(self.services.deferred_tool_definitions.iter().cloned());

        if let Some(ref plugin_svc) = self.plugin_service {
            let input = serde_json::json!({
                "tools": result,
                "model": model,
            });
            let hook_result = plugin_svc.dispatch_tool_definition(input).await;
            if let Some(tools) = hook_result.output.get("tools").and_then(|v| v.as_array()) {
                return tools
                    .iter()
                    .filter_map(|t| {
                        Some(crate::provider::ToolDefinition {
                            name: t.get("name")?.as_str()?.to_string(),
                            description: t.get("description")?.as_str()?.to_string(),
                            parameters: t.get("parameters")?.clone(),
                            defer_loading: None,
                        })
                    })
                    .collect();
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
}
