/// Core protocol conversion helpers for session, message, and provider types.
///
/// Agent-related conversions remain in root `src/protocol_conversions.rs`
/// because the agent module is not extracted into `codegg-core`.
///
/// # Transitional Notes
///
/// These conversions intentionally live in `codegg-core` rather than in
/// `codegg-protocol`. The protocol crate must remain a thin, dependency-free
/// DTO layer; it must not depend on domain or runtime crates such as
/// `codegg-core`, `codegg-providers`, or `codegg-config`.
///
/// Every conversion currently round-trips through `serde_json::Value`. This
/// works because the domain types and DTOs share the same serde attributes,
/// but it is a **transitional compatibility bridge**, not the intended
/// long-term pattern. In a future cleanup pass, prefer explicit
/// `From`/`TryFrom` implementations that avoid the intermediate JSON
/// serialization and provide better compile-time error messages.
// ── Domain → DTO (for constructing protocol responses / requests) ───────
pub fn session_to_dto(s: crate::session::Session) -> codegg_protocol::dto::Session {
    let json = serde_json::to_value(&s).expect("session::Session is always serializable");
    serde_json::from_value(json)
        .expect("dto::Session is always deserializable from session::Session")
}

pub fn message_to_dto(m: crate::session::message::Message) -> codegg_protocol::dto::Message {
    let json = serde_json::to_value(&m).expect("message::Message is always serializable");
    serde_json::from_value(json)
        .expect("dto::Message is always deserializable from message::Message")
}

pub fn provider_message_to_dto(
    m: codegg_providers::Message,
) -> codegg_protocol::dto::ProviderMessage {
    let json = serde_json::to_value(&m).expect("provider::Message is always serializable");
    serde_json::from_value(json)
        .expect("dto::ProviderMessage is always deserializable from provider::Message")
}

pub fn session_template_to_dto(
    t: codegg_config::schema::SessionTemplate,
) -> codegg_protocol::dto::SessionTemplate {
    let json = serde_json::to_value(&t).expect("SessionTemplate is always serializable");
    serde_json::from_value(json)
        .expect("dto::SessionTemplate is always deserializable from SessionTemplate")
}

pub fn sessions_to_dtos(
    sessions: Vec<crate::session::Session>,
) -> Vec<codegg_protocol::dto::Session> {
    sessions.into_iter().map(session_to_dto).collect()
}

pub fn messages_to_dtos(
    messages: Vec<crate::session::message::Message>,
) -> Vec<codegg_protocol::dto::Message> {
    messages.into_iter().map(message_to_dto).collect()
}

pub fn provider_messages_to_dtos(
    messages: Vec<codegg_providers::Message>,
) -> Vec<codegg_protocol::dto::ProviderMessage> {
    messages.into_iter().map(provider_message_to_dto).collect()
}

// ── DTO → Domain (for consuming protocol responses in the application) ─

pub fn dto_to_session(s: codegg_protocol::dto::Session) -> crate::session::Session {
    let json = serde_json::to_value(&s).expect("dto::Session is always serializable");
    serde_json::from_value(json)
        .expect("session::Session is always deserializable from dto::Session")
}

pub fn dto_to_message(m: codegg_protocol::dto::Message) -> crate::session::message::Message {
    let json = serde_json::to_value(&m).expect("dto::Message is always serializable");
    serde_json::from_value(json)
        .expect("message::Message is always deserializable from dto::Message")
}

pub fn dto_to_provider_message(
    m: codegg_protocol::dto::ProviderMessage,
) -> codegg_providers::Message {
    let json = serde_json::to_value(&m).expect("dto::ProviderMessage is always serializable");
    serde_json::from_value(json)
        .expect("provider::Message is always deserializable from dto::ProviderMessage")
}

pub fn dto_to_session_template(
    t: codegg_protocol::dto::SessionTemplate,
) -> codegg_config::schema::SessionTemplate {
    let json = serde_json::to_value(&t).expect("dto::SessionTemplate is always serializable");
    serde_json::from_value(json)
        .expect("SessionTemplate is always deserializable from dto::SessionTemplate")
}

pub fn dtos_to_sessions(
    sessions: Vec<codegg_protocol::dto::Session>,
) -> Vec<crate::session::Session> {
    sessions.into_iter().map(dto_to_session).collect()
}

