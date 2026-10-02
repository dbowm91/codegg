# Desktop Frontend and IDE Foundation Milestone 002 — Portable Local Daemon Transport

Status: ready for handoff

Repository baseline: `deebc99a39c783eb3f52918fb8f3f2c138cb65b6`

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-002--portable-local-daemon-transport-and-singleton-lifecycle`

Long-term requirements:

- `plans/000-long-term-specification.md#1-product-definition`
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`

Applicable ADRs:

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md`

Primary class: infrastructure

Hard dependency:

- M001 shared frontend client runtime must close first.

Operational closure dependency:

- live Windows runtime evidence is required before the Windows local transport is described as closed/supported.

## 1. Objective

Generalize the singleton daemon's local client/server transport from a Unix-only implementation into one platform-local CoreFrame transport contract: Unix-domain sockets on Unix and a securely ACL-constrained Windows named pipe on Windows, while replacing the hand-written Unix-only singleton file lock with the Rust 1.89 standard file-lock API where behavior is equivalent.

The milestone must preserve daemon identity, authorization, CoreFrame framing, request/event ordering, reconnect, and singleton ownership semantics. It improves portability but does not promote Windows to a guaranteed release/support tier.

## 2. Why this milestone is blocked

M001 must first establish the reusable `codegg-client` ownership boundary. Implementing Windows named pipes directly in the current root/TUI-specific `SocketCoreClient` would entrench exactly the coupling the desktop foundation is meant to remove.

Once M001 closes, the remaining protocol contract is stable: `CoreFrame` is byte-stream/JSONL compatible and does not require Unix-specific semantics.

## 3. Current implementation evidence

At the audit baseline:

- `src/core/transport/socket.rs` imports and stores `tokio::net::UnixStream` and its owned read/write halves.
- `src/core/transport/daemon_socket.rs` binds a `tokio::net::UnixListener`.
- `DaemonPaths` models `socket_path: PathBuf` and emits only `unix://...`.
- Linux/macOS production paths are user-scoped and current Unix socket/lock permissions are tightened.
- `DaemonInstanceGuard::try_acquire` uses a hand-written `libc::flock(LOCK_EX | LOCK_NB)`.
- `#[cfg(not(unix))] try_flock_exclusive` returns an error stating singleton locking is unsupported.
- Rust 1.89, already CodeGG's MSRV, stabilizes `File::try_lock`; std documents `flock` on Unix and `LockFileEx` on Windows.
- Tokio's Windows named-pipe API provides asynchronous client/server byte streams suitable for the existing JSONL framing.
- Microsoft documents that a default named-pipe security descriptor grants full control to LocalSystem/administrators/creator-owner and read access to Everyone/anonymous. That default is too broad for CodeGG's LocalOwner trust boundary.
- Windows named pipes can be remotely addressable in general; CodeGG must use a local-only pipe name and reject/deny remote access rather than treating the pipe name as authentication.
- the current distribution roadmap intentionally keeps Windows outside the guaranteed installer/support tier.

Research references:

- Rust file locking: https://doc.rust-lang.org/std/fs/struct.File.html
- Tokio named pipes: https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/
- Tokio `ServerOptions` DACL support hook: https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/struct.ServerOptions.html
- Microsoft named-pipe security: https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights
- Microsoft named-pipe locality/security note: https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipes

## 4. Invariants that must not regress

- Exactly one user-scoped daemon owns the production local endpoint.
- The lock, not the metadata file, remains authoritative for singleton ownership.
- A stale metadata record never authorizes unlinking, stealing, or connecting to another process.
- CoreFrame serialization and protocol versions are transport-independent.
- Client identity remains assigned/verified by daemon transport state, not by a payload-provided principal.
- Unix clients/daemon behavior remains compatible.
- A Windows pipe is current-user/local scoped; default broad named-pipe ACLs are not acceptable.
- Endpoint names/paths contain no secrets and are not themselves treated as authentication.
- Local transport queues and request/replay behavior remain bounded.
- The desktop/TUI does not need an HTTP server for normal local use.
- Windows portability work does not silently change the project's declared Windows support tier.

## 5. Scope

### In scope

- A platform-neutral local endpoint type/API shared by client and daemon.
- Unix endpoint implementation retaining filesystem Unix-domain sockets.
- Windows endpoint implementation using named pipes in byte-stream mode.
- Endpoint URI parsing/formatting, with an explicit Windows scheme such as `npipe://` or another documented unambiguous representation.
- A local stream abstraction sufficient for the existing JSONL reader/writer.
- A listener abstraction sufficient for the daemon's current connection lifecycle/projection ownership machinery.
- Rust 1.89 `File::try_lock` migration where semantics match current flock behavior.
- Windows user-scoped runtime/metadata path resolution.
- Explicit Windows named-pipe security descriptor/DACL.
- Explicit local-only/no-network pipe posture.
- Windows daemon child startup/teardown behavior needed for connect-or-start.
- Cross-platform compile guards plus live Windows integration evidence.
- Architecture/validation documentation.

