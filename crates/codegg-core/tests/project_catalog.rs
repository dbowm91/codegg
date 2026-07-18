//! Integration tests for the project catalog service.

use chrono::Utc;
use codegg_core::identity::{NodeId, ProjectId};
use codegg_core::project_catalog::{
    CatalogError, HealthStatus, Locator, ProjectCatalog, RegisterLocalProject,
};
use codegg_core::project_storage::ProjectStorage;
use codegg_core::session::schema;
use codegg_core::workspace::{WorkspaceId, WorkspaceRecord};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;
use tempfile::tempdir;

async fn pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    schema::migrate(&pool).await.unwrap();
    pool
}

async fn concurrent_pool() -> (SqlitePool, tempfile::TempDir) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("catalog.db");
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
        .unwrap()
        .create_if_missing(true)
        .foreign_keys(true);
    let setup = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    schema::migrate(&setup).await.unwrap();
    setup.close().await;
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
        .unwrap()
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .unwrap();
    (pool, directory)
}

fn workspace(root: &std::path::Path, name: &str) -> WorkspaceRecord {
    let now = Utc::now();
    WorkspaceRecord {
        id: WorkspaceId::new(),
        canonical_root: root.to_path_buf(),
        display_name: name.to_string(),
        created_at: now,
        last_opened_at: now,
        archived_at: None,
    }
}

async fn register_workspace(storage: &ProjectStorage, ws: &WorkspaceRecord) {
    let evidence =
        codegg_core::repository_lineage::classify_remote_urls(["https://example.test/repo"]);
    storage
        .reconcile_workspace(ws, &evidence, "test")
        .await
        .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn migration_v28_is_additive_and_idempotent() {
    let p = pool().await;
    // Running migrate again should succeed (idempotent)
    schema::migrate(&p).await.unwrap();

    // Verify new tables exist
    let table_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('project_locator', 'project_health', 'legacy_catalog_association_marker')",
    )
    .fetch_one(&p)
    .await
    .unwrap();
    assert_eq!(table_count, 3, "all three new tables should exist");

    // Verify new columns exist on logical_project
    let col_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('logical_project') WHERE name IN ('archived_at', 'description', 'tags', 'registration_source', 'time_last_opened')",
    )
    .fetch_one(&p)
    .await
    .unwrap();
    assert_eq!(col_count, 5, "all five new columns should exist");
}

#[tokio::test(flavor = "current_thread")]
async fn register_local_project_with_registered_workspace() {
    let p = pool().await;
    let catalog = ProjectCatalog::new(p.clone());

    let root = tempdir().unwrap();
    let ws = workspace(root.path(), "My Project");
    // Insert the workspace directly without binding it to a project so
    // the catalog can register a project for it.
    sqlx::query("INSERT INTO workspace (id, canonical_root, display_name, time_created, time_last_opened, time_archived) VALUES (?, ?, ?, ?, ?, NULL)")
        .bind(ws.id.as_str())
        .bind(ws.canonical_root.to_string_lossy().to_string())
        .bind(&ws.display_name)
        .bind(ws.created_at.timestamp_millis())
        .bind(ws.last_opened_at.timestamp_millis())
        .execute(&p).await.unwrap();

    let input = RegisterLocalProject {
        display_name: "My Project".to_string(),
        description: Some("A test project".to_string()),
        tags: vec!["rust".to_string()],
        primary_repository_id: None,
    };

    let record = catalog
        .register_local_project(input, &ws.id, "test")
        .await
        .unwrap();

    assert_eq!(record.display_name, "My Project");
    assert_eq!(record.description, Some("A test project".to_string()));
    assert_eq!(record.tags, vec!["rust".to_string()]);
    assert_eq!(record.registration_source, "test");
    assert!(record.archived_at.is_none());

    // Verify workspace binding exists
    let bindings = catalog
        .list_workspaces_for_project(&record.project_id)
        .await
        .unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].workspace_id, ws.id);
}

#[tokio::test(flavor = "current_thread")]
async fn register_local_project_with_unregistered_root_returns_invalid_value() {
    let p = pool().await;
    let catalog = ProjectCatalog::new(p.clone());

    let ws_id = WorkspaceId::new();
    let input = RegisterLocalProject {
        display_name: "Test".to_string(),
        description: None,
        tags: vec![],
        primary_repository_id: None,
    };

    let result = catalog.register_local_project(input, &ws_id, "test").await;
    assert!(result.is_err());
    assert!(matches!(result, Err(CatalogError::InvalidValue(_))));
}

