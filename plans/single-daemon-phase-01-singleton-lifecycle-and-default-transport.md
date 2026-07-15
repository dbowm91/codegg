# Single-Daemon Phase 1: Singleton Lifecycle and Default Transport

## Status

Proposed first implementation phase for the single-daemon multi-project orchestration roadmap.

This phase establishes the production invariant that exactly one user-scoped Codegg daemon owns execution at a time. It does not yet introduce workspace-aware execution or a global scheduler. It makes the existing daemon safe to treat as the future composition root.

## 1. Problem statement

Codegg currently exposes several execution topologies:

- ordinary TUI operation can use an in-process core;
- `codegg daemon start` constructs a socket daemon;
- `core-stdio` constructs another in-process runtime stack;
- HTTP server mode can construct another `CoreDaemon`;
- explicit socket attachment can connect a TUI to a running daemon.

Each topology can create its own SQLite pool, subagent pool, background scheduler, memory store, event bridge, and agent runtime. This permits several independent production execution environments to coexist, defeating machine-wide resource coordination.

Daemon startup also removes the target socket path before binding and relies on a PID file for stop/status. A second process can race with or unlink the pathname of a healthy daemon. PID files alone do not provide a process-lifetime singleton guarantee and are susceptible to stale PID reuse.

## 2. Goals

### 2.1 Functional goals

- Guarantee one active user-scoped production daemon.
- Make ordinary local frontend startup use a connect-or-start socket flow by default.
- Ensure a second daemon invocation connects to or reports the existing daemon rather than replacing its socket.
- Preserve explicit in-process mode for tests, embedding, and diagnostics.
- Preserve an explicit standalone mode for emergency or compatibility use, but make it visibly non-orchestrated.
- Make HTTP and stdio frontends proxy to the existing daemon rather than constructing parallel runtime stacks in normal operation.
- Add daemon generation and endpoint metadata sufficient for diagnostics and later recovery leases.

### 2.2 Safety goals

- Never unlink a socket owned by a healthy daemon.
- Never treat a PID file as sole proof that a daemon is alive or dead.
- Never force-update or steal a singleton lock from a live process.
- Preserve current local socket permissions and avoid widening the trust boundary.
- Ensure startup and shutdown cleanup are idempotent.
- Ensure SIGINT, SIGTERM, normal return, and startup failure all release owned resources correctly.

### 2.3 Maintainability goals

- Centralize daemon bootstrap in one reusable builder.
- Remove duplicated construction of pools, subagent pools, background schedulers, memory stores, and event bridges from `src/main.rs` command branches.
- Keep transport concerns separate from runtime construction.
- Make legacy modes explicit through names and configuration rather than implicit defaults.

## 3. Non-goals

This phase does not:

- add `WorkspaceId` or workspace registration;
- change session storage layout;
- add a durable job queue;
- add global resource permits;
- migrate TestRunner, Bash, Python, Git, or agent turns into a scheduler;
- redesign permission routing or client control leases;
- implement systemd or launchd service installation;
- add remote multi-host daemon discovery;
- remove the in-process core used by unit tests.

## 4. Required architecture

### 4.1 Daemon instance guard

Add a process-lifetime guard, preferably under `src/core/instance.rs` or a similarly focused module:

```rust
pub struct DaemonInstanceGuard {
    lock_file: std::fs::File,
    lock_path: PathBuf,
    socket_path: PathBuf,
    metadata_path: PathBuf,
    generation: uuid::Uuid,
}
```

The guard must acquire an advisory exclusive lock using a cross-platform implementation appropriate for the currently supported Unix targets. The lock must remain held for the daemon lifetime.

Preferred lock location:

```text
macOS: ~/Library/Application Support/codegg/daemon.lock
Linux: $XDG_RUNTIME_DIR/codegg/daemon.lock
fallback: user-scoped runtime/config directory, not a project directory
```

The endpoint path, lock path, metadata path, and log path should be resolved by one `DaemonPaths` helper rather than separate ad hoc functions.

### 4.2 Metadata record

Persist an atomic, informational metadata record after the socket is successfully bound:

```rust
pub struct DaemonInstanceMetadata {
    pub daemon_id: String,
    pub generation: String,
    pub pid: u32,
    pub socket_path: PathBuf,
    pub protocol_version: u32,
    pub started_at: DateTime<Utc>,
    pub binary_version: String,
}
```

The lock is authoritative. Metadata and PID files are diagnostic aids.

