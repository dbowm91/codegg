# Session Projection Contract

Status: implemented (M1 — projection contracts and canonical reducer;
M2 — scoped subscriptions, durable replay, daemon integration;
M005 — remote transport isolation; M006 — atomic control delivery;
agent-run/worktree M006 — durable run/worktree/group projection and closure).

Long-term references:

- `plans/000-long-term-specification.md#8-read-only-session-observation`
- `plans/000-long-term-specification.md#14-acp-integration`
- `plans/subsystems/session-projections-roadmap.md`

## Purpose

The projection is a **derived** frontend contract — never a second
session execution authority. The reducer never performs I/O, network,
filesystem, or provider calls. All frontends (local TUI, remote TUI,
observer clients, ACP adapters) consume the same logical session
model rather than re-implementing event interpretation per frontend.

Delegated-run mailbox and journal records are authoritative execution state,
not projection events. A projection or notification may summarize a child
completion for an active owning session, but reconnecting clients recover
from the durable run/control state. Projection consumers must not replay a
journal event as a provider turn or tool invocation.

## Where It Lives

| Path | Role |
|------|------|
| `crates/codegg-protocol/src/projection/mod.rs` | Module root, re-exports |
| `crates/codegg-protocol/src/projection/caps.rs` | Version/capability negotiation |
| `crates/codegg-protocol/src/projection/limits.rs` | Payload and collection bounds |
| `crates/codegg-protocol/src/projection/dto.rs` | Bounded projection DTOs |
| `crates/codegg-protocol/src/projection/event.rs` | `ProjectionEnvelope`, `ProjectionEvent` (47 variants) |
| `crates/codegg-protocol/src/projection/snapshot.rs` | `SessionProjectionSnapshot` |
| `crates/codegg-protocol/src/projection/reducer.rs` | Deterministic canonical reducer |
| `crates/codegg-protocol/src/projection/adapters.rs` | Bridges `CoreResponse`/`CoreEvent` into projection |
| `crates/codegg-protocol/src/projection/fixtures.rs` | Golden fixtures for tests |
| `crates/codegg-protocol/src/projection/replay.rs` | Durable replay transport types |
| `crates/codegg-protocol/src/projection/controller.rs` | Frontend projection client controller |
| `crates/codegg-protocol/src/projection/consumer.rs` | Non-TUI headless reference consumer |

## How It Works

### Versioning and Capability Negotiation

Two constants define the contract surface:

- `PROJECTION_PROTOCOL_VERSION` (`u32 = 1`) — current version.
- `PROJECTION_PROTOCOL_VERSION_MIN` (`u32 = 1`) — minimum
  interoperable version.

`ProjectionCapabilities` carries a `min_version..=max_version` range
plus a `supports_incremental_events` boolean. The negotiated version
is the intersection of the two sides' ranges. Versions outside the
range produce `ReducerError::UnsupportedProtocolVersion`.

The capability identifier is `PROJECTION_CAPABILITY =
"session_projection.v1"`.

### Bounded Payload and Collection Limits

All projection DTOs honour explicit caps declared in
`projection::limits`:

| Constant | Bound |
|----------|-------|
| `MAX_PROJECTION_SESSIONS` | 16 |
| `MAX_PROJECTION_MESSAGES` | 256 per turn |
| `MAX_PROJECTION_RECENT_TOOLS` | 32 per turn |
| `MAX_PROJECTION_PENDING_PERMISSIONS` | 16 |
| `MAX_PROJECTION_PENDING_QUESTIONS` | 16 |
| `MAX_PROJECTION_RUNS` | 32 |
| `MAX_PROJECTION_ARTIFACTS` | 32 |
| `MAX_PROJECTION_JOBS` | 32 |
| `MAX_PROJECTION_TOOL_PROGRAMS` | 32 |
| `MAX_PROJECTION_CALL_PAGE_SIZE` | 32 |
| `MAX_PROJECTION_TOOL_PROGRAM_CALLS` | 128 |
| `MAX_PROJECTION_NOTIFICATION_BOUND` | 16 |
| `MAX_PROJECTION_SUBAGENTS` | 16 |
| `MAX_PROJECTION_SESSION_SUBAGENTS` | 32 |
| `MAX_PROJECTION_AGENT_RUNS` | 64 |
| `MAX_PROJECTION_WORKTREES` | 32 |
| `MAX_PROJECTION_RUN_GROUPS` | 32 |
| `MAX_PROJECTION_CONVERGENCES` | 32 |
| `MAX_PROJECTION_DIAGNOSTICS` | 32 |
| `MAX_PROJECTION_DIFF_LINES` | 64 |
| `MAX_PROJECTION_STRING_BYTES` | 4,096 |
| `MAX_PROJECTION_TOOL_ARGS_BYTES` | 8,192 |
| `MAX_PROJECTION_TOOL_OUTPUT_BYTES` | 8,192 |
| `MAX_PROJECTION_RUN_SUMMARY_BYTES` | 2,048 |
| `MAX_PROJECTION_TRUNCATION_MARKER_BYTES` | 64 |

