# WorkPlan Module Architecture

Durable detailed work state for long-horizon execution (M002 foundation,
M003 projection and arbitration). `WorkPlan`/`WorkItem` records what
remains, what blocks it, and what host-owned evidence could satisfy it —
without becoming execution authority.

## Purpose

Give large tasks a revisioned, restart-durable plan that survives
compaction and restart independently of transcript summaries, TodoState
projection, Goal progress text, and continuation checkpoints. M002 landed
storage/domain semantics with no model-visible behavior change; M003 adds
the bounded model read/update surface, one-way Todo projection, canonical
evidence correlation, and the host-owned completion arbiter.

## Where It Lives

| Component | Location |
|-----------|----------|
| Core types, validation, actionability | `crates/codegg-core/src/work_plan/model.rs` |
| Durable store, CAS, Goal scope checks | `crates/codegg-core/src/work_plan/store.rs` |
| Pure completion assessment (M003) | `crates/codegg-core/src/work_plan/assessment.rs` |
| Bounded projection/pagination (M003) | `crates/codegg-core/src/work_plan/projection.rs` |
| Host-evidence snapshot (M003) | `crates/codegg-core/src/work_plan/evidence.rs` |
| One-way Todo contract (M003) | `crates/codegg-core/src/work_plan/todo_projection.rs` |
| Module re-exports | `crates/codegg-core/src/work_plan/mod.rs` |
| Model tools (`work_plan_get`, `work_plan_update_item`) | `src/tool/work_plan.rs` |
| Evidence assembly (jobs/runs) | `src/work_plan_evidence.rs` |
| Enriched evidence resolver | `src/work_plan_evidence.rs::assemble_resolved_evidence`; resolves subjects from durable JobAttempt rows only |
| Completion arbiter + turn/Goal gates | `src/work_plan_arbiter.rs` |
| Todo one-way sync | `src/work_plan_todo_sync.rs` |
| DB schema | `crates/codegg-core/src/session/schema.rs` migration v67; layout `storage::STORAGE_LAYOUT_VERSION = 67` |
| Integration tests | `crates/codegg-core/tests/work_plan_foundation.rs`, `crates/codegg-core/tests/work_plan_projection_arbiter.rs`, `tests/work_plan_projection_arbiter.rs` + in-module unit tests |

## How It Works

### WorkPlan

```rust
pub struct WorkPlan {
    pub id: WorkPlanId,          // wp_ prefix, distinct from Goal/Job/Run/Todo IDs
    pub revision: i64,           // monotonic CAS token, starts 0
    pub session_id: String,
    pub project_id: String,
    pub origin_turn_id: Option<String>,
    pub goal_id: Option<String>, // exact bound Goal, same session/project only
    pub objective: String,       // immutable after creation (<= 4000 chars)
    pub objective_digest: String,// sha256: of objective only
    pub origin_provenance: String, // immutable, e.g. turn:turn-1 (<= 1024)
    pub status: WorkPlanStatus,  // Active | Blocked | Completed | Cancelled
    pub current_phase: Option<String>,
    pub current_item_id: Option<WorkItemId>,
    pub created_at / updated_at / completed_at: DateTime<Utc>,
}
```

One active (`Active`/`Blocked`) plan per session. `create_active()` cancels
any predecessor in the same transaction; the predecessor row and its items
are retained as historical evidence, never deleted. Objective and provenance
are immutable; phase/current-item/goal-binding evolve through CAS.

Plan transitions: `Active <-> Blocked`, `Active`/`Blocked -> Completed`/
`Cancelled`. Terminal states have no outgoing edges. Completion in M002 is
an explicit guarded decision; completion arbitration belongs to M003.

### WorkItem

```rust
pub struct WorkItem {
    pub id: WorkItemId,          // wi_ prefix
    pub plan_id: WorkPlanId,
    pub revision: i64,           // per-item CAS token
    pub position: i64,           // stable ordering
    pub parent_item_id: Option<WorkItemId>,
    pub dependencies: Vec<WorkItemId>,
    pub status: WorkItemStatus,  // Pending|Actionable|InProgress|Blocked|Completed|Cancelled
    pub description: String,     // <= 1024
    pub acceptance: Vec<WorkAcceptance>, // <= 16, each <= 512 + disposition
    pub evidence: Vec<WorkEvidenceRef>,  // <= 16, canonical refs only
    pub owner_run_id / owner_job_id: Option<String>, // provenance only, <= 256
    pub attempts: i64,           // increments on each InProgress entry
    pub blocker: Option<String>, // required exactly when Blocked (<= 1024)
    pub next_action: Option<String>,     // <= 1024
    pub created_at / updated_at: DateTime<Utc>,
}
```

