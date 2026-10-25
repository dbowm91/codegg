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

use std::collections::{HashMap, HashSet};

use codegg_client::StoppedDriver;
use codegg_protocol::{
    core::{
        CommandCatalogScopeDto, CommandExecuteRequestDto, CommandKindDto, CoreRequest,
        CoreResponse, RequestEnvelope, PROTOCOL_VERSION,
    },
    dto::Session,
    git::{
        GitChangeKindDto, GitReadConsistencyDto, GitReadOperationDto, GitReadRequestDto,
        GitReadResultDto,
    },
    skill::{SkillCatalogueRequestDto, SkillSourceDto},
};

use super::bridge::{
    CommandCatalogView, CommandDescriptorView, CommandExecuteView, CommandFormView,
    CommandReportView, GitBranchListView, GitBranchView, GitChangeKind, GitChangedFileView,
    GitConsistency, GitDiffView, GitReadKind, GitReadOperation, GitReadView, GitStatusView,
    ProjectDetailView, RouteTokenView, SessionListView, SessionSummaryView, SessionView,
    SkillCatalogueView, SkillSource, SkillSummaryView, WorkspaceView,
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
    pub(crate) controller: Option<super::bridge::ControllerSummaryView>,
    pub(crate) responding: HashSet<String>,
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

/// The protocol operation a desktop git operation names.
///
/// The mapping is total and closed in both directions: the renderer can
/// only name an operation the daemon already knows how to read.
fn git_read_operation(operation: GitReadOperation) -> GitReadOperationDto {
    match operation {
        GitReadOperation::Status => GitReadOperationDto::Status,
        GitReadOperation::Diff => GitReadOperationDto::Diff,
        GitReadOperation::Branches => GitReadOperationDto::Branches,
    }
}

/// How a file changed, in the bridge's own vocabulary.
fn git_change_kind(kind: GitChangeKindDto) -> GitChangeKind {
    match kind {
        GitChangeKindDto::Added => GitChangeKind::Added,
        GitChangeKindDto::Modified => GitChangeKind::Modified,
        GitChangeKindDto::Deleted => GitChangeKind::Deleted,
        GitChangeKindDto::Renamed => GitChangeKind::Renamed,
        GitChangeKindDto::Copied => GitChangeKind::Copied,
        GitChangeKindDto::Other => GitChangeKind::Other,
    }
}

/// The namespace a skill was discovered in, in the bridge's own
/// vocabulary.
fn skill_source(source: SkillSourceDto) -> SkillSource {
    match source {
        SkillSourceDto::Project => SkillSource::Project,
        SkillSourceDto::Global => SkillSource::Global,
        SkillSourceDto::Plugin => SkillSource::Plugin,
        SkillSourceDto::Configured => SkillSource::Configured,
        SkillSourceDto::Native => SkillSource::Native,
    }
}

/// Build the renderer view for one git read.
///
/// The daemon tags its answer by the operation that produced it, so a
/// payload for another operation is a failed read, never an empty one.
fn git_read_view(
    result: GitReadResultDto,
    operation: GitReadOperation,
) -> Result<GitReadView, String> {
    match (operation, result) {
        (GitReadOperation::Status, GitReadResultDto::Status(status)) => Ok(GitReadView {
            kind: GitReadKind::Status,
            status: Some(GitStatusView {
                branch: status.branch,
                is_dirty: status.is_dirty,
                commit_hash: status.commit_hash,
                stash_count: status.stash_count,
                consistency: git_consistency(status.consistency),
            }),
            diff: None,
            branches: None,
        }),
        (GitReadOperation::Diff, GitReadResultDto::Diff(diff)) => Ok(GitReadView {
            kind: GitReadKind::Diff,
            status: None,
            diff: Some(GitDiffView {
                files_changed: diff.files_changed,
                insertions: diff.insertions,
                deletions: diff.deletions,
                files: diff
                    .files
                    .into_iter()
                    .map(|file| GitChangedFileView {
                        path: file.path,
                        kind: git_change_kind(file.kind),
                    })
                    .collect(),
                total_files: diff.total_files,
                truncated: diff.truncated,
                consistency: git_consistency(diff.consistency),
            }),
            branches: None,
        }),
        (GitReadOperation::Branches, GitReadResultDto::Branches(branches)) => Ok(GitReadView {
            kind: GitReadKind::Branches,
            status: None,
            diff: None,
            branches: Some(GitBranchListView {
                branches: branches
                    .branches
                    .into_iter()
                    .map(|branch| GitBranchView {
                        name: branch.name,
                        is_current: branch.is_current,
                        is_detached: branch.is_detached,
                        upstream: branch.upstream,
                        ahead: branch.ahead,
                        behind: branch.behind,
                        head: branch.head,
                    })
                    .collect(),
                total: branches.total,
                truncated: branches.truncated,
                consistency: git_consistency(branches.consistency),
            }),
        }),
        _ => Err("daemon returned a git read result for another operation".into()),
    }
}

/// Carry the daemon's consistency class into the view.
///
/// The class is carried rather than dropped: a renderer that received the
/// facts without it would be asserting a guarantee it was never given.
fn git_consistency(class: GitReadConsistencyDto) -> GitConsistency {
    match class {
        GitReadConsistencyDto::ImmutableGeneration => GitConsistency::ImmutableGeneration,
        GitReadConsistencyDto::CapturedNotCurrent => GitConsistency::CapturedNotCurrent,
        GitReadConsistencyDto::LiveMutable => GitConsistency::LiveMutable,
        GitReadConsistencyDto::NoObjectGeneration => GitConsistency::NoObjectGeneration,
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

    /// Read one read-only git fact set for the route-selected project.
    ///
    /// The project identity comes from the route, never from the
    /// renderer: the request carries a project id and no path, and the
    /// daemon resolves the repository root from its own durable binding
    /// (`crates/codegg-protocol/src/git.rs:40-56`). The renderer chooses
    /// only *which* fact set to read.
    ///
    /// The read is fenced exactly like the control-plane refresh
    /// (`apps/desktop/src-tauri/src/control.rs:90-124`): a completion for
    /// a superseded route or connection generation is discarded, never
    /// published. A reply that is not a git read — or that carries
    /// another operation's facts — is a failed read, not an empty one.
    pub(crate) async fn route_git_read(
        &self,
        operation: GitReadOperation,
        expected_generation: u64,
    ) -> Result<GitReadView, String> {
        let (project_id, _workspace_id, connection_generation) =
            self.route_selection(expected_generation, false).await?;
        let (client, _) = self.route_request().await?;
        let response = client
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload: CoreRequest::GitRead {
                    request: GitReadRequestDto {
                        project_id: project_id.clone(),
                        operation: git_read_operation(operation),
                        max_rows: None,
                    },
                },
            })
            .await
            .map_err(|error| error.to_string())?;
        let CoreResponse::GitRead { result } = response else {
            return Err(unexpected_daemon_response(&response));
        };
        let view = git_read_view(result, operation)?;
        {
            let route = self.route.lock().await;
            if self.current_generation() != connection_generation
                || route.connection_generation != connection_generation
                || route.route_generation != expected_generation
                || route.project_id.as_deref() != Some(project_id.as_str())
            {
                return Err("stale route; git read discarded".into());
            }
        }
        Ok(view)
    }

    /// Read the route-selected project's skill catalogue.
    ///
    /// Project-scoped like the git read: the project identity comes from
    /// the route and the daemon resolves the roots to search from its own
    /// durable binding. The response is metadata only — never a skill
    /// body and never a path — and it stays inside the daemon's own entry
    /// bound, so the view keeps the daemon's `total`/`truncated` pair
    /// rather than implying the list is complete.
    /// M007-D: read the daemon's command catalog for the route-selected
    /// project and workspace.
    ///
    /// The renderer holds no command list of its own: it asks the daemon
    /// for the catalog and renders what the daemon says. The scope is the
    /// route's own selection, never a renderer-supplied project.
    pub(crate) async fn route_command_catalog(
        &self,
        expected_generation: u64,
    ) -> Result<CommandCatalogView, String> {
        let (project_id, workspace_id, _connection_generation) =
            self.route_selection(expected_generation, false).await?;
        let (client, _) = self.route_request().await?;
        let response = client
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload: CoreRequest::CommandCatalog {
                    scope: CommandCatalogScopeDto {
                        project_id: project_id.clone(),
                        workspace_id: workspace_id.clone(),
                        session_id: None,
                    },
                },
            })
            .await
            .map_err(|error| error.to_string())?;
        let CoreResponse::CommandCatalog { catalog } = response else {
            return Err(unexpected_daemon_response(&response));
        };
        Ok(CommandCatalogView {
            catalog_version: catalog.catalog_version,
            commands: catalog
                .commands
                .into_iter()
                .map(|command| CommandDescriptorView {
                    command_id: command.command_id,
                    name: command.name,
                    aliases: command.aliases,
                    description: command.description,
                    domain: command.domain,
                    kind: command_kind_name(command.kind),
                    capability: command.capability,
                    forms: command
                        .forms
                        .into_iter()
                        .map(|form| CommandFormView {
                            form: form.form,
                            kind: command_kind_name(form.kind),
                            argument_hint: form.argument_hint,
                            argument_required: form.argument_required,
                            daemon_served: form.daemon_served,
                        })
                        .collect(),
                })
                .collect(),
        })
    }

    /// M007-D: execute one daemon-effect command form.
    ///
    /// The daemon authorizes the execution under `command.execute` and
    /// audits it as a canonical run; the host never executes a command
    /// itself. The `catalog_version` is the one the renderer resolved the
    /// command against, so a stale palette is refused rather than executed
    /// against a classification the daemon no longer serves.
    pub(crate) async fn route_command_execute(
        &self,
        expected_generation: u64,
        command_id: String,
        form: String,
        argument: String,
        catalog_version: u64,
    ) -> Result<CommandExecuteView, String> {
        let (project_id, workspace_id, _connection_generation) =
            self.route_selection(expected_generation, false).await?;
        let (client, _) = self.route_request().await?;
        let response = client
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload: CoreRequest::CommandExecute {
                    request: CommandExecuteRequestDto {
                        scope: CommandCatalogScopeDto {
                            project_id,
                            workspace_id,
                            session_id: None,
                        },
                        command_id: command_id.clone(),
                        form,
                        argument,
                        catalog_version,
                    },
                },
            })
            .await
            .map_err(|error| error.to_string())?;
        let CoreResponse::CommandExecute { result } = response else {
            return Err(unexpected_daemon_response(&response));
        };
        Ok(CommandExecuteView {
            run_id: result.run_id,
            command_id: result.command_id,
            status: command_outcome_name(result.status),
            summary: result.summary,
            lines: result.lines,
            report: result.report.map(|report| CommandReportView {
                added: report.added,
                removed: report.removed,
                changed: report.changed,
                shadowed: report.shadowed,
                invalid: report.invalid,
            }),
            started_at_ms: result.started_at_ms,
            completed_at_ms: result.completed_at_ms,
        })
    }

    pub(crate) async fn route_skill_catalogue(
        &self,
        expected_generation: u64,
    ) -> Result<SkillCatalogueView, String> {
        let (project_id, _workspace_id, connection_generation) =
            self.route_selection(expected_generation, false).await?;
        let (client, _) = self.route_request().await?;
        let response = client
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload: CoreRequest::SkillCatalogueList {
                    request: SkillCatalogueRequestDto {
                        project_id: project_id.clone(),
                        max_entries: None,
                    },
                },
            })
            .await
            .map_err(|error| error.to_string())?;
        let CoreResponse::SkillCatalogueList { result } = response else {
            return Err(unexpected_daemon_response(&response));
        };
        if result.project_id != project_id {
            return Err("daemon returned a skill catalogue for another project".into());
        }
        let view = SkillCatalogueView {
            project_id: result.project_id,
            skills: result
                .skills
                .into_iter()
                .map(|skill| SkillSummaryView {
                    name: skill.name,
                    description: skill.description,
                    source: skill_source(skill.source),
                    content_digest: skill.content_digest,
                    resource_count: skill.resource_count,
                })
                .collect(),
            total: result.total,
            truncated: result.truncated,
        };
        {
            let route = self.route.lock().await;
            if self.current_generation() != connection_generation
                || route.connection_generation != connection_generation
                || route.route_generation != expected_generation
                || route.project_id.as_deref() != Some(project_id.as_str())
            {
                return Err("stale route; skill catalogue read discarded".into());
            }
        }
        Ok(view)
    }

    /// Snapshot the current project/workspace selection, requiring a
    /// live connection, a matching route generation, and (optionally) a
    /// selected workspace.
    /// The route's selected project and workspace, fenced on the expected
    /// route generation.
    ///
    /// This is the **one** implementation of the rule. The editor's host
    /// commands resolve their scope through it too: a second implementation
    /// of "which project and workspace is this call about" is a second
    /// rule, and two rules drift.
    pub(crate) async fn route_selection(
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

/// The wire name of a command kind.
fn command_kind_name(kind: CommandKindDto) -> String {
    match kind {
        CommandKindDto::DaemonEffect => "daemon_effect".to_string(),
        CommandKindDto::FrontendEffect => "frontend_effect".to_string(),
    }
}

/// The wire name of a command outcome status.
fn command_outcome_name(status: codegg_protocol::core::CommandOutcomeStatusDto) -> String {
    use codegg_protocol::core::CommandOutcomeStatusDto;
    match status {
        CommandOutcomeStatusDto::Completed => "completed".to_string(),
        CommandOutcomeStatusDto::Refused => "refused".to_string(),
        CommandOutcomeStatusDto::Failed => "failed".to_string(),
        CommandOutcomeStatusDto::Cancelled => "cancelled".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_protocol::dto::{ProjectDetailsDto, ProjectSummaryDto, ProjectWorkspaceSummaryDto};
    use codegg_protocol::frames::{CoreFrame, ServerCapabilities, ServerHello};
    use codegg_protocol::git::{
        GitBranchListDto, GitChangedFileDto, GitDiffSummaryDto, GitRepoStatusDto,
    };
    use codegg_protocol::skill::{SkillCatalogueDto, SkillSummaryDto};

    /// The consistency class the daemon reports for a read served from one
    /// materialized generation. The route carries it into the view, so a
    /// test that asserts the view's class is asserting the whole chain.
    const TEST_CONSISTENCY: GitReadConsistencyDto = GitReadConsistencyDto::ImmutableGeneration;

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
        if let Ok((_, payload)) =
            tokio::time::timeout(std::time::Duration::from_millis(200), daemon.next_request()).await
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
                ..
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
    async fn open_control_route(state: &std::sync::Arc<HostState>) -> RouteTokenView {
        let token = select_workspace_only(state).await;
        state
            .route_session_open("s-1".into(), token.route_generation)
            .await
            .expect("open")
            .route_token
    }

    #[cfg(unix)]
    fn controller_fixture(principal: &str, revision: u64) -> CoreResponse {
        CoreResponse::SessionControl {
            controller: Some(codegg_protocol::core::SessionControllerDto {
                session_id: "s-1".into(),
                turn_id: "turn-1".into(),
                controller_principal: principal.into(),
                origin_client: None,
                revision,
                created_at_ms: 1,
                updated_at_ms: 2,
                last_action: "acquire".into(),
                last_actor: None,
                last_reason: None,
            }),
            requests: Vec::new(),
            truncated: false,
        }
    }

    /// Server prelude shared by control tests: project detail plus the
    /// session attach for `s-1`.
    #[cfg(unix)]
    async fn control_prelude(daemon: &mut FakeDaemon) {
        let (id, _) = daemon.next_request().await;
        daemon
            .respond(
                id,
                detail_response("proj-a", vec![("ws-1", "One", Some("/root-a"))], 0),
            )
            .await;
        let (id, payload) = daemon.next_request().await;
        assert!(
            matches!(payload, CoreRequest::SessionAttach { .. }),
            "expected SessionAttach, got {payload:?}"
        );
        daemon
            .respond(
                id,
                CoreResponse::Session {
                    session: session_dto("s-1", "proj-a", "ws-1", "First"),
                },
            )
            .await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn control_refresh_installs_controller() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            control_prelude(&mut daemon).await;
            let (id, payload) = daemon.next_request().await;
            let CoreRequest::SessionControlGet { session_id } = payload else {
                panic!("expected SessionControlGet, got {payload:?}");
            };
            assert_eq!(session_id, "s-1");
            daemon.respond(id, controller_fixture("alice", 3)).await;
        });

        let state = routed_host(&endpoint).await;
        let token = open_control_route(&state).await;
        let summary = state
            .route_control_refresh(token.route_generation)
            .await
            .expect("refresh")
            .expect("controller present");
        assert_eq!(summary.controller_principal, "alice");
        assert_eq!(summary.revision, 3);
        assert_eq!(summary.turn_id, "turn-1");
        let stored = state.route.lock().await.controller.clone().expect("stored");
        assert_eq!(stored.controller_principal, "alice");
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn control_refresh_idle_clears_controller() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            control_prelude(&mut daemon).await;
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    CoreResponse::SessionControl {
                        controller: None,
                        requests: Vec::new(),
                        truncated: false,
                    },
                )
                .await;
        });

        let state = routed_host(&endpoint).await;
        let token = open_control_route(&state).await;
        let summary = state
            .route_control_refresh(token.route_generation)
            .await
            .expect("refresh");
        assert!(summary.is_none());
        assert!(state.route.lock().await.controller.is_none());
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn permission_respond_sends_opaque_id_and_refreshes() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            control_prelude(&mut daemon).await;
            // The respond carries the opaque id + choice only: no
            // principal, client, turn, or session authority.
            let (id, payload) = daemon.next_request().await;
            let CoreRequest::PermissionRespond {
                id: pending,
                choice,
            } = payload
            else {
                panic!("expected PermissionRespond, got {payload:?}");
            };
            assert_eq!(pending, "perm:s-1:turn-1:p-1");
            assert_eq!(choice, "allow");
            daemon.respond(id, CoreResponse::Ack).await;
            let (id, payload) = daemon.next_request().await;
            assert!(
                matches!(payload, CoreRequest::SessionControlGet { .. }),
                "expected post-accept refresh, got {payload:?}"
            );
            daemon.respond(id, controller_fixture("alice", 4)).await;
            expect_no_further_request(&mut daemon).await;
        });

        let state = routed_host(&endpoint).await;
        let token = open_control_route(&state).await;
        let summary = state
            .route_permission_respond(
                "perm:s-1:turn-1:p-1".into(),
                "allow".into(),
                token.route_generation,
            )
            .await
            .expect("respond")
            .expect("controller present");
        assert_eq!(summary.revision, 4);
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn permission_double_respond_coalesced() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            control_prelude(&mut daemon).await;
            let (id, payload) = daemon.next_request().await;
            assert!(
                matches!(payload, CoreRequest::PermissionRespond { .. }),
                "expected PermissionRespond, got {payload:?}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            daemon.respond(id, CoreResponse::Ack).await;
            let (id, _) = daemon.next_request().await;
            daemon.respond(id, controller_fixture("alice", 1)).await;
            expect_no_further_request(&mut daemon).await;
        });

        let state = routed_host(&endpoint).await;
        let token = open_control_route(&state).await;
        let generation = token.route_generation;
        let first_state = state.clone();
        let first = tokio::spawn(async move {
            first_state
                .route_permission_respond("perm:s-1:turn-1:p-1".into(), "deny".into(), generation)
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let duplicate = state
            .route_permission_respond("perm:s-1:turn-1:p-1".into(), "deny".into(), generation)
            .await
            .expect_err("concurrent duplicate coalesces");
        assert!(
            duplicate.contains("already in flight"),
            "unexpected error: {duplicate}"
        );
        first.await.expect("join").expect("first accepted");
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn permission_cross_session_id_rejected_without_request() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            control_prelude(&mut daemon).await;
            expect_no_further_request(&mut daemon).await;
        });

        let state = routed_host(&endpoint).await;
        let token = open_control_route(&state).await;
        let error = state
            .route_permission_respond(
                "perm:s-other:turn-9:p-1".into(),
                "allow".into(),
                token.route_generation,
            )
            .await
            .expect_err("cross-session id fails closed");
        assert!(
            error.contains("another session"),
            "unexpected error: {error}"
        );
        // The renderer cannot supply principal/client identity: an
        // invalid choice never reaches the daemon either.
        let error = state
            .route_permission_respond(
                "perm:s-1:turn-1:p-1".into(),
                "maybe".into(),
                token.route_generation,
            )
            .await
            .expect_err("invalid choice fails closed");
        assert!(
            error.contains("invalid permission choice"),
            "unexpected error: {error}"
        );
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn permission_denied_releases_id_for_retry() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            control_prelude(&mut daemon).await;
            // First attempt: controller lost between render and click.
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    CoreResponse::Error {
                        code: "session_control_not_controller".into(),
                        message: "not the controller".into(),
                    },
                )
                .await;
            // Retry after the denial is a fresh request, not a wedged id.
            let (id, payload) = daemon.next_request().await;
            assert!(
                matches!(payload, CoreRequest::PermissionRespond { .. }),
                "expected retried PermissionRespond, got {payload:?}"
            );
            daemon.respond(id, CoreResponse::Ack).await;
            let (id, _) = daemon.next_request().await;
            daemon.respond(id, controller_fixture("bob", 2)).await;
        });

        let state = routed_host(&endpoint).await;
        let token = open_control_route(&state).await;
        let generation = token.route_generation;
        let error = state
            .route_permission_respond("perm:s-1:turn-1:p-1".into(), "allow".into(), generation)
            .await
            .expect_err("stale controller denied");
        assert!(
            error.contains("session_control_not_controller"),
            "unexpected error: {error}"
        );
        let summary = state
            .route_permission_respond("perm:s-1:turn-1:p-1".into(), "allow".into(), generation)
            .await
            .expect("retry accepted")
            .expect("controller present");
        assert_eq!(summary.controller_principal, "bob");
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn question_oversize_answers_rejected_locally() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            control_prelude(&mut daemon).await;
            expect_no_further_request(&mut daemon).await;
        });

        let state = routed_host(&endpoint).await;
        let token = open_control_route(&state).await;
        let error = state
            .route_question_respond(
                "question:s-1:turn-1:q-1".into(),
                serde_json::Value::String("x".repeat(70_000)),
                token.route_generation,
            )
            .await
            .expect_err("oversize answers fail closed");
        assert!(error.contains("bounded size"), "unexpected error: {error}");
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn control_loss_between_render_and_click_refreshes() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            control_prelude(&mut daemon).await;
            // Render-time refresh: alice controls revision 1.
            let (id, _) = daemon.next_request().await;
            daemon.respond(id, controller_fixture("alice", 1)).await;
            // Click-time respond: lease moved on; daemon denies.
            let (id, _) = daemon.next_request().await;
            daemon
                .respond(
                    id,
                    CoreResponse::Error {
                        code: "session_control_not_controller".into(),
                        message: "lease moved".into(),
                    },
                )
                .await;
            // Post-denial refresh converges on bob's lease.
            let (id, _) = daemon.next_request().await;
            daemon.respond(id, controller_fixture("bob", 2)).await;
        });

        let state = routed_host(&endpoint).await;
        let token = open_control_route(&state).await;
        let generation = token.route_generation;
        let rendered = state
            .route_control_refresh(generation)
            .await
            .expect("render-time refresh")
            .expect("controller present");
        assert_eq!(rendered.controller_principal, "alice");
        let error = state
            .route_question_respond(
                "question:s-1:turn-1:q-1".into(),
                serde_json::json!([{"question": "Pick one", "answer": "a"}]),
                generation,
            )
            .await
            .expect_err("moved lease denied");
        assert!(
            error.contains("session_control_not_controller"),
            "unexpected error: {error}"
        );
        let converged = state
            .route_control_refresh(generation)
            .await
            .expect("post-denial refresh")
            .expect("controller present");
        assert_eq!(converged.controller_principal, "bob");
        assert_eq!(converged.revision, 2);
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

    // ---- Phase 3: read-only project facts (git facts, skill catalogue) ----

    #[test]
    fn the_git_operation_set_is_closed() {
        // The renderer names which facts to read; an unknown name is
        // refused rather than defaulted, so a read can never be widened.
        assert_eq!(
            GitReadOperation::parse("status").expect("status"),
            GitReadOperation::Status
        );
        assert_eq!(
            GitReadOperation::parse("diff").expect("diff"),
            GitReadOperation::Diff
        );
        assert_eq!(
            GitReadOperation::parse("branches").expect("branches"),
            GitReadOperation::Branches
        );
        assert!(GitReadOperation::parse("commit").is_err());
        assert!(GitReadOperation::parse("").is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn git_read_carries_the_routes_project_id() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectGet { .. }));
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", None)], 0),
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            let request = match payload {
                CoreRequest::GitRead { request } => request,
                other => panic!("expected a git read, got {other:?}"),
            };
            // The renderer named only the operation: the project comes
            // from the route, and the request carries no path at all.
            assert_eq!(request.project_id, "proj-a");
            assert_eq!(request.operation, GitReadOperationDto::Status);
            assert_eq!(request.max_rows, None);
            let rendered = serde_json::to_value(&request).expect("request renders");
            assert!(
                rendered.get("path").is_none() && rendered.get("repository_root").is_none(),
                "a client-supplied path must not survive into the request: {rendered}"
            );
            daemon
                .respond(
                    id,
                    CoreResponse::GitRead {
                        result: GitReadResultDto::Status(GitRepoStatusDto {
                            branch: "main".into(),
                            is_dirty: true,
                            commit_hash: Some("abc123".into()),
                            stash_count: 2,
                            consistency: TEST_CONSISTENCY,
                        }),
                    },
                )
                .await;
        });

        let state = routed_host(&endpoint).await;
        let detail = state
            .route_project_detail("proj-a".into())
            .await
            .expect("detail");
        let view = state
            .route_git_read(
                GitReadOperation::Status,
                detail.route_token.route_generation,
            )
            .await
            .expect("git read");
        assert_eq!(view.kind, GitReadKind::Status);
        let status = view.status.expect("status facts");
        assert_eq!(status.branch, "main");
        assert!(status.is_dirty);
        assert_eq!(status.commit_hash.as_deref(), Some("abc123"));
        assert_eq!(status.stash_count, 2);
        // The view carries one fact set; the others stay absent.
        assert!(view.diff.is_none());
        assert!(view.branches.is_none());
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn git_read_keeps_the_daemons_bound_and_its_change_kinds() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectGet { .. }));
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", None)], 0),
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::GitRead { .. }));
            daemon
                .respond(
                    id,
                    CoreResponse::GitRead {
                        result: GitReadResultDto::Diff(GitDiffSummaryDto {
                            files_changed: 900,
                            insertions: 12,
                            deletions: 3,
                            files: vec![GitChangedFileDto {
                                path: "src/lib.rs".into(),
                                kind: GitChangeKindDto::Modified,
                            }],
                            total_files: 900,
                            truncated: true,
                            consistency: TEST_CONSISTENCY,
                        }),
                    },
                )
                .await;
        });

        let state = routed_host(&endpoint).await;
        let detail = state
            .route_project_detail("proj-a".into())
            .await
            .expect("detail");
        let view = state
            .route_git_read(GitReadOperation::Diff, detail.route_token.route_generation)
            .await
            .expect("git read");
        assert_eq!(view.kind, GitReadKind::Diff);
        let diff = view.diff.expect("diff facts");
        assert_eq!(diff.files_changed, 900);
        assert_eq!(diff.total_files, 900);
        // The daemon capped the list: the renderer must be able to say so.
        assert!(diff.truncated);
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].path, "src/lib.rs");
        assert_eq!(diff.files[0].kind, GitChangeKind::Modified);
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn git_read_is_discarded_when_the_route_moves_on() {
        let (endpoint, listener) = short_endpoint();
        let seen = std::sync::Arc::new(tokio::sync::Notify::new());
        let seen_server = seen.clone();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectGet { .. }));
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", None)], 0),
                )
                .await;
            let (git_id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::GitRead { .. }));
            seen_server.notify_one();
            // The route moves on while the read is in flight.
            let (detail_id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectGet { .. }));
            daemon
                .respond(
                    detail_id,
                    detail_response("proj-b", vec![("ws-b", "Two", None)], 0),
                )
                .await;
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            daemon
                .respond(
                    git_id,
                    CoreResponse::GitRead {
                        result: GitReadResultDto::Status(GitRepoStatusDto {
                            branch: "stale".into(),
                            is_dirty: false,
                            commit_hash: None,
                            stash_count: 0,
                            consistency: TEST_CONSISTENCY,
                        }),
                    },
                )
                .await;
        });

        let state = routed_host(&endpoint).await;
        let detail = state
            .route_project_detail("proj-a".into())
            .await
            .expect("detail");
        let generation = detail.route_token.route_generation;
        let reading = state.clone();
        let read = tokio::spawn(async move {
            reading
                .route_git_read(GitReadOperation::Status, generation)
                .await
        });
        seen.notified().await;
        state
            .route_project_detail("proj-b".into())
            .await
            .expect("second detail");
        let error = read
            .await
            .expect("join")
            .expect_err("a stale read must not publish");
        assert!(error.contains("discarded"), "unexpected error: {error}");
        let route = state.route.lock().await;
        assert_eq!(route.project_id.as_deref(), Some("proj-b"));
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn git_read_rejects_facts_for_another_operation() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectGet { .. }));
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", None)], 0),
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::GitRead { .. }));
            // The daemon answered a different fact set than the one asked
            // for: a failed read, not an empty one.
            daemon
                .respond(
                    id,
                    CoreResponse::GitRead {
                        result: GitReadResultDto::Branches(GitBranchListDto {
                            branches: Vec::new(),
                            total: 0,
                            truncated: false,
                            consistency: TEST_CONSISTENCY,
                        }),
                    },
                )
                .await;
        });

        let state = routed_host(&endpoint).await;
        let detail = state
            .route_project_detail("proj-a".into())
            .await
            .expect("detail");
        let error = state
            .route_git_read(
                GitReadOperation::Status,
                detail.route_token.route_generation,
            )
            .await
            .expect_err("a mismatched payload must not read as an empty answer");
        assert!(
            error.contains("another operation"),
            "unexpected error: {error}"
        );
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn skill_catalogue_read_carries_the_routes_project_id() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectGet { .. }));
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", None)], 0),
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            let request = match payload {
                CoreRequest::SkillCatalogueList { request } => request,
                other => panic!("expected a skill catalogue read, got {other:?}"),
            };
            // The project comes from the route, and the request carries no
            // root for the daemon to be pointed at.
            assert_eq!(request.project_id, "proj-a");
            assert_eq!(request.max_entries, None);
            let rendered = serde_json::to_value(&request).expect("request renders");
            assert!(
                rendered.get("path").is_none() && rendered.get("skills_root").is_none(),
                "a client-supplied root must not survive into the request: {rendered}"
            );
            daemon
                .respond(
                    id,
                    CoreResponse::SkillCatalogueList {
                        result: SkillCatalogueDto {
                            project_id: "proj-a".into(),
                            skills: vec![SkillSummaryDto {
                                name: "testing-ci".into(),
                                description: "Select focused verification.".into(),
                                source: SkillSourceDto::Project,
                                content_digest: "sha256:abc".into(),
                                resource_count: 2,
                            }],
                            total: 300,
                            truncated: true,
                        },
                    },
                )
                .await;
        });

        let state = routed_host(&endpoint).await;
        let detail = state
            .route_project_detail("proj-a".into())
            .await
            .expect("detail");
        let catalogue = state
            .route_skill_catalogue(detail.route_token.route_generation)
            .await
            .expect("catalogue");
        assert_eq!(catalogue.project_id, "proj-a");
        assert_eq!(catalogue.skills.len(), 1);
        assert_eq!(catalogue.skills[0].name, "testing-ci");
        assert_eq!(catalogue.skills[0].source, SkillSource::Project);
        assert_eq!(catalogue.skills[0].resource_count, 2);
        // The daemon capped the catalogue: the renderer must be able to
        // say so rather than implying the list is complete.
        assert_eq!(catalogue.total, 300);
        assert!(catalogue.truncated);
        server.await.expect("server");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn skill_catalogue_fails_closed_on_a_daemon_error() {
        let (endpoint, listener) = short_endpoint();
        let server = tokio::spawn(async move {
            let mut daemon = FakeDaemon::accept(listener).await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::ProjectGet { .. }));
            daemon
                .respond(
                    id,
                    detail_response("proj-a", vec![("ws-1", "One", None)], 0),
                )
                .await;
            let (id, payload) = daemon.next_request().await;
            assert!(matches!(payload, CoreRequest::SkillCatalogueList { .. }));
            daemon
                .respond(
                    id,
                    CoreResponse::Error {
                        code: "skill_catalogue_unavailable".into(),
                        message: "project workspace is unavailable".into(),
                    },
                )
                .await;
        });

        let state = routed_host(&endpoint).await;
        let detail = state
            .route_project_detail("proj-a".into())
            .await
            .expect("detail");
        let error = state
            .route_skill_catalogue(detail.route_token.route_generation)
            .await
            .expect_err("a rejected read must not read as an empty catalogue");
        assert!(
            error.contains("skill_catalogue_unavailable"),
            "unexpected error: {error}"
        );
        server.await.expect("server");
    }
}
