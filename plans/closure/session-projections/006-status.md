# Session Projections Milestone 006 Closure

Status: closed

Implementation plan: `plans/implementation/session-projections/006-atomic-control-delivery-transport-verification-hardening.md`

Roadmap: `plans/subsystems/session-projections-roadmap.md`

Repository baseline: `0ee134e9ddabf808a7a46310436b3c1342900fb2`

Implementation commit: `8ca570fddc08eb9663b894f3190ae0ed0af2b98b`

Closure record introduced in commit: `270cc5f`

## 1. Closure decision

M006 is closed. Projection control delivery is now atomic at the transport
boundary: a connection remains `Initializing` until its canonical response has
passed the bounded critical writer path, and every failure path rolls back the
connection-owned receiver and daemon subscription. The implementation does not
change projection storage, sequence authority, reducer semantics, disclosure
policy, replay authority, or the version-5 wire contract.

## 2. Requirement and evidence matrix

| Requirement | Implementation | Evidence |
|---|---|---|
| Bounded critical delivery | Shared timeout/cancellation helper and typed failures in `src/core/transport/projection.rs`; writer receipts in `src/server/ws.rs`; typed Unix writes in `src/core/transport/daemon_socket.rs` | Projection unit tests: 9 passed; server/ws unit tests: 2 passed; daemon socket tests: 13 passed |
| Atomic `Initializing -> Live` | `activate_after_delivery` is the only ready transition; forwarders await the ready permit | `src/core/transport/projection.rs:265`; real `/core`, `/tui`, and Unix tests publish only after observing the initial response |
| Rollback and unsubscribe | `rollback_subscription` / adapter cleanup cancel, remove, abort, and issue daemon unsubscribe | Timeout, cancellation, closed-writer, serialization, queue-full, and disconnect paths covered by focused tests and adapter cleanup code |
| `/tui` critical projection controls | Snapshot, replay, resync, capability acknowledgement, acknowledgement, unsubscribe, status, and artifact outcomes use `critical_send` where delivery is required | `src/server/ws.rs:38`, `src/server/ws.rs:1189`; real TUI projection test passed |
| `/core` response lifecycle | Response delivery is awaited before `activate_after_delivery`; foreign operations are checked against the daemon-issued connection owner | `src/core/transport/daemon_socket.rs:257`; real CoreFrame projection test passed |
| Unix transport verification | Real newline-delimited Unix clients exercise projection response ordering, live delivery, isolation, and foreign unsubscribe rejection | `two_socket_projection_clients_are_ordered_and_isolated`: passed; daemon socket focused result: 13 passed |
| Real WebSocket verification | SQLite-backed Axum `/core` and `/tui` clients exercise two-client projection isolation, response-before-live, and foreign unsubscribe rejection | `projection_transport_real`: 10 passed |
| Raw `/tui` scope | SessionInfo is checked atomically with serialization/queueing; only the selected session and allowlisted globals pass; projection-primary suppresses raw mutation traffic | Real TUI raw isolation and primary suppression tests passed |
| Raw `/core` scope | Shared `event_matches_filter` is used for replay/live filtering; projection-private events never use raw broadcast | Unix filter/replay tests and real CoreFrame raw isolation tests passed |
| Legacy `/ws` bounds | Deprecated JSON-RPC endpoint uses a finite 256-message outbound queue and closes on overflow | `scripts/check_websocket_bounds.py`: passed; server documentation updated |
| Static regression protection | Projection transport and WebSocket bounds guards are wired into CI and documented in AGENTS.md | Both guards passed; core boundary, cwd, execution ownership, git policy, scheduler bypass, and disclosure guards passed |

## 3. Critical delivery state machine

```text
daemon subscription / receiver installed
    -> connection-owned Initializing entry
    -> control frame serialized
    -> bounded outbound send
    -> writer receipt (or bounded Unix write/flush)
    -> activate_after_delivery
    -> ready permit released
    -> projection forwarder may deliver live events
```

The shared critical timeout is 500 ms (`CRITICAL_DELIVERY_TIMEOUT`). WebSocket
critical messages carry a one-shot writer receipt. Unix-socket critical frames
use typed serialization/write/flush errors under the same bounded delivery
helper. A forwarder cannot pass its ready permit until activation succeeds.

## 4. Failure-path matrix

| Failure | Result | Cleanup |
|---|---|---|
| Serialization error | `Serialization` | No successful response; staged subscription is rolled back |
| Bounded queue full | `Timeout` after the 500 ms critical wait | Connection operation fails; staged subscription is rolled back |
| Queue closed | `QueueClosed` | Connection operation fails; staged subscription is rolled back |
| Writer close/error | `WriterClosed` | Pending receipt fails; connection cleanup unsubscribes daemon state |
| Connection cancellation | `Cancelled` | Critical wait ends immediately; owner and receiver are removed |
| Critical timeout | `Timeout` | No activation; receiver/forwarder and daemon subscription are removed |
| Disconnect during install | Writer/receipt failure or cancellation | Idempotent connection cleanup cancels forwarders and unsubscribes owned IDs |
| Invalid or duplicate activation | Typed lifecycle error | No second ready transition; caller rolls back the staged owner |

Best-effort raw and projection-live queue overflow remains bounded. Projection
lag stops the forwarder and attempts a typed resync; it does not advance the
acknowledged cursor silently.

## 5. Real transport matrix

