# Frontend-Neutral Session Projections and Replay Roadmap

Status: active

Long-term references:

- `plans/000-long-term-specification.md#8-read-only-session-observation`
- `plans/000-long-term-specification.md#14-acp-integration`
- `plans/001-terminology-and-domain-model.md` — session, turn, observation subscription, projection
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Related ADRs:

- None required for the current projection/replay boundary. Create an ADR if replay work changes the authoritative event-log ownership model, replaces SQLite, promises deployment-global ordering, requires principal-specific durable stream multiplication, or turns projection replay into audit retention.

## 1. Purpose and ownership boundary

This subsystem owns the canonical, frontend-neutral representation of session activity and the snapshot/event/replay semantics used by local TUI, remote TUI, later observer mode, ACP adapters, web clients, and future frontends. It defines bounded projection DTOs, reducer semantics, subscriptions, sequence/acknowledgement/resume behavior, durable replay indexing, visibility/redaction classification, artifact handles, bounded reads, and frontend adoption contracts.

It consumes stable project/session identities, daemon events, turn/tool/run/job state, project-aware TUI routing state, session/message persistence, artifact stores, transport-derived capability context, and protocol negotiation. It must not own final team authorization policy, presence, chat, full audit retention, raw provider hidden reasoning, or agent-tree execution semantics beyond placeholders and stable references.

## 2. Work classification

### Invariants

- All frontends reconstruct the same logical session state from the same snapshot and event stream.
- Sequence ordering is monotonic within the defined stream scope and replay is deterministic.
- Expired or missing history triggers bounded resynchronization rather than silent divergence.
- Secret-bearing data is redacted before it enters a shared projection.
- Large logs, artifacts, file bodies, and tool output remain behind bounded handles.
- Raw terminal render frames are not the canonical collaboration protocol.
- Unknown event variants and capability versions degrade safely.
- A caller cannot self-assign principal identity or projection capabilities through request payloads.

### Capabilities

- A reconnecting frontend resumes from a known sequence or receives a complete bounded snapshot.
- Different frontend implementations display equivalent session state.
- Project- and session-scoped subscriptions avoid unrelated event traffic.
- Authorized clients can retrieve bounded artifact excerpts through opaque handles.
- Later observer and ACP clients can consume the same projection contract.

### Infrastructure

- Projection DTOs and versioning.
- Canonical reducer/projector.
- Durable replay store/index and retention.
- Subscription registry and stream cursors.
- Visibility/redaction policy pipeline.
- Artifact/log handles and bounded reads.
- Shared frontend projection client/controller.

### Polish

- Projection diagnostics and developer test fixtures.
- Efficient compaction/checkpointing.
- Frontend migration documentation and compatibility reporting.
- Performance and long-running resource evidence.

## 3. Non-goals

- Implementing final team principals, roles, invitations, or organization authorization decisions.
- Exposing provider-private chain-of-thought.
- Streaming unbounded terminal buffers or complete repositories.
- Building presence, chat, or observer-panel UX in this phase.
- Replacing the audit log with the frontend replay log.
- Finalizing durable agent-tree projection details before the agent hierarchy subsystem.
- Cross-daemon merged sequence ordering.

## 4. Current state

Milestone 1 added `codegg_protocol::projection`: versioned bounded projection DTOs, capability negotiation, deterministic reducer semantics, snapshot builders, adapters from existing core snapshots/events, and independent-consumer equivalence fixtures.

Milestone 2's library/crate layer landed at `8dc4b85`: replay DTOs and protocol variants, storage migration v32, stream/event/checkpoint stores, sequence allocation, retention/checkpoints, subscription registry, safe-publication classification, resume/resync logic, metrics, and focused tests.

The production daemon integration is not complete. The conditional closure record at `plans/closure/session-projections/002-status.md` identifies missing publication wiring, daemon request dispatch, live subscription routing, and binding-revision invalidation. Direct inspection also shows empty canonical IDs, discarded subscription receivers, and placeholder stream IDs in the current service publication/delivery path. The corrective plan `plans/implementation/session-projections/002-corrective-daemon-integration-and-closure.md` is therefore the current dependency-ready handoff.

Multi-Project TUI Milestone 2 closed at `f569386`. Its picker/tab/session navigation is available. TUI Milestone 3 is now planned to provide project/session-correct event routing and active-view lifecycle interfaces required by final projection frontend adoption.

All remaining projection milestone plans are now authored:

- Milestone 3: `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md`;
- Milestone 4: `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md`.

Both remain blocked on their explicit dependencies. Existing raw core-event subscribe/resume behavior remains the compatibility path and must not be removed by the corrective pass.

## 5. Target architecture

