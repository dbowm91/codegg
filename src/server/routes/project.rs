use axum::{
    extract::{Extension, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use super::super::authz;
use super::super::scope::{context_error, resolve_context, ScopeQuery};
use super::super::state::ServerState;
use crate::error::{AppError, AxumAppError};
use codegg_core::context::ProjectContext;
use codegg_core::identity::ProjectId;
use codegg_core::project_catalog::{ProjectCatalog, ProjectCatalogRecord, RegisterLocalProject};
use codegg_core::team::{ProjectRole, TeamStore};
use codegg_core::transport_auth::AuthenticatedPrincipal;
use codegg_core::workspace::{SqliteWorkspaceStore, WorkspaceRegistry};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Serialize)]
pub struct ProjectInfo {
    /// Stable logical project identity. This is never a filesystem path.
    pub id: String,
    pub name: String,
    /// Compatibility locator retained for the single-project HTTP surface.
    pub path: String,
    pub git_root: Option<String>,
    pub session_count: usize,
}

#[derive(Serialize)]
pub struct ProjectListResponse {
    pub projects: Vec<ProjectInfo>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProjectListQuery {
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default)]
    pub limit: usize,
}

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub path: String,
}

pub(crate) async fn context_for_locator(
    pool: &sqlx::SqlitePool,
    locator: &Path,
) -> Result<ProjectContext, AxumAppError> {
    let scope = ScopeQuery {
        directory: Some(locator.to_string_lossy().into_owned()),
        ..ScopeQuery::default()
    };
    resolve_context(pool, &scope, None).await
}

async fn project_for_locator(
    pool: &sqlx::SqlitePool,
    locator: &Path,
) -> Result<(ProjectCatalogRecord, PathBuf), AxumAppError> {
    let context = context_for_locator(pool, locator).await?;
    let project = ProjectCatalog::new(pool.clone())
        .get_project(&context.project_id)
        .await
        .map_err(|e| context_error("project_context_unavailable", e.to_string()))?;
    Ok((project, context.workspace_root))
}

fn project_info(project: ProjectCatalogRecord, path: PathBuf, session_count: usize) -> ProjectInfo {
    let git_root =
        crate::worktree::find_git_root(&path).map(|root| root.to_string_lossy().into_owned());
    ProjectInfo {
        id: project.project_id.to_string(),
        name: project.display_name,
        path: path.to_string_lossy().into_owned(),
        git_root,
        session_count,
    }
}

pub async fn get_project(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
    Query(scope): Query<ScopeQuery>,
) -> Result<Json<ProjectInfo>, AxumAppError> {
    if scope.workspace_id.is_some() && scope.project_id.is_none() {
        return Err(context_error(
            "project_context_required",
            "workspace_id requires its project_id",
        ));
    }
    // Resolve the canonical project first so the capability check below
    // runs against server-side identity, never a caller-supplied grant.
    let target: ProjectId = if let Some(raw) = scope.project_id.as_deref() {
        ProjectId::parse(raw).map_err(|_| authz::denial_not_found())?
    } else {
        let locator = Path::new(scope.directory.as_deref().unwrap_or_default());
        let context = context_for_locator(&state.pool, locator).await?;
        context.project_id.clone()
    };
    authz::authorize_project(
        &state.pool,
        &principal,
        &target,
        codegg_core::authorization::Capability::ProjectRead,
        "project_get",
    )
    .await?;
    let (project, path) = if scope.project_id.is_some() {
        let project_id = ProjectId::parse(scope.project_id.as_deref().unwrap_or_default())
            .map_err(|e| context_error("invalid_project_context", e.to_string()))?;
        let project = ProjectCatalog::new(state.pool.clone())
            .get_project(&project_id)
            .await
            .map_err(|e| context_error("project_context_not_found", e.to_string()))?;
        let path = if scope.workspace_id.is_some() {
            resolve_context(&state.pool, &scope, None)
                .await?
                .workspace_root
        } else {
            ProjectCatalog::new(state.pool.clone())
                .list_workspaces_for_project(&project.project_id)
                .await
                .map_err(|e| context_error("project_context_unavailable", e.to_string()))?
                .first()
                .map(|workspace| workspace.canonical_root.clone())
                .unwrap_or_default()
        };
        (project, path)
    } else {
        project_for_locator(
            &state.pool,
            Path::new(scope.directory.as_deref().unwrap_or_default()),
        )
        .await?
    };
    let session_count = ProjectCatalog::new(state.pool.clone())
        .list_sessions_for_project(&project.project_id)
        .await
        .map_err(|e| context_error("project_context_unavailable", e.to_string()))?;
    Ok(Json(project_info(project, path, session_count)))
}

