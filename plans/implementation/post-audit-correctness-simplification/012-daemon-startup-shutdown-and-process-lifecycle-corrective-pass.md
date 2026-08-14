# Post-Audit Correctness, Simplification, and Footprint C003 — Daemon Startup, Shutdown, and Process-Lifecycle Corrective Pass

Status: ready

Source subsystem roadmap and corrective control surface:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`
- `plans/subsystems/post-audit-correctness-simplification-daemon-lifecycle-corrective-addendum.md`
- corrective control point C003

Historical related milestone/closure:

- `plans/implementation/post-audit-correctness-simplification/002-daemon-stop-identity-and-cli-json-correctness.md`
- `plans/closure/post-audit-correctness-simplification/002-status.md`

Repository baseline reviewed: `2249c0bdd58481316c53207ee5297a3319c0a34f`

Primary class: lifecycle correctness / production startup blocker

Dependencies:

- hard: none
- interface: existing singleton lock/metadata contract and existing core protocol
- soft: none
- operational: Unix signal/process-group behavior must be exercised on a supported local/hosted Unix environment

Target closure record:

- `plans/closure/post-audit-correctness-simplification/012-status.md`

## 1. Objective

Restore the ordinary `codegg` binary as a reliably runnable production entrypoint by correcting the existing singleton-daemon lifecycle rather than bypassing it.

A plain `codegg` invocation must be able to start from a fresh user state, create or reuse exactly one user-scoped daemon, establish a verified bounded socket connection, initialize the remote-core TUI path, and leave the daemon alive independently of the initiating frontend. `codegg daemon stop` must then terminate that daemon through the documented graceful path and clean up owned runtime artifacts within a finite bound.

This is a corrective implementation pass. Preserve the accepted single-daemon architecture. Do not "fix" the symptom by changing the default back to in-process mode, silently forcing `--standalone`, or splitting the executable topology.

## 2. Explicit non-goals

Do not:

- change the invariant that the production TUI is a client of one user-scoped daemon;
- make standalone/in-process mode the production default;
- add systemd, launchd, Windows Service, supervisor-daemon, container, or external service-manager dependencies;
- split `codegg` and the daemon into separately packaged binaries;
- redesign scheduler ownership, durable jobs, project/session projection protocols, agent execution, provider routing, or sandbox authority;
- add a second persistence authority for sessions/jobs/workspaces;
- add a new daemon shutdown protocol merely because signal handling is currently incomplete;
- add a blanket short timeout to every core request without first determining whether existing request types legitimately run longer;
- implement speculative automatic restart loops;
- add CI lanes, matrices, scheduled lifecycle checks, release automation, or service-installation tests;
- broadly refactor MCP, LSP, shell execution, or managed-process code that is not implicated by a reproduced lifecycle defect.

## 3. Current implementation evidence and defect inventory

Inspect and preserve current repository reality before editing. At minimum read:

- `src/main.rs` — CLI transport selection, `launch_tui`, `AttachDaemon`, `run_daemon`, stop/status/log handling, and `run_core_stdio`;
- `src/core/instance.rs` — runtime paths, singleton lock/metadata, and `connect_or_start_daemon`;
- `src/core/transport/socket.rs` — handshake, pending requests, reader termination, and reconnect;
- `src/core/transport/daemon_socket.rs` — listener shutdown and connection-task lifecycle;
- `crates/codegg-core/src/storage/mod.rs` and `crates/codegg-core/src/storage/paths.rs` — daemon catalog versus legacy project store;
- `crates/codegg-config/src/schema.rs` — daemon startup/shutdown configuration;
- `tests/single_daemon_lifecycle.rs` and relevant transport integration tests;
- `src/mcp/local.rs` — local child stdio lifecycle;
- `src/core/transport/stdio.rs` — adjacent compatibility transport audit only unless a small directly reproduced defect is addressed.

### 3.1 Critical — autostart kills the daemon it just started

`connect_or_start_daemon` currently calls:

```rust
Command::new(current_exe)
    .args(["daemon", "start"])
    ...
    .kill_on_drop(true)
    .spawn()
