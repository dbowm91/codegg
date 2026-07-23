//! Connection-local ownership for remote projection subscriptions.
//!
//! The daemon replay service remains the sequence and subscription authority.
//! This module only records which authenticated transport connection installed
//! each receiver and bounds the transient work associated with that connection.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex};
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

/// Deterministic checkpoints that a transport adapter may use when testing
/// subscription setup and critical response delivery.  The seam is
/// connection-local; it has no process-wide hooks or mutable test state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectionLifecycleBoundary {
    AfterDaemonSubscriptionCreation,
    AfterReceiverInstallation,
    BeforeControlEnqueue,
    AfterControlEnqueueBeforeWriterReceipt,
    DuringWriterWrite,
    BeforeActivation,
}

/// Accepted cancellation-token forms for lifecycle checkpoints. The
/// transport adapters commonly hold either a borrowed connection token or a
/// cloned token while a setup task is in flight.
pub trait ProjectionCancellation {
    fn clone_token(&self) -> CancellationToken;
}

impl ProjectionCancellation for CancellationToken {
    fn clone_token(&self) -> CancellationToken {
        self.clone()
    }
}

impl ProjectionCancellation for &CancellationToken {
    fn clone_token(&self) -> CancellationToken {
        (*self).clone()
    }
}

/// A one-shot gate used by [`ProjectionLifecycleSeam`] to pause an adapter at
/// a deterministic boundary.  The atomic flags close the notify registration
/// race, while `notify_one` retains a permit if release happens first.
#[derive(Clone)]
pub struct ProjectionLifecycleGate {
    inner: Arc<ProjectionLifecycleGateState>,
}

struct ProjectionLifecycleGateState {
    entered: std::sync::atomic::AtomicBool,
    released: std::sync::atomic::AtomicBool,
    entered_notify: Notify,
    released_notify: Notify,
}

impl ProjectionLifecycleGate {
    fn new() -> Self {
        Self {
            inner: Arc::new(ProjectionLifecycleGateState {
                entered: std::sync::atomic::AtomicBool::new(false),
                released: std::sync::atomic::AtomicBool::new(false),
                entered_notify: Notify::new(),
                released_notify: Notify::new(),
            }),
        }
    }

    /// Wait until the adapter reaches this gate.
    pub async fn wait_until_entered(&self) {
        loop {
            if self
                .inner
                .entered
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return;
            }
            let notified = self.inner.entered_notify.notified();
            if self
                .inner
                .entered
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return;
            }
            notified.await;
        }
    }

    /// Release the paused adapter. Releasing before the adapter arrives is
    /// supported and is intentionally not lost.
    pub fn release(&self) {
        self.inner
            .released
            .store(true, std::sync::atomic::Ordering::Release);
        self.inner.released_notify.notify_one();
    }

    async fn wait(&self, cancellation: &CancellationToken) -> Result<(), CriticalDeliveryError> {
        self.inner
            .entered
            .store(true, std::sync::atomic::Ordering::Release);
        self.inner.entered_notify.notify_one();

        loop {
            if self
                .inner
                .released
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return Ok(());
            }
            let notified = self.inner.released_notify.notified();
            if self
                .inner
                .released
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return Ok(());
            }
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(CriticalDeliveryError::Cancelled),
                _ = notified => {}
            }
        }
    }
}

enum ProjectionLifecycleFault {
    Failure(CriticalDeliveryError),
    Pause(ProjectionLifecycleGate),
}

/// Connection-owned fault injection for transport adapter tests.
///
/// Adapters call [`ProjectionLifecycleSeam::checkpoint`] at the boundaries
/// above. Production connections use the same no-op seam, so tests can inject
/// a pause or one-shot failure without process-wide mutable state.
#[derive(Clone, Default)]
pub struct ProjectionLifecycleSeam {
    faults: Arc<Mutex<HashMap<ProjectionLifecycleBoundary, VecDeque<ProjectionLifecycleFault>>>>,
}

impl ProjectionLifecycleSeam {
    /// Fail the next checkpoint at `boundary` with `error`.
    pub fn fail_next(&self, boundary: ProjectionLifecycleBoundary, error: CriticalDeliveryError) {
        self.faults
            .lock()
            .expect("projection lifecycle seam lock poisoned")
            .entry(boundary)
            .or_default()
            .push_back(ProjectionLifecycleFault::Failure(error));
    }

