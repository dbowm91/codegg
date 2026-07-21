# Session Projections Milestone 006 — Atomic Control Delivery, Transport Verification, and Raw Compatibility Hardening

Status: closed

Repository baseline: `0ee134e9ddabf808a7a46310436b3c1342900fb2` (`main`)

Source subsystem:

- `plans/subsystems/session-projections-roadmap.md`

Related closure evidence:

- `plans/closure/session-projections/005-status.md`

Primary class: correctness / transport hardening / verification

## 1. Objective

Close the remaining failure-mode and evidence gaps in the remote projection transport without changing projection storage, replay authority, reducer semantics, disclosure policy, or the version-5 wire contract.

Milestone 005 correctly introduced connection-local subscription ownership, exact stream identity, typed cursor resume, bounded queues, lifecycle operations, and projection-private event filtering. Post-closure inspection found one remaining correctness risk: critical snapshot/replay/control frames are placed into bounded WebSocket queues with non-blocking `try_send`, while the subscription may still transition to live state if that control frame is dropped. A saturated control queue can therefore produce a client that never received its canonical initial state but subsequently receives live projection events.

This milestone also strengthens closure evidence by testing real Unix-socket and Axum WebSocket connections rather than relying primarily on replay-service receiver tests. It scopes raw compatibility traffic by connection/session and bounds the older `/ws` JSON-RPC channel so the server has no remaining unbounded WebSocket queue.

The milestone succeeds when:

- subscription establishment is atomic from the client’s perspective;
- a subscription cannot become live unless its snapshot or replay response was successfully written or durably accepted for writing by the owning connection;
- failed critical delivery cancels the receiver and unsubscribes daemon state;
- real two-client `/tui`, `/core`, and Unix-socket tests prove isolation, resume, cleanup, saturation, and foreign-operation rejection;
- raw compatibility traffic is scoped to the connection’s explicit session/filter selection;
- the legacy `/ws` endpoint is bounded or explicitly disabled;
- no storage schema, projection event shape, reducer contract, or replay authority is changed.

## 2. Why this pass is required

### 2.1 `/tui` critical control delivery is best-effort

`queue_tui` serializes a `TuiMessage` and calls `try_send` on a bounded channel. It returns `false` when the queue is full.

The projection subscribe path currently:

1. creates the daemon subscription;
2. installs the live receiver and forwarder;
3. calls `queue_tui` for `ProjectionSnapshot` without enforcing success;
4. marks the subscription live.

If the control queue is saturated at step 3, the snapshot is silently omitted while the forwarder becomes live. The client can then reduce event N+1 without state through N.

The same class of failure applies to replay and typed resync/control outcomes when queue failure is ignored.

### 2.2 `/core` can activate after a dropped response

The `/core` WebSocket request loop submits response frames with `try_send`. If the queue is full, it breaks out of the frame-send loop, but a later pass still marks subscriptions represented by those response frames live.

A dropped `ProjectionSubscribed` or `ProjectionReplay` response must never be followed by live delivery.

### 2.3 Control and live ordering is not represented as one atomic transition

The transport owner has `Initializing` and `Live` lifecycle states, but the transition is not tied to successful delivery of the initial snapshot/replay frame. The correct transition boundary is:

```text
receiver installed
    -> canonical control response accepted by writer path
        -> subscription marked live
            -> forwarder released
```

Any failure before the final step must roll back the transient subscription.

### 2.4 Current transport-isolation tests are mostly service-level

`tests/projection_replay_transport_isolation.rs` verifies independent replay-service receivers and unsubscribe cleanup. Those tests are useful, but they do not instantiate:

- two `/tui` WebSocket clients;
- two `/core` WebSocket clients;
- two Unix-socket clients;
- bounded control/live queue saturation;
- disconnect during receiver installation;
- real frame ordering across replay-to-live handoff.

Strict transport closure should include those production adapter paths.

