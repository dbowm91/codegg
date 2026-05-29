use crate::config::schema::Config;
use crate::model_profile::types::{
    ModelProfileConfig, PromptProfileKind, ReliabilityTier, ResolvedModelProfile, TaskStatePolicy,
};

pub struct ModelProfileResolver<'a> {
    config: &'a Config,
}

impl<'a> ModelProfileResolver<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }

    pub fn resolve(&self, model: &str) -> ResolvedModelProfile {
        let base = if let Some(cfg) = self.find_config_override(model) {
            let built_in = infer_builtin_profile(model);
            apply_config_override(built_in, cfg)
        } else {
            infer_builtin_profile(model)
        };
        base
    }

    fn find_config_override(&self, model: &str) -> Option<&ModelProfileConfig> {
        let profiles = self.config.model_profile.as_ref()?;
        if let Some(cfg) = profiles.get(model) {
            return Some(cfg);
        }
        if let Some(suffix) = model.split_once('/') {
            if let Some(cfg) = profiles.get(suffix.1) {
                return Some(cfg);
            }
            if let Some(deeper) = suffix.1.split_once('/') {
                if let Some(cfg) = profiles.get(deeper.1) {
                    return Some(cfg);
                }
            }
        }
        None
    }
}

pub fn apply_config_override(
    mut base: ResolvedModelProfile,
    cfg: &ModelProfileConfig,
) -> ResolvedModelProfile {
    if let Some(v) = cfg.prompt_profile {
        base.prompt_profile = v;
    }
    if let Some(ref v) = cfg.family {
        base.family = v.clone();
    }
    if let Some(v) = cfg.context_window {
        base.context_window = Some(v);
    }
    if let Some(v) = cfg.max_output_tokens {
        base.max_output_tokens = Some(v);
    }
    if let Some(v) = cfg.tool_call_reliability {
        base.tool_call_reliability = v;
    }
    if let Some(v) = cfg.instruction_adherence {
        base.instruction_adherence = v;
    }
    if let Some(v) = cfg.patch_reliability {
        base.patch_reliability = v;
    }
    if let Some(v) = cfg.supports_late_system_messages {
        base.supports_late_system_messages = v;
    }
    if let Some(v) = cfg.prefers_user_control_messages {
        base.prefers_user_control_messages = v;
    }
    if let Some(v) = cfg.prefers_small_patches {
        base.prefers_small_patches = v;
    }
    if let Some(v) = cfg.requires_explicit_tool_contract {
        base.requires_explicit_tool_contract = v;
    }
    if let Some(v) = cfg.requires_post_tool_continue_nudge {
        base.requires_post_tool_continue_nudge = v;
    }
    if let Some(ref v) = cfg.default_reasoning_effort {
        base.default_reasoning_effort = Some(v.clone());
    }
    if let Some(v) = cfg.default_thinking_budget {
        base.default_thinking_budget = Some(v);
    }
    if let Some(v) = cfg.max_parallel_tools {
        base.max_parallel_tools = Some(v);
    }
    if let Some(ref v) = cfg.preferred_tools {
        base.preferred_tools = Some(v.clone());
    }
    if let Some(ref v) = cfg.disabled_tools {
        base.disabled_tools = Some(v.clone());
    }
    if let Some(ref v) = cfg.task_state_policy {
        base.task_state_policy = base.task_state_policy.apply_config(v);
    }
    base
}

pub fn infer_builtin_profile(model: &str) -> ResolvedModelProfile {
    let id = model.to_lowercase();

    if id.contains("minimax") {
        return fast_executor_tool_fragile(model, "minimax");
    }

    if id.contains("gpt")
        || id.contains("o1")
        || id.contains("o3")
        || id.contains("o4")
        || id.contains("codex")
    {
        return frontier_reasoning(model, "openai");
    }

    if id.contains("claude") || id.contains("sonnet") || id.contains("opus") || id.contains("haiku")
    {
        return frontier_reasoning(model, "anthropic");
    }

    if id.contains("gemini") {
        return long_context_planner(model, "google");
    }

    if id.contains("deepseek") {
        return frontier_executor(model, "deepseek");
    }

    if id.contains("qwen") || id.contains("qwq") {
        return local_or_open_executor(model, "qwen");
    }

    if id.contains("kimi") {
        return frontier_executor(model, "kimi");
    }

    if id.contains("ollama")
        || id.contains("lmstudio")
        || id.contains("localhost")
        || id.contains("local")
    {
        return local_strict(model, "local");
    }

    default_profile(model)
}

