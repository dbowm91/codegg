# Session Projections Milestone 002 — Scoped Subscriptions and Durable Replay

Status: ready for handoff

Repository baseline: `1c37787afc6b2afd437f1d3f21a6fe26226a73d7` (`main`; planning-only commits after this baseline do not alter production behavior)

Production implementation baseline:

- `ec42dce` — canonical project/workspace/session context and additive identity-aware protocol bindings.
- `d1e5b70` — daemon-authoritative project catalog protocol and request-scoped server context.
- `62e26b1` — project-aware TUI state seam and bounded catalog client, providing the first downstream project/session projection consumer boundary.
- `f6c8669` — `codegg_protocol::projection`, bounded `SessionProjectionSnapshot`, `ProjectionEnvelope`, `ProjectionStreamScope`, deterministic `ProjectionReducer`, capability negotiation, adapters, and independent-consumer fixtures.
- Existing `src/core/event_log.rs` — global in-memory ring plus best-effort SQLite persistence for a selected subset of `CoreEvent` values. It is a compatibility/recovery source, not a sufficient durable projection replay authority.

Source roadmap:

- `plans/subsystems/session-projections-roadmap.md#milestone-2--scoped-subscriptions-and-durable-replay`

Applicable closure evidence:

- `plans/closure/domain-identity/003-corrective-status.md`
- `plans/closure/project-catalog/004-status.md`
- `plans/closure/tui-project-sessions/001-status.md`
- `plans/closure/session-projections/001-status.md`

Applicable ADRs:

- None initially. Use the existing SQLite authority and additive migration model. Stop for an ADR if implementation requires replacing SQLite, changing the authoritative core event-log ownership model, promising deployment-global total ordering, or making projection replay the audit-retention system.

Primary class: capability

## 1. Objective

Add daemon-owned, bounded, project/session-scoped projection subscriptions and durable replay so a frontend can:

- subscribe to one canonical session or one canonical project;
- receive a bounded initial snapshot bundle and a stable opaque stream cursor;
- acknowledge applied projection events;
- disconnect and resume from an available cursor without receiving unrelated project/session events;
- survive daemon restart while retained replay history and checkpoints remain available;
- receive an explicit bounded `ResyncRequired` result when the cursor is expired, ahead, gapped, scope-mismatched, or version-incompatible;
- rebuild through the existing deterministic `ProjectionReducer` without introducing a second execution/session authority.

The milestone succeeds when replay continuity is durable and deterministic for accepted projection events, sequence assignment has one authority per stream, persistence commits before live delivery, lag is bounded, and project subscriptions cannot receive events from another project.

## 2. Why this milestone is ready

Session Projections Milestone 001 is closed and provides:

- `PROJECTION_PROTOCOL_VERSION = 1` and version-range negotiation;
- bounded projection DTOs and collection/string limits;
- `ProjectionStreamScope::{Session, Project, Workspace, Daemon}`;
- deterministic idempotent reducer semantics;
- adapters from current snapshots and core events;
- `ResyncRequired` as an explicit reducer event/outcome;
- stable golden fixtures and an independent second consumer.

The project/session identity and project-aware frontend prerequisites are closed. No final team authorization policy is required to implement local/current-access scoped replay, but the design must preserve a future filtering seam.

## 3. Current implementation evidence and gaps

At the repository baseline:

- `ProjectionEnvelope` carries projection protocol version, event sequence, timestamp, optional session/turn IDs, a coarse scope enum, and a bounded `ProjectionEvent`.
- `ProjectionReducer` deduplicates by `event_seq`, rejects unsupported versions, detects scope mismatch, reconciles impossible transitions, and can request resync.
- `ProjectionStreamScope` includes project/session/workspace/daemon variants, but there is no stable stream ID, cursor DTO, subscription request, acknowledgement request, retention metadata, checkpoint, replay response, or daemon subscription registry.
- Current adapters reuse the source `CoreEvent` envelope sequence. That sequence is daemon-global, not a durable per-projection-stream cursor contract.
- `EventLog` starts `next_seq` at 1, keeps a bounded in-memory ring, broadcasts live events, and best-effort inserts a selected subset into `core_event_log`.
- `core_event_log` excludes deltas and snapshots, has no project/workspace stream key, no retention/checkpoint policy, no projection protocol version, and no acknowledgement/subscriber state.
- `EventLog::publish` broadcasts even when the best-effort SQLite insert fails; this cannot guarantee persist-before-deliver replay semantics.
- `EventLog::new_with_pool` does not visibly establish a durable per-stream sequence authority from persisted high-water marks.
- Existing `CoreRequest::Subscribe` / `Resume` and socket filters are primarily session/global-shaped and operate on raw `CoreEvent`, not canonical projection streams.
- M1 visibility tags exist, but final multi-user visibility/redaction policy is explicitly deferred to Milestone 003.

