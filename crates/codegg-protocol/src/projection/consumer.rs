//! Headless reference consumer for the session projection protocol.
//!
//! This module is deliberately transport-neutral: a connected/authenticated
//! transport hands [`HeadlessProjectionConsumer`] the canonical
//! `CoreResponse` values it received from the daemon.  The consumer owns only
//! bounded, reconnectable presentation state; the daemon remains the
//! subscription, cursor, artifact, and execution authority.

use std::collections::{HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::core::{CoreRequest, CoreResponse};
use crate::projection::caps::{ProjectionCapabilities, PROJECTION_PROTOCOL_VERSION};
use crate::projection::dto::{ToolOutputProjection, VisibilityClass};
use crate::projection::event::{ProjectionEnvelope, ProjectionEvent, ProjectionStreamScope};
use crate::projection::reducer::{ApplyOutcome, ProjectionReducer, ReducerError};
use crate::projection::replay::{
    ProjectionArtifactHandleDto, ProjectionArtifactReadOutcome, ProjectionArtifactReadRequest,
    ProjectionArtifactReadResponse, ProjectionCursor, ProjectionReplayBatch,
    ProjectionResyncReason, ProjectionSnapshotBundle, ProjectionStreamDescriptor,
    ProjectionSubscriptionId, ProjectionSubscriptionRequest,
};
use crate::projection::snapshot::SessionProjectionSnapshot;

/// Maximum diagnostics retained by the reference consumer.
pub const MAX_HEADLESS_DIAGNOSTICS: usize = 32;

/// Connection and subscription state visible to a headless caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadlessConnectionState {
    Disconnected,
    Connected,
    Attached,
    ResyncRequired,
}

/// Result of applying one event to the headless state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadlessEventOutcome {
    Applied {
        cursor: ProjectionCursor,
    },
    Duplicate {
        cursor: ProjectionCursor,
    },
    Reconciled {
        cursor: ProjectionCursor,
    },
    IgnoredNonPublic {
        event_seq: u64,
    },
    ResyncRequired {
        reason: ProjectionResyncReason,
        cursor: ProjectionCursor,
    },
    Error(ReducerError),
}

/// Result of ingesting a replay response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadlessReplayOutcome {
    pub applied: usize,
    pub duplicates: usize,
    pub reconciled: usize,
    pub next_cursor: Option<ProjectionCursor>,
}

/// Errors raised before a request is sent or when a response cannot be
/// safely incorporated into the bounded consumer state.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HeadlessConsumerError {
    #[error("projection consumer is not connected")]
    NotConnected,
    #[error("projection capabilities are unsupported")]
    UnsupportedCapabilities,
    #[error("projection consumer has no attached session")]
    NotAttached,
    #[error("projection response was not a single-session snapshot")]
    InvalidSnapshotBundle,
    #[error("projection response stream does not match the attached session")]
    StreamMismatch,
    #[error("projection response is not valid for the current consumer state")]
    InvalidResponse,
    #[error("projection artifact handle is not safe for public consumption")]
    UnsafeArtifactHandle,
    #[error("projection artifact handle belongs to another project")]
    ForeignArtifactHandle,
    #[error("projection artifact read range is invalid")]
    InvalidArtifactRange,
    #[error("projection artifact response exceeds the bounded read limit")]
    ArtifactTooLarge,
    #[error("projection artifact response does not match the requested handle")]
    ArtifactHandleMismatch,
}

/// Small non-TUI consumer used by headless observers, automation, and
/// reference tests.
pub struct HeadlessProjectionConsumer {
    capabilities: ProjectionCapabilities,
    negotiated_version: Option<u32>,
    connection_state: HeadlessConnectionState,
    session_id: Option<String>,
    descriptor: Option<ProjectionStreamDescriptor>,
    subscription_id: Option<ProjectionSubscriptionId>,
    snapshot: Option<SessionProjectionSnapshot>,
    cursor: Option<ProjectionCursor>,
    last_resync_reason: Option<ProjectionResyncReason>,
    diagnostics: VecDeque<String>,
    artifact_handles: Vec<ProjectionArtifactHandleDto>,
    last_artifact: Option<ProjectionArtifactReadResponse>,
    reducer: ProjectionReducer,
}

