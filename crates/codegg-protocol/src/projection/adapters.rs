//! Adapters that bridge the existing daemon protocol into the
//! projection contract.
//!
//! These adapters are intentionally **non-replacing**: they consume
//! existing [`crate::core::CoreResponse`] snapshot variants and
//! [`crate::core::CoreEvent`] events and emit projection DTOs and
//! [`crate::projection::event::ProjectionEvent`] variants without
//! altering the wire shape that other clients depend on.

use serde::{Deserialize, Serialize};

use crate::core::{CoreEvent, CoreResponse, EventEnvelope, SessionSnapshot, PROTOCOL_VERSION};
use crate::projection::dto::{
    AgentTreeNodeProjection, AgentTreeStatus, ArtifactHandleProjection, FileChangeProjection,
    JobProjection, MessageProjection, MessageRole, PermissionProjection, PermissionStatus,
    ProjectSummaryProjection, QuestionProjection, RunProjection, SessionSummaryProjection,
    ToolArgumentProjection, ToolOutputProjection, ToolProjection, ToolStatus, TurnProjection,
    TurnStatus, VisibilityClass, WorkspaceHealthProjection, WorkspaceSummaryProjection,
};
use crate::projection::event::{ProjectionEnvelope, ProjectionEvent};
use crate::projection::limits::{
    clip_str, truncate_str, MAX_PROJECTION_RUN_SUMMARY_BYTES, MAX_PROJECTION_STRING_BYTES,
};
use crate::projection::snapshot::SessionProjectionSnapshot;

/// Build a [`SessionProjectionSnapshot`] from the bounded
/// [`CoreResponse::SnapshotSession`] payload.
///
/// The adapter honours the projection version and uses the session
/// identity carried by the snapshot as the snapshot's primary session
/// id. Empty / missing fields fall back to safe defaults so the
/// reducer never has to handle an absent session.
pub fn snapshot_from_snapshot_session(
    snapshot: &CoreResponse,
    event_seq: u64,
    timestamp_ms: i64,
) -> Option<SessionProjectionSnapshot> {
    let CoreResponse::SnapshotSession {
        session,
        messages,
        status,
        selected_model,
        selected_agent,
        pending_permissions,
        pending_questions,
        input_tokens,
        output_tokens,
        active_subagents,
        ..
    } = snapshot
    else {
        return None;
    };
    let workspace_id = session
        .workspace_id
        .clone()
        .or_else(|| session.binding.as_ref().map(|b| b.workspace_id.clone()))
        .unwrap_or_default();
    let summary = SessionSummaryProjection {
        session_id: session.id.clone(),
        project_id: session.project_id.clone(),
        workspace_id: workspace_id.clone(),
        title: truncate_str(&session.title, MAX_PROJECTION_STRING_BYTES).into_owned(),
        status: truncate_str(status, MAX_PROJECTION_STRING_BYTES).into_owned(),
        selected_model: selected_model
            .as_deref()
            .map(|m| truncate_str(m, MAX_PROJECTION_STRING_BYTES).into_owned()),
        selected_agent: selected_agent
            .as_deref()
            .map(|a| truncate_str(a, MAX_PROJECTION_STRING_BYTES).into_owned()),
        has_active_turn: !messages.is_empty(),
        pending_permission_count: pending_permissions.len(),
        pending_question_count: pending_questions.len(),
        input_tokens: *input_tokens,
        output_tokens: *output_tokens,
        active_subagents: *active_subagents,
        time_created_at: Some(session.time_created),
        time_updated_at: Some(session.time_updated),
        recent_summary: None,
    };
    let workspace = WorkspaceSummaryProjection {
        workspace_id: workspace_id.clone(),
        canonical_root: session.directory.clone(),
        display_name: truncate_str(&session.title, MAX_PROJECTION_STRING_BYTES).into_owned(),
        created_at: session.time_created,
        last_opened_at: session.time_updated,
        archived_at: session.time_archived,
        active_sessions: 0,
        services_active: false,
        active_leases: 0,
        config_revision: 0,
        health: WorkspaceHealthProjection::default(),
    };
    let mut snap = SessionProjectionSnapshot {
        protocol_version: crate::projection::caps::PROJECTION_PROTOCOL_VERSION,
        event_seq,
        generated_at_ms: timestamp_ms,
        primary_session_id: session.id.clone(),
        project_id: session.project_id.clone(),
        workspace_id,
        primary_session: summary,
        secondary_sessions: Vec::new(),
        workspace,
        active_turn: None,
        recent_turns: Vec::new(),
        runs: Vec::new(),
        jobs: Vec::new(),
        diagnostics: Vec::new(),
    };

    if !messages.is_empty() {
        let turn = TurnProjection {
            turn_id: messages
                .first()
                .map(|m| m.id.clone())
                .unwrap_or_else(|| "replayed".into()),
            status: TurnStatus::Completed,
            started_at: messages.first().map(|m| m.time_created).unwrap_or(0),
            updated_at: messages.last().map(|m| m.time_updated).unwrap_or(0),
            stop_reason: None,
            error: None,
            messages: messages
                .iter()
                .map(|m| MessageProjection {
                    message_id: m.id.clone(),
                    parent_turn_id: "replayed".into(),
                    role: MessageRole::Assistant,
                    text: extract_message_text(m),
                    tool_call_id: None,
                    visibility: VisibilityClass::Public,
                    created_at: m.time_created,
                    truncated: false,
                })
                .collect(),
            tools: Vec::new(),
            pending_permissions: pending_permissions
                .iter()
                .map(|id| PermissionProjection {
                    permission_id: id.clone(),
                    tool: String::new(),
                    path: None,
                    status: PermissionStatus::Pending,
                    created_at: timestamp_ms,
                    resolved_at: None,
                })
                .collect(),
            pending_questions: pending_questions
                .iter()
                .map(|id| QuestionProjection {
                    question_id: id.clone(),
                    header: None,
                    prompt: String::new(),
                    status: PermissionStatus::Pending,
                    created_at: timestamp_ms,
                    resolved_at: None,
                })
                .collect(),
            agent_tree: Vec::new(),
            subagent_count: *active_subagents,
            input_tokens: *input_tokens,
            output_tokens: *output_tokens,
        };
        snap.recent_turns.push(turn);
        snap.primary_session.has_active_turn = false;
    }
    Some(snap)
}

