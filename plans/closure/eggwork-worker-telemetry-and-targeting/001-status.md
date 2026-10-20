# Eggwork Worker Targeting and Host Telemetry M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/eggwork-worker-telemetry-and-targeting/001-explicit-target-job-and-schedule-boundary.md`

Source subsystem roadmap:

- `plans/subsystems/eggwork-worker-telemetry-and-targeting-roadmap.md`

Repository baseline reviewed: `643aadcf` (CodeGG `main`, 2026-10-19)

Implementation commits or pull requests:

- `ceb62154` — explicit remote target boundary: typed wire target, schedule target preservation, host-boundary admission (shape / administrator / principal), remote kind-and-argv policy, target identity on the readback, static ownership rules 6 and 7.

Primary class: capability (no migration)

Existing closed evidence reused: `plans/closure/eggwork-fixed-target-remote-execution/003-status.md`
(implementation `1ce377ce`, hosted CI run `36745285774` success with live derived reuse under
required isolation).

## 1. Executive finding

The milestone's capability boundary is complete. An authenticated client can now name a
configured Eggwork node for a job or a durable schedule through the ordinary
`JobSubmit` / `ScheduleCreate` interfaces, and the daemon admits that name only when three
independent gates agree: the job is remotely runnable, the administrator approved that
node, and the caller holds the node-target capability in the resolved project scope.

The milestone's *infrastructure* boundary is equally deliberate: no second queue, no
tool-side or frontend-side targeting, no arbitrary host URL, no credential in a durable
field, and no fallback to local execution when a remote submission is refused. A refusal
is typed, creates nothing, and names no endpoint or key path.

