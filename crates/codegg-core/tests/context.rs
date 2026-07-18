//! Focused tests for the core project/workspace context authority.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use codegg_core::context::{
    ContextResolutionError, DirectoryCompatibilityOutcome, ProjectContextRequest,
    ProjectContextResolver,
};
use codegg_core::project_catalog::{ProjectCatalog, RegisterLocalProject};
use codegg_core::project_storage::{BindingStatus, ProjectStorage};
use codegg_core::session::models::CreateSession;
use codegg_core::session::schema;
use codegg_core::session::SessionStore;
use codegg_core::workspace::{
    InMemoryWorkspaceStore, WorkspaceId, WorkspaceRecord, WorkspaceStore,
};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use tempfile::tempdir;

async fn pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("sqlite pool");
    schema::migrate(&pool).await.expect("schema migration");
    pool
}

fn make_workspace(root: impl Into<PathBuf>, name: &str) -> WorkspaceRecord {
    let now = Utc::now();
    WorkspaceRecord {
        id: WorkspaceId::new(),
        canonical_root: root.into(),
        display_name: name.to_string(),
        created_at: now,
        last_opened_at: now,
        archived_at: None,
    }
}

async fn add_workspace(
    pool: &SqlitePool,
    store: &InMemoryWorkspaceStore,
    record: &WorkspaceRecord,
) {
    store.upsert(record).await.expect("workspace store insert");
    sqlx::query(
        "INSERT INTO workspace (id, canonical_root, display_name, time_created, time_last_opened, time_archived) VALUES (?, ?, ?, ?, ?, NULL)",
    )
    .bind(record.id.as_str())
    .bind(record.canonical_root.to_string_lossy().to_string())
    .bind(&record.display_name)
    .bind(record.created_at.timestamp_millis())
    .bind(record.last_opened_at.timestamp_millis())
    .execute(pool)
    .await
    .expect("workspace database insert");
}

async fn project_for(
    pool: &SqlitePool,
    store: &InMemoryWorkspaceStore,
    root: impl Into<PathBuf>,
    name: &str,
) -> (WorkspaceRecord, codegg_core::identity::ProjectId) {
    let record = make_workspace(root, name);
    add_workspace(pool, store, &record).await;
    let catalog = ProjectCatalog::new(pool.clone());
    let project = catalog
        .register_local_project(
            RegisterLocalProject {
                display_name: name.to_string(),
                description: None,
                tags: Vec::new(),
                primary_repository_id: None,
            },
            &record.id,
            "context-test",
        )
        .await
        .expect("project registration");
    (record, project.project_id)
}

