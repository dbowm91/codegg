# Session Projections Milestone 002 — Corrective Daemon Integration and Closure

Status: implemented (closed at this commit)

Repository baseline: `8c232697063d86545c16ac33226832a71397450e` (`main`; subsequent planning-only commits do not alter production behavior)

Source milestone:

- `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md`
- `plans/subsystems/session-projections-roadmap.md#milestone-2--scoped-subscriptions-and-durable-replay`

Closure evidence driving this corrective pass:

- `plans/closure/session-projections/002-status.md`
- `8dc4b85` — library/crate implementation of scoped subscriptions and durable replay
- `8c23269` — conditional closure record identifying unresolved daemon integration

Primary class: correctness / integration closure

## 1. Objective

Complete Session Projections Milestone 002 by wiring the landed replay subsystem into the production daemon and transports without creating a second event authority or weakening existing raw `CoreEvent` compatibility.

The pass must:

1. route every production core event through one centralized publication seam;
2. preserve the legacy raw `EventLog` path for existing clients;
3. persist accepted projection events before projection live delivery;
4. dispatch every additive `CoreRequest::Projection*` operation through `CoreDaemon`;
5. deliver `CoreEvent::ProjectionStreamEvent` only to the owning subscription/client;
6. use canonical project/workspace/session bindings and real stream descriptors rather than empty or placeholder identifiers;
7. invalidate/recreate streams when the canonical session binding revision changes;
8. prove restart, replay, isolation, lag, cancellation, and compatibility behavior in daemon-level integration tests;
9. update documentation and replace the conditional closure record with strict closure evidence.

Milestone 003 must remain blocked until this plan closes.

## 2. Why this corrective pass is dependency-ready

The underlying protocol, storage, replay, retention, subscription, and metrics layers already exist:

- `codegg_protocol::projection::replay` defines stream descriptors, cursors, subscription requests, replay batches, acknowledgements, resync reasons, limits, and capability declarations;
- `CoreRequest`, `CoreResponse`, and `CoreEvent` contain additive projection variants;
- storage migration v32 defines `projection_stream`, `projection_event`, and `projection_checkpoint`;
- `ProjectionReplayStore`, `ProjectionReplayService`, `SubscriptionRegistry`, `RetentionPolicy`, `SafePublicationGate`, and metrics are implemented;
- focused storage/subscription/resume/retention/failpoint tests are present;
- `ProjectionReplayHandle` provides a bounded service façade.

No new replay architecture or database backend is required. This is a production integration and correctness pass against the landed design.

## 3. Current production evidence and defects

At baseline `8c23269`:

- `ProjectionReplayHandle::publish_core_event` calls only the projection service. It does not replace or wrap the daemon's legacy `EventLog::publish` authority.
- Production daemon, turn-runtime, and bridge call sites still invoke `event_log.publish(...)` directly, so production events do not populate projection replay storage.
- `CoreDaemon::handle_request` does not dispatch the new projection request family.
- daemon-socket transport does not route projection live events by subscription ownership.
- the subscription service registers a receiver but drops it, leaving no durable transport-owned live-delivery channel.
- `ProjectionReplayService::publish_from_core` currently creates session streams with empty project/workspace context and project streams with an empty project ID.
- post-commit delivery uses synthetic `"session-stream"` / `"project-stream"` identifiers instead of the actual persisted stream descriptors.
- projection adaptation contains incomplete context values for several event families; project/workspace/session association must be resolved from daemon-owned canonical stores before durable publication.
- session binding revision is stored in the schema but is not checked during publication, so a rebound session may continue writing to the old project stream.
- the original M2 plan still says `ready for handoff` even though the library implementation landed and the closure record is conditional.

These are closure-blocking correctness issues, not optional polish.

## 4. Invariants

- `CoreDaemon` remains the only production event-publication authority.
- Every source event is assigned one legacy raw event envelope and is observed by projection publication at most once.
- Existing raw `CoreEvent` clients continue to receive the same additive-compatible event stream.
- Projection persistence completes before a projection event becomes eligible for live delivery or acknowledgement.
- Projection failure never produces a live projection event without a committed replay row.
- Projection publication failure is observable and fail-closed for projection consumers; it must not silently corrupt or skip cursor state.
- A session stream is keyed only by canonical `SessionId` plus its current canonical binding revision.
- A project stream is keyed only by canonical `ProjectId`.
- Paths, labels, tab IDs, socket client IDs, and compatibility directories never define stream identity.
- The actual persisted `ProjectionStreamDescriptor.stream_id` is used for queue delivery, cursor validation, replay, and acknowledgement.
- A subscription receiver has one explicit runtime owner and is cleaned up on unsubscribe, disconnect, daemon shutdown, or lag transition.
- Projection events are delivered only to the client that owns the matching subscription ID.
- Session rebind invalidates the old stream generation and forces existing cursors to resync with a stable mismatch reason.
- Unbound, ambiguous, archived, unresolved, or revision-mismatched session context fails closed without consuming a stream sequence.
- Existing retention, queue, replay-size, subscription-count, and snapshot bounds remain enforced.
- The canonical `ProjectionReducer` remains pure and unchanged unless a concrete compatibility defect requires an additive fix.
- No frontend adoption, role policy, or final redaction policy is implemented in this corrective pass.

