use crate::config::schema::ContextPolicyConfig;
use crate::context::effective_cost::{EffectiveCostAction, EffectiveCostAnalysis};
use crate::provider::ToolDefinition;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextPolicyDecisionKind {
    Noop,
    WarnOnly,
    ReduceToolPalette,
}

#[derive(Debug, Clone)]
pub struct ContextPolicyDecision {
    pub kind: ContextPolicyDecisionKind,
    pub reason: String,
    pub recommended_action: EffectiveCostAction,
    pub original_tool_count: usize,
    pub selected_tool_count: usize,
    pub selected_tools: Vec<String>,
    pub omitted_tools: Vec<String>,
    /// For WarnOnly decisions: the counts that *would* have been selected/omitted if reduction had been applied (dry-run).
    /// None for non-warn or when not computed.
    pub would_selected_tool_count: Option<usize>,
    pub would_omitted_tool_count: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct ToolPaletteReduction {
    pub selected: Vec<ToolDefinition>,
    pub omitted: Vec<String>,
    pub reason: String,
    pub cap_exceeded_by_required: bool,
}

pub fn decide_policy(
    analysis: &EffectiveCostAnalysis,
    current_tool_count: usize,
    config: &ContextPolicyConfig,
    phase: Option<&str>,
    observed_count: usize,
    // Optional base palette for Warn-mode dry-run computation of would_ counts.
    // When Some, Warn will compute reduce_tool_palette(base, ...) to populate would_*.
    // When None, would_* remain None for Warn (backward compat for tests that don't pass it).
    base_tools_for_dry_run: Option<&[ToolDefinition]>,
) -> ContextPolicyDecision {
    if !config.enabled() {
        return ContextPolicyDecision {
            kind: ContextPolicyDecisionKind::Noop,
            reason: "policy disabled".to_string(),
            recommended_action: analysis.recommended_action,
            original_tool_count: current_tool_count,
            selected_tool_count: current_tool_count,
            selected_tools: vec![],
            omitted_tools: vec![],
            would_selected_tool_count: None,
            would_omitted_tool_count: None,
        };
    }

    let mode = config.mode();
    if mode == crate::config::schema::ContextPolicyMode::Observe {
        return ContextPolicyDecision {
            kind: ContextPolicyDecisionKind::Noop,
            reason: "mode=observe (diagnostics only)".to_string(),
            recommended_action: analysis.recommended_action,
            original_tool_count: current_tool_count,
            selected_tool_count: current_tool_count,
            selected_tools: vec![],
            omitted_tools: vec![],
            would_selected_tool_count: None,
            would_omitted_tool_count: None,
        };
    }

    let is_review = config.review_tool_palette_threshold()
        && analysis.recommended_action == EffectiveCostAction::ReviewToolPalette;
    let meets_obs = observed_count >= config.min_cache_observations();
    let over_cap = current_tool_count > config.max_tool_definitions();
    let phase_ok = phase.map_or(true, |p| {
        let pl = p.to_lowercase();
        pl.contains("beforeprovider") || pl.contains("initial") || pl.contains("before_provider")
    });

    if mode == crate::config::schema::ContextPolicyMode::Warn && is_review {
        // Dry-run reduction for observability in Warn mode (no mutation).
        let (would_sel, would_omit) = if let Some(base) = base_tools_for_dry_run {
            let red = reduce_tool_palette(base, config, None);
            (Some(red.selected.len()), Some(red.omitted.len()))
        } else {
            (None, None)
        };
        return ContextPolicyDecision {
            kind: ContextPolicyDecisionKind::WarnOnly,
            reason: format!(
                "would reduce tool palette: {} tools (recommended ReviewToolPalette after {} obs, over cap={})",
                current_tool_count, observed_count, over_cap
            ),
            recommended_action: analysis.recommended_action,
            original_tool_count: current_tool_count,
            selected_tool_count: current_tool_count,
            selected_tools: vec![],
            omitted_tools: vec![],
            would_selected_tool_count: would_sel,
            would_omitted_tool_count: would_omit,
        };
    }

    if mode == crate::config::schema::ContextPolicyMode::ToolPaletteReduce
        && is_review
        && meets_obs
        && over_cap
        && phase_ok
    {
        let max = config.max_tool_definitions();
        return ContextPolicyDecision {
            kind: ContextPolicyDecisionKind::ReduceToolPalette,
            reason: format!(
                "reducing tool palette from {} to <= {} (ReviewToolPalette, obs={}, phase ok)",
                current_tool_count, max, observed_count
            ),
            recommended_action: analysis.recommended_action,
            original_tool_count: current_tool_count,
            selected_tool_count: std::cmp::min(current_tool_count, max),
            selected_tools: vec![],
            omitted_tools: vec![],
            would_selected_tool_count: None,
            would_omitted_tool_count: None,
        };
    }

    ContextPolicyDecision {
        kind: ContextPolicyDecisionKind::Noop,
        reason: "no policy action triggered".to_string(),
        recommended_action: analysis.recommended_action,
        original_tool_count: current_tool_count,
        selected_tool_count: current_tool_count,
        selected_tools: vec![],
        omitted_tools: vec![],
        would_selected_tool_count: None,
        would_omitted_tool_count: None,
    }
}

pub fn reduce_tool_palette(
    tools: &[ToolDefinition],
    config: &ContextPolicyConfig,
    required_names: Option<&[String]>,
) -> ToolPaletteReduction {
    if tools.is_empty() {
        return ToolPaletteReduction {
            selected: vec![],
            omitted: vec![],
            reason: "no tools to reduce".to_string(),
            cap_exceeded_by_required: false,
        };
    }

    let max = config.max_tool_definitions();
    let mut req: std::collections::HashSet<String> = std::collections::HashSet::new();
    for n in [
        "context_read",
        "tool_search",
        "todowrite",
        "question",
        "plan_enter",
        "plan_exit",
    ] {
        req.insert(n.to_string());
    }
    for n in config.always_include_tools() {
        req.insert(n);
    }
    for n in config.never_reduce_tools() {
        req.insert(n);
    }
    if let Some(extra) = required_names {
        for n in extra {
            req.insert(n.clone());
        }
    }

    let mut selected: Vec<ToolDefinition> = vec![];
    let mut included_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for t in tools {
        if req.contains(&t.name) && !included_names.contains(&t.name) {
            selected.push(t.clone());
            included_names.insert(t.name.clone());
        }
    }

    let required_included_count = selected.len();
    let cap_exceeded_by_required = required_included_count > max;

    let mut reason = if cap_exceeded_by_required {
        format!(
            "required tools ({}) exceed max_tool_definitions ({}); included all required (cap exceeded by required)",
            required_included_count, max
        )
    } else {
        String::new()
    };

    if !cap_exceeded_by_required {
        for t in tools {
            if selected.len() >= max {
                break;
            }
            if !included_names.contains(&t.name) {
                selected.push(t.clone());
                included_names.insert(t.name.clone());
            }
        }
    }

    if selected.is_empty() && !tools.is_empty() {
        let fallback = if max == 0 { 1 } else { max };
        let take_n = std::cmp::min(fallback, tools.len());
        selected = tools.iter().take(take_n).cloned().collect();
        included_names = selected.iter().map(|t| t.name.clone()).collect();
        let fb = format!("fallback to first {} to avoid empty palette", take_n);
        if reason.is_empty() {
            reason = fb;
        } else {
            reason = format!("{}; {}", reason, fb);
        }
    }

    let omitted: Vec<String> = tools
        .iter()
        .filter(|t| !included_names.contains(&t.name))
        .map(|t| t.name.clone())
        .collect();

    if reason.is_empty() {
        reason = format!(
            "tool palette reduced to {} of {} to respect max_tool_definitions={}",
            selected.len(),
            tools.len(),
            max
        );
    }

    ToolPaletteReduction {
        selected,
        omitted,
        reason,
        cap_exceeded_by_required,
    }
}

/// Detect tool-palette starvation: the model attempted a tool that exists in the
/// unreduced base palette but was omitted from the selected (reduced) palette.
///
/// Returns a de-duplicated list of omitted base-palette tool names that were called,
/// in call order. Only tools present in `base_tool_names` AND absent from
/// `selected_tool_names` are reported; unknown tool names are never blamed on
/// policy reduction.
///
/// If `selected_tool_names` is empty (no reduction has occurred), returns empty.
pub fn detect_palette_starvation(
    base_tool_names: &[String],
    selected_tool_names: &[String],
    called_tool_names: &[String],
) -> Vec<String> {
    if selected_tool_names.is_empty() || base_tool_names.is_empty() {
        return Vec::new();
    }
    let base_set: std::collections::HashSet<&str> =
        base_tool_names.iter().map(|s| s.as_str()).collect();
    let selected_set: std::collections::HashSet<&str> =
        selected_tool_names.iter().map(|s| s.as_str()).collect();

    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for name in called_tool_names {
        if base_set.contains(name.as_str())
            && !selected_set.contains(name.as_str())
            && seen.insert(name.clone())
        {
            result.push(name.clone());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::ContextPolicyMode;
    use serde_json::json;

    fn make_tool(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: format!("desc for {}", name),
            parameters: json!({}),
            defer_loading: None,
        }
    }

    fn make_analysis(action: EffectiveCostAction) -> EffectiveCostAnalysis {
        EffectiveCostAnalysis {
            input_tokens: 10000,
            cached_input_tokens: 2000,
            uncached_input_tokens: 8000,
            cache_hit_rate: 0.2,
            stable_prefix_tokens: 1000,
            slow_changing_tokens: 7000,
            volatile_tokens: 2000,
            recommended_action: action,
            reason: "slow-changing tokens high; review tool palette".to_string(),
        }
    }

    fn make_config(
        enabled: bool,
        mode: ContextPolicyMode,
        min_obs: usize,
        max_tools: usize,
    ) -> ContextPolicyConfig {
        ContextPolicyConfig {
            enabled: Some(enabled),
            mode: Some(mode),
            min_cache_observations: Some(min_obs),
            review_tool_palette_threshold: Some(true),
            max_tool_definitions: Some(max_tools),
            always_include_tools: None,
            never_reduce_tools: None,
            log_policy_decisions: Some(true),
            volatile_tail_compaction: None,
            volatile_tail_mode: None,
            min_volatile_tokens_for_compaction: None,
            preserve_recent_messages: None,
            max_compacted_tail_tokens: None,
            require_effective_cost_signal: None,
            compact_tool_results_only_first: None,
        }
    }

    #[test]
    fn disabled_always_noop() {
        let cfg = make_config(false, ContextPolicyMode::ToolPaletteReduce, 3, 24);
        let a = make_analysis(EffectiveCostAction::ReviewToolPalette);
        let d = decide_policy(&a, 30, &cfg, Some("BeforeProviderCall"), 10, None);
        assert_eq!(d.kind, ContextPolicyDecisionKind::Noop);
        assert_eq!(d.original_tool_count, 30);
        assert_eq!(d.selected_tool_count, 30);
    }

    #[test]
    fn observe_always_noop() {
        let cfg = make_config(true, ContextPolicyMode::Observe, 3, 24);
        let a = make_analysis(EffectiveCostAction::ReviewToolPalette);
        let d = decide_policy(&a, 30, &cfg, None, 10, None);
        assert_eq!(d.kind, ContextPolicyDecisionKind::Noop);
        assert!(d.reason.contains("observe"));
    }

    #[test]
    fn warn_produces_warnonly_no_mutation() {
        let cfg = make_config(true, ContextPolicyMode::Warn, 3, 24);
        let a = make_analysis(EffectiveCostAction::ReviewToolPalette);
        let d = decide_policy(&a, 30, &cfg, Some("BeforeProviderCall"), 5, None);
        assert_eq!(d.kind, ContextPolicyDecisionKind::WarnOnly);
        assert_eq!(d.selected_tool_count, 30);
        assert!(d.selected_tools.is_empty());
        assert!(d.omitted_tools.is_empty());
    }

    #[test]
    fn reduce_only_when_review_enough_obs_over_cap_phase_ok() {
        let cfg = make_config(true, ContextPolicyMode::ToolPaletteReduce, 3, 10);
        let a = make_analysis(EffectiveCostAction::ReviewToolPalette);
        let d = decide_policy(&a, 30, &cfg, Some("BeforeProviderCall"), 5, None);
        assert_eq!(d.kind, ContextPolicyDecisionKind::ReduceToolPalette);
    }

    #[test]
    fn reduce_noop_when_not_enough_obs() {
        let cfg = make_config(true, ContextPolicyMode::ToolPaletteReduce, 10, 10);
        let a = make_analysis(EffectiveCostAction::ReviewToolPalette);
        let d = decide_policy(&a, 30, &cfg, Some("BeforeProviderCall"), 2, None);
        assert_eq!(d.kind, ContextPolicyDecisionKind::Noop);
    }

    #[test]
    fn reduce_noop_when_not_over_cap() {
        let cfg = make_config(true, ContextPolicyMode::ToolPaletteReduce, 3, 50);
        let a = make_analysis(EffectiveCostAction::ReviewToolPalette);
        let d = decide_policy(&a, 20, &cfg, Some("BeforeProviderCall"), 5, None);
        assert_eq!(d.kind, ContextPolicyDecisionKind::Noop);
    }

    #[test]
    fn reduce_noop_when_not_review() {
        let cfg = make_config(true, ContextPolicyMode::ToolPaletteReduce, 3, 10);
        let a = make_analysis(EffectiveCostAction::NoAction);
        let d = decide_policy(&a, 30, &cfg, Some("BeforeProviderCall"), 5, None);
        assert_eq!(d.kind, ContextPolicyDecisionKind::Noop);
    }

    #[test]
    fn reduce_noop_when_phase_not_provider() {
        let cfg = make_config(true, ContextPolicyMode::ToolPaletteReduce, 3, 10);
        let a = make_analysis(EffectiveCostAction::ReviewToolPalette);
        let d = decide_policy(&a, 30, &cfg, Some("AfterToolResults"), 5, None);
        assert_eq!(d.kind, ContextPolicyDecisionKind::Noop);
    }

    #[test]
    fn required_preserved_and_cap_respected() {
        let cfg = make_config(true, ContextPolicyMode::ToolPaletteReduce, 1, 5);
        let tools = vec![
            make_tool("bash"),
            make_tool("read"),
            make_tool("context_read"),
            make_tool("todowrite"),
            make_tool("tool_search"),
            make_tool("question"),
            make_tool("edit"),
            make_tool("plan_enter"),
        ];
        let red = reduce_tool_palette(&tools, &cfg, None);
        let names: Vec<_> = red.selected.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"context_read"));
        assert!(names.contains(&"todowrite"));
        assert!(names.contains(&"tool_search"));
        assert!(names.contains(&"question"));
        assert!(names.contains(&"plan_enter"));
        assert!(red.selected.len() <= 5);
        assert!(!red.cap_exceeded_by_required);
    }

