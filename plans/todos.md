# Codegg implementation plan: model-profile-aware todo/task-state policy

Audience: smaller implementation model (MiMo v2.5 or similar). This plan is written to be executed in the current `dbowm91/codegg` Rust codebase.

Goal: replace the current one-size-fits-all `todowrite` behavior with a model-profile-aware task-state policy. Strong/frontier models should get sparse Codex-like planning. Mid-tier models should get explicit OpenCode/Claude-like todo state. Local/tool-fragile models should get compact current-task guidance and fewer chances to corrupt task state. Unknown models should use a conservative default.

Non-goal: do not build a full project-management system or durable `/goal` replacement. This feature is session-local execution state. It should not become long-term project memory.

## Current repo anchors

The repo already has model-profile infrastructure under `src/model_profile/`. `src/model_profile/types.rs` defines `PromptProfileKind`, `ReliabilityTier`, `ModelProfileConfig`, and `ResolvedModelProfile`. `src/model_profile/resolve.rs` already infers built-in profiles for OpenAI/Codex, Anthropic/Claude, Gemini, DeepSeek, Qwen, Kimi, Minimax, and local endpoints. `src/model_profile/policy.rs` already contains prompt/control-message injection helpers such as `apply_startup_profile_policy` and `push_control_instruction`.

The repo already has a basic `todowrite` tool in `src/tool/todo.rs`. It stores `TodoItem { content, status, priority }` in an in-memory `Arc<Mutex<TodoStore>>`, replaces the whole list on every call, supports only `pending`, `in_progress`, and `completed`, and does not enforce invariants such as a single in-progress item. The tool is registered in `src/tool/mod.rs` by default. There is not currently a separate `todoread` tool.

The session layer already has persisted todo-related structs. `src/session/models.rs` defines `TodoItem` and `TodoItemInput`, and `src/session/mod.rs` re-exports `TodoStore` from `src/session/store.rs`. This means persistent session todos appear partially implemented already. Prefer using or extending this rather than creating an unrelated second storage model.

The agent system already treats subagent todo mutation as special. In `src/agent/mod.rs`, the built-in `general` subagent is described as "Subagent without todo management" and denies `todowrite`. Preserve this principle: the root/manager agent owns the global task ledger; ordinary subagents should not mutate it.

The agent loop is in `src/agent/loop.rs`. It already imports `push_control_instruction` from `model_profile::policy`, manages tools through `ToolRegistry`, tracks tool timeout config including `todo`, and performs compaction/tool-loop orchestration. This is the most likely integration point for active todo reminder injection.

## Desired behavior

Implement a `TaskStatePolicy` chosen from the resolved model profile. The policy determines whether todos are disabled, sparse, explicit, or harness-guided; whether the model can read and/or write todos; max list size; whether completed items are included in model-facing reminders; reminder cadence after tool calls; and subagent access.

Use four presets:

1. `SparsePlan`: for strong/frontier models such as OpenAI/Codex and Claude-class models. The model may use todos, but should use them only for non-trivial multi-step tasks and update them at meaningful milestones. Reminder injection is rare.

2. `ExplicitTodo`: for mid-tier models. The model has `todowrite` and `todoread`, gets explicit but compact task state after boundary events, and reminders after moderate tool loops.

3. `GuidedCurrentTask`: for local/tool-fragile models. The model sees only the active item and perhaps the next item. It should not freely rewrite the full todo list by default. It should follow harness-provided current-task reminders.

4. `Disabled`: for trivial/non-agentic flows or explicit user/config opt-out.

Unknown models should default to `ExplicitTodo`, not `SparsePlan`, because assuming frontier-level self-management for unknown OpenAI-compatible endpoints is unsafe.

## Phase 1: add task policy types to model profiles

Edit `src/model_profile/types.rs`.

Add these enums and config structs. Keep serde `snake_case`.

```rust
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
```

Implement `Default` for `TaskStatePolicyConfig` as all `None`. Implement associated constructors on `TaskStatePolicy`:

```rust
impl TaskStatePolicy {
    pub fn sparse_plan() -> Self { ... }
    pub fn explicit_todo() -> Self { ... }
    pub fn guided_current_task() -> Self { ... }
    pub fn disabled() -> Self { ... }

    pub fn apply_config(mut self, cfg: &TaskStatePolicyConfig) -> Self { ... }

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
```

Add these fields:

```rust
// in ModelProfileConfig
pub task_state_policy: Option<TaskStatePolicyConfig>,

// in ResolvedModelProfile
pub task_state_policy: TaskStatePolicy,
```

