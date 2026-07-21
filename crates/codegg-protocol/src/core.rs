use serde::{Deserialize, Serialize};

use crate::dto::{
    CancelResultDto, ConfigDiagnosticDto, JobAttemptDto, JobQueryDto, JobRecordDto, JobSubmitDto,
    JobSummaryDto, ProjectDetailsDto, ProjectHealthDto, ProjectRegisterRequestDto,
    ProjectSummaryDto, RecoveryReportDto, RunQueryDto, RunRecordDto, RunSummaryDto,
    ScheduleCreateDto, ScheduleRecordDto, ScheduleSummaryDto, SessionBindingDto,
    WorkspaceServiceHealthDto,
};
use crate::provider::{
    ConnectionDetailDto, ConnectionProvisioningStatusDto, ConnectionRefreshStatusDto,
    ConnectionRotateChange, ConnectionRotateStatusDto, CreateEggpoolConnectionRequest,
    CreateEggpoolConnectionResult, ProviderConnectionSummaryDto, ProviderModelDto, PurgeOutcome,
    SecretInput, SecretInputRef, SessionLifecycleProjection, SessionSelectionDto,
    UpdateSessionSelectionRequest,
};

/// Core protocol version.
///
/// Bumped to 2 in Phase 15: `CoreEvent::PluginUiEffect` now carries a
/// typed [`crate::ui::UiEffectEnvelope`] (with explicit source) rather
/// than flat fields, making plugin UI transport frontend-neutral and
/// uniformly validated across the bus, event log, and remote replay
/// path. Old clients that ignore unknown variants remain
/// forward-compatible.
pub const PROTOCOL_VERSION: u32 = 2;
pub const ASSET_REFRESH_CAPABILITY: &str = "runtime_assets.refresh.v1";
pub const PROJECT_CATALOG_CAPABILITY: &str = "project_catalog.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEnvelope<T> {
    pub protocol_version: u32,
    pub request_id: String,
    pub payload: T,
}

