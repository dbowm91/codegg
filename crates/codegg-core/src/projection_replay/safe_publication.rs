use codegg_protocol::core::CoreEvent;
use codegg_protocol::projection::dto::VisibilityClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafePublicationClass {
    Safe,
    Internal,
    ClientLocal,
    Sensitive,
}

pub fn classify(event: &CoreEvent) -> SafePublicationClass {
    match event {
        CoreEvent::TurnStarted { .. } => SafePublicationClass::Safe,
        CoreEvent::TurnTextDelta { .. } => SafePublicationClass::Safe,
        CoreEvent::ToolStarted { .. } => SafePublicationClass::Safe,
        CoreEvent::ToolCompleted { .. } => SafePublicationClass::Safe,
        CoreEvent::PermissionPending { .. } => SafePublicationClass::Safe,
        CoreEvent::TurnCompleted { .. } => SafePublicationClass::Safe,
        CoreEvent::TurnFailed { .. } => SafePublicationClass::Safe,
        CoreEvent::SubagentStarted { .. } => SafePublicationClass::Safe,
        CoreEvent::SubagentProgress { .. } => SafePublicationClass::Safe,
        CoreEvent::SubagentCompleted { .. } => SafePublicationClass::Safe,
        CoreEvent::SubagentFailed { .. } => SafePublicationClass::Safe,
        CoreEvent::FileChanged { .. } => SafePublicationClass::Safe,
        CoreEvent::RunStarted { .. } => SafePublicationClass::Safe,
        CoreEvent::RunProgress { .. } => SafePublicationClass::Safe,
        CoreEvent::RunArtifactCreated { .. } => SafePublicationClass::Safe,
        CoreEvent::RunCompleted { .. } => SafePublicationClass::Safe,
        CoreEvent::RunDenied { .. } => SafePublicationClass::Safe,
        CoreEvent::TestRunStarted { .. } => SafePublicationClass::Safe,
        CoreEvent::TestRunProgress { .. } => SafePublicationClass::Safe,
        CoreEvent::TestRunCompleted { .. } => SafePublicationClass::Safe,
        CoreEvent::JobCreated { .. } => SafePublicationClass::Safe,
        CoreEvent::JobStarted { .. } => SafePublicationClass::Safe,
        CoreEvent::JobCompleted { .. } => SafePublicationClass::Safe,
        CoreEvent::JobFailed { .. } => SafePublicationClass::Safe,
        CoreEvent::AssetRefreshCompleted { .. } => SafePublicationClass::Safe,
        CoreEvent::SessionUpdated { .. } => SafePublicationClass::Safe,
        CoreEvent::ProjectRegistered { .. } => SafePublicationClass::Safe,
        CoreEvent::ProjectRestored { .. } => SafePublicationClass::Safe,
        CoreEvent::TurnReasoningDelta { .. } => SafePublicationClass::Internal,
        CoreEvent::ConnectionRotated { .. } => SafePublicationClass::Sensitive,
        CoreEvent::ConnectionStateChanged { .. } => SafePublicationClass::Sensitive,
        CoreEvent::Error { .. } => SafePublicationClass::Safe,
        CoreEvent::SnapshotSession { .. } => SafePublicationClass::Safe,
        CoreEvent::SnapshotWorkspace { .. } => SafePublicationClass::Safe,
        CoreEvent::SnapshotModels { .. } => SafePublicationClass::Safe,
        CoreEvent::ProjectArchived { .. } => SafePublicationClass::Safe,
        CoreEvent::ProjectHealthChanged { .. } => SafePublicationClass::Safe,
        CoreEvent::PluginUiEffect { .. } => SafePublicationClass::Internal,
        CoreEvent::JobQueued { .. } => SafePublicationClass::Safe,
        CoreEvent::JobBlocked { .. } => SafePublicationClass::Safe,
        CoreEvent::JobAttemptCreated { .. } => SafePublicationClass::Safe,
        CoreEvent::JobProgress { .. } => SafePublicationClass::Safe,
        CoreEvent::JobCancelRequested { .. } => SafePublicationClass::Safe,
        CoreEvent::JobCancelled { .. } => SafePublicationClass::Safe,
        CoreEvent::JobTimedOut { .. } => SafePublicationClass::Safe,
        CoreEvent::JobInterrupted { .. } => SafePublicationClass::Safe,
        CoreEvent::JobRetried { .. } => SafePublicationClass::Safe,
        CoreEvent::ScheduleCreated { .. } => SafePublicationClass::Safe,
        CoreEvent::ScheduleOccurrenceQueued { .. } => SafePublicationClass::Safe,
        CoreEvent::ScheduleSkipped { .. } => SafePublicationClass::Safe,
        CoreEvent::SchedulePaused { .. } => SafePublicationClass::Safe,
        CoreEvent::ScheduleResumed { .. } => SafePublicationClass::Safe,
        CoreEvent::ScheduleDeleted { .. } => SafePublicationClass::Safe,
        CoreEvent::RunPinned { .. } => SafePublicationClass::Safe,
        CoreEvent::ContextPromotionChanged { .. } => SafePublicationClass::Safe,
        CoreEvent::RunRerunLinked { .. } => SafePublicationClass::Safe,
        CoreEvent::RunProjectionReady { .. } => SafePublicationClass::Safe,
        CoreEvent::ProjectionStreamEvent { .. } => SafePublicationClass::Internal,
        CoreEvent::QuestionPending { .. } => SafePublicationClass::Safe,
    }
}