Strings exceeding their bound are truncated with
`TRUNCATION_MARKER` (`"\u{2026}[truncated]"`) rounded to the nearest
UTF-8 char boundary. Tool arguments/outputs exceeding caps become
`TruncatedArguments` / `TruncatedOutput` variants.

### Visibility Classification

Every payload field carries a `VisibilityClass` tag:

- `Public` — visible to any frontend (default).
- `ClientLocal` — active client only (subagent task ids, diagnostics).
- `Internal` — never serialized (reducer drops before publishing).
- `Sensitive` — must be redacted before leaving daemon (Milestone 3
  lands the full policy).

### Canonical Reducer

`ProjectionReducer` is pure and deterministic. It accepts a bounded
`SessionProjectionSnapshot` and an ordered stream of
`ReducerEventInput`. Each input yields one `ApplyOutcome`:

| Outcome | Meaning |
|---------|---------|
| `Applied` | Snapshot updated. |
| `Duplicate` | `event_seq` at or below snapshot's. |
| `ScopeMismatch` | `session_id` does not match. |
| `Reconciled` | Impossible transition; diagnostic recorded. |
| `ResyncRequired` | Full resync requested. |
| `Error(ReducerError)` | Protocol version or seq regression. |

Lifecycle invariants:

- Persisted row sequence is the stream-local authority. Stored
  envelopes are rewritten to their own stream's row sequence on
  persist, and the same rewritten envelope feeds live delivery, so
  replay and live agree on cursors, gap detection, and replay ranges.
  The daemon-global log sequence never appears on the wire as a
  stream cursor (M004 live-turn: global seqs on stream rows made every
  post-snapshot event look like a history gap).
- A replay/continuation on an already-live owned subscription is a
  continuation, not a birth: the daemon delivers the batch and leaves
  the live receiver/forwarder untouched instead of re-running
  activation (which trips `InvalidLifecycle(Live)` and destroys the
  healthy subscription).

- Two compliant reducers with the same `(snapshot, events)` produce
  equivalent serialized snapshots.
- Out-of-order transitions do not panic; they record a diagnostic
  and return `Reconciled`.
- The reducer never performs I/O.
- Concurrent readers may share immutable snapshot clones; one writer
  applies ordered events per projection instance.

`ProjectionState` is the public extension trait that exposes the
snapshot helpers (`upsert_secondary`, `push_recent_turn`). External
implementations MUST go through these helpers.

### ProjectionEvent (47 variants)