The named target survives the protocol, the durable job store, a daemon restart, and a
schedule occurrence. Old clients and old persisted templates are unaffected: an omitted
target is the local executor, exactly as before.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Old `JobSubmitDto` deserializes as Local | `crates/codegg-protocol/src/dto.rs` `omitted_target_decodes_as_local` | pass | the field is `#[serde(default)] Option<ExecutionTargetDto>` |
| Old persisted `JobTemplate` materializes as Local | `crates/codegg-core/src/jobs/schedule_store.rs` `template_without_target_key_decodes_as_local` | pass | a real serialized template with the `target` key removed decodes as `Local`; no backfill |
| Local schedule and TUI tests unchanged | `cargo test -p codegg-core --lib jobs::schedule`; existing TUI task/schedule suites in the full sweep | pass | `JobTemplate.target` defaults to `Local`; the TUI submits subagent and test kinds, which were never remote-eligible |
| Explicit named target survives protocol roundtrip | `crates/codegg-protocol/src/dto.rs` `target_round_trips_through_the_wire_form` | pass | wire shape `{"kind":"eggwork_node","node_id":"…"}` |
| … `JobStore` persistence | `tests/eggwork_remote_execution.rs` `sqlite_target_and_handle_round_trip` (pre-existing) and `named_target_survives_a_store_restart` | pass | durable `target_kind` / `target_node_id` columns (schema v66) |
| … daemon restart | `tests/eggwork_remote_execution.rs` `named_target_survives_a_store_restart` | pass | a fresh store handle over the same database reads the target back and still routes to `ExecutorKind::Eggwork` |
| … due occurrence | `crates/codegg-core/src/jobs/schedule_store.rs` `sqlite_one_shot_schedule_materializes_one_remote_job`, `in_memory_materialization_preserves_remote_target` | pass | `JobTemplate::new_job_for_occurrence` is the single construction path |
| The correct Eggwork executor receives exactly one admitted attempt | `tests/eggwork_remote_execution.rs` `admitted_remote_target_reaches_the_eggwork_executor_once` | pass | one submit (`client.submits == 1`), routing is `ExecutorKind::Eggwork` |
| Unknown node fails closed, no implicit local job | `tests/eggwork_target_boundary.rs` `unapproved_node_is_refused_and_creates_no_job` | pass | `remote_target_not_approved`; `job_count == 0` |
| Unauthorized node fails closed | `tests/eggwork_target_boundary.rs` `contributor_cannot_name_a_remote_node_but_owner_can` | pass | a contributor is refused `authorization_denied`; the project owner is admitted |
| Remote Test fails closed | `tests/eggwork_target_boundary.rs` `remote_kind_without_a_remote_executor_is_refused` | pass | `remote_target_kind_deferred` |
| Remote Subagent fails closed | `tests/eggwork_target_boundary.rs` `remote_kind_without_a_remote_executor_is_refused` | pass | `remote_target_unsupported_kind` |
| Malformed target fails closed | `tests/eggwork_target_boundary.rs` `malformed_target_is_a_typed_refusal_not_a_local_fallback`; `crates/codegg-protocol/src/dto.rs` `unknown_kind_decodes_to_invalid_instead_of_failing_the_request`, `empty_node_id_decodes_to_invalid`, `missing_node_id_decodes_to_invalid`, `endpoint_override_is_rejected` | pass | `invalid_job_submit`; the refusal names the reason, never a local job |
| Invalid policy fails closed | `src/core/remote_targets.rs` `approved_node_below_ceiling_is_refused` | pass | `remote_target_policy_denied` |
| Missing node credentials fail closed | `src/core/daemon_construct.rs` (the Eggwork executor is registered only when `EggworkExecutorConfig::from_daemon_config` resolves) and `crates/codegg-config/src/schema.rs` `eggwork_validation_rejects_plain_http_and_embedded_secrets` | pass | an unresolvable node is never registered, so it can never be approved or targeted |
| A daemon with zero configured remote nodes refuses remote submission and keeps local behavior | `tests/eggwork_target_boundary.rs` `daemon_without_approved_nodes_refuses_every_remote_target` | pass | `remote_target_not_approved`, then a local submission still succeeds |
| One-shot and interval schedules target Eggwork | `crates/codegg-core/src/jobs/schedule_store.rs` `sqlite_one_shot_schedule_materializes_one_remote_job` | pass | |
| Overlap / missed-run do not duplicate; restart/replay preserves target and idempotency | `crates/codegg-core/src/jobs/schedule_store.rs` `sqlite_one_shot_schedule_materializes_one_remote_job` (the replay yields no second occurrence) | pass | the occurrence key is `(schedule_id, scheduled_for)`, written `INSERT OR IGNORE` in the same transaction that advances `next_run_at` |
| Cancel a queued/running remote job and observe the terminal result through the existing Job API | `tests/eggwork_remote_execution.rs` `cancellation_during_running_cancels_exact_handle`, `restart_with_live_remote_execution_cancels_and_interrupts` (pre-existing, unchanged) | pass (unchanged path) | a remote job is an ordinary `JobRecord` with a target; there is no second remote-job registry |
| Authenticated real-node mTLS fixture verifies execution and RunStore | `tests/eggwork_remote_execution_live.rs` (unchanged by this milestone); qualified by the closed predecessor | pass (unchanged path) | not re-run here — this environment is macOS and the fixture requires a Linux loopback node; see §10 |
| Static routing ownership guards still detect a bypass | `scripts/check_eggwork_target_routing.py` rules 1–5 unchanged and green | pass | |
| No frontend/tool locally spawns a remote-targeted command | `scripts/check_eggwork_target_routing.py` rule 6 (no `src/tool`, `src/tui`, `src/agent`, `src/command` file may mention `EggworkNode`) and rule 7 (production code constructs a remote target only at the two sanctioned boundaries), each proved non-vacuous by the 6-case `--self-test` | pass | a surface that cannot name a remote node cannot spawn one |

## 3. Production implementation evidence

### Protocol (plan §3.A)

- `ExecutionTargetDto` (`crates/codegg-protocol/src/dto.rs`) is the typed wire target:
  `Local`, `EggworkNode { node_id }`, or `Invalid { reason }`.
- Its `Deserialize` is **total**. An unknown kind, a missing or empty node id, and any
  key beyond the documented shape (an `endpoint`, a key path) decode to `Invalid` instead
  of failing the enclosing request. This is deliberate: the socket transport drops
  frames it cannot decode (`src/core/transport/daemon_socket.rs`), so a decode failure
  there would leave the caller waiting forever for a reply that can never arrive. The
  daemon therefore answers a malformed target with a typed refusal.
- `JobSubmitDto.target` is optional and additive. Omitted or `null` is `Local` — the
  exact pre-M001 behavior of every client, including the TUI, the desktop shell, and the
  socket daemon client.
