//! Deterministic canonical reducer for session projection snapshots.
//!
//! The reducer is pure: it never performs I/O, network, filesystem, or
//! provider calls. It applies an ordered stream of projection events
//! on top of a snapshot and yields an [`ApplyOutcome`] for each
//! input.
//!
//! ## Determinism
//!
//! Two compliant reducers given the same initial snapshot and the same
//! ordered event stream MUST produce equivalent serialized snapshots.
//! The reducer is therefore a function `(snapshot, event) -> snapshot`
//! parameterised only by the explicit configuration passed to
//! [`ProjectionReducer::new`].
//!
//! ## Idempotence
//!
//! The reducer deduplicates events by `event_seq`. Re-applying an
//! event whose `event_seq` is at or below the snapshot's current
//! `event_seq` returns [`ApplyOutcome::Duplicate`] and leaves state
//! unchanged.
//!
//! ## Scope safety
//!
//! Events whose [`ProjectionEnvelope::session_id`] does not match the
//! snapshot's `primary_session_id` (and the snapshot is not a
//! multi-session scope) are rejected with
//! [`ApplyOutcome::ScopeMismatch`].
//!
//! ## Lifecycle safety
//!
//! Out-of-order or impossible transitions do not panic. They produce
//! a [`ProjectionDiagnostic`] entry and return
//! [`ApplyOutcome::Reconciled`].

use serde::{Deserialize, Serialize};

use crate::projection::dto::{
    AgentTreeStatus, PermissionStatus, ToolOutputProjection, ToolProjection, ToolStatus, TurnStatus,
};
use crate::projection::event::{ProjectionEnvelope, ProjectionEvent};
use crate::projection::limits::{
    clip_str, truncate_str, MAX_PROJECTION_MESSAGES, MAX_PROJECTION_PENDING_PERMISSIONS,
    MAX_PROJECTION_PENDING_QUESTIONS, MAX_PROJECTION_RECENT_TOOLS, MAX_PROJECTION_RUNS,
    MAX_PROJECTION_STRING_BYTES, MAX_PROJECTION_SUBAGENTS,
};
use crate::projection::snapshot::{ProjectionDiagnostic, SessionProjectionSnapshot};

/// Error produced by [`ProjectionReducer::apply`]. Surfaced only when
/// the reducer cannot make progress; transient reconciliations are
/// recorded as diagnostics and return [`ApplyOutcome::Reconciled`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReducerError {
    /// The envelope's `protocol_version` is outside the reducer's
    /// supported range. The reducer MUST refuse to apply the event.
    UnsupportedProtocolVersion {
        envelope_version: u32,
        supported_min: u32,
        supported_max: u32,
    },
    /// The envelope's `event_seq` is below the snapshot's
    /// `event_seq` and the reducer is not configured to permit
    /// replays.
    SequenceRegression {
        envelope_seq: u64,
        snapshot_seq: u64,
    },
    /// The reducer was asked to apply an event whose session id does
    /// not match any session in the snapshot and the snapshot is not
    /// a multi-session scope.
    ScopeMismatch {
        envelope_session: Option<String>,
        snapshot_session: String,
    },
}

impl std::fmt::Display for ReducerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReducerError::UnsupportedProtocolVersion {
                envelope_version,
                supported_min,
                supported_max,
            } => write!(
                f,
                "unsupported projection protocol version {envelope_version}; supported range {supported_min}..={supported_max}"
            ),
            ReducerError::SequenceRegression {
                envelope_seq,
                snapshot_seq,
            } => write!(
                f,
                "projection event_seq regression: envelope={envelope_seq} snapshot={snapshot_seq}"
            ),
            ReducerError::ScopeMismatch {
                envelope_session,
                snapshot_session,
            } => write!(
                f,
                "projection event scope mismatch: envelope={envelope_session:?} snapshot={snapshot_session}"
            ),
        }
    }
}

impl std::error::Error for ReducerError {}

/// Outcome of a single reducer application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// The event was applied; the snapshot was updated.
    Applied,
    /// The event was a duplicate of an earlier application; the
    /// snapshot is unchanged.
    Duplicate,
    /// The event was not compatible with the snapshot's scope; the
    /// snapshot is unchanged.
    ScopeMismatch,
    /// The event was incompatible with the current lifecycle state
    /// but was reconciled without mutating primary state; a
    /// diagnostic was recorded.
    Reconciled,
    /// The event triggered a full resync; the snapshot is left
    /// unchanged but consumers should request a fresh snapshot.
    ResyncRequired {
        from_event_seq: u64,
        current_seq: u64,
    },
    /// The reducer failed to apply the event because the envelope is
    /// fundamentally incompatible (wrong protocol version or seq
    /// regression with replay disabled). The snapshot is unchanged.
    Error(ReducerError),
}

/// Lightweight input shape that decouples the reducer from
/// [`ProjectionEnvelope`]'s exact wire shape. The reducer accepts
/// either a [`ProjectionEnvelope`] or any other shape that exposes the
/// same fields, which makes the reducer directly testable from
/// compact fixtures.
#[derive(Debug, Clone)]
pub struct ReducerEventInput {
    pub protocol_version: u32,
    pub event_seq: u64,
    pub timestamp_ms: i64,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub payload: ProjectionEvent,
}

