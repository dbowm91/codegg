//! Session projection ownership for the desktop route (M004 WP C).
//!
//! This module binds one [`SessionProjectionDriver`](codegg_client::SessionProjectionDriver)
//! to the current route. The driver remains the only production
//! projection-state owner: this module owns fencing, replacement,
//! teardown, renderer fan-out, and cursor-retaining stop/resume —
//! never reduction, cursors, or snapshots.
//!
//! Ownership rules:
//! - at most one projection owner per route; attaching replaces the
//!   previous owner (stop + join) outside every lock;
//! - every attach completion is applied only when the captured
//!   (connection, route) generation remains current; a stale
//!   completion stops its fresh driver (releasing the daemon
//!   subscription) and reports stale without touching route state;
//! - project/workspace switches and disconnects stop the owner and
//!   abort/join the renderer watcher; a re-subscribe replaces any
//!   previous watcher, so reloads never accumulate them;
//! - disconnect retains the stopped driver (canonical cursor + last
//!   snapshot) so a reconnect can `resume` the same session without
//!   reusing the old subscription id; attaching a different session
//!   drops the retained stop;
//! - renderer watchers consume the driver's latest-only watch channel,
//!   so slow renderers coalesce rather than queue (update cadence §8).

use std::sync::Arc;
use std::time::Duration;

use codegg_client::SessionProjectionDriver;
use tauri::ipc::Channel;

use super::bridge::SessionPresentationView;
use super::present::present_driver;
use super::HostState;

/// Sink for session presentation views pushed to the renderer.
///
/// The production implementation wraps the Tauri IPC `Channel`; tests
/// use an in-memory recording sink. `send_view` returns false when the
/// sink is terminally closed, which ends the watcher. Latest-only
/// semantics live in the driver's watch channel: a slow sink coalesces
/// rather than queues.
pub(crate) trait ProjectionViewSink: Send + Sync + 'static {
    fn send_view(&self, view: SessionPresentationView) -> bool;
}

pub(crate) struct ChannelViewSink(pub(crate) Channel<SessionPresentationView>);

impl ProjectionViewSink for ChannelViewSink {
    fn send_view(&self, view: SessionPresentationView) -> bool {
        self.0.send(view).is_ok()
    }
}

/// Live projection attachment bound to one route generation.
pub(crate) struct ProjectionOwner {
    session_id: String,
    connection_generation: u64,
    route_generation: u64,
    driver: SessionProjectionDriver,
}

impl HostState {
    /// Start (or resume) projection for the route-selected session.
    /// The requested session must be the currently selected one: the
    /// renderer cannot attach an arbitrary session id through this
    /// command. Generations fence the attach; see
    /// [`Self::attach_projection`].
    pub(crate) async fn projection_start(
        &self,
        session_id: String,
        expected_generation: u64,
    ) -> Result<SessionPresentationView, String> {
        let (connection_generation, route_generation) = {
            let route = self.route.lock().await;
            if route.connection_generation != self.current_generation()
                || route.project_id.is_none()
            {
                return Err("no current route; select a project first".into());
            }
            if route.route_generation != expected_generation {
                return Err("stale route; refresh before starting projection".into());
            }
            if route.session_id.as_deref() != Some(session_id.as_str()) {
                return Err("session is not the route-selected session".into());
            }
            (route.connection_generation, route.route_generation)
        };
        self.attach_projection(session_id, connection_generation, route_generation)
            .await
    }
    /// Take the live projection owner without stopping it. The caller
    /// must stop/join outside the route lock.
    async fn take_projection_owner(&self) -> Option<ProjectionOwner> {
        self.route.lock().await.projection.take()
    }

    /// Abort and join the renderer projection watcher. Idempotent.
    async fn join_projection_watcher(&self) {
        let watcher = { self.route.lock().await.watcher.take() };
        if let Some(watcher) = watcher {
            watcher.abort();
            let _ = watcher.await;
        }
    }

