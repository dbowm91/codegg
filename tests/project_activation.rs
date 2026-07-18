//! Project Catalog Milestone 3 activation, health, isolation, and hydration.

use std::str::FromStr;

use codegg::core::daemon::CoreDaemon;
use codegg::session::schema::migrate;
use codegg_core::project_catalog::{ProjectCatalog, RegisterLocalProject};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tempfile::TempDir;

async fn pool() -> sqlx::SqlitePool {
    let url = format!(
        "file:project_activation_{}?mode=memory&cache=shared",
        uuid::Uuid::new_v4().simple()
    );
    let options = SqliteConnectOptions::from_str(&url)
        .expect("valid sqlite options")
        .create_if_missing(true)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("connect sqlite");
    migrate(&pool).await.expect("migrate schema");
    pool
}

async fn seed(daemon: &CoreDaemon, root: &TempDir, name: &str) -> (String, String) {
    let workspace = daemon
        .workspaces
        .get_or_register(root.path())
        .await
        .expect("register workspace");
    let project = ProjectCatalog::new(daemon.pool.clone().expect("daemon pool"))
        .register_local_project(
            RegisterLocalProject {
                display_name: name.to_string(),
                description: None,
                tags: Vec::new(),
                primary_repository_id: None,
            },
            &workspace.id,
            "project-activation-test",
        )
        .await
        .expect("register project");
    (project.project_id.to_string(), workspace.id.to_string())
}

