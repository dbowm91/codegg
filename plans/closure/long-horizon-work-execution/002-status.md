# Long-Horizon Work Execution M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/long-horizon-work-execution/002-durable-work-plan-foundation.md`

Source subsystem roadmap:

- `plans/subsystems/long-horizon-work-execution-roadmap.md#7-milestones`

Repository baseline reviewed: `18365458f881f6ac4524c9ea05224b69923faa4f`

Implementation commits or pull requests:

- `0aceb37d` — long-horizon M002: durable WorkPlan foundation

## 1. Executive finding

M002 is complete. `codegg-core` now owns a bounded, revisioned,
restart-durable `WorkPlan`/`WorkItem` domain with additive migration v59
(`STORAGE_LAYOUT_VERSION` 58 → 59), CAS semantics, deterministic dependency
gating, typed acceptance/evidence refs, and an optional Goal-binding seam
with validated same-session/project ownership. A 20-item plan (larger than
TodoState's 10-12 projection cap) round-trips and reloads after file-backed
reopen with stable IDs/revisions; stale writers receive explicit
`Conflict` rather than last-write-wins; dangling evidence refs stay explicit
and never auto-satisfy; completing a bare item with no host signal is
refused. No model-facing tools, Todo projection/writing, completion
continuation, context-epoch, workflow-scheduler, or CI-lane changes were
introduced. This unblocks M003.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Core typed IDs and enums (§5/§6) | `work_plan/model.rs::WorkPlanId` (`wp_`)/`WorkItemId` (`wi_`), `WorkPlanStatus`, `WorkItemStatus`, `WorkAcceptanceDisposition`, `WorkEvidenceKind` | pass | Distinct types + prefixes from `AgentTaskId`/`AgentRunId`/`JobId`/Goal/Todo IDs; `ids_carry_distinct_prefixes` |
| Plan/item shape (§6) | `WorkPlan` (id/revision/session/project/origin-turn/goal/objective+digest/provenance/status/phase/current-item/timestamps), `WorkItem` (id/plan/revision/position/parent/deps/status/description/acceptance/evidence/owner-run/job/attempts/blocker/next-action/timestamps) | pass | Objective/provenance immutable; phase/current-item/goal-binding evolve via CAS |
| Bounded acceptance/evidence/blocker (§5/§6) | `WorkAcceptance{description,disposition,note}`, `WorkEvidenceRef{kind,ref_id,detail}`, blocker rule; bound table §3 | pass | `validate_acceptance`, `validate_evidence_ref`, `validate_item_blocker_rule`; oversized lists rejected |
| Owner/run/job provenance refs (§5) | `owner_run_id`/`owner_job_id` stored, shape-validated, never authority | pass | `owner_refs_are_provenance_only`; rg shows no tool/agent/TUI consumer |
| Additive migration + layout bump (§6) | `migrate_v59` + `WORK_PLAN_SCHEMA_STATEMENTS`, `STORAGE_LAYOUT_VERSION` 58 → 59 | pass | `migration_is_additive_and_layout_tracks_v59`; catalog guard 7/7 |
| Store APIs (§6) | `WorkPlanStore::{create_active,get,active_for_session,active_for_goal,list_items,get_item,actionable_items,add_item,update_item,transition_item,update_plan_meta,transition_plan,bind_goal,unbind_goal}` | pass | One store/service owner; see §3 |
| CAS updates, no last-write-wins (§6/§8) | Plan-revision + per-item-revision checks; every item mutation bumps plan revision in-tx; `WorkPlanError::Conflict{expected,found}` | pass | `item_lifecycle_with_cas`, `plan_cas_serializes_against_item_mutation`, `concurrent_item_writers_serialize_with_explicit_conflict` |
| Dependency/actionability (§6) | `validate_item_graph` (same-plan membership, self/cycle rejection, bounded DFS) + `actionable_items` (Pending/Actionable + all deps Completed, stable `(position,id)` order) | pass | `actionability_gates_on_completed_dependencies`, `cycle_rejected`, `oversized_cyclic_malformed_rejected_before_persistence` |
| Evidence-ref validation, no inference (§6) | Closed kind set + bounded id/detail; completion floor requires acceptance/evidence/owner signal; missing targets stay explicit | pass | `evidence_shape_validated`, `completion_requires_host_signal`, `missing_evidence_target_is_unavailable_not_satisfied` |
| Goal binding seam, no Goal behavior change (§6) | `bind_goal`/`unbind_goal` with exact-Goal existence + same-session/project scope check; at most one active plan per goal and per session | pass | `second_active_plan_per_goal_rejected_across_sessions`, `goal_replacement_binding_race_fails_closed`, `cross_session_goal_binding_rejected`, `missing_goal_binding_rejected` |
| Scope validation (§6) | Session/project non-empty ≤256; goal/turn refs bounded; deps/parent same-plan; cross-plan/cross-session rejected | pass | `update_item_patch_validates_bounds_and_scope`, cross-session tests |
| No hidden reasoning (§4) | No reasoning field; digest covers objective only; serialization guard | pass | `no_reasoning_payload_exists`, `origin_digest_stable_and_objective_only` |
| Docs (§12) | `architecture/work_plan.md` new; `goal.md`, `model_profile_task_state.md`, `session.md` updated | pass | See §9 |
| No new guard required (§6) | Existing `check-core-boundary.sh` + `check_project_catalog_invariants.py` cover ownership; rg ownership census in §9 | pass | No CI lane added per verification policy |

## 3. Production implementation evidence

Core/domain (`codegg-core`, boundary-clean):

- `crates/codegg-core/src/work_plan/model.rs` (new, ~700 lines with tests):
  bounds (`MAX_ITEMS_PER_PLAN = 64`, objective 4000, provenance 1024,
  phase 256, description 1024, deps 16, acceptance 16×512+512, evidence
  16×512+512, blocker/next-action 1024, owner refs 256, scope IDs 256),
  `WorkPlanId`/`WorkItemId`, statuses, acceptance/evidence types,
  `NewWorkPlan`/`NewWorkItem`/`WorkItemPatch`, `WorkPlanError`
  (`Validation`/`Conflict`/`NotFound`/`ScopeMismatch`/`Terminal`/`Storage`),
  `work_plan_origin_digest` (SHA-256 `sha256:` of objective only),
  `validate_*`, `can_transition_item`, `can_transition_plan`,
  `validate_item_graph` (bounded DFS + parent-chain walk),
  `actionable_items`. Eleven unit tests.
- `crates/codegg-core/src/work_plan/store.rs` (new, ~1400 lines with tests):
  `WORK_PLAN_SCHEMA_STATEMENTS`, `WorkPlanStore` with the §6 API surface,
  JSON codecs for deps/acceptance/evidence with closed-enum parsing,
  `create_active` (session predecessor cancel in-tx, goal scope + per-goal
  uniqueness), item add/update/transition (CAS + plan-revision bump in-tx,
  attempts on `InProgress`, blocker discipline, completion floor),
  `update_plan_meta` (phase/current-item only; objective immutable;
  current-item same-plan check), `transition_plan`, `bind_goal`/`unbind_goal`
  (existence + scope + per-goal uniqueness, history-preserving). Nine
  store tests.
- `crates/codegg-core/src/work_plan/mod.rs` (new): re-exports.
- `crates/codegg-core/src/lib.rs`: registers `work_plan`.
- `crates/codegg-core/src/session/schema.rs`: `migrate_v59` + chain/dispatch
  wiring.
- `crates/codegg-core/src/storage/mod.rs`: `STORAGE_LAYOUT_VERSION` 58 → 59.

Tests:

- `crates/codegg-core/tests/work_plan_foundation.rs` (new): ten integration
  tests (migration, pre-migration upgrade, file-backed restart, concurrent
  writers, cancel-vs-completion race, goal bind race, cross-session
  rejection, dangling evidence, owner provenance, malformed rejection).
- `crates/codegg-core/tests/continuation_checkpoint.rs`: version asserts
  58 → 59 (highest-migration tracking; no behavior change).

### Schema and bound table

Migration v59 creates (all `IF NOT EXISTS`, length-checked):

- `work_plan(id PK, revision, session_id, project_id, origin_turn_id?,
  goal_id?, objective ≤4000, objective_digest, origin_provenance ≤1024,
  status ∈ active/blocked/completed/cancelled, current_phase?,
  current_item_id?, created_at, updated_at, completed_at?)` with
  `idx_work_plan_session_status`, `idx_work_plan_goal`.
- `work_item(id PK, plan_id FK CASCADE, revision, position, parent_item_id?,
  dependencies_json ≤4096, status ∈ six states, description ≤1024,
  acceptance_json ≤16384, evidence_json ≤16384, owner_run_id?,
  owner_job_id?, attempts, blocker?, next_action?, created_at, updated_at)`
  with `idx_work_item_plan`.

Domain bounds (enforced in `model.rs`, re-asserted in SQL):

| Field | Bound |
|---|---|
| Items per plan | 64 |
| Objective | 4000 chars |
| Origin provenance | 1024 chars |
| Current phase | 256 chars |
| Item description | 1024 chars |
| Dependencies per item | 16 |
| Acceptance per item | 16; description 512; note 512 |
| Evidence per item | 16; ref 512; detail 512 |
| Blocker / next action | 1024 chars each |
| Owner run/job refs | 256 chars each |
| Session/project/turn/goal IDs | 256 chars each |

### Transition, actionability, and CAS requirement matrix

Item (`can_transition_item`; `Blocked → Completed` never direct; terminal
states have no outgoing edges):

| From ↓ / To → | Pending | Actionable | InProgress | Blocked | Completed | Cancelled |
|---|---|---|---|---|---|---|
| Pending | = | ✓ | ✓ | ✓ | ✗ | ✓ |
| Actionable | ✓ | = | ✓ | ✓ | ✓ | ✓ |
| InProgress | ✗ | ✓ | = | ✓ | ✓ | ✓ |
| Blocked | ✓ | ✓ | ✓ | = | ✗ | ✓ |
| Completed | ✗ | ✗ | ✗ | ✗ | = | ✗ |
| Cancelled | ✗ | ✗ | ✗ | ✗ | ✗ | = |

Plan (`can_transition_plan`; terminal states have no outgoing edges):

| From ↓ / To → | Active | Blocked | Completed | Cancelled |
|---|---|---|---|---|
| Active | = | ✓ | ✓ | ✓ |
| Blocked | ✓ | = | ✓ | ✓ |
| Completed | ✗ | ✗ | = | ✗ |
| Cancelled | ✗ | ✗ | ✗ | = |

Actionability: `Pending`/`Actionable` + all deps `Completed`, stable
`(position, id)` order; terminal/in-progress/blocked never actionable.
CAS: plan ops require plan revision; item ops require item revision; every
item mutation bumps the owning plan revision in the same transaction, so a
stale plan cancel/complete fails with `Conflict{expected,found}`.
Blocker required exactly when `Blocked`. Completion requires a host signal
(non-`Unmet` acceptance, evidence ref, or owner run/job provenance).

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core --lib -- work_plan
cargo test -p codegg-core --test work_plan_foundation
cargo test -p codegg-core -- migration
cargo test -p codegg-core --test continuation_checkpoint
cargo test --test storage_migrations
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
bash scripts/check-core-boundary.sh
python3 scripts/check_project_catalog_invariants.py
```

Planned-command deviations (per plan §11, recorded here rather than hidden):

- `python3 scripts/check_core_boundary.py` does not exist; the repository's
  current focused equivalent is `bash scripts/check-core-boundary.sh` (run,
  pass).
- `cargo test -p codegg-core -- work_plan` as written matches no lib-target
  filter form in this repo layout; the current focused equivalents are
  `cargo test -p codegg-core --lib -- work_plan` (20/20) and
  `cargo test -p codegg-core --test work_plan_foundation` (10/10).
- `cargo test -p codegg-core -- migration` matches the two migration tests
  by substring (2/2); full migration coverage additionally comes from
  `cargo test --test storage_migrations` (4/4) and the continuation
  checkpoint migration test (11/11).

### Results

- `cargo test -p codegg-core --lib -- work_plan`: 20/20 pass (11 model +
  9 store).
- `cargo test -p codegg-core --test work_plan_foundation`: 10/10 pass
  (migration, pre-migration upgrade, file-backed restart, concurrent
  writers, cancel-vs-completion race, goal bind race, cross-session
  rejection, dangling evidence, owner provenance, malformed rejection).
- `cargo test -p codegg-core -- migration`: 2/2 pass.
- `cargo test -p codegg-core --test continuation_checkpoint`: 11/11 pass.
- `cargo test --test storage_migrations`: 4/4 pass.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `./scripts/verify.sh quick`: pass (fmt, builtin-agents check,
  core-boundary, sandbox contract, execution ownership, TUI project
  authority, workspace check).
- `bash scripts/check-core-boundary.sh`: pass (no UI/server/plugin deps).
- `python3 scripts/check_project_catalog_invariants.py`: 7/7 pass (layout
  marker tracks highest wired migration 59).

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| WorkPlan is detailed work state, not execution authority | No executor/scheduler calls; owner/job refs provenance-only; no replay on restart; `owner_refs_are_provenance_only` |
| `WorkPlanId`/`WorkItemId` distinct from task/run/job/goal/todo IDs | Newtypes with `wp_`/`wi_` prefixes; `ids_carry_distinct_prefixes`; rejects bare `goal-1` |
| Goal status/budget authoritative when Goal-bound | Binding is a scope-checked reference; no `GoalStore`/verifier/budget change; `goal.md` documents authority |
| TodoState bounded and independent | No todo imports/writes in `work_plan/`; `model_profile_task_state.md` documents no M002 integration; rg shows no tool/agent/TUI consumer |
| Revision/CAS; stale writers fail | `Conflict{expected,found}` on plan and item paths; plan bump on every item mutation; three conflict tests |
| Completed acceptance evidence cannot come from bare model text | Completion floor (acceptance/evidence/owner signal); `completion_requires_host_signal`; dangling refs stay explicit |
| Counts and text/list fields bounded before persistence/projection | Bound table §3; domain + SQL CHECKs; oversized tests |
| No hidden reasoning stored | No reasoning field; digest covers objective only; `no_reasoning_payload_exists` fails closed on `reasoning`/`thinking`/`chain_of_thought`/`scratchpad` |

## 6. Failure and recovery review

| Concern (§8) | Evidence |
|---|---|
| Transaction failure leaves prior revision intact | All mutations commit plan+item changes in one transaction; conflict paths return before commit; no partial-row test residue (malformed test asserts count) |
| Stale revision returns explicit conflict, no mutation | `Conflict` on plan and item CAS; `concurrent_item_writers_serialize_with_explicit_conflict`, `plan_cas_serializes_against_item_mutation` |
| Goal delete/cancel never silently deletes plan evidence | No `ON DELETE` from goal to plan; predecessor cancel retains rows/items (`replacement_cancels_predecessor_without_deleting`); unbind keeps the plan row |
| Missing Job/Run/Artifact is unavailable, not satisfied | Dangling ref round-trips as explicit ref; no availability inference in M002 (`missing_evidence_target_is_unavailable_not_satisfied`) |
| Restart loads exactly; `in_progress` implies no replay | File-backed reopen test asserts same revision/status/attempts with no automatic transition |
| Cycles rejected, traversal bounded | `validate_item_graph` + bounded DFS over ≤64 nodes; `cycle_rejected`; self/unknown/cross-plan rejected |
| Concurrent completion vs cancel serializes | Item completion bumps plan revision; stale cancel fails, fresh cancel wins (`cancel_racing_completion_serializes_deterministically`) |

## 7. Migration and compatibility review

Additive v59 with `IF NOT EXISTS` tables/indexes. Pre-migration databases
(with version rewound to 58 and new tables dropped) migrate to 59 with empty
`work_plan`/`work_item` and byte-identical Goal rows
(`pre_migration_database_gains_empty_tables_without_touching_goals`).
No Goal/Todo/session rows backfilled; legacy sessions read as no active
plan. Readers tolerate absent optional Goal/origin-turn bindings (`None`
maps to `NULL`). Rollback is a plain revert of `0aceb37d` with no data
backfill (v59 tables unused by older code). `storage_migrations` (4/4) and
continuation checkpoint migration (11/11) remain green.

## 8. Security review

Store operations carry owning session/project identity; Goal binding
re-checks existence plus same-session/project scope, and cross-session
binding fails (`cross_session_goal_binding_rejected`,
`second_active_plan_per_goal_rejected_across_sessions`). Item graph checks
enforce same-plan membership so cross-plan refs cannot smuggle scope.
A referenced run/job grants no capability (no authorization path consumes
these fields; TUI/tools do not read them in M002). All text/ID fields reject
NUL; scope IDs ≤256; no secrets logged (digests and reason codes only).
`codegg-core` boundary guard passes (no UI/server/plugin/auth deps).

## 9. Documentation and operations

- `architecture/work_plan.md` (new): module map, plan/item shapes, bound
  table, transition/actionability/CAS matrix, schema, ownership boundaries,
  invariants, tests.
- `architecture/goal.md`: WorkPlan-binding section (Goal authority
  preserved, M003 arbiter noted).
- `architecture/model_profile_task_state.md`: WorkPlan-relation section
  (TodoState stays bounded projection; no M002 integration).
- `architecture/session.md`: v59 `work_plan`/`work_item` group.
- Ownership evidence: direct SQL against `work_plan`/`work_item` appears
  only in `crates/codegg-core/src/work_plan/store.rs`, its tests, and the
  `migrate_v59` wiring in `session/schema.rs`; `rg WorkPlan|work_plan
  src/tool/ src/agent/ src/tui/` is empty (no model-visible surface). No new
  static-guard script was added per plan §6 ("only if needed"): the existing
  core-boundary and catalog-invariant guards already enforce the real
  ownership invariants, and no new CI lane was added per verification
  policy.
- Operator signals: `WorkPlanError::Conflict{expected,found}` diagnostics;
  existing Goal/Todo surfaces unchanged.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `actionable_items` is a read-time computation; `Actionable` status labels are not auto-rewritten when dependencies complete | A `Pending` item with satisfied deps reads as actionable without a status write; callers must use the computation, not the label alone | M003 projection must consume `actionable_items()`, not status equality; no M002 corrective pass required |
| low | Evidence availability (live vs missing job/run/artifact) is not resolved against canonical stores in M002 | Dangling refs are explicit but not classified `Unavailable` with liveness | M003 evidence correlation owns resolution; out of M002 scope by plan §5 |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone closed and next dependency may proceed. Dependency audit for the
unblock check (registry Blocked-work section + roadmap §6 dependency graph):

- M002 durable WorkPlan foundation: sole closure-sequence gate was M001
  closure (interface dep on ADR-0003 + closed session/storage already
  satisfied). Now closed → M003 moves to `ready`.
- M003 projection + completion arbiter: hard dep on M002 → now unblocked
  (`blocked` → `ready for handoff`).
- M004 context-epoch reset/handoff: hard deps on M003 + closed
  context-continuity → stays `blocked`.
- M005 trajectory qualification: hard deps on M001–M004; M001+M002 now
  closed → stays `blocked`, blocker narrows to M003–M004 closure.
- No other registered plan lists M002 as a hard/interface dep; no new
  corrective plan is required (§10 has no qualifying defect).

## 12. Registry updates

In the same closure commit:

- `plans/implementation/long-horizon-work-execution/002-...md`: `ready for
  handoff` → `implemented` (landed with `0aceb37d`).
- `plans/implementation/long-horizon-work-execution/003-...md`: `blocked` →
  `ready for handoff`.
- `plans/registry.md`: long-horizon M002 row `ready` → `closed` (closure +
  implementation refs); add M003 `ready` row; move M003 out of Blocked work;
  narrow M005 blocker to M003–M004; advance execution-order gate 1 and the
  long-horizon control row to M002 closed / M003 ready; append M002 to
  recently-closed.
- `plans/subsystems/long-horizon-work-execution-roadmap.md`: M002 milestone
  + §12 table row → closed with closure/implementation refs; M003 row →
  ready; M005 blocker narrows to M003–M004 closure.
