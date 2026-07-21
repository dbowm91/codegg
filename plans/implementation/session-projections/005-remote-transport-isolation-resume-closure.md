# Session Projections Milestone 005 — Remote Transport Isolation, Resume, and Compatibility Closure

Status: ready for handoff

Repository baseline: `bdc2138b7923592d08057485341d4168d504eb14` (`main`; subsequent planning-only commits do not alter production behavior)

Source subsystem:

- `plans/subsystems/session-projections-roadmap.md`

Closure evidence requiring correction:

- `plans/closure/session-projections/004-status.md`
- `plans/closure/session-projections/002-status.md`

Primary class: correctness / security / transport closure

## 1. Objective

Correct the remote projection transport so every live envelope, replay batch, acknowledgement, resync response, artifact read, and cleanup action is owned by exactly one authenticated connection and one daemon-issued projection subscription.

Milestones 001–004 established the projection DTOs, durable replay store, publication/redaction policy, controller, and frontend state. The remaining defect is in transport integration: the Unix daemon socket has a per-subscription receiver path, while the `/tui` and `/core` WebSocket adapters still depend on daemon-wide raw event broadcasts and do not retain enough connection-local projection ownership to implement live delivery, resume, unsubscribe, or disconnect cleanup correctly.

This corrective milestone succeeds when:

- a projection event can reach only the connection that owns its `ProjectionSubscriptionId`;
- remote reconnect resumes from the client-held `ProjectionCursor` or receives an explicit typed resync;
- no transport fabricates stream identity or subscription identity;
- disconnect, unsubscribe, resubscribe, capability renegotiation, lag, and shutdown have bounded deterministic cleanup;
- raw compatibility traffic remains available to older clients but cannot mutate projection-primary state or carry subscription-private projection envelopes;
- the M4 closure record and planning registry accurately describe the resulting production behavior.

## 2. Why this pass is required

Post-closure inspection at baseline `bdc2138` found the following closure-blocking discrepancies.

### 2.1 `/tui` WebSocket has no projection ownership state

`TuiSessionState` stores only:

- optional session ID;
- optional model;
- rate-limit key.

It stores no negotiated projection mode, subscription IDs, stream IDs, cursors, receiver tasks, reconnect generation, or artifact-read ownership.

### 2.2 `/tui` live delivery is wired to the daemon-wide raw broadcast

`upgrade_tui` calls `daemon.subscribe()` once per connection and passes every convertible `CoreEvent` through `convert_core_event_to_tui`. That converter accepts any `CoreEvent::ProjectionStreamEvent` and emits a `TuiMessage::ProjectionEvent` without checking whether the current WebSocket owns the subscription.

The subscribe handler forwards `CoreRequest::ProjectionSubscribe`, but it does not call `take_subscription_receiver`, retain the daemon-issued subscription ID, or spawn an owned per-subscription forwarder. Therefore the adapter either receives no live projection envelopes from the proper subscription receiver or can forward projection events from an unrelated daemon-wide source. Neither behavior satisfies the projection contract.

### 2.3 Remote resume is not projection resume

The existing `TuiMessage::Resume` replays the legacy raw `EventLog` by global `event_seq` and then sends a generic `ResyncRequired`. It does not send `CoreRequest::ProjectionResume` with a stream-scoped `ProjectionCursor`.

The M4 closure explicitly records missing `requested_resume_from_seq` server-side plumbing. Durable projection reconnect is therefore not end-to-end over the remote TUI transport.

### 2.4 Subscription response handling loses canonical identity

The `/tui` subscribe handler:

- ignores the cursor and retention floor returned by `ProjectionSubscribed`;
- does not retain the descriptor for live delivery;
- synthesizes a `ProjectionSubscriptionId` from `batch.descriptor.stream_id` when it receives `ProjectionReplay`;
- converts `ProjectionResyncRequired` into an untyped generic error instead of `TuiMessage::ProjectionResync`.

A stream ID and subscription ID are distinct opaque identities and must never be substituted for one another.

### 2.5 `/core` WebSocket also lacks connection-local projection forwarding