## 5. Scope

### In scope

- centralized dual-publication façade or `EventLog` sink hook;
- migration of all production raw event publication paths to that seam;
- canonical binding lookup and revision validation;
- real stream descriptor propagation through persistence and live delivery;
- projection request dispatch in `CoreDaemon`;
- projection subscription receiver ownership and transport routing;
- per-client subscription ownership checks;
- disconnect/unsubscribe/shutdown cleanup;
- capability negotiation and unsupported behavior;
- daemon startup construction of replay store/service/handle;
- maintenance-loop integration for retention/checkpoints;
- daemon/socket/in-process/stdio/server compatibility;
- failpoint, restart, rebind, isolation, lag, and backward-compatibility tests;
- metrics and documentation;
- strict M2 closure record.

### Explicitly out of scope

- Session Projections Milestone 003 visibility/redaction/artifact policy;
- final principals, roles, authorization, observer mode, presence, or chat;
- frontend migration to `SessionProjectionSnapshot`;
- workspace-wide or daemon-wide projection subscriptions;
- cross-daemon replication or global ordering;
- replacing `core_event_log` for legacy clients;
- replacing SQLite or session/message storage;
- persistent client presence;
- changing project/session identity semantics;
- broad event DTO redesign.

## 6. Required architecture

### 6.1 One production publication seam

Introduce one daemon-owned publication abstraction, for example `CoreEventPublisher`, or install one bounded projection sink inside `EventLog`.

The selected design must:

1. construct or receive the single canonical `EventEnvelope<CoreEvent>`;
2. retain legacy ring/database/broadcast behavior;
3. invoke projection adaptation/publication exactly once;
4. prevent recursive publication when `ProjectionStreamEvent` itself is emitted;
5. return structured legacy/projection outcomes for diagnostics;
6. avoid requiring every caller to understand replay storage.

Do not leave a mixed system where some call sites use `EventLog` and others use `ProjectionReplayHandle` directly.

Inventory and migrate all production sites, including:

- `src/core/daemon.rs`;
- `src/agent/turn_runtime.rs`;
- compatibility/event-bus bridges;
- any scheduler/run/job/test publication helpers;
- socket/server adapters that synthesize core events.

Add a static or structural guard that prevents new direct production `event_log.publish(...)` calls outside the centralized implementation and tests.

### 6.2 Canonical context resolution

Before projection adaptation or stream creation:

- resolve the source session through canonical session storage;
- obtain `ProjectId`, `WorkspaceId`, binding status, and binding revision;
- reject unresolved or ambiguous bindings;
- verify the event's explicit session/project/workspace fields do not conflict with canonical state;
- pass typed canonical context into adaptation rather than filling DTO fields with empty strings.

Refactor the publication API to accept a context object such as:

```text
ProjectionPublicationContext
|-- SessionId
|-- ProjectId
|-- WorkspaceId
|-- binding_revision
|-- source_event_seq
`-- source_timestamp
```

The adapter may still map event-specific payloads, but it must not invent identity.

### 6.3 Real stream identity and transactional fan-out

Refactor `publish_from_core` so the transaction retains the actual session and project stream descriptors used during insertion.

Required behavior:

- get or create the session stream using canonical session/project/workspace IDs and binding revision;
- get or create the project stream using canonical project ID;
- allocate each stream sequence transactionally;
- insert all derived events and update high-water marks in one transaction;
- commit;
- deliver using the exact persisted stream IDs and committed stream sequences;
- never construct placeholder stream IDs after commit;
- do not allocate a sequence for events rejected by the safe-publication/context gate.

Add failpoints between allocation, insertion, high-water update, commit, and live delivery.

### 6.4 Binding-revision lifecycle

Thread `SessionBindingRecord::revision` through stream creation and lookup.

On publication or subscribe:

- if the active stream revision matches the canonical binding revision, continue;
- if the session has rebound, invalidate the previous stream with a stable reason;
- stop delivery on old subscriptions;
- create or activate the new session stream associated with the new project/workspace/revision;
- return `StreamMismatch` or a more precise additive resync reason to old cursors;
- ensure the event is not written to the former project stream.

Cover rebind, archive/restore, restart after rebind, and concurrent publish/rebind races.

### 6.5 Daemon request dispatch

Implement exhaustive daemon handling for:

- projection capability inquiry;
- subscribe;
- resume;
- acknowledge;
- unsubscribe;
- snapshot get;
- subscription status.

Dispatch must:

- validate negotiated projection version;
- validate client/subscription ownership;
- validate canonical scope IDs;
- reject workspace/daemon scopes as explicitly unsupported;
- map storage/subscription failures to stable bounded protocol errors;
- return bounded snapshots/replay batches;
- avoid leaking payload bodies in diagnostics.

Use helper conversion functions rather than one oversized match arm.

### 6.6 Live receiver and transport ownership

Change subscription registration so the receiver is returned to and retained by an explicit daemon transport runtime owner.

Required semantics:

- one bounded receiver per subscription;
- one client owns each subscription ID;
- socket clients receive only their own `ProjectionStreamEvent` values;
- stdio and in-process clients have equivalent APIs/behavior;
- unsubscribe or disconnect drops the receiver and registry entry;
- queue overflow sends one bounded resync/status transition when possible, then stops incremental delivery;
- reconnect uses resume/cursor APIs rather than reusing an orphan receiver;
- raw core-event broadcast remains unchanged for legacy subscribers.

Do not route projection events through the unfiltered global raw-event broadcast channel.

### 6.7 Startup, shutdown, and maintenance

Construct the replay store/service/handle from the daemon's canonical SQLite pool during daemon startup.

- schema migration must complete before replay service construction;
- startup must not eagerly hydrate every stream;
- retained high-water and binding revisions must be loaded safely on first use;
- a bounded maintenance tick handles retention/checkpoints;
- shutdown removes transient subscriptions without deleting replay history;
- service absence or migration failure produces an actionable startup/health diagnostic rather than a partially enabled capability.

### 6.8 Documentation and closure hygiene

Update:

- `architecture/projection.md`;
- `architecture/protocol.md`;
- `architecture/core.md` or daemon architecture documentation;
- transport/client documentation;
- troubleshooting/metrics documentation;
- the original M2 implementation plan status;
- subsystem roadmap and registry;
- `plans/closure/session-projections/002-status.md` or a corrective closure record.

The final record must clearly distinguish raw core replay from canonical projection replay.

## 7. Ordered work packages

### Work package A — Correctness inventory and publication façade

- enumerate every production raw publication site;
- choose and document the single publication architecture;
- install the façade/hook;
- migrate all sites;
- add a direct-publish guard.

Exit evidence: one publication path in production and unchanged raw-client behavior.

### Work package B — Canonical context and stream correctness

- add canonical publication context;
- remove empty project/workspace stream creation;
- remove synthetic stream IDs;
- preserve actual descriptors through commit/delivery;
- add binding-revision invalidation.

Exit evidence: session rebind cannot publish to the old project stream.

### Work package C — Daemon protocol dispatch

- implement request handlers and response mapping;
- enforce capability/version/scope/ownership checks;
- add daemon-level request tests across all variants.

Exit evidence: every additive projection request round-trips through `CoreDaemon`.

### Work package D — Live transport routing

- retain subscription receivers;
- route by subscription/client ownership;
- clean up on unsubscribe/disconnect/shutdown;
- handle queue overflow/resync.

Exit evidence: two clients with different subscriptions cannot observe each other's projection events.

### Work package E — Restart, maintenance, and failure semantics

- wire startup and maintenance;
- test restart high-water and binding revision;
- test failpoints and lag;
- confirm no acknowledged-but-unpersisted events.

Exit evidence: retained cursors replay correctly across daemon restart.

### Work package F — Documentation and strict closure

- update architecture/operations docs;
- run full verification;
- record exact results and residual findings;
- mark M2 closed only if every acceptance criterion passes;
- register M3 only after its separate principal-capability dependency is verified.

## 8. Failure and concurrency semantics

- If projection adaptation rejects an event, record a bounded skipped outcome and continue the raw compatibility path.
- If projection persistence fails, emit no projection live event and increment failure metrics.
- Do not advance a projection stream's committed high-water on rollback.
- Concurrent events for one stream serialize sequence allocation transactionally.
- Concurrent subscribe/unsubscribe/disconnect operations must not leak registry entries or receivers.
- Concurrent rebind and publication must resolve to either the old fully committed binding revision or the new fully committed revision; no event may fan out across mismatched projects.
- Duplicate source delivery may produce a bounded duplicate diagnostic or idempotent result, but must not silently create divergent reducer state.
- A cursor from an invalidated binding revision always resyncs; it is never rebound implicitly.
- Maintenance pruning must not delete the only checkpoint needed to recover the retention floor.

## 9. Required tests

### Publication integration

- every inventoried production event family reaches legacy raw delivery;
- accepted events also reach projection storage;
- internal/sensitive rejected events do not consume projection sequence numbers;
- direct-publish guard catches a new unauthorized call site;
- projection publication failure produces no projection live event.

### Stream/context correctness

- session/project streams contain canonical non-empty IDs;
- actual persisted stream IDs are used for live delivery;
- Project A never receives Project B events;
- Session A never receives sibling Session B events;
- ambiguous/unbound/archived bindings fail closed;
- rebind invalidates old stream and moves new events to the new project stream;
- concurrent rebind/publication test.

### Protocol dispatch

- capabilities;
- subscribe session/project;
- unsupported workspace/daemon scope;
- resume replay/empty/resync cases;
- ack monotonicity and ahead rejection;
- unsubscribe and status;
- wrong-client subscription access denied;
- incompatible projection version.

### Transport lifecycle

- socket, stdio, and in-process subscription delivery;
- two-client isolation;
- disconnect cleanup;
- queue overflow -> `SubscriberLagged` resync;
- reconnect/resume without orphan receiver;
- legacy raw event subscriber regression.

### Restart and failpoints

- daemon restart preserves projection events and high-water;
- no sequence reuse;
- failure before commit rolls back session/project fan-out;
- failure after commit but before delivery resumes correctly;
- maintenance/retention retains usable checkpoint;
- restart after session rebind preserves the new binding revision.

### Security and negative tests

- no credential/provider-secret fields in stored projection rows;
- no raw hidden reasoning or unrestricted file bodies;
- bounded error messages and metrics;
- invalid stream/subscription IDs rejected;
- paths and tab IDs cannot select streams;
- client cannot subscribe to or acknowledge another client's subscription.

## 10. Verification commands

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p codegg-protocol
cargo test -p codegg-core projection_replay
cargo test --test projection_replay_storage
cargo test --test projection_replay_subscription
cargo test --test projection_replay_resume
cargo test --test projection_replay_retention
cargo test --test projection_replay_failpoint
cargo test --test projection_replay_safe_publication
cargo test --test single_daemon_lifecycle
cargo test --test core_daemon_protocol
cargo test --test daemon_socket_integration
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=14
bash scripts/check-core-boundary.sh
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
git diff --check
```

