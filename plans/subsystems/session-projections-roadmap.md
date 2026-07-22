# Frontend-Neutral Session Projections and Replay Roadmap

Status: active — Milestone 009 production-shaped transport verification and strict closure

Long-term references:

- `plans/000-long-term-specification.md#8-read-only-session-observation`
- `plans/000-long-term-specification.md#14-acp-integration`
- `plans/001-terminology-and-domain-model.md` — session, turn, observation subscription, projection
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Related ADRs:

- None required for the current verification pass. Create an ADR only if work changes authoritative replay ownership, replaces SQLite, promises deployment-global ordering, or changes projection retention into audit retention.

## 1. Purpose and ownership boundary

This subsystem owns the canonical frontend-neutral session representation and the snapshot/event/replay semantics used by local TUI, remote TUI, observer mode, ACP adapters, web clients, and future frontends. It defines bounded projection DTOs, reducer semantics, subscriptions, sequence/acknowledgement/resume behavior, durable replay indexing, visibility/redaction classification, artifact handles, and transport delivery invariants.

It consumes stable project/session identities, daemon events, turn/tool/run/job state, project-aware TUI state, session/message persistence, and protocol capability negotiation. It does not own team authorization policy, presence, chat, full audit retention, raw provider hidden reasoning, or deployment-global ordering.

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
- Every connection-scoped send, receive, raw, and projection task has explicit ownership, cancellation, and joined teardown.
- Disconnect removes transient receivers and tasks without deleting replay history.
- Transport queues and per-connection tasks remain bounded.
- Raw compatibility traffic is explicitly scoped by connection/session/filter selection.
- Changing a TUI raw session route invalidates queued events from prior route generations.
- Reconnect replay delivers exactly the missing committed envelopes once and transitions to the next live sequence without a gap or duplicate.
- Production queue-timeout evidence must fill the actual bounded adapter queue; an injected timeout classification is not equivalent.
- Production disconnect evidence must close or fail the real peer; an injected cancellation classification is not equivalent.
- Every real staged failure returns connection tasks, forwarders, receivers, ownership, and daemon subscriptions to baseline.
- No WebSocket server adapter uses an unbounded outbound queue.
- Closure evidence names exact tests, commands, counts, commits, mechanism type, and residual failures truthfully.

## 3. Current state

### Closed foundations

- **M1** delivered projection contracts, capability negotiation, canonical reducer semantics, snapshot builders, adapters, and independent-consumer fixtures.
- **M2** delivered durable replay storage, cursor validation, retention/checkpoints, daemon publication, binding resolution, subscription receivers, and replay protocol tests.
- **M3** delivered disclosure/redaction policy, artifact handles, bounded reads, and negative persistence tests.
- **M4** delivered the shared projection controller and local/remote frontend adoption.
- **M5** delivered connection-local subscription ownership, exact stream identity, typed resume/ack/unsubscribe/status/artifact operations, raw projection filtering, bounded queues, and disconnect cleanup.
- **M6 implementation** delivered critical writer receipts, activation-after-delivery, rollback, normal-flow transport isolation, raw scoping, and bounded legacy `/ws` output.
- **M7 implementation** delivered joined Unix raw lifecycle, TUI route generations, deterministic lifecycle seams, blocked-response tests, foreign-operation coverage, reconnect fixtures, and evidence reconciliation.
- **M8 implementation** delivered shared joined `/core` and `/tui` task ownership plus exact envelope-level replay continuity.

### Conditional closure history

M6 and M7 remain valid implementation foundations with historical conditional closure records. Their principal findings were addressed by later milestones.

M8 materially fixed the final known production task-lifecycle defect and strengthened replay evidence. Post-closure inspection found verification gaps rather than a new architecture defect:

- queue timeout is represented in the adapter matrix by an injected `Timeout` classification rather than actual bounded queue saturation;
- disconnect/cancellation is represented primarily by injected `Cancelled` classifications rather than real peer close or socket failure during staged setup;
- per-scenario matrices do not prove every ownership, receiver, forwarder, task/drop, idempotence, and unrelated-client invariant;
- real adapter lifecycle tests do not cover every first-exit shape, repeated churn, and client-A/client-B continuity;
- fresh connection identity and disconnect-during-replay cleanup/retry remain incompletely proven;
- M008 planning and closure records required correction from strict to conditional status.

Authoritative documents:

- M6 conditional record: `plans/closure/session-projections/006-status.md`
- M7 conditional record: `plans/closure/session-projections/007-status.md`
- M8 conditional record: `plans/closure/session-projections/008-status.md`
- M9 final verification handoff: `plans/implementation/session-projections/009-production-shaped-transport-verification-and-strict-closure.md`