fn frontier_reasoning(model: &str, family: &str) -> ResolvedModelProfile {
    ResolvedModelProfile {
        model: model.to_string(),
        prompt_profile: PromptProfileKind::FrontierReasoning,
        family: family.to_string(),
        context_window: Some(128_000),
        max_output_tokens: Some(16_384),
        tool_call_reliability: ReliabilityTier::High,
        instruction_adherence: ReliabilityTier::High,
        patch_reliability: ReliabilityTier::High,
        supports_late_system_messages: true,
        prefers_user_control_messages: false,
        prefers_small_patches: false,
        requires_explicit_tool_contract: false,
        requires_post_tool_continue_nudge: false,
        default_reasoning_effort: None,
        default_thinking_budget: None,
        max_parallel_tools: None,
        preferred_tools: None,
        disabled_tools: None,
        task_state_policy: TaskStatePolicy::default(),
    }
}

fn frontier_executor(model: &str, family: &str) -> ResolvedModelProfile {
    ResolvedModelProfile {
        model: model.to_string(),
        prompt_profile: PromptProfileKind::FrontierExecutor,
        family: family.to_string(),
        context_window: Some(128_000),
        max_output_tokens: Some(16_384),
        tool_call_reliability: ReliabilityTier::High,
        instruction_adherence: ReliabilityTier::High,
        patch_reliability: ReliabilityTier::High,
        supports_late_system_messages: true,
        prefers_user_control_messages: false,
        prefers_small_patches: false,
        requires_explicit_tool_contract: false,
        requires_post_tool_continue_nudge: false,
        default_reasoning_effort: None,
        default_thinking_budget: None,
        max_parallel_tools: None,
        preferred_tools: None,
        disabled_tools: None,
        task_state_policy: TaskStatePolicy::default(),
    }
}

fn fast_executor_tool_fragile(model: &str, family: &str) -> ResolvedModelProfile {
    ResolvedModelProfile {
        model: model.to_string(),
        prompt_profile: PromptProfileKind::FastExecutor,
        family: family.to_string(),
        context_window: Some(128_000),
        max_output_tokens: Some(8_192),
        tool_call_reliability: ReliabilityTier::Medium,
        instruction_adherence: ReliabilityTier::Medium,
        patch_reliability: ReliabilityTier::Medium,
        supports_late_system_messages: false,
        prefers_user_control_messages: true,
        prefers_small_patches: true,
        requires_explicit_tool_contract: true,
        requires_post_tool_continue_nudge: true,
        default_reasoning_effort: None,
        default_thinking_budget: None,
        max_parallel_tools: Some(2),
        preferred_tools: None,
        disabled_tools: None,
        task_state_policy: TaskStatePolicy::guided_current_task(),
    }
}

fn long_context_planner(model: &str, family: &str) -> ResolvedModelProfile {
    ResolvedModelProfile {
        model: model.to_string(),
        prompt_profile: PromptProfileKind::LongContextPlanner,
        family: family.to_string(),
        context_window: Some(512_000),
        max_output_tokens: Some(16_384),
        tool_call_reliability: ReliabilityTier::High,
        instruction_adherence: ReliabilityTier::High,
        patch_reliability: ReliabilityTier::High,
        supports_late_system_messages: true,
        prefers_user_control_messages: false,
        prefers_small_patches: false,
        requires_explicit_tool_contract: false,
        requires_post_tool_continue_nudge: false,
        default_reasoning_effort: None,
        default_thinking_budget: None,
        max_parallel_tools: Some(8),
        preferred_tools: None,
        disabled_tools: None,
        task_state_policy: TaskStatePolicy::default(),
    }
}

