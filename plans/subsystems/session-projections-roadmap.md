# Frontend-Neutral Session Projections and Replay Roadmap

Status: active

Long-term references:

- `plans/000-long-term-specification.md#8-read-only-session-observation`
- `plans/000-long-term-specification.md#14-acp-integration`
- `plans/001-terminology-and-domain-model.md` — session, turn, observation subscription, projection
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Related ADRs:

- None required for the initial projection boundary. Create an ADR if durable replay storage changes the authoritative event-log ownership model or requires a new database backend.

## 1. Purpose and ownership boundary

This subsystem owns the canonical, frontend-neutral representation of session activity and the snapshot/event/replay semantics used by local TUI, remote TUI, later observer mode, ACP adapters, web clients, and future frontends. It defines bounded projection DTOs, reducer semantics, subscriptions, sequence/acknowledgement/resume behavior, durable replay indexing, visibility/redaction classification, and artifact handles.

It consumes stable project/session identities, daemon events, turn/tool/run/job state, project-aware TUI state, session/message persistence, and protocol capability negotiation. It must not own team authorization policy, presence, chat, full audit retention, raw provider hidden reasoning, or agent-tree execution semantics beyond placeholders and stable references.

## 2. Work classification

### Invariants

- All frontends reconstruct the same logical session state from the same snapshot and event stream.
- Sequence ordering is monotonic within the defined stream scope and replay is deterministic.
- Expired or missing history triggers bounded resynchronization rather than silent divergence.
- Secret-bearing data is redacted before it enters a shared projection.
- Large logs, artifacts, and file bodies remain behind bounded handles.
- Raw terminal render frames are not the canonical collaboration protocol.
- Unknown event variants and capability versions degrade safely.

### Capabilities

- A reconnecting frontend resumes from a known sequence or receives a complete bounded snapshot.
- Different frontend implementations display equivalent session state.
- Project- and session-scoped subscriptions avoid unrelated event traffic.
- Later observer and ACP clients can consume the same projection contract.

### Infrastructure

- Projection DTOs and versioning.
- Canonical reducer/projector.
- Durable replay store/index and retention.
- Subscription registry and stream cursors.
- Visibility/redaction pipeline.
- Artifact/log handles and bounded reads.

### Polish

- Projection diagnostics and developer test fixtures.
- Efficient compaction/checkpointing.
- Frontend migration documentation and compatibility reporting.

## 3. Non-goals

- Implementing team principals, roles, or final authorization decisions.
- Exposing provider-private chain-of-thought.
- Streaming unbounded terminal buffers or complete repositories.
- Building presence, chat, or observer-panel UX in this phase.
- Replacing the audit log with the frontend replay log.
- Finalizing durable agent-tree projection details before the agent hierarchy subsystem.

## 4. Current state

Milestone 1 added `codegg_protocol::projection`: versioned bounded projection DTOs, projection capability negotiation, deterministic reducer semantics, snapshot builders, adapters from existing core snapshots/events, and independent-consumer equivalence fixtures.

The core protocol also has request/event envelopes, daemon-global event sequence fields, session/turn correlation, session and daemon snapshots, and raw subscribe/resume paths. The server/remote TUI path maintains bounded in-memory event buffers and selected raw core events are best-effort persisted in `core_event_log`.

There is still no authoritative durable per-session/per-project projection stream, stable stream cursor, subscription/ack registry, retention/checkpoint policy, or restart-safe projection high-water ownership. Existing raw event persistence is intentionally not sufficient for canonical projection replay.

## 5. Target architecture

Define versioned projection types:

- `SessionProjectionSnapshot`;
- `SessionSummaryProjection`;
- `TurnProjection`;
- `MessageProjection`;
- `ToolProjection`;
- `RunProjection`;
- `JobProjection`;
- `PermissionProjection` and `QuestionProjection`;
- `AgentTreeProjection` placeholder with stable run references;
- `ArtifactHandleProjection`;
- project/workspace/worktree summary references.

A canonical projector consumes durable/session events and produces snapshots using deterministic reducer rules. Projection events carry stream scope, sequence, timestamp, causation/correlation references, visibility class, payload version, and bounded content.

Replay storage uses append-only bounded database rows plus periodic snapshots/checkpoints. Retention is bounded by count/time/bytes. A client subscribes at project or session scope, acknowledges a sequence, resumes from a cursor, or receives `ResyncRequired` with a fresh snapshot when history is unavailable or capabilities differ.

Redaction occurs before publication into shared replay storage. Raw tool/run artifacts stay in existing stores and are referenced through authorized bounded handles.

## 6. Dependency graph

```text
Milestone 1: projection DTOs, versioning, and canonical reducer
        |
        v
Milestone 2: scoped subscriptions and durable replay
        |
        +--> Milestone 3: visibility, redaction, and artifact handles
        |           |
        |           v
        `--> Milestone 4: frontend adoption, compatibility, and closure