    /// Stop the live owner (if any), retaining its stopped cursor for a
    /// same-session resume. Stops and joins outside the route lock;
    /// never touches the current route selection itself. Also used for
    /// full teardown paths: callers that invalidate the route itself
    /// (project/workspace switches clear the selection first) rely on
    /// the next attach dropping a retained stop bound to another
    /// session.
    pub(crate) async fn stop_projection_owner(&self) {
        self.join_projection_watcher().await;
        let owner = self.take_projection_owner().await;
        if let Some(owner) = owner {
            let session_id = owner.session_id.clone();
            match owner.driver.stop().await {
                Ok(stopped) => {
                    let mut route = self.route.lock().await;
                    // A newer attach may have installed a fresh owner
                    // while this stop was joining: only retain the
                    // cursor when no live owner exists.
                    if route.projection.is_none() {
                        route.stopped = Some(stopped);
                    }
                    eprintln!("desktop projection stopped; cursor retained: {session_id}");
                }
                Err(error) => {
                    eprintln!("desktop projection stop failed for {session_id}: {error}");
                }
            }
        }
    }

    /// Attach (or resume) a projection driver for `session_id` on the
    /// captured generations. A retained stopped cursor for the same
    /// session resumes without reusing the old subscription id; any
    /// other retained stop is dropped.
    ///
    /// Slow driver attach/resume runs outside every lock; the final
    /// install is fenced on (connection, route) generation. A stale
    /// completion stops its fresh driver before reporting stale, so no
    /// orphan subscription survives a route switch mid-attach.
    pub(crate) async fn attach_projection(
        &self,
        session_id: String,
        connection_generation: u64,
        route_generation: u64,
    ) -> Result<SessionPresentationView, String> {
        let client = self
            .client
            .lock()
            .await
            .clone()
            .ok_or_else(|| "desktop is not connected".to_owned())?;
        if self.current_generation() != connection_generation || connection_generation == 0 {
            return Err("connection changed; projection attach discarded".into());
        }
        let stopped = { self.route.lock().await.stopped.take() };
        let (driver, resumed) = match stopped {
            Some(stopped) if stopped.session_id() == session_id => {
                let cursor = stopped.cursor_seq();
                let driver = SessionProjectionDriver::resume(client, stopped)
                    .await
                    .map_err(|error| error.to_string())?;
                eprintln!("desktop projection resumed from cursor for {session_id}: {cursor:?}");
                (driver, true)
            }
            _ => {
                let driver = SessionProjectionDriver::attach(client, session_id.clone())
                    .await
                    .map_err(|error| error.to_string())?;
                (driver, false)
            }
        };
        let view = self.present_with_controller(&driver.current()).await;
        {
            let mut route = self.route.lock().await;
            if self.current_generation() != connection_generation
                || route.connection_generation != connection_generation
                || route.route_generation != route_generation
            {
                drop(route);
                // Stale route: release the fresh subscription before
                // reporting, so a switch mid-attach leaks nothing.
                if driver.stop().await.is_err() {
                    eprintln!("stale projection attach stop failed");
                }
                return Err("stale route; projection attach discarded".into());
            }
            let previous = route.projection.replace(ProjectionOwner {
                session_id,
                connection_generation,
                route_generation,
                driver,
            });
            drop(route);
            if let Some(previous) = previous {
                if previous.driver.stop().await.is_err() {
                    eprintln!("replaced projection stop failed");
                }
            }
        }
        eprintln!("desktop projection attached (resumed={resumed})");
        Ok(view)
    }

    /// Current presentation view for the live owner. Fails closed when
    /// no projection is attached or the owner belongs to a superseded
    /// generation.
    pub(crate) async fn projection_current(&self) -> Result<SessionPresentationView, String> {
        let (snapshot_view, controller) = {
            let route = self.route.lock().await;
            let Some(owner) = route.projection.as_ref() else {
                return Err("no session projection attached for the current route".into());
            };
            if owner.connection_generation != self.current_generation()
                || owner.route_generation != route.route_generation
            {
                return Err("session projection is stale; reattach".into());
            }
            (owner.driver.current(), route.controller.clone())
        };
        let mut presented = present_driver(&snapshot_view);
        presented.controller = controller;
        Ok(presented)
    }

