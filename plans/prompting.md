# Codegg Initiative Plan: Profile-Aware Prompting and Model Behavior Policy

**Status**: 10/10 PHASES COMPLETE (verified 2026-06-02)

| Phase | Status | Notes |
|-------|--------|-------|
| 1. Profile types | **DONE** | `src/model_profile/types.rs` |
| 2. Config support | **DONE** | `Config.model_profile` field |
| 3. Built-in resolver | **DONE** | `ModelProfileResolver` with 5 tests |
| 4. Module wiring | **DONE** | `src/model_profile/mod.rs` |
| 5. Replace MiniMax late system msg | **DONE** | Profile-based `should_avoid_late_system_messages` |
| 6. Replace MiniMax tool contract | **DONE** | `apply_startup_profile_policy` with 12 tests |
| 7. Profile defaults to ChatRequest | **DONE** | `apply_model_profile_defaults` |
| 8. Prompt fragment composition | **DONE** | `assemble_system_prompt_with_profile` with 13 tests |
| 9. Agent role field | **DONE** | `Agent.role` + `role_contract()` |
| 10. Profile-aware tool policy | **DONE** | `filter_tool_definitions_for_profile` implemented (2026-06-02) |

All phases complete.

## Goal

Implement profile-aware prompting for codegg without creating an unmaintainable set of per-model prompt files.

The desired architecture is:

```text
model string + agent role + config overrides
    -> resolved model profile
    -> prompt fragments + runtime policy
    -> final ChatRequest behavior
```

This should support different classes of models, such as frontier reasoning models, fast coding executors, local/open models, and tool-fragile models. The implementation should avoid hardcoding exact model-specific behavior directly inside `AgentLoop` except as a temporary migration step.

The initial milestone is not to perfectly tune every known coding model. The initial milestone is to create a clean abstraction that can later support known coding models and user overrides.

## Current Relevant Architecture

The main files to inspect and modify are:

```text
src/agent/prompt.rs
src/agent/loop.rs
src/agent/router.rs
src/agent/mod.rs
src/config/schema.rs
src/provider/mod.rs
```

Relevant existing behavior:

`src/agent/prompt.rs` contains `select_provider_prompt(model_id)` and `assemble_system_prompt(...)`. `select_provider_prompt` currently chooses a static prompt based on string matching against model IDs such as GPT, Codex, Gemini, Claude, Kimi, Trinity, and default. `assemble_system_prompt` currently concatenates agent prompt, agent name/description, tools, skills, model name, config instructions, and custom instructions.

`src/config/schema.rs` already contains global model fields:

```rust
pub model: Option<String>,
pub small_model: Option<String>,
pub medium_model: Option<String>,
pub auto_route_models: Option<bool>,
```

It also contains provider config, provider model config, model variant config, and agent config. `AgentConfig` already has:

```rust
pub model: Option<String>,
pub variant: Option<String>,
pub temperature: Option<f64>,
pub top_p: Option<f64>,
pub prompt: Option<String>,
pub description: Option<String>,
pub steps: Option<u32>,
pub options: Option<HashMap<String, serde_json::Value>>,
```

`src/agent/router.rs` currently defines `ModelRouter`, which classifies tasks as `Simple`, `Medium`, or `Complex`, and routes them to `small_model`, `medium_model`, or `model`.

`src/agent/loop.rs` currently applies auto-routing, then applies agent config. This means an explicit agent model override wins over auto-routing. Preserve this behavior for now.

`src/agent/loop.rs` also has hardcoded MiniMax-specific behavior:

```rust
fn should_avoid_late_system_messages(model: &str) -> bool {
    model.to_lowercase().contains("minimax")
}
```

and a hardcoded MiniMax system prompt injection in `AgentLoop::run()`:

```rust
if model_lower.contains("minimax") {
    ...
    "Tool-use contract: For repository/file/code/doc tasks, emit structured tool calls..."
}
```

This hardcoded model behavior should be replaced by a general profile policy.

## Non-Goals for the First Milestone

Do not implement a full model benchmark system.

Do not create separate prompts for every exact model name.

Do not rewrite the provider layer.

