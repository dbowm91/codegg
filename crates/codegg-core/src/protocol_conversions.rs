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
pub fn run_query_from_dto(
    query: codegg_protocol::dto::RunQueryDto,
) -> crate::run_store::RunQuery {
    let kind = query.kinds.into_iter().find_map(|s| run_kind_from_str(&s));
    let status = query.statuses.into_iter().find_map(|s| run_status_from_str(&s));
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