Therefore this milestone must add a separate projection replay subsystem while preserving the existing raw core-event compatibility path.

## 4. Invariants that must not regress

- Projection state remains derived frontend state, never execution/session authority.
- The canonical `ProjectionReducer` remains pure and I/O-free.
- One daemon-owned service assigns sequence numbers for each projection stream.
- Cursors are scoped to one immutable stream ID and cannot be replayed against another stream.
- A project stream contains only sessions canonically bound to that project.
- A session stream contains only the canonical session and its relevant project/workspace context.
- A path, directory, label, or tab ID never defines a stream.
- Projection events are persisted transactionally before they are eligible for live delivery or acknowledgement.
- Acknowledgement never advances beyond the committed stream high-water mark.
- Duplicate delivery is allowed at transport boundaries and remains reducer-idempotent; missing committed events are not silently skipped.
- Retention is bounded by event count, age, and serialized bytes.
- Subscriber queues and subscription counts are bounded.
- Lag, queue overflow, expired history, sequence gaps, future/ahead cursors, scope mismatch, and incompatible versions produce explicit resync/error behavior.
- Existing `CoreEvent` subscribers and raw core replay remain backward compatible.
- Projection replay storage is distinct from chat/message storage and final audit retention.
- Raw terminal render frames, full file bodies, unrestricted logs, provider-private hidden reasoning, credentials, and secret-bearing provider configuration do not enter durable projection replay.
- Until Milestone 003 lands full policy/redaction, only explicitly accepted safe publication classes enter shared durable replay.
- Restart hydration never reuses a committed sequence number.
- No network or filesystem operation is performed by replay reducers or cursor validation.

## 5. Scope

### In scope

- Stable opaque projection stream IDs and stream descriptors.
- Session and project subscription scopes.
- Stream cursor, retention-floor, high-water, checkpoint, and replay metadata DTOs.
- Additive projection subscribe/resume/ack/unsubscribe/snapshot protocol operations.
- Daemon-owned projection replay coordinator/service.
- Additive SQLite schema for streams, events, and checkpoints.
- Per-stream sequence allocation and restart hydration.
- Transactional persist-before-broadcast publication.
- Mapping current core events to accepted durable projection events using Milestone 001 adapters.
- Canonical session-binding resolution for event routing.
- Project stream fan-out with its own monotonic project cursor.
- Bounded project/session snapshot bundles.
- Periodic/count/byte checkpoint creation.
- Count/age/byte retention and incremental pruning.
- In-memory bounded subscription registry and monotonic acknowledgements.
- Resume/resync decision logic.
- Slow-subscriber/queue-overflow handling.
- Current raw `EventLog` high-water hydration correctness where required for source correlation.
- Socket, stdio, in-process, and server adapter compatibility.
- Focused restart, failpoint, retention, lag, duplicate, gap, and isolation tests.
- Metrics/diagnostics and architecture documentation.

### Explicitly out of scope

- Final role/principal authorization policy.
- Final visibility/redaction pipeline and artifact read authorization; Milestone 003 owns them.
- Durable client presence or chat.
- Durable persistence of every `ClientLocal`, `Internal`, or `Sensitive` event before Milestone 003 defines policy.
- Workspace- or daemon-wide projection subscriptions. Their enum values remain reserved and must return explicit unsupported diagnostics in this milestone.
- Cross-node replication or merged ordering across several daemons.
- Deployment-global sequence ordering.
- Replacing `core_event_log` for existing raw-core clients.
- Replacing session/message storage.
- Long-term audit retention or compliance export.
- Frontend migration to canonical projection replay; Milestone 004 owns broad adoption.
- ACP, web frontend, observer UX, or team collaboration UI.
- A new database backend.

## 6. Target architecture

### 6.1 Protocol types

Add a replay transport module under `codegg_protocol::projection`, for example `projection/replay.rs`, without changing the semantic fields of the Milestone 001 snapshot/event DTOs.

Required types:

- `ProjectionStreamKind { Session, Project }`;
- `ProjectionStreamId(String)` as an opaque validated identifier;
- `ProjectionStreamDescriptor` containing stream ID, kind, canonical project ID, optional workspace/session IDs, projection version, retention floor, high-water mark, and latest checkpoint sequence;
- `ProjectionCursor { stream_id, event_seq, projection_version }`;
- `ProjectionSubscriptionId(String)` as connection-local opaque identity;
- `ProjectionSubscriptionRequest` with explicit project/session scope and optional cursor;
- `ProjectionSnapshotBundle` containing one session snapshot for session scope or a bounded vector of independent session snapshots for project scope;
- `ProjectionReplayBatch` containing descriptor, ordered events, optional snapshot/checkpoint, replay start/end, current high-water, truncation/resync flags;
- `ProjectionResyncReason` with stable variants such as `HistoryExpired`, `HistoryGap`, `CursorAhead`, `StreamMismatch`, `ScopeMismatch`, `VersionMismatch`, `SnapshotUnavailable`, and `SubscriberLagged`;
- `ProjectionAck` and bounded subscription status/diagnostic DTOs.

Do not overload `ProjectionEnvelope.session_id` to carry project/workspace identity. The descriptor and subscription scope own those identities.

Project snapshot bundles must not force several sessions into one fake primary-session snapshot. Reuse independent bounded `SessionProjectionSnapshot` values and a bounded project summary wrapper.

### 6.2 Additive core protocol operations

Add additive request/response/event variants following existing serde-default and capability behavior.

Requests:

- `ProjectionCapabilities`;
- `ProjectionSubscribe { request }`;
- `ProjectionResume { cursor, include_snapshot_if_resync }`;
- `ProjectionAck { subscription_id, cursor }`;
- `ProjectionUnsubscribe { subscription_id }`;
- `ProjectionSnapshotGet { scope }`;
- optional bounded `ProjectionSubscriptionStatus` for diagnostics.

Responses:

- capabilities/version/limits;
- subscription established with descriptor, snapshot bundle, current cursor, and retention floor;
- ordered replay batch;
- explicit resync-required response with reason and bounded snapshot path/bundle;
- acknowledgement accepted/current lag;
- unsubscribe acknowledgement;
- typed bounded errors.

Live event delivery:

- add one projection-stream event shape carrying `subscription_id`, stream ID, cursor/event sequence, and `ProjectionEnvelope`;
- transport adapters must deliver it only to clients with the matching live subscription;
- never broadcast a project projection event unfiltered to every client and expect frontend-only filtering.

Older clients continue using existing snapshot/raw-event paths. Projection capability fields remain additive and default false.

### 6.3 Stream identity and routing

Create durable stream rows lazily and idempotently.

Canonical uniqueness:

- one session stream per canonical `SessionId` and its current canonical binding;
- one project stream per canonical `ProjectId`;
- stream IDs are generated opaque IDs stored in SQLite, not hashes or path strings;
- uniqueness constraints cover stream kind plus canonical identity fields;
- a session rebind that changes project/workspace must not silently append to the old project stream. Record the binding revision and route future events to the new canonical stream after an explicit rebind transition/resync boundary.

Routing rules:

1. A source event with a session ID resolves the current canonical session binding.
2. Accepted projection events are appended to the session stream.
3. The same accepted event is appended to the owning project stream with an independent project-stream sequence.
4. Project/catalog events with explicit project identity route directly to the project stream when they map to the projection contract.
5. Missing, legacy-unbound, archived, or ambiguous session context fails closed to a bounded diagnostic and does not guess a project.
6. Workspace/daemon stream requests are explicitly unsupported in M2.

A single source event may map to several projection events. All events for all target streams produced by one source event must be committed in one transaction or fail as a unit so session/project replay cannot partially diverge.

### 6.4 Storage schema

Use the next additive schema migration after current `CURRENT_SCHEMA_VERSION`. Do not rewrite or drop `core_event_log`.

Minimum tables:

#### `projection_stream`

- `id TEXT PRIMARY KEY`;
- `kind TEXT NOT NULL`;
- `project_id TEXT NOT NULL`;
- nullable `workspace_id`;
- nullable `session_id`;
- `binding_revision INTEGER` where applicable;
- `projection_version INTEGER NOT NULL`;
- `next_seq INTEGER NOT NULL`;
- `retention_floor_seq INTEGER NOT NULL`;
- `high_water_seq INTEGER NOT NULL`;
- nullable `latest_checkpoint_seq`;
- timestamps;
- lifecycle/state field for invalidated/rebound streams if required;
- unique constraints for project and session scope identities.

#### `projection_event`

- `stream_id TEXT NOT NULL`;
- `event_seq INTEGER NOT NULL`;
- `projection_version INTEGER NOT NULL`;
- timestamp;
- nullable canonical session/turn IDs;
- event kind;
- visibility/publication class;
- serialized bounded payload;
- serialized byte count;
- optional source `core_event_seq` for diagnostics only;
- created timestamp;
- primary key `(stream_id, event_seq)`;
- indexed `(stream_id, event_seq)` and retention timestamp/byte accounting fields.