Do not change the user-facing behavior of model routing unless necessary.

Do not remove existing provider prompt files yet.

Do not make complex regex-based model matching in the first pass. Use exact match plus simple family inference.

Do not add live network/model evals to CI.

## Target Architecture

Add a new model profile layer.

Recommended new module:

```text
src/model_profile/
    mod.rs
    types.rs
    catalog.rs
    resolve.rs
    policy.rs
```

Alternative acceptable location:

```text
src/agent/profile.rs
```

Prefer `src/model_profile/` because the profile affects more than prompting. It should eventually affect tool exposure, late-system-message behavior, context budget, and control-message strategy.

## Phase 1: Add Profile Types

Create `src/model_profile/types.rs`.

Add these initial types:

```rust
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
```

Keep the field list intentionally close to known runtime behaviors. Avoid adding speculative fields until needed.

## Phase 2: Add Config Support

Modify `src/config/schema.rs`.

Add this field to `Config`:

```rust
pub model_profile: Option<HashMap<String, ModelProfileConfig>>,
```

This requires importing the new type:

```rust
use crate::model_profile::ModelProfileConfig;
```

If that creates module dependency issues, either re-export the type from `crate::model_profile` or define the config-facing struct inside `schema.rs` and convert it later. Prefer the re-export approach if it compiles cleanly.

The config key should support exact model strings, for example:

```json
{
  "model_profile": {
    "openai/gpt-5.5": {
      "prompt_profile": "frontier_reasoning",
      "tool_call_reliability": "high",
      "instruction_adherence": "high"
    },
    "minimax/minimax-2.7": {
      "prompt_profile": "fast_executor",
      "supports_late_system_messages": false,
      "prefers_user_control_messages": true,
      "requires_explicit_tool_contract": true
    }
  }
}
```

Add validation warnings, not hard errors, for obviously invalid values where serde does not already handle them.

Validation targets:

```text
max_parallel_tools must be > 0 if present.
context_window must be >= 1000 if present.
max_output_tokens must be >= 1 if present.
preferred_tools and disabled_tools should not contain empty strings.
```

Do not validate exact tool names yet unless there is a central static list easily available without creating dependency cycles.

## Phase 3: Add Built-In Profile Resolver

Create `src/model_profile/resolve.rs`.

Implement:

```rust
pub struct ModelProfileResolver<'a> {
    config: &'a crate::config::schema::Config,
}

impl<'a> ModelProfileResolver<'a> {
    pub fn new(config: &'a crate::config::schema::Config) -> Self;

    pub fn resolve(&self, model: &str) -> ResolvedModelProfile;
}
```

Resolution order:

```text
1. Exact user override: config.model_profile[model]
2. Exact user override by model suffix after provider split
   Example: model = "openrouter/qwen/qwen3-coder"
   Check "qwen/qwen3-coder" and "qwen3-coder" if straightforward.
3. Built-in family inference.
4. Default profile.
```

Built-in family inference should be conservative and simple.

Suggested inference:

```rust
fn infer_builtin_profile(model: &str) -> ResolvedModelProfile {
    let id = model.to_lowercase();

    if id.contains("minimax") {
        return fast_executor_tool_fragile(model, "minimax");
    }

    if id.contains("gpt") || id.contains("o1") || id.contains("o3") || id.contains("o4") || id.contains("codex") {
        return frontier_reasoning(model, "openai");
    }

    if id.contains("claude") || id.contains("sonnet") || id.contains("opus") || id.contains("haiku") {
        return frontier_reasoning(model, "anthropic");
    }

    if id.contains("gemini") {
        return long_context_planner_or_frontier(model, "google");
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

    if id.contains("ollama") || id.contains("lmstudio") || id.contains("localhost") || id.contains("local") {
        return local_strict(model, "local");
    }

    default_profile(model)
}
```

Do not worry if these mappings are imperfect. The user override should always win.

Implement a helper to merge a `ModelProfileConfig` into a built-in `ResolvedModelProfile`:

```rust
fn apply_config_override(
    mut base: ResolvedModelProfile,
    cfg: &ModelProfileConfig,
) -> ResolvedModelProfile
```