    /// Subscribe a renderer sink to latest-only presentation views.
    /// The watcher exits on generation drift, owner replacement, stop,
    /// or sink close. Slow renderers coalesce: the driver watch
    /// channel keeps only the newest view.
    pub(crate) async fn projection_subscribe_with_sink(
        self: &Arc<Self>,
        sink: Arc<dyn ProjectionViewSink>,
    ) -> Result<(), String> {
        let (session_id, connection_generation, route_generation, views) = {
            let route = self.route.lock().await;
            let Some(owner) = route.projection.as_ref() else {
                return Err("no session projection attached for the current route".into());
            };
            (
                owner.session_id.clone(),
                owner.connection_generation,
                owner.route_generation,
                owner.driver.subscribe_views(),
            )
        };
        if connection_generation != self.current_generation() {
            return Err("session projection is stale; reattach".into());
        }
        let state = Arc::clone(self);
        let watcher = tokio::spawn(async move {
            let mut views = views;
            // Push the current view immediately so subscribe/install is
            // atomic from the renderer's view.
            let snapshot = views.borrow().clone();
            if !sink.send_view(state.present_with_controller(&snapshot).await) {
                return;
            }
            loop {
                match tokio::time::timeout(Duration::from_secs(30), views.changed()).await {
                    Ok(Ok(())) => {}
                    // Lagged: borrow the newest view and continue; the
                    // view itself is atomic-replace, so coalescing is
                    // lossless at presentation granularity.
                    Ok(Err(_)) => {
                        let snapshot = views.borrow().clone();
                        if !sink.send_view(state.present_with_controller(&snapshot).await) {
                            return;
                        }
                        continue;
                    }
                    Err(_) => continue,
                }
                if !state
                    .projection_watcher_current(
                        &session_id,
                        connection_generation,
                        route_generation,
                    )
                    .await
                {
                    return;
                }
                let snapshot = views.borrow().clone();
                if !sink.send_view(state.present_with_controller(&snapshot).await) {
                    return;
                }
            }
        });
        // One renderer route owns at most one projection watcher:
        // a re-subscribe replaces (abort + join) the previous one, so
        // renderer reloads can never accumulate watchers.
        let previous = { self.route.lock().await.watcher.replace(watcher) };
        if let Some(previous) = previous {
            previous.abort();
            let _ = previous.await;
        }
        Ok(())
    }

    async fn projection_watcher_current(
        &self,
        session_id: &str,
        connection_generation: u64,
        route_generation: u64,
    ) -> bool {
        let route = self.route.lock().await;
        self.current_generation() == connection_generation
            && route.connection_generation == connection_generation
            && route.route_generation == route_generation
            && route.projection.as_ref().is_some_and(|owner| {
                owner.session_id == session_id
                    && owner.connection_generation == connection_generation
                    && owner.route_generation == route_generation
            })
    }

    #[cfg(test)]
    pub(crate) async fn test_projection_owner_session(&self) -> Option<String> {
        self.route
            .lock()
            .await
            .projection
            .as_ref()
            .map(|owner| owner.session_id.clone())
    }

    #[cfg(test)]
    pub(crate) async fn test_retained_cursor_seq(&self) -> Option<u64> {
        self.route
            .lock()
            .await
            .stopped
            .as_ref()
            .and_then(|stopped| stopped.cursor_seq())
    }

