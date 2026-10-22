# Core Architecture

This document covers two distinct "core" concerns:

1. **`codegg-core` workspace crate** — domain types, storage, bus, and state
2. **`src/core/` module** — the daemon/transport facade (CoreClient, InprocCoreClient, etc.)

## Provider connection runtime ownership

`ConnectionManager` caches provider instances by `(connection_id, revision)`.
Rotation and refresh invalidate future resolutions after their transaction
commits while preserving already captured instances for in-flight requests.
Durable lifecycle transitions and purge eligibility live in the core
provider-connection store; the daemon protocol routes operator actions through
that authority.

---

## `codegg-core` Workspace Crate

**Location**: `crates/codegg-core/`

### Owned Modules

`codegg-core` currently owns 46 modules (exported from
`crates/codegg-core/src/lib.rs`):

| Module | Key Types |
|--------|-----------|
| `agent_convergence` | durable convergence records |
| `agent_run` | durable agent-run records and store |
| `agent_run_control` | durable run control (cancel/steer) service |
| `agent_run_group` | durable run-group state |
| `approval` | approval policy types |
| `audit` | append-only audit events |
| `audit_instrumentation` | audit emission helpers |
| `authorization` | capability authorization service and operation matrix |
| `bus` | GlobalEventBus, PermissionRegistry, QuestionRegistry |
| `collaboration` | project-chat collaboration domain |
| `context` | ProjectContextResolver, ProjectContext, SessionId |
| `decision` | bounded backend-neutral decision semantics (no tool authority, model, or transport) |
| `error` | AppError, ProviderError, ToolError, is_retryable |
| `goal` | Goal, GoalStatus, GoalBudget, GoalStore, runtime |
| `identity` | typed IDs and project/repository/workspace/session relations |
| `jobs` | JobStore, ScheduleStore, JobState, DaemonGeneration, RecoveryPolicy |
| `memory` | persistent session-to-session learning |
| `migration` | database migration and legacy store conversion |
| `model_profile` | model profile types |
| `model_routing` | Codegg adapter for shared semantic model-policy compilation |
| `presence` | ephemeral presence leases |
| `project_catalog` | durable logical project registry |
| `project_discovery` | project root detection heuristics |
| `project_discovery_service` | service facade for discovery |
| `project_storage` | project-scoped asset persistence |
| `projection_replay` | projection stream store, service, seam |
| `protocol_conversions` | core-safe domain↔DTO conversions |
| `provider_connections` | connection store, metadata, lifecycle |
| `repository_lineage` | repository identity and lineage |
| `resilience` | CircuitBreaker, FallbackProvider |
| `run_result` | durable run result records |
| `run_store` | run record persistence and artifact storage |
| `session` | session storage, message history, checkpointing |
| `session_control` | session control state |
| `snapshot` | file state capture and restore |
| `storage` | SQLite initialization, connection pooling |
| `task_state` | task state tracking |
| `team` | principal, membership, role, capability domain |
| `tool_program` | tool program IR, language, interpreter |
| `transport_auth` | transport authentication and principal binding (M002) |
| `work_order` | durable project work orders, occurrences, lanes, triggers |
| `work_plan` | work-plan types |
| `workspace` | WorkspaceRegistry, WorkspaceId, ExecutionContext |
| `workspace_services` | per-workspace service bundles and lifecycle |
| `worktree` | git worktree management |
| `worktree_service` | managed worktree lifecycle service |

### User Prompt Queue Read Contract (M002a, ADR-0035)

### User Prompt Queue Due-Time Contract (M007, ADR-0039)

`UserPromptQueueAdd` and `UserPromptQueueEdit` carry an optional
`run_at_ms: Option<i64>`, `#[serde(default)]`, so a pre-M007 client that omits it
sends `None` and a newer daemon reads an older projection without a sentinel.
`UserPromptQueueEntryDto` carries it back for the same reason.

**What the daemon owes.** A due time is a **floor, not a deadline**. The entry
becomes eligible at that instant and is delivered at the next turn boundary at or
after it. It is never delivered early — the store predicate is
`(run_at_ms IS NULL OR run_at_ms <= ?)` and every caller re-evaluates it against
its own `now_ms`, so two callers can only ever disagree about whether the instant
has passed, never about whether it has been reached.

**What the daemon does not owe.** Nothing about wall-clock precision. A due time
that passes while the daemon is down is honoured *late* on the next boot; the
daemon is late, not early. There is no scheduler permit, no executor, and no
`JobTemplate` — ADR-0035 rejects that model, and a due time does not make a
prompt a job.

**Refusals.** A due time at or beyond the entry's own expiry is refused with
`user_prompt_queue_due_time_after_expiry`, carrying both instants so a client can
explain the conflict without being handed prompt text. It is refused, not
clamped: clamping schedules an entry for its expiry, where it is due only once it
has already expired — a scheduled prompt that can never run, with nothing on
screen saying so. A due time in the **past** is accepted rather than refused; the
user has already passed it, so the entry is eligible immediately.

**Wakeup authority is ADR-0039's, not this contract's.** See
`scripts/check_queue_delivery_callsites.py`: `submit_queued_prompt_turn` has
exactly three permitted callers, and the third (the bounded queue advance tick)
was a recorded decision, not a quiet third call site.

**This is read-only infrastructure. No mutation surface exists yet.**
`CoreRequest::UserPromptQueueList { session_id }` returns
`CoreResponse::UserPromptQueue { session_id, entries }`, and nothing can
enqueue, edit, reorder, or delete an entry. Every mutation operation belongs to
M002b, which is gated on ADR-0037. **No user can queue a prompt as of this
milestone**, and the read therefore always returns an empty list.

| Property | Value |
|---|---|
| Authorization | `ScopeKind::ViaSession` + `Capability::SessionRead`, via `operation_descriptor` (`policy.rs`). The handler hand-rolls no check. |
| Audit class | `UNINSTRUMENTED_OPERATIONS` — a read of inert metadata with no shared-state mutation. Denial *is* audited upstream by `authorize_request`. |
| Routing | `DaemonRequestFamily::Turns`; registered in `session_id_for_request` so the audit chain carries the session locator. |
| Storage | `codegg_core::session::UserPromptQueueStore` over the daemon pool (`daemon_turns::list_user_prompt_queue`). |
| Ordering | Daemon-owned dense `ordinal`, ascending. The frontend never re-sorts. |
| Expiry | Lazy and deterministic — the read marks due entries `expired` and returns survivors in one transaction. |

The read deliberately does **not** route through `is_controller_bypass`. That
bypass is an identity-only comparison against `LOCAL_OWNER_PRINCIPAL_ID` that
is not bound to a session, so it would let any local owner read any session's
queue. A read needs session read authority.

Every new DTO field is `#[serde(default)]`, so a projection written by a newer
daemon stays readable by an older client and an absent projection reads as an
empty queue. Entry text never appears in a `tracing` call, an audit record, or
a `CoreResponse::Error` message — `UserPromptQueueError`'s Display carries only
identity, bounds, and availability.

### Two Ownership Notions — Which Is Which

ADR-0037 (accepted, Option B) introduced a second ownership concept **beside**
the turn controller lease. They are adjacent, they are both durable rows keyed
by `session_id`, and they answer different questions. Every seam names which
one it is asking about.

| | Turn controller lease | Queue owner (v81) |
|---|---|---|
| Table | `session_turn_controller` | `user_prompt_queue_owner` |
| Answers | *May this principal steer or cancel this in-flight turn?* | *May this principal author this session's prompt queue?* |
| Lifetime | **Turn-scoped.** `acquire` writes it, `release` deletes it. It exists only while a turn runs. | **Session-scoped.** It persists across turns and across daemon restart. |
| Identity | ADR-0007 | ADR-0037 (accepted 2026-10-18) |
| Changed by | `session_control_transfer` / `takeover`, CAS on `revision` | `user_prompt_queue_adopt` / `user_prompt_queue_transfer`, CAS on `revision` |

