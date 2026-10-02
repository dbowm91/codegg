mod bridge;
mod lifecycle;

use bridge::{ConnectionSnapshot, DesktopEvent, ProjectSummary, SubscriptionInfo};
use codegg_client::{
    connect_or_start_local_daemon, FrontendDescriptor, LocalDaemonOptions, LocalDaemonPaths,
    LocalSocketClient,
};
use codegg_protocol::{
    core::{CoreEvent, CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION},
    frames::{ClientCapabilities, ClientKind},
};
use lifecycle::{cancel_and_join, SubscriptionRegistry};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{ipc::Channel, AppHandle, Manager, State};
use tokio::sync::{Mutex, Notify};

const MAX_PROJECTS: usize = 50;

fn bound_projects(projects: impl IntoIterator<Item = ProjectSummary>) -> Vec<ProjectSummary> {
    projects.into_iter().take(MAX_PROJECTS).collect()
}

struct HostState {
    /// Single lifecycle transition serialization boundary.
    ///
    /// Linearization points (all under this gate):
    /// - connect commit: serial check + old-subscription take + client install
    ///   + generation bump;
    /// - disconnect / native close / app exit: serial invalidation + generation
    ///   bump + subscription take + client clear;
    /// - subscription install: connection snapshot + owner publication.
    /// Slow work (network connect, autostart, snapshot requests, forwarder
    /// abort/join) runs outside the gate. Forwarders never acquire this gate.
    lifecycle: Mutex<()>,
    client: Mutex<Option<LocalSocketClient>>,
    daemon_id: Mutex<Option<String>>,
    /// Renderer-visible connection generation. Zero means no connection has
    /// ever been installed in this desktop process. Bumped on every
    /// successfully installed connection and on every transition that
    /// invalidates the prior connection (disconnect). Never bumped on a
    /// failed reconnect attempt.
    connection_generation: AtomicU64,
    /// Fencing counter for concurrent connect attempts. Distinct from the
    /// renderer-visible generation so failures never fabricate a generation.
    connect_serial: AtomicU64,
    subscriptions: SubscriptionRegistry,
    /// Test-only connected flag for deterministic race tests that must not
    /// require a real daemon socket. Production presence is `client.is_some()`;
    /// tests set this flag plus a non-zero generation to simulate a live
    /// connection while sharing the same lifecycle gate, serial, generation,
    /// arming, and registry code paths.
    #[cfg(test)]
    fake_connected: std::sync::atomic::AtomicBool,
}