impl Default for HeadlessProjectionConsumer {
    fn default() -> Self {
        Self::new()
    }
}

impl HeadlessProjectionConsumer {
    /// Create a consumer advertising the current projection capability.
    pub fn new() -> Self {
        Self::with_capabilities(ProjectionCapabilities::current())
    }

    pub fn with_capabilities(capabilities: ProjectionCapabilities) -> Self {
        Self {
            capabilities,
            negotiated_version: None,
            connection_state: HeadlessConnectionState::Disconnected,
            session_id: None,
            descriptor: None,
            subscription_id: None,
            snapshot: None,
            cursor: None,
            last_resync_reason: None,
            diagnostics: VecDeque::new(),
            artifact_handles: Vec::new(),
            last_artifact: None,
            reducer: ProjectionReducer::default(),
        }
    }

    pub fn capabilities(&self) -> &ProjectionCapabilities {
        &self.capabilities
    }

    pub fn connection_state(&self) -> HeadlessConnectionState {
        self.connection_state
    }

    pub fn negotiated_version(&self) -> Option<u32> {
        self.negotiated_version
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn descriptor(&self) -> Option<&ProjectionStreamDescriptor> {
        self.descriptor.as_ref()
    }

    pub fn subscription_id(&self) -> Option<&ProjectionSubscriptionId> {
        self.subscription_id.as_ref()
    }

    pub fn snapshot(&self) -> Option<&SessionProjectionSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn cursor(&self) -> Option<&ProjectionCursor> {
        self.cursor.as_ref()
    }

    pub fn last_resync_reason(&self) -> Option<ProjectionResyncReason> {
        self.last_resync_reason
    }

    pub fn diagnostics(&self) -> impl Iterator<Item = &str> {
        self.diagnostics.iter().map(String::as_str)
    }

    pub fn artifact_handles(&self) -> &[ProjectionArtifactHandleDto] {
        &self.artifact_handles
    }

    pub fn last_artifact(&self) -> Option<&ProjectionArtifactReadResponse> {
        self.last_artifact.as_ref()
    }

    /// Accept capabilities from an already authenticated transport.
    ///
    /// Authentication belongs to the transport boundary. This method only
    /// records the negotiated projection version and refuses a daemon that
    /// cannot provide incremental projection events.
    pub fn connect(
        &mut self,
        daemon: &ProjectionCapabilities,
    ) -> Result<u32, HeadlessConsumerError> {
        let Some(version) = ProjectionCapabilities::negotiate(&self.capabilities, daemon) else {
            self.connection_state = HeadlessConnectionState::Disconnected;
            self.push_diagnostic("no compatible projection version");
            return Err(HeadlessConsumerError::UnsupportedCapabilities);
        };
        if !(crate::projection::caps::PROJECTION_PROTOCOL_VERSION_MIN..=PROJECTION_PROTOCOL_VERSION)
            .contains(&version)
        {
            self.connection_state = HeadlessConnectionState::Disconnected;
            self.push_diagnostic("negotiated projection version is unsupported");
            return Err(HeadlessConsumerError::UnsupportedCapabilities);
        }
        if !daemon.supports_incremental_events || !self.capabilities.supports_incremental_events {
            self.connection_state = HeadlessConnectionState::Disconnected;
            self.push_diagnostic("incremental projection events are unsupported");
            return Err(HeadlessConsumerError::UnsupportedCapabilities);
        }
        self.negotiated_version = Some(version);
        self.connection_state = HeadlessConnectionState::Connected;
        self.last_resync_reason = None;
        Ok(version)
    }

    /// Consume the daemon's typed capability response.
    pub fn connect_from_response(
        &mut self,
        response: &CoreResponse,
    ) -> Result<u32, HeadlessConsumerError> {
        let CoreResponse::ProjectionCapabilitiesResponse {
            supported,
            projection_version,
            ..
        } = response
        else {
            return Err(HeadlessConsumerError::InvalidResponse);
        };
        if !supported {
            self.connection_state = HeadlessConnectionState::Disconnected;
            self.push_diagnostic("daemon does not support session projections");
            return Err(HeadlessConsumerError::UnsupportedCapabilities);
        }
        self.connect(&ProjectionCapabilities {
            min_version: *projection_version,
            max_version: *projection_version,
            ..ProjectionCapabilities::current()
        })
    }

    /// Mark the transport interrupted while retaining the last accepted
    /// cursor and bounded snapshot for a resume request.
    pub fn disconnect(&mut self) {
        self.connection_state = HeadlessConnectionState::Disconnected;
        self.subscription_id = None;
        self.last_artifact = None;
    }

    /// Build a fresh session subscription request. After an interruption,
    /// callers should use [`Self::resume_request`] so the daemon remains the
    /// replay authority.
    pub fn attach_request(
        &self,
        session_id: impl Into<String>,
    ) -> Result<ProjectionSubscriptionRequest, HeadlessConsumerError> {
        if self.connection_state == HeadlessConnectionState::Disconnected {
            return Err(HeadlessConsumerError::NotConnected);
        }
        Ok(ProjectionSubscriptionRequest {
            scope: crate::projection::replay::ProjectionStreamKind::Session,
            scope_id: session_id.into(),
            cursor: None,
            projection_version: self
                .negotiated_version
                .unwrap_or(PROJECTION_PROTOCOL_VERSION),
        })
    }

    /// Build the canonical cursor-based resume request after interruption or
    /// when a replay batch advertises a continuation cursor.
    pub fn resume_request(&self) -> Result<CoreRequest, HeadlessConsumerError> {
        if self.connection_state == HeadlessConnectionState::Disconnected {
            return Err(HeadlessConsumerError::NotConnected);
        }
        let Some(cursor) = self.cursor.clone() else {
            return Err(HeadlessConsumerError::NotAttached);
        };
        Ok(CoreRequest::ProjectionResume {
            cursor,
            include_snapshot_if_resync: true,
        })
    }

    /// Build a typed request suitable for an authenticated core transport.
    pub fn attach_core_request(
        &self,
        session_id: impl Into<String>,
    ) -> Result<CoreRequest, HeadlessConsumerError> {
        Ok(CoreRequest::ProjectionSubscribe {
            request: self.attach_request(session_id)?,
        })
    }

    /// Install the canonical subscribe response and its initial snapshot.
    pub fn accept_subscribed(
        &mut self,
        subscription_id: ProjectionSubscriptionId,
        descriptor: ProjectionStreamDescriptor,
        snapshot: ProjectionSnapshotBundle,
        cursor: ProjectionCursor,
    ) -> Result<(), HeadlessConsumerError> {
        let snapshot = Self::single_snapshot(snapshot)?;
        self.validate_stream(&descriptor, &snapshot)?;
        if cursor.stream_id != descriptor.stream_id
            || cursor.projection_version != descriptor.projection_version
        {
            return Err(HeadlessConsumerError::StreamMismatch);
        }
        let mut snapshot = *snapshot;
        Self::sanitize_snapshot(&mut snapshot);
        snapshot.event_seq = cursor.event_seq;
        self.session_id = Some(snapshot.primary_session_id.clone());
        self.descriptor = Some(descriptor);
        self.subscription_id = Some(subscription_id);
        self.snapshot = Some(snapshot);
        self.cursor = Some(cursor);
        self.connection_state = HeadlessConnectionState::Attached;
        self.last_resync_reason = None;
        Ok(())
    }

    /// Consume any canonical projection response emitted by the core
    /// protocol. Transport-specific framing remains outside this module.
    pub fn accept_response(
        &mut self,
        response: &CoreResponse,
    ) -> Result<Option<HeadlessReplayOutcome>, HeadlessConsumerError> {
        match response {
            CoreResponse::ProjectionCapabilitiesResponse { .. } => {
                self.connect_from_response(response)?;
                Ok(None)
            }
            CoreResponse::ProjectionSubscribed {
                subscription_id,
                descriptor,
                snapshot,
                cursor,
                ..
            } => {
                self.accept_subscribed(
                    subscription_id.clone(),
                    descriptor.clone(),
                    snapshot.clone(),
                    cursor.clone(),
                )?;
                Ok(None)
            }
            CoreResponse::ProjectionReplay {
                subscription_id,
                batch,
            } => {
                if let Some(subscription_id) = subscription_id {
                    self.subscription_id = Some(subscription_id.clone());
                }
                Ok(Some(self.accept_replay(batch)?))
            }
            CoreResponse::ProjectionResyncRequired {
                reason,
                descriptor,
                snapshot,
                ..
            } => {
                self.connection_state = HeadlessConnectionState::ResyncRequired;
                self.last_resync_reason = Some(*reason);
                if let (Some(descriptor), Some(snapshot)) = (descriptor, snapshot) {
                    let subscription_id = self
                        .subscription_id
                        .clone()
                        .unwrap_or_else(|| ProjectionSubscriptionId::new("resync"));
                    self.accept_subscribed(
                        subscription_id,
                        descriptor.clone(),
                        snapshot.clone(),
                        ProjectionCursor {
                            stream_id: descriptor.stream_id.clone(),
                            event_seq: match snapshot {
                                ProjectionSnapshotBundle::One { snapshot } => snapshot.event_seq,
                                ProjectionSnapshotBundle::BoundedSessionList { .. } => 0,
                            },
                            projection_version: descriptor.projection_version,
                        },
                    )?;
                }
                Ok(None)
            }
            CoreResponse::ProjectionArtifactList { handles } => {
                self.accept_artifact_handles(handles.clone())?;
                Ok(None)
            }
            CoreResponse::ProjectionArtifactRead { outcome } => {
                self.accept_artifact_outcome(outcome)?;
                Ok(None)
            }
            _ => Err(HeadlessConsumerError::InvalidResponse),
        }
    }

    /// Apply one live or replayed envelope. Duplicate sequence numbers are
    /// explicitly idempotent; gaps request resync instead of corrupting state.
    pub fn apply_event(&mut self, envelope: ProjectionEnvelope) -> HeadlessEventOutcome {
        let Some(mut cursor) = self.cursor.clone() else {
            return HeadlessEventOutcome::Error(ReducerError::ScopeMismatch {
                envelope_session: envelope.session_id,
                snapshot_session: "<unattached>".into(),
            });
        };
        let Some(descriptor) = self.descriptor.as_ref() else {
            return HeadlessEventOutcome::Error(ReducerError::ScopeMismatch {
                envelope_session: envelope.session_id,
                snapshot_session: "<no stream>".into(),
            });
        };
        if !Self::envelope_matches_descriptor(&envelope, descriptor) {
            self.last_resync_reason = Some(ProjectionResyncReason::StreamMismatch);
            self.connection_state = HeadlessConnectionState::ResyncRequired;
            return HeadlessEventOutcome::ResyncRequired {
                reason: ProjectionResyncReason::StreamMismatch,
                cursor,
            };
        }
        if envelope.event_seq <= cursor.event_seq {
            return HeadlessEventOutcome::Duplicate { cursor };
        }
        if envelope.event_seq != cursor.event_seq.saturating_add(1) {
            self.last_resync_reason = Some(ProjectionResyncReason::HistoryGap);
            self.connection_state = HeadlessConnectionState::ResyncRequired;
            return HeadlessEventOutcome::ResyncRequired {
                reason: ProjectionResyncReason::HistoryGap,
                cursor,
            };
        }
        if !Self::event_is_public(&envelope.payload, self.snapshot.as_ref()) {
            cursor.event_seq = envelope.event_seq;
            self.cursor = Some(cursor.clone());
            if let Some(snapshot) = self.snapshot.as_mut() {
                snapshot.event_seq = envelope.event_seq;
                snapshot.generated_at_ms = envelope.timestamp_ms;
            }
            return HeadlessEventOutcome::IgnoredNonPublic {
                event_seq: envelope.event_seq,
            };
        }
        let Some(snapshot) = self.snapshot.as_mut() else {
            return HeadlessEventOutcome::Error(ReducerError::ScopeMismatch {
                envelope_session: envelope.session_id,
                snapshot_session: "<no snapshot>".into(),
            });
        };
        match self.reducer.apply(snapshot, envelope.clone().into()) {
            ApplyOutcome::Applied => {
                cursor.event_seq = envelope.event_seq;
                self.cursor = Some(cursor.clone());
                HeadlessEventOutcome::Applied { cursor }
            }
            ApplyOutcome::Duplicate => HeadlessEventOutcome::Duplicate { cursor },
            ApplyOutcome::Reconciled => {
                cursor.event_seq = envelope.event_seq;
                self.cursor = Some(cursor.clone());
                HeadlessEventOutcome::Reconciled { cursor }
            }
            ApplyOutcome::ScopeMismatch => HeadlessEventOutcome::ResyncRequired {
                reason: ProjectionResyncReason::ScopeMismatch,
                cursor,
            },
            ApplyOutcome::ResyncRequired { .. } => {
                self.last_resync_reason = Some(ProjectionResyncReason::HistoryGap);
                self.connection_state = HeadlessConnectionState::ResyncRequired;
                HeadlessEventOutcome::ResyncRequired {
                    reason: ProjectionResyncReason::HistoryGap,
                    cursor,
                }
            }
            ApplyOutcome::Error(error) => HeadlessEventOutcome::Error(error),
        }
    }

    pub fn accept_replay(
        &mut self,
        batch: &ProjectionReplayBatch,
    ) -> Result<HeadlessReplayOutcome, HeadlessConsumerError> {
        if let Some(snapshot) = &batch.snapshot {
            let snapshot = Self::single_snapshot(snapshot.clone())?;
            let mut snapshot = *snapshot;
            Self::sanitize_snapshot(&mut snapshot);
            self.validate_stream(&batch.descriptor, &snapshot)?;
            self.snapshot = Some(snapshot);
            self.descriptor = Some(batch.descriptor.clone());
            self.session_id = self
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.primary_session_id.clone());
            self.cursor = Some(ProjectionCursor {
                stream_id: batch.descriptor.stream_id.clone(),
                event_seq: self
                    .snapshot
                    .as_ref()
                    .map_or(0, |snapshot| snapshot.event_seq),
                projection_version: batch.descriptor.projection_version,
            });
        }
        if self.descriptor.as_ref() != Some(&batch.descriptor) {
            return Err(HeadlessConsumerError::StreamMismatch);
        }
        if self.snapshot.is_none() || self.cursor.is_none() {
            return Err(HeadlessConsumerError::NotAttached);
        }
        let mut applied = 0;
        let mut duplicates = 0;
        let mut reconciled = 0;
        for event in &batch.events {
            match self.apply_event(event.clone()) {
                HeadlessEventOutcome::Applied { .. } => applied += 1,
                HeadlessEventOutcome::Duplicate { .. }
                | HeadlessEventOutcome::IgnoredNonPublic { .. } => duplicates += 1,
                HeadlessEventOutcome::Reconciled { .. } => reconciled += 1,
                HeadlessEventOutcome::ResyncRequired { reason, .. } => {
                    self.last_resync_reason = Some(reason);
                    return Ok(HeadlessReplayOutcome {
                        applied,
                        duplicates,
                        reconciled,
                        next_cursor: None,
                    });
                }
                HeadlessEventOutcome::Error(_) => {
                    return Err(HeadlessConsumerError::InvalidResponse)
                }
            }
        }
        self.connection_state = HeadlessConnectionState::Attached;
        Ok(HeadlessReplayOutcome {
            applied,
            duplicates,
            reconciled,
            next_cursor: batch.next_cursor.clone(),
        })
    }