The lease cannot answer the queue question **between turns**, because there is
no row to read: `controller_lease_for` returns `None` and the old predicate
refused every principal, the legitimate owner included. That is precisely the
case queue authoring exists for — a user composes the next instruction when no
turn is running.

### Queue Mutation Contract (M002b, ADR-0037)

Six operations, all `ScopeKind::ViaSession` + `Capability::AgentInvoke`:

| Operation | Effect |
|---|---|
| `UserPromptQueueAdd { session_id, entry_id, text }` | Append; on an absent queue, create **and** adopt in one transaction. |
| `UserPromptQueueEdit { session_id, entry_id, text }` | Replace text only. Identity and position unchanged. |
| `UserPromptQueueDelete { session_id, entry_id }` | Remove, then renumber densely. |
| `UserPromptQueueReorder { session_id, entry_ids }` | Write the complete new order, collision-free. |
| `UserPromptQueueAdopt { session_id }` | The only repair path for an unowned queue. |
| `UserPromptQueueTransfer { session_id, recipient_principal, expected_revision }` | CAS move of ownership. |

**M002b still delivers no user-visible capability.** There is no command, no
dialog, and no key. M004/M005 own anything a user can press. Delivery into
turns arrived with M003 below.

### Queue Send-Now Contract (M006, ADR-0036, Option A)

`CoreDaemon::handle_user_prompt_queue_send_now` is the daemon side of `s` in
`/queue`, and it is **not** a seventh `QueueMutation`. ADR-0037 excludes steering
from the durable owner binding, so the *handover* is gated by the turn controller
alone; the queue-ownership question is re-asked inside `mark_steer_delivered`'s
transaction, where it is about a write to the queue rather than a reach into a turn.
Running it through `ControllerScope::QueueMutation` would refuse a principal
steering its own turn in a session it does not own.

Its order is the safety argument, and each step is there because the next cannot
undo it:

1. **Pre-flight** the entry via `get_live`, so a prompt that could never be settled
   is never steered at all. This is what stops a `delivering` row — already running
   as its own turn — from being handed to a turn as well.
2. **Steer** `runtime.active_turn`, resolving `turn_id` from the daemon's **own**
   handle and applying `check_turn_controller`. The handle's `(turn_id, sender)`
   pair is cloned under a short read, before any storage work, so no read lock is
   held across a database round trip. A sender cloned from a since-ended turn is
   not a hazard: its receiver is dropped with the turn, so `try_send` reports
   `Closed` and the prompt stays queued.
3. **Settle** with `mark_steer_delivered`, guarded on `status = 'pending'`.

Step 3 can fail after step 2 succeeded — the turn holds the text and the row did not
move. That is `AcceptedNotSettled`, never an error: an error would tell the user to
resend a prompt that has already run. `steer_channel_code` is shared with
`TurnSteer`, so the two steering paths cannot disagree about a full or closed channel.

Adds **no** caller of `submit_queued_prompt_turn`; it reuses `TurnSteer`'s channel
rather than a second delivery entry.

### Queue Delivery Contract (M003, ADR-0038, Option B)

`CoreDaemon::submit_queued_prompt_turn` is the **single** daemon-internal
delivery entry: claim one entry in ordinal order, resolve the delivery principal
from durable rows, re-authorize against current team state, admit through the
canonical `CoreRequest::TurnSubmit` path, then resolve the claim as
`delivered`, `failed`, or back to `pending`. There is no queue-specific submit,
no relaxed receipt, and no replay of an interrupted one.

**As whom.** The delivered principal is the queue owner: `principal_id` from
`user_prompt_queue_owner`, `kind` from `TeamStore`, `AuthMethod::QueueDelivery`
and `TransportClass::DaemonDelivery` as a *delivery classification*, and a
daemon-owned `queue:<entry_id>` client marker that is metadata and never
authority. A synthetic daemon identity would hold the controller lease as that
identity, so the owner's next mid-turn queue mutation would fail
`user_prompt_queue_controller_mismatch` — the queue would lock itself out the
moment it started working. `is_controller_bypass` is refused for the same
reason.

**At-most-once** is inherited from `turn_admission_receipt`, not reimplemented:
the submission id is a deterministic hash of the entry id alone, so a retry
re-derives the same receipt and receives `Duplicate`.

**Three triggers, and only three** — `scripts/check_queue_delivery_callsites.py`,
which also fails when none is wired (a guard that passes on an unwired feature
constrains nothing):

| Caller | When | Why there |
|---|---|---|
| `deliver_next_queued_prompt` | after `spawn_turn_reaper` observes a terminal event, the lease is released, and the runtime is parked `Idle` | Both preconditions for delivery — free session and no lease row — are established there, which is why it is not on the terminal event itself |
| `reconcile_stranded_queue_deliveries` | once per daemon start, after `interrupt_incomplete_turn_receipts` and after `reconcile_session_controllers` | A restart is the second moment a session is reliably free; before the controller reconciliation a stale lease would release every entry it tried to deliver |
| `advance_due_queue_entries` | on the bounded queue advance tick, when `run_queue_advance_task` wakes at the earliest due instant across all sessions | **M007, authorized by ADR-0039 (accepted).** A scheduled entry on an idle session has no terminal transition and no restart to wait for; without this it would deliver late, or never within the session |

None of the three is a poller. The daemon has **no periodic session-observation
loop** — its three `tokio::time` intervals are projection retention (300s),
opt-in provider refresh (30s), and opt-in TTS drain (500ms) — and M003 adds
none. M007 adds one wakeup source and no loop: the tick computes the earliest
due instant, sleeps exactly until it, and **parks on a `Notify` with no timer
armed** when nothing is scheduled, so an idle daemon stays idle.

Stranded `delivering` rows are resolved **from the receipt**, never from the
entry, because the receipt is the only evidence of whether a turn actually
started. The sweep is bounded (`USER_PROMPT_QUEUE_MAX_RECONCILE`, 256).

A turn that reached admission and was refused is **terminal**; one that never
reached admission is **released**. The split is on the typed `AppError` class
(`Storage`/`Io` are transient), never on error wording.

#### Three-valued ownership

| State | Meaning | Who may mutate |
|---|---|---|
| `Absent` | no owner row **and** no entries | the first `add`, which creates both atomically |
| `Owned(p)` | an owner row names `p` | `p` |
| `Unowned` | entries exist, **no** owner row | nobody — an invalid state |

`Unowned` is refused on **every** mutation including `add`, and is repaired only
by explicit `UserPromptQueueAdopt`. First-add must not silently adopt here, or
an invalidated queue would quietly become owned by whoever touched it next. It
is unreachable through this milestone's own code paths — the only path creating
entries also creates the owner in the same transaction — so it is a defensive
state the tests construct by hand.

**The adoption policy ADR-0037 deferred to M002b:** the *calling principal*
becomes the owner on first add. Not the session creator, not the local
interactive owner. The binding records who actually authored the queue.

#### The single predicate

`ControllerScope` (`src/core/daemon_control.rs`) generalizes
`check_turn_controller` into one predicate whose call shape names the question:

```rust
enum ControllerScope<'a> {
    Turn { session_id: &'a str, turn_id: &'a str },
    QueueMutation { session_id: &'a str },
    QueueAdoption { session_id: &'a str },
}
```

Named variants, deliberately **not** `Option<&str>`: `None` cannot distinguish
"a queue mutation on an unowned queue" from "a turn operation with no turn", and
those need different refusals. `check_turn_controller` remains as the turn-scoped
entry point and delegates here, so every pre-existing call site is unchanged and
its behavior is proven by its existing tests.

Mid-turn, `QueueMutation` requires the caller to be **both** the queue owner and
the turn controller. Fail-closed, with no precedence rule:

| Queue owner | Turn controller | Result |
|---|---|---|
| caller | caller | allowed |
| caller | someone else | `user_prompt_queue_controller_mismatch` |
| someone else | caller | `user_prompt_queue_not_owner` |
| someone else | someone else | `user_prompt_queue_not_owner` |