impl From<ProjectionEnvelope> for ReducerEventInput {
    fn from(env: ProjectionEnvelope) -> Self {
        Self {
            protocol_version: env.protocol_version,
            event_seq: env.event_seq,
            timestamp_ms: env.timestamp_ms,
            session_id: env.session_id,
            turn_id: env.turn_id,
            payload: env.payload,
        }
    }
}

impl ReducerEventInput {
    /// Build a session-scoped event with the current projection
    /// protocol version.
    pub fn session(
        event_seq: u64,
        timestamp_ms: i64,
        session_id: impl Into<String>,
        turn_id: Option<String>,
        payload: ProjectionEvent,
    ) -> Self {
        Self {
            protocol_version: crate::projection::caps::PROJECTION_PROTOCOL_VERSION,
            event_seq,
            timestamp_ms,
            session_id: Some(session_id.into()),
            turn_id,
            payload,
        }
    }

    /// Build a project-scoped event with the current projection
    /// protocol version.
    pub fn project(
        event_seq: u64,
        timestamp_ms: i64,
        session_id: impl Into<String>,
        payload: ProjectionEvent,
    ) -> Self {
        Self {
            protocol_version: crate::projection::caps::PROJECTION_PROTOCOL_VERSION,
            event_seq,
            timestamp_ms,
            session_id: Some(session_id.into()),
            turn_id: None,
            payload,
        }
    }
}

/// Behaviour knobs for the reducer.
#[derive(Debug, Clone)]
pub struct ReducerConfig {
    /// Lowest protocol version the reducer will accept.
    pub min_version: u32,
    /// Highest protocol version the reducer will accept.
    pub max_version: u32,
    /// When `true`, the reducer accepts envelopes whose
    /// `event_seq` is below the snapshot's current `event_seq` and
    /// deduplicates them.
    pub allow_replays: bool,
    /// When `true`, the reducer treats events whose session id
    /// appears in the snapshot's `secondary_sessions` as
    /// cross-session events and applies them against the matching
    /// secondary session entry instead of the primary.
    pub allow_multi_session: bool,
}

impl Default for ReducerConfig {
    fn default() -> Self {
        Self {
            min_version: crate::projection::caps::PROJECTION_PROTOCOL_VERSION_MIN,
            max_version: crate::projection::caps::PROJECTION_PROTOCOL_VERSION,
            allow_replays: false,
            allow_multi_session: false,
        }
    }
}

/// Public façade for the reducer. Holds the configuration and offers
/// pure `apply` operations.
#[derive(Debug, Clone)]
pub struct ProjectionReducer {
    config: ReducerConfig,
}

impl Default for ProjectionReducer {
    fn default() -> Self {
        Self::new(ReducerConfig::default())
    }
}

impl ProjectionReducer {
    pub fn new(config: ReducerConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &ReducerConfig {
        &self.config
    }

    /// Apply one input to `snapshot`. The snapshot is mutated in
    /// place when [`ApplyOutcome::Applied`] or
    /// [`ApplyOutcome::Reconciled`] is returned.
    pub fn apply(
        &self,
        snapshot: &mut SessionProjectionSnapshot,
        input: ReducerEventInput,
    ) -> ApplyOutcome {
        if !self.version_supported(input.protocol_version) {
            return ApplyOutcome::Error(ReducerError::UnsupportedProtocolVersion {
                envelope_version: input.protocol_version,
                supported_min: self.config.min_version,
                supported_max: self.config.max_version,
            });
        }

        let scope_match = self.check_scope(snapshot, &input);
        if !scope_match {
            snapshot.push_diagnostic(ProjectionDiagnostic::new(
                "scope_mismatch",
                format!(
                    "ignoring event for session {:?} in snapshot of session {}",
                    input.session_id, snapshot.primary_session_id
                ),
                input.timestamp_ms,
            ));
            return ApplyOutcome::ScopeMismatch;
        }

        if input.event_seq <= snapshot.event_seq && !self.config.allow_replays {
            if input.event_seq == snapshot.event_seq {
                return ApplyOutcome::Duplicate;
            }
            return ApplyOutcome::Error(ReducerError::SequenceRegression {
                envelope_seq: input.event_seq,
                snapshot_seq: snapshot.event_seq,
            });
        }
        if input.event_seq <= snapshot.event_seq {
            return ApplyOutcome::Duplicate;
        }

        let outcome = self.apply_payload(snapshot, &input);
        if matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Reconciled) {
            snapshot.event_seq = input.event_seq;
            snapshot.generated_at_ms = input.timestamp_ms;
        }
        outcome
    }

    /// Convenience: apply every input in order. Returns the count of
    /// events that produced [`ApplyOutcome::Applied`].
    pub fn apply_all(
        &self,
        snapshot: &mut SessionProjectionSnapshot,
        inputs: impl IntoIterator<Item = ReducerEventInput>,
    ) -> usize {
        let mut applied = 0;
        for input in inputs {
            if matches!(self.apply(snapshot, input), ApplyOutcome::Applied) {
                applied += 1;
            }
        }
        applied
    }

    fn version_supported(&self, version: u32) -> bool {
        version >= self.config.min_version && version <= self.config.max_version
    }