| Family | Variants |
|--------|----------|
| Session | `SessionActivated` |
| Turn | `TurnStarted`, `TurnCompleted`, `TurnFailed` |
| Message | `MessageAppended`, `ReasoningAppended` |
| Tool | `ToolStarted`, `ToolCompleted`, `ToolFailed` |
| Permission | `PermissionPending`, `PermissionResolved` |
| Question | `QuestionPending`, `QuestionResolved`, `LspDiagnosticsUpdated` |
| Subagent | `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed` |
| Durable agent run | `AgentRunUpserted`, `AgentRunProgress`, `AgentRunTerminal`, `AgentRunControlUpdated` |
| Worktree/group | `WorktreeUpserted`, `AgentRunGroupUpserted` |
| Convergence | `ConvergenceUpserted` |
| File | `FileChanged` |
| Run | `RunStarted`, `RunProgress`, `RunArtifactCreated`, `RunCompleted`, `RunDenied` |
| Job | `JobUpserted`, `JobRemoved` |
| Tool Program | `ToolProgramSubmitted`, `ToolProgramTerminal`, `ToolProgramAdmitted`, `ToolProgramStarted`, `ToolProgramProgress`, `ToolProgramWaitingForCall`, `ToolProgramWaitingForJob`, `ToolProgramRetryBackoff` |
| Selection | `TokenUsageUpdated`, `ModelSelected`, `AgentSelected` |
| Meta | `Diagnostic`, `ResyncRequired`, `Unknown` |

Subagent events carry a generation-safe `child_instance_id` minted at worker
dispatch and an optional immutable `originating_turn_id`. The session snapshot
keeps a separately bounded `subagents` collection (32 entries); the bounded
per-turn `agent_tree` remains a conversational compatibility view. Durable
`run_id` and `job_id` associations are explicit and never inferred from the
numeric worker task id. A daemon-local active-child registry supplies late
session snapshots for ephemeral workers; it starts empty after restart, while
AgentRunStore and JobStore rebuild durable work. Capacity is live-first and
reports an omission diagnostic when all retained child slots are live.

### Adapter Layer

`projection::adapters` bridges existing `CoreResponse` snapshot
variants and `CoreEvent` families:

- `snapshot_from_snapshot_session` — builds projection from
  `CoreResponse::SnapshotSession`.
- `snapshot_from_daemon` — from `CoreResponse::SnapshotDaemon`.
- `snapshot_from_session_snapshot` — from `SessionSnapshot`.
- `projection_events_from_core` — converts `CoreEvent` into
  projection events.
- `projection_envelopes_from_core` — wraps events in envelopes.

Adapters never replace existing core events. They add the projection
surface as an additive layer.

### Replay and Durable Subscriptions (M2)

`projection::replay` defines the transport-neutral replay protocol:

- `ProjectionStreamId`, `ProjectionSubscriptionId` — opaque IDs.
- `ProjectionStreamDescriptor` — durable stream metadata.
- `ProjectionCursor` — client-held cursor for resume.
- `ProjectionReplayBatch` — batch of replayed events with optional
  snapshot.
- `ProjectionSnapshotBundle` — `One` or `BoundedSessionList`.
- `ProjectionAck` — acknowledgement of processed events.
- `ProjectionArtifactReadRequest/Response` — bounded artifact reads.

Replay caps: `MAX_REPLAY_EVENTS = 512`, `MAX_REPLAY_BYTES = 1 MB`,
`MAX_REPLAY_EVENT_BYTES = 64 KB`.

### Frontend Controller (M2)

`ProjectionClientController` is a transport-neutral state machine
that:

1. Negotiates projection capabilities with the daemon.
2. Selects a `ProjectionMode` (`ProjectionPrimary`,
   `RawCompatibility`, or `Unsupported`).
3. Subscribes to scoped projection streams.
4. Applies events through the canonical reducer.
5. Acknowledges cursors with bounded cadence.
6. Handles resync / restart / version mismatch.

Key constants: `MAX_CONTROLLER_SUBSCRIPTIONS = 16`,
`MAX_OUTSTANDING_LAG = 1024`, `DEFAULT_ACK_CADENCE = 16`.

### Headless Reference Consumer (M008)

`projection::consumer::HeadlessProjectionConsumer` is a small non-TUI
consumer of the canonical `CoreResponse` projection surface. It is intended
for automation, observer clients, and protocol acceptance tests. The
authenticated transport remains responsible for establishing the connection;
the consumer negotiates projection capabilities, installs a single-session
snapshot, applies ordered events, and emits typed subscribe/resume/ack/artifact
requests for the transport to send.