### Explicitly out of scope

- Windows installer support.
- Promoting Windows to a guaranteed release target.
- Tauri itself.
- TCP loopback fallback.
- Remote `/core` WebSocket.
- Changing CoreFrame JSONL framing.
- Reworking projection/replay semantics.
- Service installation (Windows Service, launchd, systemd).
- Global/machine-wide daemon mode.
- Multi-user shared Windows daemon.
- Cross-host named pipes.
- General-purpose IPC crate extraction unless the implementation clearly proves a second independent consumer.

## 6. Required production changes

### Core/domain

Introduce a typed local endpoint rather than passing raw strings whose meaning is inferred ad hoc. The shape may resemble:

```rust
enum LocalEndpoint {
    #[cfg(unix)]
    Unix { path: PathBuf },
    #[cfg(windows)]
    WindowsPipe { name: OsString },
}
```

The exact public API may be more abstract, but callers must not concatenate `unix://` or `\\.\pipe\` strings throughout the codebase.

Client and daemon must use the same normalization/canonical-name logic.

### Local byte-stream abstraction

Parameterize the CoreFrame reader/writer over Tokio `AsyncRead + AsyncWrite` (or an equivalent internal trait/type) so request correlation, JSONL framing, and event dispatch are not duplicated per platform.

Unix and Windows should differ at endpoint connect/listen/accept, not at CoreFrame semantics.

If owned read/write halves differ enough that a small enum is clearer than boxed trait objects, that is acceptable; avoid two copies of the frame state machine.

### Unix transport

Retain Unix-domain socket behavior, permission tightening, stale-endpoint handling, and lifecycle tests.

M002 must not regress the heavily qualified projection transport task-ownership semantics merely to generalize the listener type.

### Windows named pipe

Use a duplex byte-mode named pipe suitable for line-delimited CoreFrame traffic.

The implementation must:

- use a local `\\.\pipe\...` name;
- prevent remote-client use where the platform API permits;
- install an explicit DACL that grants the intended current user (and narrowly justified system/administrator identities if required) the necessary access;
- deny anonymous/Everyone broad access;
- make the first-instance/listener race behavior deterministic;
- maintain an available server instance while accepting connections so transient client `NotFound` races are not introduced;
- bound instance count consistently with daemon client limits;
- treat ACL construction failure as fatal rather than falling back to the default descriptor.

Tokio exposes raw security-attribute hooks but constructing Windows security descriptors may require Windows API calls. Any necessary unsafe must be isolated behind one small reviewed Windows platform module or crate with explicit invariants and tests. Do not weaken a crate-wide unsafe policy broadly.

A third-party IPC crate may be considered only if its security semantics are auditable and it can enforce the same current-user/local boundary. Do not adopt one merely to avoid writing a small adapter while accepting a default broad ACL.

### Singleton lock

Replace custom Unix `libc::flock` where possible with Rust 1.89 `File::try_lock`.

Map `TryLockError::WouldBlock` to the existing "lock held" state and preserve all other I/O errors.

Retain one open file handle in `DaemonInstanceGuard` for lock lifetime. Do not call `try_lock` twice on the same held handle.

Add Windows file-open semantics compatible with `LockFileEx` (read/write rather than append-only).

### Runtime paths

Define a user-scoped Windows daemon runtime root using an appropriate per-user application-data/runtime location. The lock and metadata files live there; the named-pipe endpoint itself is a namespaced object, not a filesystem socket file.

Metadata must encode the endpoint in a platform-neutral form. If the persisted metadata schema changes, keep backward-compatible decoding for existing Unix metadata or version the record explicitly.

### Daemon process lifecycle

Audit `detach_daemon_process` and startup behavior on Windows.

The connect-or-start caller must be able to launch an existing `codegg.exe daemon start ...`, detect early exit, and converge on a concurrent singleton winner exactly as on Unix.

Do not add a Windows Service in this milestone.

### Storage and migrations

No SQLite migration.

Daemon metadata JSON may require an additive endpoint field/version. Any change must retain current Unix readability and be covered by fixture tests.

### Protocol and DTOs

No CoreFrame protocol-version bump is expected.

Do not encode platform-local endpoint details into normal session/project protocol DTOs.

### Runtime and concurrency

Preserve:

- one connection reader owner;
- bounded per-connection writes/events;
- request correlation;
- projection forwarder ownership/teardown;
- accept-loop cancellation/join behavior;
- peer EOF/read-error observability.

Windows accept-loop implementation must maintain a pending pipe instance before handing off an accepted connection, following Tokio's documented named-pipe server pattern.

### Frontend or operator surface

No GUI changes.

Existing `--endpoint` / `CODEGG_CORE_ENDPOINT` behavior must become platform-aware and documented. Invalid schemes fail explicitly.

### Security and authorization

Windows named-pipe security is a closure-bearing requirement, not polish.

Required evidence includes:

- expected current-user connection succeeds;
- a deliberately broader/untrusted identity is not granted pipe access where the test environment can construct that principal;
- remote-network path is denied/not exposed;
- default descriptor fallback cannot occur;
- endpoint names do not contain secrets;
- daemon authorization still runs after transport admission.

If meaningful cross-user negative testing cannot run in available CI, require a reproducible manual/live Windows qualification record before closure and classify the missing automated evidence honestly.

### Documentation and static guards

Update:

- `architecture/core.md`
- `architecture/client.md`
- `architecture/protocol.md`
- `architecture/security.md` if local transport trust description changes
- `RELEASING.md` only to clarify compatibility, not support-tier promotion
- cross-platform validation docs

Add guards preventing new generic code from directly naming `UnixStream`/Unix socket paths outside the local transport adapter.

## 7. Ordered work packages

### Work package A — Endpoint and stream abstraction

Intent:

Separate CoreFrame semantics from the OS primitive.

Required changes:

- typed endpoint parse/format;
- internal async stream/read-write abstraction;
- migrate Unix client/server through it;
- preserve current Unix tests before adding Windows.

Acceptance evidence:

- Unix behavior is unchanged;
- no duplicated request/frame state machine;
- invalid endpoint schemes fail explicitly.

### Work package B — Standard singleton lock

Intent:

Remove a now-unnecessary Unix-only primitive and establish portable lock semantics at the current MSRV.

Required changes:

- migrate lock acquisition/probe to `File::try_lock`;
- map WouldBlock/error exactly;
- retain lifetime ownership;
- remove obsolete libc flock code if no other owner needs it.

Acceptance evidence:

- singleton contention fixtures pass on Unix;
- Windows compile/runtime lock fixture passes;
- lock-held metadata semantics remain unchanged.

### Work package C — Windows named-pipe transport

Intent:

Add platform-local Windows CoreFrame transport without broadening trust.

Required changes:

- listener and client;
- byte-mode split read/write;
- accept-loop instance management;
- restrictive DACL/local-only policy;
- endpoint naming/path resolution;
- connection cleanup.

Acceptance evidence:

- live request/response/event fixture on Windows;
- concurrent clients;
- peer-death pending-request cleanup;
- reconnect;
- DACL inspection/negative evidence.

### Work package D — Windows daemon lifecycle

Intent:

Make connect-or-start semantics real, not compile-only.

Required changes:

- user-scoped runtime root;
- lock/metadata lifecycle;
- daemon child launch and early-exit handling;
- singleton startup race;
- stale metadata behavior.

Acceptance evidence:

- no-daemon autostart;
- reuse running daemon;
- two starters converge;
- frontend exit does not kill daemon;
- daemon stop/replace behavior is deterministic.

### Work package E — Cross-platform convergence and docs

Intent:

Prove one protocol implementation spans platforms.

Required changes:

- shared fixtures;
- static ownership guard;
- docs and support-tier wording.

Acceptance evidence:

- Linux/macOS regressions green;
- Windows live evidence recorded;
- no docs claim Windows installer/support tier from this milestone alone.

## 8. Failure, cancellation, restart, and contention semantics

- Lock contention returns "already owned" without blocking indefinitely.
- Pipe/socket endpoint creation failure does not delete another live endpoint.
- Windows ACL setup failure aborts listener startup.
- Connect retry handles the documented named-pipe busy/not-yet-ready cases within the existing bounded startup budget.
- Accepted connection task ownership matches existing daemon lifecycle semantics.
- Client disconnect tears down only that connection and its transient subscriptions/attachments.
- Daemon restart invalidates ephemeral transport client ids and PTY attachment ids as already specified.
- Concurrent daemon starters converge on the lock winner.
- Metadata remains diagnostic and may be stale; it never overrides the lock.

## 9. Compatibility and migration

Unix `unix://` endpoints remain accepted.

