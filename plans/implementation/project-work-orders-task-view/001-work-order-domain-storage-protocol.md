# Project Work Orders and Task View M001 — WorkOrder Domain, Storage, Authorization, and Protocol

Status: ready

Repository baseline: `3ed785618bfac5a85f504813bdb7fc3a923e8679`

Source roadmap:

- `plans/subsystems/project-work-orders-task-view-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#6-canonical-identity-relationships`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#22-audit-architecture`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADR:

- `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`

Primary class: infrastructure / invariant

Hard dependencies: none beyond accepted ADR-0005 and closed project/session/jobs/authorization/storage foundations.

## 1. Objective

Add the bounded, revisioned, restart-durable project-level `WorkOrder` domain and its public daemon protocol without starting WorkOrders yet.

This milestone establishes the authoritative identities, lifecycle/storage semantics, release-gate description, sequence-lane ordering, project authorization, origin attribution, audit/event shapes, and bounded projections that M002-M007 consume.

## 2. Current implementation evidence

- `ProjectCatalog` already gives stable project identity and project→workspace/session bindings.
- `SessionStore` persists real human-facing sessions and must not receive speculative future sessions.
- `JobStore`/`ScheduleStore` own durable execution/schedule primitives; they are not project-task intent stores.
- `WorkPlan` already owns detailed within-session work state and explicitly excludes generic workflow/user automation.
- `AgentTask`/`TaskTool` already mean delegated child intent and must not be reused for project WorkOrders.
- Authorization already has direct-project/via-session/via-job scope resolution, project privacy filtering, immutable origin attribution, and exhaustive `CoreRequest` classification.
- The current TUI `/tasks` surface is a presentation adapter over `ScheduleCreate/List/Get/Delete`; M001 does not replace that UI yet.

## 3. Invariants that must not regress

- `WorkOrderId`, `WorkOrderOccurrenceId`, and `SequenceLaneId` are distinct typed IDs.
- WorkOrder is project intent, not scheduler execution authority.
- Waiting WorkOrders do not create Session rows.
- WorkOrder store does not execute jobs, allocate worktrees, or invoke models.
- No second schedule/job/retry owner is introduced.
- All list/get/mutate APIs are project-authorizable; ID-only operations resolve owning project daemon-side before capability evaluation.
- Stale WorkOrder/lane writers receive explicit conflict; no last-writer-wins reorder.
- Prompt/title/gate/list/repeat fields are bounded before persistence and projection.
- Trigger secret material is not introduced in M001.
- No hidden reasoning field exists.

## 4. Scope

### In scope

- canonical terminology/spec amendment authorized by ADR-0005;
- typed IDs and enums;
- `WorkOrder`, `WorkOrderOccurrence`, `SequenceLane` records;
- bounded release-gate specification and finite repeat policy shape;
- lifecycle validation and attention/error reason codes;
- additive SQLite schema/migration and typed store/service;
- CAS update/reorder semantics;
- project-scoped CoreRequest/CoreResponse DTOs/events/capabilities;
- daemon authorization matrix integration;
- origin attribution and audit/event correlation;
- bounded project WorkOrder query/projection;
- idempotent single and batch submission primitives required by later agent tooling;
- migration/restart/contention/security tests;
- architecture documentation.

### Explicitly out of scope

- evaluating release gates;
- starting sessions/jobs;
- worktree allocation;
- TUI Task mode/dashboard;
- HTTP trigger endpoint;
- model-visible WorkOrder tool;
- automatic migration of existing Schedule rows;
- arbitrary workflow expression language.

## 5. Required domain contract

Exact type names may vary only if semantics remain explicit. Minimum shape:

```text
WorkOrder
  id: WorkOrderId
  revision
  project_id
  creator_principal_id / origin attribution reference
  parent_session_id? / parent_turn_id? / parent_work_order_id?
  title?
  prompt or immutable prompt payload reference
  requested provider/model identity?
  requested approval mode / sandbox profile / workspace policy
  gate_spec
  sequence_lane_id? / sequence_position?
  repeat_policy
  state
  created_at / updated_at / cancelled_at?

WorkOrderOccurrence
  id: WorkOrderOccurrenceId
  work_order_id
  occurrence_index
  state
  gate_latches
  not_before / next_check_at?
  session_id? / job_id? / workspace_id? / worktree_id?
  claimed_at / started_at / terminal_at?
  attention_code? / bounded diagnostic?

SequenceLane
  id: SequenceLaneId
  project_id
  revision
  label?
  continuation/failure policy
  created_at / updated_at
```

