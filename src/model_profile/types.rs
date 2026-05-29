use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PromptProfileKind {
    FrontierReasoning,
    FrontierExecutor,
    FastExecutor,
    LocalStrict,
    ToolFragile,
    LongContextPlanner,
    Reviewer,
    Summarizer,
    #[default]
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReliabilityTier {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelProfileConfig {
    pub prompt_profile: Option<PromptProfileKind>,
    pub family: Option<String>,

    pub context_window: Option<usize>,
    pub max_output_tokens: Option<usize>,

    pub tool_call_reliability: Option<ReliabilityTier>,
    pub instruction_adherence: Option<ReliabilityTier>,
    pub patch_reliability: Option<ReliabilityTier>,

    pub supports_late_system_messages: Option<bool>,
    pub prefers_user_control_messages: Option<bool>,
    pub prefers_small_patches: Option<bool>,
    pub requires_explicit_tool_contract: Option<bool>,
    pub requires_post_tool_continue_nudge: Option<bool>,

    pub default_reasoning_effort: Option<String>,
    pub default_thinking_budget: Option<usize>,

    pub max_parallel_tools: Option<usize>,
    pub preferred_tools: Option<Vec<String>>,
    pub disabled_tools: Option<Vec<String>>,
}

impl Default for ModelProfileConfig {
    fn default() -> Self {
        Self {
            prompt_profile: None,
            family: None,
            context_window: None,
            max_output_tokens: None,
            tool_call_reliability: None,
            instruction_adherence: None,
            patch_reliability: None,
            supports_late_system_messages: None,
            prefers_user_control_messages: None,
            prefers_small_patches: None,
            requires_explicit_tool_contract: None,
            requires_post_tool_continue_nudge: None,
            default_reasoning_effort: None,
            default_thinking_budget: None,
            max_parallel_tools: None,
            preferred_tools: None,
            disabled_tools: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedModelProfile {
    pub model: String,
    pub prompt_profile: PromptProfileKind,
    pub family: String,

    pub context_window: Option<usize>,
    pub max_output_tokens: Option<usize>,

    pub tool_call_reliability: ReliabilityTier,
    pub instruction_adherence: ReliabilityTier,
    pub patch_reliability: ReliabilityTier,

    pub supports_late_system_messages: bool,
    pub prefers_user_control_messages: bool,
    pub prefers_small_patches: bool,
    pub requires_explicit_tool_contract: bool,
    pub requires_post_tool_continue_nudge: bool,

    pub default_reasoning_effort: Option<String>,
    pub default_thinking_budget: Option<usize>,

    pub max_parallel_tools: Option<usize>,
    pub preferred_tools: Option<Vec<String>>,
    pub disabled_tools: Option<Vec<String>>,
}