- `execution_target_from_dto` (`crates/codegg-core/src/protocol_conversions.rs`) maps
  the DTO onto the domain `ExecutionTarget` and runs the domain `validate()` (non-empty,
  ≤128 bytes, identifier charset). A malformed target is a typed `invalid_job_submit`,
  never a silent fallback to local execution.
- The node id names a key of the daemon's `[eggwork.nodes]` table — the same identifier
  the executor resolves the target against. It is never an endpoint, a certificate path,
  or a key path.
- The protocol clients need no change: the field is optional and additive. The
  in-repo `JobSubmitDto` construction sites are the TUI `/test` command
  (`src/tui/commands/test.rs`) and the TUI task/schedule command
  (`src/tui/commands/tasks.rs`), both of which submit a local-only kind; the
  desktop shell (`apps/`) and the native client crate (`crates/codegg-client`)
  have no job-submission surface at all.

### Schedule target (plan §3.B)

- `JobTemplate.target` (`crates/codegg-core/src/jobs/schedule.rs`) is `#[serde(default)]`.
  The template is persisted as a JSON blob in `schedule.job_template_json`, so a template
  written before M001 has no `target` key and decodes as `Local`.
- **The domain target's JSON representation was corrected by this milestone.** The
  pre-existing `ExecutionTarget` used an adjacently tagged serde representation
  (`tag = "kind", content = "node_id"`), which nests the variant's own `node_id` field
  under a second `node_id` key: `{"kind":"eggwork_node","node_id":{"node_id":"builder"}}`.
  That form disagreed with the wire contract, so a client could not express a remote
  target in a schedule template using the documented shape — the template decode failed
  outright. The domain type is now internally tagged, so the domain form and the
  protocol DTO form are the same `{"kind":"eggwork_node","node_id":"builder"}`.
  No persisted row carried the old form: the target's durable representation is the
  `target_kind` / `target_node_id` **columns** (schema v66), and the only JSON
  persistence of the type is inside a schedule template, which could not carry a remote
  target before this milestone.
- `JobTemplate::new_job_for_occurrence` is the single construction path for a
  materialized occurrence, so the template's target is carried into the durable `NewJob`
  unchanged. The in-memory and SQLite materializer call sites both use it.
- `schedule_create_from_dto` validates the template's target, so a malformed node id is a
  typed `invalid_schedule_create` rather than a template that could never be admitted.
- `ScheduleGet` returns the target identity in `ScheduleRecordDto.target_node_id`.

### Host-boundary authorization (plan §3.C)

- `RemoteTargetPolicy` (`src/core/remote_targets.rs`) is built once from the daemon
  configuration and answers whether a node may be named. It holds node *identity* and
  declared posture only.
- `CoreDaemon::admit_remote_target` (`src/core/daemon_jobs.rs`) applies three fail-closed
  gates **before anything is enqueued**:
  1. **Shape** — `validate_remote_job` (`src/scheduler/eggwork.rs`).
  2. **Administrator** — the approved-node allowlist and the posture ceiling.
  3. **Principal** — `Capability::NodeTarget` in the resolved project scope.
- The administrator gate runs before the principal gate so an unauthorized caller learns
  nothing about which nodes exist. No refusal message contains an endpoint, a certificate
  path, or a key path.
- A refused submission creates no job at all — not even a local one — and a refused
  schedule creates no schedule, so it can never fire a local job later.
- Execution time re-checks the node's live posture against its declared policy (the
  executor's posture agreement). Creation time and execution time are deliberately
  different questions.

### Remote kind and argv policy (plan §3.D)

- `validate_remote_job` is the single policy for "can this job run remotely". It admits
  `Build`, `Lint`, `Format`, `ManagedProcess`, and `Shell` with a coherent command argv;
  it defers `Test` (remote execution cannot preserve TestRunner durable evidence) and
  refuses `AgentTurn`, `Subagent`, `Python`, `ToolProgram`, and every unhandled kind.
- The command argv must be coherent: non-empty, a non-blank program, no blank entry, at
  most `MAX_REMOTE_ARGV_ENTRIES` entries, each entry at most
  `MAX_REMOTE_ARGV_ENTRY_BYTES` bytes.
- `EggworkExecutor::validate` calls the same function, so a job admitted at the host
  boundary is always dispatchable and a job refused at the host boundary could never
  have run. One policy, two call sites, pinned by
  `host_boundary_and_executor_share_one_remote_policy`.
