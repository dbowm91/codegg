//! Connection-local ownership for remote projection subscriptions.
//!
//! The daemon replay service remains the sequence and subscription authority.
//! This module only records which authenticated transport connection installed
//! each receiver and bounds the transient work associated with that connection.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::time::Duration;

use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::protocol::projection::replay::{
    ProjectionCursor, ProjectionStreamDescriptor, ProjectionSubscriptionId,
    ProjectionSubscriptionState,
};

pub const MAX_CONNECTION_PROJECTION_SUBSCRIPTIONS: usize = 32;
pub const MAX_CONNECTION_ARTIFACT_READS: usize = 8;
pub const MAX_CONNECTION_DIAGNOSTICS: usize = 32;
pub const CRITICAL_DELIVERY_TIMEOUT: Duration = Duration::from_millis(500);

/// Failure reasons for control frames whose delivery is part of the
/// connection's state transition.  A control frame is never treated as
/// delivered merely because it was serialized or queued: the owning writer
/// must acknowledge the write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CriticalDeliveryError {
    Serialization,
    QueueClosed,
    QueueFull,
    Timeout,
    Cancelled,
    WriterClosed,
}

impl fmt::Display for CriticalDeliveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self {
            Self::Serialization => "serialization",
            Self::QueueClosed => "queue_closed",
            Self::QueueFull => "queue_full",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::WriterClosed => "writer_closed",
        };
        f.write_str(reason)
    }
}