```

The helper keeps the `Child` only until a socket client becomes ready. On successful return the local child handle is dropped, so `kill_on_drop(true)` terminates the production daemon. This directly contradicts the ownership model: the frontend that happens to create the singleton daemon must not own its lifetime.

### 3.2 Critical/high — production daemon bootstraps the wrong database

`run_daemon` currently calls `storage::init_legacy_project_store(Path::new(&project_dir))`.

`crates/codegg-core/src/storage/mod.rs` explicitly documents that this is a workspace-local compatibility/import store and that new production code MUST NOT use it; production daemons use `init_daemon_catalog`. `init_legacy_project_store` also only opens/configures the pool and does not invoke schema migration.

Consequences include:

- daemon authority becomes dependent on whichever project directory happened to launch the first daemon;
- a fresh legacy database can be structurally empty when daemon components expect current tables;
- multi-project singleton semantics conflict with project-local storage placement;
- runtime behavior diverges from architecture/storage documentation and from `run_core_stdio`, which already demonstrates a migrated daemon-catalog bootstrap sequence.

### 3.3 High — pending socket requests can hang after daemon death

`SocketCoreClient::request` inserts a oneshot sender in `pending` and waits on its receiver. The background reader exits on EOF/read error but does not drain/fail the pending map. A request written immediately before daemon death can therefore wait indefinitely even though the connection reader is gone.

This is particularly damaging during TUI startup because terminal initialization occurs only after remote-core/bootstrap work.

### 3.4 High — startup readiness is not actually bounded

`ConnectOrStartOptions::startup_timeout` bounds the polling loop, but both the existing-daemon path and newly-connected child path perform `client.request(SnapshotDaemon).await` without a deadline. A listening but non-responsive/foreign/stuck socket can therefore bypass the nominal startup timeout and block indefinitely.

Readiness must mean a successful CodeGG handshake plus a bounded live-daemon proof, not merely successful `UnixStream::connect`.

### 3.5 High — concurrent autostart race can incorrectly fail one client

Two first clients may both observe no socket and each spawn `daemon start`. The singleton lock correctly permits only one daemon. The losing `daemon start` process may exit successfully after observing the winner daemon, but its parent `connect_or_start_daemon` currently treats any child exit as `ChildExited` and fails instead of performing a final verified connection to the winner.

The lock already provides the right authority. The frontend helper must converge on that authority.

### 3.6 High — the "detached" daemon is not deliberately detached/reaped

Removing `kill_on_drop(true)` alone is necessary but incomplete. The autostart path must define both sides of process ownership:

- the daemon must not remain coupled to the initiating frontend's terminal/process group in a way that makes frontend termination terminate the singleton unexpectedly;
- while the parent frontend remains alive, an exited daemon child should be reaped rather than accumulating as a zombie.

Use the smallest supported-Unix mechanism that establishes the intended session/process-group boundary. Keep a background reaper task after readiness if useful; dropping that task/handle when the frontend exits must not kill the daemon.

### 3.7 High — SIGTERM is not wired to graceful shutdown

`daemon stop` sends SIGTERM after live identity verification. `run_daemon` creates a cancellation token but only waits on `tokio::signal::ctrl_c()`. On Unix, SIGTERM therefore follows the default abrupt termination path, skipping the explicit socket/PID/metadata cleanup code and contradicting comments/docs that describe SIGINT/SIGTERM graceful shutdown.

SIGINT and SIGTERM must trigger the same owned cancellation path.

### 3.8 High — shutdown drain is unbounded and configured timeout is unused

After cancellation, `run_core_socket_with_listener` waits for every client task in its `JoinSet` without a deadline. A connection task that is awaiting a long/non-cancellable core request can indefinitely delay graceful daemon termination. `DaemonConfig::shutdown_timeout_ms` exists but current code search shows no production consumer.

Give that configuration a narrow, explicit meaning: the maximum graceful connection-drain interval after daemon shutdown begins. At expiry, connection-owned tasks may be aborted/closed as appropriate, after which owned runtime artifacts must still be cleaned up. Do not silently turn this into a global agent/job timeout.

### 3.9 High/medium — endpoint overrides are fragmented and partly ignored

`AttachDaemon` resolves an endpoint and stores it in `cli_copy.core_endpoint`, but `launch_tui` later constructs `core::instance::DaemonPaths::resolve()` directly and does not apply `cli.core_endpoint`. The same issue affects legacy explicit socket transport use.

`src/main.rs::default_socket_path()` also duplicates runtime path resolution and disagrees with `core::instance::DaemonPaths` on Linux when `XDG_RUNTIME_DIR` is absent (`/tmp/codegg` versus XDG data/home fallbacks).

There must be one endpoint/path resolution authority. Explicit CLI/env endpoint selection must be normalized once and applied to the `DaemonPaths` used by connect, status, stop, and attach semantics as appropriate.

### 3.10 Medium — daemon diagnostics are disconnected from lifecycle paths

The runtime `DaemonPaths` type has `log_path`, but:

- autostart sends stdout/stderr to null;
- `daemon logs` defaults to relative `codegg_debug.log`;
- normal daemon tracing goes to stderr unless debug-file conditions happen to be enabled.

A failed background startup can therefore disappear without useful diagnostics. Reuse the user-scoped daemon log path instead of inventing another logging subsystem.

### 3.11 Medium — local MCP stderr pipe is never drained

`LocalClient::initialize` configures stdin/stdout/stderr as piped but only takes stdin/stdout. A child that writes enough stderr can block on the full pipe and make otherwise valid MCP requests time out.

This is a classic child-process lifecycle defect. Add a bounded background drain or intentionally redirect stderr; do not retain an unread pipe.

### 3.12 Adjacent findings requiring disposition, not automatic scope expansion

`StdioCoreClient` currently extracts stdin/stdout and drops the `Child` handle; complete write+read request transactions are not serialized by one gate, and reads are unbounded. Because `--stdio` is an explicit compatibility/testing mode, inspect and add a focused regression if practical. If fixing it requires a separate transport lifecycle design, record a follow-up rather than making C003 depend on it.

The inspected `egglsp` launch/client path already has explicit child ownership, background failure propagation, and stderr-drain support. The canonical `managed_process` path intentionally uses `kill_on_drop(true)` for finite scheduler-owned jobs and implements process-session/group termination plus bounded output draining. Do not mechanically alter those correct ownership models.

## 4. Invariants that cannot regress

- The singleton advisory lock remains authoritative for production daemon ownership.
- Metadata remains diagnostic; it cannot override live lock/protocol identity.
- Plain `codegg` remains daemon-client mode by default.
- The daemon survives the lifecycle of the frontend that autostarted it unless explicitly stopped or it fails independently.
- At most one production daemon owns the user scope after concurrent startup.
- Production persistent authority uses the user-scoped daemon catalog.
- Project-local legacy stores remain import/compatibility sources only.
- Startup and shutdown operations have finite, documented bounds.
- A socket must prove it is a compatible CodeGG daemon before it is treated as ready.
- Peer death closes/fails all request waiters associated with that connection.
- Reconnection cannot silently retain stale daemon/client identity or lose required subscription state.
- Explicit endpoint overrides work or fail clearly; they are never parsed and then ignored.
- Shutdown removes only paths owned by the exiting daemon generation.
- `daemon stop` continues to verify live daemon identity before sending a signal.
- Local MCP child stderr cannot block the child.
- Existing supported features, protocol versions, scheduler authority, manual release policy, and single-binary topology remain unchanged.

## 5. Preferred target design

### 5.1 One production daemon catalog bootstrap

Extract or reuse a small helper that opens a production daemon catalog correctly:

1. resolve `storage::DaemonPaths` for the user data scope;
2. open the catalog with the migration-safe single-connection path;
3. run `session::schema::migrate`;
4. close the migration pool;
5. reopen via `init_daemon_catalog` for normal runtime use.

`run_core_stdio` already contains this sequence and should not remain a separate hand-written copy if a small storage helper can make the production invariant obvious.

The real daemon integration tests must isolate both runtime socket/lock paths and catalog data paths. Do not allow a test to touch the developer's real catalog merely because `CODEGG_DAEMON_HOME` only scopes runtime artifacts.

### 5.2 Verified connect-or-start state machine

Treat `connect_or_start_daemon` as a finite state machine rather than a connect/spawn convenience wrapper.

Preferred semantics:

1. resolve/ensure the canonical runtime paths;
2. attempt a **verified** bounded connection to the target endpoint;
3. verified connection means ClientHello write succeeds, ServerHello is received/validated, and a bounded live daemon status/identity probe succeeds;
4. if unavailable and autostart is disabled, return a classified error;
5. if autostart is enabled, spawn `codegg daemon start` without `kill_on_drop(true)`;
6. detach the daemon from the initiating terminal/process session using the smallest supported Unix mechanism appropriate for the current platform contract;
7. direct startup diagnostics to the user-scoped daemon log;
8. poll verified readiness until the configured deadline while observing child exit;
9. if the child exits, perform a final verified connection before classifying failure so concurrent autostart can converge on another lock winner;
10. on success, move a still-owned child handle into a background reaper task and return the live client; the reaper MUST NOT kill the daemon on drop;
11. on timeout/failure, terminate and reap only the child this helper spawned if it is still alive, then return an actionable error including the daemon log path.

Do not use an arbitrary sleep as lifetime management.

### 5.3 Socket connection lifecycle

Prefer an explicit handshake helper used by both initial connect and any reconnect:

- every newly opened Unix stream sends ClientHello and checks write/flush errors;
- reset negotiated `client_id`/`daemon_id` state before a reconnect;
- wait for ServerHello under a finite handshake timeout;
- validate protocol compatibility;
- do not report connect success before handshake completion;
- when the reader exits for EOF/error, remove/drop all pending request senders so every waiter resolves with an error;
- do not reuse a stale pending request across a replacement connection;
- if transparent reconnect is preserved, re-establish every state required by the protocol (hello plus subscriptions) before retrying traffic;
- if complete transparent reconnect is too broad, remove the partial retry and return a connection error so the TUI/projection layer can perform its existing fresh reconnect/resync path.

Do not add one globally short timeout to all core requests without request-semantics review. Add a bounded control-plane request mechanism for startup/status/health probes and, if necessary, bounded TUI startup hydration calls.

### 5.4 Canonical endpoint resolution

Remove `src/main.rs::default_socket_path` as an independent source of truth.

Use `core::instance::DaemonPaths` plus a small normalization helper that accepts either a plain filesystem path or `unix://...` URI and returns one canonical socket path/endpoint representation.

