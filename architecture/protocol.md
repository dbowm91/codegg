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
├── document.rs         # document.v1 DTOs and bounds (MAX_DOCUMENT_*)
├── dto.rs              # Shared DTOs (Session, Message, etc.)
├── frames.rs           # ClientHello/ServerHello, ClientCapabilities,
│                      #   ServerCapabilities
├── interactive_process.rs # InteractiveProcess attach/stream DTOs
├── human_shell.rs        # human_shell.v1 bounded daemon run DTOs
├── lsp.rs              # LSP preview-apply, read-surface, and completion DTOs
├── plugin.rs           # PluginManifestDto, PluginInvocation, PluginResponse
├── projection/         # Frontend-neutral session projection contract
├── provider.rs         # Secret-safe provider connection/provisioning DTOs
├── runtime_assets.rs   # Inert bounded runtime-asset manifest DTOs
├── scratch.rs          # Managed scratch census/reclaim wire DTOs
├── tui.rs              # TuiMessage, QuestionSpec, RemoteTuiStateSnapshot
├── ui.rs               # UiNode, UiEffect, UiEffectEnvelope, UiLimits
└── work_order.rs       # Durable project work order wire DTOs
```

`lib.rs` declares 15 public modules; the `projection/` directory holds 12
submodules (`adapters`, `caps`, `consumer`, `controller`, `dto`, `event`,
`fixtures`, `limits`, `reducer`, `replay`, `snapshot`, `workstream`).

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
**Stdio/local daemon**: JSONL over stdin/stdout or a platform-local byte
stream (Unix domain socket on Unix, current-user named pipe on Windows).
**Remote TUI (Server)**: WebSocket / HTTP through Axum, with `TuiMessage`
events pushed to subscribed clients.

### Human finite shell runs (`human_shell.v1`)

`HumanShellRunCapabilities`, `HumanShellRunCreate`, `HumanShellRunRead`, and
`HumanShellRunCancel` negotiate and operate bounded one-shot runs. Create
requests carry command text, the selected 1–3600 second timeout
(`HUMAN_SHELL_MAX_TIMEOUT_SECS = 60 * 60`), and project, workspace, and
optional session locators. The daemon resolves authority and canonical cwd;
request DTOs do not carry caller cwd, shell path, environment, principal, or
execution-node claims. Command text is capped at 16 KiB
(`HUMAN_SHELL_MAX_COMMAND_BYTES`). Output reads are sequence-based and capped
at 64 KiB (`HUMAN_SHELL_MAX_READ_BYTES`). The daemon retains run
status/output ephemerally for ten minutes (`RUN_TTL_MS` in
`src/shell/daemon.rs`), and daemon restart invalidates handles without
replay. The wire surface carries its own
`HUMAN_SHELL_PROTOCOL_VERSION = 1` (`human_shell.rs:5`).

## Protocol Versions

| Constant | Value | Location |
|----------|-------|----------|
| `PROTOCOL_VERSION` | 2 | `core.rs:29` |
| `REMOTE_TUI_PROTOCOL_VERSION` | 5 | `tui.rs:14` |
| `PLUGIN_PROTOCOL_VERSION` | 1 | `plugin.rs:7` |
| `PROJECTION_PROTOCOL_VERSION` | 1 | `projection/caps.rs:27` |
| `HUMAN_SHELL_PROTOCOL_VERSION` | 1 | `human_shell.rs:5` |
| `SCRATCH_PROTOCOL_VERSION` | 2 | `scratch.rs:32` |

Version history:
- `PROTOCOL_VERSION`: 1 -> 2 (Phase 15, typed `UiEffectEnvelope`)
- `REMOTE_TUI_PROTOCOL_VERSION`: 2 -> 3 (Phase 15, typed plugin UI),
  3 -> 4 (projection surface), 4 -> 5 (connection-owned projection
  resume, lifecycle, artifact, compatibility diagnostics)
- `PLUGIN_PROTOCOL_VERSION`: stable at 1
- `PROJECTION_PROTOCOL_VERSION`: 1 (Session Projections M1)
- `HUMAN_SHELL_PROTOCOL_VERSION`: 1
- `SCRATCH_PROTOCOL_VERSION`: 2 (version 2 made the per-tree reclaim verdict
  tri-state — `reclaimable` is `Option<bool>`, where `None` means the verdict
  could not be established)

## Key Types & APIs

### RequestEnvelope (`core.rs:77`)

```rust
pub struct RequestEnvelope<T> {
    pub protocol_version: u32,
    pub request_id: String,
    pub payload: T,
}
```

### EventEnvelope (`core.rs:1188`)

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

### CoreRequest (`core.rs:2315`)

Tagged enum with 266 variants (240 struct-bodied + 26 unit). Major groups:

**Asset Refresh (3)**: `AssetRefresh`, `AssetRefreshStatus`,
`AssetRefreshCapabilities`

**Project Initialization (2)**: `ProjectInitDraftGet` and
`ProjectInitPublish`, a bounded preview and one-use-token publication of the
selected project's root `AGENTS.md`.

**Connection Lifecycle (21)**: `EggpoolConnectionCreate` (temporary
compatibility adapter), `ProviderConnectionCreate` (canonical
provider-neutral create), `ProviderSetupList` (secret-free setup catalog),
`EggpoolConnectionCancel`, `EggpoolConnectionStatus`,
`ProviderConnectionList`, `ProviderConnectionModels`,
`ConnectionRotateBegin`, `ConnectionRotateSecretStage`,
`ConnectionRotateCancel`, `ConnectionRotateStatus`,
`ConnectionRefreshBegin`, `ConnectionRefreshCancel`,
`ConnectionRefreshStatus`, `ConnectionGet`, `ConnectionListDetail`,
`ConnectionEnable`, `ConnectionDisable`, `ConnectionDelete`,
`ConnectionRestore`, `ConnectionPurge`

**Session Lifecycle (20)**: `Initialize`, `Subscribe`, `Resume`,
`SessionList`, `SessionCreate` (with optional `project_id`/`workspace_id`
for identity-aware clients), `SessionAttach`, `SessionLoad`,
`SessionMessagesLoad`, `SessionMessageCounts`, `SessionFork`,
`SessionLaunch`, `SessionDelete`, `SessionArchive`, `SessionRestore`,
`SessionShare`, `SessionUnshare`, `SessionRename`, `SessionExport`,
`SessionImportData`, `SessionCreateFromTemplate`

**Session Selection (4)**: `SessionSelectionGet`, `SessionSelectionList`,
`SessionSelectionUpdate`, `SessionSelectionModels`

**Session Lifecycle Query (1)**: `SessionLifecycleGet`

**Turn (6)**: `TurnSubmit`, `TurnCancel`, `TurnSteer`, `AgentSelect`,
`ModelSelect` (M004 compatibility adapter over durable
`SessionSelectionUpdate`; runtime-only mutation is not authoritative),
`SessionPromptSubmit` (M004 desktop prompt path: Turns family,
ViaSession+AgentInvoke, resolves durable model/agents/messages
daemon-side then delegates to the identical `TurnSubmit` body, so the
desktop never composes turns itself)

**Model (1)**: `ModelsRefresh`

**Permission/Question (2)**: `PermissionRespond`, `QuestionRespond`
(M004 gates both on the owning session/turn controller lease in
addition to existing authority; see `architecture/authorization.md`)

**Session Control (5)**: `SessionControlGet`, `SessionControlRequest`,
`SessionControlTransfer`, `SessionControlRelease`,
`SessionControlTakeover` (`session_control.v1`, ADR-0007; responses
`SessionControl` / `SessionControlUpdated`, events
`SessionControlChanged` / `SessionControlRequested` — ids, revisions,
and bounded reasons only, never secrets)

**Memory (4)**: `MemorySearch`, `MemoryList`, `MemoryRemember`,
`MemoryForget`

**Task (3)**: `TaskList`, `TaskSchedule`, `TaskDelete`

**Worktree (1)**: `WorktreeList`

**Workspace (6)**: `WorkspaceRegister`, `WorkspaceList`,
`WorkspaceArchive`, `WorkspaceSnapshotRequest`,
`WorkspaceServicesSnapshot`, `WorkspaceConfigReload`

**Project Catalog (10)**: `ProjectList`, `ProjectGet`, `ProjectRegister`,
`ProjectArchive`, `ProjectRestore`, `ProjectDelete`, `ProjectUpdate`,
`ProjectHealth`, `ProjectCatalogCapabilities`, `WorkspaceDashboard`

**Run (4)**: `RunList`, `RunGet`, `RunArtifactRead`, `RunRerun`

**Goal (9)**: `GoalSet`, `GoalFromFile`, `GoalShow`, `GoalPause`,
`GoalResume`, `GoalClear`, `GoalDone`, `GoalCheckpoint`,
`GoalSetBudget`

**Todo (1)**: `TodoList`

**Snapshot (4)**: `SnapshotSession`, `SnapshotWorkspace`, `SnapshotModels`,
`SnapshotDaemon`

**Notification (2)**: `NotificationSpeak`, `NotificationStop`

**Durable Jobs (9)**: `JobSubmit`, `JobWait`, `JobGet`, `JobList`,
`JobCancel`, `JobRetry`, `JobAttempts`, `SchedulerSnapshot`,
`JobRecoveryReport`

**Schedules (6)**: `ScheduleCreate`, `ScheduleList`, `ScheduleGet`,
`SchedulePause`, `ScheduleResume`, `ScheduleDelete`

**Projections (8)**: `ProjectionCapabilities`, `ProjectionSubscribe`,
`ProjectionResume`, `ProjectionAck`, `ProjectionUnsubscribe`,
`ProjectionSnapshotGet`, `ProjectionArtifactRead`, `ProjectionArtifactList`

**Tool Programs (5)**: `ToolProgramList`, `ToolProgramInspect`,
`ToolProgramCallPage`, `ToolProgramNotificationReinject`,
`ToolProgramRecoveryDebugInspect`

**Human Shell Runs (4)**: `HumanShellRunCreate`,
`HumanShellRunCapabilities`, `HumanShellRunRead`, `HumanShellRunCancel`

**Human Shell Promotions (2)**: `HumanShellPromotionStage`,
`HumanShellPromotionsGet` — daemon-staged `!cmd` / `!!cmd` envelopes promoted
into the next turn's model context.

**User Prompt Queue (8)**: `UserPromptQueueList`, `UserPromptQueueAdd`,
`UserPromptQueueEdit`, `UserPromptQueueDelete`, `UserPromptQueueReorder`,
`UserPromptQueueAdopt`, `UserPromptQueueTransfer`,
`UserPromptQueueSendNow` — the durable, queued human-prompt surface.

**Continuation Grants (3)**: `ContinuationGrantCreate`,
`ContinuationGrantRevoke`, `ContinuationGrantStatus` — bounded capability
grants for a continued run.

**Node Telemetry (1)**: `NodeTelemetry` — execution-node telemetry feed.

**Scratch (2)**: `ScratchUsage`, `ScratchReclaim` — the daemon-owned,
disposable scratch tree census and per-tree reclaim verdict
(`reclaimable: Option<bool>` since scratch protocol version 2).

Groups present in `CoreRequest` but not enumerated above (see
`crates/codegg-protocol/src/core.rs:2315` for the authoritative list):
`Document*` (11: `DocumentCapabilities`, `DocumentOpen`, `DocumentCreate`,
`DocumentDelete`, `DocumentSnapshotGet`, `DocumentStatusGet`,
`DocumentWriterAcquire`, `DocumentChange`, `DocumentSave`, `DocumentReload`,
`DocumentClose`),
`WorkspaceDirectoryList` (C005/ADR-0022: daemon-owned directory metadata,
`direct_project` + `file.read`; C006: worker-owned single-flight permit +
full serialized `CoreResponse` 512 KiB cap with trailing-top-level trim to
zero, `truncated=true` when emptied),
`ActiveGoalLoad`,
`ManagedWorktree*` (4), `WorkPlanBindRepository` /
`WorkPlanRepositoryBinding` (2),
`EditCheckpoint*` (6), `Lsp*` (5, incl. M003 `LspCompletionGet`), `Audit*` (3),
`InteractiveProcess*` (10), `Presence*` (3), `Chat*` (20), `Team*` (11),
approval/execution policy (`ApprovalPreferenceGet`, `ApprovalModeSet`,
`SandboxProfileSet`, `NetworkIntentSet`, `RuntimePolicySet`,
`TaskModelPreferenceSet`, `ExecutionPolicyGet`), provider reasoning control
(`ReasoningPreferenceGet`, `ReasoningPreferenceSet`, `ReasoningCapabilities` —
M002: the principal-scoped extended-thinking selection and the
open-vocabulary capability advertisement), and `WorkOrder*` (22, which
already contains `WorkOrderBindRepository`).

### CoreResponse (`core.rs:1202`)

Tagged enum with 174 variants (173 struct-bodied plus the unit `Ack`). Major
groups:

**Connection Responses**: `EggpoolConnectionCreated` (shared result shape,
also aliased as `CreateProviderConnectionResult`),
`ProviderConnectionCreated`, `ProviderSetupList`,
`EggpoolConnectionStatus`, `EggpoolConnectionCancelled`,
`ProviderConnections`, `ProviderConnectionModels`, `ConnectionDetail`,
`ConnectionDetails`, `ConnectionRotateStatus`,
`ConnectionRotateSecretStaged`, `ConnectionRefreshStatus`,
`ConnectionRefreshResult`, `ConnectionPurge`

`/connect` drives provider selection from the secret-free `ProviderSetupList`
projection and submits the secret-bearing `ProviderConnectionCreate` request
over the local boundary only; remote transport denies it via the
`is_secret_bearing` predicate. Eggpool is one catalog entry (a proxy preset),
not a separate protocol.

**Session Responses**: `Ack`, `Json`, `Session`, `SessionMessages`,
`SessionMessageCounts`, `SessionList`, `SnapshotSession`, `SnapshotDaemon`,
`SchedulerSnapshot`, `ModelsSnapshot`, `Events`, `ResyncRequired`,
`Error`

**Workspace Responses**: `WorkspaceList`, `WorkspaceSnapshot`,
`WorkspaceServicesSnapshot`, `WorkspaceConfigReload`

**Project Responses**: `ProjectList`, `ProjectGet`, `ProjectRegistered`,
`ProjectArchived`, `ProjectRestored`, `ProjectDeleted`, `ProjectUpdated`, `ProjectHealth`,
`ProjectCatalogCapabilities`

**Project Initialization Responses**: `ProjectInitDraft` carries the
bounded candidate, diff, evidence, and diagnostics; `ProjectInitPublished`
confirms the resulting digest.

**Run Responses**: `RunList`, `RunGet`, `RunArtifactChunk`, `RunRerunAccepted`

`RunRerun` is an additive request for a bounded, credential-free historical
test-run reconstruction. The response contains the fresh child job identity;
the child RunStore identity and `RunRerunLinked` event arrive through normal
scheduler/test-run completion projections.

**Job Responses**: `JobGet`, `JobList`, `JobAttempts`, `JobCancelResult`,
`JobSubmitted`, `JobWaited`, `JobRetryStarted`

**Schedule Responses**: `ScheduleCreated`, `ScheduleList`, `ScheduleGet`,
`SchedulePaused`, `ScheduleResumed`, `ScheduleDeleted`

**Projection Responses**: `ProjectionCapabilitiesResponse`,
`ProjectionSubscribed`, `ProjectionReplay`, `ProjectionResyncRequired`,
`ProjectionAckAccepted`, `ProjectionUnsubscribed`,
`ProjectionSubscriptionStatusResponse`, `ProjectionArtifactRead`,
`ProjectionArtifactList`

The non-TUI reference consumer in
`codegg_protocol::projection::consumer::HeadlessProjectionConsumer` consumes
these typed responses directly. It is transport-neutral and assumes the
underlying socket/WebSocket has already authenticated the caller. It retains
only a bounded snapshot and cursor, emits `ProjectionResume` after an
interruption, and never treats raw compatibility events as projection state.

**Tool Program Responses**: `ToolProgramList`, `ToolProgramInspect`,
`ToolProgramCallPage`, `ToolProgramNotificationReinjectReport`,
`ToolProgramRecoveryDebugInspectReport`

**Lifecycle Responses**: `SessionLifecycle`, `SessionSelection`,
`SessionSelectionUpdated`, `AssetRefresh`, `AssetRefreshStatus`,
`AssetRefreshCapabilities`, `JobRecoveryReport`

**Reasoning Responses**: `ReasoningPreference` carries the caller's
principal-scoped extended-thinking selection (M002); `ReasoningCapabilities`
carries the daemon's open-vocabulary capability advertisement (M002).

### CoreEvent (`core.rs:4162`)

Tagged enum with 92 variants. Major groups:

**Snapshot (6)**: `SnapshotSession`, `SnapshotWorkspace`, `SnapshotModels`,
`AssetRefreshCompleted`, `ConnectionRotated`, `ConnectionStateChanged`

**Project (6)**: `ProjectRegistered`, `ProjectArchived`,
`ProjectRestored`, `ProjectDeleted`, `ProjectUpdated`, `ProjectHealthChanged`

**Turn (5)**: `TurnStarted`, `TurnTextDelta`, `TurnReasoningDelta`,
`TurnCompleted`, `TurnFailed`

**Tool (2)**: `ToolStarted`, `ToolCompleted`

**Permission/Question (2)**: `PermissionPending`, `QuestionPending`

**Session (2)**: `SessionUpdated`, `FileChanged`

**Subagent (4)**: `SubagentStarted`, `SubagentProgress`,
`SubagentCompleted`, `SubagentFailed`

**Test Run (3)**: `TestRunStarted`, `TestRunProgress`, `TestRunCompleted`

**Run (9)**: `RunStarted`, `RunProgress`, `RunArtifactCreated`,
`RunProjectionReady`, `RunCompleted`, `RunDenied`, `RunPinned`,
`ContextPromotionChanged`, `RunRerunLinked`

**Plugin UI (1)**: `PluginUiEffect` (carries `UiEffectEnvelope`)

**Job (13)**: `JobCreated`, `JobQueued`, `JobBlocked`,
`JobAttemptCreated`, `JobStarted`, `JobProgress`, `JobCancelRequested`,
`JobCompleted`, `JobFailed`, `JobCancelled`, `JobTimedOut`,
`JobInterrupted`, `JobRetried`

**Schedule (6)**: `ScheduleCreated`, `ScheduleOccurrenceQueued`,
`ScheduleSkipped`, `SchedulePaused`, `ScheduleResumed`, `ScheduleDeleted`

**Agent Run (6)**: `AgentRunUpserted`, `AgentRunProgress`, `AgentRunTerminal`,
`AgentRunControlUpdated`, `AgentRunGroupUpserted`, `ConvergenceUpserted` —
see `architecture/agent.md`

**Session Control (2)**: `SessionControlChanged`, `SessionControlRequested`

**Todos (1)**: `TodoListUpdated`

**Worktree (1)**: `WorktreeUpserted`

**LSP (1)**: `LspDiagnosticsUpdated`

**Tool Program (3)**: `ToolProgramCompleted`, `ToolProgramFailed`,
`ToolProgramUpdated`

**Projection (1)**: `ProjectionStreamEvent`

**Interactive Process (1)**: `InteractiveProcessExited`

**Presence (1)**: `PresenceUpdated`

**Chat (5)**: `ChatMessageCommitted`, `ChatMessageEdited`, `ChatMessageRedacted`,
`ChatComposingUpdated`, `ChatActionUpdated`

**Chat Policy (1)**: `ChatPolicyChanged`

**Team (3)**: `TeamMembershipChanged`, `TeamPrincipalChanged`,
`TeamTokenChanged`

**Work Order (4)**: `WorkOrderChanged`, `WorkOrderLaneChanged`,
`WorkOrderOccurrenceChanged`, `WorkOrderTriggerChanged`

**Background Job (1)**: `BackgroundJobCompleted`

**User Prompt Queue (1)**: `UserPromptQueue`

**Error (1)**: `Error`

### TuiMessage (`tui.rs:19`)

Tagged enum with 40 variants. Major groups:

**Envelope (1)**: `EventEnvelope`

**Client-to-Server (3)**: `Input`, `KeyDown`, `MouseClick`

**Connection (4)**: `Resize`, `Resume`, `RequestSnapshot`,
`ResyncRequired`

**Response (2)**: `PermissionResponse`, `QuestionResponse`

**Server-to-Client (11)**: `RenderFrame` (unsupported — returns error),
`TextDelta`, `PermissionPending`, `QuestionPending`, `SessionInfo`,
`SessionEnded`, `ToolCallStarted`, `ToolResult`, `PluginUiEffect`,
`Error`, `StateSnapshot`

**Projection (19)**: `ProjectionCapabilities`,
`ProjectionCapabilitiesAck`, `ProjectionSubscribe`,
`ProjectionSnapshot`, `ProjectionReplay`, `ProjectionResync`,
`ProjectionAck`, `ProjectionAckResult`, `ProjectionEvent`,
`ProjectionResume`, `ProjectionUnsubscribe`,
`ProjectionUnsubscribeResult`, `ProjectionSubscriptionStatus`,
`ProjectionSubscriptionStatusResult`,
`ProjectionArtifactListRequest`, `ProjectionArtifactListResult`,
`ProjectionArtifactReadRequest`, `ProjectionArtifactReadResult`,
`ProjectionCompatibilityDiagnostic`

### ClientHello / ServerHello (`frames.rs`)

`ClientHello` carries `client_name`, `ClientKind` (Tui/Gui/Web/Cli/Automation),
`protocol_version`, and `ClientCapabilities`. `ServerHello` carries
`daemon_id`, `protocol_version`, `ServerCapabilities`, and `client_id`.

The trusted frontend composition selects these values with
`codegg_client::FrontendDescriptor`; renderer input does not choose its
connection identity or capabilities. The descriptor changes client-side
composition only and does not grant authorization.

`ClientCapabilities` (`frames.rs:35`) has 16 boolean fields:
`visual_notifications`, `desktop_notifications`, `audio`, `tts`,
`multi_session_view`, 8 `plugin_ui_*` flags (`plugin_ui_dialog`,
`plugin_ui_toast`, `plugin_ui_panel`, `plugin_ui_status_item`,
`plugin_ui_table`, `plugin_ui_markdown`, `plugin_ui_code`,
`plugin_ui_progress`), and `workspace_registration`, `project_catalog`,
`session_projection`.

`ServerCapabilities` (`frames.rs:102`) has 10 boolean fields:

| Field | `#[serde(default)]` | Meaning |
|-------|---------------------|---------|
| `event_replay` | no | Event replay is supported |
| `session_management` | no | Session CRUD is supported |
| `permission_routing` | no | Permission routing is supported |
| `workspace_registration` | yes | Workspace registration/snapshots (replaces legacy `SnapshotWorkspace { project_dir }`) |
| `workspace_snapshots` | yes | `WorkspaceSnapshot` records in turn snapshots |
| `durable_jobs` | yes | Durable job submit/list/cancel protocol variants |
| `durable_schedules` | yes | Durable schedules |
| `identity_aware_context` | yes | Canonical project/workspace context + session bindings |
| `project_catalog` | yes | Bounded project catalog request/response/event family |
| `session_projection` | yes | `codegg_protocol::projection` frontend-neutral contract |

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

