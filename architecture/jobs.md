# Durable Jobs and Schedules (Phase 4)

Phase 4 introduced a durable execution-control domain: jobs, attempts,
schedules, dependencies, cancellation, retry policy, and restart recovery.
Phase 5 adds a daemon-owned submission boundary and makes the global
scheduler the admission authority for scheduler-backed work. Durable jobs
remain the queue/lifecycle authority; RunStore remains the artifact and
execution-provenance authority.

## Purpose

Provide a daemon- and workspace-scoped durable queue with transactional
lifecycle transitions so every scheduled or deferred piece of work
survives daemon restarts, retries, and cancellations without silent
data loss.

## Where It Lives

| Module | Purpose |
|--------|---------|
| `crates/codegg-core/src/jobs/mod.rs` | Typed IDs, domain types (`JobKind`, `JobSource`, `JobPriority`, `ResourceRequest`, `RetryPolicy`, `IdempotencyClass`, `JobState`, `AttemptState`, `JobPayload`, `NewJob`, `JobRecord`, `JobAttempt`, `CancelReason`, `CancelResult`, `CancelOutcome`, `RecoveryPolicy`, `RecoveryReport`, `AttemptCompletion`), `JobStore` trait, `JobStoreError`, `recover_at_startup` |
| `crates/codegg-core/src/jobs/store.rs` | `InMemoryJobStore`, `SqliteJobStore`, `JobStoreQuery`, `JobSummary`, `validate_state_transition`, `job_state_transitions`, `attempt_state_transitions`, `validate_attempt_transition` |
| `crates/codegg-core/src/jobs/schedule.rs` | `ScheduleState`, `ScheduleKind`, `OverlapPolicy`, `MissedRunPolicy`, `ScheduleRecord`, `ScheduleSummary`, `ScheduleTemplate`, `ScheduleQuery`, `OccurrenceStatus`, `ScheduleError`, `ScheduleStore` trait, `OccurrenceMaterializer` trait, `ClaimedOccurrence`, `MaterializerError`, `JobTemplate`, `compute_next_run`, `missed_run_targets` |
| `crates/codegg-core/src/jobs/schedule_store.rs` | `InMemoryScheduleStore`, `SqliteScheduleStore` |
| `src/scheduler/submission.rs` | `JobSubmissionService`, `SubmissionKey`, idempotent durable create/enqueue boundary |
| `src/managed_process.rs` | Canonical managed argv execution: environment policy, process groups, cancellation, bounded output, and provenance |
| `src/job_dispatcher.rs` | `JobDispatcher` trait, `SubAgentJobDispatcher`, `NullJobDispatcher` |
| `src/job_recovery.rs` | `recover_jobs_at_startup` helper, `RecoveryReportSummary` |
| `src/background_task_migration.rs` | `migrate_legacy_background_tasks` |

## How It Works

A caller submits a `NewJob` through `JobSubmissionService`, which
validates the payload, applies the canonical resource profile, creates
one durable `JobRecord` in `Queued` state, and wakes the scheduler.
The scheduler's fair queue picks it up on the next reconcile tick,
asks the admission controller for permits, and dispatches to a typed
executor. The executor creates an `AttemptRecord`, runs the work, and
persists exactly one terminal `AttemptCompletion` that atomically
advances the parent job to a terminal state.

The daemon installs a bounded post-admission lifecycle publisher on the
submission service. After durable creation and successful enqueue, it awaits
channel capacity and forwards the `JobRecord` to `EventLog` as an owner-scoped
`JobCreated`; a closed receiver is logged and later snapshots still rebuild
from `JobStore`. Direct callers such
as `TaskTool` and `CoreRequest::JobSubmit` therefore share one publication
path; the request handler does not emit a duplicate. Scheduler admission and
terminal events resolve the record from `JobStore` before publication, so
session and turn ownership come from durable state. Session projection
snapshots rebuild jobs from a bounded session-indexed store query, selecting
live states before recent terminal records. See
`architecture/projection.md` "Background job visibility".

When the daemon has an active goal for the submitted session, the host
submission boundary attaches the reserved `goal_id` label to supervised
`Test` and `Subagent` jobs before enqueueing them. Callers do not provide
this label, and model/tool payloads cannot override it. Child evidence jobs
use the active-goal snapshot read at their own host submission boundary;
there is no inherited ambient or later-resolved goal identity. The label is
durable job metadata used for evidence correlation only and grants no
execution authority.

## Key Types & APIs

### Typed Identifiers (`mod.rs:346-465`)

All identifiers are opaque UUID v4 strings wrapped in newtypes. They
are never parsed as integers.

```rust
pub struct JobId(String);        // line 346
pub struct AttemptId(String);    // line 373
pub struct ScheduleId(String);   // line 394
pub struct DependencyId(String); // line 415
pub struct DaemonGeneration(String); // line 439
```

`DaemonGeneration::new()` (line 442) produces a fresh UUID at each
daemon startup. An attempt is valid only while its stored generation
matches the active daemon generation.

### Job Kinds (`mod.rs:472`, 15 variants)

```rust
pub enum JobKind {
    AgentTurn, Subagent, Build, Test, Lint, Format, Shell,
    ManagedProcess, Python, GitRead, GitMutation, Research,
    Maintenance, ToolProgram,
    #[serde(other)] Unsupported,
}
```

Unknown future kinds deserialize into `Unsupported` for forward
compatibility. The daemon refuses to execute `Unsupported` jobs but
persists them so newer daemons can pick them up.

### Job Source and Priority (`mod.rs:619, 651`)

`JobSource` distinguishes `Interactive`, `Scheduled`, `AgentDelegated`,
`Retry`, `Maintenance`, and `Api` origins. `JobPriority` has five
buckets (`Urgent` through `Maintenance`) — persisted and validated but
not yet used for admission ordering.

### Job Payload (`mod.rs:1012`)

Typed payload variants (`JobPayload`) carry enough data to rerun safely
without consulting stale client state. Secret material must never be
embedded — use credential references.

### Execution Target (`mod.rs:517`, schema v66)

`ExecutionTarget` selects where an attempt runs: `Local` (default) or
`EggworkNode { node_id }` (one named Eggwork node). It is persisted on
`NewJob`/`JobRecord` (`target_kind`/`target_node_id` columns; historical
rows read back as `Local`). The job stores only the node id; endpoints
and key material stay in daemon configuration (`[eggwork.nodes]`).

### Explicit target boundary (M001)

A client may now *ask* for a named node instead of leaving the target
implicit. The request surface is `JobSubmitDto.target`
(`Option<ExecutionTargetDto>`) and, for schedules, the `target` field of
the persisted `JobTemplate`.

Wire shape (`codegg-protocol/src/dto.rs`):

| Form | Meaning |
|---|---|
| field absent / `null` | local executor — the pre-M001 behavior, unchanged |
| `{"kind":"local"}` | local executor, stated explicitly |
| `{"kind":"eggwork_node","node_id":"<table key>"}` | the named `[eggwork.nodes]` entry |
| anything else | typed refusal (`invalid_job_submit` / `invalid_schedule_create`) |