pub fn dtos_to_messages(
    messages: Vec<codegg_protocol::dto::Message>,
) -> Vec<crate::session::message::Message> {
    messages.into_iter().map(dto_to_message).collect()
}

pub fn dtos_to_provider_messages(
    messages: Vec<codegg_protocol::dto::ProviderMessage>,
) -> Vec<codegg_providers::Message> {
    messages.into_iter().map(dto_to_provider_message).collect()
}

/// Convert a registered `WorkspaceRecord` into the wire DTO. The active
/// session count is provided by the daemon snapshot builder because the
/// record itself does not own a session index.
pub fn workspace_record_to_dto(
    record: &crate::workspace::WorkspaceRecord,
    active_sessions: usize,
) -> codegg_protocol::dto::WorkspaceSnapshot {
    codegg_protocol::dto::WorkspaceSnapshot {
        workspace_id: record.id.as_str().to_string(),
        canonical_root: record.canonical_root.to_string_lossy().into_owned(),
        display_name: record.display_name.clone(),
        created_at: record.created_at.timestamp_millis(),
        last_opened_at: record.last_opened_at.timestamp_millis(),
        archived_at: record.archived_at.map(|d| d.timestamp_millis()),
        active_sessions,
        services_active: false,
        active_leases: 0,
        config_revision: 0,
    }
}

/// Convert a registered `WorkspaceRecord` plus an active
/// `WorkspaceServiceSnapshot` into the wire DTO. Used by the
/// `WorkspaceList` handler so clients see service health inline with
/// the workspace metadata.
pub fn workspace_record_with_services_to_dto(
    record: &crate::workspace::WorkspaceRecord,
    active_sessions: usize,
    service: Option<&crate::workspace_services::WorkspaceServiceSnapshot>,
) -> codegg_protocol::dto::WorkspaceSnapshot {
    let mut dto = workspace_record_to_dto(record, active_sessions);
    if let Some(snap) = service {
        dto.services_active = true;
        dto.active_leases = snap.active_leases;
        dto.config_revision = snap.config_revision;
    }
    dto
}

/// Phase 3: convert a workspace service snapshot to the wire DTO.
pub fn workspace_service_snapshot_to_dto(
    snap: &crate::workspace_services::WorkspaceServiceSnapshot,
) -> codegg_protocol::dto::WorkspaceServiceHealthDto {
    codegg_protocol::dto::WorkspaceServiceHealthDto {
        workspace_id: snap.workspace_id.as_str().to_string(),
        canonical_root: snap.canonical_root.to_string_lossy().into_owned(),
        display_name: snap.display_name.clone(),
        activated_at: snap.activated_at.timestamp_millis(),
        last_used_at: snap.last_used_at.timestamp_millis(),
        active_leases: snap.active_leases,
        config_revision: snap.config_revision,
    }
}

/// Phase 3: convert a config diagnostic to the wire DTO.
pub fn config_diagnostic_to_dto(
    diag: &crate::workspace_services::ConfigDiagnostic,
) -> codegg_protocol::dto::ConfigDiagnosticDto {
    let severity = match diag.severity {
        crate::workspace_services::ConfigDiagnosticSeverity::Warning => "warning",
        crate::workspace_services::ConfigDiagnosticSeverity::Error => "error",
    };
    codegg_protocol::dto::ConfigDiagnosticDto {
        severity: severity.to_string(),
        source: diag.source.to_string_lossy().into_owned(),
        message: diag.message.clone(),
    }
}

/// Phase 3: convert a `RunQueryDto` into the workspace-local
/// `RunStore::list_runs` query. Filters are intentionally loose; the
/// store applies them in-memory and returns matches in start order.
pub fn run_query_from_dto(query: codegg_protocol::dto::RunQueryDto) -> crate::run_store::RunQuery {
    let kind = query.kinds.into_iter().find_map(|s| run_kind_from_str(&s));
    let status = query
        .statuses
        .into_iter()
        .find_map(|s| run_status_from_str(&s));
    let since = query
        .since_ms
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis);
    let until = query
        .until_ms
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis);
    let limit = if query.limit == 0 {
        Some(100)
    } else {
        Some(query.limit)
    };
    crate::run_store::RunQuery {
        kind,
        status,
        session_id: None,
        since,
        until,
        limit,
    }
}