`ProjectionEvent` has 47 variants (`projection/event.rs:128`). The reducer
is pure, deterministic, and never performs I/O. It deduplicates by
`event_seq` and records diagnostics for impossible transitions rather
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
- Replay transport types for subscription replay (`replay.rs`)
- Transport-neutral `ProjectionClientController` frontend state machine
  (`controller.rs`) and the `HeadlessProjectionConsumer` reference consumer
  (`consumer.rs`)
- Bounded workstream view derived from one authorized session snapshot
  (`workstream.rs`)
- Golden fixtures (`fixtures.rs`)

## The Command Family (M007-D)

ADR-0052 gave the daemon one **catalog** and two **request variants**. The
family is `DaemonRequestFamily::Commands` (`src/core/daemon_family.rs:71`),
routed to `src/core/daemon_commands.rs::handle_command_request`
(`src/core/daemon_family.rs:402`). It sits **beside** the existing families
and changes none of them.

**Two variants, and only two.**

- `CoreRequest::CommandCatalog` (`crates/codegg-protocol/src/core.rs:2349`)
  — the daemon's catalog, as `CommandCatalogDto` (`:1276`).
- `CoreRequest::CommandExecute` (`crates/codegg-protocol/src/core.rs:2356`)
  — one execution of one catalog form, as `CommandExecuteRequestDto`
  (`:1290`), answered by `CommandExecuteResultDto` (`:1335`).