fn extract_message_text(message: &crate::dto::Message) -> String {
    let mut out = String::new();
    for part in &message.data.parts {
        match &part.data {
            crate::dto::PartData::Text { text } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(text);
            }
            crate::dto::PartData::Reasoning { reasoning } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(reasoning);
            }
            crate::dto::PartData::ToolCall { name, .. } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(name);
            }
            crate::dto::PartData::Image { url } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(url);
            }
            crate::dto::PartData::File { path, .. } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(path);
            }
        }
    }
    truncate_str(&out, MAX_PROJECTION_STRING_BYTES).into_owned()
}

/// Map a [`CoreResponse::SnapshotDaemon`] payload to a single
/// daemon-level snapshot. Returns `None` when the response variant
/// does not match.
///
/// The adapter is used by observer-level consumers that want a
/// daemon-wide view without picking a primary session.
pub fn snapshot_from_daemon(
    snapshot: &CoreResponse,
    event_seq: u64,
    timestamp_ms: i64,
) -> Option<SessionProjectionSnapshot> {
    let CoreResponse::SnapshotDaemon {
        active_sessions, ..
    } = snapshot
    else {
        return None;
    };
    let session = active_sessions.first()?;
    snapshot_from_session_snapshot(session, event_seq, timestamp_ms)
}