The target decodes **totally**: an unknown kind, a missing or empty
node id, and any extra key (an `endpoint`, a key path — anything beyond
the documented shape) decode to `ExecutionTargetDto::Invalid` instead of
failing the enclosing request. Transports drop frames they cannot decode
(`src/core/transport/daemon_socket.rs`), so a decode failure here would
leave the caller waiting forever for a reply that can never arrive. A
malformed target is therefore refused with a typed error, and it is
never a silent fallback to local execution: a client that asked for a
remote node must not receive a local job instead.

The node id names a key of the daemon's `[eggwork.nodes]` table — the
same identifier the executor resolves the target against. It is never an
endpoint, a certificate path, or a key path.

#### Host-boundary gates

`CoreDaemon::admit_remote_target` (`src/core/daemon_jobs.rs`) applies
three independent gates to a remote target *before anything is enqueued*,
so a refused request creates no job — not even a local one — and a
refused schedule never becomes durable and can never fire a local job
later.

1. **Shape** — `validate_remote_job` (`src/scheduler/eggwork.rs`) is the
   single policy for "can this job run remotely". The kinds that can are
   the ones whose payload is a typed command: `build`, `lint`, `format`,
   `shell`, `managed_process`. `test` is deferred (remote execution
   cannot preserve TestRunner durable evidence) and `agent_turn`,
   `subagent`, `python`, `tool_program` have no remote executor. The
   command argv must be coherent: non-empty, a non-blank program, no
   blank entry, and within the remote argv bounds. The executor's own
   `validate` calls the same function, so a job admitted at the host
   boundary is always dispatchable, and a job refused at the host
   boundary could never have run.
2. **Administrator** — `RemoteTargetPolicy`
   (`src/core/remote_targets.rs`) is built once from the daemon
   configuration and answers whether a node may be named at all.
   `[eggwork.allowed_target_nodes]` is the approved-node allowlist; an
   absent allowlist approves nothing, which is the fail-closed default.
   `[eggwork.required_isolation_policy]` and
   `[eggwork.required_network_policy]` are the daemon-wide posture
   ceiling a targetable node must declare.
3. **Principal** — the caller must hold the node-target capability
   (`Capability::NodeTarget`, wire name `node.target`) in the resolved
   project scope. That capability is granted to project **owners** only,
   so a contributor who may submit jobs still cannot name a remote node.
   The local-owner broad policy owns the daemon and passes without team
   state.

The administrator gate is checked before the principal gate so an
unauthorized caller learns nothing about which nodes exist. No gate ever
names an endpoint, a certificate path, or a key path, and no refusal
message contains one.