A third variant would be a wider protocol surface than ADR-0052 authorized,
so the milestone stops rather than adding one.

**The scope.** `CommandCatalogScopeDto` (`core.rs:1208`) carries the
`project_id`, the `workspace_id`, and an **optional** `session_id`. The
session is absent by default and the daemon refuses a session-scoped
command with a typed reason rather than acting on a session the caller did
not name: the daemon resolves the project and the workspace from its own
durable binding, but the session is the caller's selection.

**The result carries the daemon's inner response.**
`CommandExecuteResultDto::response` (`core.rs:1363`) is the command's
**own** response, exactly as the daemon's request handler serializes it,
alongside the bounded presentation fields (`summary`, `lines`, `report`).
The daemon owns the effect *and* the authoritative data; the frontend keeps
the formatting. A frontend that reads `response` formats the same data the
TUI formats — which is what makes presentation parity real rather than
approximate — and a frontend that does not read it renders `summary` and
`lines` instead.

**Serve-ability is published, not guessed.** `CommandFormDto::daemon_served`
(`core.rs:1246`) says whether the daemon can execute that form. A frontend
reads the flag and refuses an unserved form itself, rather than dispatching
into a refusal.

**The bounds** live in `crates/codegg-core/src/command_catalog.rs`
(`bounds`, `:186`): 256 commands, 512 forms, 256 KiB of serialized catalog,
256 bytes per description, 8 aliases per command, 32 forms per command, 64
bytes per command identity, 4 KiB per argument, 256 result lines and 64 KiB
of serialized result. An over-bound catalog is **refused** with a typed
`CatalogBoundError` (`check_catalog_bounds()`, `:3234`, enforced at
`src/core/daemon_commands.rs:633`) — never silently truncated.


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
- **Per-model reasoning capability (M004).** `ProviderModelDto` and
  `SelectedModelDto` each carry an optional
  `reasoning_capability: Option<ReasoningCapabilityDto>`, declared with
  `#[serde(default, skip_serializing_if = "Option::is_none")]` — the same
  additive-optional pattern `ConnectionHealthDto::credential_status` already
  uses in `provider.rs`. A daemon that publishes no capability and a client
  that predates the field therefore interoperate in both directions: an old
  client ignores the new field, and a new client reads the absence as
  "unknown", never as "no reasoning".
