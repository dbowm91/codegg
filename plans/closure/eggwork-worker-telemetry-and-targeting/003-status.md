# Eggwork Worker Targeting and Host Telemetry M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/eggwork-worker-telemetry-and-targeting/003-remote-resource-admission-and-limits.md`

Source subsystem roadmap:

- `plans/subsystems/eggwork-worker-telemetry-and-targeting-roadmap.md`

Repository baseline reviewed: `5a04cdad` (CodeGG `planning/eggwork-targeting-and-telemetry-m001-m003`, 2026-10-19)

Implementation commits or pull requests:

- `5748074b` — remote resource policy and scheduler capacity convergence: typed operator ceiling and job request, unit-safe conversion into the node's cgroup requirements, capability-gated fail-closed preflight, node-scoped scheduler reservations, typed remote outcomes with a bounded same-node wait, and truthful resource outcome recording.

Primary class: capability (no migration)

Existing closed evidence reused: `plans/closure/eggwork-fixed-target-remote-execution/003-status.md`
(implementation `1ce377ce`, hosted CI run `36745285774` success with live derived reuse under
required isolation).

## 1. Executive finding

The milestone's capability boundary is complete. An operator can now declare, per
Eggwork node, the physical limits that node may be asked to enforce — resident
memory, a CPU quota, and a process count — each with an explicit `required` or
`best_effort` enforcement kind, and a client job may narrow that ceiling but never
widen it, never introduce a dimension the operator did not configure, and never
weaken a mandatory limit. Every refusal happens at the host boundary before the
job becomes durable, so a doomed request creates nothing at all.

The milestone's *infrastructure* boundary is equally deliberate. The scheduler no
longer charges remote compute against the controller's local CPU, memory, and
process budget: a remote reservation consumes a named-node capacity key and one
controller transfer slot — drawn from its own `max_remote_transfers` budget
rather than the local CPU/memory/process or local network budgets — and it keeps
every exclusivity key. There is still one scheduler, one fair queue, and one
admission controller.

The upstream contract was audited rather than assumed, and the audit changed the
design. Eggwork's CPU field is named `cpu_millis` but is **not** a CPU-time
budget: it is a CPU *quota* in milli-percent of one CPU, which the node renders as
`CPUQuota = value/10 . value%10 %`. CodeGG's own field is therefore named
`cpu_quota_milli_percent`, and the two vocabularies — CodeGG's abstract scheduler
tokens and the node's physical limits — are never translated into one another. A
static guard now rejects any attempt to do so.