The consumer retains only bounded ephemeral state and the last accepted
`ProjectionCursor` across a disconnect. It treats duplicate events as
idempotent, turns sequence gaps and stream mismatches into explicit resync
state, ignores internal reasoning and non-public diagnostics, and accepts
artifact bodies only when the handle is project-scoped and the read is within
the 64 KiB protocol limit. `session_is_terminal()` and
`run_is_terminal()` expose terminal observation without creating execution
authority or importing TUI state.

### Remote Transport Isolation (M005)

`ProjectionConnectionState` is a transport-neutral transient owner
shared by Unix socket, `/core`, and `/tui` adapters. It bounds
subscriptions, artifact reads, diagnostics, and reconnect generations.

Raw event forwarders explicitly discard
`CoreEvent::ProjectionStreamEvent`. Projection-private envelopes
originate only from the receiver owned by the matching authenticated
connection.

Discarding is a **transport boundary** rule, not a rule that projection
data is never consumed. A generic forwarder must not carry one
connection's envelopes into another connection's frame, because the
envelopes are scoped to a subscription that connection itself
established and the daemon authorized. The frontend that *owns* such a
subscription is required to consume its envelopes — it is the only party
allowed to see them.

### Daemon-attached seeding and live delivery

Two frontends consume projections and they must both seed the controller:

- **WS-attach** receives `ProjectionReplay` / `ProjectionEvent` frames on
  `remote_event_rx` and installs them via `install_replay`.
- **Daemon socket** (`codegg`, the default) establishes the subscription
  through `ProjectionSubscribe` and receives live envelopes as
  `CoreEvent::ProjectionStreamEvent` on the core-event channel.

`ProjectionSubscribed` carries an authoritative
`ProjectionSnapshotBundle`. That snapshot must be installed, not
discarded: the daemon sends current state *at subscribe time*, so a
frontend that drops it knows only what changed after it attached, and an
otherwise healthy session projects as empty until some unrelated live
event happens to arrive.

Two consequences are load-bearing:

- `apply_core_event` must have a `ProjectionStreamEvent` arm. Falling
  through to a catch-all `_ => false` silently discards live envelopes,
  which looks identical to a projection that has nothing to report.
- `active_snapshot()` must not require `active_tab_id`. The
  daemon-attached TUI subscribes per session and never switches a project
  tab, so that id is legitimately `None`. With exactly one live
  subscription the stream is unambiguous and must be returned; with two or
  more it must stay `None`, because guessing would attribute another
  session's jobs to the visible one.

### Atomic Control Delivery (M006)

Remote projection control responses are critical writer operations.
The adapter serializes the frame, enqueues on a bounded control
channel, and waits for a bounded writer receipt (500 ms). `Initializing
-> Live` completes only after that receipt.

### Durable delegated-run projection (agent-run/worktree M006)

`AgentRunSummary`, `WorktreeSummaryProjection`, and
`AgentRunGroupSummaryProjection` are bounded, additive frontend summaries of
the authoritative `AgentRunStore`, `WorktreeService`, and group service. They
carry typed opaque IDs, lifecycle/control state, authoritative durable depth,
branch/base/result commit, validation and attention summaries; prompts,
mailboxes, transcripts, hidden reasoning, credentials, and full artifacts are
not projected. The daemon builds reconnect/resync snapshots from those stores,
while scheduler and group transitions publish the same summaries as projection
events.

Run-group summaries include an additive owner discriminator (`turn` or `run`)
and, for turn-owned groups, bounded originating session/turn identifiers.
Older snapshots deserialize with the fields absent. These fields describe
ownership only; they do not grant control authority, which remains enforced by
the durable group/control services.

Member-terminal reconciliation publishes an `AgentRunGroupUpdated`
`AppEvent` (`crates/codegg-core/src/bus/events.rs:365`), which the
adapter projects as `AgentRunGroupUpserted`, for the authoritative
recomputed summary, including terminal summaries reached
without a later status_group request. The event's session routing comes from
the persisted turn owner or the owner run's originating task. Terminal live
follow-up delivery is separately claim-gated by durable group notification
state, so replay can refresh projections without duplicating user messages.

### Bounded workstream view (agent-workstream-blackboard M002)

