//! Projection event surface.
//!
//! A projection event is a typed mutation the reducer can apply on
//! top of an existing [`crate::projection::SessionProjectionSnapshot`].
//! Events carry a [`ProjectionEnvelope`] that mirrors the existing
//! [`crate::core::EventEnvelope`] shape: monotonic `event_seq`,
//! timestamp, optional session id, and a [`ProjectionStreamScope`].
//!
//! Events that arrive with a session id other than the snapshot's
//! primary session id are ignored unless the snapshot has a wildcard
//! multi-session [`ProjectionStreamScope::Project`] scope. Out-of-order
//! or impossible lifecycle transitions produce a
//! [`ProjectionEvent::Diagnostic`] entry but never panic the reducer.

use serde::{Deserialize, Serialize};

use crate::projection::caps::PROJECTION_PROTOCOL_VERSION;
use crate::projection::dto::{
    AgentTreeNodeProjection, ArtifactHandleProjection, FileChangeProjection, JobProjection,
    MessageProjection, PermissionProjection, PermissionStatus, QuestionProjection, RunProjection,
    SessionSummaryProjection, ToolArgumentProjection, ToolOutputProjection, ToolProjection,
    ToolStatus, TurnProjection, VisibilityClass,
};

/// Type tag prefix used when projection events are embedded in a
/// protocol envelope. Matches the existing `snake_case` tag
/// convention used by [`crate::core::CoreEvent`].
pub const EVENT_KIND_PREFIX: &str = "projection_";

/// Scope of a projection event stream.
///
/// `Project` is the default: a reducer receives events for any
/// session in the project. `Session` is the stricter scope used when a
/// client subscribes to one session only. `Workspace` and
/// `Daemon` cover the wider cross-project scope used by observer
/// views and the daemon overview panel.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionStreamScope {
    Project,
    #[default]
    Session,
    Workspace,
    Daemon,
}

/// Envelope carried by every projection event.
///
/// Mirrors [`crate::core::EventEnvelope`] but uses
/// [`ProjectionStreamScope`] and embeds the negotiated projection
/// protocol version so consumers can validate before applying.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionEnvelope {
    pub protocol_version: u32,
    pub event_seq: u64,
    pub timestamp_ms: i64,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub scope: ProjectionStreamScope,
    pub payload: ProjectionEvent,
}

impl ProjectionEnvelope {
    /// Build a session-scoped envelope with the current projection
    /// protocol version and `event_seq`.
    pub fn session_event(
        event_seq: u64,
        timestamp_ms: i64,
        session_id: impl Into<String>,
        turn_id: Option<String>,
        payload: ProjectionEvent,
    ) -> Self {
        Self {
            protocol_version: PROJECTION_PROTOCOL_VERSION,
            event_seq,
            timestamp_ms,
            session_id: Some(session_id.into()),
            turn_id,
            scope: ProjectionStreamScope::Session,
            payload,
        }
    }

    /// `true` when the envelope's scope and session id are compatible
    /// with `snapshot`. A session-scoped envelope is only compatible
    /// with a snapshot whose `primary_session_id` matches; a
    /// project-scoped envelope is compatible with any session of the
    /// same project.
    pub fn is_compatible_with(
        &self,
        snapshot: &crate::projection::SessionProjectionSnapshot,
    ) -> bool {
        match self.scope {
            ProjectionStreamScope::Daemon => true,
            ProjectionStreamScope::Workspace => self
                .session_id
                .as_ref()
                .map(|sid| snapshot.workspace_id == *sid || sid == &snapshot.primary_session_id)
                .unwrap_or(false),
            ProjectionStreamScope::Project => self
                .session_id
                .as_ref()
                .map(|sid| {
                    snapshot
                        .secondary_sessions
                        .iter()
                        .any(|s| s.session_id == *sid)
                        || snapshot.primary_session_id == *sid
                })
                .unwrap_or(false),
            ProjectionStreamScope::Session => self
                .session_id
                .as_ref()
                .map(|sid| sid == &snapshot.primary_session_id)
                .unwrap_or(false),
        }
    }
}