The optional telemetry phase (plan work package E) is explicitly **not** claimed
here and does not gate this closure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Exact Eggwork `cpu_millis` semantics audited and documented | `architecture/jobs.md` "The `cpu_millis` trap"; `src/scheduler/eggwork_limits.rs` `cpu_quota_percent` and `cpu_quota_is_milli_percent_not_a_time_budget`; upstream `eggwork-core/src/lib.rs` `ResourceRequirements` and `eggwork-runner/src/lib.rs` `ResourceSetupRequest::properties` at the pinned revision `e6a5d82` | pass | the node's `cpu_millis` is a CPU quota in milli-percent of one CPU, not a time budget |
| Maximum valid ranges documented and enforced | `crates/codegg-config/src/schema.rs` `MAX_REMOTE_MEMORY_BYTES` / `MAX_REMOTE_CPU_QUOTA_MILLI_PERCENT` / `MAX_REMOTE_PIDS`, matching the node's own `ResourceRequirements::validate`; `a_limit_beyond_the_node_bound_is_a_configuration_error` | pass | a value beyond the node's bound is refused, never clamped |
| Linux / non-Linux enforcement difference recorded | the node advertises `resources.cgroups-v2.*` only where a cgroup v2 backend exists; a node that cannot enforce a `required` dimension does not advertise the matching capability | pass | see `architecture/jobs.md` and §5 |
| Unit-safe typed conversion, not a blind token translation | `src/scheduler/eggwork_limits.rs` `combine_limits` / `resolve_remote_limits`; `scripts/check_eggwork_target_routing.py` rules 8 and 9 | pass | the abstract tokens never appear inside a `ResourceRequirements` literal |
| Explicit node/profile limit policy with required vs best-effort | `crates/codegg-config/src/schema.rs` `RemoteResourceLimits` / `RemoteResourceLimit` / `RemoteLimitEnforcement`; `src/scheduler/eggwork_limits.rs` | pass | per-dimension enforcement kind, matching the node's own `Requirement<T>` |
| Conservative defaults preserve the prior no-limits behavior | `crates/codegg-config/src/schema.rs` `remote_limits_are_absent_by_default_so_prior_behavior_is_preserved`; `tests/eggwork_remote_limits.rs` `a_remote_job_without_limits_is_admitted_and_keeps_the_legacy_profile` | pass | an absent ceiling means "no limits configured" |
| Operator ceiling is authoritative; a job hint cannot widen or disable it | `tests/eggwork_remote_limits.rs` `a_request_above_the_ceiling_is_refused_and_creates_no_job`, `a_request_that_weakens_a_mandatory_limit_is_refused`, `a_request_for_an_unconfigured_dimension_is_refused`, `a_request_against_a_node_without_a_ceiling_is_refused` | pass | each refusal is typed and creates no job |
| Validated against fresh authenticated capabilities in preflight | `src/scheduler/eggwork.rs` `preflight` (the conservative capability/status intersection) and `resolve_remote_limits(... &policy.features)` | pass | the gate uses the same fresh feature set the preflight just collected |
| Fail closed before upload when `Required` cannot be enforced | `tests/eggwork_remote_execution.rs` `a_required_limit_the_node_cannot_enforce_fails_before_upload` | pass | `uploads == 0` and `submits == 0`; the refusal precedes the workspace transfer |
| Never downgrade to `NotRequested` silently | `src/scheduler/eggwork_limits.rs` — a dimension is `NotRequested` only when neither the ceiling nor the request names it; `a_best_effort_limit_is_submitted_even_without_the_capability` | pass | a best-effort limit stays `BestEffort` on the wire |
| Invalid units and overflow fail closed | `tests/eggwork_remote_limits.rs` `a_zero_limit_is_refused_rather_than_read_as_unlimited`, `a_limit_beyond_the_node_bound_is_refused_rather_than_clamped`, `a_request_above_the_ceiling_is_refused_before_the_node_bound_is_consulted`; `crates/codegg-protocol/src/dto.rs` `an_unusable_value_decodes_to_zero_rather_than_failing` | pass | a zero limit is invalid, not "unlimited"; the operator's ceiling is the first authority consulted, so a request above it is reported as `remote_limit_exceeds_ceiling` |
| Resource token confusion fails closed | `tests/eggwork_remote_limits.rs` `an_abstract_scheduler_token_in_the_remote_slot_is_refused`; `crates/codegg-protocol/src/dto.rs` `an_unknown_dimension_is_refused_rather_than_ignored` | pass | `cpu_weight` in the remote slot is a typed refusal, not a silently dropped key |
| A malicious job override fails closed | `src/core/daemon_jobs.rs` `admit_remote_target` (gate 2b) and `src/core/remote_targets.rs` `check_resource_request` | pass | the host boundary applies the same rules the executor does |
| Policy threaded into `build_spec` | `src/scheduler/eggwork.rs` `build_spec(... resources)`; `tests/eggwork_remote_execution.rs` `resolved_limits_reach_the_node_spec_in_the_nodes_own_units` | pass | the node receives `Required(512 MiB)`, `Required(1000)` milli-percent, `Required(64)` |
| Requested / applied / not-applied / limit-exceeded outcomes recorded in bounded RunStore metadata | `crates/codegg-core/src/run_store.rs` `RemoteResourceRecord` / `RemoteResourceOutcomes` / `RemoteResourceOutcome`; `src/scheduler/eggwork.rs` `remote_resource_record`; `tests/eggwork_remote_execution.rs` `a_limit_the_node_does_not_apply_is_recorded_truthfully` | pass | a node that reports nothing leaves `outcomes` absent, which is not the same claim as "not requested" |
| Live handles, lease renewal, event streaming, and restart reconciliation unchanged | the executor's handle derivation, `set_attempt_remote_handle`, renewal task, and `reconcile_persisted` are untouched; `tests/eggwork_remote_execution.rs` `submitted_lease_matches_persisted_handle`, `restart_after_remote_acceptance_does_not_resubmit`, `lease_is_renewed_while_execution_is_live` (pre-existing) | pass (unchanged path) | |
| `ExecutorMetrics.cpu_time_ms` / `peak_memory_mb` not populated with host numbers | the executor still returns `ExecutorMetrics::default()`; the resource outcome is recorded in its own typed field | pass | Gregg host telemetry is M002's scope |
| Local and remote budgets split; remote concurrency no longer consumes fictitious local CPU capacity | `src/scheduler/admission.rs` `try_admit` (the remote branch); `src/scheduler/admission.rs` `a_remote_reservation_does_not_consume_local_cpu_or_process_capacity`, `remote_reservations_on_two_nodes_use_separate_budgets`, `remote_work_is_admitted_while_the_local_budget_is_full` | pass | four remote build-shaped reservations leave `used_cpu`, `used_memory`, and `used_process` at zero |
| A named-node capacity key and a small controller transfer budget | `src/scheduler/admission.rs` `a_node_ceiling_blocks_the_next_remote_reservation`, `a_released_remote_reservation_frees_the_node_capacity_key`, `the_controller_transfer_ceiling_is_its_own_budget` | pass | the transfer budget is `[scheduler.resources].max_remote_transfers` (default 16), deliberately separate from the local `max_network_slots`; it is charged and released with the reservation |
| Workspace exclusivity preserved for remote work | `src/scheduler/admission.rs` `a_remote_reservation_keeps_workspace_exclusivity` | pass | `exclusive:workspace-mutation` still contends |
| Local job classes unaffected by the split | `src/scheduler/admission.rs` `local_work_still_consumes_the_local_budget` | pass | the local accounting path is byte-for-byte the previous behavior |
| One scheduler, one fair queue, no second queue or remote-job registry | the split lives inside the existing `AdmissionController`; `ResolvedSchedulerConfig` gained one map; `check_scheduler_bypass.py` green | pass | no new queue, no new registry |
| Node busy / draining / offline / capability mismatch handled with typed outcomes | `src/scheduler/eggwork.rs` `RemoteAdmissionOutcome` | pass | `NodeBusy`, `NodeDraining`, `NodeUnreachable`, `CapabilityMismatch`, `IdentityMismatch`, `LimitRefused`, `Cancelled` |
| Retryable contention distinguished from non-retryable policy | `RemoteAdmissionOutcome::is_retryable` — only `NodeBusy` and `NodeUnreachable` | pass | |
| Retry/wait stays on the same node, bounded by backoff and deadline | `src/scheduler/eggwork.rs` `preflight_with_wait`; `tests/eggwork_remote_execution.rs` `a_busy_node_is_waited_on_and_retried_on_the_same_node` | pass | exactly one submission; the same handle |
| No silent move to another node and no local respawn of an accepted attempt | the wait never re-targets; the target is read once from the durable job; the persisted handle keeps a retry idempotent | pass | |
| No head-of-line blockage of other workspaces | the wait holds only the attempt's own workspace lease; the fair queue's per-workspace cap and the node-scoped key bound the rest | pass | |
| No retry storm and no stale-permit leakage | the wait budget is bounded in count, per-wait, and total deadline, and is clamped in configuration; the permit is released by the RAII guard on every exit | pass | |
| Node outage and retry exhaustion | `tests/eggwork_remote_execution.rs` `an_unreachable_node_is_waited_on_a_bounded_number_of_times` | pass | the probe count is exactly the budget plus one |
| A draining node is not waited on | `tests/eggwork_remote_execution.rs` `a_draining_node_is_not_waited_on` | pass | |
| Positive live Landlock and no remote/local fallback preserved | `tests/eggwork_remote_execution_live.rs` (unchanged); `tests/eggwork_remote_execution.rs` `capability_mismatch_fails_before_upload` (pre-existing) | pass (unchanged path) | live qualification is the closed predecessor's evidence; see §10 |
| Resource observation cannot grant additional execution permissions | the capability gate only *refuses*; a node that advertises nothing is refused a `required` limit, never granted one | pass | |
| Configuration merge behavior covered | `crates/codegg-config/src/schema.rs` `remote_limits_merge_per_dimension`, `node_resource_limits_survive_a_layered_merge`; `check_config_merge_coverage.py` green | pass | |
| Static guard still detects a bypass | `scripts/check_eggwork_target_routing.py` rules 8 and 9, proved non-vacuous by the 10-case `--self-test` | pass | |