- **`ReasoningCapabilityDto` is four independent axes**, not one verdict:
  `effort`, `disable`, `budget`, and `always_reasons`, each a
  `ReasoningCapabilityAxis` (`Supported | Unsupported | Unknown | Mixed |
  Conflicting`). `Unknown` means *nobody has published a contract for this
  dimension*; it is deliberately distinct from `Unsupported` (*the model
  refuses this*). The payload is display and selection data only — it grants
  no permission, and the daemon's admission policy never reads it as
  authority.
- **`reasoning_options(capability)` is a pure derivation**, not a contract:
  it turns a capability into the option set a frontend may offer, and it can
  only ever narrow what the capability permits. With no capability it falls
  back to the known-label ladder and sets `fallback = true`, so a surface
  that shows the ladder says so instead of presenting it as the model's
  options.
- **The WS-attach transport cannot write the selection.** `TuiMessage` carries
  no `CoreRequest` variant, so a frontend attached over the remote WebSocket
  has no channel to send `CoreRequest::ReasoningPreferenceSet` on. There,
  `/reasoning` reports that the selection cannot be stored and leaves the
  display alone; it does not fall back to a local value the daemon would
  ignore. Adding the channel means adding a `TuiMessage` variant that carries
  a core request — a protocol change that is deliberately not smuggled into
  the frontend milestone.

