//! Desktop route/session controller (M004 WP B).
//!
//! This module extends the C002 connection lifecycle with a separate
//! bounded session-view route. It never touches the M003 project-catalog
//! `SubscriptionRegistry`: catalog invalidation stays catalog-only, and
//! all session/projection state lives here.
//!
//! Authority rules enforced in this module:
//! - route identity is explicit project id + workspace id + session id;
//! - project workspaces come from authorized `ProjectGet`, never from
//!   global `WorkspaceList` (LocalOwner/proven-local only) or renderer
//!   paths;
//! - canonical workspace roots stay Rust-host data and are never
//!   serialized to the renderer or sent back as authority;
//! - session creation always carries explicit project/workspace ids with
//!   the Rust-resolved canonical root as `directory`;
//! - every async completion is applied only when the captured route
//!   token (connection + route generation) remains current, otherwise
//!   it is stale-dropped without mutating route state.

use std::collections::HashMap;

use codegg_client::StoppedDriver;
use codegg_protocol::{
    core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION},
    dto::Session,
};

use super::bridge::{
    ProjectDetailView, RouteTokenView, SessionListView, SessionSummaryView, SessionView,
    WorkspaceView,
};
use super::projection::ProjectionOwner;
use super::HostState;

/// Bounded session summaries per list call. The daemon enforces its own
/// limit; the host additionally truncates so the renderer can never
/// observe an unbounded array.
pub(crate) const MAX_SESSIONS: usize = 50;

/// Rust-host route state. `roots` holds canonical workspace roots and is
/// never serialized; only `*_view` DTOs cross the bridge. `projection`
/// is the single live projection owner for the route (WP C);
/// `stopped` retains a stopped cursor for same-session resume; `prompt`
/// is the in-flight/failed prompt intent for at-most-once coalescing
/// (WP D).
#[derive(Default)]
pub(crate) struct RouteState {
    pub(crate) route_generation: u64,
    pub(crate) connection_generation: u64,
    pub(crate) project_id: Option<String>,
    pub(crate) workspace_id: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) roots: HashMap<String, String>,
    pub(crate) workspaces: Vec<WorkspaceView>,
    pub(crate) projection: Option<ProjectionOwner>,
    pub(crate) stopped: Option<StoppedDriver>,
    pub(crate) watcher: Option<tokio::task::JoinHandle<()>>,
    pub(crate) prompt: Option<codegg_client::PromptIntent>,
}

impl RouteState {
    fn token(&self, session_id: Option<String>) -> RouteTokenView {
        RouteTokenView {
            connection_generation: self.connection_generation,
            route_generation: self.route_generation,
            project_id: self.project_id.clone().unwrap_or_default(),
            workspace_id: self.workspace_id.clone().unwrap_or_default(),
            session_id,
        }
    }

    pub(crate) fn current_token(&self) -> RouteTokenView {
        let session_id = self.session_id.clone();
        self.token(session_id)
    }
}

/// Render a daemon rejection with its code so callers (and the WP D
/// prompt path) can distinguish failure kinds. Anything else stays a
/// generic unexpected-response failure; both fail closed.
pub(crate) fn unexpected_daemon_response(response: &CoreResponse) -> String {
    if let CoreResponse::Error { code, message } = response {
        return format!("{code}: {message}");
    }
    "daemon returned an unexpected response".to_string()
}

/// `true` when the daemon-returned session is bound to the given
/// project/workspace route, through either the canonical binding or the
/// legacy locator fields. Anything else fails closed.
pub(crate) fn session_binding_matches(
    session: &Session,
    project_id: &str,
    workspace_id: &str,
) -> bool {
    if let Some(binding) = &session.binding {
        binding.project_id == project_id && binding.workspace_id == workspace_id
    } else {
        session.project_id == project_id && session.workspace_id.as_deref() == Some(workspace_id)
    }
}

pub(crate) fn session_summary_view(session: &Session) -> SessionSummaryView {
    let workspace_id = session
        .binding
        .as_ref()
        .map(|binding| binding.workspace_id.clone())
        .or_else(|| session.workspace_id.clone());
    SessionSummaryView {
        session_id: session.id.clone(),
        title: session.title.clone(),
        project_id: session
            .binding
            .as_ref()
            .map(|binding| binding.project_id.clone())
            .unwrap_or_else(|| session.project_id.clone()),
        workspace_id,
    }
}

