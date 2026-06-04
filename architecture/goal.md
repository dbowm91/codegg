# Goal Module Architecture

## Overview

The `goal` module (`src/goal/`) implements a Codex-style long-horizon goal runtime with budget enforcement, TUI-rendered status, and autonomous continuation. Goals are the durable, multi-session planning surface — distinct from in-flight todos.

## Module Structure

```
src/goal/
├── mod.rs          # Module root, re-exports GoalRuntimeOutcome
├── model.rs        # Goal, GoalStatus, GoalBudget, GoalUsage structs
├── store.rs        # GoalStore: SQLite persistence, budget accounting
├── runtime.rs      # GoalWallClock, should_continue, continuation prompts
├── tool.rs         # Tool definitions: goal_set, goal_update_progress, goal_request_completion
├── render.rs       # Goal rendering helpers for TUI
└── checkpoint.rs   # Session checkpoint integration for goals
```

## Key Types

### Goal (`model.rs`)

```rust
pub struct Goal {
    pub id: String,
    pub session_id: String,
    pub project_id: String,
    pub title: String,
    pub objective: String,
    pub status: GoalStatus,
    pub current_phase: Option<String>,
    pub progress_summary: String,
    pub next_action: Option<String>,
    pub completion_criteria: Vec<String>,
    pub open_questions: Vec<String>,
    pub budget: GoalBudget,
    pub usage: GoalUsage,
    // timestamps: created_at, updated_at, started_at, completed_at
}
```

### GoalStatus

```rust
pub enum GoalStatus {
    Active,        // Agent is actively working
    Paused,        // User paused via /goal pause
    AwaitingUser,  // Blocked on user input
    BudgetLimited, // Budget axis exhausted — agent wraps up
    Complete,      // Goal met (requires evidence)
    Failed,        // Goal failed
    Cancelled,     // User cancelled
}
```

### GoalBudget (`model.rs`)

Four enforcement axes (all optional):

```rust
pub struct GoalBudget {
    pub max_turns: Option<i64>,
    pub max_model_tokens: Option<i64>,
    pub max_tool_calls: Option<i64>,
    pub max_wallclock_secs: Option<i64>,  // seconds, durable across sessions
}
```

### GoalUsage (`model.rs`)

Tracks cumulative usage (persisted in SQLite):

```rust
pub struct GoalUsage {
    pub turns_used: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub tool_calls: i64,
    pub wallclock_secs: i64,  // durable across sessions
}
```

## GoalRuntimeOutcome (`runtime.rs`)

Returned by `account_for_turn()` after advancing usage:

```rust
pub enum GoalRuntimeOutcome {
    NoActiveGoal,
    Advanced { goal_id, usage, budget },
    BudgetLimited { goal_id, reason, usage, budget },
}
```

## ContinuationDecision (`runtime.rs`)

Returned by `should_continue()` / `should_continue_for_session()`:

```rust
pub struct ContinuationDecision {
    pub should_continue: bool,
    pub reason: String,
    pub prompt: Option<String>,  // continuation or wrap-up prompt
}
```

## AgentLoop Integration (`agent/loop.rs`)

The agent loop holds two goal-related fields:

```rust
pub goal_store: Option<Arc<GoalStore>>,
pub goal_wall_clock: Mutex<GoalWallClock>,
```

### Turn Lifecycle

1. **Stream completes** → `ChatEvent::Finish { usage }` captured; per-turn `input_tokens`/`output_tokens` stored on `AgentLoopState`.
2. **Publish finished** → `publish_agent_finished()` emits `AgentFinished`.
3. **Account** → `account_goal_for_turn()` computes wall-clock delta, calls `runtime::account_for_turn()`, returns `GoalRuntimeOutcome`.
4. **Continue?** → `maybe_continue_goal()` calls `should_continue_for_session()` in a loop (max 32 iterations):
   - `Continue` → queue `build_continuation_prompt()` via `follow_up_tx`, drain, re-account.
   - `BudgetLimited` → queue `build_budget_wrap_up_prompt()`, drain once, stop.
   - Terminal / no goal → exit.

### Per-Turn Token Tracking