Each `Option<T>` in the config should override the base only if `Some`.

Default profiles should roughly be:

### `frontier_reasoning`

```text
prompt_profile = FrontierReasoning
tool_call_reliability = High
instruction_adherence = High
patch_reliability = High
supports_late_system_messages = true
prefers_user_control_messages = false
prefers_small_patches = false
requires_explicit_tool_contract = false
requires_post_tool_continue_nudge = false
```

### `frontier_executor`

```text
prompt_profile = FrontierExecutor
tool_call_reliability = High
instruction_adherence = High
patch_reliability = High
supports_late_system_messages = true
prefers_user_control_messages = false
prefers_small_patches = false
requires_explicit_tool_contract = false
requires_post_tool_continue_nudge = false
```

### `fast_executor`

```text
prompt_profile = FastExecutor
tool_call_reliability = Medium
instruction_adherence = Medium
patch_reliability = Medium
supports_late_system_messages = true
prefers_user_control_messages = false
prefers_small_patches = true
requires_explicit_tool_contract = true
requires_post_tool_continue_nudge = true
```

### `local_strict`

```text
prompt_profile = LocalStrict
tool_call_reliability = Medium
instruction_adherence = Medium
patch_reliability = Medium
supports_late_system_messages = false
prefers_user_control_messages = true
prefers_small_patches = true
requires_explicit_tool_contract = true
requires_post_tool_continue_nudge = true
max_parallel_tools = Some(1)
```

### `tool_fragile`

```text
prompt_profile = ToolFragile
tool_call_reliability = Low
instruction_adherence = Medium
patch_reliability = Medium
supports_late_system_messages = false
prefers_user_control_messages = true
prefers_small_patches = true
requires_explicit_tool_contract = true
requires_post_tool_continue_nudge = true
max_parallel_tools = Some(1)
```

### `default`

```text
prompt_profile = Default
tool_call_reliability = Medium
instruction_adherence = Medium
patch_reliability = Medium
supports_late_system_messages = true
prefers_user_control_messages = false
prefers_small_patches = false
requires_explicit_tool_contract = false
requires_post_tool_continue_nudge = false
```

## Phase 4: Wire the Resolver into the Crate

Update `src/lib.rs` or the relevant module root to expose:

```rust
pub mod model_profile;
```

If there is no `lib.rs`, add the module declaration in the appropriate root, likely `main.rs` or a central module file depending on current structure.

Add `src/model_profile/mod.rs`:

```rust
pub mod types;
pub mod resolve;
pub mod policy;

pub use types::*;
pub use resolve::*;
```

## Phase 5: Replace MiniMax-Specific Late System Message Logic

In `src/agent/loop.rs`, find:

```rust
fn should_avoid_late_system_messages(model: &str) -> bool {
    model.to_lowercase().contains("minimax")
}
```

Replace it with profile-aware behavior.

Minimal approach:

1. Add a helper function that accepts a profile:

```rust
fn should_avoid_late_system_messages(profile: &ResolvedModelProfile) -> bool {
    !profile.supports_late_system_messages || profile.prefers_user_control_messages
}
```

2. Update `push_control_instruction`.

Current signature:

```rust
fn push_control_instruction(messages: &mut Vec<Message>, model: &str, content: &str)
```

Change to:

```rust
fn push_control_instruction(
    messages: &mut Vec<Message>,
    profile: &ResolvedModelProfile,
    content: &str,
)
```

Behavior:

```rust
if should_avoid_late_system_messages(profile) {
    if let Some(Message::System { content: system_content }) = messages.first_mut() {
        let merged = format!("{system_content}\n\n{content}");
        *system_content = merged.into();
        return;
    }

    messages.push(Message::User {
        content: vec![ContentPart::Text {
            text: format!("Instruction: {content}").into(),
        }],
    });
    return;
}

messages.push(Message::System {
    content: content.to_string().into(),
});
```

3. Because `push_control_instruction` is called inside the loop, `AgentLoop::run()` needs access to the resolved profile.

Inside `run()`, after:

```rust
self.apply_auto_routing(&mut request);
self.apply_agent_config(&mut request);
```