`upgrade_core_ws` subscribes to the daemon-wide raw event broadcast and writes every event to every connected client. Its request loop does not take and own the projection receiver returned after `ProjectionSubscribe`, unlike `src/core/transport/daemon_socket.rs`.

Projection-private events must not be delivered through the generic broadcast path.

### 2.6 Unix daemon socket emits a synthetic stream ID

The Unix-socket `projection_forwarder` owns the correct per-subscription receiver, but constructs `ProjectionStreamId` from `ProjectionSubscriptionId`. The actual persisted stream ID is available in the `ProjectionSubscribed` descriptor and must be retained and forwarded unchanged.

### 2.7 Remote projection protocol is incomplete

`TuiMessage` currently carries capabilities, subscribe, snapshot, replay, resync, ack, and live event variants, but has no explicit projection resume, unsubscribe, subscription-status, or artifact-read request/response operations. M4’s local artifact cache exists, but the remote transport does not expose the complete bounded artifact-read lifecycle through the projection protocol.

### 2.8 Remote queues are unbounded

The `/tui` and `/core` WebSocket adapters use unbounded MPSC channels. Projection replay/live traffic must have bounded queues and explicit lag/resync behavior rather than unconstrained memory growth.

These are production correctness and isolation defects, not optional M5 UX work.

## 3. Invariants

- `ProjectionSubscriptionId`, `ProjectionStreamId`, `ProjectId`, `WorkspaceId`, `SessionId`, client ID, and transport connection ID remain distinct typed identities.
- Only the daemon-issued subscription receiver may produce live envelopes for that subscription.
- A connection may send acknowledgements, resume, unsubscribe, status, or artifact-read requests only for subscriptions/scopes it owns.
- Generic raw `CoreEvent` broadcast paths never carry `ProjectionStreamEvent` to unverified recipients.
- The actual persisted descriptor stream ID is preserved from subscribe/resume through live delivery.
- Projection cursor validation remains daemon-owned; transports do not reinterpret sequence numbers.
- Persistence precedes projection live delivery.
- Duplicate delivery may occur at reconnect boundaries, but the canonical reducer remains idempotent and no committed sequence is silently skipped.
- Expired, ahead, gapped, mismatched, rebound, incompatible, or lagged cursors produce typed resync behavior.
- Disconnect and shutdown remove transient subscriptions/forwarders but do not delete replay history.
- Raw compatibility is mode-isolated from projection-primary state.
- Connection queues, replay batches, pending writes, subscriptions, diagnostics, and artifact reads are bounded.
- No transport derives authorization from a client-supplied subscription ID alone.
- Secret-bearing or redacted content is not added to diagnostics.

## 4. Scope

### In scope

- Shared connection-local projection subscription ownership used by Unix socket, `/core`, and `/tui` transports.
- Exact descriptor and cursor retention after subscribe/resume.
- Per-subscription receiver ownership and forwarder lifecycle.
- Projection resume, unsubscribe, status, and artifact-read/list operations over remote TUI protocol.
- Typed replay/resync/ack/unsubscribe/artifact outcomes.
- Bounded WebSocket queues and explicit lag behavior.
- Raw-event filtering that excludes subscription-private projection envelopes.
- Capability renegotiation and reconnect generation handling.
- Disconnect/shutdown cleanup and daemon unsubscribe calls.
- Correct stream IDs in Unix-socket projection events.
- Compatibility diagnostics for legacy remote channels.
- Transport-level isolation, replay, restart, lag, and compatibility tests.
- Documentation, closure correction, and registry cleanup.

### Explicitly out of scope

- Final team roles, organization policy, presence, or chat.
- Cross-tab artifact hand-off UX.
- Numeric hot-key acknowledgement UX.
- Plugin-specific `ProjectionEvent::PluginUi` semantics.
- Removing legacy `RenderFrame`, `StateSnapshot`, or raw-core variants in the same release.
- Cross-daemon replay replication or deployment-global ordering.
- Replacing SQLite, the projection store, or the canonical reducer.
- Expanding artifact reads beyond the existing bounded policy.

## 5. Required architecture

### 5.1 Shared connection projection state