## 3. Production implementation evidence

### The upstream audit (plan §3.A)

The pinned upstream revision is `eggstack/eggwork` at `e6a5d82e1ea392bb505aef1315c0e8c91d8baa0c`
(the revision this repository already depends on). Its
`crates/eggwork-core/src/lib.rs` defines:

```rust
pub struct ResourceRequirements {
    pub memory_bytes: Requirement<u64>,
    pub cpu_millis: Requirement<u64>,
    pub pids: Requirement<u32>,
}
pub enum Requirement<T> { NotRequested, BestEffort(T), Required(T) }
```

with `validate()` bounds of `1..=1_125_899_906_842_624` bytes,
`1..=1_000_000` for `cpu_millis`, and `1..=1_000_000` for `pids`. A `null`
requirement is `NotRequested`, a bare value is `BestEffort`, and
`{"required": v}` is `Required`.

The `cpu_millis` semantics were read from the node's own enforcement path
(`crates/eggwork-runner/src/lib.rs`): the runner turns the value into
`CPUQuota = value/10 . value%10 %` on a Linux cgroup v2 scope, alongside
`MemoryMax` / `MemorySwapMax` for `memory_bytes` and `TasksMax` for `pids`. So
`cpu_millis` is a CPU **quota** in milli-percent of one CPU — `1000` is one full
CPU, `2000` is two — and the name reads like a CPU-time budget only by accident.
Enforcement exists only where the node has a cgroup v2 backend; the node
advertises `resources.cgroups-v2.memory` / `.cpu` / `.pids` accordingly, and the
server refuses a `Required` dimension whose capability is absent.