/// The set of mutations a reducer can apply to a session projection
/// snapshot.
///
/// Variants are intentionally additive: introducing a new variant
/// MUST NOT cause existing reducers to reject older events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectionEvent {
    /// A new session has been activated. The reducer replaces the
    /// primary session summary; secondary sessions are evicted when
    /// the cap is exceeded.
    SessionActivated { summary: SessionSummaryProjection },
    /// A new turn started inside the active session.
    TurnStarted { turn: TurnProjection },
    /// A bounded message (assistant text, user text, tool result)
    /// arrived for the active turn.
    MessageAppended { message: MessageProjection },
    /// A reasoning delta arrived for the active turn.
    ReasoningAppended { message_id: String, delta: String },
    /// A tool invocation started.
    ToolStarted { tool: ToolProjection },
    /// A tool invocation produced output / completed.
    ToolCompleted {
        tool_id: String,
        output: ToolOutputProjection,
        success: bool,
        duration_ms: u64,
        completed_at: i64,
    },
    /// A tool invocation failed without producing output.
    ToolFailed {
        tool_id: String,
        error: String,
        completed_at: i64,
    },
    /// A permission request was registered.
    PermissionPending { permission: PermissionProjection },
    /// A permission request was resolved.
    PermissionResolved {
        permission_id: String,
        status: PermissionStatus,
        resolved_at: i64,
    },
    /// An interactive question was registered.
    QuestionPending { question: QuestionProjection },
    /// An interactive question was resolved.
    QuestionResolved {
        question_id: String,
        status: PermissionStatus,
        resolved_at: i64,
    },
    /// The active turn completed.
    TurnCompleted {
        turn_id: String,
        stop_reason: String,
        completed_at: i64,
    },
    /// The active turn failed.
    TurnFailed {
        turn_id: String,
        message: String,
        failed_at: i64,
    },
    /// A subagent started.
    SubagentStarted { node: AgentTreeNodeProjection },
    /// A subagent reported progress (kept bounded).
    SubagentProgress {
        task_id: u64,
        message: String,
        at: i64,
    },
    /// A subagent completed.
    SubagentCompleted {
        task_id: u64,
        result_summary: String,
        completed_at: i64,
    },
    /// A subagent failed.
    SubagentFailed {
        task_id: u64,
        error: String,
        failed_at: i64,
    },
    /// A file change occurred (workspace-aware, summary only).
    FileChanged {
        change: FileChangeProjection,
        at: i64,
    },
    /// A run started (test, command, script).
    RunStarted { run: RunProjection },
    /// A run reported progress (kept bounded by the reducer).
    RunProgress {
        run_id: String,
        message: String,
        at: i64,
    },
    /// An artifact was produced for a run.
    RunArtifactCreated { artifact: ArtifactHandleProjection },
    /// A run completed.
    RunCompleted {
        run_id: String,
        status: String,
        summary: String,
        completed_at: i64,
    },
    /// A run was denied by policy.
    RunDenied {
        run_id: String,
        reason: String,
        at: i64,
    },
    /// A job (durable, Phase 4 contract) was created or updated.
    JobUpserted { job: JobProjection },
    /// A job was removed from the projection surface.
    JobRemoved { job_id: String },
    /// Token usage changed.
    TokenUsageUpdated {
        input_tokens: Option<usize>,
        output_tokens: Option<usize>,
    },
    /// Selected model changed.
    ModelSelected { model: String, at: i64 },
    /// Selected agent changed.
    AgentSelected { agent: String, at: i64 },
    /// Projection diagnostic — used by adapters and reducers to
    /// surface impossible or out-of-order transitions without
    /// mutating session state. The reducer appends the diagnostic to
    /// the snapshot's bounded diagnostics list.
    Diagnostic { code: String, message: String },
    /// Explicit resync signal. The reducer MUST rebuild the
    /// projection from a fresh snapshot; in-flight events whose
    /// `event_seq` is below `from_event_seq` are discarded.
    ResyncRequired {
        from_event_seq: u64,
        current_seq: u64,
    },
    /// Marker for variants a client does not yet recognise. Carries
    /// the original `kind` tag and a generic message so the reducer
    /// can record a single bounded diagnostic instead of crashing.
    Unknown {
        variant_name: String,
        notice: String,
    },
}