Item transitions (`can_transition_item`): `Pending -> Actionable`/
`InProgress`/`Blocked`/`Cancelled`; `Actionable -> Pending`/`InProgress`/
`Blocked`/`Completed`/`Cancelled`; `InProgress -> Actionable`/`Blocked`/
`Completed`/`Cancelled`; `Blocked -> Pending`/`Actionable`/`InProgress`/
`Cancelled` (never directly to `Completed` — unblocking stays explicit);
terminal states have no outgoing edges.

### Acceptance and evidence

```rust
pub enum WorkAcceptanceDisposition { Unmet, Satisfied, RequiresUserJudgment }
pub struct WorkAcceptance { pub description: String, pub disposition, pub note: Option<String> }
pub enum WorkEvidenceKind { TestJob, DelegatedRun, SchedulerJob, AgentRun, Artifact, Commit }
pub struct WorkEvidenceRef { pub kind: WorkEvidenceKind, pub ref_id: String, pub detail: Option<String> }
```

Model text can explain progress but never creates `Satisfied`: only a
canonical host-owned source (or explicit user judgment) does. A ref whose
target cannot be found is unavailable evidence, never satisfied. M002
validates ref shape (closed kind + bounded id/detail); M003 correlates
against canonical job/run/artifact stores. As an M002 floor, completing an
item requires at least one host signal (non-`Unmet` acceptance, evidence
ref, or owner run/job provenance) so bare model text cannot manufacture
completion.

### Actionability

`actionable_items()` is deterministic: an item is actionable when its status
is `Pending`/`Actionable` and every dependency is `Completed`. Ordering is
stable by `(position, id)`. Terminal, in-progress, and blocked items are
never actionable. Dependencies only gate actionability; there is no cron,
condition language, loop, retry-as-workflow, or user automation.

Dependency/parent validation (`validate_item_graph`): deps and parent must
exist in the same plan (cross-plan refs rejected), must not be self, and
must not introduce dependency or parent-chain cycles. Cycles are rejected at
creation/update and never cause unbounded traversal (bounded DFS over at
most `MAX_ITEMS_PER_PLAN = 64` nodes).

### Bounds

| Field | Bound |
|-------|-------|
| Items per plan | 64 (larger than TodoState cap 10-12) |
| Objective | 4000 chars |
| Origin provenance | 1024 chars |
| Current phase | 256 chars |
| Item description | 1024 chars |
| Dependencies per item | 16 |
| Acceptance per item | 16, each description 512, note 512 |
| Evidence per item | 16, each ref 512, detail 512 |
| Blocker / next action | 1024 chars each |
| Owner run/job refs | 256 chars each |
| Session/project/turn/goal scope IDs | 256 chars each |

Bounds are enforced in domain validation and re-asserted with SQL `CHECK`
constraints. No hidden-reasoning field exists; serialization tests fail
closed if `reasoning`/`thinking`/`chain_of_thought`/`scratchpad` ever
appears.

### Store and CAS

`WorkPlanStore` is the single owner. Plan mutations require the plan
revision; item mutations require the item revision. Every item mutation
bumps the owning plan revision in the same transaction, so concurrent item
completion and plan cancellation serialize deterministically (first writer
wins, loser gets `WorkPlanError::Conflict`). Transaction failure leaves the
prior revision intact. Restart loads durable state exactly; `in_progress`
does not imply replay.

Goal binding (`bind_goal`/`unbind_goal`): the bound Goal must exist and live
in the same session/project (validated via the `goal` table; Goal runtime
behavior untouched). At most one active plan per goal as well as per
session. Deleting/cancelling a Goal never deletes WorkPlan evidence; active
behavior stops only through an explicit plan transition.

## DB Schema

Migration v59 (`migrate_v59`, `WORK_PLAN_SCHEMA_STATEMENTS`):

- `work_plan` — plan row with revision, scope, objective/digest/provenance,
  status, phase/current-item pointers, timestamps. Indexes on
  `(session_id, status)` and `goal_id`.
- `work_item` — item row with revision, position, parent, `dependencies_json`,
  `acceptance_json`, `evidence_json` (each length-checked), owner refs,
  attempts, blocker/next-action, timestamps. Index on `(plan_id, position)`.