### 2.5 Raw compatibility live delivery remains too broad

Projection-private events are now filtered correctly, but raw compatibility event tasks still consume the daemon-wide event broadcast.

- `/tui` converts every compatible non-projection event without applying the connection’s current session identity.
- `/core` replays using `CoreFrame::Subscribe` filters, but its live event task forwards every non-projection event regardless of the connection’s filter set.

This is tolerable only under a single fully trusted operator. It is incompatible with later observer/team use and can leak text deltas, tool output, permissions, or questions across sessions.

### 2.6 Legacy `/ws` still uses an unbounded queue

The older JSON-RPC WebSocket endpoint uses `mpsc::unbounded_channel`. Even though it is deprecated, a slow or hostile client can create unbounded memory growth. The endpoint must use a bounded queue with explicit overflow behavior or be disabled by default behind an explicit compatibility option.

## 3. Invariants

- Projection storage, sequence assignment, cursor validation, and replay authority remain daemon-owned.
- `ProjectionSubscriptionId` and `ProjectionStreamId` remain distinct typed identities.
- A subscription remains `Initializing` until its canonical initial control response is successfully accepted by the connection writer path.
- Live receiver forwarding cannot begin before successful snapshot/replay delivery.
- Failure to deliver a critical control frame causes deterministic cancellation and daemon unsubscribe.
- A failed or closed writer cannot leave an active transient subscription.
- Control-message failure is never represented as successful subscription establishment.
- Projection event queues remain bounded and overflow produces typed resync or connection termination, never silent continuation.
- Raw compatibility delivery is explicitly scoped by session/filter ownership.
- Projection-primary clients do not mutate canonical projection state from raw compatibility events.
- No connection receives another connection’s session-scoped raw traffic.
- Legacy `/ws` has a finite queue and finite per-connection work.
- Diagnostics never contain event payload bodies, artifact content, or secrets.
- Cancellation, disconnect, downgrade, and shutdown cleanup remain idempotent.

## 4. Scope

### In scope

- Reliable critical-control delivery for `/tui` and `/core`.
- Atomic `Initializing -> Live` transition tied to successful initial response delivery.
- Rollback/unsubscribe on serialization, queue, writer, timeout, or connection failure.
- Explicit critical versus best-effort outbound message classes.
- Bounded writer acknowledgements or equivalent delivery receipts.
- Real two-client `/tui`, `/core`, and Unix-socket integration tests.
- Queue-saturation, writer-close, and disconnect-during-install tests.
- Session/filter scoping for raw compatibility live events.
- Projection-primary suppression of raw session mutation events.
- Bounded or disabled-by-default legacy `/ws` queueing.
- Static guards for activation-before-control-delivery and unbounded WebSocket channels.
- Documentation, roadmap, registry, and closure evidence.

### Explicitly out of scope

- Changing projection DTOs or incrementing the projection protocol version.
- Replacing SQLite or replay storage.
- Changing retention, checkpoint, or sequence semantics.
- Removing version-4 compatibility variants.
- Final multi-user roles, organization policy, presence, or chat.
- Reworking the TUI renderer or tab architecture.
- Cross-daemon replication.
- General server authorization redesign.
- Replacing Axum or Tokio transport primitives.

## 5. Required architecture

### 5.1 Critical outbound delivery contract

Introduce one transport-neutral critical-send abstraction, for example:

```text
CriticalOutbound<T>
|-- bounded sender
|-- connection cancellation token
|-- bounded send timeout
|-- optional writer acknowledgement
`-- typed failure reason
```

A critical send must distinguish:

- serialization failure;
- channel closed;
- queue timeout/full;
- writer failure;
- connection cancellation;
- protocol incompatibility.

Snapshot, replay, resync, subscribe acknowledgement, and unsubscribe result messages are critical. Diagnostics and raw compatibility events may remain best-effort where explicitly documented.

Do not solve this by changing the bounded channels back to unbounded channels.

### 5.2 Atomic subscription activation

Create a single helper/state-machine operation used by `/tui`, `/core`, and where practical Unix socket:

```text
install_receiver_and_stage_subscription(...)
    -> Initializing owned subscription
    -> spawn blocked forwarder
    -> send critical snapshot/replay response
    -> on success: mark_live + release forwarder
    -> on failure: cancel + remove + daemon unsubscribe