```rust
pub last_turn_input_tokens: i64,   // from ChatEvent::Finish
pub last_turn_output_tokens: i64,
```

Written on each `ChatEvent::Finish` inside `stream_once(&mut self, ...)`. Reset to 0 before each continuation turn so deltas are per-turn, not cumulative.

## GoalStore (`store.rs`)

SQLite-backed goal storage:

- `create_goal()` → insert with Active status
- `active_for_session()` → fetch active goal for a session
- `increment_usage()` → atomic budget check + usage advance; transitions to `BudgetLimited` if axis exceeded; returns `Option<GoalUsageUpdate>`
- `set_budget()` → replace budget; revives `BudgetLimited` → `Active` if new budget is high enough
- `update_progress()` → advance phase/next-action/open_questions
- `request_completion()` → evidence-based transition to `Complete`

### GoalUsageUpdate

```rust
pub struct GoalUsageUpdate {
    pub goal_id: String,
    pub old_status: GoalStatus,
    pub new_status: GoalStatus,
    pub breached_axis: Option<String>,
}
```

## TUI Integration

### Status Bar (`tui/components/status_bar.rs`)

The `StatusBarWidget` renders a goal line in the status bar when `active_goal` is set:

```
[active] Ship codex-style goals  !tok 1.2K/20K turns 2/10 calls 5/50 !wall 12s/600s
```

- `!` prefix appears on any axis that's at or over the limit.
- Formatted by `format_goal_status_line(&GoalSnapshot)` in `tui/app/mod.rs`.

### Sidebar

`App::set_active_goal()` stores a `GoalSnapshot` on the `App` struct. The sidebar renders it when present.

### Slash Commands

```text
/goal set <objective>        # Create a new goal
/goal show                   # Show active goal details
/goal pause                  # Pause active goal
/goal resume                 # Resume paused goal
/goal clear                  # Cancel active goal
/goal done                   # Mark goal complete
/goal from-file <path>       # Load goal from markdown file
/goal checkpoint             # Create session checkpoint
/goal budget show            # Show budget/usage in toast
/goal budget raise <axis> <n>  # Raise a budget axis
```

Budget axes: `tokens`, `turns`, `tool-calls`, `wallclock`.

### AppEvent Wiring

| Event | Source | TUI Handler |
|-------|--------|-------------|
| `GoalUpdated` | GoalStore mutations | Updates `app.active_goal` |
| `GoalUsageUpdated` | `account_for_turn` | Updates usage on `app.active_goal` |
| `GoalBudgetLimited` | `GoalStore::increment_usage` | Shows budget-limited toast |
| `GoalCompleted` | `goal_request_completion` | Clears active goal, shows toast |

## System Prompt Steering

The `goal_and_todos_contract()` in `agent/prompt.rs` instructs the model:

- **In-flight planning**: use the `todo` tool for single-step tasks the user can check off.
- **Long-horizon planning**: use `goal_set` / `goal_update_progress` / `goal_request_completion` for multi-session work.
- **Completion evidence**: must include concrete commands run, files changed, tests passing, and `remaining_risks`.

## Design Decisions

### Goals vs. Todos (Two Separate Surfaces)

Goals are long-horizon, multi-session, durable, and autonomous. Todos are in-flight, per-turn, and ephemeral. They form a hierarchy: a goal spans many sessions; each session may have todos that are steps toward the goal.

### Budget Enforcement

Budget axes are checked atomically in `GoalStore::increment_usage()`. When any axis is exceeded, the goal transitions to `BudgetLimited` and the agent receives a wrap-up prompt on the next turn. The user can raise the budget with `/goal budget raise`, which revives the goal to `Active`.

### Wall-Clock Accounting

`GoalWallClock` tracks time since the last accounting tick. The delta is computed from `Instant::now()` and added to `usage.wallclock_secs`. The clock is reset after each accounting tick to avoid stale leaks. The value is persisted in SQLite so it survives session restarts.

### Continuation Loop Safety

`maybe_continue_goal()` caps at `MAX_CONTINUATIONS = 32` per `run()` invocation to prevent infinite loops. The runtime's `should_continue()` checks all budget axes and terminal statuses on every iteration.