Introduce a transport-neutral connection owner, for example:

```text
ProjectionConnectionState
|-- trusted connection/client identity
|-- negotiated projection version and mode
|-- reconnect generation
|-- bounded map<ProjectionSubscriptionId, OwnedProjectionSubscription>
|-- bounded artifact-read tasks
|-- bounded diagnostics
`-- cancellation token

OwnedProjectionSubscription
|-- subscription_id
|-- descriptor with real stream_id
|-- latest delivered/acked cursor
|-- receiver-forwarder task
|-- scope/project/session identity
|-- generation
`-- lifecycle state
```

The owner must be connection-local. It may wrap existing daemon services, but must not duplicate subscription storage or sequence authority.

Use one implementation seam across transports so `/core`, `/tui`, and Unix-socket behavior cannot drift.

### 5.2 Subscribe and live receiver ownership

On `ProjectionSubscribed`:

1. validate the response variant and descriptor;
2. retain the returned cursor and retention floor;
3. call the existing replay service `take_subscription_receiver(subscription_id)` exactly once;
4. spawn one bounded forwarder carrying the real descriptor stream ID;
5. insert the owned subscription into the connection registry;
6. reject duplicate IDs, capacity overflow, or missing receivers with typed cleanup and diagnostics;
7. send the initial snapshot and cursor metadata only after ownership is installed.

If installation fails after the daemon created the subscription, issue `ProjectionUnsubscribe` before returning failure.

No generic daemon broadcast task may be used as a substitute for the owned receiver.

### 5.3 Live event delivery

Each forwarder receives `ProjectionEnvelope` from its owned bounded receiver and emits:

- exact `subscription_id`;
- exact persisted `descriptor.stream_id`;
- envelope cursor/event sequence;
- reconnect generation where the transport needs stale-frame rejection.

The raw event forwarder must explicitly exclude `CoreEvent::ProjectionStreamEvent`. Add a static/structural guard preventing future transport code from forwarding projection stream events through daemon-wide broadcast paths.

### 5.4 Remote resume

Add an explicit remote operation carrying `ProjectionCursor`, not raw core `event_seq`:

```text
ProjectionResume {
    cursor,
    include_snapshot_if_resync,
}
```

The server must dispatch `CoreRequest::ProjectionResume` and return exactly one of:

- replay batch using the canonical descriptor/stream ID;
- typed resync with reason and optional bounded snapshot;
- typed error.

When resume establishes a new live subscription, the response must contain or resolve the daemon-issued subscription ID and install its receiver before live delivery. Do not derive a subscription ID from a stream ID.

If the existing core resume response cannot associate replay with a newly owned live subscription, make an additive protocol correction rather than guessing identity. Preserve compatibility through capability/version negotiation.

### 5.5 Unsubscribe and cleanup

Add explicit remote unsubscribe and acknowledgement responses.

Unsubscribe must:

- verify connection ownership;
- dispatch daemon `ProjectionUnsubscribe`;
- cancel and await/abort the forwarder;
- remove local ownership state;
- reject repeated or foreign IDs without affecting another connection.

On disconnect, capability downgrade, reconnect replacement, or server shutdown, drain all owned subscriptions through bounded cleanup. Ensure cleanup is idempotent if the socket disappears mid-request.

### 5.6 `/core` WebSocket integration

Refactor `upgrade_core_ws` so projection requests are intercepted after daemon response in the same way as the Unix socket:

- retain per-connection projection state;
- take receivers after `ProjectionSubscribed`;
- forward live projection frames only from owned receivers;
- filter projection-private events from the generic event broadcast;
- clean up subscriptions on disconnect.

Raw `CoreFrame::Subscribe` filters remain for legacy raw events and must not grant projection stream access.

### 5.7 `/tui` protocol and compatibility

Bump `REMOTE_TUI_PROTOCOL_VERSION` additively and add typed variants for:

- projection resume request;
- unsubscribe request/result;
- typed subscription/replay/resync errors;
- optional subscription status;
- bounded artifact list/read request and outcome;
- deprecation diagnostic for raw compatibility channels.

