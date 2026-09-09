//! Daemon authorization and originating-principal attribution (M003).
//!
//! M001 owns the durable principal/membership/capability domain in
//! [`crate::team`]. M002 owns transport authentication in
//! [`crate::transport_auth`]. This module owns the M003 decision layer:
//! mapping every native [`CoreRequest`] to a scope plus semantic
//! [`Capability`](crate::team::Capability), evaluating that capability
//! server-side against [`TeamStore`](crate::team::TeamStore) state, and
//! propagating the immutable originating principal into durable
//! attribution records.
//!
//! ## Design notes
//!
//! - Authorization occurs server-side. [`AuthorizationRequest`] is built
//!   only from the transport-bound [`AuthenticatedPrincipal`] plus request
//!   locators (project/session/job ids). Request DTOs supply locators but
//!   never capabilities: there is no constructor that accepts a
//!   caller-supplied principal, role, or capability.
//! - [`ProjectRole`](crate::team::ProjectRole) expansion stays in
//!   [`crate::team`]. Handler code asks for semantic capabilities through
//!   [`operation_descriptor`]; role interpretation never appears at call
//!   sites.
//! - [`LOCAL_OWNER broad policy`](is_local_owner_broad): the personal-local
//!   owner resolves through this same API and receives a broad local
//!   policy. It is an explicit policy composition, not a bypass: the
//!   decision is still constructed, still carries a policy marker, and
//!   still binds a correlation/decision id for attribution.
//! - Requests whose required capability has no resolvable project scope
//!   fail closed for team principals ([`AuthorizationError::MissingScope`]).
//!   Local-owner broad policy is the only path that authorizes without a
//!   project binding.
//! - Project enumeration itself is protected: [`visible_projects`] filters
//!   listings to `project.read` grants, and [`denial_as_not_found`] maps a
//!   single-project denial onto the same `project_not_found` shape as a
//!   genuinely absent project so unauthorized callers cannot infer
//!   existence.
//! - Effective agent/tool authority can only narrow. [`narrow_authority`]
//!   intersects parent and child capability sets; any child capability
//!   outside the parent is an escalation and fails closed.
//! - Pre-M003 durable records carry no canonical principal. They are
//!   attributed explicitly through [`OriginAttribution::legacy_local`],
//!   never by silently fabricating a team identity.
//!
//! ## Transport contract (M004/M005 own persistence of these decisions)
//!
//! Decisions are returned to the daemon boundary for enforcement and
//! attribution capture. The append-only audit store (M004) and its
//! instrumentation (M005) consume [`AuthorizationDecision`] and
//! [`OriginAttribution`]; this module never writes audit events itself.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use thiserror::Error;

use crate::error::StorageError;
use crate::identity::{PrincipalId, ProjectId};
use crate::projection_replay::context::{
    BoundedProjectResolver, ProjectionCapability, ProjectionCapabilitySet,
};
use crate::provider_connections::ProviderScope;
use crate::team::{PrincipalKind, PrincipalStatus, TeamStore, LOCAL_OWNER_PRINCIPAL_ID};
use crate::transport_auth::{AuthMethod, AuthenticatedPrincipal, TransportClass};

/// Re-export so callers do not need a second import for principal kinds.
pub use crate::team::ProjectRole;
/// Re-exported capability vocabulary evaluated by the service.
pub use crate::team::{Capability, CapabilitySet};

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
        R::WorkspaceRegister { .. } => {
            OperationDescriptor::new("workspace_register", ScopeKind::Global, None)
        }
        R::WorkspaceList { .. } => {
            OperationDescriptor::new("workspace_list", ScopeKind::Global, None)
        }
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
        // Registration is the team bootstrap: any active principal may
        // create a project, and the daemon grants the creator Owner.
        R::ProjectRegister { .. } => {
            OperationDescriptor::new("project_register", ScopeKind::Global, None)
        }
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
            ScopeKind::Opaque,
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
fn representative_requests() -> Vec<codegg_protocol::core::CoreRequest> {
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

/// Policy composition that produced an [`AuthorizationDecision`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyKind {
    /// Personal-local owner resolved over a trusted local transport (or
    /// the bootstrap compatibility seam). Broad local policy, evaluated
    /// through this same API and carrying a decision id for attribution.
    LocalOwnerBroad,
    /// Team membership evaluated against current [`TeamStore`] state.
    TeamMembership,
}

impl PolicyKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalOwnerBroad => "local_owner_broad",
            Self::TeamMembership => "team_membership",
        }
    }
}

/// Server-side authorization request.
///
/// Constructed by the daemon transport from the connection-bound
/// [`AuthenticatedPrincipal`] plus request locators. There is deliberately
/// no constructor that accepts a caller-supplied principal, role, or
/// capability, so a spoofed payload principal cannot influence the
/// decision.
#[derive(Debug, Clone)]
pub struct AuthorizationRequest {
    principal: AuthenticatedPrincipal,
    operation: &'static str,
    scope_kind: ScopeKind,
    capability: Option<Capability>,
    project: Option<ProjectId>,
    correlation_id: String,
}

impl AuthorizationRequest {
    pub fn new(
        principal: AuthenticatedPrincipal,
        descriptor: OperationDescriptor,
        project: Option<ProjectId>,
        correlation_id: impl Into<String>,
    ) -> Self {
        Self {
            principal,
            operation: descriptor.operation,
            scope_kind: descriptor.scope_kind,
            capability: descriptor.capability,
            project,
            correlation_id: correlation_id.into(),
        }
    }

    pub fn principal(&self) -> &AuthenticatedPrincipal {
        &self.principal
    }

    pub fn principal_id(&self) -> &PrincipalId {
        self.principal.principal_id()
    }

    pub fn operation(&self) -> &'static str {
        self.operation
    }

    pub fn scope_kind(&self) -> ScopeKind {
        self.scope_kind
    }

    pub fn capability(&self) -> Option<Capability> {
        self.capability
    }

    pub fn project(&self) -> Option<&ProjectId> {
        self.project.as_ref()
    }

    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }
}

