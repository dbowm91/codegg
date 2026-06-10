use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct CacheStatsEntry {
    pub last_input_tokens: usize,
    pub last_cached_tokens: usize,
    pub last_output_tokens: usize,
    pub total_input_tokens: usize,
    pub total_cached_tokens: usize,
    pub total_output_tokens: usize,
    pub call_count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ContextCacheStats {
    entries: HashMap<String, CacheStatsEntry>,
}

impl ContextCacheStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a usage event for a given model key.
    pub fn record_usage(
        &mut self,
        model_key: &str,
        input_tokens: usize,
        cached_tokens: Option<usize>,
        output_tokens: usize,
    ) {
        let entry = self.entries.entry(model_key.to_string()).or_default();
        let cached = cached_tokens.unwrap_or(0);
        entry.last_input_tokens = input_tokens;
        entry.last_cached_tokens = cached;
        entry.last_output_tokens = output_tokens;
        entry.total_input_tokens += input_tokens;
        entry.total_cached_tokens += cached;
        entry.total_output_tokens += output_tokens;
        entry.call_count += 1;
    }

    /// Get the rolling cache hit rate for a model (0.0 - 1.0).
    pub fn cache_hit_rate(&self, model_key: &str) -> f64 {
        self.entries
            .get(model_key)
            .and_then(|e| {
                if e.total_input_tokens == 0 {
                    None
                } else {
                    Some(e.total_cached_tokens as f64 / e.total_input_tokens as f64)
                }
            })
            .unwrap_or(0.0)
    }

    /// Get the cache stats entry for a model.
    pub fn get(&self, model_key: &str) -> Option<&CacheStatsEntry> {
        self.entries.get(model_key)
    }

    /// Get all model keys with stats.
    pub fn models(&self) -> Vec<&str> {
        self.entries.keys().map(|s| s.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_stats_returns_zero_rate() {
        let stats = ContextCacheStats::new();
        assert_eq!(stats.cache_hit_rate("claude-3"), 0.0);
        assert!(stats.get("claude-3").is_none());
        assert!(stats.models().is_empty());
    }

    #[test]
    fn test_none_cached_tokens_treated_as_zero() {
        let mut stats = ContextCacheStats::new();
        stats.record_usage("gpt-4", 1000, None, 500);

        let entry = stats.get("gpt-4").unwrap();
        assert_eq!(entry.last_cached_tokens, 0);
        assert_eq!(entry.total_cached_tokens, 0);
        assert_eq!(entry.last_input_tokens, 1000);
        assert_eq!(entry.total_input_tokens, 1000);
        assert_eq!(entry.last_output_tokens, 500);
        assert_eq!(entry.total_output_tokens, 500);
        assert_eq!(entry.call_count, 1);
        assert_eq!(stats.cache_hit_rate("gpt-4"), 0.0);
    }

    #[test]
    fn test_cache_hit_rate_computed_correctly() {
        let mut stats = ContextCacheStats::new();
        // 500 cached out of 1000 input -> 0.5
        stats.record_usage("model-a", 1000, Some(500), 200);
        assert!((stats.cache_hit_rate("model-a") - 0.5).abs() < f64::EPSILON);

        // Second call: 800 cached out of 2000 input -> (500+800)/(1000+2000) = 1300/3000 ~ 0.4333
        stats.record_usage("model-a", 2000, Some(800), 300);
        let expected = 1300.0 / 3000.0;
        assert!((stats.cache_hit_rate("model-a") - expected).abs() < 1e-10);
    }

    #[test]
    fn test_multiple_models_tracked_independently() {
        let mut stats = ContextCacheStats::new();
        stats.record_usage("claude-3", 1000, Some(600), 100);
        stats.record_usage("gpt-4", 500, Some(100), 50);

        assert_eq!(stats.models().len(), 2);
        assert!((stats.cache_hit_rate("claude-3") - 0.6).abs() < f64::EPSILON);
        assert!((stats.cache_hit_rate("gpt-4") - 0.2).abs() < f64::EPSILON);

        // Verify independence
        let claude = stats.get("claude-3").unwrap();
        assert_eq!(claude.call_count, 1);
        let gpt = stats.get("gpt-4").unwrap();
        assert_eq!(gpt.call_count, 1);
    }

    #[test]
    fn test_rolling_stats_accumulate() {
        let mut stats = ContextCacheStats::new();
        for i in 0..5 {
            stats.record_usage("model", 1000, Some(100 * i), 50);
        }
        let entry = stats.get("model").unwrap();
        assert_eq!(entry.call_count, 5);
        assert_eq!(entry.total_input_tokens, 5000);
        // cached: 0+100+200+300+400 = 1000
        assert_eq!(entry.total_cached_tokens, 1000);
        assert_eq!(entry.total_output_tokens, 250);
        // last values are from the final call
        assert_eq!(entry.last_input_tokens, 1000);
        assert_eq!(entry.last_cached_tokens, 400);
        assert_eq!(entry.last_output_tokens, 50);
    }

    #[test]
    fn test_stats_are_bounded_and_deterministic() {
        let mut stats = ContextCacheStats::new();
        for i in 0..100 {
            let cached = if i % 2 == 0 { Some(500) } else { None };
            stats.record_usage("stable-model", 1000, cached, 100);
        }

        // With deterministic inputs the rates must be reproducible
        let entry = stats.get("stable-model").unwrap();
        assert_eq!(entry.call_count, 100);
        assert_eq!(entry.total_input_tokens, 100_000);
        assert_eq!(entry.total_cached_tokens, 50 * 500); // 50 calls with 500 cached
        let final_rate = stats.cache_hit_rate("stable-model");
        assert!((final_rate - 0.25).abs() < 1e-10);
    }

    #[test]
    fn test_models_returns_unique_keys() {
        let mut stats = ContextCacheStats::new();
        stats.record_usage("a", 100, None, 10);
        stats.record_usage("b", 200, Some(50), 20);
        stats.record_usage("c", 300, None, 30);

        let mut models = stats.models();
        models.sort();
        assert_eq!(models, vec!["a", "b", "c"]);
    }
}
