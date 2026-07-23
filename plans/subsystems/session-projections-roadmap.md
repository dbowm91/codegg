# Frontend-Neutral Session Projections and Replay Roadmap

Status: closed — Milestone 012 accepted

Long-term references:

- `plans/000-long-term-specification.md#8-read-only-session-observation`
- `plans/000-long-term-specification.md#14-acp-integration`
- `plans/001-terminology-and-domain-model.md` — session, turn, observation subscription, projection
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Related ADRs:

- None required for M012 unless implementation changes replay authority, persistence ownership, public projection protocol meaning, deployment-global ordering, or retention semantics.

## 1. Purpose and ownership boundary

This subsystem owns the canonical frontend-neutral session representation and snapshot/event/replay semantics used by local TUI, remote TUI, observer mode, ACP adapters, web clients, and future frontends.

It owns:

- bounded versioned projection DTOs;
- reducer and snapshot semantics;
- scoped subscription ownership;
- sequence, acknowledgement, resume, and resynchronization behavior;
- durable replay indexing and cursor authority;
- visibility/redaction classification and artifact handles;
- transport delivery, task ownership, cancellation, rollback, and replay-to-live invariants.

It consumes stable project/session identities, daemon events, turn/tool/run/job state, project-aware TUI state, session persistence, and protocol capability negotiation.

It does not own team authorization policy, presence/chat, full audit retention, raw provider hidden reasoning, cross-daemon replay replication, or deployment-global ordering.

## 2. Subsystem invariants

### Projection and replay

- All frontends reconstruct equivalent logical state from the same canonical snapshot and event stream.
- Sequence ordering is monotonic within one stream.
- Replay is deterministic and delivers the exact missing committed range once.
- The first live envelope after replay is `replay_end_seq + 1` without a gap, duplicate, or reorder.
- Expired or missing history produces bounded typed resynchronization.
- Disconnect removes transient ownership without deleting durable replay history.
- Stream ID, subscription ID, project ID, session ID, client ID, and transport connection ID remain distinct.

### Disclosure and bounds

- Secret-bearing data is redacted before shared durable storage.
- Large logs, artifacts, and file bodies remain behind bounded handles.
- Raw terminal frames are not the canonical collaboration protocol.
- Unknown versions and variants degrade safely.
- Raw compatibility is explicitly scoped by connection/session/filter selection.
- No WebSocket adapter uses an unbounded outbound queue.
- No TUI request pipeline may be unbounded or create one detached task per message.

### Delivery and activation

- A subscription cannot become live before its canonical snapshot/replay response is successfully delivered.
- Critical control failure rolls back connection ownership and daemon subscription state.
- Queue-timeout evidence must establish fullness before the target operation and directly attribute the typed result to that operation.
- Observer-enabled and observer-disabled delivery use the same production implementation and timeout budget.
- Peer-failure evidence must observe the real peer or production I/O operation before the named response completes.

### Connection lifecycle

- Exactly one task owns and polls each socket read half.
- Socket Close, EOF, and read error remain observable while lifecycle handling is pending.
- Connection cancellation occurs before sibling teardown.
- Every connection-scoped writer, socket reader, request handler, raw-event task, and projection forwarder has explicit bounded ownership and joined teardown.
- Panic in any owned task does not prevent cancellation and joining of siblings.
- Every real failure returns connection ownership, daemon ownership, receivers, tasks, forwarders, queues, probes, and diagnostic counters to baseline.
- An unrelated client remains live and receives its own unique marker event through another client’s failure.

### Evidence integrity

- Closure evidence names exact commits, tests, counts, commands, repetitions, mechanisms, exceptions, and CI availability truthfully.
- Local execution is not described as CI.
- A reduced flake rate cannot satisfy closure.
- A milestone cannot be strictly closed while a reproducible production deadlock, resource leak, or unresolved high/medium finding remains.

## 3. Current state

### Closed implementation foundations