The node also already owns the outcome vocabulary this milestone needed:
`ResourceResult { memory_bytes, cpu_millis, pids }` of
`ResourceDimensionResult` (`NotRequested` / `NotApplied { reason }` /
`Applied { backend }` / `LimitExceeded { backend }`).

### Configuration (`crates/codegg-config/src/schema.rs`)

- `RemoteLimitEnforcement { Required, BestEffort }` — `Required` is the default
  when a limit value is present, because the stricter reading is the safe one.
- `RemoteResourceLimit { value, enforcement }` — one dimension.
- `RemoteResourceLimits { memory_bytes, cpu_quota_milli_percent, pids }` — the
  operator's ceiling for one node.
- `EggworkNodeProfile.resource_limits` and `EggworkNodeProfile.max_concurrent_executions`.
- `EggworkConfig.remote_retry` / `EggworkRemoteRetryPolicy` — the bounded wait
  budget, with every bound clamped in `resolve()`.

Validation rejects a zero limit ("zero is not 'unlimited'"), a value beyond the
node's own bound, a zero concurrency ceiling, and an inverted backoff. The
per-dimension merge composes a layered configuration one dimension at a time.

### Protocol and domain (`crates/codegg-protocol/src/dto.rs`, `crates/codegg-core/src/jobs/mod.rs`)

- `RemoteResourceLimitsDto` is an internally tagged union with a `Limits` and an
  `Invalid` variant, and a **total** `Deserialize`: an unrecognized key, a
  non-object value, an unknown `kind`, a non-numeric `value`, and an unknown
  `enforcement` keyword all decode without failing the enclosing frame.
  Transports drop frames they cannot decode, so a decode failure here would hang
  the caller rather than answer it. An explicit `null` dimension is an *absent*
  dimension, not a zero limit: a request that merely omits a dimension must not
  be refused, and the round-trip test that proves it caught the difference.
- `JobSubmitDto.remote_resources` is the client's only channel for a remote limit
  request.
- `ResourceRequest.remote: Option<RemoteResourceLimits>` is `#[serde(default)]`
  and is persisted inside the existing `resource_json` blob.

### The resolution (`src/scheduler/eggwork_limits.rs`)

`combine_limits(ceiling, request)` applies the operator's rules with no node
contact, so the host boundary can refuse an impossible request before the job
becomes durable. `resolve_remote_limits(ceiling, request, capabilities)` adds
the capability gate and produces the node's `ResourceRequirements`.
`cpu_quota_percent` is the single written-down place where the upstream
milli-percent value becomes a cgroup percentage.

