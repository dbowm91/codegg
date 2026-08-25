# Model Profile & Task State

These two `codegg-core` modules form a coupled subsystem: `model_profile`
resolves model-specific behavioral parameters via a declarative adapter
system, and `task_state` manages the todo/task list that the agent uses
to track progress. The `TaskStatePolicy` on each model profile controls
how the task state system behaves.

## Model Profile (`crates/codegg-core/src/model_profile/`)

### Purpose

Each LLM model has different capabilities and quirks. The model profile
system resolves a `ResolvedModelProfile` (and optionally a
`ResolvedModelAdapter`) for any model ID, providing:

- Prompt profile selection (how to format system prompts)
- Reliability tiers for tool calling, instruction adherence, and patching
- Context window and output token limits
- Behavioral flags (late system messages, small patches, explicit tool
  contracts)
- Task state policy (how todos are managed for this model)
- Tool format, aliases, argument aliases, and request transforms
- Recovery policies (malformed tool retry, turn limits)
- Server requirements (tool-call parser, reasoning parser)

### Resolution (`resolve.rs` + `adapter.rs`)

Resolution is a two-layer system:

```
Model ID + Provider → resolve_adapter() → ResolvedModelAdapter
                         │
          ┌──────────────┼──────────────┐
          │              │              │
   builtin TOML    generic fallback   score + merge
   definitions     (always matches)   (highest wins)
          │              │              │
          └──────────────┼──────────────┘
                         │
                    effective_profile()
                         │
                    config override
                    (ModelProfileResolver)
                         │
                    ResolvedModelProfile
```

1. **Declarative adapter matching** (`resolve_adapter`):
   - At build time, `build.rs` reads TOML files from
     `crates/codegg-core/assets/model-adapters/` and embeds them as
     `BUILTIN_ADAPTER_SOURCES`.
   - At runtime, `definitions()` lazily parses all adapter TOMLs into
     `AdapterDefinition` structs.
   - `provider_for(model)` infers a provider string from the model ID.
   - For each adapter's `[[match]]` rules, `match_score()` computes a
     numeric score based on exact model, provider, prefix/suffix, and
     regex matches.
   - The highest-scoring adapter wins; ties broken by adapter priority
     then ID lexicographic order.
   - The selected adapter is merged with the `generic` fallback adapter.
   - `effective_profile()` builds a `ResolvedModelProfile` from the
     merged adapter. Models matching `FastExecutor` or `LocalStrict`
     profiles get `TaskStatePolicy::guided_current_task()` automatically.

2. **Config override** (`ModelProfileResolver`): Merges
   `[model_profile.<model>]` config entries over the resolved adapter
   profile. Supports suffix matching (e.g., config key `qwen3-coder`
   matches model `openrouter/qwen/qwen3-coder`). Also applies
   `text_tool_repair` overrides for tool compatibility grammars.

### Built-in Adapter Definitions

Seven TOML files in `crates/codegg-core/assets/model-adapters/`:

| Adapter ID | Priority | Matches | Prompt Profile |
|------------|----------|---------|----------------|
| `generic` | -1000 | everything (fallback) | `Default` |
| `openai-frontier` | 10 | openai provider, gpt/o1/o3/o4/codex regex | `FrontierReasoning` |
| `anthropic-frontier` | 10 | anthropic provider, claude/sonnet/opus/haiku regex | `FrontierReasoning` |
| `google-long-context` | 10 | google/gemini provider, gemini regex | `LongContextPlanner` |
| `minimax-fast-executor` | 20 | minimax provider, minimax regex | `FastExecutor` |
| `local-strict` | 5 | local/ollama/lmstudio/vllm/sglang provider, qwen/qwq/deepseek/kimi regex | `LocalStrict` |
| `poolside-laguna-agentic` | 40 | local/vllm/sglang/openai/poolside provider, laguna-m/xs/s regex | `LocalStrict` |

### AdapterDefinition (built at compile time)

Each adapter TOML defines:

- **`[adapter]`**: id, version, priority, description
- **`[[match]]`**: provider list, exact_model list, model_prefix,
  model_suffix, model_regex, exclude_regex
- **`[profile]`**: prompt_profile, family, context_window,
  max_output_tokens, reliability tiers, behavioral flags