pub fn visibility_for_class(class: SafePublicationClass) -> VisibilityClass {
    match class {
        SafePublicationClass::Safe => VisibilityClass::Public,
        SafePublicationClass::Internal => VisibilityClass::Internal,
        SafePublicationClass::ClientLocal => VisibilityClass::ClientLocal,
        SafePublicationClass::Sensitive => VisibilityClass::Sensitive,
    }
}

pub fn is_persistent(class: SafePublicationClass) -> bool {
    matches!(class, SafePublicationClass::Safe)
}

pub fn has_safe_origin(event: &CoreEvent) -> bool {
    match event {
        CoreEvent::TurnStarted { session_id, .. } => !session_id.is_empty(),
        CoreEvent::TurnTextDelta { session_id, .. } => !session_id.is_empty(),
        CoreEvent::ToolStarted { session_id, .. } => !session_id.is_empty(),
        CoreEvent::ToolCompleted { session_id, .. } => !session_id.is_empty(),
        CoreEvent::PermissionPending { session_id, .. } => !session_id.is_empty(),
        CoreEvent::TurnCompleted { session_id, .. } => !session_id.is_empty(),
        CoreEvent::TurnFailed { session_id, .. } => !session_id.is_empty(),
        CoreEvent::SubagentStarted { session_id, .. } => !session_id.is_empty(),
        CoreEvent::SubagentProgress { session_id, .. } => !session_id.is_empty(),
        CoreEvent::SubagentCompleted { session_id, .. } => !session_id.is_empty(),
        CoreEvent::SubagentFailed { session_id, .. } => !session_id.is_empty(),
        CoreEvent::RunStarted { session_id, .. } => !session_id.is_empty(),
        CoreEvent::RunProgress { session_id, .. } => !session_id.is_empty(),
        CoreEvent::RunArtifactCreated { session_id, .. } => !session_id.is_empty(),
        CoreEvent::RunCompleted { session_id, .. } => !session_id.is_empty(),
        CoreEvent::RunDenied { session_id, .. } => !session_id.is_empty(),
        CoreEvent::TestRunStarted { session_id, .. } => !session_id.is_empty(),
        CoreEvent::TestRunProgress { session_id, .. } => !session_id.is_empty(),
        CoreEvent::TestRunCompleted { session_id, .. } => !session_id.is_empty(),
        CoreEvent::SessionUpdated { session_id, .. } => !session_id.is_empty(),
        CoreEvent::FileChanged { .. } => true,
        CoreEvent::JobCreated { session_id, .. } => session_id.is_some(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_core_event_variants_classified() {
        let events = vec![
            CoreEvent::TurnStarted {
                session_id: "s".into(),
                turn_id: "t".into(),
            },
            CoreEvent::TurnTextDelta {
                session_id: "s".into(),
                turn_id: "t".into(),
                delta: "x".into(),
            },
            CoreEvent::TurnReasoningDelta {
                session_id: "s".into(),
                turn_id: "t".into(),
                delta: "r".into(),
            },
            CoreEvent::ToolStarted {
                session_id: "s".into(),
                turn_id: None,
                tool_name: "bash".into(),
                tool_id: "t1".into(),
                arguments: "{}".into(),
            },
            CoreEvent::ToolCompleted {
                session_id: "s".into(),
                turn_id: None,
                tool_id: "t1".into(),
                output: "ok".into(),
                success: true,
            },
            CoreEvent::PermissionPending {
                id: "p1".into(),
                session_id: "s".into(),
                turn_id: None,
                tool: "bash".into(),
                path: None,
            },
            CoreEvent::TurnCompleted {
                session_id: "s".into(),
                turn_id: "t".into(),
                stop_reason: "end_turn".into(),
            },
            CoreEvent::TurnFailed {
                session_id: "s".into(),
                turn_id: None,
                message: "err".into(),
            },
            CoreEvent::SubagentStarted {
                session_id: "s".into(),
                task_id: 1,
                agent: "a".into(),
                description: "d".into(),
            },
            CoreEvent::SubagentProgress {
                session_id: "s".into(),
                task_id: 1,
                agent: "a".into(),
                message: "m".into(),
            },
            CoreEvent::SubagentCompleted {
                session_id: "s".into(),
                task_id: 1,
                agent: "a".into(),
                result_summary: "r".into(),
            },
            CoreEvent::SubagentFailed {
                session_id: "s".into(),
                task_id: 1,
                agent: "a".into(),
                error: "e".into(),
            },
            CoreEvent::FileChanged {
                path: "f".into(),
                action: "modified".into(),
            },
            CoreEvent::RunStarted {
                session_id: "s".into(),
                run_id: "r".into(),
                kind: "test".into(),
                command: "cargo test".into(),
            },
            CoreEvent::RunProgress {
                session_id: "s".into(),
                run_id: "r".into(),
                message: "m".into(),
            },
            CoreEvent::RunArtifactCreated {
                session_id: "s".into(),
                run_id: "r".into(),
                artifact_id: "a".into(),
                kind: "log".into(),
                byte_length: 100,
            },
            CoreEvent::RunCompleted {
                session_id: "s".into(),
                run_id: "r".into(),
                status: "ok".into(),
                summary: "s".into(),
            },
            CoreEvent::RunDenied {
                session_id: "s".into(),
                run_id: "r".into(),
                reason: "denied".into(),
            },
            CoreEvent::TestRunStarted {
                session_id: "s".into(),
                job_id: "j".into(),
                command: "cargo test".into(),
                cwd: "/tmp".into(),
            },
            CoreEvent::TestRunProgress {
                session_id: "s".into(),
                job_id: "j".into(),
                message: "m".into(),
            },
            CoreEvent::TestRunCompleted {
                session_id: "s".into(),
                job_id: "j".into(),
                status: "ok".into(),
                summary: "s".into(),
                log_dir: None,
            },
            CoreEvent::JobCreated {
                job_id: "j".into(),
                workspace_id: "w".into(),
                kind: "test".into(),
                session_id: Some("s".into()),
                turn_id: None,
            },
            CoreEvent::JobStarted {
                job_id: "j".into(),
                attempt_id: "a".into(),
            },
            CoreEvent::JobCompleted {
                job_id: "j".into(),
                attempt_id: "a".into(),
            },
            CoreEvent::JobFailed {
                job_id: "j".into(),
                attempt_id: "a".into(),
                error_class: "e".into(),
                message: "m".into(),
            },
            CoreEvent::AssetRefreshCompleted {
                report: codegg_protocol::core::AssetRefreshReportDto {
                    scope: codegg_protocol::core::AssetRefreshScopeDto {
                        project_id: "p".into(),
                        workspace_id: "w".into(),
                    },
                    reason: codegg_protocol::core::AssetRefreshReasonDto::Manual,
                    outcome: codegg_protocol::core::AssetRefreshOutcomeDto::Published,
                    generation: None,
                    previous_generation: None,
                    fingerprint: None,
                    added: vec![],
                    removed: vec![],
                    changed: vec![],
                    shadowed: vec![],
                    invalid: vec![],
                    retained: vec![],
                    diagnostics: vec![],
                    coalesced: false,
                    completed_at_ms: 0,
                },
            },
            CoreEvent::SessionUpdated {
                session_id: "s".into(),
            },
            CoreEvent::ProjectRegistered {
                project_id: "p".into(),
                project: codegg_protocol::dto::ProjectSummaryDto {
                    project_id: "p".into(),
                    display_name: "p".into(),
                    lifecycle: "active".into(),
                    description: None,
                    tags: vec![],
                    time_last_opened_at: None,
                    registration_source: "test".into(),
                    archived_at: None,
                    created_at: 0,
                    updated_at: 0,
                },
            },
            CoreEvent::ProjectRestored {
                project_id: "p".into(),
                project: codegg_protocol::dto::ProjectSummaryDto {
                    project_id: "p".into(),
                    display_name: "p".into(),
                    lifecycle: "active".into(),
                    description: None,
                    tags: vec![],
                    time_last_opened_at: None,
                    registration_source: "test".into(),
                    archived_at: None,
                    created_at: 0,
                    updated_at: 0,
                },
            },
            CoreEvent::ConnectionRotated {
                connection_id: "c".into(),
                new_revision: 2,
                catalog_revision: None,
                actor_seam: "test".into(),
            },
            CoreEvent::ConnectionStateChanged {
                connection_id: "c".into(),
                old_state: "active".into(),
                new_state: "disabled".into(),
                actor_seam: "test".into(),
                at: 0,
            },
            CoreEvent::Error {
                code: "e".into(),
                message: "m".into(),
            },
            CoreEvent::SnapshotSession {
                session_id: "s".into(),
            },
            CoreEvent::SnapshotWorkspace {
                project_dir: "/tmp".into(),
            },
            CoreEvent::SnapshotModels {
                current_model: None,
                models: vec![],
            },
            CoreEvent::ProjectArchived {
                project_id: "p".into(),
                project: codegg_protocol::dto::ProjectSummaryDto {
                    project_id: "p".into(),
                    display_name: "p".into(),
                    lifecycle: "archived".into(),
                    description: None,
                    tags: vec![],
                    time_last_opened_at: None,
                    registration_source: "test".into(),
                    archived_at: None,
                    created_at: 0,
                    updated_at: 0,
                },
            },
            CoreEvent::ProjectHealthChanged {
                project_id: "p".into(),
                workspace_id: "w".into(),
                health: codegg_protocol::dto::ProjectHealthDto {
                    project_id: "p".into(),
                    workspace_id: "w".into(),
                    overall: "available".into(),
                    catalog: codegg_protocol::dto::ProjectHealthLayerDto {
                        state: "ok".into(),
                        code: None,
                        message: None,
                    },
                    workspace: codegg_protocol::dto::ProjectHealthLayerDto {
                        state: "ok".into(),
                        code: None,
                        message: None,
                    },
                    assets: codegg_protocol::dto::ProjectHealthLayerDto {
                        state: "ok".into(),
                        code: None,
                        message: None,
                    },
                    services: codegg_protocol::dto::ProjectHealthLayerDto {
                        state: "ok".into(),
                        code: None,
                        message: None,
                    },
                    diagnostics: vec![],
                    durable: None,
                },
            },
            CoreEvent::QuestionPending {
                id: "q".into(),
                session_id: "s".into(),
                turn_id: None,
                questions: serde_json::json!({"header":"h","prompt":"p"}),
            },
            CoreEvent::JobQueued {
                job_id: "j".into(),
                workspace_id: "w".into(),
            },
            CoreEvent::JobBlocked {
                job_id: "j".into(),
                workspace_id: "w".into(),
            },
            CoreEvent::JobAttemptCreated {
                job_id: "j".into(),
                attempt_id: "a".into(),
                sequence: 1,
                daemon_generation: "g".into(),
            },
            CoreEvent::JobProgress {
                job_id: "j".into(),
                attempt_id: "a".into(),
                message: "m".into(),
            },
            CoreEvent::JobCancelRequested {
                job_id: "j".into(),
                reason: "r".into(),
            },
            CoreEvent::JobCancelled {
                job_id: "j".into(),
                attempt_id: "a".into(),
            },
            CoreEvent::JobTimedOut {
                job_id: "j".into(),
                attempt_id: "a".into(),
            },
            CoreEvent::JobInterrupted {
                job_id: "j".into(),
                attempt_id: "a".into(),
                recovery_generation: "g".into(),
            },
            CoreEvent::JobRetried {
                job_id: "j".into(),
                new_attempt_id: "a2".into(),
                prior_attempt_id: "a1".into(),
            },
            CoreEvent::ScheduleCreated {
                schedule_id: "sch".into(),
                workspace_id: "w".into(),
                kind_summary: "k".into(),
            },
            CoreEvent::ScheduleOccurrenceQueued {
                schedule_id: "sch".into(),
                scheduled_for_ms: 0,
                job_id: "j".into(),
            },
            CoreEvent::ScheduleSkipped {
                schedule_id: "sch".into(),
                scheduled_for_ms: 0,
                reason: "r".into(),
            },
            CoreEvent::SchedulePaused {
                schedule_id: "sch".into(),
            },
            CoreEvent::ScheduleResumed {
                schedule_id: "sch".into(),
            },
            CoreEvent::ScheduleDeleted {
                schedule_id: "sch".into(),
            },
            CoreEvent::RunPinned {
                run_id: "r".into(),
                pinned: true,
            },
            CoreEvent::ContextPromotionChanged {
                session_id: "s".into(),
                run_id: "r".into(),
                state: "promoted".into(),
            },
            CoreEvent::RunRerunLinked {
                session_id: "s".into(),
                parent_run_id: "r1".into(),
                child_run_id: "r2".into(),
            },
            CoreEvent::RunProjectionReady {
                session_id: "s".into(),
                run_id: "r".into(),
                projector: "p".into(),
                exactness: "exact".into(),
            },
            CoreEvent::ProjectionStreamEvent {
                subscription_id: codegg_protocol::projection::replay::ProjectionSubscriptionId(
                    "sub".into(),
                ),
                stream_id: codegg_protocol::projection::replay::ProjectionStreamId("s".into()),
                envelope: codegg_protocol::projection::event::ProjectionEnvelope::session_event(
                    1,
                    0,
                    "s",
                    None,
                    codegg_protocol::projection::event::ProjectionEvent::Diagnostic {
                        code: "d".into(),
                        message: "m".into(),
                    },
                ),
            },
        ];

        for event in &events {
            let class = classify(event);
            assert!(
                matches!(
                    class,
                    SafePublicationClass::Safe
                        | SafePublicationClass::Internal
                        | SafePublicationClass::ClientLocal
                        | SafePublicationClass::Sensitive
                ),
                "event not classified: {:?}",
                event
            );
        }
    }

    #[test]
    fn internal_events_never_persistent() {
        let events = vec![
            CoreEvent::TurnReasoningDelta {
                session_id: "s".into(),
                turn_id: "t".into(),
                delta: "x".into(),
            },
            CoreEvent::ProjectionStreamEvent {
                subscription_id: codegg_protocol::projection::replay::ProjectionSubscriptionId(
                    "sub".into(),
                ),
                stream_id: codegg_protocol::projection::replay::ProjectionStreamId("s".into()),
                envelope: codegg_protocol::projection::event::ProjectionEnvelope::session_event(
                    1,
                    0,
                    "s",
                    None,
                    codegg_protocol::projection::event::ProjectionEvent::Diagnostic {
                        code: "d".into(),
                        message: "m".into(),
                    },
                ),
            },
        ];
        for event in &events {
            assert!(!is_persistent(classify(event)));
        }
    }

    #[test]
    fn sensitive_events_never_persistent() {
        let events = vec![
            CoreEvent::ConnectionRotated {
                connection_id: "c".into(),
                new_revision: 2,
                catalog_revision: None,
                actor_seam: "test".into(),
            },
            CoreEvent::ConnectionStateChanged {
                connection_id: "c".into(),
                old_state: "active".into(),
                new_state: "disabled".into(),
                actor_seam: "test".into(),
                at: 0,
            },
        ];
        for event in &events {
            assert!(!is_persistent(classify(event)));
        }
    }
}