- **M1** delivered projection contracts, capability negotiation, reducer semantics, snapshots, and independent-consumer fixtures.
- **M2** delivered durable replay storage, cursor validation, retention/checkpoints, daemon publication, subscription receivers, and replay protocol tests.
- **M3** delivered disclosure/redaction policy, artifact handles, bounded reads, and negative persistence tests.
- **M4** delivered the shared projection controller and local/remote frontend adoption.
- **M5** delivered connection-local ownership, exact stream identity, lifecycle operations, bounded queues, compatibility routing, and disconnect cleanup.

### Conditional implementation history

- **M6** added atomic critical control delivery and transport hardening.
- **M7** added lifecycle seams, joined Unix raw lifecycle, route generations, race fixtures, and evidence reconciliation.
- **M8** added shared joined WebSocket task ownership and exact replay-to-live continuity.
- **M9** added connection probes, real WebSocket close/churn coverage, two-client continuity, and interrupted replay retry.
- **M10** added connection-local queue/writer controls, first-task recording, raw-source controls, TUI writer barriers, broader Unix fixtures, and repeated transport runs.
- **M11** added per-connection probes, operation-correlated queue observations, symmetric `/core` and `/tui` saturation fixtures, six production-path task-owner cases, TUI raw-source-first, Unix F1–F5 regression coverage, and stronger guards.

M6–M11 remain valid implementation foundations with historical conditional closure records. M12 addressed the remaining lifecycle and evidence findings; strict closure is now recorded by `plans/closure/session-projections/012-status.md`.

### M011 post-closure findings

The final reviewed M011 head `1a93167ee3bdfdc55e4bd2746180443cc19b7c96` records a confirmed `/tui` production lifecycle defect:

- the receive task awaits projection handling inline and cannot poll `ws_rx` while a response is pending;
- the writer may remain parked;
- no task may fire `connection_cancel` after peer close;
- the connection owner may never enter joined teardown;
- daemon subscription ownership may leak;
- the pending-snapshot fixture still fails approximately 2 of 10 runs after the writer-side mitigation.

Further review found:

- observer-enabled staged delivery is a separate two-budget implementation rather than instrumentation of the canonical one-budget path;
- Unix I/O tests infer failure from EOF/cleanup without asserting the server-side typed error;
- rollback helpers omit forwarder joins and complete ownership/leakage assertions;
- TUI rollback documentation incorrectly denies daemon-side subscription ownership;
- synthetic subscription identity is used in one rollback fixture;
- probe registration can silently fail through `try_lock()`;
- elapsed time is used as sibling-join proof;
- M011 plan, closure, roadmap, registry, stability evidence, and CI status were contradictory.

Authoritative records:

- M6: `plans/closure/session-projections/006-status.md`
- M7: `plans/closure/session-projections/007-status.md`
- M8: `plans/closure/session-projections/008-status.md`
- M9: `plans/closure/session-projections/009-status.md`
- M10: `plans/closure/session-projections/010-status.md`
- M11 corrected conditional record: `plans/closure/session-projections/011-status.md`
- M12 handoff: `plans/implementation/session-projections/012-tui-disconnect-lifecycle-and-final-evidence-closure.md`
- M12 accepted closure: `plans/closure/session-projections/012-status.md`

M12 does not reopen projection storage, reducer semantics, disclosure policy, replay authority, cursor/sequence meaning, or public protocol DTOs.

## 4. Target lifecycle architecture

```text
TUI WebSocket read half
    |
    v
owned socket-reader task
    |-- observes Text / Close / EOF / error
    |-- fires connection_cancel on terminal peer state
    |
    `-- bounded ordered request queue
                    |
                    v
             owned request-handler task
                    |
                    |-- daemon lifecycle operation
                    |-- canonical one-budget critical send
                    `-- cancellation-aware pending work

connection owner
    |-- writer task
    |-- socket-reader task
    |-- request-handler task
    |-- raw-event task
    |-- owned projection forwarders
    |-- per-connection probe/final record
    `-- cancel -> abort as required -> await all -> rollback once