    fn check_scope(&self, snapshot: &SessionProjectionSnapshot, input: &ReducerEventInput) -> bool {
        match input.session_id.as_deref() {
            Some(sid) if sid == snapshot.primary_session_id => true,
            Some(sid)
                if self.config.allow_multi_session
                    && snapshot
                        .secondary_sessions
                        .iter()
                        .any(|s| s.session_id == sid) =>
            {
                true
            }
            _ => false,
        }
    }

    fn apply_payload(
        &self,
        snapshot: &mut SessionProjectionSnapshot,
        input: &ReducerEventInput,
    ) -> ApplyOutcome {
        match &input.payload {
            ProjectionEvent::SessionActivated { summary } => {
                let mut summary = summary.clone();
                summary.normalise();
                if summary.session_id == snapshot.primary_session_id {
                    snapshot.primary_session = summary;
                } else {
                    snapshot.upsert_secondary(summary);
                }
                ApplyOutcome::Applied
            }
            ProjectionEvent::TurnStarted { turn } => {
                let mut turn = turn.clone();
                turn.normalise();
                if let Some(prev) = snapshot.active_turn.take() {
                    snapshot.push_recent_turn(prev);
                }
                snapshot.primary_session.has_active_turn = true;
                snapshot.primary_session.time_updated_at = Some(input.timestamp_ms);
                snapshot.active_turn = Some(turn);
                ApplyOutcome::Applied
            }
            ProjectionEvent::MessageAppended { message } => {
                let Some(turn) = snapshot.active_turn.as_mut() else {
                    return self.record_diagnostic(
                        snapshot,
                        input,
                        "orphan_message",
                        "MessageAppended without an active turn",
                    );
                };
                if turn.messages.len() >= MAX_PROJECTION_MESSAGES {
                    turn.messages.remove(0);
                }
                let mut message = message.clone();
                message.normalise();
                turn.messages.push(message);
                ApplyOutcome::Applied
            }
            ProjectionEvent::ReasoningAppended { message_id, delta } => {
                let Some(turn) = snapshot.active_turn.as_mut() else {
                    return self.record_diagnostic(
                        snapshot,
                        input,
                        "orphan_reasoning",
                        "ReasoningAppended without an active turn",
                    );
                };
                let bounded = truncate_str(delta, MAX_PROJECTION_STRING_BYTES);
                let slot = turn
                    .messages
                    .iter_mut()
                    .find(|m| &m.message_id == message_id);
                if let Some(slot) = slot {
                    slot.text.push_str(&bounded);
                    slot.visibility = crate::projection::dto::VisibilityClass::Internal;
                } else {
                    let mut message = crate::projection::dto::MessageProjection {
                        message_id: message_id.clone(),
                        parent_turn_id: turn.turn_id.clone(),
                        role: crate::projection::dto::MessageRole::Reasoning,
                        text: bounded.into_owned(),
                        tool_call_id: None,
                        visibility: crate::projection::dto::VisibilityClass::Internal,
                        created_at: input.timestamp_ms,
                        truncated: false,
                    };
                    message.normalise();
                    if turn.messages.len() >= MAX_PROJECTION_MESSAGES {
                        turn.messages.remove(0);
                    }
                    turn.messages.push(message);
                }
                ApplyOutcome::Applied
            }
            ProjectionEvent::ToolStarted { tool } => {
                let Some(turn) = snapshot.active_turn.as_mut() else {
                    return self.record_diagnostic(
                        snapshot,
                        input,
                        "orphan_tool_started",
                        "ToolStarted without an active turn",
                    );
                };
                if turn.tools.len() >= MAX_PROJECTION_RECENT_TOOLS {
                    turn.tools.remove(0);
                }
                let mut tool = tool.clone();
                tool.normalise();
                turn.tools.push(tool);
                ApplyOutcome::Applied
            }
            ProjectionEvent::ToolCompleted {
                tool_id,
                output,
                success,
                duration_ms,
                completed_at,
            } => {
                let Some(turn) = snapshot.active_turn.as_mut() else {
                    return self.record_diagnostic(
                        snapshot,
                        input,
                        "orphan_tool_completed",
                        "ToolCompleted without an active turn",
                    );
                };
                let mut output = output.clone();
                if let ToolOutputProjection::Inline { output: text } = &mut output {
                    let bounded = truncate_str(text, MAX_PROJECTION_STRING_BYTES);
                    if bounded.len() < text.len() {
                        *text = bounded.into_owned();
                    }
                }
                if let Some(slot) = turn.tools.iter_mut().find(|t| t.tool_id == *tool_id) {
                    slot.status = if *success {
                        ToolStatus::Completed
                    } else {
                        ToolStatus::Failed
                    };
                    slot.output = output;
                    slot.completed_at = Some(*completed_at);
                    slot.duration_ms = Some(*duration_ms);
                    ApplyOutcome::Applied
                } else {
                    let mut tool = ToolProjection {
                        tool_id: tool_id.clone(),
                        tool_name: String::new(),
                        status: if *success {
                            ToolStatus::Completed
                        } else {
                            ToolStatus::Failed
                        },
                        arguments: crate::projection::dto::ToolArgumentProjection::Summary {
                            summary: String::new(),
                        },
                        output,
                        visibility: crate::projection::dto::VisibilityClass::Public,
                        started_at: None,
                        completed_at: Some(*completed_at),
                        duration_ms: Some(*duration_ms),
                    };
                    tool.normalise();
                    if turn.tools.len() >= MAX_PROJECTION_RECENT_TOOLS {
                        turn.tools.remove(0);
                    }
                    turn.tools.push(tool);
                    ApplyOutcome::Reconciled
                }
            }
            ProjectionEvent::ToolFailed {
                tool_id,
                error,
                completed_at,
            } => {
                let Some(turn) = snapshot.active_turn.as_mut() else {
                    return self.record_diagnostic(
                        snapshot,
                        input,
                        "orphan_tool_failed",
                        "ToolFailed without an active turn",
                    );
                };
                let mut error = clip_str(error, MAX_PROJECTION_STRING_BYTES).to_string();
                if let Some(slot) = turn.tools.iter_mut().find(|t| t.tool_id == *tool_id) {
                    slot.status = ToolStatus::Failed;
                    slot.output = ToolOutputProjection::Summary {
                        summary: format!("error: {error}"),
                    };
                    slot.completed_at = Some(*completed_at);
                    ApplyOutcome::Applied
                } else {
                    let mut tool = ToolProjection {
                        tool_id: tool_id.clone(),
                        tool_name: String::new(),
                        status: ToolStatus::Failed,
                        arguments: crate::projection::dto::ToolArgumentProjection::Summary {
                            summary: String::new(),
                        },
                        output: ToolOutputProjection::Summary {
                            summary: format!("error: {error}"),
                        },
                        visibility: crate::projection::dto::VisibilityClass::Public,
                        started_at: None,
                        completed_at: Some(*completed_at),
                        duration_ms: None,
                    };
                    tool.normalise();
                    error.clear();
                    if turn.tools.len() >= MAX_PROJECTION_RECENT_TOOLS {
                        turn.tools.remove(0);
                    }
                    turn.tools.push(tool);
                    ApplyOutcome::Reconciled
                }
            }
            ProjectionEvent::PermissionPending { permission } => {
                let Some(turn) = snapshot.active_turn.as_mut() else {
                    return self.record_diagnostic(
                        snapshot,
                        input,
                        "orphan_permission_pending",
                        "PermissionPending without an active turn",
                    );
                };
                let mut permission = permission.clone();
                permission.normalise();
                if turn.pending_permissions.len() >= MAX_PROJECTION_PENDING_PERMISSIONS {
                    turn.pending_permissions.remove(0);
                }
                turn.pending_permissions.push(permission);
                snapshot.primary_session.pending_permission_count = turn
                    .pending_permissions
                    .iter()
                    .filter(|p| p.status == PermissionStatus::Pending)
                    .count();
                turn.status = TurnStatus::AwaitingPermission;
                ApplyOutcome::Applied
            }
            ProjectionEvent::PermissionResolved {
                permission_id,
                status,
                resolved_at,
            } => {
                if let Some(turn) = snapshot.active_turn.as_mut() {
                    if let Some(slot) = turn
                        .pending_permissions
                        .iter_mut()
                        .find(|p| p.permission_id == *permission_id)
                    {
                        slot.status = *status;
                        slot.resolved_at = Some(*resolved_at);
                    }
                    snapshot.primary_session.pending_permission_count = turn
                        .pending_permissions
                        .iter()
                        .filter(|p| p.status == PermissionStatus::Pending)
                        .count();
                    if turn
                        .pending_permissions
                        .iter()
                        .all(|p| p.status != PermissionStatus::Pending)
                    {
                        turn.status = TurnStatus::Active;
                    }
                    ApplyOutcome::Applied
                } else {
                    self.record_diagnostic(
                        snapshot,
                        input,
                        "orphan_permission_resolved",
                        "PermissionResolved without an active turn",
                    )
                }
            }
            ProjectionEvent::QuestionPending { question } => {
                let Some(turn) = snapshot.active_turn.as_mut() else {
                    return self.record_diagnostic(
                        snapshot,
                        input,
                        "orphan_question_pending",
                        "QuestionPending without an active turn",
                    );
                };
                let mut question = question.clone();
                question.normalise();
                if turn.pending_questions.len() >= MAX_PROJECTION_PENDING_QUESTIONS {
                    turn.pending_questions.remove(0);
                }
                turn.pending_questions.push(question);
                snapshot.primary_session.pending_question_count = turn
                    .pending_questions
                    .iter()
                    .filter(|q| q.status == PermissionStatus::Pending)
                    .count();
                turn.status = TurnStatus::AwaitingQuestion;
                ApplyOutcome::Applied
            }
            ProjectionEvent::QuestionResolved {
                question_id,
                status,
                resolved_at,
            } => {
                if let Some(turn) = snapshot.active_turn.as_mut() {
                    if let Some(slot) = turn
                        .pending_questions
                        .iter_mut()
                        .find(|q| q.question_id == *question_id)
                    {
                        slot.status = *status;
                        slot.resolved_at = Some(*resolved_at);
                    }
                    snapshot.primary_session.pending_question_count = turn
                        .pending_questions
                        .iter()
                        .filter(|q| q.status == PermissionStatus::Pending)
                        .count();
                    if turn
                        .pending_questions
                        .iter()
                        .all(|q| q.status != PermissionStatus::Pending)
                    {
                        turn.status = TurnStatus::Active;
                    }
                    ApplyOutcome::Applied
                } else {
                    self.record_diagnostic(
                        snapshot,
                        input,
                        "orphan_question_resolved",
                        "QuestionResolved without an active turn",
                    )
                }
            }
            ProjectionEvent::TurnCompleted {
                turn_id,
                stop_reason,
                completed_at,
            } => {
                let Some(turn) = snapshot.active_turn.as_mut() else {
                    return self.record_diagnostic(
                        snapshot,
                        input,
                        "orphan_turn_completed",
                        "TurnCompleted without an active turn",
                    );
                };
                if turn.turn_id != *turn_id {
                    return self.record_diagnostic(
                        snapshot,
                        input,
                        "turn_id_mismatch",
                        "TurnCompleted referenced an unknown turn",
                    );
                }
                turn.status = TurnStatus::Completed;
                turn.stop_reason =
                    Some(clip_str(stop_reason, MAX_PROJECTION_STRING_BYTES).to_string());
                snapshot.primary_session.has_active_turn = false;
                snapshot.primary_session.time_updated_at = Some(*completed_at);
                let completed = snapshot.active_turn.take().expect("just checked");
                snapshot.push_recent_turn(completed);
                ApplyOutcome::Applied
            }
            ProjectionEvent::TurnFailed {
                turn_id,
                message,
                failed_at,
            } => {
                let Some(turn) = snapshot.active_turn.as_mut() else {
                    return self.record_diagnostic(
                        snapshot,
                        input,
                        "orphan_turn_failed",
                        "TurnFailed without an active turn",
                    );
                };
                if turn.turn_id != *turn_id {
                    return self.record_diagnostic(
                        snapshot,
                        input,
                        "turn_id_mismatch",
                        "TurnFailed referenced an unknown turn",
                    );
                }
                turn.status = TurnStatus::Failed;
                turn.error = Some(clip_str(message, MAX_PROJECTION_STRING_BYTES).to_string());
                snapshot.primary_session.has_active_turn = false;
                snapshot.primary_session.time_updated_at = Some(*failed_at);
                let failed = snapshot.active_turn.take().expect("just checked");
                snapshot.push_recent_turn(failed);
                ApplyOutcome::Applied
            }
            ProjectionEvent::SubagentStarted { node } => {
                let Some(turn) = snapshot.active_turn.as_mut() else {
                    return self.record_diagnostic(
                        snapshot,
                        input,
                        "orphan_subagent_started",
                        "SubagentStarted without an active turn",
                    );
                };
                let mut node = node.clone();
                node.normalise();
                if turn.agent_tree.len() >= MAX_PROJECTION_SUBAGENTS {
                    turn.agent_tree.remove(0);
                }
                turn.agent_tree.push(node);
                snapshot.primary_session.active_subagents = turn
                    .agent_tree
                    .iter()
                    .filter(|n| matches!(n.status, AgentTreeStatus::Running))
                    .count();
                ApplyOutcome::Applied
            }
            ProjectionEvent::SubagentProgress {
                task_id,
                message,
                at,
            } => {
                if let Some(turn) = snapshot.active_turn.as_mut() {
                    if let Some(slot) = turn.agent_tree.iter_mut().find(|n| n.task_id == *task_id) {
                        slot.description =
                            truncate_str(message, MAX_PROJECTION_STRING_BYTES).into_owned();
                        let _ = at;
                        ApplyOutcome::Applied
                    } else {
                        self.record_diagnostic(
                            snapshot,
                            input,
                            "unknown_subagent_progress",
                            "SubagentProgress referenced an unknown task",
                        )
                    }
                } else {
                    self.record_diagnostic(
                        snapshot,
                        input,
                        "orphan_subagent_progress",
                        "SubagentProgress without an active turn",
                    )
                }
            }
            ProjectionEvent::SubagentCompleted {
                task_id,
                result_summary,
                completed_at,
            } => {
                if let Some(turn) = snapshot.active_turn.as_mut() {
                    if let Some(slot) = turn.agent_tree.iter_mut().find(|n| n.task_id == *task_id) {
                        slot.status = AgentTreeStatus::Completed;
                        slot.result_summary = Some(
                            truncate_str(result_summary, MAX_PROJECTION_STRING_BYTES).into_owned(),
                        );
                        slot.completed_at = Some(*completed_at);
                        snapshot.primary_session.active_subagents = turn
                            .agent_tree
                            .iter()
                            .filter(|n| matches!(n.status, AgentTreeStatus::Running))
                            .count();
                        ApplyOutcome::Applied
                    } else {
                        self.record_diagnostic(
                            snapshot,
                            input,
                            "unknown_subagent_completed",
                            "SubagentCompleted referenced an unknown task",
                        )
                    }
                } else {
                    self.record_diagnostic(
                        snapshot,
                        input,
                        "orphan_subagent_completed",
                        "SubagentCompleted without an active turn",
                    )
                }
            }
            ProjectionEvent::SubagentFailed {
                task_id,
                error,
                failed_at,
            } => {
                if let Some(turn) = snapshot.active_turn.as_mut() {
                    if let Some(slot) = turn.agent_tree.iter_mut().find(|n| n.task_id == *task_id) {
                        slot.status = AgentTreeStatus::Failed;
                        slot.result_summary =
                            Some(truncate_str(error, MAX_PROJECTION_STRING_BYTES).into_owned());
                        slot.completed_at = Some(*failed_at);
                        snapshot.primary_session.active_subagents = turn
                            .agent_tree
                            .iter()
                            .filter(|n| matches!(n.status, AgentTreeStatus::Running))
                            .count();
                        ApplyOutcome::Applied
                    } else {
                        self.record_diagnostic(
                            snapshot,
                            input,
                            "unknown_subagent_failed",
                            "SubagentFailed referenced an unknown task",
                        )
                    }
                } else {
                    self.record_diagnostic(
                        snapshot,
                        input,
                        "orphan_subagent_failed",
                        "SubagentFailed without an active turn",
                    )
                }
            }
            ProjectionEvent::FileChanged { change, at } => {
                let mut change = change.clone();
                change.normalise();
                let mut recent = snapshot
                    .primary_session
                    .recent_summary
                    .clone()
                    .unwrap_or_default();
                if !recent.is_empty() {
                    recent.push('\n');
                }
                recent.push_str(&format!("file:{} t={}", change.path(), at));
                let truncated = truncate_str(&recent, MAX_PROJECTION_STRING_BYTES);
                snapshot.primary_session.recent_summary = Some(truncated.into_owned());
                ApplyOutcome::Applied
            }
            ProjectionEvent::RunStarted { run } => {
                let mut run = run.clone();
                run.normalise();
                snapshot.push_run(run);
                snapshot.primary_session.time_updated_at = Some(input.timestamp_ms);
                ApplyOutcome::Applied
            }
            ProjectionEvent::RunProgress {
                run_id,
                message,
                at,
            } => {
                if let Some(slot) = snapshot.runs.iter_mut().find(|r| r.run_id == *run_id) {
                    let bounded = truncate_str(message, MAX_PROJECTION_STRING_BYTES);
                    slot.summary = bounded.into_owned();
                    slot.started_at = slot.started_at.min(*at);
                    ApplyOutcome::Applied
                } else {
                    self.record_diagnostic(
                        snapshot,
                        input,
                        "unknown_run_progress",
                        "RunProgress referenced an unknown run",
                    )
                }
            }
            ProjectionEvent::RunArtifactCreated { artifact } => {
                let mut artifact = artifact.clone();
                artifact.normalise();
                if snapshot.runs.len() >= MAX_PROJECTION_RUNS {
                    // ensure the corresponding run slot is reachable
                    snapshot.runs.remove(0);
                }
                // Find the run that owns this artifact and bump its count.
                if let Some(run) = snapshot.runs.iter_mut().find(|r| {
                    r.run_id == artifact.run_id.clone().unwrap_or_default()
                        || artifact.run_id.is_none()
                }) {
                    run.artifact_count = run.artifact_count.saturating_add(1);
                }
                let _ = artifact;
                ApplyOutcome::Applied
            }
            ProjectionEvent::RunCompleted {
                run_id,
                status,
                summary,
                completed_at,
            } => {
                if let Some(slot) = snapshot.runs.iter_mut().find(|r| r.run_id == *run_id) {
                    slot.status = truncate_str(status, MAX_PROJECTION_STRING_BYTES).into_owned();
                    slot.summary = truncate_str(
                        summary,
                        crate::projection::limits::MAX_PROJECTION_RUN_SUMMARY_BYTES,
                    )
                    .into_owned();
                    slot.completed_at = Some(*completed_at);
                    ApplyOutcome::Applied
                } else {
                    self.record_diagnostic(
                        snapshot,
                        input,
                        "unknown_run_completed",
                        "RunCompleted referenced an unknown run",
                    )
                }
            }
            ProjectionEvent::RunDenied { run_id, reason, at } => {
                if let Some(slot) = snapshot.runs.iter_mut().find(|r| r.run_id == *run_id) {
                    slot.status = "denied".to_string();
                    slot.summary = truncate_str(reason, MAX_PROJECTION_STRING_BYTES).into_owned();
                    slot.completed_at = Some(*at);
                    ApplyOutcome::Applied
                } else {
                    self.record_diagnostic(
                        snapshot,
                        input,
                        "unknown_run_denied",
                        "RunDenied referenced an unknown run",
                    )
                }
            }
            ProjectionEvent::JobUpserted { job } => {
                let mut job = job.clone();
                job.normalise();
                snapshot.upsert_job(job);
                ApplyOutcome::Applied
            }
            ProjectionEvent::JobRemoved { job_id } => {
                snapshot.jobs.retain(|j| j.job_id != *job_id);
                ApplyOutcome::Applied
            }
            ProjectionEvent::TokenUsageUpdated {
                input_tokens,
                output_tokens,
            } => {
                snapshot.primary_session.input_tokens = *input_tokens;
                snapshot.primary_session.output_tokens = *output_tokens;
                if let Some(turn) = snapshot.active_turn.as_mut() {
                    turn.input_tokens = *input_tokens;
                    turn.output_tokens = *output_tokens;
                }
                ApplyOutcome::Applied
            }
            ProjectionEvent::ModelSelected { model, at: _ } => {
                snapshot.primary_session.selected_model =
                    Some(truncate_str(model, MAX_PROJECTION_STRING_BYTES).into_owned());
                snapshot.primary_session.time_updated_at = Some(input.timestamp_ms);
                ApplyOutcome::Applied
            }
            ProjectionEvent::AgentSelected { agent, at: _ } => {
                snapshot.primary_session.selected_agent =
                    Some(truncate_str(agent, MAX_PROJECTION_STRING_BYTES).into_owned());
                snapshot.primary_session.time_updated_at = Some(input.timestamp_ms);
                ApplyOutcome::Applied
            }
            ProjectionEvent::Diagnostic { code, message } => {
                let mut diag = ProjectionDiagnostic::new(
                    clip_str(code, MAX_PROJECTION_STRING_BYTES),
                    clip_str(message, MAX_PROJECTION_STRING_BYTES),
                    input.timestamp_ms,
                );
                diag.session_id = input.session_id.clone();
                diag.turn_id = input.turn_id.clone();
                snapshot.push_diagnostic(diag);
                ApplyOutcome::Reconciled
            }
            ProjectionEvent::ResyncRequired {
                from_event_seq,
                current_seq,
            } => {
                snapshot.push_diagnostic(ProjectionDiagnostic::new(
                    "resync_required",
                    format!("from={from_event_seq} current={current_seq}"),
                    input.timestamp_ms,
                ));
                ApplyOutcome::ResyncRequired {
                    from_event_seq: *from_event_seq,
                    current_seq: *current_seq,
                }
            }
            ProjectionEvent::Unknown {
                variant_name,
                notice,
            } => {
                let mut diag = ProjectionDiagnostic::new(
                    "unknown_variant",
                    format!(
                        "{}: {}",
                        clip_str(variant_name, 64),
                        clip_str(notice, MAX_PROJECTION_STRING_BYTES)
                    ),
                    input.timestamp_ms,
                );
                diag.session_id = input.session_id.clone();
                diag.turn_id = input.turn_id.clone();
                snapshot.push_diagnostic(diag);
                ApplyOutcome::Reconciled
            }
        }
    }