Required precedence should remain explicit and testable:

1. command-line endpoint argument;
2. `CODEGG_CORE_ENDPOINT` environment override;
3. canonical `DaemonPaths::resolve()` default.

`launch_tui`, `AttachDaemon`, daemon start/stop/status, and any compatibility `--core-transport socket` path must use the same normalization. Do not create a second lock root merely because a custom socket path is used; retain the documented singleton lock semantics unless the current architecture explicitly says otherwise.

### 5.5 Graceful shutdown

On Unix, wait for either SIGINT or SIGTERM and cancel one daemon shutdown token.

After cancellation:

1. stop accepting new socket connections;
2. notify connection handlers;
3. allow connection-owned cleanup for at most `shutdown_timeout_ms`;
4. after the deadline, abort/close remaining connection tasks without converting the setting into an agent/job execution timeout;
5. remove the owned socket and legacy PID file;
6. drop/release daemon metadata/lock ownership;
7. ensure a subsequent daemon can start immediately without stale-state manual cleanup.

`daemon stop` may wait for bounded observable shutdown completion (socket/metadata disappearance) and report a timeout if the verified daemon does not exit. Do not blindly SIGKILL an unverified/reused PID.

### 5.6 Daemon diagnostics

Use `core::instance::DaemonPaths::log_path` as the default daemon log location.