## Invariants & Gotchas

- **RenderFrame is unsupported**: `TuiMessage::RenderFrame` returns an
  error with code `unsupported_render_frame`. Use `StateSnapshot` instead.
- **Event ordering**: Events are ordered by monotonic `event_seq`.
  No separate sequence system is used.
- **Session-scoped effects**: Plugin effects with `session_id` are
  filtered to subscribed clients.
- **Durable surfaces**: Panels and status items survive reconnect.
  Dialogs and toasts are transient.
- **Secret-bearing requests rejected by remote WebSocket**: denial is
  defined by secret-bearing semantics (`CoreRequest::is_secret_bearing`),
  not by provider names — `EggpoolConnectionCreate`,
  `ProviderConnectionCreate`, `ConnectionRotateSecretStage`,
  `ConnectionRotateBegin`, and `TeamTokenCreate` are local-only. The remote
  core WebSocket rejects them with `secret_operation_remote_denied`, and the
  `secret_bearing_variants_are_denied` protocol test pins the set so a
  future secret-bearing variant fails until it is added to the guard.
- **Projection is additive**: Unknown optional variants are tolerated
  within the declared version range.
- **Presence is ephemeral and non-authoritative** (M001):
  `PresenceHeartbeat` / `PresenceSnapshotGet` carry only locators plus
  a semantic `PresenceActivityDto`; principals come from transport
  authority. Snapshots are bounded and privacy-filtered; denials use
  `project_not_found`. `PresenceUpdated` carries no collaborator
  detail. See `architecture/presence.md`.
