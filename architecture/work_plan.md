# WorkPlan Module Architecture

Durable detailed work state for long-horizon execution (M002). `WorkPlan`/
`WorkItem` records what remains, what blocks it, and what host-owned
evidence could satisfy it — without becoming execution authority.

## Purpose

Give large tasks a revisioned, restart-durable plan that survives
compaction and restart independently of transcript summaries, TodoState
projection, Goal progress text, and continuation checkpoints. Model-facing
plan tools and automatic completion arbitration belong to M003; this module
lands storage/domain semantics only with no model-visible behavior change.

## Where It Lives

| Component | Location |
|-----------|----------|
| Core types, validation, actionability | `crates/codegg-core/src/work_plan/model.rs` |
| Durable store, CAS, Goal scope checks | `crates/codegg-core/src/work_plan/store.rs` |
| Module re-exports | `crates/codegg-core/src/work_plan/mod.rs` |
| DB schema | `crates/codegg-core/src/session/schema.rs` migration v59; layout `storage::STORAGE_LAYOUT_VERSION = 59` |
| Integration tests | `crates/codegg-core/tests/work_plan_foundation.rs` + in-module unit tests |

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
- TodoState stays bounded and independent; M002 adds no Todo projection or
  writing integration (M003).
- Continuation checkpoints remain distinct with their own lifecycle; M002
  adds no checkpoint coupling (M004).
- No model-facing WorkPlan tools in M002; no Todo/runtime behavior change.

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

## Testing

```bash
cargo test -p codegg-core -- work_plan
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
