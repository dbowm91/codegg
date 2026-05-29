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

    pub task_state_policy: Option<TaskStatePolicyConfig>,
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
            task_state_policy: None,
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

    pub task_state_policy: TaskStatePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TodoMode {
    Disabled,
    SparsePlan,
    #[default]
    ExplicitTodo,
    GuidedCurrentTask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TodoUpdateFrequency {
    Never,
    MilestonesOnly,
    #[default]
    MilestonesAndTaskSwitches,
    HarnessManaged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompletedTodoExposure {
    #[default]
    NoneUnlessAsked,
    SummaryOnly,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SubagentTodoAccess {
    #[default]
    None,
    ReadOnlyScoped,
    NoMutation,
    Full,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskStatePolicyConfig {
    pub mode: Option<TodoMode>,
    pub update_frequency: Option<TodoUpdateFrequency>,
    pub max_total_items: Option<usize>,
    pub expose_completed_items: Option<CompletedTodoExposure>,
    pub allow_model_todo_read: Option<bool>,
    pub allow_model_todo_write: Option<bool>,
    pub require_single_in_progress: Option<bool>,
    pub require_blocker_reason: Option<bool>,
    pub inject_after_tool_calls: Option<usize>,
    pub inject_on_resume: Option<bool>,
    pub inject_after_compaction: Option<bool>,
    pub subagent_todo_access: Option<SubagentTodoAccess>,
}

impl Default for TaskStatePolicyConfig {
    fn default() -> Self {
        Self {
            mode: None,
            update_frequency: None,
            max_total_items: None,
            expose_completed_items: None,
            allow_model_todo_read: None,
            allow_model_todo_write: None,
            require_single_in_progress: None,
            require_blocker_reason: None,
            inject_after_tool_calls: None,
            inject_on_resume: None,
            inject_after_compaction: None,
            subagent_todo_access: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskStatePolicy {
    pub mode: TodoMode,
    pub update_frequency: TodoUpdateFrequency,
    pub max_total_items: usize,
    pub expose_completed_items: CompletedTodoExposure,
    pub allow_model_todo_read: bool,
    pub allow_model_todo_write: bool,
    pub require_single_in_progress: bool,
    pub require_blocker_reason: bool,
    pub inject_after_tool_calls: Option<usize>,
    pub inject_on_resume: bool,
    pub inject_after_compaction: bool,
    pub subagent_todo_access: SubagentTodoAccess,
}

impl Default for TaskStatePolicy {
    fn default() -> Self {
        Self::explicit_todo()
    }
}

impl TaskStatePolicy {
    pub fn sparse_plan() -> Self {
        Self {
            mode: TodoMode::SparsePlan,
            update_frequency: TodoUpdateFrequency::MilestonesOnly,
            max_total_items: 8,
            expose_completed_items: CompletedTodoExposure::NoneUnlessAsked,
            allow_model_todo_read: true,
            allow_model_todo_write: true,
            require_single_in_progress: true,
            require_blocker_reason: false,
            inject_after_tool_calls: Some(10),
            inject_on_resume: true,
            inject_after_compaction: true,
            subagent_todo_access: SubagentTodoAccess::None,
        }
    }

    pub fn explicit_todo() -> Self {
        Self {
            mode: TodoMode::ExplicitTodo,
            update_frequency: TodoUpdateFrequency::MilestonesAndTaskSwitches,
            max_total_items: 10,
            expose_completed_items: CompletedTodoExposure::SummaryOnly,
            allow_model_todo_read: true,
            allow_model_todo_write: true,
            require_single_in_progress: true,
            require_blocker_reason: false,
            inject_after_tool_calls: Some(5),
            inject_on_resume: true,
            inject_after_compaction: true,
            subagent_todo_access: SubagentTodoAccess::None,
        }
    }

    pub fn guided_current_task() -> Self {
        Self {
            mode: TodoMode::GuidedCurrentTask,
            update_frequency: TodoUpdateFrequency::HarnessManaged,
            max_total_items: 4,
            expose_completed_items: CompletedTodoExposure::NoneUnlessAsked,
            allow_model_todo_read: true,
            allow_model_todo_write: false,
            require_single_in_progress: true,
            require_blocker_reason: false,
            inject_after_tool_calls: Some(3),
            inject_on_resume: true,
            inject_after_compaction: true,
            subagent_todo_access: SubagentTodoAccess::None,
        }
    }

    pub fn disabled() -> Self {
        Self {
            mode: TodoMode::Disabled,
            update_frequency: TodoUpdateFrequency::Never,
            max_total_items: 0,
            expose_completed_items: CompletedTodoExposure::NoneUnlessAsked,
            allow_model_todo_read: false,
            allow_model_todo_write: false,
            require_single_in_progress: false,
            require_blocker_reason: false,
            inject_after_tool_calls: None,
            inject_on_resume: false,
            inject_after_compaction: false,
            subagent_todo_access: SubagentTodoAccess::None,
        }
    }

    pub fn apply_config(mut self, cfg: &TaskStatePolicyConfig) -> Self {
        if let Some(v) = cfg.mode {
            self.mode = v;
        }
        if let Some(v) = cfg.update_frequency {
            self.update_frequency = v;
        }
        if let Some(v) = cfg.max_total_items {
            self.max_total_items = v;
        }
        if let Some(v) = cfg.expose_completed_items {
            self.expose_completed_items = v;
        }
        if let Some(v) = cfg.allow_model_todo_read {
            self.allow_model_todo_read = v;
        }
        if let Some(v) = cfg.allow_model_todo_write {
            self.allow_model_todo_write = v;
        }
        if let Some(v) = cfg.require_single_in_progress {
            self.require_single_in_progress = v;
        }
        if let Some(v) = cfg.require_blocker_reason {
            self.require_blocker_reason = v;
        }
        if cfg.inject_after_tool_calls.is_some() {
            self.inject_after_tool_calls = cfg.inject_after_tool_calls;
        }
        if let Some(v) = cfg.inject_on_resume {
            self.inject_on_resume = v;
        }
        if let Some(v) = cfg.inject_after_compaction {
            self.inject_after_compaction = v;
        }
        if let Some(v) = cfg.subagent_todo_access {
            self.subagent_todo_access = v;
        }
        self.validate()
    }

    pub fn validate(mut self) -> Self {
        if self.mode == TodoMode::Disabled {
            self.allow_model_todo_read = false;
            self.allow_model_todo_write = false;
            self.inject_after_tool_calls = None;
            self.max_total_items = 0;
        }
        if self.mode == TodoMode::GuidedCurrentTask {
            self.allow_model_todo_write = false;
            self.max_total_items = self.max_total_items.min(4);
        }
        if self.max_total_items > 12 {
            self.max_total_items = 12;
        }
        self
    }
}