    #[test]
    fn cap_exceeded_by_required_flagged() {
        let cfg = make_config(true, ContextPolicyMode::ToolPaletteReduce, 1, 2);
        let tools = vec![
            make_tool("context_read"),
            make_tool("todowrite"),
            make_tool("tool_search"),
            make_tool("question"),
            make_tool("plan_enter"),
            make_tool("plan_exit"),
        ];
        let red = reduce_tool_palette(&tools, &cfg, None);
        assert!(red.cap_exceeded_by_required);
        assert_eq!(red.selected.len(), 6);
        assert!(red.reason.contains("exceed"));
    }

    #[test]
    fn deterministic_order_and_non_required_fill() {
        let cfg = make_config(true, ContextPolicyMode::ToolPaletteReduce, 1, 4);
        let tools = vec![
            make_tool("bash"),
            make_tool("read"),
            make_tool("context_read"),
            make_tool("edit"),
            make_tool("todowrite"),
            make_tool("ls"),
        ];
        let red = reduce_tool_palette(&tools, &cfg, None);
        let names: Vec<_> = red.selected.iter().map(|t| t.name.as_str()).collect();
        // Required tools (context_read, todowrite, + schema defaults) are collected first
        // in the relative order they appear in the input, then non-required fill the rest.
        // The exact interleaving is deterministic from the two-pass "required then original-order" rule.
        assert!(names.contains(&"context_read"));
        assert!(names.contains(&"todowrite"));
        assert!(names.len() <= 4);
        // First two after required collection should be the non-required that appeared before the later requireds in original order.
        assert!(names[0] == "bash" || names[0] == "context_read");
    }