    #[cfg(test)]
    pub(crate) async fn test_watcher_count(&self) -> usize {
        usize::from(self.route.lock().await.watcher.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_protocol::core::{CoreRequest, CoreResponse, PROTOCOL_VERSION};
    use codegg_protocol::dto::{
        ProjectDetailsDto, ProjectSummaryDto, ProjectWorkspaceSummaryDto, Session,
    };
    use codegg_protocol::frames::{CoreFrame, ServerCapabilities, ServerHello};
    use codegg_protocol::projection::caps::PROJECTION_PROTOCOL_VERSION;
    use codegg_protocol::projection::replay::{
        ProjectionCursor, ProjectionReplayBatch, ProjectionSnapshotBundle,
        ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind,
        ProjectionSubscriptionId,
    };
    use codegg_protocol::projection::snapshot::SessionProjectionSnapshot;

    fn detail_response(project_id: &str, workspace_id: &str, root: Option<&str>) -> CoreResponse {
        CoreResponse::ProjectGet {
            project: ProjectDetailsDto {
                project: ProjectSummaryDto {
                    project_id: project_id.into(),
                    display_name: format!("Project {project_id}"),
                    lifecycle: "active".into(),
                    description: None,
                    tags: Vec::new(),
                    time_last_opened_at: None,
                    registration_source: "test".into(),
                    archived_at: None,
                    created_at: 0,
                    updated_at: 0,
                },
                workspaces: vec![ProjectWorkspaceSummaryDto {
                    workspace_id: workspace_id.into(),
                    display_name: "Workspace".into(),
                    canonical_root: root.map(str::to_string),
                }],
                session_count: 1,
                health: None,
            },
        }
    }

    fn session_dto(id: &str, project_id: &str, workspace_id: &str) -> Session {
        Session {
            id: id.into(),
            project_id: project_id.into(),
            workspace_id: Some(workspace_id.into()),
            binding: Some(codegg_protocol::dto::SessionBindingDto {
                project_id: project_id.into(),
                workspace_id: workspace_id.into(),
                repository_id: None,
                binding_state: Some("bound".into()),
                binding_revision: Some(1),
                compatibility_directory: None,
            }),
            directory: format!("/tmp/{project_id}/{workspace_id}"),
            title: "session".into(),
            ..Default::default()
        }
    }

    fn caps_response() -> CoreResponse {
        CoreResponse::ProjectionCapabilitiesResponse {
            supported: true,
            projection_version: PROJECTION_PROTOCOL_VERSION,
            max_events_per_batch: 512,
            max_event_bytes: 64 * 1024,
            max_subscriptions_per_client: 32,
            max_subscriptions_per_daemon: 256,
            retention_session_max_events: 20_000,
            retention_project_max_events: 50_000,
        }
    }

    fn stream_descriptor(session_id: &str) -> ProjectionStreamDescriptor {
        ProjectionStreamDescriptor {
            stream_id: ProjectionStreamId::new("test-stream-1").expect("stream id"),
            kind: ProjectionStreamKind::Session,
            project_id: "proj-a".into(),
            workspace_id: Some("ws-1".into()),
            session_id: Some(session_id.into()),
            projection_version: PROJECTION_PROTOCOL_VERSION,
            retention_floor_seq: 0,
            high_water_seq: 0,
            latest_checkpoint_seq: None,
        }
    }

    fn subscribed_response(sub_id: &str, session_id: &str) -> CoreResponse {
        let descriptor = stream_descriptor(session_id);
        CoreResponse::ProjectionSubscribed {
            subscription_id: ProjectionSubscriptionId::new(sub_id),
            descriptor: descriptor.clone(),
            snapshot: ProjectionSnapshotBundle::One {
                snapshot: Box::new(SessionProjectionSnapshot::empty(
                    session_id, "proj-a", "ws-1",
                )),
            },
            cursor: ProjectionCursor {
                stream_id: descriptor.stream_id.clone(),
                event_seq: 0,
                projection_version: PROJECTION_PROTOCOL_VERSION,
            },
            retention_floor_seq: 0,
        }
    }

    fn empty_replay(sub_id: &str, session_id: &str, seq: u64) -> CoreResponse {
        CoreResponse::ProjectionReplay {
            subscription_id: Some(ProjectionSubscriptionId::new(sub_id)),
            batch: ProjectionReplayBatch {
                descriptor: stream_descriptor(session_id),
                events: Vec::new(),
                snapshot: None,
                replay_start_seq: seq,
                replay_end_seq: seq,
                current_high_water: seq,
                truncation_flag: false,
                next_cursor: None,
            },
        }
    }

    struct FakeDaemon {
        reader: tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>,
        writer: tokio::net::unix::OwnedWriteHalf,
    }

    impl FakeDaemon {
        async fn accept(listener: &tokio::net::UnixListener) -> Self {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
            let (stream, _) = listener.accept().await.expect("accept");
            let (read, mut writer) = stream.into_split();
            let mut reader = tokio::io::BufReader::new(read);
            let mut line = String::new();
            reader.read_line(&mut line).await.expect("hello");
            let hello = CoreFrame::ServerHello(ServerHello {
                daemon_id: "fake-daemon".into(),
                protocol_version: PROTOCOL_VERSION,
                server_capabilities: ServerCapabilities {
                    event_replay: true,
                    session_management: true,
                    permission_routing: true,
                    workspace_registration: true,
                    workspace_snapshots: true,
                    durable_jobs: true,
                    durable_schedules: true,
                    identity_aware_context: true,
                    project_catalog: true,
                    session_projection: true,
                },
                client_id: "projection-client-id".into(),
            });
            writer
                .write_all(format!("{}\n", serde_json::to_string(&hello).unwrap()).as_bytes())
                .await
                .expect("hello");
            writer.flush().await.expect("flush");
            loop {
                line.clear();
                reader.read_line(&mut line).await.expect("post-hello");
                if matches!(
                    serde_json::from_str::<CoreFrame>(line.trim()).expect("decode"),
                    CoreFrame::Subscribe { .. }
                ) {
                    break;
                }
            }
            Self { reader, writer }
        }

        async fn next_request(&mut self) -> (String, CoreRequest) {
            use tokio::io::AsyncBufReadExt;
            loop {
                let mut line = String::new();
                tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    self.reader.read_line(&mut line),
                )
                .await
                .expect("request timeout")
                .expect("read");
                match serde_json::from_str::<CoreFrame>(line.trim()).expect("decode") {
                    CoreFrame::Request(request) => return (request.request_id, request.payload),
                    _ => continue,
                }
            }
        }

        async fn respond(&mut self, request_id: String, response: CoreResponse) {
            use tokio::io::AsyncWriteExt;
            let frame = CoreFrame::Response {
                request_id,
                response: Box::new(response),
            };
            self.writer
                .write_all(format!("{}\n", serde_json::to_string(&frame).unwrap()).as_bytes())
                .await
                .expect("respond");
            self.writer.flush().await.expect("flush");
        }

        async fn expect_unsubscribe(&mut self) -> String {
            let (_, payload) = self.next_request().await;
            let CoreRequest::ProjectionUnsubscribe { subscription_id } = payload else {
                panic!("expected ProjectionUnsubscribe, got {payload:?}");
            };
            subscription_id.as_str().to_string()
        }

        async fn send_frame(&mut self, frame: &CoreFrame) {
            use tokio::io::AsyncWriteExt;
            self.writer
                .write_all(format!("{}\n", serde_json::to_string(frame).unwrap()).as_bytes())
                .await
                .expect("send frame");
            self.writer.flush().await.expect("flush");
        }
    }

    fn short_endpoint() -> (String, tokio::net::UnixListener) {
        let short = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let socket = std::env::temp_dir().join(format!("cgd-proj-{short}.sock"));
        let listener = tokio::net::UnixListener::bind(&socket).expect("bind");
        (format!("unix://{}", socket.display()), listener)
    }

    async fn connected_host(endpoint: &str) -> std::sync::Arc<HostState> {
        let state = std::sync::Arc::new(HostState::default());
        let client =
            codegg_client::LocalSocketClient::connect(endpoint.to_string(), crate::descriptor())
                .await
                .expect("connect");
        let daemon_id = client.daemon_id().await.expect("daemon id");
        assert_eq!(state.install_connection(client, daemon_id).await, 1);
        state
    }

    async fn open_session_one(state: &std::sync::Arc<HostState>) -> crate::bridge::RouteTokenView {
        let detail = state
            .route_project_detail("proj-a".into())
            .await
            .expect("detail");
        let token = state
            .route_workspace_select("ws-1".into(), detail.route_token.route_generation)
            .await
            .expect("select");
        let view = state
            .route_session_open("session-1".into(), token.route_generation)
            .await
            .expect("open");
        view.route_token
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn start_attaches_and_current_returns_view() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(&listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(id, detail_response("proj-a", "ws-1", None))
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::SessionAttach { .. }));
            daemon
                .respond(
                    id,
                    CoreResponse::Session {
                        session: session_dto("session-1", "proj-a", "ws-1"),
                    },
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
            daemon.respond(id, caps_response()).await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
            daemon
                .respond(id, subscribed_response("sub-1", "session-1"))
                .await;
            let released = daemon.expect_unsubscribe().await;
            assert_eq!(released, "sub-1");
        });

