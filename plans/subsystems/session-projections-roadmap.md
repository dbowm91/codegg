# Frontend-Neutral Session Projections and Replay Roadmap

Status: closed — Milestone 006 atomic control delivery and transport verification hardening

Long-term references:

- `plans/000-long-term-specification.md#8-read-only-session-observation`
- `plans/000-long-term-specification.md#14-acp-integration`
- `plans/001-terminology-and-domain-model.md` — session, turn, observation subscription, projection
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Related ADRs:

- None required for the current projection/replay boundary. Create an ADR if replay work changes authoritative event-log ownership, replaces SQLite, promises deployment-global ordering, or turns projection replay into audit retention.

## 1. Purpose and ownership boundary

This subsystem owns the canonical frontend-neutral session representation and the snapshot/event/replay semantics used by local TUI, remote TUI, later observer mode, ACP adapters, web clients, and future frontends. It defines bounded projection DTOs, reducer semantics, subscriptions, sequence/acknowledgement/resume behavior, durable replay indexing, visibility/redaction classification, artifact handles, and transport delivery invariants.

It consumes stable project/session identities, daemon events, turn/tool/run/job state, project-aware TUI state, session/message persistence, and protocol capability negotiation. It must not own team authorization policy, presence, chat, full audit retention, raw provider hidden reasoning, or deployment-global ordering.

## 2. Invariants

- All frontends reconstruct equivalent logical session state from the same snapshot and event stream.
- Sequence ordering is monotonic within one stream and replay is deterministic.
- Expired or missing history triggers bounded typed resynchronization.
- Secret-bearing data is redacted before shared durable storage.
- Large logs, artifacts, and file bodies remain behind bounded handles.
- Raw terminal frames are not the canonical collaboration protocol.
- Unknown variants and versions degrade safely.
- Project/session subscriptions avoid unrelated traffic.
- A live projection envelope reaches only the connection that owns its daemon-issued subscription.
- Subscription ID, stream ID, project ID, session ID, client ID, and transport connection ID remain distinct.
- Transports preserve the real persisted stream ID and never synthesize it from a subscription ID.
- Generic raw event broadcasts never carry subscription-private projection events.
- Disconnect removes transient subscription receivers without deleting replay history.
- Transport queues and per-connection tasks remain bounded.
- A subscription cannot become live before its canonical snapshot/replay response is successfully delivered.
- Critical control delivery failure rolls back connection ownership and daemon subscription state.
- Raw compatibility traffic is explicitly scoped by connection/session/filter selection.
- No WebSocket server adapter uses an unbounded outbound queue.

## 3. Current state

### Closed foundations

- **M1** added versioned bounded projection DTOs, capability negotiation, canonical reducer semantics, snapshot builders, adapters, and independent-consumer fixtures.
- **M2 library** landed at `8dc4b85`: replay DTOs, schema migration v32, stream/event/checkpoint stores, retention, subscriptions, cursor validation, resync logic, metrics, and focused tests.
- **M2 daemon integration** landed at `c1d910a`: centralized publication, canonical binding resolution, request dispatch, subscription receivers, retention/checkpoint maintenance, and strict daemon-level tests.
- **M3** landed through `bac73ce`: capability context, fail-closed disclosure/redaction, artifact handles, bounded reads, and negative persistence tests.
- **M4 implementation** landed at `bdc2138`: shared projection controller, local TUI projection state, remote protocol additions, independent controller/reducer equivalence, and bounded frontend artifact caches.
- **M5 corrective transport closure** landed at `4c751ff`: connection-local receiver ownership, exact stream identity, cursor resume, typed lifecycle operations, bounded queues, raw projection filtering, and disconnect cleanup.
- **M6 atomic control delivery and transport verification hardening** landed at `8ca570f`: critical writer receipts, activation-after-delivery state transitions, deterministic rollback, real Unix/WebSocket isolation tests, raw compatibility scoping, and bounded legacy `/ws` output. Closure: `plans/closure/session-projections/006-status.md`.

### Post-M5 hardening finding

M5 fixed the original cross-connection projection isolation and resume defects. Post-closure inspection identified a narrower atomic-delivery problem:

- `/tui` and `/core` place critical snapshot/replay/control frames into bounded queues with non-blocking sends;
- queue failure can be ignored while a staged subscription is subsequently marked live;
- a client can therefore miss its canonical initial state and receive later live events;
- current transport-isolation evidence is weighted toward replay-service receiver tests rather than real two-client sockets;
- raw compatibility live traffic is not fully scoped by the connection’s session/filter state;
- the deprecated `/ws` endpoint still uses an unbounded outbound queue.

The authoritative hardening handoff is:

- `plans/implementation/session-projections/006-atomic-control-delivery-transport-verification-hardening.md`

M5 remains the historical closure for its implemented isolation work. M006 is a distinct transport-control and verification hardening milestone; it does not reopen storage, reducer, disclosure, or replay semantics.

## 4. Target architecture

```text
Projection store / replay service
|-- real project/session stream descriptors
|-- per-stream sequence and cursor authority
|-- bounded retained events/checkpoints
|-- fail-closed disclosure and artifact policy
`-- daemon-issued subscription receiver

Transport connection
|-- trusted connection/client identity
|-- negotiated projection version/mode
|-- bounded owned subscription map
|-- staged Initializing subscription
|-- critical control writer with timeout/cancellation
|-- mark-live only after snapshot/replay delivery success
|-- one bounded receiver-forwarder per live subscription
|-- session/filter-scoped raw compatibility
`-- deterministic rollback/unsubscribe/cleanup

Frontend controller
|-- canonical reducer
|-- cursor/ack/resync lifecycle
|-- bounded tab summaries and active view
`-- explicit raw compatibility mode
```

Projection storage remains separate from chat/message storage and final audit retention.

## 5. Dependency graph

```text
M1 projection contracts and reducer                     [closed]
        |
        v
M2 durable replay + daemon integration                   [closed]
        |
        v
M3 visibility, redaction, artifact handles               [closed]
        |
        v
M4 frontend adoption/controller                          [closed]
        |
        v
M5 remote transport isolation, resume, compatibility     [closed]
        |
        v
