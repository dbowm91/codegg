//! Static request classification and capability policy descriptors.

use crate::team::Capability;

/// How the daemon should resolve a request to a project scope.
///
/// The descriptor is static data; resolution itself happens at the daemon
/// boundary where the pool is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeKind {
    /// No project data involved. Any active principal may proceed; the
    /// daemon still constructs a decision so attribution has a context.
    Global,
    /// The request carries a direct `project_id` locator.
    DirectProject,
    /// The request carries a `session_id` locator; the project is the
    /// owning session's project.
    ViaSession,
    /// The request carries a `job_id` locator; the project is resolved
    /// through the job's owning session.
    ViaJob,
    /// A bounded listing whose rows are privacy-filtered after the read
    /// (see [`visible_projects`]).
    Enumeration,
    /// No project linkage is available in the DTO. Team principals fail
    /// closed ([`AuthorizationError::MissingScope`]); only the local-owner
    /// broad policy authorizes these operations.
    Opaque,
}

impl ScopeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::DirectProject => "direct_project",
            Self::ViaSession => "via_session",
            Self::ViaJob => "via_job",
            Self::Enumeration => "enumeration",
            Self::Opaque => "opaque",
        }
    }
}

/// Static authorization descriptor for one native daemon operation.
///
/// Handlers ask for [`OperationDescriptor::capability`]; role expansion
/// stays in [`crate::team`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationDescriptor {
    /// Stable operation name (the `CoreRequest` variant).
    pub operation: &'static str,
    /// How the daemon resolves the request to a project scope.
    pub scope_kind: ScopeKind,
    /// Required semantic capability, or `None` for global operations
    /// that need an authenticated active principal but no project grant.
    pub capability: Option<Capability>,
}

impl OperationDescriptor {
    pub const fn new(
        operation: &'static str,
        scope_kind: ScopeKind,
        capability: Option<Capability>,
    ) -> Self {
        Self {
            operation,
            scope_kind,
            capability,
        }
    }

    /// Capability wire name, or `"none"` for global operations.
    pub fn capability_name(self) -> &'static str {
        self.capability.map_or("none", Capability::as_str)
    }
}

