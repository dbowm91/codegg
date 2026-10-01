# Desktop Frontend and IDE Foundation Milestone 001 — Shared Frontend Client Runtime

Status: implemented

Repository baseline: `53dea47f414641c3f9756c3f8f181f6208be8115`

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-001--shared-frontend-client-runtime`

Long-term requirements:

- `plans/000-long-term-specification.md#1-product-definition`
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Applicable ADRs:

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md`

Primary class: infrastructure

## 1. Objective

Create a reusable `crates/codegg-client` frontend runtime that owns native-protocol client identity, request/event transport, reconnect primitives, and frontend-safe singleton-daemon discovery/connect/start semantics, while preserving the current TUI production behavior and leaving daemon execution authority in the root/core implementation.

The milestone closes only when a non-TUI `ClientKind::Gui` test consumer can use the extracted client without importing `src/tui`, `src/server`, or daemon execution modules.

## 2. Why this milestone is ready

Hard dependencies are already closed:

- the singleton daemon and CoreFrame request/event transport exist;
- `codegg-protocol` is a standalone frontend-neutral wire crate;
- `ClientKind::Gui` and `ClientKind::Web` already exist;
- session projection/replay M012 is closed and the canonical `ProjectionClientController` is transport-neutral;
- TUI project/session convergence is closed and already routes durable operations through `CoreClient`.

No protocol-version, storage, scheduler, authorization, or projection semantic redesign is required.

## 3. Current implementation evidence

At the baseline:

- `src/core/mod.rs` owns `CoreClient`, `InprocCoreClient`, and root `AppError` coupling.
- `src/core/transport/socket.rs` owns the production `SocketCoreClient`.
- `SocketCoreClient::send_client_hello` hard-codes `client_name = "codegg-tui"`, `ClientKind::Tui`, and a fixed TUI capability struct.
- the socket client multiplexes responses with a pending-request map, owns one reader task, exposes a broadcast-backed event subscription, reconnects, and sends a default global subscription after `ServerHello`.
- `src/core/instance.rs` owns `DaemonPaths`, `ConnectOrStartOptions`, `ConnectOrStartOutcome`, `DaemonConnectError`, and `connect_or_start_daemon`. The options already support an explicit executable override suitable for an embedded/desktop launcher.
- `src/client/` is specifically a `/tui` WebSocket client; it must not be renamed into the new generic crate without separating that compatibility role.
- `crates/codegg-protocol/src/projection/controller.rs` already owns the canonical transport-neutral projection state machine. M001 must reuse it rather than move reducer semantics into the client crate.
- the TUI has presentation-specific projection/tab/artifact state under `src/tui/app/state/projection_client.rs`; that state is not part of this extraction.

## 4. Invariants that must not regress

- The daemon remains the only execution and durable-state authority.
- `codegg-protocol` remains the wire-format source of truth.
- Existing TUI requests, events, handshake semantics, global subscription behavior, and daemon autostart remain compatible.
- Client identity and capability fields are supplied by trusted frontend composition code, not inferred from UI payloads.
- A peer EOF or terminal transport error resolves every outstanding request with an error; no pending waiter may hang indefinitely.
- Reconnect establishes a fresh handshake/client identity.
- Frontend disconnect releases client-owned transport state but does not stop daemon-owned work.
- No client crate dependency may reach into TUI rendering, providers, tools, scheduler, agent runtime, storage, or daemon internals.
- Projection reduction remains owned by `ProjectionClientController`; this milestone must not create a second reducer.
- The default CLI/TUI build must not acquire any Tauri, Node, webview, or desktop dependency.

## 5. Scope

### In scope

- New workspace crate `crates/codegg-client`.
- A crate-owned client error model suitable for transport/protocol failures.
- A configurable frontend descriptor, including:
  - `client_name`;
  - `ClientKind`;
  - `ClientCapabilities`;
  - protocol version from `codegg-protocol`.
- Extraction/reuse of the local CoreFrame socket client.
- Request correlation, event subscription, handshake state, reconnect, and default/core subscription behavior.
- Frontend-safe local daemon path/endpoint discovery and connect-or-start orchestration needed by both TUI and later desktop.
- Explicit executable override retained for launchers.
- A root compatibility adapter where broad conversion of every `AppError`-returning TUI callsite would otherwise make the milestone unbounded.
- TUI composition updated to pass an explicit TUI frontend descriptor rather than relying on hard-coded transport identity.
- Independent non-TUI/GUI-kind test consumer.
- Ownership/dependency guards.
- Architecture documentation updates for the moved boundary.

### Explicitly out of scope

- Windows named pipes or Windows singleton qualification (M002).
- Tauri/React/TypeScript (M003).
- Remote `/core` WebSocket client implementation.
- `/tui` protocol changes.
- REST changes.
- Session/projection DTO changes unless a direct extraction defect requires an additive compatibility fix.
- TUI state-management refactor.
- Moving `InprocCoreClient` or daemon construction into `codegg-client`.
- Editor/document state.
- PTY/editor GUI.
- Changing CodeGG's root MSRV.

## 6. Required production changes

### Core/domain

Create `crates/codegg-client` as a leaf-side frontend crate. A likely internal decomposition is:

```text
crates/codegg-client/
  Cargo.toml
  src/
    lib.rs
    descriptor.rs
    error.rs
    local/
      mod.rs
      endpoint.rs
      connect.rs
      socket.rs
