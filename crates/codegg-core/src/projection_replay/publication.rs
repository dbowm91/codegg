use codegg_protocol::core::{CoreEvent, EventEnvelope};
use codegg_protocol::projection::event::{ProjectionEnvelope, ProjectionEvent};
use codegg_protocol::projection::replay::ProjectionStreamKind;

pub fn projection_events_from_core(
    envelope: &EventEnvelope<CoreEvent>,
) -> Vec<(ProjectionStreamKind, ProjectionEnvelope)> {
    let mut result = Vec::new();

    let base = match &envelope.payload {
        CoreEvent::TurnStarted {
            session_id,
            turn_id,
        } => {
            let event = ProjectionEvent::TurnStarted {
                turn: codegg_protocol::projection::dto::TurnProjection {
                    turn_id: turn_id.clone(),
                    status: codegg_protocol::projection::dto::TurnStatus::Starting,
                    started_at: envelope.timestamp_ms,
                    updated_at: envelope.timestamp_ms,
                    stop_reason: None,
                    error: None,
                    messages: vec![],
                    tools: vec![],
                    pending_permissions: vec![],
                    pending_questions: vec![],
                    agent_tree: vec![],
                    subagent_count: 0,
                    input_tokens: None,
                    output_tokens: None,
                },
            };
            Some((session_id.clone(), None, event))
        }
        CoreEvent::TurnTextDelta {
            session_id,
            turn_id,
            delta,
        } => {
            let event = ProjectionEvent::MessageAppended {
                message: codegg_protocol::projection::dto::MessageProjection {
                    message_id: format!("{}-{}", turn_id, envelope.event_seq),
                    parent_turn_id: turn_id.clone(),
                    role: codegg_protocol::projection::dto::MessageRole::Assistant,
                    text: delta.clone(),
                    tool_call_id: None,
                    visibility: codegg_protocol::projection::dto::VisibilityClass::Public,
                    created_at: envelope.timestamp_ms,
                    truncated: false,
                },
            };
            Some((session_id.clone(), None, event))
        }
        CoreEvent::ToolStarted {
            session_id,
            turn_id,
            tool_name,
            tool_id,
            arguments,
            ..
        } => {
            let event = ProjectionEvent::ToolStarted {
                tool: codegg_protocol::projection::dto::ToolProjection {
                    tool_id: tool_id.clone(),
                    tool_name: tool_name.clone(),
                    status: codegg_protocol::projection::dto::ToolStatus::Started,
                    arguments: codegg_protocol::projection::dto::ToolArgumentProjection::Inline {
                        arguments: arguments.clone(),
                    },
                    output: codegg_protocol::projection::dto::ToolOutputProjection::Pending,
                    visibility: codegg_protocol::projection::dto::VisibilityClass::Public,
                    started_at: Some(envelope.timestamp_ms),
                    completed_at: None,
                    duration_ms: None,
                },
            };
            Some((session_id.clone(), turn_id.clone(), event))
        }
        CoreEvent::ToolCompleted {
            session_id,
            turn_id,
            tool_id,
            output,
            success,
            ..
        } => {
            let event = ProjectionEvent::ToolCompleted {
                tool_id: tool_id.clone(),
                output: codegg_protocol::projection::dto::ToolOutputProjection::Inline {
                    output: output.clone(),
                },
                success: *success,
                duration_ms: 0,
                completed_at: envelope.timestamp_ms,
            };
            Some((session_id.clone(), turn_id.clone(), event))
        }
        CoreEvent::PermissionPending {
            id,
            session_id,
            turn_id,
            tool,
            path,
        } => {
            let event = ProjectionEvent::PermissionPending {
                permission: codegg_protocol::projection::dto::PermissionProjection {
                    permission_id: id.clone(),
                    tool: tool.clone(),
                    path: path.clone(),
                    status: codegg_protocol::projection::dto::PermissionStatus::Pending,
                    created_at: envelope.timestamp_ms,
                    resolved_at: None,
                },
            };
            Some((session_id.clone(), turn_id.clone(), event))
        }
        CoreEvent::TurnCompleted {
            session_id,
            turn_id,
            stop_reason,
            ..
        } => {
            let event = ProjectionEvent::TurnCompleted {
                turn_id: turn_id.clone(),
                stop_reason: stop_reason.clone(),
                completed_at: envelope.timestamp_ms,
            };
            Some((session_id.clone(), None, event))
        }
        CoreEvent::TurnFailed {
            session_id,
            turn_id,
            message,
            ..
        } => {
            let event = ProjectionEvent::TurnFailed {
                turn_id: turn_id.clone().unwrap_or_default(),
                message: message.clone(),
                failed_at: envelope.timestamp_ms,
            };
            Some((session_id.clone(), turn_id.clone(), event))
        }
        CoreEvent::SubagentStarted {
            session_id,
            task_id,
            agent,
            description,
        } => {
            let event = ProjectionEvent::SubagentStarted {
                node: codegg_protocol::projection::dto::AgentTreeNodeProjection {
                    task_id: *task_id,
                    agent: agent.clone(),
                    description: description.clone(),
                    status: codegg_protocol::projection::dto::AgentTreeStatus::Running,
                    parent_task_id: None,
                    created_at: envelope.timestamp_ms,
                    completed_at: None,
                    result_summary: None,
                },
            };
            Some((session_id.clone(), None, event))
        }
        CoreEvent::SubagentCompleted {
            session_id,
            task_id,
            result_summary,
            ..
        } => {
            let event = ProjectionEvent::SubagentCompleted {
                task_id: *task_id,
                result_summary: result_summary.clone(),
                completed_at: envelope.timestamp_ms,
            };
            Some((session_id.clone(), None, event))
        }
        CoreEvent::SubagentFailed {
            session_id,
            task_id,
            error,
            ..
        } => {
            let event = ProjectionEvent::SubagentFailed {
                task_id: *task_id,
                error: error.clone(),
                failed_at: envelope.timestamp_ms,
            };
            Some((session_id.clone(), None, event))
        }
        CoreEvent::FileChanged { path, action } => {
            let change = match action.as_str() {
                "created" => codegg_protocol::projection::dto::FileChangeProjection::Created {
                    path: path.clone(),
                },
                "deleted" => codegg_protocol::projection::dto::FileChangeProjection::Deleted {
                    path: path.clone(),
                },
                "renamed" => codegg_protocol::projection::dto::FileChangeProjection::Renamed {
                    from: path.clone(),
                    to: path.clone(),
                },
                _ => codegg_protocol::projection::dto::FileChangeProjection::Modified {
                    path: path.clone(),
                },
            };
            let event = ProjectionEvent::FileChanged {
                change,
                at: envelope.timestamp_ms,
            };
            Some((envelope.session_id.clone().unwrap_or_default(), None, event))
        }
        CoreEvent::RunStarted {
            session_id,
            run_id,
            kind,
            command,
        } => {
            let event = ProjectionEvent::RunStarted {
                run: codegg_protocol::projection::dto::RunProjection {
                    run_id: run_id.clone(),
                    kind: kind.clone(),
                    command: command.clone(),
                    status: "running".into(),
                    summary: String::new(),
                    job_id: None,
                    log_dir: None,
                    started_at: envelope.timestamp_ms,
                    completed_at: None,
                    artifact_count: 0,
                    pinned: false,
                },
            };
            Some((session_id.clone(), None, event))
        }
        CoreEvent::RunCompleted {
            session_id,
            run_id,
            status,
            summary,
            ..
        } => {
            let event = ProjectionEvent::RunCompleted {
                run_id: run_id.clone(),
                status: status.clone(),
                summary: summary.clone(),
                completed_at: envelope.timestamp_ms,
            };
            Some((session_id.clone(), None, event))
        }
        CoreEvent::RunDenied {
            session_id,
            run_id,
            reason,
            ..
        } => {
            let event = ProjectionEvent::RunDenied {
                run_id: run_id.clone(),
                reason: reason.clone(),
                at: envelope.timestamp_ms,
            };
            Some((session_id.clone(), None, event))
        }
        CoreEvent::SessionUpdated { session_id } => {
            let event = ProjectionEvent::SessionActivated {
                summary: codegg_protocol::projection::dto::SessionSummaryProjection {
                    session_id: session_id.clone(),
                    project_id: String::new(),
                    workspace_id: String::new(),
                    title: String::new(),
                    status: "active".into(),
                    selected_model: None,
                    selected_agent: None,
                    has_active_turn: false,
                    pending_permission_count: 0,
                    pending_question_count: 0,
                    input_tokens: None,
                    output_tokens: None,
                    active_subagents: 0,
                    time_created_at: None,
                    time_updated_at: Some(envelope.timestamp_ms),
                    recent_summary: None,
                },
            };
            Some((session_id.clone(), None, event))
        }
        CoreEvent::JobCreated {
            session_id,
            job_id,
            workspace_id,
            kind,
            ..
        } => {
            let event = ProjectionEvent::JobUpserted {
                job: codegg_protocol::projection::dto::JobProjection {
                    job_id: job_id.clone(),
                    workspace_id: workspace_id.clone(),
                    kind: kind.clone(),
                    state: "created".into(),
                    summary: String::new(),
                    session_id: session_id.clone(),
                    turn_id: None,
                    active_attempt_id: None,
                    error_class: None,
                    updated_at: envelope.timestamp_ms,
                },
            };
            Some((session_id.clone().unwrap_or_default(), None, event))
        }
        CoreEvent::JobStarted {
            job_id, attempt_id, ..
        } => {
            let event = ProjectionEvent::JobUpserted {
                job: codegg_protocol::projection::dto::JobProjection {
                    job_id: job_id.clone(),
                    workspace_id: String::new(),
                    kind: String::new(),
                    state: "running".into(),
                    summary: String::new(),
                    session_id: None,
                    turn_id: None,
                    active_attempt_id: Some(attempt_id.clone()),
                    error_class: None,
                    updated_at: envelope.timestamp_ms,
                },
            };
            Some((String::new(), None, event))
        }
        CoreEvent::JobCompleted {
            job_id, attempt_id, ..
        } => {
            let event = ProjectionEvent::JobUpserted {
                job: codegg_protocol::projection::dto::JobProjection {
                    job_id: job_id.clone(),
                    workspace_id: String::new(),
                    kind: String::new(),
                    state: "completed".into(),
                    summary: String::new(),
                    session_id: None,
                    turn_id: None,
                    active_attempt_id: Some(attempt_id.clone()),
                    error_class: None,
                    updated_at: envelope.timestamp_ms,
                },
            };
            Some((String::new(), None, event))
        }
        CoreEvent::JobFailed {
            job_id,
            attempt_id,
            error_class,
            ..
        } => {
            let event = ProjectionEvent::JobUpserted {
                job: codegg_protocol::projection::dto::JobProjection {
                    job_id: job_id.clone(),
                    workspace_id: String::new(),
                    kind: String::new(),
                    state: "failed".into(),
                    summary: String::new(),
                    session_id: None,
                    turn_id: None,
                    active_attempt_id: Some(attempt_id.clone()),
                    error_class: Some(error_class.clone()),
                    updated_at: envelope.timestamp_ms,
                },
            };
            Some((String::new(), None, event))
        }
        _ => None,
    };

    if let Some((session_id, turn_id, event)) = base {
        let vis = event.visibility();
        if !matches!(
            vis,
            codegg_protocol::projection::dto::VisibilityClass::Internal
        ) {
            let env = ProjectionEnvelope {
                protocol_version: 1,
                event_seq: envelope.event_seq,
                timestamp_ms: envelope.timestamp_ms,
                session_id: Some(session_id),
                turn_id,
                scope: codegg_protocol::projection::event::ProjectionStreamScope::Session,
                payload: event,
            };
            result.push((ProjectionStreamKind::Session, env.clone()));
            result.push((ProjectionStreamKind::Project, env));
        }
    }

    result
}