#### `projection_checkpoint`

- `stream_id TEXT NOT NULL`;
- `checkpoint_seq INTEGER NOT NULL`;
- projection version/reducer version;
- bounded serialized `ProjectionSnapshotBundle`;
- serialized byte count;
- created timestamp;
- primary key `(stream_id, checkpoint_seq)`;
- index for latest checkpoint lookup.

Subscription registry state is transient by default and must not be stored unless a concrete restart requirement demonstrates value. Client-held cursors are the reconnect contract.

### 6.5 Sequence authority and transactional publication

Create one daemon-owned `ProjectionReplayService` or equivalent. It owns:

- stream lookup/creation;
- per-stream sequence allocation;
- projection adapter invocation;
- safe-publication classification;
- transactional event persistence;
- checkpoint scheduling;
- retention pruning;
- live subscription delivery;
- cursor validation and replay reads.

Required publication ordering:

1. Receive one canonical source event and correlation metadata.
2. Resolve canonical project/workspace/session scope.
3. Map through Milestone 001 adapters.
4. Apply M2 safe-publication gate.
5. Begin SQLite transaction (`BEGIN IMMEDIATE` or equivalent existing write-serialization pattern).
6. Load/create target stream rows and allocate contiguous per-stream sequence values.
7. Insert all bounded projection events and update stream high-water/next-seq values.
8. Optionally create/update a checkpoint in the same or a clearly ordered follow-up transaction.
9. Commit.
10. Only after commit, publish live projection events to matching subscriptions.

If persistence fails, do not send the event as durable projection output. Emit an operator diagnostic/metric through a non-replay path and leave the last committed cursor unchanged.

Avoid acknowledged-but-unpersisted gaps. A subscriber may acknowledge only committed cursors.

### 6.6 Current `EventLog` integration

Do not implement projection replay by reading only `core_event_log`:

- it is best-effort;
- it stores only selected event families;
- it lacks project scope and projection version;
- it can omit text/progress events required by the projection reducer;
- it has no checkpoint/retention/subscriber contract.

Integrate at a daemon-owned publication seam so accepted projection events are durably committed near source publication.

Implementation must inventory direct `event_log.publish` call sites. Prefer one centralized daemon helper or a bounded sink hook rather than adding inconsistent manual replay writes at many call sites.

Narrow correctness requirement:

- hydrate the raw `EventLog` next sequence from persisted `MAX(event_seq) + 1` where a pooled log is used, or otherwise prove existing restart construction already does so before serving;
- add restart tests proving new raw events do not collide with persisted `core_event_log` rows;
- keep raw core sequence as correlation metadata only, not projection cursor authority.

### 6.7 Safe-publication gate before Milestone 003

Milestone 003 owns full visibility/redaction and future principal filtering. M2 must not persist unsafe classes in the meantime.

Define and test an explicit publication classification:

- `Public`: eligible for shared durable replay after normal bounds/normalization.
- `ClientLocal`: live-only to the originating client when a safe origin can be established; otherwise omit from shared durable replay and increment a bounded diagnostic/metric.
- `Internal`: never persist or emit through shared projection replay.
- `Sensitive`: fail closed; replace with a safe bounded diagnostic or require resync to a structurally safe snapshot. Never persist the original payload.

The accepted durable subset must be documented event-by-event. Events excluded by visibility must not consume a durable stream sequence and therefore must not create cursor gaps.

Checkpoint/snapshot construction must apply the same structural publication filter. Do not serialize a full M1 snapshot and rely on a later consumer to hide sensitive/internal fields.

This is a minimum safety gate, not the final Milestone 003 policy engine.

### 6.8 Snapshot and checkpoint behavior

Initial subscribe:

- validate scope/capability/version;
- resolve/create the stream;
- build or load the latest safe checkpoint;
- return a bounded snapshot bundle plus descriptor and committed high-water cursor;
- register the live subscription only after the snapshot/high-water boundary is fixed so events cannot race between snapshot and live delivery.

Use one of two safe handoff patterns:

- transactionally capture high-water, load/build snapshot at that boundary, then replay events after it before enabling live delivery; or
- register a bounded pending queue, capture/build snapshot, drain events after boundary, then mark live.

Document and test the chosen no-gap/no-duplication semantics.

Checkpoint policy must be configurable and bounded. Initial target defaults:

- checkpoint after 256 accepted events or 1 MiB of accepted serialized event payload since the prior checkpoint;
- checkpoint active streams at least every 5 minutes when state changed;
- retain a bounded number of checkpoints per stream, including the latest checkpoint at or above the retention floor;
- create checkpoints incrementally through `ProjectionReducer`; do not rebuild full message history on every event.