    /// Request the bounded public artifact catalogue for the attached
    /// session's project.
    pub fn artifact_list_request(&self) -> Result<CoreRequest, HeadlessConsumerError> {
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or(HeadlessConsumerError::NotAttached)?;
        Ok(CoreRequest::ProjectionArtifactList {
            project_id: snapshot.project_id.clone(),
        })
    }

    /// Build an acknowledgement for the last accepted cursor. The caller
    /// decides when to send it; this keeps network I/O and retry policy out of
    /// the pure consumer.
    pub fn ack_request(&self) -> Result<CoreRequest, HeadlessConsumerError> {
        let subscription_id = self
            .subscription_id
            .clone()
            .ok_or(HeadlessConsumerError::NotAttached)?;
        let cursor = self
            .cursor
            .clone()
            .ok_or(HeadlessConsumerError::NotAttached)?;
        Ok(CoreRequest::ProjectionAck {
            ack: crate::projection::replay::ProjectionAck {
                subscription_id,
                cursor,
            },
        })
    }

    pub fn unsubscribe_request(&self) -> Result<CoreRequest, HeadlessConsumerError> {
        Ok(CoreRequest::ProjectionUnsubscribe {
            subscription_id: self
                .subscription_id
                .clone()
                .ok_or(HeadlessConsumerError::NotAttached)?,
        })
    }