| Transport | Real coverage | Result |
|---|---|---|
| Unix socket | Two projection clients on separate project streams; canonical response observed before publication; live event isolation; foreign unsubscribe rejected. Also session-filter, global-only, replay/live filter, and writer tests. | 13 focused tests passed |
| `/core` | Two projection clients; canonical `ProjectionSubscribed` responses precede publication; live project isolation; foreign unsubscribe rejected. Separate raw session A/B isolation test. | 2 projection + 1 raw test passed |
| `/tui` | Two projection clients; canonical snapshots precede publication; live project isolation; foreign unsubscribe rejected. Separate raw session A/B isolation and projection-primary suppression tests. | 2 projection + 2 raw tests passed |

The real harness reads each initial snapshot/response from the socket before it
publishes the test event. This is the byte/frame ordering proof: the live
projection event is only made available after the canonical response has been
observed by the client. The deterministic lifecycle tests additionally prove
that a forwarder cannot run before the ready permit.

## 6. Isolation and raw filter matrix

| Surface | Allowed | Rejected or suppressed |
|---|---|---|
| Unix projection | Client A receives project A; client B receives project B | B cannot unsubscribe A and receives no A event |
| `/core` projection | Client A/B receive only their owned project stream | Foreign unsubscribe returns `projection_subscription_not_owned`; foreign live event is absent |
| `/tui` projection | Client A/B receive only their owned project stream | Foreign unsubscribe result is rejected; foreign live event is absent |
| `/tui` raw | Session A receives A; session B receives B | Cross-session text/tool/session traffic is absent |
| `/tui` projection-primary | Typed projection channel only | Raw session mutation event is suppressed after negotiation |
| `/core` raw | Replay and live use the same connection-local filter | Session A/B cross-delivery is absent; global-only filters do not receive session events |

`event_matches_filter` is shared by the in-memory event log replay path and the
Unix/core live filtering paths. Projection-private `ProjectionStreamEvent`
payloads are excluded from raw broadcasts.

## 7. Bounds and compatibility

- WebSocket outbound queues: 256 messages per channel/connection.
- Projection subscriptions: 32 per connection; daemon maximum remains 256.
- Artifact reads: 8 concurrent reads per connection.
- Diagnostics: 32 retained diagnostics per connection.
- Critical delivery: 500 ms timeout, cancellation-aware, with bounded writer receipts.
- Legacy `/ws`: finite 256-message queue, deprecated, overflow closes the connection.
- Projection protocol: remains version 5; version-4/raw compatibility remains present.
- No storage migration, DTO shape, sequence, reducer, replay authority, or disclosure-policy change.

## 8. Verification record

Passed focused commands:

```text
rtk proxy cargo fmt -- --check
rtk proxy cargo check -p codegg --features server --lib
rtk proxy cargo check --workspace --all-features
rtk proxy cargo clippy -p codegg-protocol --all-targets -- -D warnings
rtk proxy cargo test -p codegg-protocol --all-features -- --nocapture  # 157 passed
rtk proxy cargo test -p codegg-core --all-features -- --nocapture      # 273 passed plus 26 integration tests
rtk proxy cargo test -p codegg --lib core::transport::projection --all-features -- --nocapture  # 9 passed
rtk proxy cargo test -p codegg --lib server::ws --all-features -- --nocapture                 # 2 passed
rtk proxy cargo test -p codegg --lib core::transport::daemon_socket --all-features -- --nocapture # 13 passed
rtk proxy cargo test --test projection_transport_real --features server -- --nocapture          # 10 passed
rtk proxy cargo test --test projection_replay_daemon_protocol -- --nocapture                    # 13 passed
rtk proxy cargo test --test projection_replay_subscription -- --nocapture                       # 13 passed
rtk proxy cargo test --test projection_replay_resume -- --nocapture                              # 9 passed
rtk proxy cargo test --test projection_replay_restart_recovery -- --nocapture                    # 8 passed
rtk proxy cargo test --test projection_replay_transport_isolation -- --nocapture                 # 7 passed
rtk proxy cargo test --test projection_disclosure_invariants -- --nocapture                      # 16 passed
rtk proxy cargo test --test projection_artifact_handles -- --nocapture                           # 13 passed
rtk git diff --check
```

Passed static guards:

```text
rtk python3 scripts/check_projection_transport_isolation.py
rtk python3 scripts/check_websocket_bounds.py
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_git_forbidden_patterns.py
rtk python3 scripts/check_scheduler_bypass.py
rtk bash scripts/check_projection_disclosure.sh
```

The capped repository-wide command reached 4,009 library tests: 4,007 passed
and two unrelated, unchanged assertions failed (`python_script` OS filesystem
isolation and the remote protocol version expectation). Workspace clippy still
reports only the three pre-existing `question_mark` suggestions in
`crates/egglsp/src/edit.rs`. The Tokio flavor guard still reports its existing
repository-wide baseline of 850 bare test annotations. None of these are an
M006 transport finding; no high or medium M006 findings remain.

## 9. Planning disposition

The implementation plan is marked `closed`. The M005 closure remains historical
and now contains a post-closure follow-up note; its claims were not rewritten
or reopened. The session-projections roadmap and registry are strict closed.
No future implementation plan is waiting on M006, so no future plan required
unblocking or a status change. Deferred UX, team/presence, plugin semantics,
replication, and compatibility-window work remains explicitly deferred and
unregistered.
