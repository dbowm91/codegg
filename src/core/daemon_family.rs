//! M002: `CoreDaemon` request-family ownership map.
//!
//! `CoreDaemon` remains the single daemon composition/lifecycle authority.
//! [`DaemonRequestFamily::of`] is the only request-to-owner routing table:
//! the thin dispatcher in `super::daemon` routes each envelope to exactly
//! one family handler operating on the same daemon-owned state. Chat and
//! interactive-process requests are classified here for documentation but are
//! served before the router by their existing boxed/spawned paths, preserving
//! their stack/cancellation semantics exactly.
//!
//! | Family | Owner module | Responsibility |
//! |---|---|---|---|
//! | `assets` | `daemon_assets.rs` | runtime-asset refresh/status over the daemon-owned AssetRefreshCoordinator |
//! | `providers` | `daemon_providers.rs` | Eggpool provisioning and provider-connection lifecycle over the daemon-owned provisioner |
//! | `sessions` | `daemon_sessions.rs` | session CRUD, selection reads, message reads, and import/export over daemon-owned session stores |
//! | `turns` | `daemon_turns.rs` | turn submit/cancel/steer, agent/model selection writes, permission/question responses, and transport lifecycle |
//! | `jobs` | `daemon_jobs.rs` | durable jobs, schedules, run records, tool-program inspection, and legacy task shims over the scheduler/job stores |
//! | `projects` | `daemon_projects.rs` | project catalog, workspace registry/services, managed worktrees, and daemon/workspace snapshots |
//! | `goals` | `daemon_goals.rs` | session goals, todos, edit checkpoints, and LSP preview apply over daemon-owned domain stores |
//! | `projection` | `daemon_projection.rs` | projection replay subscribe/resume/ack/snapshot/artifacts plus ephemeral presence leases |
//! | `ops` | `daemon_ops.rs` | audit query/export, memory, and notification routing over daemon-owned stores |
//! | `chat` | `daemon.rs::handle_chat_request` (pre-router, boxed) | durable project chat channels/messages/actions |
//! | `interactive` | `daemon.rs::run_interactive_request` (pre-router, spawned task) | bounded attach/resume over the scheduler admission controller |

use crate::protocol::core::CoreRequest;

/// Coherent `CoreRequest` family with exactly one owner module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonRequestFamily {
    Assets,
    Providers,
    Sessions,
    Turns,
    Jobs,
    Projects,
    Goals,
    Projection,
    Ops,
    Chat,
    Interactive,
}

