# Session Projections Milestone 006 Closure

Status: conditionally closed — Milestone 007 corrective lifecycle and evidence closure required

Implementation plan: `plans/implementation/session-projections/006-atomic-control-delivery-transport-verification-hardening.md`

Roadmap: `plans/subsystems/session-projections-roadmap.md`

Repository baseline: `0ee134e9ddabf808a7a46310436b3c1342900fb2`

Implementation commit: `8ca570fddc08eb9663b894f3190ae0ed0af2b98b`

Closure record introduced in commit: `270cc5f`

Corrective handoff:

- `plans/implementation/session-projections/007-corrective-transport-lifecycle-and-evidence-closure.md`

## 1. Closure decision

The principal M006 production changes remain accepted. Projection control delivery is staged through a bounded critical writer path, `Initializing -> Live` is tied to successful response delivery, failed critical sends invoke rollback, raw compatibility is materially better scoped, and the legacy `/ws` outbound queue is bounded.

Strict closure was invalidated by post-closure source and test inspection. Milestone 007 must close the remaining connection-task lifecycle, TUI raw-session generation, deterministic adapter-failure, foreign-operation, reconnect/resume, and evidence-accuracy gaps before the subsystem returns to strict closed status.

M007 does not reopen projection storage, sequence authority, reducer semantics, disclosure policy, replay authority, or the version-5 wire contract.

## 2. Post-closure findings

### 2.1 Unix raw forwarder is detached from connection teardown

`handle_client` spawns the Unix raw `forward_events` task without retaining its `JoinHandle` or giving it a connection-scoped cancellation branch. Projection subscriptions are cleaned up when the client loop exits, but the raw forwarder can remain blocked on the event receiver while retaining the writer and filter state until another event arrives or the daemon event bus closes.

Required correction:

- retain the task handle;
- add connection cancellation;
- cancel and await the task on EOF, failure, shutdown, and normal teardown;
- prove repeated connect/disconnect cycles return task/resource counts to baseline.

### 2.2 `/tui` raw session switching lacks stale-queue invalidation

The raw event task filters against the current `SessionInfo` value before queue insertion, but the queued item carries no route generation. An event accepted for session A may remain in the bounded raw queue and be written after the same connection has switched to session B.

Required correction:

- add a monotonically increasing raw-route generation;
- attach the generation to raw outbound items;
- reject stale generations at the final writer boundary;
- test A -> B and A -> B -> A switching with intentionally blocked queues.

### 2.3 Atomic failure evidence is mostly helper-level

The critical-send helper has typed tests for cancellation, queue closure, writer failure, serialization failure, and timeout. The real transport tests, however, normally wait until the client has already consumed the canonical response before publishing a live event. They therefore do not exercise the response-blocked replay-to-live race that M006 was created to close.

Required correction:

- pause each production adapter after receiver installation and before response completion;
- publish while the response is blocked;
- prove no live event is emitted before the response;
- inject queue, writer, timeout, cancellation, serialization, and disconnect failures against staged daemon subscriptions;
- prove connection and daemon state return to baseline.

### 2.4 Real foreign-operation and reconnect coverage is incomplete

The real two-client tests cover isolation and foreign unsubscribe, but not the full planned matrix of foreign acknowledgement, resume, status, and artifact operations. They also do not prove disconnect/reconnect with exact missing-range replay followed by live delivery.

Required correction:

- add real fail-closed tests for all supported foreign operations;
- prove A remains unaffected by B’s rejected operations;
- reconnect with a new connection identity from a persisted cursor;
- prove exactly-once missing-range replay and gap-free replay-to-live handoff.

### 2.5 Closure evidence requires reconciliation

The closure record reports test counts and response-before-live proof more strongly than the visible test structure supports. It also classifies a remote protocol-version expectation failure as unrelated even though the expected version may need updating after the additive protocol work.

Required correction:

- reconcile exact test names and counts;
- correct the protocol-version expectation or document the intended compatibility assertion;
- distinguish milestone-local failures from repository-wide unrelated failures;
- require exact command output or CI evidence for final strict closure.

## 3. Accepted M006 requirement matrix

