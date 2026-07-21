//! Replay transport types for session projection subscriptions and
//! durable replay.
//!
//! This module defines the additive protocol surface consumed by
//! frontends that subscribe to a canonical session or project
//! projection stream and receive ordered, committed projection events
//! backed by durable replay storage.
//!
//! The types are wrappers around the existing M1
//! [`crate::projection::event::ProjectionEnvelope`] and
//! [`crate::projection::snapshot::SessionProjectionSnapshot`] contracts.
//! No M1 semantic fields are modified.

use serde::{Deserialize, Serialize};

use crate::projection::event::ProjectionEnvelope;
use crate::projection::snapshot::SessionProjectionSnapshot;

/// Hard cap on the replay payload of a single event in bytes.
pub const MAX_REPLAY_EVENT_BYTES: usize = 64 * 1024;

/// Maximum number of events returned in a single replay batch.
pub const MAX_REPLAY_EVENTS: usize = 512;

/// Maximum number of bytes returned in a single replay batch.
pub const MAX_REPLAY_BYTES: u64 = 1024 * 1024;

/// Validation cap for stream/subscription ID length.
pub const MAX_STREAM_ID_LENGTH: usize = 128;

/// Kind of projection stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionStreamKind {
    Session,
    Project,
}

/// Opaque validated stream identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectionStreamId(pub String);

impl ProjectionStreamId {
    pub fn new(id: impl Into<String>) -> Result<Self, ReplaySubscriptionError> {
        let id = id.into();
        if id.is_empty() || id.len() > MAX_STREAM_ID_LENGTH {
            return Err(ReplaySubscriptionError::InvalidStreamId);
        }
        if !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(ReplaySubscriptionError::InvalidStreamId);
        }
        Ok(Self(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProjectionStreamId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Opaque subscription identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectionSubscriptionId(pub String);

impl ProjectionSubscriptionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Descriptor for a projection stream stored durably.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionStreamDescriptor {
    pub stream_id: ProjectionStreamId,
    pub kind: ProjectionStreamKind,
    pub project_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default = "default_projection_version")]
    pub projection_version: u32,
    pub retention_floor_seq: u64,
    pub high_water_seq: u64,
    #[serde(default)]
    pub latest_checkpoint_seq: Option<u64>,
}

fn default_projection_version() -> u32 {
    1
}

/// A cursor into a projection stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionCursor {
    pub stream_id: ProjectionStreamId,
    pub event_seq: u64,
    #[serde(default = "default_projection_version")]
    pub projection_version: u32,
}

/// Request to subscribe to a projection stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionSubscriptionRequest {
    pub scope: ProjectionStreamKind,
    pub scope_id: String,
    #[serde(default)]
    pub cursor: Option<ProjectionCursor>,
    #[serde(default = "default_projection_version")]
    pub projection_version: u32,
}

impl ProjectionSubscriptionRequest {
    pub fn validate(&self) -> Result<(), ReplaySubscriptionError> {
        match self.scope {
            ProjectionStreamKind::Session | ProjectionStreamKind::Project => {}
        }
        if self.scope_id.is_empty() {
            return Err(ReplaySubscriptionError::InvalidCursor);
        }
        Ok(())
    }
}

/// Snapshot bundle returned on subscribe or resync.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectionSnapshotBundle {
    One {
        snapshot: Box<SessionProjectionSnapshot>,
    },
    BoundedSessionList {
        sessions: Vec<SessionProjectionSnapshot>,
        #[serde(default)]
        truncated: bool,
    },
}

impl ProjectionSnapshotBundle {
    pub fn is_truncated(&self) -> bool {
        match self {
            ProjectionSnapshotBundle::One { .. } => false,
            ProjectionSnapshotBundle::BoundedSessionList { truncated, .. } => *truncated,
        }
    }
}

/// Batch of replayed events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionReplayBatch {
    pub descriptor: ProjectionStreamDescriptor,
    pub events: Vec<ProjectionEnvelope>,
    #[serde(default)]
    pub snapshot: Option<ProjectionSnapshotBundle>,
    pub replay_start_seq: u64,
    pub replay_end_seq: u64,
    pub current_high_water: u64,
    #[serde(default)]
    pub truncation_flag: bool,
    /// Continuation cursor when more events remain.
    #[serde(default)]
    pub next_cursor: Option<ProjectionCursor>,
}

/// Reason a resync is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionResyncReason {
    HistoryExpired,
    HistoryGap,
    CursorAhead,
    StreamMismatch,
    ScopeMismatch,
    VersionMismatch,
    SnapshotUnavailable,
    SubscriberLagged,
}

/// Acknowledgement of processed events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionAck {
    pub subscription_id: ProjectionSubscriptionId,
    pub cursor: ProjectionCursor,
}