`projection::workstream::project_workstream` derives an `AgentWorkstreamSnapshot`
from one already authorized `SessionProjectionSnapshot`. Its revision is the
source snapshot's `event_seq`, so reconnect and restart use the existing
snapshot/replay cursor and do not introduce another journal or polling loop.
The pure adapter joins a run group to at most 16 run, job, and worktree facts;
missing or mismatched run facts are listed as unavailable, and linked jobs or
worktrees distinguish unavailable records from observed state. The response
omits task descriptions, job summaries, terminal prose, paths, prompts, and
logs. It grants no control authority and performs no storage access.

Agent Workstream Blackboard M003 adds `project_workstream_with_claims`, which
accepts bounded claims and dependency summaries supplied by the authorized
group reader. It filters them to the source project/workspace/group and caps
claims at 128 and projected dependency edges at 512. This is a derived view;
the daemon claim store remains authoritative for revisions and ownership.

M004 adds claim/dependency summaries to authorized run-group projection
updates for display. A complete refresh carries `workstream_claims_updated_at`;
ordinary group lifecycle updates preserve the last complete board, while an
explicit refresh with empty lists clears released claims. These fields confer
no authority. Child loops receive same-group global-bus events only as
push hints, reduced to an allowlist and injected as untrusted user data at a
provider boundary. They restore membership from the durable group store after
restart, filter by the originating session, and emit a continuity-gap notice
when the bounded event stream has a gap. The task board remains the canonical
read path and identifies itself as snapshot-only when no replay cursor exists.

The reducer only upserts derived state. It never sends a message, cancels a
run, allocates a worktree, or submits a job. Existing numeric subagent events
and `task get` remain compatibility adapters for the migration window; new
clients should use typed run IDs, `wait`/group joins, and push notifications.
Older snapshots deserialize with empty durable-run/worktree/group collections
because the new fields are serde-defaulted, and older clients ignore the
additive fields/events through the existing negotiated protocol.

### Fixtures and Independent Consumers

`projection::fixtures` provides golden snapshots and event scripts:
`idle_snapshot`, `active_turn_event_script`, `completed_snapshot`,
`permission_event_script`, `subagent_event_script`,
`file_change_event_script`, `job_event_script`,
`question_event_script`, `project_summary_fixture`.

The canonical reducer and the headless consumer are the production owners of
projection interpretation. Two independent consumers are exercised by the
test corpus:

1. In-crate reducer tests in `projection::reducer::tests`.
2. Root integration test `tests/core_daemon/headless_projection_consumer.inc` drives
   `HeadlessProjectionConsumer` with typed capability, subscribe, replay,
   resume, terminal-state, and artifact responses.

The former `tests/session_projection_consumer.rs` binary was consolidated
into `tests/session_family/projection_consumer.inc` (M004) and is now
reachable through the `session_family` target. It remains a bounded
equivalence fixture for the existing TUI-shaped adapter; it is not the
headless consumer or a second projection protocol.

## Key Types & APIs

| Type | File:line | Purpose |
|------|-----------|---------|
| `ProjectionCapabilities` | `caps.rs:40` | Version negotiation |
| `SessionProjectionSnapshot` | `snapshot.rs:29` | Bounded session snapshot |
| `ProjectionReducer` | `reducer.rs:236` | Canonical pure reducer |
| `ReducerEventInput` | `reducer.rs:143` | Lightweight reducer input |
| `ApplyOutcome` | `reducer.rs:112` | Reducer application result |
| `ProjectionEvent` | `event.rs:128` | 47-variant event enum |
| `ProjectionEnvelope` | `event.rs:54` | Event envelope with metadata |
| `ProjectionStreamScope` | `event.rs:40` | Session/Project/Workspace/Daemon |
| `ProjectionClientController` | `controller.rs:219` | Frontend state machine |
| `HeadlessProjectionConsumer` | `consumer.rs` | Non-TUI snapshot/replay/resume/artifact consumer |
| `ProjectionMode` | `controller.rs:83` | ProjectionPrimary/RawCompat/Unsupported |
| `ProjectionStreamId` | `replay.rs:41` | Opaque stream identifier |
| `ProjectionCursor` | `replay.rs:107` | Client cursor for resume |
| `ProjectionReplayBatch` | `replay.rs:177` | Replay event batch |
| `ToolProgramSummary` | `dto.rs:822` | Background tool program state |
| `ToolProgramDetail` | `dto.rs:947` | Full tool program inspection |
| `ToolProgramCallPage` | `dto.rs:918` | Paginated call history |
| `VisibilityClass` | `dto.rs:27` | Public/ClientLocal/Internal/Sensitive |

