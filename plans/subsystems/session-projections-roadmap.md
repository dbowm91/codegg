# Frontend-Neutral Session Projections and Replay Roadmap

Status: active — Milestone 011 evidence correctness and mechanism verification closure

Long-term references:

- `plans/000-long-term-specification.md#8-read-only-session-observation`
- `plans/000-long-term-specification.md#14-acp-integration`
- `plans/001-terminology-and-domain-model.md` — session, turn, observation subscription, projection
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Related ADRs:

- None required for M011. Create an ADR only if work changes replay authority, persistence ownership, deployment-global ordering, retention semantics, or the public projection protocol.

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
- Transports preserve the persisted stream ID and never synthesize it from a subscription ID.
- Generic raw event broadcasts never carry subscription-private projection events.
- A subscription cannot become live before its canonical snapshot/replay response is successfully delivered.
- Critical control failure rolls back connection ownership and daemon subscription state.
- Every connection-scoped send, receive, raw, and projection task has explicit ownership, cancellation, and joined teardown.
- Disconnect removes transient receivers and tasks without deleting replay history.
- Transport queues and per-connection tasks remain bounded.
- Raw compatibility traffic is explicitly scoped by connection/session/filter selection.
- Changing a TUI raw session route invalidates queued events from prior route generations.
- Reconnect replay delivers exactly the missing committed envelopes once and transitions to the next live sequence without a gap or duplicate.
- A queue-timeout claim must correlate observed fullness to the exact production send and identify whether timeout occurred before enqueue or during writer receipt.
- A peer-failure claim must fail the real peer or production I/O operation before the named response completes.
- A raw-source-first claim must terminate the actual raw source while the peer remains healthy and record `RawEvent` as first exit.
- Task-owner evidence must execute the production cancel/abort-and-await path and prove sibling joins.
- Every real staged failure returns per-connection tasks, forwarders, receivers, ownership, queues, and daemon subscriptions to baseline.
- Unrelated clients remain live through another client's failure.
- No WebSocket server adapter uses an unbounded outbound queue.
- Closure evidence names exact tests, commands, counts, commits, mechanisms, repetitions, exceptions, and CI availability truthfully.

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
- **M9 implementation** delivered connection probes, real WebSocket peer-close and churn coverage, two-client continuity, cancellation fixtures, exact `/core` interrupted replay retry, and fresh `/core` connection identity.
- **M10 implementation** delivered connection-local transport controls, capacity-one `Full` observation, writer barriers, first-task-kind recording, `/core` raw-source control, TUI pending-response interruption, broader Unix regression fixtures, a full focused local matrix, and a probe scheduling-flake correction.

### Conditional closure history

M6–M8 remain valid implementation foundations with historical conditional records. Their principal production findings were addressed by later milestones.

M9 remains conditionally closed; M10 addressed many of its verification gaps but did not establish every mechanism causally.

M10 remains conditionally closed because post-closure review found evidence-attribution and completeness defects rather than a new architecture defect:

- `/core` queue timeout may occur during writer receipt rather than blocked enqueue on a full queue;
- `/tui` actual full-queue timeout is absent;
- the six-case task-owner matrix does not execute all clean/panic cases through production joins;
- `/tui` raw-source-first remains peer-close coverage;
- Unix fixtures do not prove pre-response actual I/O failure, forced completion/cancellation orders, interrupted second replay, or repeated convergence;
- rollback assertions are incomplete and may use aggregate probes shared by multiple connections;
- guards do not reject these false-positive patterns;
- closure, registry, final commits, and CI evidence required correction.

Authoritative documents:

- M6 conditional record: `plans/closure/session-projections/006-status.md`
- M7 conditional record: `plans/closure/session-projections/007-status.md`
- M8 conditional record: `plans/closure/session-projections/008-status.md`
- M9 conditional record: `plans/closure/session-projections/009-status.md`
- M10 conditional record: `plans/closure/session-projections/010-status.md`
- M11 handoff: `plans/implementation/session-projections/011-evidence-correctness-and-mechanism-verification-closure.md`