impl DaemonRequestFamily {
    /// Route one request to its single owning family.
    pub fn of(request: &CoreRequest) -> Self {
        match request {
            CoreRequest::AssetRefresh { .. } => Self::Assets,
            CoreRequest::AssetRefreshCapabilities => Self::Assets,
            CoreRequest::AssetRefreshStatus { .. } => Self::Assets,
            CoreRequest::ConnectionDelete { .. } => Self::Providers,
            CoreRequest::ConnectionDisable { .. } => Self::Providers,
            CoreRequest::ConnectionEnable { .. } => Self::Providers,
            CoreRequest::ConnectionGet { .. } => Self::Providers,
            CoreRequest::ConnectionListDetail => Self::Providers,
            CoreRequest::ConnectionPurge { .. } => Self::Providers,
            CoreRequest::ConnectionRefreshBegin { .. } => Self::Providers,
            CoreRequest::ConnectionRefreshCancel { .. } => Self::Providers,
            CoreRequest::ConnectionRefreshStatus { .. } => Self::Providers,
            CoreRequest::ConnectionRestore { .. } => Self::Providers,
            CoreRequest::ConnectionRotateBegin { .. } => Self::Providers,
            CoreRequest::ConnectionRotateCancel { .. } => Self::Providers,
            CoreRequest::ConnectionRotateSecretStage { .. } => Self::Providers,
            CoreRequest::ConnectionRotateStatus { .. } => Self::Providers,
            CoreRequest::EggpoolConnectionCancel { .. } => Self::Providers,
            CoreRequest::EggpoolConnectionCreate { .. } => Self::Providers,
            CoreRequest::EggpoolConnectionStatus { .. } => Self::Providers,
            CoreRequest::ProviderConnectionList => Self::Providers,
            CoreRequest::ProviderConnectionModels { .. } => Self::Providers,
            CoreRequest::SessionArchive { .. } => Self::Sessions,
            CoreRequest::SessionAttach { .. } => Self::Sessions,
            CoreRequest::SessionCreate { .. } => Self::Sessions,
            CoreRequest::SessionCreateFromTemplate { .. } => Self::Sessions,
            CoreRequest::SessionDelete { .. } => Self::Sessions,
            CoreRequest::SessionExport { .. } => Self::Sessions,
            CoreRequest::SessionFork { .. } => Self::Sessions,
            CoreRequest::SessionImportData { .. } => Self::Sessions,
            CoreRequest::SessionLifecycleGet { .. } => Self::Sessions,
            CoreRequest::SessionList { .. } => Self::Sessions,
            CoreRequest::SessionLoad { .. } => Self::Sessions,
            CoreRequest::SessionMessageCounts { .. } => Self::Sessions,
            CoreRequest::SessionMessagesLoad { .. } => Self::Sessions,
            CoreRequest::SessionRename { .. } => Self::Sessions,
            CoreRequest::SessionRestore { .. } => Self::Sessions,
            CoreRequest::SessionSelectionGet { .. } => Self::Sessions,
            CoreRequest::SessionSelectionList { .. } => Self::Sessions,
            CoreRequest::SessionSelectionModels { .. } => Self::Sessions,
            CoreRequest::SessionSelectionUpdate { .. } => Self::Sessions,
            CoreRequest::SessionShare { .. } => Self::Sessions,
            CoreRequest::SessionUnshare { .. } => Self::Sessions,
            CoreRequest::AgentSelect { .. } => Self::Turns,
            CoreRequest::ModelSelect { .. } => Self::Turns,
            CoreRequest::ModelsRefresh => Self::Turns,
            CoreRequest::PermissionRespond { .. } => Self::Turns,
            CoreRequest::QuestionRespond { .. } => Self::Turns,
            CoreRequest::Resume { .. } => Self::Turns,
            CoreRequest::SnapshotModels => Self::Turns,
            CoreRequest::SnapshotSession { .. } => Self::Turns,
            CoreRequest::Subscribe { .. } => Self::Turns,
            CoreRequest::TurnCancel { .. } => Self::Turns,
            CoreRequest::TurnSteer { .. } => Self::Turns,
            CoreRequest::TurnSubmit { .. } => Self::Turns,
            CoreRequest::JobAttempts { .. } => Self::Jobs,
            CoreRequest::JobCancel { .. } => Self::Jobs,
            CoreRequest::JobGet { .. } => Self::Jobs,
            CoreRequest::JobList { .. } => Self::Jobs,
            CoreRequest::JobRecoveryReport => Self::Jobs,
            CoreRequest::JobRetry { .. } => Self::Jobs,
            CoreRequest::JobSubmit { .. } => Self::Jobs,
            CoreRequest::JobWait { .. } => Self::Jobs,
            CoreRequest::RunArtifactRead { .. } => Self::Jobs,
            CoreRequest::RunGet { .. } => Self::Jobs,
            CoreRequest::RunList { .. } => Self::Jobs,
            CoreRequest::RunRerun { .. } => Self::Jobs,
            CoreRequest::ScheduleCreate { .. } => Self::Jobs,
            CoreRequest::ScheduleDelete { .. } => Self::Jobs,
            CoreRequest::ScheduleGet { .. } => Self::Jobs,
            CoreRequest::ScheduleList { .. } => Self::Jobs,
            CoreRequest::SchedulePause { .. } => Self::Jobs,
            CoreRequest::ScheduleResume { .. } => Self::Jobs,
            CoreRequest::SchedulerSnapshot => Self::Jobs,
            CoreRequest::TaskDelete { .. } => Self::Jobs,
            CoreRequest::TaskList => Self::Jobs,
            CoreRequest::TaskSchedule { .. } => Self::Jobs,
            CoreRequest::ToolProgramCallPage { .. } => Self::Jobs,
            CoreRequest::ToolProgramInspect { .. } => Self::Jobs,
            CoreRequest::ToolProgramList { .. } => Self::Jobs,
            CoreRequest::ToolProgramNotificationReinject { .. } => Self::Jobs,
            CoreRequest::ToolProgramRecoveryDebugInspect { .. } => Self::Jobs,
            CoreRequest::ManagedWorktreeArchive { .. } => Self::Projects,
            CoreRequest::ManagedWorktreeCleanup { .. } => Self::Projects,
            CoreRequest::ManagedWorktreeGet { .. } => Self::Projects,
            CoreRequest::ManagedWorktreeList { .. } => Self::Projects,
            CoreRequest::ProjectArchive { .. } => Self::Projects,
            CoreRequest::ProjectCatalogCapabilities => Self::Projects,
            CoreRequest::ProjectGet { .. } => Self::Projects,
            CoreRequest::ProjectHealth { .. } => Self::Projects,
            CoreRequest::ProjectList { .. } => Self::Projects,
            CoreRequest::ProjectRegister { .. } => Self::Projects,
            CoreRequest::ProjectRestore { .. } => Self::Projects,
            CoreRequest::SnapshotDaemon => Self::Projects,
            CoreRequest::SnapshotWorkspace { .. } => Self::Projects,
            CoreRequest::WorkspaceArchive { .. } => Self::Projects,
            CoreRequest::WorkspaceConfigReload { .. } => Self::Projects,
            CoreRequest::WorkspaceList { .. } => Self::Projects,
            CoreRequest::WorkspaceRegister { .. } => Self::Projects,
            CoreRequest::WorkspaceServicesSnapshot => Self::Projects,
            CoreRequest::WorkspaceSnapshotRequest { .. } => Self::Projects,
            CoreRequest::WorktreeList { .. } => Self::Projects,
            CoreRequest::ActiveGoalLoad { .. } => Self::Goals,
            CoreRequest::EditCheckpointGet { .. } => Self::Goals,
            CoreRequest::EditCheckpointList { .. } => Self::Goals,
            CoreRequest::EditCheckpointReapply { .. } => Self::Goals,
            CoreRequest::EditCheckpointReapplyLatest { .. } => Self::Goals,
            CoreRequest::EditCheckpointUndo { .. } => Self::Goals,
            CoreRequest::EditCheckpointUndoLatest { .. } => Self::Goals,
            CoreRequest::GoalCheckpoint { .. } => Self::Goals,
            CoreRequest::GoalClear { .. } => Self::Goals,
            CoreRequest::GoalDone { .. } => Self::Goals,
            CoreRequest::GoalFromFile { .. } => Self::Goals,
            CoreRequest::GoalPause { .. } => Self::Goals,
            CoreRequest::GoalResume { .. } => Self::Goals,
            CoreRequest::GoalSet { .. } => Self::Goals,
            CoreRequest::GoalSetBudget { .. } => Self::Goals,
            CoreRequest::GoalShow { .. } => Self::Goals,
            CoreRequest::LspPreviewApply { .. } => Self::Goals,
            CoreRequest::TodoList { .. } => Self::Goals,
            CoreRequest::PresenceCapabilities => Self::Projection,
            CoreRequest::PresenceHeartbeat { .. } => Self::Projection,
            CoreRequest::PresenceSnapshotGet { .. } => Self::Projection,
            CoreRequest::ProjectionAck { .. } => Self::Projection,
            CoreRequest::ProjectionArtifactList { .. } => Self::Projection,
            CoreRequest::ProjectionArtifactRead { .. } => Self::Projection,
            CoreRequest::ProjectionCapabilities => Self::Projection,
            CoreRequest::ProjectionResume { .. } => Self::Projection,
            CoreRequest::ProjectionSnapshotGet { .. } => Self::Projection,
            CoreRequest::ProjectionSubscribe { .. } => Self::Projection,
            CoreRequest::ProjectionUnsubscribe { .. } => Self::Projection,
            CoreRequest::AuditCapabilities => Self::Ops,
            CoreRequest::AuditExport { .. } => Self::Ops,
            CoreRequest::AuditQuery { .. } => Self::Ops,
            CoreRequest::MemoryForget { .. } => Self::Ops,
            CoreRequest::MemoryList { .. } => Self::Ops,
            CoreRequest::MemoryRemember { .. } => Self::Ops,
            CoreRequest::MemorySearch { .. } => Self::Ops,
            CoreRequest::NotificationSpeak { .. } => Self::Ops,
            CoreRequest::NotificationStop => Self::Ops,
            CoreRequest::ChatActionGet { .. } => Self::Chat,
            CoreRequest::ChatActionList { .. } => Self::Chat,
            CoreRequest::ChatActionSubmit { .. } => Self::Chat,
            CoreRequest::ChatCapabilities => Self::Chat,
            CoreRequest::ChatChannelEnsure { .. } => Self::Chat,
            CoreRequest::ChatChannelList { .. } => Self::Chat,
            CoreRequest::ChatComposingList { .. } => Self::Chat,
            CoreRequest::ChatComposingSet { .. } => Self::Chat,
            CoreRequest::ChatEdit { .. } => Self::Chat,
            CoreRequest::ChatHistory { .. } => Self::Chat,
            CoreRequest::ChatReadGet { .. } => Self::Chat,
            CoreRequest::ChatReadSet { .. } => Self::Chat,
            CoreRequest::ChatRedact { .. } => Self::Chat,
            CoreRequest::ChatSend { .. } => Self::Chat,
            CoreRequest::ChatSync { .. } => Self::Chat,
            CoreRequest::InteractiveProcessAttach { .. } => Self::Interactive,
            CoreRequest::InteractiveProcessCapabilities => Self::Interactive,
            CoreRequest::InteractiveProcessCreate { .. } => Self::Interactive,
            CoreRequest::InteractiveProcessDetach { .. } => Self::Interactive,
            CoreRequest::InteractiveProcessInput { .. } => Self::Interactive,
            CoreRequest::InteractiveProcessList { .. } => Self::Interactive,
            CoreRequest::InteractiveProcessRemove { .. } => Self::Interactive,
            CoreRequest::InteractiveProcessResize { .. } => Self::Interactive,
            CoreRequest::InteractiveProcessResume { .. } => Self::Interactive,
            CoreRequest::InteractiveProcessTerminate { .. } => Self::Interactive,
            CoreRequest::Initialize => Self::Turns,
        }
    }