    #[test]
    fn tool_search_preserved() {
        let cfg = make_config(true, ContextPolicyMode::ToolPaletteReduce, 1, 3);
        let tools = vec![
            make_tool("bash"),
            make_tool("tool_search"),
            make_tool("edit"),
            make_tool("read"),
            make_tool("write"),
        ];
        let red = reduce_tool_palette(&tools, &cfg, None);
        let names: Vec<_> = red.selected.iter().map(|t| t.name.as_str()).collect();
        // tool_search is both a hardcoded "always include if present" name and appears in schema defaults.
        // Because it is present in the input, it must be preserved.
        assert!(names.contains(&"tool_search"));
        // context_read is in schema defaults but NOT present in this input slice, so it is not synthesized.
        assert!(!names.contains(&"context_read"));
        assert!(names.len() <= 3);
    }

    #[test]
    fn empty_fallback_when_no_required() {
        let cfg = make_config(true, ContextPolicyMode::ToolPaletteReduce, 1, 0);
        let tools = vec![make_tool("bash"), make_tool("read")];
        let red = reduce_tool_palette(&tools, &cfg, None);
        assert!(!red.selected.is_empty());
        assert!(red.reason.contains("fallback"));
    }

    #[test]
    fn extra_required_names_via_param() {
        let cfg = make_config(true, ContextPolicyMode::ToolPaletteReduce, 1, 3);
        let tools = vec![make_tool("bash"), make_tool("foo"), make_tool("bar")];
        let red = reduce_tool_palette(&tools, &cfg, Some(&["foo".to_string()]));
        let names: Vec<_> = red.selected.iter().map(|t| t.name.as_str()).collect();
        // "foo" was passed explicitly as required via the param, so it must be kept.
        assert!(names.contains(&"foo"));
        // context_read is a schema default but is NOT present in this input list; reducer never synthesizes missing tools.
        assert!(!names.contains(&"context_read"));
        assert!(names.len() <= 3);
    }

