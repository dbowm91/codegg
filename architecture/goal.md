# Goal Module Architecture

The `goal` module implements a Codex-style long-horizon goal runtime
with budget enforcement, TUI-rendered status, and autonomous
continuation. Goals are the durable, multi-session planning surface —
distinct from in-flight todos.

## Purpose

Provide a structured way for the agent to track long-running objectives
across sessions, enforce resource budgets, and autonomously continue
work until the goal is complete or budget is exhausted.

## Where It Lives

| Component | Location |
|-----------|----------|
| Core types, store, runtime | `crates/codegg-core/src/goal/` |
| Model-facing tools | `src/tool/goal.rs` |
| TUI slash commands | `src/tui/commands/` (`/goal *`) |
| DB schema | `crates/codegg-core/src/session/schema.rs` (migration v16; revision CAS added in v45) |

### Module Structure

```
crates/codegg-core/src/goal/
├── mod.rs          # Re-exports
├── model.rs        # Goal, GoalStatus, GoalBudget, GoalUsage structs
├── store.rs        # GoalStore: SQLite persistence, budget accounting
├── runtime.rs      # GoalWallClock, should_continue, continuation prompts
├── render.rs       # Goal rendering helpers for TUI
├── checkpoint.rs   # Session checkpoint integration for goals
└── verification.rs # Host-owned completion proposals, evidence, and verdicts
```

## How It Works

### Goal Lifecycle

1. User creates a goal via `/goal set <objective>`.
2. `GoalStore::create_active()` pauses any existing active goal for the
   session and inserts a new one with `Active` status.
3. Each turn, `account_for_turn()` advances usage counters (tokens,
   tool calls, turns, wall-clock).
4. `should_continue()` checks budget axes and returns a
   `ContinuationDecision`. If active with budget remaining, a
   continuation prompt is queued.
5. `maybe_continue_goal()` loops up to 32 iterations, re-accounting
   after each continuation.
6. Budget exhaustion → `BudgetLimited` status + wrap-up prompt.
7. `goal_request_completion` submits a model proposal to the host-owned
   verifier. Only a deterministic `Met` verdict can transition the goal to
   `Complete`; model prose and claimed file/test lists are not authority.

### Budget Enforcement

`GoalStore::increment_usage()` atomically advances counters and checks
breaches via `first_budget_breach()`. On breach, status transitions to
`BudgetLimited`. `/goal budget raise` calls `set_budget()` which
revives `BudgetLimited` → `Active` if the new budget is sufficient.

### Wall-Clock Accounting

`GoalWallClock` tracks time via `Instant::now()`. The delta since the
last tick is added to `usage.wallclock_secs` and persisted in SQLite,
surviving session restarts.

## Key Types & APIs

### Goal (`crates/codegg-core/src/goal/model.rs:51`)

```rust
pub struct Goal {
    pub id: String,
    pub revision: i64,
    pub session_id: String,
    pub project_id: String,
    pub title: String,
    pub objective: String,
    pub status: GoalStatus,
    pub plan_path: Option<String>,
    pub checkpoint_path: Option<String>,
    pub current_phase: Option<String>,
    pub progress_summary: String,
    pub next_action: Option<String>,
    pub completion_criteria: Vec<String>,
    pub open_questions: Vec<String>,
    pub budget: GoalBudget,
    pub usage: GoalUsage,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}
```

### GoalStatus (`:6`)

`Active`, `Paused`, `AwaitingUser`, `BudgetLimited`, `Complete`,
`Failed`, `Cancelled`.

`is_terminal()` (:112) returns true for `Complete | Failed | Cancelled |
BudgetLimited`. `is_active()` (:123) returns true only for `Active`.

### GoalBudget (`:18`)

```rust
pub struct GoalBudget {
    pub max_turns: Option<i64>,
    pub max_model_tokens: Option<i64>,
    pub max_tool_calls: Option<i64>,
    pub max_wallclock_secs: Option<i64>,
}
```

All axes are optional. Budget is checked in priority order: tokens →
tool calls → turns → wall-clock.

### GoalUsage (`:34`)

```rust
pub struct GoalUsage {
    pub turns_used: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub tool_calls: i64,
    pub wallclock_secs: i64,
}
```

### GoalProgressUpdate (`:78`)

```rust
pub struct GoalProgressUpdate {
    pub current_phase: Option<String>,
    pub progress_summary: Option<String>,
    pub next_action: Option<String>,
    pub completed_items: Vec<String>,
    pub remaining_items: Vec<String>,
    pub open_questions: Vec<String>,
}
```

### CompletionRequest (`:88`)

```rust
pub struct CompletionRequest {
    pub evidence: String,
    pub files_changed: Vec<String>,
    pub tests_run: Vec<String>,
    pub remaining_risks: Vec<String>,
}
```

