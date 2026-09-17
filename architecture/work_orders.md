# Project Work Orders

Daemon-owned durable project-level task intent (Project Work Orders
M001+M002). A `WorkOrder` records that some future work should happen in a
project, under which release conditions, in which sequence lane, and with
which requested model/policy snapshot. Waiting work orders are projected
as future work but are never scheduler jobs, schedule rules, delegated
agent tasks, or within-session work plans. M002 materializes ready
occurrences into ordinary canonical sessions exactly once through the
existing global scheduler boundary.

Long-term references: `plans/000-long-term-specification.md`
(`#4`, `#6`, `#13`, `#17`, `#22`, `#24`, `#26`, `#29`).
Governing decision:
`plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`.
Roadmap: `plans/subsystems/project-work-orders-task-view-roadmap.md`.

## Ownership

`codegg-core::work_order` owns the domain types, validation, release-gate
evaluation policy (`coordinator.rs`: deterministic latches, delay
deadlines, sequence-hold rules, workspace-policy resolution,
model/policy narrowing, idempotency keys), and the durable store
(`WorkOrderService`, including M002 claim/linkage/repeat primitives). It
is UI-, server-, plugin-, and auth-free: it takes already-resolved
`ProjectId` scopes, performs no authorization itself, never executes work
(no session creation, no job submission, no worktree allocation, no model
calls), and never references `AgentLoop`, `JobScheduler`,
`JobSubmissionService`, `SessionStore`, `WorktreeService`, or provider
registries (`scripts/check_work_order_coordinator.py` enforces this). The
daemon boundary (`src/core/daemon_work_orders.rs` for the M001 request
family; `src/core/work_order_coordinator.rs` for the M002
`WorkOrderCoordinator`) owns project resolution, capability gating,
origin attribution, audit emission, and event publication. Wire DTOs live
in `codegg-protocol::work_order`; the core `CoreRequest`/`CoreResponse`/
`CoreEvent` variants live in `codegg-protocol::core`.

Canonical ownership across layers:

```text
WorkOrder decides when a normal session may be born (M002 coordinator).
Session owns human/agent interaction once materialized.
Job/Scheduler owns admission and execution lifecycle.
Worktree/Workspace owns filesystem isolation and execution root.
WorkPlan owns detailed within-session completion state.
AgentTask owns delegated child intent below an AgentRun.
```

No layer may become a second owner of another layer's state machine.
In particular M001 introduces no second scheduler, admission queue,
background loop, or retry owner.

## Identity

Three distinct typed identities (`crates/codegg-core/src/identity.rs`):

- `WorkOrderId` — one durable project work order (project intent).
- `WorkOrderOccurrenceId` — one execution instance of a work order.
  Indexing is explicit and 0-based: occurrence 0 is the first
  execution; a `repeat_count` of N covers indices `0..N`.
- `SequenceLaneId` — one revisioned project sequence lane.

All three satisfy the shared identity lexical contract (opaque,
bounded, never path-derived) and are unrelated by type to `AgentTaskId`,
scheduler `JobId`, `ScheduleId`, and session identity.

## Records

`WorkOrder` carries revision, project, creator principal, optional
parent session/turn/work-order lineage, optional title, prompt (bounded
intent text), requested model/approval/sandbox/workspace-policy
snapshots, release-gate set, finite repeat count, lane membership
(resolved from lane rows, not stored on the work-order row), lifecycle
state, and timestamps. The original prompt/provenance is immutable once
an occurrence that depends on it has been claimed.

`WorkOrderOccurrence` carries occurrence index, state, gate latches,
scheduling hints, materialization references (absent until the M002
coordinator claims the occurrence), attention code plus bounded
diagnostic, and claim/start/terminal timestamps.

`SequenceLane` carries project, CAS revision, optional label, failure
policy (`hold_lane` default: failed/attention predecessors hold
downstream work), and the deterministic member order. Ordering authority
lives in the lane row plus the normalized `sequence_lane_member`
table — never in scheduler job dependencies.

## Release gates (description only in M001)

