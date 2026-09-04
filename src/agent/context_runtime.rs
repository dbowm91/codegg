//! Context policy and context-packer ownership for the agent turn.

use super::r#loop::{AgentLoop, ContextPackObservationPhase};
use crate::agent::processor::EventProcessor;
use crate::provider::{ChatRequest, Message, ToolCall};

impl AgentLoop {
    pub(super) fn build_packer_candidates(
        &self,
        request: &ChatRequest,
    ) -> Vec<crate::context::ContextBlock> {
        let adapter = crate::model_profile::resolve_adapter(None, &request.model);
        let compiler = self.prompt_compiler_fingerprint.clone().unwrap_or_else(|| {
            request
                .messages
                .iter()
                .find_map(|message| match message {
                    Message::System { content } => {
                        Some(crate::context::stable_hash_hex(content.as_bytes()))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| crate::context::stable_hash_hex(""))
        });
        crate::context::ContextPlan::from_request(
            request,
            self.provider.name(),
            &adapter.fingerprint,
            &compiler,
            crate::context::ContextPlanMode::Observation,
        )
        .map(|plan| plan.packing_blocks())
        .unwrap_or_default()
    }

    pub(super) fn compute_context_pack_result(
        &self,
        request: &ChatRequest,
    ) -> Option<crate::context::ContextPackResult> {
        if !self.context_packer_config.enabled.unwrap_or(false) {
            return None;
        }
        let candidates = self.build_packer_candidates(request);
        let budget = crate::context::ContextPackBudget {
            max_tokens: self
                .context_packer_config
                .max_stable_prefix_tokens
                .unwrap_or(32000)
                + self
                    .context_packer_config
                    .max_volatile_tokens
                    .unwrap_or(24000),
            reserved_output_tokens: 10000,
            emergency_margin_tokens: 4000,
        };
        Some(crate::context::packer::pack(candidates, &budget))
    }

    pub(super) fn observe_context_pack(
        &self,
        request: &ChatRequest,
        _model_profile: &crate::model_profile::types::ResolvedModelProfile,
        phase: ContextPackObservationPhase,
    ) {
        if !self.context_packer_config.enabled.unwrap_or(false) {
            return;
        }
        // Emit the active-mode request warning (from Phase 1) at observation time so it is visible
        // for any phase where diagnostics run. (Forced observe-only behavior is unchanged.)
        if !self.context_packer_config.observe_only.unwrap_or(true) {
            tracing::warn!(
                "context-packer active mode is not yet safe; running in observe-only mode"
            );
        }

        let Some(result) = self.compute_context_pack_result(request) else {
            return;
        };

        if self.context_packer_config.log_diagnostics.unwrap_or(true) {
            let model = &request.model;
            let total_candidate_est = result.estimated_tokens
                + result
                    .omitted_blocks
                    .iter()
                    .map(|o| o.estimated_tokens)
                    .sum::<usize>();
            let slow_tokens: usize = result
                .blocks
                .iter()
                .filter(|b| b.kind.tier() == crate::context::CacheClass::SlowChanging)
                .map(|b| b.estimated_tokens)
                .sum();
            let tool_definitions_hash =
                crate::context::tool_definitions_hash(request.tools.as_deref().unwrap_or(&[]));
            let hit_rate = self.context_cache_stats.cache_hit_rate(model);
            let omitted_count = result.omitted_blocks.len();
            let top_omitted: Vec<String> = result
                .omitted_blocks
                .iter()
                .take(5)
                .map(|o| format!("{:?}({}t:{:?})", o.kind, o.estimated_tokens, o.reason))
                .collect();

            tracing::info!(
                "context-packer[{phase:?}]: model={}, candidates={}, packed={}, stable_prefix_tokens={}, slow_changing_tokens={}, volatile_tokens={}, omitted={}, tool_definitions_hash={}, cache_hit_rate={:.4}",
                model,
                total_candidate_est,
                result.estimated_tokens,
                result.stable_prefix_tokens,
                slow_tokens,
                result.volatile_tokens,
                omitted_count,
                tool_definitions_hash,
                hit_rate,
            );
            // Compute effective-cost analysis (observation-only, no mutation)
            let analysis = crate::context::EffectiveCostAnalysis::analyze(
                &self.context_cache_stats,
                model,
                result.stable_prefix_tokens,
                slow_tokens,
                result.volatile_tokens,
            );
            tracing::info!(
                "context-packer[{phase:?}]: recommended_action={}, uncached_input_tokens={}, effective_cache_hit_rate={:.4}, effective_reason={}",
                analysis.recommended_action,
                analysis.uncached_input_tokens,
                analysis.cache_hit_rate,
                analysis.reason,
            );
            if let Some(e) = self.context_cache_stats.get(model) {
                tracing::debug!(
                    "context-packer[{phase:?}]: cache_stats model={} last_in={} last_cached={} total_in={} total_cached={} calls={} rate={:.4}",
                    model, e.last_input_tokens, e.last_cached_tokens, e.total_input_tokens, e.total_cached_tokens, e.call_count, hit_rate
                );
            }
            if !top_omitted.is_empty() {
                tracing::debug!("context-packer[{phase:?}]: top_omitted={:?}", top_omitted);
            }
            for omitted in &result.omitted_blocks {
                tracing::debug!(
                    "context-packer[{phase:?}]: omitted block {:?} ({} tokens, reason: {:?})",
                    omitted.id,
                    omitted.estimated_tokens,
                    omitted.reason,
                );
            }
        }
    }

    pub(super) fn apply_tool_palette_policy_if_active(
        &mut self,
        request: &mut ChatRequest,
        phase: &str,
    ) {
        if !self.context_policy_config.enabled() {
            return;
        }
        let mode = self.context_policy_config.mode();
        if mode == crate::config::schema::ContextPolicyMode::Observe {
            return;
        }
        if self.base_request_tools.is_empty() {
            return;
        }
        if request.tools.is_none() {
            return;
        }
        if let Some(until) = self.context_policy_runtime.reduction_disabled_until_turn {
            if self.state.turn_count <= until {
                tracing::info!(
                    policy = "context_tool_palette",
                    action = "backoff",
                    reduction_disabled_until_turn = ?until,
                    turn_count = self.state.turn_count,
                    "context policy backoff active"
                );
                request.tools = Some(self.base_request_tools.clone());
                self.context_policy_runtime.last_reason =
                    Some("backoff active; using full base palette".to_string());
                return;
            }
        }
        let current_count_for_decision = self.base_request_tools.len();
        let pack_res = self.compute_context_pack_result(request);
        let analysis = if let Some(res) = pack_res {
            let slow_tokens: usize = res
                .blocks
                .iter()
                .filter(|b| b.kind.tier() == crate::context::CacheClass::SlowChanging)
                .map(|b| b.estimated_tokens)
                .sum();
            crate::context::EffectiveCostAnalysis::analyze(
                &self.context_cache_stats,
                &request.model,
                res.stable_prefix_tokens,
                slow_tokens,
                res.volatile_tokens,
            )
        } else {
            return;
        };
        let observed_count = self
            .context_cache_stats
            .get(&request.model)
            .map(|e| e.call_count)
            .unwrap_or(0);
        let decision = crate::context::decide_policy(
            &analysis,
            current_count_for_decision,
            &self.context_policy_config,
            Some(phase),
            observed_count,
            Some(&self.base_request_tools),
        );
        match decision.kind {
            crate::context::ContextPolicyDecisionKind::ReduceToolPalette => {
                let mut red = crate::context::reduce_tool_palette(
                    &self.base_request_tools,
                    &self.context_policy_config,
                    None,
                );
                let cap_exceeded_by_required = red.cap_exceeded_by_required;
                if red.selected.is_empty() && !self.base_request_tools.is_empty() {
                    red = crate::context::ToolPaletteReduction {
                        selected: self.base_request_tools.clone(),
                        omitted: vec![],
                        reason:
                            "fallback to full base palette to avoid empty selection after reduction"
                                .to_string(),
                        cap_exceeded_by_required,
                    };
                    self.context_policy_runtime.reduction_disabled_until_turn =
                        Some(self.state.turn_count + 1);
                }
                let selected = red.selected.clone();
                let omitted = red.omitted.clone();
                let reason = red.reason.clone();
                if let Some(ref mut tlist) = request.tools {
                    *tlist = selected.clone();
                }
                self.context_policy_runtime.last_selected_tool_count = selected.len();
                self.context_policy_runtime.last_omitted_tools = omitted.clone();
                self.context_policy_runtime.last_reason = Some(reason.clone());
                self.context_policy_runtime.last_selected_tools =
                    selected.iter().map(|t| t.name.clone()).collect();
                self.context_policy_runtime.consecutive_reductions += 1;
                if self.context_policy_config.log_policy_decisions() {
                    let reduction_disabled_until_turn =
                        self.context_policy_runtime.reduction_disabled_until_turn;
                    let policy_backoff_active =
                        reduction_disabled_until_turn.is_some_and(|u| self.state.turn_count <= u);
                    tracing::info!(
                        policy = "context_tool_palette",
                        mode = ?mode,
                        action = "ReduceToolPalette",
                        recommended_action = ?decision.recommended_action,
                        base_tool_count = self.base_request_tools.len(),
                        selected_tool_count = selected.len(),
                        omitted_tool_count = omitted.len(),
                        reason = %reason,
                        policy_backoff_active = policy_backoff_active,
                        reduction_disabled_until_turn = ?reduction_disabled_until_turn,
                        cap_exceeded_by_required = cap_exceeded_by_required,
                        "context policy decision"
                    );
                    tracing::debug!(
                        selected = ?selected.iter().map(|t| t.name.clone()).collect::<Vec<_>>(),
                        omitted = ?omitted,
                        "context policy tool selection"
                    );
                    if cap_exceeded_by_required {
                        tracing::debug!(
                            cap_exceeded_by_required = true,
                            "context policy: required tools forced cap overflow"
                        );
                    }
                }
            }
            crate::context::ContextPolicyDecisionKind::WarnOnly
                if self.context_policy_config.log_policy_decisions() =>
            {
                if request.tools.is_some() {
                    request.tools = Some(self.base_request_tools.clone());
                }
                self.context_policy_runtime.last_selected_tool_count = decision.selected_tool_count;
                self.context_policy_runtime.last_omitted_tools = decision.omitted_tools.clone();
                self.context_policy_runtime.last_reason = Some(decision.reason.clone());
                self.context_policy_runtime.last_selected_tools = decision.selected_tools.clone();
                let would_s = decision
                    .would_selected_tool_count
                    .unwrap_or(decision.selected_tool_count);
                let would_o = decision.would_omitted_tool_count.unwrap_or(0);
                tracing::warn!(
                    "context policy would reduce tool palette: {} -> {} ({}) would_select={} would_omit={}",
                    decision.original_tool_count,
                    decision.selected_tool_count,
                    decision.reason,
                    would_s,
                    would_o
                );
            }
            _ => {
                if request.tools.is_some() {
                    request.tools = Some(self.base_request_tools.clone());
                }
                self.context_policy_runtime.last_selected_tool_count = decision.selected_tool_count;
                self.context_policy_runtime.last_omitted_tools = decision.omitted_tools.clone();
                self.context_policy_runtime.last_reason = Some(decision.reason.clone());
                self.context_policy_runtime.last_selected_tools = decision.selected_tools.clone();
            }
        }
    }

    pub(super) fn observe_or_apply_volatile_tail_policy(
        &mut self,
        request: &mut ChatRequest,
        phase: &str,
    ) {
        if !self.context_policy_config.volatile_tail_compaction() {
            return;
        }

        let mode = self.context_policy_config.volatile_tail_mode();

        // Build a minimal effective-cost analysis for the decision gate.
        // Reuse the pack result if available, otherwise build from message estimates.
        let pack_res = self.compute_context_pack_result(request);
        let model_name = &request.model;
        let analysis = if let Some(res) = pack_res {
            let slow_tokens: usize = res
                .blocks
                .iter()
                .filter(|b| b.kind.tier() == crate::context::CacheClass::SlowChanging)
                .map(|b| b.estimated_tokens)
                .sum();
            crate::context::EffectiveCostAnalysis::analyze(
                &self.context_cache_stats,
                model_name,
                res.stable_prefix_tokens,
                slow_tokens,
                res.volatile_tokens,
            )
        } else {
            // Without packer data, build a conservative analysis from message estimates
            let total_tokens: usize = request
                .messages
                .iter()
                .map(crate::context::volatile_tail::estimate_message_tokens)
                .sum();
            crate::context::EffectiveCostAnalysis {
                input_tokens: total_tokens,
                cached_input_tokens: 0,
                uncached_input_tokens: total_tokens,
                cache_hit_rate: 0.0,
                stable_prefix_tokens: 0,
                slow_changing_tokens: 0,
                volatile_tokens: total_tokens,
                recommended_action: if total_tokens > 12000 {
                    crate::context::EffectiveCostAction::CompactVolatileTailFirst
                } else {
                    crate::context::EffectiveCostAction::NoAction
                },
                reason: "no packer data; conservative estimate".into(),
            }
        };

        let plan = crate::context::volatile_tail::plan_volatile_tail_compaction(
            &request.messages,
            &analysis,
            &self.context_policy_config,
        );

        let decision = crate::context::volatile_tail::decide_volatile_tail(
            &analysis,
            &self.context_policy_config,
            &plan,
        );

        match decision.kind {
            crate::context::volatile_tail::VolatileTailDecisionKind::Compact => {
                let applied = crate::context::volatile_tail::apply_volatile_tail_compaction(
                    &mut request.messages,
                    &plan,
                );
                if self.context_policy_config.log_policy_decisions() {
                    tracing::info!(
                        policy = "volatile_tail_compaction",
                        mode = ?mode,
                        action = "Compact",
                        recommended_action = ?analysis.recommended_action,
                        candidate_count = plan.candidates.len(),
                        safe_candidate_count = plan.safe_candidates.len(),
                        planned_compaction_tokens = plan.planned_tokens,
                        applied_compactions = applied,
                        preserved_recent_messages = self.context_policy_config.preserve_recent_messages(),
                        phase = %phase,
                        "volatile tail policy decision"
                    );
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        for c in &plan.safe_candidates {
                            tracing::debug!(
                                message_index = c.message_index,
                                kind = ?c.kind,
                                estimated_tokens = c.estimated_tokens,
                                has_recovery_handle = c.has_recovery_handle,
                                "volatile tail compaction candidate selected"
                            );
                        }
                    }
                }
            }
            crate::context::volatile_tail::VolatileTailDecisionKind::WarnOnly => {
                if self.context_policy_config.log_policy_decisions() {
                    tracing::warn!(
                        policy = "volatile_tail_compaction",
                        mode = ?mode,
                        action = "WarnOnly",
                        recommended_action = ?analysis.recommended_action,
                        candidate_count = plan.candidates.len(),
                        safe_candidate_count = plan.safe_candidates.len(),
                        planned_compaction_tokens = plan.planned_tokens,
                        preserved_recent_messages = self.context_policy_config.preserve_recent_messages(),
                        reason = %decision.reason,
                        phase = %phase,
                        "volatile tail would compact but only warning"
                    );
                }
            }
            crate::context::volatile_tail::VolatileTailDecisionKind::Noop => {
                if self.context_policy_config.log_policy_decisions()
                    && tracing::enabled!(tracing::Level::DEBUG)
                {
                    tracing::debug!(
                        policy = "volatile_tail_compaction",
                        mode = ?mode,
                        action = "Noop",
                        reason = %decision.reason,
                        candidate_count = plan.candidates.len(),
                        phase = %phase,
                        "volatile tail policy noop"
                    );
                }
            }
        }
    }

    pub(super) fn observe_tool_palette_starvation(&mut self, tool_calls: &[ToolCall]) -> bool {
        if self.base_request_tools.is_empty() {
            return false;
        }
        if self.context_policy_runtime.last_selected_tools.is_empty() {
            return false;
        }
        if self.context_policy_runtime.last_omitted_tools.is_empty() {
            return false;
        }

        let base_names: Vec<String> = self
            .base_request_tools
            .iter()
            .map(|t| t.name.clone())
            .collect();
        let called_names: Vec<String> = tool_calls.iter().map(|tc| tc.name.to_string()).collect();
        let starved = crate::context::detect_palette_starvation(
            &base_names,
            &self.context_policy_runtime.last_selected_tools,
            &called_names,
        );

        if !starved.is_empty() {
            for name in &starved {
                tracing::warn!(
                    policy = "context_tool_palette",
                    tool = %name,
                    base_tool_count = self.base_request_tools.len(),
                    last_selected_tool_count = self.context_policy_runtime.last_selected_tool_count,
                    last_omitted_tool_count = self.context_policy_runtime.last_omitted_tools.len(),
                    turn_count = self.state.turn_count,
                    reduction_disabled_until_turn = %(self.state.turn_count + 1),
                    "context policy starvation detected: model attempted omitted base-palette tool"
                );
            }
            self.context_policy_runtime.reduction_disabled_until_turn =
                Some(self.state.turn_count + 1);
            self.context_policy_runtime.last_reason =
                Some("starvation: model attempted omitted base-palette tool".to_string());
        }

        !starved.is_empty()
    }

    pub(super) fn record_context_cache_stats_from_processor(
        &mut self,
        model: &str,
        processor: &EventProcessor,
    ) -> Option<crate::context::NormalizedProviderUsage> {
        if !processor.is_complete() {
            return None;
        }

        let input_tokens = processor.input_tokens();
        let output_tokens = processor.output_tokens();

        // Do not record a fake provider call if usage is completely absent.
        if input_tokens == 0 && output_tokens == 0 && processor.cached_tokens().is_none() {
            return None;
        }

        let usage = crate::context::normalize_from_finish(
            input_tokens,
            output_tokens,
            processor.cached_tokens(),
        );

        let cache_key = self.context_plan_cache_key.as_deref().unwrap_or(model);
        self.context_cache_stats.record_usage(
            cache_key,
            usage.input_tokens,
            usage.cached_input_tokens,
            usage.output_tokens,
        );

        tracing::debug!(
            model = %model,
            cache_key = %cache_key,
            input_tokens = usage.input_tokens,
            cached_input_tokens = ?usage.cached_input_tokens,
            output_tokens = usage.output_tokens,
            cache_hit_rate = self.context_cache_stats.cache_hit_rate(cache_key),
            "updated context cache stats"
        );

        Some(usage)
    }
}
