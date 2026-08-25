# Protocol Module

## Purpose

The protocol crate (`crates/codegg-protocol/`) defines the wire-format
envelopes, request/response enums, and event types used for all TUI <-> Core
communication. It is the single source of truth for the serialization format
between the daemon, embedded TUI, remote TUI, CLI/automation, and future
GUI/web clients.

**Re-export**: `codegg::protocol` via `pub use codegg_protocol as protocol`
in `src/lib.rs`.

## Where It Lives

```
crates/codegg-protocol/src/
├── lib.rs              # Module exports
├── core.rs             # CoreRequest, CoreResponse, CoreEvent, envelopes
├── dto.rs              # Shared DTOs (Session, Message, etc.)
├── provider.rs         # Secret-safe provider connection/provisioning DTOs
├── frames.rs           # ClientCapabilities, RequestEnvelope, EventEnvelope
├── plugin.rs           # PluginManifestDto, PluginInvocation, PluginResponse
├── projection/         # Frontend-neutral session projection contract
├── tui.rs              # TuiMessage, QuestionSpec, RemoteTuiStateSnapshot
└── ui.rs               # UiNode, UiEffect, UiEffectEnvelope, UiLimits
```

Domain identity types live in `codegg-core::identity`, not in this wire
crate. Protocol DTOs keep string-backed `project_id`, `workspace_id`, and
`directory` fields for wire compatibility.

## How It Works

### Serialization

All request/response enums use `#[serde(tag = "type", rename_all = "snake_case")]`
for JSON discrimination. `TuiMessage` uses `#[serde(tag = "type")]` without
`rename_all`.

### Versioned Envelopes

Every request carries a `RequestEnvelope<T>` with `protocol_version`. Every
event carries an `EventEnvelope<T>` with `protocol_version`, `event_seq`,
`timestamp_ms`, and optional `session_id`/`turn_id` for ordered delivery.

### Transport Flows

**In-process (InprocCoreClient)**: Direct function calls.
**Stdio/Socket**: JSONL over stdin/stdout or Unix socket.
**Remote TUI (Server)**: WebSocket / HTTP through Axum, with `TuiMessage`
events pushed to subscribed clients.

## Protocol Versions

| Constant | Value | Location |
|----------|-------|----------|
| `PROTOCOL_VERSION` | 2 | `core.rs:26` |
| `REMOTE_TUI_PROTOCOL_VERSION` | 5 | `tui.rs:14` |
| `PLUGIN_PROTOCOL_VERSION` | 1 | `plugin.rs` |
| `PROJECTION_PROTOCOL_VERSION` | 1 | `projection/caps.rs` |

Version history:
- `PROTOCOL_VERSION`: 1 -> 2 (Phase 15, typed `UiEffectEnvelope`)
- `REMOTE_TUI_PROTOCOL_VERSION`: 2 -> 3 (Phase 15, typed plugin UI),
  3 -> 4 (projection surface), 4 -> 5 (connection-owned projection
  resume, lifecycle, artifact, compatibility diagnostics)
- `PLUGIN_PROTOCOL_VERSION`: stable at 1
- `PROJECTION_PROTOCOL_VERSION`: 1 (Session Projections M1)

## Key Types & APIs

### RequestEnvelope (`core.rs:30`)

```rust
pub struct RequestEnvelope<T> {
    pub protocol_version: u32,
    pub request_id: String,
    pub payload: T,
}
```

### EventEnvelope (`core.rs:125`)

```rust
pub struct EventEnvelope<T> {
    pub protocol_version: u32,
    pub event_seq: u64,
    pub timestamp_ms: i64,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub payload: T,
}
```

### CoreRequest (`core.rs:524`)

Tagged enum with ~100 variants. Major groups:

**Asset Refresh (3)**: `AssetRefresh`, `AssetRefreshStatus`,
`AssetRefreshCapabilities`

**Connection Lifecycle (~20)**: `EggpoolConnectionCreate`,
`EggpoolConnectionCancel`, `EggpoolConnectionStatus`,
`ProviderConnectionList`, `ProviderConnectionModels`,
`ConnectionRotateBegin`, `ConnectionRotateSecretStage`,
`ConnectionRotateCancel`, `ConnectionRotateStatus`,
`ConnectionRefreshBegin`, `ConnectionRefreshCancel`,
`ConnectionRefreshStatus`, `ConnectionGet`, `ConnectionListDetail`,
`ConnectionEnable`, `ConnectionDisable`, `ConnectionDelete`,
`ConnectionRestore`, `ConnectionPurge`