M6 atomic control delivery and real transport evidence   [closed]
```

M006 has no unmet design dependency. It consumes the M2 receiver seam, M4 controller, and M5 connection-local transport owner.

## 6. Milestones

### Milestone 1 — Projection contracts and canonical reducer

Status: closed.

- Plan: `plans/implementation/session-projections/001-projection-contracts.md`
- Closure: `plans/closure/session-projections/001-status.md`

### Milestone 2 — Scoped subscriptions and durable replay

Status: closed (strict).

- Plans:
  - `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md`
  - `plans/implementation/session-projections/002-corrective-daemon-integration-and-closure.md`
- Closure: `plans/closure/session-projections/002-status.md`

### Milestone 3 — Visibility, redaction, and artifact handles

Status: closed.

- Plan: `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md`
- Closure: `plans/closure/session-projections/003-status.md`

### Milestone 4 — Frontend adoption and compatibility

Status: closed after M5 corrective integration.

- Plan: `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md`
- Closure: `plans/closure/session-projections/004-status.md`

### Milestone 5 — Remote transport isolation, resume, and compatibility closure

Status: closed.

- Plan: `plans/implementation/session-projections/005-remote-transport-isolation-resume-closure.md`
- Closure: `plans/closure/session-projections/005-status.md`
- Implementation: `4c751ff`

Accepted outcomes include connection-local ownership, exact stream/subscription identity, typed resume/resync/ack/unsubscribe/status/artifact operations, projection-private raw-broadcast filtering, bounded queues, and deterministic disconnect cleanup.

### Milestone 6 — Atomic control delivery, transport verification, and raw compatibility hardening

Status: closed.

Implementation plan:

- `plans/implementation/session-projections/006-atomic-control-delivery-transport-verification-hardening.md`

Class: correctness / transport hardening / verification

Implementation: `8ca570fddc08eb9663b894f3190ae0ed0af2b98b`

Closure: `plans/closure/session-projections/006-status.md`

Objective: make projection subscription establishment atomic with critical response delivery, prove the production adapters with real multi-client socket tests, scope raw compatibility traffic, and eliminate the remaining unbounded WebSocket queue.

Required deliverables:

- bounded critical-send primitive with timeout/cancellation;
- `Initializing -> Live` only after successful snapshot/replay response delivery;
- rollback and daemon unsubscribe on queue/writer/serialization/timeout failure;
- `/tui` and `/core` activation coupled to actual response delivery outcome;
- real two-client `/tui`, `/core`, and Unix-socket tests;
- queue-saturation and disconnect-during-install tests;
- response-before-live ordering proof;
- session-scoped `/tui` raw compatibility;
- live `/core` raw filtering matching replay filters;
- projection-primary suppression of raw session mutations;
- bounded or disabled-by-default deprecated `/ws` endpoint;
- static guards against unbounded WebSocket channels and known activation drift;
- dedicated M006 closure record.

Exit conditions:

- a client cannot receive live event N+1 without first receiving canonical state through N;
- control queue saturation cannot produce an apparently live subscription;
- failed critical delivery leaves no receiver, forwarder, connection owner, or daemon subscription leak;
- real transport tests prove isolation and foreign-operation rejection;
- raw compatibility is connection/session/filter scoped;
- no server WebSocket adapter uses `mpsc::unbounded_channel`;
- no unresolved high or medium transport finding remains;
- roadmap and registry return to strict closed status.

## 7. Deferred product work

These items are not part of M006:

- cross-tab artifact hand-off UX;
- numeric acknowledgement/resync hot-key UX;
- plugin-specific `ProjectionEvent::PluginUi` semantics;
- final team principals/roles/presence/chat;
- cross-daemon replay replication;
- removal of version-4 compatibility before its compatibility window expires.

## 8. Cross-cutting requirements

### Protocol and compatibility

Do not increment the projection protocol or remove version-4 compatibility. M006 changes delivery reliability and filter enforcement, not wire meaning.

### Security and authorization

Connection authentication is not subscription ownership. Raw compatibility scoping must reduce visibility, never broaden it. Foreign ack/resume/unsubscribe/status/artifact operations remain fail-closed.

### Concurrency, cancellation, and recovery

Persistence and receiver installation precede response staging; successful response delivery precedes live release. Every failure path has bounded cancellation and idempotent rollback.

### Observability

Expose bounded reason codes for critical-send timeout, queue saturation, writer close, activation rollback, raw-scope rejection, and legacy endpoint overflow. Never log payload bodies or secrets.

### Performance

Critical sends remain bounded and prioritized. No solution may reintroduce unbounded queues or hold transport state locks across socket I/O.

### Documentation

Maintain transport state-machine, queue, compatibility, and test matrices. Closure evidence must include exact production commits and real socket results.

## 9. Verification strategy

Use:

- canonical reducer/controller fixtures;
- daemon replay/storage/failpoint tests;
- real two-client Unix-socket, `/core`, and `/tui` fixtures;
- queue-full and writer-close activation tests;
- response-before-live ordering tests;
- restart and replay-to-live race tests;
- unsubscribe/disconnect/shutdown tests;
- raw compatibility session/filter tests;
- legacy `/ws` resource-bound tests;
- disclosure/static guards.

## 10. Risks and decision points

- Treating queue enqueue as delivery may still activate a subscription after writer failure; use acknowledgement where required.
- Awaiting critical sends without timeout can convert a slow client into a stuck daemon task.
- A separate control queue is useful only if its failure is propagated into lifecycle rollback.
- Raw compatibility scoping must share filter semantics with replay to avoid drift.
- Disabling `/ws` may affect legacy users; retain an explicit bounded opt-in if compatibility is required.
- A replay-authority or storage change requires an ADR; M006 should require neither.

## 11. Completion definition

This roadmap returns to strict closed status when projection transport is not only connection-owned and bounded, but also atomically established: canonical state delivery is proven before live release, all delivery failures roll back cleanly, raw compatibility is scoped, and real multi-client transport tests verify the production adapters.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/session-projections/001-projection-contracts.md` | `plans/closure/session-projections/001-status.md` | — |
| 2 | closed (strict) | library + corrective plans | `plans/closure/session-projections/002-status.md` | — |
| 3 | closed | `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md` | `plans/closure/session-projections/003-status.md` | — |
| 4 | closed | `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md` | `plans/closure/session-projections/004-status.md` | — |
| 5 | closed | `plans/implementation/session-projections/005-remote-transport-isolation-resume-closure.md` | `plans/closure/session-projections/005-status.md` | — |
| 6 | ready | `plans/implementation/session-projections/006-atomic-control-delivery-transport-verification-hardening.md` | — | — |