| Requirement | Accepted implementation | Remaining M007 evidence or lifecycle work |
|---|---|---|
| Bounded critical delivery | Shared timeout/cancellation helper, writer receipts, and typed Unix writes | Exercise failures through staged production subscriptions |
| Atomic `Initializing -> Live` | `activate_after_delivery` and ready-gated forwarders | Publish while response delivery is blocked and prove ordering |
| Rollback and unsubscribe | Adapter rollback paths remove transport ownership and request daemon unsubscribe | Prove queue/writer/cancellation/disconnect rollback end to end |
| `/tui` critical controls | Snapshot, replay, resync, ack, unsubscribe, status, and artifact outcomes use critical delivery where required | Add deterministic saturation and disconnect tests |
| `/core` response lifecycle | Response delivery is awaited before activation | Add blocked-response race and complete foreign-operation tests |
| Unix projection delivery | Critical response precedes ready release | Own and terminate the separate raw forwarder; add failure races |
| Real WebSocket isolation | Two-client `/core` and `/tui` projection isolation landed | Complete response-blocked, reconnect, and foreign-operation matrix |
| Raw `/tui` scope | Current-session filtering and projection-primary suppression landed | Add final-boundary route-generation rejection for queued stale events |
| Raw `/core` scope | Replay/live use shared filter semantics | Retain and regression-test |
| Legacy `/ws` bounds | Finite queue and deprecation documentation landed | Retain and regression-test |
| Static protection | Unbounded WebSocket and projection transport guards landed | Extend guards for detached Unix tasks and generation-free TUI raw routing |

## 4. Accepted critical delivery state machine

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

The shared critical timeout remains 500 ms. WebSocket critical messages carry a one-shot writer receipt. Unix critical frames use typed serialization/write/flush failures under the same bounded delivery helper. Milestone 007 must prove this state machine under blocked and failing production-adapter conditions.

## 5. Accepted bounds and compatibility

- WebSocket outbound queues: 256 messages per channel/connection.
- Projection subscriptions: 32 per connection; daemon maximum remains 256.
- Artifact reads: 8 concurrent reads per connection.
- Diagnostics: 32 retained diagnostics per connection.
- Critical delivery: 500 ms timeout, cancellation-aware, with bounded writer receipts.
- Legacy `/ws`: finite 256-message queue, deprecated, overflow closes the connection.
- Projection protocol: remains version 5; version-4/raw compatibility remains present.
- No storage migration, DTO-shape, sequence, reducer, replay-authority, or disclosure-policy change is required by M007.

## 6. Historical verification record

The following commands were recorded as passing during the original M006 closure pass. They are retained as historical implementation evidence, not as sufficient M007 closure evidence:

```text
rtk proxy cargo fmt -- --check
rtk proxy cargo check -p codegg --features server --lib
rtk proxy cargo check --workspace --all-features
rtk proxy cargo clippy -p codegg-protocol --all-targets -- -D warnings
rtk proxy cargo test -p codegg-protocol --all-features -- --nocapture
rtk proxy cargo test -p codegg-core --all-features -- --nocapture
rtk proxy cargo test -p codegg --lib core::transport::projection --all-features -- --nocapture
rtk proxy cargo test -p codegg --lib server::ws --all-features -- --nocapture
rtk proxy cargo test -p codegg --lib core::transport::daemon_socket --all-features -- --nocapture
rtk proxy cargo test --test projection_transport_real --features server -- --nocapture
rtk proxy cargo test --test projection_replay_daemon_protocol -- --nocapture
rtk proxy cargo test --test projection_replay_subscription -- --nocapture
rtk proxy cargo test --test projection_replay_resume -- --nocapture
rtk proxy cargo test --test projection_replay_restart_recovery -- --nocapture
rtk proxy cargo test --test projection_replay_transport_isolation -- --nocapture
rtk proxy cargo test --test projection_disclosure_invariants -- --nocapture
rtk proxy cargo test --test projection_artifact_handles -- --nocapture
rtk git diff --check
```

Historical static guards:

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

The original capped repository-wide run recorded 4,007 passing library tests and two failing assertions, plus three pre-existing `egglsp` clippy suggestions and the existing Tokio annotation baseline. M007 must rerun the relevant matrix and classify each residual accurately.

## 7. Planning disposition

- M006 is conditionally closed.
- Accepted M006 implementation remains the foundation for M007.
- M007 is the sole dependency-ready projection handoff.
- The session-projections roadmap is active until M007 closure is accepted.
- Strict closure requires a dedicated `plans/closure/session-projections/007-status.md` with exact implementation commit, closure commit, commands, test names/counts, failure classification, and zero unresolved high or medium M007 findings.
- Deferred UX, team/presence, plugin semantics, replication, and compatibility-window work remains deferred and unregistered.