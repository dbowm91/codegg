# Model Profile & Task State

These two `codegg-core` modules form a coupled subsystem: `model_profile` resolves model-specific behavioral parameters, and `task_state` manages the todo/task list that the agent uses to track progress. The `TaskStatePolicy` on each model profile controls how the task state system behaves.

## Model Profile (`crates/codegg-core/src/model_profile/`)

### Purpose

Each LLM model has different capabilities and quirks. The model profile system resolves a `ResolvedModelProfile` for any model ID, providing:

- Prompt profile selection (how to format system prompts)
- Reliability tiers for tool calling, instruction adherence, and patching
- Context window and output token limits
- Behavioral flags (late system messages, small patches, explicit tool contracts)
- Task state policy (how todos are managed for this model)

### Resolution (`resolve.rs`)

```
Model ID → infer_builtin_profile() → ResolvedModelProfile
                │
                ▼
    find_config_override() → apply_config_override()
```

1. **Builtin inference** (`infer_builtin_profile`): Pattern-matches model ID strings to determine family and prompt profile:
   - `gpt`, `o1`, `o3`, `o4`, `codex` → `FrontierReasoning` (OpenAI family)
   - `claude`, `sonnet`, `opus`, `haiku` → `FrontierReasoning` (Anthropic family)
   - `gemini` → `LongContextPlanner` (Google family, 512K context)
   - `deepseek` → `FrontierExecutor`
   - `qwen`, `qwq` → `LocalStrict` (32K context, tool-fragile)
   - `minimax` → `FastExecutor` (tool-fragile, guided task state)
   - `ollama`, `lmstudio`, `localhost` → `LocalStrict`
   - Unknown → `Default` profile

2. **Config override** (`apply_config_override`): Merges `[model_profile.<model>]` config entries over the builtin profile. Supports suffix matching (e.g., config key `qwen3-coder` matches model `openrouter/qwen/qwen3-coder`).

### ResolvedModelProfile (`types.rs`)

~25 fields controlling model behavior:

| Field | Purpose |
|-------|---------|
| `model` | Model identifier string |
| `prompt_profile` | `FrontierReasoning`, `FrontierExecutor`, `FastExecutor`, `LongContextPlanner`, `LocalStrict`, `Default` |
| `family` | Model family (openai, anthropic, google, deepseek, qwen, minimax, local, default) |
| `context_window` | Max context tokens (32K–512K) |
| `max_output_tokens` | Max output tokens (4K–16K) |
| `tool_call_reliability` | `High` / `Medium` — affects retry behavior |
| `instruction_adherence` | `High` / `Medium` — affects prompt complexity |
| `patch_reliability` | `High` / `Medium` — affects patch auto-apply |
| `supports_late_system_messages` | Whether model handles system messages after user messages |
| `prefers_user_control_messages` | Whether model works better with user-role control messages |
| `prefers_small_patches` | Whether to break large edits into smaller patches |
| `requires_explicit_tool_contract` | Whether tool definitions need explicit schemas |
| `requires_post_tool_continue_nudge` | Whether model needs nudge to continue after tool calls |
| `default_reasoning_effort` | Optional reasoning effort level |
| `default_thinking_budget` | Optional thinking token budget |
| `max_parallel_tools` | Max concurrent tool calls (1–8) |
| `preferred_tools` | Optional tool preference list |
| `disabled_tools` | Optional tool exclusion list |
| `task_state_policy` | How todos behave for this model |

### Prompt Profiles

| Profile | Context Window | Output Tokens | Tool Reliability | Parallel Tools |
|---------|---------------|---------------|------------------|----------------|
| `FrontierReasoning` | 128K | 16K | High | unlimited |
| `FrontierExecutor` | 128K | 16K | High | unlimited |
| `LongContextPlanner` | 512K | 16K | High | 8 |
| `FastExecutor` | 128K | 8K | Medium | 2 |
| `LocalStrict` | 32K | 4K | Medium | 1 |
| `Default` | 128K | 8K | Medium | unlimited |

