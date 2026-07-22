# Frontend-Neutral Session Projections and Replay Roadmap

Status: closed

Long-term references:

- `plans/000-long-term-specification.md#8-read-only-session-observation`
- `plans/000-long-term-specification.md#14-acp-integration`
- `plans/001-terminology-and-domain-model.md` — session, turn, observation subscription, projection
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Related ADRs:

- None required for M010. Create an ADR only if work changes authoritative replay ownership, replaces SQLite, promises deployment-global ordering, or changes projection retention into audit retention.

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
- A queue-saturation claim requires observed channel fullness and a direct production timeout result.
- A peer-disconnect claim requires a real peer close or socket failure before the named operation completes.
- A raw-source-first claim requires actual raw-source termination and observed first task kind.
- Every real staged failure returns connection tasks, forwarders, receivers, ownership, queues, and daemon subscriptions to baseline.
- No WebSocket server adapter uses an unbounded outbound queue.
- Closure evidence names exact tests, commands, counts, commits, mechanisms, exceptions, and residual failures truthfully.

## 3. Current state

### Closed foundations

- **M1** delivered versioned bounded projection DTOs, capability negotiation, canonical reducer semantics, snapshot builders, adapters, and independent-consumer fixtures.
- **M2** delivered durable replay storage, cursor validation, retention/checkpoints, daemon publication, binding resolution, subscription receivers, and replay protocol tests.
- **M3** delivered disclosure/redaction policy, artifact handles, bounded reads, and negative persistence tests.
- **M4** delivered the shared projection controller and local/remote frontend adoption.
- **M5** delivered connection-local subscription ownership, exact stream identity, typed resume/ack/unsubscribe/status/artifact operations, raw projection filtering, bounded queues, and disconnect cleanup.
- **M6 implementation** delivered critical writer receipts, activation-after-delivery, rollback, normal-flow transport isolation, raw scoping, and bounded legacy `/ws` output.
- **M7 implementation** delivered joined Unix raw lifecycle, TUI route generations, deterministic lifecycle seams, blocked-response tests, foreign-operation coverage, reconnect fixtures, and evidence reconciliation.
- **M8 implementation** delivered shared joined `/core` and `/tui` task ownership plus exact envelope-level replay continuity.
- **M9 implementation** delivered connection probes, real WebSocket peer-close and churn coverage, two-client continuity, additional cancellation fixtures, exact `/core` interrupted replay retry, and fresh `/core` connection identity.

### Conditional closure history

M6 and M7 remain valid implementation foundations with historical conditional records. Their principal production findings were addressed by later milestones.

M8 materially fixed the final known WebSocket abort-without-await defect and strengthened exact replay evidence. Its closure remains conditional because M9 was required for production-shaped verification.

M9 materially improved WebSocket lifecycle evidence but remains conditionally closed after source inspection found mechanism and evidence defects:

- the `/core` queue test does not fill the actual queue or directly assert the production timeout result;
- `/tui` has no actual bounded-queue saturation fixture;
- Unix peer-close/write/flush/race/interrupted-replay fixtures are absent;
- shared first-exit coverage is send-first only and adapter raw-first tests do not terminate the raw source first;
- TUI pending setup/replay is not interrupted before canonical response completion;
- the complete rollback helper is incomplete and not consistently used;
- static guards are mostly name-oriented;
- M009 plan, closure, registry, commits, test counts, commands, and CI evidence are inconsistent.

Authoritative documents:

- M6 conditional record: `plans/closure/session-projections/006-status.md`
- M7 conditional record: `plans/closure/session-projections/007-status.md`
- M8 conditional record: `plans/closure/session-projections/008-status.md`
- M9 conditional record: `plans/closure/session-projections/009-status.md`
- M10 final handoff: `plans/implementation/session-projections/010-mechanism-faithful-transport-verification-and-final-closure.md`