Write metadata by temporary file plus atomic rename. Restrict file permissions to the user where the platform permits it.

### 4.3 Startup algorithm

The startup sequence must be deterministic:

1. Resolve user-scoped daemon paths.
2. Ensure the parent directory exists with user-only permissions where supported.
3. Attempt to acquire the singleton lock without blocking indefinitely.
4. If acquisition fails:
   - connect to the configured socket;
   - perform `ClientHello`/`ServerHello` and `Ping`/`Pong` or `SnapshotDaemon` health verification;
   - report the active daemon identity and return an `AlreadyRunning` result;
   - if the socket is unreachable, report an inconsistent lock/socket state and do not unlink or steal the lock.
5. If acquisition succeeds:
   - inspect any existing socket path;
   - if a health check succeeds, treat this as an invariant violation and abort without unlinking;
   - if no listener exists, remove the stale socket path;
   - build the daemon runtime;
   - bind the socket;
   - atomically write metadata;
   - enter the serve loop while retaining `DaemonInstanceGuard`.
6. On graceful shutdown, stop accepting new work, terminate runtime services, remove owned socket/metadata paths, and drop the guard.

Do not remove the socket before lock acquisition.

### 4.4 Connect-or-start frontend flow

Add a reusable function such as:

```rust
pub async fn connect_or_start_daemon(
    options: ConnectOrStartOptions,
) -> Result<SocketCoreClient, DaemonConnectError>;
```

Expected behavior:

- try connecting to the user-scoped endpoint;
- if successful, return the client;
- if absent and autostart is enabled, start the daemon through a clearly bounded foreground-child or detached-launch mechanism already acceptable to the repository;
- poll the endpoint with a bounded timeout and protocol handshake;
- return a typed startup error with log/metadata location on failure.

Avoid shell-based daemon launching. Use `std::process::Command`/`tokio::process::Command` with explicit argv and `kill_on_drop` semantics appropriate to the chosen lifecycle.

This phase may initially use a foreground child spawned by the TUI and detached only after successful socket readiness. The implementation must document ownership and avoid orphan ambiguity. A later service-installation phase may replace the launch mechanism.

### 4.5 Production mode classification

Introduce an explicit mode enum rather than treating all `CoreTransport` choices as equivalent:

```rust
pub enum CoreRuntimeMode {
    DaemonClient,
    StandaloneInproc,
    StandaloneStdio,
}
```

Recommended user-facing behavior:

- plain `codegg`: `DaemonClient` with connect-or-start;
- `codegg attach-daemon`: explicit `DaemonClient`;
- `codegg daemon start`: start the singleton daemon in foreground unless an existing daemon is healthy;
- `codegg --standalone`: explicit `StandaloneInproc` with a warning that global scheduling is unavailable;
- hidden `core-stdio`: compatibility/testing only, clearly marked standalone;
- HTTP server: proxy or embed a client connection to the daemon by default; an explicit standalone server flag is required to construct an independent core.

Preserve existing `--core-transport` parsing temporarily, but map deprecated values through compatibility handling and diagnostics.

## 5. Concrete repository changes

### `src/main.rs`

- Replace duplicated daemon/subagent/scheduler construction with `DaemonRuntimeBuilder` or equivalent.
- Replace the ordinary TUI default from `Inproc` to daemon connect-or-start.
- Add explicit `--standalone` or equivalent compatibility flag.
- Make `DaemonCommand::Start`, `Stop`, and `Status` use `DaemonPaths` and typed instance metadata.
- Make `Stop` verify metadata/lock/socket state before signaling a PID.
- Remove unconditional pre-bind socket deletion.
- Ensure `core-stdio` and server mode are explicitly standalone or daemon-proxying.

### `src/core/daemon.rs`

- Accept a daemon generation/instance descriptor at construction rather than generating all identity internally, or expose a builder that keeps identity consistent with metadata.
- Add a graceful shutdown hook surface for runtime-owned services.
- Keep `CoreDaemon` transport-neutral.

### `src/core/transport/daemon_socket.rs`

- Accept an already-bound listener or bind only after singleton validation.
- Add graceful accept-loop shutdown through `CancellationToken` or watch channel.
- Preserve per-client task cleanup and event forwarding.
- Return typed bind/serve errors without deleting paths it does not own.

### `src/core/transport/socket.rs`

- Add a bounded health-check/handshake helper reusable during startup, status, and stale-socket evaluation.
- Distinguish connection refusal, protocol mismatch, timeout, and malformed server hello.

### `src/core/mod.rs`