```

Required properties:

- the forwarder cannot observe live-ready before critical response success;
- only one caller can complete or roll back activation;
- duplicate completion is harmless;
- connection cancellation races resolve to rollback;
- the daemon receiver is taken exactly once;
- rollback drops any pending receiver and durable transient subscription entry;
- the response’s descriptor/cursor exactly match the installed ownership state.

### 5.3 `/tui` critical response handling

Replace ignored `queue_tui` results for projection control messages with awaited bounded delivery.

At minimum treat these as critical:

- `ProjectionCapabilitiesAck` when enabling projection-primary mode;
- `ProjectionSnapshot`;
- `ProjectionReplay`;
- `ProjectionResync`;
- `ProjectionAckResult` when required for cursor progression;
- `ProjectionUnsubscribeResult`;
- artifact read/list outcomes carrying request IDs.

The implementation may prioritize a dedicated control queue, but saturation must either wait within a bounded timeout or close/rollback the connection. It must not silently drop the message and continue.

### 5.4 `/core` response ordering

The `/core` adapter must mark a projection subscription live only after the corresponding `CoreFrame::Response` is successfully delivered through the critical writer path.

Avoid a two-pass design where response enqueue failure is disconnected from later lifecycle activation. Return a typed per-frame send outcome or stage activation callbacks alongside response frames.

A practical model is:

```text
PendingCoreResponse {
    frame,
    on_delivered: optional subscription activation,
    on_failed: rollback action,
}
```

The exact type may differ, but lifecycle transitions must be coupled to delivery outcome.

### 5.5 Writer task and acknowledgements

If channel enqueue alone is insufficient to prove the frame reached the socket writer, add bounded one-shot acknowledgements from the writer task for critical frames.

Requirements:

- acknowledgement objects are bounded by the same outbound capacity;
- writer failure resolves all pending acknowledgements as failed;
- connection shutdown cannot strand waiters indefinitely;
- send timeout is explicit and tested;
- control traffic remains prioritized over projection-live and raw compatibility traffic;
- no lock is held across socket I/O.

### 5.6 Raw compatibility scoping

#### `/tui`

- Raw live delivery must use the connection’s selected session ID from `SessionInfo` or an explicit future-compatible filter.
- Session-scoped events for another session are dropped.
- Sessionless global events require an allowlist; they are not implicitly all forwarded.
- Projection-primary mode must ignore raw text/tool/session mutation events after negotiation, except explicitly documented compatibility diagnostics/control messages.
- Changing session identity must update the filter atomically and reject stale events using a generation/epoch if needed.

#### `/core`

- The live raw event task must consult the same connection-local `EventFilter` set used for replay.
- A `CoreFrame::Subscribe` filter must affect both replay and subsequent live events.
- No-filter connections receive no session-scoped raw events.
- Projection-primary subscriptions do not grant raw all-session visibility.

Prefer a shared `event_matches_filter` helper across Unix socket and `/core` rather than parallel semantics.

### 5.7 Legacy `/ws` bounding

Choose one documented policy:

1. replace the unbounded outbound channel with a bounded queue and terminate the connection on overflow; or
2. disable `/ws` by default and require an explicit compatibility configuration flag, while still bounding it when enabled.

Requirements:

- finite queue capacity;
- finite request concurrency;
- no silent response loss represented as success;
- rate-limit behavior remains bounded;
- documentation labels `/ws` deprecated and points clients to `/core` or `/tui`;
- a static guard rejects new `unbounded_channel` use in server WebSocket adapters.

## 6. Work packages

### A — Critical delivery primitive

- Define critical/best-effort outbound classes.
- Add bounded timeout/cancellation behavior.
- Add writer acknowledgement if needed.
- Add typed delivery failure diagnostics.

### B — Atomic `/tui` subscription establishment

- Stage owned receiver in `Initializing`.
- Deliver snapshot/replay/resync through the critical path.
- Mark live only after success.
- Roll back and unsubscribe on failure.
- Apply the same rules to resume-created subscriptions.

### C — Atomic `/core` response lifecycle

- Couple response delivery and subscription activation.
- Remove activation after failed `try_send`.
- Roll back on queue/writer failure.
- Preserve control priority and exact frame ordering.

### D — Raw compatibility filtering

- Add connection-local session/filter state to `/tui` raw forwarding.
- Reuse filter matching in `/core` live forwarding.
- Suppress raw mutation traffic in projection-primary mode.
- Add safe global-event allowlist behavior.

### E — Legacy `/ws` resource hardening

- Bound or disable the endpoint by default.
- Add finite request/output policy and overflow handling.
- Add deprecation documentation and static guard.

### F — Real transport integration tests

- Build test harnesses for real Axum WebSocket and Unix-socket connections.
- Exercise two-client isolation and foreign operations.
- Saturate queues and close writers at precise activation points.
- Verify reconnect/resume and replay-to-live ordering.

### G — Closure and planning hygiene

- Add M006 closure record with exact commits and commands.
- Update M005 closure with a post-closure follow-up note without rewriting historical evidence.
- Return roadmap/registry to closed only after all acceptance criteria pass.

## 7. Required tests

### Atomic control delivery

- Saturated `/tui` control queue prevents `ProjectionSnapshot` delivery and causes rollback/unsubscribe.
- Saturated `/tui` replay response queue does not release the live forwarder.
- Saturated `/core` response queue prevents `mark_live`.
- Writer failure after receiver installation but before snapshot write removes the owned subscription.
- Serialization failure rolls back without leaking receiver/task state.
- Critical-send timeout cancels and unsubscribes.
- Connection cancellation and successful send racing resolve to exactly one terminal outcome.
- No live event is observed before snapshot/replay response in the real socket byte stream.

### Real two-client isolation

For `/tui`, `/core`, and Unix socket independently:

- client A subscribes to session/project A;
- client B subscribes to session/project B;
- interleaved events are published;
- each client receives only its owned projection envelopes;
- stream and subscription IDs match the daemon response;
- client B cannot ack, resume, unsubscribe, query status, or read artifacts for A;
- disconnecting A does not disturb B;
- reconnecting A from its cursor receives exactly the missing range then live events.

### Raw compatibility scope

- `/tui` raw client bound to session A receives no session B text/tool/permission/question events.
- `/tui` projection-primary client receives no raw session mutation after negotiation.
- changing `SessionInfo` updates raw scope without delivering stale old-session events.
- `/core` live delivery honors the same filters as replay.
- `/core` with no `Subscribe` filter receives no session-scoped raw events.
- allowed global events are explicitly tested; disallowed global events are dropped.

### Queue and resource behavior

- control traffic remains deliverable under raw-event flood until the explicit bound/timeout is reached.
- projection-live overflow yields typed resync and stops the forwarder.
- raw queue overflow does not starve control traffic.
- no per-connection task or one-shot acknowledgement leak after repeated saturation/disconnect cycles.
- `/ws` queue capacity is enforced and overflow closes or rejects deterministically.
- static guard fails on `mpsc::unbounded_channel` in WebSocket server adapters.

### Regression

- all M1–M5 projection suites remain green;
- TUI project routing/restoration tests remain green;
- version-4 raw compatibility fixtures remain decodable;
- disclosure and artifact policy tests remain green;
- daemon singleton and restart recovery remain green.

## 8. Acceptance criteria

- No projection subscription becomes live before successful delivery of its canonical snapshot or replay response.
- Failed critical delivery always removes connection ownership and daemon subscription state.
- `/tui` and `/core` no longer ignore queue failure for critical projection control messages.
- Critical control delivery is bounded, cancellation-aware, and cannot wait indefinitely.
- Real socket tests prove response-before-live ordering.
- Real two-client tests exist for `/tui`, `/core`, and Unix socket.
- Foreign subscription and artifact operations fail on real transport connections.
- `/tui` raw compatibility is session-scoped.
- `/core` live raw traffic honors connection filters.
- Projection-primary clients do not consume raw session mutation traffic.
- `/ws` has no unbounded outbound queue and is clearly deprecated or disabled by default.
- A static guard prevents reintroduction of unbounded WebSocket channels and activation-before-delivery patterns where structurally detectable.
- No unresolved high or medium transport findings remain.
- The registry and roadmap return to strict closed status only after a dedicated M006 closure record is accepted.

## 9. Verification commands

At minimum:

```bash
cargo fmt -- --check
CARGO_BUILD_JOBS=1 cargo check --workspace --all-features
cargo clippy -p codegg-protocol --all-targets -- -D warnings
cargo test -p codegg-protocol
cargo test -p codegg-core
cargo test -p codegg --lib core::transport::projection -- --nocapture
cargo test -p codegg --lib daemon_socket_integration -- --nocapture
cargo test --test projection_replay_daemon_protocol -- --nocapture
cargo test --test projection_replay_subscription -- --nocapture
cargo test --test projection_replay_resume -- --nocapture
cargo test --test projection_replay_restart_recovery -- --nocapture
cargo test --test projection_replay_transport_isolation -- --nocapture
cargo test --test projection_disclosure_invariants -- --nocapture
cargo test --test projection_artifact_handles -- --nocapture
cargo test --test tui -- --nocapture
cargo test --test tui_render -- --nocapture
cargo test --test single_daemon_lifecycle -- --nocapture
python3 scripts/check_projection_transport_isolation.py
bash scripts/check-core-boundary.sh
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_scheduler_bypass.py
bash scripts/check_projection_disclosure.sh
```

Add dedicated suites, with stable names recorded in closure evidence, for:

- `/tui` atomic control delivery and two-client isolation;
- `/core` atomic response delivery and two-client isolation;
- Unix-socket response-before-live ordering;
- raw compatibility session/filter scoping;
- legacy `/ws` queue bounds;
- repeated saturation/disconnect resource soak.

The repository-wide clippy command should also be attempted. Pre-existing unrelated warnings may be recorded separately, but no new M006 warning is acceptable.

## 10. Closure evidence required

The M006 closure record must contain:

- exact implementation commit SHAs;
- the critical-send state machine and timeout policy;
- proof that `mark_live` occurs only after successful initial control delivery;
- failure-path matrix for queue full, writer close, cancellation, timeout, serialization error, and disconnect;
- real transport test matrix for Unix socket, `/core`, and `/tui`;
- two-client isolation and foreign-operation results;
- response-before-live byte/frame ordering evidence;
- raw compatibility filter matrix;
- `/ws` bounding/deprecation result;
- queue, task, acknowledgement, and subscription bounds;
- static guard results;
- exact test counts and commands;
- zero unresolved high or medium findings.

## 11. Handoff constraints

- Do not alter projection storage or sequence authority.
- Do not make queues unbounded to avoid delivery failure.
- Do not mark a subscription live based only on successful receiver installation.
- Do not synthesize successful control responses after queue or writer failure.
- Do not broaden raw compatibility visibility while adding filters.
- Do not remove version-4 compatibility in this pass.
- Reuse shared transport ownership and event-filter helpers rather than creating another parallel subsystem.
- Stop for an ADR only if implementation requires changing authoritative replay ordering, storage backend, or compatibility guarantees.