/// Bounded project/workspace runtime-asset refresh protocol surface. Asset
/// bodies and absolute paths stay local to the daemon and are never carried
/// in these DTOs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetRefreshScopeDto {
    pub project_id: String,
    pub workspace_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssetRefreshReasonDto {
    Startup,
    ProjectActivation,
    SessionLifecycle,
    Manual,
    Reload,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssetRefreshOutcomeDto {
    Published,
    Retained,
    Cancelled,
    Invalid,
    Coalesced,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetRefreshRequestDto {
    pub scope: AssetRefreshScopeDto,
    #[serde(default = "default_manual_refresh_reason")]
    pub reason: AssetRefreshReasonDto,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetRefreshReportDto {
    pub scope: AssetRefreshScopeDto,
    pub reason: AssetRefreshReasonDto,
    pub outcome: AssetRefreshOutcomeDto,
    #[serde(default)]
    pub generation: Option<u64>,
    #[serde(default)]
    pub previous_generation: Option<u64>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub added: Vec<String>,
    #[serde(default)]
    pub removed: Vec<String>,
    #[serde(default)]
    pub changed: Vec<String>,
    #[serde(default)]
    pub shadowed: Vec<String>,
    #[serde(default)]
    pub invalid: Vec<String>,
    #[serde(default)]
    pub retained: Vec<String>,
    #[serde(default)]
    pub diagnostics: Vec<String>,
    #[serde(default)]
    pub coalesced: bool,
    pub completed_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetRefreshStatusDto {
    pub scope: AssetRefreshScopeDto,
    #[serde(default)]
    pub generation: Option<u64>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub last_success_at_ms: Option<i64>,
    pub in_flight: bool,
    #[serde(default)]
    pub last_outcome: Option<AssetRefreshOutcomeDto>,
    #[serde(default)]
    pub last_diagnostics: Vec<String>,
}

fn default_manual_refresh_reason() -> AssetRefreshReasonDto {
    AssetRefreshReasonDto::Manual
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope<T> {
    pub protocol_version: u32,
    pub event_seq: u64,
    pub timestamp_ms: i64,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub payload: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreResponse {
    Ack,
    AssetRefresh {
        report: AssetRefreshReportDto,
    },
    AssetRefreshStatus {
        status: AssetRefreshStatusDto,
    },
    AssetRefreshCapabilities {
        supported: bool,
        max_report_entries: usize,
    },
    EggpoolConnectionCreated {
        result: CreateEggpoolConnectionResult,
    },
    EggpoolConnectionStatus {
        status: ConnectionProvisioningStatusDto,
    },
    EggpoolConnectionCancelled {
        operation_id: String,
    },
    ProviderConnections {
        connections: Vec<ProviderConnectionSummaryDto>,
    },
    ProviderConnectionModels {
        connection_id: String,
        catalog_revision: Option<String>,
        models: Vec<ProviderModelDto>,
    },
    ConnectionDetail {
        detail: ConnectionDetailDto,
    },
    ConnectionDetails {
        details: Vec<ConnectionDetailDto>,
    },
    ConnectionRotateStatus {
        result: ConnectionRotateStatusDto,
    },
    ConnectionRotateSecretStaged {
        request_id: String,
        secret: SecretInputRef,
    },
    ConnectionRefreshStatus {
        result: ConnectionRefreshStatusDto,
    },
    ConnectionRefreshResult {
        result: ConnectionRefreshStatusDto,
    },
    ConnectionPurge {
        outcome: PurgeOutcome,
    },
    SessionLifecycle {
        projection: SessionLifecycleProjection,
    },
    /// Provider Connections Milestone 3: redacted list of selectable
    /// connections plus their model catalogs for the current session.
    SessionSelection {
        session_id: String,
        selection: SessionSelectionDto,
    },
    /// Provider Connections Milestone 3: confirmation of a successful
    /// session selection update.
    SessionSelectionUpdated {
        session_id: String,
        selection: SessionSelectionDto,
    },
    Json {
        data: serde_json::Value,
    },
    Session {
        session: crate::dto::Session,
    },
    SessionMessages {
        session_id: String,
        messages: Vec<crate::dto::Message>,
    },
    SessionMessageCounts {
        counts: std::collections::HashMap<String, usize>,
    },
    SessionList {
        sessions: Vec<crate::dto::Session>,
    },
    /// Phase 2: registered workspaces returned from `WorkspaceList`.
    /// Clients that don't yet advertise the workspace capability simply
    /// ignore this variant (or destructure it for future-proofing).
    WorkspaceList {
        workspaces: Vec<crate::dto::WorkspaceSnapshot>,
    },
    /// Phase 2: snapshot of a single registered workspace.
    WorkspaceSnapshot {
        workspace: crate::dto::WorkspaceSnapshot,
    },
    /// Phase 3: workspace service health snapshots for every active
    /// bundle.
    WorkspaceServicesSnapshot {
        services: Vec<WorkspaceServiceHealthDto>,
    },
    /// Phase 3: reload result for a workspace configuration.
    WorkspaceConfigReload {
        workspace_id: String,
        previous_revision: u64,
        new_revision: u64,
        diagnostics: Vec<ConfigDiagnosticDto>,
    },
    /// Project Catalog M004: bounded project catalog list.
    ProjectList {
        projects: Vec<ProjectSummaryDto>,
        truncated: bool,
    },
    /// Project Catalog M004: one project and bounded relation summaries.
    ProjectGet {
        project: ProjectDetailsDto,
    },
    ProjectRegistered {
        project: ProjectSummaryDto,
    },
    ProjectArchived {
        project: ProjectSummaryDto,
    },
    ProjectRestored {
        project: ProjectSummaryDto,
    },
    ProjectHealth {
        health: ProjectHealthDto,
    },
    ProjectCatalogCapabilities {
        supported: bool,
        max_list_items: usize,
        max_workspaces_per_project: usize,
    },
    /// Phase 3: run summaries returned from `RunList`.
    RunList {
        workspace_id: String,
        runs: Vec<RunSummaryDto>,
    },
    /// Phase 3: full run record returned from `RunGet`.
    RunGet {
        workspace_id: String,
        run: Option<RunRecordDto>,
    },
    /// Phase 3: artifact chunk returned from `RunArtifactRead`.
    RunArtifactChunk {
        workspace_id: String,
        artifact_id: String,
        data_b64: String,
        byte_offset: usize,
        total_bytes: u64,
    },
    Error {
        code: String,
        message: String,
    },
    SnapshotSession {
        event_seq: u64,
        session: crate::dto::Session,
        messages: Vec<crate::dto::Message>,
        status: String,
        selected_model: Option<String>,
        selected_agent: Option<String>,
        pending_permissions: Vec<String>,
        pending_questions: Vec<String>,
        input_tokens: Option<usize>,
        output_tokens: Option<usize>,
        active_subagents: usize,
    },
    SnapshotDaemon {
        event_seq: u64,
        daemon_id: String,
        uptime_secs: u64,
        active_sessions: Vec<SessionSnapshot>,
        connected_clients: Vec<ClientSnapshot>,
        /// Bounded scheduler state. Historical jobs remain available through
        /// the job query APIs rather than being embedded in this snapshot.
        #[serde(default)]
        scheduler_snapshot: Option<serde_json::Value>,
    },
    /// Bounded scheduler state for operator-facing clients.
    SchedulerSnapshot {
        snapshot: serde_json::Value,
    },
    ModelsSnapshot {
        current_model: Option<String>,
        models: Vec<String>,
    },
    Events {
        events: Vec<EventEnvelope<CoreEvent>>,
        current_seq: u64,
    },
    ResyncRequired {
        from_event_seq: u64,
        current_seq: u64,
        session_id: Option<String>,
    },
    // ── Phase 4: Durable Jobs and Schedules ──────────────────────────
    /// Full job record returned from `JobGet`.
    JobGet {
        job: Option<JobRecordDto>,
    },
    /// Job summaries returned from `JobList`.
    JobList {
        jobs: Vec<JobSummaryDto>,
    },
    /// Attempt records returned from `JobAttempts`.
    JobAttempts {
        job_id: String,
        attempts: Vec<JobAttemptDto>,
    },
    /// Outcome of a cancellation request.
    JobCancelResult {
        result: CancelResultDto,
    },
    /// Acknowledgement of a successful job submission.
    JobSubmitted {
        job_id: String,
    },
    /// Bounded completion projection for a daemon-owned job. Large output
    /// remains in the RunStore or executor-specific artifacts.
    JobWaited {
        job_id: String,
        status: String,
        summary: String,
        #[serde(default)]
        run_id: Option<String>,
    },
    /// Acknowledgement that a retry attempt was started.
    JobRetryStarted {
        job_id: String,
        attempt_id: String,
    },
    /// Acknowledgement of a successful schedule creation.
    ScheduleCreated {
        schedule_id: String,
    },
    /// Schedule summaries returned from `ScheduleList`.
    ScheduleList {
        schedules: Vec<ScheduleSummaryDto>,
    },
    /// Full schedule record returned from `ScheduleGet`.
    ScheduleGet {
        schedule: ScheduleRecordDto,
    },
    /// Acknowledgement that a schedule was paused.
    SchedulePaused {
        schedule_id: String,
    },
    /// Acknowledgement that a schedule was resumed.
    ScheduleResumed {
        schedule_id: String,
    },
    /// Acknowledgement that a schedule was deleted.
    ScheduleDeleted {
        schedule_id: String,
    },
    /// Report from a recovery pass triggered by `JobRecoveryReport`.
    JobRecoveryReport {
        report: RecoveryReportDto,
    },
    // ── Session Projections M2: Replay Protocol ──────────────────────
    /// Daemon projection replay capabilities and negotiated limits.
    ProjectionCapabilitiesResponse {
        supported: bool,
        projection_version: u32,
        max_events_per_batch: usize,
        max_event_bytes: usize,
        max_subscriptions_per_client: usize,
        max_subscriptions_per_daemon: usize,
        retention_session_max_events: usize,
        retention_project_max_events: usize,
    },
    /// Subscription established with descriptor, snapshot, and cursor.
    ProjectionSubscribed {
        subscription_id: crate::projection::replay::ProjectionSubscriptionId,
        descriptor: crate::projection::replay::ProjectionStreamDescriptor,
        snapshot: crate::projection::replay::ProjectionSnapshotBundle,
        cursor: crate::projection::replay::ProjectionCursor,
        retention_floor_seq: u64,
    },
    /// Ordered replay batch returned on resume or subscribe catch-up.
    ProjectionReplay {
        batch: crate::projection::replay::ProjectionReplayBatch,
    },
    /// Resync required with reason and optional snapshot bundle.
    ProjectionResyncRequired {
        reason: crate::projection::replay::ProjectionResyncReason,
        descriptor: Option<crate::projection::replay::ProjectionStreamDescriptor>,
        requested_cursor: Option<crate::projection::replay::ProjectionCursor>,
        snapshot: Option<crate::projection::replay::ProjectionSnapshotBundle>,
    },
    /// Acknowledgement accepted with current lag.
    ProjectionAckAccepted {
        subscription_id: crate::projection::replay::ProjectionSubscriptionId,
        last_acked_seq: u64,
        lag_count: u64,
    },
    /// Unsubscribe acknowledgement.
    ProjectionUnsubscribed {
        subscription_id: crate::projection::replay::ProjectionSubscriptionId,
    },
    /// Subscription status for diagnostics.
    ProjectionSubscriptionStatusResponse {
        status: crate::projection::replay::ProjectionSubscriptionStatus,
    },
    // ── Session Projections M3: Artifact Read Protocol ────────────────
    /// Artifact read outcome.
    ProjectionArtifactRead {
        outcome: crate::projection::replay::ProjectionArtifactReadOutcome,
    },
    /// Artifact list for a project.
    ProjectionArtifactList {
        handles: Vec<crate::projection::replay::ProjectionArtifactHandleDto>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub project_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// Canonical project/workspace binding. Absent on legacy sessions.
    #[serde(default)]
    pub binding: Option<SessionBindingDto>,
    #[serde(default)]
    pub directory: String,
    pub status: String,
    pub selected_model: Option<String>,
    pub selected_agent: Option<String>,
    pub has_active_turn: bool,
    pub pending_permissions: Vec<String>,
    pub pending_questions: Vec<String>,
    pub input_tokens: Option<usize>,
    pub output_tokens: Option<usize>,
    pub active_subagents: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSnapshot {
    pub client_id: String,
    pub client_name: String,
    pub connected_at: String,
    pub attached_sessions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreRequest {
    Initialize,
    AssetRefresh {
        request: AssetRefreshRequestDto,
    },
    AssetRefreshStatus {
        scope: AssetRefreshScopeDto,
    },
    AssetRefreshCapabilities,
    EggpoolConnectionCreate {
        request: CreateEggpoolConnectionRequest,
    },
    EggpoolConnectionCancel {
        operation_id: String,
    },
    EggpoolConnectionStatus {
        operation_id: String,
    },
    ProviderConnectionList,
    ProviderConnectionModels {
        connection_id: String,
    },
    /// Secret-bearing rotation request. The secret is an opaque local input
    /// handle and is rejected by the remote WebSocket transport.
    ConnectionRotateBegin {
        request_id: String,
        connection_id: String,
        expected_revision: u64,
        change: ConnectionRotateChange,
        secret: SecretInputRef,
    },
    /// Local-only staging request. The plaintext is accepted only across the
    /// daemon's local authenticated boundary and is immediately moved into a
    /// bounded daemon-owned secret buffer.
    ConnectionRotateSecretStage {
        request_id: String,
        secret: SecretInput,
    },
    ConnectionRotateCancel {
        request_id: String,
    },
    ConnectionRotateStatus {
        request_id: String,
    },
    ConnectionRefreshBegin {
        connection_id: String,
        expected_revision: u64,
    },
    ConnectionRefreshCancel {
        operation_id: String,
    },
    ConnectionRefreshStatus {
        operation_id: String,
    },
    ConnectionGet {
        connection_id: String,
    },
    ConnectionListDetail,
    ConnectionEnable {
        connection_id: String,
        expected_revision: u64,
        #[serde(default)]
        require_probe: bool,
    },
    ConnectionDisable {
        connection_id: String,
        expected_revision: u64,
    },
    ConnectionDelete {
        connection_id: String,
        expected_revision: u64,
    },
    ConnectionRestore {
        connection_id: String,
        expected_revision: u64,
    },
    ConnectionPurge {
        connection_id: String,
        expected_revision: u64,
    },
    Subscribe {
        session_id: Option<String>,
    },
    Resume {
        session_id: Option<String>,
        from_event_seq: u64,
    },
    SessionList {
        project_id: String,
        show_archived: bool,
        limit: usize,
    },
    SessionCreate {
        directory: String,
        title: Option<String>,
        /// Optional canonical identity for identity-aware clients.
        #[serde(default)]
        project_id: Option<String>,
        /// Optional canonical workspace for identity-aware clients.
        #[serde(default)]
        workspace_id: Option<String>,
    },
    SessionAttach {
        session_id: String,
    },
    SessionLoad {
        session_id: String,
    },
    SessionMessagesLoad {
        session_id: String,
    },
    SessionMessageCounts {
        session_ids: Vec<String>,
    },
    SessionFork {
        session_id: String,
    },
    SessionDelete {
        session_id: String,
        permanent: bool,
    },
    SessionArchive {
        session_id: String,
        unarchive: bool,
    },
    SessionRestore {
        session_id: String,
    },
    SessionShare {
        session_id: String,
    },
    SessionUnshare {
        session_id: String,
    },
    SessionRename {
        session_id: String,
        new_title: String,
    },
    SessionExport {
        session_id: String,
    },
    SessionImportData {
        data: serde_json::Value,
    },
    SessionCreateFromTemplate {
        template: crate::dto::SessionTemplate,
        /// Legacy project projection, now optional for identity-aware
        /// clients that provide the explicit project/workspace context.
        #[serde(default)]
        project_id: Option<String>,
        directory: String,
        /// Optional canonical workspace for identity-aware clients.
        #[serde(default)]
        workspace_id: Option<String>,
    },
    TurnSubmit {
        session_id: String,
        text: String,
        plan_mode: bool,
        model: String,
        agents: Vec<crate::dto::Agent>,
        current_agent_idx: usize,
        messages: Vec<crate::dto::ProviderMessage>,
    },
    TurnCancel {
        session_id: String,
        turn_id: String,
    },
    TurnSteer {
        session_id: String,
        turn_id: String,
        text: String,
    },
    AgentSelect {
        session_id: String,
        agent_name: String,
    },
    ModelSelect {
        session_id: String,
        model: String,
    },
    /// Provider Connections Milestone 3: read the session's current
    /// connection/model selection. Resolves the legacy `provider/model`
    /// string on demand and returns a typed diagnostic when no
    /// connection matches.
    SessionSelectionGet {
        session_id: String,
    },
    /// Provider Connections Milestone 3: list connections available to
    /// the current session for selection. Always redacted; never carries
    /// credentials.
    SessionSelectionList {
        session_id: String,
    },
    /// Provider Connections Milestone 3: write a new connection + model
    /// selection for a session with optimistic revision checks.
    SessionSelectionUpdate {
        request: Box<UpdateSessionSelectionRequest>,
    },
    /// Provider Connections Milestone 3: list the bounded model catalog
    /// for a single connection, scoped to the session's authoritative
    /// context. The catalog revision is returned so stale revisions are
    /// detected.
    SessionSelectionModels {
        session_id: String,
        connection_id: String,
    },
    SessionLifecycleGet {
        session_id: String,
    },
    ModelsRefresh,
    PermissionRespond {
        id: String,
        choice: String,
    },
    QuestionRespond {
        id: String,
        answers: serde_json::Value,
    },
    MemorySearch {
        query: String,
    },
    MemoryList {
        namespace: String,
    },
    MemoryRemember {
        text: String,
        namespace: Option<String>,
    },
    MemoryForget {
        id: String,
    },
    TaskList,
    TaskSchedule {
        session_id: String,
        interval_secs: u64,
        message: String,
    },
    TaskDelete {
        id: u64,
    },
    WorktreeList {
        project_dir: String,
    },
    /// Phase 2: register or look up the workspace rooted at `root`.
    /// Returns the same workspace id on every call (idempotent).
    WorkspaceRegister {
        root: String,
    },
    /// Phase 2: list registered workspaces (archived ones opt-in).
    WorkspaceList {
        include_archived: bool,
    },
    /// Phase 2: archive a workspace. Subsequent turn submissions for its
    /// sessions are rejected until the workspace is rebound or restored.
    WorkspaceArchive {
        workspace_id: String,
    },
    /// Phase 2: snapshot a single workspace by id.
    WorkspaceSnapshotRequest {
        workspace_id: String,
    },
    /// Phase 3: snapshot of every active workspace service bundle.
    /// Used by remote/socket TUIs to render health indicators and by
    /// the `WorkspaceServiceRegistry::evict_idle` task to surface
    /// decisions over the protocol.
    WorkspaceServicesSnapshot,
    /// Phase 3: reload a workspace's configuration snapshot, bumping
    /// the revision seen by future leases. Existing leases continue
    /// to see their previously-held snapshot.
    WorkspaceConfigReload {
        workspace_id: String,
    },
    /// Project Catalog M004: list durable logical projects.
    ProjectList {
        #[serde(default)]
        include_archived: bool,
        #[serde(default)]
        limit: usize,
    },
    ProjectGet {
        project_id: String,
    },
    ProjectRegister {
        request: ProjectRegisterRequestDto,
    },
    ProjectArchive {
        project_id: String,
    },
    ProjectRestore {
        project_id: String,
    },
    ProjectHealth {
        project_id: String,
        workspace_id: String,
    },
    ProjectCatalogCapabilities,
    /// Phase 3: list runs visible from the workspace's RunStore. The
    /// run query parameters mirror `RunStore::list_runs`.
    RunList {
        workspace_id: String,
        query: RunQueryDto,
    },
    /// Phase 3: read a single run record.
    RunGet {
        workspace_id: String,
        run_id: String,
    },
    /// Phase 3: read a range of an artifact's bytes from the
    /// workspace's RunStore. The response is a binary chunk carried
    /// inside a JSON envelope for transport simplicity.
    RunArtifactRead {
        workspace_id: String,
        artifact_id: String,
        start: usize,
        end: usize,
    },
    GoalSet {
        session_id: String,
        project_id: String,
        objective: String,
    },
    GoalFromFile {
        session_id: String,
        project_id: String,
        path: String,
    },
    GoalShow {
        session_id: String,
    },
    GoalPause {
        session_id: String,
    },
    GoalResume {
        session_id: String,
    },
    GoalClear {
        session_id: String,
    },
    GoalDone {
        session_id: String,
    },
    GoalCheckpoint {
        session_id: String,
        project_id: String,
    },
    /// Load the persisted todo list for a session so the TUI can render
    /// it without keeping a separate `Arc<Mutex<TodoState>>` in sync.
    TodoList {
        session_id: String,
    },
    /// Load the active goal snapshot (and progress) for a session.
    ActiveGoalLoad {
        session_id: String,
    },
    /// Set or replace the goal budget. The store revives a
    /// `BudgetLimited` goal to `Active` if the new budget is high
    /// enough to satisfy the existing usage.
    GoalSetBudget {
        session_id: String,
        max_turns: Option<i64>,
        max_model_tokens: Option<i64>,
        max_tool_calls: Option<i64>,
        max_wallclock_secs: Option<i64>,
    },
    SnapshotSession {
        session_id: String,
    },
    SnapshotWorkspace {
        project_dir: String,
    },
    SnapshotModels,
    SnapshotDaemon,
    /// Route a TTS speak request through the daemon's `NotificationRouter`
    /// rather than speaking locally. Used in `RemoteCore` mode where the
    /// local TUI has no audio output of its own.
    ///
    /// `kind` and `priority` are optional string labels (`turn_completed`,
    /// `turn_failed`, `awaiting_input`, `permission_required`,
    /// `question_required`, `subagent_completed`, `subagent_failed`,
    /// `error`; and `low` / `normal` / `high` / `urgent` respectively).
    /// Unknown values fall back to a normal-priority `AwaitingInput`
    /// event so the router still surfaces the message.
    NotificationSpeak {
        text: String,
        kind: Option<String>,
        priority: Option<String>,
        session_id: Option<String>,
    },
    /// Ask the daemon to stop any currently-active TTS playback
    /// (delegates to the `AudioArbiter` interrupt channel).
    NotificationStop,
    // ── Phase 4: Durable Jobs and Schedules ──────────────────────────
    /// Submit a new durable job.
    JobSubmit {
        spec: JobSubmitDto,
    },
    /// Request the current bounded scheduler snapshot.
    SchedulerSnapshot,
    /// Wait for one durable job completion without giving the client direct
    /// access to scheduler internals.
    JobWait {
        job_id: String,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    /// Fetch a single job record by id.
    JobGet {
        job_id: String,
    },
    /// List jobs matching a query.
    JobList {
        query: JobQueryDto,
    },
    /// Request cancellation of a running or queued job.
    JobCancel {
        job_id: String,
        #[serde(default)]
        reason: Option<String>,
    },
    /// Retry a failed or timed-out job by creating a new attempt.
    JobRetry {
        job_id: String,
    },
    /// List all attempts for a job.
    JobAttempts {
        job_id: String,
    },
    /// Create a new durable schedule.
    ScheduleCreate {
        spec: ScheduleCreateDto,
    },
    /// List schedules, optionally filtered by workspace.
    ScheduleList {
        #[serde(default)]
        workspace_id: Option<String>,
        #[serde(default)]
        include_archived: bool,
    },
    /// Fetch a single schedule record by id.
    ScheduleGet {
        schedule_id: String,
    },
    /// Pause a schedule.
    SchedulePause {
        schedule_id: String,
    },
    /// Resume a paused schedule.
    ScheduleResume {
        schedule_id: String,
    },
    /// Delete a schedule.
    ScheduleDelete {
        schedule_id: String,
    },
    /// Trigger a recovery pass and return the report.
    JobRecoveryReport,
    // ── Session Projections M2: Replay Protocol ──────────────────────
    /// Query daemon projection replay capabilities and limits.
    ProjectionCapabilities,
    /// Subscribe to a canonical projection stream.
    ProjectionSubscribe {
        request: crate::projection::replay::ProjectionSubscriptionRequest,
    },
    /// Resume from a cursor and optionally receive a snapshot on resync.
    ProjectionResume {
        cursor: crate::projection::replay::ProjectionCursor,
        #[serde(default)]
        include_snapshot_if_resync: bool,
    },
    /// Acknowledge processed projection events.
    ProjectionAck {
        ack: crate::projection::replay::ProjectionAck,
    },
    /// Unsubscribe from a projection stream.
    ProjectionUnsubscribe {
        subscription_id: crate::projection::replay::ProjectionSubscriptionId,
    },
    /// Fetch a snapshot bundle for a stream.
    ProjectionSnapshotGet {
        scope: crate::projection::replay::ProjectionStreamKind,
        scope_id: String,
    },
    // ── Session Projections M3: Artifact Read Protocol ────────────────
    /// Read artifact content through an authorized handle.
    ProjectionArtifactRead {
        request: crate::projection::replay::ProjectionArtifactReadRequest,
        project_id: String,
        context_correlation_id: Option<String>,
    },
    /// List artifact handles for a project.
    ProjectionArtifactList {
        project_id: String,
    },
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreEvent {
    AssetRefreshCompleted {
        report: AssetRefreshReportDto,
    },
    ConnectionRotated {
        connection_id: String,
        new_revision: u64,
        catalog_revision: Option<String>,
        actor_seam: String,
    },
    ConnectionStateChanged {
        connection_id: String,
        old_state: String,
        new_state: String,
        actor_seam: String,
        at: i64,
    },
    SnapshotSession {
        session_id: String,
    },
    SnapshotWorkspace {
        project_dir: String,
    },
    ProjectRegistered {
        project_id: String,
        project: ProjectSummaryDto,
    },
    ProjectArchived {
        project_id: String,
        project: ProjectSummaryDto,
    },
    ProjectRestored {
        project_id: String,
        project: ProjectSummaryDto,
    },
    ProjectHealthChanged {
        project_id: String,
        workspace_id: String,
        health: ProjectHealthDto,
    },
    SnapshotModels {
        #[serde(skip_serializing_if = "Option::is_none")]
        current_model: Option<String>,
        models: Vec<String>,
    },
    TurnStarted {
        session_id: String,
        turn_id: String,
    },
    TurnTextDelta {
        session_id: String,
        turn_id: String,
        delta: String,
    },
    TurnReasoningDelta {
        session_id: String,
        turn_id: String,
        delta: String,
    },
    ToolStarted {
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        tool_name: String,
        tool_id: String,
        arguments: String,
    },
    ToolCompleted {
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        tool_id: String,
        output: String,
        success: bool,
    },
    PermissionPending {
        id: String,
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        tool: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    QuestionPending {
        id: String,
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        questions: serde_json::Value,
    },
    TurnCompleted {
        session_id: String,
        turn_id: String,
        stop_reason: String,
    },
    TurnFailed {
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        message: String,
    },
    SessionUpdated {
        session_id: String,
    },
    FileChanged {
        path: String,
        action: String,
    },
    SubagentStarted {
        session_id: String,
        task_id: u64,
        agent: String,
        description: String,
    },
    SubagentProgress {
        session_id: String,
        task_id: u64,
        agent: String,
        message: String,
    },
    SubagentCompleted {
        session_id: String,
        task_id: u64,
        agent: String,
        result_summary: String,
    },
    SubagentFailed {
        session_id: String,
        task_id: u64,
        agent: String,
        error: String,
    },
    /// A supervised test run started.
    TestRunStarted {
        session_id: String,
        job_id: String,
        command: String,
        cwd: String,
    },
    /// Progress during a supervised test run.
    TestRunProgress {
        session_id: String,
        job_id: String,
        message: String,
    },
    /// A supervised test run completed.
    TestRunCompleted {
        session_id: String,
        job_id: String,
        status: String,
        summary: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        log_dir: Option<String>,
    },
    /// A command/script/test run started.
    RunStarted {
        session_id: String,
        run_id: String,
        kind: String,
        command: String,
    },
    /// Progress update for a long-running run.
    RunProgress {
        session_id: String,
        run_id: String,
        message: String,
    },
    /// An artifact was created for a run.
    RunArtifactCreated {
        session_id: String,
        run_id: String,
        artifact_id: String,
        kind: String,
        byte_length: u64,
    },
    /// Projection output is ready for a run.
    RunProjectionReady {
        session_id: String,
        run_id: String,
        projector: String,
        exactness: String,
    },
    /// A run completed.
    RunCompleted {
        session_id: String,
        run_id: String,
        status: String,
        summary: String,
    },
    /// A run was denied by policy.
    RunDenied {
        session_id: String,
        run_id: String,
        reason: String,
    },
    /// A run was pinned or unpinned.
    RunPinned {
        run_id: String,
        pinned: bool,
    },
    /// Context promotion state changed for a run.
    ContextPromotionChanged {
        session_id: String,
        run_id: String,
        state: String,
    },
    /// A rerun was linked to its parent run.
    RunRerunLinked {
        session_id: String,
        parent_run_id: String,
        child_run_id: String,
    },
    /// A plugin produced a UI effect (dialog, toast, panel, status item,
    /// etc.) through a lifecycle hook or session-scoped command.
    ///
    /// Phase 15: the effect is carried inside a typed
    /// [`crate::ui::UiEffectEnvelope`] so the origin (Plugin/Core/Tui),
    /// session, and invocation are all encoded uniformly. This makes
    /// ownership checks and capability gating deterministic across the
    /// bus, event log, and remote replay path.
    PluginUiEffect {
        envelope: crate::ui::UiEffectEnvelope,
    },
    // ── Phase 4: Durable Jobs and Schedules ──────────────────────────
    /// A new durable job was created.
    JobCreated {
        job_id: String,
        workspace_id: String,
        kind: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
    },
    /// A job moved to the queued state.
    JobQueued {
        job_id: String,
        workspace_id: String,
    },
    /// A job was blocked waiting on dependencies.
    JobBlocked {
        job_id: String,
        workspace_id: String,
    },
    /// A new execution attempt was created for a job.
    JobAttemptCreated {
        job_id: String,
        attempt_id: String,
        sequence: u32,
        daemon_generation: String,
    },
    /// An attempt started executing.
    JobStarted {
        job_id: String,
        attempt_id: String,
    },
    /// Progress update for a running attempt.
    JobProgress {
        job_id: String,
        attempt_id: String,
        message: String,
    },
    /// A cancellation was requested for a job.
    JobCancelRequested {
        job_id: String,
        reason: String,
    },
    /// A job attempt completed successfully.
    JobCompleted {
        job_id: String,
        attempt_id: String,
    },
    /// A job attempt failed.
    JobFailed {
        job_id: String,
        attempt_id: String,
        error_class: String,
        message: String,
    },
    /// A job attempt was cancelled.
    JobCancelled {
        job_id: String,
        attempt_id: String,
    },
    /// A job attempt timed out.
    JobTimedOut {
        job_id: String,
        attempt_id: String,
    },
    /// A job attempt was interrupted by daemon restart.
    JobInterrupted {
        job_id: String,
        attempt_id: String,
        recovery_generation: String,
    },
    /// A retry was initiated for a job.
    JobRetried {
        job_id: String,
        new_attempt_id: String,
        prior_attempt_id: String,
    },
    /// A new schedule was created.
    ScheduleCreated {
        schedule_id: String,
        workspace_id: String,
        kind_summary: String,
    },
    /// A schedule occurrence was queued as a job.
    ScheduleOccurrenceQueued {
        schedule_id: String,
        scheduled_for_ms: i64,
        job_id: String,
    },
    /// A schedule occurrence was skipped.
    ScheduleSkipped {
        schedule_id: String,
        scheduled_for_ms: i64,
        reason: String,
    },
    /// A schedule was paused.
    SchedulePaused {
        schedule_id: String,
    },
    /// A schedule was resumed.
    ScheduleResumed {
        schedule_id: String,
    },
    /// A schedule was deleted.
    ScheduleDeleted {
        schedule_id: String,
    },
    /// Live projection event delivered to a subscription.
    ProjectionStreamEvent {
        subscription_id: crate::projection::replay::ProjectionSubscriptionId,
        stream_id: crate::projection::replay::ProjectionStreamId,
        envelope: crate::projection::event::ProjectionEnvelope,
    },
    Error {
        code: String,
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UiEffect;

    #[test]
    fn protocol_version_is_set() {
        assert_eq!(PROTOCOL_VERSION, 2);
    }

    #[test]
    fn request_envelope_serializes() {
        let req = RequestEnvelope {
            protocol_version: 1,
            request_id: "test-1".to_string(),
            payload: CoreRequest::SessionCreate {
                directory: "/tmp".to_string(),
                title: Some("Test".to_string()),
                project_id: None,
                workspace_id: None,
            },
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("session_create"));
        assert!(json.contains("test-1"));
    }

    #[test]
    fn legacy_session_create_fixture_decodes_without_identity_context() {
        let request: CoreRequest = serde_json::from_str(
            r#"{"type":"session_create","directory":"/tmp/legacy","title":null}"#,
        )
        .unwrap();

        match request {
            CoreRequest::SessionCreate {
                directory,
                title,
                project_id,
                workspace_id,
            } => {
                assert_eq!(directory, "/tmp/legacy");
                assert_eq!(title, None);
                assert_eq!(project_id, None);
                assert_eq!(workspace_id, None);
            }
            other => panic!("expected legacy SessionCreate, got {other:?}"),
        }
    }

    #[test]
    fn identity_aware_session_create_round_trips_without_version_bump() {
        let request = CoreRequest::SessionCreate {
            directory: "/tmp/project".to_string(),
            title: Some("Identity-aware".to_string()),
            project_id: Some("project-1".to_string()),
            workspace_id: Some("workspace-1".to_string()),
        };

        let json = serde_json::to_string(&request).unwrap();
        let decoded: CoreRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            decoded,
            CoreRequest::SessionCreate {
                project_id: Some(ref project_id),
                workspace_id: Some(ref workspace_id),
                ..
            } if project_id == "project-1" && workspace_id == "workspace-1"
        ));
        assert_eq!(PROTOCOL_VERSION, 2);
    }

    #[test]
    fn legacy_template_create_fixture_decodes_with_optional_context_fields() {
        let request: CoreRequest = serde_json::from_str(
            r#"{"type":"session_create_from_template","template":{},"project_id":"legacy-project","directory":"/tmp/legacy"}"#,
        )
        .unwrap();

        match request {
            CoreRequest::SessionCreateFromTemplate {
                project_id,
                directory,
                workspace_id,
                ..
            } => {
                assert_eq!(project_id.as_deref(), Some("legacy-project"));
                assert_eq!(directory, "/tmp/legacy");
                assert_eq!(workspace_id, None);
            }
            other => panic!("expected legacy template request, got {other:?}"),
        }
    }

    #[test]
    fn legacy_session_snapshot_fixture_defaults_binding() {
        let snapshot: SessionSnapshot = serde_json::from_str(
            r#"{
                "session_id":"session-1",
                "project_id":"legacy-project",
                "status":"idle",
                "selected_model":null,
                "selected_agent":null,
                "has_active_turn":false,
                "pending_permissions":[],
                "pending_questions":[],
                "input_tokens":null,
                "output_tokens":null,
                "active_subagents":0
            }"#,
        )
        .unwrap();

        assert_eq!(snapshot.binding, None);
        assert_eq!(snapshot.directory, "");
    }

    #[test]
    fn response_serializes_ack() {
        let resp = CoreResponse::Ack;
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("ack"));
    }

    #[test]
    fn response_serializes_error() {
        let resp = CoreResponse::Error {
            code: "test_error".to_string(),
            message: "test message".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("test_error"));
    }

    #[test]
    fn response_serializes_events() {
        let env = EventEnvelope {
            protocol_version: 1,
            event_seq: 7,
            timestamp_ms: 100,
            session_id: Some("s1".to_string()),
            turn_id: None,
            payload: CoreEvent::Error {
                code: "e".to_string(),
                message: "m".to_string(),
            },
        };
        let resp = CoreResponse::Events {
            events: vec![env],
            current_seq: 7,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"events\""));
        assert!(json.contains("\"current_seq\":7"));
        assert!(json.contains("\"event_seq\":7"));
        let back: CoreResponse = serde_json::from_str(&json).unwrap();
        match back {
            CoreResponse::Events {
                events,
                current_seq,
            } => {
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].event_seq, 7);
                assert_eq!(current_seq, 7);
            }
            other => panic!("expected Events, got {:?}", other),
        }
    }

    #[test]
    fn response_serializes_resync_required() {
        let resp = CoreResponse::ResyncRequired {
            from_event_seq: 5,
            current_seq: 100,
            session_id: Some("s1".to_string()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"resync_required\""));
        assert!(json.contains("\"from_event_seq\":5"));
        assert!(json.contains("\"current_seq\":100"));
        let back: CoreResponse = serde_json::from_str(&json).unwrap();
        match back {
            CoreResponse::ResyncRequired {
                from_event_seq,
                current_seq,
                session_id,
            } => {
                assert_eq!(from_event_seq, 5);
                assert_eq!(current_seq, 100);
                assert_eq!(session_id.as_deref(), Some("s1"));
            }
            other => panic!("expected ResyncRequired, got {:?}", other),
        }
    }

    #[test]
    fn event_envelope_has_seq() {
        let env = EventEnvelope {
            protocol_version: 1,
            event_seq: 42,
            timestamp_ms: 1234567890,
            session_id: Some("s1".to_string()),
            turn_id: None,
            payload: CoreEvent::Error {
                code: "e".to_string(),
                message: "m".to_string(),
            },
        };
        assert_eq!(env.event_seq, 42);
        assert_eq!(env.session_id.as_deref(), Some("s1"));
    }

    #[test]
    fn core_frame_tagged_correctly() {
        use crate::frames::CoreFrame;
        let frame = CoreFrame::Ping;
        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains("ping"));
    }

    #[test]
    fn core_event_plugin_ui_effect_round_trip() {
        let effect = crate::ui::UiEffect::ShowToast {
            toast: crate::ui::ToastSpec {
                level: crate::ui::ToastLevel::Info,
                message: "plugin says hi".into(),
            },
        };
        let env = EventEnvelope {
            protocol_version: PROTOCOL_VERSION,
            event_seq: 10,
            timestamp_ms: 100,
            session_id: Some("s1".into()),
            turn_id: None,
            payload: CoreEvent::PluginUiEffect {
                envelope: crate::ui::UiEffectEnvelope {
                    session_id: Some("s1".into()),
                    source: crate::ui::UiEffectSource::Plugin {
                        plugin_id: "my-plugin".into(),
                    },
                    invocation_id: Some("inv-1".into()),
                    effect,
                },
            },
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("plugin_ui_effect"));
        assert!(json.contains("my-plugin"));
        let back: EventEnvelope<CoreEvent> = serde_json::from_str(&json).unwrap();
        match back.payload {
            CoreEvent::PluginUiEffect { envelope } => {
                assert_eq!(
                    envelope.source,
                    crate::ui::UiEffectSource::Plugin {
                        plugin_id: "my-plugin".into(),
                    }
                );
                assert_eq!(envelope.invocation_id.as_deref(), Some("inv-1"));
                assert_eq!(envelope.session_id.as_deref(), Some("s1"));
                assert!(matches!(envelope.effect, UiEffect::ShowToast { .. }));
            }
            other => panic!("expected PluginUiEffect, got {:?}", other),
        }
    }

    #[test]
    fn core_event_plugin_ui_effect_with_core_source() {
        let env = EventEnvelope {
            protocol_version: PROTOCOL_VERSION,
            event_seq: 11,
            timestamp_ms: 101,
            session_id: None,
            turn_id: None,
            payload: CoreEvent::PluginUiEffect {
                envelope: crate::ui::UiEffectEnvelope {
                    session_id: None,
                    source: crate::ui::UiEffectSource::Core,
                    invocation_id: None,
                    effect: crate::ui::UiEffect::EmitChat {
                        block: crate::ui::ChatBlock {
                            format: crate::ui::ChatFormat::Plain,
                            content: "core says hi".into(),
                        },
                    },
                },
            },
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("core"));
        let back: EventEnvelope<CoreEvent> = serde_json::from_str(&json).unwrap();
        match back.payload {
            CoreEvent::PluginUiEffect { envelope } => {
                assert_eq!(envelope.source, crate::ui::UiEffectSource::Core);
                assert!(matches!(
                    envelope.effect,
                    crate::ui::UiEffect::EmitChat { .. }
                ));
            }
            other => panic!("expected PluginUiEffect, got {:?}", other),
        }
    }

    #[test]
    fn core_event_test_run_started_round_trip() {
        let env = EventEnvelope {
            protocol_version: PROTOCOL_VERSION,
            event_seq: 20,
            timestamp_ms: 200,
            session_id: Some("s1".into()),
            turn_id: None,
            payload: CoreEvent::TestRunStarted {
                session_id: "s1".into(),
                job_id: "job-1".into(),
                command: "cargo test".into(),
                cwd: "/tmp".into(),
            },
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("test_run_started"));
        assert!(json.contains("cargo test"));
        let back: EventEnvelope<CoreEvent> = serde_json::from_str(&json).unwrap();
        match back.payload {
            CoreEvent::TestRunStarted {
                session_id,
                job_id,
                command,
                cwd,
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(job_id, "job-1");
                assert_eq!(command, "cargo test");
                assert_eq!(cwd, "/tmp");
            }
            other => panic!("expected TestRunStarted, got {:?}", other),
        }
    }

    #[test]
    fn core_event_test_run_progress_round_trip() {
        let env = EventEnvelope {
            protocol_version: PROTOCOL_VERSION,
            event_seq: 21,
            timestamp_ms: 201,
            session_id: Some("s1".into()),
            turn_id: None,
            payload: CoreEvent::TestRunProgress {
                session_id: "s1".into(),
                job_id: "job-1".into(),
                message: "3 passed, 1 failed".into(),
            },
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("test_run_progress"));
        let back: EventEnvelope<CoreEvent> = serde_json::from_str(&json).unwrap();
        match back.payload {
            CoreEvent::TestRunProgress {
                session_id,
                job_id,
                message,
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(job_id, "job-1");
                assert_eq!(message, "3 passed, 1 failed");
            }
            other => panic!("expected TestRunProgress, got {:?}", other),
        }
    }

    #[test]
    fn core_event_test_run_completed_round_trip() {
        let env = EventEnvelope {
            protocol_version: PROTOCOL_VERSION,
            event_seq: 22,
            timestamp_ms: 202,
            session_id: Some("s1".into()),
            turn_id: None,
            payload: CoreEvent::TestRunCompleted {
                session_id: "s1".into(),
                job_id: "job-1".into(),
                status: "passed".into(),
                summary: "5 passed in 2.3s".into(),
                log_dir: Some("/tmp/logs".into()),
            },
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("test_run_completed"));
        assert!(json.contains("passed"));
        let back: EventEnvelope<CoreEvent> = serde_json::from_str(&json).unwrap();
        match back.payload {
            CoreEvent::TestRunCompleted {
                session_id,
                job_id,
                status,
                summary,
                log_dir,
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(job_id, "job-1");
                assert_eq!(status, "passed");
                assert_eq!(summary, "5 passed in 2.3s");
                assert_eq!(log_dir.as_deref(), Some("/tmp/logs"));
            }
            other => panic!("expected TestRunCompleted, got {:?}", other),
        }
    }

    #[test]
    fn core_event_test_run_completed_omits_none_log_dir() {
        let env = EventEnvelope {
            protocol_version: PROTOCOL_VERSION,
            event_seq: 23,
            timestamp_ms: 203,
            session_id: Some("s1".into()),
            turn_id: None,
            payload: CoreEvent::TestRunCompleted {
                session_id: "s1".into(),
                job_id: "job-2".into(),
                status: "failed".into(),
                summary: "1 failed".into(),
                log_dir: None,
            },
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(!json.contains("log_dir"));
    }

    #[test]
    fn asset_refresh_protocol_is_bounded_and_defaults_manual_reason() {
        let request: CoreRequest = serde_json::from_str(
            r#"{"type":"asset_refresh","request":{"scope":{"project_id":"p1","workspace_id":"w1"}}}"#,
        )
        .unwrap();
        match request {
            CoreRequest::AssetRefresh { request } => {
                assert_eq!(request.reason, AssetRefreshReasonDto::Manual);
                assert!(request.session_id.is_none());
            }
            other => panic!("expected asset refresh request, got {other:?}"),
        }

        let report = AssetRefreshReportDto {
            scope: AssetRefreshScopeDto {
                project_id: "p1".into(),
                workspace_id: "w1".into(),
            },
            reason: AssetRefreshReasonDto::Reload,
            outcome: AssetRefreshOutcomeDto::Published,
            generation: Some(3),
            previous_generation: Some(2),
            fingerprint: Some("abc".into()),
            added: vec!["skill:review".into()],
            removed: vec![],
            changed: vec![],
            shadowed: vec![],
            invalid: vec![],
            retained: vec![],
            diagnostics: vec!["bounded diagnostic".into()],
            coalesced: false,
            completed_at_ms: 1,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("body"));
        assert!(!json.contains("absolute_path"));
        assert!(json.len() < 1024);
    }

    #[test]
    fn project_catalog_requests_responses_and_events_round_trip() {
        let request: CoreRequest = serde_json::from_value(serde_json::json!({
            "type": "project_list",
            "include_archived": false,
            "limit": 2
        }))
        .unwrap();
        assert!(matches!(
            request,
            CoreRequest::ProjectList {
                include_archived: false,
                limit: 2
            }
        ));

        let project = ProjectSummaryDto {
            project_id: "project-1".into(),
            display_name: "Project One".into(),
            lifecycle: "active".into(),
            description: None,
            tags: vec![],
            time_last_opened_at: None,
            registration_source: "test".into(),
            archived_at: None,
            created_at: 1,
            updated_at: 2,
        };
        let response = CoreResponse::ProjectList {
            projects: vec![project.clone()],
            truncated: false,
        };
        let decoded: CoreResponse =
            serde_json::from_value(serde_json::to_value(response).unwrap()).unwrap();
        assert!(matches!(
            decoded,
            CoreResponse::ProjectList {
                projects,
                truncated: false
            } if projects == vec![project.clone()]
        ));

        let event = CoreEvent::ProjectRegistered {
            project_id: "project-1".into(),
            project,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("project_registered"));
        assert!(json.contains("project-1"));
    }
}