impl ProjectionEvent {
    /// `true` when the event is bounded entirely to `turn_id`. Events
    /// that affect multiple turns (e.g. file changes) return `false`.
    pub fn is_turn_scoped(&self, turn_id: &str) -> bool {
        match self {
            ProjectionEvent::TurnStarted { turn } => turn.turn_id == turn_id,
            ProjectionEvent::MessageAppended { message } => message.parent_turn_id == turn_id,
            ProjectionEvent::ReasoningAppended { .. } => false,
            ProjectionEvent::ToolStarted { tool: _ } => false,
            ProjectionEvent::ToolCompleted { .. } => false,
            ProjectionEvent::ToolFailed { .. } => false,
            ProjectionEvent::PermissionPending { .. } => false,
            ProjectionEvent::PermissionResolved { .. } => false,
            ProjectionEvent::QuestionPending { .. } => false,
            ProjectionEvent::QuestionResolved { .. } => false,
            ProjectionEvent::TurnCompleted { turn_id: tid, .. } => tid == turn_id,
            ProjectionEvent::TurnFailed { turn_id: tid, .. } => tid == turn_id,
            ProjectionEvent::SubagentStarted { .. } => false,
            ProjectionEvent::SubagentProgress { .. } => false,
            ProjectionEvent::SubagentCompleted { .. } => false,
            ProjectionEvent::SubagentFailed { .. } => false,
            ProjectionEvent::FileChanged { .. } => false,
            ProjectionEvent::RunStarted { .. } => false,
            ProjectionEvent::RunProgress { .. } => false,
            ProjectionEvent::RunArtifactCreated { .. } => false,
            ProjectionEvent::RunCompleted { .. } => false,
            ProjectionEvent::RunDenied { .. } => false,
            ProjectionEvent::JobUpserted { .. } => false,
            ProjectionEvent::JobRemoved { .. } => false,
            ProjectionEvent::TokenUsageUpdated { .. } => false,
            ProjectionEvent::ModelSelected { .. } => false,
            ProjectionEvent::AgentSelected { .. } => false,
            ProjectionEvent::Diagnostic { .. } => false,
            ProjectionEvent::ResyncRequired { .. } => false,
            ProjectionEvent::Unknown { .. } => false,
            _ => false,
        }
    }

    /// Visibility class for the user-visible text in the event, when
    /// applicable. Used by the reducer to redact sensitive payloads
    /// before adding them to the snapshot.
    pub fn visibility(&self) -> VisibilityClass {
        match self {
            ProjectionEvent::ReasoningAppended { .. } => VisibilityClass::Internal,
            ProjectionEvent::Diagnostic { .. } => VisibilityClass::ClientLocal,
            ProjectionEvent::ResyncRequired { .. } => VisibilityClass::Public,
            ProjectionEvent::Unknown { .. } => VisibilityClass::ClientLocal,
            ProjectionEvent::SubagentProgress { .. } => VisibilityClass::ClientLocal,
            ProjectionEvent::SubagentFailed { .. } => VisibilityClass::ClientLocal,
            ProjectionEvent::JobUpserted { .. } => VisibilityClass::ClientLocal,
            ProjectionEvent::JobRemoved { .. } => VisibilityClass::ClientLocal,
            _ => VisibilityClass::Public,
        }
    }
}

#[allow(clippy::too_many_arguments)]
impl ProjectionEvent {
    /// Convenience: build a [`ProjectionEvent::ToolStarted`] variant
    /// from raw fields, applying the standard [`ToolProjection::normalise`]
    /// pass.
    pub fn tool_started(
        tool_id: impl Into<String>,
        tool_name: impl Into<String>,
        arguments: ToolArgumentProjection,
        started_at: i64,
    ) -> Self {
        let mut tool = ToolProjection {
            tool_id: tool_id.into(),
            tool_name: tool_name.into(),
            status: ToolStatus::Started,
            arguments,
            output: ToolOutputProjection::Pending,
            visibility: VisibilityClass::Public,
            started_at: Some(started_at),
            completed_at: None,
            duration_ms: None,
        };
        tool.normalise();
        ProjectionEvent::ToolStarted { tool }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::snapshot::SessionProjectionSnapshot;

    fn empty_snapshot() -> SessionProjectionSnapshot {
        SessionProjectionSnapshot::empty("s1", "p1", "w1")
    }

    #[test]
    fn envelope_compatible_with_same_session() {
        let snap = empty_snapshot();
        let env = ProjectionEnvelope::session_event(
            1,
            0,
            "s1",
            None,
            ProjectionEvent::Diagnostic {
                code: "x".into(),
                message: "y".into(),
            },
        );
        assert!(env.is_compatible_with(&snap));
    }

    #[test]
    fn envelope_incompatible_with_different_session() {
        let snap = empty_snapshot();
        let env = ProjectionEnvelope::session_event(
            1,
            0,
            "s2",
            None,
            ProjectionEvent::Diagnostic {
                code: "x".into(),
                message: "y".into(),
            },
        );
        assert!(!env.is_compatible_with(&snap));
    }

    #[test]
    fn turn_scoped_event_matches_turn() {
        let event = ProjectionEvent::TurnCompleted {
            turn_id: "t1".into(),
            stop_reason: "ok".into(),
            completed_at: 0,
        };
        assert!(event.is_turn_scoped("t1"));
        assert!(!event.is_turn_scoped("t2"));
    }
}