M9 does not reopen storage, reducer, disclosure, replay authority, sequence semantics, or protocol DTO meaning.

## 4. Target architecture and verification boundary

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
|-- owned send/receive/raw task set
|-- one owned receiver-forwarder per live subscription
|-- generation-scoped TUI raw routing
|-- session/filter-scoped raw compatibility
|-- connection cancellation before task teardown
`-- deterministic rollback/unsubscribe/abort-and-await cleanup

Production-shaped verification
|-- actual bounded queue saturation
|-- real WebSocket/Unix peer close and socket failure
|-- connection cancellation winning a pending operation
|-- task/forwarder/receiver/ownership baseline probes
|-- unrelated-client continuity
|-- interrupted replay cleanup and retry
`-- exact executable evidence and closure record

Replay-to-live handoff
|-- stable stream identity
|-- fresh connection identity where exposed
|-- new subscription identity
|-- exact missing envelope range
|-- monotonic sequence continuity
`-- first live envelope = replay_end_seq + 1, no duplicates
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
M6 atomic control delivery and transport hardening          [conditionally closed; principal findings addressed]
        |
        v
M7 lifecycle ownership, route epochs, race evidence         [conditionally closed; principal findings addressed]
        |
        v
M8 joined WebSocket teardown and exact replay evidence       [conditionally closed; production-shaped verification required]
        |
        v
M9 real transport mechanisms and strict verification         [ready]
```

M9 has no unmet design dependency. It consumes the M8 production task owner, lifecycle seam, exact replay fixtures, and static guard.

## 6. Milestones

### Milestones 1–5

Status: closed.

- M1: `plans/implementation/session-projections/001-projection-contracts.md`
- M2: durable replay library and corrective daemon integration plans; closure `plans/closure/session-projections/002-status.md`
- M3: `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md`
- M4: `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md`
- M5: `plans/implementation/session-projections/005-remote-transport-isolation-resume-closure.md`

### Milestone 6 — Atomic control delivery and transport hardening

Status: conditionally closed; principal findings addressed by M7/M8.

- Plan: `plans/implementation/session-projections/006-atomic-control-delivery-transport-verification-hardening.md`
- Closure: `plans/closure/session-projections/006-status.md`
- Implementation: `8ca570fddc08eb9663b894f3190ae0ed0af2b98b`

### Milestone 7 — Corrective transport lifecycle and evidence closure

Status: conditionally closed; principal findings addressed by M8.

- Plan: `plans/implementation/session-projections/007-corrective-transport-lifecycle-and-evidence-closure.md`
- Closure: `plans/closure/session-projections/007-status.md`
- Implementation: `9887c2d581a3d01280485523161695d08469c34f`

### Milestone 8 — Final transport lifecycle and replay evidence polish

Status: conditionally closed; M9 production-shaped verification required.

- Plan: `plans/implementation/session-projections/008-final-transport-lifecycle-and-replay-evidence-polish.md`
- Conditional closure: `plans/closure/session-projections/008-status.md`
- Implementation: `6975050af530eb5bd7a640c1f7ac9a31859dfda3`

Accepted outcomes:

- shared cancel/abort-and-await task ownership for `/core` and `/tui`;
- joined Unix raw/client lifecycle;
- bounded critical response delivery and activation-after-delivery;
- route-generation rejection of stale TUI raw traffic;
- exact replay envelope sequence and identity assertions for all three transports;
- paused `/core` replay-response/live-publication race;
- lifecycle guard rejecting abort-without-await cleanup.

Residual verification findings:

- actual queue saturation not yet demonstrated through adapter queues;
- real peer-close/cancellation setup races incomplete;
- complete per-scenario lifecycle baseline assertions incomplete;
- first-exit, churn, and two-client lifecycle matrices incomplete;
- interrupted replay cleanup/retry and connection identity evidence incomplete.

### Milestone 9 — Production-shaped transport verification and strict closure

Status: ready.

Implementation plan:

- `plans/implementation/session-projections/009-production-shaped-transport-verification-and-strict-closure.md`

Class: verification closure / production-shaped transport faults / lifecycle evidence / planning reconciliation

Repository baseline: `33c7cc4a9515263015d644f5bf713178bf5fbcb9`

Objective: exercise actual bounded queue saturation, real peer disconnect and cancellation races, complete task/receiver/forwarder/ownership baseline assertions, interrupted replay cleanup/retry, and final truthful closure evidence.

Required deliverables:

- connection-local bounded lifecycle probes;
- send-first, receive-first, and raw-event-first shared task-owner tests;
- real `/core` and `/tui` peer-close, writer-failure, raw-source, paused-setup cancellation, churn, and two-client continuity tests;
- actual `/core` and `/tui` queue-saturation timeout tests;
- Unix peer-disconnect and cancellation/response-completion race tests;
- reusable complete per-scenario rollback assertions;
- fresh connection identity checks where exposed;
- disconnect-during-replay cleanup and successful retry from the same cursor;
- static guards that distinguish real mechanisms from injected error classifications;
- dedicated M9 closure record and reconciled M8/roadmap/registry status.

Exit conditions:

- real bounded queue saturation produces the production critical-send timeout;
- real peer close/cancellation terminates staged setup and all connection-scoped tasks;
- all task, forwarder, receiver, ownership, and daemon-subscription probes return to baseline;
- repeated cleanup is idempotent and unrelated clients remain live;
- interrupted replay removes only transient state and remains durable for a later resume;
- replay retry remains exact and duplicate-free;
- focused suites and guards pass;
- closure evidence distinguishes seam tests from production mechanism tests;
- no unresolved high or medium M9 finding remains;
- M9 closure returns the roadmap and registry to strict closed status.

## 7. Deferred product work

These items are outside M9:

- cross-tab artifact hand-off UX;
- numeric acknowledgement/resync hot-key UX;
- plugin-specific `ProjectionEvent::PluginUi` semantics;
- final team principals/roles/presence/chat;
- cross-daemon replay replication;
- removal of version-4 compatibility before its compatibility window expires.

## 8. Cross-cutting requirements

### Protocol and compatibility

Do not increment the projection protocol or remove version-4 compatibility. M9 changes verification depth, test instrumentation, and closure evidence—not wire meaning.

### Security and authorization

Connection authentication is not subscription ownership. Foreign lifecycle and artifact operations remain fail-closed. Probes must not retain payload bodies, artifact bytes, hidden reasoning, or secrets.

### Concurrency, cancellation, and recovery

Persistence and receiver installation precede response staging; successful response delivery precedes live release. Connection cancellation precedes sibling task teardown. Every retained task is awaited. Every failure path has bounded cancellation and idempotent rollback. No state lock is held across socket I/O or task join.

### Performance

Critical sends remain bounded and prioritized. Test instrumentation must not add production queue growth, per-event tasks, or runtime-global mutable state.

### Documentation

Maintain transport state-machine, task ownership, queue, route-generation, replay continuity, compatibility, mechanism-verification, and test matrices. Closure evidence must include exact implementation and closure commits.

## 9. Verification strategy

Use:

- actual bounded queue saturation through production adapter senders;
- real WebSocket and Unix peer close/socket failure;
- connection cancellation while setup is pending;
- three first-exit shared task-owner cases;
- task/drop/forwarder/receiver/ownership baselines;
- 100-cycle connection and staged-failure churn;
- two-client non-interference;
- exact reconnect and interrupted replay retry tests;
- existing replay, disclosure, artifact, TUI, raw compatibility, and static guards.

Injected lifecycle seams remain valid for boundary reachability, serialization-equivalent failure, and pre-activation rollback. They are not substitutes for real queue or socket mechanisms.

## 10. Completion definition

This roadmap returns to strict closed status when all three production transports are bounded and connection-owned at both subscription and task levels; actual queue saturation, peer disconnect, and cancellation paths prove complete cleanup; replay interruption preserves durable history; all transient resources return to baseline; and closure evidence precisely matches the executable repository.

## 11. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/session-projections/001-projection-contracts.md` | `plans/closure/session-projections/001-status.md` | — |
| 2 | closed (strict) | library + corrective plans | `plans/closure/session-projections/002-status.md` | — |
| 3 | closed | `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md` | `plans/closure/session-projections/003-status.md` | — |
| 4 | closed | `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md` | `plans/closure/session-projections/004-status.md` | — |
| 5 | closed | `plans/implementation/session-projections/005-remote-transport-isolation-resume-closure.md` | `plans/closure/session-projections/005-status.md` | — |
| 6 | conditionally closed | `plans/implementation/session-projections/006-atomic-control-delivery-transport-verification-hardening.md` | `plans/closure/session-projections/006-status.md` | principal findings addressed by later milestones |
| 7 | conditionally closed | `plans/implementation/session-projections/007-corrective-transport-lifecycle-and-evidence-closure.md` | `plans/closure/session-projections/007-status.md` | principal findings addressed by M8 |
| 8 | conditionally closed | `plans/implementation/session-projections/008-final-transport-lifecycle-and-replay-evidence-polish.md` | `plans/closure/session-projections/008-status.md` | M9 production-shaped verification and strict closure |
| 9 | ready | `plans/implementation/session-projections/009-production-shaped-transport-verification-and-strict-closure.md` | — | — |