- **Observation reuses projection subscribe/resume** (presence M003):
  no new wire variant. Session-scope `ProjectionSubscribe` requires
  canonical `session.observe` (in addition to the gate's
  `project.observe`); `ProjectionResume` rechecks on every resume;
  session/project subscribe and artifact list/read denials use
  `project_not_found`. Redaction and artifact bounds are unchanged.
  See `architecture/presence.md`.
- **Project chat is a versioned namespaced capability** (collaboration
  M001, `chat.v1`, protocol version 1): `ChatChannelEnsure/List`,
  `ChatHistory/Send/Edit/Redact`, `ChatReadSet/Get`,
  `ChatComposingSet/List`, `ChatSync`, plus `ChatCapabilities`.
  `project.chat` is the role baseline; team-collaboration M002
  (ADR-0006) adds the revisioned access overlay (active membership
  first, then project/channel overrides). Gate denials use
  `project_not_found`, handler denials use `chat_channel_not_found`.
  Message bodies are bounded inert text (free text never executes);
  secrets are redacted before durable write. Sync is bounded cursor
  pagination with explicit `resync_required` once a cursor predates
  the retention floor. Events (`ChatMessageCommitted/Edited/Redacted`,
  `ChatComposingUpdated`) are structural liveness hints. Older clients
  ignore the chat surface and retain role-default behavior. M003 adds
  explicit `ChatActionSubmit/Get/List`, `ChatAction/ChatActionList`,
  and `ChatActionUpdated` for separately authorized structured actions
  (typed payload + idempotency key; chat access plus the ordinary
  semantic capability; free text never becomes an action). Team M002
  adds `ChatPolicyGet/List`, `ChatProjectPolicySet`,
  `ChatChannelPolicySet` (`member.manage`, optimistic revision),
   `ChatPolicy/ChatPolicyList`, and `ChatPolicyChanged` (ids/revision
   only). See `architecture/collaboration.md`.
