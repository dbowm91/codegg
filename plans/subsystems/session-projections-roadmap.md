# Frontend-Neutral Session Projections and Replay Roadmap

Status: closed — Milestone 005 corrective transport closure completed at `4c751ff`

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

## 3. Current state

### Closed foundations

- **M1** added versioned bounded projection DTOs, capability negotiation, canonical reducer semantics, snapshot builders, adapters, and independent-consumer fixtures.
- **M2 library** landed at `8dc4b85`: replay DTOs, schema migration v32, stream/event/checkpoint stores, retention, subscriptions, cursor validation, resync logic, metrics, and focused tests.
- **M2 daemon integration** landed at `c1d910a`: centralized publication, canonical binding resolution, request dispatch, subscription receivers, retention/checkpoint maintenance, and strict daemon-level tests.
- **M3** landed through `bac73ce`: capability context, fail-closed disclosure/redaction, artifact handles, bounded reads, and negative persistence tests.
- **M4 implementation** landed at `bdc2138`: shared projection controller, local TUI projection state, remote protocol additions, independent controller/reducer equivalence, and bounded frontend artifact caches.

### Post-M4 corrective finding (resolved)

The M4 frontend/controller work remains valid. Strict subsystem closure was
temporarily reopened after production transport inspection and was restored by
M005 at `4c751ff`.

The `/tui` and `/core` WebSocket adapters do not own projection receivers per connection. `/tui` stores no subscription IDs/descriptors/cursors, both WebSocket paths depend on daemon-wide raw event broadcasts, remote projection resume is not wired through `ProjectionCursor`, typed resync is degraded to generic error in one path, subscription identity is synthesized from stream identity for replay, and the Unix-socket forwarder emits a synthetic stream ID.

The authoritative corrective handoff is:

- `plans/implementation/session-projections/005-remote-transport-isolation-resume-closure.md`

The M4 historical record and corrective closure are:

- `plans/closure/session-projections/004-status.md`
- `plans/closure/session-projections/005-status.md`

## 4. Target architecture

The target remains:

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
|-- real descriptor + cursor per subscription
|-- one receiver-forwarder per subscription
|-- bounded outbound queues and lag state
`-- deterministic unsubscribe/disconnect cleanup

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
```

Milestone 005 has no unmet design dependency. It consumes existing M2 receiver ownership, M3 policy, and M4 controller/protocol seams.

## 6. Milestones

### Milestone 1 — Projection contracts and canonical reducer

Status: closed.

Implementation plan:

- `plans/implementation/session-projections/001-projection-contracts.md`

Closure:

- `plans/closure/session-projections/001-status.md`

### Milestone 2 — Scoped subscriptions and durable replay

Status: closed (strict).

Implementation plans:

- `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md`
- `plans/implementation/session-projections/002-corrective-daemon-integration-and-closure.md`

Closure:

- `plans/closure/session-projections/002-status.md`

Accepted outcomes include durable stream/cursor storage, persist-before-deliver publication, canonical binding routing, receiver ownership in the daemon socket, replay/resync, retention, and raw compatibility.

### Milestone 3 — Visibility, redaction, and artifact handles

Status: closed.

Implementation plan:

- `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md`

Closure:

- `plans/closure/session-projections/003-status.md`

Accepted outcomes include transport-derived capability context, structural-first redaction, fail-closed disclosure, artifact handles, bounded policy-checked reads, and adversarial persistence tests.

### Milestone 4 — Frontend adoption and compatibility

Status: closed.

Implementation plan:

- `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md`

Conditional closure:

- `plans/closure/session-projections/004-status.md`

Accepted outcomes:

- shared projection controller;
- local TUI bounded projection state;
- independent reducer/controller equivalence;
- additive remote protocol DTOs;
- bounded raw compatibility mode;
- frontend artifact caches.

Corrective findings were closed by Milestone 005.

### Milestone 5 — Remote transport isolation, resume, and compatibility closure

Status: closed.

Implementation plan:

- `plans/implementation/session-projections/005-remote-transport-isolation-resume-closure.md`

Closure:

- `plans/closure/session-projections/005-status.md`

Class: correctness / security / transport closure

Objective: make Unix socket, `/core`, and `/tui` projection delivery connection-owned, cursor-resumable, identity-correct, bounded, and cleanup-safe.

Required deliverables:

- shared connection-local projection subscription ownership;
- owned receiver forwarders for `/core` and `/tui`;
- real descriptor stream IDs on all transports;
- explicit projection resume/unsubscribe/status/artifact operations;
- typed replay/resync outcomes;
- generic broadcast exclusion of projection-private events;
- bounded WebSocket queues and lag behavior;
- disconnect/shutdown cleanup;
- versioned compatibility diagnostics;
- two-client transport isolation and restart/resume tests;
- strict corrected closure record.

Exit conditions:

- connection A cannot receive or operate on connection B’s subscription;
- reconnect resumes exactly missing committed events or returns typed resync;
- replay-to-live handoff has no loss window;
- stream and subscription IDs are never interchanged;
- all transient receivers/tasks are cleaned up deterministically;
- raw compatibility remains functional but mode-isolated;
- no unresolved high/medium transport findings remain;
- M4 and M5 closure records return the subsystem to strict closed status; both
  are now accepted.

## 7. Deferred product work

These items are not part of M5 correctness closure:

- cross-tab artifact hand-off UX;
- numeric acknowledgement/resync hot-key UX;
- plugin-specific `ProjectionEvent::PluginUi` semantics;
- final team principals/roles/presence/chat;
- cross-daemon replay replication;
- removal of legacy remote variants before the compatibility window expires.

## 8. Cross-cutting requirements

### Protocol and compatibility

Negotiate projection version and capabilities during initialization. Older clients remain on bounded raw compatibility or receive explicit unsupported behavior. M5 may add protocol variants/versioning but must not silently reinterpret version-4 messages.

### Security and authorization

Redaction remains structural first and heuristic second. Connection authentication is not subscription ownership. Every ack/resume/unsubscribe/artifact operation must verify connection-local ownership and daemon policy.

### Concurrency, cancellation, and recovery

Sequence assignment remains daemon-owned. Persistence precedes delivery. Subscribers and queues are bounded. Lag causes typed resync. Cancellation removes transient receivers only. Rebind invalidates prior stream revisions.

### Observability

Expose bounded counts and reason codes for subscriptions, queue lag, replay/resync, cleanup, foreign-operation rejection, and compatibility mode. Never log payload bodies or secrets.

### Performance

Cap event size, replay windows, subscriber queues, snapshots, artifact reads, connection subscriptions, forwarders, and pending WebSocket messages. Add reconnect and burst soak tests.

### Documentation

Maintain projection-version and transport compatibility matrices. Closure evidence must list exact commits and transport ownership behavior.

## 9. Verification strategy

Use:

- canonical reducer/controller fixtures;
- daemon replay/storage/failpoint tests;
- two-client Unix-socket, `/core`, and `/tui` isolation fixtures;
- restart and replay-to-live race tests;
- expired/gapped/ahead/mismatched cursor tests;
- unsubscribe/disconnect/shutdown tests;
- bounded queue/lag tests;
- remote artifact ownership tests;
- version-4 compatibility fixtures;
- disclosure/static guards.

## 10. Risks and decision points

- A daemon-wide event subscriber is convenient but cannot enforce projection subscription ownership.
- Taking a subscription receiver in more than one transport path can duplicate delivery; receiver ownership must be singular.
- Resume can lose events if replay high-water and live receiver installation are not coordinated.
- Fabricating stream identity breaks cursor validation and can leak cross-stream data.
- Unbounded WebSocket queues can convert a slow client into daemon memory growth.
- Legacy compatibility must be isolated by mode and deprecated deliberately, not removed abruptly.
- A backend or replay-authority change requires an ADR; M5 should not require either.

## 11. Completion definition

This roadmap returns to strict closed status when CodeGG has one bounded redacted projection contract, deterministic durable replay, equivalent frontend reduction, and connection-owned remote transport delivery that survives reconnect/restart without cross-client leakage, synthetic identity, silent gaps, or unbounded queue growth.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/session-projections/001-projection-contracts.md` | `plans/closure/session-projections/001-status.md` | — |
| 2 | closed (strict) | library + corrective plans | `plans/closure/session-projections/002-status.md` | — |
| 3 | closed | `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md` | `plans/closure/session-projections/003-status.md` | — |
| 4 | closed | `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md` | `plans/closure/session-projections/004-status.md` | — |
| 5 | closed | `plans/implementation/session-projections/005-remote-transport-isolation-resume-closure.md` | `plans/closure/session-projections/005-status.md` | — |
