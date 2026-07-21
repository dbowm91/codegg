# Session Projections Milestone 007 — Corrective Transport Lifecycle and Evidence Closure

Status: ready for handoff

Repository baseline: `dbbaabdde51db09f0c5beb704234ce1d94d01c9a` (`main`)

Source subsystem:

- `plans/subsystems/session-projections-roadmap.md`

Corrected closure record:

- `plans/closure/session-projections/006-status.md`

Primary class: correctness / lifecycle cleanup / transport verification

## 1. Objective

Close the remaining production-lifecycle and verification gaps found after Milestone 006 without changing projection storage, replay authority, reducer semantics, disclosure policy, protocol DTO meaning, or the version-5 wire contract.

Milestone 006 correctly introduced bounded critical writer receipts, `Initializing -> Live` activation after successful control delivery, rollback on critical-send failure, real socket isolation tests, raw session filtering, and bounded legacy `/ws` output. Post-closure inspection found two concrete production issues and several acceptance-evidence gaps:

1. the Unix raw-event forwarder is spawned detached and is not cancelled or joined when its owning connection disconnects;
2. `/tui` raw events are filtered before queue insertion but carry no session-routing generation, so already queued events from the old session may drain after `SessionInfo` switches the connection to a new session;
3. real transport tests publish live events only after the client has consumed the initial response, so they do not exercise the response-blocked replay-to-live race;
4. queue saturation, writer close, cancellation, and disconnect rollback are tested at helper level but not against a staged daemon subscription through each production adapter;
5. real foreign-operation coverage is incomplete for acknowledgement, resume, status, and artifact operations;
6. reconnect/resume exact-range behavior and disconnect isolation are not proven through the real adapters;
7. the M006 closure record overstates portions of the transport evidence and records inconsistent test counts.

The milestone succeeds when all per-connection tasks are deterministically owned and cleaned up, raw session switching cannot deliver stale queued events, every critical-delivery failure is proven to remove both transport and daemon subscription state, and the closure record contains exact reproducible evidence matching the tests that exist.

## 2. Scope boundaries

### In scope

- Unix raw-event forwarder ownership, cancellation, joining, and shutdown cleanup.
- Connection-local raw-routing generation for `/tui` session changes.
- Stale queued raw-event rejection at the final writer boundary.
- Deterministic adapter-level critical-delivery failure injection.
- Real `/tui`, `/core`, and Unix response-before-live race tests.
- Real foreign ack, resume, unsubscribe, status, and artifact rejection tests where the operation exists on that transport.
- Real disconnect/reconnect and exact replay-to-live tests.
- Repeated saturation/disconnect leak checks.
- Reconciliation of protocol-version regression expectations and recorded test counts.
- Static regression guards for detached Unix connection tasks and generation-free TUI raw routing.
- Corrected M006/M007 closure and planning state.

### Explicitly out of scope

- New projection event or snapshot DTOs.
- Projection protocol version changes.
- SQLite schema, retention, checkpoint, or sequence changes.
- Replacing the replay service or event log.
- Removing version-4/raw compatibility.
- Team authorization, presence, chat, or organization policy.
- Cross-daemon replay replication.
- General TUI renderer or project-tab redesign.
- General server authentication redesign.

## 3. Required invariants

- Every task created for a transport connection has an owning handle or structured task scope.
- Connection teardown cancels and awaits all raw and projection forwarders before returning.
- No Unix raw forwarder retains a writer, filter set, or event receiver after its client disconnects.
- Cleanup remains idempotent under disconnect, server shutdown, writer failure, and concurrent subscription rollback.
- A `/tui` raw event is associated with the routing generation under which it was accepted.
- The writer drops a raw event whose generation no longer matches the connection’s current session generation.
- Changing `SessionInfo` increments the generation atomically with changing the session identity.
- A queued session-A event cannot be delivered after the connection commits to session B.
- Projection-primary mode continues to suppress raw mutation traffic.
- A staged projection forwarder cannot release before its initial snapshot/replay response writer receipt succeeds.
- Queue full, serialization failure, writer close, cancellation, timeout, or disconnect before activation removes connection ownership and daemon subscription state.
- A failed subscription establishment cannot be reported as successful and cannot leave a receiver or task alive.
- Foreign subscription operations fail closed using the daemon-issued connection identity.
- Reconnect from a valid cursor delivers exactly the missing committed range, then live events once, in order.
- Test names, counts, commands, and closure claims match the repository exactly.