Ownership is evaluated before the turn term, which is what makes rows 3 and 4
report `NotQueueOwner` rather than depending on check order.
`QueueAdoption` is separate because it repairs a queue that has *no* owner, so
the queue-owner term is absent by definition; the turn lease, if any, is the
only standing available.

**Authorization and mutation are never separable by a crash.** The store re-reads
the owner row inside the same transaction that writes, so a refused mutation
leaves no partial write and a committed one was authorized by the row it acted
under.

#### `is_controller_bypass` gains no queue authority

ADR-0037 rules this out explicitly, and the code implements it: the bypass's
early return is now guarded by `matches!(scope, ControllerScope::Turn { .. })`.
The bypass is an identity-only comparison against `LOCAL_OWNER_PRINCIPAL_ID`,
not bound to a session, so letting it carry queue authority would let the local
interactive owner mutate **any** session's queue. Proven by execution in
`user_prompt_queue_controller_bypass_carries_no_queue_authority`, not by
inspection.

#### Refusals

| Code | Meaning |
|---|---|
| `user_prompt_queue_not_owner` | Caller is not the queue owner. **Carries no owner identity** — naming it to a refused caller would itself be the leak. |
| `user_prompt_queue_unowned` | Queue has entries but no owner row. |
| `user_prompt_queue_controller_mismatch` | Owner is not the controller of the active turn. |
| `user_prompt_queue_owner_revision_conflict` | Transfer's `expected_revision` fence rejected the write; the loser is named. |
| `user_prompt_queue_order_mismatch` | Reorder's entry list is not a permutation of the live set. |
| `user_prompt_queue_not_found` / `_expired` | No such entry / the entry is terminal. |

#### Audit

Every mutation, **including adoption and transfer**, emits the
`prompt_submit` action ADR-0037 fixed, via
`audit_instrumentation::user_prompt_queue_mutation_event`. Records carry entry
identity, the actor's own owner identity, and a byte count — never prompt text,
and never a digest of it, because a length is bounded while a digest is a
content oracle. This deliberately diverges from `session_control_transfer`, which
maps to `membership_change`: the divergence is directed by ADR-0037, not an
oversight.

#### Reorder is collision-free by construction

`UNIQUE (session_id, ordinal)` means a renumber walking straight to its targets
can collide with a row it has not moved yet. `reorder` first displaces every
affected row by `USER_PROMPT_QUEUE_RENUMBER_BAND_OFFSET` (1,000,000, well above
every reachable ordinal), then writes the dense targets. A uniform offset keeps
displaced rows distinct and lifts them clear of the target band, so neither
statement can violate the constraint. A positive band offset rather than negative
sentinels, because v80's `CHECK (ordinal >= 0)` forbids them.

### Re-exports into Root

Root `src/lib.rs` re-exports these modules so downstream code can use
`crate::bus`, `crate::session`, etc.:

```rust
pub use codegg_core::{
    agent_convergence, agent_run, agent_run_control, agent_run_group, bus,
    goal, identity, memory, migration, model_profile, project_storage,
    repository_lineage, resilience, run_store, session, snapshot, storage,
    task_state, work_plan, workspace, workspace_services, worktree,
};
```

### Root-Side Modules (intentionally not moved)

These modules remain in root `src/` due to high coupling with UI/server/agent:

| Module | Reason |
|--------|--------|
| `acp` | agent communication protocol |
| `agent` | AgentLoop, compaction, routing, durable run control |
| `tool` | all built-in tools |
| `permission` | access control, modes |
| `mcp` | Model Context Protocol client |
| `tui` | terminal user interface |
| `server` | HTTP/WebSocket server (feature-gated) |
| `client` | remote TUI client (feature-gated) |
| `core` | daemon runtime, transport adapters |
| `plugin` | WASM plugin system |
| `search`, `search_backend` | web search |
| `research` | deep research |
| `auth` | typed auth config, credential store |
| `context` | context projection and compaction |
| `scheduler` | global admission control (Phase 5) |
| `managed_process` | non-interactive process execution |
| `interactive_process_attach` | bounded attach/resume protocol over the M001 PTY engine |
| `command_intent` | command classification and routing |
| `command_planner` | execution backend planning |
| `command_routing` | structured command dispatch |
| `command_outcome` | command outcome tracking |
| `shell` | human shell execution model |
| `python_script` | restricted-Python scripting |
| `preflight` | pre-mutation validation harness |
| `theme` | theme system |
| `tts` | text-to-speech |
| `upgrade` | self-upgrade |
| `hooks` | agent lifecycle hooks |
| `ide` | IDE integration |
| `lsp` | Language Server Protocol |
| `security` | SSRF, sandboxing |
| `skills` | skill loading and activation |
| `command` | slash command registry |
| `exec` | non-interactive exec mode |
| `util` | clipboard, fuzzy search, pricing |
| `protocol_conversions` | agent-specific domain↔DTO conversions |
| `eggsact` | deterministic tool runtime (in-process) |
| `background_task_migration` | legacy task migration |
| `git_mutation_projector` | git mutation projection |
| `git_mutations` | typed git mutation executor |
| `git_mutations_ops` | git mutation operation implementations |
| `git_network_ops` | git network operation implementations |
| `git_network_policy` | git network policy enforcement |
| `git_recovery` | git conflict recovery |
| `git_run_store` | git run store integration |
| `git_service` | canonical read git executor |
| `job_dispatcher` | job dispatch logic |
| `job_recovery` | job recovery logic |
| `test_runner` | test execution harness |

### Dependencies (Cargo.toml)

`codegg-core` depends on sibling workspace crates and external libraries,
but **never** on UI/server/plugin crates:

```
codegg-config, codegg-git, codegg-protocol, codegg-providers
egggit, egglsp, eggsentry
anyhow, base64, async-trait, chrono, dashmap, dirs,
md5, once_cell, parking_lot, rand, regex, serde, serde_json,
sha2, similar, sqlx, thiserror, tokio, tokio-util, toml,
tracing, uuid, rustpython-parser
```

### Forbidden Dependencies

`codegg-core` must NOT depend on:

- **UI**: `ratatui`, `crossterm`, `ratatui_textarea`
- **Server**: `axum`, `tower_http`, `tokio_tungstenite`
- **Plugin**: `wasmtime`, `wasmtime_wasi`

Run `./scripts/check-core-boundary.sh` to verify no forbidden imports or
dependencies have crept in.

### Why Root `src/error.rs` Still Exists

Root `src/error.rs` re-exports `codegg_core::error::*` and adds
Axum-specific response wrappers (`AxumAppError`, `AxumServerRuntimeError`)
behind `#[cfg(feature = "server")]`. This avoids pulling `axum` into
`codegg-core`.

### Why Protocol Conversions Are Split

- `crates/codegg-core/src/protocol_conversions.rs`: Core-safe conversions
  (session, message, provider, config) that don't depend on agent/server
  runtime.
- `src/protocol_conversions.rs`: Agent-specific conversions + re-export of
  core conversions via `pub use codegg_core::protocol_conversions::*;`.

`codegg-protocol` must not depend on domain/runtime crates; conversions
intentionally live outside it.

### Next Likely Extraction Target

The daemon/agent/tool/permission boundary, not TUI. Residual M002 split
the `CoreDaemon` request dispatch into `core::daemon_family` plus 16
`core::daemon_*` family handler modules (and the two sub-handler modules
`daemon_control` / `daemon_workspace_dashboard`); Residual M003 has separated
construction, bootstrap/recovery, refresh, and shutdown into
`core::daemon_construct`, `core::daemon_bootstrap`, `core::daemon_refresh`,
and `core::daemon_shutdown` while preserving the exact initialization order
and joined shutdown sequence.

---

## `src/core/` Module (Transport Facade)

**Location**: `src/core/`