```

Exact filenames may differ, but responsibilities must remain separate enough that M002 can replace/extend only the local transport implementation.

Do not move daemon request dispatch, storage, authorization, scheduler, or runtime construction into this crate.

### Client descriptor

Replace the hard-coded hello with a caller-provided immutable descriptor. Provide convenience constructors only when they are truthful, for example a `tui()` constructor in the root composition layer or a generic builder in the client crate.

A GUI test must be able to advertise `ClientKind::Gui` and its own capability values and observe those values server-side.

No untrusted renderer message may supply the trusted descriptor after connection establishment.

### Error boundary

Define a bounded client error classification that can represent at least:

- connect/open failure;
- handshake timeout/version mismatch;
- serialization/protocol failure;
- peer closed;
- write/read failure;
- response waiter cancellation;
- daemon startup timeout;
- inconsistent singleton state;
- launched daemon exited early.

Root code may map this type into `AppError` through a narrow compatibility adapter. Do not make `codegg-client` depend on root `AppError`.

### Local daemon discovery/connect/start

Extract the frontend-consumable portions of `src/core/instance.rs` needed to:

1. resolve the canonical local endpoint;
2. attempt a verified CoreFrame connection;
3. optionally launch the existing `codegg daemon start` command;
4. wait for a complete handshake plus bounded `SnapshotDaemon` identity probe;
5. return a live client and daemon identity;
6. preserve the explicit executable override.

Daemon-only lock acquisition, metadata ownership, listener binding, and core construction may remain root-owned. If `DaemonPaths` is shared, establish one direction of dependency rather than copying path formulas into desktop/TUI.

### Transport

Preserve the existing JSONL CoreFrame framing and one-reader/many-pending-request semantics.

Make the local socket implementation consume a descriptor and avoid TUI names in generic code. Keep channel capacities bounded and preserve current peer-death cleanup behavior.

Expose only transport/client operations required by frontends. Avoid exporting raw write-half ownership or daemon internals.

### Projection seam

Depend on and re-export/use `codegg_protocol::projection::ProjectionClientController` where helpful, but do not fork it.

M001 does not need to invent a complete high-level projection service. It must, however, ensure the extracted client exposes the request/event primitives required for M004 to drive capability negotiation, subscribe/resume/ack/unsubscribe, and resync without reaching back into root/TUI modules.

### Storage and migrations

None.

### Protocol and DTOs

No wire-version bump is expected.

If tests reveal that server hello/capability registration has been implicitly assuming TUI, fix that assumption additively while preserving old clients. Do not add desktop-only CoreRequest variants in this milestone.

### Runtime and concurrency

- exactly one reader task owns the local stream read half;
- pending-request registration occurs before write;
- write failure removes the pending waiter;
- peer termination fails all outstanding waiters exactly once;
- reconnect cannot reuse the prior negotiated client id;
- event fan-out remains bounded;
- dropping one subscription receiver does not terminate the client;
- dropping the client terminates owned reader work without leaking tasks.

### Frontend or operator surface

No new GUI.

The existing TUI must select the extracted client through its normal daemon-client mode. Standalone inproc/stdio compatibility behavior remains available.

### Security and authorization

The client descriptor is descriptive capability negotiation, not authorization. Daemon operation authorization remains mandatory.

Do not move token/secret/provider state into the client crate.

### Documentation and static guards

Update:

- `architecture/client.md` to distinguish the reusable native client from the remote-TUI adapter;
- `architecture/core.md` and `architecture/overview.md` for the new crate boundary;
- `architecture/protocol.md` for configurable client identity;
- `architecture/tui.md` only where ownership moved.

Add a guard that prevents `codegg-client` from depending on the root package, `codegg-core`, `codegg-providers`, TUI/render crates, Axum/server code, or desktop/Tauri crates unless a later plan explicitly changes that boundary.

## 7. Ordered work packages

### Work package A — Dependency and API skeleton

Intent:

Establish the leaf dependency direction before moving code.

Required changes:

- add `crates/codegg-client` to the root workspace;
- depend on `codegg-protocol`, Tokio features required for the current local client, serde/json as needed, and minimal synchronization/error crates;
- define client descriptor and error types;
- add dependency-direction/static-guard coverage.

Acceptance evidence:

- crate compiles independently;
- dependency guard proves no core/TUI/provider/server dependency;
- descriptor round-trip/unit tests cover TUI and GUI identities.

### Work package B — Extract the production local CoreFrame client

Intent:

Make the proven socket client reusable without changing its wire semantics.

Required changes:

- move or re-home `SocketCoreClient` logic;
- parameterize hello identity/capabilities;
- preserve request multiplexing, event subscription, daemon-id handshake, and reconnect;
- keep root compatibility exports/adapters only where needed.

Acceptance evidence:

- existing socket peer-death/reconnect tests pass from the new crate;
- GUI descriptor is observed by a fake/server fixture;
- concurrent request IDs cannot cross-deliver responses.

### Work package C — Extract frontend daemon connect/start

Intent:

Let a later Tauri host reuse exactly the TUI's local daemon lifecycle.

Required changes:

- move/share endpoint/path discovery and verified connection loop;
- retain executable override;
- keep daemon lock/listener ownership root-side;
- remove duplicated frontend-side launch logic if any emerges during migration.

Acceptance evidence:

- existing-daemon reuse fixture;
- no-daemon autostart fixture;
- losing concurrent starter converges on singleton winner;
- bad child/timeout/inconsistent-lock failures remain typed/actionable.

### Work package D — Migrate the production TUI composition path

Intent:

Prove the extracted crate is not dead infrastructure.

Required changes:

- construct an explicit TUI client descriptor;
- route default daemon-client startup through `codegg-client`;
- preserve standalone modes;
- preserve public CLI behavior and help.

Acceptance evidence:

- local TUI daemon-client integration passes;
- daemon sees `ClientKind::Tui` with the same effective capabilities;
- no new TUI-specific branch is added to generic client code.

### Work package E — Independent GUI-kind consumer fixture and cleanup

Intent:

Prove a second frontend can use the boundary before Tauri is introduced.

Required changes:

- build a test-only/headless `ClientKind::Gui` consumer;
- connect, request daemon/project/session-safe data, subscribe to events, disconnect;
- update docs/guards;
- remove obsolete duplicate local-client ownership from root.

Acceptance evidence:

- GUI-kind fixture imports `codegg-client` + `codegg-protocol`, not TUI modules;
- TUI and GUI-kind clients can connect concurrently and receive distinct client IDs;
- dropping GUI fixture leaves TUI connection live.

## 8. Failure, cancellation, restart, and contention semantics

- Startup races converge on the existing authoritative singleton lock owner.
- A failed launch never steals/removes a live owner's lock or endpoint.
- Handshake timeout closes the candidate client and returns a typed error.
- Peer death wakes all pending request waiters.
- Reconnect obtains a fresh `ServerHello.client_id` and daemon identity; callers must re-negotiate frontend-owned subscriptions.
- Event lag remains diagnostic/typed according to existing transport behavior; M001 does not silently make queues unbounded.
- Client drop cancels client-owned reader/event tasks only.
- A GUI/TUI client disconnect does not mutate durable daemon work.

## 9. Compatibility and migration

The CoreFrame wire protocol remains unchanged.

The root may temporarily re-export the extracted client type so existing internal paths do not require a flag-day import migration. Re-exports must be documented as compatibility shims, not independent implementations.

`src/client/` keeps its existing remote-TUI meaning until a later plan explicitly introduces a remote native-protocol client.

No storage migration.

No configuration migration expected beyond moving local endpoint resolution behind the shared crate without changing current environment-variable behavior.

## 10. Required tests

### Focused unit tests

- descriptor construction for TUI/GUI/Web kinds;
- client error classification;
- endpoint normalization;
- request-id correlation;
- hello version/capability serialization.

### Integration tests

- fake/local CoreFrame server handshake;
- TUI descriptor behavior equivalence;
- GUI-kind descriptor reaches server registry;
- concurrent TUI + GUI clients;
- request/response multiplex under interleaved events;
- event subscriber drop;
- peer EOF with outstanding request;
- reconnect with fresh client id.

### Restart and recovery tests

- existing daemon reuse;
- autostart and verified readiness;
- child exits before readiness;
- singleton-start race converges on winner;
- daemon identity probe mismatch/failure is not treated as healthy.

### Contention and cancellation tests

- concurrent requests from one client;
- concurrent clients;
- client drop with in-flight request;
- event-channel lag stays bounded.

### Security and negative tests

- GUI descriptor cannot elevate operation authorization;
- protocol mismatch fails explicitly;
- malformed frame does not satisfy an unrelated pending request;
- client crate dependency guard.

### Migration and compatibility tests

- current TUI default daemon path/endpoint remains identical on supported Unix platforms;
- standalone inproc/stdio modes remain functional;
- existing CoreFrame fixtures remain decodable.

## 11. Required verification commands

The implementation agent must adapt exact package/test names to the final file layout and record only commands actually run.

```bash
cargo test -p codegg-client
cargo test -p codegg --lib core
cargo test -p codegg --test projection_transport_real --features server
python3 scripts/check_architecture_boundaries.py   # if existing guard is extended
cargo clippy -p codegg-client --all-targets -- -D warnings
cargo clippy -p codegg --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
scripts/verify.sh quick
```

If the repository has a more specific existing dependency guard, extend/use it instead of inventing a duplicate script.

## 12. Documentation updates

- `architecture/client.md`
- `architecture/core.md`
- `architecture/overview.md`
- `architecture/protocol.md`
- `architecture/tui.md` where ownership changes
- workspace crate/module maps

## 13. Acceptance criteria

M001 is complete when:

1. `crates/codegg-client` exists with the bounded dependency direction described above.
2. Generic local client code contains no hard-coded `codegg-tui` or `ClientKind::Tui`.
3. TUI production daemon-client startup uses the extracted implementation with an explicit TUI descriptor.
4. A GUI-kind headless fixture connects through the same implementation and receives a distinct daemon-assigned client id.
5. Request/event/reconnect/peer-death behavior is covered by focused tests.
6. Frontend daemon connect/start can be reused by a later Tauri host with an explicit `codegg` executable path.
7. No CoreFrame protocol version or storage schema change was introduced without explicit evidence.
8. Root default builds acquire no desktop dependency.
9. Documentation and ownership guards match the implementation.
10. No unresolved high/medium correctness or security finding remains.

## 14. Stop conditions

Stop and report instead of improvising if:

- extraction requires moving daemon execution/storage/authorization into the client crate;
- a protocol-version bump becomes necessary;
- maintaining TUI compatibility would require two independently evolving socket implementations;
- current path/lock ownership cannot be shared without creating a dependency cycle that changes core ownership;
- a root MSRV bump is proposed merely to simplify future Tauri work;
- the scope expands into Windows named-pipe implementation, Tauri, editor state, or remote `/core` networking.

## 15. Closure evidence required

The closure record must include:

- implementation commit(s);
- final dependency graph for `codegg-client`;
- before/after ownership map for `CoreClient`, local socket client, and connect/start;
- evidence that TUI and GUI-kind fixtures use the same extracted local implementation;
- exact focused test and Clippy results;
- peer-death/reconnect/concurrent-client evidence;
- protocol/storage/MSRV impact statement;
- documentation/guard updates;
- unresolved findings with severity;
- recommendation: closed, conditionally closed, corrective required, or blocked.

## 16. Handoff notes

Preserve unrelated active work in the large root package.

Prefer compatibility re-exports/adapters over a broad mechanical TUI error-type rewrite if both preserve one underlying client implementation.

Do not use M001 to clean up the entire `src/tui/app` state machine.

The M002 plan assumes M001 leaves a narrow local transport module that can be generalized without changing request/projection semantics.