- **Team administration is a versioned namespaced capability**
  (team-collaboration M003, `team.v1`, protocol version 1):
  `TeamMembershipList/Add/Update/Revoke` (`member.manage`;
  `TeamMembership/TeamMembershipList`), `TeamPrincipalList/Create/
  StatusSet` and `TeamTokenList/Create/Revoke` (LocalOwner-only
  `Opaque`; `TeamPrincipal/TeamPrincipalList`, `TeamToken/TeamTokenList`),
  plus `TeamCapabilities`. `TeamTokenCreated` returns the one-time
  `cggt_...` plaintext exactly once (list/get never repeat it);
  `TeamTokenCreate` is secret-bearing (local-only,
  `secret_operation_remote_denied`). Events (`TeamMembershipChanged`,
  `TeamPrincipalChanged`, `TeamTokenChanged`) carry ids/revision only.
  Membership denials use `project_not_found`; principal/token denials
  keep the typed scope shape. Older clients ignore the team surface.
  See `architecture/identity.md`.

## Testing

```bash
cargo test -p codegg-protocol              # all protocol tests
cargo test --test session_projection_consumer  # projection consumer
cargo test --test headless_projection_consumer # non-TUI CoreResponse consumer
```

## Related Docs

- `architecture/collaboration.md` — project channels/messages/sync (M001)
- `architecture/presence.md` — project-scoped presence leases (M001)
- `architecture/projection.md` — full projection contract
- `architecture/core.md` — core facade and transport adapters
- `architecture/server.md` — HTTP/WebSocket server
- `architecture/plugin.md` — plugin system
# Editor document protocol

The additive native `document.v1` family exposes capabilities, open, snapshot,
writer acquisition, status, change, save, reload, and close. Open attaches
read-only; `file.modify` writer ownership is a separate lease. Changes carry a
base revision and stable change ID. Save responses report the committed
revision, disk-base digest, and whether LSP synchronization degraded. A
`document_disk_conflict` response exposes only structural error metadata and
leaves unsaved text intact. Snapshot responses reconcile external disk changes
at an explicit boundary: clean text refreshes with a new revision, while dirty
text is preserved and marked conflicted. Document bodies remain bounded by
the protocol's 8 MiB limit.

The reusable `codegg-client::DocumentController` calls this same request
family for TUI and headless native clients. It preserves stable change IDs for
uncertain retries and uses `DocumentStatusGet` only for observer metadata;
status revision changes require an explicit authoritative snapshot request.

## Source verification

Verified 2026-10-23 against `crates/codegg-protocol/src/lib.rs`,
`core.rs`, `frames.rs`, `tui.rs`, `document.rs`, `human_shell.rs`,
`scratch.rs`, `ui.rs`, and `projection/`, plus
`crates/codegg-core/src/authorization/policy.rs` for the capability split and
`src/shell/daemon.rs` for the human-shell run TTL.
- Re-counted every wire total by enumerating variant declarations rather
  than estimating: **CoreRequest 266** (240 struct-bodied + 26 unit,
  `core.rs:2315` through `core.rs:3958`), **CoreResponse 174** (173
  struct-bodied plus the unit `Ack`, `core.rs:1202` through `core.rs:2147`),
  **CoreEvent 92** (`core.rs:4162` onward), **ProjectionEvent 47**
  (`projection/event.rs:128`), and **TuiMessage 40** (`tui.rs:19`). These
  supersede the 234 / 149 / 87 totals recorded by the 2026-10-10 pass and
  match the `architecture/overview.md` verified-counts table.