The `core` module is the request/response facade that separates TUI
transport from the underlying agent and session logic.

### Key Responsibilities

- Provide a typed request/response boundary for UI and transport adapters
- Centralize session, memory, task, worktree, permission, and question
  operations
- Support in-process, stdio, and socket-backed execution modes
- Bridge core events into the global event bus when running in-process

### Module Inventory

| Module | Key Types | Purpose |
|--------|-----------|---------|
| `core::daemon` | `CoreDaemon` | Single composition/lifecycle authority; owns workspace registry, event log, scheduler, workspace services, session runtime, notification router, asset refresh coordinator, and projection seam. `handle_request_with_client` runs authorization/audit, the boxed chat pre-router, the spawned interactive-process pre-router, then a thin `DaemonRequestFamily` router that delegates each envelope to exactly one family handler. ~6,000 lines (was ~12,000 before M002; lifecycle now in `daemon_construct`/`daemon_bootstrap`/`daemon_refresh`/`daemon_shutdown`). |
| `core::daemon_family` | `DaemonRequestFamily` | Sole request-to-owner routing table: `of(&CoreRequest)` maps all 261 variants to one family plus `owner_module()`. Chat, team, interactive, human shell, and work orders classify here but are served pre-router to preserve stack/cancellation semantics. |
| `core::daemon_assets` | `handle_assets_request` | Asset refresh/status/capabilities over the daemon-owned `AssetRefreshCoordinator`. |
| `core::daemon_control` | `handle_control_request` | Active-turn control requests (cancel/steer/permission-question response routing) owned outside the turn handler. Also owns `spawn_turn_reaper`, whose post-terminal hook is M003's queue-delivery wake, and `publish_queue_changed`. |
| `core::daemon_queue_delivery` | `CoreDaemon::submit_queued_prompt_turn`, `reconcile_stranded_queue_deliveries`, `advance_due_queue_entries` | M003 (ADR-0038): the single daemon-internal queue-delivery entry, its bounded restart twin, and M007's bounded advance tick (ADR-0039, accepted). Restricted to three callers by `scripts/check_queue_delivery_callsites.py`. |
| `core::daemon_documents` | `handle_document_request` | Open/close/reload document operations over daemon-owned document state. |
| `core::daemon_project_init` | `handle_project_init_request` | Bounded `/init` evidence preview and one-use, project-scoped guarded publication of root `AGENTS.md`. |
| `core::daemon_lsp` | `handle_lsp_request` | LSP status/operations plus diagnostics-store reads. |
| `core::daemon_scratch` | `handle_scratch_request` | The scratch usage census (M006) and the scratch retention sweep (M007): the daemon-held measurement of its own disposable scratch tree, and the bounded reclamation of that tree. |
| `core::daemon_team` | `handle_team_request` | Team membership, principal, and device-token administration (team-collaboration M003). |
| `core::daemon_human_shell` | `handle_human_shell_request` | The human shell family: the staged human command surface and its promotion model. |
| `core::daemon_work_orders` | `handle_work_order_request` | Project work orders, occurrences, sequence lanes, and trigger management. |
| `core::daemon_workspace_dashboard` | `handle_workspace_dashboard_request` | Workspace dashboard aggregates over daemon-owned workspace state. |
| `core::daemon_providers` | `handle_providers_request` | Eggpool provisioning and provider-connection lifecycle over the daemon-owned provisioner. |
| `core::daemon_sessions` | `handle_sessions_request` | Session CRUD, selection reads, message reads, import/export/template over daemon-owned session stores. |
| `core::daemon_turns` | `handle_turns_request` | Turn submit/cancel/steer, agent/model selection writes, permission/question responses, transport lifecycle. |
| `core::daemon_jobs` | `handle_jobs_request` | Durable jobs, schedules, run records, tool-program inspection, legacy task shims over scheduler/job stores. |
| `core::daemon_projects` | `handle_projects_request` | Project catalog, workspace registry/services, managed worktrees, daemon/workspace snapshots. |
| `core::daemon_goals` | `handle_goals_request` | Session goals, todos, edit checkpoints, LSP preview apply over daemon-owned domain stores. |
| `core::daemon_projection` | `handle_projection_request` | Projection replay subscribe/resume/ack/snapshot/artifacts plus ephemeral presence leases. |
| `core::daemon_ops` | `handle_ops_request` | Audit query/export, memory, notification routing over daemon-owned stores. |
| `core::daemon_construct` | `with_deps`, `with_deps_and_identity`, `new`, `SeamProjectionSink` | Deterministic dependency construction in documented order; single `Self { .. }` assembly with no partial publish. |
| `core::daemon_bootstrap` | `hydrate_workspace_registry`, `recover_state`, `recover_jobs`, `start_event_bridge`, `initialize_recovery_sequence` | Startup hydration, event bridge, turn/job recovery, and replay. `initialize_recovery_sequence` is the canonical in-process hydrate -> bridge -> recover order. |
| `core::daemon_refresh` | `refresh_project_context`, `refresh_project_activation`, `activate_project_workspace`, `project_health`, `refresh_runtime_assets` | Runtime refresh coordinators plus shared workspace/binding resolvers. All refresh flows through the daemon-owned `AssetRefreshCoordinator`. |
| `core::daemon_shutdown` | `Drop`, `abort_background_handles` | Joined shutdown: cancel precedes joins; aborts projection-maintenance and worktree-reconcile tasks in order. Scheduler loop stays detached by design. |
| `core::instance` | `DaemonPaths`, `DaemonInstanceGuard`, `DaemonInstanceMetadata`, `CoreRuntimeMode`, `connect_or_start_daemon` | Daemon-owned lock/metadata lifecycle and compatibility-facing connect-or-start API. |
| `core::runtime_deps` | `CoreRuntimeDeps`, `LegacyAgentRuntimeDeps` | Bundles pool, memory_store, legacy_agent (subagent_pool), agent_run_store, run_control, run_group_service, convergence_store, turn_runtime, lsp_service, workspace_services, worktree_service, workspace_service_policy, job_store, schedule_store, recovery_policy, daemon_generation, scheduler, submission, scheduler_config, connection_manager. Always has a default TurnRuntime; override via `with_turn_runtime()`. |
| `core::transport` | `SocketCoreClient`, `StdioCoreClient` | JSONL over platform-local byte streams and stdio. Also contains `daemon_socket` for the daemon-side accept loop. |
| `core::transport::projection` | projection stream management | Connection-local projection subscription, cursor, and forwarding state. |
| `core::event_log` | `EventLog` | In-memory event ring buffer with optional SQLite-backed projection sink. |
| `core::client_registry` | `ClientRegistry`, `AuthenticatedPrincipal` | Maps transport connection IDs to metadata plus the immutable transport-bound canonical principal (M002) for projection ownership and request authority. |
| `core::notification` | `NotificationRouter`, `AudioArbiter` | TTS and notification policy routing. |
| `core::lsp_diagnostics_store` | diagnostics store | Daemon-owned LSP diagnostics persistence. |
| `core::work_order_coordinator` | work-order coordinator | Wakes and advances work-order gates and schedules. |
| `core::session_runtime` | `SessionRuntimeRegistry` | Active session runtime state tracking. |
| `core::session_selection` | `SelectionService` | Session-level connection/model selection via typed stores. M004: `ModelSelect` adapter, `apply_last_used_preference`, durable-projected runtime cache. |
| `core::provider_connections` | `ConnectionManager`, `ProviderConnectionStore` | Provider instance caching, lifecycle, and purge. |
| `core::eggpool` | `EggpoolProvisioner` | Connection provisioning and background refresh. |
| `core::project_activation` | `ProjectActivationRegistry` | Owner-scoped activation leases for projects. |

Frontend identity, local endpoint/path resolution, socket transport, and
connect/reuse/start orchestration live in the leaf `codegg-client` crate.
Root `SocketCoreClient` and `connect_or_start_daemon` preserve existing root
APIs while adapting the extracted client. Daemon lock ownership, metadata,
listener binding, and core construction remain root-owned. See
[`client.md`](client.md).