add:

```rust
let model_profile = crate::model_profile::ModelProfileResolver::new(&self.config)
    .resolve(&request.model);
```

Then replace calls like:

```rust
push_control_instruction(&mut request.messages, &request.model, &system);
```

with:

```rust
push_control_instruction(&mut request.messages, &model_profile, &system);
```

Also replace other calls that pass `&request.model`.

Important: resolve the profile after both auto-routing and agent config, because agent config currently overrides the model after auto-routing.

## Phase 6: Replace Hardcoded MiniMax Tool Contract Injection

In `AgentLoop::run()`, replace:

```rust
let model_lower = request.model.to_lowercase();
if model_lower.contains("minimax") {
    if let Some(Message::System { content }) = request.messages.first_mut() {
        let merged = format!(
            "{}\n\nTool-use contract: For repository/file/code/doc tasks, emit structured tool calls before giving conclusions. Do not only describe intended tool use in plain text.",
            content
        );
        *content = merged.into();
    }
}
```

with profile policy.

Add a function in `src/model_profile/policy.rs`:

```rust
use crate::provider::{ContentPart, Message};
use crate::model_profile::ResolvedModelProfile;

pub fn apply_startup_profile_policy(
    messages: &mut Vec<Message>,
    profile: &ResolvedModelProfile,
) {
    if profile.requires_explicit_tool_contract {
        inject_tool_contract(messages, profile);
    }

    if profile.prefers_small_patches {
        inject_small_patch_contract(messages, profile);
    }
}

fn inject_tool_contract(messages: &mut Vec<Message>, profile: &ResolvedModelProfile) {
    let contract = "Tool-use contract: For repository/file/code/doc tasks, emit structured tool calls before giving conclusions. Do not only describe intended tool use in plain text. If tools are available and the task requires repository knowledge, inspect the repository with tools before finalizing.";

    inject_control_text(messages, profile, contract);
}

fn inject_small_patch_contract(messages: &mut Vec<Message>, profile: &ResolvedModelProfile) {
    let contract = "Patch discipline: Prefer small, targeted edits. Do not rewrite unrelated files. Inspect the relevant file region before editing when possible.";

    inject_control_text(messages, profile, contract);
}

fn inject_control_text(
    messages: &mut Vec<Message>,
    profile: &ResolvedModelProfile,
    text: &str,
) {
    if let Some(Message::System { content }) = messages.first_mut() {
        let merged = format!("{content}\n\n{text}");
        *content = merged.into();
        return;
    }

    if profile.prefers_user_control_messages {
        messages.insert(
            0,
            Message::User {
                content: vec![ContentPart::Text {
                    text: format!("Instruction: {text}").into(),
                }],
            },
        );
    } else {
        messages.insert(
            0,
            Message::System {
                content: text.to_string().into(),
            },
        );
    }
}
```

Then in `AgentLoop::run()`:

```rust
let model_profile = ModelProfileResolver::new(&self.config).resolve(&request.model);

crate::model_profile::policy::apply_startup_profile_policy(
    &mut request.messages,
    &model_profile,
);
```

Do this after request.system insertion, so there is likely already a system message.

Current order should become:

```rust
self.apply_auto_routing(&mut request);
self.apply_agent_config(&mut request);

let model_profile = ModelProfileResolver::new(&self.config).resolve(&request.model);

if let Some(system) = request.system.take() {
    request.messages.insert(0, Message::System { content: system.into() });
}

crate::model_profile::policy::apply_startup_profile_policy(
    &mut request.messages,
    &model_profile,
);

request.tools = Some(self.build_tool_definitions().await);
```

## Phase 7: Apply Profile Defaults to ChatRequest

Add helper in `AgentLoop`:

```rust
fn apply_model_profile_defaults(
    &self,
    request: &mut ChatRequest,
    profile: &ResolvedModelProfile,
) {
    if request.reasoning_effort.is_none() {
        request.reasoning_effort = profile.default_reasoning_effort.clone();
    }

    if request.thinking_budget.is_none() {
        request.thinking_budget = profile.default_thinking_budget;
    }
}
```

Call it after `apply_agent_config`, but before streaming:

```rust
self.apply_auto_routing(&mut request);
self.apply_agent_config(&mut request);

let model_profile = ModelProfileResolver::new(&self.config).resolve(&request.model);
self.apply_model_profile_defaults(&mut request, &model_profile);
```

Agent config should still override profile defaults.

## Phase 8: Add Prompt Fragment Composition

This phase can be done after the runtime policy cleanup. Keep it small.

Modify `src/agent/prompt.rs`.

Add:

```rust
use crate::model_profile::{PromptProfileKind, ResolvedModelProfile};

pub struct PromptContext<'a> {
    pub agent: &'a Agent,
    pub config: &'a Config,
    pub model_profile: &'a ResolvedModelProfile,
    pub tools: &'a [String],
    pub skills: &'a [String],
    pub custom_instructions: Option<&'a str>,
}

pub fn assemble_system_prompt_with_profile(ctx: PromptContext<'_>) -> String {
    let mut parts = Vec::new();

    parts.push(base_harness_contract().to_string());
    parts.push(role_contract(ctx.agent).to_string());
    parts.push(profile_contract(ctx.model_profile).to_string());

    if let Some(prompt) = &ctx.agent.system_prompt {
        parts.push(prompt.clone());
    }

    parts.push(format!(
        "You are the {} agent. {}",
        ctx.agent.name, ctx.agent.description
    ));

    if !ctx.tools.is_empty() {
        parts.push(format!("Available tools: {}", ctx.tools.join(", ")));
    }

    if !ctx.skills.is_empty() {
        parts.push(format!("Available skills: {}", ctx.skills.join(", ")));
    }

    parts.push(format!("Using model: {}", ctx.model_profile.model));

    if let Some(instructions) = ctx.config.instructions.as_ref() {
        for instruction in instructions {
            parts.push(instruction.clone());
        }
    }

    if let Some(instructions) = ctx.custom_instructions {
        parts.push(instructions.to_string());
    }

    parts
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}
```

Add helper functions:

```rust
fn base_harness_contract() -> &'static str {
    "You are operating inside codegg, a coding agent harness. Use tools to inspect the repository before making claims about files, code, or project structure. Do not claim tests passed unless tool output confirms the test result. Prefer minimal, correct changes over broad rewrites."
}

fn role_contract(agent: &Agent) -> &'static str {
    match agent.name.as_str() {
        "plan" => "Role contract: You are a planning agent. Analyze the repository and produce an implementation plan. Do not modify files.",
        "explore" => "Role contract: You are an exploration agent. Inspect and explain repository structure. Do not modify files.",
        "summary" => "Role contract: You are a summarization agent. Preserve decisions, state, changed files, remaining risks, and next actions.",
        "compaction" => "Role contract: You are a context compaction agent. Preserve task state, decisions, file paths, tool results, and unresolved issues.",
        _ => "Role contract: You are an implementation agent. Inspect relevant files, make targeted changes, and verify them when possible.",
    }
}

fn profile_contract(profile: &ResolvedModelProfile) -> &'static str {
    match profile.prompt_profile {
        PromptProfileKind::FrontierReasoning => {
            "Model profile: Strong reasoning model. Use concise planning, then execute. Avoid unnecessary verbosity."
        }
        PromptProfileKind::FrontierExecutor => {
            "Model profile: Strong coding executor. Prefer direct repository inspection, targeted edits, and verification."
        }
        PromptProfileKind::FastExecutor => {
            "Model profile: Fast executor. Keep changes bounded. Use tools explicitly. Avoid speculative broad refactors."
        }
        PromptProfileKind::LocalStrict => {
            "Model profile: Strict local/open model mode. Use one step at a time. Prefer small patches. Do not infer file contents without reading them."
        }
        PromptProfileKind::ToolFragile => {
            "Model profile: Tool-fragile mode. Use structured tool calls exactly. Do not describe tool calls in prose when a tool call is required."
        }
        PromptProfileKind::LongContextPlanner => {
            "Model profile: Long-context planning mode. Synthesize repository context carefully. Separate facts from recommendations."
        }
        PromptProfileKind::Reviewer => {
            "Model profile: Review mode. Look for correctness, safety, regression risk, missing tests, and excessive scope."
        }
        PromptProfileKind::Summarizer => {
            "Model profile: Summarizer mode. Preserve relevant state densely and avoid adding unsupported claims."
        }
        PromptProfileKind::Default => {
            "Model profile: Default coding model. Use tools for repository facts and keep edits targeted."
        }
    }
}
```