The closed gate set is `immediate`, `delay`, `not_before`,
`sequence_ready`, and `external_trigger`, combined by one explicit
`all`/`any` join. M001 persists and validates gate sets; evaluation
arrives in M002. Validation rejects empty/invalid combinations,
duplicate kinds, `immediate` combined with other gates, negative or
overlong delays, out-of-bounds timestamps, lane references outside the
owning project, and path-like trigger references (the M001 trigger
reference is an opaque same-project locator; M005 binds it to a stored
verifier). Unknown gate kinds fail closed. There is no expression
language, and indefinite repetition has no representation
(`repeat_count` is `1..=256`, where 1 means run once).

## Lifecycle

Work-order (template) states: `active | paused | completed |
cancelled | archived`. `active`/`paused` are the only editable states;
terminal transitions are one-way (`completed | cancelled -> archived`).

Occurrence states: `waiting | ready | claiming | running |
needs_attention | completed | failed | cancelled`. M001 creates
`waiting` records only; the matrix is validated now so M002 can claim
against it. `needs_attention` resumes explicitly to `waiting`;
terminal states never transition out. Edits to execution-shaping
fields (prompt, model, gates, repeat) are rejected once any occurrence
that depends on those values has been claimed.

## Storage (migrations v60–v63)

Additive tables, safe on existing databases (existing databases gain
empty work-order tables; no `schedule` row is ever backfilled):

- `work_order` — one row per work order, `revision >= 1`, bounded
  lifecycle/status values, `(project_id, submission_key)` idempotency
  uniqueness, per-item spec digest for mismatch detection.
- `work_order_occurrence` — one row per occurrence,
  `UNIQUE(work_order_id, occurrence_index)`, project-denormalized for
  scoped queries.
- `work_order_batch` — `(project_id, batch_key)` retry ledger mapping
  a batch key to its member ids plus the batch spec digest.
- `sequence_lane` — one row per lane, `revision >= 1`, per-project
  idempotency uniqueness.
- `sequence_lane_member` — normalized `(lane_id, work_order_id,
  position)` ordering with `UNIQUE(lane_id, position)`.

Indexes cover project/state/updated listings, occurrence lookup,
lane/project lookup, and lane ordering. `STORAGE_LAYOUT_VERSION` is 63
(`storage/mod.rs:39`); `session/schema.rs` wires `migrate_v60` (domain
tables), `migrate_v61` (admits the `work_order` origin-attribution
scope in the v53 table's kind CHECK via a row-preserving rebuild;
legacy attribution rows survive verbatim), `migrate_v62` (nullable
Task-composer model columns on `runtime_preferences`; existing rows
keep their session preference, approval, and sandbox untouched), and
`migrate_v63` (verifier-only `task_trigger` credentials plus the
`task_trigger_receipt` idempotency ledger; no trigger plaintext is
ever a column).

## Store/service operations

`WorkOrderService` (pool-backed; pool-less daemons fail durable
operations with `work_order_unavailable`):

- create with idempotency key (duplicate keys converge; reused keys
  with different payloads conflict explicitly);
- atomic bounded batch create with optional lane placement, committed
  in one transaction with deterministic positions;
- get by id and bounded project/state/cursor listing;
- update with expected revision (CAS; zero mutation on stale writes);
- cancel/pause/resume along the lifecycle matrix (repeating the
  current state is idempotent);
- occurrence create/get/list (explicit M001 primitive; M002 owns
  claims);
- lane create/get/list, CAS reorder (exact-set replacement),
  CAS attach (append/insert), and atomic before/after moves;
- owning-project resolution for every opaque id
  (`work_order_project`: work-order, occurrence, and lane tables;
  `task_trigger_project` for trigger locators);
- bounded per-project summary counts.

Transaction failure creates no partial batch or lane positions.
Concurrent reorder writers serialize on the lane revision. Reopen
preserves exact identities and revisions.

## Protocol and authorization