Update defaults and every constructor in `resolve.rs` so all `ResolvedModelProfile` values include `task_state_policy`.

Suggested preset mapping:

- `frontier_reasoning` and `frontier_executor`: `TaskStatePolicy::sparse_plan()`.
- `long_context_planner`: `TaskStatePolicy::sparse_plan()` or `explicit_todo()`; choose `sparse_plan()` if you want lower context pollution for Gemini-class long-context models.
- `fast_executor_tool_fragile`: `TaskStatePolicy::explicit_todo()`.
- `local_or_open_executor` and `local_strict`: `TaskStatePolicy::guided_current_task()`.
- `default_profile`: `TaskStatePolicy::explicit_todo()`.

Update `apply_config_override` in `src/model_profile/resolve.rs` to apply `task_state_policy` if present.

Add tests in `resolve.rs`:

- OpenAI/Codex model resolves to `SparsePlan`.
- Unknown model resolves to `ExplicitTodo`.
- Ollama/local model resolves to `GuidedCurrentTask` and `allow_model_todo_write == false`.
- Config override can set `task_state_policy.mode = disabled` and validation disables read/write.

## Phase 2: split todo state from the tool implementation

Create a new module `src/todo/` or `src/task_state/`. Prefer `src/task_state/` to avoid confusion with the provider-facing tool name.

Add `src/task_state/mod.rs` and export it from `src/lib.rs`.

Define canonical session-local state. Do not rely only on the existing tool-local `Arc<Mutex<TodoStore>>` because that makes state hard to persist, read, inject, or share with the TUI.