```

The reader remains able to detect peer termination regardless of handler progress. Request processing remains sequential and bounded.

## 5. Dependency graph

```text
M1 projection contracts and reducer                         [closed]
        |
        v
M2 durable replay and daemon integration                    [closed]
        |
        v
M3 disclosure, redaction, artifact handles                  [closed]
        |
        v
M4 frontend adoption and controller                         [closed]
        |
        v
M5 transport isolation, resume, compatibility               [closed]
        |
        v
M6 atomic control delivery                                  [conditionally closed]
        |
        v
M7 lifecycle ownership and route epochs                     [conditionally closed]
        |
        v
M8 joined teardown and replay continuity                    [conditionally closed]
        |
        v
M9 production-shaped verification                           [conditionally closed]
        |
        v
M10 mechanism-oriented transport verification               [conditionally closed]
        |
        v
M11 evidence correctness and broader mechanism tests        [conditionally closed]
        |
        v
M12 TUI disconnect lifecycle and final evidence closure     [closed]
```

M12 has no unmet design dependency. It consumes the accepted M8 task owner, M9 lifecycle/replay fixtures, M10 writer/queue controls, and M11 probes/observations/Unix regression coverage.

## 6. Milestones

### Milestones 1–5

Status: closed.

- M1 plan/closure: `plans/implementation/session-projections/001-projection-contracts.md`, `plans/closure/session-projections/001-status.md`
- M2 closure: `plans/closure/session-projections/002-status.md`
- M3 plan/closure: `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md`, `plans/closure/session-projections/003-status.md`
- M4 plan/closure: `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md`, `plans/closure/session-projections/004-status.md`
- M5 plan/closure: `plans/implementation/session-projections/005-remote-transport-isolation-resume-closure.md`, `plans/closure/session-projections/005-status.md`

### Milestones 6–10

Status: conditionally closed historical foundations.

- M6: `plans/implementation/session-projections/006-atomic-control-delivery-transport-verification-hardening.md`
- M7: `plans/implementation/session-projections/007-corrective-transport-lifecycle-and-evidence-closure.md`
- M8: `plans/implementation/session-projections/008-final-transport-lifecycle-and-replay-evidence-polish.md`
- M9: `plans/implementation/session-projections/009-production-shaped-transport-verification-and-strict-closure.md`
- M10: `plans/implementation/session-projections/010-mechanism-faithful-transport-verification-and-final-closure.md`

Their accepted production work remains in force. Their closure records remain historical and conditional.

### Milestone 11 — Evidence correctness and mechanism verification

Status: conditionally closed.

- Plan: `plans/implementation/session-projections/011-evidence-correctness-and-mechanism-verification-closure.md`
- Corrected closure: `plans/closure/session-projections/011-status.md`
- Baseline: `8bd59b22662a289f3124c9b3113e545faa9446d7`
- Final reviewed head: `1a93167ee3bdfdc55e4bd2746180443cc19b7c96`

Accepted outcomes:

- per-connection probe foundation;
- queue-full-before-request fixtures for both WebSocket adapters;
- operation-correlated observation model;
- production task-owner test wrapper and six clean/panic cases;
- raw-source-first coverage for both adapters;
- Unix F1–F5 regression fixtures;
- writer-gate and writer cancellation improvements;
- expanded lifecycle guards.

Strict closure is blocked by M12 findings.

### Milestone 12 — TUI disconnect lifecycle and final evidence closure

Status: closed.

Plan:

- `plans/implementation/session-projections/012-tui-disconnect-lifecycle-and-final-evidence-closure.md`

Baseline:

- `1a93167ee3bdfdc55e4bd2746180443cc19b7c96`

Primary deliverables:

- close-responsive TUI socket reader independent of pending handler work;
- bounded sequential request-handler pipeline;
- explicit joined ownership for reader, handler, writer, raw task, and projection forwarders;
- one canonical one-budget staged critical-send implementation with optional observation;
- typed Unix production I/O observations;
- real staged subscription identities in rollback fixtures;
- complete ownership, receiver, task, forwarder, queue, probe, leakage, and unrelated-client assertions;
- infallible identity-correlated probe registration;
- direct completion/drop-based task-join proof;
- semantic guards and exact final evidence reconciliation.

M12 closure criteria C1–C18 are satisfied by the accepted closure record. The subsystem has returned to strict closed status; deferred product work remains outside this milestone.

## 7. Verification and closure policy

M12 closure requires:

- graceful Close and abrupt-drop pending-snapshot fixtures at 50/50 clean runs each;
- pending-replay close/retry at 50/50 clean runs;
- full `projection_transport_real` binary at 25 consecutive clean runs;
- Unix typed-I/O fixtures at 25 consecutive clean runs;
- repeated Unix convergence with no resource growth;
- complete focused and regression matrix;
- exact source audit at the final reviewed head;
- truthful distinction between local execution and attached CI.

Create final closure record only after implementation and verification:

- `plans/closure/session-projections/012-status.md`

The roadmap returns to strict closed status only when:

1. every M12 C1–C18 criterion passes;
2. no unresolved high or medium finding remains;
3. the closure record contains exact implementation, corrective, closure, and reviewed-head commits;
4. plan, closure, roadmap, and registry statuses agree;
5. no known flaky closure-bearing fixture remains.

## 8. Deferred product work

The following remains outside M12:

- cross-tab artifact hand-off UX;
- numeric acknowledgement/resync hot-key UX;
- plugin-specific `ProjectionEvent::PluginUi` semantics;
- team roles, presence, and chat;
- cross-daemon replay replication;
- version-4 compatibility removal before the compatibility window expires.

## 9. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blocker |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/session-projections/001-projection-contracts.md` | `plans/closure/session-projections/001-status.md` | — |
| 2 | closed | library and corrective plans | `plans/closure/session-projections/002-status.md` | — |
| 3 | closed | `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md` | `plans/closure/session-projections/003-status.md` | — |
| 4 | closed | `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md` | `plans/closure/session-projections/004-status.md` | — |
| 5 | closed | `plans/implementation/session-projections/005-remote-transport-isolation-resume-closure.md` | `plans/closure/session-projections/005-status.md` | — |
| 6 | conditionally closed | `plans/implementation/session-projections/006-atomic-control-delivery-transport-verification-hardening.md` | `plans/closure/session-projections/006-status.md` | historical; later milestones own final depth |
| 7 | conditionally closed | `plans/implementation/session-projections/007-corrective-transport-lifecycle-and-evidence-closure.md` | `plans/closure/session-projections/007-status.md` | historical; later milestones own final depth |
| 8 | conditionally closed | `plans/implementation/session-projections/008-final-transport-lifecycle-and-replay-evidence-polish.md` | `plans/closure/session-projections/008-status.md` | historical; later milestones own final depth |
| 9 | conditionally closed | `plans/implementation/session-projections/009-production-shaped-transport-verification-and-strict-closure.md` | `plans/closure/session-projections/009-status.md` | historical; later milestones own final depth |
| 10 | conditionally closed | `plans/implementation/session-projections/010-mechanism-faithful-transport-verification-and-final-closure.md` | `plans/closure/session-projections/010-status.md` | M11/M12 final evidence depth |
| 11 | conditionally closed | `plans/implementation/session-projections/011-evidence-correctness-and-mechanism-verification-closure.md` | `plans/closure/session-projections/011-status.md` | historical conditional; resolved by M12 |
| 12 | closed | `plans/implementation/session-projections/012-tui-disconnect-lifecycle-and-final-evidence-closure.md` | `plans/closure/session-projections/012-status.md` | — |
