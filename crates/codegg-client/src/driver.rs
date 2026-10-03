//! Loss-aware session projection driver for non-TUI frontends.
//!
//! [`SessionProjectionDriver`] composes one authenticated
//! [`LocalSocketClient`] with the canonical
//! [`HeadlessProjectionConsumer`](codegg_protocol::projection::consumer::HeadlessProjectionConsumer):
//! the client owns request/response correlation and the typed event
//! stream, while the consumer owns all projection reduction, cursor, and
//! snapshot state. The driver adds connection/subscription generation
//! tracking, acknowledgement cadence, resume/resync orchestration, exact
//! subscription filtering, cancellation/join ownership, and immutable
//! bounded snapshot publication. It owns no UI state, no credentials, no
//! daemon persistence, and no execution authority.
//!
//! Transport loss is never silent: [`ClientEvent::Lagged`] stops live
//! application and forces an authoritative resume/resync from the retained
//! canonical cursor before any further live event is accepted.

use std::sync::Arc;

use tokio::sync::{mpsc, oneshot, watch};

use codegg_protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope, PROTOCOL_VERSION,
};
use codegg_protocol::projection::consumer::{
    HeadlessConnectionState, HeadlessConsumerError, HeadlessEventOutcome,
    HeadlessProjectionConsumer,
};
use codegg_protocol::projection::replay::{
    ProjectionArtifactReadOutcome, ProjectionCursor, ProjectionResyncReason,
};
use codegg_protocol::projection::snapshot::SessionProjectionSnapshot;

use crate::{ClientError, ClientEvent, LocalSocketClient};

/// Presentation-level driver state published to UI observers.
///
/// This mirrors the renderer projection contract (`connecting |
/// subscribing | attached | resyncing | disconnected | unavailable`):
/// `Connecting`/`Subscribing` cover the two M004 handshake phases,
/// `Attached` means live delivery is converging on the current
/// subscription, `Resyncing` means live application is paused while an
/// authoritative resume/resync converges, `Disconnected` means the
/// transport closed or the subscription was released, and `Unavailable`
/// means the daemon denied or cannot serve the projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverState {
    Connecting,
    Subscribing,
    Attached,
    Resyncing,
    Disconnected,
    Unavailable,
}

/// Immutable bounded publication of the current driver view.
///
/// `snapshot` is a clone of the canonical consumer snapshot (already
/// visibility-sanitized and truncated by the consumer); sequence/cursor
/// fields are diagnostic and must never become projection authority in a
/// renderer.
#[derive(Debug, Clone)]
pub struct DriverSnapshotView {
    pub state: DriverState,
    pub session_id: String,
    pub generation: u64,
    pub subscription_id: Option<String>,
    pub cursor_seq: Option<u64>,
    pub snapshot: Option<Arc<SessionProjectionSnapshot>>,
    pub resync_reason: Option<ProjectionResyncReason>,
}

/// Driver tuning. All bounds are small and explicit; no unbounded
/// buffering is used to "solve" lag anywhere in this module.
#[derive(Debug, Clone)]
pub struct DriverConfig {
    /// Send a projection acknowledgement every `ack_every` processed
    /// live events (any outcome, including duplicates and ignored
    /// non-public events, since each advances or confirms the cursor).
    pub ack_every: usize,
    /// Upper bound on resume/subscribe rounds per converge pass before
    /// the driver fails closed instead of looping against the daemon.
    pub max_resume_rounds: usize,
}

impl Default for DriverConfig {
    fn default() -> Self {
        Self {
            ack_every: 8,
            max_resume_rounds: 8,
        }
    }
}