Project checkpoint bundles must cap session snapshots using existing projection bounds and identify truncation. Omitted sessions are available through session-specific subscription/snapshot requests.

### 6.9 Retention and pruning

Add configurable defaults with hard implementation caps. Initial target policy:

- session stream: retain at most 20,000 events, 7 days, or 64 MiB serialized payload, whichever bound is hit first;
- project stream: retain at most 50,000 events, 7 days, or 128 MiB serialized payload;
- hard cap one accepted serialized event at 64 KiB, with existing projection DTO bounds normally producing much smaller events;
- prune in bounded batches, not an unbounded delete transaction;
- never prune the latest usable checkpoint needed to resync;
- update `retention_floor_seq` transactionally with pruning;
- preserve cursor monotonicity; sequence numbers are never reused after pruning.

If repository-wide resource policy indicates lower defaults are required, the implementer may lower them but must preserve all three dimensions (count/time/bytes), document final values, and test the caps.

Trigger pruning through:

- bounded post-publication checks;
- a daemon maintenance task with cancellation/shutdown ownership;
- explicit operator/test seam.

Do not start one unbounded task per stream.

### 6.10 Subscription registry, acknowledgement, and lag

Create one daemon-owned transient subscription registry.

Each subscription stores:

- subscription ID;
- owning client/connection ID;
- stream ID and scope descriptor;
- negotiated projection version;
- last delivered committed sequence;
- last acknowledged sequence;
- bounded queue sender;
- creation/last-activity timestamps;
- state: initializing, live, lagged/resync-required, closed.

Initial bounds:

- maximum 32 projection subscriptions per client;
- maximum 256 total active projection subscriptions per daemon;
- maximum 512 queued events per subscription;
- bounded IDs and diagnostics;
- finite idle timeout for abandoned subscriptions if the transport does not cleanly disconnect.

Acknowledgement rules:

- monotonically increase only;
- cursor stream ID and projection version must match;
- cannot exceed the stream committed high-water;
- duplicate/lower ack is idempotent;
- ack updates lag metrics but does not delete replay history directly;
- disconnect/unsubscribe removes transient subscription state only.

Queue overflow or broadcast lag:

- do not drop arbitrary events and continue;
- transition the subscription to resync-required with reason `SubscriberLagged`;
- stop live delivery for that subscription;
- preserve durable history for normal resume if still retained;
- close the bounded queue cleanly.

### 6.11 Resume and resync semantics

Cursor validation must distinguish:

- stream not found;
- stream ID/scope mismatch;
- projection version mismatch;
- cursor exactly at high-water: return empty replay and live subscription;
- cursor below high-water and at/above retention floor with contiguous rows: return exactly missing ordered events;
- cursor below retention floor: `HistoryExpired`;
- missing row inside retained range: `HistoryGap`;
- cursor above high-water: `CursorAhead`;
- stream invalidated by canonical session rebind: `ScopeMismatch` or a dedicated bounded rebind reason;
- checkpoint unavailable/corrupt: `SnapshotUnavailable` and rebuild/fail actionably.

A `ResyncRequired` response must include:

- stable reason;
- stream descriptor;
- retention floor and high-water;
- requested cursor;
- negotiated version;
- either a bounded safe snapshot bundle or an explicit snapshot request path;
- no raw SQL/storage diagnostics.

Replay batches must be capped by event count and serialized bytes. If more events remain, return a continuation cursor; do not allocate the full retained stream into memory.

### 6.12 Restart and recovery

On daemon restart:

- hydrate stream descriptors, high-water, retention floor, and next sequence from durable rows;
- validate `next_seq > high_water` and repair only through deterministic metadata reconciliation;
- retain no live subscription state;
- accept client-held cursors against retained events/checkpoints;
- do not rebuild every stream or snapshot eagerly;
- lazy-load stream/checkpoint data on first subscribe/resume;
- resume pruning/maintenance without deleting usable history;
- quarantine or fail actionably on corrupt stream/checkpoint metadata rather than guessing.

Add failpoints or deterministic test seams around:

- stream row creation;
- event insert after sequence allocation;
- multi-stream session/project transaction before commit;
- checkpoint write;
- pruning floor update;
- live delivery after commit.

Restart tests must prove no sequence reuse and no partially committed session/project fan-out.

## 7. Ordered work packages

### Work package A — Replay protocol contracts

Intent: define additive transport shapes without modifying M1 projection semantics.

Required changes:

- replay module types;
- stream/cursor/subscription/resync DTOs;
- core request/response/live event variants;
- capability/limit negotiation;
- serde/default/unknown-version tests;
- project/session scope validation.

Acceptance evidence:

- old fixtures default new fields safely;
- cursor/stream mismatch is typed;
- workspace/daemon subscription request returns explicit unsupported behavior;
- no render-frame or secret-bearing type enters replay DTOs.

### Work package B — Additive storage and sequence authority

Intent: establish durable per-stream ordering.

Required changes:

- next schema migration;
- stream/event/checkpoint stores or service-owned repository APIs;
- indexes/constraints;
- transactionally allocated per-stream sequences;
- restart hydration/reconciliation;
- raw `EventLog` persisted high-water correction where needed.

Acceptance evidence:

- concurrent publish to one stream yields contiguous unique sequence values;
- session/project fan-out is all-or-nothing;
- restart allocates above persisted high-water;
- rollback leaves no visible live event or partial row.

### Work package C — Projection publication integration

Intent: map daemon events once and persist accepted projection events before delivery.

Required changes:

- centralized publication seam or sink;
- canonical binding routing;
- M1 adapter reuse;
- safe-publication gate;
- stream lookup/creation;
- post-commit live dispatch;
- bounded diagnostics/metrics.

Acceptance evidence:

- no broad set of inconsistent manual replay writes;
- public accepted events replay identically through `ProjectionReducer`;
- internal/sensitive events never enter durable rows;
- unbound/mismatched sessions fail actionably without path fallback.

### Work package D — Snapshot/checkpoint and retention

Intent: provide bounded resync and restart reconstruction.

Required changes:

- safe snapshot bundle builder;
- no-gap initial subscribe boundary;
- incremental checkpoints;
- count/time/byte accounting;
- bounded prune batches;
- maintenance/shutdown ownership.

Acceptance evidence:

- initial snapshot plus subsequent events reconstruct current state;
- checkpoint restart replay is equivalent;
- retention floor advances without sequence reuse;
- latest usable checkpoint remains;
- project bundle truncation is explicit.

### Work package E — Subscription/ack/live delivery

Intent: deliver only relevant committed events with bounded lag.

Required changes:

- subscription registry;
- per-client/daemon caps;
- bounded queues;
- ack validation;
- transport filtering;
- disconnect/unsubscribe cleanup;
- lag-to-resync transition.

Acceptance evidence:

- project A subscription receives no project B events;
- session subscription receives no sibling-session events;
- ack cannot move ahead or cross streams;
- queue overflow cannot silently drop and continue;
- disconnect removes only transient subscription state.

### Work package F — Resume/resync/restart

Intent: make reconnect deterministic.

Required changes:

- cursor coverage/continuity query;
- paginated replay batches;
- explicit resync reasons;
- lazy restart hydration;
- corruption and failpoint handling;
- compatibility fallback for clients without projection replay capability.

Acceptance evidence:

- available cursor replays exactly missing events;
- caught-up cursor returns empty replay;
- expired/gapped/ahead/version-mismatched cursor returns bounded resync;
- daemon restart preserves accepted replay history;
- duplicate transport delivery remains reducer-idempotent.

### Work package G — Observability, docs, and broad verification

Intent: close the capability as an operable bounded subsystem.

Required changes:

- metrics/status for stream count, event bytes, retention floor/high-water, checkpoint age, subscriber count/lag, resync reasons, omitted visibility classes, and publication failures;
- architecture/protocol/storage/server/client documentation;
- focused integration fixtures and static guards;
- broad workspace validation under repository resource caps.

Acceptance evidence:

- diagnostics contain IDs/counters, not payload bodies/secrets;
- operator can distinguish history-expired, lagged, gap, version, and storage failure;
- documentation clearly separates projection replay from raw core replay and audit retention.

## 8. Concurrency and correctness model

- Serialize sequence allocation per stream with SQLite transaction/uniqueness protection; do not rely only on an in-memory mutex.
- Permit different streams to publish concurrently within SQLite's existing single-writer/resource policy.
- A single source event fan-out transaction may touch session and project streams atomically.
- Broadcast occurs only after commit.
- Checkpoint creation observes a committed high-water and records its exact checkpoint sequence.
- Pruning never deletes rows newer than a subscriber ack requirement as a special guarantee; retention is policy-based, and lagging clients resync when history expires.
- Acks are advisory flow/lag state, not database commit confirmation.
- Repeated subscribe/resume/ack/unsubscribe requests are idempotent where request IDs/subscription IDs match.
- Client disconnect races with live delivery are safe and bounded.
- Shutdown cancels maintenance and closes subscriptions after committed writes settle; it does not truncate retained replay.