**Session Lifecycle (19)**: `Initialize`, `Subscribe`, `Resume`,
`SessionList`, `SessionCreate` (with optional `project_id`/`workspace_id`
for identity-aware clients), `SessionAttach`, `SessionLoad`,
`SessionMessagesLoad`, `SessionMessageCounts`, `SessionFork`,
`SessionDelete`, `SessionArchive`, `SessionRestore`, `SessionShare`,
`SessionUnshare`, `SessionRename`, `SessionExport`, `SessionImportData`,
`SessionCreateFromTemplate`

**Session Selection (4)**: `SessionSelectionGet`, `SessionSelectionList`,
`SessionSelectionUpdate`, `SessionSelectionModels`

**Session Lifecycle (2)**: `SessionLifecycleGet`

**Turn (5)**: `TurnSubmit`, `TurnCancel`, `TurnSteer`, `AgentSelect`,
`ModelSelect`

**Model (1)**: `ModelsRefresh`

**Permission/Question (2)**: `PermissionRespond`, `QuestionRespond`

**Memory (4)**: `MemorySearch`, `MemoryList`, `MemoryRemember`,
`MemoryForget`

**Task (3)**: `TaskList`, `TaskSchedule`, `TaskDelete`

**Worktree (1)**: `WorktreeList`

**Workspace (5)**: `WorkspaceRegister`, `WorkspaceList`,
`WorkspaceArchive`, `WorkspaceSnapshotRequest`,
`WorkspaceServicesSnapshot`, `WorkspaceConfigReload`

**Project Catalog (7)**: `ProjectList`, `ProjectGet`, `ProjectRegister`,
`ProjectArchive`, `ProjectRestore`, `ProjectHealth`,
`ProjectCatalogCapabilities`

**Run (3)**: `RunList`, `RunGet`, `RunArtifactRead`

**Goal (9)**: `GoalSet`, `GoalFromFile`, `GoalShow`, `GoalPause`,
`GoalResume`, `GoalClear`, `GoalDone`, `GoalCheckpoint`,
`GoalSetBudget`

**Todo (1)**: `TodoList`

**Snapshot (4)**: `SnapshotSession`, `SnapshotWorkspace`, `SnapshotModels`,
`SnapshotDaemon`

**Notification (2)**: `NotificationSpeak`, `NotificationStop`

**Durable Jobs (10)**: `JobSubmit`, `JobWait`, `JobGet`, `JobList`,
`JobCancel`, `JobRetry`, `JobAttempts`, `SchedulerSnapshot`,
`JobRecoveryReport`

**Schedules (6)**: `ScheduleCreate`, `ScheduleList`, `ScheduleGet`,
`SchedulePause`, `ScheduleResume`, `ScheduleDelete`

**Projections (7)**: `ProjectionCapabilities`, `ProjectionSubscribe`,
`ProjectionResume`, `ProjectionAck`, `ProjectionUnsubscribe`,
`ProjectionSnapshotGet`, `ProjectionArtifactRead`, `ProjectionArtifactList`

**Tool Programs (5)**: `ToolProgramList`, `ToolProgramInspect`,
`ToolProgramCallPage`, `ToolProgramNotificationReinject`,
`ToolProgramRecoveryDebugInspect`

### CoreResponse (`core.rs:137`)

Tagged enum with ~60 variants. Major groups:

**Connection Responses**: `EggpoolConnectionCreated`,
`EggpoolConnectionStatus`, `EggpoolConnectionCancelled`,
`ProviderConnections`, `ProviderConnectionModels`, `ConnectionDetail`,
`ConnectionDetails`, `ConnectionRotateStatus`,
`ConnectionRotateSecretStaged`, `ConnectionRefreshStatus`,
`ConnectionRefreshResult`, `ConnectionPurge`

**Session Responses**: `Ack`, `Json`, `Session`, `SessionMessages`,
`SessionMessageCounts`, `SessionList`, `SnapshotSession`, `SnapshotDaemon`,
`SchedulerSnapshot`, `ModelsSnapshot`, `Events`, `ResyncRequired`,
`Error`

**Workspace Responses**: `WorkspaceList`, `WorkspaceSnapshot`,
`WorkspaceServicesSnapshot`, `WorkspaceConfigReload`

**Project Responses**: `ProjectList`, `ProjectGet`, `ProjectRegistered`,
`ProjectArchived`, `ProjectRestored`, `ProjectHealth`,
`ProjectCatalogCapabilities`

**Run Responses**: `RunList`, `RunGet`, `RunArtifactChunk`

**Job Responses**: `JobGet`, `JobList`, `JobAttempts`, `JobCancelResult`,
`JobSubmitted`, `JobWaited`, `JobRetryStarted`

