use crate::context::cache_stats::ContextCacheStats;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveCostAction {
    PreserveStablePrefix,
    CompactVolatileTailFirst,
    ReviewToolPalette,
    NoAction,
}

impl std::fmt::Display for EffectiveCostAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PreserveStablePrefix => write!(f, "preserve_stable_prefix"),
            Self::CompactVolatileTailFirst => write!(f, "compact_volatile_tail_first"),
            Self::ReviewToolPalette => write!(f, "review_tool_palette"),
            Self::NoAction => write!(f, "no_action"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EffectiveCostAnalysis {
    pub input_tokens: usize,
    pub cached_input_tokens: usize,
    pub uncached_input_tokens: usize,
    pub cache_hit_rate: f64,
    pub stable_prefix_tokens: usize,
    pub slow_changing_tokens: usize,
    pub volatile_tokens: usize,
    pub recommended_action: EffectiveCostAction,
    pub reason: String,
}

impl EffectiveCostAnalysis {
    /// Compute an effective-cost analysis from cache stats and pack result token breakdown.
    /// This is purely observational - it does not mutate any context or provider requests.
    pub fn analyze(
        cache_stats: &ContextCacheStats,
        model: &str,
        stable_prefix_tokens: usize,
        slow_changing_tokens: usize,
        volatile_tokens: usize,
    ) -> Self {
        let entry = cache_stats.get(model);
        let input_tokens = entry.map(|e| e.total_input_tokens).unwrap_or(0);
        let cached_input_tokens = entry.map(|e| e.total_cached_tokens).unwrap_or(0);
        let uncached_input_tokens = input_tokens.saturating_sub(cached_input_tokens);
        let cache_hit_rate = cache_stats.cache_hit_rate(model);

        let total = stable_prefix_tokens + slow_changing_tokens + volatile_tokens;
        let (recommended_action, reason) = Self::recommend(
            cache_hit_rate,
            stable_prefix_tokens,
            slow_changing_tokens,
            volatile_tokens,
            total,
        );

        Self {
            input_tokens,
            cached_input_tokens,
            uncached_input_tokens,
            cache_hit_rate,
            stable_prefix_tokens,
            slow_changing_tokens,
            volatile_tokens,
            recommended_action,
            reason,
        }
    }

    fn recommend(
        cache_hit_rate: f64,
        stable_prefix_tokens: usize,
        slow_changing_tokens: usize,
        volatile_tokens: usize,
        total: usize,
    ) -> (EffectiveCostAction, String) {
        if total == 0 {
            return (
                EffectiveCostAction::NoAction,
                "no token data available".to_string(),
            );
        }

        let stable_ratio = stable_prefix_tokens as f64 / total as f64;
        let volatile_ratio = volatile_tokens as f64 / total as f64;
        let slow_ratio = slow_changing_tokens as f64 / total as f64;

        if cache_hit_rate > 0.5 && stable_ratio > 0.3 {
            return (
                EffectiveCostAction::PreserveStablePrefix,
                format!(
                    "cache hit rate {:.2} is high and stable prefix is {:.0}% of total; preserving stable prefix maximizes cache reuse",
                    cache_hit_rate, stable_ratio * 100.0
                ),
            );
        }

        if volatile_ratio > 0.4 && cache_hit_rate < 0.3 {
            return (
                EffectiveCostAction::CompactVolatileTailFirst,
                format!(
                    "volatile tokens are {:.0}% of total with low cache hit rate {:.2}; compacting volatile tail first would reduce uncached burden",
                    volatile_ratio * 100.0, cache_hit_rate
                ),
            );
        }

        if slow_ratio > 0.3 && cache_hit_rate < 0.5 {
            return (
                EffectiveCostAction::ReviewToolPalette,
                format!(
                    "slow-changing tokens (tool definitions, goals) are {:.0}% of total with moderate cache hit rate {:.2}; reviewing tool palette may reduce overhead",
                    slow_ratio * 100.0, cache_hit_rate
                ),
            );
        }

        (
            EffectiveCostAction::NoAction,
            format!(
                "no strong signal: cache_hit_rate={:.2}, stable={:.0}%, volatile={:.0}%, slow={:.0}%",
                cache_hit_rate,
                stable_ratio * 100.0,
                volatile_ratio * 100.0,
                slow_ratio * 100.0,
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::cache_stats::ContextCacheStats;

    fn make_stats(
        model: &str,
        input: usize,
        cached: Option<usize>,
        output: usize,
    ) -> ContextCacheStats {
        let mut stats = ContextCacheStats::new();
        stats.record_usage(model, input, cached, output);
        stats
    }

    #[test]
    fn high_cache_large_stable_prefix_recommends_preserve() {
        let stats = make_stats("gpt-4", 10000, Some(8000), 2000);
        let analysis = EffectiveCostAnalysis::analyze(&stats, "gpt-4", 5000, 2000, 1000);
        assert_eq!(
            analysis.recommended_action,
            EffectiveCostAction::PreserveStablePrefix
        );
        assert!(analysis.reason.contains("cache hit rate"));
    }

    #[test]
    fn large_volatile_low_cache_recommends_compact() {
        let stats = make_stats("gpt-4", 10000, Some(1000), 2000);
        let analysis = EffectiveCostAnalysis::analyze(&stats, "gpt-4", 1000, 2000, 5000);
        assert_eq!(
            analysis.recommended_action,
            EffectiveCostAction::CompactVolatileTailFirst
        );
        assert!(analysis.reason.contains("compact"));
    }

    #[test]
    fn tool_heavy_context_recommends_review() {
        let stats = make_stats("gpt-4", 10000, Some(3000), 2000);
        let analysis = EffectiveCostAnalysis::analyze(&stats, "gpt-4", 1000, 5000, 2000);
        assert_eq!(
            analysis.recommended_action,
            EffectiveCostAction::ReviewToolPalette
        );
        assert!(analysis.reason.contains("slow-changing"));
    }

    #[test]
    fn empty_stats_recommends_no_action() {
        let stats = ContextCacheStats::new();
        let analysis = EffectiveCostAnalysis::analyze(&stats, "gpt-4", 0, 0, 0);
        assert_eq!(analysis.recommended_action, EffectiveCostAction::NoAction);
    }

    #[test]
    fn no_cached_tokens_zeros_out() {
        let stats = make_stats("gpt-4", 5000, None, 1000);
        let analysis = EffectiveCostAnalysis::analyze(&stats, "gpt-4", 2000, 1500, 1500);
        assert_eq!(analysis.cached_input_tokens, 0);
        assert_eq!(analysis.uncached_input_tokens, 5000);
        assert_eq!(analysis.cache_hit_rate, 0.0);
    }

    #[test]
    fn action_display_names() {
        assert_eq!(
            EffectiveCostAction::PreserveStablePrefix.to_string(),
            "preserve_stable_prefix"
        );
        assert_eq!(
            EffectiveCostAction::CompactVolatileTailFirst.to_string(),
            "compact_volatile_tail_first"
        );
        assert_eq!(
            EffectiveCostAction::ReviewToolPalette.to_string(),
            "review_tool_palette"
        );
        assert_eq!(EffectiveCostAction::NoAction.to_string(), "no_action");
    }
}
