//! Frontend-neutral session projection contracts.
//!
//! This module defines the canonical, versioned, bounded projection
//! surface that all frontends (local TUI, remote TUI, future web /
//! observer / ACP clients) consume. It owns:
//!
//! * [`caps`] — projection version and capability negotiation.
//! * [`limits`] — explicit payload and collection bounds.
//! * [`dto`] — bounded summaries and references for sessions, turns,
//!   messages, tools, runs, jobs, permissions, questions, artifacts,
//!   agent tree placeholders, and workspace / project summaries.
//! * [`event`] — additive projection event variants carried in the
//!   existing protocol envelope.
//! * [`snapshot`] — one bounded session projection snapshot.
//! * [`reducer`] — a deterministic reducer that converts a snapshot
//!   plus an ordered event stream into the equivalent state.
//! * [`adapters`] — adapters that build projection snapshots and
//!   events from the existing [`crate::core::CoreResponse`] and
//!   [`crate::core::CoreEvent`] variants without replacing them.
//! * [`fixtures`] — golden fixtures and a test builder used by
//!   independent consumer tests.
//!
//! The projection is a *derived* frontend contract: it never becomes
//! a second session execution authority. The reducer is pure, never
//! performs I/O, and only mutates state for events whose scope matches
//! the snapshot. Unknown optional variants and version mismatches
//! degrade safely according to [`caps::ProjectionCapabilities`].
//!
//! ## Versioning and compatibility
//!
//! The current version is `PROJECTION_PROTOCOL_VERSION = 1`. Clients
//! and reducers negotiate through [`caps::ProjectionCapabilities`].
//! Unknown optional fields and variants are tolerated when the
//! negotiated version is within the declared compatible range; required
//! version mismatches produce an explicit [`event::ProjectionEvent::ResyncRequired`].

#![forbid(unsafe_code)]

pub mod adapters;
pub mod caps;
pub mod dto;
pub mod event;
pub mod fixtures;
pub mod limits;
pub mod reducer;
pub mod replay;
pub mod snapshot;

pub use caps::{
    ProjectionCapabilities, PROJECTION_CAPABILITY, PROJECTION_PROTOCOL_VERSION,
    PROJECTION_PROTOCOL_VERSION_MIN,
};
pub use dto::{
    AgentTreeNodeProjection, ArtifactHandleProjection, JobProjection, MessageProjection,
    PermissionProjection, ProjectSummaryProjection, QuestionProjection, RunProjection,
    SessionSummaryProjection, ToolProjection, TurnProjection, VisibilityClass,
    WorkspaceSummaryProjection,
};
pub use event::{ProjectionEnvelope, ProjectionEvent, ProjectionStreamScope, EVENT_KIND_PREFIX};
pub use fixtures::{
    active_turn_event_script, completed_event_script, completed_snapshot, file_change_event_script,
    fixture_reducer_config, idle_snapshot, job_event_script, permission_event_script,
    permission_pending_snapshot, project_summary_fixture, question_event_script,
    subagent_event_script, FIXTURE_PROJECT_ID, FIXTURE_SESSION_ID, FIXTURE_WORKSPACE_ID,
};
pub use limits::{
    MAX_PROJECTION_ARTIFACTS, MAX_PROJECTION_DIAGNOSTICS, MAX_PROJECTION_DIFF_LINES,
    MAX_PROJECTION_JOBS, MAX_PROJECTION_MESSAGES, MAX_PROJECTION_PENDING_PERMISSIONS,
    MAX_PROJECTION_PENDING_QUESTIONS, MAX_PROJECTION_RECENT_TOOLS, MAX_PROJECTION_RUNS,
    MAX_PROJECTION_RUN_SUMMARY_BYTES, MAX_PROJECTION_SESSIONS, MAX_PROJECTION_STRING_BYTES,
    MAX_PROJECTION_SUBAGENTS, MAX_PROJECTION_TOOL_ARGS_BYTES, MAX_PROJECTION_TOOL_OUTPUT_BYTES,
    MAX_PROJECTION_TRUNCATION_MARKER_BYTES,
};
pub use reducer::{
    ApplyOutcome, ProjectionReducer, ProjectionState, ReducerError, ReducerEventInput,
};
pub use replay::{
    ProjectionAck, ProjectionArtifactHandleDto, ProjectionArtifactReadOutcome,
    ProjectionArtifactReadRequest, ProjectionArtifactReadResponse, ProjectionCursor,
    ProjectionReplayBatch, ProjectionReplayLimits, ProjectionResyncReason,
    ProjectionSnapshotBundle, ProjectionStreamDescriptor, ProjectionStreamId,
    ProjectionStreamKind, ProjectionSubscriptionId, ProjectionSubscriptionRequest,
    ProjectionSubscriptionState, ProjectionSubscriptionStatus, ReplaySubscriptionError,
};
pub use snapshot::SessionProjectionSnapshot;