- Re-derived every CoreRequest group header from the live variant list and
  made the accounting sum to 266: Session Lifecycle 19 -> 20 (added
  `SessionLaunch`), Project Catalog 9 -> 10 (added `ProjectUpdate`),
  `Document*` 9 -> 11 (added `DocumentCreate` and `DocumentDelete`),
  `Chat*` 19 -> 20 (added `ChatProjectPolicySet`/`ChatChannelPolicySet`),
  and the approval/execution-policy list gained `NetworkIntentSet`. Added
  six previously unmentioned groups: Human Shell Runs (4), Human Shell
  Promotions (2), User Prompt Queue (8), Continuation Grants (3), Node
  Telemetry (1), Scratch (2). `WorkOrder*` (22) already contains
  `WorkOrderBindRepository`, so the `WorkPlan*` pair no longer lists it
  separately — that correction is what keeps the total at 266 rather
  than 267.
- Re-derived the CoreEvent group headers: Project 5 -> 6, and added the 17
  groups that account for the 39 previously unlisted events (Schedule 6,
  Agent Run 6, Session Control 2, Todos 1, Worktree 1, LSP 1, Tool Program
  3, Projection 1, Interactive Process 1, Presence 1, Chat 5, Chat Policy 1,
  Team 3, Work Order 4, Background Job 1, User Prompt Queue 1, Error 1).
- Fixed stale line references: `PROTOCOL_VERSION` `core.rs:28` -> `:29`,
  `RequestEnvelope` `core.rs:33` -> `:77`, `EventEnvelope` `core.rs:984` ->
  `:1188`, `CoreRequest` `core.rs:1857` -> `:2315`, `CoreResponse`
  `core.rs:1041` -> `:1202`, `CoreEvent` `core.rs:3135` -> `:4162`, the
  "authoritative list" pointer `core.rs:1806` -> `:2315`, and
  `is_secret_bearing` `core.rs:3117` -> `:4127`.
- Rebuilt the "Where It Lives" tree: `lib.rs` declares 15 public modules and
  the doc omitted `scratch.rs`; `projection/` holds 12 submodules and the doc
  omitted `workstream.rs`.
- Added the two missing protocol version constants with their locations:
  `HUMAN_SHELL_PROTOCOL_VERSION = 1` (`human_shell.rs:5`) and
  `SCRATCH_PROTOCOL_VERSION = 2` (`scratch.rs:32`).
- Added the human-shell bounds the prose asserted but did not cite:
  `HUMAN_SHELL_MAX_COMMAND_BYTES = 16 KiB`,
  `HUMAN_SHELL_MAX_TIMEOUT_SECS = 3600`, `HUMAN_SHELL_MAX_READ_BYTES =
  64 KiB`, and `RUN_TTL_MS = 10 * 60 * 1000` (`src/shell/daemon.rs:17`).
- Verified accurate, no change: `ServerCapabilities` (`frames.rs:102`) has
  exactly 10 boolean fields and only `event_replay`, `session_management`,
  and `permission_routing` lack `#[serde(default)]`; `ClientCapabilities`
  (`frames.rs:35`) has exactly 16 boolean fields including the eight
  `plugin_ui_*` flags; `PLUGIN_PROTOCOL_VERSION = 1` at `plugin.rs:7`;
  `PROJECTION_PROTOCOL_VERSION = 1` at `projection/caps.rs:27`;
  `REMOTE_TUI_PROTOCOL_VERSION = 5` at `tui.rs:14`; `UiLimits`' 10 fields and
  the `balanced()` / `text_only()` presets; `validate_ui_effect()`,
  `validate_ui_effects()`, `validate_ui_node()`, `degrade_effect()`,
  `degrade_node_to_text()`, and `effect_summary()` all exist in `ui.rs`;
  `is_secret_bearing` matches exactly five variants;
  the `secret_bearing_variants_are_denied` test exists at `core.rs:4966`;
  the `CreateProviderConnectionResult` alias;
  `projection::consumer::HeadlessProjectionConsumer`; and the
  `MAX_DOCUMENT_TEXT_BYTES` 8 MiB bound.

Re-verified 2026-10-25 against `crates/codegg-protocol/src/core.rs`
(`CommandKindDto` `:1197`, `CommandCatalogScopeDto` `:1208`,
`CommandFormDto` `:1228`, `CommandDescriptorDto` `:1251`,
`CommandCatalogDto` `:1276`, `CommandExecuteRequestDto` `:1290`,
`CommandExecuteResultDto` `:1335`, `CoreRequest::CommandCatalog` `:2349`,
`CoreRequest::CommandExecute` `:2356`), `src/core/daemon_family.rs`
(`DaemonRequestFamily::Commands` `:71`, its handler mapping `:402`) and
`src/core/daemon_commands.rs` (the bounds check `:633`): the command family
adds exactly **two** `CoreRequest` variants and changes no existing one.