fn run_kind_from_str(s: &str) -> Option<crate::run_store::RunKind> {
    use crate::run_store::RunKind;
    Some(match s {
        "raw_shell" => RunKind::RawShell,
        "managed_process" => RunKind::ManagedProcess,
        "test" => RunKind::Test,
        "git_read" => RunKind::GitRead,
        "git_mutation" => RunKind::GitMutation,
        "search" => RunKind::Search,
        "python" => RunKind::Python,
        "native_tool" => RunKind::NativeTool,
        _ => return None,
    })
}

fn run_status_from_str(s: &str) -> Option<crate::run_store::RunStatus> {
    use crate::run_store::RunStatus;
    Some(match s {
        "running" => RunStatus::Running,
        "complete" => RunStatus::Complete,
        "failed" => RunStatus::Failed,
        "timed_out" => RunStatus::TimedOut,
        "cancelled" => RunStatus::Cancelled,
        "incomplete" => RunStatus::Incomplete,
        _ => return None,
    })
}

/// Phase 3: convert a run summary into the wire DTO.
pub fn run_summary_to_dto(
    summary: &crate::run_store::RunSummary,
    workspace_id: Option<&str>,
) -> codegg_protocol::dto::RunSummaryDto {
    codegg_protocol::dto::RunSummaryDto {
        run_id: summary.run_id.as_str().to_string(),
        kind: run_kind_to_str(&summary.kind).to_string(),
        status: run_status_to_str(&summary.status).to_string(),
        started_at_ms: summary.started_at.timestamp_millis(),
        completed_at_ms: summary.completed_at.map(|d| d.timestamp_millis()),
        command: summary.command.clone(),
        workspace_id: workspace_id.map(|s| s.to_string()),
    }
}

/// Phase 3: convert a full run manifest into the wire DTO.
pub fn run_manifest_to_dto(
    manifest: &crate::run_store::RunManifest,
    workspace_id: Option<&str>,
) -> codegg_protocol::dto::RunRecordDto {
    codegg_protocol::dto::RunRecordDto {
        run_id: manifest.run_id.as_str().to_string(),
        kind: run_kind_to_str(&manifest.kind).to_string(),
        status: run_status_to_str(&manifest.status).to_string(),
        started_at_ms: manifest.started_at.timestamp_millis(),
        completed_at_ms: manifest.completed_at.map(|d| d.timestamp_millis()),
        command: manifest.invocation.command.clone(),
        argv: manifest.invocation.argv.clone().unwrap_or_default(),
        backend_family: Some(manifest.backend.family.clone()),
        backend_detail: manifest.backend.detail.clone(),
        workspace_id: workspace_id.map(|s| s.to_string()),
        artifacts: manifest
            .artifacts
            .iter()
            .map(|a| codegg_protocol::dto::RunArtifactSummaryDto {
                artifact_id: a.artifact_id.as_str().to_string(),
                kind: artifact_kind_to_str(&a.kind).to_string(),
                size: a.byte_length,
                sha256: Some(a.sha256.clone()),
            })
            .collect(),
    }
}

fn run_kind_to_str(k: &crate::run_store::RunKind) -> &'static str {
    use crate::run_store::RunKind;
    match k {
        RunKind::RawShell => "raw_shell",
        RunKind::ManagedProcess => "managed_process",
        RunKind::Test => "test",
        RunKind::GitRead => "git_read",
        RunKind::GitMutation => "git_mutation",
        RunKind::Search => "search",
        RunKind::Python => "python",
        RunKind::NativeTool => "native_tool",
    }
}

fn run_status_to_str(s: &crate::run_store::RunStatus) -> &'static str {
    use crate::run_store::RunStatus;
    match s {
        RunStatus::Running => "running",
        RunStatus::Complete => "complete",
        RunStatus::Failed => "failed",
        RunStatus::TimedOut => "timed_out",
        RunStatus::Cancelled => "cancelled",
        RunStatus::Incomplete => "incomplete",
    }
}

fn artifact_kind_to_str(k: &crate::run_store::ArtifactKind) -> &'static str {
    use crate::run_store::ArtifactKind;
    match k {
        ArtifactKind::Stdout => "stdout",
        ArtifactKind::Stderr => "stderr",
        ArtifactKind::CombinedLog => "combined_log",
        ArtifactKind::CommandSource => "command_source",
        ArtifactKind::TestReport => "test_report",
        ArtifactKind::TestLog => "test_log",
        ArtifactKind::UnifiedDiff => "unified_diff",
        ArtifactKind::ChangedFiles => "changed_files",
        ArtifactKind::Projection => "projection",
        ArtifactKind::RtkProjection => "rtk_projection",
        ArtifactKind::StructuredJson => "structured_json",
        ArtifactKind::PolicyEvidence => "policy_evidence",
    }
}