    /// Pause the next checkpoint at `boundary` until the returned gate is
    /// released or `cancellation` wins the checkpoint.
    pub fn pause_next(&self, boundary: ProjectionLifecycleBoundary) -> ProjectionLifecycleGate {
        let gate = ProjectionLifecycleGate::new();
        self.faults
            .lock()
            .expect("projection lifecycle seam lock poisoned")
            .entry(boundary)
            .or_default()
            .push_back(ProjectionLifecycleFault::Pause(gate.clone()));
        gate
    }

    /// Reach a deterministic adapter boundary. A cancellation that is already
    /// visible wins before an injected fault, matching critical delivery's
    /// cancellation contract.
    pub async fn checkpoint<C: ProjectionCancellation>(
        &self,
        boundary: ProjectionLifecycleBoundary,
        cancellation: C,
    ) -> Result<(), CriticalDeliveryError> {
        let cancellation = cancellation.clone_token();
        if cancellation.is_cancelled() {
            return Err(CriticalDeliveryError::Cancelled);
        }

        let fault = self
            .faults
            .lock()
            .expect("projection lifecycle seam lock poisoned")
            .get_mut(&boundary)
            .and_then(VecDeque::pop_front);

        match fault {
            Some(ProjectionLifecycleFault::Failure(error)) => Err(error),
            Some(ProjectionLifecycleFault::Pause(gate)) => gate.wait(&cancellation).await,
            None => Ok(()),
        }
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
    Cancelled,
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

    /// Complete activation. This is private so transports must use
    /// `ProjectionConnectionState::activate_after_delivery`, which owns the
    /// response-delivery and cancellation checks.
    fn mark_live(&mut self) {
        self.lifecycle = OwnedProjectionLifecycle::Live;
        // `notify_one` retains a permit when the forwarder has not been
        // polled yet, closing the response/forwarder scheduling race.
        self.ready.notify_one();
    }

    pub fn mark_resync_required(&mut self) {
        if self.lifecycle == OwnedProjectionLifecycle::Closed {
            return;
        }
        self.lifecycle = OwnedProjectionLifecycle::ResyncRequired;
        self.cancellation.cancel();
        self.ready.notify_waiters();
    }

    pub fn cancel(&mut self) {
        if self.lifecycle == OwnedProjectionLifecycle::Closed {
            return;
        }
        self.lifecycle = OwnedProjectionLifecycle::Closed;
        self.cancellation.cancel();
        self.ready.notify_waiters();
    }

    fn abort_forwarder(&mut self) {
        if let Some(forwarder) = self.forwarder.as_ref() {
            forwarder.abort();
        }
    }

    /// Cancel this subscription and abort-and-await its forwarder. The method
    /// is idempotent, making it safe for rollback, disconnect, and shutdown
    /// paths that converge on the same owned subscription.
    pub async fn shutdown(&mut self) {
        self.cancel();
        if let Some(forwarder) = self.forwarder.take() {
            forwarder.abort();
            let _ = forwarder.await;
        }
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
    lifecycle_seam: ProjectionLifecycleSeam,
    forwarder_join_counter: Option<Arc<std::sync::atomic::AtomicUsize>>,
    forwarder_install_counter: Option<Arc<std::sync::atomic::AtomicUsize>>,
}

impl ProjectionConnectionState {
    pub fn new(connection_id: impl Into<String>) -> Self {
        Self::new_with_lifecycle_seam(connection_id, ProjectionLifecycleSeam::default())
    }

    /// Construct connection state with an adapter-owned lifecycle seam.
    /// Production callers should normally use [`Self::new`].
    pub fn new_with_lifecycle_seam(
        connection_id: impl Into<String>,
        lifecycle_seam: ProjectionLifecycleSeam,
    ) -> Self {
        Self {
            connection_id: connection_id.into(),
            mode: ProjectionConnectionMode::Negotiating,
            negotiated_version: None,
            reconnect_generation: 0,
            subscriptions: HashMap::new(),
            artifact_reads: 0,
            diagnostics: VecDeque::with_capacity(MAX_CONNECTION_DIAGNOSTICS),
            cancellation: CancellationToken::new(),
            lifecycle_seam,
            forwarder_join_counter: None,
            forwarder_install_counter: None,
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

    /// Return the connection-local lifecycle seam used by transport adapter
    /// tests. The default seam is a no-op.
    pub fn lifecycle_seam(&self) -> ProjectionLifecycleSeam {
        self.lifecycle_seam.clone()
    }

    /// Install a payload-free lifecycle counter used by an owning transport to
    /// record projection-forwarder joins only after the handle has been
    /// awaited. The core state remains independent of any server probe type.
    pub fn set_forwarder_join_counter(&mut self, counter: Arc<std::sync::atomic::AtomicUsize>) {
        self.forwarder_join_counter = Some(counter);
    }

    pub fn forwarder_join_counter(&self) -> Option<Arc<std::sync::atomic::AtomicUsize>> {
        self.forwarder_join_counter.clone()
    }

    /// Install a payload-free lifecycle counter used by an owning transport
    /// to record each projection forwarder that was successfully installed.
    pub fn set_forwarder_install_counter(&mut self, counter: Arc<std::sync::atomic::AtomicUsize>) {
        self.forwarder_install_counter = Some(counter);
    }

    pub fn forwarder_install_counter(&self) -> Option<Arc<std::sync::atomic::AtomicUsize>> {
        self.forwarder_install_counter.clone()
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
        let connection_cancelled = self.cancellation.is_cancelled();
        let subscription = self
            .subscriptions
            .get_mut(subscription_id)
            .ok_or(ProjectionActivationError::MissingSubscription)?;
        if subscription.lifecycle != OwnedProjectionLifecycle::Initializing {
            return Err(ProjectionActivationError::InvalidLifecycle(
                subscription.lifecycle,
            ));
        }
        if connection_cancelled || subscription.cancellation.is_cancelled() {
            subscription.cancel();
            return Err(ProjectionActivationError::Cancelled);
        }
        subscription.mark_live();
        Ok(())
    }

    /// Remove and cancel a subscription before awaiting its task. Callers
    /// must release the connection-state mutex before joining the returned
    /// forwarder; task shutdown can otherwise deadlock on the same state.
    pub fn remove_subscription_for_cleanup(
        &mut self,
        subscription_id: &ProjectionSubscriptionId,
    ) -> Option<OwnedProjectionSubscription> {
        let mut subscription = self.subscriptions.remove(subscription_id)?;
        subscription.cancel();
        subscription.abort_forwarder();
        Some(subscription)
    }

    pub fn remove_subscription(
        &mut self,
        subscription_id: &ProjectionSubscriptionId,
    ) -> Option<OwnedProjectionSubscription> {
        let mut subscription = self.subscriptions.remove(subscription_id)?;
        // This synchronous extraction cannot await. Request termination here
        // so a caller that drops the returned value cannot leave a live
        // forwarder; async callers should still join it.
        subscription.cancel();
        subscription.abort_forwarder();
        Some(subscription)
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

    /// Cancel and detach all connection-owned subscriptions. The returned
    /// tasks must be joined after the caller releases the state lock.
    pub fn drain_for_cleanup(&mut self) -> Vec<OwnedProjectionSubscription> {
        self.cancellation.cancel();
        let mut subscriptions = std::mem::take(&mut self.subscriptions)
            .into_values()
            .collect::<Vec<_>>();
        for subscription in &mut subscriptions {
            subscription.cancel();
        }
        self.artifact_reads = 0;
        subscriptions
    }

    /// Abort and await detached connection-owned forwarders. This is kept
    /// separate from [`Self::drain_for_cleanup`] so no state mutex is held
    /// while a task is being joined.
    pub async fn join_cleanup_tasks(mut subscriptions: Vec<OwnedProjectionSubscription>) {
        for subscription in &mut subscriptions {
            subscription.shutdown().await;
        }
    }

    pub async fn join_cleanup_tasks_with_counter(
        mut subscriptions: Vec<OwnedProjectionSubscription>,
        counter: Arc<std::sync::atomic::AtomicUsize>,
    ) {
        for subscription in &mut subscriptions {
            let had_forwarder = subscription.forwarder.is_some();
            subscription.shutdown().await;
            if had_forwarder {
                counter.fetch_add(1, std::sync::atomic::Ordering::Release);
            }
        }
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
        let subscriptions = state.drain_for_cleanup();
        ProjectionConnectionState::join_cleanup_tasks(subscriptions).await;
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

    #[test]
    fn activation_and_rollback_are_single_terminal() {
        let mut state = ProjectionConnectionState::new("connection-a");
        let subscription_id = ProjectionSubscriptionId::new("sub-a");
        state.insert_subscription(owned("sub-a")).unwrap();

        state.activate_after_delivery(&subscription_id).unwrap();
        assert_eq!(
            state.activate_after_delivery(&subscription_id),
            Err(ProjectionActivationError::InvalidLifecycle(
                OwnedProjectionLifecycle::Live
            ))
        );

        let removed = state.remove_subscription_for_cleanup(&subscription_id);
        assert!(removed.is_some());
        assert!(state
            .remove_subscription_for_cleanup(&subscription_id)
            .is_none());
    }

    #[test]
    fn activation_rejects_connection_cancellation() {
        let mut state = ProjectionConnectionState::new("connection-a");
        let subscription_id = ProjectionSubscriptionId::new("sub-a");
        state.insert_subscription(owned("sub-a")).unwrap();
        state.cancellation().cancel();

        assert_eq!(
            state.activate_after_delivery(&subscription_id),
            Err(ProjectionActivationError::Cancelled)
        );
        assert_eq!(
            state.subscription(&subscription_id).unwrap().lifecycle,
            OwnedProjectionLifecycle::Closed
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

        let subscription = state.remove_subscription_for_cleanup(&subscription_id);
        assert!(subscription.is_some());
        assert!(cancellation.is_cancelled());
        assert!(!state.owns(&subscription_id));
        ProjectionConnectionState::join_cleanup_tasks(subscription.into_iter().collect()).await;
        assert!(state
            .remove_subscription_for_cleanup(&subscription_id)
            .is_none());
    }

    struct NotifyOnDrop(Option<tokio::sync::oneshot::Sender<()>>);

    impl Drop for NotifyOnDrop {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cleanup_aborts_and_awaits_forwarders() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (stopped_tx, stopped_rx) = tokio::sync::oneshot::channel();
        let forwarder = tokio::spawn(async move {
            let _on_drop = NotifyOnDrop(Some(stopped_tx));
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        started_rx.await.unwrap();

        let mut subscription = owned("sub-a");
        subscription.forwarder = Some(forwarder);
        let mut state = ProjectionConnectionState::new("connection-a");
        state.insert_subscription(subscription).unwrap();

        let subscriptions = state.drain_for_cleanup();
        ProjectionConnectionState::join_cleanup_tasks(subscriptions).await;
        stopped_rx.await.unwrap();
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

    #[tokio::test(flavor = "current_thread")]
    async fn lifecycle_seam_injects_failure_and_cancellable_pause() {
        let seam = ProjectionLifecycleSeam::default();
        let mut state = ProjectionConnectionState::new_with_lifecycle_seam("connection-a", seam);
        let subscription_id = ProjectionSubscriptionId::new("sub-a");
        state.insert_subscription(owned("sub-a")).unwrap();
        let seam = state.lifecycle_seam();
        let cancellation = state.cancellation();
        seam.fail_next(
            ProjectionLifecycleBoundary::BeforeActivation,
            CriticalDeliveryError::WriterClosed,
        );
        assert_eq!(
            seam.checkpoint(ProjectionLifecycleBoundary::BeforeActivation, &cancellation,)
                .await,
            Err(CriticalDeliveryError::WriterClosed)
        );

        let gate = seam.pause_next(ProjectionLifecycleBoundary::BeforeActivation);
        let checkpoint = {
            let seam = seam.clone();
            let cancellation = cancellation.clone();
            tokio::spawn(async move {
                seam.checkpoint(ProjectionLifecycleBoundary::BeforeActivation, &cancellation)
                    .await
            })
        };
        gate.wait_until_entered().await;
        cancellation.cancel();
        assert_eq!(
            checkpoint.await.unwrap(),
            Err(CriticalDeliveryError::Cancelled)
        );
        assert_eq!(
            state.activate_after_delivery(&subscription_id),
            Err(ProjectionActivationError::Cancelled)
        );
    }
}