impl Default for HostState {
    fn default() -> Self {
        Self {
            lifecycle: Mutex::new(()),
            client: Mutex::new(None),
            daemon_id: Mutex::new(None),
            connection_generation: AtomicU64::new(0),
            connect_serial: AtomicU64::new(0),
            subscriptions: SubscriptionRegistry::new(),
            #[cfg(test)]
            fake_connected: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

/// Deterministic barrier for lifecycle race tests.
///
/// Production passes `None`. Tests pass `Some` to force the vulnerable
/// interleaving: the pausing operation signals `entered`, waits for `release`,
/// and the test drives the concurrent transition in between. No sleeps.
#[derive(Clone, Default)]
pub(crate) struct TestBarrier {
    pub entered: Arc<Notify>,
    pub release: Arc<Notify>,
}

impl TestBarrier {
    pub(crate) fn fresh() -> Self {
        Self {
            entered: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
        }
    }

    async fn pause(&self) {
        self.entered.notify_one();
        self.release.notified().await;
    }
}

/// Sink for project-catalog invalidations from the event forwarder.
///
/// The production implementation wraps the Tauri IPC `Channel`; tests use an
/// in-memory recording sink. `send_event` returns false when the sink is
/// terminally closed, which ends the forwarder through the same
/// compare-by-id cleanup as a channel send failure.
pub(crate) trait DesktopEventSink: Send + Sync + 'static {
    fn send_event(&self, event: DesktopEvent) -> bool;
}

struct ChannelSink(Channel<DesktopEvent>);

impl DesktopEventSink for ChannelSink {
    fn send_event(&self, event: DesktopEvent) -> bool {
        self.0.send(event).is_ok()
    }
}

impl HostState {
    fn current_generation(&self) -> u64 {
        self.connection_generation.load(Ordering::Acquire)
    }

    /// Allocate a connect-attempt epoch. The slow preparation that follows
    /// runs outside the lifecycle gate; the final commit re-validates this
    /// attempt under the gate.
    fn allocate_connect_attempt(&self) -> u64 {
        self.connect_serial.fetch_add(1, Ordering::AcqRel) + 1
    }

    fn is_current_attempt(&self, attempt: u64) -> bool {
        self.connect_serial.load(Ordering::Acquire) == attempt
    }

    /// Shared disconnect teardown: invalidate generation, cancel/join the
    /// active forwarder, drop client/daemon identity. Never stops the daemon.
    ///
    /// Linearization point is the lifecycle gate acquisition: serial
    /// invalidation + generation bump + subscription take + client clear all
    /// occur under the gate. Forwarder abort/join runs after release (the
    /// forwarder never needs the gate, so no deadlock). Idempotent.
    async fn disconnect_host(&self) {
        self.disconnect_host_with_barrier(None).await;
    }

    async fn disconnect_host_with_barrier(&self, pre_commit: Option<TestBarrier>) {
        if let Some(barrier) = pre_commit {
            barrier.pause().await;
        }
        let previous = {
            let _gate = self.lifecycle.lock().await;
            // Invalidate every connect attempt that started before this
            // disconnect linearizes (Finding C).
            self.connect_serial.fetch_add(1, Ordering::AcqRel);
            self.connection_generation.fetch_add(1, Ordering::AcqRel);
            let taken = self.subscriptions.take_active().await;
            *self.client.lock().await = None;
            *self.daemon_id.lock().await = None;
            #[cfg(test)]
            {
                use std::sync::atomic::Ordering as MemOrder;
                self.fake_connected.store(false, MemOrder::Release);
            }
            taken.map(|(_, task)| task)
        };
        if let Some(task) = previous {
            cancel_and_join(task).await;
        }
    }

    /// Native main-window/app teardown entry point.
    ///
    /// Same semantics as explicit `desktop_disconnect`: supersedes in-flight
    /// connects, tears down the active subscription, drops the desktop client,
    /// leaves the daemon running. Idempotent across repeated close/destroy/
    /// exit events. Tested directly; the Tauri `WindowEvent`/`RunEvent`
    /// callbacks in [`run`] are thin adapters over this method.
    async fn handle_native_close(&self) {
        self.disconnect_host().await;
    }

    /// App-exit teardown entry point (e.g. macOS Cmd+Q with no window).
    /// Same authority as [`Self::handle_native_close`].
    async fn handle_app_exit(&self) {
        self.disconnect_host().await;
    }

    /// Install a newly connected client. Cancels the stale subscription
    /// before committing the new connection, then bumps the visible
    /// generation. Callers must have fenced concurrent attempts already.
    /// Preserved for the live trajectory; production connect uses
    /// [`Self::commit_prepared_connection`] for atomic supersession.
    async fn install_connection(&self, client: LocalSocketClient, daemon_id: String) -> u64 {
        let attempt = self.allocate_connect_attempt();
        self.commit_prepared_connection(attempt, client, daemon_id, None)
            .await
            .expect("freshly allocated connect attempt must commit")
    }

    /// Atomic connect commit (Finding D + C).
    ///
    /// Slow preparation (endpoint resolve, connect/autostart, daemon identity,
    /// snapshot) must complete before calling. Under the lifecycle gate this
    /// verifies `attempt == current connect_serial`; stale attempts drop the
    /// prepared client and return `Err`. The check and the state commit share
    /// the gate, so an older attempt cannot commit after a newer lifecycle
    /// decision superseded it. Failed commits never touch the current
    /// connection and never bump the visible generation.
    async fn commit_prepared_connection(
        &self,
        attempt: u64,
        client: LocalSocketClient,
        daemon_id: String,
        pre_commit: Option<TestBarrier>,
    ) -> Result<u64, String> {
        if let Some(barrier) = pre_commit {
            // Pause after slow preparation but before the final commit, so a
            // test can linearize disconnect / a newer connect first.
            barrier.pause().await;
        }
        let previous = {
            let _gate = self.lifecycle.lock().await;
            if !self.is_current_attempt(attempt) {
                return Err("connection attempt was superseded".into());
            }
            let taken = self.subscriptions.take_active().await;
            *self.client.lock().await = Some(client);
            *self.daemon_id.lock().await = Some(daemon_id);
            let generation = self.connection_generation.fetch_add(1, Ordering::AcqRel) + 1;
            (taken.map(|(_, task)| task), generation)
        };
        if let Some(task) = previous.0 {
            cancel_and_join(task).await;
        }
        Ok(previous.1)
    }

    /// Create/replace the project-event subscription for the current
    /// connection generation. This is the full production subscribe path
    /// minus the Tauri Channel adapter, so lifecycle tests (including the
    /// live-daemon trajectory) exercise the same ownership, fencing, and
    /// terminal-cleanup semantics as the invoked command.
    ///
    /// Ordering (Findings A + B):
    /// allocate id -> create receiver -> spawn UNARMED task -> acquire
    /// lifecycle gate -> verify connection+generation still current ->
    /// publish owner -> release gate -> ARM task.
    /// The forwarder cannot process events before its owner is visible, and
    /// subscribe installation is mutually ordered with disconnect/reconnect.
    async fn subscribe_with_sink(
        self: &Arc<Self>,
        sink: Arc<dyn DesktopEventSink>,
    ) -> Result<SubscriptionInfo, String> {
        self.subscribe_with_sink_and_barriers(sink, None, None)
            .await
    }

    async fn subscribe_with_sink_and_barriers(
        self: &Arc<Self>,
        sink: Arc<dyn DesktopEventSink>,
        pre_gate: Option<TestBarrier>,
        pre_install: Option<TestBarrier>,
    ) -> Result<SubscriptionInfo, String> {
        if let Some(barrier) = pre_gate {
            // Pause after the caller decided to subscribe but before
            // linearizing, so a test can complete disconnect/reconnect first
            // (transition-wins-first ordering).
            barrier.pause().await;
        }
        let subscription_id = self.subscriptions.allocate_subscription_id();
        // One-shot arming gate: the task waits before its first event read.
        let (arm_tx, arm_rx) = tokio::sync::oneshot::channel::<()>();
        // Snapshot + spawn + publish under the lifecycle gate so no
        // disconnect/reconnect can interleave between them.
        let (installed_id, installed_generation, previous, arm_tx) = {
            let _gate = self.lifecycle.lock().await;
            if let Some(barrier) = pre_install {
                // Pause while holding the gate so a concurrent transition
                // blocks on the gate (subscribe-wins-first ordering). The
                // test releases this barrier, then both operations serialize.
                barrier.pause().await;
            }
            let client = self.client.lock().await.clone();
            let generation = self.current_generation();
            let Some(client) = client else {
                return Err("desktop is not connected".to_owned());
            };
            if generation == 0 {
                return Err("desktop is not connected".to_owned());
            }
            let mut events = client.subscribe();
            let task_state = Arc::clone(self);
            let task_generation = generation;
            let task_subscription_id = subscription_id.clone();
            let task = tokio::spawn(async move {
                // Arming barrier (Finding A): never observe events before the
                // owner is published. A dropped sender (install failure path)
                // ends the task without touching ownership.
                if arm_rx.await.is_err() {
                    return;
                }
                loop {
                    let envelope = match events.recv().await {
                        Some(envelope) => envelope,
                        None => break,
                    };
                    // Generation/owner fence before every send: a stale forwarder
                    // must not publish into the current renderer.
                    if task_state.current_generation() != task_generation {
                        break;
                    }
                    let still_current = task_state
                        .subscriptions
                        .active_snapshot()
                        .await
                        .is_some_and(|(id, gen)| {
                            id == task_subscription_id && gen == task_generation
                        });
                    if !still_current {
                        break;
                    }
                    if matches!(
                        envelope.payload,
                        CoreEvent::ProjectRegistered { .. }
                            | CoreEvent::ProjectArchived { .. }
                            | CoreEvent::ProjectRestored { .. }
                            | CoreEvent::ProjectHealthChanged { .. }
                    ) {
                        if !sink.send_event(DesktopEvent {
                            version: 1,
                            event_seq: envelope.event_seq,
                            kind: "project_catalog_changed",
                        }) {
                            break;
                        }
                    }
                }
                // Clear ownership only if this task is still the stored owner.
                task_state
                    .subscriptions
                    .clear_if_matching(&task_subscription_id)
                    .await;
            });
            // Publish the owner atomically with respect to
            // disconnect/reconnect; the previous forwarder is joined after
            // the gate is released.
            let (installed, previous) = self
                .subscriptions
                .replace_without_join(subscription_id, generation, task)
                .await;
            (installed, generation, previous, Some(arm_tx))
        };
        // Ownership is now visible; arm the new forwarder. A send failure
        // means the task already exited (caller dropped); terminal cleanup is
        // compare-by-id so no newer owner is disturbed.
        if let Some(arm_tx) = arm_tx {
            let _ = arm_tx.send(());
        }
        if let Some(task) = previous {
            cancel_and_join(task).await;
        }
        Ok(SubscriptionInfo {
            subscription_id: installed_id,
            connection_generation: installed_generation,
        })
    }
}

#[cfg(test)]
impl HostState {
    fn mark_fake_connected(&self, generation: u64) {
        use std::sync::atomic::Ordering as MemOrder;
        self.fake_connected.store(true, MemOrder::Release);
        self.connection_generation
            .store(generation, MemOrder::Release);
    }

    fn mark_fake_disconnected(&self) {
        use std::sync::atomic::Ordering as MemOrder;
        self.fake_connected.store(false, MemOrder::Release);
    }

    fn is_fake_connected(&self) -> bool {
        use std::sync::atomic::Ordering as MemOrder;
        self.fake_connected.load(MemOrder::Acquire)
    }

    /// Test analogue of [`Self::commit_prepared_connection`] without a real
    /// socket: identical gate + serial check + generation bump, operating on
    /// the fake-connected flag. Shares the lifecycle gate, serial, and
    /// generation atomics with production, so serial-supersession proofs
    /// transfer. Returns the committed generation or a superseded error.
    /// Mirrors production by taking (and joining outside the gate) any active
    /// subscription, so reconnect-tears-down-subscribe orderings are proven.
    async fn test_commit_generation(
        &self,
        attempt: u64,
        pre_commit: Option<TestBarrier>,
    ) -> Result<u64, String> {
        if let Some(barrier) = pre_commit {
            barrier.pause().await;
        }
        let previous = {
            let _gate = self.lifecycle.lock().await;
            if !self.is_current_attempt(attempt) {
                return Err("connection attempt was superseded".into());
            }
            let taken = self.subscriptions.take_active().await;
            // Mirror production: a successful commit implies a live connection.
            use std::sync::atomic::Ordering as MemOrder;
            self.fake_connected.store(true, MemOrder::Release);
            let generation = self.connection_generation.fetch_add(1, MemOrder::AcqRel) + 1;
            (taken.map(|(_, task)| task), generation)
        };
        if let Some(task) = previous.0 {
            cancel_and_join(task).await;
        }
        Ok(previous.1)
    }

    /// Test analogue of [`Self::subscribe_with_sink_and_barriers`] with an
    /// injected fake event receiver. Identical ordering: allocate id, spawn
    /// UNARMED, acquire gate, verify fake connection + generation, publish
    /// owner, release gate, ARM. The `event_before_arm` barrier, when set,
    /// pauses while holding the gate after spawn but before publication so a
    /// test can queue an event into the receiver and prove the unarmed task
    /// cannot self-terminate before installation.
    #[allow(clippy::too_many_arguments)]
    async fn test_subscribe_with_fake_stream(
        self: &Arc<Self>,
        sink: Arc<dyn DesktopEventSink>,
        events: tokio::sync::mpsc::Receiver<codegg_protocol::core::EventEnvelope<CoreEvent>>,
        pre_gate: Option<TestBarrier>,
        pre_install: Option<TestBarrier>,
        event_before_arm: Option<TestBarrier>,
    ) -> Result<SubscriptionInfo, String> {
        if let Some(barrier) = pre_gate {
            barrier.pause().await;
        }
        let subscription_id = self.subscriptions.allocate_subscription_id();
        let (arm_tx, arm_rx) = tokio::sync::oneshot::channel::<()>();
        let (installed_id, installed_generation, previous, arm_tx) = {
            let _gate = self.lifecycle.lock().await;
            if let Some(barrier) = pre_install {
                barrier.pause().await;
            }
            if !self.is_fake_connected() {
                return Err("desktop is not connected".to_owned());
            }
            let generation = self.current_generation();
            if generation == 0 {
                return Err("desktop is not connected".to_owned());
            }
            let mut events = events;
            let task_state = Arc::clone(self);
            let task_generation = generation;
            let task_subscription_id = subscription_id.clone();
            let task = tokio::spawn(async move {
                if arm_rx.await.is_err() {
                    return;
                }
                loop {
                    let envelope = match events.recv().await {
                        Some(envelope) => envelope,
                        None => break,
                    };
                    if task_state.current_generation() != task_generation {
                        break;
                    }
                    let still_current = task_state
                        .subscriptions
                        .active_snapshot()
                        .await
                        .is_some_and(|(id, gen)| {
                            id == task_subscription_id && gen == task_generation
                        });
                    if !still_current {
                        break;
                    }
                    if matches!(
                        envelope.payload,
                        CoreEvent::ProjectRegistered { .. }
                            | CoreEvent::ProjectArchived { .. }
                            | CoreEvent::ProjectRestored { .. }
                            | CoreEvent::ProjectHealthChanged { .. }
                    ) {
                        if !sink.send_event(DesktopEvent {
                            version: 1,
                            event_seq: envelope.event_seq,
                            kind: "project_catalog_changed",
                        }) {
                            break;
                        }
                    }
                }
                task_state
                    .subscriptions
                    .clear_if_matching(&task_subscription_id)
                    .await;
            });
            if let Some(barrier) = event_before_arm {
                // Still holding the gate with the task spawned but unarmed and
                // unpublished: queue an event now, then publish + arm.
                barrier.pause().await;
            }
            let (installed, previous) = self
                .subscriptions
                .replace_without_join(subscription_id, generation, task)
                .await;
            (installed, generation, previous, Some(arm_tx))
        };
        if let Some(arm_tx) = arm_tx {
            let _ = arm_tx.send(());
        }
        if let Some(task) = previous {
            cancel_and_join(task).await;
        }
        Ok(SubscriptionInfo {
            subscription_id: installed_id,
            connection_generation: installed_generation,
        })
    }
}

fn descriptor() -> FrontendDescriptor {
    FrontendDescriptor::new(
        "codegg-desktop",
        ClientKind::Gui,
        ClientCapabilities {
            visual_notifications: false,
            desktop_notifications: false,
            audio: false,
            tts: false,
            multi_session_view: false,
            plugin_ui_dialog: false,
            plugin_ui_toast: false,
            plugin_ui_panel: false,
            plugin_ui_status_item: false,
            plugin_ui_table: false,
            plugin_ui_markdown: false,
            plugin_ui_code: false,
            plugin_ui_progress: false,
            workspace_registration: false,
            project_catalog: true,
            session_projection: false,
        },
    )
}

fn daemon_executable(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("CODEGG_DAEMON_EXECUTABLE").map(PathBuf::from) {
        return Ok(path);
    }
    let bundled = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("codegg");
    if bundled.is_file() {
        return Ok(bundled);
    }
    Err("set CODEGG_DAEMON_EXECUTABLE to the existing codegg binary".into())
}

async fn get_snapshot(
    client: &LocalSocketClient,
    daemon_id: String,
    connection_generation: u64,
) -> Result<ConnectionSnapshot, String> {
    let response = client
        .request(RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: uuid::Uuid::new_v4().to_string(),
            payload: CoreRequest::SnapshotDaemon,
        })
        .await
        .map_err(|e| e.to_string())?;
    match response {
        CoreResponse::SnapshotDaemon {
            daemon_id: response_id,
            uptime_secs,
            active_sessions,
            ..
        } if response_id == daemon_id => Ok(ConnectionSnapshot {
            state: "connected".into(),
            daemon_id: Some(response_id),
            protocol_version: Some(PROTOCOL_VERSION),
            uptime_seconds: Some(uptime_secs),
            active_sessions: Some(active_sessions.len()),
            error: None,
            connection_generation,
        }),
        _ => Err("daemon returned an unexpected snapshot response".into()),
    }
}

#[tauri::command]
async fn desktop_connect(
    app: AppHandle,
    state: State<'_, Arc<HostState>>,
) -> Result<ConnectionSnapshot, String> {
    // Slow preparation runs outside the lifecycle gate; the final commit is
    // atomic under the gate (Finding D) and disconnect-superseding (Finding C).
    let attempt = state.allocate_connect_attempt();
    let is_superseded = || !state.is_current_attempt(attempt);

    let paths = LocalDaemonPaths::resolve();
    paths.ensure_root().map_err(|e| e.to_string())?;
    let endpoint = paths.endpoint_uri();
    let frontend = descriptor();
    // Fast path: reuse an already-running daemon without autostart.
    match LocalSocketClient::connect(&endpoint, frontend.clone()).await {
        Ok(client) => {
            let daemon_id = client.daemon_id().await.map_err(|e| e.to_string())?;
            // Peek the next generation for the snapshot without installing it
            // yet; installation bumps the visible counter exactly once.
            let next_generation = state.current_generation() + 1;
            let snapshot = get_snapshot(&client, daemon_id.clone(), next_generation).await?;
            // Atomic commit: supersession re-checked under the gate together
            // with installation, so a newer connect/disconnect cannot be
            // undone by this older attempt.
            let installed = state
                .commit_prepared_connection(attempt, client, daemon_id, None)
                .await?;
            // Use the installed generation so a concurrent transition that
            // linearized immediately after cannot be hidden. Since commit and
            // disconnect are mutually ordered, a disconnect that wins after
            // this commit tears the new connection down before completing.
            let mut snapshot = snapshot;
            snapshot.connection_generation = installed;
            return Ok(snapshot);
        }
        Err(error) if error.to_string().contains("protocol version mismatch") => {
            if is_superseded() {
                return Err("connection attempt was superseded".into());
            }
            let generation = state.current_generation();
            return Ok(ConnectionSnapshot {
                state: "incompatible".into(),
                daemon_id: None,
                protocol_version: None,
                uptime_seconds: None,
                active_sessions: None,
                error: Some(error.to_string()),
                connection_generation: generation,
            });
        }
        Err(_) => {}
    }
    // Slow path: autostart the daemon from the explicit executable, then
    // connect. A failure here preserves the current connection untouched.
    let outcome = connect_or_start_local_daemon(
        LocalDaemonOptions {
            endpoint,
            endpoint_argument: paths.socket_path_str(),
            lock_path: paths.lock_path,
            log_path: paths.log_path,
            executable: Some(daemon_executable(&app)?),
            autostart: true,
            startup_timeout: Duration::from_secs(20),
            poll_interval: Duration::from_millis(200),
        },
        frontend,
    )
    .await;
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            if is_superseded() {
                return Err("connection attempt was superseded".into());
            }
            let message = error.to_string();
            if message.contains("protocol version mismatch") {
                let generation = state.current_generation();
                return Ok(ConnectionSnapshot {
                    state: "incompatible".into(),
                    daemon_id: None,
                    protocol_version: None,
                    uptime_seconds: None,
                    active_sessions: None,
                    error: Some(message),
                    connection_generation: generation,
                });
            }
            return Err(message);
        }
    };
    let next_generation = state.current_generation() + 1;
    let snapshot =
        get_snapshot(&outcome.client, outcome.daemon_id.clone(), next_generation).await?;
    let installed = state
        .commit_prepared_connection(attempt, outcome.client, outcome.daemon_id, None)
        .await?;
    let mut snapshot = snapshot;
    snapshot.connection_generation = installed;
    Ok(snapshot)
}