`GoalCompletionProposal` is the bounded runtime form of this request. The
application assembles `GoalEvidenceContext` from durable session-scoped job
and todo stores. Failed or in-flight supervised tests/delegated jobs and
unfinished todos produce a bounded `NotMet` verdict. Criteria that cannot be
decided deterministically produce `AwaitingUser` rather than being guessed by
the model. The verifier is stateless and read-only; restart reconstructs its
inputs from the owning stores.

### GoalRuntimeOutcome (`crates/codegg-core/src/goal/runtime.rs:55`)

```rust
pub enum GoalRuntimeOutcome {
    NoActiveGoal,
    Advanced { goal_id, usage, budget },
    BudgetLimited { goal_id, reason, usage, budget },
}
```

### ContinuationDecision (`runtime.rs:146`)

```rust
pub struct ContinuationDecision {
    pub should_continue: bool,
    pub reason: String,
    pub prompt: Option<String>,
}
```

### GoalStore (`crates/codegg-core/src/goal/store.rs:56`)

SQLite-backed. Key methods:

| Method | Line | Description |
|--------|------|-------------|
| `create_active(...)` | :157 | Pause existing, insert new Active goal |
| `active_for_session(session_id)` | :209 | Fetch active/awaiting/budget-limited goal |
| `get(id)` | :222 | Fetch by ID |
| `update_status(id, status)` | :231 | Transition non-certification status |
| `complete_if_active(id, revision)` | — | Atomic host-accepted terminal transition |
| `clear_active_for_session(sid)` | :261 | Cancel all active goals for session |
| `update_progress(id, update)` | :278 | Advance phase/next-action/open_questions |
| `increment_usage(...)` | :363 | Atomic usage advance + budget check |
| `enforce_budget(id)` | :424 | Check budget without advancing |
| `set_budget(id, budget)` | :440 | Replace budget, revive if BudgetLimited |
| `latest_paused_for_session(sid)` | :469 | Fetch latest paused goal |

### GoalUsageUpdate (`store.rs:11`)

Returned by `increment_usage()`:

```rust
pub struct GoalUsageUpdate {
    pub usage: GoalUsage,
    pub budget: GoalBudget,
    pub budget_limited: bool,
    pub reason: Option<String>,
}
```

### Model-Facing Tools (`src/tool/goal.rs`)

| Tool | Struct | Description |
|------|--------|-------------|
| `goal_get` | `GoalGetTool` (:9) | Get current active goal |
| `goal_update_progress` | `GoalUpdateProgressTool` (:71) | Update progress |
| `goal_request_completion` | `GoalRequestCompletionTool` (:187) | Request completion with evidence |

**Note**: There is no `goal_set` tool. Goals are created via TUI
`/goal set` commands which call `GoalStore::create_active()` directly.

### Checkpoint System (`crates/codegg-core/src/goal/checkpoint.rs`)

- `create_checkpoint_file()` (:9) — creates `.codegg/goals/{id}.checkpoint.md`
- `read_checkpoint_excerpt()` (:85) — read with truncation
- `append_checkpoint_update()` (:103) — append progress updates

### Render Helpers (`crates/codegg-core/src/goal/render.rs`)

- `render_goal_context()` (:5) — full goal context for system prompt
- `render_goal_status()` (:52) — one-line status summary

## Configuration Surface

No dedicated config section. Goals are always available. The model
receives instructions via `goal_and_todos_contract()` in the system
prompt.

### TUI Slash Commands

```
/goal set <objective>        # Create new goal
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

## Invariants & Gotchas

- `create_active()` **pauses** any existing active/awaiting/budget-
  limited goal for the session before creating the new one.
- `increment_usage()` only advances if goal `is_active()`. Terminal
  goals silently skip accounting.
- `maybe_continue_goal()` caps at `MAX_CONTINUATIONS = 32` per run to
  prevent infinite loops.
- `GoalRequestCompletionTool` submits a bounded model proposal; only a
  passing host-owned test/delegated-job evidence set can produce `Met`.
  Failed/missing evidence produces `NotMet`, and semantic criteria or
  remaining risks require `AwaitingUser`.
- `BudgetLimited` is treated as terminal by `is_terminal()` — the agent
  cannot auto-continue. The user must raise the budget to resume.
- Wall-clock seconds are persisted in SQLite and survive session
  restarts. The clock resets after each accounting tick.
- Unaccounted deltas (`unaccounted_input_tokens`, etc.) are retained on
  storage failure rather than lost or double-counted.

## DB Schema

Defined in `crates/codegg-core/src/session/schema.rs` migration v16.
Indexes on `(session_id, status)` and `(project_id, status)`. Migration v45
adds a monotonic `goal.revision` used by host verification compare-and-set
transitions; old rows receive revision zero.

## Testing

```bash
cargo test -p codegg-core -- goal
```

## Related Docs

- [agent.md](agent.md) — AgentLoop integration
- `src/tool/goal.rs` — model-facing tool implementations