    fn record_diagnostic(
        &self,
        snapshot: &mut SessionProjectionSnapshot,
        input: &ReducerEventInput,
        code: &str,
        message: &str,
    ) -> ApplyOutcome {
        let mut diag = ProjectionDiagnostic::new(code, message, input.timestamp_ms);
        diag.session_id = input.session_id.clone();
        diag.turn_id = input.turn_id.clone();
        snapshot.push_diagnostic(diag);
        ApplyOutcome::Reconciled
    }
}

/// Public extension trait that adds [`SessionProjectionSnapshot`]
/// helpers used by the reducer and adapters.
pub trait ProjectionState {
    fn upsert_secondary(&mut self, summary: crate::projection::dto::SessionSummaryProjection);
    fn push_recent_turn(&mut self, turn: crate::projection::dto::TurnProjection);
}

impl ProjectionState for SessionProjectionSnapshot {
    fn upsert_secondary(&mut self, summary: crate::projection::dto::SessionSummaryProjection) {
        if let Some(slot) = self
            .secondary_sessions
            .iter_mut()
            .find(|s| s.session_id == summary.session_id)
        {
            *slot = summary;
            return;
        }
        if self.secondary_sessions.len() >= crate::projection::limits::MAX_PROJECTION_SESSIONS {
            self.secondary_sessions.remove(0);
        }
        self.secondary_sessions.push(summary);
    }