## Task State (`crates/codegg-core/src/task_state/`)

### Purpose

Manages the agent's todo/task list — a structured representation of work items that the model can read and update during a session. The task state is injected into the model context as a compact projection, keeping the agent oriented toward its current goals.

### TodoState (`mod.rs`)

```rust
pub struct TodoState {
    pub items: Vec<TodoItem>,
    pub revision: u64,
    pub reminder_pending: bool,
    pub tool_calls_since_injection: usize,
}
```

- `revision` — Monotonically increasing, incremented on each `replace_from_model()`
- `reminder_pending` — True when unfinished items exist and the reminder hasn't been injected yet
- `tool_calls_since_injection` — Counter for rate-limited injection

### TodoItem

```rust
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,    // Pending, InInProgress, Completed, Blocked, Cancelled
    pub priority: TodoPriority, // Low, Medium, High
    pub blocker: Option<String>,
}
```

### Todo Modes

Controlled by `TaskStatePolicy.mode`:

| Mode | Description | Model writes? | Max items |
|------|-------------|---------------|-----------|
| `Disabled` | No task state injection | No | 0 |
| `SparsePlan` | Single-line status summary | Yes | 8 |
| `ExplicitTodo` | Full bullet list with status | Yes | 10 |
| `GuidedCurrentTask` | "Current task: X. Do this task only." | No | 4 |

### TaskStatePolicy (`types.rs`)

Controls injection frequency, permissions, and constraints:

```rust
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

#### Preset Policies

| Preset | Mode | Max Items | Write? | Inject After |
|--------|------|-----------|--------|--------------|
| `sparse_plan()` | SparsePlan | 8 | Yes | 10 tool calls |
| `explicit_todo()` (default) | ExplicitTodo | 10 | Yes | 5 tool calls |
| `guided_current_task()` | GuidedCurrentTask | 4 | No | 3 tool calls |
| `disabled()` | Disabled | 0 | No | Never |

#### Injection Timing

- `inject_after_tool_calls` — Rate-limited: inject reminder after N tool calls since last injection
- `inject_on_resume` — Inject when session resumes
- `inject_after_compaction` — Inject after context compaction

#### Validation Rules

- `Disabled` mode forces: `allow_model_todo_read/write = false`, `max_total_items = 0`
- `GuidedCurrentTask` forces: `allow_model_todo_write = false`, `max_total_items = min(4, configured)`
- `max_total_items` capped at 12

### State Transitions

`replace_from_model()` validates against the policy before accepting:

1. Rejects if mode is `Disabled` (`TodoStateError::ModeDisabled`)
2. Rejects if `allow_model_todo_write = false` (`TodoStateError::WriteNotAllowed`)
3. Rejects if items exceed `max_total_items` (`TodoStateError::TooManyItems`)
4. Rejects multiple in-progress items if `require_single_in_progress` (`TodoStateError::MultipleInProgress`)
5. Rejects blocked items without blocker reason if `require_blocker_reason` (`TodoStateError::MissingBlockerReason`)

### Projections

- **`compact_projection(policy)`** — Model-facing compact text injected into context. Format depends on mode:
  - `SparsePlan`: "Active task state: in_progress: X; pending: Y."
  - `ExplicitTodo`: "- in_progress: X\n- pending: Y\nContinue from the in-progress item..."
  - `GuidedCurrentTask`: "Current task: X. Do this task only."
- **`full_projection_for_user()`** — User-facing full list with numbers, status, and priority

### Integration

- `build_todo_reminder()` is called by the agent loop to determine if a todo reminder should be injected
- `TodoState` is persisted via `TodoItemInput` / `TodoItem` session models
- `AppEvent::TodoUpdated` broadcasts todo snapshots to the TUI
- The task state policy is resolved per-model via `ResolvedModelProfile.task_state_policy`