If a new platform-neutral endpoint serializer changes metadata, implement additive/backward-compatible decoding for the current Unix record.

Environment/config endpoint overrides remain supported with explicit scheme validation.

No database migration.

No network protocol change.

Windows support statements remain "best effort/compatibility" until the distribution/support roadmap says otherwise.

## 10. Required tests

### Focused unit tests

- endpoint parsing/formatting;
- Windows pipe-name sanitization;
- file-lock WouldBlock/error mapping;
- metadata legacy/current decode;
- DACL builder unit tests where possible.

### Integration tests

Unix:
- existing local socket request/event/reconnect suite.

Windows:
- named-pipe handshake;
- request/event;
- concurrent clients;
- reconnect;
- peer death with pending request;
- listener restart;
- daemon identity probe.

### Restart and recovery tests

- two concurrent daemon starters;
- stale metadata with no lock;
- held lock/unreachable endpoint;
- child early exit;
- daemon restart and new generation.

### Contention and cancellation tests

- lock contention;
- pipe busy/instance pressure;
- connection churn;
- client cancellation during request;
- accept-loop shutdown.

### Security and negative tests

- explicit restrictive pipe ACL;
- anonymous/Everyone broad access absent;
- remote-client access rejected/not exposed;
- invalid/malicious pipe names rejected;
- endpoint name does not authorize a request;
- transport admission does not bypass daemon capability gate.