```

- Milestone 1 has hard dependencies on Domain Identity Milestone 3, Project Catalog Milestone 4, and the project-aware TUI/session state interfaces.
- Milestone 2 has a hard dependency on Milestone 1.
- Milestone 3 has hard dependencies on Milestones 1–2 and an interface dependency on future principal capability filtering.
- Milestone 4 has hard dependencies on Milestones 1–3.

## 7. Milestones

### Milestone 1 — Projection contracts and canonical reducer

Status: closed; see `plans/closure/session-projections/001-status.md`.

Class: infrastructure

Objective: define bounded, versioned session projection types and one deterministic reducer shared by tests and frontend adapters.

Dependencies: hard on stable project/session identities and project-aware session routing.

Deliverable boundary: DTOs, projection capabilities, reducer state machine, snapshot builder, mapping from current core events, payload limits, unknown-version behavior, and equivalence fixtures.

User or operator value: frontends can consume one logical session model rather than bespoke event interpretations.

Exit conditions:

- local and test frontends reconstruct byte/semantically equivalent snapshots from the same inputs;
- current turn/tool/run/job/permission/question states are represented;
- large output is replaced by bounded summaries/handles;
- unknown optional variants do not crash older compatible reducers;
- raw render frames are absent from the canonical contract.

Deferred work: durable replay and final authorization filtering.

### Milestone 2 — Scoped subscriptions and durable replay

Status: ready for handoff; see `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md`.

Class: capability

Objective: support deterministic reconnect/resume across client and daemon restart.

Dependencies: hard on Milestone 1.

Deliverable boundary: project/session subscription requests, stream IDs/cursors, durable event index, acknowledgement, retention/checkpoints, resume/resync, replay after restart, and lag handling.

Exit conditions:

- reconnect from an available cursor replays exactly the missing events;
- daemon restart preserves accepted replay history within retention;
- expired/gapped/ahead cursors return bounded `ResyncRequired` plus snapshot path;
- project subscriptions do not deliver unrelated project events;
- duplicate delivery is idempotent at the reducer.

Deferred work: distributed node event replication.

### Milestone 3 — Visibility, redaction, and artifact handles

Class: invariant

Objective: ensure projection storage and transport are safe for later multi-user observation.

Dependencies: hard on Milestones 1–2; interface dependency on principal capability seam.

Deliverable boundary: visibility enum, policy hook, redaction pipeline, secret-pattern and typed-field redaction, artifact/log handles, bounded read APIs, actor-only/admin placeholders, and negative tests.

Exit conditions:

- credentials/environment secrets never enter shared projection events;
- tool arguments/outputs are redacted before durable replay publication;
- large content is retrievable only through bounded handles and policy checks;
- visibility filtering can be supplied a future principal/capability context;
- redaction failures fail closed or downgrade to safe summaries.

Deferred work: final role policy and audit retention.

### Milestone 4 — Frontend adoption and closure

Class: capability

Objective: migrate local/remote TUI paths to the canonical projection and prove cross-frontend equivalence.

Dependencies: hard on Milestones 1–3.

Deliverable boundary: TUI adapter/reducer integration, remote TUI protocol migration, compatibility capability negotiation, reference second frontend/test client, performance bounds, documentation, and closure evidence.

Exit conditions:

- local TUI and a second independent client render equivalent state;
- remote reconnect no longer depends solely on an in-memory UI-specific buffer;
- incompatible clients receive explicit capability/version behavior;
- all Phase 5 exit criteria are evidenced.

## 8. Cross-cutting requirements

### Storage and migration

Replay storage is separate from chat and final audit storage. Use additive schema and bounded retention/checkpoints. Existing session/message data may seed snapshots but must not be duplicated unboundedly.

### Protocol and compatibility

Negotiate projection version and capabilities during initialization. Preserve current event-envelope compatibility where possible. Old remote TUI clients receive a bounded snapshot compatibility path or explicit unsupported diagnostics.

### Security and authorization

Redaction is structural first and heuristic second. Never rely only on regex. Policy hooks default to least disclosure. Provider-private reasoning is not represented unless explicitly emitted as user-visible content.

### Concurrency, cancellation, and recovery

Sequence assignment has one authority per stream. Publishing and durable acknowledgement must avoid acknowledged-but-unpersisted gaps. Subscribers are bounded; lagging clients resync. Cancellation removes subscriptions without deleting replay history.

### Observability and audit

Expose replay depth, retention, checkpoint age, subscriber lag, resync reasons, redaction counters, dropped/oversized payloads, and reducer version. Projection events retain correlation seams for later audit.

### Performance and resource use

Cap event size, replay window, subscriber queues, snapshot size, and artifact reads. Build checkpoints incrementally where practical. Avoid rebuilding full message history on every event.

### Documentation and operations

Update protocol, server, TUI, session, run/artifact, and troubleshooting docs. Provide a projection-version compatibility matrix and test-client examples.

## 9. Verification strategy

Use golden reducer fixtures, snapshot/event equivalence tests, property tests for idempotent replay, daemon restart fixtures, sequence-gap/duplicate/ahead cases, retention expiry, slow subscriber backpressure, redaction adversarial cases, large artifact handles, and two-client equivalence tests.

## 10. Risks and decision points

- Replay storage can become a second audit system. Keep retention and purpose explicitly frontend-oriented.
- Global versus per-project/per-session sequence ownership affects scaling. Begin with clearly scoped monotonic streams and document ordering guarantees rather than promising total deployment order.
- Redaction after event creation is too late. Require safe typed event construction before durable publication.
- If SQLite write amplification is excessive, optimize/checkpoint before changing database architecture; a database-backend change requires an ADR.

## 11. Completion definition

This roadmap closes when CodeGG has one versioned, bounded, redacted session projection contract; deterministic scoped replay survives reconnect and daemon restart; lag produces explicit resync; and at least two frontend implementations consume equivalent state without raw render-frame dependence.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/session-projections/001-projection-contracts.md` | `plans/closure/session-projections/001-status.md` | — |
| 2 | ready | `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md` | — | —; Milestone 1 closed |
| 3 | not started | — | — | Milestones 1–2 and authorization interface |
| 4 | not started | — | — | Milestones 1–3 closure |