/// Build a bounded session projection from the lightweight
/// [`SessionSnapshot`] carried inside [`CoreResponse::SnapshotDaemon`].
pub fn snapshot_from_session_snapshot(
    snapshot: &SessionSnapshot,
    event_seq: u64,
    timestamp_ms: i64,
) -> Option<SessionProjectionSnapshot> {
    let workspace_id = snapshot
        .workspace_id
        .clone()
        .or_else(|| snapshot.binding.as_ref().map(|b| b.workspace_id.clone()))
        .unwrap_or_default();
    let mut snap =
        SessionProjectionSnapshot::empty(&snapshot.session_id, &snapshot.project_id, &workspace_id);
    snap.event_seq = event_seq;
    snap.generated_at_ms = timestamp_ms;
    let summary = SessionSummaryProjection {
        session_id: snapshot.session_id.clone(),
        project_id: snapshot.project_id.clone(),
        workspace_id: workspace_id.clone(),
        title: truncate_str(&snapshot.directory, MAX_PROJECTION_STRING_BYTES).into_owned(),
        status: truncate_str(&snapshot.status, MAX_PROJECTION_STRING_BYTES).into_owned(),
        selected_model: snapshot
            .selected_model
            .as_deref()
            .map(|m| truncate_str(m, MAX_PROJECTION_STRING_BYTES).into_owned()),
        selected_agent: snapshot
            .selected_agent
            .as_deref()
            .map(|a| truncate_str(a, MAX_PROJECTION_STRING_BYTES).into_owned()),
        has_active_turn: snapshot.has_active_turn,
        pending_permission_count: snapshot.pending_permissions.len(),
        pending_question_count: snapshot.pending_questions.len(),
        input_tokens: snapshot.input_tokens,
        output_tokens: snapshot.output_tokens,
        active_subagents: snapshot.active_subagents,
        time_created_at: None,
        time_updated_at: Some(timestamp_ms),
        recent_summary: None,
    };
    snap.primary_session = summary;
    let workspace = WorkspaceSummaryProjection {
        workspace_id: workspace_id.clone(),
        canonical_root: snapshot.directory.clone(),
        display_name: truncate_str(&snapshot.directory, MAX_PROJECTION_STRING_BYTES).into_owned(),
        created_at: 0,
        last_opened_at: timestamp_ms,
        archived_at: None,
        active_sessions: 1,
        services_active: false,
        active_leases: 0,
        config_revision: 0,
        health: WorkspaceHealthProjection::default(),
    };
    snap.workspace = workspace;
    Some(snap)
}

/// Build a projection summary from a [`crate::dto::ProjectSummaryDto`].
pub fn project_summary_from_dto(dto: &crate::dto::ProjectSummaryDto) -> ProjectSummaryProjection {
    ProjectSummaryProjection {
        project_id: dto.project_id.clone(),
        display_name: truncate_str(&dto.display_name, MAX_PROJECTION_STRING_BYTES).into_owned(),
        lifecycle: truncate_str(&dto.lifecycle, MAX_PROJECTION_STRING_BYTES).into_owned(),
        description: dto
            .description
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_STRING_BYTES).into_owned()),
        tags: dto
            .tags
            .iter()
            .map(|t| truncate_str(t, MAX_PROJECTION_STRING_BYTES).into_owned())
            .collect(),
        time_last_opened_at: dto.time_last_opened_at,
        registration_source: truncate_str(&dto.registration_source, MAX_PROJECTION_STRING_BYTES)
            .into_owned(),
        archived_at: dto.archived_at,
        created_at: dto.created_at,
        updated_at: dto.updated_at,
    }
}