For autostart, open/append the log with user-only permissions and direct child stdout/stderr there. `daemon logs` should resolve the same path when `--file` is absent.

Keep foreground `daemon start` usable interactively; do not force all manual daemon output away from the terminal unless an explicit daemon-background marker is used.

### 5.7 Local MCP child stderr

Take the piped stderr handle and spawn a continuously draining task. Retain at most a small bounded diagnostic tail or emit bounded tracing; after the retained cap is reached, continue draining without unbounded memory growth.

On shutdown/child exit, ensure the stderr task terminates naturally or is cancelled. Add a fake MCP fixture that writes more than a typical pipe capacity to stderr while still serving initialize/tools requests; the client must not deadlock.

## 6. Ordered work packages

### Work package A — Reproduce and pin the production startup failures

1. Add a multi-process regression using private runtime and data roots that calls the real connect-or-start path.
2. Assert that an autostarted daemon remains reachable after `connect_or_start_daemon` returns.
3. Add a plain-entrypoint smoke seam proving ordinary `codegg` reaches immediately-before/inside TUI event-loop initialization without a manually prestarted daemon. Avoid trying to snapshot terminal escape sequences if a deterministic seam can prove the same contract.
4. Add a fresh-empty-catalog fixture that fails on the current legacy-store bootstrap if that failure is reproducible.
5. Add a peer-death request test showing current pending-request behavior.

