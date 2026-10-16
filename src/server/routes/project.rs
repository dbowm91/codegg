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
    let decision = authz::authorize_project(
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
    emit_project_lifecycle_audit(
        &state.pool,
        &principal,
        &decision,
        &project.project_id,
        if restore { "reactivate" } else { "deactivate" },
        session_count,
    )
    .await;
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

#[derive(Serialize)]
pub struct ProjectDeletedResponse {
    pub project_id: String,
    pub deleted_session_count: usize,
}

pub async fn delete_project(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Json(request): axum::Json<crate::protocol::dto::ProjectDeleteRequestDto>,
) -> Result<Json<ProjectDeletedResponse>, AxumAppError> {
    let project_id = ProjectId::parse(&id).map_err(|_| authz::denial_not_found())?;
    if request.project_id != id {
        return Err(context_error(
            "project_context_mismatch",
            "path project_id does not match request project_id",
        ));
    }
    let decision = authz::authorize_project(
        &state.pool,
        &principal,
        &project_id,
        codegg_core::authorization::Capability::ProjectConfigure,
        "project_delete",
    )
    .await?;
    let result = ProjectCatalog::new(state.pool.clone())
        .delete_project(&project_id, request.expected_session_count)
        .await
        .map_err(|e| context_error("project_delete_failed", e.to_string()))?;
    emit_project_lifecycle_audit(
        &state.pool,
        &principal,
        &decision,
        &project_id,
        "destroy",
        result.deleted_session_count(),
    )
    .await;
    Ok(Json(ProjectDeletedResponse {
        project_id: project_id.to_string(),
        deleted_session_count: result.deleted_session_count(),
    }))
}

#[derive(Serialize)]
pub struct ProjectUpdatedResponse {
    pub project_id: String,
    pub display_name: String,
    pub canonical_root: String,
    pub updated_at: i64,
    pub rebound_session_count: usize,
}

pub async fn update_project(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Json(request): axum::Json<crate::protocol::dto::ProjectUpdateRequestDto>,
) -> Result<Json<ProjectUpdatedResponse>, AxumAppError> {
    let project_id = ProjectId::parse(&id).map_err(|_| authz::denial_not_found())?;
    if request.project_id != id {
        return Err(context_error(
            "project_context_mismatch",
            "path project_id does not match request project_id",
        ));
    }
    if !Path::new(&request.root).is_absolute() {
        return Err(context_error(
            "invalid_workspace_root",
            "project root must be an absolute local directory",
        ));
    }
    let decision = authz::authorize_project(
        &state.pool,
        &principal,
        &project_id,
        codegg_core::authorization::Capability::ProjectConfigure,
        "project_update",
    )
    .await?;
    let requested_root = Path::new(&request.root);
    let metadata = tokio::fs::metadata(requested_root)
        .await
        .map_err(AppError::Io)?;
    if !metadata.is_dir() {
        return Err(context_error(
            "invalid_workspace_root",
            "project root must be a directory",
        ));
    }
    let canonical_root = tokio::fs::canonicalize(requested_root)
        .await
        .map_err(AppError::Io)?;
    let workspaces = codegg_core::workspace::WorkspaceRegistry::load(Arc::new(
        codegg_core::workspace::SqliteWorkspaceStore::new(state.pool.clone()),
    ))
    .await
    .map_err(|e| context_error("invalid_workspace_root", e.to_string()))?;
    let workspace = workspaces
        .get_or_register(&canonical_root)
        .await
        .map_err(|e| context_error("invalid_workspace_root", e.to_string()))?;
    let pool = state.pool.clone();
    let current = ProjectCatalog::new(pool.clone())
        .get_project(&project_id)
        .await
        .map_err(|e| context_error("project_update_failed", e.to_string()))?;
    let old_workspaces = ProjectCatalog::new(pool.clone())
        .list_workspaces_for_project(&project_id)
        .await
        .map_err(|e| context_error("project_update_failed", e.to_string()))?;
    let result = ProjectCatalog::new(pool.clone())
        .update_project(
            &project_id,
            &request.display_name,
            &workspace.id,
            request.expected_updated_at,
            "http-project-update",
        )
        .await
        .map_err(|e| context_error("project_update_failed", e.to_string()))?;
    let provenance = codegg_core::authorization::audit_provenance(&decision);
    let old_root = old_workspaces
        .first()
        .map(|w| w.canonical_root.to_string_lossy().into_owned())
        .unwrap_or_default();
    let audit = codegg_core::audit::AuditEventBuilder::new(
        codegg_core::audit::AuditAction::ConfigChange,
        &principal,
        &provenance,
    )
    .with_correlation(format!("http-project-update-{}", uuid::Uuid::new_v4()))
    .with_project(project_id)
    .with_metadata("project.update.operation", "rename_repath")
    .with_metadata("project.update.previous_name", &current.display_name)
    .with_metadata("project.update.new_name", &result.project.display_name)
    .with_metadata("project.update.previous_root", old_root)
    .with_metadata(
        "project.update.new_root",
        workspace.canonical_root.to_string_lossy().as_ref(),
    )
    .with_metadata(
        "project.update.rebound_session_count",
        result.rebound_session_ids.len().to_string(),
    )
    .with_metadata("decision.id", provenance.decision_id())
    .with_metadata("decision.outcome", "allow");
    codegg_core::audit_instrumentation::ExecutionAuditEmitter::new(Some(pool))
        .emit(audit)
        .await;
    Ok(Json(ProjectUpdatedResponse {
        project_id: result.project.project_id.to_string(),
        display_name: result.project.display_name,
        canonical_root: workspace.canonical_root.to_string_lossy().into_owned(),
        updated_at: result.project.updated_at.timestamp_millis(),
        rebound_session_count: result.rebound_session_ids.len(),
    }))
}

async fn emit_project_lifecycle_audit(
    pool: &sqlx::SqlitePool,
    principal: &AuthenticatedPrincipal,
    decision: &codegg_core::authorization::AuthorizationDecision,
    project_id: &ProjectId,
    operation: &str,
    session_count: usize,
) {
    let provenance = codegg_core::authorization::audit_provenance(decision);
    let audit = codegg_core::audit::AuditEventBuilder::new(
        codegg_core::audit::AuditAction::ConfigChange,
        principal,
        &provenance,
    )
    .with_correlation(format!("http-project-lifecycle-{}", uuid::Uuid::new_v4()))
    .with_project(project_id.clone())
    .with_metadata("project.lifecycle.operation", operation)
    .with_metadata("project.session_count", session_count.to_string())
    .with_metadata("project.deleted_session_count", session_count.to_string())
    .with_metadata("decision.id", provenance.decision_id())
    .with_metadata("decision.outcome", "allow");
    codegg_core::audit_instrumentation::ExecutionAuditEmitter::new(Some(pool.clone()))
        .emit(audit)
        .await;
}

pub async fn create_project(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<ProjectInfo>), AxumAppError> {
    // Raw daemon-local project registration is LocalOwner/proven-local
    // deployment authority, not team bootstrap (Core `project_register`
    // is Opaque + `project.configure` with no project locator, so team
    // principals fail closed with `MissingScope`). The gate runs before
    // any filesystem/catalog/membership side effect; team denials use
    // the bounded LocalOwner-only shape regardless of whether the
    // supplied path exists. No body/query principal, role, or
    // capability field is trusted. LocalOwner needs no membership row
    // to operate under the broad policy.
    authz::require_local_owner(&state.pool, &principal, "project_register").await?;
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
    // LocalOwner operates under the broad policy with no membership row.
    // There is intentionally no non-LocalOwner Owner-grant bootstrap
    // branch: a future team project-creation path must atomically define
    // creator membership and workspace authority under a separately
    // reviewed contract.
    Ok((
        StatusCode::CREATED,
        Json(project_info(project, canonical, 0)),
    ))
}