/// Convert a [`CoreEvent`] envelope into a list of projection
/// events. Events that have no projection mapping (e.g. session-lifecycle
/// housekeeping) yield an empty vector.
///
/// Multiple projection events may be produced by one core event when
/// the core event bundles related state changes (e.g. `SnapshotSession`
/// emits a [`ProjectionEvent::SessionActivated`] plus a
/// [`ProjectionEvent::Diagnostic`] for the snapshot itself).
pub fn projection_events_from_core(env: &EventEnvelope<CoreEvent>) -> Vec<ProjectionEvent> {
    let mut events: Vec<ProjectionEvent> = Vec::new();
    match &env.payload {
        CoreEvent::TurnStarted { .. } => {
            events.push(ProjectionEvent::Diagnostic {
                code: "core_event_observed".into(),
                message: "TurnStarted observed".into(),
            });
        }
        CoreEvent::TurnTextDelta { delta, .. } => {
            events.push(ProjectionEvent::MessageAppended {
                message: MessageProjection {
                    message_id: format!("core-{}", env.event_seq),
                    parent_turn_id: env.turn_id.clone().unwrap_or_default(),
                    role: MessageRole::Assistant,
                    text: clip_str(delta, MAX_PROJECTION_STRING_BYTES).to_string(),
                    tool_call_id: None,
                    visibility: VisibilityClass::Public,
                    created_at: env.timestamp_ms,
                    truncated: delta.len() > MAX_PROJECTION_STRING_BYTES,
                },
            });
        }
        CoreEvent::TurnReasoningDelta { delta, .. } => {
            events.push(ProjectionEvent::ReasoningAppended {
                message_id: format!("reason-{}", env.event_seq),
                delta: clip_str(delta, MAX_PROJECTION_STRING_BYTES).to_string(),
            });
        }
        CoreEvent::ToolStarted {
            tool_name,
            tool_id,
            arguments,
            ..
        } => {
            let mut tool = ToolProjection {
                tool_id: tool_id.clone(),
                tool_name: truncate_str(tool_name, MAX_PROJECTION_STRING_BYTES).into_owned(),
                status: ToolStatus::Started,
                arguments: ToolArgumentProjection::Inline {
                    arguments: clip_str(arguments, MAX_PROJECTION_STRING_BYTES).to_string(),
                },
                output: ToolOutputProjection::Pending,
                visibility: VisibilityClass::Public,
                started_at: Some(env.timestamp_ms),
                completed_at: None,
                duration_ms: None,
            };
            tool.normalise();
            events.push(ProjectionEvent::ToolStarted { tool });
        }
        CoreEvent::ToolCompleted {
            tool_id,
            output,
            success,
            ..
        } => {
            let output = if output.len() > MAX_PROJECTION_STRING_BYTES {
                ToolOutputProjection::TruncatedOutput {
                    original_bytes: output.len(),
                    preview: clip_str(output, MAX_PROJECTION_STRING_BYTES).to_string(),
                }
            } else {
                ToolOutputProjection::Inline {
                    output: clip_str(output, MAX_PROJECTION_STRING_BYTES).to_string(),
                }
            };
            events.push(ProjectionEvent::ToolCompleted {
                tool_id: tool_id.clone(),
                output,
                success: *success,
                duration_ms: 0,
                completed_at: env.timestamp_ms,
            });
        }
        CoreEvent::PermissionPending { id, tool, path, .. } => {
            let mut permission = PermissionProjection {
                permission_id: id.clone(),
                tool: truncate_str(tool, MAX_PROJECTION_STRING_BYTES).into_owned(),
                path: path
                    .as_deref()
                    .map(|p| truncate_str(p, MAX_PROJECTION_STRING_BYTES).into_owned()),
                status: PermissionStatus::Pending,
                created_at: env.timestamp_ms,
                resolved_at: None,
            };
            permission.normalise();
            events.push(ProjectionEvent::PermissionPending { permission });
        }
        CoreEvent::QuestionPending { id, questions, .. } => {
            let mut question = QuestionProjection {
                question_id: id.clone(),
                header: None,
                prompt: truncate_str(&questions.to_string(), MAX_PROJECTION_STRING_BYTES)
                    .into_owned(),
                status: PermissionStatus::Pending,
                created_at: env.timestamp_ms,
                resolved_at: None,
            };
            question.normalise();
            events.push(ProjectionEvent::QuestionPending { question });
        }
        CoreEvent::TurnCompleted {
            turn_id,
            stop_reason,
            ..
        } => {
            events.push(ProjectionEvent::TurnCompleted {
                turn_id: turn_id.clone(),
                stop_reason: truncate_str(stop_reason, MAX_PROJECTION_STRING_BYTES).into_owned(),
                completed_at: env.timestamp_ms,
            });
        }
        CoreEvent::TurnFailed {
            turn_id, message, ..
        } => {
            events.push(ProjectionEvent::TurnFailed {
                turn_id: turn_id.clone().unwrap_or_default(),
                message: truncate_str(message, MAX_PROJECTION_STRING_BYTES).into_owned(),
                failed_at: env.timestamp_ms,
            });
        }
        CoreEvent::SubagentStarted {
            task_id,
            agent,
            description,
            ..
        } => {
            let mut node = AgentTreeNodeProjection {
                task_id: *task_id,
                agent: truncate_str(agent, MAX_PROJECTION_STRING_BYTES).into_owned(),
                description: truncate_str(description, MAX_PROJECTION_STRING_BYTES).into_owned(),
                status: AgentTreeStatus::Running,
                parent_task_id: None,
                created_at: env.timestamp_ms,
                completed_at: None,
                result_summary: None,
            };
            node.normalise();
            events.push(ProjectionEvent::SubagentStarted { node });
        }
        CoreEvent::SubagentProgress {
            task_id, message, ..
        } => {
            events.push(ProjectionEvent::SubagentProgress {
                task_id: *task_id,
                message: truncate_str(message, MAX_PROJECTION_STRING_BYTES).into_owned(),
                at: env.timestamp_ms,
            });
        }
        CoreEvent::SubagentCompleted {
            task_id,
            result_summary,
            ..
        } => {
            events.push(ProjectionEvent::SubagentCompleted {
                task_id: *task_id,
                result_summary: truncate_str(result_summary, MAX_PROJECTION_RUN_SUMMARY_BYTES)
                    .into_owned(),
                completed_at: env.timestamp_ms,
            });
        }
        CoreEvent::SubagentFailed { task_id, error, .. } => {
            events.push(ProjectionEvent::SubagentFailed {
                task_id: *task_id,
                error: truncate_str(error, MAX_PROJECTION_STRING_BYTES).into_owned(),
                failed_at: env.timestamp_ms,
            });
        }
        CoreEvent::FileChanged { path, action } => {
            let change = match action.as_str() {
                "created" | "create" => FileChangeProjection::Created { path: path.clone() },
                "deleted" | "delete" => FileChangeProjection::Deleted { path: path.clone() },
                "renamed" => FileChangeProjection::Renamed {
                    from: path.clone(),
                    to: path.clone(),
                },
                _ => FileChangeProjection::Modified { path: path.clone() },
            };
            events.push(ProjectionEvent::FileChanged {
                change,
                at: env.timestamp_ms,
            });
        }
        CoreEvent::RunStarted {
            run_id,
            kind,
            command,
            ..
        } => {
            let run = RunProjection {
                run_id: run_id.clone(),
                kind: truncate_str(kind, MAX_PROJECTION_STRING_BYTES).into_owned(),
                command: truncate_str(command, MAX_PROJECTION_STRING_BYTES).into_owned(),
                status: "running".to_string(),
                summary: String::new(),
                job_id: None,
                log_dir: None,
                started_at: env.timestamp_ms,
                completed_at: None,
                artifact_count: 0,
                pinned: false,
            };
            events.push(ProjectionEvent::RunStarted { run });
        }
        CoreEvent::RunProgress {
            run_id, message, ..
        } => {
            events.push(ProjectionEvent::RunProgress {
                run_id: run_id.clone(),
                message: truncate_str(message, MAX_PROJECTION_STRING_BYTES).into_owned(),
                at: env.timestamp_ms,
            });
        }
        CoreEvent::RunArtifactCreated {
            run_id,
            artifact_id,
            kind,
            byte_length,
            ..
        } => {
            let artifact = ArtifactHandleProjection {
                artifact_id: artifact_id.clone(),
                kind: truncate_str(kind, MAX_PROJECTION_STRING_BYTES).into_owned(),
                byte_length: *byte_length,
                run_id: Some(run_id.clone()),
                created_at: env.timestamp_ms,
                preview: None,
            };
            events.push(ProjectionEvent::RunArtifactCreated { artifact });
        }
        CoreEvent::RunCompleted {
            run_id,
            status,
            summary,
            ..
        } => {
            events.push(ProjectionEvent::RunCompleted {
                run_id: run_id.clone(),
                status: truncate_str(status, MAX_PROJECTION_STRING_BYTES).into_owned(),
                summary: truncate_str(summary, MAX_PROJECTION_RUN_SUMMARY_BYTES).into_owned(),
                completed_at: env.timestamp_ms,
            });
        }
        CoreEvent::RunDenied { run_id, reason, .. } => {
            events.push(ProjectionEvent::RunDenied {
                run_id: run_id.clone(),
                reason: truncate_str(reason, MAX_PROJECTION_STRING_BYTES).into_owned(),
                at: env.timestamp_ms,
            });
        }
        CoreEvent::JobCreated {
            job_id,
            workspace_id,
            kind,
            session_id,
            turn_id,
        } => {
            let job = JobProjection {
                job_id: job_id.clone(),
                workspace_id: workspace_id.clone(),
                kind: truncate_str(kind, MAX_PROJECTION_STRING_BYTES).into_owned(),
                state: "created".to_string(),
                summary: String::new(),
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
                active_attempt_id: None,
                error_class: None,
                updated_at: env.timestamp_ms,
            };
            events.push(ProjectionEvent::JobUpserted { job });
        }
        CoreEvent::JobStarted {
            job_id, attempt_id, ..
        } => {
            let job = JobProjection {
                job_id: job_id.clone(),
                workspace_id: String::new(),
                kind: String::new(),
                state: "started".to_string(),
                summary: attempt_id.clone(),
                session_id: None,
                turn_id: None,
                active_attempt_id: Some(attempt_id.clone()),
                error_class: None,
                updated_at: env.timestamp_ms,
            };
            events.push(ProjectionEvent::JobUpserted { job });
        }
        CoreEvent::JobCompleted {
            job_id, attempt_id, ..
        } => {
            let job = JobProjection {
                job_id: job_id.clone(),
                workspace_id: String::new(),
                kind: String::new(),
                state: "completed".to_string(),
                summary: attempt_id.clone(),
                session_id: None,
                turn_id: None,
                active_attempt_id: Some(attempt_id.clone()),
                error_class: None,
                updated_at: env.timestamp_ms,
            };
            events.push(ProjectionEvent::JobUpserted { job });
        }
        CoreEvent::JobFailed {
            job_id,
            attempt_id,
            error_class,
            message,
        } => {
            let job = JobProjection {
                job_id: job_id.clone(),
                workspace_id: String::new(),
                kind: String::new(),
                state: "failed".to_string(),
                summary: truncate_str(message, MAX_PROJECTION_RUN_SUMMARY_BYTES).into_owned(),
                session_id: None,
                turn_id: None,
                active_attempt_id: Some(attempt_id.clone()),
                error_class: Some(
                    truncate_str(error_class, MAX_PROJECTION_STRING_BYTES).into_owned(),
                ),
                updated_at: env.timestamp_ms,
            };
            events.push(ProjectionEvent::JobUpserted { job });
        }
        _ => {
            events.push(ProjectionEvent::Unknown {
                variant_name: core_event_kind(&env.payload),
                notice: "no projection mapping; ignored".to_string(),
            });
        }
    }
    events
}