**Schedule Responses**: `ScheduleCreated`, `ScheduleList`, `ScheduleGet`,
`SchedulePaused`, `ScheduleResumed`, `ScheduleDeleted`

**Projection Responses**: `ProjectionCapabilitiesResponse`,
`ProjectionSubscribed`, `ProjectionReplay`, `ProjectionResyncRequired`,
`ProjectionAckAccepted`, `ProjectionUnsubscribed`,
`ProjectionSubscriptionStatusResponse`, `ProjectionArtifactRead`,
`ProjectionArtifactList`

**Tool Program Responses**: `ToolProgramList`, `ToolProgramInspect`,
`ToolProgramCallPage`, `ToolProgramNotificationReinjectReport`,
`ToolProgramRecoveryDebugInspectReport`

**Lifecycle Responses**: `SessionLifecycle`, `SessionSelection`,
`SessionSelectionUpdated`, `AssetRefresh`, `AssetRefreshStatus`,
`AssetRefreshCapabilities`, `JobRecoveryReport`

### CoreEvent (`core.rs:1058`)

Tagged enum with ~40 variants. Major groups:

**Snapshot (5)**: `SnapshotSession`, `SnapshotWorkspace`, `SnapshotModels`,
`AssetRefreshCompleted`, `ConnectionRotated`, `ConnectionStateChanged`

**Project (4)**: `ProjectRegistered`, `ProjectArchived`,
`ProjectRestored`, `ProjectHealthChanged`

**Turn (5)**: `TurnStarted`, `TurnTextDelta`, `TurnReasoningDelta`,
`TurnCompleted`, `TurnFailed`

**Tool (2)**: `ToolStarted`, `ToolCompleted`

**Permission/Question (2)**: `PermissionPending`, `QuestionPending`

**Session (2)**: `SessionUpdated`, `FileChanged`

**Subagent (4)**: `SubagentStarted`, `SubagentProgress`,
`SubagentCompleted`, `SubagentFailed`

**Test Run (3)**: `TestRunStarted`, `TestRunProgress`, `TestRunCompleted`

**Run (7)**: `RunStarted`, `RunProgress`, `RunArtifactCreated`,
`RunProjectionReady`, `RunCompleted`, `RunDenied`, `RunPinned`,
`ContextPromotionChanged`, `RunRerunLinked`

**Plugin UI (1)**: `PluginUiEffect` (carries `UiEffectEnvelope`)

**Job (10+)**: `JobCreated`, `JobQueued`, `JobBlocked`,
`JobAttemptCreated`, `JobStarted`, `JobProgress`, `JobCancelRequested`,
`JobCompleted`, `JobFailed`, `JobCancelled`, `JobTimedOut`,
`JobInterrupted`, `JobRecovered`, `JobScheduled`, `JobDependencyResolved`

### TuiMessage (`tui.rs:19`)

Tagged enum with ~39 variants. Major groups:

**Client-to-Server (3)**: `Input`, `KeyDown`, `MouseClick`

**Connection (3)**: `Resize`, `Resume`, `RequestSnapshot`

**Response (2)**: `PermissionResponse`, `QuestionResponse`

**Server-to-Client (10)**: `RenderFrame` (unsupported — returns error),
`TextDelta`, `PermissionPending`, `QuestionPending`, `SessionInfo`,
`SessionEnded`, `ToolCallStarted`, `ToolResult`, `PluginUiEffect`,
`Error`, `StateSnapshot`

**Projection (17)**: `ProjectionCapabilities`,
`ProjectionCapabilitiesAck`, `ProjectionSubscribe`,
`ProjectionSnapshot`, `ProjectionReplay`, `ProjectionResync`,
`ProjectionAck`, `ProjectionAckResult`, `ProjectionEvent`,
`ProjectionResume`, `ProjectionUnsubscribe`,
`ProjectionUnsubscribeResult`, `ProjectionSubscriptionStatus`,
`ProjectionSubscriptionStatusResult`,
`ProjectionArtifactListRequest`, `ProjectionArtifactListResult`,
`ProjectionArtifactReadRequest`, `ProjectionArtifactReadResult`,
`ProjectionCompatibilityDiagnostic`

**Special (1)**: `ResyncRequired`

### ClientHello / ServerHello (`frames.rs`)

`ClientHello` carries `client_name`, `ClientKind` (Tui/Gui/Web/Cli/Automation),
`protocol_version`, and `ClientCapabilities`. `ServerHello` carries
`daemon_id`, `protocol_version`, `ServerCapabilities`, and `client_id`.

`ClientCapabilities` includes `visual_notifications`, `desktop_notifications`,
`audio`, `tts`, `multi_session_view`, and 7 `plugin_ui_*` capability flags.

