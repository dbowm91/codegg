# Desktop Frontend and IDE Foundation Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/001-shared-frontend-client-runtime.md`

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-001--shared-frontend-client-runtime`

Repository baseline reviewed: `53dea47f414641c3f9756c3f8f181f6208be8115`

Implementation commits:

- `b5b3348` — extract the shared frontend client and route TUI connect/start through it.
- `0861c74` — qualify startup failure paths and register daemon-spawn ownership.
- `26d844e` — cover protocol mismatch, malformed frames, and subscriber cleanup.
- `deebc99` — remove canceled requests from the pending map and release their capacity permits.

## 1. Executive finding

M001 is complete. The native CoreFrame client, client identity, bounded request/event handling, reconnect, and frontend-safe singleton connect/start behavior now live in the leaf `codegg-client` crate. The production TUI uses that client with an explicit TUI descriptor. A headless GUI consumer proves the boundary works without TUI imports. Daemon execution, singleton lock acquisition, authorization, and storage remain root-owned.

The transport remains Unix-domain-socket based in this milestone. Portable Windows IPC and its ACL/runtime qualification belong to M002; this closure makes no Windows support claim.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Leaf reusable client crate and dependency boundary | `crates/codegg-client`; `scripts/check-client-boundary.sh`; workspace quick check | pass | The crate uses protocol and transport utilities only; it has no root, UI, provider, server, or desktop dependency. |
| Configurable frontend identity and capabilities | GUI `ClientHello` assertion; TUI production composition in `src/main.rs` | pass | The daemon assigns distinct client IDs to concurrent TUI and GUI clients. |
| Request correlation and malformed-frame isolation | `gui_consumer_uses_native_client_without_root_or_tui_imports` | pass | Responses arrive in reverse request order; an intervening malformed frame does not cross-deliver either response. |
| Bounded request/event fan-out and subscriber-drop behavior | `LocalSocketClient` request semaphore and bounded broadcast/mpsc channels; GUI fixture drops its event receiver and continues requesting | pass | Capacities are 1024 pending requests and 256 events per fan-out channel. |
| Peer-death cleanup and canceled request cleanup | `peer_death_releases_pending_request_with_error`; `cancelling_request_releases_its_id_for_retry` | pass | A canceled waiter removes its pending entry and releases its capacity permit. |
| Reconnect uses a fresh daemon-assigned client ID | `reconnect_negotiates_a_fresh_client_identity` | pass | The fixture assigns and observes a different ID on the second handshake. |
| Protocol mismatch fails explicitly | `protocol_version_mismatch_fails_the_handshake` | pass | Client connect returns a typed handshake error. |
| Verified existing-daemon reuse and startup race convergence | `connect_or_start_reuses_only_a_verified_existing_daemon`; `tests/single_daemon_lifecycle.rs` | pass | The integration suite verifies daemon identity, autostart, and concurrent starters converging on one daemon. |
| Bad-child and stale-endpoint failure behavior | `child_exit_before_readiness_returns_a_typed_startup_error`; `unresponsive_endpoint_does_not_consume_the_daemon_startup_budget` | pass | Early child exit remains typed/actionable; a stalled endpoint cannot consume the whole startup deadline. |
| Production TUI migration and compatibility | Root workspace quick check; all eight `single_daemon_lifecycle` integration tests | pass | Standalone/in-process behavior remains root-side; lifecycle and CLI composition compile in the default workspace. |
| No wire or storage migration | Protocol/schema diff and workspace check | pass | No CoreFrame version or storage schema change was introduced. |
| Required docs and ownership inventory | Architecture/client/core/protocol/TUI docs; `docs/execution-ownership.toml`; process-ownership architecture; quick guards | pass | The moved daemon launch site is explicitly classified. |

## 3. Production implementation evidence

`crates/codegg-client` owns the immutable `FrontendDescriptor`, typed client errors, Unix CoreFrame transport, request multiplexing, event subscriptions, reconnect, shared daemon path discovery, and connect-or-start probing. Startup still invokes only the canonical `codegg daemon start` entry point. It validates the handshake daemon ID against `SnapshotDaemon`, limits endpoint probes, rotates an oversized prior log, and returns typed child, timeout, and inconsistent-singleton errors.

`SocketCoreClient` is now a compatibility adapter over `LocalSocketClient`. The default daemon-client composition in `src/main.rs` passes explicit TUI identity and capabilities. Root daemon lock, metadata, listener, execution, and authorization ownership remain in their existing root-side modules.

## 4. Verification executed

### Commands run

```bash
CARGO_TARGET_DIR=/tmp/codegg-client-target CARGO_BUILD_JOBS=2 cargo test -p codegg-client --locked --offline
CARGO_TARGET_DIR=/tmp/codegg-client-target CARGO_BUILD_JOBS=2 cargo clippy -p codegg-client --all-targets --locked --offline -- -D warnings
CARGO_TARGET_DIR=/tmp/codegg-root-check CARGO_BUILD_JOBS=2 cargo test -p codegg --test single_daemon_lifecycle --locked --offline
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
scripts/check-client-boundary.sh
python3 scripts/check_execution_ownership.py
```

### Results

- Client tests passed: 1 unit test and 9 integration tests.
- Client Clippy passed with warnings denied.
- Daemon lifecycle integration tests passed: 8 passed, 0 failed. This includes autostart, concurrent startup convergence, stale socket recovery, daemon identity checks, and graceful stop/restart.
- `scripts/verify.sh quick` passed, including all static guards and `cargo check --workspace --all-targets --locked`.
- Formatting, whitespace, client-boundary, and execution-ownership checks passed.
- The root-only `cargo test -p codegg --lib core::transport::socket::tests` harness was attempted twice but interrupted after prolonged rustc filesystem waits. Its peer-death behavior is directly covered against the extracted production client in `codegg-client`, while the root adapter and all targets compile under quick verification. No test result is attributed to either interrupted root-harness attempt.
- The quick check reports one existing deprecation warning in `src/util/metrics.rs` for `AtomicU64::fetch_update`; it does not fail verification and is unrelated to M001.

## 5. Invariant review

- One reader task owns each local stream read half.
- Pending requests are registered before writes; duplicate IDs are rejected; cancellation, write failure, peer closure, and client drop release waiters and request permits.
- Requests and event channels remain bounded. A dropped event subscriber does not terminate the shared reader/client.
- Reconnect negotiates fresh client and daemon identity state.
- Client descriptors advertise frontend capabilities but do not grant operation authorization.
- No daemon execution, provider credentials, storage, or authorization state moved into `codegg-client`.
- Default builds acquire no desktop/Tauri dependency.
- CoreFrame protocol and storage schema remain unchanged.

## 6. Failure and recovery review

Existing daemon reuse requires a successful handshake and matching daemon identity probe. Autostart uses the canonical executable path and command, rotates an oversized prior log, and does not treat an unresponsive endpoint as healthy. Lifecycle tests prove concurrent starters converge on the singleton winner, stale socket recovery, graceful shutdown, and restart. Client tests prove typed early-child failure, protocol mismatch, peer-death cleanup, cancellation retry, and fresh identity after reconnect. GUI disconnect leaves a concurrent TUI client usable.

## 7. Migration and compatibility review

There is no storage migration or wire-version change. Existing endpoint environment variables and user-scoped daemon path conventions are retained through shared path resolution. The root `SocketCoreClient` remains as a compatibility adapter. Existing standalone/in-process modes remain root-owned. The portable transport migration is explicitly deferred to M002.

## 8. Security review

The frontend descriptor is descriptive capability negotiation only; daemon authorization remains authoritative. The new client crate has no provider-secret or daemon execution dependencies. It starts only the canonical daemon command without a shell, uses bounded request/event resources, writes daemon logs with restrictive Unix permissions, and registers its process-spawn site in the execution-ownership inventory. The dependency guard prevents the frontend client from depending on root/core/provider/server/UI/desktop layers.

## 9. Documentation and operations

Updated `architecture/client.md`, `architecture/core.md`, `architecture/native_crates.md`, `architecture/overview.md`, `architecture/protocol.md`, `architecture/tui.md`, and `architecture/process-tool-execution-ownership.md`. Updated the workspace member count and quick verification to include the new crate boundary guard. Added the process site to `docs/execution-ownership.toml`.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| None | No unresolved M001 correctness or security finding remains. | — | — |

The interrupted root unit harness is an evidence limitation with a passing equivalent peer-death regression in the extracted crate and passing root lifecycle/quick checks; it is not an unresolved correctness finding.

## 11. Roadmap disposition

M001 is closed. M002 portable IPC and M003 Tauri shell are dependency-ready. M002 is the next sequential implementation. M003 may proceed on Unix once selected, but M002 remains an operational prerequisite before Windows qualification. M004 remains blocked on M003. No M005/M006 IDE work is unblocked by M001.

## 12. Registry updates

- Move M001 from active/closing work to recently closed with this closure record and implementation commits.
- Mark M002 and M003 ready: both hard-depend on M001; M002's Windows live-runtime evidence remains a closure gate for Windows claims, and M002 is operational only for Windows qualification of M003.
- Keep M004 blocked on M003; retain M005/M006 as deferred.