/// Convert a [`CoreEvent`] envelope into a [`ProjectionEnvelope`]
/// using the existing `event_seq` and `timestamp_ms`. Multiple
/// projection events per core event are flattened into one envelope
/// by selecting the first; the rest are returned in
/// [`MultiProjectionEnvelope::extras`].
///
/// This is convenient for adapters that publish one envelope per
/// core event without yet supporting compound events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiProjectionEnvelope {
    pub primary: ProjectionEnvelope,
    pub extras: Vec<ProjectionEnvelope>,
}

pub fn projection_envelopes_from_core(env: &EventEnvelope<CoreEvent>) -> MultiProjectionEnvelope {
    let events = projection_events_from_core(env);
    let mut iter = events.into_iter();
    let primary = match iter.next() {
        Some(event) => ProjectionEnvelope {
            protocol_version: crate::projection::caps::PROJECTION_PROTOCOL_VERSION,
            event_seq: env.event_seq,
            timestamp_ms: env.timestamp_ms,
            session_id: env.session_id.clone(),
            turn_id: env.turn_id.clone(),
            scope: crate::projection::event::ProjectionStreamScope::Session,
            payload: event,
        },
        None => ProjectionEnvelope {
            protocol_version: crate::projection::caps::PROJECTION_PROTOCOL_VERSION,
            event_seq: env.event_seq,
            timestamp_ms: env.timestamp_ms,
            session_id: env.session_id.clone(),
            turn_id: env.turn_id.clone(),
            scope: crate::projection::event::ProjectionStreamScope::Session,
            payload: ProjectionEvent::Unknown {
                variant_name: core_event_kind(&env.payload),
                notice: "no projection mapping".to_string(),
            },
        },
    };
    let extras: Vec<ProjectionEnvelope> = iter
        .map(|event| ProjectionEnvelope {
            protocol_version: crate::projection::caps::PROJECTION_PROTOCOL_VERSION,
            event_seq: env.event_seq,
            timestamp_ms: env.timestamp_ms,
            session_id: env.session_id.clone(),
            turn_id: env.turn_id.clone(),
            scope: crate::projection::event::ProjectionStreamScope::Session,
            payload: event,
        })
        .collect();
    MultiProjectionEnvelope { primary, extras }
}