Bounded `CoreRequest`/`CoreResponse` DTOs plus capability negotiation
(`work_order_capabilities`). `project_id` requests resolve directly;
ID-only requests resolve the owning project server-side before
capability evaluation. Capability mapping is conservative: creation
requires `session.create` (authority sufficient to create the future
session); reads use `session.read`; cancel/update/reorder use
`session.create`. No broad local-owner-only opaque operation is the
primary surface. All denials use the privacy-preserving not-found
shape, indistinguishable from absent rows.

Creator origin attribution is captured on creation (immutable,
first-write-wins via `OriginAttributionStore` scope `work_order`);
actor mutations emit structural `work_order_lifecycle` audit events
with decision ids and durable revisions (never prompt bodies, secrets,
or reasoning). Structural `WorkOrderChanged`/`WorkOrderLaneChanged`
events (identity, change kind, revision only) notify project
subscribers; receivers re-fetch through the authorized get/list path.

## Failure, restart, contention

- Duplicate submission/batch keys return the stored canonical
  object(s) or an explicit mismatch conflict — never a second commit.
- Stale work-order/lane revisions make zero mutation
  (`work_order_revision_conflict`).
- Reorder is serialized by lane revision and committed atomically
  (delete + re-insert in one transaction).
- Reopen preserves exact identities/revisions; no session rows exist
  for waiting work, so restart cannot duplicate or lose execution
  (there is none yet).
- Cross-project parent/lane/gate references fail closed
  (`work_order_project_mismatch`).
- Unknown enum values follow forward-compatibility policy: they fail
  closed and never become executable by default.
- Cancellation before execution marks future eligibility inert; M002
  defines in-flight propagation.

## Non-execution boundary (M001)

M001 starts nothing: no release-gate evaluation, no session/job
creation, no worktree allocation, no trigger endpoint, no model-visible
tool, no TUI task surface. The store API cannot reach the scheduler,
session store, worktree service, or any provider. M002 (release
coordinator and materialization) is the first milestone allowed to
cross that boundary, through `JobSubmissionService` only.

## Release coordinator and materialization (M002)

`WorkOrderCoordinator` (`src/core/work_order_coordinator.rs`, daemon-owned)
is a readiness/materialization coordinator, not a scheduler or executor.
Every initial turn enters `JobSubmissionService` and the existing global
scheduler as `JobKind::AgentTurn`; the scheduler-owned `AgentTurnExecutor`
(`src/scheduler/executors.rs`) records admission without constructing an
`AgentLoop` here. Daemon `TurnSubmit` remains the ordinary-session turn
path; coordinator admission completing means "session is an ordinary
session drivable via `TurnSubmit`", and the occurrence stays `Running`
until the canonical session/job terminal state is projected back (job
completion alone never completes an occurrence).

Release semantics:

- Immediate satisfies at once; delay anchors first occurrences to
  creation/activation and repeats to the prior terminal/release timestamp,
  with the calculated deadline persisted (`not_before`) so restart never
  recalculates a new clock origin; not-before latches (backward clocks
  never un-satisfy); sequence-ready consults lane order + predecessor
  terminal state (failed/cancelled/needs-attention predecessors hold by
  default under `HoldLane`); external-trigger latches only through the
  internal service method until M005 owns the endpoint; `All`/`Any` joins
  combine enabled gates; latches are per occurrence and post-claim gate
  changes cannot create another execution.
- The coordinator maintains a bounded due set (`next_check_at` query +
  explicit wakes) and never polls every work order on a scheduler tick.
- Claim is an atomic `waiting/ready → claiming` CAS: duplicate wakes and
  concurrent coordinators admit exactly one winner; losers reconcile.
- Durable intent + idempotent reconciliation surrounds every side effect:
  workspace link → session link (persisted before any job submission, with
  a deterministic occurrence-derived session id) → job link (deterministic
  submission key, reconciled by key after restart) → running. Restart
  recovery queries canonical stores before creating anything new.
- Workspace policy: `AutoIsolated` (default) takes a managed worktree for
  Git mutation (lazy at claim/start); read-only work shares safely;
  `Serialized`/shared mutation contends through scheduler exclusivity
  (one writer); non-Git mutation never claims worktree isolation (attention
  with `isolation_unavailable`, or serialized sharing).