### Host boundary (`src/core/daemon_jobs.rs`, `src/core/remote_targets.rs`)

`RemoteTargetPolicy` now also holds each node's resource ceiling, and
`check_resource_request` runs as gate 2b of `admit_remote_target` — after the
administrator gate (so an unauthorized caller learns nothing about the fleet) and
before the principal gate. Both `JobSubmit` and `ScheduleCreate` go through it,
and a refusal creates no job and no schedule.

### Executor (`src/scheduler/eggwork.rs`)

- `preflight` returns a typed `RemoteAdmissionOutcome` instead of a string.
- `preflight_with_wait` wraps it in the bounded, same-node wait.
- The effective limits are resolved against the fresh capability set **before**
  the workspace upload, and a refusal names the dimension and the capability.
- `build_spec` carries the resolved requirements into the command spec.
- `remote_resource_record` maps the node's reported per-dimension outcome into
  the durable `RemoteResourceRecord`.

### Scheduler capacity (`src/scheduler/admission.rs`, `src/scheduler/permit.rs`, `src/scheduler/config.rs`)

`PermitDimensions` gained `remote_node: Option<String>`. The admission
controller's remote branch reserves a named-node capacity key and one
controller transfer slot, and charges every exclusivity key; it does not touch
the local CPU, memory, process, I/O, or local-network budget. The transfer
budget is `[scheduler.resources].max_remote_transfers` (default 16) — a
separate budget on purpose: a remote transfer must not be throttled by the
local network demand of unrelated jobs, and vice versa. `ResolvedSchedulerConfig` gained
`remote_node_capacity`, populated from the Eggwork configuration in
`src/core/daemon_construct.rs`.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core --locked
cargo test -p codegg-protocol --locked
cargo test -p codegg-config --locked
cargo test --lib scheduler::
cargo test --test eggwork_remote_limits
cargo test --test eggwork_remote_execution
cargo test --test eggwork_target_boundary
cargo test --test scheduler_contention
cargo test --test scheduler_phase5
python3 scripts/check_eggwork_target_routing.py
python3 scripts/check_eggwork_target_routing.py --self-test
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_config_merge_coverage.py
python3 scripts/check_audit_coverage.py
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
```

### Results

| Command | Result |
|---|---|
| `cargo test -p codegg-core --locked` | **passed** — 0 failed |
| `cargo test -p codegg-protocol --locked` | **passed** — 259 tests, 0 failed (8 of them the new remote-limit DTO tests) |
| `cargo test -p codegg-config --locked` | **passed** — 109 tests, 0 failed (10 of them the new remote-limit configuration tests) |
| `cargo test --lib scheduler::` | **passed** — 15 admission tests + the rest, 0 failed (the new admission-split and limit-resolution tests included) |
| `cargo test --lib` (whole lib) | 6678 passed, 6 failed — all six proven pre-existing, see "Pre-existing failures" below |
| `cargo test --test eggwork_remote_limits` | **passed** — 18 tests, 0 failed (new file) |
| `cargo test --test eggwork_remote_execution` | **passed** — 38 tests, 0 failed (the new limit, outcome, and wait tests included) |
| `cargo test --test eggwork_target_boundary` | **passed** — 0 failed (M001 suite unchanged) |
| `cargo test --test scheduler_contention` | **passed** — 0 failed |
| `cargo test --test scheduler_phase5` | **passed** — 0 failed |
| `python3 scripts/check_eggwork_target_routing.py` | **passed** — "eggwork target-routing guard ok" |
| `python3 scripts/check_eggwork_target_routing.py --self-test` | **passed** — 10 case(s) |
| `python3 scripts/check_scheduler_bypass.py` | **passed** — "scheduler-bypass guard ok" |
| `python3 scripts/check_execution_ownership.py` | **passed** — "execution-ownership guard ok" |
| `python3 scripts/check_config_merge_coverage.py` | **passed** — "All config merge coverage invariants verified" |
| `python3 scripts/check_audit_coverage.py` | **passed** — "All audit coverage invariants verified" |
| `scripts/verify.sh quick` | **passed** — "Quick verification passed" |
| `cargo fmt --all -- --check` | **passed** — clean |
| `git diff --check` | **passed** — clean |

### Pre-existing failures (proven, not caused by this milestone)

The whole-lib sweep surfaced six failures. Four of them fail *identically*
on the untouched baseline, and the other two are load-sensitive TUI terminal
tests that pass on the baseline once the machine is quiet. The proof: the
milestone's work was stashed (`git stash push -u`, baseline `5a04cdad`
verified byte-identical by `git diff HEAD | shasum` before and after), the
same tests were re-run, and the work was restored.

| Test | Baseline result | Cause |
|---|---|---|
| `core::daemon::tests::session_create_through_daemon` | **fails identically** | `InvalidRootPath("/tmp")` — on macOS `/tmp` is a symlink to `/private/tmp`, and the workspace-root validation rejects a symlinked root |
| `core::daemon::tests::turn_submit_rejects_when_active` | **fails identically** | the same `/tmp` root |
| `tool_advisor::causal_frontier::tests::m003_checked_in_receipt_matches_recomputation` | **fails identically** | a checked-in receipt fingerprint (`94ef4d74…`) no longer matches the recomputed one (`6c08e8d0…`) — pre-existing drift in an unrelated milestone's evidence file |
| `tui::commands::interactive_terminal::tests::a_refused_clear_reports_itself_and_leaves_the_screen_intact` | **fails identically** | an unrelated TUI terminal surface test |
| `tui::commands::interactive_terminal::tests::a_live_terminal_surface_repaints_with_no_further_user_input` | passes on the baseline | load-sensitive; the machine was running a second workstream's builds |
| `tui::commands::interactive_terminal::tests::a_hidden_terminal_surface_does_not_arm_the_loop` | passes on the baseline | load-sensitive; the same |

None of the six touches a module this milestone changed, and none of them
concerns the remote-target, resource-limit, or scheduler-capacity surfaces.

### Not run, and why

- `cargo test --test eggwork_remote_execution_live` — the live mTLS fixture
  requires a Linux loopback Eggwork node and is excluded from the default
  sweep; this environment is macOS. The path it covers (transport, lease
  fencing, artifact import) is unchanged by this milestone; the *new* limit
  path is CodeGG-side encoding that the node validates, and the node's own
  enforcement is upstream's. See §10 for the residual and the exact future
  evidence.
- The full workspace suite under nextest was not run in this session; the
  focused suites above plus `verify.sh quick` are the evidence cited here, and
  the broader suite runs on the hosted CI job.

## 5. Invariant review

| Roadmap / plan invariant | Evidence | Status |
|---|---|---|
| Scheduler and `JobSubmissionService` remain the sole CodeGG execution admission; no tool or frontend bypass and no second queue | the limit policy is applied by the existing host boundary and the existing admission controller; `check_scheduler_bypass.py` and `check_execution_ownership.py` green | holds |
| Explicit named `EggworkNode` persists durably; no arbitrary URLs or key paths in job payloads | the limit request carries dimensions and values only; the DTO rejects an extra key; the readback carries the target identity only | holds |
| Existing local jobs/schedules remain local when the target is omitted | `a_remote_job_without_limits_is_admitted_and_keeps_the_legacy_profile`, `local_submission_is_untouched_by_the_remote_limit_gates` | holds |
| Authorized remote requests fail closed; no hidden local fallback | every limit refusal is typed and creates nothing; the executor's own refusal is typed and never falls back | holds |
| Exact remote handle/lease fencing, no duplicate submit on restart, RunStore artifacts and bounded workspace transfer must not regress | the executor's handle, renewal, and reconciliation paths are untouched; the pre-existing suites remain green | holds |
| Gregg is never called by CodeGG | nothing in this milestone contacts Gregg; the executor's metrics are still `ExecutorMetrics::default()` | holds |
| Host metrics must not be falsely assigned to a single attempt | the resource outcome is a categorical, per-dimension record; no host measurement is written | holds |
| Do not charge remote CPU/memory as local machine execution forever | the admission controller's remote branch charges no local CPU, memory, or process budget | holds |
| Resource accounting changes require explicit contention qualification | the node-scoped key, the transfer budget, the separate-budget tests, and the busy-node wait are the qualification | holds |
| One global fair queue; Eggwork keeps its own local admission boundary | the node still refuses work it cannot take; the controller's node key only avoids flooding it | holds |

## 6. Failure and recovery review

- **Duplicate delivery and idempotency.** Unchanged for the limit path: the
  remote handle is derived before any side effect and persisted, so a retry
  under the same handle is idempotent on the node. The busy-node wait happens
  *before* the first upload, so it cannot create a second submission
  (`a_busy_node_is_waited_on_and_retried_on_the_same_node` observes exactly one
  submit and one handle).
- **Cancellation races.** A cancelled job does not sit out the wait budget: the
  cancellation is checked between waits, and the pre-existing
  `cancel_before_submit` path still applies after the preflight.
- **Daemon or node restart.** The limit request is durable (inside
  `resource_json`), and the restart-reconciliation path re-derives the effective
  limits from the operator's ceiling and the durable request — the capability
  gate decides acceptance, not the values. The pre-existing restart suites stay
  green.
- **Partial persistence failure.** The limit request rides the job's existing
  resource blob, so there is no new write to fail half-way. The schedule
  materializer's job-creation/occurrence-insert atomicity remains the dormant
  finding recorded by the M001 closure.
- **Stale generation or lease.** Unchanged; the lease identity is still
  single-sourced and fenced by Eggwork.
- **Contention and resource release.** A remote reservation's node key and
  transfer slot are released by the same RAII guard that released the local
  dimensions (`a_released_remote_reservation_frees_the_node_capacity_key`).
- **Malformed or unauthorized input.** Zero, overflowing, unit-confused, and
  credential-bearing limits are all typed refusals; the DTO never fails a frame
  decode.
- **Bounded event and artifact behavior.** Unchanged.

## 7. Migration and compatibility review

- **Schema migration: none.** `ResourceRequest.remote` is `#[serde(default)]`
  and is persisted inside the existing `resource_json` blob, so a job written
  before this milestone decodes as "no remote limits" and the stored
  representation needs no new column. The decision is documented rather than
  assumed: the stored form is a JSON document, so the extension is additive and
  backward compatible by construction. `STORAGE_LAYOUT_VERSION` is unchanged.