## Configuration Surface

No runtime configuration. All limits, versions, and caps are
compile-time constants. The projection protocol version is bumped
when additive changes land that the reducer MUST interpret.

## Background job visibility

The default `/core` TUI owns one projection subscription for its active
session. It negotiates projection support, installs the authorized
`ProjectionSubscribed` snapshot, applies only envelopes matching that
subscription and stream, and acknowledges at the controller's existing
cadence. Events arriving while the UI loop is applying the subscribe response
are retained in a bounded queue and drained after snapshot installation. A
switch or reconnect clears the prior generation and takes a fresh snapshot.
The TUI presents loading and unavailable states separately from an
authoritative empty snapshot. `/observe` remains an independent read-only
subscription.

`JobSubmissionService` publishes `JobCreated` after durable creation and
successful scheduler enqueue, covering direct tool submissions and the
`CoreRequest::JobSubmit` path through the same seam. Scheduler admission and
release events publish owner-scoped started and terminal transitions after
resolving the durable `JobRecord`; ownerless lifecycle events are dropped with
a diagnostic. Session snapshot assembly queries `JobStore` by owner, selects
live jobs before recent terminals, and reports bounded truncation or read
failure diagnostics. `JobUpserted` merge rules prevent delayed created/running
events from regressing a later state, while a new retry attempt may reopen a
terminal job.

This lifecycle stream complements the durable
`SessionEvent::JobTerminalNotification` completion history. It does not add a
frontend database query or polling loop. The `job` model tool continues to
provide cancel/wait/inspect over the same store. Reconnect currently obtains a
fresh snapshot rather than resuming from a retained primary-TUI cursor.

C004 adds one additive live hint on the same durability: a persisted,
session-scoped `CoreEvent::BackgroundJobCompleted` (identity, kind,
terminal status, bounded redacted summary; `Safe` publication) published
immediately after the notice commits, without waiting for the next turn.
Reconnect and daemon restart replay the identical notice id/status exactly
once via `core_event_log`; the notice row (plus next-turn session event)
remains the authority and the toast is never a second database.

The hint adapts to the existing session job list for projection
consumers (ADR-0031 slice): `projection_events_from_core` maps
`BackgroundJobCompleted` to `ProjectionEvent::JobUpserted` (terminal
state/summary, session-scoped; `workspace_id` empty by the same
convention as the `JobStarted`/`JobCompleted` arms — the envelope
carries no workspace and adapters must not fabricate one), so
desktop/remote clients observe the terminal on their authorized
`ProjectionSubscribe`/`Resume` stream with no new event type and no new
authority. The native TUI renders the same event as a session-scoped
info toast (`apply_core_event`; foreign sessions never toast). Crash
between the notice commit and the hint publish heals by bounded startup
reconciliation (`reconcile_idle_completion_hints`, deduped by notice
id — never a second notice, never a turn); the forwarder skips hints
already persisted (cross-instance duplicate absorption).

## Invariants & Gotchas

1. **Derived, not authoritative**: The projection is never a second
   session execution authority. The reducer never performs I/O.

2. **Deterministic**: Two compliant reducers with the same
   `(snapshot, events)` MUST produce equivalent serialized snapshots.

3. **Additive-only**: New `ProjectionEvent` variants MUST NOT cause
   existing reducers to reject older events. Unknown variants map to
   `Unknown { variant_name, notice }`.

4. **No credentials in DTOs**: The adapter layer never imports
   `SecretInput` / `SecretInputRef` types. Tool outputs exceeding
   bounds become `TruncatedOutput`.