The store may normalize lane membership separately rather than persisting `sequence_position` directly on WorkOrder, but one authority must own ordering and CAS revision.

## 6. Release-gate/repeat description semantics

M001 persists/validates but does not execute the closed gate set:

- Immediate/zero delay;
- Delay(duration);
- NotBefore(timestamp);
- SequenceReady(lane);
- ExternalTrigger(trigger locator/reference placeholder, without secret storage yet).

Multiple gates use exactly one `All` or `Any` join policy. Reject empty/invalid combinations, duplicate incompatible gates, negative durations, timestamps outside supported bounds, and references to another project.

Repeat policy is finite and bounded. Define a hard maximum count. Occurrence 0/1 indexing must be explicit and tested. Do not represent indefinite repetition with a magic large integer.

## 7. Lifecycle and transitions

Define closed WorkOrder/occurrence state machines. Suggested separation:

- WorkOrder/template: active | paused | completed | cancelled | archived;
- occurrence: waiting | ready | claiming | running | needs_attention | completed | failed | cancelled.

M001 may leave runtime-only states unused, but transition validation must be ready for M002. Terminal transitions are one-way. `NeedsAttention` is non-terminal only if an explicit resume/retry transition exists.

Edits to prompt/model/gates/lane/repeat are permitted only while no occurrence that depends on those values has been claimed, unless the exact field is documented as mutable for future occurrences only.

## 8. Storage and migration

Add the next sequential migration and bump `STORAGE_LAYOUT_VERSION` atomically with migration wiring.

Prefer normalized tables for WorkOrder, occurrence, lane, and lane membership. Bounded JSON is acceptable for small closed gate/policy structures if validation and query ownership remain simple.

Required constraints/indexes include:

- project lookup and active/future ordering;
- unique occurrence index per WorkOrder;
- unique lane position or equivalent deterministic ordering per project/lane;
- idempotency/submission-key uniqueness;
- revision checks;
- bounded lifecycle/status values;
- foreign-key/project-scope consistency where practical.

Do not backfill old `schedule` rows into WorkOrders by guesswork. Existing databases gain empty WorkOrder tables.

## 9. Store/service operations

Minimum host API:

- create WorkOrder with idempotency key;
- atomic bounded batch create with optional lane/order;
- get by ID;
- list bounded by project/state/cursor;
- update waiting WorkOrder with expected revision;
- cancel/pause/resume as lifecycle permits;
- create/get/list occurrence records;
- create/get/reorder lane with expected lane revision;
- atomically move one waiting WorkOrder before/after another;
- resolve owning project for every opaque ID;
- fetch bounded project summary counts needed by later projections.

Batch creation must be all-or-nothing and establish deterministic positions in one transaction.

## 10. Protocol and authorization

Add bounded DTOs and capability negotiation. Prefer direct-project requests where possible:

- `WorkOrderCreate` / `WorkOrderBatchCreate`;
- `WorkOrderList`;
- `WorkOrderGet`;
- `WorkOrderUpdate`;
- `WorkOrderCancel` / pause-resume if retained;
- `WorkOrderLaneGet/List/Reorder`;
- optional `WorkOrderCapabilities`.

ID-only requests must resolve the owning project before authorization. Extend `operation_descriptor` exhaustively.

Map capabilities conservatively. Creation must require authority sufficient to create/invoke the future session; read/list uses project/session read semantics; cancel/reorder/update need explicit existing semantic capabilities. Do not introduce broad local-owner-only opaque operations for the primary user-facing API.

Capture creator origin attribution on creation. Audit subsequent actor mutations with decision IDs/revisions without storing full prompts when policy does not require content retention.

## 11. Canonical planning/documentation amendment

ADR-0005 explicitly authorizes the product-direction amendment. Update the minimum relevant canonical text:

- add `WorkOrder` / `WorkOrderOccurrence` / `SequenceLane` definitions to `plans/001-terminology-and-domain-model.md`;
- add WorkOrders to top-level/project relationships in `plans/000-long-term-specification.md` where sessions/jobs are enumerated;
- clarify that project Task/Workspace UI is a projection and does not redefine canonical `Workspace`;
- retain the non-goal against a general distributed workflow engine.

Do not rewrite unrelated canonical history.

## 12. Ordered work packages

### A — Canonical domain and bounds

Define IDs, gate/repeat/lifecycle types, validation, bounds, and project-scope rules with unit tests.

### B — Additive durable schema/store

Add migration, tables/indexes, CAS/idempotency/batch transactions, reopen tests, and storage layout guard updates.

### C — Authorization and attribution

Add project resolution for every WorkOrder ID, operation descriptors, capability mapping, origin attribution, privacy-negative tests, and audit/event envelopes.

### D — Protocol and projections

Add bounded DTOs/request/response/event types, list cursors/limits, summary counts, capability negotiation, and protocol compatibility tests.

### E — Architecture/canonical docs

Land the ADR-required terminology/spec amendment plus `architecture/work_orders.md` documenting ownership and the M001 non-execution boundary.

## 13. Failure, cancellation, restart, and contention semantics

- Transaction failure creates no partial batch/lane positions.
- Duplicate submission key returns the existing canonical object(s) or an explicit mismatch conflict.
- Stale WorkOrder/lane revision makes zero mutation.
- Concurrent reorder is serialized by lane revision/transaction, not last-write-wins.
- Reopen preserves exact WorkOrder/occurrence/lane identities and revisions.
- Cancellation before execution marks future eligibility inert; M002 defines in-flight propagation.
- Cross-project parent/lane/gate references fail closed.
- Unknown enum values follow the project's existing forward-compatibility policy; they must never become executable by default.

## 14. Required tests

Focused:

- ID parsing/serialization;
- field/list/count bounds;
- gate combination validation;
- finite repeat validation;
- lifecycle transition matrix;
- lane position/reorder/CAS behavior;
- batch atomicity/idempotency.

Integration:

- create/list/get/update/cancel;
- project-filtered pagination;
- concurrent reorder writers;
- duplicate submission keys;
- file-backed reopen;
- migration from pre-M001 DB with no speculative rows.

Authorization/security:

- unauthorized project list filters/hides rows;
- opaque WorkOrder ID cannot leak project existence;
- cross-project update/reorder rejected;
- origin attribution immutable on create;
- bounded DTO/event content contains no hidden reasoning/credentials.

## 15. Required verification

```bash
cargo test -p codegg-core -- work_order
cargo test -p codegg-protocol -- work_order
cargo test --test authorization -- work_order
cargo test -p codegg-core -- migration
python3 scripts/check_authorization_matrix.py
python3 scripts/check_project_catalog_invariants.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

Use current equivalent target names if the repository changes; closure must record exact commands/outcomes.

## 16. Acceptance criteria

- WorkOrders/occurrences/lanes survive restart with stable typed IDs/revisions.
- Waiting WorkOrders exist without Session rows.
- Atomic batch creation can create an ordered lane segment without partial commit.
- Concurrent reorder cannot silently lose another client's change.
- Every user-facing operation is project-authorizable and privacy-safe.
- Existing Schedule/Job/Session/AgentTask/WorkPlan behavior remains unchanged.
- No WorkOrder executes yet.
- Canonical terminology/spec and architecture docs accurately state the new domain boundary.

## 17. Stop conditions

Stop and revise the plan if implementation requires converting WorkOrder into a Job/Schedule/Session/AgentTask/WorkPlan alias, adding a second scheduler, storing unbounded workflow expressions, or exposing opaque local-owner-only APIs as the primary team-facing surface.

## 18. Closure evidence required

- implementation/migration commits;
- schema/table/index and bounds summary;
- lifecycle/CAS/idempotency requirement matrix;
- authorization matrix entries and privacy-negative evidence;
- file-backed restart and concurrent reorder outcomes;
- protocol compatibility results;
- canonical terminology/spec diff summary;
- architecture ownership evidence;
- exact verification commands and residual findings.
