//! Frontend projection client controller.
//!
//! The [`ProjectionClientController`] is a transport-neutral state machine
//! that every frontend (local TUI, remote TUI, future observer / ACP /
//! web clients) uses to:
//!
//! 1. negotiate projection capabilities with the daemon;
//! 2. select a [`ProjectionMode`] (`ProjectionPrimary`, bounded
//!    `RawCompatibility`, or `Unsupported`);
//! 3. subscribe to scoped projection streams;
//! 4. atomically install an initial snapshot;
//! 5. apply ordered projection events through the canonical
//!    [`crate::projection::reducer::ProjectionReducer`];
//! 6. acknowledge cursors with bounded cadence;
//! 7. handle resync / restart / version mismatch / capability change;
//! 8. unsubscribe cleanly on scope removal;
//! 9. expose immutable, bounded view models to frontends.
//!
//! The controller never performs daemon storage or execution work; it
//! only consumes the existing projection replay protocol surface and
//! produces deterministic local state.
//!
//! ## Invariants
//!
//! - The canonical reducer is the only reducer. The controller does not
//!   fork projection semantics.
//! - Subscriptions are stream-scoped; a cursor for one stream cannot be
//!   used against another.
//! - Resync replaces state atomically — partial merges across streams
//!   are not permitted.
//! - Reconnect must renegotiate; a changed capability set invalidates
//!   subscriptions.
//!
//! ## Modes
//!
//! - [`ProjectionMode::ProjectionPrimary`] is selected when the daemon
//!   advertises a compatible projection version.
//! - [`ProjectionMode::RawCompatibility`] is a bounded fallback that
//!   keeps the existing raw-core client path active for older peers.
//! - [`ProjectionMode::Unsupported`] is the explicit diagnostic state
//!   when no compatible mode is available.

use serde::{Deserialize, Serialize};

use crate::projection::caps::{
    ProjectionCapabilities, PROJECTION_PROTOCOL_VERSION, PROJECTION_PROTOCOL_VERSION_MIN,
};
use crate::projection::event::ProjectionEnvelope;
use crate::projection::reducer::{
    ApplyOutcome, ProjectionReducer, ReducerError, ReducerEventInput,
};
use crate::projection::replay::{
    ProjectionAck, ProjectionCursor, ProjectionReplayLimits, ProjectionResyncReason,
    ProjectionSnapshotBundle, ProjectionStreamDescriptor, ProjectionStreamId,
    ProjectionSubscriptionId, ProjectionSubscriptionRequest, ProjectionSubscriptionState,
    ProjectionSubscriptionStatus,
};
use crate::projection::snapshot::SessionProjectionSnapshot;

/// Maximum number of subscriptions a single controller can hold.
pub const MAX_CONTROLLER_SUBSCRIPTIONS: usize = 16;

/// Maximum number of un-acked deliveries before the controller requests
/// a resync to bound outstanding lag.
pub const MAX_OUTSTANDING_LAG: u64 = 1024;

/// Default cadence for acknowledgement of applied cursors.
pub const DEFAULT_ACK_CADENCE: u64 = 16;

/// Maximum number of in-memory diagnostics retained on the controller.
pub const MAX_CONTROLLER_DIAGNOSTICS: usize = 32;

/// Diagnostic surfaced by the controller to frontends.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerDiagnostic {
    pub code: String,
    pub message: String,
}

/// Operating mode the controller settled into after negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ProjectionMode {
    /// The controller consumes the canonical projection stream and
    /// applies events through the canonical reducer.
    ProjectionPrimary,
    /// The controller kept the existing raw-core fallback because the
    /// peer cannot speak the projection contract.
    RawCompatibility,
    /// The controller could not establish a compatible mode; the
    /// reason is surfaced through the [`ControllerDiagnostic`] list.
    Unsupported,
}

impl ProjectionMode {
    pub fn is_projection_primary(&self) -> bool {
        matches!(self, ProjectionMode::ProjectionPrimary)
    }

    pub fn is_raw_compatibility(&self) -> bool {
        matches!(self, ProjectionMode::RawCompatibility)
    }

    pub fn is_unsupported(&self) -> bool {
        matches!(self, ProjectionMode::Unsupported)
    }
}

/// Snapshot of controller-level metadata that frontends can render.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionControllerInfo {
    pub mode: ProjectionMode,
    pub negotiated_version: Option<u32>,
    pub reconnect_epoch: u64,
    pub active_subscriptions: usize,
    pub pending_ack_count: u64,
    pub last_resync_reason: Option<ProjectionResyncReason>,
    pub fallback_reason: Option<String>,
}