## 9. Required tests

### Protocol and compatibility tests

- stream/cursor/subscription DTO round-trip;
- old capability fixtures default replay unsupported;
- version negotiation intersection/disjoint cases;
- unknown optional fields/variants;
- unsupported workspace/daemon scope;
- replay batch byte/event bounds.

### Storage and sequence tests

- additive migration idempotency;
- unique stream creation under contention;
- concurrent contiguous per-stream sequence allocation;
- independent session and project sequence spaces;
- all-or-nothing multi-stream fan-out;
- restart high-water/next-seq hydration;
- raw `core_event_log` next-seq collision regression;
- corrupt/checkpoint metadata handling.

### Publication and reducer tests

- current core event maps through M1 adapter and persisted replay reduces to the expected snapshot;
- one source event mapping to several projection events preserves order;
- duplicate delivery returns reducer duplicate without state divergence;
- missing/unbound/mismatched binding rejected;
- public event persists;
- `Internal` and `Sensitive` event payload absent from durable rows;
- `ClientLocal` durable behavior matches documented M2 gate;
- oversized serialized event rejected/downgraded safely.

### Subscription isolation tests

- project A versus project B isolation;
- session A versus sibling session B isolation;
- several subscribers to one stream receive identical committed order;
- subscription/client/global caps;
- ack monotonicity, duplicate ack, ahead ack, wrong-stream ack;
- unsubscribe/disconnect cleanup;
- queue overflow produces `SubscriberLagged` resync state.

### Replay/resync tests

- resume from zero;
- resume from middle cursor returns exactly missing events;
- resume at high-water returns empty;
- cursor ahead;
- cursor below retention floor;
- missing row/gap;
- stream/scope mismatch;
- version mismatch;
- paginated replay continuation;
- snapshot boundary race with concurrent publish has no gap;
- replay plus live transition has no missing event and reducer tolerates duplicate boundary delivery if the chosen design permits it.

### Retention/checkpoint tests

- count bound;
- age bound;
- byte bound;
- bounded prune batches;
- retention floor update;
- no sequence reuse;
- latest usable checkpoint preserved;
- checkpoint + replay equivalent to full accepted-event reduction;
- project snapshot bundle truncation explicit;
- restart from checkpoint.

### Failpoint/recovery tests

- fail before event insert;
- fail after one target stream insert before transaction commit;
- fail before stream high-water update;
- fail during checkpoint write;
- fail during pruning floor update;
- live delivery failure after successful commit;
- daemon restart after each failpoint preserves a coherent committed prefix.

### Security and negative tests

- no credentials/provider secret DTOs in replay module;
- no raw render frames or file bodies;
- no path-derived stream IDs;
- project/session scope validated through canonical binding;
- diagnostics bounded and payload-free;
- sensitive/internal publication fails closed;
- raw remote client cannot subscribe to a scope it cannot resolve through existing access checks.

## 10. Required verification commands

Inspect current target names before execution and add focused replay integration targets.

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings

cargo test -p codegg-protocol projection -- --test-threads=1
cargo test -p codegg-core session -- --test-threads=1
cargo test -p codegg --lib core::event_log -- --test-threads=1
cargo test -p codegg --lib core::daemon -- --test-threads=1
cargo test --test session_projection_consumer -- --test-threads=1
cargo test --test single_daemon_lifecycle -- --test-threads=1
cargo test --test storage_migrations -- --test-threads=1

