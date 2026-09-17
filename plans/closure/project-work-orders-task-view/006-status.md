# Project Work Orders and Task View M006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-work-orders-task-view/006-agent-work-order-tool-and-batches.md`

Source subsystem roadmap:

- `plans/subsystems/project-work-orders-task-view-roadmap.md#7-milestones`

Repository baseline reviewed: `3ed785618bfac5a85f504813bdb7fc3a923e8679`
(M006 plan baseline; implementation developed on the M005-closure tree)

Implementation commits or pull requests:

- `38e533a8` — work-orders M006: agent WorkOrder tool and atomic batches
  (dedicated `work_order` tool/service adapter, factory registration,
  authority/lineage/idempotency adapter, single/batch creation with lane
  and `start_first_now`, durable fan-out/depth/repeat/total-bytes limits,
  compact results, docs, focused unit + integration tests) — commit hash
  recorded at commit time below.

## 1. Executive finding

M006 is complete. An authorized agent creates one project WorkOrder or one
atomic ordered batch through a dedicated model-visible `work_order` tool
that is semantically separate from delegated `task`/TaskTool: bounded
single creation and 1..=16-item batch creation commit through the canonical
M001 `WorkOrderService` with deterministic lane ordering; `start_first_now`
makes the first member immediate and the rest sequence-ready without
bypassing `WorkOrderCoordinator`; parent session/turn/run/WorkOrder lineage
is host-derived; approval/sandbox ceilings narrow (broader requests fail
closed); default model comes from the creating session effective model,
never the human Task-mode preference; fan-out/depth/repeat/total-bytes
limits are durable and survive restart; invocation-scoped idempotency
converges retries without cross-call dedupe; results are compact and
secret-free; trigger secrets never cross the tool. The plan-file queue user
story works with ordinary file tools plus one batch call. This is
capability/security work as the plan classifies it; M007 is unblocked by
this closure.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| Dedicated tool name/schema, TaskTool unchanged (§5, §14A) | `WorkOrderTool::name() == "work_order"`, `TaskTool::name() == "task"`; factory registers both; description states durable future sessions, NOT delegated runs; `factory_exposes_work_order_without_changing_task` | pass | No second `task` tool; no TaskTool rename |
| Read/write category/risk, factory wiring, model-profile availability, docs (§14A) | `ToolCategory::Mutating`; factory registers when pool+project exist; deferred (discoverable via `tool_search`, like `work_plan` tools); `architecture/work_orders.md` M006 section | pass | Plan-mode excludes mutating tools by construction |
| Inspect/list capability, bounded outputs (§5) | `list`/`get` return `{id, state, short_title}` + previews, never full prompts; `list_and_get_are_bounded_and_project_authorized` | pass | Limit clamped 1..=32, items capped to batch max |
| Single creation narrow fields (§6) | `prompt`/`title`/`delay_secs`/`not_before_ms`/`sequence_lane_id`/`new_lane_label`/`repeat_count`/`requested_model`/`approval`/`sandbox`/`workspace_policy`; `single_create_records_lineage_and_compact_result` | pass | Gates derived host-side; no raw gate JSON input |
| Default is explicit immediate vs sequence (§6) | Lane-free single is immediate; lane single is sequence_ready; batch lane-free is all immediate; batch lane is first-immediate-or-all-sequence by `start_first_now` | pass | Never inherits Task composer last-used prefs |
| Atomic sequential batch, bounds, lane/order/start-first (§7, §14C) | M001 `batch_create_work_orders` transaction + deterministic positions; batch 16, total 64 KiB, per-item prompt 32 KiB; `batch_creates_ordered_lane_with_first_immediate`, `batch_start_first_now_releases_only_first_through_coordinator` | pass | Lane creation idempotent on invocation namespace; empty-lane-on-validation-failure documented (no WorkOrder partial) |
| Plans-directory workflow with ordinary file tools (§8) | Tool never scans directories; `plan_file_queue_uses_one_atomic_batch` proves synthetic plan list -> one batch -> ordered lane + titles, no body leak | pass | No unbounded traversal in tool |
| Lineage capture (§9, §14B) | Bound session/turn + durable session->occurrence lookup for `parent_work_order_id`; creator from `origin_principal` else bound; `single_create_records_lineage_and_compact_result` round-trips session/turn/creator/model | pass | Model parent/project fields rejected as host-owned |
| Authority/policy narrowing (§10, §14B) | Project override rejected; approval/sandbox rank-checked against `permission_mode`/`sandbox_profile` ceilings (broader rejected); model allowlist hook + claim-time backstop; trigger gates never created; `broader_yolo_and_full_host_are_rejected`, `cross_project_target_is_rejected`, `unauthorized_model_is_rejected_when_allowlist_set` | pass | Reviewer `caller_class` rejected defense-in-depth (allowlist already excludes `work_order`) |
| Fan-out/recursion/resource bounds (§11, §14D) | Batch 16; per-turn 32; pending/project 100; descendants/root 32; depth 4; agent repeat 8; total 64 KiB; durable SQL counts + chain BFS; `turn_budget_survives_restart`, `depth_limit_blocks_nested_self_replication`, `batch_bounds_are_exact` | pass | Counts survive file-DB close/reopen |
| Disable switch (§11) | `with_enabled(false)` rejects mutations with actionable error while human Task mode (CoreRequest path) is untouched | pass | Unit-pinned via builder default true |
| Idempotency/model retry (§12, §14B) | `aw:{invocation}[:{explicit}]:{single\|batch\|item#i\|lane-new}` truncated/hashed to 128 B as M001 key; `same_invocation_retry_returns_same_ids`, `changed_payload_under_same_key_conflicts`, `identical_text_in_distinct_calls_creates_distinct_rows` | pass | M001 digest mismatch surfaces `idempotency conflict` |
| Tool result/context budget (§13) | `{created, lane, items[{id, position, state, short_title}], first_release}`; prompts never echoed; `single_create_records_lineage_and_compact_result`, batch tests assert no body leak | pass | Errors typed as validation/authorization-ceiling/conflict/batch-bound/unavailable without secrets |
| Negative/retry/security qualification (§14E) | Duplicate retry, partial-validation abort, broader-authority, cross-project, model scope, trigger-secret negative, TaskTool non-regression (`subagent` 22 green) | pass | See §4 |
| Tool docs distinguish WorkOrders from TaskTool (§4) | Description + `architecture/work_orders.md` M006 section state future sessions vs live child runs | pass | — |