/// Bounded driver failures. Transport loss surfaces as state
/// transitions inside the event loop; only attach/resume setup failures
/// and fail-closed convergence failures are returned as errors.
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("projection driver transport failed: {0}")]
    Transport(#[from] ClientError),
    #[error("projection driver consumer refused frame: {0}")]
    Consumer(#[from] HeadlessConsumerError),
    #[error("daemon does not support session projections")]
    Unsupported,
    #[error("daemon denied projection subscription: {0}")]
    Denied(String),
    #[error("projection driver could not converge after resume")]
    NoConvergence,
    #[error("projection driver task failed to join")]
    JoinFailed,
}

struct DriverCore {
    client: LocalSocketClient,
    consumer: HeadlessProjectionConsumer,
    session_id: String,
    generation: u64,
    processed_since_ack: usize,
    ignored_mismatches: u64,
    config: DriverConfig,
    updates: watch::Sender<DriverSnapshotView>,
}

/// One bounded artifact excerpt request to the driver event loop. The
/// loop owns the consumer, so handle validation
/// (`artifact_read_request`) and outcome validation
/// (`accept_artifact_outcome`) run against live canonical state.
struct ArtifactReadCommand {
    project_id: String,
    handle_id: String,
    start: u64,
    end: Option<u64>,
    reply: oneshot::Sender<Result<ProjectionArtifactReadOutcome, DriverError>>,
}

impl DriverCore {
    fn publish(&self, state: DriverState) {
        let _ = self.updates.send(DriverSnapshotView {
            state,
            session_id: self.session_id.clone(),
            generation: self.generation,
            subscription_id: self
                .consumer
                .subscription_id()
                .map(|id| id.as_str().to_string()),
            cursor_seq: self.consumer.cursor().map(|cursor| cursor.event_seq),
            snapshot: self.consumer.snapshot().cloned().map(Arc::new),
            resync_reason: self.consumer.last_resync_reason(),
        });
    }

    async fn request(&self, payload: CoreRequest) -> Result<CoreResponse, DriverError> {
        let envelope = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: format!("projection-driver-{}", uuid::Uuid::new_v4()),
            payload,
        };
        Ok(self.client.request(envelope).await?)
    }

    /// Negotiate projection capabilities on the current client. Any
    /// non-supporting or unexpected answer is `Unsupported`, never a
    /// silent downgrade: without incremental events the driver cannot
    /// guarantee lossless delivery.
    async fn negotiate(&mut self) -> Result<(), DriverError> {
        match self.request(CoreRequest::ProjectionCapabilities).await? {
            response @ CoreResponse::ProjectionCapabilitiesResponse { .. } => {
                self.consumer
                    .connect_from_response(&response)
                    .map_err(|error| match error {
                        HeadlessConsumerError::UnsupportedCapabilities => DriverError::Unsupported,
                        other => DriverError::Consumer(other),
                    })?;
                Ok(())
            }
            _ => Err(DriverError::Unsupported),
        }
    }

    /// Converge the consumer to `Attached` through resume (when a cursor
    /// is retained) or fresh subscribe, following daemon replay/resync
    /// authority. A daemon `Error` on resume falls back to one fresh
    /// subscribe; a daemon `Error` on subscribe is a fail-closed denial.
    /// Replay continuation cursors are followed until the daemon stops
    /// advertising them or the round budget is exhausted.
    async fn converge(&mut self) -> Result<(), DriverError> {
        let mut force_subscribe = self.consumer.cursor().is_none();
        let mut continuation = false;
        for _ in 0..self.config.max_resume_rounds {
            if self.consumer.connection_state() == HeadlessConnectionState::Attached
                && !continuation
            {
                return Ok(());
            }
            continuation = false;
            let resume = !force_subscribe && self.consumer.cursor().is_some();
            let payload = if resume {
                self.consumer.resume_request()?
            } else {
                self.consumer.attach_core_request(self.session_id.clone())?
            };
            let response = self.request(payload).await?;
            if let CoreResponse::Error { code, .. } = &response {
                if resume {
                    // The daemon rejected the cursor resume; fall back to
                    // one authoritative fresh subscribe next round.
                    force_subscribe = true;
                    continue;
                }
                return Err(DriverError::Denied(code.clone()));
            }
            let outcome = self.consumer.accept_response(&response)?;
            match self.consumer.connection_state() {
                HeadlessConnectionState::Attached => {
                    if outcome.and_then(|replay| replay.next_cursor).is_some() {
                        continuation = true;
                    } else {
                        return Ok(());
                    }
                }
                // Resume converged without an installable snapshot (bare
                // resync) or the cursor was refused: fresh subscribe next.
                _ => force_subscribe = true,
            }
        }
        Err(DriverError::NoConvergence)
    }

    /// Best-effort acknowledgement cadence. Ack failures never corrupt
    /// projection state; a dead transport is observed through the event
    /// stream and handled there.
    async fn maybe_ack(&mut self) -> bool {
        if self.processed_since_ack < self.config.ack_every {
            return true;
        }
        self.processed_since_ack = 0;
        let payload = match self.consumer.ack_request() {
            Ok(payload) => payload,
            Err(_) => return true,
        };
        match self.request(payload).await {
            Ok(_) => true,
            Err(DriverError::Transport(_)) => false,
            Err(_) => true,
        }
    }

    /// Authoritative resume/resync: stop applying live events, converge
    /// from the retained cursor, and only then accept live delivery
    /// again. Returns `false` when the loop must terminate.
    async fn resync(&mut self) -> bool {
        self.processed_since_ack = 0;
        self.publish(DriverState::Resyncing);
        match self.converge().await {
            Ok(()) => {
                self.publish(DriverState::Attached);
                true
            }
            Err(DriverError::Transport(_)) => {
                self.consumer.disconnect();
                self.publish(DriverState::Disconnected);
                false
            }
            Err(_) => {
                self.publish(DriverState::Unavailable);
                false
            }
        }
    }

    /// Apply one transport envelope. Non-projection payloads and events
    /// for a stale subscription are ignored without touching consumer
    /// state. Returns `false` when the loop must terminate.
    async fn on_event(&mut self, envelope: EventEnvelope<CoreEvent>) -> bool {
        let CoreEvent::ProjectionStreamEvent {
            subscription_id,
            envelope,
            ..
        } = &envelope.payload
        else {
            return true;
        };
        if Some(subscription_id) != self.consumer.subscription_id() {
            // Exact subscription filtering: a live event for any other
            // subscription (stale generation, foreign stream) must never
            // advance this driver's cursor.
            self.ignored_mismatches = self.ignored_mismatches.saturating_add(1);
            return true;
        }
        match self.consumer.apply_event(envelope.clone()) {
            HeadlessEventOutcome::Applied { .. }
            | HeadlessEventOutcome::Reconciled { .. }
            | HeadlessEventOutcome::Duplicate { .. }
            | HeadlessEventOutcome::IgnoredNonPublic { .. } => {
                self.processed_since_ack = self.processed_since_ack.saturating_add(1);
                let alive = self.maybe_ack().await;
                self.publish(if alive {
                    DriverState::Attached
                } else {
                    DriverState::Disconnected
                });
                if !alive {
                    self.consumer.disconnect();
                }
                alive
            }
            HeadlessEventOutcome::ResyncRequired { .. } => self.resync().await,
            HeadlessEventOutcome::Error(_) => {
                self.publish(DriverState::Unavailable);
                false
            }
        }
    }

    /// Bounded artifact excerpt through the canonical consumer: the
    /// authorized handle registry is refreshed first so a stale
    /// renderer handle fails closed, then the read request is built
    /// with consumer-side project/revision/bounds validation and the
    /// outcome is validated before it is returned.
    async fn on_artifact_read(&mut self, cmd: ArtifactReadCommand) {
        let outcome = self.read_artifact(&cmd).await;
        let _ = cmd.reply.send(outcome);
    }

    async fn read_artifact(
        &mut self,
        cmd: &ArtifactReadCommand,
    ) -> Result<ProjectionArtifactReadOutcome, DriverError> {
        match self
            .request(CoreRequest::ProjectionArtifactList {
                project_id: cmd.project_id.clone(),
            })
            .await?
        {
            CoreResponse::ProjectionArtifactList { handles } => {
                self.consumer
                    .accept_artifact_handles(handles)
                    .map_err(DriverError::Consumer)?;
            }
            other => {
                return Err(DriverError::Denied(format!(
                    "unexpected artifact list response: {other:?}"
                )));
            }
        }
        let payload = self
            .consumer
            .artifact_read_request(&cmd.handle_id, cmd.start, cmd.end)
            .map_err(DriverError::Consumer)?;
        match self.request(payload).await? {
            CoreResponse::ProjectionArtifactRead { outcome } => {
                self.consumer
                    .accept_artifact_outcome(&outcome)
                    .map_err(DriverError::Consumer)?;
                Ok(outcome)
            }
            other => Err(DriverError::Denied(format!(
                "unexpected artifact response: {other:?}"
            ))),
        }
    }

    async fn run(
        mut self,
        mut events: mpsc::Receiver<ClientEvent>,
        mut stop: oneshot::Receiver<()>,
        mut commands: mpsc::Receiver<ArtifactReadCommand>,
    ) -> HeadlessProjectionConsumer {
        // `events` was installed before the subscribe request was sent
        // (attach/resume install it pre-request), preserving the daemon's
        // response-before-live ordering contract: no committed live event
        // can predate this receiver.
        //
        // The liveness tick covers peer death: the shared broadcast stays
        // open while any client clone lives, so a dead reader would
        // otherwise leave this loop parked in `recv` forever. The driver
        // requires a stable client — an in-place `reconnect` underneath
        // an attached driver fails closed here (documented stop path:
        // stop the driver, reconnect, then `resume`).
        let mut liveness = tokio::time::interval(std::time::Duration::from_millis(25));
        loop {
            tokio::select! {
                _ = &mut stop => break,
                // Disabled once every excerpt handle is gone so a
                // closed channel can never busy-loop; live delivery
                // continues until stop.
                cmd = commands.recv(), if !commands.is_closed() => {
                    if let Some(cmd) = cmd {
                        self.on_artifact_read(cmd).await;
                    }
                }
                _ = liveness.tick() => {
                    if self.client.is_closed() {
                        self.consumer.disconnect();
                        self.publish(DriverState::Disconnected);
                        break;
                    }
                }
                item = events.recv() => match item {
                    Some(ClientEvent::Event(envelope)) => {
                        if !self.on_event(*envelope).await {
                            break;
                        }
                    }
                    Some(ClientEvent::Lagged { dropped }) => {
                        tracing::warn!(
                            dropped,
                            session_id = %self.session_id,
                            "projection driver lagged; forcing authoritative resync"
                        );
                        if !self.resync().await {
                            break;
                        }
                    }
                    Some(ClientEvent::Closed) | None => {
                        self.consumer.disconnect();
                        self.publish(DriverState::Disconnected);
                        break;
                    }
                },
            }
        }
        // Best-effort explicit unsubscribe: release daemon subscription
        // ownership with the exact current id. Transport loss here is
        // harmless — the daemon owns subscription expiry.
        if let Ok(payload) = self.consumer.unsubscribe_request() {
            let _ = self.request(payload).await;
        }
        self.consumer.disconnect();
        self.publish(DriverState::Disconnected);
        self.consumer
    }
}