Projection-primary clients ignore raw `RenderFrame`, `StateSnapshot`, and raw session event messages. Version-4 clients remain on documented bounded compatibility behavior. Do not remove legacy variants in this pass.

Correct protocol documentation counts and variant lists.

### 5.8 Bounded queues and lag

Replace unbounded projection-facing channels with bounded channels. Define explicit limits for:

- outbound WebSocket messages;
- per-subscription pending envelopes;
- replay batches in flight;
- concurrent artifact reads;
- subscriptions per connection;
- diagnostics.

On overflow or lag:

- stop live delivery for the affected subscription;
- issue typed `SubscriberLagged`/resync behavior when possible;
- clean up the old forwarder;
- never silently drop committed projection events and continue advancing acknowledgements.

Raw compatibility traffic may use a separate bounded policy, but cannot starve projection control messages.

### 5.9 Authorization and capability context

Bind connection identity and negotiated capabilities before accepting projection operations. The transport must pass the trusted server-derived access context into daemon policy evaluation.

At minimum prove:

- one authenticated connection cannot acknowledge, resume, unsubscribe, or read artifacts for another connection’s subscription;
- project/session scope checks still execute at the daemon boundary;
- client-supplied IDs are treated as locators, not authority.

This pass does not define final multi-user roles.

## 6. Work packages

### A — Shared transport ownership primitive

- Add connection-local projection registry and owned-subscription type.
- Retain descriptor, cursor, receiver task, and cancellation state.
- Add bounded capacity and deterministic cleanup.

### B — Unix socket stream identity correction

- Pass the real descriptor stream ID into `projection_forwarder`.
- Remove conversion from subscription ID to stream ID.
- Ensure generic raw forwarding cannot duplicate projection delivery.

### C — `/core` WebSocket integration

- Install owned receivers after subscribe/resume.
- Filter projection events from daemon-wide broadcast.
- Add disconnect/unsubscribe cleanup and isolation tests.

### D — `/tui` WebSocket integration

- Extend protocol for resume/unsubscribe/status/artifact operations.
- Install owned receivers and typed outcomes.
- Replace generic errors with typed resync/errors.
- Add bounded queues and reconnect generation handling.

### E — Compatibility and deprecation diagnostics

- Keep version-4/raw compatibility functional.
- Add explicit channel-deprecated diagnostics in projection-primary capable clients.
- Document removal criteria and minimum compatibility window.
- Do not remove legacy variants yet.

### F — Verification and strict corrective closure

- Add two-connection isolation tests for every transport.
- Add restart/resume/resync/lag/cancellation tests.
- Correct M4 closure findings, roadmap status, registry SHAs, and compatibility matrix.
- Strictly close M5 only after production transport evidence passes.

## 7. Required tests

### Identity and isolation

- connection A subscription event is never delivered to connection B;
- connection B cannot ack A’s subscription;
- connection B cannot resume or unsubscribe A’s subscription;
- project A stream cannot appear on project B connection;
- stream ID equals persisted descriptor stream ID and never subscription ID;
- duplicate subscription IDs fail closed;
- missing receiver after subscribe triggers daemon unsubscribe cleanup.

### Resume and replay

- reconnect from retained cursor returns exactly missing events in order;
- duplicate boundary event remains reducer-idempotent;
- expired cursor returns typed `HistoryExpired` resync;
- ahead, gap, stream mismatch, version mismatch, and binding-revision mismatch are typed;
- daemon restart preserves retained stream sequence and replay;
- resume installs live receiver before post-replay events can be lost;
- subscribe/replay race has no gap between replay high-water and live delivery.

### Lifecycle

- explicit unsubscribe removes receiver and daemon subscription;
- disconnect drains all owned subscriptions;
- repeated cleanup is idempotent;
- reconnect replacement cannot leave duplicate forwarders;
- capability downgrade cancels projection-primary subscriptions;
- server shutdown cancels forwarders without deleting replay history;
- lag/queue overflow transitions to resync and stops acknowledgements.

### Transport compatibility