Do not implement first and then write tests that cannot fail on the baseline.

### Work package B — Correct daemon storage bootstrap

1. Replace the production legacy project-store initialization with the migrated user-scoped daemon catalog path.
2. Prefer one shared `open_migrated_daemon_catalog`-style helper if it removes duplication with `run_core_stdio`.
3. Ensure daemon tests isolate catalog data from the real user profile.
4. Confirm session/workspace/job recovery operates against the catalog.
5. Do not auto-promote a project-local legacy DB into production authority; preserve existing explicit migration/import semantics.

### Work package C — Correct autostart ownership and races

1. Remove `kill_on_drop(true)` from the singleton daemon autostart child only.
2. Establish the intended process session/group detachment for the supported Unix targets.
3. Keep child observation during startup.
4. After verified readiness, hand the child to a non-killing reaper task.
5. On child exit during startup, perform a final verified connect before returning failure.
6. On startup timeout/failure, terminate and reap the helper-owned child if still alive.
7. Add a two-concurrent-client regression: exactly one daemon survives and both clients succeed.

### Work package D — Make socket readiness and disconnect behavior finite

1. Make ClientHello write errors visible.
2. Complete ServerHello handshake before a connection is considered ready.
3. Add bounded handshake/control-plane probe semantics tied to the startup deadline.
4. Drain/fail pending request senders on reader EOF/error.
5. Ensure the connect-or-start deadline includes readiness/status proof, not only `UnixStream::connect` polling.
6. Add fixtures for non-responsive socket, peer death after request write, and wrong/non-CodeGG listener.
7. Reconcile reconnect behavior: fully re-handshake/resubscribe or fail fast for higher-level reconnect; do not retain partial current semantics.

### Work package E — Centralize endpoint selection

1. Delete or stop using duplicate `default_socket_path` logic.
2. Normalize plain paths and `unix://` URIs once.
3. Apply CLI/env override to the actual `DaemonPaths` passed into `connect_or_start_daemon`.
4. Make `AttachDaemon --endpoint` testably use the requested socket.
5. Add Linux fallback coverage where `XDG_RUNTIME_DIR` is absent so attach/default paths cannot diverge.

### Work package F — Correct graceful SIGTERM shutdown and bound draining

1. Install supported Unix SIGINT and SIGTERM listeners feeding the same cancellation token.
2. Thread `shutdown_timeout_ms` to connection cleanup.
3. Bound `JoinSet` draining and dispose of remaining connection tasks safely at expiry.
4. Preserve daemon stop identity verification before SIGTERM.
5. Optionally have `daemon stop` wait boundedly for observable shutdown completion using the existing timeout configuration.
6. Add a real SIGTERM regression asserting socket/PID/metadata cleanup, lock release, and immediate restart.
7. Add a stuck-client fixture proving shutdown completes within the configured bound.

### Work package G — Make daemon logs useful

1. Resolve default logs through `core::instance::DaemonPaths::log_path`.
2. Autostart with stdout/stderr directed to that log rather than null.
3. Preserve user-only permissions.
4. Include the log path in startup timeout/child-exit diagnostics.
5. Keep explicit `daemon logs --file` behavior unchanged.

### Work package H — Drain local MCP stderr

1. Take the child's stderr pipe.
2. Spawn a bounded-memory continuous drain task.
3. Ensure read-loop shutdown and child shutdown do not leak the drain task.
4. Add a noisy-stderr MCP integration/unit fixture.

### Work package I — Adjacent transport disposition and documentation

1. Inspect `StdioCoreClient` child ownership and concurrent request behavior.
2. If a small request gate/EOF correction is sufficient and directly tested, it may land here; otherwise record a separate follow-up in the closure record and registry only if it is dependency-ready and materially supported.
3. Do not modify `egglsp` or `managed_process` absent a reproduced independent defect.
4. Update `architecture/core.md`, `architecture/tui.md`, `architecture/storage.md`, `architecture/client.md`, and `docs/TROUBLESHOOTING.md` only where current statements become inaccurate.
5. Update AGENTS/skills only if they encode lifecycle commands or path invariants affected by C003.