/// Successful server-side authorization decision.
///
/// The daemon enforces the decision before side effects and captures its
/// context (decision id, membership revision, policy) with the resulting
/// turn, run, or job attribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationDecision {
    pub principal_id: PrincipalId,
    pub operation: String,
    pub capability: Option<String>,
    pub project_id: Option<ProjectId>,
    /// Membership revision observed at decision time. Callers bind
    /// long-lived operations to this revision so a concurrent revocation
    /// cannot be masked by a stale allow.
    pub membership_revision: Option<u64>,
    pub policy: PolicyKind,
    pub decision_id: String,
    pub correlation_id: String,
    pub reason: String,
    pub decided_at_ms: i64,
}

/// Structured authorization failure.
///
/// Denial codes and messages are typed for older-client compatibility
/// (`CoreResponse::Error { code, message }`) and contain no secret
/// material and no project-existence signal beyond the operation and
/// capability names (which the caller already supplied).
#[derive(Debug, Error)]
pub enum AuthorizationError {
    #[error("not authorized for {capability} on {operation}")]
    Denied {
        operation: &'static str,
        capability: &'static str,
    },
    #[error("operation {operation} requires a project scope")]
    MissingScope { operation: &'static str },
    #[error("operation {operation} has an ambiguous project scope")]
    AmbiguousScope { operation: &'static str },
    #[error("principal is disabled")]
    DisabledPrincipal,
    #[error("principal is not active")]
    PrincipalNotActive,
    #[error("authorization store is unavailable")]
    Unavailable(#[from] StorageError),
    #[error("team error: {0}")]
    Team(#[from] crate::team::TeamError),
}

impl AuthorizationError {
    /// Stable wire code for `CoreResponse::Error`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Denied { .. } => "authorization_denied",
            Self::MissingScope { .. } => "authorization_scope_required",
            Self::AmbiguousScope { .. } => "authorization_scope_ambiguous",
            Self::DisabledPrincipal | Self::PrincipalNotActive => {
                "authorization_principal_inactive"
            }
            Self::Unavailable(_) | Self::Team(_) => "authorization_unavailable",
        }
    }

    /// `true` for policy denials as opposed to infrastructure failures.
    pub fn is_denial(&self) -> bool {
        matches!(
            self,
            Self::Denied { .. }
                | Self::MissingScope { .. }
                | Self::AmbiguousScope { .. }
                | Self::DisabledPrincipal
                | Self::PrincipalNotActive
        )
    }
}

/// `true` when `principal` is the deterministic local owner.
///
/// LocalOwner principals are evaluated through the same
/// [`AuthorizationService`] API under the broad local policy; this
/// helper only names the composition so call sites stay readable.
pub fn is_local_owner_broad(principal: &AuthenticatedPrincipal) -> bool {
    principal.principal_id().as_str() == LOCAL_OWNER_PRINCIPAL_ID
}

/// Centralized daemon authorization service over M001 team state.
///
/// The service is the single authority that expands roles to
/// capabilities at request time. Handler code asks for semantic
/// capabilities via [`operation_descriptor`]; role interpretation never
/// appears at call sites.
#[derive(Clone)]
pub struct AuthorizationService {
    team: TeamStore,
}

impl AuthorizationService {
    pub fn new(team: TeamStore) -> Self {
        Self { team }
    }

    pub fn team(&self) -> &TeamStore {
        &self.team
    }

    /// Evaluate `request` against current team state.
    ///
    /// LocalOwner principals receive the broad local policy through this
    /// same entry point. Every other principal must be `Active`; global
    /// operations then allow, while project-scoped operations require a
    /// resolved project plus a current membership grant. Unknown
    /// principals, non-active memberships, and missing scopes all fail
    /// closed.
    pub async fn authorize(
        &self,
        request: &AuthorizationRequest,
    ) -> Result<AuthorizationDecision, AuthorizationError> {
        let principal = request.principal();
        if is_local_owner_broad(principal) {
            return Ok(self.allow(
                request,
                None,
                PolicyKind::LocalOwnerBroad,
                "local-owner broad local policy",
            ));
        }
        let record = self
            .team
            .get_principal(request.principal_id())
            .await?
            .ok_or(AuthorizationError::PrincipalNotActive)?;
        if record.status != PrincipalStatus::Active {
            return Err(AuthorizationError::DisabledPrincipal);
        }
        let Some(capability) = request.capability() else {
            return Ok(self.allow(
                request,
                None,
                PolicyKind::TeamMembership,
                "global operation",
            ));
        };
        let Some(project) = request.project() else {
            return Err(AuthorizationError::MissingScope {
                operation: request.operation(),
            });
        };
        let membership = self
            .team
            .get_membership(project, request.principal_id())
            .await?;
        let Some(membership) = membership else {
            return Err(AuthorizationError::Denied {
                operation: request.operation(),
                capability: capability.as_str(),
            });
        };
        if !membership.has_capability(capability) {
            return Err(AuthorizationError::Denied {
                operation: request.operation(),
                capability: capability.as_str(),
            });
        }
        Ok(self.allow(
            request,
            Some(membership.revision),
            PolicyKind::TeamMembership,
            "team membership grant",
        ))
    }

    /// Authorize a bounded enumeration (listing) operation.
    ///
    /// Enumeration rows are privacy-filtered after the read (see
    /// [`visible_projects`]), so no project scope is required up front.
    /// The principal must still be active: unknown or disabled
    /// principals fail closed. LocalOwner uses the broad local policy.
    pub async fn authorize_enumeration(
        &self,
        principal: &AuthenticatedPrincipal,
        operation: &'static str,
        correlation_id: &str,
    ) -> Result<AuthorizationDecision, AuthorizationError> {
        if is_local_owner_broad(principal) {
            return Ok(AuthorizationDecision {
                principal_id: principal.principal_id().clone(),
                operation: operation.to_owned(),
                capability: None,
                project_id: None,
                membership_revision: None,
                policy: PolicyKind::LocalOwnerBroad,
                decision_id: uuid::Uuid::new_v4().to_string(),
                correlation_id: correlation_id.to_owned(),
                reason: "local-owner broad local policy".to_owned(),
                decided_at_ms: now_millis(),
            });
        }
        let record = self
            .team
            .get_principal(principal.principal_id())
            .await?
            .ok_or(AuthorizationError::PrincipalNotActive)?;
        if record.status != PrincipalStatus::Active {
            return Err(AuthorizationError::DisabledPrincipal);
        }
        Ok(AuthorizationDecision {
            principal_id: principal.principal_id().clone(),
            operation: operation.to_owned(),
            capability: None,
            project_id: None,
            membership_revision: None,
            policy: PolicyKind::TeamMembership,
            decision_id: uuid::Uuid::new_v4().to_string(),
            correlation_id: correlation_id.to_owned(),
            reason: "enumeration; rows filtered post-read".to_owned(),
            decided_at_ms: now_millis(),
        })
    }

    fn allow(
        &self,
        request: &AuthorizationRequest,
        membership_revision: Option<u64>,
        policy: PolicyKind,
        reason: &str,
    ) -> AuthorizationDecision {
        AuthorizationDecision {
            principal_id: request.principal_id().clone(),
            operation: request.operation().to_owned(),
            capability: request
                .capability()
                .map(Capability::as_str)
                .map(str::to_owned),
            project_id: request.project().cloned(),
            membership_revision,
            policy,
            decision_id: uuid::Uuid::new_v4().to_string(),
            correlation_id: request.correlation_id().to_owned(),
            reason: reason.to_owned(),
            decided_at_ms: now_millis(),
        }
    }
}

/// Intersect `child` with `parent`: the effective authority of a
/// delegated agent, tool call, or sub-job.
///
/// Authority can only narrow. [`child_escalates`] detects the negative
/// case; [`authorize_child_delegation`] enforces it.
pub fn narrow_authority(parent: &CapabilitySet, child: &CapabilitySet) -> CapabilitySet {
    parent
        .iter()
        .filter(|capability| child.has(*capability))
        .collect()
}

/// `true` when `child` requests any capability outside `parent`.
pub fn child_escalates(parent: &CapabilitySet, child: &CapabilitySet) -> bool {
    child.iter().any(|capability| !parent.has(capability))
}

/// Enforce delegation narrowing: return the narrowed set, or fail closed
/// when the child requests authority beyond the parent decision.
pub fn authorize_child_delegation(
    parent: &CapabilitySet,
    child: &CapabilitySet,
) -> Result<CapabilitySet, AuthorizationError> {
    if child_escalates(parent, child) {
        return Err(AuthorizationError::Denied {
            operation: "child_delegation",
            capability: "narrowed_parent_authority",
        });
    }
    Ok(narrow_authority(parent, child))
}

/// Check that `principal` may use a provider connection with `scope`.
///
/// - Personal connections are owner-only.
/// - Project connections require `capability` on the scoped project.
/// - Deployment connections require the local-owner broad policy or an
///   Owner grant in at least one project (conservative: deployment scope
///   is daemon-wide).
pub async fn authorize_provider_use(
    service: &AuthorizationService,
    principal: &AuthenticatedPrincipal,
    scope: &ProviderScope,
    capability: Capability,
    correlation_id: &str,
) -> Result<AuthorizationDecision, AuthorizationError> {
    if is_local_owner_broad(principal) {
        return Ok(AuthorizationDecision {
            principal_id: principal.principal_id().clone(),
            operation: "provider_connection_use".to_owned(),
            capability: Some(capability.as_str().to_owned()),
            project_id: None,
            membership_revision: None,
            policy: PolicyKind::LocalOwnerBroad,
            decision_id: uuid::Uuid::new_v4().to_string(),
            correlation_id: correlation_id.to_owned(),
            reason: "local-owner broad local policy".to_owned(),
            decided_at_ms: now_millis(),
        });
    }
    match scope {
        ProviderScope::Personal { owner } => {
            if owner != principal.principal_id() {
                return Err(AuthorizationError::Denied {
                    operation: "provider_connection_use",
                    capability: capability.as_str(),
                });
            }
            // Ownership is itself the grant; the principal must still be active.
            let record = service
                .team
                .get_principal(principal.principal_id())
                .await?
                .ok_or(AuthorizationError::PrincipalNotActive)?;
            if record.status != PrincipalStatus::Active {
                return Err(AuthorizationError::DisabledPrincipal);
            }
            Ok(AuthorizationDecision {
                principal_id: principal.principal_id().clone(),
                operation: "provider_connection_use".to_owned(),
                capability: Some(capability.as_str().to_owned()),
                project_id: None,
                membership_revision: Some(record.revision),
                policy: PolicyKind::TeamMembership,
                decision_id: uuid::Uuid::new_v4().to_string(),
                correlation_id: correlation_id.to_owned(),
                reason: "personal connection owner".to_owned(),
                decided_at_ms: now_millis(),
            })
        }
        ProviderScope::Project { project_id } => {
            let descriptor = OperationDescriptor::new(
                "provider_connection_use",
                ScopeKind::DirectProject,
                Some(capability),
            );
            let request = AuthorizationRequest::new(
                principal.clone(),
                descriptor,
                Some(project_id.clone()),
                correlation_id,
            );
            service.authorize(&request).await
        }
        ProviderScope::Deployment { .. } => {
            let memberships = service
                .team
                .list_memberships_for_principal(principal.principal_id())
                .await?;
            let owner_grant = memberships.iter().find(|membership| {
                membership.role == ProjectRole::Owner
                    && membership.state == crate::team::MembershipState::Active
            });
            match owner_grant {
                Some(membership) => Ok(AuthorizationDecision {
                    principal_id: principal.principal_id().clone(),
                    operation: "provider_connection_use".to_owned(),
                    capability: Some(capability.as_str().to_owned()),
                    project_id: Some(membership.project_id.clone()),
                    membership_revision: Some(membership.revision),
                    policy: PolicyKind::TeamMembership,
                    decision_id: uuid::Uuid::new_v4().to_string(),
                    correlation_id: correlation_id.to_owned(),
                    reason: "deployment scope via owner grant".to_owned(),
                    decided_at_ms: now_millis(),
                }),
                None => Err(AuthorizationError::Denied {
                    operation: "provider_connection_use",
                    capability: capability.as_str(),
                }),
            }
        }
    }
}

/// Map team capabilities onto projection-layer capabilities.
///
/// `project.observe`/`session.observe` open the corresponding projection
/// streams; read capabilities open artifact/tool/diff reads; local
/// diagnostics stay local-only. The mapping is conservative: unknown or
/// empty team sets yield an empty projection set.
pub fn team_capabilities_to_projection(caps: &CapabilitySet) -> ProjectionCapabilitySet {
    let mut out = Vec::new();
    if caps.has(Capability::ProjectObserve) {
        out.push(ProjectionCapability::ObservePublicProjection);
    }
    if caps.has(Capability::SessionObserve) || caps.has(Capability::SessionRead) {
        out.push(ProjectionCapability::ObserveSessionProjection);
    }
    if caps.has(Capability::ProjectObserve) || caps.has(Capability::SessionRead) {
        out.push(ProjectionCapability::ObserveClientLocal);
    }
    if caps.has(Capability::SessionRead) {
        out.push(ProjectionCapability::ReadRunArtifact);
        out.push(ProjectionCapability::ReadToolOutput);
    }
    if caps.has(Capability::FileRead) || caps.has(Capability::GitRead) {
        out.push(ProjectionCapability::ReadDiffOrLog);
    }
    ProjectionCapabilitySet::from_iter(out)
}

/// Bounded project resolver for the projects `principal` may read.
///
/// LocalOwner callers should keep the allow-all resolver; team
/// principals receive exactly the projects where they hold
/// `project.read`, so projection `authorize_scope` enforces the same
/// membership grants as the daemon boundary.
pub async fn bounded_resolver_for_principal(
    service: &AuthorizationService,
    principal: &AuthenticatedPrincipal,
    candidates: &[ProjectId],
) -> BoundedProjectResolver {
    let mut allowed = Vec::new();
    for project in candidates {
        let descriptor = OperationDescriptor::new(
            "projection_scope",
            ScopeKind::DirectProject,
            Some(Capability::ProjectRead),
        );
        let request = AuthorizationRequest::new(
            principal.clone(),
            descriptor,
            Some(project.clone()),
            "projection-resolver",
        );
        if service.authorize(&request).await.is_ok() {
            allowed.push(project.as_str().to_owned());
        }
    }
    BoundedProjectResolver::new(allowed)
}

/// Copy audit decision provenance from a gate-enforced decision (M004).
///
/// The daemon boundary calls this with the [`AuthorizationDecision`] it
/// just enforced, so the resulting
/// [`crate::audit::AuditDecisionProvenance`] carries the real decision
/// linkage into the append-only audit store. Instrumentation (M005) MUST
/// use this bridge rather than fabricating provenance: request payloads
/// supply locators but never authority.
pub fn audit_provenance(decision: &AuthorizationDecision) -> crate::audit::AuditDecisionProvenance {
    crate::audit::AuditDecisionProvenance::new(
        decision.decision_id.clone(),
        decision.correlation_id.clone(),
        decision.policy.as_str().to_owned(),
        decision.project_id.clone(),
    )
}

/// Filter `candidates` to the projects where `principal` holds
/// `project.read`.
///
/// Enumeration responses (project/session/job/schedule lists) must pass
/// through this filter so unauthorized callers cannot infer the
/// existence of projects they may not observe. LocalOwner broad policy
/// observes everything.
pub async fn visible_projects(
    service: &AuthorizationService,
    principal: &AuthenticatedPrincipal,
    candidates: &[ProjectId],
) -> Vec<ProjectId> {
    if is_local_owner_broad(principal) {
        return candidates.to_vec();
    }
    let mut visible = Vec::new();
    for project in candidates {
        if service
            .team
            .has_capability(project, principal.principal_id(), Capability::ProjectRead)
            .await
            .unwrap_or(false)
        {
            visible.push(project.clone());
        }
    }
    visible
}

/// Privacy-preserving denial shape for single-project reads.
///
/// Returns the same `(code, message)` the catalog returns for a
/// genuinely absent project, so a denial is indistinguishable from
/// non-existence.
pub fn denial_as_not_found() -> (&'static str, String) {
    ("project_not_found", "project not found".to_owned())
}

/// Immutable originating-principal attribution for durable work.
///
/// Built by the daemon boundary from the transport-bound principal plus
/// the captured [`AuthorizationDecision`]. The origin never changes for
/// the lifetime of the attributed scope; continuation and cancellation
/// rules consume the captured policy/revision instead of re-resolving
/// authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OriginAttribution {
    pub origin_principal: PrincipalId,
    pub origin_kind: PrincipalKind,
    pub auth_method: AuthMethod,
    pub transport_class: TransportClass,
    pub policy: PolicyKind,
    pub membership_revision: Option<u64>,
    pub decision_id: String,
    pub correlation_id: String,
    pub created_at_ms: i64,
}

impl OriginAttribution {
    /// Build attribution from transport authority plus its decision.
    ///
    /// The principal comes from the bound [`AuthenticatedPrincipal`];
    /// `decision` supplies the captured policy context. Callers cannot
    /// substitute a payload-supplied identity.
    pub fn from_authority(
        principal: &AuthenticatedPrincipal,
        decision: &AuthorizationDecision,
    ) -> Self {
        Self {
            origin_principal: principal.principal_id().clone(),
            origin_kind: principal.kind(),
            auth_method: principal.auth_method(),
            transport_class: principal.transport_class(),
            policy: decision.policy,
            membership_revision: decision.membership_revision,
            decision_id: decision.decision_id.clone(),
            correlation_id: decision.correlation_id.clone(),
            created_at_ms: now_millis(),
        }
    }