M11 does not reopen projection storage, reducer semantics, disclosure policy, replay authority, cursor/sequence meaning, or protocol DTOs.

## 4. Target verification boundary

```text
Projection store / replay service
|-- persisted stream identity
|-- sequence and cursor authority
|-- retained events/checkpoints
|-- disclosure/artifact policy
`-- daemon-issued subscription receiver

Transport connection
|-- trusted client/connection identity
|-- bounded owned-subscription map
|-- staged Initializing subscription
|-- bounded critical sender
|-- live only after canonical delivery success
|-- owned send/receive/raw task set
|-- owned projection forwarders
|-- cancellation before sibling teardown
`-- idempotent rollback and joined cleanup

M011 evidence chain
|-- per-connection probe
|-- exact mechanism precondition
|-- operation-correlated observation
|-- exact typed result
|-- production task teardown
|-- ownership/receiver/forwarder convergence
|-- unrelated-client continuity
|-- replay durability and identity
|-- repeated stability runs
`-- exact closure and CI/local evidence
```

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
M9 production-shaped transport verification                 [conditionally closed; principal findings addressed]
        |
        v
M10 transport instrumentation and broader mechanism tests   [conditionally closed; evidence defects remain]
        |
        v
M11 evidence correctness and final verification closure     [ready]
```

M11 has no unmet design dependency. It consumes M8 task ownership, M9 lifecycle/replay fixtures, and M10 writer gates, observers, raw-source controls, and first-task recording.

## 6. Milestones

### Milestones 1–5

Status: closed.

- M1: `plans/implementation/session-projections/001-projection-contracts.md`
- M2: durable replay library and corrective daemon integration plans; closure `plans/closure/session-projections/002-status.md`
- M3: `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md`
- M4: `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md`
- M5: `plans/implementation/session-projections/005-remote-transport-isolation-resume-closure.md`

### Milestone 6 — Atomic control delivery and transport hardening

Status: conditionally closed; principal findings addressed by later milestones.

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

### Milestone 9 — Production-shaped transport verification

Status: conditionally closed; principal findings addressed by M10/M11.

- Plan: `plans/implementation/session-projections/009-production-shaped-transport-verification-and-strict-closure.md`
- Closure: `plans/closure/session-projections/009-status.md`
- Implementation/evidence: `3406c742a23b6470def32fb7a04cdc7b72a40dea`, `426dfffec05c9d694f54a816213a6cca514e91b4`

### Milestone 10 — Mechanism-faithful transport verification

Status: conditionally closed; strict evidence superseded by M11.

- Plan: `plans/implementation/session-projections/010-mechanism-faithful-transport-verification-and-final-closure.md`
- Closure: `plans/closure/session-projections/010-status.md`
- Final reviewed M10 head: `8bd59b22662a289f3124c9b3113e545faa9446d7`

Accepted M10 outcomes:

- connection-local capacity and writer controls;
- pre-`recv()` full-channel observation;
- lifecycle observers and first-task-kind recording;
- `/core` raw-source cancellation;
- TUI pending snapshot/replay interruption;
- additional Unix lifecycle/replay regression tests;
- full focused local matrix;
- repeated-run scheduling-flake correction.

Residual evidence findings are owned exclusively by M11.

### Milestone 11 — Evidence correctness and mechanism verification closure

Status: ready.

Implementation plan:

- `plans/implementation/session-projections/011-evidence-correctness-and-mechanism-verification-closure.md`

Class: verification correction / mechanism attribution / per-connection evidence / Unix I/O races / closure integrity

Repository baseline: `8bd59b22662a289f3124c9b3113e545faa9446d7`

Objective: prove each failure claim through a causal chain of exact precondition, exact production operation, exact typed result, and complete per-connection convergence.

Required deliverables:

- one probe per upgraded connection;
- operation-correlated critical-send observations;
- deterministic `/core` and `/tui` full-queue enqueue timeout fixtures;
- six production-path task-owner clean/panic first-exit tests;
- real raw-source-first for both adapters;
- actual Unix pre-response I/O failure, forced completion/cancellation orders, interrupted replay, and repeated convergence;
- complete rollback/non-interference harness used by every real failure fixture;
- guards rejecting M10 false-positive patterns;
- full verification matrix and 25-run stability loops;
- exact M10/M11 planning and closure evidence.