python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_execution_ownership.py
bash scripts/check-core-boundary.sh
git diff --check
```

Add focused test binaries for projection replay storage, subscription isolation, restart/resume, retention/checkpointing, and failpoints. Run the repository's capped full-suite command after focused validation; do not increase test/process fan-out beyond established resource policy.

## 11. Documentation updates

Update:

- `architecture/projection.md` — replay module, stream identity, accepted durable visibility classes, sequence authority, checkpoint/retention policy, subscribe/resume/ack/resync, restart behavior, and M3 boundary.
- `architecture/protocol.md` — additive requests/responses/events, capability negotiation, cursor semantics, and compatibility matrix.
- `architecture/storage.md` — new tables, indexes, retention/pruning, migration, and separation from raw core/audit storage.
- `architecture/server.md` and `architecture/client.md` — filtered delivery, client ownership, reconnect flow, and lag behavior.
- `architecture/session.md` — canonical binding-to-stream routing and rebind behavior.
- operational troubleshooting — history expired, lagged subscriber, sequence gap, version mismatch, unavailable snapshot, and storage failure.

## 12. Observability requirements

Expose bounded structured metrics/status for:

- total/project/session stream count;
- stream high-water and retention-floor distance;
- retained event count and bytes;
- checkpoint count, latest age, and build duration;
- accepted versus omitted/downgraded publication counts by visibility class;
- publication/storage failures;
- active subscriptions by client/scope;
- subscriber queue depth and cursor lag;
- ack rejection reasons;
- replay batch size/count/latency;
- resync counts by reason;
- prune rows/bytes/duration;
- corrupt/quarantined stream/checkpoint count.

Never include event bodies, tool outputs, credentials, full paths, or unrestricted identifiers in logs/metrics.

## 13. Acceptance criteria

- A client can subscribe to one canonical session and receive a bounded snapshot plus live committed projection events.
- A client can subscribe to one canonical project and receives no events from another project.
- Session and project streams have opaque stable IDs and independent contiguous monotonic sequence spaces.
- Accepted projection events are committed before live delivery.
- One source event's session/project fan-out is atomic.
- Reconnect from an available cursor returns exactly the missing ordered events, in bounded batches.
- Daemon restart preserves retained stream descriptors, checkpoints, cursors, and accepted events without sequence reuse.
- A caught-up cursor returns an empty replay and resumes live delivery.
- Expired, gapped, ahead, wrong-stream, wrong-scope, rebound, and version-incompatible cursors return explicit bounded resync behavior.
- Acknowledgements are monotonic, stream-scoped, and cannot exceed committed high-water.
- Slow subscriber overflow transitions to resync-required rather than silently dropping events.
- Retention is bounded by count, age, and bytes, prunes incrementally, preserves a usable checkpoint, and never reuses sequence numbers.
- Snapshot/checkpoint plus replay reduces to the same accepted public-safe state as direct ordered reduction.
- `Internal` and original `Sensitive` payloads never enter shared durable projection replay.
- Existing raw `CoreEvent` subscribe/resume compatibility remains functional.
- `core_event_log` is not falsely presented as the projection replay store.
- Projection replay remains frontend-oriented and is not treated as the final audit log.
- Milestone 003 can add full visibility/redaction/artifact policy without replacing stream/cursor/storage ownership.

## 14. Stop conditions

Stop and report rather than improvising when:

- deterministic publication requires changing the M1 projection semantic schema rather than adding replay transport wrappers;
- SQLite cannot provide atomic session/project fan-out and contiguous stream allocation under current ownership without a broader storage ADR;
- direct `event_log.publish` call sites cannot be centralized or hooked safely enough to prevent unpersisted live projection delivery;
- canonical project/session binding cannot be resolved without path inference;
- safe durable publication would require persisting `Sensitive` or `Internal` payloads before Milestone 003;
- a principal/authorization design is required to make current scope safe; restrict the capability rather than inventing role policy;
- workspace/daemon-wide streams are needed to satisfy a current caller;
- checkpoint construction requires unbounded message/history loading;
- retention would delete the only usable resync checkpoint;
- implementation begins to replace audit, chat, or message storage;
- a new database backend appears necessary.

## 15. Closure evidence required

- implementation commit(s);
- protocol compatibility/version matrix;
- storage schema and migration evidence;
- stream identity and sequence-allocation proof;
- publication ordering diagram showing persist-before-deliver;
- inventory of centralized source publication seams;
- accepted/omitted durable visibility-class matrix;
- session/project isolation tests;
- concurrent publish and all-or-nothing fan-out tests;
- ack/lag/overflow tests;
- resume/resync matrix;
- retention/checkpoint bounds and equivalence evidence;
- daemon restart and failpoint evidence;
- raw `EventLog` high-water collision evidence;
- static security/boundary guard results;
- exact verification commands/results;
- unresolved findings and explicit M3/M4 deferrals;
- closure recommendation.

## 16. Handoff notes

- Reuse `ProjectionReducer`, `ProjectionEnvelope`, adapters, limits, and fixtures from Milestone 001. Do not create a second reducer.
- Keep replay transport metadata in additive wrapper types; do not force project cursor identity into `ProjectionEnvelope.session_id`.
- Use a separate projection replay store. `core_event_log` remains a raw-core compatibility/recovery source.
- Persist before broadcast. A best-effort insert followed by live delivery does not satisfy this milestone.
- Sequence authority is per durable stream, not the source daemon-global core event sequence.
- Support Session and Project scopes only. Reject reserved Workspace/Daemon scopes explicitly.
- Treat M2 safe-publication classification as a narrow fail-closed bridge to Milestone 003, not the final authorization/redaction policy.
- Do not make transient subscription/ack state durable unless evidence demonstrates a concrete restart requirement.
- Inspect actual current schema version and publication call sites before implementation and record them in closure evidence.