    /// Build a normalized bounded artifact read request. The handle must have
    /// arrived through the authorized artifact list/fixture path.
    pub fn artifact_read_request(
        &self,
        handle_id: &str,
        start: u64,
        end: Option<u64>,
    ) -> Result<CoreRequest, HeadlessConsumerError> {
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or(HeadlessConsumerError::NotAttached)?;
        let handle = self
            .artifact_handles
            .iter()
            .find(|handle| handle.handle_id == handle_id)
            .ok_or(HeadlessConsumerError::UnsafeArtifactHandle)?;
        let end =
            end.unwrap_or(start.saturating_add(ProjectionArtifactReadRequest::MAX_READ_BYTES));
        if end < start {
            return Err(HeadlessConsumerError::InvalidArtifactRange);
        }
        Ok(CoreRequest::ProjectionArtifactRead {
            request: ProjectionArtifactReadRequest {
                handle_id: handle.handle_id.clone(),
                start,
                end: Some(
                    end.min(start.saturating_add(ProjectionArtifactReadRequest::MAX_READ_BYTES)),
                ),
                expected_revision: handle.revision,
            },
            project_id: snapshot.project_id.clone(),
            context_correlation_id: Some("headless-projection-consumer".into()),
        })
    }

    pub fn accept_artifact_handles(
        &mut self,
        handles: Vec<ProjectionArtifactHandleDto>,
    ) -> Result<(), HeadlessConsumerError> {
        let project_id = self
            .snapshot
            .as_ref()
            .ok_or(HeadlessConsumerError::NotAttached)?
            .project_id
            .clone();
        let mut seen = HashSet::new();
        let mut safe = Vec::new();
        for handle in handles {
            if handle.project_id != project_id {
                return Err(HeadlessConsumerError::ForeignArtifactHandle);
            }
            if !is_safe_opaque_id(&handle.handle_id)
                || handle.source_record_id.contains("..")
                || handle.source_record_id.contains('/')
                || handle.source_record_id.contains('\\')
                || !seen.insert(handle.handle_id.clone())
            {
                return Err(HeadlessConsumerError::UnsafeArtifactHandle);
            }
            safe.push(handle);
            if safe.len() == crate::projection::limits::MAX_PROJECTION_ARTIFACTS {
                break;
            }
        }
        self.artifact_handles = safe;
        Ok(())
    }