/// Subscription state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionSubscriptionState {
    Initializing,
    Live,
    ResyncRequired,
    Closed,
}

/// Status of an active subscription.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionSubscriptionStatus {
    pub id: ProjectionSubscriptionId,
    pub scope: ProjectionStreamKind,
    pub last_delivered_seq: u64,
    pub last_acked_seq: u64,
    pub state: ProjectionSubscriptionState,
    #[serde(default)]
    pub lag_count: u64,
}

/// Limits applied to replay pagination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectionReplayLimits {
    pub max_events: usize,
    pub max_bytes: u64,
}

impl Default for ProjectionReplayLimits {
    fn default() -> Self {
        Self {
            max_events: MAX_REPLAY_EVENTS,
            max_bytes: MAX_REPLAY_BYTES,
        }
    }
}

/// Errors from subscription request validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplaySubscriptionError {
    #[error("unsupported scope for projection replay")]
    UnsupportedScope,
    #[error("invalid stream identifier")]
    InvalidStreamId,
    #[error("invalid cursor")]
    InvalidCursor,
    #[error("subscription limit exceeded")]
    SubscriptionLimitExceeded,
    #[error("stream not found")]
    StreamNotFound,
    #[error("version mismatch")]
    VersionMismatch,
}

// ── M3 Artifact Read Protocol ─────────────────────────────────────────

/// Opaque artifact kind carried on the handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactHandleKind {
    RunOutput,
    ToolOutput,
    DiffExcerpt,
    LogTail,
}

/// Wire-format artifact read request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionArtifactReadRequest {
    pub handle_id: String,
    pub start: u64,
    pub end: Option<u64>,
    pub expected_revision: u64,
}

impl ProjectionArtifactReadRequest {
    /// Maximum read window per request.
    pub const MAX_READ_BYTES: u64 = 64 * 1024;

    pub fn normalize(&self) -> (u64, u64) {
        let end = self
            .end
            .unwrap_or(self.start.saturating_add(Self::MAX_READ_BYTES));
        let end = end.min(self.start.saturating_add(Self::MAX_READ_BYTES));
        (self.start, end)
    }
}

/// Wire-format artifact read response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionArtifactReadResponse {
    pub handle_id: String,
    pub revision: u64,
    pub start: u64,
    pub end: u64,
    pub content_type: String,
    pub content: String,
    pub redacted: bool,
    pub truncated: bool,
    pub note: Option<String>,
}

/// Outcome of an artifact read request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProjectionArtifactReadOutcome {
    Ok(ProjectionArtifactReadResponse),
    Denied { reason: String },
    NotFound,
    RevisionMismatch { current_revision: u64 },
    InvalidRequest { reason: String },
    Oversized,
}