Additive `IF NOT EXISTS`; pre-M002 databases gain empty tables with no
backfill. Legacy sessions simply have no active plan.

## Ownership Boundaries

- WorkPlan is detailed work state, not execution authority. It references
  `AgentRun`/`Job` IDs for provenance; those refs grant no write permission.
- Goal status/budget remain authoritative when a plan is Goal-bound.
  GoalVerification remains the final Goal completion authority; the WorkPlan
  arbiter is an additional prerequisite, not a replacement.
- TodoState is a bounded one-way projection (M003): WorkPlan selects
  current/actionable items, TodoState renders a small subset per
  `TaskStatePolicy`. Permitted Todo status changes translate back only on
  exact `wi_*@rN` identity/revision mapping; a Todo `completed` flag alone
  never satisfies host-only acceptance. Child-owned items require the
  explicit WorkPlan tool with matching `caller_run_id`.
- Continuation checkpoints remain distinct with their own lifecycle; M003
  adds no checkpoint coupling (M004).
- Model tools cannot set host evidence (`acceptance`/`evidence`/`owner`
  refs), rewrite `objective`/`origin_provenance`/`dependencies`, or mark
  host-only items complete by assertion. Stale writers receive explicit
  conflicts.

## M003 projection and arbitration

- Assessment (`assessment.rs`): pure `Complete` / `ActionableWorkRemaining`
  / `Blocked` / `AwaitingUserJudgment` / `InFlight` from plan/items plus the
  host-evidence snapshot. `Blocked` is a WorkPlan state, not a GoalStatus.
  Completed items without host satisfaction read as actionable, never done.
- Projection (`projection.rs`): defaults to current/actionable (limit 5,
  max 8 items, 4096 bytes); completed history requires explicit
  `include_completed` with pagination. Single-item lookup stays bounded.
- Evidence (`evidence.rs` + `src/work_plan_evidence.rs`): Test/Scheduler/
  Delegated refs resolve against the durable job store; AgentRun refs
  resolve against `agent_run` (job-store fallback); Artifact/Commit refs
  stay `Unavailable` without a host `Satisfied` acceptance. Missing targets
  are unavailable, never satisfied; claimed test text alone never passes.
- Todo contract (`todo_projection.rs` + `src/work_plan_todo_sync.rs`):
  projection truncates to `policy.max_total_items` (Disabled 0, Sparse 8,
  Explicit 10, Guided 4) with at most one `InProgress`. Feedback validates
  exact revision, allowed transitions, blocker discipline, and host-evidence
  gating for `Completed`.
- Arbiter (`src/work_plan_arbiter.rs` + `src/agent/loop.rs` hook):
  terminal-answer boundary checks the active plan before the turn is
  considered done. Actionable work injects one bounded control message
  (current item, unmet condition, next action) and continues within existing
  turn/tool/time/token limits; `InFlight` polls the existing handle;
  `Blocked`/`Inconclusive` preserve state and surface a typed report;
  `AwaitingUserJudgment`/`Complete` return control. Ordinary turn-scoped
  plans auto-complete only after the host passes and budgets have not
  expired; Goal-bound plans additionally pass `GoalVerificationService`,
  whose `NotMet` may update one actionable item's `next_action` through CAS.
- Protocol (`codegg-core::bus::events::WorkPlanUpdated` + `WorkPlanSnapshot`):
  bounded frontend summary (ids, revision, status, counts, assessment code).
  Frontends render only; mutations stay in model tools/daemon service.

## Invariants & Gotchas

- `WorkPlanId` (`wp_`) / `WorkItemId` (`wi_`) are distinct types from
  `AgentTaskId`, `AgentRunId`, `JobId`, Goal ID, Todo ID.
- Stale writers get `Conflict`, never last-write-wins.
- `Blocked` items require a blocker reason; non-blocked items must not
  retain one.
- `Blocked` items cannot complete directly.
- Objective/provenance immutable; only phase/current-item/goal-binding mutate.
- Owner/run/job/evidence refs never duplicate payloads and never carry
  reasoning.
- `src/session/schema.rs` wiring (chain + dispatch + definition) and
  `storage::STORAGE_LAYOUT_VERSION` must advance together (guard:
  `scripts/check_project_catalog_invariants.py`).

## M004 checkpoint provenance and fresh epochs

