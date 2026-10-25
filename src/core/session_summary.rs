//! The session summary the daemon publishes.
//!
//! A summary is the session's *own* state: its title and timestamps come
//! from the durable record; its status, selection and counters from the
//! runtime.

use std::sync::atomic::Ordering;

use codegg_protocol::projection::dto::SessionSummaryProjection;

use super::session_runtime::SessionRuntime;

/// Build the session's own summary.
///
/// Neither input is required: an absent record is not an empty session.
pub(crate) async fn session_summary(
    session_id: &str,
    record: Option<&codegg_core::session::Session>,
    runtime: Option<&SessionRuntime>,
) -> SessionSummaryProjection {
    let mut summary = SessionSummaryProjection {
        session_id: session_id.to_string(),
        project_id: String::new(),
        workspace_id: String::new(),
        title: String::new(),
        status: "idle".to_string(),
        selected_model: None,
        selected_agent: None,
        has_active_turn: false,
        pending_permission_count: 0,
        pending_question_count: 0,
        input_tokens: None,
        output_tokens: None,
        active_subagents: 0,
        time_created_at: None,
        time_updated_at: None,
        recent_summary: None,
    };
    if let Some(record) = record {
        summary.title = record.title.clone();
        summary.time_created_at = Some(record.time_created);
        summary.time_updated_at = Some(record.time_updated);
        summary.project_id = record.project_id.clone();
        if let Some(workspace) = record.workspace_id.as_deref() {
            summary.workspace_id = workspace.to_string();
        }
    }
    if let Some(runtime) = runtime {
        if summary.project_id.is_empty() {
            summary.project_id = runtime.project_id.clone();
        }
        if summary.workspace_id.is_empty() {
            summary.workspace_id = runtime.workspace_id.as_str().to_string();
        }
        summary.status = format!("{:?}", *runtime.status.read().await);
        summary.selected_model = runtime.selected_model.read().await.clone();
        summary.selected_agent = runtime.selected_agent.read().await.clone();
        summary.has_active_turn = runtime.active_turn.read().await.is_some();
        summary.pending_permission_count = runtime.pending_permissions.len();
        summary.pending_question_count = runtime.pending_questions.len();
        summary.input_tokens = *runtime.last_input_tokens.read().await;
        summary.output_tokens = *runtime.last_output_tokens.read().await;
        summary.active_subagents = runtime.active_subagent_count.load(Ordering::Acquire);
    }
    summary
}