## 3. Production implementation evidence

Ownership landed:

- `src/tool/work_order.rs` (new, ~1490 lines incl. 7 unit tests):
  `WorkOrderTool` (`work_order` name, `Mutating`, `has_functional_backend`
  iff pool+project bound), actions `create`/`create_batch`/`list`/`get`,
  host-owned field rejection, project-scope check, approval/sandbox rank
  ceilings, session-model default, allowlist hook, single/batch gate
  derivation, lane create-or-resolve (idempotent), durable turn/project/
  depth/descendant enforcement, invocation-namespaced submission/batch/
  item keys, `ensure_occurrences` backfill (idempotent, converges after
  crash before occurrence step), bounded results, reviewer rejection.
  No scheduler/`AgentLoop`/session/job/worktree/provider construction;
  only `WorkOrderService` rows.
- `src/tool/mod.rs`: `pub mod work_order`.
- `src/tool/factory.rs`: registers `WorkOrderTool` when `pool` + bound
  `project_id` exist, with bound session/turn/effective-model
  (`parent_model.clone()` fix + `turn_id.clone()` for the pre-existing
  move). TaskTool wiring unchanged.
- `architecture/work_orders.md`: M006 section (contract, gates, lineage,
  ceilings, bounds, idempotency, results, ownership, test commands).
- Tests: `tests/work_orders_m006_agent_tool.rs` (new, 21 integration
  tests).

Constants (host-owned, pinned by `agent_repeat_cap_is_below_human_cap`):

- `MAX_AGENT_WORK_ORDER_BATCH = 16` (M001 32; WorkPlan 64)
- `MAX_AGENT_BATCH_TOTAL_BYTES = 65536`
- `MAX_AGENT_PROMPT_BYTES_PER_ITEM = 32768` (M001 bound; total cap bounds context)
- `MAX_AGENT_REPEAT_COUNT = 8` (human 256)
- `MAX_AGENT_WORK_ORDER_DEPTH = 4`
- `MAX_AGENT_CREATED_PER_TURN = 32`
- `MAX_AGENT_PENDING_PER_PROJECT = 100` (agent-lineage rows only)
- `MAX_AGENT_DESCENDANTS_PER_ROOT = 32`

