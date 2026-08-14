# Post-Audit Correctness, Simplification, and Footprint — Daemon Lifecycle Corrective Addendum

Status: active

Source roadmap:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`

Historical source milestone and closure:

- `plans/implementation/post-audit-correctness-simplification/002-daemon-stop-identity-and-cli-json-correctness.md`
- `plans/closure/post-audit-correctness-simplification/002-status.md`

Prior corrective closure remains historical and closed:

- `plans/subsystems/post-audit-correctness-simplification-corrective-closure-addendum.md`
- C001/C002 remain accepted and MUST NOT be rewritten to conceal their closure history.

Current corrective control point:

- C003 — daemon startup, shutdown, transport, and process-lifecycle correctness
- implementation plan: `plans/implementation/post-audit-correctness-simplification/012-daemon-startup-shutdown-and-process-lifecycle-corrective-pass.md`
- target closure: `plans/closure/post-audit-correctness-simplification/012-status.md`

Repository baseline reviewed: `2249c0bdd58481316c53207ee5297a3319c0a34f`

## 1. Why this addendum exists

The earlier M002/C001/C002 work correctly hardened daemon-stop identity and unrelated post-audit defects, but later direct inspection of the ordinary `codegg` startup path found a separate class of lifecycle failures that M002 explicitly did not own. These failures are not evidence that the prior daemon-stop identity correction was invalid; they are new evidence that the singleton daemon's startup, connection, and shutdown mechanics were never covered end-to-end as one production lifecycle contract.

The user-visible symptom is severe: a freshly invoked `codegg` can fail to reach terminal initialization, so the TUI appears not to launch at all.

The highest-confidence root cause is in `src/core/instance.rs::connect_or_start_daemon`: the helper spawns `codegg daemon start` with `kill_on_drop(true)`, waits until the child is reachable, returns a socket client, and then drops the only `Child` handle. Tokio therefore kills the daemon that the helper has just started. The TUI then continues startup against a dying socket connection.

That defect combines with other lifecycle gaps:

- `SocketCoreClient` can leave request futures pending forever when its reader observes EOF because outstanding pending senders are not failed/drained;
- the connect-or-start readiness probe performs unbounded `SnapshotDaemon` requests outside the nominal startup deadline;
- a concurrently launched second autostart child may exit successfully because another process won the singleton lock, but its parent treats any child exit as fatal instead of converging on the winner daemon;
- the autostart child is described as detached but is not deliberately separated from the initiating terminal/session and has no explicit reaper after successful startup;
- production `run_daemon` currently opens `storage::init_legacy_project_store`, even though that API explicitly states that new production daemon code MUST use the user-scoped daemon catalog; the legacy initializer also does not run schema migration;
- `daemon stop` sends SIGTERM, while `run_daemon` installs only `tokio::signal::ctrl_c()` and therefore does not execute its documented graceful cancellation/cleanup path on SIGTERM;
- `DaemonConfig::shutdown_timeout_ms` is present but no production lifecycle code consumes it, while the socket server waits without a bound for all connection tasks to join during shutdown;
- explicit TUI socket endpoints are parsed into `Cli::core_endpoint`, but `launch_tui` constructs `DaemonPaths::resolve()` directly and does not apply the override; `AttachDaemon` therefore does not reliably attach to the requested socket;
- `src/main.rs::default_socket_path` duplicates and disagrees with `core::instance::DaemonPaths` Linux fallback semantics;
- `DaemonPaths::log_path` is not the default for `daemon logs`, and autostart discards daemon stdout/stderr, making startup failures needlessly opaque.

The process-management audit also found one adjacent confirmed defect in `src/mcp/local.rs`: local MCP children are spawned with `stderr(Stdio::piped())`, but the pipe is not taken or drained. A sufficiently noisy MCP server can fill the pipe, block the child, and surface as request timeouts. This is small enough and close enough to daemon-owned child-process reliability to include in C003.

## 2. C003 ownership boundary

C003 owns only the concrete lifecycle mechanics required to make the existing single-daemon architecture operational and bounded:

- correct production daemon catalog bootstrap and migration;
- daemon autostart child ownership, detachment, race convergence, failure cleanup, and reaping;
- verified/bounded local socket handshake and startup readiness;
- connection-loss propagation to pending requests;
- coherent reconnect behavior or explicit fail-fast replacement of the current partial reconnect path;
- canonical endpoint override/path resolution for ordinary TUI, `AttachDaemon`, daemon commands, and environment overrides;
- SIGINT/SIGTERM graceful shutdown, bounded connection draining, and owned socket/PID/metadata cleanup;
- daemon startup diagnostics/log-path coherence;
- bounded draining of local MCP child stderr.

C003 does not reopen scheduler ownership, durable scheduling, provider architecture, session projections, agent-loop ownership, sandbox authority, release policy, CI topology, or executable topology.

## 3. Required invariants

- Exactly one user-scoped production daemon remains authoritative; C003 fixes lifecycle mechanics without weakening the singleton lock.
- Plain `codegg` MUST either connect to a verified live daemon or start one that remains alive independently of the initiating frontend.
- A frontend MUST NOT own the production daemon lifetime merely because it happened to autostart it.
- A failed/timed-out autostart MUST NOT leave an unintended unverified daemon child behind.
- Concurrent first-client startup MUST converge on the single lock winner rather than making a losing frontend fail solely because its own child exited after observing an already-running daemon.
- Production daemon storage MUST be the migrated user-scoped daemon catalog. Legacy project stores remain compatibility/import sources, not production daemon authority.
- Socket connection success alone is not readiness. Readiness requires a successful CodeGG handshake and bounded live-daemon identity/status proof.
- Connection EOF/error MUST release every waiter associated with that connection.
- Transparent reconnect MUST never operate with stale daemon/client identity or missing protocol subscriptions. If complete reconnect cannot be guaranteed narrowly, fail the connection and let the higher-level frontend perform a fresh attach/resync.
- Explicit endpoint overrides MUST select the same endpoint for connection, lock/metadata diagnostics, status, stop, and attach operations where applicable.
- SIGTERM and SIGINT MUST enter the same owned graceful-shutdown path on supported Unix platforms.
- Graceful shutdown MUST have a finite bound; the configured shutdown timeout must have observable semantics rather than being dead configuration.
- Socket, PID, and metadata cleanup MUST only remove artifacts owned by the exiting daemon generation.
- Local child processes with piped output MUST continuously drain every piped stream or inherit/null it intentionally.
- No new service manager, supervisor daemon, systemd dependency, launchd dependency, CI lane, release automation, or binary split is introduced.

## 4. Audit disposition

### Critical / high findings owned by C003

1. Autostart daemon killed by `kill_on_drop(true)` when `connect_or_start_daemon` returns.
2. Production daemon uses the legacy project-local store instead of the migrated user-scoped daemon catalog.
3. Socket request waiters can hang after peer death; startup readiness probes are not bounded by the startup deadline.
4. SIGTERM bypasses graceful cancellation even though `daemon stop` uses SIGTERM.
5. Concurrent autostart can incorrectly fail the losing frontend when another daemon wins the singleton race.
6. Explicit socket endpoint overrides are not honored by the ordinary TUI launch path.
7. Current reconnect logic does not re-establish the complete negotiated connection state.
8. Shutdown connection draining has no finite bound despite a configured shutdown timeout.

### Medium findings owned by C003

9. Autostart discards daemon diagnostics and `daemon logs` defaults to a different relative path instead of the user-scoped daemon log.
10. Local MCP stderr is piped but not drained, allowing a child-process pipe deadlock.

### Audited but not automatically widened into C003

- `StdioCoreClient` is an explicit compatibility/testing transport. It drops the `Child` handle after extracting stdin/stdout, has no complete request transaction gate, and has no transport-level response timeout. The current code should receive focused tests during C003 inspection. If a reproducible correctness failure requires more than a small local correction, register a separate stdio-transport follow-up rather than broadening production-daemon work.
- `egglsp` already has explicit child ownership, transport failure propagation, and a stderr-drain path. No C003 blocker was identified from the inspected launch/client surface. Do not refactor LSP merely for symmetry.
- `managed_process` intentionally uses process-session/group cleanup, bounded stdout/stderr draining, cancellation, timeout handling, and `kill_on_drop` for finite scheduler-owned jobs. Its `kill_on_drop` semantics are correct for that ownership model and MUST NOT be mechanically changed because the daemon autostart use is wrong.

## 5. Dependency and execution order

C003 has no hard dependency on another active subsystem. It is dependency-ready and should be implemented before treating the ordinary CLI/TUI runtime as operational.

Recommended order:

1. add failing lifecycle regressions against the current baseline;
2. correct daemon catalog bootstrap/migration;
3. correct autostart ownership, race convergence, detachment/reaping, and failure cleanup;
4. make socket handshake/readiness and disconnect propagation bounded and explicit;
5. centralize endpoint resolution and remove duplicate fallback logic;
6. implement SIGTERM/SIGINT graceful shutdown plus bounded drain/cleanup;
7. wire daemon log diagnostics coherently;
8. drain local MCP stderr and add a noisy-child regression;
9. run focused lifecycle tests and the existing quick verification contract;
10. write `plans/closure/post-audit-correctness-simplification/012-status.md` only after the ordinary binary entrypoint is demonstrated working through the production daemon path.

## 6. Verification posture

C003 is a runtime correctness pass, not a new verification program.

Required evidence is focused and mechanism-faithful:

- real multi-process singleton/autostart integration tests using private runtime and data roots;
- socket peer-death/handshake/reconnect tests;
- SIGTERM cleanup/restart test on supported Unix;
- endpoint override test;
- fresh empty user-scoped catalog startup test;
- noisy local-MCP stderr test;
- one real ordinary `codegg` startup smoke test that proves the process reaches TUI terminal initialization or a test seam immediately before the event loop without requiring a manually prestarted daemon;
- `cargo fmt --all -- --check`;
- targeted tests;
- `scripts/verify.sh quick`.

Do not add CI matrices, daemon service installation tests, systemd/launchd integration, coverage gates, scheduled lifecycle jobs, binary-size gates, or release automation.

## 7. Exit conditions

C003 may return this workstream to `closed` only when:

- plain `codegg` no longer kills its autostarted daemon;
- the autostarted daemon remains independently reachable after the helper returns;
- concurrent first-client startup converges on one live daemon;
- fresh production daemon startup uses a migrated user-scoped catalog and does not create/use a project-local legacy DB as daemon authority;
- startup cannot block forever on a dead or non-responsive socket peer;
- peer death releases pending socket request futures;
- reconnect semantics are complete or deliberately fail-fast with a higher-level fresh reconnect path;
- explicit endpoint overrides are honored consistently;
- SIGTERM executes graceful cancellation and owned artifact cleanup;
- connection draining is bounded by the configured shutdown timeout;
- `daemon logs` points to useful daemon startup/runtime diagnostics;
- local MCP stderr cannot block the child because of an undrained pipe;
- no critical/high unresolved lifecycle finding remains in C003 scope;
- focused tests and `scripts/verify.sh quick` pass;
- closure evidence records exact implementation commits and any intentionally deferred low/medium findings.

Until those conditions are met, the prior post-audit closure remains historical evidence but the current subsystem disposition is `active` through C003.