fn local_or_open_executor(model: &str, family: &str) -> ResolvedModelProfile {
    ResolvedModelProfile {
        model: model.to_string(),
        prompt_profile: PromptProfileKind::LocalStrict,
        family: family.to_string(),
        context_window: Some(32_000),
        max_output_tokens: Some(4_096),
        tool_call_reliability: ReliabilityTier::Medium,
        instruction_adherence: ReliabilityTier::Medium,
        patch_reliability: ReliabilityTier::Medium,
        supports_late_system_messages: false,
        prefers_user_control_messages: true,
        prefers_small_patches: true,
        requires_explicit_tool_contract: true,
        requires_post_tool_continue_nudge: true,
        max_parallel_tools: Some(1),
        preferred_tools: None,
        disabled_tools: None,
        default_reasoning_effort: None,
        default_thinking_budget: None,
        task_state_policy: TaskStatePolicy::guided_current_task(),
    }
}

fn local_strict(model: &str, family: &str) -> ResolvedModelProfile {
    ResolvedModelProfile {
        model: model.to_string(),
        prompt_profile: PromptProfileKind::LocalStrict,
        family: family.to_string(),
        context_window: Some(32_000),
        max_output_tokens: Some(4_096),
        tool_call_reliability: ReliabilityTier::Medium,
        instruction_adherence: ReliabilityTier::Medium,
        patch_reliability: ReliabilityTier::Medium,
        supports_late_system_messages: false,
        prefers_user_control_messages: true,
        prefers_small_patches: true,
        requires_explicit_tool_contract: true,
        requires_post_tool_continue_nudge: true,
        max_parallel_tools: Some(1),
        preferred_tools: None,
        disabled_tools: None,
        default_reasoning_effort: None,
        default_thinking_budget: None,
        task_state_policy: TaskStatePolicy::guided_current_task(),
    }
}

pub fn default_profile(model: &str) -> ResolvedModelProfile {
    ResolvedModelProfile {
        model: model.to_string(),
        prompt_profile: PromptProfileKind::Default,
        family: "default".to_string(),
        context_window: Some(128_000),
        max_output_tokens: Some(8_192),
        tool_call_reliability: ReliabilityTier::Medium,
        instruction_adherence: ReliabilityTier::Medium,
        patch_reliability: ReliabilityTier::Medium,
        supports_late_system_messages: true,
        prefers_user_control_messages: false,
        prefers_small_patches: false,
        requires_explicit_tool_contract: false,
        requires_post_tool_continue_nudge: false,
        default_reasoning_effort: None,
        default_thinking_budget: None,
        max_parallel_tools: None,
        preferred_tools: None,
        disabled_tools: None,
        task_state_policy: TaskStatePolicy::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimax_inference() {
        let profile = infer_builtin_profile("minimax/minimax-2.7");
        assert_eq!(profile.prompt_profile, PromptProfileKind::FastExecutor);
        assert!(!profile.supports_late_system_messages);
        assert!(profile.requires_explicit_tool_contract);
        assert_eq!(profile.family, "minimax");
    }

    #[test]
    fn test_unknown_model_defaults() {
        let profile = infer_builtin_profile("some-provider/some-model");
        assert_eq!(profile.prompt_profile, PromptProfileKind::Default);
        assert_eq!(profile.family, "default");
    }

    #[test]
    fn test_local_model_inference() {
        let profile = infer_builtin_profile("ollama/qwen2.5-coder:32b");
        assert_eq!(profile.prompt_profile, PromptProfileKind::LocalStrict);
        assert!(!profile.supports_late_system_messages);
        assert_eq!(profile.max_parallel_tools, Some(1));
    }

    #[test]
    fn test_config_override_wins() {
        let mut config = Config::default();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert(
            "minimax/minimax-2.7".to_string(),
            ModelProfileConfig {
                supports_late_system_messages: Some(true),
                ..Default::default()
            },
        );
        config.model_profile = Some(overrides);

        let resolver = ModelProfileResolver::new(&config);
        let profile = resolver.resolve("minimax/minimax-2.7");
        assert!(profile.supports_late_system_messages);
    }

    #[test]
    fn test_config_override_suffix_match() {
        let mut config = Config::default();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert(
            "qwen3-coder".to_string(),
            ModelProfileConfig {
                prompt_profile: Some(PromptProfileKind::FastExecutor),
                ..Default::default()
            },
        );
        config.model_profile = Some(overrides);

        let resolver = ModelProfileResolver::new(&config);
        let profile = resolver.resolve("openrouter/qwen/qwen3-coder");
        assert_eq!(profile.prompt_profile, PromptProfileKind::FastExecutor);
    }
}