Exit conditions:

- both WebSocket adapters prove fullness before target send and an enqueue-stage `Timeout` for that operation;
- all six task-owner cases execute production cancellation and joins;
- both adapters prove raw-source-first while peer remains healthy;
- Unix tests use actual peer-induced I/O errors and interrupt the resumed replay response;
- forced Unix race orders converge identically over repeated cycles;
- all closure-relevant failures use per-connection complete rollback assertions;
- unrelated clients remain live;
- guards reject prior ambiguous/injected patterns;
- complete matrix and stability loops pass;
- exact commits and CI/local evidence are reconciled;
- no unresolved high or medium M11 finding remains;
- M11 closure returns the roadmap and registry to strict closed status.

## 7. Deferred product work

These items remain outside M11:

- cross-tab artifact hand-off UX;
- numeric acknowledgement/resync hot-key UX;
- plugin-specific `ProjectionEvent::PluginUi` semantics;
- final team principals/roles/presence/chat;
- cross-daemon replay replication;
- removal of version-4 compatibility before its compatibility window expires.

## 8. Cross-cutting requirements

### Protocol and compatibility

Do not increment the projection protocol or remove version-4 compatibility. M11 changes test instrumentation, verification, and closure evidence—not wire meaning.

### Security and authorization

Connection authentication is not subscription ownership. Foreign lifecycle and artifact operations remain fail-closed. Probes retain no payload bodies, artifact bytes, hidden reasoning, or secrets.

### Concurrency, cancellation, and recovery

Persistence and receiver installation precede response staging; successful response delivery precedes live release. Connection cancellation precedes sibling teardown. Every retained task is awaited. No state lock is held across socket I/O or task join. Test controls remain connection-local and bounded.

### Performance

Production queue capacities and timeout behavior remain unchanged. Test controls must add no production queue growth, per-event task creation, or runtime-global mutable state.

### Documentation

Closure evidence must include exact implementation, follow-up, closure, and reviewed-head commits; exact tests/counts/commands; repeated-run results; and truthful CI availability.

## 9. Verification strategy

Use:

- operation-correlated queue observations;
- full-before-request ordering;
- per-connection probe ownership;
- production-path first-exit teardown tests;
- real raw-source termination for both adapters;
- actual Unix peer-induced I/O errors;
- forced completion-first/cancellation-first barriers;
- interrupted second replay and third-connection retry;
- complete rollback and unrelated-client assertions;
- 100-cycle WebSocket churn, 50-cycle Unix convergence, and 25 repeated suite runs;
- existing replay, disclosure, artifact, TUI, raw compatibility, and boundary guards.

Injected lifecycle seams remain valid for boundary reachability and serialization-equivalent tests. They cannot satisfy real I/O or queue-mechanism closure requirements.

## 10. Completion definition

This roadmap returns to strict closed status when the same fixture proves the exact mechanism precondition, invokes the exact production operation, observes the exact typed result, and verifies complete per-connection convergence for all closure-critical paths; all three transports satisfy replay and failure invariants; guards reject prior false-positive patterns; and closure evidence matches executable and independently available evidence.

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
| 8 | conditionally closed | `plans/implementation/session-projections/008-final-transport-lifecycle-and-replay-evidence-polish.md` | `plans/closure/session-projections/008-status.md` | principal findings addressed by M9–M11 |
| 9 | conditionally closed | `plans/implementation/session-projections/009-production-shaped-transport-verification-and-strict-closure.md` | `plans/closure/session-projections/009-status.md` | principal findings addressed by M10/M11 |
| 10 | conditionally closed | `plans/implementation/session-projections/010-mechanism-faithful-transport-verification-and-final-closure.md` | `plans/closure/session-projections/010-status.md` | M11 evidence correctness and final closure |
| 11 | ready | `plans/implementation/session-projections/011-evidence-correctness-and-mechanism-verification-closure.md` | — | — |