// ── Phase 4: Durable Jobs and Schedules conversion helpers ───────────

pub fn job_kind_to_str(k: crate::jobs::JobKind) -> &'static str {
    k.as_str()
}

pub fn job_kind_from_str(s: &str) -> crate::jobs::JobKind {
    crate::jobs::JobKind::from_str_lossy(s)
}

pub fn job_priority_to_str(p: crate::jobs::JobPriority) -> &'static str {
    p.as_str()
}

pub fn job_priority_from_str(s: &str) -> crate::jobs::JobPriority {
    crate::jobs::JobPriority::from_str_lossy(s)
}

pub fn job_state_to_str(s: crate::jobs::JobState) -> &'static str {
    s.as_str()
}

pub fn job_state_from_str(s: &str) -> crate::jobs::JobState {
    crate::jobs::JobState::from_str_lossy(s)
}

pub fn attempt_state_to_str(s: crate::jobs::AttemptState) -> &'static str {
    s.as_str()
}

pub fn attempt_state_from_str(s: &str) -> crate::jobs::AttemptState {
    crate::jobs::AttemptState::from_str_lossy(s)
}

pub fn idempotency_to_str(i: crate::jobs::IdempotencyClass) -> &'static str {
    match i {
        crate::jobs::IdempotencyClass::ReadOnly => "read_only",
        crate::jobs::IdempotencyClass::SafeRepeat => "safe_repeat",
        crate::jobs::IdempotencyClass::Conditional => "conditional",
        crate::jobs::IdempotencyClass::NonIdempotent => "non_idempotent",
        crate::jobs::IdempotencyClass::Destructive => "destructive",
    }
}

pub fn idempotency_from_str(s: &str) -> Result<crate::jobs::IdempotencyClass, String> {
    match s {
        "read_only" => Ok(crate::jobs::IdempotencyClass::ReadOnly),
        "safe_repeat" => Ok(crate::jobs::IdempotencyClass::SafeRepeat),
        "conditional" => Ok(crate::jobs::IdempotencyClass::Conditional),
        "non_idempotent" => Ok(crate::jobs::IdempotencyClass::NonIdempotent),
        "destructive" => Ok(crate::jobs::IdempotencyClass::Destructive),
        _ => Err(format!("unknown idempotency class: {s}")),
    }
}

pub fn schedule_state_to_str(s: crate::jobs::ScheduleState) -> &'static str {
    s.as_str()
}

pub fn schedule_state_from_str(s: &str) -> crate::jobs::ScheduleState {
    crate::jobs::ScheduleState::from_str_lossy(s)
}

pub fn overlap_policy_to_str(p: crate::jobs::OverlapPolicy) -> &'static str {
    match p {
        crate::jobs::OverlapPolicy::SkipIfRunning => "skip_if_running",
        crate::jobs::OverlapPolicy::QueueOne => "queue_one",
        crate::jobs::OverlapPolicy::Allow => "allow",
    }
}

pub fn overlap_policy_from_str(s: &str) -> Result<crate::jobs::OverlapPolicy, String> {
    match s {
        "skip_if_running" => Ok(crate::jobs::OverlapPolicy::SkipIfRunning),
        "queue_one" => Ok(crate::jobs::OverlapPolicy::QueueOne),
        "allow" => Ok(crate::jobs::OverlapPolicy::Allow),
        _ => Err(format!("unknown overlap policy: {s}")),
    }
}

pub fn cancel_outcome_to_str(o: crate::jobs::CancelOutcome) -> &'static str {
    match o {
        crate::jobs::CancelOutcome::Cancelled => "cancelled",
        crate::jobs::CancelOutcome::Requested => "requested",
        crate::jobs::CancelOutcome::AlreadyTerminal => "already_terminal",
    }
}

pub fn missed_run_policy_to_str(p: &crate::jobs::MissedRunPolicy) -> serde_json::Value {
    serde_json::to_value(p).expect("MissedRunPolicy is always serializable")
}