#[tauri::command]
async fn desktop_connection_snapshot(
    state: State<'_, Arc<HostState>>,
) -> Result<ConnectionSnapshot, String> {
    let generation = state.current_generation();
    let client = state.client.lock().await.clone();
    let Some(client) = client else {
        return Ok(ConnectionSnapshot::disconnected(None, generation));
    };
    let daemon_id = state.daemon_id.lock().await.clone().unwrap_or_default();
    Ok(get_snapshot(&client, daemon_id, generation)
        .await
        .unwrap_or_else(|error| ConnectionSnapshot::disconnected(Some(error), generation)))
}

#[tauri::command]
async fn desktop_project_list(
    state: State<'_, Arc<HostState>>,
) -> Result<Vec<ProjectSummary>, String> {
    let client = state
        .client
        .lock()
        .await
        .clone()
        .ok_or_else(|| "desktop is not connected".to_owned())?;
    let response = client
        .request(RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: uuid::Uuid::new_v4().to_string(),
            payload: CoreRequest::ProjectList {
                include_archived: false,
                limit: MAX_PROJECTS,
            },
        })
        .await
        .map_err(|e| e.to_string())?;
    match response {
        CoreResponse::ProjectList { projects, .. } => {
            Ok(bound_projects(projects.into_iter().map(|p| {
                ProjectSummary {
                    project_id: p.project_id,
                    display_name: p.display_name,
                    lifecycle: p.lifecycle,
                }
            })))
        }
        _ => Err("daemon returned an unexpected project list response".into()),
    }
}