- Model/policy: the requested stable identity is re-resolved at claim
  (removed models → `NeedsAttention(model_unavailable)`, never silent
  fallback); approval/sandbox snapshots narrow against current ceilings
  and never widen; revoked authority fails closed before new execution.
- Cancellation is state-aware (pre-claim creates nothing; running routes
  through canonical job/session control, then records terminal state;
  terminal cancels are idempotent; completion-vs-cancel races resolve
  deterministically toward the recorded terminal).
- Repeats are finite occurrence creation with distinct ids; only the
  latest terminal occurrence spawns its successor, so duplicate wakes
  converge instead of forking a second chain.
- Structural `WorkOrderOccurrenceChanged` events (identity/state only)
  cover ready/claimed/running/attention/terminal/repeat; actor mutations
  keep the structural `work_order_lifecycle` audit shape (never prompt
  bodies, secrets, or reasoning).

## Testing

```bash
cargo test -p codegg-core -- work_order
cargo test -p codegg-protocol -- work_order
cargo test -p codegg --lib -- work_order_coordinator
cargo test --test work_orders_m002_materialization
python3 scripts/check_work_order_coordinator.py
cargo test -p codegg --lib -- tui::commands::work_orders
cargo test -p codegg --lib -- tui::app::state::work_orders
cargo test -p codegg --lib -- tui::components::dialogs::task
```

Integration (`tests/work_orders_m001_foundation.rs`): daemon-level
create/list/get/update/cancel, project-filtered pagination, concurrent
reorder writers, duplicate submission keys, file-backed reopen,
migration from a pre-M001 database, and authorization/privacy
negatives (unauthorized filtering, opaque-id privacy, cross-project
rejection, immutable origin attribution, bounded secret-free DTOs).

## Task composer, scheduling sheet, and Task view (M003)

User-facing project Task mode for the reference TUI
(`src/tui/commands/work_orders.rs`,
`src/tui/app/state/work_orders.rs`,
`src/tui/components/dialogs/task_schedule.rs`,
`src/tui/components/dialogs/task_view.rs`):

- `ComposerMode::Session | Task` is frontend submission state owned by
  prompt UI state, distinct from `InputMode::Insert | Normal` (which
  stays a text-editing/Vim concern). `Ctrl+G` (`ToggleComposerMode`,
  configurable like every action) toggles it; bare Tab stays
  `SwitchAgent` and Shift+Tab stays permission-mode cycling, so there
  is no keybinding collision (audit test in
  `tui::commands::work_orders::tests`). The header shows
  `composer:task` next to agent/model context in Task mode only.
- Enter in Task mode opens the scheduling sheet instead of submitting
  a turn. Defaults are immediate/zero delay, one occurrence, no
  external trigger. The sheet validates bounded delay forms, RFC 3339
  not-before with explicit offset (naive input fails, never guesses),
  finite repeat, lane/order, All/Any join, model, and the effective
  policy/workspace summary; the external-trigger row is a disabled
  placeholder until M005 owns the capability. Confirm sends exactly
  one `CoreRequest::WorkOrderCreate` — the TUI never creates
  sessions/jobs directly.
- The prompt is never moved to a transcript before confirmation
  succeeds; failures restore it exactly once (stash-then-restore, same
  rule as the session-create continuation). Late create successes may
  have committed daemon state and are never deleted to "undo".
- Last Task-model preference is a separately-scoped daemon-owned
  `runtime_preferences` column pair (`migrate_v62`,
  `STORAGE_LAYOUT_VERSION` 61 → 62) with its own
  `RuntimePreferenceStore::set_task_model_preference` and
  `CoreRequest::TaskModelPreferenceSet` (Global, principal from
  transport authority). Ordinary session preference writes never touch
  it and vice versa; policy writes preserve both. The composer falls
  back to the current/default model with a visible notice when the
  remembered model is gone; created WorkOrders snapshot the model and
  never change silently.
