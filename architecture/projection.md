# Session Projection Contract

Status: implemented (Milestone 1 — projection contracts and canonical reducer; Milestone 2 — scoped subscriptions and durable replay strictly closed at the corrective daemon-integration commit).

Long-term references:

- `plans/000-long-term-specification.md#8-read-only-session-observation`
- `plans/000-long-term-specification.md#14-acp-integration`
- `plans/subsystems/session-projections-roadmap.md`

This document describes the frontend-neutral session projection
contract implemented by `codegg-protocol::projection`. It covers the
versioned bounded DTOs, the deterministic canonical reducer, the
adapter layer that bridges the existing core protocol, and the
explicit non-goals that bound Milestone 1.

## 1. Purpose and ownership boundary

The projection is a **derived** frontend contract. It is not a second
session execution authority and the reducer never performs I/O,
network, filesystem, or provider calls. The contract exists so local
TUI, remote TUI, observer clients, and ACP adapters all consume the
same logical session model rather than re-implementing event
interpretation per frontend.

Milestone 1 is intentionally bounded:

- One versioned, bounded projection schema (`SessionProjectionSnapshot`).
- One deterministic canonical reducer (`ProjectionReducer`).
- One adapter layer that maps the existing `CoreResponse` snapshot
  variants and `CoreEvent` families into the projection contract
  without replacing the wire shape.
- At least two independent consumers (the in-crate reducer tests and
  the root `session_projection_consumer` integration test) producing
  equivalent logical state from the same fixtures.

Out of scope for this milestone:

- Durable replay database, indexed events, or checkpoints.
- Subscription registry, acknowledgement cursors, or retention.
- Final visibility / authorization policy.
- Presence, observer UX, chat, ACP implementation, or web frontend.
- Exposing hidden chain-of-thought.
- Full durable agent-run tree semantics.

## 2. Module layout

```
crates/codegg-protocol/src/projection/
├── mod.rs        # Re-exports + module map
├── caps.rs       # ProjectionCapabilities, version negotiation
├── limits.rs     # MAX_PROJECTION_* constants, truncate_str, clip_str
├── dto.rs        # Bounded projection DTOs (SessionSummaryProjection, …)
├── event.rs      # ProjectionEnvelope, ProjectionEvent, ProjectionStreamScope
├── snapshot.rs   # SessionProjectionSnapshot, ProjectionDiagnostic
├── reducer.rs    # ProjectionReducer, ReducerEventInput, ReducerConfig
├── adapters.rs   # snapshot_from_snapshot_session, projection_events_from_core, …
└── fixtures.rs   # idle_snapshot, active_turn_event_script, completed_snapshot, …
```

The integration test `tests/session_projection_consumer.rs` exercises
the second independent consumer (a "fake TUI-style" state builder)
that produces equivalent logical state from the same fixtures.

## 3. Versioning and capability negotiation

Two constants define the contract surface:

- `PROJECTION_PROTOCOL_VERSION` (`u32 = 1`) — the current projection
  protocol version.
- `PROJECTION_PROTOCOL_VERSION_MIN` (`u32 = 1`) — the minimum version
  this build can interoperate with.

A capability declaration [`caps::ProjectionCapabilities`] carries a
`min_version..=max_version` range plus two booleans:
`supports_incremental_events` and `supports_unknown_fields`. The
negotiated version is the intersection of the two sides' ranges.
Versions outside the declared range produce a typed
`ReducerError::UnsupportedProtocolVersion` rather than silently
mutating state.

The capability identifier is `PROJECTION_CAPABILITY =
"session_projection.v1"`. Daemons and clients add it to their
existing `ClientCapabilities` / `ServerCapabilities` declarations;
this milestone does not yet wire it into the protocol frames — that
follows Milestone 2.

## 4. Bounded payload and collection limits

All projection DTOs honour explicit caps declared in
`projection::limits`:

| Constant | Bound |
|---|---|
| `MAX_PROJECTION_SESSIONS` | 16 |
| `MAX_PROJECTION_MESSAGES` | 256 per turn |
| `MAX_PROJECTION_RECENT_TOOLS` | 32 per turn |
| `MAX_PROJECTION_PENDING_PERMISSIONS` | 16 |
| `MAX_PROJECTION_PENDING_QUESTIONS` | 16 |
| `MAX_PROJECTION_RUNS` | 32 |
| `MAX_PROJECTION_ARTIFACTS` | 32 |
| `MAX_PROJECTION_JOBS` | 32 |
| `MAX_PROJECTION_SUBAGENTS` | 16 |
| `MAX_PROJECTION_DIAGNOSTICS` | 32 |
| `MAX_PROJECTION_DIFF_LINES` | 64 |
| `MAX_PROJECTION_STRING_BYTES` | 4,096 |
| `MAX_PROJECTION_TOOL_ARGS_BYTES` | 8,192 |
| `MAX_PROJECTION_TOOL_OUTPUT_BYTES` | 8,192 |
| `MAX_PROJECTION_RUN_SUMMARY_BYTES` | 2,048 |
| `MAX_PROJECTION_TRUNCATION_MARKER_BYTES` | 64 |

Strings that exceed their bound are truncated using
`limits::truncate_str`, which appends the `TRUNCATION_MARKER`
(`"\u{2026}[truncated]"`) and rounds the cut down to the nearest UTF-8
char boundary. Tool arguments and outputs that exceed their caps are
replaced with [`ToolArgumentProjection::TruncatedArguments`] /
[`ToolOutputProjection::TruncatedOutput`] variants carrying the
original byte length and a bounded preview. Anything that exceeds
`MAX_PROJECTION_ARTIFACTS` is replaced by an
[`ArtifactHandleProjection`] rather than being embedded inline.

## 5. Visibility classification

Every payload field carries a [`dto::VisibilityClass`] tag:

- `Public` — visible to any frontend client (default).
- `ClientLocal` — visible to the active client only. Reserved for
  subagent task ids and diagnostics that may reveal internal
  sequencing.
- `Internal` — internal; never serialised into a projection event.
- `Sensitive` — must be redacted before leaving the daemon. The
  reducer currently leaves the field as-is and records a diagnostic;
  the full policy lands in Milestone 3.

This milestone ships the typed seam; runtime filtering is the
responsibility of Milestone 3.

## 6. Canonical reducer

`projection::reducer::ProjectionReducer` is pure and deterministic.
It accepts a bounded [`snapshot::SessionProjectionSnapshot`] and an
ordered stream of [`reducer::ReducerEventInput`] (which the
`ProjectionEnvelope` shape can be converted into via `From`). Each
input yields one [`reducer::ApplyOutcome`]:

| Outcome | Meaning |
|---|---|
| `Applied` | Snapshot updated. |
| `Duplicate` | `event_seq` was at or below the snapshot's `event_seq`. |
| `ScopeMismatch` | `session_id` does not match. |
| `Reconciled` | Impossible / out-of-order transition; diagnostic recorded. |
| `ResyncRequired { from_event_seq, current_seq }` | Reducer requested full resync. |
| `Error(ReducerError)` | `UnsupportedProtocolVersion` or `SequenceRegression` with replay disabled. |

Lifecycle invariants enforced by the reducer:

- Two compliant reducers given the same `(snapshot, events)` produce
  equivalent serialized snapshots.
- Events whose `session_id` does not match the snapshot's primary
  session id (and the snapshot is not a multi-session scope) are
  ignored with a `scope_mismatch` diagnostic.
- Out-of-order or impossible transitions do not panic; they append a
  diagnostic and return `Reconciled`.
- The reducer never performs I/O, network, filesystem, or provider
  calls.
- Concurrent readers may share immutable snapshot clones; one
  controlled writer/reducer applies ordered events per projection
  instance.

`ProjectionState` is the public extension trait that exposes the
snapshot helpers (`upsert_secondary`, `push_recent_turn`) used by
the reducer and the adapters. External reducer implementations
**MUST** go through these helpers so the bound invariants stay in
sync with the snapshot type.