The target is:

- one versioned `SessionProjectionSnapshot` contract;
- one deterministic `ProjectionReducer`;
- daemon-owned project/session stream descriptors and per-stream cursors;
- transactional persist-before-live-delivery publication;
- bounded replay, checkpoints, retention, queues, acknowledgements, and resync;
- canonical project/workspace/session binding and revision routing;
- a transport-derived principal/capability policy input seam;
- structural-first fail-closed disclosure before persistence/delivery;
- bounded artifact/log handles instead of inline large bodies;
- a shared transport-neutral frontend projection controller;
- additive compatibility for existing raw `CoreEvent` clients.

Projection storage remains separate from chat/message storage and final audit retention. Canonical shared replay should contain the least-disclosure safe form; client-local overlays remain owner-isolated and bounded.

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
- Milestone 2 library work is conditionally closed; its corrective daemon-integration pass is ready.
- Milestone 3 is authored but has hard dependencies on strict Milestone 2 closure and a transport-derived principal/capability filtering seam.
- Milestone 4 is authored but has hard dependencies on Milestone 3 closure and Multi-Project TUI Milestone 3 routing/lifecycle interfaces.

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

Status: corrective pass ready.

Library implementation: `8dc4b85`.

Conditional closure record: `plans/closure/session-projections/002-status.md`.

Corrective implementation plan: `plans/implementation/session-projections/002-corrective-daemon-integration-and-closure.md`.

Class: capability / correctness closure

Objective: support deterministic reconnect/resume across client and daemon restart through a fully wired daemon-owned replay authority.

The landed library layer provides stream/cursor DTOs, protocol variants, schema/storage, sequence allocation, replay, retention, checkpoints, subscription state, safe-publication classification, metrics, and focused tests.

The corrective pass owns:

- one production event-publication seam;
- canonical non-empty context resolution;
- real persisted stream ID delivery;
- subscription receiver ownership;
- daemon request dispatch;
- client/subscription-isolated live routing;
- binding-revision invalidation;
- startup/shutdown/maintenance integration;
- daemon-level restart/failpoint/compatibility evidence;
- strict closure documentation.

Exit conditions:

- every production source event reaches the centralized publisher exactly once;
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

Status: blocked.

Implementation plan: `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md`.

Class: invariant / security

Objective: ensure projection storage, snapshots, checkpoints, live transport, and bounded artifact access are safe for later multi-user observation.

Dependencies:

- strict Milestone 2 corrective closure;
- a daemon/transport-derived principal and semantic-capability filtering interface that cannot be forged by request fields.

Deliverable boundary:

- `ProjectionAccessContext` / capability seam;
- typed allow/deny/redact/summarize/handle/client-local decisions;
- exhaustive structural field classification;
- structural-first redaction with bounded heuristic defense for untyped text;
- owner-isolated client-local overlays;
- opaque project/session-scoped artifact handles;
- bounded, authorized, revision-aware, cancellable artifact/log/diff/output reads;
- replay/checkpoint/live enforcement before serialization/commit;
- adversarial and database-negative tests.

Exit conditions:

- credentials, environment secrets, provider connection material, and private keys never enter shared projection storage or transport;
- provider-private reasoning and internal diagnostics are absent from shared projection state;
- client-local content reaches only its owner and is absent from shared project replay;
- tool arguments/outputs are transformed before durable publication;
- large content is accessible only through bounded handles and policy checks;
- visibility filtering consumes a transport-derived principal/capability context;
- unknowns and redaction failures fail closed or downgrade to safe summaries;
- artifact reads are path-free, scope-authorized, range/byte/rate/concurrency bounded, and revision-aware.

Deferred work: final role policy, presence, chat, and audit retention.

### Milestone 4 — Frontend adoption, compatibility, and closure

Status: blocked.

Implementation plan: `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md`.

Class: capability / migration / closure

Objective: migrate local and remote TUI paths to the canonical projection, prove cross-frontend equivalence, retain bounded compatibility for older peers, and close the roadmap.

Dependencies:

- Milestone 3 strict closure;
- Multi-Project TUI Milestone 3 project/session-correct routing and active-view lifecycle;
- continued regression-clean Milestone 2 replay integration.

Deliverable boundary:

- shared transport-neutral `ProjectionClientController`;
- capability/version mode negotiation (`ProjectionPrimary`, `RawCompatibility`, `Unsupported`);
- subscribe/snapshot/live reduce/ack/resume/resync/unsubscribe/reconnect lifecycle;
- local and remote TUI adoption;
- bounded active/inactive tab projection state;
- artifact handle expansion UX;
- explicit raw-core compatibility adapter and removal criteria;
- an independent headless/reference client;
- production daemon cross-client equivalence tests;
- performance/resource/security/compatibility closure evidence.

