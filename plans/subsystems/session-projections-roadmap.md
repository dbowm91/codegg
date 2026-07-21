# Frontend-Neutral Session Projections and Replay Roadmap

Status: active — Milestone 007 corrective transport lifecycle and evidence closure

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
- A subscription cannot become live before its canonical snapshot/replay response is successfully delivered.
- Critical control failure rolls back connection ownership and daemon subscription state.
- Every connection-scoped raw and projection forwarder has explicit ownership, cancellation, and teardown.
- Disconnect removes transient receivers and tasks without deleting replay history.
- Transport queues and per-connection tasks remain bounded.
- Raw compatibility traffic is explicitly scoped by connection/session/filter selection.
- Changing a TUI raw session route invalidates queued events from prior route generations.
- No WebSocket server adapter uses an unbounded outbound queue.
- Closure evidence names exact tests, commands, counts, commits, and residual failures truthfully.

## 3. Current state

### Closed foundations

- **M1** added versioned bounded projection DTOs, capability negotiation, canonical reducer semantics, snapshot builders, adapters, and independent-consumer fixtures.
- **M2 library** landed at `8dc4b85`: replay DTOs, schema migration v32, stream/event/checkpoint stores, retention, subscriptions, cursor validation, resync logic, metrics, and focused tests.
- **M2 daemon integration** landed at `c1d910a`: centralized publication, canonical binding resolution, request dispatch, subscription receivers, retention/checkpoint maintenance, and strict daemon-level tests.
- **M3** landed through `bac73ce`: capability context, fail-closed disclosure/redaction, artifact handles, bounded reads, and negative persistence tests.
- **M4** landed at `bdc2138`: shared projection controller, local TUI projection state, remote protocol additions, independent controller/reducer equivalence, and bounded frontend artifact caches.
- **M5** landed at `4c751ff`: connection-local receiver ownership, exact stream identity, cursor resume, typed lifecycle operations, bounded queues, raw projection filtering, and disconnect cleanup.
- **M6 implementation** landed at `8ca570f`: critical writer receipts, activation-after-delivery transitions, rollback paths, real normal-flow Unix/WebSocket isolation tests, raw compatibility scoping, and bounded legacy `/ws` output.

### M6 conditional closure

M6 remains a valid implementation foundation but is conditionally closed after post-closure inspection found:

- the Unix raw forwarder is detached from connection teardown and may retain writer/filter/event-receiver state after peer disconnect;
- `/tui` raw events have no route generation, so an event queued for session A may drain after `SessionInfo` switches the connection to session B;
- helper-level critical-send failure tests do not prove staged daemon subscription rollback through each production adapter;
- real tests publish only after consuming the initial response and therefore do not exercise the response-blocked replay-to-live race;
- real foreign ack/resume/status/artifact and reconnect/exact-replay coverage is incomplete;
- the M6 closure record requires test-count and protocol-version failure reconciliation.

Authoritative documents:

- Conditional M6 record: `plans/closure/session-projections/006-status.md`
- M7 handoff: `plans/implementation/session-projections/007-corrective-transport-lifecycle-and-evidence-closure.md`

M7 does not reopen storage, reducer, disclosure, replay authority, or protocol DTO meaning.

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
|-- critical control writer with timeout/cancellation/receipt
|-- mark-live only after snapshot/replay delivery success
|-- one owned receiver-forwarder per live subscription
|-- one owned raw forwarder with connection cancellation
|-- generation-scoped TUI raw routing
|-- session/filter-scoped raw compatibility
`-- deterministic rollback/unsubscribe/joined cleanup

Frontend controller
|-- canonical reducer
|-- cursor/ack/resync lifecycle
|-- bounded tab summaries and active view
`-- explicit raw compatibility mode
```

Projection storage remains separate from chat/message storage and final audit retention.

## 5. Dependency graph

```text
M1 projection contracts and reducer                         [closed]
        |
        v
M2 durable replay + daemon integration                      [closed]
        |
        v
M3 visibility, redaction, artifact handles                  [closed]
        |
        v
M4 frontend adoption/controller                             [closed]
        |
        v
M5 remote transport isolation, resume, compatibility        [closed]
        |
        v
M6 atomic control delivery and transport hardening          [conditionally closed]
        |
        v