## 4. Work package A — Unix connection task ownership

Primary files:

- `src/core/transport/daemon_socket.rs`
- `src/core/transport/daemon_socket_integration_tests.rs`

Required changes:

1. Retain the raw `forward_events` task handle in `handle_client`.
2. Give the raw forwarder a connection-scoped cancellation token.
3. Add cancellation to the `select!` around `event_rx.recv()` and any writer operation.
4. On every exit path from the client loop:
   - cancel the connection token;
   - cancel projection ownership;
   - await or abort-and-await the raw forwarder;
   - unsubscribe daemon projection subscriptions;
   - unregister the client;
   - release writer/filter state.
5. Avoid holding the filter lock or projection-state lock while awaiting socket I/O or joining tasks.
6. Treat daemon shutdown and peer EOF identically for cleanup purposes.
7. Add bounded diagnostics for abnormal forwarder termination without logging payload bodies.

Required tests:

- peer EOF stops the raw forwarder without waiting for another event;
- server shutdown stops the raw forwarder;
- repeated connect/subscribe/disconnect cycles return raw-forwarder counts to baseline;
- disconnecting client A does not stop or perturb client B;
- no writer/filter/event-receiver strong reference remains after teardown;
- raw and projection forwarders both terminate under concurrent disconnect.

## 5. Work package B — Epoch-safe `/tui` raw session switching

Primary file:

- `src/server/ws.rs`

Recommended model:

```text
TuiRawRouteState {
    session_id: Option<String>,
    generation: u64,
}

RawOutbound {
    generation: u64,
    message: WsMessage,
}
```

Equivalent types are acceptable, but the generation must be checked at the final delivery boundary rather than only when the event is admitted to the queue.

Required changes:

1. Add a monotonically increasing raw-routing generation to `TuiSessionState` or a dedicated route state.
2. Increment the generation whenever `SessionInfo` changes the selected session, including transitions to or from an empty/unknown session.
3. Capture the current generation when a raw event passes session/global filtering.
4. Preserve that generation in the raw outbound item.
5. Before writing a raw item, compare its generation with the current connection generation; discard stale items.
6. Ensure the check and session transition semantics are race-safe without holding state locks across WebSocket I/O.
7. Decide and document whether an unchanged `SessionInfo` value increments the generation. Prefer no increment for an identical normalized route unless doing so materially simplifies correctness.
8. Keep control and projection-live messages outside this raw generation mechanism.
9. Preserve the explicit global-event allowlist and projection-primary raw suppression.

Required tests:

- queue an A event, switch to B before writer release, and prove A is dropped;
- after switching to B, a B event is delivered;
- switching A -> B -> A cannot release stale events from either prior generation;
- identical `SessionInfo` behavior matches the documented policy;
- disallowed global events remain dropped across a session switch;
- allowed globals carry the generation policy explicitly chosen;
- projection-primary mode ignores raw queued mutation events even across a capability/session race.

## 6. Work package C — Deterministic adapter failure injection

Primary files:

- `src/core/transport/projection.rs`
- `src/core/transport/daemon_socket.rs`
- `src/server/ws.rs`
- transport test support modules

Introduce test-only or internal injection seams that can pause or fail these boundaries deterministically:

- after daemon subscription creation;
- after receiver installation;
- before control-channel enqueue;
- after enqueue but before writer receipt;
- during socket write/flush;
- immediately before `activate_after_delivery`.

The seam must not expose production-wide mutable global state. Prefer an adapter-owned test hook, injectable writer abstraction, or cfg-gated barrier/fault policy.

Required scenarios for `/tui` and `/core`:

- control queue remains full until critical timeout;
- writer closes after receiver installation and before receipt;
- connection cancellation wins while the writer receipt is pending;
- serialization failure occurs with an installed staged subscription where practical;
- disconnect occurs while activation is blocked;
- a live event is published while the initial response is blocked;
- duplicate completion/rollback race resolves to one terminal state.

Required scenarios for Unix socket:

- write or flush fails after receiver installation;
- peer disconnects before canonical response completion;
- cancellation and response completion race;
- event is published while the canonical response path is blocked.

Every scenario must assert:

- no `Live` lifecycle transition occurred on failure;
- the forwarder did not emit a live event before the response;
- connection ownership no longer contains the subscription;
- daemon subscription count returns to baseline;
- the receiver cannot be taken again;
- the forwarder task is terminated;
- cleanup is idempotent.

## 7. Work package D — Real response-before-live race proof

Primary files:

- `tests/projection_transport_real.rs`
- `src/core/transport/daemon_socket_integration_tests.rs`

The existing tests consume the initial response before publishing an event. Retain those normal-flow tests, but add race tests that publish while delivery is intentionally blocked.

For each transport:

1. initiate projection subscription;
2. pause the canonical response at a deterministic writer boundary;
3. wait until the daemon receiver is installed and the subscription is `Initializing`;
4. publish one or more projection events;
5. prove the client receives no live event while the response is blocked;
6. release response delivery;
7. prove the canonical response is the first relevant frame/line observed;
8. prove buffered live events follow exactly once and in sequence;
9. repeat with response failure and prove no live event is ever delivered.

Do not treat “the test published only after receiving the response” as race evidence.

## 8. Work package E — Complete real foreign-operation coverage

Use two independently connected clients with daemon-issued connection identities.

For `/core` and Unix CoreFrame transport, prove client B cannot operate on client A’s subscription through:

- `ProjectionAck`;
- `ProjectionResume`;
- `ProjectionUnsubscribe`;
- subscription status/snapshot queries where exposed;
- `ProjectionArtifactList` for an unowned project;
- `ProjectionArtifactRead` using A’s scope/handle.

For `/tui`, prove equivalent typed rejection through:

- `ProjectionAck`;
- `ProjectionResume`;
- `ProjectionUnsubscribe`;
- `ProjectionSubscriptionStatus`;
- artifact list/read request variants.

Assertions:

- the response is typed and fail-closed;
- A’s subscription remains live and unaffected;
- B acquires no project ownership as a side effect;
- no artifact bytes or metadata beyond a bounded rejection are disclosed;
- daemon last-acked cursor and subscription state are unchanged.

## 9. Work package F — Disconnect, reconnect, and exact replay-to-live coverage

For `/tui`, `/core`, and Unix transport:

1. subscribe and receive canonical state;
2. publish and acknowledge a known event range;
3. disconnect the client;
4. publish a bounded missing range while disconnected;
5. reconnect with a fresh connection identity;
6. resume from the prior persisted cursor;
7. assert the response identifies the real stream and new subscription identity;
8. assert exactly the missing committed range is replayed once and in order;
9. publish a live event during or immediately after replay handoff;
10. assert no gap, duplication, or reordering;
11. disconnect during replay and prove cleanup returns to baseline.

Also prove disconnecting one client does not disturb another client’s subscription, raw filter, or writer task.

## 10. Work package G — Resource and static regression guards

Extend or add guards so CI rejects:

- detached connection-scoped `tokio::spawn` calls in `daemon_socket.rs` whose handle is not owned or explicitly documented as daemon-global;
- Unix raw forwarders lacking a cancellation branch;
- `/tui` raw outbound messages without a route generation or equivalent final-boundary stale check;
- critical projection responses routed through best-effort `try_send`;
- server WebSocket `unbounded_channel` use;
- direct `mark_live` calls outside the approved activation helper;
- closure test-count claims that can be mechanically derived and are inconsistent where practical.

Static guards supplement but do not replace runtime tests.

## 11. Work package H — Verification hygiene and closure correction

1. Correct the M006 closure record to `conditionally closed` until this milestone passes.
2. Preserve the valid M006 implementation evidence:
   - bounded critical writer receipts;
   - activation-after-delivery state machine;
   - normal-flow socket isolation;
   - raw filter scoping;
   - bounded `/ws` output.