- **Backward compatibility.** Old clients that send no `remote_resources` are
  unaffected. Old persisted jobs decode as before. `JobSubmitDto`'s new field
  is optional, so old clients and new daemons interoperate.
- **Protocol negotiation.** The node's requirement encoding is unchanged: the
  same `Requirement<T>` wire form the node already accepts, with the same
  `NotRequested` / `BestEffort` / `Required` vocabulary.
- **Configuration behavior.** Every new key is optional with a conservative
  default, and the layered merge composes per dimension.
- **Rollback limitations.** A daemon that still holds a configured ceiling
  keeps enforcing it; rolling back the binary removes the ceiling *and* the
  job-level request, so a job submitted with a request would run with no limit
  after the rollback. That is a widening of the enforced limit, which is why the
  ceiling is operator-owned and the request is a narrowing of it.
- **Legacy-path status.** A daemon with no `resource_limits` configured runs
  remote work exactly as before this milestone.

## 8. Security review

- **Authorization enforcement.** The limit gates run inside the existing remote
  target admission, after the administrator gate and before the principal gate.
  A contributor still cannot reach a remote node at all, and the limit refusal
  adds no new authority to anyone.
- **Secret handling.** The limit vocabulary carries numbers only. The DTO
  rejects an extra key inside a limit, so an endpoint or key path cannot ride
  along (`a_credential_bearing_locator_cannot_ride_along_in_a_limit` asserts
  the refusal message carries neither).
