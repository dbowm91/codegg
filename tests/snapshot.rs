use codegg::snapshot::{FileSnapshot, SnapshotView, SnapshotManager};
use std::collections::HashMap;
use sqlx::SqlitePool;

async fn create_test_manager() -> SnapshotManager {
    let pool = SqlitePool::connect("sqlite::memory:").await.expect("failed to connect to memory db");
    // Run migrations
    codegg::session::schema::migrate(&pool).await.expect("failed to run migrations");
    
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    SnapshotManager::new(pool, temp_dir.path().to_path_buf())
}

#[tokio::test]
async fn test_snapshot_manager_new() {
    let manager = create_test_manager().await;
    assert_eq!(manager.list_for_session("any-session").await.unwrap().len(), 0);
}

#[tokio::test]
async fn test_snapshot_manager_get_nonexistent() {
    let manager = create_test_manager().await;
    assert!(manager.get("nonexistent-id").await.unwrap().is_none());
}

#[tokio::test]
async fn test_snapshot_manager_latest_none() {
    let manager = create_test_manager().await;
    assert!(manager.latest("any-session").await.unwrap().is_none());
}

#[tokio::test]
async fn test_snapshot_capture_empty_dir() {
    let (mut manager, pool) = create_test_manager_with_pool().await;
    
    // Create project and session
    sqlx::query("INSERT INTO project (id, worktree, sandboxes, time_created, time_updated) VALUES (?, ?, ?, ?, ?)")
        .bind("test-project")
        .bind(".")
        .bind("[]")
        .bind(0)
        .bind(0)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
        .bind("test-session")
        .bind("test-project")
        .bind("test-session")
        .bind(".")
        .bind("Test Session")
        .bind("1")
        .bind(0)
        .bind(0)
        .execute(&pool)
        .await
        .unwrap();

    let result = manager.capture("test-session", Some("test-label".to_string())).await;
    assert!(result.is_ok(), "Capture failed: {:?}", result.err());

    let snapshot = result.unwrap();
    assert_eq!(snapshot.session_id, "test-session");
    assert_eq!(snapshot.label, Some("test-label".to_string()));
    assert!(snapshot.id.len() > 0);
}

async fn create_test_manager_with_pool() -> (SnapshotManager, SqlitePool) {
    let pool = SqlitePool::connect("sqlite::memory:").await.expect("failed to connect to memory db");
    // Run migrations
    codegg::session::schema::migrate(&pool).await.expect("failed to run migrations");
    
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    (SnapshotManager::new(pool.clone(), temp_dir.path().to_path_buf()), pool)
}

#[test]
fn test_file_snapshot_creation() {
    let snapshot = FileSnapshot {
        path: "test/path.txt".to_string(),
        content: "file content".to_string(),
        hash: "abc123".to_string(),
        timestamp: 1234567890,
    };

    assert_eq!(snapshot.path, "test/path.txt");
    assert_eq!(snapshot.content, "file content");
    assert_eq!(snapshot.hash, "abc123");
}

#[test]
fn test_snapshot_view_creation() {
    let mut files = HashMap::new();
    files.insert(
        "test.txt".to_string(),
        FileSnapshot {
            path: "test.txt".to_string(),
            content: "content".to_string(),
            hash: "hash".to_string(),
            timestamp: 100,
        },
    );

    let snapshot = SnapshotView {
        id: "id".to_string(),
        session_id: "session".to_string(),
        files,
        created_at: 200,
        label: Some("label".to_string()),
    };

    assert_eq!(snapshot.id, "id");
    assert_eq!(snapshot.files.len(), 1);
}