/// Map every native [`CoreRequest`](codegg_protocol::core::CoreRequest) to
/// its scope plus semantic capability.
///
/// The match is exhaustive with no wildcard arm, so adding a
/// `CoreRequest` variant is a compile error until it is classified here.
/// `scripts/check_authorization_matrix.py` additionally asserts that every
/// variant spelled in the protocol crate appears in this function.
///
/// Classification rationale (see `architecture/authorization.md` for the
/// full matrix):
///
/// - Reads require the narrowest observe/read capability; mutations
///   require the corresponding create/modify/execute capability.
/// - Turn/agent/model/provider-selection writes require `agent.invoke`;
///   delegation and test-recovery inspection require `agent.delegate`.
/// - Project lifecycle and sharing require `project.configure`; audit and
///   recovery surfaces require `audit.read`.
/// - Caller-scoped or unguessable-ID operations (memory namespaces,
///   permission/question responses, tasks, notifications, snapshots of
///   daemon-global state) are global: they carry no project-keyed data.
/// - Filesystem-locator, credential-adjacent, and cross-project operations
///   without a project locator are opaque and fail closed for team
///   principals.
pub fn operation_descriptor(request: &codegg_protocol::core::CoreRequest) -> OperationDescriptor {
    use codegg_protocol::core::CoreRequest as R;
    match request {
        R::Initialize => OperationDescriptor::new("initialize", ScopeKind::Global, None),
        R::AssetRefresh { .. } => OperationDescriptor::new(
            "asset_refresh",
            ScopeKind::DirectProject,
            Some(Capability::ProjectConfigure),
        ),
        R::AssetRefreshStatus { .. } => OperationDescriptor::new(
            "asset_refresh_status",
            ScopeKind::DirectProject,
            Some(Capability::ProjectRead),
        ),
        R::AssetRefreshCapabilities => {
            OperationDescriptor::new("asset_refresh_capabilities", ScopeKind::Global, None)
        }
        R::EggpoolConnectionCreate { .. } => OperationDescriptor::new(
            "eggpool_connection_create",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::ProviderConnectionCreate { .. } => OperationDescriptor::new(
            "provider_connection_create",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::ProviderSetupList => {
            OperationDescriptor::new("provider_setup_list", ScopeKind::Enumeration, None)
        }
        R::EggpoolConnectionCancel { .. } => OperationDescriptor::new(
            "eggpool_connection_cancel",
            ScopeKind::Opaque,
            Some(Capability::ProjectRead),
        ),
        R::EggpoolConnectionStatus { .. } => OperationDescriptor::new(
            "eggpool_connection_status",
            ScopeKind::Opaque,
            Some(Capability::ProjectRead),
        ),
        R::ProviderConnectionList => {
            OperationDescriptor::new("provider_connection_list", ScopeKind::Enumeration, None)
        }
        R::ProviderConnectionModels { .. } => OperationDescriptor::new(
            "provider_connection_models",
            ScopeKind::Opaque,
            Some(Capability::ProjectRead),
        ),
        R::ConnectionRotateBegin { .. } => OperationDescriptor::new(
            "connection_rotate_begin",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::ConnectionRotateSecretStage { .. } => OperationDescriptor::new(
            "connection_rotate_secret_stage",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::ConnectionRotateCancel { .. } => OperationDescriptor::new(
            "connection_rotate_cancel",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::ConnectionRotateStatus { .. } => OperationDescriptor::new(
            "connection_rotate_status",
            ScopeKind::Opaque,
            Some(Capability::ProjectRead),
        ),
        R::ConnectionRefreshBegin { .. } => OperationDescriptor::new(
            "connection_refresh_begin",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::ConnectionRefreshCancel { .. } => OperationDescriptor::new(
            "connection_refresh_cancel",
            ScopeKind::Opaque,
            Some(Capability::ProjectRead),
        ),
        R::ConnectionRefreshStatus { .. } => OperationDescriptor::new(
            "connection_refresh_status",
            ScopeKind::Opaque,
            Some(Capability::ProjectRead),
        ),
        R::ConnectionGet { .. } => OperationDescriptor::new(
            "connection_get",
            ScopeKind::Opaque,
            Some(Capability::ProjectRead),
        ),
        R::ConnectionListDetail => {
            OperationDescriptor::new("connection_list_detail", ScopeKind::Enumeration, None)
        }
        R::ConnectionEnable { .. } => OperationDescriptor::new(
            "connection_enable",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::ConnectionDisable { .. } => OperationDescriptor::new(
            "connection_disable",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::ConnectionDelete { .. } => OperationDescriptor::new(
            "connection_delete",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::ConnectionRestore { .. } => OperationDescriptor::new(
            "connection_restore",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::ConnectionPurge { .. } => OperationDescriptor::new(
            "connection_purge",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::Subscribe { .. } => OperationDescriptor::new("subscribe", ScopeKind::Global, None),
        R::Resume { .. } => OperationDescriptor::new("resume", ScopeKind::Global, None),
        R::SessionList { .. } => OperationDescriptor::new(
            "session_list",
            ScopeKind::DirectProject,
            Some(Capability::SessionRead),
        ),
        R::SessionCreate { .. } => OperationDescriptor::new(
            "session_create",
            ScopeKind::DirectProject,
            Some(Capability::SessionCreate),
        ),
        R::SessionAttach { .. } => OperationDescriptor::new(
            "session_attach",
            ScopeKind::ViaSession,
            Some(Capability::SessionRead),
        ),
        R::SessionLoad { .. } => OperationDescriptor::new(
            "session_load",
            ScopeKind::ViaSession,
            Some(Capability::SessionRead),
        ),
        R::SessionMessagesLoad { .. } => OperationDescriptor::new(
            "session_messages_load",
            ScopeKind::ViaSession,
            Some(Capability::SessionRead),
        ),
        R::SessionMessageCounts { .. } => OperationDescriptor::new(
            "session_message_counts",
            ScopeKind::Opaque,
            Some(Capability::SessionRead),
        ),
        R::SessionFork { .. } => OperationDescriptor::new(
            "session_fork",
            ScopeKind::ViaSession,
            Some(Capability::SessionRead),
        ),
        R::SessionDelete { .. } => OperationDescriptor::new(
            "session_delete",
            ScopeKind::ViaSession,
            Some(Capability::SessionCreate),
        ),
        R::SessionArchive { .. } => OperationDescriptor::new(
            "session_archive",
            ScopeKind::ViaSession,
            Some(Capability::SessionCreate),
        ),
        R::SessionRestore { .. } => OperationDescriptor::new(
            "session_restore",
            ScopeKind::ViaSession,
            Some(Capability::SessionCreate),
        ),
        R::SessionShare { .. } => OperationDescriptor::new(
            "session_share",
            ScopeKind::ViaSession,
            Some(Capability::ProjectConfigure),
        ),
        R::SessionUnshare { .. } => OperationDescriptor::new(
            "session_unshare",
            ScopeKind::ViaSession,
            Some(Capability::ProjectConfigure),
        ),
        R::SessionRename { .. } => OperationDescriptor::new(
            "session_rename",
            ScopeKind::ViaSession,
            Some(Capability::SessionCreate),
        ),
        R::SessionExport { .. } => OperationDescriptor::new(
            "session_export",
            ScopeKind::ViaSession,
            Some(Capability::SessionRead),
        ),
        R::SessionImportData { .. } => OperationDescriptor::new(
            "session_import_data",
            ScopeKind::Opaque,
            Some(Capability::SessionCreate),
        ),
        R::SessionCreateFromTemplate { .. } => OperationDescriptor::new(
            "session_create_from_template",
            ScopeKind::DirectProject,
            Some(Capability::SessionCreate),
        ),
        R::TurnSubmit { .. } => OperationDescriptor::new(
            "turn_submit",
            ScopeKind::ViaSession,
            Some(Capability::AgentInvoke),
        ),
        R::TurnCancel { .. } => OperationDescriptor::new(
            "turn_cancel",
            ScopeKind::ViaSession,
            Some(Capability::AgentInvoke),
        ),
        R::TurnSteer { .. } => OperationDescriptor::new(
            "turn_steer",
            ScopeKind::ViaSession,
            Some(Capability::AgentInvoke),
        ),
        R::AgentSelect { .. } => OperationDescriptor::new(
            "agent_select",
            ScopeKind::ViaSession,
            Some(Capability::AgentInvoke),
        ),
        R::ModelSelect { .. } => OperationDescriptor::new(
            "model_select",
            ScopeKind::ViaSession,
            Some(Capability::AgentInvoke),
        ),
        R::SessionSelectionGet { .. } => OperationDescriptor::new(
            "session_selection_get",
            ScopeKind::ViaSession,
            Some(Capability::SessionRead),
        ),
        R::SessionSelectionList { .. } => OperationDescriptor::new(
            "session_selection_list",
            ScopeKind::ViaSession,
            Some(Capability::SessionRead),
        ),
        R::SessionSelectionUpdate { .. } => OperationDescriptor::new(
            "session_selection_update",
            ScopeKind::ViaSession,
            Some(Capability::AgentInvoke),
        ),
        R::SessionSelectionModels { .. } => OperationDescriptor::new(
            "session_selection_models",
            ScopeKind::ViaSession,
            Some(Capability::SessionRead),
        ),
        R::SessionLifecycleGet { .. } => OperationDescriptor::new(
            "session_lifecycle_get",
            ScopeKind::ViaSession,
            Some(Capability::SessionRead),
        ),
        R::ModelsRefresh => OperationDescriptor::new("models_refresh", ScopeKind::Global, None),
        // Permission/question responses carry unguessable single-use ids;
        // the pending-item ownership check lives at the permission layer.
        R::PermissionRespond { .. } => {
            OperationDescriptor::new("permission_respond", ScopeKind::Global, None)
        }
        R::QuestionRespond { .. } => {
            OperationDescriptor::new("question_respond", ScopeKind::Global, None)
        }
        // Memory namespaces are caller-scoped and carry no project-keyed data.
        R::MemorySearch { .. } => {
            OperationDescriptor::new("memory_search", ScopeKind::Global, None)
        }
        R::MemoryList { .. } => OperationDescriptor::new("memory_list", ScopeKind::Global, None),
        R::MemoryRemember { .. } => {
            OperationDescriptor::new("memory_remember", ScopeKind::Global, None)
        }
        R::MemoryForget { .. } => {
            OperationDescriptor::new("memory_forget", ScopeKind::Global, None)
        }
        // Legacy task queue: explicitly rejected downstream; no project data.
        R::TaskList => OperationDescriptor::new("task_list", ScopeKind::Global, None),
        R::TaskSchedule { .. } => {
            OperationDescriptor::new("task_schedule", ScopeKind::Global, None)
        }
        R::TaskDelete { .. } => OperationDescriptor::new("task_delete", ScopeKind::Global, None),
        R::WorktreeList { .. } => OperationDescriptor::new(
            "worktree_list",
            ScopeKind::Opaque,
            Some(Capability::GitRead),
        ),
        R::ManagedWorktreeGet { .. } => OperationDescriptor::new(
            "managed_worktree_get",
            ScopeKind::Opaque,
            Some(Capability::GitRead),
        ),
        R::ManagedWorktreeList { .. } => OperationDescriptor::new(
            "managed_worktree_list",
            ScopeKind::Opaque,
            Some(Capability::GitRead),
        ),
        R::ManagedWorktreeCleanup { .. } => OperationDescriptor::new(
            "managed_worktree_cleanup",
            ScopeKind::Opaque,
            Some(Capability::WorktreeRemove),
        ),
        R::ManagedWorktreeArchive { .. } => OperationDescriptor::new(
            "managed_worktree_archive",
            ScopeKind::Opaque,
            Some(Capability::WorktreeRemove),
        ),
        R::WorkspaceRegister { .. } => OperationDescriptor::new(
            "workspace_register",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::WorkspaceList { .. } => OperationDescriptor::new(
            "workspace_list",
            ScopeKind::Opaque,
            Some(Capability::ProjectRead),
        ),
        R::WorkspaceArchive { .. } => OperationDescriptor::new(
            "workspace_archive",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::WorkspaceSnapshotRequest { .. } => OperationDescriptor::new(
            "workspace_snapshot_request",
            ScopeKind::Opaque,
            Some(Capability::ProjectRead),
        ),
        R::WorkspaceServicesSnapshot => {
            OperationDescriptor::new("workspace_services_snapshot", ScopeKind::Global, None)
        }
        R::WorkspaceConfigReload { .. } => OperationDescriptor::new(
            "workspace_config_reload",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::ProjectList { .. } => OperationDescriptor::new(
            "project_list",
            ScopeKind::Enumeration,
            Some(Capability::ProjectRead),
        ),
        R::ProjectGet { .. } => OperationDescriptor::new(
            "project_get",
            ScopeKind::DirectProject,
            Some(Capability::ProjectRead),
        ),
        // Registration is daemon-local deployment authority, not team
        // bootstrap: raw workspace/project registration and global
        // workspace enumeration are LocalOwner/proven-local only until an
        // explicit team workspace-ownership contract exists. The Opaque
        // scope carries no project locator, so ordinary team principals
        // fail closed with `MissingScope`; only the LocalOwner broad
        // policy authorizes. A `workspace_id` locator never proves
        // ownership and must not authorize project creation.
        R::ProjectRegister { .. } => OperationDescriptor::new(
            "project_register",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::ProjectArchive { .. } => OperationDescriptor::new(
            "project_archive",
            ScopeKind::DirectProject,
            Some(Capability::ProjectConfigure),
        ),
        R::ProjectRestore { .. } => OperationDescriptor::new(
            "project_restore",
            ScopeKind::DirectProject,
            Some(Capability::ProjectConfigure),
        ),
        R::ProjectHealth { .. } => OperationDescriptor::new(
            "project_health",
            ScopeKind::DirectProject,
            Some(Capability::ProjectRead),
        ),
        R::ProjectCatalogCapabilities => {
            OperationDescriptor::new("project_catalog_capabilities", ScopeKind::Global, None)
        }
        // ── Project Work Orders M004: Global Workspace Dashboard ──
        //
        // Enumeration-style like `project_list`: the daemon returns only
        // projects where the caller holds `project.read`. Per-row
        // task/session counts additionally require `session.read` on
        // that project (checked per row in the handler); rows without
        // it carry `counts_visible == false` with zeroed counts so row
        // presence reveals nothing beyond `ProjectList`.
        R::WorkspaceDashboard { .. } => OperationDescriptor::new(
            "workspace_dashboard",
            ScopeKind::Enumeration,
            Some(Capability::ProjectRead),
        ),
        R::RunList { .. } => {
            OperationDescriptor::new("run_list", ScopeKind::Opaque, Some(Capability::SessionRead))
        }
        R::RunGet { .. } => {
            OperationDescriptor::new("run_get", ScopeKind::Opaque, Some(Capability::SessionRead))
        }
        R::RunArtifactRead { .. } => OperationDescriptor::new(
            "run_artifact_read",
            ScopeKind::Opaque,
            Some(Capability::SessionRead),
        ),
        R::RunRerun { .. } => OperationDescriptor::new(
            "run_rerun",
            ScopeKind::ViaSession,
            Some(Capability::AgentInvoke),
        ),
        R::GoalSet { .. } => OperationDescriptor::new(
            "goal_set",
            ScopeKind::ViaSession,
            Some(Capability::AgentInvoke),
        ),
        R::GoalFromFile { .. } => OperationDescriptor::new(
            "goal_from_file",
            ScopeKind::ViaSession,
            Some(Capability::AgentInvoke),
        ),
        R::GoalShow { .. } => OperationDescriptor::new(
            "goal_show",
            ScopeKind::ViaSession,
            Some(Capability::SessionRead),
        ),
        R::GoalPause { .. } => OperationDescriptor::new(
            "goal_pause",
            ScopeKind::ViaSession,
            Some(Capability::SessionCreate),
        ),
        R::GoalResume { .. } => OperationDescriptor::new(
            "goal_resume",
            ScopeKind::ViaSession,
            Some(Capability::SessionCreate),
        ),
        R::GoalClear { .. } => OperationDescriptor::new(
            "goal_clear",
            ScopeKind::ViaSession,
            Some(Capability::SessionCreate),
        ),
        R::GoalDone { .. } => OperationDescriptor::new(
            "goal_done",
            ScopeKind::ViaSession,
            Some(Capability::SessionCreate),
        ),
        R::GoalCheckpoint { .. } => OperationDescriptor::new(
            "goal_checkpoint",
            ScopeKind::ViaSession,
            Some(Capability::SessionCreate),
        ),
        R::TodoList { .. } => OperationDescriptor::new(
            "todo_list",
            ScopeKind::ViaSession,
            Some(Capability::SessionRead),
        ),
        R::ActiveGoalLoad { .. } => OperationDescriptor::new(
            "active_goal_load",
            ScopeKind::ViaSession,
            Some(Capability::SessionRead),
        ),
        R::GoalSetBudget { .. } => OperationDescriptor::new(
            "goal_set_budget",
            ScopeKind::ViaSession,
            Some(Capability::SessionCreate),
        ),
        R::SnapshotSession { .. } => OperationDescriptor::new(
            "snapshot_session",
            ScopeKind::ViaSession,
            Some(Capability::SessionRead),
        ),
        R::SnapshotWorkspace { .. } => OperationDescriptor::new(
            "snapshot_workspace",
            ScopeKind::Opaque,
            Some(Capability::ProjectRead),
        ),
        R::SnapshotModels => OperationDescriptor::new("snapshot_models", ScopeKind::Global, None),
        R::SnapshotDaemon => OperationDescriptor::new("snapshot_daemon", ScopeKind::Global, None),
        R::NotificationSpeak { .. } => {
            OperationDescriptor::new("notification_speak", ScopeKind::Global, None)
        }
        R::NotificationStop => {
            OperationDescriptor::new("notification_stop", ScopeKind::Global, None)
        }
        R::JobSubmit { .. } => OperationDescriptor::new(
            "job_submit",
            ScopeKind::ViaSession,
            Some(Capability::JobSubmit),
        ),
        R::SchedulerSnapshot => OperationDescriptor::new(
            "scheduler_snapshot",
            ScopeKind::Opaque,
            Some(Capability::SessionRead),
        ),
        R::JobWait { .. } => {
            OperationDescriptor::new("job_wait", ScopeKind::ViaJob, Some(Capability::SessionRead))
        }
        R::JobGet { .. } => {
            OperationDescriptor::new("job_get", ScopeKind::ViaJob, Some(Capability::SessionRead))
        }
        R::JobList { .. } => {
            OperationDescriptor::new("job_list", ScopeKind::Opaque, Some(Capability::SessionRead))
        }
        R::JobCancel { .. } => {
            OperationDescriptor::new("job_cancel", ScopeKind::ViaJob, Some(Capability::JobCancel))
        }
        R::JobRetry { .. } => {
            OperationDescriptor::new("job_retry", ScopeKind::ViaJob, Some(Capability::JobCancel))
        }
        R::JobAttempts { .. } => OperationDescriptor::new(
            "job_attempts",
            ScopeKind::ViaJob,
            Some(Capability::SessionRead),
        ),
        R::ScheduleCreate { .. } => OperationDescriptor::new(
            "schedule_create",
            ScopeKind::ViaSession,
            Some(Capability::JobSubmit),
        ),
        R::ScheduleList { .. } => OperationDescriptor::new(
            "schedule_list",
            ScopeKind::Opaque,
            Some(Capability::SessionRead),
        ),
        R::ScheduleGet { .. } => OperationDescriptor::new(
            "schedule_get",
            ScopeKind::Opaque,
            Some(Capability::SessionRead),
        ),
        R::SchedulePause { .. } => OperationDescriptor::new(
            "schedule_pause",
            ScopeKind::Opaque,
            Some(Capability::JobCancel),
        ),
        R::ScheduleResume { .. } => OperationDescriptor::new(
            "schedule_resume",
            ScopeKind::Opaque,
            Some(Capability::JobCancel),
        ),
        R::ScheduleDelete { .. } => OperationDescriptor::new(
            "schedule_delete",
            ScopeKind::Opaque,
            Some(Capability::JobCancel),
        ),
        R::JobRecoveryReport => OperationDescriptor::new(
            "job_recovery_report",
            ScopeKind::Opaque,
            Some(Capability::AuditRead),
        ),
        R::ProjectionCapabilities => {
            OperationDescriptor::new("projection_capabilities", ScopeKind::Global, None)
        }
        R::ProjectionSubscribe { .. } => OperationDescriptor::new(
            "projection_subscribe",
            ScopeKind::Opaque,
            Some(Capability::ProjectObserve),
        ),
        R::ProjectionResume { .. } => {
            OperationDescriptor::new("projection_resume", ScopeKind::Global, None)
        }
        R::ProjectionAck { .. } => {
            OperationDescriptor::new("projection_ack", ScopeKind::Global, None)
        }
        R::ProjectionUnsubscribe { .. } => {
            OperationDescriptor::new("projection_unsubscribe", ScopeKind::Global, None)
        }
        R::ProjectionSnapshotGet { .. } => OperationDescriptor::new(
            "projection_snapshot_get",
            ScopeKind::Opaque,
            Some(Capability::ProjectObserve),
        ),
        R::ProjectionArtifactRead { .. } => OperationDescriptor::new(
            "projection_artifact_read",
            ScopeKind::DirectProject,
            Some(Capability::ProjectObserve),
        ),
        R::ProjectionArtifactList { .. } => OperationDescriptor::new(
            "projection_artifact_list",
            ScopeKind::DirectProject,
            Some(Capability::ProjectObserve),
        ),
        R::ToolProgramList { .. } => OperationDescriptor::new(
            "tool_program_list",
            ScopeKind::ViaSession,
            Some(Capability::SessionRead),
        ),
        R::ToolProgramInspect { .. } => OperationDescriptor::new(
            "tool_program_inspect",
            ScopeKind::Opaque,
            Some(Capability::SessionRead),
        ),
        R::ToolProgramCallPage { .. } => OperationDescriptor::new(
            "tool_program_call_page",
            ScopeKind::Opaque,
            Some(Capability::SessionRead),
        ),
        R::ToolProgramNotificationReinject { .. } => OperationDescriptor::new(
            "tool_program_notification_reinject",
            ScopeKind::ViaSession,
            Some(Capability::AgentDelegate),
        ),
        R::ToolProgramRecoveryDebugInspect { .. } => OperationDescriptor::new(
            "tool_program_recovery_debug_inspect",
            ScopeKind::ViaSession,
            Some(Capability::AgentDelegate),
        ),
        R::EditCheckpointList { .. } => OperationDescriptor::new(
            "edit_checkpoint_list",
            ScopeKind::ViaSession,
            Some(Capability::FileRead),
        ),
        R::EditCheckpointGet { .. } => OperationDescriptor::new(
            "edit_checkpoint_get",
            ScopeKind::Opaque,
            Some(Capability::FileRead),
        ),
        R::EditCheckpointUndo { .. } => OperationDescriptor::new(
            "edit_checkpoint_undo",
            ScopeKind::ViaSession,
            Some(Capability::FileModify),
        ),
        R::EditCheckpointUndoLatest { .. } => OperationDescriptor::new(
            "edit_checkpoint_undo_latest",
            ScopeKind::ViaSession,
            Some(Capability::FileModify),
        ),
        R::EditCheckpointReapply { .. } => OperationDescriptor::new(
            "edit_checkpoint_reapply",
            ScopeKind::ViaSession,
            Some(Capability::FileModify),
        ),
        R::EditCheckpointReapplyLatest { .. } => OperationDescriptor::new(
            "edit_checkpoint_reapply_latest",
            ScopeKind::ViaSession,
            Some(Capability::FileModify),
        ),
        R::LspPreviewApply { .. } => OperationDescriptor::new(
            "lsp_preview_apply",
            ScopeKind::ViaSession,
            Some(Capability::FileModify),
        ),
        R::AuditQuery { .. } => OperationDescriptor::new(
            "audit_query",
            ScopeKind::DirectProject,
            Some(Capability::AuditRead),
        ),
        R::AuditExport { .. } => OperationDescriptor::new(
            "audit_export",
            ScopeKind::DirectProject,
            Some(Capability::AuditRead),
        ),
        R::AuditCapabilities => {
            OperationDescriptor::new("audit_capabilities", ScopeKind::Global, None)
        }
        // ── Interactive Process Sessions M002: Bounded Attach/Resume ──
        //
        // Transport-level scope is intentionally `Global` with no semantic
        // capability: per-process authority is enforced by the daemon's
        // attachment registry (ownership derived from the trusted
        // transport `client_id`, never from payload fields), and
        // terminate/remove additionally require the local-owner transport
        // (or a future semantic capability plugged through the same
        // authority context without a wire change). The daemon-wide
        // authorization gate therefore admits any authenticated caller;
        // the attachment seam fails closed per process.
        R::InteractiveProcessCapabilities => {
            OperationDescriptor::new("interactive_process_capabilities", ScopeKind::Global, None)
        }
        R::InteractiveProcessCreate { .. } => {
            OperationDescriptor::new("interactive_process_create", ScopeKind::Global, None)
        }
        R::InteractiveProcessList { .. } => {
            OperationDescriptor::new("interactive_process_list", ScopeKind::Global, None)
        }
        R::InteractiveProcessAttach { .. } => {
            OperationDescriptor::new("interactive_process_attach", ScopeKind::Global, None)
        }
        R::InteractiveProcessDetach { .. } => {
            OperationDescriptor::new("interactive_process_detach", ScopeKind::Global, None)
        }
        R::InteractiveProcessInput { .. } => {
            OperationDescriptor::new("interactive_process_input", ScopeKind::Global, None)
        }
        R::InteractiveProcessResize { .. } => {
            OperationDescriptor::new("interactive_process_resize", ScopeKind::Global, None)
        }
        R::InteractiveProcessResume { .. } => {
            OperationDescriptor::new("interactive_process_resume", ScopeKind::Global, None)
        }
        R::InteractiveProcessTerminate { .. } => {
            OperationDescriptor::new("interactive_process_terminate", ScopeKind::Global, None)
        }
        R::InteractiveProcessRemove { .. } => {
            OperationDescriptor::new("interactive_process_remove", ScopeKind::Global, None)
        }
        // ── Presence and Observation M001: Project-Scoped Presence Leases ──
        //
        // Heartbeat writes only the caller's own contribution (principal
        // and client come from transport authority) and snapshot reads
        // only the named project. Both require `project.observe` so
        // unauthorized projects are indistinguishable from absent via
        // the `project_not_found` denial shape.
        R::PresenceCapabilities => {
            OperationDescriptor::new("presence_capabilities", ScopeKind::Global, None)
        }
        R::PresenceHeartbeat { .. } => OperationDescriptor::new(
            "presence_heartbeat",
            ScopeKind::DirectProject,
            Some(Capability::ProjectObserve),
        ),
        R::PresenceSnapshotGet { .. } => OperationDescriptor::new(
            "presence_snapshot_get",
            ScopeKind::DirectProject,
            Some(Capability::ProjectObserve),
        ),
        // ── Project Collaboration M001: Project Channel/Message Protocol ──
        //
        // Every chat operation requires `project.chat` on the owning
        // project. Channel-scoped requests carry only the channel
        // locator; the daemon resolves the owning project server-side
        // through the durable channel row (unknown channels fail
        // closed). Reads and writes share the same capability so
        // unauthorized callers cannot enumerate channels, read
        // messages, or infer existence.
        R::ChatCapabilities => {
            OperationDescriptor::new("chat_capabilities", ScopeKind::Global, None)
        }
        R::ChatChannelEnsure { .. } => OperationDescriptor::new(
            "chat_channel_ensure",
            ScopeKind::DirectProject,
            Some(Capability::ProjectChat),
        ),
        R::ChatChannelList { .. } => OperationDescriptor::new(
            "chat_channel_list",
            ScopeKind::DirectProject,
            Some(Capability::ProjectChat),
        ),
        R::ChatHistory { .. } => OperationDescriptor::new(
            "chat_history",
            ScopeKind::DirectProject,
            Some(Capability::ProjectChat),
        ),
        R::ChatSend { .. } => OperationDescriptor::new(
            "chat_send",
            ScopeKind::DirectProject,
            Some(Capability::ProjectChat),
        ),
        R::ChatEdit { .. } => OperationDescriptor::new(
            "chat_edit",
            ScopeKind::DirectProject,
            Some(Capability::ProjectChat),
        ),
        R::ChatRedact { .. } => OperationDescriptor::new(
            "chat_redact",
            ScopeKind::DirectProject,
            Some(Capability::ProjectChat),
        ),
        R::ChatReadSet { .. } => OperationDescriptor::new(
            "chat_read_set",
            ScopeKind::DirectProject,
            Some(Capability::ProjectChat),
        ),
        R::ChatReadGet { .. } => OperationDescriptor::new(
            "chat_read_get",
            ScopeKind::DirectProject,
            Some(Capability::ProjectChat),
        ),
        R::ChatComposingSet { .. } => OperationDescriptor::new(
            "chat_composing_set",
            ScopeKind::DirectProject,
            Some(Capability::ProjectChat),
        ),
        R::ChatComposingList { .. } => OperationDescriptor::new(
            "chat_composing_list",
            ScopeKind::DirectProject,
            Some(Capability::ProjectChat),
        ),
        R::ChatSync { .. } => OperationDescriptor::new(
            "chat_sync",
            ScopeKind::DirectProject,
            Some(Capability::ProjectChat),
        ),
        // ── Project Collaboration M003: Separately Authorized Structured Chat Actions ──
        //
        // The gate enforces `project.chat` on the owning project (same
        // as M001 chat). The daemon handler additionally checks the
        // ordinary semantic capability for the action kind before
        // creating anything (`agent.delegate` / `job.submit` /
        // `session.read`). Reads share the chat gate so unauthorized
        // callers cannot enumerate actions.
        R::ChatActionSubmit { .. } => OperationDescriptor::new(
            "chat_action_submit",
            ScopeKind::DirectProject,
            Some(Capability::ProjectChat),
        ),
        R::ChatActionGet { .. } => OperationDescriptor::new(
            "chat_action_get",
            ScopeKind::DirectProject,
            Some(Capability::ProjectChat),
        ),
        R::ChatActionList { .. } => OperationDescriptor::new(
            "chat_action_list",
            ScopeKind::DirectProject,
            Some(Capability::ProjectChat),
        ),
        // ── Team Collaboration Corrective M002: Chat Access Policy Admin ──
        //
        // Policy inspection/mutation is membership/collaboration
        // administration, not ordinary message sending. Reads and
        // writes both require `member.manage` on the owning project
        // (Owner only in the current role model). Channel-scoped
        // policy requests resolve the project server-side; unknown or
        // foreign channels fail closed before any policy row is read.
        R::ChatPolicyGet { .. } => OperationDescriptor::new(
            "chat_policy_get",
            ScopeKind::DirectProject,
            Some(Capability::MemberManage),
        ),
        R::ChatPolicyList { .. } => OperationDescriptor::new(
            "chat_policy_list",
            ScopeKind::DirectProject,
            Some(Capability::MemberManage),
        ),
        R::ChatProjectPolicySet { .. } => OperationDescriptor::new(
            "chat_project_policy_set",
            ScopeKind::DirectProject,
            Some(Capability::MemberManage),
        ),
        R::ChatChannelPolicySet { .. } => OperationDescriptor::new(
            "chat_channel_policy_set",
            ScopeKind::DirectProject,
            Some(Capability::MemberManage),
        ),
        // ── Team Collaboration Corrective M003: Membership/Device-Token Admin ──
        //
        // Membership administration is project teamwork: reads and writes
        // both require `member.manage` on the owning project (Owner only
        // in the current role model), so a project Owner can manage only
        // their own project. Principal/token operations carry no project
        // locator (`Opaque + project.configure`): ordinary team
        // principals fail closed with `MissingScope` and only the
        // LocalOwner broad policy passes, so a project Owner can never
        // mint deployment-wide device credentials via a shared project.
        // `TeamCapabilities` is a global version probe with no data.
        R::TeamCapabilities => {
            OperationDescriptor::new("team_capabilities", ScopeKind::Global, None)
        }
        R::TeamMembershipList { .. } => OperationDescriptor::new(
            "team_membership_list",
            ScopeKind::DirectProject,
            Some(Capability::MemberManage),
        ),
        R::TeamMembershipAdd { .. } => OperationDescriptor::new(
            "team_membership_add",
            ScopeKind::DirectProject,
            Some(Capability::MemberManage),
        ),
        R::TeamMembershipUpdate { .. } => OperationDescriptor::new(
            "team_membership_update",
            ScopeKind::DirectProject,
            Some(Capability::MemberManage),
        ),
        R::TeamMembershipRevoke { .. } => OperationDescriptor::new(
            "team_membership_revoke",
            ScopeKind::DirectProject,
            Some(Capability::MemberManage),
        ),
        R::TeamPrincipalList { .. } => OperationDescriptor::new(
            "team_principal_list",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::TeamPrincipalCreate { .. } => OperationDescriptor::new(
            "team_principal_create",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::TeamPrincipalStatusSet { .. } => OperationDescriptor::new(
            "team_principal_status_set",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::TeamTokenList { .. } => OperationDescriptor::new(
            "team_token_list",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::TeamTokenCreate { .. } => OperationDescriptor::new(
            "team_token_create",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        R::TeamTokenRevoke { .. } => OperationDescriptor::new(
            "team_token_revoke",
            ScopeKind::Opaque,
            Some(Capability::ProjectConfigure),
        ),
        // ── Team Collaboration Corrective M004: Shared-Session Controller Lease ──
        //
        // Turn-scoped controller lease from ADR-0007. All operations are
        // session-scoped (`ViaSession`): `SessionControlGet` inspects the
        // lease plus inert requests (`session.read`); request/transfer/
        // release narrow `agent.invoke` in-flight control to the lease
        // holder (enforced in the handler after this gate, never widening
        // it); forced takeover requires Maintainer/Owner-equivalent
        // `project.configure` and a bounded reason, and is audited.
        R::SessionControlGet { .. } => OperationDescriptor::new(
            "session_control_get",
            ScopeKind::ViaSession,
            Some(Capability::SessionRead),
        ),
        R::SessionControlRequest { .. } => OperationDescriptor::new(
            "session_control_request",
            ScopeKind::ViaSession,
            Some(Capability::AgentInvoke),
        ),
        R::SessionControlTransfer { .. } => OperationDescriptor::new(
            "session_control_transfer",
            ScopeKind::ViaSession,
            Some(Capability::AgentInvoke),
        ),
        R::SessionControlRelease { .. } => OperationDescriptor::new(
            "session_control_release",
            ScopeKind::ViaSession,
            Some(Capability::AgentInvoke),
        ),
        R::SessionControlTakeover { .. } => OperationDescriptor::new(
            "session_control_takeover",
            ScopeKind::ViaSession,
            Some(Capability::ProjectConfigure),
        ),
        // ── Execution Reliability M003: Approval / Sandbox / Policy ──
        //
        // Principal-scoped daemon-owned preferences. Transport-level scope
        // is `Global` with no semantic capability: the daemon binds the
        // principal server-side from transport authority (payloads carry
        // no identity), and mode changes never override explicit deny or
        // project/admin ceilings (enforced in the handler/router, not by
        // widening this gate).
        R::ApprovalPreferenceGet => {
            OperationDescriptor::new("approval_preference_get", ScopeKind::Global, None)
        }
        R::ApprovalModeSet { .. } => {
            OperationDescriptor::new("approval_mode_set", ScopeKind::Global, None)
        }
        R::SandboxProfileSet { .. } => {
            OperationDescriptor::new("sandbox_profile_set", ScopeKind::Global, None)
        }
        R::RuntimePolicySet { .. } => {
            OperationDescriptor::new("runtime_policy_set", ScopeKind::Global, None)
        }
        R::TaskModelPreferenceSet { .. } => {
            OperationDescriptor::new("task_model_preference_set", ScopeKind::Global, None)
        }
        R::ExecutionPolicyGet { .. } => {
            OperationDescriptor::new("execution_policy_get", ScopeKind::Global, None)
        }
        // ── Project Work Orders M001: Domain, Storage, and Protocol ──
        //
        // Every work-order operation is project-scoped from its first
        // version. `project_id` requests resolve directly; ID-only
        // requests resolve the owning project server-side through the
        // durable work-order/occurrence/lane row (unknown ids fail
        // closed), so no broad local-owner-only opaque operation is the
        // primary team-facing surface. Creation requires the authority
        // sufficient to create the future session (`session.create`);
        // reads use `session.read`; cancel/update/reorder use the
        // explicit `session.create` mutation semantic.
        R::WorkOrderCapabilities => {
            OperationDescriptor::new("work_order_capabilities", ScopeKind::Global, None)
        }
        R::WorkOrderCreate { .. } => OperationDescriptor::new(
            "work_order_create",
            ScopeKind::DirectProject,
            Some(Capability::SessionCreate),
        ),
        R::WorkOrderBatchCreate { .. } => OperationDescriptor::new(
            "work_order_batch_create",
            ScopeKind::DirectProject,
            Some(Capability::SessionCreate),
        ),
        R::WorkOrderList { .. } => OperationDescriptor::new(
            "work_order_list",
            ScopeKind::DirectProject,
            Some(Capability::SessionRead),
        ),
        R::WorkOrderGet { .. } => OperationDescriptor::new(
            "work_order_get",
            ScopeKind::DirectProject,
            Some(Capability::SessionRead),
        ),
        R::WorkOrderUpdate { .. } => OperationDescriptor::new(
            "work_order_update",
            ScopeKind::DirectProject,
            Some(Capability::SessionCreate),
        ),
        R::WorkOrderCancel { .. } => OperationDescriptor::new(
            "work_order_cancel",
            ScopeKind::DirectProject,
            Some(Capability::SessionCreate),
        ),
        R::WorkOrderPause { .. } => OperationDescriptor::new(
            "work_order_pause",
            ScopeKind::DirectProject,
            Some(Capability::SessionCreate),
        ),
        R::WorkOrderResume { .. } => OperationDescriptor::new(
            "work_order_resume",
            ScopeKind::DirectProject,
            Some(Capability::SessionCreate),
        ),
        R::WorkOrderLaneCreate { .. } => OperationDescriptor::new(
            "work_order_lane_create",
            ScopeKind::DirectProject,
            Some(Capability::SessionCreate),
        ),
        R::WorkOrderLaneGet { .. } => OperationDescriptor::new(
            "work_order_lane_get",
            ScopeKind::DirectProject,
            Some(Capability::SessionRead),
        ),
        R::WorkOrderLaneList { .. } => OperationDescriptor::new(
            "work_order_lane_list",
            ScopeKind::DirectProject,
            Some(Capability::SessionRead),
        ),
        R::WorkOrderLaneReorder { .. } => OperationDescriptor::new(
            "work_order_lane_reorder",
            ScopeKind::DirectProject,
            Some(Capability::SessionCreate),
        ),
        R::WorkOrderLaneAttach { .. } => OperationDescriptor::new(
            "work_order_lane_attach",
            ScopeKind::DirectProject,
            Some(Capability::SessionCreate),
        ),
        R::WorkOrderOccurrenceGet { .. } => OperationDescriptor::new(
            "work_order_occurrence_get",
            ScopeKind::DirectProject,
            Some(Capability::SessionRead),
        ),
        R::WorkOrderOccurrenceList { .. } => OperationDescriptor::new(
            "work_order_occurrence_list",
            ScopeKind::DirectProject,
            Some(Capability::SessionRead),
        ),
        R::WorkOrderSummary { .. } => OperationDescriptor::new(
            "work_order_summary",
            ScopeKind::DirectProject,
            Some(Capability::SessionRead),
        ),
        // ── Project Work Orders M005: external task triggers ─────────
        //
        // Trigger *management* is ordinary principal-authorized project
        // mutation/reads: creation requires the authority sufficient to
        // modify/schedule the work order (`session.create`); metadata
        // reads use `session.read`; revocation is monotonic. The *fire*
        // operation is deliberately NOT a Core operation: the bearer
        // fires through the narrow HTTP POST route only and can never
        // authorize general Core APIs.
        R::WorkOrderTriggerCreate { .. } => OperationDescriptor::new(
            "work_order_trigger_create",
            ScopeKind::DirectProject,
            Some(Capability::SessionCreate),
        ),
        R::WorkOrderTriggerList { .. } => OperationDescriptor::new(
            "work_order_trigger_list",
            ScopeKind::DirectProject,
            Some(Capability::SessionRead),
        ),
        R::WorkOrderTriggerGet { .. } => OperationDescriptor::new(
            "work_order_trigger_get",
            ScopeKind::DirectProject,
            Some(Capability::SessionRead),
        ),
        R::WorkOrderTriggerRevoke { .. } => OperationDescriptor::new(
            "work_order_trigger_revoke",
            ScopeKind::DirectProject,
            Some(Capability::SessionCreate),
        ),
    }
}

/// Executable operation-to-capability matrix.
///
/// Every row is `(operation, scope_kind, capability)` with the capability
/// as its wire name or `"none"`. The matrix is derived from
/// [`operation_descriptor`] over a fixed representative request per
/// operation so documentation, diagnostics, and the static guard cannot
/// drift from the implementation. It is deliberately not a second
/// hand-maintained table.
pub fn operation_capability_matrix() -> Vec<(String, String, String)> {
    representative_requests()
        .iter()
        .map(|request| {
            let descriptor = operation_descriptor(request);
            (
                descriptor.operation.to_owned(),
                descriptor.scope_kind.as_str().to_owned(),
                descriptor.capability_name().to_owned(),
            )
        })
        .collect()
}

/// One representative request per `CoreRequest` operation, used only to
/// derive [`operation_capability_matrix`] and table-coverage tests.
/// Payload values are inert placeholders; only the variant matters.
#[allow(clippy::too_many_lines)]
pub fn representative_requests() -> Vec<codegg_protocol::core::CoreRequest> {
    use codegg_protocol::core::CoreRequest as R;
    vec![
        R::Initialize,
        R::AssetRefresh {
            request: codegg_protocol::core::AssetRefreshRequestDto {
                scope: codegg_protocol::core::AssetRefreshScopeDto {
                    project_id: String::new(),
                    workspace_id: String::new(),
                },
                reason: codegg_protocol::core::AssetRefreshReasonDto::Manual,
                session_id: None,
            },
        },
        R::AssetRefreshStatus {
            scope: codegg_protocol::core::AssetRefreshScopeDto {
                project_id: String::new(),
                workspace_id: String::new(),
            },
        },
        R::AssetRefreshCapabilities,
        R::EggpoolConnectionCreate {
            request: dummy_eggpool_create(),
        },
        R::ProviderConnectionCreate {
            request: dummy_provider_connection_create(),
        },
        R::ProviderSetupList,
        R::EggpoolConnectionCancel {
            operation_id: String::new(),
        },
        R::EggpoolConnectionStatus {
            operation_id: String::new(),
        },
        R::ProviderConnectionList,
        R::ProviderConnectionModels {
            connection_id: String::new(),
        },
        R::ConnectionRotateBegin {
            request_id: String::new(),
            connection_id: String::new(),
            expected_revision: 0,
            change: dummy_rotate_change(),
            secret: dummy_secret_ref(),
        },
        R::ConnectionRotateSecretStage {
            request_id: String::new(),
            secret: dummy_secret_input(),
        },
        R::ConnectionRotateCancel {
            request_id: String::new(),
        },
        R::ConnectionRotateStatus {
            request_id: String::new(),
        },
        R::ConnectionRefreshBegin {
            connection_id: String::new(),
            expected_revision: 0,
        },
        R::ConnectionRefreshCancel {
            operation_id: String::new(),
        },
        R::ConnectionRefreshStatus {
            operation_id: String::new(),
        },
        R::ConnectionGet {
            connection_id: String::new(),
        },
        R::ConnectionListDetail,
        R::ConnectionEnable {
            connection_id: String::new(),
            expected_revision: 0,
            require_probe: false,
        },
        R::ConnectionDisable {
            connection_id: String::new(),
            expected_revision: 0,
        },
        R::ConnectionDelete {
            connection_id: String::new(),
            expected_revision: 0,
        },
        R::ConnectionRestore {
            connection_id: String::new(),
            expected_revision: 0,
        },
        R::ConnectionPurge {
            connection_id: String::new(),
            expected_revision: 0,
        },
        R::Subscribe { session_id: None },
        R::Resume {
            session_id: None,
            from_event_seq: 0,
        },
        R::SessionList {
            project_id: String::new(),
            show_archived: false,
            limit: 0,
        },
        R::SessionCreate {
            directory: String::new(),
            title: None,
            project_id: None,
            workspace_id: None,
        },
        R::SessionAttach {
            session_id: String::new(),
        },
        R::SessionLoad {
            session_id: String::new(),
        },
        R::SessionMessagesLoad {
            session_id: String::new(),
        },
        R::SessionMessageCounts {
            session_ids: Vec::new(),
        },
        R::SessionFork {
            session_id: String::new(),
        },
        R::SessionDelete {
            session_id: String::new(),
            permanent: false,
        },
        R::SessionArchive {
            session_id: String::new(),
            unarchive: false,
        },
        R::SessionRestore {
            session_id: String::new(),
        },
        R::SessionShare {
            session_id: String::new(),
        },
        R::SessionUnshare {
            session_id: String::new(),
        },
        R::SessionRename {
            session_id: String::new(),
            new_title: String::new(),
        },
        R::SessionExport {
            session_id: String::new(),
        },
        R::SessionImportData {
            data: serde_json::Value::Null,
        },
        R::SessionCreateFromTemplate {
            template: dummy_session_template(),
            project_id: None,
            directory: String::new(),
            workspace_id: None,
        },
        R::TurnSubmit {
            session_id: String::new(),
            text: String::new(),
            plan_mode: false,
            model: String::new(),
            agents: Vec::new(),
            current_agent_idx: 0,
            messages: Vec::new(),
        },
        R::TurnCancel {
            session_id: String::new(),
            turn_id: String::new(),
        },
        R::TurnSteer {
            session_id: String::new(),
            turn_id: String::new(),
            text: String::new(),
        },
        R::AgentSelect {
            session_id: String::new(),
            agent_name: String::new(),
        },
        R::ModelSelect {
            session_id: String::new(),
            model: String::new(),
        },
        R::SessionSelectionGet {
            session_id: String::new(),
        },
        R::SessionSelectionList {
            session_id: String::new(),
        },
        R::SessionSelectionUpdate {
            request: Box::new(dummy_selection_update()),
        },
        R::SessionSelectionModels {
            session_id: String::new(),
            connection_id: String::new(),
        },
        R::SessionLifecycleGet {
            session_id: String::new(),
        },
        R::ModelsRefresh,
        R::PermissionRespond {
            id: String::new(),
            choice: String::new(),
        },
        R::QuestionRespond {
            id: String::new(),
            answers: serde_json::Value::Null,
        },
        R::MemorySearch {
            query: String::new(),
        },
        R::MemoryList {
            namespace: String::new(),
        },
        R::MemoryRemember {
            text: String::new(),
            namespace: None,
        },
        R::MemoryForget { id: String::new() },
        R::TaskList,
        R::TaskSchedule {
            session_id: String::new(),
            interval_secs: 0,
            message: String::new(),
        },
        R::TaskDelete { id: 0 },
        R::WorktreeList {
            project_dir: String::new(),
        },
        R::ManagedWorktreeGet {
            worktree_id: String::new(),
        },
        R::ManagedWorktreeList {
            workspace_id: None,
            repository_id: None,
            run_id: None,
            include_removed: false,
        },
        R::ManagedWorktreeCleanup {
            worktree_id: String::new(),
            lease_generation: 0,
        },
        R::ManagedWorktreeArchive {
            worktree_id: String::new(),
            lease_generation: 0,
        },
        R::WorkspaceRegister {
            root: String::new(),
        },
        R::WorkspaceList {
            include_archived: false,
        },
        R::WorkspaceArchive {
            workspace_id: String::new(),
        },
        R::WorkspaceSnapshotRequest {
            workspace_id: String::new(),
        },
        R::WorkspaceServicesSnapshot,
        R::WorkspaceConfigReload {
            workspace_id: String::new(),
        },
        R::ProjectList {
            include_archived: false,
            limit: 0,
        },
        R::ProjectGet {
            project_id: String::new(),
        },
        R::ProjectRegister {
            request: dummy_project_register(),
        },
        R::ProjectArchive {
            project_id: String::new(),
        },
        R::ProjectRestore {
            project_id: String::new(),
        },
        R::ProjectHealth {
            project_id: String::new(),
            workspace_id: String::new(),
        },
        R::ProjectCatalogCapabilities,
        R::WorkspaceDashboard {
            cursor: None,
            limit: None,
            include_archived: false,
        },
        R::RunList {
            workspace_id: String::new(),
            query: dummy_run_query(),
        },
        R::RunGet {
            workspace_id: String::new(),
            run_id: String::new(),
        },
        R::RunArtifactRead {
            workspace_id: String::new(),
            artifact_id: String::new(),
            start: 0,
            end: 0,
        },
        R::RunRerun {
            workspace_id: String::new(),
            parent_run_id: String::new(),
            session_id: None,
        },
        R::GoalSet {
            session_id: String::new(),
            project_id: String::new(),
            objective: String::new(),
        },
        R::GoalFromFile {
            session_id: String::new(),
            project_id: String::new(),
            path: String::new(),
        },
        R::GoalShow {
            session_id: String::new(),
        },
        R::GoalPause {
            session_id: String::new(),
        },
        R::GoalResume {
            session_id: String::new(),
        },
        R::GoalClear {
            session_id: String::new(),
        },
        R::GoalDone {
            session_id: String::new(),
        },
        R::GoalCheckpoint {
            session_id: String::new(),
            project_id: String::new(),
        },
        R::TodoList {
            session_id: String::new(),
        },
        R::ActiveGoalLoad {
            session_id: String::new(),
        },
        R::GoalSetBudget {
            session_id: String::new(),
            max_turns: None,
            max_model_tokens: None,
            max_tool_calls: None,
            max_wallclock_secs: None,
        },
        R::SnapshotSession {
            session_id: String::new(),
        },
        R::SnapshotWorkspace {
            project_dir: String::new(),
        },
        R::SnapshotModels,
        R::SnapshotDaemon,
        R::NotificationSpeak {
            text: String::new(),
            kind: None,
            priority: None,
            session_id: None,
        },
        R::NotificationStop,
        R::JobSubmit {
            spec: dummy_job_submit(),
        },
        R::SchedulerSnapshot,
        R::JobWait {
            job_id: String::new(),
            timeout_ms: None,
        },
        R::JobGet {
            job_id: String::new(),
        },
        R::JobList {
            query: dummy_job_query(),
        },
        R::JobCancel {
            job_id: String::new(),
            reason: None,
        },
        R::JobRetry {
            job_id: String::new(),
        },
        R::JobAttempts {
            job_id: String::new(),
        },
        R::ScheduleCreate {
            spec: dummy_schedule_create(),
        },
        R::ScheduleList {
            workspace_id: None,
            include_archived: false,
        },
        R::ScheduleGet {
            schedule_id: String::new(),
        },
        R::SchedulePause {
            schedule_id: String::new(),
        },
        R::ScheduleResume {
            schedule_id: String::new(),
        },
        R::ScheduleDelete {
            schedule_id: String::new(),
        },
        R::JobRecoveryReport,
        R::ProjectionCapabilities,
        R::ProjectionSubscribe {
            request: dummy_projection_subscribe(),
        },
        R::ProjectionResume {
            cursor: dummy_projection_cursor(),
            include_snapshot_if_resync: false,
        },
        R::ProjectionAck {
            ack: dummy_projection_ack(),
        },
        R::ProjectionUnsubscribe {
            subscription_id: dummy_projection_subscription_id(),
        },
        R::ProjectionSnapshotGet {
            scope: dummy_projection_stream_kind(),
            scope_id: String::new(),
        },
        R::ProjectionArtifactRead {
            request: dummy_projection_artifact_read(),
            project_id: String::new(),
            context_correlation_id: None,
        },
        R::ProjectionArtifactList {
            project_id: String::new(),
        },
        R::ToolProgramList {
            session_id: String::new(),
            state_filter: None,
        },
        R::ToolProgramInspect {
            program_id: String::new(),
        },
        R::ToolProgramCallPage {
            program_id: String::new(),
            offset: 0,
        },
        R::ToolProgramNotificationReinject {
            session_id: String::new(),
        },
        R::ToolProgramRecoveryDebugInspect {
            session_id: String::new(),
            notification_id: String::new(),
        },
        R::EditCheckpointList {
            workspace_id: String::new(),
            session_id: String::new(),
            limit: None,
        },
        R::EditCheckpointGet {
            checkpoint_id: String::new(),
            workspace_id: String::new(),
        },
        R::EditCheckpointUndo {
            checkpoint_id: String::new(),
            workspace_id: String::new(),
            session_id: String::new(),
        },
        R::EditCheckpointUndoLatest {
            workspace_id: String::new(),
            session_id: String::new(),
        },
        R::EditCheckpointReapply {
            checkpoint_id: String::new(),
            workspace_id: String::new(),
            session_id: String::new(),
        },
        R::EditCheckpointReapplyLatest {
            workspace_id: String::new(),
            session_id: String::new(),
        },
        R::LspPreviewApply {
            request: dummy_lsp_preview_apply(),
        },
        R::AuditQuery {
            query: codegg_protocol::core::AuditQueryRequestDto {
                project_id: String::new(),
                action_filter: None,
                principal_filter: None,
                from_seq: None,
                limit: None,
            },
        },
        R::AuditExport {
            request: codegg_protocol::core::AuditExportRequestDto {
                project_id: String::new(),
                action_filter: None,
                principal_filter: None,
                from_seq: None,
                limit: None,
            },
        },
        R::AuditCapabilities,
        R::InteractiveProcessCapabilities,
        R::InteractiveProcessCreate {
            request: codegg_protocol::interactive_process::InteractiveProcessCreateRequest {
                workspace_id: String::new(),
                argv: vec![String::new()],
                cwd: None,
                env_overrides: Vec::new(),
                cols: None,
                rows: None,
                scrollback_bytes: None,
            },
        },
        R::InteractiveProcessList {
            workspace_id: None,
            limit: None,
        },
        R::InteractiveProcessAttach {
            handle: String::new(),
            from_seq: None,
            max_bytes: None,
        },
        R::InteractiveProcessDetach {
            attachment_id: String::new(),
        },
        R::InteractiveProcessInput {
            attachment_id: String::new(),
            data_b64: String::new(),
        },
        R::InteractiveProcessResize {
            attachment_id: String::new(),
            cols: 80,
            rows: 24,
        },
        R::InteractiveProcessResume {
            attachment_id: String::new(),
            from_seq: 0,
            max_bytes: None,
        },
        R::InteractiveProcessTerminate {
            attachment_id: String::new(),
        },
        R::InteractiveProcessRemove {
            attachment_id: String::new(),
        },
        R::PresenceCapabilities,
        R::PresenceHeartbeat {
            request: codegg_protocol::core::PresenceHeartbeatRequestDto {
                project_id: String::new(),
                session_id: None,
                activity: codegg_protocol::core::PresenceActivityDto::Active,
                connection_generation: 0,
            },
        },
        R::PresenceSnapshotGet {
            project_id: String::new(),
        },
        R::ChatCapabilities,
        R::ChatChannelEnsure {
            project_id: String::new(),
            name: None,
        },
        R::ChatChannelList {
            project_id: String::new(),
            limit: None,
        },
        R::ChatHistory {
            channel_id: String::new(),
            from_seq: None,
            limit: None,
        },
        R::ChatSend {
            channel_id: String::new(),
            body: String::new(),
            reply_to: None,
            thread_root: None,
            mentions: Vec::new(),
            references: Vec::new(),
            idempotency_key: None,
        },
        R::ChatEdit {
            channel_id: String::new(),
            message_id: String::new(),
            expected_revision: 0,
            new_body: String::new(),
        },
        R::ChatRedact {
            channel_id: String::new(),
            message_id: String::new(),
            expected_revision: None,
            reason: None,
        },
        R::ChatReadSet {
            channel_id: String::new(),
            last_read_seq: 0,
        },
        R::ChatReadGet {
            channel_id: String::new(),
        },
        R::ChatComposingSet {
            channel_id: String::new(),
            composing: false,
        },
        R::ChatComposingList {
            channel_id: String::new(),
        },
        R::ChatSync {
            channel_id: String::new(),
            from_seq: 0,
            limit: None,
        },
        R::ChatActionSubmit {
            channel_id: String::new(),
            message_id: String::new(),
            action: codegg_protocol::core::ChatActionSubmitDto::JobReference {
                job_id: String::new(),
                title: None,
            },
            idempotency_key: String::new(),
        },
        R::ChatActionGet {
            channel_id: String::new(),
            action_id: String::new(),
        },
        R::ChatActionList {
            channel_id: String::new(),
            message_id: None,
            limit: None,
        },
        R::ChatPolicyGet {
            project_id: String::new(),
            channel_id: None,
        },
        R::ChatPolicyList {
            project_id: String::new(),
        },
        R::ChatProjectPolicySet {
            project_id: String::new(),
            principal_id: String::new(),
            decision: None,
            expected_revision: None,
        },
        R::ChatChannelPolicySet {
            channel_id: String::new(),
            mode: None,
            principal_id: None,
            decision: None,
            expected_revision: None,
        },
        R::TeamCapabilities,
        R::TeamMembershipList {
            project_id: String::new(),
            limit: None,
        },
        R::TeamMembershipAdd {
            project_id: String::new(),
            principal_id: String::new(),
            role: String::new(),
        },
        R::TeamMembershipUpdate {
            project_id: String::new(),
            principal_id: String::new(),
            expected_revision: 0,
            role: None,
            state: None,
        },
        R::TeamMembershipRevoke {
            project_id: String::new(),
            principal_id: String::new(),
            expected_revision: 0,
        },
        R::TeamPrincipalList { limit: None },
        R::TeamPrincipalCreate {
            kind: None,
            display_name: String::new(),
        },
        R::TeamPrincipalStatusSet {
            principal_id: String::new(),
            status: String::new(),
            expected_revision: 0,
        },
        R::TeamTokenList {
            principal_id: String::new(),
        },
        R::TeamTokenCreate {
            principal_id: String::new(),
            label: String::new(),
            expires_at_ms: None,
            idempotency_key: None,
        },
        R::TeamTokenRevoke {
            token_id: String::new(),
        },
        R::SessionControlGet {
            session_id: String::new(),
        },
        R::SessionControlRequest {
            session_id: String::new(),
            message: None,
        },
        R::SessionControlTransfer {
            session_id: String::new(),
            recipient_principal: String::new(),
            expected_revision: 0,
            reason: None,
        },
        R::SessionControlRelease {
            session_id: String::new(),
            expected_revision: 0,
        },
        R::SessionControlTakeover {
            session_id: String::new(),
            expected_revision: 0,
            reason: String::new(),
        },
        R::ApprovalPreferenceGet,
        R::ApprovalModeSet {
            approval_mode: String::new(),
            expected_revision: None,
        },
        R::SandboxProfileSet {
            sandbox_profile: String::new(),
            expected_revision: None,
        },
        R::RuntimePolicySet {
            approval_mode: None,
            sandbox_profile: None,
            expected_revision: None,
        },
        R::TaskModelPreferenceSet {
            connection_id: None,
            model_id: None,
            expected_revision: None,
        },
        R::ExecutionPolicyGet { session_id: None },
        R::WorkOrderCapabilities,
        R::WorkOrderCreate {
            request: codegg_protocol::work_order::WorkOrderCreateRequest {
                project_id: String::new(),
                title: None,
                prompt: String::new(),
                requested_model: None,
                requested_approval: None,
                requested_sandbox: None,
                workspace_policy: None,
                gates: Vec::new(),
                gate_join: None,
                repeat_count: None,
                sequence_lane_id: None,
                parent_session_id: None,
                parent_turn_id: None,
                parent_work_order_id: None,
                idempotency_key: None,
            },
        },
        R::WorkOrderBatchCreate {
            request: codegg_protocol::work_order::WorkOrderBatchCreateRequest {
                project_id: String::new(),
                items: Vec::new(),
                sequence_lane_id: None,
                batch_key: None,
            },
        },
        R::WorkOrderList {
            project_id: String::new(),
            state_filter: None,
            cursor: None,
            limit: None,
        },
        R::WorkOrderGet {
            work_order_id: String::new(),
        },
        R::WorkOrderUpdate {
            request: codegg_protocol::work_order::WorkOrderUpdateRequest {
                work_order_id: String::new(),
                expected_revision: 0,
                title: None,
                prompt: None,
                requested_model: None,
                requested_approval: None,
                requested_sandbox: None,
                workspace_policy: None,
                gates: None,
                gate_join: None,
                repeat_count: None,
            },
        },
        R::WorkOrderCancel {
            work_order_id: String::new(),
        },
        R::WorkOrderPause {
            work_order_id: String::new(),
        },
        R::WorkOrderResume {
            work_order_id: String::new(),
        },
        R::WorkOrderLaneCreate {
            request: codegg_protocol::work_order::WorkOrderLaneCreateRequest {
                project_id: String::new(),
                label: None,
                failure_policy: None,
                idempotency_key: None,
            },
        },
        R::WorkOrderLaneGet {
            lane_id: String::new(),
        },
        R::WorkOrderLaneList {
            project_id: String::new(),
            limit: None,
        },
        R::WorkOrderLaneReorder {
            request: codegg_protocol::work_order::WorkOrderLaneReorderRequest {
                lane_id: String::new(),
                expected_revision: 0,
                ordered_work_order_ids: Vec::new(),
            },
        },
        R::WorkOrderLaneAttach {
            request: codegg_protocol::work_order::WorkOrderLaneAttachRequest {
                lane_id: String::new(),
                expected_revision: 0,
                work_order_id: String::new(),
                position: None,
            },
        },
        R::WorkOrderOccurrenceGet {
            occurrence_id: String::new(),
        },
        R::WorkOrderOccurrenceList {
            work_order_id: String::new(),
            limit: None,
        },
        R::WorkOrderSummary {
            project_id: String::new(),
        },
        R::WorkOrderTriggerCreate {
            request: codegg_protocol::work_order::TaskTriggerCreateRequest {
                project_id: String::new(),
                work_order_id: String::new(),
                trigger_ref: None,
                expires_at_ms: None,
                max_fires: None,
                idempotency_key: None,
            },
        },
        R::WorkOrderTriggerList {
            request: codegg_protocol::work_order::TaskTriggerListRequest {
                project_id: String::new(),
                work_order_id: None,
                limit: None,
            },
        },
        R::WorkOrderTriggerGet {
            trigger_id: String::new(),
        },
        R::WorkOrderTriggerRevoke {
            trigger_id: String::new(),
        },
    ]
}

fn dummy_eggpool_create() -> codegg_protocol::provider::CreateEggpoolConnectionRequest {
    codegg_protocol::provider::CreateEggpoolConnectionRequest {
        host: "localhost".to_owned(),
        port: None,
        tls_policy: codegg_protocol::provider::EggpoolTlsPolicy::Required,
        api_key: codegg_protocol::provider::SecretInput::new("placeholder").expect("literal"),
        display_name: None,
        scope: codegg_protocol::provider::EggpoolConnectionScope::Personal {
            owner_id: "placeholder".to_owned(),
        },
        operation_id: None,
    }
}

fn dummy_provider_connection_create() -> codegg_protocol::provider::CreateProviderConnectionRequest
{
    codegg_protocol::provider::CreateProviderConnectionRequest {
        provider_id: "placeholder".to_owned(),
        endpoint: None,
        port: None,
        tls_policy: None,
        credential: codegg_protocol::provider::SecretInput::new("placeholder").expect("literal"),
        credential_kind: codegg_protocol::provider::ProviderCredentialKind::default(),
        display_name: None,
        scope: codegg_protocol::provider::ProviderConnectionScope::Personal {
            owner_id: "placeholder".to_owned(),
        },
        operation_id: None,
    }
}

fn dummy_rotate_change() -> codegg_protocol::provider::ConnectionRotateChange {
    codegg_protocol::provider::ConnectionRotateChange::CredentialOnly
}

fn dummy_secret_ref() -> codegg_protocol::provider::SecretInputRef {
    codegg_protocol::provider::SecretInputRef::new("placeholder-handle").expect("literal")
}

fn dummy_secret_input() -> codegg_protocol::provider::SecretInput {
    codegg_protocol::provider::SecretInput::new("placeholder").expect("literal")
}

fn dummy_session_template() -> codegg_protocol::dto::SessionTemplate {
    codegg_protocol::dto::SessionTemplate::default()
}

fn dummy_selection_update() -> codegg_protocol::provider::UpdateSessionSelectionRequest {
    codegg_protocol::provider::UpdateSessionSelectionRequest {
        session_id: String::new(),
        connection_id: String::new(),
        model_id: String::new(),
        expected_connection_revision: None,
        expected_catalog_revision: None,
    }
}

fn dummy_project_register() -> codegg_protocol::dto::ProjectRegisterRequestDto {
    codegg_protocol::dto::ProjectRegisterRequestDto {
        workspace_id: String::new(),
        display_name: "placeholder".to_owned(),
        description: None,
        tags: Vec::new(),
        repository_id: None,
        source: "placeholder".to_owned(),
    }
}

fn dummy_run_query() -> codegg_protocol::dto::RunQueryDto {
    codegg_protocol::dto::RunQueryDto::default()
}

fn dummy_job_submit() -> codegg_protocol::dto::JobSubmitDto {
    codegg_protocol::dto::JobSubmitDto::default()
}

fn dummy_job_query() -> codegg_protocol::dto::JobQueryDto {
    codegg_protocol::dto::JobQueryDto::default()
}

fn dummy_schedule_create() -> codegg_protocol::dto::ScheduleCreateDto {
    codegg_protocol::dto::ScheduleCreateDto::default()
}

fn dummy_projection_subscribe() -> codegg_protocol::projection::replay::ProjectionSubscriptionRequest
{
    codegg_protocol::projection::replay::ProjectionSubscriptionRequest {
        scope: codegg_protocol::projection::replay::ProjectionStreamKind::Project,
        scope_id: String::new(),
        cursor: None,
        projection_version: 1,
    }
}

fn dummy_projection_cursor() -> codegg_protocol::projection::replay::ProjectionCursor {
    codegg_protocol::projection::replay::ProjectionCursor {
        stream_id: codegg_protocol::projection::replay::ProjectionStreamId(String::new()),
        event_seq: 0,
        projection_version: 1,
    }
}

fn dummy_projection_ack() -> codegg_protocol::projection::replay::ProjectionAck {
    codegg_protocol::projection::replay::ProjectionAck {
        subscription_id: dummy_projection_subscription_id(),
        cursor: dummy_projection_cursor(),
    }
}

fn dummy_projection_subscription_id(
) -> codegg_protocol::projection::replay::ProjectionSubscriptionId {
    codegg_protocol::projection::replay::ProjectionSubscriptionId(String::new())
}

fn dummy_projection_stream_kind() -> codegg_protocol::projection::replay::ProjectionStreamKind {
    codegg_protocol::projection::replay::ProjectionStreamKind::Project
}

fn dummy_projection_artifact_read(
) -> codegg_protocol::projection::replay::ProjectionArtifactReadRequest {
    codegg_protocol::projection::replay::ProjectionArtifactReadRequest {
        handle_id: String::new(),
        start: 0,
        end: None,
        expected_revision: 0,
    }
}

fn dummy_lsp_preview_apply() -> codegg_protocol::lsp::LspPreviewApplyRequestDto {
    codegg_protocol::lsp::LspPreviewApplyRequestDto {
        preview_id: String::new(),
        preview_revision: 0,
        preview_digest: String::new(),
        kind: String::new(),
        title: String::new(),
        provenance: String::new(),
        workspace_id: String::new(),
        session_id: String::new(),
        turn_id: None,
        patches: Vec::new(),
    }
}