Deliberate adjustments from the plan text (semantics preserved):

- No storage migration: M006 reuses M001 `work_order`/`sequence_lane*`/
  `work_order_batch`/`work_order_occurrence` columns. Occurrence backfill
  (`ensure_occurrences`) creates one `Waiting` occurrence per new WorkOrder
  so the M002 coordinator has a due row; batch WorkOrder/lane atomicity
  remains the M001 transaction, and occurrence backfill converges on
  retry (duplicate batch returns existing rows, then missing occurrences
  are created). A crash between WorkOrder commit and occurrence backfill
  recovers the same way.
- New-lane creation precedes batch commit on an invocation-namespaced
  lane key. A batch validation failure after lane creation leaves an
  empty lane but zero WorkOrders, preserving the invariant that no
  partial WorkOrder/lane-position set commits.
- Per-item policy overrides are rejected; batch policy is uniform. This
  keeps narrowing auditable and satisfies the plan's "bounded override
  only where explicitly allowed" with the narrowest coherent surface.
- Model availability at tool time is syntactic + optional allowlist
  (tests pin rejection); semantic availability remains enforced at
  M002 claim (`model_unavailable` attention, never silent fallback).
  This matches TaskTool, which also defers provider resolution.
- Creator defaults to `origin_principal` when the daemon threads it,
  else the bound creator (`local-owner` in tests/standalone). Daemon
  CoreRequest audit remains the human-path audit; agent-path attribution
  is the durable `creator_principal` + parent lineage + invocation key,
  same posture as TaskTool delegation records.
- `list` limit clamps to 32 (tighter than M001 list 100) and echoes at
  most one batch worth of items, keeping model context bounded.

## 4. Verification executed

### Commands run (local; no CI lane added per verification policy)

```bash
cargo test -p codegg --lib -- tool::task
cargo test -p codegg --lib -- work_order
cargo test -p codegg-core --lib -- work_order
cargo test -p codegg-protocol
cargo test --test work_orders_m001_foundation
cargo test --test work_orders_m002_materialization
cargo test --test work_orders_m004_dashboard
cargo test --features server --test work_orders_m005_trigger
cargo test --test work_orders_m006_agent_tool
cargo test --test identity_m003_daemon_authorization
cargo test --test subagent
cargo test --test storage_migrations
python3 scripts/check_execution_ownership.py
python3 scripts/check_authorization_matrix.py
python3 scripts/check_work_order_coordinator.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
```

Plan §16 literal-to-actual mapping (recorded, not hidden):

- `cargo test -p codegg-core -- work_order`: literal filter matches the
  lib target only with `--lib`; substitute is
  `cargo test -p codegg-core --lib -- work_order` (43 passed).
- `cargo test --test authorization -- work_order` / `cargo test --test agent_run -- work_order`:
  no `tests/authorization.rs` / `tests/agent_run.rs` targets exist (same
  substitution as M001/M004/M005 closures); substitutes are
  `identity_m003_daemon_authorization` (9, project auth/privacy) and
  `subagent` (22, TaskTool non-regression), plus the authorization matrix
  guard.
- Clippy ran with the repo-conventional `--locked` flag added.

### Results (local)

- `codegg --lib tool::task`: 4 passed (TaskTool identity/delegation unchanged).
- `codegg --lib work_order`: 37 passed (7 new `tool::work_order` unit +
  coordinator/TUI work-order suites; no regression).
- `codegg-core --lib work_order`: 43 passed (35 pre-existing + 8 trigger unit).
- `codegg-protocol`: 185 passed (no DTO change; M006 reuses M001 DTOs).
- M001 12 / M002 10 / M004 8 — no regressions.
- M005 suite (server): 22 passed — no trigger regression.
- M006 suite: 21 passed (factory separation, single lineage + session-model
  default, ordered batch + lane + gate shape, coordinator first-only
  release, abort/no-partial, lane-conflict abort, exact bounds, retry/
  conflict/distinct, ceiling/allowlist/cross-project negatives,
  secret-free outputs, turn-budget file-DB restart, depth chain,
  plan-file queue trajectory, bounded list/get).