- **`[tools]`**: format, tool_choice, max_parallel,
  require_structured_calls, text_tool_repair, rename map, arguments map
- **`[prompt]`**: profile, fragments, system_role, control_role
- **`[recovery]`**: malformed_tool_retry, no_action_turn_limit,
  restore_full_palette_on_missing_tool
- **`[server_requirements]`**: tool_call_parser, reasoning_parser,
  auto_tool_choice
- **`[[transforms]]`**: closed set of request mutations (SetRequestField,
  RemoveRequestField, RenameToolArgument, SetSystemRole, SetToolChoice,
  SetMaxParallelTools, SetThinkingParameter, RequireLateSystemMessages,
  RequireContinueNudge)

### ResolvedModelAdapter (`adapter.rs`)

The full adapter result, beyond just the profile:

| Field | Purpose |
|-------|---------|
| `profile` | `ResolvedModelProfile` |
| `adapter_id` | Matched adapter identifier |
| `adapter_version` | Adapter schema version |
| `fingerprint` | SHA-256 of serialized adapter definition |
| `source_layers` | Which builtin adapters were merged |
| `tool_format` | Provider-specific tool format |
| `tool_choice` | Auto/none/required |
| `max_parallel_tools` | Max concurrent tool calls |
| `require_structured_calls` | Enforce structured tool calling |
| `text_tool_repair` | Compatibility grammar for malformed calls |
| `tool_aliases` | Canonical→wire tool name mapping |
| `argument_aliases` | Tool→argument canonical→wire mapping |
| `prompt_fragments` | Extra prompt text fragments |
| `prompt_system_role` | Override system message role |
| `prompt_control_role` | Override control message role |
| `recovery` | Retry and turn-limit policies |
| `server_requirements` | Parser requirements for serving |
| `transforms` | Request-level mutations |

### ResolvedModelProfile (`types.rs`)

~18 fields controlling model behavior:

| Field | Purpose |
|-------|---------|
| `model` | Model identifier string |
| `prompt_profile` | `FrontierReasoning`, `FrontierExecutor`, `FastExecutor`, `LongContextPlanner`, `LocalStrict`, `Default` |
| `family` | Model family string (openai, anthropic, google, etc.) |
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
| `FrontierReasoning` | 128K | 16K | High | 10 |
| `FrontierExecutor` | 128K | 16K | High | 10 |
| `LongContextPlanner` | 512K | 16K | High | 8 |
| `FastExecutor` | 128K | 8K | Medium | 2 |
| `LocalStrict` | 32K | 4K | Medium | 1 |
| `Default` | 128K | 8K | Medium | 1 |

### Policy (`policy.rs`)

`should_avoid_late_system_messages(profile)` returns true when the
model does not support late system messages or prefers user control
messages. `push_control_instruction(messages, profile, content)` deduplicates
and appends control instructions, merging into the first system message
when late system messages are avoided.

### Model-facing tools

Two tools in `src/tool/todo.rs` expose the task state to the model:

- **`todowrite`** (`TodoWriteTool`): Replaces the entire todo list.
  Validates against the policy, persists to session store, and broadcasts
  `AppEvent::TodoUpdated`. Rejects writes in `Disabled` or
  `GuidedCurrentTask` modes.
- **`todoread`** (`TodoReadTool`): Returns the compact projection of
  the current task state.

A legacy `TodoTool` wrapper exists for non-session callers (tests,
`ToolRegistry::with_defaults()`).

## Task State (`crates/codegg-core/src/task_state/`)

### Purpose

Manages the agent's todo/task list — a structured representation of work
items that the model can read and update during a session. The task state
is injected into the model context as a compact projection, keeping the
agent oriented toward its current goals.

### TodoState (`mod.rs`)

```rust
pub struct TodoState {
    pub items: Vec<TodoItem>,
    pub revision: u64,
    pub reminder_pending: bool,
    pub tool_calls_since_injection: usize,
}
```

- `revision` — Monotonically increasing, incremented on each
  `replace_from_model()`
- `reminder_pending` — True when unfinished items exist and the
  reminder hasn't been injected yet
- `tool_calls_since_injection` — Counter for rate-limited injection

### TodoItem

```rust
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
    pub priority: TodoPriority,
    pub blocker: Option<String>,
}
```