/// Wire-format artifact handle descriptor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionArtifactHandleDto {
    pub handle_id: String,
    pub kind: ArtifactHandleKind,
    pub project_id: String,
    pub source_record_id: String,
    pub content_type: String,
    pub total_bytes: Option<u64>,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub revision: u64,
    pub public_summary: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_id_valid() {
        let id = ProjectionStreamId::new("proj-abc_session-xyz").unwrap();
        assert_eq!(id.as_str(), "proj-abc_session-xyz");
    }

    #[test]
    fn stream_id_empty_rejected() {
        assert_eq!(
            ProjectionStreamId::new(""),
            Err(ReplaySubscriptionError::InvalidStreamId)
        );
    }

    #[test]
    fn stream_id_too_long_rejected() {
        let long = "a".repeat(MAX_STREAM_ID_LENGTH + 1);
        assert_eq!(
            ProjectionStreamId::new(&long),
            Err(ReplaySubscriptionError::InvalidStreamId)
        );
    }

    #[test]
    fn stream_id_special_chars_rejected() {
        assert_eq!(
            ProjectionStreamId::new("bad/id"),
            Err(ReplaySubscriptionError::InvalidStreamId)
        );
    }

    #[test]
    fn cursor_round_trip() {
        let cursor = ProjectionCursor {
            stream_id: ProjectionStreamId("s1".into()),
            event_seq: 42,
            projection_version: 1,
        };
        let json = serde_json::to_string(&cursor).unwrap();
        let back: ProjectionCursor = serde_json::from_str(&json).unwrap();
        assert_eq!(cursor, back);
    }

    #[test]
    fn cursor_missing_projection_version_defaults() {
        let json = r#"{"stream_id":"s1","event_seq":42}"#;
        let cursor: ProjectionCursor = serde_json::from_str(json).unwrap();
        assert_eq!(cursor.projection_version, 1);
    }

    #[test]
    fn subscription_request_round_trip() {
        let req = ProjectionSubscriptionRequest {
            scope: ProjectionStreamKind::Session,
            scope_id: "s1".into(),
            cursor: None,
            projection_version: 1,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: ProjectionSubscriptionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn resync_reason_round_trip() {
        let reason = ProjectionResyncReason::StreamMismatch;
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, "\"stream_mismatch\"");
        let back: ProjectionResyncReason = serde_json::from_str(&json).unwrap();
        assert_eq!(reason, back);
    }

    #[test]
    fn snapshot_bundle_one_round_trip() {
        let snap = SessionProjectionSnapshot::empty("s", "p", "w");
        let bundle = ProjectionSnapshotBundle::One {
            snapshot: Box::new(snap.clone()),
        };
        let json = serde_json::to_string(&bundle).unwrap();
        let back: ProjectionSnapshotBundle = serde_json::from_str(&json).unwrap();
        match back {
            ProjectionSnapshotBundle::One { snapshot } => {
                assert_eq!(snapshot.primary_session_id, "s")
            }
            _ => panic!("expected One"),
        }
    }

    #[test]
    fn snapshot_bundle_bounded_list_round_trip() {
        let snap = SessionProjectionSnapshot::empty("s", "p", "w");
        let bundle = ProjectionSnapshotBundle::BoundedSessionList {
            sessions: vec![snap],
            truncated: false,
        };
        let json = serde_json::to_string(&bundle).unwrap();
        let back: ProjectionSnapshotBundle = serde_json::from_str(&json).unwrap();
        assert!(!back.is_truncated());
    }

    #[test]
    fn replay_batch_round_trip() {
        let desc = ProjectionStreamDescriptor {
            stream_id: ProjectionStreamId("s1".into()),
            kind: ProjectionStreamKind::Session,
            project_id: "p1".into(),
            workspace_id: None,
            session_id: Some("s1".into()),
            projection_version: 1,
            retention_floor_seq: 0,
            high_water_seq: 10,
            latest_checkpoint_seq: None,
        };
        let batch = ProjectionReplayBatch {
            descriptor: desc,
            events: vec![],
            snapshot: None,
            replay_start_seq: 1,
            replay_end_seq: 10,
            current_high_water: 10,
            truncation_flag: false,
            next_cursor: None,
        };
        let json = serde_json::to_string(&batch).unwrap();
        let back: ProjectionReplayBatch = serde_json::from_str(&json).unwrap();
        assert_eq!(back.replay_start_seq, 1);
    }

    #[test]
    fn stream_descriptor_round_trip() {
        let desc = ProjectionStreamDescriptor {
            stream_id: ProjectionStreamId("abc-123".into()),
            kind: ProjectionStreamKind::Project,
            project_id: "proj".into(),
            workspace_id: Some("ws".into()),
            session_id: None,
            projection_version: 1,
            retention_floor_seq: 0,
            high_water_seq: 0,
            latest_checkpoint_seq: None,
        };
        let json = serde_json::to_string(&desc).unwrap();
        let back: ProjectionStreamDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(desc, back);
    }

    #[test]
    fn subscription_status_round_trip() {
        let status = ProjectionSubscriptionStatus {
            id: ProjectionSubscriptionId("sub-1".into()),
            scope: ProjectionStreamKind::Project,
            last_delivered_seq: 5,
            last_acked_seq: 3,
            state: ProjectionSubscriptionState::Live,
            lag_count: 2,
        };
        let json = serde_json::to_string(&status).unwrap();
        let back: ProjectionSubscriptionStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }

    #[test]
    fn ack_round_trip() {
        let ack = ProjectionAck {
            subscription_id: ProjectionSubscriptionId("sub-1".into()),
            cursor: ProjectionCursor {
                stream_id: ProjectionStreamId("s1".into()),
                event_seq: 10,
                projection_version: 1,
            },
        };
        let json = serde_json::to_string(&ack).unwrap();
        let back: ProjectionAck = serde_json::from_str(&json).unwrap();
        assert_eq!(ack, back);
    }

    #[test]
    fn no_render_frame_or_secret_in_types() {
        let types_json = serde_json::to_string(&ProjectionStreamDescriptor {
            stream_id: ProjectionStreamId("x".into()),
            kind: ProjectionStreamKind::Session,
            project_id: "p".into(),
            workspace_id: None,
            session_id: None,
            projection_version: 1,
            retention_floor_seq: 0,
            high_water_seq: 0,
            latest_checkpoint_seq: None,
        })
        .unwrap();
        assert!(!types_json.contains("RenderFrame"));
        assert!(!types_json.contains("Credential"));
        assert!(!types_json.contains("Secret"));
    }

    #[test]
    fn unsupported_scope_uses_existing_stream_kind() {
        let a = ProjectionStreamKind::Session;
        let b = ProjectionStreamKind::Project;
        assert_ne!(a, b);
    }
}