- `identity_m003` 9 / `subagent` 22 / `storage_migrations` 4. All green.
- Guards: execution-ownership ok; authorization matrix verified;
  work-order coordinator ownership ok; core-boundary pass; fmt clean;
  clippy (`--workspace --all-targets --locked`) clean; `verify.sh quick` pass.

## 5. Invariant review

| Plan §3 invariant | Evidence |
|---|---|
| TaskTool still means delegated work; WorkOrder is distinct | Names `task` vs `work_order`; factory registers both; description pins future-sessions vs live runs; `subagent` suite green |
| Model never receives trigger secrets/credentials | Tool never creates trigger gates, never returns `cggtr_`/`secret`/`verifier`; list/get censuses assert absence |
| Agent WorkOrders project-scoped with exact lineage | Bound project enforced; parent session/turn from bound scope; parent WO via durable lookup; creator stored; lineage test round-trips |
| No broader project/approval/sandbox/filesystem/model/workspace authority | Cross-project rejected; approval/sandbox rank-rejected (never narrowed silently); full_host/yolo require yolo+full_host caller; model allowlist + claim backstop; workspace policy parsed but never widens |
| Batch atomic | M001 transaction; invalid item N and lane conflict leave zero WorkOrders (tests assert row count 0) |
| Fan-out/depth/repeat/rate bounded | Batch 16 / turn 32 / project-pending 100 / descendants 32 / depth 4 / repeat 8 / total 64 KiB; restart-pinned |
| No recursive self-invocation via reviewer/special agent | Reviewer allowlist excludes `work_order`; tool rejects `approval-reviewer` caller and reviewer-flavored agent ids |
| Model assertion is not permission | Mutating category (permission prompt); deterministic rank checks; approval-router ownership untouched |
| Default model from session, not Task preference | Bound `parent_model` (session effective) used; Task-preference store never read; dedicated test pins `claude/sonnet` default |

## 6. Failure and recovery review

- Duplicate delivery: invocation-namespaced submission/batch/item keys
  converge (`duplicate` path returns stored rows); conflicting payload
  under same key is explicit `idempotency conflict`; distinct calls with
  identical text create distinct rows.
- Stale writers: lane get validates project scope before batch; lane
  revision conflicts surface `conflict/stale lane` with zero mutation
  (M001 CAS preserved).
- Concurrent batches: M001 unique-violation mapping converges or
  conflicts explicitly; 16-way-style races remain owned by M001/M002
  primitives (suites green).
- Restart: file-DB close/reopen/remigrate preserves WorkOrders, lanes,
  occurrences, batch ledger; turn-budget and descendant/depth checks
  re-query durable rows (restart test pins turn budget); occurrence
  backfill converges after a crash between WorkOrder commit and
  occurrence creation.
- Partial persistence: batch WorkOrders + lane positions commit together;
  lane-then-batch leaves at most an empty lane on validation failure
  (documented, no WorkOrder partial); occurrences backfill idempotently.
- Malformed input: empty prompt, over-count/over-bytes/over-repeat,
  unknown action/state/lane, host-owned fields, dual lane selectors all
  fail closed with typed messages.
- Cancellation: pre-claim WorkOrders remain ordinary waiting rows
  cancellable through the existing `WorkOrderCancel` path (unchanged).
- Sequence: first-immediate + rest-sequence_ready holds downstream until
  predecessor terminal; `HoldLane` default preserved; coordinator test
  pins only-first-ready.
- Contention: per-turn/project/descendant/depth checks bound retries;
  receipt-free (M001 ledger is the convergence record).

## 7. Migration and compatibility review

- No migration: M006 reuses M001 tables (`work_order`,
  `sequence_lane`, `sequence_lane_member`, `work_order_batch`,
  `work_order_occurrence`); `STORAGE_LAYOUT_VERSION` unchanged
  (`storage_migrations` 4 green).
- Protocol additive: no new `CoreRequest`/`CoreResponse`/event variants;
  `PROTOCOL_VERSION` unchanged. Old clients never send `work_order`
  tool calls; old servers lack the tool (fail closed via unknown tool).