- No foreground or agent-tool named-node selection surface was added. The plan makes it
  optional ("may add"), and the two in-repo submission surfaces (the TUI `/test` command
  and the TUI task/schedule command) submit `Test` and `Subagent` kinds, which are never
  remote-eligible — adding a node picker there would imply remote subagent support,
  which the plan forbids. The client that can target a node is an authenticated
  protocol client.

### Introspection (plan §3.E)

- `JobSummaryDto`, `JobRecordDto`, and `ScheduleRecordDto` gained an optional
  `target_node_id`: identity only, no endpoint and no key path, optional on the wire so
  clients that predate it keep working.
- `JobGet`, `JobList`, `ScheduleGet`, `JobWait`, `JobStatus`, `JobCancel`, and the
  terminal notice all work against a remote job through the same paths as a local one.
  There is no second remote-job registry.

### Static guard

`scripts/check_eggwork_target_routing.py` gained two ownership rules and a `--self-test`
that feeds synthetic sources through the same detectors used against the real tree:

- Rule 6: no file under `src/tool`, `src/tui`, `src/agent`, or `src/command` may
  mention `EggworkNode`. A frontend or tool surface that cannot name a remote node
  cannot spawn a remote-targeted command locally.
- Rule 7: production code constructs a remote `ExecutionTarget` only at the two
  sanctioned boundaries — the protocol conversion (`crates/codegg-core/src/protocol_conversions.rs`)
  and the durable read path (`crates/codegg-core/src/jobs/store.rs`).