### Migration and compatibility tests

- current Unix endpoint URI unchanged;
- old Unix daemon metadata decodes if metadata shape changes;
- no protocol-version bump;
- root Linux/macOS install/release behavior unchanged.

## 11. Required verification commands

Examples; record exact commands actually run.

```bash
cargo test -p codegg-client
cargo test -p codegg --lib core
cargo test -p codegg --test projection_transport_real --features server
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
scripts/verify.sh quick

# Cross-compile/compile evidence when target toolchain exists:
cargo check --target x86_64-pc-windows-msvc -p codegg-client
cargo check --target x86_64-pc-windows-msvc -p codegg

# Live Windows commands must be recorded from a Windows host rather than
# represented by the cross-check above.
```

If desktop-specific Tauri work has already begun independently, do not make M002 depend on its Node/Tauri toolchain.

## 12. Documentation updates

- `architecture/core.md`
- `architecture/client.md`
- `architecture/protocol.md`
- `architecture/security.md` if transport trust text changes
- `docs/validation/git-cross-platform.md` or a new local-IPC validation document where more appropriate
- support/release wording only as required for accuracy

## 13. Acceptance criteria

M002 is complete when:

1. generic CoreFrame client/server logic no longer depends directly on Unix stream types.
2. Unix local transport remains production-compatible.
3. singleton locking uses Rust 1.89 standard file locking or a documented equivalent with cross-platform semantics.
4. Windows uses a local named-pipe transport with explicit restrictive security rather than the default descriptor.
5. Windows connect/start/reuse/race/restart behavior is demonstrated on a live Windows host.
6. request/event/projection semantics remain one shared implementation.
7. no localhost TCP fallback or new network exposure was introduced.
8. no CoreFrame or storage migration was required unless explicitly documented.
9. Windows release/support-tier wording remains honest.
10. no unresolved high/medium transport/security finding remains.

## 14. Stop conditions

Stop and report if:

- M001 has not closed or its client boundary materially differs from the expected seam;
- the only feasible Windows implementation would accept the default broad named-pipe security descriptor;
- secure Windows pipe construction would require broad unsafe allowances rather than a narrow auditable boundary;
- portable transport changes would require altering projection ordering/replay semantics;
- an implementation proposes loopback TCP merely to avoid local IPC work;
- a support-tier/installer expansion is required to claim completion;
- live Windows evidence is unavailable: implementation may land, but closure must remain conditional/blocked rather than declaring the Windows path qualified.

## 15. Closure evidence required

The closure record must include:

- implementation commits;
- endpoint/stream ownership diagram;
- Unix regression evidence;
- exact Rust std locking behavior used;
- Windows named-pipe ACL design and inspection evidence;
- live Windows handshake/request/reconnect/singleton-race results;
- cross-platform compile results;
- protocol/storage/support-tier impact statement;
- unsafe-code inventory for Windows platform glue, if any;
- unresolved findings by severity;
- closure recommendation.

## 16. Handoff notes

Do not rewrite the mature projection WebSocket lifecycle while abstracting local IPC.

The Windows default named-pipe DACL is specifically disallowed by ADR-0010; this is a known security trap, not an optional hardening item.

Rust 1.89 already contains the file-lock primitive needed for the singleton, so adding a locking dependency requires evidence of a missing semantic.

M003 may proceed on Linux/macOS after M001 even while M002 is still being qualified. Keep interfaces stable enough for that parallelism.