    /// Owner module path for documentation and closure evidence.
    pub fn owner_module(&self) -> &'static str {
        match self {
            Self::Assets => "src/core/daemon_assets.rs",
            Self::Providers => "src/core/daemon_providers.rs",
            Self::Sessions => "src/core/daemon_sessions.rs",
            Self::Turns => "src/core/daemon_turns.rs",
            Self::Jobs => "src/core/daemon_jobs.rs",
            Self::Projects => "src/core/daemon_projects.rs",
            Self::Goals => "src/core/daemon_goals.rs",
            Self::Projection => "src/core/daemon_projection.rs",
            Self::Ops => "src/core/daemon_ops.rs",
            Self::Chat => "src/core/daemon.rs::handle_chat_request",
            Self::Interactive => "src/core/daemon.rs::run_interactive_request",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::core::{CoreRequest, CoreResponse};

    #[test]
    fn request_family_routes_each_coherent_family() {
        let cases: Vec<(CoreRequest, DaemonRequestFamily)> = vec![
            (
                CoreRequest::AssetRefreshCapabilities,
                DaemonRequestFamily::Assets,
            ),
            (
                CoreRequest::ProviderConnectionList,
                DaemonRequestFamily::Providers,
            ),
            (
                CoreRequest::SessionAttach {
                    session_id: "s".into(),
                },
                DaemonRequestFamily::Sessions,
            ),
            (
                CoreRequest::SessionLoad {
                    session_id: "s".into(),
                },
                DaemonRequestFamily::Sessions,
            ),
            (CoreRequest::ModelsRefresh, DaemonRequestFamily::Turns),
            (
                CoreRequest::Subscribe { session_id: None },
                DaemonRequestFamily::Turns,
            ),
            (CoreRequest::Initialize, DaemonRequestFamily::Turns),
            (CoreRequest::TaskList, DaemonRequestFamily::Jobs),
            (
                CoreRequest::ProjectCatalogCapabilities,
                DaemonRequestFamily::Projects,
            ),
            (
                CoreRequest::TodoList {
                    session_id: "s".into(),
                },
                DaemonRequestFamily::Goals,
            ),
            (
                CoreRequest::ProjectionCapabilities,
                DaemonRequestFamily::Projection,
            ),
            (
                CoreRequest::PresenceCapabilities,
                DaemonRequestFamily::Projection,
            ),
            (CoreRequest::AuditCapabilities, DaemonRequestFamily::Ops),
            (CoreRequest::ChatCapabilities, DaemonRequestFamily::Chat),
            (
                CoreRequest::InteractiveProcessCapabilities,
                DaemonRequestFamily::Interactive,
            ),
        ];
        for (request, expected) in &cases {
            assert_eq!(DaemonRequestFamily::of(request), *expected);
            assert!(!expected.owner_module().is_empty());
        }
    }

    /// The thin dispatcher must reach the owning family handler instead of
    /// falling through to the historical `unimplemented` contract. A
    /// pool-less daemon answers every capability probe below without durable
    /// state, so each response proves one family delegation path.
    #[tokio::test(flavor = "current_thread")]
    async fn thin_dispatcher_delegates_to_family_handlers() {
        use crate::core::daemon::CoreDaemon;
        let daemon = CoreDaemon::new(None, None, None);
        for payload in [
            CoreRequest::AssetRefreshCapabilities,
            CoreRequest::AuditCapabilities,
            CoreRequest::PresenceCapabilities,
            CoreRequest::ProjectionCapabilities,
            CoreRequest::ProjectCatalogCapabilities,
        ] {
            let response = daemon
                .handle_request(crate::core::new_request("req-family".into(), payload))
                .await
                .expect("dispatch succeeds");
            if let CoreResponse::Error { code, .. } = response {
                assert_ne!(
                    code, "unimplemented",
                    "router must reach the owning family handler"
                );
            }
        }
        // Jobs family: legacy tasks keep their explicit rejection contract.
        let response = daemon
            .handle_request(crate::core::new_request(
                "req-family".into(),
                CoreRequest::TaskList,
            ))
            .await
            .expect("dispatch succeeds");
        match response {
            CoreResponse::Error { code, .. } => assert_ne!(code, "unimplemented"),
            other => panic!("expected legacy task rejection, got {other:?}"),
        }
    }
}