### `CoreClient`

```rust
#[async_trait]
pub trait CoreClient: Send + Sync {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError>;

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>>;
}
```

`subscribe()` is event-capable for in-process and local-socket clients.
Stdio currently exposes request/response transport and returns an empty
receiver. `SocketCoreClient` adapts `codegg-client::LocalSocketClient` and
forwards its bounded event stream.

### Core Clients

| Type | Purpose |
|------|---------|
| `InprocCoreClient` | Runs the core in the current process. Constructed via `with_deps(CoreRuntimeDeps, Config)` (preferred) or the legacy convenience constructor. `subscribe()` reads from `daemon.event_log` when a daemon is present; falls back to `GlobalEventBus` in legacy no-daemon mode. |
| `StdioCoreClient` | Spawns `codegg core-stdio` and exchanges JSONL requests/responses over stdin/stdout |
| `SocketCoreClient` | Connects to the platform-local daemon endpoint and exchanges JSONL requests/responses |

### Protocol

Defined in `crates/codegg-protocol/src/core.rs`.

#### Envelopes

| Type | Purpose |
|------|---------|
| `RequestEnvelope<T>` | Wraps requests with `protocol_version` and `request_id` |
| `EventEnvelope<T>` | Wraps events with sequence, timestamp, and optional session/turn metadata |
| `CoreRequest` | Typed requests (see families below) |
| `CoreResponse` | Typed responses for acknowledgements, JSON payloads, sessions, and errors |
| `CoreEvent` | Core-side event stream for in-process subscribers |

#### Request Families

- **Session lifecycle**: list, create, load, attach, fork, delete, archive,
  restore, share, unshare, rename, export, import, create-from-template,
  initialize, subscribe, resume
- **Turn lifecycle**: submit, cancel, steer, agent select, model select
- **Session data**: message loading and message counts
- **Provider connections**: create, cancel, status, list, models, rotate,
  refresh, enable, disable, delete, restore, purge, detail
- **Session selection**: get, list, update, models
- **Workspace**: register, list, archive, snapshot, services snapshot,
  config reload
- **Project catalog**: list, get, register, archive, restore, health,
  capabilities
- **Durable jobs**: submit, wait, get, list, cancel, retry, attempts,
  recovery report
- **Schedules**: create, list, get, pause, resume, delete
- **Goals**: set, from-file, show, pause, resume, clear, done, checkpoint,
  set-budget
- **Projections**: capabilities, subscribe, resume, ack, unsubscribe,
  snapshot-get, artifact-read, artifact-list
- **Presence** (M001, ephemeral): capabilities, heartbeat, snapshot-get.
  Daemon-owned leases; principals come from transport authority; never
  authoritative. See `architecture/presence.md`.
- **Tool programs**: list, inspect, call-page, notification-reinject,
  recovery-debug-inspect
- **Memory**: search, list, remember, forget
- **Tasks**: list, schedule, delete
- **Worktree**: list
- **Operational**: model refresh, permission/question response, snapshot
  (session/workspace/models/daemon), notification speak/stop, todo list,
  active goal load, run list/get/artifact-read, asset refresh/status,
  eggpool connection lifecycle

#### Request Handler Behavior

**Handled variants** (produce meaningful response):
- `TurnSubmit` — Spawns agent loop, returns `Ack` immediately
- `SessionMessagesLoad` / `SessionMessageCounts` — Returns session data
- `SessionCreate` / `SessionLoad` / `SessionAttach` — Session operations
- All other session variants (List, Fork, Delete, Archive, Restore, Share,
  Unshare, Rename, Export, Import, CreateFromTemplate)
- `Subscribe` — returns `Json { current_seq, session_id }`
  (`daemon_turns.rs:643`); it is a probe, not a streaming registration
- `Resume` — replays events from `from_event_seq`, or returns
  `ResyncRequired` when the requested sequence is no longer covered, or an
  empty `Events` when the client is already in sync (`daemon_turns.rs:652`)
- `TurnCancel` / `TurnSteer` — gated on the controller predicate; observers
  and non-controller Contributors are rejected (`daemon_turns.rs:695`/`:737`)
- `AgentSelect` — binds through `bind_runtime_for_session`, returns
  `session_unbound` when binding fails (`daemon_turns.rs:798`)
- `ModelSelect` — durable `SelectionService` adapter; the runtime cache is
  projected only *after* durable success (`daemon_turns.rs:825`)
- `Initialize` — classified to the Turns family
  (`daemon_family.rs:292`) but has no handler arm in `daemon_turns.rs` and
  falls into the family's `_ =>` `unimplemented` response
- `PermissionRespond` / `QuestionRespond` — Registry responses
- `ModelsRefresh` — Returns refreshed model list
- `TaskList` / `TaskSchedule` / `TaskDelete` — Task operations
- `MemoryList` / `MemorySearch` / `MemoryRemember` / `MemoryForget`
- `WorktreeList` — Returns worktree list
- All workspace, project, job, schedule, goal, projection, provider
  connection, selection, asset refresh, and eggpool variants

### Transport Modes

| Mode | Description |
|------|-------------|
| DaemonClient (default) | Connects to (or auto-starts) the user-scoped singleton daemon via `connect_or_start_daemon` (`src/core/instance.rs`). Uses `SocketCoreClient`. |
| StandaloneInproc | Runs the core in the current process via `InprocCoreClient`. Visible non-production mode; requires `--standalone`. |
| StandaloneStdio | Spawns `codegg core-stdio` via `StdioCoreClient`. Compatibility/testing; requires `--stdio`. |

Selection: `CoreRuntimeMode` enum (default `DaemonClient`). `--standalone`
maps to `StandaloneInproc`; `--stdio` maps to `StandaloneStdio`. Legacy
`--core-transport inproc|stdio` still parses but emits a deprecation warning.

End-user vs internal surface: ordinary `codegg --help` shows only user
operations. `--standalone` stays visible (documented non-daemon user
mode); the deprecated `--core-transport`, the compatibility `--stdio`,
and the internal `--core-endpoint` still parse but are hidden from
normal help, as is the `core-stdio` JSONL entry point. Local daemon
attachment is `codegg daemon attach` (top-level `attach-daemon` remains
as a hidden compatibility alias); it is distinct from the
feature-gated remote HTTP `attach`, which only exists in `server`
builds. `CODEGG_CORE_TRANSPORT` remains honored as a deprecated
environment override with the same mapping and warnings.

### Singleton Lifecycle

Phase 1 establishes the production invariant that exactly one user-scoped
Codegg daemon owns execution at a time. All implementation lives in
`src/core/instance.rs`.

**`DaemonPaths`** resolves all per-user daemon artifacts:

| Path | Purpose |
|------|---------|
| `daemon.lock` | Advisory nonblocking `File::try_lock` — authoritative identity |
| `daemon.json` | Atomic metadata record (diagnostic only) |
| `core.sock` / named pipe | Platform-local CoreFrame endpoint; Windows implementation is present but live transport/lifecycle qualification is pending |
| `daemon.log` | Debug log (best-effort, rotated at 10 MB) |

Production locations (from `default_user_runtime_root`,
`crates/codegg-client/src/paths.rs:161`):
- macOS: `$HOME/Library/Application Support/codegg`
- Linux, in order: `$XDG_RUNTIME_DIR/codegg`, then `$XDG_DATA_HOME/codegg`,
  then `$HOME/.local/share/codegg`
- Other Unix: `/tmp/codegg`
- Windows: `dirs::data_local_dir()`, else `%TEMP%`

Override via `CODEGG_DAEMON_HOME`.

**`DaemonInstanceGuard`** is an RAII guard that holds the platform lock for the
daemon's lifetime. The lock is acquired with nonblocking `File::try_lock`; on drop it
removes the metadata file (if owned by this guard) and releases the lock. The
OS also releases the lock automatically on process exit. The standard library
maps this API to the platform locking primitive, including `LockFileEx` on
Windows. Windows live runtime qualification remains required before claiming
Windows support.