- The project Task view (`Dialog::TaskView`) shows
  RUNNING / FUTURE-WAITING / NEEDS-ATTENTION / RECENT from one bounded
  `WorkOrderList` (+ summary/lanes); occurrence detail is lazy per
  selected row, never N+1. `j`/`k`/arrows navigate, Shift+J/K reorder
  waiting lane members under CAS (conflicts refresh with "queue
  changed; retry"; the pinned running head never moves; lane order
  never edits Job dependencies). Enter opens a materialized session
  through canonical project-tab/session routing; future rows fetch
  detail instead of a fake session. Running rows delegate
  stop/cancel/steer to session/job control. Attention (permissions,
  model/policy/workspace, predecessor holds, worktree conflicts)
  renders in place without focus theft. Close/switch never cancels
  daemon-owned work; every completion carries route + request/
  generation identity and stale ones drop.
- `/tasks` (and `/task`) opens the Task view when the WorkOrder
  capability is available, else the legacy schedule list with an
  explicit diagnostic; `/schedules` keeps direct low-level
  `Schedule*` access and the protocol is preserved. No legacy
  `Task*` protocol returns.

## Global Workspace dashboard (M004)

One bounded daemon-owned aggregate for every authorized project
(`src/core/daemon_workspace_dashboard.rs`,
`crates/codegg-protocol/src/work_order.rs`
`ProjectActivitySummaryDto`,
`CoreRequest::WorkspaceDashboard` /
`CoreResponse::WorkspaceDashboard`):

- Rows carry coarse counts only — running/waiting/future/attention
  WorkOrders, running sessions, pending permission/question counts —
  plus a closed status code (`permission`, `question`, `attention`,
  `failed`, `running`, `waiting`, `idle`, `archived`) and
  `last_activity_at`. No prompt, path, secret, diff, or reasoning.
  `waiting` counts release-pending occurrence instances while
  `future` counts durable active/paused templates (M001 creates no
  occurrence rows before release, so a delayed template is future
  work, not a waiting instance).
- Ordering is deterministic (attention, running activity, recency,
  name, id) with cursor pagination; pages clamp to
  `MAX_WORKSPACE_DASHBOARD_LIMIT` (128, the catalog bound).
- Authorization is enumeration-style (`workspace_dashboard`,
  `Enumeration` + `project.read`, same preamble as `project_list`):
  rows are privacy-filtered to visible projects and per-row counts
  additionally require `session.read`. Callers with `project.read`
  but without `session.read` keep project presence with
  `counts_visible == false` and zeroed counts (explicit Viewer
  decision: the current role matrix grants Viewers `session.read`,
  so Viewers see counts; the zeroed branch is reserved for future
  least-privilege grants and is unit-pinned).
- No eager activation by construction: the handler touches only the
  probe-free catalog listing, indexed `COUNT(*)`/`MAX(updated_at)`
  aggregates, the in-memory session registry, and the team store —
  never LSP, Git, provider, build, or workspace services (static
  assertion in `daemon_workspace_dashboard::tests`). No storage
  migration: the projection derives from canonical stores on
  request; caches are rebuildable and never authoritative.
- TUI (`src/tui/commands/workspace_dashboard.rs`,
  `src/tui/app/state/workspace_dashboard.rs`,
  `src/tui/components/dialogs/workspace_dashboard.rs`): one
  aggregate request per refresh (no N+1 fan-out, fake-client counted
  in tests); `Dialog::WorkspaceDashboard` overlay preserves the
  active tab/session and `Esc` pops back without reloads; `j`/`k`,
  arrows, `g`/`G`, type-to-filter, `Tab` expand (one bounded
  `WorkOrderList` for the selected project only), `Ctrl+R` refresh;
  `Enter` focuses/opens the project tab and opens its Task view
  through existing machinery (no second tab model, no fabricated
  sessions). Event hints mark rows dirty without focus theft;
  reconnect resyncs from the daemon; revocation clears via
  whole-page replacement. `/workspace` and the configurable
  `OpenWorkspaceDashboard` action (`Ctrl+O`, vim `W`) open the same
  view; collision audit and help entries cover both.

## External task triggers (M005)