`--self-test` (6 cases) fails if a planted violation is not reported. It is wired into
`scripts/verify.sh quick`.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core --lib jobs::schedule
cargo test -p codegg-protocol --lib execution_target
cargo test --test eggwork_target_boundary
cargo test --test eggwork_remote_execution
python3 scripts/check_eggwork_target_routing.py
python3 scripts/check_eggwork_target_routing.py --self-test
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
```

### Results

All commands above were run on the implementation head in this environment
(macOS, `CARGO_BUILD_JOBS=2`).

| Command | Result |
|---|---|
| `cargo test -p codegg-core --locked` | **passed** — 878 library tests plus the crate's integration suites, 0 failed |
| `cargo test -p codegg-protocol --locked` | **passed** — 251 tests, 0 failed (8 of them the new `execution_target_tests`) |
| `cargo test -p codegg-config --locked` | **passed** — 99 tests, 0 failed (4 of them the new `eggwork` targeting tests) |
| `cargo test --test eggwork_target_boundary` | **passed** — 9 tests, 0 failed (new file) |
| `cargo test --test eggwork_remote_execution` | **passed** — 28 tests, 0 failed (3 of them new: the host/executor policy agreement, the single-admitted-attempt proof, the store-restart target proof) |
| `python3 scripts/check_eggwork_target_routing.py` | **passed** — "eggwork target-routing guard ok" |
| `python3 scripts/check_eggwork_target_routing.py --self-test` | **passed** — 6 case(s) |
| `python3 scripts/check_scheduler_bypass.py` | **passed** — "scheduler-bypass guard ok" |
| `python3 scripts/check_execution_ownership.py` | **passed** — "execution-ownership guard ok" |
| `python3 scripts/check_config_merge_coverage.py` | **passed** — "All config merge coverage invariants verified" |
| `python3 scripts/check_audit_coverage.py` | **passed** — "All audit coverage invariants verified" |
| `scripts/verify.sh quick` | **passed** — 40 steps, "Quick verification passed" (fmt, all guards, workspace `cargo check --all-targets`) |
| `cargo fmt --all -- --check` | **passed** — clean |
| `git diff --check` | **passed** — clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | **not clean — pre-existing failures only** (see below) |

### Pre-existing clippy failures (not introduced by this milestone)

The workspace clippy gate is red on this branch for reasons that predate this
milestone. Every reported location is in a file this milestone did not touch:

| Location | Lint |
|---|---|
| `crates/codegg-core/src/workspace_services.rs:156` | `clippy::unnecessary_sort_by` |
| `src/agent/tool_batch.rs:137` | `clippy::empty_line_after_doc_comments` |
| `src/mcp/mod.rs:554` | `clippy::collapsible_if` |
| `src/search_backend/eggsearch.rs:626` | `clippy::too_many_arguments` |
| `src/tool/grep.rs:781`, `src/tool/grep.rs:782` | `clippy::search_is_some` |

Re-running the workspace clippy with only `clippy::unnecessary_sort_by` allowed
(`-D warnings -A clippy::unnecessary_sort_by`) reports exactly the remaining four
locations and **no location in any file this milestone changed**. The milestone's
own code is clippy-clean; the gate is red because the branch is red. This is
recorded rather than repaired: the fixes are unrelated to the target boundary and
would put five unrelated modules in this milestone's diff. The hosted CI job
surfaces them independently of this change.

### Not run, and why

- `cargo test --test eggwork_remote_execution_live` — the live mTLS fixture requires a
  Linux loopback Eggwork node and is excluded from the default sweep; this
  environment is macOS. The path it covers is unchanged by this milestone, and
  its qualification remains the closed predecessor's evidence
  (`plans/closure/eggwork-fixed-target-remote-execution/003-status.md`, hosted CI run
  `36745285774`). No new live claim is made here.
- The full workspace test suite (`cargo nextest run --workspace --locked --profile ci`)
  was not run in this session; the focused suites above plus `verify.sh quick` are the
  evidence cited here, and the broader suite runs on the hosted CI job.

## 5. Invariant review

| Roadmap / plan invariant | Evidence | Status |
|---|---|---|
| Scheduler and `JobSubmissionService` remain the sole CodeGG execution admission; no tool or frontend bypass and no second queue | The target is only ever read at the daemon host boundary and carried on the durable job; rule 6 forbids a frontend or tool surface from naming a remote node; `check_scheduler_bypass.py` and `check_execution_ownership.py` green | holds |
| Explicit named `EggworkNode` persists durably, using configuration-owned endpoints and mTLS only, never arbitrary URLs or key paths in user job payloads | The wire target carries a node **id** only; the DTO decoder rejects any extra key such as an endpoint; the domain `validate()` bounds the id; endpoints and key paths stay in `[eggwork.nodes]` and are resolved at execution time | holds |
| Existing local jobs/schedules and serialized DTOs remain local when the target is omitted | `omitted_target_decodes_as_local`, `template_without_target_key_decodes_as_local`, `local_submission_is_unchanged_by_the_remote_boundary` | holds |
| Authorized remote requests fail closed for invalid/unknown/disallowed targets, unsupported kinds/policies or inaccessible nodes; no hidden local fallback | Three fail-closed gates before enqueue; every refusal is typed and creates nothing; `remote_kind_without_a_remote_executor_is_refused`, `malformed_target_is_a_typed_refusal_not_a_local_fallback`, `approved_node_below_ceiling_is_refused` | holds |
| Exact remote handle/lease fencing, no duplicate submit on restart, RunStore artifacts and bounded workspace transfer must not regress | The executor's remote path is unchanged; only `validate` was rewired to the shared policy function, with identical admit/reject outcomes; the pre-existing live and restart suites remain in place | holds |
| Never embed TLS credentials in durable job fields or logs | The durable job carries the node id only; the readback DTO carries the node id only; `RemoteTargetPolicy` holds posture and identity only, never key paths; the refusal messages name no endpoint | holds |
| A schedule created by an authorized principal must not silently elevate if node permissions later change | Creation-time validation is explicit, and the executor re-checks the node's live posture at execution time; the administrator allowlist is a construction-time snapshot, recorded in §10 | holds |
| No automatic target selection; no cluster selection | The target is always supplied by the caller or defaults to `Local`; nothing in the scheduler chooses a node | holds |

## 6. Failure and recovery review

- **Duplicate delivery and idempotency.** The occurrence key is
  `(schedule_id, scheduled_for)`, written with `INSERT OR IGNORE` in the same transaction
  that advances `next_run_at` / `last_occurrence_at`. A replay of the same tick cannot
  produce a second occurrence (`sqlite_one_shot_schedule_materializes_one_remote_job`).
- **Cancellation races.** Unchanged. A remote job is an ordinary durable job, so
  `JobCancel` acts on the same record as before, and the executor cancels the exact
  persisted remote handle.
- **Daemon or node restart.** The target is a durable column; a restarted daemon reads
  it back and still routes to the Eggwork executor
  (`named_target_survives_a_store_restart`). The executor's persisted-handle
  reconciliation is unchanged.
- **Partial persistence failure.** See §7 — the materializer's job creation and the
  occurrence insert are not in one transaction today. Dormant, because no production
  materializer is wired into the daemon.
- **Stale generation or lease.** Unchanged. The lease identity is still single-sourced
  and fenced by Eggwork.
- **Contention and resource release.** Unchanged; the scheduler permit still spans the
  whole remote lifetime.
- **Malformed or unauthorized input.** Covered by the total DTO decode and the three
  host-boundary gates. A malformed target produces a typed error rather than a dropped
  frame or a local fallback.
- **Bounded event and artifact behavior.** Unchanged; the readback additions are a
  single optional string.

## 7. Migration and compatibility review

- **Schema migration: none.** `STORAGE_LAYOUT_VERSION` is unchanged. The durable
  representation of a target already existed (schema v66 added `target_kind` /
  `target_node_id`), and `ExecutionTarget::default()` is `Local`, so historical rows
  read back as local. The only new persisted field is `JobTemplate.target`, which is
  `#[serde(default)]` and lives inside the template JSON blob — a template written
  before M001 has no `target` key and decodes as `Local`. A backfill would rewrite rows
  to say what they already meant, so none was written.