`ServerCapabilities` includes `event_replay`, `session_management`,
`permission_routing`, `workspace_registration`, `workspace_snapshots`.

### UiEffectEnvelope (`ui.rs`)

```rust
pub struct UiEffectEnvelope {
    pub session_id: Option<String>,
    pub source: UiEffectSource,
    pub invocation_id: Option<String>,
    pub effect: UiEffect,
}
pub enum UiEffectSource {
    Plugin { plugin_id: String },
    Core,
    Tui,
}
```

### UiLimits (`ui.rs`)

Bounded resource caps: `max_effects_per_response`, `max_effect_bytes`,
`max_node_depth`, `max_table_rows`, `max_table_columns`, `max_string_len`,
`max_panels_per_plugin`, `max_status_items_per_plugin`,
`max_open_dialogs_global`, `max_snapshot_body_bytes`.

Presets: `UiLimits::balanced()` (TUI), `UiLimits::text_only()` (CLI).

Validation: `validate_ui_effect()`, `validate_ui_effects()`,
`validate_ui_node()` — reject or truncate with diagnostics, never panic.

Degradation: `degrade_effect()`, `degrade_node_to_text()`,
`effect_summary()`.

### Projection Contract (`projection/`)

Milestone 1 defines the frontend-neutral, versioned session projection
contract. Key types: `ProjectionCapabilities`, `ProjectionEvent`,
`ProjectionEnvelope`, `SessionProjectionSnapshot`, `ProjectionReducer`.

The reducer is pure, deterministic, and never performs I/O. It deduplicates
by `event_seq` and records diagnostics for impossible transitions rather
than panicking.

## Session Projection Contract (M1)

The `projection/` submodule defines:
- `ProjectionCapabilities` and `PROJECTION_PROTOCOL_VERSION = 1`
- Bounded payload and collection limits (`limits.rs`)
- Bounded summaries for sessions, turns, messages, tools, runs, jobs,
  permissions, questions, artifacts, agent-tree (`dto.rs`)
- `ProjectionEvent` variants and `ProjectionEnvelope` (`event.rs`)
- `SessionProjectionSnapshot` and `ProjectionDiagnostic` (`snapshot.rs`)
- Deterministic `ProjectionReducer` with `ReducerEventInput` and
  `ReducerConfig` (`reducer.rs`)
- Adapters from `CoreResponse`/`CoreEvent` (`adapters.rs`)
- Golden fixtures (`fixtures.rs`)

## Identity-Aware Additive Protocol

Protocol version 2 remains wire-compatible. `ProjectContextDto` and
`SessionBindingDto` expose stable project/workspace identities while
legacy directory fields remain compatibility data. `SessionCreate` and
`SessionCreateFromTemplate` accept optional canonical IDs; omitted IDs
resolve through deterministic lookup of an existing unique locator.

## Implementation Notes

- Subagent events in `CoreEvent` carry `task_id: u64` (not String)
- `ToolStarted` and `ToolCompleted` have `turn_id: Option<String>`
- `PermissionPending` and `QuestionPending` include `session_id` and
  optional `turn_id` for proper routing
- `RemoteTuiStateSnapshot` includes `git: Option<RemoteGitInfo>`,
  `plugin_panels: Vec<RemotePanelView>`,
  `plugin_status_items: Vec<RemoteStatusItemView>`
- `PluginUiEffect` in both `CoreEvent` and `TuiMessage` carries a typed
  `UiEffectEnvelope` (Phase 15)

## Invariants & Gotchas

- **RenderFrame is unsupported**: `TuiMessage::RenderFrame` returns an
  error with code `unsupported_render_frame`. Use `StateSnapshot` instead.
- **Event ordering**: Events are ordered by monotonic `event_seq`.
  No separate sequence system is used.
- **Session-scoped effects**: Plugin effects with `session_id` are
  filtered to subscribed clients.
- **Durable surfaces**: Panels and status items survive reconnect.
  Dialogs and toasts are transient.
- **Secret-bearing requests rejected by remote WebSocket**:
  `EggpoolConnectionCreate` and `ConnectionRotateSecretStage` are
  local-only. The remote core WebSocket rejects them.
- **Projection is additive**: Unknown optional variants are tolerated
  within the declared version range.

## Testing

```bash
cargo test -p codegg-protocol              # all protocol tests
cargo test --test session_projection_consumer  # projection consumer
```

## Related Docs

- `architecture/projection.md` — full projection contract
- `architecture/core.md` — core facade and transport adapters
- `architecture/server.md` — HTTP/WebSocket server
- `architecture/plugin.md` — plugin system