A task trigger lets shell scripts, CI glue, cron wrappers, and other
automation satisfy one declared `ExternalTrigger` gate on one
`WorkOrder` occurrence without receiving a general CodeGG principal
token or broader project authority
(`crates/codegg-core/src/work_order/trigger.rs`,
`src/server/routes/task_trigger.rs`,
`src/core/daemon_work_orders.rs::fire_work_order_trigger`):

- Bearer shape `cggtr_<trigger-id>.<secret>` (32 CSPRNG bytes,
  base64url-no-pad). The public locator finds the row without
  scanning hashes; the secret verifies constant-time against a
  SHA-256 verifier — the same primitive posture as the
  personal-token digest store, without sharing its table or granting
  principal authority. Only the verifier persists; the plaintext is
  returned exactly once at creation (revoke + create to rotate).
- Trigger state is `active | revoked` durably; `expired`/`exhausted`
  derive from `expires_at_ms`/`max_fires`/`fire_count` at read/fire
  time, so expiry and exhaustion fail closed with no background
  sweeper and survive restart trivially.
- Management travels the ordinary authenticated project protocol:
  `WorkOrderTriggerCreate` (requires `session.create`; binds a work
  order's `ExternalTrigger` gate by explicit `trigger_ref`, or
  implicitly when the work order declares exactly one),
  `WorkOrderTriggerList/Get` (`session.read`, metadata only — never
  secret or verifier), `WorkOrderTriggerRevoke` (`session.create`,
  monotonic and idempotent). Creation supports idempotency keys;
  converged retries return the row with no second secret.
- Firing is not a Core operation: `POST
  /api/v1/task-triggers/{trigger_id}/fire` with
  `Authorization: Bearer cggtr_<id>.<secret>` and an empty body is
  the only fire path (see [server.md](server.md)). The bearer never
  enters principal resolution and grants no other access.
- The fire transaction verifies the capability, latches the bound
  gate on exactly the earliest waiting/ready occurrence, increments
  the fire count only for the latch winner, records the idempotency
  receipt, and wakes the M002 coordinator after commit. Duplicate,
  replayed, and concurrent fires converge on the latch CAS: one
  winner per occurrence, no duplicate execution, no pre-arming of
  future repeats. After an occurrence is terminal and a repeat
  occurrence is created, the same active trigger may fire the next
  occurrence until `max_fires`, expiry, or revocation.
- Failures are privacy-safe: unknown locators, wrong secrets, and
  revoked/expired/exhausted triggers share one generic `401`
  (`trigger_invalid`) carrying no locator, project, secret, or
  verifier content. `GET` is inert (`405`); query-string secrets are
  ignored; principal-shaped bearers are rejected without principal
  verification; revocation/expiry races resolve inside the
  transaction with one deterministic winner.
- Observability is secret-free: `WorkOrderTriggerChanged`
  (`created`/`revoked`/`fired`/`replay`) events and
  `work_order_lifecycle` audit rows carry identity/state/counts
  only. `scripts/check_task_trigger_boundaries.py` pins the prefix
  separation, verifier-only storage, POST-only routing, log hygiene,
  non-Core fire shape, and migration wiring.

M005 also repaired a latent server defect found by its HTTP
qualification: `run_server` served a bare `Router`, which discards
the accept-side address, so the IP-keyed rate limiter's
`ConnectInfo` extraction failed and every HTTP request 500'd. The
server now serves
`into_make_service_with_connect_info::<SocketAddr>()`, matching the
long-standing WebSocket test harness pattern.

## Related docs

- [authorization.md](authorization.md) — project-scoped capability
  mapping and denial privacy for all `work_order_*` operations,
  including the four `work_order_trigger_*` management operations
  (firing is not a Core operation).
- [audit.md](audit.md) — `work_order_lifecycle` action, coverage, and
  structural metadata keys.
- [server.md](server.md) — the narrow `POST
  /api/v1/task-triggers/{trigger_id}/fire` endpoint contract.
- [storage.md](storage.md) — v60–v63 migration context.
- [jobs.md](jobs.md) — scheduler/job ownership that work orders
  consume but never duplicate.
- `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`
  — governing decision.