impl HostState {
    pub(crate) async fn route_request(
        &self,
    ) -> Result<(codegg_client::LocalSocketClient, u64), String> {
        // Snapshot the client clone; the slow daemon work that follows
        // runs outside every lock.
        let client = self
            .client
            .lock()
            .await
            .clone()
            .ok_or_else(|| "desktop is not connected".to_owned())?;
        let connection_generation = self.current_generation();
        if connection_generation == 0 {
            return Err("desktop is not connected".to_owned());
        }
        Ok((client, connection_generation))
    }

    /// Fetch the selected project's workspaces and install a fresh
    /// route. Any in-flight route work for a previous generation becomes
    /// stale and is dropped on application, never merged.
    pub(crate) async fn route_project_detail(
        &self,
        project_id: String,
    ) -> Result<ProjectDetailView, String> {
        let (client, connection_generation) = self.route_request().await?;
        // Snapshot the route identity this fetch started from. Any commit
        // after this point (a concurrent switch or a reconnect) makes
        // this completion stale, no matter how fresh its payload is.
        let (route_conn_before, route_gen_before) = {
            let route = self.route.lock().await;
            (route.connection_generation, route.route_generation)
        };
        let response = client
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload: CoreRequest::ProjectGet {
                    project_id: project_id.clone(),
                },
            })
            .await
            .map_err(|error| error.to_string())?;
        let CoreResponse::ProjectGet { project } = response else {
            return Err("daemon returned an unexpected project response".into());
        };
        if project.project.project_id != project_id {
            return Err("daemon returned a different project than requested".into());
        }
        let mut roots = HashMap::new();
        let workspaces: Vec<WorkspaceView> = project
            .workspaces
            .iter()
            .map(|workspace| {
                if let Some(root) = workspace.canonical_root.clone() {
                    roots.insert(workspace.workspace_id.clone(), root);
                }
                WorkspaceView {
                    workspace_id: workspace.workspace_id.clone(),
                    display_name: workspace.display_name.clone(),
                }
            })
            .collect();
        let mut route = self.route.lock().await;
        // A reconnect during the fetch, or a route commit by a
        // concurrent selection, invalidates this completion: discard
        // instead of installing cross-connection or cross-project state.
        if self.current_generation() != connection_generation
            || (route.connection_generation, route.route_generation)
                != (route_conn_before, route_gen_before)
        {
            return Err("connection changed during project load; route discarded".into());
        }
        route.route_generation = route.route_generation.saturating_add(1);
        route.connection_generation = connection_generation;
        route.project_id = Some(project_id);
        route.workspace_id = None;
        route.session_id = None;
        route.roots = roots;
        route.workspaces = workspaces.clone();
        let token = route.current_token();
        let view = ProjectDetailView {
            project_id: project.project.project_id,
            display_name: project.project.display_name,
            workspaces,
            session_count: project.session_count,
            route_token: token,
        };
        drop(route);
        // A new project route invalidates any live projection: stop the
        // previous owner (cursor retained for a same-session resume) and
        // join its renderer watchers. The retained stop is dropped on
        // the next attach for a different session.
        self.stop_projection_owner().await;
        Ok(view)
    }

    /// Select a workspace from the installed project detail. Clears any
    /// session selection and fences the route generation so in-flight
    /// session work for the previous workspace is stale-dropped.
    pub(crate) async fn route_workspace_select(
        &self,
        workspace_id: String,
        expected_generation: u64,
    ) -> Result<RouteTokenView, String> {
        let mut route = self.route.lock().await;
        if route.connection_generation != self.current_generation() || route.project_id.is_none() {
            return Err("no current route; select a project first".into());
        }
        if route.route_generation != expected_generation {
            return Err("stale route; refresh the project first".into());
        }
        if !route
            .workspaces
            .iter()
            .any(|workspace| workspace.workspace_id == workspace_id)
        {
            return Err("unknown workspace for the current project".into());
        }
        route.route_generation = route.route_generation.saturating_add(1);
        route.workspace_id = Some(workspace_id);
        route.session_id = None;
        let token = route.current_token();
        drop(route);
        // Workspace switch clears the session selection: stop the live
        // projection owner with it.
        self.stop_projection_owner().await;
        Ok(token)
    }

    /// Bounded session list for the current project route. Late
    /// responses for a superseded generation are dropped, never merged.
    pub(crate) async fn route_session_list(
        &self,
        expected_generation: u64,
    ) -> Result<SessionListView, String> {
        let (project_id, connection_generation) = {
            let route = self.route.lock().await;
            if route.connection_generation != self.current_generation()
                || route.project_id.is_none()
            {
                return Err("no current route; select a project first".into());
            }
            if route.route_generation != expected_generation {
                return Err("stale route; refresh before listing sessions".into());
            }
            (
                route.project_id.clone().unwrap_or_default(),
                route.connection_generation,
            )
        };
        let (client, _) = self.route_request().await?;
        let response = client
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload: CoreRequest::SessionList {
                    project_id,
                    show_archived: false,
                    limit: MAX_SESSIONS,
                },
            })
            .await
            .map_err(|error| error.to_string())?;
        let route = self.route.lock().await;
        // Mid-flight disconnect, route switch, or generation drift:
        // drop the late list, never merge it into the current route.
        if self.current_generation() != connection_generation
            || route.connection_generation != connection_generation
            || route.route_generation != expected_generation
        {
            return Err("stale route; session list discarded".into());
        }
        let token = route.current_token();
        drop(route);
        let CoreResponse::SessionList { sessions } = response else {
            return Err(unexpected_daemon_response(&response));
        };
        Ok(SessionListView {
            sessions: sessions
                .iter()
                .take(MAX_SESSIONS)
                .map(session_summary_view)
                .collect(),
            route_token: token,
        })
    }

    /// Open an existing session after verifying its canonical binding
    /// matches the captured route. Mismatches fail closed without
    /// touching route state.
    pub(crate) async fn route_session_open(
        &self,
        session_id: String,
        expected_generation: u64,
    ) -> Result<SessionView, String> {
        let (project_id, workspace_id, connection_generation) =
            self.route_selection(expected_generation, true).await?;
        let (client, _) = self.route_request().await?;
        let response = client
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload: CoreRequest::SessionAttach {
                    session_id: session_id.clone(),
                },
            })
            .await
            .map_err(|error| error.to_string())?;
        let CoreResponse::Session { session } = response else {
            return Err(unexpected_daemon_response(&response));
        };
        let mut route = self.route.lock().await;
        if self.current_generation() != connection_generation
            || route.connection_generation != connection_generation
            || route.route_generation != expected_generation
        {
            return Err("stale route; session open discarded".into());
        }
        if !session_binding_matches(&session, &project_id, &workspace_id) {
            return Err("session binding does not match the current route".into());
        }
        route.session_id = Some(session.id.clone());
        let token = route.current_token();
        Ok(SessionView {
            session: session_summary_view(&session),
            route_token: token,
        })
    }

    /// Create a session with explicit project/workspace identity and the
    /// Rust-resolved canonical root. The renderer never supplies a path.
    /// A result bound anywhere but the captured route is rejected and
    /// leaves route state untouched.
    pub(crate) async fn route_session_create(
        &self,
        title: Option<String>,
        expected_generation: u64,
    ) -> Result<SessionView, String> {
        let (project_id, workspace_id, connection_generation) =
            self.route_selection(expected_generation, true).await?;
        let root = {
            let route = self.route.lock().await;
            route
                .roots
                .get(&workspace_id)
                .cloned()
                .ok_or_else(|| "workspace has no authorized canonical root".to_owned())?
        };
        let (client, _) = self.route_request().await?;
        let response = client
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload: CoreRequest::SessionCreate {
                    directory: root,
                    title,
                    project_id: Some(project_id.clone()),
                    workspace_id: Some(workspace_id.clone()),
                },
            })
            .await
            .map_err(|error| error.to_string())?;
        let CoreResponse::Session { session } = response else {
            return Err(unexpected_daemon_response(&response));
        };
        let mut route = self.route.lock().await;
        if self.current_generation() != connection_generation
            || route.connection_generation != connection_generation
            || route.route_generation != expected_generation
        {
            return Err("stale route; session create discarded".into());
        }
        if !session_binding_matches(&session, &project_id, &workspace_id) {
            return Err("created session binding does not match the current route".into());
        }
        route.session_id = Some(session.id.clone());
        let token = route.current_token();
        Ok(SessionView {
            session: session_summary_view(&session),
            route_token: token,
        })
    }

    /// Snapshot the current project/workspace selection, requiring a
    /// live connection, a matching route generation, and (optionally) a
    /// selected workspace.
    async fn route_selection(
        &self,
        expected_generation: u64,
        require_workspace: bool,
    ) -> Result<(String, String, u64), String> {
        let route = self.route.lock().await;
        if route.connection_generation != self.current_generation() || route.project_id.is_none() {
            return Err("no current route; select a project first".into());
        }
        if route.route_generation != expected_generation {
            return Err("stale route; refresh before continuing".into());
        }
        let project_id = route.project_id.clone().unwrap_or_default();
        let Some(workspace_id) = route.workspace_id.clone() else {
            if require_workspace {
                return Err("no workspace selected for the current route".into());
            }
            return Ok((project_id, String::new(), route.connection_generation));
        };
        Ok((project_id, workspace_id, route.connection_generation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_protocol::dto::{ProjectDetailsDto, ProjectSummaryDto, ProjectWorkspaceSummaryDto};
    use codegg_protocol::frames::{CoreFrame, ServerCapabilities, ServerHello};

    fn detail_response(
        project_id: &str,
        workspaces: Vec<(&str, &str, Option<&str>)>,
        session_count: usize,
    ) -> CoreResponse {
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
                workspaces: workspaces
                    .into_iter()
                    .map(|(id, name, root)| ProjectWorkspaceSummaryDto {
                        workspace_id: id.into(),
                        display_name: name.into(),
                        canonical_root: root.map(str::to_string),
                    })
                    .collect(),
                session_count,
                health: None,
            },
        }
    }

    fn session_dto(id: &str, project_id: &str, workspace_id: &str, title: &str) -> Session {
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
            title: title.into(),
            ..Default::default()
        }
    }

    #[cfg(unix)]
    struct FakeDaemon {
        reader: tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>,
        writer: tokio::net::unix::OwnedWriteHalf,
    }

    #[cfg(unix)]
    impl FakeDaemon {
        async fn accept(listener: tokio::net::UnixListener) -> Self {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
            let (stream, _) = listener.accept().await.expect("accept");
            let (read, mut writer) = stream.into_split();
            let mut reader = tokio::io::BufReader::new(read);
            let mut line = String::new();
            reader.read_line(&mut line).await.expect("hello");
            assert!(matches!(
                serde_json::from_str::<CoreFrame>(line.trim()).expect("decode hello"),
                CoreFrame::ClientHello(_)
            ));
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
                client_id: "route-client-id".into(),
            });
            writer
                .write_all(format!("{}\n", serde_json::to_string(&hello).unwrap()).as_bytes())
                .await
                .expect("hello");
            writer.flush().await.expect("flush");
            // Consume the automatic post-hello transport subscribe.
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
                    CoreFrame::Request(request) => {
                        return (request.request_id, request.payload);
                    }
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
    }

    #[cfg(unix)]
    async fn routed_host(endpoint: &str) -> std::sync::Arc<HostState> {
        let state = std::sync::Arc::new(HostState::default());
        let client =
            codegg_client::LocalSocketClient::connect(endpoint.to_string(), crate::descriptor())
                .await
                .expect("connect");
        let daemon_id = client.daemon_id().await.expect("daemon id");
        assert_eq!(state.install_connection(client, daemon_id).await, 1);
        state
    }

    #[cfg(unix)]
    fn short_endpoint() -> (String, tokio::net::UnixListener) {
        let short = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let socket = std::env::temp_dir().join(format!("cgd-route-{short}.sock"));
        let listener = tokio::net::UnixListener::bind(&socket).expect("bind");
        (format!("unix://{}", socket.display()), listener)
    }

    #[test]
    fn binding_matches_canonical_and_legacy_forms() {
        let canonical = session_dto("s", "proj-a", "ws-1", "t");
        assert!(session_binding_matches(&canonical, "proj-a", "ws-1"));
        assert!(!session_binding_matches(&canonical, "proj-b", "ws-1"));
        assert!(!session_binding_matches(&canonical, "proj-a", "ws-2"));
        let mut legacy = canonical.clone();
        legacy.binding = None;
        assert!(session_binding_matches(&legacy, "proj-a", "ws-1"));
        assert!(!session_binding_matches(&legacy, "proj-a", "ws-9"));
        let mut unbound = legacy.clone();
        unbound.workspace_id = None;
        assert!(!session_binding_matches(&unbound, "proj-a", "ws-1"));
    }

    #[test]
    fn route_token_uses_camel_case_bridge_fields() {
        let token = RouteTokenView {
            connection_generation: 2,
            route_generation: 3,
            project_id: "p".into(),
            workspace_id: "w".into(),
            session_id: Some("s".into()),
        };
        let json = serde_json::to_value(token).unwrap();
        assert_eq!(json["connectionGeneration"], 2);
        assert_eq!(json["routeGeneration"], 3);
        assert_eq!(json["projectId"], "p");
        assert_eq!(json["workspaceId"], "w");
        assert_eq!(json["sessionId"], "s");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn detail_installs_workspaces_and_hides_roots() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectGet { .. }));
            daemon
                .respond(
                    id,
                    detail_response(
                        "proj-a",
                        vec![
                            ("ws-1", "Workspace 1", Some("/secret/root-a")),
                            ("ws-2", "Workspace 2", None),
                        ],
                        4,
                    ),
                )
                .await;
        });

        let state = routed_host(&endpoint).await;
        let detail = state
            .route_project_detail("proj-a".into())
            .await
            .expect("detail");
        assert_eq!(detail.workspaces.len(), 2);
        assert_eq!(detail.session_count, 4);
        assert_eq!(detail.route_token.route_generation, 1);
        assert_eq!(detail.route_token.project_id, "proj-a");
        // Canonical roots never reach the renderer surface.
        let json = serde_json::to_value(&detail).unwrap().to_string();
        assert!(!json.contains("/secret/root-a"), "root leaked: {json}");
        assert!(!json.contains("canonical"), "root key leaked: {json}");
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn project_switch_drops_stale_detail() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            // Read both selections before answering either: the second
            // selection wins by completion order and the first must be
            // stale-dropped, never merged.
            let (id_a, first) = daemon.next_request().await;
            assert!(matches!(first, CoreRequest::ProjectGet { .. }));
            let (id_b, second) = daemon.next_request().await;
            assert!(matches!(second, CoreRequest::ProjectGet { .. }));
            daemon
                .respond(
                    id_b,
                    detail_response("proj-b", vec![("ws-b", "W", None)], 0),
                )
                .await;
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            daemon
                .respond(
                    id_a,
                    detail_response("proj-a", vec![("ws-a", "W", None)], 0),
                )
                .await;
        });

        let state = routed_host(&endpoint).await;
        let state_a = state.clone();
        let first =
            tokio::spawn(async move { state_a.route_project_detail("proj-a".into()).await });
        // Let the first selection reach the daemon before superseding it.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let detail_b = state
            .route_project_detail("proj-b".into())
            .await
            .expect("second selection wins");
        assert_eq!(detail_b.project_id, "proj-b");
        let stale = first.await.expect("join").expect_err("stale detail drops");
        assert!(stale.contains("discarded"), "unexpected error: {stale}");
        let route = state.route.lock().await;
        assert_eq!(route.project_id.as_deref(), Some("proj-b"));
        assert_eq!(route.route_generation, 1);
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn workspace_switch_drops_stale_session_list() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectGet { .. }));
            daemon
                .respond(
                    id,
                    detail_response(
                        "proj-a",
                        vec![("ws-1", "One", None), ("ws-2", "Two", None)],
                        0,
                    ),
                )
                .await;
            // Delay the session list past the workspace switch.
            let (list_id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::SessionList { .. }));
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            daemon
                .respond(
                    list_id,
                    CoreResponse::SessionList {
                        sessions: vec![session_dto("s-old", "proj-a", "ws-1", "old")],
                    },
                )
                .await;
        });

        let state = routed_host(&endpoint).await;
        let detail = state
            .route_project_detail("proj-a".into())
            .await
            .expect("detail");
        let select = state
            .route_workspace_select("ws-1".into(), detail.route_token.route_generation)
            .await
            .expect("select ws-1");
        let state_list = state.clone();
        let listed =
            tokio::spawn(
                async move { state_list.route_session_list(select.route_generation).await },
            );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let switched = state
            .route_workspace_select("ws-2".into(), select.route_generation)
            .await
            .expect("switch ws-2");
        assert_eq!(switched.workspace_id, "ws-2");
        let stale = listed.await.expect("join").expect_err("stale list drops");
        assert!(stale.contains("discarded"), "unexpected error: {stale}");
        let route = state.route.lock().await;
        assert_eq!(route.workspace_id.as_deref(), Some("ws-2"));
        assert!(route.session_id.is_none());
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn session_open_binding_mismatch_fails_closed() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", None)], 0),
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::SessionAttach { .. }));
            // Bound to a different project: must be rejected.
            daemon
                .respond(
                    id,
                    CoreResponse::Session {
                        session: session_dto("s-x", "proj-evil", "ws-1", "x"),
                    },
                )
                .await;
        });

        let state = routed_host(&endpoint).await;
        let detail = state
            .route_project_detail("proj-a".into())
            .await
            .expect("detail");
        let token = state
            .route_workspace_select("ws-1".into(), detail.route_token.route_generation)
            .await
            .expect("select");
        let error = state
            .route_session_open("s-x".into(), token.route_generation)
            .await
            .expect_err("mismatch fails closed");
        assert!(error.contains("binding"), "unexpected error: {error}");
        assert!(state.route.lock().await.session_id.is_none());
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn session_create_uses_explicit_identity_and_resolved_root() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", Some("/root/ws-1"))], 0),
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            let CoreRequest::SessionCreate {
                directory,
                title,
                project_id,
                workspace_id,
            } = payload
            else {
                panic!("expected SessionCreate, got {payload:?}");
            };
            // The renderer never supplies a path: the host resolves the
            // canonical root and always sends explicit identity.
            assert_eq!(directory, "/root/ws-1");
            assert_eq!(project_id.as_deref(), Some("proj-a"));
            assert_eq!(workspace_id.as_deref(), Some("ws-1"));
            assert_eq!(title.as_deref(), Some("hello"));
            daemon
                .respond(
                    id,
                    CoreResponse::Session {
                        session: session_dto("s-new", "proj-a", "ws-1", "hello"),
                    },
                )
                .await;
        });

        let state = routed_host(&endpoint).await;
        let detail = state
            .route_project_detail("proj-a".into())
            .await
            .expect("detail");
        let token = state
            .route_workspace_select("ws-1".into(), detail.route_token.route_generation)
            .await
            .expect("select");
        let view = state
            .route_session_create(Some("hello".into()), token.route_generation)
            .await
            .expect("create");
        assert_eq!(view.session.session_id, "s-new");
        assert_eq!(view.route_token.session_id.as_deref(), Some("s-new"));
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stale_session_create_result_cannot_bind() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", Some("/root/ws-1"))], 0),
                )
                .await;
            // Read both the create and the superseding detail before
            // answering: the detail wins, the late create is rejected.
            let (create_id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::SessionCreate { .. }));
            let (detail_id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectGet { .. }));
            daemon
                .respond(
                    detail_id,
                    detail_response("proj-b", vec![("ws-b", "W", None)], 0),
                )
                .await;
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            daemon
                .respond(
                    create_id,
                    CoreResponse::Session {
                        session: session_dto("s-late", "proj-a", "ws-1", "late"),
                    },
                )
                .await;
        });

        let state = routed_host(&endpoint).await;
        let detail = state
            .route_project_detail("proj-a".into())
            .await
            .expect("detail");
        let token = state
            .route_workspace_select("ws-1".into(), detail.route_token.route_generation)
            .await
            .expect("select");
        let state_create = state.clone();
        let created = tokio::spawn(async move {
            state_create
                .route_session_create(None, token.route_generation)
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let detail_b = state
            .route_project_detail("proj-b".into())
            .await
            .expect("switch wins");
        assert_eq!(detail_b.project_id, "proj-b");
        let stale = created.await.expect("join").expect_err("late create drops");
        assert!(stale.contains("discarded"), "unexpected error: {stale}");
        let route = state.route.lock().await;
        assert_eq!(route.project_id.as_deref(), Some("proj-b"));
        assert!(route.session_id.is_none());
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn connection_change_invalidates_route() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", None)], 0),
                )
                .await;
        });

        let state = routed_host(&endpoint).await;
        let detail = state
            .route_project_detail("proj-a".into())
            .await
            .expect("detail");
        state.disconnect_host().await;
        let error = state
            .route_session_list(detail.route_token.route_generation)
            .await
            .expect_err("dead route fails closed");
        assert!(
            error.contains("no current route") || error.contains("not connected"),
            "unexpected error: {error}"
        );
        let error = state
            .route_workspace_select("ws-1".into(), detail.route_token.route_generation)
            .await
            .expect_err("dead route fails closed");
        assert!(
            error.contains("no current route") || error.contains("not connected"),
            "unexpected error: {error}"
        );
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn session_list_is_bounded() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", None)], 0),
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            let CoreRequest::SessionList {
                project_id,
                show_archived,
                limit,
            } = payload
            else {
                panic!("expected SessionList, got {payload:?}");
            };
            assert_eq!(project_id, "proj-a");
            assert!(!show_archived);
            assert_eq!(limit, MAX_SESSIONS);
            let sessions: Vec<Session> = (0..MAX_SESSIONS + 10)
                .map(|index| session_dto(&format!("s-{index}"), "proj-a", "ws-1", "t"))
                .collect();
            daemon
                .respond(id, CoreResponse::SessionList { sessions })
                .await;
        });

        let state = routed_host(&endpoint).await;
        let detail = state
            .route_project_detail("proj-a".into())
            .await
            .expect("detail");
        let token = state
            .route_workspace_select("ws-1".into(), detail.route_token.route_generation)
            .await
            .expect("select");
        let listed = state
            .route_session_list(token.route_generation)
            .await
            .expect("list");
        assert_eq!(listed.sessions.len(), MAX_SESSIONS);
        server.await.expect("server");
    }

    #[cfg(unix)]
    async fn select_workspace_only(state: &std::sync::Arc<HostState>) -> RouteTokenView {
        let detail = state
            .route_project_detail("proj-a".into())
            .await
            .expect("detail");
        state
            .route_workspace_select("ws-1".into(), detail.route_token.route_generation)
            .await
            .expect("select")
    }

    #[cfg(unix)]
    async fn expect_no_further_request(daemon: &mut FakeDaemon) {
        if let Ok((_, payload)) = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            daemon.next_request(),
        )
        .await
        {
            panic!("unexpected late daemon request: {payload:?}");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn prompt_submit_existing_session_accepted() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", Some("/root-a"))], 0),
                )
                .await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    CoreResponse::Session {
                        session: session_dto("s-1", "proj-a", "ws-1", "First"),
                    },
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            let CoreRequest::SessionPromptSubmit {
                session_id,
                text,
                plan_mode,
            } = payload
            else {
                panic!("expected SessionPromptSubmit, got {payload:?}");
            };
            assert_eq!(session_id, "s-1");
            assert_eq!(text, "hello desktop");
            assert!(!plan_mode);
            daemon.respond(id, CoreResponse::Ack).await;
        });

        let state = routed_host(&endpoint).await;
        let token = select_workspace_only(&state).await;
        state
            .route_session_open("s-1".into(), token.route_generation)
            .await
            .expect("open");
        let view = state
            .route_prompt_submit("hello desktop".into(), false, token.route_generation)
            .await
            .expect("submit");
        assert!(!view.intent_id.is_empty());
        assert_eq!(view.route_token.session_id.as_deref(), Some("s-1"));
        // Terminal accept clears the stored intent.
        assert!(state.route.lock().await.prompt.is_none());
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn prompt_double_submit_coalesced() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", Some("/root-a"))], 0),
                )
                .await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    CoreResponse::Session {
                        session: session_dto("s-1", "proj-a", "ws-1", "First"),
                    },
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(
                matches!(payload, CoreRequest::SessionPromptSubmit { .. }),
                "expected SessionPromptSubmit, got {payload:?}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            daemon.respond(id, CoreResponse::Ack).await;
            expect_no_further_request(&mut daemon).await;
        });

        let state = routed_host(&endpoint).await;
        let token = select_workspace_only(&state).await;
        state
            .route_session_open("s-1".into(), token.route_generation)
            .await
            .expect("open");
        let first_state = state.clone();
        let generation = token.route_generation;
        let first = tokio::spawn(async move {
            first_state
                .route_prompt_submit("same text".into(), false, generation)
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let duplicate = state
            .route_prompt_submit("same text".into(), false, generation)
            .await
            .expect_err("concurrent duplicate coalesces");
        assert!(
            duplicate.contains("already being submitted"),
            "unexpected error: {duplicate}"
        );
        first.await.expect("join").expect("first accepted");
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn prompt_create_then_submit_exactly_once() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", Some("/root-a"))], 1),
                )
                .await;
            // Exactly one creation ...
            let (id, payload) = daemon.next_request().await;
            let CoreRequest::SessionCreate {
                project_id,
                workspace_id,
                ..
            } = payload
            else {
                panic!("expected SessionCreate, got {payload:?}");
            };
            assert_eq!(project_id.as_deref(), Some("proj-a"));
            assert_eq!(workspace_id.as_deref(), Some("ws-1"));
            daemon
                .respond(
                    id,
                    CoreResponse::Session {
                        session: session_dto("s-new", "proj-a", "ws-1", "Fresh"),
                    },
                )
                .await;
            // ... followed by exactly one submit for the new session.
            let (id, payload) = daemon.next_request().await;
            let CoreRequest::SessionPromptSubmit { session_id, .. } = payload else {
                panic!("expected SessionPromptSubmit, got {payload:?}");
            };
            assert_eq!(session_id, "s-new");
            daemon.respond(id, CoreResponse::Ack).await;
            expect_no_further_request(&mut daemon).await;
        });

        let state = routed_host(&endpoint).await;
        let token = select_workspace_only(&state).await;
        let view = state
            .route_prompt_submit("fresh session prompt".into(), true, token.route_generation)
            .await
            .expect("create-then-submit");
        assert_eq!(view.route_token.session_id.as_deref(), Some("s-new"));
        assert_eq!(
            state.route.lock().await.session_id.as_deref(),
            Some("s-new")
        );
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn prompt_create_failure_marks_failed_and_sends_no_submit() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", Some("/root-a"))], 0),
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(
                matches!(payload, CoreRequest::SessionCreate { .. }),
                "expected SessionCreate, got {payload:?}"
            );
            daemon
                .respond(
                    id,
                    CoreResponse::Error {
                        code: "session_create_failed".into(),
                        message: "disk is gone".into(),
                    },
                )
                .await;
            expect_no_further_request(&mut daemon).await;
        });

        let state = routed_host(&endpoint).await;
        let token = select_workspace_only(&state).await;
        let error = state
            .route_prompt_submit("doomed".into(), false, token.route_generation)
            .await
            .expect_err("create failure surfaces");
        assert!(
            error.contains("session_create_failed"),
            "unexpected error: {error}"
        );
        // No session bound, and the failed intent is retained for
        // same-text retry coalescing.
        let route = state.route.lock().await;
        assert!(route.session_id.is_none());
        assert!(route.prompt.as_ref().is_some_and(|intent| {
            intent.state() == codegg_client::PromptIntentState::Failed && intent.text() == "doomed"
        }));
        drop(route);
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn prompt_route_switch_after_create_prevents_submit() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", Some("/root-a"))], 0),
                )
                .await;
            // Hold the create response until the route moves on.
            let (create_id, payload) = daemon.next_request().await;
            assert!(
                matches!(payload, CoreRequest::SessionCreate { .. }),
                "expected SessionCreate, got {payload:?}"
            );
            let (get_id, payload) = daemon.next_request().await;
            assert!(
                matches!(payload, CoreRequest::ProjectGet { .. }),
                "expected ProjectGet, got {payload:?}"
            );
            daemon
                .respond(
                    get_id,
                    detail_response("proj-b", vec![("ws-9", "Nine", Some("/root-b"))], 0),
                )
                .await;
            // Let the switch install before the late create arrives:
            // the create completion must observe the new generation.
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            daemon
                .respond(
                    create_id,
                    CoreResponse::Session {
                        session: session_dto("s-late", "proj-a", "ws-1", "Late"),
                    },
                )
                .await;
            expect_no_further_request(&mut daemon).await;
        });

        let state = routed_host(&endpoint).await;
        let token = select_workspace_only(&state).await;
        let submitting = state.clone();
        let generation = token.route_generation;
        let submit = tokio::spawn(async move {
            submitting
                .route_prompt_submit("late".into(), false, generation)
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        state
            .route_project_detail("proj-b".into())
            .await
            .expect("switch wins");
        let error = submit.await.expect("join").expect_err("late result drops");
        assert!(error.contains("discarded"), "unexpected error: {error}");
        let route = state.route.lock().await;
        assert_eq!(route.project_id.as_deref(), Some("proj-b"));
        assert!(route.session_id.is_none());
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn prompt_disconnect_during_submit_discards_result() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", Some("/root-a"))], 0),
                )
                .await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    CoreResponse::Session {
                        session: session_dto("s-1", "proj-a", "ws-1", "First"),
                    },
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(
                matches!(payload, CoreRequest::SessionPromptSubmit { .. }),
                "expected SessionPromptSubmit, got {payload:?}"
            );
            // The daemon still accepts the turn; the host must not
            // report it against the dead route.
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            daemon.respond(id, CoreResponse::Ack).await;
        });

        let state = routed_host(&endpoint).await;
        let token = select_workspace_only(&state).await;
        state
            .route_session_open("s-1".into(), token.route_generation)
            .await
            .expect("open");
        let submitting = state.clone();
        let generation = token.route_generation;
        let submit = tokio::spawn(async move {
            submitting
                .route_prompt_submit("gone".into(), false, generation)
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        state.disconnect_host().await;
        let error = submit.await.expect("join").expect_err("dead route drops");
        assert!(
            error.contains("discarded")
                || error.contains("not connected")
                || error.contains("no current route"),
            "unexpected error: {error}"
        );
        server.await.expect("server");
    }
}