/// Apply the common bounded timeout/cancellation contract to a transport
/// specific send operation.  The operation itself is responsible for
/// distinguishing queue and writer failures.
pub async fn bounded_critical_delivery<F>(
    cancellation: &CancellationToken,
    send: F,
) -> Result<(), CriticalDeliveryError>
where
    F: Future<Output = Result<(), CriticalDeliveryError>>,
{
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(CriticalDeliveryError::Cancelled),
        result = tokio::time::timeout(CRITICAL_DELIVERY_TIMEOUT, send) => {
            result.unwrap_or(Err(CriticalDeliveryError::Timeout))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionConnectionMode {
    Negotiating,
    ProjectionPrimary,
    RawCompatibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnedProjectionLifecycle {
    Initializing,
    Live,
    ResyncRequired,
    Closed,
}

/// Result of trying to complete the connection-local half of projection
/// activation. The transport must call this only after its critical initial
/// response has been accepted by its writer path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionActivationError {
    MissingSubscription,
    InvalidLifecycle(OwnedProjectionLifecycle),
}

impl From<OwnedProjectionLifecycle> for ProjectionSubscriptionState {
    fn from(value: OwnedProjectionLifecycle) -> Self {
        match value {
            OwnedProjectionLifecycle::Initializing => Self::Initializing,
            OwnedProjectionLifecycle::Live => Self::Live,
            OwnedProjectionLifecycle::ResyncRequired => Self::ResyncRequired,
            OwnedProjectionLifecycle::Closed => Self::Closed,
        }
    }
}

pub struct OwnedProjectionSubscription {
    pub subscription_id: ProjectionSubscriptionId,
    pub descriptor: ProjectionStreamDescriptor,
    pub latest_cursor: ProjectionCursor,
    pub last_acked_seq: u64,
    pub retention_floor_seq: u64,
    pub generation: u64,
    pub lifecycle: OwnedProjectionLifecycle,
    pub ready: std::sync::Arc<Notify>,
    pub cancellation: CancellationToken,
    pub forwarder: Option<JoinHandle<()>>,
}

impl OwnedProjectionSubscription {
    pub fn new(
        subscription_id: ProjectionSubscriptionId,
        descriptor: ProjectionStreamDescriptor,
        cursor: ProjectionCursor,
        retention_floor_seq: u64,
        generation: u64,
    ) -> Self {
        Self {
            subscription_id,
            descriptor,
            last_acked_seq: cursor.event_seq,
            latest_cursor: cursor,
            retention_floor_seq,
            generation,
            lifecycle: OwnedProjectionLifecycle::Initializing,
            ready: std::sync::Arc::new(Notify::new()),
            cancellation: CancellationToken::new(),
            forwarder: None,
        }
    }

    pub fn mark_live(&mut self) {
        self.lifecycle = OwnedProjectionLifecycle::Live;
        // `notify_one` retains a permit when the forwarder has not been
        // polled yet, closing the response/forwarder scheduling race.
        self.ready.notify_one();
    }

    pub fn mark_resync_required(&mut self) {
        self.lifecycle = OwnedProjectionLifecycle::ResyncRequired;
        self.cancellation.cancel();
        self.ready.notify_waiters();
    }

    pub fn cancel(&mut self) {
        self.lifecycle = OwnedProjectionLifecycle::Closed;
        self.cancellation.cancel();
        self.ready.notify_waiters();
    }
}

pub struct ProjectionConnectionState {
    connection_id: String,
    mode: ProjectionConnectionMode,
    negotiated_version: Option<u32>,
    reconnect_generation: u64,
    subscriptions: HashMap<ProjectionSubscriptionId, OwnedProjectionSubscription>,
    artifact_reads: usize,
    diagnostics: VecDeque<String>,
    cancellation: CancellationToken,
}

impl ProjectionConnectionState {
    pub fn new(connection_id: impl Into<String>) -> Self {
        Self {
            connection_id: connection_id.into(),
            mode: ProjectionConnectionMode::Negotiating,
            negotiated_version: None,
            reconnect_generation: 0,
            subscriptions: HashMap::new(),
            artifact_reads: 0,
            diagnostics: VecDeque::with_capacity(MAX_CONNECTION_DIAGNOSTICS),
            cancellation: CancellationToken::new(),
        }
    }

    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub fn mode(&self) -> ProjectionConnectionMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: ProjectionConnectionMode, version: Option<u32>) {
        if self.mode != mode || self.negotiated_version != version {
            self.reconnect_generation = self.reconnect_generation.saturating_add(1);
        }
        self.mode = mode;
        self.negotiated_version = version;
        if mode != ProjectionConnectionMode::ProjectionPrimary {
            for subscription in self.subscriptions.values_mut() {
                subscription.cancel();
            }
        }
    }

    pub fn negotiated_version(&self) -> Option<u32> {
        self.negotiated_version
    }

    pub fn reconnect_generation(&self) -> u64 {
        self.reconnect_generation
    }

    pub fn advance_generation(&mut self) -> u64 {
        self.reconnect_generation = self.reconnect_generation.saturating_add(1);
        self.reconnect_generation
    }

    pub fn cancellation(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub fn insert_subscription(
        &mut self,
        subscription: OwnedProjectionSubscription,
    ) -> Result<(), &'static str> {
        if self.subscriptions.len() >= MAX_CONNECTION_PROJECTION_SUBSCRIPTIONS {
            return Err("projection subscription capacity exceeded");
        }
        if self
            .subscriptions
            .contains_key(&subscription.subscription_id)
        {
            return Err("projection subscription already owned");
        }
        self.subscriptions
            .insert(subscription.subscription_id.clone(), subscription);
        Ok(())
    }

    pub fn owns(&self, subscription_id: &ProjectionSubscriptionId) -> bool {
        self.subscriptions.contains_key(subscription_id)
    }

    pub fn subscription(
        &self,
        subscription_id: &ProjectionSubscriptionId,
    ) -> Option<&OwnedProjectionSubscription> {
        self.subscriptions.get(subscription_id)
    }

    pub fn subscription_mut(
        &mut self,
        subscription_id: &ProjectionSubscriptionId,
    ) -> Option<&mut OwnedProjectionSubscription> {
        self.subscriptions.get_mut(subscription_id)
    }

    /// Complete activation after successful delivery of the canonical
    /// snapshot/replay response. Keeping this transition here gives each
    /// transport the same ordering and lifecycle contract.
    pub fn activate_after_delivery(
        &mut self,
        subscription_id: &ProjectionSubscriptionId,
    ) -> Result<(), ProjectionActivationError> {
        let subscription = self
            .subscriptions
            .get_mut(subscription_id)
            .ok_or(ProjectionActivationError::MissingSubscription)?;
        if subscription.lifecycle != OwnedProjectionLifecycle::Initializing {
            return Err(ProjectionActivationError::InvalidLifecycle(
                subscription.lifecycle,
            ));
        }
        subscription.mark_live();
        Ok(())
    }

    /// Cancel and remove a subscription whose initial delivery failed. This
    /// also stops its forwarder, so a failed activation cannot retain a live
    /// task or receiver in the connection-local registry.
    pub async fn rollback_subscription(
        &mut self,
        subscription_id: &ProjectionSubscriptionId,
    ) -> bool {
        let Some(mut subscription) = self.subscriptions.remove(subscription_id) else {
            return false;
        };
        subscription.cancel();
        if let Some(forwarder) = subscription.forwarder.take() {
            forwarder.abort();
            let _ = forwarder.await;
        }
        true
    }

    pub fn remove_subscription(
        &mut self,
        subscription_id: &ProjectionSubscriptionId,
    ) -> Option<OwnedProjectionSubscription> {
        self.subscriptions.remove(subscription_id)
    }

    pub fn subscriptions(&self) -> impl Iterator<Item = &OwnedProjectionSubscription> {
        self.subscriptions.values()
    }

    pub fn owns_project(&self, project_id: &str) -> bool {
        self.subscriptions
            .values()
            .any(|subscription| subscription.descriptor.project_id == project_id)
    }

    pub fn try_begin_artifact_read(&mut self) -> bool {
        if self.artifact_reads >= MAX_CONNECTION_ARTIFACT_READS {
            return false;
        }
        self.artifact_reads += 1;
        true
    }

    pub fn end_artifact_read(&mut self) {
        self.artifact_reads = self.artifact_reads.saturating_sub(1);
    }

    pub fn artifact_reads(&self) -> usize {
        self.artifact_reads
    }

    pub fn record_diagnostic(&mut self, diagnostic: impl Into<String>) {
        if self.diagnostics.len() >= MAX_CONNECTION_DIAGNOSTICS {
            self.diagnostics.pop_front();
        }
        self.diagnostics.push_back(diagnostic.into());
    }

    pub fn diagnostics(&self) -> impl Iterator<Item = &String> {
        self.diagnostics.iter()
    }

    pub async fn cleanup(&mut self) {
        self.cancellation.cancel();
        let mut subscriptions = std::mem::take(&mut self.subscriptions);
        for subscription in subscriptions.values_mut() {
            subscription.cancel();
            if let Some(forwarder) = subscription.forwarder.take() {
                forwarder.abort();
                let _ = forwarder.await;
            }
        }
        self.artifact_reads = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(id: &str) -> OwnedProjectionSubscription {
        let stream_id =
            crate::protocol::projection::replay::ProjectionStreamId::new(format!("stream-{id}"))
                .unwrap();
        let descriptor = ProjectionStreamDescriptor {
            stream_id: stream_id.clone(),
            kind: crate::protocol::projection::replay::ProjectionStreamKind::Session,
            project_id: "project".into(),
            workspace_id: None,
            session_id: Some("session".into()),
            projection_version: 1,
            retention_floor_seq: 0,
            high_water_seq: 0,
            latest_checkpoint_seq: None,
        };
        OwnedProjectionSubscription::new(
            ProjectionSubscriptionId::new(id),
            descriptor,
            ProjectionCursor {
                stream_id,
                event_seq: 0,
                projection_version: 1,
            },
            0,
            0,
        )
    }

    #[test]
    fn bounds_and_foreign_lookup_are_fail_closed() {
        let mut state = ProjectionConnectionState::new("connection-a");
        state.insert_subscription(owned("sub-a")).unwrap();
        assert!(state.owns(&ProjectionSubscriptionId::new("sub-a")));
        assert!(!state.owns(&ProjectionSubscriptionId::new("sub-b")));
        assert!(state.insert_subscription(owned("sub-a")).is_err());
    }

    #[test]
    fn stream_and_subscription_identity_remain_distinct() {
        let state = ProjectionConnectionState::new("connection-a");
        let subscription = owned("subscription");
        assert_ne!(
            subscription.subscription_id.0,
            subscription.descriptor.stream_id.0
        );
        assert_eq!(state.reconnect_generation(), 0);
    }

    #[test]
    fn artifact_reads_and_generation_are_bounded() {
        let mut state = ProjectionConnectionState::new("connection-a");
        for _ in 0..MAX_CONNECTION_ARTIFACT_READS {
            assert!(state.try_begin_artifact_read());
        }
        assert!(!state.try_begin_artifact_read());
        state.end_artifact_read();
        assert!(state.try_begin_artifact_read());
        assert_eq!(state.advance_generation(), 1);
        state.record_diagnostic("one");
        assert_eq!(state.diagnostics().count(), 1);
    }

    #[tokio::test]
    async fn capability_downgrade_cancels_owned_subscriptions() {
        let mut state = ProjectionConnectionState::new("connection-a");
        let subscription = owned("sub-a");
        let cancellation = subscription.cancellation.clone();
        state.insert_subscription(subscription).unwrap();
        state.set_mode(ProjectionConnectionMode::RawCompatibility, None);
        assert!(cancellation.is_cancelled());
        assert_eq!(
            state
                .subscription(&ProjectionSubscriptionId::new("sub-a"))
                .unwrap()
                .lifecycle,
            OwnedProjectionLifecycle::Closed
        );
        state.cleanup().await;
        assert_eq!(state.subscriptions().count(), 0);
    }

    #[test]
    fn activation_is_only_valid_from_initializing() {
        let mut state = ProjectionConnectionState::new("connection-a");
        let subscription_id = ProjectionSubscriptionId::new("sub-a");
        state.insert_subscription(owned("sub-a")).unwrap();

        state
            .activate_after_delivery(&subscription_id)
            .expect("initial delivery activates subscription");
        assert_eq!(
            state.subscription(&subscription_id).unwrap().lifecycle,
            OwnedProjectionLifecycle::Live
        );
        assert_eq!(
            state.activate_after_delivery(&subscription_id),
            Err(ProjectionActivationError::InvalidLifecycle(
                OwnedProjectionLifecycle::Live
            ))
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rollback_removes_and_cancels_initializing_subscription() {
        let mut state = ProjectionConnectionState::new("connection-a");
        let subscription_id = ProjectionSubscriptionId::new("sub-a");
        let cancellation = {
            let subscription = owned("sub-a");
            let cancellation = subscription.cancellation.clone();
            state.insert_subscription(subscription).unwrap();
            cancellation
        };

        assert!(state.rollback_subscription(&subscription_id).await);
        assert!(cancellation.is_cancelled());
        assert!(!state.owns(&subscription_id));
        assert!(!state.rollback_subscription(&subscription_id).await);
    }

    #[test]
    fn project_ownership_is_connection_local() {
        let mut a = ProjectionConnectionState::new("connection-a");
        let mut b = ProjectionConnectionState::new("connection-b");
        a.insert_subscription(owned("sub-a")).unwrap();
        b.insert_subscription(owned("sub-b")).unwrap();
        assert!(a.owns_project("project"));
        assert!(!b.owns(&ProjectionSubscriptionId::new("sub-a")));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn critical_delivery_is_bounded() {
        let cancellation = CancellationToken::new();
        let result = bounded_critical_delivery(&cancellation, async {
            tokio::time::sleep(CRITICAL_DELIVERY_TIMEOUT + Duration::from_millis(25)).await;
            Ok(())
        })
        .await;
        assert_eq!(result, Err(CriticalDeliveryError::Timeout));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn critical_delivery_observes_connection_cancellation() {
        let cancellation = CancellationToken::new();
        let cancel = cancellation.clone();
        let task = tokio::spawn(async move {
            bounded_critical_delivery(
                &cancel,
                std::future::pending::<Result<(), CriticalDeliveryError>>(),
            )
            .await
        });
        cancellation.cancel();
        assert_eq!(task.await.unwrap(), Err(CriticalDeliveryError::Cancelled));
    }
}