        let state = connected_host(&endpoint).await;
        let token = open_session_one(&state).await;
        let view = state
            .projection_start("session-1".into(), token.route_generation)
            .await
            .expect("start");
        assert_eq!(view.state, "attached");
        assert_eq!(view.session_id, "session-1");
        assert_eq!(view.project_id, "proj-a");
        let current = state.projection_current().await.expect("current");
        assert_eq!(current.state, "attached");
        assert_eq!(
            state.test_projection_owner_session().await.as_deref(),
            Some("session-1")
        );
        // Clean stop joins the driver: the fake observes the exact
        // subscription release.
        state.stop_projection_owner().await;
        assert!(state.test_projection_owner_session().await.is_none());
        assert_eq!(state.test_retained_cursor_seq().await, Some(0));
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn start_rejects_non_selected_session_without_network() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(&listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(id, detail_response("proj-a", "ws-1", None))
                .await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    CoreResponse::Session {
                        session: session_dto("session-1", "proj-a", "ws-1"),
                    },
                )
                .await;
            // No projection handshake may follow: the wrong-session
            // start must fail before any network request.
            let timed: Result<(String, CoreRequest), _> =
                tokio::time::timeout(std::time::Duration::from_millis(400), daemon.next_request())
                    .await;
            assert!(timed.is_err(), "unexpected projection request");
        });

        let state = connected_host(&endpoint).await;
        let _ = open_session_one(&state).await;
        let error = state
            .projection_start("session-evil".into(), 999)
            .await
            .expect_err("wrong session rejected");
        assert!(error.contains("not the route-selected session") || error.contains("stale route"));
        assert!(state.test_projection_owner_session().await.is_none());
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stale_start_releases_fresh_subscription() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(&listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    CoreResponse::ProjectGet {
                        project: ProjectDetailsDto {
                            project: ProjectSummaryDto {
                                project_id: "proj-a".into(),
                                display_name: "Project proj-a".into(),
                                lifecycle: "active".into(),
                                description: None,
                                tags: Vec::new(),
                                time_last_opened_at: None,
                                registration_source: "test".into(),
                                archived_at: None,
                                created_at: 0,
                                updated_at: 0,
                            },
                            workspaces: vec![
                                ProjectWorkspaceSummaryDto {
                                    workspace_id: "ws-1".into(),
                                    display_name: "One".into(),
                                    canonical_root: None,
                                },
                                ProjectWorkspaceSummaryDto {
                                    workspace_id: "ws-2".into(),
                                    display_name: "Two".into(),
                                    canonical_root: None,
                                },
                            ],
                            session_count: 0,
                            health: None,
                        },
                    },
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::SessionAttach { .. }));
            daemon
                .respond(
                    id,
                    CoreResponse::Session {
                        session: session_dto("session-1", "proj-a", "ws-1"),
                    },
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
            daemon.respond(id, caps_response()).await;
            // Delay the subscribe response past the workspace switch so
            // the attach completion lands on a superseded generation.
            let (sub_id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            daemon
                .respond(sub_id, subscribed_response("sub-stale", "session-1"))
                .await;
            // The stale completion must release its fresh subscription.
            let released = daemon.expect_unsubscribe().await;
            assert_eq!(released, "sub-stale");
        });

        let state = connected_host(&endpoint).await;
        let token = open_session_one(&state).await;
        let state_start = state.clone();
        let started = tokio::spawn(async move {
            state_start
                .projection_start("session-1".into(), token.route_generation)
                .await
        });
        // Let the attach reach the daemon before superseding the route.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        state
            .route_workspace_select("ws-2".into(), token.route_generation)
            .await
            .expect("switch wins");
        let error = started.await.expect("join").expect_err("stale start drops");
        assert!(error.contains("stale route"), "unexpected error: {error}");
        assert!(state.test_projection_owner_session().await.is_none());
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn watcher_pushes_latest_only_views_and_joins_on_stop() {
        use codegg_protocol::core::CoreEvent;
        use codegg_protocol::core::EventEnvelope;
        use codegg_protocol::projection::event::{ProjectionEnvelope, ProjectionEvent};
        use std::sync::Mutex;

        struct RecordingSink {
            views: Mutex<Vec<SessionPresentationView>>,
            notify: tokio::sync::Notify,
        }

        impl RecordingSink {
            fn new() -> Arc<Self> {
                Arc::new(Self {
                    views: Mutex::new(Vec::new()),
                    notify: tokio::sync::Notify::new(),
                })
            }

            async fn wait_for_views(sink: &Arc<Self>, count: usize) {
                let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
                loop {
                    let len = sink.views.lock().expect("lock").len();
                    if len >= count {
                        return;
                    }
                    if tokio::time::Instant::now() >= deadline {
                        panic!("timed out waiting for {count} views");
                    }
                    tokio::time::timeout(
                        std::time::Duration::from_millis(50),
                        sink.notify.notified(),
                    )
                    .await
                    .ok();
                }
            }
        }

        impl ProjectionViewSink for RecordingSink {
            fn send_view(&self, view: SessionPresentationView) -> bool {
                self.views.lock().expect("lock").push(view);
                self.notify.notify_waiters();
                true
            }
        }

        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(&listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(id, detail_response("proj-a", "ws-1", None))
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::SessionAttach { .. }));
            daemon
                .respond(
                    id,
                    CoreResponse::Session {
                        session: session_dto("session-1", "proj-a", "ws-1"),
                    },
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
            daemon.respond(id, caps_response()).await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
            daemon
                .respond(id, subscribed_response("sub-1", "session-1"))
                .await;
            // Let the renderer subscribe before the live event lands.
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            daemon
                .send_frame(&CoreFrame::Event(EventEnvelope {
                    protocol_version: PROTOCOL_VERSION,
                    event_seq: 1,
                    timestamp_ms: 0,
                    session_id: None,
                    turn_id: None,
                    payload: CoreEvent::ProjectionStreamEvent {
                        subscription_id: ProjectionSubscriptionId::new("sub-1"),
                        stream_id: stream_descriptor("session-1").stream_id.clone(),
                        envelope: ProjectionEnvelope::session_event(
                            1,
                            0,
                            "session-1",
                            None,
                            ProjectionEvent::Diagnostic {
                                code: "watcher-test".into(),
                                message: "live".into(),
                            },
                        ),
                    },
                }))
                .await;
            let released = daemon.expect_unsubscribe().await;
            assert_eq!(released, "sub-1");
        });

        let state = connected_host(&endpoint).await;
        let token = open_session_one(&state).await;
        state
            .projection_start("session-1".into(), token.route_generation)
            .await
            .expect("start");
        let sink = RecordingSink::new();
        state
            .projection_subscribe_with_sink(sink.clone())
            .await
            .expect("subscribe");
        assert_eq!(state.test_watcher_count().await, 1);
        // Initial install view plus one live-event view.
        RecordingSink::wait_for_views(&sink, 2).await;
        {
            let views = sink.views.lock().expect("lock");
            assert!(views.iter().all(|view| view.session_id == "session-1"));
            assert!(views.iter().all(|view| view.state == "attached"));
        }
        // Stop joins the watcher with the driver.
        state.stop_projection_owner().await;
        assert_eq!(state.test_watcher_count().await, 0);
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn disconnect_stops_owner_retains_cursor_and_releases() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(&listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(id, detail_response("proj-a", "ws-1", None))
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::SessionAttach { .. }));
            daemon
                .respond(
                    id,
                    CoreResponse::Session {
                        session: session_dto("session-1", "proj-a", "ws-1"),
                    },
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
            daemon.respond(id, caps_response()).await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
            daemon
                .respond(id, subscribed_response("sub-1", "session-1"))
                .await;
            let released = daemon.expect_unsubscribe().await;
            assert_eq!(released, "sub-1");
        });

        let state = connected_host(&endpoint).await;
        let token = open_session_one(&state).await;
        state
            .projection_start("session-1".into(), token.route_generation)
            .await
            .expect("start");
        // Route close joins the driver: the daemon observes the exact
        // subscription release, the owner is gone, and the canonical
        // cursor is retained for resume.
        state.disconnect_host().await;
        assert!(state.test_projection_owner_session().await.is_none());
        assert_eq!(state.test_retained_cursor_seq().await, Some(0));
        assert!(state.projection_current().await.is_err());
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reconnect_resumes_cursor_with_fresh_subscription() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            // First connection: full attach on sub-A.
            let mut daemon = FakeDaemon::accept(&listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(id, detail_response("proj-a", "ws-1", None))
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::SessionAttach { .. }));
            daemon
                .respond(
                    id,
                    CoreResponse::Session {
                        session: session_dto("session-1", "proj-a", "ws-1"),
                    },
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
            daemon.respond(id, caps_response()).await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
            daemon
                .respond(id, subscribed_response("sub-A", "session-1"))
                .await;
            let released = daemon.expect_unsubscribe().await;
            assert_eq!(released, "sub-A");
            drop(daemon);

            // Second connection: resume must carry the retained cursor
            // and mint a fresh subscription id, never sub-A.
            let mut daemon = FakeDaemon::accept(&listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(id, detail_response("proj-a", "ws-1", None))
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::SessionAttach { .. }));
            daemon
                .respond(
                    id,
                    CoreResponse::Session {
                        session: session_dto("session-1", "proj-a", "ws-1"),
                    },
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
            daemon.respond(id, caps_response()).await;
            let (id, payload) = daemon.next_request().await;
            let CoreRequest::ProjectionResume { cursor, .. } = payload else {
                panic!("expected ProjectionResume, got {payload:?}");
            };
            assert_eq!(cursor.event_seq, 0);
            daemon
                .respond(id, empty_replay("sub-B", "session-1", 0))
                .await;
            let released = daemon.expect_unsubscribe().await;
            assert_eq!(released, "sub-B");
        });

        let state = connected_host(&endpoint).await;
        let token = open_session_one(&state).await;
        state
            .projection_start("session-1".into(), token.route_generation)
            .await
            .expect("start");
        state.disconnect_host().await;
        assert_eq!(state.test_retained_cursor_seq().await, Some(0));

        // Reconnect on a new transport: the old owner is gone and the
        // route must be re-established before the same session resumes.
        let client =
            codegg_client::LocalSocketClient::connect(endpoint.clone(), crate::descriptor())
                .await
                .expect("reconnect");
        let daemon_id = client.daemon_id().await.expect("daemon id");
        state.install_connection(client, daemon_id).await;
        let token = open_session_one(&state).await;
        let view = state
            .projection_start("session-1".into(), token.route_generation)
            .await
            .expect("resume");
        assert_eq!(view.state, "attached");
        assert_eq!(view.session_id, "session-1");
        state.stop_projection_owner().await;
        server.await.expect("server");
    }
}