pub fn schedule_kind_to_value(k: &crate::jobs::ScheduleKind) -> serde_json::Value {
    serde_json::to_value(k).expect("ScheduleKind is always serializable")
}

pub fn job_source_to_value(s: &crate::jobs::JobSource) -> serde_json::Value {
    serde_json::to_value(s).expect("JobSource is always serializable")
}

pub fn job_payload_to_value(p: &crate::jobs::JobPayload) -> serde_json::Value {
    serde_json::to_value(p).expect("JobPayload is always serializable")
}

pub fn retry_policy_to_value(p: &crate::jobs::RetryPolicy) -> serde_json::Value {
    serde_json::to_value(p).expect("RetryPolicy is always serializable")
}

pub fn job_template_to_value(t: &crate::jobs::schedule::JobTemplate) -> serde_json::Value {
    serde_json::to_value(t).expect("JobTemplate is always serializable")
}

pub fn missed_run_policy_from_value(
    v: &serde_json::Value,
) -> Result<crate::jobs::MissedRunPolicy, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("invalid missed_run_policy: {e}"))
}

pub fn schedule_kind_from_value(
    v: &serde_json::Value,
) -> Result<crate::jobs::ScheduleKind, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("invalid schedule kind: {e}"))
}

pub fn job_source_from_value(v: &serde_json::Value) -> Result<crate::jobs::JobSource, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("invalid job source: {e}"))
}

pub fn job_payload_from_value(v: &serde_json::Value) -> Result<crate::jobs::JobPayload, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("invalid job payload: {e}"))
}

pub fn retry_policy_from_value(v: &serde_json::Value) -> Result<crate::jobs::RetryPolicy, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("invalid retry policy: {e}"))
}

pub fn job_template_from_value(
    v: &serde_json::Value,
) -> Result<crate::jobs::schedule::JobTemplate, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("invalid job template: {e}"))
}

/// Domain → DTO: convert a `JobSummary` into the wire DTO.
pub fn job_summary_to_dto(
    s: &crate::jobs::store::JobSummary,
) -> codegg_protocol::dto::JobSummaryDto {
    codegg_protocol::dto::JobSummaryDto {
        job_id: s.job_id.as_str().to_string(),
        workspace_id: s.workspace_id.as_str().to_string(),
        session_id: None, // populated by daemon handler from labels/context
        kind: job_kind_to_str(s.kind).to_string(),
        priority: job_priority_to_str(s.priority).to_string(),
        state: job_state_to_str(s.state).to_string(),
        attempt_count: s.attempt_count,
        current_attempt_id: s
            .current_attempt_id
            .as_ref()
            .map(|a| a.as_str().to_string()),
        schedule_id: s.schedule_id.as_ref().map(|sid| sid.as_str().to_string()),
        cancel_requested_at: s.cancel_requested_at.map(|d| d.timestamp_millis()),
        created_at_ms: s.created_at.timestamp_millis(),
        updated_at_ms: s.updated_at.timestamp_millis(),
        labels: std::collections::HashMap::new(), // populated by daemon handler
    }
}

/// Domain → DTO: convert a `JobRecord` into the wire DTO.
pub fn job_record_to_dto(r: &crate::jobs::JobRecord) -> codegg_protocol::dto::JobRecordDto {
    codegg_protocol::dto::JobRecordDto {
        job_id: r.job_id.as_str().to_string(),
        workspace_id: r.workspace_id.as_str().to_string(),
        session_id: r.session_id.clone(),
        turn_id: r.turn_id.clone(),
        kind: job_kind_to_str(r.kind).to_string(),
        source: job_source_to_value(&r.source),
        priority: job_priority_to_str(r.priority).to_string(),
        payload: job_payload_to_value(&r.payload),
        state: job_state_to_str(r.state).to_string(),
        current_attempt_id: r
            .current_attempt_id
            .as_ref()
            .map(|a| a.as_str().to_string()),
        attempt_count: r.attempt_count,
        retry_policy: retry_policy_to_value(&r.retry_policy),
        idempotency: idempotency_to_str(r.idempotency).to_string(),
        not_before: r.not_before.map(|d| d.timestamp_millis()),
        deadline: r.deadline.map(|d| d.timestamp_millis()),
        schedule_id: r.schedule_id.as_ref().map(|sid| sid.as_str().to_string()),
        created_at_ms: r.created_at.timestamp_millis(),
        updated_at_ms: r.updated_at.timestamp_millis(),
        terminal_at_ms: r.terminal_at.map(|d| d.timestamp_millis()),
        cancel_requested_at: r.cancel_requested_at.map(|d| d.timestamp_millis()),
        cancel_reason: r.cancel_reason.clone(),
        labels: r.labels.clone(),
    }
}

