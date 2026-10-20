use codegg_protocol::core::{CoreEvent, EventEnvelope};
use codegg_protocol::projection::event::{ProjectionEnvelope, ProjectionEvent};
use codegg_protocol::projection::replay::ProjectionStreamKind;

pub fn projection_events_from_core(
    envelope: &EventEnvelope<CoreEvent>,
) -> Vec<(ProjectionStreamKind, ProjectionEnvelope)> {
    let mut result = Vec::new();

    // M006-B: LSP diagnostics are project-scoped and are emitted on the
    // `Project` stream only — never the session one. Handled before the
    // session-keyed match below so it cannot be swept along with it.
    if let CoreEvent::LspDiagnosticsUpdated {
        project_id, file, ..
    } = &envelope.payload
    {
        let event = ProjectionEvent::LspDiagnosticsUpdated {
            file: codegg_protocol::lsp::LspDiagnosticsProjectionDto {
                project_id: project_id.clone(),
                file: file.clone(),
            },
        };
        result.push((
            ProjectionStreamKind::Project,
            ProjectionEnvelope {
                protocol_version: 1,
                event_seq: envelope.event_seq,
                timestamp_ms: envelope.timestamp_ms,
                session_id: None,
                turn_id: None,
                scope: codegg_protocol::projection::event::ProjectionStreamScope::Project,
                payload: event,
            },
        ));
        return result;
    }

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
                    messages: vec![].into(),
                    tools: vec![].into(),
                    pending_permissions: vec![].into(),
                    pending_questions: vec![].into(),
                    agent_tree: vec![].into(),
                    subagent_count: 0,
                    input_tokens: None,
                    output_tokens: None,
                    cached_tokens: None,
                    reasoning_tokens: None,
                    context_limit: None,
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
            input_tokens,
            output_tokens,
            cached_tokens,
            reasoning_tokens,
            context_limit,
            ..
        } => {
            let event = ProjectionEvent::TurnCompleted {
                turn_id: turn_id.clone(),
                stop_reason: stop_reason.clone(),
                completed_at: envelope.timestamp_ms,
                input_tokens: *input_tokens,
                output_tokens: *output_tokens,
                cached_tokens: *cached_tokens,
                reasoning_tokens: *reasoning_tokens,
                context_limit: *context_limit,
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
            child_instance_id,
            originating_turn_id,
            task_id,
            agent,
            description,
            display_name,
            profile,
            usefulness,
            run_id,
        } => {
            let event = ProjectionEvent::SubagentStarted {
                node: codegg_protocol::projection::dto::AgentTreeNodeProjection {
                    child_instance_id: child_instance_id.clone(),
                    originating_turn_id: originating_turn_id.clone(),
                    task_id: *task_id,
                    agent: agent.clone(),
                    description: description.clone(),
                    status: codegg_protocol::projection::dto::AgentTreeStatus::Running,
                    parent_task_id: None,
                    created_at: envelope.timestamp_ms,
                    completed_at: None,
                    result_summary: None,
                    // Replayed verbatim from the recorded event. This is the
                    // determinism contract: a replay that re-drew the name
                    // would make the historical view disagree with the live
                    // one about what a given subagent was called. The
                    // descriptors are carried under the same contract.
                    display_name: display_name.clone(),
                    profile: profile.clone(),
                    usefulness: usefulness.clone(),
                    run_id: run_id.clone(),
                },
            };
            Some((session_id.clone(), None, event))
        }
        CoreEvent::SubagentCompleted {
            session_id,
            child_instance_id,
            task_id,
            result_summary,
            ..
        } => {
            let event = ProjectionEvent::SubagentCompleted {
                child_instance_id: child_instance_id.clone(),
                task_id: *task_id,
                result_summary: result_summary.clone(),
                completed_at: envelope.timestamp_ms,
            };
            Some((session_id.clone(), None, event))
        }
        CoreEvent::SubagentFailed {
            session_id,
            child_instance_id,
            task_id,
            error,
            ..
        } => {
            let event = ProjectionEvent::SubagentFailed {
                child_instance_id: child_instance_id.clone(),
                task_id: *task_id,
                error: error.clone(),
                failed_at: envelope.timestamp_ms,
            };
            Some((session_id.clone(), None, event))
        }
        CoreEvent::AgentRunUpserted { session_id, run } => Some((
            session_id.clone(),
            None,
            ProjectionEvent::AgentRunUpserted { run: run.clone() },
        )),
        CoreEvent::AgentRunProgress {
            session_id,
            run_id,
            status,
            progress,
        } => Some((
            session_id.clone(),
            None,
            ProjectionEvent::AgentRunProgress {
                run_id: run_id.clone(),
                status: status.clone(),
                progress: progress.clone(),
                at: envelope.timestamp_ms,
            },
        )),
        CoreEvent::AgentRunTerminal {
            session_id,
            run_id,
            status,
            terminal_summary,
            result_commit,
            validation_summary,
            attention_required,
        } => Some((
            session_id.clone(),
            None,
            ProjectionEvent::AgentRunTerminal {
                run_id: run_id.clone(),
                status: status.clone(),
                terminal_summary: terminal_summary.clone(),
                result_commit: result_commit.clone(),
                validation_summary: validation_summary.clone(),
                attention_required: *attention_required,
                completed_at: envelope.timestamp_ms,
            },
        )),
        CoreEvent::AgentRunControlUpdated {
            session_id,
            run_id,
            control_status,
            cancellation_requested,
        } => Some((
            session_id.clone(),
            None,
            ProjectionEvent::AgentRunControlUpdated {
                run_id: run_id.clone(),
                control_status: control_status.clone(),
                cancellation_requested: *cancellation_requested,
                at: envelope.timestamp_ms,
            },
        )),
        CoreEvent::WorktreeUpserted {
            session_id,
            worktree,
        } => Some((
            session_id.clone(),
            None,
            ProjectionEvent::WorktreeUpserted {
                worktree: worktree.clone(),
            },
        )),
        CoreEvent::AgentRunGroupUpserted { session_id, group } => Some((
            session_id.clone(),
            None,
            ProjectionEvent::AgentRunGroupUpserted {
                group: group.clone(),
            },
        )),
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
        CoreEvent::SessionUpdated { session_id, .. } => {
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
            let Some(owner_session_id) = session_id.clone().filter(|id| !id.is_empty()) else {
                tracing::warn!(
                    event_seq = envelope.event_seq,
                    "dropping ownerless job creation from session projection"
                );
                return result;
            };
            let event = ProjectionEvent::JobUpserted {
                job: codegg_protocol::projection::dto::JobProjection {
                    job_id: job_id.clone(),
                    workspace_id: workspace_id.clone(),
                    kind: kind.clone(),
                    state: "created".into(),
                    summary: String::new(),
                    session_id: Some(owner_session_id.clone()),
                    turn_id: None,
                    active_attempt_id: None,
                    error_class: None,
                    updated_at: envelope.timestamp_ms,
                },
            };
            Some((owner_session_id, envelope.turn_id.clone(), event))
        }
        CoreEvent::JobStarted {
            job_id, attempt_id, ..
        } => {
            let Some(owner_session_id) = envelope.session_id.clone().filter(|id| !id.is_empty())
            else {
                tracing::warn!(
                    event_seq = envelope.event_seq,
                    "dropping job lifecycle event without a session owner"
                );
                return result;
            };
            let event = ProjectionEvent::JobUpserted {
                job: codegg_protocol::projection::dto::JobProjection {
                    job_id: job_id.clone(),
                    workspace_id: String::new(),
                    kind: String::new(),
                    state: "running".into(),
                    summary: String::new(),
                    session_id: Some(owner_session_id.clone()),
                    turn_id: None,
                    active_attempt_id: Some(attempt_id.clone()),
                    error_class: None,
                    updated_at: envelope.timestamp_ms,
                },
            };
            Some((owner_session_id, envelope.turn_id.clone(), event))
        }
        CoreEvent::JobCompleted {
            job_id, attempt_id, ..
        } => {
            let Some(owner_session_id) = envelope.session_id.clone().filter(|id| !id.is_empty())
            else {
                tracing::warn!(
                    event_seq = envelope.event_seq,
                    "dropping job lifecycle event without a session owner"
                );
                return result;
            };
            let event = ProjectionEvent::JobUpserted {
                job: codegg_protocol::projection::dto::JobProjection {
                    job_id: job_id.clone(),
                    workspace_id: String::new(),
                    kind: String::new(),
                    state: "completed".into(),
                    summary: String::new(),
                    session_id: Some(owner_session_id.clone()),
                    turn_id: None,
                    active_attempt_id: Some(attempt_id.clone()),
                    error_class: None,
                    updated_at: envelope.timestamp_ms,
                },
            };
            Some((owner_session_id, envelope.turn_id.clone(), event))
        }
        CoreEvent::JobFailed {
            job_id,
            attempt_id,
            error_class,
            ..
        } => {
            let Some(owner_session_id) = envelope.session_id.clone().filter(|id| !id.is_empty())
            else {
                tracing::warn!(
                    event_seq = envelope.event_seq,
                    "dropping job lifecycle event without a session owner"
                );
                return result;
            };
            let event = ProjectionEvent::JobUpserted {
                job: codegg_protocol::projection::dto::JobProjection {
                    job_id: job_id.clone(),
                    workspace_id: String::new(),
                    kind: String::new(),
                    state: "failed".into(),
                    summary: String::new(),
                    session_id: Some(owner_session_id.clone()),
                    turn_id: None,
                    active_attempt_id: Some(attempt_id.clone()),
                    error_class: Some(error_class.clone()),
                    updated_at: envelope.timestamp_ms,
                },
            };
            Some((owner_session_id, envelope.turn_id.clone(), event))
        }
        CoreEvent::JobCancelled { job_id, attempt_id } => {
            let Some(owner_session_id) = envelope.session_id.clone().filter(|id| !id.is_empty())
            else {
                tracing::warn!(
                    event_seq = envelope.event_seq,
                    "dropping job lifecycle event without a session owner"
                );
                return result;
            };
            Some((
                owner_session_id.clone(),
                envelope.turn_id.clone(),
                ProjectionEvent::JobUpserted {
                    job: codegg_protocol::projection::dto::JobProjection {
                        job_id: job_id.clone(),
                        workspace_id: String::new(),
                        kind: String::new(),
                        state: "cancelled".into(),
                        summary: String::new(),
                        session_id: Some(owner_session_id),
                        turn_id: None,
                        active_attempt_id: Some(attempt_id.clone()),
                        error_class: None,
                        updated_at: envelope.timestamp_ms,
                    },
                },
            ))
        }
        CoreEvent::JobTimedOut { job_id, attempt_id } => {
            let Some(owner_session_id) = envelope.session_id.clone().filter(|id| !id.is_empty())
            else {
                tracing::warn!(
                    event_seq = envelope.event_seq,
                    "dropping job lifecycle event without a session owner"
                );
                return result;
            };
            Some((
                owner_session_id.clone(),
                envelope.turn_id.clone(),
                ProjectionEvent::JobUpserted {
                    job: codegg_protocol::projection::dto::JobProjection {
                        job_id: job_id.clone(),
                        workspace_id: String::new(),
                        kind: String::new(),
                        state: "timed_out".into(),
                        summary: String::new(),
                        session_id: Some(owner_session_id),
                        turn_id: None,
                        active_attempt_id: Some(attempt_id.clone()),
                        error_class: None,
                        updated_at: envelope.timestamp_ms,
                    },
                },
            ))
        }
        CoreEvent::JobInterrupted {
            job_id, attempt_id, ..
        } => {
            let Some(owner_session_id) = envelope.session_id.clone().filter(|id| !id.is_empty())
            else {
                tracing::warn!(
                    event_seq = envelope.event_seq,
                    "dropping job lifecycle event without a session owner"
                );
                return result;
            };
            Some((
                owner_session_id.clone(),
                envelope.turn_id.clone(),
                ProjectionEvent::JobUpserted {
                    job: codegg_protocol::projection::dto::JobProjection {
                        job_id: job_id.clone(),
                        workspace_id: String::new(),
                        kind: String::new(),
                        state: "interrupted".into(),
                        summary: String::new(),
                        session_id: Some(owner_session_id),
                        turn_id: None,
                        active_attempt_id: Some(attempt_id.clone()),
                        error_class: None,
                        updated_at: envelope.timestamp_ms,
                    },
                },
            ))
        }
        CoreEvent::JobRetried {
            job_id,
            new_attempt_id,
            ..
        } => {
            let Some(owner_session_id) = envelope.session_id.clone().filter(|id| !id.is_empty())
            else {
                tracing::warn!(
                    event_seq = envelope.event_seq,
                    "dropping job lifecycle event without a session owner"
                );
                return result;
            };
            Some((
                owner_session_id.clone(),
                envelope.turn_id.clone(),
                ProjectionEvent::JobUpserted {
                    job: codegg_protocol::projection::dto::JobProjection {
                        job_id: job_id.clone(),
                        workspace_id: String::new(),
                        kind: String::new(),
                        state: "queued".into(),
                        summary: String::new(),
                        session_id: Some(owner_session_id),
                        turn_id: None,
                        active_attempt_id: Some(new_attempt_id.clone()),
                        error_class: None,
                        updated_at: envelope.timestamp_ms,
                    },
                },
            ))
        }
        // Idle-delivery completion (C004 fanout, ADR-0031): one
        // `BackgroundJobCompleted` per logical terminal adapts to the
        // existing session job list so desktop/remote projection
        // consumers observe the terminal with no new event type and no
        // new authority. `workspace_id` stays empty by the same
        // convention as the arms above: the envelope carries no
        // workspace and this pure function must not fabricate one
        // (session binding owns scope). Length bounds are enforced by
        // `JobProjection::normalise` in the reducer; the summary is
        // already bounded at the notice commit.
        CoreEvent::BackgroundJobCompleted {
            session_id,
            job_id,
            kind,
            status,
            summary,
            ..
        } => {
            let event = ProjectionEvent::JobUpserted {
                job: codegg_protocol::projection::dto::JobProjection {
                    job_id: job_id.clone(),
                    workspace_id: String::new(),
                    kind: kind.clone(),
                    state: status.clone(),
                    summary: summary.clone(),
                    session_id: Some(session_id.clone()),
                    turn_id: None,
                    active_attempt_id: None,
                    error_class: None,
                    updated_at: envelope.timestamp_ms,
                },
            };
            Some((session_id.clone(), None, event))
        }
        CoreEvent::ToolProgramCompleted {
            program_id,
            job_id,
            status,
            summary,
            session_id,
            ..
        } => {
            let event = ProjectionEvent::ToolProgramTerminal {
                program_id: program_id.clone(),
                job_id: job_id.clone(),
                status: status.clone(),
                summary: summary.clone(),
                completed_at: envelope.timestamp_ms,
            };
            Some((session_id.clone().unwrap_or_default(), None, event))
        }
        CoreEvent::ToolProgramFailed {
            program_id,
            job_id,
            status,
            error,
            session_id,
            ..
        } => {
            let event = ProjectionEvent::ToolProgramTerminal {
                program_id: program_id.clone(),
                job_id: job_id.clone(),
                status: status.clone(),
                summary: error.clone(),
                completed_at: envelope.timestamp_ms,
            };
            Some((session_id.clone().unwrap_or_default(), None, event))
        }
        CoreEvent::ToolProgramUpdated {
            program_id,
            job_id,
            state,
            phase,
            calls_completed,
            summary,
            session_id,
            ..
        } => {
            let event = match state.as_str() {
                "admitted" => ProjectionEvent::ToolProgramAdmitted {
                    program_id: program_id.clone(),
                    job_id: job_id.clone(),
                    admitted_at: envelope.timestamp_ms,
                },
                "running" => ProjectionEvent::ToolProgramStarted {
                    program_id: program_id.clone(),
                    job_id: job_id.clone(),
                    attempt_id: phase.clone(),
                    started_at: envelope.timestamp_ms,
                },
                "progress" => ProjectionEvent::ToolProgramProgress {
                    program_id: program_id.clone(),
                    job_id: job_id.clone(),
                    message: summary.clone().unwrap_or_default(),
                    calls_completed: *calls_completed,
                    at: envelope.timestamp_ms,
                },
                "waiting_for_call" => {
                    let tool_name = phase.as_deref().unwrap_or("unknown");
                    ProjectionEvent::ToolProgramWaitingForCall {
                        program_id: program_id.clone(),
                        job_id: job_id.clone(),
                        call_id: String::new(),
                        tool_name: tool_name.to_string(),
                        at: envelope.timestamp_ms,
                    }
                }
                "waiting_for_job" => {
                    let depends_on = phase.as_deref().unwrap_or("");
                    ProjectionEvent::ToolProgramWaitingForJob {
                        program_id: program_id.clone(),
                        job_id: job_id.clone(),
                        depends_on_job_id: depends_on.to_string(),
                        at: envelope.timestamp_ms,
                    }
                }
                "retry_backoff" => ProjectionEvent::ToolProgramRetryBackoff {
                    program_id: program_id.clone(),
                    job_id: job_id.clone(),
                    attempt: *calls_completed,
                    backoff_ms: 0,
                    reason: summary.clone().unwrap_or_default(),
                    at: envelope.timestamp_ms,
                },
                _ => ProjectionEvent::Diagnostic {
                    code: "tool_program_updated".into(),
                    message: format!("program={program_id} state={state}"),
                },
            };
            Some((session_id.clone().unwrap_or_default(), None, event))
        }
        // M006-B: project-scoped, and routed on the **Project** stream only.
        //
        // Every other arm above is session-keyed and pushed to both streams.
        // Diagnostics must not be: the session reducer has no place for
        // project file state, and the payload carries source content that
        // belongs to the project's own authorization, not a session's. The
        // projection envelope is built in the early return above.
        CoreEvent::LspDiagnosticsUpdated { .. } => None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_protocol::projection::event::ProjectionEvent;

    fn started_envelope(display_name: Option<&str>) -> EventEnvelope<CoreEvent> {
        EventEnvelope {
            protocol_version: 1,
            event_seq: 7,
            timestamp_ms: 1_700_000_000_000,
            session_id: Some("s1".into()),
            turn_id: None,
            payload: CoreEvent::SubagentStarted {
                session_id: "s1".into(),
                child_instance_id: String::new(),
                originating_turn_id: None,
                task_id: 42,
                agent: "explore".into(),
                description: "map the auth flow".into(),
                display_name: display_name.map(str::to_string),
                // Mirrors the real publisher: a pool name has a descriptor
                // behind it, a fallback name has none. Replay must carry
                // whatever the recording captured, not invent it.
                profile: display_name.map(|_| "Reconnaissance generalist.".to_string()),
                usefulness: display_name.map(|_| "Useful for broad recon.".to_string()),
                run_id: Some("run-42".into()),
            },
        }
    }

    fn started_node(
        envelope: &EventEnvelope<CoreEvent>,
    ) -> codegg_protocol::projection::dto::AgentTreeNodeProjection {
        projection_events_from_core(envelope)
            .into_iter()
            .find_map(|(_, env)| match env.payload {
                ProjectionEvent::SubagentStarted { node } => Some(node),
                _ => None,
            })
            .expect("a SubagentStarted projection event")
    }

    /// The load-bearing guarantee: replay copies the recorded name instead of
    /// drawing a fresh one. A re-drawn name would make the historical view
    /// disagree with the live one about what a given subagent was called.
    #[test]
    fn replay_preserves_the_recorded_display_name() {
        let node = started_node(&started_envelope(Some("Sally")));
        assert_eq!(node.display_name.as_deref(), Some("Sally"));
    }

    #[test]
    fn job_lifecycle_projection_uses_the_trusted_event_envelope_owner() {
        let envelope = EventEnvelope {
            protocol_version: 1,
            event_seq: 11,
            timestamp_ms: 100,
            session_id: Some("owner-session".into()),
            turn_id: Some("turn-1".into()),
            payload: CoreEvent::JobStarted {
                job_id: "job-1".into(),
                attempt_id: "attempt-1".into(),
                // The lifecycle event carries no scope of its own here: the
                // envelope is the trusted owner, which is exactly what this
                // fixture exercises.
                session_id: None,
                workspace_id: None,
                kind: None,
            },
        };
        let events = projection_events_from_core(&envelope);
        assert_eq!(events.len(), 2);
        for (stream, projected) in events {
            assert_eq!(projected.session_id.as_deref(), Some("owner-session"));
            assert_eq!(projected.turn_id.as_deref(), Some("turn-1"));
            assert!(matches!(
                stream,
                ProjectionStreamKind::Session | ProjectionStreamKind::Project
            ));
            let ProjectionEvent::JobUpserted { job } = projected.payload else {
                panic!("expected a job upsert");
            };
            assert_eq!(job.session_id.as_deref(), Some("owner-session"));
            assert_eq!(job.state, "running");
        }
    }

    #[test]
    fn ownerless_job_lifecycle_is_dropped_instead_of_globalized() {
        let envelope = EventEnvelope {
            protocol_version: 1,
            event_seq: 12,
            timestamp_ms: 100,
            session_id: None,
            turn_id: None,
            payload: CoreEvent::JobStarted {
                job_id: "job-1".into(),
                attempt_id: "attempt-1".into(),
                // The lifecycle event carries no scope of its own here: the
                // envelope is the trusted owner, which is exactly what this
                // fixture exercises.
                session_id: None,
                workspace_id: None,
                kind: None,
            },
        };
        assert!(projection_events_from_core(&envelope).is_empty());
    }

    #[test]
    fn replay_preserves_the_recorded_run_id() {
        let node = started_node(&started_envelope(Some("Sally")));
        assert_eq!(node.run_id.as_deref(), Some("run-42"));
    }

    /// Two replays of the same recorded event must agree. This is the test
    /// that would fail if the name were minted on the replay path.
    #[test]
    fn replay_is_deterministic_across_repeated_application() {
        let envelope = started_envelope(Some("Sally-2"));
        let first = started_node(&envelope);
        for _ in 0..8 {
            let again = started_node(&envelope);
            assert_eq!(first, again, "replay diverged between runs");
        }
    }

    /// A node recorded before display names existed must still decode, with
    /// the field simply absent. Consumers fall back to the agent type.
    #[test]
    fn an_event_without_a_display_name_replays_as_absent() {
        let node = started_node(&started_envelope(None));
        assert_eq!(node.display_name, None);
        assert_eq!(node.agent, "explore", "the agent type is the fallback");
    }
}