**`DaemonInstanceMetadata`** (`daemon.json`) carries: `daemon_id`,
`generation` (UUID), `pid`, `socket_path` (legacy/native form), optional
platform-neutral `endpoint_uri`, `protocol_version`, `started_at`, and
`binary_version`. Written atomically (temp file + rename) after listener bind.
Legacy records without `endpoint_uri` remain readable. The lock is
authoritative; metadata is diagnostic.

**`connect_or_start_daemon`** is the canonical frontend entry point
(`src/core/instance.rs`). It tries a verified connection to the
user-scoped endpoint; readiness requires a `SnapshotDaemon` identity probe
response. If unavailable and autostart is enabled, it spawns
`codegg daemon start --endpoint <socket> --force-take-lock`, directs
stdout/stderr to `daemon.log`, polls for
readiness with bounded timeout, and reaps the child after readiness without
making the frontend its lifetime owner. Unix launches use a detached session;
Windows launches use an independent child process. Graceful `daemon stop` is
currently Unix-only; Windows stop/replacement behavior remains a closure
condition. Concurrent starters converge on
whichever process owns the singleton lock; if the child exits early, the
frontend continues probing through the original deadline.

`ConnectOrStartOptions` controls: `paths`, `autostart`, `startup_timeout`
(default 10 s), `poll_interval` (default 100 ms), and optional
`executable` override (also accepts `CODEGG_DAEMON_EXECUTABLE` env var).

`DaemonConnectError` variants: `StartupTimeout`, `InconsistentState`,
`ChildExited`, `Io`.

The ordinary TUI remains a daemon client by default. On Unix, `SIGINT` and
`SIGTERM` use the same cancellation path; graceful shutdown stops accepting
clients, drains within the configured bound, removes the owned
endpoint/metadata artifacts, and releases the lock. `daemon stop` verifies
the live wire daemon identity against metadata before signaling and waits
boundedly for observable cleanup without force-killing an unverified PID.

Endpoint selection is centralized in `DaemonPaths::resolve_for_endpoint`:
an explicit CLI endpoint wins over `CODEGG_CORE_ENDPOINT`, otherwise the
platform default is used. URI schemes are `unix://` on Unix and `npipe://` on
Windows; unsupported schemes fail explicitly. Custom endpoints reuse the
documented user-scoped lock, metadata, and log root. The production daemon opens and migrates the
user-scoped catalog (`codegg.db`) before normal runtime initialization;
project-local `.codegg/sessions.db` remains legacy/import storage only.

**`CoreRuntimeMode`** enum (`src/core/instance.rs:41`):
- `DaemonClient` (default) — connect-or-start against the singleton daemon
- `StandaloneInproc` — in-process core, no daemon interaction (`--standalone`)
- `StandaloneStdio` — `core-stdio` subprocess (`--stdio`)

`InprocCoreClient` is now only used by tests, embedding, and
`--standalone` mode. The default TUI uses `SocketCoreClient` through
`connect_or_start_daemon`.

The `PROTOCOL_VERSION = 2` constant is unchanged. The `generation` UUID
lives in the on-disk metadata file, not in the wire protocol.

### Workspace Registry and Execution Context (Phase 2)

Phase 2 introduces workspace identity as a first-class daemon concept. A
daemon may now serve multiple distinct workspaces (project roots) and must
track which workspace each execution context targets.

**`WorkspaceRegistry`** (`crates/codegg-core/src/workspace.rs`) is
daemon-owned and deduplicates canonical roots via `get_or_register`. Rejects
nonexistent paths and symlink aliases. `CoreDaemon` holds
`workspaces: Arc<WorkspaceRegistry>`.

**`ExecutionContext`** (`crates/codegg-core/src/workspace.rs`) is immutable
and passed by `Arc` through `TurnRunInput` to every daemon execution path.
Replaces `std::env::current_dir()` reasoning. Carries `workspace_root`,
`workspace_id`, `session_id`, and path policy. `TurnRunInput` has
`execution: Arc<ExecutionContext>`.

**`WorkspaceId`** is a typed `String` newtype identifying a registered
workspace.

**Session binding**: `CoreDaemon::bind_runtime_for_session` (`daemon_refresh.rs:830`)
resolves a `session_id` to a `SessionRuntime` via `SessionStore` +
`WorkspaceRegistry`. `TurnSubmit` and `AgentSelect` reject unbound sessions
with `session_unbound`. `ModelSelect` does not bind a runtime: it validates
session existence through `SelectionService::update`
(`src/core/session_selection.rs:641`) and then projects the selection into the
runtime cache best-effort, only after durable success
(`src/core/daemon_turns.rs:825-899`).

**Storage**: workspace tables were introduced by migration v22 (a `workspace`
table plus `workspace_id` index on `session`). The schema has advanced well
past that; the current layout version is defined by
`storage::STORAGE_LAYOUT_VERSION`
(`crates/codegg-core/src/storage/mod.rs`). Existing sessions are lazily
resolved on next access; their `directory` is canonicalized into a workspace
record.

**Protocol**: `WorkspaceSnapshot` DTO,
`CoreRequest::WorkspaceRegister|WorkspaceList|WorkspaceArchive|WorkspaceSnapshotRequest`,
`SessionSnapshot::workspace_id` + `directory`,
`ServerCapabilities::workspace_registration` + `workspace_snapshots`.
Raw `WorkspaceRegister`, global `WorkspaceList`, and unscoped
`ProjectRegister` are LocalOwner/proven-local only (`opaque` scope;
team principals fail closed); see `architecture/authorization.md` and
`architecture/workspace.md`.

**Static guard**: `scripts/check_daemon_cwd_usage.py` rejects *any*
`std::env::current_dir()` / `set_current_dir()` use in daemon-owned execution
surfaces (`src/tool/`, `src/scheduler/`, `src/core/`, `src/agent/worker.rs`,
`src/agent/turn_runtime.rs`, the Git execution services), exempting only
`#[cfg(test)]` modules and the two pre-session bootstrap files `src/main.rs`
and `src/exec.rs`. There is no line-pattern allowlist — the predecessor's
patterns matched the production sites themselves, so it passed with every
violation present. Use `--self-test` to confirm the guard still discriminates.

See `crates/codegg-core/src/workspace.rs` for the full contract.

### Scheduler-owned execution (Phase 5 cutover)

Daemon-owned heavy work crosses `JobSubmissionService` before admission. The
facade validates the workspace-bound payload, applies the central resource
profile and exclusivity policy, creates the durable job, and enqueues it as
one logical operation. `JobScheduler` then owns queueing, permits, attempt
lifecycle, cancellation, and completion persistence.

`CoreRequest::JobSubmit`, `CoreRequest::JobWait`, and
`CoreRequest::SchedulerSnapshot` are the client-facing boundary. The daemon
snapshot carries only a bounded scheduler projection; clients fetch full job
and attempt records through dedicated operations. A disabled scheduler is an
explicit error state in daemon mode, never a route back to direct execution.

The canonical non-shell process policy is implemented by
`src/managed_process.rs`. It receives a durable job/attempt provenance pair,
uses sanitized noninteractive environment defaults, manages process groups,
enforces timeout and cancellation cleanup, and bounds captured output.

Explicit `--standalone` and `--stdio` compatibility modes may retain narrow
legacy adapters for tests and embedding, but they do not participate in the
singleton daemon's machine-wide admission guarantee. See
[`scheduler.md`](scheduler.md) for the execution-surface inventory and
compatibility boundary.

### Implementation Notes

- The core protocol version is currently `2` (`PROTOCOL_VERSION` in
  `crates/codegg-protocol/src/core.rs:28`).