- **Backward compatibility.** The wire surface is additive in both directions:
  `JobSubmitDto.target` is optional, and `target_node_id` on the readback DTOs is
  optional. An old client that omits the field behaves exactly as before; an old
  client that ignores the new readback field behaves exactly as before.
- **Protocol negotiation.** No protocol version bump: the new field is optional and
  the new readback field is optional.
- **Configuration behavior.** `[eggwork.allowed_target_nodes]` is the approved-node
  allowlist; an absent list approves nothing. An allowlist entry that names no
  configured node is a **configuration error** reported by `Config::validate()`, not a
  silently ignored line. `[eggwork.required_isolation_policy]` and
  `[eggwork.required_network_policy]` are the daemon-wide posture ceiling; absent
  means the node's own declaration governs.
- **Rollback limitations.** Rolling back to a pre-M001 daemon is safe: the durable
  columns and the optional wire field are both ignored by the older code, and a remote
  job then simply has no executor for its target.
- **Legacy-path status.** Live and unchanged.

## 8. Security review

- **Authorization enforcement.** The host boundary applies the node-target capability
  (`node.target`, project **Owner** only) as a second decision inside `job_submit` and
  `schedule_create`. A contributor who may submit jobs still cannot name a remote node
  (`contributor_cannot_name_a_remote_node_but_owner_can`). The local-owner broad policy
  owns the daemon and passes without team state, as it does for every other gate.
- **Privilege boundary.** The administrator gate (`[eggwork.allowed_target_nodes]`) is
  independent of the principal gate: a node the administrator never approved is
  unreachable even for an owner, and a caller without the capability cannot reach a
  node the administrator did approve.
- **Secret handling.** No endpoint, certificate path, or key path is ever carried by
  the protocol, the durable job fields, the readback DTOs, the policy object, a log
  line, or a refusal message. `RemoteTargetPolicy` holds posture and identity only.
- **Path validation.** The node id is bounded (non-empty, ≤128 bytes) and restricted to
  an identifier charset by the domain `validate()`; the DTO decoder additionally rejects
  any key beyond the documented shape, so a client cannot smuggle an endpoint or a key
  path into a target.
- **Denial-of-service bounds.** The remote argv is bounded in entry count and entry
  size; a hostile argv is refused at the host boundary rather than forwarded.
- **Redaction.** The existing `Debug` redaction of the private-key reference in
  `EggworkNodeProfile` is unchanged; the policy never copies key material.
- **Audit behavior.** The existing origin-attribution capture on creation arms is
  unchanged: a refused request creates nothing and therefore writes no attribution row.

## 9. Documentation and operations

Updated:

- `architecture/jobs.md` — new "Explicit target boundary (M001)" section: wire shape,
  total decoding, the three host-boundary gates, schedule target preservation,
  introspection.
- `architecture/scheduler.md` — new "Host-boundary admission (M001)" subsection under
  the fixed-target Eggwork section.
- `architecture/config.md` — the two new `[eggwork]` keys, the approved-node allowlist
  semantics, and the posture ceiling.
- `architecture/authorization.md` — new "Remote node targeting (Eggwork M001)"
  subsection describing the second gate and why it is not a new request variant.
