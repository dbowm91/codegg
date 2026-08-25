# Session Projection Contract

Status: implemented (M1 — projection contracts and canonical reducer;
M2 — scoped subscriptions, durable replay, daemon integration;
M005 — remote transport isolation; M006 — atomic control delivery).

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

## Where It Lives

| Path | Role |
|------|------|
| `crates/codegg-protocol/src/projection/mod.rs` | Module root, re-exports |
| `crates/codegg-protocol/src/projection/caps.rs` | Version/capability negotiation |
| `crates/codegg-protocol/src/projection/limits.rs` | Payload and collection bounds |
| `crates/codegg-protocol/src/projection/dto.rs` | Bounded projection DTOs |
| `crates/codegg-protocol/src/projection/event.rs` | `ProjectionEnvelope`, `ProjectionEvent` (39 variants) |
| `crates/codegg-protocol/src/projection/snapshot.rs` | `SessionProjectionSnapshot` |
| `crates/codegg-protocol/src/projection/reducer.rs` | Deterministic canonical reducer |
| `crates/codegg-protocol/src/projection/adapters.rs` | Bridges `CoreResponse`/`CoreEvent` into projection |
| `crates/codegg-protocol/src/projection/fixtures.rs` | Golden fixtures for tests |
| `crates/codegg-protocol/src/projection/replay.rs` | Durable replay transport types |
| `crates/codegg-protocol/src/projection/controller.rs` | Frontend projection client controller |

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

### ProjectionEvent (39 variants)

| Family | Variants |
|--------|----------|
| Session | `SessionActivated` |
| Turn | `TurnStarted`, `TurnCompleted`, `TurnFailed` |
| Message | `MessageAppended`, `ReasoningAppended` |
| Tool | `ToolStarted`, `ToolCompleted`, `ToolFailed` |
| Permission | `PermissionPending`, `PermissionResolved` |
| Question | `QuestionPending`, `QuestionResolved` |
| Subagent | `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed` |
| File | `FileChanged` |
| Run | `RunStarted`, `RunProgress`, `RunArtifactCreated`, `RunCompleted`, `RunDenied` |
| Job | `JobUpserted`, `JobRemoved` |
| Tool Program | `ToolProgramSubmitted`, `ToolProgramTerminal`, `ToolProgramAdmitted`, `ToolProgramStarted`, `ToolProgramProgress`, `ToolProgramWaitingForCall`, `ToolProgramWaitingForJob`, `ToolProgramRetryBackoff` |
| Selection | `TokenUsageUpdated`, `ModelSelected`, `AgentSelected` |
| Meta | `Diagnostic`, `ResyncRequired`, `Unknown` |

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

### Remote Transport Isolation (M005)

`ProjectionConnectionState` is a transport-neutral transient owner
shared by Unix socket, `/core`, and `/tui` adapters. It bounds
subscriptions, artifact reads, diagnostics, and reconnect generations.

Raw event forwarders explicitly discard
`CoreEvent::ProjectionStreamEvent`. Projection-private envelopes
originate only from the receiver owned by the matching authenticated
connection.

### Atomic Control Delivery (M006)

Remote projection control responses are critical writer operations.
The adapter serializes the frame, enqueues on a bounded control
channel, and waits for a bounded writer receipt (500 ms). `Initializing
-> Live` completes only after that receipt.

### Fixtures and Independent Consumers

`projection::fixtures` provides golden snapshots and event scripts:
`idle_snapshot`, `active_turn_event_script`, `completed_snapshot`,
`permission_event_script`, `subagent_event_script`,
`file_change_event_script`, `job_event_script`,
`question_event_script`, `project_summary_fixture`.

Two independent consumers produce equivalent logical state:

1. In-crate reducer tests in `projection::reducer::tests`.
2. Root integration test `tests/session_projection_consumer.rs`
   re-implements a minimal `FakeTuiState` that consumes the same
   fixture scripts.

## Key Types & APIs

| Type | File:line | Purpose |
|------|-----------|---------|
| `ProjectionCapabilities` | `caps.rs:39` | Version negotiation |
| `SessionProjectionSnapshot` | `snapshot.rs:26` | Bounded session snapshot |
| `ProjectionReducer` | `reducer.rs:236` | Canonical pure reducer |
| `ReducerEventInput` | `reducer.rs:143` | Lightweight reducer input |
| `ApplyOutcome` | `reducer.rs:112` | Reducer application result |
| `ProjectionEvent` | `event.rs:127` | 39-variant event enum |
| `ProjectionEnvelope` | `event.rs:53` | Event envelope with metadata |
| `ProjectionStreamScope` | `event.rs:38` | Session/Project/Workspace/Daemon |
| `ProjectionClientController` | `controller.rs` | Frontend state machine |
| `ProjectionMode` | `controller.rs:82` | ProjectionPrimary/RawCompat/Unsupported |
| `ProjectionStreamId` | `replay.rs:41` | Opaque stream identifier |
| `ProjectionCursor` | `replay.rs:106` | Client cursor for resume |
| `ProjectionReplayBatch` | `replay.rs:161` | Replay event batch |
| `ToolProgramSummary` | `dto.rs:583` | Background tool program state |
| `ToolProgramDetail` | `dto.rs:708` | Full tool program inspection |
| `ToolProgramCallPage` | `dto.rs:679` | Paginated call history |
| `VisibilityClass` | `dto.rs:26` | Public/ClientLocal/Internal/Sensitive |

## Configuration Surface

No runtime configuration. All limits, versions, and caps are
compile-time constants. The projection protocol version is bumped
when additive changes land that the reducer MUST interpret.

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
   only from the matching authenticated connection's receiver.

6. **RenderFrame unsupported**: The `/tui` protocol does not support
   `RenderFrame`. It is event/state-driven via `TuiCommand`.

7. **Protocol version 5**: The `/tui` protocol version is 5.
   Projection-primary clients negotiate before using resume,
   acknowledgement, or unsubscribe.

## Testing

```bash
# Inline unit tests (caps, limits, dto, event, snapshot, reducer,
# adapters, fixtures, controller)
cargo test -p codegg-protocol

# Independent consumer equivalence test
cargo test --test session_projection_consumer

# Adversarial tests
cargo test --test context_projection_adversarial

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