- `CoreDaemon` spans `~6,800` lines in `daemon.rs` plus 16 `daemon_*`
  family handler modules totaling ~17,400 lines, plus `daemon_family.rs`
  (~560 lines), plus four `daemon_*` lifecycle modules totaling ~3,400
  lines. It holds daemon identity, runtime deps, event log, session/client
  registries, notification router, workspace registry,
  workspace services, eggpool provisioner, selection service, asset refresh
  coordinator, project activation, and projection seam. Family handlers and
  lifecycle helpers are boring `impl CoreDaemon` methods operating on the
  same daemon-owned state; they introduce no new store, scheduler, state
  machine, or authority. Shared helpers used across families
  (`record_origin_with_decision`, connection DTOs, tool-program DTOs, audit
  emitters) stay in `daemon.rs` as `pub(crate)` so every family calls the
  same canonical implementation; refresh/binding helpers live canonically in
  `daemon_refresh` and bootstrap helpers in `daemon_bootstrap`. The dispatch
  futures stay boxed per family, preserving the pre-M002 stack discipline
  that keeps the top-level dispatcher small.
- Lifecycle ownership: `daemon_construct` owns `with_deps` /
  `with_deps_and_identity` / `new` in the documented 11-phase order with a
  single assembly point (no partial publish). `daemon_bootstrap` owns
  hydrate, event bridge, `recover_state`, `recover_jobs`, and replay, with
  `initialize_recovery_sequence` as the canonical in-process hydrate ->
  bridge -> recover order. `daemon_refresh` owns all asset/activation/health
  coordinators plus the shared binding resolvers. `daemon_shutdown` owns
  `Drop` + `abort_background_handles` (cancel precedes joins; scheduler loop
  stays detached by design). Two private join handles were widened
  `private` -> `pub(crate)` so construction/shutdown can own them without a
  public API change (same precedent as M002 helper widening).
- Projection transport ownership is connection-local in
  `src/core/transport/projection.rs`. The Unix socket and `/core` WebSocket
  retain daemon-issued subscription IDs, persisted stream descriptors,
  cursors, forwarder tasks, and cancellation state in the same bounded
  owner model.
- Local TUI flows should prefer `CoreClient` over direct store access when
  a request already exists in `CoreRequest`.
- The in-process client subscribes to `daemon.event_log` (via
  `EventLog::subscribe()`) when a daemon is present and forwards events to
  the channel receiver. In legacy no-daemon mode it falls back to
  `GlobalEventBus::subscribe()`. Actual event publishing happens inside
  `tokio::spawn` within turn execution handlers.
- `CoreDaemon` uses `CoreRuntimeDeps` to bundle runtime dependencies. The
  legacy convenience constructor remains for embedded callers; scheduled
  work is never supplied as a separate runtime dependency. Prefer
  `with_deps` for new code.
- Turn execution goes through the injected `TurnRuntime` trait
  (`agent::turn_runtime`). `CoreRuntimeDeps` always holds an
  `Arc<dyn TurnRuntime>` (defaults to `DefaultTurnRuntime`); the daemon
  calls `deps.turn_runtime.run_turn(input)` instead of constructing a
  runtime directly. The runtime owns tool registry construction, permission
  checker construction, agent loop construction, system prompt assembly,
  and background spawning.
- `build_agent_loop` is used only internally by `DefaultTurnRuntime`; its
  complete typed input prevents workspace identity from being patched into a
  partially initialized loop. New code should prefer `TurnRuntime`.
- `src/core/daemon.rs` has zero direct references to `AgentLoop`,
  `ToolRegistry`, `PermissionChecker`, `TaskToolRuntime`, or
  `build_session_tool_registry`.
- Daemon provider validation is intentionally duplicated (daemon validates
  provider existence before delegating to turn runtime) to preserve
  backward-compatible provider_not_found response shape.
- Daemon still owns: request validation, session_id/turn_id management,
  active-turn bookkeeping, TurnStarted event publishing, and CoreResponse
  return.

### Test Coverage

- `turn_submit_uses_injected_runtime` (`src/core/daemon.rs:5944`) —
  Verifies that `TurnSubmit` delegates to the injected `TurnRuntime` rather
  than constructing one inline.
- `request_family_routes_each_coherent_family`
  (`src/core/daemon_family.rs`) — Pins one representative variant per
  family (plus `Initialize` → turns, both session-attach spellings →
  sessions, chat/interactive pre-router classification) to
  `DaemonRequestFamily::of`, so a new variant cannot land without an
  explicit owner.
- `thin_dispatcher_delegates_to_family_handlers`
  (`src/core/daemon_family.rs`) — Drives a pool-less daemon through every
  capability probe plus the legacy `TaskList` rejection and asserts no
  response falls through to the historical `unimplemented` contract,
  proving the thin router reaches each owning family handler.
- Lifecycle ordering pins (`daemon_construct`, `daemon_bootstrap`,
  `daemon_refresh`, `daemon_shutdown`) — `construction_phases_match_documented_order`,
  `bootstrap_phases_match_documented_order`,
  `refresh_coordinators_match_documented_set`, and
  `shutdown_phases_match_documented_order` fail on silent reorder; the
  19 new lifecycle tests (5 construct + 5 bootstrap + 5 refresh + 4
  shutdown) cover wiring, pool-less shape, identity, service reuse,
  hydrate/bridge/recover order, missing-pool/table tolerance, health/activation
  fail-closed behavior, and abort-on-drop without new frameworks.

### Project context resolver

Daemon request handlers use the core-owned `ProjectContextResolver` for
session creation, loading, turns, and project-scoped listing. It performs
bounded input parsing and durable membership/lifecycle checks before
execution. The resolver does not authorize principals and does not scan the
filesystem or use process cwd as identity authority.

Because the resolver resolves a directory only by catalog lookup, a directory
CodeGG has never registered has no executable context at all. The frontend owns
closing that gap: `App::bootstrap_launch_project_context` runs at TUI startup,
before any session-scoped request, and issues `WorkspaceRegister` followed by
`ProjectRegister` for the launch directory. Both are idempotent, so a known
directory re-resolves to the same ids, and the resulting pair is written onto
the active tab via `set_active_identities`. Registration runs under the
`LocalOwner` broad policy, which is what a local socket TUI binds; an
unauthorized principal still cannot name an arbitrary root.

The launch directory is the frontend's own cwd, which is why this is safe in
`AppMode::RemoteCore`. The project picker's "register by path" remains gated to
`AppMode::Embedded` because there the path is user-supplied text that a remote
attach must not be able to place on a daemon with a different filesystem.

## Source verification

Verified 2026-10-06 against `crates/codegg-core/src/lib.rs`, `src/lib.rs`,
`src/core/*.rs`, `src/core/instance.rs`,
`crates/codegg-protocol/src/core.rs`, and
`crates/codegg-core/src/storage/mod.rs`. Corrected the `codegg-core`
module inventory, which listed 28 of the crate's 46 `pub mod`
declarations: added `agent_convergence`, `agent_run`, `agent_run_control`,
`agent_run_group`, `approval`, `audit`, `audit_instrumentation`,
`authorization`, `collaboration`, `decision`, `presence`, `run_result`,
`session_control`, `team`, `transport_auth`, `work_order`, `work_plan`,
and `worktree_service`, and stated the 46 total. Corrected the root
`src/lib.rs` re-export block, which omitted `agent_convergence`,
`agent_run`, `agent_run_control`, `agent_run_group`, and `work_plan`.
Corrected `daemon.rs` size `~4,600` → `~6,000` lines (actual 6,005) and
the family-module count nine → 15 handlers (actual 12,626 lines across the
15 handler modules, plus 423 in `daemon_family.rs`, plus 2,391 across the
four lifecycle modules); corrected the `DaemonRequestFamily::of` variant
count 166 → 232 to match `CoreRequest` (counted programmatically from the
enum body). Added the six undocumented `src/core/daemon_*` family
modules (`daemon_control`, `daemon_documents`, `daemon_lsp`,
`daemon_team`, `daemon_work_orders`, `daemon_workspace_dashboard`) and
the two undocumented modules `core::lsp_diagnostics_store` and
`core::work_order_coordinator`. Corrected `file.rs:line` refs:
`PROTOCOL_VERSION` `core.rs:26` → `:28`, `CoreRuntimeMode`
`instance.rs:42` → `:41`, and `turn_submit_uses_injected_runtime`
`daemon.rs:4489` → `:5944`. Verified accurate: the three `CoreClient`
implementations, the three `CoreRuntimeMode` variants, the
`DaemonPaths` / `DaemonInstanceGuard` / `DaemonInstanceMetadata` /
`DaemonConnectError` / `ConnectOrStartOptions` types and their
declarations in `instance.rs`, the `ConnectOrStartOptions` 10 s / 100 ms
defaults, `STORAGE_LAYOUT_VERSION` = 72, `PROTOCOL_VERSION` = 2, and the
`ExecutionContext` / `WorkspaceRegistry` contract.