3. Explicitly list the post-M006 findings rather than rewriting historical implementation claims.
4. Reconcile the actual number of tests in `projection_transport_real` with the closure record.
5. Resolve or accurately classify the remote protocol version expectation failure; do not call it unrelated if it is caused by the M005/M006 protocol version change.
6. Record repository-wide failures separately from milestone-local failures.
7. Require exact command output or CI status for final strict closure.
8. Add `plans/closure/session-projections/007-status.md` only after implementation and verification complete.
9. Return the roadmap and registry to strict closed status only when M007 has no unresolved high or medium finding.

## 12. Required verification matrix

At minimum run:

```bash
cargo fmt -- --check
CARGO_BUILD_JOBS=1 cargo check --workspace --all-features
cargo clippy -p codegg-protocol --all-targets -- -D warnings
cargo test -p codegg-protocol --all-features -- --nocapture
cargo test -p codegg-core --all-features -- --nocapture
cargo test -p codegg --lib core::transport::projection --all-features -- --nocapture
cargo test -p codegg --lib server::ws --all-features -- --nocapture
cargo test -p codegg --lib core::transport::daemon_socket --all-features -- --nocapture
cargo test --test projection_transport_real --features server -- --nocapture
cargo test --test projection_replay_daemon_protocol -- --nocapture
cargo test --test projection_replay_subscription -- --nocapture
cargo test --test projection_replay_resume -- --nocapture
cargo test --test projection_replay_restart_recovery -- --nocapture
cargo test --test projection_replay_transport_isolation -- --nocapture
cargo test --test projection_disclosure_invariants -- --nocapture
cargo test --test projection_artifact_handles -- --nocapture
cargo test --test tui -- --nocapture
cargo test --test tui_render -- --nocapture
cargo test --test tui_project_routing -- --nocapture
cargo test --test tui_project_tabs -- --nocapture
cargo test --test single_daemon_lifecycle -- --nocapture
python3 scripts/check_projection_transport_isolation.py
python3 scripts/check_websocket_bounds.py
bash scripts/check-core-boundary.sh
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_scheduler_bypass.py
bash scripts/check_projection_disclosure.sh
git diff --check
```

Add focused commands for any new test module or guard.

## 13. Acceptance criteria

- Unix raw forwarding is connection-owned, cancellation-aware, and joined during teardown.
- Repeated Unix connect/disconnect cycles leave no raw-forwarder task or writer/filter reference leak.
- `/tui` session changes invalidate queued raw events from prior generations.
- A deterministic test proves a stale session-A event cannot drain after session B becomes current.
- `/tui`, `/core`, and Unix race tests publish while the canonical response is blocked and prove response-before-live ordering.
- Failure at every critical-delivery boundary rolls back both transport and daemon subscription state.
- Queue saturation, writer close, cancellation, serialization failure, and disconnect-during-install have adapter-level tests.
- Foreign ack, resume, unsubscribe, status, and artifact operations fail closed over real transports where supported.
- Disconnect/reconnect resumes exactly the missing committed range, then transitions to live without gaps or duplicates.
- Disconnecting one connection does not disturb another.
- The remote protocol version expectation regression is corrected or truthfully explained with a passing intended assertion.
- Closure test names and counts match the repository.
- Required focused suites and guards pass.
- No unresolved high or medium M007 finding remains.
- `plans/closure/session-projections/007-status.md` records exact implementation and closure commits.
- The registry contains no ready projection plan only after M007 is strictly closed.

## 14. Handoff order

Recommended execution order:

1. Work package A — Unix task ownership.
2. Work package B — TUI raw-route generations.
3. Work package C — deterministic fault seams.
4. Work package D — response-before-live race tests.
5. Work package E — foreign operations.
6. Work package F — reconnect/replay-to-live.
7. Work package G — guards.
8. Work package H — closure correction and final evidence.

Do not close the milestone after only adding tests or documentation. Production lifecycle fixes, deterministic failure coverage, and corrected closure evidence are all required.