fn resolver(pool: &SqlitePool, store: Arc<InMemoryWorkspaceStore>) -> ProjectContextResolver {
    ProjectContextResolver::new(
        ProjectStorage::new(pool.clone()),
        ProjectCatalog::new(pool.clone()),
        store,
    )
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_context_resolves_and_optional_session_must_match() {
    let pool = pool().await;
    let store = Arc::new(InMemoryWorkspaceStore::new());
    let root = tempdir().expect("temporary workspace");
    let (workspace, project_id) = project_for(&pool, &store, root.path(), "Project").await;
    let resolver = resolver(&pool, store.clone());
    assert!(matches!(
        resolver
            .resolve(ProjectContextRequest::new(
                project_id.clone(),
                WorkspaceId::new_unchecked("invalid/workspace"),
            ))
            .await,
        Err(ContextResolutionError::InvalidInput(_))
    ));

    let sessions = SessionStore::new(pool.clone());
    sessions
        .create_with_id(
            "session-context",
            CreateSession {
                project_id: "legacy-projection".to_string(),
                directory: root.path().display().to_string(),
                title: Some("Context".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("legacy session row");
    ProjectStorage::new(pool.clone())
        .bind_session(
            "session-context",
            &project_id,
            &workspace.id,
            "context-test",
        )
        .await
        .expect("canonical session binding");

    let context = resolver
        .resolve(
            ProjectContextRequest::new(project_id.clone(), workspace.id.clone()).with_session_id(
                codegg_core::context::SessionId::parse("session-context").expect("session ID"),
            ),
        )
        .await
        .expect("resolved context");

    assert_eq!(context.project_id, project_id);
    assert_eq!(context.workspace_id, workspace.id);
    assert_eq!(context.workspace_root, root.path());
    assert!(context.is_resolved());
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_context_rejects_archived_project_missing_workspace_and_mismatch() {
    let pool = pool().await;
    let store = Arc::new(InMemoryWorkspaceStore::new());
    let first_root = tempdir().expect("first workspace");
    let second_root = tempdir().expect("second workspace");
    let (first, first_project) = project_for(&pool, &store, first_root.path(), "First").await;
    let (second, second_project) = project_for(&pool, &store, second_root.path(), "Second").await;
    let catalog = ProjectCatalog::new(pool.clone());
    catalog
        .archive_project(&first_project, "context-test")
        .await
        .expect("archive project");

    let resolver = resolver(&pool, store.clone());
    assert!(matches!(
        resolver
            .resolve(ProjectContextRequest::new(
                first_project.clone(),
                first.id.clone()
            ))
            .await,
        Err(ContextResolutionError::ProjectArchived)
    ));
    assert!(matches!(
        resolver
            .resolve(ProjectContextRequest::new(
                first_project.clone(),
                second.id.clone()
            ))
            .await,
        Err(ContextResolutionError::ProjectArchived)
    ));
    assert!(matches!(
        resolver
            .resolve(ProjectContextRequest::new(
                second_project.clone(),
                first.id.clone()
            ))
            .await,
        Err(ContextResolutionError::BindingProjectMismatch)
    ));

    let unbound_root = tempdir().expect("unbound workspace");
    let unbound = make_workspace(unbound_root.path(), "Unbound");
    add_workspace(&pool, &store, &unbound).await;
    assert!(matches!(
        resolver
            .resolve(ProjectContextRequest::new(second_project, unbound.id))
            .await,
        Err(ContextResolutionError::BindingMissing)
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn unresolved_binding_and_session_mismatch_are_non_executable() {
    let pool = pool().await;
    let store = Arc::new(InMemoryWorkspaceStore::new());
    let root = tempdir().expect("workspace");
    let (workspace, project) = project_for(&pool, &store, root.path(), "Project").await;
    sqlx::query(
        "UPDATE workspace_project_binding SET status = 'rebind_required' WHERE workspace_id = ?",
    )
    .bind(workspace.id.as_str())
    .execute(&pool)
    .await
    .expect("mark binding unresolved");

    let resolver = resolver(&pool, store.clone());
    assert!(matches!(
        resolver
            .resolve(ProjectContextRequest::new(
                project.clone(),
                workspace.id.clone()
            ))
            .await,
        Err(ContextResolutionError::BindingNotResolved {
            status: BindingStatus::RebindRequired
        })
    ));

    // Restore the workspace binding, then create a session binding with a
    // different project identity to verify cross-context rejection.
    sqlx::query("UPDATE workspace_project_binding SET status = 'resolved' WHERE workspace_id = ?")
        .bind(workspace.id.as_str())
        .execute(&pool)
        .await
        .expect("restore binding");
    let other_root = tempdir().expect("other workspace");
    let (other_workspace, other_project) =
        project_for(&pool, &store, other_root.path(), "Other").await;
    let sessions = SessionStore::new(pool.clone());
    sessions
        .create_with_id(
            "session-mismatch",
            CreateSession {
                project_id: "legacy-projection".to_string(),
                directory: root.path().display().to_string(),
                title: Some("Mismatch".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("session row");
    ProjectStorage::new(pool.clone())
        .bind_session(
            "session-mismatch",
            &other_project,
            &other_workspace.id,
            "context-test",
        )
        .await
        .expect("other canonical session binding");

    assert!(matches!(
        resolver
            .resolve(
                ProjectContextRequest::new(project, workspace.id).with_session_id(
                    codegg_core::context::SessionId::parse("session-mismatch").expect("session ID"),
                ),
            )
            .await,
        Err(ContextResolutionError::SessionBindingMismatch)
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn directory_compatibility_is_unique_none_or_ambiguous_without_identity_derivation() {
    let pool = pool().await;
    let store = Arc::new(InMemoryWorkspaceStore::new());
    let root = tempdir().expect("workspace");
    let (workspace, project) = project_for(&pool, &store, root.path(), "Project").await;
    let resolver = resolver(&pool, store.clone());

    let unique = resolver
        .lookup_directory(root.path())
        .await
        .expect("unique lookup");
    assert!(matches!(
        unique,
        DirectoryCompatibilityOutcome::Unique(candidate)
            if candidate.project_id == project && candidate.workspace_id == workspace.id
    ));

    let resolved = resolver
        .resolve_directory(root.path())
        .await
        .expect("unique directory resolution");
    assert_eq!(resolved.project_id, project);

    let missing = resolver
        .lookup_directory(tempdir().expect("missing directory").path())
        .await
        .expect("none lookup");
    assert_eq!(missing, DirectoryCompatibilityOutcome::None);

    // The second stored locator is lexically different but canonicalizes to
    // the same directory. This models legacy aliases without constructing an
    // identity from either path.
    let alias = make_workspace(root.path().join("."), "Alias");
    add_workspace(&pool, &store, &alias).await;
    let catalog = ProjectCatalog::new(pool.clone());
    let alias_project = catalog
        .register_local_project(
            RegisterLocalProject {
                display_name: "Alias".to_string(),
                description: None,
                tags: Vec::new(),
                primary_repository_id: None,
            },
            &alias.id,
            "context-test",
        )
        .await
        .expect("alias project");

    let ambiguous = resolver
        .lookup_directory(root.path())
        .await
        .expect("ambiguous lookup");
    match ambiguous {
        DirectoryCompatibilityOutcome::Ambiguous(candidates) => {
            assert_eq!(candidates.len(), 2);
            assert!(candidates
                .iter()
                .any(|candidate| candidate.project_id == project));
            assert!(candidates
                .iter()
                .any(|candidate| candidate.project_id == alias_project.project_id));
        }
        other => panic!("expected ambiguous outcome, got {other:?}"),
    }

    assert!(matches!(
        resolver.resolve_directory(root.path()).await,
        Err(ContextResolutionError::DirectoryAmbiguous(_))
    ));
}

#[test]
fn raw_context_input_is_bounded_and_typed() {
    assert!(ProjectContextRequest::from_raw("/tmp/project", "workspace", None).is_err());
    assert!(
        ProjectContextRequest::from_raw("project", "workspace", Some(&"x".repeat(129)),).is_err()
    );
    assert!(ProjectContextRequest::from_raw("project", "workspace", None).is_ok());
    assert!(codegg_core::context::DirectoryLocator::parse(Path::new("")).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn canonical_session_create_and_list_are_atomic_and_identity_backed() {
    let pool = pool().await;
    let workspace_store = Arc::new(InMemoryWorkspaceStore::new());
    let root = tempdir().expect("workspace");
    let (workspace, project) = project_for(&pool, &workspace_store, root.path(), "Project").await;
    let sessions = SessionStore::new(pool.clone());

    let session = sessions
        .create_with_binding(
            CreateSession {
                project_id: "legacy-projection".to_string(),
                workspace_id: Some("legacy-workspace".to_string()),
                directory: root.path().display().to_string(),
                title: Some("Canonical session".to_string()),
                ..Default::default()
            },
            &project,
            &workspace.id,
            "context-test-create",
        )
        .await
        .expect("canonical session create");

    let binding: (String, String, String) = sqlx::query_as(
        "SELECT project_id, workspace_id, status FROM session_project_binding WHERE session_id = ?",
    )
    .bind(&session.id)
    .fetch_one(&pool)
    .await
    .expect("canonical binding row");
    assert_eq!(binding.0, project.as_str());
    assert_eq!(binding.1, workspace.id.as_str());
    assert_eq!(binding.2, "resolved");

    let listed = sessions
        .list_by_canonical_project(project.as_str(), Some(10))
        .await
        .expect("canonical session list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, session.id);
}