## 7. Adapter layer

The adapter layer in `projection::adapters` bridges the existing
`CoreResponse` snapshot variants and `CoreEvent` families:

- `snapshot_from_snapshot_session` builds a projection snapshot
  from `CoreResponse::SnapshotSession`.
- `snapshot_from_daemon` and `snapshot_from_session_snapshot` build
  projections from the lightweight `SessionSnapshot` carried in
  `CoreResponse::SnapshotDaemon`.
- `project_summary_from_dto` builds a projection project summary
  from `ProjectSummaryDto`.
- `projection_events_from_core` and `projection_envelopes_from_core`
  convert any `CoreEvent` envelope into one or more projection
  events / envelopes.

The adapters **never** replace existing core events. They add the
projection surface as an additive layer: clients that do not advertise
the projection capability simply ignore the new variants.

Mappings cover the following families:

- turn lifecycle (`TurnStarted` / `TurnTextDelta` /
  `TurnReasoningDelta` / `TurnCompleted` / `TurnFailed`),
- tool lifecycle (`ToolStarted` / `ToolCompleted`),
- permission / question lifecycle (`PermissionPending` /
  `QuestionPending`),
- subagent lifecycle (`SubagentStarted` / `SubagentProgress` /
  `SubagentCompleted` / `SubagentFailed`),
- file changes (`FileChanged`),
- run lifecycle (`RunStarted` / `RunProgress` /
  `RunArtifactCreated` / `RunCompleted` / `RunDenied`),
- job lifecycle (`JobCreated` / `JobStarted` / `JobCompleted` /
  `JobFailed`),
- everything else maps to `ProjectionEvent::Unknown { variant_name,
  notice }` and produces a single bounded diagnostic.

Tool output larger than `MAX_PROJECTION_TOOL_OUTPUT_BYTES` becomes a
`TruncatedOutput` rather than being embedded inline.

## 8. Fixtures and independent consumers

The fixture module (`projection::fixtures`) provides:

- `idle_snapshot`, `active_turn_snapshot`, `permission_pending_snapshot`,
  `completed_snapshot`, `project_summary_fixture`,
- event scripts: `active_turn_event_script`, `permission_event_script`,
  `completed_event_script`, `subagent_event_script`,
  `file_change_event_script`, `job_event_script`, `question_event_script`.

Two independent consumers produce equivalent logical state:

1. The in-crate reducer tests in `projection::reducer::tests` and
   `projection::fixtures::tests` apply the same event scripts through
   the canonical `ProjectionReducer` and assert the expected
   logical state.
2. The root integration test `tests/session_projection_consumer.rs`
   re-implements a minimal "fake TUI-style" state consumer
   (`FakeTuiState`) that consumes the exact same fixture scripts and
   compares its own logical state to the canonical snapshot's
   logical state. The test asserts that both consumers agree on
   active turn id, status, message count, tool count, pending
   permission count, recent turn count, and active subagent count.

## 9. Static guarantees

The projection module is enforced to remain UI / server / plugin
free by `scripts/check-core-boundary.sh`. The contract depends only
on `serde` / `serde_json`; it has no `axum`, `ratatui`, `crossterm`,
`wasmtime`, or filesystem dependency.

The reducer is dependency-free: it does not pull in the daemon, the
TUI, or any storage layer. Frontends wire to it via the projection
types only.

## 10. Compatibility matrix

| Component | Compatibility |
|---|---|
| `codegg::protocol::projection` consumers | Forward and backward compatible via `ProjectionCapabilities`; unknown optional variants tolerated. |
| Existing `CoreResponse` / `CoreEvent` consumers | Unchanged; projection events are additive. |
| `TuiMessage` / remote TUI | Unchanged this milestone; durable migration is Milestone 2. |
| Web / observer / ACP adapters | Not yet present; deferred to Milestones 2–4. |

## 11. Security classification

- No credential, secret, environment-variable, or raw provider
  config field is ever embedded in a projection DTO. The adapter
  layer asserts this contract statically by not importing the
  `SecretInput` / `SecretInputRef` types from `codegg-protocol`.
