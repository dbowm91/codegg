# Frontend-Neutral Session Projections and Replay Roadmap

Status: active — Milestone 008 final transport lifecycle and replay evidence polish

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
- Every connection-scoped send, receive, raw, and projection task has explicit ownership, cancellation, and joined teardown.
- Disconnect removes transient receivers and tasks without deleting replay history.
- Transport queues and per-connection tasks remain bounded.
- Raw compatibility traffic is explicitly scoped by connection/session/filter selection.
- Changing a TUI raw session route invalidates queued events from prior route generations.
- Reconnect replay delivers exactly the missing committed envelopes once and transitions to the next live sequence without a gap or duplicate.
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
- **M6 implementation** landed at `8ca570f`: critical writer receipts, activation-after-delivery transitions, rollback paths, normal-flow transport isolation tests, raw compatibility scoping, and bounded legacy `/ws` output.
- **M7 implementation** landed at `9887c2d`: joined Unix raw lifecycle, TUI route generations, deterministic lifecycle seams, blocked-response tests, expanded foreign-operation coverage, reconnect fixtures, and closure-evidence reconciliation.

### Conditional closure history

M6 remains a valid implementation foundation. Its historical closure record is conditionally closed because M7 was required to resolve Unix task ownership, TUI route generations, production-shaped race evidence, foreign-operation coverage, reconnect fixtures, and evidence reconciliation.

M7 resolved those findings materially. Post-closure inspection identified three narrower polish items:

- `/core` and `/tui` abort sibling connection tasks but do not await the aborted handles before completing connection cleanup;
- helper and selected adapter tests do not yet constitute the complete staged-subscription queue/cancellation/serialization/disconnect matrix prescribed by M7;
- reconnect fixtures verify replay range metadata and a subsequent live subscription ID, but do not directly prove full envelope identity, exact sequence continuity, and absence of duplication.

Authoritative documents:

- M6 conditional record: `plans/closure/session-projections/006-status.md`
- M7 conditional record: `plans/closure/session-projections/007-status.md`
- M8 final polish handoff: `plans/implementation/session-projections/008-final-transport-lifecycle-and-replay-evidence-polish.md`

M8 does not reopen storage, reducer, disclosure, replay authority, sequence semantics, or protocol DTO meaning.

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
|-- owned send/receive/raw task set
|-- one owned receiver-forwarder per live subscription
|-- generation-scoped TUI raw routing
|-- session/filter-scoped raw compatibility
|-- connection cancellation before task teardown
`-- deterministic rollback/unsubscribe/abort-and-await cleanup

Replay-to-live handoff
|-- stable stream identity
|-- new connection and subscription identity
|-- exact missing envelope range
|-- monotonic sequence continuity
`-- first live envelope = replay_end_seq + 1, no duplicates

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
M6 atomic control delivery and transport hardening          [conditionally closed; findings addressed by M7]
        |
        v
M7 lifecycle ownership, route epochs, race evidence         [conditionally closed; final polish required]
        |
        v