## 7. Storage, protocol, migration, and compatibility effects

### Storage

- Production daemon authority moves back to the already-designed user-scoped daemon catalog.
- No new schema is expected.
- Existing current migrations MUST run before normal daemon use.
- Legacy project stores remain compatibility/import data and are not deleted.

### Protocol

- No new wire message is expected.
- Existing ClientHello/ServerHello and SnapshotDaemon are sufficient for verified readiness.
- Reconnect behavior may become stricter if the current retry path cannot preserve negotiated state safely.

### Configuration

- `startup_timeout_ms` must actually bound connect-or-start readiness.
- `shutdown_timeout_ms` must actually bound graceful connection draining.
- Do not add new config knobs unless the existing two cannot express the required behavior.

### CLI compatibility

- Plain `codegg` remains the normal command.
- `--standalone` and `--stdio` remain explicit alternatives.
- `daemon start|stop|status|logs` remain available.
- `AttachDaemon --endpoint` and existing endpoint environment override become correct rather than silently ignored/divergent.

## 8. Focused verification

Use exact selectors that exist after implementation. The expected minimum is:

```bash
cargo fmt --all -- --check
cargo test --test single_daemon_lifecycle -- --nocapture
cargo test -p codegg core::instance -- --nocapture
cargo test -p codegg core::transport::socket -- --nocapture
cargo test -p codegg core::transport::daemon_socket -- --nocapture
cargo test -p codegg mcp::local -- --nocapture
scripts/verify.sh quick
```

Add focused real-process tests for all of the following:

1. **autostart lifetime** — helper starts daemon, helper returns, second independent client still gets `SnapshotDaemon`;
2. **ordinary TUI startup** — no manually prestarted daemon; production mode reaches TUI initialization;
3. **concurrent autostart** — two clients race from an empty runtime root; one daemon owns the lock and both clients connect successfully;
4. **fresh catalog** — empty user data root migrates and starts; no project-local `.codegg/sessions.db` becomes daemon authority;
5. **startup timeout** — a listener that accepts but never completes CodeGG readiness cannot hang past the configured startup bound;
6. **peer death** — a request pending when the socket closes returns an error and is removed from `pending`;
7. **endpoint override** — attach/connect uses the exact custom socket path;
8. **SIGTERM graceful stop** — signal enters cancellation path, runtime artifacts are cleaned, and immediate restart succeeds;
9. **bounded shutdown** — a stuck client cannot prevent daemon exit beyond `shutdown_timeout_ms` plus a small test tolerance;
10. **daemon logs** — autostart failure writes actionable diagnostics to the same default path read by `daemon logs`;
11. **noisy MCP stderr** — a child writing beyond normal pipe capacity continues serving requests and shuts down cleanly.

Avoid broad arbitrary sleeps. Prefer readiness sockets, metadata appearance/disappearance, process status, channels, and bounded polling with explicit deadlines.

## 9. Static guards

No new repository-wide source scanner is required.

The compiler plus focused lifecycle tests should carry these invariants. A very small unit test asserting canonical endpoint/path resolution is preferable to a text scanner. Do not add grep-based CI policy for `kill_on_drop` because the call is valid in other ownership domains such as `managed_process`.

## 10. Documentation updates

At closure, documentation must state the implemented truth:

- ordinary `codegg` connects to or starts one daemon and the daemon survives the initiating frontend;
- production daemon storage is user-scoped catalog storage;
- endpoint precedence is canonical and explicit;
- startup/shutdown timeouts have defined semantics;
- SIGINT/SIGTERM use graceful shutdown on supported Unix;
- daemon logs resolve to the user-scoped daemon log path;
- stale socket recovery and concurrent startup behavior are documented if operator-visible.

Do not duplicate implementation details into every document. Keep `architecture/core.md` authoritative and link from troubleshooting/client/TUI docs where appropriate.

## 11. Acceptance criteria

C003 is complete only when every required criterion below is met.

### Production startup

- [ ] Plain `codegg` can start from a fresh runtime/data root without `--standalone` and reach the TUI startup/event-loop boundary.
- [ ] `connect_or_start_daemon` never kills a successfully started daemon because its local child handle is dropped.
- [ ] The started daemon survives independently of the initiating frontend.
- [ ] The parent frontend does not accumulate an unreaped daemon child while it remains alive.
- [ ] Concurrent first-client startup yields exactly one live daemon and all clients converge on it.
- [ ] Failed/timed-out autostart does not leave an unintended helper-owned child behind.