Keep the existing `assemble_system_prompt()` function for compatibility. It can call the new function with a default resolved profile later, but do not force all call sites to migrate in one PR if that creates churn.

## Phase 9: Optional Agent Role Field

This can be deferred if time is limited.

Add to `AgentConfig`:

```rust
pub role: Option<String>,
```

Add to `Agent`:

```rust
pub role: Option<String>,
```

Default built-in roles:

```text
build -> executor
plan -> planner
general -> executor
explore -> explorer
title -> title
summary -> summarizer
compaction -> compactor
```

Use `role` in `role_contract()` instead of matching on agent name.

If this becomes too invasive, skip this phase. Name-based role mapping is acceptable for v0.

## Phase 10: Profile-Aware Tool Policy

This can be deferred until after prompt/profile basics compile.

Goal: allow a model profile to reduce tool exposure for tool-fragile or local models.

Current tool building happens in `AgentLoop::build_tool_definitions()` and already considers model and plan mode.

Minimal approach:

1. Resolve profile before building tools.
2. Pass profile to a new helper that filters tool definitions after existing filtering.

Example helper:

```rust
fn filter_tool_definitions_for_profile(
    defs: Vec<crate::provider::ToolDefinition>,
    profile: &ResolvedModelProfile,
) -> Vec<crate::provider::ToolDefinition> {
    let mut defs = defs;

    if let Some(disabled) = &profile.disabled_tools {
        defs.retain(|d| !disabled.iter().any(|name| name == &d.name));
    }

    if let Some(preferred) = &profile.preferred_tools {
        defs.retain(|d| preferred.iter().any(|name| name == &d.name));
    }

    defs
}
```

Do not make this authoritative yet if it causes behavior surprises. It is acceptable to only support `disabled_tools` in v0.

## Phase 11: Tests

Add tests before or alongside implementation. Minimum useful tests:

### `tests/model_profile.rs`

Test exact config override:

```rust
#[test]
fn exact_model_profile_override_wins() {
    ...
}
```

Expected behavior:

```text
model_profile["minimax/minimax-2.7"].supports_late_system_messages = true
```

should override the built-in MiniMax default of `false`.

Test MiniMax inference:

```text
minimax/minimax-2.7 resolves to:
prompt_profile = FastExecutor
requires_explicit_tool_contract = true
supports_late_system_messages = false
```

Test unknown model:

```text
some-provider/some-model resolves to Default
```

Test local inference:

```text
ollama/qwen2.5-coder resolves to LocalStrict or local/open profile
```

### Prompt assembly tests

Add tests in `src/agent/prompt.rs` test module or `tests/prompt_profile.rs`.

Test that `LocalStrict` prompt contains:

```text
Prefer small patches
Do not infer file contents without reading them
```

Test that `ToolFragile` prompt contains:

```text
Use structured tool calls exactly
```

Test that user/config instructions are still included.

### Runtime policy tests

Test `apply_startup_profile_policy()`.

Cases:

1. Existing system message + explicit tool contract required.
   Expected: system message is modified and contains tool contract.

2. No system message + prefers user control messages.
   Expected: first message is `Message::User` with `Instruction: ...`.

3. Profile does not require explicit tool contract.
   Expected: messages unchanged.

### Router compatibility tests

Existing router tests should continue passing.

Do not rewrite router tests unless the router API changes.

## Suggested Work Order for a Smaller Model

Use this exact order to reduce risk:

```text
1. Add model_profile module and types.
2. Add Config.model_profile field.
3. Add resolver with built-in defaults and override merging.
4. Add tests for resolver.
5. Replace should_avoid_late_system_messages with profile-based logic.
6. Replace hardcoded MiniMax startup prompt injection with profile policy.
7. Add tests for startup profile policy.
8. Add prompt-profile-aware assembly function without removing the old one.
9. Add prompt assembly tests.
10. Optionally wire new prompt assembly into the request creation path if call sites are clear.
11. Optionally add profile-aware tool filtering.
```