/// Result of a single reducer apply call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerApplyOutcome {
    Applied {
        last_seq: u64,
    },
    Duplicate {
        last_seq: u64,
    },
    ScopeMismatch {
        envelope_session: Option<String>,
        snapshot_session: String,
    },
    Reconciled {
        last_seq: u64,
        diagnostic: ControllerDiagnostic,
    },
    ResyncRequested {
        reason: ProjectionResyncReason,
        last_seq: u64,
        envelope_seq: u64,
    },
    Error(ReducerError),
}

impl ControllerApplyOutcome {
    pub fn last_seq(&self) -> Option<u64> {
        match self {
            ControllerApplyOutcome::Applied { last_seq }
            | ControllerApplyOutcome::Duplicate { last_seq }
            | ControllerApplyOutcome::Reconciled { last_seq, .. }
            | ControllerApplyOutcome::ResyncRequested { last_seq, .. } => Some(*last_seq),
            ControllerApplyOutcome::ScopeMismatch { .. } | ControllerApplyOutcome::Error(_) => None,
        }
    }
}

/// Outcome of a subscription request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerSubscribeOutcome {
    Subscribed {
        subscription_id: ProjectionSubscriptionId,
        descriptor: ProjectionStreamDescriptor,
        snapshot: SessionProjectionSnapshot,
    },
    ResyncRequired {
        reason: ProjectionResyncReason,
        snapshot: SessionProjectionSnapshot,
    },
    Replay {
        subscription_id: ProjectionSubscriptionId,
        descriptor: ProjectionStreamDescriptor,
        batch: Vec<ProjectionEnvelope>,
        snapshot: Option<ProjectionSnapshotBundle>,
    },
    Failed {
        reason: ControllerSubscribeFailure,
    },
}

/// Bounded subscribe failure reasons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerSubscribeFailure {
    UnsupportedMode,
    InvalidRequest,
    SubscriptionLimitExceeded,
    VersionMismatch,
    StreamMismatch,
    NotFound,
}

impl std::fmt::Display for ControllerSubscribeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ControllerSubscribeFailure::UnsupportedMode => write!(f, "unsupported projection mode"),
            ControllerSubscribeFailure::InvalidRequest => write!(f, "invalid subscription request"),
            ControllerSubscribeFailure::SubscriptionLimitExceeded => {
                write!(f, "subscription limit exceeded")
            }
            ControllerSubscribeFailure::VersionMismatch => write!(f, "version mismatch"),
            ControllerSubscribeFailure::StreamMismatch => write!(f, "stream mismatch"),
            ControllerSubscribeFailure::NotFound => write!(f, "stream not found"),
        }
    }
}

/// A bounded subscription record held by the controller.
#[derive(Debug, Clone)]
struct SubscriptionRecord {
    subscription_id: ProjectionSubscriptionId,
    descriptor: ProjectionStreamDescriptor,
    state: ProjectionSubscriptionState,
    last_delivered_seq: u64,
    last_acked_seq: u64,
    pending_ack: u64,
}

/// Transport-neutral projection client controller.
pub struct ProjectionClientController {
    capabilities: ProjectionCapabilities,
    negotiated_version: Option<u32>,
    mode: ProjectionMode,
    fallback_reason: Option<String>,
    reducer: ProjectionReducer,
    subscriptions: Vec<SubscriptionRecord>,
    snapshots: std::collections::HashMap<ProjectionStreamId, SessionProjectionSnapshot>,
    diagnostics: Vec<ControllerDiagnostic>,
    last_resync_reason: Option<ProjectionResyncReason>,
    reconnect_epoch: u64,
    ack_cadence: u64,
}

impl ProjectionClientController {
    /// Build a controller that defaults to [`ProjectionMode::Unsupported`].
    pub fn new(capabilities: ProjectionCapabilities) -> Self {
        Self {
            capabilities,
            negotiated_version: None,
            mode: ProjectionMode::Unsupported,
            fallback_reason: None,
            reducer: ProjectionReducer::default(),
            subscriptions: Vec::new(),
            snapshots: std::collections::HashMap::new(),
            diagnostics: Vec::new(),
            last_resync_reason: None,
            reconnect_epoch: 0,
            ack_cadence: DEFAULT_ACK_CADENCE,
        }
    }

    pub fn negotiated_version(&self) -> Option<u32> {
        self.negotiated_version
    }

    pub fn mode(&self) -> ProjectionMode {
        self.mode
    }

    pub fn reconnect_epoch(&self) -> u64 {
        self.reconnect_epoch
    }

    pub fn last_resync_reason(&self) -> Option<ProjectionResyncReason> {
        self.last_resync_reason
    }

    pub fn capabilities(&self) -> &ProjectionCapabilities {
        &self.capabilities
    }

