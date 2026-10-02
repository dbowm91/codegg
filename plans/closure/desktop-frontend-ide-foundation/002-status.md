# Desktop Frontend and IDE Foundation M002 — Closure Record

Status: conditionally closed

Implementation plan: `plans/implementation/desktop-frontend-ide-foundation/002-portable-local-daemon-transport.md`

Implementation commit: `ebc7c79a` (`feat: add portable local daemon transport`)

Closure transition commit: `a3708cb` (`plans: move portable transport to closing`)

## Outcome

The shared local CoreFrame path now has typed `unix://` / `npipe://` endpoint
handling and one byte-stream request/event implementation. Unix remains on its
existing domain socket adapter. Windows code now provides a named-pipe client
and listener, byte mode, remote-client rejection, first-instance ownership,
per-user plus LocalSystem DACLs, a pending replacement instance, and a bounded
client count. The singleton lock uses Rust's standard nonblocking
`File::try_lock`; the held file remains owned for the daemon lifetime. New
metadata includes an optional platform-neutral `endpoint_uri`, while legacy
records without that field continue to decode.

Windows graceful `daemon stop` is deliberately rejected. Windows server/client
runtime behavior, cross-user negative access, connection lifecycle, and
singleton restart behavior have not been run on a Windows host. This satisfies
the plan's stop condition for unavailable live Windows evidence, so the record
does not qualify Windows or claim strict milestone completion.

## Endpoint and ownership flow

```text
Frontend
  -> SocketCoreClient compatibility adapter
  -> codegg-client LocalEndpoint + LocalSocketClient
  -> platform client stream (UnixStream | named-pipe client)
  -> shared CoreFrame JSONL reader/writer

Daemon start
  -> DaemonPaths + File::try_lock guard
  -> daemon_socket LocalSocketListener
  -> UnixListener | protected Windows named-pipe listener
  -> shared boxed byte-stream handler
  -> CoreDaemon request authorization and event/projection lifecycle
```

Endpoint names are routing data, not caller identity. The Windows DACL is
constructed from the current process token SID and LocalSystem only, is
protected from inherited ACEs, and has no Everyone/anonymous ACE. Construction
failure aborts listener setup. The listener also requests remote-client
rejection. No HTTP, TCP, CoreFrame version change, SQLite migration, or support
tier change was introduced.

## Verification evidence

- `scripts/verify.sh quick` — passed after implementation; all workspace
  all-target checks and repository guards passed. One unrelated existing
  `AtomicU64::fetch_update` deprecation warning remains.
- `cargo test -p codegg --lib core::instance::tests` — 11 passed, including
  lock contention, metadata roundtrip, and legacy metadata decode.
- `cargo test -p codegg --lib core::transport::daemon_socket::daemon_socket_integration_tests`
  — 22 passed; existing Unix listener, request, event, and lifecycle behavior
  passed through the generalized stream boundary.
- `cargo test -p codegg-client` — 12 passed.
- `cargo clippy -p codegg-client --all-targets -- -D warnings` — passed.
- `cargo check -p codegg-client --tests --target x86_64-pc-windows-msvc` —
  passed, including Windows endpoint, DACL, process-probe, and test code.
- `cargo clippy -p codegg-client --tests --target x86_64-pc-windows-msvc -- -D warnings`
  — passed.
- `scripts/check-client-boundary.sh`, `cargo fmt --check`, and
  `git diff --check` — passed.
- `cargo check -p codegg --lib --target x86_64-pc-windows-msvc` — attempted;
  blocked before checking CodeGG because the `ring` build script requires an
  MSVC-compatible compiler, which is not installed in this Linux environment.

## Lock and security details

`DaemonInstanceGuard::try_acquire` and the client lock probe both call
`File::try_lock`. `TryLockError::WouldBlock` maps to the existing held-lock
state; other I/O errors remain errors. The lock file is opened read/write on
Windows and the guard retains the one acquired file handle. Windows PID
liveness checks use a process synchronization handle; unknown liveness fails
closed. `--force-take-lock` cannot unlink a path while the OS lock is held.

The narrow unsafe boundaries are `crates/codegg-client/src/windows_pipe_security.rs`
for Windows token/SID/security descriptor APIs,
`crates/codegg-client/src/windows_process.rs` for process liveness, and the
single annotated `ServerOptions::create_with_security_attributes_raw` call in
`src/core/transport/daemon_socket.rs`. No crate-wide unsafe policy was relaxed.

## Remaining qualification blockers

- Run a full CodeGG Windows target build using a supported Windows toolchain.
- On Windows, verify allowed current-user access and denied broader/untrusted
  access; inspect the effective DACL and remote-client rejection.
- Run live handshake, request/event, concurrent-client, reconnect, peer-death,
  listener restart, autostart/reuse, and concurrent-start convergence cases.
- Implement and qualify a graceful Windows daemon stop/replacement path; it is
  currently reported as unavailable.

These are correctness and runtime qualification requirements, not support-tier
promotion work. Windows remains best-effort/compatibility-only.

## Dependency audit and disposition

- No future hard dependency was resolved by this closure.
- M003 remains `ready`: its plan explicitly classifies M002 as operational
  only for Windows, and the Unix interface is implemented. M003 may proceed on
  Unix while this Windows qualification is pending.
- M004 remains `blocked` on M003, unchanged.
- M005 and M006 remain deferred, unchanged.
- No Windows installer/support plan is unblocked or implied by this record.