    pub fn accept_artifact_outcome(
        &mut self,
        outcome: &ProjectionArtifactReadOutcome,
    ) -> Result<(), HeadlessConsumerError> {
        let ProjectionArtifactReadOutcome::Ok(response) = outcome else {
            self.last_artifact = None;
            return Ok(());
        };
        if !self
            .artifact_handles
            .iter()
            .any(|handle| handle.handle_id == response.handle_id)
        {
            return Err(HeadlessConsumerError::ArtifactHandleMismatch);
        }
        if response.end < response.start
            || response.end.saturating_sub(response.start)
                > ProjectionArtifactReadRequest::MAX_READ_BYTES
            || response.content.len() > ProjectionArtifactReadRequest::MAX_READ_BYTES as usize
        {
            return Err(HeadlessConsumerError::ArtifactTooLarge);
        }
        self.last_artifact = Some(response.clone());
        Ok(())
    }

    /// A session is terminal only when the daemon's session status says so;
    /// an absent active turn alone is not treated as terminal.
    pub fn session_is_terminal(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|snapshot| is_terminal_status(&snapshot.primary_session.status))
    }

    pub fn run_is_terminal(&self, run_id: &str) -> Option<bool> {
        self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .runs
                .iter()
                .find(|run| run.run_id == run_id)
                .map(|run| is_terminal_status(&run.status))
        })
    }

    fn single_snapshot(
        bundle: ProjectionSnapshotBundle,
    ) -> Result<Box<SessionProjectionSnapshot>, HeadlessConsumerError> {
        match bundle {
            ProjectionSnapshotBundle::One { snapshot } => Ok(snapshot),
            ProjectionSnapshotBundle::BoundedSessionList { .. } => {
                Err(HeadlessConsumerError::InvalidSnapshotBundle)
            }
        }
    }

    fn validate_stream(
        &self,
        descriptor: &ProjectionStreamDescriptor,
        snapshot: &SessionProjectionSnapshot,
    ) -> Result<(), HeadlessConsumerError> {
        if descriptor.kind != crate::projection::replay::ProjectionStreamKind::Session
            || descriptor.session_id.as_deref() != Some(snapshot.primary_session_id.as_str())
            || descriptor.project_id != snapshot.project_id
            || descriptor.projection_version
                != self
                    .negotiated_version
                    .unwrap_or(descriptor.projection_version)
        {
            return Err(HeadlessConsumerError::StreamMismatch);
        }
        Ok(())
    }

    fn envelope_matches_descriptor(
        envelope: &ProjectionEnvelope,
        descriptor: &ProjectionStreamDescriptor,
    ) -> bool {
        envelope.scope == ProjectionStreamScope::Session
            && envelope.session_id.as_deref() == descriptor.session_id.as_deref()
            && envelope.protocol_version == descriptor.projection_version
    }

    fn event_is_public(
        event: &ProjectionEvent,
        snapshot: Option<&SessionProjectionSnapshot>,
    ) -> bool {
        match event {
            ProjectionEvent::ReasoningAppended { .. } | ProjectionEvent::Unknown { .. } => false,
            ProjectionEvent::MessageAppended { message } => {
                message.visibility == VisibilityClass::Public
            }
            ProjectionEvent::ToolStarted { tool } => tool.visibility == VisibilityClass::Public,
            ProjectionEvent::ToolCompleted { tool_id, .. }
            | ProjectionEvent::ToolFailed { tool_id, .. } => snapshot
                .and_then(|snapshot| snapshot.active_turn.as_ref())
                .and_then(|turn| turn.tools.iter().find(|tool| tool.tool_id == *tool_id))
                .is_some_and(|tool| tool.visibility == VisibilityClass::Public),
            ProjectionEvent::Diagnostic { .. } => false,
            _ => true,
        }
    }

    fn sanitize_snapshot(snapshot: &mut SessionProjectionSnapshot) {
        let sanitize_turn = |turn: &mut crate::projection::dto::TurnProjection| {
            turn.messages
                .retain(|message| message.visibility == VisibilityClass::Public);
            turn.tools
                .retain(|tool| tool.visibility == VisibilityClass::Public);
            for tool in &mut turn.tools {
                if let ToolOutputProjection::Inline { output } = &mut tool.output {
                    if output.len() > crate::projection::limits::MAX_PROJECTION_STRING_BYTES {
                        *output = crate::projection::limits::truncate_str(
                            output,
                            crate::projection::limits::MAX_PROJECTION_STRING_BYTES,
                        )
                        .into_owned();
                    }
                }
            }
        };
        if let Some(turn) = snapshot.active_turn.as_mut() {
            sanitize_turn(turn);
        }
        for turn in &mut snapshot.recent_turns {
            sanitize_turn(turn);
        }
        snapshot.diagnostics.clear();
    }

    fn push_diagnostic(&mut self, diagnostic: &str) {
        if self.diagnostics.len() >= MAX_HEADLESS_DIAGNOSTICS {
            self.diagnostics.pop_front();
        }
        self.diagnostics.push_back(diagnostic.to_string());
    }
}

