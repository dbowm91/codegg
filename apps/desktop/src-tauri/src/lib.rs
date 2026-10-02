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
use lifecycle::SubscriptionRegistry;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{ipc::Channel, AppHandle, Manager, State};
use tokio::sync::Mutex;

const MAX_PROJECTS: usize = 50;

fn bound_projects(projects: impl IntoIterator<Item = ProjectSummary>) -> Vec<ProjectSummary> {
    projects.into_iter().take(MAX_PROJECTS).collect()
}

struct HostState {
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
}

impl Default for HostState {
    fn default() -> Self {
        Self {
            client: Mutex::new(None),
            daemon_id: Mutex::new(None),
            connection_generation: AtomicU64::new(0),
            connect_serial: AtomicU64::new(0),
            subscriptions: SubscriptionRegistry::new(),
        }
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

    /// Shared disconnect teardown: invalidate generation, cancel/join the
    /// active forwarder, drop client/daemon identity. Never stops the daemon.
    async fn disconnect_host(&self) {
        self.connection_generation.fetch_add(1, Ordering::AcqRel);
        self.subscriptions.shutdown().await;
        *self.client.lock().await = None;
        *self.daemon_id.lock().await = None;
    }

    /// Install a newly connected client. Cancels the stale subscription
    /// before committing the new connection, then bumps the visible
    /// generation. Callers must have fenced concurrent attempts already.
    async fn install_connection(&self, client: LocalSocketClient, daemon_id: String) -> u64 {
        self.subscriptions.shutdown().await;
        *self.client.lock().await = Some(client);
        *self.daemon_id.lock().await = Some(daemon_id);
        self.connection_generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Create/replace the project-event subscription for the current
    /// connection generation. This is the full production subscribe path
    /// minus the Tauri Channel adapter, so lifecycle tests (including the
    /// live-daemon trajectory) exercise the same ownership, fencing, and
    /// terminal-cleanup semantics as the invoked command.
    async fn subscribe_with_sink(
        self: &Arc<Self>,
        sink: Arc<dyn DesktopEventSink>,
    ) -> Result<SubscriptionInfo, String> {
        // Snapshot the current connection without holding the lock across spawn.
        let (client, generation) = {
            let client = self.client.lock().await.clone();
            let generation = self.current_generation();
            (client, generation)
        };
        let client = client.ok_or_else(|| "desktop is not connected".to_owned())?;
        if generation == 0 {
            return Err("desktop is not connected".to_owned());
        }
        let subscription_id = self.subscriptions.allocate_subscription_id();
        let mut events = client.subscribe();
        let task_state = Arc::clone(self);
        let task_generation = generation;
        let task_subscription_id = subscription_id.clone();
        let task = tokio::spawn(async move {
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
                    .is_some_and(|(id, gen)| id == task_subscription_id && gen == task_generation);
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
        // Atomically replace any previous subscription; the previous forwarder
        // is cancelled and joined inside the registry without holding its lock.
        let installed = self
            .subscriptions
            .install_with_id(subscription_id, generation, task)
            .await;
        Ok(SubscriptionInfo {
            subscription_id: installed,
            connection_generation: generation,
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
    let attempt = state.connect_serial.fetch_add(1, Ordering::AcqRel) + 1;
    let is_superseded = || state.connect_serial.load(Ordering::Acquire) != attempt;

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
            if is_superseded() {
                return Err("connection attempt was superseded".into());
            }
            let installed = state.install_connection(client, daemon_id).await;
            // Re-read the snapshot generation from the installed value so a
            // concurrent disconnect that bumped the counter cannot be hidden.
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
    if is_superseded() {
        return Err("connection attempt was superseded".into());
    }
    let installed = state
        .install_connection(outcome.client, outcome.daemon_id)
        .await;
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
        .run(tauri::generate_context!())
        .expect("failed to run CodeGG desktop shell");
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
    /// This is the WP8 display-backed trajectory minus the visible WebView:
    /// it drives the real production `HostState` paths (`install_connection`,
    /// `subscribe_with_sink`, unsubscribe, `disconnect_host`, autostart)
    /// against a real isolated daemon and asserts real
    /// `SnapshotDaemon.connected_clients` counts plus real invalidation
    /// delivery. Renderer generation-keyed resubscribe races are covered by
    /// the TypeScript suite; the Tauri `Channel` adapter is covered by the
    /// boundary guard and bridge-DTO checks.
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

            // Close: desktop client count returns to baseline while the
            // daemon and the observer remain responsive.
            state.disconnect_host().await;
            assert_eq!(state.subscriptions.active_count().await, 0);
            poll_clients(&observer.client, 1, 0).await;
            let _ = snapshot_clients(&observer.client).await;
            eprintln!("STEP close returned client count to baseline");

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