M8 joined WebSocket teardown and exact replay evidence       [ready]
```

M8 has no unmet design dependency. It consumes the M6 critical-send/activation state machine and the M7 lifecycle seam, route generation, foreign-operation, and reconnect foundations.

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

Status: conditionally closed; corrective findings were materially addressed by M7.

- Plan: `plans/implementation/session-projections/006-atomic-control-delivery-transport-verification-hardening.md`
- Conditional closure: `plans/closure/session-projections/006-status.md`
- Implementation: `8ca570fddc08eb9663b894f3190ae0ed0af2b98b`

Accepted outcomes include bounded critical sends, WebSocket writer receipts, activation after canonical response delivery, rollback on critical failure, normal-flow transport isolation, raw filtering, projection-primary suppression, and bounded legacy `/ws` output.

### Milestone 7 — Corrective transport lifecycle and evidence closure

Status: conditionally closed; final task-lifecycle and replay-evidence polish is owned by M8.

- Plan: `plans/implementation/session-projections/007-corrective-transport-lifecycle-and-evidence-closure.md`
- Conditional closure: `plans/closure/session-projections/007-status.md`
- Implementation: `9887c2d581a3d01280485523161695d08469c34f`

Accepted outcomes include:

- owned/cancelled/joined Unix raw forwarder;
- generation-tagged `/tui` raw routing and final-boundary stale rejection;
- connection-local deterministic lifecycle seams;
- blocked-response ordering tests for all three transports;
- selected production-adapter rollback tests;
- expanded foreign ack/resume/unsubscribe/status/artifact coverage;
- reconnect/resume fixtures with fresh connection/subscription identity;
- corrected protocol-version expectation and transport test count;
- lifecycle static guard.

Residual M7 polish findings:

- WebSocket sibling tasks are aborted but not awaited;
- the adapter-level staged failure matrix is incomplete;
- reconnect tests do not assert full envelope sequence continuity and no duplication.

### Milestone 8 — Final transport lifecycle and replay evidence polish

Status: ready for handoff.

Implementation plan:

- `plans/implementation/session-projections/008-final-transport-lifecycle-and-replay-evidence-polish.md`

Class: correctness polish / task lifecycle / adapter verification / closure reconciliation

Repository baseline: `8b547a3d02e571a480a826f5dea9c81d79d95cc4`

Objective: complete joined `/core` and `/tui` task teardown, finish the production-adapter critical failure matrix, prove reconnect replay-to-live continuity at full envelope/sequence level, and produce final closure evidence matching the executable repository.

Required deliverables:

- shared or equivalent structured cancel/abort-and-await teardown for `/core` and `/tui`;
- repeated connection churn/drop-probe tests;
- adapter-level queue timeout, writer failure, cancellation, serialization-equivalent, disconnect-during-install, and pre-activation rollback tests where applicable;
- assertions for connection ownership removal, daemon subscription removal, receiver non-reuse, forwarder/task termination, and idempotent cleanup;
- exact replay envelope sequence and unique event-identity assertions for Unix, `/core`, and `/tui`;
- first-live sequence `replay_end_seq + 1` and bounded no-duplicate checks;
- replay handoff race and disconnect-during-replay cleanup coverage;
- lifecycle static guard extension rejecting WebSocket abort-without-await cleanup;
- corrected M7 record and dedicated M8 closure record.

Exit conditions:

- no `/core` or `/tui` connection task survives handler teardown;
- no abort-only sibling task cleanup remains;
- all material staged critical failure classes remove transport and daemon state;
- no failed receiver can be reused and no projection forwarder remains alive;
- reconnect replay contains exactly the missing committed envelopes once, in sequence;
- the first live envelope follows replay without a gap, duplicate, or reorder;
- stream identity remains stable while connection/subscription identities change;
- focused suites and guards pass;
- closure evidence matches executable test names, counts, commands, and commits;
- no unresolved high or medium M8 finding remains;
- M8 closure returns the roadmap and registry to strict closed status.

## 7. Deferred product work

These items are not part of M8:

- cross-tab artifact hand-off UX;
- numeric acknowledgement/resync hot-key UX;
- plugin-specific `ProjectionEvent::PluginUi` semantics;
- final team principals/roles/presence/chat;
- cross-daemon replay replication;
- removal of version-4 compatibility before its compatibility window expires.

## 8. Cross-cutting requirements

### Protocol and compatibility

Do not increment the projection protocol or remove version-4 compatibility. M8 changes task teardown, verification depth, and closure evidence—not wire meaning.

### Security and authorization

Connection authentication is not subscription ownership. Foreign ack/resume/unsubscribe/status/artifact operations remain fail-closed. Replay assertions must not expose artifact contents, secrets, or hidden reasoning.

### Concurrency, cancellation, and recovery

Persistence and receiver installation precede response staging; successful response delivery precedes live release. Connection cancellation precedes sibling task teardown. Every retained task is awaited. Every failure path has bounded cancellation and idempotent rollback. No state lock is held across socket I/O or task join.

### Observability

Expose bounded reason codes for task cancellation, task panic/error, critical-send failure, activation rollback, stale raw generation rejection, foreign-operation rejection, reconnect replay, and resource cleanup. Never log payload bodies, artifact bytes, or secrets.

### Performance

Critical sends remain bounded and prioritized. Task ownership and replay assertions must not introduce unbounded queues, per-event unbounded task creation, or production-only test bookkeeping.

### Documentation

Maintain transport state-machine, task ownership, queue, route-generation, replay continuity, compatibility, and test matrices. Closure evidence must include exact production and closure commits.

## 9. Verification strategy

Use:

- canonical reducer/controller fixtures;
- daemon replay/storage/failpoint tests;
- real two-client Unix, `/core`, and `/tui` fixtures;
- deterministic response-blocking and fault-injection hooks;
- queue-full, writer-close, cancellation, timeout, serialization-equivalent, pre-activation, and disconnect tests;
- task/drop probes and repeated connection churn;
- route-generation session-switch tests;
- foreign-operation matrix tests;
- exact reconnect and replay-to-live sequence tests;
- replay-response pause and disconnect-during-replay tests;
- legacy `/ws` and raw compatibility regressions;
- disclosure and static guards.

## 10. Risks and decision points

- Aborting a task without awaiting it can retain resources until the runtime polls the cancellation; teardown must join or abort-and-await.
- Awaiting task handles while holding connection-state locks can deadlock cleanup.
- Synthetic serialization failure must represent the real staged serialization boundary without distorting production protocol types.
- Replay range metadata alone does not prove exact event identity or absence of duplication.
- A live helper returning only subscription ID cannot prove sequence continuity.
- Test-only fault hooks must remain connection-local and must not introduce production global mutable state.
- A replay-authority or storage change requires an ADR; M8 should require neither.

## 11. Completion definition

This roadmap returns to strict closed status when all three transports are bounded and connection-owned at both subscription and task levels, every retained connection task is deterministically terminated, critical setup failures are proven against staged daemon subscriptions, raw session switching rejects stale queued traffic, replay-to-live continuity is exact at envelope and sequence level without gaps or duplicates, foreign operations remain fail-closed, and closure evidence precisely matches the repository.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/session-projections/001-projection-contracts.md` | `plans/closure/session-projections/001-status.md` | — |
| 2 | closed (strict) | library + corrective plans | `plans/closure/session-projections/002-status.md` | — |
| 3 | closed | `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md` | `plans/closure/session-projections/003-status.md` | — |
| 4 | closed | `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md` | `plans/closure/session-projections/004-status.md` | — |
| 5 | closed | `plans/implementation/session-projections/005-remote-transport-isolation-resume-closure.md` | `plans/closure/session-projections/005-status.md` | — |
| 6 | conditionally closed; findings addressed by M7 | `plans/implementation/session-projections/006-atomic-control-delivery-transport-verification-hardening.md` | `plans/closure/session-projections/006-status.md` | strict subsystem closure deferred to M8 |
| 7 | conditionally closed | `plans/implementation/session-projections/007-corrective-transport-lifecycle-and-evidence-closure.md` | `plans/closure/session-projections/007-status.md` | M8 final task-lifecycle and replay-evidence polish |
| 8 | ready | `plans/implementation/session-projections/008-final-transport-lifecycle-and-replay-evidence-polish.md` | — | — |