Minimum implementation:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
    pub priority: TodoPriority,
    pub blocker: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoPriority { Low, Medium, High }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TodoState {
    pub items: Vec<TodoItem>,
    pub revision: u64,
    pub reminder_pending: bool,
    pub tool_calls_since_injection: usize,
}
```

Implement methods:

```rust
impl TodoState {
    pub fn replace_from_model(&mut self, items: Vec<TodoItem>, policy: &TaskStatePolicy) -> Result<(), TodoStateError>;
    pub fn active_item(&self) -> Option<&TodoItem>;
    pub fn unfinished_items(&self) -> impl Iterator<Item = &TodoItem>;
    pub fn is_all_done(&self) -> bool;
    pub fn compact_projection(&self, policy: &TaskStatePolicy) -> Option<String>;
    pub fn full_projection_for_user(&self) -> String;
}
```

Validation rules in `replace_from_model`:

- If policy mode is `Disabled`, reject with a clear error.
- If `allow_model_todo_write == false`, reject with a clear error.
- Truncate or reject lists longer than `policy.max_total_items`. Prefer reject, because silent truncation can corrupt task state.
- If `require_single_in_progress`, reject more than one `in_progress` item.
- If `require_blocker_reason`, reject `blocked` items without a non-empty `blocker`.
- Empty list is allowed and clears state.
- Increment `revision` on successful mutation.
- Set `reminder_pending = true` after successful mutation unless all items are complete/cancelled.

Add unit tests for all invariants.

## Phase 3: replace `todowrite` and add `todoread`

Edit `src/tool/todo.rs`.

Refactor it so `TodoTool` wraps shared `Arc<Mutex<TodoState>>` plus a `TaskStatePolicy`, not the current local-only `TodoStore`.

Add two tools:

- `TodoWriteTool`, name `todowrite`, mutates state using `TodoState::replace_from_model`.
- `TodoReadTool`, name `todoread`, returns `TodoState::compact_projection(policy)` or `No active todos.`.

Keep backwards compatibility for the input schema of `todowrite` where possible: `todos: [{ content, status, priority }]`. Add optional `id` and optional `blocker`. If `id` is missing, derive a stable-ish ID from position/content for now or generate a UUID. Simpler for MiMo: generate `uuid::Uuid::new_v4().to_string()` when missing.

Schema changes:

- Add `blocked` and `cancelled` to `status` enum.
- Add `blocker` optional string.
- Keep `priority` optional default `medium`.

If model policy is `GuidedCurrentTask`, `todowrite` should return an error like:

```text
Error: this model profile is configured for harness-guided todos. Use todoread and follow the active task.
```

`todoread` should be read-only and allowed even when write is disabled, unless `allow_model_todo_read == false`.

Update `src/tool/mod.rs` registration. The current registry uses `TodoTool::default()` and stores tools in a static default registry. Because todo state and model policy are session-specific, do not rely only on `ToolRegistry::with_defaults()` for agent sessions. Add a registry constructor that accepts session-specific state/policy, for example:

```rust
impl ToolRegistry {
    pub fn with_session_defaults(todo_state: Arc<Mutex<TodoState>>, policy: &TaskStatePolicy) -> Self { ... }
}
```

In `with_session_defaults`, register `todowrite` only when `policy.allow_model_todo_write` is true and mode is not disabled. Register `todoread` when `policy.allow_model_todo_read` is true. Keep the old `with_defaults()` behavior for tests/simple use, but have it use `TaskStatePolicy::explicit_todo()` and a fresh `TodoState`.

Add tests in `src/tool/todo.rs`:

- `todowrite` accepts a valid list and `todoread` returns the active item.
- `todowrite` rejects two `in_progress` items.
- `todowrite` rejects blocked without blocker.
- `todowrite` rejects writes under `GuidedCurrentTask`.
- `todoread` returns compact output without completed items when policy exposure is `NoneUnlessAsked`.

## Phase 4: wire policy/state into the agent loop

Find where `AgentLoop` is constructed and where `ToolRegistry::with_defaults()` or `default_registry()` is used for interactive sessions. Likely files: `src/agent/loop.rs`, `src/agent/processor.rs`, `src/core/`, or TUI session launch code. Replace session agent construction so each `AgentLoop` gets:

```rust
Arc<Mutex<TodoState>>
TaskStatePolicy
```

The `TaskStatePolicy` should come from:

1. Resolve active model with `ModelProfileResolver::new(&config).resolve(model_id)`.
2. Read `profile.task_state_policy`.
3. Apply any user config override already handled by profile resolution.
4. Use the resulting policy to construct the session tool registry.

Do not pass the same global todo state to independent sessions. Todo state is per session.

Add an `AgentLoop` field:

```rust
pub todo_state: Arc<Mutex<TodoState>>,
pub task_state_policy: TaskStatePolicy,
```

If `AgentLoopState` exists and is more appropriate, store runtime counters there, but canonical todo items should live in `TodoState`.

## Phase 5: compact active reminder injection

Implement a helper in `src/task_state/mod.rs` or `src/model_profile/policy.rs`:

```rust
pub fn build_todo_reminder(todo: &TodoState, policy: &TaskStatePolicy) -> Option<String>
```

Output should be short. Examples:

For `SparsePlan`:

```text
Active task state: in_progress: Refactor provider adapter error handling; pending: Add tests; pending: Run cargo test. Continue from the active item unless the user changes direction.
```

For `ExplicitTodo`:

```text
Active todo state:
- in_progress: Refactor provider adapter error handling
- pending: Add tests
- pending: Run cargo test
Continue from the in-progress item unless the user changes direction.
```

For `GuidedCurrentTask`:

```text
Current task: Refactor provider adapter error handling. Do this task only. Report a blocker if unable to continue.
Next task: Add tests.
```

Injection rules:

- Inject on a new user turn when unfinished todos exist and `policy.inject_on_resume` or `todo.reminder_pending` is true.
- Inject after compaction when unfinished todos exist and `policy.inject_after_compaction` is true.
- Inject after `policy.inject_after_tool_calls` tool calls without a todo update.
- Do not inject on every loop iteration.
- After injection, set `reminder_pending = false` and reset `tool_calls_since_injection = 0`.
- Use existing `push_control_instruction(messages, profile, content)` so models that dislike late system messages get a merged system/user-control instruction rather than a late system message.

Implementation location: in `src/agent/loop.rs`, immediately before building/sending each `ChatRequest` is the best place. The code already imports `push_control_instruction`, so use that rather than duplicating late-system-message logic.

Also increment `tool_calls_since_injection` after non-todo tool results. Reset it after `todowrite`.

Add tests if there are existing agent-loop tests. If agent-loop tests are too heavy, test the pure helper functions and add one lightweight integration test around message vector injection.

## Phase 6: prompt profile text for todo policy

Extend `apply_startup_profile_policy` in `src/model_profile/policy.rs` to inject a short todo discipline instruction depending on `profile.task_state_policy.mode`.

Suggested text:

SparsePlan:

```text
Task planning: Use todos only for non-trivial multi-step work. Keep the list short. Maintain exactly one in-progress item. Update it at meaningful milestones, not after every minor read.
```

ExplicitTodo:

```text
Task planning: For multi-step coding work, keep a short todo list. Keep exactly one item in_progress. Mark items completed only after verification. Update the list when task direction changes.
```

GuidedCurrentTask:

```text
Task planning: Follow the active task reminder. Do not create or rewrite the global todo list unless explicitly allowed. Complete the current task, report blockers, then proceed.
```

Disabled: inject nothing.

Do not make these prompts long. The purpose is to avoid context pollution.

Add tests in `policy.rs` showing the expected instruction is injected for local/unknown/frontier profiles.

## Phase 7: persistence/resume behavior

Use the existing session todo store if practical. Search in `src/session/store.rs` for todo methods. If a `TodoStore` already supports replace/list by session, use it. Otherwise add methods:

```rust
impl TodoStore {
    pub async fn replace_for_session(&self, session_id: &str, items: &[TodoItem]) -> Result<(), StorageError>;
    pub async fn list_for_session(&self, session_id: &str) -> Result<Vec<TodoItem>, StorageError>;
}
```

Map between `task_state::TodoItem` and `session::models::TodoItemInput` or extend the session model if you add `id`/`blocker`. If schema migration is too much for the first pass, persist only content/status/priority/position using the existing schema and reconstruct IDs on load. Document this limitation with a TODO comment.

Load persisted unfinished todos when resuming a session. Save after successful `todowrite` calls. If all items are completed/cancelled, persistence should reflect completion but model-facing reminders should not continue.

Acceptance criterion: after `codegg -c` or opening a session, unfinished todos are available to `todoread` and eligible for a one-shot reminder.

## Phase 8: TUI/event rendering

Do not block this feature on a sophisticated UI. Minimal viable rendering is enough.

Search for existing app events in `src/bus/events.rs`. Add an event such as:

```rust
AppEvent::TodoUpdated { session_id: String, todos_json: String }
```

Publish this event after successful `todowrite`. If an existing event already exists for tool output or todo updates, use that.

The TUI can continue showing the tool result text initially. A later pass can render a sidebar checklist. The important part for this implementation is that the harness has canonical state and the model receives compact reminders.

## Phase 9: config surface

Because `Config` already has `model_profile: Option<HashMap<String, ModelProfileConfig>>`, avoid adding a top-level config key in the first pass. Let users override through `model_profile`:

```jsonc
{
  "model_profile": {
    "minimax/minimax-2.7": {
      "task_state_policy": {
        "mode": "explicit_todo",
        "inject_after_tool_calls": 4,
        "max_total_items": 5
      }
    },
    "ollama/qwen2.5-coder:32b": {
      "task_state_policy": {
        "mode": "guided_current_task",
        "allow_model_todo_write": false,
        "inject_after_tool_calls": 3
      }
    }
  }
}
```

Update `codegg.example.jsonc` if it exists. If not, update README config documentation briefly.

## Phase 10: test and validation checklist

Run:

```bash
cargo fmt
cargo test model_profile
cargo test todo
cargo test task_state
cargo test
```

Manual test scenarios:

1. Strong model profile, e.g. `openai/gpt-5` or `anthropic/claude-sonnet-*`: tools include `todowrite` and `todoread`; startup prompt includes sparse todo guidance; reminders are rare.

2. Unknown model: resolves to explicit todo policy; `todowrite` rejects two in-progress items.

3. Local model, e.g. `ollama/qwen2.5-coder:32b`: startup prompt says follow active task; `todowrite` is not available or returns a clear policy error; `todoread` is available if read is enabled.

4. Subagent `general`: still cannot mutate global todos. Do not regress the existing deny rule.

5. Resume session with unfinished todos: one compact reminder is injected, then not repeated every turn.

6. After compaction: unfinished todos are retained as compact state and reintroduced once.

## Implementation order for MiMo

Do this in small commits or checkpoints:

1. Add `TaskStatePolicy` types and resolver tests.
2. Add `src/task_state/mod.rs` with pure state validation and tests.
3. Refactor `src/tool/todo.rs` into `todowrite` + `todoread` using `TaskStatePolicy`.
4. Add `ToolRegistry::with_session_defaults(...)` and keep old `with_defaults()` working.
5. Wire resolved model policy into the agent loop/session construction.
6. Add compact reminder injection using `push_control_instruction`.
7. Add persistence/resume using existing session todo storage.
8. Add config docs and run full tests.

## Constraints and design cautions

Keep completed todos out of model context by default. The TUI may show them, but the model should not repeatedly receive historical completed work.

Do not let subagents write the root todo state unless a future manager-specific subagent explicitly needs it. Ordinary subagents should receive a scoped task and return findings.

Do not add nested todo trees in this pass. Flat lists with one active item are easier for weaker models and easier to validate.

Do not inject reminders on every provider request. Use one-shot boundary reminders and a tool-call threshold to avoid context pollution.

Do not silently truncate todo lists if the model exceeds `max_total_items`. Reject with a clear error so the model can correct its state.

Prefer pure functions and unit tests for policy/state logic. Agent-loop integration can be thin once state validation and reminder formatting are reliable.