    #[test]
    fn never_reduce_tools_treated_required() {
        let mut cfg = make_config(true, ContextPolicyMode::ToolPaletteReduce, 1, 2);
        cfg.never_reduce_tools = Some(vec!["bash".into()]);
        let tools = vec![
            make_tool("bash"),
            make_tool("read"),
            make_tool("edit"),
            make_tool("write"),
        ];
        let red = reduce_tool_palette(&tools, &cfg, None);
        let names: Vec<_> = red.selected.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"bash"));
        assert!(names.len() <= 2);
    }

    // Phase 8 hardening tests

    #[test]
    fn review_threshold_false_disables_review_trigger() {
        let mut cfg = make_config(true, ContextPolicyMode::ToolPaletteReduce, 1, 10);
        cfg.review_tool_palette_threshold = Some(false);
        let a = make_analysis(EffectiveCostAction::ReviewToolPalette);
        // Even with Review action, threshold=false => is_review=false => Noop
        let d = decide_policy(&a, 30, &cfg, Some("BeforeProviderCall"), 5, None);
        assert_eq!(d.kind, ContextPolicyDecisionKind::Noop);
        assert!(d.reason.contains("no policy action"));
    }

    #[test]
    fn warn_with_base_populates_would_counts_dry_run() {
        let cfg = make_config(true, ContextPolicyMode::Warn, 1, 5);
        let a = make_analysis(EffectiveCostAction::ReviewToolPalette);
        let base = vec![
            make_tool("bash"),
            make_tool("read"),
            make_tool("edit"),
            make_tool("context_read"),
            make_tool("todowrite"),
            make_tool("tool_search"),
        ];
        let d = decide_policy(
            &a,
            base.len(),
            &cfg,
            Some("BeforeProviderCall"),
            3,
            Some(&base),
        );
        assert_eq!(d.kind, ContextPolicyDecisionKind::WarnOnly);
        // Decision itself reports current (no mutation), but would_* must be populated from dry-run reduce
        assert!(d.would_selected_tool_count.is_some());
        assert!(d.would_omitted_tool_count.is_some());
        // Would select should respect cap and requireds; at most 5, and must keep context_read/todowrite/tool_search
        let ws = d.would_selected_tool_count.unwrap();
        assert!(ws <= 5);
        assert!(ws >= 3); // at least the 3 required that are present
    }

    #[test]
    fn repeated_reduce_from_same_base_yields_identical_selection() {
        let cfg = make_config(true, ContextPolicyMode::ToolPaletteReduce, 1, 4);
        let base = vec![
            make_tool("bash"),
            make_tool("read"),
            make_tool("context_read"),
            make_tool("todowrite"),
            make_tool("edit"),
            make_tool("tool_search"),
        ];
        let r1 = reduce_tool_palette(&base, &cfg, None);
        let r2 = reduce_tool_palette(&base, &cfg, None);
        assert_eq!(r1.selected.len(), r2.selected.len());
        let n1: Vec<_> = r1.selected.iter().map(|t| t.name.clone()).collect();
        let n2: Vec<_> = r2.selected.iter().map(|t| t.name.clone()).collect();
        assert_eq!(n1, n2);
        // Non-cumulative: second reduce from full base does not shrink further
        assert!(n1.len() <= 4);
    }

    // --- detect_palette_starvation tests ---

    #[test]
    fn starvation_omitted_base_tool_triggers() {
        let base = vec!["read".into(), "bash".into(), "edit".into()];
        let selected = vec!["read".into(), "edit".into()];
        let called = vec!["bash".into()];
        let starved = detect_palette_starvation(&base, &selected, &called);
        assert_eq!(starved, vec!["bash"]);
    }

    #[test]
    fn starvation_selected_tool_does_not_trigger() {
        let base = vec!["read".into(), "bash".into(), "edit".into()];
        let selected = vec!["read".into(), "edit".into()];
        let called = vec!["read".into()];
        let starved = detect_palette_starvation(&base, &selected, &called);
        assert!(starved.is_empty());
    }

    #[test]
    fn starvation_unknown_tool_does_not_trigger() {
        let base = vec!["read".into(), "edit".into()];
        let selected = vec!["read".into()];
        let called = vec!["nonexistent_tool".into()];
        let starved = detect_palette_starvation(&base, &selected, &called);
        assert!(starved.is_empty());
    }

    #[test]
    fn starvation_no_prior_reduction_does_not_trigger() {
        let base = vec!["read".into(), "bash".into()];
        let selected: Vec<String> = Vec::new();
        let called = vec!["bash".into()];
        let starved = detect_palette_starvation(&base, &selected, &called);
        assert!(starved.is_empty());
    }

    #[test]
    fn starvation_multiple_omitted_calls_deduplicated() {
        let base = vec!["read".into(), "bash".into(), "grep".into(), "edit".into()];
        let selected = vec!["read".into()];
        let called = vec!["grep".into(), "bash".into(), "grep".into()];
        let starved = detect_palette_starvation(&base, &selected, &called);
        assert_eq!(starved, vec!["grep", "bash"]);
    }

    #[test]
    fn starvation_empty_base_returns_empty() {
        let base: Vec<String> = Vec::new();
        let selected = vec!["read".into()];
        let called = vec!["bash".into()];
        let starved = detect_palette_starvation(&base, &selected, &called);
        assert!(starved.is_empty());
    }

    #[test]
    fn starvation_empty_called_returns_empty() {
        let base = vec!["read".into(), "bash".into()];
        let selected = vec!["read".into()];
        let called: Vec<String> = Vec::new();
        let starved = detect_palette_starvation(&base, &selected, &called);
        assert!(starved.is_empty());
    }

    // --- review_tool_palette_threshold=false in Warn mode ---

    #[test]
    fn warn_mode_review_threshold_false_disables_review_trigger() {
        let mut cfg = make_config(true, ContextPolicyMode::Warn, 1, 10);
        cfg.review_tool_palette_threshold = Some(false);
        let a = make_analysis(EffectiveCostAction::ReviewToolPalette);
        let d = decide_policy(&a, 30, &cfg, Some("BeforeProviderCall"), 5, None);
        assert_eq!(d.kind, ContextPolicyDecisionKind::Noop);
        assert!(d.reason.contains("no policy action"));
    }
}