    /// Explicit legacy provenance for pre-M003 records that carry no
    /// canonical principal.
    ///
    /// The provenance marker is the literal `"legacy-local"` principal
    /// namespace: it records that the work predates attribution without
    /// fabricating a team identity for it.
    pub fn legacy_local(correlation_id: impl Into<String>) -> Self {
        Self {
            origin_principal: PrincipalId::parse("legacy-local")
                .expect("legacy-local satisfies the identity lexical contract"),
            origin_kind: PrincipalKind::LocalOwner,
            auth_method: AuthMethod::LocalOwner,
            transport_class: TransportClass::Local,
            policy: PolicyKind::LocalOwnerBroad,
            membership_revision: None,
            decision_id: "legacy-local".to_owned(),
            correlation_id: correlation_id.into(),
            created_at_ms: now_millis(),
        }
    }

    /// `true` for the explicit legacy provenance marker (not a team grant).
    pub fn is_legacy(&self) -> bool {
        self.origin_principal.as_str() == "legacy-local"
    }
}

/// Durable store for originating-principal attribution.
///
/// One row per attributed scope `(scope_kind, scope_id)`. The first
/// attribution wins (`INSERT ... ON CONFLICT DO NOTHING`): origin is
/// immutable, and a concurrent second writer cannot rewrite it. Scopes
/// name sessions, turns, runs, jobs, worktrees, and provider selections.
#[derive(Clone)]
pub struct OriginAttributionStore {
    pool: SqlitePool,
}

impl OriginAttributionStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Record the origin for `scope`. The first write wins; later calls
    /// for the same scope return the stored row unchanged.
    pub async fn record(
        &self,
        scope_kind: &str,
        scope_id: &str,
        attribution: &OriginAttribution,
    ) -> Result<OriginAttribution, StorageError> {
        validate_attribution_scope(scope_kind, scope_id)?;
        let json = serde_json::to_string(attribution)
            .map_err(|e| StorageError::Database(e.to_string()))?;
        sqlx::query(
            "INSERT INTO origin_attribution (scope_kind, scope_id, origin_principal, \
             origin_kind, auth_method, transport_class, policy, membership_revision, \
             decision_id, correlation_id, time_created, attribution_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(scope_kind, scope_id) DO NOTHING",
        )
        .bind(scope_kind)
        .bind(scope_id)
        .bind(attribution.origin_principal.as_str())
        .bind(origin_kind_str(attribution.origin_kind))
        .bind(auth_method_str(attribution.auth_method))
        .bind(transport_class_str(attribution.transport_class))
        .bind(attribution.policy.as_str())
        .bind(attribution.membership_revision.map(|r| r as i64))
        .bind(&attribution.decision_id)
        .bind(&attribution.correlation_id)
        .bind(attribution.created_at_ms)
        .bind(&json)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        self.get(scope_kind, scope_id)
            .await?
            .ok_or_else(|| StorageError::Database("origin attribution write lost".to_owned()))
    }