Second pass (2026-10-06) against `src/core/daemon.rs`,
`src/core/daemon_family.rs`, `src/core/daemon_turns.rs`,
`src/core/session_selection.rs`, `src/core/instance.rs`,
`crates/codegg-client/src/paths.rs`, and `crates/codegg-core/src/lib.rs`:
- **Module count was 45, actual 46**: `decision` was missing from the
  inventory and from the total. Added with its own contract line.
- **The Module Inventory table was broken.** A prose paragraph about
  `codegg-client` had been inserted mid-table between the `core::transport`
  and `core::transport::projection` rows, silently dropping those rows out of
  the rendered table. Moved the paragraph after the table.
- **"Fallthrough variants (return `Ack` without processing)" was wrong.**
  It listed `Initialize`, `Subscribe`, `Resume`, `TurnCancel`, `TurnSteer`,
  `AgentSelect`, and `ModelSelect` as no-ops. All seven have real handler
  arms: `Subscribe` returns a `current_seq` probe (`daemon_turns.rs:643`),
  `Resume` replays or returns `ResyncRequired` (`:652`), `TurnCancel`/
  `TurnSteer` enforce the controller predicate (`:695`/`:737`), `AgentSelect`
  binds a runtime (`:798`), and `ModelSelect` runs the durable
  `SelectionService` CAS (`:825`). Only `Initialize` genuinely reaches the
  family's `_ =>` `unimplemented` arm despite being classified to Turns
  (`daemon_family.rs:292`). Rewrote the section.
- `SelectionService::update` ref `session_selection.rs:471-475` → `:641`
  (`:471-475` is the body of the free `update_selection` helper).
- `bind_runtime_for_session` is in `daemon_refresh.rs:830`, not `daemon.rs`.
- **Family-module line totals were overstated** as "~15,400 across the 16
  family modules". Actual: 15 handler modules = 12,626 lines, plus
  `daemon_family.rs` = 423, plus four lifecycle modules = 2,391. Corrected in
  both the Implementation Notes and the extraction-target paragraph.
- Verified accurate: the 15-variant `DaemonRequestFamily` enum and its
  `owner_module` mapping, the 11-family vs 4-pre-router split (verified by
  reading the dispatch in `daemon.rs:3545-3624` — chat, work-order, team,
  and interactive each return before the family match), the
  `Box::pin`-per-family discipline, the 11 construction phases
  (`daemon_construct.rs:460`), the `File::try_lock` + drop-release guard
  (`instance.rs:277`/`:587`), the `default_user_runtime_root` cascade
  (`crates/codegg-client/src/paths.rs:161-190`),
  the four root files under the daemon root, the hidden-CLI-flag test
  (`main.rs:1025-1053`), and `WorkspaceRegistry::get_or_register`
  (`workspace.rs:478`).
- **The Linux daemon-home cascade was wrong**: it read
  `${XDG_RUNTIME_DIR:-/tmp}/codegg (falls back to $HOME/.local/share/codegg
  when neither is writable)`. `default_user_runtime_root`
  (`crates/codegg-client/src/paths.rs:161-190`) tries, in order,
  `$XDG_RUNTIME_DIR/codegg`, `$XDG_DATA_HOME/codegg`, and
  `$HOME/.local/share/codegg`, with `/tmp/codegg` only as the
  non-Linux-Unix fallback. The Windows entry was also vaguer than the source
  (`dirs::data_local_dir()` else `env::temp_dir()`). This is the same cascade
  the `.opencode/skills/core/SKILL.md` table already carried correctly.

Third pass (2026-10-21) against `src/core/daemon.rs`,
`src/core/daemon_family.rs`, `crates/codegg-protocol/src/core.rs`, and the
`src/core/daemon_*.rs` files, after M006 (the scratch usage census) and M007
(the scratch retention sweep) landed. The two earlier passes above are left
as they were written; this pass records what had drifted since.

Every count below was re-measured against the source on this tree rather than
copied forward, and each is measured the way this document has always
measured it: the enum bodies counted programmatically, the family modules
read off `DaemonRequestFamily::owner_module`, and the line totals taken from
the files themselves.

- **The family-handler module count was stale: 15 → 16.** The extraction
  paragraph said "plus 15 `core::daemon_*` family handler modules". The
  `owner_module` table names **16** distinct family owner files:
  `daemon_assets`, `daemon_documents`, `daemon_goals`, `daemon_human_shell`,
  `daemon_jobs`, `daemon_lsp`, `daemon_ops`, `daemon_project_init`,
  `daemon_projection`, `daemon_projects`, `daemon_providers`,
  `daemon_scratch`, `daemon_sessions`, `daemon_team`, `daemon_turns`, and
  `daemon_work_orders`. `daemon_human_shell` had been added without updating
  this paragraph, and M006 added `daemon_scratch`.
- **The `of(&CoreRequest)` variant count was stale: 234 → 261.**
  `CoreRequest` now has **261** variants, counted programmatically from the
  enum body — the method this document already used.
- **The `DaemonRequestFamily` enum was stale: 15 variants → 18.** The enum
  holds `Assets`, `Providers`, `Sessions`, `Turns`, `Jobs`, `Projects`,
  `Goals`, `Projection`, `Ops`, `Chat`, `Team`, `Interactive`, `HumanShell`,
  `WorkOrders`, `Documents`, `ProjectInitialization`, `Lsp`, and `Scratch`.
- **The pre-router split was stale: 11 vs 4 → 13 vs 5.** The family router
  dispatches **13** families. **5** return earlier through their own
  pre-router arms — chat, team, interactive, human shell, and work orders —
  verified by reading the match arm in `daemon.rs`, which routes exactly
  those five before the family match. The old text named only chat and
  interactive as pre-router families.
- **The line totals were stale.** `daemon.rs` is **6,807** lines (the text
  said `~6,000`); the 16 family handler modules total **17,422** lines (the
  text said `~12,600`, and the earlier passes recorded 12,626 across 15);
  `daemon_family.rs` is **559** lines (the text said `~420`, earlier passes
  423); and the four lifecycle modules — `daemon_construct` (970),
  `daemon_bootstrap` (1,053), `daemon_refresh` (1,268), `daemon_shutdown`
  (151) — total **3,442** lines (the text said `~2,400`, earlier passes
  2,391).
- **The module inventory was missing two family modules.** Neither
  `core::daemon_human_shell` nor `core::daemon_scratch` had a row, so the
  inventory did not describe the dispatch it documented. Both rows are added.
- The two sub-handler modules, `daemon_control` (1,671 lines) and
  `daemon_workspace_dashboard` (547), total **2,218** lines. The earlier
  passes' 2,391 was the *lifecycle* figure, not the sub-handler one.

Verified accurate on this tree: the `Box::pin`-per-family discipline, the
`DaemonRequestFamily::owner_module` mapping, the four lifecycle modules and
their separated responsibilities, and the `CoreRequest`/`CoreResponse`
variant counts of **261** and **171** respectively.
