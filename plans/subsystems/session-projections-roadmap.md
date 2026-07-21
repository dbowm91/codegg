# Frontend-Neutral Session Projections and Replay Roadmap

Status: active

Long-term references:

- `plans/000-long-term-specification.md#8-read-only-session-observation`
- `plans/000-long-term-specification.md#14-acp-integration`
- `plans/001-terminology-and-domain-model.md` — session, turn, observation subscription, projection
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Related ADRs:

- None required for the current projection/replay boundary. Create an ADR if replay work changes the authoritative event-log ownership model, replaces SQLite, promises deployment-global ordering, or turns projection replay into audit retention.

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

Milestone 1 added `codegg_protocol::projection`: versioned bounded projection DTOs, capability negotiation, deterministic reducer semantics, snapshot builders, adapters from existing core snapshots/events, and independent-consumer equivalence fixtures.

Milestone 2's library/crate layer landed at `8dc4b85`: replay DTOs and protocol variants, storage migration v32, stream/event/checkpoint stores, sequence allocation, retention/checkpoints, subscription registry, safe-publication classification, resume/resync logic, metrics, and focused tests.

Milestone 2's corrective daemon integration landed at this commit: one `ProjectionPublicationSeam` is installed as an `EventLog` sink hook so every production event reaches durable projection storage; canonical session binding resolves `ProjectId` / `WorkspaceId` / `binding_revision` from `ProjectStorage`; rebind invalidates the old stream; real `ProjectionStreamId` is used for live delivery; `CoreRequest::Projection*` dispatches through `CoreDaemon`; per-client subscription receivers own their forwarders; a 5-minute maintenance tick drives retention/checkpoints; a new static guard rejects unauthorized direct publication paths. Strict closure evidence is recorded at `plans/closure/session-projections/002-status.md`.

Existing raw core-event subscribe/resume behavior remains the compatibility path and was preserved by the corrective pass.

## 5. Target architecture

The target remains:

- one versioned `SessionProjectionSnapshot` contract;
- one deterministic `ProjectionReducer`;
- daemon-owned project/session stream descriptors and per-stream cursors;
- transactional persist-before-live-delivery publication;
- bounded replay, checkpoints, retention, queues, acknowledgements, and resync;
- canonical project/workspace/session binding and revision routing;
- fail-closed publication and later principal-aware visibility filtering;
- bounded artifact/log handles instead of inline large bodies;
- additive compatibility for existing raw `CoreEvent` clients.

Projection storage remains separate from chat/message storage and final audit retention.

## 6. Dependency graph

```text
Milestone 1: projection DTOs, versioning, and canonical reducer
        |
        v
Milestone 2: scoped subscriptions and durable replay
        |
        +-- corrective daemon integration and strict closure
        |
        v
Milestone 3: visibility, redaction, and artifact handles
        |
        v
Milestone 4: frontend adoption, compatibility, and closure
```

- Milestone 1 is closed.
- Milestone 2 is strictly closed (library at `8dc4b85` + corrective daemon integration at this commit).
- Milestone 3 has a hard dependency on the principal capability filtering interface; Milestone 2 wiring is no longer a blocker.
- Milestone 4 has hard dependencies on Milestones 1–3.

## 7. Milestones

### Milestone 1 — Projection contracts and canonical reducer

Status: closed; see `plans/closure/session-projections/001-status.md`.

Class: infrastructure

Objective: define bounded, versioned session projection types and one deterministic reducer shared by tests and frontend adapters.

Deliverable boundary: DTOs, projection capabilities, reducer state machine, snapshot builder, mapping from current core events, payload limits, unknown-version behavior, and equivalence fixtures.

Exit conditions:

- independent consumers reconstruct equivalent state from the same inputs;
- turn/tool/run/job/permission/question states are represented;
- large output is bounded or represented by handles;
- unknown optional variants do not crash compatible reducers;
- raw render frames are absent.

### Milestone 2 — Scoped subscriptions and durable replay

Status: **closed (strict)**.

Library implementation: `8dc4b85`.

Corrective daemon integration and strict closure: this commit (see
`plans/closure/session-projections/002-status.md`).

Class: capability / correctness closure

Objective: support deterministic reconnect/resume across client and daemon restart through a fully wired daemon-owned replay authority.

The landed library layer provides stream/cursor DTOs, protocol variants, schema/storage, sequence allocation, replay, retention, checkpoints, subscription state, safe-publication classification, metrics, and focused tests.

The corrective pass owns:

- one production event-publication seam;
- canonical non-empty context resolution;
- real persisted stream ID delivery;
- daemon request dispatch;
- client/subscription-isolated live routing;
- binding-revision invalidation;
- startup/shutdown/maintenance integration;
- daemon-level restart/failpoint/compatibility evidence;
- strict closure documentation.

Exit conditions:

- every production source event reaches the centralized publisher;
- accepted projection events persist before projection delivery;
- project/session streams use canonical IDs and current binding revision;
- all `Projection*` requests dispatch through `CoreDaemon`;
- projection live events reach only the owning subscription/client;
- reconnect from an available cursor replays exactly missing events;
- restart preserves retained history without sequence reuse;
- expired/gapped/ahead/mismatched cursors resync explicitly;
- legacy raw core-event clients remain compatible;
- closure status becomes strict `closed`.

Deferred work: distributed node event replication and final multi-user policy.

### Milestone 3 — Visibility, redaction, and artifact handles

Status: blocked (principal capability filtering seam still required).

Class: invariant

Objective: ensure projection storage and transport are safe for later multi-user observation.

Dependencies: strict Milestone 2 closure (now satisfied) plus a principal/capability filtering interface. The Milestone 2 wiring is no longer a blocker.

Deliverable boundary: visibility enum, policy hook, structural and heuristic redaction, secret-field handling, artifact/log handles, bounded reads, actor-only/admin placeholders, and negative tests.

Exit conditions:

- credentials and environment secrets never enter shared projection events;
- tool arguments/outputs are redacted before durable publication;
- large content is accessible only through bounded handles and policy checks;
- visibility filtering accepts a future principal/capability context;
- redaction failures fail closed or downgrade to safe summaries.

Deferred work: final role policy and audit retention.

### Milestone 4 — Frontend adoption and closure

Status: blocked.

Class: capability

Objective: migrate local/remote TUI paths to the canonical projection and prove cross-frontend equivalence.

Dependencies: Milestones 1–3 closed.

Deliverable boundary: TUI adapter/reducer integration, remote TUI protocol migration, compatibility negotiation, a reference second client, performance bounds, documentation, and closure evidence.

Exit conditions:

- local TUI and a second independent client render equivalent state;
- remote reconnect no longer depends solely on an in-memory UI-specific buffer;
- incompatible clients receive explicit capability/version behavior;
- all Phase 5 exit criteria are evidenced.

## 8. Cross-cutting requirements

### Storage and migration

Replay storage is separate from chat and final audit storage. Use additive schema and bounded retention/checkpoints. Existing session/message data may seed snapshots but must not be duplicated unboundedly.

### Protocol and compatibility

Negotiate projection version and capabilities during initialization. Preserve existing event-envelope compatibility. Older clients continue through the bounded raw snapshot/event path or receive explicit unsupported diagnostics.

### Security and authorization

Redaction is structural first and heuristic second. Never rely only on regex. Policy hooks default to least disclosure. Provider-private reasoning is not represented unless explicitly emitted as user-visible content.

### Concurrency, cancellation, and recovery

Sequence assignment has one authority per stream. Persistence precedes projection delivery and acknowledgement. Subscribers are bounded; lagging clients resync. Cancellation removes transient subscriptions without deleting replay history. Session rebinds invalidate prior stream revisions.

### Observability and audit

Expose replay depth, retention, checkpoint age, subscriber lag, resync reasons, publication outcomes, dropped/oversized payloads, and reducer version. Diagnostics contain IDs and counts, not payload bodies or secrets.

### Performance and resource use

Cap event size, replay windows, subscriber queues, snapshots, and artifact reads. Build checkpoints incrementally. Avoid rebuilding complete message history on each event.

### Documentation and operations

Update protocol, daemon, server, client, TUI, session, run/artifact, and troubleshooting docs. Maintain a projection-version compatibility matrix and daemon-level test examples.

## 9. Verification strategy

Use golden reducer fixtures, daemon request/transport tests, restart fixtures, sequence-gap/duplicate/ahead cases, retention expiry, slow subscriber backpressure, session-rebind races, failpoints before/after commit, safe-publication adversarial cases, large artifact handles, and two-client isolation/equivalence tests.

## 10. Risks and decision points

- A dual publication path can duplicate or reorder events. Keep one centralized publisher.
- Empty or placeholder stream identity can create cross-project leakage. Resolve canonical bindings before stream creation.
- Replay storage can become a second audit system. Keep retention and purpose frontend-oriented.
- Redaction after durable publication is too late. Require safe typed publication.
- If SQLite write amplification is excessive, optimize/checkpoint before changing backend; a backend change requires an ADR.
- Final authorization policy must not be smuggled into the M2 corrective integration pass.

## 11. Completion definition

This roadmap closes when CodeGG has one versioned, bounded, redacted session projection contract; deterministic scoped replay survives reconnect and daemon restart; production daemon events are published through one canonical replay seam; lag and binding changes produce explicit resync; and at least two frontend implementations consume equivalent state without raw render-frame dependence.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/session-projections/001-projection-contracts.md` | `plans/closure/session-projections/001-status.md` | — |
| 2 | closed (strict) | `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md` (library) and `plans/implementation/session-projections/002-corrective-daemon-integration-and-closure.md` (daemon integration) | `plans/closure/session-projections/002-status.md` | — |
| 3 | blocked | — | — | Principal/capability filtering interface; Milestone 2 wiring is no longer a blocker |
| 4 | blocked | — | — | Milestones 1–3 closure |