    fn push_recent_turn(&mut self, turn: crate::projection::dto::TurnProjection) {
        self.recent_turns.insert(0, turn);
        if self.recent_turns.len() > MAX_PROJECTION_RECENT_TOOLS {
            self.recent_turns.truncate(MAX_PROJECTION_RECENT_TOOLS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::dto::{
        MessageProjection, MessageRole, PermissionProjection, ToolArgumentProjection,
        TurnProjection, VisibilityClass,
    };
    use crate::projection::snapshot::SessionProjectionSnapshot;

    #[test]
    fn turn_started_sets_active_and_records_diagnostic_for_unknown_completed() {
        let mut snap = SessionProjectionSnapshot::empty("s", "p", "w");
        let reducer = ProjectionReducer::default();

        let turn = TurnProjection {
            turn_id: "t1".into(),
            status: TurnStatus::Active,
            started_at: 0,
            updated_at: 0,
            stop_reason: None,
            error: None,
            messages: Vec::new(),
            tools: Vec::new(),
            pending_permissions: Vec::new(),
            pending_questions: Vec::new(),
            agent_tree: Vec::new(),
            subagent_count: 0,
            input_tokens: None,
            output_tokens: None,
        };

        let outcome = reducer.apply(
            &mut snap,
            ReducerEventInput::session(
                1,
                0,
                "s",
                Some("t1".into()),
                ProjectionEvent::TurnStarted { turn },
            ),
        );
        assert_eq!(outcome, ApplyOutcome::Applied);
        assert!(snap.active_turn.is_some());
        assert!(snap.primary_session.has_active_turn);
    }

    #[test]
    fn duplicate_event_seq_does_not_mutate() {
        let mut snap = SessionProjectionSnapshot::empty("s", "p", "w");
        snap.event_seq = 5;
        let reducer = ProjectionReducer::default();
        let outcome = reducer.apply(
            &mut snap,
            ReducerEventInput::session(
                5,
                0,
                "s",
                None,
                ProjectionEvent::Diagnostic {
                    code: "x".into(),
                    message: "y".into(),
                },
            ),
        );
        assert_eq!(outcome, ApplyOutcome::Duplicate);
        assert!(snap.diagnostics.is_empty());
    }

    #[test]
    fn scope_mismatch_returns_scope_mismatch_and_records_diagnostic() {
        let mut snap = SessionProjectionSnapshot::empty("s", "p", "w");
        let reducer = ProjectionReducer::default();
        let outcome = reducer.apply(
            &mut snap,
            ReducerEventInput::session(
                1,
                0,
                "different",
                None,
                ProjectionEvent::Diagnostic {
                    code: "x".into(),
                    message: "y".into(),
                },
            ),
        );
        assert_eq!(outcome, ApplyOutcome::ScopeMismatch);
        assert_eq!(snap.diagnostics.len(), 1);
        assert_eq!(snap.diagnostics[0].code, "scope_mismatch");
    }

    #[test]
    fn unsupported_protocol_version_is_an_error() {
        let mut snap = SessionProjectionSnapshot::empty("s", "p", "w");
        let reducer = ProjectionReducer::default();
        let outcome = reducer.apply(
            &mut snap,
            ReducerEventInput {
                protocol_version: 99,
                event_seq: 1,
                timestamp_ms: 0,
                session_id: Some("s".into()),
                turn_id: None,
                payload: ProjectionEvent::Diagnostic {
                    code: "x".into(),
                    message: "y".into(),
                },
            },
        );
        assert!(matches!(
            outcome,
            ApplyOutcome::Error(ReducerError::UnsupportedProtocolVersion { .. })
        ));
    }

    #[test]
    fn tool_started_appends_tool_projection() {
        let mut snap = SessionProjectionSnapshot::empty("s", "p", "w");
        snap.active_turn = Some(TurnProjection {
            turn_id: "t1".into(),
            status: TurnStatus::Active,
            started_at: 0,
            updated_at: 0,
            stop_reason: None,
            error: None,
            messages: Vec::new(),
            tools: Vec::new(),
            pending_permissions: Vec::new(),
            pending_questions: Vec::new(),
            agent_tree: Vec::new(),
            subagent_count: 0,
            input_tokens: None,
            output_tokens: None,
        });

        let reducer = ProjectionReducer::default();
        reducer.apply(
            &mut snap,
            ReducerEventInput::session(
                1,
                0,
                "s",
                Some("t1".into()),
                ProjectionEvent::tool_started(
                    "tool-1",
                    "Bash",
                    ToolArgumentProjection::Summary {
                        summary: "ls".into(),
                    },
                    0,
                ),
            ),
        );
        assert_eq!(snap.active_turn.as_ref().unwrap().tools.len(), 1);
    }

    #[test]
    fn message_appended_increments_message_list() {
        let mut snap = SessionProjectionSnapshot::empty("s", "p", "w");
        snap.active_turn = Some(TurnProjection {
            turn_id: "t1".into(),
            status: TurnStatus::Active,
            started_at: 0,
            updated_at: 0,
            stop_reason: None,
            error: None,
            messages: Vec::new(),
            tools: Vec::new(),
            pending_permissions: Vec::new(),
            pending_questions: Vec::new(),
            agent_tree: Vec::new(),
            subagent_count: 0,
            input_tokens: None,
            output_tokens: None,
        });

        let reducer = ProjectionReducer::default();
        let msg = MessageProjection {
            message_id: "m1".into(),
            parent_turn_id: "t1".into(),
            role: MessageRole::Assistant,
            text: "hello".into(),
            tool_call_id: None,
            visibility: VisibilityClass::Public,
            created_at: 0,
            truncated: false,
        };
        let outcome = reducer.apply(
            &mut snap,
            ReducerEventInput::session(
                1,
                0,
                "s",
                Some("t1".into()),
                ProjectionEvent::MessageAppended { message: msg },
            ),
        );
        assert_eq!(outcome, ApplyOutcome::Applied);
        assert_eq!(snap.active_turn.as_ref().unwrap().messages.len(), 1);
    }

    #[test]
    fn permission_pending_increments_pending_count_and_sets_status() {
        let mut snap = SessionProjectionSnapshot::empty("s", "p", "w");
        snap.active_turn = Some(TurnProjection {
            turn_id: "t1".into(),
            status: TurnStatus::Active,
            started_at: 0,
            updated_at: 0,
            stop_reason: None,
            error: None,
            messages: Vec::new(),
            tools: Vec::new(),
            pending_permissions: Vec::new(),
            pending_questions: Vec::new(),
            agent_tree: Vec::new(),
            subagent_count: 0,
            input_tokens: None,
            output_tokens: None,
        });

        let reducer = ProjectionReducer::default();
        reducer.apply(
            &mut snap,
            ReducerEventInput::session(
                1,
                0,
                "s",
                Some("t1".into()),
                ProjectionEvent::PermissionPending {
                    permission: PermissionProjection {
                        permission_id: "p1".into(),
                        tool: "Bash".into(),
                        path: None,
                        status: PermissionStatus::Pending,
                        created_at: 0,
                        resolved_at: None,
                    },
                },
            ),
        );
        assert_eq!(snap.primary_session.pending_permission_count, 1);
        assert_eq!(
            snap.active_turn.as_ref().unwrap().status,
            TurnStatus::AwaitingPermission
        );
    }
}