- Tool-surface compatibility: `task` definitions unchanged; `work_order`
  is additive and deferred (discoverable via `tool_search`, like
  `work_plan` tools); plan-mode excludes it as mutating.
- Rollback: downgrading drops the tool; WorkOrder rows created through
  it remain ordinary M001 rows readable through list/get and the Task
  view.

## 8. Security review

- Authorization: tool is scoped to the daemon-bound project at
  construction; ID-only lane lookups resolve server-side via the service
  (`get_lane` in-project, else privacy-preserving `work order not found`);
  denials leak no other-project existence.
- Privacy: list/get carry identity/state/short titles + previews only;
  no prompt bodies, paths, secrets, credentials, or reasoning.
- Secrets: no trigger bearer created/returned/logged; DTO/event JSON
  censuses assert `cggtr_`/`secret`/`verifier` absence.
- Isolation truthfulness: tool never claims isolation; workspace policy
  is a snapshot re-resolved at M002 claim.
- Bounds as DoS control: item/total/repeat/depth/turn/project/
  descendant caps; cursor-bounded listing; key lengths truncated/hashed
  to 128 B; diagnostics bounded.
- Attribution: creator + parent session/turn/WO + invocation key are
  durable; reviewer invocation rejected.

## 9. Documentation and operations

- `architecture/work_orders.md`: M006 section (contract, gates, lineage,
  ceilings, bounds, idempotency, results, ownership, test commands).
- Operator diagnostics: tool errors surface as `code: message`
  (`validation:`, `authorization/project ceiling:`, `conflict/stale lane:`,
  `batch bound:`, `idempotency conflict:`, `service unavailable:`);
  success is the bounded JSON above.
- Static guards: authorization matrix, work-order coordinator ownership,
  execution-ownership — all green (no new scheduler site; tool performs
  no process/job/subagent dispatch).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | No dedicated `tests/authorization.rs` / `tests/agent_run.rs` for the plan's literal verification lines | Substituted with named existing targets + guards (same surfaces, consistent with M001/M004/M005 closures) | None; future plans should name existing targets |
| low | Daemon wake driving for non-trigger WorkOrders remains on explicit wake (carried from M002) | Agent batches create release-ready occurrences, but autonomous timer/predecessor wakes still belong to a later hook | M007 to qualify explicit-wake trajectories; no corrective pass (documented semantics) |
| low | Claim-time approval/sandbox ceilings remain `None` (carried from M002) | Per-turn ceilings still enforced on `TurnSubmit`; snapshots never widen | Future milestone may thread ceilings into claim; unchanged behavior |
| low | New-lane-then-batch leaves an empty lane when batch validation fails | No WorkOrder partial commit; one empty lane row remains | None for M006; a future polish pass may garbage-collect empty lanes |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone closed and the next dependency may proceed: M006 exit
condition is satisfied (an authorized agent atomically enqueues a bounded
ordered plan-file queue, starts the first immediately through the
coordinator, and cannot create unbounded or broader-authority persistent
work).

Blocked-work audit (registry `Blocked work` + roadmap §6 dependency
graph):

- M006 (agent WorkOrder tool, hard dep M002/M001 service satisfied;
  ordering gate M005 satisfied) → **closed** by this record.
- M007 (qualification, hard dep M001-M006) → **ready**. All six hard
  dependencies are now closed (M001 `a856f2e0`, M002 `c54656e5`, M003
  `79a00460`, M004 `8c6e8190`, M005 `f22c9d8d`, M006 `38e533a8`); no
  interface dependency remains without a stable contract.
- Dependency-security M005 remains independently blocked on the
  generalized external updater interface (unchanged).

## 12. Registry updates

- Move M006 (`006-agent-work-order-tool-and-batches.md`) from ready to
  closed with this closure record (implementation `38e533a8`).
- Move M007 (`007-work-order-trajectory-and-recovery-qualification.md`)
  from blocked to ready: hard dependencies M001-M006 are now all closed.
- Record M006 under recently closed work; update the subsystem roadmap
  M006 status to closed and M007 to ready; mark the M006 plan file
  implemented.
