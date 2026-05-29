use crate::config::schema::Config;
use crate::model_profile::types::{PromptProfileKind, ResolvedModelProfile, TaskStatePolicy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExposureMode {
    Full,
    Curated,
    MinimalWithDiscovery,
}

#[derive(Debug, Clone)]
pub struct ExecutionPolicy {
    pub model: String,
    pub prompt_profile: PromptProfileKind,
    pub context_window: usize,
    pub compaction_threshold: f64,
    pub reserved_output_tokens: usize,
    pub max_tool_result_tokens: usize,
    pub max_parallel_tools: usize,
    pub expose_tool_search: bool,
    pub initial_tool_mode: ToolExposureMode,
    pub allow_bootstrap_tool: bool,
    pub allow_post_tool_continue_nudge: bool,
    pub prefer_user_control_messages: bool,
    pub supports_late_system_messages: bool,
    pub disabled_tools: Option<Vec<String>>,
    pub task_state_policy: TaskStatePolicy,
}

impl ExecutionPolicy {
    pub fn from_profile(profile: &ResolvedModelProfile, config: &Config) -> Self {
        let compaction_threshold = config
            .compaction
            .as_ref()
            .and_then(|c| c.threshold)
            .unwrap_or_else(|| default_threshold(profile));

        let max_parallel_tools = config
            .server
            .as_ref()
            .and_then(|s| s.max_parallel_tools)
            .or(profile.max_parallel_tools)
            .unwrap_or_else(|| default_max_parallel(profile));

        let context_window = config
            .compaction
            .as_ref()
            .and_then(|c| c.max_tokens)
            .or(profile.context_window)
            .unwrap_or_else(|| default_context_window(profile));

        let reserved_output_tokens = config
            .compaction
            .as_ref()
            .and_then(|c| c.reserved)
            .unwrap_or_else(|| default_reserved(profile));

        let max_tool_result_tokens = default_max_tool_result_tokens(profile);

        let initial_tool_mode = default_tool_exposure(profile);
        let allow_bootstrap_tool = matches!(
            initial_tool_mode,
            ToolExposureMode::MinimalWithDiscovery
        ) || profile.requires_explicit_tool_contract;
        let allow_post_tool_continue_nudge = profile.requires_post_tool_continue_nudge;

        ExecutionPolicy {
            model: profile.model.clone(),
            prompt_profile: profile.prompt_profile,
            context_window,
            compaction_threshold,
            reserved_output_tokens,
            max_tool_result_tokens,
            max_parallel_tools,
            expose_tool_search: true,
            initial_tool_mode,
            allow_bootstrap_tool,
            allow_post_tool_continue_nudge,
            prefer_user_control_messages: profile.prefers_user_control_messages,
            supports_late_system_messages: profile.supports_late_system_messages,
            disabled_tools: profile.disabled_tools.clone(),
            task_state_policy: profile.task_state_policy.clone(),
        }
    }
}

fn default_context_window(profile: &ResolvedModelProfile) -> usize {
    match profile.prompt_profile {
        PromptProfileKind::FrontierReasoning | PromptProfileKind::FrontierExecutor => 128_000,
        PromptProfileKind::LongContextPlanner => 512_000,
        PromptProfileKind::FastExecutor | PromptProfileKind::ToolFragile => 128_000,
        PromptProfileKind::LocalStrict => 32_000,
        PromptProfileKind::Reviewer => 128_000,
        PromptProfileKind::Summarizer => 64_000,
        PromptProfileKind::Default => 128_000,
    }
}

fn default_threshold(profile: &ResolvedModelProfile) -> f64 {
    match profile.prompt_profile {
        PromptProfileKind::FrontierReasoning | PromptProfileKind::FrontierExecutor => 0.85,
        PromptProfileKind::LongContextPlanner => 0.70,
        PromptProfileKind::FastExecutor | PromptProfileKind::ToolFragile => 0.70,
        PromptProfileKind::LocalStrict => 0.65,
        PromptProfileKind::Reviewer => 0.80,
        PromptProfileKind::Summarizer => 0.75,
        PromptProfileKind::Default => 0.85,
    }
}

fn default_reserved(profile: &ResolvedModelProfile) -> usize {
    match profile.prompt_profile {
        PromptProfileKind::FrontierReasoning | PromptProfileKind::FrontierExecutor => 12_000,
        PromptProfileKind::LongContextPlanner => 16_000,
        PromptProfileKind::FastExecutor | PromptProfileKind::ToolFragile => 8_000,
        PromptProfileKind::LocalStrict => 4_000,
        PromptProfileKind::Reviewer => 10_000,
        PromptProfileKind::Summarizer => 4_000,
        PromptProfileKind::Default => 10_000,
    }
}

fn default_max_tool_result_tokens(profile: &ResolvedModelProfile) -> usize {
    match profile.prompt_profile {
        PromptProfileKind::FrontierReasoning | PromptProfileKind::FrontierExecutor => 8_000,
        PromptProfileKind::LongContextPlanner => 8_000,
        PromptProfileKind::FastExecutor | PromptProfileKind::ToolFragile => 4_000,
        PromptProfileKind::LocalStrict => 2_000,
        PromptProfileKind::Reviewer => 6_000,
        PromptProfileKind::Summarizer => 2_000,
        PromptProfileKind::Default => 6_000,
    }
}

fn default_max_parallel(profile: &ResolvedModelProfile) -> usize {
    match profile.prompt_profile {
        PromptProfileKind::FrontierReasoning | PromptProfileKind::FrontierExecutor => 10,
        PromptProfileKind::LongContextPlanner => 8,
        PromptProfileKind::FastExecutor | PromptProfileKind::ToolFragile => 2,
        PromptProfileKind::LocalStrict => 1,
        PromptProfileKind::Reviewer => 4,
        PromptProfileKind::Summarizer => 1,
        PromptProfileKind::Default => 6,
    }
}

fn default_tool_exposure(profile: &ResolvedModelProfile) -> ToolExposureMode {
    match profile.prompt_profile {
        PromptProfileKind::FrontierReasoning | PromptProfileKind::FrontierExecutor => {
            ToolExposureMode::Curated
        }
        PromptProfileKind::LongContextPlanner => ToolExposureMode::Curated,
        PromptProfileKind::FastExecutor | PromptProfileKind::ToolFragile => {
            ToolExposureMode::MinimalWithDiscovery
        }
        PromptProfileKind::LocalStrict => ToolExposureMode::MinimalWithDiscovery,
        PromptProfileKind::Reviewer => ToolExposureMode::Curated,
        PromptProfileKind::Summarizer => ToolExposureMode::MinimalWithDiscovery,
        PromptProfileKind::Default => ToolExposureMode::Full,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_profile::resolve::infer_builtin_profile;

    #[test]
    fn test_frontier_reasoning_policy() {
        let profile = infer_builtin_profile("claude-sonnet-4-20250514");
        let config = Config::default();
        let policy = ExecutionPolicy::from_profile(&profile, &config);

        assert_eq!(policy.context_window, 128_000);
        assert!((policy.compaction_threshold - 0.85).abs() < f64::EPSILON);
        assert_eq!(policy.reserved_output_tokens, 12_000);
        assert_eq!(policy.max_tool_result_tokens, 8_000);
        assert_eq!(policy.max_parallel_tools, 10);
        assert_eq!(policy.initial_tool_mode, ToolExposureMode::Curated);
        assert!(!policy.allow_bootstrap_tool);
        assert!(!policy.allow_post_tool_continue_nudge);
        assert!(policy.supports_late_system_messages);
    }

    #[test]
    fn test_fast_executor_tool_fragile_policy() {
        let profile = infer_builtin_profile("minimax/minimax-2.7");
        let config = Config::default();
        let policy = ExecutionPolicy::from_profile(&profile, &config);

        assert_eq!(policy.context_window, 128_000);
        assert!((policy.compaction_threshold - 0.70).abs() < f64::EPSILON);
        assert_eq!(policy.reserved_output_tokens, 8_000);
        assert_eq!(policy.max_tool_result_tokens, 4_000);
        assert!(policy.max_parallel_tools <= 2);
        assert_eq!(policy.initial_tool_mode, ToolExposureMode::MinimalWithDiscovery);
        assert!(policy.allow_bootstrap_tool);
        assert!(policy.allow_post_tool_continue_nudge);
        assert!(!policy.supports_late_system_messages);
    }

    #[test]
    fn test_local_strict_policy() {
        let profile = infer_builtin_profile("ollama/qwen2.5-coder:32b");
        let config = Config::default();
        let policy = ExecutionPolicy::from_profile(&profile, &config);

        assert_eq!(policy.context_window, 32_000);
        assert!((policy.compaction_threshold - 0.65).abs() < f64::EPSILON);
        assert_eq!(policy.reserved_output_tokens, 4_000);
        assert_eq!(policy.max_tool_result_tokens, 2_000);
        assert_eq!(policy.max_parallel_tools, 1);
        assert_eq!(policy.initial_tool_mode, ToolExposureMode::MinimalWithDiscovery);
    }

    #[test]
    fn test_config_override_wins() {
        let mut config = Config::default();
        config.compaction = Some(crate::config::schema::CompactionConfig {
            max_tokens: Some(256_000),
            threshold: Some(0.90),
            reserved: Some(20_000),
            ..Default::default()
        });
        config.server = Some(crate::config::schema::ServerConfig {
            max_parallel_tools: Some(4),
            ..Default::default()
        });

        let profile = infer_builtin_profile("openai/gpt-5");
        let policy = ExecutionPolicy::from_profile(&profile, &config);

        assert_eq!(policy.context_window, 256_000);
        assert!((policy.compaction_threshold - 0.90).abs() < f64::EPSILON);
        assert_eq!(policy.reserved_output_tokens, 20_000);
        assert_eq!(policy.max_parallel_tools, 4);
    }

    #[test]
    fn test_long_context_planner_policy() {
        let profile = infer_builtin_profile("gemini-2.5-pro");
        let config = Config::default();
        let policy = ExecutionPolicy::from_profile(&profile, &config);

        assert_eq!(policy.context_window, 512_000);
        assert!((policy.compaction_threshold - 0.70).abs() < f64::EPSILON);
        assert_eq!(policy.reserved_output_tokens, 16_000);
        assert_eq!(policy.initial_tool_mode, ToolExposureMode::Curated);
    }

    #[test]
    fn test_default_model_full_exposure() {
        let profile = infer_builtin_profile("some-unknown/model");
        let config = Config::default();
        let policy = ExecutionPolicy::from_profile(&profile, &config);

        assert_eq!(policy.initial_tool_mode, ToolExposureMode::Full);
        assert_eq!(policy.context_window, 128_000);
    }
}