fn core_event_kind(event: &CoreEvent) -> String {
    serde_json::to_value(event)
        .ok()
        .and_then(|v| {
            v.get("type")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Public re-export for the current core protocol version so
/// adapters can record provenance without depending on the layout of
/// [`crate::core`].
pub fn core_protocol_version() -> u32 {
    PROTOCOL_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::EventEnvelope;

    fn envelope(event: CoreEvent, seq: u64) -> EventEnvelope<CoreEvent> {
        EventEnvelope {
            protocol_version: PROTOCOL_VERSION,
            event_seq: seq,
            timestamp_ms: 0,
            session_id: Some("s1".into()),
            turn_id: Some("t1".into()),
            payload: event,
        }
    }

    #[test]
    fn tool_started_produces_tool_started_event() {
        let env = envelope(
            CoreEvent::ToolStarted {
                session_id: "s1".into(),
                turn_id: Some("t1".into()),
                tool_name: "Bash".into(),
                tool_id: "tool-1".into(),
                arguments: "ls".into(),
            },
            1,
        );
        let events = projection_events_from_core(&env);
        assert!(matches!(events[0], ProjectionEvent::ToolStarted { .. }));
    }

    #[test]
    fn oversized_tool_output_becomes_truncated() {
        let long = "x".repeat(MAX_PROJECTION_STRING_BYTES + 16);
        let env = envelope(
            CoreEvent::ToolCompleted {
                session_id: "s1".into(),
                turn_id: Some("t1".into()),
                tool_id: "tool-1".into(),
                output: long,
                success: true,
            },
            2,
        );
        let events = projection_events_from_core(&env);
        match &events[0] {
            ProjectionEvent::ToolCompleted { output, .. } => {
                assert!(matches!(
                    output,
                    ToolOutputProjection::TruncatedOutput { .. }
                ));
            }
            _ => panic!("expected ToolCompleted event"),
        }
    }

    #[test]
    fn unknown_core_event_becomes_unknown_projection_event() {
        let env = envelope(
            CoreEvent::Error {
                code: "x".into(),
                message: "y".into(),
            },
            3,
        );
        let events = projection_events_from_core(&env);
        assert!(matches!(events[0], ProjectionEvent::Unknown { .. }));
    }

    #[test]
    fn projection_envelope_uses_core_event_seq() {
        let env = envelope(
            CoreEvent::ToolStarted {
                session_id: "s1".into(),
                turn_id: Some("t1".into()),
                tool_name: "Bash".into(),
                tool_id: "tool-1".into(),
                arguments: "ls".into(),
            },
            42,
        );
        let multi = projection_envelopes_from_core(&env);
        assert_eq!(multi.primary.event_seq, 42);
    }
}