#[tokio::test(flavor = "current_thread")]
async fn two_workspaces_one_project_reuses() {
    let p = pool().await;
    let storage = ProjectStorage::new(p.clone());
    let catalog = ProjectCatalog::new(p.clone());

    let root1 = tempdir().unwrap();
    let root2 = tempdir().unwrap();
    let ws1 = workspace(root1.path(), "First");
    let ws2 = workspace(root2.path(), "Second");

    // Both use the same repo → same lineage → same project
    let evidence =
        codegg_core::repository_lineage::classify_remote_urls(["https://example.test/repo"]);
    storage
        .reconcile_workspace(&ws1, &evidence, "test")
        .await
        .unwrap();
    storage
        .reconcile_workspace(&ws2, &evidence, "test")
        .await
        .unwrap();

    // Register both via catalog
    let input1 = RegisterLocalProject {
        display_name: "First".to_string(),
        description: None,
        tags: vec![],
        primary_repository_id: None,
    };
    let input2 = RegisterLocalProject {
        display_name: "Second".to_string(),
        description: None,
        tags: vec![],
        primary_repository_id: None,
    };

    let r1 = catalog
        .register_local_project(input1, &ws1.id, "test")
        .await
        .unwrap();
    let r2 = catalog
        .register_local_project(input2, &ws2.id, "test")
        .await
        .unwrap();

    // Same project_id because same repository lineage
    assert_eq!(r1.project_id, r2.project_id);
}