    /// Fetch the stored origin for `scope`, if any.
    pub async fn get(
        &self,
        scope_kind: &str,
        scope_id: &str,
    ) -> Result<Option<OriginAttribution>, StorageError> {
        validate_attribution_scope(scope_kind, scope_id)?;
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT attribution_json FROM origin_attribution WHERE scope_kind = ? AND scope_id = ?",
        )
        .bind(scope_kind)
        .bind(scope_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        row.map(|(json,)| {
            serde_json::from_str(&json).map_err(|e| StorageError::Database(e.to_string()))
        })
        .transpose()
    }
}

fn validate_attribution_scope(scope_kind: &str, scope_id: &str) -> Result<(), StorageError> {
    const ALLOWED_KINDS: [&str; 6] = ["session", "turn", "run", "job", "worktree", "provider"];
    if !ALLOWED_KINDS.contains(&scope_kind) {
        return Err(StorageError::Database(format!(
            "unknown attribution scope kind {scope_kind:?}"
        )));
    }
    if scope_id.is_empty() || scope_id.len() > 128 {
        return Err(StorageError::Database(
            "attribution scope id must be non-empty and bounded".to_owned(),
        ));
    }
    if scope_id.contains('/') || scope_id.contains('\\') || scope_id.contains('\0') {
        return Err(StorageError::Database(
            "attribution scope id must not be path-like".to_owned(),
        ));
    }
    Ok(())
}