Use the actual test target names present at implementation time. If a named target does not exist, record the equivalent command rather than silently omitting coverage.

## 11. Acceptance criteria

- one production publication seam covers every core event source;
- legacy raw event clients remain compatible;
- accepted projection events persist before projection delivery;
- canonical non-empty project/workspace/session context is used;
- real persisted stream IDs are delivered and acknowledged;
- all projection protocol operations dispatch through the daemon;
- live projection events are isolated by client and subscription;
- subscription receivers have explicit ownership and cleanup;
- session rebind invalidates old streams and prevents cross-project publication;
- restart preserves retained replay and never reuses sequence numbers;
- lag, gaps, expiry, mismatch, and version conflicts produce explicit resync;
- no secret-bearing or unbounded content enters replay storage;
- all focused and broad verification passes;
- M2 closure record is strict `closed`, not conditional;
- M3 remains unregistered until the principal-capability interface is independently ready.

## 12. Stop conditions

Stop and report rather than improvising if:

- completing the pass requires replacing SQLite or the event-log authority model;
- project/session binding cannot be resolved without path-derived identity;
- legacy raw compatibility requires duplicate or recursively published events;
- subscription ownership requires introducing final team authorization policy;
- a request demands workspace/daemon projection streams;
- the current replay schema cannot represent binding revision without destructive migration;
- fixing the integration requires changing the M1 projection semantic contract incompatibly.

## 13. Closure evidence required

- implementation commit(s);
- complete production publication-site inventory before/after;
- request/response dispatch matrix;
- stream/context and binding-revision evidence;
- two-client transport isolation evidence;
- restart/failpoint/retention evidence;
- legacy raw-client compatibility results;
- exact verification command results;
- static guard results;
- unresolved findings with severity;
- strict close/defer recommendation for Milestone 003.

## 14. Handoff notes

- Do not reimplement the replay store, reducer, or DTOs.
- Treat `8dc4b85` as the library baseline and correct its integration defects in place.
- Prefer one centralized publisher over editing dozens of call sites with ad hoc dual writes.
- Preserve raw event compatibility while making projection replay a first-class daemon service.
- Inspect current `main` before implementation and record the exact production baseline in the closure record.