- Unix socket, `/core`, and `/tui` produce equivalent projection envelopes;
- `/core` raw event subscription cannot receive projection-private envelopes;
- `/tui` raw compatibility mode still works for protocol-version-4 fixtures;
- projection-primary mode ignores legacy raw session mutations;
- typed resync is not encoded as a generic error;
- artifact read/list is bounded and ownership checked;
- unsupported older clients receive actionable capability behavior.

### Resource and security

- outbound queue bounds are enforced under replay bursts;
- subscription and artifact-read counts are capped;
- no payload bodies or secrets enter diagnostics;
- unauthorized artifact handle reads fail closed;
- no unbounded per-connection task growth in reconnect soak tests;
- static guard rejects daemon-wide forwarding of `ProjectionStreamEvent`.

### Regression

- projection reducer/controller equivalence fixtures remain green;
- M2 storage/replay/failpoint tests remain green;
- M3 disclosure/artifact tests remain green;
- TUI multi-project routing/restoration tests remain green;
- existing raw socket and remote TUI compatibility tests remain green.

## 8. Acceptance criteria

- All transports use connection-local projection receiver ownership.
- `/tui` and `/core` no longer depend on daemon-wide raw broadcasts for projection live delivery.
- Generic event forwarders explicitly exclude projection-private events.
- The real persisted stream ID is delivered unchanged on every transport.
- Remote resume uses `ProjectionCursor` and daemon `ProjectionResume` semantics end-to-end.
- Replay-to-live handoff has no loss window or duplicate forwarder.
- Typed resync reasons reach remote clients.
- Unsubscribe/disconnect/shutdown cleanup is complete and idempotent.
- Foreign subscription operations fail without information leakage.
- Projection queues and tasks are bounded with explicit lag behavior.
- Remote artifact reads are bounded and policy checked.
- Version-4/raw compatibility remains functional and visibly deprecated where appropriate.
- M4 closure documentation is corrected to conditional/history status and M5 has strict closure evidence.
- The session-projections roadmap can return to closed with no unresolved transport-isolation or resume findings.

## 9. Verification commands

At minimum:

```bash
cargo fmt -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
CARGO_BUILD_JOBS=1 cargo check --workspace --all-features
cargo test -p codegg-protocol
cargo test -p codegg-core
cargo test --test projection_replay_daemon_protocol
cargo test --test projection_replay_subscription
cargo test --test projection_replay_resume
cargo test --test projection_disclosure_invariants
cargo test --test projection_artifact_handles
cargo test --test session_projection_m4_controller
cargo test --test single_daemon_lifecycle
cargo test --test tui --test tui_render
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_scheduler_bypass.py
bash scripts/check-core-boundary.sh
bash scripts/check_projection_disclosure.sh
```

Add focused suites for:

- Unix-socket projection stream identity;
- `/core` two-client subscription isolation;
- `/tui` two-client subscription isolation;
- remote resume/replay-to-live handoff;
- unsubscribe/disconnect cleanup;
- bounded queue lag/resync;
- remote artifact ownership;
- protocol v4/v5 compatibility.

## 10. Closure evidence required

The closure record must include:

- exact production commits;
- a transport-by-transport ownership matrix;
- evidence that generic broadcast paths exclude projection-private events;
- exact stream/subscription/cursor identity tests;
- two-client isolation results;
- restart and replay-to-live race results;
- queue/task/resource bounds;
- compatibility matrix for Unix socket, `/core`, `/tui` v4, and `/tui` corrected version;
- security review of acknowledgement, unsubscribe, resume, and artifact ownership;
- updated M4 historical closure note and current registry SHAs;
- zero unresolved high or medium transport-isolation findings.

## 11. Handoff constraints

- Do not replace the existing projection store, reducer, or daemon sequence authority.
- Do not create a second subscription registry in durable storage.
- Reuse the existing `take_subscription_receiver` ownership mechanism or refactor it into a shared transport adapter.
- Do not derive a stream ID from a subscription ID.
- Do not treat authentication token possession as subscription ownership.
- Do not remove legacy remote variants before the documented compatibility window.
- Stop for an ADR only if implementation requires changing replay authority, storage backend, or ordering guarantees.