- `docs/configuration.md` — the `eggwork` group in the "What else configuration
  controls" table and the administrator-facing note.

Static guards:

- `scripts/check_eggwork_target_routing.py` — rules 6 and 7, plus the `--self-test`
  (6 cases), wired into `scripts/verify.sh quick`.

Operator note: the daemon resolves the `eggwork` configuration **once**, at
construction. Changing `allowed_target_nodes` therefore requires a daemon restart to
take effect. The `EggworkExecutor` node table behaves the same way, so the two are
consistent.

One **pre-existing** failure was repaired in the same change because it blocked the
canonical verification: `scripts/check_adr_integrity.py` failed on the subsystem
roadmap's "Upstream authority" line, which named the upstream Eggwork ADR as a local
`plans/adrs/` path (the upstream file is `ADR-0006-optional-gregg-host-telemetry.md`
inside the *Eggwork* repository). That path is in the *Eggwork* repository, not this
one, and the guard resolves `plans/adrs/...` references against this repository only. The reference was reworded to name the upstream document
without the local path prefix. The failure was verified to exist on the branch head
before this change (`git stash` → the guard failed with the same message); it was not
introduced by this milestone.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The live Linux mTLS fixture (`tests/eggwork_remote_execution_live.rs`) was not re-run in this environment (macOS; the fixture requires a Linux loopback node and is excluded from the default sweep). | The remote execution path it covers — lease fencing, workspace transfer, RunStore artifact import, restart reconciliation — is unchanged by this milestone, and the new code is a host-boundary decision plus a wire field. `EggworkExecutor::validate` was rewired to the shared policy function with the same admit/reject outcomes it had before, pinned by `host_boundary_and_executor_share_one_remote_policy`. No new live claim is made here. | None for this milestone. The next hosted CI run on the main push carries the live fixture (main pushes always require live). |
| low | The schedule materializer is not wired into the daemon, so the job-creation / occurrence-insert atomicity wrinkle is dormant: the materializer creates the job through the job store's own transaction, so a crash between job creation and the occurrence insert could leave a job without its occurrence row. | Not reachable in production today. | Address in the milestone that wires a production materializer (M003 or the scheduler-side schedule owner), together with an occurrence-level idempotency key. |
| low | The approved-node allowlist is a construction-time snapshot; a configuration reload does not change it for a running daemon. | An operator who edits `allowed_target_nodes` sees no effect until the daemon restarts. | Documented in §9. No code change requested. |
| low | The workspace clippy gate is red on this branch for reasons that predate this milestone (§4, "Pre-existing clippy failures"). The milestone's own code is clippy-clean. | The hosted CI clippy step fails independently of this change. | Repair the five pre-existing lint sites in their own change; they are unrelated to the target boundary. |

## 11. Roadmap disposition

**Milestone closed and the next dependency may proceed.**

M003 (explicit-target remote resource-policy convergence) is unblocked and ready: its
registered blocker was this closure, and the target contract, the node-target
capability, and the administrator allowlist now exist, so typed per-kind remote limits
can be defined against a real target contract.

M002 (authenticated host telemetry projection) remains blocked on the upstream Eggwork
M002 closure and a reviewed client pin. Nothing in this milestone changes that, and no
direct Gregg connection was introduced.

## 12. Registry updates

- `plans/registry.md`:
  - *Active subsystem roadmaps* — the Eggwork worker targeting row now reads
    "active planning; M001 closed, M002 blocked, M003 ready".
  - *Dependency-ready implementation plans* — the M001 row is removed (it is closed) and
    an M003 row is added as **ready**.
  - *Blocked work* — the M003 row now records that it is unblocked and ready since the
    M001 closure; the M002 row is unchanged.
  - *Recently closed or conditionally closed work* — an M001 closure row is added,
    pointing at `plans/closure/eggwork-worker-telemetry-and-targeting/001-status.md`.
- `plans/subsystems/eggwork-worker-telemetry-and-targeting-roadmap.md`:
  - the status line reads "M001 closed, M002 blocked on Eggwork telemetry API closure,
    M003 ready";
  - the dependency graph marks the M001 node closed and the M003 node ready;
  - the M001 milestone is marked closed with its closure record, and M003 is marked
    ready rather than "ready after M001";
  - the M003 paragraph records that M001 closed with the explicit target boundary.