- **Privilege boundaries.** A limit is a *narrowing* of what the node may do.
  There is no path by which a client widens the node's authority: a request
  above the ceiling, an unconfigured dimension, and a downgrade are all refused
  before the job is durable.
- **Denial-of-service bounds.** The wait budget is bounded in count, per-wait,
  and total deadline, and every configuration bound is clamped. The node-scoped
  admission key bounds how much work the controller sends at one node.
- **Redaction.** The refusal messages name a dimension, a value, and a
  capability — never an endpoint, a certificate path, or a key path.
- **Audit behavior.** The outcome record is bounded structured metadata in the
  RunStore manifest: three dimensions plus the limits asked for.

## 9. Documentation and operations

- `architecture/jobs.md` — new "Remote resource limits (M003)" section: the two
  vocabularies, the `cpu_millis` trap with its conversion table, the operator
  ceiling and job-request rules, the durable representation, the capability
  gate, and the truthful outcome record.
- `architecture/scheduler.md` — new "Local and remote budgets (M003)" section
  (what each reservation is charged against, the node-scoped key, the transfer
  slot, the preserved exclusivity, and the single-queue guarantee) and new
  "Typed remote outcomes and the bounded wait" section (the outcome table, the
  retryable/non-retryable split, and the same-node guarantee).