M10 does not reopen projection storage, reducer semantics, disclosure policy, replay authority, cursor/sequence meaning, or protocol DTOs.

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
|-- mark-live only after canonical response delivery success
|-- owned send/receive/raw task set
|-- one owned receiver-forwarder per live subscription
|-- generation-scoped TUI raw routing
|-- session/filter-scoped raw compatibility
|-- connection cancellation before task teardown
`-- deterministic rollback/unsubscribe/abort-and-await cleanup

Mechanism-faithful verification
|-- connection-local queue capacity and writer controls
|-- fill-to-Full observation
|-- direct CriticalDeliveryError::Timeout assertion
|-- explicit first terminal task kind
|-- actual raw-source termination
|-- real WebSocket and Unix peer/socket failure
|-- pending-response interruption before delivery success
|-- complete rollback harness
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
M8 joined WebSocket teardown and exact replay evidence       [conditionally closed; principal findings addressed]
        |
        v
M9 production-shaped WebSocket verification                 [conditionally closed; mechanism gaps remain]
        |
        v
M10 mechanism-faithful queues, Unix races, rollback closure [ready]
```

M10 has no unmet design dependency. It consumes the M8 task owner and replay fixtures plus the M9 probes, churn, peer-close, continuity, and interrupted-replay foundations.

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

Status: conditionally closed; principal task-join and replay findings addressed.

- Plan: `plans/implementation/session-projections/008-final-transport-lifecycle-and-replay-evidence-polish.md`
- Closure: `plans/closure/session-projections/008-status.md`
- Implementation: `6975050af530eb5bd7a640c1f7ac9a31859dfda3`

Accepted outcomes include shared joined WebSocket task ownership, joined Unix raw/client lifecycle, bounded critical delivery, activation-after-delivery, route-generation stale rejection, and exact replay-to-live sequence continuity.

### Milestone 9 — Production-shaped transport verification and strict closure

Status: conditionally closed; principal findings resolved by M10.

- Plan: `plans/implementation/session-projections/009-production-shaped-transport-verification-and-strict-closure.md`
- Conditional closure: `plans/closure/session-projections/009-status.md`
- Implementation/evidence: `3406c742a23b6470def32fb7a04cdc7b72a40dea`, `426dfffec05c9d694f54a816213a6cca514e91b4`

Accepted outcomes include connection-local probes, real WebSocket peer close/drop, 100-cycle `/core` and `/tui` churn, two-client continuity, exact `/core` interrupted replay retry, successful TUI replay durability, and fresh `/core` connection identity.

Residual findings (queue fill, task-kind, Unix fixtures, TUI pending interruption, rollback harness, semantic guards, exact closure evidence) are addressed by M10 closure: `plans/closure/session-projections/010-status.md`.

### Milestone 10 — Mechanism-faithful transport verification and final closure

Status: closed.

Implementation plan:

- `plans/implementation/session-projections/010-mechanism-faithful-transport-verification-and-final-closure.md`

Closure: `plans/closure/session-projections/010-status.md`

Class: verification correction / bounded-queue mechanics / Unix transport races / lifecycle observability / closure reconciliation

Repository baseline: `426dfffec05c9d694f54a816213a6cca514e91b4`

Objective: replace nominal queue, first-exit, TUI interruption, Unix lifecycle, rollback, guard, and evidence claims with direct production-mechanism proof.

Required deliverables (all accepted):

- connection-local queue capacity, writer, raw-source, and first-task controls;
- actual `/core` and `/tui` fill-to-full tests with direct production `Timeout` result assertion;
- six-case task-owner first-exit/panic matrix;
- real raw-source-first adapter fixtures;
- true TUI snapshot/replay pending-delivery interruption;
- real Unix peer-close, write/flush, cancellation/completion race, churn, fresh identity, and interrupted replay retry;
- complete rollback harness used by every real failure fixture;
- semantic lifecycle guards;
- exact full verification and M010 closure evidence.