pub async fn list_projects(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
    Query(query): Query<ProjectListQuery>,
) -> Result<Json<ProjectListResponse>, AxumAppError> {
    // Enumeration gate first: unknown/disabled principals fail closed
    // before any catalog read is projected.
    authz::authorize_enumeration(&state.pool, &principal, "project_list").await?;
    let limit = if query.limit == 0 {
        crate::protocol::dto::MAX_PROJECT_LIST_ITEMS
    } else {
        query
            .limit
            .min(crate::protocol::dto::MAX_PROJECT_LIST_ITEMS)
    };
    let catalog = ProjectCatalog::new(state.pool.clone());
    let projects = catalog
        .list_projects(query.include_archived)
        .await
        .map_err(|e| context_error("project_catalog_unavailable", e.to_string()))?;
    // Privacy filter before building response rows: team principals observe
    // exactly their `project.read` grants; enumeration races may omit newly
    // granted rows but never include a denied row.
    let candidate_ids: Vec<ProjectId> = projects.iter().map(|p| p.project_id.clone()).collect();
    let visible = authz::filter_visible_projects(&state.pool, &principal, &candidate_ids).await;
    let mut result = Vec::with_capacity(projects.len().min(limit));
    for project in projects.into_iter().take(limit) {
        if !visible.contains(&project.project_id) {
            continue;
        }
        let workspaces = catalog
            .list_workspaces_for_project(&project.project_id)
            .await
            .map_err(|e| context_error("project_context_unavailable", e.to_string()))?;
        let path = workspaces
            .first()
            .map(|workspace| workspace.canonical_root.clone())
            .unwrap_or_default();
        let session_count = catalog
            .list_sessions_for_project(&project.project_id)
            .await
            .map_err(|e| context_error("project_context_unavailable", e.to_string()))?;
        result.push(project_info(project, path, session_count));
    }
    Ok(Json(ProjectListResponse { projects: result }))
}

pub async fn get_project_by_id(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(mut scope): Query<ScopeQuery>,
) -> Result<Json<ProjectInfo>, AxumAppError> {
    if scope.project_id.as_deref().is_some_and(|value| value != id) {
        return Err(context_error(
            "project_context_mismatch",
            "path project_id does not match query project_id",
        ));
    }
    scope.project_id = Some(id);
    get_project(Extension(principal), State(state), Query(scope)).await
}

async fn project_lifecycle(
    principal: AuthenticatedPrincipal,
    state: ServerState,
    id: String,
    restore: bool,
) -> Result<Json<ProjectInfo>, AxumAppError> {
    let project_id = ProjectId::parse(&id).map_err(|_| authz::denial_not_found())?;
    // Authorize before any mutation: `project.configure`, same as the Core
    // `project_archive` / `project_restore` equivalents. Denials are
    // privacy-safe and produce zero side effects.
    let operation = if restore {
        "project_restore"
    } else {
        "project_archive"
    };
    authz::authorize_project(
        &state.pool,
        &principal,
        &project_id,
        codegg_core::authorization::Capability::ProjectConfigure,
        operation,
    )
    .await?;
    let catalog = ProjectCatalog::new(state.pool.clone());
    let project = if restore {
        catalog
            .restore_project(&project_id, "server")
            .await
            .map_err(|e| context_error("project_restore_failed", e.to_string()))?
    } else {
        catalog
            .archive_project(&project_id, "server")
            .await
            .map_err(|e| context_error("project_archive_failed", e.to_string()))?
    };
    let path = catalog
        .list_workspaces_for_project(&project.project_id)
        .await
        .map_err(|e| context_error("project_context_unavailable", e.to_string()))?
        .first()
        .map(|workspace| workspace.canonical_root.clone())
        .unwrap_or_default();
    let session_count = catalog
        .list_sessions_for_project(&project.project_id)
        .await
        .map_err(|e| context_error("project_context_unavailable", e.to_string()))?;
    Ok(Json(project_info(project, path, session_count)))
}

pub async fn archive_project(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<ProjectInfo>, AxumAppError> {
    project_lifecycle(principal, state, id, false).await
}

pub async fn restore_project(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<ProjectInfo>, AxumAppError> {
    project_lifecycle(principal, state, id, true).await
}

pub async fn create_project(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<ProjectInfo>), AxumAppError> {
    // Project registration is the team bootstrap (Core `project_register`
    // is global): any active principal may create. The gate runs before
    // any filesystem/catalog mutation; the creator receives Owner so the
    // new project is immediately usable through the canonical authority.
    // No body/query principal, role, or capability field is trusted.
    authz::authorize_enumeration(&state.pool, &principal, "project_register").await?;
    let requested = Path::new(&req.path);
    if !requested.is_absolute() {
        return Err(context_error(
            "project_context_required",
            "project path must be an absolute local locator; the server has no default project",
        ));
    }
    let full = requested.to_path_buf();
    if !full.exists() {
        tokio::fs::create_dir_all(&full)
            .await
            .map_err(AppError::Io)?;
    }
    let canonical = full.canonicalize().map_err(AppError::Io)?;
    let workspace = if let Some(daemon) = &state.daemon {
        daemon
            .workspaces
            .get_or_register(&canonical)
            .await
            .map_err(|e| context_error("workspace_registration_failed", e.to_string()))?
    } else {
        let registry = WorkspaceRegistry::new_for_tests(Arc::new(SqliteWorkspaceStore::new(
            state.pool.clone(),
        )));
        registry
            .get_or_register(&canonical)
            .await
            .map_err(|e| context_error("workspace_registration_failed", e.to_string()))?
    };

    let catalog = ProjectCatalog::new(state.pool.clone());
    let project = catalog
        .register_local_project(
            RegisterLocalProject {
                display_name: req.name,
                description: None,
                tags: Vec::new(),
                primary_repository_id: None,
            },
            &workspace.id,
            "server",
        )
        .await
        .map_err(|e| context_error("project_registration_failed", e.to_string()))?;
    // Grant the creator Owner through the canonical team authority so the
    // bootstrap project is immediately visible to them. LocalOwner needs no
    // row; failure to grant never fails the registration itself.
    if !codegg_core::authorization::is_local_owner_broad(&principal) {
        let team = TeamStore::new(state.pool.clone());
        let _ = team
            .create_membership(
                &project.project_id,
                principal.principal_id(),
                ProjectRole::Owner,
            )
            .await;
    }
    Ok((
        StatusCode::CREATED,
        Json(project_info(project, canonical, 0)),
    ))
}