If stuck, stop after step 7. Steps 1–7 are the most important because they remove hardcoded model behavior and establish the model profile abstraction.

## Acceptance Criteria for v0

The implementation is acceptable when:

```text
cargo fmt passes.
cargo test passes.
Existing model routing behavior is preserved.
MiniMax behavior is no longer hardcoded directly by string matching in AgentLoop startup prompt injection.
Late system-message behavior is controlled by ResolvedModelProfile.
A user can configure model_profile overrides in config.
Unknown models still work via a default profile.
Prompt-profile-aware assembly exists and is covered by tests.
Existing provider-specific prompt files are not removed.
```

## Example Config After Implementation

```json
{
  "model": "openai/gpt-5.5",
  "small_model": "minimax/minimax-2.7",
  "medium_model": "deepseek/deepseek-v4",
  "auto_route_models": true,

  "model_profile": {
    "openai/gpt-5.5": {
      "prompt_profile": "frontier_reasoning",
      "tool_call_reliability": "high",
      "instruction_adherence": "high",
      "patch_reliability": "high"
    },

    "minimax/minimax-2.7": {
      "prompt_profile": "fast_executor",
      "tool_call_reliability": "medium",
      "instruction_adherence": "medium",
      "patch_reliability": "medium",
      "supports_late_system_messages": false,
      "prefers_user_control_messages": true,
      "prefers_small_patches": true,
      "requires_explicit_tool_contract": true,
      "requires_post_tool_continue_nudge": true
    },

    "ollama/qwen2.5-coder:32b": {
      "prompt_profile": "local_strict",
      "supports_late_system_messages": false,
      "prefers_user_control_messages": true,
      "requires_explicit_tool_contract": true,
      "prefers_small_patches": true,
      "max_parallel_tools": 1,
      "disabled_tools": ["batch", "multiedit"]
    }
  }
}
```

## Design Notes

This initiative should not become a catalog of one-off prompts for exact model names. The durable abstraction is:

```text
role × capability profile
```

not:

```text
exact model × provider × variant × endpoint
```

Exact model overrides should exist only as config entries that choose or tweak a profile.

Good examples of profile classes:

```text
frontier_reasoning
frontier_executor
fast_executor
local_strict
tool_fragile
long_context_planner
reviewer
summarizer
default
```

Keep model-specific behavior small, observable, and easy to override.

## Migration Notes

Existing functions should not be removed immediately:

```rust
select_provider_prompt(model_id)
assemble_system_prompt(...)
```

Keep them for compatibility. Add profile-aware alternatives first. Once the new path is clearly wired and tested, old functions can be simplified or made wrappers.

The hardcoded MiniMax behavior should be migrated first because it is the clearest current example of model-specific runtime policy leaking into the main agent loop.

## Likely Pitfalls

Do not resolve the model profile before `apply_agent_config()`, because the agent can override the model after auto-routing.

Do not make profile fields mandatory in user config.

Do not panic on unknown model profiles.

Do not make `disabled_tools` remove critical tools unexpectedly unless the user explicitly configured it or the built-in profile is very conservative.

Do not change `AgentMode`; it currently means `primary`, `subagent`, or `all`, not planner/executor/reviewer. If adding cognitive roles, use a separate `role` field.

Do not overfit the built-in model inference. User override is the safety valve.

Do not require live provider calls in tests.

## Final Deliverable

A successful first implementation should produce:

```text
src/model_profile/mod.rs
src/model_profile/types.rs
src/model_profile/resolve.rs
src/model_profile/policy.rs

Updated:
src/config/schema.rs
src/agent/loop.rs
src/agent/prompt.rs

Tests:
tests/model_profile.rs
tests/prompt_profile.rs
or equivalent module-level tests
```

The v0 should be small enough that future work can add richer prompt fragments, role fields, model evals, and profile-aware tool exposure without revisiting the basic architecture.