- `architecture/config.md` — the new `resource_limits`,
  `max_concurrent_executions`, and `remote_retry` keys with their units and
  defaults.
- `docs/configuration.md` — the operator-facing summary of the same keys.
- `scripts/check_eggwork_target_routing.py` — rules 8 and 9 (node-limit
  construction ownership, and the abstract-token/physical-limit confusion),
  with four new `--self-test` cases (10 in total).
- Operator diagnostics: a limit refusal is a typed `CoreResponse::Error` whose
  code names the dimension (`remote_limit_exceeds_ceiling`,
  `remote_limit_downgrade_refused`, `remote_limit_not_configured`,
  `remote_limit_invalid_value`, `remote_limit_capability_unavailable`) and a
  remote failure carries a typed code (`remote_node_busy`,
  `remote_node_draining`, `remote_node_unreachable`,
  `remote_capability_mismatch`, `remote_identity_mismatch`).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | The live Linux mTLS fixture was not re-run for the new limit path in this environment (macOS). The CodeGG-side encoding is covered by the scripted node fixture; the node's own cgroup enforcement is upstream's. | The end-to-end "a real node applied a required memory/CPU/PID limit and reported `Applied`" receipt is not produced here. | Run `tests/eggwork_remote_execution_live.rs` on a Linux runner with cgroup v2 and record the acceptance receipt. The upstream's own `resources.cgroups-v2.*` qualification is unchanged. |
| low | The workspace clippy gate is red on six pre-existing lint sites in files this milestone never touched (the same sites recorded by the M001 closure). | The milestone's own code is clippy-clean; the branch-level gate is red for unrelated reasons. | Repair separately; not widened into this milestone's diff. |
| low | The whole-lib sweep has six pre-existing failures (two macOS `/tmp` workspace-root rejections, one drifted tool-advisor receipt fingerprint, three load-sensitive TUI terminal tests). | Unrelated to this milestone; four fail identically on the untouched baseline and the rest are load-sensitive. | Repair separately; recorded in §4 with the baseline proof. |
| low | The schedule materializer is still not wired into the daemon, so job-creation/occurrence-insert atomicity is not yet guaranteed. | Dormant — inherited from the M001 closure. | The milestone that wires a production materializer. |
| low | `[eggwork.allowed_target_nodes]`, `resource_limits`, and `max_concurrent_executions` are construction-time snapshots. | A daemon restart is required after editing them. | Documented in `architecture/config.md`. |

## 11. Roadmap disposition

**Milestone closed and next dependency may proceed.**

The M003 capability boundary is complete: the operator's ceiling is enforced,
the job request may only narrow it, the unit confusion is structurally
impossible, the capability gate fails closed before upload, the local and
remote budgets are separated inside the one existing admission controller, and
every remote failure has a typed outcome with a bounded same-node wait.

The subsystem roadmap's M002 remains blocked on upstream Eggwork M002 and is
**not** advanced by this closure. The optional telemetry phase (plan §3.E) is
explicitly not claimed here.

## 12. Registry updates

- `plans/registry.md`:
  - the subsystem row becomes "M001 closed, M002 blocked, M003 closed";
  - the M003 row moves from the dependency-ready table to the closed table with
    this closure record and the implementation SHA;
  - the blocked-table M003 row is removed.
- `plans/subsystems/eggwork-worker-telemetry-and-targeting-roadmap.md`:
  - the status line, the dependency graph, and the M003 milestone entry become
    "closed", with a pointer to this record.
- `plans/implementation/eggwork-worker-telemetry-and-targeting/003-remote-resource-admission-and-limits.md`:
  - the status becomes `implemented`.