### Storage bootstrap

- [ ] Production `run_daemon` uses a migrated user-scoped daemon catalog.
- [ ] Fresh catalog startup succeeds.
- [ ] Project-local legacy storage is not used as production daemon authority.
- [ ] Existing legacy import/compatibility behavior is not deleted or silently redefined.

### Socket lifecycle

- [ ] Connection readiness requires a successful CodeGG handshake.
- [ ] Startup readiness/status proof is bounded by the configured startup deadline.
- [ ] EOF/read failure releases every pending request waiter associated with the failed connection.
- [ ] A non-responsive or non-CodeGG Unix listener cannot make startup hang indefinitely.
- [ ] Reconnect either fully restores negotiated state/subscriptions or fails explicitly for a fresh higher-level reconnect; no partial silent reconnect remains.

### Endpoint correctness

- [ ] CLI endpoint override is honored by the TUI/socket path.
- [ ] `AttachDaemon --endpoint` reaches the requested endpoint.
- [ ] `CODEGG_CORE_ENDPOINT` follows documented precedence.
- [ ] Duplicate Linux default-socket fallback logic is removed or made impossible to diverge.

### Shutdown

- [ ] SIGINT and SIGTERM enter the same graceful daemon cancellation path on supported Unix.
- [ ] `shutdown_timeout_ms` bounds graceful connection draining.
- [ ] Socket, PID, and metadata cleanup occurs on graceful SIGTERM.
- [ ] The lock is released and an immediate restart succeeds after `daemon stop`/SIGTERM.
- [ ] No unverified PID is force-killed as a timeout fallback.

### Diagnostics and child processes

- [ ] Autostart daemon stdout/stderr are not discarded without an operator-readable path.
- [ ] `daemon logs` defaults to that same user-scoped daemon log.
- [ ] Local MCP stderr is continuously drained with bounded memory.
- [ ] Noisy MCP stderr cannot deadlock initialization or tool requests.

### Verification/closure

- [ ] Focused lifecycle/process tests pass.
- [ ] `scripts/verify.sh quick` passes.
- [ ] No new CI lane/matrix/release automation/service-manager dependency is added.
- [ ] No critical/high unresolved C003 finding remains.
- [ ] `plans/closure/post-audit-correctness-simplification/012-status.md` records exact implementation SHA(s), test evidence, and any lower-severity deferred findings.

## 12. Stop conditions

Stop and report instead of broadening C003 if:

- correct daemon detachment requires changing the supported platform contract or introducing an external service manager;
- production catalog recovery requires a new storage schema or data migration beyond running existing migrations/import machinery;
- verified socket readiness requires a new protocol message rather than existing ClientHello/ServerHello/SnapshotDaemon;
- correct reconnect requires redesigning the session-projection protocol rather than choosing a safe fail-fast/fresh-resync behavior;
- bounded shutdown requires changing scheduler/job semantics or killing active jobs without an existing authority contract;
- fixing local MCP stderr requires replacing the MCP subsystem rather than draining the pipe;
- implementation pressure suggests making standalone mode the default or weakening the singleton invariant;
- tests require touching the real user's daemon catalog/runtime paths.

Record the blocker precisely and create a separate architecture decision/follow-up only if the current architecture genuinely cannot satisfy the lifecycle contract.

## 13. Required closure evidence

`plans/closure/post-audit-correctness-simplification/012-status.md` must include:

- exact implementation commit(s)/PR;
- requirement-to-evidence matrix covering every acceptance criterion;
- proof that the autostarted daemon remains alive after helper return;
- concurrent-autostart convergence evidence;
- fresh migrated daemon-catalog path and test evidence;
- handshake/startup-timeout and peer-death pending-request evidence;
- endpoint override evidence;
- SIGTERM graceful cleanup and bounded-shutdown evidence;
- daemon-log path evidence;
- noisy-MCP-stderr evidence;
- exact focused verification commands and outcomes;
- security/ownership review confirming singleton/stop identity invariants were not weakened;
- compatibility notes for reconnect or endpoint behavior;
- unresolved findings classified critical/high/medium/low/deferred;
- final recommendation: closed, conditionally closed, corrective pass required, or blocked.