#[tokio::test(flavor = "current_thread")]
async fn catalog_listing_is_probe_free_and_activation_is_lazy() {
    let daemon = CoreDaemon::new(Some(pool().await), None, None, None);
    let root_a = tempfile::tempdir().unwrap();
    let root_b = tempfile::tempdir().unwrap();
    let _ = seed(&daemon, &root_a, "A").await;
    let _ = seed(&daemon, &root_b, "B").await;

    let projects = ProjectCatalog::new(daemon.pool.clone().unwrap())
        .list_projects(false)
        .await
        .unwrap();
    assert_eq!(projects.len(), 2);
    assert_eq!(daemon.workspace_services.active_count(), 0);
    assert_eq!(daemon.project_activation.active_count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn activation_refreshes_assets_and_is_idempotent_per_owner() {
    let daemon = CoreDaemon::new(Some(pool().await), None, None, None);
    let root = tempfile::tempdir().unwrap();
    let (project_id, workspace_id) = seed(&daemon, &root, "single").await;

    let first = daemon
        .activate_project_workspace(&project_id, &workspace_id, "test-owner")
        .await
        .unwrap();
    let second = daemon
        .activate_project_workspace(&project_id, &workspace_id, "test-owner")
        .await
        .unwrap();

    assert_eq!(first.lease.lease_id(), second.lease.lease_id());
    assert_eq!(daemon.project_activation.active_count(), 1);
    assert_eq!(daemon.workspace_services.active_count(), 1);
    assert!(first.lease.is_active());
    assert!(first.refresh.generation.is_some());
    assert_eq!(first.binding_revision, 1);
    assert_eq!(first.diagnostics.len(), 1);
    assert_eq!(first.health.project_id, project_id);
    assert_eq!(first.health.workspace_id, workspace_id);
    assert_eq!(
        first.health.services.state,
        codegg::core::project_activation::HealthState::Available
    );

    drop(first);
    assert_eq!(daemon.project_activation.active_count(), 1);
    drop(second);
    assert_eq!(daemon.project_activation.active_count(), 0);
    assert!(daemon
        .workspace_services
        .peek(&codegg::workspace::WorkspaceId::new_unchecked(
            workspace_id.clone()
        ))
        .is_some());
    let eviction = daemon
        .workspace_services
        .evict_idle(chrono::Utc::now() + chrono::Duration::hours(1));
    assert!(eviction
        .evicted
        .iter()
        .any(|id| id.as_str() == workspace_id));
    assert!(daemon
        .workspace_services
        .peek(&codegg::workspace::WorkspaceId::new_unchecked(workspace_id))
        .is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_same_owner_activation_coalesces_scope_and_bundle() {
    let daemon = std::sync::Arc::new(CoreDaemon::new(Some(pool().await), None, None, None));
    let root = tempfile::tempdir().unwrap();
    let (project_id, workspace_id) = seed(&daemon, &root, "contention").await;
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let daemon = daemon.clone();
        let project_id = project_id.clone();
        let workspace_id = workspace_id.clone();
        tasks.push(tokio::spawn(async move {
            let activation = daemon
                .activate_project_workspace(&project_id, &workspace_id, "same-owner")
                .await
                .unwrap();
            (
                activation.lease.lease_id().to_string(),
                activation.refresh.coalesced,
            )
        }));
    }
    let mut results = Vec::new();
    for task in tasks {
        results.push(task.await.unwrap());
    }
    assert!(results
        .iter()
        .all(|(lease_id, _)| lease_id == &results[0].0));
    assert_eq!(daemon.project_activation.active_count(), 0);
    assert_eq!(daemon.workspace_services.active_count(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn activation_rejects_refresh_without_usable_generation_and_releases_lease() {
    let daemon = CoreDaemon::new(Some(pool().await), None, None, None);
    let root = tempfile::tempdir().unwrap();
    let (project_id, workspace_id) = seed(&daemon, &root, "invalid-refresh").await;
    let missing_root = root.path().join("missing-after-selection");
    sqlx::query("UPDATE workspace SET canonical_root = ? WHERE id = ?")
        .bind(missing_root.to_string_lossy().as_ref())
        .bind(&workspace_id)
        .execute(&daemon.pool.clone().unwrap())
        .await
        .unwrap();

    let error = daemon
        .activate_project_workspace(&project_id, &workspace_id, "invalid-owner")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("runtime asset refresh"));
    assert_eq!(daemon.project_activation.active_count(), 0);
    assert_eq!(
        daemon
            .workspace_services
            .peek(&codegg::workspace::WorkspaceId::new_unchecked(workspace_id))
            .unwrap()
            .active_leases,
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn two_project_activation_scopes_are_isolated() {
    let daemon = CoreDaemon::new(Some(pool().await), None, None, None);
    let root_a = tempfile::tempdir().unwrap();
    let root_b = tempfile::tempdir().unwrap();
    let (project_a, workspace_a) = seed(&daemon, &root_a, "A").await;
    let (project_b, workspace_b) = seed(&daemon, &root_b, "B").await;

    let activation_a = daemon
        .activate_project_workspace(&project_a, &workspace_a, "owner-a")
        .await
        .unwrap();
    let inactive_b = daemon
        .project_health(&project_b, &workspace_b)
        .await
        .unwrap();
    assert_eq!(
        inactive_b.services.state,
        codegg::core::project_activation::HealthState::Unavailable
    );

    let activation_b = daemon
        .activate_project_workspace(&project_b, &workspace_b, "owner-b")
        .await
        .unwrap();
    assert_eq!(daemon.workspace_services.active_count(), 2);
    assert_ne!(
        activation_a
            .lease
            .service_snapshot()
            .unwrap()
            .canonical_root,
        activation_b
            .lease
            .service_snapshot()
            .unwrap()
            .canonical_root
    );
}

#[tokio::test(flavor = "current_thread")]
async fn restart_hydrates_catalog_and_asset_metadata_without_activation() {
    let shared_pool = pool().await;
    let daemon = CoreDaemon::new(Some(shared_pool.clone()), None, None, None);
    let root = tempfile::tempdir().unwrap();
    let (project_id, workspace_id) = seed(&daemon, &root, "restart").await;
    let activation = daemon
        .activate_project_workspace(&project_id, &workspace_id, "restart-test")
        .await
        .unwrap();
    let generation = activation.refresh.generation.unwrap();
    drop(activation);

    let restarted = CoreDaemon::new(Some(shared_pool), None, None, None);
    restarted.hydrate_workspace_registry().await.unwrap();
    assert_eq!(restarted.project_activation.active_count(), 0);
    assert_eq!(restarted.workspace_services.active_count(), 0);
    assert!(restarted
        .workspaces
        .resolve(&codegg::workspace::WorkspaceId::new_unchecked(
            workspace_id.clone()
        ))
        .await
        .is_some());
    let status = restarted
        .asset_refresh
        .status(&codegg::agent::asset_refresh::AssetScope::new(
            project_id,
            workspace_id,
        ))
        .await;
    assert_eq!(status.generation, Some(generation));
}