fn origin_kind_str(kind: PrincipalKind) -> &'static str {
    kind.as_str()
}

fn auth_method_str(method: AuthMethod) -> &'static str {
    method.as_str()
}

fn transport_class_str(class: TransportClass) -> &'static str {
    class.as_str()
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::team::{MembershipState, PrincipalKind, ProjectRole};

    async fn test_service() -> (TeamStore, AuthorizationService) {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate test store");
        let team = TeamStore::new(pool);
        let service = AuthorizationService::new(team.clone());
        (team, service)
    }

    fn local_owner_request(
        operation: &'static str,
        capability: Option<Capability>,
        project: Option<ProjectId>,
    ) -> AuthorizationRequest {
        let principal = AuthenticatedPrincipal::local_owner("client-local");
        AuthorizationRequest::new(
            principal,
            OperationDescriptor::new(operation, ScopeKind::DirectProject, capability),
            project,
            "corr-1",
        )
    }

    fn remote_request(
        principal: AuthenticatedPrincipal,
        operation: &'static str,
        capability: Option<Capability>,
        project: Option<ProjectId>,
    ) -> AuthorizationRequest {
        AuthorizationRequest::new(
            principal,
            OperationDescriptor::new(operation, ScopeKind::DirectProject, capability),
            project,
            "corr-remote",
        )
    }

    async fn human_with_token(
        team: &TeamStore,
        name: &str,
        client: &str,
    ) -> AuthenticatedPrincipal {
        use crate::transport_auth::PersonalTokenStore;
        let record = team
            .create_principal(PrincipalKind::Human, name)
            .await
            .unwrap();
        let tokens = PersonalTokenStore::with_team(team.pool().clone(), team.clone());
        let (plaintext, _) = tokens
            .create_personal_token(&record.id, "device", None)
            .await
            .unwrap();
        tokens.verify_for_client(&plaintext, client).await.unwrap()
    }

    #[test]
    fn operation_matrix_has_no_duplicate_operations() {
        let matrix = operation_capability_matrix();
        let mut names: Vec<&str> = matrix.iter().map(|(op, _, _)| op.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            matrix.len(),
            "matrix operations must be unique"
        );
        // The matrix must cover the full native surface (exhaustive match
        // guarantees classification; this pins the breadth for reviewers).
        assert!(
            matrix.len() >= 130,
            "matrix must cover the native surface, got {}",
            matrix.len()
        );
    }

    #[test]
    fn operation_matrix_spot_checks_match_plan() {
        let find = |operation: &str| {
            operation_capability_matrix()
                .into_iter()
                .find(|(op, _, _)| op == operation)
                .unwrap_or_else(|| panic!("missing matrix row for {operation}"))
        };
        assert_eq!(
            find("turn_submit"),
            (
                "turn_submit".to_owned(),
                "via_session".to_owned(),
                "agent.invoke".to_owned()
            )
        );
        assert_eq!(
            find("project_list"),
            (
                "project_list".to_owned(),
                "enumeration".to_owned(),
                "project.read".to_owned()
            )
        );
        assert_eq!(
            find("project_register"),
            (
                "project_register".to_owned(),
                "global".to_owned(),
                "none".to_owned()
            )
        );
        assert_eq!(
            find("session_create"),
            (
                "session_create".to_owned(),
                "direct_project".to_owned(),
                "session.create".to_owned()
            )
        );
        assert_eq!(
            find("job_submit"),
            (
                "job_submit".to_owned(),
                "via_session".to_owned(),
                "job.submit".to_owned()
            )
        );
        assert_eq!(
            find("projection_artifact_read"),
            (
                "projection_artifact_read".to_owned(),
                "direct_project".to_owned(),
                "project.observe".to_owned()
            )
        );
        assert_eq!(
            find("lsp_preview_apply"),
            (
                "lsp_preview_apply".to_owned(),
                "opaque".to_owned(),
                "file.modify".to_owned()
            )
        );
        assert_eq!(
            find("initialize"),
            (
                "initialize".to_owned(),
                "global".to_owned(),
                "none".to_owned()
            )
        );
    }

    #[test]
    fn every_representative_request_maps_to_a_named_operation() {
        for request in representative_requests() {
            let descriptor = operation_descriptor(&request);
            assert!(!descriptor.operation.is_empty());
            assert!(!descriptor.scope_kind.as_str().is_empty());
        }
        // Representative set and matrix stay in lockstep.
        assert_eq!(
            representative_requests().len(),
            operation_capability_matrix().len()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn role_allow_deny_matrix_matches_m001_expansion() {
        let (team, service) = test_service().await;
        let project = ProjectId::new();
        let cases = [
            (ProjectRole::Viewer, Capability::ProjectRead, true),
            (ProjectRole::Viewer, Capability::GitWrite, false),
            (ProjectRole::Viewer, Capability::SessionCreate, false),
            (ProjectRole::Contributor, Capability::SessionCreate, true),
            (ProjectRole::Contributor, Capability::AgentInvoke, true),
            (ProjectRole::Contributor, Capability::MemberManage, false),
            (
                ProjectRole::Contributor,
                Capability::ProjectConfigure,
                false,
            ),
            (ProjectRole::Maintainer, Capability::ProjectConfigure, true),
            (ProjectRole::Maintainer, Capability::AuditRead, true),
            (ProjectRole::Maintainer, Capability::MemberManage, false),
            (ProjectRole::Maintainer, Capability::NodeTarget, false),
            (ProjectRole::Owner, Capability::MemberManage, true),
            (ProjectRole::Owner, Capability::NodeTarget, true),
        ];
        for (role, capability, allowed) in cases {
            let name = format!("{role:?}-probe");
            let principal = team
                .create_principal(PrincipalKind::Human, &name)
                .await
                .unwrap();
            team.create_membership(&project, &principal.id, role)
                .await
                .unwrap();
            let bound = AuthenticatedPrincipal::internal_test(&principal, "client-x");
            let request = remote_request(bound, "probe", Some(capability), Some(project.clone()));
            let outcome = service.authorize(&request).await.is_ok();
            assert_eq!(outcome, allowed, "{role:?} vs {}", capability.as_str());
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn local_owner_uses_same_api_with_broad_policy() {
        let (_, service) = test_service().await;
        // No membership rows exist at all; LocalOwner still authorizes.
        let request = local_owner_request(
            "turn_submit",
            Some(Capability::AgentInvoke),
            Some(ProjectId::new()),
        );
        let decision = service.authorize(&request).await.unwrap();
        assert_eq!(decision.policy, PolicyKind::LocalOwnerBroad);
        assert!(!decision.decision_id.is_empty());
        // And without any project scope as well.
        let request = local_owner_request("lsp_preview_apply", Some(Capability::FileModify), None);
        let decision = service.authorize(&request).await.unwrap();
        assert_eq!(decision.policy, PolicyKind::LocalOwnerBroad);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_scope_fails_closed_for_team_principals() {
        let (team, service) = test_service().await;
        let principal = human_with_token(&team, "Ada", "client-ada").await;
        let request = remote_request(
            principal,
            "session_create",
            Some(Capability::SessionCreate),
            None,
        );
        let error = service.authorize(&request).await.unwrap_err();
        assert!(matches!(error, AuthorizationError::MissingScope { .. }));
        assert_eq!(error.code(), "authorization_scope_required");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn enumeration_requires_active_principal_but_no_scope() {
        let (team, service) = test_service().await;
        let active = human_with_token(&team, "Ada", "client-ada").await;
        let decision = service
            .authorize_enumeration(&active, "project_list", "corr-enum")
            .await
            .unwrap();
        assert_eq!(decision.policy, PolicyKind::TeamMembership);
        let record = team
            .create_principal(PrincipalKind::Human, "Mallory")
            .await
            .unwrap();
        team.set_principal_status(&record.id, record.revision, PrincipalStatus::Disabled)
            .await
            .unwrap();
        let disabled = AuthenticatedPrincipal::internal_test(&record, "client-mallory");
        assert!(service
            .authorize_enumeration(&disabled, "project_list", "corr-enum")
            .await
            .is_err());
        let local = AuthenticatedPrincipal::local_owner("client-local");
        let decision = service
            .authorize_enumeration(&local, "project_list", "corr-enum")
            .await
            .unwrap();
        assert_eq!(decision.policy, PolicyKind::LocalOwnerBroad);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn spoofed_payload_project_cannot_grant_authority() {
        let (team, service) = test_service().await;
        let victim_project = ProjectId::new();
        // The DTO names the victim project, but the bound principal is an
        // outsider with no membership: the decision must deny.
        let attacker = human_with_token(&team, "Mallory", "client-mallory").await;
        let request = remote_request(
            attacker,
            "turn_submit",
            Some(Capability::AgentInvoke),
            Some(victim_project),
        );
        let error = service.authorize(&request).await.unwrap_err();
        assert!(matches!(error, AuthorizationError::Denied { .. }));
        assert_eq!(error.code(), "authorization_denied");
        // The denial carries no secret and no existence signal.
        let rendered = format!("{error:?}");
        for forbidden in ["secret", "token", "bearer", "digest", "password"] {
            assert!(!rendered.to_ascii_lowercase().contains(forbidden));
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn membership_removal_race_fails_new_authorization() {
        let (team, service) = test_service().await;
        let project = ProjectId::new();
        let record = team
            .create_principal(PrincipalKind::Human, "Ada")
            .await
            .unwrap();
        let membership = team
            .create_membership(&project, &record.id, ProjectRole::Owner)
            .await
            .unwrap();
        let bound = AuthenticatedPrincipal::internal_test(&record, "client-ada");
        let request = remote_request(
            bound.clone(),
            "project_archive",
            Some(Capability::ProjectConfigure),
            Some(project.clone()),
        );
        let decision = service.authorize(&request).await.unwrap();
        assert_eq!(decision.membership_revision, Some(membership.revision));
        // Revoke, then re-authorization with the same (now stale) context fails.
        team.revoke_membership(&project, &record.id, membership.revision)
            .await
            .unwrap();
        let error = service.authorize(&request).await.unwrap_err();
        assert!(matches!(error, AuthorizationError::Denied { .. }));
        // A stale writer holding the pre-revocation revision cannot restore
        // authority through the membership store either.
        let stale = team
            .update_membership(
                &project,
                &record.id,
                membership.revision,
                Some(ProjectRole::Owner),
                Some(MembershipState::Active),
            )
            .await;
        assert!(stale.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn disabled_principal_cannot_authorize() {
        let (team, service) = test_service().await;
        let project = ProjectId::new();
        let record = team
            .create_principal(PrincipalKind::Human, "Ada")
            .await
            .unwrap();
        team.create_membership(&project, &record.id, ProjectRole::Owner)
            .await
            .unwrap();
        team.set_principal_status(&record.id, record.revision, PrincipalStatus::Disabled)
            .await
            .unwrap();
        let bound = AuthenticatedPrincipal::internal_test(&record, "client-ada");
        let request = remote_request(
            bound,
            "session_create",
            Some(Capability::SessionCreate),
            Some(project),
        );
        assert!(service.authorize(&request).await.is_err());
    }

    #[test]
    fn child_authority_narrows_and_escalation_fails() {
        let parent = ProjectRole::Contributor.capabilities();
        let child_ok = CapabilitySet::from_caps([Capability::FileRead, Capability::GitRead]);
        assert!(!child_escalates(&parent, &child_ok));
        let narrowed = authorize_child_delegation(&parent, &child_ok).unwrap();
        assert_eq!(narrowed, child_ok);
        let child_evil = CapabilitySet::from_caps([Capability::FileRead, Capability::MemberManage]);
        assert!(child_escalates(&parent, &child_evil));
        let error = authorize_child_delegation(&parent, &child_evil).unwrap_err();
        assert_eq!(error.code(), "authorization_denied");
        // Narrowing is an intersection even for benign overlap.
        let partial = CapabilitySet::from_caps([Capability::GitWrite, Capability::NodeTarget]);
        let narrowed = narrow_authority(&parent, &partial);
        assert!(narrowed.has(Capability::GitWrite));
        assert!(!narrowed.has(Capability::NodeTarget));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn provider_scope_checks_enforce_ownership() {
        let (team, service) = test_service().await;
        let project = ProjectId::new();
        let alice = team
            .create_principal(PrincipalKind::Human, "Alice")
            .await
            .unwrap();
        let bob = team
            .create_principal(PrincipalKind::Human, "Bob")
            .await
            .unwrap();
        team.create_membership(&project, &alice.id, ProjectRole::Contributor)
            .await
            .unwrap();
        let alice_bound = AuthenticatedPrincipal::internal_test(&alice, "client-alice");
        let bob_bound = AuthenticatedPrincipal::internal_test(&bob, "client-bob");

        // Personal scope is owner-only.
        let personal = ProviderScope::Personal {
            owner: alice.id.clone(),
        };
        assert!(authorize_provider_use(
            &service,
            &alice_bound,
            &personal,
            Capability::AgentInvoke,
            "c1"
        )
        .await
        .is_ok());
        assert!(authorize_provider_use(
            &service,
            &bob_bound,
            &personal,
            Capability::AgentInvoke,
            "c1"
        )
        .await
        .is_err());
        // Project scope follows membership grants.
        let project_scope = ProviderScope::Project {
            project_id: project.clone(),
        };
        assert!(authorize_provider_use(
            &service,
            &alice_bound,
            &project_scope,
            Capability::AgentInvoke,
            "c1"
        )
        .await
        .is_ok());
        assert!(authorize_provider_use(
            &service,
            &bob_bound,
            &project_scope,
            Capability::AgentInvoke,
            "c1"
        )
        .await
        .is_err());
        // Deployment scope needs an Owner grant somewhere (Bob has none).
        let deployment = ProviderScope::deployment("deployment-fixture").expect("deployment scope");
        assert!(authorize_provider_use(
            &service,
            &bob_bound,
            &deployment,
            Capability::AgentInvoke,
            "c1"
        )
        .await
        .is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn deployment_scope_owner_grant_allows() {
        let (team, service) = test_service().await;
        let project = ProjectId::new();
        let owner = team
            .create_principal(PrincipalKind::Human, "Owner")
            .await
            .unwrap();
        team.create_membership(&project, &owner.id, ProjectRole::Owner)
            .await
            .unwrap();
        let bound = AuthenticatedPrincipal::internal_test(&owner, "client-owner");
        let deployment = ProviderScope::deployment("deployment-fixture").expect("deployment scope");
        let decision =
            authorize_provider_use(&service, &bound, &deployment, Capability::AgentInvoke, "c1")
                .await
                .unwrap();
        assert_eq!(decision.policy, PolicyKind::TeamMembership);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn enumeration_is_privacy_filtered() {
        let (team, service) = test_service().await;
        let project_a = ProjectId::new();
        let project_b = ProjectId::new();
        let alice = team
            .create_principal(PrincipalKind::Human, "Alice")
            .await
            .unwrap();
        team.create_membership(&project_a, &alice.id, ProjectRole::Viewer)
            .await
            .unwrap();
        let bound = AuthenticatedPrincipal::internal_test(&alice, "client-alice");
        let visible =
            visible_projects(&service, &bound, &[project_a.clone(), project_b.clone()]).await;
        assert_eq!(visible, vec![project_a]);
        // LocalOwner observes everything through the broad policy.
        let local = AuthenticatedPrincipal::local_owner("client-local");
        let visible = visible_projects(&service, &local, std::slice::from_ref(&project_b)).await;
        assert_eq!(visible, vec![project_b.clone()]);
        // Denial-as-not-found carries no existence signal.
        let (code, message) = denial_as_not_found();
        assert_eq!(code, "project_not_found");
        assert!(!message.contains(project_b.as_str()));
    }

    #[test]
    fn team_capabilities_map_conservatively_onto_projection() {
        let viewer = ProjectRole::Viewer.capabilities();
        let set = team_capabilities_to_projection(&viewer);
        assert!(set.has(ProjectionCapability::ObservePublicProjection));
        assert!(set.has(ProjectionCapability::ObserveSessionProjection));
        assert!(set.has(ProjectionCapability::ReadRunArtifact));
        assert!(!set.has(ProjectionCapability::AdminBypass));
        let empty = team_capabilities_to_projection(&CapabilitySet::new());
        assert!(empty.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn origin_attribution_round_trip_and_first_write_wins() {
        let pool = SqlitePool::connect("sqlite::memory:").await.expect("pool");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        let store = OriginAttributionStore::new(pool);
        let principal = AuthenticatedPrincipal::local_owner("client-1");
        let decision = AuthorizationDecision {
            principal_id: principal.principal_id().clone(),
            operation: "session_create".to_owned(),
            capability: Some("session.create".to_owned()),
            project_id: None,
            membership_revision: None,
            policy: PolicyKind::LocalOwnerBroad,
            decision_id: "decision-1".to_owned(),
            correlation_id: "corr-1".to_owned(),
            reason: "test".to_owned(),
            decided_at_ms: 1,
        };
        let attribution = OriginAttribution::from_authority(&principal, &decision);
        assert!(!attribution.is_legacy());
        let stored = store
            .record("session", "session-1", &attribution)
            .await
            .unwrap();
        assert_eq!(stored, attribution);
        // A concurrent second writer cannot rewrite the origin.
        let mut other = attribution.clone();
        other.decision_id = "decision-2".to_owned();
        let stored_again = store.record("session", "session-1", &other).await.unwrap();
        assert_eq!(stored_again.decision_id, "decision-1");
        assert!(store.get("session", "nope").await.unwrap().is_none());
        assert!(store.record("bogus", "x", &attribution).await.is_err());
    }

    #[test]
    fn legacy_attribution_is_explicit_never_fabricated() {
        let legacy = OriginAttribution::legacy_local("corr-legacy");
        assert!(legacy.is_legacy());
        assert_eq!(legacy.origin_principal.as_str(), "legacy-local");
        let json = serde_json::to_string(&legacy).expect("serialize legacy");
        assert!(json.contains("legacy-local"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn attribution_survives_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("m003-attribution.db");
        let url = format!("sqlite:{}?mode=rwc", path.display());
        let pool = SqlitePool::connect(&url).await.expect("connect");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        let store = OriginAttributionStore::new(pool.clone());
        let principal = AuthenticatedPrincipal::local_owner("client-1");
        let decision = AuthorizationDecision {
            principal_id: principal.principal_id().clone(),
            operation: "job_submit".to_owned(),
            capability: Some("job.submit".to_owned()),
            project_id: None,
            membership_revision: None,
            policy: PolicyKind::LocalOwnerBroad,
            decision_id: "decision-restart".to_owned(),
            correlation_id: "corr-restart".to_owned(),
            reason: "test".to_owned(),
            decided_at_ms: 1,
        };
        let attribution = OriginAttribution::from_authority(&principal, &decision);
        store.record("job", "job-1", &attribution).await.unwrap();
        pool.close().await;
        let pool2 = SqlitePool::connect(&url).await.expect("reconnect");
        crate::session::schema::migrate(&pool2)
            .await
            .expect("remigrate");
        let store2 = OriginAttributionStore::new(pool2.clone());
        let reloaded = store2.get("job", "job-1").await.unwrap().unwrap();
        assert_eq!(reloaded, attribution);
        pool2.close().await;
    }

    #[test]
    fn authorization_errors_carry_no_secrets() {
        let denied = AuthorizationError::Denied {
            operation: "turn_submit",
            capability: "agent.invoke",
        };
        assert_eq!(denied.code(), "authorization_denied");
        let rendered = format!("{denied} {denied:?}");
        for forbidden in [
            "secret",
            "token",
            "bearer",
            "digest",
            "password",
            "credential",
        ] {
            assert!(
                !rendered.to_ascii_lowercase().contains(forbidden),
                "{forbidden} leaked"
            );
        }
        assert!(AuthorizationError::MissingScope { operation: "x" }.is_denial());
        assert!(
            !AuthorizationError::Unavailable(StorageError::Database("db".to_owned())).is_denial()
        );
    }
}