- Tool outputs that exceed `MAX_PROJECTION_TOOL_OUTPUT_BYTES`
  become `TruncatedOutput { original_bytes, preview }` rather than
  being embedded.
- File bodies, render frames, and chat content stay in existing
  stores / tools. The projection never embeds `RawRenderFrame` or
  chat block content.
- Provider private hidden reasoning is **not** mapped by the
  adapter. The `TurnReasoningDelta` event is folded into a
  `VisibilityClass::Internal` message in the projection; consumers
  may choose to render it or hide it.
- Full redaction policy lands in Milestone 3.

## 12. Verification commands

```bash
# Inline unit tests for the projection module (caps, limits, dto,
# event, snapshot, reducer, adapters, fixtures).
cargo test -p codegg-protocol

# Independent consumer equivalence test (root integration test).
cargo test --test session_projection_consumer

# Lint.
cargo clippy -p codegg-protocol --all-targets --all-features -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Format.
cargo fmt -- --check
```

## 13. Future milestone ownership

- **Milestone 2** owns durable replay, subscription registry,
  acknowledgement cursors, and retention. It consumes
  `ProjectionCapabilities` and the reducer deterministically;
  adding replay MUST NOT require changes to the projection DTOs.
- **Milestone 3** owns the visibility / redaction pipeline, the
  full authorization seam, and bounded artifact read APIs. It
  consumes the `VisibilityClass` tag introduced here.
- **Milestone 4** owns frontend adoption (local / remote TUI and
  ACP), migration of `TuiMessage::StateSnapshot` to the new
  contract, and reference second-frontend compatibility tests.

## 14. M2 daemon integration summary

The corrective daemon-integration commit lands the full production
plumbing for the M2 replay subsystem while preserving M1 invariants:

- `EventLog` carries one optional `ProjectionSink` hook. The
  default `CoreDaemon` installs `SeamProjectionSink` so every
  production `event_log.publish(...)` call site reaches the
  durable `projection_event` store exactly once. Legacy raw
  `CoreEvent` clients continue to receive the additive-compatible
  broadcast on the unfiltered channel.
- `ProjectionPublicationSeam` resolves canonical
  `(ProjectId, WorkspaceId, binding_revision)` from
  `ProjectStorage::session_binding(session_id)` before publication.
  Unbound, ambiguous, archived, or unresolved bindings fail closed
  with `PublishOutcome::Skipped { UnboundSession }` before any
  sequence allocation.
- `ProjectionReplayStore::get_or_create_session_stream_with_revision`
  invalidates the old stream when the canonical binding revision
  advances. The new active stream carries the new revision; the old
  row is marked `lifecycle = 'rebound'`.
- Real `ProjectionStreamId` values minted by the store are used
  for queue delivery, cursor validation, replay, and acknowledgement.
  The previous synthetic `"session-stream"` / `"project-stream"`
  placeholders are gone.
- `CoreRequest::Projection*` requests dispatch through
  `CoreDaemon::handle_request`. Each variant maps to a bounded
  `CoreResponse::Projection*`. Capability, version, scope, and
  client ownership are enforced.
- `daemon_socket` opens one bounded receiver per
  `ProjectionSubscribe` and spawns a per-connection forwarder that
  writes `CoreEvent::ProjectionStreamEvent { subscription_id, ... }`
  to the owning client. Disconnect / unsubscribe / lag /
  generation change drop the receiver and abort the forwarder.
  Two clients with different subscriptions never observe each
  other's projection events.
- The replay store, service, and seam are constructed from the
  daemon's SQLite pool during `CoreDaemon::with_deps_and_identity`.
  A background task runs `ProjectionReplayService::maintenance_tick`
  on a 300-second interval to drive retention pruning and
  checkpoint writing.
- `scripts/check_projection_publication_seam.sh` is a new static
  guard that fails CI if any non-test production source calls
  `ProjectionReplayHandle::publish_core_event` outside the
  centralized `EventLog` sink.