/// Domain → DTO: convert a `JobAttempt` into the wire DTO.
pub fn job_attempt_to_dto(a: &crate::jobs::JobAttempt) -> codegg_protocol::dto::JobAttemptDto {
    codegg_protocol::dto::JobAttemptDto {
        attempt_id: a.attempt_id.as_str().to_string(),
        job_id: a.job_id.as_str().to_string(),
        sequence: a.sequence,
        state: attempt_state_to_str(a.state).to_string(),
        daemon_generation: a.daemon_generation.as_str().to_string(),
        executor: a.executor.clone(),
        run_id: a.run_id.as_ref().map(|rid| rid.as_str().to_string()),
        heartbeat_at_ms: a.heartbeat_at.map(|d| d.timestamp_millis()),
        started_at_ms: a.started_at.map(|d| d.timestamp_millis()),
        completed_at_ms: a.completed_at.map(|d| d.timestamp_millis()),
        error_message: a.error.as_ref().map(|e| e.message.clone()),
        error_class: a.error.as_ref().map(|e| failure_class_to_str(e.class)),
        created_at_ms: a.created_at.timestamp_millis(),
        updated_at_ms: a.updated_at.timestamp_millis(),
    }
}

/// Domain → DTO: convert a `ScheduleSummary` into the wire DTO.
pub fn schedule_summary_to_dto(
    s: &crate::jobs::ScheduleSummary,
) -> codegg_protocol::dto::ScheduleSummaryDto {
    codegg_protocol::dto::ScheduleSummaryDto {
        schedule_id: s.schedule_id.clone(),
        workspace_id: s.workspace_id.clone(),
        session_id: None, // populated by daemon handler
        kind: schedule_kind_to_value(&s.kind),
        state: schedule_state_to_str(s.state).to_string(),
        overlap_policy: String::new(), // populated by daemon handler from full record
        missed_run_policy: serde_json::Value::Null, // populated by daemon handler
        next_run_at_ms: s.next_run_at.map(|d| d.timestamp_millis()),
        last_occurrence_at_ms: s.last_occurrence_at.map(|d| d.timestamp_millis()),
        created_at_ms: 0, // populated by daemon handler from full record
        updated_at_ms: 0, // populated by daemon handler from full record
    }
}

/// Domain → DTO: convert a `ScheduleRecord` into the wire DTO.
pub fn schedule_record_to_dto(
    r: &crate::jobs::ScheduleRecord,
) -> codegg_protocol::dto::ScheduleRecordDto {
    codegg_protocol::dto::ScheduleRecordDto {
        schedule_id: r.schedule_id.as_str().to_string(),
        workspace_id: r.workspace_id.as_str().to_string(),
        session_id: r.session_id.clone(),
        kind: schedule_kind_to_value(&r.kind),
        job_template: job_template_to_value(&r.job_template),
        state: schedule_state_to_str(r.state).to_string(),
        overlap_policy: overlap_policy_to_str(r.overlap_policy).to_string(),
        missed_run_policy: missed_run_policy_to_str(&r.missed_run_policy),
        next_run_at_ms: r.next_run_at.map(|d| d.timestamp_millis()),
        last_occurrence_at_ms: r.last_occurrence_at.map(|d| d.timestamp_millis()),
        created_at_ms: r.created_at.timestamp_millis(),
        updated_at_ms: r.updated_at.timestamp_millis(),
    }
}

/// Domain → DTO: convert a `RecoveryReport` into the wire DTO.
pub fn recovery_report_to_dto(
    r: &crate::jobs::RecoveryReport,
) -> codegg_protocol::dto::RecoveryReportDto {
    codegg_protocol::dto::RecoveryReportDto {
        interrupted_attempts: r.interrupted_attempts,
        requeued_jobs: r.requeued_jobs,
        terminal_jobs: r.terminal_jobs,
        schedules_reconciled: r.schedules_reconciled,
    }
}

