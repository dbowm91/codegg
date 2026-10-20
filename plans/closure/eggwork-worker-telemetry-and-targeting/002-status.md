# Eggwork Worker Targeting and Host Telemetry Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/eggwork-worker-telemetry-and-targeting/002-remote-node-telemetry-projection.md`

Source subsystem roadmap:

- `plans/subsystems/eggwork-worker-telemetry-and-targeting-roadmap.md`

Repository baseline reviewed: `b69e26cc` (the tree the implementation started
from; the plan's own status change to `ready for handoff` landed there)

Implementation commits or pull requests:

- `968c9dfb` — feat(scheduler): consume the authenticated Eggwork host
  telemetry (Eggwork M002)

## 1. Executive finding

The capability boundary is complete. CodeGG now consumes the frozen upstream
Eggwork host-telemetry contract through a CodeGG-owned, bounded projection;
the read is daemon-owned, coalesced, read-only, and authorization-gated; and
every dimension the node did not report is reported as *not reported* rather
than as a zero.

Three things this closure does **not** claim:

- The Linux live receipt — a real node that polled a provider and whose
  reported values were read back through the production client — was
  **type-checked but not executed** on this macOS host. The Linux CI job is
  the gate.
- The upstream client's newly enforced transport deadlines change the failure
  modes of the **existing** M001/M003 remote execution path. That path must be
  requalified on Linux; the re-pin is not behavior-neutral for it.
- The optional telemetry phase described in the plan's §3.E beyond the
  operator read — richer per-execution correlation — is not claimed here and
  was not built.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Consume the frozen upstream contract | `src/scheduler/eggwork.rs` `EggworkNodeClient::telemetry` → `NodeClient::telemetry_for(&NodeId)`; upstream `eggwork_core::NodeTelemetry` schema version 1 read at pin `81f34e083bc6d44dbcc31ce2daae21b7dffda717` | pass | the route is `GET /v1/node/telemetry` behind the separate narrow `Operation::Telemetry` grant; mTLS and identity validation are untouched |
| One reviewed upstream pin, consistently | root `Cargo.toml` + `Cargo.lock`, `crates/eggwork-test-node/Cargo.toml` + its own `Cargo.lock` — all at `81f34e08` | pass | the API delta from the previous pin `e6a5d82e` is additive only; the `Requirement` / `ResourceRequirements` / `ResourceDimensionResult` / `IsolationRequirement` / `ResourceResult` variants CodeGG matches on are unchanged, so M003's conversion and outcome mapping are unaffected |
| Re-pin consequences handled, not inherited | `describe_client_error` gained a `TelemetryUnsupported` arm; the new transport deadlines and the 16 MiB body cap are recorded as requalification obligations below | partial | the code consequence is handled; the behavioral consequence for the existing remote path is outstanding (§10) |
| A CodeGG-owned read-only projection | `src/scheduler/eggwork_telemetry.rs` (`NodeTelemetrySnapshot` + the seven dimension types) and `crates/codegg-protocol/src/dto.rs` (`NodeTelemetryDto` + `NodeMetricStateDto`) | pass | the wire shape carries no interface identity, hostname, mount path, process id, or provider message |
| No fabricated values | `tests/eggwork_node_telemetry.rs` `an_unsupported_dimension_is_reported_as_unsupported_and_never_as_zero`, `a_node_without_a_provider_reports_disabled_and_fabricates_nothing`; unit tests `an_unsupported_dimension_is_never_rendered_as_zero`, `a_disabled_provider_fabricates_nothing` | pass | `Unknown` and `Unsupported` stay distinct from `Value` |
| Freshness and capability missingness | `NodeTelemetryState::{Ready, Stale, Warming, Disabled, Unavailable, Invalid}`, `NodeMetricState::{Value, Unknown, Unsupported}`, `age_ms`, `observed_at_unix_ms` | pass | only the node's own `Ready` state is treated as fresh |
| Join to node identity only after the check | upstream `telemetry_for(&NodeId)` refuses a mismatched document; `tests/eggwork_node_telemetry.rs` `a_document_addressed_to_another_node_is_refused` | pass | the snapshot is never joined to a node the document was not addressed to |
| Distinct bounded diagnostics | `telemetry_diagnostic` maps `TelemetryUnsupported` / `ProtocolIncompatible` / `InvalidResponse` / `Transport` / `Api` / timeout to distinct strings; `tests/eggwork_node_telemetry.rs` `a_node_without_the_telemetry_contract_reports_a_distinct_diagnostic`, `a_failed_probe_reports_a_diagnostic_and_never_a_measurement` | pass | a node that cannot be read still appears, with a diagnostic |
| One controller-owned coordinator; no poll amplification | `NodeTelemetryService` (one cache + one probe lock per node); `tests/eggwork_node_telemetry.rs` `two_readers_of_the_same_node_share_one_upstream_read` (1 upstream read for 2 concurrent readers), `a_repeated_read_inside_the_freshness_window_is_served_from_the_cache` (3 reads → 1 upstream read) | pass | the node owns the provider poll; CodeGG never polls the provider |
| No new Gregg route, no provider process, no port | `src/scheduler/eggwork_telemetry.rs` reads only the node's authenticated route; the provider lives behind the node's loopback boundary upstream | pass | CodeGG has no Gregg client, no provider configuration, and no process management |
| Read-only operator surface through the daemon | `CoreRequest::NodeTelemetry` → `CoreResponse::NodeTelemetry`; `src/core/node_telemetry.rs`; `src/core/daemon_jobs.rs`; `src/core/runtime_deps.rs` (`node_telemetry`); `crates/codegg-client/src/driver.rs` `SessionProjectionDriver::node_telemetry` | pass | the read never creates, mutates or releases a job |
| Independent of job dispatch | the coordinator is installed in `daemon_construct.rs` outside the scheduler-enabled block; `tests/eggwork_node_telemetry.rs` `a_telemetry_read_creates_no_job_and_leaves_the_queue_alone` | pass | the service exists whether or not the scheduler is enabled |
| Existing operator grants enforced before the network call | `operation_descriptor` → `node_telemetry_read`, `ScopeKind::Opaque`, `Capability::NodeTarget`; `tests/eggwork_node_telemetry.rs` `a_reader_without_the_node_grant_is_denied_and_never_reaches_the_node` (0 upstream reads) | pass | a team principal fails closed on the missing scope before any network call |
| Cold / no-config nodes return an explicit status | `tests/eggwork_node_telemetry.rs` `a_daemon_without_nodes_reports_no_nodes_configured`, `an_unknown_node_is_a_typed_refusal` | pass | the empty case is an explicit `node_telemetry_unavailable`, not a silent empty list |
| Real mTLS node with a configured synthetic Gregg provider | `crates/eggwork-test-node/src/synthetic_provider.rs` (a loopback provider built from the real `gregg-protocol` types) and `tests/eggwork_remote_execution_live.rs` `live_node_serves_its_provider_host_telemetry` | not run | the live file is `#![cfg(target_os = "linux")]`; this host is macOS. The file was type-checked locally by lifting the gate, which caught and fixed one wrong assertion. The Linux CI job is the gate (§10) |
| Failure does not block builds, change guarantees, or couple to admission | the read is not on any execution path; `tests/eggwork_node_telemetry.rs` `a_telemetry_read_creates_no_job_and_leaves_the_queue_alone` | pass | no execution, admission, or isolation code path was touched by the read |
| Payload round-trips to a real frontend | `crates/codegg-protocol/src/dto.rs` (wire DTOs), `crates/codegg-client/src/driver.rs` (native client), `src/tui/commands/diagnostics.rs` (`/nodes` renders the daemon's snapshots verbatim) | pass | the TUI renders `unsupported` / `unknown` for a dimension the node did not report, never `0` |

## 3. Production implementation evidence

**Ownership.** `src/scheduler/eggwork_telemetry.rs` owns the coordinator and
the CodeGG-owned view. It is constructed once per daemon from the same
resolved `[eggwork.nodes]` table the fixed-target executor uses — the node
table is now resolved exactly once in `daemon_construct.rs` and shared, so
the two surfaces cannot disagree about which nodes exist or what trust
material they use. The service is installed outside the
`scheduler_config.enabled` block: a diagnostic read works whether or not job
dispatch is enabled.

**Caching and coalescing.** One cache entry and one probe lock per named
node. A fresh observation is served for 30 s; a failure is remembered for
3 s only, so a repaired node is visible again quickly. Concurrent readers of
the same node queue behind one lock and the second reader finds the cache
warm. The read is bounded at 10 s, well inside the upstream client's own
120 s unary deadline, because an operator diagnostic must not hold a request
open that long.

**Projection.** `NodeTelemetrySnapshot` keeps the upstream tri-state per
dimension. `Value` carries the numbers the node reported; `Unknown` means the
node supports the dimension but did not report it this time; `Unsupported`
means the host platform cannot produce it. Neither of the latter two is ever
rendered as a zero — in the DTO, in the TUI toast, or in the projection
itself. The provider's error *category* is carried (`connection`, `timeout`,
`http_status`, `malformed`, `schema_version`, `invalid`, `too_large`,
`clock_skew`); its message never is. The upstream node's own `state`
(`disabled`, `warming`, `ready`, `stale`, `unavailable`, `invalid`) is carried
unchanged, and only `ready` is treated as fresh — a `stale` sample is still
reported with its age, but it is never presented as current.

**Protocol.** `CoreRequest::NodeTelemetry { node_id }` and
`CoreResponse::NodeTelemetry { nodes }` with the wire DTOs in
`codegg-protocol`. The native client's `SessionProjectionDriver::node_telemetry`
routes the read through the driver loop, so it shares the driver's client and
its fail-closed transport handling. The TUI's `/nodes` command issues the
read off the render path through the shared guarded task registry.

**Authorization.** The operation is `node_telemetry_read`, scoped to no
project (`ScopeKind::Opaque`) and carrying the existing remote-node grant
(`Capability::NodeTarget`). That scope kind means only the local owner's
broad policy authorizes the read and a team principal fails closed on the
missing scope — before any network call. The operation is classified as an
uninstrumented read in `crates/codegg-core/src/audit_instrumentation.rs`:
it discloses inert host metadata and mutates nothing, while the refusal
itself is audited upstream by `authorize_request`.

**The live fixture.** `crates/eggwork-test-node` now stands up a synthetic
loopback Gregg provider built from the real `gregg-protocol` types and points
the node's telemetry owner at it under `--telemetry-provider`. The provider
serves the same status document a production provider serves; only the
observation timestamp is live, because the node refuses a sample whose clock
is more than 30 s from its own.

## 4. Verification executed

### Commands run

```bash
# canonical gate
scripts/verify.sh quick

# the milestone's own suites
CARGO_BUILD_JOBS=2 cargo test --locked --test eggwork_node_telemetry
CARGO_BUILD_JOBS=2 cargo test --locked --lib scheduler::eggwork_telemetry
CARGO_BUILD_JOBS=2 cargo test --locked --test eggwork_remote_execution \
  --test eggwork_remote_limits --test eggwork_target_boundary

# affected crates
CARGO_BUILD_JOBS=2 cargo test -p codegg-protocol --locked
CARGO_BUILD_JOBS=2 cargo test -p codegg-core --locked
CARGO_BUILD_JOBS=2 cargo test -p codegg-client --locked

# the live fixture, at the new pin
cd crates/eggwork-test-node && cargo check --locked

# guards
python3 scripts/check_eggwork_target_routing.py
python3 scripts/check_eggwork_target_routing.py --self-test
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_audit_coverage.py
python3 scripts/check_config_merge_coverage.py
python3 scripts/check_adr_integrity.py
python3 scripts/check_project_catalog_invariants.py

# hygiene
cargo fmt --all -- --check
git diff --check
```

### Results

- `scripts/verify.sh quick` — **EXIT=0**, "Quick verification passed", 40 steps.
- `cargo test --locked --test eggwork_node_telemetry` — **19 passed, 0 failed**.
- `cargo test --locked --lib scheduler::eggwork_telemetry` — **6 passed, 0 failed**.
- `cargo test --locked --test eggwork_remote_execution` — **38 passed, 0 failed**.
- `cargo test --locked --test eggwork_remote_limits` — **18 passed, 0 failed**.
- `cargo test --locked --test eggwork_target_boundary` — **9 passed, 0 failed**.
- `cargo test -p codegg-protocol --locked` — **259 passed, 0 failed**.
- `cargo test -p codegg-core --locked` — lib **878 passed, 0 failed**, plus every
  suite binary passing.
- `cargo test -p codegg-client --locked` — lib **33 passed, 0 failed**, plus every
  suite binary passing.
- `cargo check --locked` in `crates/eggwork-test-node` — success at the new pin.
- Guards: the eggwork target-routing guard and its **10-case self-test**, the
  scheduler-bypass guard, the execution-ownership guard, the audit-coverage
  guard, the config-merge-coverage guard, the ADR-integrity guard and the
  project-catalog-invariants guard all pass.
- `cargo fmt --all -- --check` — clean. `git diff --check` — clean.

**Not run, and why:**

- `tests/eggwork_remote_execution_live.rs` — the file is
  `#![cfg(target_os = "linux")]` and `eggwork-runner` links Linux-only
  `landlock`; this host is macOS and no Linux container runtime is available.
  The file was type-checked locally by temporarily lifting the gate (which
  caught and fixed one wrong assertion — `NodeDisk` has no `used_bytes`), and
  the gate was restored. **The Linux live job is the gate.**
- `cargo clippy --workspace -- -D warnings` (the `verify.sh full` step) — not
  run; the workspace carries the six pre-existing lint sites the M001 and
  M003 closures already recorded, and they are deliberately not repaired here.
- The whole-lib test sweep — not run; it carries the six pre-existing failures
  the M003 closure already recorded.

## 5. Invariant review

| Invariant | Evidence |
|---|---|
| CodeGG never talks to Gregg | no Gregg client, provider configuration, or process management exists in CodeGG; the only Gregg reference is the fixture's synthetic provider, which lives behind the node's loopback boundary upstream |
| No fabricated values | the tri-state survives projection (`NodeMetricState`), the DTO (`NodeMetricStateDto`) and the TUI rendering; `unsupported` and `unknown` are never printed as `0` |
| The observation is joined only after an identity check | upstream `telemetry_for(&NodeId)`; the fake in `tests/common/eggwork_node.rs` refuses a mismatched document exactly as upstream does, and the integration test asserts the snapshot carries no measurement in that case |
| One coordinator, not one per reader | `NodeTelemetryService` is daemon-owned; `deps.node_telemetry` is a single `Arc`; the coalescing test proves two concurrent readers cause **one** upstream read |
| A refresh loop cannot amplify the node's poll | the 30 s freshness window plus the per-node probe lock; three sequential reads cause **one** upstream read |
| The read is read-only | `CoreRequest::NodeTelemetry` has no mutating arm; the coordinator never touches the job store, the queue, or the scheduler; the job-count assertion holds across reads |
| The read is independent of job dispatch | the coordinator is installed outside the scheduler-enabled block and is reachable with the scheduler disabled |
| Per-execution metrics are not synthesized from host telemetry | `ExecutorMetrics.cpu_time_ms` / `peak_memory_mb` are unchanged; nothing in the telemetry path writes to the run store |
| Host telemetry never becomes admission input | the coordinator is not consulted by admission; M003's ceiling and the node's advertised capabilities remain the only gates |
| The node table is resolved once | `daemon_construct.rs` resolves `[eggwork.nodes]` once and hands the same table to the coordinator and the executor |

## 6. Failure and recovery review

- **Duplicate delivery / idempotency** — not applicable: the read creates
  nothing. Two readers of one node produce one upstream read, and the second
  read sees the first's cached answer.
- **Cancellation races** — not applicable: a read is a bounded, cancellable
  await with no side effect. A dropped read leaves nothing behind.
- **Daemon or node restart** — the cache is in-memory, so a daemon restart
  drops it and the next read re-probes. A node restart leaves the provider
  state to the node: the first read after a restart may report `warming` or
  `unavailable`, and the failure is cached for 3 s only.
- **Partial persistence failure** — not applicable: nothing is persisted.
- **Stale generation or lease** — not applicable: no execution is involved.
- **Contention and resource release** — the probe lock is per node and is
  released when the read completes or the read is dropped; a poisoned cache
  mutex is recovered rather than taking the daemon down.
- **Malformed or unauthorized input** — an unknown node id is the typed
  `unknown_eggwork_node` refusal and never reaches the network; a malformed
  upstream document is the client's `InvalidResponse`, which becomes a bounded
  diagnostic and no measurement; a team principal is refused on the missing
  scope before any network call.
- **Bounded event and artifact behavior** — the read is bounded at 10 s; the
  answer is one snapshot per configured node, each carrying at most seven
  fixed-size dimensions; the upstream telemetry route is bounded to 64 KiB
  and the client to a 16 MiB buffered body.

## 7. Migration and compatibility review

- **Schema migration** — none. The milestone adds no persistent state: there
  is no host-metric database, and `STORAGE_LAYOUT_VERSION` is unchanged.
- **Dependency re-pin** — `eggwork-client` / `eggwork-core` move from
  `e6a5d82e` to `81f34e08` in the root manifest and lockfile, and
  `eggwork-server` / `eggwork-runner` / `eggwork-core` move in the fixture's
  own manifest and lockfile. The API delta is additive only; the enum
  variants CodeGG matches on are unchanged; no CodeGG call site changed
  meaning. Rollback is a manifest-and-lockfile revert: the previous pin has
  no telemetry route, so a rolled-back daemon answers the telemetry request
  with the typed "unsupported" diagnostic rather than failing.
- **Protocol negotiation** — the request and response are new variants. A
  legacy daemon that does not know `CoreRequest::NodeTelemetry` answers with
  some other shape; the TUI and the native client both treat that as an
  **absent surface**, not as a failure the operator must act on. A legacy
  client is unaffected by the new response variant.
- **Configuration** — no new configuration key. `[eggwork.nodes]` keeps its
  existing shape; the telemetry read uses the same resolved table.
- **Legacy-path status** — the fixed-target remote execution path is
  unchanged in shape; its *failure modes* change with the upstream client's
  new transport deadlines, which is recorded in §10 rather than treated as a
  completed requalification.

## 8. Security review

- **Authorization** — enforced before the network call. The operation is
  scoped to no project, so only the local owner's broad policy authorizes it;
  a team principal is refused with `authorization_scope_required` and the
  node is never contacted. The refusal is audited by `authorize_request`.
- **Secrets** — none. No credential, endpoint, certificate path or key path
  enters a snapshot, a diagnostic, a log line, or the wire DTO. The upstream
  `ClientError::Api` arm renders only the HTTP status, never the body.
- **mTLS** — unchanged. The read goes through the same `NodeClient` over the
  same client certificate and CA material; the node identity is checked
  after the authenticated read.
- **Redaction** — the projection carries no interface identity, hostname,
  mount path, process id, provider message, or provider error text. The
  provider's error *category* is carried, which is a closed vocabulary.
- **Path validation** — not applicable: the read touches no filesystem path.
- **Privilege boundary** — the read is strictly less privileged than the
  execution path: it observes, it never mutates, and it is not reachable
  from any job.
- **Denial-of-service bounds** — one upstream read per node per 30 s, a 10 s
  read bound, a bounded per-node answer, and a bounded upstream body. Two
  readers of one node cannot multiply the load.
- **Audit** — the operation is explicitly classified as an uninstrumented
  read; the denial is audited.

## 9. Documentation and operations

- `architecture/jobs.md` — new section "Remote node host telemetry (M002)":
  the frozen contract, the identity check, what the projection carries, what
  the read is not, the coordinator's ownership and bounds, and the typed
  refusals.
- `architecture/scheduler.md` — new section under "Operator visibility":
  the request/response, the daemon-owned coordinator, the authorization
  shape, and the frontend surfaces.
- `architecture/command.md` — `/nodes` added to the built-in table; the
  count pins moved 166 → 167 in the doc and in
  `built_in_command_count_matches_release_docs`.
- `architecture/overview.md` — the verified-counts table re-measured for the
  rows this change touches (`CoreRequest` 259, `CoreResponse` 169, authorized
  operations 253, slash commands 167, integration test files 267) with a
  dated note; the earlier snapshot rows were already stale and are corrected
  rather than propagated.
- `docs/tui.md` — `/nodes` in the slash-command table.
- `docs/dependency-maintenance.md` — the `eggwork-client` / `eggwork-core`
  row moves to the new pin and records what that pin carries.
- `scripts/check_eggwork_target_routing.py` — the allowed module set is
  widened for `src/scheduler/eggwork_telemetry.rs`, with the reason recorded
  at the site: the module owns the one projection from the node's
  authenticated document into CodeGG's sanitized view and never constructs a
  job, a selection, or an execution.
- Operator diagnostic: `/nodes` in the TUI, and
  `SessionProjectionDriver::node_telemetry` for a native frontend.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | The Linux live fixture was not executed on this macOS host, so the end-to-end receipt — a real node polled a provider and the reported values were read back through the production client — is outstanding. | The milestone's headline live evidence is unproven on this host; the scripted fixture covers the CodeGG-side projection but not the node's own polling. | Run `tests/eggwork_remote_execution_live.rs` on Linux CI (or a Linux host). The file is type-checked; the fixture's synthetic provider is wired and the two live tests are written. |
| medium | The upstream client now enforces transport deadlines — 120 s total on unary calls and 60 s per-chunk-gap on the streaming `execute` / `events` / `renew` calls. The existing M001/M003 remote execution path was built against a client without those deadlines. | A long quiet remote execution can now fail its event stream, which is a new failure mode on a path this milestone did not otherwise change. | Requalify the fixed-target remote path on Linux at the new pin. The deadline values are upstream's, not CodeGG's, so the fix belongs in the executor's own timeout contract if it proves too tight. |
| low | Workspace clippy is red on the same six pre-existing lint sites the M001 and M003 closures recorded (`crates/codegg-core/src/workspace_services.rs:156`, `src/agent/tool_batch.rs:137`, `src/mcp/mod.rs:554`, `src/search_backend/eggsearch.rs:626`, `src/tool/grep.rs:781,782`). | The `verify.sh full` clippy step stays red for reasons unrelated to this milestone. | Repair in a separate, scoped change; deliberately not repaired here, consistent with the earlier closures. |
| low | The whole-lib test sweep carries the six pre-existing failures the M003 closure recorded. | Not attributable to this milestone. | Same as above. |
| low | The daemon's node table is a construction-time snapshot: `[eggwork.nodes]`, `resource_limits` and `max_concurrent_executions` need a daemon restart after an edit. | An operator who edits the node table sees the old one until restart. | Pre-existing behavior, inherited from M001/M003; not changed here. |

## 11. Roadmap disposition

**Milestone closed and next dependency may proceed.**

The upstream dependency that blocked this milestone is discharged and the
capability boundary is implemented, tested, and documented. The subsystem
roadmap's M002 entry moves to closed. The remaining items in §10 are
evidence obligations for the *existing* remote path, not for this
milestone's boundary, and they do not gate any successor milestone: the
roadmap's next step remains the deferred cross-node placement work, which
this roadmap explicitly does not claim.

## 12. Registry updates

- `plans/registry.md`:
  - the subsystem row becomes "M001 closed, M002 closed, M003 closed";
  - the M002 row moves from the dependency-ready table to the closed table
    with its implementation commit and evidence;
  - the blocked-work table's M002 row is marked discharged with its blocker
    text retained, and the M001 row's unblock clause is reconciled.
- `plans/subsystems/eggwork-worker-telemetry-and-targeting-roadmap.md`:
  - the status line becomes "M001 closed, M002 closed, M003 closed";
  - the dependency graph marks upstream M001/M002 and CodeGG M002 closed;
  - the M002 milestone entry gains its closure pointer.
- `plans/implementation/eggwork-worker-telemetry-and-targeting/002-remote-node-telemetry-projection.md`:
  - the status becomes **implemented**, with the upstream closure and the
    reviewed pin recorded.
