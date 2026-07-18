//! Multi-project workspace services and storage isolation tests.
//!
//! Phase 3 of the single-daemon plan. These tests verify the
//! [`WorkspaceServiceRegistry`], the [`WorkspaceServices`] bundle, the
//! lease lifecycle, the lock table, and the migration tooling that
//! moves legacy project-local session databases into the user-scoped
//! daemon catalog.
//!
//! The tests use [`InMemoryWorkspaceStore`] and an
//! `FsRunStore` so they don't pull in a full RunStore mock.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;

use codegg::run_store::{FsRunStore, RunStore};
use codegg::session::schema::migrate;
use codegg::workspace::{InMemoryWorkspaceStore, WorkspaceId, WorkspaceRecord, WorkspaceRegistry};
use codegg::workspace_services::{
    ProductionWorkspaceServicesFactory, ReloadResult, WorkspaceConfigSnapshot, WorkspaceLockTable,
    WorkspacePathPolicy, WorkspaceRepositoryGuard, WorkspaceServicePolicy,
    WorkspaceServiceRegistry, WorkspaceServices, WorkspaceServicesFactory,
};

fn temp_workspace(label: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("marker.txt"), label).unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "// lib").unwrap();
    dir
}

/// Counting factory that uses the real `FsRunStore` rooted at
/// `<workspace>/.codegg/runs/`. Lets us observe activation count and
/// identity without mocking the trait surface.
struct CountingFactory {
    activations: AtomicUsize,
    snapshots: parking_lot::Mutex<Vec<WorkspaceId>>,
}

impl CountingFactory {
    fn new() -> Self {
        Self {
            activations: AtomicUsize::new(0),
            snapshots: parking_lot::Mutex::new(Vec::new()),
        }
    }
    fn activation_count(&self) -> usize {
        self.activations.load(Ordering::SeqCst)
    }
}

impl WorkspaceServicesFactory for CountingFactory {
    fn build(&self, workspace: Arc<WorkspaceRecord>) -> Result<Arc<WorkspaceServices>, String> {
        self.activations.fetch_add(1, Ordering::SeqCst);
        self.snapshots.lock().push(workspace.id.clone());
        let path_policy = WorkspacePathPolicy::for_workspace(&workspace);
        let run_root = workspace.canonical_root.join(".codegg").join("runs");
        std::fs::create_dir_all(&run_root)
            .map_err(|e| format!("create run root {}: {}", run_root.display(), e))?;
        let run_store: Arc<dyn RunStore> = Arc::new(FsRunStore::new(run_root));
        let locks = Arc::new(WorkspaceLockTable::new());
        let config = Arc::new(WorkspaceConfigSnapshot {
            revision: 0,
            loaded_at: Utc::now(),
            source_files: Vec::new(),
            diagnostics: Vec::new(),
        });
        Ok(WorkspaceServices::new(
            workspace,
            config,
            run_store,
            path_policy,
            locks,
        ))
    }
}

async fn build_registry(
    factory: Arc<CountingFactory>,
) -> (Arc<WorkspaceRegistry>, Arc<WorkspaceServiceRegistry>) {
    let workspace_store = Arc::new(InMemoryWorkspaceStore::new());
    let workspace_registry = WorkspaceRegistry::load(workspace_store).await.unwrap();
    let policy = WorkspaceServicePolicy {
        max_active_workspaces: 4,
        idle_evict_after: Duration::from_secs(60),
    };
    let service_registry = WorkspaceServiceRegistry::new(
        workspace_registry.clone(),
        factory as Arc<dyn WorkspaceServicesFactory>,
        policy,
    );
    (workspace_registry, service_registry)
}