M7 lifecycle ownership, route epochs, exact race evidence   [ready]
```

M7 has no unmet design dependency. It consumes the M6 critical-send and activation state machine and tightens production lifecycle ownership and evidence.

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

Accepted outcomes include connection-local ownership, exact stream/subscription identity, typed resume/resync/ack/unsubscribe/status/artifact operations, projection-private raw-broadcast filtering, bounded queues, and deterministic projection subscription cleanup.

### Milestone 6 — Atomic control delivery, transport verification, and raw compatibility hardening

Status: conditionally closed.

- Plan: `plans/implementation/session-projections/006-atomic-control-delivery-transport-verification-hardening.md`
- Conditional closure: `plans/closure/session-projections/006-status.md`
- Implementation: `8ca570fddc08eb9663b894f3190ae0ed0af2b98b`

Accepted outcomes:

- bounded critical-send primitive with timeout/cancellation;
- WebSocket writer receipts;
- `Initializing -> Live` after successful initial response delivery;
- rollback paths on critical failure;
- real normal-flow two-client `/tui`, `/core`, and Unix isolation fixtures;
- current-session raw filtering and projection-primary suppression;
- shared `/core` replay/live filtering;
- bounded deprecated `/ws` output;
- static guards against unbounded server WebSocket channels.

Strict closure is deferred to M7 because Unix raw task ownership, stale queued TUI raw routing, adapter-level failure/race tests, full foreign-operation coverage, reconnect evidence, and closure accuracy remain incomplete.

### Milestone 7 — Corrective transport lifecycle and evidence closure

Status: ready.

Implementation plan:

- `plans/implementation/session-projections/007-corrective-transport-lifecycle-and-evidence-closure.md`

Class: correctness / lifecycle cleanup / transport verification

Repository baseline: `dbbaabdde51db09f0c5beb704234ce1d94d01c9a`

Objective: make every connection-scoped task deterministically owned and terminated, make TUI raw session switching epoch-safe, prove critical failure rollback and response-before-live ordering through production adapters, complete foreign-operation and reconnect coverage, and correct closure evidence.

Required deliverables:

- owned/cancelled/joined Unix raw forwarder;
- teardown tests proving no retained writer/filter/event-receiver state;
- generation-tagged `/tui` raw outbound routing with final-boundary stale rejection;
- deterministic production-adapter fault injection;
- response-blocked live publication tests for `/tui`, `/core`, and Unix;
- queue-full, writer-close, cancellation, serialization, timeout, and disconnect rollback tests against staged daemon subscriptions;
- real foreign ack/resume/unsubscribe/status/artifact rejection coverage;
- disconnect/reconnect exact missing-range replay and gap-free replay-to-live tests;
- resource-leak and cross-client non-interference tests;
- extended static guards;
- reconciled test counts and protocol-version expectations;
- dedicated M7 closure record.

Exit conditions:

- no connection-scoped raw or projection task survives teardown;
- no stale queued TUI raw event crosses a committed session-route generation;
- no live event can overtake a blocked canonical response;
- all critical failure boundaries remove transport and daemon subscription state;
- foreign operations are fail-closed and side-effect free;
- reconnect resumes exactly the missing committed range and transitions live without gap or duplication;
- closure evidence matches executable tests and command output;
- no unresolved high or medium M7 finding remains;
- M6 and M7 records support strict subsystem closure.

## 7. Deferred product work

These items are not part of M7:

- cross-tab artifact hand-off UX;
- numeric acknowledgement/resync hot-key UX;
- plugin-specific `ProjectionEvent::PluginUi` semantics;
- final team principals/roles/presence/chat;
- cross-daemon replay replication;
- removal of version-4 compatibility before its compatibility window expires.

## 8. Cross-cutting requirements

### Protocol and compatibility

Do not increment the projection protocol or remove version-4 compatibility. M7 changes lifecycle ownership, route invalidation, verification, and evidence—not wire meaning.

### Security and authorization

Connection authentication is not subscription ownership. Raw route generations must only reduce stale visibility. Foreign ack/resume/unsubscribe/status/artifact operations remain fail-closed.

### Concurrency, cancellation, and recovery

Persistence and receiver installation precede response staging; successful response delivery precedes live release. Every connection task is owned. Every failure path has bounded cancellation and idempotent rollback. No state lock is held across socket I/O or task join.

### Observability

Expose bounded reason codes for task cancellation, critical-send failure, activation rollback, stale raw generation rejection, foreign-operation rejection, reconnect replay, and resource cleanup. Never log payload bodies, artifact bytes, or secrets.

### Performance

Critical sends remain bounded and prioritized. Raw route generation checks must be constant-time and must not reintroduce unbounded queues or per-event unbounded task creation.

### Documentation

Maintain transport state-machine, task ownership, queue, route-generation, compatibility, and test matrices. Closure evidence must include exact production and closure commits.

## 9. Verification strategy

Use:

- canonical reducer/controller fixtures;
- daemon replay/storage/failpoint tests;
- real two-client Unix, `/core`, and `/tui` fixtures;
- deterministic response-blocking and fault-injection hooks;
- queue-full, writer-close, cancellation, timeout, serialization, and disconnect tests;
- route-generation session-switch tests;
- foreign-operation matrix tests;
- reconnect and replay-to-live race tests;
- repeated teardown/resource-baseline tests;
- legacy `/ws` and raw compatibility regressions;
- disclosure and static guards.

## 10. Risks and decision points

- Aborting a task without awaiting it can still retain resources until scheduling completes; teardown must join or abort-and-await.
- Filtering before queue insertion is insufficient when route identity can change while items wait in the queue.
- Test-only fault hooks must remain connection-local and must not introduce production global mutable state.
- A normal-flow socket test is not evidence for a blocked-response race.
- Foreign-operation tests must verify both rejection and lack of side effects on the owner.
- Reconnect tests must distinguish stream identity from the new subscription identity.
- A replay-authority or storage change requires an ADR; M7 should require neither.

## 11. Completion definition

This roadmap returns to strict closed status when the projection transport is connection-owned at both subscription and task-lifecycle levels, canonical state delivery is proven before live release under actual races and failures, raw session switching rejects stale queued traffic, reconnect replay is exact, foreign operations are fail-closed, and closure evidence precisely matches the repository.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/session-projections/001-projection-contracts.md` | `plans/closure/session-projections/001-status.md` | — |
| 2 | closed (strict) | library + corrective plans | `plans/closure/session-projections/002-status.md` | — |
| 3 | closed | `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md` | `plans/closure/session-projections/003-status.md` | — |
| 4 | closed | `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md` | `plans/closure/session-projections/004-status.md` | — |
| 5 | closed | `plans/implementation/session-projections/005-remote-transport-isolation-resume-closure.md` | `plans/closure/session-projections/005-status.md` | — |
| 6 | conditionally closed | `plans/implementation/session-projections/006-atomic-control-delivery-transport-verification-hardening.md` | `plans/closure/session-projections/006-status.md` | M7 lifecycle/evidence findings |
| 7 | ready | `plans/implementation/session-projections/007-corrective-transport-lifecycle-and-evidence-closure.md` | — | — |