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
├── progress.rs     # Host-observed Progress/VerifiedWait/NoProgress assessment (M001)
├── render.rs       # Goal rendering helpers for TUI
├── checkpoint.rs   # Session checkpoint integration for goals
└── verification.rs # Host-owned completion proposals, evidence, and verdicts
```

Application assembly lives in `src/goal_continuation.rs` (read-only evidence
from `Goal` + in-memory `TodoState` + goal-labelled durable jobs) and the
loop driver in `src/agent/turn_completion.rs::maybe_continue_goal()`.

## How It Works

### Goal Lifecycle

1. User creates a goal via `/goal set <objective>`.
2. `GoalStore::create_active()` pauses any existing active goal for the
   session and inserts a new one with `Active` status.
3. Each turn, `account_for_turn()` advances usage counters (tokens,
   tool calls, turns, wall-clock).
4. `should_continue()` checks budget axes and terminal status. It remains
   the authoritative budget/status gate but no longer decides continuation
   alone.
5. `maybe_continue_goal()` reloads the current Goal revision each cycle and
   assesses host-observed progress via `progress.rs`:
   `Progress` (todo status or goal-owned test/delegated-run change) resets
   stagnation and continues; `VerifiedWait` (a live goal-labelled
   `InProgress` test/delegated run) polls the existing handle without
   relaunching it; `NoProgress` increments a small run-local consecutive
   counter (nudge, then explicit replan instruction). After
   `MAX_CONSECUTIVE_NOPROGRESS_BEFORE_AWAITING_USER = 3` consecutive
   no-progress cycles the same Goal revision is transitioned to the
   existing `AwaitingUser` state with a concise bounded blocker report.
   The loop still caps at `MAX_CONTINUATIONS = 32` as an emergency
   invariant only.
6. Budget exhaustion → `BudgetLimited` status + wrap-up prompt.
7. `goal_request_completion` submits a model proposal to the host-owned
   verifier. Only a deterministic `Met` verdict can transition the goal to
   `Complete`; model prose and claimed file/test lists are not authority.

There is no `Blocked` goal status. `GoalStatus` is exactly `Active`,
`Paused`, `AwaitingUser`, `BudgetLimited`, `Complete`, `Failed`,
`Cancelled`. Blockers are reported through `goal_update_progress`
`open_questions`; repeated unresolved no-progress state becomes
`AwaitingUser`. Todo-level `Blocked` is a separate short-horizon todo
state and never a goal status.

Convergence stores its semantic `Pass | Revise | Inconclusive` verdict
separately from goal verification. A semantic pass is explanatory evidence and
cannot satisfy a goal, override a failed or missing host-recorded test, mutate
`GoalStatus`, or grant completion authority. Goal completion continues through
`GoalVerificationService` and its deterministic host evidence rules.
Repair and replan are bounded owner decisions only. Their resulting producer
commit remains an explicit integration handoff; parent cleanliness/base
revalidation and deterministic goal verification are still required.

Supervised Test and Subagent jobs created by the daemon while a goal is
active carry the host-written reserved `goal_id` label. Completion evidence
is eligible only when that durable label matches the exact goal being
verified, with session identity and creation time serving only as additional
bounds. Jobs without the label are legacy/unavailable evidence; their
relation is never inferred from timestamps, display names, or model claims.

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

### Goal (`crates/codegg-core/src/goal/model.rs:52`)

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

### GoalStatus (`:8`)

`Active`, `Paused`, `AwaitingUser`, `BudgetLimited`, `Complete`,
`Failed`, `Cancelled`.

`is_terminal()` (:115) returns true for `Complete | Failed | Cancelled |
BudgetLimited`. `is_active()` (:126) returns true only for `Active`.

### GoalBudget (`:19`)

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

### GoalUsage (`:35`)

```rust
pub struct GoalUsage {
    pub turns_used: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub tool_calls: i64,
    pub wallclock_secs: i64,
}
```

### GoalProgressUpdate (`:82`)

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

### CompletionRequest (`:92`)

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

### Continuation progress (`crates/codegg-core/src/goal/progress.rs`)

```rust
pub enum GoalProgressDisposition {
    Progress { fingerprint: String },
    VerifiedWait { handle: WaitHandleRef, fingerprint: String },
    NoProgress { fingerprint: String, reason: GoalNoProgressReason },
}
```

`GoalContinuationEvidence` carries goal id/revision/status, todo revision
plus per-status counts, open-question count, and goal-labelled
test/delegated-run records. `goal_continuation_fingerprint()` hashes only
that bounded metadata (SHA-256, `sha256:` prefix); free-form
`progress_summary`, file contents, command output, and transcript text never
enter the fingerprint, so prose-only updates cannot read as progress.
`assess_goal_continuation()` maps todo/execution changes to `Progress`
(`NewEvidence`/`StateChanged`/`ChildAdvanced` vocabulary), a persisting live
execution to `VerifiedWait`, and anything else to `NoProgress`
(`NoStateChange`/`BlockerReported`/`EvidenceLoadFailed`).
`stagnation_step()` adapts the graduated `Nudge -> Replan -> Stall`
semantics to continuation scope (`ContinueWithNudge`,
`ContinueWithReplan`, `EscalateToAwaitingUser`) without creating a second
general recovery controller. Operator reason codes are `progress`,
`verified_wait`, `replan`, `awaiting_user_no_progress`, and
`budget_limited`; diagnostics never dump command output or plan content.

### WorkPlan binding (M002)

A Goal may have at most one active bound `WorkPlan`
(`crates/codegg-core/src/work_plan/`). Binding is an exact Goal-ID
reference validated for same session/project ownership; it changes no Goal
runtime behavior. Goal status/budget/verification remain authoritative —
the plan owns detailed execution progress only. Goal completion for bound
plans additionally requires no actionable/unmet work (M003 arbiter); M002
establishes the reference seam with CAS bind/unbind. See
`architecture/work_plan.md`.

### GoalStore (`crates/codegg-core/src/goal/store.rs:56`)

SQLite-backed. Key methods:

| Method | Line | Description |
|--------|------|-------------|
| `create_active(...)` | :160 | Pause existing, insert new Active goal |
| `active_for_session(session_id)` | :212 | Fetch active/awaiting/budget-limited goal |
| `get(id)` | :225 | Fetch by ID |
| `update_status(id, status)` | :234 | Transition non-certification status |
| `complete_if_active(id, revision)` | — | Atomic host-accepted terminal transition |
| `clear_active_for_session(sid)` | :261 | Cancel all active goals for session |
| `update_progress(id, update)` | :333 | Advance phase/next-action/open_questions |
| `increment_usage(...)` | :451 | Atomic usage advance + budget check |
| `enforce_budget(id)` | :514 | Check budget without advancing |
| `set_budget(id, budget)` | :530 | Replace budget, revive if BudgetLimited |
| `latest_paused_for_session(sid)` | :560 | Fetch latest paused goal |

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
| `goal_update_progress` | `GoalUpdateProgressTool` (:72) | Update progress |
| `goal_request_completion` | `GoalRequestCompletionTool` (:188) | Request completion with evidence |

**Note**: There is no `goal_set` tool. Goals are created via TUI
`/goal set` commands which call `GoalStore::create_active()` directly.

### Checkpoint System (`crates/codegg-core/src/goal/checkpoint.rs`)

- `create_checkpoint_file()` (:9) — creates `.codegg/goals/{id}.checkpoint.md`
- `read_checkpoint_excerpt()` — bounded head prefix (legacy callers/tests only)
- `read_checkpoint_tail()` / `checkpoint_tail_of()` (M002) — bounded latest tail of the append-only journal with UTF-8-safe char slicing
- `append_checkpoint_update()` — append progress updates

The Markdown journal is a user-facing historical journal, not canonical
current-state authority. Typed `Goal` fields (objective, phase,
progress, next action, open questions, revision) provide current state;
when the journal is included, callers use the bounded tail so newer
progress appended after the old head prefix is not hidden. Turn-start
projection uses `render_goal_context_with_tail`.

Host-owned Goal/Todo revisions stay authoritative through compaction
(M004): the rollover captures goal ID/revision, plan digest, todo revision,
and parent lineage before enrichment and revalidates before install. A
newer active goal revision than the installed checkpoint merges with M002
precedence at turn start and is never hidden by stale checkpoint next
steps.

### Render Helpers (`crates/codegg-core/src/goal/render.rs`)

- `render_goal_context()` — full goal context for system prompt (legacy excerpt form; truncation is char-boundary-safe)
- `render_goal_context_with_tail()` (M002) — typed current state plus a bounded latest journal tail, kept separate from chronological updates; never parses the journal to rediscover typed fields
- `render_goal_status()` — one-line status summary

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
- `maybe_continue_goal()` caps at `MAX_CONTINUATIONS = 32` per run as an
  emergency invariant only. Normal stagnation exits after 3 consecutive
  no-progress cycles via replan then `AwaitingUser`, well before the cap.
- An Active goal never auto-continues on budget alone: every cycle needs
  `Progress` or a `VerifiedWait` on a live goal-owned handle. `VerifiedWait`
  polls the existing handle and never relaunches the operation; a handle
  that disappears is reloaded and its terminal outcome becomes
  progress/evidence.
- Failure to load progress evidence is `NoProgress(EvidenceLoadFailed)`,
  never `Progress`. Cancellation/steering stops continuation immediately
  and is never converted into blocker progression. Replacement aborts the
  stale path; `AwaitingUser` escalation uses `update_status_if_revision`
  so concurrent progress wins. The no-progress counter is run-local and
  restarts conservatively on daemon restart without ever marking complete.
- `GoalRequestCompletionTool` submits a bounded model proposal; only a
  passing exact-goal-owned test/delegated-job evidence set can produce `Met`.
  Failed/missing evidence produces `NotMet`, and non-empty natural-language
  criteria or remaining risks require `AwaitingUser`. Criteria are not
  classified by words such as `test`, `pass`, `green`, `todo`, or `task`.
- `proposal.tests_run` and `proposal.files_changed` are bounded explanatory
  claims. A test claim requires a passing host-owned test for the active goal,
  but its free-form name is not treated as invocation identity; file claims
  never provide positive proof without a separate host-owned source.
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