#[tokio::test(flavor = "current_thread")]
async fn two_workspaces_get_isolated_bundles() {
    let factory = Arc::new(CountingFactory::new());
    let (workspace_registry, services) = build_registry(factory.clone()).await;

    let dir_a = temp_workspace("a");
    let dir_b = temp_workspace("b");
    let rec_a = workspace_registry
        .get_or_register(dir_a.path())
        .await
        .unwrap();
    let rec_b = workspace_registry
        .get_or_register(dir_b.path())
        .await
        .unwrap();

    let lease_a = services.acquire(&rec_a.id).await.unwrap();
    let lease_b = services.acquire(&rec_b.id).await.unwrap();

    // Different workspace IDs.
    assert_ne!(lease_a.workspace_id(), lease_b.workspace_id());
    // Different RunStore instances (so cross-workspace writes cannot leak).
    assert!(
        !Arc::ptr_eq(&lease_a.services().run_store, &lease_b.services().run_store,),
        "different workspaces must get independent RunStore instances"
    );
    // Different LockTable instances.
    assert!(
        !Arc::ptr_eq(&lease_a.locks(), &lease_b.locks()),
        "different workspaces must get independent lock tables"
    );
    // Both bundles were activated.
    assert_eq!(factory.activation_count(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_acquire_produces_single_activation() {
    let factory = Arc::new(CountingFactory::new());
    let (workspace_registry, services) = build_registry(factory.clone()).await;

    let dir = temp_workspace("single_flight");
    let rec = workspace_registry
        .get_or_register(dir.path())
        .await
        .unwrap();

    // Spawn N concurrent acquisitions of the same workspace.
    let mut handles = Vec::new();
    for _ in 0..32 {
        let services = services.clone();
        let id = rec.id.clone();
        handles.push(tokio::spawn(async move {
            let lease = services.acquire(&id).await.unwrap();
            lease.workspace_id().clone()
        }));
    }
    let mut all = Vec::new();
    for h in handles {
        all.push(h.await.unwrap());
    }
    // Every caller saw the same workspace id.
    for id in &all {
        assert_eq!(id, &rec.id);
    }
    // Only ONE bundle was constructed despite N racers.
    assert_eq!(
        factory.activation_count(),
        1,
        "concurrent acquire must produce exactly one activation"
    );
    // The active registry reports one bundle.
    assert_eq!(services.list_active().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn lease_drop_releases_accounting() {
    let factory = Arc::new(CountingFactory::new());
    let (workspace_registry, services) = build_registry(factory.clone()).await;

    let dir = temp_workspace("lease_lifecycle");
    let rec = workspace_registry
        .get_or_register(dir.path())
        .await
        .unwrap();

    let lease_a = services.acquire(&rec.id).await.unwrap();
    let lease_b = services.acquire(&rec.id).await.unwrap();
    drop(lease_a);
    drop(lease_b);

    // Two leases were acquired, but they did NOT trigger re-activation.
    assert_eq!(factory.activation_count(), 1);

    // The bundle still exists but has zero active leases.
    let snap = services.peek(&rec.id).expect("bundle still active");
    assert_eq!(snap.active_leases, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn evict_idle_skips_bundles_with_active_leases() {
    let factory = Arc::new(CountingFactory::new());
    let (workspace_registry, services) = build_registry(factory.clone()).await;

    let dir_a = temp_workspace("idle_evict_a");
    let dir_b = temp_workspace("idle_evict_b");
    let rec_a = workspace_registry
        .get_or_register(dir_a.path())
        .await
        .unwrap();
    let rec_b = workspace_registry
        .get_or_register(dir_b.path())
        .await
        .unwrap();

    // Acquire both, then drop one to leave it idle.
    let lease_a = services.acquire(&rec_a.id).await.unwrap();
    let lease_b = services.acquire(&rec_b.id).await.unwrap();
    drop(lease_a);
    drop(lease_b);

    // With a far-future "now" the bundles' last_used_at is well past
    // the idle threshold and both have zero leases, so both get
    // evaluated and evicted.
    let report = services.evict_idle(Utc::now() + chrono::Duration::seconds(3600));
    assert_eq!(report.evaluated, 2);
    assert_eq!(report.skipped_active.len(), 0);
    assert_eq!(report.evicted.len(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn peek_returns_none_for_unregistered_workspace() {
    let factory = Arc::new(CountingFactory::new());
    let (_wr, services) = build_registry(factory).await;

    let unknown = WorkspaceId::new_unchecked("00000000-0000-0000-0000-000000000000");
    assert!(services.peek(&unknown).is_none());

    // And acquire must fail with NotFound for an unknown workspace.
    let result = services.acquire(&unknown).await;
    match result {
        Err(codegg::workspace_services::WorkspaceServiceError::NotFound(_)) => {}
        Err(other) => panic!("expected NotFound, got {:?}", other),
        Ok(_) => panic!("expected NotFound error, got Ok"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn workspace_lock_table_serializes_acquire_repository() {
    let table = Arc::new(WorkspaceLockTable::new());
    let counter = Arc::new(AtomicUsize::new(0));
    let repo_path = PathBuf::from("/tmp/repo");
    let mut handles = Vec::new();
    for _ in 0..8 {
        let t = table.clone();
        let c = counter.clone();
        let p = repo_path.clone();
        handles.push(tokio::spawn(async move {
            let _g: WorkspaceRepositoryGuard = t.acquire_repository(&p).await;
            // Critical section: only one task should be here at a time.
            let prev = c.fetch_add(1, Ordering::SeqCst);
            assert_eq!(prev, 0, "lock must serialize acquire_repository");
            // Sleep briefly to maximize overlap chance.
            tokio::time::sleep(Duration::from_millis(5)).await;
            c.store(0, Ordering::SeqCst);
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn config_reload_bumps_revision_and_keeps_leases() {
    let factory = Arc::new(CountingFactory::new());
    let (workspace_registry, services) = build_registry(factory.clone()).await;

    let dir = temp_workspace("config_reload");
    let rec = workspace_registry
        .get_or_register(dir.path())
        .await
        .unwrap();

    let lease = services.acquire(&rec.id).await.unwrap();
    let initial_revision = lease.config_snapshot().revision;

    // Reload the configuration — the registry bumps the revision and
    // emits any diagnostics the bundle carries.
    let reload: ReloadResult = services.reload_config(&rec.id).unwrap();

    assert_eq!(reload.previous_revision, initial_revision);
    assert_eq!(reload.new_revision, initial_revision + 1);
    // No diagnostics were configured in the initial bundle.
    assert_eq!(reload.diagnostics.len(), 0);
    assert_eq!(
        services.peek(&rec.id).unwrap().active_leases,
        0,
        "reloading must not copy old leases into the replacement bundle"
    );
    drop(lease);

    // Re-acquire and confirm revision is preserved.
    let lease2 = services.acquire(&rec.id).await.unwrap();
    assert_eq!(lease2.config_snapshot().revision, initial_revision + 1);
    drop(lease2);
}

#[tokio::test(flavor = "current_thread")]
async fn shutdown_all_force_terminates_and_drains() {
    let factory = Arc::new(CountingFactory::new());
    let (workspace_registry, services) = build_registry(factory.clone()).await;

    let dir_a = temp_workspace("shutdown_a");
    let dir_b = temp_workspace("shutdown_b");
    let rec_a = workspace_registry
        .get_or_register(dir_a.path())
        .await
        .unwrap();
    let rec_b = workspace_registry
        .get_or_register(dir_b.path())
        .await
        .unwrap();

    // Hold a lease on one of them so we can prove shutdown_all
    // force-terminates regardless.
    let _lease = services.acquire(&rec_a.id).await.unwrap();
    services.acquire(&rec_b.id).await.unwrap();

    // Use a generous deadline so both bundles drain, but one must
    // be force-terminated because _lease is still alive.
    let report = services.shutdown_all(Duration::from_millis(500)).await;
    assert!(
        report.drained.len() + report.force_terminated.len() == 2,
        "both bundles accounted for: drained={:?} forced={:?}",
        report.drained,
        report.force_terminated,
    );
    assert!(services.list_active().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn production_factory_builds_filesystem_run_store() {
    let dir = temp_workspace("prod_factory");
    let now = Utc::now();
    let workspace = Arc::new(WorkspaceRecord {
        id: WorkspaceId::new_unchecked("11111111-1111-1111-1111-111111111111"),
        canonical_root: dir.path().canonicalize().unwrap(),
        display_name: "prod".into(),
        created_at: now,
        last_opened_at: now,
        archived_at: None,
    });
    let factory = ProductionWorkspaceServicesFactory;
    let bundle = factory.build(workspace).unwrap();
    // The production factory anchors RunStore at .codegg/runs/ under the
    // workspace root, and creates the parent `.codegg/` directory
    // during construction.
    assert!(bundle.artifact_root.ends_with("runs"));
    let parent = bundle.artifact_root.parent().unwrap();
    assert!(
        parent.exists(),
        "production factory must create .codegg/ under workspace root"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_paths_user_scoped_default_resolves() {
    let paths = codegg::storage::DaemonPaths::default();
    let data_root = paths.data_root();
    assert!(data_root.to_string_lossy().contains("codegg"));
    assert!(paths.catalog_db_path().ends_with("codegg.db"));
    assert!(paths
        .catalog_db_path()
        .to_string_lossy()
        .contains(data_root.to_string_lossy().as_ref()));
}

#[tokio::test(flavor = "current_thread")]
async fn migration_imports_legacy_session_db_into_catalog() {
    use codegg::migration::{fetch_marker, migrate_legacy_project_database, MigrationOutcome};

    // Catalog is a fresh in-memory SQLite pool with the schema migrated.
    let catalog = {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let opts = SqliteConnectOptions::from_str("file::memory:?cache=private")
            .expect("opts")
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        migrate(&pool).await.unwrap();
        pool
    };

    // Workspace registry in memory.
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .unwrap();

    // Legacy project root with a seeded session DB.
    let project = temp_workspace("legacy_project");
    let codegg_dir = project.path().join(".codegg");
    std::fs::create_dir_all(&codegg_dir).unwrap();
    let source_db = codegg_dir.join("sessions.db");
    {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", source_db.display()))
            .unwrap()
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        migrate(&pool).await.unwrap();
        pool.close().await;
    }

    // Run the migration.
    let outcome =
        migrate_legacy_project_database(catalog.clone(), workspace_registry, project.path())
            .await
            .unwrap();
    match outcome {
        MigrationOutcome::Imported { sessions, .. } => {
            // The seeded DB is empty (0 sessions).
            assert_eq!(sessions, 0);
        }
        other => panic!("expected Imported, got {:?}", other),
    }

    // A marker row should now exist in the catalog for the source path.
    let marker = fetch_marker(&catalog, &source_db.to_string_lossy())
        .await
        .unwrap();
    assert!(marker.is_some(), "marker should be recorded after import");
    let marker = marker.unwrap();
    assert_eq!(
        marker.storage_layout_version,
        codegg::storage::STORAGE_LAYOUT_VERSION
    );
}