    pub fn diagnostics(&self) -> &[ControllerDiagnostic] {
        &self.diagnostics
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    pub fn ack_cadence(&self) -> u64 {
        self.ack_cadence
    }

    pub fn set_ack_cadence(&mut self, cadence: u64) {
        self.ack_cadence = cadence.max(1);
    }

    pub fn snapshot_for(
        &self,
        stream_id: &ProjectionStreamId,
    ) -> Option<&SessionProjectionSnapshot> {
        self.snapshots.get(stream_id)
    }

    /// Borrow all currently held snapshots. Used by reference-client
    /// tests that need to compare controller state to a direct reducer
    /// consumer.
    pub fn snapshots(
        &self,
    ) -> &std::collections::HashMap<ProjectionStreamId, SessionProjectionSnapshot> {
        &self.snapshots
    }

    pub fn subscription(
        &self,
        id: &ProjectionSubscriptionId,
    ) -> Option<ProjectionSubscriptionStatus> {
        self.subscriptions
            .iter()
            .find(|s| &s.subscription_id == id)
            .map(|s| ProjectionSubscriptionStatus {
                id: s.subscription_id.clone(),
                scope: s.descriptor.kind,
                last_delivered_seq: s.last_delivered_seq,
                last_acked_seq: s.last_acked_seq,
                state: s.state,
                lag_count: s.pending_ack,
            })
    }

    /// Negotiate capabilities with the daemon side.
    ///
    /// When `daemon` is `None`, the controller stays in
    /// [`ProjectionMode::Unsupported`] and records a fallback reason.
    pub fn negotiate(&mut self, daemon: Option<&ProjectionCapabilities>) {
        let Some(daemon) = daemon else {
            self.negotiated_version = None;
            self.mode = ProjectionMode::Unsupported;
            self.fallback_reason = Some("no projection capabilities advertised".to_string());
            self.push_diagnostic("unsupported", "daemon capabilities absent");
            return;
        };
        match ProjectionCapabilities::negotiate(&self.capabilities, daemon) {
            Some(version)
                if (PROJECTION_PROTOCOL_VERSION_MIN..=PROJECTION_PROTOCOL_VERSION)
                    .contains(&version) =>
            {
                self.negotiated_version = Some(version);
                self.mode = ProjectionMode::ProjectionPrimary;
                self.fallback_reason = None;
            }
            Some(version) => {
                self.negotiated_version = Some(version);
                self.mode = ProjectionMode::Unsupported;
                self.fallback_reason = Some(format!(
                    "negotiated version {version} outside supported range"
                ));
                self.push_diagnostic(
                    "version_mismatch",
                    &format!("negotiated version {version} outside supported range"),
                );
            }
            None => {
                self.negotiated_version = None;
                self.mode = ProjectionMode::Unsupported;
                self.fallback_reason = Some("no overlapping projection version".to_string());
                self.push_diagnostic(
                    "version_mismatch",
                    "no overlapping projection version between client and daemon",
                );
            }
        }
    }

    /// Select [`ProjectionMode::RawCompatibility`] explicitly.
    pub fn enter_raw_compatibility(&mut self, reason: impl Into<String>) {
        self.mode = ProjectionMode::RawCompatibility;
        self.fallback_reason = Some(reason.into());
        self.subscriptions.clear();
        self.snapshots.clear();
        self.negotiated_version = None;
    }

    /// Bump the reconnect epoch and drop all subscriptions. The caller
    /// is expected to re-negotiate and re-subscribe.
    pub fn on_reconnect(&mut self) {
        self.reconnect_epoch = self.reconnect_epoch.saturating_add(1);
        self.subscriptions.clear();
        self.snapshots.clear();
        self.diagnostics.clear();
        self.last_resync_reason = None;
        self.negotiated_version = None;
        self.mode = ProjectionMode::Unsupported;
        self.fallback_reason = Some("reconnect pending renegotiation".to_string());
    }

    /// Validate a subscription request against the negotiated mode and
    /// version. Returns `Err` if the controller cannot subscribe.
    pub fn validate_subscription(
        &self,
        request: &ProjectionSubscriptionRequest,
    ) -> Result<(), ControllerSubscribeFailure> {
        if !self.mode.is_projection_primary() {
            return Err(ControllerSubscribeFailure::UnsupportedMode);
        }
        request
            .validate()
            .map_err(|_| ControllerSubscribeFailure::InvalidRequest)?;
        let version = self
            .negotiated_version
            .unwrap_or(PROJECTION_PROTOCOL_VERSION);
        if request.projection_version != version {
            return Err(ControllerSubscribeFailure::VersionMismatch);
        }
        if self.subscriptions.len() >= MAX_CONTROLLER_SUBSCRIPTIONS {
            return Err(ControllerSubscribeFailure::SubscriptionLimitExceeded);
        }
        Ok(())
    }

    /// Install a subscription after the daemon responds. Records the
    /// descriptor and installs the initial snapshot atomically.
    pub fn install_subscription(
        &mut self,
        subscription_id: ProjectionSubscriptionId,
        descriptor: ProjectionStreamDescriptor,
        snapshot: SessionProjectionSnapshot,
    ) -> ControllerSubscribeOutcome {
        if !self.mode.is_projection_primary() {
            return ControllerSubscribeOutcome::Failed {
                reason: ControllerSubscribeFailure::UnsupportedMode,
            };
        }
        if self.subscriptions.len() >= MAX_CONTROLLER_SUBSCRIPTIONS {
            self.push_diagnostic("subscription_limit_exceeded", descriptor.stream_id.as_str());
            return ControllerSubscribeOutcome::Failed {
                reason: ControllerSubscribeFailure::SubscriptionLimitExceeded,
            };
        }
        if self
            .subscriptions
            .iter()
            .any(|s| s.descriptor.stream_id == descriptor.stream_id)
        {
            self.push_diagnostic("duplicate_subscription", descriptor.stream_id.as_str());
            return ControllerSubscribeOutcome::Failed {
                reason: ControllerSubscribeFailure::StreamMismatch,
            };
        }
        let last_seq = snapshot.event_seq;
        let record = SubscriptionRecord {
            subscription_id: subscription_id.clone(),
            descriptor: descriptor.clone(),
            state: ProjectionSubscriptionState::Live,
            last_delivered_seq: last_seq,
            last_acked_seq: last_seq,
            pending_ack: 0,
        };
        self.subscriptions.push(record);
        self.snapshots
            .insert(descriptor.stream_id.clone(), snapshot.clone());
        ControllerSubscribeOutcome::Subscribed {
            subscription_id,
            descriptor,
            snapshot,
        }
    }

    /// Install a replay batch on a subscription.
    pub fn install_replay(
        &mut self,
        subscription_id: ProjectionSubscriptionId,
        descriptor: ProjectionStreamDescriptor,
        events: Vec<ProjectionEnvelope>,
        snapshot: Option<ProjectionSnapshotBundle>,
        initial: SessionProjectionSnapshot,
    ) -> ControllerSubscribeOutcome {
        if !self.mode.is_projection_primary() {
            return ControllerSubscribeOutcome::Failed {
                reason: ControllerSubscribeFailure::UnsupportedMode,
            };
        }
        let stream_id = descriptor.stream_id.clone();
        let mut current = initial;
        let reducer = self.reducer.clone();
        for envelope in &events {
            let input = ReducerEventInput::from(envelope.clone());
            let _ = reducer.apply(&mut current, input);
        }
        if let Some(bundle) = snapshot {
            match bundle {
                ProjectionSnapshotBundle::One { snapshot } => {
                    current = *snapshot;
                }
                ProjectionSnapshotBundle::BoundedSessionList { .. } => {
                    self.push_diagnostic(
                        "replay_list_bundle_ignored",
                        "BoundedSessionList ignored in install_replay",
                    );
                }
            }
        }
        let last_seq = current.event_seq;
        let record = SubscriptionRecord {
            subscription_id: subscription_id.clone(),
            descriptor: descriptor.clone(),
            state: ProjectionSubscriptionState::Live,
            last_delivered_seq: last_seq,
            last_acked_seq: last_seq,
            pending_ack: 0,
        };
        self.subscriptions
            .retain(|s| s.subscription_id != subscription_id);
        self.subscriptions.push(record);
        self.snapshots.insert(stream_id, current);
        ControllerSubscribeOutcome::Replay {
            subscription_id,
            descriptor,
            batch: events,
            snapshot: None,
        }
    }

    /// Apply an envelope to the subscription's snapshot. The controller
    /// rejects events whose stream id does not match the subscription
    /// and events whose protocol version is outside the negotiated range.
    pub fn apply_envelope(
        &mut self,
        subscription_id: &ProjectionSubscriptionId,
        envelope: ProjectionEnvelope,
    ) -> ControllerApplyOutcome {
        if !self.mode.is_projection_primary() {
            return ControllerApplyOutcome::Error(ReducerError::UnsupportedProtocolVersion {
                envelope_version: envelope.protocol_version,
                supported_min: PROJECTION_PROTOCOL_VERSION_MIN,
                supported_max: PROJECTION_PROTOCOL_VERSION,
            });
        }
        let Some(record) = self
            .subscriptions
            .iter_mut()
            .find(|s| &s.subscription_id == subscription_id)
        else {
            self.push_diagnostic("unknown_subscription", subscription_id.as_str());
            return ControllerApplyOutcome::Error(ReducerError::ScopeMismatch {
                envelope_session: envelope.session_id.clone(),
                snapshot_session: "<unknown subscription>".into(),
            });
        };
        if envelope.protocol_version != self.negotiated_version.unwrap_or(envelope.protocol_version)
        {
            let last_seq = record.last_delivered_seq;
            let envelope_seq = envelope.event_seq;
            let _ = record;
            self.last_resync_reason = Some(ProjectionResyncReason::VersionMismatch);
            return ControllerApplyOutcome::ResyncRequested {
                reason: ProjectionResyncReason::VersionMismatch,
                last_seq,
                envelope_seq,
            };
        }
        if record.descriptor.stream_id != envelope_to_stream_id(&envelope) {
            let last_seq = record.last_delivered_seq;
            let envelope_seq = envelope.event_seq;
            let _ = record;
            self.last_resync_reason = Some(ProjectionResyncReason::StreamMismatch);
            return ControllerApplyOutcome::ResyncRequested {
                reason: ProjectionResyncReason::StreamMismatch,
                last_seq,
                envelope_seq,
            };
        }
        if envelope.event_seq <= record.last_delivered_seq {
            return ControllerApplyOutcome::Duplicate {
                last_seq: record.last_delivered_seq,
            };
        }
        let snapshot = match self.snapshots.get_mut(&record.descriptor.stream_id) {
            Some(snapshot) => snapshot,
            None => {
                self.last_resync_reason = Some(ProjectionResyncReason::SnapshotUnavailable);
                return ControllerApplyOutcome::ResyncRequested {
                    reason: ProjectionResyncReason::SnapshotUnavailable,
                    last_seq: record.last_delivered_seq,
                    envelope_seq: envelope.event_seq,
                };
            }
        };
        let input = ReducerEventInput::from(envelope.clone());
        let reducer_outcome = self.reducer.apply(snapshot, input);
        match reducer_outcome {
            ApplyOutcome::Applied => {
                let last_seq = envelope.event_seq;
                record.last_delivered_seq = last_seq;
                record.pending_ack = record.pending_ack.saturating_add(1);
                ControllerApplyOutcome::Applied { last_seq }
            }
            ApplyOutcome::Duplicate => ControllerApplyOutcome::Duplicate {
                last_seq: record.last_delivered_seq,
            },
            ApplyOutcome::ScopeMismatch => ControllerApplyOutcome::ScopeMismatch {
                envelope_session: envelope.session_id.clone(),
                snapshot_session: snapshot.primary_session_id.clone(),
            },
            ApplyOutcome::Reconciled => {
                let last_seq = envelope.event_seq;
                let code = "reconciled".to_string();
                let message = format!("event_seq {last_seq}");
                let diagnostic = ControllerDiagnostic { code, message };
                record.last_delivered_seq = last_seq;
                record.pending_ack = record.pending_ack.saturating_add(1);
                let _ = record;
                self.push_diagnostic(&diagnostic.code, &diagnostic.message);
                ControllerApplyOutcome::Reconciled {
                    last_seq,
                    diagnostic,
                }
            }
            ApplyOutcome::ResyncRequired {
                from_event_seq,
                current_seq,
            } => {
                record.state = ProjectionSubscriptionState::ResyncRequired;
                let _ = record;
                self.last_resync_reason = Some(ProjectionResyncReason::HistoryGap);
                ControllerApplyOutcome::ResyncRequested {
                    reason: ProjectionResyncReason::HistoryGap,
                    last_seq: current_seq,
                    envelope_seq: from_event_seq,
                }
            }
            ApplyOutcome::Error(err) => {
                let last_seq = record.last_delivered_seq;
                let envelope_seq = envelope.event_seq;
                let _ = record;
                if matches!(err, ReducerError::UnsupportedProtocolVersion { .. }) {
                    self.last_resync_reason = Some(ProjectionResyncReason::VersionMismatch);
                    ControllerApplyOutcome::ResyncRequested {
                        reason: ProjectionResyncReason::VersionMismatch,
                        last_seq,
                        envelope_seq,
                    }
                } else {
                    ControllerApplyOutcome::Error(err)
                }
            }
        }
    }

    /// Mark a resync requested by the caller and demote the
    /// subscription state. Existing state is retained until the new
    /// snapshot is installed.
    pub fn request_resync(
        &mut self,
        subscription_id: &ProjectionSubscriptionId,
        reason: ProjectionResyncReason,
    ) {
        self.last_resync_reason = Some(reason);
        if let Some(record) = self
            .subscriptions
            .iter_mut()
            .find(|s| &s.subscription_id == subscription_id)
        {
            record.state = ProjectionSubscriptionState::ResyncRequired;
        }
    }

    /// Acknowledge applied events on a subscription. Returns the new
    /// status when the controller decides to send the ack upstream.
    pub fn try_ack(&mut self, subscription_id: &ProjectionSubscriptionId) -> Option<ProjectionAck> {
        let record = self
            .subscriptions
            .iter_mut()
            .find(|s| &s.subscription_id == subscription_id)?;
        if record.pending_ack < self.ack_cadence {
            return None;
        }
        record.last_acked_seq = record.last_delivered_seq;
        let pending = std::mem::take(&mut record.pending_ack);
        record.pending_ack = 0;
        let _ = pending;
        Some(ProjectionAck {
            subscription_id: subscription_id.clone(),
            cursor: ProjectionCursor {
                stream_id: record.descriptor.stream_id.clone(),
                event_seq: record.last_acked_seq,
                projection_version: record.descriptor.projection_version,
            },
        })
    }

    /// Force an acknowledgement with the current delivered cursor.
    pub fn force_ack(
        &mut self,
        subscription_id: &ProjectionSubscriptionId,
    ) -> Option<ProjectionAck> {
        let record = self
            .subscriptions
            .iter_mut()
            .find(|s| &s.subscription_id == subscription_id)?;
        record.last_acked_seq = record.last_delivered_seq;
        record.pending_ack = 0;
        Some(ProjectionAck {
            subscription_id: subscription_id.clone(),
            cursor: ProjectionCursor {
                stream_id: record.descriptor.stream_id.clone(),
                event_seq: record.last_acked_seq,
                projection_version: record.descriptor.projection_version,
            },
        })
    }

    /// Remove a subscription and its snapshot.
    pub fn unsubscribe(
        &mut self,
        subscription_id: &ProjectionSubscriptionId,
    ) -> Option<ProjectionSubscriptionStatus> {
        let idx = self
            .subscriptions
            .iter()
            .position(|s| &s.subscription_id == subscription_id)?;
        let record = self.subscriptions.remove(idx);
        self.snapshots.remove(&record.descriptor.stream_id);
        Some(ProjectionSubscriptionStatus {
            id: record.subscription_id,
            scope: record.descriptor.kind,
            last_delivered_seq: record.last_delivered_seq,
            last_acked_seq: record.last_acked_seq,
            state: ProjectionSubscriptionState::Closed,
            lag_count: 0,
        })
    }

    /// Snapshot of controller info, suitable for render and metrics.
    pub fn info(&self) -> ProjectionControllerInfo {
        let pending = self
            .subscriptions
            .iter()
            .map(|s| s.pending_ack)
            .sum::<u64>();
        ProjectionControllerInfo {
            mode: self.mode,
            negotiated_version: self.negotiated_version,
            reconnect_epoch: self.reconnect_epoch,
            active_subscriptions: self.subscriptions.len(),
            pending_ack_count: pending,
            last_resync_reason: self.last_resync_reason,
            fallback_reason: self.fallback_reason.clone(),
        }
    }

    /// Default replay limits the controller will respect.
    pub fn replay_limits(&self) -> ProjectionReplayLimits {
        ProjectionReplayLimits::default()
    }

    /// Record a diagnostic and bound the retained list.
    fn push_diagnostic(&mut self, code: &str, message: &str) {
        self.diagnostics.push(ControllerDiagnostic {
            code: code.to_string(),
            message: message.to_string(),
        });
        if self.diagnostics.len() > MAX_CONTROLLER_DIAGNOSTICS {
            let drop = self.diagnostics.len() - MAX_CONTROLLER_DIAGNOSTICS;
            self.diagnostics.drain(0..drop);
        }
    }
}

impl Clone for ProjectionClientController {
    fn clone(&self) -> Self {
        Self {
            capabilities: self.capabilities.clone(),
            negotiated_version: self.negotiated_version,
            mode: self.mode,
            fallback_reason: self.fallback_reason.clone(),
            reducer: ProjectionReducer::default(),
            subscriptions: self.subscriptions.clone(),
            snapshots: self.snapshots.clone(),
            diagnostics: self.diagnostics.clone(),
            last_resync_reason: self.last_resync_reason,
            reconnect_epoch: self.reconnect_epoch,
            ack_cadence: self.ack_cadence,
        }
    }
}

fn envelope_to_stream_id(envelope: &ProjectionEnvelope) -> ProjectionStreamId {
    use crate::projection::event::ProjectionStreamScope;
    let scope_session = envelope
        .session_id
        .clone()
        .unwrap_or_default()
        .replace(':', "-");
    let raw = match envelope.scope {
        ProjectionStreamScope::Session => format!("session-{scope_session}"),
        ProjectionStreamScope::Project => format!("project-{scope_session}"),
        ProjectionStreamScope::Workspace => format!("workspace-{scope_session}"),
        ProjectionStreamScope::Daemon => "daemon".to_string(),
    };
    ProjectionStreamId::new(raw).unwrap_or_else(|_| ProjectionStreamId("invalid".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::caps::ProjectionCapabilities;
    use crate::projection::event::{ProjectionEvent, ProjectionStreamScope};
    use crate::projection::replay::ProjectionStreamKind;
    use crate::projection::snapshot::SessionProjectionSnapshot;

    fn empty_snapshot_for(session_id: &str) -> SessionProjectionSnapshot {
        SessionProjectionSnapshot::empty(session_id, "p1", "w1")
    }

    fn dummy_descriptor(stream: &str) -> ProjectionStreamDescriptor {
        ProjectionStreamDescriptor {
            stream_id: ProjectionStreamId::new(stream).expect("stream id"),
            kind: ProjectionStreamKind::Session,
            project_id: "p1".into(),
            workspace_id: None,
            session_id: Some("s1".into()),
            projection_version: 1,
            retention_floor_seq: 0,
            high_water_seq: 0,
            latest_checkpoint_seq: None,
        }
    }

    fn envelope(scope_session: &str, seq: u64) -> ProjectionEnvelope {
        ProjectionEnvelope {
            protocol_version: 1,
            event_seq: seq,
            timestamp_ms: 0,
            session_id: Some(scope_session.into()),
            turn_id: None,
            scope: ProjectionStreamScope::Session,
            payload: ProjectionEvent::TurnStarted {
                turn: crate::projection::dto::TurnProjection {
                    turn_id: format!("turn-{seq}"),
                    status: crate::projection::dto::TurnStatus::Active,
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
                    input_tokens: Some(0),
                    output_tokens: Some(0),
                },
            },
        }
    }

    #[test]
    fn negotiate_primary_when_versions_intersect() {
        let mut c = ProjectionClientController::new(ProjectionCapabilities::current());
        let daemon = ProjectionCapabilities::default();
        c.negotiate(Some(&daemon));
        assert!(c.mode().is_projection_primary());
        assert_eq!(c.negotiated_version(), Some(1));
    }

    #[test]
    fn negotiate_unsupported_when_no_overlap() {
        let client = ProjectionCapabilities {
            min_version: 5,
            max_version: 5,
            ..Default::default()
        };
        let mut c = ProjectionClientController::new(client);
        let daemon = ProjectionCapabilities::default();
        c.negotiate(Some(&daemon));
        assert!(c.mode().is_unsupported());
        assert!(!c.diagnostics().is_empty());
    }

    #[test]
    fn negotiate_unsupported_when_daemon_absent() {
        let mut c = ProjectionClientController::new(ProjectionCapabilities::current());
        c.negotiate(None);
        assert!(c.mode().is_unsupported());
    }

    #[test]
    fn raw_compatibility_mode_records_reason() {
        let mut c = ProjectionClientController::new(ProjectionCapabilities::current());
        c.enter_raw_compatibility("no projection version overlap");
        assert!(c.mode().is_raw_compatibility());
        assert!(c
            .validate_subscription(&ProjectionSubscriptionRequest {
                scope: ProjectionStreamKind::Session,
                scope_id: "s1".into(),
                cursor: None,
                projection_version: 1,
            })
            .is_err());
    }

    #[test]
    fn subscribe_then_unsubscribe_round_trip() {
        let mut c = ProjectionClientController::new(ProjectionCapabilities::current());
        c.negotiate(Some(&ProjectionCapabilities::default()));
        let descriptor = dummy_descriptor("session-s1");
        let snap = empty_snapshot_for("s1");
        let outcome = c.install_subscription(
            ProjectionSubscriptionId::new("sub-1"),
            descriptor.clone(),
            snap,
        );
        assert!(matches!(
            outcome,
            ControllerSubscribeOutcome::Subscribed { .. }
        ));
        let status = c
            .unsubscribe(&ProjectionSubscriptionId::new("sub-1"))
            .expect("status");
        assert_eq!(status.state, ProjectionSubscriptionState::Closed);
    }

    #[test]
    fn subscribe_limit_enforced() {
        let mut c = ProjectionClientController::new(ProjectionCapabilities::current());
        c.negotiate(Some(&ProjectionCapabilities::default()));
        for i in 0..MAX_CONTROLLER_SUBSCRIPTIONS {
            let stream = format!("session-s{i}");
            let descriptor = dummy_descriptor(&stream);
            let _ = c.install_subscription(
                ProjectionSubscriptionId::new(format!("sub-{i}")),
                descriptor,
                empty_snapshot_for("s1"),
            );
        }
        let descriptor = dummy_descriptor("session-overflow");
        let outcome = c.install_subscription(
            ProjectionSubscriptionId::new("sub-overflow"),
            descriptor,
            empty_snapshot_for("s1"),
        );
        assert!(matches!(
            outcome,
            ControllerSubscribeOutcome::Failed {
                reason: ControllerSubscribeFailure::SubscriptionLimitExceeded
            }
        ));
    }

    #[test]
    fn reconnect_invalidates_state() {
        let mut c = ProjectionClientController::new(ProjectionCapabilities::current());
        c.negotiate(Some(&ProjectionCapabilities::default()));
        let descriptor = dummy_descriptor("session-s1");
        let _ = c.install_subscription(
            ProjectionSubscriptionId::new("sub-1"),
            descriptor,
            empty_snapshot_for("s1"),
        );
        c.on_reconnect();
        assert_eq!(c.reconnect_epoch(), 1);
        assert_eq!(c.subscription_count(), 0);
        assert!(c.mode().is_unsupported());
    }

    #[test]
    fn apply_envelope_updates_seq_and_snapshot() {
        let mut c = ProjectionClientController::new(ProjectionCapabilities::current());
        c.negotiate(Some(&ProjectionCapabilities::default()));
        let descriptor = dummy_descriptor("session-s1");
        let _ = c.install_subscription(
            ProjectionSubscriptionId::new("sub-1"),
            descriptor,
            empty_snapshot_for("s1"),
        );
        let sub_id = ProjectionSubscriptionId::new("sub-1");
        let envelope = envelope("s1", 1);
        let outcome = c.apply_envelope(&sub_id, envelope);
        assert!(matches!(
            outcome,
            ControllerApplyOutcome::Applied { last_seq: 1 }
        ));
    }

    #[test]
    fn duplicate_envelope_returns_duplicate() {
        let mut c = ProjectionClientController::new(ProjectionCapabilities::current());
        c.negotiate(Some(&ProjectionCapabilities::default()));
        let descriptor = dummy_descriptor("session-s1");
        let _ = c.install_subscription(
            ProjectionSubscriptionId::new("sub-1"),
            descriptor,
            empty_snapshot_for("s1"),
        );
        let sub_id = ProjectionSubscriptionId::new("sub-1");
        let _ = c.apply_envelope(&sub_id, envelope("s1", 1));
        let outcome = c.apply_envelope(&sub_id, envelope("s1", 1));
        assert!(matches!(outcome, ControllerApplyOutcome::Duplicate { .. }));
    }

    #[test]
    fn stream_mismatch_triggers_resync() {
        let mut c = ProjectionClientController::new(ProjectionCapabilities::current());
        c.negotiate(Some(&ProjectionCapabilities::default()));
        let descriptor = dummy_descriptor("session-s1");
        let _ = c.install_subscription(
            ProjectionSubscriptionId::new("sub-1"),
            descriptor,
            empty_snapshot_for("s1"),
        );
        let sub_id = ProjectionSubscriptionId::new("sub-1");
        let mut bad = envelope("s1", 1);
        bad.session_id = Some("other".into());
        bad.scope = ProjectionStreamScope::Session;
        let outcome = c.apply_envelope(&sub_id, bad);
        assert!(matches!(
            outcome,
            ControllerApplyOutcome::ResyncRequested {
                reason: ProjectionResyncReason::StreamMismatch,
                ..
            }
        ));
    }

    #[test]
    fn ack_emitted_at_cadence() {
        let mut c = ProjectionClientController::new(ProjectionCapabilities::current());
        c.negotiate(Some(&ProjectionCapabilities::default()));
        let descriptor = dummy_descriptor("session-s1");
        let _ = c.install_subscription(
            ProjectionSubscriptionId::new("sub-1"),
            descriptor,
            empty_snapshot_for("s1"),
        );
        let sub_id = ProjectionSubscriptionId::new("sub-1");
        for seq in 1..=DEFAULT_ACK_CADENCE {
            let _ = c.apply_envelope(&sub_id, envelope("s1", seq));
        }
        let ack = c.try_ack(&sub_id).expect("ack");
        assert_eq!(ack.cursor.event_seq, DEFAULT_ACK_CADENCE);
    }

    #[test]
    fn force_ack_emits_even_below_cadence() {
        let mut c = ProjectionClientController::new(ProjectionCapabilities::current());
        c.negotiate(Some(&ProjectionCapabilities::default()));
        let descriptor = dummy_descriptor("session-s1");
        let _ = c.install_subscription(
            ProjectionSubscriptionId::new("sub-1"),
            descriptor,
            empty_snapshot_for("s1"),
        );
        let sub_id = ProjectionSubscriptionId::new("sub-1");
        let _ = c.apply_envelope(&sub_id, envelope("s1", 1));
        let ack = c.force_ack(&sub_id).expect("ack");
        assert_eq!(ack.cursor.event_seq, 1);
    }

    #[test]
    fn controller_info_reports_state() {
        let mut c = ProjectionClientController::new(ProjectionCapabilities::current());
        c.negotiate(Some(&ProjectionCapabilities::default()));
        let info = c.info();
        assert!(info.mode.is_projection_primary());
        assert_eq!(info.negotiated_version, Some(1));
    }

    #[test]
    fn diagnostics_bounded() {
        let mut c = ProjectionClientController::new(ProjectionCapabilities::current());
        c.negotiate(None);
        for i in 0..(MAX_CONTROLLER_DIAGNOSTICS * 2) {
            c.push_diagnostic(&format!("code-{i}"), "msg");
        }
        assert!(c.diagnostics().len() <= MAX_CONTROLLER_DIAGNOSTICS);
    }
}