fn is_safe_opaque_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= crate::projection::replay::MAX_STREAM_ID_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn is_terminal_status(status: &str) -> bool {
    matches!(
        status,
        "completed" | "failed" | "cancelled" | "canceled" | "denied"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::fixtures::{active_turn_event_script, idle_snapshot};
    use crate::projection::replay::{
        ProjectionArtifactHandleDto, ProjectionStreamId, ProjectionStreamKind,
    };

    fn descriptor() -> ProjectionStreamDescriptor {
        ProjectionStreamDescriptor {
            stream_id: ProjectionStreamId::new("session-session-fixture").unwrap(),
            kind: ProjectionStreamKind::Session,
            project_id: "project-fixture".into(),
            workspace_id: Some("workspace-fixture".into()),
            session_id: Some("session-fixture".into()),
            projection_version: PROJECTION_PROTOCOL_VERSION,
            retention_floor_seq: 0,
            high_water_seq: 0,
            latest_checkpoint_seq: None,
        }
    }

    fn consumer_with_snapshot() -> HeadlessProjectionConsumer {
        let mut consumer = HeadlessProjectionConsumer::new();
        consumer
            .connect(&ProjectionCapabilities::current())
            .unwrap();
        consumer
            .accept_subscribed(
                ProjectionSubscriptionId::new("sub-1"),
                descriptor(),
                ProjectionSnapshotBundle::One {
                    snapshot: Box::new(idle_snapshot()),
                },
                ProjectionCursor {
                    stream_id: descriptor().stream_id,
                    event_seq: 0,
                    projection_version: PROJECTION_PROTOCOL_VERSION,
                },
            )
            .unwrap();
        consumer
    }

    #[test]
    fn capabilities_and_attach_are_transport_neutral() {
        let mut consumer = HeadlessProjectionConsumer::new();
        assert_eq!(consumer.connect(&ProjectionCapabilities::current()), Ok(1));
        let request = consumer.attach_core_request("session-fixture").unwrap();
        assert!(matches!(request, CoreRequest::ProjectionSubscribe { .. }));
    }

    #[test]
    fn live_event_and_duplicate_are_idempotent() {
        let mut consumer = consumer_with_snapshot();
        let event = ProjectionEnvelope::session_event(
            1,
            1,
            "session-fixture",
            Some("turn-1".into()),
            active_turn_event_script()
                .into_iter()
                .next()
                .unwrap()
                .payload,
        );
        assert!(matches!(
            consumer.apply_event(event.clone()),
            HeadlessEventOutcome::Applied { .. }
        ));
        assert!(matches!(
            consumer.apply_event(event),
            HeadlessEventOutcome::Duplicate { .. }
        ));
        assert_eq!(consumer.cursor().unwrap().event_seq, 1);
    }

    #[test]
    fn gap_requests_resync() {
        let mut consumer = consumer_with_snapshot();
        let event = ProjectionEnvelope::session_event(
            2,
            2,
            "session-fixture",
            None,
            crate::projection::event::ProjectionEvent::Diagnostic {
                code: "hidden".into(),
                message: "ignored".into(),
            },
        );
        assert!(matches!(
            consumer.apply_event(event),
            HeadlessEventOutcome::ResyncRequired {
                reason: ProjectionResyncReason::HistoryGap,
                ..
            }
        ));
        assert_eq!(
            consumer.connection_state(),
            HeadlessConnectionState::ResyncRequired
        );
    }

    #[test]
    fn non_public_event_advances_cursor_without_exposing_content() {
        let mut consumer = consumer_with_snapshot();
        let event = ProjectionEnvelope::session_event(
            1,
            1,
            "session-fixture",
            Some("turn-1".into()),
            ProjectionEvent::ReasoningAppended {
                message_id: "reasoning-1".into(),
                delta: "secret reasoning".into(),
            },
        );
        assert!(matches!(
            consumer.apply_event(event),
            HeadlessEventOutcome::IgnoredNonPublic { event_seq: 1 }
        ));
        assert!(consumer.snapshot().unwrap().active_turn.as_ref().is_none());
    }

    #[test]
    fn artifact_handles_are_project_scoped_and_bounded() {
        let mut consumer = consumer_with_snapshot();
        consumer
            .accept_artifact_handles(vec![ProjectionArtifactHandleDto {
                handle_id: "art-1".into(),
                kind: crate::projection::replay::ArtifactHandleKind::RunOutput,
                project_id: "project-fixture".into(),
                source_record_id: "run-1".into(),
                content_type: "text/plain".into(),
                total_bytes: Some(3),
                created_at: 0,
                expires_at: None,
                revision: 1,
                public_summary: Some("ok".into()),
            }])
            .unwrap();
        let request = consumer.artifact_read_request("art-1", 0, Some(3)).unwrap();
        assert!(matches!(
            request,
            CoreRequest::ProjectionArtifactRead { .. }
        ));
        assert!(!consumer.session_is_terminal());
    }
}