`TodoStatus` variants: `Pending`, `InProgress`, `Completed`, `Blocked`,
`Cancelled`.

`TodoPriority` variants: `Low`, `Medium`, `High`.

### Todo Modes

Controlled by `TaskStatePolicy.mode` (re-exported from
`codegg_config::schema::TodoMode`):

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

- `inject_after_tool_calls` — Rate-limited: inject reminder after N
  tool calls since last injection
- `inject_on_resume` — Inject when session resumes
- `inject_after_compaction` — Inject after context compaction

#### Validation Rules

- `Disabled` mode forces: `allow_model_todo_read/write = false`,
  `max_total_items = 0`, `inject_after_tool_calls = None`
- `GuidedCurrentTask` forces: `allow_model_todo_write = false`,
  `max_total_items = min(4, configured)`
- `max_total_items` capped at 12

### State Transitions

`replace_from_model()` validates against the policy before accepting:

1. Rejects if mode is `Disabled` (`TodoStateError::ModeDisabled`)
2. Rejects if `allow_model_todo_write = false`
   (`TodoStateError::WriteNotAllowed`)
3. Rejects if items exceed `max_total_items`
   (`TodoStateError::TooManyItems`)
4. Rejects multiple in-progress items if `require_single_in_progress`
   (`TodoStateError::MultipleInProgress`)
5. Rejects blocked items without blocker reason if
   `require_blocker_reason` (`TodoStateError::MissingBlockerReason`)

After acceptance, `revision` increments and `reminder_pending` is set
to `true` unless all items are `Completed` or `Cancelled`.

### Projections

- **`compact_projection(policy)`** — Model-facing compact text injected
  into context. Returns `None` when disabled, empty, or all done.
  Format depends on mode:
  - `SparsePlan`: "Active task state: in_progress: X; pending: Y.
    Continue from the active item..."
  - `ExplicitTodo`: "Active todo state:\n- in_progress: X\n- pending: Y\n
    Continue from the in-progress item..."
  - `GuidedCurrentTask`: "Current task: X. Do this task only.
    Report a blocker if unable to continue.\nNext task: Y."
- **`full_projection_for_user()`** — User-facing full list with
  numbers, status, and priority

### Injection (`build_todo_reminder`)

`build_todo_reminder(todo, policy)` determines if a reminder should be
injected. Returns `None` when:
- Mode is `Disabled`
- Items are empty
- `reminder_pending` is false AND `tool_calls_since_injection` is below
  the `inject_after_tool_calls` threshold

### Integration

- `TodoState` is persisted via `TodoItemInput` / `TodoItem` session
  models
- `AppEvent::TodoUpdated` broadcasts todo snapshots to the TUI
- The task state policy is resolved per-model via
  `ResolvedModelProfile.task_state_policy`

## Configuration Surface

```toml
[model_profile."openrouter/qwen/qwen3-coder"]
prompt_profile = "fast_executor"
context_window = 65536

[model_profile."minimax/minimax-2.7"]
supports_late_system_messages = true
```

Config overrides are applied on top of the declarative adapter result.
Suffix matching tries the full model ID, then the part after the first
`/`, then the part after the second `/`.

## Invariants & Gotchas

- `Disabled` mode is absolute — no projection, no injection, no writes.
- `GuidedCurrentTask` prevents model writes; the harness drives state.
- `FastExecutor` and `LocalStrict` profiles automatically get
  `guided_current_task()` policy unless config overrides it.
- Adapter merging is overlay-on-base: the selected adapter's fields
  override the generic adapter's fields. Missing fields fall back.
- `fingerprint` is a SHA-256 of the TOML serialization, used for
  cache invalidation.
- `TaskStatePolicy` types (`TodoMode`, `TodoUpdateFrequency`,
  `CompletedTodoExposure`, `SubagentTodoAccess`) are re-exported from
  `codegg_config::schema`, not defined in codegg-core.

## Testing

```bash
cargo test -p codegg-core -- model_profile
cargo test -p codegg-core -- task_state
```

Narrowest: `cargo test -p codegg-core -- model_profile::adapter::tests`
for adapter matching, `cargo test -p codegg-core -- task_state::tests`
for state machine.

## Related Docs

- `architecture/codegg_core.md` — the parent crate boundary
- `architecture/native_crates.md` — library-first tool architecture