- Provenance (`work_plan/checkpoint.rs`): bounded `WorkPlanCheckpointProvenance`
  (ID/revision/status/phase/item + ≤5 actionable/blocked summaries + source
  digest + counts). Built by `build_checkpoint_provenance`, validated by
  `validate_provenance`, carried additively in continuation payloads under
  `work_plan`. Pre-M004 checkpoints without it remain readable; malformed
  blocks fail closed at restart validation.
- Snapshot (`src/context/continuation.rs`): `ContinuationSnapshot.work_plan`
  plus `WorkPlanNextAction` current-task precedence (Goal → WorkPlan →
  Todo → previous). Frame/footer and `rollover::render_installed_projection`
  surface the plan ID/revision/phase/item without embedding the full plan.
- Revalidation (`work_plan/checkpoint.rs::revalidate_against_current` +
  `rollover::RolloverSourceRevisions` work-plan fields): captured ID/revision
  must equal current durable state before install; drift aborts/rebuilds.
- Policy (`work_plan/epoch_policy.rs`): deterministic `decide_epoch` with
  typed triggers (`phase_boundary`, `repeated_compaction`,
  `explicit_operator`, `model_profile_policy`) and keep reasons
  (`disabled`, `unsupported_profile`, ...). Default disabled; supported
  profiles are long-horizon only; same state → same decision.
- Reconstruction (`src/context/epoch.rs` + `AgentLoop::try_start_fresh_epoch`):
  consumer of the canonical compaction/rollover owners (no second engine,
  guarded by `no_second_compaction_engine_or_history_store`). Emits one
  versioned handoff block, preserves steering/handles, publishes bounded
  `context_epoch:started`. Normal compaction remains default.

## M005 trajectory and recovery qualification

M005 qualified the M001–M004 contracts with no production delta. The
representative harness is `tests/long_horizon_trajectory_qualification.rs`
(22 scenarios): a 9-item/3-phase plan with dependency chains, a delegated
child (`owner_run_id` + `DelegatedRun` evidence), live-then-completing
Test/Subagent jobs, and user steering after an early phase is driven across
eight context transitions (repeated `compact_context` + checkpoint
prepare/install with WorkPlan provenance, plus one policy-gated fresh epoch
at a verified phase boundary). Per-transition assertions cover objective,
phase/current item, remaining required work (9→0), next action, steering
visibility, and stable canonical evidence identity.

Focused scenarios additionally cover Goal-bound trajectories (bound plan
gates Goal completion; verifier `Met` only after host completion),
premature-final continuation, verified waits naming the identical handle,
no-progress nudge → replan → `AwaitingUser` below the emergency cap,
passing/failed/forged evidence, file-backed reopen (plan mutation, live
job, post-completion race, prepared/installed boundary), steering/stale
contention, cancellation (unfinished stays unfinished; close fails closed),
child-vs-parent races, security negatives (no forged completion, no hidden
content in diagnostics, epoch preserves execution policy), legacy
migration paths, bounded projections/diagnostics, missing-artifact
degradation, and the no-second-compaction/workflow static guard.

Standing results: completed work keeps exactly one attempt per item across
all resets; canonical job counts never grow due to a reset; complete plans
terminate with a single turn-end close. Full evidence matrix:
`plans/closure/long-horizon-work-execution/005-status.md`.

## Testing

```bash
cargo test -p codegg-core --lib -- work_plan
cargo test -p codegg-core --test work_plan_foundation
cargo test -p codegg-core --test work_plan_projection_arbiter
cargo test --test work_plan_projection_arbiter
cargo test --test long_horizon_trajectory_qualification
cargo test -p codegg-core -- migration
```

Unit (model): IDs, bounds, transition matrices, blocker rule, actionability,
cycle rejection, digest stability, evidence shape, no-reasoning guard.
Store: create/load/replacement, item CAS lifecycle, completion floor,
plan/item revision serialization, patch bounds/scope, goal binding scope.
Integration (`tests/work_plan_foundation.rs`): v59 migration, pre-migration
upgrade with goal preservation, file-backed restart, concurrent writers,
cancel-vs-completion race, goal bind/unbind race, cross-session rejection,
dangling evidence explicitness, owner provenance, malformed rejection.

## Related Docs

- [goal.md](goal.md) — Goal remains objective/budget/continuation authority
- [model_profile_task_state.md](model_profile_task_state.md) — TodoState stays bounded projection
- [session.md](session.md) — storage ownership, v59 tables
- ADR-0003 — ownership decision; M003/M004 consume this foundation
