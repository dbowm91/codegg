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

impl AgentLoop {
    /// Apply tool exposure filtering based on execution policy's initial_tool_mode.
    fn apply_tool_exposure_filter(
        &self,
        definitions: Vec<crate::provider::ToolDefinition>,
    ) -> Vec<crate::provider::ToolDefinition> {
        let Some(ref policy) = self.services.execution_policy else {
            return definitions;
        };

        // First apply exposure mode filter. Palettes are owned by the
        // canonical disclosure module (M002); no second list lives here.
        let filtered = match policy.initial_tool_mode {
            crate::agent::policy::ToolExposureMode::Full => definitions,
            crate::agent::policy::ToolExposureMode::Curated => definitions
                .into_iter()
                .filter(|t| crate::tool::disclosure::CURATED_PALETTE.contains(&t.name.as_str()))
                .collect(),
            crate::agent::policy::ToolExposureMode::MinimalWithDiscovery => definitions
                .into_iter()
                .filter(|t| crate::tool::disclosure::MINIMAL_PALETTE.contains(&t.name.as_str()))
                .collect(),
        };

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
        let Some(provider_name) = decision.resolved_model.split('/').next() else {
            return Err(crate::error::AppError::Provider(
                crate::error::ProviderError::NotFound(decision.resolved_model),
            ));
        };
        let Some(provider) = self.services.provider_registry.get(provider_name) else {
            return Err(crate::error::AppError::Provider(
                crate::error::ProviderError::NotFound(format!(
                    "Provider '{}' not found",
                    provider_name
                )),
            ));
        };
        self.services.provider = provider.clone_box();
        request.model = decision
            .resolved_model
            .split('/')
            .next_back()
            .unwrap_or(&decision.resolved_model)
            .to_string();
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

        let permission_version = self.permission_version();

        if let Some((
            ref cache_model,
            cache_plan,
            cache_lsp,
            ref cache_mcp_count,
            cache_perm_ver,
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
                && cache_expose_raw == expose_raw_search
                && cache_tool_deferral == &self.services.config.tool_deferral
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

        let all_definitions = self.apply_tool_exposure_filter(all_definitions);

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
        let surface = match crate::agent::tool_surface::ResolvedToolSurface::resolve(
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
                let should_defer = !is_always_loaded && def.defer_loading == Some(true);

                if should_defer {
                    deferred_tools.push(def);
                } else {
                    immediate.push(def);
                }
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
}