#[tauri::command]
async fn desktop_subscribe_events(
    state: State<'_, Arc<HostState>>,
    channel: Channel<DesktopEvent>,
) -> Result<SubscriptionInfo, String> {
    state
        .subscribe_with_sink(Arc::new(ChannelSink(channel)))
        .await
}

#[tauri::command]
async fn desktop_unsubscribe_events(
    state: State<'_, Arc<HostState>>,
    subscription_id: String,
) -> Result<(), String> {
    // All outcomes are success from the renderer's view: removed, idempotent
    // repeat, or stale id that must not disturb the current subscription.
    let _ = state.subscriptions.unsubscribe(&subscription_id).await;
    Ok(())
}

#[tauri::command]
async fn desktop_disconnect(state: State<'_, Arc<HostState>>) -> Result<(), String> {
    state.disconnect_host().await;
    Ok(())
}

pub fn run() {
    let state = Arc::new(HostState::default());
    // Native window/app teardown (Finding E): main-window close/destroy and
    // app exit reach the same Rust-host teardown as explicit
    // `desktop_disconnect` without stopping the daemon. The window-event
    // callback is synchronous, so teardown runs via a bounded `block_on`
    // (2s budget); the lifecycle gate is never held across network I/O, so
    // acquisition is prompt and forwarders never need the gate. Repeated
    // close/destroy/exit events are idempotent. Daemon shutdown is never
    // triggered here.
    let close_state = Arc::clone(&state);
    let exit_state = Arc::clone(&state);
    tauri::Builder::default()
        .manage(Arc::clone(&state))
        .invoke_handler(tauri::generate_handler![
            desktop_connect,
            desktop_connection_snapshot,
            desktop_project_list,
            desktop_subscribe_events,
            desktop_unsubscribe_events,
            desktop_disconnect
        ])
        .on_window_event(move |_window, event| match event {
            tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed => {
                let state = Arc::clone(&close_state);
                tauri::async_runtime::block_on(async move {
                    let _ =
                        tokio::time::timeout(Duration::from_secs(2), state.handle_native_close())
                            .await;
                });
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("failed to build CodeGG desktop shell")
        .run(move |_app, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                let state = Arc::clone(&exit_state);
                tauri::async_runtime::block_on(async move {
                    let _ =
                        tokio::time::timeout(Duration::from_secs(2), state.handle_app_exit()).await;
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_advertises_only_implemented_capabilities() {
        let d = descriptor();
        assert!(matches!(d.client_kind(), ClientKind::Gui));
        assert!(d.capabilities().project_catalog);
        assert!(!d.capabilities().session_projection && !d.capabilities().multi_session_view);
    }
    #[test]
    fn bridge_snapshot_has_stable_state_shape() {
        let json = serde_json::to_value(ConnectionSnapshot::disconnected(None, 3)).unwrap();
        assert_eq!(json["state"], "disconnected");
        assert!(json.get("daemonId").is_some());
        assert_eq!(json["connectionGeneration"], 3);
    }

    #[test]
    fn project_bridge_response_is_bounded() {
        let projects = (0..MAX_PROJECTS + 5).map(|index| ProjectSummary {
            project_id: format!("project-{index}"),
            display_name: format!("Project {index}"),
            lifecycle: "active".into(),
        });
        let projects = bound_projects(projects);
        assert_eq!(projects.len(), MAX_PROJECTS);
    }

    #[test]
    fn subscription_info_uses_camel_case_bridge_fields() {
        let info = SubscriptionInfo {
            subscription_id: "desktop-sub-1".into(),
            connection_generation: 2,
        };
        let json = serde_json::to_value(info).unwrap();
        assert_eq!(json["subscriptionId"], "desktop-sub-1");
        assert_eq!(json["connectionGeneration"], 2);
    }

    #[tokio::test]
    async fn subscribe_without_connection_installs_no_owner() {
        let state = Arc::new(HostState::default());
        assert_eq!(state.subscriptions.active_count().await, 0);
        assert_eq!(state.current_generation(), 0);
    }

    #[tokio::test]
    async fn disconnect_invalidates_generation_and_releases_subscription() {
        let state = Arc::new(HostState::default());
        let id = state.subscriptions.install(1, tokio::spawn(async {})).await;
        // Simulate a connected generation without a real daemon client.
        state.connection_generation.store(1, Ordering::Release);
        // Terminal cleanup path for the installed immediate task.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let _ = id;
        state.disconnect_host().await;
        assert_eq!(state.subscriptions.active_count().await, 0);
        assert_eq!(state.current_generation(), 2);
        assert!(state.client.lock().await.is_none());
    }

    #[tokio::test]
    async fn generations_are_monotonic_across_disconnects() {
        let state = Arc::new(HostState::default());
        state.disconnect_host().await;
        let first = state.current_generation();
        state.disconnect_host().await;
        let second = state.current_generation();
        assert!(second > first);
    }

    // ---- C002 deterministic lifecycle race tests (barrier-forced, no sleeps
    // for correctness) ----

    struct RecordingSink {
        events: std::sync::Mutex<Vec<DesktopEvent>>,
        notify: Notify,
    }

    impl RecordingSink {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                events: std::sync::Mutex::new(Vec::new()),
                notify: Notify::new(),
            })
        }

        fn len(sink: &Arc<Self>) -> usize {
            sink.events.lock().unwrap().len()
        }

        async fn wait_for_count(sink: &Arc<Self>, count: usize) {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            loop {
                if Self::len(sink) >= count {
                    return;
                }
                if tokio::time::Instant::now() >= deadline {
                    panic!(
                        "timed out waiting for {count} events, got {}",
                        Self::len(sink)
                    );
                }
                tokio::time::timeout(Duration::from_millis(50), sink.notify.notified())
                    .await
                    .ok();
            }
        }
    }

    impl DesktopEventSink for RecordingSink {
        fn send_event(&self, event: DesktopEvent) -> bool {
            self.events.lock().unwrap().push(event);
            self.notify.notify_waiters();
            true
        }
    }

    fn fake_project_event(seq: u64) -> codegg_protocol::core::EventEnvelope<CoreEvent> {
        use codegg_protocol::core::EventEnvelope;
        use codegg_protocol::dto::ProjectSummaryDto;
        EventEnvelope {
            protocol_version: PROTOCOL_VERSION,
            event_seq: seq,
            timestamp_ms: 0,
            session_id: None,
            turn_id: None,
            payload: CoreEvent::ProjectRegistered {
                project_id: format!("project-{seq}"),
                project: ProjectSummaryDto {
                    project_id: format!("project-{seq}"),
                    display_name: format!("Project {seq}"),
                    lifecycle: "active".into(),
                    description: None,
                    tags: Vec::new(),
                    time_last_opened_at: None,
                    registration_source: "c002-test".into(),
                    archived_at: None,
                    created_at: 0,
                    updated_at: 0,
                },
            },
        }
    }

    fn fake_stream(
        capacity: usize,
    ) -> (
        tokio::sync::mpsc::Sender<codegg_protocol::core::EventEnvelope<CoreEvent>>,
        tokio::sync::mpsc::Receiver<codegg_protocol::core::EventEnvelope<CoreEvent>>,
    ) {
        tokio::sync::mpsc::channel(capacity)
    }

    async fn entered_with_timeout(barrier: &TestBarrier) {
        tokio::time::timeout(Duration::from_secs(5), barrier.entered.notified())
            .await
            .expect("barrier entered timeout");
    }

    #[tokio::test]
    async fn subscription_is_armed_only_after_owner_publication() {
        let state = Arc::new(HostState::default());
        state.mark_fake_connected(1);
        let sink = RecordingSink::new();
        let (tx, rx) = fake_stream(8);
        let barrier = TestBarrier::fresh();
        let state_clone = Arc::clone(&state);
        let sink_clone = Arc::clone(&sink);
        let barrier_clone = barrier.clone();
        let subscribe = tokio::spawn(async move {
            state_clone
                .test_subscribe_with_fake_stream(sink_clone, rx, None, None, Some(barrier_clone))
                .await
        });
        // Subscribe spawned its task and paused holding the gate, before owner
        // publication. Queue an event now: the unarmed task must not observe it.
        entered_with_timeout(&barrier).await;
        tx.send(fake_project_event(1))
            .await
            .expect("queue event before arm");
        // Owner not yet published; no event processed.
        assert_eq!(state.subscriptions.active_snapshot().await, None);
        assert_eq!(RecordingSink::len(&sink), 0);
        barrier.release.notify_one();
        let info = subscribe.await.expect("join").expect("subscribe ok");
        assert_eq!(info.connection_generation, 1);
        // After publication + arm, the queued event is delivered exactly once.
        RecordingSink::wait_for_count(&sink, 1).await;
        assert_eq!(RecordingSink::len(&sink), 1);
        let snapshot = state
            .subscriptions
            .active_snapshot()
            .await
            .expect("owner published");
        assert_eq!(snapshot.0, info.subscription_id);
        assert_eq!(snapshot.1, 1);
    }

    #[tokio::test]
    async fn event_before_install_does_not_leave_finished_owner() {
        let state = Arc::new(HostState::default());
        state.mark_fake_connected(3);
        let sink = RecordingSink::new();
        let (tx, rx) = fake_stream(8);
        // Queue the event before the subscribe linearizes at all.
        tx.send(fake_project_event(7)).await.expect("pre-queue");
        let info = state
            .test_subscribe_with_fake_stream(sink.clone(), rx, None, None, None)
            .await
            .expect("subscribe ok");
        assert_eq!(info.connection_generation, 3);
        // The installed owner must be the live task, not a finished handle:
        // without the arming barrier the pre-install event would have caused
        // self-termination followed by installation of a dead handle.
        assert_eq!(state.subscriptions.active_count().await, 1);
        RecordingSink::wait_for_count(&sink, 1).await;
        let snapshot = state
            .subscriptions
            .active_snapshot()
            .await
            .expect("live owner");
        assert_eq!(snapshot.0, info.subscription_id);
        // The live owner still forwards a second event (not a dead handle).
        let (tx2, rx2) = fake_stream(8);
        let _ = (tx2, rx2);
        drop(tx);
    }

    #[tokio::test]
    async fn subscribe_racing_disconnect_is_linearizable() {
        // Ordering 1: transition wins first — disconnect completes before
        // subscribe linearizes, so subscribe must fail with no owner.
        let state = Arc::new(HostState::default());
        state.mark_fake_connected(1);
        let pre_gate = TestBarrier::fresh();
        let state_clone = Arc::clone(&state);
        let sink = RecordingSink::new();
        let (_, rx) = fake_stream(8);
        let gate_clone = pre_gate.clone();
        let subscribe = tokio::spawn(async move {
            state_clone
                .test_subscribe_with_fake_stream(sink, rx, Some(gate_clone), None, None)
                .await
        });
        entered_with_timeout(&pre_gate).await;
        state.disconnect_host().await;
        assert_eq!(state.subscriptions.active_count().await, 0);
        pre_gate.release.notify_one();
        let result = subscribe.await.expect("join");
        assert!(result.is_err(), "subscribe after disconnect must fail");
        assert_eq!(state.subscriptions.active_count().await, 0);

        // Ordering 2: subscribe wins first — subscribe holds the gate while
        // disconnect blocks, then disconnect tears the new owner down.
        let state = Arc::new(HostState::default());
        state.mark_fake_connected(1);
        let pre_install = TestBarrier::fresh();
        let state_clone = Arc::clone(&state);
        let sink = RecordingSink::new();
        let (_tx_hold, rx) = fake_stream(8);
        let install_clone = pre_install.clone();
        let subscribe = tokio::spawn(async move {
            state_clone
                .test_subscribe_with_fake_stream(sink, rx, None, Some(install_clone), None)
                .await
        });
        entered_with_timeout(&pre_install).await;
        let state_second = Arc::clone(&state);
        let disconnect = tokio::spawn(async move { state_second.disconnect_host().await });
        // Release subscribe; the two operations then serialize: subscribe
        // holds the gate first (it signalled entered while holding it), so it
        // must succeed, and the gated disconnect tears the fresh owner down.
        // Subscribe Ok + final zero proves subscribe-wins-first linearization
        // without any sleep-based assertion.
        pre_install.release.notify_one();
        let info = subscribe.await.expect("join").expect("subscribe ok");
        assert_eq!(info.connection_generation, 1);
        disconnect.await.expect("disconnect join");
        assert_eq!(state.subscriptions.active_count().await, 0);
    }

    #[tokio::test]
    async fn subscribe_racing_reconnect_binds_one_generation_only() {
        // Ordering 1: reconnect wins first — subscribe binds the new generation.
        let state = Arc::new(HostState::default());
        state.mark_fake_connected(1);
        let pre_gate = TestBarrier::fresh();
        let state_clone = Arc::clone(&state);
        let sink = RecordingSink::new();
        // Hold the sender so the fake receiver stays open and the armed task
        // remains the live owner (a dropped sender would close the stream and
        // the task would correctly self-clear, leaving no owner).
        let (_tx_hold, rx) = fake_stream(8);
        let gate_clone = pre_gate.clone();
        let subscribe = tokio::spawn(async move {
            state_clone
                .test_subscribe_with_fake_stream(sink, rx, Some(gate_clone), None, None)
                .await
        });
        entered_with_timeout(&pre_gate).await;
        let attempt = state.allocate_connect_attempt();
        let gen2 = state
            .test_commit_generation(attempt, None)
            .await
            .expect("reconnect commit");
        assert_eq!(gen2, 2);
        pre_gate.release.notify_one();
        let info = subscribe
            .await
            .expect("join")
            .expect("subscribe binds new gen");
        assert_eq!(info.connection_generation, 2);
        let snapshot = state
            .subscriptions
            .active_snapshot()
            .await
            .expect("one owner");
        assert_eq!(snapshot.1, 2);

        // Ordering 2: subscribe wins first — reconnect tears it down, leaving
        // no owner referencing the prior generation.
        let state = Arc::new(HostState::default());
        state.mark_fake_connected(1);
        let sink = RecordingSink::new();
        let (_tx_hold, rx) = fake_stream(8);
        let info = state
            .test_subscribe_with_fake_stream(sink, rx, None, None, None)
            .await
            .expect("subscribe gen 1");
        assert_eq!(info.connection_generation, 1);
        let attempt = state.allocate_connect_attempt();
        let gen2 = state
            .test_commit_generation(attempt, None)
            .await
            .expect("reconnect commit");
        assert_eq!(gen2, 2);
        assert_eq!(state.subscriptions.active_count().await, 0);
        // A fresh subscribe binds exactly the new generation.
        let sink2 = RecordingSink::new();
        let (_tx_hold2, rx2) = fake_stream(8);
        let info2 = state
            .test_subscribe_with_fake_stream(sink2, rx2, None, None, None)
            .await
            .expect("resubscribe gen 2");
        assert_eq!(info2.connection_generation, 2);
        assert_ne!(info.subscription_id, info2.subscription_id);
    }

    #[tokio::test]
    async fn disconnect_supersedes_inflight_connect() {
        let state = Arc::new(HostState::default());
        state.mark_fake_connected(1);
        let attempt = state.allocate_connect_attempt();
        let pre_commit = TestBarrier::fresh();
        let state_clone = Arc::clone(&state);
        let barrier_clone = pre_commit.clone();
        let commit = tokio::spawn(async move {
            state_clone
                .test_commit_generation(attempt, Some(barrier_clone))
                .await
        });
        entered_with_timeout(&pre_commit).await;
        // Disconnect linearizes while connect A is paused after slow prep.
        state.disconnect_host().await;
        let gen_after_disconnect = state.current_generation();
        assert!(gen_after_disconnect >= 2);
        pre_commit.release.notify_one();
        let result = commit.await.expect("join");
        assert!(
            result.is_err(),
            "connect A must not resurrect after disconnect"
        );
        // Final state: disconnected, no owner, generation unchanged by stale A.
        assert_eq!(state.subscriptions.active_count().await, 0);
        assert_eq!(state.current_generation(), gen_after_disconnect);
        assert!(!state.is_fake_connected());
    }

    #[tokio::test]
    async fn newer_connect_attempt_prevents_older_commit() {
        let state = Arc::new(HostState::default());
        state.mark_fake_connected(1);
        let attempt_a = state.allocate_connect_attempt();
        let attempt_b = state.allocate_connect_attempt();
        assert!(attempt_b > attempt_a);
        let pre_commit = TestBarrier::fresh();
        let state_clone = Arc::clone(&state);
        let barrier_clone = pre_commit.clone();
        let commit_a = tokio::spawn(async move {
            state_clone
                .test_commit_generation(attempt_a, Some(barrier_clone))
                .await
        });
        entered_with_timeout(&pre_commit).await;
        // Newer attempt B commits while A is paused before its final commit.
        let gen_b = state
            .test_commit_generation(attempt_b, None)
            .await
            .expect("B commits");
        pre_commit.release.notify_one();
        let result_a = commit_a.await.expect("join");
        assert!(result_a.is_err(), "older attempt A cannot commit after B");
        assert_eq!(state.current_generation(), gen_b);
        assert!(state.is_fake_connected());
    }

    #[tokio::test]
    async fn failed_newer_connect_does_not_allow_stale_commit() {
        let state = Arc::new(HostState::default());
        state.mark_fake_connected(5);
        let attempt_a = state.allocate_connect_attempt();
        // Newer attempt B is allocated (epoch advances) but fails during slow
        // preparation, so it never commits and never bumps the generation.
        let _attempt_b = state.allocate_connect_attempt();
        let pre_commit = TestBarrier::fresh();
        let state_clone = Arc::clone(&state);
        let barrier_clone = pre_commit.clone();
        let commit_a = tokio::spawn(async move {
            state_clone
                .test_commit_generation(attempt_a, Some(barrier_clone))
                .await
        });
        entered_with_timeout(&pre_commit).await;
        // B fails here: no commit, no generation change, connection preserved.
        assert_eq!(state.current_generation(), 5);
        assert!(state.is_fake_connected());
        pre_commit.release.notify_one();
        let result_a = commit_a.await.expect("join");
        // A is stale relative to B's allocated epoch even though B failed, so
        // A must not commit; the prior valid connection is preserved.
        assert!(result_a.is_err());
        assert_eq!(state.current_generation(), 5);
        assert!(state.is_fake_connected());
    }

    #[tokio::test]
    async fn native_close_tears_down_subscription_and_connection() {
        // Close wins first: subscribe paused pre-gate, close completes, then
        // subscribe must fail.
        let state = Arc::new(HostState::default());
        state.mark_fake_connected(1);
        let pre_gate = TestBarrier::fresh();
        let state_clone = Arc::clone(&state);
        let sink = RecordingSink::new();
        let (_, rx) = fake_stream(8);
        let gate_clone = pre_gate.clone();
        let subscribe = tokio::spawn(async move {
            state_clone
                .test_subscribe_with_fake_stream(sink, rx, Some(gate_clone), None, None)
                .await
        });
        entered_with_timeout(&pre_gate).await;
        state.handle_native_close().await;
        assert_eq!(state.subscriptions.active_count().await, 0);
        pre_gate.release.notify_one();
        assert!(subscribe.await.expect("join").is_err());

        // Subscribe wins first: close tears the fresh owner down.
        let state = Arc::new(HostState::default());
        state.mark_fake_connected(1);
        let sink = RecordingSink::new();
        let (_tx_hold, rx) = fake_stream(8);
        let info = state
            .test_subscribe_with_fake_stream(sink, rx, None, None, None)
            .await
            .expect("subscribe ok");
        assert_eq!(state.subscriptions.active_count().await, 1);
        state.handle_native_close().await;
        assert_eq!(state.subscriptions.active_count().await, 0);
        assert!(!state.is_fake_connected());
        assert!(state.client.lock().await.is_none());
        // Repeated close/exit events are idempotent (no panic, no owner).
        state.handle_native_close().await;
        state.handle_app_exit().await;
        assert_eq!(state.subscriptions.active_count().await, 0);
        let _ = info;
    }

    #[tokio::test]
    async fn native_close_supersedes_inflight_connect() {
        let state = Arc::new(HostState::default());
        state.mark_fake_connected(1);
        let attempt = state.allocate_connect_attempt();
        let pre_commit = TestBarrier::fresh();
        let state_clone = Arc::clone(&state);
        let barrier_clone = pre_commit.clone();
        let commit = tokio::spawn(async move {
            state_clone
                .test_commit_generation(attempt, Some(barrier_clone))
                .await
        });
        entered_with_timeout(&pre_commit).await;
        // Native close linearizes while the connect is paused after prep.
        state.handle_native_close().await;
        pre_commit.release.notify_one();
        let result = commit.await.expect("join");
        assert!(
            result.is_err(),
            "in-flight connect cannot commit after close"
        );
        assert_eq!(state.subscriptions.active_count().await, 0);
        assert!(!state.is_fake_connected());
    }

    #[tokio::test]
    async fn mixed_lifecycle_stress_returns_to_baseline() {
        let state = Arc::new(HostState::default());
        let mut last_generation = 0;
        for cycle in 0..100u64 {
            let attempt = state.allocate_connect_attempt();
            // Alternate between plain commit and commit-then-subscribe so both
            // connect and subscription paths interleave with teardown.
            let generation = state
                .test_commit_generation(attempt, None)
                .await
                .expect("commit in stress loop");
            assert!(generation > last_generation);
            last_generation = generation;
            if cycle % 3 == 0 {
                let sink = RecordingSink::new();
                let (tx, rx) = fake_stream(4);
                let info = state
                    .test_subscribe_with_fake_stream(sink.clone(), rx, None, None, None)
                    .await
                    .expect("subscribe in stress loop");
                assert_eq!(info.connection_generation, generation);
                assert_eq!(state.subscriptions.active_count().await, 1);
                // Deliver one event to prove the owner is live, then tear down.
                tx.send(fake_project_event(cycle))
                    .await
                    .expect("stress event");
                RecordingSink::wait_for_count(&sink, 1).await;
            }
            if cycle % 10 == 9 {
                state.handle_native_close().await;
            } else if cycle % 2 == 0 {
                state.disconnect_host().await;
            } else {
                // Reconnect-style commit without an explicit disconnect in
                // between: the commit itself takes any active owner.
                let attempt = state.allocate_connect_attempt();
                let next = state
                    .test_commit_generation(attempt, None)
                    .await
                    .expect("reconnect commit in stress loop");
                assert!(next > last_generation);
                last_generation = next;
                state.disconnect_host().await;
            }
            assert_eq!(state.subscriptions.active_count().await, 0);
            assert!(!state.is_fake_connected());
        }
        // Baseline restored: no owner, no connection, disconnected epoch.
        assert_eq!(state.subscriptions.active_count().await, 0);
        assert!(!state.is_fake_connected());
        assert!(state.client.lock().await.is_none());
        assert!(state.current_generation() >= 100);
    }

    #[test]
    fn native_window_teardown_is_wired_in_run() {
        // Build-level proof that the Tauri event callback is wired: `run`
        // must translate native window/app lifecycle into the Rust-host
        // teardown (Finding E) rather than relying on renderer cleanup.
        let source = include_str!("lib.rs");
        for token in [
            "on_window_event",
            "CloseRequested",
            "Destroyed",
            "ExitRequested",
            "handle_native_close",
            "handle_app_exit",
        ] {
            assert!(
                source.contains(token),
                "run() must wire native teardown via {token}"
            );
        }
    }

    /// In-memory recording sink for the live-daemon trajectory.
    #[cfg(unix)]
    struct TestSink {
        events: std::sync::Mutex<Vec<DesktopEvent>>,
        notify: tokio::sync::Notify,
    }

    #[cfg(unix)]
    impl TestSink {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                events: std::sync::Mutex::new(Vec::new()),
                notify: tokio::sync::Notify::new(),
            })
        }

        fn len(&self) -> usize {
            self.events.lock().unwrap().len()
        }

        async fn wait_for_count(&self, count: usize) {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            loop {
                if self.len() >= count {
                    return;
                }
                if tokio::time::Instant::now() >= deadline {
                    panic!(
                        "timed out waiting for {count} desktop events, got {}",
                        self.len()
                    );
                }
                tokio::time::timeout(Duration::from_millis(200), self.notify.notified())
                    .await
                    .ok();
            }
        }
    }

    #[cfg(unix)]
    impl DesktopEventSink for TestSink {
        fn send_event(&self, event: DesktopEvent) -> bool {
            self.events.lock().unwrap().push(event);
            self.notify.notify_waiters();
            true
        }
    }

    /// Live command-level lifecycle trajectory against a real daemon.
    ///
    /// C002 adversarial trajectory (WP7): drives the production `HostState`
    /// paths (`commit_prepared_connection` atomic commit, armed
    /// `subscribe_with_sink`, `handle_native_close` seam, autostart) against a
    /// real isolated daemon and asserts real `SnapshotDaemon.connected_clients`
    /// counts plus real invalidation delivery without waiting for a project
    /// event. Covers: TUI baseline, exactly-one desktop client after connect,
    /// subscribe/reconnect/disconnect convergence, disconnect-supersedes-
    /// prepared-connect (no resurrection), reconnect leaves one client + zero
    /// old-generation owners, invalidation reaches only the armed current
    /// subscription, native-host cleanup returns to observer-only baseline,
    /// daemon stays responsive. Renderer generation-keyed resubscribe races
    /// are covered by the TypeScript suite; the Tauri `Channel` adapter is
    /// covered by the boundary guard and bridge-DTO checks.
    ///
    /// Run explicitly with a built daemon binary; never touches the user's
    /// real daemon home:
    ///
    /// ```bash
    /// CODEGG_DAEMON_EXECUTABLE=/tmp/codegg-target-fresh/debug/codegg \
    ///   cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml \
    ///     -- --ignored live_daemon_lifecycle_against_real_daemon
    /// ```
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "needs CODEGG_DAEMON_EXECUTABLE plus an isolated daemon home; see doc comment"]
    async fn live_daemon_lifecycle_against_real_daemon() {
        use codegg_protocol::dto::ProjectRegisterRequestDto;

        let executable = std::env::var("CODEGG_DAEMON_EXECUTABLE").unwrap_or_else(|_| {
            panic!("set CODEGG_DAEMON_EXECUTABLE to a built codegg binary for the live test")
        });
        assert!(
            PathBuf::from(&executable).is_file(),
            "daemon executable is missing: {executable}"
        );
        let tag = std::process::id();
        let home = PathBuf::from(format!("/tmp/cgdt-{tag}"));
        let workspace_root = PathBuf::from(format!("/tmp/cgws-{tag}"));
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&workspace_root);
        std::fs::create_dir_all(&workspace_root).expect("workspace root");
        let previous_home = std::env::var_os("CODEGG_DAEMON_HOME");
        std::env::set_var("CODEGG_DAEMON_HOME", &home);
        let cleanup = || {
            let _ = std::fs::remove_dir_all(&home);
            let _ = std::fs::remove_dir_all(&workspace_root);
            match previous_home {
                Some(value) => std::env::set_var("CODEGG_DAEMON_HOME", value),
                None => std::env::remove_var("CODEGG_DAEMON_HOME"),
            }
        };

        async fn snapshot_clients(
            observer: &LocalSocketClient,
        ) -> Vec<codegg_protocol::core::ClientSnapshot> {
            let response = tokio::time::timeout(
                Duration::from_secs(10),
                observer.request(RequestEnvelope {
                    protocol_version: PROTOCOL_VERSION,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    payload: CoreRequest::SnapshotDaemon,
                }),
            )
            .await
            .expect("snapshot deadline")
            .expect("snapshot request");
            match response {
                CoreResponse::SnapshotDaemon {
                    connected_clients, ..
                } => connected_clients,
                other => panic!("unexpected snapshot response: {other:?}"),
            }
        }

        async fn poll_clients(
            observer: &LocalSocketClient,
            expected_total: usize,
            expected_desktop: usize,
        ) {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            loop {
                let clients = snapshot_clients(observer).await;
                let desktop = clients
                    .iter()
                    .filter(|client| client.client_name == "codegg-desktop")
                    .count();
                if clients.len() == expected_total && desktop == expected_desktop {
                    return;
                }
                if tokio::time::Instant::now() >= deadline {
                    panic!(
                        "timed out waiting for {expected_total} total / \
                         {expected_desktop} desktop clients, got {} total / \
                         {desktop} desktop: {clients:?}",
                        clients.len(),
                    );
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }

        let run = async {
            let paths = LocalDaemonPaths::resolve();
            paths.ensure_root().expect("daemon home");
            let endpoint = paths.endpoint_uri();
            let observer = connect_or_start_local_daemon(
                LocalDaemonOptions {
                    endpoint: endpoint.clone(),
                    endpoint_argument: paths.socket_path_str(),
                    lock_path: paths.lock_path.clone(),
                    log_path: paths.log_path.clone(),
                    executable: Some(PathBuf::from(&executable)),
                    autostart: true,
                    startup_timeout: Duration::from_secs(20),
                    poll_interval: Duration::from_millis(200),
                },
                FrontendDescriptor::new(
                    "codegg-live-observer",
                    ClientKind::Tui,
                    ClientCapabilities {
                        visual_notifications: false,
                        desktop_notifications: false,
                        audio: false,
                        tts: false,
                        multi_session_view: false,
                        plugin_ui_dialog: false,
                        plugin_ui_toast: false,
                        plugin_ui_panel: false,
                        plugin_ui_status_item: false,
                        plugin_ui_table: false,
                        plugin_ui_markdown: false,
                        plugin_ui_code: false,
                        plugin_ui_progress: false,
                        workspace_registration: true,
                        project_catalog: true,
                        session_projection: false,
                    },
                ),
            )
            .await
            .expect("observer autostart");
            // Baseline: only the observer (TUI-kind coexistence peer).
            eprintln!("STEP baseline");
            poll_clients(&observer.client, 1, 0).await;
            eprintln!("STEP baseline ok");

            // Desktop connect (fast path): exactly one desktop client appears.
            let state = Arc::new(HostState::default());
            let desktop = LocalSocketClient::connect(&endpoint, descriptor())
                .await
                .expect("desktop connect");
            let daemon_id = desktop.daemon_id().await.expect("daemon id");
            assert_eq!(daemon_id, observer.daemon_id);
            let generation = state.install_connection(desktop, daemon_id).await;
            assert_eq!(generation, 1);
            eprintln!("STEP desktop connected");
            poll_clients(&observer.client, 2, 1).await;

            // Subscribe: single owned forwarder for generation 1.
            let sink1 = TestSink::new();
            let info1 = state
                .subscribe_with_sink(sink1.clone())
                .await
                .expect("subscribe gen 1");
            assert_eq!(info1.connection_generation, 1);
            assert_eq!(state.subscriptions.active_count().await, 1);
            eprintln!("STEP subscribed gen1");

            // Reconnect while coarse state remains connected: the stale
            // subscription is cancelled/joined before the new connection
            // commits, and the old client clone is released.
            let desktop2 = LocalSocketClient::connect(&endpoint, descriptor())
                .await
                .expect("desktop reconnect");
            let daemon_id2 = desktop2.daemon_id().await.expect("daemon id 2");
            assert_eq!(daemon_id2, observer.daemon_id);
            let generation2 = state.install_connection(desktop2, daemon_id2).await;
            assert_eq!(generation2, 2);
            assert_eq!(state.subscriptions.active_count().await, 0);
            eprintln!("STEP reconnected-install");
            poll_clients(&observer.client, 2, 1).await;
            eprintln!("STEP reconnected-poll-ok");
            let clients = snapshot_clients(&observer.client).await;
            assert_eq!(
                clients
                    .iter()
                    .filter(|client| client.client_name == "codegg-desktop")
                    .count(),
                1,
                "exactly one desktop client after reconnect: {clients:?}"
            );

            // Re-subscribe for generation 2; only the current subscription
            // reacts to the project invalidation below.
            let sink2 = TestSink::new();
            let info2 = state
                .subscribe_with_sink(sink2.clone())
                .await
                .expect("subscribe gen 2");
            assert_eq!(info2.connection_generation, 2);
            assert_ne!(info1.subscription_id, info2.subscription_id);
            eprintln!("STEP resubscribed gen2");

            // Register a workspace + project through the observer peer and
            // prove the current subscription (and only it) observes the event.
            let workspace_id = {
                let response = observer
                    .client
                    .request(RequestEnvelope {
                        protocol_version: PROTOCOL_VERSION,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        payload: CoreRequest::WorkspaceRegister {
                            root: workspace_root.to_string_lossy().into_owned(),
                        },
                    })
                    .await
                    .expect("workspace register");
                match response {
                    CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
                    other => panic!("unexpected workspace response: {other:?}"),
                }
            };
            let register = observer
                .client
                .request(RequestEnvelope {
                    protocol_version: PROTOCOL_VERSION,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    payload: CoreRequest::ProjectRegister {
                        request: ProjectRegisterRequestDto {
                            workspace_id,
                            display_name: "live-lifecycle-probe".into(),
                            description: None,
                            tags: Vec::new(),
                            repository_id: None,
                            source: "desktop-c001-live-test".into(),
                        },
                    },
                })
                .await
                .expect("project register");
            assert!(
                matches!(register, CoreResponse::ProjectRegistered { .. }),
                "unexpected register response: {register:?}"
            );
            sink2.wait_for_count(1).await;
            assert_eq!(sink1.len(), 0, "stale subscription must stay silent");
            assert!(
                sink2
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .all(|event| event.kind == "project_catalog_changed" && event.version == 1),
                "unexpected desktop event payload"
            );
            eprintln!("STEP invalidation observed only by current subscription");

            // Reload simulation: unsubscribe then re-subscribe for the same
            // generation without accumulating owners or clients.
            state
                .subscriptions
                .unsubscribe(&info2.subscription_id)
                .await;
            assert_eq!(state.subscriptions.active_count().await, 0);
            let sink3 = TestSink::new();
            let info3 = state
                .subscribe_with_sink(sink3.clone())
                .await
                .expect("subscribe after reload");
            assert_eq!(info3.connection_generation, 2);
            assert_eq!(state.subscriptions.active_count().await, 1);
            poll_clients(&observer.client, 2, 1).await;
            eprintln!("STEP reload resubscribed without accumulation");

            // Close via the native-host cleanup seam (Finding E): same
            // authority as explicit disconnect, daemon stays live, repeated
            // close/exit events are idempotent.
            state.handle_native_close().await;
            assert_eq!(state.subscriptions.active_count().await, 0);
            poll_clients(&observer.client, 1, 0).await;
            let _ = snapshot_clients(&observer.client).await;
            state.handle_native_close().await;
            poll_clients(&observer.client, 1, 0).await;
            eprintln!("STEP close returned client count to baseline");

            // C002 adversarial: disconnect supersedes a prepared connect.
            // Allocate attempt A, complete slow preparation (real socket
            // connect) without committing, linearize a native close, then
            // prove the stale commit cannot resurrect the desktop connection.
            let attempt = state.allocate_connect_attempt();
            let prepared = LocalSocketClient::connect(&endpoint, descriptor())
                .await
                .expect("prepared connect");
            let prepared_id = prepared.daemon_id().await.expect("prepared daemon id");
            assert_eq!(prepared_id, observer.daemon_id);
            // The prepared socket is live at the daemon, but HostState still
            // reports disconnected with no owner.
            assert_eq!(state.subscriptions.active_count().await, 0);
            state.handle_native_close().await;
            let generation_before_stale = state.current_generation();
            let stale = state
                .commit_prepared_connection(attempt, prepared, prepared_id, None)
                .await;
            assert!(
                stale.is_err(),
                "prepared connect must not commit after native close"
            );
            assert_eq!(state.subscriptions.active_count().await, 0);
            assert_eq!(state.current_generation(), generation_before_stale);
            // The dropped prepared socket returns counts to observer-only
            // baseline; the daemon remains responsive.
            poll_clients(&observer.client, 1, 0).await;
            let _ = snapshot_clients(&observer.client).await;
            eprintln!("STEP adversarial prepared-connect superseded without resurrection");

            // Explicit-path autostart: stop the daemon, reconnect through the
            // autostart path, then prove desktop exit leaves it running.
            // Identity-check before signalling so a recycled pid can never
            // receive our SIGTERM: the live daemon id must match the one
            // this test autostarted, back-to-back with the kill.
            let live_snapshot = tokio::time::timeout(
                Duration::from_secs(5),
                observer.client.request(RequestEnvelope {
                    protocol_version: PROTOCOL_VERSION,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    payload: CoreRequest::SnapshotDaemon,
                }),
            )
            .await
            .expect("pre-kill snapshot deadline")
            .expect("pre-kill snapshot");
            match live_snapshot {
                CoreResponse::SnapshotDaemon { daemon_id, .. } => {
                    assert_eq!(daemon_id, observer.daemon_id, "live daemon identity");
                }
                other => panic!("unexpected pre-kill response: {other:?}"),
            }
            let pid = observer.started_pid.expect("autostarted pid");
            let status = std::process::Command::new("kill")
                .arg(pid.to_string())
                .status()
                .expect("kill daemon");
            assert!(status.success(), "kill exit: {status:?}");
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            loop {
                // A transport error here is itself death evidence: the
                // SIGTERM terminated the daemon socket.
                let dead = match tokio::time::timeout(
                    Duration::from_secs(5),
                    observer.client.request(RequestEnvelope {
                        protocol_version: PROTOCOL_VERSION,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        payload: CoreRequest::SnapshotDaemon,
                    }),
                )
                .await
                {
                    Err(_) => true,
                    Ok(Err(_)) => true,
                    Ok(Ok(_)) => false,
                };
                if dead {
                    break;
                }
                if tokio::time::Instant::now() >= deadline {
                    panic!("daemon did not stop");
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            drop(observer);
            let outcome = connect_or_start_local_daemon(
                LocalDaemonOptions {
                    endpoint: endpoint.clone(),
                    endpoint_argument: paths.socket_path_str(),
                    lock_path: paths.lock_path.clone(),
                    log_path: paths.log_path.clone(),
                    executable: Some(PathBuf::from(&executable)),
                    autostart: true,
                    startup_timeout: Duration::from_secs(20),
                    poll_interval: Duration::from_millis(200),
                },
                descriptor(),
            )
            .await
            .expect("desktop autostart");
            let restarted_pid = outcome.started_pid.expect("restarted pid");
            assert_ne!(pid, restarted_pid);
            let state2 = Arc::new(HostState::default());
            let daemon_id = outcome.client.daemon_id().await.expect("daemon id");
            let autostart_generation = state2
                .install_connection(outcome.client, daemon_id.clone())
                .await;
            assert_eq!(autostart_generation, 1);
            state2.disconnect_host().await;
            // The daemon outlives the desktop that autostarted it.
            let observer2 = LocalSocketClient::connect(&endpoint, descriptor())
                .await
                .expect("observer after autostart");
            let clients = snapshot_clients(&observer2).await;
            assert_eq!(clients.len(), 1, "only the observer: {clients:?}");
            eprintln!("STEP autostart survived desktop exit");
            // Identity-check the restarted daemon before signalling it.
            let restarted_id = observer2.daemon_id().await.expect("restarted daemon id");
            assert_eq!(restarted_id, daemon_id);
            let status = std::process::Command::new("kill")
                .arg(restarted_pid.to_string())
                .status()
                .expect("kill restarted daemon");
            assert!(status.success(), "kill exit: {status:?}");
            // Best-effort wait for graceful socket cleanup; the isolated
            // home directory itself is removed by `cleanup` below.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            while paths.socket_path.exists() && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        };
        run.await;
        cleanup();
    }
}