Execution time re-checks the node's live posture against its declared
policy (the executor's posture agreement). Creation time and execution
time are deliberately different checks: creation answers "may this
caller name this node", execution answers "does this node still meet
its declared posture".

#### Schedule target preservation

`JobTemplate` carries the target (`#[serde(default)]`), so templates
persisted before explicit targeting existed decode as `Local` — exactly
the behavior they always had — and no storage migration is required.
`JobTemplate::new_job_for_occurrence` is the single construction path
for a materialized occurrence, so the template's target is carried into
the durable `NewJob` unchanged. `ScheduleCreate` validates and admits the
target before the schedule becomes durable; `ScheduleGet` returns the
target identity in `ScheduleRecordDto.target_node_id`.

#### Introspection

`JobSummaryDto`, `JobRecordDto`, and `ScheduleRecordDto` gained an
optional `target_node_id`. It is identity only — no endpoint, no key
path — and it is optional on the wire, so clients that predate it keep
working unchanged. `JobGet`, `JobWait`, `JobStatus`, `JobCancel`, and
the terminal notice all work against a remote job through the same
paths as a local one; there is no second remote-job registry.

`RemoteExecutionHandle` (schema version 1) persists the bound remote
identity on `JobAttempt` (`remote_handle_json`): node id, execution id,
generation, lease id. The Eggwork executor writes it before any
upload/execute side effect and reconciles it on restart instead of
resubmitting, so one CodeGG attempt maps to at most one accepted remote
execution. Bearer lease material lives only in this attempt-scoped row,
never in job payloads, progress text, or audit metadata. The M003
derived-transfer optimization cache is deliberately NOT persisted here:
it is an in-memory, node-scoped, bounded hint that starts cold after
restart and returns the executor to full materialization.

### Remote resource limits (M003)

A remote job may run under limits the node enforces with cgroup v2. The
limit vocabulary is deliberately **separate** from the abstract scheduler
tokens on `ResourceRequest`:

| Vocabulary | Carried by | Units | Meaning |
|---|---|---|---|
| abstract scheduler tokens | `ResourceRequest.cpu_weight`, `.memory_mb_hint`, `.process_slots`, `.io_weight`, `.network_slots` | none — they are weights and hints | how the **local** admission controller weighs a job |
| physical remote limits | `ResourceRequest.remote` (`Option<RemoteResourceLimits>`) | bytes, milli-percent of one CPU, task count | what the **node** must enforce |

The two do not correspond, and this repository never translates one into
the other. A CPU *weight* is not a CPU *quota*; a memory *hint* is not a
byte count. `scripts/check_eggwork_target_routing.py` rule 9 rejects any
`ResourceRequirements` literal that mentions an abstract token, and rule
8 confines node-limit construction to
`src/scheduler/eggwork_limits.rs` (the resolution) and
`src/scheduler/eggwork.rs` (the command spec).

#### The `cpu_millis` trap

The node's CPU field is named `cpu_millis`, which reads like a
millisecond CPU-time budget. **It is not.** It is a CPU *quota* expressed
in *milli-percent of one CPU*, and the node renders it as
`CPUQuota = value/10 . value%10 %`:

| Value | cgroup property | Meaning |
|---|---|---|
| `1000` | `CPUQuota=100.0%` | one full CPU |
| `500` | `CPUQuota=50.0%` | half a CPU |
| `2000` | `CPUQuota=200.0%` | two CPUs |
| `1` | `CPUQuota=0.1%` | a thousandth of a CPU |

Treating it as a time budget would ask for a thousandth of the intended
CPU — or, in the other direction, for a thousand times too much.
`eggwork_limits::cpu_quota_percent` is the single place that mapping is
written down, and CodeGG's own field is named
`cpu_quota_milli_percent` for the same reason.

#### Operator ceiling and job request

The **operator ceiling** is authoritative. It is written per node in
`[eggwork.nodes.<id>].resource_limits`, and each dimension carries a
value and an enforcement kind:

```jsonc
"resource_limits": {
  "memory_bytes":           { "value": 1073741824, "enforcement": "required" },
  "cpu_quota_milli_percent": { "value": 1000,         "enforcement": "required" },
  "pids":                   { "value": 64,            "enforcement": "required" }
}
```

`required` is a hard contract: the daemon refuses to submit when the
node cannot enforce the limit. `best_effort` is advisory: the node may
decline to apply it, and the outcome is reported truthfully as *not
applied* rather than being downgraded to *not requested*.

A job may supply its own request (`JobSubmitDto.remote_resources`).
The rules, in order of authority:

1. A job may **narrow** a dimension. The effective value is the job's.
2. A job may **not widen** a dimension — refused with
   `remote_limit_exceeds_ceiling`.
3. A job may **not introduce** a dimension the operator never configured
   — refused with `remote_limit_not_configured`.
4. A job may **not weaken** a `required` ceiling to `best_effort` —
   refused with `remote_limit_downgrade_refused`.
5. A job **may strengthen** a `best_effort` ceiling to `required`.

With no ceiling configured, a node runs remote work unconstrained — the
pre-M003 behavior — and a job that asks for limits against such a node
is refused (`remote_limit_not_configured`), because the operator wrote
no limit for it.

Every refusal happens at the host boundary, before the job becomes
durable, so a refused submission creates no job at all.

#### Durable representation

`ResourceRequest.remote` is `#[serde(default)]` and the request is
persisted inside the existing `resource_json` blob, so a job written
before M003 decodes as "no remote limits" and **no storage migration is
required**. The decision is documented rather than assumed: the stored
representation is a JSON document, so the extension is additive and
backward compatible by construction.

#### Capability gate

A `required` dimension needs the node to advertise the matching
capability (`resources.cgroups-v2.memory` / `.cpu` / `.pids`). The gate
runs in the executor's preflight against the **fresh** authenticated
capability set, and it refuses *before the workspace upload* — a limit
the node cannot enforce never reaches the node. A stale or empty
capability set refuses rather than assuming.

#### Truthful outcome

The node reports per-dimension outcomes (`NotRequested`, `NotApplied`,
`Applied`, `LimitExceeded`). The executor records them, together with
the limits that were asked for, in the RunStore manifest
(`RunManifest.remote_resources`, a `RemoteResourceRecord`). A node that
reported nothing leaves `outcomes` absent, which is deliberately not the
same claim as "no limit was applied".

The record never contains an endpoint, a certificate path, a key path, or
a host measurement, and `ExecutorMetrics.cpu_time_ms` /
`peak_memory_mb` are **not** populated from remote numbers — those are
Gregg host telemetry and belong to M002.

### Remote node host telemetry (M002)

`src/scheduler/eggwork_telemetry.rs` owns the read-only view of a remote
node's **host**. The node polls its own Gregg provider and serves the
resulting observation over its authenticated telemetry route; CodeGG reads
that route and nothing else. There is no Gregg connection in CodeGG, no
provider poll, and no daemon-managed provider process.

**The upstream contract is frozen.** `eggwork_core::NodeTelemetry` at schema
version 1 (`deny_unknown_fields`, with a `validate()` that enforces the
documented `state`/`observation` pairing and the age invariant), served by
`GET /v1/node/telemetry` behind the separate narrow `Operation::Telemetry`
grant — not folded into `status` — and read through
`NodeClient::telemetry()` / `telemetry_for(&NodeId)`.

**Identity is checked before the join.** The read uses `telemetry_for`, so a
document addressed to another node is refused rather than returned. The
snapshot is only ever joined to the node that was asked for.

**What CodeGG projects.** The snapshot carries the provider's state, the
freshness of the observation, and the metric dimensions the node actually
reported. Each dimension keeps the upstream tri-state: `Value` (the node
reported it), `Unknown` (the node supports it but did not report it this
time), and `Unsupported` (the host platform cannot produce it). The last
two are never rendered as a zero. The provider's error *category* is
carried; its message is not. No interface identity, hostname, mount path or
process id is ever carried.

**What the read is not.** It is not a measurement of a CodeGG execution:
per-attempt CPU, RSS and elapsed time stay the executor's own numbers. It
is not an admission input: a node's free memory never reserves capacity.
It is not a second scheduler: a read never creates, mutates or releases a
job, and it does not touch the queue.

**One coordinator.** `NodeTelemetryService` is daemon-owned and holds one
cache and one probe lock per named node. Concurrent readers of the same node
coalesce onto a single upstream read, and a repeated read inside the
30 s freshness window is served from the cache — a refresh loop cannot
amplify the node's poll of its provider. A failed probe is remembered for
3 s only, so a fixed node is visible again quickly. The read is bounded at
10 s, well inside the upstream client's own 120 s unary deadline.

**Refusals are typed and distinct.** A node that does not implement the
contract (`TelemetryUnsupported`), a node whose schema this client does not
understand (`ProtocolIncompatible`), a rejected response
(`InvalidResponse`), a transport failure and a timeout each produce their
own bounded diagnostic. A node that cannot be read still appears in the
answer with a diagnostic — it is never dropped, and it never reports a
measurement.

### Eggwork lease-fencing contract (corrective C001)

The persisted tuple must be byte-exact with the live Eggwork
`ExecutionHandle` accepted by the node. Eggwork stores
`lease_hash(lease_id)` at acceptance and fences control operations
against it: cancel/renew with a divergent lease token is rejected with
HTTP 403 `invalid_lease` (a wrong principal yields 403 `forbidden`).
Consequences for CodeGG:

- `derive_handle` mints exactly one lease token per fresh CodeGG attempt
  and copies it verbatim into both the live handle and the durable
  record (`RemoteExecutionHandle::from_parts`); no second token exists.
- `to_eggwork_handle` reconstructs the exact fenced tuple from durable
  state; retransmission of the same attempt reuses the persisted handle
  and never re-derives.
- A new CodeGG attempt derives a distinct execution id (deterministic
  digest over job id + attempt id) and therefore a fresh lease.
- Restart reconciliation observes the persisted execution: terminal
  snapshots map to completions, still-live executions are cancelled
  under their own persisted handle and the attempt reports
  `Interrupted`. Loss of the exact execution is a typed
  interruption/failure, never a local fallback.
- The scripted `ScriptedClient` seam optionally enforces the same
  fencing (`invalid_lease`); `tests/eggwork_remote_execution_live.rs`
  qualifies the production `NodeClientFactory` path against a real
  loopback node with mTLS (Linux).
- Execution policy is daemon-owned node configuration, not durable job
  intent. `none` + `unrestricted` preserves legacy behavior and is
  explicitly diagnosed. `required` filesystem isolation and `disabled`
  networking require fresh authenticated capability/status agreement
  before workspace upload; the resulting spec follows the configured
  policy with no silent downgrade. M002a qualifies `required` over the
  production mTLS path against the corrected Eggwork pin: workspace access
  succeeds, outside-workspace read/write is denied, and terminal evidence
  reports applied `workspace_rw`. Network-disabled execution remains
  unsupported and is refused before upload.

### JobState Machine (`mod.rs:895`)

`JobState` has 10 variants: `Scheduled`, `Queued`, `Running`, `Completed`,
`Failed`, `Cancelled`, `TimedOut`, `Interrupted`, `Blocked`, `Expired`.
The transition table lives in `job_state_transitions()` (`store.rs:86`) and
is enforced by `validate_state_transition()` (`store.rs:70`):

```
Scheduled → Queued | Cancelled | Expired
Queued    → Running | Cancelled | Expired | Blocked
Running   → Completed | Failed | Cancelled | TimedOut | Interrupted
Completed → — (terminal)
Failed    → Queued (retry only)
Cancelled → — (terminal)
TimedOut  → Queued (retry only)
Interrupted → Queued (recovery only)
Blocked   → Queued | Cancelled | Expired
Expired   → — (terminal)
```

Terminal states (`Completed`, `Failed`, `Cancelled`, `TimedOut`,
`Expired`) never transition. Transitions go through `JobStore` methods
— no generic `set_state`.

### AttemptState Machine (`mod.rs:956`)

`AttemptState` has 8 variants: `Created`, `Admitted`, `Running`,
`Completed`, `Failed`, `Cancelled`, `TimedOut`, `Interrupted`.

```
Created|Admitted → Running | Failed | Cancelled | Interrupted
Running          → Completed | Failed | Cancelled | TimedOut | Interrupted
```

Terminal states (`Completed`, `Failed`, `Cancelled`, `TimedOut`,
`Interrupted`) never transition. `AttemptState::Interrupted` is used
during daemon generation recovery.

The transition table is authoritative in
`attempt_state_transitions()` (`store.rs:104`) and is enforced by
`validate_attempt_transition()` (`store.rs:119`):

| From | Allowed next states |
|------|---------------------|
| `Created` | `Admitted`, `Running`, `Failed`, `Cancelled`, `Interrupted` |
| `Admitted` | `Running`, `Failed`, `Cancelled`, `Interrupted` |
| `Running` | `Completed`, `Failed`, `Cancelled`, `TimedOut`, `Interrupted` |
| `Completed` | — (terminal) |
| `Failed` | — (terminal) |
| `Cancelled` | — (terminal) |
| `TimedOut` | — (terminal) |
| `Interrupted` | — (terminal) |

`Created → Admitted` is therefore a legal modeled transition, but it is not
a step the production scheduler performs durably:
`mark_attempt_running`'s SQL accepts `state IN ('created', 'admitted')`
(`store.rs:1815`), and the scheduler records admission in its in-memory
running-attempts map and calls `mark_attempt_running` directly
(`src/scheduler/scheduler.rs:1079`), so the durable attempt goes
`Created → Running`. `Admitted` remains decodable for external writers and
the projection replay path (`projection_replay/publication.rs:514`).

### JobStore Trait (`mod.rs:1668`)

24 methods on `JobStore`:

| Method | Purpose |
|--------|---------|
| `create_job(NewJob)` | Persist a new job, generate `JobId` |
| `set_job_labels(JobId, HashMap)` | Persist host-owned labels for a newly created job |
| `create_job_with_labels(NewJob, HashMap)` | Create a job and attach host-owned labels atomically |
| `get_job(JobId)` | Fetch by id |
| `get_jobs(&[JobId])` | Fetch multiple jobs in one store operation |
| `list_jobs(JobStoreQuery)` | Filter by workspace/state/kind/session |
| `count_jobs_by_kind_state(&[JobState])` | Count jobs grouped by kind for requested states |
| `list_job_records(JobStoreQuery)` | List full job records matching a query |
| `list_attempts(JobId)` | All attempts for a job, ordered by sequence |
| `enqueue(JobId)` | `Scheduled`/`Blocked` → `Queued` |
| `begin_attempt(JobId, DaemonGeneration)` | Create attempt, transition job to `Running` |
| `mark_attempt_running(AttemptId)` | `Created`/`Admitted` → `Running` |
| `set_attempt_executor(AttemptId, executor)` | Persist executor provenance before an attempt enters `Running` |
| `set_attempt_remote_handle(AttemptId, Option<RemoteExecutionHandle>)` | Persist the bound remote identity before remote side effects (Eggwork M001) |
| `set_attempt_source_subject_started(AttemptId, &ExecutionSubjectProvenance)` | Record the attempt-scoped execution source when it starts materializing |
| `seal_attempt_source_subject(AttemptId, &ExecutionSubjectProvenance)` | Seal the execution subject (v67 `source_subject_json`) |
| `record_heartbeat(AttemptId, DateTime)` | Persist heartbeat timestamp |
| `finish_attempt(AttemptCompletion)` | Atomically persist attempt + job completion |
| `request_cancel(JobId, CancelReason)` | Apply or record cancellation request |
| `retry_job(JobId, DaemonGeneration, AttemptId)` | Create new attempt for retry |
| `block_job(JobId)` | Transition to `Blocked` state for dependency waiting |
| `recover_generation(DaemonGeneration, RecoveryPolicy)` | Mark stale attempts `Interrupted`, requeue eligible jobs |
| `find_descendants(JobId) → Vec<JobSummary>` | Find all non-terminal child jobs of a parent (M012) |
| `cancel_descendants(JobId, CancelReason) → usize` | Cancel all non-terminal descendants; returns count (M012) |

### ScheduleStore Trait (`schedule.rs:235`)

6 methods on `ScheduleStore` (`create`, `set_state`, `delete`, `get`,
`list`, `claim_due`):

| Method | Purpose |
|--------|---------|
| `create(ScheduleTemplate)` | Persist a new schedule |
| `set_state(ScheduleId, ScheduleState)` | Pause/resume/archive |
| `delete(ScheduleId)` | Remove schedule |
| `get(ScheduleId)` | Fetch by id |
| `list(ScheduleQuery)` | Filter by workspace/state |
| `claim_due(DateTime, &dyn OccurrenceMaterializer)` | Atomically claim due occurrences, create jobs |

### `claim_due` Semantics (`schedule.rs`, impls `schedule_store.rs`)

`claim_due` is the only way an occurrence becomes a job. It runs in three
phases, and the split is load-bearing:

1. **Plan** — a short read of the schedules where `next_run_at <= now` and
   state is `Active`, plus the occurrences each one owes
   (`missed_run_targets`, by `MissedRunPolicy`) and the schedule's
   running-job count for the overlap policy. No transaction is held.
2. **Materialize** — the slow part: a job submission, and for a remote
   target a node round trip. It runs with **no** transaction open.
3. **Commit** — one write transaction that inserts the
   `schedule_occurrence` rows and advances the schedules' cursors
   together.

The split is not cosmetic. Materialization needs the job store, the job
store needs a pool connection, and materializing while the schedule store
held its own connection waits for a second connection that a
single-connection pool never has — a self-deadlock that no amount of
waiting resolves. The phases are safe to separate because the occurrence's
job id is a pure function of the occurrence (below), so a crash between
the materialization and the commit replays into the *same* job.

The commit transaction reads each schedule's stored cursor and writes it
back, so it opens with `BEGIN IMMEDIATE` (`ImmediateWrite`) rather than a
deferred transaction — a deferred transaction holds a shared read lock
across the read and then has to upgrade it to write, which SQLite refuses
with `SQLITE_BUSY` without waiting (C002).

The `PRIMARY KEY(schedule_id, scheduled_for)` constraint prevents
double-firing after restart, and the cursor only ever advances.

### Occurrence identity and idempotency (M004)

An occurrence is the pair `(schedule_id, scheduled_for)`. Its job id is a
pure function of that pair — `occurrence_job_id` in
`crates/codegg-core/src/jobs/schedule.rs` — and the job store derives a
scheduled job's id from its source. The `job.id` primary key is therefore
the **durable** uniqueness constraint for an occurrence: a database
guarantee, not a convention. `JobStore::create_job` is idempotent on that
id; a non-scheduled job keeps a random UUID v4, which cannot collide.

The consequence is that a daemon which stopped between "the job row was
written" and "the occurrence row was written" replays into the **same**
job. The occurrence table's primary key then records it. One occurrence
never owns two jobs.

The occurrence identity is **derived, not stored** — no migration, and
`STORAGE_LAYOUT_VERSION` is unchanged.

### Refused occurrences (M004)

A refused occurrence is **not** recorded. The occurrence row is the
durable "this already happened" marker, and its primary key would turn
the later retry into a no-op, so an unrecorded occurrence stays
retryable. The consequences are all deliberate:

- the schedule's `last_occurrence_at` does not advance past the refusal,
  so the occurrence is re-planned;
- the schedule's next horizon does not move past the refusal, so the
  occurrence is not skipped;
- `OccurrenceStatus::Failed` and `ClaimedOccurrence::job_id: Option<JobId>`
  report the outcome. An occurrence that produced no job says `None`,
  never a synthetic id.
- `ClaimedOccurrence::failure_code` carries the refusal's stable,
  secret-free reason code — `remote_target_not_approved`, `invalid_template`,
  `job_store_error` — so the operator's log says *why* a schedule did not
  fire, without any message, endpoint, or policy detail.

The code is a log-line fact, not a durable one: a refused occurrence is
not recorded, so the schedule's own durable state cannot answer "why did
my schedule not run?". That remains an open operability gap.

The cursor advances to the **occurrence**, not to the tick time. An
interval schedule anchored at `T0` therefore keeps its own grid instead
of drifting to whenever the daemon happened to tick.

### Production materialization (M004)

`src/scheduler/schedule_materializer.rs` is the production
`OccurrenceMaterializer`; `src/scheduler/schedule_driver.rs` is the
daemon-owned earliest-due wake loop that calls it. The materializer
creates a job through `JobSubmissionService` — the single production
admission facade — so a scheduled job is admitted exactly like a submitted
one, and it invokes no executor.

The materializer **re-applies the host-boundary gates** at materialization
time: the remotely-runnable shape, the administrator's approval, and the
operator's resource ceiling. A schedule is authorized once, when it is
created; by the time an occurrence fires, the daemon may have restarted
with a different configuration, and the administrator may have withdrawn
the node's approval. The *principal* gate is deliberately not re-applied:
a timer-driven occurrence has no live principal, and re-deriving one
would manufacture authority rather than check it.

Refusals carry a stable, secret-free reason code. No endpoint, credential
path, or policy detail reaches a log line or a durable field.

The driver is installed only where it can do its job: the scheduler
enabled (the submission facade refuses every submission otherwise) and a
pool present (a pool-less daemon has no cross-process durability to
claim). Schedule creation and resumption wake it.

### Delegated scratch attempt leases (managed-agent-scratch M002)

The accepted `AgentRunRecord` carries an immutable list of explicitly granted
shared scratch IDs. `SubagentJobExecutor` acquires leases for the run-private
resource and each shared resource under the scheduler `JobId` and `AttemptId`
before dispatch, then releases them only after the child worker returns. A
retry acquires the same accepted run snapshot under the new attempt identity.
Revocation prevents new grants and new lease acquisition; it does not remove
scratch from an attempt already admitted. Startup/recovery reconciliation
removes leases only when the durable job attempt is no longer created, admitted,
or running. Revoked shared roots are reclaimed only after the last attempt
lease releases. Terminal run-private scratch is reclaimed after the supervised
child returns. Soft-deleted session scratch is reclaimed after all durable
runs and jobs are terminal; active work defers cleanup, session restoration
cancels it, and daemon startup resumes cleanup for deleted sessions. Durable
run results and Git worktrees are stored separately from scratch.

## Configuration Surface

Job records carry their configuration at creation time. Key defaults
are centralized in `ResourceRequest::for_kind` (`mod.rs:713`):

| Kind | CPU | Memory hint | Processes | IO | Network | Default conflict |
|---|---:|---:|---:|---:|---:|---|
| AgentTurn | 1 | 512 MB | 1 | 1 | 1 | — |
| Subagent | 1 | 512 MB | 1 | 1 | 1 | — |
| Research | 1 | 512 MB | 1 | 1 | 1 | — |
| Build | 3 | 2048 MB | 1 | 3 | 0 | exclusive:workspace-mutation |
| Lint | 1 | 768 MB | 1 | 1 | 0 | — |
| Format | 1 | 256 MB | 1 | 1 | 0 | exclusive:workspace-mutation |
| Test | 2 | 1024 MB | 1 | 2 | 0 | — |
| Shell/ManagedProcess | 1 | 256 MB | 1 | 1 | 0 | — |
| Python | 1 | 512 MB | 1 | 1 | 0 | — |
| GitRead | 1 | 128 MB | 1 | 1 | 0 | — |
| GitMutation | 1 | 256 MB | 1 | 1 | 0 | exclusive:worktree-mutation |
| Maintenance | 1 | 128 MB | 1 | 1 | 0 | — |
| ToolProgram | 1 | 512 MB | 1 | 1 | 0 | — |

`RecoveryPolicy` defaults (`mod.rs:1630`): requeue `ReadOnly` and
`SafeRepeat`; never auto-retry `Conditional`, `NonIdempotent`, or
`Destructive`.

## Invariants & Gotchas

### Recovery Contract (`mod.rs:1622`, `store.rs:915`)

At daemon startup (`recover_generation`):

1. All attempts in non-terminal states whose `daemon_generation` ≠
   the current generation are marked `Interrupted`
2. Parent jobs whose `RecoveryPolicy` permits requeue for the job's
   `IdempotencyClass` are transitioned to `Queued`; otherwise they
   are left in `Interrupted`
3. Default `RecoveryPolicy`: requeue `ReadOnly` and `SafeRepeat` jobs;
   never auto-retry `Conditional`, `NonIdempotent`, or `Destructive`
4. A `RecoveryReport` is returned summarizing interrupted attempts,
   requeued jobs, terminal jobs, and schedules reconciled

The idempotency class is persisted at creation time — it is never
re-inferred from code at restart.

### `recover_at_startup` integration (`src/scheduler/scheduler.rs:1764`)

`JobScheduler::recover_at_startup` calls `JobStore::recover_generation`
once at daemon startup and wakes the scheduler with
`WokeReason::Reconciled` so the fair queue is rebuilt from durable
state. `src/job_recovery.rs` provides a thin
`recover_jobs_at_startup` wrapper used by `CoreDaemon::recover_jobs`.
The return type is `RecoveryReportSummary` (`src/job_recovery.rs:19`),
a compact summary suitable for operator-facing logging.

### InMemory vs SQLite recovery (`store.rs:915, 2263`)

Both implementations were historically divergent. The SQLite version
was canonical; the in-memory version had inverted comparison logic
(interrupting attempts whose generation *matched* the new generation
rather than those that differed). This was fixed so both implementations
agree: the `stale` parameter is the *new* daemon generation and
attempts whose stored generation differs are interrupted.

### Cancellation Race Semantics (`store.rs:770`)

Deterministic precedence rules (`request_cancel`):
- **Queued/Blocked/Scheduled job, no attempt started**: transition
  directly to `Cancelled`
- **Running job**: persist `cancel_requested_at` and reason; return
  `CancelOutcome::Requested` to caller; the active executor is
  notified via `CancellationToken`
- **Terminal job**: reject with `CancelOutcome::AlreadyTerminal`

If completion is persisted before cancel request, the job remains
completed. If cancel is persisted first but the process exits
successfully, the terminal state is `Completed` (not `Cancelled`).
Stale workers may not overwrite a terminal state.

### Descendant Cancellation (`src/scheduler/scheduler.rs:1346, 1747`)

When a parent attempt terminates (timeout, failure, cancel, interrupt),
the scheduler calls `cancel_descendants` to ensure children do not
outlive the parent. This runs both in the executor-completion task and
in `request_cancel`.

### RunStore Linkage (`JobAttempt` at `mod.rs:1255`)

`JobAttempt.run_id: Option<RunId>` links an attempt to a RunStore
record. The two stores serve different purposes:
- **JobStore**: queue/lifecycle/control state
- **RunStore**: execution provenance, output, artifacts, changes,
  rerun descriptors

When an executor calls `RunStore::begin_run`, the returned `RunId` is
persisted on the attempt. If RunStore begin fails, the job/attempt
record is kept and a structured persistence warning is recorded — the
process is never retried solely to obtain a `RunId`.

### Background jobs and terminal notices (M001)

`test.execution_mode=background` submits the same canonical `JobKind::Test`
through `JobSubmissionService` with a `SubmissionKey test-bg:<digest>` (same
key + same fingerprint reuses the job; same key + different payload
conflicts; distinct calls stay distinct), then durably subscribes
`(job_id, session, turn, invocation_key)` before returning a stable
`JobHandle { job_id, kind, status, submitted_at, inspect_ref, cancel_ref }`.
Foreground behavior is unchanged. Background requires a real
host-originated session context or fails without spawning.

One `JobTerminalNotice` per subscribed job is persisted from the final
outcome only (`Completed|Failed|Cancelled|TimedOut|Expired`; `Interrupted`
is retryable, never final). The notice carries kind, status, bounded
summary (2,048 chars), and RunStore link — never stdout dumps or secrets.
The `job` model tool (`status|wait|cancel|output`) reuses the
`JobGet/JobWait/JobCancel` authority plus exact-owner session scope;
`wait` timeout means still-running. See `architecture/scheduler.md`
"Generic job terminal notices" and `architecture/authorization.md" for the
owner rule.

### GitHub Actions watching (M004)

`src/scheduler/ci_watch.rs` registers one immutable watch target
(`github_actions` + owner/repo + exact run id, or commit SHA resolving to
exactly one run) and returns a stable `ciw-<digest>` handle. Observations
are short bounded GETs (15s timeout, 256KiB cap, ETag revalidation,
Retry-After honored, exponential backoff + deterministic jitter, attempt
budget, TTL) advanced at daemon-owned points — explicit `ci_watch` tool
calls, turn-boundary advancement (max 2 per boundary), and since C001 the
canonical idle coordinator (`CiWatchIdleDriver`: earliest-due wake, bounded
global advancement, empty queue sleeps). No per-watch interval, no
per-watch worker, no process/CPU permit held across polls; a dormant
watcher is one durable row (`ci_watch`, migration v75, C001 lease columns
in v76) plus at most one persisted CAS lease per due occurrence. Two
service handles never double-poll the same due lease; expired leases are
recoverable; terminal transitions release the lease and never re-poll.
Terminal `completed` maps every GitHub conclusion explicitly (`success` →
pass; `failure/cancelled/timed_out/action_required/startup_failure` →
typed failure with a bounded failing-step summary; `neutral/skipped/stale/
unknown` → attention, never passed). Transport failures (401/403/404/429/
5xx, redirects, malformed bodies) back off within budget and never become
"CI failed"; ambiguous/multiple runs, SHA drift, repo mismatch, and
cross-origin redirects fail closed as attention. Watch cancellation stops
watching only, never the remote workflow. Exactly one terminal M001 notice
(kind `ci_watch`, `notice_id = ciwatch:<watch_id>`) follows the persisted
final; terminal targets are never re-polled; M003 delivers notify/resume
from there. Base URLs come from daemon configuration
(`CODEGG_GITHUB_API_ALLOWLIST`, default api.github.com) and are never
model-supplied; credentials resolve per poll from `GITHUB_TOKEN`/`GH_TOKEN`
(never stored — rows keep `credential_ref` only) and never enter logs.

### Safe continuation and visibility (M003)

Background submission accepts `on_complete: notify_only|resume_owner`
(default `notify_only`). `resume_owner` creates one single-use continuation
lease (`job_continuation_lease`, migration v74) bound to the host-resolved
session/turn/principal/workspace ancestry with a bounded TTL (default 30m,
max 4h) and exactly one wake. Delivery consumes the lease once per logical
terminal at the next safe, user-authorized turn boundary — never a second
continuation under duplicates or concurrent frontends. Cancel/expiry/
revocation/foreign-owner suppresses the lease before any invocation.
Delivery states (`Pending → SessionEventCommitted →
AvailableToOwner/ResumeQueued → Consumed/Expired/Suppressed`) are queryable;
crash at each boundary reconstructs without duplicate inference. Ended turns
spend no tokens; the durable notice remains visible on reconnect/restart/
next turn. See `architecture/agent.md` "Background job continuation" and
`architecture/projection.md` "Background job visibility".

### Explicit idle owner continuation (C002)

C002 adds the opt-in idle leg on top of M003 without changing its next-turn
marker: a host-trusted `ContinuationGrant` (`continuation_grant`, migration
v77) authorizes at most one bounded owner turn for one background wait
(`notice_job_id` = scheduler `job_id` or `ciwatch:<watch_id>`), bound to
principal/session/workspace/origin-turn, allowed model/connection, budgets,
expiry, and max continuations (default 1). A model setting `resume_owner`
without a grant stays notify-only for idle (explicit denial); legacy leases
without a grant never auto-resume. Dispatch is exactly-once per
`(notice, grant)` via `idle_continuation` intents (`Pending → Eligible →
Claimed → TurnAccepted → Started → Completed`, plus
`Suppressed/Expired/NeedsAttention`, `UNIQUE(notice_id, grant_id)`):
the accepted turn id + cause `completed_background_job` persist before
provider dispatch, and ambiguity after provider submission becomes
`needs_attention` (never blind replay). Only host-authored compact
status/result refs enter authorized prompt context. Grant/queued/resumed/
declined/expired status is visible through existing session/attention
projections (no TUI polling); user cancel always wins before invocation.

### Production idle delivery (C004, delivery slice)

C004 wires the daemon-owned delivery leg without changing the next-turn
marker above: one `ContinuationService` singleton per daemon lifetime
(`CoreRuntimeDeps::shared_continuation_service`, `CoreDaemon::
continuation_service`; pointer identity is the singleton proof), plus a
single post-commit completion fanout. `JobNotificationService::
record_notice` sends each newly stored terminal (machine jobs and CI
watches share this one path; idempotent duplicates never send) over an
unbounded channel after its durable commit; a daemon-owned forwarder
publishes one owner-scoped `CoreEvent::BackgroundJobCompleted`
(session/notice/job identity, kind, terminal status, bounded redacted
summary — never stdout, credentials, raw CI logs, or remote
instructions) via the existing `EventLog` + projection/replay path, plus
an ephemeral `NotificationRouter` toast (dedupe-keyed by notice id, never
a second completion database). The event is persisted (`should_persist`,
`core_event_type = background_job_completed`, `Safe` publication), so
live subscribers and `Resume`/replay observe the identical notice
id/status without starting an agent turn, and a restarted daemon over the
same pool replays the same notice. Crash between the notice commit and
the hint publish heals by bounded startup reconciliation
(`CoreDaemon::reconcile_idle_completion_hints`, wired into
`recover_jobs`): durable notices missing from `core_event_log` get
exactly their hint republished, deduped by notice id — never a second
notice, never a turn. The forwarder checks
`EventLog::has_persisted_background_completion` before publishing, so a
second daemon over the same pool (stale `flock`, divergent
`CODEGG_DAEMON_HOME`) absorbs rather than duplicates hints; the residual
same-id race is benign (consumers dedupe by notice id). No per-job poller, no second
scheduler, no model tokens spent by delivery. C005 (ADR-0031, accepted)
closes the resume half the slice left blocked: an authenticated grant
workflow (`ContinuationGrantCreate`/`Revoke`/`Status` through daemon
request authority, server-resolved origin, immutable
`continuation_grant` attribution in migration v78) plus the
daemon-internal delegated entry `submit_idle_continuation_turn`
(post-commit fanout wake + bounded restart twin
`reconcile_idle_continuation_wakes` in `recover_jobs`; call-site guard
`scripts/check_idle_continuation_callsites.py`). The wake revalidates
the grant against current team/session/provider state, CAS-claims the
`(notice, grant)` intent, and admits through the canonical `TurnSubmit`
path attributed to the grant owner (`idle:<grant_id>` client marker).
Legacy rows without stored origin stay non-autonomous; one logical
terminal admits at most one owner turn (deterministic delegation
submission id + receipt before provider entry; ambiguity becomes
`needs_attention`); store caches never overwrite (DB-fresh grant loads
for authority decisions, never-overwrite intent fills).

### Boundary Enforcement

`crates/codegg-core/src/jobs/` is UI-, server-, plugin-, and auth-free:
it is the lowest level at which the daemon reasons about queued and
scheduled work. Run `scripts/check-core-boundary.sh` after touching
this module.

## Protocol Additions

Phase 4 adds:
- **13 `CoreRequest` variants**: `JobSubmit`, `JobGet`, `JobList`,
  `JobCancel`, `JobRetry`, `ScheduleCreate`, `ScheduleList`,
  `SchedulePause`, `ScheduleResume`, `ScheduleDelete`, `ScheduleGet`,
  `JobListAttempts`, `JobRecover`
- **13 `CoreResponse` variants**: matching responses for each request
- **18 `CoreEvent` variants**: `JobCreated`, `JobQueued`, `JobBlocked`,
  `JobAttemptCreated`, `JobStarted`, `JobProgress`,
  `JobCancelRequested`, `JobCompleted`, `JobFailed`, `JobCancelled`,
  `JobTimedOut`, `JobInterrupted`, `JobRetried`, `ScheduleCreated`,
  `ScheduleOccurrenceQueued`, `ScheduleSkipped`, `SchedulePaused`,
  `ScheduleResumed`, `ScheduleDeleted`
- **11 DTOs**: `JobSubmitDto`, `JobQueryDto`, `JobSummaryDto`,
  `JobRecordDto`, `JobAttemptDto`, `ScheduleCreateDto`,
  `ScheduleSummaryDto`, `ScheduleRecordDto`, `RecoveryReportDto`,
  `CancelResultDto`, `AttemptCompletionDto`
- **2 `ServerCapabilities` fields**: `durable_jobs`, `schedule_support`

Phase 5 adds `JobWait`, `SchedulerSnapshot`, the optional bounded
scheduler projection on `SnapshotDaemon`, and `submission_key` on
`JobSubmitDto`.

### Scheduler submission and execution

The active TUI task schedule/list/delete commands use the durable
`ScheduleCreate`, `ScheduleList`, and `ScheduleDelete` requests. They
carry the daemon-resolved workspace and session authority and use opaque
durable schedule IDs. The retained legacy `Task*` requests are an
explicit rejection boundary for old external clients; they do not start
or reach an independent background scheduler.

The TUI's live-work selector associates a job with an AgentRun by explicit
`job_id` and omits the duplicate job row. Tool programs enrich their matching
job row through the same ID. `JobGet` and `JobAttempts` back the job detail
dialog; `JobCancel` is issued only from its explicit cancel key, and the UI
reloads the record before presenting the resulting state. The daemon remains
the authority for owner/project scope and cancellation outcome. Read-only
observers can inspect but do not receive an enabled cancel affordance; other
request refusals remain daemon-authoritative and are reported without claiming
that cancellation succeeded.

`JobSubmissionService` validates payload size and kind, resolves the
canonical workspace, applies the central resource profile and
exclusivity rules, then creates and enqueues the durable job as one
logical operation. A repeated `SubmissionKey` with the same request
fingerprint returns the original job; the in-memory idempotency index
is intentionally scoped to one daemon generation.

`ManagedArgvExecutor` is only an adapter. Non-shell argv work delegates
to `ManagedProcessService`, which supplies sanitized noninteractive
environment defaults, process-group/session cleanup,
timeout/cancellation handling, bounded output, and
`CODEGG_JOB_ID`/`CODEGG_ATTEMPT_ID` provenance. The service retains
independent stdout/stderr head-plus-tail buffers (256 KiB per stream
by default), drains both pipes concurrently, and reports truncation,
timeout, cancellation, output-limit termination, sandbox-helper
failure, and cleanup diagnostics distinctly. Explicit sandbox launches
use the installation-owned helper sibling, an owner-only system-temp
launch spec capped at 64 KiB, and a private versioned status pipe
capped at 16 KiB; target output is never a control channel. On Unix,
finite executions run in a child session; cancellation and timeout
send SIGTERM, wait a bounded grace period, then SIGKILL the verified
process group and reap the direct child. Other platforms retain
direct-child cleanup only. On Linux, constrained launches also pass through
the internal composable launch contract owned by this service. Its host
feature probe is diagnostic only: the service still selects the existing
filesystem helper, and namespace, offline-network, seccomp, and cgroup
dimensions remain unavailable until native enforcement is implemented and
qualified. Probe results never establish a child's obtained status.
Shell, TestRunner, and SubAgentPool retain
their domain semantics behind typed executors; TestRunner is an
explicit lifecycle exemption because it streams parser input into
durable line-oriented test logs and owns stall-timeout semantics.
Synchronous callers use the bounded `run_blocking` adapter and streaming
foreground callers use `run_streaming`; neither adapter admits durable work.
See `architecture/process-tool-execution-ownership.md` for the complete
spawn-site disposition and protocol exceptions.

### Tool program child-job composition (M007)

Tool programs may submit scheduler-owned child jobs via
`submit_job(op, config)` in restricted-Python source. The
`ExecuteChildJob` IR opcode triggers
`BrokerCallback::submit_child_job()`, which maps `ChildJobOp` variants
(`Test`, `Build`, `Lint`, `Format`) to typed `JobKind`/`JobPayload`
combinations and submits through `JobSubmissionService`. Child jobs
inherit the parent program's workspace, authority, and deadlines. They
use `IdempotencyClass::SafeRepeat` and `RetryPolicy::no_retry()`. The
broker adapter waits for completion and returns a `ChildJobResult` with
per-op typed details. See `architecture/tool_programs.md` §M007 for
the full contract.

### Tool Program ownership closure (M011)

Tool Program jobs are admitted only through `JobSubmissionService` and
are validated before attempt creation. Their durable payload includes
the generated program identity, explicit retry invocation key,
authority digest, frozen tool manifest, and serialized execution
context. The scheduler supplies the outer deadline and a durable
heartbeat sink. A child job uses the parent program/sequence in its
submission key, inherits parent session and turn identity, and
receives the narrower of its requested and parent deadlines.

The Tool Program executor persists per-call reservations, completions,
and interpreter checkpoints before advancing. A typed result record is
written before the scheduler completion is projected, and terminal
background notifications are derived from that record. Missing or
divergent context, source, authority, or call identity fails closed.

## Testing

| Category | Coverage |
|----------|----------|
| State-machine unit tests | Every valid and invalid transition, terminal-state monotonicity, concurrent completion/cancellation races, retry sequence numbering |
| Store tests | Create/get/list filters, transactional job/attempt transitions, concurrent attempt creation, cancellation while queued/running, dependency blocking, schedule occurrence uniqueness, overlap/missed-run policies, generation recovery; in-memory and SQLite implementations share a conformance suite |
| Migration tests | UUID background task imports, malformed IDs reported, ambiguous durations warned, idempotent re-migration |
| Fault-injection tests | Crash after job creation before attempt, after attempt creation before process start, after process completion before RunStore completion, after RunStore completion before JobStore completion; restart recovery at each state |
| Integration tests | Synthetic executors with marker files: one dispatch per attempt, cancellation delivery, retry history preservation, non-idempotent job non-requeue, frontend disconnect does not cancel durable jobs |

45 integration tests in `tests/durable_jobs_phase4.rs`.

### Narrowest run commands

```bash
cargo test -p codegg-core jobs                              # unit tests
cargo test -p codegg-core schedule                          # schedule tests
cargo test --test durable_jobs_phase4                       # integration tests
bash scripts/check-core-boundary.sh                        # boundary guard
```

## Related Docs

- `architecture/scheduler.md` — Phase 5 admission and dispatch
- `architecture/overview.md` — full module map
- `.opencode/skills/jobs/SKILL.md` — skill reference
- `architecture/tool_programs.md` — M007/M011 tool program contracts

## Source verification

Verified 2026-10-06 against `crates/codegg-core/src/jobs/{mod,store,schedule,schedule_store}.rs`,
`src/scheduler/scheduler.rs`, and `src/job_recovery.rs`. Corrected every
line ref: the prior set pointed at `jobs/store.rs` for both state enums,
which actually live in `jobs/mod.rs` (`JobState` `:895`, `AttemptState`
`:956`) — `store.rs` only holds the transition tables
(`job_state_transitions` `:86`, `attempt_state_transitions` `:104`);
`JobStore` trait `:1274` → `:1668`, `ScheduleStore` `:231` → `:235`,
`JobSource`/`JobPriority` `:554,586` → `:619,651`, `JobPayload` `:947` →
`:1012`, `ExecutionTarget` `:515` → `:517`, `for_kind` `:648` → `:713`,
`RecoveryPolicy` defaults `:1240` → `:1630`, `request_cancel` `:557` →
`:770`, `recover_generation` `:694` → `:915` (SQLite `:2263`). Corrected
the scheduler refs to the real path `src/scheduler/scheduler.rs`
(`recover_at_startup` `:1764`, `cancel_descendants` `:1346, 1747`) and
`RecoveryReportSummary` `job_recovery.rs:18` → `:19`. Corrected
`JobStore` method count 21 → 24 and added the two undocumented
`set_attempt_source_subject_started` / `seal_attempt_source_subject`
methods with their real signatures. Refuted the prior review's
"omits `Created → Admitted`" claim only in part: the transition table at
`store.rs:104` *does* allow `Created → Admitted`, but the production
scheduler never writes it durably (it calls `mark_attempt_running`
directly, `store.rs:1815` accepts `'created'`/`'admitted'`), so both facts
are now documented. Also added the 10-variant `JobState` and 8-variant
`AttemptState` counts with their authoritative transition tables, and the
15-variant `JobKind` count.
# Execution source provenance

`JobAttempt.source_subject` is the authority for the source state consumed by
one execution. It is a versioned bounded envelope; a retry captures a new
subject and never inherits the preceding attempt's value. `JobRecord` labels
are not provenance authority. The additive schema v67 column is nullable so
legacy attempts remain unavailable; migration never consults a workspace.

The scheduler captures S1 under the canonical workspace lease before executor
side effects. Local live-workspace execution seals S2 after executor cleanup
and before terminal persistence. Equal subjects are Stable; differences are
Drifted while the execution status remains truthful. Eggwork seals around its
materialized source snapshot before upload/submit; later local edits do not
rewrite that immutable input identity. M003 preserves this ordering
exactly: capture S1, build the immutable full snapshot, seal the full
manifest digest, then choose full/derived transport. A derived patch is
never persisted as the historical source identity. Capture failure, non-Git workspaces,
legacy NULLs, and unsupported remote boundaries remain unavailable.

## Source verification (second pass)

Verified 2026-10-06 against `crates/codegg-core/src/jobs/{mod,store,schedule,schedule_store}.rs`,
`src/scheduler/scheduler.rs`, `src/job_recovery.rs`,
`crates/codegg-core/src/projection_replay/publication.rs`, and
`tests/durable_jobs_phase4.rs`. Four residuals left by the first pass were
corrected: the `claim_due` trait ref `schedule.rs:243` → `:262` (line 243 is
the first *implementation* in `schedule_store.rs`, not the trait method, and a
second impl sits at `:521`); the in-memory-scheduler ref
`scheduler.rs:1071-1078` → `:1079` (the actual `mark_attempt_running` call,
preceded by an explanatory comment at `:1074`); and the integration test count
`42` → `45` `#[test]`/`#[tokio::test]` functions in `tests/durable_jobs_phase4.rs`.
The `scheduler.rs` refs in this file are correct and were **not** affected by
the `JobScheduler` `:99` → `:135` drift seen in `scheduler.md` — this file
cites `recover_at_startup` `:1764`, `cancel_descendants` `:1346` and `:1747`,
and `mark_attempt_running` `:1079`, all verified in place.
Independently confirmed the first pass's corrections: the five typed-ID
newtypes (`JobId` `:346`, `AttemptId` `:373`, `ScheduleId` `:394`,
`DependencyId` `:415`, `DaemonGeneration` `:439`, with `new()` at `:442`),
the 15-variant `JobKind` (`:472`), `JobSource`/`JobPriority` (`:619`, `:651`),
`JobPayload` (`:1012`), `ExecutionTarget` (`:517`), `JobState` (`:895`),
`AttemptState` (`:956`), `JobAttempt` (`:1255`), `RemoteExecutionHandle`
(`:1536`), `RecoveryPolicy` (`:1622`, `Default` at `:1630`), the `JobStore`
trait (`:1668`) with exactly **24** methods matching the documented table in
order, the `ScheduleStore` trait (`schedule.rs:235`) with exactly **6**
methods, the transition tables (`store.rs:86`, `:104`) and validators
(`:70`, `:119`), `request_cancel` (`:770`), `recover_generation` (`:915`,
SQLite `:2263`), `mark_attempt_running`'s `state IN ('created', 'admitted')`
SQL (`store.rs:1815`), the generation-mismatch recovery predicate
(`store.rs:2277`), and the `"admitted"` projection replay arm
(`projection_replay/publication.rs:514`).