- Export instance and lifecycle types behind focused modules.

### `crates/codegg-config/src/schema.rs`

Add conservative daemon lifecycle configuration:

```rust
pub struct DaemonConfig {
    // existing fields...
    pub autostart: Option<bool>,
    pub mode: Option<DaemonModeConfig>,
    pub startup_timeout_ms: Option<u64>,
    pub shutdown_timeout_ms: Option<u64>,
}
```

Default `autostart` should be true only when the connect-or-start path is operationally proven on supported platforms. During initial implementation it may remain opt-in while plain `codegg` emits a migration notice.

### Documentation

Update:

- `architecture/core.md`;
- `architecture/client.md`;
- `architecture/server.md`;
- `architecture/protocol.md` if handshake capabilities change;
- `docs/TROUBLESHOOTING.md`;
- README startup examples.

## 6. Compatibility and migration

### In-process tests

Do not remove `InprocCoreClient`. Unit tests and focused integration tests may continue constructing it directly.

### Existing flags

- Continue parsing `--core-transport inproc|stdio|socket` for one deprecation cycle.
- Emit a targeted warning for production `inproc`/`stdio` use.
- Do not silently reinterpret an explicitly requested standalone mode as daemon mode.

### Existing daemon endpoints

Honor `CODEGG_CORE_ENDPOINT` and explicit `--endpoint`, but singleton scope must be clear. If custom endpoints are allowed to create additional daemon instances, require an explicit development/test mode. Production defaults must remain one user-scoped endpoint.

### HTTP server

If daemon proxying cannot land safely in this phase, server mode must require an explicit `--standalone-core` flag before constructing another daemon. The default should fail closed with an actionable message rather than silently creating a second core.

## 7. Testing plan

Run Rust tests with `--test-threads=1` according to repository policy.

### Unit tests

- path resolution on macOS/Linux/fallback;
- atomic metadata serialization and parsing;
- lock acquisition and release;
- stale metadata without lock;
- lock held with missing socket;
- socket present without lock;
- explicit mode resolution and deprecated flag mapping;
- startup timeout and protocol mismatch errors.

### Multi-process integration tests

Add tests under `tests/` using a temporary runtime directory:

1. Start daemon A and verify health.
2. Start daemon B against the same lock/endpoint.
3. Assert B reports `AlreadyRunning` and does not remove A's socket.
4. Assert A remains reachable after B exits.
5. Kill A ungracefully, retain stale socket/metadata, then start C.
6. Assert C acquires the free lock, verifies no listener, removes stale paths, and starts successfully.

Add a simultaneous-start race with two child processes and assert exactly one becomes server.

### Frontend tests

- plain frontend connects to an existing daemon;
- absent daemon triggers bounded autostart when enabled;
- frontend disconnect does not stop daemon;
- explicit standalone mode never touches the singleton lock;
- server/stdio compatibility modes cannot silently become second production daemons.

### Shutdown tests

- SIGINT removes only owned socket/metadata;
- SIGTERM follows the same cleanup path;
- graceful shutdown stops accepting clients before resource teardown;
- abnormal exit leaves a recoverable stale socket but no permanently held lock.

## 8. Acceptance criteria

Phase 1 is complete when:

- two normal Codegg daemon processes cannot be active for one user scope;
- startup never unlinks the socket of a healthy daemon;
- plain local frontend startup uses or can transition cleanly to connect-or-start daemon operation;
- production server and stdio paths do not silently construct parallel cores;
- in-process execution remains available only through explicit standalone/test APIs;
- daemon status reports daemon ID, generation, PID, protocol version, endpoint, uptime, clients, and sessions;
- stop/status behavior validates the live daemon rather than trusting only a PID file;
- all singleton race, stale-path, shutdown, and compatibility tests pass;
- documentation clearly distinguishes daemon-client and standalone modes.

## 9. Handoff checklist

- [ ] Introduce `DaemonPaths` and `DaemonInstanceGuard`.
- [ ] Add atomic daemon metadata and generation identity.
- [ ] Centralize daemon runtime construction.
- [ ] Add safe startup and stale-socket decision logic.
- [ ] Add graceful socket accept-loop cancellation.
- [ ] Implement connect-or-start frontend helper.
- [ ] Switch or stage ordinary TUI default toward daemon client mode.
- [ ] Gate standalone in-process/stdio/server modes explicitly.
- [ ] Add multi-process singleton and recovery tests.
- [ ] Update architecture, CLI, and troubleshooting documentation.