/// Handle to one active primary session projection.
///
/// Exactly one active projection per renderer route is supported; the
/// driver owns one connection generation, one daemon subscription, one
/// loss-aware receiver, and one ordered event loop task.
pub struct SessionProjectionDriver {
    task: Option<tokio::task::JoinHandle<HeadlessProjectionConsumer>>,
    snapshots: watch::Receiver<DriverSnapshotView>,
    stop_tx: Option<oneshot::Sender<()>>,
    commands: mpsc::Sender<ArtifactReadCommand>,
    session_id: String,
    generation: u64,
}

impl std::fmt::Debug for SessionProjectionDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionProjectionDriver")
            .field("session_id", &self.session_id)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl SessionProjectionDriver {
    /// Negotiate capabilities and subscribe to `session_id` on `client`,
    /// then spawn the ordered event loop.
    pub async fn attach(
        client: LocalSocketClient,
        session_id: String,
    ) -> Result<Self, DriverError> {
        Self::attach_with_config(client, session_id, DriverConfig::default()).await
    }

    pub async fn attach_with_config(
        client: LocalSocketClient,
        session_id: String,
        config: DriverConfig,
    ) -> Result<Self, DriverError> {
        let (updates, snapshots) = watch::channel(DriverSnapshotView {
            state: DriverState::Connecting,
            session_id: session_id.clone(),
            generation: 1,
            subscription_id: None,
            cursor_seq: None,
            snapshot: None,
            resync_reason: None,
        });
        let mut core = DriverCore {
            client,
            consumer: HeadlessProjectionConsumer::new(),
            session_id: session_id.clone(),
            generation: 1,
            processed_since_ack: 0,
            ignored_mismatches: 0,
            config,
            updates,
        };
        core.publish(DriverState::Connecting);
        // Install the loss-aware receiver BEFORE sending the subscribe
        // request so no committed live event can predate it (§2.3).
        let events = core.client.subscribe_events();
        core.negotiate().await?;
        core.publish(DriverState::Subscribing);
        core.converge().await?;
        core.publish(DriverState::Attached);
        let (stop_tx, stop_rx) = oneshot::channel();
        let (command_tx, command_rx) = mpsc::channel(8);
        let task = tokio::spawn(core.run(events, stop_rx, command_rx));
        Ok(Self {
            task: Some(task),
            snapshots,
            stop_tx: Some(stop_tx),
            commands: command_tx,
            session_id,
            generation: 1,
        })
    }

    /// Resume a stopped session on a new client generation with the
    /// retained canonical cursor. The old subscription id is never
    /// reused; the daemon issues a fresh one through replay/resync.
    pub async fn resume(
        client: LocalSocketClient,
        stopped: StoppedDriver,
    ) -> Result<Self, DriverError> {
        Self::resume_with_config(client, stopped, DriverConfig::default()).await
    }

    pub async fn resume_with_config(
        client: LocalSocketClient,
        stopped: StoppedDriver,
        config: DriverConfig,
    ) -> Result<Self, DriverError> {
        let generation = stopped.generation.saturating_add(1);
        let session_id = stopped.session_id.clone();
        let (updates, snapshots) = watch::channel(DriverSnapshotView {
            state: DriverState::Connecting,
            session_id: session_id.clone(),
            generation,
            subscription_id: None,
            cursor_seq: stopped.consumer.cursor().map(|cursor| cursor.event_seq),
            snapshot: stopped.consumer.snapshot().cloned().map(Arc::new),
            resync_reason: None,
        });
        let mut core = DriverCore {
            client,
            consumer: stopped.consumer,
            session_id: session_id.clone(),
            generation,
            processed_since_ack: 0,
            ignored_mismatches: 0,
            config,
            updates,
        };
        core.publish(DriverState::Connecting);
        // Install the loss-aware receiver BEFORE sending the resume
        // request so no committed live event can predate it (§2.3).
        let events = core.client.subscribe_events();
        core.negotiate().await?;
        core.publish(DriverState::Resyncing);
        core.converge().await?;
        core.publish(DriverState::Attached);
        let (stop_tx, stop_rx) = oneshot::channel();
        let (command_tx, command_rx) = mpsc::channel(8);
        let task = tokio::spawn(core.run(events, stop_rx, command_rx));
        Ok(Self {
            task: Some(task),
            snapshots,
            stop_tx: Some(stop_tx),
            commands: command_tx,
            session_id,
            generation,
        })
    }

    /// Borrow the latest published view without subscribing.
    pub fn current(&self) -> DriverSnapshotView {
        self.snapshots.borrow().clone()
    }

    /// Read one bounded artifact excerpt through the driver event loop.
    /// The loop refreshes the authorized handle registry from the
    /// daemon, builds the read with consumer-side validation (opaque
    /// handle, project binding, revision, 64 KiB window), and validates
    /// the outcome before returning it. Unknown/stale handles fail
    /// closed without any path-based read. Requires a live driver;
    /// a stopped driver reports `JoinFailed`.
    pub async fn artifact_excerpt(
        &self,
        project_id: &str,
        handle_id: &str,
        start: u64,
        end: Option<u64>,
    ) -> Result<ProjectionArtifactReadOutcome, DriverError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.commands
            .send(ArtifactReadCommand {
                project_id: project_id.to_owned(),
                handle_id: handle_id.to_owned(),
                start,
                end,
                reply: reply_tx,
            })
            .await
            .map_err(|_| DriverError::JoinFailed)?;
        reply_rx.await.map_err(|_| DriverError::JoinFailed)?
    }
    /// Subscribe to view updates. The channel is latest-only and
    /// bounded; slow observers coalesce rather than queue.
    pub fn subscribe_views(&self) -> watch::Receiver<DriverSnapshotView> {
        self.snapshots.clone()
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Release the daemon subscription and join the event loop,
    /// retaining the canonical cursor and last snapshot for resume.
    /// The old subscription id is dropped and never reused.
    pub async fn stop(mut self) -> Result<StoppedDriver, DriverError> {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        let task = self.task.take().ok_or(DriverError::JoinFailed)?;
        let consumer = task.await.map_err(|_| DriverError::JoinFailed)?;
        Ok(StoppedDriver {
            session_id: self.session_id,
            generation: self.generation,
            consumer,
        })
    }
}

/// A stopped session projection: subscription released, cursor and last
/// snapshot retained for an authoritative resume on a new generation.
pub struct StoppedDriver {
    session_id: String,
    generation: u64,
    consumer: HeadlessProjectionConsumer,
}

impl StoppedDriver {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The retained canonical cursor (`None` when the driver never
    /// attached). Resume replays from here; the daemon stays the replay
    /// authority.
    pub fn cursor(&self) -> Option<ProjectionCursor> {
        self.consumer.cursor().cloned()
    }

    pub fn cursor_seq(&self) -> Option<u64> {
        self.consumer.cursor().map(|cursor| cursor.event_seq)
    }

    /// The last installed canonical snapshot, if any.
    pub fn last_snapshot(&self) -> Option<&SessionProjectionSnapshot> {
        self.consumer.snapshot()
    }

    /// The daemon subscription is always released on stop and never
    /// carried into a resume.
    pub fn subscription_released(&self) -> bool {
        self.consumer.subscription_id().is_none()
    }
}