#[tokio::test(flavor = "current_thread")]
async fn archive_preserves_related_rows() {
    let p = pool().await;
    let storage = ProjectStorage::new(p.clone());
    let catalog = ProjectCatalog::new(p.clone());

    let root = tempdir().unwrap();
    let ws = workspace(root.path(), "Archivable");
    register_workspace(&storage, &ws).await;

    let input = RegisterLocalProject {
        display_name: "Archivable".to_string(),
        description: None,
        tags: vec![],
        primary_repository_id: None,
    };
    let record = catalog
        .register_local_project(input, &ws.id, "test")
        .await
        .unwrap();

    // Set health
    catalog
        .set_health(&record.project_id, HealthStatus::Available, "test")
        .await
        .unwrap();

    // Attach a locator
    let locator = Locator::Ssh {
        host: "example.com".to_string(),
        port: Some(22),
        user: None,
        path: "/repo".to_string(),
        label: None,
    };
    catalog
        .attach_locator(&record.project_id, locator, "test")
        .await
        .unwrap();

    // Archive
    let archived = catalog
        .archive_project(&record.project_id, "test")
        .await
        .unwrap();
    assert!(archived.archived_at.is_some());

    // Workspace binding still exists
    let bindings = catalog
        .list_workspaces_for_project(&record.project_id)
        .await
        .unwrap();
    assert_eq!(bindings.len(), 1);

    // Health record still exists
    let health = catalog.get_health(&record.project_id).await.unwrap();
    assert!(health.is_some());

    // Locator still exists
    let locators = catalog.list_locators(&record.project_id).await.unwrap();
    assert_eq!(locators.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn list_excludes_archived_by_default() {
    let p = pool().await;
    let storage = ProjectStorage::new(p.clone());
    let catalog = ProjectCatalog::new(p.clone());

    let root = tempdir().unwrap();
    let ws = workspace(root.path(), "ListTest");
    register_workspace(&storage, &ws).await;

    let input = RegisterLocalProject {
        display_name: "ListTest".to_string(),
        description: None,
        tags: vec![],
        primary_repository_id: None,
    };
    let record = catalog
        .register_local_project(input, &ws.id, "test")
        .await
        .unwrap();

    // Active shows up
    let all_active = catalog.list_projects(false).await.unwrap();
    assert_eq!(all_active.len(), 1);

    // Archive it
    catalog
        .archive_project(&record.project_id, "test")
        .await
        .unwrap();

    // No longer shows with include_archived=false
    let active = catalog.list_projects(false).await.unwrap();
    assert!(active.is_empty());

    // Shows with include_archived=true
    let all = catalog.list_projects(true).await.unwrap();
    assert_eq!(all.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn restore_clears_archived_at() {
    let p = pool().await;
    let storage = ProjectStorage::new(p.clone());
    let catalog = ProjectCatalog::new(p.clone());

    let root = tempdir().unwrap();
    let ws = workspace(root.path(), "RestoreTest");
    register_workspace(&storage, &ws).await;

    let input = RegisterLocalProject {
        display_name: "RestoreTest".to_string(),
        description: None,
        tags: vec![],
        primary_repository_id: None,
    };
    let record = catalog
        .register_local_project(input, &ws.id, "test")
        .await
        .unwrap();

    // Archive then restore
    catalog
        .archive_project(&record.project_id, "test")
        .await
        .unwrap();
    let restored = catalog
        .restore_project(&record.project_id, "test")
        .await
        .unwrap();
    assert!(restored.archived_at.is_none());
    assert_eq!(
        restored.lifecycle,
        codegg_core::project_storage::ProjectLifecycle::Active
    );
}

#[tokio::test(flavor = "current_thread")]
async fn attach_ssh_locator_stores_placeholder() {
    let p = pool().await;
    let storage = ProjectStorage::new(p.clone());
    let catalog = ProjectCatalog::new(p.clone());

    let root = tempdir().unwrap();
    let ws = workspace(root.path(), "SSHTest");
    register_workspace(&storage, &ws).await;

    let input = RegisterLocalProject {
        display_name: "SSHTest".to_string(),
        description: None,
        tags: vec![],
        primary_repository_id: None,
    };
    let record = catalog
        .register_local_project(input, &ws.id, "test")
        .await
        .unwrap();

    let locator = Locator::Ssh {
        host: "git.example.com".to_string(),
        port: Some(2222),
        user: Some("dev".to_string()),
        path: "/repos/my-project".to_string(),
        label: Some("production".to_string()),
    };
    let stored = catalog
        .attach_locator(&record.project_id, locator, "test")
        .await
        .unwrap();

    // Verify the stored record
    assert!(!stored.locator.is_local());
    assert!(stored.display_summary.starts_with("ssh:"));
    // No path leakage in summary
    assert!(!stored.display_summary.contains("/repos/my-project"));

    // Verify in list
    let locators = catalog.list_locators(&record.project_id).await.unwrap();
    assert_eq!(locators.len(), 1);
    assert!(!locators[0].locator.is_local());
}

#[tokio::test(flavor = "current_thread")]
async fn restart_hydration_returns_expected_counts() {
    let p = pool().await;
    let storage = ProjectStorage::new(p.clone());
    let catalog = ProjectCatalog::new(p.clone());

    let root = tempdir().unwrap();
    let ws = workspace(root.path(), "HydrationTest");
    register_workspace(&storage, &ws).await;

    let input = RegisterLocalProject {
        display_name: "HydrationTest".to_string(),
        description: None,
        tags: vec![],
        primary_repository_id: None,
    };
    catalog
        .register_local_project(input, &ws.id, "test")
        .await
        .unwrap();

    let report = catalog.restart_hydration().await.unwrap();
    assert_eq!(report.active_project_count, 1);
    assert_eq!(report.total_project_count, 1);
    assert_eq!(report.health_count, 0);
    assert_eq!(report.locator_count, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn mark_opened_updates_timestamp() {
    let p = pool().await;
    let storage = ProjectStorage::new(p.clone());
    let catalog = ProjectCatalog::new(p.clone());

    let root = tempdir().unwrap();
    let ws = workspace(root.path(), "MarkTest");
    register_workspace(&storage, &ws).await;

    let input = RegisterLocalProject {
        display_name: "MarkTest".to_string(),
        description: None,
        tags: vec![],
        primary_repository_id: None,
    };
    let record = catalog
        .register_local_project(input, &ws.id, "test")
        .await
        .unwrap();

    assert!(record.time_last_opened_at.is_none());

    catalog.mark_opened(&record.project_id).await.unwrap();

    let refreshed = catalog.get_project(&record.project_id).await.unwrap();
    assert!(refreshed.time_last_opened_at.is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn count_by_lifecycle() {
    let p = pool().await;
    let storage = ProjectStorage::new(p.clone());
    let catalog = ProjectCatalog::new(p.clone());

    let counts = catalog.count_by_lifecycle().await.unwrap();
    assert_eq!(counts.total, 0);

    let root = tempdir().unwrap();
    let ws = workspace(root.path(), "CountTest");
    register_workspace(&storage, &ws).await;

    let input = RegisterLocalProject {
        display_name: "CountTest".to_string(),
        description: None,
        tags: vec![],
        primary_repository_id: None,
    };
    let record = catalog
        .register_local_project(input, &ws.id, "test")
        .await
        .unwrap();

    let counts = catalog.count_by_lifecycle().await.unwrap();
    assert_eq!(counts.active, 1);
    assert_eq!(counts.archived, 0);
    assert_eq!(counts.total, 1);

    catalog
        .archive_project(&record.project_id, "test")
        .await
        .unwrap();

    let counts = catalog.count_by_lifecycle().await.unwrap();
    assert_eq!(counts.active, 0);
    assert_eq!(counts.archived, 1);
    assert_eq!(counts.total, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn detach_locator() {
    let p = pool().await;
    let storage = ProjectStorage::new(p.clone());
    let catalog = ProjectCatalog::new(p.clone());

    let root = tempdir().unwrap();
    let ws = workspace(root.path(), "DetachTest");
    register_workspace(&storage, &ws).await;

    let input = RegisterLocalProject {
        display_name: "DetachTest".to_string(),
        description: None,
        tags: vec![],
        primary_repository_id: None,
    };
    let record = catalog
        .register_local_project(input, &ws.id, "test")
        .await
        .unwrap();

    let locator = Locator::LinkedNode {
        node_id: NodeId::new(),
        alias: None,
        path_hint: None,
    };
    let stored = catalog
        .attach_locator(&record.project_id, locator, "test")
        .await
        .unwrap();

    catalog.detach_locator(&stored.id).await.unwrap();

    let locators = catalog.list_locators(&record.project_id).await.unwrap();
    assert!(locators.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn set_health_placeholder() {
    let p = pool().await;
    let storage = ProjectStorage::new(p.clone());
    let catalog = ProjectCatalog::new(p.clone());

    let root = tempdir().unwrap();
    let ws = workspace(root.path(), "HealthTest");
    register_workspace(&storage, &ws).await;

    let input = RegisterLocalProject {
        display_name: "HealthTest".to_string(),
        description: None,
        tags: vec![],
        primary_repository_id: None,
    };
    let record = catalog
        .register_local_project(input, &ws.id, "test")
        .await
        .unwrap();

    let health = catalog
        .set_health(&record.project_id, HealthStatus::Unavailable, "test")
        .await
        .unwrap();
    assert_eq!(health.status, HealthStatus::Unavailable);

    let fetched = catalog
        .get_health(&record.project_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, HealthStatus::Unavailable);
}

#[tokio::test(flavor = "current_thread")]
async fn get_project_not_found() {
    let p = pool().await;
    let catalog = ProjectCatalog::new(p.clone());

    let result = catalog.get_project(&ProjectId::new()).await;
    assert!(matches!(result, Err(CatalogError::NotFound(_))));
}

#[tokio::test(flavor = "current_thread")]
async fn archive_nonexistent_project_returns_not_found() {
    let p = pool().await;
    let catalog = ProjectCatalog::new(p.clone());

    let result = catalog.archive_project(&ProjectId::new(), "test").await;
    assert!(matches!(result, Err(CatalogError::NotFound(_))));
}

#[tokio::test(flavor = "current_thread")]
async fn restore_nonexistent_project_returns_not_found() {
    let p = pool().await;
    let catalog = ProjectCatalog::new(p.clone());

    let result = catalog.restore_project(&ProjectId::new(), "test").await;
    assert!(matches!(result, Err(CatalogError::NotFound(_))));
}

#[tokio::test(flavor = "current_thread")]
async fn conservative_legacy_association_idempotent() {
    use codegg_core::project_catalog::conservative_legacy_association;

    let p = pool().await;

    let root = tempdir().unwrap();
    let ws = workspace(root.path(), "LegacyTest");

    // Register workspace first (without binding to project)
    sqlx::query(
        "INSERT INTO workspace (id, canonical_root, display_name, time_created, time_last_opened) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(ws.id.as_str())
    .bind(ws.canonical_root.to_string_lossy().as_ref())
    .bind(&ws.display_name)
    .bind(ws.created_at.timestamp_millis())
    .bind(ws.last_opened_at.timestamp_millis())
    .execute(&p)
    .await
    .unwrap();

    // Run association
    let report = conservative_legacy_association(&p, std::slice::from_ref(&ws), "test_legacy")
        .await
        .unwrap();
    assert!(!report.already_migrated);
    assert!(report.projects_associated <= report.diagnostics_recorded + 1);

    // Run again — should be idempotent
    let report2 = conservative_legacy_association(&p, std::slice::from_ref(&ws), "test_legacy")
        .await
        .unwrap();
    assert!(report2.already_migrated);
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_duplicate_registration_converges() {
    let (p, _dir) = concurrent_pool().await;
    let storage = ProjectStorage::new(p.clone());
    let _catalog = ProjectCatalog::new(p.clone());

    let root = tempdir().unwrap();
    let ws = workspace(root.path(), "Concurrent");
    register_workspace(&storage, &ws).await;

    let mut handles = vec![];
    for _ in 0..4 {
        let pool = p.clone();
        let ws_id = ws.id.clone();
        handles.push(tokio::spawn(async move {
            let catalog = ProjectCatalog::new(pool);
            let input = RegisterLocalProject {
                display_name: "Concurrent".to_string(),
                description: None,
                tags: vec![],
                primary_repository_id: None,
            };
            catalog
                .register_local_project(input, &ws_id, "test")
                .await
                .unwrap()
        }));
    }

    let mut project_ids = vec![];
    for handle in handles {
        let record = handle.await.unwrap();
        project_ids.push(record.project_id);
    }

    // All should converge to the same project
    for id in &project_ids[1..] {
        assert_eq!(*id, project_ids[0]);
    }
}