Exit conditions:

- local TUI, remote TUI, and a second independent client reconstruct equivalent logical state;
- the canonical reducer is the only projection-state reducer;
- reconnect uses durable scoped replay or explicit atomic resync;
- multi-project routing remains identity/epoch correct;
- redaction/artifact policy is consumed rather than duplicated;
- older peers receive explicit safe compatibility or unsupported behavior;
- duplicate raw/projection mutable authorities are removed or isolated by negotiated mode;
- performance and long-running resource bounds are met;
- all Phase 5 exit criteria are evidenced.

## 8. Cross-cutting requirements

### Storage and migration

Replay storage is separate from chat and final audit storage. Use additive schema and bounded retention/checkpoints. Existing session/message data may seed snapshots but must not be duplicated unboundedly. Artifact handles reference authoritative stores rather than copying large content into replay.

### Protocol and compatibility

Negotiate projection version, replay limits, visibility/artifact capabilities, and fallback mode during initialization. Preserve existing event-envelope compatibility. Older clients continue through the bounded raw snapshot/event path or receive explicit unsupported diagnostics.

### Security and authorization

Redaction is structural first and heuristic second. Never rely only on regex. Policy hooks default to least disclosure. Provider-private reasoning is not represented. Capability context is derived by transport/daemon authority, not request payloads.

### Concurrency, cancellation, and recovery

Sequence assignment has one authority per stream. Persistence precedes projection delivery and acknowledgement. Subscribers are bounded; lagging clients resync. Cancellation removes transient subscriptions without deleting replay history. Session rebinds invalidate prior stream revisions. Artifact reads are bounded, cancellable, and cleaned on frontend scope changes.

### Observability and audit

Expose replay depth, retention, checkpoint age, subscriber lag, resync reasons, publication/disclosure outcomes, handle issuance/read denials, dropped/oversized payloads, and reducer version. Diagnostics contain IDs and counts, not payload bodies or secrets.

### Performance and resource use

Cap event size, replay windows, subscriber queues, snapshots, controller state, inactive-tab summaries, artifact reads/caches, and reconnect work. Build checkpoints incrementally. Avoid rebuilding complete message history on each event.

### Documentation and operations

Update protocol, daemon, server, client, TUI, session, run/artifact, security, compatibility, and troubleshooting docs. Maintain a projection-version/capability compatibility matrix and daemon-level test-client examples.

## 9. Verification strategy

Use golden reducer fixtures, daemon request/transport tests, restart fixtures, sequence-gap/duplicate/ahead cases, retention expiry, slow-subscriber backpressure, session-rebind races, failpoints before/after commit, structural/heuristic redaction adversarial cases, database-negative secret scans, artifact handle authorization/bounds tests, multi-project TUI routing races, and two-client equivalence/performance tests.

## 10. Risks and decision points

- A dual publication path can duplicate or reorder events. Keep one centralized publisher.
- Empty or placeholder stream identity can create cross-project leakage. Resolve canonical bindings before stream creation.
- Replay storage can become a second audit system. Keep retention and purpose frontend-oriented.
- Redaction after durable publication is too late. Require safe typed publication.
- Principal-specific durable streams can multiply storage and split canonical semantics. Prefer one redacted shared stream plus bounded client-local overlays; stop for an ADR if this cannot satisfy policy.
- Frontend migration can create two mutable reducers. Isolate raw compatibility and projection-primary modes explicitly.
- If SQLite write amplification is excessive, optimize/checkpoint before changing backend; a backend change requires an ADR.

## 11. Completion definition

This roadmap closes when CodeGG has one versioned, bounded, redacted session projection contract; deterministic scoped replay survives reconnect and daemon restart; production daemon events are published through one canonical replay seam; lag and binding changes produce explicit resync; large content remains behind authorized bounded handles; and local/remote plus a second frontend consume equivalent state without raw render-frame dependence.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/session-projections/001-projection-contracts.md` | `plans/closure/session-projections/001-status.md` | — |
| 2 | corrective pass ready | `plans/implementation/session-projections/002-corrective-daemon-integration-and-closure.md` | `plans/closure/session-projections/002-status.md` (conditional) | Library layer landed at `8dc4b85`; daemon publication, canonical stream identity, request dispatch, live routing, receiver ownership, and binding-revision closure remain |
| 3 | blocked | `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md` | — | Strict Milestone 2 closure plus transport-derived principal/capability filtering interface |
| 4 | blocked | `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md` | — | Milestone 3 closure plus Multi-Project TUI Milestone 3 routing/lifecycle |