Exit conditions (all met):

- actual queue fullness causes and directly returns the production timeout in both WebSocket adapters;
- all task first-exit and panic cases cancel and join siblings;
- actual raw-source termination selects raw-event-first;
- WebSocket and Unix peer/socket failures prove complete cleanup;
- TUI canonical setup/replay is interrupted before delivery success;
- Unix replay interruption retains durable history and retries exactly;
- every real failure invokes the complete rollback harness;
- all transient resources and unrelated clients satisfy baseline/non-interference invariants;
- focused suites and guards pass;
- closure evidence matches exact commits, tests, counts, commands, outputs, and CI status;
- no unresolved high or medium M10 finding remains;
- M10 closure returns roadmap and registry to strict closed status.

## 7. Deferred product work

These items are outside M10:

- cross-tab artifact hand-off UX;
- numeric acknowledgement/resync hot-key UX;
- plugin-specific `ProjectionEvent::PluginUi` semantics;
- final team principals/roles/presence/chat;
- cross-daemon replay replication;
- removal of version-4 compatibility before its compatibility window expires.

## 8. Cross-cutting requirements

### Protocol and compatibility

Do not increment the projection protocol or remove version-4 compatibility. M10 changes test controls, verification depth, and closure evidence—not wire meaning.

### Security and authorization

Connection authentication is not subscription ownership. Foreign lifecycle and artifact operations remain fail-closed. Probes must not retain payload bodies, artifact bytes, hidden reasoning, or secrets.

### Concurrency, cancellation, and recovery

Persistence and receiver installation precede response staging; successful response delivery precedes live release. Connection cancellation precedes sibling teardown. Every retained task is awaited. No state lock is held across socket I/O or task join. Test controls are connection-local and bounded.

### Performance

Production queue capacities and critical-send behavior remain unchanged. Test-only capacity reduction, gates, and observers must add no production queue growth, per-event task creation, or runtime-global mutable state.

### Documentation

Maintain transport state-machine, task ownership, queue, route-generation, replay continuity, compatibility, mechanism-verification, and test matrices. Closure evidence must include exact implementation and closure commits.

## 9. Verification strategy

Use:

- connection-local capacity-1/capacity-2 queue fixtures;
- explicit fill-until-Full assertions;
- direct adapter `Timeout` result assertions;
- real WebSocket and Unix peer/socket failure;
- deterministic send/receive/raw/panic first-exit tests;
- actual raw-source termination;
- pending TUI response interruption before delivery success;
- task/drop/forwarder/receiver/ownership baselines;
- 100-cycle WebSocket churn and repeated Unix races;
- two-client non-interference;
- exact reconnect and interrupted replay retry;
- existing replay, disclosure, artifact, TUI, raw compatibility, and static guards.

Injected lifecycle seams remain valid for boundary reachability, serialization-equivalent failure, and deterministic pre-activation rollback. They are not substitutes for real queue, raw-source, or socket mechanisms.

## 10. Completion definition

This roadmap returns to strict closed status when all three production transports are bounded and connection-owned at subscription and task levels; actual channel fullness, raw-source termination, peer/socket failure, and pending-response interruption prove complete cleanup; replay interruption preserves durable history; all transient resources return to baseline; and closure evidence precisely matches the executable repository.

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
| 8 | conditionally closed | `plans/implementation/session-projections/008-final-transport-lifecycle-and-replay-evidence-polish.md` | `plans/closure/session-projections/008-status.md` | principal findings addressed by M9/M10 |
| 9 | conditionally closed | `plans/implementation/session-projections/009-production-shaped-transport-verification-and-strict-closure.md` | `plans/closure/session-projections/009-status.md` | All principal findings addressed by M10 |
| 10 | closed | `plans/implementation/session-projections/010-mechanism-faithful-transport-verification-and-final-closure.md` | `plans/closure/session-projections/010-status.md` | — |