/// Domain → DTO: convert a `CancelResult` into the wire DTO.
pub fn cancel_result_to_dto(
    r: &crate::jobs::CancelResult,
) -> codegg_protocol::dto::CancelResultDto {
    codegg_protocol::dto::CancelResultDto {
        job_id: r.job_id.as_str().to_string(),
        outcome: cancel_outcome_to_str(r.state).to_string(),
        terminal: r.terminal,
    }
}

/// DTO → Domain: convert a `JobSubmitDto` into a `NewJob`.
pub fn job_submit_from_dto(
    d: codegg_protocol::dto::JobSubmitDto,
) -> Result<crate::jobs::NewJob, String> {
    let kind = job_kind_from_str(&d.kind);
    if kind == crate::jobs::JobKind::Unsupported {
        return Err(format!("unsupported job kind: {}", d.kind));
    }
    let priority = job_priority_from_str(&d.priority);
    let source = job_source_from_value(&d.source)?;
    let payload = job_payload_from_value(&d.payload)?;
    let idempotency = idempotency_from_str(&d.idempotency)?;
    let retry_policy = crate::jobs::RetryPolicy {
        max_attempts: d.retry_max_attempts.max(1),
        backoff: crate::jobs::BackoffPolicy::None,
        retryable_failures: d
            .retryable_failures
            .iter()
            .filter_map(|s| failure_class_from_str(s))
            .collect(),
    };
    let timeout = d
        .timeout_ms
        .map(|ms| std::time::Duration::from_millis(ms as u64));
    let not_before = d
        .not_before_ms
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis);
    let deadline = d
        .deadline_ms
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis);
    let schedule_id = d.schedule_id.map(crate::jobs::ScheduleId::new_unchecked);
    let depends_on: Vec<crate::jobs::JobId> = d
        .depends_on
        .into_iter()
        .map(crate::jobs::JobId::new_unchecked)
        .collect();
    let resource_request = crate::jobs::ResourceRequest::for_kind(kind);

    Ok(crate::jobs::NewJob {
        workspace_id: crate::workspace::WorkspaceId::new_unchecked(d.workspace_id),
        session_id: d.session_id,
        turn_id: d.turn_id,
        kind,
        source,
        priority,
        payload,
        resource_request,
        timeout,
        retry_policy,
        idempotency,
        not_before,
        deadline,
        schedule_id,
        depends_on,
    })
}

/// DTO → Domain: convert a `ScheduleCreateDto` into a `ScheduleTemplate`.
pub fn schedule_create_from_dto(
    d: codegg_protocol::dto::ScheduleCreateDto,
) -> Result<crate::jobs::ScheduleTemplate, String> {
    let kind = schedule_kind_from_value(&d.kind)?;
    let job_template = job_template_from_value(&d.job_template)?;
    let overlap_policy = overlap_policy_from_str(&d.overlap_policy)?;
    let missed_run_policy = missed_run_policy_from_value(&d.missed_run_policy)?;

    Ok(crate::jobs::ScheduleTemplate {
        workspace_id: crate::workspace::WorkspaceId::new_unchecked(d.workspace_id),
        session_id: d.session_id,
        kind,
        job_template,
        overlap_policy,
        missed_run_policy,
        next_run_at: None,
        labels: d.labels,
    })
}

fn failure_class_from_str(s: &str) -> Option<crate::jobs::FailureClass> {
    match s {
        "transient" => Some(crate::jobs::FailureClass::Transient),
        "timeout" => Some(crate::jobs::FailureClass::Timeout),
        "cancelled" => Some(crate::jobs::FailureClass::Cancelled),
        "permission" => Some(crate::jobs::FailureClass::Permission),
        "validation" => Some(crate::jobs::FailureClass::Validation),
        "execution" => Some(crate::jobs::FailureClass::Execution),
        "unknown" => Some(crate::jobs::FailureClass::Unknown),
        _ => None,
    }
}

fn failure_class_to_str(c: crate::jobs::FailureClass) -> String {
    match c {
        crate::jobs::FailureClass::Transient => "transient",
        crate::jobs::FailureClass::Timeout => "timeout",
        crate::jobs::FailureClass::Cancelled => "cancelled",
        crate::jobs::FailureClass::Permission => "permission",
        crate::jobs::FailureClass::Validation => "validation",
        crate::jobs::FailureClass::Execution => "execution",
        crate::jobs::FailureClass::Unknown => "unknown",
    }
    .to_string()
}