5. **Transport isolation**: Raw event forwarders discard
   `ProjectionStreamEvent`. Projection-private envelopes originate
   only from the matching authenticated connection's receiver. The
   connection's **own** frontend still consumes them — see "Daemon-attached
   seeding and live delivery".

6. **Publication is retried, not dropped**: `SeamProjectionSink::publish`
   retries bounded times (3 attempts, 50 ms apart) when the failure is
   transient SQLite lock contention, and gives up immediately otherwise.
   The daemon's pool runs several connections against one WAL database, so
   a writer can collide with a reader's snapshot; `busy_timeout` does not
   cover the WAL snapshot-conflict variant (`SQLITE_BUSY_SNAPSHOT`, 517).
   Logging a warning and discarding was how the projection ended up
   permanently short of job and subagent lifecycle events: `JobUpserted`
   stopped appearing entirely while jobs kept being created, and the
   frontend then had nothing to show for work that was genuinely running.

7. **RenderFrame unsupported**: The `/tui` protocol does not support
   `RenderFrame`. It is event/state-driven via `TuiCommand`.

8. **Protocol version 5**: The `/tui` protocol version is 5.
   Projection-primary clients negotiate before using resume,
   acknowledgement, or unsubscribe.

## Testing

```bash
# Inline unit tests (caps, limits, dto, event, snapshot, reducer,
# adapters, fixtures, controller)
cargo test -p codegg-protocol

# Independent consumer equivalence test
cargo test --test session_family

# Headless reference consumer and canonical CoreResponse flow
cargo test --test headless_projection_consumer

# Adversarial tests
cargo test --test context_projection_adversarial

# Projection replay family (consolidated binary, M003 pilot): 11 modules
cargo test --test projection_replay

# Remote transport tests (needs server feature)
cargo test --test projection_transport_real --features server

# Lint
cargo clippy -p codegg-protocol --all-targets --all-features -- -D warnings
```

## Static Guards

```bash
bash scripts/check_projection_disclosure.sh
bash scripts/check_projection_publication_seam.sh
python3 scripts/check_projection_transport_isolation.py
python3 scripts/check_projection_transport_lifecycle.py
python3 scripts/check_websocket_bounds.py
```

## Related Docs

- `architecture/server.md` — HTTP/WebSocket server
- `architecture/tui.md` — local TUI
- `architecture/acp.md` — ACP adapter
- `architecture/bus.md` — event bus

## Source verification

Verified 2026-10-06 against `crates/codegg-protocol/src/projection/`.
Corrected the `ProjectionEvent` variant count 46 → 47 and added the two
missing variants: `LspDiagnosticsUpdated` (listed under Question; the
reducer turns it into a diagnostic because it belongs to the project
projection stream) and `ConvergenceUpserted` (new Convergence family
row). Added the missing `MAX_PROJECTION_CONVERGENCES` = 32 row to the
limits table. Corrected `file.rs:line` refs: `ProjectionStreamScope`
`event.rs:38` → `:40`, `ProjectionCursor` `replay.rs:106` → `:107`,
`ProjectionReplayBatch` `replay.rs:161` → `:177`, `VisibilityClass`
`dto.rs:26` → `:27`. Clarified that `AgentRunGroupUpdated` is a bus
`AppEvent` (`crates/codegg-core/src/bus/events.rs:365`) that the adapter
projects as `AgentRunGroupUpserted`. Verified accurate: every other
limit value, the three replay caps (512 events / 1 MiB / 64 KiB), the
three controller caps, `PROJECTION_PROTOCOL_VERSION` /
`PROJECTION_PROTOCOL_VERSION_MIN` = 1, `PROJECTION_CAPABILITY`, the six
`ApplyOutcome` variants, the four `ProjectionStreamScope` variants, and
`REMOTE_TUI_PROTOCOL_VERSION` = 5. Retargeted the two references to the
deleted `tests/session_projection_consumer.rs` binary (M004 consolidated
it into `tests/session_family/projection_consumer.inc`): the Testing entry
now runs `--test session_family`, and the fixture paragraph names the
consolidated module. All five static guards pass today.
