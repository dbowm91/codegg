//! Multi-project workspace isolation tests.
//!
//! Phase 2 of the single-daemon plan. These tests verify that two
//! workspaces registered with the same `CoreDaemon` maintain correct
//! isolation: session identity, workspace root, path resolution, and
//! that a malicious `workdir` cannot cross into another workspace.

use std::sync::Arc;

use codegg::session::schema::migrate;
use codegg::workspace::{
    ExecutionContext, InMemoryWorkspaceStore, PathPolicyError, WorkspaceRegistry,
};

async fn in_memory_pool() -> sqlx::SqlitePool {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    let url = format!(
        "file:workspace_iso_{}?mode=memory&cache=shared",
        uuid::Uuid::new_v4().simple()
    );
    let opts = SqliteConnectOptions::from_str(&url)
        .expect("valid sqlite options")
        .create_if_missing(true)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .unwrap();
    migrate(&pool).await.unwrap();
    pool
}

fn temp_workspace(label: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("marker.txt"), label).unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "// lib").unwrap();
    dir
}

#[tokio::test(flavor = "current_thread")]
async fn two_workspaces_register_and_resolve_independently() {
    let dir_a = temp_workspace("project_a");
    let dir_b = temp_workspace("project_b");

    let store = Arc::new(InMemoryWorkspaceStore::new());
    let registry = WorkspaceRegistry::load(store).await.unwrap();

    let rec_a = registry.get_or_register(dir_a.path()).await.unwrap();
    let rec_b = registry.get_or_register(dir_b.path()).await.unwrap();

    assert_ne!(
        rec_a.id, rec_b.id,
        "different directories get different workspace IDs"
    );
    assert_eq!(rec_a.canonical_root, dir_a.path().canonicalize().unwrap());
    assert_eq!(rec_b.canonical_root, dir_b.path().canonicalize().unwrap());

    // Verify the registry deduplicates aliases.
    let alias = dir_a.path().join("alias");
    std::os::unix::fs::symlink(dir_a.path(), &alias).unwrap();
    let alias_rec = registry.get_or_register(&alias).await.unwrap();
    assert_eq!(
        rec_a.id, alias_rec.id,
        "symlink alias deduplicates to same workspace"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn execution_context_isolation_between_workspaces() {
    let dir_a = temp_workspace("project_a");
    let dir_b = temp_workspace("project_b");

    let store = Arc::new(InMemoryWorkspaceStore::new());
    let registry = WorkspaceRegistry::load(store).await.unwrap();

    let rec_a = registry.get_or_register(dir_a.path()).await.unwrap();
    let rec_b = registry.get_or_register(dir_b.path()).await.unwrap();

    let ctx_a = ExecutionContext::new(rec_a.clone(), Some("session_a".into()), Default::default());
    let ctx_b = ExecutionContext::new(rec_b.clone(), Some("session_b".into()), Default::default());

    // Workspace roots differ.
    assert_ne!(ctx_a.workspace_root, ctx_b.workspace_root);

    // Relative paths resolve under the correct workspace root.
    let resolved_a = ctx_a
        .resolve_relative_cwd(Some(std::path::Path::new("src")))
        .unwrap();
    assert!(resolved_a.starts_with(&rec_a.canonical_root));
    assert!(resolved_a.ends_with("src"));

    let resolved_b = ctx_b
        .resolve_relative_cwd(Some(std::path::Path::new("src")))
        .unwrap();
    assert!(resolved_b.starts_with(&rec_b.canonical_root));
    assert!(resolved_b.ends_with("src"));
    assert_ne!(
        resolved_a, resolved_b,
        "src under different workspace roots resolves differently"
    );

    // A relative escape via `..` is rejected.
    let escape = std::path::Path::new("../other_workspace");
    assert!(matches!(
        ctx_a.resolve_relative_cwd(Some(escape)),
        Err(PathPolicyError::OutsideWorkspace(_, _))
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn session_runtime_deduplicates_across_workspace_registrations() {
    let dir = temp_workspace("project_a");

    let store = Arc::new(InMemoryWorkspaceStore::new());
    let registry = WorkspaceRegistry::load(store).await.unwrap();
    let rec = registry.get_or_register(dir.path()).await.unwrap();

    let sessions = codegg::core::session_runtime::SessionRuntimeRegistry::new();
    let rt1 = sessions.get_or_create(
        "session-1",
        rec.id.clone(),
        rec.canonical_root.clone(),
        "project_id".into(),
        dir.path().to_path_buf(),
    );
    let rt2 = sessions.get_or_create(
        "session-1",
        rec.id.clone(),
        rec.canonical_root.clone(),
        "project_id".into(),
        dir.path().to_path_buf(),
    );
    assert!(
        Arc::ptr_eq(&rt1, &rt2),
        "get_or_create deduplicates by session_id"
    );

    // Different session_id gets a different runtime.
    let rt3 = sessions.get_or_create(
        "session-2",
        rec.id.clone(),
        rec.canonical_root.clone(),
        "project_id".into(),
        dir.path().to_path_buf(),
    );
    assert!(
        !Arc::ptr_eq(&rt1, &rt3),
        "different session_id gets a different runtime"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn workspace_record_to_dto_roundtrip() {
    let dir = temp_workspace("project_a");
    let store = Arc::new(InMemoryWorkspaceStore::new());
    let registry = WorkspaceRegistry::load(store).await.unwrap();
    let rec = registry.get_or_register(dir.path()).await.unwrap();

    let dto = codegg::protocol_conversions::workspace_record_to_dto(&rec, 2);
    assert_eq!(dto.workspace_id, rec.id.as_str());
    assert_eq!(dto.canonical_root, rec.canonical_root.to_string_lossy());
    assert_eq!(dto.active_sessions, 2);

    // Verify the DTO round-trips through serde.
    let json = serde_json::to_string(&dto).unwrap();
    let dto2: codegg::protocol::dto::WorkspaceSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(dto2.workspace_id, dto.workspace_id);
    assert_eq!(dto2.canonical_root, dto.canonical_root);
}

#[tokio::test(flavor = "current_thread")]
async fn workspace_registry_persists_through_pool() {
    let pool = in_memory_pool().await;
    let store = Arc::new(codegg::workspace::SqliteWorkspaceStore::new(pool.clone()));

    let dir = temp_workspace("persist_test");

    // Register in the first registry instance.
    {
        let registry = WorkspaceRegistry::load(store.clone()).await.unwrap();
        let rec = registry.get_or_register(dir.path()).await.unwrap();
        // display_name is derived from the path's file_name, which is
        // the temp dir leaf (e.g. ".tmpXXXXX"), not the label.
        assert_eq!(rec.canonical_root, dir.path().canonicalize().unwrap());
    }

    // Load a new registry from the same store — the record survives.
    {
        let registry = WorkspaceRegistry::load(store).await.unwrap();
        let list = registry.list(true).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].canonical_root, dir.path().canonicalize().unwrap());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn core_daemon_workspace_binding_rejects_unbound_turn_submit() {
    // CoreDaemon::TurnSubmit now requires a resolvable session.
    // A TurnSubmit with a session_id that was never SessionCreated
    // should be rejected with session_unbound.
    std::env::set_var("OPENAI_API_KEY", "test-key");
    let pool = in_memory_pool().await;
    let deps = codegg::core::runtime_deps::CoreRuntimeDeps::new(Some(pool), None, None, None);
    let daemon = codegg::core::daemon::CoreDaemon::with_deps(deps);
    daemon.hydrate_workspace_registry().await.unwrap();

    let agent = codegg::protocol_conversions::agent_to_dto(codegg::agent::Agent {
        name: "test".into(),
        description: "test".into(),
        ..Default::default()
    });
    let req = codegg::protocol::core::CoreRequest::TurnSubmit {
        session_id: "never-created".into(),
        text: "hello".into(),
        plan_mode: false,
        model: "openai/gpt-4o".into(),
        agents: vec![agent],
        current_agent_idx: 0,
        messages: vec![],
    };
    let resp = daemon
        .handle_request(codegg::core::new_request("req-1".into(), req))
        .await
        .unwrap();
    match resp {
        codegg::protocol::core::CoreResponse::Error { code, message } => {
            assert_eq!(code, "session_unbound");
            assert!(
                message.contains("never-created"),
                "error message should include session_id: {}",
                message
            );
        }
        other => panic!("expected Error(session_unbound), got {:?}", other),
    }
}
